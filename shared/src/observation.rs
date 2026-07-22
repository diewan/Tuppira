//! Source-neutral observation-plane records.
//!
//! These types describe what a source reported and how Tuppira collected it.
//! They are storage/API projections, not Parwana claims, mandates, or verifier
//! results. Exact source bytes remain identified by digests rather than being
//! re-serialized here.

use serde::{Deserialize, Serialize};

/// The only observation schema version understood by this release.
pub const OBSERVATION_SCHEMA_VERSION: u16 = 1;
/// Maximum UTF-8 bytes accepted in an identifier.
pub const MAX_OBSERVATION_ID_BYTES: usize = 512;
/// Maximum subjects or authenticity references on one observation.
pub const MAX_OBSERVATION_REFS: usize = 128;

/// A digest of exact bytes, using the algorithm named by the containing record.
pub type ContentDigest = [u8; 32];

/// Normalization profile for Piteka's deployment evidence export.
pub const DEPLOYMENT_ATTESTATION_PROFILE_ID: &str =
    "org.diewan.piteka.deployment-attestation-observation.v1";

/// A value reported by evidence, or an explicit account of why it is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservedOrMissing<T> {
    Observed { value: T },
    Missing { reasons: Vec<String> },
}

/// One source-reported deployment status. This is history, not a truth verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentStatusObservation {
    pub status: String,
    pub asserted_at: u64,
    pub evidence_refs: Vec<String>,
}

/// Provider workflow identity, when the disclosed evidence identifies one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowIdentityObservation {
    pub provider: String,
    pub repository: String,
    pub workflow_ref: String,
    pub run_id: String,
    pub run_attempt: u32,
}

/// Artifact material reported in the export. Authenticity is deliberately not inferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactAttestationObservation {
    pub digest_algorithm: String,
    /// The artifact digest may be withheld even when an attestation is present.
    pub artifact_digest: ObservedOrMissing<ContentDigest>,
    /// Digest of the exact disclosed attestation evidence content.
    pub attestation_digest: ContentDigest,
    pub attestation_evidence_refs: Vec<String>,
}

/// Typed projection used by F-05. Every required evidence class remains explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentAttestationObservationProfile {
    pub schema_version: u16,
    pub receipt_id: String,
    pub mandate_id: String,
    pub intent_id: String,
    pub attempt_id: String,
    pub status_history: ObservedOrMissing<Vec<DeploymentStatusObservation>>,
    pub workflow_identity: ObservedOrMissing<WorkflowIdentityObservation>,
    pub artifact_attestation: ObservedOrMissing<ArtifactAttestationObservation>,
}

impl DeploymentAttestationObservationProfile {
    pub fn validate(&self) -> Result<(), ObservationValidationError> {
        validate_version(self.schema_version)?;
        for (value, field) in [
            (&self.receipt_id, "receipt_id"),
            (&self.mandate_id, "mandate_id"),
            (&self.intent_id, "intent_id"),
            (&self.attempt_id, "attempt_id"),
        ] {
            validate_id(value, field)?;
        }
        validate_status_history(&self.status_history)?;
        match &self.workflow_identity {
            ObservedOrMissing::Observed { value } => {
                validate_id(&value.provider, "workflow.provider")?;
                validate_id(&value.repository, "workflow.repository")?;
                validate_id(&value.workflow_ref, "workflow.workflow_ref")?;
                validate_id(&value.run_id, "workflow.run_id")?;
                if value.run_attempt == 0 {
                    return Err(ObservationValidationError::InvalidField(
                        "workflow.run_attempt",
                    ));
                }
            }
            ObservedOrMissing::Missing { reasons } => validate_reasons(reasons)?,
        }
        match &self.artifact_attestation {
            ObservedOrMissing::Observed { value } => {
                validate_id(&value.digest_algorithm, "artifact.digest_algorithm")?;
                match &value.artifact_digest {
                    ObservedOrMissing::Observed { value } => {
                        validate_digest(value, "artifact.digest")?
                    }
                    ObservedOrMissing::Missing { reasons } => validate_reasons(reasons)?,
                }
                validate_digest(&value.attestation_digest, "artifact.attestation_digest")?;
                if value.attestation_evidence_refs.is_empty() {
                    return Err(ObservationValidationError::InvalidField(
                        "artifact.attestation_evidence_refs",
                    ));
                }
                validate_refs(
                    &value.attestation_evidence_refs,
                    "artifact.attestation_evidence_refs",
                )?;
            }
            ObservedOrMissing::Missing { reasons } => validate_reasons(reasons)?,
        }
        Ok(())
    }
}

/// A source that Tuppira can observe without assigning it authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub schema_version: u16,
    pub source_id: String,
    /// Registered connector kind, such as `github-export` or `solana-rpc`.
    pub connector_kind: String,
    pub display_name: String,
    pub retention_class_id: String,
}

/// An identity asserted by or configured for a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentityRecord {
    pub schema_version: u16,
    pub source_identity_id: String,
    pub source_id: String,
    pub identity_scheme: String,
    pub identity_bytes: Vec<u8>,
}

/// Retraction state reported by the source; it is not a truth verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetractionStatus {
    Active,
    Retracted,
}

/// Disclosure boundary attached to an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum TenantVisibility {
    Public,
    Tenant { tenant_id: String },
}

/// A normalized, source-bounded account of one source event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationRecord {
    pub schema_version: u16,
    pub observation_id: String,
    pub source_id: String,
    pub source_event_id: String,
    pub source_event_type: String,
    pub subject_refs: Vec<String>,
    pub asserted_event_time: Option<u64>,
    pub observed_at: u64,
    pub normalized_profile_id: String,
    pub normalized_profile_version: u16,
    pub normalized_payload_digest: ContentDigest,
    pub raw_payload_digest: Option<ContentDigest>,
    pub authenticity_material_refs: Vec<String>,
    pub collection_run_id: String,
    pub supersedes: Option<String>,
    pub retraction_status: RetractionStatus,
    pub tenant_visibility: TenantVisibility,
}

/// One subject referenced by an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationSubjectRecord {
    pub schema_version: u16,
    pub observation_id: String,
    pub subject_ref: String,
    pub subject_kind: String,
}

/// A non-authoritative relationship between two observed records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationEdgeRecord {
    pub schema_version: u16,
    pub from_observation_id: String,
    pub to_observation_id: String,
    pub relationship: String,
}

/// Custody metadata for retained or commitment-only source bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPayloadDescriptor {
    pub schema_version: u16,
    pub payload_id: String,
    pub digest_algorithm: String,
    pub payload_digest: ContentDigest,
    pub media_type: String,
    pub byte_length: u64,
    pub custody_locator: Option<String>,
    pub retention_class_id: String,
}

/// One bounded connector collection attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionRunRecord {
    pub schema_version: u16,
    pub collection_run_id: String,
    pub source_id: String,
    pub started_at: u64,
    pub completed_at: Option<u64>,
}

/// Opaque progress owned by a specific source connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncCursorRecord {
    pub schema_version: u16,
    pub source_id: String,
    pub cursor_version: u16,
    pub cursor: Vec<u8>,
    pub observed_at: u64,
}

/// Provider-supplied authenticity material, without a validity conclusion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSignatureRecord {
    pub schema_version: u16,
    pub authenticity_material_id: String,
    pub source_identity_id: String,
    pub signature_scheme: String,
    pub signed_payload_digest: ContentDigest,
    pub signature: Vec<u8>,
}

/// A chain or transparency-service anchor as observed from a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorObservationRecord {
    pub schema_version: u16,
    pub observation_id: String,
    pub anchor_system: String,
    pub anchor_reference: String,
    pub anchored_digest: ContentDigest,
}

/// A source history discontinuity such as a chain reorganization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReorgRecord {
    pub schema_version: u16,
    pub reorg_id: String,
    pub source_id: String,
    pub detected_at: u64,
    pub prior_tip: String,
    pub replacement_tip: String,
}

/// Explicit lineage between a correction and its previous observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupersessionRecord {
    pub schema_version: u16,
    pub superseding_observation_id: String,
    pub superseded_observation_id: String,
    pub observed_at: u64,
}

/// A query hint that observations disagree; never an authoritative verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContradictionHintRecord {
    pub schema_version: u16,
    pub hint_id: String,
    pub left_observation_id: String,
    pub right_observation_id: String,
    pub detector_id: String,
}

/// Purpose limitation and lifecycle policy for observed data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionClassRecord {
    pub schema_version: u16,
    pub retention_class_id: String,
    pub purpose: String,
    pub retain_for_seconds: Option<u64>,
    pub raw_payload_permitted: bool,
}

/// Rejection reasons at the source-neutral boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationValidationError {
    UnsupportedVersion(u16),
    InvalidField(&'static str),
    InvalidDigest(&'static str),
    BoundsExceeded(&'static str),
    DuplicateReference(&'static str),
    SelfSupersession,
}

impl ObservationRecord {
    /// Fail-closed validation for ingestion and deserialization boundaries.
    pub fn validate(&self) -> Result<(), ObservationValidationError> {
        validate_version(self.schema_version)?;
        validate_id(&self.observation_id, "observation_id")?;
        validate_id(&self.source_id, "source_id")?;
        validate_id(&self.source_event_id, "source_event_id")?;
        validate_id(&self.source_event_type, "source_event_type")?;
        validate_id(&self.normalized_profile_id, "normalized_profile_id")?;
        validate_id(&self.collection_run_id, "collection_run_id")?;
        if self.observed_at == 0 || self.normalized_profile_version == 0 {
            return Err(ObservationValidationError::InvalidField(
                "time_or_profile_version",
            ));
        }
        validate_digest(&self.normalized_payload_digest, "normalized_payload_digest")?;
        if let Some(digest) = &self.raw_payload_digest {
            validate_digest(digest, "raw_payload_digest")?;
        }
        validate_refs(&self.subject_refs, "subject_refs")?;
        validate_refs(
            &self.authenticity_material_refs,
            "authenticity_material_refs",
        )?;
        if self.supersedes.as_deref() == Some(self.observation_id.as_str()) {
            return Err(ObservationValidationError::SelfSupersession);
        }
        if let Some(id) = &self.supersedes {
            validate_id(id, "supersedes")?;
        }
        if let TenantVisibility::Tenant { tenant_id } = &self.tenant_visibility {
            validate_id(tenant_id, "tenant_id")?;
        }
        Ok(())
    }
}

fn validate_version(version: u16) -> Result<(), ObservationValidationError> {
    if version != OBSERVATION_SCHEMA_VERSION {
        return Err(ObservationValidationError::UnsupportedVersion(version));
    }
    Ok(())
}

fn validate_id(value: &str, field: &'static str) -> Result<(), ObservationValidationError> {
    if value.trim().is_empty() || value.len() > MAX_OBSERVATION_ID_BYTES || value.contains('\0') {
        return Err(ObservationValidationError::InvalidField(field));
    }
    Ok(())
}

fn validate_digest(
    digest: &ContentDigest,
    field: &'static str,
) -> Result<(), ObservationValidationError> {
    if *digest == [0; 32] {
        return Err(ObservationValidationError::InvalidDigest(field));
    }
    Ok(())
}

fn validate_refs(values: &[String], field: &'static str) -> Result<(), ObservationValidationError> {
    if values.len() > MAX_OBSERVATION_REFS {
        return Err(ObservationValidationError::BoundsExceeded(field));
    }
    for (index, value) in values.iter().enumerate() {
        validate_id(value, field)?;
        if values[..index].contains(value) {
            return Err(ObservationValidationError::DuplicateReference(field));
        }
    }
    Ok(())
}

fn validate_reasons(reasons: &[String]) -> Result<(), ObservationValidationError> {
    if reasons.is_empty() {
        return Err(ObservationValidationError::InvalidField("missing.reasons"));
    }
    validate_refs(reasons, "missing.reasons")
}

fn validate_status_history(
    history: &ObservedOrMissing<Vec<DeploymentStatusObservation>>,
) -> Result<(), ObservationValidationError> {
    match history {
        ObservedOrMissing::Missing { reasons } => validate_reasons(reasons),
        ObservedOrMissing::Observed { value } => {
            if value.is_empty() || value.len() > MAX_OBSERVATION_REFS {
                return Err(ObservationValidationError::BoundsExceeded("status_history"));
            }
            let mut previous = 0;
            for status in value {
                validate_id(&status.status, "status_history.status")?;
                if status.asserted_at == 0 || status.asserted_at < previous {
                    return Err(ObservationValidationError::InvalidField(
                        "status_history.asserted_at",
                    ));
                }
                validate_refs(&status.evidence_refs, "status_history.evidence_refs")?;
                previous = status.asserted_at;
            }
            Ok(())
        }
    }
}

// ── On-chain anchor observations (ANCHOR-01) ─────────────────────────────────

/// One observation-plane read of an on-chain accountability anchor's finality.
///
/// Tuppira *observes* anchor finality; it never asserts it. A set of these from
/// different sources is reconciled with the Parwana [`reconcile_anchor`] semantics
/// — the single protocol source of truth — so a reorg or RPC disagreement is
/// preserved as an explicit disagreement, never collapsed into a false final.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorFinalityObservation {
    /// Observation schema version.
    pub schema_version: u16,
    /// The anchored commitment digest, hex-encoded.
    pub commitment_hex: String,
    /// Canonical chain identifier (for example `ethereum-sepolia`).
    pub chain_id: String,
    /// The observing source (for example an RPC endpoint id).
    pub source: String,
    /// The block height the source reports for the anchor.
    pub block_height: u64,
    /// The block hash the source reports at that height, hex-encoded.
    pub block_hash_hex: String,
    /// Confirmations observed so far by this source.
    pub observed_confirmations: u64,
    /// Reorg-safe confirmations required before the anchor is final.
    pub required_confirmations: u64,
}

/// A failure reconciling anchor observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorObservationError {
    /// A block hash was not 32 bytes of hex.
    MalformedBlockHash,
}

impl core::fmt::Display for AnchorObservationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedBlockHash => f.write_str("block hash must be 32 bytes of hex"),
        }
    }
}

impl std::error::Error for AnchorObservationError {}

fn parse_block_hash(hex: &str) -> Result<[u8; 32], AnchorObservationError> {
    let bytes = ::hex::decode(hex).map_err(|_| AnchorObservationError::MalformedBlockHash)?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AnchorObservationError::MalformedBlockHash)?;
    Ok(array)
}

/// Reconciles a set of anchor finality observations using the Parwana protocol
/// reconciliation. Sources that disagree on the including block yield an explicit
/// [`csv_sdk::accountability::AnchorReconciliation::Disagreement`] (a reorg or RPC
/// disagreement); only unanimous agreement can be `Agreed`, and finality is never
/// fabricated.
///
/// # Errors
///
/// Returns [`AnchorObservationError::MalformedBlockHash`] if any observation's
/// block hash is not 32 bytes of hex.
pub fn reconcile_anchor_observations(
    records: &[AnchorFinalityObservation],
) -> Result<csv_sdk::accountability::AnchorReconciliation, AnchorObservationError> {
    use csv_sdk::accountability::{AnchorFinality, AnchorObservation, reconcile_anchor};

    let observations: Vec<AnchorObservation> = records
        .iter()
        .map(|record| {
            Ok(AnchorObservation {
                source: record.source.clone(),
                block_height: record.block_height,
                block_hash: parse_block_hash(&record.block_hash_hex)?,
                finality: AnchorFinality::from_confirmations(
                    record.observed_confirmations,
                    record.required_confirmations,
                ),
            })
        })
        .collect::<Result<_, AnchorObservationError>>()?;
    Ok(reconcile_anchor(&observations))
}

#[cfg(test)]
mod anchor_observation_tests {
    use super::*;
    use csv_sdk::accountability::AnchorReconciliation;

    fn record(source: &str, block_hash_hex: &str, observed: u64) -> AnchorFinalityObservation {
        AnchorFinalityObservation {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            commitment_hex: "11".repeat(32),
            chain_id: "ethereum-sepolia".to_string(),
            source: source.to_string(),
            block_height: 1_000,
            block_hash_hex: block_hash_hex.to_string(),
            observed_confirmations: observed,
            required_confirmations: 12,
        }
    }

    #[test]
    fn unanimous_final_observations_agree_final() {
        let reconciliation = reconcile_anchor_observations(&[
            record("rpc-a", &"22".repeat(32), 20),
            record("rpc-b", &"22".repeat(32), 20),
        ])
        .unwrap();
        match reconciliation {
            AnchorReconciliation::Agreed { finality, .. } => assert!(finality.is_final()),
            other => panic!("expected agreed-final, got {other:?}"),
        }
    }

    #[test]
    fn reorg_between_sources_is_a_disagreement_not_a_false_final() {
        // Two sources report a different block hash at the same height: a reorg /
        // RPC disagreement. It must be preserved, never resolved into a final.
        let reconciliation = reconcile_anchor_observations(&[
            record("rpc-a", &"22".repeat(32), 20),
            record("rpc-b", &"33".repeat(32), 20),
        ])
        .unwrap();
        assert!(matches!(
            reconciliation,
            AnchorReconciliation::Disagreement { .. }
        ));
    }

    #[test]
    fn a_lagging_source_keeps_the_reconciliation_pending() {
        let reconciliation = reconcile_anchor_observations(&[
            record("rpc-a", &"22".repeat(32), 20),
            record("rpc-b", &"22".repeat(32), 2),
        ])
        .unwrap();
        match reconciliation {
            AnchorReconciliation::Agreed { finality, .. } => assert!(!finality.is_final()),
            other => panic!("expected agreed-pending, got {other:?}"),
        }
    }

    #[test]
    fn a_malformed_block_hash_fails_closed() {
        assert_eq!(
            reconcile_anchor_observations(&[record("rpc-a", "not-hex", 20)]),
            Err(AnchorObservationError::MalformedBlockHash)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation() -> ObservationRecord {
        ObservationRecord {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            observation_id: "obs:github:deployment:42".into(),
            source_id: "source:piteka-export".into(),
            source_event_id: "deployment:42".into(),
            source_event_type: "github.deployment.created".into(),
            subject_refs: vec!["repo:diewan/example".into(), "git:sha:abc123".into()],
            asserted_event_time: Some(100),
            observed_at: 101,
            normalized_profile_id: "org.diewan.github-deployment-observation.v1".into(),
            normalized_profile_version: 1,
            normalized_payload_digest: [1; 32],
            raw_payload_digest: Some([2; 32]),
            authenticity_material_refs: vec!["signature:42".into()],
            collection_run_id: "run:1".into(),
            supersedes: None,
            retraction_status: RetractionStatus::Active,
            tenant_visibility: TenantVisibility::Tenant {
                tenant_id: "tenant:acme".into(),
            },
        }
    }

    #[test]
    fn accepts_source_neutral_observation_without_authority_fields() {
        assert_eq!(observation().validate(), Ok(()));
    }

    #[test]
    fn rejects_unknown_versions_instead_of_renormalizing() {
        let mut value = observation();
        value.schema_version = OBSERVATION_SCHEMA_VERSION + 1;
        assert_eq!(
            value.validate(),
            Err(ObservationValidationError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn rejects_zero_digest_duplicate_subject_and_self_supersession() {
        let mut value = observation();
        value.normalized_payload_digest = [0; 32];
        assert_eq!(
            value.validate(),
            Err(ObservationValidationError::InvalidDigest(
                "normalized_payload_digest"
            ))
        );

        let mut value = observation();
        value.subject_refs.push(value.subject_refs[0].clone());
        assert_eq!(
            value.validate(),
            Err(ObservationValidationError::DuplicateReference(
                "subject_refs"
            ))
        );

        let mut value = observation();
        value.supersedes = Some(value.observation_id.clone());
        assert_eq!(
            value.validate(),
            Err(ObservationValidationError::SelfSupersession)
        );
    }

    #[test]
    fn serde_rejects_ambiguous_unknown_fields() {
        let json = r#"{"schema_version":1,"observation_id":"obs:1","source_id":"source:1","source_event_id":"event:1","source_event_type":"event","subject_refs":[],"asserted_event_time":null,"observed_at":1,"normalized_profile_id":"profile:1","normalized_profile_version":1,"normalized_payload_digest":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"raw_payload_digest":null,"authenticity_material_refs":[],"collection_run_id":"run:1","supersedes":null,"retraction_status":"active","tenant_visibility":{"scope":"public"},"authorized":true}"#;
        assert!(serde_json::from_str::<ObservationRecord>(json).is_err());
    }

    #[test]
    fn deployment_profile_preserves_observed_and_missing_evidence() {
        let profile = DeploymentAttestationObservationProfile {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            receipt_id: "receipt:1".into(),
            mandate_id: "mandate:1".into(),
            intent_id: "intent:1".into(),
            attempt_id: "attempt:1".into(),
            status_history: ObservedOrMissing::Observed {
                value: vec![DeploymentStatusObservation {
                    status: "succeeded".into(),
                    asserted_at: 10,
                    evidence_refs: vec!["evidence:status:1".into()],
                }],
            },
            workflow_identity: ObservedOrMissing::Missing {
                reasons: vec!["workflow identity was not disclosed".into()],
            },
            artifact_attestation: ObservedOrMissing::Observed {
                value: ArtifactAttestationObservation {
                    digest_algorithm: "sha-256".into(),
                    artifact_digest: ObservedOrMissing::Observed { value: [3; 32] },
                    attestation_digest: [4; 32],
                    attestation_evidence_refs: vec!["evidence:attestation:1".into()],
                },
            },
        };
        assert_eq!(profile.validate(), Ok(()));
    }

    #[test]
    fn deployment_profile_rejects_implicit_or_ambiguous_absence() {
        let profile = DeploymentAttestationObservationProfile {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            receipt_id: "receipt:1".into(),
            mandate_id: "mandate:1".into(),
            intent_id: "intent:1".into(),
            attempt_id: "attempt:1".into(),
            status_history: ObservedOrMissing::Observed { value: Vec::new() },
            workflow_identity: ObservedOrMissing::Missing {
                reasons: Vec::new(),
            },
            artifact_attestation: ObservedOrMissing::Missing {
                reasons: Vec::new(),
            },
        };
        assert!(profile.validate().is_err());
    }
}
