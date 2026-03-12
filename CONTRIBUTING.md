# Contributing to Turbine

Thanks for your interest in contributing! Here's how to get started.

## Getting Started

```bash
git clone https://github.com/svssathvik7/turbine.git
cd turbine
cargo build
cargo run -- --config config.toml
```

The proxy starts on `http://127.0.0.1:8080`. Open `/dashboard` in your browser to verify it's running.

## Development

Format and lint before committing:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

There are no tests yet — contributions adding test coverage are very welcome!

## Submitting Changes

1. Fork the repo and create a branch (`feat/my-feature`, `fix/my-bug`)
2. Make your changes
3. Ensure `cargo fmt` and `cargo clippy` pass cleanly
4. Test manually against a config file
5. Open a PR against `main`

CI will run formatting, linting, and build checks automatically.

## Architecture Overview

```
src/
├── main.rs             CLI entry point (clap)
├── lib.rs              Public Turbine struct and builder API
├── config.rs           TOML config parsing and validation
├── types.rs            JSON-RPC request/response types
├── metrics.rs          Lock-free atomic counters per chain
├── cache.rs            Moka cache with TTL presets (EVM, Solana)
├── dashboard.rs        Self-contained HTML dashboard
├── proxy/
│   ├── mod.rs          AppState and ChainState structs
│   ├── server.rs       Axum router setup and handler wiring
│   ├── handler.rs      Request parsing, cache lookup, retry logic
│   └── forwarder.rs    HTTP forwarding with auth injection
└── health/
    ├── mod.rs          Module exports
    ├── pool.rs         Endpoint pool with rotation strategies
    ├── state.rs        Per-endpoint health and telemetry tracking
    └── checker.rs      Background health check task
```

**Request flow:** `handler.rs` receives a request, resolves the chain from the route, checks the cache, picks a healthy endpoint from `pool.rs`, forwards via `forwarder.rs` (injecting auth), and retries once on failure with a different endpoint.
