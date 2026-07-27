/// GraphQL type mappings and input types for the Tuppira API.
use async_graphql::*;

/// Versioned consumer read model. It omits raw payload and custody details.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ObservationGql")]
pub struct ObservationProjectionV1 {
    pub observation_id: String,
    pub source_id: String,
    pub source_event_id: String,
    pub source_event_type: String,
    pub subject_refs: Vec<String>,
    pub asserted_event_time: Option<i64>,
    pub observed_at: i64,
    pub normalized_profile_id: String,
    pub normalized_profile_version: i32,
    pub normalized_payload_digest: String,
    pub authenticity_material_refs: Vec<String>,
    pub collection_run_id: String,
    pub supersedes: Option<String>,
    pub retraction_status: String,
    pub visibility_scope: String,
}

impl From<tuppira_shared::ObservationRecord> for ObservationProjectionV1 {
    fn from(value: tuppira_shared::ObservationRecord) -> Self {
        let visibility_scope = match value.tenant_visibility {
            tuppira_shared::TenantVisibility::Public => "public",
            tuppira_shared::TenantVisibility::Tenant { .. } => "tenant",
        }
        .to_string();
        Self {
            observation_id: value.observation_id,
            source_id: value.source_id,
            source_event_id: value.source_event_id,
            source_event_type: value.source_event_type,
            subject_refs: value.subject_refs,
            asserted_event_time: value.asserted_event_time.map(|v| v as i64),
            observed_at: value.observed_at as i64,
            normalized_profile_id: value.normalized_profile_id,
            normalized_profile_version: i32::from(value.normalized_profile_version),
            normalized_payload_digest: hex::encode(value.normalized_payload_digest),
            authenticity_material_refs: value.authenticity_material_refs,
            collection_run_id: value.collection_run_id,
            supersedes: value.supersedes,
            retraction_status: format!("{:?}", value.retraction_status).to_lowercase(),
            visibility_scope,
        }
    }
}

/// Versioned consumer projection of collector liveness, not source truth.
#[derive(SimpleObject, Clone)]
#[graphql(name = "SourceHealthGql")]
pub struct SourceHealthProjectionV1 {
    pub source_id: String,
    pub connector_kind: String,
    pub display_name: String,
    pub last_run_started_at: Option<i64>,
    pub last_run_completed_at: Option<i64>,
    pub cursor_observed_at: Option<i64>,
}

impl From<tuppira_storage::repositories::observations::SourceHealthProjection>
    for SourceHealthProjectionV1
{
    fn from(value: tuppira_storage::repositories::observations::SourceHealthProjection) -> Self {
        Self {
            source_id: value.source_id,
            connector_kind: value.connector_kind,
            display_name: value.display_name,
            last_run_started_at: value.last_run_started_at.map(|v| v as i64),
            last_run_completed_at: value.last_run_completed_at.map(|v| v as i64),
            cursor_observed_at: value.cursor_observed_at.map(|v| v as i64),
        }
    }
}
// ---------------------------------------------------------------------------
// V2 source-closure read models (TUP-NE-002)
// ---------------------------------------------------------------------------

/// Versioned consumer read model for one recorded source-closure observation.
///
/// The typed fields are query keys. The closure statement itself is `payload`,
/// carried in the profile's own versioned wire shape and named by `profile_id`
/// and `profile_version`. Re-typing every facet here would create a second
/// definition of the projection that could drift from the one the observation's
/// digest commits to, and a drifted copy is exactly how an indeterminate facet
/// turns into an apparent pass.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureObservationGql")]
pub struct ClosureObservationProjectionV1 {
    pub observation_id: String,
    pub observed_at: i64,
    /// Retraction of the observation *record* — the collector withdrew it.
    /// Distinct from the source revoking the closure, which is inside `payload`.
    pub record_retraction_status: String,
    pub profile_id: String,
    pub profile_version: i32,
    pub chain_id: String,
    pub network_id: String,
    pub closure_kind: String,
    pub closure_identity_hex: String,
    pub consumed_transition_id_hex: String,
    pub consumed_output_index: i64,
    pub successor_commitment_hex: String,
    /// Every state this one observation establishes, as a set and never a badge.
    pub established_states: Vec<String>,
    /// The projection exactly as stored, under the profile named above.
    pub payload: JsonValueScalar,
    /// The way back to the chain event this closure was normalized from.
    /// Absent for a closure relayed by a source that is not a chain.
    pub chain_evidence: Option<ChainClosureEvidenceGql>,
}

/// Chain-native locators addressing the evidence behind a normalized closure.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ChainClosureEvidenceGql")]
pub struct ChainClosureEvidenceGql {
    pub schema_version: i32,
    /// The native event family, equal to the closure's `closure_kind`.
    pub native_event_kind: String,
    /// Locators addressing the exact chain evidence, in the recorded order.
    pub evidence_refs: Vec<String>,
    /// Digest of the exact source bytes the projection was normalized from.
    pub raw_event_digest_hex: String,
}

impl From<tuppira_shared::ChainClosureEvidenceRecord> for ChainClosureEvidenceGql {
    fn from(value: tuppira_shared::ChainClosureEvidenceRecord) -> Self {
        Self {
            schema_version: i32::from(value.schema_version),
            native_event_kind: value.native_event_kind,
            evidence_refs: value.evidence_refs,
            raw_event_digest_hex: hex::encode(value.raw_event_digest),
        }
    }
}

impl From<tuppira_shared::RecordedSourceClosureObservationV1> for ClosureObservationProjectionV1 {
    fn from(value: tuppira_shared::RecordedSourceClosureObservationV1) -> Self {
        let projection = value.projection;
        Self {
            observation_id: value.observation_id,
            observed_at: value.observed_at as i64,
            record_retraction_status: state_name(&value.record_retraction_status),
            profile_id: tuppira_shared::SOURCE_CLOSURE_OBSERVATION_PROFILE_ID.to_string(),
            profile_version: i32::from(projection.schema_version),
            chain_id: projection.chain_id.clone(),
            network_id: projection.network_id.clone(),
            closure_kind: projection.closure_identity.closure_kind.clone(),
            closure_identity_hex: projection.closure_identity.closure_identity_hex.clone(),
            consumed_transition_id_hex: projection.consumed_state.transition_id_hex.clone(),
            consumed_output_index: i64::from(projection.consumed_state.output_index),
            successor_commitment_hex: projection
                .closure_identity
                .successor_commitment_hex
                .clone(),
            established_states: projection
                .established_states()
                .iter()
                .map(state_name)
                .collect(),
            payload: JsonValueScalar(
                serde_json::to_value(&projection).unwrap_or(JsonValue::Null),
            ),
            // Set by the single-observation read, which fetches evidence
            // alongside the projection. The subject account leaves it absent
            // and a consumer reaches it by observation identifier.
            chain_evidence: None,
        }
    }
}

/// Versioned consumer read model for a subject's closure account.
///
/// `generation` is the discriminant a consumer branches on. `pre_closure` means
/// no closure observation is recorded — which is what every Sanad, transfer, and
/// seal indexed before the closure profile reports — and it carries reasons
/// rather than an empty closure. It is never derived from a V1 explorer status.
#[derive(SimpleObject, Clone)]
#[graphql(name = "SubjectClosureGql")]
pub struct SubjectClosureProjectionV1Gql {
    pub schema_version: i32,
    pub profile_id: String,
    pub subject_ref: String,
    /// `source_closure_v2` or `pre_closure`.
    pub generation: String,
    /// Why no closure statement exists. Empty unless `generation` is `pre_closure`.
    pub pre_closure_reasons: Vec<String>,
    /// Recorded observations, newest acquisition first. Empty for `pre_closure`.
    pub observations: Vec<ClosureObservationProjectionV1>,
    /// The union of what those observations establish; `["unknown"]` when none.
    pub established_states: Vec<String>,
}

impl From<tuppira_shared::SubjectClosureProjectionV1> for SubjectClosureProjectionV1Gql {
    fn from(value: tuppira_shared::SubjectClosureProjectionV1) -> Self {
        let established_states = value.established_states().iter().map(state_name).collect();
        let (generation, pre_closure_reasons, observations) = match value.closure_generation {
            tuppira_shared::ClosureProfileGeneration::PreClosure { reasons } => {
                ("pre_closure", reasons, Vec::new())
            }
            tuppira_shared::ClosureProfileGeneration::SourceClosureV2 { observations } => (
                "source_closure_v2",
                Vec::new(),
                observations.into_iter().map(Into::into).collect(),
            ),
        };
        Self {
            schema_version: i32::from(value.schema_version),
            profile_id: tuppira_shared::SUBJECT_CLOSURE_PROJECTION_PROFILE_ID.to_string(),
            subject_ref: value.subject_ref,
            generation: generation.to_string(),
            pre_closure_reasons,
            observations,
            established_states,
        }
    }
}

/// Render an observation-plane enum using its own serde name.
///
/// The wire name comes from the type's `snake_case` serde attribute rather than
/// a second table here, so a state added or renamed upstream cannot silently
/// keep its old spelling on this boundary.
fn state_name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(JsonValue::String(name)) => name,
        _ => "unknown".to_string(),
    }
}

use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Scalar types
// ---------------------------------------------------------------------------

/// DateTime scalar for GraphQL.
#[derive(Clone, Debug)]
pub struct DateTimeScalar(DateTime<Utc>);

#[Scalar]
impl ScalarType for DateTimeScalar {
    fn parse(value: Value) -> InputValueResult<Self> {
        if let Value::String(s) = &value {
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map(DateTimeScalar)
                .map_err(|e| InputValueError::custom(format!("Invalid DateTime: {}", e)))
        } else {
            Err(InputValueError::expected_type(value))
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.to_rfc3339())
    }
}

/// JSON Value scalar.
#[derive(Clone, Debug)]
pub struct JsonValueScalar(JsonValue);

#[Scalar]
impl ScalarType for JsonValueScalar {
    fn parse(value: Value) -> InputValueResult<Self> {
        let json_str = match &value {
            Value::String(s) => s.clone(),
            _ => value.to_string(),
        };
        serde_json::from_str(&json_str)
            .map(JsonValueScalar)
            .map_err(|e| InputValueError::custom(e.to_string()))
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.to_string())
    }
}

/// Wallet-feed view of an explorer observation.  `cryptographically_verified`
/// is intentionally not exposed as a success flag: feed data is always an
/// untrusted indexed report and wallets must use runtime/verifier receipts for
/// authority.
#[derive(SimpleObject, Clone)]
pub struct WalletFeedEnvelopeGql {
    pub schema_version: i32,
    pub protocol_version: String,
    pub observation_id: String,
    pub sequence: i64,
    pub chain: String,
    pub network: String,
    pub block_height: i64,
    pub block_hash: String,
    pub tip_height: i64,
    pub tip_hash: String,
    pub indexed_at: DateTimeScalar,
    pub lag_blocks: i64,
    pub freshness: String,
    pub finality: String,
    pub producer: String,
    pub source_cursor: String,
    pub reorg_replaces_observation_id: Option<String>,
}

impl From<tuppira_shared::WalletFeedEnvelope> for WalletFeedEnvelopeGql {
    fn from(value: tuppira_shared::WalletFeedEnvelope) -> Self {
        Self {
            schema_version: value.schema_version as i32,
            protocol_version: value.protocol_version,
            observation_id: value.observation_id,
            sequence: value.sequence as i64,
            chain: value.chain_id.as_str().to_owned(),
            network: value.network.to_string(),
            block_height: value.observed_block.height as i64,
            block_hash: value.observed_block.hash,
            tip_height: value.freshness.tip.height as i64,
            tip_hash: value.freshness.tip.hash,
            indexed_at: DateTimeScalar(value.freshness.indexed_at),
            lag_blocks: value.freshness.lag_blocks as i64,
            freshness: value.freshness.status.as_str().to_owned(),
            finality: value.finality.as_str().to_owned(),
            producer: value.provenance.producer,
            source_cursor: value.provenance.source_cursor,
            reorg_replaces_observation_id: value
                .reorg_replacement
                .map(|item| item.replaced_observation_id),
        }
    }
}

// ---------------------------------------------------------------------------
// GraphQL entity types
// ---------------------------------------------------------------------------

/// GraphQL Sanad type.
#[derive(SimpleObject)]
pub struct Sanad {
    pub id: String,
    pub chain: String,
    pub seal_ref: String,
    pub commitment: String,
    pub owner: String,
    pub created_at: DateTimeScalar,
    pub created_tx: String,
    pub status: String,
    pub metadata: Option<JsonValueScalar>,
    pub transfer_count: i64,
    pub last_transfer_at: Option<DateTimeScalar>,
}

impl From<tuppira_shared::SanadRecord> for Sanad {
    fn from(r: tuppira_shared::SanadRecord) -> Self {
        Self {
            id: r.id,
            chain: r.chain,
            seal_ref: r.seal_ref,
            commitment: r.commitment,
            owner: r.owner,
            created_at: DateTimeScalar(r.created_at),
            created_tx: r.created_tx,
            status: r.status.to_string(),
            metadata: r.metadata.map(JsonValueScalar),
            transfer_count: r.transfer_count as i64,
            last_transfer_at: r.last_transfer_at.map(DateTimeScalar),
        }
    }
}

/// GraphQL Transfer type.
#[derive(SimpleObject)]
pub struct Transfer {
    pub id: String,
    pub sanad_id: String,
    pub from_chain: String,
    pub to_chain: String,
    pub from_owner: String,
    pub to_owner: String,
    pub lock_tx: String,
    pub mint_tx: Option<String>,
    pub proof_ref: Option<String>,
    pub status: String,
    pub created_at: DateTimeScalar,
    pub completed_at: Option<DateTimeScalar>,
    pub duration_ms: Option<i64>,
}

impl From<tuppira_shared::TransferRecord> for Transfer {
    fn from(t: tuppira_shared::TransferRecord) -> Self {
        Self {
            id: t.id,
            sanad_id: t.sanad_id,
            from_chain: t.from_chain,
            to_chain: t.to_chain,
            from_owner: t.from_owner,
            to_owner: t.to_owner,
            lock_tx: t.lock_tx,
            mint_tx: t.mint_tx,
            proof_ref: t.proof_ref,
            status: t.status.to_string(),
            created_at: DateTimeScalar(t.created_at),
            completed_at: t.completed_at.map(DateTimeScalar),
            duration_ms: t.duration_ms.map(|v| v as i64),
        }
    }
}

/// GraphQL Seal type.
#[derive(SimpleObject)]
pub struct Seal {
    pub id: String,
    pub chain: String,
    pub seal_type: String,
    pub seal_ref: String,
    pub sanad_id: Option<String>,
    pub status: String,
    pub consumed_at: Option<DateTimeScalar>,
    pub consumed_tx: Option<String>,
    pub block_height: i64,
}

impl From<tuppira_shared::SealRecord> for Seal {
    fn from(s: tuppira_shared::SealRecord) -> Self {
        Self {
            id: s.id,
            chain: s.chain,
            seal_type: s.seal_type.to_string(),
            seal_ref: s.seal_ref,
            sanad_id: s.sanad_id,
            status: s.status.to_string(),
            consumed_at: s.consumed_at.map(DateTimeScalar),
            consumed_tx: s.consumed_tx,
            block_height: s.block_height as i64,
        }
    }
}

/// GraphQL Contract type.
#[derive(SimpleObject)]
pub struct CsvContractGql {
    pub id: String,
    pub chain: String,
    pub contract_type: String,
    pub address: String,
    pub deployed_tx: String,
    pub deployed_at: DateTimeScalar,
    pub version: String,
    pub status: String,
}

impl From<tuppira_shared::CsvContract> for CsvContractGql {
    fn from(c: tuppira_shared::CsvContract) -> Self {
        Self {
            id: c.id,
            chain: c.chain,
            contract_type: c.contract_type.to_string(),
            address: c.address,
            deployed_tx: c.deployed_tx,
            deployed_at: DateTimeScalar(c.deployed_at),
            version: c.version,
            status: c.status.to_string(),
        }
    }
}

/// GraphQL ChainInfo type.
#[derive(SimpleObject)]
pub struct ChainInfoGql {
    pub id: String,
    pub name: String,
    pub network: String,
    pub status: String,
    pub latest_block: i64,
    pub latest_slot: Option<i64>,
    pub rpc_url: String,
    pub sync_lag: i64,
}

impl From<tuppira_shared::ChainInfo> for ChainInfoGql {
    fn from(c: tuppira_shared::ChainInfo) -> Self {
        Self {
            id: c.id,
            name: c.name,
            network: c.network.to_string(),
            status: c.status.to_string(),
            latest_block: c.latest_block as i64,
            latest_slot: c.latest_slot.map(|v| v as i64),
            rpc_url: c.rpc_url,
            sync_lag: c.sync_lag as i64,
        }
    }
}

/// GraphQL Stats type.
#[derive(SimpleObject)]
pub struct Stats {
    pub total_sanads: i64,
    pub total_transfers: i64,
    pub total_seals: i64,
    pub total_contracts: i64,
    pub transfer_success_rate: f64,
    pub average_transfer_time_ms: Option<i64>,
}

impl From<tuppira_shared::TuppiraStats> for Stats {
    fn from(s: tuppira_shared::TuppiraStats) -> Self {
        Self {
            total_sanads: s.total_sanads as i64,
            total_transfers: s.total_transfers as i64,
            total_seals: s.total_seals as i64,
            total_contracts: s.total_contracts as i64,
            transfer_success_rate: s.transfer_success_rate,
            average_transfer_time_ms: s.average_transfer_time_ms.map(|v| v as i64),
        }
    }
}

/// Chain count type.
#[derive(SimpleObject)]
pub struct ChainCount {
    pub chain: String,
    pub count: i64,
}

/// Chain pair count type.
#[derive(SimpleObject)]
pub struct ChainPairCount {
    pub from_chain: String,
    pub to_chain: String,
    pub count: i64,
}

// ---------------------------------------------------------------------------
// Pagination types
// ---------------------------------------------------------------------------

/// Connection for paginated Sanad results.
#[derive(SimpleObject)]
pub struct SanadConnection {
    pub edges: Vec<SanadEdge>,
    pub page_info: PageInfo,
    pub total_count: i64,
}

#[derive(SimpleObject)]
pub struct SanadEdge {
    pub node: Sanad,
    pub cursor: String,
}

/// Connection for paginated Transfer results.
#[derive(SimpleObject)]
pub struct TransferConnection {
    pub edges: Vec<TransferEdge>,
    pub page_info: PageInfo,
    pub total_count: i64,
}

#[derive(SimpleObject)]
pub struct TransferEdge {
    pub node: Transfer,
    pub cursor: String,
}

/// Connection for paginated Seal results.
#[derive(SimpleObject)]
pub struct SealConnection {
    pub edges: Vec<SealEdge>,
    pub page_info: PageInfo,
    pub total_count: i64,
}

#[derive(SimpleObject)]
pub struct SealEdge {
    pub node: Seal,
    pub cursor: String,
}

/// Connection for paginated Contract results.
#[derive(SimpleObject)]
pub struct ContractConnection {
    pub edges: Vec<ContractEdge>,
    pub page_info: PageInfo,
    pub total_count: i64,
}

#[derive(SimpleObject)]
pub struct ContractEdge {
    pub node: CsvContractGql,
    pub cursor: String,
}

/// Standard pagination info.
#[derive(SimpleObject)]
pub struct PageInfo {
    pub has_next_page: bool,
    pub has_previous_page: bool,
    pub start_cursor: Option<String>,
    pub end_cursor: Option<String>,
}

impl PageInfo {
    pub fn new(
        has_next_page: bool,
        has_previous_page: bool,
        start_cursor: Option<String>,
        end_cursor: Option<String>,
    ) -> Self {
        Self {
            has_next_page,
            has_previous_page,
            start_cursor,
            end_cursor,
        }
    }
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Input type for filtering sanads.
#[derive(Default, InputObject)]
pub struct SanadFilterInput {
    pub chain: Option<String>,
    pub owner: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// Input type for filtering transfers.
#[derive(Default, InputObject)]
pub struct TransferFilterInput {
    pub sanad_id: Option<String>,
    pub from_chain: Option<String>,
    pub to_chain: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// Input type for filtering seals.
#[derive(Default, InputObject)]
pub struct SealFilterInput {
    pub chain: Option<String>,
    pub seal_type: Option<String>,
    pub status: Option<String>,
    pub sanad_id: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// Input type for filtering contracts.
#[derive(Default, InputObject)]
pub struct ContractFilterInput {
    pub chain: Option<String>,
    pub contract_type: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

// ---------------------------------------------------------------------------
// Advanced commitment and proof types
// ---------------------------------------------------------------------------

/// GraphQL EnhancedSanad type with commitment metadata.
#[derive(SimpleObject)]
pub struct EnhancedSanad {
    pub id: String,
    pub chain: String,
    pub seal_ref: String,
    pub commitment: String,
    pub owner: String,
    pub created_at: DateTimeScalar,
    pub created_tx: String,
    pub status: String,
    pub commitment_scheme: String,
    pub commitment_version: i32,
    pub protocol_id: String,
    pub mpc_root: Option<String>,
    pub domain_separator: Option<String>,
    pub inclusion_proof_type: String,
    pub finality_proof_type: String,
    pub proof_size_bytes: Option<i64>,
    pub confirmations: Option<i64>,
}

impl From<tuppira_shared::EnhancedSanadRecord> for EnhancedSanad {
    fn from(r: tuppira_shared::EnhancedSanadRecord) -> Self {
        Self {
            id: r.id,
            chain: r.chain,
            seal_ref: r.seal_ref,
            commitment: r.commitment,
            owner: r.owner,
            created_at: DateTimeScalar(r.created_at),
            created_tx: r.created_tx,
            status: r.status,
            commitment_scheme: format!("{:?}", r.commitment_scheme),
            commitment_version: r.commitment_version as i32,
            protocol_id: r.protocol_id,
            mpc_root: r.mpc_root,
            domain_separator: r.domain_separator,
            inclusion_proof_type: format!("{:?}", r.inclusion_proof_type),
            finality_proof_type: format!("{:?}", r.finality_proof_type),
            proof_size_bytes: r.proof_size_bytes.map(|v| v as i64),
            confirmations: r.confirmations.map(|v| v as i64),
        }
    }
}

/// GraphQL EnhancedSeal type with proof metadata.
#[derive(SimpleObject)]
pub struct EnhancedSeal {
    pub id: String,
    pub chain: String,
    pub seal_type: String,
    pub seal_ref: String,
    pub sanad_id: Option<String>,
    pub status: String,
    pub consumed_at: Option<DateTimeScalar>,
    pub consumed_tx: Option<String>,
    pub block_height: i64,
    pub seal_proof_type: String,
    pub seal_proof_verified: Option<bool>,
}

impl From<tuppira_shared::EnhancedSealRecord> for EnhancedSeal {
    fn from(s: tuppira_shared::EnhancedSealRecord) -> Self {
        Self {
            id: s.id,
            chain: s.chain,
            seal_type: s.seal_type,
            seal_ref: s.seal_ref,
            sanad_id: s.sanad_id,
            status: s.status,
            consumed_at: s.consumed_at.map(DateTimeScalar),
            consumed_tx: s.consumed_tx,
            block_height: s.block_height as i64,
            seal_proof_type: s.seal_proof_type,
            seal_proof_verified: s.seal_proof_verified,
        }
    }
}

/// GraphQL ProofStatistics type.
#[derive(SimpleObject)]
pub struct ProofStatisticsGql {
    pub total_sanads: i64,
    pub total_seals: i64,
    pub sanads_by_commitment_scheme: Vec<SchemeCountGql>,
    pub sanads_by_inclusion_proof: Vec<InclusionProofCountGql>,
    pub sanads_by_finality_proof: Vec<FinalityProofCountGql>,
    pub seals_by_proof_type: Vec<SealProofCountGql>,
}

impl From<tuppira_shared::ProofStatistics> for ProofStatisticsGql {
    fn from(s: tuppira_shared::ProofStatistics) -> Self {
        Self {
            total_sanads: s.total_sanads as i64,
            total_seals: s.total_seals as i64,
            sanads_by_commitment_scheme: s
                .sanads_by_commitment_scheme
                .into_iter()
                .map(|c| c.into())
                .collect(),
            sanads_by_inclusion_proof: s
                .sanads_by_inclusion_proof
                .into_iter()
                .map(|c| c.into())
                .collect(),
            sanads_by_finality_proof: s
                .sanads_by_finality_proof
                .into_iter()
                .map(|c| c.into())
                .collect(),
            seals_by_proof_type: s
                .seals_by_proof_type
                .into_iter()
                .map(|c| c.into())
                .collect(),
        }
    }
}

/// Count of sanads by commitment scheme.
#[derive(SimpleObject)]
pub struct SchemeCountGql {
    pub scheme: String,
    pub count: i64,
}

impl From<tuppira_shared::SchemeCount> for SchemeCountGql {
    fn from(c: tuppira_shared::SchemeCount) -> Self {
        Self {
            scheme: format!("{:?}", c.scheme),
            count: c.count as i64,
        }
    }
}

/// Count of sanads by inclusion proof type.
#[derive(SimpleObject)]
pub struct InclusionProofCountGql {
    pub proof_type: String,
    pub count: i64,
}

impl From<tuppira_shared::InclusionProofCount> for InclusionProofCountGql {
    fn from(c: tuppira_shared::InclusionProofCount) -> Self {
        Self {
            proof_type: format!("{:?}", c.proof_type),
            count: c.count as i64,
        }
    }
}

/// Count of sanads by finality proof type.
#[derive(SimpleObject)]
pub struct FinalityProofCountGql {
    pub proof_type: String,
    pub count: i64,
}

impl From<tuppira_shared::FinalityProofCount> for FinalityProofCountGql {
    fn from(c: tuppira_shared::FinalityProofCount) -> Self {
        Self {
            proof_type: format!("{:?}", c.proof_type),
            count: c.count as i64,
        }
    }
}

/// Count of seals by proof type.
#[derive(SimpleObject)]
pub struct SealProofCountGql {
    pub proof_type: String,
    pub count: i64,
}

impl From<tuppira_shared::SealProofCount> for SealProofCountGql {
    fn from(c: tuppira_shared::SealProofCount) -> Self {
        Self {
            proof_type: c.proof_type,
            count: c.count as i64,
        }
    }
}
