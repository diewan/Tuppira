/// API server setup and configuration.
///
/// Combines GraphQL and REST APIs with CORS, tracing, and metrics.
use async_graphql::http::{GraphQLPlaygroundConfig, playground_source};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQLSubscription};
use axum::{
    Router, Server,
    extract::{DefaultBodyLimit, State},
    http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE},
    response::{Html, IntoResponse},
    routing::get,
};
use sqlx::SqlitePool;
use tower::ServiceBuilder;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};

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
        let feed = WalletFeedHub::from_pool(pool.clone())
            .await
            .map_err(csv_explorer_shared::ExplorerError::Internal)?;
        let schema = create_schema(
            GraphqlContext {
                pool: pool.clone(),
                feed: feed.clone(),
            },
            self.config.enable_graphql_playground,
        );
        let cors = cors_layer(&self.config.cors_origins)?;

        // Merge REST API into main router
        let mut app = Router::new()
            // GraphQL endpoint
            .route("/graphql", axum::routing::post(graphql_handler))
            // GraphQL WebSocket subscriptions use the same schema DTO/context.
            .route_service("/graphql/ws", GraphQLSubscription::new(schema.clone()))
            // REST API v1
            .merge(rest::routes::api_v1_routes())
            // Prometheus metrics
            .route("/metrics", get(metrics_handler))
            // Health check
            .route("/health", get(health_handler))
            // Middleware
            .layer(cors)
            .layer(DefaultBodyLimit::max(1_048_576))
            .layer(TraceLayer::new_for_http())
            .layer(ServiceBuilder::new())
            .with_state((schema, pool, feed));
        if self.config.enable_graphql_playground {
            app = app.route("/playground", get(graphql_playground));
        }

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

fn cors_layer(origins: &[String]) -> Result<CorsLayer> {
    let mut allowed = Vec::with_capacity(origins.len());
    for origin in origins {
        if origin == "*" {
            return Err(csv_explorer_shared::ExplorerError::Parse(
                "api.cors_origins must list explicit origins; '*' is forbidden".to_string(),
            ));
        }
        let value = HeaderValue::from_str(origin).map_err(|error| {
            csv_explorer_shared::ExplorerError::Parse(format!(
                "invalid api.cors_origins entry {origin:?}: {error}"
            ))
        })?;
        allowed.push(value);
    }

    let layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers([CONTENT_TYPE]);
    if allowed.is_empty() {
        Ok(layer)
    } else {
        Ok(layer.allow_origin(AllowOrigin::list(allowed)))
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
    let request = inner_req;
    let response = schema.execute(request).await;
    GraphQLResponse::from(response)
}

/// Serve Prometheus metrics.
async fn metrics_handler() -> impl IntoResponse {
    csv_explorer_indexer::metrics::encode_metrics()
}

/// Health check handler.
async fn health_handler(
    State((_, pool, _)): State<(
        async_graphql::Schema<
            crate::graphql::schema::Query,
            crate::graphql::schema::Mutation,
            crate::graphql::schema::Subscription,
        >,
        SqlitePool,
        WalletFeedHub,
    )>,
) -> impl IntoResponse {
    if sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&pool)
        .await
        .is_ok()
    {
        (
            StatusCode::OK,
            axum::Json(serde_json::json!({"status": "ok", "service": "csv-explorer-api"})),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({"status": "unavailable", "service": "csv-explorer-api"})),
        )
    }
}
