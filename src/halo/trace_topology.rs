use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub timestamp_ms: u64,
    pub tool_name: String,
    pub duration_ms: u64,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetric {
    pub tool_names: Vec<String>,
    pub distances: Vec<Vec<u32>>,
}

impl ToolMetric {
    pub fn from_uniform(tool_names: Vec<String>, off_diag_distance: u32) -> Self {
        let n = tool_names.len();
        let mut distances = vec![vec![0; n]; n];
        for (i, row) in distances.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                if i != j {
                    *cell = off_diag_distance;
                }
            }
        }
        Self {
            tool_names,
            distances,
        }
    }

    pub fn index_of(&self, tool_name: &str) -> Option<usize> {
        self.tool_names.iter().position(|name| name == tool_name)
    }

    pub fn distance(&self, a: usize, b: usize) -> u32 {
        self.distances
            .get(a)
            .and_then(|row| row.get(b).copied())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceChain {
    pub events: Vec<usize>,
    pub length: u32,
}

/// Keep chains whose filtration length is at most `threshold`.
/// Exposed for future blurred magnitude-homology integrations.
pub fn blurred_chains(chains: &[TraceChain], threshold: u32) -> Vec<&TraceChain> {
    chains
        .iter()
        .filter(|chain| chain.length <= threshold)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceEntry {
    pub birth: u32,
    pub death: u32,
    pub representative: Vec<String>,
}

pub fn trace_persistence(
    events: &[TraceEvent],
    metric: &ToolMetric,
    max_chain_degree: usize,
) -> Vec<PersistenceEntry> {
    if events.is_empty() || metric.tool_names.is_empty() {
        return vec![];
    }

    let index_sequence = events
        .iter()
        .filter_map(|event| metric.index_of(&event.tool_name))
        .collect::<Vec<_>>();
    if index_sequence.is_empty() {
        return vec![];
    }

    let degree = max_chain_degree.max(1);
    let chains = build_trace_chains(events, metric, degree);
    if chains.is_empty() {
        return vec![];
    }

    // H0 persistence over chain-derived edges. Degree-1 recovers consecutive
    // transitions; higher degrees add skip edges between chain endpoints.
    let mut edge_weights = BTreeMap::<(usize, usize), u32>::new();
    for chain in chains {
        if chain.events.len() < 2 {
            continue;
        }
        let a = chain.events[0];
        let b = *chain.events.last().unwrap_or(&a);
        if a == b {
            continue;
        }
        let edge = if a < b { (a, b) } else { (b, a) };
        let d = metric.distance(a, b);
        edge_weights
            .entry(edge)
            .and_modify(|existing| *existing = (*existing).min(d))
            .or_insert(d);
    }

    let mut used_nodes = BTreeSet::<usize>::new();
    for idx in &index_sequence {
        used_nodes.insert(*idx);
    }
    let nodes = used_nodes.into_iter().collect::<Vec<_>>();
    if nodes.is_empty() {
        return vec![];
    }

    // Union-find H0 persistence.
    let max_idx = nodes.iter().copied().max().unwrap_or(0);
    let mut uf = UnionFind::new(max_idx + 1);
    let mut entries = Vec::<PersistenceEntry>::new();
    let mut entry_by_root = BTreeMap::<usize, usize>::new();

    for node in &nodes {
        let idx = entries.len();
        entries.push(PersistenceEntry {
            birth: 0,
            death: u32::MAX,
            representative: vec![metric.tool_names[*node].clone()],
        });
        entry_by_root.insert(*node, idx);
    }

    let mut sorted_edges = edge_weights.into_iter().collect::<Vec<_>>();
    sorted_edges.sort_by_key(|(_, distance)| *distance);

    for ((a, b), distance) in sorted_edges {
        let ra = uf.find(a);
        let rb = uf.find(b);
        if ra == rb {
            continue;
        }

        let keep = ra.min(rb);
        let kill = ra.max(rb);
        uf.union(keep, kill);

        if let Some(entry_idx) = entry_by_root.remove(&kill) {
            if let Some(entry) = entries.get_mut(entry_idx) {
                entry.death = distance;
            }
        }

        if let Some(&keep_idx) = entry_by_root.get(&keep) {
            let merged = merged_component_representative(&mut uf, keep, metric, &nodes);
            if let Some(entry) = entries.get_mut(keep_idx) {
                entry.representative = merged;
            }
        }
    }

    entries.sort_by(|a, b| {
        let a_life = a.death.saturating_sub(a.birth);
        let b_life = b.death.saturating_sub(b.birth);
        b_life.cmp(&a_life)
    });

    entries
}

/// Build chain windows up to `max_chain_degree`.
/// Public surface is intentionally preserved for topology experimentation.
pub fn build_trace_chains(
    events: &[TraceEvent],
    metric: &ToolMetric,
    max_chain_degree: usize,
) -> Vec<TraceChain> {
    let indices = events
        .iter()
        .filter_map(|event| metric.index_of(&event.tool_name))
        .collect::<Vec<_>>();
    build_trace_chains_from_indices(&indices, metric, max_chain_degree)
}

pub fn map_halo_trace_events(events: &[crate::halo::schema::TraceEvent]) -> Vec<TraceEvent> {
    let mut out = Vec::new();
    for event in events {
        if !matches!(
            event.event_type,
            crate::halo::schema::EventType::ToolCall | crate::halo::schema::EventType::McpToolCall
        ) {
            continue;
        }
        let tool = event
            .tool_name
            .as_deref()
            .or_else(|| event.content.get("tool_name").and_then(|v| v.as_str()))
            .or_else(|| event.content.get("tool").and_then(|v| v.as_str()))
            .unwrap_or("")
            .trim()
            .to_string();
        if tool.is_empty() {
            continue;
        }
        out.push(TraceEvent {
            timestamp_ms: normalize_timestamp_ms(event.timestamp),
            tool_name: tool,
            duration_ms: extract_duration_ms(&event.content),
            success: infer_success(event),
        });
    }
    out
}

fn build_trace_chains_from_indices(
    indices: &[usize],
    metric: &ToolMetric,
    max_chain_degree: usize,
) -> Vec<TraceChain> {
    if indices.len() < 2 || max_chain_degree == 0 {
        return vec![];
    }

    let mut chains = Vec::new();
    for degree in 1..=max_chain_degree {
        let window = degree + 1;
        if window > indices.len() {
            break;
        }
        for segment in indices.windows(window) {
            let mut length = 0u32;
            for pair in segment.windows(2) {
                length = length.saturating_add(metric.distance(pair[0], pair[1]));
            }
            chains.push(TraceChain {
                events: segment.to_vec(),
                length,
            });
        }
    }
    chains
}

fn merged_component_representative(
    uf: &mut UnionFind,
    root: usize,
    metric: &ToolMetric,
    nodes: &[usize],
) -> Vec<String> {
    let mut names = nodes
        .iter()
        .copied()
        .filter(|node| uf.find(*node) == root)
        .filter_map(|idx| metric.tool_names.get(idx).cloned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[derive(Debug, Clone)]
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            let root = self.find(self.parent[x]);
            self.parent[x] = root;
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

fn normalize_timestamp_ms(ts: u64) -> u64 {
    // Heuristic: Unix ms are already >= 1e12 for modern epochs.
    if ts >= 1_000_000_000_000 {
        ts
    } else {
        ts.saturating_mul(1000)
    }
}

fn extract_duration_ms(content: &Value) -> u64 {
    for key in ["duration_ms", "latency_ms", "elapsed_ms"] {
        if let Some(v) = field_in_content(content, key).and_then(value_to_u64) {
            return v;
        }
    }
    if let Some(secs) = field_in_content(content, "duration_secs").and_then(value_to_u64) {
        return secs.saturating_mul(1000);
    }
    0
}

fn infer_success(event: &crate::halo::schema::TraceEvent) -> bool {
    if matches!(event.event_type, crate::halo::schema::EventType::Error) {
        return false;
    }
    if let Some(v) = field_in_content(&event.content, "success").and_then(value_to_bool) {
        return v;
    }
    if let Some(v) = field_in_content(&event.content, "ok").and_then(value_to_bool) {
        return v;
    }
    if let Some(status) = field_in_content(&event.content, "status").and_then(|v| v.as_str()) {
        let s = status.trim().to_ascii_lowercase();
        if matches!(
            s.as_str(),
            "ok" | "success" | "succeeded" | "complete" | "completed" | "done" | "pass" | "passed"
        ) {
            return true;
        }
        if matches!(
            s.as_str(),
            "error"
                | "failed"
                | "failure"
                | "timeout"
                | "timed_out"
                | "denied"
                | "forbidden"
                | "rejected"
                | "cancelled"
                | "canceled"
        ) {
            return false;
        }
    }
    if field_in_content(&event.content, "error").is_some_and(|v| !v.is_null()) {
        return false;
    }
    true
}

fn field_in_content<'a>(content: &'a Value, key: &str) -> Option<&'a Value> {
    content
        .get(key)
        .or_else(|| content.get("result").and_then(|v| v.get(key)))
        .or_else(|| content.get("output").and_then(|v| v.get(key)))
        .or_else(|| content.get("tool_output").and_then(|v| v.get(key)))
}

fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| {
            value.as_f64().and_then(|n| {
                if n.is_finite() && n >= 0.0 {
                    Some(n as u64)
                } else {
                    None
                }
            })
        })
}

fn value_to_bool(value: &Value) -> Option<bool> {
    value
        .as_bool()
        .or_else(|| value.as_u64().map(|n| n != 0))
        .or_else(|| value.as_i64().map(|n| n != 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::schema::{EventType, TraceEvent as HaloTraceEvent};
    use serde_json::json;

    #[test]
    fn blurred_inclusion_monotone() {
        let chains = vec![
            TraceChain {
                events: vec![0, 1],
                length: 3,
            },
            TraceChain {
                events: vec![0, 2],
                length: 5,
            },
            TraceChain {
                events: vec![1, 2],
                length: 7,
            },
        ];
        let at_4 = blurred_chains(&chains, 4);
        let at_6 = blurred_chains(&chains, 6);
        let at_8 = blurred_chains(&chains, 8);
        assert!(at_4.len() <= at_6.len());
        assert!(at_6.len() <= at_8.len());
    }

    #[test]
    fn trace_persistence_has_live_component() {
        let metric = ToolMetric {
            tool_names: vec![
                "search".to_string(),
                "prove".to_string(),
                "check".to_string(),
            ],
            distances: vec![vec![0, 2, 3], vec![2, 0, 1], vec![3, 1, 0]],
        };
        let events = vec![
            TraceEvent {
                timestamp_ms: 1,
                tool_name: "search".to_string(),
                duration_ms: 10,
                success: true,
            },
            TraceEvent {
                timestamp_ms: 2,
                tool_name: "prove".to_string(),
                duration_ms: 10,
                success: true,
            },
            TraceEvent {
                timestamp_ms: 3,
                tool_name: "check".to_string(),
                duration_ms: 10,
                success: true,
            },
        ];

        let entries = trace_persistence(&events, &metric, 2);
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|entry| entry.death == u32::MAX));
    }

    #[test]
    fn trace_persistence_uses_max_chain_degree_for_skip_edges() {
        let metric = ToolMetric {
            tool_names: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            distances: vec![vec![0, 10, 1], vec![10, 0, 10], vec![1, 10, 0]],
        };
        let events = vec![
            TraceEvent {
                timestamp_ms: 1,
                tool_name: "a".to_string(),
                duration_ms: 1,
                success: true,
            },
            TraceEvent {
                timestamp_ms: 2,
                tool_name: "b".to_string(),
                duration_ms: 1,
                success: true,
            },
            TraceEvent {
                timestamp_ms: 3,
                tool_name: "c".to_string(),
                duration_ms: 1,
                success: true,
            },
        ];

        let d1 = trace_persistence(&events, &metric, 1)
            .into_iter()
            .filter_map(|entry| (entry.death != u32::MAX).then_some(entry.death))
            .min()
            .unwrap_or(u32::MAX);
        let d2 = trace_persistence(&events, &metric, 2)
            .into_iter()
            .filter_map(|entry| (entry.death != u32::MAX).then_some(entry.death))
            .min()
            .unwrap_or(u32::MAX);

        assert_eq!(d1, 10);
        assert_eq!(d2, 1);
    }

    #[test]
    fn map_halo_trace_events_extracts_duration_status_and_timestamp_units() {
        let events = vec![
            HaloTraceEvent {
                seq: 1,
                timestamp: 123,
                event_type: EventType::ToolCall,
                content: json!({"tool":"rg","duration_ms":42,"success":false}),
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            },
            HaloTraceEvent {
                seq: 2,
                timestamp: 1_700_000_000_123,
                event_type: EventType::McpToolCall,
                content: json!({"tool_name":"query","duration_secs":2,"status":"failed"}),
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

        let mapped = map_halo_trace_events(&events);
        assert_eq!(mapped.len(), 2);

        assert_eq!(mapped[0].timestamp_ms, 123_000);
        assert_eq!(mapped[0].duration_ms, 42);
        assert!(!mapped[0].success);

        assert_eq!(mapped[1].timestamp_ms, 1_700_000_000_123);
        assert_eq!(mapped[1].duration_ms, 2_000);
        assert!(!mapped[1].success);
    }

    #[test]
    fn union_find_applies_path_compression() {
        let mut uf = UnionFind {
            parent: vec![0, 0, 1, 2],
        };
        assert_eq!(uf.find(3), 0);
        assert_eq!(uf.parent[3], 0);
        assert_eq!(uf.parent[2], 0);
    }
}
