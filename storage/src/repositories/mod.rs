/// Repository modules for database access.
pub mod advanced_proofs;
pub mod contracts;
pub mod priority_addresses;
pub mod sanads;
pub mod seals;
pub mod stats;
pub mod sync;
pub mod transfers;

pub use advanced_proofs::AdvancedProofRepository;
pub use contracts::ContractsRepository;
pub use priority_addresses::{IndexingActivityRequest, PriorityAddressRepository};
pub use sanads::SanadsRepository;
pub use seals::SealsRepository;
pub use stats::StatsRepository;
pub use sync::SyncRepository;
pub use transfers::TransfersRepository;
