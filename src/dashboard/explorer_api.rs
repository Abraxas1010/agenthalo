//! Proof Explorer Game — API endpoints (stub / simulation mode).
//!
//! The game runs its simulation engine client-side using pre-computed proof
//! trees. These endpoints define the contract for future Lean server
//! integration (Pantograph / LeanInteract).
//!
//! When a real Lean proof server is available, implement these handlers
//! to delegate to the server process. The frontend transparently switches
//! between client-side simulation and server-backed mode based on the
//! `/api/explorer/status` response.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

/// GET /api/explorer/status — reports proof server availability.
pub async fn api_explorer_status() -> impl IntoResponse {
    Json(json!({
        "mode": "simulation",
        "lean_server": false,
        "message": "Client-side simulation active. Connect a Lean proof server for genuine verification.",
        "library_count": 12,
    }))
}

/// POST /api/explorer/load — start a new game session.
/// Returns 501 in stub mode; client-side simulation handles this.
pub async fn api_explorer_load() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Lean proof server not connected. Use client-side simulation.",
            "hint": "The frontend simulation engine handles theorem loading directly."
        })),
    )
}

/// POST /api/explorer/tactic — apply a tactic to a goal.
pub async fn api_explorer_tactic() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Lean proof server not connected. Use client-side simulation."
        })),
    )
}

/// POST /api/explorer/suggest — get tactic suggestions for a goal.
pub async fn api_explorer_suggest() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Lean proof server not connected. Suggestions generated client-side."
        })),
    )
}

/// POST /api/explorer/verify — compile the proof via Lean.
pub async fn api_explorer_verify() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Lean proof server not connected. Verification simulated client-side.",
            "hint": "Connect Pantograph or LeanInteract for genuine Lean compiler verification."
        })),
    )
}

/// POST /api/explorer/hint — spawn HALO agent for tactic hint.
pub async fn api_explorer_hint() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "HALO agent hint integration pending. Basic hints available client-side."
        })),
    )
}

/// POST /api/explorer/autosolve — spawn proof search agent.
pub async fn api_explorer_autosolve() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "HALO agent auto-solve integration pending."
        })),
    )
}

/// GET /api/explorer/library — serve the theorem library.
/// In production this would return theorems from HeytingLean + Mathlib.
/// For now, the client-side library is authoritative.
pub async fn api_explorer_library() -> impl IntoResponse {
    Json(json!({
        "source": "client-side",
        "message": "Theorem library is embedded in the frontend. Server-side library not yet available.",
        "categories": [
            { "name": "Tutorial", "count": 5 },
            { "name": "Logic", "count": 5 },
            { "name": "Arithmetic", "count": 2 },
        ]
    }))
}
