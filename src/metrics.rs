use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct ChainMetrics {
    pub total_requests: AtomicU64,
    pub successful_requests: AtomicU64,
    pub failed_requests: AtomicU64,
}

impl ChainMetrics {
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
        }
    }

    pub fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_success(&self) {
        self.successful_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self, name: &str, active_endpoints: usize, total_endpoints: usize) -> ChainMetricsSnapshot {
        ChainMetricsSnapshot {
            chain: name.to_string(),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            successful_requests: self.successful_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
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
    pub active_endpoints: usize,
    pub total_endpoints: usize,
}
