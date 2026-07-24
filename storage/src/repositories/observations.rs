//! Typed persistence for source-neutral observations and their lineage.

use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tuppira_shared::{
    CollectionRunRecord, ContradictionHintRecord, ObservationRecord, RawPayloadDescriptor,
    ReorgRecord, Result, RetentionClassRecord, RetractionStatus, SourceRecord, SyncCursorRecord,
    TenantVisibility, TuppiraError,
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

fn optional_u64(value: Option<i64>, field: &str) -> Result<Option<u64>> {
    value.map(|value| to_u64(value, field)).transpose()
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
    let retraction_status = match row.try_get::<String, _>("retraction_status")?.as_str() {
        "active" => RetractionStatus::Active,
        "retracted" => RetractionStatus::Retracted,
        _ => return Err(invalid("unsupported retraction status")),
    };
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
}
