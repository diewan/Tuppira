/// Repository for priority address management.
///
/// Handles registration and tracking of wallet addresses for priority indexing.
use chrono::{DateTime, Utc};
use csv_explorer_shared::{
    IndexingActivity, Network, PriorityAddress, PriorityIndexingStatus, PriorityLevel,
};
use sqlx::{Row, SqlitePool};

/// Parameters for recording an indexing activity.
pub struct IndexingActivityRequest<'a> {
    /// The address being indexed
    pub address: &'a str,
    /// The chain identifier
    pub chain: &'a str,
    /// The network type
    pub network: Network,
    /// The type of data indexed
    pub indexed_type: &'a str,
    /// Number of items indexed
    pub items_count: u64,
    /// Whether indexing succeeded
    pub success: bool,
    /// Optional error message
    pub error: Option<&'a str>,
}

impl<'a> IndexingActivityRequest<'a> {
    /// Create a new indexing activity request
    pub fn new(
        address: &'a str,
        chain: &'a str,
        network: Network,
        indexed_type: &'a str,
        items_count: u64,
        success: bool,
    ) -> Self {
        Self {
            address,
            chain,
            network,
            indexed_type,
            items_count,
            success,
            error: None,
        }
    }

    /// Set the error message (builder pattern)
    pub fn with_error(mut self, error: &'a str) -> Self {
        self.error = Some(error);
        self
    }
}

/// Repository for managing priority addresses.
pub struct PriorityAddressRepository {
    pool: SqlitePool,
}

impl PriorityAddressRepository {
    /// Create a new priority address repository.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Initialize the priority addresses table.
    pub async fn init(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS priority_addresses (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                address TEXT NOT NULL,
                chain TEXT NOT NULL,
                network TEXT NOT NULL,
                priority TEXT NOT NULL DEFAULT 'normal',
                wallet_id TEXT NOT NULL,
                registered_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                last_indexed_at DATETIME,
                is_active BOOLEAN NOT NULL DEFAULT 1,
                UNIQUE(address, chain, network, wallet_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS indexing_activities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                address TEXT NOT NULL,
                chain TEXT NOT NULL,
                network TEXT NOT NULL,
                indexed_type TEXT NOT NULL,
                items_count INTEGER NOT NULL DEFAULT 0,
                timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL DEFAULT 1,
                error TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for performance
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_priority_addr_address 
            ON priority_addresses(address)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_priority_addr_wallet 
            ON priority_addresses(wallet_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_priority_addr_chain_network
            ON priority_addresses(chain, network)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_indexing_activity_address 
            ON indexing_activities(address, chain, network)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_indexing_activity_timestamp 
            ON indexing_activities(timestamp)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Register a new priority address.
    pub async fn register_address(
        &self,
        address: &str,
        chain: &str,
        network: Network,
        priority: PriorityLevel,
        wallet_id: &str,
    ) -> Result<(), sqlx::Error> {
        let network_str = match network {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Devnet => "devnet",
        };

        let priority_str = match priority {
            PriorityLevel::High => "high",
            PriorityLevel::Normal => "normal",
            PriorityLevel::Low => "low",
        };

        sqlx::query(
            r#"
            INSERT INTO priority_addresses (address, chain, network, priority, wallet_id)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(address, chain, network, wallet_id) 
            DO UPDATE SET priority = ?4, is_active = 1
            "#,
        )
        .bind(address)
        .bind(chain)
        .bind(network_str)
        .bind(priority_str)
        .bind(wallet_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Unregister a priority address.
    pub async fn unregister_address(
        &self,
        address: &str,
        chain: &str,
        network: Network,
        wallet_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let network_str = match network {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Devnet => "devnet",
        };

        let result = sqlx::query(
            r#"
            UPDATE priority_addresses 
            SET is_active = 0
            WHERE address = ?1 AND chain = ?2 AND network = ?3 AND wallet_id = ?4
            "#,
        )
        .bind(address)
        .bind(chain)
        .bind(network_str)
        .bind(wallet_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all active priority addresses.
    pub async fn get_all_active_addresses(&self) -> Result<Vec<PriorityAddress>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT address, chain, network, priority, wallet_id, 
                   registered_at, last_indexed_at, is_active
            FROM priority_addresses
            WHERE is_active = 1
            ORDER BY 
                CASE priority
                    WHEN 'high' THEN 1
                    WHEN 'normal' THEN 2
                    WHEN 'low' THEN 3
                END,
                registered_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut addresses = Vec::new();
        for row in rows {
            let address: String = row.get("address");
            let chain: String = row.get("chain");
            let network_str: String = row.get("network");
            let priority_str: String = row.get("priority");
            let wallet_id: String = row.get("wallet_id");
            let registered_at: DateTime<Utc> = row.get("registered_at");
            let last_indexed_at: Option<DateTime<Utc>> = row.get("last_indexed_at");
            let is_active: bool = row.get("is_active");

            let network = match network_str.as_str() {
                "mainnet" => Network::Mainnet,
                "testnet" => Network::Testnet,
                "devnet" => Network::Devnet,
                _ => Network::Mainnet,
            };

            let priority = match priority_str.as_str() {
                "high" => PriorityLevel::High,
                "normal" => PriorityLevel::Normal,
                "low" => PriorityLevel::Low,
                _ => PriorityLevel::Normal,
            };

            addresses.push(PriorityAddress {
                address,
                chain,
                network,
                priority,
                wallet_id,
                registered_at,
                last_indexed_at,
                is_active,
            });
        }

        Ok(addresses)
    }

    /// Get priority addresses by wallet ID.
    pub async fn get_addresses_by_wallet(
        &self,
        wallet_id: &str,
    ) -> Result<Vec<PriorityAddress>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT address, chain, network, priority, wallet_id, 
                   registered_at, last_indexed_at, is_active
            FROM priority_addresses
            WHERE wallet_id = ?1 AND is_active = 1
            ORDER BY 
                CASE priority
                    WHEN 'high' THEN 1
                    WHEN 'normal' THEN 2
                    WHEN 'low' THEN 3
                END,
                registered_at ASC
            "#,
        )
        .bind(wallet_id)
        .fetch_all(&self.pool)
        .await?;

        let mut addresses = Vec::new();
        for row in rows {
            let address: String = row.get("address");
            let chain: String = row.get("chain");
            let network_str: String = row.get("network");
            let priority_str: String = row.get("priority");
            let wallet_id: String = row.get("wallet_id");
            let registered_at: DateTime<Utc> = row.get("registered_at");
            let last_indexed_at: Option<DateTime<Utc>> = row.get("last_indexed_at");
            let is_active: bool = row.get("is_active");

            let network = match network_str.as_str() {
                "mainnet" => Network::Mainnet,
                "testnet" => Network::Testnet,
                "devnet" => Network::Devnet,
                _ => Network::Mainnet,
            };

            let priority = match priority_str.as_str() {
                "high" => PriorityLevel::High,
                "normal" => PriorityLevel::Normal,
                "low" => PriorityLevel::Low,
                _ => PriorityLevel::Normal,
            };

            addresses.push(PriorityAddress {
                address,
                chain,
                network,
                priority,
                wallet_id,
                registered_at,
                last_indexed_at,
                is_active,
            });
        }

        Ok(addresses)
    }

    /// Get priority addresses by chain and network.
    pub async fn get_addresses_by_chain_and_network(
        &self,
        chain: &str,
        network: Network,
    ) -> Result<Vec<PriorityAddress>, sqlx::Error> {
        let network_str = match network {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Devnet => "devnet",
        };

        let rows = sqlx::query(
            r#"
            SELECT address, chain, network, priority, wallet_id, 
                   registered_at, last_indexed_at, is_active
            FROM priority_addresses
            WHERE chain = ?1 AND network = ?2 AND is_active = 1
            ORDER BY 
                CASE priority
                    WHEN 'high' THEN 1
                    WHEN 'normal' THEN 2
                    WHEN 'low' THEN 3
                END,
                registered_at ASC
            "#,
        )
        .bind(chain)
        .bind(network_str)
        .fetch_all(&self.pool)
        .await?;

        let mut addresses = Vec::new();
        for row in rows {
            let address: String = row.get("address");
            let chain: String = row.get("chain");
            let network_str: String = row.get("network");
            let priority_str: String = row.get("priority");
            let wallet_id: String = row.get("wallet_id");
            let registered_at: DateTime<Utc> = row.get("registered_at");
            let last_indexed_at: Option<DateTime<Utc>> = row.get("last_indexed_at");
            let is_active: bool = row.get("is_active");

            let network = match network_str.as_str() {
                "mainnet" => Network::Mainnet,
                "testnet" => Network::Testnet,
                "devnet" => Network::Devnet,
                _ => Network::Mainnet,
            };

            let priority = match priority_str.as_str() {
                "high" => PriorityLevel::High,
                "normal" => PriorityLevel::Normal,
                "low" => PriorityLevel::Low,
                _ => PriorityLevel::Normal,
            };

            addresses.push(PriorityAddress {
                address,
                chain,
                network,
                priority,
                wallet_id,
                registered_at,
                last_indexed_at,
                is_active,
            });
        }

        Ok(addresses)
    }

    /// Update last indexed timestamp for an address.
    pub async fn update_last_indexed_at(
        &self,
        address: &str,
        chain: &str,
        network: Network,
    ) -> Result<(), sqlx::Error> {
        let network_str = match network {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Devnet => "devnet",
        };

        sqlx::query(
            r#"
            UPDATE priority_addresses
            SET last_indexed_at = CURRENT_TIMESTAMP
            WHERE address = ?1 AND chain = ?2 AND network = ?3
            "#,
        )
        .bind(address)
        .bind(chain)
        .bind(network_str)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Record an indexing activity.
    pub async fn record_indexing_activity(
        &self,
        request: IndexingActivityRequest<'_>,
    ) -> Result<(), sqlx::Error> {
        let network_str = match request.network {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Devnet => "devnet",
        };

        sqlx::query(
            r#"
            INSERT INTO indexing_activities 
            (address, chain, network, indexed_type, items_count, success, error)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(request.address)
        .bind(request.chain)
        .bind(network_str)
        .bind(request.indexed_type)
        .bind(request.items_count as i64)
        .bind(request.success)
        .bind(request.error)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get recent indexing activities.
    pub async fn get_recent_activities(
        &self,
        limit: usize,
    ) -> Result<Vec<IndexingActivity>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT address, chain, network, indexed_type, items_count, 
                   timestamp, success, error
            FROM indexing_activities
            ORDER BY timestamp DESC
            LIMIT ?1
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut activities = Vec::new();
        for row in rows {
            let address: String = row.get("address");
            let chain: String = row.get("chain");
            let network_str: String = row.get("network");
            let indexed_type: String = row.get("indexed_type");
            let items_count: i64 = row.get("items_count");
            let timestamp: DateTime<Utc> = row.get("timestamp");
            let success: bool = row.get("success");
            let error: Option<String> = row.get("error");

            let network = match network_str.as_str() {
                "mainnet" => Network::Mainnet,
                "testnet" => Network::Testnet,
                "devnet" => Network::Devnet,
                _ => Network::Mainnet,
            };

            activities.push(IndexingActivity {
                address,
                chain,
                network,
                indexed_type,
                items_count: items_count as u64,
                timestamp,
                success,
                error,
            });
        }

        Ok(activities)
    }

    /// Get priority indexing status.
    pub async fn get_priority_indexing_status(
        &self,
    ) -> Result<PriorityIndexingStatus, sqlx::Error> {
        let total_row = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM priority_addresses
            WHERE is_active = 1
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let total_addresses: i64 = total_row.get("count");

        let recent_activities = self.get_recent_activities(20).await?;

        Ok(PriorityIndexingStatus {
            total_addresses: total_addresses as u64,
            active_indexing: 0, // Would be tracked separately if needed
            completed_indexing: total_addresses as u64,
            recent_activities,
        })
    }
}
