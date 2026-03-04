//! Thin parser/checker for Lean4 export-like proof certificate files.
//!
//! Extended with certificate metadata parsing for freshness/authenticity checks.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use ed25519_dalek::{
    Signature as Ed25519Signature, Verifier as _, VerifyingKey as Ed25519VerifyingKey,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MULTICODEC_ED25519_PUB: &[u8] = &[0xed, 0x01];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    Untrusted,
    Legacy,
    Standard,
    CryptoExtended,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExportMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theorem_statement_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_ed25519: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key_multibase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_valid: Option<bool>,
}

impl ExportMetadata {
    pub fn is_legacy(&self) -> bool {
        self.commit_hash.is_none()
            && self.theorem_statement_sha256.is_none()
            && self.generated_at.is_none()
            && self.signature_ed25519.is_none()
            && self.signing_did.is_none()
            && self.signing_key_multibase.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationResult {
    pub all_checked: bool,
    pub declarations_checked: usize,
    pub declarations_failed: usize,
    pub axioms_used: Vec<String>,
    pub axioms_trusted: bool,
    pub theorem_names: Vec<String>,
    pub trust_tier: TrustTier,
    pub metadata: ExportMetadata,
    pub errors: Vec<String>,
    pub elapsed_ms: u64,
}

const STANDARD_AXIOMS: &[&str] = &["propext", "Classical.choice", "Quot.sound"];
const HKDF_AXIOM: &str = "HeytingLean.NucleusDB.Comms.Identity.hkdf_is_prf";
const TRUSTED_AXIOMS: &[&str] = &["propext", "Classical.choice", "Quot.sound", HKDF_AXIOM];

fn decode_ed25519_verifying_key(multibase_key: &str) -> Result<Ed25519VerifyingKey, String> {
    let (_, decoded) = multibase::decode(multibase_key)
        .map_err(|e| format!("decode metadata signing key multibase: {e}"))?;
    if decoded.len() < MULTICODEC_ED25519_PUB.len() || !decoded.starts_with(MULTICODEC_ED25519_PUB)
    {
        return Err("metadata signing key has unexpected multicodec prefix".to_string());
    }
    let raw = &decoded[MULTICODEC_ED25519_PUB.len()..];
    let key: [u8; 32] = raw
        .try_into()
        .map_err(|_| "metadata signing key must be 32-byte Ed25519 key".to_string())?;
    Ed25519VerifyingKey::from_bytes(&key)
        .map_err(|e| format!("metadata signing key is invalid Ed25519 key: {e}"))
}

fn parse_meta(metadata: &mut ExportMetadata, key: &str, value: &str, errors: &mut Vec<String>) {
    match key {
        "commit_hash" => metadata.commit_hash = Some(value.to_string()),
        "theorem_statement_sha256" => metadata.theorem_statement_sha256 = Some(value.to_string()),
        "generated_at" => match value.parse::<u64>() {
            Ok(v) => metadata.generated_at = Some(v),
            Err(e) => errors.push(format!("invalid #META generated_at `{value}`: {e}")),
        },
        "signature_ed25519" => metadata.signature_ed25519 = Some(value.to_string()),
        "signing_did" => metadata.signing_did = Some(value.to_string()),
        "signing_key_multibase" => metadata.signing_key_multibase = Some(value.to_string()),
        _ => errors.push(format!("unknown #META key `{key}`")),
    }
}

pub fn verify_export(export_path: &Path) -> Result<VerificationResult, String> {
    let start = std::time::Instant::now();
    let raw = std::fs::read_to_string(export_path)
        .map_err(|e| format!("read export {}: {e}", export_path.display()))?;

    let mut decl_count = 0usize;
    let mut axioms = Vec::new();
    let mut theorems = Vec::new();
    let mut metadata = ExportMetadata::default();
    let mut errors = Vec::new();
    let mut signable_lines: Vec<String> = Vec::new();

    for line in raw.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("--") {
            continue;
        }

        if let Some(rest) = line.strip_prefix("#META") {
            let rest = rest.trim();
            let mut parts = rest.splitn(2, char::is_whitespace);
            let key = parts.next().unwrap_or_default().trim();
            let value = parts.next().unwrap_or_default().trim();
            if key.is_empty() || value.is_empty() {
                errors.push(format!("invalid #META line `{line}`"));
                continue;
            }
            parse_meta(&mut metadata, key, value, &mut errors);
            if key != "signature_ed25519" {
                signable_lines.push(line.to_string());
            }
            continue;
        }

        signable_lines.push(line.to_string());

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

    metadata.signature_valid =
        if metadata.signature_ed25519.is_none() && metadata.signing_key_multibase.is_none() {
            None
        } else {
            match (
                metadata.signature_ed25519.as_deref(),
                metadata.signing_key_multibase.as_deref(),
            ) {
                (Some(sig_b64), Some(key_mb)) => {
                    let payload = signable_lines.join("\n");
                    let vk = match decode_ed25519_verifying_key(key_mb) {
                        Ok(v) => v,
                        Err(e) => {
                            errors.push(e);
                            return Ok(VerificationResult {
                                all_checked: false,
                                declarations_checked: decl_count,
                                declarations_failed: 0,
                                axioms_used: axioms,
                                axioms_trusted: false,
                                theorem_names: theorems,
                                trust_tier: TrustTier::Untrusted,
                                metadata,
                                errors,
                                elapsed_ms: start.elapsed().as_millis() as u64,
                            });
                        }
                    };
                    let sig_bytes = match B64.decode(sig_b64) {
                        Ok(v) => v,
                        Err(e) => {
                            errors.push(format!("decode #META signature_ed25519: {e}"));
                            Vec::new()
                        }
                    };
                    if sig_bytes.is_empty() {
                        Some(false)
                    } else {
                        match Ed25519Signature::from_slice(&sig_bytes) {
                            Ok(sig) => Some(vk.verify(payload.as_bytes(), &sig).is_ok()),
                            Err(e) => {
                                errors.push(format!("parse #META signature_ed25519: {e}"));
                                Some(false)
                            }
                        }
                    }
                }
                _ => {
                    errors.push(
                    "metadata signature requires both signing_key_multibase and signature_ed25519"
                        .to_string(),
                );
                    Some(false)
                }
            }
        };

    let axioms_trusted = axioms
        .iter()
        .all(|a| TRUSTED_AXIOMS.iter().any(|t| t == &a.as_str()));
    let uses_only_standard = axioms
        .iter()
        .all(|a| STANDARD_AXIOMS.iter().any(|t| t == &a.as_str()));

    let trust_tier = if !axioms_trusted {
        TrustTier::Untrusted
    } else if metadata.is_legacy() {
        TrustTier::Legacy
    } else if uses_only_standard {
        TrustTier::Standard
    } else if axioms.iter().any(|a| a == HKDF_AXIOM) {
        TrustTier::CryptoExtended
    } else {
        TrustTier::Standard
    };

    let all_checked = errors.is_empty();

    Ok(VerificationResult {
        all_checked,
        declarations_checked: decl_count,
        declarations_failed: 0,
        axioms_used: axioms,
        axioms_trusted,
        theorem_names: theorems,
        trust_tier,
        metadata,
        errors,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

pub fn has_theorem(result: &VerificationResult, theorem_name: &str) -> bool {
    result.theorem_names.iter().any(|t| t == theorem_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey as Ed25519SigningKey};

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

    fn signed_export_body(theorem_hash: &str) -> String {
        let signing_key = Ed25519SigningKey::from_bytes(&[7u8; 32]);
        let public = signing_key.verifying_key().to_bytes();
        let key_mb = {
            let mut payload = Vec::with_capacity(34);
            payload.extend_from_slice(MULTICODEC_ED25519_PUB);
            payload.extend_from_slice(&public);
            multibase::encode(multibase::Base::Base58Btc, payload)
        };
        let preimage = format!(
            "#THM HeytingLean.NucleusDB.Core.replay_preserves\n#AX propext\n#META commit_hash deadbeef\n#META theorem_statement_sha256 {}\n#META generated_at 1700000000\n#META signing_did did:key:z6MkvTest\n#META signing_key_multibase {}",
            theorem_hash,
            key_mb
        );
        let sig = signing_key.sign(preimage.as_bytes()).to_bytes();
        let sig_b64 = B64.encode(sig);
        format!("{}\n#META signature_ed25519 {}\n", preimage, sig_b64)
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
        assert_eq!(out.trust_tier, TrustTier::Legacy);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn flags_untrusted_axioms() {
        let file = temp_file("axiom", "#THM T\n#AX Unknown.Axiom\n");
        let out = verify_export(&file).expect("verify export");
        assert!(!out.axioms_trusted);
        assert_eq!(out.trust_tier, TrustTier::Untrusted);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn parses_metadata_and_verifies_signature() {
        let file = temp_file("meta", &signed_export_body("sha256:abc123"));
        let out = verify_export(&file).expect("verify export");
        assert!(out.axioms_trusted);
        assert_eq!(out.trust_tier, TrustTier::Standard);
        assert_eq!(
            out.metadata.theorem_statement_sha256.as_deref(),
            Some("sha256:abc123")
        );
        assert_eq!(out.metadata.signature_valid, Some(true));
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn bad_signature_is_marked_invalid() {
        let body =
            signed_export_body("sha256:xyz").replace("signature_ed25519 ", "signature_ed25519 bad");
        let file = temp_file("meta_bad_sig", &body);
        let out = verify_export(&file).expect("verify export");
        assert_eq!(out.metadata.signature_valid, Some(false));
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn missing_file_errors() {
        let path = std::env::temp_dir().join("does_not_exist.lean4export");
        assert!(verify_export(&path).is_err());
    }
}
