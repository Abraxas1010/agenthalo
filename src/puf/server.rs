use crate::puf::core::{
    build_result, challenge_response, verify_fingerprint, ChallengeResponse, DevicePuf,
    PufComponent, PufResult, PufTier, VerifyResult,
};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

const STABLE_IFACE_PREFIXES: &[&str] = &["en", "eth", "ib", "wl", "ww", "bond"];

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_machine_id() -> Option<String> {
    read_trim("/etc/machine-id")
}

fn read_cpu_model() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    text.lines().find_map(|line| {
        line.strip_prefix("model name\t: ")
            .map(|v| v.trim().to_string())
    })
}

fn read_gpu_uuid() -> Option<String> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=uuid", "--format=csv,noheader"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .and_then(|s| s.lines().next().map(|v| v.trim().to_string()))
}

fn stable_mac_addresses() -> Vec<String> {
    let mut macs = Vec::new();
    let entries = match fs::read_dir("/sys/class/net") {
        Ok(v) => v,
        Err(_) => return macs,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "lo" || !STABLE_IFACE_PREFIXES.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let path = format!("/sys/class/net/{name}/address");
        if let Some(mac) = read_trim(&path) {
            if mac != "00:00:00:00:00:00" {
                macs.push(format!("{name}:{mac}"));
            }
        }
    }
    macs.sort();
    macs
}

fn disk_serials() -> Vec<String> {
    let mut serials = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/block") {
        for entry in entries.flatten() {
            let dev = entry.file_name().to_string_lossy().to_string();
            for candidate in [
                format!("/sys/class/block/{dev}/device/serial"),
                format!("/sys/block/{dev}/device/serial"),
            ] {
                if let Some(s) = read_trim(&candidate) {
                    if !s.is_empty() {
                        serials.push(format!("{dev}:{s}"));
                        break;
                    }
                }
            }
        }
    }
    serials.sort();
    serials
}

fn nvme_serials() -> Vec<String> {
    let mut serials = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/class/nvme") {
        for entry in entries.flatten() {
            let dev = entry.file_name().to_string_lossy().to_string();
            let path = format!("/sys/class/nvme/{dev}/serial");
            if let Some(v) = read_trim(&path) {
                if !v.is_empty() {
                    serials.push(format!("{dev}:{v}"));
                }
            }
        }
    }
    serials.sort();
    serials
}

pub(crate) fn timing_jitter_bytes() -> [u8; 32] {
    let mut samples = [0u128; 64];
    for item in &mut samples {
        let start = Instant::now();
        let mut sink = 0u64;
        for i in 0..5000u64 {
            sink = sink.wrapping_add(i ^ 0x5A5A_A5A5);
        }
        let _ = sink;
        *item = start.elapsed().as_nanos();
    }
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    h.update(b"nucleusdb.puf.jitter.v1|");
    for v in &samples {
        h.update(v.to_le_bytes());
    }
    h.finalize().into()
}

pub(crate) fn collect_components() -> Vec<PufComponent> {
    let mut out = Vec::new();
    if let Some(id) = read_machine_id() {
        out.push(PufComponent {
            name: "machine_id".to_string(),
            value: id.into_bytes(),
            entropy_bits: 64,
            stable: true,
        });
    }
    if let Some(cpu) = read_cpu_model() {
        out.push(PufComponent {
            name: "cpu_model".to_string(),
            value: cpu.into_bytes(),
            entropy_bits: 20,
            stable: true,
        });
    }
    if let Some(gpu) = read_gpu_uuid() {
        out.push(PufComponent {
            name: "gpu_uuid".to_string(),
            value: gpu.into_bytes(),
            entropy_bits: 64,
            stable: true,
        });
    }

    let macs = stable_mac_addresses().join("|");
    if !macs.is_empty() {
        out.push(PufComponent {
            name: "stable_macs".to_string(),
            value: macs.into_bytes(),
            entropy_bits: 48,
            stable: true,
        });
    }

    let serials = disk_serials().join("|");
    if !serials.is_empty() {
        out.push(PufComponent {
            name: "disk_serials".to_string(),
            value: serials.into_bytes(),
            entropy_bits: 48,
            stable: true,
        });
    }
    let nvmes = nvme_serials().join("|");
    if !nvmes.is_empty() {
        out.push(PufComponent {
            name: "nvme_serial".to_string(),
            value: nvmes.into_bytes(),
            entropy_bits: 32,
            stable: true,
        });
    }

    out.push(PufComponent {
        name: "timing_jitter".to_string(),
        value: timing_jitter_bytes().to_vec(),
        entropy_bits: 12,
        stable: false,
    });
    out
}

pub fn collect() -> Option<PufResult> {
    let components = collect_components();
    if components.is_empty() {
        return None;
    }
    Some(build_result(PufTier::Server, components))
}

pub struct ServerPuf;

impl DevicePuf for ServerPuf {
    fn collect() -> Option<PufResult> {
        collect()
    }

    fn challenge(nonce: &[u8]) -> Option<ChallengeResponse> {
        let p = collect()?;
        Some(challenge_response(PufTier::Server, &p.fingerprint, nonce))
    }

    fn verify(reference: &[u8; 32], threshold: u32) -> Option<VerifyResult> {
        let p = collect()?;
        Some(verify_fingerprint(&p.fingerprint, reference, threshold))
    }

    fn tier() -> PufTier {
        PufTier::Server
    }
}

pub fn likely_dgx_host() -> bool {
    Path::new("/sys/class/infiniband").exists()
        && Path::new("/sys/class/nvme/nvme0/serial").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puf_server_collect_has_fingerprint() {
        if let Some(r) = collect() {
            assert!(r.entropy_bits > 0);
            assert_ne!(r.fingerprint, [0u8; 32]);
        }
    }
}
