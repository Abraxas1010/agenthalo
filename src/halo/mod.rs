pub mod adapters;
pub mod addons;
pub mod agentpmt;
pub mod api_keys;
pub mod attest;
pub mod audit;
pub mod auth;
pub mod circuit;
pub mod circuit_policy;
pub mod config;
pub mod detect;
pub mod funding;
pub mod identity;
pub mod identity_ledger;
pub mod onchain;
pub mod pinata;
pub mod pq;
pub mod pricing;
pub mod profile;
pub mod proxy;
pub mod public_input_schema;
pub mod runner;
pub mod schema;
pub mod trace;
pub mod trust;
pub mod util;
pub mod vault;
pub mod viewer;
pub mod wdk_proxy;
pub mod wrap;
pub mod x402;

pub fn generic_agents_allowed() -> bool {
    std::env::var("AGENTHALO_ALLOW_GENERIC")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}
