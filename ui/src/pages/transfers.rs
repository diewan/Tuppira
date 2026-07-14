use csv_explorer_shared::TransferRecord;
/// Transfers list page with filtering and pagination.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;
use crate::components::ChainBadge;
use crate::hooks::use_api::ApiClient;

#[component]
pub fn TransfersList() -> Element {
    let mut from_chain_filter = use_signal(|| String::new());
    let mut to_chain_filter = use_signal(|| String::new());
    let mut status_filter = use_signal(|| String::new());
    let mut page = use_signal(|| 1u64);
    let mut transfers = use_signal(|| Vec::<TransferRecord>::new());
    let mut loading = use_signal(|| false);

    let limit = 20;

    use_effect(move || {
        spawn(async move {
            loading.set(true);
            let client = ApiClient::new();
            let from_chain_str = from_chain_filter.read().clone();
            let to_chain_str = to_chain_filter.read().clone();
            let status_str = status_filter.read().clone();
            let from_chain = if from_chain_str.is_empty() {
                None
            } else {
                Some(from_chain_str.as_str())
            };
            let to_chain = if to_chain_str.is_empty() {
                None
            } else {
                Some(to_chain_str.as_str())
            };
            let status = if status_str.is_empty() {
                None
            } else {
                Some(status_str.as_str())
            };
            let offset = ((*page.read() - 1) * limit as u64) as usize;

            if let Ok(records) = client
                .get_transfers(
                    None,
                    from_chain,
                    to_chain,
                    status,
                    Some(limit),
                    Some(offset),
                )
                .await
            {
                transfers.set(records);
            }
            loading.set(false);
        });
    });

    rsx! {
        div { class: "space-y-6",
            div { class: "flex items-center justify-between",
                h1 { class: "text-2xl font-bold text-gray-100", "Cross-Chain Transfers" }
                span { class: "text-gray-400 text-sm",
                    if loading() { "Loading..." } else { "{transfers.read().len()} transfers found" }
                }
            }

            // Filters
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-4",
                div { class: "flex flex-wrap gap-4",
                    select {
                        class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        onchange: move |evt| from_chain_filter.set(evt.value()),
                        option { value: "", "All Source Chains" }
                        option { value: "bitcoin", "Bitcoin" }
                        option { value: "ethereum", "Ethereum" }
                        option { value: "sui", "Sui" }
                        option { value: "aptos", "Aptos" }
                        option { value: "solana", "Solana" }
                    }
                    select {
                        class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        onchange: move |evt| to_chain_filter.set(evt.value()),
                        option { value: "", "All Destination Chains" }
                        option { value: "bitcoin", "Bitcoin" }
                        option { value: "ethereum", "Ethereum" }
                        option { value: "sui", "Sui" }
                        option { value: "aptos", "Aptos" }
                        option { value: "solana", "Solana" }
                    }
                    select {
                        class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        onchange: move |evt| status_filter.set(evt.value()),
                        option { value: "", "All Statuses" }
                        option { value: "pending", "Pending" }
                        option { value: "in_progress", "In Progress" }
                        option { value: "completed", "Completed" }
                        option { value: "failed", "Failed" }
                    }
                }
            }

            // Table
            div { class: "bg-gray-900 rounded-xl border border-gray-800 overflow-hidden",
                table { class: "w-full",
                    thead {
                        tr { class: "border-b border-gray-800 text-left text-sm text-gray-400",
                            th { class: "px-6 py-3 font-medium", "Transfer ID" }
                            th { class: "px-6 py-3 font-medium", "Route" }
                            th { class: "px-6 py-3 font-medium", "Sanad ID" }
                            th { class: "px-6 py-3 font-medium", "Status" }
                            th { class: "px-6 py-3 font-medium", "Lock TX" }
                            th { class: "px-6 py-3 font-medium", "Created" }
                        }
                    }
                    tbody { class: "divide-y divide-gray-800",
                        if loading() {
                            tr {
                                td { colspan: 6, class: "px-6 py-12 text-center text-gray-500",
                                    "Loading transfers..."
                                }
                            }
                        } else if transfers.read().is_empty() {
                            tr {
                                td { colspan: 6, class: "px-6 py-12 text-center text-gray-500",
                                    "No transfers found. Start the indexer to begin syncing data."
                                }
                            }
                        } else {
                            for transfer in transfers.read().clone() {
                                TransferRow {
                                    key: "{transfer.id}",
                                    id: transfer.id.clone(),
                                    from_chain: transfer.from_chain.clone(),
                                    to_chain: transfer.to_chain.clone(),
                                    sanad_id: transfer.sanad_id.clone(),
                                    status: transfer.status.to_string(),
                                    lock_tx: transfer.lock_tx.clone(),
                                    created_at: transfer.created_at,
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
fn TransferRow(
    id: String,
    from_chain: String,
    to_chain: String,
    sanad_id: String,
    status: String,
    lock_tx: String,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Element {
    rsx! {
        tr { class: "hover:bg-gray-800/50 transition-colors",
            td { class: "px-6 py-4",
                Link { to: Route::TransferDetail { id: id.clone() },
                    class: "font-mono text-sm text-blue-400 hover:text-blue-300",
                    "{id}"
                }
            }
            td { class: "px-6 py-4",
                div { class: "flex items-center gap-2",
                    ChainBadge { chain: from_chain.clone() }
                    span { class: "text-gray-500", "→" }
                    ChainBadge { chain: to_chain.clone() }
                }
            }
            td { class: "px-6 py-4 font-mono text-sm text-gray-300",
                "{sanad_id}"
            }
            td { class: "px-6 py-4",
                crate::components::status_badge::StatusBadge { status }
            }
            td { class: "px-6 py-4 font-mono text-sm text-gray-400",
                "{lock_tx}"
            }
            td { class: "px-6 py-4 text-sm text-gray-400",
                "{format_datetime(created_at)}"
            }
        }
    }
}

fn format_datetime(dt: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let diff = (now - dt).num_seconds();
    if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}
