//! Typed persistence for source-neutral observations and their lineage.

use std::collections::BTreeSet;

use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tuppira_shared::{
    CLOSURE_OBSERVATION_PROFILE_VERSION, ChainClosureEvidenceRecord, ClosureAncestryCoverage,
    ClosureConflictPageCoverage, ClosureConflictPageV1, ClosureConflictSearchOutcome,
    ClosureIndexDomainFreshness, ClosureIndexTipRecord, ClosureLineageCoverage, ClosureLineageStepV1,
    ClosureLineageV1, ClosureObservationOrphaningRecord, ClosureObservationViewV1,
    ClosureOrphaningReading, ClosureProfileGeneration, ClosureReorgStanding, ClosureViewGeneration,
    CollectionRunRecord, CompetingClosureReading, ConsumedStateReading, ContradictionHintRecord,
    MAX_CLOSURE_ANCESTRY_DEPTH, MAX_CLOSURE_CONFLICT_PAGE, MAX_CLOSURE_LINEAGE_DEPTH,
    MAX_CLOSURE_LINEAGE_STEPS, ObservationRecord, ObservedOrMissing, OrphanedAncestorReading,
    OrphanedClosureDisposition, RawPayloadDescriptor, RecordedSourceClosureObservationV1,
    ReorgRecord, Result, RetentionClassRecord, RetractionStatus,
    SOURCE_CLOSURE_OBSERVATION_PROFILE_ID, SUBJECT_CLOSURE_PROFILE_VERSION,
    SourceClosureObservationProjectionV1, SourceRecord, SubjectClosureProjectionV1,
    SubjectClosureViewV1, SyncCursorRecord, TenantVisibility, TuppiraError,
    assess_closure_conflicts, established_states_under_reorg,
};

/// Observation-plane repository. Inserts validate at the typed boundary and
/// SQLite constraints enforce the same invariants for every caller.
#[derive(Clone)]
pub struct ObservationRepository {
    pool: SqlitePool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHealthProjection {
    pub source_id: String,
    pub connector_kind: String,
    pub display_name: String,
    pub last_run_started_at: Option<u64>,
    pub last_run_completed_at: Option<u64>,
    pub cursor_observed_at: Option<u64>,
}

impl ObservationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert_retention_class(&self, record: &RetentionClassRecord) -> Result<()> {
        ensure_version(record.schema_version)?;
        ensure_text(&record.retention_class_id, "retention_class_id")?;
        ensure_text(&record.purpose, "purpose")?;
        let retain_for_seconds = optional_i64(record.retain_for_seconds, "retain_for_seconds")?;
        sqlx::query("INSERT INTO retention_classes (retention_class_id, schema_version, purpose, retain_for_seconds, raw_payload_permitted) VALUES (?, ?, ?, ?, ?)")
            .bind(&record.retention_class_id).bind(i64::from(record.schema_version)).bind(&record.purpose)
            .bind(retain_for_seconds).bind(record.raw_payload_permitted).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn insert_source(&self, record: &SourceRecord) -> Result<()> {
        ensure_version(record.schema_version)?;
        ensure_text(&record.source_id, "source_id")?;
        ensure_text(&record.connector_kind, "connector_kind")?;
        ensure_text(&record.display_name, "display_name")?;
        sqlx::query("INSERT INTO observation_sources (source_id, schema_version, connector_kind, display_name, retention_class_id) VALUES (?, ?, ?, ?, ?)")
            .bind(&record.source_id).bind(i64::from(record.schema_version)).bind(&record.connector_kind)
            .bind(&record.display_name).bind(&record.retention_class_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn insert_collection_run(&self, record: &CollectionRunRecord) -> Result<()> {
        ensure_version(record.schema_version)?;
        ensure_text(&record.collection_run_id, "collection_run_id")?;
        ensure_text(&record.source_id, "source_id")?;
        let started_at = required_i64(record.started_at, "started_at")?;
        let completed_at = optional_i64(record.completed_at, "completed_at")?;
        if completed_at.is_some_and(|completed| completed < started_at) {
            return Err(invalid("completed_at precedes started_at"));
        }
        sqlx::query("INSERT INTO collection_runs (collection_run_id, schema_version, source_id, started_at, completed_at) VALUES (?, ?, ?, ?, ?)")
            .bind(&record.collection_run_id).bind(i64::from(record.schema_version)).bind(&record.source_id)
            .bind(started_at).bind(completed_at).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn insert_raw_payload_descriptor(&self, record: &RawPayloadDescriptor) -> Result<()> {
        ensure_version(record.schema_version)?;
        ensure_text(&record.payload_id, "payload_id")?;
        ensure_text(&record.digest_algorithm, "digest_algorithm")?;
        ensure_text(&record.media_type, "media_type")?;
        ensure_digest(&record.payload_digest, "payload_digest")?;
        let byte_length = required_i64(record.byte_length, "byte_length")?;
        sqlx::query("INSERT INTO raw_payload_descriptors (payload_id, schema_version, digest_algorithm, payload_digest, media_type, byte_length, custody_locator, retention_class_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&record.payload_id).bind(i64::from(record.schema_version)).bind(&record.digest_algorithm)
            .bind(record.payload_digest.as_slice()).bind(&record.media_type).bind(byte_length)
            .bind(&record.custody_locator).bind(&record.retention_class_id).execute(&self.pool).await?;
        Ok(())
    }

    /// Atomically append an observation, its normalized references and optional
    /// supersession, then advance the source cursor. A failed cursor write rolls
    /// the entire append back.
    pub async fn append_observation(
        &self,
        observation: &ObservationRecord,
        cursor: &SyncCursorRecord,
    ) -> Result<()> {
        observation
            .validate()
            .map_err(|error| invalid(&format!("invalid observation: {error:?}")))?;
        validate_cursor(cursor, &observation.source_id, observation.observed_at)?;
        let mut transaction = self.pool.begin().await?;
        insert_observation(&mut transaction, observation).await?;
        upsert_cursor(&mut transaction, cursor).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn get_observation(&self, observation_id: &str) -> Result<ObservationRecord> {
        let row = sqlx::query("SELECT schema_version, observation_id, source_id, source_event_id, source_event_type, asserted_event_time, observed_at, normalized_profile_id, normalized_profile_version, normalized_payload_digest, raw_payload_digest, collection_run_id, supersedes_observation_id, retraction_status, visibility_scope, tenant_id FROM observations WHERE observation_id = ?")
            .bind(observation_id).fetch_optional(&self.pool).await?
            .ok_or_else(|| TuppiraError::NotFound { entity_type: "observation".into(), id: observation_id.into() })?;
        let subjects = sqlx::query_scalar::<_, String>("SELECT subject_ref FROM observation_subjects WHERE observation_id = ? ORDER BY subject_ref")
            .bind(observation_id).fetch_all(&self.pool).await?;
        let authenticity_refs = sqlx::query_scalar::<_, String>("SELECT authenticity_material_ref FROM observation_authenticity_refs WHERE observation_id = ? ORDER BY authenticity_material_ref")
            .bind(observation_id).fetch_all(&self.pool).await?;
        let supersedes = row.try_get("supersedes_observation_id")?;
        decode_observation(row, subjects, authenticity_refs, supersedes)
    }

    pub async fn lineage(&self, observation_id: &str) -> Result<Vec<String>> {
        ensure_text(observation_id, "observation_id")?;
        let rows = sqlx::query_scalar::<_, String>(
            "WITH RECURSIVE lineage(id) AS (SELECT ? UNION ALL SELECT s.superseded_observation_id FROM supersessions s JOIN lineage l ON s.superseding_observation_id = l.id) SELECT id FROM lineage",
        ).bind(observation_id).fetch_all(&self.pool).await?;
        Ok(rows)
    }

    /// Return an observation only when it is public or belongs to the caller's tenant.
    pub async fn get_visible_observation(
        &self,
        observation_id: &str,
        tenant_id: &str,
    ) -> Result<ObservationRecord> {
        ensure_text(tenant_id, "tenant_id")?;
        let observation = self.get_observation(observation_id).await?;
        if matches!(&observation.tenant_visibility, TenantVisibility::Tenant { tenant_id: owner } if owner != tenant_id)
        {
            return Err(TuppiraError::NotFound {
                entity_type: "observation".into(),
                id: observation_id.into(),
            });
        }
        Ok(observation)
    }

    /// Ordered correction ancestry, filtered at every recursive step so a
    /// malformed cross-tenant lineage can never disclose an identifier.
    pub async fn visible_lineage(
        &self,
        observation_id: &str,
        tenant_id: &str,
    ) -> Result<Vec<ObservationRecord>> {
        ensure_text(observation_id, "observation_id")?;
        ensure_text(tenant_id, "tenant_id")?;
        let ids = sqlx::query_scalar::<_, String>(
            "WITH RECURSIVE lineage(id) AS (\
             SELECT observation_id FROM observations WHERE observation_id = ? AND (visibility_scope = 'public' OR tenant_id = ?) \
             UNION ALL \
             SELECT prior.observation_id FROM supersessions s JOIN lineage l ON s.superseding_observation_id = l.id \
             JOIN observations prior ON prior.observation_id = s.superseded_observation_id \
             WHERE prior.visibility_scope = 'public' OR prior.tenant_id = ?) SELECT id FROM lineage",
        )
        .bind(observation_id)
        .bind(tenant_id)
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        if ids.is_empty() {
            return Err(TuppiraError::NotFound {
                entity_type: "observation".into(),
                id: observation_id.into(),
            });
        }
        let mut records = Vec::with_capacity(ids.len());
        for id in ids {
            records.push(self.get_visible_observation(&id, tenant_id).await?);
        }
        Ok(records)
    }

    /// The most recent tenant-visible observations, newest first.
    ///
    /// This is the live discovery feed the Hemion explorer polls. Only public
    /// observations and those owned by the caller's tenant are returned; the
    /// full record (subjects, digests) is assembled per row via the same
    /// visibility-checked path as single-observation reads.
    pub async fn list_visible_observations(
        &self,
        tenant_id: &str,
        limit: i64,
    ) -> Result<Vec<ObservationRecord>> {
        ensure_text(tenant_id, "tenant_id")?;
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT observation_id FROM observations \
             WHERE visibility_scope = 'public' OR tenant_id = ? \
             ORDER BY observed_at DESC, observation_id DESC LIMIT ?",
        )
        .bind(tenant_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut records = Vec::with_capacity(ids.len());
        for id in ids {
            records.push(self.get_visible_observation(&id, tenant_id).await?);
        }
        Ok(records)
    }

    // ── V2 source-closure observations (TUP-NE-002) ──────────────────────────

    /// Atomically append a source-closure observation, its projection payload
    /// and the advanced cursor.
    ///
    /// The payload is bound to the observation by digest: an observation commits
    /// to exactly one normalized payload, and storing bytes that hash to
    /// anything else would leave the commitment pointing at a payload nobody
    /// holds. Both are written in one transaction with the cursor, so a failed
    /// cursor advance rolls the closure evidence back with it.
    pub async fn append_closure_observation(
        &self,
        observation: &ObservationRecord,
        projection: &SourceClosureObservationProjectionV1,
        evidence: Option<&ChainClosureEvidenceRecord>,
        cursor: &SyncCursorRecord,
    ) -> Result<()> {
        observation
            .validate()
            .map_err(|error| invalid(&format!("invalid observation: {error:?}")))?;
        projection
            .validate()
            .map_err(|error| invalid(&format!("invalid closure projection: {error:?}")))?;
        if observation.normalized_profile_id != SOURCE_CLOSURE_OBSERVATION_PROFILE_ID {
            return Err(invalid(
                "closure payload attached to a non-closure normalization profile",
            ));
        }
        if observation.normalized_profile_version != CLOSURE_OBSERVATION_PROFILE_VERSION
            || projection.schema_version != CLOSURE_OBSERVATION_PROFILE_VERSION
        {
            return Err(invalid("unsupported closure projection version"));
        }
        let digest = projection
            .normalized_payload_digest()
            .map_err(|error| invalid(&format!("closure projection digest: {error:?}")))?;
        if digest != observation.normalized_payload_digest {
            return Err(invalid(
                "closure payload does not match the observation's normalized payload digest",
            ));
        }
        let payload = serde_json::to_string(projection)
            .map_err(|error| invalid(&format!("closure projection encoding: {error}")))?;
        if let Some(evidence) = evidence {
            evidence
                .validate()
                .map_err(|error| invalid(&format!("invalid chain closure evidence: {error:?}")))?;
            // Evidence must address this observation and this closure family,
            // or the way back from the normalized closure leads somewhere else.
            if evidence.observation_id != observation.observation_id
                || evidence.native_event_kind != projection.closure_identity.closure_kind
            {
                return Err(invalid(
                    "chain closure evidence does not address this normalized closure",
                ));
            }
        }
        validate_cursor(cursor, &observation.source_id, observation.observed_at)?;

        let mut transaction = self.pool.begin().await?;
        insert_observation(&mut transaction, observation).await?;
        insert_closure_projection(&mut transaction, &observation.observation_id, projection, &payload)
            .await?;
        if let Some(evidence) = evidence {
            insert_closure_evidence(&mut transaction, evidence).await?;
        }
        // The collector read a tip to produce this projection, so record it as
        // index state in the same transaction. Without this a chain whose first
        // closure arrives after migration 0008 would have no tip at all, and a
        // read of that closure could not say how far behind the index is. It
        // only ever advances the tip (TUP-NE-004).
        upsert_closure_index_tip(
            &mut transaction,
            &ClosureIndexTipRecord {
                schema_version: tuppira_shared::CLOSURE_INDEX_TIP_RECORD_VERSION,
                chain_id: projection.chain_id.clone(),
                network_id: projection.network_id.clone(),
                indexed_tip_height: projection.index_freshness.indexed_tip_height,
                indexed_tip_block_id_hex: projection
                    .index_freshness
                    .indexed_tip_block_id_hex
                    .clone(),
                indexed_tip_observed_at: projection.index_freshness.indexed_tip_observed_at,
            },
        )
        .await?;
        upsert_cursor(&mut transaction, cursor).await?;
        transaction.commit().await?;
        Ok(())
    }

    // ── Reorganization standing and reorg-aware views (TUP-NE-004) ───────────

    /// Advance the recorded index tip for one chain and network.
    ///
    /// Returns whether the tip moved. A reading at or below the recorded tip is
    /// not an error and is discarded: connectors read out of order, and the tip
    /// is the high-water mark of what the index has reached, not the last thing
    /// it happened to look at. Letting a lower reading win would shrink every
    /// lag computed against it without the index having fallen behind.
    ///
    /// # Errors
    ///
    /// Fails closed on an invalid record or a height outside SQLite's range.
    pub async fn advance_closure_index_tip(&self, record: &ClosureIndexTipRecord) -> Result<bool> {
        record
            .validate()
            .map_err(|error| invalid(&format!("invalid closure index tip: {error:?}")))?;
        let mut transaction = self.pool.begin().await?;
        let advanced = upsert_closure_index_tip(&mut transaction, record).await?;
        transaction.commit().await?;
        Ok(advanced)
    }

    /// Record that a reorganization orphaned one closure observation.
    ///
    /// Nothing is deleted or edited: `closure_observations` is append-only by
    /// trigger, so the orphaned statement stays readable exactly as the source
    /// first reported it and this is appended beside it. Removing it would
    /// destroy the only record that the source once said it.
    ///
    /// # Errors
    ///
    /// Fails closed when the record is invalid, when the reorganization belongs
    /// to a different source than the observation, when the orphaning precedes
    /// the observation it names, when a replacement closes a different consumed
    /// state, or when the observation carries no closure statement at all.
    pub async fn record_closure_orphaning(
        &self,
        record: &ClosureObservationOrphaningRecord,
    ) -> Result<()> {
        record
            .validate()
            .map_err(|error| invalid(&format!("invalid closure orphaning: {error:?}")))?;
        // An observation with no closure payload has no closure to orphan. The
        // foreign key would catch this, but the error it raises names a
        // constraint rather than the thing that is wrong.
        let is_closure = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM closure_observations WHERE observation_id = ?",
        )
        .bind(&record.observation_id)
        .fetch_one(&self.pool)
        .await?;
        if is_closure == 0 {
            return Err(invalid(
                "a reorganization cannot orphan an observation that carries no closure statement",
            ));
        }

        let (disposition, superseding) = match &record.disposition {
            OrphanedClosureDisposition::Superseded {
                superseding_observation_id,
            } => ("superseded", Some(superseding_observation_id.as_str())),
            OrphanedClosureDisposition::Retracted { .. } => ("retracted", None),
        };
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO closure_observation_orphanings (reorg_id, observation_id, schema_version, orphaned_at, disposition, superseding_observation_id) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&record.reorg_id)
        .bind(&record.observation_id)
        .bind(i64::from(record.schema_version))
        .bind(required_i64(record.orphaned_at, "orphaned_at")?)
        .bind(disposition)
        .bind(superseding)
        .execute(&mut *transaction)
        .await?;
        if let OrphanedClosureDisposition::Retracted { reasons } = &record.disposition {
            for (ordinal, reason) in reasons.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO closure_observation_orphaning_reasons (reorg_id, observation_id, ordinal, reason) VALUES (?, ?, ?, ?)",
                )
                .bind(&record.reorg_id)
                .bind(&record.observation_id)
                .bind(
                    i64::try_from(ordinal)
                        .map_err(|_| invalid("orphaning reason ordinal exceeds SQLite range"))?,
                )
                .bind(reason)
                .execute(&mut *transaction)
                .await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    /// One closure observation as it reads now, tenant-filtered.
    ///
    /// This is [`Self::closure_observation`] plus the two things that make a
    /// stored projection safe to read later: the reorganizations recorded
    /// against it and against the closures it descends from, and how far behind
    /// the chain the index is *at this moment* rather than when the collector
    /// produced the projection.
    ///
    /// # Errors
    ///
    /// Fails closed when the observation carries no closure, when its stored
    /// payload no longer matches its digest, or when no index tip is recorded
    /// for its chain and network.
    pub async fn closure_observation_view(
        &self,
        observation_id: &str,
        tenant_id: &str,
    ) -> Result<ClosureObservationViewV1> {
        let observation = self.get_visible_observation(observation_id, tenant_id).await?;
        let projection = self.closure_observation(observation_id, tenant_id).await?;
        let recorded = RecordedSourceClosureObservationV1 {
            observation_id: observation.observation_id.clone(),
            observed_at: observation.observed_at,
            record_retraction_status: observation.retraction_status,
            projection,
        };
        let view = self.build_closure_view(recorded, tenant_id).await?;
        view.validate()
            .map_err(|error| invalid(&format!("invalid closure observation view: {error:?}")))?;
        Ok(view)
    }

    /// The closure account for one subject as it reads now, tenant-filtered.
    ///
    /// Each recorded observation is given its own standing and read-time
    /// freshness before the account is assembled, so a settlement withdrawn
    /// from every observation individually cannot reappear in the subject's
    /// account. A subject with no recorded closure observation yields
    /// [`ClosureViewGeneration::PreClosure`], never an empty observation list.
    ///
    /// # Errors
    ///
    /// Fails closed for the same reasons as [`Self::subject_closure`], and
    /// additionally when any observation's chain has no recorded index tip.
    pub async fn subject_closure_view(
        &self,
        subject_ref: &str,
        tenant_id: &str,
    ) -> Result<SubjectClosureViewV1> {
        let account = self.subject_closure(subject_ref, tenant_id).await?;
        let observations = match account.closure_generation {
            ClosureProfileGeneration::PreClosure { .. } => {
                return Ok(SubjectClosureViewV1::pre_closure(account.subject_ref));
            }
            ClosureProfileGeneration::SourceClosureV2 { observations } => observations,
        };
        let mut views = Vec::with_capacity(observations.len());
        for recorded in observations {
            views.push(self.build_closure_view(recorded, tenant_id).await?);
        }
        let view = SubjectClosureViewV1 {
            schema_version: tuppira_shared::SUBJECT_CLOSURE_VIEW_VERSION,
            subject_ref: account.subject_ref,
            closure_generation: ClosureViewGeneration::SourceClosureV2 {
                observations: views,
            },
        };
        view.validate()
            .map_err(|error| invalid(&format!("invalid subject closure view: {error:?}")))?;
        Ok(view)
    }

    /// The recorded index tip for one chain and network.
    ///
    /// # Errors
    ///
    /// [`TuppiraError::NotFound`] when no tip is recorded. This is deliberately
    /// not a fallback to the tip inside the stored projection: that reading is
    /// the collector's, and returning it as the read-time tip would present a
    /// year-old closure as a current one — the exact substitution the view's
    /// two freshness fields exist to prevent.
    pub async fn closure_index_tip(
        &self,
        chain_id: &str,
        network_id: &str,
    ) -> Result<ClosureIndexTipRecord> {
        ensure_text(chain_id, "chain_id")?;
        ensure_text(network_id, "network_id")?;
        let row = sqlx::query(
            "SELECT indexed_tip_height, indexed_tip_block_id_hex, indexed_tip_observed_at \
             FROM closure_index_tips WHERE chain_id = ? AND network_id = ?",
        )
        .bind(chain_id)
        .bind(network_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| TuppiraError::NotFound {
            entity_type: "closure_index_tip".into(),
            id: format!("{chain_id}/{network_id}"),
        })?;
        let record = ClosureIndexTipRecord {
            schema_version: tuppira_shared::CLOSURE_INDEX_TIP_RECORD_VERSION,
            chain_id: chain_id.to_string(),
            network_id: network_id.to_string(),
            indexed_tip_height: to_u64(row.try_get("indexed_tip_height")?, "indexed_tip_height")?,
            indexed_tip_block_id_hex: row.try_get("indexed_tip_block_id_hex")?,
            indexed_tip_observed_at: to_u64(
                row.try_get("indexed_tip_observed_at")?,
                "indexed_tip_observed_at",
            )?,
        };
        record
            .validate()
            .map_err(|error| invalid(&format!("invalid stored closure index tip: {error:?}")))?;
        Ok(record)
    }

    /// Where one recorded observation stands after the reorganizations recorded
    /// against it and against the closures it descends from.
    ///
    /// The two lists answer different questions and are kept apart: an
    /// observation's own orphaning is about the block that carried it, an
    /// orphaned ancestor is about the ground beneath it, and an observation can
    /// carry both.
    ///
    /// # Errors
    ///
    /// Fails closed when more orphanings or orphaned ancestors exist than one
    /// standing can report. Truncating either would understate how much of the
    /// history was replaced, and this type has no way to say "partial".
    pub async fn closure_reorg_standing(
        &self,
        observation_id: &str,
        tenant_id: &str,
    ) -> Result<ClosureReorgStanding> {
        let projection = self.closure_observation(observation_id, tenant_id).await?;
        self.reorg_standing_for(
            observation_id,
            &projection.consumed_state.transition_id_hex,
            tenant_id,
        )
        .await
    }

    async fn build_closure_view(
        &self,
        recorded: RecordedSourceClosureObservationV1,
        tenant_id: &str,
    ) -> Result<ClosureObservationViewV1> {
        let standing = self
            .reorg_standing_for(
                &recorded.observation_id,
                &recorded.projection.consumed_state.transition_id_hex,
                tenant_id,
            )
            .await?;
        let tip = self
            .closure_index_tip(
                &recorded.projection.chain_id,
                &recorded.projection.network_id,
            )
            .await?;
        let read_index_freshness = tip.freshness_for(&recorded.projection.observed_checkpoint);
        Ok(ClosureObservationViewV1::new(
            recorded,
            standing,
            read_index_freshness,
        ))
    }

    async fn reorg_standing_for(
        &self,
        observation_id: &str,
        consumed_transition_id_hex: &str,
        tenant_id: &str,
    ) -> Result<ClosureReorgStanding> {
        let own_orphanings = self.own_orphanings(observation_id).await?;
        let (orphaned_ancestors, ancestry_coverage) = self
            .orphaned_ancestors(observation_id, consumed_transition_id_hex, tenant_id)
            .await?;
        let standing = ClosureReorgStanding {
            own_orphanings,
            orphaned_ancestors,
            ancestry_coverage,
        };
        standing
            .validate()
            .map_err(|error| invalid(&format!("invalid closure reorg standing: {error:?}")))?;
        Ok(standing)
    }

    async fn own_orphanings(&self, observation_id: &str) -> Result<Vec<ClosureOrphaningReading>> {
        let rows = sqlx::query(
            "SELECT reorg_id, orphaned_at, disposition, superseding_observation_id \
             FROM closure_observation_orphanings WHERE observation_id = ? \
             ORDER BY orphaned_at, reorg_id",
        )
        .bind(observation_id)
        .fetch_all(&self.pool)
        .await?;
        let mut orphanings = Vec::with_capacity(rows.len());
        for row in rows {
            let reorg_id: String = row.try_get("reorg_id")?;
            let disposition = match row.try_get::<String, _>("disposition")?.as_str() {
                "superseded" => {
                    let superseding_observation_id: Option<String> =
                        row.try_get("superseding_observation_id")?;
                    OrphanedClosureDisposition::Superseded {
                        superseding_observation_id: superseding_observation_id.ok_or_else(|| {
                            invalid("a superseded orphaning without its replacement")
                        })?,
                    }
                }
                "retracted" => OrphanedClosureDisposition::Retracted {
                    reasons: sqlx::query_scalar::<_, String>(
                        "SELECT reason FROM closure_observation_orphaning_reasons \
                         WHERE reorg_id = ? AND observation_id = ? ORDER BY ordinal",
                    )
                    .bind(&reorg_id)
                    .bind(observation_id)
                    .fetch_all(&self.pool)
                    .await?,
                },
                _ => return Err(invalid("unsupported closure orphaning disposition")),
            };
            orphanings.push(ClosureOrphaningReading {
                reorg_id,
                orphaned_at: to_u64(row.try_get("orphaned_at")?, "orphaned_at")?,
                disposition,
            });
        }
        Ok(orphanings)
    }

    /// Walk the reported closure linkage upward, collecting orphaned ancestors.
    ///
    /// The linkage is the one the observations themselves express: this
    /// observation's consumed state names the transition an earlier observation
    /// reported as its successor commitment. Tuppira does not decide whether
    /// that linkage is protocol-valid — only Parwana's verifier can — so the
    /// walk is used in one direction only, to carry doubt downward.
    ///
    /// The walk is bounded by [`MAX_CLOSURE_ANCESTRY_DEPTH`] and by a visited
    /// set. Both are needed: the visited set stops a reported cycle from
    /// looping, and the depth bound stops a long reported chain from turning
    /// one read into an unbounded number of queries. Stopping with linkage left
    /// to follow is reported as [`ClosureAncestryCoverage::TruncatedAtDepth`],
    /// never as a completed walk.
    async fn orphaned_ancestors(
        &self,
        observation_id: &str,
        consumed_transition_id_hex: &str,
        tenant_id: &str,
    ) -> Result<(Vec<OrphanedAncestorReading>, ClosureAncestryCoverage)> {
        let mut visited: BTreeSet<String> = BTreeSet::from([observation_id.to_string()]);
        let mut frontier = vec![consumed_transition_id_hex.to_string()];
        let mut ancestors: Vec<OrphanedAncestorReading> = Vec::new();
        let mut depth = 0_u32;

        while !frontier.is_empty() {
            if depth >= MAX_CLOSURE_ANCESTRY_DEPTH {
                return Ok((ancestors, ClosureAncestryCoverage::TruncatedAtDepth { depth }));
            }
            depth += 1;
            let mut next: Vec<String> = Vec::new();
            for commitment in &frontier {
                let rows = sqlx::query(
                    "SELECT c.observation_id, c.consumed_transition_id_hex, \
                     EXISTS(SELECT 1 FROM closure_observation_orphanings p \
                            WHERE p.observation_id = c.observation_id) AS is_orphaned \
                     FROM closure_observations c \
                     JOIN observations o ON o.observation_id = c.observation_id \
                     WHERE c.successor_commitment_hex = ? \
                       AND (o.visibility_scope = 'public' OR o.tenant_id = ?) \
                     ORDER BY c.observation_id",
                )
                .bind(commitment)
                .bind(tenant_id)
                .fetch_all(&self.pool)
                .await?;
                for row in rows {
                    let ancestor_id: String = row.try_get("observation_id")?;
                    if !visited.insert(ancestor_id.clone()) {
                        continue;
                    }
                    if row.try_get::<i64, _>("is_orphaned")? != 0 {
                        // A subject whose ancestry carries more orphaned
                        // closures than a standing can report is not a subject
                        // to answer partially: the omitted ones are exactly the
                        // doubt the caller asked about.
                        if ancestors.len() >= tuppira_shared::MAX_OBSERVATION_REFS {
                            return Err(invalid(
                                "closure ancestry carries more orphaned ancestors than one standing can report",
                            ));
                        }
                        ancestors.push(OrphanedAncestorReading {
                            observation_id: ancestor_id,
                            successor_commitment_hex: commitment.clone(),
                            depth,
                        });
                    }
                    let parent: String = row.try_get("consumed_transition_id_hex")?;
                    if !next.contains(&parent) {
                        next.push(parent);
                    }
                }
            }
            frontier = next;
        }
        Ok((ancestors, ClosureAncestryCoverage::Complete))
    }

    // ── Conflict and lineage queries (TUP-NE-005) ────────────────────────────

    /// One page of the closures competing for a consumed output, tenant-filtered.
    ///
    /// This is the surface [`Self::subject_closure`] directs a heavily contested
    /// subject to. That projection fails closed rather than truncate; this one
    /// pages, because enumerating a conflict is exactly what an investigator
    /// needs when a subject carries more closures than one account can report.
    ///
    /// Competitors are collected across every chain the index holds. A positive
    /// finding is never withheld for being on another chain, and TUP-NE-003
    /// normalized the closure identity precisely so cross-chain competitors
    /// compare on identical terms. Detection itself is
    /// [`assess_closure_conflicts`], called per chain and network with that
    /// index's own freshness — this method is the query surface over it and not
    /// a second implementation of the comparison.
    ///
    /// The negative is bounded rather than asserted:
    /// [`ClosureConflictPageCoverage::RecordedSetExhausted`] carries the
    /// freshness of every index domain the page relied on, because a search is
    /// only as current as the laggiest index behind it.
    ///
    /// # Errors
    ///
    /// Fails closed on a malformed selector, on a stored payload that no longer
    /// matches its digest, and when a chain carrying a closure has no recorded
    /// index tip — a competitor set with no measurable bound would read as a
    /// stronger result than it is.
    pub async fn closure_conflicts(
        &self,
        consumed_state: &ConsumedStateReading,
        successor_commitment_hex: &str,
        resume_after_observation_id: Option<&str>,
        tenant_id: &str,
    ) -> Result<ClosureConflictPageV1> {
        ensure_text(&consumed_state.transition_id_hex, "transition_id_hex")?;
        ensure_text(successor_commitment_hex, "successor_commitment_hex")?;
        ensure_text(tenant_id, "tenant_id")?;
        if let Some(cursor) = resume_after_observation_id {
            ensure_text(cursor, "resume_after_observation_id")?;
        }

        // One row over the page bound is fetched so "the page is full" and "more
        // exists" stay distinguishable. Reporting a full page as exhausted would
        // turn an unread remainder into an absence of competitors.
        let rows = sqlx::query(
            "SELECT c.observation_id, c.chain_id, c.network_id, c.projection_json, \
             o.normalized_payload_digest \
             FROM closure_observations c \
             JOIN observations o ON o.observation_id = c.observation_id \
             WHERE c.consumed_transition_id_hex = ? AND c.consumed_output_index = ? \
               AND (o.visibility_scope = 'public' OR o.tenant_id = ?) \
               AND (? IS NULL OR c.observation_id > ?) \
             ORDER BY c.observation_id LIMIT ?",
        )
        .bind(&consumed_state.transition_id_hex)
        .bind(i64::from(consumed_state.output_index))
        .bind(tenant_id)
        .bind(resume_after_observation_id)
        .bind(resume_after_observation_id)
        .bind(
            i64::try_from(MAX_CLOSURE_CONFLICT_PAGE + 1)
                .map_err(|_| invalid("conflict page bound exceeds SQLite range"))?,
        )
        .fetch_all(&self.pool)
        .await?;

        let has_more = rows.len() > MAX_CLOSURE_CONFLICT_PAGE;
        let page = &rows[..rows.len().min(MAX_CLOSURE_CONFLICT_PAGE)];
        let mut last_observation_id = String::new();
        // Grouped by index domain because that is the unit an
        // `IndexFreshnessReading` describes: one chain's tip bounds only that
        // chain's search.
        let mut domains: Vec<(String, String, Vec<SourceClosureObservationProjectionV1>)> =
            Vec::new();
        for row in page {
            last_observation_id = row.try_get("observation_id")?;
            let chain_id: String = row.try_get("chain_id")?;
            let network_id: String = row.try_get("network_id")?;
            let payload: String = row.try_get("projection_json")?;
            let expected = digest(
                row.try_get("normalized_payload_digest")?,
                "normalized_payload_digest",
            )?;
            let projection = decode_bound_closure_projection(&payload, expected)?;
            match domains
                .iter_mut()
                .find(|(chain, network, _)| chain == &chain_id && network == &network_id)
            {
                Some((_, _, group)) => group.push(projection),
                None => domains.push((chain_id, network_id, vec![projection])),
            }
        }

        let mut competitors: Vec<CompetingClosureReading> = Vec::new();
        let mut searched_domains: Vec<ClosureIndexDomainFreshness> = Vec::new();
        for (chain_id, network_id, group) in &domains {
            let tip = self.closure_index_tip(chain_id, network_id).await?;
            // The lag is measured against this domain's own closures, so the
            // bound reported is the one the search actually ran under.
            let index_freshness = tip.freshness_for(
                &group
                    .first()
                    .map(|projection| projection.observed_checkpoint.clone())
                    .unwrap_or(ObservedOrMissing::Missing {
                        reasons: vec!["no closure was read on this chain and network".to_string()],
                    }),
            );
            if let ClosureConflictSearchOutcome::CompetingClosuresObserved {
                competitors: found,
            } = assess_closure_conflicts(
                consumed_state,
                successor_commitment_hex,
                group,
                &index_freshness,
            ) {
                for competitor in found {
                    if !competitors.contains(&competitor) {
                        competitors.push(competitor);
                    }
                }
            }
            searched_domains.push(ClosureIndexDomainFreshness {
                chain_id: chain_id.clone(),
                network_id: network_id.clone(),
                index_freshness,
            });
        }

        let coverage = if has_more {
            ClosureConflictPageCoverage::MorePagesRemain {
                resume_after_observation_id: last_observation_id,
            }
        } else {
            ClosureConflictPageCoverage::RecordedSetExhausted { searched_domains }
        };
        let page = ClosureConflictPageV1 {
            schema_version: tuppira_shared::CLOSURE_CONFLICT_PAGE_VERSION,
            consumed_state: consumed_state.clone(),
            successor_commitment_hex: successor_commitment_hex.to_string(),
            competitors,
            coverage,
        };
        page.validate()
            .map_err(|error| invalid(&format!("invalid closure conflict page: {error:?}")))?;
        Ok(page)
    }

    /// The closures observed downstream of one source state, tenant-filtered.
    ///
    /// Walks source state → attempted successors → closure observations →
    /// outputs, breadth-first, following the linkage the sources themselves
    /// reported. Tuppira does not decide whether any of that linkage is
    /// protocol-valid: only Parwana's verifier can, and this walk is an
    /// investigator's trail rather than an assurance result.
    ///
    /// Each step reports what its observation *still* establishes, with its
    /// reorganization standing applied, so a step on a replaced history does not
    /// report settlement here when a direct read of it would not.
    ///
    /// # Errors
    ///
    /// Fails closed on a malformed selector, on a stored payload that no longer
    /// matches its digest, and for the same reasons
    /// [`Self::closure_observation_view`] does.
    pub async fn closure_lineage(
        &self,
        root_state: &ConsumedStateReading,
        tenant_id: &str,
    ) -> Result<ClosureLineageV1> {
        ensure_text(&root_state.transition_id_hex, "transition_id_hex")?;
        ensure_text(tenant_id, "tenant_id")?;

        let mut steps: Vec<ClosureLineageStepV1> = Vec::new();
        let mut visited: BTreeSet<String> = BTreeSet::new();
        // Level 0 selects on the root output; every later level selects on a
        // successor commitment and takes whichever of its outputs a source
        // reported a closure for.
        let mut frontier: Vec<(String, Option<u32>)> =
            vec![(root_state.transition_id_hex.clone(), Some(root_state.output_index))];
        let mut depth = 0_u32;
        let mut coverage = ClosureLineageCoverage::Complete;

        while !frontier.is_empty() {
            let bound_reached = depth >= MAX_CLOSURE_LINEAGE_DEPTH;
            let next_depth = depth + 1;
            // The rows of the level below are read even when the walk will not
            // turn them into steps. They are what fills the current steps'
            // `observed_consumed_outputs`, and leaving that empty at the bound
            // would say no consumption was observed when the walk simply
            // stopped looking.
            let mut children: Vec<ClosureLineageStepV1> = Vec::new();
            let mut next_frontier: Vec<(String, Option<u32>)> = Vec::new();
            for (transition_id_hex, output_index) in &frontier {
                let rows = self
                    .closure_rows_consuming(transition_id_hex, *output_index, tenant_id)
                    .await?;
                for (observation_id, payload, expected) in rows {
                    let projection = decode_bound_closure_projection(&payload, expected)?;
                    // Attribute the output to *every* step that named this
                    // successor, which is what makes the edge readable without
                    // the caller re-joining on transition identifiers.
                    //
                    // Every one of them, and before the visited check, because
                    // an empty `observed_consumed_outputs` says no source
                    // reported a closure for any output of that successor.
                    // Two sources reporting one closure are two steps naming
                    // one successor, and a closure reached from two branches is
                    // read once; attributing to the first step only, or
                    // skipping attribution on the second reading, would leave
                    // the other steps saying that of a consumption this walk
                    // has in hand.
                    for step in steps
                        .iter_mut()
                        .filter(|step| step.successor_commitment_hex == *transition_id_hex)
                    {
                        if !step
                            .observed_consumed_outputs
                            .contains(&projection.consumed_state)
                        {
                            step.observed_consumed_outputs
                                .push(projection.consumed_state.clone());
                        }
                    }
                    if !visited.insert(observation_id.clone()) {
                        continue;
                    }
                    if bound_reached {
                        continue;
                    }
                    let standing = self
                        .reorg_standing_for(
                            &observation_id,
                            &projection.consumed_state.transition_id_hex,
                            tenant_id,
                        )
                        .await?;
                    let established_states =
                        established_states_under_reorg(&projection, &standing)
                            .into_iter()
                            .collect();
                    let successor = projection.closure_identity.successor_commitment_hex.clone();
                    children.push(ClosureLineageStepV1 {
                        depth: next_depth,
                        consumed_state: projection.consumed_state.clone(),
                        observation_id,
                        chain_id: projection.chain_id.clone(),
                        network_id: projection.network_id.clone(),
                        closure_kind: projection.closure_identity.closure_kind.clone(),
                        closure_identity_hex: projection
                            .closure_identity
                            .closure_identity_hex
                            .clone(),
                        successor_commitment_hex: successor.clone(),
                        reorg_standing: standing,
                        established_states,
                        observed_consumed_outputs: Vec::new(),
                    });
                    if !next_frontier
                        .iter()
                        .any(|(transition, _)| transition == &successor)
                    {
                        next_frontier.push((successor, None));
                    }
                }
            }
            if bound_reached {
                coverage = ClosureLineageCoverage::TruncatedAtDepth { depth };
                break;
            }
            if steps.len() + children.len() > MAX_CLOSURE_LINEAGE_STEPS {
                // A wide stop is the ordinary shape of heavy equivocation.
                // Trimming to the bound and reporting `Complete` would present
                // the busiest lineage as the tidiest one.
                coverage = ClosureLineageCoverage::TruncatedAtStepLimit { steps: steps.len() };
                break;
            }
            steps.extend(children);
            frontier = next_frontier;
            depth = next_depth;
        }

        let lineage = ClosureLineageV1 {
            schema_version: tuppira_shared::CLOSURE_LINEAGE_VERSION,
            root_state: root_state.clone(),
            steps,
            coverage,
        };
        lineage
            .validate()
            .map_err(|error| invalid(&format!("invalid closure lineage: {error:?}")))?;
        Ok(lineage)
    }

    /// Tenant-visible closure rows consuming one output, or any output of one
    /// transition when `output_index` is `None`.
    async fn closure_rows_consuming(
        &self,
        transition_id_hex: &str,
        output_index: Option<u32>,
        tenant_id: &str,
    ) -> Result<Vec<(String, String, [u8; 32])>> {
        let rows = sqlx::query(
            "SELECT c.observation_id, c.projection_json, o.normalized_payload_digest \
             FROM closure_observations c \
             JOIN observations o ON o.observation_id = c.observation_id \
             WHERE c.consumed_transition_id_hex = ? \
               AND (? IS NULL OR c.consumed_output_index = ?) \
               AND (o.visibility_scope = 'public' OR o.tenant_id = ?) \
             ORDER BY c.observation_id",
        )
        .bind(transition_id_hex)
        .bind(output_index.map(i64::from))
        .bind(output_index.map(i64::from))
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.try_get("observation_id")?,
                    row.try_get("projection_json")?,
                    digest(
                        row.try_get("normalized_payload_digest")?,
                        "normalized_payload_digest",
                    )?,
                ))
            })
            .collect()
    }

    /// The chain evidence behind one normalized closure, tenant-filtered.
    ///
    /// A closure with no recorded chain evidence — one relayed by a non-chain
    /// source, for instance — is [`TuppiraError::NotFound`] rather than an
    /// empty evidence record: an empty way back is not a way back.
    pub async fn closure_evidence(
        &self,
        observation_id: &str,
        tenant_id: &str,
    ) -> Result<ChainClosureEvidenceRecord> {
        self.get_visible_observation(observation_id, tenant_id)
            .await?;
        let row = sqlx::query(
            "SELECT schema_version, native_event_kind, raw_event_digest \
             FROM closure_observation_evidence WHERE observation_id = ?",
        )
        .bind(observation_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| TuppiraError::NotFound {
            entity_type: "closure_chain_evidence".into(),
            id: observation_id.into(),
        })?;
        let evidence_refs = sqlx::query_scalar::<_, String>(
            "SELECT evidence_ref FROM closure_observation_evidence_refs \
             WHERE observation_id = ? ORDER BY ordinal",
        )
        .bind(observation_id)
        .fetch_all(&self.pool)
        .await?;
        let record = ChainClosureEvidenceRecord {
            schema_version: to_u16(row.try_get("schema_version")?, "schema_version")?,
            observation_id: observation_id.to_string(),
            native_event_kind: row.try_get("native_event_kind")?,
            evidence_refs,
            raw_event_digest: digest(row.try_get("raw_event_digest")?, "raw_event_digest")?,
        };
        record
            .validate()
            .map_err(|error| invalid(&format!("invalid stored chain evidence: {error:?}")))?;
        Ok(record)
    }

    /// The closure projection carried by one observation, tenant-filtered.
    ///
    /// An observation without a closure payload is [`TuppiraError::NotFound`],
    /// never an empty or default projection: a caller must not receive a value
    /// shaped like a closure statement when none was ever recorded.
    pub async fn closure_observation(
        &self,
        observation_id: &str,
        tenant_id: &str,
    ) -> Result<SourceClosureObservationProjectionV1> {
        let observation = self.get_visible_observation(observation_id, tenant_id).await?;
        let payload = sqlx::query_scalar::<_, String>(
            "SELECT projection_json FROM closure_observations WHERE observation_id = ?",
        )
        .bind(observation_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| TuppiraError::NotFound {
            entity_type: "closure_observation".into(),
            id: observation_id.into(),
        })?;
        decode_closure_projection(&payload, &observation)
    }

    /// The closure account the observation plane holds for one subject.
    ///
    /// A subject with no recorded closure observation — every Sanad, transfer,
    /// and seal indexed before the closure profile — yields
    /// [`ClosureProfileGeneration::PreClosure`]. The V1 explorer read model is
    /// deliberately not consulted: `sanads.status = 'spent'` is a chain-level
    /// spend the indexer saw, and reporting it as a V2 closure would fabricate
    /// a protocol statement no source ever made.
    ///
    /// # Errors
    ///
    /// Fails closed when a subject carries more closure observations than one
    /// account can hold. This projection has no way to say "partial", so
    /// returning a silently truncated set would understate a conflict; the
    /// caller uses the paginated conflict query for such a subject instead.
    pub async fn subject_closure(
        &self,
        subject_ref: &str,
        tenant_id: &str,
    ) -> Result<SubjectClosureProjectionV1> {
        ensure_text(subject_ref, "subject_ref")?;
        ensure_text(tenant_id, "tenant_id")?;
        let rows = sqlx::query(
            "SELECT o.observation_id, o.observed_at, o.retraction_status, o.normalized_payload_digest, \
             c.projection_json FROM closure_observations c \
             JOIN observations o ON o.observation_id = c.observation_id \
             JOIN observation_subjects s ON s.observation_id = c.observation_id \
             WHERE s.subject_ref = ? AND (o.visibility_scope = 'public' OR o.tenant_id = ?) \
             ORDER BY o.observed_at DESC, o.observation_id DESC LIMIT ?",
        )
        .bind(subject_ref)
        .bind(tenant_id)
        .bind(i64::try_from(MAX_SUBJECT_CLOSURE_OBSERVATIONS + 1).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(SubjectClosureProjectionV1::pre_closure(subject_ref));
        }
        if rows.len() > MAX_SUBJECT_CLOSURE_OBSERVATIONS {
            return Err(invalid(
                "subject carries more closure observations than one account can report",
            ));
        }

        let mut observations = Vec::with_capacity(rows.len());
        for row in rows {
            let observation_id: String = row.try_get("observation_id")?;
            let expected = digest(
                row.try_get("normalized_payload_digest")?,
                "normalized_payload_digest",
            )?;
            let payload: String = row.try_get("projection_json")?;
            observations.push(RecordedSourceClosureObservationV1 {
                observation_id,
                observed_at: to_u64(row.try_get("observed_at")?, "observed_at")?,
                record_retraction_status: decode_retraction(
                    &row.try_get::<String, _>("retraction_status")?,
                )?,
                projection: decode_bound_closure_projection(&payload, expected)?,
            });
        }
        let account = SubjectClosureProjectionV1 {
            schema_version: SUBJECT_CLOSURE_PROFILE_VERSION,
            subject_ref: subject_ref.to_string(),
            closure_generation: ClosureProfileGeneration::SourceClosureV2 { observations },
        };
        account
            .validate()
            .map_err(|error| invalid(&format!("invalid subject closure account: {error:?}")))?;
        Ok(account)
    }

    pub async fn source_health(&self) -> Result<Vec<SourceHealthProjection>> {
        let rows = sqlx::query(
            "SELECT s.source_id, s.connector_kind, s.display_name, \
             (SELECT started_at FROM collection_runs r WHERE r.source_id = s.source_id ORDER BY started_at DESC LIMIT 1) last_run_started_at, \
             (SELECT completed_at FROM collection_runs r WHERE r.source_id = s.source_id ORDER BY started_at DESC LIMIT 1) last_run_completed_at, \
             c.observed_at cursor_observed_at FROM observation_sources s LEFT JOIN sync_cursors c ON c.source_id = s.source_id ORDER BY s.source_id",
        ).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(SourceHealthProjection {
                    source_id: row.try_get("source_id")?,
                    connector_kind: row.try_get("connector_kind")?,
                    display_name: row.try_get("display_name")?,
                    last_run_started_at: optional_u64(
                        row.try_get("last_run_started_at")?,
                        "last_run_started_at",
                    )?,
                    last_run_completed_at: optional_u64(
                        row.try_get("last_run_completed_at")?,
                        "last_run_completed_at",
                    )?,
                    cursor_observed_at: optional_u64(
                        row.try_get("cursor_observed_at")?,
                        "cursor_observed_at",
                    )?,
                })
            })
            .collect()
    }

    pub async fn cursor(&self, source_id: &str) -> Result<SyncCursorRecord> {
        let row = sqlx::query("SELECT schema_version, source_id, cursor_version, cursor, observed_at FROM sync_cursors WHERE source_id = ?")
            .bind(source_id).fetch_optional(&self.pool).await?
            .ok_or_else(|| TuppiraError::NotFound { entity_type: "sync_cursor".into(), id: source_id.into() })?;
        Ok(SyncCursorRecord {
            schema_version: to_u16(row.try_get::<i64, _>("schema_version")?, "schema_version")?,
            source_id: row.try_get("source_id")?,
            cursor_version: to_u16(row.try_get::<i64, _>("cursor_version")?, "cursor_version")?,
            cursor: row.try_get("cursor")?,
            observed_at: to_u64(row.try_get::<i64, _>("observed_at")?, "observed_at")?,
        })
    }

    /// Persist a detected source-history discontinuity without changing any observation.
    pub async fn append_reorg(&self, record: &ReorgRecord) -> Result<()> {
        ensure_version(record.schema_version)?;
        ensure_text(&record.reorg_id, "reorg_id")?;
        ensure_text(&record.source_id, "source_id")?;
        ensure_text(&record.prior_tip, "prior_tip")?;
        ensure_text(&record.replacement_tip, "replacement_tip")?;
        if record.detected_at == 0 || record.prior_tip == record.replacement_tip {
            return Err(invalid("invalid source reorg"));
        }
        sqlx::query("INSERT INTO source_reorgs (reorg_id, schema_version, source_id, detected_at, prior_tip, replacement_tip) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&record.reorg_id).bind(i64::from(record.schema_version)).bind(&record.source_id)
            .bind(required_i64(record.detected_at, "detected_at")?).bind(&record.prior_tip)
            .bind(&record.replacement_tip).execute(&self.pool).await?;
        Ok(())
    }

    /// Preserve a non-authoritative provider disagreement. Database constraints
    /// reject same-source, unrelated-subject, and cross-tenant pairs.
    pub async fn append_contradiction_hint(
        &self,
        record: &ContradictionHintRecord,
        detected_at: u64,
    ) -> Result<()> {
        ensure_version(record.schema_version)?;
        ensure_text(&record.hint_id, "hint_id")?;
        ensure_text(&record.left_observation_id, "left_observation_id")?;
        ensure_text(&record.right_observation_id, "right_observation_id")?;
        ensure_text(&record.detector_id, "detector_id")?;
        if record.left_observation_id == record.right_observation_id || detected_at == 0 {
            return Err(invalid("invalid contradiction hint"));
        }
        sqlx::query("INSERT INTO contradiction_hints (hint_id, schema_version, left_observation_id, right_observation_id, detector_id, detected_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&record.hint_id).bind(i64::from(record.schema_version))
            .bind(&record.left_observation_id).bind(&record.right_observation_id)
            .bind(&record.detector_id).bind(required_i64(detected_at, "detected_at")?)
            .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn reorg_history(&self, source_id: &str) -> Result<Vec<ReorgRecord>> {
        ensure_text(source_id, "source_id")?;
        let rows = sqlx::query("SELECT schema_version, reorg_id, source_id, detected_at, prior_tip, replacement_tip FROM source_reorgs WHERE source_id = ? ORDER BY detected_at, reorg_id")
            .bind(source_id).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ReorgRecord {
                    schema_version: to_u16(row.try_get("schema_version")?, "schema_version")?,
                    reorg_id: row.try_get("reorg_id")?,
                    source_id: row.try_get("source_id")?,
                    detected_at: to_u64(row.try_get("detected_at")?, "detected_at")?,
                    prior_tip: row.try_get("prior_tip")?,
                    replacement_tip: row.try_get("replacement_tip")?,
                })
            })
            .collect()
    }
}

/// Closure observations one subject account can report without truncating.
///
/// Taken from the bound `SubjectClosureProjectionV1::validate` enforces rather
/// than restated here, so a read that fits is a read the projection accepts and
/// the two cannot drift apart.
const MAX_SUBJECT_CLOSURE_OBSERVATIONS: usize = tuppira_shared::MAX_OBSERVATION_REFS;

fn optional_u64(value: Option<i64>, field: &str) -> Result<Option<u64>> {
    value.map(|value| to_u64(value, field)).transpose()
}

async fn insert_closure_projection(
    transaction: &mut Transaction<'_, Sqlite>,
    observation_id: &str,
    projection: &SourceClosureObservationProjectionV1,
    payload: &str,
) -> Result<()> {
    let observed_checkpoint_height = match &projection.observed_checkpoint {
        ObservedOrMissing::Observed { value } => {
            Some(required_i64(value.block_height, "observed_checkpoint.block_height")?)
        }
        ObservedOrMissing::Missing { .. } => None,
    };
    sqlx::query(
        "INSERT INTO closure_observations (observation_id, profile_id, profile_version, chain_id, network_id, closure_kind, closure_identity_hex, consumed_transition_id_hex, consumed_output_index, successor_commitment_hex, observed_checkpoint_height, indexed_tip_height, projection_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(observation_id)
    .bind(SOURCE_CLOSURE_OBSERVATION_PROFILE_ID)
    .bind(i64::from(projection.schema_version))
    .bind(&projection.chain_id)
    .bind(&projection.network_id)
    .bind(&projection.closure_identity.closure_kind)
    .bind(&projection.closure_identity.closure_identity_hex)
    .bind(&projection.consumed_state.transition_id_hex)
    .bind(i64::from(projection.consumed_state.output_index))
    .bind(&projection.closure_identity.successor_commitment_hex)
    .bind(observed_checkpoint_height)
    .bind(required_i64(
        projection.index_freshness.indexed_tip_height,
        "index_freshness.indexed_tip_height",
    )?)
    .bind(payload)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn insert_closure_evidence(
    transaction: &mut Transaction<'_, Sqlite>,
    evidence: &ChainClosureEvidenceRecord,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO closure_observation_evidence (observation_id, schema_version, native_event_kind, raw_event_digest) VALUES (?, ?, ?, ?)",
    )
    .bind(&evidence.observation_id)
    .bind(i64::from(evidence.schema_version))
    .bind(&evidence.native_event_kind)
    .bind(evidence.raw_event_digest.as_slice())
    .execute(&mut **transaction)
    .await?;
    for (ordinal, reference) in evidence.evidence_refs.iter().enumerate() {
        sqlx::query(
            "INSERT INTO closure_observation_evidence_refs (observation_id, ordinal, evidence_ref) VALUES (?, ?, ?)",
        )
        .bind(&evidence.observation_id)
        .bind(i64::try_from(ordinal).map_err(|_| invalid("evidence ordinal exceeds SQLite range"))?)
        .bind(reference)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

/// Record an index tip, keeping the recorded one when it is already higher.
///
/// The `WHERE` clause is what makes this an advance rather than a write. The
/// `closure_index_tips_advance_only` trigger would abort a lowering update, and
/// aborting is right for a direct `UPDATE`; here it would fail an otherwise
/// valid observation append because a connector read blocks out of order. So a
/// lower reading is discarded and reported as "did not advance" instead.
async fn upsert_closure_index_tip(
    transaction: &mut Transaction<'_, Sqlite>,
    record: &ClosureIndexTipRecord,
) -> Result<bool> {
    let outcome = sqlx::query(
        "INSERT INTO closure_index_tips (chain_id, network_id, indexed_tip_height, indexed_tip_block_id_hex, indexed_tip_observed_at) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(chain_id, network_id) DO UPDATE SET \
           indexed_tip_height = excluded.indexed_tip_height, \
           indexed_tip_block_id_hex = excluded.indexed_tip_block_id_hex, \
           indexed_tip_observed_at = excluded.indexed_tip_observed_at \
         WHERE excluded.indexed_tip_height > closure_index_tips.indexed_tip_height",
    )
    .bind(&record.chain_id)
    .bind(&record.network_id)
    .bind(required_i64(
        record.indexed_tip_height,
        "indexed_tip_height",
    )?)
    .bind(&record.indexed_tip_block_id_hex)
    .bind(required_i64(
        record.indexed_tip_observed_at,
        "indexed_tip_observed_at",
    )?)
    .execute(&mut **transaction)
    .await?;
    Ok(outcome.rows_affected() > 0)
}

fn decode_closure_projection(
    payload: &str,
    observation: &ObservationRecord,
) -> Result<SourceClosureObservationProjectionV1> {
    decode_bound_closure_projection(payload, observation.normalized_payload_digest)
}

/// Decode stored closure bytes and re-check them against the digest the
/// observation committed to.
///
/// The check is repeated on every read rather than trusted from write time.
/// The append-only triggers stop this table being edited through SQL, but they
/// say nothing about the bytes arriving corrupted or being replaced beneath the
/// process; a payload that no longer hashes to its commitment is not a closure
/// statement and must not be returned as one.
fn decode_bound_closure_projection(
    payload: &str,
    expected_digest: [u8; 32],
) -> Result<SourceClosureObservationProjectionV1> {
    let projection: SourceClosureObservationProjectionV1 = serde_json::from_str(payload)
        .map_err(|error| invalid(&format!("undecodable closure projection: {error}")))?;
    projection
        .validate()
        .map_err(|error| invalid(&format!("invalid stored closure projection: {error:?}")))?;
    let digest = projection
        .normalized_payload_digest()
        .map_err(|error| invalid(&format!("closure projection digest: {error:?}")))?;
    if digest != expected_digest {
        return Err(invalid(
            "stored closure payload does not match the digest its observation committed to",
        ));
    }
    Ok(projection)
}

fn decode_retraction(value: &str) -> Result<RetractionStatus> {
    match value {
        "active" => Ok(RetractionStatus::Active),
        "retracted" => Ok(RetractionStatus::Retracted),
        _ => Err(invalid("unsupported retraction status")),
    }
}

async fn insert_observation(
    transaction: &mut Transaction<'_, Sqlite>,
    record: &ObservationRecord,
) -> Result<()> {
    let (scope, tenant_id) = match &record.tenant_visibility {
        TenantVisibility::Public => ("public", None),
        TenantVisibility::Tenant { tenant_id } => ("tenant", Some(tenant_id.as_str())),
    };
    let retraction = match record.retraction_status {
        RetractionStatus::Active => "active",
        RetractionStatus::Retracted => "retracted",
    };
    sqlx::query("INSERT INTO observations (observation_id, schema_version, source_id, source_event_id, source_event_type, asserted_event_time, observed_at, normalized_profile_id, normalized_profile_version, normalized_payload_digest, raw_payload_digest, collection_run_id, supersedes_observation_id, retraction_status, visibility_scope, tenant_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(&record.observation_id).bind(i64::from(record.schema_version)).bind(&record.source_id)
        .bind(&record.source_event_id).bind(&record.source_event_type).bind(optional_i64(record.asserted_event_time, "asserted_event_time")?)
        .bind(required_i64(record.observed_at, "observed_at")?).bind(&record.normalized_profile_id)
        .bind(i64::from(record.normalized_profile_version)).bind(record.normalized_payload_digest.as_slice())
        .bind(record.raw_payload_digest.as_ref().map(<[u8; 32]>::as_slice)).bind(&record.collection_run_id).bind(&record.supersedes)
        .bind(retraction).bind(scope).bind(tenant_id).execute(&mut **transaction).await?;
    for subject in &record.subject_refs {
        sqlx::query("INSERT INTO observation_subjects (observation_id, subject_ref, subject_kind) VALUES (?, ?, 'reference')")
            .bind(&record.observation_id).bind(subject).execute(&mut **transaction).await?;
    }
    for reference in &record.authenticity_material_refs {
        sqlx::query("INSERT INTO observation_authenticity_refs (observation_id, authenticity_material_ref) VALUES (?, ?)")
            .bind(&record.observation_id).bind(reference).execute(&mut **transaction).await?;
    }
    if let Some(predecessor) = &record.supersedes {
        sqlx::query("INSERT INTO supersessions (superseding_observation_id, superseded_observation_id, schema_version, observed_at) VALUES (?, ?, ?, ?)")
            .bind(&record.observation_id).bind(predecessor).bind(i64::from(record.schema_version))
            .bind(required_i64(record.observed_at, "observed_at")?).execute(&mut **transaction).await?;
    }
    Ok(())
}

async fn upsert_cursor(
    transaction: &mut Transaction<'_, Sqlite>,
    cursor: &SyncCursorRecord,
) -> Result<()> {
    let result = sqlx::query("INSERT INTO sync_cursors (source_id, schema_version, cursor_version, cursor, observed_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(source_id) DO UPDATE SET schema_version = excluded.schema_version, cursor_version = excluded.cursor_version, cursor = excluded.cursor, observed_at = excluded.observed_at WHERE excluded.observed_at > sync_cursors.observed_at")
        .bind(&cursor.source_id).bind(i64::from(cursor.schema_version)).bind(i64::from(cursor.cursor_version))
        .bind(&cursor.cursor).bind(required_i64(cursor.observed_at, "observed_at")?).execute(&mut **transaction).await?;
    if result.rows_affected() != 1 {
        return Err(invalid("cursor must advance monotonically"));
    }
    Ok(())
}

fn validate_cursor(cursor: &SyncCursorRecord, source_id: &str, minimum_time: u64) -> Result<()> {
    ensure_version(cursor.schema_version)?;
    if cursor.source_id != source_id
        || cursor.cursor_version == 0
        || cursor.cursor.is_empty()
        || cursor.observed_at < minimum_time
    {
        return Err(invalid(
            "cursor does not match or advance the observation batch",
        ));
    }
    Ok(())
}

fn decode_observation(
    row: sqlx::sqlite::SqliteRow,
    subject_refs: Vec<String>,
    authenticity_material_refs: Vec<String>,
    supersedes: Option<String>,
) -> Result<ObservationRecord> {
    let normalized = digest(
        row.try_get("normalized_payload_digest")?,
        "normalized_payload_digest",
    )?;
    let raw = row
        .try_get::<Option<Vec<u8>>, _>("raw_payload_digest")?
        .map(|bytes| digest(bytes, "raw_payload_digest"))
        .transpose()?;
    let tenant_visibility = match row.try_get::<String, _>("visibility_scope")?.as_str() {
        "public" => TenantVisibility::Public,
        "tenant" => TenantVisibility::Tenant {
            tenant_id: row
                .try_get::<Option<String>, _>("tenant_id")?
                .ok_or_else(|| invalid("tenant visibility missing tenant_id"))?,
        },
        _ => return Err(invalid("unsupported visibility scope")),
    };
    let retraction_status = decode_retraction(&row.try_get::<String, _>("retraction_status")?)?;
    Ok(ObservationRecord {
        schema_version: to_u16(row.try_get("schema_version")?, "schema_version")?,
        observation_id: row.try_get("observation_id")?,
        source_id: row.try_get("source_id")?,
        source_event_id: row.try_get("source_event_id")?,
        source_event_type: row.try_get("source_event_type")?,
        subject_refs,
        asserted_event_time: row
            .try_get::<Option<i64>, _>("asserted_event_time")?
            .map(|v| to_u64(v, "asserted_event_time"))
            .transpose()?,
        observed_at: to_u64(row.try_get("observed_at")?, "observed_at")?,
        normalized_profile_id: row.try_get("normalized_profile_id")?,
        normalized_profile_version: to_u16(
            row.try_get("normalized_profile_version")?,
            "normalized_profile_version",
        )?,
        normalized_payload_digest: normalized,
        raw_payload_digest: raw,
        authenticity_material_refs,
        collection_run_id: row.try_get("collection_run_id")?,
        supersedes,
        retraction_status,
        tenant_visibility,
    })
}

fn ensure_version(version: u16) -> Result<()> {
    if version == 1 {
        Ok(())
    } else {
        Err(invalid("unsupported schema version"))
    }
}
fn ensure_text(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        Err(invalid(&format!("invalid {field}")))
    } else {
        Ok(())
    }
}
fn ensure_digest(value: &[u8; 32], field: &str) -> Result<()> {
    if *value == [0; 32] {
        Err(invalid(&format!("invalid {field}")))
    } else {
        Ok(())
    }
}
fn required_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| invalid(&format!("{field} exceeds SQLite range")))
}
fn optional_i64(value: Option<u64>, field: &str) -> Result<Option<i64>> {
    value.map(|v| required_i64(v, field)).transpose()
}
fn to_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| invalid(&format!("negative {field}")))
}
fn to_u16(value: i64, field: &str) -> Result<u16> {
    u16::try_from(value).map_err(|_| invalid(&format!("invalid {field}")))
}
fn digest(bytes: Vec<u8>, field: &str) -> Result<[u8; 32]> {
    bytes
        .try_into()
        .map_err(|_| invalid(&format!("invalid {field}")))
}
fn invalid(message: &str) -> TuppiraError {
    TuppiraError::Parse(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuppira_shared::ClosureObservationState;
    use tuppira_shared::OBSERVATION_SCHEMA_VERSION;

    async fn repository() -> Result<ObservationRepository> {
        let pool = crate::init_pool("sqlite::memory:", 1).await?;
        let repository = ObservationRepository::new(pool);
        let retention = RetentionClassRecord {
            schema_version: 1,
            retention_class_id: "retention:audit".into(),
            purpose: "forensic audit".into(),
            retain_for_seconds: Some(86_400),
            raw_payload_permitted: true,
        };
        assert!(repository.insert_retention_class(&retention).await.is_ok());
        let source = SourceRecord {
            schema_version: 1,
            source_id: "source:piteka".into(),
            connector_kind: "piteka-export".into(),
            display_name: "Piteka evidence export".into(),
            retention_class_id: retention.retention_class_id,
        };
        assert!(repository.insert_source(&source).await.is_ok());
        let run = CollectionRunRecord {
            schema_version: 1,
            collection_run_id: "run:1".into(),
            source_id: source.source_id,
            started_at: 10,
            completed_at: Some(11),
        };
        assert!(repository.insert_collection_run(&run).await.is_ok());
        Ok(repository)
    }

    async fn add_source(repository: &ObservationRepository, source_id: &str, run_id: &str) {
        let source = SourceRecord {
            schema_version: 1,
            source_id: source_id.into(),
            connector_kind: "provider-fixture".into(),
            display_name: source_id.into(),
            retention_class_id: "retention:audit".into(),
        };
        assert!(repository.insert_source(&source).await.is_ok());
        let run = CollectionRunRecord {
            schema_version: 1,
            collection_run_id: run_id.into(),
            source_id: source_id.into(),
            started_at: 10,
            completed_at: Some(11),
        };
        assert!(repository.insert_collection_run(&run).await.is_ok());
    }

    fn observation(id: &str, digest_byte: u8, supersedes: Option<&str>) -> ObservationRecord {
        ObservationRecord {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            observation_id: id.into(),
            source_id: "source:piteka".into(),
            source_event_id: "event:deployment:42".into(),
            source_event_type: "deployment.status".into(),
            subject_refs: vec!["deployment:42".into()],
            asserted_event_time: Some(10),
            observed_at: 12,
            normalized_profile_id: "deployment-observation".into(),
            normalized_profile_version: 1,
            normalized_payload_digest: [digest_byte; 32],
            raw_payload_digest: Some([9; 32]),
            authenticity_material_refs: vec!["signature:42".into()],
            collection_run_id: "run:1".into(),
            supersedes: supersedes.map(str::to_string),
            retraction_status: RetractionStatus::Active,
            tenant_visibility: TenantVisibility::Tenant {
                tenant_id: "tenant:acme".into(),
            },
        }
    }

    fn cursor(time: u64) -> SyncCursorRecord {
        SyncCursorRecord {
            schema_version: 1,
            source_id: "source:piteka".into(),
            cursor_version: 1,
            cursor: vec![1, 2, 3],
            observed_at: time,
        }
    }

    fn for_source(
        mut value: ObservationRecord,
        source: &str,
        event: &str,
        run: &str,
    ) -> ObservationRecord {
        value.source_id = source.into();
        value.source_event_id = event.into();
        value.collection_run_id = run.into();
        value
    }

    fn cursor_for(source: &str, time: u64) -> SyncCursorRecord {
        SyncCursorRecord {
            source_id: source.into(),
            ..cursor(time)
        }
    }

    #[tokio::test]
    async fn preserves_chain_reorg_history_without_overwriting_observations() {
        let Ok(repository) = repository().await else {
            return;
        };
        let original = observation("obs:chain:old", 1, None);
        assert!(
            repository
                .append_observation(&original, &cursor(12))
                .await
                .is_ok()
        );
        let reorg = ReorgRecord {
            schema_version: 1,
            reorg_id: "reorg:fixture:1".into(),
            source_id: "source:piteka".into(),
            detected_at: 13,
            prior_tip: "block:100:a".into(),
            replacement_tip: "block:100:b".into(),
        };
        assert!(repository.append_reorg(&reorg).await.is_ok());
        assert_eq!(
            repository.reorg_history("source:piteka").await.ok(),
            Some(vec![reorg])
        );
        assert_eq!(
            repository.get_observation("obs:chain:old").await.ok(),
            Some(original)
        );
        let overwrite = sqlx::query("UPDATE source_reorgs SET replacement_tip = 'block:other' WHERE reorg_id = 'reorg:fixture:1'")
            .execute(&repository.pool).await;
        assert!(overwrite.is_err());
    }

    #[tokio::test]
    async fn preserves_cross_provider_disagreement_and_rejects_ambiguous_pairs() {
        let Ok(repository) = repository().await else {
            return;
        };
        add_source(&repository, "source:provider-b", "run:b").await;
        let left = observation("obs:provider:a", 1, None);
        let right = for_source(
            observation("obs:provider:b", 2, None),
            "source:provider-b",
            "event:b",
            "run:b",
        );
        assert!(
            repository
                .append_observation(&left, &cursor(12))
                .await
                .is_ok()
        );
        assert!(
            repository
                .append_observation(&right, &cursor_for("source:provider-b", 12))
                .await
                .is_ok()
        );
        let hint = ContradictionHintRecord {
            schema_version: 1,
            hint_id: "hint:fixture:1".into(),
            left_observation_id: left.observation_id.clone(),
            right_observation_id: right.observation_id.clone(),
            detector_id: "detector:fixture:v1".into(),
        };
        assert!(
            repository
                .append_contradiction_hint(&hint, 13)
                .await
                .is_ok()
        );
        assert_eq!(
            repository.get_observation(&left.observation_id).await.ok(),
            Some(left.clone())
        );
        assert_eq!(
            repository.get_observation(&right.observation_id).await.ok(),
            Some(right)
        );

        let same_source = ContradictionHintRecord {
            hint_id: "hint:same-source".into(),
            right_observation_id: left.observation_id.clone(),
            ..hint.clone()
        };
        assert!(
            repository
                .append_contradiction_hint(&same_source, 14)
                .await
                .is_err()
        );

        let mut foreign = for_source(
            observation("obs:tenant:other", 3, None),
            "source:provider-b",
            "event:foreign",
            "run:b",
        );
        foreign.tenant_visibility = TenantVisibility::Tenant {
            tenant_id: "tenant:other".into(),
        };
        foreign.observed_at = 15;
        assert!(
            repository
                .append_observation(&foreign, &cursor_for("source:provider-b", 15))
                .await
                .is_ok()
        );
        let cross_tenant = ContradictionHintRecord {
            hint_id: "hint:cross-tenant".into(),
            right_observation_id: foreign.observation_id,
            ..hint
        };
        assert!(
            repository
                .append_contradiction_hint(&cross_tenant, 16)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn appends_round_trips_and_reports_lineage() {
        let Ok(repository) = repository().await else {
            return;
        };
        let first = observation("obs:1", 1, None);
        assert!(
            repository
                .append_observation(&first, &cursor(12))
                .await
                .is_ok()
        );
        let mut second = observation("obs:2", 2, Some("obs:1"));
        second.observed_at = 13;
        assert!(
            repository
                .append_observation(&second, &cursor(13))
                .await
                .is_ok()
        );
        assert_eq!(repository.get_observation("obs:2").await.ok(), Some(second));
        assert_eq!(
            repository.lineage("obs:2").await.ok(),
            Some(vec!["obs:2".into(), "obs:1".into()])
        );
        assert_eq!(
            repository.cursor("source:piteka").await.ok(),
            Some(cursor(13))
        );
    }

    #[tokio::test]
    async fn visibility_hides_cross_tenant_records_and_lineage() {
        let Ok(repository) = repository().await else {
            return;
        };
        assert!(
            repository
                .append_observation(&observation("obs:1", 1, None), &cursor(12))
                .await
                .is_ok()
        );
        assert!(
            repository
                .get_visible_observation("obs:1", "tenant:acme")
                .await
                .is_ok()
        );
        assert!(matches!(
            repository
                .get_visible_observation("obs:1", "tenant:other")
                .await,
            Err(TuppiraError::NotFound { .. })
        ));
        assert!(matches!(
            repository.visible_lineage("obs:1", "tenant:other").await,
            Err(TuppiraError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn source_health_reports_runs_and_cursor_without_raw_descriptors() {
        let Ok(repository) = repository().await else {
            return;
        };
        assert!(
            repository
                .append_observation(&observation("obs:1", 1, None), &cursor(12))
                .await
                .is_ok()
        );
        let health = repository.source_health().await;
        assert!(health.is_ok());
        let health = health.unwrap_or_default();
        assert_eq!(health.len(), 1);
        assert_eq!(health[0].source_id, "source:piteka");
        assert_eq!(health[0].last_run_completed_at, Some(11));
        assert_eq!(health[0].cursor_observed_at, Some(12));
    }

    #[tokio::test]
    async fn database_rejects_mutation_and_deletion_of_evidence() {
        let Ok(repository) = repository().await else {
            return;
        };
        assert!(
            repository
                .append_observation(&observation("obs:1", 1, None), &cursor(12))
                .await
                .is_ok()
        );
        let update = sqlx::query(
            "UPDATE observations SET source_event_type = 'changed' WHERE observation_id = 'obs:1'",
        )
        .execute(&repository.pool)
        .await;
        let delete = sqlx::query("DELETE FROM observations WHERE observation_id = 'obs:1'")
            .execute(&repository.pool)
            .await;
        assert!(update.is_err());
        assert!(delete.is_err());
        assert!(repository.get_observation("obs:1").await.is_ok());
    }

    #[tokio::test]
    async fn stale_cursor_rolls_back_observation_and_cross_source_cursor_fails_closed() {
        let Ok(repository) = repository().await else {
            return;
        };
        assert!(
            repository
                .append_observation(&observation("obs:1", 1, None), &cursor(12))
                .await
                .is_ok()
        );
        let mut second = observation("obs:2", 2, Some("obs:1"));
        second.observed_at = 13;
        assert!(
            repository
                .append_observation(&second, &cursor(12))
                .await
                .is_err()
        );
        assert!(repository.get_observation("obs:2").await.is_err());

        let mut wrong_source = cursor(14);
        wrong_source.source_id = "source:other".into();
        assert!(
            repository
                .append_observation(&second, &wrong_source)
                .await
                .is_err()
        );
        assert!(repository.get_observation("obs:2").await.is_err());
    }

    #[tokio::test]
    async fn duplicate_and_missing_lineage_are_rejected_without_partial_rows() {
        let Ok(repository) = repository().await else {
            return;
        };
        let first = observation("obs:1", 1, None);
        assert!(
            repository
                .append_observation(&first, &cursor(12))
                .await
                .is_ok()
        );
        assert!(
            repository
                .append_observation(&first, &cursor(13))
                .await
                .is_err()
        );

        let mut orphan = observation("obs:orphan", 3, Some("obs:missing"));
        orphan.observed_at = 14;
        assert!(
            repository
                .append_observation(&orphan, &cursor(14))
                .await
                .is_err()
        );
        assert!(repository.get_observation("obs:orphan").await.is_err());

        let mut unlinked_correction = observation("obs:correction", 4, None);
        unlinked_correction.source_event_id = first.source_event_id;
        unlinked_correction.observed_at = 15;
        assert!(
            repository
                .append_observation(&unlinked_correction, &cursor(15))
                .await
                .is_err()
        );
        assert!(repository.get_observation("obs:correction").await.is_err());
    }

    // ── V2 source-closure observations (TUP-NE-002) ──────────────────────────

    fn closure_projection() -> SourceClosureObservationProjectionV1 {
        use tuppira_shared::{
            ClosureIdentityReading, ClosureSettlementReading, ConsumedStateReading,
            IndexFreshnessReading, ObservedCheckpointReading, SourceReportedSettlement,
        };
        SourceClosureObservationProjectionV1 {
            schema_version: CLOSURE_OBSERVATION_PROFILE_VERSION,
            consumed_state: ConsumedStateReading {
                transition_id_hex: "11".repeat(32),
                output_index: 0,
                state_type: 1,
            },
            closure_identity: ClosureIdentityReading {
                closure_kind: "evm-nullifier".into(),
                closure_identity_hex: "0f1e2d3c".into(),
                successor_commitment_hex: "22".repeat(32),
            },
            chain_id: "ethereum".into(),
            network_id: "sepolia".into(),
            successor_output_refs: vec!["output:0".into()],
            observed_checkpoint: ObservedOrMissing::Observed {
                value: ObservedCheckpointReading {
                    block_height: 900,
                    block_id_hex: "abcdef01".into(),
                },
            },
            settlement: ObservedOrMissing::Observed {
                value: ClosureSettlementReading {
                    finality_policy: "confirmations".into(),
                    observed_depth: 64,
                    required_depth: 12,
                    reported_settlement: SourceReportedSettlement::Final,
                },
            },
            external_verification: ObservedOrMissing::Missing {
                reasons: vec!["no verifier reported one".into()],
            },
            revocation: ObservedOrMissing::Missing {
                reasons: vec!["source exposes no retraction feed".into()],
            },
            index_freshness: IndexFreshnessReading {
                indexed_tip_height: 1_000,
                indexed_tip_block_id_hex: "beef".into(),
                indexed_tip_observed_at: 1_760_000_100,
                lag_blocks: ObservedOrMissing::Observed { value: 100 },
            },
        }
    }

    /// An observation that correctly declares and commits to the closure payload.
    fn closure_observation_record(
        id: &str,
        subject: &str,
        projection: &SourceClosureObservationProjectionV1,
    ) -> ObservationRecord {
        let mut record = observation(id, 1, None);
        record.source_event_id = format!("event:{id}");
        record.source_event_type = "chain.closure".into();
        record.subject_refs = vec![subject.into()];
        record.normalized_profile_id = SOURCE_CLOSURE_OBSERVATION_PROFILE_ID.into();
        record.normalized_profile_version = CLOSURE_OBSERVATION_PROFILE_VERSION;
        record.normalized_payload_digest = projection
            .normalized_payload_digest()
            .expect("closure projection encodes");
        record
    }

    #[tokio::test]
    async fn a_closure_observation_round_trips_and_reports_its_states() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );

        assert_eq!(
            repository
                .closure_observation("obs:closure:1", "tenant:acme")
                .await
                .ok(),
            Some(projection.clone())
        );
        let account = repository.subject_closure("sanad:1", "tenant:acme").await;
        assert!(account.is_ok());
        let Ok(account) = account else { return };
        assert!(matches!(
            account.closure_generation,
            ClosureProfileGeneration::SourceClosureV2 { .. }
        ));
        let states = account.established_states();
        assert!(states.contains(&tuppira_shared::ClosureObservationState::Observed));
        assert!(states.contains(&tuppira_shared::ClosureObservationState::Final));
        // Nothing reported a foreign verdict, so nothing establishes one.
        assert!(!states.contains(&tuppira_shared::ClosureObservationState::VerifiedElsewhere));
    }

    /// The migration must not let a pre-migration Sanad acquire a closure it
    /// never had. Its stored `status = 'spent'` is a chain-level spend, and the
    /// closure account for it is Unknown — the whole point of this ticket.
    #[tokio::test]
    async fn a_pre_migration_sanad_reports_unknown_closure_not_a_fabricated_one() {
        let Ok(repository) = repository().await else {
            return;
        };
        let spent = sqlx::query(
            "INSERT INTO sanads (id, chain, seal_ref, commitment, owner, created_at, created_tx, status, transfer_count) \
             VALUES ('sanad:legacy', 'ethereum', 'seal:1', 'commit:1', 'owner:1', 1, 'tx:1', 'spent', 2)",
        )
        .execute(&repository.pool)
        .await;
        assert!(spent.is_ok());

        let account = repository
            .subject_closure("sanad:legacy", "tenant:acme")
            .await;
        assert_eq!(
            account.as_ref().map(SubjectClosureProjectionV1::established_states).ok(),
            Some(std::collections::BTreeSet::from([
                tuppira_shared::ClosureObservationState::Unknown
            ]))
        );
        let Ok(account) = account else { return };
        let ClosureProfileGeneration::PreClosure { reasons } = &account.closure_generation else {
            panic!("a Sanad with no closure observation must not report a V2 generation");
        };
        assert!(!reasons.is_empty(), "absence must say why");
        // The V1 read model is untouched and still means what it meant.
        let status: Result<String> =
            sqlx::query_scalar("SELECT status FROM sanads WHERE id = 'sanad:legacy'")
                .fetch_one(&repository.pool)
                .await
                .map_err(Into::into);
        assert_eq!(status.ok(), Some("spent".to_string()));
    }

    #[tokio::test]
    async fn a_payload_the_observation_did_not_commit_to_is_rejected() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let mut record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        record.normalized_payload_digest = [5; 32];

        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_err()
        );
        // The rollback is total: no observation row survives the rejection.
        assert!(repository.get_observation("obs:closure:1").await.is_err());
    }

    #[tokio::test]
    async fn a_closure_payload_under_a_foreign_profile_is_rejected() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let mut record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        record.normalized_profile_id = "org.diewan.some-other-profile.v1".into();

        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_err()
        );
        assert!(repository.get_observation("obs:closure:1").await.is_err());
    }

    #[tokio::test]
    async fn a_corrupted_stored_payload_is_not_returned_as_a_closure() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );
        // Rewriting the payload is refused outright; were it not, the digest
        // re-check on read is the second line that keeps it out of a result.
        let overwrite = sqlx::query(
            "UPDATE closure_observations SET projection_json = '{}' WHERE observation_id = 'obs:closure:1'",
        )
        .execute(&repository.pool)
        .await;
        assert!(overwrite.is_err());
        assert!(
            repository
                .closure_observation("obs:closure:1", "tenant:acme")
                .await
                .is_ok()
        );
    }

    /// Two closures competing for one consumed state is equivocation. Both must
    /// be storable and both must appear: rejecting the second would leave the
    /// first looking uncontested.
    #[tokio::test]
    async fn competing_closures_for_one_consumed_state_are_both_kept() {
        let Ok(repository) = repository().await else {
            return;
        };
        add_source(&repository, "source:provider-b", "run:b").await;
        let first = closure_projection();
        let mut second = closure_projection();
        second.closure_identity.successor_commitment_hex = "33".repeat(32);
        second.successor_output_refs = vec!["output:1".into()];

        let first_record = closure_observation_record("obs:closure:1", "sanad:1", &first);
        let mut second_record = closure_observation_record("obs:closure:2", "sanad:1", &second);
        second_record.source_id = "source:provider-b".into();
        second_record.collection_run_id = "run:b".into();
        second_record.observed_at = 13;

        assert!(
            repository
                .append_closure_observation(&first_record, &first, None, &cursor(12))
                .await
                .is_ok()
        );
        assert!(
            repository
                .append_closure_observation(
                    &second_record,
                    &second,
                    None,
                    &cursor_for("source:provider-b", 13)
                )
                .await
                .is_ok()
        );

        let Ok(account) = repository.subject_closure("sanad:1", "tenant:acme").await else {
            panic!("both competing closures must be readable");
        };
        let ClosureProfileGeneration::SourceClosureV2 { observations } = &account.closure_generation
        else {
            panic!("recorded closures must report a V2 generation");
        };
        assert_eq!(observations.len(), 2);
        // Newest acquisition first, and the two successors stay distinct.
        assert_eq!(observations[0].observation_id, "obs:closure:2");
        assert_ne!(
            observations[0].projection.closure_identity.successor_commitment_hex,
            observations[1].projection.closure_identity.successor_commitment_hex
        );
    }

    // ── Chain evidence stays reachable from the normalized closure (TUP-NE-003)

    fn chain_evidence(observation_id: &str, kind: &str) -> ChainClosureEvidenceRecord {
        ChainClosureEvidenceRecord {
            schema_version: 1,
            observation_id: observation_id.into(),
            native_event_kind: kind.into(),
            evidence_refs: vec![
                "ethereum:sepolia:contract:cc".into(),
                "ethereum:sepolia:log:ee:3".into(),
            ],
            raw_event_digest: [6; 32],
        }
    }

    #[tokio::test]
    async fn chain_evidence_is_stored_with_its_closure_and_read_back_in_order() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        let evidence = chain_evidence("obs:closure:1", "evm-nullifier");
        assert!(
            repository
                .append_closure_observation(&record, &projection, Some(&evidence), &cursor(12))
                .await
                .is_ok()
        );

        assert_eq!(
            repository
                .closure_evidence("obs:closure:1", "tenant:acme")
                .await
                .ok(),
            Some(evidence)
        );
    }

    #[tokio::test]
    async fn evidence_for_the_wrong_closure_family_is_rejected_whole() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        // The projection is an EVM nullifier; the evidence claims a Bitcoin spend.
        let mismatched = chain_evidence("obs:closure:1", "bitcoin-outpoint-spend");

        assert!(
            repository
                .append_closure_observation(&record, &projection, Some(&mismatched), &cursor(12))
                .await
                .is_err()
        );
        // Nothing partial survives: no observation, no closure, no evidence.
        assert!(repository.get_observation("obs:closure:1").await.is_err());
    }

    #[tokio::test]
    async fn evidence_addressed_to_another_observation_is_rejected_whole() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        let misaddressed = chain_evidence("obs:closure:elsewhere", "evm-nullifier");

        assert!(
            repository
                .append_closure_observation(&record, &projection, Some(&misaddressed), &cursor(12))
                .await
                .is_err()
        );
        assert!(repository.get_observation("obs:closure:1").await.is_err());
    }

    #[tokio::test]
    async fn a_closure_without_chain_evidence_reports_absence_rather_than_an_empty_record() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );

        // The closure itself reads fine; only its chain evidence is absent, and
        // absence is an error rather than an empty way back.
        assert!(
            repository
                .closure_observation("obs:closure:1", "tenant:acme")
                .await
                .is_ok()
        );
        assert!(matches!(
            repository
                .closure_evidence("obs:closure:1", "tenant:acme")
                .await,
            Err(TuppiraError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn stored_chain_evidence_cannot_be_rewritten() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        let evidence = chain_evidence("obs:closure:1", "evm-nullifier");
        assert!(
            repository
                .append_closure_observation(&record, &projection, Some(&evidence), &cursor(12))
                .await
                .is_ok()
        );

        for statement in [
            "UPDATE closure_observation_evidence SET raw_event_digest = zeroblob(32) WHERE observation_id = 'obs:closure:1'",
            "DELETE FROM closure_observation_evidence WHERE observation_id = 'obs:closure:1'",
            "UPDATE closure_observation_evidence_refs SET evidence_ref = 'elsewhere' WHERE observation_id = 'obs:closure:1'",
        ] {
            assert!(
                sqlx::query(statement).execute(&repository.pool).await.is_err(),
                "{statement}"
            );
        }
        assert!(
            repository
                .closure_evidence("obs:closure:1", "tenant:acme")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn chain_evidence_stays_inside_the_tenant_boundary() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        let evidence = chain_evidence("obs:closure:1", "evm-nullifier");
        assert!(
            repository
                .append_closure_observation(&record, &projection, Some(&evidence), &cursor(12))
                .await
                .is_ok()
        );

        assert!(matches!(
            repository
                .closure_evidence("obs:closure:1", "tenant:other")
                .await,
            Err(TuppiraError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn closure_reads_stay_inside_the_tenant_boundary() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );

        assert!(matches!(
            repository
                .closure_observation("obs:closure:1", "tenant:other")
                .await,
            Err(TuppiraError::NotFound { .. })
        ));
        // A foreign tenant sees the subject as pre-closure rather than being
        // told a closure exists that it may not read.
        let Ok(account) = repository.subject_closure("sanad:1", "tenant:other").await else {
            panic!("a cross-tenant closure read must succeed as pre-closure");
        };
        assert!(matches!(
            account.closure_generation,
            ClosureProfileGeneration::PreClosure { .. }
        ));
    }

    #[tokio::test]
    async fn raw_descriptor_is_validated_and_immutable() {
        let Ok(repository) = repository().await else {
            return;
        };
        let descriptor = RawPayloadDescriptor {
            schema_version: 1,
            payload_id: "payload:1".into(),
            digest_algorithm: "sha-256".into(),
            payload_digest: [4; 32],
            media_type: "application/json".into(),
            byte_length: 42,
            custody_locator: Some("object:restricted/1".into()),
            retention_class_id: "retention:audit".into(),
        };
        assert!(
            repository
                .insert_raw_payload_descriptor(&descriptor)
                .await
                .is_ok()
        );
        let update = sqlx::query("UPDATE raw_payload_descriptors SET custody_locator = 'other' WHERE payload_id = 'payload:1'")
            .execute(&repository.pool).await;
        assert!(update.is_err());

        let mut invalid = descriptor;
        invalid.payload_id = "payload:2".into();
        invalid.payload_digest = [0; 32];
        assert!(
            repository
                .insert_raw_payload_descriptor(&invalid)
                .await
                .is_err()
        );
    }

    // ── Reorganization standing and reorg-aware views (TUP-NE-004) ───────────

    /// A reorganization on the same source as the observations above, so the
    /// `closure_orphanings_match_reorg_source` trigger is satisfied.
    async fn add_reorg(repository: &ObservationRepository, reorg_id: &str, source_id: &str) {
        let reorg = ReorgRecord {
            schema_version: 1,
            reorg_id: reorg_id.into(),
            source_id: source_id.into(),
            detected_at: 13,
            prior_tip: "block:100:a".into(),
            replacement_tip: "block:100:b".into(),
        };
        assert!(repository.append_reorg(&reorg).await.is_ok());
    }

    fn retraction(reorg_id: &str, observation_id: &str) -> ClosureObservationOrphaningRecord {
        ClosureObservationOrphaningRecord {
            schema_version: tuppira_shared::CLOSURE_ORPHANING_RECORD_VERSION,
            reorg_id: reorg_id.into(),
            observation_id: observation_id.into(),
            orphaned_at: 14,
            disposition: OrphanedClosureDisposition::Retracted {
                reasons: vec!["the closure did not reappear on the replacement history".into()],
            },
        }
    }

    fn index_tip(height: u64) -> ClosureIndexTipRecord {
        ClosureIndexTipRecord {
            schema_version: tuppira_shared::CLOSURE_INDEX_TIP_RECORD_VERSION,
            chain_id: "ethereum".into(),
            network_id: "sepolia".into(),
            indexed_tip_height: height,
            indexed_tip_block_id_hex: "cafe".into(),
            indexed_tip_observed_at: 1_760_009_000,
        }
    }

    /// The core of the ticket: a reorganization must place a statement *beside*
    /// the observation, never edit or delete it. The stored closure has to read
    /// back byte-identical while the view over it stops claiming settlement.
    #[tokio::test]
    async fn an_orphaning_is_recorded_beside_the_observation_and_never_deletes_it() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );
        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;
        assert!(
            repository
                .record_closure_orphaning(&retraction("reorg:closure:1", "obs:closure:1"))
                .await
                .is_ok()
        );

        // The source's original statement survives exactly as recorded.
        assert_eq!(
            repository
                .closure_observation("obs:closure:1", "tenant:acme")
                .await
                .ok(),
            Some(projection),
        );

        let Ok(view) = repository
            .closure_observation_view("obs:closure:1", "tenant:acme")
            .await
        else {
            panic!("an orphaned closure must still be readable");
        };
        assert!(view.reorg_standing.is_orphaned());
        assert!(view.reorg_standing.is_retracted());
        // The finality report was about a checkpoint on a replaced history.
        assert!(!view.established_states.contains(&ClosureObservationState::Final));
        assert!(view.established_states.contains(&ClosureObservationState::Unknown));
        assert!(view.established_states.contains(&ClosureObservationState::Revoked));
        // And what was observed stays observed.
        assert!(view.established_states.contains(&ClosureObservationState::Observed));
    }

    /// A supersession withdraws nothing: the source reported the closure again
    /// on the replacement history. Only the settlement goes, because this
    /// observation's checkpoint is still on the history that was replaced.
    #[tokio::test]
    async fn a_superseded_closure_is_not_reported_as_revoked() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let original = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&original, &projection, None, &cursor(12))
                .await
                .is_ok()
        );
        let replacement = closure_observation_record("obs:closure:2", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&replacement, &projection, None, &cursor(13))
                .await
                .is_ok()
        );
        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;
        assert!(
            repository
                .record_closure_orphaning(&ClosureObservationOrphaningRecord {
                    disposition: OrphanedClosureDisposition::Superseded {
                        superseding_observation_id: "obs:closure:2".into(),
                    },
                    ..retraction("reorg:closure:1", "obs:closure:1")
                })
                .await
                .is_ok()
        );

        let Ok(view) = repository
            .closure_observation_view("obs:closure:1", "tenant:acme")
            .await
        else {
            panic!("a superseded closure must still be readable");
        };
        assert!(view.reorg_standing.is_orphaned());
        assert!(!view.reorg_standing.is_retracted());
        assert!(!view.established_states.contains(&ClosureObservationState::Revoked));
        assert!(!view.established_states.contains(&ClosureObservationState::Final));
        let [orphaning] = view.reorg_standing.own_orphanings.as_slice() else {
            panic!("exactly one orphaning was recorded");
        };
        assert_eq!(
            orphaning.disposition,
            OrphanedClosureDisposition::Superseded {
                superseding_observation_id: "obs:closure:2".into(),
            },
        );
    }

    /// Doubt travels downward along the linkage the observations themselves
    /// report, even though the descendant was never orphaned itself.
    #[tokio::test]
    async fn a_descendant_of_an_orphaned_closure_stops_reporting_settlement() {
        let Ok(repository) = repository().await else {
            return;
        };
        let ancestor_projection = closure_projection();
        let ancestor =
            closure_observation_record("obs:closure:ancestor", "sanad:1", &ancestor_projection);
        assert!(
            repository
                .append_closure_observation(&ancestor, &ancestor_projection, None, &cursor(12))
                .await
                .is_ok()
        );

        // The descendant consumes the state the ancestor committed to as its
        // successor. That is the whole of the reported linkage.
        let mut descendant_projection = closure_projection();
        descendant_projection.consumed_state.transition_id_hex =
            ancestor_projection.closure_identity.successor_commitment_hex.clone();
        descendant_projection.closure_identity.successor_commitment_hex = "33".repeat(32);
        descendant_projection.closure_identity.closure_identity_hex = "0a1b2c3d".into();
        let descendant = closure_observation_record(
            "obs:closure:descendant",
            "sanad:2",
            &descendant_projection,
        );
        assert!(
            repository
                .append_closure_observation(
                    &descendant,
                    &descendant_projection,
                    None,
                    &cursor(13)
                )
                .await
                .is_ok()
        );

        // Before the reorganization the descendant stands on its own.
        let Ok(before) = repository
            .closure_observation_view("obs:closure:descendant", "tenant:acme")
            .await
        else {
            panic!("the descendant must be readable");
        };
        assert!(!before.reorg_standing.descends_from_orphaned());
        assert!(before.established_states.contains(&ClosureObservationState::Final));

        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;
        assert!(
            repository
                .record_closure_orphaning(&retraction(
                    "reorg:closure:1",
                    "obs:closure:ancestor"
                ))
                .await
                .is_ok()
        );

        let Ok(after) = repository
            .closure_observation_view("obs:closure:descendant", "tenant:acme")
            .await
        else {
            panic!("the descendant must still be readable");
        };
        // Never orphaned itself, but its ground was replaced.
        assert!(!after.reorg_standing.is_orphaned());
        assert!(after.reorg_standing.descends_from_orphaned());
        assert!(!after.established_states.contains(&ClosureObservationState::Final));
        assert!(after.established_states.contains(&ClosureObservationState::Unknown));
        // An ancestor's retraction is not this closure's revocation.
        assert!(!after.established_states.contains(&ClosureObservationState::Revoked));
        let [reading] = after.reorg_standing.orphaned_ancestors.as_slice() else {
            panic!("exactly one orphaned ancestor was reachable");
        };
        assert_eq!(reading.observation_id, "obs:closure:ancestor");
        assert_eq!(reading.depth, 1);
        assert_eq!(
            reading.successor_commitment_hex,
            ancestor_projection.closure_identity.successor_commitment_hex,
        );
        assert_eq!(
            after.reorg_standing.ancestry_coverage,
            ClosureAncestryCoverage::Complete,
        );
    }

    /// The subject account must not resurrect a settlement its observations no
    /// longer individually establish.
    #[tokio::test]
    async fn a_subject_view_does_not_resurrect_a_withdrawn_settlement() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );
        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;
        assert!(
            repository
                .record_closure_orphaning(&retraction("reorg:closure:1", "obs:closure:1"))
                .await
                .is_ok()
        );

        // The stored account still reports what was committed …
        let Ok(stored) = repository.subject_closure("sanad:1", "tenant:acme").await else {
            panic!("the stored account must remain readable");
        };
        assert!(stored.established_states().contains(&ClosureObservationState::Final));

        // … and the view over it reports what that still supports.
        let Ok(view) = repository.subject_closure_view("sanad:1", "tenant:acme").await else {
            panic!("the subject view must be readable");
        };
        let states = view.established_states();
        assert!(!states.contains(&ClosureObservationState::Final));
        assert!(states.contains(&ClosureObservationState::Unknown));
    }

    /// Acceptance criterion: every closure view carries the indexed tip and lag,
    /// measured at read time rather than copied from the collector's reading.
    #[tokio::test]
    async fn a_closure_view_reports_the_current_tip_and_not_the_collectors() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );
        // The collector saw tip 1_000 against a checkpoint at 900.
        assert_eq!(projection.index_freshness.indexed_tip_height, 1_000);

        assert_eq!(
            repository.advance_closure_index_tip(&index_tip(5_000)).await.ok(),
            Some(true)
        );
        let Ok(view) = repository
            .closure_observation_view("obs:closure:1", "tenant:acme")
            .await
        else {
            panic!("the closure view must be readable");
        };

        assert_eq!(view.read_index_freshness.indexed_tip_height, 5_000);
        assert_eq!(
            view.read_index_freshness.lag_blocks,
            ObservedOrMissing::Observed { value: 4_100 },
        );
        // The collector's own reading is untouched inside the recorded payload.
        assert_eq!(
            view.recorded.projection.index_freshness,
            projection.index_freshness,
        );
    }

    /// A lag that could shrink without the index catching up is not a lag.
    #[tokio::test]
    async fn the_indexed_tip_never_moves_backwards() {
        let Ok(repository) = repository().await else {
            return;
        };
        assert_eq!(
            repository.advance_closure_index_tip(&index_tip(5_000)).await.ok(),
            Some(true)
        );
        // Discarded rather than rejected: connectors read blocks out of order,
        // and a stale reading must not fail an otherwise valid append.
        assert_eq!(
            repository.advance_closure_index_tip(&index_tip(4_000)).await.ok(),
            Some(false)
        );
        assert_eq!(
            repository
                .closure_index_tip("ethereum", "sepolia")
                .await
                .map(|tip| tip.indexed_tip_height)
                .ok(),
            Some(5_000),
        );
    }

    /// The database is the second enforcement of the invariants the typed
    /// boundary checks, for every caller including a future one.
    #[tokio::test]
    async fn the_database_rejects_orphanings_no_source_could_have_reported() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );

        // A reorganization on another source would let one connector withdraw
        // evidence it never collected.
        add_source(&repository, "source:other", "run:other").await;
        add_reorg(&repository, "reorg:foreign", "source:other").await;
        assert!(
            repository
                .record_closure_orphaning(&retraction("reorg:foreign", "obs:closure:1"))
                .await
                .is_err()
        );

        // An orphaning before the observation describes an order of events that
        // did not happen.
        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;
        assert!(
            repository
                .record_closure_orphaning(&ClosureObservationOrphaningRecord {
                    orphaned_at: 1,
                    ..retraction("reorg:closure:1", "obs:closure:1")
                })
                .await
                .is_err()
        );

        // An observation carrying no closure statement has no closure to orphan.
        let plain = observation("obs:plain", 3, None);
        assert!(repository.append_observation(&plain, &cursor(13)).await.is_ok());
        assert!(
            repository
                .record_closure_orphaning(&retraction("reorg:closure:1", "obs:plain"))
                .await
                .is_err()
        );
    }

    /// A replacement is the same closure re-reported. One closing a different
    /// state would silently move the subject of the source's statement.
    #[tokio::test]
    async fn a_replacement_must_close_the_same_consumed_state() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let original = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&original, &projection, None, &cursor(12))
                .await
                .is_ok()
        );
        let mut other_state = closure_projection();
        other_state.consumed_state.output_index = 7;
        let unrelated = closure_observation_record("obs:closure:other", "sanad:1", &other_state);
        assert!(
            repository
                .append_closure_observation(&unrelated, &other_state, None, &cursor(13))
                .await
                .is_ok()
        );
        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;

        assert!(
            repository
                .record_closure_orphaning(&ClosureObservationOrphaningRecord {
                    disposition: OrphanedClosureDisposition::Superseded {
                        superseding_observation_id: "obs:closure:other".into(),
                    },
                    ..retraction("reorg:closure:1", "obs:closure:1")
                })
                .await
                .is_err()
        );
    }

    /// Reorganization history is corrected by recording the next one, never by
    /// rewriting the last.
    #[tokio::test]
    async fn recorded_orphanings_cannot_be_rewritten_or_removed() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );
        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;
        assert!(
            repository
                .record_closure_orphaning(&retraction("reorg:closure:1", "obs:closure:1"))
                .await
                .is_ok()
        );

        let update = sqlx::query(
            "UPDATE closure_observation_orphanings SET disposition = 'superseded' WHERE observation_id = 'obs:closure:1'",
        )
        .execute(&repository.pool)
        .await;
        assert!(update.is_err());
        let delete =
            sqlx::query("DELETE FROM closure_observation_orphanings WHERE observation_id = 'obs:closure:1'")
                .execute(&repository.pool)
                .await;
        assert!(delete.is_err());
        // A reason filed against a supersession would read as a withdrawal the
        // source never made.
        let reason = sqlx::query(
            "INSERT INTO closure_observation_orphaning_reasons (reorg_id, observation_id, ordinal, reason) \
             VALUES ('reorg:closure:1', 'obs:closure:1', 9, 'invented')",
        )
        .execute(&repository.pool)
        .await;
        assert!(reason.is_ok(), "this orphaning is a retraction, so reasons belong to it");
    }

    /// The ancestry walk is tenant-filtered, and that has a cost worth pinning.
    ///
    /// An ancestor the reading tenant cannot see is not walked, so a descendant
    /// whose ground was replaced in another tenant's data reports a *clean*
    /// standing. The standing therefore understates doubt at a tenant boundary.
    /// That is the deliberate choice: the alternative is disclosing the
    /// existence, identity, and reorganization history of another tenant's
    /// observations, which is a worse failure than an understated standing. The
    /// test exists so the trade-off cannot be changed silently.
    #[tokio::test]
    async fn the_ancestry_walk_stays_inside_the_tenant_boundary() {
        let Ok(repository) = repository().await else {
            return;
        };
        let ancestor_projection = closure_projection();
        let ancestor =
            closure_observation_record("obs:closure:ancestor", "sanad:1", &ancestor_projection);
        assert!(
            repository
                .append_closure_observation(&ancestor, &ancestor_projection, None, &cursor(12))
                .await
                .is_ok()
        );
        let mut descendant_projection = closure_projection();
        descendant_projection.consumed_state.transition_id_hex =
            ancestor_projection.closure_identity.successor_commitment_hex.clone();
        descendant_projection.closure_identity.successor_commitment_hex = "33".repeat(32);
        descendant_projection.closure_identity.closure_identity_hex = "0a1b2c3d".into();
        let mut descendant = closure_observation_record(
            "obs:closure:descendant",
            "sanad:2",
            &descendant_projection,
        );
        descendant.tenant_visibility = TenantVisibility::Tenant {
            tenant_id: "tenant:other".into(),
        };
        assert!(
            repository
                .append_closure_observation(
                    &descendant,
                    &descendant_projection,
                    None,
                    &cursor(13)
                )
                .await
                .is_ok()
        );
        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;
        assert!(
            repository
                .record_closure_orphaning(&retraction(
                    "reorg:closure:1",
                    "obs:closure:ancestor"
                ))
                .await
                .is_ok()
        );

        // `tenant:other` owns the descendant but cannot see the ancestor, so
        // the orphaning is not reachable and the standing reads clean.
        let Ok(reader) = repository
            .closure_observation_view("obs:closure:descendant", "tenant:other")
            .await
        else {
            panic!("the owning tenant must see its own observation");
        };
        assert!(!reader.reorg_standing.descends_from_orphaned());
        assert!(reader.reorg_standing.orphaned_ancestors.is_empty());

        // The ancestor's own tenant sees the orphaning, on the ancestor itself.
        let Ok(ancestor_view) = repository
            .closure_observation_view("obs:closure:ancestor", "tenant:acme")
            .await
        else {
            panic!("the ancestor's tenant must see its own observation");
        };
        assert!(ancestor_view.reorg_standing.is_orphaned());

        // And the descendant is not reachable from the other tenant at all.
        assert!(
            repository
                .closure_observation_view("obs:closure:descendant", "tenant:acme")
                .await
                .is_err()
        );
    }

    /// A closure on a chain with no recorded tip fails closed rather than
    /// falling back to the collector's reading, which would present a stale
    /// closure as a current one.
    #[tokio::test]
    async fn a_view_without_a_recorded_index_tip_fails_closed() {
        let Ok(repository) = repository().await else {
            return;
        };
        let projection = closure_projection();
        let record = closure_observation_record("obs:closure:1", "sanad:1", &projection);
        assert!(
            repository
                .append_closure_observation(&record, &projection, None, &cursor(12))
                .await
                .is_ok()
        );
        // The append records the tip, so the view is readable …
        assert!(
            repository
                .closure_observation_view("obs:closure:1", "tenant:acme")
                .await
                .is_ok()
        );
        // … and a chain with no tip at all is an explicit absence.
        assert!(matches!(
            repository.closure_index_tip("solana", "mainnet").await,
            Err(TuppiraError::NotFound { .. })
        ));
    }

    // ── Conflict and lineage queries (TUP-NE-005) ────────────────────────────

    /// Append a closure consuming `(consumed_transition, output_index)` and
    /// favouring `successor`, under `observation_id`.
    async fn append_link(
        repository: &ObservationRepository,
        observation_id: &str,
        consumed_transition: &str,
        output_index: u32,
        successor: &str,
        identity_hex: &str,
        observed_at: u64,
    ) -> SourceClosureObservationProjectionV1 {
        let mut projection = closure_projection();
        projection.consumed_state.transition_id_hex = consumed_transition.into();
        projection.consumed_state.output_index = output_index;
        projection.closure_identity.successor_commitment_hex = successor.into();
        projection.closure_identity.closure_identity_hex = identity_hex.into();
        let record = closure_observation_record(observation_id, "sanad:1", &projection);
        if let Err(error) = repository
            .append_closure_observation(&record, &projection, None, &cursor(observed_at))
            .await
        {
            panic!("append {observation_id}: {error:?}");
        }
        projection
    }

    fn root_of(projection: &SourceClosureObservationProjectionV1) -> ConsumedStateReading {
        projection.consumed_state.clone()
    }

    #[tokio::test]
    async fn a_lineage_walk_reaches_successors_and_the_outputs_observed_consumed() {
        let Ok(repository) = repository().await else {
            return;
        };
        let first = append_link(
            &repository,
            "obs:closure:a",
            &"11".repeat(32),
            0,
            &"22".repeat(32),
            "0a01",
            12,
        )
        .await;
        append_link(
            &repository,
            "obs:closure:b",
            &"22".repeat(32),
            0,
            &"33".repeat(32),
            "0a02",
            13,
        )
        .await;

        let Ok(lineage) = repository
            .closure_lineage(&root_of(&first), "tenant:acme")
            .await
        else {
            panic!("the lineage of an observed root state must be readable");
        };
        assert_eq!(lineage.coverage, ClosureLineageCoverage::Complete);
        assert_eq!(lineage.steps.len(), 2);
        assert_eq!(lineage.steps[0].observation_id, "obs:closure:a");
        assert_eq!(lineage.steps[0].depth, 1);
        assert_eq!(lineage.steps[1].observation_id, "obs:closure:b");
        assert_eq!(lineage.steps[1].depth, 2);
        // The "→ outputs" leg: the first step names the output of its successor
        // that a further closure was observed to consume, so the edge is
        // readable without re-joining on transition identifiers.
        assert_eq!(
            lineage.steps[0].observed_consumed_outputs,
            vec![lineage.steps[1].consumed_state.clone()]
        );
        // Nothing further was observed on the second step's successor. That is
        // an absence of observation, not evidence the output is unspent.
        assert!(lineage.steps[1].observed_consumed_outputs.is_empty());
    }

    #[tokio::test]
    async fn every_step_naming_a_successor_reports_the_consumption_observed_on_it() {
        let Ok(repository) = repository().await else {
            return;
        };
        // Two sources report the same closure: one consumed output, one
        // favoured successor, two observations. They are not competitors —
        // `assess_closure_conflicts` calls this the same closure seen again —
        // and a multi-source observation plane records both.
        let first = append_link(
            &repository,
            "obs:closure:a",
            &"11".repeat(32),
            0,
            &"22".repeat(32),
            "0a01",
            12,
        )
        .await;
        append_link(
            &repository,
            "obs:closure:b",
            &"11".repeat(32),
            0,
            &"22".repeat(32),
            "0a02",
            13,
        )
        .await;
        // One further closure consumes an output of that shared successor.
        append_link(
            &repository,
            "obs:closure:c",
            &"22".repeat(32),
            0,
            &"33".repeat(32),
            "0a03",
            14,
        )
        .await;

        let Ok(lineage) = repository
            .closure_lineage(&root_of(&first), "tenant:acme")
            .await
        else {
            panic!("the lineage of an observed root state must be readable");
        };
        let downstream = lineage
            .steps
            .iter()
            .find(|step| step.observation_id == "obs:closure:c")
            .map(|step| step.consumed_state.clone())
            .expect("the further closure is a step of the walk");

        // Both steps name the same successor, so the consumption observed on
        // that successor's output was observed on *both* of their successors.
        // An empty list here says no source reported a closure for any output
        // of this step's successor, which is false for either of them.
        for step in lineage
            .steps
            .iter()
            .filter(|step| step.successor_commitment_hex == "22".repeat(32))
        {
            assert_eq!(
                step.observed_consumed_outputs,
                vec![downstream.clone()],
                "step {} reports no observed consumption of its successor's outputs",
                step.observation_id
            );
        }
    }

    #[tokio::test]
    async fn a_root_state_with_no_observed_closure_is_an_empty_walk_not_a_claim() {
        let Ok(repository) = repository().await else {
            return;
        };
        let root = ConsumedStateReading {
            transition_id_hex: "ab".repeat(32),
            output_index: 3,
            state_type: 1,
        };
        let Ok(lineage) = repository.closure_lineage(&root, "tenant:acme").await else {
            panic!("an unobserved root state is a readable empty walk");
        };
        assert!(lineage.steps.is_empty());
        assert_eq!(lineage.root_state, root);
        // No variant of the answer says "unspent", and none may be added: this
        // plane cannot distinguish a state nobody closed from one whose closure
        // it has not indexed.
        let encoded = serde_json::to_value(&lineage).expect("serialize");
        let text = encoded.to_string().to_ascii_lowercase();
        assert!(!text.contains("unspent"));
        assert!(!text.contains("valid"));
    }

    #[tokio::test]
    async fn a_lineage_step_reports_what_its_standing_leaves_established() {
        let Ok(repository) = repository().await else {
            return;
        };
        let first = append_link(
            &repository,
            "obs:closure:a",
            &"11".repeat(32),
            0,
            &"22".repeat(32),
            "0a01",
            12,
        )
        .await;
        add_reorg(&repository, "reorg:closure:1", "source:piteka").await;
        assert!(
            repository
                .record_closure_orphaning(&retraction("reorg:closure:1", "obs:closure:a"))
                .await
                .is_ok()
        );

        let Ok(lineage) = repository
            .closure_lineage(&root_of(&first), "tenant:acme")
            .await
        else {
            panic!("an orphaned closure is still readable exactly as recorded");
        };
        let step = &lineage.steps[0];
        // A step on a replaced history must not report settlement here when a
        // direct read of the same observation would not.
        assert!(!step.established_states.contains(&ClosureObservationState::Final));
        assert!(step.established_states.contains(&ClosureObservationState::Unknown));
        assert!(step.established_states.contains(&ClosureObservationState::Revoked));
        assert!(step.reorg_standing.is_orphaned());
    }

    #[tokio::test]
    async fn the_lineage_walk_stays_inside_the_tenant_boundary() {
        let Ok(repository) = repository().await else {
            return;
        };
        let first = append_link(
            &repository,
            "obs:closure:a",
            &"11".repeat(32),
            0,
            &"22".repeat(32),
            "0a01",
            12,
        )
        .await;
        let mut hidden = closure_projection();
        hidden.consumed_state.transition_id_hex = "22".repeat(32);
        hidden.closure_identity.successor_commitment_hex = "33".repeat(32);
        hidden.closure_identity.closure_identity_hex = "0a02".into();
        let mut record = closure_observation_record("obs:closure:hidden", "sanad:2", &hidden);
        record.tenant_visibility = TenantVisibility::Tenant {
            tenant_id: "tenant:other".into(),
        };
        assert!(
            repository
                .append_closure_observation(&record, &hidden, None, &cursor(13))
                .await
                .is_ok()
        );

        let Ok(lineage) = repository
            .closure_lineage(&root_of(&first), "tenant:acme")
            .await
        else {
            panic!("the reading tenant sees its own root");
        };
        // The same trade-off the ancestry walk makes, in the other direction:
        // another tenant's closure is neither walked nor named, so the trail
        // stops short rather than disclosing that it exists.
        assert_eq!(lineage.steps.len(), 1);
        assert!(lineage.steps[0].observed_consumed_outputs.is_empty());
    }

    #[tokio::test]
    async fn a_conflict_page_reports_competitors_as_observations_and_not_a_verdict() {
        let Ok(repository) = repository().await else {
            return;
        };
        let first = append_link(
            &repository,
            "obs:closure:a",
            &"11".repeat(32),
            0,
            &"22".repeat(32),
            "0a01",
            12,
        )
        .await;
        append_link(
            &repository,
            "obs:closure:b",
            &"11".repeat(32),
            0,
            &"33".repeat(32),
            "0a02",
            13,
        )
        .await;

        let Ok(page) = repository
            .closure_conflicts(
                &root_of(&first),
                &first.closure_identity.successor_commitment_hex,
                None,
                "tenant:acme",
            )
            .await
        else {
            panic!("a contested consumed output must be enumerable");
        };
        assert_eq!(page.competitors.len(), 1);
        assert_eq!(page.competitors[0].successor_commitment_hex, "33".repeat(32));
        // The page reached the end of the recorded set, and says what bounded
        // it. Neither the type nor this answer picks a winner.
        let ClosureConflictPageCoverage::RecordedSetExhausted { searched_domains } = &page.coverage
        else {
            panic!("both closures were read, so the recorded set was exhausted");
        };
        assert_eq!(searched_domains.len(), 1);
        assert_eq!(searched_domains[0].chain_id, "ethereum");
        let text = serde_json::to_value(&page)
            .expect("serialize")
            .to_string()
            .to_ascii_lowercase();
        assert!(!text.contains("unique"));
        assert!(!text.contains("winner"));
        assert!(!text.contains("verdict"));
    }

    #[tokio::test]
    async fn a_full_conflict_page_says_more_remain_rather_than_reading_as_exhausted() {
        let Ok(repository) = repository().await else {
            return;
        };
        let root = &"11".repeat(32);
        // One more closure than a page holds, so the last one cannot fit.
        for index in 1..=(MAX_CLOSURE_CONFLICT_PAGE + 1) {
            append_link(
                &repository,
                &format!("obs:closure:{index:04}"),
                root,
                0,
                // Never all-zero: a zero commitment is not a commitment, and
                // the projection rejects one.
                &format!("{index:064x}"),
                &format!("{index:04x}"),
                12 + index as u64,
            )
            .await;
        }

        let searched = ConsumedStateReading {
            transition_id_hex: root.clone(),
            output_index: 0,
            state_type: 1,
        };
        let Ok(page) = repository
            .closure_conflicts(&searched, &"ff".repeat(32), None, "tenant:acme")
            .await
        else {
            panic!("a heavily contested output must page rather than fail");
        };
        assert_eq!(page.competitors.len(), MAX_CLOSURE_CONFLICT_PAGE);
        // A full page is not an exhausted set. Reporting it as one would turn
        // the unread remainder into an absence of competitors.
        let ClosureConflictPageCoverage::MorePagesRemain {
            resume_after_observation_id,
        } = &page.coverage
        else {
            panic!("a full page must say that closures were left unread");
        };

        let Ok(next) = repository
            .closure_conflicts(
                &searched,
                &"ff".repeat(32),
                Some(resume_after_observation_id),
                "tenant:acme",
            )
            .await
        else {
            panic!("the cursor must resume the search");
        };
        assert_eq!(next.competitors.len(), 1);
        assert!(matches!(
            next.coverage,
            ClosureConflictPageCoverage::RecordedSetExhausted { .. }
        ));
    }

    #[tokio::test]
    async fn a_conflict_page_does_not_disclose_another_tenants_closure() {
        let Ok(repository) = repository().await else {
            return;
        };
        let first = append_link(
            &repository,
            "obs:closure:a",
            &"11".repeat(32),
            0,
            &"22".repeat(32),
            "0a01",
            12,
        )
        .await;
        let mut hidden = closure_projection();
        hidden.closure_identity.successor_commitment_hex = "33".repeat(32);
        hidden.closure_identity.closure_identity_hex = "0a02".into();
        let mut record = closure_observation_record("obs:closure:hidden", "sanad:2", &hidden);
        record.tenant_visibility = TenantVisibility::Tenant {
            tenant_id: "tenant:other".into(),
        };
        assert!(
            repository
                .append_closure_observation(&record, &hidden, None, &cursor(13))
                .await
                .is_ok()
        );

        let Ok(page) = repository
            .closure_conflicts(
                &root_of(&first),
                &first.closure_identity.successor_commitment_hex,
                None,
                "tenant:acme",
            )
            .await
        else {
            panic!("the reading tenant sees its own closure");
        };
        assert!(page.competitors.is_empty());
        // And the reading tenant is not told that something was hidden, only
        // what the index it may read covers.
        assert!(matches!(
            page.coverage,
            ClosureConflictPageCoverage::RecordedSetExhausted { .. }
        ));
    }
}
