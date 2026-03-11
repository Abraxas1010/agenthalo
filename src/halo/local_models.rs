//! Local model discovery, cataloging, and managed serving for Ollama/vLLM.
//!
//! AgentHALO treats local inference as another upstream backend. This module
//! owns the durable config/state, CLI-friendly orchestration, and lightweight
//! discovery helpers used by the proxy, dashboard, and doctor surfaces.

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

const DEFAULT_OLLAMA_PORT: u16 = 11434;
const DEFAULT_VLLM_PORT: u16 = 8000;
const HF_TOKEN_ENV: &str = "HF_TOKEN";
const HF_FALLBACK_TOKEN_PATH: &str = ".cache/huggingface/token";
const HF_METADATA_FILE: &str = ".agenthalo-model.json";
const LOCAL_MODEL_HTTP_TIMEOUT: Duration = Duration::from_secs(5);
const LOCAL_MODEL_CACHE_TTL: Duration = Duration::from_secs(10);
const LOCAL_MODEL_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const LOCAL_MODEL_POLL_INTERVAL: Duration = Duration::from_millis(250);
const LOCAL_MODEL_STDERR_TAIL_BYTES: usize = 4096;

#[derive(Default)]
struct InstalledModelCache {
    refreshed_at: Option<Instant>,
    models: Vec<InstalledLocalModel>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LocalBackendType {
    Ollama,
    Vllm,
}

impl LocalBackendType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::Vllm => "vllm",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::Vllm => "vLLM",
        }
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
            "ollama" => Ok(Self::Ollama),
            "vllm" => Ok(Self::Vllm),
            other => Err(format!("unknown backend `{other}`; expected ollama|vllm")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ManagedServeState {
    pub backend: LocalBackendType,
    pub port: u16,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    pub started_at_unix: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstalledModelHint {
    pub backend: LocalBackendType,
    pub model: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum ManagedServeStateField {
    One(ManagedServeState),
    Many(Vec<ManagedServeState>),
}

fn deserialize_managed_states<'de, D>(deserializer: D) -> Result<Vec<ManagedServeState>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<ManagedServeStateField>::deserialize(deserializer)?;
    Ok(match value {
        Some(ManagedServeStateField::One(item)) => vec![item],
        Some(ManagedServeStateField::Many(items)) => items,
        None => Vec::new(),
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LocalModelsConfig {
    pub preferred_backend: LocalBackendType,
    pub ollama_port: u16,
    pub vllm_port: u16,
    pub hf_token_path: String,
    #[serde(default)]
    pub vllm_default_model: Option<String>,
    #[serde(default, deserialize_with = "deserialize_managed_states")]
    pub managed: Vec<ManagedServeState>,
    #[serde(default)]
    pub installed_hints: Vec<InstalledModelHint>,
    #[serde(default)]
    pub local_compute_cost_per_1k_tokens_usd: f64,
}

impl Default for LocalModelsConfig {
    fn default() -> Self {
        Self {
            preferred_backend: LocalBackendType::Ollama,
            ollama_port: DEFAULT_OLLAMA_PORT,
            vllm_port: DEFAULT_VLLM_PORT,
            hf_token_path: default_hf_token_path().display().to_string(),
            vllm_default_model: None,
            managed: Vec::new(),
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
    pub backend: LocalBackendType,
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
    pub ollama: BackendStatus,
    pub vllm: BackendStatus,
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
    pub quantization: Option<String>,
    #[serde(default)]
    pub downloads: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fits_gpu: Option<bool>,
    #[serde(default)]
    pub source_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServeRequest {
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
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTag>,
}

#[derive(Clone, Debug, Deserialize)]
struct OllamaTag {
    name: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    details: Option<OllamaDetails>,
}

#[derive(Clone, Debug, Deserialize)]
struct OllamaDetails {
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    quantization_level: Option<String>,
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
            backend: model.backend,
            model: model.model.clone(),
        })
        .collect::<Vec<_>>();
    hints.sort_by(|a, b| {
        a.model
            .cmp(&b.model)
            .then(a.backend.as_str().cmp(b.backend.as_str()))
    });
    hints.dedup_by(|a, b| a.model == b.model && a.backend == b.backend);
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

fn backend_from_hints(model: &str, hints: &[InstalledModelHint]) -> Option<LocalBackendType> {
    hints
        .iter()
        .find(|hint| hint.model == model)
        .map(|hint| hint.backend)
}

fn upsert_installed_hint(cfg: &mut LocalModelsConfig, hint: InstalledModelHint) -> bool {
    if let Some(existing) = cfg
        .installed_hints
        .iter_mut()
        .find(|existing| existing.backend == hint.backend && existing.model == hint.model)
    {
        *existing = hint;
        return false;
    }
    cfg.installed_hints.push(hint);
    cfg.installed_hints.sort_by(|a, b| {
        a.model
            .cmp(&b.model)
            .then(a.backend.as_str().cmp(b.backend.as_str()))
    });
    true
}

fn remove_installed_hint(
    cfg: &mut LocalModelsConfig,
    backend: LocalBackendType,
    model: &str,
) -> bool {
    let before = cfg.installed_hints.len();
    cfg.installed_hints
        .retain(|hint| !(hint.backend == backend && hint.model == model));
    before != cfg.installed_hints.len()
}

fn managed_state(cfg: &LocalModelsConfig, backend: LocalBackendType) -> Option<&ManagedServeState> {
    cfg.managed
        .iter()
        .find(|managed| managed.backend == backend)
}

fn upsert_managed_state(cfg: &mut LocalModelsConfig, state: ManagedServeState) {
    if let Some(existing) = cfg
        .managed
        .iter_mut()
        .find(|managed| managed.backend == state.backend)
    {
        *existing = state;
        return;
    }
    cfg.managed.push(state);
    cfg.managed
        .sort_by(|a, b| a.backend.as_str().cmp(b.backend.as_str()));
}

fn remove_managed_state(cfg: &mut LocalModelsConfig, backend: LocalBackendType) -> bool {
    let before = cfg.managed.len();
    cfg.managed.retain(|managed| managed.backend != backend);
    before != cfg.managed.len()
}

fn refresh_installed_models_snapshot(cfg: &LocalModelsConfig) -> Vec<InstalledLocalModel> {
    let mut models = query_ollama_models(&base_url(LocalBackendType::Ollama, cfg.ollama_port))
        .unwrap_or_default();
    let installed_hf = list_installed_hf_models();
    let served_vllm =
        query_openai_models(&base_url(LocalBackendType::Vllm, cfg.vllm_port)).unwrap_or_default();
    models.extend(mark_served_hf_models(installed_hf, &served_vllm));
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
        if let Some(item) = cache.models.iter().find(|item| item.model == normalized) {
            return Some(item.backend);
        }
    }
    let cfg = load_or_default();
    backend_from_hints(normalized, &cfg.installed_hints)
}

pub fn detect_status() -> LocalModelsStatus {
    let cfg = load_or_default();
    let hf_token = resolve_hf_token_with_config(&cfg).ok();
    let gpu = detect_gpu();
    let installed_hf = list_installed_hf_models();
    let vllm_served = query_openai_models(&base_url(LocalBackendType::Vllm, cfg.vllm_port));
    let ollama_served = query_ollama_models(&base_url(LocalBackendType::Ollama, cfg.ollama_port));

    let ollama_error = ollama_served.as_ref().err().cloned();
    let ollama = BackendStatus {
        backend: LocalBackendType::Ollama,
        cli_installed: which_path("ollama").is_some(),
        cli_path: which_path("ollama"),
        cli_version: command_version("ollama", &["--version"]),
        base_url: base_url(LocalBackendType::Ollama, cfg.ollama_port),
        healthy: ollama_served.is_ok(),
        served_models: ollama_served
            .as_ref()
            .map(|items| items.iter().map(|item| item.model.clone()).collect())
            .unwrap_or_default(),
        installed_models: ollama_served.unwrap_or_default(),
        managed: managed_state(&cfg, LocalBackendType::Ollama).cloned(),
        error: ollama_error,
    };
    let served_vllm_models = vllm_served.unwrap_or_default();
    let vllm = BackendStatus {
        backend: LocalBackendType::Vllm,
        cli_installed: which_path("vllm").is_some(),
        cli_path: which_path("vllm"),
        cli_version: command_version("vllm", &["--version"]),
        base_url: base_url(LocalBackendType::Vllm, cfg.vllm_port),
        healthy: !served_vllm_models.is_empty(),
        served_models: served_vllm_models.clone(),
        installed_models: mark_served_hf_models(installed_hf, &served_vllm_models),
        managed: managed_state(&cfg, LocalBackendType::Vllm).cloned(),
        error: if served_vllm_models.is_empty() {
            Some("vLLM API not responding at configured endpoint".to_string())
        } else {
            None
        },
    };
    LocalModelsStatus {
        config: cfg,
        huggingface_token_configured: hf_token
            .as_ref()
            .map(|token| !token.trim().is_empty())
            .unwrap_or(false),
        gpu,
        ollama,
        vllm,
    }
}

pub fn search_models(query: &str, limit: usize) -> Result<Vec<ModelSearchResult>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("search query must not be empty".to_string());
    }
    let cfg = load_or_default();
    let gpu = detect_gpu();
    let ollama = search_ollama(query, limit)?;
    let huggingface = search_huggingface(query, limit, gpu.as_ref(), &cfg)?;
    let mut results = Vec::new();
    let mut ollama_iter = ollama.into_iter();
    let mut hf_iter = huggingface.into_iter();
    while results.len() < limit {
        let mut progressed = false;
        if let Some(item) = ollama_iter.next() {
            results.push(item);
            progressed = true;
            if results.len() >= limit {
                break;
            }
        }
        if let Some(item) = hf_iter.next() {
            results.push(item);
            progressed = true;
        }
        if !progressed {
            break;
        }
    }
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
                "backend": model.backend.as_str(),
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
    config::ensure_halo_dir()?;
    let source = normalize_source(source, model)?;
    match source {
        LocalBackendType::Ollama => {
            let mut child = Command::new("ollama")
                .args(["pull", model])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("start `ollama pull {model}`: {e}"))?;
            stream_child_output(&mut child, writer)?;
            let installed = query_ollama_models(&base_url(
                LocalBackendType::Ollama,
                load_or_default().ollama_port,
            ))
            .unwrap_or_default()
            .into_iter()
            .find(|item| item.model == model)
            .unwrap_or(InstalledLocalModel {
                source: "ollama".to_string(),
                backend: LocalBackendType::Ollama,
                model: model.to_string(),
                size: None,
                quantization: None,
                path: None,
                served: false,
            });
            invalidate_installed_models_cache();
            let mut cfg = load_or_default();
            if upsert_installed_hint(
                &mut cfg,
                InstalledModelHint {
                    backend: LocalBackendType::Ollama,
                    model: installed.model.clone(),
                },
            ) {
                save_config(&cfg)?;
            }
            Ok(installed)
        }
        LocalBackendType::Vllm => {
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
            if upsert_installed_hint(
                &mut cfg,
                InstalledModelHint {
                    backend: LocalBackendType::Vllm,
                    model: model.to_string(),
                },
            ) {
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
    }
}

pub fn remove_model(model: &str, source: Option<&str>) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("model name must not be empty".to_string());
    }
    let source = normalize_source(source, model)?;
    match source {
        LocalBackendType::Ollama => {
            let status = Command::new("ollama")
                .args(["rm", model])
                .status()
                .map_err(|e| format!("run `ollama rm {model}`: {e}"))?;
            if !status.success() {
                return Err(format!(
                    "`ollama rm {model}` exited with {:?}",
                    status.code()
                ));
            }
            invalidate_installed_models_cache();
            let mut cfg = load_or_default();
            if remove_installed_hint(&mut cfg, LocalBackendType::Ollama, model) {
                save_config(&cfg)?;
            }
            Ok(())
        }
        LocalBackendType::Vllm => {
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
            if remove_installed_hint(&mut cfg, LocalBackendType::Vllm, model) {
                save_config(&cfg)?;
            }
            Ok(())
        }
    }
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
    let port = request.port.unwrap_or(match request.backend {
        LocalBackendType::Ollama => cfg.ollama_port,
        LocalBackendType::Vllm => cfg.vllm_port,
    });
    let base_url = base_url(request.backend, port);
    clear_stale_managed_state(&mut cfg, request.backend, port, &base_url);
    let mut served_model = request.model.clone();

    if is_backend_healthy(request.backend, &base_url) {
        return Ok(ServeResult {
            backend: request.backend,
            base_url,
            port,
            model: served_model,
            already_running: true,
            pid: managed_state(&cfg, request.backend)
                .filter(|managed| managed.port == port)
                .and_then(|managed| managed.pid),
        });
    }

    let mut command = match request.backend {
        LocalBackendType::Ollama => {
            let mut cmd = Command::new("ollama");
            cmd.arg("serve")
                .env("OLLAMA_HOST", format!("127.0.0.1:{port}"));
            cmd
        }
        LocalBackendType::Vllm => {
            let model = request
                .model
                .clone()
                .or_else(|| cfg.vllm_default_model.clone())
                .ok_or_else(|| {
                    "vLLM serving requires --model <huggingface-model> or a configured default"
                        .to_string()
                })?;
            cfg.vllm_default_model = Some(model.clone());
            served_model = Some(model.clone());
            let model_arg = find_installed_hf_model_path(&model)
                .unwrap_or_else(|| PathBuf::from(model.clone()))
                .display()
                .to_string();
            let mut cmd = Command::new("vllm");
            cmd.args([
                "serve",
                &model_arg,
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ]);
            if let Ok(token) = resolve_hf_token_with_config(&cfg) {
                cmd.env(HF_TOKEN_ENV, token);
            }
            cmd
        }
    };

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {} serve: {e}", request.backend.display_name()))?;
    let pid = Some(child.id());
    wait_for_backend_start(
        request.backend,
        &base_url,
        &mut child,
        LOCAL_MODEL_STARTUP_TIMEOUT,
    )?;
    drop(child);
    cfg.preferred_backend = request.backend;
    match request.backend {
        LocalBackendType::Ollama => cfg.ollama_port = port,
        LocalBackendType::Vllm => cfg.vllm_port = port,
    }
    upsert_managed_state(
        &mut cfg,
        ManagedServeState {
            backend: request.backend,
            port,
            model: served_model.clone(),
            pid,
            started_at_unix: now_unix_secs(),
        },
    );
    save_config(&cfg)?;
    Ok(ServeResult {
        backend: request.backend,
        base_url,
        port,
        model: served_model,
        already_running: false,
        pid,
    })
}

pub fn stop_backend(backend: LocalBackendType) -> Result<StopResult, String> {
    let mut cfg = load_or_default();
    let port = match backend {
        LocalBackendType::Ollama => cfg.ollama_port,
        LocalBackendType::Vllm => cfg.vllm_port,
    };
    let base_url = base_url(backend, port);
    let cleared_stale = clear_stale_managed_state(&mut cfg, backend, port, &base_url);
    if cleared_stale {
        save_config(&cfg)?;
    }
    let managed = managed_state(&cfg, backend).cloned().ok_or_else(|| {
        format!(
            "no managed {} process recorded; if it is still running, stop it externally",
            backend.display_name()
        )
    })?;

    if let Some(pid) = managed.pid {
        terminate_pid(pid)?;
        wait_for_backend_stop(backend, &base_url, Duration::from_secs(10))?;
    }

    if remove_managed_state(&mut cfg, backend) {
        save_config(&cfg)?;
    }

    Ok(StopResult {
        backend,
        stopped: true,
        pid: managed.pid,
        base_url,
        message: format!("stopped managed {}", backend.display_name()),
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
    let (hint, requested_model) = if let Some(rest) = stripped.strip_prefix("ollama/") {
        (Some(LocalBackendType::Ollama), rest.trim())
    } else if let Some(rest) = stripped.strip_prefix("vllm/") {
        (Some(LocalBackendType::Vllm), rest.trim())
    } else {
        (None, stripped)
    };
    if requested_model.is_empty() {
        return Err("local model name must not be empty".to_string());
    }

    let explicit_backend = installed_backend_for_model(requested_model);
    let backend = hint.or(explicit_backend).unwrap_or(cfg.preferred_backend);
    let base_url = base_url(
        backend,
        match backend {
            LocalBackendType::Ollama => cfg.ollama_port,
            LocalBackendType::Vllm => cfg.vllm_port,
        },
    );
    if !is_backend_healthy(backend, &base_url) {
        return Err(format!(
            "{} is not healthy at {}; run `agenthalo models serve --backend {}` first",
            backend.display_name(),
            base_url,
            backend.as_str()
        ));
    }
    if backend == LocalBackendType::Vllm {
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
    }
    Ok(ResolvedLocalRoute {
        backend,
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

fn base_url(backend: LocalBackendType, port: u16) -> String {
    match backend {
        LocalBackendType::Ollama => format!("http://127.0.0.1:{port}"),
        LocalBackendType::Vllm => format!("http://127.0.0.1:{port}"),
    }
}

fn is_backend_healthy(backend: LocalBackendType, base_url: &str) -> bool {
    match backend {
        LocalBackendType::Ollama => query_ollama_models(base_url).is_ok(),
        LocalBackendType::Vllm => query_openai_models(base_url)
            .map(|models| !models.is_empty())
            .unwrap_or(false),
    }
}

fn wait_for_backend_start(
    backend: LocalBackendType,
    base_url: &str,
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if is_backend_healthy(backend, base_url) {
            invalidate_installed_models_cache();
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("poll {} startup: {e}", backend.display_name()))?
        {
            let stderr = read_pipe_tail(child.stderr.take());
            let code = status
                .code()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "signal".to_string());
            return Err(if stderr.is_empty() {
                format!(
                    "{} exited before becoming healthy (exit {code})",
                    backend.display_name()
                )
            } else {
                format!(
                    "{} exited before becoming healthy (exit {code}): {}",
                    backend.display_name(),
                    stderr
                )
            });
        }
        std::thread::sleep(LOCAL_MODEL_POLL_INTERVAL);
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(format!(
        "{} did not become healthy within {}s",
        backend.display_name(),
        timeout.as_secs()
    ))
}

fn wait_for_backend_stop(
    backend: LocalBackendType,
    base_url: &str,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !is_backend_healthy(backend, base_url) {
            invalidate_installed_models_cache();
            return Ok(());
        }
        std::thread::sleep(LOCAL_MODEL_POLL_INTERVAL);
    }
    Err(format!(
        "{} did not stop within {}s",
        backend.display_name(),
        timeout.as_secs()
    ))
}

fn clear_stale_managed_state(
    cfg: &mut LocalModelsConfig,
    backend: LocalBackendType,
    port: u16,
    base_url: &str,
) -> bool {
    let Some(state) = managed_state(cfg, backend).cloned() else {
        return false;
    };
    if state.port == port && !is_backend_healthy(backend, base_url) {
        return remove_managed_state(cfg, backend);
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

fn search_ollama(query: &str, limit: usize) -> Result<Vec<ModelSearchResult>, String> {
    let url = format!("https://ollama.com/search?q={}", urlencoding(query));
    let html = http_client::get_with_timeout(&url, LOCAL_MODEL_HTTP_TIMEOUT)?
        .call()
        .map_err(|e| format!("ollama search request failed: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("read Ollama search response: {e}"))?;
    let mut results = Vec::new();
    for block in extract_blocks(&html, "<li x-test-model", "</li>")
        .into_iter()
        .take(limit)
    {
        let model = extract_marker_text(&block, "x-test-search-response-title")
            .or_else(|| extract_between(&block, "title=\"", "\""))
            .unwrap_or_default();
        if model.is_empty() {
            continue;
        }
        let description = extract_first_paragraph(&block);
        let sizes = extract_all_marker_text(&block, "x-test-size");
        let downloads = extract_marker_text(&block, "x-test-pull-count");
        let href = extract_between(&block, "href=\"", "\"")
            .map(|path| format!("https://ollama.com{}", path));
        results.push(ModelSearchResult {
            index: 0,
            source: "ollama".to_string(),
            backend: LocalBackendType::Ollama,
            model,
            size: if sizes.is_empty() {
                None
            } else {
                Some(sizes.join(", "))
            },
            quantization: Some("GGUF".to_string()),
            downloads,
            description,
            fits_gpu: None,
            source_url: href,
        });
    }
    Ok(results)
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
                let required_bytes = size
                    .saturating_mul(2)
                    .saturating_add(2 * 1024 * 1024 * 1024);
                Some(required_bytes as f64 <= gpu.total_memory_gib * 1024f64.powi(3))
            }
            _ => None,
        };
        let description = Some(format!(
            "{}{}{}",
            item.pipeline_tag
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
            quantization: quantization_from_tags(&item.tags),
            downloads: item.downloads.map(format_compact_u64),
            description,
            fits_gpu,
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

fn query_ollama_models(base_url: &str) -> Result<Vec<InstalledLocalModel>, String> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let response: OllamaTagsResponse =
        http_client::get_with_timeout(&url, LOCAL_MODEL_HTTP_TIMEOUT)?
            .call()
            .map_err(|e| format!("ollama tags request failed: {e}"))?
            .into_body()
            .read_json()
            .map_err(|e| format!("parse Ollama tags response: {e}"))?;
    Ok(response
        .models
        .into_iter()
        .map(|model| InstalledLocalModel {
            source: "ollama".to_string(),
            backend: LocalBackendType::Ollama,
            model: model.name.clone(),
            size: model.size.map(format_bytes).or_else(|| {
                model
                    .details
                    .as_ref()
                    .and_then(|details| details.parameter_size.clone())
            }),
            quantization: model
                .details
                .as_ref()
                .and_then(|details| details.quantization_level.clone()),
            path: None,
            served: true,
        })
        .collect())
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

fn normalize_source(source: Option<&str>, model: &str) -> Result<LocalBackendType, String> {
    if let Some(source) = source {
        if source.eq_ignore_ascii_case("hf") {
            return Ok(LocalBackendType::Vllm);
        }
        return source.parse();
    }
    if model.contains('/') {
        Ok(LocalBackendType::Vllm)
    } else {
        Ok(LocalBackendType::Ollama)
    }
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

fn quantization_from_tags(tags: &[String]) -> Option<String> {
    for tag in tags {
        let normalized = tag.to_ascii_lowercase();
        if normalized.contains("4bit") {
            return Some("4-bit".to_string());
        }
        if normalized.contains("8bit") {
            return Some("8-bit".to_string());
        }
        if normalized.contains("awq") {
            return Some("AWQ".to_string());
        }
        if normalized.contains("gptq") {
            return Some("GPTQ".to_string());
        }
        if normalized.contains("gguf") {
            return Some("GGUF".to_string());
        }
        if normalized.contains("fp16") {
            return Some("FP16".to_string());
        }
    }
    None
}

fn extract_blocks(haystack: &str, start: &str, end: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut remainder = haystack;
    while let Some(start_idx) = remainder.find(start) {
        let chunk = &remainder[start_idx..];
        if let Some(end_idx) = chunk.find(end) {
            blocks.push(chunk[..end_idx + end.len()].to_string());
            remainder = &chunk[end_idx + end.len()..];
        } else {
            break;
        }
    }
    blocks
}

fn extract_marker_text(block: &str, marker: &str) -> Option<String> {
    let search = format!("{marker}>");
    let idx = block.find(&search)?;
    let rest = &block[idx + search.len()..];
    extract_html_text(rest).map(|text| text.trim().to_string())
}

fn extract_all_marker_text(block: &str, marker: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = block;
    let search = format!("{marker}>");
    while let Some(idx) = rest.find(&search) {
        let tail = &rest[idx + search.len()..];
        if let Some(value) = extract_html_text(tail) {
            values.push(value.trim().to_string());
        }
        rest = tail;
    }
    values
}

fn extract_first_paragraph(block: &str) -> Option<String> {
    let start = block.find("<p")?;
    let rest = &block[start..];
    let open_end = rest.find('>')?;
    extract_html_text(&rest[open_end + 1..]).map(|value| value.trim().to_string())
}

fn extract_html_text(text: &str) -> Option<String> {
    let end = text.find('<')?;
    Some(html_unescape(&text[..end]))
}

fn extract_between(haystack: &str, start: &str, end: &str) -> Option<String> {
    let start_idx = haystack.find(start)? + start.len();
    let tail = &haystack[start_idx..];
    let end_idx = tail.find(end)?;
    Some(html_unescape(&tail[..end_idx]))
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
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
    fn html_search_parser_extracts_ollama_model_details() {
        let html = r#"
        <li x-test-model class="flex">
          <a href="/library/qwen2.5-coder" class="group w-full">
            <div><span x-test-search-response-title>qwen2.5-coder</span></div>
            <p class="desc">Coding model.</p>
            <span x-test-size>7b</span>
            <span x-test-size>14b</span>
            <span x-test-pull-count>1.2M</span>
          </a>
        </li>
        "#;
        let items = extract_blocks(html, "<li x-test-model", "</li>");
        assert_eq!(items.len(), 1);
        assert_eq!(
            extract_marker_text(&items[0], "x-test-search-response-title").as_deref(),
            Some("qwen2.5-coder")
        );
        assert_eq!(
            extract_all_marker_text(&items[0], "x-test-size"),
            vec!["7b", "14b"]
        );
        assert_eq!(
            extract_marker_text(&items[0], "x-test-pull-count").as_deref(),
            Some("1.2M")
        );
    }

    #[test]
    fn normalize_source_prefers_hf_for_repo_ids() {
        assert_eq!(
            normalize_source(None, "Qwen/Qwen2.5-Coder-7B").expect("hf source"),
            LocalBackendType::Vllm
        );
        assert_eq!(
            normalize_source(None, "qwen2.5-coder:7b").expect("ollama source"),
            LocalBackendType::Ollama
        );
    }

    #[test]
    fn normalize_source_accepts_hf_alias() {
        assert_eq!(
            normalize_source(Some("hf"), "Qwen/Qwen2.5-Coder-7B").expect("hf alias"),
            LocalBackendType::Vllm
        );
    }

    #[test]
    fn format_bytes_uses_human_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn legacy_single_managed_state_deserializes_into_vector() {
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
        assert_eq!(cfg.managed.len(), 1);
        assert_eq!(cfg.managed[0].backend, LocalBackendType::Ollama);
        assert_eq!(cfg.managed[0].pid, Some(1234));
    }

    #[test]
    fn backend_from_hints_matches_persisted_hint_without_probe() {
        let hints = vec![InstalledModelHint {
            backend: LocalBackendType::Ollama,
            model: "llama3.1".to_string(),
        }];
        assert_eq!(
            backend_from_hints("llama3.1", &hints),
            Some(LocalBackendType::Ollama)
        );
        assert_eq!(backend_from_hints("missing", &hints), None);
    }
}
