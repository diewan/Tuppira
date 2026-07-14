/// Transfer detail page showing full information about a cross-chain transfer.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;

#[component]
pub fn TransferDetail(id: String) -> Element {
    let mut transfer: Signal<Option<csv_explorer_shared::TransferRecord>> = use_signal(|| None);

    use_effect({
        let id = id.clone();
        move || {
            spawn(async move {
                // Fetch transfer details from API
                match fetch_transfer(&id).await {
                    Ok(transfer_data) => transfer.set(Some(transfer_data)),
                    Err(e) => {
                        tracing::error!("Failed to fetch transfer {}: {}", id, e);
                        transfer.set(None);
                    }
                }
            });
        }
    });

    rsx! {
        div { class: "space-y-8",
            // Breadcrumb
            div { class: "flex items-center gap-2 text-sm text-gray-500",
                Link { to: Route::TransfersList {}, class: "hover:text-gray-300", "Transfers" }
                span { "→" }
                span { class: "text-gray-300 font-mono", "{id}" }
            }

            // Header
            div { class: "flex items-center justify-between",
                h1 { class: "text-2xl font-bold", "Transfer Detail" }
                if let Some(ref t) = *transfer.read() {
                    StatusBadge { status: t.status.to_string() }
                }
            }

            if let Some(ref t) = *transfer.read() {
                // Transfer overview
                div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                    div { class: "grid grid-cols-1 md:grid-cols-2 gap-6",
                        DetailRow { label: "Transfer ID", value: t.id.clone() }
                        DetailRow { label: "Sanad ID", value: t.sanad_id.clone() }
                        DetailRow { label: "From Chain", value: t.from_chain.clone() }
                        DetailRow { label: "To Chain", value: t.to_chain.clone() }
                        DetailRow { label: "From Owner", value: t.from_owner.clone() }
                        DetailRow { label: "To Owner", value: t.to_owner.clone() }
                        DetailRow { label: "Lock TX", value: t.lock_tx.clone() }
                        DetailRow { label: "Created", value: t.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string() }
                        if let Some(ref mint_tx) = t.mint_tx {
                            DetailRow { label: "Mint TX", value: mint_tx.clone() }
                        }
                        if let Some(ref proof_ref) = t.proof_ref {
                            DetailRow { label: "Proof Reference", value: proof_ref.clone() }
                        }
                        if let Some(completed_at) = t.completed_at {
                            DetailRow { label: "Completed", value: completed_at.format("%Y-%m-%d %H:%M:%S UTC").to_string() }
                        }
                        if let Some(duration_ms) = t.duration_ms {
                            DetailRow { label: "Duration", value: format_duration(duration_ms) }
                        }
                    }
                }

                // Transfer timeline
                div {
                    h2 { class: "text-lg font-semibold mb-4", "Transfer Progress" }
                    div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                        TransferTimeline {
                            status: t.status.to_string(),
                            lock_tx: t.lock_tx.clone(),
                            mint_tx: t.mint_tx.clone(),
                            created_at: t.created_at,
                            completed_at: t.completed_at,
                        }
                    }
                }

                // Related links
                div { class: "flex items-center gap-4",
                    Link { to: Route::SanadDetail { id: t.sanad_id.clone() },
                        button { class: "px-4 py-2 bg-gray-800 hover:bg-gray-700 rounded-lg text-sm font-medium transition-colors",
                            "View Sanad →"
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
        "pending" => "bg-yellow-500/20 text-yellow-400 border-yellow-500/30",
        "in_progress" => "bg-blue-500/20 text-blue-400 border-blue-500/30",
        "completed" => "bg-green-500/20 text-green-400 border-green-500/30",
        "failed" => "bg-red-500/20 text-red-400 border-red-500/30",
        _ => "bg-gray-800 text-gray-400 border-gray-700",
    };

    rsx! {
        span { class: "px-3 py-1 rounded-full text-xs font-medium border {color_class}",
            "{status}"
        }
    }
}

#[component]
fn TransferTimeline(
    status: String,
    lock_tx: String,
    mint_tx: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Element {
    let steps = vec![
        TimelineStep {
            label: "Lock Submitted".to_string(),
            description: "Transaction submitted on source chain".to_string(),
            time: created_at.format("%Y-%m-%d %H:%M").to_string(),
            completed: true,
        },
        TimelineStep {
            label: "Proof Generated".to_string(),
            description: "Cross-chain proof generated".to_string(),
            time: String::new(),
            completed: matches!(status.to_lowercase().as_str(), "in_progress" | "completed"),
        },
        TimelineStep {
            label: "Mint Completed".to_string(),
            description: "Sanad minted on destination chain".to_string(),
            time: completed_at
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default(),
            completed: status.to_lowercase() == "completed",
        },
    ];

    rsx! {
        div { class: "space-y-0",
            {steps.iter().enumerate().map(|(i, step)| rsx! {
                div { key: "{i}", class: "flex gap-4",
                    div { class: "flex flex-col items-center",
                        div {
                            class: "w-8 h-8 rounded-full flex items-center justify-center text-sm {step_icon_class(step.completed)}",
                            if step.completed { "✓" } else { "○" }
                        }
                        if i < steps.len() - 1 {
                            div { class: "w-0.5 h-12 {step_line_class(step.completed)}" }
                        }
                    }
                    div { class: "pb-8",
                        div { class: "font-medium {step_text_class(step.completed)}",
                            "{step.label}"
                        }
                        div { class: "text-sm text-gray-500",
                            "{step.description}"
                        }
                        if !step.time.is_empty() {
                            div { class: "text-xs text-gray-600 mt-1",
                                "{step.time}"
                            }
                        }
                    }
                }
            })}
        }
    }
}

struct TimelineStep {
    label: String,
    description: String,
    time: String,
    completed: bool,
}

fn step_icon_class(completed: bool) -> &'static str {
    if completed {
        "bg-green-500/20 text-green-400 border border-green-500/30"
    } else {
        "bg-gray-800 text-gray-600 border border-gray-700"
    }
}

fn step_line_class(completed: bool) -> &'static str {
    if completed {
        "bg-green-500/30"
    } else {
        "bg-gray-800"
    }
}

fn step_text_class(completed: bool) -> &'static str {
    if completed {
        "text-gray-200"
    } else {
        "text-gray-500"
    }
}

fn format_duration(duration_ms: u64) -> String {
    let seconds = duration_ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;

    if hours > 0 {
        format!("{}h {}m", hours, minutes % 60)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds % 60)
    } else {
        format!("{}s", seconds)
    }
}

/// Fetch transfer details from the API
async fn fetch_transfer(id: &str) -> Result<csv_explorer_shared::TransferRecord, String> {
    let client = reqwest::Client::new();
    let response = client
        .get(&format!("/api/transfers/{}", id))
        .send()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;
    
    if response.status() == 404 {
        return Err(format!("Transfer {} not found", id));
    }
    
    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }
    
    response
        .json::<csv_explorer_shared::TransferRecord>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}
