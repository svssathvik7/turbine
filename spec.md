# Turbine — Multi-Chain RPC Proxy

## Overview

Turbine is an open-source, multi-chain RPC proxy built in Rust. It accepts JSON-RPC requests, routes them to a pool of upstream RPC endpoints per chain, and intelligently rotates across them to balance load and avoid rate limits.

Unlike eRPC (which is EVM-only), Turbine is chain-agnostic — any blockchain that speaks JSON-RPC over HTTP is supported out of the box.

---

## Stage 1 — Scope

### Core Features

#### 1. HTTP JSON-RPC Proxy
- Accept incoming JSON-RPC 2.0 requests over HTTP
- Forward requests to upstream RPC endpoints
- Return upstream JSON-RPC responses to the caller
- Support both single and batch JSON-RPC requests

#### 2. Multi-Chain Support
- Configure multiple chains, each with its own pool of RPC endpoints
- Route requests to the correct chain based on a path prefix (e.g., `/ethereum`, `/solana`, `/starknet`)
- Any chain that uses JSON-RPC over HTTP is supported — no chain-specific logic in Stage 1

#### 3. Round-Robin Rotation
- Rotate requests across the endpoint pool for each chain using round-robin
- Maintain per-chain rotation state (atomic counter)
- Skip endpoints that are currently marked unhealthy

#### 4. Passive Health Tracking
- Track consecutive failures per endpoint
- Mark an endpoint as **unhealthy** after `N` consecutive failures (configurable, default: 3)
- Mark an endpoint as **healthy** again after a configurable cooldown period has passed (default: 30s) and the next request to it succeeds
- If all endpoints for a chain are unhealthy, attempt the least-recently-failed one

#### 5. Basic Metrics
- Expose a `GET /metrics` endpoint returning JSON with per-chain stats:
  - `total_requests`: total requests received
  - `successful_requests`: requests that returned a valid JSON-RPC response
  - `failed_requests`: requests where the upstream returned an error or was unreachable
  - `active_endpoints`: number of currently healthy endpoints
  - `total_endpoints`: total configured endpoints
- Metrics reset on server restart (in-memory only for Stage 1)

#### 6. TOML Configuration
```toml
[server]
host = "127.0.0.1"
port = 8080

[[chains]]
name = "ethereum"
route = "/ethereum"

endpoints = [
    "https://eth.llamarpc.com",
    "https://rpc.ankr.com/eth",
    "https://ethereum-rpc.publicnode.com",
]

[chains.health]
max_consecutive_failures = 3
cooldown_seconds = 30

[[chains]]
name = "solana"
route = "/solana"

endpoints = [
    "https://api.mainnet-beta.solana.com",
    "https://rpc.ankr.com/solana",
]

[chains.health]
max_consecutive_failures = 5
cooldown_seconds = 60
```

### Non-Goals (Stage 1)
- No WebSocket support
- No authentication / API key passthrough
- No caching
- No chain-specific logic (block height checks, re-org awareness)
- No persistent metrics / storage
- No deployment tooling

---

## Architecture

```
                          ┌─────────────────────────────┐
                          │         Turbine Proxy        │
                          │                              │
  Client ──► HTTP ──────► │  Router (path-based)         │
  JSON-RPC                │    │                         │
                          │    ├── /ethereum ──► Pool    │
                          │    │    ├── endpoint 1       │
                          │    │    ├── endpoint 2       │
                          │    │    └── endpoint 3       │
                          │    │                         │
                          │    ├── /solana ──► Pool      │
                          │    │    ├── endpoint 1       │
                          │    │    └── endpoint 2       │
                          │    │                         │
                          │    └── /metrics              │
                          │                              │
                          │  Health Tracker (passive)    │
                          │  Metrics Collector           │
                          └─────────────────────────────┘
```

### Components

| Component | Responsibility |
|---|---|
| **Server** | HTTP server (axum), receives JSON-RPC requests |
| **Router** | Maps request path to the correct chain pool |
| **Endpoint Pool** | Holds endpoints for a chain, applies round-robin rotation |
| **Health Tracker** | Tracks consecutive failures, marks endpoints healthy/unhealthy |
| **Forwarder** | Forwards JSON-RPC request to the selected endpoint via reqwest |
| **Metrics Collector** | Aggregates per-chain request/failure counts |

### Tech Stack

| Dependency | Purpose |
|---|---|
| `axum` | HTTP server + routing |
| `tokio` | Async runtime |
| `reqwest` | HTTP client for upstream requests |
| `serde` / `serde_json` | JSON-RPC serialization |
| `toml` | Config parsing |
| `tracing` | Structured logging |

---

## Request Flow

1. Client sends `POST /<chain-route>` with a JSON-RPC body
2. Router matches the path to a chain pool
3. Endpoint pool selects the next healthy endpoint via round-robin
4. Forwarder sends the JSON-RPC request to the selected endpoint
5. On success: return response to client, increment `successful_requests`
6. On failure (timeout, connection error, HTTP 429/5xx):
   - Increment consecutive failure count for that endpoint
   - If failures >= threshold, mark endpoint unhealthy
   - Increment `failed_requests`
   - Retry with the next healthy endpoint (max 1 retry in Stage 1)
7. If no healthy endpoints remain, return a JSON-RPC error response to the client

---

## Project Structure

```
turbine/
├── Cargo.toml
├── config.toml
├── spec.md
├── src/
│   ├── main.rs           # Entry point, server startup
│   ├── config.rs          # TOML config parsing
│   ├── server.rs          # Axum server + route setup
│   ├── router.rs          # Chain route matching
│   ├── pool.rs            # Endpoint pool + round-robin logic
│   ├── health.rs          # Passive health tracking
│   ├── forwarder.rs       # HTTP client, request forwarding
│   ├── metrics.rs         # Per-chain metrics collection
│   └── types.rs           # JSON-RPC types (request, response, error)
```

---

## Stage 2 — Future

- WebSocket support (subscriptions)
- API key authentication + passthrough to upstream
- Latency-based / weighted rotation strategies
- Persistent metrics (Prometheus export)
- Chain-aware health checks (block height, staleness)
- Caching layer
- Docker / deployment tooling

---

## CLI

```
turbine --config config.toml
```

Flags:
- `--config <path>` — path to TOML config file (default: `config.toml`)
- `--port <port>` — override port from config
- `--log-level <level>` — trace, debug, info, warn, error (default: info)
