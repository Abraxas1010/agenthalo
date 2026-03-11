use serde::Serialize;
use sha2::{Digest, Sha256};

const ASSERTION_KEYWORDS: &[&str] = &[
    "theorem",
    "lemma",
    "proposition",
    "proof",
    "claim",
    "corollary",
    "definition",
    "assume",
    "suppose",
    "therefore",
    "hence",
    "implies",
    "shows",
    "demonstrates",
    "evidence",
    "result",
    "because",
    "thus",
];

#[derive(Debug, Clone, Serialize)]
pub struct VerificationClaim {
    pub text: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationResult {
    pub valid: bool,
    pub proof_hash: String,
    pub completeness_score: f64,
    pub consistency_score: f64,
    pub structure_score: f64,
    pub claim_count: usize,
    pub section_count: usize,
    pub warnings: Vec<String>,
    pub claims: Vec<VerificationClaim>,
}

pub fn verify_paper(title: &str, content: &str) -> VerificationResult {
    let normalized = content.trim();
    let sections = detect_sections(normalized);
    let claims = extract_claims(normalized);
    let structure_score = structure_score(normalized, sections.len());
    let completeness_score = completeness_score(normalized, claims.len(), sections.len());
    let consistency_score = consistency_score(normalized, &claims);

    let mut warnings = Vec::new();
    if sections.len() < 2 {
        warnings.push("paper has fewer than two structural sections".to_string());
    }
    if claims.is_empty() {
        warnings.push("no high-signal mathematical claims were detected".to_string());
    }
    if normalized.len() < 200 {
        warnings.push("paper content is very short".to_string());
    }

    let valid = claim_count(&claims) >= 2
        && sections.len() >= 2
        && structure_score >= 0.3
        && completeness_score >= 0.3
        && consistency_score >= 0.3;

    VerificationResult {
        valid,
        proof_hash: proof_hash(title, normalized, &claims),
        completeness_score,
        consistency_score,
        structure_score,
        claim_count: claims.len(),
        section_count: sections.len(),
        warnings,
        claims,
    }
}

fn detect_sections(content: &str) -> Vec<&str> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| {
            if line.len() < 4 {
                return false;
            }
            let markdown_header = line.starts_with('#');
            let titled = line
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_whitespace() || c == ':' || c == '-');
            markdown_header || titled
        })
        .collect()
}

fn extract_claims(content: &str) -> Vec<VerificationClaim> {
    content
        .split_terminator(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|sentence| sentence.len() >= 32)
        .filter_map(|sentence| {
            let lowercase = sentence.to_ascii_lowercase();
            let hits = ASSERTION_KEYWORDS
                .iter()
                .filter(|kw| lowercase.contains(**kw))
                .count();
            if hits == 0 {
                return None;
            }
            let score = (hits as f64 / 4.0).min(1.0);
            Some(VerificationClaim {
                text: sentence.chars().take(240).collect(),
                score,
            })
        })
        .take(16)
        .collect()
}

fn structure_score(content: &str, section_count: usize) -> f64 {
    let length_factor = (content.len() as f64 / 1600.0).min(1.0);
    let section_factor = (section_count as f64 / 4.0).min(1.0);
    0.55 * length_factor + 0.45 * section_factor
}

fn completeness_score(content: &str, claim_count: usize, section_count: usize) -> f64 {
    let claim_factor = (claim_count as f64 / 6.0).min(1.0);
    let section_factor = (section_count as f64 / 5.0).min(1.0);
    let citation_factor = if content.contains('[') && content.contains(']') {
        1.0
    } else {
        0.4
    };
    0.5 * claim_factor + 0.3 * section_factor + 0.2 * citation_factor
}

fn consistency_score(content: &str, claims: &[VerificationClaim]) -> f64 {
    let lowercase = content.to_ascii_lowercase();
    let has_proof_language = lowercase.contains("proof") || lowercase.contains("demonstrate");
    let has_assumption_language = lowercase.contains("assume") || lowercase.contains("suppose");
    let claim_factor = (claims.len() as f64 / 5.0).min(1.0);
    let discourse_factor = [has_proof_language, has_assumption_language]
        .into_iter()
        .filter(|v| *v)
        .count() as f64
        / 2.0;
    0.65 * claim_factor + 0.35 * discourse_factor
}

fn proof_hash(title: &str, content: &str, claims: &[VerificationClaim]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(b"\n");
    hasher.update(content.as_bytes());
    hasher.update(b"\nclaims:\n");
    for claim in claims {
        hasher.update(claim.text.as_bytes());
        hasher.update(format!("|{:.3}\n", claim.score).as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn claim_count(claims: &[VerificationClaim]) -> usize {
    claims.len()
}

#[cfg(test)]
mod tests {
    use super::verify_paper;

    #[test]
    fn verify_paper_accepts_structured_submission() {
        let content = r#"
# Introduction
We prove a theorem about compositional verification and provide evidence for each stage.

# Main Theorem
Theorem. Suppose a verifier receives a structured proof sketch and supporting evidence.
Proof. Because the submission includes explicit assumptions, a result statement, and a proof outline,
the verification pipeline can extract stable claims and compute a proof hash.

# Discussion
Therefore the paper demonstrates a consistent argument with enough structure for review.
[1] Internal verification note.
"#;
        let result = verify_paper("Structured Verification", content);
        assert!(result.valid);
        assert_eq!(result.proof_hash.len(), 64);
        assert!(result.claim_count >= 2);
    }
}
