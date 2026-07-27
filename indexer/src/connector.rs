//! Source-neutral connector contract.
//!
//! Connectors discover exact source bytes, authenticate those bytes, and only
//! then normalize them into observation candidates. Authentication is kept
//! separate from parsing so a well-formed payload is never implicitly trusted.

use async_trait::async_trait;
use std::collections::HashSet;

use tuppira_shared::{
    CLOSURE_OBSERVATION_PROFILE_VERSION, ContradictionHintRecord,
    DeploymentAttestationProjectionV1, ObservationRecord, ProviderSignatureRecord,
    RawPayloadDescriptor, ReorgRecord, SOURCE_CLOSURE_OBSERVATION_PROFILE_ID, SupersessionRecord,
    TenantVisibility, TuppiraError,
};

use crate::closure_normalization::NormalizedClosureObservation;

/// Maximum number of events returned by one bounded discovery call.
pub const MAX_DISCOVERY_LIMIT: u32 = 10_000;
/// Maximum size of a single retained source event.
pub const MAX_RAW_EVENT_BYTES: usize = 8 * 1024 * 1024;
/// Maximum size of an opaque connector cursor.
pub const MAX_CURSOR_BYTES: usize = 64 * 1024;

/// Result type used by source-neutral connectors.
pub type ConnectorResult<T> = Result<T, ConnectorError>;

/// Fail-closed connector boundary errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectorError {
    #[error("unsupported connector contract version {0}")]
    UnsupportedContractVersion(u16),
    #[error("unsupported normalization profile {profile_id} version {version}")]
    UnsupportedProfile { profile_id: String, version: u16 },
    #[error("invalid connector field: {0}")]
    InvalidField(&'static str),
    #[error("connector bound exceeded: {0}")]
    BoundExceeded(&'static str),
    #[error("duplicate source event id: {0}")]
    DuplicateSourceEvent(String),
    #[error("source event belongs to {actual}, expected {expected}")]
    SourceMismatch { expected: String, actual: String },
    #[error("source authentication rejected event {event_id}: {reason}")]
    AuthenticationRejected { event_id: String, reason: String },
    #[error("source authentication is indeterminate for event {event_id}: {reason}")]
    AuthenticationIndeterminate { event_id: String, reason: String },
    #[error("connector operation failed: {0}")]
    Operation(String),
}

impl From<TuppiraError> for ConnectorError {
    fn from(value: TuppiraError) -> Self {
        Self::Operation(value.to_string())
    }
}

/// Opaque progress owned and versioned by one connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorCursor {
    pub source_id: String,
    pub cursor_version: u16,
    pub bytes: Vec<u8>,
}

impl ConnectorCursor {
    pub fn validate(&self) -> ConnectorResult<()> {
        validate_id(&self.source_id, "cursor.source_id")?;
        if self.cursor_version == 0 {
            return Err(ConnectorError::InvalidField("cursor.cursor_version"));
        }
        if self.bytes.is_empty() || self.bytes.len() > MAX_CURSOR_BYTES {
            return Err(ConnectorError::BoundExceeded("cursor.bytes"));
        }
        Ok(())
    }
}

/// Exact event bytes plus non-authoritative transport metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSourceEvent {
    pub source_id: String,
    pub source_event_id: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub observed_at: u64,
    pub tenant_visibility: TenantVisibility,
}

impl RawSourceEvent {
    pub fn validate(&self) -> ConnectorResult<()> {
        validate_id(&self.source_id, "raw_event.source_id")?;
        validate_id(&self.source_event_id, "raw_event.source_event_id")?;
        validate_id(&self.media_type, "raw_event.media_type")?;
        if self.observed_at == 0 {
            return Err(ConnectorError::InvalidField("raw_event.observed_at"));
        }
        if self.bytes.is_empty() || self.bytes.len() > MAX_RAW_EVENT_BYTES {
            return Err(ConnectorError::BoundExceeded("raw_event.bytes"));
        }
        if let TenantVisibility::Tenant { tenant_id } = &self.tenant_visibility {
            validate_id(tenant_id, "raw_event.tenant_id")?;
        }
        Ok(())
    }
}

/// One bounded discovery result. It does not imply authenticity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSourceBatch {
    pub source_id: String,
    pub events: Vec<RawSourceEvent>,
    pub proposed_cursor: ConnectorCursor,
}

impl RawSourceBatch {
    pub fn validate(&self, requested_limit: u32) -> ConnectorResult<()> {
        validate_limit(requested_limit)?;
        validate_id(&self.source_id, "batch.source_id")?;
        self.proposed_cursor.validate()?;
        if self.proposed_cursor.source_id != self.source_id {
            return Err(ConnectorError::SourceMismatch {
                expected: self.source_id.clone(),
                actual: self.proposed_cursor.source_id.clone(),
            });
        }
        if self.events.len() > requested_limit as usize {
            return Err(ConnectorError::BoundExceeded("batch.events"));
        }
        let mut ids = HashSet::with_capacity(self.events.len());
        for event in &self.events {
            event.validate()?;
            if event.source_id != self.source_id {
                return Err(ConnectorError::SourceMismatch {
                    expected: self.source_id.clone(),
                    actual: event.source_id.clone(),
                });
            }
            if !ids.insert(event.source_event_id.as_str()) {
                return Err(ConnectorError::DuplicateSourceEvent(
                    event.source_event_id.clone(),
                ));
            }
        }
        Ok(())
    }
}

/// Result of authenticating exact raw bytes. Parsing success is not a variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceAuthentication {
    Authenticated {
        material: Vec<ProviderSignatureRecord>,
    },
    Rejected {
        reason: String,
    },
    Indeterminate {
        reason: String,
    },
}

/// Input to persistence after authenticated bytes have been normalized.
///
/// The contained [`ObservationRecord`] has full constitutional provenance; the
/// wrapper is named Input because it has not yet passed storage constraints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedObservationInput {
    pub observation: ObservationRecord,
    pub raw_payload: Option<RawPayloadDescriptor>,
    /// Present only for the typed deployment/attestation normalization profile.
    pub deployment_profile: Option<DeploymentAttestationProjectionV1>,
    /// Present only for the source-closure normalization profile. It carries
    /// the projection together with the chain evidence it was derived from,
    /// because a normalized closure separated from its evidence is an
    /// assertion rather than an observation.
    pub closure_observation: Option<NormalizedClosureObservation>,
}

/// Non-authoritative comparison assessment; never a source fact or verifier result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReconciliationAssessment {
    pub source_id: String,
    pub subject_ref: String,
    pub reorgs: Vec<ReorgRecord>,
    pub supersessions: Vec<SupersessionRecord>,
    pub contradictions: Vec<ContradictionHintRecord>,
}

impl SourceReconciliationAssessment {
    /// Validate connector-owned reconciliation output before persistence. This
    /// checks shape and source ownership; storage enforces relational and tenant invariants.
    pub fn validate(&self, expected_source_id: &str) -> ConnectorResult<()> {
        validate_id(&self.source_id, "reconciliation.source_id")?;
        validate_id(&self.subject_ref, "reconciliation.subject_ref")?;
        if self.source_id != expected_source_id {
            return Err(ConnectorError::SourceMismatch {
                expected: expected_source_id.to_string(),
                actual: self.source_id.clone(),
            });
        }
        for reorg in &self.reorgs {
            validate_id(&reorg.reorg_id, "reconciliation.reorg_id")?;
            validate_id(&reorg.prior_tip, "reconciliation.prior_tip")?;
            validate_id(&reorg.replacement_tip, "reconciliation.replacement_tip")?;
            if reorg.schema_version != 1
                || reorg.source_id != self.source_id
                || reorg.detected_at == 0
                || reorg.prior_tip == reorg.replacement_tip
            {
                return Err(ConnectorError::InvalidField("reconciliation.reorg"));
            }
        }
        for supersession in &self.supersessions {
            validate_id(
                &supersession.superseding_observation_id,
                "reconciliation.superseding",
            )?;
            validate_id(
                &supersession.superseded_observation_id,
                "reconciliation.superseded",
            )?;
            if supersession.schema_version != 1
                || supersession.observed_at == 0
                || supersession.superseding_observation_id == supersession.superseded_observation_id
            {
                return Err(ConnectorError::InvalidField("reconciliation.supersession"));
            }
        }
        for contradiction in &self.contradictions {
            validate_id(&contradiction.hint_id, "reconciliation.hint_id")?;
            validate_id(&contradiction.left_observation_id, "reconciliation.left")?;
            validate_id(&contradiction.right_observation_id, "reconciliation.right")?;
            validate_id(&contradiction.detector_id, "reconciliation.detector_id")?;
            if contradiction.schema_version != 1
                || contradiction.left_observation_id == contradiction.right_observation_id
            {
                return Err(ConnectorError::InvalidField("reconciliation.contradiction"));
            }
        }
        Ok(())
    }
}

/// Connector liveness only. Health never asserts that source claims are true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceHealth {
    Healthy,
    Degraded { reason: String },
    Unavailable { reason: String },
}

/// The source-neutral connector contract from the architecture plan.
#[async_trait]
pub trait SourceConnector: Send + Sync {
    fn source_id(&self) -> &str;

    async fn discover(
        &self,
        cursor: Option<&ConnectorCursor>,
        limit: u32,
    ) -> ConnectorResult<RawSourceBatch>;

    async fn authenticate(
        &self,
        raw_event: &RawSourceEvent,
    ) -> ConnectorResult<SourceAuthentication>;

    fn normalize(
        &self,
        raw_event: &RawSourceEvent,
        profile_version: u16,
    ) -> ConnectorResult<NormalizedObservationInput>;

    fn checkpoint(&self, batch: &RawSourceBatch) -> ConnectorResult<ConnectorCursor>;

    async fn reconcile(
        &self,
        subject_ref: &str,
        interval: (u64, u64),
    ) -> ConnectorResult<SourceReconciliationAssessment>;

    async fn health(&self) -> SourceHealth;
}

/// Authenticate first, then normalize. Rejected or uncertain input cannot enter ingestion.
pub async fn authenticate_and_normalize(
    connector: &dyn SourceConnector,
    raw_event: &RawSourceEvent,
    profile_version: u16,
) -> ConnectorResult<NormalizedObservationInput> {
    raw_event.validate()?;
    if raw_event.source_id != connector.source_id() {
        return Err(ConnectorError::SourceMismatch {
            expected: connector.source_id().to_string(),
            actual: raw_event.source_id.clone(),
        });
    }
    let authentication_material = match connector.authenticate(raw_event).await? {
        SourceAuthentication::Authenticated { material } => material,
        SourceAuthentication::Rejected { reason } => {
            return Err(ConnectorError::AuthenticationRejected {
                event_id: raw_event.source_event_id.clone(),
                reason,
            });
        }
        SourceAuthentication::Indeterminate { reason } => {
            return Err(ConnectorError::AuthenticationIndeterminate {
                event_id: raw_event.source_event_id.clone(),
                reason,
            });
        }
    };
    let candidate = connector.normalize(raw_event, profile_version)?;
    if let Some(profile) = &candidate.deployment_profile {
        profile.validate().map_err(|error| {
            ConnectorError::Operation(format!("invalid deployment profile: {error:?}"))
        })?;
        if candidate.observation.normalized_profile_id
            != tuppira_shared::DEPLOYMENT_ATTESTATION_PROFILE_ID
        {
            return Err(ConnectorError::InvalidField("normalized profile linkage"));
        }
    }
    if let Some(closure) = &candidate.closure_observation {
        closure.projection.validate().map_err(|error| {
            ConnectorError::Operation(format!("invalid closure projection: {error:?}"))
        })?;
        closure.evidence.validate().map_err(|error| {
            ConnectorError::Operation(format!("invalid chain closure evidence: {error:?}"))
        })?;
        if candidate.observation.normalized_profile_id != SOURCE_CLOSURE_OBSERVATION_PROFILE_ID
            || candidate.observation.normalized_profile_version
                != CLOSURE_OBSERVATION_PROFILE_VERSION
        {
            return Err(ConnectorError::InvalidField("normalized profile linkage"));
        }
        // The observation commits to exactly one normalized payload. A closure
        // whose bytes hash to anything else would leave that commitment
        // pointing at a projection nobody holds.
        let digest = closure.projection.normalized_payload_digest().map_err(|error| {
            ConnectorError::Operation(format!("closure projection digest: {error:?}"))
        })?;
        if digest != candidate.observation.normalized_payload_digest {
            return Err(ConnectorError::InvalidField("normalized closure commitment"));
        }
        // Evidence must address the observation and the family it belongs to,
        // or the way back from the normalized closure leads somewhere else.
        if closure.evidence.observation_id != candidate.observation.observation_id
            || closure.evidence.native_event_kind
                != closure.projection.closure_identity.closure_kind
        {
            return Err(ConnectorError::InvalidField("normalized evidence linkage"));
        }
    }
    candidate
        .observation
        .validate()
        .map_err(|error| ConnectorError::Operation(format!("invalid observation: {error:?}")))?;
    if candidate.observation.source_id != raw_event.source_id
        || candidate.observation.source_event_id != raw_event.source_event_id
        || candidate.observation.tenant_visibility != raw_event.tenant_visibility
    {
        return Err(ConnectorError::InvalidField("normalized source boundary"));
    }
    let mut authenticated_refs = HashSet::with_capacity(authentication_material.len());
    for material in &authentication_material {
        validate_id(
            &material.authenticity_material_id,
            "authentication.material_id",
        )?;
        if material.signature.is_empty()
            || material.signed_payload_digest == [0; 32]
            || !authenticated_refs.insert(material.authenticity_material_id.as_str())
        {
            return Err(ConnectorError::InvalidField("authentication.material"));
        }
    }
    let candidate_refs: HashSet<&str> = candidate
        .observation
        .authenticity_material_refs
        .iter()
        .map(String::as_str)
        .collect();
    if candidate_refs != authenticated_refs {
        return Err(ConnectorError::InvalidField(
            "normalized authenticity linkage",
        ));
    }
    Ok(candidate)
}

pub fn validate_limit(limit: u32) -> ConnectorResult<()> {
    if limit == 0 || limit > MAX_DISCOVERY_LIMIT {
        return Err(ConnectorError::BoundExceeded("discover.limit"));
    }
    Ok(())
}

fn validate_id(value: &str, field: &'static str) -> ConnectorResult<()> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(ConnectorError::InvalidField(field));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuppira_shared::{OBSERVATION_SCHEMA_VERSION, RetractionStatus};

    struct FixtureConnector {
        authentication: SourceAuthentication,
        alter_visibility: bool,
    }

    #[async_trait]
    impl SourceConnector for FixtureConnector {
        fn source_id(&self) -> &str {
            "source:fixture"
        }

        async fn discover(
            &self,
            _cursor: Option<&ConnectorCursor>,
            limit: u32,
        ) -> ConnectorResult<RawSourceBatch> {
            validate_limit(limit)?;
            let batch = RawSourceBatch {
                source_id: self.source_id().to_string(),
                events: vec![raw_event()],
                proposed_cursor: cursor(),
            };
            batch.validate(limit)?;
            Ok(batch)
        }

        async fn authenticate(
            &self,
            _raw_event: &RawSourceEvent,
        ) -> ConnectorResult<SourceAuthentication> {
            Ok(self.authentication.clone())
        }

        fn normalize(
            &self,
            raw_event: &RawSourceEvent,
            profile_version: u16,
        ) -> ConnectorResult<NormalizedObservationInput> {
            if profile_version != 1 {
                return Err(ConnectorError::UnsupportedProfile {
                    profile_id: "fixture.v1".to_string(),
                    version: profile_version,
                });
            }
            Ok(NormalizedObservationInput {
                observation: ObservationRecord {
                    schema_version: OBSERVATION_SCHEMA_VERSION,
                    observation_id: "observation:fixture:1".to_string(),
                    source_id: raw_event.source_id.clone(),
                    source_event_id: raw_event.source_event_id.clone(),
                    source_event_type: "fixture_event".to_string(),
                    subject_refs: vec!["subject:1".to_string()],
                    asserted_event_time: None,
                    observed_at: raw_event.observed_at,
                    normalized_profile_id: "fixture.v1".to_string(),
                    normalized_profile_version: profile_version,
                    // Fixture profile v1 defines this digest for the one captured payload.
                    normalized_payload_digest: [7; 32],
                    raw_payload_digest: Some([8; 32]),
                    authenticity_material_refs: vec!["auth:fixture:1".to_string()],
                    collection_run_id: "run:fixture:1".to_string(),
                    supersedes: None,
                    retraction_status: RetractionStatus::Active,
                    tenant_visibility: if self.alter_visibility {
                        TenantVisibility::Public
                    } else {
                        raw_event.tenant_visibility.clone()
                    },
                },
                raw_payload: None,
                deployment_profile: None,
                closure_observation: None,
            })
        }

        fn checkpoint(&self, batch: &RawSourceBatch) -> ConnectorResult<ConnectorCursor> {
            batch.validate(MAX_DISCOVERY_LIMIT)?;
            Ok(batch.proposed_cursor.clone())
        }

        async fn reconcile(
            &self,
            subject_ref: &str,
            interval: (u64, u64),
        ) -> ConnectorResult<SourceReconciliationAssessment> {
            if subject_ref.is_empty() || interval.0 > interval.1 {
                return Err(ConnectorError::InvalidField("reconcile"));
            }
            Ok(SourceReconciliationAssessment {
                source_id: self.source_id().to_string(),
                subject_ref: subject_ref.to_string(),
                reorgs: Vec::new(),
                supersessions: Vec::new(),
                contradictions: Vec::new(),
            })
        }

        async fn health(&self) -> SourceHealth {
            SourceHealth::Healthy
        }
    }

    fn authenticated_connector() -> FixtureConnector {
        FixtureConnector {
            authentication: SourceAuthentication::Authenticated {
                material: vec![authentication_material()],
            },
            alter_visibility: false,
        }
    }

    fn authentication_material() -> ProviderSignatureRecord {
        ProviderSignatureRecord {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            authenticity_material_id: "auth:fixture:1".to_string(),
            source_identity_id: "identity:fixture:1".to_string(),
            signature_scheme: "fixture-captured-signature-v1".to_string(),
            signed_payload_digest: [8; 32],
            signature: vec![1, 2, 3],
        }
    }

    fn raw_event() -> RawSourceEvent {
        RawSourceEvent {
            source_id: "source:fixture".to_string(),
            source_event_id: "event:1".to_string(),
            media_type: "application/json".to_string(),
            bytes: br#"{"event":"captured"}"#.to_vec(),
            observed_at: 1,
            tenant_visibility: TenantVisibility::Tenant {
                tenant_id: "tenant:1".to_string(),
            },
        }
    }

    fn cursor() -> ConnectorCursor {
        ConnectorCursor {
            source_id: "source:fixture".to_string(),
            cursor_version: 1,
            bytes: b"cursor:1".to_vec(),
        }
    }

    #[tokio::test]
    async fn authenticates_before_deterministic_normalization() -> ConnectorResult<()> {
        let connector = authenticated_connector();
        let raw = raw_event();
        let first = authenticate_and_normalize(&connector, &raw, 1).await?;
        let second = authenticate_and_normalize(&connector, &raw, 1).await?;
        assert_eq!(first, second);
        assert_eq!(first.observation.tenant_visibility, raw.tenant_visibility);
        Ok(())
    }

    #[tokio::test]
    async fn rejected_authentication_never_reaches_normalization() {
        let connector = FixtureConnector {
            authentication: SourceAuthentication::Rejected {
                reason: "signature mismatch".to_string(),
            },
            alter_visibility: false,
        };
        let result = authenticate_and_normalize(&connector, &raw_event(), 1).await;
        assert!(matches!(
            result,
            Err(ConnectorError::AuthenticationRejected { .. })
        ));
    }

    #[tokio::test]
    async fn indeterminate_authentication_fails_closed() {
        let connector = FixtureConnector {
            authentication: SourceAuthentication::Indeterminate {
                reason: "key unavailable".to_string(),
            },
            alter_visibility: false,
        };
        let result = authenticate_and_normalize(&connector, &raw_event(), 1).await;
        assert!(matches!(
            result,
            Err(ConnectorError::AuthenticationIndeterminate { .. })
        ));
    }

    #[tokio::test]
    async fn normalization_cannot_widen_tenant_visibility() {
        let connector = FixtureConnector {
            authentication: SourceAuthentication::Authenticated {
                material: vec![authentication_material()],
            },
            alter_visibility: true,
        };
        let result = authenticate_and_normalize(&connector, &raw_event(), 1).await;
        assert_eq!(
            result,
            Err(ConnectorError::InvalidField("normalized source boundary"))
        );
    }

    #[tokio::test]
    async fn normalization_cannot_claim_unreturned_authentication_material() {
        let mut material = authentication_material();
        material.authenticity_material_id = "auth:different".to_string();
        let connector = FixtureConnector {
            authentication: SourceAuthentication::Authenticated {
                material: vec![material],
            },
            alter_visibility: false,
        };
        assert_eq!(
            authenticate_and_normalize(&connector, &raw_event(), 1).await,
            Err(ConnectorError::InvalidField(
                "normalized authenticity linkage"
            ))
        );
    }

    // ── The closure linkage a normalized closure must satisfy (TUP-NE-003) ────

    /// A connector that emits a chain closure, with each linkage independently
    /// breakable so the boundary can be tested one failure at a time.
    struct ClosureConnector {
        break_commitment: bool,
        break_evidence_target: bool,
        break_evidence_kind: bool,
    }

    impl ClosureConnector {
        fn intact() -> Self {
            Self {
                break_commitment: false,
                break_evidence_target: false,
                break_evidence_kind: false,
            }
        }
    }

    fn chain_closure_reading() -> crate::closure_normalization::ChainClosureEventReading {
        use crate::closure_normalization::{ChainClosureEventReading, NativeClosureEventReading};
        use tuppira_shared::{
            ClosureSettlementReading, ConsumedStateReading, IndexFreshnessReading,
            ObservedCheckpointReading, ObservedOrMissing, SourceReportedSettlement,
        };
        ChainClosureEventReading {
            chain_id: "ethereum".to_string(),
            network_id: "sepolia".to_string(),
            native_event: NativeClosureEventReading::EvmNullifierRegistration {
                contract_address_hex: "cc".repeat(20),
                nullifier_hex: "dd".repeat(32),
                transaction_hash_hex: "ee".repeat(32),
                log_index: 3,
            },
            consumed_state: ConsumedStateReading {
                transition_id_hex: "11".repeat(31) + "ab",
                output_index: 0,
                state_type: 1,
            },
            successor_commitment_hex: "22".repeat(31) + "cd",
            successor_output_refs: vec!["output:0".to_string()],
            observed_checkpoint: ObservedOrMissing::Observed {
                value: ObservedCheckpointReading {
                    block_height: 900,
                    block_id_hex: "abcdef01".to_string(),
                },
            },
            settlement: ObservedOrMissing::Observed {
                value: ClosureSettlementReading {
                    finality_policy: "confirmations".to_string(),
                    observed_depth: 64,
                    required_depth: 12,
                    reported_settlement: SourceReportedSettlement::Final,
                },
            },
            revocation: ObservedOrMissing::Missing {
                reasons: vec!["source exposes no retraction feed".to_string()],
            },
            index_freshness: IndexFreshnessReading {
                indexed_tip_height: 1_000,
                indexed_tip_block_id_hex: "beef".to_string(),
                indexed_tip_observed_at: 1_760_000_100,
                lag_blocks: ObservedOrMissing::Observed { value: 100 },
            },
            raw_event_digest: [6; 32],
        }
    }

    #[async_trait]
    impl SourceConnector for ClosureConnector {
        fn source_id(&self) -> &str {
            "source:fixture"
        }

        async fn discover(
            &self,
            _cursor: Option<&ConnectorCursor>,
            limit: u32,
        ) -> ConnectorResult<RawSourceBatch> {
            validate_limit(limit)?;
            Err(ConnectorError::Operation("discovery not exercised".into()))
        }

        async fn authenticate(
            &self,
            _raw_event: &RawSourceEvent,
        ) -> ConnectorResult<SourceAuthentication> {
            Ok(SourceAuthentication::Authenticated {
                material: vec![authentication_material()],
            })
        }

        fn normalize(
            &self,
            raw_event: &RawSourceEvent,
            profile_version: u16,
        ) -> ConnectorResult<NormalizedObservationInput> {
            let observation_id = "observation:closure:1".to_string();
            let mut closure = crate::closure_normalization::normalize_chain_closure_event(
                &observation_id,
                &chain_closure_reading(),
            )
            .map_err(|error| ConnectorError::Operation(error.to_string()))?;
            let digest = closure
                .projection
                .normalized_payload_digest()
                .map_err(|error| ConnectorError::Operation(format!("{error:?}")))?;
            if self.break_evidence_target {
                closure.evidence.observation_id = "observation:someone-else".to_string();
            }
            if self.break_evidence_kind {
                closure.evidence.native_event_kind = "bitcoin-outpoint-spend".to_string();
            }
            Ok(NormalizedObservationInput {
                observation: ObservationRecord {
                    schema_version: OBSERVATION_SCHEMA_VERSION,
                    observation_id,
                    source_id: raw_event.source_id.clone(),
                    source_event_id: raw_event.source_event_id.clone(),
                    source_event_type: "chain.closure".to_string(),
                    subject_refs: vec!["sanad:1".to_string()],
                    asserted_event_time: None,
                    observed_at: raw_event.observed_at,
                    normalized_profile_id: SOURCE_CLOSURE_OBSERVATION_PROFILE_ID.to_string(),
                    normalized_profile_version: profile_version,
                    normalized_payload_digest: if self.break_commitment {
                        [7; 32]
                    } else {
                        digest
                    },
                    raw_payload_digest: Some([8; 32]),
                    authenticity_material_refs: vec!["auth:fixture:1".to_string()],
                    collection_run_id: "run:fixture:1".to_string(),
                    supersedes: None,
                    retraction_status: RetractionStatus::Active,
                    tenant_visibility: raw_event.tenant_visibility.clone(),
                },
                raw_payload: None,
                deployment_profile: None,
                closure_observation: Some(closure),
            })
        }

        fn checkpoint(&self, batch: &RawSourceBatch) -> ConnectorResult<ConnectorCursor> {
            batch.validate(MAX_DISCOVERY_LIMIT)?;
            Ok(batch.proposed_cursor.clone())
        }

        async fn reconcile(
            &self,
            subject_ref: &str,
            _interval: (u64, u64),
        ) -> ConnectorResult<SourceReconciliationAssessment> {
            Ok(SourceReconciliationAssessment {
                source_id: self.source_id().to_string(),
                subject_ref: subject_ref.to_string(),
                reorgs: Vec::new(),
                supersessions: Vec::new(),
                contradictions: Vec::new(),
            })
        }

        async fn health(&self) -> SourceHealth {
            SourceHealth::Healthy
        }
    }

    #[tokio::test]
    async fn a_normalized_chain_closure_passes_the_boundary_with_its_evidence() {
        let candidate =
            authenticate_and_normalize(&ClosureConnector::intact(), &raw_event(), 1).await;

        let Ok(candidate) = candidate else {
            panic!("an intact chain closure must pass the connector boundary");
        };
        let Some(closure) = candidate.closure_observation else {
            panic!("the closure must survive the boundary");
        };
        assert_eq!(closure.evidence.observation_id, "observation:closure:1");
        assert_eq!(
            candidate.observation.normalized_profile_id,
            SOURCE_CLOSURE_OBSERVATION_PROFILE_ID
        );
    }

    #[tokio::test]
    async fn a_closure_the_observation_did_not_commit_to_is_rejected() {
        let connector = ClosureConnector {
            break_commitment: true,
            ..ClosureConnector::intact()
        };
        assert_eq!(
            authenticate_and_normalize(&connector, &raw_event(), 1).await,
            Err(ConnectorError::InvalidField("normalized closure commitment"))
        );
    }

    #[tokio::test]
    async fn evidence_pointing_at_another_observation_is_rejected() {
        let connector = ClosureConnector {
            break_evidence_target: true,
            ..ClosureConnector::intact()
        };
        assert_eq!(
            authenticate_and_normalize(&connector, &raw_event(), 1).await,
            Err(ConnectorError::InvalidField("normalized evidence linkage"))
        );
    }

    #[tokio::test]
    async fn evidence_from_the_wrong_chain_family_is_rejected() {
        // Bitcoin locators filed against an EVM nullifier would send a reader
        // back to a chain the closure never happened on.
        let connector = ClosureConnector {
            break_evidence_kind: true,
            ..ClosureConnector::intact()
        };
        assert_eq!(
            authenticate_and_normalize(&connector, &raw_event(), 1).await,
            Err(ConnectorError::InvalidField("normalized evidence linkage"))
        );
    }

    #[test]
    fn batch_rejects_duplicate_and_cross_source_events() {
        let mut duplicate = raw_event();
        duplicate.bytes = b"different bytes, same provider id".to_vec();
        let batch = RawSourceBatch {
            source_id: "source:fixture".to_string(),
            events: vec![raw_event(), duplicate],
            proposed_cursor: cursor(),
        };
        assert!(matches!(
            batch.validate(2),
            Err(ConnectorError::DuplicateSourceEvent(_))
        ));

        let mut foreign = raw_event();
        foreign.source_id = "source:foreign".to_string();
        foreign.source_event_id = "event:2".to_string();
        let batch = RawSourceBatch {
            source_id: "source:fixture".to_string(),
            events: vec![foreign],
            proposed_cursor: cursor(),
        };
        assert!(matches!(
            batch.validate(1),
            Err(ConnectorError::SourceMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn unsupported_profile_and_malformed_bounds_are_rejected() {
        let connector = authenticated_connector();
        assert!(matches!(
            authenticate_and_normalize(&connector, &raw_event(), 2).await,
            Err(ConnectorError::UnsupportedProfile { version: 2, .. })
        ));
        assert_eq!(
            connector.discover(None, 0).await,
            Err(ConnectorError::BoundExceeded("discover.limit"))
        );
        let mut empty = raw_event();
        empty.bytes.clear();
        assert_eq!(
            authenticate_and_normalize(&connector, &empty, 1).await,
            Err(ConnectorError::BoundExceeded("raw_event.bytes"))
        );
    }

    #[test]
    fn reconciliation_rejects_cross_source_and_ambiguous_reorgs() {
        let report = SourceReconciliationAssessment {
            source_id: "source:fixture".into(),
            subject_ref: "subject:1".into(),
            reorgs: vec![ReorgRecord {
                schema_version: 1,
                reorg_id: "reorg:1".into(),
                source_id: "source:fixture".into(),
                detected_at: 10,
                prior_tip: "tip:a".into(),
                replacement_tip: "tip:b".into(),
            }],
            supersessions: Vec::new(),
            contradictions: Vec::new(),
        };
        assert_eq!(report.validate("source:fixture"), Ok(()));
        assert!(matches!(
            report.validate("source:other"),
            Err(ConnectorError::SourceMismatch { .. })
        ));
        let mut ambiguous = report;
        ambiguous.reorgs[0].replacement_tip = ambiguous.reorgs[0].prior_tip.clone();
        assert_eq!(
            ambiguous.validate("source:fixture"),
            Err(ConnectorError::InvalidField("reconciliation.reorg"))
        );
    }
}
