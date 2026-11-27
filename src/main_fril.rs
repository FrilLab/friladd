fn main() {
    if let Err(error) = friladd::cli::run_as(env!("CARGO_BIN_NAME")) {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}
