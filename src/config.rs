use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const CONFIG_FILE: &str = ".friladd.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_registry: Option<String>,
    #[serde(default)]
    pub registries: BTreeMap<String, RegistryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Project,
    Global,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

pub fn validate_alias(alias: &str) -> Result<(), String> {
    if alias.is_empty() || alias.len() > 128 || !alias.is_ascii() {
        return Err(format!(
            "Invalid registry alias '{alias}': use 1 to 128 lowercase ASCII letters, digits, '.', '_' or '-'"
        ));
    }
    let bytes = alias.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit()
        || !bytes.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
        || alias == "."
        || alias == ".."
    {
        return Err(format!(
            "Invalid registry alias '{alias}': use 1 to 128 lowercase ASCII letters, digits, '.', '_' or '-'"
        ));
    }
    Ok(())
}

pub fn validate_config(config: &Config) -> Result<(), String> {
    if config.schema_version != 1 {
        return Err(format!(
            "Unsupported FrilAdd configuration schema version: {}",
            config.schema_version
        ));
    }
    for (alias, entry) in &config.registries {
        validate_alias(alias)?;
        if entry.source.trim().is_empty() {
            return Err(format!("Registry '{alias}' has an empty source"));
        }
    }
    if let Some(default) = &config.default_registry {
        validate_alias(default)?;
        if !config.registries.contains_key(default) {
            return Err(format!("Default registry '{default}' is not registered"));
        }
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<Config, String> {
    if !path.exists() {
        return Ok(Config {
            schema_version: 1,
            ..Config::default()
        });
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect configuration {}: {e}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "Configuration path must be a regular non-symlink file: {}",
            path.display()
        ));
    }
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Cannot read configuration {}: {e}", path.display()))?;
    let config: Config = serde_json::from_str(&text)
        .map_err(|e| format!("Invalid configuration {}: {e}", path.display()))?;
    validate_config(&config)
        .map_err(|e| format!("Invalid configuration {}: {e}", path.display()))?;
    Ok(config)
}

pub fn write(path: &Path, config: &Config) -> Result<(), String> {
    validate_config(config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Cannot create configuration directory {}: {e}",
                parent.display()
            )
        })?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(format!(
                "Configuration path must be a regular non-symlink file: {}",
                path.display()
            ));
        }
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => {
            return Err(format!(
                "Cannot inspect configuration {}: {error}",
                path.display()
            ));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("Configuration has no parent directory: {}", path.display()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
        format!(
            "Cannot create temporary configuration in {}: {e}",
            parent.display()
        )
    })?;
    serde_json::to_writer_pretty(&mut temp, config)
        .map_err(|e| format!("Cannot serialize configuration: {e}"))?;
    temp.write_all(b"\n")
        .map_err(|e| format!("Cannot write temporary configuration: {e}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("Cannot sync temporary configuration: {e}"))?;
    // Recheck immediately before atomic replacement to avoid following a swapped symlink.
    if let Ok(meta) = fs::symlink_metadata(path)
        && (meta.file_type().is_symlink() || !meta.is_file())
    {
        return Err(format!(
            "Configuration path changed to an unsafe file: {}",
            path.display()
        ));
    }
    temp.persist(path).map_err(|e| {
        format!(
            "Cannot atomically write configuration {}: {}",
            path.display(),
            e.error
        )
    })?;
    Ok(())
}

pub fn project_path(root: &Path) -> PathBuf {
    root.join(CONFIG_FILE)
}
pub fn global_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("FRILADD_CONFIG_DIR") {
        return Ok(PathBuf::from(path).join(CONFIG_FILE));
    }
    ProjectDirs::from("org", "FrilLab", "FrilAdd")
        .map(|dirs| dirs.config_dir().join(CONFIG_FILE))
        .ok_or_else(|| "Cannot determine the platform configuration directory".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_config_and_default_reference() {
        let mut c = Config {
            schema_version: 1,
            ..Config::default()
        };
        validate_config(&c).unwrap();
        c.default_registry = Some("missing".into());
        assert!(validate_config(&c).is_err());
        c.default_registry = None;
        c.registries
            .insert("bad/name".into(), RegistryEntry { source: "x".into() });
        assert!(validate_config(&c).is_err());
    }
    #[test]
    fn malformed_and_unsupported_configs_are_rejected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(CONFIG_FILE);
        fs::write(&p, "{").unwrap();
        assert!(load(&p).is_err());
        fs::write(&p, r#"{"schemaVersion":2,"registries":{}}"#).unwrap();
        assert!(load(&p).unwrap_err().contains("Unsupported"));
    }
    #[test]
    fn writes_and_reads_config() {
        let d = tempfile::tempdir().unwrap();
        let p = project_path(d.path());
        let mut c = Config {
            schema_version: 1,
            ..Config::default()
        };
        c.registries.insert(
            "test".into(),
            RegistryEntry {
                source: "./registry.json".into(),
            },
        );
        write(&p, &c).unwrap();
        assert_eq!(load(&p).unwrap().registries.len(), 1);
    }
}
