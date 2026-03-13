# Method-Based Endpoint Routing — Design

**Date:** 2026-03-13
**Version target:** 0.7.0 (next minor)
**Status:** Approved

## Problem

All endpoints in a chain pool receive all RPC methods regardless of their capability or intent. There is no way to restrict `eth_sendRawTransaction` to only private/premium RPCs, or `debug_traceTransaction` to only archive nodes.

## Decision

**Option A — per-endpoint allowlist** was chosen over a chain-level routing table. Config lives next to the endpoint it affects, mirrors how real providers work, and is the simplest model for the common case.

## Config Schema

Add an optional `methods` field to `EndpointConfig`:

```toml
[[chains.endpoints]]
url = "https://private-rpc.example.com"
methods = ["eth_sendRawTransaction"]   # only receives these methods

[[chains.endpoints]]
url = "https://public-rpc.example.com"
# no methods field = receives everything not claimed by another endpoint
```

## Routing Semantics

For a given method M:
- If **any** endpoint claims M in its `methods` list → route **only** to those endpoints
- If **no** endpoint claims M → route to all endpoints with `methods = None` (unconstrained)
- Endpoints with a non-empty `methods` list are **excluded** from unconstrained routing

## Component Changes

### `config.rs`
- Add `methods: Option<Vec<String>>` to `EndpointConfig` (defaults `None`)
- Validation: warn if `methods = []` (empty list is a no-op)

### `health/pool.rs`
- Add `fn eligible_indices_for_method(&self, method: &str) -> Vec<usize>`
  - Returns claimed endpoint indices if any; else unconstrained endpoint indices
- Add `fn next_endpoint_for_method(&self, method: &str, exclude: &[usize]) -> Option<(usize, &str)>`
  - Runs existing round-robin/weighted logic filtered to eligible indices
  - Returns `None` if no eligible healthy endpoint exists (triggers hard fail)

### `proxy/handler.rs`
- **Single request:** extract method, call `next_endpoint_for_method` instead of `next_endpoint`
- **Batch request:** group uncached misses by routing key (eligible endpoint index set), forward each group as a sub-batch, merge all responses back by original position

## Error Behavior

- All eligible endpoints unhealthy → 503 with message:
  `"No healthy endpoints for method: <method>"`
- Batch with one failing routing group → entire batch returns 503 immediately (no partial responses)
- No `methods` field on any endpoint → existing behavior, zero breaking changes

## Backward Compatibility

`methods` is `Option<Vec<String>>` defaulting to `None`. All existing configs work unchanged.
