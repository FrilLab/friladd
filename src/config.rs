use directories::ProjectDirs;
use serde::{Deserialize, Serialize, de};
use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const CONFIG_FILE: &str = ".friladd.json";
const MAX_CONFIG_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_registry: Option<String>,
    #[serde(default, deserialize_with = "deserialize_registries")]
    pub registries: BTreeMap<String, RegistryEntry>,
}

fn deserialize_registries<'de, D: de::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, RegistryEntry>, D::Error> {
    struct UniqueRegistries;
    impl<'de> de::Visitor<'de> for UniqueRegistries {
        type Value = BTreeMap<String, RegistryEntry>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object of unique registry aliases")
        }

        fn visit_map<M: de::MapAccess<'de>>(self, mut entries: M) -> Result<Self::Value, M::Error> {
            let mut registries = BTreeMap::new();
            while let Some((alias, entry)) = entries.next_entry()? {
                if registries.insert(alias, entry).is_some() {
                    return Err(de::Error::custom("duplicate registry alias"));
                }
            }
            Ok(registries)
        }
    }
    deserializer.deserialize_map(UniqueRegistries)
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
    let bytes = alias.as_bytes();
    if bytes.len() > 128
        || !bytes
            .first()
            .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        || !bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err(
            "Invalid registry alias: use 1 to 128 lowercase ASCII letters, digits, '.', '_' or '-'"
                .into(),
        );
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
    validate_config_directory(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config {
                schema_version: 1,
                ..Config::default()
            });
        }
        Err(error) => {
            return Err(format!(
                "Cannot inspect configuration {}: {error}",
                path.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "Configuration path must be a regular non-symlink file: {}",
            path.display()
        ));
    }
    let file = fs::File::open(path)
        .map_err(|e| format!("Cannot open configuration {}: {e}", path.display()))?;
    let bytes = crate::downloader::read_bounded(file, MAX_CONFIG_BYTES, "configuration")?;
    let config: Config = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Invalid configuration {}: {e}", path.display()))?;
    validate_config(&config)
        .map_err(|e| format!("Invalid configuration {}: {e}", path.display()))?;
    Ok(config)
}

fn validate_config_directory(path: &Path) -> Result<(), String> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(format!(
            "Cannot inspect configuration: expected a non-symlink directory at {}",
            parent.display()
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Cannot inspect configuration directory {}: {error}",
            parent.display()
        )),
    }
}

pub fn write(path: &Path, config: &Config) -> Result<(), String> {
    validate_config(config)?;
    validate_config_directory(path)?;
    let mut bytes = serde_json::to_vec_pretty(config)
        .map_err(|e| format!("Cannot serialize configuration: {e}"))?;
    bytes.push(b'\n');
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(format!(
            "Configuration exceeds the {MAX_CONFIG_BYTES} byte limit"
        ));
    }
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
    temp.write_all(&bytes)
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

    #[cfg(unix)]
    #[test]
    fn broken_symlink_is_not_treated_as_missing_configuration() {
        use std::os::unix::fs::symlink;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(CONFIG_FILE);
        symlink(d.path().join("missing-target"), &p).unwrap();
        assert!(load(&p).unwrap_err().contains("non-symlink"));
    }

    #[test]
    fn filesystem_errors_are_not_treated_as_missing_configuration() {
        let d = tempfile::tempdir().unwrap();
        let parent = d.path().join("file");
        fs::write(&parent, "not a directory").unwrap();
        let error = load(&parent.join(CONFIG_FILE)).unwrap_err();
        assert!(error.contains("Cannot inspect configuration"));
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
