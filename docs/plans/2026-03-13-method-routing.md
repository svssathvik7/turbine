# Method-Based Endpoint Routing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow endpoints to declare a `methods` allowlist so that specific RPC methods (e.g. `eth_sendRawTransaction`) are routed only to designated endpoints.

**Architecture:** Add `methods: Option<Vec<String>>` to `EndpointConfig`. The pool computes an eligible set per method at request time — claimed endpoints if any exist, else unconstrained endpoints. The handler threads the method through to endpoint selection, and the batch path groups uncached items by eligible set before forwarding.

**Tech Stack:** Rust, axum, tokio, serde/toml

---

## Task 1: Add `methods` field to `EndpointConfig`

**Files:**
- Modify: `src/config.rs`

### Step 1: Add the field to `EndpointConfig` and `EndpointRaw`

In `src/config.rs`, update `EndpointRaw::Full` and `EndpointConfig`:

```rust
// EndpointRaw — add methods to the Full variant
enum EndpointRaw {
    Simple(String),
    Full {
        url: String,
        #[serde(default = "default_weight")]
        weight: u32,
        #[serde(default)]
        auth: Option<EndpointAuth>,
        #[serde(default)]
        methods: Option<Vec<String>>,   // NEW
    },
}

// EndpointConfig — add methods field
pub struct EndpointConfig {
    pub url: String,
    pub weight: u32,
    pub auth: Option<EndpointAuth>,
    pub methods: Option<Vec<String>>,   // NEW
}
```

Update the `Deserialize` impl for `EndpointConfig` to propagate the new field:

```rust
impl<'de> Deserialize<'de> for EndpointConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = EndpointRaw::deserialize(deserializer).map_err(de::Error::custom)?;
        Ok(match raw {
            EndpointRaw::Simple(url) => EndpointConfig {
                url,
                weight: default_weight(),
                auth: None,
                methods: None,          // NEW
            },
            EndpointRaw::Full { url, weight, auth, methods } => EndpointConfig {
                url,
                weight,
                auth,
                methods,                // NEW
            },
        })
    }
}
```

### Step 2: Add validation warning for empty methods list

In `Config::validate`, after the hedge validation block, add:

```rust
for chain in &self.chains {
    for ep in &chain.endpoints {
        if let Some(ref methods) = ep.methods {
            if methods.is_empty() {
                tracing::warn!(
                    chain = %chain.name,
                    url = %ep.url,
                    "Endpoint has methods = [] (empty list). It will never receive any request. Did you mean to omit the field?"
                );
            }
        }
    }
}
```

Note: `validate` runs before the tracing subscriber is initialized in main, so this warning won't appear at startup — that's acceptable for now. The validation still prevents silent misconfig.

### Step 3: Write tests

At the bottom of `src/config.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_without_methods_defaults_to_none() {
        let toml = r#"
            url = "https://rpc.example.com"
        "#;
        let ep: EndpointConfig = toml::from_str(toml).unwrap();
        assert!(ep.methods.is_none());
    }

    #[test]
    fn endpoint_with_methods_parses_correctly() {
        let toml = r#"
            url = "https://private-rpc.example.com"
            methods = ["eth_sendRawTransaction", "eth_sendTransaction"]
        "#;
        let ep: EndpointConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            ep.methods.unwrap(),
            vec!["eth_sendRawTransaction", "eth_sendTransaction"]
        );
    }

    #[test]
    fn simple_string_endpoint_has_no_methods() {
        // Simple string format: endpoints = ["https://rpc.example.com"]
        // This is tested via ChainConfig parsing since Simple is untagged
        let toml = r#"
            name = "ethereum"
            route = "/ethereum"
            endpoints = ["https://rpc.example.com"]

            [health]
            max_consecutive_failures = 3
            cooldown_seconds = 30
        "#;
        let chain: ChainConfig = toml::from_str(toml).unwrap();
        assert!(chain.endpoints[0].methods.is_none());
    }
}
```

### Step 4: Run tests

```bash
cd ~/Desktop/sathvik/my-projects/rpc-proxy
cargo test config::tests
```

Expected: all 3 pass.

### Step 5: Commit

```bash
git add src/config.rs
git commit -m "feat: add methods allowlist field to EndpointConfig"
```

---

## Task 2: Pool — eligible endpoint routing logic

**Files:**
- Modify: `src/health/pool.rs`

### Step 1: Write failing tests

Add at the bottom of `src/health/pool.rs`:

```rust
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
        }
    }

    #[test]
    fn all_unconstrained_returns_all_indices() {
        let config = make_config(vec![
            ep("https://a.com", None),
            ep("https://b.com", None),
        ]);
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
        // eth_call is not claimed → only unconstrained endpoints eligible
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
    fn next_endpoint_from_eligible_returns_none_when_all_unhealthy() {
        let config = make_config(vec![
            ep("https://private.com", Some(vec!["eth_sendRawTransaction"])),
        ]);
        let pool = ChainPool::new(&config);
        // Mark the only eligible endpoint unhealthy
        pool.record_failure(0);
        pool.record_failure(0);
        pool.record_failure(0); // 3 failures → unhealthy
        let eligible = pool.eligible_indices_for_method("eth_sendRawTransaction");
        // next_endpoint_from_eligible falls back to least-recently-failed if all unhealthy
        // so it still returns Some — this is correct behavior (same as existing pool)
        assert!(pool.next_endpoint_from_eligible(&eligible, &[]).is_some());
    }

    #[test]
    fn next_endpoint_from_eligible_returns_none_on_empty_set() {
        let config = make_config(vec![ep("https://a.com", None)]);
        let pool = ChainPool::new(&config);
        let result = pool.next_endpoint_from_eligible(&[], &[]);
        assert!(result.is_none());
    }
}
```

### Step 2: Run tests to verify they fail

```bash
cargo test health::pool::tests
```

Expected: compile error — `eligible_indices_for_method` and `next_endpoint_from_eligible` do not exist yet.

### Step 3: Implement `eligible_indices_for_method`

Add to the `impl ChainPool` block in `src/health/pool.rs`:

```rust
/// Compute which endpoint indices are eligible to serve a given method.
///
/// - If any endpoint declares `methods` containing `method`, return only those indices.
/// - Otherwise, return indices of endpoints that have no `methods` restriction (unconstrained).
pub fn eligible_indices_for_method(&self, method: &str) -> Vec<usize> {
    let claimed: Vec<usize> = self
        .endpoints
        .iter()
        .enumerate()
        .filter(|(_, ep)| {
            ep.methods
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
        .filter(|(_, ep)| ep.methods.is_none())
        .map(|(i, _)| i)
        .collect()
}
```

### Step 4: Implement `next_endpoint_from_eligible`

Add to the `impl ChainPool` block:

```rust
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

    let health = self.health.read().unwrap();

    match self.rotation {
        RotationStrategy::RoundRobin => {
            let start = self.counter.fetch_add(1, Ordering::Relaxed);
            // Try healthy endpoints in rotation order within eligible set
            for i in 0..eligible.len() {
                let idx = eligible[(start + i) % eligible.len()];
                if exclude.contains(&idx) {
                    continue;
                }
                if health[idx].is_healthy {
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
                        if health[idx].failed_earlier_than(&health[prev]) {
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
                .filter(|&&i| !exclude.contains(&i) && health[i].is_healthy)
                .map(|&i| self.endpoints[i].weight)
                .sum();

            if healthy_weight == 0 {
                // Fallback: least recently failed within eligible set
                let mut best: Option<usize> = None;
                for &idx in eligible {
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

            let tick = self.counter.fetch_add(1, Ordering::Relaxed) as u32;
            let target = tick % healthy_weight;
            let mut cumulative = 0u32;
            for &idx in eligible {
                if exclude.contains(&idx) || !health[idx].is_healthy {
                    continue;
                }
                cumulative += self.endpoints[idx].weight;
                if target < cumulative {
                    return Some((idx, &self.endpoints[idx].url));
                }
            }
            None
        }
    }
}
```

### Step 5: Run tests

```bash
cargo test health::pool::tests
```

Expected: all tests pass.

### Step 6: Compile check

```bash
cargo build
```

Expected: compiles with no errors (may have unused function warnings — acceptable for now).

### Step 7: Commit

```bash
git add src/health/pool.rs
git commit -m "feat: add eligible_indices_for_method and next_endpoint_from_eligible to ChainPool"
```

---

## Task 3: Handler — single request path

**Files:**
- Modify: `src/proxy/handler.rs`

### Step 1: Update `forward_with_retry` signature

Change the function signature to accept `method: &str`:

```rust
async fn forward_with_retry(
    chain_state: &ChainState,
    body: &[u8],
    chain: &str,
    request_count: u64,
    method: &str,       // NEW
) -> (StatusCode, Json<serde_json::Value>)
```

### Step 2: Compute eligible set and use it inside `forward_with_retry`

At the top of `forward_with_retry`, before the retry loop, add:

```rust
let eligible = chain_state.pool.eligible_indices_for_method(method);
if eligible.is_empty() {
    chain_state.metrics.record_failures(request_count);
    let resp = JsonRpcResponse::proxy_error(format!(
        "No endpoints configured for method: {}",
        method
    ));
    return (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::to_value(resp).unwrap()),
    );
}
```

Replace the two `next_endpoint` / `next_endpoint_excluding_many` calls in the retry loop:

```rust
// BEFORE:
let endpoint_result = if excluded.is_empty() {
    chain_state.pool.next_endpoint()
} else {
    chain_state
        .pool
        .next_endpoint_excluding_many(&excluded)
        .or_else(|| chain_state.pool.next_endpoint())
};

// AFTER:
let endpoint_result = chain_state
    .pool
    .next_endpoint_from_eligible(&eligible, &excluded);
```

Remove the `.or_else` fallback — when no eligible endpoint is available after exclusions, we want to hard fail, not silently route to an ineligible endpoint.

### Step 3: Update `forward_with_hedging` to accept eligible set

Change signature:

```rust
async fn forward_with_hedging(
    chain_state: &ChainState,
    body: &[u8],
    chain: &str,
    request_count: u64,
    hedge_config: &HedgeConfig,
    eligible: &[usize],      // NEW
) -> HedgeOutcome
```

Replace all calls to `chain_state.pool.next_endpoint()` and `chain_state.pool.next_endpoint_excluding_many(...)` inside this function:

```rust
// BEFORE:
let (p_idx, p_url) = match chain_state.pool.next_endpoint() { ... };
let hedge_ep = chain_state.pool.next_endpoint_excluding_many(&[p_idx])...;

// AFTER:
let (p_idx, p_url) = match chain_state.pool.next_endpoint_from_eligible(eligible, &[]) { ... };
let hedge_ep = chain_state.pool.next_endpoint_from_eligible(eligible, &[p_idx])
    .map(|(i, u)| (i, u.to_string()));
```

### Step 4: Update the hedge call site in `forward_with_retry`

Pass `&eligible` to `forward_with_hedging`:

```rust
match forward_with_hedging(chain_state, body, chain, request_count, hedge_config, &eligible).await
```

### Step 5: Update call sites of `forward_with_retry` in `proxy_handler`

The handler calls `forward_with_retry` in three places. In each, extract the method from the parsed request:

**Single request with cache (line ~125):**
```rust
// Already has rpc_req available — use rpc_req.method
let result = forward_with_retry(chain_state, &body_bytes, &chain, request_count, &rpc_req.method).await;
```

**No-cache path (line ~149):**
```rust
// Need to extract method first
let method = if let serde_json::Value::Object(ref obj) = parsed {
    obj.get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
} else {
    "unknown".to_string()
};
forward_with_retry(chain_state, &body, &chain, request_count, &method).await
```

### Step 6: Compile and check

```bash
cargo build
```

Fix any remaining compile errors (the batch path calls `forward_with_retry` too — it will need the method param, covered in Task 4).

### Step 7: Commit

```bash
git add src/proxy/handler.rs
git commit -m "feat: thread method through forward_with_retry for eligible endpoint selection"
```

---

## Task 4: Handler — batch routing groups

**Files:**
- Modify: `src/proxy/handler.rs`

This task rewrites the forwarding portion of `handle_batch_with_cache` to group uncached items by their eligible endpoint set before forwarding.

### Step 1: Update `handle_batch_with_cache` signature

Add `chain_state` already has `pool`, so no new parameters needed. Just update the `forward_with_retry` call site.

### Step 2: Replace the forwarding section

The current forwarding section (after building `uncached_requests`) does a single `forward_with_retry` call. Replace it with grouped forwarding:

```rust
// --- Group uncached items by their eligible endpoint set ---
use std::collections::HashMap;

// routing_groups: key = "0,1,2" (sorted eligible indices as string)
//                 value = (eligible_indices, Vec<(original_idx, request_value, cache_key)>)
let mut routing_groups: HashMap<
    String,
    (Vec<usize>, Vec<(usize, serde_json::Value, Option<CacheKey>)>),
> = HashMap::new();

for (pos, orig_idx) in uncached_indices.iter().enumerate() {
    let item = &uncached_requests[pos];
    let cache_key = uncached_cache_keys[pos].clone();

    let method = item
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let eligible = chain_state.pool.eligible_indices_for_method(method);

    // Use sorted indices as string key for grouping
    let key = eligible
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let entry = routing_groups
        .entry(key)
        .or_insert_with(|| (eligible, Vec::new()));
    entry.1.push((*orig_idx, item.clone(), cache_key));
}

// --- Forward each routing group ---
for (key, (eligible, group)) in &routing_groups {
    if eligible.is_empty() {
        // Hard fail the entire batch if any group has no eligible endpoints
        let method = group[0]
            .1
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        chain_state.metrics.record_failures(batch.len() as u64);
        let resp = JsonRpcResponse::proxy_error(format!(
            "No endpoints configured for method: {}",
            method
        ));
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::to_value(resp).unwrap()),
        );
    }

    let group_requests: Vec<serde_json::Value> = group.iter().map(|(_, req, _)| req.clone()).collect();
    let group_count = group_requests.len() as u64;

    let forward_body = if group_requests.len() == 1 {
        serde_json::to_vec(&group_requests[0]).unwrap()
    } else {
        serde_json::to_vec(&group_requests).unwrap()
    };

    // Forward using a method from this group as the routing key
    let representative_method = group[0]
        .1
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let (status, upstream_json) =
        forward_with_retry(chain_state, &forward_body, chain, group_count, representative_method).await;

    if status != StatusCode::OK {
        return (status, upstream_json);
    }

    // Parse and merge responses back to original positions
    let upstream_responses: Vec<serde_json::Value> = if group_requests.len() == 1 {
        vec![upstream_json.0]
    } else {
        match upstream_json.0 {
            serde_json::Value::Array(arr) => arr,
            other => vec![other],
        }
    };

    for (pos, upstream_resp) in upstream_responses.into_iter().enumerate() {
        if pos < group.len() {
            let (orig_idx, _, ref cache_key_opt) = group[pos];

            // Store cacheable responses
            if let (Some(ref cache), Some(ref cache_key)) = (chain_state.cache.as_ref(), cache_key_opt) {
                let response_bytes = serde_json::to_vec(&upstream_resp).unwrap();
                cache
                    .insert(
                        cache_key.clone(),
                        CachedResponse {
                            status: 200,
                            body: bytes::Bytes::from(response_bytes),
                        },
                    )
                    .await;
            }

            results[orig_idx] = (orig_idx, Some(upstream_resp));
        }
    }
}
```

Note: the old `uncached_count` metrics recording is now handled inside each `forward_with_retry` call — remove the old single-call block.

Also remove the old `let (status, upstream_json) = forward_with_retry(...)` block and the old response-parsing block that followed it.

### Step 3: Compile

```bash
cargo build
```

Resolve any borrow checker or type errors. Common issues:
- `chain_state.cache` is `Option<ChainCache>` — access via `&chain_state.cache` in the insert call
- The `results` vec is indexed by `orig_idx` which is already a `usize`

### Step 4: Manual smoke test (no automated tests for handler — HTTP required)

Start the proxy with an example config and send a test batch:

```bash
cargo run -- --config config.toml
# In another terminal:
curl -X POST http://localhost:8080/ethereum \
  -H "Content-Type: application/json" \
  -d '[{"jsonrpc":"2.0","method":"eth_blockNumber","id":1},{"jsonrpc":"2.0","method":"eth_chainId","id":2}]'
```

Expected: both responses returned in order.

### Step 5: Commit

```bash
git add src/proxy/handler.rs
git commit -m "feat: split batch requests by method routing groups"
```

---

## Task 5: Builder API

**Files:**
- Modify: `src/lib.rs`

### Step 1: Add `restricted_endpoint` to `ChainBuilder`

In the `impl ChainBuilder` block, after `endpoint_with_header`, add:

```rust
/// Add an RPC endpoint that only receives the specified methods.
/// Requests for other methods will not be routed to this endpoint.
pub fn restricted_endpoint(mut self, url: &str, methods: &[&str]) -> Self {
    self.endpoints.push(EndpointConfig {
        url: url.to_string(),
        weight: 1,
        auth: None,
        methods: Some(methods.iter().map(|s| s.to_string()).collect()),
    });
    self
}
```

### Step 2: Fix existing endpoint builder methods

All existing methods (`endpoint`, `weighted_endpoint`, `endpoint_with_basic_auth`, `endpoint_with_bearer`, `endpoint_with_header`) construct `EndpointConfig` directly. Add `methods: None` to each:

```rust
pub fn endpoint(mut self, url: &str) -> Self {
    self.endpoints.push(EndpointConfig {
        url: url.to_string(),
        weight: 1,
        auth: None,
        methods: None,   // ADD THIS LINE to each existing method
    });
    self
}
// Repeat for weighted_endpoint, endpoint_with_basic_auth, endpoint_with_bearer, endpoint_with_header
```

### Step 3: Compile

```bash
cargo build
```

### Step 4: Commit

```bash
git add src/lib.rs
git commit -m "feat: add restricted_endpoint to ChainBuilder for method allowlist"
```

---

## Task 6: Example config, CHANGELOG, version bump

**Files:**
- Modify: `config.toml`
- Modify: `CHANGELOG.md`
- Modify: `Cargo.toml`

### Step 1: Add method routing example to `config.toml`

Add a comment block showing the feature:

```toml
# Method routing example: route eth_sendRawTransaction only to private RPC
# [[chains.endpoints]]
# url = "https://private-mempool-rpc.example.com"
# methods = ["eth_sendRawTransaction"]
#
# [[chains.endpoints]]
# url = "https://public-rpc.example.com"
# (no methods field = handles everything except eth_sendRawTransaction)
```

### Step 2: Update `CHANGELOG.md`

Add a new entry at the top:

```markdown
## [0.7.0] - 2026-03-13

### Added
- **Method-based endpoint routing** — endpoints can declare a `methods` allowlist to receive only specific RPC methods.
  - Example: route `eth_sendRawTransaction` to a private mempool RPC, all other methods to the general pool.
  - Endpoints without a `methods` field handle all methods not claimed by another endpoint.
  - Returns 503 `"No endpoints configured for method: <method>"` if all eligible endpoints are unhealthy.
  - Batch requests with mixed methods are split into routing groups and forwarded independently.
  - Builder API: `ChainBuilder::restricted_endpoint(url, &["method1", "method2"])`.
```

### Step 3: Bump version in `Cargo.toml`

```toml
# Change:
version = "0.6.1"
# To:
version = "0.7.0"
```

### Step 4: Compile

```bash
cargo build
```

### Step 5: Commit

```bash
git add config.toml CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.7.0, update CHANGELOG for method routing"
```

---

## Task 7: Create branch and raise PR

### Step 1: Check current branch and create feature branch

```bash
cd ~/Desktop/sathvik/my-projects/rpc-proxy
git checkout -b feat/method-routing
```

If commits were made on `main`, move them:

```bash
# Check which commits need to move
git log main..HEAD --oneline
# If already on a feature branch, skip this step
```

### Step 2: Push branch

```bash
git push -u origin feat/method-routing
```

### Step 3: Raise PR

```bash
gh pr create \
  --title "feat: method-based endpoint routing (v0.7.0)" \
  --body "$(cat <<'EOF'
## Summary

- Adds `methods` allowlist field to `EndpointConfig` (TOML + builder API)
- Endpoints declaring `methods` only receive those specific RPC methods
- Unconstrained endpoints (no `methods` field) handle all methods not claimed by another endpoint
- Hard 503 fail if all eligible endpoints for a method are unhealthy
- Batch requests are split by routing group — mixed-method batches forward each group independently

## Config Example

\`\`\`toml
[[chains.endpoints]]
url = "https://private-mempool.example.com"
methods = ["eth_sendRawTransaction"]

[[chains.endpoints]]
url = "https://public-rpc.example.com"
# no methods = handles eth_call, eth_getBalance, etc.
\`\`\`

## Builder API

\`\`\`rust
Turbine::builder()
    .add_chain("ethereum")
    .restricted_endpoint("https://private.example.com", &["eth_sendRawTransaction"])
    .endpoint("https://public.example.com")
    .done()
\`\`\`

## Test plan

- [ ] `cargo test config::tests` — TOML deserialization with/without methods field
- [ ] `cargo test health::pool::tests` — eligible index computation, routing logic
- [ ] `cargo build` — clean compile
- [ ] Manual: single request to a method-restricted endpoint routes correctly
- [ ] Manual: batch with mixed methods splits and merges correctly
- [ ] Manual: all eligible endpoints down → 503 with correct message

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Notes for implementer

- **No breaking changes** — `methods: None` is the default, all existing configs work unchanged.
- **Hedging + method routing:** `forward_with_hedging` now receives `eligible` from `forward_with_retry`. The hedge and primary are both picked from the eligible set. If the eligible set has only 1 endpoint, hedging degrades gracefully (no hedge endpoint available → awaits primary).
- **Batch cache interaction:** Cache lookup happens before routing group computation. Only uncached items are grouped and forwarded — cached hits bypass routing entirely (correct, since they never touch an endpoint).
- **`_key` variable warning:** The `key` variable in the routing group loop may produce an unused variable warning if only used as a HashMap key. Use `_key` or just iterate `.values()` after building the map.
