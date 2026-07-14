use csv_explorer_shared::SanadRecord;
/// Sanads list page with filtering, sorting, and pagination.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;
use crate::components::ChainBadge;
use crate::hooks::use_api::ApiClient;

#[component]
pub fn SanadsList() -> Element {
    let mut chain_filter = use_signal(|| String::new());
    let mut status_filter = use_signal(|| String::new());
    let mut search_query = use_signal(|| String::new());
    let mut page = use_signal(|| 1u64);
    let mut sanads = use_signal(|| Vec::<SanadRecord>::new());
    let mut loading = use_signal(|| false);

    let limit = 20;

    // Fetch sanads when component mounts or filters change
    use_effect(move || {
        spawn(async move {
            loading.set(true);
            let client = ApiClient::new();
            let chain_str = chain_filter.read().clone();
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
                .get_sanads(chain, status, Some(limit), Some(offset))
                .await
            {
                sanads.set(records);
            }
            loading.set(false);
        });
    });

    rsx! {
        div { class: "space-y-6",
            // Header
            div { class: "flex items-center justify-between",
                h1 { class: "text-2xl font-bold text-gray-100", "Sanads" }
                span { class: "text-gray-400 text-sm",
                    if loading() { "Loading..." } else { "{sanads.read().len()} sanads found" }
                }
            }

            // Filters
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-4",
                div { class: "flex flex-wrap gap-4",
                    // Search
                    div { class: "flex-1 min-w-[200px]",
                        input {
                            r#type: "text",
                            value: "{search_query.read()}",
                            oninput: move |evt| search_query.set(evt.value()),
                            r#type: "text",
                            class: "w-full bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                        }
                    }
                    // Chain filter
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
                    // Status filter
                    select {
                        class: "bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-sm",
                        onchange: move |evt| status_filter.set(evt.value()),
                        option { value: "", "All Statuses" }
                        option { value: "active", "Active" }
                        option { value: "spent", "Spent" }
                        option { value: "pending", "Pending" }
                    }
                }
            }

            // Table
            div { class: "bg-gray-900 rounded-xl border border-gray-800 overflow-hidden",
                table { class: "w-full",
                    thead {
                        tr { class: "border-b border-gray-800 text-left text-sm text-gray-400",
                            th { class: "px-6 py-3 font-medium", "Sanad ID" }
                            th { class: "px-6 py-3 font-medium", "Chain" }
                            th { class: "px-6 py-3 font-medium", "Owner" }
                            th { class: "px-6 py-3 font-medium", "Status" }
                            th { class: "px-6 py-3 font-medium", "Created" }
                            th { class: "px-6 py-3 font-medium", "Transfers" }
                        }
                    }
                    tbody { class: "divide-y divide-gray-800",
                        if loading() {
                            tr {
                                td { colspan: 6, class: "px-6 py-12 text-center text-gray-500",
                                    "Loading sanads..."
                                }
                            }
                        } else if sanads.read().is_empty() {
                            tr {
                                td { colspan: 6, class: "px-6 py-12 text-center text-gray-500",
                                    "No sanads found. Start the indexer to begin syncing data."
                                }
                            }
                        } else {
                            for sanad in sanads.read().clone() {
                                SanadRow {
                                    key: "{sanad.id}",
                                    id: sanad.id.clone(),
                                    chain: sanad.chain.clone(),
                                    owner: sanad.owner.clone(),
                                    status: sanad.status.to_string(),
                                    created_at: sanad.created_at,
                                    transfer_count: sanad.transfer_count,
                                }
                            }
                        }
                    }
                }
            }

            // Pagination
            div { class: "flex items-center justify-between",
                button {
                    onclick: move |_| {
                        let current_page = *page.read();
                        if current_page > 1 {
                            page.set(current_page - 1);
                        }
                    },
                    disabled: *page.read() <= 1,
                    class: "px-4 py-2 rounded-lg bg-gray-800 hover:bg-gray-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors",
                    "Previous"
                }
                span { class: "text-gray-400", "Page {*page.read()}" }
                button {
                    onclick: move |_| {
                        let current_page = *page.read();
                        page.set(current_page + 1);
                    },
                    class: "px-4 py-2 rounded-lg bg-gray-800 hover:bg-gray-700 transition-colors",
                    "Next"
                }
            }
        }
    }
}

#[component]
fn SanadRow(
    id: String,
    chain: String,
    owner: String,
    status: String,
    created_at: chrono::DateTime<chrono::Utc>,
    transfer_count: u64,
) -> Element {
    rsx! {
        tr { class: "hover:bg-gray-800/50 transition-colors",
            td { class: "px-6 py-4",
                Link {
                    to: Route::SanadDetail { id: id.clone() },
                    class: "font-mono text-sm text-blue-400 hover:text-blue-300",
                    "{id}"
                }
            }
            td { class: "px-6 py-4",
                ChainBadge { chain }
            }
            td { class: "px-6 py-4 font-mono text-sm text-gray-300",
                "{owner}"
            }
            td { class: "px-6 py-4",
                crate::components::status_badge::StatusBadge { status }
            }
            td { class: "px-6 py-4 text-sm text-gray-400",
                "{format_datetime(created_at)}"
            }
            td { class: "px-6 py-4 text-sm text-gray-300",
                "{transfer_count}"
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
