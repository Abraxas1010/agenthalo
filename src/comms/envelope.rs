use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrchestratorResultEnvelope {
    pub task_id: String,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
}

pub fn unwrap_orchestrator_result(
    result: serde_json::Value,
) -> Result<OrchestratorResultEnvelope, String> {
    if let Ok(parsed) = serde_json::from_value::<OrchestratorResultEnvelope>(result.clone()) {
        return Ok(parsed);
    }
    if let Some(sc) = result
        .get("structuredContent")
        .or_else(|| result.get("structured_content"))
    {
        if let Ok(parsed) = serde_json::from_value::<OrchestratorResultEnvelope>(sc.clone()) {
            return Ok(parsed);
        }
    }
    Ok(OrchestratorResultEnvelope {
        task_id: result
            .get("task_id")
            .or_else(|| result.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        status: result
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("failed")
            .to_string(),
        result: result
            .get("result")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        error: result
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        exit_code: result
            .get("exit_code")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32),
    })
}
