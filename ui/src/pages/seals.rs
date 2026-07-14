use csv_explorer_shared::SealRecord;
/// Seals list page with filtering and pagination.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;
use crate::components::ChainBadge;
use crate::hooks::use_api::ApiClient;

#[component]
pub fn SealsList() -> Element {
    let mut chain_filter = use_signal(|| String::new());
    let mut type_filter = use_signal(|| String::new());
    let mut status_filter = use_signal(|| String::new());
    let mut page = use_signal(|| 1u64);
    let mut seals = use_signal(|| Vec::<SealRecord>::new());
    let mut loading = use_signal(|| false);

    let limit = 20;

    use_effect(move || {
        spawn(async move {
            loading.set(true);
            let client = ApiClient::new();
            let chain_str = chain_filter.read().clone();
            let type_str = type_filter.read().clone();
            let status_str = status_filter.read().clone();
            let chain = if chain_str.is_empty() {
                None
            } else {
                Some(chain_str.as_str())
            };
            let status = if status_str.is_empty() {
                None
            } else {
                Some(status_str.as_str())
            };
            let offset = ((*page.read() - 1) * limit as u64) as usize;

            if let Ok(records) = client
                .get_seals(chain, status, Some(limit), Some(offset))
                .await
            {
                // Filter by type client-side if needed
                let filtered = if type_str.is_empty() {
                    records
                } else {
                    records
                        .into_iter()
                        .filter(|s| s.seal_type.to_string() == type_str)
                        .collect()
                };
                seals.set(filtered);
            }
            loading.set(false);
        });
    });

    rsx! {
        div { class: "space-y-6",
            div { class: "flex items-center justify-between",
                h1 { class: "text-2xl font-bold text-gray-100", "Seals" }
                span { class: "text-gray-400 text-sm",
                    if loading() { "Loading..." } else { "{seals.read().len()} seals found" }
                }
            }

            // Filters
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-4",
                div { class: "flex flex-wrap gap-4",
                    select {
                        class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        onchange: move |evt| chain_filter.set(evt.value()),
                        option { value: "", "All Chains" }
                        option { value: "bitcoin", "Bitcoin" }
                        option { value: "ethereum", "Ethereum" }
                        option { value: "sui", "Sui" }
                        option { value: "aptos", "Aptos" }
                        option { value: "solana", "Solana" }
                    }
                    select {
                        class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        onchange: move |evt| type_filter.set(evt.value()),
                        option { value: "", "All Types" }
                        option { value: "utxo", "UTXO" }
                        option { value: "object", "Object" }
                        option { value: "resource", "Resource" }
                        option { value: "nullifier", "Nullifier" }
                        option { value: "account", "Account" }
                    }
                    select {
                        class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        onchange: move |evt| status_filter.set(evt.value()),
                        option { value: "", "All Statuses" }
                        option { value: "available", "Available" }
                        option { value: "consumed", "Consumed" }
                    }
                }
            }

            // Table
            div { class: "bg-gray-900 rounded-xl border border-gray-800 overflow-hidden",
                table { class: "w-full",
                    thead {
                        tr { class: "border-b border-gray-800 text-left text-sm text-gray-400",
                            th { class: "px-6 py-3 font-medium", "Seal ID" }
                            th { class: "px-6 py-3 font-medium", "Chain" }
                            th { class: "px-6 py-3 font-medium", "Type" }
                            th { class: "px-6 py-3 font-medium", "Status" }
                            th { class: "px-6 py-3 font-medium", "Sanad ID" }
                            th { class: "px-6 py-3 font-medium", "Block" }
                        }
                    }
                    tbody { class: "divide-y divide-gray-800",
                        if loading() {
                            tr {
                                td { colspan: 6, class: "px-6 py-12 text-center text-gray-500",
                                    "Loading seals..."
                                }
                            }
                        } else if seals.read().is_empty() {
                            tr {
                                td { colspan: 6, class: "px-6 py-12 text-center text-gray-500",
                                    "No seals found. Start the indexer to begin syncing data."
                                }
                            }
                        } else {
                            for seal in seals.read().clone() {
                                SealRow {
                                    key: "{seal.id}",
                                    id: seal.id.clone(),
                                    chain: seal.chain.clone(),
                                    seal_type: seal.seal_type.to_string(),
                                    status: seal.status.to_string(),
                                    sanad_id: seal.sanad_id.clone(),
                                    block_height: seal.block_height,
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
fn SealRow(
    id: String,
    chain: String,
    seal_type: String,
    status: String,
    sanad_id: Option<String>,
    block_height: u64,
) -> Element {
    rsx! {
        tr { class: "hover:bg-gray-800/50 transition-colors",
            td { class: "px-6 py-4",
                Link { to: Route::SealDetail { id: id.clone() },
                    class: "font-mono text-sm text-blue-400 hover:text-blue-300",
                    "{id}"
                }
            }
            td { class: "px-6 py-4",
                ChainBadge { chain }
            }
            td { class: "px-6 py-4",
                span { class: "px-2 py-1 rounded-full text-xs font-medium bg-gray-800 text-gray-300",
                    "{seal_type}"
                }
            }
            td { class: "px-6 py-4",
                crate::components::status_badge::StatusBadge { status }
            }
            td { class: "px-6 py-4 font-mono text-sm text-gray-300",
                if let Some(rid) = sanad_id {
                    Link { to: Route::SanadDetail { id: rid.clone() },
                        class: "text-blue-400 hover:text-blue-300",
                        "{rid}"
                    }
                } else {
                    span { class: "text-gray-600", "—" }
                }
            }
            td { class: "px-6 py-4 text-sm text-gray-400",
                "{block_height}"
            }
        }
    }
}
