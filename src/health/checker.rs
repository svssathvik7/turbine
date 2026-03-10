use super::ChainPool;
use crate::config::{default_health_method, EndpointAuth};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Spawns a background task that periodically checks endpoint health
/// by calling the configured JSON-RPC method and comparing block heights.
pub fn spawn_health_checker(
    chain_name: String,
    pool: Arc<ChainPool>,
    health_method: Option<String>,
    interval_secs: u64,
    max_block_lag: u64,
) {
    let method = health_method.unwrap_or_else(|| default_health_method(&chain_name).to_string());

    tokio::spawn(async move {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build health check client");

        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

        loop {
            interval.tick().await;

            let block_heights = fetch_block_heights(&client, &pool, &method).await;

            if block_heights.is_empty() {
                continue;
            }

            let max_height = block_heights
                .iter()
                .filter_map(|(_, h)| *h)
                .max()
                .unwrap_or(0);

            if max_height == 0 {
                continue;
            }

            for (idx, height) in &block_heights {
                match height {
                    Some(h) if max_height - h > max_block_lag => {
                        warn!(
                            chain = %chain_name,
                            endpoint = %pool.endpoints[*idx].url,
                            block_height = h,
                            max_height = max_height,
                            lag = max_height - h,
                            "Endpoint is stale, marking unhealthy"
                        );
                        pool.update_block_height(*idx, *h);
                        pool.mark_stale(*idx);
                    }
                    Some(h) => {
                        debug!(
                            chain = %chain_name,
                            endpoint = %pool.endpoints[*idx].url,
                            block_height = h,
                            "Endpoint healthy"
                        );
                        pool.update_block_height(*idx, *h);
                        // If it was previously stale but now caught up, re-enable it
                        pool.record_success(*idx);
                    }
                    None => {
                        debug!(
                            chain = %chain_name,
                            endpoint = %pool.endpoints[*idx].url,
                            "Health check failed, passive tracking will handle"
                        );
                    }
                }
            }
        }
    });
}

async fn fetch_block_heights(
    client: &Client,
    pool: &ChainPool,
    method: &str,
) -> Vec<(usize, Option<u64>)> {
    let mut results = Vec::with_capacity(pool.endpoints.len());

    for (idx, endpoint) in pool.endpoints.iter().enumerate() {
        let start = std::time::Instant::now();
        let height = fetch_block_height(client, &endpoint.url, method, endpoint.auth.as_ref()).await;
        let latency_ms = start.elapsed().as_millis() as u64;
        if height.is_some() {
            pool.update_latency(idx, latency_ms);
        }
        results.push((idx, height));
    }

    results
}

async fn fetch_block_height(client: &Client, endpoint: &str, method: &str, auth: Option<&EndpointAuth>) -> Option<u64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": [],
        "id": 1
    });

    let mut req = client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .json(&body);

    if let Some(auth) = auth {
        req = match auth {
            EndpointAuth::Basic { username, password } => req.basic_auth(username, Some(password)),
            EndpointAuth::Bearer(token) => req.bearer_auth(token),
            EndpointAuth::Header { name, value } => req.header(name, value),
        };
    }

    let response = req.send().await.ok()?;

    let json: serde_json::Value = response.json().await.ok()?;

    let result = json.get("result")?;

    // Handle hex string (EVM: "0x1a2b3c")
    if let Some(hex_str) = result.as_str() {
        let hex_str = hex_str.trim_start_matches("0x");
        return u64::from_str_radix(hex_str, 16).ok();
    }

    // Handle plain number (Solana: 123456789)
    if let Some(num) = result.as_u64() {
        return Some(num);
    }

    None
}
