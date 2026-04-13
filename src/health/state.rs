use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RosterStatus {
    Active,
    Reserve,
}

#[derive(Debug)]
pub struct EndpointHealth {
    pub consecutive_failures: AtomicU32,
    last_failure: Mutex<Option<Instant>>,
    pub is_healthy: AtomicBool,
    /// When an endpoint is marked unhealthy, this records when it should be
    /// automatically re-enabled for probing (cooldown expiry).
    unhealthy_until: Mutex<Option<Instant>>,
    /// Tracks upstream 429 throttling separately from hard failures.
    /// Stores the Instant (as nanos since some epoch) until which this endpoint
    /// should be avoided due to rate limiting. Uses exponential backoff.
    throttled_until: Mutex<Option<Instant>>,
    /// Current throttle backoff duration in milliseconds — doubles on each
    /// consecutive throttle, resets on success.
    throttle_backoff_ms: AtomicU64,
    block_height: AtomicU64,
    last_latency_ms: AtomicU64,
    rolling_latency_ms: AtomicU64,
    pub request_count: AtomicU64,
    pub success_count: AtomicU64,
    pub failure_count: AtomicU64,
    pub throttle_count: AtomicU64,
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
    pub is_throttled: bool,
    pub consecutive_failures: u32,
    pub block_height: Option<u64>,
    pub last_latency_ms: Option<u64>,
    pub rolling_latency_ms: Option<f64>,
    pub request_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub throttle_count: u64,
    pub roster_status: RosterStatus,
}

/// Initial throttle backoff: 1 second.
const THROTTLE_BACKOFF_INIT_MS: u64 = 1_000;
/// Maximum throttle backoff: 30 seconds.
const THROTTLE_BACKOFF_MAX_MS: u64 = 30_000;

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
            unhealthy_until: Mutex::new(None),
            throttled_until: Mutex::new(None),
            throttle_backoff_ms: AtomicU64::new(THROTTLE_BACKOFF_INIT_MS),
            block_height: AtomicU64::new(NONE_U64),
            last_latency_ms: AtomicU64::new(NONE_U64),
            rolling_latency_ms: AtomicU64::new(f64_to_atomic(None)),
            request_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            failure_count: AtomicU64::new(0),
            throttle_count: AtomicU64::new(0),
        }
    }

    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.is_healthy.store(true, Ordering::Release);
        *self.last_failure.lock().unwrap() = None;
        *self.unhealthy_until.lock().unwrap() = None;
        // Reset throttle backoff on success
        self.throttle_backoff_ms
            .store(THROTTLE_BACKOFF_INIT_MS, Ordering::Relaxed);
        *self.throttled_until.lock().unwrap() = None;
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.success_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self, max_failures: u32, cooldown_seconds: u64) {
        let prev = self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        *self.last_failure.lock().unwrap() = Some(Instant::now());
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.failure_count.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= max_failures {
            self.is_healthy.store(false, Ordering::Release);
            // Set cooldown-based auto-recovery timer
            if cooldown_seconds > 0 {
                *self.unhealthy_until.lock().unwrap() =
                    Some(Instant::now() + Duration::from_secs(cooldown_seconds));
            }
        }
    }

    /// Record an upstream 429 throttle. Does NOT count toward consecutive
    /// failures (the endpoint is working, just rate-limiting us). Uses
    /// exponential backoff to determine how long to avoid this endpoint.
    pub fn record_throttle(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.throttle_count.fetch_add(1, Ordering::Relaxed);

        // Exponential backoff: load current, set throttled_until, then double
        let backoff_ms = self.throttle_backoff_ms.load(Ordering::Relaxed);
        *self.throttled_until.lock().unwrap() =
            Some(Instant::now() + Duration::from_millis(backoff_ms));

        // Double for next time, capped at max
        let next = (backoff_ms * 2).min(THROTTLE_BACKOFF_MAX_MS);
        self.throttle_backoff_ms.store(next, Ordering::Relaxed);
    }

    /// Check if this endpoint should be considered available for requests.
    /// Accounts for:
    /// - Hard health status (consecutive failures)
    /// - Cooldown-based auto-recovery (re-enables after cooldown_seconds)
    /// - Throttle backoff (avoids 429'd endpoints temporarily)
    pub fn is_available(&self) -> bool {
        // Check throttle first (short-lived, common case)
        if let Some(until) = *self.throttled_until.lock().unwrap() {
            if Instant::now() < until {
                return false;
            }
        }

        // Check hard health
        if self.is_healthy.load(Ordering::Acquire) {
            return true;
        }

        // Unhealthy — check if cooldown has expired (auto-recovery)
        if let Some(until) = *self.unhealthy_until.lock().unwrap() {
            if Instant::now() >= until {
                // Cooldown expired — re-enable for probing
                self.is_healthy.store(true, Ordering::Release);
                self.consecutive_failures.store(0, Ordering::Relaxed);
                *self.unhealthy_until.lock().unwrap() = None;
                return true;
            }
        }

        false
    }

    /// Returns true if currently throttled (429 backoff active).
    pub fn is_throttled(&self) -> bool {
        if let Some(until) = *self.throttled_until.lock().unwrap() {
            Instant::now() < until
        } else {
            false
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

    /// Mark unhealthy with a cooldown timer for auto-recovery.
    pub fn mark_unhealthy_with_cooldown(&self, cooldown_seconds: u64) {
        self.is_healthy.store(false, Ordering::Release);
        if cooldown_seconds > 0 {
            *self.unhealthy_until.lock().unwrap() =
                Some(Instant::now() + Duration::from_secs(cooldown_seconds));
        }
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
            is_healthy: self.is_available(),
            is_throttled: self.is_throttled(),
            block_height: self.block_height(),
            last_latency_ms: self.last_latency_ms(),
            rolling_latency_ms: self.rolling_latency_ms(),
            request_count: self.request_count.load(Ordering::Relaxed),
            success_count: self.success_count.load(Ordering::Relaxed),
            failure_count: self.failure_count.load(Ordering::Relaxed),
            throttle_count: self.throttle_count.load(Ordering::Relaxed),
        }
    }
}

/// A point-in-time copy of health data (all plain types, no atomics).
pub struct EndpointHealthSnapshot {
    pub consecutive_failures: u32,
    pub is_healthy: bool,
    pub is_throttled: bool,
    pub block_height: Option<u64>,
    pub last_latency_ms: Option<u64>,
    pub rolling_latency_ms: Option<f64>,
    pub request_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub throttle_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_status_serializes_to_lowercase() {
        let active = serde_json::to_string(&RosterStatus::Active).unwrap();
        let reserve = serde_json::to_string(&RosterStatus::Reserve).unwrap();
        assert_eq!(active, "\"active\"");
        assert_eq!(reserve, "\"reserve\"");
    }
}
