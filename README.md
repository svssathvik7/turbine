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
                         │  ┌─────────┐         │ • block height  │  │     │
                         │  │ Metrics │         │ • stale detect  │  │     │
                         │  │ /metrics│         │ • auto-cooldown │  │     │
                         │  └─────────┘         └─────────────────┘  │     │
                         │                                                │
                         └─────────────────────────────────────────────────┘

  Flow: Request ──> Route to chain ──> Check cache ──> Pick healthy endpoint
        ──> Inject auth (Basic/Bearer/Header) ──> Forward ──> On fail, retry next
```

## Features

- **Multi-chain** — configure any number of chains, each with its own endpoint pool
- **Round-robin & weighted rotation** — distribute requests evenly or by weight
- **Passive health tracking** — automatically detects and skips failing endpoints
- **Active health checks** — background block-height polling to detect stale nodes
- **Auto-retry** — retries with a different endpoint on failure
- **Response caching** — per-method TTL cache with EVM/Solana presets
- **Upstream authentication** — per-endpoint Basic Auth, Bearer tokens, or custom headers
- **Metrics** — per-chain request/failure/cache stats via `/metrics`

## Install

```bash
cargo add turbine-rpc-proxy
```

## As a Library

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
            .done()
        .add_chain("bitcoin")
            .endpoint_with_basic_auth("http://node1:8332", "rpcuser", "pass1")
            .endpoint_with_basic_auth("http://node2:8332", "rpcuser", "pass2")
            .health_method("getblockcount")
            .done()
        .build()
        .unwrap();

    turbine.serve("127.0.0.1:8080").await.unwrap();
}
```

Or embed in your existing axum app:

```rust
let turbine = Turbine::from_config("config.toml".as_ref()).unwrap();
let router = turbine.into_router();
// Merge with your own routes, add middleware, etc.
```

## As a CLI

```bash
cargo build --release
./target/release/turbine --config config.toml
./target/release/turbine --config config.toml --port 9090 --log-level debug
```

## Configuration

See [config.toml](config.toml) for a sample configuration. Generate one visually at [turbine-config.pages.dev](https://turbine-config.pages.dev).

```toml
[server]
host = "127.0.0.1"
port = 8080

[[chains]]
name = "ethereum"
route = "/ethereum"
endpoints = [
    "https://eth.llamarpc.com",
    { url = "https://rpc.quicknode.com", auth = { header = { name = "x-api-key", value = "key-123" } } },
]

[chains.health]
max_consecutive_failures = 3
cooldown_seconds = 30

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
health_method = "getblockcount"
```

### Authentication

Auth is optional per-endpoint. Three methods are supported:

| Method | Config | Use Case |
|---|---|---|
| Basic Auth | `{ basic = { username, password } }` | Bitcoin Core, self-hosted nodes |
| Bearer Token | `{ bearer = "token" }` | Managed RPC providers |
| Custom Header | `{ header = { name, value } }` | Alchemy (`x-api-key`), QuickNode |

Clients don't need credentials — Turbine injects them automatically when forwarding to upstream nodes.

## Usage

```bash
# Ethereum
curl -X POST http://localhost:8080/ethereum \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Bitcoin (auth handled by Turbine, client sends plain request)
curl -X POST http://localhost:8080/bitcoin \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'

# Metrics
curl http://localhost:8080/metrics
```
