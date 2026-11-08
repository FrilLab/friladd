use clap::{Args, Parser, Subcommand};
use std::{env, path::PathBuf};

use crate::{downloader, installer, planner, registry};

#[derive(Debug, Parser)]
#[command(
    name = "friladd",
    version,
    about = "Install selected source files from a registry"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List the items in a registry.
    List(RegistryArgs),
    /// Install one item from a registry.
    Add(AddArgs),
}

#[derive(Debug, Args)]
struct RegistryArgs {
    /// Local manifest path or HTTPS manifest URL.
    #[arg(long, required = true)]
    registry: String,
}

#[derive(Debug, Args)]
struct AddArgs {
    /// Registry item name.
    component: String,
    /// Local manifest path or HTTPS manifest URL.
    #[arg(long, required = true)]
    registry: String,
    /// Destination project directory (defaults to the current directory).
    #[arg(long)]
    project: Option<PathBuf>,
    /// Show the validated installation plan without writing project files.
    #[arg(long)]
    dry_run: bool,
}

pub fn run() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Command::List(args) => list(&args.registry),
        Command::Add(args) => add(args),
    }
}

fn list(location: &str) -> Result<(), String> {
    let loaded = registry::load_registry(location)?;
    println!("Available items ({}):", loaded.registry.name);
    for item in &loaded.registry.items {
        println!("- {}: {}", item.name, item.description);
    }
    Ok(())
}

fn add(args: AddArgs) -> Result<(), String> {
    let loaded = registry::load_registry(&args.registry)?;
    let item = loaded
        .registry
        .find(&args.component)
        .ok_or_else(|| format!("Component not found: {}", args.component))?;
    let project = args.project.unwrap_or(
        env::current_dir().map_err(|e| format!("Cannot determine current directory: {e}"))?,
    );
    let plan = planner::build_plan(&project, item)?;

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

fn print_plan(plan: &planner::InstallationPlan, registry_name: &str, component: &str) {
    println!("FrilAdd installation plan\n");
    println!("Registry: {registry_name}");
    println!("Component: {component}");
    println!("Project: {}\n", plan.project_root.display());
    println!("Files:");
    for file in &plan.files {
        if file.exists {
            println!("  ! {} (already exists)", file.target_display);
        } else {
            println!("  + {}", file.target_display);
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
