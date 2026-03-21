//! Proof Forge API — autoformalization & verification workbench.
//!
//! Endpoints under `/api/forge/*` for multi-modal math input → swarm
//! pipeline → Lean 4 formalization → verify / fix / prove → save.
//!
//! All agent spawning goes through `Orchestrator::launch_agent()` (F1).
//! All file I/O uses `spawn_blocking` (H5).
//! WebSocket for real-time pipeline updates (F2).

use super::gate_check;
use super::DashboardState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State as AxumState};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::{broadcast, Mutex};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_FIX_ITERATIONS: u32 = 5;
const PIPELINE_CHANNEL_CAPACITY: usize = 64;
const DEFAULT_AGENT_TIMEOUT_SECS: u64 = 120;
const MAX_HISTORY_ENTRIES: usize = 50;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single pipeline run tracking all swarm state.
#[derive(Clone, Debug)]
struct ForgePipeline {
    id: String,
    mode: String,
    status: PipelineStatus,
    phases: Vec<PipelinePhase>,
    tasks: Vec<ForgeTask>,
    lean_output: Option<String>,
    explanation: Option<String>,
    leader_agent_id: Option<String>,
    created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PipelineStatus {
    Running,
    Completed,
    Failed,
}

impl PipelineStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct PipelinePhase {
    name: String,
    status: String,
    agent_id: Option<String>,
    output: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ForgeTask {
    id: String,
    subject: String,
    status: String,
    agent_id: Option<String>,
}

/// Verification result returned by the verify agent.
#[derive(Clone, Debug, Serialize)]
struct VerificationResult {
    id: String,
    status: String,
    errors: Vec<LeanError>,
    sorry_locations: Vec<SorryLocation>,
    raw_output: String,
    agent_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LeanError {
    line: u32,
    column: u32,
    message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SorryLocation {
    line: u32,
    goal_state: String,
}

/// A history entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct HistoryEntry {
    id: String,
    mode: String,
    input_preview: String,
    output_preview: String,
    status: String,
    timestamp: u64,
}

/// Module-level pipeline store — shared across all handlers.
type PipelineStore = Arc<Mutex<HashMap<String, ForgePipeline>>>;
type VerificationStore = Arc<Mutex<HashMap<String, VerificationResult>>>;
type BroadcastStore = Arc<Mutex<HashMap<String, broadcast::Sender<String>>>>;

static PIPELINES: LazyLock<PipelineStore> = LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));
static VERIFICATIONS: LazyLock<VerificationStore> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));
static BROADCASTS: LazyLock<BroadcastStore> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<DashboardState> {
    Router::new()
        .route("/submit", post(api_forge_submit))
        .route("/pipeline/{id}/status", get(api_forge_pipeline_status))
        .route("/pipeline/{id}/output", get(api_forge_pipeline_output))
        .route("/pipeline/{id}/ws", get(ws_forge_pipeline))
        .route("/verify", post(api_forge_verify))
        .route("/verify/{id}/status", get(api_forge_verify_status))
        .route("/fix", post(api_forge_fix))
        .route("/prove", post(api_forge_prove))
        .route("/save", post(api_forge_save))
        .route("/templates", get(api_forge_templates))
        .route("/history", get(api_forge_history))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn api_err(code: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (code, Json(json!({"error": msg})))
}

fn local_orchestrator(
    state: &DashboardState,
) -> Result<Arc<crate::orchestrator::Orchestrator>, (StatusCode, Json<Value>)> {
    state.orchestrator.clone().ok_or_else(|| {
        api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Proof Forge requires a local orchestrator. Set AGENTHALO_ORCHESTRATOR_PROXY=0 or start without proxy mode.",
        )
    })
}

fn require_sensitive_access(state: &DashboardState) -> Result<(), (StatusCode, Json<Value>)> {
    let authenticated =
        crate::halo::auth::is_dashboard_authenticated(&state.credentials_path);
    if authenticated {
        Ok(())
    } else {
        Err(api_err(
            StatusCode::UNAUTHORIZED,
            "authentication required",
        ))
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn gen_id(prefix: &str) -> String {
    format!("{prefix}_{:x}", now_unix() ^ (std::process::id() as u64))
}

/// Build the list of pipeline phases for a given input mode.
fn phases_for_mode(mode: &str) -> Vec<PipelinePhase> {
    let names: Vec<&str> = match mode {
        "image" => vec!["ocr", "latex_parse", "formalize", "explain"],
        "latex" => vec!["latex_parse", "formalize", "explain"],
        "nl" => vec!["formalize", "explain"],
        "diagram" => vec!["ocr", "formalize", "explain"],
        "lean" => vec![], // goes directly to verify
        _ => vec!["formalize", "explain"],
    };
    names
        .into_iter()
        .map(|name| PipelinePhase {
            name: name.to_string(),
            status: "waiting".to_string(),
            agent_id: None,
            output: None,
        })
        .collect()
}

/// Send a pipeline update to all connected WebSocket clients.
async fn broadcast_pipeline_update(pipeline_id: &str, pipeline: &ForgePipeline) {
    let store = BROADCASTS.lock().await;
    if let Some(tx) = store.get(pipeline_id) {
        let msg = json!({
            "type": "pipeline_update",
            "pipeline_id": pipeline_id,
            "status": pipeline.status.as_str(),
            "phases": pipeline.phases,
            "tasks": pipeline.tasks,
        });
        let _ = tx.send(msg.to_string());
    }
}

/// Send an output update to all connected WebSocket clients.
async fn broadcast_output_update(pipeline_id: &str, lean_code: &str, explanation: &str) {
    let store = BROADCASTS.lock().await;
    if let Some(tx) = store.get(pipeline_id) {
        let msg = json!({
            "type": "output_update",
            "pipeline_id": pipeline_id,
            "lean_code": lean_code,
            "explanation": explanation,
        });
        let _ = tx.send(msg.to_string());
    }
}

/// Build the prompt for the leader/formalization agent.
fn build_formalization_prompt(mode: &str, content: &str, imports: &[String]) -> String {
    let import_block = if imports.is_empty() {
        String::new()
    } else {
        format!(
            "\nUse these Lean imports:\n```\n{}\n```\n",
            imports.join("\n")
        )
    };

    match mode {
        "image" => format!(
            "You are an autoformalization agent. The user has uploaded an image containing \
             mathematical content. The OCR output is:\n\n{content}\n\n\
             Translate this into well-typed Lean 4 code. Include:\n\
             1. All necessary imports\n\
             2. Type definitions if needed\n\
             3. Theorem statements with proof attempts\n\
             4. Use `sorry` only where you cannot immediately prove a goal\n\
             5. A brief natural-language explanation of what was formalized\n\
             {import_block}\n\
             Respond with two fenced code blocks:\n\
             ```lean\n<your Lean code>\n```\n\
             ```explanation\n<your explanation>\n```"
        ),
        "latex" => format!(
            "You are an autoformalization agent. Translate this LaTeX into Lean 4:\n\n\
             ```latex\n{content}\n```\n\n\
             Include all necessary imports, definitions, and theorem statements.\n\
             Use `sorry` only where you cannot immediately prove a goal.\n\
             {import_block}\n\
             Respond with two fenced code blocks:\n\
             ```lean\n<your Lean code>\n```\n\
             ```explanation\n<your explanation>\n```"
        ),
        "nl" => format!(
            "You are an autoformalization agent. Translate this natural-language \
             mathematical statement into Lean 4:\n\n{content}\n\n\
             Include all necessary imports, definitions, and theorem statements.\n\
             Use `sorry` only where you cannot immediately prove a goal.\n\
             {import_block}\n\
             Respond with two fenced code blocks:\n\
             ```lean\n<your Lean code>\n```\n\
             ```explanation\n<your explanation>\n```"
        ),
        "diagram" => format!(
            "You are an autoformalization agent. The user has uploaded a diagram. \
             The OCR/description output is:\n\n{content}\n\n\
             Translate the mathematical content of this diagram into Lean 4.\n\
             Use `sorry` only where you cannot immediately prove a goal.\n\
             {import_block}\n\
             Respond with two fenced code blocks:\n\
             ```lean\n<your Lean code>\n```\n\
             ```explanation\n<your explanation>\n```"
        ),
        _ => format!(
            "Formalize in Lean 4:\n\n{content}\n{import_block}\n\
             Respond with ```lean and ```explanation blocks."
        ),
    }
}

/// Build the prompt for the verification agent (F3: lean --run, NOT lake build).
fn build_verification_prompt(lean_code: &str) -> String {
    format!(
        "You are a Lean 4 verification agent. Do the following:\n\n\
         1. Save the following Lean code to /tmp/forge_verify.lean\n\
         2. Run: `lean --run /tmp/forge_verify.lean`\n\
         3. Report the compilation output VERBATIM (stdout + stderr)\n\
         4. Classify the result as one of:\n\
            - verified: zero errors, zero `sorry`\n\
            - partial: compiles but contains `sorry`\n\
            - failed: compilation errors\n\
         5. For each error, extract: line number, column, message\n\
         6. For each `sorry`, extract: line number, goal state (from error output)\n\n\
         IMPORTANT: Do NOT run `lake build`. Use `lean --run` only.\n\n\
         ```lean\n{lean_code}\n```\n\n\
         Respond with a JSON block:\n\
         ```json\n{{\n  \"status\": \"verified|partial|failed\",\n  \
         \"errors\": [{{\"line\": N, \"column\": N, \"message\": \"...\"}}],\n  \
         \"sorry_locations\": [{{\"line\": N, \"goal_state\": \"...\"}}],\n  \
         \"raw_output\": \"...\"\n}}\n```"
    )
}

/// Build the prompt for the fix agent.
fn build_fix_prompt(lean_code: &str, errors: &[String]) -> String {
    let error_list = errors
        .iter()
        .enumerate()
        .map(|(i, e)| format!("{}. {}", i + 1, e))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are a Lean 4 code repair agent. Fix the following compilation errors.\n\n\
         **Rules:**\n\
         - Do NOT use `sorry` or `admit`\n\
         - Preserve the mathematical intent of the code\n\
         - Fix only the errors — do not refactor unrelated code\n\n\
         **Current code:**\n```lean\n{lean_code}\n```\n\n\
         **Errors:**\n{error_list}\n\n\
         Respond with the corrected code in a single fenced block:\n\
         ```lean\n<fixed code>\n```"
    )
}

/// Build the prompt for the proof search agent.
fn build_prove_prompt(lean_code: &str, sorry_locations: &[SorryLocation]) -> String {
    let sorry_list = sorry_locations
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. Line {}: goal `{}`", i + 1, s.line, s.goal_state))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are a Lean 4 proof search agent. Fill in the `sorry` gaps in this code.\n\n\
         **Strategy (try in order):**\n\
         1. `omega`, `simp`, `aesop`, `decide`, `norm_num`\n\
         2. `exact?`, `apply?`\n\
         3. Construct a manual proof term\n\n\
         **Code:**\n```lean\n{lean_code}\n```\n\n\
         **Sorry locations:**\n{sorry_list}\n\n\
         Replace each `sorry` with a valid proof. Respond with the complete fixed code:\n\
         ```lean\n<code with sorry replaced>\n```"
    )
}

/// Parse Lean code from agent output (extract from ```lean ... ``` block).
fn extract_lean_block(output: &str) -> Option<String> {
    let marker = "```lean";
    let start = output.find(marker)?;
    let code_start = start + marker.len();
    // skip optional newline after marker
    let code_start = if output[code_start..].starts_with('\n') {
        code_start + 1
    } else {
        code_start
    };
    let end = output[code_start..].find("```")?;
    Some(output[code_start..code_start + end].trim_end().to_string())
}

/// Parse explanation from agent output.
fn extract_explanation_block(output: &str) -> Option<String> {
    let marker = "```explanation";
    let start = output.find(marker)?;
    let text_start = start + marker.len();
    let text_start = if output[text_start..].starts_with('\n') {
        text_start + 1
    } else {
        text_start
    };
    let end = output[text_start..].find("```")?;
    Some(output[text_start..text_start + end].trim_end().to_string())
}

/// Parse JSON result from verification agent output.
fn extract_json_block(output: &str) -> Option<Value> {
    let marker = "```json";
    let start = output.find(marker)?;
    let json_start = start + marker.len();
    let json_start = if output[json_start..].starts_with('\n') {
        json_start + 1
    } else {
        json_start
    };
    let end = output[json_start..].find("```")?;
    let json_str = &output[json_start..json_start + end];
    serde_json::from_str(json_str).ok()
}

/// Save a history entry to the history file.
async fn save_history_entry(entry: HistoryEntry) {
    let _ = tokio::task::spawn_blocking(move || {
        let history_path = crate::halo::config::halo_dir().join("forge_history.json");
        let mut entries: Vec<HistoryEntry> = if history_path.exists() {
            std::fs::read_to_string(&history_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        entries.insert(0, entry);
        entries.truncate(MAX_HISTORY_ENTRIES);
        let _ = std::fs::write(&history_path, serde_json::to_string_pretty(&entries).unwrap_or_default());
    })
    .await;
}

// ---------------------------------------------------------------------------
// POST /api/forge/submit
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ForgeSubmitRequest {
    mode: String,
    content: String,
    #[serde(default)]
    imports: Vec<String>,
}

async fn api_forge_submit(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<ForgeSubmitRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;

    // Validate mode
    let valid_modes = ["image", "latex", "nl", "diagram", "lean"];
    if !valid_modes.contains(&req.mode.as_str()) {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            &format!(
                "invalid mode '{}'. Expected one of: {}",
                req.mode,
                valid_modes.join(", ")
            ),
        ));
    }

    if req.content.trim().is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "content is empty"));
    }

    // For Lean mode, skip the pipeline — the content IS the Lean code
    if req.mode == "lean" {
        let pipeline_id = gen_id("forge");
        let pipeline = ForgePipeline {
            id: pipeline_id.clone(),
            mode: "lean".to_string(),
            status: PipelineStatus::Completed,
            phases: vec![],
            tasks: vec![],
            lean_output: Some(req.content.clone()),
            explanation: Some("Direct Lean input — ready for verification.".to_string()),
            leader_agent_id: None,
            created_at: now_unix(),
        };
        PIPELINES.lock().await.insert(pipeline_id.clone(), pipeline);
        return Ok(Json(json!({
            "pipeline_id": pipeline_id,
            "status": "completed",
            "mode": "lean",
            "lean_code": req.content,
            "explanation": "Direct Lean input — ready for verification.",
        })));
    }

    // Spawn leader agent via orchestrator (F1)
    let orchestrator = local_orchestrator(&state)?;
    let prompt = build_formalization_prompt(&req.mode, &req.content, &req.imports);
    let pipeline_id = gen_id("forge");

    // Create broadcast channel for this pipeline
    let (tx, _) = broadcast::channel(PIPELINE_CHANNEL_CAPACITY);
    BROADCASTS
        .lock()
        .await
        .insert(pipeline_id.clone(), tx.clone());

    // Build initial pipeline state
    let phases = phases_for_mode(&req.mode);
    let pipeline = ForgePipeline {
        id: pipeline_id.clone(),
        mode: req.mode.clone(),
        status: PipelineStatus::Running,
        phases: phases.clone(),
        tasks: vec![ForgeTask {
            id: gen_id("task"),
            subject: format!("Formalize {} input", req.mode),
            status: "pending".to_string(),
            agent_id: None,
        }],
        lean_output: None,
        explanation: None,
        leader_agent_id: None,
        created_at: now_unix(),
    };
    PIPELINES
        .lock()
        .await
        .insert(pipeline_id.clone(), pipeline.clone());

    // Broadcast initial state
    broadcast_pipeline_update(&pipeline_id, &pipeline).await;

    // Spawn agent asynchronously
    let pid = pipeline_id.clone();
    let input_preview = req.content.chars().take(80).collect::<String>();
    tokio::spawn(async move {
        // Launch formalization agent
        let launched = orchestrator
            .launch_agent(crate::orchestrator::LaunchAgentRequest {
                agent: "claude".to_string(),
                agent_name: format!("forge-formalize-{}", &pid[..8.min(pid.len())]),
                working_dir: None,
                env: std::collections::BTreeMap::new(),
                timeout_secs: DEFAULT_AGENT_TIMEOUT_SECS,
                model: None,
                trace: true,
                capabilities: vec!["lean".to_string(), "math".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await;

        let agent_id = match launched {
            Ok(ref a) => a.agent_id.clone(),
            Err(e) => {
                // Mark pipeline as failed
                let mut store = PIPELINES.lock().await;
                if let Some(p) = store.get_mut(&pid) {
                    p.status = PipelineStatus::Failed;
                    if let Some(task) = p.tasks.first_mut() {
                        task.status = "failed".to_string();
                    }
                    broadcast_pipeline_update(&pid, p).await;
                }
                eprintln!("[forge] agent launch failed: {e}");
                return;
            }
        };

        // Update pipeline with agent ID
        {
            let mut store = PIPELINES.lock().await;
            if let Some(p) = store.get_mut(&pid) {
                p.leader_agent_id = Some(agent_id.clone());
                if let Some(task) = p.tasks.first_mut() {
                    task.agent_id = Some(agent_id.clone());
                    task.status = "in_progress".to_string();
                }
                // Mark first phase as active
                if let Some(phase) = p.phases.first_mut() {
                    phase.status = "active".to_string();
                    phase.agent_id = Some(agent_id.clone());
                }
                broadcast_pipeline_update(&pid, p).await;
            }
        }

        // Send task and wait for result
        let task_result = orchestrator
            .send_task(crate::orchestrator::SendTaskRequest {
                agent_id: agent_id.clone(),
                task: prompt,
                timeout_secs: Some(DEFAULT_AGENT_TIMEOUT_SECS),
                wait: true,
            })
            .await;

        match task_result {
            Ok(task) => {
                let output = task.result.unwrap_or_default();
                let lean_code = extract_lean_block(&output);
                let explanation = extract_explanation_block(&output);

                let mut store = PIPELINES.lock().await;
                if let Some(p) = store.get_mut(&pid) {
                    if let Some(code) = &lean_code {
                        p.lean_output = Some(code.clone());
                        p.status = PipelineStatus::Completed;
                    } else {
                        // Agent responded but no lean block found — use raw output
                        p.lean_output = Some(output.clone());
                        p.status = PipelineStatus::Completed;
                    }
                    p.explanation = explanation.or(Some("Formalization complete.".to_string()));

                    // Mark all phases complete
                    for phase in &mut p.phases {
                        phase.status = "complete".to_string();
                    }
                    if let Some(task) = p.tasks.first_mut() {
                        task.status = "completed".to_string();
                    }

                    // Broadcast output
                    broadcast_output_update(
                        &pid,
                        p.lean_output.as_deref().unwrap_or(""),
                        p.explanation.as_deref().unwrap_or(""),
                    )
                    .await;
                    broadcast_pipeline_update(&pid, p).await;
                }

                // Save history
                save_history_entry(HistoryEntry {
                    id: pid.clone(),
                    mode: "formalize".to_string(),
                    input_preview,
                    output_preview: lean_code
                        .as_deref()
                        .unwrap_or(&output)
                        .chars()
                        .take(80)
                        .collect(),
                    status: "completed".to_string(),
                    timestamp: now_unix(),
                })
                .await;
            }
            Err(e) => {
                let mut store = PIPELINES.lock().await;
                if let Some(p) = store.get_mut(&pid) {
                    p.status = PipelineStatus::Failed;
                    for phase in &mut p.phases {
                        if phase.status == "waiting" || phase.status == "active" {
                            phase.status = "failed".to_string();
                        }
                    }
                    if let Some(task) = p.tasks.first_mut() {
                        task.status = "failed".to_string();
                    }
                    broadcast_pipeline_update(&pid, p).await;
                }
                eprintln!("[forge] formalization task failed: {e}");
            }
        }

        // Stop the agent after completion
        let _ = orchestrator
            .stop_agent(crate::orchestrator::StopRequest {
                agent_id,
                force: false,
                purge: false,
            })
            .await;
    });

    Ok(Json(json!({
        "pipeline_id": pipeline_id,
        "status": "running",
        "mode": req.mode,
    })))
}

// ---------------------------------------------------------------------------
// POST /api/forge/verify
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ForgeVerifyRequest {
    lean_code: String,
    #[serde(default)]
    imports: Vec<String>,
}

async fn api_forge_verify(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<ForgeVerifyRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;

    if req.lean_code.trim().is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "lean_code is empty"));
    }

    let orchestrator = local_orchestrator(&state)?;
    let verification_id = gen_id("verify");
    let prompt = build_verification_prompt(&req.lean_code);

    // Launch verification agent (F1)
    let launched = orchestrator
        .launch_agent(crate::orchestrator::LaunchAgentRequest {
            agent: "claude".to_string(),
            agent_name: format!(
                "forge-verify-{}",
                &verification_id[..8.min(verification_id.len())]
            ),
            working_dir: None,
            env: std::collections::BTreeMap::new(),
            timeout_secs: DEFAULT_AGENT_TIMEOUT_SECS,
            model: None,
            trace: true,
            capabilities: vec!["lean".to_string(), "verify".to_string()],
            dispatch_mode: None,
            container_hookup: None,
        })
        .await
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;

    let agent_id = launched.agent_id.clone();
    let vid = verification_id.clone();

    // Run verification asynchronously
    tokio::spawn(async move {
        let task_result = orchestrator
            .send_task(crate::orchestrator::SendTaskRequest {
                agent_id: agent_id.clone(),
                task: prompt,
                timeout_secs: Some(DEFAULT_AGENT_TIMEOUT_SECS),
                wait: true,
            })
            .await;

        let result = match task_result {
            Ok(task) => {
                let output = task.result.unwrap_or_default();

                // Try to parse structured JSON from agent output
                if let Some(parsed) = extract_json_block(&output) {
                    let status = parsed["status"].as_str().unwrap_or("failed").to_string();
                    let errors: Vec<LeanError> = parsed["errors"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                                .collect()
                        })
                        .unwrap_or_default();
                    let sorry_locations: Vec<SorryLocation> = parsed["sorry_locations"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                                .collect()
                        })
                        .unwrap_or_default();
                    let raw_output = parsed["raw_output"]
                        .as_str()
                        .unwrap_or(&output)
                        .to_string();

                    VerificationResult {
                        id: vid.clone(),
                        status,
                        errors,
                        sorry_locations,
                        raw_output,
                        agent_id: agent_id.clone(),
                    }
                } else {
                    // Fallback: try to infer status from raw output
                    let has_sorry = output.contains("sorry");
                    let has_error = output.contains("error:")
                        || output.contains("Error:")
                        || output.contains("unknown identifier");
                    let status = if has_error {
                        "failed"
                    } else if has_sorry {
                        "partial"
                    } else {
                        "verified"
                    };

                    VerificationResult {
                        id: vid.clone(),
                        status: status.to_string(),
                        errors: vec![],
                        sorry_locations: vec![],
                        raw_output: output,
                        agent_id: agent_id.clone(),
                    }
                }
            }
            Err(e) => VerificationResult {
                id: vid.clone(),
                status: "failed".to_string(),
                errors: vec![LeanError {
                    line: 0,
                    column: 0,
                    message: format!("Agent task failed: {e}"),
                }],
                sorry_locations: vec![],
                raw_output: e.to_string(),
                agent_id: agent_id.clone(),
            },
        };

        VERIFICATIONS.lock().await.insert(vid, result);

        // Stop the agent
        let _ = orchestrator
            .stop_agent(crate::orchestrator::StopRequest {
                agent_id,
                force: false,
                purge: false,
            })
            .await;
    });

    Ok(Json(json!({
        "verification_id": verification_id,
        "agent_id": launched.agent_id,
        "status": "running",
    })))
}

// ---------------------------------------------------------------------------
// POST /api/forge/fix
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ForgeFixRequest {
    lean_code: String,
    errors: Vec<String>,
    iteration: u32,
}

async fn api_forge_fix(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<ForgeFixRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;

    // F6: cap fix iterations
    if req.iteration >= MAX_FIX_ITERATIONS {
        return Ok(Json(json!({
            "status": "exhausted",
            "message": format!(
                "Maximum fix iterations ({MAX_FIX_ITERATIONS}) reached. \
                 Edit the Lean code manually or try a different approach."
            ),
            "iteration": req.iteration,
        })));
    }

    let orchestrator = local_orchestrator(&state)?;
    let prompt = build_fix_prompt(&req.lean_code, &req.errors);

    // Launch fix agent
    let launched = orchestrator
        .launch_agent(crate::orchestrator::LaunchAgentRequest {
            agent: "claude".to_string(),
            agent_name: format!("forge-fix-iter{}", req.iteration),
            working_dir: None,
            env: std::collections::BTreeMap::new(),
            timeout_secs: DEFAULT_AGENT_TIMEOUT_SECS,
            model: None,
            trace: true,
            capabilities: vec!["lean".to_string(), "repair".to_string()],
            dispatch_mode: None,
            container_hookup: None,
        })
        .await
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;

    // Send task synchronously (wait: true) since the user is waiting for the fix
    let task = orchestrator
        .send_task(crate::orchestrator::SendTaskRequest {
            agent_id: launched.agent_id.clone(),
            task: prompt,
            timeout_secs: Some(DEFAULT_AGENT_TIMEOUT_SECS),
            wait: true,
        })
        .await
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, &e))?;

    let output = task.result.unwrap_or_default();
    let fixed_code = extract_lean_block(&output).unwrap_or(output.clone());

    // Stop the agent
    let _ = orchestrator
        .stop_agent(crate::orchestrator::StopRequest {
            agent_id: launched.agent_id.clone(),
            force: false,
            purge: false,
        })
        .await;

    Ok(Json(json!({
        "status": "fixed",
        "lean_code": fixed_code,
        "iteration": req.iteration + 1,
        "agent_id": launched.agent_id,
    })))
}

// ---------------------------------------------------------------------------
// POST /api/forge/prove
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ForgeProveRequest {
    lean_code: String,
    sorry_locations: Vec<SorryLocation>,
}

async fn api_forge_prove(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<ForgeProveRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;

    if req.sorry_locations.is_empty() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "no sorry_locations provided",
        ));
    }

    let orchestrator = local_orchestrator(&state)?;
    let prompt = build_prove_prompt(&req.lean_code, &req.sorry_locations);

    // Launch proof search agent
    let launched = orchestrator
        .launch_agent(crate::orchestrator::LaunchAgentRequest {
            agent: "claude".to_string(),
            agent_name: format!("forge-prove-{}", req.sorry_locations.len()),
            working_dir: None,
            env: std::collections::BTreeMap::new(),
            timeout_secs: DEFAULT_AGENT_TIMEOUT_SECS,
            model: None,
            trace: true,
            capabilities: vec!["lean".to_string(), "prove".to_string()],
            dispatch_mode: None,
            container_hookup: None,
        })
        .await
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;

    let task = orchestrator
        .send_task(crate::orchestrator::SendTaskRequest {
            agent_id: launched.agent_id.clone(),
            task: prompt,
            timeout_secs: Some(DEFAULT_AGENT_TIMEOUT_SECS),
            wait: true,
        })
        .await
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, &e))?;

    let output = task.result.unwrap_or_default();
    let proved_code = extract_lean_block(&output).unwrap_or(output.clone());

    // Count remaining sorrys
    let remaining_sorrys = proved_code.matches("sorry").count();

    let _ = orchestrator
        .stop_agent(crate::orchestrator::StopRequest {
            agent_id: launched.agent_id.clone(),
            force: false,
            purge: false,
        })
        .await;

    Ok(Json(json!({
        "status": if remaining_sorrys == 0 { "proved" } else { "partial" },
        "lean_code": proved_code,
        "remaining_sorrys": remaining_sorrys,
        "agent_id": launched.agent_id,
    })))
}

// ---------------------------------------------------------------------------
// POST /api/forge/save
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[allow(dead_code)]
struct ForgeSaveRequest {
    lean_code: String,
    file_path: String,
    #[serde(default)]
    repo_id: Option<String>,
}

async fn api_forge_save(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<ForgeSaveRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;

    if req.lean_code.trim().is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "lean_code is empty"));
    }
    if req.file_path.trim().is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "file_path is empty"));
    }

    // Resolve workspace root
    let workspace_root = super::editor_api::resolve_workspace_root(None)
        .map_err(|(_status, body)| {
            api_err(
                StatusCode::BAD_REQUEST,
                body.0["error"].as_str().unwrap_or("cannot resolve workspace root"),
            )
        })?;

    // Check worktree gate (F7)
    let config = gate_check::load_config(&workspace_root);
    if let Err(gate_err) = gate_check::check_worktree_gate(&workspace_root, &config) {
        return Err(api_err(
            StatusCode::FORBIDDEN,
            &format!(
                "CodeGuard worktree gate blocked save: {}",
                gate_err.message
            ),
        ));
    }

    // Build the full file path
    let full_path = workspace_root.join(&req.file_path);

    // Ensure parent directory exists (H5: spawn_blocking for file I/O)
    let lean_code = req.lean_code.clone();
    let fp = full_path.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = fp.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&fp, lean_code.as_bytes())?;
        Ok::<_, std::io::Error>(())
    })
    .await
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, &format!("join error: {e}")))?
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, &format!("write error: {e}")))?;

    // Check if CodeGuard manifest exists (H3: do NOT write to lock directly)
    let manifest_exists = gate_check::find_manifest(&workspace_root).is_some();
    let proposed_node = if manifest_exists {
        Some(json!({
            "action": "propose_manifest_update",
            "file_path": req.file_path,
            "note": "File saved. Use CodeGuard page to review and approve manifest update."
        }))
    } else {
        None
    };

    Ok(Json(json!({
        "ok": true,
        "file_path": full_path.display().to_string(),
        "manifest_exists": manifest_exists,
        "proposed_node": proposed_node,
        "message": "File saved. Review in CodeGuard if manifest update is needed.",
    })))
}

// ---------------------------------------------------------------------------
// WebSocket handler for pipeline updates (F2)
// ---------------------------------------------------------------------------

async fn ws_forge_pipeline(
    ws: WebSocketUpgrade,
    Path(pipeline_id): Path<String>,
    AxumState(_state): AxumState<DashboardState>,
) -> Response {
    // Check pipeline exists
    let store = PIPELINES.lock().await;
    if !store.contains_key(&pipeline_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "pipeline not found"})),
        )
            .into_response();
    }
    drop(store);

    // Get or create broadcast channel
    let rx = {
        let mut broadcasts = BROADCASTS.lock().await;
        let tx = broadcasts
            .entry(pipeline_id.clone())
            .or_insert_with(|| broadcast::channel(PIPELINE_CHANNEL_CAPACITY).0);
        tx.subscribe()
    };

    ws.on_upgrade(move |socket| handle_forge_ws(socket, rx))
        .into_response()
}

async fn handle_forge_ws(
    socket: WebSocket,
    mut rx: broadcast::Receiver<String>,
) {
    let (mut tx, mut incoming) = futures_util::StreamExt::split(socket);
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if tx.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            inbound = futures_util::StreamExt::next(&mut incoming) => {
                match inbound {
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = tx.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {} // ignore other messages
                }
            }
        }
    }
    let _ = tx.close().await;
}

// ---------------------------------------------------------------------------
// GET endpoints
// ---------------------------------------------------------------------------

async fn api_forge_pipeline_status(
    Path(id): Path<String>,
    AxumState(_state): AxumState<DashboardState>,
) -> ApiResult {
    let store = PIPELINES.lock().await;
    let pipeline = store
        .get(&id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, "pipeline not found"))?;
    Ok(Json(json!({
        "pipeline_id": pipeline.id,
        "mode": pipeline.mode,
        "status": pipeline.status.as_str(),
        "phases": pipeline.phases,
        "tasks": pipeline.tasks,
        "leader_agent_id": pipeline.leader_agent_id,
        "created_at": pipeline.created_at,
    })))
}

async fn api_forge_pipeline_output(
    Path(id): Path<String>,
    AxumState(_state): AxumState<DashboardState>,
) -> ApiResult {
    let store = PIPELINES.lock().await;
    let pipeline = store
        .get(&id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, "pipeline not found"))?;
    Ok(Json(json!({
        "pipeline_id": pipeline.id,
        "lean_code": pipeline.lean_output,
        "explanation": pipeline.explanation,
        "status": pipeline.status.as_str(),
    })))
}

async fn api_forge_verify_status(
    Path(id): Path<String>,
    AxumState(_state): AxumState<DashboardState>,
) -> ApiResult {
    let store = VERIFICATIONS.lock().await;
    if let Some(result) = store.get(&id) {
        Ok(Json(json!({
            "verification_id": result.id,
            "status": result.status,
            "errors": result.errors,
            "sorry_locations": result.sorry_locations,
            "raw_output": result.raw_output,
            "agent_id": result.agent_id,
        })))
    } else {
        Ok(Json(json!({
            "verification_id": id,
            "status": "running",
        })))
    }
}

async fn api_forge_templates(
    AxumState(_state): AxumState<DashboardState>,
) -> Json<Value> {
    Json(json!({
        "templates": [
            {"id": "theorem", "name": "Prove a property", "mode": "nl",
             "content": "Prove that [property] holds for [object]"},
            {"id": "define", "name": "Define a structure", "mode": "nl",
             "content": "Define [concept] as a structure with [fields]"},
            {"id": "textbook", "name": "Formalize from textbook", "mode": "image",
             "content": ""},
            {"id": "latex_eq", "name": "LaTeX equation to Lean", "mode": "latex",
             "content": "\\forall x : \\mathbb{N}, x + 0 = x"},
            {"id": "quick_verify", "name": "Quick verification", "mode": "lean",
             "content": "theorem test : 1 + 1 = 2 := by norm_num"},
            {"id": "ocr_formalize", "name": "OCR and formalize", "mode": "image",
             "content": ""},
            {"id": "nucleus", "name": "Nucleus property", "mode": "latex",
             "content": "\\forall x, R(x) \\leq x \\land R(R(x)) = R(x)"},
            {"id": "sky_combinator", "name": "SKY combinator reduction", "mode": "lean",
             "content": "-- SKY combinator reduction proof\nexample : SKY.reduce (SKY.app (SKY.S) (SKY.K) (SKY.Y)) = SKY.Y := by\n  sorry"},
            {"id": "blueprint_node", "name": "Blueprint node", "mode": "lean",
             "content": "@[blueprint]\ntheorem my_theorem : True := by\n  trivial"}
        ]
    }))
}

async fn api_forge_history(
    AxumState(_state): AxumState<DashboardState>,
) -> Json<Value> {
    let entries: Vec<HistoryEntry> = tokio::task::spawn_blocking(|| {
        let history_path = crate::halo::config::halo_dir().join("forge_history.json");
        if history_path.exists() {
            std::fs::read_to_string(&history_path)
                .ok()
                .and_then(|s| serde_json::from_str::<Vec<HistoryEntry>>(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    })
    .await
    .unwrap_or_default();

    Json(json!({ "history": entries }))
}
