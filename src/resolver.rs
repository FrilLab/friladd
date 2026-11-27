use crate::{config, config::Config};
use std::path::{Path, PathBuf};

pub const MAX_EFFECTIVE_REGISTRIES: usize = 64;

pub struct Resolution<'a> {
    pub manifest: String,
    pub item: &'a str,
}

pub struct RegistrySelection {
    pub manifest: String,
    pub alias: Option<String>,
}

/// Resolve configured aliases in deterministic order, with project entries shadowing globals.
pub fn effective_registries(
    project: &Path,
    global: &Path,
) -> Result<Vec<(String, String)>, String> {
    let project_config = config::load(project)?;
    let global_config = config::load(global)?;
    let mut sources = std::collections::BTreeMap::new();
    for (alias, entry) in global_config.registries {
        sources.insert(
            alias,
            (
                entry.source,
                global.parent().unwrap_or(Path::new(".")).to_path_buf(),
            ),
        );
    }
    for (alias, entry) in project_config.registries {
        sources.insert(
            alias,
            (
                entry.source,
                project.parent().unwrap_or(Path::new(".")).to_path_buf(),
            ),
        );
    }
    if sources.len() > MAX_EFFECTIVE_REGISTRIES {
        return Err(format!(
            "Multi-registry operation is limited to {MAX_EFFECTIVE_REGISTRIES} effective registries"
        ));
    }
    sources
        .into_iter()
        .map(|(alias, (source, base))| {
            let path = if source.contains("://") || Path::new(&source).is_absolute() {
                PathBuf::from(&source)
            } else {
                base.join(source)
            };
            Ok((
                alias,
                path.into_os_string()
                    .into_string()
                    .map_err(|_| "Registry path is not UTF-8")?,
            ))
        })
        .collect()
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
        "No default registry is configured.\n\nRegister a project registry:\n  fril registry add <name> <url> --default\n\nOr register one globally:\n  fril registry add <name> <url> --global --default".to_string()
    })?;
    Ok(Resolution { manifest, item })
}

pub fn resolve_list(
    explicit: Option<&str>,
    alias: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<String, String> {
    Ok(resolve_list_selection(explicit, alias, project, global)?.manifest)
}

pub fn resolve_list_selection(
    explicit: Option<&str>,
    alias: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<RegistrySelection, String> {
    resolve_registry_selection(explicit, alias, project, global)?.ok_or_else(|| {
        "No default registry is configured. Use 'fril registry list' to inspect registered registries.".into()
    })
}

pub fn resolve_optional_default(
    project: &Path,
    global: &Path,
) -> Result<Option<RegistrySelection>, String> {
    resolve_registry_selection(None, None, project, global)
}

fn resolve_registry(
    explicit: Option<&str>,
    alias: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<Option<String>, String> {
    Ok(
        resolve_registry_selection(explicit, alias, project, global)?
            .map(|selection| selection.manifest),
    )
}

pub fn resolve_registry_selection(
    explicit: Option<&str>,
    alias: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<Option<RegistrySelection>, String> {
    if let Some(location) = explicit
        && !registered_alias_location(location)
    {
        return Ok(Some(RegistrySelection {
            manifest: location.into(),
            alias: None,
        }));
    }
    let project_config = config::load(project)?;
    let global_config = config::load(global)?;
    let scopes = [(&project_config, project), (&global_config, global)];
    if let Some(alias) = alias.or(explicit) {
        config::validate_alias(alias)?;
        for (config, path) in scopes {
            if config.registries.contains_key(alias) {
                return source_for(config, alias, path).map(|manifest| {
                    Some(RegistrySelection {
                        manifest,
                        alias: Some(alias.into()),
                    })
                });
            }
        }
        return Err(format!("Registry alias '{alias}' is not registered"));
    }
    for (config, path) in scopes {
        if let Some(alias) = config.default_registry.as_deref() {
            return source_for(config, alias, path).map(|manifest| {
                Some(RegistrySelection {
                    manifest,
                    alias: Some(alias.into()),
                })
            });
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
