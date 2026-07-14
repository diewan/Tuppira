/// Wallet connection page.
use dioxus::prelude::*;
use dioxus_router::components::Link;

use crate::app::routes::Route;

#[component]
pub fn Wallet() -> Element {
    let mut connected = use_signal(|| false);
    let mut wallet_address = use_signal(|| String::new());

    rsx! {
        div { class: "space-y-8 max-w-2xl mx-auto",
            h1 { class: "text-2xl font-bold", "Wallet" }

            if !*connected.read() {
                // Connect wallet view
                div { class: "bg-gray-900 rounded-xl border border-gray-800 p-12 text-center",
                    div { class: "mb-6",
                        span { class: "text-6xl", "🔗" }
                    }
                    h2 { class: "text-xl font-semibold mb-2", "Connect Your CSV Wallet" }
                    p { class: "text-gray-400 mb-6",
                        "Connect your wallet to view your sanads and initiate transfers across chains."
                    }
                    button {
                        onclick: move |_| {
                            connected.set(true);
                            wallet_address.set("bc1q...example".to_string());
                        },
                        class: "px-8 py-3 bg-blue-600 hover:bg-blue-700 rounded-lg font-medium transition-colors",
                        "Connect Wallet"
                    }
                }
            } else {
                // Connected wallet view
                div {
                    // Wallet info
                    div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                        div { class: "flex items-center justify-between mb-4",
                            h2 { class: "text-lg font-semibold", "Connected Wallet" }
                            button {
                                onclick: move |_| {
                                    connected.set(false);
                                    wallet_address.set(String::new());
                                },
                                class: "text-sm text-red-400 hover:text-red-300",
                                "Disconnect"
                            }
                        }
                        div { class: "flex items-center gap-3",
                            span { class: "w-10 h-10 rounded-full bg-blue-600 flex items-center justify-center text-lg", "🔑" }
                            div {
                                div { class: "font-mono text-sm", "{wallet_address.read()}" }
                                div { class: "text-xs text-gray-500", "Connected" }
                            }
                        }
                    }

                    // Sanads owned by wallet
                    div {
                        div { class: "flex items-center justify-between mb-4",
                            h2 { class: "text-lg font-semibold", "Your Sanads" }
                            Link { to: Route::SanadsList {},
                                span { class: "text-blue-400 hover:text-blue-300 text-sm", "View all →" }
                            }
                        }
                        div { class: "bg-gray-900 rounded-xl border border-gray-800 p-8 text-center text-gray-500",
                            "No sanads found for this wallet"
                        }
                    }

                    // Quick actions
                    div { class: "bg-gray-900 rounded-xl border border-gray-800 p-6",
                        h2 { class: "text-lg font-semibold mb-4", "Quick Actions" }
                        div { class: "grid grid-cols-2 gap-4",
                            button { class: "p-4 bg-gray-800 rounded-lg hover:bg-gray-700 transition-colors text-left",
                                div { class: "text-xl mb-2", "⇄" }
                                div { class: "font-medium text-sm", "Transfer Sanad" }
                                div { class: "text-xs text-gray-500", "Move a sanad to another chain" }
                            }
                            button { class: "p-4 bg-gray-800 rounded-lg hover:bg-gray-700 transition-colors text-left",
                                div { class: "text-xl mb-2", "🔍" }
                                div { class: "font-medium text-sm", "Search Explorer" }
                                div { class: "text-xs text-gray-500", "Find sanads and transfers" }
                            }
                        }
                    }
                }
            }
        }
    }
}
