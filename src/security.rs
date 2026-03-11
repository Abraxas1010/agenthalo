use crate::commitment::{validate_commitment_policy, CommitmentPolicy, CommitmentPolicyError};
use crate::transparency::ct6962::{sth_signature, NodeHash};
use crate::vc::kzg::TrustedSetupError;
use crate::witness::WitnessConfig;
use serde::{Deserialize, Serialize};

pub type FormalProvenance = (&'static str, &'static str, Option<&'static str>);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VcProfile {
    Ipa,
    Kzg,
    BinaryMerkle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityAssumption {
    CollisionResistance,
    WitnessUnforgeability,
    CtAppendOnly,
    IpaBinding,
    KzgTrustedSetup,
    MerkleBinding,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReductionContract {
    pub claim: String,
    pub assumption: SecurityAssumption,
    pub loss_bits: u16,
    pub max_queries: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterSet {
    pub field_bits: u16,
    pub max_vector_len: usize,
    pub max_delta_writes: usize,
    pub max_witnesses: usize,
    pub min_witness_threshold: usize,
    pub max_witness_threshold: usize,
    pub require_kzg_trusted_setup: bool,
    pub kzg_trusted_setup_id: Option<String>,
    pub kzg_trusted_setup_path: Option<String>,
    pub kzg_trusted_setup_attestation_sha512: Option<String>,
    pub commitment_policy: CommitmentPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParameterError {
    ZeroFieldBits,
    FieldBitsTooLarge { bits: u16, max_bits: u16 },
    MaxVectorLenZero,
    MaxDeltaWritesZero,
    WitnessSetTooLarge { witnesses: usize, max: usize },
    WitnessThresholdTooSmall { threshold: usize, min: usize },
    WitnessThresholdTooLarge { threshold: usize, max: usize },
    ThresholdExceedsWitnessSet { threshold: usize, witnesses: usize },
    MissingKzgTrustedSetup,
    MissingKzgTrustedSetupPath,
    MissingKzgTrustedSetupAttestation,
    CommitmentPolicy(CommitmentPolicyError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecurityPolicyError {
    Parameter(ParameterError),
    MissingReductionContracts,
    ReductionLossTooLarge {
        claim: String,
        loss_bits: u16,
    },
    ReductionMaxQueriesZero {
        claim: String,
    },
    KzgTrustedSetup(TrustedSetupError),
    KzgSetupDegreeInsufficient {
        setup_id: String,
        setup_max_degree: usize,
        required_max_vector_len: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefinementError {
    DeltaTooLarge { writes: usize, max: usize },
    StateVectorTooLarge { len: usize, max: usize },
    CommitHeightMismatch { expected: u64, got: u64 },
    PrevRootMismatch { expected: NodeHash, got: NodeHash },
    SthSizeMismatch { expected: u64, got: u64 },
    InvalidSthSignature { expected: String, got: String },
}

impl Default for ParameterSet {
    fn default() -> Self {
        Self {
            field_bits: 64,
            max_vector_len: 1 << 20,
            max_delta_writes: 1 << 16,
            max_witnesses: 128,
            min_witness_threshold: 1,
            max_witness_threshold: 128,
            require_kzg_trusted_setup: true,
            kzg_trusted_setup_id: Some("demo-trusted-setup-v1".to_string()),
            kzg_trusted_setup_path: Some("artifacts/kzg/demo-trusted-setup-v1.json".to_string()),
            kzg_trusted_setup_attestation_sha512: Some(
                "b1ba68a25d64fb0e29348404c7ef8ece8503ae5bd2eb0d8a172ddbc726d70df694cb5f6d323e7435649c35fa365339e93b354262735da11e01c00ad8b17923f1"
                    .to_string(),
            ),
            commitment_policy: CommitmentPolicy {
                scheme_id: "auto".to_string(),
                domain_separator: "nucleusdb.vc.auto.v1".to_string(),
                max_degree: 1 << 20,
            },
        }
    }
}

pub fn default_reduction_contracts(profile: VcProfile) -> Vec<ReductionContract> {
    let mut out = vec![
        ReductionContract {
            claim: "state_authentication_soundness".to_string(),
            assumption: SecurityAssumption::CollisionResistance,
            loss_bits: 16,
            max_queries: 1_000_000,
        },
        ReductionContract {
            claim: "ct_append_only_history".to_string(),
            assumption: SecurityAssumption::CtAppendOnly,
            loss_bits: 8,
            max_queries: 1_000_000,
        },
        ReductionContract {
            claim: "witness_quorum_unforgeability".to_string(),
            assumption: SecurityAssumption::WitnessUnforgeability,
            loss_bits: 8,
            max_queries: 1_000_000,
        },
    ];
    match profile {
        VcProfile::Ipa => out.push(ReductionContract {
            claim: "ipa_binding".to_string(),
            assumption: SecurityAssumption::IpaBinding,
            loss_bits: 24,
            max_queries: 1_000_000,
        }),
        VcProfile::Kzg => out.push(ReductionContract {
            claim: "kzg_binding".to_string(),
            assumption: SecurityAssumption::KzgTrustedSetup,
            loss_bits: 24,
            max_queries: 1_000_000,
        }),
        VcProfile::BinaryMerkle => out.push(ReductionContract {
            claim: "binary_merkle_binding".to_string(),
            assumption: SecurityAssumption::MerkleBinding,
            loss_bits: 24,
            max_queries: 1_000_000,
        }),
    }
    out
}

pub fn validate_parameters(
    params: &ParameterSet,
    profile: VcProfile,
    witness_cfg: &WitnessConfig,
) -> Result<(), ParameterError> {
    const MAX_FIELD_BITS: u16 = 4096;

    if params.field_bits == 0 {
        return Err(ParameterError::ZeroFieldBits);
    }
    if params.field_bits > MAX_FIELD_BITS {
        return Err(ParameterError::FieldBitsTooLarge {
            bits: params.field_bits,
            max_bits: MAX_FIELD_BITS,
        });
    }
    if params.max_vector_len == 0 {
        return Err(ParameterError::MaxVectorLenZero);
    }
    if params.max_delta_writes == 0 {
        return Err(ParameterError::MaxDeltaWritesZero);
    }
    if witness_cfg.witnesses.len() > params.max_witnesses {
        return Err(ParameterError::WitnessSetTooLarge {
            witnesses: witness_cfg.witnesses.len(),
            max: params.max_witnesses,
        });
    }
    if witness_cfg.threshold < params.min_witness_threshold {
        return Err(ParameterError::WitnessThresholdTooSmall {
            threshold: witness_cfg.threshold,
            min: params.min_witness_threshold,
        });
    }
    if witness_cfg.threshold > params.max_witness_threshold {
        return Err(ParameterError::WitnessThresholdTooLarge {
            threshold: witness_cfg.threshold,
            max: params.max_witness_threshold,
        });
    }
    if witness_cfg.threshold > witness_cfg.witnesses.len() {
        return Err(ParameterError::ThresholdExceedsWitnessSet {
            threshold: witness_cfg.threshold,
            witnesses: witness_cfg.witnesses.len(),
        });
    }

    if profile == VcProfile::Kzg
        && params.require_kzg_trusted_setup
        && params.kzg_trusted_setup_id.is_none()
    {
        return Err(ParameterError::MissingKzgTrustedSetup);
    }
    if profile == VcProfile::Kzg
        && params.require_kzg_trusted_setup
        && params.kzg_trusted_setup_path.is_none()
    {
        return Err(ParameterError::MissingKzgTrustedSetupPath);
    }
    if profile == VcProfile::Kzg
        && params.require_kzg_trusted_setup
        && params.kzg_trusted_setup_attestation_sha512.is_none()
    {
        return Err(ParameterError::MissingKzgTrustedSetupAttestation);
    }

    validate_commitment_policy(profile, params.max_vector_len, &params.commitment_policy)
        .map_err(ParameterError::CommitmentPolicy)?;

    Ok(())
}

pub fn validate_reduction_contracts(
    contracts: &[ReductionContract],
) -> Result<(), SecurityPolicyError> {
    if contracts.is_empty() {
        return Err(SecurityPolicyError::MissingReductionContracts);
    }

    for c in contracts {
        if c.loss_bits > 128 {
            return Err(SecurityPolicyError::ReductionLossTooLarge {
                claim: c.claim.clone(),
                loss_bits: c.loss_bits,
            });
        }
        if c.max_queries == 0 {
            return Err(SecurityPolicyError::ReductionMaxQueriesZero {
                claim: c.claim.clone(),
            });
        }
    }
    Ok(())
}

pub fn validate_commit_shape(
    params: &ParameterSet,
    writes: usize,
    projected_state_len: usize,
) -> Result<(), RefinementError> {
    if writes > params.max_delta_writes {
        return Err(RefinementError::DeltaTooLarge {
            writes,
            max: params.max_delta_writes,
        });
    }
    if projected_state_len > params.max_vector_len {
        return Err(RefinementError::StateVectorTooLarge {
            len: projected_state_len,
            max: params.max_vector_len,
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn verify_post_commit_refinement(
    expected_height: u64,
    expected_prev_root: NodeHash,
    entry_height: u64,
    entry_prev_root: NodeHash,
    sth_size: u64,
    sth_root: NodeHash,
    sth_timestamp_unix_secs: u64,
    sth_sig: &str,
) -> Result<(), RefinementError> {
    if entry_height != expected_height {
        return Err(RefinementError::CommitHeightMismatch {
            expected: expected_height,
            got: entry_height,
        });
    }
    if entry_prev_root != expected_prev_root {
        return Err(RefinementError::PrevRootMismatch {
            expected: expected_prev_root,
            got: entry_prev_root,
        });
    }
    if sth_size != expected_height {
        return Err(RefinementError::SthSizeMismatch {
            expected: expected_height,
            got: sth_size,
        });
    }
    let expected_sig = sth_signature(sth_size, &sth_root, sth_timestamp_unix_secs);
    if sth_sig != expected_sig {
        return Err(RefinementError::InvalidSthSignature {
            expected: expected_sig,
            got: sth_sig.to_string(),
        });
    }
    Ok(())
}

/// Formal provenance for the security-parameter, reduction, and refinement layer.
pub fn formal_provenance() -> Vec<FormalProvenance> {
    vec![
        (
            "nucleus_combine_floor_bound",
            "HeytingLean.EpistemicCalculus.nucleus_combine_floor_bound",
            Some("HeytingLean.NucleusDB.Core.NucleusBridge.nucleus_combine_floor_bound"),
        ),
        (
            "vUpdate_chain_comm",
            "HeytingLean.EpistemicCalculus.Updating.vUpdate_chain_comm",
            Some("HeytingLean.NucleusDB.Core.NucleusBridge.vUpdate_chain_comm"),
        ),
        (
            "validFor_of_bounds",
            "HeytingLean.NucleusDB.Security.validFor_of_bounds",
            Some("HeytingLean.NucleusDB.Security.validFor_of_bounds"),
        ),
        (
            "validFor_kzg_requires_setup_id",
            "HeytingLean.NucleusDB.Security.validFor_kzg_requires_setup_id",
            Some("HeytingLean.NucleusDB.Security.validFor_kzg_requires_setup_id"),
        ),
        (
            "singleton_bundle_valid",
            "HeytingLean.NucleusDB.Security.singleton_bundle_valid",
            Some("HeytingLean.NucleusDB.Security.singleton_bundle_valid"),
        ),
        (
            "bundle_member_valid",
            "HeytingLean.NucleusDB.Security.bundle_member_valid",
            Some("HeytingLean.NucleusDB.Security.bundle_member_valid"),
        ),
        (
            "certificate_to_refinement",
            "HeytingLean.NucleusDB.Security.certificate_to_refinement",
            Some("HeytingLean.NucleusDB.Security.certificate_to_refinement"),
        ),
    ]
}
