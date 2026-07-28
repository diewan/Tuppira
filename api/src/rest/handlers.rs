/// REST API handlers for the Tuppira.
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};

use tuppira_storage::repositories::{
    EntityRepository, ObservationRepository, SanadsRepository, SealsRepository, StatsRepository,
    TransfersRepository,
};

use std::str::FromStr;
use tuppira_shared::{SanadFilter, SealFilter, TransferFilter, TuppiraError};

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

// The full application state tuple
type AppState = (
    async_graphql::Schema<
        crate::graphql::schema::Query,
        crate::graphql::schema::Mutation,
        crate::graphql::schema::Subscription,
    >,
    sqlx::SqlitePool,
    crate::feed::WalletFeedHub,
    // Per-chain configured network, used to build official block-explorer links.
    std::sync::Arc<std::collections::HashMap<String, tuppira_shared::Network>>,
);

// ---------------------------------------------------------------------------
// Response wrappers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub data: T,
    pub success: bool,
}

#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: u64,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub success: bool,
}

impl<T: Serialize> From<T> for ApiResponse<T> {
    fn from(data: T) -> Self {
        Self {
            data,
            success: true,
        }
    }
}

/// GET /api/v1/entities/:id. This is an observational profile, never an
/// authorization conclusion. Tenant scope is derived from authenticated access.
pub async fn entity_profile(
    Path(id): Path<String>,
    headers: HeaderMap,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    let access = crate::access::authenticate(&headers).map_err(auth_error)?;
    let profile = EntityRepository::new(pool)
        .profile(&access.tenant_id, &id)
        .await
        .map_err(tuppira_error)?;
    Ok(Json(ApiResponse::from(serde_json::json!({
        "entity_id": profile.entity_id,
        "entity_kind": profile.entity_kind,
        "display_name": profile.display_name,
        "profile_digest": profile.profile_digest_hex,
        "updated_at": profile.updated_at,
        "semantics": "observational_not_authorization"
    }))))
}

/// GET /api/v1/entities/:id/accountability. Missing or withheld relationships
/// remain explicit records and are never interpreted as authorization.
pub async fn entity_accountability(
    Path(id): Path<String>,
    headers: HeaderMap,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    let access = crate::access::authenticate(&headers).map_err(auth_error)?;
    let aggregate = EntityRepository::new(pool)
        .aggregate(&access.tenant_id, &id)
        .await
        .map_err(tuppira_error)?;
    Ok(Json(ApiResponse::from(serde_json::json!({
        "entity": {
            "entity_id": aggregate.profile.entity_id,
            "entity_kind": aggregate.profile.entity_kind,
            "display_name": aggregate.profile.display_name,
            "profile_digest": aggregate.profile.profile_digest_hex,
            "updated_at": aggregate.profile.updated_at
        },
        "references": aggregate.references.into_iter().map(|item| serde_json::json!({
            "kind": item.ref_kind, "disclosure_state": item.disclosure_state,
            "object_id": item.object_id, "source_observation_id": item.source_observation_id,
            "observed_at": item.observed_at
        })).collect::<Vec<_>>(),
        "relationships": aggregate.relationships.into_iter().map(|item| serde_json::json!({
            "kind": item.relationship_kind, "disclosure_state": item.disclosure_state,
            "related_entity_id": item.related_entity_id
        })).collect::<Vec<_>>(),
        "semantics": "observational_not_authorization"
    }))))
}

/// Reconnect cursor for the versioned, untrusted wallet discovery feed.
#[derive(Deserialize)]
pub struct WalletFeedQuery {
    pub after_sequence: Option<u64>,
}

/// GET /api/v1/wallet/feed
///
/// Explorer observations are suitable only for discovery and disposable UI
/// projections. They never authorize wallet or runtime mutation.
pub async fn wallet_feed(
    Query(query): Query<WalletFeedQuery>,
    State((_, _, feed, _)): State<AppState>,
) -> Json<ApiResponse<Vec<tuppira_shared::WalletFeedEnvelope>>> {
    Json(ApiResponse::from(
        feed.since(query.after_sequence.unwrap_or(0)).await,
    ))
}

/// GET /api/v1/observations/:id/lineage. Raw bytes, custody locators, and raw
/// payload digests are intentionally absent from this ordinary read model.
pub async fn observation_lineage(
    Path(id): Path<String>,
    headers: HeaderMap,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, (StatusCode, Json<ErrorResponse>)> {
    let access = crate::access::authenticate(&headers).map_err(auth_error)?;
    let records = ObservationRepository::new(pool)
        .visible_lineage(&id, &access.tenant_id)
        .await
        .map_err(tuppira_error)?;
    let data: Vec<serde_json::Value> = records.into_iter().map(|record| serde_json::json!({
        "observation_id": record.observation_id, "source_id": record.source_id,
        "source_event_id": record.source_event_id, "source_event_type": record.source_event_type,
        "subject_refs": record.subject_refs, "asserted_event_time": record.asserted_event_time,
        "observed_at": record.observed_at, "normalized_profile_id": record.normalized_profile_id,
        "normalized_profile_version": record.normalized_profile_version,
        "normalized_payload_digest": hex::encode(record.normalized_payload_digest),
        "authenticity_material_refs": record.authenticity_material_refs,
        "collection_run_id": record.collection_run_id, "supersedes": record.supersedes,
        "retraction_status": format!("{:?}", record.retraction_status).to_lowercase(),
        "visibility_scope": match record.tenant_visibility { tuppira_shared::TenantVisibility::Public => "public", tuppira_shared::TenantVisibility::Tenant { .. } => "tenant" }
    })).collect();
    Ok(Json(ApiResponse::from(data)))
}

/// GET /api/v1/observation-sources/health.
pub async fn observation_source_health(
    headers: HeaderMap,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, (StatusCode, Json<ErrorResponse>)> {
    let _access = crate::access::authenticate(&headers).map_err(auth_error)?;
    let records = ObservationRepository::new(pool)
        .source_health()
        .await
        .map_err(tuppira_error)?;
    Ok(Json(ApiResponse::from(records.into_iter().map(|record| serde_json::json!({
        "source_id": record.source_id, "connector_kind": record.connector_kind,
        "display_name": record.display_name, "last_run_started_at": record.last_run_started_at,
        "last_run_completed_at": record.last_run_completed_at,
        "cursor_observed_at": record.cursor_observed_at
    })).collect::<Vec<_>>())))
}

/// Query parameters for the observation feed.
#[derive(Deserialize)]
pub struct ListObservationsQuery {
    /// Maximum rows to return (default 50, clamped to [1, 500]).
    pub limit: Option<i64>,
}

/// GET /api/v1/observations. The live discovery feed: the most recent
/// tenant-visible observations, newest first. Hemion polls this to stream new
/// activity. Discovery is not verification — validity is recomputed locally.
pub async fn observation_list(
    headers: HeaderMap,
    Query(query): Query<ListObservationsQuery>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, (StatusCode, Json<ErrorResponse>)> {
    let access = crate::access::authenticate(&headers).map_err(auth_error)?;
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let records = ObservationRepository::new(pool)
        .list_visible_observations(&access.tenant_id, limit)
        .await
        .map_err(tuppira_error)?;
    let data: Vec<serde_json::Value> = records
        .into_iter()
        .map(|record| {
            serde_json::json!({
                "observation_id": record.observation_id, "source_id": record.source_id,
                "source_event_id": record.source_event_id, "source_event_type": record.source_event_type,
                "subject_refs": record.subject_refs, "asserted_event_time": record.asserted_event_time,
                "observed_at": record.observed_at, "normalized_profile_id": record.normalized_profile_id,
                "normalized_profile_version": record.normalized_profile_version,
                "normalized_payload_digest": hex::encode(record.normalized_payload_digest),
                "authenticity_material_refs": record.authenticity_material_refs,
                "collection_run_id": record.collection_run_id, "supersedes": record.supersedes,
                "retraction_status": format!("{:?}", record.retraction_status).to_lowercase(),
                "visibility_scope": match record.tenant_visibility { tuppira_shared::TenantVisibility::Public => "public", tuppira_shared::TenantVisibility::Tenant { .. } => "tenant" }
            })
        })
        .collect();
    Ok(Json(ApiResponse::from(data)))
}

// ---------------------------------------------------------------------------
// V2 source-closure reads (TUP-NE-002)
// ---------------------------------------------------------------------------

/// GET /api/v1/observations/:id/closure.
///
/// The closure statement one observation carries, in the profile's own
/// versioned wire shape. `profile_id` and `profile_version` name that shape, so
/// a consumer that does not understand it rejects the payload instead of
/// reading it under this release's rules.
pub async fn observation_closure(
    Path(id): Path<String>,
    headers: HeaderMap,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    let access = crate::access::authenticate(&headers).map_err(auth_error)?;
    let repository = ObservationRepository::new(pool);
    // The reorg-aware view, not the bare projection: an observation whose
    // history a reorganization replaced must not report `final` here, and a
    // read must say how far behind the index is *now* rather than only how far
    // behind it was when the collector produced the projection (TUP-NE-004).
    let view = repository
        .closure_observation_view(&id, &access.tenant_id)
        .await
        .map_err(tuppira_error)?;
    // The way back to the chain event, when the closure came from one. A
    // closure relayed by a non-chain source has none, and that reads as an
    // explicit absence rather than an empty evidence object.
    let chain_evidence = match repository.closure_evidence(&id, &access.tenant_id).await {
        Ok(evidence) => serde_json::json!(evidence),
        Err(tuppira_shared::TuppiraError::NotFound { .. }) => serde_json::json!({
            "availability": "missing",
            "reasons": ["no chain evidence is recorded for this closure observation"]
        }),
        Err(error) => return Err(tuppira_error(error)),
    };
    Ok(Json(ApiResponse::from(serde_json::json!({
        "observation_id": id,
        "observed_at": view.recorded.observed_at,
        // The collector withdrawing the record and the source withdrawing the
        // closure are different events; the second lives inside `payload`.
        "record_retraction_status":
            format!("{:?}", view.recorded.record_retraction_status).to_lowercase(),
        "profile_id": tuppira_shared::SOURCE_CLOSURE_OBSERVATION_PROFILE_ID,
        "profile_version": view.recorded.projection.schema_version,
        // Versions the three reorg-aware fields below. `profile_version` above
        // versions `payload` and did not change when they were added.
        "view_version": view.schema_version,
        // What the observation *still* establishes. Never `final` on a replaced
        // history; a consumer that wants the source's original statement reads
        // `payload`.
        "established_states": view.established_states,
        "reorg_standing": view.reorg_standing,
        // Index freshness now, distinct from `payload.index_freshness`, which
        // is what the collector measured when the projection was produced.
        "read_index_freshness": view.read_index_freshness,
        "payload": view.recorded.projection,
        "chain_evidence": chain_evidence
    }))))
}

/// Subject selector for the closure account.
#[derive(Deserialize)]
pub struct SubjectClosureQuery {
    /// The subject reference as observations carry it, such as `sanad:42`.
    pub subject_ref: String,
}

/// GET /api/v1/closure-observations?subject_ref=…
///
/// The closure account for one subject. A subject with no recorded closure
/// observation returns generation `pre_closure` with the reasons it is absent
/// and the single state `unknown` — never a closure derived from a V1 explorer
/// status such as a Sanad's stored `spent`.
pub async fn subject_closure(
    headers: HeaderMap,
    Query(query): Query<SubjectClosureQuery>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    let access = crate::access::authenticate(&headers).map_err(auth_error)?;
    let account = ObservationRepository::new(pool)
        .subject_closure_view(&query.subject_ref, &access.tenant_id)
        .await
        .map_err(tuppira_error)?;
    // The union of the per-observation sets *after* each has had its standing
    // applied. Unioning the stored sets instead would let a settlement
    // withdrawn from every observation individually reappear here.
    let established_states = account.established_states();
    Ok(Json(ApiResponse::from(serde_json::json!({
        "profile_id": tuppira_shared::SUBJECT_CLOSURE_VIEW_PROFILE_ID,
        "schema_version": account.schema_version,
        "subject_ref": account.subject_ref,
        "closure_generation": account.closure_generation,
        "established_states": established_states
    }))))
}

fn auth_error(status: StatusCode) -> (StatusCode, Json<ErrorResponse>) {
    let message = if status == StatusCode::SERVICE_UNAVAILABLE {
        "observation authentication is not configured"
    } else {
        "observation authentication failed"
    };
    (
        status,
        Json(ErrorResponse {
            error: message.into(),
            success: false,
        }),
    )
}

// ---------------------------------------------------------------------------
// Sanads handlers
// ---------------------------------------------------------------------------

/// Query parameters for listing sanads.
#[derive(Deserialize)]
pub struct ListSanadsQuery {
    pub chain: Option<String>,
    pub owner: Option<String>,
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// GET /api/v1/sanads
pub async fn list_sanads(
    Query(query): Query<ListSanadsQuery>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<PaginatedResponse<tuppira_shared::SanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let repo = SanadsRepository::new(pool);

    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let filter = SanadFilter {
        chain: query.chain,
        owner: query.owner,
        status: query.status.as_deref().map(|s| match s {
            "active" => tuppira_shared::SanadStatus::Active,
            "spent" => tuppira_shared::SanadStatus::Spent,
            "pending" => tuppira_shared::SanadStatus::Pending,
            _ => tuppira_shared::SanadStatus::Active,
        }),
        limit: Some(limit),
        offset: Some(offset),
    };

    let total = repo.count().await.map_err(tuppira_error)?;

    let data = repo.list(&filter).await.map_err(tuppira_error)?;

    Ok(Json(ApiResponse::from(PaginatedResponse {
        data,
        total: total as u64,
        limit,
        offset,
    })))
}

/// GET /api/v1/sanads/:id
pub async fn get_sanad(
    Path(id): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<tuppira_shared::SanadRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let repo = SanadsRepository::new(pool);

    let sanad = repo.get_by_id(&id).await.map_err(tuppira_error)?;

    match sanad {
        Some(r) => Ok(Json(ApiResponse::from(r))),
        None => Err(not_found(&format!("Sanad {} not found", id))),
    }
}

// ---------------------------------------------------------------------------
// Transfers handlers
// ---------------------------------------------------------------------------

/// Query parameters for listing transfers.
#[derive(Deserialize)]
pub struct ListTransfersQuery {
    pub sanad_id: Option<String>,
    pub from_chain: Option<String>,
    pub to_chain: Option<String>,
    pub status: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// GET /api/v1/transfers
pub async fn list_transfers(
    Query(query): Query<ListTransfersQuery>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<PaginatedResponse<tuppira_shared::TransferRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let repo = TransfersRepository::new(pool);

    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let filter = TransferFilter {
        sanad_id: query.sanad_id,
        from_chain: query.from_chain,
        to_chain: query.to_chain,
        status: query.status.as_deref().map(|s| match s {
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

    let total = repo.count(filter.clone()).await.map_err(tuppira_error)?;

    let data = repo.list(filter).await.map_err(tuppira_error)?;

    Ok(Json(ApiResponse::from(PaginatedResponse {
        data,
        total,
        limit,
        offset,
    })))
}

/// GET /api/v1/transfers/:id
pub async fn get_transfer(
    Path(id): Path<String>,
    State((_, pool, _, networks)): State<AppState>,
) -> Result<Json<ApiResponse<tuppira_shared::TransferRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let repo = TransfersRepository::new(pool);

    let mut transfer = repo.get(&id).await.map_err(tuppira_error)?;

    match transfer {
        Some(ref mut t) => {
            // Link out to each chain's official explorer, network-aware. `None`
            // (unknown chain / local devnet) leaves the field absent rather than
            // fabricating a URL.
            let network_of = |chain: &str| {
                networks
                    .get(chain)
                    .copied()
                    .unwrap_or(tuppira_shared::Network::Mainnet)
            };
            t.lock_tx_explorer_url = tuppira_shared::block_explorer::tx_url(
                &t.from_chain,
                network_of(&t.from_chain),
                &t.lock_tx,
            );
            if let Some(ref mint_tx) = t.mint_tx {
                t.mint_tx_explorer_url = tuppira_shared::block_explorer::tx_url(
                    &t.to_chain,
                    network_of(&t.to_chain),
                    mint_tx,
                );
            }
            Ok(Json(ApiResponse::from(t.clone())))
        }
        None => Err(not_found(&format!("Transfer {} not found", id))),
    }
}

// ---------------------------------------------------------------------------
// Seals handlers
// ---------------------------------------------------------------------------

/// Query parameters for listing seals.
#[derive(Deserialize)]
pub struct ListSealsQuery {
    pub chain: Option<String>,
    pub seal_type: Option<String>,
    pub status: Option<String>,
    pub sanad_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// GET /api/v1/seals
pub async fn list_seals(
    Query(query): Query<ListSealsQuery>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<PaginatedResponse<tuppira_shared::SealRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let repo = SealsRepository::new(pool);

    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let filter = SealFilter {
        chain: query.chain,
        seal_type: query.seal_type.as_deref().map(|s| match s {
            "utxo" => tuppira_shared::SealType::Utxo,
            "object" => tuppira_shared::SealType::Object,
            "resource" => tuppira_shared::SealType::Resource,
            "nullifier" => tuppira_shared::SealType::Nullifier,
            "account" => tuppira_shared::SealType::Account,
            _ => tuppira_shared::SealType::Utxo,
        }),
        status: query.status.as_deref().map(|s| match s {
            "available" => tuppira_shared::SealStatus::Available,
            "consumed" => tuppira_shared::SealStatus::Consumed,
            _ => tuppira_shared::SealStatus::Available,
        }),
        sanad_id: query.sanad_id,
        limit: Some(limit),
        offset: Some(offset),
    };

    let total = repo.count(filter.clone()).await.map_err(tuppira_error)?;

    let data = repo.list(filter).await.map_err(tuppira_error)?;

    Ok(Json(ApiResponse::from(PaginatedResponse {
        data,
        total,
        limit,
        offset,
    })))
}

/// GET /api/v1/seals/:id
pub async fn get_seal(
    Path(id): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<tuppira_shared::SealRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let repo = SealsRepository::new(pool);

    let seal = repo.get(&id).await.map_err(tuppira_error)?;

    match seal {
        Some(s) => Ok(Json(ApiResponse::from(s))),
        None => Err(not_found(&format!("Seal {} not found", id))),
    }
}

// ---------------------------------------------------------------------------
// Stats handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/stats
pub async fn get_stats(
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<tuppira_shared::TuppiraStats>>, (StatusCode, Json<ErrorResponse>)> {
    let repo = StatsRepository::new(pool);

    let stats = repo.get_stats().await.map_err(tuppira_error)?;

    Ok(Json(ApiResponse::from(stats)))
}

// ---------------------------------------------------------------------------
// Chains handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/chains
pub async fn list_chains(
    _state: State<AppState>,
) -> Result<Json<ApiResponse<Vec<tuppira_shared::ChainInfo>>>, (StatusCode, Json<ErrorResponse>)> {
    Err(service_unavailable(
        "chain status is unavailable: the explorer is not connected to an authoritative indexer status source",
    ))
}

// ---------------------------------------------------------------------------
// Wallet priority indexing handlers
// ---------------------------------------------------------------------------

/// Request body for registering a wallet address.
#[derive(Deserialize, Serialize)]
pub struct RegisterWalletAddressRequest {
    pub address: String,
    pub chain: String,
    pub network: String,
    pub priority: String,
    pub wallet_id: String,
}

/// POST /api/v1/wallet/addresses
pub async fn register_wallet_address(
    State((_, pool, _, _)): State<AppState>,
    Json(request): Json<RegisterWalletAddressRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    use tuppira_shared::{Network, PriorityLevel};

    let network = match request.network.to_lowercase().as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "devnet" => Network::Devnet,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid network. Must be: mainnet, testnet, or devnet".to_string(),
                    success: false,
                }),
            ));
        }
    };

    let priority = match request.priority.to_lowercase().as_str() {
        "high" => PriorityLevel::High,
        "normal" | "medium" => PriorityLevel::Normal,
        "low" => PriorityLevel::Low,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid priority. Must be: high, normal, or low".to_string(),
                    success: false,
                }),
            ));
        }
    };

    // Register the address in the priority repository
    let priority_repo = tuppira_storage::repositories::PriorityAddressRepository::new(pool);

    priority_repo
        .register_address(
            &request.address,
            &request.chain,
            network,
            priority,
            &request.wallet_id,
        )
        .await
        .map_err(internal_error)?;

    Ok(Json(ApiResponse::from(serde_json::json!({
        "message": "Address registered for priority indexing",
        "address": request.address,
        "chain": request.chain,
        "network": request.network,
        "priority": request.priority,
    }))))
}

/// Request body for unregistering a wallet address.
#[derive(Deserialize, Serialize)]
pub struct UnregisterWalletAddressRequest {
    pub address: String,
    pub chain: String,
    pub network: String,
    pub wallet_id: String,
}

/// DELETE /api/v1/wallet/addresses
pub async fn unregister_wallet_address(
    State((_, pool, _, _)): State<AppState>,
    Json(request): Json<UnregisterWalletAddressRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    use tuppira_shared::Network;

    let network = match request.network.to_lowercase().as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "devnet" => Network::Devnet,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid network. Must be: mainnet, testnet, or devnet".to_string(),
                    success: false,
                }),
            ));
        }
    };

    let priority_repo = tuppira_storage::repositories::PriorityAddressRepository::new(pool);

    let removed = priority_repo
        .unregister_address(
            &request.address,
            &request.chain,
            network,
            &request.wallet_id,
        )
        .await
        .map_err(internal_error)?;

    if !removed {
        return Err(not_found("Address not found or already unregistered"));
    }

    Ok(Json(ApiResponse::from(serde_json::json!({
        "message": "Address unregistered from priority indexing",
        "address": request.address,
        "chain": request.chain,
    }))))
}

/// GET /api/v1/wallet/{wallet_id}/addresses
pub async fn get_wallet_addresses(
    Path(wallet_id): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<tuppira_shared::PriorityAddress>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let priority_repo = tuppira_storage::repositories::PriorityAddressRepository::new(pool);

    let addresses = priority_repo
        .get_addresses_by_wallet(&wallet_id)
        .await
        .map_err(internal_error)?;

    Ok(Json(ApiResponse::from(addresses)))
}

/// GET /api/v1/wallet/address/{address}/data
pub async fn get_address_data(
    Path(address): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    use tuppira_shared::{SanadFilter, SealFilter, TransferFilter};

    // Get sanads for this address
    let sanads_repo = SanadsRepository::new(pool.clone());
    let sanads_filter = SanadFilter {
        owner: Some(address.clone()),
        limit: Some(100),
        offset: Some(0),
        chain: None,
        status: None,
    };
    let sanads = sanads_repo
        .list(&sanads_filter)
        .await
        .map_err(tuppira_error)?;

    // Get seals for this address
    let seals_repo = SealsRepository::new(pool.clone());
    // Note: SealFilter doesn't have owner field, so we'll get all seals
    let seals_filter = SealFilter {
        limit: Some(100),
        offset: Some(0),
        chain: None,
        seal_type: None,
        status: None,
        sanad_id: None,
    };
    let seals = seals_repo.list(seals_filter).await.map_err(tuppira_error)?;

    // Get transfers for this address
    let transfers_repo = TransfersRepository::new(pool.clone());
    let transfers_filter = TransferFilter {
        limit: Some(100),
        offset: Some(0),
        sanad_id: None,
        from_chain: None,
        to_chain: None,
        status: None,
    };
    let transfers = transfers_repo
        .list(transfers_filter)
        .await
        .map_err(tuppira_error)?;

    // Filter transfers where this address is involved
    let filtered_transfers: Vec<_> = transfers
        .into_iter()
        .filter(|t| t.from_owner == address || t.to_owner == address)
        .collect();

    Ok(Json(ApiResponse::from(serde_json::json!({
        "address": address,
        "sanads": sanads,
        "seals": seals,
        "transfers": filtered_transfers,
        "summary": {
            "total_sanads": sanads.len(),
            "total_seals": seals.len(),
            "total_transfers": filtered_transfers.len(),
        }
    }))))
}

/// GET /api/v1/wallet/address/{address}/sanads
pub async fn get_address_sanads(
    Path(address): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<Vec<tuppira_shared::SanadRecord>>>, (StatusCode, Json<ErrorResponse>)>
{
    let repo = SanadsRepository::new(pool);

    let filter = SanadFilter {
        owner: Some(address),
        limit: Some(100),
        offset: Some(0),
        chain: None,
        status: None,
    };

    let sanads = repo.list(&filter).await.map_err(tuppira_error)?;

    Ok(Json(ApiResponse::from(sanads)))
}

/// GET /api/v1/wallet/address/{address}/seals
pub async fn get_address_seals(
    Path(_address): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<Vec<tuppira_shared::SealRecord>>>, (StatusCode, Json<ErrorResponse>)> {
    let repo = SealsRepository::new(pool);

    let filter = SealFilter {
        limit: Some(100),
        offset: Some(0),
        chain: None,
        seal_type: None,
        status: None,
        sanad_id: None,
    };

    let seals = repo.list(filter).await.map_err(tuppira_error)?;

    Ok(Json(ApiResponse::from(seals)))
}

/// GET /api/v1/wallet/address/{address}/transfers
pub async fn get_address_transfers(
    Path(address): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<Vec<tuppira_shared::TransferRecord>>>, (StatusCode, Json<ErrorResponse>)>
{
    let repo = TransfersRepository::new(pool);

    let filter = TransferFilter {
        limit: Some(100),
        offset: Some(0),
        sanad_id: None,
        from_chain: None,
        to_chain: None,
        status: None,
    };

    let transfers = repo.list(filter).await.map_err(tuppira_error)?;

    // Filter transfers where this address is involved
    let filtered_transfers: Vec<_> = transfers
        .into_iter()
        .filter(|t| t.from_owner == address || t.to_owner == address)
        .collect();

    Ok(Json(ApiResponse::from(filtered_transfers)))
}

/// GET /api/v1/wallet/priority/status
pub async fn get_priority_indexing_status(
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<tuppira_shared::PriorityIndexingStatus>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let priority_repo = tuppira_storage::repositories::PriorityAddressRepository::new(pool);

    let status = priority_repo
        .get_priority_indexing_status()
        .await
        .map_err(internal_error)?;

    Ok(Json(ApiResponse::from(status)))
}

// ---------------------------------------------------------------------------
// Health check
// ---------------------------------------------------------------------------

/// GET /health
pub async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "tuppira-api"
    }))
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn server_error(e: &TuppiraError) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: e.to_string(),
            success: false,
        }),
    )
}

fn not_found(message: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: message.to_string(),
            success: false,
        }),
    )
}

fn service_unavailable(message: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: message.to_string(),
            success: false,
        }),
    )
}

fn internal_error(e: sqlx::Error) -> (StatusCode, Json<ErrorResponse>) {
    server_error(&TuppiraError::Internal(e.to_string()))
}

fn tuppira_error(e: TuppiraError) -> (StatusCode, Json<ErrorResponse>) {
    server_error(&e)
}

#[cfg(test)]
mod tests {
    use super::service_unavailable;
    use axum::{Json, http::StatusCode};

    #[test]
    fn unavailable_response_is_not_a_success_payload() {
        let (status, Json(response)) = service_unavailable("unavailable");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!response.success);
        assert_eq!(response.error, "unavailable");
    }
}

// ---------------------------------------------------------------------------
// Enhanced sanads and proof metadata handlers
// ---------------------------------------------------------------------------

/// Query parameters for listing enhanced sanads.
#[derive(Deserialize)]
pub struct EnhancedSanadsQuery {
    pub chain: Option<String>,
    pub owner: Option<String>,
    pub commitment_scheme: Option<String>,
    pub inclusion_proof_type: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// GET /api/v1/sanads/enhanced
pub async fn list_enhanced_sanads(
    Query(query): Query<EnhancedSanadsQuery>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<tuppira_shared::EnhancedSanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use tuppira_shared::SanadProofFilter;
    use tuppira_storage::repositories::AdvancedProofRepository;

    let repo = AdvancedProofRepository::new(pool);

    let filter = SanadProofFilter {
        chain: query.chain,
        owner: query.owner,
        commitment_scheme: query
            .commitment_scheme
            .as_deref()
            .and_then(|s| tuppira_shared::CommitmentScheme::from_str(s).ok()),
        inclusion_proof_type: query
            .inclusion_proof_type
            .as_deref()
            .and_then(|s| tuppira_shared::InclusionProofType::from_str(s).ok()),
        finality_proof_type: None,
        limit: query.limit,
        offset: query.offset,
    };

    let records = repo
        .query_enhanced_sanads(filter)
        .await
        .map_err(internal_error)?;

    Ok(Json(ApiResponse::from(records)))
}

/// GET /api/v1/sanads/enhanced/:id
pub async fn get_enhanced_sanad(
    Path(id): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<tuppira_shared::EnhancedSanadRecord>>, (StatusCode, Json<ErrorResponse>)>
{
    use tuppira_shared::SanadProofFilter;
    use tuppira_storage::repositories::AdvancedProofRepository;

    let repo = AdvancedProofRepository::new(pool);

    let filter = SanadProofFilter {
        chain: None,
        owner: None,
        commitment_scheme: None,
        inclusion_proof_type: None,
        finality_proof_type: None,
        limit: Some(1),
        offset: Some(0),
    };

    let records = repo
        .query_enhanced_sanads(filter)
        .await
        .map_err(internal_error)?;

    let record = records.into_iter().find(|r| r.id == id);

    match record {
        Some(r) => Ok(Json(ApiResponse::from(r))),
        None => Err(not_found(&format!("Enhanced sanad {} not found", id))),
    }
}

/// GET /api/v1/seals/enhanced
pub async fn list_enhanced_seals(
    Query(query): Query<crate::rest::handlers::ListSealsQuery>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<tuppira_shared::EnhancedSealRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use tuppira_shared::SealProofFilter;
    use tuppira_storage::repositories::AdvancedProofRepository;

    let repo = AdvancedProofRepository::new(pool);

    let filter = SealProofFilter {
        chain: query.chain,
        seal_type: query.seal_type,
        seal_proof_type: None,
        seal_proof_verified: None,
        limit: query.limit,
        offset: query.offset,
    };

    let records = repo
        .query_enhanced_seals(filter)
        .await
        .map_err(internal_error)?;

    Ok(Json(ApiResponse::from(records)))
}

/// GET /api/v1/seals/enhanced/:id
pub async fn get_enhanced_seal(
    Path(id): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<tuppira_shared::EnhancedSealRecord>>, (StatusCode, Json<ErrorResponse>)>
{
    use tuppira_shared::SealProofFilter;
    use tuppira_storage::repositories::AdvancedProofRepository;

    let repo = AdvancedProofRepository::new(pool);

    let filter = SealProofFilter {
        chain: None,
        seal_type: None,
        seal_proof_type: None,
        seal_proof_verified: None,
        limit: Some(1),
        offset: Some(0),
    };

    let records = repo
        .query_enhanced_seals(filter)
        .await
        .map_err(internal_error)?;

    let record = records.into_iter().find(|r| r.id == id);

    match record {
        Some(r) => Ok(Json(ApiResponse::from(r))),
        None => Err(not_found(&format!("Enhanced seal {} not found", id))),
    }
}

/// GET /api/v1/proofs/statistics
pub async fn get_proof_statistics(
    State((_, pool, _, _)): State<AppState>,
) -> Result<Json<ApiResponse<tuppira_shared::ProofStatistics>>, (StatusCode, Json<ErrorResponse>)> {
    use tuppira_storage::repositories::AdvancedProofRepository;

    let repo = AdvancedProofRepository::new(pool);

    let stats = repo.get_proof_statistics().await.map_err(internal_error)?;

    Ok(Json(ApiResponse::from(stats)))
}

/// GET /api/v1/sanads/by-scheme/:scheme
pub async fn get_sanads_by_scheme(
    Path(scheme): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<tuppira_shared::EnhancedSanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use tuppira_shared::{CommitmentScheme, SanadProofFilter};
    use tuppira_storage::repositories::AdvancedProofRepository;

    let commitment_scheme = CommitmentScheme::from_str(&scheme)
        .map_err(|_| not_found(&format!("Unknown commitment scheme: {}", scheme)))?;

    let repo = AdvancedProofRepository::new(pool);

    let filter = SanadProofFilter {
        chain: None,
        owner: None,
        commitment_scheme: Some(commitment_scheme),
        inclusion_proof_type: None,
        finality_proof_type: None,
        limit: Some(100),
        offset: Some(0),
    };

    let records = repo
        .query_enhanced_sanads(filter)
        .await
        .map_err(internal_error)?;

    Ok(Json(ApiResponse::from(records)))
}

/// GET /api/v1/sanads/by-proof/:proof_type
pub async fn get_sanads_by_proof_type(
    Path(proof_type): Path<String>,
    State((_, pool, _, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<tuppira_shared::EnhancedSanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use tuppira_shared::{InclusionProofType, SanadProofFilter};
    use tuppira_storage::repositories::AdvancedProofRepository;

    let inclusion_proof_type = InclusionProofType::from_str(&proof_type)
        .map_err(|_| not_found(&format!("Unknown inclusion proof type: {}", proof_type)))?;

    let repo = AdvancedProofRepository::new(pool);

    let filter = SanadProofFilter {
        chain: None,
        owner: None,
        commitment_scheme: None,
        inclusion_proof_type: Some(inclusion_proof_type),
        finality_proof_type: None,
        limit: Some(100),
        offset: Some(0),
    };

    let records = repo
        .query_enhanced_sanads(filter)
        .await
        .map_err(internal_error)?;

    Ok(Json(ApiResponse::from(records)))
}
