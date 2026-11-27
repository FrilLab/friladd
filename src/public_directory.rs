//! Parser and policy checks for the static curated directory document.

use crate::public_transport::{ApprovedRepository, validate_public_url};
use serde::Deserialize;
use std::collections::HashSet;

pub const MAX_DIRECTORY_BYTES: usize = 1024 * 1024;
pub const MAX_DIRECTORY_ENTRIES: usize = 256;

const OFFICIAL_NAMESPACE_PREFIX: &str = "fril";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tier {
    Official,
    Community,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Active,
    Deprecated,
    Suspended,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryEntry {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub registry_url: String,
    pub repository: String,
    pub publisher: String,
    pub tier: Tier,
    pub status: Status,
    pub reviewed_revision: String,
    pub manifest_sha256: String,
    pub review: Review,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Review {
    pub reviewed_at: String,
    pub reviewer: String,
    pub notes: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicIdentity {
    pub registry_id: String,
    pub publisher: String,
    pub tier: String,
    pub repository: String,
    pub registry_url: String,
    pub reviewed_revision: String,
    pub manifest_sha256: String,
}

#[derive(Debug)]
pub struct PublicDirectory {
    pub registries: Vec<DirectoryEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DirectoryDocument {
    schema_version: u32,
    registries: Vec<DirectoryEntryDocument>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DirectoryEntryDocument {
    id: String,
    display_name: String,
    description: String,
    registry_url: String,
    repository: String,
    publisher: String,
    tier: String,
    status: String,
    reviewed_revision: String,
    manifest_sha256: String,
    review: ReviewDocument,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewDocument {
    reviewed_at: String,
    reviewer: String,
    notes: String,
}

impl PublicDirectory {
    pub fn fetch() -> Result<Self, String> {
        Self::parse(&crate::public_transport::fetch_directory()?)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_DIRECTORY_BYTES {
            return Err(format!(
                "Public directory exceeds the {MAX_DIRECTORY_BYTES} byte limit"
            ));
        }
        let document: DirectoryDocument = serde_json::from_slice(bytes)
            .map_err(|_| "Invalid public directory JSON or unsupported directory fields")?;
        if document.schema_version != 1 {
            return Err(format!(
                "Unsupported public directory schema version: {}",
                document.schema_version
            ));
        }
        if document.registries.len() > MAX_DIRECTORY_ENTRIES {
            return Err(format!(
                "Public directory contains more than {MAX_DIRECTORY_ENTRIES} entries"
            ));
        }
        let mut ids = HashSet::new();
        let mut urls = HashSet::new();
        let mut repositories = HashSet::new();
        let mut entries = Vec::with_capacity(document.registries.len());
        for entry in document.registries {
            let parsed = parse_entry(entry)?;
            if !ids.insert(parsed.id.clone()) {
                return Err(format!("Duplicate public registry ID: {}", parsed.id));
            }
            let normalized_url = validate_public_url(&parsed.registry_url)?.to_string();
            if !urls.insert(normalized_url) {
                return Err(format!(
                    "Public registry URL is listed more than once: {}",
                    parsed.id
                ));
            }
            let normalized_repository = parsed.repository.to_ascii_lowercase();
            if !repositories.insert(normalized_repository) {
                return Err(format!(
                    "Public repository is listed more than once: {}",
                    parsed.id
                ));
            }
            entries.push(parsed);
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(Self {
            registries: entries,
        })
    }

    pub fn get(&self, id: &str) -> Option<&DirectoryEntry> {
        self.registries.iter().find(|entry| entry.id == id)
    }

    pub fn active_official(&self) -> impl Iterator<Item = &DirectoryEntry> {
        self.registries
            .iter()
            .filter(|entry| entry.tier == Tier::Official && entry.status == Status::Active)
    }

    pub fn active_public(&self) -> impl Iterator<Item = &DirectoryEntry> {
        self.registries
            .iter()
            .filter(|entry| entry.status == Status::Active)
    }

    pub(crate) fn load_manifest(
        entry: &DirectoryEntry,
    ) -> Result<crate::registry::LoadedRegistry, String> {
        let repository = ApprovedRepository::new(&entry.repository, &entry.reviewed_revision)?;
        let url = repository.validate_manifest_url(&entry.registry_url)?;
        let bytes = crate::public_transport::fetch_verified_manifest(
            &url,
            &repository,
            &entry.manifest_sha256,
        )?;
        let loaded = crate::registry::load_public_registry(
            &entry.registry_url,
            bytes,
            repository,
            entry.clone(),
        )?;
        let manifest_provider = loaded
            .v2
            .as_ref()
            .map(|v2| v2.id.as_str())
            .unwrap_or(loaded.registry.name.as_str());
        if manifest_provider != entry.id {
            return Err(format!(
                "Public registry {} manifest declares a different provider identity",
                entry.id
            ));
        }
        Ok(loaded)
    }
}

impl DirectoryEntry {
    pub fn public_identity(&self) -> PublicIdentity {
        PublicIdentity {
            registry_id: self.id.clone(),
            publisher: self.publisher.clone(),
            tier: match self.tier {
                Tier::Official => "official",
                Tier::Community => "community",
            }
            .to_owned(),
            repository: self.repository.clone(),
            registry_url: self.registry_url.clone(),
            reviewed_revision: self.reviewed_revision.clone(),
            manifest_sha256: self.manifest_sha256.clone(),
        }
    }
}

fn parse_entry(entry: DirectoryEntryDocument) -> Result<DirectoryEntry, String> {
    if !valid_id(&entry.id) {
        return Err(format!("Invalid public registry ID: {}", entry.id));
    }
    validate_text(&entry.display_name, 128, "displayName")?;
    validate_text(&entry.description, 512, "description")?;
    validate_text(&entry.publisher, 128, "publisher")?;
    validate_text(&entry.review.reviewer, 128, "reviewer")?;
    validate_text(&entry.review.notes, 1024, "review.notes")?;
    if !valid_timestamp(&entry.review.reviewed_at) {
        return Err(format!("Invalid review timestamp for {}", entry.id));
    }
    if entry.manifest_sha256.len() != 64
        || !entry
            .manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!("Invalid manifest SHA-256 for {}", entry.id));
    }
    let tier = match entry.tier.as_str() {
        "official" => Tier::Official,
        "community" => Tier::Community,
        _ => return Err(format!("Invalid tier for {}", entry.id)),
    };
    let status = match entry.status.as_str() {
        "active" => Status::Active,
        "deprecated" => Status::Deprecated,
        "suspended" => Status::Suspended,
        _ => return Err(format!("Invalid status for {}", entry.id)),
    };
    let approved = ApprovedRepository::new(&entry.repository, &entry.reviewed_revision)?;
    approved.validate_manifest_url(&entry.registry_url)?;
    let repository = url::Url::parse(&entry.repository).map_err(|_| "Invalid repository URL")?;
    let mut components = repository
        .path_segments()
        .ok_or("Invalid repository path")?;
    let owner = components.next().unwrap_or_default();
    let repo_name = components.next().unwrap_or_default();
    let expected_publisher = format!("github:{owner}");
    if entry.publisher != expected_publisher {
        return Err(format!(
            "Publisher identity does not match repository owner for {}",
            entry.id
        ));
    }
    match tier {
        Tier::Official => {
            if owner != "FrilLab"
                || entry.publisher != "github:FrilLab"
                || !entry.id.starts_with(OFFICIAL_NAMESPACE_PREFIX)
                || repo_name.to_ascii_lowercase() != entry.id
            {
                return Err(format!(
                    "Official registry {} must use its matching FrilLab repository and namespace",
                    entry.id
                ));
            }
        }
        Tier::Community => {
            if owner == "FrilLab" {
                return Err(format!(
                    "FrilLab-owned repository cannot be declared as community registry {}",
                    entry.id
                ));
            }
            if entry.id.starts_with(OFFICIAL_NAMESPACE_PREFIX) {
                return Err(format!(
                    "The FrilLab registry namespace cannot be community-owned: {}",
                    entry.id
                ));
            }
        }
    }
    Ok(DirectoryEntry {
        id: entry.id,
        display_name: entry.display_name,
        description: entry.description,
        registry_url: entry.registry_url,
        repository: entry.repository,
        publisher: entry.publisher,
        tier,
        status,
        reviewed_revision: entry.reviewed_revision,
        manifest_sha256: entry.manifest_sha256,
        review: Review {
            reviewed_at: entry.review.reviewed_at,
            reviewer: entry.review.reviewer,
            notes: entry.review.notes,
        },
    })
}

pub(crate) fn is_official_identity(identity: &PublicIdentity) -> bool {
    if identity.tier != "official"
        || identity.publisher != "github:FrilLab"
        || !identity.registry_id.starts_with(OFFICIAL_NAMESPACE_PREFIX)
    {
        return false;
    }
    let Ok(repository) = url::Url::parse(&identity.repository) else {
        return false;
    };
    let Some(mut segments) = repository.path_segments() else {
        return false;
    };
    let owner = segments.next().unwrap_or_default();
    let name = segments.next().unwrap_or_default();
    owner == "FrilLab"
        && name.to_ascii_lowercase() == identity.registry_id
        && segments.next().is_none()
        && ApprovedRepository::new(&identity.repository, &identity.reviewed_revision)
            .and_then(|approved| approved.validate_manifest_url(&identity.registry_url))
            .is_ok()
}

fn valid_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn validate_text(value: &str, max: usize, name: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err(format!("Invalid {name} value in public directory"));
    }
    Ok(())
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || !value.ends_with('Z')
        || !(0..19)
            .all(|index| matches!(index, 4 | 7 | 10 | 13 | 16) || bytes[index].is_ascii_digit())
    {
        return false;
    }
    if bytes.len() > 20
        && (bytes.get(19) != Some(&b'.')
            || bytes[20..bytes.len() - 1].is_empty()
            || !bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit))
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let segment = std::str::from_utf8(bytes.get(range)?).ok()?;
        segment.parse().ok()
    };
    let year = number(0..4);
    let month = number(5..7);
    let day = number(8..10);
    let hour = number(11..13);
    let minute = number(14..16);
    let second = number(17..19);
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) =
        (year, month, day, hour, minute, second)
    else {
        return false;
    };
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return false,
    };
    year > 0 && (1..=days_in_month).contains(&day) && hour <= 23 && minute <= 59 && second <= 59
}

#[cfg(test)]
mod tests {
    use super::*;

    const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

    fn document(id: &str, tier: &str, status: &str, revision: &str) -> Vec<u8> {
        let (owner, repository) = if tier == "official" {
            ("FrilLab", id)
        } else {
            ("example", id)
        };
        let manifest_url = format!(
            "https://raw.githubusercontent.com/{owner}/{repository}/{REVISION}/registry.json"
        );
        let repository_url = format!("https://github.com/{owner}/{repository}");
        let publisher = format!("github:{owner}");
        format!(
            r#"{{"schemaVersion":1,"registries":[{{"id":"{id}","displayName":"Provider","description":"Reviewed components","registryUrl":"{manifest_url}","repository":"{repository_url}","publisher":"{publisher}","tier":"{tier}","status":"{status}","reviewedRevision":"{revision}","manifestSha256":"{}","review":{{"reviewedAt":"2026-11-27T00:00:00Z","reviewer":"maintainer","notes":"Reviewed immutable source"}}}}]}}"#,
            "a".repeat(64)
        )
        .into_bytes()
    }

    #[test]
    fn parses_and_filters_directory_entries_deterministically() {
        let directory =
            PublicDirectory::parse(&document("frilcraft", "official", "active", REVISION)).unwrap();
        assert_eq!(directory.get("frilcraft").unwrap().id, "frilcraft");
        assert_eq!(directory.active_official().count(), 1);
        assert_eq!(directory.active_public().count(), 1);
        let community = PublicDirectory::parse(&document(
            "community-tools",
            "community",
            "suspended",
            REVISION,
        ))
        .unwrap();
        assert_eq!(community.active_public().count(), 0);
        let future_official =
            PublicDirectory::parse(&document("frilphysics", "official", "active", REVISION))
                .unwrap();
        assert_eq!(future_official.active_official().count(), 1);
        assert!(
            PublicDirectory::parse(&document("frilcraft", "official", "suspended", REVISION))
                .unwrap()
                .active_official()
                .next()
                .is_none()
        );
    }

    #[test]
    fn rejects_unknown_duplicate_oversized_and_spoofed_entries() {
        let mut json: serde_json::Value =
            serde_json::from_slice(&document("frilcraft", "official", "active", REVISION)).unwrap();
        json["arbitraryUrl"] = serde_json::json!("http://127.0.0.1/");
        assert!(PublicDirectory::parse(&serde_json::to_vec(&json).unwrap()).is_err());
        let value: serde_json::Value =
            serde_json::from_slice(&document("frilcraft", "official", "active", REVISION)).unwrap();
        let first = value["registries"][0].clone();
        let second = first.clone();
        let duplicate = serde_json::json!({"schemaVersion": 1, "registries": [first, second]});
        assert!(PublicDirectory::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
        assert!(
            PublicDirectory::parse(br#"{"schemaVersion":1,"schemaVersion":1,"registries":[]}"#)
                .is_err()
        );
        assert!(PublicDirectory::parse(&vec![b' '; MAX_DIRECTORY_BYTES + 1]).is_err());
        assert!(
            PublicDirectory::parse(&document("frilcraft", "community", "active", REVISION))
                .is_err()
        );
        let wrong_revision = "1123456789abcdef0123456789abcdef01234567";
        assert!(
            PublicDirectory::parse(&document("frilcraft", "official", "active", wrong_revision))
                .is_err()
        );
    }

    #[test]
    fn review_timestamps_require_real_utc_calendar_values() {
        for timestamp in [
            "2026-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-11-27T24:00:00Z",
            "2026-11-27T00:00:60Z",
            "0000-01-01T00:00:00Z",
            "2026-11-27T00:00:00+00:00",
            "2026-11-27T00:00:00.Z",
        ] {
            assert!(!valid_timestamp(timestamp), "accepted {timestamp}");
        }
        assert!(valid_timestamp("2024-02-29T00:00:00.123Z"));
    }
}
