use axum::extract::ws::{CloseFrame as AxumCloseFrame, Message as AxumMessage, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::Response;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::{
    self,
    client::IntoClientRequest,
    http::HeaderValue,
    protocol::{frame::coding::CloseCode, CloseFrame as TungsteniteCloseFrame},
    Message as TungsteniteMessage,
};
use tracing::{debug, error, info, warn};

use crate::config::EndpointAuth;
use crate::proxy::AppState;

/// Axum handler for WebSocket upgrade requests.
///
/// Looks up the chain by route name (or chain ID fallback), upgrades the
/// connection, then hands off to `handle_ws_connection` for the bidirectional
/// relay loop.
pub async fn ws_proxy_handler(
    ws: WebSocketUpgrade,
    Path(chain): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    // Resolve chain key — same pattern as proxy_handler
    let chain_key = if state.chains.contains_key(&chain) {
        chain.clone()
    } else if let Ok(id) = chain.parse::<u64>() {
        match state.chain_id_map.get(&id) {
            Some(key) => key.clone(),
            None => {
                return axum::response::IntoResponse::into_response((
                    axum::http::StatusCode::NOT_FOUND,
                    format!("Unknown chain: {}", chain),
                ));
            }
        }
    } else {
        return axum::response::IntoResponse::into_response((
            axum::http::StatusCode::NOT_FOUND,
            format!("Unknown chain: {}", chain),
        ));
    };

    // Clone Arc and chain_key into the upgrade callback (must be 'static + Send)
    let state_clone = Arc::clone(&state);
    let chain_key_clone = chain_key.clone();

    ws.on_upgrade(move |socket| handle_ws_connection(socket, state_clone, chain_key_clone))
}

/// Core WebSocket relay logic.
///
/// 1. Selects an upstream endpoint via the chain pool
/// 2. Connects to the upstream WS with auth headers
/// 3. Bidirectionally relays messages between client and upstream
/// 4. On upstream disconnect, attempts reconnection to a different endpoint
///    and notifies the client with a `turbine_reconnected` JSON-RPC notification
async fn handle_ws_connection(client_ws: WebSocket, state: Arc<AppState>, chain_key: String) {
    let chain_state = match state.chains.get(&chain_key) {
        Some(cs) => cs,
        None => {
            error!(chain = %chain_key, "Chain state disappeared after upgrade");
            return;
        }
    };

    let pool = &chain_state.pool;
    let metrics = &chain_state.metrics;

    // Track connection metrics
    metrics.ws_connections_total.fetch_add(1, Ordering::Relaxed);
    metrics
        .ws_active_connections
        .fetch_add(1, Ordering::Relaxed);

    // Ensure active connections are decremented on exit
    let _guard = WsConnectionGuard { metrics };

    // Select initial upstream endpoint
    let (mut upstream_idx, upstream_url_str) = match pool.next_endpoint() {
        Some((idx, url)) => (idx, url.to_string()),
        None => {
            error!(chain = %chain_key, "No healthy endpoints for WS connection");
            return;
        }
    };

    let ws_url = match pool.endpoints[upstream_idx].effective_ws_url() {
        Some(url) => url,
        None => {
            error!(chain = %chain_key, endpoint = %upstream_url_str, "No WS URL available");
            return;
        }
    };

    let auth = pool.endpoints[upstream_idx].auth.as_ref();

    // Connect to upstream
    let upstream_ws = match connect_upstream(&ws_url, auth).await {
        Ok(ws) => ws,
        Err(e) => {
            error!(chain = %chain_key, endpoint = %ws_url, error = %e, "Failed to connect upstream WS");
            return;
        }
    };

    info!(chain = %chain_key, endpoint = %ws_url, "WS upstream connected");

    // Split both connections into sink + stream halves
    let (mut client_sink, mut client_stream) = client_ws.split();
    let (mut upstream_sink, mut upstream_stream) = upstream_ws.split();

    // Bidirectional relay loop
    loop {
        tokio::select! {
            // Client -> Upstream
            client_msg = client_stream.next() => {
                match client_msg {
                    Some(Ok(msg)) => {
                        if let AxumMessage::Close(_) = &msg {
                            debug!(chain = %chain_key, "Client sent close frame");
                            // Forward close to upstream and exit
                            let _ = upstream_sink.send(axum_to_tungstenite(msg)).await;
                            break;
                        }
                        metrics.ws_messages_relayed.fetch_add(1, Ordering::Relaxed);
                        if let Err(e) = upstream_sink.send(axum_to_tungstenite(msg)).await {
                            warn!(chain = %chain_key, error = %e, "Failed to send to upstream");
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        debug!(chain = %chain_key, error = %e, "Client stream error");
                        break;
                    }
                    None => {
                        debug!(chain = %chain_key, "Client stream ended");
                        break;
                    }
                }
            }
            // Upstream -> Client
            upstream_msg = upstream_stream.next() => {
                match upstream_msg {
                    Some(Ok(msg)) => {
                        if let TungsteniteMessage::Close(_) = &msg {
                            debug!(chain = %chain_key, "Upstream sent close frame");
                            // Attempt reconnection to a different endpoint
                            match attempt_reconnect(pool, upstream_idx, &chain_key).await {
                                Some((new_idx, new_ws, new_url)) => {
                                    upstream_idx = new_idx;
                                    let (new_sink, new_stream) = new_ws.split();
                                    upstream_sink = new_sink;
                                    upstream_stream = new_stream;
                                    metrics.ws_reconnections.fetch_add(1, Ordering::Relaxed);
                                    info!(chain = %chain_key, endpoint = %new_url, "WS reconnected to new upstream");

                                    // Notify client of reconnection
                                    let notification = serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "method": "turbine_reconnected",
                                        "params": {"reason": "upstream_disconnect"}
                                    });
                                    let _ = client_sink.send(AxumMessage::Text(notification.to_string().into())).await;
                                    continue;
                                }
                                None => {
                                    warn!(chain = %chain_key, "No alternative endpoint for WS reconnect");
                                    let _ = client_sink.send(tungstenite_to_axum(TungsteniteMessage::Close(None))).await;
                                    break;
                                }
                            }
                        }
                        metrics.ws_messages_relayed.fetch_add(1, Ordering::Relaxed);
                        if let Err(e) = client_sink.send(tungstenite_to_axum(msg)).await {
                            debug!(chain = %chain_key, error = %e, "Failed to send to client");
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        warn!(chain = %chain_key, error = %e, "Upstream stream error, attempting reconnect");
                        // Upstream errored — try to reconnect
                        match attempt_reconnect(pool, upstream_idx, &chain_key).await {
                            Some((new_idx, new_ws, new_url)) => {
                                upstream_idx = new_idx;
                                let (new_sink, new_stream) = new_ws.split();
                                upstream_sink = new_sink;
                                upstream_stream = new_stream;
                                metrics.ws_reconnections.fetch_add(1, Ordering::Relaxed);
                                info!(chain = %chain_key, endpoint = %new_url, "WS reconnected after upstream error");

                                let notification = serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "method": "turbine_reconnected",
                                    "params": {"reason": "upstream_disconnect"}
                                });
                                let _ = client_sink.send(AxumMessage::Text(notification.to_string().into())).await;
                                continue;
                            }
                            None => {
                                warn!(chain = %chain_key, "No alternative endpoint for WS reconnect");
                                let _ = client_sink.send(tungstenite_to_axum(TungsteniteMessage::Close(None))).await;
                                break;
                            }
                        }
                    }
                    None => {
                        debug!(chain = %chain_key, "Upstream stream ended, attempting reconnect");
                        match attempt_reconnect(pool, upstream_idx, &chain_key).await {
                            Some((new_idx, new_ws, new_url)) => {
                                upstream_idx = new_idx;
                                let (new_sink, new_stream) = new_ws.split();
                                upstream_sink = new_sink;
                                upstream_stream = new_stream;
                                metrics.ws_reconnections.fetch_add(1, Ordering::Relaxed);
                                info!(chain = %chain_key, endpoint = %new_url, "WS reconnected after upstream stream end");

                                let notification = serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "method": "turbine_reconnected",
                                    "params": {"reason": "upstream_disconnect"}
                                });
                                let _ = client_sink.send(AxumMessage::Text(notification.to_string().into())).await;
                                continue;
                            }
                            None => {
                                warn!(chain = %chain_key, "No alternative endpoint for WS reconnect");
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    debug!(chain = %chain_key, "WS connection relay ended");
}

/// Attempt to reconnect to a different upstream endpoint, excluding the current one.
async fn attempt_reconnect(
    pool: &crate::health::ChainPool,
    exclude_idx: usize,
    chain_key: &str,
) -> Option<(
    usize,
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    String,
)> {
    let (new_idx, _url) = pool.next_endpoint_excluding(exclude_idx)?;
    let ws_url = pool.endpoints[new_idx].effective_ws_url()?;
    let auth = pool.endpoints[new_idx].auth.as_ref();

    match connect_upstream(&ws_url, auth).await {
        Ok(ws) => {
            info!(chain = %chain_key, endpoint = %ws_url, "Reconnected to alternative upstream");
            Some((new_idx, ws, ws_url))
        }
        Err(e) => {
            error!(chain = %chain_key, endpoint = %ws_url, error = %e, "Failed to reconnect to alternative upstream");
            None
        }
    }
}

/// Connect to an upstream WebSocket endpoint with optional auth headers.
async fn connect_upstream(
    ws_url: &str,
    auth: Option<&EndpointAuth>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tungstenite::Error,
> {
    let mut request = ws_url.into_client_request().map_err(|e| {
        tungstenite::Error::Http(Box::new(
            tungstenite::http::Response::builder()
                .status(400)
                .body(Some(format!("Invalid WS URL: {}", e).into_bytes()))
                .unwrap(),
        ))
    })?;

    // Inject auth into the upgrade request headers
    if let Some(auth) = auth {
        match auth {
            EndpointAuth::Basic { username, password } => {
                let credentials = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", username, password));
                request.headers_mut().insert(
                    "Authorization",
                    HeaderValue::from_str(&format!("Basic {}", credentials)).unwrap(),
                );
            }
            EndpointAuth::Bearer(token) => {
                request.headers_mut().insert(
                    "Authorization",
                    HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
                );
            }
            EndpointAuth::Header { name, value } => {
                let header_name =
                    tungstenite::http::HeaderName::from_bytes(name.as_bytes()).unwrap();
                request
                    .headers_mut()
                    .insert(header_name, HeaderValue::from_str(value).unwrap());
            }
        }
    }

    let (ws_stream, _response) = tokio_tungstenite::connect_async(request).await?;
    Ok(ws_stream)
}

// ---------------------------------------------------------------------------
// Message type conversions between axum WS and tungstenite
// ---------------------------------------------------------------------------

fn axum_to_tungstenite(msg: AxumMessage) -> TungsteniteMessage {
    match msg {
        AxumMessage::Text(text) => TungsteniteMessage::Text(text.to_string().into()),
        AxumMessage::Binary(data) => TungsteniteMessage::Binary(bytes::Bytes::from(data.to_vec())),
        AxumMessage::Ping(data) => TungsteniteMessage::Ping(bytes::Bytes::from(data.to_vec())),
        AxumMessage::Pong(data) => TungsteniteMessage::Pong(bytes::Bytes::from(data.to_vec())),
        AxumMessage::Close(Some(cf)) => TungsteniteMessage::Close(Some(TungsteniteCloseFrame {
            code: CloseCode::from(cf.code),
            reason: cf.reason.to_string().into(),
        })),
        AxumMessage::Close(None) => TungsteniteMessage::Close(None),
    }
}

fn tungstenite_to_axum(msg: TungsteniteMessage) -> AxumMessage {
    match msg {
        TungsteniteMessage::Text(text) => AxumMessage::Text(text.to_string().into()),
        TungsteniteMessage::Binary(data) => AxumMessage::Binary(data.to_vec().into()),
        TungsteniteMessage::Ping(data) => AxumMessage::Ping(data.to_vec().into()),
        TungsteniteMessage::Pong(data) => AxumMessage::Pong(data.to_vec().into()),
        TungsteniteMessage::Close(Some(cf)) => AxumMessage::Close(Some(AxumCloseFrame {
            code: cf.code.into(),
            reason: cf.reason.to_string().into(),
        })),
        TungsteniteMessage::Close(None) => AxumMessage::Close(None),
        TungsteniteMessage::Frame(_) => AxumMessage::Binary(vec![].into()),
    }
}

// ---------------------------------------------------------------------------
// RAII guard to decrement ws_active_connections on drop
// ---------------------------------------------------------------------------

struct WsConnectionGuard<'a> {
    metrics: &'a crate::metrics::ChainMetrics,
}

impl<'a> Drop for WsConnectionGuard<'a> {
    fn drop(&mut self) {
        self.metrics
            .ws_active_connections
            .fetch_sub(1, Ordering::Relaxed);
    }
}
