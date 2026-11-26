use serde::Deserialize;
use std::{collections::HashSet, fs, path::PathBuf};
use url::Url;

use crate::downloader;

pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_ITEMS: usize = 1_000;
const MAX_FILES_PER_ITEM: usize = 256;

#[derive(Clone, Debug)]
pub enum RegistrySource {
    Local { root: PathBuf },
    Remote { manifest_url: Url },
}

#[derive(Debug)]
pub struct LoadedRegistry {
    pub registry: Registry,
    pub source: RegistrySource,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Registry {
    pub schema_version: u32,
    pub name: String,
    pub items: Vec<RegistryItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryItem {
    pub name: String,
    pub description: String,
    pub files: Vec<RegistryFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryFile {
    pub source: String,
    pub target: String,
    pub sha256: String,
}

pub fn load_registry(location: &str) -> Result<LoadedRegistry, String> {
    let (bytes, source) = if location.contains("://") {
        let url = Url::parse(location).map_err(|error| format!("Invalid registry URL: {error}"))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(
                "Remote registries must use an HTTPS URL without embedded credentials".to_string(),
            );
        }
        let (bytes, final_url) =
            downloader::fetch_url(&url, MAX_MANIFEST_BYTES, "registry manifest")?;
        (
            bytes,
            RegistrySource::Remote {
                manifest_url: final_url,
            },
        )
    } else {
        let path = PathBuf::from(location);
        let manifest = fs::canonicalize(&path)
            .map_err(|error| format!("Cannot resolve registry {}: {error}", path.display()))?;
        let metadata = fs::metadata(&manifest)
            .map_err(|error| format!("Cannot inspect registry {}: {error}", manifest.display()))?;
        if !metadata.is_file() {
            return Err(format!(
                "Registry manifest is not a regular file: {}",
                manifest.display()
            ));
        }
        if metadata.len() > MAX_MANIFEST_BYTES as u64 {
            return Err(format!(
                "Registry manifest exceeds the {} byte limit",
                MAX_MANIFEST_BYTES
            ));
        }
        let bytes = fs::read(&manifest)
            .map_err(|error| format!("Cannot read registry {}: {error}", manifest.display()))?;
        let root = manifest
            .parent()
            .ok_or_else(|| "Registry manifest has no parent directory".to_string())?
            .to_path_buf();
        (bytes, RegistrySource::Local { root })
    };

    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "Registry manifest is not valid UTF-8".to_string())?;
    let registry = parse_registry(text)?;
    Ok(LoadedRegistry { registry, source })
}

pub fn parse_registry(content: &str) -> Result<Registry, String> {
    if content.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "Registry manifest exceeds the {} byte limit",
            MAX_MANIFEST_BYTES
        ));
    }
    let registry: Registry = serde_json::from_str(content)
        .map_err(|error| format!("Invalid registry manifest: {error}"))?;
    validate_registry(&registry)?;
    Ok(registry)
}

fn validate_registry(registry: &Registry) -> Result<(), String> {
    if registry.schema_version != 1 {
        return Err(format!(
            "Unsupported registry schema version: {}",
            registry.schema_version
        ));
    }
    validate_name(&registry.name, "registry")?;
    if registry.items.len() > MAX_ITEMS {
        return Err(format!("Registry contains more than {MAX_ITEMS} items"));
    }

    let mut item_names = HashSet::new();
    for item in &registry.items {
        validate_name(&item.name, "item")?;
        if !item_names.insert(item.name.to_ascii_lowercase()) {
            return Err(format!("Duplicate item name: {}", item.name));
        }
        if item.description.trim().is_empty() || item.description.len() > 1_024 {
            return Err(format!(
                "Item {} must have a description of 1 to 1024 bytes",
                item.name
            ));
        }
        if item.files.is_empty() || item.files.len() > MAX_FILES_PER_ITEM {
            return Err(format!(
                "Item {} must declare between 1 and {MAX_FILES_PER_ITEM} files",
                item.name
            ));
        }
        let mut targets = HashSet::new();
        for file in &item.files {
            validate_relative_path(&file.source, "source")?;
            let target = validate_relative_path(&file.target, "target")?;
            if !targets.insert(target.to_ascii_lowercase()) {
                return Err(format!(
                    "Duplicate destination path in {}: {}",
                    item.name, file.target
                ));
            }
            if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!(
                    "Invalid SHA-256 digest for {}: expected 64 hexadecimal characters",
                    file.target
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_relative_path(value: &str, field: &str) -> Result<String, String> {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(format!(
            "Invalid {field} path: expected a relative slash-separated path"
        ));
    }
    let segments: Vec<&str> = value.split('/').collect();
    if segments.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.ends_with('.')
            || is_windows_reserved_name(part)
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    }) {
        return Err(format!("Invalid {field} path: {value}"));
    }
    Ok(segments.join("/"))
}

fn is_windows_reserved_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        })
}

fn validate_name(name: &str, kind: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 || !name.is_ascii() {
        return Err(format!(
            "Invalid {kind} name: expected 1 to 128 ASCII characters"
        ));
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return Err(format!("Invalid {kind} name: {name}"));
    }
    if !bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
    }) {
        return Err(format!("Invalid {kind} name: {name}"));
    }
    Ok(())
}

impl Registry {
    pub fn find(&self, name: &str) -> Option<&RegistryItem> {
        self.items.iter().find(|item| item.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn manifest(items: &str) -> String {
        format!(r#"{{"schemaVersion":1,"name":"local-dev","items":{items}}}"#)
    }

    fn item(files: &str) -> String {
        format!(r#"{{"name":"sample","description":"Sample item","files":{files}}}"#)
    }

    fn file(source: &str, target: &str, hash: &str) -> String {
        format!(r#"{{"source":"{source}","target":"{target}","sha256":"{hash}"}}"#)
    }

    #[test]
    fn parses_valid_manifest_and_finds_item() {
        let json = manifest(&format!(
            "[{}]",
            item(&format!("[{}]", file("src/a.tsx", "src/a.tsx", HASH)))
        ));
        let registry = parse_registry(&json).unwrap();
        assert_eq!(registry.name, "local-dev");
        assert!(registry.find("sample").is_some());
    }

    #[test]
    fn rejects_unsupported_schema_missing_fields_and_bad_json() {
        assert!(
            parse_registry(r#"{"schemaVersion":99,"name":"x","items":[]}"#)
                .unwrap_err()
                .contains("Unsupported")
        );
        assert!(parse_registry(r#"{"schemaVersion":1,"name":"x"}"#).is_err());
        assert!(parse_registry("{").is_err());
    }

    #[test]
    fn rejects_invalid_names_and_oversized_manifests() {
        let invalid_registry_name = manifest("[]").replace("local-dev", "Not Valid");
        assert!(
            parse_registry(&invalid_registry_name)
                .unwrap_err()
                .contains("registry name")
        );
        let invalid_item_name = manifest(&format!(
            "[{}]",
            item(&format!("[{}]", file("src/a.tsx", "src/a.tsx", HASH)))
                .replace("sample", "Bad-Name")
        ));
        assert!(
            parse_registry(&invalid_item_name)
                .unwrap_err()
                .contains("item name")
        );
        let oversized = " ".repeat(MAX_MANIFEST_BYTES + 1);
        assert!(
            parse_registry(&oversized)
                .unwrap_err()
                .contains("byte limit")
        );
    }

    #[test]
    fn rejects_duplicate_items_targets_and_invalid_digests() {
        let valid = file("a.tsx", "same.tsx", HASH);
        let duplicates = manifest(&format!(
            "[{},{}]",
            item(&format!("[{valid}]")),
            item(&format!("[{valid}]"))
        ));
        assert!(
            parse_registry(&duplicates)
                .unwrap_err()
                .contains("Duplicate item")
        );
        let two_targets = manifest(&format!(
            "[{}]",
            item(&format!("[{},{}]", valid, file("b.tsx", "SAME.tsx", HASH)))
        ));
        assert!(
            parse_registry(&two_targets)
                .unwrap_err()
                .contains("Duplicate destination")
        );
        let bad_hash = manifest(&format!(
            "[{}]",
            item(&format!("[{}]", file("a.tsx", "a.tsx", "xyz")))
        ));
        assert!(parse_registry(&bad_hash).unwrap_err().contains("SHA-256"));
    }

    #[test]
    fn rejects_absolute_traversal_windows_and_url_sources() {
        for path in [
            "/tmp/file",
            "../outside",
            "a/../../b",
            r"..\outside",
            r"C:\\file",
            r"\\server\\share",
            "https://example.com/a",
            "NUL.txt",
            "folder./file.tsx",
        ] {
            assert!(
                validate_relative_path(path, "source").is_err(),
                "accepted {path}"
            );
        }
    }
}
