use crate::halo::schema::{EventType, TraceEvent};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Tsallis 2-entropy (Gini impurity) of a discrete distribution.
/// Matches the HeytingLean tsallisEntropy specialization at q=2.
pub fn tsallis2(distribution: &[f64]) -> f64 {
    1.0 - distribution.iter().map(|p| p * p).sum::<f64>()
}

/// Enriched similarity between two distributions.
/// sim(p1, p2) = Σ p1(v) * p2(v)
pub fn enriched_similarity(p1: &[f64], p2: &[f64]) -> f64 {
    assert_eq!(
        p1.len(),
        p2.len(),
        "enriched_similarity requires equal support size"
    );
    p1.iter().zip(p2.iter()).map(|(a, b)| a * b).sum()
}

/// Strategy diversity score in [0, 100], normalized by the maximum Tsallis value
/// for the number of observed categories.
pub fn agent_diversity_score(tool_counts: &[u64]) -> f64 {
    let total: u64 = tool_counts.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let n = tool_counts.len();
    if n <= 1 {
        return 0.0;
    }
    let dist: Vec<f64> = tool_counts
        .iter()
        .map(|&c| c as f64 / total as f64)
        .collect();
    let raw = tsallis2(&dist);
    let max_entropy = 1.0 - 1.0 / n as f64;
    if max_entropy <= 0.0 {
        return 0.0;
    }
    (raw / max_entropy * 100.0).clamp(0.0, 100.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiversitySnapshot {
    pub score: f64,
    pub raw_tsallis: f64,
    pub max_tsallis: f64,
    pub total_calls: u64,
    pub tools: BTreeMap<String, u64>,
    pub distribution: BTreeMap<String, f64>,
}

pub fn extract_tool_counts(events: &[TraceEvent]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::<String, u64>::new();
    for event in events {
        if !matches!(
            event.event_type,
            EventType::ToolCall | EventType::McpToolCall
        ) {
            continue;
        }
        let Some(tool_name) = extract_tool_name(event) else {
            continue;
        };
        let entry = counts.entry(tool_name).or_insert(0);
        *entry = entry.saturating_add(1);
    }
    counts
}

pub fn build_snapshot(tool_counts: &BTreeMap<String, u64>) -> DiversitySnapshot {
    let total_calls: u64 = tool_counts.values().copied().sum();
    let mut distribution = BTreeMap::new();
    if total_calls > 0 {
        for (tool, count) in tool_counts {
            distribution.insert(tool.clone(), *count as f64 / total_calls as f64);
        }
    }

    let raw_tsallis = if distribution.is_empty() {
        0.0
    } else {
        let values = distribution.values().copied().collect::<Vec<_>>();
        tsallis2(&values)
    };

    let max_tsallis = if distribution.len() > 1 {
        1.0 - 1.0 / distribution.len() as f64
    } else {
        0.0
    };

    let score = agent_diversity_score(&tool_counts.values().copied().collect::<Vec<_>>());

    DiversitySnapshot {
        score,
        raw_tsallis,
        max_tsallis,
        total_calls,
        tools: tool_counts.clone(),
        distribution,
    }
}

fn extract_tool_name(event: &TraceEvent) -> Option<String> {
    if let Some(name) = event.tool_name.as_deref() {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    for key in ["tool_name", "tool", "name"] {
        if let Some(name) = event.content.get(key).and_then(|v| v.as_str()) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tsallis2_uniform() {
        assert!((tsallis2(&[0.25, 0.25, 0.25, 0.25]) - 0.75).abs() < 1e-10);
    }

    #[test]
    fn tsallis2_concentrated() {
        assert!((tsallis2(&[1.0, 0.0, 0.0, 0.0]) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn enriched_similarity_self() {
        let p = vec![0.5, 0.3, 0.2];
        let expected = 0.25 + 0.09 + 0.04;
        assert!((enriched_similarity(&p, &p) - expected).abs() < 1e-10);
    }

    #[test]
    #[should_panic(expected = "equal support size")]
    fn enriched_similarity_rejects_mismatched_lengths() {
        let _ = enriched_similarity(&[0.4, 0.6], &[1.0]);
    }

    #[test]
    fn diversity_score_range() {
        let score = agent_diversity_score(&[100, 1, 1, 1]);
        assert!((0.0..=100.0).contains(&score));
    }

    #[test]
    fn extract_counts_uses_tool_name_and_content_fallback() {
        let events = vec![
            TraceEvent {
                seq: 1,
                timestamp: 1,
                event_type: EventType::ToolCall,
                content: json!({}),
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: Some("rg".to_string()),
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            },
            TraceEvent {
                seq: 2,
                timestamp: 2,
                event_type: EventType::McpToolCall,
                content: json!({"tool":"nucleusdb_query"}),
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            },
        ];
        let counts = extract_tool_counts(&events);
        assert_eq!(counts.get("rg"), Some(&1));
        assert_eq!(counts.get("nucleusdb_query"), Some(&1));
    }
}
