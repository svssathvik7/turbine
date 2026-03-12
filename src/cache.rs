use moka::future::Cache;
use moka::Expiry;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::CacheConfig;

/// Cache key combining method name and a hash of the serialized params.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CacheKey {
    pub method: String,
    pub params_hash: u64,
}

impl CacheKey {
    pub fn new(method: &str, params: &serde_json::Value) -> Self {
        let mut hasher = DefaultHasher::new();
        let serialized = params.to_string();
        serialized.hash(&mut hasher);
        Self {
            method: method.to_string(),
            params_hash: hasher.finish(),
        }
    }
}

/// The value stored in cache — the raw response bytes and HTTP status.
#[derive(Clone, Debug)]
pub struct CachedResponse {
    pub status: u16,
    pub body: bytes::Bytes,
}

/// Chain-family presets with default method→TTL mappings.
#[derive(Debug, Clone, PartialEq)]
pub enum CachePreset {
    Evm,
    Solana,
    None,
}

impl CachePreset {
    /// Auto-detect preset from chain name, matching the pattern used in `default_health_method`.
    pub fn from_chain_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("solana") {
            CachePreset::Solana
        } else if lower.contains("ethereum")
            || lower.contains("polygon")
            || lower.contains("arbitrum")
            || lower.contains("optimism")
            || lower.contains("base")
            || lower.contains("avalanche")
            || lower.contains("bsc")
            || lower.contains("fantom")
            || lower.contains("gnosis")
            || lower.contains("zksync")
            || lower.contains("linea")
            || lower.contains("scroll")
        {
            CachePreset::Evm
        } else {
            // Default to EVM for unknown chains, since most chains are EVM-compatible.
            CachePreset::Evm
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "evm" => CachePreset::Evm,
            "solana" => CachePreset::Solana,
            "none" => CachePreset::None,
            _ => CachePreset::None,
        }
    }

    /// Returns the default method→TTL map for this preset.
    fn default_ttls(&self) -> HashMap<String, Duration> {
        let mut map = HashMap::new();
        match self {
            CachePreset::Evm => {
                map.insert("eth_chainId".to_string(), Duration::from_secs(86400));
                map.insert("net_version".to_string(), Duration::from_secs(86400));
                map.insert("eth_getBlockByNumber".to_string(), Duration::from_secs(300));
                map.insert("eth_getBlockByHash".to_string(), Duration::from_secs(300));
                map.insert(
                    "eth_getTransactionByHash".to_string(),
                    Duration::from_secs(300),
                );
                map.insert(
                    "eth_getTransactionReceipt".to_string(),
                    Duration::from_secs(300),
                );
                map.insert("eth_getCode".to_string(), Duration::from_secs(300));
            }
            CachePreset::Solana => {
                map.insert("getGenesisHash".to_string(), Duration::from_secs(86400));
                map.insert("getVersion".to_string(), Duration::from_secs(3600));
                map.insert("getBlock".to_string(), Duration::from_secs(300));
                map.insert("getTransaction".to_string(), Duration::from_secs(300));
            }
            CachePreset::None => {}
        }
        map
    }
}

/// Per-entry expiry that looks up TTL from a method→Duration map.
struct MethodExpiry {
    ttls: Arc<HashMap<String, Duration>>,
}

impl Expiry<CacheKey, CachedResponse> for MethodExpiry {
    fn expire_after_create(
        &self,
        key: &CacheKey,
        _value: &CachedResponse,
        _created_at: Instant,
    ) -> Option<Duration> {
        self.ttls.get(&key.method).copied()
    }
}

/// Per-chain cache wrapping a moka async cache.
pub struct ChainCache {
    cache: Cache<CacheKey, CachedResponse>,
    ttls: Arc<HashMap<String, Duration>>,
}

impl ChainCache {
    /// Build a new cache from config.
    ///
    /// - Starts with preset defaults
    /// - Applies user overrides from `config.methods`
    pub fn new(config: &CacheConfig, chain_name: &str) -> Self {
        let preset = match &config.preset {
            Some(p) => CachePreset::from_str(p),
            None => CachePreset::from_chain_name(chain_name),
        };

        let mut ttls = preset.default_ttls();

        // Apply user overrides
        for method_cfg in &config.methods {
            ttls.insert(
                method_cfg.name.clone(),
                Duration::from_secs(method_cfg.ttl_seconds),
            );
        }

        let ttls = Arc::new(ttls);
        let expiry = MethodExpiry {
            ttls: Arc::clone(&ttls),
        };

        let cache = Cache::builder()
            .max_capacity(config.max_capacity.unwrap_or(10_000))
            .expire_after(expiry)
            .build();

        Self { cache, ttls }
    }

    /// Check if a method is cacheable (has a TTL entry).
    pub fn is_cacheable(&self, method: &str) -> bool {
        self.ttls.contains_key(method)
    }

    /// Try to get a cached response.
    pub async fn get(&self, key: &CacheKey) -> Option<CachedResponse> {
        self.cache.get(key).await
    }

    /// Insert a response into the cache (only if the method is cacheable).
    pub async fn insert(&self, key: CacheKey, response: CachedResponse) {
        if self.is_cacheable(&key.method) {
            self.cache.insert(key, response).await;
        }
    }
}
