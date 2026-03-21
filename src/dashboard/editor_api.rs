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
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(err(StatusCode::BAD_REQUEST, "absolute paths not allowed"));
    }

    for component in rel_path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(err(StatusCode::BAD_REQUEST, "path traversal not allowed"));
            }
        }
    }

    let full = root.join(rel_path);
    let canonical_probe = nearest_existing_ancestor(&full);
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let canon_probe = canonical_probe
        .canonicalize()
        .unwrap_or_else(|_| canonical_probe.clone());
    if !canon_probe.starts_with(&canon_root) {
        return Err(err(StatusCode::BAD_REQUEST, "path outside workspace"));
    }
    Ok(full)
}

fn nearest_existing_ancestor(path: &Path) -> PathBuf {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            return path.to_path_buf();
        }
    }
    probe
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
    let full_path = match guard_traversal(&q.path, &root) {
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
        })();

        // Get working tree content
        let work_content = std::fs::read_to_string(&full_path).ok();

        if head_content.is_none() && work_content.is_none() {
            return Err("file not found".to_string());
        }

        let ext = std::path::Path::new(&rel_path)
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        Ok::<_, String>(json!({
            "path": rel_path,
            "original": head_content.unwrap_or_default(),
            "modified": work_content.unwrap_or_default(),
            "language": language_from_ext(&ext),
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(json!({ "ok": true, "diff": data })).into_response(),
        Ok(Err(e)) if e == "file not found" => {
            err(StatusCode::NOT_FOUND, "file not found").into_response()
        }
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
        revwalk.push_head().map_err(|e| format!("push_head: {e}"))?;
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

            let diff = match repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) {
                Ok(d) => d,
                Err(_) => continue,
            };

            for delta in diff.deltas() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::util::ServiceExt;

    async fn json_request(app: Router, uri: &str) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let json = serde_json::from_slice::<Value>(&body).expect("json");
        (status, json)
    }

    fn commit_all(repo: &git2::Repository, message: &str) {
        let mut index = repo.index().expect("index");
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .expect("add");
        index.write().expect("index write");
        let tree_id = index.write_tree().expect("tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let sig = git2::Signature::now("Test User", "test@example.com").expect("sig");
        let parents = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .and_then(|oid| repo.find_commit(oid).ok());
        if let Some(parent) = parents.as_ref() {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[parent])
                .expect("commit");
        } else {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
                .expect("initial commit");
        }
    }

    #[tokio::test]
    async fn git_diff_blocks_parent_traversal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let app = router();
        let uri = format!(
            "/git-diff?root={}&path=..%2F..%2Fetc%2Fhosts",
            temp.path().display()
        );
        let (status, json) = json_request(app, &uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["error"], "path traversal not allowed");
    }

    #[tokio::test]
    async fn file_tree_and_read_round_trip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        std::fs::create_dir_all(root.join("src/dashboard")).expect("mkdirs");
        std::fs::write(root.join("src/dashboard/sample.rs"), "fn sample() {}\n").expect("write");

        let app = router();
        let tree_uri = format!("/tree?root={}&path=src%2Fdashboard", root.display());
        let (tree_status, tree_json) = json_request(app.clone(), &tree_uri).await;
        assert_eq!(tree_status, StatusCode::OK);
        assert!(tree_json["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|entry| entry["path"] == "src/dashboard/sample.rs"));

        let read_uri = format!(
            "/read?root={}&path=src%2Fdashboard%2Fsample.rs",
            root.display()
        );
        let (read_status, read_json) = json_request(app, &read_uri).await;
        assert_eq!(read_status, StatusCode::OK);
        assert_eq!(read_json["language"], "rust");
        assert_eq!(read_json["content"], "fn sample() {}\n");
    }

    #[tokio::test]
    async fn recent_respects_limit_and_returns_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let repo = git2::Repository::init(root).expect("repo");

        std::fs::write(root.join("one.rs"), "fn one() {}\n").expect("write one");
        commit_all(&repo, "first commit");

        std::fs::write(root.join("two.rs"), "fn two() {}\n").expect("write two");
        commit_all(&repo, "second commit");

        let app = router();
        let recent_uri = format!("/recent?root={}&limit=1", root.display());
        let (status, json) = json_request(app, &recent_uri).await;
        assert_eq!(status, StatusCode::OK);
        let files = json["files"].as_array().expect("files");
        assert_eq!(files.len(), 1);
        assert!(files[0]["path"].as_str().expect("path").ends_with(".rs"));
        assert_eq!(files[0]["author"], "Test User");
        assert!(files[0]["summary"]
            .as_str()
            .expect("summary")
            .contains("commit"));
        assert!(files[0]["commit_time"].as_i64().expect("commit_time") > 0);
    }
}
