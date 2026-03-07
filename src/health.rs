use std::time::Instant;

#[derive(Debug)]
pub struct EndpointHealth {
    pub consecutive_failures: u32,
    pub last_failure: Option<Instant>,
    pub is_healthy: bool,
}

impl EndpointHealth {
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_failure: None,
            is_healthy: true,
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.is_healthy = true;
        self.last_failure = None;
    }

    pub fn record_failure(&mut self, max_failures: u32) {
        self.consecutive_failures += 1;
        self.last_failure = Some(Instant::now());
        if self.consecutive_failures >= max_failures {
            self.is_healthy = false;
        }
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
