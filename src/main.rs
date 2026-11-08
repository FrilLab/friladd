use std::{env, process};

fn main() {
    // Collect arguments, excluding the executable name.
    let args: Vec<String> = env::args().skip(1).collect();

    if let Err(message) = run(&args) {
        eprintln!("Error: {message}");
        eprintln!("Run 'friladd help' for usage.");
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

            println!("Available components (sample data):");
            println!("- envelope-reveal");
            Ok(())
        }

        Some("add") => {
            if args.len() != 2 || args[1].starts_with('-') || args[1].trim().is_empty() {
                return Err("Usage: friladd add <component>".to_string());
            }

            println!("Selected component: {}", args[1]);
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

        let result = run(&args);

        assert!(result.is_err());
    }

    #[test]
    fn add_without_name_returns_error() {
        let args = vec!["add".to_string()];

        let result = run(&args);

        assert!(result.is_err());
    }
}
