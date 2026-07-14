/// Aptos chain indexer implementation.
///
/// Subscribes to Aptos ledger state updates and tracks:
/// - Resource creation/destruction for seals
/// - Move events from CSV modules
/// - Transaction finality
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use super::chain_indexer::{AddressIndexingResult, ChainIndexer, ChainResult};
use super::rpc_manager::RpcManager;
use csv_explorer_shared::{
    ChainConfig, CommitmentScheme, ContractStatus, ContractType, CsvContract, EnhancedSanadRecord,
    EnhancedSealRecord, EnhancedTransferRecord, FinalityProofType, InclusionProofType, SanadRecord,
    SealRecord, SealStatus, SealType, TransferRecord,
};

/// Aptos-specific indexer.
pub struct AptosIndexer {
    config: ChainConfig,
    /// HTTP client for making API requests (retained for future use)
    _http_client: Client,
    /// RPC manager for handling multiple RPC endpoints
    rpc_manager: Option<RpcManager>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LedgerInfo {
    block_height: String,
    ledger_version: String,
    ledger_timestamp: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BlockTransactions {
    transactions: Vec<AptosTxn>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AptosTxn {
    hash: String,
    version: String,
    events: Option<Vec<AptosEvent>>,
    timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AptosEvent {
    guid: Option<serde_json::Value>,
    sequence_number: String,
    type_: String,
    data: serde_json::Value,
}

#[async_trait]
impl ChainIndexer for AptosIndexer {
    fn chain_id(&self) -> &str {
        "aptos"
    }

    fn chain_name(&self) -> &str {
        "Aptos"
    }

    async fn initialize(&self) -> ChainResult<()> {
        tracing::info!(chain = "aptos", "Aptos indexer initialized");
        Ok(())
    }

    async fn get_chain_tip(&self) -> ChainResult<u64> {
        let rpc_url = if let Some(ref manager) = self.rpc_manager {
            if let Some(endpoint) = manager.get_endpoint("aptos") {
                endpoint.url.clone()
            } else {
                self.config.rpc_url.clone()
            }
        } else {
            self.config.rpc_url.clone()
        };

        let client = if let Some(ref manager) = self.rpc_manager {
            manager.get_client("aptos")
        } else {
            Some(Client::new())
        };

        let url = format!("{}/v1", rpc_url.trim_end_matches("/v1"));
        let info: LedgerInfo = match client {
            Some(ref client) => client.get(&url).send().await?.json().await?,
            None => {
                let client = Client::new();
                client.get(&url).send().await?.json().await?
            }
        };
        Ok(info.ledger_version.parse::<u64>().unwrap_or(0))
    }

    async fn get_latest_synced_block(&self) -> ChainResult<u64> {
        Ok(0)
    }

    async fn index_sanads(&self, block: u64) -> ChainResult<Vec<SanadRecord>> {
        let txns = self.fetch_block_transactions(block).await?;
        let mut sanads = Vec::new();

        for txn in &txns {
            if let Some(events) = &txn.events {
                for event in events {
                    // Match AnchorEvent from CSV modules
                    // Pattern: {module_address}::csv_seal::AnchorEvent
                    if event.type_.contains("csv_seal") && event.type_.contains("AnchorEvent") {
                        if let Some(sanad) = self.parse_sanad_from_event(event, &txn.hash) {
                            sanads.push(sanad);
                        }
                    }
                }
            }
        }

        tracing::debug!(
            chain = "aptos",
            block,
            count = sanads.len(),
            "Indexed sanads"
        );
        Ok(sanads)
    }

    async fn index_seals(&self, block: u64) -> ChainResult<Vec<SealRecord>> {
        let txns = self.fetch_block_transactions(block).await?;
        let mut seals = Vec::new();

        for txn in &txns {
            if let Some(events) = &txn.events {
                for event in events {
                    // Match seal-related events from CSV modules
                    if event.type_.contains("csv_seal") || event.type_.contains("Seal") {
                        if let Some(seal) = self.parse_seal_from_event(event, block) {
                            seals.push(seal);
                        }
                    }
                }
            }
        }

        tracing::debug!(chain = "aptos", block, count = seals.len(), "Indexed seals");
        Ok(seals)
    }

    async fn index_transfers(&self, block: u64) -> ChainResult<Vec<TransferRecord>> {
        let txns = self.fetch_block_transactions(block).await?;
        let mut transfers = Vec::new();

        for txn in &txns {
            if let Some(events) = &txn.events {
                for event in events {
                    // Match cross-chain transfer events
                    if event.type_.contains("CrossChain") || event.type_.contains("bridge_transfer")
                    {
                        if let Some(transfer) = self.parse_transfer_from_event(event, &txn.hash) {
                            transfers.push(transfer);
                        }
                    }
                }
            }
        }

        tracing::debug!(
            chain = "aptos",
            block,
            count = transfers.len(),
            "Indexed transfers"
        );
        Ok(transfers)
    }

    async fn index_contracts(&self, _block: u64) -> ChainResult<Vec<CsvContract>> {
        Ok(vec![CsvContract {
            id: "aptos-csv-module".to_string(),
            chain: "aptos".to_string(),
            contract_type: ContractType::NullifierRegistry,
            address: "0x0000000000000000000000000000000000000000000000000000000000000001"
                .to_string(),
            deployed_tx: "genesis".to_string(),
            deployed_at: chrono::Utc::now(),
            version: "1.0.0".to_string(),
            status: ContractStatus::Active,
        }])
    }

    // -----------------------------------------------------------------------
    // Address-based indexing methods (for priority indexing)
    // -----------------------------------------------------------------------

    async fn index_sanads_by_address(&self, _address: &str) -> ChainResult<Vec<SanadRecord>> {
        Ok(Vec::new())
    }

    async fn index_seals_by_address(&self, _address: &str) -> ChainResult<Vec<SealRecord>> {
        Ok(Vec::new())
    }

    async fn index_transfers_by_address(&self, _address: &str) -> ChainResult<Vec<TransferRecord>> {
        Ok(Vec::new())
    }

    async fn index_addresses_with_priority(
        &self,
        addresses: &[String],
        _priority: csv_explorer_shared::PriorityLevel,
        _network: csv_explorer_shared::Network,
    ) -> ChainResult<AddressIndexingResult> {
        let mut result = AddressIndexingResult {
            addresses_processed: 0,
            sanads_indexed: 0,
            seals_indexed: 0,
            transfers_indexed: 0,
            contracts_indexed: 0,
            errors: Vec::new(),
        };

        for address in addresses {
            if let Ok(sanads) = self.index_sanads_by_address(address).await {
                result.sanads_indexed += sanads.len() as u64;
                result.addresses_processed += 1;
            }
            if let Ok(seals) = self.index_seals_by_address(address).await {
                result.seals_indexed += seals.len() as u64;
            }
            if let Ok(transfers) = self.index_transfers_by_address(address).await {
                result.transfers_indexed += transfers.len() as u64;
            }
        }

        Ok(result)
    }

    // -----------------------------------------------------------------------
    // Advanced commitment and proof indexing methods
    // -----------------------------------------------------------------------

    async fn index_enhanced_sanads(&self, block: u64) -> ChainResult<Vec<EnhancedSanadRecord>> {
        let txns = self.fetch_block_transactions(block).await?;
        let mut sanads = Vec::new();

        for txn in &txns {
            if let Some(events) = &txn.events {
                for event in events {
                    if event.type_.contains("csv_seal") && event.type_.contains("AnchorEvent") {
                        if let Some(sanad) = self.parse_sanad_from_event(event, &txn.hash) {
                            let enhanced = EnhancedSanadRecord {
                                id: sanad.id.clone(),
                                chain: sanad.chain.clone(),
                                seal_ref: sanad.seal_ref.clone(),
                                commitment: sanad.commitment.clone(),
                                owner: sanad.owner.clone(),
                                created_at: sanad.created_at,
                                created_tx: sanad.created_tx.clone(),
                                status: sanad.status.to_string(),
                                metadata: sanad.metadata,
                                transfer_count: sanad.transfer_count,
                                last_transfer_at: sanad.last_transfer_at,
                                commitment_scheme: CommitmentScheme::HashBased,
                                commitment_version: 2,
                                protocol_id: "csv-apt".to_string(),
                                mpc_root: None,
                                domain_separator: Some("aptos-mainnet".to_string()),
                                inclusion_proof_type: InclusionProofType::Accumulator,
                                finality_proof_type: FinalityProofType::Checkpoint,
                                proof_size_bytes: None,
                                confirmations: None,
                            };
                            sanads.push(enhanced);
                        }
                    }
                }
            }
        }

        Ok(sanads)
    }

    async fn index_enhanced_seals(&self, block: u64) -> ChainResult<Vec<EnhancedSealRecord>> {
        let txns = self.fetch_block_transactions(block).await?;
        let mut seals = Vec::new();

        for txn in &txns {
            if let Some(events) = &txn.events {
                for event in events {
                    if event.type_.contains("csv_seal") || event.type_.contains("Seal") {
                        if let Some(seal) = self.parse_seal_from_event(event, block) {
                            let enhanced = EnhancedSealRecord {
                                id: seal.id.clone(),
                                chain: seal.chain.clone(),
                                seal_type: seal.seal_type.to_string(),
                                seal_ref: seal.seal_ref.clone(),
                                sanad_id: seal.sanad_id.clone(),
                                status: seal.status.to_string(),
                                consumed_at: seal.consumed_at,
                                consumed_tx: seal.consumed_tx.clone(),
                                block_height: seal.block_height,
                                seal_proof_type: "accumulator".to_string(),
                                seal_proof_verified: None,
                            };
                            seals.push(enhanced);
                        }
                    }
                }
            }
        }

        Ok(seals)
    }

    async fn index_enhanced_transfers(
        &self,
        block: u64,
    ) -> ChainResult<Vec<EnhancedTransferRecord>> {
        let txns = self.fetch_block_transactions(block).await?;
        let mut transfers = Vec::new();

        for txn in &txns {
            if let Some(events) = &txn.events {
                for event in events {
                    if event.type_.contains("CrossChain") || event.type_.contains("bridge_transfer")
                    {
                        if let Some(transfer) = self.parse_transfer_from_event(event, &txn.hash) {
                            let enhanced = EnhancedTransferRecord {
                                id: transfer.id.clone(),
                                sanad_id: transfer.sanad_id.clone(),
                                from_chain: transfer.from_chain.clone(),
                                to_chain: transfer.to_chain.clone(),
                                from_owner: transfer.from_owner.clone(),
                                to_owner: transfer.to_owner.clone(),
                                lock_tx: transfer.lock_tx.clone(),
                                mint_tx: transfer.mint_tx.clone(),
                                proof_ref: transfer.proof_ref.clone(),
                                status: transfer.status.to_string(),
                                created_at: transfer.created_at,
                                completed_at: transfer.completed_at,
                                duration_ms: transfer.duration_ms,
                                cross_chain_proof_type: Some("accumulator".to_string()),
                                bridge_contract: Some("0x1::csv_bridge".to_string()),
                                bridge_proof_verified: None,
                            };
                            transfers.push(enhanced);
                        }
                    }
                }
            }
        }

        Ok(transfers)
    }

    fn detect_commitment_scheme(&self, _data: &[u8]) -> Option<CommitmentScheme> {
        Some(CommitmentScheme::HashBased)
    }

    fn detect_inclusion_proof_type(&self) -> InclusionProofType {
        InclusionProofType::Accumulator
    }

    fn detect_finality_proof_type(&self) -> FinalityProofType {
        FinalityProofType::Checkpoint
    }
}

impl AptosIndexer {
    /// Create a new Aptos indexer.
    pub fn new(config: ChainConfig, rpc_manager: RpcManager) -> Self {
        Self {
            config,
            _http_client: Client::new(),
            rpc_manager: Some(rpc_manager),
        }
    }

    /// Fetch transactions for a specific block/version.
    async fn fetch_block_transactions(&self, version: u64) -> ChainResult<Vec<AptosTxn>> {
        // Aptos uses version-based indexing
        let rpc_url = if let Some(ref manager) = self.rpc_manager {
            if let Some(endpoint) = manager.get_endpoint("aptos") {
                endpoint.url.clone()
            } else {
                self.config.rpc_url.clone()
            }
        } else {
            self.config.rpc_url.clone()
        };

        let client = if let Some(ref manager) = self.rpc_manager {
            manager.get_client("aptos")
        } else {
            Some(Client::new())
        };

        let base_url = rpc_url.trim_end_matches('/');
        let url = format!("{}/v1/transactions?start={}&limit=100", base_url, version);

        let resp: Vec<AptosTxn> = match client {
            Some(ref client) => client.get(&url).send().await?.json().await?,
            None => {
                let client = Client::new();
                client.get(&url).send().await?.json().await?
            }
        };
        Ok(resp)
    }

    fn parse_sanad_from_event(&self, event: &AptosEvent, tx_hash: &str) -> Option<SanadRecord> {
        let sanad_id = event.data.get("sanad_id")?.as_str()?.to_string();
        let owner = event.data.get("owner")?.as_str()?.to_string();
        let commitment = event.data.get("commitment")?.as_str()?.to_string();

        // Use actual seal_id from event data if available, otherwise "unknown"
        let seal_ref = event
            .data
            .get("seal_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        Some(SanadRecord {
            id: sanad_id,
            chain: "aptos".to_string(),
            seal_ref,
            commitment,
            owner,
            created_at: chrono::Utc::now(),
            created_tx: tx_hash.to_string(),
            status: csv_explorer_shared::SanadStatus::Active,
            metadata: Some(event.data.clone()),
            transfer_count: 0,
            last_transfer_at: None,
        })
    }

    fn parse_seal_from_event(&self, event: &AptosEvent, block: u64) -> Option<SealRecord> {
        let seal_id = event
            .data
            .get("seal_id")
            .or_else(|| event.data.get("nullifier"))?
            .as_str()?
            .to_string();

        let is_consumed = event.type_.contains("Consumed") || event.type_.contains("spent");

        // seal_ref should be the actual seal_id from on-chain event data, not a constructed string
        let seal_ref = seal_id.clone();

        Some(SealRecord {
            id: seal_id,
            chain: "aptos".to_string(),
            seal_type: SealType::Resource,
            seal_ref,
            sanad_id: None,
            status: if is_consumed {
                SealStatus::Consumed
            } else {
                SealStatus::Available
            },
            consumed_at: None,
            consumed_tx: None,
            block_height: block,
        })
    }

    fn parse_transfer_from_event(
        &self,
        event: &AptosEvent,
        tx_hash: &str,
    ) -> Option<TransferRecord> {
        let sanad_id = event.data.get("sanad_id")?.as_str()?.to_string();
        let from_chain = event.data.get("from_chain")?.as_str()?.to_string();
        let to_chain = event.data.get("to_chain")?.as_str()?.to_string();

        Some(TransferRecord {
            id: format!("aptos-xfer-{}", tx_hash),
            sanad_id,
            from_chain,
            to_chain,
            from_owner: "aptos-sender".to_string(),
            to_owner: "pending".to_string(),
            lock_tx: tx_hash.to_string(),
            mint_tx: None,
            proof_ref: None,
            status: csv_explorer_shared::TransferStatus::Initiated,
            created_at: chrono::Utc::now(),
            completed_at: None,
            duration_ms: None,
            lock_tx_explorer_url: None,
            mint_tx_explorer_url: None,
        })
    }
}
