pub mod chains;
pub mod contracts;
/// Page components module.
pub mod home;
// pub mod sanad_detail;
// pub mod sanads;
pub mod seal_detail;
pub mod seals;
pub mod stats;
pub mod transfer_detail;
pub mod transfers;
pub mod wallet;

// Re-export all page components for use in routing
pub use chains::Chains;
pub use contracts::ContractsList;
pub use home::Home;
// pub use sanad_detail::SanadDetail;
// pub use sanads::SanadsList;
pub use seal_detail::SealDetail;
pub use seals::SealsList;
pub use stats::Stats;
pub use transfer_detail::TransferDetail;
pub use transfers::TransfersList;
pub use wallet::Wallet;
