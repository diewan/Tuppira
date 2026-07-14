/// Reusable card component with consistent styling.
use dioxus::prelude::*;

/// Card component with optional title and children content.
#[component]
pub fn Card(title: String, children: Element) -> Element {
    rsx! {
        div { class: "bg-gray-900 rounded-xl border border-gray-800 overflow-hidden",
            if !title.is_empty() {
                div { class: "px-6 py-4 border-b border-gray-800",
                    h2 { class: "text-lg font-semibold text-gray-100", "{title}" }
                }
            }
            div { class: "p-6",
                {children}
            }
        }
    }
}

/// Stat card component for displaying key metrics.
#[component]
pub fn StatCard(label: String, value: String, icon: String) -> Element {
    rsx! {
        div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
            div { class: "flex items-center justify-between mb-2",
                span { class: "text-2xl", "{icon}" }
            }
            div { class: "text-3xl font-bold mb-1 text-gray-100", "{value}" }
            div { class: "text-gray-400 text-sm", "{label}" }
        }
    }
}
