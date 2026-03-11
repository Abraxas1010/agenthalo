use crate::puf::core::{
    build_result, challenge_response, verify_fingerprint, ChallengeResponse, DevicePuf,
    PufComponent, PufResult, PufTier, VerifyResult,
};
use crate::puf::server;
use std::fs;
use std::thread;
use std::time::Duration;

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn connectx_guids() -> Vec<String> {
    let mut out = Vec::new();
    let entries = match fs::read_dir("/sys/class/infiniband") {
        Ok(v) => v,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let dev = entry.file_name().to_string_lossy().to_string();
        let path = format!("/sys/class/infiniband/{dev}/node_guid");
        if let Some(guid) = read_trim(&path) {
            out.push(format!("{dev}:{guid}"));
        }
    }
    out.sort();
    out
}

fn nvme_serial() -> Option<String> {
    read_trim("/sys/class/nvme/nvme0/serial")
}

fn thermal_zone_snapshot() -> Vec<(String, i64)> {
    let mut samples = Vec::new();
    let Ok(hwmons) = fs::read_dir("/sys/class/hwmon") else {
        return samples;
    };
    for hw in hwmons.flatten() {
        let base = hw.path();
        let name = fs::read_to_string(base.join("name"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let Ok(entries) = fs::read_dir(&base) else {
            continue;
        };
        for e in entries.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if !fname.starts_with("temp") || !fname.ends_with("_input") {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(e.path()) {
                if let Ok(v) = raw.trim().parse::<i64>() {
                    samples.push((format!("{name}:{fname}"), v));
                }
            }
        }
    }
    samples.sort_by(|a, b| a.0.cmp(&b.0));
    samples
}

fn thermal_signature_component() -> Option<PufComponent> {
    let mut frames = Vec::new();
    for _ in 0..5 {
        let snap = thermal_zone_snapshot();
        if !snap.is_empty() {
            frames.push(snap);
        }
        thread::sleep(Duration::from_millis(100));
    }
    if frames.is_empty() {
        return None;
    }
    let mut blob = String::new();
    for (i, frame) in frames.iter().enumerate() {
        blob.push_str(&format!("f{i}:"));
        for (zone, value) in frame {
            blob.push_str(zone);
            blob.push('=');
            blob.push_str(&value.to_string());
            blob.push(';');
        }
    }
    Some(PufComponent {
        name: "thermal_signature".to_string(),
        value: blob.into_bytes(),
        entropy_bits: 32,
        stable: false,
    })
}

pub fn collect() -> Option<PufResult> {
    if !server::likely_dgx_host() {
        return None;
    }
    let mut components = server::collect_components();
    let guids = connectx_guids();
    if !guids.is_empty() {
        components.push(PufComponent {
            name: "connectx_guids".to_string(),
            value: guids.join("|").into_bytes(),
            entropy_bits: 64,
            stable: true,
        });
    }
    if let Some(serial) = nvme_serial() {
        components.push(PufComponent {
            name: "dgx_nvme_serial".to_string(),
            value: serial.into_bytes(),
            entropy_bits: 48,
            stable: true,
        });
    }
    if let Some(thermal) = thermal_signature_component() {
        components.push(thermal);
    }
    Some(build_result(PufTier::Dgx, components))
}

pub struct DgxPuf;

impl DevicePuf for DgxPuf {
    fn collect() -> Option<PufResult> {
        collect()
    }

    fn challenge(nonce: &[u8]) -> Option<ChallengeResponse> {
        let p = collect()?;
        Some(challenge_response(PufTier::Dgx, &p.fingerprint, nonce))
    }

    fn verify(reference: &[u8; 32], threshold: u32) -> Option<VerifyResult> {
        let p = collect()?;
        Some(verify_fingerprint(&p.fingerprint, reference, threshold))
    }

    fn tier() -> PufTier {
        PufTier::Dgx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puf_dgx_collects_when_available() {
        if server::likely_dgx_host() {
            let v = collect();
            assert!(v.is_some());
        }
    }
}
