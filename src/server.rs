use crate::config::Config;
use crate::forwarder::Forwarder;
use crate::health_checker::spawn_health_checker;
use crate::metrics::{ChainMetrics, ChainMetricsSnapshot};
use crate::pool::ChainPool;
use crate::router::{proxy_handler, AppState, ChainState};
use axum::extract::State;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
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

        let chain_state = ChainState {
            pool,
            metrics: ChainMetrics::new(),
            forwarder: Forwarder::new(),
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

    let state = Arc::new(AppState { chains });

    Router::new()
        .route("/metrics", get(metrics_handler))
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
