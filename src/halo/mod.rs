pub mod a2a_bridge;
pub mod adapters;
pub mod addons;
pub mod agent_auth;
pub mod agentpmt;
pub mod api_keys;
pub mod attest;
pub mod audit;
pub mod auth;
pub mod circuit;
pub mod circuit_policy;
pub mod config;
pub mod crypto_scope;
pub mod detect;
pub mod did;
pub mod didcomm;
pub mod didcomm_handler;
pub mod encrypted_file;
pub mod evm_wallet;
pub mod funding;
pub mod genesis_entropy;
pub mod genesis_seed;
pub mod hash;
pub mod http_client;
pub mod hybrid_kem;
pub mod identity;
pub mod identity_ledger;
pub mod migration;
pub mod nym;
pub mod nym_native;
pub mod onchain;
pub mod p2p_discovery;
pub mod p2p_node;
pub mod password;
pub mod pinata;
pub mod pq;
pub mod pricing;
pub mod privacy_controller;
pub mod profile;
pub mod proxy;
pub mod public_input_schema;
pub mod runner;
pub mod schema;
pub mod session_manager;
pub mod startup;
pub mod trace;
pub mod trust;
pub mod twine_anchor;
pub mod util;
pub mod vault;
pub mod viewer;
pub mod wdk_proxy;
pub mod wrap;
pub mod x402;
pub mod zk_compute;
pub mod zk_credential;
pub mod zk_guests;

pub fn generic_agents_allowed() -> bool {
    std::env::var("AGENTHALO_ALLOW_GENERIC")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}
