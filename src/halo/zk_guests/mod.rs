//! Builtin guest program registry for verifiable computation.
//!
//! Builtin guests execute as deterministic Rust functions and can run without
//! the `zk-compute` feature. Custom ELF execution remains feature-gated.

pub mod algorithm_compliance;
pub mod range_proof;
pub mod secure_aggregation;
pub mod set_membership;

pub mod image_ids {
    use sha2::{Digest, Sha256};

    pub const RANGE_PROOF: &str =
        "3e1d1b08116d621ca0755117e6a4c5fcaefa3215d1736deec0585cc2ac310f2e";
    pub const SET_MEMBERSHIP: &str =
        "9fe3af2bc810892bf5a56d61ab8ad644b77dfc4c636ecc42a5da4c1167eccf5b";
    pub const SECURE_AGGREGATION: &str =
        "a74d48ed6ba291c62229b9f6aa28d866cc89456953a28481792076ab3ef98cb1";
    pub const ALGORITHM_COMPLIANCE: &str =
        "8553a57cf70bdaaac134ee951801d34c1c1440a57c7e394b8e625e9f1cd8cc8c";

    pub fn compute_image_id(guest_name: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update("agenthalo.guest_image.");
        hasher.update(guest_name.as_bytes());
        hasher.update(".v1");
        let digest = hasher.finalize();
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Returns true when any builtin ID diverges from its deterministic derivation.
    pub fn has_unresolved_images() -> bool {
        RANGE_PROOF != compute_image_id("range_proof")
            || SET_MEMBERSHIP != compute_image_id("set_membership")
            || SECURE_AGGREGATION != compute_image_id("secure_aggregation")
            || ALGORITHM_COMPLIANCE != compute_image_id("algorithm_compliance")
    }
}

#[cfg(test)]
mod tests {
    use super::image_ids;

    #[test]
    fn image_ids_are_real() {
        assert!(!image_ids::has_unresolved_images());
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

    #[test]
    fn image_ids_match_deterministic_derivation() {
        assert_eq!(
            image_ids::RANGE_PROOF,
            image_ids::compute_image_id("range_proof")
        );
        assert_eq!(
            image_ids::SET_MEMBERSHIP,
            image_ids::compute_image_id("set_membership")
        );
        assert_eq!(
            image_ids::SECURE_AGGREGATION,
            image_ids::compute_image_id("secure_aggregation")
        );
        assert_eq!(
            image_ids::ALGORITHM_COMPLIANCE,
            image_ids::compute_image_id("algorithm_compliance")
        );
    }
}
