pub mod config;
pub mod forwarder;
pub mod health;
pub mod metrics;
pub mod pool;
pub mod router;
pub mod server;
pub mod types;

use config::{ChainConfig, Config, HealthConfig, ServerConfig};
use server::build_router;
use std::path::Path;

pub struct Turbine {
    config: Config,
}

pub struct TurbineBuilder {
    chains: Vec<ChainConfig>,
}

pub struct ChainBuilder {
    name: String,
    route: String,
    endpoints: Vec<String>,
    max_consecutive_failures: u32,
    cooldown_seconds: u64,
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
        TurbineBuilder { chains: Vec::new() }
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
            parent: self,
        }
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
            },
            chains: self.chains,
        };
        Ok(Turbine { config })
    }
}

impl ChainBuilder {
    /// Add an RPC endpoint.
    pub fn endpoint(mut self, url: &str) -> Self {
        self.endpoints.push(url.to_string());
        self
    }

    /// Add multiple RPC endpoints.
    pub fn endpoints(mut self, urls: &[&str]) -> Self {
        self.endpoints.extend(urls.iter().map(|s| s.to_string()));
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

    /// Finish configuring this chain and return to the builder.
    pub fn done(self) -> TurbineBuilder {
        let chain = ChainConfig {
            name: self.name,
            route: self.route,
            endpoints: self.endpoints,
            health: HealthConfig {
                max_consecutive_failures: self.max_consecutive_failures,
                cooldown_seconds: self.cooldown_seconds,
            },
        };
        let mut parent = self.parent;
        parent.chains.push(chain);
        parent
    }
}
