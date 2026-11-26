use crate::{
    config::{self, Config, RegistryEntry, Scope},
    downloader, lockfile, planner, registry, resolver, transaction,
};
use clap::{Args, Parser, Subcommand};
use std::{env, fs, path::PathBuf, sync::Arc};

#[derive(Debug, Parser)]
#[command(
    name = "friladd",
    version,
    about = "Install selected source files from registries",
    after_help = "Example: friladd add sample --registry ./registry.json --dry-run"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Debug, Subcommand)]
enum Command {
    /// List items in a registry.
    #[command(after_help = "Example: friladd list --registry ./registry.json")]
    List(ListArgs),
    /// Install one item from a registry.
    #[command(after_help = "Example: friladd add local/sample --project ./app --dry-run")]
    Add(AddArgs),
    /// Manage named registry sources.
    #[command(after_help = "Example: friladd registry list --global")]
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
}
#[derive(Debug, Args)]
struct ListArgs {
    /// Registry alias (or use --registry for a path or URL).
    #[arg(conflicts_with = "registry")]
    registry_alias: Option<String>,
    /// Registered alias, local manifest path, or HTTPS manifest URL.
    #[arg(long, value_name = "ALIAS_OR_SOURCE")]
    registry: Option<String>,
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
}
#[derive(Debug, Args)]
struct AddArgs {
    /// Item name or registry/item.
    #[arg(value_name = "ITEM")]
    component: String,
    /// Registered alias, local manifest path, or HTTPS manifest URL.
    #[arg(long, value_name = "ALIAS_OR_SOURCE")]
    registry: Option<String>,
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Verify the installation plan and source bytes without writing.
    #[arg(long)]
    dry_run: bool,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Subcommand)]
enum RegistryCommand {
    /// Verify all declared source files and hashes without installing anything.
    #[command(after_help = "Example: friladd registry verify --registry ./registry.json")]
    Verify(ListArgs),
    /// Register and validate a manifest in the selected scope.
    #[command(after_help = "Example: friladd registry add local ./registry.json --default")]
    Add {
        alias: String,
        /// Local manifest path or HTTPS manifest URL.
        source: String,
        /// Use only global configuration.
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// Set this alias as the default in the selected scope.
        #[arg(long)]
        default: bool,
        /// Project root (defaults to the current directory).
        #[arg(long, value_name = "DIRECTORY")]
        project: Option<PathBuf>,
    },
    /// List configured registry aliases without fetching manifests.
    #[command(after_help = "Example: friladd registry list --project ./app")]
    List {
        /// Use only global configuration.
        #[arg(long, conflicts_with = "project")]
        global: bool,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Unregister an alias from the selected scope.
    #[command(after_help = "Example: friladd registry remove local")]
    Remove {
        alias: String,
        /// Use only global configuration.
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// Project root (defaults to the current directory).
        #[arg(long, value_name = "DIRECTORY")]
        project: Option<PathBuf>,
    },
    /// Set the default registry in the selected scope.
    #[command(after_help = "Example: friladd registry default local")]
    Default {
        alias: String,
        /// Use only global configuration.
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// Project root (defaults to the current directory).
        #[arg(long, value_name = "DIRECTORY")]
        project: Option<PathBuf>,
    },
}
pub fn run() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Command::List(a) => list(a),
        Command::Add(a) => add(a),
        Command::Registry { command } => manage(command),
    }
}
fn roots(project: Option<PathBuf>) -> Result<(PathBuf, PathBuf), String> {
    Ok((project_config_path(project)?, config::global_path()?))
}

fn project_config_path(project: Option<PathBuf>) -> Result<PathBuf, String> {
    let root = match project {
        Some(p) => p,
        None => {
            env::current_dir().map_err(|e| format!("Cannot determine current directory: {e}"))?
        }
    };
    if root.as_os_str().is_empty() {
        return Err("Project directory cannot be empty".into());
    }
    let root: PathBuf = root.components().collect();
    let exists = match fs::symlink_metadata(&root) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => true,
        Ok(_) => {
            return Err(format!(
                "Project path must be a non-symlink directory: {}",
                root.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "Cannot inspect project directory {}: {error}",
                root.display()
            ));
        }
    };
    let root = if exists {
        fs::canonicalize(&root)
            .map_err(|e| format!("Cannot resolve project directory {}: {e}", root.display()))?
    } else {
        let absolute = if root.is_absolute() {
            root
        } else {
            env::current_dir()
                .map_err(|e| format!("Cannot determine current directory: {e}"))?
                .join(root)
        };
        let parent = absolute
            .parent()
            .ok_or_else(|| format!("Project path has no parent: {}", absolute.display()))?;
        let parent = fs::canonicalize(parent)
            .map_err(|e| format!("Cannot resolve project parent {}: {e}", parent.display()))?;
        parent.join(
            absolute
                .file_name()
                .ok_or_else(|| "Project path has no final directory name".to_string())?,
        )
    };
    Ok(config::project_path(&root))
}
fn list(args: ListArgs) -> Result<(), String> {
    let (project, global) = roots(args.project)?;
    let location = resolver::resolve_list(
        args.registry.as_deref(),
        args.registry_alias.as_deref(),
        &project,
        &global,
    )?;
    let loaded = registry::load_registry(&location)?;
    println!("Available items ({}):", loaded.registry.name);
    let mut items: Vec<_> = loaded.registry.items.iter().collect();
    items.sort_unstable_by_key(|item| &item.name);
    for item in items {
        if loaded.v2.is_some() && item.description.is_empty() {
            println!("- {}", item.name);
        } else {
            println!("- {}: {}", item.name, item.description);
        }
    }
    Ok(())
}
fn add(args: AddArgs) -> Result<(), String> {
    let (project_config, global) = roots(args.project.clone())?;
    let project_root = project_config.parent().unwrap().to_path_buf();
    let resolution = resolver::resolve_add(
        &args.component,
        args.registry.as_deref(),
        &project_config,
        &global,
    )?;
    let loaded = registry::load_registry(&resolution.manifest)?;
    let item = loaded
        .registry
        .find(resolution.item)
        .ok_or_else(|| format!("Component not found: {}", resolution.item.escape_debug()))?;
    let v2_item = loaded
        .v2
        .as_ref()
        .map(|v2| v2.select(resolution.item, None))
        .transpose()?;
    if v2_item.is_some_and(|item| !item.dependencies.is_empty()) {
        return Err("This item declares dependencies; dependency installation is available in a later FrilAdd release.".into());
    }
    if !args.dry_run {
        transaction::recover(&project_root)?;
    }
    let provider_id;
    let manifest_source = match &loaded.source {
        registry::RegistrySource::Local { .. } => {
            let path = registry::canonical_local_manifest(&resolution.manifest)?;
            let url = url::Url::from_file_path(path)
                .map_err(|_| "Cannot represent manifest provenance as a file URL")?;
            provider_id = if let Some(v2) = &loaded.v2 {
                format!("v2:{}", v2.id)
            } else {
                lockfile::v1_provider_id(url.as_str())?
            };
            url
        }
        registry::RegistrySource::Remote { manifest_url } => {
            let mut url = manifest_url.clone();
            url.set_query(None);
            url.set_fragment(None);
            if let Some(v2) = &loaded.v2 {
                provider_id = format!("v2:{}", v2.id);
            } else {
                provider_id = lockfile::v1_provider_id(url.as_str())?;
            }
            url
        }
    };
    let item_id = v2_item.map_or(item.name.as_str(), |v2| v2.id.as_str());
    let target_root = if let Some(v2) = v2_item {
        let map = crate::registry_v2::TargetMap::load(&project_root)?;
        map.root(v2.kind, None)?
    } else {
        None
    };
    let mapped_item = crate::registry::RegistryItem {
        name: item.name.clone(),
        description: item.description.clone(),
        files: item
            .files
            .iter()
            .map(|f| crate::registry::RegistryFile {
                source: f.source.clone(),
                target: target_root
                    .as_ref()
                    .map_or_else(|| f.target.clone(), |root| format!("{root}/{}", f.target)),
                sha256: f.sha256.clone(),
            })
            .collect(),
    };
    let plan = planner::build_plan(&project_root, &mapped_item)?;
    let before = lockfile::load(&project_root)?;
    let manifest_digest = loaded.manifest_sha256.clone();
    let manifest_locator = manifest_source.to_string();
    let file_records = plan
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let resolved_source = manifest_source
                .join(&file.source)
                .map_err(|_| "Invalid resolved artifact locator")?;
            Ok(lockfile::OwnedFile {
                source: file.source.clone(),
                resolved_source: resolved_source.to_string(),
                declared_target: item.files[index].target.clone(),
                target: file.target_display.clone(),
                expected_sha256: file.expected_sha256.to_ascii_lowercase(),
                installed: lockfile::Content {
                    sha256: file.expected_sha256.to_ascii_lowercase(),
                    size: v2_item
                        .and_then(|v| {
                            v.files
                                .iter()
                                .find(|f| f.source == file.source)
                                .map(|f| f.size)
                        })
                        .unwrap_or(0),
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut next = before.lock().clone();
    if !next.providers.iter().any(|p| p.provider_id == provider_id) {
        next.providers.push(lockfile::Provider {
            provider_id: provider_id.clone(),
            declared_source: manifest_locator.clone(),
            resolved_source: manifest_locator.clone(),
            manifest_sha256: manifest_digest.clone(),
        });
    }
    let same = next
        .items
        .iter()
        .find(|i| i.provider_id == provider_id && i.item_id == item_id)
        .cloned();
    let mut new_item = lockfile::Item {
        provider_id: provider_id.clone(),
        item_id: item_id.to_string(),
        version: v2_item.map(|v| v.version.clone()),
        manifest_source: manifest_locator,
        manifest_sha256: manifest_digest,
        target_root: target_root.clone(),
        dependencies: Vec::new(),
        files: file_records,
    };
    if let Some(existing) = same.as_ref() {
        let mut comparable = new_item.clone();
        if comparable.files.len() == existing.files.len() {
            for candidate in &mut comparable.files {
                if let Some(installed) = existing
                    .files
                    .iter()
                    .find(|record| record.target == candidate.target)
                {
                    candidate.installed = installed.installed.clone();
                    candidate.resolved_source = installed.resolved_source.clone();
                }
            }
            comparable.files.sort_by(|a, b| a.target.cmp(&b.target));
        }
        if existing == &comparable {
            let unchanged = existing.files.iter().all(|file| {
                transaction::inspect(&project_root, &file.target)
                    .ok()
                    .flatten()
                    .as_ref()
                    == Some(&file.installed)
            });
            if unchanged {
                if args.json {
                    print_add_json(
                        &plan,
                        &provider_id,
                        item_id,
                        v2_item,
                        &loaded.manifest_sha256,
                        false,
                        "unchanged",
                    );
                } else {
                    println!("Already installed {} (no changes).", item.name);
                }
                return Ok(());
            }
        }
        return Err(format!(
            "Managed item {} has drift or a different snapshot; run `friladd status` and use `restore` or `update`.",
            item.name
        ));
    }
    let owners = before.lock().owners();
    for file in &plan.files {
        if let Some((owner, _)) = owners.get(file.target_display.as_str())
            && (owner.provider_id != provider_id || owner.item_id != item_id)
        {
            return Err(format!(
                "Destination {} is owned by {}/{}",
                file.target_display, owner.provider_id, owner.item_id
            ));
        }
    }
    if !args.json {
        print_plan(&plan, &loaded.registry.name, item.name.as_str());
    }
    if !plan.conflicts.is_empty() {
        return Err(format!(
            "{} destination file(s) are already present and are not owned by this item",
            plan.conflicts.len()
        ));
    }
    let contents = if loaded.v2.is_some() {
        downloader::acquire_v2_files(&loaded.source, &plan)?
    } else {
        downloader::acquire_files(&loaded.source, &plan)?
    };
    for (file, acquired) in plan.files.iter().zip(&contents) {
        let installed = lockfile::Content::of(&acquired.bytes);
        if let Some(locator) = &acquired.resolved_locator
            && let Some(record) = new_item
                .files
                .iter_mut()
                .find(|record| record.target == file.target_display)
        {
            record.resolved_source = locator.clone();
        }
        if v2_item.is_some_and(|v| {
            v.files
                .iter()
                .find(|f| f.source == file.source)
                .is_some_and(|f| f.size != installed.size)
        }) {
            return Err(format!("Size mismatch for {}", file.source));
        }
        let record = new_item
            .files
            .iter_mut()
            .find(|record| record.target == file.target_display)
            .ok_or_else(|| {
                format!(
                    "Installation plan has no ownership record for {}",
                    file.target_display
                )
            })?;
        record.installed = installed;
    }
    next.items.push(new_item);
    let mut edits = Vec::with_capacity(plan.files.len());
    for (file, acquired) in plan.files.iter().zip(contents) {
        edits.push(transaction::Edit {
            target: file.target_display.clone(),
            before: None,
            write: Some(transaction::WriteSource::Shared(Arc::clone(
                &acquired.bytes,
            ))),
        });
    }
    let transaction = transaction::Plan::new(before, next, edits)?;
    transaction.check(&project_root)?;
    if args.dry_run {
        if args.json {
            print_add_json(
                &plan,
                &provider_id,
                item_id,
                v2_item,
                &loaded.manifest_sha256,
                true,
                "install",
            );
        } else {
            for file in &plan.files {
                println!("Will install {}", file.target_display);
            }
            println!(
                "Dry run: no project files were changed. Ownership would be recorded in friladd.lock.json."
            );
        }
        return Ok(());
    }
    transaction::apply(&project_root, transaction)?;
    if args.json {
        print_add_json(
            &plan,
            &provider_id,
            item_id,
            v2_item,
            &loaded.manifest_sha256,
            false,
            "install",
        );
    } else {
        for file in &plan.files {
            println!("Installed {}", file.target_display);
        }
    }
    Ok(())
}

fn print_add_json(
    plan: &planner::InstallationPlan,
    provider_id: &str,
    item_id: &str,
    v2_item: Option<&crate::registry_v2::Item>,
    manifest_sha256: &str,
    dry_run: bool,
    action: &str,
) {
    let mut files: Vec<_> = plan
        .files
        .iter()
        .map(|file| {
            serde_json::json!({
                "target": file.target_display,
                "action": action,
                "ownerBefore": if action == "unchanged" { Some(serde_json::json!({ "providerId": provider_id, "itemId": item_id })) } else { None },
                "ownerAfter": { "providerId": provider_id, "itemId": item_id },
            })
        })
        .collect();
    files.sort_by(|a, b| a["target"].as_str().cmp(&b["target"].as_str()));
    println!(
        "{}",
        serde_json::json!({
            "schemaVersion": 1,
            "command": "add",
            "data": {
                "dryRun": dry_run,
                "item": {
                    "providerId": provider_id,
                    "itemId": item_id,
                    "version": v2_item.map(|item| item.version.as_str()),
                    "manifestSha256": manifest_sha256,
                },
                "files": files,
            }
        })
    );
}
fn verify_registry(args: ListArgs) -> Result<(), String> {
    let (project, global) = roots(args.project)?;
    let location = resolver::resolve_list(
        args.registry.as_deref(),
        args.registry_alias.as_deref(),
        &project,
        &global,
    )?;
    let loaded = registry::load_registry(&location)?;
    let count = if let Some(v2) = &loaded.v2 {
        downloader::verify_v2_files(&loaded.source, v2)?
    } else {
        downloader::verify_registry_files(&loaded.source, &loaded.registry)?
    };
    println!(
        "Verified registry {}: {} item(s), {} file(s).",
        loaded.registry.name,
        loaded.registry.items.len(),
        count
    );
    if loaded.v2.is_some() {
        println!("Manifest SHA-256: {}", loaded.manifest_sha256);
    }
    Ok(())
}
fn manage(command: RegistryCommand) -> Result<(), String> {
    match command {
        RegistryCommand::Verify(args) => verify_registry(args),
        RegistryCommand::Add {
            alias,
            source,
            global,
            default,
            project,
        } => {
            config::validate_alias(&alias)?;
            let path = config_path(global, project)?;
            let scope = if global {
                Scope::Global
            } else {
                Scope::Project
            };
            let mut config = config::load(&path)?;
            if config.registries.contains_key(&alias) {
                return Err(format!(
                    "Registry alias '{alias}' already exists in {} configuration",
                    scope.label()
                ));
            }
            let location = if source.contains("://") {
                registry::load_registry(&source)?;
                source
            } else {
                let manifest = registry::canonical_local_manifest(&source)?;
                registry::load_registry(manifest.to_str().ok_or("Registry path is not UTF-8")?)?;
                let stored = if global {
                    manifest
                } else {
                    pathdiff::diff_paths(&manifest, path.parent().unwrap()).unwrap_or(manifest)
                };
                stored
                    .into_os_string()
                    .into_string()
                    .map_err(|_| "Registry path is not UTF-8")?
            };
            config
                .registries
                .insert(alias.clone(), RegistryEntry { source: location });
            if default {
                config.default_registry = Some(alias.clone());
            }
            config::write(&path, &config)?;
            println!(
                "Registered '{alias}' in {} configuration{}.",
                scope.label(),
                if default {
                    " and set it as default"
                } else {
                    ""
                }
            );
            Ok(())
        }
        RegistryCommand::List {
            global,
            project: project_arg,
        } => {
            if global {
                let gp = config::global_path()?;
                let gc = config::load(&gp)?;
                print_registry_entries(&gc, "global");
            } else {
                let (pp, gp) = roots(project_arg)?;
                let pc = config::load(&pp)?;
                let gc = config::load(&gp)?;
                print_registry_entries(&pc, "project");
                for (a, e) in &gc.registries {
                    if !pc.registries.contains_key(a) {
                        println!(
                            "{a}\t{}\tglobal{}",
                            registry::display_location(&e.source),
                            if gc.default_registry.as_deref() == Some(a) {
                                "\tdefault"
                            } else {
                                ""
                            }
                        );
                    }
                }
            }
            Ok(())
        }
        RegistryCommand::Remove {
            alias,
            global,
            project,
        } => {
            config::validate_alias(&alias)?;
            let path = config_path(global, project)?;
            let mut c = config::load(&path)?;
            if c.registries.remove(&alias).is_none() {
                return Err(format!(
                    "Registry alias '{alias}' is not registered in {} configuration",
                    if global { "global" } else { "project" }
                ));
            }
            let cleared = c.default_registry.as_deref() == Some(alias.as_str());
            if cleared {
                c.default_registry = None;
            }
            config::write(&path, &c)?;
            println!(
                "Removed '{alias}' from {} configuration{}.",
                if global { "global" } else { "project" },
                if cleared {
                    "; cleared its default selection"
                } else {
                    ""
                }
            );
            Ok(())
        }
        RegistryCommand::Default {
            alias,
            global,
            project,
        } => {
            config::validate_alias(&alias)?;
            let path = config_path(global, project)?;
            let mut c = config::load(&path)?;
            if !c.registries.contains_key(&alias) {
                return Err(format!(
                    "Registry alias '{alias}' is not registered in {} configuration",
                    if global { "global" } else { "project" }
                ));
            }
            c.default_registry = Some(alias.clone());
            config::write(&path, &c)?;
            println!(
                "Default registry for {} configuration is now '{alias}'.",
                if global { "global" } else { "project" }
            );
            Ok(())
        }
    }
}
fn config_path(global: bool, project: Option<PathBuf>) -> Result<PathBuf, String> {
    if global {
        config::global_path()
    } else {
        project_config_path(project)
    }
}

fn print_registry_entries(c: &Config, scope: &str) {
    for (a, e) in &c.registries {
        println!(
            "{a}\t{}\t{scope}{}",
            registry::display_location(&e.source),
            if c.default_registry.as_deref() == Some(a) {
                "\tdefault"
            } else {
                ""
            }
        );
    }
}
fn print_plan(plan: &planner::InstallationPlan, registry_name: &str, component: &str) {
    println!(
        "FrilAdd installation plan\n\nRegistry: {registry_name}\nComponent: {component}\nProject: {}\n\nFiles:",
        plan.project_root.display()
    );
    for f in &plan.files {
        if f.exists {
            println!("  ! {} (already exists)", f.target_display)
        } else {
            println!("  + {}", f.target_display)
        }
    }
    println!("\n{} file(s) planned.", plan.files.len());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clap_help_and_version_are_available() {
        let help = Cli::try_parse_from(["friladd", "--help"])
            .unwrap_err()
            .to_string();
        assert!(help.contains("Install selected source files"));
        let version = Cli::try_parse_from(["friladd", "--version"])
            .unwrap_err()
            .to_string();
        assert!(version.contains(env!("CARGO_PKG_VERSION")));
    }
}
