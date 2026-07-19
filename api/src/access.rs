//! Authentication boundary for tenant-scoped observation reads.

use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationAccess {
    pub tenant_id: String,
}

/// Authenticate an observation request against `TUPPIRA_OBSERVATION_API_KEYS`.
/// The value is a comma-separated list of `tenant-id=opaque-token` entries.
pub fn authenticate(headers: &HeaderMap) -> Result<ObservationAccess, StatusCode> {
    let tenant_id = headers
        .get("x-tuppira-tenant-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| valid_id(value))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let supplied = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let configured = std::env::var("TUPPIRA_OBSERVATION_API_KEYS")
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let expected = configured
        .split(',')
        .filter_map(|entry| entry.split_once('='))
        .find_map(|(tenant, token)| (tenant == tenant_id && !token.is_empty()).then_some(token))
        .ok_or(StatusCode::FORBIDDEN)?;
    if Sha256::digest(supplied.as_bytes()) != Sha256::digest(expected.as_bytes()) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(ObservationAccess {
        tenant_id: tenant_id.to_owned(),
    })
}

fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', ',', '='])
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn rejects_missing_credentials_before_configuration_lookup() {
        assert_eq!(
            authenticate(&HeaderMap::new()),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn rejects_ambiguous_tenant_identifiers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tuppira-tenant-id", HeaderValue::from_static("tenant=a"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        assert_eq!(authenticate(&headers), Err(StatusCode::UNAUTHORIZED));
    }
}
