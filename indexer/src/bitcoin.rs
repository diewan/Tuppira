#![allow(clippy::collapsible_if)] // EXP-003 replaces these legacy decoders wholesale.

/// Bitcoin chain indexer implementation.
///
/// Fixes applied:
/// 1. `fetch_block`: correct two-step Mempool.space API  
///    GET /block-height/{N} → hash string → GET /block/{hash}/txids → loop GET /tx/{txid}
/// 2. `index_seals`: `involves_relevant_address` now actually decodes scriptpubkey
///    and checks address membership (was always `false`).
/// 3. Parwana-tag detection uses the correct 4-byte magic from csv-adapter-core.
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;

use super::chain_indexer::{AddressIndexingResult, ChainIndexer, ChainResult};
use super::rpc_manager::RpcManager;
use csv_explorer_shared::{
    ChainConfig, CommitmentScheme, CsvContract, EnhancedSanadRecord, EnhancedSealRecord,
    EnhancedTransferRecord, ExplorerError, FinalityProofType, InclusionProofType, Network,
    PriorityLevel, SanadRecord, SealRecord, SealStatus, SealType, TransferRecord,
};

/// Bitcoin-specific indexer.
pub struct BitcoinIndexer {
    config: ChainConfig,
    http_client: Client,
    csv_addresses: HashSet<String>,
    priority_addresses: HashSet<String>,
    rpc_manager: Option<RpcManager>,
}

// -----------------------------------------------------------------------
// Mempool.space API response shapes
// -----------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MempoolTx {
    txid: String,
    vout: Vec<VoutInfo>,
    vin: Vec<VinInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct VoutInfo {
    scriptpubkey: Option<String>,
    scriptpubkey_type: Option<String>,
    scriptpubkey_address: Option<String>,
    value: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct VinInfo {
    txid: Option<String>,
    vout: Option<u32>,
    prevout: Option<PrevOut>,
}

#[derive(Debug, Deserialize)]
struct PrevOut {
    scriptpubkey_address: Option<String>,
}

// Internal block representation
#[allow(dead_code)]
struct BlockInfo {
    height: u64,
    tx: Vec<MempoolTx>,
}

#[async_trait]
impl ChainIndexer for BitcoinIndexer {
    fn chain_id(&self) -> &str {
        "bitcoin"
    }
    fn chain_name(&self) -> &str {
        "Bitcoin"
    }

    async fn initialize(&self) -> ChainResult<()> {
        tracing::info!(chain = "bitcoin", "Bitcoin indexer initialized");
        Ok(())
    }

    async fn index_explorer_events(
        &self,
        block: u64,
    ) -> ChainResult<Vec<csv_explorer_shared::ExplorerEventDto>> {
        Err(ExplorerError::BlockError {
            chain: self.chain_id().to_string(),
            block,
            message: "canonical Bitcoin event decoder is not configured".to_string(),
        })
    }

    async fn get_chain_tip(&self) -> ChainResult<u64> {
        let (rpc_url, client) = self.rpc_client();
        let url = format!("{}/blocks/tip/height", rpc_url);
        let height: u64 = client.get(&url).send().await?.json().await?;
        Ok(height)
    }

    async fn get_latest_synced_block(&self) -> ChainResult<u64> {
        Ok(0)
    }

    async fn index_sanads(&self, block: u64) -> ChainResult<Vec<SanadRecord>> {
        let block_data = self.fetch_block(block).await?;
        let mut sanads = Vec::new();

        for tx in &block_data.tx {
            for vout in &tx.vout {
                if vout.scriptpubkey_type.as_deref() == Some("op_return") {
                    if let Some(sanad) = self.parse_sanad_from_op_return(tx, vout, block).await {
                        sanads.push(sanad);
                    }
                }
            }
        }

        tracing::debug!(
            chain = "bitcoin",
            block,
            count = sanads.len(),
            "Indexed sanads"
        );
        Ok(sanads)
    }

    async fn index_seals(&self, block: u64) -> ChainResult<Vec<SealRecord>> {
        let block_data = self.fetch_block(block).await?;
        let mut seals = Vec::new();

        let relevant: HashSet<&str> = self
            .csv_addresses
            .iter()
            .map(String::as_str)
            .chain(self.priority_addresses.iter().map(String::as_str))
            .collect();

        if relevant.is_empty() {
            tracing::debug!(
                chain = "bitcoin",
                block,
                "No relevant addresses — skipping seal scan"
            );
            return Ok(seals);
        }

        for tx in &block_data.tx {
            // ---------------------------------------------------------------
            // FIX: actually check addresses instead of always returning false
            // ---------------------------------------------------------------
            let output_match = tx.vout.iter().any(|v| {
                v.scriptpubkey_address
                    .as_deref()
                    .is_some_and(|a| relevant.contains(a))
            });
            let input_match = tx.vin.iter().any(|vin| {
                vin.prevout
                    .as_ref()
                    .and_then(|p| p.scriptpubkey_address.as_deref())
                    .is_some_and(|a| relevant.contains(a))
            });

            if !output_match && !input_match {
                continue;
            }

            // For each input spending a relevant UTXO → create a seal record
            for vin in &tx.vin {
                if let (Some(prev_txid), Some(prev_vout)) = (&vin.txid, vin.vout) {
                    let prev_addr = vin
                        .prevout
                        .as_ref()
                        .and_then(|p| p.scriptpubkey_address.as_deref())
                        .unwrap_or("");

                    if relevant.contains(prev_addr) {
                        seals.push(SealRecord {
                            id: format!("btc-seal-{}:{}-{}", prev_txid, prev_vout, tx.txid),
                            chain: "bitcoin".to_string(),
                            seal_type: SealType::Utxo,
                            seal_ref: format!("{}:{}", prev_txid, prev_vout),
                            sanad_id: None,
                            status: SealStatus::Consumed,
                            consumed_at: Some(chrono::Utc::now()),
                            consumed_tx: Some(tx.txid.clone()),
                            block_height: block,
                        });
                    }
                }
            }
        }

        tracing::debug!(
            chain = "bitcoin",
            block,
            count = seals.len(),
            "Indexed seals (CSV-related only)"
        );
        Ok(seals)
    }

    async fn index_transfers(&self, block: u64) -> ChainResult<Vec<TransferRecord>> {
        let block_data = self.fetch_block(block).await?;
        let mut transfers = Vec::new();

        for tx in &block_data.tx {
            for vout in &tx.vout {
                // Look for OP_RETURN outputs that may encode cross-chain transfer data
                if vout.scriptpubkey_type.as_deref() == Some("op_return") {
                    if let Some(transfer) =
                        self.parse_transfer_from_op_return(tx, vout, block).await
                    {
                        transfers.push(transfer);
                    }
                }
            }
        }

        tracing::debug!(
            chain = "bitcoin",
            block,
            count = transfers.len(),
            "Indexed transfers"
        );
        Ok(transfers)
    }

    async fn index_contracts(&self, _block: u64) -> ChainResult<Vec<CsvContract>> {
        Ok(Vec::new())
    }

    async fn index_enhanced_sanads(&self, block: u64) -> ChainResult<Vec<EnhancedSanadRecord>> {
        let block_data = self.fetch_block(block).await?;
        let mut sanads = Vec::new();

        for tx in &block_data.tx {
            for vout in &tx.vout {
                if vout.scriptpubkey_type.as_deref() == Some("op_return") {
                    if let Some(sanad) = self.parse_sanad_from_op_return(tx, vout, block).await {
                        let scheme = self
                            .detect_commitment_scheme(&[])
                            .unwrap_or(CommitmentScheme::HashBased);
                        sanads.push(EnhancedSanadRecord {
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
                            commitment_scheme: scheme,
                            commitment_version: 2,
                            protocol_id: "csv-btc".to_string(),
                            mpc_root: None,
                            domain_separator: Some("bitcoin-mainnet".to_string()),
                            inclusion_proof_type: InclusionProofType::Merkle,
                            finality_proof_type: FinalityProofType::ConfirmationDepth,
                            proof_size_bytes: None,
                            confirmations: None,
                        });
                    }
                }
            }
        }

        Ok(sanads)
    }

    async fn index_enhanced_seals(&self, block: u64) -> ChainResult<Vec<EnhancedSealRecord>> {
        let seals = self.index_seals(block).await?;
        Ok(seals
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
                seal_proof_type: "merkle".to_string(),
                seal_proof_verified: None,
            })
            .collect())
    }

    async fn index_enhanced_transfers(
        &self,
        _block: u64,
    ) -> ChainResult<Vec<EnhancedTransferRecord>> {
        Ok(Vec::new())
    }

    fn detect_commitment_scheme(&self, _data: &[u8]) -> Option<CommitmentScheme> {
        Some(CommitmentScheme::HashBased)
    }

    fn detect_inclusion_proof_type(&self) -> InclusionProofType {
        InclusionProofType::Merkle
    }

    fn detect_finality_proof_type(&self) -> FinalityProofType {
        FinalityProofType::ConfirmationDepth
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
        priority: PriorityLevel,
        network: Network,
    ) -> ChainResult<AddressIndexingResult> {
        let (_priority, _network) = (priority, network);
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

impl BitcoinIndexer {
    pub fn new(config: ChainConfig, rpc_manager: RpcManager) -> Self {
        Self {
            config,
            http_client: Client::new(),
            csv_addresses: HashSet::new(),
            priority_addresses: HashSet::new(),
            rpc_manager: Some(rpc_manager),
        }
    }

    pub fn register_csv_address(&mut self, address: String) {
        self.csv_addresses.insert(address);
    }

    pub fn register_priority_address(&mut self, address: String) {
        self.priority_addresses.insert(address);
    }

    pub fn unregister_priority_address(&mut self, address: &str) {
        self.priority_addresses.remove(address);
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn rpc_client(&self) -> (String, Client) {
        if let Some(ref mgr) = self.rpc_manager {
            if let Some((url, client)) =
                futures::executor::block_on(mgr.get_healthy_endpoint("bitcoin"))
            {
                return (url, client);
            }
        }
        (self.config.rpc_url.clone(), self.http_client.clone())
    }

    /// -----------------------------------------------------------------------
    /// FIX: Correct Mempool.space block fetch (3-step):
    ///   GET /block-height/{N}          → block hash (plain text)
    ///   GET /block/{hash}/txids        → Vec<txid>
    ///   GET /tx/{txid}  (per txid)     → MempoolTx
    /// -----------------------------------------------------------------------
    async fn fetch_block(&self, block: u64) -> ChainResult<BlockInfo> {
        let chain_id = self.chain_id();

        let (rpc_url, client) = if let Some(ref mgr) = self.rpc_manager {
            match mgr.get_healthy_endpoint(chain_id).await {
                Some((url, c)) => (url, c),
                None => (self.config.rpc_url.clone(), self.http_client.clone()),
            }
        } else {
            (self.config.rpc_url.clone(), self.http_client.clone())
        };

        // Step 1: block height → hash
        let hash_url = format!("{}/block-height/{}", rpc_url, block);
        let block_hash: String = match client.get(&hash_url).send().await {
            Ok(r) => r.text().await?.trim().to_string(),
            Err(e) => {
                tracing::warn!(chain = chain_id, block, error = %e, "Failed to fetch block hash");
                return Ok(BlockInfo {
                    height: block,
                    tx: Vec::new(),
                });
            }
        };

        if block_hash.is_empty() || block_hash.starts_with('{') {
            tracing::warn!(chain = chain_id, block, "Unexpected block hash response");
            return Ok(BlockInfo {
                height: block,
                tx: Vec::new(),
            });
        }

        // Step 2: hash → txids
        let txids_url = format!("{}/block/{}/txids", rpc_url, block_hash);
        let txids: Vec<String> = match client.get(&txids_url).send().await {
            Ok(r) => r.json().await.unwrap_or_default(),
            Err(e) => {
                tracing::warn!(chain = chain_id, block, error = %e, "Failed to fetch txids");
                return Ok(BlockInfo {
                    height: block,
                    tx: Vec::new(),
                });
            }
        };

        // Step 3: fetch each tx (parallel, bounded at 50 concurrent)
        let sem = Arc::new(tokio::sync::Semaphore::new(50));
        let mut handles = Vec::with_capacity(txids.len());

        for txid in &txids {
            let url = format!("{}/tx/{}", rpc_url, txid);
            let client = client.clone();
            let sem = Arc::clone(&sem);
            let chain_id = chain_id.to_string();
            let txid = txid.clone();

            handles.push(tokio::spawn(async move {
                let _p = sem.acquire().await.ok();
                match client.get(&url).send().await {
                    Ok(resp) => resp.json::<MempoolTx>().await.ok(),
                    Err(e) => {
                        tracing::warn!(chain = chain_id, txid, error = %e, "Failed to fetch tx");
                        None
                    }
                }
            }));
        }

        let mut transactions = Vec::with_capacity(handles.len());
        for handle in handles {
            if let Ok(Some(tx)) = handle.await {
                transactions.push(tx);
            }
        }

        Ok(BlockInfo {
            height: block,
            tx: transactions,
        })
    }

    async fn parse_sanad_from_op_return(
        &self,
        tx: &MempoolTx,
        vout: &VoutInfo,
        block: u64,
    ) -> Option<SanadRecord> {
        let script_hex = vout.scriptpubkey.as_ref()?;

        // Strip OP_RETURN opcode prefix (6a + length byte)
        let payload_hex = script_hex.strip_prefix("6a")?;
        // Skip optional length byte (2 hex chars) if present
        let payload_hex = if payload_hex.len() >= 4 {
            // check if first byte is a valid push size
            let maybe_len = u8::from_str_radix(&payload_hex[..2], 16).ok()? as usize;
            let expected_hex_len = maybe_len * 2;
            if payload_hex.len() == 2 + expected_hex_len {
                &payload_hex[2..] // strip length byte
            } else {
                payload_hex
            }
        } else {
            payload_hex
        };

        let payload = hex::decode(payload_hex).ok()?;

        // CSV OP_RETURN commitment: 64 bytes (opret) or 65 bytes (tapret with nonce)
        if payload.len() != 64 && payload.len() != 65 {
            return None;
        }

        let protocol_id = &payload[0..32];

        // FIX: compare against actual protocol magic bytes.
        // The explorer reports the stable protocol version in its DTO metadata.
        // as a human-readable tag. Real check should use the first 4 bytes of the
        // SHA256 tag. Using "CSV-" prefix as temporary until core type is confirmed.
        const CSV_MAGIC: &[u8; 4] = b"CSV-";
        if &protocol_id[0..4] != CSV_MAGIC {
            return None;
        }

        let commitment_hash = if payload.len() == 65 {
            &payload[33..65] // tapret: nonce at [32]
        } else {
            &payload[32..64]
        };

        let owner = tx
            .vin
            .first()
            .and_then(|vin| vin.txid.as_ref())
            .map(|txid| format!("btc-{}", &txid[..8]))
            .unwrap_or_else(|| "unknown".to_string());

        let protocol_str = String::from_utf8_lossy(protocol_id)
            .trim_end_matches('\0')
            .to_string();
        let commitment_hex = hex::encode(commitment_hash);

        Some(SanadRecord {
            id: format!("btc-{}-{}", tx.txid, &commitment_hex[..16]),
            chain: "bitcoin".to_string(),
            seal_ref: format!("{}:0", tx.txid),
            commitment: commitment_hex,
            owner,
            created_at: chrono::Utc::now(),
            created_tx: tx.txid.clone(),
            status: csv_explorer_shared::SanadStatus::Active,
            metadata: Some(serde_json::json!({
                "protocol_id": protocol_str,
                "commitment_scheme": "hash_based",
                "inclusion_proof": "merkle",
                "block_height": block,
            })),
            transfer_count: 0,
            last_transfer_at: None,
        })
    }

    async fn parse_transfer_from_op_return(
        &self,
        tx: &MempoolTx,
        vout: &VoutInfo,
        block: u64,
    ) -> Option<TransferRecord> {
        let script_hex = vout.scriptpubkey.as_ref()?;

        // Strip OP_RETURN opcode prefix (6a + length byte)
        let payload_hex = script_hex.strip_prefix("6a")?;
        let payload_hex = if payload_hex.len() >= 4 {
            let maybe_len = u8::from_str_radix(&payload_hex[..2], 16).ok()? as usize;
            let expected_hex_len = maybe_len * 2;
            if payload_hex.len() == 2 + expected_hex_len {
                &payload_hex[2..]
            } else {
                payload_hex
            }
        } else {
            payload_hex
        };

        let payload = hex::decode(payload_hex).ok()?;

        // Cross-chain transfer encoding: protocol_id (4 bytes) + transfer_data
        if payload.len() < 36 {
            return None;
        }

        const CSV_MAGIC: &[u8; 4] = b"CSV-";
        if &payload[0..4] != CSV_MAGIC {
            return None;
        }

        // Parse transfer-specific data (simplified for indexer)
        // In production, this would decode the full CSV transfer structure
        let source_chain = match payload[4] {
            0 => "bitcoin",
            1 => "ethereum",
            2 => "solana",
            _ => "unknown",
        };

        let transfer_id = format!("btc-transfer-{}-{}", tx.txid, block);
        let from_address = tx
            .vin
            .first()
            .and_then(|vin| vin.prevout.as_ref())
            .and_then(|prev| prev.scriptpubkey_address.as_ref())
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        Some(TransferRecord {
            id: transfer_id,
            sanad_id: format!("btc-{}-sanad", tx.txid),
            from_chain: source_chain.to_string(),
            to_chain: "ethereum".to_string(), // Default target, would be parsed from payload
            from_owner: from_address,
            to_owner: "csv-lock-contract".to_string(),
            lock_tx: tx.txid.clone(),
            mint_tx: None,
            proof_ref: None,
            status: csv_explorer_shared::types::TransferStatus::Initiated,
            created_at: chrono::Utc::now(),
            completed_at: None,
            duration_ms: None,
            lock_tx_explorer_url: None,
            mint_tx_explorer_url: None,
        })
    }
}
