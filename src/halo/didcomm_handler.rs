use crate::halo::did::DIDIdentity;
use crate::halo::didcomm::{
    extract_x25519_public_key_from_doc, message_types, pack_authcrypt, unpack_with_resolver,
    DIDCommMessage,
};
use futures_util::future::BoxFuture;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

pub type MessageHandler =
    Arc<dyn Fn(DIDCommMessage) -> BoxFuture<'static, Option<DIDCommMessage>> + Send + Sync>;

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
        let sender_x25519 = extract_x25519_public_key_from_doc(&sender_doc)?;
        response.from = Some(self.identity.did.clone());
        response.to = vec![sender_did];

        let packed_response = pack_authcrypt(&response, &self.identity, &sender_x25519)?;
        Ok(Some(packed_response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::didcomm::{pack_authcrypt, DIDCommMessage};

    fn seed(byte: u8) -> [u8; 64] {
        [byte; 64]
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
            .handle_incoming(&packed_ping, |did| {
                if did == sender.did {
                    Some(sender.did_document.clone())
                } else if did == recipient.did {
                    Some(recipient.did_document.clone())
                } else {
                    None
                }
            })
            .await
            .expect("handle incoming")
            .expect("ack response");

        let (ack_message, _) =
            crate::halo::didcomm::unpack_with_resolver(&packed_ack, &sender, |did| {
                if did == recipient.did {
                    Some(recipient.did_document.clone())
                } else if did == sender.did {
                    Some(sender.did_document.clone())
                } else {
                    None
                }
            })
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
            .handle_incoming(&packed, |did| {
                if did == sender.did {
                    Some(sender.did_document.clone())
                } else if did == recipient.did {
                    Some(recipient.did_document.clone())
                } else {
                    None
                }
            })
            .await
            .expect("handle incoming")
            .expect("task status response");

        let (status_message, _) =
            crate::halo::didcomm::unpack_with_resolver(&packed_status, &sender, |did| {
                if did == recipient.did {
                    Some(recipient.did_document.clone())
                } else if did == sender.did {
                    Some(sender.did_document.clone())
                } else {
                    None
                }
            })
            .expect("unpack status");

        assert_eq!(status_message.type_, message_types::TASK_STATUS);
        assert_eq!(status_message.body["status"], "submitted");
    }
}
