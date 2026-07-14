//! In-memory distribution for the untrusted wallet discovery feed.
//!
//! Persistence and chain verification deliberately remain outside this API
//! component.  The hub validates ordering before a message is published.
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};

use csv_explorer_shared::{WalletFeedEnvelope, WalletFeedProjection};

#[derive(Clone)]
pub struct WalletFeedHub {
    projection: Arc<RwLock<WalletFeedProjection>>,
    publisher: broadcast::Sender<WalletFeedEnvelope>,
}

impl WalletFeedHub {
    pub fn new() -> Self {
        let (publisher, _) = broadcast::channel(256);
        Self {
            projection: Arc::new(RwLock::new(WalletFeedProjection::default())),
            publisher,
        }
    }

    /// Publish a validated indexer observation. Duplicate reconnect delivery is
    /// intentionally ignored; no runtime or wallet state is mutated here.
    pub async fn publish(&self, envelope: WalletFeedEnvelope) -> Result<(), String> {
        let mut projection = self.projection.write().await;
        if projection.apply(envelope.clone())? {
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
    use csv_explorer_shared::{
        ChainId, EXPLORER_EVENT_SCHEMA_VERSION, ExplorerEventDto, ExplorerEventPayload,
        ExplorerEventType, ExplorerFinality, FeedProvenance, IndexerFreshness,
        IndexerFreshnessStatus, Network, ObservedBlock, PROTOCOL_VERSION,
        WALLET_FEED_SCHEMA_VERSION, WalletFeedEnvelope,
    };

    use super::WalletFeedHub;

    fn observation(sequence: u64) -> WalletFeedEnvelope {
        let event = ExplorerEventDto {
            schema_version: EXPLORER_EVENT_SCHEMA_VERSION,
            chain_id: ChainId::new("ethereum"),
            network: Network::Testnet,
            contract: "0xcontract".into(),
            event_type: ExplorerEventType::SealConsumed,
            block_height: 9,
            block_hash: "0xblock".into(),
            transaction_id: "0xtx".into(),
            log_index: 1,
            finality: ExplorerFinality::Observed,
            payload: ExplorerEventPayload::SealConsumed {
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
            finality: ExplorerFinality::Observed,
            provenance: FeedProvenance {
                producer: "csv-explorer-indexer".into(),
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
}
