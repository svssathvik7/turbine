use super::ChainPool;
use crate::config::{default_health_method, EndpointAuth};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Result of a single health check request.
enum HealthCheckResult {
    /// Successful — got a block height.
    Ok(u64),
    /// Upstream returned 429 — rate limited, not a failure.
    Throttled,
    /// Request failed (timeout, connection error, bad response, etc.)
    Failed,
}

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
            .pool_max_idle_per_host(0)
            .pool_idle_timeout(Duration::from_secs(1))
            .build()
            .expect("Failed to build health check client");

        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

        loop {
            interval.tick().await;

            let results = fetch_block_heights(&client, &pool, &method).await;

            if results.is_empty() {
                continue;
            }

            let max_height = results
                .iter()
                .filter_map(|(_, r)| match r {
                    HealthCheckResult::Ok(h) => Some(*h),
                    _ => None,
                })
                .max()
                .unwrap_or(0);

            if max_height == 0 {
                continue;
            }

            for (idx, result) in &results {
                match result {
                    HealthCheckResult::Ok(h) if max_height - h > max_block_lag => {
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
                    HealthCheckResult::Ok(h) => {
                        pool.update_block_height(*idx, *h);
                        // Re-enable if previously stale/unhealthy
                        if !pool.health[*idx].is_healthy() {
                            info!(
                                chain = %chain_name,
                                endpoint = %pool.endpoints[*idx].url,
                                block_height = h,
                                "Endpoint recovered, marking healthy"
                            );
                        }
                        pool.record_success(*idx);
                    }
                    HealthCheckResult::Throttled => {
                        // Endpoint is alive but rate-limiting us — record throttle
                        // (does not increment consecutive failures)
                        debug!(
                            chain = %chain_name,
                            endpoint = %pool.endpoints[*idx].url,
                            "Health check throttled (429), endpoint is alive but rate-limiting"
                        );
                        pool.record_throttle(*idx);
                    }
                    HealthCheckResult::Failed => {
                        debug!(
                            chain = %chain_name,
                            endpoint = %pool.endpoints[*idx].url,
                            "Health check failed"
                        );
                        // Don't record_failure here — passive request tracking handles it.
                        // The health checker's job is to detect stale blocks and recover
                        // endpoints, not to pile on failures during transient issues.
                    }
                }
            }

            // --- Demotion evaluation ---
            const THROTTLE_DEMOTION_THRESHOLD: u64 = 3;
            let mut to_demote: Vec<usize> = Vec::new();

            for (idx, result) in &results {
                let should_demote = match result {
                    HealthCheckResult::Ok(h) if max_height > 0 && max_height - h > max_block_lag => {
                        true // Already marked stale above, also demote
                    }
                    HealthCheckResult::Failed => {
                        // Check if consecutive failures hit threshold
                        pool.health[*idx].consecutive_failures.load(std::sync::atomic::Ordering::Relaxed)
                            >= pool.health_config.max_consecutive_failures
                    }
                    HealthCheckResult::Throttled => {
                        // Check chronic throttling
                        pool.health[*idx].throttle_count.load(std::sync::atomic::Ordering::Relaxed)
                            >= THROTTLE_DEMOTION_THRESHOLD
                    }
                    _ => false,
                };
                if should_demote {
                    to_demote.push(*idx);
                }
            }

            for idx in to_demote {
                warn!(
                    chain = %chain_name,
                    endpoint = %pool.endpoints[idx].url,
                    "Demoting endpoint to rpc_house"
                );
                pool.demote_and_replace(idx);
            }
        }
    });
}

async fn fetch_block_heights(
    client: &Client,
    pool: &ChainPool,
    method: &str,
) -> Vec<(usize, HealthCheckResult)> {
    let active = pool.active_indices.read().unwrap().clone();
    let mut results = Vec::with_capacity(active.len());

    for &idx in &active {
        let endpoint = &pool.endpoints[idx];
        let start = std::time::Instant::now();
        let result = fetch_block_height(client, &endpoint.url, method, endpoint.auth.as_ref()).await;
        let latency_ms = start.elapsed().as_millis() as u64;

        if let HealthCheckResult::Ok(_) = &result {
            pool.update_latency(idx, latency_ms);
        }
        results.push((idx, result));
    }

    results
}

async fn fetch_block_height(
    client: &Client,
    endpoint: &str,
    method: &str,
    auth: Option<&EndpointAuth>,
) -> HealthCheckResult {
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

    let response = match req.send().await {
        Ok(r) => r,
        Err(_) => return HealthCheckResult::Failed,
    };

    // Detect upstream rate limiting
    if response.status().as_u16() == 429 {
        return HealthCheckResult::Throttled;
    }

    if !response.status().is_success() {
        return HealthCheckResult::Failed;
    }

    let json: serde_json::Value = match response.json().await {
        Ok(j) => j,
        Err(_) => return HealthCheckResult::Failed,
    };

    let result = match json.get("result") {
        Some(r) => r,
        None => return HealthCheckResult::Failed,
    };

    // Handle hex string (EVM: "0x1a2b3c")
    if let Some(hex_str) = result.as_str() {
        let hex_str = hex_str.trim_start_matches("0x");
        return match u64::from_str_radix(hex_str, 16) {
            Ok(h) => HealthCheckResult::Ok(h),
            Err(_) => HealthCheckResult::Failed,
        };
    }

    // Handle plain number (Solana: 123456789)
    if let Some(num) = result.as_u64() {
        return HealthCheckResult::Ok(num);
    }

    HealthCheckResult::Failed
}
