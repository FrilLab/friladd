mod registry;

use registry::load_registry;
use std::{env, path::Path, process};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if let Err(message) = run(&args) {
        eprintln!("Error: {message}");
        process::exit(2);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            print_help();
            Ok(())
        }

        Some("list") => {
            if args.len() != 1 {
                return Err("Usage: friladd list".to_string());
            }

            let registry = load_registry(Path::new("registry/registry.json"))?;

            println!("Available components ({}):", registry.name);

            for item in &registry.items {
                println!("- {}: {}", item.name, item.description);
            }

            Ok(())
        }

        Some("add") => {
            if args.len() != 2 || args[1].starts_with('-') || args[1].trim().is_empty() {
                return Err("Usage: friladd add <component>".to_string());
            }

            let registry = load_registry(Path::new("registry/registry.json"))?;

            let name = &args[1];

            let item = registry
                .find(name)
                .ok_or_else(|| format!("Component not found: {name}"))?;

            println!("Selected component: {}", item.name);
            println!("Description: {}", item.description);
            println!("No files have been installed yet.");

            Ok(())
        }

        Some(command) => Err(format!("Unknown command: {command}")),
    }
}

fn print_help() {
    println!("FrilAdd — Install only what you need.");
    println!();
    println!("Usage:");
    println!("  friladd help");
    println!("  friladd list");
    println!("  friladd add <component>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_command_returns_error() {
        let args = vec!["delete".to_string()];

        assert!(run(&args).is_err());
    }

    #[test]
    fn add_without_name_returns_error() {
        let args = vec!["add".to_string()];

        assert!(run(&args).is_err());
    }
}
