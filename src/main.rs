mod cli;
mod downloader;
mod installer;
mod planner;
mod registry;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}
