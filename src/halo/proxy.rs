use crate::halo::vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAI,
    Google,
    Custom { base_url: String },
}

pub fn detect_provider(model: &str) -> Result<Provider, String> {
    if model.starts_with("claude") {
        return Ok(Provider::Anthropic);
    }
    if model.starts_with("gpt")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        return Ok(Provider::OpenAI);
    }
    if model.starts_with("gemini") {
        return Ok(Provider::Google);
    }
    Err(format!(
        "unknown model prefix in '{model}' — cannot route to provider"
    ))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub stream: Option<bool>,
    #[serde(default)]
    pub top_p: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message {
    pub role: String,
    pub content: serde_json::Value,
}

pub fn proxy_chat_sync(vault: &Vault, request: &ChatCompletionRequest) -> Result<Value, String> {
    let provider = detect_provider(&request.model)?;
    let vault_provider = match &provider {
        Provider::Anthropic => "anthropic",
        Provider::OpenAI => "openai",
        Provider::Google => "google",
        Provider::Custom { .. } => return Err("custom providers not yet supported".to_string()),
    };
    let api_key = vault
        .get_key(vault_provider)
        .map_err(|_| format!("no API key configured for {vault_provider}"))?;

    let resp_body = match provider {
        Provider::Anthropic => {
            let body = transform_to_anthropic(request)?;
            let resp = ureq::post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .content_type("application/json")
                .send(&body)
                .map_err(|e| sanitize_upstream_error("anthropic", &e))?;
            let json_val: Value = resp
                .into_body()
                .read_json()
                .map_err(|e| format!("parse response: {e}"))?;
            transform_anthropic_response(&json_val)?
        }
        Provider::OpenAI => {
            let resp = ureq::post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", &format!("Bearer {api_key}"))
                .content_type("application/json")
                .send_json(
                    serde_json::to_value(request).map_err(|e| format!("serialize request: {e}"))?,
                )
                .map_err(|e| sanitize_upstream_error("openai", &e))?;
            resp.into_body()
                .read_json()
                .map_err(|e| format!("parse response: {e}"))?
        }
        Provider::Google => {
            let body = transform_to_google(request)?;
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                request.model, api_key
            );
            let resp = ureq::post(&url)
                .content_type("application/json")
                .send(&body)
                .map_err(|e| sanitize_upstream_error("google", &e))?;
            let json_val: Value = resp
                .into_body()
                .read_json()
                .map_err(|e| format!("parse response: {e}"))?;
            transform_google_response(&json_val)?
        }
        Provider::Custom { .. } => unreachable!(),
    };

    Ok(resp_body)
}

pub fn list_available_models(vault: &Vault) -> Vec<Value> {
    let mut models = Vec::new();
    if vault.get_key("anthropic").is_ok() {
        for m in [
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-haiku-4-5-20251001",
        ] {
            models.push(json!({"id": m, "object": "model", "owned_by": "anthropic"}));
        }
    }
    if vault.get_key("openai").is_ok() {
        for m in ["gpt-4o", "gpt-4o-mini", "o3", "o4-mini"] {
            models.push(json!({"id": m, "object": "model", "owned_by": "openai"}));
        }
    }
    if vault.get_key("google").is_ok() {
        for m in ["gemini-2.5-pro", "gemini-2.5-flash"] {
            models.push(json!({"id": m, "object": "model", "owned_by": "google"}));
        }
    }
    models
}

pub fn transform_to_anthropic(request: &ChatCompletionRequest) -> Result<Vec<u8>, String> {
    let mut system_parts = Vec::new();
    let mut messages = Vec::new();

    for msg in &request.messages {
        if msg.role == "system" {
            if let Some(text) = content_to_text(&msg.content) {
                system_parts.push(text);
            }
            continue;
        }
        let role = if msg.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        messages.push(json!({
            "role": role,
            "content": content_to_text(&msg.content).unwrap_or_default(),
        }));
    }

    let payload = json!({
        "model": request.model,
        "max_tokens": request.max_tokens.unwrap_or(1024),
        "temperature": request.temperature,
        "system": if system_parts.is_empty() { Value::Null } else { Value::String(system_parts.join("\n")) },
        "messages": messages,
    });

    serde_json::to_vec(&payload).map_err(|e| format!("serialize anthropic body: {e}"))
}

pub fn transform_to_google(request: &ChatCompletionRequest) -> Result<Vec<u8>, String> {
    let mut system_parts = Vec::new();
    let mut contents = Vec::new();

    for msg in &request.messages {
        if msg.role == "system" {
            if let Some(text) = content_to_text(&msg.content) {
                system_parts.push(text);
            }
            continue;
        }
        let role = if msg.role == "assistant" {
            "model"
        } else {
            "user"
        };
        contents.push(json!({
            "role": role,
            "parts": [{"text": content_to_text(&msg.content).unwrap_or_default()}],
        }));
    }

    let mut payload = json!({
        "contents": contents,
        "generationConfig": {
            "temperature": request.temperature,
            "maxOutputTokens": request.max_tokens,
            "topP": request.top_p,
        }
    });

    if !system_parts.is_empty() {
        payload["systemInstruction"] = json!({
            "parts": [{"text": system_parts.join("\n") }]
        });
    }

    serde_json::to_vec(&payload).map_err(|e| format!("serialize google body: {e}"))
}

pub fn transform_anthropic_response(resp: &Value) -> Result<Value, String> {
    let model = resp
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("claude");
    let content = resp
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(json!({
        "id": resp.get("id").and_then(|v| v.as_str()).unwrap_or("chatcmpl-anthropic"),
        "object": "chat.completion",
        "created": now_unix(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": resp.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("stop"),
        }],
    }))
}

pub fn transform_google_response(resp: &Value) -> Result<Value, String> {
    let content = resp
        .get("candidates")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("content"))
        .and_then(|v| v.get("parts"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let finish_reason = resp
        .get("candidates")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("finishReason"))
        .and_then(|v| v.as_str())
        .unwrap_or("STOP");

    Ok(json!({
        "id": format!("chatcmpl-google-{}", now_unix()),
        "object": "chat.completion",
        "created": now_unix(),
        "model": "gemini",
        "choices": [{
            "index": 0,
            "message": {"role":"assistant", "content": content},
            "finish_reason": finish_reason.to_ascii_lowercase(),
        }],
    }))
}

fn content_to_text(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let mut chunks = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    chunks.push(text.to_string());
                } else if let Some(text) = item.as_str() {
                    chunks.push(text.to_string());
                }
            }
            if chunks.is_empty() {
                None
            } else {
                Some(chunks.join("\n"))
            }
        }
        _ => Some(content.to_string()),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sanitize_upstream_error(provider: &str, err: &ureq::Error) -> String {
    let msg = err.to_string();
    if msg.contains("key=") {
        format!("{provider} upstream error (credentials redacted)")
    } else {
        format!("{provider} upstream error: {msg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request(model: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model.to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: Value::String("you are helpful".to_string()),
                },
                Message {
                    role: "user".to_string(),
                    content: Value::String("hello".to_string()),
                },
            ],
            temperature: Some(0.2),
            max_tokens: Some(128),
            stream: Some(false),
            top_p: None,
        }
    }

    #[test]
    fn detect_provider_maps_prefixes() {
        assert_eq!(
            detect_provider("claude-opus-4-6").unwrap(),
            Provider::Anthropic
        );
        assert_eq!(detect_provider("gpt-4o").unwrap(), Provider::OpenAI);
        assert_eq!(detect_provider("gemini-2.5-pro").unwrap(), Provider::Google);
        assert!(detect_provider("unknown-model").is_err());
    }

    #[test]
    fn anthropic_transform_extracts_system() {
        let req = sample_request("claude-opus-4-6");
        let payload = transform_to_anthropic(&req).expect("transform");
        let value: Value = serde_json::from_slice(&payload).expect("json");
        assert_eq!(value["system"], "you are helpful");
        assert_eq!(value["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn google_transform_maps_contents() {
        let req = sample_request("gemini-2.5-pro");
        let payload = transform_to_google(&req).expect("transform");
        let value: Value = serde_json::from_slice(&payload).expect("json");
        assert!(value.get("contents").is_some());
        assert!(value.get("systemInstruction").is_some());
    }
}
