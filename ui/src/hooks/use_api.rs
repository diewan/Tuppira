/// API client hook for fetching data from the explorer backend.
///
/// Provides a unified interface for making HTTP requests to the GraphQL
/// or REST API endpoints.
use reqwest::Client;
use std::sync::Arc;

/// Base URL for the API.
const DEFAULT_API_URL: &str = "http://localhost:8080";

/// Get the API base URL from environment or default.
fn api_base_url() -> String {
    std::env::var("API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string())
}

/// Generic API client for making requests.
pub struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    /// Create a new API client.
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: api_base_url(),
        }
    }

    /// Create a client with a custom base URL.
    pub fn with_url(base_url: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
        }
    }

    /// Execute a GraphQL query.
    pub async fn graphql_query(
        &self,
        query: &str,
        variables: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ApiError> {
        let mut payload = serde_json::json!({
            "query": query,
        });
        if let Some(vars) = variables {
            payload["variables"] = vars;
        }

        let response = self
            .client
            .post(format!("{}/graphql", self.base_url))
            .json(&payload)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        if let Some(errors) = response.get("errors") {
            return Err(ApiError::GraphQLError(errors.to_string()));
        }

        Ok(response
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    /// Fetch sanads from the REST API.
    pub async fn get_sanads(
        &self,
        chain: Option<&str>,
        status: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<csv_explorer_shared::SanadRecord>, ApiError> {
        let mut url = format!("{}/api/v1/sanads", self.base_url);
        let mut params = Vec::new();

        if let Some(c) = chain {
            params.push(format!("chain={}", c));
        }
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if let Some(o) = offset {
            params.push(format!("offset={}", o));
        }

        if !params.is_empty() {
            url.push_str(&format!("?{}", params.join("&")));
        }

        let response: ApiResponse<Vec<csv_explorer_shared::SanadRecord>> =
            self.client.get(&url).send().await?.json().await?;
        Ok(response.data)
    }

    /// Fetch a single sanad by ID.
    pub async fn get_sanad(
        &self,
        id: &str,
    ) -> Result<Option<csv_explorer_shared::SanadRecord>, ApiError> {
        let url = format!("{}/api/v1/sanads/{}", self.base_url, id);
        let response: ApiResponse<csv_explorer_shared::SanadRecord> =
            self.client.get(&url).send().await?.json().await?;
        Ok(Some(response.data))
    }

    /// Fetch transfers.
    pub async fn get_transfers(
        &self,
        sanad_id: Option<&str>,
        from_chain: Option<&str>,
        to_chain: Option<&str>,
        status: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<csv_explorer_shared::TransferRecord>, ApiError> {
        let mut url = format!("{}/api/v1/transfers", self.base_url);
        let mut params = Vec::new();

        if let Some(rid) = sanad_id {
            params.push(format!("sanad_id={}", rid));
        }
        if let Some(fc) = from_chain {
            params.push(format!("from_chain={}", fc));
        }
        if let Some(tc) = to_chain {
            params.push(format!("to_chain={}", tc));
        }
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if let Some(o) = offset {
            params.push(format!("offset={}", o));
        }

        if !params.is_empty() {
            url.push_str(&format!("?{}", params.join("&")));
        }

        let response: ApiResponse<Vec<csv_explorer_shared::TransferRecord>> =
            self.client.get(&url).send().await?.json().await?;
        Ok(response.data)
    }

    /// Fetch seals.
    pub async fn get_seals(
        &self,
        chain: Option<&str>,
        status: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<csv_explorer_shared::SealRecord>, ApiError> {
        let mut url = format!("{}/api/v1/seals", self.base_url);
        let mut params = Vec::new();

        if let Some(c) = chain {
            params.push(format!("chain={}", c));
        }
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if let Some(o) = offset {
            params.push(format!("offset={}", o));
        }

        if !params.is_empty() {
            url.push_str(&format!("?{}", params.join("&")));
        }

        let response: ApiResponse<Vec<csv_explorer_shared::SealRecord>> =
            self.client.get(&url).send().await?.json().await?;
        Ok(response.data)
    }

    /// Fetch aggregate statistics.
    pub async fn get_stats(&self) -> Result<csv_explorer_shared::ExplorerStats, ApiError> {
        let url = format!("{}/api/v1/stats", self.base_url);
        let response: ApiResponse<csv_explorer_shared::ExplorerStats> =
            self.client.get(&url).send().await?.json().await?;
        Ok(response.data)
    }

    /// Check API health.
    pub async fn health_check(&self) -> Result<bool, ApiError> {
        let url = format!("{}/health", self.base_url);
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

/// API response wrapper.
#[derive(serde::Deserialize)]
struct ApiResponse<T> {
    data: T,
    success: bool,
}

/// API error types.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("GraphQL error: {0}")]
    GraphQLError(String),

    #[error("API server unreachable")]
    Unreachable,
}
