//! Embedded static file serving via rust-embed.
//! Asset sync marker: 2026-03-18 orchestration canvas + litegraph.js.

use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "dashboard/"]
struct DashboardAssets;

/// Serve embedded static files. Falls back to index.html for SPA routing.
/// Query strings (e.g. `?v=20260318a`) are stripped for cache-busting support.
pub async fn static_handler(req: Request) -> Response {
    let raw_path = req.uri().path().trim_start_matches('/');
    let path = if raw_path.is_empty() { "index.html" } else { raw_path };

    serve_embedded(path).unwrap_or_else(|| serve_embedded("index.html").unwrap_or_else(not_found))
}

fn serve_embedded(path: &str) -> Option<Response> {
    let file = DashboardAssets::get(path)?;
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    Some(
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (
                    header::CACHE_CONTROL,
                    "no-store, no-cache, must-revalidate, max-age=0".to_string(),
                ),
                (header::PRAGMA, "no-cache".to_string()),
                (header::EXPIRES, "0".to_string()),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self' ws: wss:; font-src 'self' data:; frame-src 'self' https://p2pclaw.com https://*.p2pclaw.com https://www.agentpmt.com https://*.agentpmt.com".to_string(),
                ),
            ],
            file.data.to_vec(),
        )
            .into_response(),
    )
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}
