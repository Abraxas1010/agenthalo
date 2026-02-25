use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn digest_json(domain: &str, value: &Value) -> Result<String, String> {
    let canonical = serde_json::to_vec(value).map_err(|e| format!("serialize payload: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update([0u8]);
    hasher.update(&canonical);
    let digest = hasher.finalize();
    Ok(hex_encode(&digest))
}

pub fn digest_bytes(domain: &str, payload: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update([0u8]);
    hasher.update(payload);
    let out = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&out);
    digest
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_decode_32(hex: &str) -> Result<[u8; 32], String> {
    let s = hex.strip_prefix("0x").unwrap_or(hex);
    if s.len() != 64 {
        return Err(format!("expected 32-byte hex (64 chars), got {}", s.len()));
    }
    let mut out = [0u8; 32];
    for (i, pair) in s.as_bytes().chunks_exact(2).enumerate() {
        let hi = hex_nibble(pair[0]).ok_or_else(|| format!("invalid hex in {hex}"))?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| format!("invalid hex in {hex}"))?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

pub fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    let s = hex.strip_prefix("0x").unwrap_or(hex);
    if !s.len().is_multiple_of(2) {
        return Err("hex input must have even length".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().chunks_exact(2) {
        let hi = hex_nibble(pair[0]).ok_or_else(|| format!("invalid hex in {hex}"))?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| format!("invalid hex in {hex}"))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

pub(crate) fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_decode_variable_length() {
        assert_eq!(hex_decode("").expect("empty"), Vec::<u8>::new());
        assert_eq!(hex_decode("00").expect("single"), vec![0u8]);
        assert_eq!(hex_decode("0x0aFF").expect("mixed"), vec![0x0a, 0xff]);
        assert_eq!(
            hex_decode("deadbeef").expect("bytes"),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
        assert!(hex_decode("abc").is_err());
        assert!(hex_decode("zz").is_err());
    }
}
