use serde::de::{self, Deserializer};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub chains: Vec<ChainConfig>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct ChainConfig {
    pub name: String,
    pub route: String,
    pub endpoints: Vec<EndpointConfig>,
    pub health: HealthConfig,
    #[serde(default)]
    pub rotation: RotationStrategy,
    #[serde(default)]
    pub cache: Option<CacheConfig>,
    #[serde(default)]
    pub chain_id: Option<u64>,
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default)]
    pub hedge: Option<HedgeConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum EndpointRaw {
    Simple(String),
    Full {
        url: String,
        #[serde(default = "default_weight")]
        weight: u32,
        #[serde(default)]
        auth: Option<EndpointAuth>,
    },
}

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub url: String,
    pub weight: u32,
    pub auth: Option<EndpointAuth>,
}

/// Authentication credentials for an upstream RPC endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointAuth {
    /// HTTP Basic Auth (username + password). Used by Bitcoin Core and self-hosted nodes.
    Basic { username: String, password: String },
    /// Bearer token sent via `Authorization: Bearer <token>` header.
    Bearer(String),
    /// Custom header (e.g., `x-api-key`).
    Header { name: String, value: String },
}

fn default_weight() -> u32 {
    1
}

impl<'de> Deserialize<'de> for EndpointConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = EndpointRaw::deserialize(deserializer).map_err(de::Error::custom)?;
        Ok(match raw {
            EndpointRaw::Simple(url) => EndpointConfig {
                url,
                weight: default_weight(),
                auth: None,
            },
            EndpointRaw::Full { url, weight, auth } => EndpointConfig { url, weight, auth },
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthConfig {
    pub max_consecutive_failures: u32,
    pub cooldown_seconds: u64,
    #[serde(default)]
    pub health_method: Option<String>,
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_seconds: u64,
    #[serde(default = "default_max_block_lag")]
    pub max_block_lag: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default)]
    pub retry_delay_ms: u64,
}

fn default_health_check_interval() -> u64 {
    30
}

fn default_max_block_lag() -> u64 {
    10
}

fn default_max_retries() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    #[default]
    RoundRobin,
    Weighted,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub max_capacity: Option<u64>,
    #[serde(default)]
    pub methods: Vec<CacheMethodConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheMethodConfig {
    pub name: String,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub max_requests: u32,
    pub window_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HedgeConfig {
    pub delay_ms: u64,
    #[serde(default = "default_hedge_max_count")]
    pub max_count: u32,
}

fn default_hedge_max_count() -> u32 {
    1
}

/// Default health check methods per chain family.
pub fn default_health_method(chain_name: &str) -> &'static str {
    let name = chain_name.to_lowercase();
    if name.contains("solana") {
        "getSlot"
    } else if name.contains("starknet") {
        "starknet_blockNumber"
    } else if name.contains("bitcoin") || name.contains("btc") {
        "getblockcount"
    } else if name.contains("aptos") || name.contains("sui") {
        // Aptos/Sui use different APIs, but for JSON-RPC based:
        "eth_blockNumber"
    } else {
        // Default to EVM
        "eth_blockNumber"
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.chains.is_empty() {
            return Err("At least one chain must be configured".into());
        }

        let mut chain_ids = std::collections::HashSet::new();
        for chain in &self.chains {
            if let Some(id) = chain.chain_id {
                if !chain_ids.insert(id) {
                    return Err(format!("Duplicate chain_id {} in config", id).into());
                }
            }
        }

        for chain in &self.chains {
            if chain.endpoints.is_empty() {
                return Err(format!("Chain '{}' has no endpoints configured", chain.name).into());
            }
            if !chain.route.starts_with('/') {
                return Err(format!("Chain '{}' route must start with '/'", chain.name).into());
            }
            if chain.rotation == RotationStrategy::Weighted
                && chain.endpoints.iter().all(|e| e.weight == 0)
            {
                return Err(format!(
                    "Chain '{}' uses weighted rotation but all weights are 0",
                    chain.name
                )
                .into());
            }
            if let Some(ref hedge) = chain.hedge {
                if hedge.delay_ms == 0 {
                    return Err(format!("Chain '{}' hedge.delay_ms must be > 0", chain.name).into());
                }
                if hedge.max_count == 0 {
                    return Err(
                        format!("Chain '{}' hedge.max_count must be > 0", chain.name).into(),
                    );
                }
                if chain.endpoints.len() < 2 {
                    return Err(format!(
                        "Chain '{}' uses hedging but has fewer than 2 endpoints",
                        chain.name
                    )
                    .into());
                }
            }
            if let Some(ref rl) = chain.rate_limit {
                if rl.max_requests == 0 {
                    return Err(format!(
                        "Chain '{}' rate_limit.max_requests must be > 0",
                        chain.name
                    )
                    .into());
                }
                if rl.window_seconds == 0 {
                    return Err(format!(
                        "Chain '{}' rate_limit.window_seconds must be > 0",
                        chain.name
                    )
                    .into());
                }
            }
        }
        Ok(())
    }
}
