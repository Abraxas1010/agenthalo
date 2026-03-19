//! Workspace profile configuration for worktree isolation.
//!
//! A workspace profile defines how agent sessions are isolated:
//! - Whether worktree isolation is enabled
//! - Which host paths are injected into the worktree (skills, MCP configs, etc.)
//! - Access modes for injected paths (readonly, copy, approved_write)
//! - Cleanup policies
//! - Edit approval configuration

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Access mode for an injected path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionMode {
    /// Symlinked, filesystem permissions deny writes.
    Readonly,
    /// Copied into the worktree. Agent can modify freely.
    Copy,
    /// Symlinked, but writes require human approval.
    ApprovedWrite,
}

/// A single injection entry: host source mapped to worktree target.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Injection {
    /// Absolute path on the host (tilde-expanded at load time).
    pub source: String,
    /// Relative path inside the worktree where this will appear.
    pub target: String,
    /// Access mode.
    pub mode: InjectionMode,
    /// Human description (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// What triggers the edit approval gate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// Only injected paths require approval.
    Injected,
    /// All file edits require approval.
    All,
}

/// Edit approval configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditApprovalConfig {
    pub enabled: bool,
    #[serde(default = "default_approval_scope")]
    pub require_for: ApprovalScope,
    #[serde(default = "default_approval_method")]
    pub approval_method: String,
    #[serde(default)]
    pub auto_approve_patterns: Vec<String>,
}

fn default_approval_scope() -> ApprovalScope {
    ApprovalScope::Injected
}

fn default_approval_method() -> String {
    "cli_prompt".to_string()
}

impl Default for EditApprovalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_for: ApprovalScope::Injected,
            approval_method: default_approval_method(),
            auto_approve_patterns: Vec::new(),
        }
    }
}

/// Cleanup policy for worktree sessions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CleanupConfig {
    /// Remove worktree automatically when session ends.
    #[serde(default = "default_true")]
    pub auto_remove_on_session_end: bool,
    /// Push commits before removing.
    #[serde(default = "default_true")]
    pub push_before_remove: bool,
    /// Archive dirty files before removal.
    #[serde(default = "default_true")]
    pub archive_dirty: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            auto_remove_on_session_end: true,
            push_before_remove: true,
            archive_dirty: true,
        }
    }
}

/// Policy for external file writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalWritePolicy {
    /// Agents cannot write to files outside the container (default).
    Deny,
    /// Agents get a git worktree for external writes.
    Worktree,
}

impl Default for ExternalWritePolicy {
    fn default() -> Self {
        Self::Deny
    }
}

/// A workspace profile defining worktree isolation behaviour.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceProfile {
    pub profile_name: String,
    #[serde(default)]
    pub worktree_isolation: bool,
    /// Controls whether agents can write to files outside the container.
    /// `deny` = read-only (default), `worktree` = create a git worktree.
    #[serde(default)]
    pub external_write_policy: ExternalWritePolicy,
    #[serde(default = "default_worktree_base")]
    pub worktree_base: String,
    #[serde(default = "default_worktree_prefix")]
    pub worktree_prefix: String,
    #[serde(default = "default_worktree_branch")]
    pub worktree_branch: String,
    #[serde(default = "default_max_worktrees")]
    pub max_worktrees: usize,
    #[serde(default = "default_max_lifetime_hours")]
    pub max_lifetime_hours: u64,
    #[serde(default)]
    pub injections: Vec<Injection>,
    #[serde(default)]
    pub edit_approval: EditApprovalConfig,
    #[serde(default)]
    pub cleanup: CleanupConfig,
}

fn default_worktree_base() -> String {
    "/tmp".to_string()
}
fn default_worktree_prefix() -> String {
    "agenthalo".to_string()
}
fn default_worktree_branch() -> String {
    "origin/master".to_string()
}
fn default_max_worktrees() -> usize {
    5
}
fn default_max_lifetime_hours() -> u64 {
    168
}

impl Default for WorkspaceProfile {
    fn default() -> Self {
        Self {
            profile_name: "default".to_string(),
            worktree_isolation: false,
            external_write_policy: ExternalWritePolicy::default(),
            worktree_base: default_worktree_base(),
            worktree_prefix: default_worktree_prefix(),
            worktree_branch: default_worktree_branch(),
            max_worktrees: default_max_worktrees(),
            max_lifetime_hours: default_max_lifetime_hours(),
            injections: Vec::new(),
            edit_approval: EditApprovalConfig::default(),
            cleanup: CleanupConfig::default(),
        }
    }
}

/// Expand `~` to the user's home directory (public wrapper for use by deploy).
pub fn expand_tilde_pub(path: &str) -> String {
    expand_tilde(path)
}

/// Expand `~` to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}

/// Validation error for a workspace profile.
#[derive(Clone, Debug)]
pub struct ProfileValidationError {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for ProfileValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl WorkspaceProfile {
    /// Validate the profile for correctness.
    pub fn validate(&self) -> Vec<ProfileValidationError> {
        let mut errors = Vec::new();
        if self.profile_name.trim().is_empty() {
            errors.push(ProfileValidationError {
                field: "profile_name".to_string(),
                message: "profile name must not be empty".to_string(),
            });
        }
        if self.max_worktrees == 0 {
            errors.push(ProfileValidationError {
                field: "max_worktrees".to_string(),
                message: "max_worktrees must be > 0".to_string(),
            });
        }
        for (i, inj) in self.injections.iter().enumerate() {
            let expanded = expand_tilde(&inj.source);
            if !Path::new(&expanded).exists() {
                errors.push(ProfileValidationError {
                    field: format!("injections[{i}].source"),
                    message: format!("source path does not exist: {expanded}"),
                });
            }
            if inj.target.contains("..") {
                errors.push(ProfileValidationError {
                    field: format!("injections[{i}].target"),
                    message: "target path must not contain '..'".to_string(),
                });
            }
            if inj.target.starts_with('/') {
                errors.push(ProfileValidationError {
                    field: format!("injections[{i}].target"),
                    message: "target path must be relative".to_string(),
                });
            }
        }
        // Check for duplicate targets.
        let mut targets: Vec<&str> = self.injections.iter().map(|i| i.target.as_str()).collect();
        targets.sort();
        for window in targets.windows(2) {
            if window[0] == window[1] {
                errors.push(ProfileValidationError {
                    field: "injections".to_string(),
                    message: format!("duplicate target path: {}", window[0]),
                });
            }
        }
        errors
    }

    /// Get external skill source paths (injections whose target contains "skills").
    pub fn external_skill_sources(&self) -> Vec<PathBuf> {
        self.injections
            .iter()
            .filter(|inj| inj.target.contains("skills"))
            .map(|inj| PathBuf::from(expand_tilde(&inj.source)))
            .filter(|p| p.exists())
            .collect()
    }

    /// Get external MCP tool registry paths (injections whose target contains "mcp" and ends in .json).
    pub fn external_mcp_sources(&self) -> Vec<PathBuf> {
        self.injections
            .iter()
            .filter(|inj| {
                let t = inj.target.to_lowercase();
                (t.contains("mcp") || t.contains(".mcp")) && t.ends_with(".json")
            })
            .map(|inj| PathBuf::from(expand_tilde(&inj.source)))
            .filter(|p| p.exists())
            .collect()
    }

    /// Expand tilde in all source paths.
    pub fn expanded_injections(&self) -> Vec<(PathBuf, PathBuf, InjectionMode)> {
        self.injections
            .iter()
            .map(|inj| {
                let source = PathBuf::from(expand_tilde(&inj.source));
                let target = PathBuf::from(&inj.target);
                (source, target, inj.mode.clone())
            })
            .collect()
    }
}

/// Directory where workspace profiles are stored.
pub fn workspace_profiles_dir() -> PathBuf {
    crate::halo::config::halo_dir().join("workspace_profiles")
}

/// Path to the file recording the active profile name.
pub fn active_profile_path() -> PathBuf {
    crate::halo::config::halo_dir().join("active_workspace_profile")
}

/// Load the active profile name (defaults to "default").
pub fn active_profile_name() -> String {
    std::fs::read_to_string(active_profile_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

/// Set the active profile name.
pub fn set_active_profile(name: &str) -> Result<(), String> {
    let path = active_profile_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create halo dir: {e}"))?;
    }
    std::fs::write(&path, name)
        .map_err(|e| format!("write active profile: {e}"))
}

/// Load a workspace profile by name.
pub fn load_profile(name: &str) -> Result<WorkspaceProfile, String> {
    let path = workspace_profiles_dir().join(format!("{name}.json"));
    if !path.exists() {
        if name == "default" {
            return Ok(WorkspaceProfile::default());
        }
        return Err(format!("workspace profile '{name}' not found at {}", path.display()));
    }
    let raw = std::fs::read(&path)
        .map_err(|e| format!("read profile {}: {e}", path.display()))?;
    serde_json::from_slice(&raw)
        .map_err(|e| format!("parse profile {}: {e}", path.display()))
}

/// Load the currently active workspace profile.
pub fn load_active_profile() -> Result<WorkspaceProfile, String> {
    load_profile(&active_profile_name())
}

/// Save a workspace profile.
pub fn save_profile(profile: &WorkspaceProfile) -> Result<(), String> {
    let dir = workspace_profiles_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create workspace_profiles dir: {e}"))?;
    let path = dir.join(format!("{}.json", profile.profile_name));
    let raw = serde_json::to_vec_pretty(profile)
        .map_err(|e| format!("serialize profile: {e}"))?;
    std::fs::write(&path, &raw)
        .map_err(|e| format!("write profile {}: {e}", path.display()))
}

/// List available profile names.
pub fn list_profiles() -> Result<Vec<String>, String> {
    let dir = workspace_profiles_dir();
    if !dir.exists() {
        return Ok(vec!["default".to_string()]);
    }
    let mut names = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("read workspace_profiles dir: {e}"))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(stem) = name.strip_suffix(".json") {
            names.push(stem.to_string());
        }
    }
    if names.is_empty() {
        names.push("default".to_string());
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_valid() {
        let profile = WorkspaceProfile::default();
        // Don't validate source paths (they won't exist in test), just check structure
        assert_eq!(profile.profile_name, "default");
        assert!(!profile.worktree_isolation);
        assert_eq!(profile.max_worktrees, 5);
    }

    #[test]
    fn roundtrip_serialization() {
        let mut profile = WorkspaceProfile::default();
        profile.worktree_isolation = true;
        profile.injections.push(Injection {
            source: "/tmp/test_source".to_string(),
            target: ".agents/skills".to_string(),
            mode: InjectionMode::Readonly,
            description: Some("test".to_string()),
        });
        let json = serde_json::to_string_pretty(&profile).expect("serialize");
        let parsed: WorkspaceProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.profile_name, "default");
        assert!(parsed.worktree_isolation);
        assert_eq!(parsed.injections.len(), 1);
        assert_eq!(parsed.injections[0].mode, InjectionMode::Readonly);
    }

    #[test]
    fn validate_rejects_path_traversal() {
        let mut profile = WorkspaceProfile::default();
        profile.injections.push(Injection {
            source: "/tmp".to_string(),
            target: "../etc/passwd".to_string(),
            mode: InjectionMode::Readonly,
            description: None,
        });
        let errors = profile.validate();
        assert!(errors.iter().any(|e| e.message.contains("..")));
    }

    #[test]
    fn validate_rejects_absolute_target() {
        let mut profile = WorkspaceProfile::default();
        profile.injections.push(Injection {
            source: "/tmp".to_string(),
            target: "/etc/hosts".to_string(),
            mode: InjectionMode::Readonly,
            description: None,
        });
        let errors = profile.validate();
        assert!(errors.iter().any(|e| e.message.contains("relative")));
    }

    #[test]
    fn validate_rejects_duplicate_targets() {
        let mut profile = WorkspaceProfile::default();
        let inj = Injection {
            source: "/tmp".to_string(),
            target: "same_target".to_string(),
            mode: InjectionMode::Readonly,
            description: None,
        };
        profile.injections.push(inj.clone());
        profile.injections.push(inj);
        let errors = profile.validate();
        assert!(errors.iter().any(|e| e.message.contains("duplicate")));
    }

    #[test]
    fn expand_tilde_works() {
        let result = expand_tilde("/absolute/path");
        assert_eq!(result, "/absolute/path");
        // Tilde expansion depends on $HOME
        let tilde = expand_tilde("~/test");
        assert!(!tilde.starts_with('~'));
    }

    #[test]
    fn save_and_load_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Override the profile dir via env
        let profile = WorkspaceProfile {
            profile_name: "test_profile".to_string(),
            ..Default::default()
        };
        let path = dir.path().join("test_profile.json");
        let raw = serde_json::to_vec_pretty(&profile).expect("serialize");
        std::fs::write(&path, &raw).expect("write");
        let loaded: WorkspaceProfile =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse");
        assert_eq!(loaded.profile_name, "test_profile");
    }
}
