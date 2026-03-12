# Turbine

Multi-chain RPC proxy with intelligent endpoint rotation, health checks, and caching. Unlike EVM-only proxies, Turbine works with **any blockchain** that speaks JSON-RPC over HTTP.

```
Client ──> Turbine ──> Route to chain ──> Check cache ──> Pick healthy endpoint ──> Forward (with auth)
```

## Quick Start

**1. Create a `config.toml`:**

```toml
[server]
host = "0.0.0.0"
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
health_check_interval_seconds = 30
max_block_lag = 10

[chains.cache]
enabled = true
preset = "evm"
```

**2. Run:**

```bash
docker run -d \
  --name turbine \
  -v $(pwd)/config.toml:/etc/turbine/config.toml \
  -p 8080:8080 \
  svssathvik7/turbine
```

**3. Send requests:**

```bash
curl -X POST http://localhost:8080/ethereum \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

**4. Open the dashboard:**

Visit [http://localhost:8080/dashboard](http://localhost:8080/dashboard) for a live view of chain stats, endpoint health, and cache performance.

## Features

- **Multi-chain** — configure any number of chains, each with its own endpoint pool
- **Round-robin & weighted rotation** — distribute requests evenly or by weight
- **Passive health tracking** — automatically detects and skips failing endpoints
- **Active health checks** — background block-height polling to detect stale nodes
- **Auto-retry** — on failure, retries with a different healthy endpoint
- **Response caching** — per-method TTL cache with EVM and Solana presets
- **Upstream authentication** — per-endpoint Basic Auth, Bearer tokens, or custom headers
- **Live dashboard** — real-time web UI at `/dashboard`
- **Metrics API** — per-chain and per-endpoint stats via `/metrics` and `/api/status`
- **Batch support** — full JSON-RPC 2.0 batch request handling

## Multi-Chain Configuration

Turbine auto-detects health check methods based on chain name:

| Chain | Health method | Cache preset |
|-------|---------------|--------------|
| Ethereum / EVM | `eth_blockNumber` | `evm` |
| Bitcoin / BTC | `getblockcount` | — |
| Solana | `getSlot` | `solana` |
| Starknet | `starknet_blockNumber` | — |

```toml
# Bitcoin with Basic Auth
[[chains]]
name = "bitcoin"
route = "/bitcoin"
endpoints = [
    { url = "http://node1:8332", auth = { basic = { username = "rpcuser", password = "pass1" } } },
    { url = "http://node2:8332", auth = { basic = { username = "rpcuser", password = "pass2" } } },
]

[chains.health]
health_method = "getblockcount"

# Solana with weighted rotation
[[chains]]
name = "solana"
route = "/solana"
rotation = "weighted"
endpoints = [
    { url = "https://api.mainnet-beta.solana.com", weight = 3 },
    { url = "https://rpc.ankr.com/solana", weight = 1 },
]

[chains.health]
health_method = "getSlot"
health_check_interval_seconds = 15
max_block_lag = 50

[chains.cache]
enabled = true
preset = "solana"
```

## Endpoint Authentication

Clients don't need credentials — Turbine injects them when forwarding upstream.

| Method | Config | Use case |
|--------|--------|----------|
| Basic Auth | `{ basic = { username, password } }` | Bitcoin Core, self-hosted nodes |
| Bearer Token | `{ bearer = "token" }` | Managed RPC providers |
| Custom Header | `{ header = { name, value } }` | Alchemy (`x-api-key`), QuickNode |

```toml
endpoints = [
    { url = "https://rpc.quicknode.com", auth = { header = { name = "x-api-key", value = "your-key" } } },
    { url = "https://provider.io/rpc", auth = { bearer = "your-token" } },
]
```

## Docker Compose

```yaml
services:
  turbine:
    image: svssathvik7/turbine
    ports:
      - "8080:8080"
    volumes:
      - ./config.toml:/etc/turbine/config.toml
    restart: unless-stopped
```

## CLI Options

```bash
# Override port and log level
docker run -v $(pwd)/config.toml:/etc/turbine/config.toml -p 9090:9090 \
  svssathvik7/turbine --config /etc/turbine/config.toml --port 9090 --log-level debug
```

## Monitoring

| Endpoint | Description |
|----------|-------------|
| `/dashboard` | Live web dashboard with auto-refresh |
| `/api/status` | Detailed JSON with per-endpoint telemetry |
| `/metrics` | Compact JSON with per-chain aggregate stats |

## Supported Architectures

| Architecture | Tag |
|--------------|-----|
| `linux/amd64` | `latest`, `0.3.0` |
| `linux/arm64` | `latest`, `0.3.0` |

## Configuration Reference

Full configuration reference and Rust library/builder API docs are available on [GitHub](https://github.com/svssathvik7/turbine).

## License

MIT
