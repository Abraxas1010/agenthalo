use anyhow::Result;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// Discover all `.lean` files under `dir`, respecting .gitignore.
pub fn scan_lean_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkBuilder::new(dir)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build()
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "lean" {
                    // Skip .lake directory (build cache)
                    let rel = path.strip_prefix(dir).unwrap_or(path);
                    let rel_str = rel.to_string_lossy();
                    if !rel_str.starts_with(".lake") && !rel_str.contains("/.lake/") {
                        files.push(path.to_owned());
                    }
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_scan_finds_lean_files() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("Sub")).unwrap();
        fs::write(base.join("A.lean"), "-- file A").unwrap();
        fs::write(base.join("Sub/B.lean"), "-- file B").unwrap();
        fs::write(base.join("other.txt"), "not lean").unwrap();

        let files = scan_lean_files(base).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_scan_skips_lake() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join(".lake/packages")).unwrap();
        fs::write(base.join("A.lean"), "-- ok").unwrap();
        fs::write(base.join(".lake/packages/Dep.lean"), "-- skip").unwrap();

        let files = scan_lean_files(base).unwrap();
        assert_eq!(files.len(), 1);
    }
}
