/// REST API routes for the Tuppira.
use axum::{Router, routing::get};

use super::handlers;

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
        .route("/observations", get(handlers::observation_list))
        .route("/entities/{id}", get(handlers::entity_profile))
        .route(
            "/entities/{id}/accountability",
            get(handlers::entity_accountability),
        )
        .route(
            "/observations/{id}/lineage",
            get(handlers::observation_lineage),
        )
        .route(
            "/observation-sources/health",
            get(handlers::observation_source_health),
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
