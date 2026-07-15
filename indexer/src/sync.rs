#![allow(clippy::collapsible_if)] // EXP-003 replaces the legacy ingestion loop.

/// Sync coordination for multi-chain indexing.
///
/// Manages sync progress per chain, handles reorgs, and coordinates
/// concurrent chain syncing.
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rand::Rng;
use tokio::sync::RwLock;
use tokio::time::sleep;

use super::chain_indexer::{AddressIndexingResult, BlockIndexResult, ChainIndexer, ChainResult};
use csv_explorer_shared::{
    ChainConfig, ChainInfo, ChainStatus, CommitmentScheme, CsvContract, EnhancedSanadRecord,
    EnhancedSealRecord, EnhancedTransferRecord, ExplorerError, ExplorerEventDto, FinalityProofType,
    InclusionProofType, IndexerStatus, Network, PriorityLevel, SanadRecord, SealRecord,
    TransferRecord,
};

use csv_explorer_storage::repositories::{
    AdvancedProofRepository, ContractsRepository, SanadsRepository, SealsRepository,
    SyncRepository, TransfersRepository,
};
use sqlx::SqlitePool;

/// Sync coordinator that manages multiple chain indexers.
pub struct SyncCoordinator {
    indexers: Vec<Arc<dyn ChainIndexer>>,
    pool: SqlitePool,
    sync_repo: SyncRepository,
    sanads_repo: SanadsRepository,
    seals_repo: SealsRepository,
    transfers_repo: TransfersRepository,
    contracts_repo: ContractsRepository,
    advanced_repo: AdvancedProofRepository,
    /// Chain configurations for network info
    chain_configs: std::collections::HashMap<String, ChainConfig>,
    batch_size: u64,
    indexer_poll_interval_ms: u64,
    running: Arc<RwLock<bool>>,
    chain_states: Arc<RwLock<Vec<ChainSyncState>>>,
    /// Total indexed blocks counter
    total_indexed: Arc<RwLock<u64>>,
    /// Per-chain polling intervals (chain_id -> duration)
    chain_intervals: std::collections::HashMap<String, Duration>,
}

/// Default polling intervals per chain (base values before jitter).
fn default_chain_intervals() -> std::collections::HashMap<String, Duration> {
    [
        ("solana", Duration::from_millis(1000)),
        ("sui", Duration::from_millis(4000)),
        ("aptos", Duration::from_millis(4000)),
        ("ethereum", Duration::from_millis(12000)),
        ("bitcoin", Duration::from_millis(15000)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Per-chain sync state.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ChainSyncState {
    chain_id: String,
    chain_name: String,
    status: ChainStatus,
    latest_block: u64,
    latest_slot: Option<u64>,
    rpc_url: String,
    network: String,
}

#[derive(Clone)]
struct SyncedChainIndexer {
    inner: Arc<dyn ChainIndexer>,
    sync_repo: SyncRepository,
}

#[async_trait]
impl ChainIndexer for SyncedChainIndexer {
    fn chain_id(&self) -> &str {
        self.inner.chain_id()
    }

    fn chain_name(&self) -> &str {
        self.inner.chain_name()
    }

    async fn initialize(&self) -> ChainResult<()> {
        self.inner.initialize().await
    }

    async fn get_chain_tip(&self) -> ChainResult<u64> {
        self.inner.get_chain_tip().await
    }

    async fn get_latest_synced_block(&self) -> ChainResult<u64> {
        Ok(self
            .sync_repo
            .get_latest_block(self.chain_id())
            .await?
            .unwrap_or(0))
    }

    async fn index_sanads(&self, block: u64) -> ChainResult<Vec<SanadRecord>> {
        self.inner.index_sanads(block).await
    }

    async fn index_seals(&self, block: u64) -> ChainResult<Vec<SealRecord>> {
        self.inner.index_seals(block).await
    }

    async fn index_transfers(&self, block: u64) -> ChainResult<Vec<TransferRecord>> {
        self.inner.index_transfers(block).await
    }

    async fn index_contracts(&self, block: u64) -> ChainResult<Vec<CsvContract>> {
        self.inner.index_contracts(block).await
    }

    async fn index_explorer_events(&self, block: u64) -> ChainResult<Vec<ExplorerEventDto>> {
        self.inner.index_explorer_events(block).await
    }

    async fn process_block(&self, block: u64) -> ChainResult<BlockIndexResult> {
        self.inner.process_block(block).await
    }

    async fn index_enhanced_sanads(&self, block: u64) -> ChainResult<Vec<EnhancedSanadRecord>> {
        self.inner.index_enhanced_sanads(block).await
    }

    async fn index_enhanced_seals(&self, block: u64) -> ChainResult<Vec<EnhancedSealRecord>> {
        self.inner.index_enhanced_seals(block).await
    }

    async fn index_enhanced_transfers(
        &self,
        block: u64,
    ) -> ChainResult<Vec<EnhancedTransferRecord>> {
        self.inner.index_enhanced_transfers(block).await
    }

    async fn index_sanads_by_address(&self, address: &str) -> ChainResult<Vec<SanadRecord>> {
        self.inner.index_sanads_by_address(address).await
    }

    async fn index_seals_by_address(&self, address: &str) -> ChainResult<Vec<SealRecord>> {
        self.inner.index_seals_by_address(address).await
    }

    async fn index_transfers_by_address(&self, address: &str) -> ChainResult<Vec<TransferRecord>> {
        self.inner.index_transfers_by_address(address).await
    }

    async fn index_addresses_with_priority(
        &self,
        addresses: &[String],
        priority: PriorityLevel,
        network: Network,
    ) -> ChainResult<AddressIndexingResult> {
        self.inner
            .index_addresses_with_priority(addresses, priority, network)
            .await
    }

    fn detect_commitment_scheme(&self, data: &[u8]) -> Option<CommitmentScheme> {
        self.inner.detect_commitment_scheme(data)
    }

    fn detect_inclusion_proof_type(&self) -> InclusionProofType {
        self.inner.detect_inclusion_proof_type()
    }

    fn detect_finality_proof_type(&self) -> FinalityProofType {
        self.inner.detect_finality_proof_type()
    }
}

impl SyncCoordinator {
    /// Create a new sync coordinator.
    pub fn new(
        indexers: Vec<Arc<dyn ChainIndexer>>,
        pool: SqlitePool,
        chain_configs: std::collections::HashMap<String, ChainConfig>,
        _concurrency: usize,
        batch_size: u64,
        poll_interval_ms: u64,
    ) -> Self {
        let chain_states = indexers
            .iter()
            .map(|idx| {
                let chain_id = idx.chain_id().to_string();
                let network = chain_configs
                    .get(&chain_id)
                    .map(|c| format!("{:?}", c.network))
                    .unwrap_or_else(|| "mainnet".to_string());
                let rpc_url = chain_configs
                    .get(&chain_id)
                    .map(|c| c.rpc_url.clone())
                    .unwrap_or_default();
                ChainSyncState {
                    chain_id,
                    chain_name: idx.chain_name().to_string(),
                    status: ChainStatus::Stopped,
                    latest_block: 0,
                    latest_slot: None,
                    rpc_url,
                    network,
                }
            })
            .collect();

        let sync_repo = SyncRepository::new(pool.clone());
        let indexers = indexers
            .into_iter()
            .map(|inner| {
                Arc::new(SyncedChainIndexer {
                    inner,
                    sync_repo: sync_repo.clone(),
                }) as Arc<dyn ChainIndexer>
            })
            .collect();

        Self {
            indexers,
            pool: pool.clone(),
            sync_repo,
            sanads_repo: SanadsRepository::new(pool.clone()),
            seals_repo: SealsRepository::new(pool.clone()),
            transfers_repo: TransfersRepository::new(pool.clone()),
            contracts_repo: ContractsRepository::new(pool.clone()),
            advanced_repo: AdvancedProofRepository::new(pool),
            chain_configs,
            batch_size,
            indexer_poll_interval_ms: poll_interval_ms,
            running: Arc::new(RwLock::new(false)),
            chain_states: Arc::new(RwLock::new(chain_states)),
            total_indexed: Arc::new(RwLock::new(0)),
            chain_intervals: default_chain_intervals(),
        }
    }

    /// Get a reference to the database pool.
    pub fn get_pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get the polling interval for a specific chain with jitter.
    ///
    /// Applies ±20% random jitter to prevent thundering herd on RPC endpoints.
    fn get_chain_interval(&self, chain_id: &str) -> Duration {
        let base = self
            .chain_intervals
            .get(chain_id)
            .copied()
            .unwrap_or(Duration::from_millis(self.indexer_poll_interval_ms));

        self.apply_jitter(base)
    }

    /// Apply ±20% jitter to a duration using a thread-local RNG.
    fn apply_jitter(&self, duration: Duration) -> Duration {
        let mut rng = rand::thread_rng();
        // Jitter factor: 0.8 to 1.2 (±20%)
        let jitter_factor = rng.gen_range(0.8..=1.2);
        Duration::from_millis((duration.as_millis() as f64 * jitter_factor) as u64)
    }

    /// Initialize all chain indexers.
    pub async fn initialize(
        &self,
        chain_configs: &std::collections::HashMap<String, ChainConfig>,
    ) -> Result<(), ExplorerError> {
        for indexer in &self.indexers {
            if let Some(config) = chain_configs.get(indexer.chain_id()) {
                if config.enabled {
                    indexer.initialize().await?;
                    tracing::info!(chain = indexer.chain_id(), "Chain indexer initialized");
                }
            }
        }
        Ok(())
    }

    /// Start the sync loop for all enabled chains.
    pub async fn start(&self) -> Result<(), ExplorerError> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }
        *running = true;
        drop(running);

        tracing::info!("Starting sync coordinator with per-chain polling");

        // Spawn a dedicated sync task for each chain with its own interval
        let indexers = self.indexers.clone();
        let chain_configs = &self.chain_configs;
        let batch_size = self.batch_size;
        let total_indexed = &self.total_indexed;

        for indexer in indexers {
            let chain_id = indexer.chain_id().to_string();
            let chain_config = chain_configs.get(chain_id.as_str());

            // Only spawn if the chain is enabled
            if let Some(config) = chain_config {
                if !config.enabled {
                    continue;
                }
            }

            let poll_interval = self.get_chain_interval(&chain_id);
            tracing::info!(chain = %chain_id, interval_ms = poll_interval.as_millis(), "Starting chain sync task");

            // Clone shared state for the spawned task
            let running = Arc::clone(&self.running);
            let chain_states = Arc::clone(&self.chain_states);

            let sync_ctx = SyncContext::new(
                self.sync_repo.clone(),
                self.sanads_repo.clone(),
                self.seals_repo.clone(),
                self.transfers_repo.clone(),
                self.contracts_repo.clone(),
                self.advanced_repo.clone(),
                batch_size,
                Arc::clone(total_indexed),
                chain_config.cloned(),
            );

            let indexer_clone = Arc::clone(&indexer);

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(poll_interval);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                while *running.read().await {
                    interval.tick().await;

                    if let Err(e) = sync_chain(indexer_clone.as_ref(), &sync_ctx).await {
                        tracing::warn!(chain = %chain_id, error = %e, "Per-chain sync error");
                    }

                    // Update chain state after sync attempt
                    if let Ok(tip) = indexer_clone.get_chain_tip().await {
                        let mut states = chain_states.write().await;
                        for state in states.iter_mut() {
                            if state.chain_id == chain_id {
                                state.latest_block = tip;
                                state.status = ChainStatus::Synced;
                                break;
                            }
                        }
                    }
                }

                tracing::info!(chain = %chain_id, "Chain sync task stopped");
            });
        }

        // Main loop: keep the coordinator running and update status periodically
        while *self.running.read().await {
            sleep(Duration::from_secs(5)).await;
        }

        tracing::info!("Sync coordinator stopped");

        Ok(())
    }

    /// Stop the sync loop.
    pub async fn stop(&self) -> Result<(), ExplorerError> {
        let mut running = self.running.write().await;
        *running = false;
        tracing::info!("Stopping sync coordinator");
        Ok(())
    }

    /// Get the current status of all chains.
    pub async fn status(&self) -> IndexerStatus {
        let states = self.chain_states.read().await;
        let chains = states
            .iter()
            .map(|state| {
                // Get network from chain config instead of hardcoding Mainnet
                let network = self
                    .chain_configs
                    .get(&state.chain_id)
                    .map(|c| c.network)
                    .unwrap_or(csv_explorer_shared::Network::Mainnet);

                let sync_lag = 0u64; // Calculated from chain tip - latest_block during active sync

                ChainInfo {
                    id: state.chain_id.clone(),
                    name: state.chain_name.clone(),
                    network,
                    status: state.status,
                    latest_block: state.latest_block,
                    latest_slot: state.latest_slot,
                    rpc_url: state.rpc_url.clone(),
                    sync_lag,
                }
            })
            .collect();

        let total_indexed = *self.total_indexed.read().await;

        IndexerStatus {
            chains,
            total_indexed_blocks: total_indexed,
            is_running: *self.running.read().await,
            started_at: None,
            uptime_seconds: None,
        }
    }

    /// Force sync a specific chain.
    pub async fn sync_chain(&self, chain_id: &str) -> Result<(), ExplorerError> {
        self.sync_chain_from(chain_id, None).await
    }

    /// Force sync a specific chain from a specific block (overrides config).
    pub async fn sync_chain_from_block(
        &self,
        chain_id: &str,
        from_block: u64,
    ) -> Result<(), ExplorerError> {
        self.sync_chain_from(chain_id, Some(from_block)).await
    }

    /// Internal: sync a specific chain with optional override start block.
    async fn sync_chain_from(
        &self,
        chain_id: &str,
        override_start_block: Option<u64>,
    ) -> Result<(), ExplorerError> {
        let indexer = self
            .indexers
            .iter()
            .find(|idx| idx.chain_id() == chain_id)
            .ok_or_else(|| ExplorerError::Internal(format!("Chain {} not found", chain_id)))?;

        let chain_config = self.chain_configs.get(chain_id);

        // If override_start_block is provided, use it instead of config
        let effective_config = if override_start_block.is_some() {
            // Create a temporary config with the override start_block
            chain_config.cloned().map(|mut c| {
                c.start_block = override_start_block;
                c
            })
        } else {
            chain_config.cloned()
        };

        let ctx = SyncContext::new(
            self.sync_repo.clone(),
            self.sanads_repo.clone(),
            self.seals_repo.clone(),
            self.transfers_repo.clone(),
            self.contracts_repo.clone(),
            self.advanced_repo.clone(),
            self.batch_size,
            Arc::clone(&self.total_indexed),
            effective_config,
        );
        sync_chain(indexer.as_ref(), &ctx).await
    }

    /// Reindex a chain from a specific block.
    pub async fn reindex_from(&self, chain_id: &str, from_block: u64) -> Result<(), ExplorerError> {
        // Reset sync progress for this chain
        self.sync_repo.reset(chain_id).await?;

        // Then sync from the specified block (pass the from_block override)
        self.sync_chain_from_block(chain_id, from_block).await
    }

    /// Reset sync progress for all chains.
    pub async fn reset_sync(&self) -> Result<(), ExplorerError> {
        self.sync_repo.reset_all().await?;
        Ok(())
    }
}

/// Context for sync operations in the indexer
pub struct SyncContext {
    /// Repository for sync progress
    pub sync_repo: SyncRepository,
    /// Repository for sanads
    pub sanads_repo: SanadsRepository,
    /// Repository for seals
    pub seals_repo: SealsRepository,
    /// Repository for transfers
    pub transfers_repo: TransfersRepository,
    /// Repository for contracts
    pub contracts_repo: ContractsRepository,
    /// Repository for advanced proofs
    pub advanced_repo: AdvancedProofRepository,
    /// Batch size for processing
    pub batch_size: u64,
    /// Total indexed counter
    pub total_indexed: Arc<RwLock<u64>>,
    /// Chain configuration
    pub chain_config: Option<ChainConfig>,
}

impl SyncContext {
    /// Create a new sync context
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sync_repo: SyncRepository,
        sanads_repo: SanadsRepository,
        seals_repo: SealsRepository,
        transfers_repo: TransfersRepository,
        contracts_repo: ContractsRepository,
        advanced_repo: AdvancedProofRepository,
        batch_size: u64,
        total_indexed: Arc<RwLock<u64>>,
        chain_config: Option<ChainConfig>,
    ) -> Self {
        Self {
            sync_repo,
            sanads_repo,
            seals_repo,
            transfers_repo,
            contracts_repo,
            advanced_repo,
            batch_size,
            total_indexed,
            chain_config,
        }
    }
}

/// Sync a single chain from its last synced position.
async fn sync_chain(indexer: &dyn ChainIndexer, ctx: &SyncContext) -> Result<(), ExplorerError> {
    let chain_id = indexer.chain_id();

    // Get last synced block from database
    let db_block = ctx.sync_repo.get_latest_block(chain_id).await?;

    // Determine starting block - DATABASE TAKES PRIORITY:
    // 1. If database has data, always use the last synced block (resumes from where we left off)
    // 2. Otherwise, use start_block from chain config if provided (initial sync)
    // 3. Fall back to 0 (genesis) if neither is available
    let from_block = if let Some(block) = db_block {
        tracing::info!(chain = chain_id, block, "Resuming sync from database");
        block
    } else if let Some(config) = &ctx.chain_config {
        if let Some(start) = config.start_block {
            tracing::info!(
                chain = chain_id,
                start_block = start,
                "Starting initial sync from configured start_block"
            );
            start
        } else {
            tracing::warn!(
                chain = chain_id,
                "No start_block configured, syncing from genesis (block 0)"
            );
            0
        }
    } else {
        tracing::warn!(
            chain = chain_id,
            "No chain config provided, syncing from genesis (block 0)"
        );
        0
    };

    // Get chain tip
    let tip = match indexer.get_chain_tip().await {
        Ok(tip) => tip,
        Err(e) => {
            tracing::warn!(chain = chain_id, error = %e, "Failed to get chain tip");
            return Err(e);
        }
    };

    if from_block >= tip {
        return Ok(()); // Already caught up
    }

    // Process blocks in batches
    let mut current = from_block + 1;
    let end = std::cmp::min(current + ctx.batch_size - 1, tip);

    tracing::debug!(chain = chain_id, from = current, to = end, "Syncing chain");

    while current <= end {
        // Canonical event decoding is the only ingestion boundary. In
        // particular, do not fall back to the legacy record builders: they
        // manufacture ownership, timestamps, and deployment state.
        let events = indexer.index_explorer_events(current).await?;
        for event in &events {
            event
                .validate()
                .map_err(|message| ExplorerError::BlockError {
                    chain: chain_id.to_string(),
                    block: current,
                    message,
                })?;
        }

        // EXP-004 owns durable, atomic materialization. Until that boundary
        // exists, successfully decoded events must not be projected into the
        // legacy record tables, which have no provenance/finality columns.
        tracing::debug!(
            chain = chain_id,
            block = current,
            event_count = events.len(),
            "Validated canonical chain events"
        );

        // Update sync progress
        ctx.sync_repo
            .update_progress(chain_id, current, None)
            .await?;

        // Increment total indexed blocks counter
        {
            let mut total = ctx.total_indexed.write().await;
            *total += 1;
        }

        current += 1;
    }

    tracing::info!(chain = chain_id, block = end, "Synced to block");
    Ok(())
}
