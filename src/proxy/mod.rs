mod forwarder;
mod handler;
mod server;
pub mod ws_handler;

pub use forwarder::{ForwardError, Forwarder};
pub use handler::proxy_handler;
pub use server::build_router;
pub use ws_handler::ws_proxy_handler;

use crate::cache::ChainCache;
use crate::health::ChainPool;
use crate::metrics::ChainMetrics;
use governor::DefaultDirectRateLimiter;
use std::collections::HashMap;
use std::sync::Arc;

pub struct AppState {
    pub chains: HashMap<String, ChainState>,
    pub chain_id_map: HashMap<u64, String>,
    pub started_at: std::time::Instant,
}

pub struct ChainState {
    pub pool: Arc<ChainPool>,
    pub metrics: ChainMetrics,
    pub forwarder: Forwarder,
    pub cache: Option<ChainCache>,
    pub rate_limiter: Option<Arc<DefaultDirectRateLimiter>>,
}
