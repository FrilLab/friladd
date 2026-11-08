use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    pub schema_version: u32,
    pub name: String,
    pub items: Vec<RegistryItem>,
}

#[derive(Debug, Deserialize)]
pub struct RegistryItem {
    pub name: String,
    pub description: String,
}

pub fn load_registry(path: &Path) -> Result<Registry, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;

    parse_registry(&content)
}

fn parse_registry(content: &str) -> Result<Registry, String> {
    let registry: Registry =
        serde_json::from_str(content).map_err(|error| format!("Invalid registry JSON: {error}"))?;

    if registry.schema_version != 1 {
        return Err(format!(
            "Unsupported registry schema version: {}",
            registry.schema_version
        ));
    }

    Ok(registry)
}

impl Registry {
    pub fn find(&self, name: &str) -> Option<&RegistryItem> {
        self.items.iter().find(|item| item.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_registry() {
        let json = r#"{
            "schemaVersion": 1,
            "name": "test",
            "items": [
                {
                    "name": "envelope-reveal",
                    "description": "A test component"
                }
            ]
        }"#;

        let registry = parse_registry(json).unwrap();

        assert_eq!(registry.name, "test");
        assert_eq!(registry.items.len(), 1);
        assert!(registry.find("envelope-reveal").is_some());
    }

    #[test]
    fn rejects_unsupported_schema() {
        let json = r#"{
            "schemaVersion": 99,
            "name": "test",
            "items": []
        }"#;

        assert!(parse_registry(json).is_err());
    }
}
