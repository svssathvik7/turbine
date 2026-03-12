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
use governor::{Quota, RateLimiter};
use serde::Serialize;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub fn build_router(config: &Config) -> Router {
    let mut chains = HashMap::new();
    let mut chain_id_map = HashMap::new();

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

        let rate_limiter = chain_config.rate_limit.as_ref().map(|rl| {
            let period = Duration::from_secs(rl.window_seconds) / rl.max_requests;
            let quota = Quota::with_period(period)
                .unwrap()
                .allow_burst(NonZeroU32::new(rl.max_requests).unwrap());
            Arc::new(RateLimiter::direct(quota))
        });

        if let Some(chain_id) = chain_config.chain_id {
            chain_id_map.insert(chain_id, route.clone());
            info!(chain = %chain_config.name, chain_id = chain_id, "Registered chain ID route");
        }

        let chain_state = ChainState {
            pool,
            metrics: ChainMetrics::new(),
            forwarder: Forwarder::new(),
            cache,
            rate_limiter,
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
        chain_id_map,
        started_at: std::time::Instant::now(),
    });

    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/api/status", get(status_handler))
        .route("/dashboard", get(dashboard_handler))
        .route("/{chain}", post(proxy_handler))
        .with_state(state)
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> Json<Vec<ChainMetricsSnapshot>> {
    let snapshots: Vec<ChainMetricsSnapshot> = state
        .chains
        .values()
        .map(|chain_state| {
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
    chain_id: Option<u64>,
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    rate_limited_requests: u64,
    active_endpoints: usize,
    total_endpoints: usize,
    endpoints: Vec<EndpointStatus>,
}

async fn status_handler(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
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
                chain_id: chain_state.pool.chain_id,
                total_requests: metrics.total_requests,
                successful_requests: metrics.successful_requests,
                failed_requests: metrics.failed_requests,
                cache_hits: metrics.cache_hits,
                cache_misses: metrics.cache_misses,
                rate_limited_requests: metrics.rate_limited_requests,
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
