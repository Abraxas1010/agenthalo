use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectReport {
    pub dir: String,
    pub scan_time_ms: u64,
    pub total_files: usize,
    pub total_lines: usize,
    pub total_sorrys: usize,
    pub total_decls: usize,
    pub health_score: f64,
    pub files: Vec<FileReport>,
    pub dependency_graph: DependencyGraph,
    pub clusters: Vec<ClusterInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReport {
    pub path: String,
    pub lines: usize,
    pub decl_count: usize,
    pub sorry_count: usize,
    pub import_count: usize,
    pub health: HealthMetrics,
    pub declarations: Vec<DeclInfo>,
    pub imports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMetrics {
    pub score: f64,
    pub sorry_ratio: f64,
    pub avg_decl_length: f64,
    pub max_decl_length: usize,
    pub has_sorry: bool,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Clean,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclInfo {
    pub name: String,
    pub kind: DeclKind,
    pub line_start: usize,
    pub line_end: usize,
    pub has_sorry: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeclKind {
    Theorem,
    Lemma,
    Def,
    Instance,
    Structure,
    Class,
    Inductive,
    Abbrev,
    Axiom,
    Opaque,
}

impl DeclKind {
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "theorem" => Some(Self::Theorem),
            "lemma" => Some(Self::Lemma),
            "def" => Some(Self::Def),
            "instance" => Some(Self::Instance),
            "structure" => Some(Self::Structure),
            "class" => Some(Self::Class),
            "inductive" => Some(Self::Inductive),
            "abbrev" => Some(Self::Abbrev),
            "axiom" => Some(Self::Axiom),
            "opaque" => Some(Self::Opaque),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub nodes: Vec<String>,
    pub edges: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInfo {
    pub name: String,
    pub files: Vec<String>,
    pub total_lines: usize,
    pub total_sorrys: usize,
    pub health_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedSorry {
    pub file: String,
    pub line: usize,
    pub decl_name: Option<String>,
    pub goal_state: Option<String>,
    pub diagnostic_msg: Option<String>,
    pub dependents_count: usize,
}
