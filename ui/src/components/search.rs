/// Global search component for the header.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;

/// Global search input with autocomplete suggestions.
#[component]
pub fn GlobalSearch() -> Element {
    let mut query = use_signal(|| String::new());
    let mut show_results = use_signal(|| false);
    let suggestions = use_resource(move || async move {
        let q = query.read().clone();
        if q.len() < 3 {
            return Vec::new();
        }
        fetch_suggestions(&q).await
    });

    rsx! {
        div { class: "relative",
            div { class: "relative",
                span { class: "absolute left-3 top-1/2 -translate-y-1/2 text-gray-500", "🔍" }
                input {
                    r#type: "text",
                    value: "{query.read()}",
                    oninput: move |evt| {
                        query.set(evt.value());
                        show_results.set(!evt.value().is_empty());
                    },
                    onfocus: move |_| show_results.set(true),
                    r#type: "text",
                    class: "w-64 bg-gray-800 border border-gray-700 rounded-lg pl-10 pr-4 py-2 text-sm text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
                }
                // Keyboard shortcut hint
                span { class: "absolute sanad-3 top-1/2 -translate-y-1/2 text-xs text-gray-600",
                    "⌘K"
                }
            }

            // Suggestions dropdown
            if *show_results.read() && query.read().len() >= 3 {
                div { class: "absolute top-full mt-2 w-96 bg-gray-900 border border-gray-700 rounded-lg shadow-xl overflow-hidden z-50",
                    if let Some(suggestions_list) = suggestions.read().as_ref() {
                        if suggestions_list.is_empty() {
                            div { class: "px-4 py-3 text-sm text-gray-500",
                                "No results found for \"{query.read()}\""
                            }
                        } else {
                            div { class: "divide-y divide-gray-800",
                                {suggestions_list.iter().map(|s: &SearchSuggestion| rsx! {
                                    Link {
                                        key: "{s.id}",
                                        to: s.route.clone(),
                                        class: "px-4 py-3 hover:bg-gray-800 transition-colors flex items-center justify-between",
                                        onclick: move |_| {
                                            query.set(String::new());
                                            show_results.set(false);
                                        },
                                        div {
                                            div { class: "text-sm text-gray-200 font-mono", "{s.id}" }
                                            div { class: "text-xs text-gray-500", "{s.type_}" }
                                        }
                                        span { class: "text-xs text-gray-500", "{s.chain}" }
                                    }
                                })}
                            }
                        }
                    } else {
                        div { class: "px-4 py-3 text-sm text-gray-500 animate-pulse",
                            "Searching..."
                        }
                    }
                }
            }
        }
    }
}

/// A search suggestion result.
#[derive(Clone)]
struct SearchSuggestion {
    id: String,
    type_: String,
    chain: String,
    route: Route,
}

async fn fetch_suggestions(_query: &str) -> Vec<SearchSuggestion> {
    // In production, query the API for suggestions
    Vec::new()
}
