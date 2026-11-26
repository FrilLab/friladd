use reqwest::{Certificate, Url, blocking::Client, redirect::Policy};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::{
    planner::InstallationPlan,
    registry::{Registry, RegistrySource, validate_relative_path},
};

pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_OPERATION_BYTES: u64 = 512 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub fn acquire_files(
    source: &RegistrySource,
    plan: &InstallationPlan,
) -> Result<Vec<AcquiredFile>, String> {
    acquire_files_with_policy(source, plan, None, true)
}

pub fn acquire_v2_files(
    source: &RegistrySource,
    plan: &InstallationPlan,
) -> Result<Vec<AcquiredFile>, String> {
    acquire_files_with_policy(source, plan, None, false)
}

#[cfg(test)]
fn acquire_files_with_root(
    source: &RegistrySource,
    plan: &InstallationPlan,
    test_root: Option<Certificate>,
) -> Result<Vec<AcquiredFile>, String> {
    acquire_files_with_policy(source, plan, test_root, true)
}

fn acquire_files_with_policy(
    source: &RegistrySource,
    plan: &InstallationPlan,
    test_root: Option<Certificate>,
    allow_suffix: bool,
) -> Result<Vec<AcquiredFile>, String> {
    let mut contents = Vec::with_capacity(plan.files.len());
    let mut acquired = HashMap::<&str, (&str, Arc<[u8]>, Option<String>)>::new();
    let mut lengths = Vec::with_capacity(plan.files.len());
    for file in &plan.files {
        let source_path = file.source.as_str();
        let digest = file.expected_sha256.as_str();
        let bytes = match acquired.get(source_path) {
            Some((previous, bytes, resolved)) => {
                check_repeated_digest(source_path, previous, digest)?;
                (Arc::clone(bytes), resolved.clone())
            }
            None => {
                let (bytes, resolved) = acquire_file_with_policy(
                    source,
                    source_path,
                    digest,
                    test_root.clone(),
                    allow_suffix,
                )?;
                let bytes: Arc<[u8]> = bytes.into();
                acquired.insert(source_path, (digest, Arc::clone(&bytes), resolved.clone()));
                (bytes, resolved)
            }
        };
        lengths.push(bytes.0.len() as u64);
        contents.push(AcquiredFile {
            bytes: bytes.0,
            resolved_locator: bytes.1,
        });
    }
    check_staged_bytes(lengths)?;
    Ok(contents)
}

fn check_staged_bytes(lengths: impl IntoIterator<Item = u64>) -> Result<(), String> {
    let mut total = 0_u64;
    for length in lengths {
        total = total
            .checked_add(length)
            .ok_or("Transaction exceeds the 512 MiB byte limit for staged files")?;
        if total > MAX_OPERATION_BYTES {
            return Err("Transaction exceeds the 512 MiB byte limit for staged files".into());
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct AcquiredFile {
    pub bytes: Arc<[u8]>,
    pub resolved_locator: Option<String>,
}

/// Reacquire one exact lockfile locator and verify both its recorded digest and size.
pub fn acquire_locked_file(
    locator: &str,
    expected_sha256: &str,
    expected_size: u64,
) -> Result<Arc<[u8]>, String> {
    let url = Url::parse(locator).map_err(|_| "Invalid locked artifact locator")?;
    let bytes = match url.scheme() {
        "file" => {
            let path = url
                .to_file_path()
                .map_err(|_| "Invalid locked local artifact path")?;
            read_local_file_url(&path)?
        }
        "https" => {
            fetch_url_with_policy(
                &url,
                MAX_FILE_BYTES,
                "locked artifact",
                None,
                REQUEST_TIMEOUT,
                false,
            )?
            .0
        }
        _ => return Err("Locked artifacts must use file or HTTPS locators".into()),
    };
    verify_sha256(&bytes, expected_sha256, "locked artifact")?;
    if bytes.len() as u64 != expected_size {
        return Err(format!(
            "Locked artifact size mismatch: expected {expected_size}, got {}",
            bytes.len()
        ));
    }
    Ok(bytes.into())
}

fn read_local_file_url(path: &Path) -> Result<Vec<u8>, String> {
    if !path.is_absolute() {
        return Err("Locked local artifact path must be absolute".into());
    }
    let mut current = PathBuf::new();
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir => current.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err("Locked local artifact path contains a parent component".into());
            }
            std::path::Component::Normal(name) => names.push(name.to_os_string()),
        }
    }
    for (index, name) in names.iter().enumerate() {
        current.push(name);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("Cannot inspect locked artifact: {error}"))?;
        if metadata.file_type().is_symlink()
            || (index + 1 < names.len() && !metadata.is_dir())
            || (index + 1 == names.len() && !metadata.is_file())
        {
            return Err("Locked local artifact path contains a symlink or non-file".into());
        }
    }
    let mut file = fs::File::open(&current)
        .map_err(|error| format!("Cannot open locked local artifact: {error}"))?;
    read_bounded(&mut file, MAX_FILE_BYTES, "locked artifact")
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

pub fn verify_v2_files(
    source: &RegistrySource,
    registry: &crate::registry_v2::RegistryV2,
) -> Result<usize, String> {
    let mut seen = std::collections::HashSet::new();
    let mut count = 0;
    for item in &registry.items {
        for file in &item.files {
            if seen.insert(&file.source) {
                let (bytes, _) =
                    acquire_file_with_policy(source, &file.source, &file.sha256, None, false)?;
                if bytes.len() as u64 != file.size {
                    return Err(format!(
                        "Size mismatch for {}: expected {}, got {}",
                        file.source,
                        file.size,
                        bytes.len()
                    ));
                }
            }
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
    acquire_file_with_policy(source, file_source, expected_sha256, test_root, true)
        .map(|(bytes, _)| bytes)
}

fn acquire_file_with_policy(
    source: &RegistrySource,
    file_source: &str,
    expected_sha256: &str,
    test_root: Option<Certificate>,
    allow_suffix: bool,
) -> Result<(Vec<u8>, Option<String>), String> {
    let (bytes, resolved) = match source {
        RegistrySource::Local { root } => (read_local_source(root, file_source)?, None),
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
            let (bytes, final_url) = fetch_url_with_policy(
                &url,
                MAX_FILE_BYTES,
                &format!("file {file_source}"),
                test_root,
                REQUEST_TIMEOUT,
                allow_suffix,
            )?;
            let mut final_url = final_url;
            final_url.set_query(None);
            final_url.set_fragment(None);
            final_url
                .set_username("")
                .map_err(|_| "Invalid resolved artifact locator")?;
            final_url
                .set_password(None)
                .map_err(|_| "Invalid resolved artifact locator")?;
            (bytes, Some(final_url.to_string()))
        }
    };
    verify_sha256(&bytes, expected_sha256, file_source)?;
    Ok((bytes, resolved))
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
    fetch_url_with_policy(url, max_bytes, label, test_root, REQUEST_TIMEOUT, true)
}

fn fetch_url_with_policy(
    url: &Url,
    max_bytes: usize,
    label: &str,
    test_root: Option<Certificate>,
    request_timeout: Duration,
    allow_suffix: bool,
) -> Result<(Vec<u8>, Url), String> {
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || (!allow_suffix && (url.query().is_some() || url.fragment().is_some()))
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
            || (!allow_suffix
                && (attempt.url().query().is_some() || attempt.url().fragment().is_some()))
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

    #[test]
    fn rejects_fanout_over_operation_byte_budget_before_staging_copies() {
        let err = check_staged_bytes(std::iter::repeat_n(MAX_FILE_BYTES as u64, 52)).unwrap_err();
        assert!(err.contains("512 MiB"));
        assert!(check_staged_bytes(std::iter::repeat_n(MAX_FILE_BYTES as u64, 51)).is_ok());
    }

    #[test]
    fn reacquires_locked_file_locator_with_exact_hash_and_size() {
        let directory = tempfile::tempdir().unwrap();
        let artifact = directory.path().join("model.bin");
        let bytes = b"\0model\xffbytes";
        fs::write(&artifact, bytes).unwrap();
        let locator = Url::from_file_path(fs::canonicalize(&artifact).unwrap())
            .unwrap()
            .to_string();
        let digest = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let acquired = acquire_locked_file(&locator, &digest, bytes.len() as u64).unwrap();
        assert_eq!(acquired.as_ref(), bytes);
        assert!(acquire_locked_file(&locator, &digest, 1).is_err());
        assert!(acquire_locked_file(&locator, &"0".repeat(64), bytes.len() as u64).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn locked_local_artifact_rejects_symlink_paths() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let actual = directory.path().join("actual.bin");
        let linked = directory.path().join("linked.bin");
        fs::write(&actual, b"asset").unwrap();
        symlink(&actual, &linked).unwrap();
        let locator = Url::from_file_path(&linked).unwrap().to_string();
        assert!(acquire_locked_file(&locator, &"0".repeat(64), 5).is_err());
    }
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
        assert_eq!(acquired[0].bytes.as_ref(), contents);
        assert!(Arc::ptr_eq(&acquired[0].bytes, &acquired[1].bytes));
        assert!(!project.path().join("src").exists());
    }

    #[test]
    fn reports_final_same_origin_artifact_locator() {
        let contents = b"asset";
        let digest = Sha256::digest(contents)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let (base, root, handle) = https_sequence(&[
            ("302 Found", 0, "Location: /assets/asset\r\n", b""),
            ("200 OK", contents.len(), "", contents),
        ]);
        let source = RegistrySource::Remote { manifest_url: base };
        let result =
            acquire_file_with_policy(&source, "asset", &digest, Some(root), false).unwrap();
        handle.join().unwrap();
        assert_eq!(result.0, contents);
        let locator = result.1.unwrap();
        assert!(locator.starts_with("https://localhost:"));
        assert!(locator.ends_with("/assets/asset"));
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
    fn rejects_query_suffixes_on_v2_artifact_redirects() {
        let (url, root, handle) = https_fixture(
            "302 Found",
            0,
            "Location: /file?signature=private-value\r\n",
            b"",
        );
        let error = fetch_url_with_policy(&url, 20, "fixture", Some(root), REQUEST_TIMEOUT, false)
            .unwrap_err();
        handle.join().unwrap();
        assert!(error.contains("redirect rejected"), "{error}");
        assert!(!error.contains("private-value"), "{error}");
    }

    #[test]
    fn v2_file_acquisition_rejects_query_redirects() {
        let (mut manifest_url, root, handle) = https_fixture(
            "302 Found",
            0,
            "Location: /artifact.bin?signature=private-value\r\n",
            b"",
        );
        manifest_url.set_path("/registry.json");
        let source = RegistrySource::Remote { manifest_url };
        let project = tempfile::tempdir().unwrap();
        let plan = InstallationPlan {
            project_root: project.path().to_path_buf(),
            files: vec![crate::planner::PlannedFile {
                source: "artifact.bin".into(),
                target: "artifact.bin".into(),
                target_display: "artifact.bin".into(),
                destination: project.path().join("artifact.bin"),
                expected_sha256: "0".repeat(64),
                exists: false,
            }],
            conflicts: Vec::new(),
        };
        let error = acquire_files_with_policy(&source, &plan, Some(root), false).unwrap_err();
        handle.join().unwrap();
        assert!(error.contains("redirect rejected"), "{error}");
        assert!(!error.contains("private-value"), "{error}");
        assert!(!project.path().join("artifact.bin").exists());
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
        let result =
            fetch_url_with_policy(&url, 20, "fixture", None, Duration::from_millis(500), true);
        release.send(()).unwrap();
        handle.join().unwrap();
        let error = result.unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(!error.contains("query-secret"), "{error}");
    }
}
