use reqwest::{Certificate, Url, blocking::Client, redirect::Policy};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, time::Duration};

use crate::{
    planner::InstallationPlan,
    registry::{RegistrySource, validate_relative_path},
};

pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn acquire_files(
    source: &RegistrySource,
    plan: &InstallationPlan,
) -> Result<Vec<Vec<u8>>, String> {
    acquire_files_with_root(source, plan, None)
}

fn acquire_files_with_root(
    source: &RegistrySource,
    plan: &InstallationPlan,
    test_root: Option<Certificate>,
) -> Result<Vec<Vec<u8>>, String> {
    let mut contents = Vec::with_capacity(plan.files.len());
    for file in &plan.files {
        let bytes = match source {
            RegistrySource::Local { root } => read_local_source(root, &file.source)?,
            RegistrySource::Remote { manifest_url } => {
                let source_path = validate_relative_path(&file.source, "source")?;
                let url = manifest_url
                    .join(&source_path)
                    .map_err(|error| format!("Invalid source URL for {}: {error}", file.source))?;
                if url.scheme() != "https"
                    || url.host_str() != manifest_url.host_str()
                    || url.port_or_known_default() != manifest_url.port_or_known_default()
                {
                    return Err(format!(
                        "Registry source escaped its HTTPS origin: {}",
                        file.source
                    ));
                }
                fetch_url_with_root(
                    &url,
                    MAX_FILE_BYTES,
                    &format!("file {}", file.source),
                    test_root.clone(),
                )?
                .0
            }
        };
        verify_sha256(&bytes, &file.expected_sha256, &file.source)?;
        contents.push(bytes);
    }
    Ok(contents)
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
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(format!(
            "{label} must use HTTPS without embedded credentials"
        ));
    }
    let redirect_policy = Policy::custom(|attempt| {
        if attempt.previous().len() >= 5
            || attempt.url().scheme() != "https"
            || attempt.url().host_str().is_none()
            || !attempt.url().username().is_empty()
            || attempt.url().password().is_some()
        {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    let mut builder = Client::builder();
    if let Some(root) = test_root {
        builder = builder.add_root_certificate(root);
    }
    let client = builder
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(redirect_policy)
        .build()
        .map_err(|error| format!("Cannot create HTTPS client: {error}"))?;
    let response = client.get(url.clone()).send().map_err(|error| {
        let reason = if error.is_timeout() {
            "request timed out"
        } else if error.is_connect() {
            "could not establish a connection"
        } else {
            "request failed"
        };
        format!("Could not download {label} from {url} ({reason}): {error}")
    })?;
    if !response.status().is_success() {
        return Err(format!(
            "Download of {label} failed with HTTP {} ({})",
            response.status(),
            url
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

fn read_bounded(mut reader: impl Read, max_bytes: usize, label: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut limited = reader.by_ref().take(max_bytes as u64 + 1);
    limited
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Incomplete download of {label}: {error}"))?;
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
        sync::Arc,
        thread,
    };

    fn https_fixture(
        status: &str,
        content_length: usize,
        headers: &str,
        body: &[u8],
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
        let status = status.to_string();
        let headers = headers.to_string();
        let body = body.to_vec();
        let handle = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let connection = ServerConnection::new(Arc::new(config)).unwrap();
            let mut stream = StreamOwned::new(connection, socket);
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
            }
            let response_headers = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {content_length}\r\nConnection: close\r\n{headers}\r\n"
            );
            stream.write_all(response_headers.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
            stream.flush().unwrap();
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
    fn acquires_remote_registry_files_from_the_manifest_origin_and_checks_the_digest() {
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
        let plan = InstallationPlan {
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
        let acquired = acquire_files_with_root(&source, &plan, Some(root)).unwrap();
        handle.join().unwrap();
        assert_eq!(acquired, [contents.to_vec()]);
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
}
