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
    pub active_endpoints: usize,
    pub total_endpoints: usize,
}
