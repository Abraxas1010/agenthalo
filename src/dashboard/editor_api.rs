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
use std::path::{Component, Path, PathBuf};

/// Build the /api/files sub-router.
pub fn router() -> Router {
    Router::new()
        .route("/tree", get(api_file_tree))
        .route("/read", get(api_file_read))
        .route("/git-status", get(api_git_status))
        .route("/git-diff", get(api_git_diff))
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

// -- Helpers -----------------------------------------------------------------

fn resolve_workspace_root(
    override_root: Option<&str>,
) -> Result<PathBuf, (StatusCode, Json<Value>)> {
    if let Some(r) = override_root {
        let p = PathBuf::from(crate::halo::workspace_profile::expand_tilde_pub(r));
        if p.is_dir() {
            return Ok(p);
        }
    }
    let profile = crate::halo::workspace_profile::load_active_profile().unwrap_or_default();
    match profile.lean_project_path.as_deref() {
        Some(p) if !p.trim().is_empty() => {
            let expanded = PathBuf::from(crate::halo::workspace_profile::expand_tilde_pub(p));
            if expanded.is_dir() {
                Ok(expanded)
            } else {
                Err(err(
                    StatusCode::BAD_REQUEST,
                    "workspace root does not exist",
                ))
            }
        }
        _ => Err(err(StatusCode::BAD_REQUEST, "no workspace root configured")),
    }
}

fn guard_traversal(rel: &str, root: &Path) -> Result<PathBuf, (StatusCode, Json<Value>)> {
    validate_relative_path(rel)?;
    let full = root.join(rel);
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let check_path = nearest_existing_ancestor(&full).unwrap_or_else(|| root.to_path_buf());
    let canon_check = check_path
        .canonicalize()
        .unwrap_or_else(|_| check_path.clone());
    if !canon_check.starts_with(&canon_root) {
        return Err(err(StatusCode::BAD_REQUEST, "path outside workspace"));
    }
    Ok(full)
}

fn validate_relative_path(rel: &str) -> Result<(), (StatusCode, Json<Value>)> {
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(err(StatusCode::BAD_REQUEST, "absolute paths not allowed"));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(err(StatusCode::BAD_REQUEST, "path traversal not allowed"));
            }
        }
    }
    Ok(())
}

fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
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
        items.sort_by_key(|entry| {
            (
                !entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false),
                entry.file_name(),
            )
        });

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
        Ok((entries, q.path))
    })
    .await;

    match result {
        Ok(Ok((entries, path))) => {
            Json(json!({ "ok": true, "path": path, "entries": entries })).into_response()
        }
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
    let ext = full
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    match std::fs::read_to_string(&full) {
        Ok(content) => {
            let size = content.len();
            Json(json!({
                "ok": true,
                "path": q.path,
                "content": content,
                "language": language_from_ext(&ext),
                "size": size,
            }))
            .into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("read: {e}")).into_response(),
    }
}

// -- GET /api/files/git-status -----------------------------------------------

async fn api_git_status(Query(q): Query<TreeQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    let result = tokio::task::spawn_blocking(move || {
        let repo = git2::Repository::open(&root).map_err(|e| format!("git open: {e}"))?;
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
    let work_path = match guard_traversal(&q.path, &root) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let rel_path = q.path.clone();

    let result = tokio::task::spawn_blocking(move || {
        let repo = git2::Repository::open(&root).map_err(|e| format!("git open: {e}"))?;

        // Get HEAD tree content for this file
        let head_content = (|| -> Option<String> {
            let head = repo.head().ok()?;
            let commit = head.peel_to_commit().ok()?;
            let tree = commit.tree().ok()?;
            let entry = tree.get_path(std::path::Path::new(&rel_path)).ok()?;
            let blob = repo.find_blob(entry.id()).ok()?;
            std::str::from_utf8(blob.content())
                .ok()
                .map(|s| s.to_string())
        })()
        .unwrap_or_default();

        // Get working tree content
        let work_content = if work_path.is_file() {
            std::fs::read_to_string(&work_path).unwrap_or_default()
        } else {
            String::new()
        };

        if head_content.is_empty() && work_content.is_empty() && !work_path.exists() {
            return Err("file not found".to_string());
        }

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
        Ok(Err(e)) if e == "file not found" => err(StatusCode::NOT_FOUND, &e).into_response(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use serde_json::Value;
    use tower::ServiceExt;

    async fn request_json(uri: &str) -> (StatusCode, Value) {
        let app = router();
        let response = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        (status, value)
    }

    #[tokio::test]
    async fn git_diff_blocks_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git2::Repository::init(&root).unwrap();
        std::fs::write(temp.path().join("secret.txt"), "secret").unwrap();

        let uri = format!("/git-diff?root={}&path=../secret.txt", root.display());
        let (status, body) = request_json(&uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "path traversal not allowed");
    }

    #[tokio::test]
    async fn file_tree_and_read_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".lake")).unwrap();
        std::fs::write(root.join("src").join("Main.lean"), "def hello := 1\n").unwrap();
        std::fs::write(root.join(".hidden"), "skip").unwrap();

        let tree_uri = format!("/tree?root={}&path=src", root.display());
        let (tree_status, tree_body) = request_json(&tree_uri).await;
        assert_eq!(tree_status, StatusCode::OK);
        assert_eq!(tree_body["path"], "src");
        let entries = tree_body["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["path"], "src/Main.lean");

        let read_uri = format!("/read?root={}&path=src/Main.lean", root.display());
        let (read_status, read_body) = request_json(&read_uri).await;
        assert_eq!(read_status, StatusCode::OK);
        assert_eq!(read_body["language"], "lean4");
        assert_eq!(read_body["content"], "def hello := 1\n");
    }
}
