//! Indexer Plugin System for Plug-and-Play Chain Support
//!
//! This module provides a plugin system that allows chain indexers to be registered
//! and discovered dynamically at runtime, enabling true plug-and-play support.

use std::collections::HashMap;
use std::sync::Arc;

use crate::chain_indexer::ChainIndexer;
use crate::rpc_manager::RpcManager;
use csv_explorer_shared::ChainConfig;

/// Factory function type for creating chain indexers
type IndexerFactoryFn = Arc<dyn Fn(ChainConfig, RpcManager) -> Box<dyn ChainIndexer> + Send + Sync>;

/// Registry for chain indexer plugins
///
/// This registry manages the registration and discovery of chain indexers,
/// enabling dynamic chain support without hardcoding.
pub struct IndexerPluginRegistry {
    factories: HashMap<String, IndexerFactoryFn>,
}

impl IndexerPluginRegistry {
    /// Create a new empty indexer plugin registry
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a chain indexer factory
    ///
    /// # Arguments
    /// * `chain_id` - The unique identifier for the chain (e.g., "bitcoin", "solana")
    /// * `factory` - A factory function that creates the indexer
    ///
    /// # Example
    /// ```rust
    /// use csv_explorer_indexer::indexer_plugin::IndexerPluginRegistry;
    /// use csv_explorer_indexer::bitcoin::BitcoinIndexer;
    ///
    /// let mut registry = IndexerPluginRegistry::new();
    /// registry.register("bitcoin", |config, rpc_manager| {
    ///     Box::new(BitcoinIndexer::new(config, rpc_manager))
    /// });
    /// ```
    pub fn register<F>(&mut self, chain_id: &str, factory: F)
    where
        F: Fn(ChainConfig, RpcManager) -> Box<dyn ChainIndexer> + Send + Sync + 'static,
    {
        self.factories
            .insert(chain_id.to_string(), Arc::new(factory));
    }

    /// Unregister a chain indexer
    ///
    /// # Arguments
    /// * `chain_id` - The chain ID to unregister
    ///
    /// # Returns
    /// `true` if the indexer was removed, `false` if it didn't exist
    pub fn unregister(&mut self, chain_id: &str) -> bool {
        self.factories.remove(chain_id).is_some()
    }

    /// Check if a chain indexer is registered
    ///
    /// # Arguments
    /// * `chain_id` - The chain ID to check
    pub fn is_registered(&self, chain_id: &str) -> bool {
        self.factories.contains_key(chain_id)
    }

    /// Create an indexer for a registered chain
    ///
    /// # Arguments
    /// * `chain_id` - The chain ID
    /// * `config` - Chain configuration
    /// * `rpc_manager` - RPC manager for chain communication
    ///
    /// # Returns
    /// `Some(Box<dyn ChainIndexer>)` if the chain is registered, `None` otherwise
    pub fn create_indexer(
        &self,
        chain_id: &str,
        config: ChainConfig,
        rpc_manager: RpcManager,
    ) -> Option<Box<dyn ChainIndexer>> {
        self.factories
            .get(chain_id)
            .map(|factory| factory(config, rpc_manager))
    }

    /// Create an Arc-wrapped indexer for a registered chain
    ///
    /// # Arguments
    /// * `chain_id` - The chain ID
    /// * `config` - Chain configuration
    /// * `rpc_manager` - RPC manager for chain communication
    ///
    /// # Returns
    /// `Some(Arc<dyn ChainIndexer>)` if the chain is registered, `None` otherwise
    pub fn create_arc_indexer(
        &self,
        chain_id: &str,
        config: ChainConfig,
        rpc_manager: RpcManager,
    ) -> Option<Arc<dyn ChainIndexer>> {
        self.factories
            .get(chain_id)
            .map(|factory| Arc::from(factory(config, rpc_manager)))
    }

    /// Get all registered chain IDs
    pub fn registered_chains(&self) -> Vec<&str> {
        self.factories.keys().map(|k| k.as_str()).collect()
    }

    /// Get the number of registered indexers
    pub fn indexer_count(&self) -> usize {
        self.factories.len()
    }

    /// Create indexers for all registered chains with their configurations
    ///
    /// # Arguments
    /// * `chain_configs` - Map of chain IDs to their configurations
    /// * `rpc_manager` - RPC manager for chain communication
    ///
    /// # Returns
    /// Vector of created indexers
    pub fn create_all_indexers(
        &self,
        chain_configs: &HashMap<String, ChainConfig>,
        rpc_manager: RpcManager,
    ) -> Vec<Box<dyn ChainIndexer>> {
        let mut indexers = Vec::new();

        for (chain_id, config) in chain_configs {
            if !config.enabled {
                continue;
            }
            if let Some(indexer) =
                self.create_indexer(chain_id, config.clone(), rpc_manager.clone())
            {
                indexers.push(indexer);
            } else {
                tracing::warn!(
                    chain = %chain_id,
                    "No indexer implementation registered for enabled chain"
                );
            }
        }

        indexers
    }

    /// Create Arc-wrapped indexers for all registered chains
    ///
    /// # Arguments
    /// * `chain_configs` - Map of chain IDs to their configurations
    /// * `rpc_manager` - RPC manager for chain communication
    ///
    /// # Returns
    /// Vector of Arc-wrapped indexers
    pub fn create_all_arc_indexers(
        &self,
        chain_configs: &HashMap<String, ChainConfig>,
        rpc_manager: RpcManager,
    ) -> Vec<Arc<dyn ChainIndexer>> {
        let mut indexers = Vec::new();

        for (chain_id, config) in chain_configs {
            if !config.enabled {
                continue;
            }
            if let Some(indexer) =
                self.create_arc_indexer(chain_id, config.clone(), rpc_manager.clone())
            {
                indexers.push(indexer);
            } else {
                tracing::warn!(
                    chain = %chain_id,
                    "No indexer implementation registered for enabled chain"
                );
            }
        }

        indexers
    }
}

impl Default for IndexerPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating a pre-configured indexer plugin registry
///
/// This builder provides a convenient way to set up all built-in chain indexers
/// or customize which chains to support.
pub struct IndexerPluginRegistryBuilder {
    registry: IndexerPluginRegistry,
    include_bitcoin: bool,
    include_ethereum: bool,
    include_solana: bool,
    include_sui: bool,
    include_aptos: bool,
}

impl IndexerPluginRegistryBuilder {
    /// Create a new builder with all built-in chains enabled by default
    pub fn new() -> Self {
        Self {
            registry: IndexerPluginRegistry::new(),
            include_bitcoin: true,
            include_ethereum: true,
            include_solana: true,
            include_sui: true,
            include_aptos: true,
        }
    }

    /// Disable Bitcoin indexer
    pub fn without_bitcoin(mut self) -> Self {
        self.include_bitcoin = false;
        self
    }

    /// Disable Ethereum indexer
    pub fn without_ethereum(mut self) -> Self {
        self.include_ethereum = false;
        self
    }

    /// Disable Solana indexer
    pub fn without_solana(mut self) -> Self {
        self.include_solana = false;
        self
    }

    /// Disable Sui indexer
    pub fn without_sui(mut self) -> Self {
        self.include_sui = false;
        self
    }

    /// Disable Aptos indexer
    pub fn without_aptos(mut self) -> Self {
        self.include_aptos = false;
        self
    }

    /// Build the registry with all enabled built-in chains
    pub fn build(self) -> IndexerPluginRegistry {
        use crate::{aptos, bitcoin, ethereum, solana, sui};

        let mut registry = self.registry;

        if self.include_bitcoin {
            registry.register("bitcoin", |config, rpc_manager| {
                Box::new(bitcoin::BitcoinIndexer::new(config, rpc_manager))
            });
        }

        if self.include_ethereum {
            registry.register("ethereum", |config, rpc_manager| {
                Box::new(ethereum::EthereumIndexer::new(config, rpc_manager))
            });
        }

        if self.include_solana {
            registry.register("solana", |config, rpc_manager| {
                Box::new(solana::SolanaIndexer::new(config, rpc_manager))
            });
        }

        if self.include_sui {
            registry.register("sui", |config, rpc_manager| {
                Box::new(sui::SuiIndexer::new(config, rpc_manager))
            });
        }

        if self.include_aptos {
            registry.register("aptos", |config, rpc_manager| {
                Box::new(aptos::AptosIndexer::new(config, rpc_manager))
            });
        }

        registry
    }
}

impl Default for IndexerPluginRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain_indexer::{AddressIndexingResult, ChainResult};
    use async_trait::async_trait;
    use csv_explorer_shared::{
        CommitmentScheme, CsvContract, EnhancedSanadRecord, EnhancedSealRecord,
        EnhancedTransferRecord, FinalityProofType, InclusionProofType, Network, PriorityLevel,
        SanadRecord, SealRecord, TransferRecord,
    };

    struct MockIndexer;

    #[async_trait]
    impl ChainIndexer for MockIndexer {
        fn chain_id(&self) -> &str {
            "test"
        }
        fn chain_name(&self) -> &str {
            "Test Chain"
        }
        async fn initialize(&self) -> ChainResult<()> {
            Ok(())
        }
        async fn get_chain_tip(&self) -> ChainResult<u64> {
            Ok(100)
        }
        async fn get_latest_synced_block(&self) -> ChainResult<u64> {
            Ok(0)
        }
        async fn index_sanads(&self, _block: u64) -> ChainResult<Vec<SanadRecord>> {
            Ok(vec![])
        }
        async fn index_seals(&self, _block: u64) -> ChainResult<Vec<SealRecord>> {
            Ok(vec![])
        }
        async fn index_transfers(&self, _block: u64) -> ChainResult<Vec<TransferRecord>> {
            Ok(vec![])
        }
        async fn index_contracts(&self, _block: u64) -> ChainResult<Vec<CsvContract>> {
            Ok(vec![])
        }
        async fn index_enhanced_sanads(
            &self,
            _block: u64,
        ) -> ChainResult<Vec<EnhancedSanadRecord>> {
            Ok(vec![])
        }
        async fn index_enhanced_seals(&self, _block: u64) -> ChainResult<Vec<EnhancedSealRecord>> {
            Ok(vec![])
        }
        async fn index_enhanced_transfers(
            &self,
            _block: u64,
        ) -> ChainResult<Vec<EnhancedTransferRecord>> {
            Ok(vec![])
        }
        async fn index_sanads_by_address(&self, _address: &str) -> ChainResult<Vec<SanadRecord>> {
            Ok(vec![])
        }
        async fn index_seals_by_address(&self, _address: &str) -> ChainResult<Vec<SealRecord>> {
            Ok(vec![])
        }
        async fn index_transfers_by_address(
            &self,
            _address: &str,
        ) -> ChainResult<Vec<TransferRecord>> {
            Ok(vec![])
        }
        async fn index_addresses_with_priority(
            &self,
            _addresses: &[String],
            _priority: PriorityLevel,
            _network: Network,
        ) -> ChainResult<AddressIndexingResult> {
            Ok(AddressIndexingResult {
                addresses_processed: 0,
                sanads_indexed: 0,
                seals_indexed: 0,
                transfers_indexed: 0,
                contracts_indexed: 0,
                errors: vec![],
            })
        }
        fn detect_commitment_scheme(&self, _data: &[u8]) -> Option<CommitmentScheme> {
            None
        }
        fn detect_inclusion_proof_type(&self) -> InclusionProofType {
            InclusionProofType::Merkle
        }
        fn detect_finality_proof_type(&self) -> FinalityProofType {
            FinalityProofType::ConfirmationDepth
        }
    }

    #[test]
    fn test_registry_register_and_create() {
        let mut registry = IndexerPluginRegistry::new();

        registry.register("test", |_config, _rpc| Box::new(MockIndexer));

        assert!(registry.is_registered("test"));
        assert_eq!(registry.indexer_count(), 1);
    }

    #[test]
    fn test_registry_unregister() {
        let mut registry = IndexerPluginRegistry::new();

        registry.register("test", |_config, _rpc| Box::new(MockIndexer));
        assert!(registry.unregister("test"));
        assert!(!registry.is_registered("test"));
        assert!(!registry.unregister("test")); // Already removed
    }
}
