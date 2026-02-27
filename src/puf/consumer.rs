use crate::puf::core::{build_result, PufComponent, PufResult, PufTier};

#[cfg(feature = "puf-server")]
use crate::puf::server::timing_jitter_bytes;

#[cfg(not(feature = "puf-server"))]
fn timing_jitter_bytes() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    use std::time::Instant;

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
    let mut h = Sha256::new();
    h.update(b"nucleusdb.puf.jitter.v1|");
    for v in &samples {
        h.update(v.to_le_bytes());
    }
    h.finalize().into()
}

pub fn collect() -> Option<PufResult> {
    let mut components = Vec::new();

    // Cross-platform machine ID via `mid` crate (no admin required)
    if let Ok(id) = mid::get("agenthalo") {
        components.push(PufComponent {
            name: "machine_id".into(),
            value: id.into_bytes(),
            entropy_bits: 128,
            stable: true,
        });
    }

    // Cross-platform MAC address (no admin required)
    if let Ok(Some(addr)) = mac_address::get_mac_address() {
        components.push(PufComponent {
            name: "mac_address".into(),
            value: addr.to_string().into_bytes(),
            entropy_bits: 48,
            stable: true,
        });
    }

    // CPU core count (stdlib, works everywhere)
    if let Ok(n) = std::thread::available_parallelism() {
        components.push(PufComponent {
            name: "cpu_cores".into(),
            value: n.get().to_string().into_bytes(),
            entropy_bits: 4,
            stable: true,
        });
    }

    // OS + architecture
    components.push(PufComponent {
        name: "os".into(),
        value: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH).into_bytes(),
        entropy_bits: 8,
        stable: true,
    });

    // Timing jitter (pure computation, works everywhere)
    components.push(PufComponent {
        name: "timing_jitter".into(),
        value: timing_jitter_bytes().to_vec(),
        entropy_bits: 12,
        stable: false,
    });

    if components.is_empty() {
        return None;
    }
    Some(build_result(PufTier::Consumer, components))
}

#[cfg(test)]
mod tests {
    use super::collect;

    #[test]
    fn consumer_collect_returns_result() {
        let result = collect().expect("consumer PUF should collect baseline components");
        assert!(result.entropy_bits > 0);
        assert!(!result.components.is_empty());
    }
}
