fn main() {
    let command_name = std::env::var("FRILADD_LAUNCHER_COMMAND")
        .ok()
        .filter(|name| matches!(name.as_str(), "fril" | "friladd"))
        .unwrap_or_else(|| env!("CARGO_BIN_NAME").to_string());
    if let Err(error) = friladd::cli::run_as(&command_name) {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}
