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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum EndpointRaw {
    Simple(String),
    Full { url: String, #[serde(default = "default_weight")] weight: u32 },
}

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub url: String,
    pub weight: u32,
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
            EndpointRaw::Simple(url) => EndpointConfig { url, weight: default_weight() },
            EndpointRaw::Full { url, weight } => EndpointConfig { url, weight },
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
}

fn default_health_check_interval() -> u64 {
    30
}

fn default_max_block_lag() -> u64 {
    10
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    #[default]
    RoundRobin,
    Weighted,
}

/// Default health check methods per chain family.
pub fn default_health_method(chain_name: &str) -> &'static str {
    let name = chain_name.to_lowercase();
    if name.contains("solana") {
        "getSlot"
    } else if name.contains("starknet") {
        "starknet_blockNumber"
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
        for chain in &self.chains {
            if chain.endpoints.is_empty() {
                return Err(format!("Chain '{}' has no endpoints configured", chain.name).into());
            }
            if !chain.route.starts_with('/') {
                return Err(
                    format!("Chain '{}' route must start with '/'", chain.name).into(),
                );
            }
            if chain.rotation == RotationStrategy::Weighted
                && chain.endpoints.iter().all(|e| e.weight == 0)
            {
                return Err(
                    format!("Chain '{}' uses weighted rotation but all weights are 0", chain.name)
                        .into(),
                );
            }
        }
        Ok(())
    }
}
