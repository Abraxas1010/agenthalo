//! DIDComm ↔ MCP/ProofEnvelope bridge.
//!
//! Wraps existing MCP tool calls and ProofEnvelope exchanges in
//! DIDComm encrypted envelopes.

use crate::comms::didcomm::{
    decrypt_message, encrypt_message, DIDCommEnvelope, DIDCommMessage, MessageType,
};
use crate::halo::did::{DIDDocument, DIDIdentity};
use crate::pod::capability::CapabilityToken;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrchestratorTaskEnvelope {
    pub task_id: String,
    pub source_agent_id: String,
    pub target_agent_id: String,
    pub prompt: String,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrchestratorResultEnvelope {
    pub task_id: String,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
}

/// Generate a deterministic-ish unique message ID from timestamp + pid.
pub fn generate_message_id() -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let now = crate::pod::now_unix_nanos();
    let pid = std::process::id();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut random = [0u8; 16];
    let _ = getrandom::getrandom(&mut random);
    let hash = Sha256::digest(format!("{now}-{pid}-{counter}-{random:?}").as_bytes());
    format!("urn:uuid:{}", hex::encode(&hash[..16]))
}

/// Wrap an MCP tool call request in a DIDComm encrypted envelope.
pub fn wrap_mcp_call(
    sender: &DIDIdentity,
    recipient_doc: &DIDDocument,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<DIDCommEnvelope, String> {
    let msg = DIDCommMessage {
        id: generate_message_id(),
        type_: MessageType::McpToolCall,
        from: sender.did.clone(),
        to: vec![recipient_doc.id.clone()],
        created_time: crate::pod::now_unix(),
        expires_time: Some(crate::pod::now_unix() + 300), // 5 min TTL
        body: serde_json::json!({
            "tool_name": tool_name,
            "arguments": arguments,
        }),
        thid: None,
        pthid: None,
    };
    encrypt_message(sender, recipient_doc, &msg)
}

/// Wrap an MCP tool call response in a DIDComm encrypted envelope.
pub fn wrap_mcp_response(
    sender: &DIDIdentity,
    recipient_doc: &DIDDocument,
    tool_name: &str,
    result: serde_json::Value,
    thread_id: Option<&str>,
) -> Result<DIDCommEnvelope, String> {
    let msg = DIDCommMessage {
        id: generate_message_id(),
        type_: MessageType::McpToolResponse,
        from: sender.did.clone(),
        to: vec![recipient_doc.id.clone()],
        created_time: crate::pod::now_unix(),
        expires_time: Some(crate::pod::now_unix() + 300),
        body: serde_json::json!({
            "tool_name": tool_name,
            "result": result,
        }),
        thid: thread_id.map(|s| s.to_string()),
        pthid: None,
    };
    encrypt_message(sender, recipient_doc, &msg)
}

/// Unwrap an MCP tool call request from a DIDComm envelope.
pub fn unwrap_mcp_call(
    recipient: &DIDIdentity,
    sender_doc: &DIDDocument,
    envelope: &DIDCommEnvelope,
) -> Result<(String, String, serde_json::Value), String> {
    let msg = decrypt_message(recipient, sender_doc, envelope)?;
    if msg.is_expired() {
        return Err("DIDComm message expired".to_string());
    }
    match msg.type_ {
        MessageType::McpToolCall => {
            let tool_name = msg
                .body
                .get("tool_name")
                .and_then(|v: &serde_json::Value| v.as_str())
                .ok_or("missing tool_name in MCP call")?
                .to_string();
            let arguments = msg
                .body
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            Ok((msg.id, tool_name, arguments))
        }
        other => Err(format!("expected McpToolCall, got {other:?}")),
    }
}

/// Unwrap an MCP tool call response from a DIDComm envelope.
pub fn unwrap_mcp_response(
    recipient: &DIDIdentity,
    sender_doc: &DIDDocument,
    envelope: &DIDCommEnvelope,
) -> Result<(String, serde_json::Value), String> {
    let msg = decrypt_message(recipient, sender_doc, envelope)?;
    if msg.is_expired() {
        return Err("DIDComm message expired".to_string());
    }
    match msg.type_ {
        MessageType::McpToolResponse => {
            let tool_name = msg
                .body
                .get("tool_name")
                .and_then(|v: &serde_json::Value| v.as_str())
                .ok_or("missing tool_name in response")?
                .to_string();
            let result = msg
                .body
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            Ok((tool_name, result))
        }
        other => Err(format!("expected McpToolResponse, got {other:?}")),
    }
}

/// Wrap an orchestrator task dispatch as DIDComm McpToolCall payload.
pub fn wrap_orchestrator_task(
    sender: &DIDIdentity,
    recipient_doc: &DIDDocument,
    task: &OrchestratorTaskEnvelope,
    capabilities: &[CapabilityToken],
) -> Result<DIDCommEnvelope, String> {
    let caps: Vec<serde_json::Value> = capabilities
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("serialize capability token(s): {e}"))?;
    wrap_mcp_call(
        sender,
        recipient_doc,
        "orchestrator_send_task",
        serde_json::json!({
            "agent_id": task.target_agent_id,
            "task": task.prompt,
            "wait": true,
            "timeout_secs": task.timeout_secs,
            "orchestrator_task_id": task.task_id,
            "capability_tokens": caps,
        }),
    )
}

/// Decode a normalized orchestrator result from MCP/DIDComm tool result content.
pub fn unwrap_orchestrator_result(
    result: serde_json::Value,
) -> Result<OrchestratorResultEnvelope, String> {
    // Supports direct orchestrator response as well as MCP structured content wrappers.
    if let Ok(parsed) = serde_json::from_value::<OrchestratorResultEnvelope>(result.clone()) {
        return Ok(parsed);
    }
    if let Some(sc) = result.get("structuredContent") {
        if let Ok(parsed) = serde_json::from_value::<OrchestratorResultEnvelope>(sc.clone()) {
            return Ok(parsed);
        }
    }
    let task_id = result
        .get("task_id")
        .or_else(|| result.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let status = result
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("failed")
        .to_string();
    let result_text = result
        .get("result")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let error = result
        .get("error")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let exit_code = result
        .get("exit_code")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    Ok(OrchestratorResultEnvelope {
        task_id,
        status,
        result: result_text,
        error,
        exit_code,
    })
}

/// Wrap a ProofEnvelope in a DIDComm message.
pub fn wrap_proof_envelope(
    sender: &DIDIdentity,
    recipient_doc: &DIDDocument,
    proof_envelope: &serde_json::Value,
) -> Result<DIDCommEnvelope, String> {
    let msg = DIDCommMessage {
        id: generate_message_id(),
        type_: MessageType::EnvelopeExchange,
        from: sender.did.clone(),
        to: vec![recipient_doc.id.clone()],
        created_time: crate::pod::now_unix(),
        expires_time: Some(crate::pod::now_unix() + 600), // 10 min TTL
        body: serde_json::json!({
            "envelope": proof_envelope,
        }),
        thid: None,
        pthid: None,
    };
    encrypt_message(sender, recipient_doc, &msg)
}

/// Unwrap a ProofEnvelope from a DIDComm message.
pub fn unwrap_proof_envelope(
    recipient: &DIDIdentity,
    sender_doc: &DIDDocument,
    envelope: &DIDCommEnvelope,
) -> Result<serde_json::Value, String> {
    let msg = decrypt_message(recipient, sender_doc, envelope)?;
    if msg.is_expired() {
        return Err("DIDComm message expired".to_string());
    }
    match msg.type_ {
        MessageType::EnvelopeExchange => msg
            .body
            .get("envelope")
            .cloned()
            .ok_or_else(|| "missing envelope in body".to_string()),
        other => Err(format!("expected EnvelopeExchange, got {other:?}")),
    }
}

/// Wrap a CapabilityToken grant in a DIDComm message.
pub fn wrap_capability_grant(
    sender: &DIDIdentity,
    recipient_doc: &DIDDocument,
    token: &CapabilityToken,
) -> Result<DIDCommEnvelope, String> {
    let token_json =
        serde_json::to_value(token).map_err(|e| format!("serialize capability token: {e}"))?;
    let msg = DIDCommMessage {
        id: generate_message_id(),
        type_: MessageType::CapabilityGrant,
        from: sender.did.clone(),
        to: vec![recipient_doc.id.clone()],
        created_time: crate::pod::now_unix(),
        expires_time: Some(crate::pod::now_unix() + 600),
        body: serde_json::json!({
            "capability_token": token_json,
        }),
        thid: None,
        pthid: None,
    };
    encrypt_message(sender, recipient_doc, &msg)
}

/// Unwrap a CapabilityToken from a DIDComm grant envelope.
pub fn unwrap_capability_grant(
    recipient: &DIDIdentity,
    sender_doc: &DIDDocument,
    envelope: &DIDCommEnvelope,
) -> Result<CapabilityToken, String> {
    let msg = decrypt_message(recipient, sender_doc, envelope)?;
    if msg.is_expired() {
        return Err("DIDComm message expired".to_string());
    }
    match msg.type_ {
        MessageType::CapabilityGrant => {
            let token_val = msg
                .body
                .get("capability_token")
                .ok_or("missing capability_token in grant")?;
            serde_json::from_value(token_val.clone())
                .map_err(|e| format!("parse capability token: {e}"))
        }
        other => Err(format!("expected CapabilityGrant, got {other:?}")),
    }
}

/// Wrap a heartbeat message in a DIDComm envelope.
pub fn wrap_heartbeat(
    sender: &DIDIdentity,
    recipient_doc: &DIDDocument,
) -> Result<DIDCommEnvelope, String> {
    let msg = DIDCommMessage {
        id: generate_message_id(),
        type_: MessageType::Heartbeat,
        from: sender.did.clone(),
        to: vec![recipient_doc.id.clone()],
        created_time: crate::pod::now_unix(),
        expires_time: Some(crate::pod::now_unix() + 60),
        body: serde_json::json!({"status": "alive"}),
        thid: None,
        pthid: None,
    };
    encrypt_message(sender, recipient_doc, &msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity(byte: u8) -> DIDIdentity {
        let seed = [byte; 64];
        crate::halo::did::did_from_genesis_seed(&seed).unwrap()
    }

    #[test]
    fn mcp_call_wrap_unwrap_roundtrip() {
        let alice = test_identity(0xC1);
        let bob = test_identity(0xC2);
        let envelope = wrap_mcp_call(
            &alice,
            &bob.did_document,
            "nucleusdb_query",
            serde_json::json!({"sql": "SELECT 1"}),
        )
        .unwrap();
        let (msg_id, tool_name, arguments) =
            unwrap_mcp_call(&bob, &alice.did_document, &envelope).unwrap();
        assert!(!msg_id.is_empty());
        assert_eq!(tool_name, "nucleusdb_query");
        assert_eq!(arguments["sql"], "SELECT 1");
    }

    #[test]
    fn mcp_response_wrap_unwrap_roundtrip() {
        let alice = test_identity(0xC3);
        let bob = test_identity(0xC4);
        let envelope = wrap_mcp_response(
            &bob,
            &alice.did_document,
            "nucleusdb_query",
            serde_json::json!({"rows": []}),
            Some("thread-42"),
        )
        .unwrap();
        let (tool_name, result) =
            unwrap_mcp_response(&alice, &bob.did_document, &envelope).unwrap();
        assert_eq!(tool_name, "nucleusdb_query");
        assert_eq!(result["rows"], serde_json::json!([]));
    }

    #[test]
    fn proof_envelope_wrap_unwrap_roundtrip() {
        let alice = test_identity(0xC5);
        let bob = test_identity(0xC6);
        let proof_data = serde_json::json!({
            "version": 2,
            "key": "results/theorem_42",
            "proof": "binary_merkle_proof"
        });
        let envelope = wrap_proof_envelope(&alice, &bob.did_document, &proof_data).unwrap();
        let unwrapped = unwrap_proof_envelope(&bob, &alice.did_document, &envelope).unwrap();
        assert_eq!(unwrapped["key"], "results/theorem_42");
    }

    #[test]
    fn heartbeat_roundtrip() {
        let alice = test_identity(0xC7);
        let bob = test_identity(0xC8);
        let envelope = wrap_heartbeat(&alice, &bob.did_document).unwrap();
        let decrypted =
            crate::comms::didcomm::decrypt_message(&bob, &alice.did_document, &envelope).unwrap();
        assert_eq!(decrypted.type_, MessageType::Heartbeat);
        assert_eq!(decrypted.body["status"], "alive");
    }

    #[test]
    fn generate_message_id_is_unique() {
        let id1 = generate_message_id();
        let id2 = generate_message_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("urn:uuid:"));
        assert!(id2.starts_with("urn:uuid:"));
    }

    #[test]
    fn wrong_message_type_rejected() {
        let alice = test_identity(0xC9);
        let bob = test_identity(0xCA);
        // Wrap a heartbeat but try to unwrap as MCP call.
        let envelope = wrap_heartbeat(&alice, &bob.did_document).unwrap();
        let result = unwrap_mcp_call(&bob, &alice.did_document, &envelope);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected McpToolCall"));
    }
}
