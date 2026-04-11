use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct ChainMetrics {
    pub total_requests: AtomicU64,
    pub successful_requests: AtomicU64,
    pub failed_requests: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub rate_limited_requests: AtomicU64,
    pub upstream_throttled: AtomicU64,
    pub hedged_requests: AtomicU64,
    pub ws_connections_total: AtomicU64,
    pub ws_active_connections: AtomicU64,
    pub ws_messages_relayed: AtomicU64,
    pub ws_reconnections: AtomicU64,
}

impl Default for ChainMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainMetrics {
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            rate_limited_requests: AtomicU64::new(0),
            upstream_throttled: AtomicU64::new(0),
            hedged_requests: AtomicU64::new(0),
            ws_connections_total: AtomicU64::new(0),
            ws_active_connections: AtomicU64::new(0),
            ws_messages_relayed: AtomicU64::new(0),
            ws_reconnections: AtomicU64::new(0),
        }
    }

    pub fn record_requests(&self, count: u64) {
        self.total_requests.fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_successes(&self, count: u64) {
        self.successful_requests.fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_failures(&self, count: u64) {
        self.failed_requests.fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_rate_limited(&self) {
        self.rate_limited_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_upstream_throttled(&self) {
        self.upstream_throttled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_hedged_requests(&self, count: u64) {
        self.hedged_requests.fetch_add(count, Ordering::Relaxed);
    }

    pub fn snapshot(
        &self,
        name: &str,
        active_endpoints: usize,
        total_endpoints: usize,
    ) -> ChainMetricsSnapshot {
        ChainMetricsSnapshot {
            chain: name.to_string(),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            successful_requests: self.successful_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            rate_limited_requests: self.rate_limited_requests.load(Ordering::Relaxed),
            upstream_throttled: self.upstream_throttled.load(Ordering::Relaxed),
            hedged_requests: self.hedged_requests.load(Ordering::Relaxed),
            ws_connections_total: self.ws_connections_total.load(Ordering::Relaxed),
            ws_active_connections: self.ws_active_connections.load(Ordering::Relaxed),
            ws_messages_relayed: self.ws_messages_relayed.load(Ordering::Relaxed),
            ws_reconnections: self.ws_reconnections.load(Ordering::Relaxed),
            active_endpoints,
            total_endpoints,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChainMetricsSnapshot {
    pub chain: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub rate_limited_requests: u64,
    pub upstream_throttled: u64,
    pub hedged_requests: u64,
    pub ws_connections_total: u64,
    pub ws_active_connections: u64,
    pub ws_messages_relayed: u64,
    pub ws_reconnections: u64,
    pub active_endpoints: usize,
    pub total_endpoints: usize,
}
