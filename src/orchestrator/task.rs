use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Complete,
    Failed,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub agent_id: String,
    pub prompt: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub answer: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
    pub usage: TaskUsage,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub trace_session_id: Option<String>,
}

impl Task {
    pub fn new(task_id: String, agent_id: String, prompt: String) -> Self {
        Self {
            task_id,
            agent_id,
            prompt,
            status: TaskStatus::Pending,
            answer: None,
            result: None,
            error: None,
            exit_code: None,
            usage: TaskUsage::default(),
            started_at: None,
            completed_at: None,
            trace_session_id: None,
        }
    }

    pub fn mark_running(&mut self) {
        self.status = TaskStatus::Running;
        self.started_at = Some(crate::pod::now_unix());
    }

    pub fn mark_complete(
        &mut self,
        result: String,
        exit_code: i32,
        usage: TaskUsage,
        trace_session_id: Option<String>,
    ) {
        self.status = TaskStatus::Complete;
        self.result = Some(result);
        self.error = None;
        self.exit_code = Some(exit_code);
        self.usage = usage;
        self.trace_session_id = trace_session_id;
        self.completed_at = Some(crate::pod::now_unix());
    }

    pub fn mark_failed(&mut self, error: String, exit_code: Option<i32>) {
        self.status = TaskStatus::Failed;
        self.error = Some(error);
        self.exit_code = exit_code;
        self.completed_at = Some(crate::pod::now_unix());
    }

    pub fn mark_timeout(&mut self, error: String) {
        self.status = TaskStatus::Timeout;
        self.error = Some(error);
        self.completed_at = Some(crate::pod::now_unix());
    }
}
