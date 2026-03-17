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
    #[serde(default)]
    pub dashboard_secret: Option<String>,
    #[serde(default)]
    pub api_keys: Vec<ApiKeyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeyConfig {
    pub name: String,
    pub key: String,
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
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
        #[serde(default)]
        methods: Option<Vec<String>>,
        #[serde(default)]
        ws_url: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub url: String,
    pub weight: u32,
    pub auth: Option<EndpointAuth>,
    pub methods: Option<Vec<String>>,
    pub ws_url: Option<String>,
}

impl EndpointConfig {
    pub fn effective_ws_url(&self) -> Option<String> {
        if let Some(ref ws) = self.ws_url {
            return Some(ws.clone());
        }
        if self.url.starts_with("https://") {
            Some(self.url.replacen("https://", "wss://", 1))
        } else if self.url.starts_with("http://") {
            Some(self.url.replacen("http://", "ws://", 1))
        } else {
            None
        }
    }
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
                methods: None,
                ws_url: None,
            },
            EndpointRaw::Full {
                url,
                weight,
                auth,
                methods,
                ws_url,
            } => EndpointConfig {
                url,
                weight,
                auth,
                methods,
                ws_url,
            },
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
    Latency,
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
        for chain in &self.chains {
            for ep in &chain.endpoints {
                if let Some(ref methods) = ep.methods {
                    if methods.is_empty() {
                        eprintln!(
                            "Warning: endpoint '{}' in chain '{}' has an empty methods list and will never receive any requests",
                            ep.url, chain.name
                        );
                    }
                }
            }
        }

        let mut seen_keys: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for ak in &self.server.api_keys {
            if ak.key.trim().is_empty() {
                return Err(format!("api_key '{}' has an empty key string", ak.name).into());
            }
            if !seen_keys.insert(&ak.key) {
                return Err(format!("Duplicate api key value for name '{}'", ak.name).into());
            }
            if let Some(ref rl) = ak.rate_limit {
                if rl.max_requests == 0 {
                    return Err(format!(
                        "api_key '{}' rate_limit.max_requests must be > 0",
                        ak.name
                    )
                    .into());
                }
                if rl.window_seconds == 0 {
                    return Err(format!(
                        "api_key '{}' rate_limit.window_seconds must be > 0",
                        ak.name
                    )
                    .into());
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_without_methods_defaults_to_none() {
        let toml = r#"url = "https://rpc.example.com""#;
        let ep: EndpointConfig = toml::from_str(toml).unwrap();
        assert!(ep.methods.is_none());
    }

    #[test]
    fn endpoint_with_methods_parses_correctly() {
        let toml = r#"
            url = "https://private-rpc.example.com"
            methods = ["eth_sendRawTransaction", "eth_sendTransaction"]
        "#;
        let ep: EndpointConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            ep.methods.unwrap(),
            vec!["eth_sendRawTransaction", "eth_sendTransaction"]
        );
    }

    #[test]
    fn latency_rotation_strategy_parses() {
        let toml = r#"
            name = "ethereum"
            route = "/ethereum"
            rotation = "latency"
            endpoints = ["https://rpc.example.com"]

            [health]
            max_consecutive_failures = 3
            cooldown_seconds = 30
        "#;
        let chain: ChainConfig = toml::from_str(toml).unwrap();
        assert_eq!(chain.rotation, RotationStrategy::Latency);
    }

    #[test]
    fn simple_string_endpoint_has_no_methods() {
        let toml = r#"
            name = "ethereum"
            route = "/ethereum"
            endpoints = ["https://rpc.example.com"]

            [health]
            max_consecutive_failures = 3
            cooldown_seconds = 30
        "#;
        let chain: ChainConfig = toml::from_str(toml).unwrap();
        assert!(chain.endpoints[0].methods.is_none());
    }

    #[test]
    fn effective_ws_url_explicit() {
        let ep = EndpointConfig {
            url: "https://rpc.example.com".into(),
            weight: 1,
            auth: None,
            methods: None,
            ws_url: Some("wss://ws.example.com".into()),
        };
        assert_eq!(ep.effective_ws_url(), Some("wss://ws.example.com".into()));
    }

    #[test]
    fn effective_ws_url_derived_https() {
        let ep = EndpointConfig {
            url: "https://rpc.example.com".into(),
            weight: 1,
            auth: None,
            methods: None,
            ws_url: None,
        };
        assert_eq!(ep.effective_ws_url(), Some("wss://rpc.example.com".into()));
    }

    #[test]
    fn effective_ws_url_derived_http() {
        let ep = EndpointConfig {
            url: "http://localhost:8545".into(),
            weight: 1,
            auth: None,
            methods: None,
            ws_url: None,
        };
        assert_eq!(ep.effective_ws_url(), Some("ws://localhost:8545".into()));
    }

    #[test]
    fn ws_url_config_field_parses() {
        let toml = r#"
            url = "https://rpc.example.com"
            ws_url = "wss://ws.example.com"
        "#;
        let ep: EndpointConfig = toml::from_str(toml).unwrap();
        assert_eq!(ep.ws_url, Some("wss://ws.example.com".into()));
    }

    #[test]
    fn ws_url_defaults_to_none() {
        let toml = r#"url = "https://rpc.example.com""#;
        let ep: EndpointConfig = toml::from_str(toml).unwrap();
        assert!(ep.ws_url.is_none());
    }
}
