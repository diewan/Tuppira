/// Trait definition for chain-specific indexers.
///
/// Each chain implementation implements this trait to provide a uniform
/// interface for block scanning, event parsing, and data indexing.
use async_trait::async_trait;

use csv_explorer_shared::{
    CommitmentScheme, CsvContract, EnhancedSanadRecord, EnhancedSealRecord, EnhancedTransferRecord,
    ExplorerError, ExplorerEventDto, FinalityProofType, InclusionProofType, Network, PriorityLevel,
    SanadRecord, SealRecord, TransferRecord,
};

/// Result type alias for chain indexer operations.
pub type ChainResult<T> = std::result::Result<T, ExplorerError>;

/// Trait that each chain-specific indexer must implement.
///
/// The indexer daemon calls these methods to sync data from each chain.
#[async_trait]
pub trait ChainIndexer: Send + Sync {
    /// Unique identifier for this chain (e.g., "bitcoin", "ethereum").
    fn chain_id(&self) -> &str;

    /// Human-readable name for this chain.
    fn chain_name(&self) -> &str;

    /// Initialize the chain indexer (connect to RPC, validate config).
    async fn initialize(&self) -> ChainResult<()>;

    /// Get the current tip block number from the chain.
    async fn get_chain_tip(&self) -> ChainResult<u64>;

    /// Get the latest block number that has been fully synced.
    async fn get_latest_synced_block(&self) -> ChainResult<u64>;

    /// Index sanads found in a specific block.
    async fn index_sanads(&self, block: u64) -> ChainResult<Vec<SanadRecord>>;

    /// Index seals found in a specific block.
    async fn index_seals(&self, block: u64) -> ChainResult<Vec<SealRecord>>;

    /// Index transfers found in a specific block.
    async fn index_transfers(&self, block: u64) -> ChainResult<Vec<TransferRecord>>;

    /// Index contract deployments/events found in a specific block.
    async fn index_contracts(&self, block: u64) -> ChainResult<Vec<CsvContract>>;

    // -----------------------------------------------------------------------
    // Shared Schema Event Indexing (Production Guarantee Plan Phase 6)
    // -----------------------------------------------------------------------

    /// Index validated, versioned explorer DTOs from chain-specific logs/events.
    ///
    /// # Arguments
    /// * `block` - Block height to index
    ///
    /// # Returns
    async fn index_explorer_events(&self, block: u64) -> ChainResult<Vec<ExplorerEventDto>> {
        Err(ExplorerError::BlockError { chain: self.chain_id().to_string(), block, message: "indexer must decode validated raw chain events; record materialization is forbidden".to_string() })
    }

    /// Parse a single block and return the latest block height processed.
    /// This is a convenience method that calls all index_* methods.
    async fn process_block(&self, block: u64) -> ChainResult<BlockIndexResult> {
        let sanads = self.index_sanads(block).await?;
        let seals = self.index_seals(block).await?;
        let transfers = self.index_transfers(block).await?;
        let contracts = self.index_contracts(block).await?;

        Ok(BlockIndexResult {
            block,
            sanads_count: sanads.len() as u64,
            seals_count: seals.len() as u64,
            transfers_count: transfers.len() as u64,
            contracts_count: contracts.len() as u64,
        })
    }

    // -----------------------------------------------------------------------
    // Advanced commitment and proof indexing methods
    // -----------------------------------------------------------------------

    /// Index sanads with enhanced commitment metadata.
    async fn index_enhanced_sanads(&self, block: u64) -> ChainResult<Vec<EnhancedSanadRecord>>;

    /// Index seals with enhanced proof metadata.
    async fn index_enhanced_seals(&self, block: u64) -> ChainResult<Vec<EnhancedSealRecord>>;

    /// Index transfers with cross-chain proof metadata.
    async fn index_enhanced_transfers(
        &self,
        block: u64,
    ) -> ChainResult<Vec<EnhancedTransferRecord>>;

    // -----------------------------------------------------------------------
    // Priority address-based indexing methods
    // -----------------------------------------------------------------------

    /// Index all sanads related to a specific address.
    async fn index_sanads_by_address(&self, address: &str) -> ChainResult<Vec<SanadRecord>>;

    /// Index all seals related to a specific address.
    async fn index_seals_by_address(&self, address: &str) -> ChainResult<Vec<SealRecord>>;

    /// Index all transfers related to a specific address.
    async fn index_transfers_by_address(&self, address: &str) -> ChainResult<Vec<TransferRecord>>;

    /// Index all data (sanads, seals, transfers) for a list of addresses with priority.
    /// Returns the count of items indexed for each type.
    async fn index_addresses_with_priority(
        &self,
        addresses: &[String],
        priority: PriorityLevel,
        network: Network,
    ) -> ChainResult<AddressIndexingResult>;

    /// Detect commitment scheme from transaction/block data.
    /// Returns the detected scheme or None if unable to detect.
    fn detect_commitment_scheme(&self, data: &[u8]) -> Option<CommitmentScheme>;

    /// Detect inclusion proof type from transaction/block data.
    fn detect_inclusion_proof_type(&self) -> InclusionProofType;

    /// Detect finality proof type for this chain.
    fn detect_finality_proof_type(&self) -> FinalityProofType;
}

/// Result of processing a single block.
#[derive(Debug, Clone)]
pub struct BlockIndexResult {
    pub block: u64,
    pub sanads_count: u64,
    pub seals_count: u64,
    pub transfers_count: u64,
    pub contracts_count: u64,
}

/// Result of indexing addresses with priority.
#[derive(Debug, Clone)]
pub struct AddressIndexingResult {
    pub addresses_processed: u64,
    pub sanads_indexed: u64,
    pub seals_indexed: u64,
    pub transfers_indexed: u64,
    pub contracts_indexed: u64,
    pub errors: Vec<(String, String)>, // (address, error_message)
}
