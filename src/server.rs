use crate::config::Config;
use crate::forwarder::Forwarder;
use crate::metrics::{ChainMetrics, ChainMetricsSnapshot};
use crate::pool::ChainPool;
use crate::types::JsonRpcResponse;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct AppState {
    pub chains: HashMap<String, ChainState>,
}

pub struct ChainState {
    pub pool: ChainPool,
    pub metrics: ChainMetrics,
    pub forwarder: Forwarder,
}

pub fn build_router(config: &Config) -> Router {
    let mut chains = HashMap::new();

    for chain_config in &config.chains {
        let route = chain_config.route.trim_start_matches('/').to_string();
        let chain_state = ChainState {
            pool: ChainPool::new(chain_config),
            metrics: ChainMetrics::new(),
            forwarder: Forwarder::new(),
        };
        info!(
            chain = %chain_config.name,
            endpoints = chain_config.endpoints.len(),
            route = %chain_config.route,
            "Registered chain"
        );
        chains.insert(route, chain_state);
    }

    let state = Arc::new(AppState { chains });

    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/{chain}", post(proxy_handler))
        .with_state(state)
}

async fn proxy_handler(
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
    match chain_state.forwarder.forward(&endpoint_url, &body).await {
        Ok((_status, response_bytes)) => {
            chain_state.pool.record_success(idx);
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
    match chain_state.forwarder.forward(&retry_url, &body).await {
        Ok((_status, response_bytes)) => {
            chain_state.pool.record_success(retry_idx);
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

async fn metrics_handler(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<ChainMetricsSnapshot>> {
    let snapshots: Vec<ChainMetricsSnapshot> = state
        .chains
        .iter()
        .map(|(_, chain_state)| {
            chain_state.metrics.snapshot(
                &chain_state.pool.name,
                chain_state.pool.healthy_count(),
                chain_state.pool.endpoints.len(),
            )
        })
        .collect();

    Json(snapshots)
}
