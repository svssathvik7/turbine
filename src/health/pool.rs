use super::state::EndpointStatus;
use super::EndpointHealth;
use crate::config::{ChainConfig, EndpointConfig, HealthConfig, HedgeConfig, RotationStrategy};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

#[derive(Debug)]
pub struct ChainPool {
    pub name: String,
    pub endpoints: Vec<EndpointConfig>,
    pub counter: AtomicUsize,
    pub health: RwLock<Vec<EndpointHealth>>,
    pub health_config: HealthConfig,
    pub rotation: RotationStrategy,
    pub chain_id: Option<u64>,
    pub hedge_config: Option<HedgeConfig>,
    /// Precomputed total weight for weighted rotation.
    total_weight: u32,
}

impl ChainPool {
    pub fn new(config: &ChainConfig) -> Self {
        let health_states = config
            .endpoints
            .iter()
            .map(|_| EndpointHealth::new())
            .collect();

        let total_weight: u32 = config.endpoints.iter().map(|e| e.weight).sum();

        Self {
            name: config.name.clone(),
            endpoints: config.endpoints.clone(),
            counter: AtomicUsize::new(0),
            health: RwLock::new(health_states),
            health_config: config.health.clone(),
            rotation: config.rotation.clone(),
            chain_id: config.chain_id,
            hedge_config: config.hedge.clone(),
            total_weight,
        }
    }

    /// Select the next healthy endpoint based on rotation strategy.
    pub fn next_endpoint(&self) -> Option<(usize, &str)> {
        match self.rotation {
            RotationStrategy::RoundRobin => self.next_round_robin(),
            RotationStrategy::Weighted => self.next_weighted(),
        }
    }

    /// Select the next healthy endpoint excluding a specific index.
    pub fn next_endpoint_excluding(&self, exclude: usize) -> Option<(usize, &str)> {
        match self.rotation {
            RotationStrategy::RoundRobin => self.next_round_robin_excluding(exclude),
            RotationStrategy::Weighted => self.next_weighted_excluding(exclude),
        }
    }

    /// Select the next healthy endpoint excluding multiple indices.
    pub fn next_endpoint_excluding_many(&self, exclude: &[usize]) -> Option<(usize, &str)> {
        match self.rotation {
            RotationStrategy::RoundRobin => {
                let len = self.endpoints.len();
                let start = self.counter.fetch_add(1, Ordering::Relaxed) % len;
                let health = self.health.read().unwrap();
                for i in 0..len {
                    let idx = (start + i) % len;
                    if exclude.contains(&idx) {
                        continue;
                    }
                    if health[idx].is_healthy {
                        return Some((idx, &self.endpoints[idx].url));
                    }
                }
                // Fallback: least recently failed excluding all
                let mut best: Option<usize> = None;
                for i in 0..len {
                    if exclude.contains(&i) {
                        continue;
                    }
                    match best {
                        None => best = Some(i),
                        Some(prev) => {
                            if health[i].failed_earlier_than(&health[prev]) {
                                best = Some(i);
                            }
                        }
                    }
                }
                best.map(|idx| (idx, self.endpoints[idx].url.as_str()))
            }
            RotationStrategy::Weighted => {
                let health = self.health.read().unwrap();
                let healthy_weight: u32 = self
                    .endpoints
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !exclude.contains(i) && health[*i].is_healthy)
                    .map(|(_, e)| e.weight)
                    .sum();
                if healthy_weight == 0 {
                    let mut best: Option<usize> = None;
                    for i in 0..self.endpoints.len() {
                        if exclude.contains(&i) {
                            continue;
                        }
                        match best {
                            None => best = Some(i),
                            Some(prev) => {
                                if health[i].failed_earlier_than(&health[prev]) {
                                    best = Some(i);
                                }
                            }
                        }
                    }
                    return best.map(|idx| (idx, self.endpoints[idx].url.as_str()));
                }
                let tick = self.counter.fetch_add(1, Ordering::Relaxed) as u32;
                let target = tick % healthy_weight;
                let mut cumulative = 0u32;
                for (i, ep) in self.endpoints.iter().enumerate() {
                    if exclude.contains(&i) || !health[i].is_healthy {
                        continue;
                    }
                    cumulative += ep.weight;
                    if target < cumulative {
                        return Some((i, &ep.url));
                    }
                }
                None
            }
        }
    }

    fn next_round_robin(&self) -> Option<(usize, &str)> {
        let len = self.endpoints.len();
        let start = self.counter.fetch_add(1, Ordering::Relaxed) % len;
        let health = self.health.read().unwrap();

        for i in 0..len {
            let idx = (start + i) % len;
            if health[idx].is_healthy {
                return Some((idx, &self.endpoints[idx].url));
            }
        }

        self.least_recently_failed(&health, None)
    }

    fn next_round_robin_excluding(&self, exclude: usize) -> Option<(usize, &str)> {
        let len = self.endpoints.len();
        let start = self.counter.fetch_add(1, Ordering::Relaxed) % len;
        let health = self.health.read().unwrap();

        for i in 0..len {
            let idx = (start + i) % len;
            if idx == exclude {
                continue;
            }
            if health[idx].is_healthy {
                return Some((idx, &self.endpoints[idx].url));
            }
        }

        self.least_recently_failed(&health, Some(exclude))
    }

    fn next_weighted(&self) -> Option<(usize, &str)> {
        let health = self.health.read().unwrap();

        // Calculate total healthy weight
        let healthy_weight: u32 = self
            .endpoints
            .iter()
            .enumerate()
            .filter(|(i, _)| health[*i].is_healthy)
            .map(|(_, e)| e.weight)
            .sum();

        if healthy_weight == 0 {
            return self.least_recently_failed(&health, None);
        }

        let tick = self.counter.fetch_add(1, Ordering::Relaxed) as u32;
        let target = tick % healthy_weight;
        let mut cumulative = 0u32;

        for (i, ep) in self.endpoints.iter().enumerate() {
            if !health[i].is_healthy {
                continue;
            }
            cumulative += ep.weight;
            if target < cumulative {
                return Some((i, &ep.url));
            }
        }

        self.least_recently_failed(&health, None)
    }

    fn next_weighted_excluding(&self, exclude: usize) -> Option<(usize, &str)> {
        let health = self.health.read().unwrap();

        let healthy_weight: u32 = self
            .endpoints
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != exclude && health[*i].is_healthy)
            .map(|(_, e)| e.weight)
            .sum();

        if healthy_weight == 0 {
            return self.least_recently_failed(&health, Some(exclude));
        }

        let tick = self.counter.fetch_add(1, Ordering::Relaxed) as u32;
        let target = tick % healthy_weight;
        let mut cumulative = 0u32;

        for (i, ep) in self.endpoints.iter().enumerate() {
            if i == exclude || !health[i].is_healthy {
                continue;
            }
            cumulative += ep.weight;
            if target < cumulative {
                return Some((i, &ep.url));
            }
        }

        self.least_recently_failed(&health, Some(exclude))
    }

    fn least_recently_failed(
        &self,
        health: &[EndpointHealth],
        exclude: Option<usize>,
    ) -> Option<(usize, &str)> {
        let mut best: Option<usize> = None;
        for i in 0..self.endpoints.len() {
            if Some(i) == exclude {
                continue;
            }
            match best {
                None => best = Some(i),
                Some(prev) => {
                    if health[i].failed_earlier_than(&health[prev]) {
                        best = Some(i);
                    }
                }
            }
        }
        best.map(|idx| (idx, self.endpoints[idx].url.as_str()))
    }

    pub fn record_success(&self, idx: usize) {
        let mut health = self.health.write().unwrap();
        health[idx].record_success();
    }

    pub fn record_failure(&self, idx: usize) {
        let mut health = self.health.write().unwrap();
        health[idx].record_failure(self.health_config.max_consecutive_failures);
    }

    pub fn mark_stale(&self, idx: usize) {
        let mut health = self.health.write().unwrap();
        health[idx].is_healthy = false;
    }

    pub fn healthy_count(&self) -> usize {
        let health = self.health.read().unwrap();
        health.iter().filter(|h| h.is_healthy).count()
    }

    pub fn total_weight(&self) -> u32 {
        self.total_weight
    }

    pub fn update_block_height(&self, idx: usize, height: u64) {
        let mut health = self.health.write().unwrap();
        health[idx].update_block_height(height);
    }

    pub fn update_latency(&self, idx: usize, latency_ms: u64) {
        let mut health = self.health.write().unwrap();
        health[idx].update_latency(latency_ms);
    }

    pub fn record_success_with_latency(&self, idx: usize, latency_ms: u64) {
        let mut health = self.health.write().unwrap();
        health[idx].record_success();
        health[idx].update_latency(latency_ms);
    }

    pub fn rotation_name(&self) -> &str {
        match self.rotation {
            RotationStrategy::RoundRobin => "round_robin",
            RotationStrategy::Weighted => "weighted",
        }
    }

    pub fn endpoint_statuses(&self) -> Vec<EndpointStatus> {
        let health = self.health.read().unwrap();
        self.endpoints
            .iter()
            .enumerate()
            .map(|(i, ep)| {
                let h = &health[i];
                EndpointStatus {
                    url: redact_url(&ep.url),
                    weight: ep.weight,
                    is_healthy: h.is_healthy,
                    consecutive_failures: h.consecutive_failures,
                    block_height: h.block_height,
                    last_latency_ms: h.last_latency_ms,
                    rolling_latency_ms: h.rolling_latency_ms,
                    request_count: h.request_count,
                    success_count: h.success_count,
                    failure_count: h.failure_count,
                }
            })
            .collect()
    }
}

/// Redact query parameters from URLs to avoid leaking API keys in the dashboard.
fn redact_url(url: &str) -> String {
    if let Some(pos) = url.find('?') {
        format!("{}?...", &url[..pos])
    } else {
        url.to_string()
    }
}
