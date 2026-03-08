use bip32::{DerivationPath, XPrv};
use bip39::{Language, Mnemonic};
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use sha3::{Digest, Keccak256};

pub const DEFAULT_EVM_DERIVATION_PATH: &str = "m/44'/60'/0'/0/0";

#[derive(Clone, Debug)]
pub struct DerivedEvmWallet {
    pub derivation_path: String,
    pub evm_address: String,
    pub private_key_hex: String,
}

fn parse_signing_key(private_key_hex: &str) -> Result<SigningKey, String> {
    let hex_str = private_key_hex
        .strip_prefix("0x")
        .unwrap_or(private_key_hex);
    let key_bytes = hex::decode(hex_str).map_err(|e| format!("evm key hex decode: {e}"))?;
    SigningKey::from_bytes(key_bytes.as_slice().into()).map_err(|e| format!("evm signing key: {e}"))
}

fn evm_address_from_signing_key(signing_key: &SigningKey) -> Result<String, String> {
    let verifying = signing_key.verifying_key();
    evm_address_from_verifying_key(verifying)
}

fn evm_address_from_verifying_key(verifying: &VerifyingKey) -> Result<String, String> {
    let point = verifying.to_encoded_point(false);
    let pub_uncompressed = point.as_bytes();
    if pub_uncompressed.len() != 65 || pub_uncompressed[0] != 0x04 {
        return Err("unexpected secp256k1 public key encoding".to_string());
    }
    let digest = Keccak256::digest(&pub_uncompressed[1..]);
    Ok(format!("0x{}", hex::encode(&digest[12..])))
}

pub fn derive_from_mnemonic(
    mnemonic: &str,
    derivation_path: Option<&str>,
) -> Result<DerivedEvmWallet, String> {
    let phrase = mnemonic.trim();
    if phrase.is_empty() {
        return Err("mnemonic must not be empty".to_string());
    }
    let parsed = Mnemonic::parse_in_normalized(Language::English, phrase)
        .map_err(|e| format!("invalid mnemonic: {e}"))?;
    let seed = parsed.to_seed_normalized("");
    let path_raw = derivation_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_EVM_DERIVATION_PATH);
    let path: DerivationPath = path_raw
        .parse()
        .map_err(|e| format!("invalid derivation path {path_raw}: {e}"))?;
    let child = XPrv::derive_from_path(seed.as_slice(), &path)
        .map_err(|e| format!("derive path {path_raw}: {e}"))?;
    let private_key_bytes = child.private_key().to_bytes();
    let signing_key = SigningKey::from_bytes(&private_key_bytes)
        .map_err(|e| format!("k256 signing key from path {path_raw}: {e}"))?;
    let address = evm_address_from_signing_key(&signing_key)?;
    Ok(DerivedEvmWallet {
        derivation_path: path_raw.to_string(),
        evm_address: address,
        private_key_hex: format!("0x{}", hex::encode(private_key_bytes)),
    })
}

/// Sign a message with the EVM wallet's secp256k1 key.
/// `private_key_hex` is the "0x"-prefixed hex private key from `DerivedEvmWallet`.
pub(crate) fn sign_with_evm_key(private_key_hex: &str, message: &[u8]) -> Result<Vec<u8>, String> {
    let signing_key = parse_signing_key(private_key_hex)?;
    use k256::ecdsa::signature::Signer;
    let sig: k256::ecdsa::Signature = signing_key.sign(message);
    Ok(sig.to_bytes().to_vec())
}

/// Sign a message with a recoverable secp256k1 signature encoded as 65 bytes
/// (`r || s || v`) in hex, where `v` is the recovery id byte.
pub fn sign_recoverable_with_evm_key(
    private_key_hex: &str,
    message: &[u8],
) -> Result<String, String> {
    let signing_key = parse_signing_key(private_key_hex)?;
    let digest = Keccak256::new_with_prefix(message);
    let (signature, recovery_id) = signing_key
        .sign_digest_recoverable(digest)
        .map_err(|e| format!("evm recoverable sign: {e}"))?;
    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(signature.to_bytes().as_slice());
    out.push(recovery_id.to_byte());
    Ok(format!("0x{}", hex::encode(out)))
}

/// Verify a recoverable secp256k1 signature against the expected EVM address.
pub fn verify_recoverable_signature(
    expected_address: &str,
    message: &[u8],
    signature_hex: &str,
) -> Result<bool, String> {
    let sig_hex = signature_hex.trim().strip_prefix("0x").unwrap_or(signature_hex.trim());
    let bytes = hex::decode(sig_hex).map_err(|e| format!("evm signature hex decode: {e}"))?;
    if bytes.len() != 65 {
        return Err(format!(
            "recoverable signature must be 65 bytes, got {}",
            bytes.len()
        ));
    }

    let signature = Signature::try_from(&bytes[..64])
        .map_err(|e| format!("evm recoverable signature parse: {e}"))?;
    let recovery_byte = match bytes[64] {
        v @ 0..=3 => v,
        v @ 27..=30 => v - 27,
        v => {
            return Err(format!(
                "invalid EVM recovery id byte {v}; expected 0-3 or 27-30"
            ))
        }
    };
    let recovery_id = RecoveryId::try_from(recovery_byte)
        .map_err(|e| format!("evm recovery id parse: {e}"))?;
    let recovered = VerifyingKey::recover_from_digest(
        Keccak256::new_with_prefix(message),
        &signature,
        recovery_id,
    )
    .map_err(|e| format!("evm recover verifying key: {e}"))?;
    let recovered_address = evm_address_from_verifying_key(&recovered)?;
    Ok(recovered_address.eq_ignore_ascii_case(expected_address.trim()))
}

/// Derive the EVM address controlled by a secp256k1 private key.
pub fn evm_address_from_private_key(private_key_hex: &str) -> Result<String, String> {
    let signing_key = parse_signing_key(private_key_hex)?;
    evm_address_from_signing_key(&signing_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_is_deterministic_for_same_mnemonic() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let a = derive_from_mnemonic(mnemonic, None).expect("derive first");
        let b = derive_from_mnemonic(mnemonic, None).expect("derive second");
        assert_eq!(a.evm_address, b.evm_address);
        assert_eq!(a.private_key_hex, b.private_key_hex);
        assert_eq!(a.derivation_path, DEFAULT_EVM_DERIVATION_PATH);
    }

    #[test]
    fn derivation_changes_with_mnemonic() {
        let m1 = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m2 = "legal winner thank year wave sausage worth useful legal winner thank yellow";
        let w1 = derive_from_mnemonic(m1, None).expect("derive m1");
        let w2 = derive_from_mnemonic(m2, None).expect("derive m2");
        assert_ne!(w1.evm_address, w2.evm_address);
    }

    #[test]
    fn derived_wallet_has_expected_shapes() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let wallet = derive_from_mnemonic(mnemonic, None).expect("derive");
        assert!(wallet.evm_address.starts_with("0x"));
        assert_eq!(wallet.evm_address.len(), 42);
        assert!(wallet.private_key_hex.starts_with("0x"));
        assert_eq!(wallet.private_key_hex.len(), 66);
        assert_eq!(wallet.derivation_path, DEFAULT_EVM_DERIVATION_PATH);
    }

    #[test]
    fn private_key_roundtrips_to_same_address() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let wallet = derive_from_mnemonic(mnemonic, None).expect("derive");
        let derived =
            evm_address_from_private_key(&wallet.private_key_hex).expect("address from key");
        assert_eq!(derived, wallet.evm_address);
    }

    #[test]
    fn recoverable_signature_roundtrips_to_same_address() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let wallet = derive_from_mnemonic(mnemonic, None).expect("derive");
        let message = b"nucleusdb cab auth test";
        let signature =
            sign_recoverable_with_evm_key(&wallet.private_key_hex, message).expect("sign");
        let ok =
            verify_recoverable_signature(&wallet.evm_address, message, &signature).expect("verify");
        assert!(ok);
    }

    #[test]
    fn recoverable_signature_rejects_wrong_address() {
        let m1 = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m2 = "legal winner thank year wave sausage worth useful legal winner thank yellow";
        let wallet_a = derive_from_mnemonic(m1, None).expect("wallet a");
        let wallet_b = derive_from_mnemonic(m2, None).expect("wallet b");
        let signature = sign_recoverable_with_evm_key(&wallet_a.private_key_hex, b"payload")
            .expect("sign");
        let ok =
            verify_recoverable_signature(&wallet_b.evm_address, b"payload", &signature).expect("verify");
        assert!(!ok);
    }
}
