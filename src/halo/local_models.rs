//! Local model discovery, cataloging, and managed serving for vLLM.
//!
//! AgentHALO treats local inference as a single HuggingFace-backed vLLM
//! upstream. Ollama compatibility is preserved only as a config/CLI alias so
//! existing installations do not crash on old settings.

use crate::halo::config;
use crate::halo::http_client;
use crate::halo::vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_VLLM_PORT: u16 = 8000;
const HF_TOKEN_ENV: &str = "HF_TOKEN";
const HF_FALLBACK_TOKEN_PATH: &str = ".cache/huggingface/token";
const HF_METADATA_FILE: &str = ".agenthalo-model.json";
const LOCAL_MODEL_HTTP_TIMEOUT: Duration = Duration::from_secs(5);
const LOCAL_MODEL_CACHE_TTL: Duration = Duration::from_secs(10);
const LOCAL_MODEL_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const LOCAL_MODEL_POLL_INTERVAL: Duration = Duration::from_millis(250);
const LOCAL_MODEL_STDERR_TAIL_BYTES: usize = 4096;
const GPU_HEADROOM_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Default)]
struct InstalledModelCache {
    refreshed_at: Option<Instant>,
    models: Vec<InstalledLocalModel>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LocalBackendType {
    #[serde(alias = "ollama")]
    Vllm,
}

impl LocalBackendType {
    pub fn as_str(self) -> &'static str {
        "vllm"
    }

    pub fn display_name(self) -> &'static str {
        "vLLM"
    }
}

impl std::fmt::Display for LocalBackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for LocalBackendType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "vllm" | "hf" | "huggingface" | "ollama" => Ok(Self::Vllm),
            other => Err(format!(
                "unknown backend `{other}`; expected vllm (or legacy ollama alias)"
            )),
        }
    }
}

impl Default for LocalBackendType {
    fn default() -> Self {
        Self::Vllm
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ManagedServeState {
    pub port: u16,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    pub started_at_unix: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstalledModelHint {
    pub model: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum ManagedServeStateField {
    One(ManagedServeState),
    Many(Vec<ManagedServeState>),
}

fn deserialize_managed_state<'de, D>(deserializer: D) -> Result<Option<ManagedServeState>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<ManagedServeStateField>::deserialize(deserializer)?;
    Ok(match value {
        Some(ManagedServeStateField::One(item)) => Some(item),
        Some(ManagedServeStateField::Many(mut items)) => items.pop(),
        None => None,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LocalModelsConfig {
    pub vllm_port: u16,
    pub hf_token_path: String,
    #[serde(default)]
    pub vllm_default_model: Option<String>,
    #[serde(default, deserialize_with = "deserialize_managed_state")]
    pub managed: Option<ManagedServeState>,
    #[serde(default)]
    pub installed_hints: Vec<InstalledModelHint>,
    #[serde(default)]
    pub local_compute_cost_per_1k_tokens_usd: f64,
}

impl Default for LocalModelsConfig {
    fn default() -> Self {
        Self {
            vllm_port: DEFAULT_VLLM_PORT,
            hf_token_path: default_hf_token_path().display().to_string(),
            vllm_default_model: None,
            managed: None,
            installed_hints: Vec::new(),
            local_compute_cost_per_1k_tokens_usd: 0.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GpuInfo {
    pub vendor: String,
    pub name: String,
    pub total_memory_gib: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstalledLocalModel {
    pub source: String,
    pub backend: LocalBackendType,
    pub model: String,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub quantization: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub served: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BackendStatus {
    pub cli_installed: bool,
    #[serde(default)]
    pub cli_path: Option<String>,
    #[serde(default)]
    pub cli_version: Option<String>,
    pub base_url: String,
    pub healthy: bool,
    #[serde(default)]
    pub served_models: Vec<String>,
    #[serde(default)]
    pub installed_models: Vec<InstalledLocalModel>,
    #[serde(default)]
    pub managed: Option<ManagedServeState>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LocalModelsStatus {
    pub config: LocalModelsConfig,
    pub huggingface_token_configured: bool,
    #[serde(default)]
    pub gpu: Option<GpuInfo>,
    pub backend: BackendStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelSearchResult {
    pub index: usize,
    pub source: String,
    pub backend: LocalBackendType,
    pub model: String,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub quantization: Option<String>,
    #[serde(default)]
    pub quantizations: Vec<String>,
    #[serde(default)]
    pub downloads: Option<String>,
    #[serde(default)]
    pub likes: Option<u64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pipeline_tag: Option<String>,
    #[serde(default)]
    pub fits_gpu: Option<bool>,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub source_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServeRequest {
    #[serde(default)]
    pub backend: LocalBackendType,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServeResult {
    pub backend: LocalBackendType,
    pub base_url: String,
    pub port: u16,
    #[serde(default)]
    pub model: Option<String>,
    pub already_running: bool,
    #[serde(default)]
    pub pid: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StopResult {
    pub backend: LocalBackendType,
    pub stopped: bool,
    #[serde(default)]
    pub pid: Option<u32>,
    pub base_url: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct HfLocalModelMetadata {
    model_id: String,
    local_dir: String,
    pulled_at_unix: u64,
}

#[derive(Clone, Debug)]
pub struct ResolvedLocalRoute {
    pub backend: LocalBackendType,
    pub base_url: String,
    pub model: String,
}

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModelEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct OpenAiModelEntry {
    id: String,
}

#[derive(Deserialize)]
struct HuggingFaceSearchItem {
    #[serde(rename = "modelId")]
    model_id: String,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    pipeline_tag: Option<String>,
    #[serde(default)]
    likes: Option<u64>,
    #[serde(default)]
    #[serde(rename = "safetensors")]
    safetensors: Option<Value>,
}

pub fn load_or_default() -> LocalModelsConfig {
    let path = config::local_models_config_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => LocalModelsConfig::default(),
    }
}

pub fn save_config(cfg: &LocalModelsConfig) -> Result<(), String> {
    config::ensure_halo_dir()?;
    let path = config::local_models_config_path();
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_vec_pretty(cfg).map_err(|e| format!("serialize local models: {e}"))?;
    write_private_file(&tmp, &raw)?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("commit local model config {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod local model config {}: {e}", path.display()))?;
    }
    Ok(())
}

pub fn default_hf_token_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(HF_FALLBACK_TOKEN_PATH)
}

fn installed_models_cache() -> &'static Mutex<InstalledModelCache> {
    static CACHE: OnceLock<Mutex<InstalledModelCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(InstalledModelCache::default()))
}

fn invalidate_installed_models_cache() {
    if let Ok(mut cache) = installed_models_cache().lock() {
        cache.refreshed_at = None;
        cache.models.clear();
    }
}

fn normalize_installed_hints(models: &[InstalledLocalModel]) -> Vec<InstalledModelHint> {
    let mut hints = models
        .iter()
        .map(|model| InstalledModelHint {
            model: model.model.clone(),
        })
        .collect::<Vec<_>>();
    hints.sort_by(|a, b| a.model.cmp(&b.model));
    hints.dedup_by(|a, b| a.model == b.model);
    hints
}

fn sync_installed_hints(cfg: &mut LocalModelsConfig, models: &[InstalledLocalModel]) -> bool {
    let hints = normalize_installed_hints(models);
    if cfg.installed_hints == hints {
        return false;
    }
    cfg.installed_hints = hints;
    true
}

fn upsert_installed_hint(cfg: &mut LocalModelsConfig, model: &str) -> bool {
    if cfg.installed_hints.iter().any(|hint| hint.model == model) {
        return false;
    }
    cfg.installed_hints.push(InstalledModelHint {
        model: model.to_string(),
    });
    cfg.installed_hints.sort_by(|a, b| a.model.cmp(&b.model));
    true
}

fn remove_installed_hint(cfg: &mut LocalModelsConfig, model: &str) -> bool {
    let before = cfg.installed_hints.len();
    cfg.installed_hints.retain(|hint| hint.model != model);
    before != cfg.installed_hints.len()
}

fn refresh_installed_models_snapshot(cfg: &LocalModelsConfig) -> Vec<InstalledLocalModel> {
    let installed_hf = list_installed_hf_models();
    let served_vllm = query_openai_models(&base_url(cfg.vllm_port)).unwrap_or_default();
    let mut models = mark_served_hf_models(installed_hf, &served_vllm);
    models.sort_by(|a, b| a.model.cmp(&b.model));
    models
}

fn cached_installed_models(cfg: &LocalModelsConfig) -> Vec<InstalledLocalModel> {
    if let Ok(cache) = installed_models_cache().lock() {
        if cache
            .refreshed_at
            .map(|at| at.elapsed() < LOCAL_MODEL_CACHE_TTL)
            .unwrap_or(false)
        {
            return cache.models.clone();
        }
    }

    let refreshed = refresh_installed_models_snapshot(cfg);
    if let Ok(mut cache) = installed_models_cache().lock() {
        cache.refreshed_at = Some(Instant::now());
        cache.models = refreshed.clone();
    }
    let mut cfg_for_hints = cfg.clone();
    if sync_installed_hints(&mut cfg_for_hints, &refreshed) {
        let _ = save_config(&cfg_for_hints);
    }
    refreshed
}

pub fn installed_backend_for_model(model: &str) -> Option<LocalBackendType> {
    let normalized = model.trim();
    if normalized.is_empty() {
        return None;
    }
    if find_installed_hf_model_path(normalized).is_some() {
        return Some(LocalBackendType::Vllm);
    }
    if let Ok(cache) = installed_models_cache().lock() {
        if cache.models.iter().any(|item| item.model == normalized) {
            return Some(LocalBackendType::Vllm);
        }
    }
    let cfg = load_or_default();
    cfg.installed_hints
        .iter()
        .any(|hint| hint.model == normalized)
        .then_some(LocalBackendType::Vllm)
}

pub fn detect_status() -> LocalModelsStatus {
    let cfg = load_or_default();
    let hf_token = resolve_hf_token_with_config(&cfg).ok();
    let gpu = detect_gpu();
    let installed_hf = list_installed_hf_models();
    let base_url = base_url(cfg.vllm_port);
    let served_vllm = query_openai_models(&base_url);
    let served_vllm_models = served_vllm.clone().unwrap_or_default();
    let backend = BackendStatus {
        cli_installed: which_path("vllm").is_some(),
        cli_path: which_path("vllm"),
        cli_version: command_version("vllm", &["--version"]),
        base_url,
        healthy: served_vllm.is_ok() && !served_vllm_models.is_empty(),
        served_models: served_vllm_models.clone(),
        installed_models: mark_served_hf_models(installed_hf, &served_vllm_models),
        managed: cfg.managed.clone(),
        error: served_vllm.err().or_else(|| {
            if served_vllm_models.is_empty() {
                Some("vLLM API not responding at configured endpoint".to_string())
            } else {
                None
            }
        }),
    };
    LocalModelsStatus {
        config: cfg,
        huggingface_token_configured: hf_token
            .as_ref()
            .map(|token| !token.trim().is_empty())
            .unwrap_or(false),
        gpu,
        backend,
    }
}

pub fn search_models(query: &str, limit: usize) -> Result<Vec<ModelSearchResult>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("search query must not be empty".to_string());
    }
    let cfg = load_or_default();
    let gpu = detect_gpu();
    let mut results = search_huggingface(query, limit, gpu.as_ref(), &cfg)?;
    for (index, item) in results.iter_mut().enumerate() {
        item.index = index + 1;
    }
    Ok(results)
}

pub fn list_installed_models() -> Vec<InstalledLocalModel> {
    cached_installed_models(&load_or_default())
}

pub fn catalog_entries() -> Vec<Value> {
    list_installed_models()
        .into_iter()
        .map(|model| {
            serde_json::json!({
                "id": format!("local/{}", model.model),
                "object": "model",
                "owned_by": "local",
                "backend": LocalBackendType::Vllm.as_str(),
                "local_model": model.model,
                "source": model.source,
                "installed": true,
                "served": model.served,
            })
        })
        .collect()
}

pub fn pull_model(
    model: &str,
    source: Option<&str>,
    writer: &mut dyn Write,
) -> Result<InstalledLocalModel, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("model name must not be empty".to_string());
    }
    let _ = normalize_source(source)?;
    config::ensure_halo_dir()?;
    let cfg = load_or_default();
    let local_dir = hf_local_dir(model);
    std::fs::create_dir_all(&local_dir)
        .map_err(|e| format!("create HF model dir {}: {e}", local_dir.display()))?;
    let token = resolve_hf_token_with_config(&cfg).ok();
    let mut command = hf_download_command(model, &local_dir, token.as_deref())?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("start HF download for `{model}`: {e}"))?;
    stream_child_output(&mut child, writer)?;
    let metadata = HfLocalModelMetadata {
        model_id: model.to_string(),
        local_dir: local_dir.display().to_string(),
        pulled_at_unix: now_unix_secs(),
    };
    let raw = serde_json::to_vec_pretty(&metadata)
        .map_err(|e| format!("serialize HF model metadata: {e}"))?;
    write_private_file(&local_dir.join(HF_METADATA_FILE), &raw)?;
    invalidate_installed_models_cache();
    let mut cfg = load_or_default();
    if upsert_installed_hint(&mut cfg, model) {
        save_config(&cfg)?;
    }
    Ok(InstalledLocalModel {
        source: "huggingface".to_string(),
        backend: LocalBackendType::Vllm,
        model: model.to_string(),
        size: dir_size_string(&local_dir),
        quantization: None,
        path: Some(local_dir.display().to_string()),
        served: false,
    })
}

pub fn remove_model(model: &str, source: Option<&str>) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("model name must not be empty".to_string());
    }
    let _ = normalize_source(source)?;
    let local_dir = hf_local_dir(model);
    if !local_dir.exists() {
        return Err(format!(
            "HF model `{model}` is not installed at {}",
            local_dir.display()
        ));
    }
    std::fs::remove_dir_all(&local_dir)
        .map_err(|e| format!("remove HF model {}: {e}", local_dir.display()))?;
    invalidate_installed_models_cache();
    let mut cfg = load_or_default();
    if remove_installed_hint(&mut cfg, model) {
        save_config(&cfg)?;
    }
    Ok(())
}

pub fn login_huggingface(token: Option<&str>) -> Result<String, String> {
    let cfg = load_or_default();
    let resolved = token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| read_token_from_file(Path::new(&cfg.hf_token_path)).ok())
        .or_else(|| read_token_from_file(&default_hf_token_path()).ok())
        .ok_or_else(|| {
            "no Hugging Face token provided; pass --token or paste a token".to_string()
        })?;
    if let Some(vault) = open_vault() {
        vault
            .set_key("huggingface", HF_TOKEN_ENV, &resolved)
            .map_err(|e| format!("store Hugging Face token in vault: {e}"))?;
        let _ = vault.set_test_result("huggingface", true);
        return Ok("vault".to_string());
    }
    let path = PathBuf::from(&cfg.hf_token_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create Hugging Face token dir {}: {e}", parent.display()))?;
    }
    write_private_file(&path, resolved.as_bytes())?;
    Ok(format!(
        "file:{} (warning: encrypted vault unavailable, token stored on disk with 0600 permissions)",
        path.display()
    ))
}

pub fn serve_backend(request: ServeRequest) -> Result<ServeResult, String> {
    let mut cfg = load_or_default();
    let backend = request.backend;
    let port = request.port.unwrap_or(cfg.vllm_port);
    let base_url = base_url(port);
    clear_stale_managed_state(&mut cfg, port, &base_url);
    let mut served_model = request.model.clone();

    if is_backend_healthy(&base_url) {
        return Ok(ServeResult {
            backend,
            base_url,
            port,
            model: served_model,
            already_running: true,
            pid: cfg.managed.as_ref().and_then(|managed| {
                if managed.port == port {
                    managed.pid
                } else {
                    None
                }
            }),
        });
    }

    let model = request
        .model
        .clone()
        .or_else(|| cfg.vllm_default_model.clone())
        .ok_or_else(|| {
            "vLLM serving requires --model <huggingface-model> or a configured default".to_string()
        })?;
    cfg.vllm_default_model = Some(model.clone());
    served_model = Some(model.clone());
    let model_arg = find_installed_hf_model_path(&model)
        .unwrap_or_else(|| PathBuf::from(model.clone()))
        .display()
        .to_string();
    let mut command = Command::new("vllm");
    command.args([
        "serve",
        &model_arg,
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
    ]);
    if let Ok(token) = resolve_hf_token_with_config(&cfg) {
        command.env(HF_TOKEN_ENV, token);
    }

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {} serve: {e}", backend.display_name()))?;
    let pid = Some(child.id());
    wait_for_backend_start(&base_url, &mut child, LOCAL_MODEL_STARTUP_TIMEOUT)?;
    drop(child);
    cfg.vllm_port = port;
    cfg.managed = Some(ManagedServeState {
        port,
        model: served_model.clone(),
        pid,
        started_at_unix: now_unix_secs(),
    });
    save_config(&cfg)?;
    Ok(ServeResult {
        backend,
        base_url,
        port,
        model: served_model,
        already_running: false,
        pid,
    })
}

pub fn stop_backend(backend: LocalBackendType) -> Result<StopResult, String> {
    let mut cfg = load_or_default();
    let port = cfg.vllm_port;
    let base_url = base_url(port);
    let cleared_stale = clear_stale_managed_state(&mut cfg, port, &base_url);
    if cleared_stale {
        save_config(&cfg)?;
    }
    let managed = cfg.managed.clone().ok_or_else(|| {
        "no managed vLLM process recorded; if it is still running, stop it externally".to_string()
    })?;

    if let Some(pid) = managed.pid {
        terminate_pid(pid)?;
        wait_for_backend_stop(&base_url, Duration::from_secs(10))?;
    }

    cfg.managed = None;
    save_config(&cfg)?;

    Ok(StopResult {
        backend,
        stopped: true,
        pid: managed.pid,
        base_url,
        message: "stopped managed vLLM".to_string(),
    })
}

pub fn resolve_local_route(model: &str) -> Result<ResolvedLocalRoute, String> {
    let cfg = load_or_default();
    let normalized = model.trim();
    if normalized.is_empty() {
        return Err("local model must not be empty".to_string());
    }
    let stripped = normalized
        .strip_prefix("local/")
        .unwrap_or(normalized)
        .trim();
    let requested_model = stripped
        .strip_prefix("vllm/")
        .or_else(|| stripped.strip_prefix("ollama/"))
        .unwrap_or(stripped)
        .trim();
    if requested_model.is_empty() {
        return Err("local model name must not be empty".to_string());
    }

    let base_url = base_url(cfg.vllm_port);
    if !is_backend_healthy(&base_url) {
        return Err(format!(
            "vLLM is not healthy at {}; run `agenthalo models serve --model {requested_model}` first",
            base_url
        ));
    }
    let served = query_openai_models(&base_url)?;
    if !served
        .iter()
        .any(|served_model| served_model == requested_model)
    {
        return Err(format!(
            "vLLM is serving [{}], not `{requested_model}`",
            served.join(", ")
        ));
    }
    Ok(ResolvedLocalRoute {
        backend: LocalBackendType::Vllm,
        base_url,
        model: requested_model.to_string(),
    })
}

pub fn resolve_hf_token() -> Result<String, String> {
    resolve_hf_token_with_config(&load_or_default())
}

fn resolve_hf_token_with_config(cfg: &LocalModelsConfig) -> Result<String, String> {
    std::env::var(HF_TOKEN_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            open_vault()
                .and_then(|vault| vault.get_key("huggingface").ok())
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| read_token_from_file(Path::new(&cfg.hf_token_path)).ok())
        .or_else(|| read_token_from_file(&default_hf_token_path()).ok())
        .ok_or_else(|| "no Hugging Face token configured".to_string())
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn is_backend_healthy(base_url: &str) -> bool {
    query_openai_models(base_url)
        .map(|models| !models.is_empty())
        .unwrap_or(false)
}

fn wait_for_backend_start(
    base_url: &str,
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if is_backend_healthy(base_url) {
            invalidate_installed_models_cache();
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("poll vLLM startup: {e}"))?
        {
            let stderr = read_pipe_tail(child.stderr.take());
            let code = status
                .code()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "signal".to_string());
            return Err(if stderr.is_empty() {
                format!("vLLM exited before becoming healthy (exit {code})")
            } else {
                format!("vLLM exited before becoming healthy (exit {code}): {stderr}")
            });
        }
        std::thread::sleep(LOCAL_MODEL_POLL_INTERVAL);
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(format!(
        "vLLM did not become healthy within {}s",
        timeout.as_secs()
    ))
}

fn wait_for_backend_stop(base_url: &str, timeout: Duration) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !is_backend_healthy(base_url) {
            invalidate_installed_models_cache();
            return Ok(());
        }
        std::thread::sleep(LOCAL_MODEL_POLL_INTERVAL);
    }
    Err(format!("vLLM did not stop within {}s", timeout.as_secs()))
}

fn clear_stale_managed_state(cfg: &mut LocalModelsConfig, port: u16, base_url: &str) -> bool {
    let Some(state) = cfg.managed.clone() else {
        return false;
    };
    if state.port == port && !is_backend_healthy(base_url) {
        cfg.managed = None;
        return true;
    }
    false
}

fn terminate_pid(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|e| format!("invoke kill for pid {pid}: {e}"))?;
        if status.success() {
            return Ok(());
        }
        let kill9 = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .map_err(|e| format!("invoke kill -KILL for pid {pid}: {e}"))?;
        if kill9.success() {
            return Ok(());
        }
        return Err(format!("kill failed for pid {pid} with {:?}", kill9.code()));
    }
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .map_err(|e| format!("invoke taskkill for pid {pid}: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "taskkill failed for pid {pid} with {:?}",
                status.code()
            ))
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err("managed backend stop is unsupported on this platform".to_string())
    }
}

fn read_pipe_tail(pipe: Option<std::process::ChildStderr>) -> String {
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let mut stderr = Vec::new();
    let _ = pipe.read_to_end(&mut stderr);
    if stderr.len() > LOCAL_MODEL_STDERR_TAIL_BYTES {
        stderr = stderr[stderr.len() - LOCAL_MODEL_STDERR_TAIL_BYTES..].to_vec();
    }
    String::from_utf8_lossy(&stderr).trim().to_string()
}

fn search_huggingface(
    query: &str,
    limit: usize,
    gpu: Option<&GpuInfo>,
    _cfg: &LocalModelsConfig,
) -> Result<Vec<ModelSearchResult>, String> {
    let url = format!(
        "https://huggingface.co/api/models?search={}&pipeline_tag=text-generation&limit={}",
        urlencoding(query),
        limit
    );
    let mut request = http_client::get_with_timeout(&url, LOCAL_MODEL_HTTP_TIMEOUT)?;
    if let Ok(token) = resolve_hf_token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let items: Vec<HuggingFaceSearchItem> = request
        .call()
        .map_err(|e| format!("huggingface search request failed: {e}"))?
        .into_body()
        .read_json()
        .map_err(|e| format!("parse Hugging Face search response: {e}"))?;

    let mut results = Vec::new();
    for item in items {
        let size_bytes = item
            .safetensors
            .as_ref()
            .and_then(|value| value.get("total"))
            .and_then(|value| value.as_u64());
        let fits_gpu = match (gpu, size_bytes) {
            (Some(gpu), Some(size)) => {
                let required_bytes = size.saturating_mul(2).saturating_add(GPU_HEADROOM_BYTES);
                Some(required_bytes as f64 <= gpu.total_memory_gib * 1024f64.powi(3))
            }
            _ => None,
        };
        let quantizations = quantizations_from_tags(&item.tags);
        let quantization = quantizations.first().cloned();
        let installed = find_installed_hf_model_path(&item.model_id).is_some();
        let description = Some(format!(
            "{}{}{}",
            item.pipeline_tag
                .clone()
                .unwrap_or_else(|| "text-generation".to_string()),
            item.likes
                .map(|likes| format!(" · {likes} likes"))
                .unwrap_or_default(),
            item.tags
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
                .strip_prefix("")
                .map(|tags| if tags.is_empty() {
                    String::new()
                } else {
                    format!(" · {tags}")
                })
                .unwrap_or_default()
        ));
        results.push(ModelSearchResult {
            index: 0,
            source: "huggingface".to_string(),
            backend: LocalBackendType::Vllm,
            model: item.model_id.clone(),
            size: size_bytes.map(format_bytes),
            size_bytes,
            quantization,
            quantizations,
            downloads: item.downloads.map(format_compact_u64),
            likes: item.likes,
            description,
            tags: item.tags,
            pipeline_tag: item.pipeline_tag,
            fits_gpu,
            installed,
            source_url: Some(format!("https://huggingface.co/{}", item.model_id)),
        });
    }
    Ok(results)
}

fn list_installed_hf_models() -> Vec<InstalledLocalModel> {
    let root = config::local_models_hf_dir();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut models = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let metadata_path = entry.path().join(HF_METADATA_FILE);
        let Ok(raw) = std::fs::read_to_string(&metadata_path) else {
            continue;
        };
        let Ok(metadata) = serde_json::from_str::<HfLocalModelMetadata>(&raw) else {
            continue;
        };
        models.push(InstalledLocalModel {
            source: "huggingface".to_string(),
            backend: LocalBackendType::Vllm,
            model: metadata.model_id,
            size: dir_size_string(entry.path()),
            quantization: None,
            path: Some(metadata.local_dir),
            served: false,
        });
    }
    models
}

fn query_openai_models(base_url: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let response: OpenAiModelsResponse =
        http_client::get_with_timeout(&url, LOCAL_MODEL_HTTP_TIMEOUT)?
            .call()
            .map_err(|e| format!("model list request failed: {e}"))?
            .into_body()
            .read_json()
            .map_err(|e| format!("parse model list response: {e}"))?;
    Ok(response.data.into_iter().map(|item| item.id).collect())
}

fn mark_served_hf_models(
    models: Vec<InstalledLocalModel>,
    served_models: &[String],
) -> Vec<InstalledLocalModel> {
    models
        .into_iter()
        .map(|mut model| {
            model.served = served_models.iter().any(|served| served == &model.model);
            model
        })
        .collect()
}

fn normalize_source(source: Option<&str>) -> Result<LocalBackendType, String> {
    source
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("vllm")
        .parse()
}

fn command_version(command: &str, args: &[&str]) -> Option<String> {
    Command::new(command)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if stdout.is_empty() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if stderr.is_empty() {
                    None
                } else {
                    Some(stderr.lines().next().unwrap_or_default().to_string())
                }
            } else {
                Some(stdout.lines().next().unwrap_or_default().to_string())
            }
        })
}

fn which_path(command: &str) -> Option<String> {
    Command::new("which")
        .arg(command)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn detect_gpu() -> Option<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    let mut parts = line.split(',').map(|value| value.trim());
    let name = parts.next()?.to_string();
    let mem_mib = parts.next()?.parse::<f64>().ok()?;
    Some(GpuInfo {
        vendor: "nvidia".to_string(),
        name,
        total_memory_gib: mem_mib / 1024.0,
    })
}

fn hf_download_command(
    model: &str,
    local_dir: &Path,
    token: Option<&str>,
) -> Result<Command, String> {
    if which_path("huggingface-cli").is_some() {
        let mut command = Command::new("huggingface-cli");
        command.args([
            "download",
            model,
            "--local-dir",
            &local_dir.display().to_string(),
        ]);
        if let Some(token) = token {
            command.env(HF_TOKEN_ENV, token);
        }
        return Ok(command);
    }
    if which_path("hf").is_some() {
        let mut command = Command::new("hf");
        command.args([
            "download",
            model,
            "--local-dir",
            &local_dir.display().to_string(),
        ]);
        if let Some(token) = token {
            command.env(HF_TOKEN_ENV, token);
        }
        return Ok(command);
    }
    Err(
        "Hugging Face download requires `huggingface-cli` or `hf` on PATH; install `huggingface_hub` first"
            .to_string(),
    )
}

fn stream_child_output(
    child: &mut std::process::Child,
    writer: &mut dyn Write,
) -> Result<(), String> {
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    if let Some(ref mut pipe) = stdout {
        let _ = pipe.read_to_end(&mut stdout_buf);
    }
    if let Some(ref mut pipe) = stderr {
        let _ = pipe.read_to_end(&mut stderr_buf);
    }
    let status = child
        .wait()
        .map_err(|e| format!("wait on child process: {e}"))?;
    if !stdout_buf.is_empty() {
        let _ = writer.write_all(&stdout_buf);
    }
    if !stderr_buf.is_empty() {
        let _ = writer.write_all(&stderr_buf);
    }
    if !status.success() {
        return Err(format!("command exited with {:?}", status.code()));
    }
    Ok(())
}

fn read_token_from_file(path: &Path) -> Result<String, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("read token {}: {e}", path.display()))?;
    let token = raw.trim().to_string();
    if token.is_empty() {
        return Err(format!("token file {} is empty", path.display()));
    }
    Ok(token)
}

fn open_vault() -> Option<Vault> {
    let wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();
    if !wallet_path.exists() || !vault_path.exists() {
        return None;
    }
    Vault::open(&wallet_path, &vault_path).ok()
}

fn find_installed_hf_model_path(model: &str) -> Option<PathBuf> {
    let local_dir = hf_local_dir(model);
    if local_dir.exists() {
        Some(local_dir)
    } else {
        None
    }
}

fn hf_local_dir(model: &str) -> PathBuf {
    let encoded = model.replace('/', "__").replace(':', "_").replace(' ', "_");
    config::local_models_hf_dir().join(encoded)
}

fn dir_size_string(path: impl AsRef<Path>) -> Option<String> {
    let bytes = dir_size_bytes(path.as_ref()).ok()?;
    Some(format_bytes(bytes))
}

fn dir_size_bytes(path: &Path) -> Result<u64, String> {
    let mut total = 0u64;
    let entries =
        std::fs::read_dir(path).map_err(|e| format!("read dir {}: {e}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read dir entry {}: {e}", path.display()))?;
        let meta = entry
            .metadata()
            .map_err(|e| format!("metadata {}: {e}", entry.path().display()))?;
        if meta.is_dir() {
            total = total.saturating_add(dir_size_bytes(&entry.path())?);
        } else {
            total = total.saturating_add(meta.len());
        }
    }
    Ok(total)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        file.write_all(bytes)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        file.flush()
            .map_err(|e| format!("flush {}: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut idx = 0usize;
    while value >= 1024.0 && idx + 1 < UNITS.len() {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{bytes} {}", UNITS[idx])
    } else {
        format!("{value:.1} {}", UNITS[idx])
    }
}

fn format_compact_u64(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn quantizations_from_tags(tags: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    for tag in tags {
        let normalized = tag.to_ascii_lowercase();
        let value = if normalized.contains("4bit") {
            Some("4-bit")
        } else if normalized.contains("8bit") {
            Some("8-bit")
        } else if normalized.contains("awq") {
            Some("AWQ")
        } else if normalized.contains("gptq") {
            Some("GPTQ")
        } else if normalized.contains("gguf") {
            Some("GGUF")
        } else if normalized.contains("fp16") {
            Some("FP16")
        } else if normalized.contains("bf16") {
            Some("BF16")
        } else {
            None
        };
        if let Some(value) = value {
            let value = value.to_string();
            if !values.contains(&value) {
                values.push(value);
            }
        }
    }
    values
}

fn urlencoding(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_source_accepts_legacy_aliases() {
        assert_eq!(
            normalize_source(None).expect("default"),
            LocalBackendType::Vllm
        );
        assert_eq!(
            normalize_source(Some("hf")).expect("hf alias"),
            LocalBackendType::Vllm
        );
        assert_eq!(
            normalize_source(Some("ollama")).expect("legacy alias"),
            LocalBackendType::Vllm
        );
    }

    #[test]
    fn format_bytes_uses_human_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn legacy_single_managed_state_deserializes() {
        let cfg: LocalModelsConfig = serde_json::from_str(
            r#"{
                "preferred_backend": "ollama",
                "ollama_port": 11434,
                "vllm_port": 8000,
                "hf_token_path": "/tmp/token",
                "managed": {
                    "backend": "ollama",
                    "port": 11434,
                    "pid": 1234,
                    "started_at_unix": 42
                }
            }"#,
        )
        .expect("deserialize config");
        assert_eq!(cfg.vllm_port, 8000);
        assert_eq!(cfg.managed.as_ref().and_then(|state| state.pid), Some(1234));
    }

    #[test]
    fn quantizations_collect_unique_markers() {
        let tags = vec![
            "4bit".to_string(),
            "awq".to_string(),
            "awq".to_string(),
            "fp16".to_string(),
        ];
        assert_eq!(quantizations_from_tags(&tags), vec!["4-bit", "AWQ", "FP16"]);
    }
}
