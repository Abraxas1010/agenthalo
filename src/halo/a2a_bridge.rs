use crate::halo::did::DIDIdentity;
use crate::halo::p2p_discovery::AgentCapability;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aAgentCard {
    pub name: String,
    pub description: String,
    pub url: String,
    pub version: String,
    pub provider: A2aProvider,
    pub capabilities: A2aCapabilities,
    pub skills: Vec<A2aSkill>,
    pub security_schemes: HashMap<String, A2aSecurityScheme>,
    pub security: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aProvider {
    pub organization: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aCapabilities {
    pub streaming: bool,
    pub push_notifications: bool,
    pub state_transition_history: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aSecurityScheme {
    #[serde(rename = "type")]
    pub type_: String,
    pub did: String,
    pub description: String,
}

#[derive(Clone)]
struct BridgeState {
    card: A2aAgentCard,
}

pub fn generate_agent_card(
    identity: &DIDIdentity,
    base_url: &str,
    skills: &[AgentCapability],
) -> A2aAgentCard {
    A2aAgentCard {
        name: std::env::var("AGENT_NAME").unwrap_or_else(|_| "AgentHalo".to_string()),
        description: std::env::var("AGENT_DESCRIPTION")
            .unwrap_or_else(|_| "Sovereign AI agent with privacy-first communication".to_string()),
        url: base_url.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        provider: A2aProvider {
            organization: "Self-Sovereign".to_string(),
            url: identity.did.clone(),
        },
        capabilities: A2aCapabilities {
            streaming: true,
            push_notifications: false,
            state_transition_history: false,
        },
        skills: skills
            .iter()
            .map(|capability| A2aSkill {
                id: capability.id.clone(),
                name: capability.name.clone(),
                description: capability.description.clone(),
                tags: Vec::new(),
                examples: Vec::new(),
            })
            .collect(),
        security_schemes: HashMap::from([(
            "didAuth".to_string(),
            A2aSecurityScheme {
                type_: "didcomm".to_string(),
                did: identity.did.clone(),
                description: "DIDComm v2 authenticated messaging".to_string(),
            },
        )]),
        security: vec!["didAuth".to_string()],
    }
}

pub async fn start_a2a_bridge(
    identity: Arc<DIDIdentity>,
    port: u16,
    skills: Vec<AgentCapability>,
) -> Result<(), String> {
    if port == 0 {
        return Ok(());
    }

    let base_url = format!("http://127.0.0.1:{port}");
    let state = BridgeState {
        card: generate_agent_card(&identity, &base_url, &skills),
    };

    let app = Router::new()
        .route(
            "/.well-known/agent.json",
            get(|State(state): State<BridgeState>| async move { Json(state.card) }),
        )
        .route(
            "/",
            post(|Json(body): Json<serde_json::Value>| async move { Json(handle_jsonrpc(body)) }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("bind A2A bridge on port {port}: {e}"))?;

    eprintln!("[AgentHalo/A2A] bridge active on {base_url}");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve A2A bridge: {e}"))
}

fn handle_jsonrpc(body: serde_json::Value) -> serde_json::Value {
    let id = body.get("id").cloned().unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "status": "not_implemented",
            "detail": "A2A JSON-RPC bridge is scaffolded; method routing will be added in the next phase"
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_card_uses_did_and_skills() {
        let identity = crate::halo::did::did_from_genesis_seed(&[0x55; 64]).expect("identity");
        let card = generate_agent_card(
            &identity,
            "http://127.0.0.1:9300",
            &[AgentCapability {
                id: "coding".to_string(),
                name: "Coding".to_string(),
                description: "Writes code".to_string(),
                input_types: vec!["text/plain".to_string()],
                output_types: vec!["text/plain".to_string()],
            }],
        );
        assert_eq!(card.security, vec!["didAuth".to_string()]);
        assert_eq!(card.skills.len(), 1);
        assert_eq!(card.provider.url, identity.did);
    }
}
