use crate::{
    config::{self, Config, RegistryEntry, Scope},
    dependency, downloader, lockfile, planner, registry, resolver, transaction,
};
use clap::{Args, Parser, Subcommand};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

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
    /// Search items in the selected registry.
    #[command(after_help = "Example: friladd search model --all-registries --json")]
    Search(SearchArgs),
    /// Show metadata for one item.
    #[command(after_help = "Example: friladd info team-ui/button --json")]
    Info(InfoArgs),
    /// Diagnose project distribution state.
    #[command(after_help = "Example: friladd doctor --project ./app --json")]
    Doctor(DoctorArgs),
    /// Install one item from a registry.
    #[command(after_help = "Example: friladd add local/sample --project ./app --dry-run")]
    Add(AddArgs),
    /// List managed resources from the project lock.
    #[command(after_help = "Example: friladd installed --project ./app --json")]
    Installed(InstalledArgs),
    /// Check managed file integrity without network access.
    #[command(after_help = "Example: friladd status registry/item --project ./app --check --json")]
    Status(StatusArgs),
    /// Update one managed item from an explicitly selected candidate registry.
    #[command(
        after_help = "Example: friladd update registry/item --registry ./candidate.json --project ./app --dry-run"
    )]
    Update(UpdateArgs),
    /// Remove one managed item without deleting locally modified files.
    #[command(after_help = "Example: friladd remove registry/item --project ./app --dry-run")]
    Remove(ManagedMutationArgs),
    /// Restore missing managed files from their exact locked sources.
    #[command(after_help = "Example: friladd restore --project ./app --dry-run")]
    Restore(RestoreArgs),
    /// Manage named registry sources.
    #[command(after_help = "Example: friladd registry list --global")]
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
}
#[derive(Debug, Args)]
struct InstalledArgs {
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct StatusArgs {
    /// Item name or registry/item selector.
    #[arg(value_name = "ITEM")]
    item: Option<String>,
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Return exit status 2 when drift is reported.
    #[arg(long)]
    check: bool,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct UpdateArgs {
    /// Installed item name or registry/item selector.
    #[arg(value_name = "ITEM")]
    item: String,
    /// Explicit candidate alias, local manifest path, or HTTPS manifest URL.
    #[arg(long, value_name = "ALIAS_OR_SOURCE")]
    registry: String,
    /// Require this exact Registry v2 version from the selected snapshot.
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Permit an explicit Registry v2 downgrade.
    #[arg(long)]
    allow_downgrade: bool,
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Verify the transaction plan without writing.
    #[arg(long)]
    dry_run: bool,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct ManagedMutationArgs {
    /// Installed item name or registry/item selector (restore may omit it).
    #[arg(value_name = "ITEM")]
    item: String,
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Verify the transaction plan without writing.
    #[arg(long)]
    dry_run: bool,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct RestoreArgs {
    /// Installed item name or registry/item selector; omit to restore all.
    #[arg(value_name = "ITEM")]
    item: Option<String>,
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Verify the transaction plan without writing.
    #[arg(long)]
    dry_run: bool,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct ListArgs {
    /// Registry alias (or use --registry for a path or URL).
    #[arg(conflicts_with_all = ["registry", "all_registries"])]
    registry_alias: Option<String>,
    /// Registered alias, local manifest path, or HTTPS manifest URL.
    #[arg(long, value_name = "ALIAS_OR_SOURCE")]
    registry: Option<String>,
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Search every configured effective registry.
    #[arg(long, conflicts_with_all = ["registry", "registry_alias"])]
    all_registries: bool,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct SearchArgs {
    /// Case-insensitive text matched against item ID, name, description, tags, and kind.
    query: String,
    #[arg(
        long,
        value_name = "ALIAS_OR_SOURCE",
        conflicts_with = "all_registries"
    )]
    registry: Option<String>,
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    #[arg(long)]
    all_registries: bool,
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct RegistryVerifyArgs {
    #[arg(conflicts_with = "registry")]
    registry_alias: Option<String>,
    #[arg(long, value_name = "ALIAS_OR_SOURCE")]
    registry: Option<String>,
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct DoctorArgs {
    /// Project root (defaults to the current directory).
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    /// Check configured registry manifests with bounded requests.
    #[arg(long)]
    online: bool,
    /// Emit versioned JSON output.
    #[arg(long)]
    json: bool,
}
#[derive(Debug, Args)]
struct InfoArgs {
    /// Item name or registry/item.
    item: String,
    #[arg(long, value_name = "ALIAS_OR_SOURCE")]
    registry: Option<String>,
    #[arg(long, value_name = "DIRECTORY")]
    project: Option<PathBuf>,
    #[arg(long)]
    json: bool,
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
    Verify(RegistryVerifyArgs),
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
        /// Emit versioned JSON output.
        #[arg(long)]
        json: bool,
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
        Command::Search(a) => search(a),
        Command::Info(a) => info(a),
        Command::Doctor(a) => doctor(a),
        Command::Add(a) => add(a),
        Command::Installed(a) => installed(a),
        Command::Status(a) => status(a),
        Command::Update(a) => update(a),
        Command::Remove(a) => remove(a),
        Command::Restore(a) => restore(a),
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
    if args.all_registries {
        let registries = resolver::effective_registries(&project, &global)?;
        let mut items = Vec::new();
        for (alias, location) in registries {
            let loaded = registry::load_registry(&location)
                .map_err(|error| format!("Registry '{alias}' failed: {error}"))?;
            items.extend(list_item_json(&loaded, Some(&alias)));
        }
        items.sort_by(|a, b| {
            (a["provider"].as_str(), a["id"].as_str())
                .cmp(&(b["provider"].as_str(), b["id"].as_str()))
        });
        if args.json {
            print_catalog_json("list", serde_json::json!({"items": items}));
        } else {
            print_catalog_human(&items);
        }
        return Ok(());
    }
    let selection = resolver::resolve_list_selection(
        args.registry.as_deref(),
        args.registry_alias.as_deref(),
        &project,
        &global,
    )?;
    let loaded = registry::load_registry(&selection.manifest)?;
    let mut items = list_item_json(&loaded, selection.alias.as_deref());
    items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    if args.json {
        print_catalog_json(
            "list",
            serde_json::json!({"registry": loaded.registry.name, "items": items}),
        );
        return Ok(());
    }
    println!("Available items ({}):", loaded.registry.name);
    let mut items: Vec<_> = loaded.registry.items.iter().collect();
    items.sort_unstable_by_key(|item| &item.name);
    for item in items {
        if loaded.v2.is_some() && item.description.is_empty() {
            println!("- {}", item.name);
        } else {
            println!("- {}: {}", item.name, terminal_safe(&item.description));
        }
    }
    Ok(())
}
fn search(args: SearchArgs) -> Result<(), String> {
    if args.query.trim().is_empty() {
        return Err("Search query cannot be empty".into());
    }
    let (project, global) = roots(args.project)?;
    let registries = if args.all_registries {
        resolver::effective_registries(&project, &global)?
            .into_iter()
            .map(|(alias, location)| (Some(alias), location))
            .collect::<Vec<_>>()
    } else {
        {
            let selection = resolver::resolve_list_selection(
                args.registry.as_deref(),
                None,
                &project,
                &global,
            )?;
            vec![(selection.alias, selection.manifest)]
        }
    };
    let query = args.query.to_lowercase();
    let mut items = Vec::new();
    for (alias, location) in registries {
        let loaded = registry::load_registry(&location).map_err(|error| {
            format!(
                "Registry '{}' failed: {error}",
                alias.as_deref().unwrap_or("selected registry")
            )
        })?;
        items.extend(
            list_item_json(&loaded, alias.as_deref())
                .into_iter()
                .filter(|item| {
                    let text = [
                        item["id"].as_str().unwrap_or_default(),
                        item["name"].as_str().unwrap_or_default(),
                        item["description"].as_str().unwrap_or_default(),
                        item["kind"].as_str().unwrap_or_default(),
                    ]
                    .into_iter()
                    .chain(
                        item["tags"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|tag| tag.as_str()),
                    )
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                    text.contains(&query)
                }),
        );
    }
    items.sort_by(|a, b| {
        (a["provider"].as_str(), a["id"].as_str()).cmp(&(b["provider"].as_str(), b["id"].as_str()))
    });
    if args.json {
        print_catalog_json(
            "search",
            serde_json::json!({"query": args.query, "items": items}),
        );
    } else {
        print_catalog_human(&items);
    }
    Ok(())
}
fn info(args: InfoArgs) -> Result<(), String> {
    let (project, global) = roots(args.project)?;
    let resolution =
        resolver::resolve_add(&args.item, args.registry.as_deref(), &project, &global)?;
    let loaded = registry::load_registry(&resolution.manifest)?;
    let item = loaded
        .registry
        .find(resolution.item)
        .ok_or_else(|| format!("Item not found: {}", resolution.item.escape_debug()))?;
    let provider = loaded
        .v2
        .as_ref()
        .map(|v2| v2.id.as_str())
        .unwrap_or(&loaded.registry.name);
    let mut result = serde_json::json!({"provider": provider, "id": item.name, "name": item.name, "description": item.description, "files": item.files.iter().map(|file| serde_json::json!({"source": file.source, "target": file.target, "sha256": file.sha256})).collect::<Vec<_>>()});
    result["manifestSha256"] = serde_json::json!(loaded.manifest_sha256);
    if let Some(v2) = &loaded.v2 {
        let meta = v2.select(resolution.item, None)?;
        result["version"] = serde_json::json!(meta.version);
        result["kind"] = serde_json::json!(meta.kind);
        result["tags"] = serde_json::json!(meta.tags);
        result["license"] = serde_json::json!(
            meta.license
                .as_ref()
                .map(|license| serde_json::json!({"id": license.id, "notice": license.notice}))
        );
        result["files"] = serde_json::json!(meta.files.iter().map(|file| serde_json::json!({"source": file.source, "target": file.target, "sha256": file.sha256, "size": file.size})).collect::<Vec<_>>());
        result["dependencies"] = serde_json::json!(meta.dependencies.iter().map(|dep| serde_json::json!({"providerId": dep.provider_id, "itemId": dep.item_id, "version": dep.version, "manifestSha256": dep.manifest_sha256})).collect::<Vec<_>>());
        result["packages"] = serde_json::json!(
            meta.packages
                .iter()
                .map(
                    |pkg| serde_json::json!({"name": pkg.name, "spec": pkg.spec, "kind": pkg.kind})
                )
                .collect::<Vec<_>>()
        );
    }
    if args.json {
        print_catalog_json("info", serde_json::json!({"item": result}));
    } else {
        println!("{} — {}", item.name, terminal_safe(&item.description));
        println!("Provider: {provider}");
        println!("Manifest SHA-256: {}", loaded.manifest_sha256);
        if let Some(meta) = &loaded.v2 {
            let item_meta = meta.select(resolution.item, None)?;
            println!("Version: {}", item_meta.version);
            println!("Kind: {:?}", item_meta.kind);
            if !item_meta.tags.is_empty() {
                println!("Tags: {}", item_meta.tags.join(", "));
            }
            if let Some(license) = &item_meta.license {
                println!("License: {}", license.id);
                if let Some(notice) = &license.notice {
                    println!("License notice: {notice}");
                }
            }
        }
        for file in &item.files {
            println!(
                "File: {} -> {} (sha256 {})",
                file.source, file.target, file.sha256
            );
        }
        if let Some(meta) = &loaded.v2 {
            let item_meta = meta.select(resolution.item, None)?;
            for file in &item_meta.files {
                println!("Size: {} {} bytes", file.target, file.size);
            }
            for dependency in &item_meta.dependencies {
                println!(
                    "Dependency: {}/{}@{}",
                    dependency.provider_id, dependency.item_id, dependency.version
                );
            }
            for package in &item_meta.packages {
                println!(
                    "Package requirement: {} {} ({:?})",
                    package.name, package.spec, package.kind
                );
            }
        }
    }
    Ok(())
}
fn list_item_json(
    loaded: &registry::LoadedRegistry,
    alias: Option<&str>,
) -> Vec<serde_json::Value> {
    loaded.registry.items.iter().map(|item| {
        let mut value = serde_json::json!({"provider": loaded.v2.as_ref().map(|v2| v2.id.as_str()).unwrap_or(loaded.registry.name.as_str()), "registry": loaded.registry.name, "id": item.name, "name": item.name, "description": item.description});
        if let Some(alias) = alias { value["alias"] = serde_json::json!(alias); }
        if let Some(meta) = loaded.v2.as_ref().and_then(|v2| v2.items.iter().find(|entry| entry.id == item.name)) {
            value["version"] = serde_json::json!(meta.version); value["kind"] = serde_json::json!(meta.kind); value["tags"] = serde_json::json!(meta.tags);
        }
        value
    }).collect()
}
fn print_catalog_json(command: &str, data: serde_json::Value) {
    println!(
        "{}",
        serde_json::json!({"schemaVersion": 1, "command": command, "data": data})
    );
}
fn print_catalog_human(items: &[serde_json::Value]) {
    for item in items {
        println!(
            "{}/{}: {}",
            item["provider"].as_str().unwrap_or("registry"),
            item["id"].as_str().unwrap_or(""),
            terminal_safe(item["description"].as_str().unwrap_or(""))
        );
    }
}
fn terminal_safe(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
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
    let selected_manifest = resolution.manifest;
    let selected_item = resolution.item.to_owned();
    let loaded = registry::load_registry(&selected_manifest)?;
    let item = loaded
        .registry
        .find(&selected_item)
        .ok_or_else(|| format!("Component not found: {}", selected_item.escape_debug()))?;
    let v2_item = loaded
        .v2
        .as_ref()
        .map(|v2| v2.select(&selected_item, None))
        .transpose()?;
    if let Some(root_item_id) = v2_item.map(|item| item.id.clone()) {
        return add_v2_graph(GraphRequest {
            dry_run: args.dry_run,
            json: args.json,
            project_root: &project_root,
            project_config: &project_config,
            global_config: &global,
            declared_root_source: &selected_manifest,
            root_loaded: loaded,
            root_item_id: &root_item_id,
            updating: None,
            allow_downgrade: false,
        });
    }
    if !args.dry_run {
        transaction::recover(&project_root)?;
    }
    let provider_id;
    let declared_manifest_locator;
    let manifest_source = match &loaded.source {
        registry::RegistrySource::Local { .. } => {
            let path = registry::canonical_local_manifest(&selected_manifest)?;
            let url = url::Url::from_file_path(path)
                .map_err(|_| "Cannot represent manifest provenance as a file URL")?;
            provider_id = if let Some(v2) = &loaded.v2 {
                format!("v2:{}", v2.id)
            } else {
                lockfile::v1_provider_id(url.as_str())?
            };
            declared_manifest_locator = url.to_string();
            url
        }
        registry::RegistrySource::Remote { manifest_url } => {
            let url = sanitized_remote_locator(manifest_url.as_str())?;
            let declared = sanitized_remote_locator(&selected_manifest)?;
            if let Some(v2) = &loaded.v2 {
                provider_id = format!("v2:{}", v2.id);
            } else {
                provider_id = lockfile::v1_provider_id(url.as_str())?;
            }
            declared_manifest_locator = declared.to_string();
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
            declared_source: declared_manifest_locator,
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
                        args.dry_run,
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
    let contents = if let Some(v2_item) = v2_item {
        let sizes = v2_plan_sizes(v2_item, &plan)?;
        downloader::acquire_v2_files_with_sizes(&loaded.source, &plan, &sizes)?
    } else {
        downloader::acquire_files(&loaded.source, &plan)?
    };
    for (file, acquired) in plan.files.iter().zip(&contents.files) {
        let installed = lockfile::Content {
            sha256: acquired.sha256.clone(),
            size: acquired.size,
        };
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
    for (file, acquired) in plan.files.iter().zip(contents.files) {
        edits.push(transaction::Edit {
            target: file.target_display.clone(),
            before: None,
            write: Some(transaction::WriteSource::File(acquired.path)),
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

struct PlannedGraphItem {
    key: dependency::ItemKey,
    plan: planner::InstallationPlan,
    target_root: Option<String>,
    declared_source: String,
    resolved_source: String,
    provider_id: String,
    existing: Option<lockfile::Item>,
}

struct GraphRequest<'a> {
    dry_run: bool,
    json: bool,
    project_root: &'a Path,
    project_config: &'a Path,
    global_config: &'a Path,
    declared_root_source: &'a str,
    root_loaded: registry::LoadedRegistry,
    root_item_id: &'a str,
    updating: Option<lockfile::Item>,
    allow_downgrade: bool,
}

fn add_v2_graph(request: GraphRequest<'_>) -> Result<(), String> {
    let GraphRequest {
        dry_run,
        json,
        project_root,
        project_config,
        global_config,
        declared_root_source,
        root_loaded,
        root_item_id,
        updating,
        allow_downgrade,
    } = request;
    if !dry_run {
        transaction::recover(project_root)?;
    }
    let graph = dependency::Graph::resolve(
        declared_root_source,
        root_loaded,
        root_item_id,
        project_config,
        global_config,
    )?;
    let before = lockfile::load(project_root)?;
    let owners = before.lock().owners();
    let target_map = crate::registry_v2::TargetMap::load(project_root)?;
    let mut pending = Vec::<PlannedGraphItem>::new();
    let mut reused = BTreeMap::<dependency::ItemKey, lockfile::Item>::new();
    let mut package_requirements = BTreeMap::<String, Vec<serde_json::Value>>::new();
    let mut all_new_bytes = 0_u64;
    let mut next = before.lock().clone();

    for key in &graph.order {
        let node = graph
            .nodes
            .get(key)
            .ok_or("Resolved dependency node is missing")?;
        let item = graph.item(key)?;
        let snapshot = &graph.snapshots[node.snapshot];
        let v2 = snapshot
            .loaded
            .v2
            .as_ref()
            .ok_or("Resolved dependency registry is not Registry v2")?;
        let provider_id = format!("v2:{}", v2.id);
        let expected_dependencies = node
            .dependencies
            .iter()
            .map(|dependency_key| {
                let dependency_node = graph
                    .nodes
                    .get(dependency_key)
                    .ok_or("Resolved dependency edge is missing")?;
                let dependency_item = graph.item(dependency_key)?;
                Ok(lockfile::Dependency {
                    provider_id: dependency_key.0.clone(),
                    item_id: dependency_key.1.clone(),
                    version: dependency_item.version.clone(),
                    manifest_sha256: graph.snapshots[dependency_node.snapshot]
                        .loaded
                        .manifest_sha256
                        .clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for package in &item.packages {
            package_requirements
                .entry(package.name.clone())
                .or_default()
                .push(serde_json::json!({
                    "spec": package.spec,
                    "kind": package_kind_name(package.kind),
                    "requiredBy": { "providerId": key.0, "itemId": key.1 }
                }));
        }

        let existing_record = before
            .lock()
            .items
            .iter()
            .find(|record| record.provider_id == key.0 && record.item_id == key.1)
            .cloned();
        if let Some(existing) = existing_record.as_ref() {
            let same_graph_snapshot = existing.version.as_deref() == Some(item.version.as_str())
                && existing.manifest_sha256 == snapshot.loaded.manifest_sha256
                && existing.dependencies == expected_dependencies
                && existing.files.len() == item.files.len()
                && item.files.iter().all(|asset| {
                    existing.files.iter().any(|file| {
                        file.source == asset.source
                            && file.declared_target == asset.target
                            && file.expected_sha256 == asset.sha256
                            && file.installed.size == asset.size
                    })
                });
            if !same_graph_snapshot && updating.is_none() {
                return Err(format!(
                    "Dependency {} is already installed from a different version or graph; update it explicitly before adding this item",
                    display_graph_item(key)
                ));
            }
            if !same_graph_snapshot && updating.is_some() {
                let installed_version = existing_record
                    .as_ref()
                    .and_then(|existing| existing.version.as_deref());
                if let Some(installed_version) = installed_version
                    && semver::Version::parse(&item.version)
                        .map_err(|_| "Invalid candidate semantic version")?
                        < semver::Version::parse(installed_version)
                            .map_err(|_| "Invalid installed semantic version")?
                    && !allow_downgrade
                {
                    return Err(format!(
                        "Dependency {} candidate version {} is older than installed version {installed_version}; pass --allow-downgrade to confirm",
                        display_graph_item(key),
                        item.version
                    ));
                }
                let external_dependent = before.lock().items.iter().any(|candidate| {
                    !graph
                        .nodes
                        .contains_key(&(candidate.provider_id.clone(), candidate.item_id.clone()))
                        && candidate.dependencies.iter().any(|dependency| {
                            dependency.provider_id == key.0
                                && dependency.item_id == key.1
                                && (dependency.version != item.version
                                    || dependency.manifest_sha256
                                        != snapshot.loaded.manifest_sha256)
                        })
                });
                if external_dependent {
                    return Err(format!(
                        "Cannot update {} because an installed item outside this graph pins a different exact version or manifest digest",
                        display_graph_item(key)
                    ));
                }
            }
            for file in &existing.files {
                ensure_unchanged_managed_file(project_root, file)?;
            }
            if same_graph_snapshot {
                reused.insert(key.clone(), existing.clone());
                continue;
            }
        }

        let target_root = if updating.is_some() && existing_record.is_some() {
            existing_record
                .as_ref()
                .and_then(|existing| existing.target_root.clone())
        } else {
            target_map.root(item.kind, None)?
        };
        let mapped_item = crate::registry::RegistryItem {
            name: item.id.clone(),
            description: item.description.clone().unwrap_or_else(|| item.id.clone()),
            files: item
                .files
                .iter()
                .map(|asset| crate::registry::RegistryFile {
                    source: asset.source.clone(),
                    target: target_root.as_ref().map_or_else(
                        || asset.target.clone(),
                        |root| format!("{root}/{}", asset.target),
                    ),
                    sha256: asset.sha256.clone(),
                })
                .collect(),
        };
        let plan = planner::build_plan(project_root, &mapped_item)?;
        let conflict_targets = plan
            .conflicts
            .iter()
            .filter(|target| {
                !existing_record.as_ref().is_some_and(|existing| {
                    existing.provider_id == key.0
                        && existing.item_id == key.1
                        && existing.files.iter().any(|file| &file.target == *target)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if !conflict_targets.is_empty() {
            return Err(format!(
                "Dependency {} conflicts with existing unmanaged files: {}",
                display_graph_item(key),
                conflict_targets.join(", ")
            ));
        }
        for file in &plan.files {
            if let Some((owner, _)) = owners.get(file.target_display.as_str())
                && !existing_record.as_ref().is_some_and(|existing| {
                    owner.provider_id == existing.provider_id && owner.item_id == existing.item_id
                })
            {
                return Err(format!(
                    "Dependency destination {} is owned by {}/{}",
                    file.target_display, owner.provider_id, owner.item_id
                ));
            }
        }
        for asset in &item.files {
            all_new_bytes = all_new_bytes
                .checked_add(asset.size)
                .ok_or("Dependency operation size overflow")?;
            if asset.size > crate::downloader::MAX_V2_FILE_BYTES as u64
                || all_new_bytes > crate::downloader::MAX_OPERATION_BYTES
            {
                return Err("Dependency operation exceeds the 512 MiB acquisition limit".into());
            }
        }
        let (provenance_provider, declared_source, resolved_source) =
            candidate_provenance(&snapshot.loaded, &snapshot.declared)?;
        if provenance_provider != provider_id {
            return Err("Resolved dependency source disagrees with provider identity".into());
        }
        pending.push(PlannedGraphItem {
            key: key.clone(),
            plan,
            target_root,
            declared_source,
            resolved_source,
            provider_id,
            existing: existing_record,
        });
    }

    validate_package_requirements(&mut package_requirements)?;
    let replaced_items = pending
        .iter()
        .filter_map(|node| node.existing.as_ref())
        .map(|item| (item.provider_id.clone(), item.item_id.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    let mut project_targets: Vec<String> = before
        .lock()
        .items
        .iter()
        .filter(|item| !replaced_items.contains(&(item.provider_id.clone(), item.item_id.clone())))
        .flat_map(|item| item.files.iter().map(|file| file.target.clone()))
        .collect();
    project_targets.extend(pending.iter().flat_map(|node| {
        node.plan
            .files
            .iter()
            .map(|file| file.target_display.clone())
    }));
    crate::registry::validate_targets(project_targets.iter().map(String::as_str))?;

    let mut provenance_changed = false;
    if updating.is_some() {
        for (key, reused_item) in &mut reused {
            let node = &graph.nodes[key];
            let snapshot = &graph.snapshots[node.snapshot];
            let (provider_id, declared_source, resolved_source) =
                candidate_provenance(&snapshot.loaded, &snapshot.declared)?;
            let mut updated = reused_item.clone();
            updated.manifest_source = resolved_source.clone();
            for file in &mut updated.files {
                file.resolved_source = snapshot.loaded.source.manifest_base(&file.source)?;
            }
            if updated.manifest_source != reused_item.manifest_source
                || updated
                    .files
                    .iter()
                    .zip(&reused_item.files)
                    .any(|(new, old)| new.resolved_source != old.resolved_source)
            {
                provenance_changed = true;
                let installed = next
                    .items
                    .iter_mut()
                    .find(|item| item.provider_id == key.0 && item.item_id == key.1)
                    .ok_or("Reused graph item is missing from the lock")?;
                *installed = updated.clone();
            }
            if let Some(provider) = next
                .providers
                .iter_mut()
                .find(|provider| provider.provider_id == provider_id)
                && (provider.declared_source != declared_source
                    || provider.resolved_source != resolved_source
                    || provider.manifest_sha256 != snapshot.loaded.manifest_sha256)
            {
                provenance_changed = true;
                provider.declared_source = declared_source;
                provider.resolved_source = resolved_source;
                provider.manifest_sha256 = snapshot.loaded.manifest_sha256.clone();
            }
            *reused_item = updated;
        }
    }

    if pending.is_empty() {
        let root = graph.item(&graph.root)?;
        let root_snapshot = graph.nodes[&graph.root].snapshot;
        let files = reused
            .values()
            .flat_map(|item| {
                item.files
                    .iter()
                    .map(move |file| graph_file_json(item, file, "unchanged"))
            })
            .collect::<Vec<_>>();
        if provenance_changed {
            let transaction = transaction::Plan::new(before.clone(), next, Vec::new())?;
            if !dry_run {
                transaction::apply(project_root, transaction)?;
            }
        }
        if updating.is_some() {
            emit_graph_update(json, dry_run, &graph, files, &package_requirements)?;
        } else {
            emit_graph_add(
                json,
                dry_run,
                &graph,
                root,
                &graph.snapshots[root_snapshot].loaded.manifest_sha256,
                files,
                &package_requirements,
            )?;
        }
        if !json {
            if updating.is_some() {
                println!("{} is already current (no changes).", root.id);
            } else {
                println!(
                    "Already installed {} and its resolved dependency graph (no changes).",
                    root.id
                );
            }
            print_package_suggestions(&package_requirements);
        }
        return Ok(());
    }

    let mut acquisitions = Vec::with_capacity(pending.len());
    for planned in &pending {
        let node = &graph.nodes[&planned.key];
        let snapshot = &graph.snapshots[node.snapshot];
        let item = graph.item(&planned.key)?;
        let sizes = planned
            .plan
            .files
            .iter()
            .map(|file| {
                item.files
                    .iter()
                    .find(|asset| asset.source == file.source)
                    .map(|asset| asset.size)
                    .ok_or("Registry item asset is missing from its installation plan")
            })
            .collect::<Result<Vec<_>, _>>()?;
        acquisitions.push(downloader::acquire_v2_files_with_sizes(
            &snapshot.loaded.source,
            &planned.plan,
            &sizes,
        )?);
    }

    let mut edits = Vec::new();
    let mut json_files = reused
        .values()
        .flat_map(|item| {
            item.files
                .iter()
                .map(|file| graph_file_json(item, file, "unchanged"))
        })
        .collect::<Vec<_>>();
    for (planned, acquired) in pending.iter().zip(&acquisitions) {
        let node = &graph.nodes[&planned.key];
        let snapshot = &graph.snapshots[node.snapshot];
        let item = graph.item(&planned.key)?;
        if !next
            .providers
            .iter()
            .any(|provider| provider.provider_id == planned.provider_id)
        {
            next.providers.push(lockfile::Provider {
                provider_id: planned.provider_id.clone(),
                declared_source: planned.declared_source.clone(),
                resolved_source: planned.resolved_source.clone(),
                manifest_sha256: snapshot.loaded.manifest_sha256.clone(),
            });
        } else if planned.existing.is_some() {
            let provider = next
                .providers
                .iter_mut()
                .find(|provider| provider.provider_id == planned.provider_id)
                .ok_or("Installed dependency item has no provider record")?;
            provider.declared_source = planned.declared_source.clone();
            provider.resolved_source = planned.resolved_source.clone();
            provider.manifest_sha256 = snapshot.loaded.manifest_sha256.clone();
        }
        let dependencies = node
            .dependencies
            .iter()
            .map(|key| {
                let dep_node = &graph.nodes[key];
                let dep = graph.item(key)?;
                Ok(lockfile::Dependency {
                    provider_id: key.0.clone(),
                    item_id: key.1.clone(),
                    version: dep.version.clone(),
                    manifest_sha256: graph.snapshots[dep_node.snapshot]
                        .loaded
                        .manifest_sha256
                        .clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut files = Vec::with_capacity(planned.plan.files.len());
        let mut content_by_target = BTreeMap::new();
        for (file, content) in planned.plan.files.iter().zip(&acquired.files) {
            let asset = item
                .files
                .iter()
                .find(|asset| {
                    asset.source == file.source
                        && planned.target_root.as_ref().map_or_else(
                            || asset.target == file.target_display,
                            |root| format!("{root}/{}", asset.target) == file.target_display,
                        )
                })
                .ok_or("Registry item asset is missing from its installation plan")?;
            if content.sha256 != asset.sha256 || content.size != asset.size {
                return Err(format!("Integrity mismatch for {}", file.source));
            }
            let resolved_source = match &content.resolved_locator {
                Some(locator) => locator.clone(),
                None => snapshot.loaded.source.manifest_base(&file.source)?,
            };
            files.push(lockfile::OwnedFile {
                source: file.source.clone(),
                resolved_source,
                declared_target: asset.target.clone(),
                target: file.target_display.clone(),
                expected_sha256: asset.sha256.clone(),
                installed: lockfile::Content {
                    sha256: content.sha256.clone(),
                    size: content.size,
                },
            });
            content_by_target.insert(file.target_display.clone(), content.path.clone());
        }
        let lock_item = lockfile::Item {
            provider_id: planned.provider_id.clone(),
            item_id: planned.key.1.clone(),
            version: Some(item.version.clone()),
            manifest_source: planned.resolved_source.clone(),
            manifest_sha256: snapshot.loaded.manifest_sha256.clone(),
            target_root: planned.target_root.clone(),
            dependencies,
            files,
        };
        let previous_files = planned
            .existing
            .as_ref()
            .map(|existing| {
                existing
                    .files
                    .iter()
                    .map(|file| (file.target.as_str(), file))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        for file in &lock_item.files {
            let previous = previous_files.get(file.target.as_str()).copied();
            let (action, before_content) = match previous {
                Some(old) if old.installed == file.installed => ("unchanged", None),
                Some(old) => ("replace", Some(old.installed.clone())),
                None => ("install", None),
            };
            if action != "unchanged" {
                edits.push(transaction::Edit {
                    target: file.target.clone(),
                    before: before_content,
                    write: Some(transaction::WriteSource::File(
                        content_by_target
                            .get(&file.target)
                            .ok_or("Missing verified graph artifact bytes")?
                            .clone(),
                    )),
                });
            }
            json_files.push(json_file_action(
                &file.target,
                action,
                if previous.is_some() {
                    planned.existing.as_ref()
                } else {
                    None
                },
                Some(&lock_item),
            ));
        }
        if let Some(existing) = planned.existing.as_ref() {
            for old in &existing.files {
                if !lock_item.files.iter().any(|file| file.target == old.target) {
                    edits.push(transaction::Edit {
                        target: old.target.clone(),
                        before: Some(old.installed.clone()),
                        write: None,
                    });
                    json_files.push(json_file_action(
                        &old.target,
                        "remove",
                        Some(existing),
                        None,
                    ));
                }
            }
        }
        if let Some(index) = next.items.iter().position(|existing| {
            existing.provider_id == lock_item.provider_id && existing.item_id == lock_item.item_id
        }) {
            next.items[index] = lock_item;
        } else {
            next.items.push(lock_item);
        }
    }

    json_files.sort_by(|left, right| left["target"].as_str().cmp(&right["target"].as_str()));

    let transaction = transaction::Plan::new(before, next, edits)?;
    transaction.check(project_root)?;
    if dry_run {
        if updating.is_some() {
            emit_graph_update(json, true, &graph, json_files, &package_requirements)?;
        } else {
            emit_graph_add(
                json,
                true,
                &graph,
                graph.item(&graph.root)?,
                &graph.snapshots[graph.nodes[&graph.root].snapshot]
                    .loaded
                    .manifest_sha256,
                json_files,
                &package_requirements,
            )?;
        }
        if !json {
            for planned in &pending {
                println!(
                    "Will {} {}/{}",
                    if updating.is_some() {
                        "update"
                    } else {
                        "install"
                    },
                    planned.key.0,
                    planned.key.1
                );
                for file in &planned.plan.files {
                    println!("  {}", file.target_display);
                }
            }
            print_package_suggestions(&package_requirements);
            println!("Dry run: no project files were changed.");
        }
        return Ok(());
    }
    transaction::apply(project_root, transaction)?;
    if updating.is_some() {
        emit_graph_update(json, false, &graph, json_files, &package_requirements)?;
    } else {
        emit_graph_add(
            json,
            false,
            &graph,
            graph.item(&graph.root)?,
            &graph.snapshots[graph.nodes[&graph.root].snapshot]
                .loaded
                .manifest_sha256,
            json_files,
            &package_requirements,
        )?;
    }
    if !json {
        for planned in &pending {
            println!(
                "{} {}/{}",
                if updating.is_some() {
                    "Updated"
                } else {
                    "Installed"
                },
                planned.key.0,
                planned.key.1
            );
            for file in &planned.plan.files {
                println!("  {}", file.target_display);
            }
        }
        print_package_suggestions(&package_requirements);
    }
    Ok(())
}

trait ManifestBase {
    fn manifest_base(&self, source: &str) -> Result<String, String>;
}

impl ManifestBase for registry::RegistrySource {
    fn manifest_base(&self, source: &str) -> Result<String, String> {
        match self {
            registry::RegistrySource::Local { root } => url::Url::from_directory_path(root)
                .map_err(|_| "Cannot represent local artifact base")?
                .join(source)
                .map(|url| url.to_string())
                .map_err(|_| "Invalid resolved artifact locator".into()),
            registry::RegistrySource::Remote { manifest_url } => manifest_url
                .join(source)
                .map(|url| {
                    let mut url = url;
                    url.set_query(None);
                    url.set_fragment(None);
                    url.to_string()
                })
                .map_err(|_| "Invalid resolved artifact locator".into()),
        }
    }
}

fn package_kind_name(kind: crate::registry_v2::PackageKind) -> &'static str {
    match kind {
        crate::registry_v2::PackageKind::Runtime => "runtime",
        crate::registry_v2::PackageKind::Peer => "peer",
        crate::registry_v2::PackageKind::Dev => "dev",
    }
}

fn validate_package_requirements(
    packages: &mut BTreeMap<String, Vec<serde_json::Value>>,
) -> Result<(), String> {
    for (name, requirements) in packages {
        requirements.sort_by(|left, right| {
            (
                left["spec"].as_str(),
                left["kind"].as_str(),
                left["requiredBy"]["providerId"].as_str(),
                left["requiredBy"]["itemId"].as_str(),
            )
                .cmp(&(
                    right["spec"].as_str(),
                    right["kind"].as_str(),
                    right["requiredBy"]["providerId"].as_str(),
                    right["requiredBy"]["itemId"].as_str(),
                ))
        });
        let specs = requirements
            .iter()
            .filter_map(|requirement| requirement["spec"].as_str())
            .collect::<Vec<_>>();
        if !package_specs_overlap(&specs)? {
            return Err(format!(
                "Conflicting package requirements for {name}: {}",
                specs.join(", ")
            ));
        }
    }
    Ok(())
}

fn package_specs_overlap(specs: &[&str]) -> Result<bool, String> {
    if specs.len() < 2 {
        return Ok(true);
    }
    let requirements = specs
        .iter()
        .map(|spec| {
            let value = spec.strip_prefix(['^', '~']).unwrap_or(spec);
            let version =
                semver::Version::parse(value).map_err(|_| "Invalid package requirement version")?;
            let requirement_spec = if spec.starts_with(['^', '~']) {
                (*spec).to_owned()
            } else {
                format!("={spec}")
            };
            let requirement = semver::VersionReq::parse(&requirement_spec)
                .map_err(|_| "Invalid package requirement spec")?;
            Ok((version, requirement))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let max = requirements
        .iter()
        .map(|(version, _)| version)
        .max()
        .ok_or("Package requirement set is empty")?;
    let mut candidates = requirements
        .iter()
        .map(|(version, _)| version.clone())
        .collect::<Vec<_>>();
    candidates.push(max.clone());
    let mut stable = max.clone();
    stable.pre = semver::Prerelease::EMPTY;
    stable.build = semver::BuildMetadata::EMPTY;
    candidates.push(stable);
    let mut patch = max.clone();
    patch.pre = semver::Prerelease::EMPTY;
    patch.build = semver::BuildMetadata::EMPTY;
    patch.patch = patch.patch.saturating_add(1);
    candidates.push(patch);
    Ok(candidates.iter().any(|candidate| {
        requirements
            .iter()
            .all(|(_, requirement)| requirement.matches(candidate))
    }))
}

fn print_package_suggestions(packages: &BTreeMap<String, Vec<serde_json::Value>>) {
    for (name, requirements) in packages {
        for requirement in requirements {
            let Some(spec) = requirement["spec"].as_str() else {
                continue;
            };
            let reference = format!("{name}@{spec}");
            println!("Package requirement (advisory; FrilAdd will not run it): {reference}");
            println!("  npm: npm install '{reference}'");
            println!("  pnpm: pnpm add '{reference}'");
            println!("  yarn: yarn add '{reference}'");
        }
    }
}

fn graph_package_json(
    packages: &BTreeMap<String, Vec<serde_json::Value>>,
) -> Result<Vec<serde_json::Value>, String> {
    packages
        .iter()
        .map(|(name, requirements)| -> Result<_, String> {
            let specs = requirements
                .iter()
                .filter_map(|requirement| requirement["spec"].as_str())
                .collect::<Vec<_>>();
            let suggestions = requirements
                .iter()
                .filter_map(|requirement| requirement["spec"].as_str())
                .map(|spec| {
                    let reference = format!("{name}@{spec}");
                    serde_json::json!({
                        "npm": format!("npm install '{reference}'"),
                        "pnpm": format!("pnpm add '{reference}'"),
                        "yarn": format!("yarn add '{reference}'"),
                    })
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({
                "name": name,
                "requirements": requirements,
                "compatible": package_specs_overlap(&specs)?,
                "suggestions": {
                    "npm": suggestions.iter().map(|suggestion| suggestion["npm"].clone()).collect::<Vec<_>>(),
                    "pnpm": suggestions.iter().map(|suggestion| suggestion["pnpm"].clone()).collect::<Vec<_>>(),
                    "yarn": suggestions.iter().map(|suggestion| suggestion["yarn"].clone()).collect::<Vec<_>>(),
                },
            }))
        })
        .collect()
}

fn emit_graph_add(
    json: bool,
    dry_run: bool,
    graph: &dependency::Graph,
    root: &crate::registry_v2::Item,
    root_manifest_sha256: &str,
    files: Vec<serde_json::Value>,
    packages: &BTreeMap<String, Vec<serde_json::Value>>,
) -> Result<(), String> {
    if !json {
        return Ok(());
    }
    let items = graph_items_json(graph)?;
    let package_data = graph_package_json(packages)?;
    let root_key = &graph.root;
    println!(
        "{}",
        serde_json::json!({
            "schemaVersion": 1,
            "command": "add",
            "data": {
                "dryRun": dry_run,
                "item": {
                    "providerId": root_key.0,
                    "itemId": root.id,
                    "version": root.version,
                    "manifestSha256": root_manifest_sha256,
                },
                "items": items,
                "files": files,
                "packages": package_data,
            }
        })
    );
    Ok(())
}

fn graph_items_json(graph: &dependency::Graph) -> Result<Vec<serde_json::Value>, String> {
    let mut items = Vec::with_capacity(graph.order.len());
    for key in &graph.order {
        let node = graph
            .nodes
            .get(key)
            .ok_or("Resolved dependency node is missing")?;
        let item = graph.item(key)?;
        let mut dependencies = Vec::with_capacity(node.dependencies.len());
        for dependency in &node.dependencies {
            let dep_node = graph
                .nodes
                .get(dependency)
                .ok_or("Resolved dependency edge is missing")?;
            let dep_item = graph.item(dependency)?;
            dependencies.push(serde_json::json!({
                "providerId": dependency.0,
                "itemId": dependency.1,
                "version": dep_item.version,
                "manifestSha256": graph.snapshots[dep_node.snapshot].loaded.manifest_sha256,
            }));
        }
        items.push(serde_json::json!({
            "providerId": key.0,
            "itemId": key.1,
            "version": item.version,
            "manifestSha256": graph.snapshots[node.snapshot].loaded.manifest_sha256,
            "dependencies": dependencies,
        }));
    }
    Ok(items)
}

fn emit_graph_update(
    json: bool,
    dry_run: bool,
    graph: &dependency::Graph,
    files: Vec<serde_json::Value>,
    packages: &BTreeMap<String, Vec<serde_json::Value>>,
) -> Result<(), String> {
    if !json {
        return Ok(());
    }
    let package_data = graph_package_json(packages)?;
    let mut grouped = BTreeMap::<(String, String), Vec<serde_json::Value>>::new();
    for file in files {
        let owner = if file["ownerAfter"].is_object() {
            &file["ownerAfter"]
        } else {
            &file["ownerBefore"]
        };
        let provider = owner["providerId"]
            .as_str()
            .ok_or("Update action is missing its provider owner")?;
        let item = owner["itemId"]
            .as_str()
            .ok_or("Update action is missing its item owner")?;
        grouped
            .entry((provider.to_owned(), item.to_owned()))
            .or_default()
            .push(file);
    }
    let items = grouped
        .into_iter()
        .map(|((provider_id, item_id), mut actions)| {
            actions.sort_by(|left, right| left["target"].as_str().cmp(&right["target"].as_str()));
            serde_json::json!({
                "providerId": provider_id,
                "itemId": item_id,
                "actions": actions,
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::json!({
            "schemaVersion": 1,
            "command": "update",
            "data": {
                "dryRun": dry_run,
                "items": items,
                "dependencyGraph": graph_items_json(graph)?,
                "packages": package_data,
            }
        })
    );
    Ok(())
}

fn graph_file_json(
    item: &lockfile::Item,
    file: &lockfile::OwnedFile,
    action: &str,
) -> serde_json::Value {
    serde_json::json!({
        "target": file.target,
        "action": action,
        "ownerBefore": {"providerId": item.provider_id, "itemId": item.item_id},
        "ownerAfter": {"providerId": item.provider_id, "itemId": item.item_id},
    })
}

fn display_graph_item(key: &dependency::ItemKey) -> String {
    format!("{}/{}", key.0, key.1)
}

fn ensure_unchanged_managed_file(
    project_root: &Path,
    file: &lockfile::OwnedFile,
) -> Result<(), String> {
    match transaction::observe(project_root, &file.target)? {
        transaction::Observation::Content(content) if content == file.installed => Ok(()),
        transaction::Observation::Missing => Err(format!(
            "Managed file {} is missing; run restore before changing the dependency graph",
            file.target
        )),
        transaction::Observation::Content(_) => Err(format!(
            "Managed file {} was modified; preserve it and resolve the edit before changing the dependency graph",
            file.target
        )),
        transaction::Observation::Unsafe => Err(format!(
            "Managed path {} is unsafe; refusing dependency graph change",
            file.target
        )),
        transaction::Observation::Unreadable => Err(format!(
            "Managed file {} is unreadable; refusing dependency graph change",
            file.target
        )),
    }
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
fn installed(args: InstalledArgs) -> Result<(), String> {
    let (project_config, _) = roots(args.project)?;
    let project_root = project_config
        .parent()
        .ok_or_else(|| "Project configuration path has no parent".to_string())?;
    let snapshot = lockfile::load(project_root)?;
    let items = &snapshot.lock().items;
    if args.json {
        let data: Vec<_> = items
            .iter()
            .map(|item| {
                serde_json::json!({
                    "providerId": item.provider_id,
                    "itemId": item.item_id,
                    "version": item.version,
                    "manifestSha256": item.manifest_sha256,
                    "fileCount": item.files.len(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "schemaVersion": 1,
                "command": "installed",
                "data": { "items": data }
            })
        );
        return Ok(());
    }
    if items.is_empty() {
        println!("No managed items installed.");
        return Ok(());
    }
    for item in items {
        let version = item.version.as_deref().map_or_else(
            || format!("manifest {}", item.manifest_sha256),
            |v| format!("version {v}"),
        );
        println!(
            "{}/{}\t{}\t{} file(s)",
            item.provider_id,
            item.item_id,
            version,
            item.files.len()
        );
    }
    Ok(())
}

fn status(args: StatusArgs) -> Result<(), String> {
    let (project_config, global) = roots(args.project)?;
    let project_root = project_config
        .parent()
        .ok_or_else(|| "Project configuration path has no parent".to_string())?;
    let snapshot = lockfile::load(project_root)?;
    let lock = snapshot.lock();
    let selected = select_status_items(lock, args.item.as_deref(), &project_config, &global)?;
    if selected.is_empty()
        && let Some(selector) = args.item.as_deref()
    {
        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "schemaVersion": 1,
                    "command": "status",
                    "data": {
                        "items": [{ "selector": selector, "state": "unmanaged", "files": [] }],
                        "counts": { "unmanaged": 1 }
                    }
                })
            );
        } else {
            println!("{selector}\tunmanaged (not recorded in friladd.lock.json)");
        }
        if args.check {
            return Err(format!("Item '{selector}' is not managed by this project."));
        }
        return Ok(());
    }
    let mut items = Vec::new();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut drift = false;
    for item in selected {
        let mut files = Vec::new();
        for file in &item.files {
            let (state, actual) = match transaction::observe(project_root, &file.target)? {
                transaction::Observation::Missing => ("missing", None),
                transaction::Observation::Unsafe => ("unsafe", None),
                transaction::Observation::Unreadable => ("unreadable", None),
                transaction::Observation::Content(content) if content == file.installed => {
                    ("unchanged", Some(content.sha256))
                }
                transaction::Observation::Content(content) => ("modified", Some(content.sha256)),
            };
            *counts.entry(state.to_string()).or_default() += 1;
            drift |= state != "unchanged";
            files.push(serde_json::json!({
                "target": file.target,
                "state": state,
                "expectedSha256": file.installed.sha256,
                "actualSha256": actual,
            }));
        }
        items.push(serde_json::json!({
            "providerId": item.provider_id,
            "itemId": item.item_id,
            "version": item.version,
            "manifestSha256": item.manifest_sha256,
            "files": files,
        }));
    }
    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "schemaVersion": 1,
                "command": "status",
                "data": { "items": items, "counts": counts }
            })
        );
    } else if items.is_empty() {
        println!("No managed items to check.");
    } else {
        for item in &items {
            let provider = item["providerId"].as_str().unwrap_or_default();
            let id = item["itemId"].as_str().unwrap_or_default();
            println!("{provider}/{id}");
            if let Some(files) = item["files"].as_array() {
                for file in files {
                    println!(
                        "  {}\t{}",
                        file["state"].as_str().unwrap_or("unreadable"),
                        file["target"].as_str().unwrap_or("")
                    );
                }
            }
        }
    }
    if args.check && drift {
        return Err("Managed file drift detected; review the status report and use restore or update as appropriate.".into());
    }
    Ok(())
}

fn select_status_items<'a>(
    lock: &'a lockfile::Lock,
    selector: Option<&str>,
    project: &Path,
    global: &Path,
) -> Result<Vec<&'a lockfile::Item>, String> {
    let Some(selector) = selector else {
        return Ok(lock.items.iter().collect());
    };
    let (provider, item_id) = match selector.split_once('/') {
        Some((provider, item))
            if !provider.is_empty() && !item.is_empty() && !item.contains('/') =>
        {
            (Some(provider), item)
        }
        None if !selector.is_empty() => (None, selector),
        _ => return Err("Invalid item selector; use item or registry/item".into()),
    };
    let provider_ids = if let Some(provider) = provider {
        if lock
            .providers
            .iter()
            .any(|entry| entry.provider_id == provider)
        {
            vec![provider.to_owned()]
        } else {
            resolve_provider_ids(lock, provider, project, global)?
        }
    } else {
        Vec::new()
    };
    let matches: Vec<_> = lock
        .items
        .iter()
        .filter(|item| item.item_id == item_id)
        .filter(|item| {
            provider.is_none_or(|token| {
                item.provider_id == token || provider_ids.iter().any(|id| id == &item.provider_id)
            })
        })
        .collect();
    if matches.len() > 1 {
        return Err(format!(
            "Item selector '{selector}' is ambiguous; qualify it with a registry alias or provider ID."
        ));
    }
    if matches.is_empty() {
        return Ok(Vec::new());
    }
    Ok(matches)
}

fn resolve_provider_ids(
    lock: &lockfile::Lock,
    token: &str,
    project: &Path,
    global: &Path,
) -> Result<Vec<String>, String> {
    let source = match resolver::resolve_list(None, Some(token), project, global) {
        Ok(source) => source,
        Err(error) if error.contains("is not registered") => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let locator = if source.contains("://") {
        sanitized_remote_locator(&source)
            .ok()
            .map(|url| url.to_string())
    } else {
        local_manifest_locator(&source)
    };
    let Some(locator) = locator else {
        return Ok(Vec::new());
    };
    Ok(lock
        .providers
        .iter()
        .filter(|entry| entry.declared_source == locator || entry.resolved_source == locator)
        .map(|entry| entry.provider_id.clone())
        .collect())
}

fn sanitized_remote_locator(source: &str) -> Result<url::Url, String> {
    let mut url = url::Url::parse(source).map_err(|_| "Invalid remote manifest locator")?;
    if url.scheme() != "https" {
        return Err("Remote manifest locator must use HTTPS".into());
    }
    url.set_username("")
        .map_err(|_| "Invalid remote manifest locator")?;
    url.set_password(None)
        .map_err(|_| "Invalid remote manifest locator")?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn local_manifest_locator(source: &str) -> Option<String> {
    let path = PathBuf::from(source);
    match fs::symlink_metadata(&path) {
        Ok(_) => registry::canonical_local_manifest(source)
            .ok()
            .and_then(|path| url::Url::from_file_path(path).ok())
            .map(|url| url.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let absolute = if path.is_absolute() {
                path
            } else {
                env::current_dir().ok()?.join(path)
            };
            let normalized = normalize_absolute_path(&absolute)?;
            let mut ancestor = normalized.as_path();
            let mut missing = Vec::new();
            loop {
                match fs::symlink_metadata(ancestor) {
                    Ok(metadata) => {
                        if !metadata.is_dir() {
                            return None;
                        }
                        let mut resolved = fs::canonicalize(ancestor).ok()?;
                        for component in missing.iter().rev() {
                            resolved.push(component);
                        }
                        return url::Url::from_file_path(resolved)
                            .ok()
                            .map(|url| url.to_string());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        missing.push(ancestor.file_name()?.to_os_string());
                        ancestor = ancestor.parent()?;
                    }
                    Err(_) => return None,
                }
            }
        }
        Err(_) => None,
    }
}

fn normalize_absolute_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            std::path::Component::Normal(name) => normalized.push(name),
        }
    }
    Some(normalized)
}

fn candidate_provenance(
    loaded: &registry::LoadedRegistry,
    declared: &str,
) -> Result<(String, String, String), String> {
    match &loaded.source {
        registry::RegistrySource::Local { .. } => {
            let path = registry::canonical_local_manifest(declared)?;
            let locator = url::Url::from_file_path(path)
                .map_err(|_| "Cannot represent manifest provenance as a file URL")?
                .to_string();
            let provider = loaded.v2.as_ref().map_or_else(
                || lockfile::v1_provider_id(&locator),
                |v2| Ok(format!("v2:{}", v2.id)),
            )?;
            Ok((provider, locator.clone(), locator))
        }
        registry::RegistrySource::Remote { manifest_url } => {
            let resolved = sanitized_remote_locator(manifest_url.as_str())?.to_string();
            let declared = sanitized_remote_locator(declared)?.to_string();
            let provider = loaded.v2.as_ref().map_or_else(
                || lockfile::v1_provider_id(&resolved),
                |v2| Ok(format!("v2:{}", v2.id)),
            )?;
            Ok((provider, declared, resolved))
        }
    }
}

fn select_mutation_item<'a>(
    lock: &'a lockfile::Lock,
    selector: &str,
    project: &Path,
    global: &Path,
) -> Result<&'a lockfile::Item, String> {
    let matches = select_status_items(lock, Some(selector), project, global)?;
    matches
        .into_iter()
        .next()
        .ok_or_else(|| format!("Managed item '{selector}' is not recorded in friladd.lock.json"))
}

fn update(args: UpdateArgs) -> Result<(), String> {
    let (project_config, global) = roots(args.project.clone())?;
    let project_root = project_config
        .parent()
        .ok_or_else(|| "Project configuration path has no parent".to_string())?;
    if !args.dry_run {
        transaction::recover(project_root)?;
    }
    let snapshot = lockfile::load(project_root)?;
    let before_lock = snapshot.lock().clone();
    let existing =
        select_mutation_item(&before_lock, &args.item, &project_config, &global)?.clone();
    let candidate = args.registry.as_str();
    let candidate_location =
        resolver::resolve_list(Some(candidate), None, &project_config, &global)?;
    let loaded = registry::load_registry(&candidate_location)?;
    let (provider_id, declared_source, resolved_source) =
        candidate_provenance(&loaded, &candidate_location)?;
    if provider_id != existing.provider_id {
        return Err(format!(
            "Candidate provider {provider_id} does not match installed provider {}; refusing provider switch",
            existing.provider_id
        ));
    }
    let v2_item = if let Some(v2) = &loaded.v2 {
        Some(v2.select(&existing.item_id, args.version.as_deref())?)
    } else {
        if args.version.is_some() || args.allow_downgrade {
            return Err("--version and --allow-downgrade apply only to Registry v2 items".into());
        }
        None
    };
    if let (Some(old), Some(new)) = (
        existing.version.as_deref(),
        v2_item.map(|item| item.version.as_str()),
    ) && semver::Version::parse(new).map_err(|_| "Invalid candidate semantic version")?
        < semver::Version::parse(old).map_err(|_| "Invalid installed semantic version")?
        && !args.allow_downgrade
    {
        return Err(format!(
            "Candidate version {new} is older than installed version {old}; pass --allow-downgrade to confirm"
        ));
    }
    if v2_item.is_some() {
        let root_item_id = existing.item_id.clone();
        return add_v2_graph(GraphRequest {
            dry_run: args.dry_run,
            json: args.json,
            project_root,
            project_config: &project_config,
            global_config: &global,
            declared_root_source: &candidate_location,
            root_loaded: loaded,
            root_item_id: &root_item_id,
            updating: Some(existing),
            allow_downgrade: args.allow_downgrade,
        });
    }
    let candidate_item = loaded.registry.find(&existing.item_id).ok_or_else(|| {
        format!(
            "Candidate manifest does not contain item {}",
            existing.item_id
        )
    })?;
    for file in &existing.files {
        match transaction::observe(project_root, &file.target)? {
            transaction::Observation::Content(content) if content == file.installed => {}
            transaction::Observation::Missing => {
                return Err(format!(
                    "Managed file {} is missing; restore it before updating",
                    file.target
                ));
            }
            transaction::Observation::Content(_) => {
                return Err(format!(
                    "Managed file {} was modified; preserve it and resolve the edit before updating",
                    file.target
                ));
            }
            transaction::Observation::Unsafe => {
                return Err(format!(
                    "Managed path {} is unsafe; refusing update",
                    file.target
                ));
            }
            transaction::Observation::Unreadable => {
                return Err(format!(
                    "Managed file {} is unreadable; refusing update",
                    file.target
                ));
            }
        }
    }
    let mapped_item = crate::registry::RegistryItem {
        name: candidate_item.name.clone(),
        description: candidate_item.description.clone(),
        files: candidate_item
            .files
            .iter()
            .map(|file| crate::registry::RegistryFile {
                source: file.source.clone(),
                target: existing.target_root.as_ref().map_or_else(
                    || file.target.clone(),
                    |root| format!("{root}/{}", file.target),
                ),
                sha256: file.sha256.clone(),
            })
            .collect(),
    };
    let plan = planner::build_plan(project_root, &mapped_item)?;
    let owners = before_lock.owners();
    for target in &plan.conflicts {
        if !owners.get(target.as_str()).is_some_and(|(owner, _)| {
            owner.provider_id == existing.provider_id && owner.item_id == existing.item_id
        }) {
            return Err(format!(
                "Candidate target {target} conflicts with an unmanaged or separately owned file"
            ));
        }
    }
    let contents = if let Some(v2) = v2_item {
        let sizes = v2_plan_sizes(v2, &plan)?;
        downloader::acquire_v2_files_with_sizes(&loaded.source, &plan, &sizes)?
    } else {
        downloader::acquire_files(&loaded.source, &plan)?
    };
    let manifest_source =
        url::Url::parse(&resolved_source).map_err(|_| "Invalid candidate manifest provenance")?;
    let mut candidate_files = Vec::with_capacity(plan.files.len());
    let mut content_by_target = std::collections::BTreeMap::new();
    for ((file, declaration), acquired) in plan
        .files
        .iter()
        .zip(&candidate_item.files)
        .zip(&contents.files)
    {
        let installed = lockfile::Content {
            sha256: acquired.sha256.clone(),
            size: acquired.size,
        };
        if let Some(v2) = v2_item
            && v2
                .files
                .iter()
                .find(|entry| entry.source == file.source)
                .is_some_and(|entry| entry.size != installed.size)
        {
            return Err(format!(
                "Size mismatch for candidate source {}",
                file.source
            ));
        }
        let source_locator = match &acquired.resolved_locator {
            Some(locator) => locator.clone(),
            None => manifest_source
                .join(&file.source)
                .map_err(|_| "Invalid candidate artifact locator")?
                .to_string(),
        };
        candidate_files.push(lockfile::OwnedFile {
            source: file.source.clone(),
            resolved_source: source_locator,
            declared_target: declaration.target.clone(),
            target: file.target_display.clone(),
            expected_sha256: installed.sha256.clone(),
            installed: installed.clone(),
        });
        content_by_target.insert(file.target_display.clone(), acquired.path.clone());
    }
    candidate_files.sort_by(|left, right| left.target.cmp(&right.target));
    let candidate_lock_item = lockfile::Item {
        provider_id: existing.provider_id.clone(),
        item_id: existing.item_id.clone(),
        version: v2_item.map(|item| item.version.clone()),
        manifest_source: resolved_source.clone(),
        manifest_sha256: loaded.manifest_sha256.clone(),
        target_root: existing.target_root.clone(),
        dependencies: Vec::new(),
        files: candidate_files,
    };
    let dependents = before_lock.items.iter().any(|item| {
        item != &existing
            && item.dependencies.iter().any(|dependency| {
                dependency.provider_id == existing.provider_id
                    && dependency.item_id == existing.item_id
            })
    });
    if dependents && candidate_lock_item != existing {
        return Err("Updating this item would affect dependent resources; dependency-aware lifecycle is unavailable".into());
    }
    let mut next = before_lock.clone();
    let provider = next
        .providers
        .iter_mut()
        .find(|provider| provider.provider_id == existing.provider_id)
        .ok_or("Installed item has no provider record")?;
    provider.declared_source = declared_source;
    provider.resolved_source = resolved_source;
    provider.manifest_sha256 = loaded.manifest_sha256.clone();
    let index = next
        .items
        .iter()
        .position(|item| {
            item.provider_id == existing.provider_id && item.item_id == existing.item_id
        })
        .ok_or("Installed item disappeared from lock snapshot")?;
    next.items[index] = candidate_lock_item.clone();
    let old_files: std::collections::BTreeMap<_, _> = existing
        .files
        .iter()
        .map(|file| (file.target.as_str(), file))
        .collect();
    let new_files: std::collections::BTreeMap<_, _> = candidate_lock_item
        .files
        .iter()
        .map(|file| (file.target.as_str(), file))
        .collect();
    let targets: std::collections::BTreeSet<_> =
        old_files.keys().chain(new_files.keys()).copied().collect();
    let mut edits = Vec::new();
    let mut actions = Vec::new();
    for target in targets {
        let old = old_files.get(target).copied();
        let new = new_files.get(target).copied();
        match (old, new) {
            (Some(old), Some(new)) if old.installed == new.installed => {
                actions.push(json_file_action(
                    target,
                    "unchanged",
                    Some(&existing),
                    Some(&candidate_lock_item),
                ));
            }
            (Some(old), Some(_new)) => {
                edits.push(transaction::Edit {
                    target: target.to_owned(),
                    before: Some(old.installed.clone()),
                    write: Some(transaction::WriteSource::File(
                        content_by_target
                            .get(target)
                            .ok_or("Missing verified candidate bytes")?
                            .clone(),
                    )),
                });
                actions.push(json_file_action(
                    target,
                    "replace",
                    Some(&existing),
                    Some(&candidate_lock_item),
                ));
            }
            (None, Some(_new)) => {
                edits.push(transaction::Edit {
                    target: target.to_owned(),
                    before: None,
                    write: Some(transaction::WriteSource::File(
                        content_by_target
                            .get(target)
                            .ok_or("Missing verified candidate bytes")?
                            .clone(),
                    )),
                });
                actions.push(json_file_action(
                    target,
                    "install",
                    None,
                    Some(&candidate_lock_item),
                ));
            }
            (Some(old), None) => {
                edits.push(transaction::Edit {
                    target: target.to_owned(),
                    before: Some(old.installed.clone()),
                    write: None,
                });
                actions.push(json_file_action(target, "remove", Some(&existing), None));
            }
            (None, None) => unreachable!(),
        }
    }
    let transaction = transaction::Plan::new(snapshot, next, edits)?;
    transaction.check(project_root)?;
    if args.dry_run {
        print_mutation_result(
            "update",
            true,
            vec![json_item_actions(&existing, actions)],
            args.json,
        );
        return Ok(());
    }
    transaction::apply(project_root, transaction)?;
    print_mutation_result(
        "update",
        false,
        vec![json_item_actions(&existing, actions)],
        args.json,
    );
    Ok(())
}

fn remove(args: ManagedMutationArgs) -> Result<(), String> {
    let (project_config, global) = roots(args.project)?;
    let project_root = project_config
        .parent()
        .ok_or_else(|| "Project configuration path has no parent".to_string())?;
    if !args.dry_run {
        transaction::recover(project_root)?;
    }
    let snapshot = lockfile::load(project_root)?;
    let mut next = snapshot.lock().clone();
    let existing = select_mutation_item(&next, &args.item, &project_config, &global)?.clone();
    if next.items.iter().any(|item| {
        (item.provider_id != existing.provider_id || item.item_id != existing.item_id)
            && item.dependencies.iter().any(|dependency| {
                dependency.provider_id == existing.provider_id
                    && dependency.item_id == existing.item_id
            })
    }) {
        return Err("Another managed item depends on this item; remove dependents first".into());
    }
    let index = next
        .items
        .iter()
        .position(|item| {
            item.provider_id == existing.provider_id && item.item_id == existing.item_id
        })
        .ok_or("Installed item disappeared from lock snapshot")?;
    next.items.remove(index);
    next.providers.retain(|provider| {
        next.items
            .iter()
            .any(|item| item.provider_id == provider.provider_id)
    });
    let mut edits = Vec::new();
    let mut actions = Vec::new();
    for file in &existing.files {
        match transaction::observe(project_root, &file.target)? {
            transaction::Observation::Content(content) if content == file.installed => {
                edits.push(transaction::Edit {
                    target: file.target.clone(),
                    before: Some(content),
                    write: None,
                });
                actions.push(json_file_action(
                    &file.target,
                    "remove",
                    Some(&existing),
                    None,
                ));
            }
            transaction::Observation::Missing => {
                edits.push(transaction::Edit {
                    target: file.target.clone(),
                    before: None,
                    write: None,
                });
                actions.push(json_file_action(
                    &file.target,
                    "already-missing",
                    Some(&existing),
                    None,
                ));
            }
            transaction::Observation::Content(_) => {
                return Err(format!(
                    "Managed file {} was modified; preserve it and resolve the edit before removal",
                    file.target
                ));
            }
            transaction::Observation::Unsafe => {
                return Err(format!(
                    "Managed path {} is unsafe; refusing removal",
                    file.target
                ));
            }
            transaction::Observation::Unreadable => {
                return Err(format!(
                    "Managed file {} is unreadable; refusing removal",
                    file.target
                ));
            }
        }
    }
    let transaction = transaction::Plan::new(snapshot, next, edits)?;
    transaction.check(project_root)?;
    let result = vec![json_item_actions(&existing, actions)];
    if args.dry_run {
        print_mutation_result("remove", true, result, args.json);
        return Ok(());
    }
    transaction::apply(project_root, transaction)?;
    print_mutation_result("remove", false, result, args.json);
    Ok(())
}

fn restore(args: RestoreArgs) -> Result<(), String> {
    let (project_config, global) = roots(args.project)?;
    let project_root = project_config
        .parent()
        .ok_or_else(|| "Project configuration path has no parent".to_string())?;
    if !args.dry_run {
        transaction::recover(project_root)?;
    }
    let snapshot = lockfile::load(project_root)?;
    let before = snapshot.lock().clone();
    let selected: Vec<_> =
        if let Some(selector) = args.item.as_deref() {
            let root = select_mutation_item(&before, selector, &project_config, &global)?;
            let mut closure = std::collections::BTreeSet::<(String, String)>::new();
            let mut pending = vec![(root.provider_id.clone(), root.item_id.clone())];
            while let Some(key) = pending.pop() {
                if !closure.insert(key.clone()) {
                    continue;
                }
                let item = before
                    .items
                    .iter()
                    .find(|item| item.provider_id == key.0 && item.item_id == key.1)
                    .ok_or("Lock dependency closure is incomplete")?;
                pending.extend(item.dependencies.iter().map(|dependency| {
                    (dependency.provider_id.clone(), dependency.item_id.clone())
                }));
            }
            before
                .items
                .iter()
                .filter(|item| closure.contains(&(item.provider_id.clone(), item.item_id.clone())))
                .cloned()
                .collect()
        } else {
            before.items.clone()
        };
    let mut edits = Vec::new();
    let mut item_results = Vec::new();
    let mut restore_bytes = 0_u64;
    let mut restore_requests = Vec::new();
    let mut restore_targets = Vec::new();
    for item in &selected {
        let mut actions = Vec::new();
        for file in &item.files {
            match transaction::observe(project_root, &file.target)? {
                transaction::Observation::Content(content) if content == file.installed => {
                    actions.push(json_file_action(
                        &file.target,
                        "unchanged",
                        Some(item),
                        Some(item),
                    ));
                }
                transaction::Observation::Missing => {
                    restore_bytes = reserve_restore_bytes(restore_bytes, file.installed.size)?;
                    restore_requests.push(downloader::LockedArtifact {
                        locator: file.resolved_source.clone(),
                        expected_sha256: file.installed.sha256.clone(),
                        expected_size: file.installed.size,
                        max_bytes: if item.version.is_some() {
                            downloader::MAX_V2_FILE_BYTES
                        } else {
                            downloader::MAX_FILE_BYTES
                        },
                    });
                    restore_targets.push(file.target.clone());
                    actions.push(json_file_action(
                        &file.target,
                        "restore",
                        Some(item),
                        Some(item),
                    ));
                }
                transaction::Observation::Content(_) => {
                    return Err(format!(
                        "Managed file {} was modified; restore never overwrites edits",
                        file.target
                    ));
                }
                transaction::Observation::Unsafe => {
                    return Err(format!(
                        "Managed path {} is unsafe; refusing restore",
                        file.target
                    ));
                }
                transaction::Observation::Unreadable => {
                    return Err(format!(
                        "Managed file {} is unreadable; refusing restore",
                        file.target
                    ));
                }
            }
        }
        item_results.push(json_item_actions(item, actions));
    }
    let restored = downloader::acquire_locked_files(&restore_requests)?;
    for (target, artifact) in restore_targets.into_iter().zip(restored.files) {
        edits.push(transaction::Edit {
            target,
            before: None,
            write: Some(transaction::WriteSource::File(artifact.path)),
        });
    }
    let transaction = transaction::Plan::new(snapshot, before, edits)?;
    transaction.check(project_root)?;
    if args.dry_run {
        print_mutation_result("restore", true, item_results, args.json);
        return Ok(());
    }
    transaction::apply(project_root, transaction)?;
    print_mutation_result("restore", false, item_results, args.json);
    Ok(())
}

fn reserve_restore_bytes(current: u64, size: u64) -> Result<u64, String> {
    let total = current
        .checked_add(size)
        .ok_or("Restore exceeds the transaction byte limit")?;
    if total > transaction::MAX_OPERATION_BYTES {
        return Err("Restore exceeds the 512 MiB transaction byte limit".into());
    }
    Ok(total)
}

fn v2_plan_sizes(
    item: &crate::registry_v2::Item,
    plan: &planner::InstallationPlan,
) -> Result<Vec<u64>, String> {
    plan.files
        .iter()
        .map(|file| {
            item.files
                .iter()
                .find(|entry| entry.source == file.source)
                .map(|entry| entry.size)
                .ok_or_else(|| format!("No size declaration for source {}", file.source))
        })
        .collect()
}

fn json_file_action(
    target: &str,
    action: &str,
    owner_before: Option<&lockfile::Item>,
    owner_after: Option<&lockfile::Item>,
) -> serde_json::Value {
    let owner = |item: Option<&lockfile::Item>| {
        item.map(
            |item| serde_json::json!({ "providerId": item.provider_id, "itemId": item.item_id }),
        )
    };
    serde_json::json!({
        "target": target,
        "action": action,
        "ownerBefore": owner(owner_before),
        "ownerAfter": owner(owner_after),
    })
}

fn json_item_actions(item: &lockfile::Item, actions: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "providerId": item.provider_id,
        "itemId": item.item_id,
        "actions": actions,
    })
}

fn print_mutation_result(command: &str, dry_run: bool, items: Vec<serde_json::Value>, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "schemaVersion": 1,
                "command": command,
                "data": { "dryRun": dry_run, "items": items }
            })
        );
        return;
    }
    if items.is_empty() {
        println!("No managed files require this operation.");
        return;
    }
    for item in items {
        let provider = item["providerId"].as_str().unwrap_or_default();
        let id = item["itemId"].as_str().unwrap_or_default();
        println!("{command} {provider}/{id}");
        if let Some(actions) = item["actions"].as_array() {
            for action in actions {
                println!(
                    "  {} {}",
                    action["action"].as_str().unwrap_or("unknown"),
                    action["target"].as_str().unwrap_or("")
                );
            }
        }
    }
    if dry_run {
        println!("Dry run: no project files or lock records were changed.");
    }
}

fn doctor(args: DoctorArgs) -> Result<(), String> {
    let report = crate::doctor::inspect(args.project, args.online)?;
    if args.json {
        print_catalog_json("doctor", report.data);
    } else {
        crate::doctor::print_human(&report.data);
    }
    if report.failed {
        Err("Doctor found failed required checks; see the diagnostic report.".into())
    } else {
        Ok(())
    }
}
fn verify_registry(args: RegistryVerifyArgs) -> Result<(), String> {
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
    if args.json {
        print_catalog_json(
            "registry verify",
            serde_json::json!({
                "registry": loaded.registry.name,
                "manifestSha256": loaded.manifest_sha256,
                "itemCount": loaded.registry.items.len(),
                "fileCount": count,
            }),
        );
    } else {
        println!(
            "Verified registry {}: {} item(s), {} file(s).",
            loaded.registry.name,
            loaded.registry.items.len(),
            count
        );
        if loaded.v2.is_some() {
            println!("Manifest SHA-256: {}", loaded.manifest_sha256);
        }
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
            json,
        } => {
            let mut entries = Vec::new();
            if global {
                let gp = config::global_path()?;
                let gc = config::load(&gp)?;
                if json {
                    entries.extend(registry_entries_json(&gc, "global"));
                } else {
                    print_registry_entries(&gc, "global");
                }
            } else {
                let (pp, gp) = roots(project_arg)?;
                let pc = config::load(&pp)?;
                let gc = config::load(&gp)?;
                if json {
                    entries.extend(registry_entries_json(&pc, "project"));
                    entries.extend(registry_entries_json(&gc, "global").into_iter().filter(
                        |entry| {
                            !pc.registries
                                .contains_key(entry["alias"].as_str().unwrap_or_default())
                        },
                    ));
                } else {
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
            }
            if json {
                print_catalog_json(
                    "registry list",
                    serde_json::json!({ "registries": entries }),
                );
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
fn registry_entries_json(c: &Config, scope: &str) -> Vec<serde_json::Value> {
    c.registries
        .iter()
        .map(|(alias, entry)| {
            serde_json::json!({
                "alias": alias,
                "source": registry::display_location(&entry.source),
                "scope": scope,
                "default": c.default_registry.as_deref() == Some(alias),
            })
        })
        .collect()
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

    #[test]
    fn remote_manifest_provenance_removes_credentials_and_suffixes() {
        let locator = sanitized_remote_locator(
            "https://user:secret@example.test/registry.json?token=hidden#fragment",
        )
        .unwrap();
        assert_eq!(locator.as_str(), "https://example.test/registry.json");
    }

    #[test]
    fn restore_byte_budget_rejects_overflow_before_acquisition() {
        assert_eq!(
            reserve_restore_bytes(transaction::MAX_OPERATION_BYTES - 1, 1).unwrap(),
            transaction::MAX_OPERATION_BYTES
        );
        assert!(reserve_restore_bytes(transaction::MAX_OPERATION_BYTES, 1).is_err());
        assert!(reserve_restore_bytes(u64::MAX, 1).is_err());
    }
}
