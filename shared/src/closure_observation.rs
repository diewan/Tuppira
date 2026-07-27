//! Observation-plane projections of Parwana V2 source closure.
//!
//! Parwana owns what a closure *means*: a consumed state has exactly one
//! closure-valid successor, and only its verifier decides whether supplied
//! proof material establishes that. This module carries what a *source said*
//! about such a closure, so an investigator can read it without Tuppira ever
//! becoming the authority that declares it.
//!
//! Three rules shape every type here.
//!
//! **These are projections, not protocol state.** Nothing in this module is
//! canonical Parwana bytes, and nothing here is re-hashed or re-derived.
//! Chain-native identities are carried as the source expressed them, and the
//! exact relayed bytes stay identified by digest. A consumer that needs the
//! protocol object fetches it from Parwana and verifies it there.
//!
//! **The states are independent, not a ladder.** `observed`,
//! `verified elsewhere`, `final`, `revoked`, and `unknown` are established by
//! separate facets that cannot substitute for one another: a settled checkpoint
//! is not a verdict, a foreign verdict is not settlement, and neither of them
//! survives a retraction. [`SourceClosureObservationProjectionV1::
//! established_states`] therefore returns the whole set rather than one badge —
//! a summary is exactly the shape that would let a failed facet hide.
//!
//! **Absence is never currency.** Every facet that a source did not report
//! stays [`ObservedOrMissing::Missing`] with its reasons, and each projection
//! carries the [`IndexFreshnessReading`] it was produced under. A closure read
//! from a lagging index reports that lag rather than implying it is current.

use std::collections::BTreeSet;

use csv_sdk::protocol::closure::ClosureDimensionStatus;
use csv_sdk::v2::ClosureTrustMode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::observation::{
    ContentDigest, MAX_OBSERVATION_REFS, ObservationValidationError, ObservedOrMissing,
    RetractionStatus,
};

/// The only closure-observation projection version understood by this release.
pub const CLOSURE_OBSERVATION_PROFILE_VERSION: u16 = 1;

/// Normalization profile identifier for a V2 source-closure observation.
///
/// The trailing `v1` is this projection's own version. It is deliberately not
/// the protocol version: the profile is at v1 while the closure semantics it
/// projects are Parwana V2, and the two version lines move independently.
pub const SOURCE_CLOSURE_OBSERVATION_PROFILE_ID: &str =
    "org.diewan.parwana.source-closure-observation.v1";

/// Bytes in a hex-encoded 32-byte protocol identifier.
const HASH_HEX_LEN: usize = 64;

/// Maximum hex characters accepted for a chain-native identity.
const MAX_NATIVE_IDENTITY_HEX_LEN: usize = 2 * 1024;

/// One thing a closure observation can establish.
///
/// These are reported as a set, never reduced to a single value. Each variant
/// is established by its own facet and none implies another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosureObservationState {
    /// A source reported this closure. It says nothing about validity.
    Observed,
    /// A verifier outside Tuppira reported a verdict, which Tuppira relays.
    VerifiedElsewhere,
    /// The source reported its own finality rule satisfied at the checkpoint.
    Final,
    /// The source withdrew the closure, typically after a reorganization.
    Revoked,
    /// At least one facet was not reported and is not inferred.
    Unknown,
}

/// Parwana's identity for the state whose closure is observed.
///
/// The fields mirror `ConsumedStateRef` so an investigator can match the
/// observation to the protocol object. Tuppira does not recompute its
/// canonical bytes or its commitment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumedStateReading {
    /// Identifier of the transition that created the consumed output, hex-encoded.
    pub transition_id_hex: String,
    /// Index of the output within that transition.
    pub output_index: u32,
    /// State type of the consumed output, as the source reported it.
    pub state_type: u16,
}

impl ConsumedStateReading {
    fn validate(&self) -> Result<(), ObservationValidationError> {
        validate_hash_hex(&self.transition_id_hex, "consumed_state.transition_id_hex")
    }
}

/// The chain-native closure the source reported, and the successor it favours.
///
/// `closure_kind` is a registered connector-supplied name such as
/// `evm-nullifier` or `sui-object-consumption`. Mapping each chain's native
/// event onto these names belongs to the normalization ticket, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosureIdentityReading {
    /// Registered closure family name.
    pub closure_kind: String,
    /// Chain-native closure or nullifier identity, hex-encoded.
    pub closure_identity_hex: String,
    /// Canonical successor transition commitment, hex-encoded.
    pub successor_commitment_hex: String,
}

impl ClosureIdentityReading {
    fn validate(&self) -> Result<(), ObservationValidationError> {
        validate_name(&self.closure_kind, "closure_identity.closure_kind")?;
        validate_native_hex(
            &self.closure_identity_hex,
            "closure_identity.closure_identity_hex",
        )?;
        validate_hash_hex(
            &self.successor_commitment_hex,
            "closure_identity.successor_commitment_hex",
        )
    }
}

/// The block or checkpoint at which the source observed the closure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedCheckpointReading {
    /// Height of the observed block or checkpoint.
    pub block_height: u64,
    /// Native identity of that block or checkpoint, hex-encoded.
    pub block_id_hex: String,
}

impl ObservedCheckpointReading {
    fn validate(&self) -> Result<(), ObservationValidationError> {
        validate_native_hex(&self.block_id_hex, "observed_checkpoint.block_id_hex")
    }
}

/// What a source concluded under its own finality rule.
///
/// This is the source's conclusion about its own chain, not a verdict about
/// the closure. A verifier's finality dimension lives in
/// [`ExternalClosureVerificationReading`] and is never merged with this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceReportedSettlement {
    /// The source reported its named rule satisfied.
    Final,
    /// The source reported the rule not yet satisfied.
    NotYetFinal,
}

/// A source's settlement report for the observed checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosureSettlementReading {
    /// The finality rule the source named, verbatim.
    pub finality_policy: String,
    /// Depth, confirmations, or epochs the source reported.
    pub observed_depth: u64,
    /// Depth the named rule requires.
    pub required_depth: u64,
    /// What the source concluded under that rule.
    pub reported_settlement: SourceReportedSettlement,
}

impl ClosureSettlementReading {
    fn validate(&self) -> Result<(), ObservationValidationError> {
        validate_name(&self.finality_policy, "settlement.finality_policy")?;
        if self.required_depth == 0 {
            return Err(ObservationValidationError::InvalidField(
                "settlement.required_depth",
            ));
        }
        // A source may report less depth than its rule requires; it may not
        // report the rule satisfied while reporting depth that does not satisfy
        // it. Accepting that pairing would let a shallow read be stored as final.
        if self.reported_settlement == SourceReportedSettlement::Final
            && self.observed_depth < self.required_depth
        {
            return Err(ObservationValidationError::InvalidField(
                "settlement.reported_settlement",
            ));
        }
        Ok(())
    }
}

/// A verdict produced by a verifier outside Tuppira, relayed unmerged.
///
/// Tuppira never computes a closure verdict, so this reading always names the
/// foreign verifier that did. The four dimensions are carried separately and in
/// Parwana's own types: collapsing them into one status is what would turn an
/// indeterminate dimension into an apparent pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalClosureVerificationReading {
    /// Stable identifier of the verifier implementation that produced the verdict.
    pub verifier_id: String,
    /// Trust anchor that verifier named for its conclusions.
    pub trust_mode: ClosureTrustMode,
    /// Whether native inclusion was established.
    pub proof_validity: ClosureDimensionStatus,
    /// Whether the checkpoint satisfies its named policy.
    pub checkpoint_finality: ClosureDimensionStatus,
    /// Whether the checkpoint is fresh under the verifier's bound.
    pub checkpoint_freshness: ClosureDimensionStatus,
    /// Whether the source is closed in favour of the supplied successor.
    pub source_closure: ClosureDimensionStatus,
    /// Machine-readable reason codes exactly as the verifier emitted them.
    pub reason_codes: Vec<String>,
    /// Digest of the exact verification-report bytes relayed.
    pub report_digest: ContentDigest,
}

/// Identity prefix reserved for this observation plane's own components.
const OBSERVATION_PLANE_VERIFIER_PREFIX: &str = "tuppira";

impl ExternalClosureVerificationReading {
    fn validate(&self) -> Result<(), ObservationValidationError> {
        validate_name(&self.verifier_id, "external_verification.verifier_id")?;
        // The observation plane never computes an authoritative verdict, so a
        // verdict attributed to it is a boundary violation rather than data.
        if self
            .verifier_id
            .to_ascii_lowercase()
            .starts_with(OBSERVATION_PLANE_VERIFIER_PREFIX)
        {
            return Err(ObservationValidationError::InvalidField(
                "external_verification.verifier_id",
            ));
        }
        if self.reason_codes.is_empty() {
            return Err(ObservationValidationError::InvalidField(
                "external_verification.reason_codes",
            ));
        }
        validate_refs(&self.reason_codes, "external_verification.reason_codes")?;
        if self.report_digest == [0; 32] {
            return Err(ObservationValidationError::InvalidDigest(
                "external_verification.report_digest",
            ));
        }
        Ok(())
    }
}

/// A source's retraction report for a previously observed closure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosureRevocationReading {
    /// Retraction state the source reported.
    pub retraction_status: RetractionStatus,
    /// The reorganization the source attributed the retraction to, if any.
    pub reorg_id: Option<String>,
    /// When the source reported it.
    pub reported_at: u64,
}

impl ClosureRevocationReading {
    fn validate(&self) -> Result<(), ObservationValidationError> {
        if self.reported_at == 0 {
            return Err(ObservationValidationError::InvalidField(
                "revocation.reported_at",
            ));
        }
        if let Some(reorg_id) = &self.reorg_id {
            validate_name(reorg_id, "revocation.reorg_id")?;
        }
        // An active report is the source saying it still stands, so it cannot
        // also name the reorganization that withdrew it.
        if self.retraction_status == RetractionStatus::Active && self.reorg_id.is_some() {
            return Err(ObservationValidationError::InvalidField(
                "revocation.reorg_id",
            ));
        }
        Ok(())
    }
}

/// How far behind the chain the index was when this projection was produced.
///
/// Freshness travels on the projection rather than on the query, because a
/// closure view separated from its lag reads as current. `lag_blocks` is
/// [`ObservedOrMissing::Missing`] whenever no checkpoint was observed: a lag
/// against nothing is not zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexFreshnessReading {
    /// Highest block height indexed for this chain and network.
    pub indexed_tip_height: u64,
    /// Native identity of that tip, hex-encoded.
    pub indexed_tip_block_id_hex: String,
    /// When the tip was read.
    pub indexed_tip_observed_at: u64,
    /// Blocks between the observed checkpoint and the indexed tip.
    pub lag_blocks: ObservedOrMissing<u64>,
}

impl IndexFreshnessReading {
    fn validate(&self) -> Result<(), ObservationValidationError> {
        validate_native_hex(
            &self.indexed_tip_block_id_hex,
            "index_freshness.indexed_tip_block_id_hex",
        )?;
        if self.indexed_tip_observed_at == 0 {
            return Err(ObservationValidationError::InvalidField(
                "index_freshness.indexed_tip_observed_at",
            ));
        }
        if let ObservedOrMissing::Missing { reasons } = &self.lag_blocks {
            validate_reasons(reasons, "index_freshness.lag_blocks")?;
        }
        Ok(())
    }
}

/// A normalized, source-bounded account of one observed V2 source closure.
///
/// This is the payload of an `ObservationRecord` carrying
/// [`SOURCE_CLOSURE_OBSERVATION_PROFILE_ID`]. Provenance — source identity,
/// collection run, acquisition time, raw-payload commitment, and tenant
/// visibility — belongs to that record and is deliberately not repeated here.
/// Freshness is the exception: it is a property of this read, not of the
/// source event, so it travels with the projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceClosureObservationProjectionV1 {
    /// Projection version; see [`CLOSURE_OBSERVATION_PROFILE_VERSION`].
    pub schema_version: u16,
    /// Parwana identity of the consumed state.
    pub consumed_state: ConsumedStateReading,
    /// The chain-native closure and the successor it favours.
    pub closure_identity: ClosureIdentityReading,
    /// Stable chain identifier, such as `ethereum`.
    pub chain_id: String,
    /// Stable network identifier, such as `sepolia`.
    pub network_id: String,
    /// Successor outputs the source associated with this closure.
    pub successor_output_refs: Vec<String>,
    /// The block or checkpoint the source read.
    pub observed_checkpoint: ObservedOrMissing<ObservedCheckpointReading>,
    /// The source's settlement report under its own finality rule.
    pub settlement: ObservedOrMissing<ClosureSettlementReading>,
    /// A foreign verifier's verdict, relayed rather than computed.
    pub external_verification: ObservedOrMissing<ExternalClosureVerificationReading>,
    /// The source's retraction report.
    pub revocation: ObservedOrMissing<ClosureRevocationReading>,
    /// Index lag at the moment this projection was produced.
    pub index_freshness: IndexFreshnessReading,
}

impl SourceClosureObservationProjectionV1 {
    /// Every state this projection currently establishes.
    ///
    /// A set, not a summary: a caller that needs to know whether a closure was
    /// verified elsewhere reads that state directly and never infers it from
    /// settlement. [`ClosureObservationState::Observed`] is always present,
    /// because the projection exists only where a source reported a closure,
    /// and [`ClosureObservationState::Unknown`] joins it whenever any facet
    /// went unreported.
    pub fn established_states(&self) -> BTreeSet<ClosureObservationState> {
        let mut states = BTreeSet::new();
        states.insert(ClosureObservationState::Observed);

        if let ObservedOrMissing::Observed { value } = &self.settlement
            && value.reported_settlement == SourceReportedSettlement::Final
        {
            states.insert(ClosureObservationState::Final);
        }
        if matches!(&self.external_verification, ObservedOrMissing::Observed { .. }) {
            states.insert(ClosureObservationState::VerifiedElsewhere);
        }
        if let ObservedOrMissing::Observed { value } = &self.revocation
            && value.retraction_status == RetractionStatus::Retracted
        {
            states.insert(ClosureObservationState::Revoked);
        }

        let unreported = matches!(&self.observed_checkpoint, ObservedOrMissing::Missing { .. })
            || matches!(&self.settlement, ObservedOrMissing::Missing { .. })
            || matches!(&self.external_verification, ObservedOrMissing::Missing { .. })
            || matches!(&self.revocation, ObservedOrMissing::Missing { .. });
        if unreported {
            states.insert(ClosureObservationState::Unknown);
        }
        states
    }

    /// Fail-closed validation for ingestion and deserialization boundaries.
    ///
    /// # Errors
    ///
    /// Returns [`ObservationValidationError`] for an unsupported version, a
    /// malformed identity or digest, a bound overrun, a duplicated successor
    /// reference, or a facet whose parts contradict each other.
    pub fn validate(&self) -> Result<(), ObservationValidationError> {
        if self.schema_version != CLOSURE_OBSERVATION_PROFILE_VERSION {
            return Err(ObservationValidationError::UnsupportedVersion(
                self.schema_version,
            ));
        }
        self.consumed_state.validate()?;
        self.closure_identity.validate()?;
        validate_name(&self.chain_id, "chain_id")?;
        validate_name(&self.network_id, "network_id")?;
        validate_refs(&self.successor_output_refs, "successor_output_refs")?;

        match &self.observed_checkpoint {
            ObservedOrMissing::Observed { value } => value.validate()?,
            ObservedOrMissing::Missing { reasons } => {
                validate_reasons(reasons, "observed_checkpoint")?
            }
        }
        match &self.settlement {
            ObservedOrMissing::Observed { value } => value.validate()?,
            ObservedOrMissing::Missing { reasons } => validate_reasons(reasons, "settlement")?,
        }
        match &self.external_verification {
            ObservedOrMissing::Observed { value } => value.validate()?,
            ObservedOrMissing::Missing { reasons } => {
                validate_reasons(reasons, "external_verification")?
            }
        }
        match &self.revocation {
            ObservedOrMissing::Observed { value } => value.validate()?,
            ObservedOrMissing::Missing { reasons } => validate_reasons(reasons, "revocation")?,
        }
        self.index_freshness.validate()?;

        // Settlement is a statement about a checkpoint. Without one there is
        // nothing for the source's finality rule to have been applied to.
        if matches!(&self.observed_checkpoint, ObservedOrMissing::Missing { .. })
            && matches!(&self.settlement, ObservedOrMissing::Observed { .. })
        {
            return Err(ObservationValidationError::InvalidField("settlement"));
        }

        self.validate_freshness()
    }

    /// Lag must be derived from a checkpoint and a tip that both exist, and must
    /// be the distance between them. Anything else reports a currency the index
    /// cannot support.
    fn validate_freshness(&self) -> Result<(), ObservationValidationError> {
        let checkpoint = match &self.observed_checkpoint {
            ObservedOrMissing::Observed { value } => value,
            ObservedOrMissing::Missing { .. } => {
                return match &self.index_freshness.lag_blocks {
                    ObservedOrMissing::Missing { .. } => Ok(()),
                    ObservedOrMissing::Observed { .. } => Err(
                        ObservationValidationError::InvalidField("index_freshness.lag_blocks"),
                    ),
                };
            }
        };
        let ObservedOrMissing::Observed { value: lag } = &self.index_freshness.lag_blocks else {
            return Ok(());
        };
        let tip = self.index_freshness.indexed_tip_height;
        // A checkpoint ahead of the indexed tip is not a small lag; it means the
        // reported tip cannot have covered it.
        let expected = tip.checked_sub(checkpoint.block_height).ok_or(
            ObservationValidationError::InvalidField("index_freshness.indexed_tip_height"),
        )?;
        if *lag != expected {
            return Err(ObservationValidationError::InvalidField(
                "index_freshness.lag_blocks",
            ));
        }
        Ok(())
    }

    /// The digest an [`ObservationRecord`] carries for this projection.
    ///
    /// [`ObservationRecord`]: crate::observation::ObservationRecord
    ///
    /// Canonical bytes are this projection's JSON encoding, domain-separated by
    /// [`SOURCE_CLOSURE_OBSERVATION_PROFILE_ID`] and a `0x00` terminator. The
    /// profile id carries the projection version, so bytes read under one
    /// version can never produce a digest that matches another.
    ///
    /// The encoding is deterministic because every field of this projection is
    /// a struct field, a sequence, or an externally-tagged enum — there is no
    /// map whose iteration order could vary — so field order is fixed by the
    /// declarations above. `closure_observation_digest_is_pinned_to_its_bytes`
    /// pins the result, and a reordered field breaks it rather than silently
    /// changing what an observation commits to.
    ///
    /// # Errors
    ///
    /// Returns [`ObservationValidationError::InvalidField`] if the projection
    /// cannot be encoded. Callers validate before committing: a digest over an
    /// invalid projection is well-formed bytes over a claim that failed.
    pub fn normalized_payload_digest(&self) -> Result<ContentDigest, ObservationValidationError> {
        let encoded = serde_json::to_vec(self)
            .map_err(|_| ObservationValidationError::InvalidField("closure_projection.encoding"))?;
        let mut hasher = Sha256::new();
        hasher.update(SOURCE_CLOSURE_OBSERVATION_PROFILE_ID.as_bytes());
        hasher.update([0x00]);
        hasher.update(&encoded);
        Ok(hasher.finalize().into())
    }
}

/// The only subject-closure projection version understood by this release.
pub const SUBJECT_CLOSURE_PROFILE_VERSION: u16 = 1;

/// Normalization profile identifier for the per-subject closure read model.
pub const SUBJECT_CLOSURE_PROJECTION_PROFILE_ID: &str =
    "org.diewan.tuppira.subject-closure-projection.v1";

/// One stored source-closure observation, with the record facts around it.
///
/// `record_retraction_status` is the observation *record's* retraction — the
/// collector withdrew the record — and is deliberately separate from the
/// projection's `revocation`, which is the *source* withdrawing the closure it
/// reported. Merging them would let a withdrawn record read as a live source
/// retraction, or the reverse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedSourceClosureObservationV1 {
    /// Identifier of the observation carrying this projection.
    pub observation_id: String,
    /// When the collector acquired it.
    pub observed_at: u64,
    /// Whether the observation record itself still stands.
    pub record_retraction_status: RetractionStatus,
    /// The projection exactly as stored.
    pub projection: SourceClosureObservationProjectionV1,
}

/// Which closure profile generation the stored evidence for a subject belongs to.
///
/// The two variants are not a scale. `PreClosure` is the absence of a closure
/// statement, not a statement that nothing was closed, and it is the only thing
/// a record indexed before the closure profile can honestly report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "generation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClosureProfileGeneration {
    /// Source-closure observations are recorded for this subject.
    SourceClosureV2 {
        /// Every recorded observation, newest acquisition first. Never empty.
        observations: Vec<RecordedSourceClosureObservationV1>,
    },
    /// No source-closure observation is recorded for this subject.
    PreClosure {
        /// Why no closure statement exists. Never empty.
        reasons: Vec<String>,
    },
}

/// The closure account the observation plane holds for one explorer subject.
///
/// Sanads, transfers, and seals indexed before the closure profile carry no
/// closure observation, and this projection reports exactly that: generation
/// [`ClosureProfileGeneration::PreClosure`] and the single established state
/// [`ClosureObservationState::Unknown`].
///
/// It never derives closure from the V1 explorer read model. A Sanad whose
/// stored `status` is `spent` is a chain-level spend the indexer saw; a V2
/// closure is a protocol statement that a consumed state has exactly one
/// closure-valid successor. Reading the first as the second is the fabrication
/// this type exists to prevent, so no field of the V1 read model reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectClosureProjectionV1 {
    /// Projection version; see [`SUBJECT_CLOSURE_PROFILE_VERSION`].
    pub schema_version: u16,
    /// The subject this account is for, as observations reference it.
    pub subject_ref: String,
    /// What generation of closure evidence exists, and the evidence itself.
    pub closure_generation: ClosureProfileGeneration,
}

impl SubjectClosureProjectionV1 {
    /// The account a subject with no recorded closure observation gets.
    #[must_use]
    pub fn pre_closure(subject_ref: impl Into<String>) -> Self {
        Self {
            schema_version: SUBJECT_CLOSURE_PROFILE_VERSION,
            subject_ref: subject_ref.into(),
            closure_generation: ClosureProfileGeneration::PreClosure {
                reasons: vec![
                    "no source-closure observation is recorded for this subject".to_string(),
                    "records normalized before the closure profile carry no closure statement"
                        .to_string(),
                ],
            },
        }
    }

    /// Every state the recorded observations establish for this subject.
    ///
    /// A union, not a reduction: two observations that disagree both contribute,
    /// because a subject reported final by one source and revoked by another has
    /// established both and neither cancels the other. Deciding which survives a
    /// reorganization is TUP-NE-004's reorg-aware projection, and it needs this
    /// set intact to do it.
    ///
    /// [`ClosureProfileGeneration::PreClosure`] yields exactly
    /// [`ClosureObservationState::Unknown`] — not
    /// [`ClosureObservationState::Observed`], because no closure was observed at
    /// all.
    #[must_use]
    pub fn established_states(&self) -> BTreeSet<ClosureObservationState> {
        match &self.closure_generation {
            ClosureProfileGeneration::PreClosure { .. } => {
                BTreeSet::from([ClosureObservationState::Unknown])
            }
            ClosureProfileGeneration::SourceClosureV2 { observations } => observations
                .iter()
                .flat_map(|recorded| recorded.projection.established_states())
                .collect(),
        }
    }

    /// Fail-closed validation for storage and API boundaries.
    ///
    /// # Errors
    ///
    /// Returns [`ObservationValidationError`] for an unsupported version, a
    /// malformed subject reference, an empty or over-long generation payload, a
    /// duplicated observation identifier, or an invalid nested projection.
    pub fn validate(&self) -> Result<(), ObservationValidationError> {
        if self.schema_version != SUBJECT_CLOSURE_PROFILE_VERSION {
            return Err(ObservationValidationError::UnsupportedVersion(
                self.schema_version,
            ));
        }
        validate_name(&self.subject_ref, "subject_ref")?;
        match &self.closure_generation {
            ClosureProfileGeneration::PreClosure { reasons } => {
                validate_reasons(reasons, "closure_generation.reasons")
            }
            ClosureProfileGeneration::SourceClosureV2 { observations } => {
                // An empty V2 generation would assert that closure evidence
                // exists while carrying none — the same fabrication as reading
                // a V1 status as a closure, reached from the other direction.
                if observations.is_empty() || observations.len() > MAX_OBSERVATION_REFS {
                    return Err(ObservationValidationError::BoundsExceeded(
                        "closure_generation.observations",
                    ));
                }
                let mut seen = BTreeSet::new();
                for recorded in observations {
                    validate_name(&recorded.observation_id, "observations.observation_id")?;
                    if recorded.observed_at == 0 {
                        return Err(ObservationValidationError::InvalidField(
                            "observations.observed_at",
                        ));
                    }
                    if !seen.insert(recorded.observation_id.as_str()) {
                        return Err(ObservationValidationError::DuplicateReference(
                            "observations.observation_id",
                        ));
                    }
                    recorded.projection.validate()?;
                }
                Ok(())
            }
        }
    }
}

// ── Raw chain evidence behind a normalized closure (TUP-NE-003) ─────────────

/// The only chain-evidence record version understood by this release.
pub const CHAIN_CLOSURE_EVIDENCE_VERSION: u16 = 1;

/// Maximum evidence references retained for one normalized closure.
pub const MAX_CHAIN_EVIDENCE_REFS: usize = 32;

/// The chain-native evidence one normalized closure observation was derived from.
///
/// Normalization maps five different chain events onto one projection, and that
/// mapping necessarily drops chain-specific detail — a log index, an input
/// index, an event sequence number. This record keeps the way back: it names
/// the native event family and the locators that address the exact evidence on
/// its own chain, so a normalized closure can always be taken back to what the
/// chain actually said. It travels with the observation that carries the
/// projection and is reached by the same identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainClosureEvidenceRecord {
    /// Record version; see [`CHAIN_CLOSURE_EVIDENCE_VERSION`].
    pub schema_version: u16,
    /// The observation whose projection this evidence backs.
    pub observation_id: String,
    /// The native event family, equal to the projection's `closure_kind`.
    pub native_event_kind: String,
    /// Chain-native locators addressing the exact evidence, in a stable order.
    /// Never empty: a normalized closure with no way back to its chain evidence
    /// is an assertion rather than an observation.
    pub evidence_refs: Vec<String>,
    /// Digest of the exact source bytes the projection was normalized from.
    pub raw_event_digest: ContentDigest,
}

impl ChainClosureEvidenceRecord {
    /// Fail-closed validation for ingestion and storage boundaries.
    ///
    /// # Errors
    ///
    /// Returns [`ObservationValidationError`] for an unsupported version, a
    /// malformed identifier, an empty or over-long reference set, a duplicated
    /// reference, or an all-zero digest.
    pub fn validate(&self) -> Result<(), ObservationValidationError> {
        if self.schema_version != CHAIN_CLOSURE_EVIDENCE_VERSION {
            return Err(ObservationValidationError::UnsupportedVersion(
                self.schema_version,
            ));
        }
        validate_name(&self.observation_id, "evidence.observation_id")?;
        validate_name(&self.native_event_kind, "evidence.native_event_kind")?;
        if self.evidence_refs.is_empty() || self.evidence_refs.len() > MAX_CHAIN_EVIDENCE_REFS {
            return Err(ObservationValidationError::BoundsExceeded(
                "evidence.evidence_refs",
            ));
        }
        validate_refs(&self.evidence_refs, "evidence.evidence_refs")?;
        if self.raw_event_digest == [0; 32] {
            return Err(ObservationValidationError::InvalidDigest(
                "evidence.raw_event_digest",
            ));
        }
        Ok(())
    }
}

// ── Conflict search, always qualified by index freshness (TUP-NE-003) ───────

/// A closure competing for the same consumed state under a different successor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompetingClosureReading {
    /// Stable chain identifier the competitor was observed on.
    pub chain_id: String,
    /// Stable network identifier the competitor was observed on.
    pub network_id: String,
    /// Registered closure family name of the competitor.
    pub closure_kind: String,
    /// The competitor's chain-native closure identity, hex-encoded.
    pub closure_identity_hex: String,
    /// The successor commitment the competitor favours.
    pub successor_commitment_hex: String,
}

/// What a search for closures competing for one consumed state found.
///
/// There is deliberately no `Unique` variant, and adding one would be a
/// protocol claim this plane cannot make. A search covers the index; the index
/// covers the chain only up to the tip it has reached. Nothing an indexer can
/// observe separates "no competitor exists" from "no competitor has been
/// indexed yet", so the negative outcome names the range it actually covered
/// and carries the freshness reading that bounds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClosureConflictSearchOutcome {
    /// At least one competing closure was observed. Equivocation is recorded,
    /// never resolved here: choosing between competitors is not Tuppira's call.
    CompetingClosuresObserved {
        /// Every competitor found, in the order the search returned them.
        competitors: Vec<CompetingClosureReading>,
    },
    /// No competitor was observed within the range the index covers. This is
    /// not uniqueness, and the freshness reading is what says so.
    NoCompetitorObservedWithinIndexedRange {
        /// How far behind the chain the index was when the search ran.
        index_freshness: IndexFreshnessReading,
    },
}

impl ClosureConflictSearchOutcome {
    /// Blocks the index had not reached when the search ran, when that distance
    /// is known.
    ///
    /// [`ObservedOrMissing::Missing`] for a search whose freshness could not be
    /// measured against a checkpoint — the unexamined range is unbounded, which
    /// is weaker than a large lag, not stronger.
    #[must_use]
    pub fn unexamined_lag_blocks(&self) -> ObservedOrMissing<u64> {
        match self {
            Self::CompetingClosuresObserved { .. } => ObservedOrMissing::Missing {
                reasons: vec!["a competitor was observed, so no lag qualifies the result".into()],
            },
            Self::NoCompetitorObservedWithinIndexedRange { index_freshness } => {
                index_freshness.lag_blocks.clone()
            }
        }
    }
}

/// Search recorded closure observations for competitors of one consumed state.
///
/// A competitor is an observation of the *same* consumed state favouring a
/// *different* successor commitment: that pairing is what portable
/// non-equivocation forbids, and observing it is the whole reason this plane
/// indexes closures. Observations of the same successor are the same closure
/// seen again and are not competitors.
///
/// The result is never a uniqueness claim. See
/// [`ClosureConflictSearchOutcome`].
#[must_use]
pub fn assess_closure_conflicts(
    consumed_state: &ConsumedStateReading,
    successor_commitment_hex: &str,
    observed: &[SourceClosureObservationProjectionV1],
    index_freshness: &IndexFreshnessReading,
) -> ClosureConflictSearchOutcome {
    let mut competitors: Vec<CompetingClosureReading> = Vec::new();
    for projection in observed {
        if &projection.consumed_state != consumed_state {
            continue;
        }
        if projection.closure_identity.successor_commitment_hex == successor_commitment_hex {
            continue;
        }
        let competitor = CompetingClosureReading {
            chain_id: projection.chain_id.clone(),
            network_id: projection.network_id.clone(),
            closure_kind: projection.closure_identity.closure_kind.clone(),
            closure_identity_hex: projection.closure_identity.closure_identity_hex.clone(),
            successor_commitment_hex: projection
                .closure_identity
                .successor_commitment_hex
                .clone(),
        };
        if !competitors.contains(&competitor) {
            competitors.push(competitor);
        }
    }
    if competitors.is_empty() {
        return ClosureConflictSearchOutcome::NoCompetitorObservedWithinIndexedRange {
            index_freshness: index_freshness.clone(),
        };
    }
    ClosureConflictSearchOutcome::CompetingClosuresObserved { competitors }
}

fn validate_name(value: &str, field: &'static str) -> Result<(), ObservationValidationError> {
    if value.trim().is_empty() || value.len() > MAX_NATIVE_IDENTITY_HEX_LEN || value.contains('\0') {
        return Err(ObservationValidationError::InvalidField(field));
    }
    Ok(())
}

fn validate_hash_hex(value: &str, field: &'static str) -> Result<(), ObservationValidationError> {
    if value.len() != HASH_HEX_LEN || !is_lowercase_hex(value) {
        return Err(ObservationValidationError::InvalidField(field));
    }
    if value.bytes().all(|byte| byte == b'0') {
        return Err(ObservationValidationError::InvalidDigest(field));
    }
    Ok(())
}

fn validate_native_hex(value: &str, field: &'static str) -> Result<(), ObservationValidationError> {
    if value.is_empty() || value.len() > MAX_NATIVE_IDENTITY_HEX_LEN {
        return Err(ObservationValidationError::BoundsExceeded(field));
    }
    if !value.len().is_multiple_of(2) || !is_lowercase_hex(value) {
        return Err(ObservationValidationError::InvalidField(field));
    }
    Ok(())
}

fn is_lowercase_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_refs(values: &[String], field: &'static str) -> Result<(), ObservationValidationError> {
    if values.len() > MAX_OBSERVATION_REFS {
        return Err(ObservationValidationError::BoundsExceeded(field));
    }
    for (index, value) in values.iter().enumerate() {
        validate_name(value, field)?;
        if values[..index].contains(value) {
            return Err(ObservationValidationError::DuplicateReference(field));
        }
    }
    Ok(())
}

fn validate_reasons(
    reasons: &[String],
    field: &'static str,
) -> Result<(), ObservationValidationError> {
    if reasons.is_empty() {
        return Err(ObservationValidationError::InvalidField(field));
    }
    validate_refs(reasons, field)
}

#[cfg(test)]
mod closure_observation_tests {
    use super::*;

    const TRANSITION_ID: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";
    const SUCCESSOR: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn missing<T>(reason: &str) -> ObservedOrMissing<T> {
        ObservedOrMissing::Missing {
            reasons: vec![reason.to_string()],
        }
    }

    fn observed<T>(value: T) -> ObservedOrMissing<T> {
        ObservedOrMissing::Observed { value }
    }

    fn checkpoint() -> ObservedCheckpointReading {
        ObservedCheckpointReading {
            block_height: 900,
            block_id_hex: "abcdef01".to_string(),
        }
    }

    fn settlement(reported: SourceReportedSettlement) -> ClosureSettlementReading {
        ClosureSettlementReading {
            finality_policy: "confirmations".to_string(),
            observed_depth: 64,
            required_depth: 12,
            reported_settlement: reported,
        }
    }

    fn foreign_verdict() -> ExternalClosureVerificationReading {
        ExternalClosureVerificationReading {
            verifier_id: "csv-ethereum".to_string(),
            trust_mode: ClosureTrustMode::FullNode,
            proof_validity: ClosureDimensionStatus::Satisfied,
            checkpoint_finality: ClosureDimensionStatus::Satisfied,
            checkpoint_freshness: ClosureDimensionStatus::Indeterminate,
            source_closure: ClosureDimensionStatus::Satisfied,
            reason_codes: vec!["closure.source.satisfied".to_string()],
            report_digest: [7; 32],
        }
    }

    fn revocation(status: RetractionStatus) -> ClosureRevocationReading {
        ClosureRevocationReading {
            retraction_status: status,
            reorg_id: match status {
                RetractionStatus::Retracted => Some("reorg-17".to_string()),
                RetractionStatus::Active => None,
            },
            reported_at: 1_760_000_000,
        }
    }

    /// Everything reported, so no facet is Unknown. Individual tests remove one
    /// facet at a time rather than rebuilding this by hand.
    fn fully_reported() -> SourceClosureObservationProjectionV1 {
        SourceClosureObservationProjectionV1 {
            schema_version: CLOSURE_OBSERVATION_PROFILE_VERSION,
            consumed_state: ConsumedStateReading {
                transition_id_hex: TRANSITION_ID.to_string(),
                output_index: 0,
                state_type: 1,
            },
            closure_identity: ClosureIdentityReading {
                closure_kind: "evm-nullifier".to_string(),
                closure_identity_hex: "0f1e2d3c".to_string(),
                successor_commitment_hex: SUCCESSOR.to_string(),
            },
            chain_id: "ethereum".to_string(),
            network_id: "sepolia".to_string(),
            successor_output_refs: vec!["output:0".to_string()],
            observed_checkpoint: observed(checkpoint()),
            settlement: observed(settlement(SourceReportedSettlement::Final)),
            external_verification: observed(foreign_verdict()),
            revocation: observed(revocation(RetractionStatus::Active)),
            index_freshness: IndexFreshnessReading {
                indexed_tip_height: 1_000,
                indexed_tip_block_id_hex: "beef".to_string(),
                indexed_tip_observed_at: 1_760_000_100,
                lag_blocks: observed(100),
            },
        }
    }

    #[test]
    fn a_fully_reported_projection_validates() {
        assert_eq!(fully_reported().validate(), Ok(()));
    }

    // ── The five states are distinct and none collapses into another ──────────

    #[test]
    fn settlement_alone_never_establishes_a_foreign_verdict() {
        let mut projection = fully_reported();
        projection.external_verification = missing("no verifier reported one");

        let states = projection.established_states();
        assert!(states.contains(&ClosureObservationState::Final));
        assert!(!states.contains(&ClosureObservationState::VerifiedElsewhere));
    }

    #[test]
    fn a_foreign_verdict_never_establishes_settlement() {
        let mut projection = fully_reported();
        projection.settlement = observed(settlement(SourceReportedSettlement::NotYetFinal));

        let states = projection.established_states();
        assert!(states.contains(&ClosureObservationState::VerifiedElsewhere));
        assert!(!states.contains(&ClosureObservationState::Final));
    }

    #[test]
    fn a_satisfied_verdict_dimension_is_not_read_as_settlement() {
        // Every dimension the verifier could report is Satisfied, which is the
        // shape most likely to be mistaken for "final".
        let mut projection = fully_reported();
        projection.settlement = missing("source did not report a finality rule");
        projection.observed_checkpoint = missing("no checkpoint disclosed");
        projection.index_freshness.lag_blocks = missing("no checkpoint to measure against");

        let states = projection.established_states();
        assert!(states.contains(&ClosureObservationState::VerifiedElsewhere));
        assert!(!states.contains(&ClosureObservationState::Final));
        assert!(states.contains(&ClosureObservationState::Unknown));
    }

    #[test]
    fn a_retraction_does_not_erase_what_was_already_established() {
        let mut projection = fully_reported();
        projection.revocation = observed(revocation(RetractionStatus::Retracted));

        let states = projection.established_states();
        // The retraction is recorded alongside the history, not in place of it:
        // an investigator must still see that this was once reported final.
        assert!(states.contains(&ClosureObservationState::Revoked));
        assert!(states.contains(&ClosureObservationState::Final));
        assert!(states.contains(&ClosureObservationState::VerifiedElsewhere));
        assert!(states.contains(&ClosureObservationState::Observed));
    }

    #[test]
    fn an_active_report_is_not_a_retraction() {
        let states = fully_reported().established_states();
        assert!(!states.contains(&ClosureObservationState::Revoked));
    }

    #[test]
    fn an_unreported_facet_is_unknown_and_displaces_nothing() {
        let mut projection = fully_reported();
        projection.revocation = missing("source exposes no retraction feed");

        let states = projection.established_states();
        assert!(states.contains(&ClosureObservationState::Unknown));
        assert!(!states.contains(&ClosureObservationState::Revoked));
        assert!(states.contains(&ClosureObservationState::Final));
        assert!(states.contains(&ClosureObservationState::VerifiedElsewhere));
        assert!(states.contains(&ClosureObservationState::Observed));
    }

    #[test]
    fn a_bare_observation_establishes_only_observed_and_unknown() {
        let mut projection = fully_reported();
        projection.observed_checkpoint = missing("not disclosed");
        projection.settlement = missing("not disclosed");
        projection.external_verification = missing("not disclosed");
        projection.revocation = missing("not disclosed");
        projection.index_freshness.lag_blocks = missing("no checkpoint to measure against");

        assert_eq!(
            projection.established_states(),
            BTreeSet::from([
                ClosureObservationState::Observed,
                ClosureObservationState::Unknown,
            ])
        );
        assert_eq!(projection.validate(), Ok(()));
    }

    #[test]
    fn the_five_states_are_distinct_values() {
        let states = BTreeSet::from([
            ClosureObservationState::Observed,
            ClosureObservationState::VerifiedElsewhere,
            ClosureObservationState::Final,
            ClosureObservationState::Revoked,
            ClosureObservationState::Unknown,
        ]);
        assert_eq!(states.len(), 5);
    }

    // ── Tuppira never holds the verdict ───────────────────────────────────────

    #[test]
    fn a_verdict_attributed_to_the_observation_plane_is_rejected() {
        let mut verdict = foreign_verdict();
        verdict.verifier_id = "Tuppira-indexer".to_string();
        let mut projection = fully_reported();
        projection.external_verification = observed(verdict);

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField(
                "external_verification.verifier_id"
            ))
        );
    }

    #[test]
    fn a_relayed_verdict_without_reason_codes_is_rejected() {
        let mut verdict = foreign_verdict();
        verdict.reason_codes.clear();
        let mut projection = fully_reported();
        projection.external_verification = observed(verdict);

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField(
                "external_verification.reason_codes"
            ))
        );
    }

    #[test]
    fn a_relayed_verdict_without_report_bytes_is_rejected() {
        let mut verdict = foreign_verdict();
        verdict.report_digest = [0; 32];
        let mut projection = fully_reported();
        projection.external_verification = observed(verdict);

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidDigest(
                "external_verification.report_digest"
            ))
        );
    }

    #[test]
    fn an_indeterminate_dimension_survives_a_round_trip_unmerged() {
        let projection = fully_reported();
        let encoded = serde_json::to_string(&projection).expect("serialize");
        let decoded: SourceClosureObservationProjectionV1 =
            serde_json::from_str(&encoded).expect("deserialize");

        let ObservedOrMissing::Observed { value } = &decoded.external_verification else {
            panic!("verdict facet lost in round trip");
        };
        assert_eq!(
            value.checkpoint_freshness,
            ClosureDimensionStatus::Indeterminate
        );
        assert_eq!(value.source_closure, ClosureDimensionStatus::Satisfied);
        assert_eq!(decoded, projection);
    }

    // ── Freshness is carried, never implied ───────────────────────────────────

    #[test]
    fn a_lag_that_does_not_match_the_tip_is_rejected() {
        let mut projection = fully_reported();
        projection.index_freshness.lag_blocks = observed(0);

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField(
                "index_freshness.lag_blocks"
            ))
        );
    }

    #[test]
    fn a_checkpoint_beyond_the_indexed_tip_is_rejected() {
        let mut projection = fully_reported();
        projection.index_freshness.indexed_tip_height = 800;

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField(
                "index_freshness.indexed_tip_height"
            ))
        );
    }

    #[test]
    fn a_lag_without_a_checkpoint_is_rejected() {
        let mut projection = fully_reported();
        projection.observed_checkpoint = missing("not disclosed");
        projection.settlement = missing("no checkpoint to settle");

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField(
                "index_freshness.lag_blocks"
            ))
        );
    }

    #[test]
    fn settlement_without_a_checkpoint_is_rejected() {
        let mut projection = fully_reported();
        projection.observed_checkpoint = missing("not disclosed");
        projection.index_freshness.lag_blocks = missing("no checkpoint to measure against");

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField("settlement"))
        );
    }

    #[test]
    fn a_missing_facet_must_say_why() {
        let mut projection = fully_reported();
        projection.settlement = ObservedOrMissing::Missing { reasons: vec![] };

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField("settlement"))
        );
    }

    // ── A source cannot report more than its own rule supports ────────────────

    #[test]
    fn a_shallow_read_cannot_be_reported_final() {
        let mut shallow = settlement(SourceReportedSettlement::Final);
        shallow.observed_depth = 3;
        shallow.required_depth = 12;
        let mut projection = fully_reported();
        projection.settlement = observed(shallow);

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField(
                "settlement.reported_settlement"
            ))
        );
    }

    #[test]
    fn a_shallow_read_may_be_reported_not_yet_final() {
        let mut shallow = settlement(SourceReportedSettlement::NotYetFinal);
        shallow.observed_depth = 3;
        let mut projection = fully_reported();
        projection.settlement = observed(shallow);

        assert_eq!(projection.validate(), Ok(()));
        assert!(
            !projection
                .established_states()
                .contains(&ClosureObservationState::Final)
        );
    }

    #[test]
    fn an_active_report_cannot_name_the_reorganization_that_withdrew_it() {
        let mut contradictory = revocation(RetractionStatus::Active);
        contradictory.reorg_id = Some("reorg-17".to_string());
        let mut projection = fully_reported();
        projection.revocation = observed(contradictory);

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField("revocation.reorg_id"))
        );
    }

    // ── Identities are the source's, and must be well formed ──────────────────

    #[test]
    fn a_malformed_consumed_state_identity_is_rejected() {
        let mut projection = fully_reported();
        projection.consumed_state.transition_id_hex = "1111".to_string();

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidField(
                "consumed_state.transition_id_hex"
            ))
        );
    }

    #[test]
    fn a_zero_successor_commitment_is_rejected() {
        let mut projection = fully_reported();
        projection.closure_identity.successor_commitment_hex = "0".repeat(64);

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::InvalidDigest(
                "closure_identity.successor_commitment_hex"
            ))
        );
    }

    #[test]
    fn a_duplicated_successor_output_is_rejected() {
        let mut projection = fully_reported();
        projection.successor_output_refs = vec!["output:0".to_string(), "output:0".to_string()];

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::DuplicateReference(
                "successor_output_refs"
            ))
        );
    }

    #[test]
    fn an_unsupported_projection_version_is_rejected() {
        let mut projection = fully_reported();
        projection.schema_version = CLOSURE_OBSERVATION_PROFILE_VERSION + 1;

        assert_eq!(
            projection.validate(),
            Err(ObservationValidationError::UnsupportedVersion(
                CLOSURE_OBSERVATION_PROFILE_VERSION + 1
            ))
        );
    }

    #[test]
    fn an_unknown_field_is_rejected_rather_than_ignored() {
        let mut value = serde_json::to_value(fully_reported()).expect("serialize");
        value["closure_verified"] = serde_json::Value::Bool(true);

        let decoded = serde_json::from_value::<SourceClosureObservationProjectionV1>(value);
        assert!(decoded.is_err(), "an injected field must not be ignored");
    }

    #[test]
    fn the_profile_id_names_its_own_projection_version() {
        assert!(SOURCE_CLOSURE_OBSERVATION_PROFILE_ID.ends_with(".v1"));
        assert_eq!(CLOSURE_OBSERVATION_PROFILE_VERSION, 1);
    }

    // ── The digest commits to exactly these bytes (TUP-NE-002) ────────────────

    #[test]
    fn closure_observation_digest_is_pinned_to_its_bytes() {
        // A literal, so reordering a field or changing an encoding breaks this
        // test rather than silently changing what an observation commits to.
        assert_eq!(
            fully_reported().normalized_payload_digest().map(hex::encode),
            Ok("2ea43a8ba6ea8db9d3325223a5c70c5b49ad84be1100fa770b01e291bbb60c16".to_string())
        );
    }

    #[test]
    fn a_changed_facet_changes_the_digest() {
        let baseline = fully_reported().normalized_payload_digest();
        let mut altered = fully_reported();
        altered.external_verification = missing("no verifier reported one");

        assert_ne!(altered.normalized_payload_digest(), baseline);
    }

    #[test]
    fn the_digest_is_domain_separated_from_bare_json() {
        let projection = fully_reported();
        let bare = Sha256::digest(serde_json::to_vec(&projection).expect("serialize"));

        assert_ne!(
            projection.normalized_payload_digest(),
            Ok(<[u8; 32]>::from(bare))
        );
    }

    // ── A subject with no closure evidence reports Unknown, never a V2 state ──

    fn recorded(id: &str, projection: SourceClosureObservationProjectionV1) -> RecordedSourceClosureObservationV1 {
        RecordedSourceClosureObservationV1 {
            observation_id: id.to_string(),
            observed_at: 1_760_000_200,
            record_retraction_status: RetractionStatus::Active,
            projection,
        }
    }

    #[test]
    fn a_subject_without_closure_evidence_is_unknown_and_nothing_else() {
        let account = SubjectClosureProjectionV1::pre_closure("sanad:legacy-1");

        assert_eq!(account.validate(), Ok(()));
        assert_eq!(
            account.established_states(),
            BTreeSet::from([ClosureObservationState::Unknown])
        );
        // Not `Observed`: no closure was reported, so nothing was observed to
        // report on. This is the distinction a fabricated V2 status would erase.
        assert!(
            !account
                .established_states()
                .contains(&ClosureObservationState::Observed)
        );
    }

    #[test]
    fn a_pre_closure_subject_must_say_why_no_closure_exists() {
        let mut account = SubjectClosureProjectionV1::pre_closure("sanad:legacy-1");
        account.closure_generation = ClosureProfileGeneration::PreClosure {
            reasons: Vec::new(),
        };

        assert_eq!(
            account.validate(),
            Err(ObservationValidationError::InvalidField(
                "closure_generation.reasons"
            ))
        );
    }

    #[test]
    fn a_v2_generation_carrying_no_observation_is_rejected() {
        let account = SubjectClosureProjectionV1 {
            schema_version: SUBJECT_CLOSURE_PROFILE_VERSION,
            subject_ref: "sanad:1".to_string(),
            closure_generation: ClosureProfileGeneration::SourceClosureV2 {
                observations: Vec::new(),
            },
        };

        assert_eq!(
            account.validate(),
            Err(ObservationValidationError::BoundsExceeded(
                "closure_generation.observations"
            ))
        );
    }

    #[test]
    fn disagreeing_observations_both_survive_the_union() {
        let mut revoked = fully_reported();
        revoked.revocation = observed(revocation(RetractionStatus::Retracted));
        let account = SubjectClosureProjectionV1 {
            schema_version: SUBJECT_CLOSURE_PROFILE_VERSION,
            subject_ref: "sanad:1".to_string(),
            closure_generation: ClosureProfileGeneration::SourceClosureV2 {
                observations: vec![
                    recorded("obs:closure:2", revoked),
                    recorded("obs:closure:1", fully_reported()),
                ],
            },
        };

        assert_eq!(account.validate(), Ok(()));
        let states = account.established_states();
        // The retraction does not delete the earlier final report, and the
        // final report does not hide the retraction.
        assert!(states.contains(&ClosureObservationState::Revoked));
        assert!(states.contains(&ClosureObservationState::Final));
        assert!(states.contains(&ClosureObservationState::VerifiedElsewhere));
    }

    #[test]
    fn a_duplicated_observation_identifier_is_rejected() {
        let account = SubjectClosureProjectionV1 {
            schema_version: SUBJECT_CLOSURE_PROFILE_VERSION,
            subject_ref: "sanad:1".to_string(),
            closure_generation: ClosureProfileGeneration::SourceClosureV2 {
                observations: vec![
                    recorded("obs:closure:1", fully_reported()),
                    recorded("obs:closure:1", fully_reported()),
                ],
            },
        };

        assert_eq!(
            account.validate(),
            Err(ObservationValidationError::DuplicateReference(
                "observations.observation_id"
            ))
        );
    }

    #[test]
    fn an_invalid_nested_projection_fails_the_whole_account() {
        let mut malformed = fully_reported();
        malformed.consumed_state.transition_id_hex = "1111".to_string();
        let account = SubjectClosureProjectionV1 {
            schema_version: SUBJECT_CLOSURE_PROFILE_VERSION,
            subject_ref: "sanad:1".to_string(),
            closure_generation: ClosureProfileGeneration::SourceClosureV2 {
                observations: vec![recorded("obs:closure:1", malformed)],
            },
        };

        assert_eq!(
            account.validate(),
            Err(ObservationValidationError::InvalidField(
                "consumed_state.transition_id_hex"
            ))
        );
    }

    #[test]
    fn the_record_retraction_is_kept_apart_from_the_source_revocation() {
        // The collector withdrew the record; the source never withdrew the
        // closure. Reading either as the other is the collapse this separation
        // prevents, so the two fields must round-trip independently.
        let recorded = RecordedSourceClosureObservationV1 {
            record_retraction_status: RetractionStatus::Retracted,
            ..recorded("obs:closure:1", fully_reported())
        };
        let encoded = serde_json::to_string(&recorded).expect("serialize");
        let decoded: RecordedSourceClosureObservationV1 =
            serde_json::from_str(&encoded).expect("deserialize");

        assert_eq!(decoded.record_retraction_status, RetractionStatus::Retracted);
        let ObservedOrMissing::Observed { value } = &decoded.projection.revocation else {
            panic!("source revocation facet lost");
        };
        assert_eq!(value.retraction_status, RetractionStatus::Active);
    }

    #[test]
    fn the_subject_profile_id_names_its_own_projection_version() {
        assert!(SUBJECT_CLOSURE_PROJECTION_PROFILE_ID.ends_with(".v1"));
        assert_eq!(SUBJECT_CLOSURE_PROFILE_VERSION, 1);
    }

    // ── Raw chain evidence stays reachable (TUP-NE-003) ───────────────────────

    fn evidence() -> ChainClosureEvidenceRecord {
        ChainClosureEvidenceRecord {
            schema_version: CHAIN_CLOSURE_EVIDENCE_VERSION,
            observation_id: "obs:closure:1".to_string(),
            native_event_kind: "evm-nullifier".to_string(),
            evidence_refs: vec![
                "ethereum:sepolia:transaction:aa".to_string(),
                "ethereum:sepolia:log:3".to_string(),
            ],
            raw_event_digest: [6; 32],
        }
    }

    #[test]
    fn chain_evidence_round_trips_and_validates() {
        let record = evidence();
        assert_eq!(record.validate(), Ok(()));
        let encoded = serde_json::to_string(&record).expect("serialize");
        assert_eq!(
            serde_json::from_str::<ChainClosureEvidenceRecord>(&encoded).ok(),
            Some(record)
        );
    }

    #[test]
    fn a_normalized_closure_without_chain_evidence_is_rejected() {
        // A normalized closure nobody can trace back to a chain event is an
        // assertion wearing an observation's shape.
        let mut record = evidence();
        record.evidence_refs.clear();

        assert_eq!(
            record.validate(),
            Err(ObservationValidationError::BoundsExceeded(
                "evidence.evidence_refs"
            ))
        );
    }

    #[test]
    fn chain_evidence_without_source_bytes_is_rejected() {
        let mut record = evidence();
        record.raw_event_digest = [0; 32];

        assert_eq!(
            record.validate(),
            Err(ObservationValidationError::InvalidDigest(
                "evidence.raw_event_digest"
            ))
        );
    }

    #[test]
    fn duplicated_evidence_references_are_rejected() {
        let mut record = evidence();
        record.evidence_refs = vec!["ethereum:sepolia:log:3".to_string(); 2];

        assert_eq!(
            record.validate(),
            Err(ObservationValidationError::DuplicateReference(
                "evidence.evidence_refs"
            ))
        );
    }

    // ── Absence of a conflict is never uniqueness (TUP-NE-003) ────────────────

    fn freshness(lag: ObservedOrMissing<u64>) -> IndexFreshnessReading {
        IndexFreshnessReading {
            indexed_tip_height: 1_000,
            indexed_tip_block_id_hex: "beef".to_string(),
            indexed_tip_observed_at: 1_760_000_100,
            lag_blocks: lag,
        }
    }

    #[test]
    fn a_search_that_found_nothing_reports_the_range_it_covered() {
        let projection = fully_reported();
        let outcome = assess_closure_conflicts(
            &projection.consumed_state,
            &projection.closure_identity.successor_commitment_hex,
            std::slice::from_ref(&projection),
            &freshness(observed(100)),
        );

        // The same closure seen again is not a competitor.
        let ClosureConflictSearchOutcome::NoCompetitorObservedWithinIndexedRange {
            index_freshness,
        } = &outcome
        else {
            panic!("re-observing one closure must not read as a conflict");
        };
        assert_eq!(index_freshness.lag_blocks, observed(100));
        assert_eq!(outcome.unexamined_lag_blocks(), observed(100));
    }

    #[test]
    fn a_stale_index_finding_nothing_still_reports_only_what_it_covered() {
        // 900 blocks behind: whatever this search proves, it is not uniqueness.
        let projection = fully_reported();
        let outcome = assess_closure_conflicts(
            &projection.consumed_state,
            &projection.closure_identity.successor_commitment_hex,
            &[],
            &freshness(observed(900)),
        );

        assert!(matches!(
            outcome,
            ClosureConflictSearchOutcome::NoCompetitorObservedWithinIndexedRange { .. }
        ));
        assert_eq!(outcome.unexamined_lag_blocks(), observed(900));
    }

    #[test]
    fn an_unmeasurable_lag_stays_missing_rather_than_reading_as_zero() {
        let projection = fully_reported();
        let outcome = assess_closure_conflicts(
            &projection.consumed_state,
            &projection.closure_identity.successor_commitment_hex,
            &[],
            &freshness(missing("no checkpoint to measure against")),
        );

        // Weaker than a large lag, not stronger: the unexamined range is
        // unbounded, so it must not collapse to a number.
        assert!(matches!(
            outcome.unexamined_lag_blocks(),
            ObservedOrMissing::Missing { .. }
        ));
    }

    #[test]
    fn the_outcome_type_has_no_uniqueness_variant() {
        // A negative search result must not be serializable as a positive
        // claim, so the tag a consumer branches on is pinned here.
        let outcome = ClosureConflictSearchOutcome::NoCompetitorObservedWithinIndexedRange {
            index_freshness: freshness(observed(100)),
        };
        let encoded = serde_json::to_value(&outcome).expect("serialize");

        assert_eq!(
            encoded["outcome"],
            serde_json::json!("no_competitor_observed_within_indexed_range")
        );
        let text = encoded.to_string().to_ascii_lowercase();
        assert!(!text.contains("unique"));
        assert!(!text.contains("verified"));
    }

    #[test]
    fn a_second_successor_for_one_consumed_state_is_a_conflict() {
        let first = fully_reported();
        let mut second = fully_reported();
        second.closure_identity.successor_commitment_hex = "33".repeat(32);

        let outcome = assess_closure_conflicts(
            &first.consumed_state,
            &first.closure_identity.successor_commitment_hex,
            &[first.clone(), second],
            &freshness(observed(100)),
        );

        let ClosureConflictSearchOutcome::CompetingClosuresObserved { competitors } = &outcome
        else {
            panic!("two successors for one consumed state must be reported as a conflict");
        };
        assert_eq!(competitors.len(), 1);
        assert_eq!(competitors[0].successor_commitment_hex, "33".repeat(32));
        // A conflict is not qualified by a lag: it was observed, not not-found.
        assert!(matches!(
            outcome.unexamined_lag_blocks(),
            ObservedOrMissing::Missing { .. }
        ));
    }

    #[test]
    fn a_closure_of_a_different_consumed_state_is_not_a_conflict() {
        let subject = fully_reported();
        let mut unrelated = fully_reported();
        unrelated.consumed_state.output_index = 1;
        unrelated.closure_identity.successor_commitment_hex = "33".repeat(32);

        let outcome = assess_closure_conflicts(
            &subject.consumed_state,
            &subject.closure_identity.successor_commitment_hex,
            &[unrelated],
            &freshness(observed(100)),
        );

        assert!(matches!(
            outcome,
            ClosureConflictSearchOutcome::NoCompetitorObservedWithinIndexedRange { .. }
        ));
    }
}
