//! Wallet-Indexer bridge service.
//!
//! This service manages the connection between the wallet and the indexer,
//! allowing wallets to register addresses for priority indexing and query
//! indexed data related to those addresses.

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tokio::time::sleep;

use csv_explorer_shared::{
    ExplorerError, Network, PriorityAddress, PriorityIndexingStatus, PriorityLevel, Result,
    SanadRecord, SealRecord, TransferRecord,
};

use crate::chain_indexer::ChainIndexer;
use csv_explorer_storage::repositories::{IndexingActivityRequest, PriorityAddressRepository};

/// Configuration for the wallet-indexer bridge.
#[derive(Debug, Clone)]
pub struct WalletIndexerBridgeConfig {
    /// How often to re-index high priority addresses (in milliseconds).
    pub high_priority_interval_ms: u64,
    /// How often to re-index normal priority addresses (in milliseconds).
    pub normal_priority_interval_ms: u64,
    /// How often to re-index low priority addresses (in milliseconds).
    pub low_priority_interval_ms: u64,
    /// Maximum number of addresses to index in one batch.
    pub max_batch_size: usize,
}

impl Default for WalletIndexerBridgeConfig {
    fn default() -> Self {
        Self {
            high_priority_interval_ms: 10_000,   // 10 seconds
            normal_priority_interval_ms: 60_000, // 1 minute
            low_priority_interval_ms: 300_000,   // 5 minutes
            max_batch_size: 50,
        }
    }
}

/// Bridge service connecting wallet to indexer.
#[derive(Clone)]
pub struct WalletIndexerBridge {
    /// Database pool (retained for future use)
    _pool: SqlitePool,
    priority_repo: Arc<PriorityAddressRepository>,
    indexers: Vec<Arc<dyn ChainIndexer>>,
    config: Arc<WalletIndexerBridgeConfig>,
    running: Arc<RwLock<bool>>,
}

impl WalletIndexerBridge {
    /// Create a new wallet-indexer bridge.
    pub fn new(
        pool: SqlitePool,
        indexers: Vec<Arc<dyn ChainIndexer>>,
        config: WalletIndexerBridgeConfig,
    ) -> Self {
        let priority_repo = PriorityAddressRepository::new(pool.clone());

        Self {
            _pool: pool,
            priority_repo: Arc::new(priority_repo),
            indexers,
            config: Arc::new(config),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Initialize the bridge (create tables, etc.).
    pub async fn initialize(&self) -> Result<()> {
        self.priority_repo.init().await.map_err(|e| {
            ExplorerError::Internal(format!(
                "Failed to initialize priority address tables: {}",
                e
            ))
        })?;
        tracing::info!("Wallet-indexer bridge initialized");
        Ok(())
    }

    /// Register an address for priority indexing.
    pub async fn register_address(
        &self,
        address: String,
        chain: String,
        network: Network,
        priority: PriorityLevel,
        wallet_id: String,
    ) -> Result<()> {
        self.priority_repo
            .register_address(&address, &chain, network, priority, &wallet_id)
            .await
            .map_err(|e| ExplorerError::Internal(format!("Failed to register address: {}", e)))?;

        tracing::info!(
            address = %address,
            chain = %chain,
            network = ?network,
            priority = ?priority,
            wallet_id = %wallet_id,
            "Address registered for priority indexing"
        );

        Ok(())
    }

    /// Unregister an address from priority indexing.
    pub async fn unregister_address(
        &self,
        address: String,
        chain: String,
        network: Network,
        wallet_id: String,
    ) -> Result<bool> {
        let removed = self
            .priority_repo
            .unregister_address(&address, &chain, network, &wallet_id)
            .await
            .map_err(|e| ExplorerError::Internal(format!("Failed to unregister address: {}", e)))?;

        if removed {
            tracing::info!(
                address = %address,
                chain = %chain,
                wallet_id = %wallet_id,
                "Address unregistered from priority indexing"
            );
        }

        Ok(removed)
    }

    /// Get all registered addresses for a wallet.
    pub async fn get_wallet_addresses(&self, wallet_id: &str) -> Result<Vec<PriorityAddress>> {
        self.priority_repo
            .get_addresses_by_wallet(wallet_id)
            .await
            .map_err(|e| ExplorerError::Internal(format!("Failed to get wallet addresses: {}", e)))
    }

    /// Get indexed sanads for a specific address across all chains.
    pub async fn get_sanads_by_address(&self, address: &str) -> Result<Vec<SanadRecord>> {
        let mut all_sanads = Vec::new();

        for indexer in &self.indexers {
            match indexer.index_sanads_by_address(address).await {
                Ok(sanads) => all_sanads.extend(sanads),
                Err(e) => {
                    tracing::warn!(
                        chain = %indexer.chain_id(),
                        address = %address,
                        error = %e,
                        "Failed to index sanads for address"
                    );
                }
            }
        }

        Ok(all_sanads)
    }

    /// Get indexed seals for a specific address across all chains.
    pub async fn get_seals_by_address(&self, address: &str) -> Result<Vec<SealRecord>> {
        let mut all_seals = Vec::new();

        for indexer in &self.indexers {
            match indexer.index_seals_by_address(address).await {
                Ok(seals) => all_seals.extend(seals),
                Err(e) => {
                    tracing::warn!(
                        chain = %indexer.chain_id(),
                        address = %address,
                        error = %e,
                        "Failed to index seals for address"
                    );
                }
            }
        }

        Ok(all_seals)
    }

    /// Get indexed transfers for a specific address across all chains.
    pub async fn get_transfers_by_address(&self, address: &str) -> Result<Vec<TransferRecord>> {
        let mut all_transfers = Vec::new();

        for indexer in &self.indexers {
            match indexer.index_transfers_by_address(address).await {
                Ok(transfers) => all_transfers.extend(transfers),
                Err(e) => {
                    tracing::warn!(
                        chain = %indexer.chain_id(),
                        address = %address,
                        error = %e,
                        "Failed to index transfers for address"
                    );
                }
            }
        }

        Ok(all_transfers)
    }

    /// Get complete data (sanads, seals, transfers) for an address.
    pub async fn get_address_data(&self, address: &str) -> Result<AddressDataResult> {
        let sanads = self.get_sanads_by_address(address).await?;
        let seals = self.get_seals_by_address(address).await?;
        let transfers = self.get_transfers_by_address(address).await?;

        Ok(AddressDataResult {
            address: address.to_string(),
            sanads,
            seals,
            transfers,
        })
    }

    /// Start the priority indexing loop.
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        tracing::info!("Starting wallet-indexer bridge priority indexing loop");

        // Run the priority indexing loop
        while *self.running.read().await {
            if let Err(e) = self.run_priority_indexing_cycle().await {
                tracing::error!(error = %e, "Error in priority indexing cycle");
            }

            // Sleep for a short duration before next cycle
            sleep(Duration::from_millis(1000)).await;
        }

        tracing::info!("Wallet-indexer bridge priority indexing loop stopped");
        Ok(())
    }

    /// Stop the priority indexing loop.
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.running.write().await;
        *running = false;
        tracing::info!("Stopping wallet-indexer bridge priority indexing loop");
        Ok(())
    }

    /// Run one cycle of priority indexing.
    async fn run_priority_indexing_cycle(&self) -> Result<()> {
        let all_addresses = self
            .priority_repo
            .get_all_active_addresses()
            .await
            .map_err(|e| {
                ExplorerError::Internal(format!("Failed to get active addresses: {}", e))
            })?;

        if all_addresses.is_empty() {
            return Ok(());
        }

        // Group addresses by priority level
        let mut high_priority: Vec<PriorityAddress> = Vec::new();
        let mut normal_priority: Vec<PriorityAddress> = Vec::new();
        let mut low_priority: Vec<PriorityAddress> = Vec::new();

        for addr in all_addresses {
            match addr.priority {
                PriorityLevel::High => high_priority.push(addr),
                PriorityLevel::Normal => normal_priority.push(addr),
                PriorityLevel::Low => low_priority.push(addr),
            }
        }

        // Index based on priority level
        self.index_addresses_by_priority_level(&high_priority, PriorityLevel::High)
            .await?;
        self.index_addresses_by_priority_level(&normal_priority, PriorityLevel::Normal)
            .await?;
        self.index_addresses_by_priority_level(&low_priority, PriorityLevel::Low)
            .await?;

        Ok(())
    }

    /// Index addresses for a specific priority level.
    async fn index_addresses_by_priority_level(
        &self,
        addresses: &[PriorityAddress],
        priority: PriorityLevel,
    ) -> Result<()> {
        if addresses.is_empty() {
            return Ok(());
        }

        // Group addresses by chain
        let mut addresses_by_chain: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut network_map: std::collections::HashMap<String, Network> =
            std::collections::HashMap::new();

        for addr in addresses {
            addresses_by_chain
                .entry(addr.chain.clone())
                .or_default()
                .push(addr.address.clone());
            network_map.insert(addr.chain.clone(), addr.network);
        }

        // Index for each chain
        for (chain_id, chain_addresses) in addresses_by_chain {
            let network = network_map
                .get(&chain_id)
                .copied()
                .unwrap_or(Network::Mainnet);

            // Find the indexer for this chain
            let indexer = self.indexers.iter().find(|idx| idx.chain_id() == chain_id);

            if let Some(indexer) = indexer {
                // Limit batch size
                let batch: Vec<String> = chain_addresses
                    .into_iter()
                    .take(self.config.max_batch_size)
                    .collect();

                match indexer
                    .index_addresses_with_priority(&batch, priority, network)
                    .await
                {
                    Ok(result) => {
                        // Record indexing activities
                        for addr in &batch {
                            self.priority_repo
                                .record_indexing_activity(
                                    IndexingActivityRequest::new(
                                        addr,
                                        &chain_id,
                                        network,
                                        "sanads",
                                        result.sanads_indexed,
                                        result.errors.is_empty(),
                                    )
                                    .with_error(
                                        result
                                            .errors
                                            .first()
                                            .map(|(_, e)| e.as_str())
                                            .unwrap_or(""),
                                    ),
                                )
                                .await
                                .ok();

                            self.priority_repo
                                .record_indexing_activity(
                                    IndexingActivityRequest::new(
                                        addr,
                                        &chain_id,
                                        network,
                                        "seals",
                                        result.seals_indexed,
                                        result.errors.is_empty(),
                                    )
                                    .with_error(
                                        result
                                            .errors
                                            .first()
                                            .map(|(_, e)| e.as_str())
                                            .unwrap_or(""),
                                    ),
                                )
                                .await
                                .ok();

                            self.priority_repo
                                .record_indexing_activity(
                                    IndexingActivityRequest::new(
                                        addr,
                                        &chain_id,
                                        network,
                                        "transfers",
                                        result.transfers_indexed,
                                        result.errors.is_empty(),
                                    )
                                    .with_error(
                                        result
                                            .errors
                                            .first()
                                            .map(|(_, e)| e.as_str())
                                            .unwrap_or(""),
                                    ),
                                )
                                .await
                                .ok();

                            // Update last indexed timestamp
                            self.priority_repo
                                .update_last_indexed_at(addr, &chain_id, network)
                                .await
                                .ok();
                        }

                        tracing::info!(
                            chain = %chain_id,
                            priority = ?priority,
                            addresses = batch.len(),
                            sanads = result.sanads_indexed,
                            seals = result.seals_indexed,
                            transfers = result.transfers_indexed,
                            "Priority address indexing completed"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            chain = %chain_id,
                            priority = ?priority,
                            error = %e,
                            "Failed to index priority addresses"
                        );

                        // Record errors
                        for addr in &batch {
                            self.priority_repo
                                .record_indexing_activity(
                                    IndexingActivityRequest::new(
                                        addr, &chain_id, network, "error", 0, false,
                                    )
                                    .with_error(&e.to_string()),
                                )
                                .await
                                .ok();
                        }
                    }
                }
            } else {
                tracing::warn!(chain = %chain_id, "No indexer found for chain");
            }
        }

        Ok(())
    }

    /// Get the current priority indexing status.
    pub async fn get_priority_indexing_status(&self) -> Result<PriorityIndexingStatus> {
        self.priority_repo
            .get_priority_indexing_status()
            .await
            .map_err(|e| {
                ExplorerError::Internal(format!("Failed to get priority indexing status: {}", e))
            })
    }
}

/// Result of querying complete address data.
#[derive(Debug, Clone)]
pub struct AddressDataResult {
    /// The address this data belongs to.
    pub address: String,
    /// All sanads associated with this address.
    pub sanads: Vec<SanadRecord>,
    /// All seals associated with this address.
    pub seals: Vec<SealRecord>,
    /// All transfers associated with this address.
    pub transfers: Vec<TransferRecord>,
}
