/// Status indicator component for various entity statuses.
use dioxus::prelude::*;

/// Display a badge indicating the status of an entity.
#[component]
pub fn StatusBadge(status: String) -> Element {
    let colors = match status.to_lowercase().as_str() {
        "active" | "available" | "completed" | "synced" => "bg-green-500/20 text-green-400",
        "spent" | "consumed" | "deprecated" => "bg-gray-500/20 text-gray-400",
        "pending" | "in_progress" | "syncing" => "bg-yellow-500/20 text-yellow-400",
        "failed" | "error" | "stopped" => "bg-red-500/20 text-red-400",
        _ => "bg-gray-500/20 text-gray-400",
    };

    let icon = match status.to_lowercase().as_str() {
        "active" | "available" | "synced" | "completed" => "✓",
        "spent" | "consumed" | "deprecated" => "—",
        "pending" | "in_progress" | "syncing" => "⟳",
        "failed" | "error" | "stopped" => "✗",
        _ => "•",
    };

    rsx! {
        span {
            class: "inline-flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium {colors}",
            span { class: "text-sm", "{icon}" }
            "{status}"
        }
    }
}
