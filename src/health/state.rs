use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug)]
pub struct EndpointHealth {
    pub consecutive_failures: AtomicU32,
    last_failure: Mutex<Option<Instant>>,
    pub is_healthy: AtomicBool,
    block_height: AtomicU64,
    last_latency_ms: AtomicU64,
    rolling_latency_ms: AtomicU64,
    pub request_count: AtomicU64,
    pub success_count: AtomicU64,
    pub failure_count: AtomicU64,
}

// Sentinel value for "no data" in atomic u64 fields
const NONE_U64: u64 = 0;

/// Store f64 as u64 bits in an atomic. Returns NONE_U64 sentinel for None.
fn f64_to_atomic(val: Option<f64>) -> u64 {
    match val {
        Some(v) => v.to_bits(),
        None => NONE_U64,
    }
}

/// Load f64 from atomic u64 bits. Returns None if sentinel.
fn atomic_to_f64(bits: u64) -> Option<f64> {
    if bits == NONE_U64 {
        None
    } else {
        Some(f64::from_bits(bits))
    }
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

impl Default for EndpointHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl EndpointHealth {
    pub fn new() -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            last_failure: Mutex::new(None),
            is_healthy: AtomicBool::new(true),
            block_height: AtomicU64::new(NONE_U64),
            last_latency_ms: AtomicU64::new(NONE_U64),
            rolling_latency_ms: AtomicU64::new(f64_to_atomic(None)),
            request_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            failure_count: AtomicU64::new(0),
        }
    }

    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.is_healthy.store(true, Ordering::Release);
        *self.last_failure.lock().unwrap() = None;
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.success_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self, max_failures: u32) {
        let prev = self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        *self.last_failure.lock().unwrap() = Some(Instant::now());
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.failure_count.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= max_failures {
            self.is_healthy.store(false, Ordering::Release);
        }
    }

    pub fn update_block_height(&self, height: u64) {
        self.block_height.store(height, Ordering::Relaxed);
    }

    pub fn update_latency(&self, latency_ms: u64) {
        self.last_latency_ms.store(latency_ms, Ordering::Relaxed);

        // CAS loop for rolling average: new = prev * 0.7 + current * 0.3
        loop {
            let old_bits = self.rolling_latency_ms.load(Ordering::Relaxed);
            let new_val = match atomic_to_f64(old_bits) {
                Some(prev) => prev * 0.7 + latency_ms as f64 * 0.3,
                None => latency_ms as f64,
            };
            let new_bits = new_val.to_bits();
            match self.rolling_latency_ms.compare_exchange_weak(
                old_bits,
                new_bits,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(_) => continue,
            }
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::Acquire)
    }

    pub fn block_height(&self) -> Option<u64> {
        let v = self.block_height.load(Ordering::Relaxed);
        if v == NONE_U64 {
            None
        } else {
            Some(v)
        }
    }

    pub fn last_latency_ms(&self) -> Option<u64> {
        let v = self.last_latency_ms.load(Ordering::Relaxed);
        if v == NONE_U64 {
            None
        } else {
            Some(v)
        }
    }

    pub fn rolling_latency_ms(&self) -> Option<f64> {
        atomic_to_f64(self.rolling_latency_ms.load(Ordering::Relaxed))
    }

    pub fn mark_unhealthy(&self) {
        self.is_healthy.store(false, Ordering::Release);
    }

    /// Returns true if this endpoint failed earlier than `other`.
    /// Used to pick the least-recently-failed endpoint when all are unhealthy.
    pub fn failed_earlier_than(&self, other: &EndpointHealth) -> bool {
        let self_failure = *self.last_failure.lock().unwrap();
        let other_failure = *other.last_failure.lock().unwrap();
        match (self_failure, other_failure) {
            (Some(a), Some(b)) => a < b,
            (None, Some(_)) => true,
            _ => false,
        }
    }

    /// Snapshot for status/dashboard reporting.
    pub fn snapshot(&self) -> EndpointHealthSnapshot {
        EndpointHealthSnapshot {
            consecutive_failures: self.consecutive_failures.load(Ordering::Relaxed),
            is_healthy: self.is_healthy(),
            block_height: self.block_height(),
            last_latency_ms: self.last_latency_ms(),
            rolling_latency_ms: self.rolling_latency_ms(),
            request_count: self.request_count.load(Ordering::Relaxed),
            success_count: self.success_count.load(Ordering::Relaxed),
            failure_count: self.failure_count.load(Ordering::Relaxed),
        }
    }
}

/// A point-in-time copy of health data (all plain types, no atomics).
pub struct EndpointHealthSnapshot {
    pub consecutive_failures: u32,
    pub is_healthy: bool,
    pub block_height: Option<u64>,
    pub last_latency_ms: Option<u64>,
    pub rolling_latency_ms: Option<f64>,
    pub request_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
}
