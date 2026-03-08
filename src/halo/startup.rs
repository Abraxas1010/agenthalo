use crate::halo::did::{did_from_genesis_seed, DIDIdentity};
use crate::halo::didcomm_handler::DIDCommHandler;
use crate::halo::nym::{self, NymStatus};
use crate::halo::p2p_discovery::{
    announcement_for_identity, sign_announcement, topic_general, AgentDiscovery,
};
use crate::halo::p2p_node::{P2pConfig, P2pNode};
use base64::Engine as _;
use std::sync::Arc;
use std::time::Duration;

pub struct HaloStack {
    pub identity: Arc<DIDIdentity>,
    pub p2p_node: Option<P2pNode>,
    pub didcomm_handler: Option<DIDCommHandler>,
    pub discovery: Option<AgentDiscovery>,
    pub a2a_bridge_task: Option<tokio::task::JoinHandle<Result<(), String>>>,
    pub nym_status: NymStatus,
}

#[derive(Clone, Debug)]
pub struct StartupConfig {
    pub nym_enabled: bool,
    pub nym_max_retries: u32,
    pub nym_retry_delay: Duration,
    pub p2p_enabled: bool,
    pub p2p_config: P2pConfig,
    pub a2a_bridge_port: Option<u16>,
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self {
            nym_enabled: true,
            nym_max_retries: 10,
            nym_retry_delay: Duration::from_secs(2),
            p2p_enabled: true,
            p2p_config: P2pConfig::default(),
            a2a_bridge_port: None,
        }
    }
}

impl StartupConfig {
    pub fn from_env() -> Result<Self, String> {
        let nym_enabled = std::env::var("NYM_ENABLED")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);

        let nym_max_retries = std::env::var("NYM_MAX_RETRIES")
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or(10);

        let nym_retry_delay_secs = std::env::var("NYM_RETRY_DELAY_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(2);

        let p2p_config = P2pConfig::from_env()?;
        let p2p_enabled = std::env::var("P2P_ENABLED")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);
        let a2a_bridge_port = std::env::var("A2A_BRIDGE_PORT")
            .ok()
            .and_then(|value| value.trim().parse::<u16>().ok())
            .and_then(|value| if value == 0 { None } else { Some(value) });

        Ok(Self {
            nym_enabled,
            nym_max_retries,
            nym_retry_delay: Duration::from_secs(nym_retry_delay_secs),
            p2p_enabled,
            p2p_config,
            a2a_bridge_port,
        })
    }
}

pub async fn start(seed: &[u8; 64], config: StartupConfig) -> Result<HaloStack, String> {
    let identity = Arc::new(did_from_genesis_seed(seed)?);
    eprintln!("[AgentHalo/Startup][1/5] identity loaded: {}", identity.did);

    let mut nym_status = nym::status();
    if config.nym_enabled {
        nym::start_native_transport_if_enabled().await?;
        for attempt in 0..=config.nym_max_retries {
            nym_status = nym::status();
            if nym_status.healthy || nym_status.socks5_proxy.is_none() {
                break;
            }
            if attempt == config.nym_max_retries {
                break;
            }
            tokio::time::sleep(config.nym_retry_delay).await;
        }
        if !nym_status.healthy && nym::is_fail_closed() && nym_status.socks5_proxy.is_some() {
            return Err("Nym SOCKS5 is configured but unhealthy in fail-closed mode".to_string());
        }
    }
    eprintln!("[AgentHalo/Startup][2/5] nym mode: {:?}", nym_status.mode);

    let mut p2p_node = None;
    let mut discovery = None;
    let mut didcomm_handler = None;
    let mut a2a_bridge_task = None;

    if config.p2p_enabled && config.p2p_config.enabled {
        let mut node = P2pNode::create_from_did(&identity, &config.p2p_config)?;
        eprintln!("[AgentHalo/Startup][3/5] p2p peer id: {}", node.peer_id());

        let mut handler = DIDCommHandler::new(identity.clone());
        handler.register_builtin_handlers();
        eprintln!("[AgentHalo/Startup][4/5] didcomm handlers ready");

        if let Some(mut inbound) = nym::subscribe_mixnet_inbound() {
            let mixnet_handler = handler.clone();
            let identity_for_mixnet = identity.clone();
            tokio::spawn(async move {
                while let Ok(event) = inbound.recv().await {
                    let packed = match base64::engine::general_purpose::STANDARD
                        .decode(event.payload_base64.as_bytes())
                    {
                        Ok(bytes) => bytes,
                        Err(err) => {
                            eprintln!(
                                "[AgentHalo/Nym] failed to decode inbound mixnet payload: {err}"
                            );
                            continue;
                        }
                    };

                    match mixnet_handler
                        .handle_incoming(&packed, |did| {
                            if did == identity_for_mixnet.did {
                                Some(identity_for_mixnet.did_document.clone())
                            } else {
                                None
                            }
                        })
                        .await
                    {
                        Ok(Some(reply)) => {
                            if let Some(sender_tag) = event.sender_tag.as_deref() {
                                if let Err(err) = nym::send_mixnet_reply(sender_tag, &reply).await {
                                    eprintln!(
                                        "[AgentHalo/Nym] failed SURB reply for inbound message: {err}"
                                    );
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(err) => {
                            eprintln!("[AgentHalo/Nym] inbound DIDComm handling failed: {err}");
                        }
                    }
                }
            });
            eprintln!("[AgentHalo/Startup] native mixnet inbound bridge active");
        }

        let mut agent_discovery = AgentDiscovery::new();
        agent_discovery.subscribe(&topic_general(), node.gossipsub_mut())?;
        let mut announcement = announcement_for_identity(
            &identity,
            *node.peer_id(),
            Vec::new(),
            node.listen_addresses()
                .into_iter()
                .map(|addr| addr.to_string())
                .collect(),
        );
        for topic in announcement.topics() {
            if !agent_discovery.is_subscribed(&topic) {
                let _ = agent_discovery.subscribe(&topic, node.gossipsub_mut());
            }
        }
        sign_announcement(&identity, &mut announcement)?;
        agent_discovery.upsert_verified(announcement.clone());
        for topic in announcement.topics() {
            let _ =
                agent_discovery.announce(&identity, &topic, &announcement, node.gossipsub_mut());
        }
        let _ = agent_discovery.publish_to_dht(&announcement, node.kademlia_mut());
        eprintln!("[AgentHalo/Startup][5/5] discovery bootstrapped");

        didcomm_handler = Some(handler);
        discovery = Some(agent_discovery);
        p2p_node = Some(node);
    } else {
        eprintln!("[AgentHalo/Startup][3-5/5] p2p/didcomm/discovery disabled");
    }

    if let Some(port) = config.a2a_bridge_port {
        let identity_for_bridge = identity.clone();
        a2a_bridge_task = Some(tokio::spawn(async move {
            crate::halo::a2a_bridge::start_a2a_bridge(identity_for_bridge, port, Vec::new()).await
        }));
        eprintln!("[AgentHalo/Startup] A2A bridge task spawned on port {port}");
    }

    Ok(HaloStack {
        identity,
        p2p_node,
        didcomm_handler,
        discovery,
        a2a_bridge_task,
        nym_status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn startup_without_p2p_still_loads_identity() {
        let seed = [0x7au8; 64];
        let config = StartupConfig {
            nym_enabled: false,
            p2p_enabled: false,
            ..StartupConfig::default()
        };
        let stack = start(&seed, config).await.expect("startup without p2p");
        assert!(stack.identity.did.starts_with("did:key:"));
        assert!(stack.p2p_node.is_none());
        assert!(stack.discovery.is_none());
        assert!(stack.a2a_bridge_task.is_none());
    }

    #[tokio::test]
    async fn startup_with_nym_disabled_skips_transport() {
        let seed = [0x7bu8; 64];
        let config = StartupConfig {
            nym_enabled: false,
            p2p_enabled: false,
            ..StartupConfig::default()
        };
        let stack = start(&seed, config).await.expect("startup");
        assert!(!stack.nym_status.healthy);
        assert_eq!(stack.nym_status.mode, crate::halo::nym::NymMode::Disabled);
    }
}
