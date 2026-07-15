//! Durable distribution for the untrusted wallet discovery feed.
//!
//! Chain verification remains outside this API component. The hub validates
//! ordering and persists an observation before it is published.
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::{RwLock, broadcast};

use tuppira_shared::{WalletFeedEnvelope, WalletFeedProjection};

#[derive(Clone)]
pub struct WalletFeedHub {
    projection: Arc<RwLock<WalletFeedProjection>>,
    publisher: broadcast::Sender<WalletFeedEnvelope>,
    pool: Option<SqlitePool>,
}

impl WalletFeedHub {
    pub fn new() -> Self {
        let (publisher, _) = broadcast::channel(256);
        Self {
            projection: Arc::new(RwLock::new(WalletFeedProjection::default())),
            publisher,
            pool: None,
        }
    }

    /// Rebuild the delivery projection from persisted observations. Corrupt
    /// history is a startup failure, never an excuse to drop wallet evidence.
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, String> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT envelope_json FROM wallet_feed_events ORDER BY sequence ASC",
        )
        .fetch_all(&pool)
        .await
        .map_err(|error| format!("failed to load wallet feed: {error}"))?;
        let mut projection = WalletFeedProjection::default();
        for row in rows {
            let envelope: WalletFeedEnvelope = serde_json::from_str(&row)
                .map_err(|error| format!("stored wallet feed is invalid: {error}"))?;
            projection
                .apply(envelope)
                .map_err(|error| format!("stored wallet feed violates ordering: {error}"))?;
        }
        let (publisher, _) = broadcast::channel(256);
        Ok(Self {
            projection: Arc::new(RwLock::new(projection)),
            publisher,
            pool: Some(pool),
        })
    }

    /// Publish a validated indexer observation. Duplicate reconnect delivery is
    /// intentionally ignored; no runtime or wallet state is mutated here.
    pub async fn publish(&self, envelope: WalletFeedEnvelope) -> Result<(), String> {
        let mut projection = self.projection.write().await;
        let mut candidate = projection.clone();
        if candidate.apply(envelope.clone())? {
            if let Some(pool) = &self.pool {
                let serialized = serde_json::to_string(&envelope)
                    .map_err(|error| format!("failed to serialize wallet feed: {error}"))?;
                sqlx::query(
                    "INSERT INTO wallet_feed_events (sequence, observation_id, envelope_json) VALUES (?, ?, ?)",
                )
                .bind(envelope.sequence as i64)
                .bind(&envelope.observation_id)
                .bind(serialized)
                .execute(pool)
                .await
                .map_err(|error| format!("failed to persist wallet feed: {error}"))?;
            }
            *projection = candidate;
            let _ = self.publisher.send(envelope);
        }
        Ok(())
    }

    pub async fn since(&self, sequence: u64) -> Vec<WalletFeedEnvelope> {
        self.projection.read().await.since(sequence)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WalletFeedEnvelope> {
        self.publisher.subscribe()
    }
}

impl Default for WalletFeedHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tuppira_shared::{
        ChainId, TUPPIRA_EVENT_SCHEMA_VERSION, TuppiraEventDto, TuppiraEventPayload,
        TuppiraEventType, TuppiraFinality, FeedProvenance, IndexerFreshness,
        IndexerFreshnessStatus, Network, ObservedBlock, PROTOCOL_VERSION,
        WALLET_FEED_SCHEMA_VERSION, WalletFeedEnvelope,
    };

    use super::WalletFeedHub;

    fn observation(sequence: u64) -> WalletFeedEnvelope {
        let event = TuppiraEventDto {
            schema_version: TUPPIRA_EVENT_SCHEMA_VERSION,
            chain_id: ChainId::new("ethereum"),
            network: Network::Testnet,
            contract: "0xcontract".into(),
            event_type: TuppiraEventType::SealConsumed,
            block_height: 9,
            block_hash: "0xblock".into(),
            transaction_id: "0xtx".into(),
            log_index: 1,
            finality: TuppiraFinality::Observed,
            payload: TuppiraEventPayload::SealConsumed {
                sanad_id: "sanad".into(),
                nullifier: "nullifier".into(),
            },
        };
        WalletFeedEnvelope {
            schema_version: WALLET_FEED_SCHEMA_VERSION,
            protocol_version: PROTOCOL_VERSION.into(),
            observation_id: format!("observation-{sequence}"),
            sequence,
            chain_id: event.chain_id.clone(),
            network: event.network,
            observed_block: ObservedBlock {
                height: 9,
                hash: "0xblock".into(),
            },
            freshness: IndexerFreshness {
                indexed_at: Utc::now(),
                tip: ObservedBlock {
                    height: 9,
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

    #[tokio::test]
    async fn reconnect_query_and_subscription_do_not_duplicate_delivery() {
        let hub = WalletFeedHub::new();
        let mut subscription = hub.subscribe();
        let first = observation(1);

        assert!(hub.publish(first.clone()).await.is_ok());
        assert!(hub.publish(first).await.is_ok());

        let delivered = subscription.recv().await;
        assert!(delivered.is_ok());
        assert_eq!(delivered.map(|item| item.sequence), Ok(1));
        assert_eq!(hub.since(0).await.len(), 1);
        assert!(hub.since(1).await.is_empty());
    }

    #[tokio::test]
    async fn malformed_and_stale_observations_remain_untrusted() {
        let hub = WalletFeedHub::new();
        let mut malformed = observation(1);
        malformed.provenance.cryptographically_verified = true;
        assert!(hub.publish(malformed).await.is_err());

        let mut stale = observation(1);
        stale.freshness.status = IndexerFreshnessStatus::Stale;
        stale.freshness.lag_blocks = 4;
        assert!(hub.publish(stale).await.is_ok());
        assert_eq!(
            hub.since(0).await[0].freshness.status,
            IndexerFreshnessStatus::Stale
        );
    }

    #[tokio::test]
    async fn durable_feed_rebuilds_after_restart() {
        let pool = tuppira_storage::init_pool("sqlite::memory:", 1).await;
        assert!(pool.is_ok());
        let pool = match pool {
            Ok(pool) => pool,
            Err(_) => return,
        };
        let hub = WalletFeedHub::from_pool(pool.clone()).await;
        assert!(hub.is_ok());
        let hub = match hub {
            Ok(hub) => hub,
            Err(_) => return,
        };
        assert!(hub.publish(observation(1)).await.is_ok());
        let reloaded = WalletFeedHub::from_pool(pool).await;
        assert!(reloaded.is_ok());
        let reloaded = match reloaded {
            Ok(hub) => hub,
            Err(_) => return,
        };
        assert_eq!(reloaded.since(0).await.len(), 1);
    }
}
