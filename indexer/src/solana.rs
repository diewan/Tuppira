/// Solana chain indexer implementation.
///
/// Fixes applied:
/// 1. `get_chain_tip` — `getSlot` returns a plain u64, not `{ "slot": N }`.
///    `result.get("slot")` always returned None → slot was always 0.
/// 2. `get_transactions_for_slot` — use `getBlock` with correct params.
/// 3. RPC client from `rpc_manager.get_client()` (now builds authenticated clients).
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::chain_indexer::{AddressIndexingResult, ChainIndexer, ChainResult};
use super::rpc_manager::RpcManager;
use csv_explorer_shared::{
    ChainConfig, CommitmentScheme, ContractStatus, ContractType, CsvContract, EnhancedSanadRecord,
    EnhancedSealRecord, EnhancedTransferRecord, ExplorerError, FinalityProofType,
    InclusionProofType, Network, PriorityLevel, SanadRecord, SealRecord, SealStatus, SealType,
    TransferRecord,
};

pub struct SolanaIndexer {
    config: ChainConfig,
    http_client: Client,
    rpc_manager: Option<RpcManager>,
}

#[derive(Debug, Serialize)]
struct SolanaRpcRequest {
    jsonrpc: String,
    method: String,
    params: Vec<serde_json::Value>,
    id: u64,
}

#[derive(Debug, Deserialize)]
struct SolanaRpcResponse {
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TransactionInfo {
    slot: Option<u64>,
    transaction: Option<serde_json::Value>,
    meta: Option<serde_json::Value>,
    #[serde(rename = "blockTime")]
    block_time: Option<i64>,
}

#[async_trait]
impl ChainIndexer for SolanaIndexer {
    fn chain_id(&self) -> &str {
        "solana"
    }
    fn chain_name(&self) -> &str {
        "Solana"
    }

    async fn initialize(&self) -> ChainResult<()> {
        tracing::info!(chain = "solana", "Solana indexer initialized");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // FIX: getSlot returns a NUMBER directly, not { "slot": N }
    // -----------------------------------------------------------------------
    async fn get_chain_tip(&self) -> ChainResult<u64> {
        let (url, client) = self.rpc_endpoint();
        let req = SolanaRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "getSlot".to_string(),
            params: vec![],
            id: 1,
        };

        let resp: SolanaRpcResponse = client.post(&url).json(&req).send().await?.json().await?;

        if let Some(result) = resp.result {
            // FIX: result IS the slot number (u64), not an object
            let slot = result.as_u64().unwrap_or(0);
            Ok(slot)
        } else {
            let err = resp.error.map(|e| e.to_string()).unwrap_or_default();
            Err(ExplorerError::RpcError {
                chain: "solana".to_string(),
                message: format!("getSlot failed: {}", err),
            })
        }
    }

    async fn get_latest_synced_block(&self) -> ChainResult<u64> {
        Ok(0)
    }

    async fn index_sanads(&self, block: u64) -> ChainResult<Vec<SanadRecord>> {
        let txns = self.get_transactions_for_slot(block).await?;
        let mut sanads = Vec::new();

        for txn_info in &txns {
            if let Some(meta) = &txn_info.meta {
                if let Some(logs) = meta.get("logMessages").and_then(|v| v.as_array()) {
                    for log in logs {
                        if let Some(log_str) = log.as_str() {
                            if log_str.contains("csv_sanad_created")
                                || log_str.contains("SanadCreated")
                            {
                                if let Some(sanad) = self.parse_sanad_from_log(txn_info, log_str) {
                                    sanads.push(sanad);
                                }
                            }
                        }
                    }
                }
            }
        }

        tracing::debug!(
            chain = "solana",
            block,
            count = sanads.len(),
            "Indexed sanads"
        );
        Ok(sanads)
    }

    async fn index_seals(&self, block: u64) -> ChainResult<Vec<SealRecord>> {
        let txns = self.get_transactions_for_slot(block).await?;
        let mut seals = Vec::new();

        for txn_info in &txns {
            if let Some(meta) = &txn_info.meta {
                if let Some(logs) = meta.get("logMessages").and_then(|v| v.as_array()) {
                    for log in logs {
                        if let Some(log_str) = log.as_str() {
                            if log_str.contains("csv_seal") || log_str.contains("SealConsumed") {
                                if let Some(seal) = self.parse_seal_from_log(txn_info, block) {
                                    seals.push(seal);
                                }
                            }
                        }
                    }
                }
            }
        }

        tracing::debug!(
            chain = "solana",
            block,
            count = seals.len(),
            "Indexed seals"
        );
        Ok(seals)
    }

    async fn index_transfers(&self, block: u64) -> ChainResult<Vec<TransferRecord>> {
        let txns = self.get_transactions_for_slot(block).await?;
        let mut transfers = Vec::new();

        for txn_info in &txns {
            if let Some(meta) = &txn_info.meta {
                if let Some(logs) = meta.get("logMessages").and_then(|v| v.as_array()) {
                    for log in logs {
                        if let Some(log_str) = log.as_str() {
                            if log_str.contains("csv_transfer")
                                || log_str.contains("CrossChainTransfer")
                            {
                                if let Some(transfer) = self.parse_transfer_from_log(txn_info) {
                                    transfers.push(transfer);
                                }
                            }
                        }
                    }
                }
            }
        }

        tracing::debug!(
            chain = "solana",
            block,
            count = transfers.len(),
            "Indexed transfers"
        );
        Ok(transfers)
    }

    async fn index_contracts(&self, _block: u64) -> ChainResult<Vec<CsvContract>> {
        Ok(vec![CsvContract {
            id: "sol-csv-program".to_string(),
            chain: "solana".to_string(),
            contract_type: ContractType::SanadRegistry,
            address: "CsvRegistry111111111111111111111111111".to_string(),
            deployed_tx: "genesis".to_string(),
            deployed_at: chrono::Utc::now(),
            version: "1.0.0".to_string(),
            status: ContractStatus::Active,
        }])
    }

    async fn index_enhanced_sanads(&self, block: u64) -> ChainResult<Vec<EnhancedSanadRecord>> {
        Ok(self
            .index_sanads(block)
            .await?
            .into_iter()
            .map(|sanad| EnhancedSanadRecord {
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
                commitment_version: 1,
                protocol_id: "csv-sol".to_string(),
                mpc_root: None,
                domain_separator: Some("solana-mainnet".to_string()),
                inclusion_proof_type: InclusionProofType::AccountState,
                finality_proof_type: FinalityProofType::FinalizedBlock,
                proof_size_bytes: None,
                confirmations: None,
            })
            .collect())
    }

    async fn index_enhanced_seals(&self, block: u64) -> ChainResult<Vec<EnhancedSealRecord>> {
        Ok(self
            .index_seals(block)
            .await?
            .into_iter()
            .map(|s| EnhancedSealRecord {
                id: s.id.clone(),
                chain: s.chain.clone(),
                seal_type: s.seal_type.to_string(),
                seal_ref: s.seal_ref.clone(),
                sanad_id: s.sanad_id.clone(),
                status: s.status.to_string(),
                consumed_at: s.consumed_at,
                consumed_tx: s.consumed_tx.clone(),
                block_height: s.block_height,
                seal_proof_type: "account_proof".to_string(),
                seal_proof_verified: None,
            })
            .collect())
    }

    async fn index_enhanced_transfers(
        &self,
        block: u64,
    ) -> ChainResult<Vec<EnhancedTransferRecord>> {
        Ok(self
            .index_transfers(block)
            .await?
            .into_iter()
            .map(|t| EnhancedTransferRecord {
                id: t.id.clone(),
                sanad_id: t.sanad_id.clone(),
                from_chain: t.from_chain.clone(),
                to_chain: t.to_chain.clone(),
                from_owner: t.from_owner.clone(),
                to_owner: t.to_owner.clone(),
                lock_tx: t.lock_tx.clone(),
                mint_tx: t.mint_tx.clone(),
                proof_ref: t.proof_ref.clone(),
                status: t.status.to_string(),
                created_at: t.created_at,
                completed_at: t.completed_at,
                duration_ms: t.duration_ms,
                cross_chain_proof_type: Some("account_proof".to_string()),
                bridge_contract: None,
                bridge_proof_verified: None,
            })
            .collect())
    }

    fn detect_commitment_scheme(&self, _data: &[u8]) -> Option<CommitmentScheme> {
        Some(CommitmentScheme::HashBased)
    }

    fn detect_inclusion_proof_type(&self) -> InclusionProofType {
        InclusionProofType::AccountState
    }

    fn detect_finality_proof_type(&self) -> FinalityProofType {
        FinalityProofType::FinalizedBlock
    }

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
        _priority: PriorityLevel,
        _network: Network,
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
        }
        Ok(result)
    }
}

impl SolanaIndexer {
    pub fn new(config: ChainConfig, rpc_manager: RpcManager) -> Self {
        Self {
            config,
            http_client: Client::new(),
            rpc_manager: Some(rpc_manager),
        }
    }

    fn rpc_endpoint(&self) -> (String, Client) {
        if let Some(ref mgr) = self.rpc_manager {
            if let Some(endpoint) = mgr.get_endpoint("solana") {
                let client = mgr.get_client("solana").unwrap_or_default();
                return (endpoint.url, client);
            }
        }
        (self.config.rpc_url.clone(), self.http_client.clone())
    }

    // -----------------------------------------------------------------------
    // FIX: use getBlock with correct encoding params
    // -----------------------------------------------------------------------
    async fn get_transactions_for_slot(&self, slot: u64) -> ChainResult<Vec<TransactionInfo>> {
        let (url, client) = self.rpc_endpoint();

        let req = SolanaRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "getBlock".to_string(),
            params: vec![
                serde_json::json!(slot),
                serde_json::json!({
                    "encoding": "json",
                    "transactionDetails": "full",
                    "rewards": false,
                    "maxSupportedTransactionVersion": 0
                }),
            ],
            id: 1,
        };

        let resp: SolanaRpcResponse = match client.post(&url).json(&req).send().await {
            Ok(r) => r.json().await.map_err(|e| ExplorerError::RpcParseError {
                chain: "solana".to_string(),
                message: e.to_string(),
            })?,
            Err(e) => {
                tracing::warn!(chain = "solana", slot, error = %e, "getBlock request failed");
                return Ok(Vec::new());
            }
        };

        if let Some(result) = resp.result {
            // Block result: { transactions: [...], ... }
            if let Some(txns) = result.get("transactions").and_then(|v| v.as_array()) {
                let mut out = Vec::with_capacity(txns.len());
                for txn in txns {
                    if let Ok(info) = serde_json::from_value::<TransactionInfo>(txn.clone()) {
                        out.push(info);
                    }
                }
                Ok(out)
            } else {
                Ok(Vec::new())
            }
        } else {
            // Slot may be skipped / not yet confirmed — not an error
            tracing::debug!(
                chain = "solana",
                slot,
                "No block data for slot (possibly skipped)"
            );
            Ok(Vec::new())
        }
    }

    fn tx_signature(txn_info: &TransactionInfo) -> String {
        txn_info
            .transaction
            .as_ref()
            .and_then(|t| t.get("signatures"))
            .and_then(|s| s.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }

    fn parse_sanad_from_log(
        &self,
        txn_info: &TransactionInfo,
        _log_str: &str,
    ) -> Option<SanadRecord> {
        let sig = Self::tx_signature(txn_info);
        let slot = txn_info.slot.unwrap_or(0);

        // Extract account keys from transaction message for real seal reference
        // Solana transaction structure: transaction.message.accountKeys
        let first_account = txn_info
            .transaction
            .as_ref()
            .and_then(|t| t.get("message"))
            .and_then(|m| m.get("accountKeys"))
            .and_then(|keys| keys.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Use first account as potential seal reference, or "unknown" if unavailable
        let seal_ref = first_account.unwrap_or_else(|| "unknown".to_string());

        Some(SanadRecord {
            id: format!("sol-sanad-{}", sig),
            chain: "solana".to_string(),
            seal_ref,
            commitment: format!("sol-commitment-{}", sig),
            owner: "unknown".to_string(),
            created_at: chrono::Utc::now(),
            created_tx: sig,
            status: csv_explorer_shared::SanadStatus::Active,
            metadata: Some(serde_json::json!({
                "protocol_id": "csv-sol",
                "commitment_scheme": "hash_based",
                "inclusion_proof": "account_proof",
                "slot": slot,
            })),
            transfer_count: 0,
            last_transfer_at: None,
        })
    }

    fn parse_seal_from_log(&self, txn_info: &TransactionInfo, block: u64) -> Option<SealRecord> {
        let sig = Self::tx_signature(txn_info);

        // Extract account keys from transaction message for real seal reference
        // Solana transaction structure: transaction.message.accountKeys
        let first_account = txn_info
            .transaction
            .as_ref()
            .and_then(|t| t.get("message"))
            .and_then(|m| m.get("accountKeys"))
            .and_then(|keys| keys.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Use first account as potential seal reference, or "unknown" if unavailable
        let seal_ref = first_account.unwrap_or_else(|| "unknown".to_string());

        Some(SealRecord {
            id: format!("sol-seal-{}", sig),
            chain: "solana".to_string(),
            seal_type: SealType::Account,
            seal_ref,
            sanad_id: None,
            status: SealStatus::Consumed,
            consumed_at: Some(chrono::Utc::now()),
            consumed_tx: Some(sig),
            block_height: block,
        })
    }

    fn parse_transfer_from_log(&self, txn_info: &TransactionInfo) -> Option<TransferRecord> {
        let sig = Self::tx_signature(txn_info);
        Some(TransferRecord {
            id: format!("sol-xfer-{}", sig),
            sanad_id: format!("sol-sanad-{}", sig),
            from_chain: "solana".to_string(),
            to_chain: "unknown".to_string(),
            from_owner: "unknown".to_string(),
            to_owner: "unknown".to_string(),
            lock_tx: sig.clone(),
            mint_tx: None,
            proof_ref: Some(sig),
            status: csv_explorer_shared::TransferStatus::Initiated,
            created_at: chrono::Utc::now(),
            completed_at: None,
            duration_ms: None,
            lock_tx_explorer_url: None,
            mint_tx_explorer_url: None,
        })
    }
}
