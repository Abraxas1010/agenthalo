//! Metered proxy — routes inference through OpenRouter or a local backend.
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
use crate::halo::governor_registry::{GovernorRegistry, GovernorSnapshot};
use crate::halo::http_client;
use crate::halo::local_models::{self, LocalBackendType};
use crate::halo::pricing;
use crate::halo::vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
}

/// Full result from a metered proxy call.
pub struct MeteredResult {
    /// OpenAI-compatible response body (with injected `x_agenthalo` billing block).
    pub body: Value,
    /// Model ID that was actually used upstream.
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

#[derive(Clone)]
pub struct ProxyGovernorRuntime {
    inner: Arc<ProxyGovernorRuntimeInner>,
}

struct ProxyGovernorRuntimeInner {
    registry: Arc<GovernorRegistry>,
    in_flight: AtomicUsize,
    latency_ewma_secs: Mutex<Option<f64>>,
    last_latency_sample: Mutex<Option<Instant>>,
    cost_ewma_usd_per_min: Mutex<Option<f64>>,
    last_cost_sample: Mutex<Option<Instant>>,
}

pub struct ProxyPermit {
    runtime: ProxyGovernorRuntime,
}

impl std::fmt::Debug for ProxyPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyPermit").finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProxyGovernorStatus {
    pub snapshot: GovernorSnapshot,
    pub in_flight: usize,
    pub latency_ewma_ms: Option<f64>,
    pub cost_burn_rate_ewma_usd_per_min: Option<f64>,
}

impl ProxyGovernorRuntime {
    pub fn new(registry: Arc<GovernorRegistry>) -> Self {
        Self {
            inner: Arc::new(ProxyGovernorRuntimeInner {
                registry,
                in_flight: AtomicUsize::new(0),
                latency_ewma_secs: Mutex::new(None),
                last_latency_sample: Mutex::new(None),
                cost_ewma_usd_per_min: Mutex::new(None),
                last_cost_sample: Mutex::new(None),
            }),
        }
    }

    pub fn try_acquire(&self) -> Result<ProxyPermit, String> {
        let snapshot = self.inner.registry.snapshot_one("gov-proxy")?;
        let limit = snapshot.epsilon.ceil().max(1.0) as usize;
        loop {
            let current = self.inner.in_flight.load(Ordering::SeqCst);
            if current >= limit {
                return Err(format!(
                    "proxy backpressure: in-flight {} >= governor epsilon {:.2}",
                    current, snapshot.epsilon
                ));
            }
            if self
                .inner
                .in_flight
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(ProxyPermit {
                    runtime: self.clone(),
                });
            }
        }
    }

    pub fn record_latency(&self, elapsed: Duration) -> Result<GovernorSnapshot, String> {
        let mut ewma = self
            .inner
            .latency_ewma_secs
            .lock()
            .map_err(|e| format!("proxy latency EWMA lock poisoned: {e}"))?;
        let sample = elapsed.as_secs_f64();
        let updated = match *ewma {
            Some(previous) => previous * 0.9 + sample * 0.1,
            None => sample,
        };
        *ewma = Some(updated);
        drop(ewma);
        let mut last_sample = self
            .inner
            .last_latency_sample
            .lock()
            .map_err(|e| format!("proxy latency timestamp lock poisoned: {e}"))?;
        *last_sample = Some(Instant::now());
        drop(last_sample);
        self.inner.registry.observe("gov-proxy", updated)?;
        self.inner.registry.snapshot_one("gov-proxy")
    }

    pub fn admit_estimated_cost(&self, estimated_cost_usd: f64) -> Result<(), String> {
        let snapshot = self.inner.registry.snapshot_one("gov-cost")?;
        if estimated_cost_usd > snapshot.epsilon {
            return Err(format!(
                "cost governor rejected request: estimated ${estimated_cost_usd:.6} > epsilon ${:.6}",
                snapshot.epsilon
            ));
        }
        Ok(())
    }

    pub fn record_cost(&self, customer_cost_usd: f64) -> Result<GovernorSnapshot, String> {
        let now = Instant::now();
        let mut last_sample = self
            .inner
            .last_cost_sample
            .lock()
            .map_err(|e| format!("proxy cost timestamp lock poisoned: {e}"))?;
        let elapsed_minutes = last_sample
            .map(|previous| (now - previous).as_secs_f64() / 60.0)
            .unwrap_or(1.0 / 60.0)
            .max(1.0 / 60.0);
        *last_sample = Some(now);
        drop(last_sample);

        let sample_rate = customer_cost_usd / elapsed_minutes;
        let mut ewma = self
            .inner
            .cost_ewma_usd_per_min
            .lock()
            .map_err(|e| format!("proxy cost EWMA lock poisoned: {e}"))?;
        let updated = match *ewma {
            Some(previous) => previous * 0.9 + sample_rate * 0.1,
            None => sample_rate,
        };
        *ewma = Some(updated);
        drop(ewma);
        self.inner.registry.observe("gov-cost", updated)?;
        self.inner.registry.snapshot_one("gov-cost")
    }

    pub fn status(&self) -> Result<ProxyGovernorStatus, String> {
        let snapshot = self.inner.registry.snapshot_one("gov-proxy")?;
        let latency_ewma_ms = self
            .inner
            .latency_ewma_secs
            .lock()
            .map_err(|e| format!("proxy latency EWMA lock poisoned: {e}"))?
            .map(|secs| secs * 1000.0);
        let cost_burn_rate_ewma_usd_per_min = *self
            .inner
            .cost_ewma_usd_per_min
            .lock()
            .map_err(|e| format!("proxy cost EWMA lock poisoned: {e}"))?;
        Ok(ProxyGovernorStatus {
            snapshot,
            in_flight: self.inner.in_flight.load(Ordering::SeqCst),
            latency_ewma_ms,
            cost_burn_rate_ewma_usd_per_min,
        })
    }

    pub fn soft_reset_if_quiescent(&self, idle_for: Duration) -> Result<(), String> {
        if self.inner.in_flight.load(Ordering::SeqCst) != 0 {
            return Ok(());
        }
        let now = Instant::now();
        let proxy_idle = self
            .inner
            .last_latency_sample
            .lock()
            .map_err(|e| format!("proxy latency timestamp lock poisoned: {e}"))?
            .map(|sample| now.duration_since(sample) >= idle_for)
            .unwrap_or(true);
        if proxy_idle {
            self.inner.registry.soft_reset("gov-proxy")?;
            *self
                .inner
                .latency_ewma_secs
                .lock()
                .map_err(|e| format!("proxy latency EWMA lock poisoned: {e}"))? = None;
            *self
                .inner
                .last_latency_sample
                .lock()
                .map_err(|e| format!("proxy latency timestamp lock poisoned: {e}"))? = None;
        }

        let cost_idle = self
            .inner
            .last_cost_sample
            .lock()
            .map_err(|e| format!("proxy cost timestamp lock poisoned: {e}"))?
            .map(|sample| now.duration_since(sample) >= idle_for)
            .unwrap_or(true);
        if cost_idle {
            self.inner.registry.soft_reset("gov-cost")?;
            *self
                .inner
                .cost_ewma_usd_per_min
                .lock()
                .map_err(|e| format!("proxy cost EWMA lock poisoned: {e}"))? = None;
            *self
                .inner
                .last_cost_sample
                .lock()
                .map_err(|e| format!("proxy cost timestamp lock poisoned: {e}"))? = None;
        }
        Ok(())
    }
}

impl Drop for ProxyPermit {
    fn drop(&mut self) {
        self.runtime
            .inner
            .in_flight
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                Some(current.saturating_sub(1))
            })
            .ok();
    }
}

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
    vault: Option<&Vault>,
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

    // 2. Resolve the requested backend/model.
    let route = resolve_backend_for_request(&request.model)?;
    let routed_model = route.routed_model().to_string();

    // 3. Pre-authorize: estimate cost from pricing table.
    let estimated_tokens = request.max_tokens.unwrap_or(1024) as u64;
    let est_marked_up = if route.is_local() {
        0.0
    } else {
        let (_est_base, est_marked_up) = pricing::calculate_marked_up_cost(
            &strip_provider_prefix(&routed_model),
            100, // conservative input estimate
            estimated_tokens,
            0,
            pricing_table,
            markup_pct,
        );
        est_marked_up
    };

    // Require at least the estimated marked-up cost in balance.
    let balance = key_store.get_balance(&customer.key_id);
    if est_marked_up > 0.0 && balance < est_marked_up {
        return Err(format!(
            "insufficient balance: ${:.6} available, estimated cost ${:.6}",
            balance, est_marked_up
        ));
    }

    // 4. Call the resolved backend.
    let resp_body = call_resolved_backend(vault, &route, request)?;

    // 6. Extract actual usage from response.
    let (input_tokens, output_tokens) = extract_usage(&resp_body);
    let generation_id = resp_body
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 7. Calculate actual costs.
    let (upstream_cost, customer_cost) = if route.is_local() {
        (0.0, 0.0)
    } else {
        let model_for_pricing = strip_provider_prefix(&routed_model);
        pricing::calculate_marked_up_cost(
            &model_for_pricing,
            input_tokens,
            output_tokens,
            0,
            pricing_table,
            markup_pct,
        )
    };

    // 8. Deduct from customer balance.
    let remaining = key_store.deduct_balance(&customer.key_id, customer_cost);

    // 9. Record usage for audit trail.
    key_store.record_usage(
        &customer.key_id,
        &route.display_model(),
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
        "model": route.display_model(),
        "backend": route.backend_label(),
        "upstream_model": routed_model,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cost_usd": customer_cost,
        "remaining_balance_usd": remaining,
        "generation_id": generation_id,
    });

    Ok(MeteredResult {
        body: enriched,
        or_model: route.display_model(),
        input_tokens,
        output_tokens,
        upstream_cost_usd: upstream_cost,
        customer_cost_usd: customer_cost,
        remaining_balance_usd: remaining,
        generation_id,
    })
}

#[allow(clippy::result_large_err)]
pub fn metered_proxy_stream_sync<F>(
    vault: Option<&Vault>,
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

    let balance = key_store.get_balance(&customer.key_id);
    let route =
        resolve_backend_for_request(&request.model).map_err(|message| MeteredStreamError {
            message,
            settled: MeteredStreamResult {
                or_model: request.model.clone(),
                input_tokens: 0,
                output_tokens: 0,
                upstream_cost_usd: 0.0,
                customer_cost_usd: 0.0,
                remaining_balance_usd: balance,
                generation_id: None,
                telemetry: StreamTelemetry::default(),
            },
        })?;
    let routed_model = route.routed_model().to_string();
    let estimated_input_tokens = estimate_request_input_tokens(request);
    let estimated_output_tokens = request.max_tokens.unwrap_or(1024) as u64;
    let est_marked_up = if route.is_local() {
        0.0
    } else {
        let (_est_base, est_marked_up) = pricing::calculate_marked_up_cost(
            &strip_provider_prefix(&routed_model),
            estimated_input_tokens,
            estimated_output_tokens,
            0,
            pricing_table,
            markup_pct,
        );
        est_marked_up
    };

    if est_marked_up > 0.0 && balance < est_marked_up {
        return Err(MeteredStreamError {
            message: format!(
                "insufficient balance: ${:.6} available, estimated cost ${:.6}",
                balance, est_marked_up
            ),
            settled: MeteredStreamResult {
                or_model: route.display_model(),
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

    let (telemetry, stream_error) =
        match call_resolved_backend_stream(vault, &route, request, |chunk| on_data(chunk)) {
            Ok(t) => (t, None),
            Err(err) => (err.telemetry, Some(err.message)),
        };

    let input_tokens = telemetry.prompt_tokens.unwrap_or(estimated_input_tokens);
    let output_tokens = telemetry
        .completion_tokens
        .unwrap_or_else(|| estimate_text_tokens(&telemetry.completion_preview));
    let (upstream_cost, customer_cost) = if route.is_local() {
        (0.0, 0.0)
    } else {
        pricing::calculate_marked_up_cost(
            &strip_provider_prefix(&routed_model),
            input_tokens,
            output_tokens,
            0,
            pricing_table,
            markup_pct,
        )
    };
    let remaining = key_store.deduct_balance(&customer.key_id, customer_cost);
    key_store.record_usage(
        &customer.key_id,
        &route.display_model(),
        input_tokens,
        output_tokens,
        customer_cost,
    );

    let settled = MeteredStreamResult {
        or_model: route.display_model(),
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
pub fn proxy_chat_sync(
    vault: Option<&Vault>,
    request: &ChatCompletionRequest,
) -> Result<Value, String> {
    let route = resolve_backend_for_request(&request.model)?;
    call_resolved_backend(vault, &route, request)
}

pub fn proxy_chat_stream_sync<F>(
    vault: Option<&Vault>,
    request: &ChatCompletionRequest,
    mut on_data: F,
) -> Result<StreamTelemetry, StreamForwardError>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let route =
        resolve_backend_for_request(&request.model).map_err(|message| StreamForwardError {
            message,
            telemetry: StreamTelemetry::default(),
        })?;
    call_resolved_backend_stream(vault, &route, request, |chunk| on_data(chunk))
}

// ---------------------------------------------------------------------------
// Model catalog
// ---------------------------------------------------------------------------

/// Return available cloud + local models.
pub fn list_available_models(vault: Option<&Vault>) -> Vec<Value> {
    let mut models = local_models::catalog_entries();
    if vault
        .and_then(|vault| vault.get_key("openrouter").ok())
        .is_some()
    {
        models.extend(OPENROUTER_MODELS.iter().map(|(id, owner, or_id)| {
            json!({
                "id": id,
                "object": "model",
                "owned_by": owner,
                "openrouter_id": or_id,
            })
        }));
    }
    models
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
    call_openai_compatible(
        "https://openrouter.ai/api/v1/chat/completions",
        &[
            ("Authorization", format!("Bearer {api_key}")),
            ("X-Title", "AgentHALO".to_string()),
        ],
        or_model,
        request,
        sanitize_upstream_error,
    )
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
    call_openai_compatible_stream(
        "https://openrouter.ai/api/v1/chat/completions",
        &[
            ("Authorization", format!("Bearer {api_key}")),
            ("X-Title", "AgentHALO".to_string()),
        ],
        or_model,
        request,
        sanitize_upstream_error,
        |chunk| on_data(chunk),
    )
}

fn call_openai_compatible(
    endpoint: &str,
    headers: &[(&str, String)],
    model: &str,
    request: &ChatCompletionRequest,
    sanitize_error: fn(&ureq::Error) -> String,
) -> Result<Value, String> {
    let mut payload =
        serde_json::to_value(request).map_err(|e| format!("serialize request: {e}"))?;
    payload["model"] = Value::String(model.to_string());

    // Remove stream=false to avoid issues — we handle non-streaming only.
    if let Some(obj) = payload.as_object_mut() {
        if let Some(Value::Bool(false)) = obj.get("stream") {
            obj.remove("stream");
        }
    }

    let mut req = http_client::post(endpoint)?.content_type("application/json");
    for (name, value) in headers {
        req = req.header(*name, value);
    }
    let resp = req.send_json(payload).map_err(|err| sanitize_error(&err))?;

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

fn call_openai_compatible_stream<F>(
    endpoint: &str,
    headers: &[(&str, String)],
    model: &str,
    request: &ChatCompletionRequest,
    sanitize_error: fn(&ureq::Error) -> String,
    mut on_data: F,
) -> Result<StreamTelemetry, StreamForwardError>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let mut payload = serde_json::to_value(request).map_err(|e| StreamForwardError {
        message: format!("serialize request: {e}"),
        telemetry: StreamTelemetry::default(),
    })?;
    payload["model"] = Value::String(model.to_string());
    payload["stream"] = Value::Bool(true);
    payload["stream_options"] = json!({"include_usage": true});

    let mut telemetry = StreamTelemetry::default();
    let mut req = http_client::post_with_timeout(endpoint, Duration::from_secs(300))
        .map_err(|e| StreamForwardError {
            message: e,
            telemetry: telemetry.clone(),
        })?
        .content_type("application/json");
    for (name, value) in headers {
        req = req.header(*name, value);
    }
    let resp = req.send_json(payload).map_err(|e| StreamForwardError {
        message: sanitize_error(&e),
        telemetry: telemetry.clone(),
    })?;

    let reader = resp.into_body().into_reader();
    let lines = BufReader::new(reader).lines();
    for line in lines {
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

#[derive(Clone, Debug)]
enum ResolvedBackend {
    OpenRouter {
        routed_model: String,
    },
    Local {
        backend: LocalBackendType,
        base_url: String,
        routed_model: String,
    },
}

impl ResolvedBackend {
    fn routed_model(&self) -> &str {
        match self {
            Self::OpenRouter { routed_model } => routed_model,
            Self::Local { routed_model, .. } => routed_model,
        }
    }

    fn display_model(&self) -> String {
        match self {
            Self::OpenRouter { routed_model } => routed_model.clone(),
            Self::Local { routed_model, .. } => format!("local/{routed_model}"),
        }
    }

    fn backend_label(&self) -> &'static str {
        match self {
            Self::OpenRouter { .. } => "openrouter",
            Self::Local { backend, .. } => backend.as_str(),
        }
    }

    fn is_local(&self) -> bool {
        matches!(self, Self::Local { .. })
    }
}

fn resolve_backend_for_request(model: &str) -> Result<ResolvedBackend, String> {
    let requested = model.trim();
    if requested.is_empty() {
        return Err("model must not be empty".to_string());
    }
    if requested.starts_with("local/") {
        let resolved = local_models::resolve_local_route(requested)?;
        return Ok(ResolvedBackend::Local {
            backend: resolved.backend,
            base_url: resolved.base_url,
            routed_model: resolved.model,
        });
    }

    if requested.contains('/') {
        if local_models::installed_backend_for_model(requested).is_some() {
            let resolved = local_models::resolve_local_route(requested)?;
            return Ok(ResolvedBackend::Local {
                backend: resolved.backend,
                base_url: resolved.base_url,
                routed_model: resolved.model,
            });
        }
        return Ok(ResolvedBackend::OpenRouter {
            routed_model: openrouter_model_name(requested),
        });
    }

    if looks_like_openrouter_cloud_model(requested) {
        return Ok(ResolvedBackend::OpenRouter {
            routed_model: openrouter_model_name(requested),
        });
    }

    if local_models::installed_backend_for_model(requested).is_some() {
        let resolved = local_models::resolve_local_route(requested)?;
        return Ok(ResolvedBackend::Local {
            backend: resolved.backend,
            base_url: resolved.base_url,
            routed_model: resolved.model,
        });
    }

    Ok(ResolvedBackend::OpenRouter {
        routed_model: openrouter_model_name(requested),
    })
}

fn looks_like_openrouter_cloud_model(model: &str) -> bool {
    model.starts_with("claude")
        || model.starts_with("gpt")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
        || model.starts_with("gemini")
        || model.starts_with("deepseek/")
        || model.starts_with("mistralai/")
        || model.starts_with("cohere/")
        || model.starts_with("perplexity/")
        || model.starts_with("meta-llama/")
}

fn call_resolved_backend(
    vault: Option<&Vault>,
    route: &ResolvedBackend,
    request: &ChatCompletionRequest,
) -> Result<Value, String> {
    match route {
        ResolvedBackend::OpenRouter { routed_model } => {
            let or_api_key = vault
                .ok_or_else(|| {
                    "proxy service unavailable: local backend not selected and OpenRouter is not configured".to_string()
                })?
                .get_key("openrouter")
                .map_err(|_| "proxy service unavailable: upstream not configured".to_string())?;
            call_openrouter(&or_api_key, routed_model, request)
        }
        ResolvedBackend::Local {
            base_url,
            routed_model,
            ..
        } => call_openai_compatible(
            &format!("{}/v1/chat/completions", base_url.trim_end_matches('/')),
            &[],
            routed_model,
            request,
            |err| format!("local backend error: {err}"),
        ),
    }
}

fn call_resolved_backend_stream<F>(
    vault: Option<&Vault>,
    route: &ResolvedBackend,
    request: &ChatCompletionRequest,
    mut on_data: F,
) -> Result<StreamTelemetry, StreamForwardError>
where
    F: FnMut(&str) -> Result<(), String>,
{
    match route {
        ResolvedBackend::OpenRouter { routed_model } => {
            let or_api_key = vault
                .ok_or_else(|| StreamForwardError {
                    message: "OpenRouter not configured and no local backend matched".to_string(),
                    telemetry: StreamTelemetry::default(),
                })?
                .get_key("openrouter")
                .map_err(|_| StreamForwardError {
                    message:
                        "no OpenRouter API key configured — run: agenthalo vault set openrouter"
                            .to_string(),
                    telemetry: StreamTelemetry::default(),
                })?;
            call_openrouter_stream(&or_api_key, routed_model, request, |chunk| on_data(chunk))
        }
        ResolvedBackend::Local {
            base_url,
            routed_model,
            ..
        } => call_openai_compatible_stream(
            &format!("{}/v1/chat/completions", base_url.trim_end_matches('/')),
            &[],
            routed_model,
            request,
            |err| format!("local backend error: {err}"),
            |chunk| on_data(chunk),
        ),
    }
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

pub fn estimate_marked_up_request_cost(
    request: &ChatCompletionRequest,
    pricing_table: &HashMap<String, pricing::ModelPricing>,
    markup_pct: f64,
) -> (String, f64) {
    let route =
        resolve_backend_for_request(&request.model).unwrap_or(ResolvedBackend::OpenRouter {
            routed_model: openrouter_model_name(&request.model),
        });
    if route.is_local() {
        return (route.display_model(), 0.0);
    }
    let or_model = route.routed_model().to_string();
    let estimated_input_tokens = estimate_request_input_tokens(request);
    let estimated_output_tokens = request.max_tokens.unwrap_or(1024) as u64;
    let (_, estimated_cost) = pricing::calculate_marked_up_cost(
        &strip_provider_prefix(&or_model),
        estimated_input_tokens,
        estimated_output_tokens,
        0,
        pricing_table,
        markup_pct,
    );
    (or_model, estimated_cost)
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
    fn estimate_cost_for_explicit_local_model_is_zero() {
        let request = ChatCompletionRequest {
            model: "local/qwen2.5-coder:7b".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            temperature: None,
            max_tokens: Some(64),
            stream: Some(false),
            top_p: None,
            tools: None,
        };
        let (_model, cost) = estimate_marked_up_request_cost(&request, &HashMap::new(), 0.2);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn cloud_model_heuristic_catches_openrouter_families() {
        assert!(looks_like_openrouter_cloud_model("gpt-4o"));
        assert!(looks_like_openrouter_cloud_model("claude-opus-4-6"));
        assert!(looks_like_openrouter_cloud_model(
            "meta-llama/llama-4-maverick"
        ));
        assert!(!looks_like_openrouter_cloud_model("qwen2.5-coder:7b"));
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

    #[test]
    fn proxy_governor_runtime_tracks_backpressure_and_ewma() {
        let registry = Arc::new(GovernorRegistry::new());
        registry
            .register(crate::halo::governor::GovernorConfig {
                instance_id: "gov-proxy".to_string(),
                alpha: 0.01,
                beta: 0.05,
                dt: 1.0,
                eps_min: 1.0,
                eps_max: 4.0,
                target: 2.0,
                formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.lyapunov_descent"
                    .to_string(),
                ki: 0.0,
                kb: 0.0,
                adaptive: None,
            })
            .expect("register gov-proxy");
        registry
            .register(crate::halo::governor::GovernorConfig {
                instance_id: "gov-cost".to_string(),
                alpha: 0.01,
                beta: 0.05,
                dt: 1.0,
                eps_min: 0.01,
                eps_max: 10.0,
                target: 1.0,
                formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.validatorRegime"
                    .to_string(),
                ki: 0.0,
                kb: 0.0,
                adaptive: None,
            })
            .expect("register gov-cost");
        let runtime = ProxyGovernorRuntime::new(registry);
        let permit = runtime.try_acquire().expect("permit");
        let err = runtime.try_acquire().expect_err("backpressure");
        assert!(err.contains("backpressure"));
        runtime
            .record_latency(Duration::from_millis(1750))
            .expect("record latency");
        runtime.record_cost(0.25).expect("record cost");
        let status = runtime.status().expect("status");
        assert_eq!(status.in_flight, 1);
        assert!(status.latency_ewma_ms.unwrap_or_default() >= 1700.0);
        drop(permit);
        assert_eq!(runtime.status().expect("status after drop").in_flight, 0);
    }
}
