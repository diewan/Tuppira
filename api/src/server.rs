use async_graphql::Request;
/// API server setup and configuration.
///
/// Combines GraphQL and REST APIs with CORS, tracing, and metrics.
use async_graphql::http::{GraphQLPlaygroundConfig, playground_source};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQLSubscription};
use axum::{
    Router, Server,
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
};
use sqlx::SqlitePool;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use csv_explorer_storage::init_pool;

use crate::feed::WalletFeedHub;
use crate::graphql::{create_schema, schema::GraphqlContext};
use crate::rest;
use csv_explorer_shared::{ApiConfig, ExplorerConfig, Result};

/// The API server.
pub struct ApiServer {
    config: ApiConfig,
    pool: SqlitePool,
}

impl ApiServer {
    /// Create a new API server.
    pub async fn new(config: ExplorerConfig) -> Result<Self> {
        let pool = init_pool(&config.database.url, config.database.max_connections).await?;
        Ok(Self {
            config: config.api,
            pool,
        })
    }

    /// Start the API server.
    pub async fn start(self) -> Result<()> {
        let pool = self.pool.clone();
        let feed = WalletFeedHub::new();
        let schema = create_schema(GraphqlContext {
            pool: pool.clone(),
            feed: feed.clone(),
        });

        // Merge REST API into main router
        let app = Router::new()
            // GraphQL endpoint
            .route("/graphql", axum::routing::post(graphql_handler))
            // GraphQL WebSocket subscriptions use the same schema DTO/context.
            .route_service("/graphql/ws", GraphQLSubscription::new(schema.clone()))
            // GraphQL Playground
            .route("/playground", get(graphql_playground))
            // REST API v1
            .merge(rest::routes::api_v1_routes())
            // Prometheus metrics
            .route("/metrics", get(metrics_handler))
            // Health check
            .route("/health", get(health_handler))
            // Middleware
            .layer(CorsLayer::permissive())
            .layer(TraceLayer::new_for_http())
            .layer(ServiceBuilder::new())
            .with_state((schema, pool, feed));

        let addr: std::net::SocketAddr = self.config.bind().parse().map_err(|e| {
            csv_explorer_shared::ExplorerError::Internal(format!(
                "Invalid address {}: {}",
                self.config.bind(),
                e
            ))
        })?;

        tracing::info!(addr = %addr, "API server started");
        Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .map_err(|e| {
                csv_explorer_shared::ExplorerError::Internal(format!("Server error: {}", e))
            })?;

        Ok(())
    }
}

/// Serve the GraphQL Playground HTML.
async fn graphql_playground() -> impl IntoResponse {
    Html(playground_source(GraphQLPlaygroundConfig::new("/graphql")))
}

/// GraphQL request handler.
async fn graphql_handler(
    State((schema, _, _)): State<(
        async_graphql::Schema<
            crate::graphql::schema::Query,
            crate::graphql::schema::Mutation,
            crate::graphql::schema::Subscription,
        >,
        SqlitePool,
        WalletFeedHub,
    )>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let inner_req = req.into_inner();
    let request = Request::from(inner_req);
    let response = schema.execute(request).await;
    GraphQLResponse::from(response)
}

/// Serve Prometheus metrics.
async fn metrics_handler() -> impl IntoResponse {
    let metrics = csv_explorer_indexer::metrics::encode_metrics();
    metrics
}

/// Health check handler.
async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "service": "csv-explorer-api"
    }))
}
