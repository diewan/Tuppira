#![allow(clippy::collapsible_if)] // EXP-003 replaces these legacy decoders wholesale.

/// Ethereum chain indexer implementation.
///
/// Fixes applied:
/// 1. Event signatures computed via real keccak256 (sha3 crate) — temporary hex strings removed.
/// 2. `fetch_block` supplemented with `eth_getLogs` — transactions don't carry logs in
///    `eth_getBlockByNumber`; logs come from a separate RPC call.
/// 3. `parse_sanad_from_log` — owner read from topics[3] (indexed), not the contract address.
/// 4. RPC client obtained via `rpc_manager.get_client()` which now builds authenticated clients.
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;

use super::chain_indexer::{AddressIndexingResult, ChainIndexer, ChainResult};
use super::rpc_manager::RpcManager;
use tuppira_shared::{
    ChainConfig, CommitmentScheme, ContractStatus, ContractType, CsvContract, EnhancedSanadRecord,
    EnhancedSealRecord, EnhancedTransferRecord, FinalityProofType, InclusionProofType, Network,
    PriorityLevel, SanadRecord, SealRecord, SealStatus, SealType, TransferRecord, TuppiraError,
};

// -----------------------------------------------------------------------
// Event signatures — computed once at startup via keccak256
// -----------------------------------------------------------------------

/// keccak256("SealUsed(bytes32,bytes32)")
fn sig_seal_used() -> String {
    keccak256_selector("SealUsed(bytes32,bytes32)")
}

/// keccak256("SanadCreated(bytes32,bytes32,address)")
fn sig_sanad_created() -> String {
    keccak256_selector("SanadCreated(bytes32,bytes32,address)")
}

/// keccak256("CrossChainLock(bytes32,bytes32,address,uint8,bytes,bytes32,uint8,bytes32,bytes32,uint8,bytes32)")
fn sig_cross_chain_lock() -> String {
    keccak256_selector(
        "CrossChainLock(bytes32,bytes32,address,uint8,bytes,bytes32,uint8,bytes32,bytes32,uint8,bytes32)",
    )
}

/// keccak256("SanadMinted(bytes32,bytes32,address,uint8,bytes,uint8,bytes32,bytes32,uint8,bytes32)")
fn sig_sanad_minted() -> String {
    keccak256_selector(
        "SanadMinted(bytes32,bytes32,address,uint8,bytes,uint8,bytes32,bytes32,uint8,bytes32)",
    )
}

fn keccak256_selector(signature: &str) -> String {
    let mut hasher = Keccak256::new();
    hasher.update(signature.as_bytes());
    format!("0x{}", hex::encode(hasher.finalize()))
}

// -----------------------------------------------------------------------
// Ethereum-specific indexer
// -----------------------------------------------------------------------

pub struct EthereumIndexer {
    config: ChainConfig,
    http_client: Client,
    csv_contracts: HashMap<String, ContractType>,
    rpc_manager: Option<RpcManager>,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Vec<serde_json::Value>,
    id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

/// Minimal block shape (transactions included as hashes only — we fetch logs separately)
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BlockData {
    number: String,
    transactions: Vec<TxRef>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum TxRef {
    Hash(String),
    Full { hash: String },
}

#[allow(dead_code)]
impl TxRef {
    fn hash(&self) -> &str {
        match self {
            TxRef::Hash(h) => h,
            TxRef::Full { hash } => hash,
        }
    }
}

/// Log returned by eth_getLogs
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct LogData {
    address: String,
    topics: Vec<String>,
    data: String,
    #[serde(rename = "blockNumber")]
    block_number: String,
    #[serde(rename = "transactionHash")]
    transaction_hash: String,
}

#[async_trait]
impl ChainIndexer for EthereumIndexer {
    fn chain_id(&self) -> &str {
        "ethereum"
    }
    fn chain_name(&self) -> &str {
        "Ethereum"
    }

    async fn initialize(&self) -> ChainResult<()> {
        tracing::info!(chain = "ethereum", "Ethereum indexer initialized");
        Ok(())
    }

    async fn index_tuppira_events(
        &self,
        block: u64,
    ) -> ChainResult<Vec<tuppira_shared::TuppiraEventDto>> {
        Err(TuppiraError::BlockError {
            chain: self.chain_id().to_string(),
            block,
            message: "canonical Ethereum event decoder is not configured".to_string(),
        })
    }

    async fn get_chain_tip(&self) -> ChainResult<u64> {
        let (url, client) = self.rpc_endpoint();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_blockNumber".to_string(),
            params: vec![],
            id: 1,
        };
        let resp: JsonRpcResponse = client.post(&url).json(&req).send().await?.json().await?;
        if let Some(result) = resp.result {
            let hex_str = result.as_str().unwrap_or("0x0");
            Ok(u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).unwrap_or(0))
        } else {
            Err(TuppiraError::RpcError {
                chain: "ethereum".to_string(),
                message: "Failed to get block number".to_string(),
            })
        }
    }

    async fn get_latest_synced_block(&self) -> ChainResult<u64> {
        Ok(0)
    }

    async fn index_sanads(&self, block: u64) -> ChainResult<Vec<SanadRecord>> {
        let logs = self
            .fetch_logs(block, &[sig_seal_used(), sig_sanad_created()])
            .await?;
        let mut sanads = Vec::new();

        for log in &logs {
            if log.topics.first().map(String::as_str) == Some(&sig_seal_used())
                || log.topics.first().map(String::as_str) == Some(&sig_sanad_created())
            {
                if let Some(sanad) = self.parse_sanad_from_log(log) {
                    sanads.push(sanad);
                }
            }
        }

        tracing::debug!(
            chain = "ethereum",
            block,
            count = sanads.len(),
            "Indexed sanads"
        );
        Ok(sanads)
    }

    async fn index_seals(&self, block: u64) -> ChainResult<Vec<SealRecord>> {
        let logs = self.fetch_logs(block, &[sig_seal_used()]).await?;
        let mut seals = Vec::new();

        for log in &logs {
            if log.topics.first().map(String::as_str) == Some(&sig_seal_used()) {
                if let Some(seal) = self.parse_seal_from_log(log, block) {
                    seals.push(seal);
                }
            }
        }

        tracing::debug!(
            chain = "ethereum",
            block,
            count = seals.len(),
            "Indexed seals"
        );
        Ok(seals)
    }

    async fn index_transfers(&self, block: u64) -> ChainResult<Vec<TransferRecord>> {
        let sigs = vec![sig_cross_chain_lock(), sig_sanad_minted()];
        let logs = self.fetch_logs(block, &sigs).await?;
        let mut transfers = Vec::new();

        for log in &logs {
            let topic0 = log.topics.first().map(String::as_str);
            if topic0 == Some(&sig_cross_chain_lock()) || topic0 == Some(&sig_sanad_minted()) {
                if let Some(transfer) = self.parse_transfer_from_log(log) {
                    transfers.push(transfer);
                }
            }
        }

        tracing::debug!(
            chain = "ethereum",
            block,
            count = transfers.len(),
            "Indexed transfers"
        );
        Ok(transfers)
    }

    async fn index_contracts(&self, _block: u64) -> ChainResult<Vec<CsvContract>> {
        Ok(self
            .csv_contracts
            .iter()
            .map(|(address, contract_type)| CsvContract {
                id: format!("eth-{}", address),
                chain: "ethereum".to_string(),
                contract_type: *contract_type,
                address: address.clone(),
                deployed_tx: "genesis".to_string(),
                deployed_at: chrono::Utc::now(),
                version: "1.0.0".to_string(),
                status: ContractStatus::Active,
            })
            .collect())
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
                commitment_scheme: CommitmentScheme::KZG,
                commitment_version: 2,
                protocol_id: "csv-eth".to_string(),
                mpc_root: None,
                domain_separator: Some("ethereum-mainnet".to_string()),
                inclusion_proof_type: InclusionProofType::MerklePatricia,
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
                seal_proof_type: "merkle_patricia".to_string(),
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
                cross_chain_proof_type: Some("merkle_patricia".to_string()),
                bridge_contract: None,
                bridge_proof_verified: None,
            })
            .collect())
    }

    fn detect_commitment_scheme(&self, _data: &[u8]) -> Option<CommitmentScheme> {
        Some(CommitmentScheme::KZG)
    }

    fn detect_inclusion_proof_type(&self) -> InclusionProofType {
        InclusionProofType::MerklePatricia
    }

    fn detect_finality_proof_type(&self) -> FinalityProofType {
        FinalityProofType::FinalizedBlock
    }
}

impl EthereumIndexer {
    pub fn new(config: ChainConfig, rpc_manager: RpcManager) -> Self {
        let mut csv_contracts = HashMap::new();
        csv_contracts.insert(
            "0x0000000000000000000000000000000000000000".to_string(),
            ContractType::NullifierRegistry,
        );
        csv_contracts.insert(
            "0x0000000000000000000000000000000000000001".to_string(),
            ContractType::SanadRegistry,
        );
        csv_contracts.insert(
            "0x0000000000000000000000000000000000000002".to_string(),
            ContractType::Bridge,
        );
        Self {
            config,
            http_client: Client::new(),
            csv_contracts,
            rpc_manager: Some(rpc_manager),
        }
    }

    fn rpc_endpoint(&self) -> (String, Client) {
        if let Some(ref mgr) = self.rpc_manager {
            if let Some(endpoint) = mgr.get_endpoint("ethereum") {
                let client = mgr.get_client("ethereum").unwrap_or_default();
                return (endpoint.url, client);
            }
        }
        (self.config.rpc_url.clone(), self.http_client.clone())
    }

    // -----------------------------------------------------------------------
    // FIX: fetch logs via eth_getLogs — transactions in eth_getBlockByNumber
    //      do NOT include receipts/logs inline.
    // -----------------------------------------------------------------------

    async fn fetch_logs(&self, block: u64, topics: &[String]) -> ChainResult<Vec<LogData>> {
        let (url, client) = self.rpc_endpoint();
        let block_hex = format!("0x{:x}", block);

        // Build topics filter — topic0 list (OR match across all CSV event sigs)
        let topic0_list: Vec<serde_json::Value> = topics
            .iter()
            .map(|t| serde_json::Value::String(t.clone()))
            .collect();

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_getLogs".to_string(),
            params: vec![serde_json::json!({
                "fromBlock": block_hex,
                "toBlock":   block_hex,
                "topics":    [topic0_list],
            })],
            id: 1,
        };

        let resp: JsonRpcResponse = client.post(&url).json(&req).send().await?.json().await?;

        if let Some(result) = resp.result {
            let logs: Vec<LogData> =
                serde_json::from_value(result).map_err(|e| TuppiraError::RpcParseError {
                    chain: "ethereum".to_string(),
                    message: e.to_string(),
                })?;
            Ok(logs)
        } else {
            let err_msg = resp.error.map(|e| e.to_string()).unwrap_or_default();
            tracing::warn!(chain = "ethereum", block, error = %err_msg, "eth_getLogs failed");
            Ok(Vec::new())
        }
    }

    // -----------------------------------------------------------------------
    // FIX: owner comes from topics[3] (indexed address param), not log.address
    // -----------------------------------------------------------------------

    fn parse_sanad_from_log(&self, log: &LogData) -> Option<SanadRecord> {
        if log.topics.len() < 2 {
            return None;
        }

        let seal_id = log.topics.get(1).cloned().unwrap_or_default();
        let commitment = log.data.strip_prefix("0x").unwrap_or(&log.data).to_string();

        // FIX: owner is topics[3] (indexed address), fallback to topics[2] or contract
        let owner = log
            .topics
            .get(3)
            .or_else(|| log.topics.get(2))
            .map(|t| {
                // Ethereum addresses are sanad-aligned in 32-byte topic → take last 20 bytes
                if t.len() >= 42 {
                    t[t.len() - 40..].to_string()
                } else {
                    t.strip_prefix("0x").unwrap_or(t).to_string()
                }
            })
            .unwrap_or_else(|| {
                log.address
                    .strip_prefix("0x")
                    .unwrap_or(&log.address)
                    .to_string()
            });

        let id_suffix = if seal_id.len() >= 18 {
            &seal_id[..18]
        } else {
            &seal_id
        };
        Some(SanadRecord {
            id: format!("eth-{}-{}", log.transaction_hash, id_suffix),
            chain: "ethereum".to_string(),
            seal_ref: seal_id.clone(),
            commitment,
            owner,
            created_at: chrono::Utc::now(),
            created_tx: log.transaction_hash.clone(),
            status: tuppira_shared::SanadStatus::Active,
            metadata: Some(serde_json::json!({
                "protocol_id": "csv-eth",
                "commitment_scheme": "kzg",
                "inclusion_proof": "merkle_patricia",
                "contract_address": log.address,
            })),
            transfer_count: 0,
            last_transfer_at: None,
        })
    }

    fn parse_seal_from_log(&self, log: &LogData, block: u64) -> Option<SealRecord> {
        let seal_id = log.topics.get(1).cloned().unwrap_or_default();
        let id_suffix = if seal_id.len() >= 18 {
            &seal_id[..18]
        } else {
            &seal_id
        };
        Some(SealRecord {
            id: format!("eth-seal-{}", id_suffix),
            chain: "ethereum".to_string(),
            seal_type: SealType::Nullifier,
            seal_ref: seal_id.clone(),
            sanad_id: None,
            status: SealStatus::Consumed,
            consumed_at: Some(chrono::Utc::now()),
            consumed_tx: Some(log.transaction_hash.clone()),
            block_height: block,
        })
    }

    fn parse_transfer_from_log(&self, log: &LogData) -> Option<TransferRecord> {
        if log.topics.len() < 4 {
            return None;
        }
        let sanad_id = log.topics.get(1).cloned().unwrap_or_default();
        let from_owner = log.topics.get(3).cloned().unwrap_or_default();
        Some(TransferRecord {
            id: format!("eth-xfer-{}", log.transaction_hash),
            sanad_id,
            from_chain: "ethereum".to_string(),
            to_chain: "unknown".to_string(),
            from_owner,
            to_owner: "unknown".to_string(),
            lock_tx: log.transaction_hash.clone(),
            mint_tx: None,
            proof_ref: Some(log.transaction_hash.clone()),
            status: tuppira_shared::TransferStatus::Initiated,
            created_at: chrono::Utc::now(),
            completed_at: None,
            duration_ms: None,
            lock_tx_explorer_url: tuppira_shared::block_explorer::tx_url(
                "ethereum",
                self.config.network,
                &log.transaction_hash,
            ),
            mint_tx_explorer_url: None,
        })
    }
}
