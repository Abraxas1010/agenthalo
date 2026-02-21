use crate::security::VcProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitmentPolicy {
    pub scheme_id: String,
    pub domain_separator: String,
    pub max_degree: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitmentPolicyError {
    EmptySchemeId,
    EmptyDomainSeparator,
    ZeroMaxDegree,
    SchemeProfileMismatch {
        profile: VcProfile,
        scheme_id: String,
    },
    DegreeBoundInsufficient {
        required: usize,
        max_degree: usize,
    },
}

pub trait PolyCommitScheme {
    fn scheme_id() -> &'static str;
    fn default_domain_separator() -> &'static str;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KzgScheme;

impl PolyCommitScheme for KzgScheme {
    fn scheme_id() -> &'static str {
        "kzg"
    }

    fn default_domain_separator() -> &'static str {
        "nucleusdb.vc.kzg.v1"
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpaScheme;

impl PolyCommitScheme for IpaScheme {
    fn scheme_id() -> &'static str {
        "ipa"
    }

    fn default_domain_separator() -> &'static str {
        "nucleusdb.vc.ipa.v1"
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryMerkleScheme;

impl PolyCommitScheme for BinaryMerkleScheme {
    fn scheme_id() -> &'static str {
        "binary_merkle"
    }

    fn default_domain_separator() -> &'static str {
        "nucleusdb.vc.binary_merkle.v1"
    }
}

pub fn validate_commitment_policy(
    profile: VcProfile,
    max_vector_len: usize,
    policy: &CommitmentPolicy,
) -> Result<(), CommitmentPolicyError> {
    if policy.scheme_id.trim().is_empty() {
        return Err(CommitmentPolicyError::EmptySchemeId);
    }
    if policy.domain_separator.trim().is_empty() {
        return Err(CommitmentPolicyError::EmptyDomainSeparator);
    }
    if policy.max_degree == 0 {
        return Err(CommitmentPolicyError::ZeroMaxDegree);
    }

    let expected_scheme = match profile {
        VcProfile::Ipa => IpaScheme::scheme_id(),
        VcProfile::Kzg => KzgScheme::scheme_id(),
        VcProfile::BinaryMerkle => BinaryMerkleScheme::scheme_id(),
    };

    if policy.scheme_id != "auto" && policy.scheme_id != expected_scheme {
        return Err(CommitmentPolicyError::SchemeProfileMismatch {
            profile,
            scheme_id: policy.scheme_id.clone(),
        });
    }

    if max_vector_len > policy.max_degree {
        return Err(CommitmentPolicyError::DegreeBoundInsufficient {
            required: max_vector_len,
            max_degree: policy.max_degree,
        });
    }

    Ok(())
}

pub fn default_commitment_policy(profile: VcProfile, max_degree: usize) -> CommitmentPolicy {
    match profile {
        VcProfile::Ipa => CommitmentPolicy {
            scheme_id: IpaScheme::scheme_id().to_string(),
            domain_separator: IpaScheme::default_domain_separator().to_string(),
            max_degree,
        },
        VcProfile::Kzg => CommitmentPolicy {
            scheme_id: KzgScheme::scheme_id().to_string(),
            domain_separator: KzgScheme::default_domain_separator().to_string(),
            max_degree,
        },
        VcProfile::BinaryMerkle => CommitmentPolicy {
            scheme_id: BinaryMerkleScheme::scheme_id().to_string(),
            domain_separator: BinaryMerkleScheme::default_domain_separator().to_string(),
            max_degree,
        },
    }
}
