use crate::config::{ChainConfig, HealthConfig};
use crate::health::EndpointHealth;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

#[derive(Debug)]
pub struct ChainPool {
    pub name: String,
    pub endpoints: Vec<String>,
    pub counter: AtomicUsize,
    pub health: RwLock<Vec<EndpointHealth>>,
    pub health_config: HealthConfig,
}

impl ChainPool {
    pub fn new(config: &ChainConfig) -> Self {
        let health_states = config
            .endpoints
            .iter()
            .map(|_| EndpointHealth::new())
            .collect();

        Self {
            name: config.name.clone(),
            endpoints: config.endpoints.clone(),
            counter: AtomicUsize::new(0),
            health: RwLock::new(health_states),
            health_config: config.health.clone(),
        }
    }

    /// Select the next healthy endpoint using round-robin.
    /// Returns (index, url) or None if all endpoints are down.
    pub fn next_endpoint(&self) -> Option<(usize, &str)> {
        let len = self.endpoints.len();
        let start = self.counter.fetch_add(1, Ordering::Relaxed) % len;
        let health = self.health.read().unwrap();

        // First pass: find a healthy endpoint
        for i in 0..len {
            let idx = (start + i) % len;
            if health[idx].is_healthy {
                return Some((idx, &self.endpoints[idx]));
            }
        }

        // Second pass: pick the least-recently-failed endpoint
        let mut best: Option<usize> = None;
        for i in 0..len {
            let idx = (start + i) % len;
            match best {
                None => best = Some(idx),
                Some(prev) => {
                    if health[idx].failed_earlier_than(&health[prev]) {
                        best = Some(idx);
                    }
                }
            }
        }

        best.map(|idx| (idx, self.endpoints[idx].as_str()))
    }

    /// Select the next healthy endpoint, skipping a specific index (used for retries).
    pub fn next_endpoint_excluding(&self, exclude: usize) -> Option<(usize, &str)> {
        let len = self.endpoints.len();
        let start = self.counter.fetch_add(1, Ordering::Relaxed) % len;
        let health = self.health.read().unwrap();

        // First pass: find a healthy endpoint excluding the given index
        for i in 0..len {
            let idx = (start + i) % len;
            if idx == exclude {
                continue;
            }
            if health[idx].is_healthy {
                return Some((idx, &self.endpoints[idx]));
            }
        }

        // Second pass: pick the least-recently-failed endpoint excluding the given index
        let mut best: Option<usize> = None;
        for i in 0..len {
            let idx = (start + i) % len;
            if idx == exclude {
                continue;
            }
            match best {
                None => best = Some(idx),
                Some(prev) => {
                    if health[idx].failed_earlier_than(&health[prev]) {
                        best = Some(idx);
                    }
                }
            }
        }

        best.map(|idx| (idx, self.endpoints[idx].as_str()))
    }

    pub fn record_success(&self, idx: usize) {
        let mut health = self.health.write().unwrap();
        health[idx].record_success();
    }

    pub fn record_failure(&self, idx: usize) {
        let mut health = self.health.write().unwrap();
        health[idx].record_failure(self.health_config.max_consecutive_failures);
    }

    pub fn healthy_count(&self) -> usize {
        let health = self.health.read().unwrap();
        health.iter().filter(|h| h.is_healthy).count()
    }
}
