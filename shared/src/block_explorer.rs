//! Official block-explorer link construction for the chains Parwana touches.
//!
//! Tuppira is a Parwana-scoped indexer, not an all-in-one block explorer. For
//! any visualization deeper than the protocol data it indexes, it points users
//! to each chain's *official* explorer instead of reproducing it. These helpers
//! build those links and are network-aware: they return `None` when no public
//! explorer exists for a `(chain, network)` pair (for example a local devnet or
//! Bitcoin regtest), so callers render an explicit "no link" state rather than a
//! fabricated URL.
//!
//! The module is dependency-light and `wasm32`-safe on purpose: the same builder
//! is meant to be used by the indexer, the API, and the Hemion UI.

use crate::types::Network;

/// The kind of on-chain resource a link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resource {
    /// A transaction (hash / digest / signature).
    Tx,
    /// An externally-owned account or address.
    Address,
    /// A contract, program, package, or module.
    Contract,
}

/// Build an official block-explorer URL for `id` of `resource` kind on `chain`
/// at the given `network`. Returns `None` for unknown chains or networks with no
/// public explorer.
fn build(chain: &str, network: Network, resource: Resource, id: &str) -> Option<String> {
    match chain.to_ascii_lowercase().as_str() {
        "bitcoin" => {
            // blockstream.info; regtest/devnet has no public explorer.
            let base = match network {
                Network::Mainnet => "https://blockstream.info",
                Network::Testnet => "https://blockstream.info/testnet",
                Network::Devnet => return None,
            };
            match resource {
                Resource::Tx => Some(format!("{base}/tx/{id}")),
                Resource::Address => Some(format!("{base}/address/{id}")),
                // Bitcoin has no smart contracts.
                Resource::Contract => None,
            }
        }
        "ethereum" => {
            let base = match network {
                Network::Mainnet => "https://etherscan.io",
                Network::Testnet => "https://sepolia.etherscan.io",
                Network::Devnet => return None,
            };
            match resource {
                Resource::Tx => Some(format!("{base}/tx/{id}")),
                // On Etherscan a contract is just an address page.
                Resource::Address | Resource::Contract => Some(format!("{base}/address/{id}")),
            }
        }
        "solana" => {
            // A single host, cluster selected by query parameter.
            let cluster = match network {
                Network::Mainnet => "",
                Network::Testnet => "?cluster=testnet",
                Network::Devnet => "?cluster=devnet",
            };
            let base = "https://explorer.solana.com";
            match resource {
                Resource::Tx => Some(format!("{base}/tx/{id}{cluster}")),
                // Programs and accounts share the /address route.
                Resource::Address | Resource::Contract => {
                    Some(format!("{base}/address/{id}{cluster}"))
                }
            }
        }
        "sui" => {
            let segment = match network {
                Network::Mainnet => "mainnet",
                Network::Testnet => "testnet",
                Network::Devnet => "devnet",
            };
            let base = format!("https://suiscan.xyz/{segment}");
            match resource {
                Resource::Tx => Some(format!("{base}/tx/{id}")),
                Resource::Address => Some(format!("{base}/account/{id}")),
                // Packages and objects live under /object.
                Resource::Contract => Some(format!("{base}/object/{id}")),
            }
        }
        "aptos" => {
            let query = match network {
                Network::Mainnet => "?network=mainnet",
                Network::Testnet => "?network=testnet",
                Network::Devnet => "?network=devnet",
            };
            let base = "https://explorer.aptoslabs.com";
            match resource {
                Resource::Tx => Some(format!("{base}/txn/{id}{query}")),
                // A module is addressed by its owning account.
                Resource::Address | Resource::Contract => {
                    Some(format!("{base}/account/{id}{query}"))
                }
            }
        }
        _ => None,
    }
}

/// Official explorer URL for a transaction on `chain` at `network`.
pub fn tx_url(chain: &str, network: Network, tx_hash: &str) -> Option<String> {
    build(chain, network, Resource::Tx, tx_hash)
}

/// Official explorer URL for an account/address on `chain` at `network`.
pub fn address_url(chain: &str, network: Network, address: &str) -> Option<String> {
    build(chain, network, Resource::Address, address)
}

/// Official explorer URL for a contract/program/package on `chain` at `network`.
pub fn contract_url(chain: &str, network: Network, address: &str) -> Option<String> {
    build(chain, network, Resource::Contract, address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ethereum_is_network_aware() {
        assert_eq!(
            tx_url("ethereum", Network::Mainnet, "0xabc"),
            Some("https://etherscan.io/tx/0xabc".to_string())
        );
        assert_eq!(
            tx_url("ethereum", Network::Testnet, "0xabc"),
            Some("https://sepolia.etherscan.io/tx/0xabc".to_string())
        );
        // No hardcoded testnet fallback: a local devnet has no public explorer.
        assert_eq!(tx_url("ethereum", Network::Devnet, "0xabc"), None);
    }

    #[test]
    fn chain_casing_is_ignored() {
        assert!(tx_url("Ethereum", Network::Mainnet, "0xabc").is_some());
    }

    #[test]
    fn unknown_chain_has_no_link() {
        assert_eq!(tx_url("dogecoin", Network::Mainnet, "abc"), None);
    }

    #[test]
    fn bitcoin_has_no_contract_link() {
        assert_eq!(contract_url("bitcoin", Network::Mainnet, "abc"), None);
        assert_eq!(
            address_url("bitcoin", Network::Mainnet, "bc1xyz"),
            Some("https://blockstream.info/address/bc1xyz".to_string())
        );
    }

    #[test]
    fn solana_cluster_query() {
        assert_eq!(
            tx_url("solana", Network::Devnet, "sig1"),
            Some("https://explorer.solana.com/tx/sig1?cluster=devnet".to_string())
        );
    }
}
