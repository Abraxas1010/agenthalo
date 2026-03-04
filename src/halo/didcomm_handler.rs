use crate::halo::did::{DIDCommCredentialAttachment, DIDIdentity};
use crate::halo::didcomm::{
    message_types, pack_authcrypt_hybrid, unpack_with_resolver, AttachmentData, DIDCommAttachment,
    DIDCommMessage,
};
use crate::halo::zk_credential;
use futures_util::future::BoxFuture;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

pub type MessageHandler =
    Arc<dyn Fn(DIDCommMessage) -> BoxFuture<'static, Option<DIDCommMessage>> + Send + Sync>;

#[derive(Clone)]
pub struct DIDCommHandler {
    identity: Arc<DIDIdentity>,
    handlers: HashMap<String, MessageHandler>,
}

impl DIDCommHandler {
    pub fn new(identity: Arc<DIDIdentity>) -> Self {
        Self {
            identity,
            handlers: HashMap::new(),
        }
    }

    pub fn on_message<F>(&mut self, message_type: &str, handler: F)
    where
        F: Fn(DIDCommMessage) -> BoxFuture<'static, Option<DIDCommMessage>> + Send + Sync + 'static,
    {
        self.handlers
            .insert(message_type.to_string(), Arc::new(handler));
    }

    pub fn register_builtin_handlers(&mut self) {
        self.on_message(message_types::PING, |message| {
            Box::pin(async move {
                Some(DIDCommMessage::new(
                    message_types::ACK,
                    None,
                    message.from.into_iter().collect(),
                    json!({
                        "reply_to": message.id,
                        "status": "ok"
                    }),
                ))
            })
        });

        self.on_message(message_types::AGENT_CARD_REQUEST, |_message| {
            Box::pin(async move {
                Some(DIDCommMessage::new(
                    message_types::AGENT_CARD_RESPONSE,
                    None,
                    Vec::new(),
                    json!({
                        "name": "AgentHalo",
                        "version": env!("CARGO_PKG_VERSION"),
                        "transport": "didcomm-v2"
                    }),
                ))
            })
        });

        let holder_identity = self.identity.clone();
        self.on_message(message_types::CREDENTIAL_OFFER, move |message| {
            let holder_identity = holder_identity.clone();
            Box::pin(async move {
                match handle_credential_offer(&holder_identity, message) {
                    Ok(response) => Some(response),
                    Err(error) => Some(error_response(error)),
                }
            })
        });

        self.on_message(message_types::CREDENTIAL_REQUEST, |message| {
            Box::pin(async move {
                match handle_credential_request(message) {
                    Ok(response) => Some(response),
                    Err(error) => Some(error_response(error)),
                }
            })
        });

        self.on_message(message_types::CREDENTIAL_ISSUE, |message| {
            Box::pin(async move {
                Some(DIDCommMessage::new(
                    message_types::ACK,
                    None,
                    message.from.into_iter().collect(),
                    json!({
                        "reply_to": message.id,
                        "status": "credential_ack"
                    }),
                ))
            })
        });

        self.on_message(message_types::TASK_SEND, |message| {
            Box::pin(async move {
                Some(DIDCommMessage::new(
                    message_types::TASK_STATUS,
                    None,
                    message.from.into_iter().collect(),
                    json!({
                        "reply_to": message.id,
                        "status": "submitted"
                    }),
                ))
            })
        });

        self.on_message(message_types::TASK_CANCEL, |message| {
            Box::pin(async move {
                Some(DIDCommMessage::new(
                    message_types::TASK_STATUS,
                    None,
                    message.from.into_iter().collect(),
                    json!({
                        "reply_to": message.id,
                        "status": "canceled"
                    }),
                ))
            })
        });
    }

    pub async fn handle_incoming<F>(
        &self,
        packed: &[u8],
        resolve_document: F,
    ) -> Result<Option<Vec<u8>>, String>
    where
        F: Fn(&str) -> Option<crate::halo::did::DIDDocument>,
    {
        let (message, sender_did) =
            unpack_with_resolver(packed, &self.identity, &resolve_document)?;
        if message.is_expired() {
            return Err("message expired".to_string());
        }

        let handler = match self.handlers.get(&message.type_) {
            Some(handler) => handler,
            None => return Ok(None),
        };

        let Some(mut response) = handler(message).await else {
            return Ok(None);
        };

        let Some(sender_did) = sender_did else {
            return Ok(None);
        };

        let sender_doc = resolve_document(&sender_did)
            .ok_or_else(|| format!("cannot resolve DID document for sender `{sender_did}`"))?;
        response.from = Some(self.identity.did.clone());
        response.to = vec![sender_did];

        let packed_response = pack_authcrypt_hybrid(&response, &self.identity, &sender_doc, None)?;
        Ok(Some(packed_response))
    }
}

#[derive(Debug, Deserialize)]
struct CredentialOfferBody {
    grant: crate::pod::acl::AccessGrant,
    resource_uri: String,
    requested_action: String,
    #[serde(default)]
    current_time: Option<u64>,
}

static CREDENTIAL_KEYS: OnceLock<zk_credential::CredentialKeypair> = OnceLock::new();
const CREDENTIAL_ATTACHMENT_MEDIA_TYPE: &str = "application/agenthalo.credential-proof+json";
// T24: DIDComm↔ZK credential binding model in
// lean/NucleusDB/Comms/Protocol/ZKBindingSpec.lean

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn credential_keys() -> Result<&'static zk_credential::CredentialKeypair, String> {
    if let Some(keys) = CREDENTIAL_KEYS.get() {
        return Ok(keys);
    }
    let keys = zk_credential::setup_credential_circuit()?;
    let _ = CREDENTIAL_KEYS.set(keys);
    CREDENTIAL_KEYS
        .get()
        .ok_or_else(|| "credential proving keys unavailable".to_string())
}

fn parse_credential_offer_body(message: &DIDCommMessage) -> Result<CredentialOfferBody, String> {
    serde_json::from_value(message.body.clone())
        .map_err(|e| format!("parse credential offer body: {e}"))
}

fn requested_permissions(action: &str) -> Result<crate::pod::acl::GrantPermissions, String> {
    match action.trim().to_ascii_lowercase().as_str() {
        "read" => Ok(crate::pod::acl::GrantPermissions {
            read: true,
            write: false,
            append: false,
            control: false,
        }),
        "write" => Ok(crate::pod::acl::GrantPermissions {
            read: false,
            write: true,
            append: false,
            control: false,
        }),
        "append" => Ok(crate::pod::acl::GrantPermissions {
            read: false,
            write: false,
            append: true,
            control: false,
        }),
        "control" => Ok(crate::pod::acl::GrantPermissions {
            read: false,
            write: false,
            append: false,
            control: true,
        }),
        other => Err(format!(
            "unsupported requested_action `{other}` (expected read|write|append|control)"
        )),
    }
}

fn extract_credential_attachment(
    message: &DIDCommMessage,
) -> Result<DIDCommCredentialAttachment, String> {
    let attachment = message
        .attachments
        .as_ref()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.media_type == CREDENTIAL_ATTACHMENT_MEDIA_TYPE)
        })
        .ok_or_else(|| "credential request missing credential proof attachment".to_string())?;
    let AttachmentData::Json { json } = &attachment.data else {
        return Err("credential proof attachment must use JSON payload".to_string());
    };
    serde_json::from_value(json.clone()).map_err(|e| format!("parse credential attachment: {e}"))
}

fn validate_credential_attachment_for_request(
    attachment: &DIDCommCredentialAttachment,
) -> Result<(), String> {
    if attachment.proof_bundle.schema_version != 2 {
        return Err(format!(
            "unsupported credential proof schema `{}` (expected 2)",
            attachment.proof_bundle.schema_version
        ));
    }
    if attachment.resource_uri.trim().is_empty() {
        return Err("credential attachment missing resource_uri".to_string());
    }
    if attachment.requested_action.trim().is_empty() {
        return Err("credential attachment missing requested_action".to_string());
    }
    Ok(())
}

fn build_credential_attachment(
    attachment: &DIDCommCredentialAttachment,
) -> Result<DIDCommAttachment, String> {
    let payload = serde_json::to_value(attachment)
        .map_err(|e| format!("serialize credential attachment: {e}"))?;
    Ok(DIDCommAttachment {
        id: "credential-proof-1".to_string(),
        media_type: CREDENTIAL_ATTACHMENT_MEDIA_TYPE.to_string(),
        data: AttachmentData::Json { json: payload },
    })
}

fn handle_credential_offer(
    holder_identity: &DIDIdentity,
    message: DIDCommMessage,
) -> Result<DIDCommMessage, String> {
    let offer = parse_credential_offer_body(&message)?;
    let action = offer.requested_action.clone();
    let requested = requested_permissions(&action)?;
    let current_time = offer.current_time.unwrap_or_else(now_unix_secs);
    let keys = credential_keys()?;
    let proof_bundle = zk_credential::prove_credential(
        &keys.0,
        &offer.grant,
        &holder_identity.did,
        requested,
        current_time,
    )?;

    let attachment = DIDCommCredentialAttachment {
        proof_bundle,
        resource_uri: offer.resource_uri.clone(),
        requested_action: action.clone(),
    };

    let mut response = DIDCommMessage::new(
        message_types::CREDENTIAL_REQUEST,
        None,
        message.from.into_iter().collect(),
        json!({
            "reply_to": message.id,
            "resource_uri": offer.resource_uri,
            "requested_action": action,
            "current_time": current_time,
        }),
    );
    response = response.with_attachment(build_credential_attachment(&attachment)?);
    Ok(response)
}

fn handle_credential_request(message: DIDCommMessage) -> Result<DIDCommMessage, String> {
    let attachment = extract_credential_attachment(&message)?;
    validate_credential_attachment_for_request(&attachment)?;
    let keys = credential_keys()?;
    let verified = zk_credential::verify_credential_proof(&keys.1, &attachment.proof_bundle)?;
    if !verified {
        return Err("credential proof verification failed".to_string());
    }

    let resource_uri = attachment.resource_uri.clone();
    let requested_action = attachment.requested_action.clone();
    let mut response = DIDCommMessage::new(
        message_types::CREDENTIAL_ISSUE,
        None,
        message.from.into_iter().collect(),
        json!({
            "reply_to": message.id,
            "status": "issued",
            "verified": true,
            "resource_uri": resource_uri,
            "requested_action": requested_action,
        }),
    );
    response = response.with_attachment(build_credential_attachment(&attachment)?);
    Ok(response)
}

fn error_response(error: String) -> DIDCommMessage {
    DIDCommMessage::new(
        message_types::ERROR,
        None,
        Vec::new(),
        json!({
            "error": error,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::didcomm::{pack_authcrypt, DIDCommMessage};

    fn seed(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    fn resolver<'a>(
        a: &'a Arc<crate::halo::did::DIDIdentity>,
        b: &'a Arc<crate::halo::did::DIDIdentity>,
    ) -> impl Fn(&str) -> Option<crate::halo::did::DIDDocument> + 'a {
        move |did| {
            if did == a.did {
                Some(a.did_document.clone())
            } else if did == b.did {
                Some(b.did_document.clone())
            } else {
                None
            }
        }
    }

    #[tokio::test]
    async fn ping_handler_returns_ack() {
        let sender =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x11)).expect("sender"));
        let recipient =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x12)).expect("recipient"));
        let recipient_key =
            crate::halo::didcomm::extract_x25519_public_key_from_doc(&recipient.did_document)
                .expect("recipient x25519");

        let ping = DIDCommMessage::new(
            message_types::PING,
            Some(&sender.did),
            vec![recipient.did.clone()],
            json!({ "ping": true }),
        );
        let packed_ping = pack_authcrypt(&ping, &sender, &recipient_key).expect("pack ping");

        let mut handler = DIDCommHandler::new(recipient.clone());
        handler.register_builtin_handlers();
        let packed_ack = handler
            .handle_incoming(&packed_ping, resolver(&sender, &recipient))
            .await
            .expect("handle incoming")
            .expect("ack response");

        let (ack_message, _) = crate::halo::didcomm::unpack_with_resolver(
            &packed_ack,
            &sender,
            resolver(&recipient, &sender),
        )
        .expect("unpack ack");
        assert_eq!(ack_message.type_, message_types::ACK);
    }

    #[tokio::test]
    async fn task_send_handler_returns_task_status() {
        let sender =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x21)).expect("sender"));
        let recipient =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x22)).expect("recipient"));
        let recipient_key =
            crate::halo::didcomm::extract_x25519_public_key_from_doc(&recipient.did_document)
                .expect("recipient x25519");

        let task_send = DIDCommMessage::new(
            message_types::TASK_SEND,
            Some(&sender.did),
            vec![recipient.did.clone()],
            json!({ "task": "compile", "payload": {"goal": "proof"} }),
        );
        let packed = pack_authcrypt(&task_send, &sender, &recipient_key).expect("pack task");

        let mut handler = DIDCommHandler::new(recipient.clone());
        handler.register_builtin_handlers();
        let packed_status = handler
            .handle_incoming(&packed, resolver(&sender, &recipient))
            .await
            .expect("handle incoming")
            .expect("task status response");

        let (status_message, _) = crate::halo::didcomm::unpack_with_resolver(
            &packed_status,
            &sender,
            resolver(&recipient, &sender),
        )
        .expect("unpack status");

        assert_eq!(status_message.type_, message_types::TASK_STATUS);
        assert_eq!(status_message.body["status"], "submitted");
    }

    #[tokio::test]
    async fn task_cancel_handler_returns_canceled_status() {
        let sender =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x29)).expect("sender"));
        let recipient =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x2A)).expect("recipient"));
        let recipient_key =
            crate::halo::didcomm::extract_x25519_public_key_from_doc(&recipient.did_document)
                .expect("recipient x25519");

        let task_cancel = DIDCommMessage::new(
            message_types::TASK_CANCEL,
            Some(&sender.did),
            vec![recipient.did.clone()],
            json!({ "task_id": "task-1234" }),
        );
        let packed = pack_authcrypt(&task_cancel, &sender, &recipient_key).expect("pack cancel");

        let mut handler = DIDCommHandler::new(recipient.clone());
        handler.register_builtin_handlers();
        let packed_status = handler
            .handle_incoming(&packed, resolver(&sender, &recipient))
            .await
            .expect("handle incoming")
            .expect("task status response");

        let (status_message, _) = crate::halo::didcomm::unpack_with_resolver(
            &packed_status,
            &sender,
            resolver(&recipient, &sender),
        )
        .expect("unpack status");

        assert_eq!(status_message.type_, message_types::TASK_STATUS);
        assert_eq!(status_message.body["status"], "canceled");
    }

    #[tokio::test]
    async fn credential_offer_request_issue_roundtrip() {
        let issuer =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x31)).expect("issuer"));
        let holder =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x32)).expect("holder"));
        let holder_key =
            crate::halo::didcomm::extract_x25519_public_key_from_doc(&holder.did_document)
                .expect("holder x25519");
        let grantor_puf = [0x11u8; 32];
        let grantee_puf = [0x22u8; 32];
        let created_at = 1_000_000u64;
        let nonce = 9u64;
        let grant_id = crate::pod::acl::AccessGrant::compute_id(
            &grantor_puf,
            &grantee_puf,
            "results/*",
            created_at,
            nonce,
        );
        let grant = crate::pod::acl::AccessGrant {
            grant_id,
            grantor_puf,
            grantee_puf,
            key_pattern: "results/*".to_string(),
            permissions: crate::pod::acl::GrantPermissions::read_only(),
            expires_at: Some(2_000_000),
            created_at,
            nonce,
            revoked: false,
        };

        let offer = DIDCommMessage::new(
            message_types::CREDENTIAL_OFFER,
            Some(&issuer.did),
            vec![holder.did.clone()],
            json!({
                "grant": grant,
                "resource_uri": "pod://results/theorem_42",
                "requested_action": "read",
                "current_time": 1_500_000u64,
            }),
        );
        let packed_offer = pack_authcrypt(&offer, &issuer, &holder_key).expect("pack offer");

        let mut holder_handler = DIDCommHandler::new(holder.clone());
        holder_handler.register_builtin_handlers();
        let packed_request = holder_handler
            .handle_incoming(&packed_offer, resolver(&issuer, &holder))
            .await
            .expect("handle credential offer")
            .expect("credential request response");

        let (request_message, _) = crate::halo::didcomm::unpack_with_resolver(
            &packed_request,
            &issuer,
            resolver(&holder, &issuer),
        )
        .expect("unpack credential request");
        assert_eq!(request_message.type_, message_types::CREDENTIAL_REQUEST);

        let attachment =
            extract_credential_attachment(&request_message).expect("extract attachment");
        let keys = credential_keys().expect("credential keys");
        let verified = zk_credential::verify_credential_proof(&keys.1, &attachment.proof_bundle)
            .expect("verify request proof");
        assert!(verified);

        let mut issuer_handler = DIDCommHandler::new(issuer.clone());
        issuer_handler.register_builtin_handlers();
        let packed_issue = issuer_handler
            .handle_incoming(&packed_request, resolver(&holder, &issuer))
            .await
            .expect("handle credential request")
            .expect("credential issue response");

        let (issue_message, _) = crate::halo::didcomm::unpack_with_resolver(
            &packed_issue,
            &holder,
            resolver(&issuer, &holder),
        )
        .expect("unpack credential issue");
        assert_eq!(issue_message.type_, message_types::CREDENTIAL_ISSUE);
        assert_eq!(issue_message.body["status"], "issued");
        assert_eq!(issue_message.body["verified"], true);

        let packed_ack = holder_handler
            .handle_incoming(&packed_issue, resolver(&issuer, &holder))
            .await
            .expect("handle credential issue")
            .expect("ack response");
        let (ack_message, _) = crate::halo::didcomm::unpack_with_resolver(
            &packed_ack,
            &issuer,
            resolver(&holder, &issuer),
        )
        .expect("unpack ack");
        assert_eq!(ack_message.type_, message_types::ACK);
    }

    #[tokio::test]
    async fn credential_request_rejects_invalid_binding_metadata() {
        let issuer =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x41)).expect("issuer"));
        let holder =
            Arc::new(crate::halo::did::did_from_genesis_seed(&seed(0x42)).expect("holder"));
        let holder_key =
            crate::halo::didcomm::extract_x25519_public_key_from_doc(&holder.did_document)
                .expect("holder x25519");
        let grantor_puf = [0x51u8; 32];
        let grantee_puf = [0x61u8; 32];
        let created_at = 1_000_000u64;
        let nonce = 19u64;
        let grant_id = crate::pod::acl::AccessGrant::compute_id(
            &grantor_puf,
            &grantee_puf,
            "results/*",
            created_at,
            nonce,
        );
        let grant = crate::pod::acl::AccessGrant {
            grant_id,
            grantor_puf,
            grantee_puf,
            key_pattern: "results/*".to_string(),
            permissions: crate::pod::acl::GrantPermissions::read_only(),
            expires_at: Some(2_000_000),
            created_at,
            nonce,
            revoked: false,
        };

        let offer = DIDCommMessage::new(
            message_types::CREDENTIAL_OFFER,
            Some(&issuer.did),
            vec![holder.did.clone()],
            json!({
                "grant": grant,
                "resource_uri": "pod://results/theorem_99",
                "requested_action": "read",
                "current_time": 1_500_000u64,
            }),
        );
        let packed_offer = pack_authcrypt(&offer, &issuer, &holder_key).expect("pack offer");

        let mut holder_handler = DIDCommHandler::new(holder.clone());
        holder_handler.register_builtin_handlers();
        let packed_request = holder_handler
            .handle_incoming(&packed_offer, resolver(&issuer, &holder))
            .await
            .expect("handle credential offer")
            .expect("credential request response");

        let (request_message, _) = crate::halo::didcomm::unpack_with_resolver(
            &packed_request,
            &issuer,
            resolver(&holder, &issuer),
        )
        .expect("unpack credential request");

        let mut attachment = extract_credential_attachment(&request_message).expect("attachment");
        attachment.proof_bundle.schema_version = 1;
        let err = validate_credential_attachment_for_request(&attachment).expect_err("schema fail");
        assert!(err.contains("expected 2"));

        attachment.proof_bundle.schema_version = 2;
        attachment.resource_uri = " ".to_string();
        let err =
            validate_credential_attachment_for_request(&attachment).expect_err("resource fail");
        assert!(err.contains("resource_uri"));

        attachment.resource_uri = "pod://results/theorem_99".to_string();
        attachment.requested_action = " ".to_string();
        let err = validate_credential_attachment_for_request(&attachment).expect_err("action fail");
        assert!(err.contains("requested_action"));
    }
}
