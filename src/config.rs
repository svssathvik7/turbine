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
    pub endpoints: Vec<String>,
    pub health: HealthConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthConfig {
    pub max_consecutive_failures: u32,
    pub cooldown_seconds: u64,
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
        }
        Ok(())
    }
}
