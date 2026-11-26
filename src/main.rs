fn main() {
    if let Err(error) = friladd::cli::run() {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}
