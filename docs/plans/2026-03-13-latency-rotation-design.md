# Latency-Based Rotation — Design

**Date:** 2026-03-13
**Version target:** 0.8.0
**Status:** Approved

## Problem

Round-robin and weighted rotation don't account for actual endpoint performance. A slow endpoint gets the same traffic share as a fast one, hurting overall latency.

## Decision

Add `rotation = "latency"` as a new strategy. Uses **inverse-latency weighted selection combined with user-defined weights** (hybrid approach). This keeps all endpoints warm for health monitoring while routing more traffic to faster endpoints.

## Algorithm

For each healthy endpoint, compute:

```
effective_weight = user_weight × (1.0 / rolling_latency_ms)
```

Normalize effective weights to produce a probability distribution. Use existing tick-based counter for deterministic weighted selection.

**Cold start:** When `rolling_latency_ms == 0` (no data yet), use `user_weight` alone — behaves like plain weighted rotation until latency data arrives. Latency data is used immediately from the first request (rolling average smooths outliers).

**Example:**
```
Endpoint A: weight=1, latency=50ms  → effective = 1 × (1/50) = 0.020  → ~30%
Endpoint B: weight=1, latency=100ms → effective = 1 × (1/100) = 0.010 → ~15%
Endpoint C: weight=3, latency=80ms  → effective = 3 × (1/80) = 0.0375 → ~55%
```

## Config

```toml
[[chains]]
name = "ethereum"
rotation = "latency"
endpoints = [
    { url = "https://fast-rpc.example.com", weight = 1 },
    { url = "https://slow-rpc.example.com", weight = 1 },
]
```

## Builder API

```rust
Turbine::builder()
    .add_chain("ethereum")
    .latency_based()
    .endpoint("https://fast-rpc.example.com")
    .done()
```

## Component Changes

### `config.rs`
- Add `Latency` variant to `RotationStrategy` enum

### `health/pool.rs`
- Add `next_latency_based()` method — computes effective weights from `rolling_latency_ms` and user weights
- Wire into `next_endpoint()`, `next_endpoint_from_eligible()`, and excluding variants
- Add `rotation_name()` match for `Latency` → `"latency"`

### `lib.rs`
- Add `latency_based()` method to `ChainBuilder` — sets rotation to `Latency`

### `dashboard.rs`
- Display "latency" as rotation strategy name (already handled by `rotation_name()`)

## Backward Compatibility

New enum variant, no changes to existing strategies. All existing configs work unchanged.
