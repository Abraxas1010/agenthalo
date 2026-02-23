//! Browser-side wasm module in this crate is a host test stub only.
//! The production browser implementation is `projects/nucleusdb/puf-browser`.
use crate::puf::core::{
    challenge_response, now_unix_secs, verify_fingerprint, ChallengeResponse, DevicePuf,
    PufComponent, PufResult, PufTier, VerifyResult,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;

pub fn collect() -> Option<PufResult> {
    // Host fallback for non-wasm builds. Mirrors puf-browser/index.js v1 hashing.
    let ua = env::var("NUCLEUSDB_BROWSER_UA").ok()?;
    let mut map = BTreeMap::<String, String>::new();
    map.insert("userAgent".to_string(), ua.clone());
    map.insert(
        "platform".to_string(),
        env::var("NUCLEUSDB_BROWSER_PLATFORM").unwrap_or_default(),
    );
    map.insert(
        "language".to_string(),
        env::var("NUCLEUSDB_BROWSER_LANGUAGE").unwrap_or_default(),
    );
    map.insert(
        "screen".to_string(),
        env::var("NUCLEUSDB_BROWSER_SCREEN").unwrap_or_default(),
    );
    map.insert(
        "timezone".to_string(),
        env::var("NUCLEUSDB_BROWSER_TZ").unwrap_or_default(),
    );

    let payload = serde_json::to_vec(&map).ok()?;
    let mut h = Sha256::new();
    h.update(payload);
    let fingerprint: [u8; 32] = h.finalize().into();
    let components = vec![PufComponent {
        name: "browser_payload".to_string(),
        value: ua.into_bytes(),
        entropy_bits: 8,
        stable: false,
    }];
    Some(PufResult {
        fingerprint,
        fingerprint_scheme: "browser_js_v1".to_string(),
        legacy_json_fingerprint: None,
        tier: PufTier::Consumer,
        entropy_bits: 8,
        components,
        timestamp_unix_secs: now_unix_secs(),
    })
}

pub struct BrowserPuf;

impl DevicePuf for BrowserPuf {
    fn collect() -> Option<PufResult> {
        collect()
    }

    fn challenge(nonce: &[u8]) -> Option<ChallengeResponse> {
        let p = collect()?;
        Some(challenge_response(PufTier::Consumer, &p.fingerprint, nonce))
    }

    fn verify(reference: &[u8; 32], threshold: u32) -> Option<VerifyResult> {
        let p = collect()?;
        Some(verify_fingerprint(&p.fingerprint, reference, threshold))
    }

    fn tier() -> PufTier {
        PufTier::Consumer
    }
}
