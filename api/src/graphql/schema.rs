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

    /// The V2 closure account for one subject.
    ///
    /// A subject with no recorded closure observation reports generation
    /// `pre_closure` and the single state `unknown`. That is the honest answer
    /// for every Sanad, transfer, and seal indexed before the closure profile,
    /// and it is never synthesised from the V1 explorer status.
    async fn subject_closure(
        &self,
        ctx: &Context<'_>,
        subject_ref: String,
    ) -> Result<SubjectClosureProjectionV1Gql> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|_| Error::new("API context unavailable"))?;
        let access = ctx
            .data::<crate::access::ObservationAccess>()
            .map_err(|_| Error::new("observation authentication required"))?;
        ObservationRepository::new(gql_ctx.pool.clone())
            .subject_closure_view(&subject_ref, &access.tenant_id)
            .await
            .map(Into::into)
            .map_err(|error| Error::new(error.to_string()))
    }

    /// The closure projection carried by one observation.
    ///
    /// An observation without one is an error rather than an empty closure: a
    /// caller must not receive a value shaped like a closure statement when
    /// none was recorded.
    async fn closure_observation(
        &self,
        ctx: &Context<'_>,
        observation_id: String,
    ) -> Result<ClosureObservationProjectionV1> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|_| Error::new("API context unavailable"))?;
        let access = ctx
            .data::<crate::access::ObservationAccess>()
            .map_err(|_| Error::new("observation authentication required"))?;
        let repository = ObservationRepository::new(gql_ctx.pool.clone());
        // The reorg-aware view, not the bare projection: an observation whose
        // history a reorganization replaced must not report `final` here
        // (TUP-NE-004).
        let observation_view = repository
            .closure_observation_view(&observation_id, &access.tenant_id)
            .await
            .map_err(|error| Error::new(error.to_string()))?;
        // The way back to the chain event, when the closure came from one.
        let chain_evidence = match repository
            .closure_evidence(&observation_id, &access.tenant_id)
            .await
        {
            Ok(evidence) => Some(evidence.into()),
            Err(tuppira_shared::TuppiraError::NotFound { .. }) => None,
            Err(error) => return Err(Error::new(error.to_string())),
        };
        let mut view: ClosureObservationProjectionV1 = observation_view.into();
        view.chain_evidence = chain_evidence;
        Ok(view)
    }

    /// The closures competing for one consumed output, one page at a time.
    ///
    /// Competitors are observations. Whether any of them is closure-valid is
    /// Parwana's verifier's answer, and this query neither computes nor implies
    /// one. Read `coverage` before reading an empty competitor list: only
    /// `recorded_set_exhausted` is about the recorded set, and even that is
    /// bounded by the index freshness it carries rather than being uniqueness.
    async fn closure_conflicts(
        &self,
        ctx: &Context<'_>,
        consumed_transition_id_hex: String,
        consumed_output_index: u32,
        consumed_state_type: u16,
        successor_commitment_hex: String,
        resume_after_observation_id: Option<String>,
    ) -> Result<ClosureConflictPageGql> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|_| Error::new("API context unavailable"))?;
        let access = ctx
            .data::<crate::access::ObservationAccess>()
            .map_err(|_| Error::new("observation authentication required"))?;
        ObservationRepository::new(gql_ctx.pool.clone())
            .closure_conflicts(
                &tuppira_shared::ConsumedStateReading {
                    transition_id_hex: consumed_transition_id_hex,
                    output_index: consumed_output_index,
                    state_type: consumed_state_type,
                },
                &successor_commitment_hex,
                resume_after_observation_id.as_deref(),
                &access.tenant_id,
            )
            .await
            .map(Into::into)
            .map_err(|error| Error::new(error.to_string()))
    }

    /// The closures observed downstream of one source state.
    ///
    /// Walks source state → attempted successors → closure observations →
    /// outputs. An empty walk means no closure was observed on the root state;
    /// it is never a statement that the state is unspent.
    async fn closure_lineage(
        &self,
        ctx: &Context<'_>,
        transition_id_hex: String,
        output_index: u32,
        state_type: u16,
    ) -> Result<ClosureLineageGql> {
        let gql_ctx = ctx
            .data::<GraphqlContext>()
            .map_err(|_| Error::new("API context unavailable"))?;
        let access = ctx
            .data::<crate::access::ObservationAccess>()
            .map_err(|_| Error::new("observation authentication required"))?;
        ObservationRepository::new(gql_ctx.pool.clone())
            .closure_lineage(
                &tuppira_shared::ConsumedStateReading {
                    transition_id_hex,
                    output_index,
                    state_type,
                },
                &access.tenant_id,
            )
            .await
            .map(Into::into)
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

    // ── V2 closure reads are added beside V1, not folded into it (TUP-NE-002) ─

    /// The pre-existing observation read model must be untouched. A closure
    /// field appearing on `ObservationGql` would give every consumer of that
    /// type a closure answer it never asked for — and for pre-migration records
    /// that answer could only be fabricated.
    #[tokio::test]
    async fn the_v1_observation_read_model_gains_no_closure_field() {
        let schema = Schema::build(Query, Mutation, Subscription).finish();
        let sdl = schema.sdl();
        let Some(start) = sdl.find("type ObservationGql {") else {
            panic!("the V1 observation read model must still exist");
        };
        let body = &sdl[start..];
        let Some(end) = body.find("\n}") else {
            panic!("malformed SDL for ObservationGql");
        };
        let body = &body[..end];

        assert!(!body.to_ascii_lowercase().contains("closure"));
        // The closure read models exist, separately and each naming its profile.
        assert!(sdl.contains("type ClosureObservationGql {"));
        assert!(sdl.contains("type SubjectClosureGql {"));
        assert!(sdl.contains("subjectClosure("));
    }

    /// Acceptance criterion: every closure view carries the indexed tip and lag.
    ///
    /// A consumer must be able to reach both without asking a second question,
    /// because a closure view separated from its lag reads as current.
    #[tokio::test]
    async fn every_closure_view_carries_its_reorg_standing_and_read_freshness() {
        let schema = Schema::build(Query, Mutation, Subscription).finish();
        let sdl = schema.sdl();
        let Some(start) = sdl.find("type ClosureObservationGql {") else {
            panic!("the closure read model must exist");
        };
        let body = &sdl[start..];
        let Some(end) = body.find("\n}") else {
            panic!("malformed SDL for ClosureObservationGql");
        };
        let body = &body[..end];

        assert!(body.contains("reorgStanding: ClosureReorgStandingGql!"));
        assert!(body.contains("readIndexFreshness: ClosureIndexFreshnessGql!"));
        // The view rules are versioned separately from the payload profile, so
        // a consumer can tell which rules produced `establishedStates`.
        assert!(body.contains("viewVersion: Int!"));
        assert!(body.contains("profileVersion: Int!"));

        // The tip and the lag are both reachable, and an unmeasurable lag is
        // nullable with its reasons rather than defaulted to zero.
        let Some(freshness_start) = sdl.find("type ClosureIndexFreshnessGql {") else {
            panic!("the read-time freshness type must exist");
        };
        let freshness = &sdl[freshness_start..];
        let freshness = &freshness[..freshness.find("\n}").unwrap_or(freshness.len())];
        assert!(freshness.contains("indexedTipHeight: Int!"));
        assert!(freshness.contains("lagBlocks: Int"));
        // Nullable on purpose: an unmeasurable lag is absent, never zero.
        assert!(!freshness.contains("lagBlocks: Int!"));
        assert!(freshness.contains("lagUnavailableReasons: [String!]!"));

        // A supersession and a retraction stay distinguishable on the wire.
        let Some(standing_start) = sdl.find("type ClosureReorgStandingGql {") else {
            panic!("the reorg standing type must exist");
        };
        let standing = &sdl[standing_start..];
        let standing = &standing[..standing.find("\n}").unwrap_or(standing.len())];
        assert!(standing.contains("isOrphaned: Boolean!"));
        assert!(standing.contains("descendsFromOrphaned: Boolean!"));
        assert!(standing.contains("isRetracted: Boolean!"));
        // A truncated ancestry walk is not reported as a clean one.
        assert!(standing.contains("ancestryCoverage: String!"));
        assert!(standing.contains("ancestryCoverageDepth: Int"));
        // Absent when the coverage is `complete`, so it is nullable.
        assert!(!standing.contains("ancestryCoverageDepth: Int!"));
    }

    /// A subject with no recorded closure observation must report `pre_closure`
    /// and `unknown` — not an empty V2 generation, and not silence.
    #[tokio::test]
    async fn a_subject_without_closure_evidence_reports_pre_closure_over_graphql() {
        let Ok(pool) = tuppira_storage::init_pool("sqlite::memory:", 1).await else {
            return;
        };
        let Ok(feed) = crate::feed::WalletFeedHub::from_pool(pool.clone()).await else {
            return;
        };
        let schema = create_schema(GraphqlContext { pool, feed }, true);
        let response = schema
            .execute(
                async_graphql::Request::new(
                    "{ subjectClosure(subjectRef: \"sanad:legacy\") \
                     { generation preClosureReasons establishedStates observations { observationId } } }",
                )
                .data(crate::access::ObservationAccess {
                    tenant_id: "tenant:acme".to_string(),
                }),
            )
            .await;

        assert!(response.errors.is_empty(), "{:?}", response.errors);
        let account = &response.data.into_json().unwrap_or_default()["subjectClosure"];
        assert_eq!(account["generation"], "pre_closure");
        assert_eq!(account["establishedStates"], serde_json::json!(["unknown"]));
        assert_eq!(account["observations"], serde_json::json!([]));
        assert!(
            account["preClosureReasons"]
                .as_array()
                .is_some_and(|reasons| !reasons.is_empty()),
            "absence must say why"
        );
    }

    #[tokio::test]
    async fn closure_queries_require_authentication() {
        let Ok(pool) = tuppira_storage::init_pool("sqlite::memory:", 1).await else {
            return;
        };
        let Ok(feed) = crate::feed::WalletFeedHub::from_pool(pool.clone()).await else {
            return;
        };
        let schema = create_schema(GraphqlContext { pool, feed }, true);
        for operation in [
            "{ subjectClosure(subjectRef: \"sanad:1\") { generation } }",
            "{ closureObservation(observationId: \"obs:1\") { observationId } }",
            "{ closureLineage(transitionIdHex: \"11\", outputIndex: 0, stateType: 1) \
             { coverage } }",
            "{ closureConflicts(consumedTransitionIdHex: \"11\", consumedOutputIndex: 0, \
             consumedStateType: 1, successorCommitmentHex: \"22\") { coverage } }",
        ] {
            let denied = schema.execute(operation).await;
            assert_eq!(denied.errors.len(), 1);
            assert!(denied.errors[0].message.contains("authentication required"));
        }
    }

    // ── Conflict and lineage queries (TUP-NE-005) ────────────────────────────

    /// The investigation reads must not acquire the vocabulary of a verdict.
    ///
    /// Tuppira observes; it never computes an authoritative conclusion
    /// (`development/ARCHITECTURE.md` §5.2). A *field* named for validity or a
    /// winner would make the read look like an answer only Parwana's verifier
    /// can give, so the field names are pinned here. Descriptions are excluded
    /// on purpose: the prose has to be able to say what a field does not mean,
    /// and "not evidence it is unspent" is exactly the sentence this rule
    /// exists to make true.
    #[tokio::test]
    async fn the_investigation_reads_carry_no_verdict_vocabulary() {
        let schema = Schema::build(Query, Mutation, Subscription).finish();
        let sdl = schema.sdl();
        for type_name in [
            "type ClosureLineageGql {",
            "type ClosureLineageStepGql {",
            "type ClosureConflictPageGql {",
            "type CompetingClosureGql {",
        ] {
            let Some(start) = sdl.find(type_name) else {
                panic!("{type_name} must exist");
            };
            let body = &sdl[start..];
            let body = &body[..body.find("\n}").unwrap_or(body.len())];
            let mut in_description = false;
            let mut names: Vec<&str> = Vec::new();
            for line in body.lines().skip(1).map(str::trim) {
                // `"""` opens and closes a description block; everything inside
                // it is prose and is not a field name.
                if line.starts_with("\"\"\"") {
                    // A one-line `"""text"""` description opens and closes at
                    // once; a bare `"""` toggles the block.
                    if !(line.len() > 3 && line.ends_with("\"\"\"")) {
                        in_description = !in_description;
                    }
                    continue;
                }
                if in_description || line.is_empty() {
                    continue;
                }
                if let Some(name) = line.split(&['(', ':'][..]).next() {
                    names.push(name);
                }
            }
            let field_names = names.join(" ").to_ascii_lowercase();
            // A description-stripping bug that ate the fields too would make
            // every assertion below pass without checking anything.
            assert!(
                names.len() >= 3,
                "{type_name} parsed to too few fields to be checking anything: {field_names}"
            );
            for forbidden in ["valid", "verdict", "winner", "unique", "unspent", "proven"] {
                assert!(
                    !field_names.contains(forbidden),
                    "{type_name} must not name a field `{forbidden}`: {field_names}"
                );
            }
        }
    }

    /// Each answer must say how much it covered, beside what it found.
    ///
    /// An empty competitor list and an ended step list are the two readings a
    /// consumer is most likely to over-trust, and the coverage field is the only
    /// thing that separates "nothing is there" from "the search stopped".
    #[tokio::test]
    async fn every_investigation_read_reports_the_coverage_that_bounds_it() {
        let schema = Schema::build(Query, Mutation, Subscription).finish();
        let sdl = schema.sdl();

        let Some(start) = sdl.find("type ClosureConflictPageGql {") else {
            panic!("the conflict page must exist");
        };
        let page = &sdl[start..];
        let page = &page[..page.find("\n}").unwrap_or(page.len())];
        assert!(page.contains("coverage: String!"));
        assert!(page.contains("searchedDomains: [ClosureIndexDomainFreshnessGql!]!"));
        // Present only while pages remain, so it is nullable.
        assert!(page.contains("resumeAfterObservationId: String"));
        assert!(!page.contains("resumeAfterObservationId: String!"));

        let Some(start) = sdl.find("type ClosureLineageGql {") else {
            panic!("the lineage read must exist");
        };
        let lineage = &sdl[start..];
        let lineage = &lineage[..lineage.find("\n}").unwrap_or(lineage.len())];
        assert!(lineage.contains("coverage: String!"));
        assert!(lineage.contains("coverageDepth: Int"));
        assert!(!lineage.contains("coverageDepth: Int!"));

        // A step reports what its reorganization standing leaves established,
        // and the outputs whose consumption was actually observed.
        let Some(start) = sdl.find("type ClosureLineageStepGql {") else {
            panic!("the lineage step must exist");
        };
        let step = &sdl[start..];
        let step = &step[..step.find("\n}").unwrap_or(step.len())];
        assert!(step.contains("reorgStanding: ClosureReorgStandingGql!"));
        assert!(step.contains("establishedStates: [String!]!"));
        assert!(step.contains("observedConsumedOutputs: [ClosureConsumedOutputGql!]!"));
    }

    /// An unobserved root state answers with an empty walk, not with silence and
    /// not with a claim about the state.
    #[tokio::test]
    async fn an_unobserved_root_state_reads_as_an_empty_walk_over_graphql() {
        let Ok(pool) = tuppira_storage::init_pool("sqlite::memory:", 1).await else {
            return;
        };
        let Ok(feed) = crate::feed::WalletFeedHub::from_pool(pool.clone()).await else {
            return;
        };
        let schema = create_schema(GraphqlContext { pool, feed }, true);
        let response = schema
            .execute(
                async_graphql::Request::new(format!(
                    "{{ closureLineage(transitionIdHex: \"{}\", outputIndex: 0, stateType: 1) \
                     {{ coverage steps {{ observationId }} }} }}",
                    "ab".repeat(32)
                ))
                .data(crate::access::ObservationAccess {
                    tenant_id: "tenant:acme".to_string(),
                }),
            )
            .await;

        assert!(response.errors.is_empty(), "{:?}", response.errors);
        let lineage = &response.data.into_json().unwrap_or_default()["closureLineage"];
        assert_eq!(lineage["coverage"], "complete");
        assert_eq!(lineage["steps"], serde_json::json!([]));
    }
}
