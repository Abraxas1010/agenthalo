//! File browsing and git diff endpoints for the Observatory codefile/codediff viz types.
//!
//! All paths are resolved relative to a workspace root (from the active workspace profile
//! or a query parameter). Path traversal is blocked.

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Maximum file size served by `/api/files/read` (2 MiB).
const MAX_READ_SIZE: u64 = 2 * 1024 * 1024;

/// Build the /api/files sub-router.
pub fn router() -> Router {
    Router::new()
        .route("/tree", get(api_file_tree))
        .route("/read", get(api_file_read))
        .route("/git-status", get(api_git_status))
        .route("/git-diff", get(api_git_diff))
        .route("/recent", get(api_file_recent))
}

#[derive(Deserialize)]
struct TreeQuery {
    /// Directory path relative to workspace root. Empty = root.
    #[serde(default)]
    path: String,
    /// Workspace root override. Falls back to active profile's lean_project_path.
    #[serde(default)]
    root: Option<String>,
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
    #[serde(default)]
    root: Option<String>,
}

#[derive(Deserialize)]
struct DiffQuery {
    path: String,
    #[serde(default)]
    root: Option<String>,
}

#[derive(Deserialize)]
struct RecentQuery {
    #[serde(default = "default_recent_limit")]
    limit: usize,
    #[serde(default)]
    root: Option<String>,
}

fn default_recent_limit() -> usize {
    50
}

// -- Helpers -----------------------------------------------------------------

pub fn resolve_workspace_root(
    override_root: Option<&str>,
) -> Result<PathBuf, (StatusCode, Json<Value>)> {
    if let Some(r) = override_root {
        let p = PathBuf::from(crate::halo::workspace_profile::expand_tilde_pub(r));
        if let Some(root) = normalize_workspace_root(p) {
            return Ok(root);
        }
    }
    let profile = crate::halo::workspace_profile::load_active_profile().unwrap_or_default();
    match profile.lean_project_path.as_deref() {
        Some(p) if !p.trim().is_empty() => {
            let expanded =
                PathBuf::from(crate::halo::workspace_profile::expand_tilde_pub(p));
            if let Some(root) = normalize_workspace_root(expanded) {
                Ok(root)
            } else {
                Err(err(
                    StatusCode::BAD_REQUEST,
                    "workspace root does not exist",
                ))
            }
        }
        _ => {
            let cwd = std::env::current_dir().map_err(|e| {
                err(
                    StatusCode::BAD_REQUEST,
                    &format!("no workspace root configured: {e}"),
                )
            })?;
            if let Some(root) = normalize_workspace_root(cwd) {
                Ok(root)
            } else {
                Err(err(
                    StatusCode::BAD_REQUEST,
                    "no workspace root configured",
                ))
            }
        }
    }
}

fn normalize_workspace_root(path: PathBuf) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }
    if let Ok(repo) = git2::Repository::discover(&path) {
        if let Some(workdir) = repo.workdir() {
            return Some(workdir.to_path_buf());
        }
        if let Some(parent) = repo.path().parent() {
            return Some(parent.to_path_buf());
        }
    }
    // No git repo found — only accept if the directory looks like a project root
    // (contains at least a Cargo.toml, lakefile.lean, or package.json).
    // This prevents falling through to an overly broad CWD like "/".
    let markers = ["Cargo.toml", "lakefile.lean", "package.json", "pyproject.toml"];
    if markers.iter().any(|m| path.join(m).is_file()) {
        return Some(path);
    }
    None
}

pub fn guard_traversal(rel: &str, root: &Path) -> Result<PathBuf, (StatusCode, Json<Value>)> {
    if rel.contains("..") {
        return Err(err(StatusCode::BAD_REQUEST, "path traversal not allowed"));
    }
    let full = root.join(rel);
    // Canonicalize both to catch symlink escapes
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let canon_full = full.canonicalize().unwrap_or_else(|_| full.clone());
    if !canon_full.starts_with(&canon_root) {
        return Err(err(StatusCode::BAD_REQUEST, "path outside workspace"));
    }
    Ok(full)
}

pub fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "ok": false, "error": msg })))
}

fn language_from_ext(ext: &str) -> &'static str {
    match ext {
        "lean" => "lean4",
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "json" => "json",
        "toml" => "toml",
        "md" => "markdown",
        "css" => "css",
        "html" => "html",
        "sh" | "bash" => "shell",
        "yml" | "yaml" => "yaml",
        "c" => "c",
        "cpp" | "cc" | "cxx" => "cpp",
        "h" | "hpp" => "cpp",
        "go" => "go",
        "java" => "java",
        "sol" => "sol",
        _ => "plaintext",
    }
}

// -- GET /api/files/tree -----------------------------------------------------

async fn api_file_tree(Query(q): Query<TreeQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let dir = if q.path.is_empty() {
        root.clone()
    } else {
        match guard_traversal(&q.path, &root) {
            Ok(p) => p,
            Err(e) => return e.into_response(),
        }
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => return Err(format!("read_dir: {e}")),
        };
        let mut items: Vec<_> = read.flatten().collect();
        items.sort_by_key(|e| e.file_name());

        // Try to get git statuses for this directory
        let git_statuses = git_status_map(&root);

        for entry in items {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip hidden files and .lake
            if name.starts_with('.') || name == ".lake" {
                continue;
            }
            let is_dir = entry.path().is_dir();
            let rel = entry
                .path()
                .strip_prefix(&root)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .to_string();
            let ext = entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();

            let git_status = git_statuses
                .as_ref()
                .and_then(|m| m.get(&rel))
                .cloned()
                .unwrap_or_default();

            entries.push(json!({
                "name": name,
                "type": if is_dir { "directory" } else { "file" },
                "path": rel,
                "language": if is_dir { "" } else { language_from_ext(&ext) },
                "git_status": git_status,
            }));
        }
        Ok(entries)
    })
    .await;

    match result {
        Ok(Ok(entries)) => Json(json!({ "ok": true, "entries": entries })).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- GET /api/files/read -----------------------------------------------------

async fn api_file_read(Query(q): Query<FileQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let full = match guard_traversal(&q.path, &root) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if !full.is_file() {
        return err(StatusCode::NOT_FOUND, "file not found").into_response();
    }
    let path = q.path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let meta = std::fs::metadata(&full).map_err(|e| format!("metadata: {e}"))?;
        if meta.len() > MAX_READ_SIZE {
            return Err(format!(
                "file too large ({} bytes, max {})",
                meta.len(),
                MAX_READ_SIZE
            ));
        }
        let ext = full
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        let content =
            std::fs::read_to_string(&full).map_err(|e| format!("read: {e}"))?;
        let size = content.len();
        Ok::<_, String>(json!({
            "ok": true,
            "path": path,
            "content": content,
            "language": language_from_ext(&ext),
            "size": size,
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- GET /api/files/git-status -----------------------------------------------

async fn api_git_status(Query(q): Query<TreeQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let result = tokio::task::spawn_blocking(move || {
        let repo =
            git2::Repository::open(&root).map_err(|e| format!("git open: {e}"))?;
        let statuses = repo
            .statuses(None)
            .map_err(|e| format!("git statuses: {e}"))?;
        let mut changed = Vec::new();
        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let st = entry.status();
            let status_str = format_git_status(st);
            let staged = st.intersects(
                git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_DELETED
                    | git2::Status::INDEX_RENAMED
                    | git2::Status::INDEX_TYPECHANGE,
            );
            changed.push(json!({
                "path": path,
                "status": status_str,
                "staged": staged,
            }));
        }
        Ok::<_, String>(changed)
    })
    .await;

    match result {
        Ok(Ok(changed)) => Json(json!({ "ok": true, "changed": changed })).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- GET /api/files/git-diff -------------------------------------------------

async fn api_git_diff(Query(q): Query<DiffQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let rel_path = q.path.clone();
    let work_path = match guard_traversal(&rel_path, &root) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let repo =
            git2::Repository::open(&root).map_err(|e| format!("git open: {e}"))?;

        // Get HEAD tree content for this file
        let head_content = (|| -> Option<String> {
            let head = repo.head().ok()?;
            let commit = head.peel_to_commit().ok()?;
            let tree = commit.tree().ok()?;
            let entry = tree
                .get_path(std::path::Path::new(&rel_path))
                .ok()?;
            let blob = repo.find_blob(entry.id()).ok()?;
            std::str::from_utf8(blob.content())
                .ok()
                .map(|s| s.to_string())
        })()
        .unwrap_or_default();

        // Get working tree content
        let work_content = std::fs::read_to_string(&work_path).unwrap_or_default();

        let ext = std::path::Path::new(&rel_path)
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        Ok::<_, String>(json!({
            "path": rel_path,
            "original": head_content,
            "modified": work_content,
            "language": language_from_ext(&ext),
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(json!({ "ok": true, "diff": data })).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- GET /api/files/recent --------------------------------------------------

async fn api_file_recent(Query(q): Query<RecentQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let limit = q.limit.min(200);

    let result = tokio::task::spawn_blocking(move || {
        let repo = git2::Repository::open(&root).map_err(|e| format!("git open: {e}"))?;
        let mut revwalk = repo.revwalk().map_err(|e| format!("revwalk: {e}"))?;
        revwalk
            .push_head()
            .map_err(|e| format!("push_head: {e}"))?;
        revwalk
            .set_sorting(git2::Sort::TIME)
            .map_err(|e| format!("sort: {e}"))?;

        let mut files = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut commit_count = 0;

        for oid_result in revwalk {
            if files.len() >= limit || commit_count >= 20 {
                break;
            }
            let oid = match oid_result {
                Ok(o) => o,
                Err(_) => continue,
            };
            let commit = match repo.find_commit(oid) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let tree = match commit.tree() {
                Ok(t) => t,
                Err(_) => continue,
            };

            let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

            let diff = match repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
            {
                Ok(d) => d,
                Err(_) => continue,
            };

            for delta in diff.deltas() {
                if files.len() >= limit {
                    break;
                }
                if let Some(path) = delta.new_file().path() {
                    let path_str = path.to_string_lossy().to_string();
                    if seen.insert(path_str.clone()) {
                        let ext = path
                            .extension()
                            .map(|e| e.to_string_lossy().to_string())
                            .unwrap_or_default();
                        files.push(json!({
                            "path": path_str,
                            "language": language_from_ext(&ext),
                            "commit_time": commit.time().seconds(),
                            "author": commit.author().name().unwrap_or(""),
                            "summary": commit.summary().unwrap_or(""),
                        }));
                    }
                }
            }
            commit_count += 1;
        }

        Ok::<_, String>(files)
    })
    .await;

    match result {
        Ok(Ok(files)) => Json(json!({ "ok": true, "files": files })).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- Git helpers -------------------------------------------------------------

fn git_status_map(root: &Path) -> Option<std::collections::HashMap<String, String>> {
    let repo = git2::Repository::open(root).ok()?;
    let statuses = repo.statuses(None).ok()?;
    let mut map = std::collections::HashMap::new();
    for entry in statuses.iter() {
        if let Some(path) = entry.path() {
            map.insert(path.to_string(), format_git_status(entry.status()));
        }
    }
    Some(map)
}

fn format_git_status(s: git2::Status) -> String {
    if s.contains(git2::Status::WT_NEW) || s.contains(git2::Status::INDEX_NEW) {
        "A".into()
    } else if s.contains(git2::Status::WT_MODIFIED) || s.contains(git2::Status::INDEX_MODIFIED) {
        "M".into()
    } else if s.contains(git2::Status::WT_DELETED) || s.contains(git2::Status::INDEX_DELETED) {
        "D".into()
    } else if s.contains(git2::Status::WT_RENAMED) || s.contains(git2::Status::INDEX_RENAMED) {
        "R".into()
    } else if s.contains(git2::Status::IGNORED) {
        "I".into()
    } else {
        "?".into()
    }
}
