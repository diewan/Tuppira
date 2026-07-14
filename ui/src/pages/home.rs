use csv_explorer_shared::{ExplorerStats, SanadRecord, SealRecord, TransferRecord};
/// Home / landing page with stats, recent activity, and chain status.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;
use crate::components::StatCard;
use crate::hooks::use_api::ApiClient;

#[component]
pub fn Home() -> Element {
    let mut stats = use_signal(|| Option::<ExplorerStats>::None);
    let mut recent_sanads = use_signal(|| Vec::<SanadRecord>::new());
    let mut recent_transfers = use_signal(|| Vec::<TransferRecord>::new());
    let mut recent_seals = use_signal(|| Vec::<SealRecord>::new());
    let mut api_health = use_signal(|| "unknown".to_string());

    use_effect(move || {
        spawn(async move {
            let client = ApiClient::new();

            // Fetch stats
            if let Ok(s) = client.get_stats().await {
                stats.set(Some(s));
            }

            // Fetch recent sanads
            if let Ok(sanads) = client.get_sanads(None, None, Some(5), Some(0)).await {
                recent_sanads.set(sanads);
            }

            // Fetch recent transfers
            if let Ok(transfers) = client
                .get_transfers(None, None, None, None, Some(5), Some(0))
                .await
            {
                recent_transfers.set(transfers);
            }

            // Fetch recent seals
            if let Ok(seals) = client.get_seals(None, None, Some(5), Some(0)).await {
                recent_seals.set(seals);
            }

            // Check API health
            api_health.set(if client.health_check().await.unwrap_or(false) {
                "connected".to_string()
            } else {
                "disconnected".to_string()
            });
        });
    });

    rsx! {
        div { class: "space-y-8",
            // API Status Banner
            if api_health() == "disconnected" || api_health() == "unknown" {
                div { class: "bg-yellow-500/10 border border-yellow-500/30 rounded-xl p-6",
                    div { class: "flex items-start gap-3",
                        span { class: "text-yellow-400 text-2xl", "⚠" }
                        div { class: "flex-1",
                            h3 { class: "text-lg font-semibold text-yellow-400 mb-2",
                                "API Server Not Connected"
                            }
                            p { class: "text-gray-300 text-sm mb-3",
                                "The Explorer API (port 8080) is not running. Start the required services to view real blockchain data."
                            }
                            div { class: "bg-gray-900 rounded-lg p-4 font-mono text-xs text-gray-400 space-y-1",
                                p { class: "text-gray-500", "# Start the API server:" }
                                p { class: "text-green-400", "cd csv-explorer && cargo run -p csv-explorer-api -- start" }
                                p { class: "text-gray-500 mt-2", "# (Optional) Start the indexer to sync blockchain data:" }
                                p { class: "text-green-400", "cargo run -p csv-explorer-indexer -- start" }
                            }
                        }
                    }
                }
            }

            // Hero
            div { class: "text-center py-12",
                h1 { class: "text-4xl font-bold mb-4",
                    "Cross-Chain Sealed Verifiable Sanads Explorer"
                }
                p { class: "text-gray-400 text-lg max-w-2xl mx-auto",
                    "Track, search, and analyze CSV sanads, transfers, and seals across Bitcoin, Ethereum, Sui, Aptos, and Solana."
                }
            }

            // Stats cards
            div { class: "grid grid-cols-1 md:grid-cols-4 gap-4",
                StatCard {
                    label: "Total Sanads",
                    value: stats.with(|s| s.as_ref().map(|s| {
                        s.sanads_by_chain.iter().map(|c| c.count).sum::<u64>().to_string()
                    }).unwrap_or_else(|| "Loading...".to_string())),
                    icon: "◆"
                }
                StatCard {
                    label: "Total Transfers",
                    value: stats.with(|s| s.as_ref().map(|s| {
                        s.transfers_by_chain_pair.iter().map(|c| c.count).sum::<u64>().to_string()
                    }).unwrap_or_else(|| "Loading...".to_string())),
                    icon: "⇄"
                }
                StatCard {
                    label: "Active Seals",
                    value: stats.with(|s| s.as_ref().map(|s| s.total_seals.to_string()).unwrap_or_else(|| "Loading...".to_string())),
                    icon: "🔒"
                }
                StatCard {
                    label: "Contracts",
                    value: stats.with(|s| s.as_ref().map(|s| s.total_contracts.to_string()).unwrap_or_else(|| "Loading...".to_string())),
                    icon: "📄"
                }
            }

            // Chain status cards
            div {
                h2 { class: "text-xl font-semibold mb-4", "Chain Status" }
                div { class: "grid grid-cols-1 md:grid-cols-5 gap-4",
                    ChainStatusCard { chain: "Bitcoin".to_string(), status: "Checking...".to_string(), block: 0, sync_lag: 0 }
                    ChainStatusCard { chain: "Ethereum".to_string(), status: "Checking...".to_string(), block: 0, sync_lag: 0 }
                    ChainStatusCard { chain: "Sui".to_string(), status: "Checking...".to_string(), block: 0, sync_lag: 0 }
                    ChainStatusCard { chain: "Aptos".to_string(), status: "Checking...".to_string(), block: 0, sync_lag: 0 }
                    ChainStatusCard { chain: "Solana".to_string(), status: "Checking...".to_string(), block: 0, sync_lag: 0 }
                }
            }

            // Recent activity
            div {
                div { class: "flex items-center justify-between mb-4",
                    h2 { class: "text-xl font-semibold", "Recent Activity" }
                    Link { to: Route::TransfersList {},
                        span { class: "text-blue-400 hover:text-blue-300 text-sm", "View all →" }
                    }
                }
                div { class: "bg-gray-900 rounded-xl border border-gray-800 overflow-hidden",
                    div { class: "divide-y divide-gray-800",
                        for sanad in recent_sanads.read().clone() {
                            ActivityRow {
                                action: "Sanad Created".to_string(),
                                chain: sanad.chain.clone(),
                                id: sanad.id.clone(),
                                time: format!("{} ago", format_datetime(sanad.created_at))
                            }
                        }
                        for transfer in recent_transfers.read().clone() {
                            ActivityRow {
                                action: "Transfer".to_string(),
                                chain: format!("{} → {}", transfer.from_chain, transfer.to_chain),
                                id: transfer.id.clone(),
                                time: format!("{} ago", format_datetime(transfer.created_at))
                            }
                        }
                        for seal in recent_seals.read().clone() {
                            ActivityRow {
                                action: format!("Seal {:?}", seal.seal_type),
                                chain: seal.chain.clone(),
                                id: seal.id.clone(),
                                time: format!("Block {} ago", seal.block_height)
                            }
                        }
                        if recent_sanads.read().is_empty() && recent_transfers.read().is_empty() && recent_seals.read().is_empty() {
                            div { class: "px-6 py-12 text-center text-gray-500",
                                "No recent activity. Start the indexer to begin syncing data."
                            }
                        }
                    }
                }
            }

            // Quick search
            div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                h2 { class: "text-xl font-semibold mb-4", "Quick Search" }
                p { class: "text-gray-400 mb-4", "Search by Sanad ID, Transfer ID, Seal ID, or Owner Address" }
                div { class: "flex gap-2",
                    input {
                        r#type: "text",
                        r#type: "text",
                        class: "flex-1 bg-gray-800 border border-gray-700 rounded-lg px-4 py-2 text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
                    }
                    button { class: "px-6 py-2 bg-blue-600 hover:bg-blue-700 rounded-lg font-medium transition-colors",
                        "Search"
                    }
                }
            }

            // API status
            div { class: "flex items-center justify-between text-sm",
                span { class: "text-gray-500", "API Status" }
                span { class: "flex items-center gap-2",
                    span { class: "w-2 h-2 rounded-full",
                        class: if api_health() == "connected" { "bg-green-500" } else { "bg-red-500" }
                    }
                    "{api_health()}"
                }
            }
        }
    }
}

fn format_datetime(dt: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let diff = (now - dt).num_seconds();
    if diff < 60 {
        format!("{}s", diff)
    } else if diff < 3600 {
        format!("{}m", diff / 60)
    } else if diff < 86400 {
        format!("{}h", diff / 3600)
    } else {
        format!("{}d", diff / 86400)
    }
}

#[component]
fn ChainStatusCard(chain: String, status: String, block: u64, sync_lag: u64) -> Element {
    let status_color = match status.as_str() {
        "synced" => "bg-green-500",
        "syncing" => "bg-yellow-500",
        "error" => "bg-red-500",
        _ => "bg-gray-500",
    };

    rsx! {
        div { class: "bg-gray-900 rounded-xl border border-gray-800 p-4",
            div { class: "flex items-center gap-2 mb-3",
                span { class: "w-2 h-2 rounded-full {status_color}" }
                span { class: "font-medium", "{chain}" }
            }
            div { class: "text-sm text-gray-400 space-y-1",
                div { class: "flex justify-between",
                    span { "Block" }
                    span { class: "text-gray-200 font-mono text-xs", "{block}" }
                }
                div { class: "flex justify-between",
                    span { "Sync Lag" }
                    span { class: "text-gray-200", "{sync_lag} blocks" }
                }
            }
        }
    }
}

#[component]
fn ActivityRow(action: String, chain: String, id: String, time: String) -> Element {
    rsx! {
        div { class: "px-6 py-4 flex items-center justify-between hover:bg-gray-800/50 transition-colors",
            div { class: "flex items-center gap-4",
                div {
                    span { class: "text-sm font-medium text-blue-400", "{action}" }
                    div { class: "text-xs text-gray-500 mt-0.5", "{chain}" }
                }
            }
            div { class: "text-sanad",
                div { class: "font-mono text-sm text-gray-300", "{id}" }
                div { class: "text-xs text-gray-500", "{time}" }
            }
        }
    }
}
