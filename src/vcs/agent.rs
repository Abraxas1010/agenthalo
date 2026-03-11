use crate::vcs::{FileOp, WorkRecord, WorkRecordStore};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathConflict {
    pub path: String,
    pub left_hash: [u8; 32],
    pub right_hash: [u8; 32],
    pub left_timestamp: u64,
    pub right_timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeSnapshot {
    pub record_count: usize,
    pub merged_count: usize,
    pub conflict_count: usize,
    pub head_hash: Option<[u8; 32]>,
    pub conflicts: Vec<PathConflict>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportStats {
    pub written_files: usize,
    pub deleted_files: usize,
    pub final_paths: usize,
}

pub fn analyze_records(records: &[WorkRecord]) -> MergeSnapshot {
    let mut ordered = records.to_vec();
    ordered.sort_by_key(|r| (r.timestamp, r.hash));

    let mut latest_by_path: BTreeMap<String, (u64, [u8; 32])> = BTreeMap::new();
    let mut conflicts = Vec::new();
    let mut head_hash = None;
    let mut head_key = (0u64, [0u8; 32]);

    for rec in &ordered {
        let rec_key = (rec.timestamp, rec.hash);
        if rec_key > head_key {
            head_key = rec_key;
            head_hash = Some(rec.hash);
        }

        let touched = match &rec.op {
            FileOp::Create { path, .. } => vec![path.clone()],
            FileOp::Modify { path, .. } => vec![path.clone()],
            FileOp::Delete { path, .. } => vec![path.clone()],
            FileOp::Rename {
                old_path, new_path, ..
            } => vec![old_path.clone(), new_path.clone()],
        };

        for path in touched {
            if let Some((prev_ts, prev_hash)) = latest_by_path.get(&path).copied() {
                if prev_ts == rec.timestamp && prev_hash != rec.hash {
                    conflicts.push(PathConflict {
                        path: path.clone(),
                        left_hash: prev_hash,
                        right_hash: rec.hash,
                        left_timestamp: prev_ts,
                        right_timestamp: rec.timestamp,
                    });
                }
            }
            latest_by_path.insert(path, (rec.timestamp, rec.hash));
        }
    }

    MergeSnapshot {
        record_count: records.len(),
        merged_count: records.len().saturating_sub(conflicts.len()),
        conflict_count: conflicts.len(),
        head_hash,
        conflicts,
    }
}

fn apply_record_to_state(state: &mut BTreeMap<String, Option<[u8; 32]>>, rec: &WorkRecord) {
    match &rec.op {
        FileOp::Create { path, content_hash } => {
            state.insert(path.clone(), Some(*content_hash));
        }
        FileOp::Modify { path, new_hash, .. } => {
            state.insert(path.clone(), Some(*new_hash));
        }
        FileOp::Delete { path, .. } => {
            state.insert(path.clone(), None);
        }
        FileOp::Rename {
            old_path,
            new_path,
            content_hash,
        } => {
            state.insert(old_path.clone(), None);
            state.insert(new_path.clone(), Some(*content_hash));
        }
    }
}

pub fn materialize_state(records: &[WorkRecord]) -> BTreeMap<String, Option<[u8; 32]>> {
    let mut ordered = records.to_vec();
    ordered.sort_by_key(|r| (r.timestamp, r.hash));
    let mut state = BTreeMap::new();
    for rec in &ordered {
        apply_record_to_state(&mut state, rec);
    }
    state
}

pub fn export_state_to_worktree(
    records: &[WorkRecord],
    repo_path: &Path,
) -> Result<ExportStats, String> {
    let state = materialize_state(records);
    fs::create_dir_all(repo_path).map_err(|e| format!("create repo path: {e}"))?;

    let mut written = 0usize;
    let mut deleted = 0usize;
    let mut final_paths = 0usize;

    for (path, value) in state {
        let full = repo_path.join(&path);
        match value {
            Some(hash) => {
                if let Some(parent) = full.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("create parent directories for `{path}`: {e}`"))?;
                }
                let body = format!(
                    "abraxas.materialized.v1\npath={path}\ncontent_hash={}\n",
                    crate::vcs::hash_hex(&hash)
                );
                fs::write(&full, body).map_err(|e| format!("write `{path}`: {e}"))?;
                written += 1;
                final_paths += 1;
            }
            None => {
                if full.exists() {
                    fs::remove_file(&full).map_err(|e| format!("delete `{path}`: {e}"))?;
                    deleted += 1;
                }
            }
        }
    }

    Ok(ExportStats {
        written_files: written,
        deleted_files: deleted,
        final_paths,
    })
}

pub fn git_status_porcelain(repo_path: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("status")
        .arg("--porcelain")
        .output()
        .map_err(|e| format!("run git status: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git status failed: {stderr}"));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn hash_file_or_path(repo_path: &Path, rel_path: &str) -> [u8; 32] {
    let path = repo_path.join(rel_path);
    let bytes = fs::read(path).unwrap_or_else(|_| rel_path.as_bytes().to_vec());
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn work_records_from_workspace(
    repo_path: &Path,
    author_puf: [u8; 32],
    timestamp: u64,
) -> Result<Vec<WorkRecord>, String> {
    let lines = git_status_porcelain(repo_path)?;
    let mut out = Vec::new();
    for line in lines {
        if line.len() < 4 {
            continue;
        }
        let status = &line[..2];
        let path_part = line[3..].trim();
        let op = if status.contains('A') || status == "??" {
            let h = hash_file_or_path(repo_path, path_part);
            FileOp::Create {
                path: path_part.to_string(),
                content_hash: h,
            }
        } else if status.contains('M') {
            let new_hash = hash_file_or_path(repo_path, path_part);
            FileOp::Modify {
                path: path_part.to_string(),
                old_hash: [0u8; 32],
                new_hash,
                patch: None,
            }
        } else if status.contains('D') {
            let h = hash_file_or_path(repo_path, path_part);
            FileOp::Delete {
                path: path_part.to_string(),
                content_hash: h,
            }
        } else if status.contains('R') {
            let mut parts = path_part.split("->").map(str::trim);
            let old_path = parts.next().unwrap_or(path_part).to_string();
            let new_path = parts.next().unwrap_or(path_part).to_string();
            let h = hash_file_or_path(repo_path, &new_path);
            FileOp::Rename {
                old_path,
                new_path,
                content_hash: h,
            }
        } else {
            continue;
        };

        let mut rec = WorkRecord {
            hash: [0u8; 32],
            parents: vec![],
            author_puf,
            timestamp,
            op,
            proof_ref: None,
        };
        rec.hash = WorkRecordStore::compute_hash(&rec);
        out.push(rec);
    }
    Ok(out)
}
