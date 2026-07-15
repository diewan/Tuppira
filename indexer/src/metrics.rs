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
    pub static ref BLOCKS_INDEXED_TOTAL: Result<CounterVec, prometheus::Error> = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_blocks_indexed_total",
            "Total number of blocks indexed per chain"
        ),
        &["chain"]
    );

    /// Counter for total sanads indexed per chain.
    pub static ref SANADS_INDEXED_TOTAL: Result<CounterVec, prometheus::Error> = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_sanads_indexed_total",
            "Total number of sanads indexed per chain"
        ),
        &["chain"]
    );

    /// Counter for total seals indexed per chain.
    pub static ref SEALS_INDEXED_TOTAL: Result<CounterVec, prometheus::Error> = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_seals_indexed_total",
            "Total number of seals indexed per chain"
        ),
        &["chain"]
    );

    /// Counter for total transfers indexed per chain.
    pub static ref TRANSFERS_INDEXED_TOTAL: Result<CounterVec, prometheus::Error> = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_transfers_indexed_total",
            "Total number of transfers indexed per chain"
        ),
        &["chain"]
    );

    /// Counter for total contracts indexed per chain.
    pub static ref CONTRACTS_INDEXED_TOTAL: Result<CounterVec, prometheus::Error> = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_contracts_indexed_total",
            "Total number of contracts indexed per chain"
        ),
        &["chain"]
    );

    /// Gauge for current sync lag (blocks behind tip) per chain.
    pub static ref SYNC_LAG_SECONDS: Result<GaugeVec, prometheus::Error> = GaugeVec::new(
        prometheus::Opts::new(
            "csv_indexer_sync_lag_seconds",
            "Sync lag in seconds behind chain tip"
        ),
        &["chain"]
    );

    /// Counter for total errors per chain.
    pub static ref ERRORS_TOTAL: Result<CounterVec, prometheus::Error> = CounterVec::new(
        prometheus::Opts::new(
            "csv_indexer_errors_total",
            "Total number of errors encountered per chain"
        ),
        &["chain", "error_type"]
    );

    /// Histogram for block processing time.
    pub static ref BLOCK_PROCESSING_DURATION: Result<HistogramVec, prometheus::Error> = HistogramVec::new(
        HistogramOpts::new(
            "csv_indexer_block_processing_duration_seconds",
            "Time to process a single block"
        ),
        &["chain"]
    );

    /// Gauge for the latest block number indexed per chain.
    pub static ref LATEST_BLOCK: Result<GaugeVec, prometheus::Error> = GaugeVec::new(
        prometheus::Opts::new(
            "csv_indexer_latest_block",
            "Latest block number indexed per chain"
        ),
        &["chain"]
    );
}

/// Initialize the metrics registry with all collectors.
pub fn init_metrics() -> Result<Arc<Registry>, String> {
    REGISTRY
        .register(Box::new(
            BLOCKS_INDEXED_TOTAL
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;
    REGISTRY
        .register(Box::new(
            SANADS_INDEXED_TOTAL
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;
    REGISTRY
        .register(Box::new(
            SEALS_INDEXED_TOTAL
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;
    REGISTRY
        .register(Box::new(
            TRANSFERS_INDEXED_TOTAL
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;
    REGISTRY
        .register(Box::new(
            CONTRACTS_INDEXED_TOTAL
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;
    REGISTRY
        .register(Box::new(
            SYNC_LAG_SECONDS
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;
    REGISTRY
        .register(Box::new(
            ERRORS_TOTAL
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;
    REGISTRY
        .register(Box::new(
            BLOCK_PROCESSING_DURATION
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;
    REGISTRY
        .register(Box::new(
            LATEST_BLOCK
                .as_ref()
                .map_err(|error| error.to_string())?
                .clone(),
        ))
        .map_err(|error| error.to_string())?;

    Ok(Arc::new(REGISTRY.clone()))
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
    if let Ok(metric) = &*BLOCKS_INDEXED_TOTAL {
        metric.with_label_values(&[chain]).inc();
    }
    if let Ok(metric) = &*SANADS_INDEXED_TOTAL {
        metric
            .with_label_values(&[chain])
            .inc_by(sanads_count as f64);
    }
    if let Ok(metric) = &*SEALS_INDEXED_TOTAL {
        metric
            .with_label_values(&[chain])
            .inc_by(seals_count as f64);
    }
    if let Ok(metric) = &*TRANSFERS_INDEXED_TOTAL {
        metric
            .with_label_values(&[chain])
            .inc_by(transfers_count as f64);
    }
    if let Ok(metric) = &*CONTRACTS_INDEXED_TOTAL {
        metric
            .with_label_values(&[chain])
            .inc_by(contracts_count as f64);
    }
    if let Ok(metric) = &*BLOCK_PROCESSING_DURATION {
        metric
            .with_label_values(&[chain])
            .observe(processing_time_seconds);
    }
    if let Ok(metric) = &*LATEST_BLOCK {
        metric.with_label_values(&[chain]).set(latest_block as f64);
    }
}

/// Record a sync lag measurement.
pub fn record_sync_lag(chain: &str, lag_seconds: f64) {
    if let Ok(metric) = &*SYNC_LAG_SECONDS {
        metric.with_label_values(&[chain]).set(lag_seconds);
    }
}

/// Record an error.
pub fn record_error(chain: &str, error_type: &str) {
    if let Ok(metric) = &*ERRORS_TOTAL {
        metric.with_label_values(&[chain, error_type]).inc();
    }
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
