use reqwest::{Certificate, Url, blocking::Client, redirect::Policy};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::{Builder as TempBuilder, TempDir};

use crate::{
    planner::InstallationPlan,
    registry::{Registry, RegistrySource, validate_relative_path},
};

pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_V2_FILE_BYTES: usize = 100 * 1024 * 1024;
pub const MAX_OPERATION_BYTES: u64 = 512 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const ARTIFACT_TIMEOUT: Duration = Duration::from_secs(300);

pub fn acquire_files(
    source: &RegistrySource,
    plan: &InstallationPlan,
) -> Result<AcquiredFiles, String> {
    acquire_files_with_policy(source, plan, None, true, None)
}

pub fn acquire_v2_files_with_sizes(
    source: &RegistrySource,
    plan: &InstallationPlan,
    expected_sizes: &[u64],
) -> Result<AcquiredFiles, String> {
    if expected_sizes.len() != plan.files.len() {
        return Err("Registry v2 size declarations do not match the installation plan".into());
    }
    acquire_files_with_policy(source, plan, None, false, Some(expected_sizes))
}

#[cfg(test)]
fn acquire_files_with_root(
    source: &RegistrySource,
    plan: &InstallationPlan,
    test_root: Option<Certificate>,
) -> Result<AcquiredFiles, String> {
    acquire_files_with_policy(source, plan, test_root, true, None)
}

#[cfg(test)]
fn acquire_v2_files_with_root(
    source: &RegistrySource,
    plan: &InstallationPlan,
    expected_sizes: &[u64],
    test_root: Option<Certificate>,
) -> Result<AcquiredFiles, String> {
    acquire_files_with_policy(source, plan, test_root, false, Some(expected_sizes))
}

fn acquire_files_with_policy(
    source: &RegistrySource,
    plan: &InstallationPlan,
    test_root: Option<Certificate>,
    allow_suffix: bool,
    expected_sizes: Option<&[u64]>,
) -> Result<AcquiredFiles, String> {
    let scratch = tempfile::tempdir()
        .map_err(|error| format!("Cannot create download staging directory: {error}"))?;
    let client = build_stream_client(test_root, allow_suffix, ARTIFACT_TIMEOUT)?;
    let mut contents = Vec::with_capacity(plan.files.len());
    let mut acquired = HashMap::<&str, (&str, AcquiredFile)>::new();
    let mut total = 0_u64;
    for (index, file) in plan.files.iter().enumerate() {
        let source_path = file.source.as_str();
        let digest = file.expected_sha256.as_str();
        let expected_size = expected_sizes.map(|sizes| sizes[index]);
        let repeated_source = acquired.contains_key(source_path);
        if let Some(size) = expected_size {
            total = reserve_staged_bytes(total, size)?;
        }
        let artifact = match acquired.get(source_path) {
            Some((previous, artifact)) => {
                check_repeated_digest(source_path, previous, digest)?;
                if expected_size.is_some_and(|size| size != artifact.size) {
                    return Err(format!("Size mismatch for {source_path}"));
                }
                artifact.clone()
            }
            None => {
                let artifact = acquire_to_stage(
                    &client,
                    scratch.path(),
                    source,
                    source_path,
                    digest,
                    StreamOptions {
                        allow_suffix,
                        max_bytes: if allow_suffix {
                            MAX_FILE_BYTES
                        } else {
                            MAX_V2_FILE_BYTES
                        },
                        expected_size,
                    },
                )?;
                if expected_size.is_none() {
                    total = reserve_staged_bytes(total, artifact.size)?;
                }
                acquired.insert(source_path, (digest, artifact.clone()));
                artifact
            }
        };
        if expected_size.is_none() && repeated_source {
            total = reserve_staged_bytes(total, artifact.size)?;
        }
        contents.push(artifact);
    }
    Ok(AcquiredFiles {
        files: contents,
        _scratch: scratch,
    })
}

fn reserve_staged_bytes(current: u64, length: u64) -> Result<u64, String> {
    let total = current
        .checked_add(length)
        .ok_or("Transaction exceeds the 512 MiB byte limit for staged files")?;
    if total > MAX_OPERATION_BYTES {
        return Err("Transaction exceeds the 512 MiB byte limit for staged files".into());
    }
    Ok(total)
}

#[derive(Debug)]
pub struct AcquiredFiles {
    pub files: Vec<AcquiredFile>,
    _scratch: TempDir,
}

#[derive(Clone, Debug)]
pub struct AcquiredFile {
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
    pub resolved_locator: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LockedArtifact {
    pub locator: String,
    pub expected_sha256: String,
    pub expected_size: u64,
    pub max_bytes: usize,
}

#[derive(Clone, Copy)]
struct StreamOptions {
    allow_suffix: bool,
    max_bytes: usize,
    expected_size: Option<u64>,
}

fn build_stream_client(
    test_root: Option<Certificate>,
    allow_suffix: bool,
    timeout: Duration,
) -> Result<Client, String> {
    let redirect_policy = Policy::custom(move |attempt| {
        let origin = attempt
            .previous()
            .first()
            .map_or_else(|| attempt.url().origin(), Url::origin);
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
    let mut builder = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(timeout)
        .redirect(redirect_policy);
    if let Some(root) = test_root {
        builder = builder.no_proxy().add_root_certificate(root);
    }
    builder
        .build()
        .map_err(|_| "Cannot create HTTPS client".to_string())
}

fn acquire_to_stage(
    client: &Client,
    scratch: &Path,
    source: &RegistrySource,
    file_source: &str,
    expected_sha256: &str,
    options: StreamOptions,
) -> Result<AcquiredFile, String> {
    let StreamOptions {
        allow_suffix,
        max_bytes,
        expected_size,
    } = options;
    let mut staged = TempBuilder::new()
        .prefix("friladd-artifact-")
        .tempfile_in(scratch)
        .map_err(|error| format!("Cannot create artifact staging file: {error}"))?;
    let resolved_locator = match source {
        RegistrySource::Local { root } => {
            let path = local_source_path(root, file_source)?;
            let mut input = fs::File::open(&path)
                .map_err(|error| format!("Cannot open registry source {file_source}: {error}"))?;
            let (size, sha256) = copy_verified(
                &mut input,
                staged.as_file_mut(),
                max_bytes,
                expected_sha256,
                expected_size,
                &format!("file {file_source}"),
            )?;
            staged
                .as_file()
                .sync_all()
                .map_err(|error| format!("Cannot sync staged artifact: {error}"))?;
            let path = staged
                .into_temp_path()
                .keep()
                .map_err(|error| format!("Cannot retain staged artifact: {error}"))?;
            return Ok(AcquiredFile {
                path,
                size,
                sha256,
                resolved_locator: None,
            });
        }
        RegistrySource::Remote { manifest_url } => {
            let source_path = validate_relative_path(file_source, "source")?;
            let url = manifest_url
                .join(&source_path)
                .map_err(|_| format!("Invalid source URL for {file_source}"))?;
            if url.scheme() != "https"
                || url.host_str() != manifest_url.host_str()
                || url.port_or_known_default() != manifest_url.port_or_known_default()
            {
                return Err(format!(
                    "Registry source escaped its HTTPS origin: {file_source}"
                ));
            }
            let mut response = client.get(url.clone()).send().map_err(|error| {
                let reason = if error.is_timeout() {
                    "request timed out"
                } else if error.is_connect() {
                    "could not establish a connection"
                } else if error.is_redirect() {
                    "redirect rejected by HTTPS same-origin policy or redirect limit"
                } else {
                    "request failed"
                };
                format!(
                    "Could not download file {file_source} from {} ({reason})",
                    url.origin().ascii_serialization()
                )
            })?;
            if !response.status().is_success() {
                return Err(format!(
                    "Download of file {file_source} failed with HTTP {} ({})",
                    response.status(),
                    url.origin().ascii_serialization()
                ));
            }
            if response
                .content_length()
                .is_some_and(|size| size > max_bytes as u64)
            {
                return Err(format!(
                    "Downloaded file {file_source} exceeds the {max_bytes} byte limit"
                ));
            }
            let mut final_url = response.url().clone();
            if !allow_suffix && (final_url.query().is_some() || final_url.fragment().is_some()) {
                return Err(
                    "Registry v2 artifact redirects cannot contain a query or fragment".into(),
                );
            }
            final_url.set_query(None);
            final_url.set_fragment(None);
            final_url
                .set_username("")
                .map_err(|_| "Invalid resolved artifact locator")?;
            final_url
                .set_password(None)
                .map_err(|_| "Invalid resolved artifact locator")?;
            let (size, sha256) = copy_verified(
                &mut response,
                staged.as_file_mut(),
                max_bytes,
                expected_sha256,
                expected_size,
                &format!("file {file_source}"),
            )?;
            staged
                .as_file()
                .sync_all()
                .map_err(|error| format!("Cannot sync staged artifact: {error}"))?;
            Some(final_url.to_string()).map(|locator| (locator, size, sha256))
        }
    };
    let (resolved_locator, size, sha256) =
        resolved_locator.ok_or("Internal error: remote artifact has no resolved locator")?;
    let path = staged
        .into_temp_path()
        .keep()
        .map_err(|error| format!("Cannot retain staged artifact: {error}"))?;
    Ok(AcquiredFile {
        path,
        size,
        sha256,
        resolved_locator: Some(resolved_locator),
    })
}

fn copy_verified(
    input: &mut impl Read,
    output: &mut impl Write,
    max_bytes: usize,
    expected_sha256: &str,
    expected_size: Option<u64>,
    label: &str,
) -> Result<(u64, String), String> {
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| format!("Incomplete download or read of {label}: {}", error.kind()))?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| format!("Downloaded {label} exceeds the {max_bytes} byte limit"))?;
        if size > max_bytes as u64 {
            return Err(format!(
                "Downloaded {label} exceeds the {max_bytes} byte limit"
            ));
        }
        if expected_size.is_some_and(|expected| size > expected) {
            return Err(format!(
                "Size mismatch for {label}: expected {}, received more bytes",
                expected_size.unwrap_or_default()
            ));
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("Cannot stage {label}: {}", error.kind()))?;
    }
    if expected_size.is_some_and(|expected| size != expected) {
        return Err(format!(
            "Size mismatch for {label}: expected {}, got {size}",
            expected_size.unwrap_or_default()
        ));
    }
    let digest = format!("{:x}", hasher.finalize());
    if !digest.eq_ignore_ascii_case(expected_sha256) {
        return Err(format!(
            "SHA-256 mismatch for {label}: expected {expected_sha256}, got {digest}"
        ));
    }
    Ok((size, digest))
}

fn acquire_locked_to_stage(
    client: &Client,
    scratch: &Path,
    request: &LockedArtifact,
) -> Result<AcquiredFile, String> {
    let locator = Url::parse(&request.locator).map_err(|_| "Invalid locked artifact locator")?;
    let mut staged = TempBuilder::new()
        .prefix("friladd-restore-")
        .tempfile_in(scratch)
        .map_err(|error| format!("Cannot create restore staging file: {error}"))?;
    let (resolved_locator, size, sha256) = match locator.scheme() {
        "file" => {
            let path = local_file_url_path(&locator)?;
            let mut input = fs::File::open(path)
                .map_err(|error| format!("Cannot open locked local artifact: {error}"))?;
            let (size, hash) = copy_verified(
                &mut input,
                staged.as_file_mut(),
                request.max_bytes,
                &request.expected_sha256,
                Some(request.expected_size),
                "locked artifact",
            )?;
            (None, size, hash)
        }
        "https" => {
            if locator.host_str().is_none()
                || !locator.username().is_empty()
                || locator.password().is_some()
                || locator.query().is_some()
                || locator.fragment().is_some()
            {
                return Err(
                    "Locked HTTPS artifact locator contains credentials or a suffix".into(),
                );
            }
            let mut response = client.get(locator.clone()).send().map_err(|error| {
                let reason = if error.is_timeout() {
                    "request timed out"
                } else if error.is_connect() {
                    "could not establish a connection"
                } else if error.is_redirect() {
                    "redirect rejected by HTTPS same-origin policy or redirect limit"
                } else {
                    "request failed"
                };
                format!(
                    "Could not restore locked artifact from {} ({reason})",
                    locator.origin().ascii_serialization()
                )
            })?;
            if !response.status().is_success() {
                return Err(format!(
                    "Locked artifact request failed with HTTP {} ({})",
                    response.status(),
                    locator.origin().ascii_serialization()
                ));
            }
            if response
                .content_length()
                .is_some_and(|length| length > request.max_bytes as u64)
            {
                return Err(format!(
                    "Locked artifact exceeds the {} byte limit",
                    request.max_bytes
                ));
            }
            let mut final_url = response.url().clone();
            final_url.set_query(None);
            final_url.set_fragment(None);
            let (size, hash) = copy_verified(
                &mut response,
                staged.as_file_mut(),
                request.max_bytes,
                &request.expected_sha256,
                Some(request.expected_size),
                "locked artifact",
            )?;
            (Some(final_url.to_string()), size, hash)
        }
        _ => return Err("Locked artifacts must use file or HTTPS locators".into()),
    };
    staged
        .as_file()
        .sync_all()
        .map_err(|error| format!("Cannot sync restored artifact: {error}"))?;
    let path = staged
        .into_temp_path()
        .keep()
        .map_err(|error| format!("Cannot retain restored artifact: {error}"))?;
    Ok(AcquiredFile {
        path,
        size,
        sha256,
        resolved_locator,
    })
}

fn local_file_url_path(url: &Url) -> Result<PathBuf, String> {
    let path = url
        .to_file_path()
        .map_err(|_| "Invalid locked local artifact path")?;
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
    Ok(current)
}

/// Reacquire exact lockfile locators into bounded staging files before lifecycle transactions.
pub fn acquire_locked_files(requests: &[LockedArtifact]) -> Result<AcquiredFiles, String> {
    let scratch = tempfile::tempdir()
        .map_err(|error| format!("Cannot create restore staging directory: {error}"))?;
    let mut total = 0_u64;
    for request in requests {
        if request.expected_size > request.max_bytes as u64 {
            return Err("Locked artifact exceeds its per-file byte limit".into());
        }
        total = reserve_staged_bytes(total, request.expected_size)?;
    }
    let client = build_stream_client(None, false, ARTIFACT_TIMEOUT)?;
    let mut cache = HashMap::<(String, String, u64, usize), AcquiredFile>::new();
    let mut files = Vec::with_capacity(requests.len());
    for request in requests {
        let key = (
            request.locator.clone(),
            request.expected_sha256.clone(),
            request.expected_size,
            request.max_bytes,
        );
        let file = if let Some(file) = cache.get(&key) {
            file.clone()
        } else {
            let file = acquire_locked_to_stage(&client, scratch.path(), request)?;
            cache.insert(key, file.clone());
            file
        };
        files.push(file);
    }
    Ok(AcquiredFiles {
        files,
        _scratch: scratch,
    })
}

/// Verify each unique source independently without retaining file contents.
pub fn verify_registry_files(
    source: &RegistrySource,
    registry: &Registry,
) -> Result<usize, String> {
    let mut seen = HashMap::<&str, &str>::new();
    let mut count = 0;
    let mut total = 0_u64;
    let scratch = tempfile::tempdir()
        .map_err(|error| format!("Cannot create verification staging directory: {error}"))?;
    let client = build_stream_client(None, true, ARTIFACT_TIMEOUT)?;
    for item in &registry.items {
        for file in &item.files {
            let result = match seen.get(file.source.as_str()) {
                Some(previous) => check_repeated_digest(&file.source, previous, &file.sha256),
                None => acquire_to_stage(
                    &client,
                    scratch.path(),
                    source,
                    &file.source,
                    &file.sha256,
                    StreamOptions {
                        allow_suffix: true,
                        max_bytes: MAX_FILE_BYTES,
                        expected_size: None,
                    },
                )
                .and_then(|artifact| {
                    total = reserve_staged_bytes(total, artifact.size)?;
                    seen.insert(&file.source, &file.sha256);
                    fs::remove_file(artifact.path).map_err(|error| {
                        format!("Cannot remove verification staging file: {error}")
                    })
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
    let mut total = 0_u64;
    let scratch = tempfile::tempdir()
        .map_err(|error| format!("Cannot create verification staging directory: {error}"))?;
    let client = build_stream_client(None, false, ARTIFACT_TIMEOUT)?;
    for item in &registry.items {
        for file in &item.files {
            if seen.insert(&file.source) {
                total = reserve_staged_bytes(total, file.size)?;
                let artifact = acquire_to_stage(
                    &client,
                    scratch.path(),
                    source,
                    &file.source,
                    &file.sha256,
                    StreamOptions {
                        allow_suffix: false,
                        max_bytes: MAX_V2_FILE_BYTES,
                        expected_size: Some(file.size),
                    },
                )?;
                fs::remove_file(artifact.path)
                    .map_err(|error| format!("Cannot remove verification staging file: {error}"))?;
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

fn local_source_path(root: &Path, source: &str) -> Result<PathBuf, String> {
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
    Ok(current)
}

pub fn fetch_url(url: &Url, max_bytes: usize, label: &str) -> Result<(Vec<u8>, Url), String> {
    fetch_url_with_root(url, max_bytes, label, None)
}

pub(crate) fn fetch_url_with_root(
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

#[cfg(test)]
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
    use std::sync::Arc;

    #[test]
    fn rejects_fanout_over_operation_byte_budget_before_staging_copies() {
        let err = std::iter::repeat_n(MAX_FILE_BYTES as u64, 52)
            .try_fold(0, reserve_staged_bytes)
            .unwrap_err();
        assert!(err.contains("512 MiB"));
        assert!(
            std::iter::repeat_n(MAX_FILE_BYTES as u64, 51)
                .try_fold(0, reserve_staged_bytes)
                .is_ok()
        );
    }

    #[test]
    fn streams_chunked_binary_with_incremental_hash_and_exact_size() {
        struct Chunked<'a>(&'a [u8]);
        impl Read for Chunked<'_> {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                let count = output.len().min(self.0.len()).min(7_919);
                output[..count].copy_from_slice(&self.0[..count]);
                self.0 = &self.0[count..];
                Ok(count)
            }
        }

        let bytes: Vec<_> = (0..256 * 1024).map(|index| (index % 251) as u8).collect();
        let expected = format!("{:x}", Sha256::digest(&bytes));
        let mut output = Vec::new();
        let (size, digest) = copy_verified(
            &mut Chunked(&bytes),
            &mut output,
            bytes.len(),
            &expected,
            Some(bytes.len() as u64),
            "fixture",
        )
        .unwrap();
        assert_eq!(size, bytes.len() as u64);
        assert_eq!(digest, expected);
        assert_eq!(output, bytes);
    }

    #[test]
    fn streamed_copy_rejects_truncated_and_oversized_bodies() {
        let mut truncated = Vec::new();
        let short = copy_verified(
            &mut &b"short"[..],
            &mut truncated,
            10,
            &format!("{:x}", Sha256::digest(b"short")),
            Some(6),
            "fixture",
        )
        .unwrap_err();
        assert!(short.contains("Size mismatch"));

        let mut oversized = Vec::new();
        let large = copy_verified(
            &mut &b"too long"[..],
            &mut oversized,
            4,
            &format!("{:x}", Sha256::digest(b"too long")),
            None,
            "fixture",
        )
        .unwrap_err();
        assert!(large.contains("byte limit"));
    }

    #[test]
    fn streamed_copy_reports_staging_write_failures() {
        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "staging is unavailable",
                ))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let error = copy_verified(
            &mut &b"asset"[..],
            &mut BrokenWriter,
            10,
            &format!("{:x}", Sha256::digest(b"asset")),
            None,
            "fixture",
        )
        .unwrap_err();
        assert!(error.contains("Cannot stage fixture"));
    }

    #[test]
    fn stages_locked_file_locator_with_exact_hash_and_size() {
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
        let request = LockedArtifact {
            locator: locator.clone(),
            expected_sha256: digest.clone(),
            expected_size: bytes.len() as u64,
            max_bytes: MAX_FILE_BYTES,
        };
        let acquired = acquire_locked_files(std::slice::from_ref(&request)).unwrap();
        assert_eq!(fs::read(&acquired.files[0].path).unwrap(), bytes);
        assert!(
            acquire_locked_files(&[LockedArtifact {
                expected_size: 1,
                ..request.clone()
            }])
            .is_err()
        );
        assert!(
            acquire_locked_files(&[LockedArtifact {
                expected_sha256: "0".repeat(64),
                ..request
            }])
            .is_err()
        );
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
        assert!(
            acquire_locked_files(&[LockedArtifact {
                locator,
                expected_sha256: "0".repeat(64),
                expected_size: 5,
                max_bytes: MAX_FILE_BYTES,
            }])
            .is_err()
        );
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
        let responses: Vec<_> = responses
            .iter()
            .map(|(status, length, headers, body)| {
                let content_length = if *length == usize::MAX {
                    String::new()
                } else {
                    format!("Content-Length: {length}\r\n")
                };
                let headers = format!(
                    "HTTP/1.1 {status}\r\n{content_length}Connection: close\r\n{headers}\r\n"
                );
                (headers, body.to_vec())
            })
            .collect();
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
                stream.conn.send_close_notify();
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
        assert_eq!(fs::read(&acquired.files[0].path).unwrap(), contents);
        assert_eq!(acquired.files[0].path, acquired.files[1].path);
        assert!(!project.path().join("src").exists());
    }

    #[test]
    fn streams_https_artifact_without_content_length_and_cleans_scratch() {
        use crate::planner::{InstallationPlan, PlannedFile};

        let contents: Vec<u8> = (0..512 * 1024).map(|index| (index % 239) as u8).collect();
        let digest = format!("{:x}", Sha256::digest(&contents));
        let (mut manifest_url, root, server) = https_fixture("200 OK", usize::MAX, "", &contents);
        manifest_url.set_path("/registry.json");
        let plan = InstallationPlan {
            project_root: PathBuf::from("."),
            files: vec![PlannedFile {
                source: "asset.bin".into(),
                target: "asset.bin".into(),
                target_display: "asset.bin".into(),
                destination: PathBuf::from("asset.bin"),
                expected_sha256: digest,
                exists: false,
            }],
            conflicts: Vec::new(),
        };
        let source = RegistrySource::Remote { manifest_url };
        let acquired = acquire_files_with_root(&source, &plan, Some(root)).unwrap();
        server.join().unwrap();
        let staged = acquired.files[0].path.clone();
        assert_eq!(acquired.files[0].size, contents.len() as u64);
        assert_eq!(fs::read(&staged).unwrap(), contents);
        drop(acquired);
        assert!(!staged.exists());
    }

    #[test]
    fn reports_final_same_origin_artifact_locator() {
        use crate::planner::{InstallationPlan, PlannedFile};

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
        let project = tempfile::tempdir().unwrap();
        let plan = InstallationPlan {
            project_root: project.path().to_path_buf(),
            files: vec![PlannedFile {
                source: "asset".into(),
                target: "asset".into(),
                target_display: "asset".into(),
                destination: project.path().join("asset"),
                expected_sha256: digest,
                exists: false,
            }],
            conflicts: Vec::new(),
        };
        let result =
            acquire_v2_files_with_root(&source, &plan, &[contents.len() as u64], Some(root))
                .unwrap();
        handle.join().unwrap();
        assert_eq!(fs::read(&result.files[0].path).unwrap(), contents);
        let locator = result.files[0].resolved_locator.as_ref().unwrap();
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
    fn follows_four_redirects_and_rejects_the_fifth() {
        let (url, root, handle) = https_sequence(&[
            ("302 Found", 0, "Location: /r1\r\n", b""),
            ("302 Found", 0, "Location: /r2\r\n", b""),
            ("302 Found", 0, "Location: /r3\r\n", b""),
            ("302 Found", 0, "Location: /r4\r\n", b""),
            ("200 OK", 7, "", b"fixture"),
        ]);
        assert_eq!(
            fetch_url_with_root(&url, 20, "fixture", Some(root))
                .unwrap()
                .0,
            b"fixture"
        );
        handle.join().unwrap();

        let (url, root, handle) = https_sequence(&[
            ("302 Found", 0, "Location: /r1\r\n", b""),
            ("302 Found", 0, "Location: /r2\r\n", b""),
            ("302 Found", 0, "Location: /r3\r\n", b""),
            ("302 Found", 0, "Location: /r4\r\n", b""),
            ("302 Found", 0, "Location: /r5\r\n", b""),
        ]);
        let error = fetch_url_with_root(&url, 20, "fixture", Some(root)).unwrap_err();
        assert!(error.contains("redirect rejected"), "{error}");
        handle.join().unwrap();
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
        let error = acquire_files_with_policy(&source, &plan, Some(root), false, None).unwrap_err();
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
