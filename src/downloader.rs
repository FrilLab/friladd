use reqwest::{Certificate, Url, blocking::Client, redirect::Policy};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, io::Read, path::Path, sync::Arc, time::Duration};

use crate::{
    planner::InstallationPlan,
    registry::{Registry, RegistrySource, validate_relative_path},
};

pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn acquire_files(
    source: &RegistrySource,
    plan: &InstallationPlan,
) -> Result<Vec<Arc<[u8]>>, String> {
    acquire_files_with_root(source, plan, None)
}

fn acquire_files_with_root(
    source: &RegistrySource,
    plan: &InstallationPlan,
    test_root: Option<Certificate>,
) -> Result<Vec<Arc<[u8]>>, String> {
    let mut contents = Vec::with_capacity(plan.files.len());
    let mut acquired = HashMap::<&str, (&str, Arc<[u8]>)>::new();
    for file in &plan.files {
        let source_path = file.source.as_str();
        let digest = file.expected_sha256.as_str();
        let bytes = match acquired.get(source_path) {
            Some((previous, bytes)) => {
                check_repeated_digest(source_path, previous, digest)?;
                Arc::clone(bytes)
            }
            None => {
                let bytes: Arc<[u8]> =
                    acquire_file(source, source_path, digest, test_root.clone())?.into();
                acquired.insert(source_path, (digest, Arc::clone(&bytes)));
                bytes
            }
        };
        contents.push(bytes);
    }
    Ok(contents)
}

/// Verify each unique source independently without retaining file contents.
pub fn verify_registry_files(
    source: &RegistrySource,
    registry: &Registry,
) -> Result<usize, String> {
    let mut seen = HashMap::<&str, &str>::new();
    let mut count = 0;
    for item in &registry.items {
        for file in &item.files {
            let result = match seen.get(file.source.as_str()) {
                Some(previous) => check_repeated_digest(&file.source, previous, &file.sha256),
                None => acquire_file(source, &file.source, &file.sha256, None).map(|_| {
                    seen.insert(&file.source, &file.sha256);
                }),
            };
            result.map_err(|error| format!("Item {}: {error}", item.name))?;
            count += 1;
        }
    }
    Ok(count)
}

fn check_repeated_digest(source: &str, previous: &str, expected: &str) -> Result<(), String> {
    if previous.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!("Conflicting SHA-256 digests for source {source}"))
    }
}

fn acquire_file(
    source: &RegistrySource,
    file_source: &str,
    expected_sha256: &str,
    test_root: Option<Certificate>,
) -> Result<Vec<u8>, String> {
    let bytes = match source {
        RegistrySource::Local { root } => read_local_source(root, file_source)?,
        RegistrySource::Remote { manifest_url } => {
            let source_path = validate_relative_path(file_source, "source")?;
            let url = manifest_url
                .join(&source_path)
                .map_err(|error| format!("Invalid source URL for {file_source}: {error}"))?;
            if url.scheme() != "https"
                || url.host_str() != manifest_url.host_str()
                || url.port_or_known_default() != manifest_url.port_or_known_default()
            {
                return Err(format!(
                    "Registry source escaped its HTTPS origin: {file_source}"
                ));
            }
            fetch_url_with_root(
                &url,
                MAX_FILE_BYTES,
                &format!("file {file_source}"),
                test_root,
            )?
            .0
        }
    };
    verify_sha256(&bytes, expected_sha256, file_source)?;
    Ok(bytes)
}

fn read_local_source(root: &Path, source: &str) -> Result<Vec<u8>, String> {
    let relative = validate_relative_path(source, "source")?;
    let root = fs::canonicalize(root)
        .map_err(|error| format!("Cannot resolve registry source directory: {error}"))?;
    let mut current = root.clone();
    let segments: Vec<&str> = relative.split('/').collect();
    for (index, segment) in segments.iter().enumerate() {
        current.push(segment);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("Cannot read registry source {}: {error}", relative))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("Registry source contains a symlink: {relative}"));
        }
        if index + 1 < segments.len() && !metadata.is_dir() {
            return Err(format!(
                "Registry source parent is not a directory: {relative}"
            ));
        }
        if index + 1 == segments.len() && !metadata.is_file() {
            return Err(format!("Registry source is not a regular file: {relative}"));
        }
        let canonical = fs::canonicalize(&current)
            .map_err(|error| format!("Cannot resolve registry source {}: {error}", relative))?;
        if !canonical.starts_with(&root) {
            return Err(format!(
                "Registry source escapes its manifest directory: {relative}"
            ));
        }
    }
    let file_handle = fs::File::open(&current)
        .map_err(|error| format!("Cannot open registry source {}: {error}", relative))?;
    read_bounded(file_handle, MAX_FILE_BYTES, &format!("file {relative}"))
}

pub fn fetch_url(url: &Url, max_bytes: usize, label: &str) -> Result<(Vec<u8>, Url), String> {
    fetch_url_with_root(url, max_bytes, label, None)
}

fn fetch_url_with_root(
    url: &Url,
    max_bytes: usize,
    label: &str,
    test_root: Option<Certificate>,
) -> Result<(Vec<u8>, Url), String> {
    fetch_url_with_timeout(url, max_bytes, label, test_root, REQUEST_TIMEOUT)
}

fn fetch_url_with_timeout(
    url: &Url,
    max_bytes: usize,
    label: &str,
    test_root: Option<Certificate>,
    request_timeout: Duration,
) -> Result<(Vec<u8>, Url), String> {
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(format!(
            "{label} must use HTTPS without embedded credentials"
        ));
    }
    let origin = url.origin();
    let redirect_policy = Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5
            || attempt.url().scheme() != "https"
            || attempt.url().origin() != origin
            || !attempt.url().username().is_empty()
            || attempt.url().password().is_some()
        {
            attempt.error("HTTPS redirect exceeded limit or violated same-origin policy")
        } else {
            attempt.follow()
        }
    });
    let mut builder = Client::builder();
    if let Some(root) = test_root {
        builder = builder.no_proxy().add_root_certificate(root);
    }
    let client = builder
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(request_timeout)
        .redirect(redirect_policy)
        .build()
        .map_err(|error| format!("Cannot create HTTPS client: {error}"))?;
    let response = client.get(url.clone()).send().map_err(|error| {
        let reason = if error.is_timeout() {
            "request timed out"
        } else if error.is_redirect() {
            "redirect rejected by HTTPS same-origin policy or redirect limit"
        } else if error.is_connect() {
            "could not establish a connection"
        } else {
            "request failed"
        };
        format!(
            "Could not download {label} from {} ({reason})",
            url.origin().ascii_serialization()
        )
    })?;
    if !response.status().is_success() {
        return Err(format!(
            "Download of {label} failed with HTTP {} ({})",
            response.status(),
            url.origin().ascii_serialization()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!(
            "Downloaded {label} exceeds the {max_bytes} byte limit"
        ));
    }
    let final_url = response.url().clone();
    let bytes = read_bounded(response, max_bytes, label)?;
    Ok((bytes, final_url))
}

pub fn read_bounded(
    mut reader: impl Read,
    max_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut limited = reader.by_ref().take(max_bytes as u64 + 1);
    limited
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Incomplete download or read of {label}: {}", error.kind()))?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "Downloaded {label} exceeds the {max_bytes} byte limit"
        ));
    }
    Ok(bytes)
}

fn verify_sha256(bytes: &[u8], expected: &str, source: &str) -> Result<(), String> {
    let digest = Sha256::digest(bytes);
    let actual = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "SHA-256 mismatch for {source}: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{self, Cursor, Write},
        net::TcpListener,
        thread,
    };

    fn https_fixture(
        status: &str,
        content_length: usize,
        headers: &str,
        body: &[u8],
    ) -> (Url, Certificate, thread::JoinHandle<()>) {
        https_sequence(&[(status, content_length, headers, body)])
    }

    fn https_sequence(
        responses: &[(&str, usize, &str, &[u8])],
    ) -> (Url, Certificate, thread::JoinHandle<()>) {
        use rustls::{
            ServerConfig, ServerConnection, StreamOwned,
            pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
        };

        let generated = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let certificate_der = generated.cert.der().clone();
        let client_root = Certificate::from_der(certificate_der.as_ref()).unwrap();
        let private_key =
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(generated.key_pair.serialize_der()));
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate_der], private_key)
            .unwrap();
        let listener = TcpListener::bind("localhost:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = Url::parse(&format!("https://localhost:{port}/fixture")).unwrap();
        let config = Arc::new(config);
        let responses: Vec<_> = responses.iter().map(|(status, length, headers, body)| {
            let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {length}\r\nConnection: close\r\n{headers}\r\n"
            );
            (headers, body.to_vec())
        }).collect();
        let handle = thread::spawn(move || {
            for (headers, body) in responses {
                let (socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let connection = ServerConnection::new(Arc::clone(&config)).unwrap();
                let mut stream = StreamOwned::new(connection, socket);
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert_ne!(count, 0, "expected an HTTP request");
                    request.extend_from_slice(&buffer[..count]);
                }
                stream.write_all(headers.as_bytes()).unwrap();
                stream.write_all(&body).unwrap();
                stream.flush().unwrap();
            }
        });
        (url, client_root, handle)
    }

    #[test]
    fn verifies_sha256_and_rejects_mismatch() {
        let hash = Sha256::digest(b"source")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(verify_sha256(b"source", &hash, "sample").is_ok());
        assert!(
            verify_sha256(b"changed", &hash, "sample")
                .unwrap_err()
                .contains("SHA-256 mismatch")
        );
    }

    #[test]
    fn enforces_size_limit_and_reports_interrupted_reads() {
        assert!(
            read_bounded(Cursor::new(b"1234"), 3, "fixture")
                .unwrap_err()
                .contains("limit")
        );
        struct BrokenReader;
        impl Read for BrokenReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "interrupted",
                ))
            }
        }
        assert!(
            read_bounded(BrokenReader, 10, "fixture")
                .unwrap_err()
                .contains("Incomplete download")
        );
    }

    #[test]
    fn refuses_plain_http_registries() {
        let url = Url::parse("http://example.com/registry.json").unwrap();
        assert!(
            fetch_url(&url, 100, "registry")
                .unwrap_err()
                .contains("HTTPS")
        );
    }

    #[test]
    fn downloads_from_https_fixture_and_rejects_http_status_and_size_overflow() {
        let (url, root, handle) = https_fixture("200 OK", 7, "", b"fixture");
        let (bytes, final_url) = fetch_url_with_root(&url, 20, "fixture", Some(root)).unwrap();
        handle.join().unwrap();
        assert_eq!(bytes, b"fixture");
        assert_eq!(final_url, url);

        let (url, root, handle) = https_fixture("503 Service Unavailable", 0, "", b"");
        assert!(
            fetch_url_with_root(&url, 20, "fixture", Some(root))
                .unwrap_err()
                .contains("HTTP 503")
        );
        handle.join().unwrap();

        let (url, root, handle) = https_fixture("200 OK", 7, "", b"fixture");
        assert!(
            fetch_url_with_root(&url, 3, "fixture", Some(root))
                .unwrap_err()
                .contains("limit")
        );
        handle.join().unwrap();
    }

    #[test]
    fn acquires_each_remote_source_once_and_reuses_verified_bytes() {
        use crate::planner::{InstallationPlan, PlannedFile};

        let contents = b"remote verified source";
        let expected_sha256 = Sha256::digest(contents)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let (mut manifest_url, root, handle) =
            https_fixture("200 OK", contents.len(), "", contents);
        manifest_url.set_path("/registry/registry.json");
        let source = RegistrySource::Remote { manifest_url };
        let project = tempfile::tempdir().unwrap();
        let mut plan = InstallationPlan {
            project_root: project.path().to_path_buf(),
            files: vec![PlannedFile {
                source: "fixtures/component.tsx".into(),
                target: "src/component.tsx".into(),
                target_display: "src/component.tsx".into(),
                destination: project.path().join("src/component.tsx"),
                expected_sha256,
                exists: false,
            }],
            conflicts: Vec::new(),
        };
        plan.files.push(PlannedFile {
            source: plan.files[0].source.clone(),
            target: "src/second.tsx".into(),
            target_display: "src/second.tsx".into(),
            destination: project.path().join("src/second.tsx"),
            expected_sha256: plan.files[0].expected_sha256.to_ascii_uppercase(),
            exists: false,
        });
        let acquired = acquire_files_with_root(&source, &plan, Some(root)).unwrap();
        handle.join().unwrap();
        assert_eq!(acquired[0].as_ref(), contents);
        assert!(Arc::ptr_eq(&acquired[0], &acquired[1]));
        assert!(!project.path().join("src").exists());
    }

    #[test]
    fn rejects_insecure_redirects_and_incomplete_https_responses() {
        let (url, root, handle) = https_fixture(
            "302 Found",
            0,
            "Location: http://example.invalid/blocked\r\n",
            b"",
        );
        assert!(fetch_url_with_root(&url, 20, "fixture", Some(root)).is_err());
        handle.join().unwrap();

        let (url, root, handle) = https_fixture("200 OK", 100, "", b"cut short");
        assert!(
            fetch_url_with_root(&url, 1_000, "fixture", Some(root))
                .unwrap_err()
                .contains("Incomplete download")
        );
        handle.join().unwrap();
    }

    #[test]
    fn follows_same_origin_https_redirect_and_returns_final_manifest_location() {
        let (url, root, handle) = https_sequence(&[
            (
                "302 Found",
                0,
                "Location: /published/registry.json\r\n",
                b"",
            ),
            ("200 OK", 7, "", b"fixture"),
        ]);
        let (bytes, final_url) = fetch_url_with_root(&url, 20, "fixture", Some(root)).unwrap();
        handle.join().unwrap();
        assert_eq!(bytes, b"fixture");
        assert_eq!(final_url, url.join("/published/registry.json").unwrap());
    }

    #[test]
    fn rejects_cross_origin_redirect_and_redacts_url_components() {
        let (mut url, root, handle) = https_fixture(
            "302 Found",
            0,
            "Location: https://localhost:1/redirect-secret?token=location-secret\r\n",
            b"",
        );
        url.set_path("/locator-secret");
        url.set_query(Some("token=query-secret"));
        let error = fetch_url_with_root(&url, 20, "fixture", Some(root)).unwrap_err();
        handle.join().unwrap();
        assert!(error.contains("same-origin"));
        assert!(!error.contains("secret"), "{error}");
    }

    #[test]
    fn times_out_when_the_https_peer_stops_responding() {
        let listener = TcpListener::bind("localhost:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, hold) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
            hold.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let url = Url::parse(&format!(
            "https://localhost:{port}/registry?token=query-secret"
        ))
        .unwrap();
        let result = fetch_url_with_timeout(&url, 20, "fixture", None, Duration::from_millis(500));
        release.send(()).unwrap();
        handle.join().unwrap();
        let error = result.unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(!error.contains("query-secret"), "{error}");
    }
}
