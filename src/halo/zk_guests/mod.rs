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

    /// Returns true when any guest image ID is still a placeholder value.
    pub fn has_placeholders() -> bool {
        [
            RANGE_PROOF,
            SET_MEMBERSHIP,
            SECURE_AGGREGATION,
            ALGORITHM_COMPLIANCE,
        ]
        .iter()
        .any(|id| id.starts_with("placeholder-"))
    }
}

#[cfg(test)]
mod tests {
    use super::image_ids;

    #[test]
    fn placeholder_detection_works() {
        assert!(image_ids::has_placeholders());
    }

    #[test]
    fn all_image_ids_are_non_empty() {
        for id in [
            image_ids::RANGE_PROOF,
            image_ids::SET_MEMBERSHIP,
            image_ids::SECURE_AGGREGATION,
            image_ids::ALGORITHM_COMPLIANCE,
        ] {
            assert!(!id.is_empty(), "image ID must not be empty");
        }
    }
}
