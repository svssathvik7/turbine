[![CI](https://github.com/svssathvik7/turbine/actions/workflows/ci.yml/badge.svg)](https://github.com/svssathvik7/turbine/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/turbine-rpc-proxy)](https://crates.io/crates/turbine-rpc-proxy)
[![Docker Pulls](https://img.shields.io/docker/pulls/svssathvik7/turbine)](https://hub.docker.com/r/svssathvik7/turbine)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![GitHub Stars](https://img.shields.io/github/stars/svssathvik7/turbine)](https://github.com/svssathvik7/turbine)

# Turbine

Multi-chain RPC proxy with intelligent endpoint rotation. Unlike EVM-only proxies, Turbine works with any blockchain that speaks JSON-RPC over HTTP.

## How It Works

```
                         ┌─────────────────────────────────────────────────┐
                         │                   TURBINE                      │
                         │                                                │
  Client Request         │  ┌─────────┐    ┌───────────┐    ┌─────────┐  │
  POST /ethereum    ───────>│  Route   │───>│   Cache   │───>│ Forward │──│──> Endpoint A ✓ (auth injected)
  POST /bitcoin          │  │ Resolve  │    │  Lookup   │    │ + Retry │  │
  POST /solana           │  └─────────┘    └───────────┘    └─────────┘  │
                         │       │          hit? │ miss        │    │     │
                         │       │               │             │    │     │
                         │       │          return cached    fail  retry  │
                         │       │                             │    │     │
                         │       │                             ▼    │     │
                         │       │              ┌──────────────────┐│     │
                         │       │              │   Endpoint Pool  ││     │
                         │       │              │                  ││     │
                         │       │              │  A ✓  B ✓  C ✗  │├─────│──> Endpoint B ✓ (auth injected)
                         │       │              │  round-robin /   ││     │
                         │       │              │  weighted select ││     │
                         │       │              └──────────────────┘│     │
                         │       │                      ▲           │     │
                         │       │              ┌───────┴────────┐  │     │
                         │       │              │ Health Checker  │  │     │
                         │       ▼              │                 │  │     │
                         │  ┌──────────┐        │ • block height  │  │     │
                         │  │Dashboard │        │ • stale detect  │  │     │
                         │  │/dashboard│        │ • auto-cooldown │  │     │
                         │  │/metrics  │        └─────────────────┘  │     │
                         │  │/api/stat │                                   │
                         │  └──────────┘                                   │
                         │                                                │
                         └─────────────────────────────────────────────────┘

  Flow: Request ──> Route to chain ──> Check cache ──> Pick healthy endpoint
        ──> Inject auth (Basic/Bearer/Header) ──> Forward ──> Hedge if slow ──> On fail, retry next
```

## Features

- **Multi-chain** — configure any number of chains, each with its own endpoint pool
- **Round-robin & weighted rotation** — distribute requests evenly or by weight
- **Passive health tracking** — automatically detects and skips failing endpoints
- **Active health checks** — background block-height polling to detect stale nodes
- **Configurable retries** — set max retries and delay per chain, with automatic endpoint exclusion
- **Hedged requests** — fire parallel requests after a configurable delay to reduce tail latency
- **Per-chain rate limiting** — configurable request quotas per time window
- **Chain ID routing** — route by EVM chain ID (e.g., `/1`, `/8453`) in addition to path names
- **Response caching** — per-method TTL cache with EVM/Solana presets
- **Upstream authentication** — per-endpoint Basic Auth, Bearer tokens, or custom headers
- **Live dashboard** — real-time web UI at `/dashboard` showing chain and endpoint performance
- **Metrics API** — per-chain and per-endpoint stats via `/metrics` and `/api/status`

## Quick Start

### Install

```bash
cargo add turbine-rpc-proxy
```

### As a CLI

```bash
cargo install turbine-rpc-proxy
turbine --config config.toml
turbine --config config.toml --port 9090 --log-level debug
```

### As a Library

```rust
use turbine::Turbine;

#[tokio::main]
async fn main() {
    let turbine = Turbine::builder()
        .add_chain("ethereum")
            .endpoint("https://eth.llamarpc.com")
            .endpoint("https://rpc.ankr.com/eth")
            .endpoint_with_header("https://rpc.quicknode.com", "x-api-key", "key-123")
            .max_failures(3)
            .cooldown_secs(30)
            .cache(true)
            .cache_preset("evm")
            .hedge(500, 1)
            .done()
        .add_chain("bitcoin")
            .endpoint_with_basic_auth("http://node1:8332", "rpcuser", "pass1")
            .endpoint_with_basic_auth("http://node2:8332", "rpcuser", "pass2")
            .health_method("getblockcount")
            .done()
        .build()
        .unwrap();

    // Dashboard available at http://127.0.0.1:8080/dashboard
    turbine.serve("127.0.0.1:8080").await.unwrap();
}
```

Or embed in your existing axum app:

```rust
let turbine = Turbine::from_config("config.toml".as_ref()).unwrap();
let router = turbine.into_router();
// Merge with your own routes, add middleware, etc.
```

## Configuration

Full example — see [config.toml](config.toml) for a working sample.

```toml
[server]
host = "127.0.0.1"
port = 8080

# ─── Ethereum ───
[[chains]]
name = "ethereum"
route = "/ethereum"
chain_id = 1                       # enables routing via POST /1
rotation = "weighted"              # "round_robin" (default) or "weighted"
endpoints = [
    "https://eth.llamarpc.com",    # simple URL (weight defaults to 1)
    { url = "https://rpc.ankr.com/eth", weight = 2 },
    { url = "https://rpc.quicknode.com", weight = 3, auth = { header = { name = "x-api-key", value = "key-123" } } },
]

[chains.health]
max_consecutive_failures = 3       # failures before marking unhealthy (default: 3)
cooldown_seconds = 30              # seconds before retrying unhealthy endpoint (default: 30)
health_check_interval_seconds = 30 # how often to poll block height (default: 30)
max_block_lag = 10                 # max blocks behind before marking stale (default: 10)
max_retries = 2                    # retry up to 2 times on failure (default: 1)
retry_delay_ms = 100               # wait 100ms between retries (default: 0)
# health_method = "eth_blockNumber"  # auto-detected for known chains

[chains.rate_limit]
max_requests = 100                 # max requests per window
window_seconds = 60                # time window in seconds

[chains.hedge]
delay_ms = 500                     # fire hedge after 500ms with no response
max_count = 1                      # max 1 additional parallel request

[chains.cache]
enabled = true
preset = "evm"                     # "evm", "solana", or omit for no preset
max_capacity = 10000               # max cached entries (default: 10000)

[[chains.cache.methods]]           # override TTL for specific methods
name = "eth_blockNumber"
ttl_seconds = 5

# ─── Bitcoin ───
[[chains]]
name = "bitcoin"
route = "/bitcoin"
endpoints = [
    { url = "http://node1:8332", auth = { basic = { username = "rpcuser", password = "pass1" } } },
    { url = "http://node2:8332", auth = { basic = { username = "rpcuser", password = "pass2" } } },
]

[chains.health]
max_consecutive_failures = 3
cooldown_seconds = 30
health_method = "getblockcount"    # Bitcoin uses getblockcount instead of eth_blockNumber

# ─── Solana ───
[[chains]]
name = "solana"
route = "/solana"
rotation = "weighted"
endpoints = [
    { url = "https://api.mainnet-beta.solana.com", weight = 3 },
    { url = "https://rpc.ankr.com/solana", weight = 1 },
]

[chains.health]
max_consecutive_failures = 5
cooldown_seconds = 60
health_method = "getSlot"
health_check_interval_seconds = 15
max_block_lag = 50

[chains.cache]
enabled = true
preset = "solana"
```

### Configuration Reference

#### `[server]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `host` | string | required | IP address to bind to |
| `port` | integer | required | Port number |

#### `[[chains]]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | Chain identifier (used in logs and dashboard) |
| `route` | string | required | HTTP path prefix (e.g., `/ethereum`) |
| `chain_id` | integer | none | EVM chain ID for numeric routing (e.g., `1` for Ethereum) |
| `rotation` | string | `"round_robin"` | `"round_robin"` or `"weighted"` |
| `endpoints` | array | required | List of RPC endpoint URLs or objects |

#### Endpoint formats

```toml
# Simple string (weight = 1, no auth)
"https://eth.llamarpc.com"

# Full object
{ url = "https://...", weight = 3, auth = { ... } }
```

#### `[chains.health]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_consecutive_failures` | integer | `3` | Failures before marking endpoint unhealthy |
| `cooldown_seconds` | integer | `30` | Seconds to wait before retrying an unhealthy endpoint |
| `health_check_interval_seconds` | integer | `30` | Seconds between background health checks |
| `max_block_lag` | integer | `10` | Max blocks an endpoint can lag behind before being marked stale |
| `health_method` | string | auto-detected | JSON-RPC method for health checks |
| `max_retries` | integer | `1` | Max retries on failure (total attempts = max_retries + 1) |
| `retry_delay_ms` | integer | `0` | Milliseconds to wait between retries |

**Auto-detected health methods:** The health check method is automatically chosen based on chain name:

| Chain name contains | Health method |
|---------------------|---------------|
| `bitcoin`, `btc` | `getblockcount` |
| `solana` | `getSlot` |
| `starknet` | `starknet_blockNumber` |
| Everything else (EVM) | `eth_blockNumber` |

#### `[chains.rate_limit]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_requests` | integer | required | Maximum requests allowed per window |
| `window_seconds` | integer | required | Time window in seconds |

Rate limiting is optional per-chain. When configured, requests exceeding the limit receive HTTP 429.

#### `[chains.hedge]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `delay_ms` | integer | required | Milliseconds to wait before firing a hedge request |
| `max_count` | integer | `1` | Maximum number of additional parallel requests |

Hedging is optional per-chain and requires at least 2 endpoints. When the primary request doesn't respond within `delay_ms`, a parallel request is sent to a different endpoint — whichever responds first wins. This dramatically reduces tail latency (p99) with minimal extra upstream load.

#### `[chains.cache]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable response caching for this chain |
| `preset` | string | none | `"evm"` or `"solana"` — loads default TTLs for common methods |
| `max_capacity` | integer | `10000` | Maximum number of cached entries |

**EVM preset caches:** `eth_chainId` (24h), `net_version` (24h), `eth_getBlockByNumber` (5m), `eth_getBlockByHash` (5m), `eth_getTransactionByHash` (5m), `eth_getTransactionReceipt` (5m), `eth_getCode` (5m)

**Solana preset caches:** `getGenesisHash` (24h), `getVersion` (1h), `getBlock` (5m), `getTransaction` (5m)

Override any method's TTL with `[[chains.cache.methods]]`:

```toml
[[chains.cache.methods]]
name = "eth_blockNumber"
ttl_seconds = 5
```

### Authentication

Auth is optional per-endpoint. Three methods are supported:

| Method | Config | Use Case |
|--------|--------|----------|
| Basic Auth | `{ basic = { username, password } }` | Bitcoin Core, self-hosted nodes |
| Bearer Token | `{ bearer = "token" }` | Managed RPC providers |
| Custom Header | `{ header = { name, value } }` | Alchemy (`x-api-key`), QuickNode |

Clients don't need credentials — Turbine injects them automatically when forwarding to upstream nodes.

## Dashboard

Turbine includes a built-in live dashboard at `GET /dashboard`. No setup needed — it's served directly from the binary.

The dashboard shows:
- **Overview** — total requests, global success rate, active chains, cache hit rate
- **Per-chain cards** — request stats, success rate bars, endpoint health visualization
- **Per-endpoint details** — URL, health status, latency (rolling average), block height, weight, request/success/failure counts
- **Health indicators** — green (healthy), yellow (degraded — some endpoints down), red (all endpoints down)

The page auto-refreshes every 5 seconds. It works on mobile too.

### Monitoring Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/dashboard` | GET | Live web dashboard (HTML) |
| `/api/status` | GET | Detailed JSON with per-endpoint telemetry |
| `/metrics` | GET | Compact JSON with per-chain aggregate stats |

**`/api/status` response shape:**

```json
{
  "uptime_seconds": 3600,
  "chains": [
    {
      "name": "ethereum",
      "route": "/ethereum",
      "rotation": "weighted",
      "total_requests": 15000,
      "successful_requests": 14850,
      "failed_requests": 150,
      "cache_hits": 5000,
      "cache_misses": 10000,
      "active_endpoints": 3,
      "total_endpoints": 4,
      "endpoints": [
        {
          "url": "https://eth-mainnet.g.alchemy.com/v2/...",
          "weight": 3,
          "is_healthy": true,
          "consecutive_failures": 0,
          "block_height": 24628500,
          "last_latency_ms": 45,
          "rolling_latency_ms": 52.3,
          "request_count": 5000,
          "success_count": 4990,
          "failure_count": 10
        }
      ]
    }
  ]
}
```

**`/metrics` response shape:**

```json
[
  {
    "chain": "ethereum",
    "total_requests": 15000,
    "successful_requests": 14850,
    "failed_requests": 150,
    "cache_hits": 5000,
    "cache_misses": 10000,
    "active_endpoints": 3,
    "total_endpoints": 4
  }
]
```

## Usage

```bash
# Send a JSON-RPC request through the proxy
curl -X POST http://localhost:8080/ethereum \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Bitcoin (auth handled by Turbine, client sends plain request)
curl -X POST http://localhost:8080/bitcoin \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'

# Batch request (multiple calls in one HTTP request)
curl -X POST http://localhost:8080/ethereum \
  -H "Content-Type: application/json" \
  -d '[{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1},{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":2}]'

# View metrics
curl http://localhost:8080/metrics

# Detailed status with per-endpoint data
curl http://localhost:8080/api/status

# Route by chain ID (if chain_id = 1 configured for ethereum)
curl -X POST http://localhost:8080/1 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Open dashboard in browser
open http://localhost:8080/dashboard
```

## Builder API Reference

All builder methods for programmatic configuration:

```rust
Turbine::builder()
    .add_chain("name")                              // start configuring a chain
        .route("/custom-route")                     // custom route (default: /{name})
        .endpoint("https://...")                    // add endpoint (weight=1, no auth)
        .weighted_endpoint("https://...", 3)        // add endpoint with weight
        .endpoint_with_basic_auth("url", "u", "p") // Basic Auth endpoint
        .endpoint_with_bearer("url", "token")       // Bearer token endpoint
        .endpoint_with_header("url", "key", "val")  // Custom header endpoint
        .max_failures(3)                            // failures before unhealthy
        .cooldown_secs(30)                          // cooldown before retry
        .health_method("eth_blockNumber")           // health check RPC method
        .health_check_interval(30)                  // health check interval (secs)
        .max_block_lag(10)                          // max block lag for staleness
        .weighted()                                 // use weighted rotation
        .cache(true)                                // enable caching
        .cache_preset("evm")                        // load preset TTLs
        .cache_max_capacity(10000)                  // max cache entries
        .cache_method("eth_blockNumber", 5)         // custom method TTL
        .chain_id(1)                                // EVM chain ID for /{id} routing
        .rate_limit(100, 60)                        // 100 requests per 60 seconds
        .hedge(500, 1)                              // hedge after 500ms, max 1 extra request
        .max_retries(2)                             // retry up to 2 times on failure
        .retry_delay_ms(100)                        // 100ms between retries
        .done()                                     // finish chain, return to builder
    .build()                                        // build Turbine instance
```

## How Health Tracking Works

Turbine uses two layers of health tracking:

**Passive tracking** happens on every request:
1. If a forwarded request fails (timeout, connection error, HTTP 429/5xx), the endpoint's failure counter increments
2. After `max_consecutive_failures` failures, the endpoint is marked unhealthy and skipped
3. After `cooldown_seconds`, the endpoint is retried — if it succeeds, it's marked healthy again
4. If all endpoints are unhealthy, Turbine picks the least-recently-failed one

**Active health checks** run in the background:
1. Every `health_check_interval_seconds`, Turbine calls the health method on each endpoint
2. It compares block heights across endpoints
3. If an endpoint is more than `max_block_lag` blocks behind the highest, it's marked stale (unhealthy)
4. When a stale endpoint catches up, it's automatically re-enabled

## License

MIT
