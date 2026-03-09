use crate::halo::did::{did_from_genesis_seed, DIDIdentity};
use crate::halo::didcomm_handler::DIDCommHandler;
use crate::halo::nym::{self, NymStatus};
use crate::halo::p2p_discovery::{
    announcement_for_identity, sign_announcement, topic_general, AgentDiscovery,
};
use crate::halo::p2p_node::{P2pConfig, P2pNode};
use crate::persistence::{default_wal_path, load_wal};
use crate::protocol::NucleusDb;
use crate::swarm::chunk_store::ChunkStore;
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

fn load_swarm_db() -> Option<NucleusDb> {
    let db_path = crate::halo::config::db_path();
    let wal_path = default_wal_path(&db_path);
    if wal_path.exists() {
        load_wal(&wal_path, crate::cli::default_witness_cfg())
            .ok()
            .or_else(|| {
                NucleusDb::load_persistent(&db_path, crate::cli::default_witness_cfg()).ok()
            })
    } else if db_path.exists() {
        NucleusDb::load_persistent(&db_path, crate::cli::default_witness_cfg()).ok()
    } else {
        None
    }
}

fn hydrate_bitswap_runtime(node: &mut P2pNode) {
    let swarm_config = crate::swarm::config::SwarmConfig::from_env();
    node.bitswap_runtime_mut()
        .set_require_grants(swarm_config.require_grants);
    if let Some(db) = load_swarm_db() {
        let swarm_store = ChunkStore::load_from_db(&db);
        node.bitswap_runtime_mut()
            .register_local_chunks(&swarm_store.all_chunks());
    }
    let grant_path = crate::halo::config::db_path().with_extension("pod_grants.json");
    let grants = crate::pod::acl::GrantStore::load_or_default(&grant_path);
    node.bitswap_runtime_mut().set_grants(grants);
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
        hydrate_bitswap_runtime(&mut node);
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
        agent_discovery.upsert_trusted_announcement(announcement.clone());
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
    use crate::cli::default_witness_cfg;
    use crate::protocol::{NucleusDb, VcBackend};
    use crate::state::State;
    use crate::swarm::chunk_engine::chunk_data;
    use crate::swarm::chunk_store::ChunkStore;
    use crate::swarm::config::ChunkParams;
    use crate::test_support::lock_env;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "agenthalo_startup_{tag}_{}_{}.ndb",
            std::process::id(),
            nanos
        ))
    }

    fn cleanup_db_files(db_path: &Path) {
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(default_wal_path(db_path));
        let _ = std::fs::remove_file(db_path.with_extension("pod_grants.json"));
    }

    struct EnvVarRestore {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvVarRestore {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, prev }
        }
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            if let Some(prev) = &self.prev {
                std::env::set_var(self.key, prev);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

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

    #[test]
    fn startup_hydrates_bitswap_chunks_from_db() {
        let join = std::thread::Builder::new()
            .name("startup_hydrates_bitswap_chunks_from_db".to_string())
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build tokio runtime");
                rt.block_on(async {
                    let _guard = lock_env();
                    let db_path = temp_db_path("bitswap_hydrate");
                    let mut db = NucleusDb::new(
                        State::new(vec![]),
                        VcBackend::BinaryMerkle,
                        default_witness_cfg(),
                    );
                    let mut store = ChunkStore::new();
                    let chunk = chunk_data(b"startup bitswap", &ChunkParams::default())
                        .into_iter()
                        .next()
                        .expect("chunk");
                    store
                        .store_chunks(&mut db, &[chunk.clone()])
                        .expect("store chunks");
                    db.save_persistent(&db_path).expect("save db");
                    std::env::set_var("AGENTHALO_DB_PATH", db_path.display().to_string());

                    let seed = [0x7cu8; 64];
                    let config = StartupConfig {
                        nym_enabled: false,
                        p2p_enabled: true,
                        p2p_config: P2pConfig {
                            listen_port: 0,
                            ..P2pConfig::default()
                        },
                        ..StartupConfig::default()
                    };
                    let mut stack = start(&seed, config).await.expect("startup");
                    let peer = libp2p::PeerId::random();
                    let response = stack
                        .p2p_node
                        .as_mut()
                        .expect("p2p node")
                        .bitswap_runtime_mut()
                        .handle_request(
                            &peer,
                            crate::swarm::bitswap::BitswapMessage::Want(vec![chunk.id.clone()]),
                        );
                    assert_eq!(
                        response,
                        crate::swarm::bitswap::BitswapMessage::Have(vec![chunk.id])
                    );

                    std::env::remove_var("AGENTHALO_DB_PATH");
                    cleanup_db_files(&db_path);
                });
            })
            .expect("spawn large-stack test thread");
        join.join()
            .expect("startup bitswap hydration test thread panicked");
    }

    #[test]
    fn startup_hydrates_require_grants_mode_from_env() {
        let join = std::thread::Builder::new()
            .name("startup_hydrates_require_grants_mode_from_env".to_string())
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build tokio runtime");
                rt.block_on(async {
                    let _guard = lock_env();
                    let _require_grants = EnvVarRestore::set("HALO_BITSWAP_REQUIRE_GRANTS", "1");
                    let db_path = temp_db_path("bitswap_require_grants");
                    let _db_path_env =
                        EnvVarRestore::set("AGENTHALO_DB_PATH", &db_path.display().to_string());

                    let mut db = NucleusDb::new(
                        State::new(vec![]),
                        VcBackend::BinaryMerkle,
                        default_witness_cfg(),
                    );
                    let mut store = ChunkStore::new();
                    let chunk = chunk_data(b"startup locked bitswap", &ChunkParams::default())
                        .into_iter()
                        .next()
                        .expect("chunk");
                    store
                        .store_chunks(&mut db, &[chunk.clone()])
                        .expect("store chunks");
                    db.save_persistent(&db_path).expect("save db");

                    let seed = [0x7du8; 64];
                    let config = StartupConfig {
                        nym_enabled: false,
                        p2p_enabled: true,
                        p2p_config: P2pConfig {
                            listen_port: 0,
                            ..P2pConfig::default()
                        },
                        ..StartupConfig::default()
                    };
                    let mut stack = start(&seed, config).await.expect("startup");
                    let peer = libp2p::PeerId::random();
                    let response = stack
                        .p2p_node
                        .as_mut()
                        .expect("p2p node")
                        .bitswap_runtime_mut()
                        .handle_request(
                            &peer,
                            crate::swarm::bitswap::BitswapMessage::Want(vec![chunk.id]),
                        );
                    assert_eq!(
                        response,
                        crate::swarm::bitswap::BitswapMessage::Have(Vec::new())
                    );

                    cleanup_db_files(&db_path);
                });
            })
            .expect("spawn large-stack test thread");
        join.join()
            .expect("startup require-grants test thread panicked");
    }
}
