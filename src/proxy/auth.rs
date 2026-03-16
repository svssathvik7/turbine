use super::AppState;
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Fast path: no keys configured → open access
    if state.api_keys.is_empty() {
        return next.run(req).await;
    }

    // Extract key from Authorization: Bearer <key> or X-Api-Key: <key>
    let extracted = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned)
        .or_else(|| {
            req.headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        });

    let key_str = match extracted {
        Some(k) if !k.trim().is_empty() => k,
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32600, "message": "Missing or invalid API key" },
                    "id": null
                })),
            )
                .into_response();
        }
    };

    let entry = match state.api_keys.get(&key_str) {
        Some(e) => e,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32600, "message": "Invalid API key" },
                    "id": null
                })),
            )
                .into_response();
        }
    };

    // Per-key rate limiting
    if let Some(ref limiter) = entry.rate_limiter {
        if limiter.check().is_err() {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32029, "message": "Rate limit exceeded for API key" },
                    "id": null
                })),
            )
                .into_response();
        }
    }

    next.run(req).await
}
