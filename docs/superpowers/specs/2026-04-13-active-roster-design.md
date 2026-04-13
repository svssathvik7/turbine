# Active Roster + RPC House Design

## Problem

Turbine opens TCP connections to every configured endpoint for health checks and traffic. With 92 endpoints across 10 chains, this means 92+ persistent connections just from health checks, approaching macOS's default 256 fd limit and straining deployment environments like Hetzner VPS.

## Solution

Split each chain's endpoints into an **active set** (receives traffic and health checks) and a **reserve pool (rpc_house)** that holds zero connections. Only 5 endpoints per chain are active at any time, regardless of how many are configured.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Initial active set selection | Random | Simple, no startup delay |
| Active set size | Fixed at 5, capped to `min(5, total_endpoints)` | Simple, no extra config |
| Replacement selection from reserve | Random (FIFO from rpc_house queue) | No probe cost, bad picks get demoted again quickly |
| Reserve probing | None — fully dark, 0 connections | Maximum connection savings |
| Recycling | Exhaustion-based — only recycle demoted endpoints when rpc_house is empty | Fair rotation, every endpoint gets tried before repeats |
| All endpoints unhealthy | Keep trying active set via `least_recently_failed` fallback | Some response is better than none |

## Data Structures

### ChainPool additions (`pool.rs`)

```rust
pub struct ChainPool {
    // ... existing fields unchanged ...

    // Active Roster
    active_indices: RwLock<Vec<usize>>,      // Indices into endpoints/health that receive traffic + health checks
    rpc_house: Mutex<VecDeque<usize>>,       // Reserve queue — FIFO, front = next candidate
}
```

- `active_indices`: `RwLock` because reads (endpoint selection) are frequent and concurrent, writes (demotion/promotion) are rare
- `rpc_house`: `Mutex` since only accessed during demotion/promotion (infrequent)
- Both store indices into existing `endpoints` and `health` arrays — no data duplication

### Initialization

- If `endpoints.len() <= 5`: all indices go into `active_indices`, `rpc_house` is empty
- If `endpoints.len() > 5`: randomly pick 5 for `active_indices`, rest go into `rpc_house` in random order

## Endpoint Selection

All existing selection methods (`next_round_robin`, `next_weighted`, `next_latency_based`) filter by `active_indices` instead of iterating all endpoints.

Flow:
1. `next_endpoint()` reads `active_indices` (cheap `RwLock` read)
2. Passes those indices as the candidate set to the rotation strategy
3. Rotation, latency scoring, weighted distribution — all unchanged
4. `least_recently_failed` fallback operates within active set only

`eligible_indices_for_method()` intersects with `active_indices` first — only active endpoints can be eligible for method routing.

No changes to `Forwarder`, `handler.rs`, hedging logic, or any caller.

## Health Checker

### Only probe active endpoints

`fetch_block_heights()` reads `pool.active_indices` and only probes those. Reserve endpoints get zero health checks, zero TCP connections.

### Sequential probing

Change `join_all(futures)` to a sequential loop — 1 TCP connection at a time. Set `pool_max_idle_per_host(0)` so connections close immediately after each probe.

### 5-minute interval

Health check interval default changes from 30s to 300s (5 minutes). Configurable via existing `health.health_check_interval_seconds`.

### Demotion triggers

After each health check cycle, evaluate each active endpoint:

| Trigger | Condition |
|---------|-----------|
| Hard failure | `consecutive_failures >= max_consecutive_failures` |
| Block staleness | Block height lags behind max by `> max_block_lag` |
| Chronic throttle | Throttled 3+ times since last successful health check cycle |

The chronic throttle threshold (3) is hardcoded.

### Demotion and replacement: `pool.demote_and_replace(idx)`

1. Remove `idx` from `active_indices`
2. Push `idx` to the back of `rpc_house`
3. If `rpc_house` has only the just-demoted endpoint (i.e., it was empty before step 2) — recycle: take all demoted indices currently not in `active_indices` (excluding `idx`) and push them to `rpc_house` in random order
4. Pop the front of `rpc_house` as the replacement (guaranteed non-empty after step 3, and guaranteed != `idx` since `idx` was pushed to the back)
5. Add the replacement to `active_indices`

## Dashboard and Metrics

### RosterStatus enum

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RosterStatus {
    Active,
    Reserve,
}
```

### EndpointStatus addition

```rust
pub struct EndpointStatus {
    // ... existing fields ...
    pub roster_status: RosterStatus,
}
```

`endpoint_statuses()` checks whether each index is in `active_indices` and tags accordingly. Dashboard shows reserve endpoints greyed out or in a collapsible section.

`ChainMetrics` unchanged — `total_requests`, `successful_requests`, etc. only count proxied traffic which only goes to active endpoints.

## Config Changes

No new config fields. Existing fields reused:

| Demotion trigger | Config field |
|-----------------|-------------|
| Hard failures | `health.max_consecutive_failures` |
| Block staleness | `health.max_block_lag` |
| Chronic throttle | Hardcoded at 3 |
| Health check interval | `health.health_check_interval_seconds` (default changed to 300) |

## Connection Budget

### Before (10 chains, 92 total endpoints)

| Component | Connections |
|-----------|------------|
| Health checker | 92 persistent |
| Forwarder | Up to 324 idle (81 hosts x 4) |
| **Total ceiling** | **~416** |

### After (10 chains, 50 active endpoints max)

| Component | Connections |
|-----------|------------|
| Health checker (idle) | **0** |
| Health checker (during 5min cycle) | **1** at a time |
| Forwarder max idle | **~200** (50 hosts x 4) |
| **Total ceiling** | **~200** |

Realistic steady-state with latency rotation: **20-40 connections**.
