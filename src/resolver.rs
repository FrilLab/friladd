use crate::{
    config::{self, Config},
    registry,
};
use std::path::{Path, PathBuf};

fn registered_alias_location(location: &str) -> bool {
    !location.contains("://")
        && config::validate_alias(location).is_ok()
        && matches!(
            Path::new(location).symlink_metadata(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
}

pub struct Resolution {
    pub manifest: String,
    pub item: Option<String>,
}
fn source_for(config: &Config, alias: &str, path: &Path) -> Result<String, String> {
    let entry = config
        .registries
        .get(alias)
        .ok_or_else(|| format!("Registry alias '{alias}' is not registered"))?;
    if entry.source.contains("://") {
        return Ok(entry.source.clone());
    }
    let source = PathBuf::from(&entry.source);
    let resolved = if source.is_absolute() {
        source
    } else {
        path.parent().unwrap_or(Path::new(".")).join(source)
    };
    Ok(resolved.to_string_lossy().into_owned())
}
pub fn resolve_add(
    component: &str,
    explicit: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<Resolution, String> {
    let parts: Vec<_> = component.split('/').collect();
    if parts.len() > 2 || parts.iter().any(|part| part.is_empty()) {
        return Err(format!(
            "Invalid component identifier '{component}'; use item or registry/item"
        ));
    }
    if parts.len() == 2 && explicit.is_some() {
        return Err(
            "A qualified registry/item identifier cannot be combined with --registry".into(),
        );
    }
    let item = if parts.len() == 2 { parts[1] } else { parts[0] };
    if let Some(location) = explicit {
        if registered_alias_location(location) {
            let project_config = config::load(project)?;
            let global_config = config::load(global)?;
            let (config, path) = if project_config.registries.contains_key(location) {
                (&project_config, project)
            } else if global_config.registries.contains_key(location) {
                (&global_config, global)
            } else {
                return Err(format!(
                    "Registry alias '{location}' is not registered in project or global configuration"
                ));
            };
            let manifest = source_for(config, location, path)?;
            registry::validate_registry_location(&manifest)?;
            return Ok(Resolution {
                manifest,
                item: Some(item.into()),
            });
        }
        registry::validate_registry_location(location)?;
        return Ok(Resolution {
            manifest: location.into(),
            item: Some(item.into()),
        });
    }
    let project_config = config::load(project)?;
    let global_config = config::load(global)?;
    let (alias, config, path) = if parts.len() == 2 {
        let alias = parts[0];
        config::validate_alias(alias)?;
        if project_config.registries.contains_key(alias) {
            (alias, &project_config, project)
        } else if global_config.registries.contains_key(alias) {
            (alias, &global_config, global)
        } else {
            return Err(format!(
                "Registry alias '{alias}' is not registered in project or global configuration"
            ));
        }
    } else if let Some(alias) = project_config.default_registry.as_deref() {
        (alias, &project_config, project)
    } else if let Some(alias) = global_config.default_registry.as_deref() {
        (alias, &global_config, global)
    } else {
        return Err("No default registry is configured.\n\nRegister a project registry:\n  friladd registry add <name> <url> --default\n\nOr register one globally:\n  friladd registry add <name> <url> --global --default".into());
    };
    let manifest = source_for(config, alias, path)?;
    registry::validate_registry_location(&manifest)?;
    Ok(Resolution {
        manifest,
        item: Some(item.into()),
    })
}
pub fn resolve_list(
    registry_arg: Option<&str>,
    alias: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<String, String> {
    if let Some(location) = registry_arg {
        if registered_alias_location(location) {
            let pc = config::load(project)?;
            let gc = config::load(global)?;
            let (config, path) = if pc.registries.contains_key(location) {
                (&pc, project)
            } else if gc.registries.contains_key(location) {
                (&gc, global)
            } else {
                return Err(format!("Registry alias '{location}' is not registered"));
            };
            let manifest = source_for(config, location, path)?;
            registry::validate_registry_location(&manifest)?;
            return Ok(manifest);
        }
        registry::validate_registry_location(location)?;
        return Ok(location.into());
    }
    let pc = config::load(project)?;
    let gc = config::load(global)?;
    if let Some(alias) = alias {
        config::validate_alias(alias)?;
        if pc.registries.contains_key(alias) {
            let s = source_for(&pc, alias, project)?;
            registry::validate_registry_location(&s)?;
            return Ok(s);
        }
        if gc.registries.contains_key(alias) {
            let s = source_for(&gc, alias, global)?;
            registry::validate_registry_location(&s)?;
            return Ok(s);
        }
        return Err(format!("Registry alias '{alias}' is not registered"));
    }
    if let Some(alias) = pc.default_registry.as_deref() {
        let s = source_for(&pc, alias, project)?;
        registry::validate_registry_location(&s)?;
        return Ok(s);
    }
    if let Some(alias) = gc.default_registry.as_deref() {
        let s = source_for(&gc, alias, global)?;
        registry::validate_registry_location(&s)?;
        return Ok(s);
    }
    Err("No default registry is configured. Use 'friladd registry list' to inspect registered registries.".into())
}
