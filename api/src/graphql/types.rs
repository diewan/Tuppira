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
    /// Version of the *view* rules that produced `established_states`,
    /// `reorg_standing`, and `read_index_freshness`. Distinct from
    /// `profile_version`, which versions `payload` and did not change when
    /// these three were added (TUP-NE-004).
    pub view_version: i32,
    /// Every state this one observation **still** establishes, as a set and
    /// never a badge.
    ///
    /// This is the set after [`tuppira_shared::established_states_under_reorg`]
    /// has been applied, so it is never `final` for a closure whose history a
    /// reorganization replaced. It is therefore not the same set as
    /// `payload.established_states` would give: the rules only ever move a read
    /// toward uncertainty, and a consumer that wants the source's original
    /// statement reads `payload`.
    pub established_states: Vec<String>,
    /// Where this observation stands after the recorded reorganizations.
    pub reorg_standing: ClosureReorgStandingGql,
    /// How far behind the chain the index is **now** for this observation's
    /// chain and network.
    ///
    /// Distinct from the freshness inside `payload`, which is what the
    /// collector measured when the projection was produced. A view carrying
    /// only the second would present a year-old closure as a current one.
    pub read_index_freshness: IndexFreshnessGql,
    /// The projection exactly as stored, under the profile named above.
    pub payload: JsonValueScalar,
    /// The way back to the chain event this closure was normalized from.
    /// Absent for a closure relayed by a source that is not a chain.
    pub chain_evidence: Option<ChainClosureEvidenceGql>,
}

/// What a reorganization did to one closure observation, and to its ancestry.
///
/// The two lists are separate because they answer different questions. An
/// observation's own orphaning is a statement about the block that carried it;
/// an orphaned ancestor is a statement about the ground beneath it. An
/// observation can carry both, so neither is folded into a single verdict.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureReorgStandingGql")]
pub struct ClosureReorgStandingGql {
    /// Whether a reorganization orphaned this observation itself.
    pub is_orphaned: bool,
    /// Whether it was reported to descend from an orphaned closure.
    pub descends_from_orphaned: bool,
    /// Whether the source reported the closure did not reappear. A superseded
    /// closure is *not* retracted: the source reported it again, and the
    /// superseding observation carries that statement.
    pub is_retracted: bool,
    /// Orphanings recorded against this observation, oldest first.
    pub own_orphanings: Vec<ClosureOrphaningGql>,
    /// Orphaned closures this one was reported to descend from, nearest first.
    pub orphaned_ancestors: Vec<OrphanedClosureAncestorGql>,
    /// `complete` or `truncated_at_depth`. A truncated walk is not a clean one:
    /// an orphaned ancestor beyond the bound would not appear above.
    pub ancestry_coverage: String,
    /// The depth the walk stopped at. Absent when the coverage is `complete`.
    pub ancestry_coverage_depth: Option<i64>,
}

/// One recorded orphaning of a closure observation.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureOrphaningGql")]
pub struct ClosureOrphaningGql {
    /// The reorganization the source attributed the orphaning to.
    pub reorg_id: String,
    /// When the collector recorded it.
    pub orphaned_at: i64,
    /// `superseded` or `retracted`.
    pub disposition: String,
    /// The observation carrying the replacement statement. Present only for
    /// `superseded`.
    pub superseding_observation_id: Option<String>,
    /// Why the source reported no replacement. Present only for `retracted`.
    pub retraction_reasons: Vec<String>,
}

/// An orphaned closure a later observation was reported to descend from.
///
/// The linkage is the one the observations express: this observation's consumed
/// state names the transition an earlier observation reported as its successor.
/// Tuppira does not decide whether that linkage is protocol-valid — only
/// Parwana's verifier can — so this is never evidence that the descendant is
/// grounded, only that its ground was replaced.
#[derive(SimpleObject, Clone)]
#[graphql(name = "OrphanedClosureAncestorGql")]
pub struct OrphanedClosureAncestorGql {
    pub observation_id: String,
    /// The successor commitment the linkage was followed through.
    pub successor_commitment_hex: String,
    /// Steps between this observation and the ancestor; `1` is the parent.
    pub depth: i64,
}

/// How far behind the chain an index is, as of one read.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureIndexFreshnessGql")]
pub struct IndexFreshnessGql {
    pub indexed_tip_height: i64,
    pub indexed_tip_block_id_hex: String,
    pub indexed_tip_observed_at: i64,
    /// Blocks between the observed checkpoint and the indexed tip. Absent when
    /// it could not be measured — a lag against nothing is not zero, and an
    /// absent lag is weaker evidence than a large one, never stronger.
    pub lag_blocks: Option<i64>,
    /// Why the lag is absent. Empty when `lag_blocks` is present.
    pub lag_unavailable_reasons: Vec<String>,
}

impl From<tuppira_shared::IndexFreshnessReading> for IndexFreshnessGql {
    fn from(value: tuppira_shared::IndexFreshnessReading) -> Self {
        let (lag_blocks, lag_unavailable_reasons) = match value.lag_blocks {
            tuppira_shared::ObservedOrMissing::Observed { value } => (Some(value as i64), Vec::new()),
            tuppira_shared::ObservedOrMissing::Missing { reasons } => (None, reasons),
        };
        Self {
            indexed_tip_height: value.indexed_tip_height as i64,
            indexed_tip_block_id_hex: value.indexed_tip_block_id_hex,
            indexed_tip_observed_at: value.indexed_tip_observed_at as i64,
            lag_blocks,
            lag_unavailable_reasons,
        }
    }
}

impl From<tuppira_shared::ClosureReorgStanding> for ClosureReorgStandingGql {
    fn from(value: tuppira_shared::ClosureReorgStanding) -> Self {
        let is_orphaned = value.is_orphaned();
        let descends_from_orphaned = value.descends_from_orphaned();
        let is_retracted = value.is_retracted();
        let (ancestry_coverage, ancestry_coverage_depth) = match value.ancestry_coverage {
            tuppira_shared::ClosureAncestryCoverage::Complete => ("complete", None),
            tuppira_shared::ClosureAncestryCoverage::TruncatedAtDepth { depth } => {
                ("truncated_at_depth", Some(i64::from(depth)))
            }
        };
        Self {
            is_orphaned,
            descends_from_orphaned,
            is_retracted,
            own_orphanings: value
                .own_orphanings
                .into_iter()
                .map(|orphaning| {
                    let (disposition, superseding_observation_id, retraction_reasons) =
                        match orphaning.disposition {
                            tuppira_shared::OrphanedClosureDisposition::Superseded {
                                superseding_observation_id,
                            } => ("superseded", Some(superseding_observation_id), Vec::new()),
                            tuppira_shared::OrphanedClosureDisposition::Retracted { reasons } => {
                                ("retracted", None, reasons)
                            }
                        };
                    ClosureOrphaningGql {
                        reorg_id: orphaning.reorg_id,
                        orphaned_at: orphaning.orphaned_at as i64,
                        disposition: disposition.to_string(),
                        superseding_observation_id,
                        retraction_reasons,
                    }
                })
                .collect(),
            orphaned_ancestors: value
                .orphaned_ancestors
                .into_iter()
                .map(|ancestor| OrphanedClosureAncestorGql {
                    observation_id: ancestor.observation_id,
                    successor_commitment_hex: ancestor.successor_commitment_hex,
                    depth: i64::from(ancestor.depth),
                })
                .collect(),
            ancestry_coverage: ancestry_coverage.to_string(),
            ancestry_coverage_depth,
        }
    }
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

impl From<tuppira_shared::ClosureObservationViewV1> for ClosureObservationProjectionV1 {
    fn from(value: tuppira_shared::ClosureObservationViewV1) -> Self {
        let established_states = value.established_states.iter().map(state_name).collect();
        let reorg_standing = value.reorg_standing.into();
        let read_index_freshness = value.read_index_freshness.into();
        let recorded = value.recorded;
        let projection = recorded.projection;
        Self {
            observation_id: recorded.observation_id,
            observed_at: recorded.observed_at as i64,
            record_retraction_status: state_name(&recorded.record_retraction_status),
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
            view_version: i32::from(tuppira_shared::CLOSURE_OBSERVATION_VIEW_VERSION),
            established_states,
            reorg_standing,
            read_index_freshness,
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
///
/// `established_states` is the union of the per-observation sets *after* each
/// has had its reorganization standing applied (TUP-NE-004). Taking the union
/// of the stored sets instead would let a settlement withdrawn from every
/// observation individually reappear in the subject's account.
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

impl From<tuppira_shared::SubjectClosureViewV1> for SubjectClosureProjectionV1Gql {
    fn from(value: tuppira_shared::SubjectClosureViewV1) -> Self {
        let established_states = value.established_states().iter().map(state_name).collect();
        let (generation, pre_closure_reasons, observations) = match value.closure_generation {
            tuppira_shared::ClosureViewGeneration::PreClosure { reasons } => {
                ("pre_closure", reasons, Vec::new())
            }
            tuppira_shared::ClosureViewGeneration::SourceClosureV2 { observations } => (
                "source_closure_v2",
                Vec::new(),
                observations.into_iter().map(Into::into).collect(),
            ),
        };
        Self {
            schema_version: i32::from(value.schema_version),
            profile_id: tuppira_shared::SUBJECT_CLOSURE_VIEW_PROFILE_ID.to_string(),
            subject_ref: value.subject_ref,
            generation: generation.to_string(),
            pre_closure_reasons,
            observations,
            established_states,
        }
    }
}

// ── Conflict and lineage queries (TUP-NE-005) ───────────────────────────────

/// One closure competing for a consumed output under a different successor.
///
/// An observation, never a verdict. Two sources reporting different successors
/// for one output is the equivocation this plane records; which of them is
/// closure-valid is Parwana's verifier's answer and appears nowhere here.
#[derive(SimpleObject, Clone)]
#[graphql(name = "CompetingClosureGql")]
pub struct CompetingClosureGql {
    pub chain_id: String,
    pub network_id: String,
    pub closure_kind: String,
    pub closure_identity_hex: String,
    /// The successor this competitor favours, which is not the one searched for.
    pub successor_commitment_hex: String,
    /// The state type this competitor's source reported for the consumed output.
    ///
    /// Competitors are matched on the output's identity — the transition that
    /// created it and its index — and not on this field, which no observation
    /// plane can check against a schema. A value differing from the searched
    /// one is two sources disagreeing about what they consumed.
    pub consumed_state_type: i32,
}

/// Freshness of one chain-and-network index, as one term of a search bound.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureIndexDomainFreshnessGql")]
pub struct ClosureIndexDomainFreshnessGql {
    pub chain_id: String,
    pub network_id: String,
    pub index_freshness: IndexFreshnessGql,
}

/// One page of the closures competing for a consumed output.
///
/// Read `coverage` before reading an empty `competitors`. `more_pages_remain`
/// means closures were left unread and their absence says nothing;
/// `recorded_set_exhausted` means the page reached the end of what is recorded,
/// bounded by `searched_domains`. Neither is a uniqueness claim: an index covers
/// its chain only to the tip it has reached.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureConflictPageGql")]
pub struct ClosureConflictPageGql {
    pub schema_version: i32,
    pub consumed_transition_id_hex: String,
    pub consumed_output_index: i64,
    /// The successor whose competitors were sought.
    pub successor_commitment_hex: String,
    pub competitors: Vec<CompetingClosureGql>,
    /// `recorded_set_exhausted` or `more_pages_remain`.
    pub coverage: String,
    /// Freshness of every index the page relied on. Empty unless the coverage is
    /// `recorded_set_exhausted`.
    pub searched_domains: Vec<ClosureIndexDomainFreshnessGql>,
    /// Cursor to resume after. Present only for `more_pages_remain`.
    pub resume_after_observation_id: Option<String>,
}

impl From<tuppira_shared::ClosureConflictPageV1> for ClosureConflictPageGql {
    fn from(value: tuppira_shared::ClosureConflictPageV1) -> Self {
        let (coverage, searched_domains, resume_after_observation_id) = match value.coverage {
            tuppira_shared::ClosureConflictPageCoverage::RecordedSetExhausted {
                searched_domains,
            } => (
                "recorded_set_exhausted",
                searched_domains
                    .into_iter()
                    .map(|domain| ClosureIndexDomainFreshnessGql {
                        chain_id: domain.chain_id,
                        network_id: domain.network_id,
                        index_freshness: domain.index_freshness.into(),
                    })
                    .collect(),
                None,
            ),
            tuppira_shared::ClosureConflictPageCoverage::MorePagesRemain {
                resume_after_observation_id,
            } => (
                "more_pages_remain",
                Vec::new(),
                Some(resume_after_observation_id),
            ),
        };
        Self {
            schema_version: i32::from(value.schema_version),
            consumed_transition_id_hex: value.consumed_state.transition_id_hex,
            consumed_output_index: i64::from(value.consumed_state.output_index),
            successor_commitment_hex: value.successor_commitment_hex,
            competitors: value
                .competitors
                .into_iter()
                .map(|competitor| CompetingClosureGql {
                    chain_id: competitor.chain_id,
                    network_id: competitor.network_id,
                    closure_kind: competitor.closure_kind,
                    closure_identity_hex: competitor.closure_identity_hex,
                    successor_commitment_hex: competitor.successor_commitment_hex,
                    consumed_state_type: i32::from(competitor.consumed_state_type),
                })
                .collect(),
            coverage: coverage.to_string(),
            searched_domains,
            resume_after_observation_id,
        }
    }
}

/// One consumed output on a lineage walk.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureConsumedOutputGql")]
pub struct ClosureConsumedOutputGql {
    pub transition_id_hex: String,
    pub output_index: i64,
    /// The state type the reporting source declared for the output.
    pub state_type: i32,
}

/// One observed closure on the walk from a source state toward its successors.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureLineageStepGql")]
pub struct ClosureLineageStepGql {
    /// Distance from the queried root state; the nearest step is `1`.
    pub depth: i64,
    pub observation_id: String,
    pub chain_id: String,
    pub network_id: String,
    pub closure_kind: String,
    pub closure_identity_hex: String,
    /// The output this step's closure was reported to consume.
    pub consumed_state: ClosureConsumedOutputGql,
    /// The successor this closure favours.
    pub successor_commitment_hex: String,
    pub reorg_standing: ClosureReorgStandingGql,
    /// What the observation still establishes, its standing applied. Never the
    /// stored set: a step on a replaced history must not report settlement here
    /// when a direct read of it would not.
    pub established_states: Vec<String>,
    /// Outputs of the successor a further closure was observed to consume.
    ///
    /// Observed consumption and nothing else. An output missing here is one no
    /// source reported a closure for, which is not evidence it is unspent — only
    /// that this index has not seen it spent.
    pub observed_consumed_outputs: Vec<ClosureConsumedOutputGql>,
}

/// The closures observed downstream of one source state.
///
/// An investigator's trail, not an assurance result. Nothing in it concludes
/// that a successor is valid, that a conflict has a winner, or that a state is
/// unspent. An empty `steps` list means no closure was observed on the root, not
/// that none exists.
#[derive(SimpleObject, Clone)]
#[graphql(name = "ClosureLineageGql")]
pub struct ClosureLineageGql {
    pub schema_version: i32,
    pub root_state: ClosureConsumedOutputGql,
    /// Steps in walk order: nearest first, then by observation identifier.
    pub steps: Vec<ClosureLineageStepGql>,
    /// `complete`, `truncated_at_depth`, or `truncated_at_step_limit`. Read it
    /// before taking the end of `steps` for the end of the lineage.
    pub coverage: String,
    /// The depth the walk stopped at. Present only for `truncated_at_depth`.
    pub coverage_depth: Option<i64>,
}

fn consumed_output(value: tuppira_shared::ConsumedStateReading) -> ClosureConsumedOutputGql {
    ClosureConsumedOutputGql {
        transition_id_hex: value.transition_id_hex,
        output_index: i64::from(value.output_index),
        state_type: i32::from(value.state_type),
    }
}

impl From<tuppira_shared::ClosureLineageV1> for ClosureLineageGql {
    fn from(value: tuppira_shared::ClosureLineageV1) -> Self {
        let (coverage, coverage_depth) = match value.coverage {
            tuppira_shared::ClosureLineageCoverage::Complete => ("complete", None),
            tuppira_shared::ClosureLineageCoverage::TruncatedAtDepth { depth } => {
                ("truncated_at_depth", Some(i64::from(depth)))
            }
            tuppira_shared::ClosureLineageCoverage::TruncatedAtStepLimit { .. } => {
                ("truncated_at_step_limit", None)
            }
        };
        Self {
            schema_version: i32::from(value.schema_version),
            root_state: consumed_output(value.root_state),
            steps: value
                .steps
                .into_iter()
                .map(|step| ClosureLineageStepGql {
                    depth: i64::from(step.depth),
                    observation_id: step.observation_id,
                    chain_id: step.chain_id,
                    network_id: step.network_id,
                    closure_kind: step.closure_kind,
                    closure_identity_hex: step.closure_identity_hex,
                    consumed_state: consumed_output(step.consumed_state),
                    successor_commitment_hex: step.successor_commitment_hex,
                    reorg_standing: step.reorg_standing.into(),
                    established_states: step.established_states.iter().map(state_name).collect(),
                    observed_consumed_outputs: step
                        .observed_consumed_outputs
                        .into_iter()
                        .map(consumed_output)
                        .collect(),
                })
                .collect(),
            coverage: coverage.to_string(),
            coverage_depth,
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
