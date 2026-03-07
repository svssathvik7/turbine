# Turbine

Multi-chain RPC proxy with intelligent endpoint rotation. Unlike EVM-only proxies, Turbine works with any blockchain that speaks JSON-RPC over HTTP.

## Features

- **Multi-chain** — configure any number of chains, each with its own endpoint pool
- **Round-robin rotation** — distributes requests evenly across endpoints
- **Passive health tracking** — automatically detects and skips failing endpoints
- **Auto-retry** — retries with a different endpoint on failure
- **Metrics** — per-chain request/failure stats via `/metrics`

## Quick Start

```bash
# Build
cargo build --release

# Run with default config
./target/release/turbine --config config.toml

# Or with overrides
./target/release/turbine --config config.toml --port 9090 --log-level debug
```

## Configuration

See [config.toml](config.toml) for a sample configuration.

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
]

[chains.health]
max_consecutive_failures = 3
cooldown_seconds = 30
```

## Usage

Send JSON-RPC requests to `http://localhost:8080/<chain>`:

```bash
# Ethereum
curl -X POST http://localhost:8080/ethereum \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Solana
curl -X POST http://localhost:8080/solana \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getSlot","params":[],"id":1}'

# Metrics
curl http://localhost:8080/metrics
```
