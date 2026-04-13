use super::state::EndpointStatus;
use super::state::RosterStatus;
use super::EndpointHealth;
use crate::config::{ChainConfig, EndpointConfig, HealthConfig, HedgeConfig, RotationStrategy};
use arc_swap::ArcSwap;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct ChainPool {
    pub name: String,
    pub endpoints: Vec<EndpointConfig>,
    pub counter: AtomicUsize,
    pub health: Vec<EndpointHealth>,
    pub health_config: HealthConfig,
    pub rotation: RotationStrategy,
    pub chain_id: Option<u64>,
    pub hedge_config: Option<HedgeConfig>,
    /// Precomputed total weight for weighted rotation.
    total_weight: u32,
    /// Indices of endpoints currently receiving traffic and health checks.
    /// Uses ArcSwap for lock-free reads on the hot path (every request).
    pub active_indices: ArcSwap<Vec<usize>>,
    /// Reserve queue — FIFO. Front = next promotion candidate.
    pub rpc_house: ArcSwap<VecDeque<usize>>,
}

const ACTIVE_SET_SIZE: usize = 5;

impl ChainPool {
    pub fn new(config: &ChainConfig) -> Self {
        let health_states = config
            .endpoints
            .iter()
            .map(|_| EndpointHealth::new())
            .collect();

        let total_weight: u32 = config.endpoints.iter().map(|e| e.weight).sum();

        let num_endpoints = config.endpoints.len();
        let mut all_indices: Vec<usize> = (0..num_endpoints).collect();
        all_indices.shuffle(&mut thread_rng());

        let active_size = ACTIVE_SET_SIZE.min(num_endpoints);
        let active_indices: Vec<usize> = all_indices[..active_size].to_vec();
        let rpc_house: VecDeque<usize> = all_indices[active_size..].iter().copied().collect();

        Self {
            name: config.name.clone(),
            endpoints: config.endpoints.clone(),
            counter: AtomicUsize::new(0),
            health: health_states,
            health_config: config.health.clone(),
            rotation: config.rotation.clone(),
            chain_id: config.chain_id,
            hedge_config: config.hedge.clone(),
            total_weight,
            active_indices: ArcSwap::from_pointee(active_indices),
            rpc_house: ArcSwap::from_pointee(rpc_house),
        }
    }

    /// Select the next healthy endpoint based on rotation strategy.
    pub fn next_endpoint(&self) -> Option<(usize, &str)> {
        let active = self.active_indices.load();
        match self.rotation {
            RotationStrategy::RoundRobin | RotationStrategy::Weighted => {
                self.next_endpoint_from_eligible(&active, &[])
            }
            RotationStrategy::Latency => self.next_latency_based(&active, &[]),
        }
    }

    /// Select the next healthy endpoint excluding a specific index.
    pub fn next_endpoint_excluding(&self, exclude: usize) -> Option<(usize, &str)> {
        let active = self.active_indices.load();
        match self.rotation {
            RotationStrategy::RoundRobin | RotationStrategy::Weighted => {
                self.next_endpoint_from_eligible(&active, &[exclude])
            }
            RotationStrategy::Latency => self.next_latency_based(&active, &[exclude]),
        }
    }

    /// Select the next healthy endpoint excluding multiple indices.
    pub fn next_endpoint_excluding_many(&self, exclude: &[usize]) -> Option<(usize, &str)> {
        let active = self.active_indices.load();
        match self.rotation {
            RotationStrategy::RoundRobin | RotationStrategy::Weighted => {
                self.next_endpoint_from_eligible(&active, exclude)
            }
            RotationStrategy::Latency => self.next_latency_based(&active, exclude),
        }
    }

    /// Latency-based selection: effective_weight = user_weight × (1.0 / rolling_latency_ms).
    /// Falls back to user_weight alone when no latency data exists (cold start).
    fn next_latency_based(&self, candidates: &[usize], exclude: &[usize]) -> Option<(usize, &str)> {
        let mut effective: Vec<(usize, f64)> = Vec::new();
        for &idx in candidates {
            if exclude.contains(&idx) || !self.health[idx].is_available() {
                continue;
            }
            let user_w = self.endpoints[idx].weight as f64;
            let eff = match self.health[idx].rolling_latency_ms() {
                Some(lat) if lat > 0.0 => user_w / lat,
                _ => user_w, // cold start: use weight alone
            };
            effective.push((idx, eff));
        }

        if effective.is_empty() {
            // Fallback: least recently failed within candidates
            let mut best: Option<usize> = None;
            for &idx in candidates {
                if exclude.contains(&idx) {
                    continue;
                }
                match best {
                    None => best = Some(idx),
                    Some(prev) => {
                        if self.health[idx].failed_earlier_than(&self.health[prev]) {
                            best = Some(idx);
                        }
                    }
                }
            }
            return best.map(|idx| (idx, self.endpoints[idx].url.as_str()));
        }

        let total: f64 = effective.iter().map(|(_, w)| w).sum();
        let tick = self.counter.fetch_add(1, Ordering::Relaxed) as f64;
        let target = tick % (total * 1000.0) / 1000.0;
        let mut cumulative = 0.0;
        for &(idx, weight) in &effective {
            cumulative += weight;
            if target < cumulative {
                return Some((idx, &self.endpoints[idx].url));
            }
        }

        effective
            .last()
            .map(|&(idx, _)| (idx, self.endpoints[idx].url.as_str()))
    }

    /// Compute which endpoint indices are eligible to serve a given method.
    ///
    /// - If any endpoint declares `methods` containing `method`, return only those indices.
    /// - Otherwise, return indices of endpoints with no `methods` restriction (unconstrained).
    pub fn eligible_indices_for_method(&self, method: &str) -> Vec<usize> {
        let active = self.active_indices.load();

        let claimed: Vec<usize> = self
            .endpoints
            .iter()
            .enumerate()
            .filter(|(i, ep)| {
                active.contains(i)
                    && ep
                        .methods
                        .as_ref()
                        .map(|m| m.iter().any(|s| s == method))
                        .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect();

        if !claimed.is_empty() {
            return claimed;
        }

        // No endpoint claims this method — use unconstrained endpoints
        self.endpoints
            .iter()
            .enumerate()
            .filter(|(i, ep)| active.contains(i) && ep.methods.is_none())
            .map(|(i, _)| i)
            .collect()
    }

    /// Select the next healthy endpoint from a pre-computed eligible set.
    /// Falls back to least-recently-failed within the eligible set if all are unhealthy.
    /// Returns None if eligible set is empty.
    pub fn next_endpoint_from_eligible(
        &self,
        eligible: &[usize],
        exclude: &[usize],
    ) -> Option<(usize, &str)> {
        if eligible.is_empty() {
            return None;
        }

        match self.rotation {
            RotationStrategy::RoundRobin => {
                let start = self.counter.fetch_add(1, Ordering::Relaxed);
                for i in 0..eligible.len() {
                    let idx = eligible[(start + i) % eligible.len()];
                    if exclude.contains(&idx) {
                        continue;
                    }
                    if self.health[idx].is_available() {
                        return Some((idx, &self.endpoints[idx].url));
                    }
                }
                // Fallback: least recently failed within eligible set
                let mut best: Option<usize> = None;
                for &idx in eligible {
                    if exclude.contains(&idx) {
                        continue;
                    }
                    match best {
                        None => best = Some(idx),
                        Some(prev) => {
                            if self.health[idx].failed_earlier_than(&self.health[prev]) {
                                best = Some(idx);
                            }
                        }
                    }
                }
                best.map(|idx| (idx, self.endpoints[idx].url.as_str()))
            }
            RotationStrategy::Weighted => {
                let healthy_weight: u32 = eligible
                    .iter()
                    .filter(|&&i| !exclude.contains(&i) && self.health[i].is_healthy())
                    .map(|&i| self.endpoints[i].weight)
                    .sum();

                if healthy_weight == 0 {
                    let mut best: Option<usize> = None;
                    for &idx in eligible {
                        if exclude.contains(&idx) {
                            continue;
                        }
                        match best {
                            None => best = Some(idx),
                            Some(prev) => {
                                if self.health[idx].failed_earlier_than(&self.health[prev]) {
                                    best = Some(idx);
                                }
                            }
                        }
                    }
                    return best.map(|idx| (idx, self.endpoints[idx].url.as_str()));
                }

                let tick = self.counter.fetch_add(1, Ordering::Relaxed) as u32;
                let target = tick % healthy_weight;
                let mut cumulative = 0u32;
                for &idx in eligible {
                    if exclude.contains(&idx) || !self.health[idx].is_available() {
                        continue;
                    }
                    cumulative += self.endpoints[idx].weight;
                    if target < cumulative {
                        return Some((idx, &self.endpoints[idx].url));
                    }
                }
                None
            }
            RotationStrategy::Latency => self.next_latency_based(eligible, exclude),
        }
    }

    pub fn record_success(&self, idx: usize) {
        self.health[idx].record_success();
    }

    pub fn record_failure(&self, idx: usize) {
        self.health[idx].record_failure(
            self.health_config.max_consecutive_failures,
            self.health_config.cooldown_seconds,
        );
    }

    /// Record an upstream 429 throttle — does NOT count as a hard failure.
    pub fn record_throttle(&self, idx: usize) {
        self.health[idx].record_throttle();
    }

    pub fn mark_stale(&self, idx: usize) {
        self.health[idx].mark_unhealthy_with_cooldown(self.health_config.cooldown_seconds);
    }

    pub fn healthy_count(&self) -> usize {
        let active = self.active_indices.load();
        active
            .iter()
            .filter(|&&i| self.health[i].is_available())
            .count()
    }

    pub fn total_weight(&self) -> u32 {
        self.total_weight
    }

    pub fn update_block_height(&self, idx: usize, height: u64) {
        self.health[idx].update_block_height(height);
    }

    pub fn update_latency(&self, idx: usize, latency_ms: u64) {
        self.health[idx].update_latency(latency_ms);
    }

    pub fn record_success_with_latency(&self, idx: usize, latency_ms: u64) {
        self.health[idx].record_success();
        self.health[idx].update_latency(latency_ms);
    }

    /// Demote an active endpoint to the rpc_house and promote a replacement.
    /// If rpc_house is empty (after pushing the demoted endpoint), recycles
    /// all non-active indices back into the queue.
    pub fn demote_and_replace(&self, idx: usize) {
        // Load current state
        let current_active = self.active_indices.load();
        let current_house = self.rpc_house.load();

        let mut new_active: Vec<usize> = current_active.iter().copied().collect();
        let mut new_house: VecDeque<usize> = current_house.as_ref().clone();

        // Remove from active set
        if let Some(pos) = new_active.iter().position(|&i| i == idx) {
            new_active.remove(pos);
        } else {
            return; // Not in active set, nothing to do
        }

        // Push demoted to back of rpc_house
        new_house.push_back(idx);

        // If rpc_house only contains the just-demoted endpoint, recycle
        if new_house.len() == 1 {
            let mut recyclable: Vec<usize> = (0..self.endpoints.len())
                .filter(|i| !new_active.contains(i) && *i != idx)
                .collect();
            recyclable.shuffle(&mut thread_rng());
            for r in recyclable {
                new_house.push_back(r);
            }
        }

        // Pop front as replacement (guaranteed != idx since idx is at back)
        if let Some(replacement) = new_house.pop_front() {
            new_active.push(replacement);
        }

        // Atomically swap both
        self.active_indices.store(Arc::new(new_active));
        self.rpc_house.store(Arc::new(new_house));
    }

    pub fn rotation_name(&self) -> &str {
        match self.rotation {
            RotationStrategy::RoundRobin => "round_robin",
            RotationStrategy::Weighted => "weighted",
            RotationStrategy::Latency => "latency",
        }
    }

    pub fn endpoint_statuses(&self) -> Vec<EndpointStatus> {
        let active = self.active_indices.load();
        self.endpoints
            .iter()
            .enumerate()
            .map(|(i, ep)| {
                let snap = self.health[i].snapshot();
                let roster_status = if active.contains(&i) {
                    RosterStatus::Active
                } else {
                    RosterStatus::Reserve
                };
                EndpointStatus {
                    url: redact_url(&ep.url),
                    weight: ep.weight,
                    is_healthy: snap.is_healthy,
                    is_throttled: snap.is_throttled,
                    consecutive_failures: snap.consecutive_failures,
                    block_height: snap.block_height,
                    last_latency_ms: snap.last_latency_ms,
                    rolling_latency_ms: snap.rolling_latency_ms,
                    request_count: snap.request_count,
                    success_count: snap.success_count,
                    failure_count: snap.failure_count,
                    throttle_count: snap.throttle_count,
                    roster_status,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChainConfig, EndpointConfig, HealthConfig, RotationStrategy};

    fn make_config(endpoints: Vec<EndpointConfig>) -> ChainConfig {
        ChainConfig {
            name: "test".to_string(),
            route: "/test".to_string(),
            endpoints,
            health: HealthConfig {
                max_consecutive_failures: 3,
                cooldown_seconds: 30,
                health_method: None,
                health_check_interval_seconds: 30,
                max_block_lag: 10,
                max_retries: 1,
                retry_delay_ms: 0,
            },
            rotation: RotationStrategy::RoundRobin,
            cache: None,
            chain_id: None,
            rate_limit: None,
            hedge: None,
        }
    }

    fn ep(url: &str, methods: Option<Vec<&str>>) -> EndpointConfig {
        EndpointConfig {
            url: url.to_string(),
            weight: 1,
            auth: None,
            methods: methods.map(|m| m.into_iter().map(String::from).collect()),
            ws_url: None,
        }
    }

    #[test]
    fn all_unconstrained_returns_all_indices() {
        let config = make_config(vec![ep("https://a.com", None), ep("https://b.com", None)]);
        let pool = ChainPool::new(&config);
        let eligible = pool.eligible_indices_for_method("eth_call");
        assert_eq!(eligible, vec![0, 1]);
    }

    #[test]
    fn claimed_method_routes_only_to_claiming_endpoint() {
        let config = make_config(vec![
            ep("https://private.com", Some(vec!["eth_sendRawTransaction"])),
            ep("https://public.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let eligible = pool.eligible_indices_for_method("eth_sendRawTransaction");
        assert_eq!(eligible, vec![0]);
    }

    #[test]
    fn unconstrained_endpoint_excluded_from_claimed_method() {
        let config = make_config(vec![
            ep("https://private.com", Some(vec!["eth_sendRawTransaction"])),
            ep("https://public.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let eligible = pool.eligible_indices_for_method("eth_call");
        assert_eq!(eligible, vec![1]);
    }

    #[test]
    fn unclaimed_method_returns_only_unconstrained_endpoints() {
        let config = make_config(vec![
            ep("https://private.com", Some(vec!["eth_sendRawTransaction"])),
            ep("https://public-a.com", None),
            ep("https://public-b.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let eligible = pool.eligible_indices_for_method("eth_getBalance");
        assert_eq!(eligible, vec![1, 2]);
    }

    #[test]
    fn next_endpoint_from_eligible_returns_none_on_empty_set() {
        let config = make_config(vec![ep("https://a.com", None)]);
        let pool = ChainPool::new(&config);
        let result = pool.next_endpoint_from_eligible(&[], &[]);
        assert!(result.is_none());
    }

    #[test]
    fn next_endpoint_from_eligible_selects_from_eligible_only() {
        let config = make_config(vec![
            ep("https://private.com", Some(vec!["eth_sendRawTransaction"])),
            ep("https://public.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let eligible = vec![0]; // only the private endpoint
        let (idx, url) = pool.next_endpoint_from_eligible(&eligible, &[]).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(url, "https://private.com");
    }

    fn make_latency_config(endpoints: Vec<EndpointConfig>) -> ChainConfig {
        ChainConfig {
            name: "test".to_string(),
            route: "/test".to_string(),
            endpoints,
            health: HealthConfig {
                max_consecutive_failures: 3,
                cooldown_seconds: 30,
                health_method: None,
                health_check_interval_seconds: 30,
                max_block_lag: 10,
                max_retries: 1,
                retry_delay_ms: 0,
            },
            rotation: RotationStrategy::Latency,
            cache: None,
            chain_id: None,
            rate_limit: None,
            hedge: None,
        }
    }

    #[test]
    fn latency_cold_start_falls_back_to_weights() {
        let config =
            make_latency_config(vec![ep("https://a.com", None), ep("https://b.com", None)]);
        let pool = ChainPool::new(&config);
        let result = pool.next_endpoint();
        assert!(result.is_some());
    }

    #[test]
    fn latency_prefers_faster_endpoint() {
        let config = make_latency_config(vec![
            ep("https://fast.com", None),
            ep("https://slow.com", None),
        ]);
        let pool = ChainPool::new(&config);

        pool.health[0].update_latency(50);
        pool.health[1].update_latency(200);

        let mut fast_count = 0;
        for _ in 0..100 {
            let (idx, _) = pool.next_endpoint().unwrap();
            if idx == 0 {
                fast_count += 1;
            }
        }
        assert!(
            fast_count > 60,
            "fast={} should be > 60 out of 100",
            fast_count
        );
    }

    #[test]
    fn latency_respects_user_weights() {
        let mut ep_heavy = ep("https://heavy.com", None);
        ep_heavy.weight = 4;
        let config = make_latency_config(vec![ep("https://light.com", None), ep_heavy]);
        let pool = ChainPool::new(&config);

        pool.health[0].update_latency(100);
        pool.health[1].update_latency(100);

        let mut heavy_count = 0;
        for _ in 0..100 {
            let (idx, _) = pool.next_endpoint().unwrap();
            if idx == 1 {
                heavy_count += 1;
            }
        }
        assert!(
            heavy_count > 60,
            "heavy={} should be > 60 out of 100",
            heavy_count
        );
    }

    #[test]
    fn roster_initializes_all_active_when_5_or_fewer_endpoints() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let active = pool.active_indices.load();
        let house = pool.rpc_house.load();
        assert_eq!(active.len(), 3);
        assert!(house.is_empty());
    }

    #[test]
    fn roster_splits_when_more_than_5_endpoints() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
            ep("https://d.com", None),
            ep("https://e.com", None),
            ep("https://f.com", None),
            ep("https://g.com", None),
            ep("https://h.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let active = pool.active_indices.load();
        let house = pool.rpc_house.load();
        assert_eq!(active.len(), 5);
        assert_eq!(house.len(), 3);
        // All indices accounted for
        let mut all: Vec<usize> = active.iter().copied().collect();
        all.extend(house.iter());
        all.sort();
        assert_eq!(all, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn next_endpoint_only_returns_active_indices() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
            ep("https://d.com", None),
            ep("https://e.com", None),
            ep("https://f.com", None),
            ep("https://g.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let active = pool.active_indices.load().clone();

        for _ in 0..50 {
            let (idx, _) = pool.next_endpoint().unwrap();
            assert!(
                active.contains(&idx),
                "Selected index {} is not in active set {:?}",
                idx,
                active
            );
        }
    }

    #[test]
    fn eligible_indices_intersects_with_active_set() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
            ep("https://d.com", None),
            ep("https://e.com", None),
            ep("https://f.com", None),
            ep("https://g.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let active = pool.active_indices.load().clone();
        let eligible = pool.eligible_indices_for_method("eth_call");

        for idx in &eligible {
            assert!(
                active.contains(idx),
                "Eligible index {} is not in active set {:?}",
                idx,
                active
            );
        }
        assert_eq!(eligible.len(), active.len());
    }

    #[test]
    fn healthy_count_only_counts_active_endpoints() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
            ep("https://d.com", None),
            ep("https://e.com", None),
            ep("https://f.com", None),
            ep("https://g.com", None),
        ]);
        let pool = ChainPool::new(&config);
        assert_eq!(pool.healthy_count(), 5);
    }

    #[test]
    fn demote_and_replace_swaps_endpoint() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
            ep("https://d.com", None),
            ep("https://e.com", None),
            ep("https://f.com", None),
            ep("https://g.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let active_before = pool.active_indices.load().clone();
        let demoted_idx = active_before[0];

        pool.demote_and_replace(demoted_idx);

        let active_after = pool.active_indices.load().clone();
        assert_eq!(active_after.len(), 5);
        assert!(!active_after.contains(&demoted_idx));
        let house = pool.rpc_house.load();
        assert!(house.contains(&demoted_idx));
    }

    #[test]
    fn demote_recycles_when_rpc_house_exhausted() {
        // 6 endpoints: 5 active, 1 reserve
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
            ep("https://d.com", None),
            ep("https://e.com", None),
            ep("https://f.com", None),
        ]);
        let pool = ChainPool::new(&config);

        // Demote once — uses the single reserve endpoint
        let first_demoted = pool.active_indices.load()[0];
        pool.demote_and_replace(first_demoted);

        // Demote again — rpc_house only has first_demoted, triggers recycle
        let second_demoted = pool.active_indices.load()[0];
        pool.demote_and_replace(second_demoted);

        let active = pool.active_indices.load().clone();
        assert_eq!(active.len(), 5);
        assert!(!active.contains(&second_demoted));
    }

    #[test]
    fn demote_noop_when_no_reserve_and_only_5_endpoints() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
            ep("https://d.com", None),
            ep("https://e.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let active_before = pool.active_indices.load().clone();

        pool.demote_and_replace(active_before[0]);

        let active_after = pool.active_indices.load().clone();
        assert!(active_after.len() >= 4);
    }

    #[test]
    fn endpoint_statuses_tags_roster_status_correctly() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
            ep("https://c.com", None),
            ep("https://d.com", None),
            ep("https://e.com", None),
            ep("https://f.com", None),
            ep("https://g.com", None),
        ]);
        let pool = ChainPool::new(&config);
        let statuses = pool.endpoint_statuses();
        let active_count = statuses
            .iter()
            .filter(|s| matches!(s.roster_status, RosterStatus::Active))
            .count();
        let reserve_count = statuses
            .iter()
            .filter(|s| matches!(s.roster_status, RosterStatus::Reserve))
            .count();
        assert_eq!(active_count, 5);
        assert_eq!(reserve_count, 2);
    }
}
