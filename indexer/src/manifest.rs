//! Deployment-manifest admission gate for canonical chain decoding.
//!
//! The Explorer never guesses a contract, program, package, or module ID. A
//! chain may be enabled only after its signed deployment manifest entry has
//! been checked against the operator's selected network.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tuppira_shared::{ChainConfig, Network, TuppiraError};

#[derive(Debug, Deserialize)]
struct DeploymentManifest {
    #[serde(default)]
    signature: Option<String>,
    deployments: HashMap<String, Deployment>,
}

#[derive(Debug, Deserialize)]
struct Deployment {
    network: String,
    #[serde(default)]
    contracts: Vec<Contract>,
    program_id: Option<String>,
    package_id: Option<String>,
    module_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Contract {
    address: String,
    abi_hash: Option<String>,
}

pub fn validate_enabled_chains(
    path: &Path,
    chains: &HashMap<String, ChainConfig>,
) -> Result<(), TuppiraError> {
    if !chains.values().any(|chain| chain.enabled) {
        return Ok(());
    }

    let source = std::fs::read_to_string(path).map_err(TuppiraError::Io)?;
    let manifest: DeploymentManifest = serde_json::from_str(&source)?;
    if manifest
        .signature
        .as_deref()
        .filter(|signature| !signature.is_empty())
        .is_none()
    {
        return Err(TuppiraError::Parse(
            "deployment manifest is unsigned; enabled indexing is refused".to_string(),
        ));
    }

    for (chain_id, chain) in chains.iter().filter(|(_, chain)| chain.enabled) {
        let deployment = manifest.deployments.get(chain_id).ok_or_else(|| {
            TuppiraError::Parse(format!("missing deployment manifest entry for {chain_id}"))
        })?;
        if deployment.network != network_name(chain.network) {
            return Err(TuppiraError::Parse(format!(
                "deployment manifest network mismatch for {chain_id}"
            )));
        }
        validate_identifier(chain_id, deployment)?;
    }
    Ok(())
}

fn network_name(network: Network) -> &'static str {
    match network {
        Network::Mainnet => "mainnet",
        Network::Testnet => "testnet",
        Network::Devnet => "devnet",
    }
}

fn validate_identifier(chain_id: &str, deployment: &Deployment) -> Result<(), TuppiraError> {
    let valid = match chain_id {
        "ethereum" => deployment
            .contracts
            .iter()
            .any(|contract| !contract.address.is_empty() && contract.abi_hash.is_some()),
        "solana" => deployment
            .program_id
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
        "sui" => deployment
            .package_id
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
        "aptos" => deployment
            .module_address
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
        // Bitcoin has no contract deployment identity. Its canonical decoder
        // still needs an explicit manifest schema before it may be enabled.
        "bitcoin" => false,
        _ => false,
    };
    valid.then_some(()).ok_or_else(|| {
        TuppiraError::Parse(format!(
            "missing canonical deployment identifier for {chain_id}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::validate_enabled_chains;
    use std::collections::HashMap;
    use std::fs;
    use tuppira_shared::{ChainConfig, Network};

    #[test]
    fn unsigned_manifest_cannot_enable_a_chain() {
        let path =
            std::env::temp_dir().join(format!("unsigned-manifest-{}.json", std::process::id()));
        let write_result = fs::write(
            &path,
            r#"{"deployments":{"ethereum":{"network":"testnet","contracts":[]}}}"#,
        );
        assert!(write_result.is_ok());
        let mut chains = HashMap::new();
        chains.insert(
            "ethereum".to_string(),
            ChainConfig {
                enabled: true,
                network: Network::Testnet,
                rpc_url: "http://localhost".to_string(),
                start_block: None,
                poll_interval_ms: None,
                ..ChainConfig::default()
            },
        );
        let result = validate_enabled_chains(&path, &chains);
        let _ = fs::remove_file(path);
        assert!(
            matches!(result, Err(tuppira_shared::TuppiraError::Parse(message)) if message.contains("unsigned"))
        );
    }
}
