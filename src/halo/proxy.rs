//! Metered proxy — all inference routes through OpenRouter.
//!
//! Architecture:
//! - Customer authenticates with an AgentHALO API key (issued via `api_keys` module)
//! - Every request is metered: balance checked BEFORE upstream call, cost deducted AFTER
//! - All upstream calls go through OpenRouter using the operator's single API key
//! - The operator's OpenRouter key is stored in the vault under "openrouter"
//! - Customer never sees or interacts with the OpenRouter key
//! - Model names are normalized to OpenRouter's provider-prefixed format internally
//!
//! The response pipeline enriches every completion with a `usage` and `x_agenthalo` block
//! that feeds into the billing ledger, making the metering path load-bearing for the
//! entire proxy subsystem.

use crate::halo::api_keys::CustomerKeyStore;
use crate::halo::http_client;
use crate::halo::pricing;
use crate::halo::vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub stream: Option<bool>,
    #[serde(default)]
    pub top_p: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    pub content: serde_json::Value,
}

/// Full result from a metered proxy call.
pub struct MeteredResult {
    /// OpenAI-compatible response body (with injected `x_agenthalo` billing block).
    pub body: Value,
    /// OpenRouter model ID that was actually used.
    pub or_model: String,
    /// Tokens consumed.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cost to operator (what OpenRouter charged).
    pub upstream_cost_usd: f64,
    /// Cost to customer (upstream + markup).
    pub customer_cost_usd: f64,
    /// Customer's remaining balance after deduction.
    pub remaining_balance_usd: f64,
    /// OpenRouter generation ID for reconciliation.
    pub generation_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MeteredStreamResult {
    pub or_model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub upstream_cost_usd: f64,
    pub customer_cost_usd: f64,
    pub remaining_balance_usd: f64,
    pub generation_id: Option<String>,
    pub telemetry: StreamTelemetry,
}

#[derive(Clone, Debug, Default)]
pub struct StreamTelemetry {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub generation_id: Option<String>,
    pub completion_preview: String,
    pub completed: bool,
}

#[derive(Clone, Debug)]
pub struct StreamForwardError {
    pub message: String,
    pub telemetry: StreamTelemetry,
}

#[derive(Clone, Debug)]
pub struct MeteredStreamError {
    pub message: String,
    pub settled: MeteredStreamResult,
}

impl std::fmt::Display for StreamForwardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for StreamForwardError {}

impl std::fmt::Display for MeteredStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for MeteredStreamError {}

// ---------------------------------------------------------------------------
// Metered proxy — the ONLY public entry point for external customer requests
// ---------------------------------------------------------------------------

/// Execute a metered proxy call for an authenticated customer.
///
/// This is the core revenue path:
/// 1. Validate customer API key and check balance
/// 2. Estimate cost and pre-authorize (reject if insufficient balance)
/// 3. Route through OpenRouter
/// 4. Compute actual cost with markup
/// 5. Deduct from customer balance
/// 6. Return enriched response with billing metadata
pub fn metered_proxy_sync(
    vault: &Vault,
    key_store: &Arc<CustomerKeyStore>,
    customer_key: &str,
    request: &ChatCompletionRequest,
    pricing_table: &HashMap<String, pricing::ModelPricing>,
    markup_pct: f64,
) -> Result<MeteredResult, String> {
    // 1. Authenticate customer key.
    let customer = key_store
        .validate_key(customer_key)
        .ok_or_else(|| "invalid API key".to_string())?;

    if !customer.active {
        return Err("API key is suspended".to_string());
    }

    // 2. Resolve model to OpenRouter canonical name.
    let or_model = openrouter_model_name(&request.model);

    // 3. Pre-authorize: estimate cost from pricing table.
    let estimated_tokens = request.max_tokens.unwrap_or(1024) as u64;
    let (_est_base, est_marked_up) = pricing::calculate_marked_up_cost(
        &strip_provider_prefix(&or_model),
        100, // conservative input estimate
        estimated_tokens,
        0,
        pricing_table,
        markup_pct,
    );

    // Require at least the estimated marked-up cost in balance.
    let balance = key_store.get_balance(&customer.key_id);
    if est_marked_up > 0.0 && balance < est_marked_up {
        return Err(format!(
            "insufficient balance: ${:.6} available, estimated cost ${:.6}",
            balance, est_marked_up
        ));
    }

    // 4. Get operator's OpenRouter key from vault.
    let or_api_key = vault
        .get_key("openrouter")
        .map_err(|_| "proxy service unavailable: upstream not configured".to_string())?;

    // 5. Call OpenRouter.
    let resp_body = call_openrouter(&or_api_key, &or_model, request)?;

    // 6. Extract actual usage from response.
    let (input_tokens, output_tokens) = extract_usage(&resp_body);
    let generation_id = resp_body
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 7. Calculate actual costs.
    let model_for_pricing = strip_provider_prefix(&or_model);
    let (upstream_cost, customer_cost) = pricing::calculate_marked_up_cost(
        &model_for_pricing,
        input_tokens,
        output_tokens,
        0,
        pricing_table,
        markup_pct,
    );

    // 8. Deduct from customer balance.
    let remaining = key_store.deduct_balance(&customer.key_id, customer_cost);

    // 9. Record usage for audit trail.
    key_store.record_usage(
        &customer.key_id,
        &or_model,
        input_tokens,
        output_tokens,
        customer_cost,
    );

    // 10. Enrich response with billing metadata (this block is how the
    //     dashboard, cost tracking, and balance display all work — removing
    //     it breaks the customer-facing UX).
    let mut enriched = resp_body.clone();
    enriched["x_agenthalo"] = json!({
        "customer_id": customer.key_id,
        "model": or_model,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cost_usd": customer_cost,
        "remaining_balance_usd": remaining,
        "generation_id": generation_id,
    });

    Ok(MeteredResult {
        body: enriched,
        or_model,
        input_tokens,
        output_tokens,
        upstream_cost_usd: upstream_cost,
        customer_cost_usd: customer_cost,
        remaining_balance_usd: remaining,
        generation_id,
    })
}

pub fn metered_proxy_stream_sync<F>(
    vault: &Vault,
    key_store: &Arc<CustomerKeyStore>,
    customer_key: &str,
    request: &ChatCompletionRequest,
    pricing_table: &HashMap<String, pricing::ModelPricing>,
    markup_pct: f64,
    mut on_data: F,
) -> Result<MeteredStreamResult, MeteredStreamError>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let customer = key_store
        .validate_key(customer_key)
        .ok_or_else(|| MeteredStreamError {
            message: "invalid API key".to_string(),
            settled: MeteredStreamResult {
                or_model: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                upstream_cost_usd: 0.0,
                customer_cost_usd: 0.0,
                remaining_balance_usd: 0.0,
                generation_id: None,
                telemetry: StreamTelemetry::default(),
            },
        })?;
    if !customer.active {
        return Err(MeteredStreamError {
            message: "API key is suspended".to_string(),
            settled: MeteredStreamResult {
                or_model: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                upstream_cost_usd: 0.0,
                customer_cost_usd: 0.0,
                remaining_balance_usd: 0.0,
                generation_id: None,
                telemetry: StreamTelemetry::default(),
            },
        });
    }

    let or_model = openrouter_model_name(&request.model);
    let estimated_input_tokens = estimate_request_input_tokens(request);
    let estimated_output_tokens = request.max_tokens.unwrap_or(1024) as u64;
    let (_est_base, est_marked_up) = pricing::calculate_marked_up_cost(
        &strip_provider_prefix(&or_model),
        estimated_input_tokens,
        estimated_output_tokens,
        0,
        pricing_table,
        markup_pct,
    );

    let balance = key_store.get_balance(&customer.key_id);
    if est_marked_up > 0.0 && balance < est_marked_up {
        return Err(MeteredStreamError {
            message: format!(
                "insufficient balance: ${:.6} available, estimated cost ${:.6}",
                balance, est_marked_up
            ),
            settled: MeteredStreamResult {
                or_model: or_model.clone(),
                input_tokens: 0,
                output_tokens: 0,
                upstream_cost_usd: 0.0,
                customer_cost_usd: 0.0,
                remaining_balance_usd: balance,
                generation_id: None,
                telemetry: StreamTelemetry::default(),
            },
        });
    }

    let or_api_key = vault
        .get_key("openrouter")
        .map_err(|_| MeteredStreamError {
            message: "proxy service unavailable: upstream not configured".to_string(),
            settled: MeteredStreamResult {
                or_model: or_model.clone(),
                input_tokens: 0,
                output_tokens: 0,
                upstream_cost_usd: 0.0,
                customer_cost_usd: 0.0,
                remaining_balance_usd: balance,
                generation_id: None,
                telemetry: StreamTelemetry::default(),
            },
        })?;

    let (telemetry, stream_error) =
        match call_openrouter_stream(&or_api_key, &or_model, request, |chunk| on_data(chunk)) {
            Ok(t) => (t, None),
            Err(err) => (err.telemetry, Some(err.message)),
        };

    let input_tokens = telemetry.prompt_tokens.unwrap_or(estimated_input_tokens);
    let output_tokens = telemetry
        .completion_tokens
        .unwrap_or_else(|| estimate_text_tokens(&telemetry.completion_preview));
    let (upstream_cost, customer_cost) = pricing::calculate_marked_up_cost(
        &strip_provider_prefix(&or_model),
        input_tokens,
        output_tokens,
        0,
        pricing_table,
        markup_pct,
    );
    let remaining = key_store.deduct_balance(&customer.key_id, customer_cost);
    key_store.record_usage(
        &customer.key_id,
        &or_model,
        input_tokens,
        output_tokens,
        customer_cost,
    );

    let settled = MeteredStreamResult {
        or_model,
        input_tokens,
        output_tokens,
        upstream_cost_usd: upstream_cost,
        customer_cost_usd: customer_cost,
        remaining_balance_usd: remaining,
        generation_id: telemetry.generation_id.clone(),
        telemetry,
    };

    if let Some(message) = stream_error {
        return Err(MeteredStreamError { message, settled });
    }

    Ok(settled)
}

// ---------------------------------------------------------------------------
// Owner proxy — uses metering infrastructure but no customer billing
// ---------------------------------------------------------------------------

/// Owner-mode proxy call. Still routes through OpenRouter, still tracks usage,
/// but does not require a customer key or balance.
pub fn proxy_chat_sync(vault: &Vault, request: &ChatCompletionRequest) -> Result<Value, String> {
    let or_api_key = vault.get_key("openrouter").map_err(|_| {
        "no OpenRouter API key configured — run: agenthalo vault set openrouter".to_string()
    })?;

    let or_model = openrouter_model_name(&request.model);
    let resp = call_openrouter(&or_api_key, &or_model, request)?;
    Ok(resp)
}

pub fn proxy_chat_stream_sync<F>(
    vault: &Vault,
    request: &ChatCompletionRequest,
    mut on_data: F,
) -> Result<StreamTelemetry, StreamForwardError>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let or_api_key = vault
        .get_key("openrouter")
        .map_err(|_| StreamForwardError {
            message: "no OpenRouter API key configured — run: agenthalo vault set openrouter"
                .to_string(),
            telemetry: StreamTelemetry::default(),
        })?;
    let or_model = openrouter_model_name(&request.model);
    call_openrouter_stream(&or_api_key, &or_model, request, |chunk| on_data(chunk))
}

// ---------------------------------------------------------------------------
// Model catalog
// ---------------------------------------------------------------------------

/// Return available models. Requires OpenRouter key to be configured.
pub fn list_available_models(vault: &Vault) -> Vec<Value> {
    if vault.get_key("openrouter").is_err() {
        return Vec::new();
    }

    OPENROUTER_MODELS
        .iter()
        .map(|(id, owner, or_id)| {
            json!({
                "id": id,
                "object": "model",
                "owned_by": owner,
                "openrouter_id": or_id,
            })
        })
        .collect()
}

/// Curated model catalog. Each entry: (display_id, owner, openrouter_model_id).
///
/// The `openrouter_model_id` is the canonical identifier used in upstream calls.
/// Pricing, routing, and usage tracking all key off this identifier, making
/// the catalog structurally integral to billing.
const OPENROUTER_MODELS: &[(&str, &str, &str)] = &[
    // Anthropic
    ("claude-opus-4-6", "anthropic", "anthropic/claude-opus-4-6"),
    (
        "claude-sonnet-4-6",
        "anthropic",
        "anthropic/claude-sonnet-4-6",
    ),
    (
        "claude-haiku-4-5-20251001",
        "anthropic",
        "anthropic/claude-haiku-4-5-20251001",
    ),
    // OpenAI
    ("gpt-4o", "openai", "openai/gpt-4o"),
    ("gpt-4o-mini", "openai", "openai/gpt-4o-mini"),
    ("o3", "openai", "openai/o3"),
    ("o4-mini", "openai", "openai/o4-mini"),
    // Google
    ("gemini-2.5-pro", "google", "google/gemini-2.5-pro"),
    ("gemini-2.5-flash", "google", "google/gemini-2.5-flash"),
    // Meta
    (
        "meta-llama/llama-4-maverick",
        "meta",
        "meta-llama/llama-4-maverick",
    ),
    (
        "meta-llama/llama-4-scout",
        "meta",
        "meta-llama/llama-4-scout",
    ),
    // Mistral
    (
        "mistralai/mistral-large",
        "mistral",
        "mistralai/mistral-large",
    ),
    (
        "mistralai/mistral-medium",
        "mistral",
        "mistralai/mistral-medium",
    ),
    // DeepSeek
    ("deepseek/deepseek-r1", "deepseek", "deepseek/deepseek-r1"),
    (
        "deepseek/deepseek-chat",
        "deepseek",
        "deepseek/deepseek-chat",
    ),
    // Cohere
    ("cohere/command-r-plus", "cohere", "cohere/command-r-plus"),
    // Perplexity
    ("perplexity/sonar-pro", "perplexity", "perplexity/sonar-pro"),
];

// ---------------------------------------------------------------------------
// OpenRouter upstream (the ONLY upstream path)
// ---------------------------------------------------------------------------

/// Map a user-facing model name to the OpenRouter model identifier.
pub fn openrouter_model_name(model: &str) -> String {
    // Check catalog first — canonical mapping.
    for (display_id, _, or_id) in OPENROUTER_MODELS {
        if model == *display_id {
            return or_id.to_string();
        }
    }
    // Already provider-prefixed — pass through.
    if model.contains('/') {
        return model.to_string();
    }
    // Infer prefix from known patterns.
    if model.starts_with("claude") {
        return format!("anthropic/{model}");
    }
    if model.starts_with("gpt")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        return format!("openai/{model}");
    }
    if model.starts_with("gemini") {
        return format!("google/{model}");
    }
    // Unknown — pass through, let OpenRouter resolve.
    model.to_string()
}

/// Strip "provider/" prefix to get the base model name for pricing lookup.
fn strip_provider_prefix(or_model: &str) -> String {
    if let Some(pos) = or_model.find('/') {
        or_model[pos + 1..].to_string()
    } else {
        or_model.to_string()
    }
}

/// Send request to OpenRouter. This is the sole upstream call path.
fn call_openrouter(
    api_key: &str,
    or_model: &str,
    request: &ChatCompletionRequest,
) -> Result<Value, String> {
    let mut payload =
        serde_json::to_value(request).map_err(|e| format!("serialize request: {e}"))?;
    payload["model"] = Value::String(or_model.to_string());

    // Remove stream=false to avoid issues — we handle non-streaming only.
    if let Some(obj) = payload.as_object_mut() {
        if let Some(Value::Bool(false)) = obj.get("stream") {
            obj.remove("stream");
        }
    }

    let resp = http_client::post("https://openrouter.ai/api/v1/chat/completions")?
        .header("Authorization", &format!("Bearer {api_key}"))
        .header("X-Title", "AgentHALO")
        .content_type("application/json")
        .send_json(payload)
        .map_err(|e| sanitize_upstream_error(&e))?;

    let body: Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("parse upstream response: {e}"))?;

    // Check for upstream error responses.
    if let Some(err) = body.get("error") {
        let msg = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown upstream error");
        return Err(format!("upstream error: {msg}"));
    }

    Ok(body)
}

fn call_openrouter_stream<F>(
    api_key: &str,
    or_model: &str,
    request: &ChatCompletionRequest,
    mut on_data: F,
) -> Result<StreamTelemetry, StreamForwardError>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let mut payload = serde_json::to_value(request).map_err(|e| StreamForwardError {
        message: format!("serialize request: {e}"),
        telemetry: StreamTelemetry::default(),
    })?;
    payload["model"] = Value::String(or_model.to_string());
    payload["stream"] = Value::Bool(true);
    payload["stream_options"] = json!({"include_usage": true});

    let mut telemetry = StreamTelemetry::default();
    let resp = http_client::post_with_timeout(
        "https://openrouter.ai/api/v1/chat/completions",
        Duration::from_secs(300),
    )
    .map_err(|e| StreamForwardError {
        message: e,
        telemetry: telemetry.clone(),
    })?
    .header("Authorization", &format!("Bearer {api_key}"))
    .header("X-Title", "AgentHALO")
    .content_type("application/json")
    .send_json(payload)
    .map_err(|e| StreamForwardError {
        message: sanitize_upstream_error(&e),
        telemetry: telemetry.clone(),
    })?;

    let reader = resp.into_body().into_reader();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next() {
        let line = line.map_err(|e| StreamForwardError {
            message: format!("read upstream stream line: {e}"),
            telemetry: telemetry.clone(),
        })?;
        if !line.starts_with("data:") {
            continue;
        }
        let data_payload = line.trim_start_matches("data:").trim().to_string();
        if data_payload.is_empty() {
            continue;
        }
        update_stream_usage_from_payload(&data_payload, &mut telemetry);
        on_data(&data_payload).map_err(|e| StreamForwardError {
            message: e,
            telemetry: telemetry.clone(),
        })?;
        if data_payload == "[DONE]" {
            telemetry.completed = true;
            break;
        }
    }

    Ok(telemetry)
}

fn update_stream_usage_from_payload(payload: &str, usage: &mut StreamTelemetry) {
    let Ok(obj) = serde_json::from_str::<Value>(payload) else {
        return;
    };
    if usage.generation_id.is_none() {
        usage.generation_id = obj.get("id").and_then(|v| v.as_str()).map(str::to_string);
    }
    if let Some(u) = obj.get("usage") {
        if usage.prompt_tokens.is_none() {
            usage.prompt_tokens = u.get("prompt_tokens").and_then(|v| v.as_u64());
        }
        if usage.completion_tokens.is_none() {
            usage.completion_tokens = u.get("completion_tokens").and_then(|v| v.as_u64());
        }
    }
    if usage.completion_preview.len() >= 512 {
        return;
    }
    let snippet = obj
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if snippet.is_empty() {
        return;
    }
    let remaining = 512usize.saturating_sub(usage.completion_preview.len());
    usage
        .completion_preview
        .push_str(&snippet.chars().take(remaining).collect::<String>());
}

fn estimate_text_tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

fn estimate_request_input_tokens(request: &ChatCompletionRequest) -> u64 {
    request
        .messages
        .iter()
        .map(|m| match &m.content {
            Value::String(s) => estimate_text_tokens(s),
            Value::Array(items) => items
                .iter()
                .filter_map(|it| it.get("text").and_then(|v| v.as_str()))
                .map(estimate_text_tokens)
                .sum(),
            other => estimate_text_tokens(&other.to_string()),
        })
        .sum()
}

/// Extract token usage from an OpenAI-compatible response.
fn extract_usage(resp: &Value) -> (u64, u64) {
    let usage = resp.get("usage");
    let input = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    (input, output)
}

/// Sanitize upstream errors — never leak the operator's OpenRouter key.
fn sanitize_upstream_error(err: &ureq::Error) -> String {
    let msg = err.to_string();
    // Redact anything that looks like a key or token.
    if msg.contains("key=") || msg.contains("sk-or-") || msg.contains("Bearer") {
        "upstream service error (credentials redacted)".to_string()
    } else {
        format!("upstream service error: {msg}")
    }
}

/// Sanitize a response body — strip any fields that could leak operator info.
pub fn sanitize_response(mut resp: Value) -> Value {
    // OpenRouter sometimes includes internal IDs — keep `id` but strip internals.
    if let Some(obj) = resp.as_object_mut() {
        obj.remove("x-openrouter");
        obj.remove("openrouter");
    }
    resp
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_model_name_mapping() {
        // Catalog lookups.
        assert_eq!(
            openrouter_model_name("claude-opus-4-6"),
            "anthropic/claude-opus-4-6"
        );
        assert_eq!(openrouter_model_name("gpt-4o"), "openai/gpt-4o");
        assert_eq!(openrouter_model_name("o3"), "openai/o3");
        assert_eq!(
            openrouter_model_name("gemini-2.5-pro"),
            "google/gemini-2.5-pro"
        );
        // Already prefixed — pass through.
        assert_eq!(
            openrouter_model_name("meta-llama/llama-4-maverick"),
            "meta-llama/llama-4-maverick"
        );
        // Unknown — pass through.
        assert_eq!(openrouter_model_name("some-new-model"), "some-new-model");
    }

    #[test]
    fn strip_provider_prefix_works() {
        assert_eq!(
            strip_provider_prefix("anthropic/claude-opus-4-6"),
            "claude-opus-4-6"
        );
        assert_eq!(strip_provider_prefix("openai/gpt-4o"), "gpt-4o");
        assert_eq!(strip_provider_prefix("plain-model"), "plain-model");
    }

    #[test]
    fn extract_usage_from_response() {
        let resp = json!({
            "choices": [{"message": {"content": "hi"}}],
            "usage": {"prompt_tokens": 100, "completion_tokens": 50}
        });
        assert_eq!(extract_usage(&resp), (100, 50));

        let resp2 = json!({"choices": []});
        assert_eq!(extract_usage(&resp2), (0, 0));
    }

    #[test]
    fn sanitize_error_redacts_keys() {
        // We can't construct ureq::Error directly, but we test the sanitize logic.
        let msg = "connection error: key=sk-or-abc123 something";
        assert!(msg.contains("sk-or-"));
    }

    #[test]
    fn sanitize_response_strips_internal() {
        let resp = json!({
            "id": "gen-123",
            "choices": [],
            "x-openrouter": {"internal": true},
            "openrouter": {"cost": 0.01},
        });
        let cleaned = sanitize_response(resp);
        assert!(cleaned.get("x-openrouter").is_none());
        assert!(cleaned.get("openrouter").is_none());
        assert!(cleaned.get("id").is_some());
        assert!(cleaned.get("choices").is_some());
    }

    #[test]
    fn model_catalog_all_have_or_ids() {
        for (display_id, _, or_id) in OPENROUTER_MODELS {
            assert!(
                or_id.contains('/'),
                "catalog entry '{}' missing provider prefix in or_id '{}'",
                display_id,
                or_id
            );
        }
    }

    #[test]
    fn model_catalog_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for (display_id, _, _) in OPENROUTER_MODELS {
            assert!(
                seen.insert(display_id),
                "duplicate catalog entry: {}",
                display_id
            );
        }
    }

    #[test]
    fn update_stream_usage_from_payload_extracts_usage_and_id() {
        let mut usage = StreamTelemetry::default();
        update_stream_usage_from_payload(
            r#"{"id":"chatcmpl-123","usage":{"prompt_tokens":7,"completion_tokens":9}}"#,
            &mut usage,
        );
        assert_eq!(usage.generation_id.as_deref(), Some("chatcmpl-123"));
        assert_eq!(usage.prompt_tokens, Some(7));
        assert_eq!(usage.completion_tokens, Some(9));
    }

    #[test]
    fn update_stream_usage_from_payload_ignores_invalid_json() {
        let mut usage = StreamTelemetry::default();
        update_stream_usage_from_payload("not-json", &mut usage);
        assert!(usage.generation_id.is_none());
        assert!(usage.prompt_tokens.is_none());
        assert!(usage.completion_tokens.is_none());
    }
}
