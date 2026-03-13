use super::{AppState, ChainState};
use crate::cache::{CacheKey, CachedResponse, ChainCache};
use crate::config::HedgeConfig;
use crate::types::{JsonRpcRequest, JsonRpcResponse};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
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
    let retry_delay_ms = chain_state.pool.health_config.retry_delay_ms;
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
        if attempt > 0 && retry_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(retry_delay_ms)).await;
        }

        // Hedged first attempt
        if attempt == 0 {
            if let Some(ref hedge_config) = chain_state.pool.hedge_config {
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
                        let resp =
                            JsonRpcResponse::proxy_error("All endpoints are unhealthy".to_string());
                        return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(serde_json::to_value(resp).unwrap()),
                        );
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

/// Forward a request with hedging: fire primary, and if it doesn't respond
/// within `delay_ms`, fire a hedge to a different endpoint. First success wins.
async fn forward_with_hedging(
    chain_state: &ChainState,
    body: &[u8],
    chain: &str,
    request_count: u64,
    hedge_config: &HedgeConfig,
    eligible: &[usize],
) -> HedgeOutcome {
    // 1. Pick primary endpoint
    let (p_idx, p_url) = match chain_state.pool.next_endpoint_from_eligible(eligible, &[]) {
        Some((i, u)) => (i, u.to_string()),
        None => return HedgeOutcome::NoEndpoints,
    };
    let p_auth = chain_state.pool.endpoints[p_idx].auth.as_ref();

    // 2. Pick hedge endpoint (different from primary)
    let hedge_ep = chain_state
        .pool
        .next_endpoint_from_eligible(eligible, &[p_idx])
        .map(|(i, u)| (i, u.to_string()));

    // 3. Start primary, race against delay
    let primary_fut = chain_state.forwarder.forward(&p_url, body, p_auth);
    tokio::pin!(primary_fut);

    let delay = Duration::from_millis(hedge_config.delay_ms);
    let primary_early = tokio::select! {
        result = &mut primary_fut => Some(result),
        _ = tokio::time::sleep(delay) => None,
    };

    // 4a. Primary responded before delay — return it, no hedge fired
    if let Some(Ok((_status, ref bytes, latency))) = primary_early {
        chain_state.pool.record_success_with_latency(p_idx, latency);
        chain_state.metrics.record_successes(request_count);
        return HedgeOutcome::Success(StatusCode::OK, Json(parse_bytes(bytes)));
    }

    // 4b. Primary failed before delay — record failure, try hedge sequentially
    if let Some(Err(e)) = primary_early {
        chain_state.pool.record_failure(p_idx);
        warn!(chain = %chain, endpoint = %p_url, error = %e, "Primary failed before hedge delay");
        if let Some((h_idx, ref h_url)) = hedge_ep {
            chain_state.metrics.record_hedged_requests(1);
            let h_auth = chain_state.pool.endpoints[h_idx].auth.as_ref();
            match chain_state.forwarder.forward(h_url, body, h_auth).await {
                Ok((_status, ref bytes, latency)) => {
                    chain_state.pool.record_success_with_latency(h_idx, latency);
                    chain_state.metrics.record_successes(request_count);
                    return HedgeOutcome::Success(StatusCode::OK, Json(parse_bytes(bytes)));
                }
                Err(e2) => {
                    chain_state.pool.record_failure(h_idx);
                    return HedgeOutcome::AllFailed {
                        last_err: e2.to_string(),
                        failed_indices: vec![p_idx, h_idx],
                    };
                }
            }
        }
        return HedgeOutcome::AllFailed {
            last_err: e.to_string(),
            failed_indices: vec![p_idx],
        };
    }

    // 5. Delay expired, primary still running. Fire hedge and race.
    let Some((h_idx, ref h_url)) = hedge_ep else {
        // No hedge endpoint available — just await primary
        return match primary_fut.await {
            Ok((_status, ref bytes, latency)) => {
                chain_state.pool.record_success_with_latency(p_idx, latency);
                chain_state.metrics.record_successes(request_count);
                HedgeOutcome::Success(StatusCode::OK, Json(parse_bytes(bytes)))
            }
            Err(e) => {
                chain_state.pool.record_failure(p_idx);
                HedgeOutcome::AllFailed {
                    last_err: e.to_string(),
                    failed_indices: vec![p_idx],
                }
            }
        };
    };

    chain_state.metrics.record_hedged_requests(1);
    debug!(chain = %chain, "Hedge delay expired, firing hedge to alternate endpoint");

    let h_auth = chain_state.pool.endpoints[h_idx].auth.as_ref();
    let hedge_fut = chain_state.forwarder.forward(h_url, body, h_auth);
    tokio::pin!(hedge_fut);

    // Race primary vs hedge — if first responder fails, await the other
    let (first_result, first_idx, second_is_primary) = tokio::select! {
        r = &mut primary_fut => (r, p_idx, false),
        r = &mut hedge_fut => (r, h_idx, true),
    };

    match first_result {
        Ok((_status, ref bytes, latency)) => {
            // Winner succeeded — loser is dropped/cancelled
            chain_state
                .pool
                .record_success_with_latency(first_idx, latency);
            chain_state.metrics.record_successes(request_count);
            HedgeOutcome::Success(StatusCode::OK, Json(parse_bytes(bytes)))
        }
        Err(e) => {
            // First responder failed — await the other
            chain_state.pool.record_failure(first_idx);
            warn!(chain = %chain, error = %e, "First responder failed, awaiting other");

            let second_result = if second_is_primary {
                primary_fut.await
            } else {
                hedge_fut.await
            };
            let second_idx = if second_is_primary { p_idx } else { h_idx };

            match second_result {
                Ok((_status, ref bytes, latency)) => {
                    chain_state
                        .pool
                        .record_success_with_latency(second_idx, latency);
                    chain_state.metrics.record_successes(request_count);
                    HedgeOutcome::Success(StatusCode::OK, Json(parse_bytes(bytes)))
                }
                Err(e2) => {
                    chain_state.pool.record_failure(second_idx);
                    HedgeOutcome::AllFailed {
                        last_err: e2.to_string(),
                        failed_indices: vec![p_idx, h_idx],
                    }
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
