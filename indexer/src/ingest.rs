//! One-shot ingestion of Piteka's signed evidence-export feed.
//!
//! Post-demo Master Plan increment (§23): Tuppira's first accountability
//! connector *consumes Piteka's exported evidence feed* rather than registering
//! a competing GitHub webhook. This drives [`PitekaEvidenceFeedConnector`]
//! through the source-neutral contract — discover, authenticate, normalize,
//! persist, checkpoint — writing append-only observations Hemion can explore.
//!
//! It is deliberately a one-shot pass over the immutable feed: it reads until
//! the feed is exhausted, then exits, so an operator can drive the demo
//! step-by-step and re-run it idempotently.

use std::sync::Arc;

use sqlx::SqlitePool;
use tuppira_shared::{
    CollectionRunRecord, RetentionClassRecord, SourceRecord, SyncCursorRecord, TuppiraError,
};
use tuppira_storage::repositories::observations::ObservationRepository;

use crate::connector::{SourceConnector, authenticate_and_normalize};
use crate::piteka_feed::{HttpPitekaFeedTransport, PitekaEvidenceFeedConnector, PitekaFeedConfig};

const DISCOVERY_LIMIT: u32 = 64;
const CONNECTOR_KIND: &str = "piteka-evidence-export";
const DISPLAY_NAME: &str = "Piteka evidence feed";

/// Operator-supplied configuration for one ingestion pass.
#[derive(Debug, Clone)]
pub struct IngestConfig {
    pub endpoint: String,
    pub bearer_token: String,
    pub tenant_id: String,
    pub source_id: String,
    pub signing_key_id: String,
    pub verifying_key_hex: String,
    pub retention_class_id: String,
    pub collection_run_id: String,
}

/// What a pass did, for operator-visible reporting.
#[derive(Debug, Default, Clone)]
pub struct IngestSummary {
    pub authenticated: u64,
    pub persisted: u64,
    pub duplicates: u64,
    /// Exports the connector rejected (e.g. incomplete evidence). Skipping keeps
    /// the live feed flowing; one malformed export never halts the pipeline.
    pub skipped: u64,
    pub observation_ids: Vec<String>,
}

fn parse_verifying_key(hex_key: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_key.trim())
        .map_err(|_| "PITEKA_FEED_VERIFYING_KEY must be 64 hex characters".to_string())?;
    bytes
        .try_into()
        .map_err(|_| "PITEKA_FEED_VERIFYING_KEY must decode to exactly 32 bytes".to_string())
}

/// Registers the observation source and collection run, then drains the feed.
///
/// Prerequisites are inserted idempotently so re-running against the same store
/// does not fail; already-present observations are counted as duplicates rather
/// than treated as errors, honoring the connector's idempotency contract.
pub async fn ingest_piteka(pool: SqlitePool, config: IngestConfig) -> Result<IngestSummary, String> {
    let verifying_key = parse_verifying_key(&config.verifying_key_hex)?;

    let transport = HttpPitekaFeedTransport::new(config.endpoint.clone(), config.bearer_token.clone())
        .map_err(|error| format!("invalid feed transport: {error:?}"))?;
    let connector = PitekaEvidenceFeedConnector::new(
        PitekaFeedConfig {
            source_id: config.source_id.clone(),
            tenant_id: config.tenant_id.clone(),
            signing_key_id: config.signing_key_id.clone(),
            verifying_key,
            retention_class_id: config.retention_class_id.clone(),
            collection_run_id: config.collection_run_id.clone(),
        },
        Arc::new(transport),
    )
    .map_err(|error| format!("invalid connector config: {error:?}"))?;

    let repository = ObservationRepository::new(pool);

    // Prerequisites (retention class -> source -> collection run), idempotent.
    ignore_conflict(
        repository
            .insert_retention_class(&RetentionClassRecord {
                schema_version: 1,
                retention_class_id: config.retention_class_id.clone(),
                purpose: "Tenant-scoped Piteka evidence export".to_string(),
                retain_for_seconds: None,
                raw_payload_permitted: true,
            })
            .await,
    )?;
    ignore_conflict(
        repository
            .insert_source(&SourceRecord {
                schema_version: 1,
                source_id: config.source_id.clone(),
                connector_kind: CONNECTOR_KIND.to_string(),
                display_name: DISPLAY_NAME.to_string(),
                retention_class_id: config.retention_class_id.clone(),
            })
            .await,
    )?;

    let started_at = now_seconds();
    ignore_conflict(
        repository
            .insert_collection_run(&CollectionRunRecord {
                schema_version: 1,
                collection_run_id: config.collection_run_id.clone(),
                source_id: config.source_id.clone(),
                started_at,
                completed_at: None,
            })
            .await,
    )?;

    let mut summary = IngestSummary::default();
    let mut cursor = None;
    loop {
        let batch = connector
            .discover(cursor.as_ref(), DISCOVERY_LIMIT)
            .await
            .map_err(|error| format!("discover failed: {error:?}"))?;
        if batch.events.is_empty() {
            break;
        }

        for raw in &batch.events {
            // A single export that fails authentication or normalization (for
            // example a receipt with no disclosed intent binding) is skipped,
            // not fatal: the connector enforces evidence completeness, and the
            // live feed must keep flowing past incomplete records.
            let candidate = match authenticate_and_normalize(&connector, raw, 1).await {
                Ok(candidate) => candidate,
                Err(error) => {
                    eprintln!("ingest: skipping export: authenticate/normalize failed: {error:?}");
                    summary.skipped += 1;
                    continue;
                }
            };
            summary.authenticated += 1;

            if let Some(descriptor) = &candidate.raw_payload {
                ignore_conflict(repository.insert_raw_payload_descriptor(descriptor).await)?;
            }

            let observation_id = candidate.observation.observation_id.clone();
            let sync_cursor = SyncCursorRecord {
                schema_version: 1,
                source_id: config.source_id.clone(),
                cursor_version: 1,
                cursor: candidate.observation.observed_at.to_be_bytes().to_vec(),
                observed_at: candidate.observation.observed_at,
            };

            match repository
                .append_observation(&candidate.observation, &sync_cursor)
                .await
            {
                Ok(()) => {
                    summary.persisted += 1;
                    summary.observation_ids.push(observation_id);
                }
                Err(error) => {
                    // Already ingested (idempotent re-run) is not a failure.
                    if repository.get_observation(&observation_id).await.is_ok() {
                        summary.duplicates += 1;
                        summary.observation_ids.push(observation_id);
                    } else {
                        return Err(format!("persist failed: {error:?}"));
                    }
                }
            }
        }

        cursor = Some(
            connector
                .checkpoint(&batch)
                .map_err(|error| format!("checkpoint failed: {error:?}"))?,
        );
    }

    Ok(summary)
}

/// Treats a unique-constraint conflict as success (prerequisite already present).
fn ignore_conflict(result: Result<(), TuppiraError>) -> Result<(), String> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = format!("{error:?}");
            if message.contains("UNIQUE") || message.contains("constraint") {
                Ok(())
            } else {
                Err(message)
            }
        }
    }
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
