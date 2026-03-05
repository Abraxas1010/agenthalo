pub mod api;
pub mod audit;
pub mod blob_store;
pub mod cli;
pub mod cockpit;
pub mod commitment;
pub mod comms;
pub mod container;
pub mod dashboard;
pub mod embeddings;
pub mod halo;
pub mod immutable;
pub mod keymap;
pub mod license;
pub mod materialize;
pub mod mcp;
pub mod memory;
pub mod multitenant;
pub mod pcn;
pub mod persistence;
pub mod pod;
pub mod protocol;
pub mod puf;
pub mod security;
pub mod security_utils;
pub mod sheaf;
pub mod sql;
pub mod state;
pub mod transparency;
pub mod trust;
pub mod tui;
pub mod type_map;
pub mod typed_value;
pub mod vc;
pub mod vcs;
pub mod vector_index;
pub mod verifier;
pub mod witness;

pub use multitenant::{
    MultiTenantError, MultiTenantNucleusDb, MultiTenantPolicy, TenantRole, TenantSnapshot,
};
pub use persistence::PersistenceError;
pub use protocol::{CommitError, NucleusDb, QueryProof, VcBackend};
pub use security::{
    default_reduction_contracts, ParameterError, ParameterSet, ReductionContract, RefinementError,
    SecurityPolicyError, VcProfile,
};
pub use state::{Delta, State};

#[cfg(test)]
pub mod test_support {
    use std::sync::{Mutex, OnceLock};

    /// Global lock for process-wide environment variable mutation in tests.
    /// Rust environment access is process-global and not thread-safe across
    /// concurrent writes, so tests that set/remove env vars must serialize.
    pub fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
}
