mod cli;
mod config;
mod downloader;
mod installer;
mod planner;
mod registry;
mod resolver;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}
