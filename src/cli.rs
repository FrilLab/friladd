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
    about = "Install selected source files from registries"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Debug, Subcommand)]
enum Command {
    /// List items in a registry.
    List(ListArgs),
    /// Install one item from a registry.
    Add(AddArgs),
    /// Manage named registry sources.
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
}
#[derive(Debug, Args)]
struct ListArgs {
    /// Registry alias (or use --registry for a path or URL).
    registry_alias: Option<String>,
    #[arg(long)]
    registry: Option<String>,
    #[arg(long)]
    project: Option<PathBuf>,
}
#[derive(Debug, Args)]
struct AddArgs {
    component: String,
    #[arg(long)]
    registry: Option<String>,
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
}
#[derive(Debug, Subcommand)]
enum RegistryCommand {
    Add {
        alias: String,
        source: String,
        #[arg(long)]
        global: bool,
        #[arg(long)]
        default: bool,
    },
    List {
        #[arg(long)]
        global: bool,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    Remove {
        alias: String,
        #[arg(long)]
        global: bool,
    },
    Default {
        alias: String,
        #[arg(long)]
        global: bool,
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
    let root = match project {
        Some(p) => p,
        None => {
            env::current_dir().map_err(|e| format!("Cannot determine current directory: {e}"))?
        }
    };
    let root = if root.exists() {
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
    Ok((config::project_path(&root), config::global_path()?))
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
    for item in &loaded.registry.items {
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
        .find(resolution.item.as_deref().unwrap())
        .ok_or_else(|| format!("Component not found: {}", resolution.item.unwrap()))?;
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
fn manage(command: RegistryCommand) -> Result<(), String> {
    match command {
        RegistryCommand::Add {
            alias,
            source,
            global,
            default,
        } => {
            config::validate_alias(&alias)?;
            let (project, global_path) = roots(None)?;
            let (path, scope) = if global {
                (global_path, Scope::Global)
            } else {
                (project, Scope::Project)
            };
            let location = if global && !source.contains("://") {
                let p = PathBuf::from(&source);
                let cwd = env::current_dir()
                    .map_err(|e| format!("Cannot determine current directory: {e}"))?;
                cwd.join(p)
                    .canonicalize()
                    .map_err(|e| format!("Cannot resolve registry {}: {e}", source))?
                    .to_string_lossy()
                    .into_owned()
            } else if !source.contains("://") {
                let p = PathBuf::from(&source);
                let cwd = env::current_dir()
                    .map_err(|e| format!("Cannot determine current directory: {e}"))?;
                let abs = cwd
                    .join(p)
                    .canonicalize()
                    .map_err(|e| format!("Cannot resolve registry {}: {e}", source))?;
                let root = path.parent().unwrap();
                pathdiff::diff_paths(&abs, root)
                    .unwrap_or(abs)
                    .to_string_lossy()
                    .into_owned()
            } else {
                source.clone()
            };
            registry::load_registry(&location)?;
            let mut config = config::load(&path)?;
            if config.registries.contains_key(&alias) {
                return Err(format!(
                    "Registry alias '{alias}' already exists in {} configuration",
                    scope.label()
                ));
            }
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
            let (pp, gp) = roots(project_arg)?;
            let pc = config::load(&pp)?;
            let gc = config::load(&gp)?;
            if global {
                print_registry_entries(&gc, "global");
            } else {
                print_registry_entries(&pc, "project");
                for (a, e) in &gc.registries {
                    if !pc.registries.contains_key(a) {
                        println!(
                            "{a}\t{}{}",
                            e.source,
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
        RegistryCommand::Remove { alias, global } => {
            config::validate_alias(&alias)?;
            let (pp, gp) = roots(None)?;
            let path = if global { gp } else { pp };
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
        RegistryCommand::Default { alias, global } => {
            config::validate_alias(&alias)?;
            let (pp, gp) = roots(None)?;
            let path = if global { gp } else { pp };
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
fn print_registry_entries(c: &Config, scope: &str) {
    for (a, e) in &c.registries {
        println!(
            "{a}\t{}\t{scope}{}",
            e.source,
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
