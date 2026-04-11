use super::{AppState, ChainState};
use crate::cache::{CacheKey, CachedResponse, ChainCache};
use crate::config::HedgeConfig;
use crate::types::{JsonRpcRequest, JsonRpcResponse};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, warn};

pub async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    Path(chain): Path<String>,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let chain_key = if state.chains.contains_key(&chain) {
        chain.clone()
    } else if let Ok(id) = chain.parse::<u64>() {
        match state.chain_id_map.get(&id) {
            Some(key) => key.clone(),
            None => {
                let resp = JsonRpcResponse::proxy_error(format!("Unknown chain: {}", chain));
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::to_value(resp).unwrap()),
                );
            }
        }
    } else {
        let resp = JsonRpcResponse::proxy_error(format!("Unknown chain: {}", chain));
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::to_value(resp).unwrap()),
        );
    };

    let chain_state = state.chains.get(&chain_key).unwrap();

    // Parse body to detect batch vs single and count requests
    let parsed: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            let resp = JsonRpcResponse::proxy_error("Invalid JSON body".to_string());
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::to_value(resp).unwrap()),
            );
        }
    };

    let is_batch = parsed.is_array();

    let request_count = match &parsed {
        serde_json::Value::Array(batch) => {
            if batch.is_empty() {
                let resp = JsonRpcResponse::proxy_error("Empty batch request".to_string());
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::to_value(resp).unwrap()),
                );
            }
            batch.len() as u64
        }
        serde_json::Value::Object(_) => 1,
        _ => {
            let resp = JsonRpcResponse::proxy_error(
                "Request must be a JSON-RPC object or batch array".to_string(),
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::to_value(resp).unwrap()),
            );
        }
    };

    chain_state.metrics.record_requests(request_count);

    // Rate limiting
    if let Some(ref limiter) = chain_state.rate_limiter {
        if limiter.check().is_err() {
            chain_state.metrics.record_rate_limited();
            let resp = JsonRpcResponse::proxy_error("Rate limit exceeded".to_string());
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::to_value(resp).unwrap()),
            );
        }
    }

    // --- Cache logic ---
    if let Some(cache) = &chain_state.cache {
        if is_batch {
            return handle_batch_with_cache(chain_state, cache, &parsed, &chain).await;
        } else {
            // Single request cache path
            if let Ok(rpc_req) = serde_json::from_value::<JsonRpcRequest>(parsed.clone()) {
                let cache_key = CacheKey::new(
                    &rpc_req.method,
                    rpc_req.params.as_ref().unwrap_or(&serde_json::Value::Null),
                );

                if cache.is_cacheable(&rpc_req.method) {
                    if let Some(cached) = cache.get(&cache_key).await {
                        chain_state.metrics.record_cache_hit();
                        chain_state.metrics.record_successes(1);
                        debug!(chain = %chain, method = %rpc_req.method, "Cache hit");
                        let value: serde_json::Value = serde_json::from_slice(&cached.body)
                            .unwrap_or_else(|_| {
                                serde_json::to_value(JsonRpcResponse::proxy_error(
                                    "Invalid cached response".to_string(),
                                ))
                                .unwrap()
                            });
                        return (StatusCode::OK, Json(value));
                    }
                    chain_state.metrics.record_cache_miss();
                    debug!(chain = %chain, method = %rpc_req.method, "Cache miss");
                }

                // Forward and cache the response
                let body_bytes = serde_json::to_vec(&parsed).unwrap();
                let result = forward_with_retry(
                    chain_state,
                    &body_bytes,
                    &chain,
                    request_count,
                    &rpc_req.method,
                )
                .await;

                // Store in cache on success
                if let (StatusCode::OK, Json(ref value)) = result {
                    if cache.is_cacheable(&rpc_req.method) {
                        let response_bytes = serde_json::to_vec(value).unwrap();
                        cache
                            .insert(
                                cache_key,
                                CachedResponse {
                                    status: 200,
                                    body: bytes::Bytes::from(response_bytes),
                                },
                            )
                            .await;
                    }
                }

                return result;
            }
        }
    }

    // --- No cache path (original logic) ---
    let method = if let serde_json::Value::Object(ref obj) = parsed {
        obj.get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
    } else {
        "unknown"
    };
    let method = method.to_string();
    forward_with_retry(chain_state, &body, &chain, request_count, &method).await
}

/// Handle a batch request with cache support.
/// Splits into cached hits and uncached misses, forwards only misses,
/// then merges results preserving original order by JSON-RPC id.
async fn handle_batch_with_cache(
    chain_state: &ChainState,
    cache: &ChainCache,
    parsed: &serde_json::Value,
    chain: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    let batch = parsed.as_array().unwrap();

    // For each item in batch, try cache lookup
    // We store (original_index, rpc_request, Option<cached_response>)
    let mut results: Vec<(usize, Option<serde_json::Value>)> = vec![(0, None); batch.len()];
    let mut uncached_indices: Vec<usize> = Vec::new();
    let mut uncached_requests: Vec<serde_json::Value> = Vec::new();
    // Track cache keys for uncached items so we can store responses later
    let mut uncached_cache_keys: Vec<Option<CacheKey>> = Vec::new();

    for (i, item) in batch.iter().enumerate() {
        if let Ok(rpc_req) = serde_json::from_value::<JsonRpcRequest>(item.clone()) {
            let cache_key = CacheKey::new(
                &rpc_req.method,
                rpc_req.params.as_ref().unwrap_or(&serde_json::Value::Null),
            );

            if cache.is_cacheable(&rpc_req.method) {
                if let Some(cached) = cache.get(&cache_key).await {
                    chain_state.metrics.record_cache_hit();
                    debug!(chain = %chain, method = %rpc_req.method, "Batch item cache hit");
                    let value: serde_json::Value =
                        serde_json::from_slice(&cached.body).unwrap_or(item.clone());
                    results[i] = (i, Some(value));
                    continue;
                }
                chain_state.metrics.record_cache_miss();
                debug!(chain = %chain, method = %rpc_req.method, "Batch item cache miss");
                uncached_indices.push(i);
                uncached_requests.push(item.clone());
                uncached_cache_keys.push(Some(cache_key));
                continue;
            }
        }
        // Non-cacheable or non-parseable: forward upstream
        uncached_indices.push(i);
        uncached_requests.push(item.clone());
        uncached_cache_keys.push(None);
    }

    // If all items were cached, return immediately
    if uncached_requests.is_empty() {
        let all_results: Vec<serde_json::Value> =
            results.into_iter().map(|(_, v)| v.unwrap()).collect();
        chain_state.metrics.record_successes(batch.len() as u64);
        return (StatusCode::OK, Json(serde_json::Value::Array(all_results)));
    }

    // --- Group uncached items by their eligible endpoint set ---
    use std::collections::HashMap;

    // routing_groups: key = sorted eligible indices as string (e.g. "0,1,2")
    //                 value = (eligible_indices, Vec<(position_in_uncached, original_batch_idx, cache_key)>)
    type RoutingGroup = (Vec<usize>, Vec<(usize, usize, Option<CacheKey>)>);
    let mut routing_groups: HashMap<String, RoutingGroup> = HashMap::new();

    for (pos, &orig_idx) in uncached_indices.iter().enumerate() {
        let item = &uncached_requests[pos];
        let cache_key = uncached_cache_keys[pos].clone();

        let method = item
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let eligible = chain_state.pool.eligible_indices_for_method(method);

        if eligible.is_empty() {
            chain_state.metrics.record_failures(batch.len() as u64);
            let resp = JsonRpcResponse::proxy_error(format!(
                "No endpoints configured for method: {}",
                method
            ));
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::to_value(resp).unwrap()),
            );
        }

        let key = eligible
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let entry = routing_groups
            .entry(key)
            .or_insert_with(|| (eligible, Vec::new()));
        entry.1.push((pos, orig_idx, cache_key));
    }

    // --- Forward each routing group as a sub-batch ---
    for (_eligible, group) in routing_groups.values() {
        let group_requests: Vec<serde_json::Value> = group
            .iter()
            .map(|(pos, _, _)| uncached_requests[*pos].clone())
            .collect();
        let group_count = group_requests.len() as u64;

        let forward_body = if group_requests.len() == 1 {
            serde_json::to_vec(&group_requests[0]).unwrap()
        } else {
            serde_json::to_vec(&group_requests).unwrap()
        };

        // Use a representative method from this group for forward_with_retry
        let representative_method = group_requests[0]
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let (status, upstream_json) = forward_with_retry(
            chain_state,
            &forward_body,
            chain,
            group_count,
            representative_method,
        )
        .await;

        if status != StatusCode::OK {
            return (status, upstream_json);
        }

        // Parse upstream responses
        let upstream_responses: Vec<serde_json::Value> = if group_requests.len() == 1 {
            vec![upstream_json.0]
        } else {
            match upstream_json.0 {
                serde_json::Value::Array(arr) => arr,
                other => vec![other],
            }
        };

        // Match responses back to original positions and cache if needed
        for (resp_pos, upstream_resp) in upstream_responses.into_iter().enumerate() {
            if resp_pos < group.len() {
                let (_, orig_idx, ref cache_key_opt) = group[resp_pos];

                // Store cacheable responses in cache
                if let Some(ref cache_key) = cache_key_opt {
                    let response_bytes = serde_json::to_vec(&upstream_resp).unwrap();
                    cache
                        .insert(
                            cache_key.clone(),
                            CachedResponse {
                                status: 200,
                                body: bytes::Bytes::from(response_bytes),
                            },
                        )
                        .await;
                }

                results[orig_idx] = (orig_idx, Some(upstream_resp));
            }
        }
    }

    // Build final ordered response
    let final_results: Vec<serde_json::Value> = results
        .into_iter()
        .map(|(_, v)| {
            v.unwrap_or_else(|| {
                serde_json::to_value(JsonRpcResponse::proxy_error(
                    "Missing response for batch item".to_string(),
                ))
                .unwrap()
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::Value::Array(final_results)),
    )
}

/// Forward a request to upstream with configurable retries.
async fn forward_with_retry(
    chain_state: &ChainState,
    body: &[u8],
    chain: &str,
    request_count: u64,
    method: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    let max_retries = chain_state.pool.health_config.max_retries;
    let base_retry_delay_ms = chain_state.pool.health_config.retry_delay_ms;
    let total_attempts = max_retries + 1;
    let mut excluded: Vec<usize> = Vec::new();
    let mut last_error = String::new();

    let eligible = chain_state.pool.eligible_indices_for_method(method);
    if eligible.is_empty() {
        chain_state.metrics.record_failures(request_count);
        let resp =
            JsonRpcResponse::proxy_error(format!("No endpoints configured for method: {}", method));
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::to_value(resp).unwrap()),
        );
    }

    for attempt in 0..total_attempts {
        // Exponential backoff: base_delay * 2^(attempt-1), min 100ms on retry
        if attempt > 0 {
            let delay = if base_retry_delay_ms > 0 {
                base_retry_delay_ms * (1u64 << (attempt - 1).min(4))
            } else {
                100 * (1u64 << (attempt - 1).min(4))
            };
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }

        // Hedged first attempt — but only if enough endpoints are healthy.
        // Suppress hedging when the pool is degraded (< half healthy) to
        // avoid amplifying load on already-stressed endpoints.
        if attempt == 0 {
            if let Some(ref hedge_config) = chain_state.pool.hedge_config {
                let healthy = chain_state.pool.healthy_count();
                let total = chain_state.pool.endpoints.len();
                let should_hedge = healthy > total / 2;
                if should_hedge {
                    match forward_with_hedging(
                        chain_state,
                        body,
                        chain,
                        request_count,
                        hedge_config,
                        &eligible,
                    )
                    .await
                    {
                        HedgeOutcome::Success(status, json) => return (status, json),
                        HedgeOutcome::AllFailed {
                            last_err,
                            failed_indices,
                        } => {
                            last_error = last_err;
                            excluded.extend(failed_indices);
                            continue;
                        }
                        HedgeOutcome::NoEndpoints => {
                            chain_state.metrics.record_failures(request_count);
                            error!(chain = %chain, "All endpoints are unhealthy");
                            let resp = JsonRpcResponse::proxy_error(
                                "All endpoints are unhealthy".to_string(),
                            );
                            return (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(serde_json::to_value(resp).unwrap()),
                            );
                        }
                    }
                }
            }
        }

        let endpoint_result = chain_state
            .pool
            .next_endpoint_from_eligible(&eligible, &excluded);

        let (idx, endpoint) = match endpoint_result {
            Some(ep) => ep,
            None => {
                chain_state.metrics.record_failures(request_count);
                error!(chain = %chain, "All endpoints are unhealthy");
                let resp = JsonRpcResponse::proxy_error("All endpoints are unhealthy".to_string());
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::to_value(resp).unwrap()),
                );
            }
        };

        let endpoint_url = endpoint.to_string();
        let auth = chain_state.pool.endpoints[idx].auth.as_ref();
        match chain_state
            .forwarder
            .forward(&endpoint_url, body, auth)
            .await
        {
            Ok((_status, response_bytes, latency_ms)) => {
                chain_state
                    .pool
                    .record_success_with_latency(idx, latency_ms);
                chain_state.metrics.record_successes(request_count);
                let value: serde_json::Value = serde_json::from_slice(&response_bytes)
                    .unwrap_or_else(|_| {
                        serde_json::to_value(JsonRpcResponse::proxy_error(
                            "Invalid JSON response from upstream".to_string(),
                        ))
                        .unwrap()
                    });
                return (StatusCode::OK, Json(value));
            }
            Err(crate::proxy::ForwardError::RateLimited) => {
                // 429 — record throttle (not a hard failure), exclude and retry
                chain_state.pool.record_throttle(idx);
                chain_state.metrics.record_upstream_throttled();
                last_error = "Rate limited (429)".to_string();
                warn!(chain = %chain, endpoint = %endpoint_url, attempt = attempt + 1, "Upstream rate limited, trying next");
                excluded.push(idx);
            }
            Err(e) => {
                last_error = e.to_string();
                if attempt < total_attempts - 1 {
                    warn!(chain = %chain, endpoint = %endpoint_url, error = %e, attempt = attempt + 1, "Request failed, retrying");
                } else {
                    error!(chain = %chain, endpoint = %endpoint_url, error = %e, "All retry attempts exhausted");
                }
                chain_state.pool.record_failure(idx);
                excluded.push(idx);
            }
        }
    }

    chain_state.metrics.record_failures(request_count);
    let resp = JsonRpcResponse::proxy_error(format!("All attempts failed: {}", last_error));
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::to_value(resp).unwrap()),
    )
}

enum HedgeOutcome {
    Success(StatusCode, Json<serde_json::Value>),
    AllFailed {
        last_err: String,
        failed_indices: Vec<usize>,
    },
    NoEndpoints,
}

/// Forward a request with hedging: fire primary, and after each `delay_ms`,
/// fire additional hedges up to `max_count`. First success wins.
async fn forward_with_hedging(
    chain_state: &ChainState,
    body: &[u8],
    chain: &str,
    request_count: u64,
    hedge_config: &HedgeConfig,
    eligible: &[usize],
) -> HedgeOutcome {
    type ForwardFuture<'a> = Pin<
        Box<
            dyn std::future::Future<
                    Output = (
                        usize,
                        Result<(u16, bytes::Bytes, u64), crate::proxy::forwarder::ForwardError>,
                    ),
                > + Send
                + 'a,
        >,
    >;

    // 1. Pick primary endpoint
    let (p_idx, p_url) = match chain_state.pool.next_endpoint_from_eligible(eligible, &[]) {
        Some((i, u)) => (i, u.to_string()),
        None => return HedgeOutcome::NoEndpoints,
    };

    let mut used_indices: Vec<usize> = vec![p_idx];
    let mut failed_indices: Vec<usize> = Vec::new();

    // 2. Start primary request
    let p_auth = chain_state.pool.endpoints[p_idx].auth.as_ref();
    let mut in_flight: FuturesUnordered<ForwardFuture> = FuturesUnordered::new();
    let idx = p_idx;
    in_flight.push(Box::pin(async move {
        let result = chain_state.forwarder.forward(&p_url, body, p_auth).await;
        (idx, result)
    }));

    let delay = Duration::from_millis(hedge_config.delay_ms);
    let max_hedges = hedge_config.max_count as usize;
    let mut hedges_fired: usize = 0;

    // 3. Race loop: poll in-flight futures vs hedge delay timer
    loop {
        let can_hedge = hedges_fired < max_hedges;

        if can_hedge {
            tokio::select! {
                biased;
                Some((idx, result)) = in_flight.next() => {
                    match result {
                        Ok((_status, ref bytes, latency)) => {
                            chain_state.pool.record_success_with_latency(idx, latency);
                            chain_state.metrics.record_successes(request_count);
                            return HedgeOutcome::Success(StatusCode::OK, Json(parse_bytes(bytes)));
                        }
                        Err(ref e) => {
                            if matches!(e, crate::proxy::ForwardError::RateLimited) {
                                chain_state.pool.record_throttle(idx);
                                chain_state.metrics.record_upstream_throttled();
                            } else {
                                chain_state.pool.record_failure(idx);
                            }
                            failed_indices.push(idx);
                            warn!(chain = %chain, error = %e, "Hedged request failed");
                            // If no more in-flight and can't hedge more, give up
                            if in_flight.is_empty() && hedges_fired >= max_hedges {
                                return HedgeOutcome::AllFailed {
                                    last_err: e.to_string(),
                                    failed_indices,
                                };
                            }
                            // If no more in-flight but can still hedge, fire immediately
                            if in_flight.is_empty() {
                                if let Some((h_idx, h_url)) = chain_state.pool.next_endpoint_from_eligible(eligible, &used_indices).map(|(i, u)| (i, u.to_string())) {
                                    used_indices.push(h_idx);
                                    hedges_fired += 1;
                                    chain_state.metrics.record_hedged_requests(1);
                                    let h_auth = chain_state.pool.endpoints[h_idx].auth.as_ref();
                                    in_flight.push(Box::pin(async move {
                                        let result = chain_state.forwarder.forward(&h_url, body, h_auth).await;
                                        (h_idx, result)
                                    }));
                                } else {
                                    return HedgeOutcome::AllFailed {
                                        last_err: e.to_string(),
                                        failed_indices,
                                    };
                                }
                            }
                        }
                    }
                }
                _ = tokio::time::sleep(delay) => {
                    // Delay expired, fire next hedge
                    if let Some((h_idx, h_url)) = chain_state.pool.next_endpoint_from_eligible(eligible, &used_indices).map(|(i, u)| (i, u.to_string())) {
                        used_indices.push(h_idx);
                        hedges_fired += 1;
                        chain_state.metrics.record_hedged_requests(1);
                        debug!(chain = %chain, hedge = hedges_fired, "Firing hedge request");
                        let h_auth = chain_state.pool.endpoints[h_idx].auth.as_ref();
                        in_flight.push(Box::pin(async move {
                            let result = chain_state.forwarder.forward(&h_url, body, h_auth).await;
                            (h_idx, result)
                        }));
                    }
                    // If no new endpoint available, just continue waiting for in-flight
                }
            }
        } else {
            // No more hedges to fire, just await remaining in-flight
            match in_flight.next().await {
                Some((idx, result)) => match result {
                    Ok((_status, ref bytes, latency)) => {
                        chain_state.pool.record_success_with_latency(idx, latency);
                        chain_state.metrics.record_successes(request_count);
                        return HedgeOutcome::Success(StatusCode::OK, Json(parse_bytes(bytes)));
                    }
                    Err(ref e) => {
                        if matches!(e, crate::proxy::ForwardError::RateLimited) {
                            chain_state.pool.record_throttle(idx);
                            chain_state.metrics.record_upstream_throttled();
                        } else {
                            chain_state.pool.record_failure(idx);
                        }
                        failed_indices.push(idx);
                        warn!(chain = %chain, error = %e, "Hedged request failed");
                        if in_flight.is_empty() {
                            return HedgeOutcome::AllFailed {
                                last_err: e.to_string(),
                                failed_indices,
                            };
                        }
                    }
                },
                None => {
                    // All futures completed without success
                    return HedgeOutcome::AllFailed {
                        last_err: "All hedged endpoints exhausted".to_string(),
                        failed_indices,
                    };
                }
            }
        }
    }
}

fn parse_bytes(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap_or_else(|_| {
        serde_json::to_value(JsonRpcResponse::proxy_error(
            "Invalid JSON response from upstream".to_string(),
        ))
        .unwrap()
    })
}
