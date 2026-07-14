/// Prometheus metrics for the CSV Explorer indexer.
///
/// Exposes counters, gauges, and histograms for monitoring indexer health
/// and performance.
use prometheus::{CounterVec, GaugeVec, HistogramOpts, HistogramVec, Registry};
use std::sync::Arc;

use lazy_static::lazy_static;

lazy_static! {
    /// Global Prometheus registry.
    pub static ref REGISTRY: Registry = Registry::new();

    /// Counter for total blocks indexed per chain.
    pub static ref BLOCKS_INDEXED_TOTAL: CounterVec = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_blocks_indexed_total",
            "Total number of blocks indexed per chain"
        ),
        &["chain"]
    ).expect("Failed to create metric");

    /// Counter for total sanads indexed per chain.
    pub static ref SANADS_INDEXED_TOTAL: CounterVec = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_sanads_indexed_total",
            "Total number of sanads indexed per chain"
        ),
        &["chain"]
    ).expect("Failed to create metric");

    /// Counter for total seals indexed per chain.
    pub static ref SEALS_INDEXED_TOTAL: CounterVec = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_seals_indexed_total",
            "Total number of seals indexed per chain"
        ),
        &["chain"]
    ).expect("Failed to create metric");

    /// Counter for total transfers indexed per chain.
    pub static ref TRANSFERS_INDEXED_TOTAL: CounterVec = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_transfers_indexed_total",
            "Total number of transfers indexed per chain"
        ),
        &["chain"]
    ).expect("Failed to create metric");

    /// Counter for total contracts indexed per chain.
    pub static ref CONTRACTS_INDEXED_TOTAL: CounterVec = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_contracts_indexed_total",
            "Total number of contracts indexed per chain"
        ),
        &["chain"]
    ).expect("Failed to create metric");

    /// Gauge for current sync lag (blocks behind tip) per chain.
    pub static ref SYNC_LAG_SECONDS: GaugeVec = GaugeVec::new(
        prometheus::Opts::new(
            "csv_indexer_sync_lag_seconds",
            "Sync lag in seconds behind chain tip"
        ),
        &["chain"]
    ).expect("Failed to create metric");

    /// Counter for total errors per chain.
    pub static ref ERRORS_TOTAL: CounterVec = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_errors_total",
            "Total number of errors encountered per chain"
        ),
        &["chain", "error_type"]
    ).expect("Failed to create metric");

    /// Histogram for block processing time.
    pub static ref BLOCK_PROCESSING_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "csv_indexer_block_processing_duration_seconds",
            "Time to process a single block"
        ),
        &["chain"]
    ).expect("Failed to create metric");

    /// Gauge for the latest block number indexed per chain.
    pub static ref LATEST_BLOCK: GaugeVec = GaugeVec::new(
        prometheus::Opts::new(
            "csv_indexer_latest_block",
            "Latest block number indexed per chain"
        ),
        &["chain"]
    ).expect("Failed to create metric");
}

/// Initialize the metrics registry with all collectors.
pub fn init_metrics() -> Arc<Registry> {
    REGISTRY
        .register(Box::new(BLOCKS_INDEXED_TOTAL.clone()))
        .expect("Failed to register metric");
    REGISTRY
        .register(Box::new(SANADS_INDEXED_TOTAL.clone()))
        .expect("Failed to register metric");
    REGISTRY
        .register(Box::new(SEALS_INDEXED_TOTAL.clone()))
        .expect("Failed to register metric");
    REGISTRY
        .register(Box::new(TRANSFERS_INDEXED_TOTAL.clone()))
        .expect("Failed to register metric");
    REGISTRY
        .register(Box::new(CONTRACTS_INDEXED_TOTAL.clone()))
        .expect("Failed to register metric");
    REGISTRY
        .register(Box::new(SYNC_LAG_SECONDS.clone()))
        .expect("Failed to register metric");
    REGISTRY
        .register(Box::new(ERRORS_TOTAL.clone()))
        .expect("Failed to register metric");
    REGISTRY
        .register(Box::new(BLOCK_PROCESSING_DURATION.clone()))
        .expect("Failed to register metric");
    REGISTRY
        .register(Box::new(LATEST_BLOCK.clone()))
        .expect("Failed to register metric");

    Arc::new(REGISTRY.clone())
}

/// Record metrics for a processed block.
pub fn record_block_processed(
    chain: &str,
    sanads_count: u64,
    seals_count: u64,
    transfers_count: u64,
    contracts_count: u64,
    processing_time_seconds: f64,
    latest_block: u64,
) {
    BLOCKS_INDEXED_TOTAL.with_label_values(&[chain]).inc();
    SANADS_INDEXED_TOTAL
        .with_label_values(&[chain])
        .inc_by(sanads_count as f64);
    SEALS_INDEXED_TOTAL
        .with_label_values(&[chain])
        .inc_by(seals_count as f64);
    TRANSFERS_INDEXED_TOTAL
        .with_label_values(&[chain])
        .inc_by(transfers_count as f64);
    CONTRACTS_INDEXED_TOTAL
        .with_label_values(&[chain])
        .inc_by(contracts_count as f64);
    BLOCK_PROCESSING_DURATION
        .with_label_values(&[chain])
        .observe(processing_time_seconds);
    LATEST_BLOCK
        .with_label_values(&[chain])
        .set(latest_block as f64);
}

/// Record a sync lag measurement.
pub fn record_sync_lag(chain: &str, lag_seconds: f64) {
    SYNC_LAG_SECONDS
        .with_label_values(&[chain])
        .set(lag_seconds);
}

/// Record an error.
pub fn record_error(chain: &str, error_type: &str) {
    ERRORS_TOTAL.with_label_values(&[chain, error_type]).inc();
}

/// Encode all metrics in Prometheus text format.
pub fn encode_metrics() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&REGISTRY.gather(), &mut buffer) {
        return format!("Failed to encode metrics: {}", e);
    }
    String::from_utf8_lossy(&buffer).to_string()
}
