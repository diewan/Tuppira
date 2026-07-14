use csv_explorer_shared::ExplorerStats;
/// Statistics dashboard with charts and aggregate data.
use dioxus::prelude::*;

use crate::hooks::use_api::ApiClient;

#[component]
pub fn Stats() -> Element {
    let mut stats: Signal<Option<ExplorerStats>> = use_signal(|| None);

    use_effect(move || {
        spawn(async move {
            let result = fetch_stats().await;
            stats.set(result);
        });
    });

    rsx! {
        div { class: "space-y-8",
            h1 { class: "text-2xl font-bold", "Statistics" }

            // Summary cards
            div { class: "grid grid-cols-1 md:grid-cols-4 gap-4",
                SummaryCard { label: "Total Sanads", value: stats.with(|s| s.as_ref().map(|s| s.total_sanads.to_string()).unwrap_or_else(|| "—".to_string())) }
                SummaryCard { label: "Total Transfers", value: stats.with(|s| s.as_ref().map(|s| s.total_transfers.to_string()).unwrap_or_else(|| "—".to_string())) }
                SummaryCard { label: "Total Seals", value: stats.with(|s| s.as_ref().map(|s| s.total_seals.to_string()).unwrap_or_else(|| "—".to_string())) }
                SummaryCard { label: "Total Contracts", value: stats.with(|s| s.as_ref().map(|s| s.total_contracts.to_string()).unwrap_or_else(|| "—".to_string())) }
            }

            // Transfer metrics
            div { class: "grid grid-cols-1 md:grid-cols-2 gap-6",
                // Success rate
                div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                    h2 { class: "text-lg font-semibold mb-4", "Transfer Success Rate" }
                    if let Some(ref s) = *stats.read() {
                        div { class: "text-center",
                            div { class: "text-5xl font-bold text-green-400 mb-2",
                                {format!("{:.1}%", s.transfer_success_rate)}
                            }
                            if let Some(avg_ms) = s.average_transfer_time_ms {
                                div { class: "text-gray-400 text-sm",
                                    "Average transfer time: {avg_ms}ms"
                                }
                            }
                        }
                    }
                }

                // Sanads by chain (bar chart area)
                div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                    h2 { class: "text-lg font-semibold mb-4", "Sanads by Chain" }
                    if let Some(ref s) = *stats.read() {
                        div { class: "space-y-3",
                            {s.sanads_by_chain.iter().map(|rc| rsx! {
                                ChainBar {
                                    key: "{rc.chain}",
                                    chain: rc.chain.clone(),
                                    count: rc.count,
                                    max: s.sanads_by_chain.first().map(|c| c.count).unwrap_or(1),
                                }
                            })}
                        }
                    } else {
                        div { class: "text-gray-500 text-center py-8", "Loading..." }
                    }
                }
            }

            // Transfers by chain pair
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                h2 { class: "text-lg font-semibold mb-4", "Transfers by Chain Pair" }
                if let Some(ref s) = *stats.read() {
                    if s.transfers_by_chain_pair.is_empty() {
                        div { class: "text-gray-500 text-center py-8", "No transfer data yet" }
                    } else {
                        table { class: "w-full",
                            thead {
                                tr { class: "border-b border-gray-800 text-left text-sm text-gray-400",
                                    th { class: "px-4 py-2", "From" }
                                    th { class: "px-4 py-2", "To" }
                                    th { class: "px-4 py-2 text-sanad", "Count" }
                                }
                            }
                            tbody { class: "divide-y divide-gray-800",
                                {s.transfers_by_chain_pair.iter().map(|tp| rsx! {
                                    tr { key: "{tp.from_chain}-{tp.to_chain}",
                                        td { class: "px-4 py-2",
                                            span { class: "px-2 py-1 rounded-full text-xs bg-gray-800", "{tp.from_chain}" }
                                        }
                                        td { class: "px-4 py-2",
                                            span { class: "px-2 py-1 rounded-full text-xs bg-gray-800", "{tp.to_chain}" }
                                        }
                                        td { class: "px-4 py-2 text-sanad font-mono", "{tp.count}" }
                                    }
                                })}
                            }
                        }
                    }
                } else {
                    div { class: "text-gray-500 text-center py-8", "Loading..." }
                }
            }

            // Active seals by chain
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                h2 { class: "text-lg font-semibold mb-4", "Active Seals by Chain" }
                if let Some(ref s) = *stats.read() {
                    div { class: "grid grid-cols-2 md:grid-cols-5 gap-4",
                        {s.active_seals_by_chain.iter().map(|sc| rsx! {
                            div { key: "{sc.chain}", class: "text-center",
                                div { class: "text-2xl font-bold", "{sc.count}" }
                                div { class: "text-sm text-gray-400", "{sc.chain}" }
                            }
                        })}
                    }
                }
            }
        }
    }
}

#[component]
fn SummaryCard(label: String, value: String) -> Element {
    rsx! {
        div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6 text-center",
            div { class: "text-3xl font-bold mb-1", "{value}" }
            div { class: "text-gray-400 text-sm", "{label}" }
        }
    }
}

#[component]
fn ChainBar(chain: String, count: u64, max: u64) -> Element {
    let width_pct = if max > 0 {
        (count as f64 / max as f64) * 100.0
    } else {
        0.0
    };
    rsx! {
        div {
            div { class: "flex items-center justify-between text-sm mb-1",
                span { "{chain}" }
                span { class: "text-gray-400", "{count}" }
            }
            div { class: "w-full bg-gray-800 rounded-full h-2",
                div {
                    class: "bg-blue-500 h-2 rounded-full",
                    style: "width: {width_pct}%"
                }
            }
        }
    }
}

async fn fetch_stats() -> Option<ExplorerStats> {
    None
}
