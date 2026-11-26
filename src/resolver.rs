use crate::{config, config::Config};
use std::path::{Path, PathBuf};

pub struct Resolution<'a> {
    pub manifest: String,
    pub item: &'a str,
}

pub fn resolve_add<'a>(
    component: &'a str,
    explicit: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<Resolution<'a>, String> {
    let (alias, item) = match component.split_once('/') {
        Some((alias, item)) if !alias.is_empty() && !item.is_empty() && !item.contains('/') => {
            (Some(alias), item)
        }
        None if !component.is_empty() => (None, component),
        _ => return Err("Invalid component identifier; use item or registry/item".into()),
    };
    if alias.is_some() && explicit.is_some() {
        return Err(
            "A qualified registry/item identifier cannot be combined with --registry".into(),
        );
    }
    let manifest = resolve_registry(explicit, alias, project, global)?.ok_or_else(|| {
        "No default registry is configured.\n\nRegister a project registry:\n  friladd registry add <name> <url> --default\n\nOr register one globally:\n  friladd registry add <name> <url> --global --default".to_string()
    })?;
    Ok(Resolution { manifest, item })
}

pub fn resolve_list(
    explicit: Option<&str>,
    alias: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<String, String> {
    resolve_registry(explicit, alias, project, global)?.ok_or_else(|| {
        "No default registry is configured. Use 'friladd registry list' to inspect registered registries.".into()
    })
}

fn resolve_registry(
    explicit: Option<&str>,
    alias: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<Option<String>, String> {
    if let Some(location) = explicit
        && !registered_alias_location(location)
    {
        return Ok(Some(location.into()));
    }
    let project_config = config::load(project)?;
    let global_config = config::load(global)?;
    let scopes = [(&project_config, project), (&global_config, global)];
    if let Some(alias) = alias.or(explicit) {
        config::validate_alias(alias)?;
        for (config, path) in scopes {
            if config.registries.contains_key(alias) {
                return source_for(config, alias, path).map(Some);
            }
        }
        return Err(format!("Registry alias '{alias}' is not registered"));
    }
    for (config, path) in scopes {
        if let Some(alias) = config.default_registry.as_deref() {
            return source_for(config, alias, path).map(Some);
        }
    }
    Ok(None)
}

fn registered_alias_location(location: &str) -> bool {
    config::validate_alias(location).is_ok()
        && matches!(
            Path::new(location).symlink_metadata(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
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
    resolved
        .into_os_string()
        .into_string()
        .map_err(|_| "Registry path is not UTF-8".into())
}
