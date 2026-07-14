/// Color-coded chain badge component.
use dioxus::prelude::*;

/// Display a badge indicating which chain an entity belongs to.
#[component]
pub fn ChainBadge(chain: String) -> Element {
    let colors = match chain.to_lowercase().as_str() {
        "bitcoin" => "bg-orange-500/20 text-orange-400 border-orange-500/30",
        "ethereum" => "bg-blue-500/20 text-blue-400 border-blue-500/30",
        "sui" => "bg-cyan-500/20 text-cyan-400 border-cyan-500/30",
        "aptos" => "bg-green-500/20 text-green-400 border-green-500/30",
        "solana" => "bg-purple-500/20 text-purple-400 border-purple-500/30",
        _ => "bg-gray-500/20 text-gray-400 border-gray-500/30",
    };

    let icon = match chain.to_lowercase().as_str() {
        "bitcoin" => "₿",
        "ethereum" => "Ξ",
        "sui" => "S",
        "aptos" => "A",
        "solana" => "◎",
        _ => &chain[..1],
    };

    rsx! {
        span {
            class: "inline-flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium border {colors}",
            span { class: "text-sm", "{icon}" }
            "{chain}"
        }
    }
}
