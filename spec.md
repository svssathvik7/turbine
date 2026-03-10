# Turbine — Multi-Chain RPC Proxy

## Overview

Turbine is an open-source, multi-chain RPC proxy built in Rust. It accepts JSON-RPC requests, routes them to a pool of upstream RPC endpoints per chain, and intelligently rotates across them to balance load and avoid rate limits.

Unlike eRPC (which is EVM-only), Turbine is chain-agnostic — any blockchain that speaks JSON-RPC over HTTP is supported out of the box.

---

## Implemented Features

### Core Proxy

- **HTTP JSON-RPC Proxy** — accepts JSON-RPC 2.0 over HTTP, forwards to upstream, returns responses
- **Batch support** — handles both single and batch JSON-RPC requests
- **Multi-chain routing** — path-based routing (e.g., `/ethereum`, `/bitcoin`, `/solana`)
- **Auto-retry** — on failure, retries with a different healthy endpoint (1 retry)

### Load Balancing

- **Round-robin rotation** — cycles through healthy endpoints sequentially (default)
- **Weighted rotation** — distributes requests proportional to configured weights
- **Lock-free selection** — uses atomic counters, no mutex contention

### Health Tracking

- **Passive tracking** — counts consecutive failures per endpoint, marks unhealthy after threshold
- **Active health checks** — background task polls block height on each endpoint periodically
- **Staleness detection** — marks endpoints unhealthy if they lag behind by more than `max_block_lag` blocks
- **Auto-recovery** — unhealthy endpoints are retried after cooldown; stale endpoints re-enabled when caught up
- **Fallback** — if all endpoints are unhealthy, picks the least-recently-failed one

### Caching

- **Per-method TTL caching** — Moka async cache with configurable TTLs per JSON-RPC method
- **Presets** — EVM and Solana presets with sensible defaults for common methods
- **Batch optimization** — splits batch requests into cached hits + uncached misses, forwards only misses

### Authentication

- **Basic Auth** — username + password (Bitcoin Core, self-hosted nodes)
- **Bearer Token** — `Authorization: Bearer <token>` (managed providers)
- **Custom Header** — e.g., `x-api-key` (Alchemy, QuickNode)
- Per-endpoint, optional, injected automatically on forwarding

### Monitoring

- **Live dashboard** — self-contained HTML page at `/dashboard` with real-time chain and endpoint performance
- **Status API** — detailed JSON at `/api/status` with per-endpoint telemetry (block height, latency, request counts)
- **Metrics API** — compact JSON at `/metrics` with per-chain aggregate stats
- **Per-endpoint telemetry** — block height, rolling average latency, request/success/failure counts

### Configuration

- **TOML config** — file-based configuration with validation
- **Builder API** — programmatic chainable DSL for embedding in Rust apps
- **CLI** — `turbine --config config.toml [--port 9090] [--log-level debug]`
- **Auto-detected health methods** — bitcoin/btc → `getblockcount`, solana → `getSlot`, starknet → `starknet_blockNumber`, default → `eth_blockNumber`

---

## Architecture

```
                          ┌──────────────────────────────────┐
                          │          Turbine Proxy            │
                          │                                   │
  Client ──► HTTP ──────► │  Router (path-based)              │
  JSON-RPC                │    │                              │
                          │    ├── /ethereum ──► Pool ──► Fwd │──► Endpoints
                          │    ├── /bitcoin  ──► Pool ──► Fwd │──► Endpoints
                          │    ├── /solana   ──► Pool ──► Fwd │──► Endpoints
                          │    │                              │
                          │    ├── /dashboard  (live UI)      │
                          │    ├── /api/status (detailed JSON)│
                          │    └── /metrics    (compact JSON) │
                          │                                   │
                          │  Health Checker (background task) │
                          │  Cache (Moka, per-method TTL)     │
                          │  Metrics (atomic counters)        │
                          └──────────────────────────────────┘
```

### Components

| Component | File(s) | Responsibility |
|-----------|---------|----------------|
| **Server** | `proxy/server.rs` | HTTP server (axum), route registration, dashboard/metrics handlers |
| **Handler** | `proxy/handler.rs` | JSON-RPC request parsing, batch handling, cache integration, retry logic |
| **Forwarder** | `proxy/forwarder.rs` | HTTP forwarding with auth injection, error classification, latency measurement |
| **Endpoint Pool** | `health/pool.rs` | Endpoint selection (round-robin/weighted), health state access |
| **Health State** | `health/state.rs` | Per-endpoint failure counts, block height, latency tracking |
| **Health Checker** | `health/checker.rs` | Background block-height polling, staleness detection |
| **Cache** | `cache.rs` | Moka async cache, presets, per-method TTL, batch splitting |
| **Metrics** | `metrics.rs` | Atomic counters per chain (requests, successes, failures, cache hits/misses) |
| **Dashboard** | `dashboard.rs` | Self-contained HTML/CSS/JS dashboard page |
| **Config** | `config.rs` | TOML parsing, validation, endpoint auth variants |
| **Types** | `types.rs` | JSON-RPC request/response/error structures |
| **Turbine** | `lib.rs` | Public API (builder pattern), config loading, router generation |

### Tech Stack

| Dependency | Purpose |
|------------|---------|
| `axum 0.8` | HTTP server + routing |
| `tokio 1` | Async runtime |
| `reqwest 0.12` | HTTP client for upstream requests |
| `serde` / `serde_json` | JSON serialization |
| `toml 0.8` | TOML config parsing |
| `tracing` | Structured logging |
| `clap 4` | CLI argument parsing |
| `moka 0.12` | Async in-memory cache |
| `bytes 1` | Efficient byte handling |

---

## Request Flow

1. Client sends `POST /<chain-route>` with a JSON-RPC body
2. Router matches the path to a chain
3. If caching is enabled, check cache for a hit
4. On cache hit: return cached response, increment `cache_hits`
5. On cache miss: select next healthy endpoint via rotation strategy
6. Forwarder sends request to endpoint (with auth injected), measures latency
7. On success: return response, cache it if cacheable, record latency, increment `successful_requests`
8. On failure (timeout, connection error, HTTP 429/5xx):
   - Record failure on endpoint, increment failure counter
   - If failures >= threshold, mark endpoint unhealthy
   - Retry with the next healthy endpoint (max 1 retry)
9. If retry also fails: return JSON-RPC error to client, increment `failed_requests`

---

## Project Structure

```
turbine/
├── Cargo.toml
├── config.toml              # Example configuration
├── spec.md                  # This file
├── README.md                # User-facing documentation
├── src/
│   ├── main.rs              # CLI entry point (clap args, server startup)
│   ├── lib.rs               # Public Turbine struct, builder API
│   ├── config.rs            # TOML parsing, validation, auth variants
│   ├── types.rs             # JSON-RPC request/response/error types
│   ├── metrics.rs           # Atomic counters per chain
│   ├── cache.rs             # Moka cache, presets, TTL management
│   ├── dashboard.rs         # Self-contained HTML dashboard
│   ├── proxy/
│   │   ├── mod.rs           # AppState, ChainState structs
│   │   ├── server.rs        # Router setup, metrics/dashboard/status handlers
│   │   ├── handler.rs       # Request handling, cache logic, retry
│   │   └── forwarder.rs     # HTTP forwarding, auth injection, latency
│   └── health/
│       ├── mod.rs           # Module exports
│       ├── pool.rs          # Endpoint pool, rotation, health access
│       ├── state.rs         # Per-endpoint health + telemetry state
│       └── checker.rs       # Background health check task
```

---

## Future

- WebSocket support (subscriptions)
- Latency-based rotation strategy
- Prometheus metrics export format
- Persistent metrics storage
- Docker / deployment tooling
- Horizontal scaling

---

## CLI

```
turbine --config config.toml
```

Flags:
- `--config <path>` — path to TOML config file (default: `config.toml`)
- `--port <port>` — override port from config
- `--log-level <level>` — trace, debug, info, warn, error (default: info)
