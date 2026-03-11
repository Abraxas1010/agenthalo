use std::path::{Path, PathBuf};

pub fn wrap_agent(agent: &str, shell_rc: &Path) -> Result<(), String> {
    let marker = format!("# AGENTHALO_WRAP_{}", agent.to_ascii_uppercase());
    let alias = format!("alias {agent}='agenthalo run {agent}'");

    let mut content = if shell_rc.exists() {
        std::fs::read_to_string(shell_rc)
            .map_err(|e| format!("read shell rc {}: {e}", shell_rc.display()))?
    } else {
        String::new()
    };

    if content.contains(&marker) {
        return Ok(());
    }

    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push_str(&marker);
    content.push('\n');
    content.push_str(&alias);
    content.push('\n');

    std::fs::write(shell_rc, content)
        .map_err(|e| format!("write shell rc {}: {e}", shell_rc.display()))
}

pub fn unwrap_agent(agent: &str, shell_rc: &Path) -> Result<(), String> {
    if !shell_rc.exists() {
        return Ok(());
    }
    let marker = format!("# AGENTHALO_WRAP_{}", agent.to_ascii_uppercase());
    let raw = std::fs::read_to_string(shell_rc)
        .map_err(|e| format!("read shell rc {}: {e}", shell_rc.display()))?;

    let mut out = Vec::new();
    let mut skip_next_alias = false;
    for line in raw.lines() {
        if line.trim() == marker {
            skip_next_alias = true;
            continue;
        }
        if skip_next_alias {
            skip_next_alias = false;
            continue;
        }
        out.push(line.to_string());
    }

    let mut out_text = out.join("\n");
    if !out_text.is_empty() {
        out_text.push('\n');
    }
    std::fs::write(shell_rc, out_text)
        .map_err(|e| format!("write shell rc {}: {e}", shell_rc.display()))
}

pub fn detect_shell_rc() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "".to_string());
    if shell.contains("zsh") {
        home.join(".zshrc")
    } else {
        home.join(".bashrc")
    }
}
