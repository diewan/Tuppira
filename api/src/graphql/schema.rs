/// GraphQL schema for the Tuppira API.
///
/// Provides queries for all indexed data types with filtering,
/// pagination, and aggregate statistics.
use async_graphql::*;
use futures::{Stream, stream};
use sqlx::SqlitePool;
use std::str::FromStr;

use tuppira_shared::{CommitmentScheme, InclusionProofType};
use tuppira_storage::repositories::{
    AdvancedProofRepository, ContractsRepository, ObservationRepository, SanadsRepository,
    SealsRepository, StatsRepository, TransfersRepository,
};

use super::types::*;
use crate::feed::WalletFeedHub;

/// GraphQL context holding the database pool.
pub struct GraphqlContext {
    pub pool: SqlitePool,
    pub feed: WalletFeedHub,
}

/// Root query type.
pub struct Query;

#[Object]
impl Query {
    async fn observation_lineage(
        &self,
        ctx: &Context<'_>,
        observation_id: String,
    ) -> Result<Vec<ObservationProjectionV1>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|_| Error::new("API context unavailable"))?;
        let access = ctx
            .data::<crate::access::ObservationAccess>()
            .map_err(|_| Error::new("observation authentication required"))?;
        ObservationRepository::new(gql_ctx.pool.clone())
            .visible_lineage(&observation_id, &access.tenant_id)
            .await
            .map(|records| records.into_iter().map(Into::into).collect())
            .map_err(|error| Error::new(error.to_string()))
    }

    async fn observation_source_health(
        &self,
        ctx: &Context<'_>,
    ) -> Result<Vec<SourceHealthProjectionV1>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|_| Error::new("API context unavailable"))?;
        let _access = ctx
            .data::<crate::access::ObservationAccess>()
            .map_err(|_| Error::new("observation authentication required"))?;
        ObservationRepository::new(gql_ctx.pool.clone())
            .source_health()
            .await
            .map(|records| records.into_iter().map(Into::into).collect())
            .map_err(|error| Error::new(error.to_string()))
    }
    /// Ordered, untrusted indexer observations for wallet discovery.  These
    /// reports never constitute cryptographic verification or mutation authority.
    async fn wallet_feed(
        &self,
        ctx: &Context<'_>,
        after_sequence: Option<i64>,
    ) -> Result<Vec<WalletFeedEnvelopeGql>> {
        let feed = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?
            .feed
            .clone();
        let sequence = after_sequence.unwrap_or(0);
        if sequence < 0 {
            return Err(ServerError::new("after_sequence must not be negative", None).into());
        }
        Ok(feed
            .since(sequence as u64)
            .await
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Get a single sanad by ID.
    async fn sanad(&self, ctx: &Context<'_>, id: String) -> Result<Option<Sanad>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = SanadsRepository::new(gql_ctx.pool.clone());
        let record = repo
            .get_by_id(&id)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(record.map(Sanad::from))
    }

    /// List sanads with optional filtering and pagination.
    async fn sanads(
        &self,
        ctx: &Context<'_>,
        filter: Option<SanadFilterInput>,
    ) -> Result<SanadConnection> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = SanadsRepository::new(gql_ctx.pool.clone());

        let filter = filter.unwrap_or_default();
        let limit = filter.limit.unwrap_or(20) as usize;
        let offset = filter.offset.unwrap_or(0) as usize;

        let shared_filter = tuppira_shared::SanadFilter {
            chain: filter.chain,
            owner: filter.owner,
            status: filter.status.as_deref().map(|s| match s {
                "active" => tuppira_shared::SanadStatus::Active,
                "spent" => tuppira_shared::SanadStatus::Spent,
                "pending" => tuppira_shared::SanadStatus::Pending,
                _ => tuppira_shared::SanadStatus::Active,
            }),
            limit: Some(limit),
            offset: Some(offset),
        };

        let records = repo
            .list(&shared_filter)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let total_count = repo
            .count()
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;

        let edges: Vec<SanadEdge> = records
            .into_iter()
            .enumerate()
            .map(|(i, r)| SanadEdge {
                node: Sanad::from(r),
                cursor: (offset + i).to_string(),
            })
            .collect();

        let has_next_page = (offset + limit) < total_count as usize;
        let has_previous_page = offset > 0;
        let start_cursor = edges.first().map(|e| e.cursor.clone());
        let end_cursor = edges.last().map(|e| e.cursor.clone());

        Ok(SanadConnection {
            edges,
            page_info: PageInfo::new(has_next_page, has_previous_page, start_cursor, end_cursor),
            total_count: total_count as i64,
        })
    }

    /// Get a single transfer by ID.
    async fn transfer(&self, ctx: &Context<'_>, id: String) -> Result<Option<Transfer>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = TransfersRepository::new(gql_ctx.pool.clone());
        let record = repo
            .get(&id)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(record.map(Transfer::from))
    }

    /// List transfers with optional filtering and pagination.
    async fn transfers(
        &self,
        ctx: &Context<'_>,
        filter: Option<TransferFilterInput>,
    ) -> Result<TransferConnection> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = TransfersRepository::new(gql_ctx.pool.clone());

        let filter = filter.unwrap_or_default();
        let limit = filter.limit.unwrap_or(20) as usize;
        let offset = filter.offset.unwrap_or(0) as usize;

        let shared_filter = tuppira_shared::TransferFilter {
            sanad_id: filter.sanad_id,
            from_chain: filter.from_chain,
            to_chain: filter.to_chain,
            status: filter.status.as_deref().map(|s| match s {
                "pending" => tuppira_shared::TransferStatus::Initiated,
                "in_progress" => tuppira_shared::TransferStatus::SubmittingProof,
                "completed" => tuppira_shared::TransferStatus::Completed,
                "failed" => tuppira_shared::TransferStatus::Failed {
                    error_code: "UNKNOWN".to_string(),
                    retryable: true,
                },
                _ => tuppira_shared::TransferStatus::Initiated,
            }),
            limit: Some(limit),
            offset: Some(offset),
        };

        let records = repo
            .list(shared_filter.clone())
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let total_count = repo
            .count(shared_filter)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;

        let edges: Vec<TransferEdge> = records
            .into_iter()
            .enumerate()
            .map(|(i, t)| TransferEdge {
                node: Transfer::from(t),
                cursor: (offset + i).to_string(),
            })
            .collect();

        let has_next_page = (offset + limit) < total_count as usize;
        let has_previous_page = offset > 0;
        let start_cursor = edges.first().map(|e| e.cursor.clone());
        let end_cursor = edges.last().map(|e| e.cursor.clone());

        Ok(TransferConnection {
            edges,
            page_info: PageInfo::new(has_next_page, has_previous_page, start_cursor, end_cursor),
            total_count: total_count as i64,
        })
    }

    /// Get a single seal by ID.
    async fn seal(&self, ctx: &Context<'_>, id: String) -> Result<Option<Seal>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = SealsRepository::new(gql_ctx.pool.clone());
        let record = repo
            .get(&id)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(record.map(Seal::from))
    }

    /// List seals with optional filtering and pagination.
    async fn seals(
        &self,
        ctx: &Context<'_>,
        filter: Option<SealFilterInput>,
    ) -> Result<SealConnection> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = SealsRepository::new(gql_ctx.pool.clone());

        let filter = filter.unwrap_or_default();
        let limit = filter.limit.unwrap_or(20) as usize;
        let offset = filter.offset.unwrap_or(0) as usize;

        let shared_filter = tuppira_shared::SealFilter {
            chain: filter.chain,
            seal_type: filter.seal_type.as_deref().map(|s| match s {
                "utxo" => tuppira_shared::SealType::Utxo,
                "object" => tuppira_shared::SealType::Object,
                "resource" => tuppira_shared::SealType::Resource,
                "nullifier" => tuppira_shared::SealType::Nullifier,
                "account" => tuppira_shared::SealType::Account,
                _ => tuppira_shared::SealType::Utxo,
            }),
            status: filter.status.as_deref().map(|s| match s {
                "available" => tuppira_shared::SealStatus::Available,
                "consumed" => tuppira_shared::SealStatus::Consumed,
                _ => tuppira_shared::SealStatus::Available,
            }),
            sanad_id: filter.sanad_id,
            limit: Some(limit),
            offset: Some(offset),
        };

        let records = repo
            .list(shared_filter.clone())
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let total_count = repo
            .count(shared_filter)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;

        let edges: Vec<SealEdge> = records
            .into_iter()
            .enumerate()
            .map(|(i, s)| SealEdge {
                node: Seal::from(s),
                cursor: (offset + i).to_string(),
            })
            .collect();

        let has_next_page = (offset + limit) < total_count as usize;
        let has_previous_page = offset > 0;
        let start_cursor = edges.first().map(|e| e.cursor.clone());
        let end_cursor = edges.last().map(|e| e.cursor.clone());

        Ok(SealConnection {
            edges,
            page_info: PageInfo::new(has_next_page, has_previous_page, start_cursor, end_cursor),
            total_count: total_count as i64,
        })
    }

    /// Get a seal by its chain-native identifier (e.g., Bitcoin txid:vout, Sui ObjectId, Ethereum storage slot).
    ///
    /// This enables cross-chain seal lookup — given a seal reference from any chain,
    /// you can find the corresponding seal record in the explorer.
    async fn seal_by_chain_native_id(
        &self,
        ctx: &Context<'_>,
        chain_native_id: String,
    ) -> Result<Option<Seal>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = SealsRepository::new(gql_ctx.pool.clone());
        let record = repo
            .get_by_chain_native_id(&chain_native_id)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(record.map(Seal::from))
    }

    /// Get a single contract by ID.
    async fn contract(&self, ctx: &Context<'_>, id: String) -> Result<Option<CsvContractGql>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = ContractsRepository::new(gql_ctx.pool.clone());
        let record = repo
            .get(&id)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(record.map(CsvContractGql::from))
    }

    /// List contracts with optional filtering and pagination.
    async fn contracts(
        &self,
        ctx: &Context<'_>,
        filter: Option<ContractFilterInput>,
    ) -> Result<ContractConnection> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = ContractsRepository::new(gql_ctx.pool.clone());

        let filter = filter.unwrap_or_default();
        let limit = filter.limit.unwrap_or(20) as usize;
        let offset = filter.offset.unwrap_or(0) as usize;

        let shared_filter = tuppira_shared::ContractFilter {
            chain: filter.chain,
            contract_type: filter.contract_type.as_deref().map(|s| match s {
                "nullifier_registry" => tuppira_shared::ContractType::NullifierRegistry,
                "state_commitment" => tuppira_shared::ContractType::StateCommitment,
                "sanad_registry" => tuppira_shared::ContractType::SanadRegistry,
                "bridge" => tuppira_shared::ContractType::Bridge,
                _ => tuppira_shared::ContractType::Other,
            }),
            status: filter.status.as_deref().map(|s| match s {
                "active" => tuppira_shared::ContractStatus::Active,
                "deprecated" => tuppira_shared::ContractStatus::Deprecated,
                "error" => tuppira_shared::ContractStatus::Error,
                _ => tuppira_shared::ContractStatus::Active,
            }),
            limit: Some(limit),
            offset: Some(offset),
        };

        let records = repo
            .list(shared_filter.clone())
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let total_count = repo
            .count(shared_filter)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;

        let edges: Vec<ContractEdge> = records
            .into_iter()
            .enumerate()
            .map(|(i, c)| ContractEdge {
                node: CsvContractGql::from(c),
                cursor: (offset + i).to_string(),
            })
            .collect();

        let has_next_page = (offset + limit) < total_count as usize;
        let has_previous_page = offset > 0;
        let start_cursor = edges.first().map(|e| e.cursor.clone());
        let end_cursor = edges.last().map(|e| e.cursor.clone());

        Ok(ContractConnection {
            edges,
            page_info: PageInfo::new(has_next_page, has_previous_page, start_cursor, end_cursor),
            total_count: total_count as i64,
        })
    }

    /// Get aggregate statistics.
    async fn stats(&self, ctx: &Context<'_>) -> Result<Stats> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = StatsRepository::new(gql_ctx.pool.clone());
        let stats = repo
            .get_stats()
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(Stats::from(stats))
    }

    /// Get the status of all indexed chains.
    async fn chain_status(&self, _ctx: &Context<'_>) -> Result<Vec<ChainInfoGql>> {
        // In a real implementation, this would query the indexer for current status
        // For now, return empty as the indexer would need to be wired in
        Ok(Vec::new())
    }

    /// Get sanads by owner address.
    async fn sanads_by_owner(&self, ctx: &Context<'_>, owner: String) -> Result<Vec<Sanad>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = SanadsRepository::new(gql_ctx.pool.clone());
        let filter = tuppira_shared::SanadFilter {
            chain: None,
            owner: Some(owner),
            status: None,
            limit: Some(100),
            offset: Some(0),
        };
        let records = repo
            .list(&filter)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(records.into_iter().map(Sanad::from).collect())
    }

    /// Get transfers for a specific sanad.
    async fn transfers_by_sanad(
        &self,
        ctx: &Context<'_>,
        sanad_id: String,
    ) -> Result<Vec<Transfer>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = TransfersRepository::new(gql_ctx.pool.clone());
        let records = repo
            .by_sanad(&sanad_id)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(records.into_iter().map(Transfer::from).collect())
    }

    /// Get seals for a specific sanad.
    async fn seals_by_sanad(&self, ctx: &Context<'_>, sanad_id: String) -> Result<Vec<Seal>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = SealsRepository::new(gql_ctx.pool.clone());
        let records = repo
            .by_sanad(&sanad_id)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(records.into_iter().map(Seal::from).collect())
    }

    // -----------------------------------------------------------------------
    // Advanced commitment and proof queries
    // -----------------------------------------------------------------------

    /// Get enhanced sanads with commitment scheme and proof metadata.
    async fn enhanced_sanads(
        &self,
        ctx: &Context<'_>,
        scheme: Option<String>,
        proof_type: Option<String>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<EnhancedSanad>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = AdvancedProofRepository::new(gql_ctx.pool.clone());

        let filter = tuppira_shared::SanadProofFilter {
            chain: None,
            owner: None,
            commitment_scheme: scheme
                .as_deref()
                .and_then(|s| CommitmentScheme::from_str(s).ok()),
            inclusion_proof_type: proof_type
                .as_deref()
                .and_then(|s| InclusionProofType::from_str(s).ok()),
            finality_proof_type: None,
            limit: limit.map(|v| v as usize),
            offset: offset.map(|v| v as usize),
        };

        let records = repo
            .query_enhanced_sanads(filter)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(records.into_iter().map(EnhancedSanad::from).collect())
    }

    /// Get enhanced seals with proof metadata.
    async fn enhanced_seals(
        &self,
        ctx: &Context<'_>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<EnhancedSeal>> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = AdvancedProofRepository::new(gql_ctx.pool.clone());

        let filter = tuppira_shared::SealProofFilter {
            chain: None,
            seal_type: None,
            seal_proof_type: None,
            seal_proof_verified: None,
            limit: limit.map(|v| v as usize),
            offset: offset.map(|v| v as usize),
        };

        let records = repo
            .query_enhanced_seals(filter)
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(records.into_iter().map(EnhancedSeal::from).collect())
    }

    /// Get proof statistics across all indexed data.
    async fn proof_statistics(&self, ctx: &Context<'_>) -> Result<ProofStatisticsGql> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        let repo = AdvancedProofRepository::new(gql_ctx.pool.clone());
        let stats = repo
            .get_proof_statistics()
            .await
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?;
        Ok(ProofStatisticsGql::from(stats))
    }
}

/// Root mutation type.
pub struct Mutation;

#[Object]
impl Mutation {
    /// This operation is unavailable until the API is connected to the
    /// authoritative indexer control plane.
    async fn refresh_chain(&self, _ctx: &Context<'_>, _chain: String) -> Result<bool> {
        Err(ServerError::new(
            "chain refresh is unavailable: the explorer does not expose an indexer control plane",
            None,
        )
        .into())
    }

    /// This operation is unavailable until the API is connected to the
    /// authoritative indexer control plane.
    async fn reindex_from(&self, _ctx: &Context<'_>, _chain: String, _block: i64) -> Result<bool> {
        Err(ServerError::new(
            "chain reindex is unavailable: the explorer does not expose an indexer control plane",
            None,
        )
        .into())
    }
}

/// Build the GraphQL schema.
pub struct Subscription;

#[Subscription]
impl Subscription {
    /// Delivers the same DTO used by REST and GraphQL queries.  Clients should
    /// reconnect using `after_sequence`; duplicate observations are idempotent.
    async fn wallet_feed(
        &self,
        ctx: &Context<'_>,
    ) -> Result<impl Stream<Item = WalletFeedEnvelopeGql>> {
        let receiver = ctx
            .data::<GraphqlContext>()
            .map_err(|e| ServerError::new(format!("{:?}", e), None))?
            .feed
            .subscribe();
        Ok(stream::unfold(receiver, |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok(envelope) => return Some((WalletFeedEnvelopeGql::from(envelope), receiver)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        }))
    }
}

pub fn create_schema(
    context: GraphqlContext,
    enable_introspection: bool,
) -> Schema<Query, Mutation, Subscription> {
    let builder = Schema::build(Query, Mutation, Subscription)
        .data(context)
        .limit_depth(12)
        .limit_complexity(1_000);
    if enable_introspection {
        builder.finish()
    } else {
        builder.disable_introspection().finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphqlContext, Mutation, Query, Subscription, create_schema};
    use async_graphql::Schema;

    #[tokio::test]
    async fn indexer_control_mutations_fail_closed() {
        let schema = Schema::build(Query, Mutation, Subscription).finish();

        for operation in [
            "mutation { refreshChain(chain: \"bitcoin\") }",
            "mutation { reindexFrom(chain: \"bitcoin\", block: 0) }",
        ] {
            let response = schema.execute(operation).await;
            assert_eq!(response.errors.len(), 1);
            assert!(
                response.errors[0]
                    .message
                    .contains("does not expose an indexer control plane")
            );
        }
    }

    #[tokio::test]
    async fn observation_queries_require_authentication_and_hide_raw_fields() {
        let pool = tuppira_storage::init_pool("sqlite::memory:", 1).await;
        assert!(pool.is_ok());
        let Ok(pool) = pool else {
            return;
        };
        let feed = crate::feed::WalletFeedHub::from_pool(pool.clone()).await;
        assert!(feed.is_ok());
        let Ok(feed) = feed else {
            return;
        };
        let schema = create_schema(GraphqlContext { pool, feed }, true);
        let denied = schema
            .execute("{ observationSourceHealth { sourceId } }")
            .await;
        assert_eq!(denied.errors.len(), 1);
        assert!(denied.errors[0].message.contains("authentication required"));
        let schema_text = schema.sdl();
        assert!(!schema_text.contains("rawPayload"));
        assert!(!schema_text.contains("custodyLocator"));
    }
}
