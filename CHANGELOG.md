# Changelog

All notable changes to Turbine are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/).

## [0.9.0] - 2026-03-13

### Added

- **WebSocket proxy support**: clients connect via WS to any chain route (`GET /{chain}`), Turbine relays to upstream WSS endpoints
- Auto-derives WSS URL from HTTPS endpoint URL (`https://` → `wss://`); explicit `ws_url` override available
- 1:1 client-to-upstream connection with bidirectional frame relay
- Automatic reconnection to alternate endpoint on upstream failure with `turbine_reconnected` JSON-RPC notification
- Auth injection (Basic, Bearer, Header) on upstream WS upgrade request
- WS metrics: `ws_connections_total`, `ws_active_connections`, `ws_messages_relayed`, `ws_reconnections`
- Builder API: `ChainBuilder::endpoint_with_ws(url, ws_url)`
- Dashboard: WS metrics displayed per chain (conditionally, when WS is active)

## [0.8.0] - 2026-03-13

### Added

- **Latency-based rotation strategy**: `rotation = "latency"` routes more traffic to faster endpoints
- Uses inverse-latency weighting combined with user-defined endpoint weights
- Cold start: falls back to weight-only rotation until latency data exists
- Builder API: `ChainBuilder::latency_based()`

## [0.7.0] - 2026-03-13

### Added

- **Method-based endpoint routing**: endpoints can declare a `methods` allowlist to receive only specific RPC methods
- Route `eth_sendRawTransaction` to a private mempool RPC while other methods go to the general pool
- Endpoints without a `methods` field handle all methods not claimed by another endpoint
- Batch requests with mixed methods are split into routing groups and forwarded independently
- Returns 503 with `"No endpoints configured for method: <method>"` if all eligible endpoints are unhealthy
- Builder API: `ChainBuilder::restricted_endpoint(url, &["method1", "method2"])`

## [0.5.0] - 2026-03-12

### Added

- Hedged requests: fire parallel requests after configurable delay to reduce tail latency
- New `[chains.hedge]` config section with `delay_ms` and `max_count`
- Builder API method: `.hedge(delay_ms, max_count)`
- `hedged_requests` counter in metrics and status API

## [0.4.0] - 2026-03-12

### Added

- Per-chain rate limiting with configurable `max_requests` and `window_seconds`
- Chain ID routing: numeric paths like `/1` or `/8453` resolve to chains by `chain_id`
- Configurable retries: `max_retries` and `retry_delay_ms` in `[chains.health]`
- Builder API methods: `.chain_id()`, `.rate_limit()`, `.max_retries()`, `.retry_delay_ms()`
- Rate-limited request counter in metrics and status API
- `chain_id` field in status API response

## [0.3.0] - 2026-03-10

### Added

- Live dashboard at `/dashboard` with auto-refresh every 5 seconds
- Per-chain cards with request stats and success rate visualization
- Per-endpoint details: health status, latency (rolling average), block height, weight
- Color-coded health indicators (green/yellow/red)
- Mobile responsive layout

## [0.2.1] - 2026-03-10

### Fixed

- Bitcoin default health check method now correctly uses `getblockcount`

## [0.2.0] - 2026-03-10

### Added

- Per-endpoint upstream authentication (Basic Auth, Bearer token, custom header)
- In-process response caching with per-method TTL
- EVM and Solana cache presets with sensible defaults
- Chain-aware active health checks with block height comparison
- Weighted rotation strategy
- Batch JSON-RPC request support with per-request metrics
- Library crate with programmatic builder API
- `Turbine::into_router()` for embedding in existing axum apps

### Changed

- Reorganized flat file structure into `proxy/` and `health/` modules
- Refactored into publishable Rust crate (`lib.rs` + `main.rs`)
