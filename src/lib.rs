pub mod cache;
pub mod config;
pub mod dashboard;
pub mod health;
pub mod metrics;
pub mod proxy;
pub mod types;

use config::{
    CacheConfig, CacheMethodConfig, ChainConfig, Config, EndpointAuth, EndpointConfig,
    HealthConfig, HedgeConfig, RateLimitConfig, RotationStrategy, ServerConfig,
};
use proxy::build_router;
use std::path::Path;

pub struct Turbine {
    config: Config,
}

pub struct TurbineBuilder {
    chains: Vec<ChainConfig>,
    dashboard_secret: Option<String>,
}

pub struct ChainBuilder {
    name: String,
    route: String,
    endpoints: Vec<EndpointConfig>,
    max_consecutive_failures: u32,
    cooldown_seconds: u64,
    health_method: Option<String>,
    health_check_interval_seconds: u64,
    max_block_lag: u64,
    rotation: RotationStrategy,
    cache_enabled: bool,
    cache_preset: Option<String>,
    cache_max_capacity: Option<u64>,
    cache_methods: Vec<CacheMethodConfig>,
    chain_id: Option<u64>,
    max_retries: u32,
    retry_delay_ms: u64,
    rate_limit_max_requests: Option<u32>,
    rate_limit_window_seconds: Option<u64>,
    hedge_delay_ms: Option<u64>,
    hedge_max_count: Option<u32>,
    parent: TurbineBuilder,
}

impl Turbine {
    /// Create a Turbine instance from a TOML config file.
    pub fn from_config(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config = Config::load(path)?;
        Ok(Self { config })
    }

    /// Create a Turbine instance from an existing Config.
    pub fn from_raw_config(config: Config) -> Self {
        Self { config }
    }

    /// Start building a Turbine instance programmatically.
    pub fn builder() -> TurbineBuilder {
        TurbineBuilder {
            chains: Vec::new(),
            dashboard_secret: None,
        }
    }

    /// Get the configured host.
    pub fn host(&self) -> &str {
        &self.config.server.host
    }

    /// Get the configured port.
    pub fn port(&self) -> u16 {
        self.config.server.port
    }

    /// Get an axum Router to embed in your own server.
    pub fn into_router(self) -> axum::Router {
        build_router(&self.config)
    }

    /// Run the proxy as a standalone server.
    pub async fn serve(self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let router = build_router(&self.config);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Turbine starting on {}", addr);
        axum::serve(listener, router).await?;
        Ok(())
    }
}

impl TurbineBuilder {
    /// Add a chain to the proxy.
    pub fn add_chain(self, name: &str) -> ChainBuilder {
        ChainBuilder {
            name: name.to_string(),
            route: format!("/{}", name),
            endpoints: Vec::new(),
            max_consecutive_failures: 3,
            cooldown_seconds: 30,
            health_method: None,
            health_check_interval_seconds: 30,
            max_block_lag: 10,
            rotation: RotationStrategy::RoundRobin,
            cache_enabled: false,
            cache_preset: None,
            cache_max_capacity: None,
            cache_methods: Vec::new(),
            chain_id: None,
            max_retries: 1,
            retry_delay_ms: 0,
            rate_limit_max_requests: None,
            rate_limit_window_seconds: None,
            hedge_delay_ms: None,
            hedge_max_count: None,
            parent: self,
        }
    }

    /// Set a secret path for the dashboard (e.g., `"my-secret"` → `/{my-secret}`).
    pub fn dashboard_secret(mut self, secret: &str) -> Self {
        self.dashboard_secret = Some(secret.to_string());
        self
    }

    /// Build the Turbine instance.
    pub fn build(self) -> Result<Turbine, Box<dyn std::error::Error>> {
        if self.chains.is_empty() {
            return Err("At least one chain must be configured".into());
        }
        let config = Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                dashboard_secret: self.dashboard_secret,
            },
            chains: self.chains,
        };
        Ok(Turbine { config })
    }
}

impl ChainBuilder {
    /// Add an RPC endpoint with default weight (1) and no auth.
    pub fn endpoint(mut self, url: &str) -> Self {
        self.endpoints.push(EndpointConfig {
            url: url.to_string(),
            weight: 1,
            auth: None,
            methods: None,
        });
        self
    }

    /// Add an RPC endpoint with a specific weight and no auth.
    pub fn weighted_endpoint(mut self, url: &str, weight: u32) -> Self {
        self.endpoints.push(EndpointConfig {
            url: url.to_string(),
            weight,
            auth: None,
            methods: None,
        });
        self
    }

    /// Add an RPC endpoint with HTTP Basic Auth (e.g., Bitcoin Core nodes).
    pub fn endpoint_with_basic_auth(mut self, url: &str, username: &str, password: &str) -> Self {
        self.endpoints.push(EndpointConfig {
            url: url.to_string(),
            weight: 1,
            auth: Some(EndpointAuth::Basic {
                username: username.to_string(),
                password: password.to_string(),
            }),
            methods: None,
        });
        self
    }

    /// Add an RPC endpoint with a Bearer token.
    pub fn endpoint_with_bearer(mut self, url: &str, token: &str) -> Self {
        self.endpoints.push(EndpointConfig {
            url: url.to_string(),
            weight: 1,
            auth: Some(EndpointAuth::Bearer(token.to_string())),
            methods: None,
        });
        self
    }

    /// Add an RPC endpoint with a custom auth header (e.g., `x-api-key`).
    pub fn endpoint_with_header(
        mut self,
        url: &str,
        header_name: &str,
        header_value: &str,
    ) -> Self {
        self.endpoints.push(EndpointConfig {
            url: url.to_string(),
            weight: 1,
            auth: Some(EndpointAuth::Header {
                name: header_name.to_string(),
                value: header_value.to_string(),
            }),
            methods: None,
        });
        self
    }

    /// Set a custom route (default: `/{name}`).
    pub fn route(mut self, route: &str) -> Self {
        self.route = route.to_string();
        self
    }

    /// Set max consecutive failures before marking unhealthy (default: 3).
    pub fn max_failures(mut self, n: u32) -> Self {
        self.max_consecutive_failures = n;
        self
    }

    /// Set cooldown in seconds before retrying an unhealthy endpoint (default: 30).
    pub fn cooldown_secs(mut self, secs: u64) -> Self {
        self.cooldown_seconds = secs;
        self
    }

    /// Set the JSON-RPC method used for health checks (e.g., "eth_blockNumber", "getSlot").
    pub fn health_method(mut self, method: &str) -> Self {
        self.health_method = Some(method.to_string());
        self
    }

    /// Set the health check interval in seconds (default: 30).
    pub fn health_check_interval(mut self, secs: u64) -> Self {
        self.health_check_interval_seconds = secs;
        self
    }

    /// Set the max block lag before marking an endpoint as stale (default: 10).
    pub fn max_block_lag(mut self, lag: u64) -> Self {
        self.max_block_lag = lag;
        self
    }

    /// Set rotation strategy to weighted.
    pub fn weighted(mut self) -> Self {
        self.rotation = RotationStrategy::Weighted;
        self
    }

    /// Enable or disable caching for this chain.
    pub fn cache(mut self, enabled: bool) -> Self {
        self.cache_enabled = enabled;
        self
    }

    /// Set the cache preset ("evm", "solana", "none").
    pub fn cache_preset(mut self, preset: &str) -> Self {
        self.cache_preset = Some(preset.to_string());
        self
    }

    /// Set the max number of cache entries.
    pub fn cache_max_capacity(mut self, capacity: u64) -> Self {
        self.cache_max_capacity = Some(capacity);
        self
    }

    /// Add or override a cached method with a specific TTL in seconds.
    pub fn cache_method(mut self, method: &str, ttl_seconds: u64) -> Self {
        self.cache_methods.push(CacheMethodConfig {
            name: method.to_string(),
            ttl_seconds,
        });
        self
    }

    pub fn chain_id(mut self, id: u64) -> Self {
        self.chain_id = Some(id);
        self
    }

    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    pub fn retry_delay_ms(mut self, ms: u64) -> Self {
        self.retry_delay_ms = ms;
        self
    }

    pub fn rate_limit(mut self, max_requests: u32, window_seconds: u64) -> Self {
        self.rate_limit_max_requests = Some(max_requests);
        self.rate_limit_window_seconds = Some(window_seconds);
        self
    }

    /// Enable hedged requests. After `delay_ms`, fire up to `max_count` additional
    /// parallel requests to different endpoints. First success wins.
    pub fn hedge(mut self, delay_ms: u64, max_count: u32) -> Self {
        self.hedge_delay_ms = Some(delay_ms);
        self.hedge_max_count = Some(max_count);
        self
    }

    /// Finish configuring this chain and return to the builder.
    pub fn done(self) -> TurbineBuilder {
        let cache = if self.cache_enabled {
            Some(CacheConfig {
                enabled: true,
                preset: self.cache_preset,
                max_capacity: self.cache_max_capacity,
                methods: self.cache_methods,
            })
        } else {
            None
        };

        let rate_limit = match (self.rate_limit_max_requests, self.rate_limit_window_seconds) {
            (Some(max_requests), Some(window_seconds)) => Some(RateLimitConfig {
                max_requests,
                window_seconds,
            }),
            _ => None,
        };

        let hedge = self.hedge_delay_ms.map(|delay_ms| HedgeConfig {
            delay_ms,
            max_count: self.hedge_max_count.unwrap_or(1),
        });

        let chain = ChainConfig {
            name: self.name,
            route: self.route,
            endpoints: self.endpoints,
            health: HealthConfig {
                max_consecutive_failures: self.max_consecutive_failures,
                cooldown_seconds: self.cooldown_seconds,
                health_method: self.health_method,
                health_check_interval_seconds: self.health_check_interval_seconds,
                max_block_lag: self.max_block_lag,
                max_retries: self.max_retries,
                retry_delay_ms: self.retry_delay_ms,
            },
            rotation: self.rotation,
            cache,
            chain_id: self.chain_id,
            rate_limit,
            hedge,
        };
        let mut parent = self.parent;
        parent.chains.push(chain);
        parent
    }
}
