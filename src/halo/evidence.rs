use crate::halo::uncertainty::{translate_uncertainty, UncertaintyKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const EPS: f64 = 1e-12;
pub const EVIDENCE_COMBINATION_FORMAL_BASIS: &str =
    "HeytingLean.EpistemicCalculus.Updating.vUpdate_chain_comm";

/// Tool-provided evidence for a hypothesis.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolEvidence {
    pub tool_name: String,
    pub result: serde_json::Value,
    /// Prior probability this tool is generally reliable.
    pub prior_reliability: f64,
    /// P(E | H)
    pub likelihood_given_true: f64,
    /// P(E | ¬H)
    pub likelihood_given_false: f64,
    /// Optional confidence value from the tool's native framework.
    pub confidence_value: Option<f64>,
    /// Optional framework metadata for confidence_value.
    pub confidence_kind: Option<UncertaintyKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceStep {
    pub tool_name: String,
    pub prior_odds_false_over_true: f64,
    pub likelihood_given_true: f64,
    pub likelihood_given_false: f64,
    pub posterior_odds_false_over_true: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceCombination {
    pub posterior_odds_false_over_true: f64,
    pub posterior_probability_true: f64,
    pub steps: Vec<EvidenceStep>,
}

/// Odds update in false-over-true orientation:
/// posterior_odds = prior_odds * P(E|¬H) / P(E|H),
/// where odds = P(¬H) / P(H).
pub fn vupdate_odds(
    prior_odds_false_over_true: f64,
    likelihood_given_true: f64,
    likelihood_given_false: f64,
) -> f64 {
    let prior = prior_odds_false_over_true.max(EPS);
    let p_e_h = likelihood_given_true.max(EPS);
    let p_e_not_h = likelihood_given_false.max(EPS);
    (prior * p_e_not_h / p_e_h).max(0.0)
}

pub fn combine_evidence(
    prior_odds_false_over_true: f64,
    evidence: &[ToolEvidence],
) -> EvidenceCombination {
    let mut odds = prior_odds_false_over_true.max(EPS);
    let mut steps = Vec::with_capacity(evidence.len());

    for item in evidence {
        let prior = odds;
        odds = vupdate_odds(
            prior,
            item.likelihood_given_true,
            item.likelihood_given_false,
        );
        steps.push(EvidenceStep {
            tool_name: item.tool_name.clone(),
            prior_odds_false_over_true: prior,
            likelihood_given_true: item.likelihood_given_true,
            likelihood_given_false: item.likelihood_given_false,
            posterior_odds_false_over_true: odds,
        });
    }

    EvidenceCombination {
        posterior_odds_false_over_true: odds,
        posterior_probability_true: odds_false_true_to_probability_true(odds),
        steps,
    }
}

/// Convert P(¬H)/P(H) odds to P(H).
pub fn odds_false_true_to_probability_true(odds_false_over_true: f64) -> f64 {
    let o = odds_false_over_true.max(0.0);
    1.0 / (1.0 + o)
}

/// Convert P(H) to P(¬H)/P(H) odds.
pub fn probability_true_to_odds_false_true(probability_true: f64) -> f64 {
    let p = probability_true.clamp(0.0, 1.0);
    if p <= 0.0 {
        f64::INFINITY
    } else {
        (1.0 - p) / p
    }
}

pub fn posterior_probability_true(prior_probability_true: f64, evidence: &[ToolEvidence]) -> f64 {
    let prior_odds = probability_true_to_odds_false_true(prior_probability_true);
    combine_evidence(prior_odds, evidence).posterior_probability_true
}

pub fn normalize_tool_confidence(item: &ToolEvidence) -> Option<f64> {
    let value = item.confidence_value?;
    let kind = item.confidence_kind.unwrap_or(UncertaintyKind::Probability);
    Some(translate_uncertainty(
        kind,
        UncertaintyKind::Probability,
        value,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bayesian_recovery_matches_lean_witness() {
        // Witness from partner instructions:
        // pH=1, pH'=2, pEgH=2, pEgH'=1 => updated odds = 1
        let prior_odds_false_over_true = 2.0;
        let evidence = vec![ToolEvidence {
            tool_name: "test".to_string(),
            result: json!(null),
            prior_reliability: 1.0,
            likelihood_given_true: 2.0,
            likelihood_given_false: 1.0,
            confidence_value: None,
            confidence_kind: None,
        }];

        let combo = combine_evidence(prior_odds_false_over_true, &evidence);
        assert!((combo.posterior_odds_false_over_true - 1.0).abs() < 1e-10);
    }

    #[test]
    fn vupdate_odds_matches_bayes_direction() {
        // Prior fair: odds(¬H/H)=1. Evidence strongly supports H.
        let combo = combine_evidence(
            1.0,
            &[ToolEvidence {
                tool_name: "asym".to_string(),
                result: json!(null),
                prior_reliability: 1.0,
                likelihood_given_true: 0.9,
                likelihood_given_false: 0.1,
                confidence_value: None,
                confidence_kind: None,
            }],
        );
        assert!((combo.posterior_odds_false_over_true - (1.0 / 9.0)).abs() < 1e-10);
        assert!((combo.posterior_probability_true - 0.9).abs() < 1e-10);
    }

    #[test]
    fn combine_evidence_multistep_matches_manual_odds_chain() {
        let evidence = vec![
            ToolEvidence {
                tool_name: "e1".to_string(),
                result: json!(null),
                prior_reliability: 1.0,
                likelihood_given_true: 0.8,
                likelihood_given_false: 0.2,
                confidence_value: None,
                confidence_kind: None,
            },
            ToolEvidence {
                tool_name: "e2".to_string(),
                result: json!(null),
                prior_reliability: 1.0,
                likelihood_given_true: 0.6,
                likelihood_given_false: 0.3,
                confidence_value: None,
                confidence_kind: None,
            },
        ];
        // prior=1, then *0.2/0.8, then *0.3/0.6 => 1/8
        let combo = combine_evidence(1.0, &evidence);
        assert!((combo.posterior_odds_false_over_true - 0.125).abs() < 1e-10);
        assert!((combo.posterior_probability_true - (8.0 / 9.0)).abs() < 1e-10);
    }

    #[test]
    fn probability_odds_roundtrip() {
        let p = 0.7;
        let odds = probability_true_to_odds_false_true(p);
        let recovered = odds_false_true_to_probability_true(odds);
        assert!((recovered - p).abs() < 1e-10);
    }

    #[test]
    fn normalize_confidence_from_certainty_factor() {
        let item = ToolEvidence {
            tool_name: "cf".to_string(),
            result: json!(null),
            prior_reliability: 0.8,
            likelihood_given_true: 0.9,
            likelihood_given_false: 0.3,
            confidence_value: Some(3.0),
            confidence_kind: Some(UncertaintyKind::CertaintyFactor),
        };
        let p = normalize_tool_confidence(&item).expect("confidence");
        assert!((p - 0.75).abs() < 1e-10);
    }
}
