# Latency-Based Rotation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `rotation = "latency"` strategy that routes more traffic to faster endpoints using inverse-latency weighting combined with user-defined weights.

**Architecture:** Add `Latency` variant to `RotationStrategy`, implement `next_latency_based()` in `ChainPool` that computes `effective_weight = user_weight × (1.0 / rolling_latency_ms)` for each healthy endpoint and selects using tick-based weighted random. Cold start (no latency data) falls back to user weights only.

**Tech Stack:** Rust, existing `EndpointHealth.rolling_latency_ms: Option<f64>`

---

## Task 1: Add `Latency` variant to `RotationStrategy`

**Files:**
- Modify: `src/config.rs`

### Step 1: Add the variant

In the `RotationStrategy` enum (around line 119), add `Latency`:

```rust
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    #[default]
    RoundRobin,
    Weighted,
    Latency,
}
```

### Step 2: Add test

Add to the existing `#[cfg(test)] mod tests` in `src/config.rs`:

```rust
#[test]
fn latency_rotation_strategy_parses() {
    let toml = r#"
        name = "ethereum"
        route = "/ethereum"
        rotation = "latency"
        endpoints = ["https://rpc.example.com"]

        [health]
        max_consecutive_failures = 3
        cooldown_seconds = 30
    "#;
    let chain: ChainConfig = toml::from_str(toml).unwrap();
    assert_eq!(chain.rotation, RotationStrategy::Latency);
}
```

### Step 3: Run tests and compile

```bash
cd ~/Desktop/sathvik/my-projects/rpc-proxy
cargo test config::tests
cargo build
```

The build will produce non-exhaustive match warnings in `pool.rs` — that's expected, fixed in Task 2.

### Step 4: Commit

```bash
git add src/config.rs
git commit -m "feat: add Latency variant to RotationStrategy"
```

---

## Task 2: Implement `next_latency_based` in `ChainPool`

**Files:**
- Modify: `src/health/pool.rs`

### Step 1: Write failing tests

Add to the existing `#[cfg(test)] mod tests` in `src/health/pool.rs`:

```rust
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
    // No latency data yet — should still return endpoints (using weight only)
    let config = make_latency_config(vec![
        ep("https://a.com", None),
        ep("https://b.com", None),
    ]);
    let pool = ChainPool::new(&config);
    // Should not return None — cold start must work
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

    // Simulate latency data: fast=50ms, slow=200ms
    {
        let mut health = pool.health.write().unwrap();
        health[0].update_latency(50);
        health[1].update_latency(200);
    }

    // Over 100 picks, fast endpoint should be selected ~4x more than slow
    let mut fast_count = 0;
    let mut slow_count = 0;
    for _ in 0..100 {
        let (idx, _) = pool.next_endpoint().unwrap();
        if idx == 0 {
            fast_count += 1;
        } else {
            slow_count += 1;
        }
    }
    // fast has 4x effective weight (1/50 vs 1/200), so expect ~80/20 split
    assert!(fast_count > slow_count, "fast={} should be > slow={}", fast_count, slow_count);
    assert!(fast_count > 60, "fast={} should be > 60 out of 100", fast_count);
}

#[test]
fn latency_respects_user_weights() {
    let mut ep_heavy = ep("https://heavy.com", None);
    ep_heavy.weight = 4;
    let config = make_latency_config(vec![
        ep("https://light.com", None), // weight=1
        ep_heavy,                       // weight=4
    ]);
    let pool = ChainPool::new(&config);

    // Both same latency — user weights should dominate
    {
        let mut health = pool.health.write().unwrap();
        health[0].update_latency(100);
        health[1].update_latency(100);
    }

    let mut light_count = 0;
    let mut heavy_count = 0;
    for _ in 0..100 {
        let (idx, _) = pool.next_endpoint().unwrap();
        if idx == 0 {
            light_count += 1;
        } else {
            heavy_count += 1;
        }
    }
    // Same latency, weight 1 vs 4 → expect ~20/80 split
    assert!(heavy_count > light_count, "heavy={} should be > light={}", heavy_count, light_count);
    assert!(heavy_count > 60, "heavy={} should be > 60 out of 100", heavy_count);
}
```

### Step 2: Run tests to verify they fail

```bash
cargo test health::pool::tests
```

Expected: non-exhaustive match errors in `next_endpoint()` and related methods.

### Step 3: Implement `next_latency_based`

Add this private method to `impl ChainPool` (after `next_weighted_excluding`):

```rust
/// Latency-based selection: effective_weight = user_weight × (1.0 / rolling_latency_ms).
/// Falls back to user_weight alone when no latency data exists.
fn next_latency_based(&self, candidates: &[usize], exclude: &[usize]) -> Option<(usize, &str)> {
    let health = self.health.read().unwrap();

    // Compute effective weights for healthy, non-excluded candidates
    let mut effective: Vec<(usize, f64)> = Vec::new();
    for &idx in candidates {
        if exclude.contains(&idx) || !health[idx].is_healthy {
            continue;
        }
        let user_w = self.endpoints[idx].weight as f64;
        let eff = match health[idx].rolling_latency_ms {
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
                    if health[idx].failed_earlier_than(&health[prev]) {
                        best = Some(idx);
                    }
                }
            }
        }
        return best.map(|idx| (idx, self.endpoints[idx].url.as_str()));
    }

    let total: f64 = effective.iter().map(|(_, w)| w).sum();
    let tick = self.counter.fetch_add(1, Ordering::Relaxed) as f64;
    // Map tick to a position in [0, total) using modulo
    let target = tick % (total * 1000.0) / 1000.0;
    let mut cumulative = 0.0;
    for &(idx, weight) in &effective {
        cumulative += weight;
        if target < cumulative {
            return Some((idx, &self.endpoints[idx].url));
        }
    }

    // Rounding edge case — return last
    effective.last().map(|&(idx, _)| (idx, self.endpoints[idx].url.as_str()))
}
```

### Step 4: Wire into all match statements

Update every `match self.rotation` block in `ChainPool` to handle `Latency`. There are 5 match sites:

**`next_endpoint()`:**
```rust
RotationStrategy::Latency => {
    let all: Vec<usize> = (0..self.endpoints.len()).collect();
    self.next_latency_based(&all, &[])
}
```

**`next_endpoint_excluding()`:**
```rust
RotationStrategy::Latency => {
    let all: Vec<usize> = (0..self.endpoints.len()).collect();
    self.next_latency_based(&all, &[exclude])
}
```

**`next_endpoint_excluding_many()`:**
```rust
RotationStrategy::Latency => {
    let all: Vec<usize> = (0..self.endpoints.len()).collect();
    self.next_latency_based(&all, exclude)
}
```

**`next_endpoint_from_eligible()`:**
```rust
RotationStrategy::Latency => self.next_latency_based(eligible, exclude),
```

**`rotation_name()`:**
```rust
RotationStrategy::Latency => "latency",
```

### Step 5: Run tests

```bash
cargo test health::pool::tests -- --nocapture
```

All tests (existing 6 + new 3) must pass.

### Step 6: Clippy + fmt

```bash
cargo fmt
cargo clippy -- -D warnings
```

### Step 7: Commit

```bash
git add src/health/pool.rs
git commit -m "feat: implement latency-based endpoint rotation"
```

---

## Task 3: Builder API + config example

**Files:**
- Modify: `src/lib.rs`
- Modify: `config.toml`

### Step 1: Add `latency_based()` to `ChainBuilder`

In `src/lib.rs`, in the `impl ChainBuilder` block, after `weighted()`:

```rust
/// Set rotation strategy to latency-based.
/// Routes more traffic to faster endpoints using inverse-latency weighting.
pub fn latency_based(mut self) -> Self {
    self.rotation = RotationStrategy::Latency;
    self
}
```

### Step 2: Add example to `config.toml`

Add a commented example after the existing solana chain block:

```toml
# Latency-based rotation: routes more traffic to faster endpoints
# [[chains]]
# name = "polygon"
# route = "/polygon"
# rotation = "latency"
# endpoints = [
#     { url = "https://polygon-rpc.com", weight = 1 },
#     { url = "https://rpc.ankr.com/polygon", weight = 1 },
# ]
#
# [chains.health]
# max_consecutive_failures = 3
# cooldown_seconds = 30
```

### Step 3: Compile + test

```bash
cargo build
cargo test
```

### Step 4: Commit

```bash
git add src/lib.rs config.toml
git commit -m "feat: add latency_based() to ChainBuilder + config example"
```

---

## Task 4: CHANGELOG + version bump

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `Cargo.toml`

### Step 1: Add CHANGELOG entry

At the top, after the header:

```markdown
## [0.8.0] - 2026-03-13

### Added

- **Latency-based rotation strategy**: `rotation = "latency"` routes more traffic to faster endpoints
- Uses inverse-latency weighting combined with user-defined endpoint weights
- Cold start: falls back to weight-only rotation until latency data exists
- Builder API: `ChainBuilder::latency_based()`
```

### Step 2: Bump version

In `Cargo.toml`, change `version = "0.7.0"` to `version = "0.8.0"`.

### Step 3: Compile

```bash
cargo build
```

### Step 4: Commit

```bash
git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.8.0, update CHANGELOG for latency rotation"
```

---

## Task 5: Push + PR

### Step 1: Push

```bash
git push -u origin feat/latency-rotation
```

### Step 2: Create PR

```bash
gh pr create --title "feat: latency-based endpoint rotation (v0.8.0)" --body "$(cat <<'EOF'
## Summary

- Adds `rotation = "latency"` strategy that routes more traffic to faster endpoints
- Effective weight = `user_weight × (1 / rolling_latency_ms)` — faster endpoints get proportionally more traffic
- Cold start: uses user weights alone until latency data arrives
- Builder API: `.latency_based()`

## Example

```toml
[[chains]]
name = "ethereum"
rotation = "latency"
endpoints = [
    { url = "https://fast-rpc.com", weight = 1 },
    { url = "https://slow-rpc.com", weight = 1 },
]
```

## Test plan

- [x] `cargo test config::tests` — TOML parsing of `rotation = "latency"`
- [x] `cargo test health::pool::tests` — cold start fallback, faster endpoint preferred, user weights respected
- [x] `cargo clippy -- -D warnings` — clean
- [x] `cargo fmt --check` — clean
EOF
)"
```
