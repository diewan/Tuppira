/// Binary entry point for the CSV Explorer API server.
///
/// Subcommands:
///   start       - Start the API server
///   graphql     - Open GraphQL playground URL
///   health      - Check API health
use clap::{Parser, Subcommand};
use csv_explorer_api::ApiServer;
use csv_explorer_shared::{ExplorerConfig, Result};

/// CSV Explorer API - GraphQL and REST API server
#[derive(Parser)]
#[command(name = "csv-explorer-api")]
#[command(about = "GraphQL and REST API server for CSV Explorer", long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long)]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the API server
    Start,
    /// Print the GraphQL playground URL
    Graphql,
    /// Check API health
    Health,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load configuration
    let config = if let Some(ref path) = cli.config {
        ExplorerConfig::from_file(std::path::Path::new(path))?
    } else {
        ExplorerConfig::load()?
    };

    match cli.command {
        Commands::Start => run_start(config).await,
        Commands::Graphql => {
            run_graphql_url(&config);
            Ok(())
        }
        Commands::Health => run_health(&config).await,
    }
}

async fn run_start(config: ExplorerConfig) -> Result<()> {
    tracing::info!("Starting API server");
    let server = ApiServer::new(config).await?;
    server.start().await
}

fn run_graphql_url(config: &ExplorerConfig) {
    let url = format!("http://{}:{}/playground", config.api.host, config.api.port);
    println!("GraphQL Playground: {}", url);
}

async fn run_health(config: &ExplorerConfig) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("http://{}/health", config.api.bind());

    match client.get(&url).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                println!("API server is healthy");
                Ok(())
            } else {
                println!("API server returned status: {}", resp.status());
                Err(csv_explorer_shared::ExplorerError::Internal(format!(
                    "Health check failed with status: {}",
                    resp.status()
                )))
            }
        }
        Err(e) => {
            println!("API server is not reachable: {}", e);
            Err(csv_explorer_shared::ExplorerError::Internal(format!(
                "Failed to connect to API server: {}",
                e
            )))
        }
    }
}
