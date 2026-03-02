use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeMixnetStatus {
    pub enabled: bool,
    pub connected: bool,
    pub address: Option<String>,
    pub inbound_registered: bool,
    pub cover_traffic_active: bool,
    pub note: String,
}

impl Default for NativeMixnetStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            connected: false,
            address: None,
            inbound_registered: false,
            cover_traffic_active: false,
            note: "native mixnet disabled".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NymInboundMessage {
    pub payload_base64: String,
    pub sender_tag: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NativeMixnetConfig {
    pub enabled: bool,
    pub requested_gateway: Option<String>,
    pub include_surbs: u32,
    pub cover_traffic_interval_secs: u64,
    pub register_inbound: bool,
}

impl Default for NativeMixnetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            requested_gateway: None,
            include_surbs: 32,
            cover_traffic_interval_secs: 0,
            register_inbound: true,
        }
    }
}

impl NativeMixnetConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        cfg.enabled = std::env::var("NYM_NATIVE_ENABLED")
            .ok()
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        cfg.requested_gateway = std::env::var("NYM_NATIVE_GATEWAY")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        cfg.include_surbs = std::env::var("NYM_SURBS")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(32);
        cfg.cover_traffic_interval_secs = std::env::var("NYM_COVER_TRAFFIC_SECS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(0);
        cfg.register_inbound = std::env::var("NYM_REGISTER_INBOUND")
            .ok()
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);
        cfg
    }
}

#[cfg(feature = "nym-native")]
mod imp {
    use super::{NativeMixnetConfig, NativeMixnetStatus, NymInboundMessage};
    use crate::halo::config;
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use nym_sdk::mixnet::{
        AnonymousSenderTag, IncludedSurbs, MixnetClientBuilder, MixnetClientSender,
        MixnetMessageSender, Recipient,
    };
    use std::sync::{OnceLock, RwLock};
    use std::time::Duration;
    use tokio::sync::broadcast;

    static STATUS: OnceLock<RwLock<NativeMixnetStatus>> = OnceLock::new();
    static SENDER: OnceLock<MixnetClientSender> = OnceLock::new();
    static SELF_ADDRESS: OnceLock<String> = OnceLock::new();
    static INBOUND_TX: OnceLock<broadcast::Sender<NymInboundMessage>> = OnceLock::new();

    fn status_cell() -> &'static RwLock<NativeMixnetStatus> {
        STATUS.get_or_init(|| RwLock::new(NativeMixnetStatus::default()))
    }

    fn update_status<F>(f: F)
    where
        F: FnOnce(&mut NativeMixnetStatus),
    {
        if let Ok(mut guard) = status_cell().write() {
            f(&mut guard);
        }
    }

    pub fn status_snapshot() -> NativeMixnetStatus {
        status_cell()
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| NativeMixnetStatus::default())
    }

    pub fn subscribe_inbound() -> Option<broadcast::Receiver<NymInboundMessage>> {
        INBOUND_TX.get().map(|tx| tx.subscribe())
    }

    pub async fn ensure_connected() -> Result<(), String> {
        if SENDER.get().is_some() {
            return Ok(());
        }

        let cfg = NativeMixnetConfig::from_env();
        update_status(|s| {
            s.enabled = cfg.enabled;
            if !cfg.enabled {
                s.note = "native mixnet disabled by NYM_NATIVE_ENABLED".to_string();
            }
        });

        if !cfg.enabled {
            return Ok(());
        }

        let mut builder = MixnetClientBuilder::new_ephemeral();
        if let Some(gateway) = cfg.requested_gateway.clone() {
            builder = builder.request_gateway(gateway);
        }
        builder = builder.with_wait_for_gateway(true);

        let client = builder
            .build()
            .map_err(|e| format!("build native Nym client: {e}"))?
            .connect_to_mixnet()
            .await
            .map_err(|e| format!("connect native Nym client: {e}"))?;

        let self_address = client.nym_address().to_string();
        let self_recipient = Recipient::try_from_base58_string(&self_address)
            .map_err(|e| format!("parse own Nym address `{self_address}`: {e}"))?;
        let sender = client.split_sender();

        let (tx, _rx) = broadcast::channel(256);
        let _ = INBOUND_TX.set(tx.clone());
        let _ = SELF_ADDRESS.set(self_address.clone());
        let _ = SENDER.set(sender.clone());

        let mut receiver_client = client;
        tokio::spawn(async move {
            while let Some(messages) = receiver_client.wait_for_messages().await {
                for msg in messages {
                    let event = NymInboundMessage {
                        payload_base64: B64.encode(msg.message),
                        sender_tag: msg.sender_tag.map(|tag| tag.to_string()),
                    };
                    let _ = tx.send(event);
                }
            }
        });

        if cfg.cover_traffic_interval_secs > 0 {
            let sender_for_cover = sender.clone();
            let interval = Duration::from_secs(cfg.cover_traffic_interval_secs);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                loop {
                    ticker.tick().await;
                    let _ = sender_for_cover
                        .send_message(
                            self_recipient,
                            b"agenthalo.cover_traffic.v1".as_slice(),
                            IncludedSurbs::Amount(1),
                        )
                        .await;
                }
            });
            update_status(|s| s.cover_traffic_active = true);
        }

        if cfg.register_inbound {
            let registration = serde_json::json!({
                "nym_address": self_address,
                "registered_at": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            });
            let path = config::halo_dir().join("nym_service_provider.json");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create native nym registration dir: {e}"))?;
            }
            let payload = serde_json::to_vec_pretty(&registration)
                .map_err(|e| format!("serialize native nym registration: {e}"))?;
            std::fs::write(&path, payload)
                .map_err(|e| format!("write native nym registration: {e}"))?;
            update_status(|s| s.inbound_registered = true);
        }

        update_status(|s| {
            s.enabled = true;
            s.connected = true;
            s.address = Some(self_address);
            s.note = "native mixnet connected via nym-sdk".to_string();
        });

        Ok(())
    }

    pub async fn send_message_with_surbs(
        recipient: &str,
        payload: &[u8],
        include_surbs: u32,
    ) -> Result<(), String> {
        ensure_connected().await?;
        let Some(sender) = SENDER.get() else {
            return Err("native mixnet sender unavailable".to_string());
        };
        let recipient = Recipient::try_from_base58_string(recipient)
            .map_err(|e| format!("invalid nym recipient `{recipient}`: {e}"))?;
        let surbs = if include_surbs == 0 {
            IncludedSurbs::ExposeSelfAddress
        } else {
            IncludedSurbs::Amount(include_surbs)
        };
        sender
            .send_message(recipient, payload, surbs)
            .await
            .map_err(|e| format!("send native mixnet message: {e}"))
    }

    pub async fn send_reply_via_surb(surb_tag: &str, payload: &[u8]) -> Result<(), String> {
        ensure_connected().await?;
        let Some(sender) = SENDER.get() else {
            return Err("native mixnet sender unavailable".to_string());
        };
        let tag = AnonymousSenderTag::try_from_base58_string(surb_tag)
            .map_err(|e| format!("invalid SURB tag `{surb_tag}`: {e}"))?;
        sender
            .send_reply(tag, payload)
            .await
            .map_err(|e| format!("send native mixnet SURB reply: {e}"))
    }
}

#[cfg(not(feature = "nym-native"))]
mod imp {
    use super::{NativeMixnetStatus, NymInboundMessage};
    use tokio::sync::broadcast;

    pub fn status_snapshot() -> NativeMixnetStatus {
        NativeMixnetStatus::default()
    }

    pub fn subscribe_inbound() -> Option<broadcast::Receiver<NymInboundMessage>> {
        None
    }

    pub async fn ensure_connected() -> Result<(), String> {
        Ok(())
    }

    pub async fn send_message_with_surbs(
        _recipient: &str,
        _payload: &[u8],
        _include_surbs: u32,
    ) -> Result<(), String> {
        Err("native Nym support is not enabled (build with `--features nym-native`)".to_string())
    }

    pub async fn send_reply_via_surb(_surb_tag: &str, _payload: &[u8]) -> Result<(), String> {
        Err("native Nym support is not enabled (build with `--features nym-native`)".to_string())
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    use super::{send_message_with_surbs, status_snapshot, NativeMixnetConfig};

    #[test]
    fn native_mixnet_config_parses_env_defaults() {
        std::env::remove_var("NYM_NATIVE_ENABLED");
        std::env::remove_var("NYM_NATIVE_GATEWAY");
        std::env::remove_var("NYM_SURBS");
        std::env::remove_var("NYM_COVER_TRAFFIC_SECS");
        std::env::remove_var("NYM_REGISTER_INBOUND");

        let cfg = NativeMixnetConfig::from_env();
        assert!(!cfg.enabled);
        assert_eq!(cfg.include_surbs, 32);
        assert_eq!(cfg.cover_traffic_interval_secs, 0);
        assert!(cfg.register_inbound);
    }

    #[test]
    fn status_snapshot_reports_disabled_by_default() {
        let status = status_snapshot();
        assert!(!status.enabled);
        assert!(!status.connected);
        assert!(status.address.is_none());
    }

    #[test]
    fn send_without_native_feature_returns_error() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let err = rt
            .block_on(send_message_with_surbs("recipient", b"hello", 5))
            .expect_err("sending should fail without active native transport");
        #[cfg(not(feature = "nym-native"))]
        assert!(err.contains("not enabled"));
        #[cfg(feature = "nym-native")]
        assert!(!err.trim().is_empty());
    }
}
