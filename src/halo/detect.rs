#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentType {
    Claude,
    Codex,
    Gemini,
    Generic(String),
}

impl AgentType {
    pub fn as_str(&self) -> &str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Codex => "codex",
            AgentType::Gemini => "gemini",
            AgentType::Generic(v) => v.as_str(),
        }
    }
}

pub fn detect_agent(command: &str) -> AgentType {
    let base = std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command)
        .to_ascii_lowercase();

    match base.as_str() {
        "claude" => AgentType::Claude,
        "codex" => AgentType::Codex,
        "gemini" => AgentType::Gemini,
        other => AgentType::Generic(other.to_string()),
    }
}

/// Returns flags to inject for structured output, skipping any the user already passed.
pub fn injection_flags(agent: &AgentType, user_args: &[String]) -> Vec<String> {
    let candidate = match agent {
        AgentType::Claude => vec![
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
        ],
        AgentType::Codex => vec!["--json".to_string()],
        AgentType::Gemini => vec!["--output-format".to_string(), "stream-json".to_string()],
        AgentType::Generic(_) => vec![],
    };
    // Only inject flags the user hasn't already specified.
    let mut out = Vec::new();
    let mut skip_next_value = false;
    for flag in &candidate {
        if skip_next_value {
            // This is a value for a flag we already skipped; don't add it.
            skip_next_value = false;
            continue;
        }
        if flag.starts_with('-') && user_args.iter().any(|a| a == flag) {
            // User already has this flag — skip it and its value argument.
            skip_next_value = true;
            continue;
        }
        skip_next_value = false;
        out.push(flag.clone());
    }
    out
}
