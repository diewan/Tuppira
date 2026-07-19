/// Core explorer types for the Tuppira.
///
/// This module defines all the data types used across the explorer,
/// including sanads, transfers, seals, contracts, and chain information.
///
/// ## Protocol Alignment
///
/// Explorer types reuse stable protocol identifiers where applicable, but event
/// records remain explorer DTOs rather than canonical protocol state.
/// where applicable. The following types are re-exported from the protocol contract:
///
/// - [`ChainId`] — Canonical chain identifiers
/// - [`TransferStatus`] — Canonical transfer lifecycle
/// - [`SyncStatus`] — Indexer sync status
/// - [`ErrorCode`] — Machine-readable error codes
///
/// Explorer-specific types (SanadRecord, SealRecord, etc.) wrap these
/// protocol types with additional metadata for display purposes.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// ===========================================================================
// Re-export canonical protocol types (🔒 STABLE)
// ===========================================================================

// Chain IDs, transfer status, sync status, error codes from protocol contract
pub use csv_hash::chain_id::ChainId;
pub use csv_protocol::version::{ErrorCode, PROTOCOL_VERSION, SyncStatus, TransferStatus};

// ===========================================================================
// Explorer-specific enums
// ===========================================================================

/// Network type for chain configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Testnet,
    Devnet,
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::Mainnet => write!(f, "mainnet"),
            Network::Testnet => write!(f, "testnet"),
            Network::Devnet => write!(f, "devnet"),
        }
    }
}

/// Status of a chain indexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainStatus {
    /// Chain is actively syncing.
    Syncing,
    /// Chain is fully synced and caught up.
    Synced,
    /// Chain indexer is stopped.
    Stopped,
    /// Chain indexer encountered an error.
    Error,
}

impl std::fmt::Display for ChainStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainStatus::Syncing => write!(f, "syncing"),
            ChainStatus::Synced => write!(f, "synced"),
            ChainStatus::Stopped => write!(f, "stopped"),
            ChainStatus::Error => write!(f, "error"),
        }
    }
}

/// Status of a sanad record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SanadStatus {
    /// Sanad is currently active.
    Active,
    /// Sanad has been spent/consumed.
    Spent,
    /// Sanad is pending confirmation.
    Pending,
}

impl std::fmt::Display for SanadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SanadStatus::Active => write!(f, "active"),
            SanadStatus::Spent => write!(f, "spent"),
            SanadStatus::Pending => write!(f, "pending"),
        }
    }
}

/// Type of seal on a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SealType {
    /// UTXO-based seal (Bitcoin).
    Utxo,
    /// Object-based seal (Sui).
    Object,
    /// Resource-based seal (Aptos).
    Resource,
    /// Nullifier-based seal.
    Nullifier,
    /// Account-based seal (Solana).
    Account,
}

impl std::fmt::Display for SealType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SealType::Utxo => write!(f, "utxo"),
            SealType::Object => write!(f, "object"),
            SealType::Resource => write!(f, "resource"),
            SealType::Nullifier => write!(f, "nullifier"),
            SealType::Account => write!(f, "account"),
        }
    }
}

/// Status of a seal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SealStatus {
    /// Seal is available/unused.
    Available,
    /// Seal has been consumed.
    Consumed,
}

impl std::fmt::Display for SealStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SealStatus::Available => write!(f, "available"),
            SealStatus::Consumed => write!(f, "consumed"),
        }
    }
}

/// Type of CSV contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractType {
    /// Nullifier registry contract.
    NullifierRegistry,
    /// State commitment contract.
    StateCommitment,
    /// Sanad registry contract.
    SanadRegistry,
    /// Bridge/transfer contract.
    Bridge,
    /// Generic program/module.
    Other,
}

impl std::fmt::Display for ContractType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContractType::NullifierRegistry => write!(f, "nullifier_registry"),
            ContractType::StateCommitment => write!(f, "state_commitment"),
            ContractType::SanadRegistry => write!(f, "sanad_registry"),
            ContractType::Bridge => write!(f, "bridge"),
            ContractType::Other => write!(f, "other"),
        }
    }
}

/// Status of a deployed contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    /// Contract is active and in use.
    Active,
    /// Contract has been deprecated.
    Deprecated,
    /// Contract had an issue.
    Error,
}

impl std::fmt::Display for ContractStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContractStatus::Active => write!(f, "active"),
            ContractStatus::Deprecated => write!(f, "deprecated"),
            ContractStatus::Error => write!(f, "error"),
        }
    }
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// Information about a blockchain chain being indexed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainInfo {
    /// Unique chain identifier (e.g., "bitcoin", "ethereum").
    pub id: String,
    /// Human-readable chain name.
    pub name: String,
    /// Network type (mainnet, testnet, devnet).
    pub network: Network,
    /// Current status of the chain indexer.
    pub status: ChainStatus,
    /// Latest block number indexed.
    pub latest_block: u64,
    /// Latest slot number (for slot-based chains like Solana), if applicable.
    pub latest_slot: Option<u64>,
    /// RPC endpoint URL for the chain.
    pub rpc_url: String,
    /// Sync lag in blocks behind the chain tip.
    #[serde(default)]
    pub sync_lag: u64,
}

/// A sanad record -- the core entity tracked by the CSV system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanadRecord {
    /// Sanad identifier (hex-encoded sanad_id).
    pub id: String,
    /// Chain that enforces the seal for this sanad.
    pub chain: String,
    /// Seal reference on the chain.
    pub seal_ref: String,
    /// Commitment hash of the sanad.
    pub commitment: String,
    /// Current owner address.
    pub owner: String,
    /// When the sanad was created.
    pub created_at: DateTime<Utc>,
    /// Transaction that created this sanad.
    pub created_tx: String,
    /// Current status of the sanad.
    pub status: SanadStatus,
    /// Optional metadata associated with the sanad.
    pub metadata: Option<JsonValue>,
    /// Number of times this sanad has been transferred.
    pub transfer_count: u64,
    /// Timestamp of the last transfer, if any.
    pub last_transfer_at: Option<DateTime<Utc>>,
}

/// A cross-chain transfer record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRecord {
    /// Transfer identifier.
    pub id: String,
    /// Sanad being transferred.
    pub sanad_id: String,
    /// Source chain.
    pub from_chain: String,
    /// Destination chain.
    pub to_chain: String,
    /// Previous owner (source chain).
    pub from_owner: String,
    /// New owner (destination chain).
    pub to_owner: String,
    /// Source chain lock transaction.
    pub lock_tx: String,
    /// Destination chain mint transaction (if completed).
    pub mint_tx: Option<String>,
    /// Proof reference (if available).
    pub proof_ref: Option<String>,
    /// Current transfer status.
    pub status: TransferStatus,
    /// When the transfer was initiated.
    pub created_at: DateTime<Utc>,
    /// When the transfer completed, if applicable.
    pub completed_at: Option<DateTime<Utc>>,
    /// Duration of the transfer in milliseconds, if completed.
    pub duration_ms: Option<u64>,
    /// Block explorer URL for lock transaction.
    pub lock_tx_explorer_url: Option<String>,
    /// Block explorer URL for mint transaction (if completed).
    pub mint_tx_explorer_url: Option<String>,
}

/// A seal record -- the mechanism that binds a sanad to a chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealRecord {
    /// Seal identifier.
    pub id: String,
    /// Chain where the seal exists.
    pub chain: String,
    /// Type of seal.
    pub seal_type: SealType,
    /// Chain-specific seal reference.
    pub seal_ref: String,
    /// Linked sanad identifier, if known.
    pub sanad_id: Option<String>,
    /// Current seal status.
    pub status: SealStatus,
    /// When the seal was consumed, if applicable.
    pub consumed_at: Option<DateTime<Utc>>,
    /// Transaction that consumed the seal, if applicable.
    pub consumed_tx: Option<String>,
    /// Block height where the seal was created.
    pub block_height: u64,
}

/// A deployed CSV contract/program on a chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvContract {
    /// Contract/program identifier.
    pub id: String,
    /// Chain where the contract is deployed.
    pub chain: String,
    /// Type of contract.
    pub contract_type: ContractType,
    /// Contract address.
    pub address: String,
    /// Deployment transaction.
    pub deployed_tx: String,
    /// When the contract was deployed.
    pub deployed_at: DateTime<Utc>,
    /// Contract version.
    pub version: String,
    /// Current contract status.
    pub status: ContractStatus,
}

// ---------------------------------------------------------------------------
// Filter types
// ---------------------------------------------------------------------------

/// Filter for querying sanads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SanadFilter {
    pub chain: Option<String>,
    pub owner: Option<String>,
    pub status: Option<SanadStatus>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Filter for querying transfers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransferFilter {
    pub sanad_id: Option<String>,
    pub from_chain: Option<String>,
    pub to_chain: Option<String>,
    pub status: Option<TransferStatus>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Filter for querying seals.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SealFilter {
    pub chain: Option<String>,
    pub seal_type: Option<SealType>,
    pub status: Option<SealStatus>,
    pub sanad_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Filter for querying contracts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContractFilter {
    pub chain: Option<String>,
    pub contract_type: Option<ContractType>,
    pub status: Option<ContractStatus>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

// ---------------------------------------------------------------------------
// Stats types
// ---------------------------------------------------------------------------

/// Aggregate statistics for the explorer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TuppiraStats {
    pub total_sanads: u64,
    pub total_transfers: u64,
    pub total_seals: u64,
    pub total_contracts: u64,
    pub sanads_by_chain: Vec<ChainCount>,
    pub transfers_by_chain_pair: Vec<ChainPairCount>,
    pub active_seals_by_chain: Vec<ChainCount>,
    pub transfer_success_rate: f64,
    pub average_transfer_time_ms: Option<u64>,
}

/// Count of items on a specific chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainCount {
    pub chain: String,
    pub count: u64,
}

/// Count of transfers between a pair of chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainPairCount {
    pub from_chain: String,
    pub to_chain: String,
    pub count: u64,
}

// ---------------------------------------------------------------------------
// Indexer status types
// ---------------------------------------------------------------------------

/// Overall indexer status across all chains.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexerStatus {
    pub chains: Vec<ChainInfo>,
    pub total_indexed_blocks: u64,
    pub is_running: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub uptime_seconds: Option<u64>,
}

// ---------------------------------------------------------------------------
// Priority address indexing types
// ---------------------------------------------------------------------------

/// Priority level for address indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PriorityLevel {
    /// High priority - index immediately and frequently
    High,
    /// Normal priority - index in regular cycle
    #[default]
    Normal,
    /// Low priority - index when resources available
    Low,
}

impl std::fmt::Display for PriorityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PriorityLevel::High => write!(f, "high"),
            PriorityLevel::Normal => write!(f, "normal"),
            PriorityLevel::Low => write!(f, "low"),
        }
    }
}

/// A registered address with its priority configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityAddress {
    /// The address to index.
    pub address: String,
    /// Chain this address belongs to.
    pub chain: String,
    /// Network (mainnet/testnet).
    pub network: Network,
    /// Priority level for indexing.
    pub priority: PriorityLevel,
    /// Wallet ID that owns this address.
    pub wallet_id: String,
    /// When this address was registered.
    pub registered_at: DateTime<Utc>,
    /// Last time this address was indexed.
    pub last_indexed_at: Option<DateTime<Utc>>,
    /// Whether this address is actively being indexed.
    pub is_active: bool,
}

/// Status of priority address indexing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriorityIndexingStatus {
    /// Total number of registered addresses.
    pub total_addresses: u64,
    /// Number of addresses currently being indexed.
    pub active_indexing: u64,
    /// Number of addresses fully indexed.
    pub completed_indexing: u64,
    /// Recent indexing activities.
    pub recent_activities: Vec<IndexingActivity>,
}

/// A single indexing activity record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingActivity {
    /// Address that was indexed.
    pub address: String,
    /// Chain that was indexed.
    pub chain: String,
    /// Network (mainnet/testnet).
    pub network: Network,
    /// What was indexed (sanads, seals, transfers).
    pub indexed_type: String,
    /// Number of items indexed.
    pub items_count: u64,
    /// When this indexing occurred.
    pub timestamp: DateTime<Utc>,
    /// Whether indexing was successful.
    pub success: bool,
    /// Error message if indexing failed.
    pub error: Option<String>,
}

// ===========================================================================
// Versioned explorer event DTOs
// ===========================================================================

/// Schema version for explorer event responses. This is intentionally separate
/// from the canonical protocol codec version.
pub const TUPPIRA_EVENT_SCHEMA_VERSION: u16 = 1;

/// Schema version for the wallet-facing explorer feed.  This version is
/// independent of both the protocol codec and the event DTO schema so clients
/// can reject incompatible read-model envelopes without guessing.
pub const WALLET_FEED_SCHEMA_VERSION: u16 = 1;

/// A block identity reported by the indexer.  It is evidence provenance, not
/// an inclusion proof and never grants mutation authority to a wallet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedBlock {
    pub height: u64,
    pub hash: String,
}

/// The indexer's view of its distance from the chain tip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexerFreshness {
    pub indexed_at: DateTime<Utc>,
    pub tip: ObservedBlock,
    pub lag_blocks: u64,
    pub status: IndexerFreshnessStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexerFreshnessStatus {
    Fresh,
    Stale,
    Unknown,
}

impl IndexerFreshnessStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }
}

/// Identifies the untrusted producer and source position of an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedProvenance {
    pub producer: String,
    pub source_cursor: String,
    /// Always `false` for explorer-produced feed messages.  A verifier/runtime
    /// receipt is the only authority that may report cryptographic assurance.
    pub cryptographically_verified: bool,
}

/// A reorg explicitly retracts a prior observation and identifies the block
/// that replaces it.  This is a replacement signal, never an assertion that a
/// wallet should reverse local runtime state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReorgReplacement {
    pub replaced_observation_id: String,
    pub common_ancestor: ObservedBlock,
    pub replacement_block: ObservedBlock,
}

/// Versioned, ordered explorer evidence delivered to a wallet.  It is an
/// untrusted read-model envelope: consumers may use it for discovery and UI
/// projection only, never to consume a seal or complete a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletFeedEnvelope {
    pub schema_version: u16,
    pub protocol_version: String,
    pub observation_id: String,
    pub sequence: u64,
    pub chain_id: ChainId,
    pub network: Network,
    pub observed_block: ObservedBlock,
    pub freshness: IndexerFreshness,
    pub finality: TuppiraFinality,
    pub provenance: FeedProvenance,
    pub event: TuppiraEventDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reorg_replacement: Option<ReorgReplacement>,
}

impl WalletFeedEnvelope {
    /// Validate the envelope before it enters a wallet projection.  This does
    /// not verify chain inclusion or cryptographic proofs.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != WALLET_FEED_SCHEMA_VERSION {
            return Err("unsupported wallet feed schema version".into());
        }
        if self.protocol_version != PROTOCOL_VERSION {
            return Err("unsupported protocol version in wallet feed".into());
        }
        if self.observation_id.is_empty()
            || self.provenance.producer.is_empty()
            || self.provenance.source_cursor.is_empty()
            || self.observed_block.hash.is_empty()
            || self.freshness.tip.hash.is_empty()
        {
            return Err("wallet feed identity or provenance is incomplete".into());
        }
        if self.provenance.cryptographically_verified {
            return Err("explorer feed must not claim cryptographic verification".into());
        }
        if self.chain_id != self.event.chain_id || self.network != self.event.network {
            return Err("wallet feed and event chain identity differ".into());
        }
        if self.observed_block.height != self.event.block_height
            || self.observed_block.hash != self.event.block_hash
        {
            return Err("wallet feed and event block identity differ".into());
        }
        if self.finality != self.event.finality {
            return Err("wallet feed and event finality differ".into());
        }
        if let Some(reorg) = &self.reorg_replacement {
            if reorg.replaced_observation_id.is_empty()
                || reorg.replaced_observation_id == self.observation_id
                || reorg.common_ancestor.hash.is_empty()
                || reorg.replacement_block.hash.is_empty()
            {
                return Err("reorg replacement identity is incomplete".into());
            }
            if reorg.common_ancestor.height >= reorg.replacement_block.height
                || reorg.replacement_block != self.observed_block
            {
                return Err("reorg replacement block relationship is invalid".into());
            }
        }
        self.event.validate()
    }
}

/// An idempotent, ordered wallet feed projection.  It intentionally contains
/// no transfer or seal mutation methods.
#[derive(Debug, Clone, Default)]
pub struct WalletFeedProjection {
    last_sequence: u64,
    envelopes: std::collections::BTreeMap<u64, WalletFeedEnvelope>,
    observation_sequences: std::collections::HashMap<String, u64>,
    finality: std::collections::HashMap<String, TuppiraFinality>,
}

impl WalletFeedProjection {
    /// Applies an observation exactly once. Replays return `Ok(false)`;
    /// callers can reconnect from `last_sequence` without double delivery.
    pub fn apply(&mut self, envelope: WalletFeedEnvelope) -> Result<bool, String> {
        envelope.validate()?;
        if let Some(sequence) = self.observation_sequences.get(&envelope.observation_id) {
            return if *sequence == envelope.sequence {
                Ok(false)
            } else {
                Err("observation id was reused with a different sequence".into())
            };
        }
        if envelope.sequence != self.last_sequence.saturating_add(1) {
            return Err("wallet feed sequence is not contiguous".into());
        }
        if let Some(reorg) = &envelope.reorg_replacement {
            let replaced_sequence = self
                .observation_sequences
                .get(&reorg.replaced_observation_id)
                .ok_or_else(|| "reorg replacement references an unknown observation".to_string())?;
            let replaced = self
                .envelopes
                .get(replaced_sequence)
                .ok_or_else(|| "reorg replacement observation is unavailable".to_string())?;
            if replaced.chain_id != envelope.chain_id
                || replaced.network != envelope.network
                || reorg.common_ancestor.height >= replaced.observed_block.height
            {
                return Err("reorg replacement does not match the replaced observation".into());
            }
        }
        let event_key = format!(
            "{}:{}:{}",
            envelope.chain_id.as_str(),
            envelope.event.transaction_id,
            envelope.event.log_index
        );
        if envelope.reorg_replacement.is_none()
            && let Some(previous) = self.finality.get(&event_key)
            && finality_rank(envelope.finality) < finality_rank(*previous)
        {
            return Err("finality may not regress without an explicit reorg replacement".into());
        }
        self.last_sequence = envelope.sequence;
        self.observation_sequences
            .insert(envelope.observation_id.clone(), envelope.sequence);
        self.finality.insert(event_key, envelope.finality);
        self.envelopes.insert(envelope.sequence, envelope);
        Ok(true)
    }

    pub fn since(&self, sequence: u64) -> Vec<WalletFeedEnvelope> {
        self.envelopes
            .range((sequence.saturating_add(1))..)
            .map(|(_, value)| value.clone())
            .collect()
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
}

fn finality_rank(finality: TuppiraFinality) -> u8 {
    match finality {
        TuppiraFinality::Observed => 0,
        TuppiraFinality::ReorgPossible => 1,
        TuppiraFinality::Finalized => 2,
    }
}

/// An untrusted, versioned event projection produced only after validating raw
/// chain data. It must never be treated as a canonical protocol event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuppiraEventDto {
    pub schema_version: u16,
    pub chain_id: ChainId,
    pub network: Network,
    pub contract: String,
    pub event_type: TuppiraEventType,
    pub block_height: u64,
    pub block_hash: String,
    pub transaction_id: String,
    pub log_index: u64,
    pub finality: TuppiraFinality,
    pub payload: TuppiraEventPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuppiraEventType {
    SanadCreated,
    SealConsumed,
    TransferSent,
    TransferMaterialized,
}

/// Send and materialize are deliberately distinct event kinds. A destination
/// observation cannot be inferred from a source-chain send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TuppiraEventPayload {
    SanadCreated {
        sanad_id: String,
        commitment: String,
        owner: String,
    },
    SealConsumed {
        sanad_id: String,
        nullifier: String,
    },
    TransferSent {
        transfer_id: String,
        sanad_id: String,
        destination_chain: ChainId,
        destination_owner: String,
    },
    TransferMaterialized {
        transfer_id: String,
        source_chain: ChainId,
        source_transaction_id: String,
        sanad_id: String,
        owner: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TuppiraFinality {
    Observed,
    ReorgPossible,
    Finalized,
}

impl TuppiraFinality {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::ReorgPossible => "reorg_possible",
            Self::Finalized => "finalized",
        }
    }
}

impl TuppiraEventDto {
    /// Rejects unknown schema versions and records whose identity is incomplete.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != TUPPIRA_EVENT_SCHEMA_VERSION {
            return Err("unsupported explorer event schema version".into());
        }
        if self.chain_id.as_str().is_empty()
            || self.contract.is_empty()
            || self.block_hash.is_empty()
            || self.transaction_id.is_empty()
        {
            return Err("event identity is incomplete".into());
        }
        match (&self.event_type, &self.payload) {
            (TuppiraEventType::TransferSent, TuppiraEventPayload::TransferSent { .. })
            | (
                TuppiraEventType::TransferMaterialized,
                TuppiraEventPayload::TransferMaterialized { .. },
            )
            | (TuppiraEventType::SanadCreated, TuppiraEventPayload::SanadCreated { .. })
            | (TuppiraEventType::SealConsumed, TuppiraEventPayload::SealConsumed { .. }) => Ok(()),
            _ => Err("event type and payload do not match".into()),
        }
    }
}

/// Chain-specific adapters must establish these expectations from trusted
/// deployment configuration before converting a wire event into an explorer
/// DTO. Topic strings are transport values only; they are not protocol IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDecodeContext {
    pub chain_id: ChainId,
    pub network: Network,
    pub contract: String,
    pub topic: String,
    pub event_type: TuppiraEventType,
}

/// Minimal wire evidence retained during decoding. The indexer must verify
/// chain and network at the RPC boundary before supplying this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawContractEvent {
    pub chain_id: ChainId,
    pub network: Network,
    pub contract: String,
    pub topic: String,
    pub indexed_fields: Vec<String>,
}

/// Validates a raw contract event before a chain adapter decodes its payload.
/// This deliberately does not hash topics: topic selection is chain ABI
/// machinery, never a canonical protocol identity operation.
pub fn validate_raw_contract_event(
    raw: &RawContractEvent,
    expected: &EventDecodeContext,
) -> Result<(), String> {
    if raw.chain_id != expected.chain_id || raw.network != expected.network {
        return Err("foreign chain or network event".into());
    }
    if !same_hex(&raw.contract, &expected.contract) || !is_contract_width(&raw.contract) {
        return Err("foreign or malformed contract address".into());
    }
    if !same_hex(&raw.topic, &expected.topic) || !is_hex_width(&raw.topic, 32) {
        return Err("unknown or malformed event topic".into());
    }
    for field in &raw.indexed_fields {
        if !is_hex_width(field, 32) {
            return Err("malformed indexed event field width".into());
        }
    }
    Ok(())
}

fn same_hex(left: &str, right: &str) -> bool {
    left.strip_prefix("0x")
        .unwrap_or(left)
        .eq_ignore_ascii_case(right.strip_prefix("0x").unwrap_or(right))
}
fn is_contract_width(value: &str) -> bool {
    is_hex_width(value, 20) || is_hex_width(value, 32)
}
fn is_hex_width(value: &str, bytes: usize) -> bool {
    let value = value.strip_prefix("0x").unwrap_or(value);
    value.len() == bytes * 2 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod event_tests {
    use super::*;
    fn event(kind: TuppiraEventType, payload: TuppiraEventPayload) -> TuppiraEventDto {
        TuppiraEventDto {
            schema_version: TUPPIRA_EVENT_SCHEMA_VERSION,
            chain_id: ChainId::new("ethereum"),
            network: Network::Mainnet,
            contract: "0x1234".into(),
            event_type: kind,
            block_height: 1,
            block_hash: "0x01".into(),
            transaction_id: "0x02".into(),
            log_index: 0,
            finality: TuppiraFinality::Observed,
            payload,
        }
    }
    #[test]
    fn validates_a_versioned_send_event() {
        assert!(
            event(
                TuppiraEventType::TransferSent,
                TuppiraEventPayload::TransferSent {
                    transfer_id: "t".into(),
                    sanad_id: "s".into(),
                    destination_chain: ChainId::new("solana"),
                    destination_owner: "o".into()
                }
            )
            .validate()
            .is_ok()
        );
    }
    #[test]
    fn rejects_unknown_versions() {
        let mut value = event(
            TuppiraEventType::SealConsumed,
            TuppiraEventPayload::SealConsumed {
                sanad_id: "s".into(),
                nullifier: "n".into(),
            },
        );
        value.schema_version = 2;
        assert!(value.validate().is_err());
    }
    #[test]
    fn rejects_conflated_lifecycle() {
        assert!(
            event(
                TuppiraEventType::TransferSent,
                TuppiraEventPayload::TransferMaterialized {
                    transfer_id: "t".into(),
                    source_chain: ChainId::new("ethereum"),
                    source_transaction_id: "x".into(),
                    sanad_id: "s".into(),
                    owner: "o".into()
                }
            )
            .validate()
            .is_err()
        );
    }
    fn feed(sequence: u64) -> WalletFeedEnvelope {
        let event = event(
            TuppiraEventType::SealConsumed,
            TuppiraEventPayload::SealConsumed {
                sanad_id: "s".into(),
                nullifier: "n".into(),
            },
        );
        WalletFeedEnvelope {
            schema_version: WALLET_FEED_SCHEMA_VERSION,
            protocol_version: PROTOCOL_VERSION.into(),
            observation_id: format!("observation-{sequence}"),
            sequence,
            chain_id: event.chain_id.clone(),
            network: event.network,
            observed_block: ObservedBlock {
                height: event.block_height,
                hash: event.block_hash.clone(),
            },
            freshness: IndexerFreshness {
                indexed_at: Utc::now(),
                tip: ObservedBlock {
                    height: event.block_height,
                    hash: "0xtip".into(),
                },
                lag_blocks: 0,
                status: IndexerFreshnessStatus::Fresh,
            },
            finality: TuppiraFinality::Observed,
            provenance: FeedProvenance {
                producer: "tuppira-indexer".into(),
                source_cursor: format!("cursor-{sequence}"),
                cryptographically_verified: false,
            },
            event,
            reorg_replacement: None,
        }
    }
    #[test]
    fn reconnect_delivery_is_idempotent_and_ordered() {
        let mut projection = WalletFeedProjection::default();
        let first = feed(1);
        assert_eq!(projection.apply(first.clone()), Ok(true));
        assert_eq!(projection.apply(first), Ok(false));
        assert_eq!(projection.last_sequence(), 1);
        assert_eq!(projection.since(0).len(), 1);
    }
    #[test]
    fn rejects_malformed_or_verification_claiming_feed() {
        let mut malformed = feed(1);
        malformed.provenance.cryptographically_verified = true;
        assert!(malformed.validate().is_err());
        let mut missing_block = feed(1);
        missing_block.observed_block.hash.clear();
        assert!(missing_block.validate().is_err());
        let mut inconsistent_finality = feed(1);
        inconsistent_finality.finality = TuppiraFinality::Finalized;
        assert!(inconsistent_finality.validate().is_err());
    }
    #[test]
    fn permits_explicit_reorg_replacement_but_not_silent_finality_regression() {
        let mut projection = WalletFeedProjection::default();
        let mut finalized = feed(1);
        finalized.finality = TuppiraFinality::Finalized;
        finalized.event.finality = TuppiraFinality::Finalized;
        assert_eq!(projection.apply(finalized), Ok(true));

        let regressing = feed(2);
        assert!(projection.apply(regressing).is_err());

        let mut replacement = feed(2);
        replacement.reorg_replacement = Some(ReorgReplacement {
            replaced_observation_id: "observation-1".into(),
            common_ancestor: ObservedBlock {
                height: 0,
                hash: "0xancestor".into(),
            },
            replacement_block: replacement.observed_block.clone(),
        });
        assert_eq!(projection.apply(replacement), Ok(true));
    }
    #[test]
    fn rejects_reorgs_that_do_not_replace_a_prior_observation() {
        let mut projection = WalletFeedProjection::default();
        let mut replacement = feed(1);
        replacement.reorg_replacement = Some(ReorgReplacement {
            replaced_observation_id: "missing-observation".into(),
            common_ancestor: ObservedBlock {
                height: 0,
                hash: "0xancestor".into(),
            },
            replacement_block: replacement.observed_block.clone(),
        });
        assert!(projection.apply(replacement).is_err());
    }
    #[test]
    fn stale_indexer_is_explicitly_preserved() {
        let mut observation = feed(1);
        observation.freshness.status = IndexerFreshnessStatus::Stale;
        observation.freshness.lag_blocks = 12;
        assert!(observation.validate().is_ok());
        assert_eq!(observation.freshness.status, IndexerFreshnessStatus::Stale);
    }
    fn decode_context() -> EventDecodeContext {
        EventDecodeContext {
            chain_id: ChainId::new("ethereum"),
            network: Network::Mainnet,
            contract: format!("0x{}", "11".repeat(20)),
            topic: format!("0x{}", "22".repeat(32)),
            event_type: TuppiraEventType::SealConsumed,
        }
    }
    #[test]
    fn rejects_foreign_contract_and_malformed_fields() {
        let expected = decode_context();
        let foreign = RawContractEvent {
            chain_id: ChainId::new("ethereum"),
            network: Network::Mainnet,
            contract: format!("0x{}", "33".repeat(20)),
            topic: expected.topic.clone(),
            indexed_fields: vec![format!("0x{}", "44".repeat(32))],
        };
        assert!(validate_raw_contract_event(&foreign, &expected).is_err());
        let malformed = RawContractEvent {
            contract: expected.contract.clone(),
            topic: expected.topic.clone(),
            indexed_fields: vec!["0x01".into()],
            chain_id: expected.chain_id.clone(),
            network: expected.network,
        };
        assert!(validate_raw_contract_event(&malformed, &expected).is_err());
    }
}
