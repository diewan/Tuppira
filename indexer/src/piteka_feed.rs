//! Authenticated pull connector for Piteka's immutable evidence-export feed.
//!
//! This is deliberately a pull connector. GitHub delivery remains Piteka's
//! responsibility; Tuppira never registers a second webhook for the slice.

use async_trait::async_trait;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tuppira_shared::{
    OBSERVATION_SCHEMA_VERSION, ObservationRecord, ProviderSignatureRecord, RawPayloadDescriptor,
    RetractionStatus, TenantVisibility,
};

use crate::connector::{
    ConnectorCursor, ConnectorError, ConnectorResult, ObservationCandidate, RawSourceBatch,
    RawSourceEvent, ReconciliationReport, SourceAuthentication, SourceConnector, SourceHealth,
    validate_limit,
};

const FEED_SCHEMA_VERSION: u16 = 1;
const PROFILE_ID: &str = "org.diewan.piteka.evidence-export.v1";
const MEDIA_TYPE: &str = "application/vnd.diewan.piteka-evidence-export+json";
const SIGNATURE_DOMAIN: &[u8] = b"diewan.piteka.evidence-feed.v1\0";

/// One Piteka export and its detached feed signature. The signature covers all
/// fields used for identity, ordering, tenant isolation, and normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedPitekaExport {
    pub schema_version: u16,
    pub sequence: u64,
    pub export_id: String,
    pub revision: u32,
    pub supersedes_export_id: Option<String>,
    pub tenant_id: String,
    pub emitted_at: u64,
    pub payload: Vec<u8>,
    pub payload_sha256: [u8; 32],
    pub signing_key_id: String,
    pub signature: Vec<u8>,
}

impl SignedPitekaExport {
    fn signing_bytes(&self) -> ConnectorResult<Vec<u8>> {
        if self.schema_version != FEED_SCHEMA_VERSION {
            return Err(ConnectorError::UnsupportedContractVersion(
                self.schema_version,
            ));
        }
        if self.sequence == 0 || self.revision == 0 || self.emitted_at == 0 {
            return Err(ConnectorError::InvalidField("piteka_feed.ordering"));
        }
        validate_text(&self.export_id, "piteka_feed.export_id")?;
        validate_text(&self.tenant_id, "piteka_feed.tenant_id")?;
        validate_text(&self.signing_key_id, "piteka_feed.signing_key_id")?;
        if let Some(prior) = &self.supersedes_export_id {
            validate_text(prior, "piteka_feed.supersedes_export_id")?;
            if prior == &self.export_id {
                return Err(ConnectorError::InvalidField(
                    "piteka_feed.self_supersession",
                ));
            }
        }
        let digest: [u8; 32] = Sha256::digest(&self.payload).into();
        if self.payload.is_empty() || digest != self.payload_sha256 {
            return Err(ConnectorError::InvalidField("piteka_feed.payload_digest"));
        }

        let mut bytes = Vec::with_capacity(SIGNATURE_DOMAIN.len() + self.payload.len() + 256);
        bytes.extend_from_slice(SIGNATURE_DOMAIN);
        put_u16(&mut bytes, self.schema_version);
        put_u64(&mut bytes, self.sequence);
        put_text(&mut bytes, &self.export_id)?;
        put_u32(&mut bytes, self.revision);
        match &self.supersedes_export_id {
            Some(value) => {
                bytes.push(1);
                put_text(&mut bytes, value)?;
            }
            None => bytes.push(0),
        }
        put_text(&mut bytes, &self.tenant_id)?;
        put_u64(&mut bytes, self.emitted_at);
        bytes.extend_from_slice(&self.payload_sha256);
        put_u64(&mut bytes, self.payload.len() as u64);
        bytes.extend_from_slice(&self.payload);
        Ok(bytes)
    }
}

/// A bounded response from the immutable feed endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PitekaFeedPage {
    pub schema_version: u16,
    pub exports: Vec<SignedPitekaExport>,
    pub next_sequence: u64,
}

#[async_trait]
pub trait PitekaFeedTransport: Send + Sync {
    async fn fetch(&self, after_sequence: u64, limit: u32) -> ConnectorResult<PitekaFeedPage>;
    async fn health(&self) -> SourceHealth;
}

/// HTTP pull transport. Authentication credentials remain server-side and are
/// never placed in raw evidence or observations.
#[derive(Clone)]
pub struct HttpPitekaFeedTransport {
    client: reqwest::Client,
    endpoint: String,
    bearer_token: String,
}

impl HttpPitekaFeedTransport {
    pub fn new(endpoint: String, bearer_token: String) -> ConnectorResult<Self> {
        validate_text(&endpoint, "piteka_feed.endpoint")?;
        validate_text(&bearer_token, "piteka_feed.bearer_token")?;
        Ok(Self {
            client: reqwest::Client::new(),
            endpoint,
            bearer_token,
        })
    }
}

#[async_trait]
impl PitekaFeedTransport for HttpPitekaFeedTransport {
    async fn fetch(&self, after_sequence: u64, limit: u32) -> ConnectorResult<PitekaFeedPage> {
        validate_limit(limit)?;
        let response = self
            .client
            .get(&self.endpoint)
            .query(&[
                ("after_sequence", after_sequence),
                ("limit", u64::from(limit)),
            ])
            .header(ACCEPT, MEDIA_TYPE)
            .header(AUTHORIZATION, format!("Bearer {}", self.bearer_token))
            .send()
            .await
            .map_err(operation)?;
        if !response.status().is_success() {
            return Err(ConnectorError::Operation(format!(
                "Piteka feed returned HTTP {}",
                response.status()
            )));
        }
        response.json().await.map_err(operation)
    }

    async fn health(&self) -> SourceHealth {
        SourceHealth::Healthy
    }
}

#[derive(Debug, Clone)]
pub struct PitekaFeedConfig {
    pub source_id: String,
    pub tenant_id: String,
    pub signing_key_id: String,
    pub verifying_key: [u8; 32],
    pub retention_class_id: String,
    pub collection_run_id: String,
}

pub struct PitekaEvidenceFeedConnector {
    config: PitekaFeedConfig,
    verifying_key: VerifyingKey,
    transport: Arc<dyn PitekaFeedTransport>,
}

impl PitekaEvidenceFeedConnector {
    pub fn new(
        config: PitekaFeedConfig,
        transport: Arc<dyn PitekaFeedTransport>,
    ) -> ConnectorResult<Self> {
        validate_text(&config.source_id, "piteka_feed.source_id")?;
        validate_text(&config.tenant_id, "piteka_feed.tenant_id")?;
        validate_text(&config.signing_key_id, "piteka_feed.signing_key_id")?;
        validate_text(&config.retention_class_id, "piteka_feed.retention_class_id")?;
        validate_text(&config.collection_run_id, "piteka_feed.collection_run_id")?;
        let verifying_key = VerifyingKey::from_bytes(&config.verifying_key)
            .map_err(|_| ConnectorError::InvalidField("piteka_feed.verifying_key"))?;
        Ok(Self {
            config,
            verifying_key,
            transport,
        })
    }

    fn decode(raw: &RawSourceEvent) -> ConnectorResult<SignedPitekaExport> {
        serde_json::from_slice(&raw.bytes)
            .map_err(|_| ConnectorError::InvalidField("piteka_feed.envelope"))
    }
}

#[async_trait]
impl SourceConnector for PitekaEvidenceFeedConnector {
    fn source_id(&self) -> &str {
        &self.config.source_id
    }

    async fn discover(
        &self,
        cursor: Option<&ConnectorCursor>,
        limit: u32,
    ) -> ConnectorResult<RawSourceBatch> {
        validate_limit(limit)?;
        let after = decode_cursor(cursor, self.source_id())?;
        let page = self.transport.fetch(after, limit).await?;
        if page.schema_version != FEED_SCHEMA_VERSION || page.exports.len() > limit as usize {
            return Err(ConnectorError::UnsupportedContractVersion(
                page.schema_version,
            ));
        }
        let mut expected = after;
        let mut events = Vec::with_capacity(page.exports.len());
        for export in page.exports {
            if export.sequence != expected.saturating_add(1)
                || export.tenant_id != self.config.tenant_id
            {
                return Err(ConnectorError::InvalidField(
                    "piteka_feed.sequence_or_tenant",
                ));
            }
            expected = export.sequence;
            let source_event_id = event_id(&export);
            let observed_at = export.emitted_at;
            let bytes = serde_json::to_vec(&export).map_err(operation)?;
            events.push(RawSourceEvent {
                source_id: self.source_id().to_string(),
                source_event_id,
                media_type: MEDIA_TYPE.to_string(),
                bytes,
                observed_at,
                tenant_visibility: TenantVisibility::Tenant {
                    tenant_id: self.config.tenant_id.clone(),
                },
            });
        }
        if page.next_sequence != expected {
            return Err(ConnectorError::InvalidField("piteka_feed.next_sequence"));
        }
        let batch = RawSourceBatch {
            source_id: self.source_id().to_string(),
            events,
            proposed_cursor: encode_cursor(self.source_id(), expected),
        };
        batch.validate(limit)?;
        Ok(batch)
    }

    async fn authenticate(&self, raw: &RawSourceEvent) -> ConnectorResult<SourceAuthentication> {
        let export = Self::decode(raw)?;
        if export.tenant_id != self.config.tenant_id
            || export.signing_key_id != self.config.signing_key_id
            || raw.source_event_id != event_id(&export)
        {
            return Ok(SourceAuthentication::Rejected {
                reason: "feed identity or tenant mismatch".to_string(),
            });
        }
        let message = export.signing_bytes()?;
        let signature = Signature::from_slice(&export.signature)
            .map_err(|_| ConnectorError::InvalidField("piteka_feed.signature"))?;
        if self.verifying_key.verify(&message, &signature).is_err() {
            return Ok(SourceAuthentication::Rejected {
                reason: "invalid Piteka feed signature".to_string(),
            });
        }
        Ok(SourceAuthentication::Authenticated {
            material: vec![ProviderSignatureRecord {
                schema_version: OBSERVATION_SCHEMA_VERSION,
                authenticity_material_id: auth_id(&export),
                source_identity_id: format!("piteka-signing-key:{}", export.signing_key_id),
                signature_scheme: "ed25519-piteka-evidence-feed-v1".to_string(),
                signed_payload_digest: export.payload_sha256,
                signature: export.signature,
            }],
        })
    }

    fn normalize(
        &self,
        raw: &RawSourceEvent,
        profile_version: u16,
    ) -> ConnectorResult<ObservationCandidate> {
        if profile_version != 1 {
            return Err(ConnectorError::UnsupportedProfile {
                profile_id: PROFILE_ID.to_string(),
                version: profile_version,
            });
        }
        let export = Self::decode(raw)?;
        export.signing_bytes()?;
        let manifest: ExportManifest = serde_json::from_slice(&export.payload)
            .map_err(|_| ConnectorError::InvalidField("piteka_feed.payload"))?;
        manifest.validate()?;
        let observation_id = observation_id(&export);
        Ok(ObservationCandidate {
            observation: ObservationRecord {
                schema_version: OBSERVATION_SCHEMA_VERSION,
                observation_id,
                source_id: raw.source_id.clone(),
                source_event_id: raw.source_event_id.clone(),
                source_event_type: "piteka_evidence_export".to_string(),
                subject_refs: vec![
                    format!("mandate:{}", manifest.receipt.mandate_id),
                    format!("receipt:{}", manifest.receipt.receipt_id),
                ],
                asserted_event_time: Some(manifest.receipt.created_at),
                observed_at: raw.observed_at,
                normalized_profile_id: PROFILE_ID.to_string(),
                normalized_profile_version: 1,
                normalized_payload_digest: export.payload_sha256,
                raw_payload_digest: Some(Sha256::digest(&raw.bytes).into()),
                authenticity_material_refs: vec![auth_id(&export)],
                collection_run_id: self.config.collection_run_id.clone(),
                supersedes: export
                    .supersedes_export_id
                    .as_ref()
                    .map(|prior| prior_observation_id(prior)),
                retraction_status: RetractionStatus::Active,
                tenant_visibility: raw.tenant_visibility.clone(),
            },
            raw_payload: Some(RawPayloadDescriptor {
                schema_version: OBSERVATION_SCHEMA_VERSION,
                payload_id: format!("piteka-raw:{}", hex::encode(Sha256::digest(&raw.bytes))),
                digest_algorithm: "sha-256".to_string(),
                payload_digest: Sha256::digest(&raw.bytes).into(),
                media_type: raw.media_type.clone(),
                byte_length: raw.bytes.len() as u64,
                custody_locator: None,
                retention_class_id: self.config.retention_class_id.clone(),
            }),
        })
    }

    fn checkpoint(&self, batch: &RawSourceBatch) -> ConnectorResult<ConnectorCursor> {
        batch.validate(crate::connector::MAX_DISCOVERY_LIMIT)?;
        Ok(batch.proposed_cursor.clone())
    }

    async fn reconcile(
        &self,
        subject_ref: &str,
        interval: (u64, u64),
    ) -> ConnectorResult<ReconciliationReport> {
        validate_text(subject_ref, "piteka_feed.subject_ref")?;
        if interval.0 > interval.1 {
            return Err(ConnectorError::InvalidField("piteka_feed.interval"));
        }
        Ok(ReconciliationReport {
            source_id: self.source_id().to_string(),
            subject_ref: subject_ref.to_string(),
            reorgs: Vec::new(),
            supersessions: Vec::new(),
        })
    }

    async fn health(&self) -> SourceHealth {
        self.transport.health().await
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportManifest {
    bundle_version: String,
    receipt: ExportReceipt,
    dispatch_evidence: Vec<serde_json::Value>,
    target_evidence: Vec<serde_json::Value>,
    evidence_gaps: Vec<serde_json::Value>,
    source_attribution: serde_json::Value,
    missing_evidence: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportReceipt {
    receipt_id: String,
    mandate_id: String,
    intent_id: String,
    attempt_id: String,
    outcome: String,
    created_at: u64,
}
impl ExportManifest {
    fn validate(&self) -> ConnectorResult<()> {
        if self.bundle_version != "0.1" || self.receipt.created_at == 0 {
            return Err(ConnectorError::InvalidField("piteka_feed.manifest_version"));
        }
        for (v, f) in [
            (&self.receipt.receipt_id, "receipt_id"),
            (&self.receipt.mandate_id, "mandate_id"),
            (&self.receipt.intent_id, "intent_id"),
            (&self.receipt.attempt_id, "attempt_id"),
            (&self.receipt.outcome, "outcome"),
        ] {
            validate_text(v, f)?;
        }
        let _ = (
            &self.dispatch_evidence,
            &self.target_evidence,
            &self.evidence_gaps,
            &self.source_attribution,
            &self.missing_evidence,
        );
        Ok(())
    }
}

fn event_id(e: &SignedPitekaExport) -> String {
    format!("{}:revision:{}", e.export_id, e.revision)
}
fn observation_id(e: &SignedPitekaExport) -> String {
    format!("observation:piteka:{}:revision:{}", e.export_id, e.revision)
}
fn prior_observation_id(export_id: &str) -> String {
    format!("observation:piteka:{export_id}:revision:1")
}
fn auth_id(e: &SignedPitekaExport) -> String {
    format!(
        "piteka-feed-signature:{}:revision:{}",
        e.export_id, e.revision
    )
}
fn encode_cursor(source_id: &str, sequence: u64) -> ConnectorCursor {
    ConnectorCursor {
        source_id: source_id.to_string(),
        cursor_version: 1,
        bytes: sequence.to_be_bytes().to_vec(),
    }
}
fn decode_cursor(cursor: Option<&ConnectorCursor>, source_id: &str) -> ConnectorResult<u64> {
    let Some(cursor) = cursor else { return Ok(0) };
    cursor.validate()?;
    if cursor.source_id != source_id || cursor.cursor_version != 1 || cursor.bytes.len() != 8 {
        return Err(ConnectorError::InvalidField("piteka_feed.cursor"));
    }
    let bytes: [u8; 8] = cursor
        .bytes
        .as_slice()
        .try_into()
        .map_err(|_| ConnectorError::InvalidField("piteka_feed.cursor"))?;
    Ok(u64::from_be_bytes(bytes))
}
fn validate_text(value: &str, field: &'static str) -> ConnectorResult<()> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        Err(ConnectorError::InvalidField(field))
    } else {
        Ok(())
    }
}
fn put_text(out: &mut Vec<u8>, value: &str) -> ConnectorResult<()> {
    let len = u32::try_from(value.len())
        .map_err(|_| ConnectorError::BoundExceeded("piteka_feed.signing_field"))?;
    put_u32(out, len);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}
fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn operation(error: impl std::fmt::Display) -> ConnectorError {
    ConnectorError::Operation(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::authenticate_and_normalize;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::Mutex;

    struct FixtureTransport {
        exports: Mutex<Vec<SignedPitekaExport>>,
    }
    #[async_trait]
    impl PitekaFeedTransport for FixtureTransport {
        async fn fetch(&self, after: u64, limit: u32) -> ConnectorResult<PitekaFeedPage> {
            let exports = self
                .exports
                .lock()
                .map_err(operation)?
                .iter()
                .filter(|e| e.sequence > after)
                .take(limit as usize)
                .cloned()
                .collect::<Vec<_>>();
            let next_sequence = exports.last().map_or(after, |e| e.sequence);
            Ok(PitekaFeedPage {
                schema_version: 1,
                exports,
                next_sequence,
            })
        }
        async fn health(&self) -> SourceHealth {
            SourceHealth::Healthy
        }
    }
    fn payload(receipt: &str) -> Vec<u8> {
        format!(r#"{{"bundle_version":"0.1","receipt":{{"receipt_id":"{receipt}","mandate_id":"mandate-1","intent_id":"intent-1","attempt_id":"attempt-1","outcome":"succeeded","created_at":10}},"dispatch_evidence":[],"target_evidence":[],"evidence_gaps":[],"source_attribution":{{}},"missing_evidence":{{}}}}"#).into_bytes()
    }
    fn signed(
        key: &SigningKey,
        sequence: u64,
        export_id: &str,
        revision: u32,
        prior: Option<&str>,
    ) -> SignedPitekaExport {
        let payload = payload(&format!("receipt-{revision}"));
        let mut value = SignedPitekaExport {
            schema_version: 1,
            sequence,
            export_id: export_id.into(),
            revision,
            supersedes_export_id: prior.map(str::to_string),
            tenant_id: "tenant-1".into(),
            emitted_at: 20 + sequence,
            payload_sha256: Sha256::digest(&payload).into(),
            payload,
            signing_key_id: "key-1".into(),
            signature: Vec::new(),
        };
        value.signature = key
            .sign(&value.signing_bytes().expect("valid fixture"))
            .to_bytes()
            .to_vec();
        value
    }
    fn connector(
        exports: Vec<SignedPitekaExport>,
        key: &SigningKey,
    ) -> PitekaEvidenceFeedConnector {
        PitekaEvidenceFeedConnector::new(
            PitekaFeedConfig {
                source_id: "piteka:tenant-1".into(),
                tenant_id: "tenant-1".into(),
                signing_key_id: "key-1".into(),
                verifying_key: key.verifying_key().to_bytes(),
                retention_class_id: "tenant-evidence".into(),
                collection_run_id: "run-1".into(),
            },
            Arc::new(FixtureTransport {
                exports: Mutex::new(exports),
            }),
        )
        .expect("valid connector")
    }
    #[tokio::test]
    async fn pulls_authenticates_and_normalizes_without_a_webhook() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let connector = connector(vec![signed(&key, 1, "export-1", 1, None)], &key);
        let batch = connector.discover(None, 10).await.expect("discover");
        let first = authenticate_and_normalize(&connector, &batch.events[0], 1)
            .await
            .expect("normalize");
        let second = authenticate_and_normalize(&connector, &batch.events[0], 1)
            .await
            .expect("normalize again");
        assert_eq!(first, second);
        assert_eq!(
            first.observation.observation_id,
            "observation:piteka:export-1:revision:1"
        );
        assert_eq!(
            connector.checkpoint(&batch).expect("checkpoint").bytes,
            1_u64.to_be_bytes()
        );
    }
    #[tokio::test]
    async fn duplicate_delivery_is_stable_and_correction_is_append_only() {
        let key = SigningKey::from_bytes(&[8; 32]);
        let original = signed(&key, 1, "export-1", 1, None);
        let correction = signed(&key, 2, "export-2", 1, Some("export-1"));
        let connector = connector(vec![original.clone(), correction], &key);
        let a = connector.discover(None, 10).await.expect("first pull");
        let b = connector.discover(None, 10).await.expect("duplicate pull");
        assert_eq!(a.events, b.events);
        let corrected = authenticate_and_normalize(&connector, &a.events[1], 1)
            .await
            .expect("correction");
        assert_eq!(
            corrected.observation.supersedes.as_deref(),
            Some("observation:piteka:export-1:revision:1")
        );
        assert_eq!(
            connector
                .discover(Some(&connector.checkpoint(&a).expect("checkpoint")), 10)
                .await
                .expect("after cursor")
                .events
                .len(),
            0
        );
    }
    #[tokio::test]
    async fn rejects_tampering_cross_tenant_gaps_and_unknown_fields() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let mut tampered = signed(&key, 1, "export-1", 1, None);
        tampered.payload[0] ^= 1;
        let tampered_connector = connector(vec![tampered], &key);
        let batch = tampered_connector
            .discover(None, 10)
            .await
            .expect("discover envelope");
        assert!(
            authenticate_and_normalize(&tampered_connector, &batch.events[0], 1)
                .await
                .is_err()
        );
        let foreign = signed(&key, 1, "export-1", 1, None);
        let mut foreign = foreign;
        foreign.tenant_id = "tenant-2".into();
        let foreign_connector = connector(vec![foreign], &key);
        assert!(foreign_connector.discover(None, 10).await.is_err());
        let gap_connector = connector(vec![signed(&key, 2, "export-2", 1, None)], &key);
        assert!(gap_connector.discover(None, 10).await.is_err());
        let mut envelope =
            serde_json::to_value(signed(&key, 1, "export-1", 1, None)).expect("json");
        envelope
            .as_object_mut()
            .expect("object")
            .insert("authorized".into(), serde_json::json!(true));
        assert!(serde_json::from_value::<SignedPitekaExport>(envelope).is_err());
    }
}
