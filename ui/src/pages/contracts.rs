/// Contracts list page showing deployed CSV contracts across all chains.
use dioxus::prelude::*;

#[component]
pub fn ContractsList() -> Element {
    let mut contracts: Signal<Option<Vec<csv_explorer_shared::CsvContract>>> = use_signal(|| None);
    let _chain_filter = use_signal(|| String::new());
    let _contract_type_filter = use_signal(|| String::new());
    let _status_filter = use_signal(|| String::new());

    use_effect(move || {
        spawn(async move {
            // Fetch contracts from API
            match fetch_contracts().await {
                Ok(contract_list) => contracts.set(Some(contract_list)),
                Err(e) => {
                    tracing::error!("Failed to fetch contracts: {}", e);
                    contracts.set(None);
                }
            }
        });
    });

    rsx! {
        div { class: "space-y-6",
            // Header
            div { class: "flex items-center justify-between",
                h1 { class: "text-2xl font-bold", "CSV Contracts" }
                span { class: "text-gray-400 text-sm",
                    {contracts.with(|c| match c {
                        Some(contracts_list) => format!("{} contracts found", contracts_list.len()),
                        None => "Loading...".to_string(),
                    })}
                }
            }

            // Filters
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-4",
                div { class: "flex flex-wrap gap-4",
                    // Chain filter
                    select { class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        option { value: "", "All Chains" }
                        option { value: "bitcoin", "Bitcoin" }
                        option { value: "ethereum", "Ethereum" }
                        option { value: "sui", "Sui" }
                        option { value: "aptos", "Aptos" }
                        option { value: "solana", "Solana" }
                    }
                    // Contract type filter
                    select { class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        option { value: "", "All Types" }
                        option { value: "nullifier_registry", "Nullifier Registry" }
                        option { value: "state_commitment", "State Commitment" }
                        option { value: "sanad_registry", "Sanad Registry" }
                        option { value: "bridge", "Bridge/Transfer" }
                        option { value: "other", "Other" }
                    }
                    // Status filter
                    select { class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        option { value: "", "All Statuses" }
                        option { value: "active", "Active" }
                        option { value: "deprecated", "Deprecated" }
                        option { value: "error", "Error" }
                    }
                }
            }

            // Contracts table
            div { class: "bg-gray-900 rounded-xl border border-gray-800 overflow-hidden",
                table { class: "w-full",
                    thead {
                        tr { class: "border-b border-gray-800 text-left text-sm text-gray-400",
                            th { class: "px-6 py-3 font-medium", "Contract ID" }
                            th { class: "px-6 py-3 font-medium", "Chain" }
                            th { class: "px-6 py-3 font-medium", "Type" }
                            th { class: "px-6 py-3 font-medium", "Address" }
                            th { class: "px-6 py-3 font-medium", "Version" }
                            th { class: "px-6 py-3 font-medium", "Status" }
                            th { class: "px-6 py-3 font-medium", "Deployed" }
                        }
                    }
                    tbody { class: "divide-y divide-gray-800",
                        if let Some(contracts_list) = contracts.read().as_ref() {
                            {contracts_list.iter().map(|contract| rsx! {
                                ContractRow {
                                    key: "{contract.id}",
                                    id: contract.id.clone(),
                                    chain: contract.chain.clone(),
                                    contract_type: contract.contract_type.to_string(),
                                    address: contract.address.clone(),
                                    version: contract.version.clone(),
                                    status: contract.status.to_string(),
                                    deployed_at: contract.deployed_at.format("%Y-%m-%d %H:%M").to_string(),
                                }
                            })}
                        } else {
                            tr {
                                td { colspan: 7, class: "px-6 py-12 text-center text-gray-500",
                                    "Loading contracts..."
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ContractRow(
    id: String,
    chain: String,
    contract_type: String,
    address: String,
    version: String,
    status: String,
    deployed_at: String,
) -> Element {
    rsx! {
        tr { class: "hover:bg-gray-800/50 transition-colors",
            td { class: "px-6 py-4 font-mono text-sm text-blue-400 hover:text-blue-300",
                "{id}"
            }
            td { class: "px-6 py-4",
                ChainBadge { chain }
            }
            td { class: "px-6 py-4",
                span { class: "px-2 py-1 rounded-full text-xs font-medium bg-gray-800 text-gray-300",
                    "{contract_type}"
                }
            }
            td { class: "px-6 py-4 font-mono text-sm text-gray-300",
                "{address}"
            }
            td { class: "px-6 py-4 text-sm text-gray-400",
                "{version}"
            }
            td { class: "px-6 py-4",
                StatusBadge { status }
            }
            td { class: "px-6 py-4 text-sm text-gray-400",
                "{deployed_at}"
            }
        }
    }
}

#[component]
fn ChainBadge(chain: String) -> Element {
    let color_class = match chain.to_lowercase().as_str() {
        "bitcoin" => "bg-orange-500/20 text-orange-400 border-orange-500/30",
        "ethereum" => "bg-blue-500/20 text-blue-400 border-blue-500/30",
        "sui" => "bg-cyan-500/20 text-cyan-400 border-cyan-500/30",
        "aptos" => "bg-purple-500/20 text-purple-400 border-purple-500/30",
        "solana" => {
            "bg-gradient-to-r from-purple-500/20 to-green-500/20 text-green-400 border-green-500/30"
        }
        _ => "bg-gray-800 text-gray-400 border-gray-700",
    };

    rsx! {
        span { class: "px-2 py-1 rounded-full text-xs font-medium border {color_class}",
            "{chain}"
        }
    }
}

#[component]
fn StatusBadge(status: String) -> Element {
    let color_class = match status.to_lowercase().as_str() {
        "active" => "bg-green-500/20 text-green-400 border-green-500/30",
        "deprecated" => "bg-yellow-500/20 text-yellow-400 border-yellow-500/30",
        "error" => "bg-red-500/20 text-red-400 border-red-500/30",
        _ => "bg-gray-800 text-gray-400 border-gray-700",
    };

    rsx! {
        span { class: "px-2 py-1 rounded-full text-xs font-medium border {color_class}",
            "{status}"
        }
    }
}

/// Fetch contracts from the API
async fn fetch_contracts() -> Result<Vec<csv_explorer_shared::CsvContract>, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("/api/contracts")
        .send()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }
    
    response
        .json::<Vec<csv_explorer_shared::CsvContract>>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}
