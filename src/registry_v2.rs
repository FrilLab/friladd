//! Registry v2 provider metadata and target mapping.

use crate::{
    downloader::read_bounded,
    registry::{self, Registry, RegistryFile, RegistryItem},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub const MAX_ASSET_BYTES: u64 = crate::downloader::MAX_V2_FILE_BYTES as u64;
pub const TARGET_MAP_NAME: &str = ".friladd-targets.json";
const MAX_MAPPING_BYTES: usize = 64 * 1024;
const MAX_DECLARATIONS: usize = 256;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryV2 {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub version: String,
    pub kind: Kind,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub license: Option<License>,
    pub files: Vec<Asset>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub packages: Vec<Package>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Component,
    Asset,
    Material,
    Texture,
    Image,
    Environment,
    Animation,
    Composition,
    Other,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct License {
    pub id: String,
    #[serde(default)]
    pub notice: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    pub source: String,
    pub target: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dependency {
    pub provider_id: String,
    pub item_id: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_sha256: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub spec: String,
    pub kind: PackageKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageKind {
    Runtime,
    Peer,
    Dev,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetMap {
    pub schema_version: u32,
    #[serde(default)]
    pub default_root: Option<String>,
    #[serde(default, deserialize_with = "deserialize_kind_roots")]
    pub kind_roots: BTreeMap<Kind, String>,
}

impl Default for TargetMap {
    fn default() -> Self {
        Self {
            schema_version: 1,
            default_root: None,
            kind_roots: BTreeMap::new(),
        }
    }
}

fn deserialize_kind_roots<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<Kind, String>, D::Error> {
    struct UniqueRoots;
    impl<'de> serde::de::Visitor<'de> for UniqueRoots {
        type Value = BTreeMap<Kind, String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map of unique resource kinds")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut roots = BTreeMap::new();
            while let Some((kind, root)) = map.next_entry()? {
                if roots.insert(kind, root).is_some() {
                    return Err(serde::de::Error::custom("duplicate target mapping kind"));
                }
            }
            Ok(roots)
        }
    }
    deserializer.deserialize_map(UniqueRoots)
}

impl RegistryV2 {
    pub fn parse(content: &str) -> Result<Self, String> {
        if content.len() > registry::MAX_MANIFEST_BYTES {
            return Err("Registry v2 manifest exceeds its byte limit".into());
        }
        let manifest: Self = serde_json::from_str(content).map_err(|error| {
            format!(
                "Invalid registry v2 manifest at line {}, column {}",
                error.line(),
                error.column()
            )
        })?;
        if manifest.schema_version != 2 {
            return Err("Unsupported registry v2 schema version".into());
        }
        registry::validate_name(&manifest.id, "provider")?;
        registry::validate_name(&manifest.name, "registry")?;
        if manifest.items.len() > registry::MAX_ITEMS {
            return Err("Registry v2 has too many items".into());
        }
        let mut items = BTreeMap::new();
        let mut sources = BTreeMap::new();
        for item in &manifest.items {
            item.validate(&manifest.id)?;
            if items.insert(item.id.as_str(), item).is_some() {
                return Err(format!("Duplicate item ID: {}", item.id));
            }
            for file in &item.files {
                let expected = (file.sha256.as_str(), file.size);
                if sources
                    .insert(file.source.as_str(), expected)
                    .is_some_and(|value| value != expected)
                {
                    return Err(format!("Conflicting metadata for source {}", file.source));
                }
            }
        }
        validate_internal_graph(&manifest.id, &items)?;
        Ok(manifest)
    }

    pub fn select(&self, id: &str, exact_version: Option<&str>) -> Result<&Item, String> {
        let item = self
            .items
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("Item not found: {}", id.escape_debug()))?;
        if let Some(version) = exact_version {
            crate::lockfile::validate_version(version)?;
            if version != item.version {
                return Err(format!(
                    "Item {id} has version {}, requested {version}",
                    item.version
                ));
            }
        }
        Ok(item)
    }

    pub(crate) fn projection(&self) -> Registry {
        Registry {
            schema_version: 2,
            name: self.name.clone(),
            items: self
                .items
                .iter()
                .map(|item| RegistryItem {
                    name: item.id.clone(),
                    description: item.description.clone().unwrap_or_default(),
                    files: item
                        .files
                        .iter()
                        .map(|file| RegistryFile {
                            source: file.source.clone(),
                            target: file.target.clone(),
                            sha256: file.sha256.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

fn validate_internal_graph(provider: &str, items: &BTreeMap<&str, &Item>) -> Result<(), String> {
    let mut counts: BTreeMap<_, usize> = items.keys().map(|&id| (id, 0)).collect();
    for item in items.values() {
        for edge in item
            .dependencies
            .iter()
            .filter(|edge| edge.provider_id == provider)
        {
            let selected = items.get(edge.item_id.as_str()).ok_or_else(|| {
                format!("Missing internal dependency {provider}/{}", edge.item_id)
            })?;
            if selected.version != edge.version {
                return Err(format!(
                    "Internal dependency {} requires version {}",
                    edge.item_id, edge.version
                ));
            }
            counts
                .entry(selected.id.as_str())
                .and_modify(|count| *count += 1);
        }
    }
    let mut roots: Vec<_> = counts
        .iter()
        .filter_map(|(&id, &count)| (count == 0).then_some(id))
        .collect();
    let mut visited = 0;
    while let Some(id) = roots.pop() {
        visited += 1;
        for edge in items[id]
            .dependencies
            .iter()
            .filter(|edge| edge.provider_id == provider)
        {
            let next = edge.item_id.as_str();
            let count = counts
                .get_mut(next)
                .expect("internal dependency was validated");
            *count -= 1;
            if *count == 0 {
                roots.push(next);
            }
        }
    }
    if visited != items.len() {
        return Err(format!("Cycle in internal dependencies of {provider}"));
    }
    Ok(())
}

impl Item {
    fn validate(&self, provider: &str) -> Result<(), String> {
        registry::validate_name(&self.id, "item")?;
        crate::lockfile::validate_version(&self.version)?;
        if self
            .description
            .as_ref()
            .is_some_and(|text| text.trim().is_empty() || text.len() > 1024)
        {
            return Err(format!("Invalid description for {}", self.id));
        }
        if self.tags.len() > 32
            || self.tags.iter().collect::<BTreeSet<_>>().len() != self.tags.len()
        {
            return Err(format!("Duplicate or excessive tags for {}", self.id));
        }
        for tag in &self.tags {
            if tag.len() > 64 {
                return Err("Registry tag exceeds 64 bytes".into());
            }
            registry::validate_name(tag, "tag")?;
        }
        if self.files.is_empty() || self.files.len() > registry::MAX_FILES_PER_ITEM {
            return Err(format!("Invalid file count for {}", self.id));
        }
        registry::validate_targets(self.files.iter().map(|file| file.target.as_str()))?;
        for file in &self.files {
            registry::validate_relative_path(&file.source, "source")?;
            crate::lockfile::validate_digest(&file.sha256)?;
            if file.size > MAX_ASSET_BYTES {
                return Err(format!(
                    "Declared size exceeds the v2 per-file limit for {}",
                    file.source
                ));
            }
        }
        if let Some(license) = &self.license {
            let bytes = license.id.as_bytes();
            if bytes.len() > 128
                || !bytes
                    .first()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                || !bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'+'))
            {
                return Err("Invalid license identifier".into());
            }
            if let Some(notice) = &license.notice {
                registry::validate_relative_path(notice, "license notice")?;
                if !self.files.iter().any(|file| &file.source == notice) {
                    return Err("License notice must identify a source listed in the item".into());
                }
            }
        }
        if self.dependencies.len() > MAX_DECLARATIONS || self.packages.len() > MAX_DECLARATIONS {
            return Err("Too many dependency or package declarations".into());
        }
        let mut seen = BTreeSet::new();
        for edge in &self.dependencies {
            registry::validate_name(&edge.provider_id, "dependency provider")?;
            registry::validate_name(&edge.item_id, "dependency item")?;
            crate::lockfile::validate_version(&edge.version)?;
            if !seen.insert((&edge.provider_id, &edge.item_id)) {
                return Err("Duplicate dependency".into());
            }
            match (edge.provider_id == provider, &edge.manifest_sha256) {
                (true, None) => (),
                (false, Some(digest)) => crate::lockfile::validate_digest(digest)?,
                (true, Some(_)) => {
                    return Err("Internal dependencies use the enclosing snapshot digest".into());
                }
                (false, None) => {
                    return Err("External dependencies require an exact manifest digest".into());
                }
            }
        }
        let mut seen = BTreeSet::new();
        for package in &self.packages {
            validate_package(&package.name, &package.spec)?;
            if !seen.insert(&package.name) {
                return Err(format!("Duplicate package requirement {}", package.name));
            }
        }
        Ok(())
    }
}

impl TargetMap {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_MAPPING_BYTES {
            return Err("Target mapping exceeds the byte limit".into());
        }
        let map: Self = serde_json::from_slice(bytes).map_err(|error| {
            format!(
                "Invalid target mapping at line {}, column {}",
                error.line(),
                error.column()
            )
        })?;
        if map.schema_version != 1 {
            return Err("Unsupported target mapping schema version".into());
        }
        for root in map.default_root.iter().chain(map.kind_roots.values()) {
            registry::validate_targets([root.as_str()])?;
        }
        Ok(map)
    }

    pub fn load(project: &Path) -> Result<Self, String> {
        let project = crate::lockfile::canonical_project(project)?;
        let path = project.join(TARGET_MAP_NAME);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                let file = File::open(&path)
                    .map_err(|error| format!("Cannot open target mapping: {error}"))?;
                Self::parse(&read_bounded(file, MAX_MAPPING_BYTES, "target mapping")?)
            }
            Ok(_) => Err("Target mapping must be a regular non-symlink file".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("Cannot inspect target mapping: {error}")),
        }
    }

    pub fn root(&self, kind: Kind, explicit: Option<&str>) -> Result<Option<String>, String> {
        let root = explicit
            .or_else(|| self.kind_roots.get(&kind).map(String::as_str))
            .or(self.default_root.as_deref());
        if let Some(root) = root {
            registry::validate_targets([root])?;
        }
        Ok(root.map(str::to_owned))
    }

    pub fn targets(&self, item: &Item, explicit: Option<&str>) -> Result<Vec<String>, String> {
        let root = self.root(item.kind, explicit)?;
        let targets: Vec<_> = item
            .files
            .iter()
            .map(|file| match &root {
                Some(root) => format!("{root}/{}", file.target),
                None => file.target.clone(),
            })
            .collect();
        registry::validate_targets(targets.iter().map(String::as_str))?;
        Ok(targets)
    }

    pub fn check_destinations(
        &self,
        project: &Path,
        item: &Item,
        explicit: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let root = crate::lockfile::canonical_project(project)?;
        let targets = self.targets(item, explicit)?;
        for target in &targets {
            let relative = PathBuf::from(target);
            crate::planner::validate_destination(&root, &relative, &root.join(&relative))?;
        }
        Ok(targets)
    }
}

pub fn validate_package(name: &str, spec: &str) -> Result<(), String> {
    let valid_name = |part: &str| {
        part.as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'.' | b'_')
            })
    };
    let valid = if let Some(scoped) = name.strip_prefix('@') {
        scoped
            .split_once('/')
            .is_some_and(|(scope, package)| valid_name(scope) && valid_name(package))
    } else {
        valid_name(name)
    };
    if !valid || name.len() > 214 {
        return Err("Invalid npm package name".into());
    }
    let version = spec.strip_prefix(['^', '~']).unwrap_or(spec);
    crate::lockfile::validate_version(version)
        .map_err(|_| "Package spec must be an exact version or a single ^ or ~ range".into())
}
