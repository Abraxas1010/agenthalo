//! Worktree lifecycle manager for agent session isolation.
//!
//! Creates git worktrees per agent session, injects host paths via symlinks
//! or copies, verifies injection integrity, and manages cleanup.

use crate::halo::workspace_profile::{InjectionMode, WorkspaceProfile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Information about a created worktree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub session_id: String,
    pub branch: String,
    pub created_at: u64,
    pub repo_path: PathBuf,
    #[serde(default)]
    pub injections: Vec<InjectionRecord>,
}

/// Record of a single injection performed in a worktree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InjectionRecord {
    pub source: PathBuf,
    pub target: String,
    pub mode: String,
    pub source_hash: Option<String>,
}

/// Result of verifying injection integrity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegrityViolation {
    pub target: String,
    pub kind: String,
    pub detail: String,
}

/// Report from worktree cleanup.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CleanupReport {
    pub worktree_path: String,
    pub had_dirty_files: bool,
    pub pushed: bool,
    pub archived: bool,
    pub archive_path: Option<String>,
    pub removed: bool,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hash_path(path: &Path) -> Option<String> {
    if path.is_file() {
        let bytes = std::fs::read(path).ok()?;
        Some(hex::encode(Sha256::digest(&bytes)))
    } else if path.is_dir() {
        // Hash the directory listing as a lightweight integrity check.
        let mut entries: Vec<String> = Vec::new();
        if let Ok(readdir) = std::fs::read_dir(path) {
            for entry in readdir.flatten() {
                entries.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        entries.sort();
        Some(hex::encode(Sha256::digest(entries.join("\n").as_bytes())))
    } else {
        None
    }
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("run git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {} failed: {stderr}", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
        || Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["rev-parse", "--git-dir"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// Create a git worktree for an agent session.
pub fn create_worktree(
    repo_path: &Path,
    profile: &WorkspaceProfile,
    agent_id: &str,
    session_id: &str,
) -> Result<WorktreeInfo, String> {
    if !is_git_repo(repo_path) {
        return Err(format!(
            "not a git repository: {}",
            repo_path.display()
        ));
    }

    // Check existing HALO-managed worktrees.
    let existing = list_managed_worktrees(repo_path, &profile.worktree_prefix)?;
    if existing.len() >= profile.max_worktrees {
        return Err(format!(
            "max worktrees ({}) reached; {} active. Prune before creating more.",
            profile.max_worktrees,
            existing.len()
        ));
    }

    // Fetch latest.
    let _ = run_git(repo_path, &["fetch", "origin"]);

    let worktree_name = format!(
        "{}_{}_{}",
        profile.worktree_prefix, agent_id, session_id
    );
    let worktree_path = PathBuf::from(&profile.worktree_base).join(&worktree_name);

    // Create worktree.
    run_git(
        repo_path,
        &[
            "worktree",
            "add",
            &worktree_path.display().to_string(),
            &profile.worktree_branch,
        ],
    )?;

    // Inject paths.
    let injections = inject_paths(&worktree_path, profile)?;

    let info = WorktreeInfo {
        path: worktree_path.clone(),
        session_id: session_id.to_string(),
        branch: profile.worktree_branch.clone(),
        created_at: now_unix(),
        repo_path: repo_path.to_path_buf(),
        injections,
    };

    // Write manifest.
    let manifest_path = worktree_path.join(".agenthalo_manifest.json");
    let manifest_json = serde_json::to_vec_pretty(&info)
        .map_err(|e| format!("serialize manifest: {e}"))?;
    std::fs::write(&manifest_path, &manifest_json)
        .map_err(|e| format!("write manifest: {e}"))?;

    Ok(info)
}

/// Inject host paths into a worktree according to the profile.
fn inject_paths(
    worktree_path: &Path,
    profile: &WorkspaceProfile,
) -> Result<Vec<InjectionRecord>, String> {
    let mut records = Vec::new();
    for (source, target, mode) in profile.expanded_injections() {
        if !source.exists() {
            return Err(format!(
                "injection source does not exist: {}",
                source.display()
            ));
        }
        let full_target = worktree_path.join(&target);
        if let Some(parent) = full_target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir for injection target: {e}"))?;
        }

        // Remove existing file/symlink at target if present.
        if full_target.exists() || full_target.symlink_metadata().is_ok() {
            if full_target.is_dir() {
                std::fs::remove_dir_all(&full_target)
                    .map_err(|e| format!("remove existing dir at target: {e}"))?;
            } else {
                std::fs::remove_file(&full_target)
                    .map_err(|e| format!("remove existing file at target: {e}"))?;
            }
        }

        let mode_str = match &mode {
            InjectionMode::Readonly => "readonly",
            InjectionMode::Copy => "copy",
            InjectionMode::ApprovedWrite => "approved_write",
        };

        match &mode {
            InjectionMode::Readonly => {
                // Readonly: copy the source and make it read-only.
                // We cannot use symlinks for readonly because std::fs::write
                // follows symlinks and modifies the original host file.
                copy_recursive(&source, &full_target)?;
                set_readonly_recursive(&full_target);
            }
            InjectionMode::ApprovedWrite => {
                // Approved-write: use symlinks. The edit-gate hook intercepts
                // writes and requires human approval before they land.
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&source, &full_target)
                        .map_err(|e| {
                            format!(
                                "symlink {} -> {}: {e}",
                                source.display(),
                                full_target.display()
                            )
                        })?;
                }
                #[cfg(not(unix))]
                {
                    return Err("symlink injection only supported on Unix".to_string());
                }
            }
            InjectionMode::Copy => {
                copy_recursive(&source, &full_target)?;
            }
        }

        records.push(InjectionRecord {
            source: source.clone(),
            target: target.display().to_string(),
            mode: mode_str.to_string(),
            source_hash: hash_path(&source),
        });
    }
    Ok(records)
}

/// Restore write permissions on all files/dirs in a tree (for cleanup).
#[cfg(unix)]
fn restore_write_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return; // Don't follow symlinks.
        }
        let mode = meta.permissions().mode();
        if meta.is_dir() {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o700));
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    restore_write_permissions(&entry.path());
                }
            }
        } else {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o600));
        }
    }
}

#[cfg(not(unix))]
fn restore_write_permissions(path: &Path) {
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(path, perms);
    }
}

/// Set a file or directory tree to read-only (owner: r-x for dirs, r-- for files).
#[cfg(unix)]
fn set_readonly_recursive(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if path.is_dir() {
        // Directory: r-x (0o500)
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500));
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                set_readonly_recursive(&entry.path());
            }
        }
    } else {
        // File: r-- (0o400)
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o400));
    }
}

#[cfg(not(unix))]
fn set_readonly_recursive(path: &Path) {
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(path, perms);
    }
}

/// Recursively copy a file or directory (public wrapper for use by deploy).
pub fn copy_recursive_pub(src: &Path, dst: &Path) -> Result<(), String> {
    copy_recursive(src, dst)
}

/// Recursively copy a file or directory.
fn copy_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)
            .map_err(|e| format!("create dir {}: {e}", dst.display()))?;
        let entries = std::fs::read_dir(src)
            .map_err(|e| format!("read dir {}: {e}", src.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read dir entry: {e}"))?;
            let src_child = entry.path();
            let dst_child = dst.join(entry.file_name());
            copy_recursive(&src_child, &dst_child)?;
        }
    } else {
        std::fs::copy(src, dst)
            .map_err(|e| format!("copy {} -> {}: {e}", src.display(), dst.display()))?;
    }
    Ok(())
}

/// Verify injection integrity against the manifest.
pub fn verify_injections(worktree_path: &Path) -> Result<Vec<IntegrityViolation>, String> {
    let manifest_path = worktree_path.join(".agenthalo_manifest.json");
    if !manifest_path.exists() {
        return Err("no manifest found in worktree".to_string());
    }
    let raw = std::fs::read(&manifest_path)
        .map_err(|e| format!("read manifest: {e}"))?;
    let info: WorktreeInfo = serde_json::from_slice(&raw)
        .map_err(|e| format!("parse manifest: {e}"))?;

    let mut violations = Vec::new();
    for record in &info.injections {
        let full_target = worktree_path.join(&record.target);

        // Check symlink/file exists.
        if !full_target.exists() && full_target.symlink_metadata().is_err() {
            violations.push(IntegrityViolation {
                target: record.target.clone(),
                kind: "missing".to_string(),
                detail: "injected path no longer exists in worktree".to_string(),
            });
            continue;
        }

        // Mode-specific integrity checks.
        if record.mode == "readonly" {
            // Readonly injections are read-only copies. Verify permissions.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&full_target) {
                    let mode = meta.permissions().mode();
                    let writable = mode & 0o222 != 0;
                    if writable {
                        violations.push(IntegrityViolation {
                            target: record.target.clone(),
                            kind: "writable".to_string(),
                            detail: format!(
                                "readonly injection has write permissions (mode {:o})",
                                mode & 0o777
                            ),
                        });
                    }
                }
            }
        } else if record.mode == "approved_write" {
            // Approved-write injections are symlinks.
            if !full_target.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
                violations.push(IntegrityViolation {
                    target: record.target.clone(),
                    kind: "not_symlink".to_string(),
                    detail: "expected symlink but found regular file/dir".to_string(),
                });
            }
        }

        // Check source hash if recorded.
        if let Some(expected_hash) = &record.source_hash {
            if let Some(current_hash) = hash_path(&record.source) {
                if &current_hash != expected_hash {
                    violations.push(IntegrityViolation {
                        target: record.target.clone(),
                        kind: "hash_mismatch".to_string(),
                        detail: format!(
                            "source hash changed: expected {}, got {}",
                            &expected_hash[..8],
                            &current_hash[..8]
                        ),
                    });
                }
            }
        }
    }
    Ok(violations)
}

/// Clean up a worktree.
pub fn cleanup_worktree(
    worktree_path: &Path,
    profile: &WorkspaceProfile,
) -> Result<CleanupReport, String> {
    let mut report = CleanupReport {
        worktree_path: worktree_path.display().to_string(),
        ..Default::default()
    };

    if !worktree_path.exists() {
        report.removed = true;
        return Ok(report);
    }

    // Check for dirty files.
    let dirty = run_git(worktree_path, &["status", "--porcelain"])
        .unwrap_or_default();
    report.had_dirty_files = !dirty.is_empty();

    // Archive dirty files if configured.
    if report.had_dirty_files && profile.cleanup.archive_dirty {
        let archive_dir = crate::halo::config::halo_dir().join("worktree_archive");
        std::fs::create_dir_all(&archive_dir)
            .map_err(|e| format!("create archive dir: {e}"))?;
        // Read manifest for session_id.
        let manifest_path = worktree_path.join(".agenthalo_manifest.json");
        let session_id = if manifest_path.exists() {
            std::fs::read(&manifest_path)
                .ok()
                .and_then(|raw| serde_json::from_slice::<WorktreeInfo>(&raw).ok())
                .map(|info| info.session_id)
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            "unknown".to_string()
        };
        let archive_name = format!("{}.tar.gz", session_id);
        let archive_path = archive_dir.join(&archive_name);
        let tar_result = Command::new("tar")
            .arg("-czf")
            .arg(&archive_path)
            .arg("-C")
            .arg(worktree_path)
            .arg(".")
            .output();
        if let Ok(out) = tar_result {
            if out.status.success() {
                report.archived = true;
                report.archive_path = Some(archive_path.display().to_string());
            }
        }
    }

    // Push if configured.
    if profile.cleanup.push_before_remove {
        let unpushed = run_git(
            worktree_path,
            &["log", "--oneline", "origin/master..HEAD"],
        )
        .unwrap_or_default();
        if !unpushed.is_empty() {
            if run_git(worktree_path, &["push", "origin", "HEAD"]).is_ok() {
                report.pushed = true;
            }
        }
    }

    // Restore write permissions on readonly injections so cleanup can delete them.
    restore_write_permissions(worktree_path);

    // Load repo path from manifest to call worktree remove from the main repo.
    let manifest_path = worktree_path.join(".agenthalo_manifest.json");
    let repo_path = if manifest_path.exists() {
        std::fs::read(&manifest_path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<WorktreeInfo>(&raw).ok())
            .map(|info| info.repo_path)
    } else {
        None
    };

    // Remove worktree.
    if let Some(repo) = repo_path {
        let _ = run_git(
            &repo,
            &["worktree", "remove", "--force", &worktree_path.display().to_string()],
        );
    }
    // If git worktree remove didn't clean up, force-remove the directory.
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }
    report.removed = true;

    Ok(report)
}

/// List HALO-managed worktrees (filtered by prefix).
pub fn list_managed_worktrees(
    repo_path: &Path,
    prefix: &str,
) -> Result<Vec<WorktreeInfo>, String> {
    let output = run_git(repo_path, &["worktree", "list", "--porcelain"])?;
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(rest));
            current_branch = None;
        } else if let Some(rest) = line.strip_prefix("branch ") {
            current_branch = Some(rest.to_string());
        } else if line.is_empty() {
            if let Some(path) = current_path.take() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name.starts_with(prefix) {
                    // Try to load manifest.
                    let manifest_path = path.join(".agenthalo_manifest.json");
                    let info = if manifest_path.exists() {
                        std::fs::read(&manifest_path)
                            .ok()
                            .and_then(|raw| serde_json::from_slice::<WorktreeInfo>(&raw).ok())
                    } else {
                        None
                    };
                    worktrees.push(info.unwrap_or(WorktreeInfo {
                        path: path.clone(),
                        session_id: name,
                        branch: current_branch.take().unwrap_or_default(),
                        created_at: 0,
                        repo_path: repo_path.to_path_buf(),
                        injections: Vec::new(),
                    }));
                }
            }
            current_branch = None;
        }
    }
    // Handle last entry without trailing blank line.
    if let Some(path) = current_path {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with(prefix) {
            let manifest_path = path.join(".agenthalo_manifest.json");
            let info = if manifest_path.exists() {
                std::fs::read(&manifest_path)
                    .ok()
                    .and_then(|raw| serde_json::from_slice::<WorktreeInfo>(&raw).ok())
            } else {
                None
            };
            worktrees.push(info.unwrap_or(WorktreeInfo {
                path,
                session_id: name,
                branch: current_branch.unwrap_or_default(),
                created_at: 0,
                repo_path: repo_path.to_path_buf(),
                injections: Vec::new(),
            }));
        }
    }
    Ok(worktrees)
}

/// Prune worktrees exceeding the max lifetime.
pub fn prune_stale_worktrees(
    repo_path: &Path,
    profile: &WorkspaceProfile,
) -> Result<Vec<CleanupReport>, String> {
    let worktrees = list_managed_worktrees(repo_path, &profile.worktree_prefix)?;
    let cutoff = now_unix().saturating_sub(profile.max_lifetime_hours * 3600);
    let mut reports = Vec::new();
    for wt in worktrees {
        if wt.created_at > 0 && wt.created_at < cutoff {
            let report = cleanup_worktree(&wt.path, profile)?;
            reports.push(report);
        }
    }
    Ok(reports)
}

/// Read the injection manifest from a worktree.
pub fn read_manifest(worktree_path: &Path) -> Result<WorktreeInfo, String> {
    let manifest_path = worktree_path.join(".agenthalo_manifest.json");
    let raw = std::fs::read(&manifest_path)
        .map_err(|e| format!("read manifest: {e}"))?;
    serde_json::from_slice(&raw)
        .map_err(|e| format!("parse manifest: {e}"))
}

/// Scan a worktree for injected skills (reads .agents/skills/MANIFEST.json).
pub fn scan_injected_skills(worktree_path: &Path) -> Result<Option<serde_json::Value>, String> {
    let manifest_path = worktree_path.join(".agents/skills/MANIFEST.json");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&manifest_path)
        .map_err(|e| format!("read skill manifest: {e}"))?;
    let val: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| format!("parse skill manifest: {e}"))?;
    Ok(Some(val))
}

/// Scan a worktree for MCP tool configs.
pub fn scan_injected_mcp_tools(worktree_path: &Path) -> Result<Option<serde_json::Value>, String> {
    let mcp_path = worktree_path.join(".mcp.json");
    if !mcp_path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&mcp_path)
        .map_err(|e| format!("read .mcp.json: {e}"))?;
    let val: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| format!("parse .mcp.json: {e}"))?;
    Ok(Some(val))
}

/// Read agent instructions (AGENTS.md, CLAUDE.md, etc.) from a worktree.
pub fn read_injected_instructions(worktree_path: &Path) -> Result<Option<String>, String> {
    for name in &["AGENTS.md", "CLAUDE.md", "GEMINI.md", "CODEX.md"] {
        let path = worktree_path.join(name);
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("read {name}: {e}"))?;
            return Ok(Some(content));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_path_returns_some_for_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("test.txt");
        std::fs::write(&file, b"hello").expect("write");
        let hash = hash_path(&file);
        assert!(hash.is_some());
        assert_eq!(hash.as_ref().unwrap().len(), 64);
        // Deterministic.
        assert_eq!(hash, hash_path(&file));
    }

    #[test]
    fn hash_path_returns_some_for_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), b"a").expect("write");
        std::fs::write(dir.path().join("b.txt"), b"b").expect("write");
        let hash = hash_path(dir.path());
        assert!(hash.is_some());
    }

    #[test]
    fn copy_recursive_copies_files_and_dirs() {
        let src = tempfile::tempdir().expect("src");
        let dst = tempfile::tempdir().expect("dst");
        let dst_target = dst.path().join("copied");

        std::fs::write(src.path().join("file.txt"), b"content").expect("write");
        std::fs::create_dir_all(src.path().join("subdir")).expect("subdir");
        std::fs::write(src.path().join("subdir/nested.txt"), b"nested").expect("write nested");

        copy_recursive(src.path(), &dst_target).expect("copy");
        assert!(dst_target.join("file.txt").exists());
        assert!(dst_target.join("subdir/nested.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dst_target.join("file.txt")).unwrap(),
            "content"
        );
    }

    #[test]
    fn is_git_repo_false_for_plain_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!is_git_repo(dir.path()));
    }

    #[test]
    fn readonly_injection_blocks_writes() {
        let src = tempfile::tempdir().expect("src");
        let wt = tempfile::tempdir().expect("wt");
        let target = wt.path().join("injected");

        std::fs::write(src.path().join("secret.txt"), b"do not modify").expect("write");

        // Copy and set readonly.
        copy_recursive(src.path(), &target).expect("copy");
        set_readonly_recursive(&target);

        // Verify write is denied.
        let result = std::fs::write(target.join("secret.txt"), b"tampered");
        assert!(result.is_err(), "write to readonly injection should fail");

        // Verify content is still original.
        restore_write_permissions(&target);
        let content = std::fs::read_to_string(target.join("secret.txt")).expect("read");
        assert_eq!(content, "do not modify");
    }

    #[test]
    fn set_readonly_recursive_covers_nested_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("dir");
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).expect("mkdir");
        std::fs::write(sub.join("file.txt"), b"test").expect("write");

        set_readonly_recursive(dir.path());

        // File should be read-only (0o400).
        let file_mode = std::fs::metadata(sub.join("file.txt"))
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o400, "file should be r-- (0o400)");

        // Directory should be r-x (0o500).
        let dir_mode = std::fs::metadata(&sub)
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o500, "dir should be r-x (0o500)");

        // Restore for cleanup.
        restore_write_permissions(dir.path());
    }

    #[test]
    fn restore_write_permissions_reenables_writes() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join("file.txt"), b"test").expect("write");
        set_readonly_recursive(dir.path());

        // Cannot write.
        assert!(std::fs::write(dir.path().join("file.txt"), b"x").is_err());

        // Restore.
        restore_write_permissions(dir.path());

        // Can write again.
        std::fs::write(dir.path().join("file.txt"), b"restored").expect("write after restore");
        let content = std::fs::read_to_string(dir.path().join("file.txt")).expect("read");
        assert_eq!(content, "restored");
    }

    #[test]
    fn verify_injections_detects_missing_target() {
        let wt = tempfile::tempdir().expect("wt");
        let info = WorktreeInfo {
            path: wt.path().to_path_buf(),
            session_id: "verify-test".to_string(),
            branch: "test".to_string(),
            created_at: 0,
            repo_path: wt.path().to_path_buf(),
            injections: vec![InjectionRecord {
                source: std::path::PathBuf::from("/tmp/does_not_exist"),
                target: "missing_target".to_string(),
                mode: "readonly".to_string(),
                source_hash: None,
            }],
        };
        let manifest_path = wt.path().join(".agenthalo_manifest.json");
        let manifest_json = serde_json::to_vec_pretty(&info).expect("serialize");
        std::fs::write(&manifest_path, &manifest_json).expect("write manifest");

        let violations = verify_injections(wt.path()).expect("verify");
        assert!(!violations.is_empty());
        assert_eq!(violations[0].kind, "missing");
    }

    #[test]
    fn verify_injections_detects_writable_readonly() {
        use std::os::unix::fs::PermissionsExt;
        let wt = tempfile::tempdir().expect("wt");
        let target = wt.path().join("injected.txt");
        std::fs::write(&target, b"should be readonly").expect("write");
        // Intentionally leave it writable.
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).expect("perms");

        let info = WorktreeInfo {
            path: wt.path().to_path_buf(),
            session_id: "verify-perm-test".to_string(),
            branch: "test".to_string(),
            created_at: 0,
            repo_path: wt.path().to_path_buf(),
            injections: vec![InjectionRecord {
                source: std::path::PathBuf::from("/tmp"),
                target: "injected.txt".to_string(),
                mode: "readonly".to_string(),
                source_hash: None,
            }],
        };
        let manifest_path = wt.path().join(".agenthalo_manifest.json");
        std::fs::write(&manifest_path, serde_json::to_vec_pretty(&info).expect("ser")).expect("write");

        let violations = verify_injections(wt.path()).expect("verify");
        assert!(
            violations.iter().any(|v| v.kind == "writable"),
            "should detect writable readonly injection"
        );
    }

    #[test]
    fn verify_injections_detects_hash_mismatch() {
        let src = tempfile::tempdir().expect("src");
        let wt = tempfile::tempdir().expect("wt");
        let src_file = src.path().join("data.txt");
        std::fs::write(&src_file, b"original content").expect("write");

        let original_hash = hash_path(&src_file).expect("hash");

        // Mutate source after recording hash.
        std::fs::write(&src_file, b"mutated content").expect("mutate");

        let target = wt.path().join("data.txt");
        std::fs::write(&target, b"original content").expect("write target");

        let info = WorktreeInfo {
            path: wt.path().to_path_buf(),
            session_id: "hash-test".to_string(),
            branch: "test".to_string(),
            created_at: 0,
            repo_path: wt.path().to_path_buf(),
            injections: vec![InjectionRecord {
                source: src_file,
                target: "data.txt".to_string(),
                mode: "copy".to_string(),
                source_hash: Some(original_hash),
            }],
        };
        let manifest_path = wt.path().join(".agenthalo_manifest.json");
        std::fs::write(&manifest_path, serde_json::to_vec_pretty(&info).expect("ser")).expect("write");

        let violations = verify_injections(wt.path()).expect("verify");
        assert!(
            violations.iter().any(|v| v.kind == "hash_mismatch"),
            "should detect hash mismatch"
        );
    }

    #[test]
    fn scan_injected_skills_returns_none_when_absent() {
        let dir = tempfile::tempdir().expect("dir");
        let result = scan_injected_skills(dir.path()).expect("scan");
        assert!(result.is_none());
    }

    #[test]
    fn scan_injected_skills_returns_value_when_present() {
        let dir = tempfile::tempdir().expect("dir");
        let skills_dir = dir.path().join(".agents/skills");
        std::fs::create_dir_all(&skills_dir).expect("mkdir");
        std::fs::write(
            skills_dir.join("MANIFEST.json"),
            br#"{"skills": ["proof-tree", "formal-proof"]}"#,
        )
        .expect("write");
        let result = scan_injected_skills(dir.path()).expect("scan");
        assert!(result.is_some());
        let val = result.unwrap();
        assert!(val.get("skills").is_some());
    }

    #[test]
    fn scan_injected_mcp_tools_returns_none_when_absent() {
        let dir = tempfile::tempdir().expect("dir");
        let result = scan_injected_mcp_tools(dir.path()).expect("scan");
        assert!(result.is_none());
    }

    #[test]
    fn scan_injected_mcp_tools_returns_value_when_present() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(
            dir.path().join(".mcp.json"),
            br#"{"mcpServers": {"test": {}}}"#,
        )
        .expect("write");
        let result = scan_injected_mcp_tools(dir.path()).expect("scan");
        assert!(result.is_some());
    }

    #[test]
    fn read_injected_instructions_reads_agents_md() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join("AGENTS.md"), b"# Test instructions").expect("write");
        let result = read_injected_instructions(dir.path()).expect("read");
        assert!(result.is_some());
        assert!(result.unwrap().contains("Test instructions"));
    }

    #[test]
    fn read_injected_instructions_returns_none_when_absent() {
        let dir = tempfile::tempdir().expect("dir");
        let result = read_injected_instructions(dir.path()).expect("read");
        assert!(result.is_none());
    }

    #[test]
    fn read_manifest_roundtrips() {
        let dir = tempfile::tempdir().expect("dir");
        let info = WorktreeInfo {
            path: dir.path().to_path_buf(),
            session_id: "roundtrip-test".to_string(),
            branch: "master".to_string(),
            created_at: 1234567890,
            repo_path: dir.path().to_path_buf(),
            injections: vec![InjectionRecord {
                source: std::path::PathBuf::from("/tmp/test"),
                target: "skills".to_string(),
                mode: "readonly".to_string(),
                source_hash: Some("abcdef1234567890".to_string()),
            }],
        };
        let manifest_path = dir.path().join(".agenthalo_manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&info).expect("serialize"),
        )
        .expect("write");

        let loaded = read_manifest(dir.path()).expect("read manifest");
        assert_eq!(loaded.session_id, "roundtrip-test");
        assert_eq!(loaded.created_at, 1234567890);
        assert_eq!(loaded.injections.len(), 1);
        assert_eq!(loaded.injections[0].mode, "readonly");
    }
}
