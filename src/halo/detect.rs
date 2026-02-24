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

pub fn injection_flags(agent: &AgentType) -> Vec<String> {
    match agent {
        AgentType::Claude => vec![
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
        ],
        AgentType::Codex => vec!["--json".to_string()],
        AgentType::Gemini => vec!["--output-format".to_string(), "stream-json".to_string()],
        AgentType::Generic(_) => vec![],
    }
}
