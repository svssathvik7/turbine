# Active Roster + RPC House Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split each chain's endpoints into an active set (5 max) that receives traffic/health checks and a reserve pool (rpc_house) with zero connections, reducing TCP footprint from ~416 to ~200.

**Architecture:** Add `active_indices` (RwLock<Vec<usize>>) and `rpc_house` (Mutex<VecDeque<usize>>) to ChainPool. All selection methods filter by active set. Health checker becomes sequential and only probes active endpoints. Demoted endpoints go to rpc_house; replacements are popped FIFO.

**Tech Stack:** Rust, std::sync::{RwLock, Mutex}, std::collections::VecDeque, rand crate for shuffling

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/health/pool.rs` | Modify | Add roster fields, initialization, `demote_and_replace()`, filter selection by active set, update `endpoint_statuses()` |
| `src/health/state.rs` | Modify | Add `RosterStatus` enum, add `roster_status` to `EndpointStatus` |
| `src/health/checker.rs` | Modify | Sequential probing, only probe active endpoints, demotion trigger logic, `pool_max_idle_per_host(0)` |
| `src/proxy/server.rs` | Modify | Add `roster_status` to `ChainStatusSnapshot`, pass through in `status_handler` |
| `src/dashboard.rs` | Modify | Display roster status (active vs reserve) in dashboard UI |
| `Cargo.toml` | Modify | Add `rand` dependency |

---

### Task 1: Add `rand` dependency

**Files:**
- Modify: `Cargo.toml:22-30`

- [ ] **Step 1: Add rand to Cargo.toml**

Add to the `[dependencies]` section:

```toml
rand = "0.8"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add rand dependency for roster shuffling"
```

---

### Task 2: Add `RosterStatus` enum and update `EndpointStatus`

**Files:**
- Modify: `src/health/state.rs:1-2` (add import), `src/health/state.rs:50-64` (EndpointStatus)
- Modify: `src/health/mod.rs` (re-export)

- [ ] **Step 1: Write the test**

Add to the bottom of `src/health/state.rs`, inside a new `#[cfg(test)] mod tests` block (state.rs has no tests yet):

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test roster_status_serializes_to_lowercase -- --nocapture`
Expected: FAIL — `RosterStatus` not found

- [ ] **Step 3: Add `RosterStatus` enum**

Add after the existing `use` statements at the top of `src/health/state.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RosterStatus {
    Active,
    Reserve,
}
```

- [ ] **Step 4: Add `roster_status` field to `EndpointStatus`**

In `src/health/state.rs`, add to the `EndpointStatus` struct after `throttle_count`:

```rust
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
```

- [ ] **Step 5: Re-export `RosterStatus` from `src/health/mod.rs`**

Add to `src/health/mod.rs`:

```rust
pub use state::RosterStatus;
```

- [ ] **Step 6: Fix the `endpoint_statuses()` call in `pool.rs`**

In `src/health/pool.rs:476-489`, the `EndpointStatus` construction is now missing the `roster_status` field. For now, default all to `RosterStatus::Active` (we'll make it roster-aware in Task 4):

```rust
use super::state::RosterStatus;

// In endpoint_statuses():
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
    roster_status: RosterStatus::Active,
}
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test roster_status_serializes_to_lowercase -- --nocapture`
Expected: PASS

- [ ] **Step 8: Run full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 9: Commit**

```bash
git add src/health/state.rs src/health/pool.rs src/health/mod.rs
git commit -m "feat: add RosterStatus enum and roster_status field to EndpointStatus"
```

---

### Task 3: Add roster fields to ChainPool and initialization

**Files:**
- Modify: `src/health/pool.rs:1-5` (imports), `src/health/pool.rs:7-18` (struct), `src/health/pool.rs:20-41` (new fn)
- Test: `src/health/pool.rs` (existing test module)

- [ ] **Step 1: Write the tests**

Add to the existing `mod tests` in `src/health/pool.rs`:

```rust
#[test]
fn roster_initializes_all_active_when_5_or_fewer_endpoints() {
    let config = make_config(vec![
        ep("https://a.com", None),
        ep("https://b.com", None),
        ep("https://c.com", None),
    ]);
    let pool = ChainPool::new(&config);
    let active = pool.active_indices.read().unwrap();
    let house = pool.rpc_house.lock().unwrap();
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
    let active = pool.active_indices.read().unwrap();
    let house = pool.rpc_house.lock().unwrap();
    assert_eq!(active.len(), 5);
    assert_eq!(house.len(), 3);
    // All indices accounted for
    let mut all: Vec<usize> = active.iter().copied().collect();
    all.extend(house.iter());
    all.sort();
    assert_eq!(all, vec![0, 1, 2, 3, 4, 5, 6, 7]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test roster_initializes -- --nocapture`
Expected: FAIL — `active_indices` field not found

- [ ] **Step 3: Add imports and roster fields to ChainPool**

At the top of `src/health/pool.rs`, add to imports:

```rust
use std::collections::VecDeque;
use std::sync::{Mutex, RwLock};
use rand::seq::SliceRandom;
use rand::thread_rng;
```

Add fields to `ChainPool` struct:

```rust
pub struct ChainPool {
    pub name: String,
    pub endpoints: Vec<EndpointConfig>,
    pub counter: AtomicUsize,
    pub health: Vec<EndpointHealth>,
    pub health_config: HealthConfig,
    pub rotation: RotationStrategy,
    pub chain_id: Option<u64>,
    pub hedge_config: Option<HedgeConfig>,
    total_weight: u32,
    /// Indices of endpoints currently receiving traffic and health checks.
    pub active_indices: RwLock<Vec<usize>>,
    /// Reserve queue — FIFO. Front = next promotion candidate.
    pub rpc_house: Mutex<VecDeque<usize>>,
}
```

- [ ] **Step 4: Update `ChainPool::new()` with roster initialization**

Replace the `new` method:

```rust
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
            active_indices: RwLock::new(active_indices),
            rpc_house: Mutex::new(rpc_house),
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test roster_initializes -- --nocapture`
Expected: PASS (both tests)

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: all tests pass (existing tests still work because selection methods haven't changed yet — they still iterate all endpoints)

- [ ] **Step 7: Commit**

```bash
git add src/health/pool.rs Cargo.toml Cargo.lock
git commit -m "feat: add active_indices and rpc_house to ChainPool with random initialization"
```

---

### Task 4: Filter endpoint selection by active set

**Files:**
- Modify: `src/health/pool.rs:44-53` (next_endpoint), `src/health/pool.rs:56-65` (next_endpoint_excluding), `src/health/pool.rs:68-143` (next_endpoint_excluding_many), `src/health/pool.rs:310-335` (eligible_indices_for_method), `src/health/pool.rs:340-419` (next_endpoint_from_eligible), `src/health/pool.rs:441-443` (healthy_count), `src/health/pool.rs:470-492` (endpoint_statuses)

- [ ] **Step 1: Write the tests**

Add to `mod tests` in `src/health/pool.rs`:

```rust
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
    let active = pool.active_indices.read().unwrap().clone();

    // Make 50 selections — all should be from active set
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
    let active = pool.active_indices.read().unwrap().clone();
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
    // All active endpoints are healthy at init
    assert_eq!(pool.healthy_count(), 5);
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test next_endpoint_only_returns_active -- --nocapture`
Expected: FAIL — returns indices outside active set

- [ ] **Step 3: Update `next_endpoint()` to filter by active set**

```rust
pub fn next_endpoint(&self) -> Option<(usize, &str)> {
    let active = self.active_indices.read().unwrap();
    match self.rotation {
        RotationStrategy::RoundRobin | RotationStrategy::Weighted => {
            self.next_endpoint_from_eligible(&active, &[])
        }
        RotationStrategy::Latency => self.next_latency_based(&active, &[]),
    }
}
```

- [ ] **Step 4: Update `next_endpoint_excluding()` to filter by active set**

```rust
pub fn next_endpoint_excluding(&self, exclude: usize) -> Option<(usize, &str)> {
    let active = self.active_indices.read().unwrap();
    match self.rotation {
        RotationStrategy::RoundRobin | RotationStrategy::Weighted => {
            self.next_endpoint_from_eligible(&active, &[exclude])
        }
        RotationStrategy::Latency => self.next_latency_based(&active, &[exclude]),
    }
}
```

- [ ] **Step 5: Update `next_endpoint_excluding_many()` to filter by active set**

```rust
pub fn next_endpoint_excluding_many(&self, exclude: &[usize]) -> Option<(usize, &str)> {
    let active = self.active_indices.read().unwrap();
    match self.rotation {
        RotationStrategy::RoundRobin | RotationStrategy::Weighted => {
            self.next_endpoint_from_eligible(&active, exclude)
        }
        RotationStrategy::Latency => self.next_latency_based(&active, exclude),
    }
}
```

- [ ] **Step 6: Update `eligible_indices_for_method()` to intersect with active set**

```rust
pub fn eligible_indices_for_method(&self, method: &str) -> Vec<usize> {
    let active = self.active_indices.read().unwrap();

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

    self.endpoints
        .iter()
        .enumerate()
        .filter(|(i, ep)| active.contains(i) && ep.methods.is_none())
        .map(|(i, _)| i)
        .collect()
}
```

- [ ] **Step 7: Update `healthy_count()` to only count active endpoints**

```rust
pub fn healthy_count(&self) -> usize {
    let active = self.active_indices.read().unwrap();
    active.iter().filter(|&&i| self.health[i].is_available()).count()
}
```

- [ ] **Step 8: Update `endpoint_statuses()` to tag roster status**

```rust
pub fn endpoint_statuses(&self) -> Vec<EndpointStatus> {
    let active = self.active_indices.read().unwrap();
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
```

- [ ] **Step 9: Remove `next_round_robin`, `next_round_robin_excluding`, `next_weighted`, `next_weighted_excluding` private methods**

These are now dead code — all selection goes through `next_endpoint_from_eligible` and `next_latency_based`. Remove the following methods:
- `next_round_robin()` (lines 145-157)
- `next_round_robin_excluding()` (lines 159-174)
- `next_weighted()` (lines 176-205)
- `next_weighted_excluding()` (lines 207-235)

- [ ] **Step 10: Run all tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 11: Commit**

```bash
git add src/health/pool.rs
git commit -m "feat: filter all endpoint selection by active roster set"
```

---

### Task 5: Implement `demote_and_replace()`

**Files:**
- Modify: `src/health/pool.rs` (add method)

- [ ] **Step 1: Write the tests**

Add to `mod tests` in `src/health/pool.rs`:

```rust
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
    let active_before = pool.active_indices.read().unwrap().clone();
    let demoted_idx = active_before[0];

    pool.demote_and_replace(demoted_idx);

    let active_after = pool.active_indices.read().unwrap().clone();
    assert_eq!(active_after.len(), 5);
    assert!(!active_after.contains(&demoted_idx));
    // Demoted index should be in rpc_house now
    let house = pool.rpc_house.lock().unwrap();
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
    let first_demoted = pool.active_indices.read().unwrap()[0];
    pool.demote_and_replace(first_demoted);

    // Demote again — rpc_house only has first_demoted, triggers recycle
    let second_demoted = pool.active_indices.read().unwrap()[0];
    pool.demote_and_replace(second_demoted);

    let active = pool.active_indices.read().unwrap().clone();
    assert_eq!(active.len(), 5);
    assert!(!active.contains(&second_demoted));
    // first_demoted should have been recycled and could be back in active
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
    let active_before = pool.active_indices.read().unwrap().clone();

    // Demoting when rpc_house is empty and no other indices exist — should not crash
    pool.demote_and_replace(active_before[0]);

    // Active set might shrink to 4 since there's no replacement, or stay at 5
    // The key is it doesn't panic
    let active_after = pool.active_indices.read().unwrap().clone();
    assert!(active_after.len() >= 4);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test demote_and_replace -- --nocapture`
Expected: FAIL — method not found

- [ ] **Step 3: Implement `demote_and_replace()`**

Add to `impl ChainPool` in `src/health/pool.rs`:

```rust
/// Demote an active endpoint to the rpc_house and promote a replacement.
/// If rpc_house is empty (after pushing the demoted endpoint), recycles
/// all non-active indices back into the queue.
pub fn demote_and_replace(&self, idx: usize) {
    let mut active = self.active_indices.write().unwrap();
    let mut house = self.rpc_house.lock().unwrap();

    // Remove from active set
    if let Some(pos) = active.iter().position(|&i| i == idx) {
        active.remove(pos);
    } else {
        return; // Not in active set, nothing to do
    }

    // Push demoted to back of rpc_house
    house.push_back(idx);

    // If rpc_house only contains the just-demoted endpoint, recycle
    if house.len() == 1 {
        // Gather all indices not currently active (excluding the just-demoted one)
        let mut recyclable: Vec<usize> = (0..self.endpoints.len())
            .filter(|i| !active.contains(i) && *i != idx)
            .collect();
        recyclable.shuffle(&mut thread_rng());
        for r in recyclable {
            house.push_back(r);
        }
    }

    // Pop front as replacement (guaranteed != idx since idx is at back)
    if let Some(replacement) = house.pop_front() {
        active.push(replacement);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test demote_and_replace -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/health/pool.rs
git commit -m "feat: implement demote_and_replace for active roster management"
```

---

### Task 6: Make health checker sequential and roster-aware

**Files:**
- Modify: `src/health/checker.rs`

- [ ] **Step 1: Update the health check client config**

In `src/health/checker.rs`, change the client builder (lines 31-36):

```rust
let client = Client::builder()
    .timeout(Duration::from_secs(10))
    .pool_max_idle_per_host(0)
    .pool_idle_timeout(Duration::from_secs(1))
    .build()
    .expect("Failed to build health check client");
```

- [ ] **Step 2: Make `fetch_block_heights()` sequential and roster-aware**

Replace the `fetch_block_heights` function (lines 115-149):

```rust
async fn fetch_block_heights(
    client: &Client,
    pool: &ChainPool,
    method: &str,
) -> Vec<(usize, HealthCheckResult)> {
    let active = pool.active_indices.read().unwrap().clone();
    let mut results = Vec::with_capacity(active.len());

    for &idx in &active {
        let endpoint = &pool.endpoints[idx];
        let start = std::time::Instant::now();
        let result = fetch_block_height(client, &endpoint.url, method, endpoint.auth.as_ref()).await;
        let latency_ms = start.elapsed().as_millis() as u64;

        if let HealthCheckResult::Ok(_) = &result {
            pool.update_latency(idx, latency_ms);
        }
        results.push((idx, result));
    }

    results
}
```

- [ ] **Step 3: Remove `join_all` import**

Remove from the top of `src/health/checker.rs`:

```rust
// Remove this line:
use futures_util::future::join_all;
```

- [ ] **Step 4: Add demotion logic to the health check loop**

In `src/health/checker.rs`, after the existing `for (idx, result) in &results` loop (after line 110), add demotion evaluation:

```rust
// After the existing result processing loop, add:

// --- Demotion evaluation ---
const THROTTLE_DEMOTION_THRESHOLD: u64 = 3;
let mut to_demote: Vec<usize> = Vec::new();

for (idx, result) in &results {
    let should_demote = match result {
        HealthCheckResult::Ok(h) if max_height > 0 && max_height - h > max_block_lag => {
            true // Already marked stale above, also demote
        }
        HealthCheckResult::Failed => {
            // Check if consecutive failures hit threshold
            pool.health[*idx].consecutive_failures.load(std::sync::atomic::Ordering::Relaxed)
                >= pool.health_config.max_consecutive_failures
        }
        HealthCheckResult::Throttled => {
            // Check chronic throttling
            pool.health[*idx].throttle_count.load(std::sync::atomic::Ordering::Relaxed)
                >= THROTTLE_DEMOTION_THRESHOLD
        }
        _ => false,
    };
    if should_demote {
        to_demote.push(*idx);
    }
}

for idx in to_demote {
    warn!(
        chain = %chain_name,
        endpoint = %pool.endpoints[idx].url,
        "Demoting endpoint to rpc_house"
    );
    pool.demote_and_replace(idx);
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add src/health/checker.rs
git commit -m "feat: sequential roster-aware health checker with demotion logic"
```

---

### Task 7: Update dashboard status endpoint

**Files:**
- Modify: `src/proxy/server.rs:151-172` (ChainStatusSnapshot), `src/proxy/server.rs:187-207` (status_handler mapping)

- [ ] **Step 1: Add `roster_status` to `ChainStatusSnapshot`**

In `src/proxy/server.rs`, add the import:

```rust
use crate::health::{spawn_health_checker, ChainPool, EndpointStatus, RosterStatus};
```

Add field to `ChainStatusSnapshot`:

```rust
#[derive(Serialize)]
struct ChainStatusSnapshot {
    name: String,
    route: String,
    rotation: String,
    chain_id: Option<u64>,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    rate_limited_requests: u64,
    upstream_throttled: u64,
    hedged_requests: u64,
    ws_connections_total: u64,
    ws_active_connections: u64,
    ws_messages_relayed: u64,
    ws_reconnections: u64,
    active_endpoints: usize,
    total_endpoints: usize,
    endpoints: Vec<EndpointStatus>,
}
```

No new field needed on `ChainStatusSnapshot` itself — the `roster_status` lives on each `EndpointStatus` inside `endpoints`, which is already populated by `endpoint_statuses()` (updated in Task 4). The JSON serialization will include it automatically.

- [ ] **Step 2: Verify it compiles and serves correctly**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src/proxy/server.rs
git commit -m "feat: expose roster_status in status API endpoint responses"
```

---

### Task 8: Update dashboard UI to show roster status

**Files:**
- Modify: `src/dashboard.rs`

- [ ] **Step 1: Find the endpoint table rendering in the dashboard HTML**

The dashboard is embedded HTML/JS in `src/dashboard.rs`. Find the endpoint row rendering code that builds the table rows for each endpoint.

- [ ] **Step 2: Add visual indicator for reserve endpoints**

In the endpoint table row rendering, add a roster status indicator. Reserve endpoints should appear dimmed with a "RESERVE" badge:

Find the endpoint row rendering in the JavaScript template (look for where `ep.url` is rendered in a table row). Add conditional styling:

```javascript
const isReserve = ep.roster_status === 'reserve';
const rowClass = isReserve ? 'style="opacity:0.4"' : '';
const badge = isReserve ? '<span style="color:var(--text-dim);font-size:0.75em;margin-left:4px">[RESERVE]</span>' : '';
```

Apply `rowClass` to the `<tr>` element and append `badge` after the endpoint URL.

- [ ] **Step 3: Build and verify visually**

Run: `cargo build --release`
Start turbine and open the dashboard in a browser to verify reserve endpoints show dimmed.

- [ ] **Step 4: Commit**

```bash
git add src/dashboard.rs
git commit -m "feat: show roster status (active/reserve) in dashboard UI"
```

---

### Task 9: Change default health check interval

**Files:**
- Modify: `src/config.rs` (default value)

- [ ] **Step 1: Find and update the default**

In `src/config.rs`, find the `HealthConfig` default for `health_check_interval_seconds`. Change from `30` to `300`:

```rust
// In the Default impl or deserialization default function for HealthConfig:
health_check_interval_seconds: 300,
```

- [ ] **Step 2: Update the test-deploy.toml**

In `test-deploy.toml`, update all `health_check_interval_seconds` values from `10` to `300`:

```toml
health_check_interval_seconds = 300
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add src/config.rs test-deploy.toml
git commit -m "chore: change default health check interval to 5 minutes (300s)"
```

---

### Task 10: Integration smoke test

**Files:** None (manual verification)

- [ ] **Step 1: Build release binary**

Run: `cargo build --release`
Expected: compiles successfully

- [ ] **Step 2: Start turbine with test config**

Run: `./target/release/turbine --config test-deploy.toml --port 8081`
Expected: starts up, logs show 5 active endpoints per chain (for chains with >5 endpoints)

- [ ] **Step 3: Verify status API shows roster**

Run: `curl -s http://localhost:8081/api/status | python3 -c "import json,sys; data=json.load(sys.stdin); [print(f\"{e['url']}: {e['roster_status']}\") for c in data['chains'] for e in c['endpoints'] if c['name']=='ethereum_sepolia']" | head -10`
Expected: 5 endpoints show "active", remaining show "reserve"

- [ ] **Step 4: Send test requests and verify routing**

Run: `for i in $(seq 1 10); do curl -s -X POST http://localhost:8081/ethereum_sepolia -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | python3 -c "import json,sys; print(json.load(sys.stdin).get('result','ERROR'))"; done`
Expected: 10 successful block number responses

- [ ] **Step 5: Verify connection count**

Run: `lsof -p $(pgrep turbine) 2>/dev/null | grep TCP | wc -l`
Expected: significantly fewer than 92 TCP connections

- [ ] **Step 6: Check dashboard**

Open `http://localhost:8081/<dashboard_secret>` in browser.
Expected: reserve endpoints show dimmed with [RESERVE] badge

- [ ] **Step 7: Commit any fixes**

If any issues found during smoke test, fix and commit.
