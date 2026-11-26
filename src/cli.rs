use crate::{
    config::{self, Config, RegistryEntry, Scope},
    downloader, installer, planner, registry, resolver,
};
use clap::{Args, Parser, Subcommand};
use std::{env, fs, path::PathBuf};

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
        println!("- {}: {}", item.name, item.description);
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
    let plan = planner::build_plan(&project_root, item)?;
    print_plan(&plan, &loaded.registry.name, item.name.as_str());
    if !plan.conflicts.is_empty() {
        return Err(format!(
            "{} destination file(s) already exist; remove them before installing",
            plan.conflicts.len()
        ));
    }
    let contents = downloader::acquire_files(&loaded.source, &plan)?;
    if args.dry_run {
        println!("Dry run: no project files were changed.");
        return Ok(());
    }
    installer::install(&plan, &contents)?;
    for file in &plan.files {
        println!("Installed {}", file.target_display);
    }
    Ok(())
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
    let count = downloader::verify_registry_files(&loaded.source, &loaded.registry)?;
    println!(
        "Verified registry {}: {} item(s), {} file(s).",
        loaded.registry.name,
        loaded.registry.items.len(),
        count
    );
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
