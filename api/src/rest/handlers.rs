/// REST API handlers for the CSV Explorer.
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use csv_explorer_storage::repositories::{
    SanadsRepository, SealsRepository, StatsRepository, TransfersRepository,
};

use csv_explorer_shared::{ExplorerError, SanadFilter, SealFilter, TransferFilter};
use std::str::FromStr;

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
    State((_, _, feed)): State<AppState>,
) -> Json<ApiResponse<Vec<csv_explorer_shared::WalletFeedEnvelope>>> {
    Json(ApiResponse::from(
        feed.since(query.after_sequence.unwrap_or(0)).await,
    ))
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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<PaginatedResponse<csv_explorer_shared::SanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let repo = SanadsRepository::new(pool);

    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let filter = SanadFilter {
        chain: query.chain,
        owner: query.owner,
        status: query.status.as_deref().map(|s| match s {
            "active" => csv_explorer_shared::SanadStatus::Active,
            "spent" => csv_explorer_shared::SanadStatus::Spent,
            "pending" => csv_explorer_shared::SanadStatus::Pending,
            _ => csv_explorer_shared::SanadStatus::Active,
        }),
        limit: Some(limit),
        offset: Some(offset),
    };

    let total = repo.count().await.map_err(explorer_error)?;

    let data = repo.list(&filter).await.map_err(explorer_error)?;

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
    State((_, pool, _)): State<AppState>,
) -> Result<Json<ApiResponse<csv_explorer_shared::SanadRecord>>, (StatusCode, Json<ErrorResponse>)>
{
    let repo = SanadsRepository::new(pool);

    let sanad = repo.get_by_id(&id).await.map_err(explorer_error)?;

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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<PaginatedResponse<csv_explorer_shared::TransferRecord>>>,
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
            "pending" => csv_explorer_shared::TransferStatus::Initiated,
            "in_progress" => csv_explorer_shared::TransferStatus::SubmittingProof,
            "completed" => csv_explorer_shared::TransferStatus::Completed,
            "failed" => csv_explorer_shared::TransferStatus::Failed {
                error_code: "UNKNOWN".to_string(),
                retryable: true,
            },
            _ => csv_explorer_shared::TransferStatus::Initiated,
        }),
        limit: Some(limit),
        offset: Some(offset),
    };

    let total = repo.count(filter.clone()).await.map_err(explorer_error)?;

    let data = repo.list(filter).await.map_err(explorer_error)?;

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
    State((_, pool, _)): State<AppState>,
) -> Result<Json<ApiResponse<csv_explorer_shared::TransferRecord>>, (StatusCode, Json<ErrorResponse>)>
{
    let repo = TransfersRepository::new(pool);

    let mut transfer = repo.get(&id).await.map_err(explorer_error)?;

    match transfer {
        Some(ref mut t) => {
            // Populate block explorer URLs based on chain
            t.lock_tx_explorer_url = Some(get_explorer_url(&t.from_chain, &t.lock_tx));
            if let Some(ref mint_tx) = t.mint_tx {
                t.mint_tx_explorer_url = Some(get_explorer_url(&t.to_chain, mint_tx));
            }
            Ok(Json(ApiResponse::from(t.clone())))
        }
        None => Err(not_found(&format!("Transfer {} not found", id))),
    }
}

/// Get block explorer URL for a transaction on a specific chain.
fn get_explorer_url(chain: &str, tx_hash: &str) -> String {
    match chain.to_lowercase().as_str() {
        "bitcoin" => format!("https://blockstream.info/testnet/tx/{}", tx_hash),
        "ethereum" => format!("https://sepolia.etherscan.io/tx/{}", tx_hash),
        "solana" => format!("https://explorer.solana.com/tx/{}?cluster=devnet", tx_hash),
        "sui" => format!("https://suiscan.xyz/testnet/tx/{}", tx_hash),
        "aptos" => format!(
            "https://explorer.aptoslabs.com/txn/{}?network=testnet",
            tx_hash
        ),
        _ => format!("https://explorer.example.com/tx/{}", tx_hash),
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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<PaginatedResponse<csv_explorer_shared::SealRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let repo = SealsRepository::new(pool);

    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let filter = SealFilter {
        chain: query.chain,
        seal_type: query.seal_type.as_deref().map(|s| match s {
            "utxo" => csv_explorer_shared::SealType::Utxo,
            "object" => csv_explorer_shared::SealType::Object,
            "resource" => csv_explorer_shared::SealType::Resource,
            "nullifier" => csv_explorer_shared::SealType::Nullifier,
            "account" => csv_explorer_shared::SealType::Account,
            _ => csv_explorer_shared::SealType::Utxo,
        }),
        status: query.status.as_deref().map(|s| match s {
            "available" => csv_explorer_shared::SealStatus::Available,
            "consumed" => csv_explorer_shared::SealStatus::Consumed,
            _ => csv_explorer_shared::SealStatus::Available,
        }),
        sanad_id: query.sanad_id,
        limit: Some(limit),
        offset: Some(offset),
    };

    let total = repo.count(filter.clone()).await.map_err(explorer_error)?;

    let data = repo.list(filter).await.map_err(explorer_error)?;

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
    State((_, pool, _)): State<AppState>,
) -> Result<Json<ApiResponse<csv_explorer_shared::SealRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let repo = SealsRepository::new(pool);

    let seal = repo.get(&id).await.map_err(explorer_error)?;

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
    State((_, pool, _)): State<AppState>,
) -> Result<Json<ApiResponse<csv_explorer_shared::ExplorerStats>>, (StatusCode, Json<ErrorResponse>)>
{
    let repo = StatsRepository::new(pool);

    let stats = repo.get_stats().await.map_err(explorer_error)?;

    Ok(Json(ApiResponse::from(stats)))
}

// ---------------------------------------------------------------------------
// Chains handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/chains
pub async fn list_chains(
    _state: State<AppState>,
) -> Result<Json<ApiResponse<Vec<csv_explorer_shared::ChainInfo>>>, (StatusCode, Json<ErrorResponse>)>
{
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
    State((_, pool, _)): State<AppState>,
    Json(request): Json<RegisterWalletAddressRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    use csv_explorer_shared::{Network, PriorityLevel};

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
    let priority_repo = csv_explorer_storage::repositories::PriorityAddressRepository::new(pool);

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
    State((_, pool, _)): State<AppState>,
    Json(request): Json<UnregisterWalletAddressRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    use csv_explorer_shared::Network;

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

    let priority_repo = csv_explorer_storage::repositories::PriorityAddressRepository::new(pool);

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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<csv_explorer_shared::PriorityAddress>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let priority_repo = csv_explorer_storage::repositories::PriorityAddressRepository::new(pool);

    let addresses = priority_repo
        .get_addresses_by_wallet(&wallet_id)
        .await
        .map_err(internal_error)?;

    Ok(Json(ApiResponse::from(addresses)))
}

/// GET /api/v1/wallet/address/{address}/data
pub async fn get_address_data(
    Path(address): Path<String>,
    State((_, pool, _)): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, (StatusCode, Json<ErrorResponse>)> {
    use csv_explorer_shared::{SanadFilter, SealFilter, TransferFilter};

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
        .map_err(explorer_error)?;

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
    let seals = seals_repo
        .list(seals_filter)
        .await
        .map_err(explorer_error)?;

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
        .map_err(explorer_error)?;

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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<csv_explorer_shared::SanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let repo = SanadsRepository::new(pool);

    let filter = SanadFilter {
        owner: Some(address),
        limit: Some(100),
        offset: Some(0),
        chain: None,
        status: None,
    };

    let sanads = repo.list(&filter).await.map_err(explorer_error)?;

    Ok(Json(ApiResponse::from(sanads)))
}

/// GET /api/v1/wallet/address/{address}/seals
pub async fn get_address_seals(
    Path(_address): Path<String>,
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<csv_explorer_shared::SealRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let repo = SealsRepository::new(pool);

    let filter = SealFilter {
        limit: Some(100),
        offset: Some(0),
        chain: None,
        seal_type: None,
        status: None,
        sanad_id: None,
    };

    let seals = repo.list(filter).await.map_err(explorer_error)?;

    Ok(Json(ApiResponse::from(seals)))
}

/// GET /api/v1/wallet/address/{address}/transfers
pub async fn get_address_transfers(
    Path(address): Path<String>,
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<csv_explorer_shared::TransferRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let repo = TransfersRepository::new(pool);

    let filter = TransferFilter {
        limit: Some(100),
        offset: Some(0),
        sanad_id: None,
        from_chain: None,
        to_chain: None,
        status: None,
    };

    let transfers = repo.list(filter).await.map_err(explorer_error)?;

    // Filter transfers where this address is involved
    let filtered_transfers: Vec<_> = transfers
        .into_iter()
        .filter(|t| t.from_owner == address || t.to_owner == address)
        .collect();

    Ok(Json(ApiResponse::from(filtered_transfers)))
}

/// GET /api/v1/wallet/priority/status
pub async fn get_priority_indexing_status(
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<csv_explorer_shared::PriorityIndexingStatus>>,
    (StatusCode, Json<ErrorResponse>),
> {
    let priority_repo = csv_explorer_storage::repositories::PriorityAddressRepository::new(pool);

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
        "service": "csv-explorer-api"
    }))
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn server_error(e: &ExplorerError) -> (StatusCode, Json<ErrorResponse>) {
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
    server_error(&ExplorerError::Internal(e.to_string()))
}

fn explorer_error(e: ExplorerError) -> (StatusCode, Json<ErrorResponse>) {
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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<csv_explorer_shared::EnhancedSanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use csv_explorer_shared::SanadProofFilter;
    use csv_explorer_storage::repositories::AdvancedProofRepository;

    let repo = AdvancedProofRepository::new(pool);

    let filter = SanadProofFilter {
        chain: query.chain,
        owner: query.owner,
        commitment_scheme: query
            .commitment_scheme
            .as_deref()
            .and_then(|s| csv_explorer_shared::CommitmentScheme::from_str(s).ok()),
        inclusion_proof_type: query
            .inclusion_proof_type
            .as_deref()
            .and_then(|s| csv_explorer_shared::InclusionProofType::from_str(s).ok()),
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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<csv_explorer_shared::EnhancedSanadRecord>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use csv_explorer_shared::SanadProofFilter;
    use csv_explorer_storage::repositories::AdvancedProofRepository;

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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<csv_explorer_shared::EnhancedSealRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use csv_explorer_shared::SealProofFilter;
    use csv_explorer_storage::repositories::AdvancedProofRepository;

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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<csv_explorer_shared::EnhancedSealRecord>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use csv_explorer_shared::SealProofFilter;
    use csv_explorer_storage::repositories::AdvancedProofRepository;

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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<csv_explorer_shared::ProofStatistics>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use csv_explorer_storage::repositories::AdvancedProofRepository;

    let repo = AdvancedProofRepository::new(pool);

    let stats = repo.get_proof_statistics().await.map_err(internal_error)?;

    Ok(Json(ApiResponse::from(stats)))
}

/// GET /api/v1/sanads/by-scheme/:scheme
pub async fn get_sanads_by_scheme(
    Path(scheme): Path<String>,
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<csv_explorer_shared::EnhancedSanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use csv_explorer_shared::{CommitmentScheme, SanadProofFilter};
    use csv_explorer_storage::repositories::AdvancedProofRepository;

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
    State((_, pool, _)): State<AppState>,
) -> Result<
    Json<ApiResponse<Vec<csv_explorer_shared::EnhancedSanadRecord>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    use csv_explorer_shared::{InclusionProofType, SanadProofFilter};
    use csv_explorer_storage::repositories::AdvancedProofRepository;

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
