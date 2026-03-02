//! Pre-compiled guest program registry for verifiable computation.
//!
//! Guest programs are compiled to RISC-V ELF and executed inside a zkVM runtime.
//! This module intentionally ships placeholders in default builds.

pub mod algorithm_compliance;
pub mod range_proof;
pub mod secure_aggregation;
pub mod set_membership;

pub mod image_ids {
    pub const RANGE_PROOF: &str = "placeholder-range-proof-image-id";
    pub const SET_MEMBERSHIP: &str = "placeholder-set-membership-image-id";
    pub const SECURE_AGGREGATION: &str = "placeholder-aggregation-image-id";
    pub const ALGORITHM_COMPLIANCE: &str = "placeholder-compliance-image-id";
}
