/// REST API routes for the CSV Explorer.
use axum::{Router, routing::delete, routing::get, routing::post};

use super::handlers;

type AppState = (
    async_graphql::Schema<
        crate::graphql::schema::Query,
        crate::graphql::schema::Mutation,
        crate::graphql::schema::Subscription,
    >,
    sqlx::SqlitePool,
    crate::feed::WalletFeedHub,
);

/// Build the REST API router.
pub fn rest_routes() -> Router<AppState> {
    Router::new()
        // Sanads
        .route("/sanads", get(handlers::list_sanads))
        .route("/sanads/{id}", get(handlers::get_sanad))
        // Transfers
        .route("/transfers", get(handlers::list_transfers))
        .route("/transfers/{id}", get(handlers::get_transfer))
        // Seals
        .route("/seals", get(handlers::list_seals))
        .route("/seals/{id}", get(handlers::get_seal))
        // Stats
        .route("/stats", get(handlers::get_stats))
        // Chains
        .route("/chains", get(handlers::list_chains))
        .route("/wallet/feed", get(handlers::wallet_feed))
        // Wallet priority indexing
        .route("/wallet/addresses", post(handlers::register_wallet_address))
        .route(
            "/wallet/addresses",
            delete(handlers::unregister_wallet_address),
        )
        .route(
            "/wallet/{wallet_id}/addresses",
            get(handlers::get_wallet_addresses),
        )
        .route(
            "/wallet/address/{address}/data",
            get(handlers::get_address_data),
        )
        .route(
            "/wallet/address/{address}/sanads",
            get(handlers::get_address_sanads),
        )
        .route(
            "/wallet/address/{address}/seals",
            get(handlers::get_address_seals),
        )
        .route(
            "/wallet/address/{address}/transfers",
            get(handlers::get_address_transfers),
        )
        .route(
            "/wallet/priority/status",
            get(handlers::get_priority_indexing_status),
        )
        // Enhanced sanads with commitment metadata
        .route("/sanads/enhanced", get(handlers::list_enhanced_sanads))
        .route("/sanads/enhanced/{id}", get(handlers::get_enhanced_sanad))
        // Enhanced seals with proof metadata
        .route("/seals/enhanced", get(handlers::list_enhanced_seals))
        .route("/seals/enhanced/{id}", get(handlers::get_enhanced_seal))
        // Proof statistics
        .route("/proofs/statistics", get(handlers::get_proof_statistics))
        // Filter by commitment scheme
        .route(
            "/sanads/by-scheme/{scheme}",
            get(handlers::get_sanads_by_scheme),
        )
        // Filter by proof type
        .route(
            "/sanads/by-proof/{proof_type}",
            get(handlers::get_sanads_by_proof_type),
        )
}

/// Build the full API v1 router with prefix.
pub fn api_v1_routes() -> Router<AppState> {
    Router::new().nest("/api/v1", rest_routes())
}
