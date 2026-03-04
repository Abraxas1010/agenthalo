use crate::halo::hash::{self, HashAlgorithm};
use crate::halo::schema::SessionStatus;
use crate::halo::trace::{list_sessions, now_unix_secs, paid_operations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TrustScoreResult {
    pub score: f64,
    pub tier: String,
    pub total_sessions: u64,
    pub completed_sessions: u64,
    pub failed_sessions: u64,
    pub paid_operations_success: u64,
    pub paid_operations_failed: u64,
    pub attestation_count: u64,
    pub anonymous_attestation_count: u64,
    pub recent_sessions_30d: u64,
    pub session_id: Option<String>,
    pub digest: String,
    pub timestamp: u64,
}

pub fn query_trust_score(
    db_path: &Path,
    session_id: Option<&str>,
) -> Result<TrustScoreResult, String> {
    let sessions = list_sessions(db_path)?;
    if let Some(sid) = session_id {
        let exists = sessions.iter().any(|s| s.session_id == sid);
        if !exists {
            return Err(format!("session not found: {sid}"));
        }
    }

    let ops = paid_operations(db_path)?;
    let now = now_unix_secs();
    let window_30d = 30 * 24 * 60 * 60;

    let total_sessions = sessions.len() as u64;
    let completed_sessions = sessions
        .iter()
        .filter(|s| s.status == SessionStatus::Completed)
        .count() as u64;
    let failed_sessions = sessions
        .iter()
        .filter(|s| matches!(s.status, SessionStatus::Failed | SessionStatus::Interrupted))
        .count() as u64;
    let recent_sessions_30d = sessions
        .iter()
        .filter(|s| now.saturating_sub(s.started_at) <= window_30d)
        .count() as u64;

    let paid_operations_success = ops.iter().filter(|op| op.success).count() as u64;
    let paid_operations_failed = ops.iter().filter(|op| !op.success).count() as u64;
    let attestation_count = ops
        .iter()
        .filter(|op| {
            op.success && (op.operation_type == "attest" || op.operation_type == "attest_anon")
        })
        .count() as u64;
    let anonymous_attestation_count = ops
        .iter()
        .filter(|op| op.success && op.operation_type == "attest_anon")
        .count() as u64;

    let completion_rate = ratio(completed_sessions, total_sessions).unwrap_or(0.5);
    let paid_success_rate = ratio(
        paid_operations_success,
        paid_operations_success.saturating_add(paid_operations_failed),
    )
    .unwrap_or(0.5);
    let attestation_factor = (attestation_count as f64 / 25.0).min(1.0);
    let recency_factor = (recent_sessions_30d as f64 / 20.0).min(1.0);
    let failure_penalty = 1.0 - (failed_sessions as f64 / 30.0).min(0.4);

    let mut score = 0.20
        + 0.30 * completion_rate
        + 0.25 * paid_success_rate
        + 0.15 * attestation_factor
        + 0.10 * recency_factor;
    if session_id.is_some() {
        score += 0.02;
    }
    score *= failure_penalty;
    score = score.clamp(0.0, 1.0);

    let tier = trust_tier(score).to_string();
    let digest = score_digest(ScoreDigestInput {
        score,
        total_sessions,
        completed_sessions,
        failed_sessions,
        paid_success: paid_operations_success,
        paid_failed: paid_operations_failed,
        attestation_count,
        anonymous_attestation_count,
        recent_sessions_30d,
        session_id,
    });

    Ok(TrustScoreResult {
        score,
        tier,
        total_sessions,
        completed_sessions,
        failed_sessions,
        paid_operations_success,
        paid_operations_failed,
        attestation_count,
        anonymous_attestation_count,
        recent_sessions_30d,
        session_id: session_id.map(|s| s.to_string()),
        digest,
        timestamp: now,
    })
}

fn ratio(a: u64, b: u64) -> Option<f64> {
    if b == 0 {
        None
    } else {
        Some((a as f64) / (b as f64))
    }
}

fn trust_tier(score: f64) -> &'static str {
    if score >= 0.85 {
        "high"
    } else if score >= 0.65 {
        "medium"
    } else if score >= 0.40 {
        "cautious"
    } else {
        "low"
    }
}

struct ScoreDigestInput<'a> {
    score: f64,
    total_sessions: u64,
    completed_sessions: u64,
    failed_sessions: u64,
    paid_success: u64,
    paid_failed: u64,
    attestation_count: u64,
    anonymous_attestation_count: u64,
    recent_sessions_30d: u64,
    session_id: Option<&'a str>,
}

fn score_digest(input: ScoreDigestInput<'_>) -> String {
    let payload = format!(
        "agenthalo.trust.score.v1:{score:.6}:{total_sessions}:{completed_sessions}:{failed_sessions}:{paid_success}:{paid_failed}:{attestation_count}:{anonymous_attestation_count}:{recent_sessions_30d}:{}",
        input.session_id.unwrap_or(""),
        score = input.score,
        total_sessions = input.total_sessions,
        completed_sessions = input.completed_sessions,
        failed_sessions = input.failed_sessions,
        paid_success = input.paid_success,
        paid_failed = input.paid_failed,
        attestation_count = input.attestation_count,
        anonymous_attestation_count = input.anonymous_attestation_count,
        recent_sessions_30d = input.recent_sessions_30d,
    );
    hash::hash_hex(&HashAlgorithm::CURRENT, payload.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::schema::{PaidOperation, SessionMetadata};
    use crate::halo::trace::TraceWriter;

    fn temp_db_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "agenthalo_trust_{tag}_{}_{}.ndb",
            std::process::id(),
            now_unix_secs()
        ))
    }

    #[test]
    fn baseline_score_without_history() {
        let db_path = temp_db_path("baseline");
        let out = query_trust_score(&db_path, None).expect("score");
        assert!(out.score > 0.0);
        assert!(out.score <= 1.0);
    }

    #[test]
    fn trust_score_reflects_sessions_and_paid_ops() {
        let db_path = temp_db_path("weighted");
        let mut writer = TraceWriter::new(&db_path).expect("writer");
        writer
            .start_session(SessionMetadata {
                session_id: "sess-trust-ok".to_string(),
                agent: "codex".to_string(),
                model: None,
                started_at: now_unix_secs(),
                ended_at: None,
                prompt: None,
                status: SessionStatus::Running,
                user_id: None,
                machine_id: None,
                puf_digest: None,
            })
            .expect("start");
        writer
            .end_session(SessionStatus::Completed)
            .expect("complete");
        writer
            .record_paid_operation(PaidOperation {
                operation_id: "op-1".to_string(),
                timestamp: now_unix_secs(),
                operation_type: "attest".to_string(),
                credits_spent: 10,
                usd_equivalent: 0.1,
                session_id: Some("sess-trust-ok".to_string()),
                result_digest: Some("abc".to_string()),
                success: true,
                error: None,
            })
            .expect("paid op");

        let out = query_trust_score(&db_path, Some("sess-trust-ok")).expect("score");
        assert!(out.score >= 0.5);
        assert_eq!(out.attestation_count, 1);
        assert_eq!(out.session_id.as_deref(), Some("sess-trust-ok"));
    }
}
