use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Process-wide reference instant for converting `Instant` to/from atomic u64.
/// All time-based atomics store nanoseconds elapsed since this point.
/// Using `once_cell::sync::Lazy` would be ideal but std's LazyLock is stable since 1.80.
static EPOCH: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

/// Convert an `Instant` to nanos since EPOCH for atomic storage.
/// Returns 0 if the instant is before EPOCH (shouldn't happen).
fn instant_to_nanos(t: Instant) -> u64 {
    t.duration_since(*EPOCH).as_nanos() as u64
}

/// Convert nanos since EPOCH back to an `Instant`.
/// Returns `None` if nanos is 0 (sentinel for "no value").
fn nanos_to_instant(nanos: u64) -> Option<Instant> {
    if nanos == 0 {
        None
    } else {
        Some(*EPOCH + Duration::from_nanos(nanos))
    }
}

/// Store an `Option<Instant>` as atomic u64 nanos. `None` → 0.
fn store_instant(atom: &AtomicU64, val: Option<Instant>) {
    let nanos = match val {
        Some(t) => instant_to_nanos(t),
        None => 0,
    };
    atom.store(nanos, Ordering::Relaxed);
}

/// Load an `Option<Instant>` from atomic u64 nanos. 0 → `None`.
fn load_instant(atom: &AtomicU64) -> Option<Instant> {
    nanos_to_instant(atom.load(Ordering::Relaxed))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RosterStatus {
    Active,
    Reserve,
}

#[derive(Debug)]
pub struct EndpointHealth {
    pub consecutive_failures: AtomicU32,
    /// Nanos since EPOCH when last failure occurred. 0 = never failed.
    last_failure_nanos: AtomicU64,
    pub is_healthy: AtomicBool,
    /// Nanos since EPOCH when auto-recovery should kick in. 0 = not set.
    unhealthy_until_nanos: AtomicU64,
    /// Nanos since EPOCH until which this endpoint should be avoided due to throttling. 0 = not throttled.
    throttled_until_nanos: AtomicU64,
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
            last_failure_nanos: AtomicU64::new(0),
            is_healthy: AtomicBool::new(true),
            unhealthy_until_nanos: AtomicU64::new(0),
            throttled_until_nanos: AtomicU64::new(0),
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
        self.last_failure_nanos.store(0, Ordering::Relaxed);
        self.unhealthy_until_nanos.store(0, Ordering::Relaxed);
        // Reset throttle backoff on success
        self.throttle_backoff_ms
            .store(THROTTLE_BACKOFF_INIT_MS, Ordering::Relaxed);
        self.throttled_until_nanos.store(0, Ordering::Relaxed);
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.success_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self, max_failures: u32, cooldown_seconds: u64) {
        let prev = self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        store_instant(&self.last_failure_nanos, Some(Instant::now()));
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.failure_count.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= max_failures {
            self.is_healthy.store(false, Ordering::Release);
            // Set cooldown-based auto-recovery timer
            if cooldown_seconds > 0 {
                let until = Instant::now() + Duration::from_secs(cooldown_seconds);
                store_instant(&self.unhealthy_until_nanos, Some(until));
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
        let until = Instant::now() + Duration::from_millis(backoff_ms);
        store_instant(&self.throttled_until_nanos, Some(until));

        // Double for next time, capped at max
        let next = (backoff_ms * 2).min(THROTTLE_BACKOFF_MAX_MS);
        self.throttle_backoff_ms.store(next, Ordering::Relaxed);
    }

    /// Check if this endpoint should be considered available for requests.
    /// Fully lock-free — uses only atomic loads.
    pub fn is_available(&self) -> bool {
        // Check throttle first (short-lived, common case)
        if let Some(until) = load_instant(&self.throttled_until_nanos) {
            if Instant::now() < until {
                return false;
            }
        }

        // Check hard health
        if self.is_healthy.load(Ordering::Acquire) {
            return true;
        }

        // Unhealthy — check if cooldown has expired (auto-recovery)
        if let Some(until) = load_instant(&self.unhealthy_until_nanos) {
            if Instant::now() >= until {
                // Cooldown expired — re-enable for probing
                self.is_healthy.store(true, Ordering::Release);
                self.consecutive_failures.store(0, Ordering::Relaxed);
                self.unhealthy_until_nanos.store(0, Ordering::Relaxed);
                return true;
            }
        }

        false
    }

    /// Returns true if currently throttled (429 backoff active).
    pub fn is_throttled(&self) -> bool {
        if let Some(until) = load_instant(&self.throttled_until_nanos) {
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
            let until = Instant::now() + Duration::from_secs(cooldown_seconds);
            store_instant(&self.unhealthy_until_nanos, Some(until));
        }
    }

    /// Returns true if this endpoint failed earlier than `other`.
    /// Used to pick the least-recently-failed endpoint when all are unhealthy.
    pub fn failed_earlier_than(&self, other: &EndpointHealth) -> bool {
        let self_nanos = self.last_failure_nanos.load(Ordering::Relaxed);
        let other_nanos = other.last_failure_nanos.load(Ordering::Relaxed);
        match (self_nanos, other_nanos) {
            (0, 0) => false, // neither failed
            (0, _) => true,  // self never failed — wins
            (_, 0) => false, // other never failed — other wins
            (a, b) => a < b, // earlier timestamp wins
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

    #[test]
    fn is_available_lock_free() {
        let health = EndpointHealth::new();
        // Healthy endpoint is available
        assert!(health.is_available());

        // Record throttle — should be unavailable briefly
        health.record_throttle();
        assert!(!health.is_available());
        assert!(health.is_throttled());
    }

    #[test]
    fn failed_earlier_than_lock_free() {
        // Force EPOCH to initialize before creating endpoints
        let _ = *EPOCH;
        std::thread::sleep(std::time::Duration::from_millis(1));

        let a = EndpointHealth::new();
        let b = EndpointHealth::new();

        // Neither failed — a is not earlier
        assert!(!a.failed_earlier_than(&b));

        // a fails first
        a.record_failure(10, 60);
        // a failed, b never — a is not earlier (b never-failed wins)
        assert!(!a.failed_earlier_than(&b));
        // b asks if it failed earlier than a — b never failed, so yes
        assert!(b.failed_earlier_than(&a));
    }
}
