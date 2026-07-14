/// Styles module.
///
/// In production, this would manage Tailwind CSS configuration and
/// custom style injection for the Dioxus UI.

/// Chain-specific color palette for consistent theming.
pub mod chain_colors {
    /// Bitcoin colors.
    pub const BITCOIN_PRIMARY: &str = "#F7931A";
    pub const BITCOIN_BG: &str = "rgba(247, 147, 26, 0.2)";

    /// Ethereum colors.
    pub const ETHEREUM_PRIMARY: &str = "#627EEA";
    pub const ETHEREUM_BG: &str = "rgba(98, 126, 234, 0.2)";

    /// Sui colors.
    pub const SUI_PRIMARY: &str = "#06BDFF";
    pub const SUI_BG: &str = "rgba(6, 189, 255, 0.2)";

    /// Aptos colors.
    pub const APTOS_PRIMARY: &str = "#2DD8A3";
    pub const APTOS_BG: &str = "rgba(45, 216, 163, 0.2)";

    /// Solana colors.
    pub const SOLANA_PRIMARY: &str = "#9945FF";
    pub const SOLANA_BG: &str = "rgba(153, 69, 255, 0.2)";
}

/// Status colors.
pub mod status_colors {
    pub const SUCCESS: &str = "#22C55E";
    pub const SUCCESS_BG: &str = "rgba(34, 197, 94, 0.2)";

    pub const WARNING: &str = "#EAB308";
    pub const WARNING_BG: &str = "rgba(234, 179, 8, 0.2)";

    pub const ERROR: &str = "#EF4444";
    pub const ERROR_BG: &str = "rgba(239, 68, 68, 0.2)";

    pub const NEUTRAL: &str = "#6B7280";
    pub const NEUTRAL_BG: &str = "rgba(107, 114, 128, 0.2)";
}
