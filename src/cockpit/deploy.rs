use crate::cli::default_witness_cfg;
use crate::cockpit::pty_manager::PtyManager;
use crate::halo::admission::{evaluate_launch_admission, AdmissionMode, AdmissionReport};
use crate::halo::governor_registry::GovernorRegistry;
use crate::halo::topo_signature::{self, CompareResult, TopoSignature};
use crate::halo::vault::Vault;
use crate::persistence::{default_wal_path, load_snapshot, persist_snapshot_and_sync_wal};
use crate::state::{Delta, State};
use crate::typed_value::TypedValue;
use crate::witness::WitnessSignatureAlgorithm;
use crate::VcBackend;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize)]
pub struct AgentDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub cli_command: &'static str,
    pub detect_command: &'static str,
    pub required_keys: &'static [&'static str],
    pub modes: &'static [&'static str],
    pub injection_flags: &'static [&'static str],
    pub gui_port: Option<u16>,
    pub gui_command: Option<&'static [&'static str]>,
    pub install_hint: Option<&'static str>,
}

pub fn agent_catalog() -> Vec<AgentDef> {
    vec![
        AgentDef {
            id: "claude",
            name: "Claude",
            description: "Anthropic's CLI agent",
            icon: "⚡",
            cli_command: "claude",
            detect_command: "claude",
            required_keys: &["anthropic"],
            modes: &["terminal", "cockpit"],
            injection_flags: &[],
            gui_port: None,
            gui_command: None,
            install_hint: Some("Install Claude Code CLI and ensure `claude` is on PATH."),
        },
        AgentDef {
            id: "codex",
            name: "Codex",
            description: "OpenAI Codex CLI",
            icon: "⌁",
            cli_command: "codex",
            detect_command: "codex",
            required_keys: &["openai"],
            modes: &["terminal", "cockpit"],
            injection_flags: &[],
            gui_port: None,
            gui_command: None,
            install_hint: Some("Install Codex CLI and ensure `codex` is on PATH."),
        },
        AgentDef {
            id: "gemini",
            name: "Gemini",
            description: "Google Gemini CLI",
            icon: "◇",
            cli_command: "gemini",
            detect_command: "gemini",
            required_keys: &["google"],
            modes: &["terminal", "cockpit"],
            injection_flags: &[],
            gui_port: None,
            gui_command: None,
            install_hint: Some("Install Gemini CLI and ensure `gemini` is on PATH."),
        },
        AgentDef {
            id: "shell",
            name: "Shell",
            description: "Plain interactive shell",
            icon: "▣",
            cli_command: "/bin/bash",
            detect_command: "bash",
            required_keys: &[],
            modes: &["terminal", "cockpit"],
            injection_flags: &[],
            gui_port: None,
            gui_command: None,
            install_hint: None,
        },
    ]
}

#[derive(Clone, Debug, Serialize)]
pub struct PreflightResult {
    pub cli_installed: bool,
    pub cli_path: Option<String>,
    pub keys_configured: bool,
    pub missing_keys: Vec<String>,
    pub docker_available: bool,
    pub ready: bool,
    pub install_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_topology: Option<DeployTopologyStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admission: Option<AdmissionReport>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LaunchRequest {
    pub agent_id: String,
    pub mode: String,
    #[serde(default)]
    pub container: bool,
    pub working_dir: Option<String>,
    #[serde(default)]
    pub admission_mode: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaunchResult {
    pub session_id: String,
    pub panels: Vec<PanelInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admission: Option<AdmissionReport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PanelInfo {
    pub id: String,
    pub panel_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iframe_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeployTopologyStatus {
    pub cli_path: String,
    pub binary_sha256: String,
    pub signature: TopoSignature,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<StoredTopoRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<CompareResult>,
    pub hash_changed: bool,
    pub structural_change_flagged: bool,
    pub stored_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredTopoRecord {
    pub agent_id: String,
    pub cli_path: String,
    pub binary_sha256: String,
    pub signature: TopoSignature,
    pub observed_at_unix: u64,
}

/// Check whether a CLI agent has native OAuth credentials on disk.
///
/// Returns `true` if the agent's auth token file exists and is non-trivial
/// (> 2 bytes). This covers agents that authenticate via their own OAuth flow
/// rather than requiring an API key in the HALO vault.
pub fn cli_authenticated(agent_id: &str) -> bool {
    let home = match cli_auth_home() {
        Some(h) => h,
        None => return false,
    };
    let auth_path = match agent_id {
        "claude" => home.join(".claude/.credentials.json"),
        "codex" => home.join(".codex/auth.json"),
        "gemini" => home.join(".gemini/oauth_creds.json"),
        _ => return false,
    };
    auth_path.exists()
        && std::fs::metadata(&auth_path)
            .map(|m| m.len() > 2)
            .unwrap_or(false)
}

pub fn cli_auth_home() -> Option<PathBuf> {
    std::env::var("AGENTHALO_CLI_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
}

pub fn cli_session_env() -> Vec<(String, String)> {
    cli_auth_home()
        .map(|home| vec![("HOME".to_string(), home.to_string_lossy().to_string())])
        .unwrap_or_default()
}

pub fn preflight(
    agent_id: &str,
    vault: Option<&Vault>,
    db_path: Option<&Path>,
    governor_registry: Option<&GovernorRegistry>,
    admission_mode: AdmissionMode,
) -> Result<PreflightResult, String> {
    let catalog = agent_catalog();
    let agent = catalog
        .iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| format!("unknown agent `{agent_id}`"))?;

    let cli_path = which_command(agent.detect_command);
    let cli_installed = cli_path.is_some();

    let mut missing_keys = Vec::new();
    for provider in agent.required_keys {
        let has_key = vault
            .and_then(|v| v.get_key(provider).ok())
            .map(|k| !k.is_empty())
            .unwrap_or(false);
        if !has_key {
            missing_keys.push((*provider).to_string());
        }
    }
    let vault_keys_ok = missing_keys.is_empty();
    let native_auth = cli_authenticated(agent_id);
    // Agent is considered key-ready if vault keys are present OR it has native
    // OAuth credentials (Claude/Codex/Gemini authenticate via their own CLIs).
    let keys_configured = vault_keys_ok || native_auth;
    let docker_available = which_command("docker").is_some();
    let binary_topology = cli_path
        .as_deref()
        .map(|path| inspect_binary_topology(agent_id, Path::new(path), db_path))
        .transpose()?;
    let admission = Some(evaluate_launch_admission(
        admission_mode,
        governor_registry,
        binary_topology.as_ref(),
    ));
    let admission_allowed = admission
        .as_ref()
        .map(|report| report.allowed)
        .unwrap_or(true);

    Ok(PreflightResult {
        cli_installed,
        cli_path,
        keys_configured,
        missing_keys: if native_auth {
            Vec::new()
        } else {
            missing_keys
        },
        docker_available,
        ready: cli_installed && keys_configured && admission_allowed,
        install_hint: agent.install_hint.map(|s| s.to_string()),
        binary_topology,
        admission,
    })
}

pub fn launch(
    req: &LaunchRequest,
    pty_manager: &PtyManager,
    vault: Option<&Vault>,
    db_path: Option<&Path>,
    governor_registry: Option<&GovernorRegistry>,
) -> Result<LaunchResult, String> {
    let catalog = agent_catalog();
    let agent = catalog
        .iter()
        .find(|a| a.id == req.agent_id)
        .ok_or_else(|| format!("unknown agent `{}`", req.agent_id))?;

    let admission_mode = AdmissionMode::parse(req.admission_mode.as_deref())?;
    let pre = preflight(agent.id, vault, db_path, governor_registry, admission_mode)?;
    if !pre.cli_installed {
        return Err(format!(
            "CLI `{}` is not installed. {}",
            agent.cli_command,
            pre.install_hint
                .unwrap_or_else(|| "Install the CLI and retry.".to_string())
        ));
    }
    if !pre.keys_configured {
        return Err(format!("missing API keys: {}", pre.missing_keys.join(", ")));
    }
    if let Some(admission) = pre.admission.as_ref() {
        if !admission.allowed {
            let reasons = admission
                .issues
                .iter()
                .map(|issue| issue.message.clone())
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(format!(
                "AETHER admission policy blocked launch (mode={}): {}",
                admission.mode, reasons
            ));
        }
    }

    let mut env_vars: Vec<(String, String)> = Vec::new();
    if !agent.required_keys.is_empty() {
        // Inject vault keys if available; agents with native OAuth (Claude/
        // Codex/Gemini) can launch without vault keys, so this is best-effort.
        if let Some(v) = vault {
            if let Ok(vars) = v.env_vars_for_providers(agent.required_keys) {
                env_vars.extend(vars);
            }
        }
    }
    if agent.id != "shell" {
        env_vars.extend(cli_session_env());
    }

    let mut command = if agent.id == "shell" {
        "/bin/bash".to_string()
    } else {
        agent.cli_command.to_string()
    };

    let mut args: Vec<String> = agent
        .injection_flags
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut pty_working_dir = route_working_dir(agent.id, req.working_dir.as_deref(), &mut args);

    if req.container {
        if which_command("docker").is_none() {
            return Err("container mode requested but docker is not available on PATH".to_string());
        }
        let image = std::env::var("AGENTHALO_COCKPIT_CONTAINER_IMAGE")
            .unwrap_or_else(|_| "ubuntu:24.04".to_string());
        args = build_docker_args(
            &image,
            &command,
            &args,
            &env_vars,
            req.working_dir.as_deref(),
        );
        command = "docker".to_string();
        // Env vars are passed as docker -e flags in container mode.
        env_vars.clear();
        pty_working_dir = None;
    }

    let id = pty_manager.create_session(
        &command,
        &args,
        env_vars,
        pty_working_dir,
        120,
        36,
        Some(agent.id.to_string()),
    )?;

    let mut panels = vec![PanelInfo {
        id: id.clone(),
        panel_type: "terminal".to_string(),
        ws_url: Some(format!("/api/cockpit/sessions/{id}/ws")),
        iframe_url: None,
    }];

    if matches!(req.mode.as_str(), "gui" | "gui+terminal" | "cockpit") {
        if let Some(port) = agent.gui_port {
            panels.push(PanelInfo {
                id: format!("{id}-gui"),
                panel_type: "iframe".to_string(),
                ws_url: None,
                iframe_url: Some(format!("http://127.0.0.1:{port}")),
            });
        }
    }

    Ok(LaunchResult {
        session_id: id,
        panels,
        admission: pre.admission,
    })
}

fn inspect_binary_topology(
    agent_id: &str,
    cli_path: &Path,
    db_path: Option<&Path>,
) -> Result<DeployTopologyStatus, String> {
    let bytes =
        std::fs::read(cli_path).map_err(|e| format!("read binary {}: {e}", cli_path.display()))?;
    let binary_sha256 = hex::encode(Sha256::digest(&bytes));
    let signature = topo_signature::fingerprint(&bytes, 3);
    let stored_key = format!("topo:{binary_sha256}");

    let mut previous = None;
    let mut comparison = None;
    let mut hash_changed = false;
    let mut structural_change_flagged = false;

    if let Some(path) = db_path {
        let record = StoredTopoRecord {
            agent_id: agent_id.to_string(),
            cli_path: cli_path.display().to_string(),
            binary_sha256: binary_sha256.clone(),
            signature: signature.clone(),
            observed_at_unix: now_unix(),
        };
        let latest_key = format!("topo:agent:{agent_id}:latest");
        let mut db = load_or_create_halo_db(path)?;
        if let Some(TypedValue::Json(previous_json)) = db.get_typed(&latest_key) {
            if let Ok(previous_record) = serde_json::from_value::<StoredTopoRecord>(previous_json) {
                hash_changed = previous_record.binary_sha256 != binary_sha256;
                if hash_changed {
                    let result = topo_signature::compare(&previous_record.signature, &signature);
                    structural_change_flagged = !result.within_formal_bound;
                    comparison = Some(result);
                }
                previous = Some(previous_record);
            }
        }
        persist_topology_records(&mut db, path, &stored_key, &latest_key, &record)?;
    }

    let warning = if structural_change_flagged {
        Some(
            "binary structure changed beyond the formal Betti overlap bound; keep SHA-256 as the primary authenticator and review the artifact diff."
                .to_string(),
        )
    } else {
        comparison.as_ref().and_then(|cmp| cmp.warning.clone())
    };

    Ok(DeployTopologyStatus {
        cli_path: cli_path.display().to_string(),
        binary_sha256,
        signature,
        previous,
        comparison,
        hash_changed,
        structural_change_flagged,
        stored_key,
        warning,
    })
}

fn persist_topology_records(
    db: &mut crate::protocol::NucleusDb,
    db_path: &Path,
    stored_key: &str,
    latest_key: &str,
    record: &StoredTopoRecord,
) -> Result<(), String> {
    let desired_value =
        serde_json::to_value(record).map_err(|e| format!("serialize topology record: {e}"))?;

    let needs_hash_record = match db.get_typed(stored_key) {
        Some(TypedValue::Json(existing)) => existing != desired_value,
        Some(_) => true,
        None => true,
    };
    let needs_latest_record = match db.get_typed(latest_key) {
        Some(TypedValue::Json(existing)) => existing != desired_value,
        Some(_) => true,
        None => true,
    };

    if !needs_hash_record && !needs_latest_record {
        return Ok(());
    }

    let mut writes = Vec::new();
    if needs_hash_record {
        let (idx, cell) = db
            .put_typed(stored_key, TypedValue::Json(desired_value.clone()))
            .map_err(|e| format!("store topology record: {e}"))?;
        writes.push((idx, cell));
    }
    if needs_latest_record {
        let (idx, cell) = db
            .put_typed(latest_key, TypedValue::Json(desired_value))
            .map_err(|e| format!("store latest topology record: {e}"))?;
        writes.push((idx, cell));
    }

    if writes.is_empty() {
        return Ok(());
    }

    db.commit(Delta::new(writes), &[])
        .map_err(|e| format!("commit topology record: {e:?}"))?;
    let wal_path = default_wal_path(db_path);
    persist_snapshot_and_sync_wal(db_path, &wal_path, db)
        .map_err(|e| format!("persist topology record: {e:?}"))?;
    Ok(())
}

fn load_or_create_halo_db(db_path: &Path) -> Result<crate::protocol::NucleusDb, String> {
    let mut cfg = default_witness_cfg();
    cfg.signing_algorithm = WitnessSignatureAlgorithm::MlDsa65;
    if !db_path.exists() {
        return Ok(crate::protocol::NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            cfg,
        ));
    }
    load_snapshot(db_path, cfg).map_err(|e| format!("load NucleusDB {}: {e:?}", db_path.display()))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn build_docker_args(
    image: &str,
    inner_command: &str,
    inner_args: &[String],
    env_vars: &[(String, String)],
    requested_dir: Option<&str>,
) -> Vec<String> {
    let mut out = vec![
        "run".to_string(),
        "--rm".to_string(),
        "-it".to_string(),
        "--name".to_string(),
        format!(
            "agenthalo-{}",
            &uuid::Uuid::new_v4().as_simple().to_string()[..8]
        ),
    ];

    if let Some(dir) = requested_dir.map(str::trim).filter(|d| !d.is_empty()) {
        out.push("-v".to_string());
        out.push(format!("{dir}:{dir}"));
        out.push("-w".to_string());
        out.push(dir.to_string());
    }

    for (k, v) in env_vars {
        out.push("-e".to_string());
        out.push(format!("{k}={v}"));
    }

    out.push(image.to_string());
    out.push(inner_command.to_string());
    out.extend(inner_args.iter().cloned());
    out
}

fn route_working_dir<'a>(
    agent_id: &str,
    requested_dir: Option<&'a str>,
    args: &mut Vec<String>,
) -> Option<&'a str> {
    let dir = requested_dir.map(str::trim).filter(|d| !d.is_empty())?;
    // Not all CLIs support --cwd. Route working-dir by agent capability:
    // - shell: use PTY process cwd directly
    // - claude: supports --cwd flag
    // - others: ignore here (can be added with verified per-CLI flags later)
    match agent_id {
        "shell" => Some(dir),
        "claude" => {
            args.extend(["--cwd".to_string(), dir.to_string()]);
            None
        }
        _ => None,
    }
}

fn which_command(command: &str) -> Option<String> {
    let out = Command::new("which").arg(command).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_docker_args, cli_authenticated, cli_session_env, inspect_binary_topology, preflight,
        route_working_dir, StoredTopoRecord,
    };
    use crate::halo::admission::AdmissionMode;
    use crate::halo::governor::GovernorConfig;
    use crate::halo::governor_registry::GovernorRegistry;
    use crate::halo::topo_signature::fingerprint;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn shell_uses_pty_cwd_not_cli_flag() {
        let mut args = vec![];
        let cwd = route_working_dir("shell", Some("/tmp"), &mut args);
        assert_eq!(cwd, Some("/tmp"));
        assert!(!args.iter().any(|a| a == "--cwd"));
    }

    #[test]
    fn claude_uses_cwd_flag() {
        let mut args = vec![];
        let cwd = route_working_dir("claude", Some("/tmp/work"), &mut args);
        assert_eq!(cwd, None);
        assert_eq!(args, vec!["--cwd".to_string(), "/tmp/work".to_string()]);
    }

    #[test]
    fn other_agents_ignore_cwd_flag() {
        let mut args = vec![];
        let cwd = route_working_dir("codex", Some("/tmp/work"), &mut args);
        assert_eq!(cwd, None);
        assert!(!args.iter().any(|a| a == "--cwd"));
    }

    #[test]
    fn docker_args_include_env_and_workdir() {
        let args = build_docker_args(
            "ubuntu:24.04",
            "codex",
            &["--json".to_string()],
            &[("OPENAI_API_KEY".to_string(), "sk-test".to_string())],
            Some("/tmp/work"),
        );
        assert!(args
            .windows(2)
            .any(|w| w == ["-e", "OPENAI_API_KEY=sk-test"]));
        assert!(args.windows(2).any(|w| w == ["-w", "/tmp/work"]));
        assert!(args.iter().any(|a| a == "ubuntu:24.04"));
        assert!(args.iter().any(|a| a == "codex"));
    }

    #[test]
    fn topology_fingerprint_is_deterministic_without_db() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent.bin");
        std::fs::write(&path, b"abcdefabcdef").expect("write binary");
        let first = inspect_binary_topology("shell", &path, None).expect("first");
        let second = inspect_binary_topology("shell", &path, None).expect("second");
        assert_eq!(first.binary_sha256, second.binary_sha256);
        assert_eq!(
            first.signature.betti1_heuristic,
            second.signature.betti1_heuristic
        );
    }

    #[test]
    fn stored_topology_record_roundtrips() {
        let record = StoredTopoRecord {
            agent_id: "codex".to_string(),
            cli_path: "/tmp/codex".to_string(),
            binary_sha256: "abcd".to_string(),
            signature: fingerprint(b"abcdabcd", 3),
            observed_at_unix: 42,
        };
        let value = serde_json::to_value(&record).expect("serialize");
        let roundtrip: StoredTopoRecord = serde_json::from_value(value).expect("deserialize");
        assert_eq!(roundtrip.agent_id, "codex");
        assert_eq!(roundtrip.signature.data_len, 8);
    }

    #[test]
    fn preflight_reports_blocked_admission_when_governor_is_outside_regime() {
        let registry = GovernorRegistry::new();
        registry
            .register(GovernorConfig {
                instance_id: "gov-compute".to_string(),
                alpha: 0.8,
                beta: 0.3,
                dt: 1.0,
                eps_min: 1.0,
                eps_max: 10.0,
                target: 8.0,
                formal_basis: "test".to_string(),
                ki: 0.0,
                kb: 0.0,
                adaptive: None,
            })
            .expect("register gov-compute");
        registry
            .register(GovernorConfig {
                instance_id: "gov-pty".to_string(),
                alpha: 0.01,
                beta: 0.05,
                dt: 1.0,
                eps_min: 30.0,
                eps_max: 900.0,
                target: 120.0,
                formal_basis: "test".to_string(),
                ki: 0.0,
                kb: 0.0,
                adaptive: None,
            })
            .expect("register gov-pty");

        let result = preflight("shell", None, None, Some(&registry), AdmissionMode::Block)
            .expect("preflight");
        let admission = result.admission.expect("admission");
        assert!(!admission.allowed);
        assert!(admission
            .issues
            .iter()
            .any(|issue| issue.code == "governor_gain_violated"));
    }

    #[test]
    fn cli_authenticated_uses_agenthalo_cli_home_override() {
        let _guard = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).expect("claude dir");
        std::fs::write(claude_dir.join(".credentials.json"), br#"{"token":"ok"}"#)
            .expect("auth file");
        unsafe {
            std::env::set_var("AGENTHALO_CLI_HOME", dir.path());
            std::env::remove_var("HOME");
        }
        assert!(cli_authenticated("claude"));
        unsafe {
            std::env::remove_var("AGENTHALO_CLI_HOME");
        }
    }

    #[test]
    fn cli_session_env_uses_cli_home_override() {
        let _guard = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        unsafe {
            std::env::set_var("AGENTHALO_CLI_HOME", dir.path());
            std::env::remove_var("HOME");
        }
        let env = cli_session_env();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "HOME");
        assert_eq!(env[0].1, dir.path().to_string_lossy());
        unsafe {
            std::env::remove_var("AGENTHALO_CLI_HOME");
        }
    }
}
