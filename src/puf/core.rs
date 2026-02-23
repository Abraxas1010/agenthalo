use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PufTier {
    Dgx,
    ServerTpm,
    Server,
    Consumer,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PufComponent {
    pub name: String,
    pub value: Vec<u8>,
    pub entropy_bits: u32,
    pub stable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PufResult {
    pub fingerprint: [u8; 32],
    pub fingerprint_scheme: String,
    pub legacy_json_fingerprint: Option<[u8; 32]>,
    pub tier: PufTier,
    pub entropy_bits: u32,
    pub components: Vec<PufComponent>,
    pub timestamp_unix_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeResponse {
    pub response: [u8; 32],
    pub timestamp_unix_secs: u64,
    pub tier: PufTier,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifyResult {
    Match,
    Mismatch {
        hamming_distance: u32,
        threshold: u32,
    },
}

pub trait DevicePuf {
    fn collect() -> Option<PufResult>;
    fn challenge(nonce: &[u8]) -> Option<ChallengeResponse>;
    fn verify(reference: &[u8; 32], threshold: u32) -> Option<VerifyResult>;
    fn tier() -> PufTier;
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn canonical_fingerprint(components: &[PufComponent]) -> [u8; 32] {
    let mut sorted = components.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut h = Sha256::new();
    h.update(b"nucleusdb.puf.fingerprint.v1|");
    for c in &sorted {
        h.update(c.name.as_bytes());
        h.update([0u8]);
        h.update((c.entropy_bits as u64).to_le_bytes());
        h.update([u8::from(c.stable)]);
        h.update((c.value.len() as u64).to_le_bytes());
        h.update(&c.value);
    }
    h.finalize().into()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn decode_component_text(c: &PufComponent) -> String {
    std::str::from_utf8(&c.value)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| bytes_to_hex(&c.value))
}

fn js_replacer_filter(value: &Value, allowlist_sorted: &[String]) -> Value {
    match value {
        Value::Object(obj) => {
            let mut out = Map::new();
            for key in allowlist_sorted {
                if let Some(v) = obj.get(key) {
                    out.insert(key.clone(), js_replacer_filter(v, allowlist_sorted));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| js_replacer_filter(v, allowlist_sorted))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Build a compatibility JSON object matching the shape expected by `server/proof-binding/puf.js`.
pub fn puf_js_compat_components(components: &[PufComponent]) -> Value {
    let mut by_name = BTreeMap::<String, String>::new();
    for c in components {
        by_name.insert(c.name.clone(), decode_component_text(c));
    }

    let machine_id = by_name.get("machine_id").cloned();
    let gpu_uuid = by_name.get("gpu_uuid").cloned();
    let nvme_serial = by_name
        .get("dgx_nvme_serial")
        .cloned()
        .or_else(|| by_name.get("nvme_serial").cloned());

    let connectx = by_name
        .get("connectx_guids")
        .map(|s| {
            s.split('|')
                .filter(|e| !e.trim().is_empty())
                .map(|entry| {
                    let (device, guid) = entry
                        .split_once(':')
                        .map(|(a, b)| (a.to_string(), b.to_string()))
                        .unwrap_or_else(|| (entry.to_string(), String::new()));
                    let mut obj = Map::new();
                    obj.insert("device".to_string(), Value::String(device));
                    obj.insert("guid".to_string(), Value::String(guid));
                    Value::Object(obj)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let macs = by_name
        .get("stable_macs")
        .map(|s| {
            s.split('|')
                .filter(|e| !e.trim().is_empty())
                .map(|entry| {
                    let (iface, mac) = entry
                        .split_once(':')
                        .map(|(a, b)| (a.to_string(), b.to_string()))
                        .unwrap_or_else(|| (entry.to_string(), String::new()));
                    let mut obj = Map::new();
                    obj.insert("interface".to_string(), Value::String(iface));
                    obj.insert("mac".to_string(), Value::String(mac));
                    Value::Object(obj)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut thermal = Map::new();
    // The JS replacer list strips these nested fields, but include canonical keys
    // so the object shape mirrors puf.js.
    thermal.insert("baseline".to_string(), Value::Object(Map::new()));
    thermal.insert("variance".to_string(), Value::Object(Map::new()));
    thermal.insert(
        "sampleCount".to_string(),
        Value::Number(serde_json::Number::from(
            by_name
                .get("thermal_signature")
                .map(|_| 1u64)
                .unwrap_or(0u64),
        )),
    );

    let mut top = Map::new();
    top.insert(
        "machine_id".to_string(),
        machine_id.map(Value::String).unwrap_or(Value::Null),
    );
    top.insert(
        "gpu_uuid".to_string(),
        gpu_uuid.map(Value::String).unwrap_or(Value::Null),
    );
    top.insert("connectx_guids".to_string(), Value::Array(connectx));
    top.insert(
        "nvme_serial".to_string(),
        nvme_serial.map(Value::String).unwrap_or(Value::Null),
    );
    top.insert("mac_addresses".to_string(), Value::Array(macs));
    top.insert("thermal_signature".to_string(), Value::Object(thermal));
    Value::Object(top)
}

/// Compatibility hash matching `puf.js`:
/// `sha256(JSON.stringify(components, Object.keys(components).sort()))`.
pub fn puf_js_compat_fingerprint(components: &[PufComponent]) -> [u8; 32] {
    let value = puf_js_compat_components(components);
    let allowlist_sorted = value
        .as_object()
        .map(|m| {
            let mut keys = m.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            keys
        })
        .unwrap_or_default();
    let filtered = js_replacer_filter(&value, &allowlist_sorted);
    let payload = serde_json::to_vec(&filtered).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(payload);
    h.finalize().into()
}

pub fn build_result(tier: PufTier, components: Vec<PufComponent>) -> PufResult {
    let entropy_bits = components
        .iter()
        .fold(0u32, |acc, c| acc.saturating_add(c.entropy_bits));
    let fingerprint = canonical_fingerprint(&components);
    PufResult {
        fingerprint,
        fingerprint_scheme: "canonical_v2".to_string(),
        legacy_json_fingerprint: Some(puf_js_compat_fingerprint(&components)),
        tier,
        entropy_bits,
        components,
        timestamp_unix_secs: now_unix_secs(),
    }
}

pub fn challenge_response(
    tier: PufTier,
    fingerprint: &[u8; 32],
    nonce: &[u8],
) -> ChallengeResponse {
    let mut h = Sha256::new();
    h.update(b"nucleusdb.puf.challenge.v1|");
    h.update(fingerprint);
    h.update((nonce.len() as u64).to_le_bytes());
    h.update(nonce);
    h.update(now_unix_secs().to_le_bytes());
    ChallengeResponse {
        response: h.finalize().into(),
        timestamp_unix_secs: now_unix_secs(),
        tier,
    }
}

pub fn hamming_distance(a: &[u8; 32], b: &[u8; 32]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

pub fn verify_fingerprint(
    current: &[u8; 32],
    reference: &[u8; 32],
    threshold: u32,
) -> VerifyResult {
    let d = hamming_distance(current, reference);
    if d <= threshold {
        VerifyResult::Match
    } else {
        VerifyResult::Mismatch {
            hamming_distance: d,
            threshold,
        }
    }
}

impl PufTier {
    pub fn detect() -> Self {
        if std::path::Path::new("/sys/class/infiniband").exists()
            && std::path::Path::new("/sys/class/nvme/nvme0/serial").exists()
        {
            return Self::Dgx;
        }
        if std::path::Path::new("/sys/class/tpm/tpm0").exists() {
            return Self::ServerTpm;
        }
        if std::path::Path::new("/etc/machine-id").exists() {
            return Self::Server;
        }
        Self::Consumer
    }
}

pub fn collect_auto() -> Option<PufResult> {
    match PufTier::detect() {
        PufTier::Dgx => {
            #[cfg(feature = "puf-dgx")]
            {
                if let Some(v) = crate::puf::dgx::collect() {
                    return Some(v);
                }
            }
            #[cfg(feature = "puf-server")]
            {
                crate::puf::server::collect()
            }
            #[cfg(not(feature = "puf-server"))]
            {
                None
            }
        }
        PufTier::ServerTpm => {
            #[cfg(feature = "puf-tpm")]
            {
                if let Some(v) = crate::puf::tpm::collect() {
                    return Some(v);
                }
            }
            #[cfg(feature = "puf-server")]
            {
                crate::puf::server::collect()
            }
            #[cfg(not(feature = "puf-server"))]
            {
                None
            }
        }
        PufTier::Server => {
            #[cfg(feature = "puf-server")]
            {
                crate::puf::server::collect()
            }
            #[cfg(not(feature = "puf-server"))]
            {
                None
            }
        }
        PufTier::Consumer => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_stable_for_same_components() {
        let c = vec![PufComponent {
            name: "machine_id".to_string(),
            value: b"abc".to_vec(),
            entropy_bits: 16,
            stable: true,
        }];
        assert_eq!(canonical_fingerprint(&c), canonical_fingerprint(&c));
    }

    #[test]
    fn verify_threshold_works() {
        let a = [0u8; 32];
        let mut b = [0u8; 32];
        b[0] = 1;
        assert_eq!(verify_fingerprint(&a, &a, 0), VerifyResult::Match);
        match verify_fingerprint(&a, &b, 0) {
            VerifyResult::Mismatch { .. } => {}
            VerifyResult::Match => panic!("expected mismatch"),
        }
    }
}
