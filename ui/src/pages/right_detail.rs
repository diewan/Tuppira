/// Sanad detail page showing full information about a CSV sanad.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;

#[component]
pub fn SanadDetail(id: String) -> Element {
    let mut sanad: Signal<Option<csv_explorer_shared::SanadRecord>> = use_signal(|| None);
    let mut transfers: Signal<Option<Vec<csv_explorer_shared::TransferRecord>>> =
        use_signal(|| None);
    let mut seals: Signal<Option<Vec<csv_explorer_shared::SealRecord>>> = use_signal(|| None);

    use_effect({
        let id = id.clone();
        move || {
            spawn(async move {
                // Data fetching from API to be implemented
                // Currently displays empty state
                sanad.set(None);
                transfers.set(None);
                seals.set(None);
            });
        }
    });

    rsx! {
        div { class: "space-y-8",
            // Breadcrumb
            div { class: "flex items-center gap-2 text-sm text-gray-500",
                Link { to: Route::SanadsList {}, class: "hover:text-gray-300", "Sanads" }
                span { "→" }
                span { class: "text-gray-300 font-mono", "{id}" }
            }

            // Header
            div { class: "flex items-center justify-between",
                h1 { class: "text-2xl font-bold", "Sanad Detail" }
                if let Some(ref r) = *sanad.read() {
                    StatusBadge { status: r.status.to_string() }
                }
            }

            if let Some(ref r) = *sanad.read() {
                // Main info card
                div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                    div { class: "grid grid-cols-1 md:grid-cols-2 gap-6",
                        DetailRow { label: "Sanad ID", value: r.id.clone() }
                        DetailRow { label: "Chain", value: r.chain.clone() }
                        DetailRow { label: "Owner", value: r.owner.clone() }
                        DetailRow { label: "Seal Reference", value: r.seal_ref.clone() }
                        DetailRow { label: "Commitment", value: r.commitment.clone() }
                        DetailRow { label: "Created", value: r.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string() }
                        DetailRow { label: "Transfer Count", value: r.transfer_count.to_string() }
                        if let Some(last_transfer) = r.last_transfer_at {
                            DetailRow { label: "Last Transfer", value: last_transfer.format("%Y-%m-%d %H:%M:%S UTC").to_string() }
                        }
                    }
                    if let Some(ref metadata) = r.metadata {
                        div { class: "mt-6 pt-6 border-t border-gray-800",
                            h3 { class: "text-sm font-semibold text-gray-400 mb-3", "Metadata" }
                            pre { class: "bg-gray-800 rounded-lg p-4 text-sm font-mono overflow-x-auto",
                                {serde_json::to_string_pretty(metadata).unwrap_or_default()}
                            }
                        }
                    }
                }

                // Transfers history
                div {
                    h2 { class: "text-lg font-semibold mb-4", "Transfer History" }
                    if let Some(ref txs) = *transfers.read() {
                        if txs.is_empty() {
                            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-8 text-center text-gray-500",
                                "No transfers for this sanad yet"
                            }
                        } else {
                            div { class: "bg-gray-900 rounded-xl border border-gray-800 overflow-hidden",
                                table { class: "w-full",
                                    thead {
                                        tr { class: "border-b border-gray-800 text-left text-sm text-gray-400",
                                            th { class: "px-6 py-3 font-medium", "Transfer ID" }
                                            th { class: "px-6 py-3 font-medium", "Route" }
                                            th { class: "px-6 py-3 font-medium", "Status" }
                                            th { class: "px-6 py-3 font-medium", "Created" }
                                        }
                                    }
                                    tbody { class: "divide-y divide-gray-800",
                                        {txs.iter().map(|tx| rsx! {
                                            tr { key: "{tx.id}", class: "hover:bg-gray-800/50",
                                                td { class: "px-6 py-4 font-mono text-sm",
                                                    Link { to: Route::TransferDetail { id: tx.id.clone() },
                                                        class: "text-blue-400 hover:text-blue-300",
                                                        "{tx.id}"
                                                    }
                                                }
                                                td { class: "px-6 py-4",
                                                    div { class: "flex items-center gap-2 text-sm",
                                                        span { class: "text-gray-300", "{tx.from_chain}" }
                                                        span { class: "text-gray-500", "→" }
                                                        span { class: "text-gray-300", "{tx.to_chain}" }
                                                    }
                                                }
                                                td { class: "px-6 py-4",
                                                    StatusBadge { status: tx.status.to_string() }
                                                }
                                                td { class: "px-6 py-4 text-sm text-gray-400",
                                                    {tx.created_at.format("%Y-%m-%d %H:%M").to_string()}
                                                }
                                            }
                                        })}
                                    }
                                }
                            }
                        }
                    } else {
                        div { class: "bg-gray-900 rounded-xl border border-gray-800 p-8 text-center text-gray-500",
                            "Loading transfers..."
                        }
                    }
                }

                // Seals
                div {
                    h2 { class: "text-lg font-semibold mb-4", "Associated Seals" }
                    if let Some(ref seals_list) = *seals.read() {
                        if seals_list.is_empty() {
                            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-8 text-center text-gray-500",
                                "No seals associated with this sanad"
                            }
                        } else {
                            div { class: "grid grid-cols-1 md:grid-cols-2 gap-4",
                                {seals_list.iter().map(|seal| rsx! {
                                    SealCard {
                                        key: "{seal.id}",
                                        id: seal.id.clone(),
                                        chain: seal.chain.clone(),
                                        seal_type: seal.seal_type.to_string(),
                                        status: seal.status.to_string(),
                                        block_height: seal.block_height,
                                    }
                                })}
                            }
                        }
                    } else {
                        div { class: "bg-gray-900 rounded-xl border border-gray-800 p-8 text-center text-gray-500",
                            "Loading seals..."
                        }
                    }
                }
            } else {
                // Loading state
                div { class: "bg-gray-900 rounded-xl border border-gray-800 p-12 text-center",
                    div { class: "animate-pulse space-y-4",
                        div { class: "h-8 bg-gray-800 rounded w-48 mx-auto" }
                        div { class: "h-4 bg-gray-800 rounded w-32 mx-auto" }
                    }
                }
            }
        }
    }
}

#[component]
fn DetailRow(label: String, value: String) -> Element {
    rsx! {
        div {
            div { class: "text-sm text-gray-500 mb-1", "{label}" }
            div { class: "font-mono text-sm text-gray-200 break-all", "{value}" }
        }
    }
}

#[component]
fn StatusBadge(status: String) -> Element {
    let color_class = match status.to_lowercase().as_str() {
        "active" => "bg-green-500/20 text-green-400 border-green-500/30",
        "spent" => "bg-red-500/20 text-red-400 border-red-500/30",
        "pending" => "bg-yellow-500/20 text-yellow-400 border-yellow-500/30",
        _ => "bg-gray-800 text-gray-400 border-gray-700",
    };

    rsx! {
        span { class: "px-3 py-1 rounded-full text-xs font-medium border {color_class}",
            "{status}"
        }
    }
}

#[component]
fn SealCard(
    id: String,
    chain: String,
    seal_type: String,
    status: String,
    block_height: u64,
) -> Element {
    rsx! {
        Link { to: Route::SealDetail { id: id.clone() },
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-4 hover:bg-gray-800/50 transition-colors",
                div { class: "flex items-center justify-between mb-3",
                    span { class: "font-mono text-sm text-blue-400 hover:text-blue-300", "{id}" }
                    StatusBadge { status }
                }
                div { class: "space-y-2 text-sm",
                    div { class: "flex justify-between",
                        span { class: "text-gray-500", "Chain" }
                        span { class: "text-gray-300", "{chain}" }
                    }
                    div { class: "flex justify-between",
                        span { class: "text-gray-500", "Seal Type" }
                        span { class: "text-gray-300", "{seal_type}" }
                    }
                    div { class: "flex justify-between",
                        span { class: "text-gray-500", "Block Height" }
                        span { class: "text-gray-300 font-mono", "{block_height}" }
                    }
                }
            }
        }
    }
}
