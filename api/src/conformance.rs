//! Rendering boundary for Parwana's published portable-conformance corpus.
//!
//! The corpus records expected *Parwana assurance* results. Tuppira may display
//! those results as fixture metadata, but must not translate them into its
//! observation states or present them as a verdict produced by Tuppira.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The Parwana package version consumed by this build.
pub const PARWANA_CONFORMANCE_PACKAGE_VERSION: &str = csv_sdk::v2::CONFORMANCE_PACKAGE_VERSION;

/// What a case distributes, as the package declares it.
///
/// Parwana stopped putting `bytes_hex` at the top of a case: a case may
/// distribute canonical consignment bytes, cite a vector in the separately
/// versioned transition-vector package, or distribute nothing at all and say
/// why. Flattening those three into one optional byte string is what made a
/// non-distributing case look like an empty one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PublishedConformanceMaterial {
    /// Canonical V2 consignment bytes.
    ConsignmentV2 {
        bytes_hex: String,
        sha256: String,
        entry_point: String,
    },
    /// The executable material lives in the transition-vector package.
    TransitionVectorRef {
        package: String,
        package_version: u32,
        vector_id: String,
        entry_point: String,
    },
    /// Nothing is distributed; the package records why and what to supply.
    None {
        not_distributed_because: String,
        consumer_must_supply: String,
    },
}

/// A case from Parwana's immutable conformance manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedConformanceCase {
    pub id: String,
    pub category: String,
    pub contract_version: String,
    pub wire_version: u16,
    pub material: PublishedConformanceMaterial,
    pub reproducible_by_sdk_consumer: serde_json::Value,
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
    reason_code_registry: serde_json::Value,
    consumer_contract: serde_json::Value,
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
    /// What the package distributes for this fixture. A reader must be able to
    /// tell a fixture backed by bytes from one that ships nothing, or the
    /// absence of material reads as an unremarkable empty field.
    pub parwana_distributes: &'static str,
    pub tuppira_assertion: &'static str,
}

impl PublishedConformanceMaterial {
    /// Stable name for what this case distributes.
    #[must_use]
    pub const fn distributes(&self) -> &'static str {
        match self {
            Self::ConsignmentV2 { .. } => "consignment-v2",
            Self::TransitionVectorRef { .. } => "transition-vector-ref",
            Self::None { .. } => "nothing",
        }
    }
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
        manifest.reason_code_registry,
        manifest.consumer_contract,
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
        parwana_distributes: case.material.distributes(),
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
            assert_eq!(view.parwana_distributes, case.material.distributes());
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

    /// A fixture that distributes nothing must be visibly distinct from one
    /// backed by bytes. The previous reader flattened every case to a
    /// `bytes_hex` string, so a non-distributing case and an empty one were the
    /// same value — and once Parwana restructured the field, the reader stopped
    /// decoding the published manifest at all.
    #[test]
    fn a_fixture_that_distributes_nothing_says_so() {
        let cases = published_conformance_cases().expect("pinned manifest must decode");
        let positive = cases
            .iter()
            .find(|case| case.id == "valid-v2")
            .expect("the positive case is published");
        let conflict = cases
            .iter()
            .find(|case| case.id == "losing-conflict")
            .expect("the conflict case is published");
        let graph = cases
            .iter()
            .find(|case| case.id == "graph-cycle")
            .expect("the cycle case is published");

        assert_eq!(
            render_published_fixture(positive).parwana_distributes,
            "consignment-v2"
        );
        assert_eq!(
            render_published_fixture(conflict).parwana_distributes,
            "nothing"
        );
        assert_eq!(
            render_published_fixture(graph).parwana_distributes,
            "transition-vector-ref"
        );

        // A case that ships nothing still owes a reader an actionable answer.
        match &conflict.material {
            PublishedConformanceMaterial::None {
                consumer_must_supply,
                ..
            } => assert!(!consumer_must_supply.is_empty()),
            other => panic!("losing-conflict distributes {other:?}"),
        }
    }
}
