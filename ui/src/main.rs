/// Binary entry point for the CSV Explorer UI.
///
/// Supports both web and desktop targets via feature flags.
///
/// Usage:
///   csv-explorer-ui serve    - Serve the UI (web mode)
///   csv-explorer-ui desktop  - Launch desktop app
use clap::{Parser, Subcommand};

mod app;
mod components;
mod hooks;
mod pages;
mod styles;

/// CSV Explorer UI - Multiplatform explorer interface
#[derive(Parser)]
#[command(name = "csv-explorer-ui")]
#[command(about = "Multiplatform UI for CSV Explorer", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Serve the UI in web mode
    Serve,
    /// Launch the desktop application
    Desktop,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve => {
            println!("Starting CSV Explorer UI in web mode...");
            println!("Open http://localhost:3000 in your browser");
            launch_web();
        }
        Commands::Desktop => {
            println!("Starting CSV Explorer UI in desktop mode...");
            launch_desktop();
        }
    }
}

#[cfg(feature = "web")]
fn launch_web() {
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], 3000).into();

    println!("Starting server on {}", addr);

    dioxus::launch(app::App);
}

#[cfg(not(feature = "web"))]
async fn launch_web() {
    eprintln!("Web feature not enabled. Build with --features web");
}

#[cfg(feature = "desktop")]
fn launch_desktop() {
    dioxus::launch(app::App);
}

#[cfg(not(feature = "desktop"))]
fn launch_desktop() {
    eprintln!("Desktop feature not enabled. Build with --features desktop");
}
