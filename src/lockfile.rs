//! Validated, deterministic project ownership state.

use crate::{downloader::read_bounded, registry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};
use url::Url;

pub const NAME: &str = "friladd.lock.json";
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = crate::downloader::MAX_V2_FILE_BYTES as u64;
const MAX_ITEMS: usize = 10_000;
const MAX_FILES: usize = 100_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lock {
    pub schema_version: u32,
    pub providers: Vec<Provider>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provider {
    pub provider_id: String,
    pub declared_source: String,
    pub resolved_source: String,
    pub manifest_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_identity: Option<crate::public_directory::PublicIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub provider_id: String,
    pub item_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub manifest_source: String,
    pub manifest_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_root: Option<String>,
    pub dependencies: Vec<Dependency>,
    pub files: Vec<OwnedFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dependency {
    pub provider_id: String,
    pub item_id: String,
    pub version: String,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedFile {
    pub source: String,
    pub resolved_source: String,
    pub declared_target: String,
    pub target: String,
    pub expected_sha256: String,
    pub installed: Content,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Content {
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub(crate) lock: Lock,
    pub(crate) bytes: Option<Vec<u8>>,
}

impl Snapshot {
    pub fn lock(&self) -> &Lock {
        &self.lock
    }
}

impl Default for Lock {
    fn default() -> Self {
        Self {
            schema_version: 1,
            providers: Vec::new(),
            items: Vec::new(),
        }
    }
}

impl Content {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            sha256: format!("{:x}", Sha256::digest(bytes)),
            size: bytes.len() as u64,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        validate_digest(&self.sha256)?;
        if self.size > MAX_FILE_BYTES {
            return Err("Lock content exceeds the per-file byte limit".into());
        }
        Ok(())
    }
}

impl Lock {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("Lock exceeds the byte limit".into());
        }
        let mut lock: Self = serde_json::from_slice(bytes).map_err(|err| {
            format!(
                "Invalid lock JSON at line {}, column {}",
                err.line(),
                err.column()
            )
        })?;
        lock.validate()?;
        lock.sort();
        Ok(lock)
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut lock = self.clone();
        lock.sort();
        let mut bytes = serde_json::to_vec_pretty(&lock).map_err(|err| err.to_string())?;
        bytes.push(b'\n');
        if bytes.len() > MAX_BYTES {
            return Err("Lock exceeds the byte limit".into());
        }
        Ok(bytes)
    }

    pub fn owners(&self) -> BTreeMap<&str, (&Item, &OwnedFile)> {
        self.items
            .iter()
            .flat_map(|item| {
                item.files
                    .iter()
                    .map(move |file| (file.target.as_str(), (item, file)))
            })
            .collect()
    }

    fn sort(&mut self) {
        self.providers
            .sort_unstable_by(|a, b| a.provider_id.cmp(&b.provider_id));
        self.items.sort_unstable_by(|a, b| key(a).cmp(&key(b)));
        for item in &mut self.items {
            item.files.sort_unstable_by(|a, b| a.target.cmp(&b.target));
            item.dependencies.sort_unstable();
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "Unsupported lock schema version: {}",
                self.schema_version
            ));
        }
        if self.items.len() > MAX_ITEMS || self.providers.len() > MAX_ITEMS {
            return Err("Too many lock items or providers".into());
        }
        let mut providers = BTreeSet::new();
        for provider in &self.providers {
            validate_locator(&provider.declared_source)?;
            validate_locator(&provider.resolved_source)?;
            validate_digest(&provider.manifest_sha256)?;
            if let Some(identity) = &provider.public_identity {
                validate_id(&identity.registry_id, "public registry")?;
                if !matches!(identity.tier.as_str(), "official" | "community")
                    || identity.publisher.is_empty()
                    || identity.manifest_sha256 != provider.manifest_sha256
                {
                    return Err("Invalid public provider trust record".into());
                }
                if identity.registry_id != identity.registry_id.to_ascii_lowercase()
                    || identity.publisher.is_empty()
                    || identity.publisher.len() > 128
                {
                    return Err("Invalid public provider trust identity".into());
                }
                let approved = crate::public_transport::ApprovedRepository::new(
                    &identity.repository,
                    &identity.reviewed_revision,
                )?;
                approved.validate_manifest_url(&identity.registry_url)?;
                if (identity.tier == "official"
                    && !crate::public_directory::is_official_identity(identity))
                    || (identity.tier == "community" && identity.registry_id.starts_with("fril"))
                {
                    return Err("Lock public provider tier does not match its namespace".into());
                }
                if provider.resolved_source != identity.registry_url {
                    return Err(
                        "Lock public manifest source disagrees with its trust record".into(),
                    );
                }
            }
            if let Some(id) = provider.provider_id.strip_prefix("v2:") {
                validate_id(id, "provider")?;
            } else if provider.provider_id != v1_provider_id(&provider.resolved_source)? {
                return Err("Invalid v1 provider identity".into());
            }
            if !providers.insert(provider.provider_id.as_str()) {
                return Err("Duplicate lock provider".into());
            }
        }
        let mut items = BTreeMap::new();
        let mut file_count = 0;
        let mut edge_count = 0;
        for item in &self.items {
            validate_id(&item.item_id, "item")?;
            validate_locator(&item.manifest_source)?;
            validate_digest(&item.manifest_sha256)?;
            if !providers.contains(item.provider_id.as_str())
                || items.insert(key(item), item).is_some()
            {
                return Err("Unknown provider or duplicate lock item".into());
            }
            match &item.version {
                Some(version) if item.provider_id.starts_with("v2:") => validate_version(version)?,
                None if item.provider_id.starts_with("v1:") && item.dependencies.is_empty() => {
                    if item.provider_id != v1_provider_id(&item.manifest_source)? {
                        return Err("V1 item source disagrees with provider identity".into());
                    }
                }
                _ => return Err("Invalid version or dependencies for lock item".into()),
            }
            file_count += item.files.len();
            edge_count += item.dependencies.len();
            if item.files.is_empty() || file_count > MAX_FILES || edge_count > MAX_FILES {
                return Err("Invalid lock file or dependency count".into());
            }
            if let Some(root) = &item.target_root {
                registry::validate_targets([root.as_str()])?;
            }
            for file in &item.files {
                registry::validate_relative_path(&file.source, "source")?;
                registry::validate_targets([file.declared_target.as_str()])?;
                validate_file_locator(&item.manifest_source, file)?;
                validate_digest(&file.expected_sha256)?;
                file.installed.validate()?;
                let target = item.target_root.as_ref().map_or_else(
                    || file.declared_target.clone(),
                    |root| format!("{root}/{}", file.declared_target),
                );
                if target != file.target || file.expected_sha256 != file.installed.sha256 {
                    return Err(
                        "Locked file disagrees with target mapping or expected digest".into(),
                    );
                }
            }
        }
        registry::validate_targets(
            self.items
                .iter()
                .flat_map(|item| item.files.iter().map(|file| file.target.as_str())),
        )?;
        validate_dependencies(&items)
    }
}

fn key(item: &Item) -> (&str, &str) {
    (&item.provider_id, &item.item_id)
}

fn validate_dependencies(items: &BTreeMap<(&str, &str), &Item>) -> Result<(), String> {
    let mut incoming: BTreeMap<_, _> = items.keys().map(|&key| (key, 0usize)).collect();
    for item in items.values() {
        let mut seen = BTreeSet::new();
        for edge in &item.dependencies {
            let dependency = (edge.provider_id.as_str(), edge.item_id.as_str());
            let resolved = items.get(&dependency).ok_or("Unresolved lock dependency")?;
            if !seen.insert(dependency)
                || resolved.version.as_deref() != Some(edge.version.as_str())
                || resolved.manifest_sha256 != edge.manifest_sha256
            {
                return Err("Duplicate or conflicting lock dependency".into());
            }
            *incoming
                .get_mut(&dependency)
                .expect("dependency was looked up") += 1;
        }
    }
    let mut free: Vec<_> = incoming
        .iter()
        .filter_map(|(&key, &n)| (n == 0).then_some(key))
        .collect();
    let mut visited = 0;
    while let Some(key) = free.pop() {
        visited += 1;
        for edge in &items[&key].dependencies {
            let key = (edge.provider_id.as_str(), edge.item_id.as_str());
            let count = incoming.get_mut(&key).expect("dependencies were checked");
            *count -= 1;
            if *count == 0 {
                free.push(key);
            }
        }
    }
    if visited != items.len() {
        return Err("Cycle in lock dependencies".into());
    }
    Ok(())
}

fn validate_file_locator(manifest: &str, file: &OwnedFile) -> Result<(), String> {
    validate_locator(&file.resolved_source)?;
    let manifest = Url::parse(manifest).map_err(|_| "Invalid manifest locator")?;
    let source = Url::parse(&file.resolved_source).map_err(|_| "Invalid file locator")?;
    let matches = if manifest.scheme() == "file" {
        manifest
            .join(&file.source)
            .is_ok_and(|declared| declared == source)
    } else {
        manifest.origin() == source.origin()
    };
    if !matches {
        return Err("File provenance escaped its manifest location".into());
    }
    Ok(())
}

pub fn v1_provider_id(locator: &str) -> Result<String, String> {
    validate_locator(locator)?;
    Ok(format!("v1:{}", Content::of(locator.as_bytes()).sha256))
}

pub fn validate_locator(locator: &str) -> Result<(), String> {
    let url = Url::parse(locator).map_err(|_| "Invalid provenance locator")?;
    if locator.len() > 8192
        || url.as_str() != locator
        || !matches!(url.scheme(), "https" | "file")
        || (url.scheme() == "https" && url.host_str().is_none())
        || (url.scheme() == "file" && url.cannot_be_a_base())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Provenance must be a canonical file or HTTPS URL without authorization data".into(),
        );
    }
    Ok(())
}

pub fn validate_version(version: &str) -> Result<(), String> {
    if version.len() > 128 || semver::Version::parse(version).is_err() {
        return Err("Expected an exact semantic version".into());
    }
    Ok(())
}

pub fn validate_id(id: &str, label: &str) -> Result<(), String> {
    let bytes = id.as_bytes();
    if bytes.len() > 128
        || !bytes
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        || !bytes.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
    {
        return Err(format!("Invalid {label} identity"));
    }
    Ok(())
}

pub fn validate_digest(digest: &str) -> Result<(), String> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err("Lock digest must be 64 lowercase hexadecimal characters".into());
    }
    Ok(())
}

pub(crate) fn canonical_project(project: &Path) -> Result<PathBuf, String> {
    let project: PathBuf = project.components().collect();
    let meta = fs::symlink_metadata(&project)
        .map_err(|err| format!("Cannot inspect project {}: {err}", project.display()))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(format!(
            "Expected a non-symlink project directory: {}",
            project.display()
        ));
    }
    fs::canonicalize(&project)
        .map_err(|err| format!("Cannot resolve project {}: {err}", project.display()))
}

pub fn load(project: &Path) -> Result<Snapshot, String> {
    let path = canonical_project(project)?.join(NAME);
    let bytes = read_optional(&path)?;
    let lock = bytes
        .as_deref()
        .map(Lock::parse)
        .transpose()?
        .unwrap_or_default();
    Ok(Snapshot { lock, bytes })
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            let file = File::open(path)
                .map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
            read_bounded(file, MAX_BYTES, "lock").map(Some)
        }
        Ok(_) => Err(format!(
            "Expected a regular non-symlink file at {}",
            path.display()
        )),
        Err(error) => Err(format!("Cannot inspect {}: {error}", path.display())),
    }
}
