//! PQ-gated EVM transaction signing.
//!
//! Every EVM signature request must be authorized by the agent's DID identity
//! (Ed25519 + ML-DSA-65) before secp256k1 signing is allowed.

use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use crate::halo::evm_wallet;
use crate::halo::util;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

const EVM_GATE_DOMAIN: &str = "agenthalo.evm_gate.v1";
pub const EVM_GATE_FORMAL_BASIS: &str =
    "HeytingLean.NucleusDB.Crypto.EVMGate.evm_sign_requires_dual_auth";
/// Runtime-local mirror theorem for the nucleusdb gate state machine.
pub const EVM_GATE_FORMAL_BASIS_LOCAL: &str =
    "HeytingLean.NucleusDB.Comms.Identity.EVMGate.evm_sign_requires_dual_auth";

/// Canonical/local theorem-path pair for PQ-gated EVM signing.
pub fn evm_gate_formal_provenance() -> (&'static str, &'static str) {
    (EVM_GATE_FORMAL_BASIS, EVM_GATE_FORMAL_BASIS_LOCAL)
}

/// Authorization request — the agent intends to sign this EVM payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvmSigningRequest {
    /// Raw message bytes to be signed by secp256k1.
    pub message: Vec<u8>,
    /// The EVM address that will sign (must match the signing key).
    pub evm_address: String,
    /// Unix timestamp of the request.
    pub requested_at: u64,
    /// Nonce to prevent replay.
    pub nonce: u64,
}

/// Authorization proof — dual-signed by the agent's DID identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvmSigningAuthorization {
    /// The signing request being authorized.
    pub request: EvmSigningRequest,
    /// Ed25519 signature over canonical(request).
    pub ed25519_signature: Vec<u8>,
    /// ML-DSA-65 signature over canonical(request).
    pub mldsa65_signature: Vec<u8>,
}

fn sha512_hex(data: &[u8]) -> String {
    util::hex_encode(Sha512::digest(data).as_slice())
}

fn canonical_request_bytes(request: &EvmSigningRequest) -> Vec<u8> {
    format!(
        "{}|addr={}|nonce={}|ts={}|msg_sha512={}",
        EVM_GATE_DOMAIN,
        request.evm_address.to_lowercase(),
        request.nonce,
        request.requested_at,
        sha512_hex(&request.message),
    )
    .into_bytes()
}

fn validate_request_key_binding(
    request: &EvmSigningRequest,
    evm_private_key_hex: &str,
) -> Result<(), String> {
    let derived = evm_wallet::evm_address_from_private_key(evm_private_key_hex)?;
    if derived.eq_ignore_ascii_case(&request.evm_address) {
        Ok(())
    } else {
        Err(format!(
            "EVM signing key/address mismatch: request={}, key={}",
            request.evm_address, derived
        ))
    }
}

/// Verify authorization, then sign with secp256k1.
pub fn sign_evm_gated(
    authorization: &EvmSigningAuthorization,
    did_document: &DIDDocument,
    evm_private_key_hex: &str,
) -> Result<Vec<u8>, String> {
    validate_request_key_binding(&authorization.request, evm_private_key_hex)?;
    let canonical = canonical_request_bytes(&authorization.request);
    let verified = dual_verify(
        did_document,
        &canonical,
        &authorization.ed25519_signature,
        &authorization.mldsa65_signature,
    )?;
    if !verified {
        return Err("EVM gate authorization signature verification failed".to_string());
    }
    evm_wallet::sign_with_evm_key(evm_private_key_hex, &authorization.request.message)
}

fn authorize_and_sign_at(
    identity: &DIDIdentity,
    evm_private_key_hex: &str,
    evm_address: &str,
    message: &[u8],
    nonce: u64,
    requested_at: u64,
) -> Result<(EvmSigningAuthorization, Vec<u8>), String> {
    let request = EvmSigningRequest {
        message: message.to_vec(),
        evm_address: evm_address.to_string(),
        requested_at,
        nonce,
    };
    let canonical = canonical_request_bytes(&request);
    let (ed25519_signature, mldsa65_signature) = dual_sign(identity, &canonical)?;
    let authorization = EvmSigningAuthorization {
        request,
        ed25519_signature,
        mldsa65_signature,
    };
    let signature = sign_evm_gated(&authorization, &identity.did_document, evm_private_key_hex)?;
    Ok((authorization, signature))
}

/// Create authorization and immediately sign the EVM payload.
pub fn authorize_and_sign(
    identity: &DIDIdentity,
    evm_private_key_hex: &str,
    evm_address: &str,
    message: &[u8],
    nonce: u64,
) -> Result<(EvmSigningAuthorization, Vec<u8>), String> {
    authorize_and_sign_at(
        identity,
        evm_private_key_hex,
        evm_address,
        message,
        nonce,
        util::now_unix_secs(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    fn wallet_from_seed(seed: &[u8; 64]) -> crate::halo::evm_wallet::DerivedEvmWallet {
        let entropy = crate::halo::genesis_seed::derive_wallet_entropy32_from_seed_public(seed)
            .expect("wallet entropy");
        let mnemonic =
            bip39::Mnemonic::from_entropy_in(bip39::Language::English, &entropy).expect("mnemonic");
        crate::halo::evm_wallet::derive_from_mnemonic(&mnemonic.to_string(), None).expect("wallet")
    }

    #[test]
    fn authorize_and_sign_roundtrip() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x31)).expect("identity");
        let wallet = wallet_from_seed(&seed(0x31));
        let message = b"evm payload";

        let (authorization, secp_sig) = authorize_and_sign(
            &identity,
            &wallet.private_key_hex,
            &wallet.evm_address,
            message,
            7,
        )
        .expect("authorize and sign");
        assert_eq!(authorization.request.message, message);
        assert!(!secp_sig.is_empty());
    }

    #[test]
    fn gated_sign_rejects_bad_ed25519_sig() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x32)).expect("identity");
        let wallet = wallet_from_seed(&seed(0x32));
        let (mut authorization, _) = authorize_and_sign_at(
            &identity,
            &wallet.private_key_hex,
            &wallet.evm_address,
            b"payload",
            1,
            1_700_000_000,
        )
        .expect("base authorization");
        authorization.ed25519_signature[0] ^= 0x01;
        let err = sign_evm_gated(
            &authorization,
            &identity.did_document,
            &wallet.private_key_hex,
        )
        .expect_err("bad ed25519 must fail");
        assert!(err.contains("verification") || err.contains("ed25519"));
    }

    #[test]
    fn gated_sign_rejects_bad_mldsa65_sig() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x33)).expect("identity");
        let wallet = wallet_from_seed(&seed(0x33));
        let (mut authorization, _) = authorize_and_sign_at(
            &identity,
            &wallet.private_key_hex,
            &wallet.evm_address,
            b"payload",
            2,
            1_700_000_001,
        )
        .expect("base authorization");
        authorization.mldsa65_signature[0] ^= 0x01;
        let err = sign_evm_gated(
            &authorization,
            &identity.did_document,
            &wallet.private_key_hex,
        )
        .expect_err("bad mldsa65 must fail");
        assert!(err.contains("verification") || err.contains("ML-DSA-65"));
    }

    #[test]
    fn gated_sign_rejects_wrong_evm_address() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x34)).expect("identity");
        let wallet_a = wallet_from_seed(&seed(0x34));
        let wallet_b = wallet_from_seed(&seed(0x35));
        let (authorization, _) = authorize_and_sign_at(
            &identity,
            &wallet_a.private_key_hex,
            &wallet_a.evm_address,
            b"payload",
            3,
            1_700_000_002,
        )
        .expect("base authorization");
        let err = sign_evm_gated(
            &authorization,
            &identity.did_document,
            &wallet_b.private_key_hex,
        )
        .expect_err("wrong key must fail");
        assert!(err.contains("mismatch"));
    }

    #[test]
    fn gated_sign_rejects_unsigned_request() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x36)).expect("identity");
        let wallet = wallet_from_seed(&seed(0x36));
        let authorization = EvmSigningAuthorization {
            request: EvmSigningRequest {
                message: b"unsigned".to_vec(),
                evm_address: wallet.evm_address.clone(),
                requested_at: 1_700_000_003,
                nonce: 4,
            },
            ed25519_signature: Vec::new(),
            mldsa65_signature: Vec::new(),
        };
        let err = sign_evm_gated(
            &authorization,
            &identity.did_document,
            &wallet.private_key_hex,
        )
        .expect_err("unsigned request must fail");
        assert!(err.contains("signature"));
    }

    #[test]
    fn authorize_and_sign_deterministic() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x37)).expect("identity");
        let wallet = wallet_from_seed(&seed(0x37));
        let a = authorize_and_sign_at(
            &identity,
            &wallet.private_key_hex,
            &wallet.evm_address,
            b"deterministic",
            9,
            1_700_000_004,
        )
        .expect("first");
        let b = authorize_and_sign_at(
            &identity,
            &wallet.private_key_hex,
            &wallet.evm_address,
            b"deterministic",
            9,
            1_700_000_004,
        )
        .expect("second");
        assert_eq!(
            serde_json::to_vec(&a.0).expect("serialize"),
            serde_json::to_vec(&b.0).expect("serialize")
        );
        assert_eq!(a.1, b.1);
    }
}
