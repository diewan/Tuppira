/// Binary entry point for the CSV Explorer indexer.
///
/// Subcommands:
///   start       - Start the indexer daemon
///   status      - Show indexer status
///   sync        - Force sync a specific chain
///   reindex     - Reindex from a specific block
///   reset       - Reset sync progress
use clap::{Parser, Subcommand};
use csv_explorer_indexer::Indexer;
use csv_explorer_shared::{ExplorerConfig, Result};
use csv_explorer_storage::init_pool;

/// CSV Explorer Indexer - Multi-chain indexing daemon
#[derive(Parser)]
#[command(name = "csv-explorer-indexer")]
#[command(about = "Multi-chain indexing daemon for CSV Explorer", long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long)]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the indexer daemon
    Start,
    /// Show indexer status
    Status,
    /// Force sync a specific chain
    Sync {
        /// Chain ID to sync (e.g., bitcoin, ethereum)
        chain: String,
        /// Optional: block number to start from (overrides config start_block)
        #[arg(short, long)]
        from_block: Option<u64>,
    },
    /// Reindex from a specific block
    Reindex {
        /// Chain ID to reindex
        chain: String,
        /// Block number to start from
        #[arg(short, long)]
        from_block: u64,
    },
    /// Reset sync progress
    Reset {
        /// Optional: specific chain to reset (resets all if omitted)
        chain: Option<String>,
    },
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

    // Initialize database
    let pool = init_pool(&config.database.url, config.database.max_connections).await?;

    // Create indexer
    let indexer = Indexer::new(config, pool).await?;

    match cli.command {
        Commands::Start => run_start(&indexer).await,
        Commands::Status => run_status(&indexer).await,
        Commands::Sync { chain, from_block } => run_sync(&indexer, &chain, from_block).await,
        Commands::Reindex { chain, from_block } => run_reindex(&indexer, &chain, from_block).await,
        Commands::Reset { chain } => run_reset(&indexer, chain).await,
    }
}

async fn run_start(indexer: &Indexer) -> Result<()> {
    tracing::info!("Starting indexer daemon");

    indexer.initialize().await?;
    indexer.start().await?;

    // Wait for shutdown signal
    wait_for_shutdown().await;

    indexer.stop().await?;
    Ok(())
}

async fn run_status(indexer: &Indexer) -> Result<()> {
    let status = indexer.status().await;

    println!("Indexer Status");
    println!("==============");
    println!("Running: {}", status.is_running);
    println!("Total Indexed Blocks: {}", status.total_indexed_blocks);

    if let Some(started) = status.started_at {
        println!("Started At: {}", started);
    }
    if let Some(uptime) = status.uptime_seconds {
        println!("Uptime: {}s", uptime);
    }

    println!("\nChain Status:");
    println!("-------------");
    for chain in &status.chains {
        println!(
            "  {:<12} {:<10} block {:>12}  status: {:?}",
            chain.id, chain.name, chain.latest_block, chain.status
        );
    }

    Ok(())
}

async fn run_sync(indexer: &Indexer, chain: &str, from_block: Option<u64>) -> Result<()> {
    if let Some(block) = from_block {
        tracing::info!(chain = %chain, from_block = block, "Forcing sync from specific block");
        indexer.sync_chain_from_block(chain, block).await?;
        println!("Sync completed for chain: {} from block {}", chain, block);
    } else {
        tracing::info!(chain = %chain, "Forcing sync");
        indexer.sync_chain(chain).await?;
        println!("Sync completed for chain: {}", chain);
    }
    Ok(())
}

async fn run_reindex(indexer: &Indexer, chain: &str, from_block: u64) -> Result<()> {
    tracing::info!(chain = %chain, from_block, "Starting reindex");
    indexer.reindex_from(chain, from_block).await?;
    println!(
        "Reindex completed for chain: {} from block {}",
        chain, from_block
    );
    Ok(())
}

async fn run_reset(indexer: &Indexer, chain: Option<String>) -> Result<()> {
    if let Some(chain) = chain {
        tracing::info!(chain = %chain, "Resetting sync progress");
        println!("Reset sync progress for chain: {}", chain);
    } else {
        tracing::info!("Resetting all sync progress");
        indexer.reset_sync().await?;
        println!("Reset all sync progress");
    }
    Ok(())
}

/// Wait for OS shutdown signal (SIGINT or SIGTERM).
async fn wait_for_shutdown() {
    use tokio::signal;

    let ctrl_c = signal::ctrl_c();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => Some(signal),
            Err(error) => {
                tracing::warn!(%error, "SIGTERM handling is unavailable");
                None
            }
        };
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(signal) => Some(signal),
            Err(error) => {
                tracing::warn!(%error, "SIGHUP handling is unavailable");
                None
            }
        };

        tokio::select! {
            result = ctrl_c => match result {
                Ok(()) => tracing::info!("Received SIGINT"),
                Err(error) => tracing::warn!(%error, "SIGINT handling failed"),
            },
            _ = async { match sigterm.as_mut() { Some(signal) => signal.recv().await, None => std::future::pending().await } } => tracing::info!("Received SIGTERM"),
            _ = async { match sighup.as_mut() { Some(signal) => signal.recv().await, None => std::future::pending().await } } => tracing::info!("Received SIGHUP"),
        }
    }
    #[cfg(not(unix))]
    {
        match ctrl_c.await {
            Ok(()) => tracing::info!("Received Ctrl+C"),
            Err(error) => tracing::warn!(%error, "Ctrl+C handling failed"),
        }
    }
}
