mod forwarder;
mod handler;
mod server;

pub use forwarder::{ForwardError, Forwarder};
pub use handler::proxy_handler;
pub use server::build_router;

use crate::cache::ChainCache;
use crate::health::ChainPool;
use crate::metrics::ChainMetrics;
use std::collections::HashMap;
use std::sync::Arc;

pub struct AppState {
    pub chains: HashMap<String, ChainState>,
}

pub struct ChainState {
    pub pool: Arc<ChainPool>,
    pub metrics: ChainMetrics,
    pub forwarder: Forwarder,
    pub cache: Option<ChainCache>,
}
