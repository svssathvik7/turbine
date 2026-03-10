use super::{proxy_handler, AppState, ChainState, Forwarder};
use crate::cache::ChainCache;
use crate::config::Config;
use crate::dashboard::DASHBOARD_HTML;
use crate::health::{spawn_health_checker, ChainPool, EndpointStatus};
use crate::metrics::{ChainMetrics, ChainMetricsSnapshot};
use axum::extract::State;
use axum::response::{Html, Json};
use axum::routing::{get, post};
use axum::Router;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

pub fn build_router(config: &Config) -> Router {
    let mut chains = HashMap::new();

    for chain_config in &config.chains {
        let route = chain_config.route.trim_start_matches('/').to_string();
        let pool = Arc::new(ChainPool::new(chain_config));

        // Spawn background health checker
        spawn_health_checker(
            chain_config.name.clone(),
            Arc::clone(&pool),
            chain_config.health.health_method.clone(),
            chain_config.health.health_check_interval_seconds,
            chain_config.health.max_block_lag,
        );

        let cache = match &chain_config.cache {
            Some(cache_config) if cache_config.enabled => {
                let c = ChainCache::new(cache_config, &chain_config.name);
                info!(chain = %chain_config.name, "Cache enabled");
                Some(c)
            }
            _ => None,
        };

        let chain_state = ChainState {
            pool,
            metrics: ChainMetrics::new(),
            forwarder: Forwarder::new(),
            cache,
        };
        info!(
            chain = %chain_config.name,
            endpoints = chain_config.endpoints.len(),
            route = %chain_config.route,
            rotation = ?chain_config.rotation,
            "Registered chain"
        );
        chains.insert(route, chain_state);
    }

    let state = Arc::new(AppState {
        chains,
        started_at: std::time::Instant::now(),
    });

    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/api/status", get(status_handler))
        .route("/dashboard", get(dashboard_handler))
        .route("/{chain}", post(proxy_handler))
        .with_state(state)
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

#[derive(Serialize)]
struct StatusResponse {
    uptime_seconds: u64,
    chains: Vec<ChainStatusSnapshot>,
}

#[derive(Serialize)]
struct ChainStatusSnapshot {
    name: String,
    route: String,
    rotation: String,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    active_endpoints: usize,
    total_endpoints: usize,
    endpoints: Vec<EndpointStatus>,
}

async fn status_handler(
    State(state): State<Arc<AppState>>,
) -> Json<StatusResponse> {
    let uptime_seconds = state.started_at.elapsed().as_secs();

    let mut chains: Vec<ChainStatusSnapshot> = state
        .chains
        .iter()
        .map(|(route, chain_state)| {
            let metrics = chain_state.metrics.snapshot(
                &chain_state.pool.name,
                chain_state.pool.healthy_count(),
                chain_state.pool.endpoints.len(),
            );

            ChainStatusSnapshot {
                name: chain_state.pool.name.clone(),
                route: format!("/{}", route),
                rotation: chain_state.pool.rotation_name().to_string(),
                total_requests: metrics.total_requests,
                successful_requests: metrics.successful_requests,
                failed_requests: metrics.failed_requests,
                cache_hits: metrics.cache_hits,
                cache_misses: metrics.cache_misses,
                active_endpoints: metrics.active_endpoints,
                total_endpoints: metrics.total_endpoints,
                endpoints: chain_state.pool.endpoint_statuses(),
            }
        })
        .collect();

    chains.sort_by(|a, b| a.name.cmp(&b.name));

    Json(StatusResponse {
        uptime_seconds,
        chains,
    })
}

async fn dashboard_handler() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}
