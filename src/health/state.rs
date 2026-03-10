use serde::Serialize;
use std::time::Instant;

#[derive(Debug)]
pub struct EndpointHealth {
    pub consecutive_failures: u32,
    pub last_failure: Option<Instant>,
    pub is_healthy: bool,
    pub block_height: Option<u64>,
    pub last_latency_ms: Option<u64>,
    pub rolling_latency_ms: Option<f64>,
    pub request_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
}

#[derive(Debug, Serialize)]
pub struct EndpointStatus {
    pub url: String,
    pub weight: u32,
    pub is_healthy: bool,
    pub consecutive_failures: u32,
    pub block_height: Option<u64>,
    pub last_latency_ms: Option<u64>,
    pub rolling_latency_ms: Option<f64>,
    pub request_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
}

impl EndpointHealth {
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_failure: None,
            is_healthy: true,
            block_height: None,
            last_latency_ms: None,
            rolling_latency_ms: None,
            request_count: 0,
            success_count: 0,
            failure_count: 0,
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.is_healthy = true;
        self.last_failure = None;
        self.request_count += 1;
        self.success_count += 1;
    }

    pub fn record_failure(&mut self, max_failures: u32) {
        self.consecutive_failures += 1;
        self.last_failure = Some(Instant::now());
        self.request_count += 1;
        self.failure_count += 1;
        if self.consecutive_failures >= max_failures {
            self.is_healthy = false;
        }
    }

    pub fn update_block_height(&mut self, height: u64) {
        self.block_height = Some(height);
    }

    pub fn update_latency(&mut self, latency_ms: u64) {
        self.last_latency_ms = Some(latency_ms);
        self.rolling_latency_ms = Some(match self.rolling_latency_ms {
            Some(prev) => prev * 0.7 + latency_ms as f64 * 0.3,
            None => latency_ms as f64,
        });
    }

    pub fn should_retry(&self, cooldown_secs: u64) -> bool {
        if self.is_healthy {
            return true;
        }
        match self.last_failure {
            Some(last) => last.elapsed().as_secs() >= cooldown_secs,
            None => true,
        }
    }

    /// Returns true if this endpoint failed earlier than `other`.
    /// Used to pick the least-recently-failed endpoint when all are unhealthy.
    pub fn failed_earlier_than(&self, other: &EndpointHealth) -> bool {
        match (self.last_failure, other.last_failure) {
            (Some(a), Some(b)) => a < b,
            (None, Some(_)) => true,
            _ => false,
        }
    }
}
