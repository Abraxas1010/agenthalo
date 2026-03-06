use crate::orchestrator::task::TaskStatus;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipeTransform {
    Identity,
    #[serde(alias = "assistant_answer")]
    ClaudeAnswer,
    JsonExtract(String),
    Prefix(String),
    Suffix(String),
    Chain(Vec<PipeTransform>),
}

impl PipeTransform {
    pub fn parse(raw: Option<&str>, task_prefix: Option<&str>) -> Result<Self, String> {
        let mut transforms = Vec::new();
        let base = raw.unwrap_or("identity").trim();
        if !base.is_empty() && !base.eq_ignore_ascii_case("identity") {
            if base.eq_ignore_ascii_case("claude_answer")
                || base.eq_ignore_ascii_case("assistant_answer")
            {
                transforms.push(Self::ClaudeAnswer);
            } else if let Some(path) = base.strip_prefix("json_extract:") {
                transforms.push(Self::JsonExtract(path.trim().to_string()));
            } else if let Some(prefix) = base.strip_prefix("prefix:") {
                transforms.push(Self::Prefix(prefix.to_string()));
            } else if let Some(suffix) = base.strip_prefix("suffix:") {
                transforms.push(Self::Suffix(suffix.to_string()));
            } else {
                return Err(format!("unknown pipe transform '{base}'"));
            }
        } else {
            transforms.push(Self::Identity);
        }
        if let Some(prefix) = task_prefix.filter(|s| !s.is_empty()) {
            transforms.insert(0, Self::Prefix(prefix.to_string()));
        }
        if transforms.len() == 1 {
            Ok(transforms.remove(0))
        } else {
            Ok(Self::Chain(transforms))
        }
    }

    pub fn apply(&self, input: &str) -> String {
        self.apply_with_answer(input, None)
    }

    pub fn apply_with_answer(&self, input: &str, answer: Option<&str>) -> String {
        match self {
            Self::Identity => input.to_string(),
            Self::ClaudeAnswer => answer
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(input)
                .to_string(),
            Self::JsonExtract(path) => {
                json_extract(input, path).unwrap_or_else(|| input.to_string())
            }
            Self::Prefix(prefix) => format!("{prefix}{input}"),
            Self::Suffix(suffix) => format!("{input}{suffix}"),
            Self::Chain(chain) => chain.iter().fold(input.to_string(), |acc, transform| {
                transform.apply_with_answer(&acc, answer)
            }),
        }
    }
}

fn json_extract(input: &str, path: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    let mut cur = &value;
    for part in path.trim().trim_start_matches('.').split('.') {
        if part.is_empty() {
            continue;
        }
        if let Some((name, idx_raw)) = part.split_once('[') {
            cur = cur.get(name)?;
            let idx = idx_raw.strip_suffix(']')?.parse::<usize>().ok()?;
            cur = cur.get(idx)?;
        } else {
            cur = cur.get(part)?;
        }
    }
    Some(match cur {
        serde_json::Value::String(s) => s.clone(),
        _ => cur.to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub task_id: String,
    pub agent_id: String,
    pub status: TaskStatus,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEdge {
    pub source_task_id: String,
    pub target_agent_id: String,
    pub transform: PipeTransform,
    pub generated_task_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskGraph {
    pub nodes: BTreeMap<String, TaskNode>,
    pub edges: Vec<TaskEdge>,
}

impl TaskGraph {
    pub fn upsert_node(&mut self, task_id: &str, agent_id: &str, status: TaskStatus) {
        let entry = self.nodes.entry(task_id.to_string()).or_insert(TaskNode {
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            status: status.clone(),
            depends_on: Vec::new(),
        });
        entry.status = status;
    }

    pub fn set_generated_task(
        &mut self,
        source_task_id: &str,
        target_agent_id: &str,
        task_id: String,
    ) {
        for edge in &mut self.edges {
            if edge.source_task_id == source_task_id && edge.target_agent_id == target_agent_id {
                edge.generated_task_id = Some(task_id.clone());
            }
        }
    }

    pub fn outgoing_for(&self, source_task_id: &str) -> Vec<TaskEdge> {
        self.edges
            .iter()
            .filter(|e| e.source_task_id == source_task_id)
            .cloned()
            .collect()
    }

    pub fn add_edge(&mut self, edge: TaskEdge) -> Result<(), String> {
        if edge.source_task_id.trim().is_empty() {
            return Err("source_task_id must not be empty".to_string());
        }
        if edge.target_agent_id.trim().is_empty() {
            return Err("target_agent_id must not be empty".to_string());
        }
        let source_node = self
            .nodes
            .get(&edge.source_task_id)
            .ok_or_else(|| format!("unknown source_task_id {}", edge.source_task_id))?;
        if source_node.agent_id == edge.target_agent_id {
            return Err("pipe edge would introduce a cycle".to_string());
        }
        if self.agent_would_cycle(&source_node.agent_id, &edge.target_agent_id) {
            return Err("pipe edge would introduce a cycle".to_string());
        }
        self.edges.push(edge);
        Ok(())
    }

    fn agent_would_cycle(&self, source_agent: &str, target_agent: &str) -> bool {
        if source_agent == target_agent {
            return true;
        }
        let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for edge in &self.edges {
            let Some(source_node) = self.nodes.get(&edge.source_task_id) else {
                continue;
            };
            graph
                .entry(source_node.agent_id.clone())
                .or_default()
                .insert(edge.target_agent_id.clone());
        }
        graph
            .entry(source_agent.to_string())
            .or_default()
            .insert(target_agent.to_string());

        // A new source->target edge introduces a cycle if target can already reach source.
        let mut stack = vec![target_agent.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node.clone()) {
                continue;
            }
            if node == source_agent {
                return true;
            }
            if let Some(next) = graph.get(&node) {
                for n in next {
                    stack.push(n.clone());
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_chain_applies_prefix_and_extract() {
        let t = PipeTransform::Chain(vec![
            PipeTransform::JsonExtract(".result".to_string()),
            PipeTransform::Prefix("next: ".to_string()),
        ]);
        let out = t.apply(r#"{"result":"ok"}"#);
        assert_eq!(out, "next: ok");
    }

    #[test]
    fn parse_transform_uses_task_prefix() {
        let t = PipeTransform::parse(Some("identity"), Some("pre: ")).expect("parse transform");
        assert_eq!(t.apply("x"), "pre: x");
    }

    #[test]
    fn graph_adds_edges_and_detects_direct_cycle() {
        let mut graph = TaskGraph::default();
        graph.upsert_node("t1", "a", TaskStatus::Complete);
        graph.upsert_node("t2", "b", TaskStatus::Complete);
        graph
            .add_edge(TaskEdge {
                source_task_id: "t1".to_string(),
                target_agent_id: "b".to_string(),
                transform: PipeTransform::Identity,
                generated_task_id: None,
            })
            .expect("first edge");
        let err = graph
            .add_edge(TaskEdge {
                source_task_id: "t1".to_string(),
                target_agent_id: "a".to_string(),
                transform: PipeTransform::Identity,
                generated_task_id: None,
            })
            .expect_err("must reject cycle");
        assert!(err.contains("cycle"));

        let reverse_err = graph
            .add_edge(TaskEdge {
                source_task_id: "t2".to_string(),
                target_agent_id: "a".to_string(),
                transform: PipeTransform::Identity,
                generated_task_id: None,
            })
            .expect_err("reverse dependency should be rejected as cycle");
        assert!(reverse_err.contains("cycle"));
    }

    #[test]
    fn parse_transform_rejects_unknown() {
        let err = PipeTransform::parse(Some("regex:s/foo/bar/"), None).expect_err("must reject");
        assert!(err.contains("unknown pipe transform"));
    }

    #[test]
    fn claude_answer_transform_prefers_parsed_answer() {
        let t = PipeTransform::parse(Some("claude_answer"), None).expect("parse transform");
        let out = t.apply_with_answer("{\"raw\":\"json\"}", Some("final answer"));
        assert_eq!(out, "final answer");
    }

    #[test]
    fn serde_accepts_assistant_answer_alias() {
        let t: PipeTransform =
            serde_json::from_str("\"assistant_answer\"").expect("deserialize alias");
        assert!(matches!(t, PipeTransform::ClaudeAnswer));
    }
}
