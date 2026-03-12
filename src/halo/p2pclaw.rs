use crate::halo::config;
use crate::halo::http_client;
use crate::halo::vault::Vault;
use hmac::{Hmac, Mac};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;
use zeroize::Zeroize;

type HmacSha256 = Hmac<Sha256>;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BODY_BYTES: u64 = 10 * 1024 * 1024;
const MAX_ERROR_BODY_BYTES: u64 = 16 * 1024;
pub const P2PCLAW_VAULT_KEY: &str = "p2pclaw_auth";
pub const P2PCLAW_VAULT_ENV: &str = "P2PCLAW_AUTH_SECRET";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct P2PClawConfig {
    /// Base URL of the P2PCLAW gateway.
    pub endpoint_url: String,
    /// Agent ID used when interacting with the hive.
    pub agent_id: String,
    /// Display name shown to other hive members.
    pub agent_name: String,
    /// True when an auth secret is configured (vault-first, insecure fallback).
    pub auth_configured: bool,
    /// Tier selection: "tier1" or "tier2".
    pub tier: String,
    /// Last successful connection timestamp (unix seconds).
    pub last_connected_at: u64,
}

impl Default for P2PClawConfig {
    fn default() -> Self {
        Self {
            endpoint_url: "https://p2pclaw.com".to_string(),
            agent_id: "agenthalo".to_string(),
            agent_name: "AgentHALO".to_string(),
            auth_configured: false,
            tier: "tier1".to_string(),
            last_connected_at: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct P2PClawDiskConfig {
    endpoint_url: String,
    agent_id: String,
    agent_name: String,
    auth_configured: bool,
    tier: String,
    last_connected_at: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "auth_secret_INSECURE"
    )]
    auth_secret_insecure: Option<String>,
}

impl From<P2PClawDiskConfig> for P2PClawConfig {
    fn from(value: P2PClawDiskConfig) -> Self {
        Self {
            endpoint_url: value.endpoint_url,
            agent_id: value.agent_id,
            agent_name: value.agent_name,
            auth_configured: value.auth_configured,
            tier: value.tier,
            last_connected_at: value.last_connected_at,
        }
    }
}

impl P2PClawDiskConfig {
    fn from_config(cfg: &P2PClawConfig, auth_secret_insecure: Option<String>) -> Self {
        Self {
            endpoint_url: cfg.endpoint_url.clone(),
            agent_id: cfg.agent_id.clone(),
            agent_name: cfg.agent_name.clone(),
            auth_configured: cfg.auth_configured,
            tier: cfg.tier.clone(),
            last_connected_at: cfg.last_connected_at,
            auth_secret_insecure,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SwarmStatus {
    pub agents: u64,
    pub papers: u64,
    pub mempool: u64,
    #[serde(default)]
    pub last_event_ts: Option<u64>,
    #[serde(default)]
    pub raw: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Paper {
    #[serde(default, alias = "paperId", alias = "id")]
    pub paper_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub timestamp: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PaperResult {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, alias = "paperId")]
    pub paper_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ValidationResult {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, alias = "paperId")]
    pub paper_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct HiveEvent {
    #[serde(default, alias = "eventId", alias = "id")]
    pub event_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub timestamp: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Investigation {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AgentRank {
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub rank: Option<String>,
    #[serde(default)]
    pub contributions: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct InvestigationCreateResult {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WheelResult {
    #[serde(rename = "isDuplicate", default)]
    pub is_duplicate: bool,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub meta: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub fn validate_endpoint(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    let parsed = Url::parse(trimmed).map_err(|e| format!("invalid P2PCLAW endpoint URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        other => Err(format!(
            "P2PCLAW endpoint must use http:// or https:// scheme, got: {other}"
        )),
    }
}

pub fn load_config() -> Result<P2PClawConfig, String> {
    let cfg = load_disk_config_optional()?.ok_or_else(|| {
        "P2PCLAW is not configured. Configure via p2pclaw_configure or dashboard networking page."
            .to_string()
    })?;
    Ok(cfg.into())
}

pub fn load_or_default() -> P2PClawConfig {
    load_config().unwrap_or_default()
}

pub fn save_config(cfg: &P2PClawConfig) -> Result<(), String> {
    validate_endpoint(&cfg.endpoint_url)?;
    let insecure = load_disk_config_optional()?.and_then(|disk| disk.auth_secret_insecure);
    save_config_with_insecure_secret(cfg, insecure)
}

pub fn save_config_with_insecure_secret(
    cfg: &P2PClawConfig,
    auth_secret_insecure: Option<String>,
) -> Result<(), String> {
    validate_endpoint(&cfg.endpoint_url)?;
    config::ensure_halo_dir()?;
    let path = config::p2pclaw_config_path();
    let disk = P2PClawDiskConfig::from_config(cfg, auth_secret_insecure);
    let raw = serde_json::to_string_pretty(&disk)
        .map_err(|e| format!("serialize p2pclaw config {}: {e}", path.display()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod 600 {}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

pub fn get_auth_secret(vault: Option<&Vault>) -> Result<Option<String>, String> {
    if let Some(v) = vault {
        if let Ok(secret) = v.get_key(P2PCLAW_VAULT_KEY) {
            return Ok(Some(secret));
        }
    }
    if let Some(disk) = load_disk_config_optional()? {
        if let Some(secret) = disk.auth_secret_insecure {
            eprintln!(
                "[AgentHalo/P2PCLAW] WARNING: reading auth secret from UNENCRYPTED config. \
                 Initialize vault (`agenthalo vault init`) for encrypted storage."
            );
            return Ok(Some(secret));
        }
    }
    Ok(None)
}

pub fn configure(
    cfg: &mut P2PClawConfig,
    auth_secret: Option<String>,
) -> Result<ConfigureResult, String> {
    validate_endpoint(&cfg.endpoint_url)?;
    let mut secret = auth_secret.unwrap_or_default();
    if secret.trim().is_empty() {
        save_config(cfg)?;
        secret.zeroize();
        return Ok(ConfigureResult {
            auth_in_vault: false,
            auth_configured: cfg.auth_configured,
        });
    }
    cfg.auth_configured = true;
    let mut insecure_secret = None::<String>;
    let mut auth_in_vault = false;
    match open_vault() {
        Some(v) => match v.set_key(P2PCLAW_VAULT_KEY, P2PCLAW_VAULT_ENV, &secret) {
            Ok(_) => auth_in_vault = true,
            Err(e) => {
                eprintln!(
                    "[AgentHalo/P2PCLAW] WARNING: failed to store vault secret ({e}); \
                     storing auth_secret_INSECURE in config. Run `agenthalo vault init`."
                );
                insecure_secret = Some(secret.clone());
            }
        },
        None => {
            eprintln!(
                "[AgentHalo/P2PCLAW] WARNING: vault unavailable; storing auth_secret_INSECURE \
                 in config. Run `agenthalo vault init`."
            );
            insecure_secret = Some(secret.clone());
        }
    }
    save_config_with_insecure_secret(cfg, insecure_secret.clone())?;
    if let Some(ref mut insecure) = insecure_secret {
        insecure.zeroize();
    }
    secret.zeroize();
    Ok(ConfigureResult {
        auth_in_vault,
        auth_configured: true,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ConfigureResult {
    pub auth_in_vault: bool,
    pub auth_configured: bool,
}

pub fn ping(cfg: &P2PClawConfig) -> Result<SwarmStatus, String> {
    let raw = request_get_json(cfg, "/swarm-status", &[])?;
    Ok(SwarmStatus {
        agents: extract_u64(
            &raw,
            &[&["agents"], &["active_agents"], &["swarm", "active_agents"]],
        )
        .unwrap_or(0),
        papers: extract_u64(
            &raw,
            &[
                &["papers"],
                &["papers_in_la_rueda"],
                &["swarm", "papers_in_la_rueda"],
            ],
        )
        .unwrap_or(0),
        mempool: extract_u64(
            &raw,
            &[
                &["mempool"],
                &["papers_in_mempool"],
                &["swarm", "papers_in_mempool"],
            ],
        )
        .unwrap_or(0),
        last_event_ts: extract_u64(&raw, &[&["timestamp"], &["swarm", "last_seen"]]),
        raw,
    })
}

pub fn list_papers(cfg: &P2PClawConfig, limit: Option<u64>) -> Result<Vec<Paper>, String> {
    let mut query = Vec::new();
    if let Some(v) = limit {
        query.push(("limit", v.to_string()));
    }
    let raw = request_get_json(cfg, "/latest-papers", &query)?;
    parse_list_from_value(raw, &["papers", "latest"])
}

pub fn list_mempool(cfg: &P2PClawConfig) -> Result<Vec<Paper>, String> {
    let raw = request_get_json(cfg, "/mempool", &[])?;
    parse_list_from_value(raw, &["papers", "mempool"])
}

pub fn get_agent_rank(cfg: &P2PClawConfig, agent_id: Option<&str>) -> Result<AgentRank, String> {
    let agent = agent_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(cfg.agent_id.as_str());
    let raw = request_get_json(cfg, "/agent-rank", &[("agent", agent.to_string())])?;
    serde_json::from_value(raw).map_err(|e| format!("parse agent-rank response: {e}"))
}

pub fn get_agent_briefing(cfg: &P2PClawConfig, agent_id: Option<&str>) -> Result<Value, String> {
    let agent = agent_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(cfg.agent_id.as_str());
    request_get_json(cfg, "/agent-briefing", &[("agent_id", agent.to_string())])
}

pub fn publish_paper(
    cfg: &P2PClawConfig,
    title: &str,
    content: &str,
) -> Result<PaperResult, String> {
    let payload = json!({
        "title": title,
        "content": content,
        "author": cfg.agent_name,
        "agentId": cfg.agent_id,
    });
    let raw = request_post_json(cfg, "/publish-paper", &payload)?;
    serde_json::from_value(raw).map_err(|e| format!("parse publish-paper response: {e}"))
}

pub fn validate_paper(
    cfg: &P2PClawConfig,
    paper_id: &str,
    approve: bool,
    occam_score: Option<f64>,
) -> Result<ValidationResult, String> {
    let mut payload = json!({
        "paperId": paper_id,
        "agentId": cfg.agent_id,
        "result": approve,
    });
    if let Some(score) = occam_score {
        payload["occam_score"] = Value::from(score);
    }
    let raw = request_post_json(cfg, "/validate-paper", &payload)?;
    serde_json::from_value(raw).map_err(|e| format!("parse validate-paper response: {e}"))
}

pub fn poll_events(
    cfg: &P2PClawConfig,
    since: Option<u64>,
    limit: Option<u64>,
) -> Result<Vec<HiveEvent>, String> {
    let mut query = Vec::new();
    if let Some(v) = since {
        query.push(("since", v.to_string()));
    }
    if let Some(v) = limit {
        query.push(("limit", v.to_string()));
    }
    let raw = request_get_json(cfg, "/hive-events", &query)?;
    parse_list_from_value(raw, &["events"])
}

pub fn send_chat(cfg: &P2PClawConfig, message: &str, channel: Option<&str>) -> Result<(), String> {
    let payload = json!({
        "message": message,
        "sender": cfg.agent_name,
        "agentId": cfg.agent_id,
        "channel": channel.unwrap_or("research"),
    });
    let _ = request_post_json_fallback(cfg, &["/chat", "/hive-chat"], &payload)?;
    Ok(())
}

pub fn list_investigations(cfg: &P2PClawConfig) -> Result<Vec<Investigation>, String> {
    let raw = request_get_json(cfg, "/investigations", &[])?;
    parse_list_from_value(raw, &["investigations"])
}

pub fn create_investigation(
    cfg: &P2PClawConfig,
    title: &str,
    description: &str,
) -> Result<InvestigationCreateResult, String> {
    let payload = json!({
        "title": title,
        "description": description,
        "ownerId": cfg.agent_id,
    });
    let raw = request_post_json(cfg, "/investigations", &payload)?;
    serde_json::from_value(raw).map_err(|e| format!("parse create investigation response: {e}"))
}

pub fn search_wheel(cfg: &P2PClawConfig, query: &str) -> Result<WheelResult, String> {
    let primary = request_get_json(cfg, "/wheel", &[("q", query.to_string())]);
    let raw = match primary {
        Ok(v) => v,
        Err(primary_err) => request_get_json(cfg, "/wheel", &[("query", query.to_string())])
            .map_err(|fallback_err| {
                format!("search wheel failed with q and query parameters: {primary_err}; {fallback_err}")
            })?,
    };
    serde_json::from_value(raw).map_err(|e| format!("parse wheel response: {e}"))
}

pub fn get_briefing(cfg: &P2PClawConfig) -> Result<String, String> {
    request_get_text(cfg, "/briefing", &[], Some("text/markdown"))
}

pub fn report_tau_tick(cfg: &P2PClawConfig, compute_cycles: u64) -> Result<Value, String> {
    let payload = json!({
        "agent_id": cfg.agent_id,
        "compute_cycles": compute_cycles,
    });
    request_post_json_fallback(cfg, &["/tau/tick", "/tau-sync/tick"], &payload)
}

fn request_get_json(
    cfg: &P2PClawConfig,
    path: &str,
    query: &[(&str, String)],
) -> Result<Value, String> {
    let url = build_url(cfg, path, query)?;
    let mut req = http_client::get_with_timeout(&url, REQUEST_TIMEOUT)?;
    let headers = build_auth_headers(cfg, "")?;
    for (name, value) in headers {
        req = req.header(&name, &value);
    }
    let mut resp = match req.call() {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(format!("P2PCLAW GET {url} returned HTTP {code}"));
        }
        Err(e) => return Err(format!("P2PCLAW GET {url} failed: {e}")),
    };
    ensure_success_response(&mut resp, "GET", &url)?;
    resp.body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BODY_BYTES)
        .read_json()
        .map_err(|e| format!("parse P2PCLAW response {url}: {e}"))
}

fn request_get_text(
    cfg: &P2PClawConfig,
    path: &str,
    query: &[(&str, String)],
    accept: Option<&str>,
) -> Result<String, String> {
    let url = build_url(cfg, path, query)?;
    let mut req = http_client::get_with_timeout(&url, REQUEST_TIMEOUT)?;
    let headers = build_auth_headers(cfg, "")?;
    for (name, value) in headers {
        req = req.header(&name, &value);
    }
    if let Some(value) = accept {
        req = req.header("Accept", value);
    }
    let mut resp = match req.call() {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(format!("P2PCLAW GET {url} returned HTTP {code}"));
        }
        Err(e) => return Err(format!("P2PCLAW GET {url} failed: {e}")),
    };
    ensure_success_response(&mut resp, "GET", &url)?;
    resp.body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BODY_BYTES)
        .read_to_string()
        .map_err(|e| format!("read P2PCLAW text response {url}: {e}"))
}

fn request_post_json(cfg: &P2PClawConfig, path: &str, payload: &Value) -> Result<Value, String> {
    let url = build_url(cfg, path, &[])?;
    let body = serde_json::to_string(payload).map_err(|e| format!("serialize POST body: {e}"))?;
    let mut req =
        http_client::post_with_timeout(&url, REQUEST_TIMEOUT)?.content_type("application/json");
    let headers = build_auth_headers(cfg, &body)?;
    for (name, value) in headers {
        req = req.header(&name, &value);
    }
    let mut resp = match req.send(body) {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(format!("P2PCLAW POST {url} returned HTTP {code}"));
        }
        Err(e) => return Err(format!("P2PCLAW POST {url} failed: {e}")),
    };
    ensure_success_response(&mut resp, "POST", &url)?;
    resp.body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BODY_BYTES)
        .read_json()
        .map_err(|e| format!("parse P2PCLAW response {url}: {e}"))
}

fn request_post_json_fallback(
    cfg: &P2PClawConfig,
    paths: &[&str],
    payload: &Value,
) -> Result<Value, String> {
    let mut errors = Vec::new();
    for path in paths {
        match request_post_json(cfg, path, payload) {
            Ok(value) => return Ok(value),
            Err(err) => errors.push(format!("{path}: {err}")),
        }
    }
    Err(format!(
        "P2PCLAW POST failed for all candidate paths: {}",
        errors.join(" | ")
    ))
}

fn build_url(cfg: &P2PClawConfig, path: &str, query: &[(&str, String)]) -> Result<String, String> {
    validate_endpoint(&cfg.endpoint_url)?;
    let base = cfg.endpoint_url.trim_end_matches('/');
    let tail = path.trim_start_matches('/');
    let mut url =
        Url::parse(&format!("{base}/{tail}")).map_err(|e| format!("invalid URL build: {e}"))?;
    {
        let mut qp = url.query_pairs_mut();
        for (name, value) in query {
            qp.append_pair(name, value);
        }
    }
    Ok(url.to_string())
}

fn ensure_success_response(
    resp: &mut ureq::http::Response<ureq::Body>,
    method: &str,
    url: &str,
) -> Result<(), String> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let body = resp
        .body_mut()
        .with_config()
        .limit(MAX_ERROR_BODY_BYTES)
        .read_to_string()
        .unwrap_or_else(|_| "<unreadable error body>".to_string());
    Err(format!(
        "P2PCLAW {method} {url} returned HTTP {}: {}",
        status.as_u16(),
        body.trim()
    ))
}

fn build_auth_headers(cfg: &P2PClawConfig, body: &str) -> Result<Vec<(String, String)>, String> {
    if !cfg.auth_configured {
        return Ok(Vec::new());
    }
    let vault = open_vault();
    let mut secret = get_auth_secret(vault.as_ref())?.ok_or_else(|| {
        "P2PCLAW auth is configured but no secret is available in vault or auth_secret_INSECURE"
            .to_string()
    })?;
    let ts = now_unix_ms().to_string();
    let body_hash = sha256_hex(body.as_bytes());
    let message = format!("{}:{ts}:{body_hash}", cfg.agent_id);

    let signature = compute_auth_signature_message(&message, &secret);
    secret.zeroize();

    let signature = signature?;
    Ok(vec![
        ("x-agent-id".to_string(), cfg.agent_id.clone()),
        ("x-agent-ts".to_string(), ts),
        ("x-agent-signature".to_string(), signature),
    ])
}

fn parse_list_from_value<T: DeserializeOwned>(
    raw: Value,
    list_keys: &[&str],
) -> Result<Vec<T>, String> {
    if raw.is_array() {
        return serde_json::from_value(raw).map_err(|e| format!("parse list response: {e}"));
    }
    for key in list_keys {
        if let Some(value) = raw.get(*key) {
            return serde_json::from_value(value.clone())
                .map_err(|e| format!("parse `{key}` list response: {e}"));
        }
    }
    Err(format!(
        "list response missing expected keys: {}",
        list_keys.join(", ")
    ))
}

fn extract_u64(value: &Value, candidate_paths: &[&[&str]]) -> Option<u64> {
    for path in candidate_paths {
        let mut cursor = value;
        let mut ok = true;
        for key in *path {
            match cursor.get(*key) {
                Some(next) => cursor = next,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        if let Some(v) = cursor.as_u64() {
            return Some(v);
        }
        if let Some(s) = cursor.as_str() {
            if let Ok(v) = s.parse::<u64>() {
                return Some(v);
            }
        }
    }
    None
}

fn sha256_hex(input: &[u8]) -> String {
    hex::encode(Sha256::digest(input))
}

pub fn compute_auth_signature(
    agent_id: &str,
    ts_millis: u64,
    body: &str,
    secret: &str,
) -> Result<String, String> {
    let body_hash = sha256_hex(body.as_bytes());
    let message = format!("{agent_id}:{ts_millis}:{body_hash}");
    compute_auth_signature_message(&message, secret)
}

fn compute_auth_signature_message(message: &str, secret: &str) -> Result<String, String> {
    // TODO(pq): upgrade to HMAC-SHA512 when P2PCLAW server supports it.
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| format!("hmac init: {e}"))?;
    mac.update(message.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn load_disk_config_optional() -> Result<Option<P2PClawDiskConfig>, String> {
    let path = config::p2pclaw_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read p2pclaw config {}: {e}", path.display()))?;
    let cfg: P2PClawDiskConfig = serde_json::from_str(&raw)
        .map_err(|e| format!("parse p2pclaw config {}: {e}", path.display()))?;
    Ok(Some(cfg))
}

fn open_vault() -> Option<Vault> {
    let pq_wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();
    if !pq_wallet_path.exists() {
        return None;
    }
    Vault::open(&pq_wallet_path, &vault_path).ok()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
