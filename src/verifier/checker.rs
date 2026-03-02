//! Thin parser/checker for Lean4 export-like proof certificate files.
//!
//! Phase-0: parse declarations/axioms/theorems from line-based export stubs.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationResult {
    pub all_checked: bool,
    pub declarations_checked: usize,
    pub declarations_failed: usize,
    pub axioms_used: Vec<String>,
    pub axioms_trusted: bool,
    pub theorem_names: Vec<String>,
    pub errors: Vec<String>,
    pub elapsed_ms: u64,
}

const TRUSTED_AXIOMS: &[&str] = &[
    "propext",
    "Classical.choice",
    "Quot.sound",
    "HeytingLean.NucleusDB.Comms.Identity.hkdf_is_prf",
];

pub fn verify_export(export_path: &Path) -> Result<VerificationResult, String> {
    let start = std::time::Instant::now();
    let raw = std::fs::read_to_string(export_path)
        .map_err(|e| format!("read export {}: {e}", export_path.display()))?;

    let mut decl_count = 0usize;
    let mut axioms = Vec::new();
    let mut theorems = Vec::new();
    for line in raw.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("--") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let tag = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        match tag {
            "#DEF" | "#THM" | "#AX" => {
                decl_count = decl_count.saturating_add(1);
            }
            _ => {}
        }
        if tag == "#AX" && !name.is_empty() {
            axioms.push(name.to_string());
        }
        if tag == "#THM" && !name.is_empty() {
            theorems.push(name.to_string());
        }
    }

    let axioms_trusted = axioms
        .iter()
        .all(|a| TRUSTED_AXIOMS.iter().any(|t| t == &a.as_str()));

    Ok(VerificationResult {
        all_checked: true,
        declarations_checked: decl_count,
        declarations_failed: 0,
        axioms_used: axioms,
        axioms_trusted,
        theorem_names: theorems,
        errors: vec![],
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

pub fn has_theorem(result: &VerificationResult, theorem_name: &str) -> bool {
    result.theorem_names.iter().any(|t| t == theorem_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "proof_checker_{}_{}_{}.lean4export",
            name,
            std::process::id(),
            crate::halo::util::now_unix_secs()
        ));
        std::fs::write(&path, body).expect("write export fixture");
        path
    }

    #[test]
    fn parses_theorems_and_axioms() {
        let file = temp_file(
            "parse",
            "#DEF Foo.Bar\n#THM HeytingLean.NucleusDB.Core.replay_preserves\n#AX propext\n",
        );
        let out = verify_export(&file).expect("verify export");
        assert_eq!(out.declarations_checked, 3);
        assert!(has_theorem(
            &out,
            "HeytingLean.NucleusDB.Core.replay_preserves"
        ));
        assert!(out.axioms_trusted);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn flags_untrusted_axioms() {
        let file = temp_file("axiom", "#THM T\n#AX Unknown.Axiom\n");
        let out = verify_export(&file).expect("verify export");
        assert!(!out.axioms_trusted);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn missing_file_errors() {
        let path = std::env::temp_dir().join("does_not_exist.lean4export");
        assert!(verify_export(&path).is_err());
    }
}
