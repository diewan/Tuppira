/// Chains status page showing indexer status for all supported chains.
use dioxus::prelude::*;

#[component]
pub fn Chains() -> Element {
    let mut chains: Signal<Option<Vec<csv_explorer_shared::ChainInfo>>> = use_signal(|| None);

    use_effect(move || {
        spawn(async move {
            // Fetch chain status from API
            match fetch_chains().await {
                Ok(chain_list) => chains.set(Some(chain_list)),
                Err(e) => {
                    tracing::error!("Failed to fetch chains: {}", e);
                    chains.set(None);
                }
            }
        });
    });

    rsx! {
        div { class: "space-y-8",
            // Header
            div { class: "flex items-center justify-between",
                h1 { class: "text-2xl font-bold", "Chain Status" }
                div { class: "flex items-center gap-2",
                    span { class: "w-2 h-2 rounded-full bg-green-500 animate-pulse" }
                    span { class: "text-sm text-gray-400", "Indexer Running" }
                }
            }

            // Chain cards
            if let Some(chain_list) = chains.read().as_ref() {
                div { class: "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6",
                    {chain_list.iter().map(|chain| rsx! {
                        ChainCard {
                            key: "{chain.id}",
                            id: chain.id.clone(),
                            name: chain.name.clone(),
                            network: chain.network.to_string(),
                            status: chain.status.to_string(),
                            latest_block: chain.latest_block,
                            latest_slot: chain.latest_slot,
                            rpc_url: chain.rpc_url.clone(),
                            sync_lag: chain.sync_lag,
                        }
                    })}
                }
            } else {
                // Loading state with skeleton cards
                div { class: "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6",
                    {vec!["Bitcoin", "Ethereum", "Sui", "Aptos", "Solana"].iter().map(|name| rsx! {
                        div { key: "{name}", class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                            div { class: "animate-pulse space-y-4",
                                div { class: "flex items-center gap-2",
                                    div { class: "w-2 h-2 rounded-full bg-gray-800" }
                                    div { class: "h-6 bg-gray-800 rounded w-24" }
                                }
                                div { class: "space-y-2",
                                    div { class: "h-4 bg-gray-800 rounded w-full" }
                                    div { class: "h-4 bg-gray-800 rounded w-3/4" }
                                    div { class: "h-4 bg-gray-800 rounded w-1/2" }
                                }
                            }
                        }
                    })}
                }
            }

            // Chain information
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                h2 { class: "text-lg font-semibold mb-4", "Supported Chains" }
                div { class: "grid grid-cols-1 md:grid-cols-2 gap-6",
                    ChainInfo {
                        name: "Bitcoin",
                        seal_type: "UTXO/Tapret",
                        description: "Bitcoin UTXO-based seals using Taproot commitments and Tapret proofs.",
                        features: vec!["OP_RETURN commitments", "Tapret proofs", "UTXO sealing"],
                    }
                    ChainInfo {
                        name: "Ethereum",
                        seal_type: "Account/Nullifier",
                        description: "Ethereum account-based seals with smart contract nullifier registry.",
                        features: vec!["Smart contract events", "Nullifier registry", "Account sealing"],
                    }
                    ChainInfo {
                        name: "Sui",
                        seal_type: "Object",
                        description: "Sui object-based seals using native object model and Move smart contracts.",
                        features: vec!["Object creation/deletion", "Move events", "Native object sealing"],
                    }
                    ChainInfo {
                        name: "Aptos",
                        seal_type: "Resource/Nullifier",
                        description: "Aptos resource-based seals with Move module integration.",
                        features: vec!["Resource changes", "Move events", "Resource sealing"],
                    }
                    ChainInfo {
                        name: "Solana",
                        seal_type: "Account",
                        description: "Solana account-based seals using PDAs and account state tracking.",
                        features: vec!["Account state changes", "Transaction logs", "PDA sealing"],
                    }
                }
            }
        }
    }
}

#[component]
fn ChainCard(
    id: String,
    name: String,
    network: String,
    status: String,
    latest_block: u64,
    latest_slot: Option<u64>,
    rpc_url: String,
    sync_lag: u64,
) -> Element {
    let status_color = match status.to_lowercase().as_str() {
        "synced" => "bg-green-500",
        "syncing" => "bg-yellow-500",
        "stopped" => "bg-gray-500",
        "error" => "bg-red-500",
        _ => "bg-gray-500",
    };

    rsx! {
        div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6 hover:bg-gray-800/30 transition-colors",
            div { class: "flex items-center justify-between mb-4",
                div { class: "flex items-center gap-2",
                    span { class: "w-3 h-3 rounded-full {status_color}" }
                    h3 { class: "text-lg font-semibold", "{name}" }
                }
                NetworkBadge { network }
            }

            div { class: "space-y-3 text-sm",
                div { class: "flex justify-between",
                    span { class: "text-gray-500", "Status" }
                    span { class: "text-gray-300 capitalize", "{status}" }
                }
                div { class: "flex justify-between",
                    span { class: "text-gray-500", "Latest Block" }
                    span { class: "text-gray-300 font-mono", "{latest_block}" }
                }
                if let Some(slot) = latest_slot {
                    div { class: "flex justify-between",
                        span { class: "text-gray-500", "Latest Slot" }
                        span { class: "text-gray-300 font-mono", "{slot}" }
                    }
                }
                div { class: "flex justify-between",
                    span { class: "text-gray-500", "Sync Lag" }
                    span { class: "{sync_lag_class(sync_lag)}",
                        "{sync_lag} blocks"
                    }
                }
            }

            if sync_lag > 10 {
                div { class: "mt-4 pt-4 border-t border-gray-800",
                    div { class: "text-xs text-yellow-400",
                        "⚠ High sync lag detected"
                    }
                }
            }
        }
    }
}

#[component]
fn NetworkBadge(network: String) -> Element {
    let color_class = match network.to_lowercase().as_str() {
        "mainnet" => "bg-green-500/20 text-green-400 border-green-500/30",
        "testnet" => "bg-blue-500/20 text-blue-400 border-blue-500/30",
        "devnet" => "bg-purple-500/20 text-purple-400 border-purple-500/30",
        _ => "bg-gray-800 text-gray-400 border-gray-700",
    };

    rsx! {
        span { class: "px-2 py-1 rounded-full text-xs font-medium border {color_class}",
            "{network}"
        }
    }
}

#[component]
fn ChainInfo(
    name: &'static str,
    seal_type: &'static str,
    description: &'static str,
    features: Vec<&'static str>,
) -> Element {
    rsx! {
        div { class: "space-y-3",
            div { class: "flex items-center justify-between",
                h3 { class: "font-semibold", "{name}" }
                span { class: "text-xs text-gray-500", "{seal_type}" }
            }
            p { class: "text-sm text-gray-400", "{description}" }
            div { class: "flex flex-wrap gap-2",
                {features.iter().map(|feature| rsx! {
                    span { key: "{feature}", class: "px-2 py-1 bg-gray-800 rounded text-xs text-gray-300",
                        "{feature}"
                    }
                })}
            }
        }
    }
}

fn sync_lag_class(sync_lag: u64) -> &'static str {
    if sync_lag == 0 {
        "text-green-400"
    } else {
        "text-yellow-400"
    }
}

/// Fetch chain information from the API
async fn fetch_chains() -> Result<Vec<csv_explorer_shared::ChainInfo>, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("/api/chains")
        .send()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }
    
    response
        .json::<Vec<csv_explorer_shared::ChainInfo>>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}
