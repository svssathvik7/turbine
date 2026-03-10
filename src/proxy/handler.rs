use super::{AppState, ChainState};
use crate::cache::{CacheKey, CachedResponse, ChainCache};
use crate::types::{JsonRpcRequest, JsonRpcResponse};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use std::sync::Arc;
use tracing::{debug, error, warn};

pub async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    Path(chain): Path<String>,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let chain_state = match state.chains.get(&chain) {
        Some(s) => s,
        None => {
            let resp = JsonRpcResponse::proxy_error(format!("Unknown chain: {}", chain));
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(resp).unwrap()),
            );
        }
    };

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
                        let value: serde_json::Value =
                            serde_json::from_slice(&cached.body).unwrap_or_else(|_| {
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
                let result =
                    forward_with_retry(chain_state, &body_bytes, &chain, request_count).await;

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
    forward_with_retry(chain_state, &body, &chain, request_count).await
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
    let mut results: Vec<(usize, Option<serde_json::Value>)> =
        vec![(0, None); batch.len()];
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
        let all_results: Vec<serde_json::Value> = results
            .into_iter()
            .map(|(_, v)| v.unwrap())
            .collect();
        chain_state
            .metrics
            .record_successes(batch.len() as u64);
        return (StatusCode::OK, Json(serde_json::Value::Array(all_results)));
    }

    // Forward uncached items as a batch (or single if only one)
    let forward_body = if uncached_requests.len() == 1 {
        serde_json::to_vec(&uncached_requests[0]).unwrap()
    } else {
        serde_json::to_vec(&uncached_requests).unwrap()
    };

    let uncached_count = uncached_requests.len() as u64;

    let (status, upstream_json) =
        forward_with_retry(chain_state, &forward_body, chain, uncached_count).await;

    if status != StatusCode::OK {
        // If upstream failed, return the error for the whole batch
        return (status, upstream_json);
    }

    // Parse upstream responses and match back to original positions
    let upstream_responses: Vec<serde_json::Value> = if uncached_requests.len() == 1 {
        vec![upstream_json.0]
    } else {
        match upstream_json.0 {
            serde_json::Value::Array(arr) => arr,
            other => vec![other],
        }
    };

    // Match upstream responses to uncached indices by position
    for (pos, upstream_resp) in upstream_responses.into_iter().enumerate() {
        if pos < uncached_indices.len() {
            let orig_idx = uncached_indices[pos];

            // Store cacheable responses in cache
            if let Some(ref cache_key) = uncached_cache_keys[pos] {
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

/// Forward a request to upstream with one retry on failure.
async fn forward_with_retry(
    chain_state: &ChainState,
    body: &[u8],
    chain: &str,
    request_count: u64,
) -> (StatusCode, Json<serde_json::Value>) {
    // First attempt
    let (idx, endpoint) = match chain_state.pool.next_endpoint() {
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
    match chain_state.forwarder.forward(&endpoint_url, body, auth).await {
        Ok((_status, response_bytes, latency_ms)) => {
            chain_state.pool.record_success_with_latency(idx, latency_ms);
            chain_state.metrics.record_successes(request_count);
            let value: serde_json::Value =
                serde_json::from_slice(&response_bytes).unwrap_or_else(|_| {
                    serde_json::to_value(JsonRpcResponse::proxy_error(
                        "Invalid JSON response from upstream".to_string(),
                    ))
                    .unwrap()
                });
            return (StatusCode::OK, Json(value));
        }
        Err(e) => {
            warn!(chain = %chain, endpoint = %endpoint_url, error = %e, "Request failed, retrying");
            chain_state.pool.record_failure(idx);
        }
    }

    // Retry with a different endpoint
    let (retry_idx, retry_endpoint) = match chain_state.pool.next_endpoint_excluding(idx) {
        Some(ep) => ep,
        None => {
            chain_state.metrics.record_failures(request_count);
            error!(chain = %chain, "No healthy endpoints available for retry");
            let resp =
                JsonRpcResponse::proxy_error("All endpoints failed or unhealthy".to_string());
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::to_value(resp).unwrap()),
            );
        }
    };

    let retry_url = retry_endpoint.to_string();
    let retry_auth = chain_state.pool.endpoints[retry_idx].auth.as_ref();
    match chain_state.forwarder.forward(&retry_url, body, retry_auth).await {
        Ok((_status, response_bytes, latency_ms)) => {
            chain_state.pool.record_success_with_latency(retry_idx, latency_ms);
            chain_state.metrics.record_successes(request_count);
            let value: serde_json::Value =
                serde_json::from_slice(&response_bytes).unwrap_or_else(|_| {
                    serde_json::to_value(JsonRpcResponse::proxy_error(
                        "Invalid JSON response from upstream".to_string(),
                    ))
                    .unwrap()
                });
            (StatusCode::OK, Json(value))
        }
        Err(e) => {
            error!(chain = %chain, endpoint = %retry_url, error = %e, "Retry also failed");
            chain_state.pool.record_failure(retry_idx);
            chain_state.metrics.record_failures(request_count);
            let resp = JsonRpcResponse::proxy_error(format!("All attempts failed: {}", e));
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::to_value(resp).unwrap()),
            )
        }
    }
}
