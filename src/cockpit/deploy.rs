use crate::cockpit::pty_manager::PtyManager;
use crate::halo::vault::Vault;
use serde::{Deserialize, Serialize};
use std::process::Command;

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
            injection_flags: &["--output-format", "stream-json", "--verbose"],
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
            injection_flags: &["--json"],
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
            injection_flags: &["--output-format", "stream-json"],
            gui_port: None,
            gui_command: None,
            install_hint: Some("Install Gemini CLI and ensure `gemini` is on PATH."),
        },
        AgentDef {
            id: "openclaw",
            name: "OpenClaw",
            description: "OpenAI-compatible autonomous agent",
            icon: "🦾",
            cli_command: "openclaw",
            detect_command: "openclaw",
            required_keys: &["openai"],
            modes: &["terminal", "gui", "cockpit", "gui+terminal"],
            injection_flags: &[],
            gui_port: Some(3110),
            gui_command: Some(&["openclaw", "--gui", "--port", "3110"]),
            install_hint: Some("Install OpenClaw and ensure `openclaw` is on PATH."),
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
}

#[derive(Clone, Debug, Deserialize)]
pub struct LaunchRequest {
    pub agent_id: String,
    pub mode: String,
    #[serde(default)]
    pub container: bool,
    pub working_dir: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaunchResult {
    pub session_id: String,
    pub panels: Vec<PanelInfo>,
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

/// Check whether a CLI agent has native OAuth credentials on disk.
///
/// Returns `true` if the agent's auth token file exists and is non-trivial
/// (> 2 bytes). This covers agents that authenticate via their own OAuth flow
/// rather than requiring an API key in the HALO vault.
pub fn cli_authenticated(agent_id: &str) -> bool {
    let home = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h),
        Err(_) => return false,
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

pub fn preflight(agent_id: &str, vault: Option<&Vault>) -> Result<PreflightResult, String> {
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
        ready: cli_installed && keys_configured,
        install_hint: agent.install_hint.map(|s| s.to_string()),
    })
}

pub fn launch(
    req: &LaunchRequest,
    pty_manager: &PtyManager,
    vault: Option<&Vault>,
) -> Result<LaunchResult, String> {
    let catalog = agent_catalog();
    let agent = catalog
        .iter()
        .find(|a| a.id == req.agent_id)
        .ok_or_else(|| format!("unknown agent `{}`", req.agent_id))?;

    let pre = preflight(agent.id, vault)?;
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
    })
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
    use super::{build_docker_args, route_working_dir};

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
}
