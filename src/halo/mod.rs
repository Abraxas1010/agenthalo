pub mod adapters;
pub mod agentpmt;
pub mod attest;
pub mod audit;
pub mod auth;
pub mod config;
pub mod detect;
pub mod pq;
pub mod pricing;
pub mod runner;
pub mod schema;
pub mod trace;
pub mod trust;
pub mod viewer;
pub mod wrap;

pub fn generic_agents_allowed() -> bool {
    std::env::var("AGENTHALO_ALLOW_GENERIC")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}
