/// Seal detail page showing full information about a CSV seal.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;

#[component]
pub fn SealDetail(id: String) -> Element {
    let mut seal: Signal<Option<csv_explorer_shared::SealRecord>> = use_signal(|| None);

    use_effect({
        let id = id.clone();
        move || {
            spawn(async move {
                // Fetch seal details from API
                match fetch_seal(&id).await {
                    Ok(seal_data) => seal.set(Some(seal_data)),
                    Err(e) => {
                        tracing::error!("Failed to fetch seal {}: {}", id, e);
                        seal.set(None);
                    }
                }
            });
        }
    });

    rsx! {
        div { class: "space-y-8",
            // Breadcrumb
            div { class: "flex items-center gap-2 text-sm text-gray-500",
                Link { to: Route::SealsList {}, class: "hover:text-gray-300", "Seals" }
                span { "→" }
                span { class: "text-gray-300 font-mono", "{id}" }
            }

            // Header
            div { class: "flex items-center justify-between",
                h1 { class: "text-2xl font-bold", "Seal Detail" }
                if let Some(ref s) = *seal.read() {
                    StatusBadge { status: s.status.to_string() }
                }
            }

            if let Some(ref s) = *seal.read() {
                // Main info card
                div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                    div { class: "grid grid-cols-1 md:grid-cols-2 gap-6",
                        DetailRow { label: "Seal ID", value: s.id.clone() }
                        DetailRow { label: "Chain", value: s.chain.clone() }
                        DetailRow { label: "Seal Type", value: s.seal_type.to_string() }
                        DetailRow { label: "Seal Reference", value: s.seal_ref.clone() }
                        DetailRow { label: "Block Height", value: s.block_height.to_string() }
                        if let Some(ref sanad_id) = s.sanad_id {
                            DetailRow { label: "Linked Sanad", value: sanad_id.clone() }
                        }
                        DetailRow { label: "Status", value: s.status.to_string() }
                        if let Some(consumed_at) = s.consumed_at {
                            DetailRow { label: "Consumed At", value: consumed_at.format("%Y-%m-%d %H:%M:%S UTC").to_string() }
                        }
                        if let Some(ref consumed_tx) = s.consumed_tx {
                            DetailRow { label: "Consumed TX", value: consumed_tx.clone() }
                        }
                    }
                }

                // Seal type explanation
                div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                    h2 { class: "text-lg font-semibold mb-4", "Seal Type Information" }
                    SealTypeInfo { seal_type: s.seal_type.to_string() }
                }

                // Related links
                if let Some(ref sanad_id) = s.sanad_id {
                    div { class: "flex items-center gap-4",
                        Link { to: Route::SanadDetail { id: sanad_id.clone() },
                            button { class: "px-4 py-2 bg-gray-800 hover:bg-gray-700 rounded-lg text-sm font-medium transition-colors",
                                "View Linked Sanad →"
                            }
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
        "available" => "bg-green-500/20 text-green-400 border-green-500/30",
        "consumed" => "bg-gray-800 text-gray-400 border-gray-700",
        _ => "bg-gray-800 text-gray-400 border-gray-700",
    };

    rsx! {
        span { class: "px-3 py-1 rounded-full text-xs font-medium border {color_class}",
            "{status}"
        }
    }
}

#[component]
fn SealTypeInfo(seal_type: String) -> Element {
    let (description, example) = match seal_type.as_str() {
        "utxo" => (
            "A UTXO-based seal that locks a sanad in a specific unspent transaction output on Bitcoin.",
            "Created by committing a sanad to a Taproot output with a specific spending condition.",
        ),
        "object" => (
            "An object-based seal that stores a sanad in a Sui object on the Sui blockchain.",
            "Created by wrapping a sanad in a Sui object with sealed transfer capabilities.",
        ),
        "resource" => (
            "A resource-based seal that embeds a sanad in an Aptos resource.",
            "Created by storing a sanad in an Aptos account resource with access controls.",
        ),
        "nullifier" => (
            "A nullifier-based seal that prevents double-spending by recording a nullifier.",
            "Created by generating a cryptographic nullifier from the sanad's commitment.",
        ),
        "account" => (
            "An account-based seal that associates a sanad with a Solana account state.",
            "Created by storing a sanad in a Solana program-derived account (PDA).",
        ),
        _ => (
            "Unknown seal type. This may indicate a new or unsupported seal mechanism.",
            "Please check the documentation for supported seal types.",
        ),
    };

    rsx! {
        div { class: "space-y-4",
            div {
                h3 { class: "text-sm font-semibold text-gray-400 mb-2", "Description" }
                p { class: "text-sm text-gray-300", "{description}" }
            }
            div {
                h3 { class: "text-sm font-semibold text-gray-400 mb-2", "Example" }
                p { class: "text-sm text-gray-300", "{example}" }
            }
            div { class: "bg-gray-800 rounded-lg p-3",
                code { class: "text-xs font-mono text-blue-400", "{seal_type}" }
            }
        }
    }
}

/// Fetch seal details from the API
async fn fetch_seal(id: &str) -> Result<csv_explorer_shared::SealRecord, String> {
    let client = reqwest::Client::new();
    let response = client
        .get(&format!("/api/seals/{}", id))
        .send()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;
    
    if response.status() == 404 {
        return Err(format!("Seal {} not found", id));
    }
    
    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }
    
    response
        .json::<csv_explorer_shared::SealRecord>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}
