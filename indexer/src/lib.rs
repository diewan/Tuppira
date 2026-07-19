/// Multi-chain indexer daemon for the Tuppira.
///
/// Coordinates chain-specific indexers, manages sync progress,
/// and exposes metrics for monitoring.
pub mod aptos;
pub mod bitcoin;
pub mod chain_indexer;
pub mod connector;
pub mod ethereum;
pub mod indexer_plugin;
pub mod manifest;
pub mod metrics;
pub mod piteka_feed;
pub mod rpc_manager;
pub mod solana;
pub mod sui;
pub mod sync;
pub mod wallet_bridge;

pub use chain_indexer::{
    AddressIndexingResult, ChainIndexer, ChainResult, LegacyChainConnectorAdapter,
};
pub use connector::{
    ConnectorCursor, ConnectorError, ConnectorResult, ObservationCandidate, RawSourceBatch,
    RawSourceEvent, ReconciliationReport, SourceAuthentication, SourceConnector, SourceHealth,
    authenticate_and_normalize,
};
pub use indexer_plugin::{IndexerPluginRegistry, IndexerPluginRegistryBuilder};
pub use piteka_feed::{
    HttpPitekaFeedTransport, PitekaEvidenceFeedConnector, PitekaFeedConfig, PitekaFeedPage,
    PitekaFeedTransport, SignedPitekaExport,
};
pub use rpc_manager::{AuthType, RpcConfig, RpcEndpoint, RpcManager, RpcType, load_rpc_config};
pub use sync::SyncCoordinator;
pub use wallet_bridge::{WalletIndexerBridge, WalletIndexerBridgeConfig};

use sqlx::SqlitePool;
use tuppira_shared::{Result, TuppiraConfig};

/// The main indexer that wraps all chain indexers.
pub struct Indexer {
    config: TuppiraConfig,
    coordinator: SyncCoordinator,
    wallet_bridge: Option<WalletIndexerBridge>,
    rpc_manager: RpcManager,
    plugin_registry: IndexerPluginRegistry,
}

impl Indexer {
    /// Create a new indexer with the given configuration and database pool.
    /// Uses the plug-and-play indexer registry for dynamic chain support.
    pub async fn new(config: TuppiraConfig, pool: SqlitePool) -> Result<Self> {
        if config.chains.values().any(|chain| chain.enabled) {
            let manifest_path = std::env::var("DEPLOYMENT_MANIFEST_PATH").map_err(|_| {
                tuppira_shared::TuppiraError::Parse(
                    "DEPLOYMENT_MANIFEST_PATH is required when any chain is enabled".to_string(),
                )
            })?;
            manifest::validate_enabled_chains(
                std::path::Path::new(&manifest_path),
                &config.chains,
            )?;
        }
        // Load RPC configuration
        let rpc_manager = RpcManager::new(load_rpc_config()?);

        // Build plugin registry with all built-in chains
        let plugin_registry = IndexerPluginRegistryBuilder::new().build();

        // Create indexers using the plugin registry
        let indexers = plugin_registry.create_all_arc_indexers(&config.chains, rpc_manager.clone());

        tracing::info!(
            "Indexer initialized with {} chains from plugin registry",
            indexers.len()
        );

        let coordinator = SyncCoordinator::new(
            indexers,
            pool.clone(),
            config.chains.clone(),
            config.indexer.concurrency,
            config.indexer.batch_size,
            config.indexer.poll_interval_ms,
        );

        Ok(Self {
            config,
            coordinator,
            wallet_bridge: None,
            rpc_manager,
            plugin_registry,
        })
    }

    /// Create a new indexer with a custom plugin registry.
    /// This enables custom chain support beyond the built-in chains.
    pub async fn with_registry(
        config: TuppiraConfig,
        pool: SqlitePool,
        plugin_registry: IndexerPluginRegistry,
    ) -> Result<Self> {
        // Load RPC configuration
        let rpc_manager = RpcManager::new(load_rpc_config()?);

        // Create indexers using the provided plugin registry
        let indexers = plugin_registry.create_all_arc_indexers(&config.chains, rpc_manager.clone());

        tracing::info!(
            "Indexer initialized with {} chains from custom plugin registry",
            indexers.len()
        );

        let coordinator = SyncCoordinator::new(
            indexers,
            pool.clone(),
            config.chains.clone(),
            config.indexer.concurrency,
            config.indexer.batch_size,
            config.indexer.poll_interval_ms,
        );

        Ok(Self {
            config,
            coordinator,
            wallet_bridge: None,
            rpc_manager,
            plugin_registry,
        })
    }

    /// Initialize the wallet-indexer bridge with priority indexing.
    /// Uses the plugin registry for dynamic chain support.
    pub async fn with_wallet_bridge(mut self, config: WalletIndexerBridgeConfig) -> Result<Self> {
        // Create Arc-wrapped indexers using the plugin registry
        let indexers = self
            .plugin_registry
            .create_all_arc_indexers(&self.config.chains, self.rpc_manager.clone());

        tracing::info!(
            "Wallet bridge initialized with {} chains from plugin registry",
            indexers.len()
        );

        let bridge =
            WalletIndexerBridge::new(self.coordinator.get_pool().clone(), indexers, config);

        bridge.initialize().await?;

        self.wallet_bridge = Some(bridge);
        Ok(self)
    }

    /// Get the plugin registry for dynamic chain registration.
    pub fn plugin_registry(&self) -> &IndexerPluginRegistry {
        &self.plugin_registry
    }

    /// Check if a chain is supported by the indexer.
    pub fn is_chain_supported(&self, chain_id: &str) -> bool {
        self.plugin_registry.is_registered(chain_id)
    }

    /// Get all supported chain IDs.
    pub fn supported_chains(&self) -> Vec<&str> {
        self.plugin_registry.registered_chains()
    }

    /// Get a reference to the wallet bridge if available.
    pub fn wallet_bridge(&self) -> Option<&WalletIndexerBridge> {
        self.wallet_bridge.as_ref()
    }

    /// Initialize all chain indexers.
    pub async fn initialize(&self) -> Result<()> {
        self.coordinator.initialize(&self.config.chains).await?;
        Ok(())
    }

    /// Start the indexer daemon.
    pub async fn start(&self) -> Result<()> {
        // Start wallet bridge if available
        if let Some(bridge) = &self.wallet_bridge {
            let bridge_clone = bridge.clone();
            tokio::spawn(async move {
                if let Err(e) = bridge_clone.start().await {
                    tracing::error!(error = %e, "Wallet bridge error");
                }
            });
        }

        self.coordinator.start().await?;
        tracing::info!("Indexer daemon started");
        Ok(())
    }

    /// Stop the indexer daemon.
    pub async fn stop(&self) -> Result<()> {
        if let Some(bridge) = &self.wallet_bridge {
            bridge.stop().await?;
        }
        self.coordinator.stop().await?;
        tracing::info!("Indexer daemon stopped");
        Ok(())
    }

    /// Get the current indexer status.
    pub async fn status(&self) -> tuppira_shared::IndexerStatus {
        self.coordinator.status().await
    }

    /// Force sync a specific chain.
    pub async fn sync_chain(&self, chain: &str) -> Result<()> {
        self.coordinator.sync_chain(chain).await
    }

    /// Force sync a specific chain from a specific block (overrides config).
    pub async fn sync_chain_from_block(&self, chain: &str, from_block: u64) -> Result<()> {
        self.coordinator
            .sync_chain_from_block(chain, from_block)
            .await
    }

    /// Reindex a chain from a specific block.
    pub async fn reindex_from(&self, chain: &str, from_block: u64) -> Result<()> {
        self.coordinator.reindex_from(chain, from_block).await
    }

    /// Reset all sync progress.
    pub async fn reset_sync(&self) -> Result<()> {
        self.coordinator.reset_sync().await
    }
}

/// Build a boxed indexer using the plugin registry.
/// This function is kept for backward compatibility.
/// Consider using IndexerPluginRegistry directly for new code.
pub fn build_boxed_indexer(
    chain_id: &str,
    config: tuppira_shared::ChainConfig,
    rpc_manager: RpcManager,
) -> Option<Box<dyn ChainIndexer>> {
    let registry = IndexerPluginRegistryBuilder::new().build();
    registry.create_indexer(chain_id, config, rpc_manager)
}

/// Build an Arc-wrapped indexer using the plugin registry.
/// This function is kept for backward compatibility.
/// Consider using IndexerPluginRegistry directly for new code.
pub fn build_arc_indexer(
    chain_id: &str,
    config: tuppira_shared::ChainConfig,
    rpc_manager: RpcManager,
) -> Option<std::sync::Arc<dyn ChainIndexer>> {
    let registry = IndexerPluginRegistryBuilder::new().build();
    registry.create_arc_indexer(chain_id, config, rpc_manager)
}
