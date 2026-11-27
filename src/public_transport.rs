//! Restricted transport for curated public directory and provider resources.
//!
//! This module intentionally has no configuration or proxy hooks. Custom registries
//! continue to use the existing transport in `downloader`.

use reqwest::{Url, blocking::Client, redirect::Policy};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    time::Duration,
};

pub const PUBLIC_REGISTRY_HOST: &str = "raw.githubusercontent.com";
pub const PUBLIC_DIRECTORY_URL: &str =
    "https://raw.githubusercontent.com/FrilLab/frilregistry/main/registries.json";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedRepository {
    pub owner: String,
    pub name: String,
    pub revision: String,
}

impl ApprovedRepository {
    pub fn validate_artifact_url(
        &self,
        manifest_url: &Url,
        artifact_url: &Url,
    ) -> Result<Url, String> {
        let manifest = self.validate_manifest_url(manifest_url.as_str())?;
        let artifact = self.validate_manifest_url(artifact_url.as_str())?;
        let prefix = format!("{}/{}/{}/", self.owner, self.name, self.revision);
        let root_path = format!("/{prefix}");
        if !artifact.path().starts_with(&root_path) || !manifest.path().starts_with(&root_path) {
            return Err("Public resource URL escapes the reviewed repository revision".into());
        }
        let manifest_parent = manifest
            .path()
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .ok_or("Invalid reviewed manifest path")?;
        if artifact.path() == manifest.path()
            || !artifact.path().starts_with(&format!("{manifest_parent}/"))
        {
            return Err("Public artifact URL escapes its reviewed manifest directory".into());
        }
        Ok(artifact)
    }
}

impl ApprovedRepository {
    pub fn artifact_url(&self, registry_url: &Url, source: &str) -> Result<Url, String> {
        let manifest_path = registry_url
            .path_segments()
            .ok_or("Invalid public manifest path")?
            .collect::<Vec<_>>();
        if manifest_path.len() < 4 {
            return Err("Invalid public manifest directory".into());
        }
        let source = crate::registry::validate_relative_path(source, "source")?;
        let mut directory = manifest_path[3..manifest_path.len() - 1].to_vec();
        directory.extend(source.split('/'));
        let base = format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            self.owner,
            self.name,
            self.revision,
            directory.join("/")
        );
        let artifact = Url::parse(&base).map_err(|_| "Invalid public artifact URL")?;
        self.validate_artifact_url(registry_url, &artifact)
    }
}

impl ApprovedRepository {
    pub fn new(repository: &str, revision: &str) -> Result<Self, String> {
        if !repository.starts_with("https://github.com/") {
            return Err("Approved repositories must use a canonical GitHub HTTPS URL".into());
        }
        let url = Url::parse(repository).map_err(|_| "Invalid approved repository URL")?;
        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Approved repositories must use a canonical GitHub HTTPS URL".into());
        }
        let mut segments = url
            .path_segments()
            .ok_or("Invalid approved repository path")?;
        let owner = segments.next().unwrap_or_default();
        let name = segments.next().unwrap_or_default();
        if owner.is_empty()
            || name.is_empty()
            || segments.next().is_some()
            || !valid_github_segment(owner)
            || !valid_github_segment(name.strip_suffix(".git").unwrap_or(name))
        {
            return Err(
                "Approved repository URL must identify exactly one GitHub repository".into(),
            );
        }
        let repository_name = name.strip_suffix(".git").unwrap_or(name);
        if !valid_revision(revision) {
            return Err(
                "Approved revision must be a 40 or 64 character lowercase hexadecimal commit"
                    .into(),
            );
        }
        Ok(Self {
            owner: owner.to_string(),
            name: repository_name.to_string(),
            revision: revision.to_string(),
        })
    }

    pub fn validate_manifest_url(&self, raw_url: &str) -> Result<Url, String> {
        let url = validate_public_url(raw_url)?;
        let segments: Vec<_> = url
            .path_segments()
            .ok_or("Invalid public registry path")?
            .collect();
        if segments.len() < 4
            || segments[0] != self.owner
            || segments[1] != self.name
            || segments[2] != self.revision
            || segments[3..].iter().any(|part| !valid_github_segment(part))
        {
            return Err(
                "Public registry URL does not match its approved repository and immutable revision"
                    .into(),
            );
        }
        Ok(url)
    }
}

pub fn validate_public_url(raw_url: &str) -> Result<Url, String> {
    if raw_url.len() > 2_048
        || !raw_url.starts_with("https://raw.githubusercontent.com/")
        || raw_url.bytes().any(|byte| byte.is_ascii_control())
        || raw_url.contains('%')
        || raw_url.contains('\\')
    {
        return Err("Public URL contains an unsafe or oversized encoding".into());
    }
    let url = Url::parse(raw_url).map_err(|_| "Invalid public HTTPS URL")?;
    if url.scheme() != "https"
        || url.host_str() != Some(PUBLIC_REGISTRY_HOST)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().starts_with("//")
    {
        return Err(
            "Public resources must use the approved raw GitHub HTTPS origin without URL suffixes"
                .into(),
        );
    }
    Ok(url)
}

pub fn fetch(
    url: &Url,
    approved_repository: &ApprovedRepository,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if max_bytes == 0 {
        return Err("Public response limit must be greater than zero".into());
    }
    let checked = approved_repository.validate_manifest_url(url.as_str())?;
    fetch_checked_url(checked, max_bytes)
}

pub fn fetch_artifact(
    url: &Url,
    manifest_url: &Url,
    approved_repository: &ApprovedRepository,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if max_bytes == 0 {
        return Err("Public response limit must be greater than zero".into());
    }
    let checked = approved_repository.validate_artifact_url(manifest_url, url)?;
    fetch_checked_url(checked, max_bytes)
}

/// Fetches only FrilLab's compiled canonical static directory endpoint.
pub fn fetch_directory() -> Result<Vec<u8>, String> {
    let checked = validate_public_url(PUBLIC_DIRECTORY_URL)?;
    if checked.as_str() != PUBLIC_DIRECTORY_URL
        || checked.path() != "/FrilLab/frilregistry/main/registries.json"
    {
        return Err("Compiled public directory endpoint is invalid".into());
    }
    fetch_checked_url(checked, crate::public_directory::MAX_DIRECTORY_BYTES)
}

fn fetch_checked_url(checked: Url, max_bytes: usize) -> Result<Vec<u8>, String> {
    if max_bytes == 0 {
        return Err("Public response limit must be greater than zero".into());
    }
    let addresses = resolve_public_addresses(checked.host_str().ok_or("Missing public host")?)?;
    let builder = Client::builder()
        .https_only(true)
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(Policy::none())
        .resolve_to_addrs(PUBLIC_REGISTRY_HOST, &addresses);
    // Avoid inherited system proxy settings and preserve certificate validation for
    // the URL hostname while the resolver is pinned to the checked socket addresses.
    let client = builder
        .build()
        .map_err(|_| "Cannot create restricted public HTTPS client")?;
    let mut response = client
        .get(checked)
        .send()
        .map_err(|_| "Public registry request failed")?;
    if !response.status().is_success() {
        return Err(format!(
            "Public registry request failed with HTTP status {}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!(
            "Public response exceeds the {max_bytes} byte limit"
        ));
    }
    let mut bytes =
        Vec::with_capacity(response.content_length().unwrap_or(0).min(max_bytes as u64) as usize);
    response
        .by_ref()
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read public registry response")?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "Public response exceeds the {max_bytes} byte limit"
        ));
    }
    Ok(bytes)
}

pub fn fetch_verified_manifest(
    url: &Url,
    approved_repository: &ApprovedRepository,
    expected_sha256: &str,
) -> Result<Vec<u8>, String> {
    if expected_sha256.len() != 64
        || !expected_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("Invalid reviewed manifest SHA-256 digest".into());
    }
    let bytes = fetch(
        url,
        approved_repository,
        crate::registry::MAX_MANIFEST_BYTES,
    )?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != expected_sha256 {
        return Err("Public registry manifest does not match its reviewed SHA-256 digest".into());
    }
    Ok(bytes)
}

fn resolve_public_addresses(host: &str) -> Result<Vec<SocketAddr>, String> {
    if host != PUBLIC_REGISTRY_HOST {
        return Err("Public host is not allowlisted".into());
    }
    let addresses: Vec<_> = (host, 443)
        .to_socket_addrs()
        .map_err(|_| "Cannot resolve approved public registry host")?
        .collect();
    validate_resolved_addresses(&addresses)?;
    Ok(addresses)
}

fn validate_resolved_addresses(addresses: &[SocketAddr]) -> Result<(), String> {
    if addresses.is_empty() {
        return Err("Approved public host resolved to no addresses".into());
    }
    if addresses
        .iter()
        .any(|address| !is_public_unicast(address.ip()))
    {
        return Err("Approved public host resolved to a non-public network address".into());
    }
    Ok(())
}

fn is_public_unicast(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            !(ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_multicast()
                || octets[0] == 0
                || octets[0] >= 240
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
                || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
                || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
                || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            let global_unicast_prefix = segments[0] & 0xe000 == 0x2000;
            global_unicast_prefix
                && !ip.is_unspecified()
                && !ip.is_loopback()
                && !ip.is_multicast()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && !(segments[0] == 0x2001 && (segments[1] <= 0x01ff || segments[1] == 0x0db8))
                && segments[0] != 0x2002 // 6to4 embeds arbitrary IPv4 destinations.
        }
    }
}

fn valid_revision(revision: &str) -> bool {
    matches!(revision.len(), 40 | 64)
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_github_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn repo() -> ApprovedRepository {
        ApprovedRepository::new(
            "https://github.com/FrilLab/frilcraft",
            "0123456789abcdef0123456789abcdef01234567",
        )
        .unwrap()
    }

    #[test]
    fn only_approved_https_origin_and_canonical_urls_are_accepted() {
        for value in [
            "http://raw.githubusercontent.com/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/registry.json",
            "https://raw.githubusercontent.com.evil.test/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/registry.json",
            "https://user@raw.githubusercontent.com/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/registry.json",
            "https://raw.githubusercontent.com/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/registry.json?x=1",
            "https://raw.githubusercontent.com/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/%2e%2e/secret",
            "https://127.0.0.1/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/registry.json",
        ] {
            assert!(validate_public_url(value).is_err(), "accepted {value}");
        }
        assert_eq!(
            validate_public_url(PUBLIC_DIRECTORY_URL).unwrap().as_str(),
            PUBLIC_DIRECTORY_URL
        );
        assert!(
            validate_public_url(
                "https://raw.githubusercontent.com/Other/frilregistry/main/registries.json"
            )
            .is_ok()
        );
    }

    #[test]
    fn manifest_url_must_match_reviewed_repository_and_revision() {
        let valid = "https://raw.githubusercontent.com/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/registry.json";
        assert!(repo().validate_manifest_url(valid).is_ok());
        for suffix in [
            "https://raw.githubusercontent.com/Other/frilcraft/0123456789abcdef0123456789abcdef01234567/registry.json",
            "https://raw.githubusercontent.com/FrilLab/other/0123456789abcdef0123456789abcdef01234567/registry.json",
            "https://raw.githubusercontent.com/FrilLab/frilcraft/1123456789abcdef0123456789abcdef01234567/registry.json",
            "https://raw.githubusercontent.com/FrilLab/frilcraft/main/registry.json",
            "https://raw.githubusercontent.com/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/../registry.json",
        ] {
            assert!(
                repo().validate_manifest_url(suffix).is_err(),
                "accepted {suffix}"
            );
        }
    }

    #[test]
    fn artifact_url_stays_under_reviewed_manifest_directory() {
        let approved =
            ApprovedRepository::new("https://github.com/example/provider", &"a".repeat(40))
                .unwrap();
        let manifest = approved
            .validate_manifest_url(&format!(
                "https://raw.githubusercontent.com/example/provider/{}/registry/registry.json",
                "a".repeat(40)
            ))
            .unwrap();
        let artifact = approved
            .artifact_url(&manifest, "assets/envelope.bin")
            .unwrap();
        assert!(artifact.path().ends_with("/assets/envelope.bin"));
        assert!(approved.validate_artifact_url(&manifest, &artifact).is_ok());
        assert!(approved.artifact_url(&manifest, "../escape").is_err());
        let sibling = Url::parse(&format!(
            "https://raw.githubusercontent.com/example/provider/{}/other.bin",
            "a".repeat(40)
        ))
        .unwrap();
        assert!(approved.validate_artifact_url(&manifest, &sibling).is_err());
        assert!(
            approved
                .artifact_url(&manifest, "https://example.invalid/file")
                .is_err()
        );
    }

    #[test]
    fn rejects_non_public_and_mixed_dns_answers_before_connection() {
        let public = "93.184.216.34:443".parse().unwrap();
        let private = "10.2.3.4:443".parse().unwrap();
        assert!(validate_resolved_addresses(&[public]).is_ok());
        assert!(validate_resolved_addresses(&[public, private]).is_err());
        assert!(validate_resolved_addresses(&[]).is_err());
        for ip in [
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(169, 254, 169, 254),
            Ipv4Addr::new(100, 64, 0, 1),
            Ipv4Addr::new(198, 18, 0, 1),
            Ipv4Addr::new(203, 0, 113, 10),
        ] {
            assert!(!is_public_unicast(IpAddr::V4(ip)), "accepted {ip}");
        }
        for ip in [
            Ipv6Addr::LOCALHOST,
            Ipv6Addr::UNSPECIFIED,
            "fe80::1".parse().unwrap(),
            "fd00::1".parse().unwrap(),
            "2001:db8::1".parse().unwrap(),
            "::ffff:127.0.0.1".parse().unwrap(),
        ] {
            assert!(!is_public_unicast(IpAddr::V6(ip)), "accepted {ip}");
        }
        assert!(is_public_unicast(IpAddr::V6(
            "2606:4700:4700::1111".parse().unwrap()
        )));
    }

    #[test]
    fn validates_digest_representation_before_network_access() {
        let url = repo()
            .validate_manifest_url("https://raw.githubusercontent.com/FrilLab/frilcraft/0123456789abcdef0123456789abcdef01234567/registry.json")
            .unwrap();
        assert!(fetch_verified_manifest(&url, &repo(), "not-a-digest").is_err());
        assert!(fetch_verified_manifest(&url, &repo(), &"A".repeat(64)).is_err());
    }
}
