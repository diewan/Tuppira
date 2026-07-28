//! Rendering boundary for Parwana's published portable-conformance corpus.
//!
//! The corpus records expected *Parwana assurance* results. Tuppira may display
//! those results as fixture metadata, but must not translate them into its
//! observation states or present them as a verdict produced by Tuppira.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The Parwana package version consumed by this build.
pub const PARWANA_CONFORMANCE_PACKAGE_VERSION: &str = csv_sdk::v2::CONFORMANCE_PACKAGE_VERSION;

/// A case from Parwana's immutable conformance manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedConformanceCase {
    pub id: String,
    pub category: String,
    pub contract_version: String,
    pub wire_version: u16,
    pub bytes_hex: String,
    pub expected_dimensions: BTreeMap<String, String>,
    pub expected_reason_code: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedConformanceManifest {
    schema_version: u16,
    version: String,
    package: String,
    platforms: serde_json::Value,
    cases: Vec<PublishedConformanceCase>,
}

/// Display model for a published fixture.
///
/// The field names deliberately keep Parwana's expected result under the
/// `parwana_expected_*` namespace. `tuppira_assertion` is a constant statement
/// of provenance, not a computed assurance dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedConformanceFixtureView {
    pub fixture_id: String,
    pub category: String,
    pub contract_version: String,
    pub wire_version: u16,
    pub parwana_expected_dimensions: BTreeMap<String, String>,
    pub parwana_expected_reason_code: String,
    pub tuppira_assertion: &'static str,
}

/// Load the exact manifest embedded by the pinned Parwana SDK.
///
/// Keeping this read behind the SDK facade makes a stale copied corpus
/// impossible: upgrading the pin changes the bytes exercised by Tuppira's
/// conformance test.
pub fn published_conformance_cases() -> Result<Vec<PublishedConformanceCase>, serde_json::Error> {
    let manifest: PublishedConformanceManifest =
        serde_json::from_slice(csv_sdk::v2::conformance_manifest())?;
    // Reading these fields is intentional: serde's deny-unknown-fields protects
    // the contract shape, while the SDK constants protect package identity.
    let _metadata = (
        manifest.schema_version,
        manifest.version,
        manifest.package,
        manifest.platforms,
    );
    Ok(manifest.cases)
}

/// Render one published fixture without upgrading it into Tuppira authority.
#[must_use]
pub fn render_published_fixture(
    case: &PublishedConformanceCase,
) -> PublishedConformanceFixtureView {
    PublishedConformanceFixtureView {
        fixture_id: case.id.clone(),
        category: case.category.clone(),
        contract_version: case.contract_version.clone(),
        wire_version: case.wire_version,
        parwana_expected_dimensions: case.expected_dimensions.clone(),
        parwana_expected_reason_code: case.expected_reason_code.clone(),
        tuppira_assertion: "published_fixture_metadata_only",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_published_fixture_renders_without_becoming_a_tuppira_verdict() {
        let cases = published_conformance_cases().expect("pinned manifest must decode");
        assert!(!cases.is_empty());

        let mut covered = BTreeSet::new();
        for case in &cases {
            let view = render_published_fixture(case);
            let json = serde_json::to_value(&view).expect("view must render");

            assert_eq!(view.fixture_id, case.id);
            assert_eq!(view.parwana_expected_dimensions, case.expected_dimensions);
            assert_eq!(view.parwana_expected_reason_code, case.expected_reason_code);
            assert_eq!(view.tuppira_assertion, "published_fixture_metadata_only");
            assert!(json.get("parwana_expected_dimensions").is_some());
            assert!(json.get("verification_verdict").is_none());
            assert!(json.get("tuppira_verdict").is_none());

            if case.id == "valid-v2"
                || case.id == "losing-conflict"
                || case.id == "checkpoint-stale"
                || case.id == "reorganization"
            {
                covered.insert(case.id.as_str());
            }
        }

        assert_eq!(
            covered,
            BTreeSet::from([
                "checkpoint-stale",
                "losing-conflict",
                "reorganization",
                "valid-v2",
            ])
        );
    }
}
