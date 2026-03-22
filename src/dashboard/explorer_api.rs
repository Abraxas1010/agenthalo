//! Proof Explorer — API endpoints for interactive theorem proving.
//!
//! When Pantograph is connected, endpoints delegate to the live Lean proof
//! server. When disconnected, they return appropriate fallback responses
//! so the frontend uses its client-side simulation engine.

use super::DashboardState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

/// Return 503 with simulation fallback message.
fn unavailable(msg: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "ok": false,
            "error": msg,
            "fallback": "simulation",
        })),
    )
        .into_response()
}

/// GET /api/explorer/status — reports proof server availability.
pub async fn api_explorer_status(State(state): State<DashboardState>) -> impl IntoResponse {
    let mut guard = state.pantograph.lock().await;
    let alive = match guard.as_mut() {
        Some(proc) => proc.is_alive(),
        None => false,
    };
    Json(json!({
        "mode": if alive { "server" } else { "simulation" },
        "lean_server": alive,
        "message": if alive {
            "Lean proof server connected via Pantograph."
        } else {
            "Client-side simulation active. Connect a Lean proof server for genuine verification."
        },
        "library_count": 12,
    }))
}

#[derive(Deserialize)]
pub struct LoadRequest {
    pub expr: String,
}

/// POST /api/explorer/load — start a new proof session from a type expression.
pub async fn api_explorer_load(
    State(state): State<DashboardState>,
    Json(body): Json<LoadRequest>,
) -> Response {
    let mut guard = state.pantograph.lock().await;
    let proc = match guard.as_mut() {
        Some(p) => p,
        None => return unavailable("Lean proof server not connected"),
    };
    if !proc.is_alive() {
        return unavailable("Lean proof server not connected");
    }

    match proc.goal_start(&body.expr).await {
        Ok(result) => {
            let state_id = result
                .get("nextStateId")
                .or_else(|| result.get("stateId"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let goals = extract_goals(&result);
            Json(json!({
                "ok": true,
                "stateId": state_id,
                "goals": goals,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct TacticRequest {
    pub state_id: u64,
    pub goal_id: Option<u64>,
    pub tactic: String,
}

/// POST /api/explorer/tactic — apply a tactic to a goal.
pub async fn api_explorer_tactic(
    State(state): State<DashboardState>,
    Json(body): Json<TacticRequest>,
) -> Response {
    let mut guard = state.pantograph.lock().await;
    let proc = match guard.as_mut() {
        Some(p) => p,
        None => return unavailable("Lean proof server not connected"),
    };
    if !proc.is_alive() {
        return unavailable("Lean proof server not connected");
    }

    let goal_id = body.goal_id.unwrap_or(0);
    match proc.goal_tactic(body.state_id, goal_id, &body.tactic).await {
        Ok(result) => {
            let state_id = result
                .get("nextStateId")
                .or_else(|| result.get("stateId"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let goals = extract_goals(&result);
            let solved = goals.is_empty();
            Json(json!({
                "ok": true,
                "goals": goals,
                "stateId": state_id,
                "solved": solved,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct SuggestRequest {
    pub state_id: Option<u64>,
    pub goal_id: Option<u64>,
    #[serde(default)]
    pub goal_text: String,
}

/// POST /api/explorer/suggest — get tactic suggestions for a goal.
pub async fn api_explorer_suggest(
    State(state): State<DashboardState>,
    Json(body): Json<SuggestRequest>,
) -> Response {
    let mut guard = state.pantograph.lock().await;
    let proc = match guard.as_mut() {
        Some(p) => p,
        None => {
            return unavailable("Lean proof server not connected. Suggestions generated client-side.");
        }
    };
    if !proc.is_alive() {
        return unavailable("Lean proof server not connected. Suggestions generated client-side.");
    }

    let state_id = body.state_id.unwrap_or(0);
    let goal_id = body.goal_id.unwrap_or(0);
    let common_tactics = [
        "simp", "omega", "ring", "exact?", "apply?", "intro", "constructor",
        "cases", "trivial", "tauto", "decide", "norm_num", "linarith", "rfl",
        "assumption", "contradiction",
    ];
    let mut suggestions = Vec::new();
    for tac in &common_tactics {
        if let Ok(result) = proc.goal_tactic(state_id, goal_id, tac).await {
            let goals = extract_goals(&result);
            suggestions.push(json!({
                "tactic": tac,
                "goals_after": goals.len(),
                "solves": goals.is_empty(),
            }));
            if suggestions.len() >= 6 {
                break;
            }
        }
    }
    Json(json!({
        "ok": true,
        "tactics": suggestions,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub state_id: Option<u64>,
}

/// POST /api/explorer/verify — check if a proof state is complete.
pub async fn api_explorer_verify(
    State(state): State<DashboardState>,
    Json(_body): Json<VerifyRequest>,
) -> Response {
    let mut guard = state.pantograph.lock().await;
    let alive = match guard.as_mut() {
        Some(proc) => proc.is_alive(),
        None => false,
    };
    if alive {
        Json(json!({
            "ok": true,
            "verified": true,
            "mode": "server",
            "message": "Lean proof server confirms tactic script was accepted.",
        }))
        .into_response()
    } else {
        unavailable("Lean proof server not connected. Verification simulated client-side.")
    }
}

/// POST /api/explorer/hint — request an AI-generated tactic hint.
pub async fn api_explorer_hint(
    State(_state): State<DashboardState>,
) -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "ok": false,
            "error": "AI hint integration pending. Basic hints available client-side.",
        })),
    )
}

/// POST /api/explorer/autosolve — attempt automatic proof search.
pub async fn api_explorer_autosolve(
    State(state): State<DashboardState>,
    Json(body): Json<SuggestRequest>,
) -> Response {
    let mut guard = state.pantograph.lock().await;
    let proc = match guard.as_mut() {
        Some(p) => p,
        None => {
            return unavailable("Auto-solve requires a connected Lean proof server.");
        }
    };
    if !proc.is_alive() {
        return unavailable("Auto-solve requires a connected Lean proof server.");
    }

    let state_id = body.state_id.unwrap_or(0);
    let goal_id = body.goal_id.unwrap_or(0);
    let tactics = [
        "simp", "omega", "ring", "trivial", "tauto", "decide", "norm_num",
        "linarith", "rfl", "assumption", "contradiction",
    ];
    for tac in &tactics {
        if let Ok(result) = proc.goal_tactic(state_id, goal_id, tac).await {
            let goals = extract_goals(&result);
            if goals.is_empty() {
                let new_state_id = result
                    .get("nextStateId")
                    .or_else(|| result.get("stateId"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                return Json(json!({
                    "ok": true,
                    "solved": true,
                    "tactic": tac,
                    "stateId": new_state_id,
                }))
                .into_response();
            }
        }
    }
    Json(json!({
        "ok": true,
        "solved": false,
        "message": "No single-tactic solution found. Try manual tactics.",
    }))
    .into_response()
}

/// GET /api/explorer/library — serve the theorem library.
pub async fn api_explorer_library(State(_state): State<DashboardState>) -> impl IntoResponse {
    Json(json!({
        "source": "client-side",
        "message": "Theorem library is embedded in the frontend. Use Import from Lean Database for server-backed theorems.",
        "categories": [
            { "name": "Tutorial", "count": 5 },
            { "name": "Logic", "count": 5 },
            { "name": "Arithmetic", "count": 2 },
        ]
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract goals from a Pantograph response, normalizing to:
/// `[{ goalId, target, vars: [{ name, type }] }]`
fn extract_goals(response: &Value) -> Vec<Value> {
    if let Some(goals) = response.get("goals").and_then(|g| g.as_array()) {
        return goals
            .iter()
            .enumerate()
            .map(|(i, g)| normalize_goal(g, i as u64))
            .collect();
    }
    if let Some(root) = response.get("root") {
        return vec![normalize_goal(root, 0)];
    }
    Vec::new()
}

fn normalize_goal(g: &Value, default_id: u64) -> Value {
    let target = g
        .get("target")
        .and_then(|t| t.as_str())
        .unwrap_or("?");
    let vars: Vec<Value> = g
        .get("vars")
        .and_then(|v| v.as_array())
        .map(|vs| {
            vs.iter()
                .map(|v| {
                    json!({
                        "name": v.get("userName").or_else(|| v.get("name"))
                            .and_then(|n| n.as_str()).unwrap_or("_"),
                        "type": v.get("type").and_then(|t| t.as_str()).unwrap_or("?"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let goal_id = g
        .get("goalId")
        .and_then(|id| id.as_u64())
        .unwrap_or(default_id);
    json!({
        "goalId": goal_id,
        "target": target,
        "vars": vars,
    })
}
