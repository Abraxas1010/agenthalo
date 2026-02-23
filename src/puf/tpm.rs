use crate::puf::core::{
    build_result, challenge_response, verify_fingerprint, ChallengeResponse, DevicePuf,
    PufComponent, PufResult, PufTier, VerifyResult,
};
use std::fs;
use std::path::Path;
use std::process::Command;

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn has_tpm() -> bool {
    Path::new("/sys/class/tpm/tpm0").exists()
}

#[cfg(feature = "puf-tpm")]
fn tss_esapi_probe() -> Option<String> {
    // Lightweight compile/runtime probe: confirms tss-esapi path is wired.
    // Full quote workflows remain available via tpm2-tools command probes below.
    let alg = tss_esapi::interface_types::algorithm::HashingAlgorithm::Sha256;
    Some(format!("tss-esapi:{alg:?}"))
}

#[cfg(not(feature = "puf-tpm"))]
fn tss_esapi_probe() -> Option<String> {
    None
}

fn run_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn collect() -> Option<PufResult> {
    if !has_tpm() {
        return None;
    }
    let mut components = Vec::new();
    if let Some(id) = read_trim("/etc/machine-id") {
        components.push(PufComponent {
            name: "machine_id".to_string(),
            value: id.into_bytes(),
            entropy_bits: 64,
            stable: true,
        });
    }
    if let Some(v) = read_trim("/sys/class/tpm/tpm0/tpm_version_major") {
        components.push(PufComponent {
            name: "tpm_version_major".to_string(),
            value: v.into_bytes(),
            entropy_bits: 8,
            stable: true,
        });
    }
    if let Some(desc) = read_trim("/sys/class/tpm/tpm0/device/description") {
        components.push(PufComponent {
            name: "tpm_description".to_string(),
            value: desc.into_bytes(),
            entropy_bits: 24,
            stable: true,
        });
    }
    if let Some(v) = tss_esapi_probe() {
        components.push(PufComponent {
            name: "tpm_tss_probe".to_string(),
            value: v.into_bytes(),
            entropy_bits: 4,
            stable: true,
        });
    }
    // Prefer real TPM attestation surfaces when tpm2-tools are available.
    if let Some(ek_pub) = run_cmd("tpm2_readpublic", &["-c", "endorsement", "-f", "pem"]) {
        components.push(PufComponent {
            name: "tpm_ek_public".to_string(),
            value: ek_pub.into_bytes(),
            entropy_bits: 96,
            stable: true,
        });
    }
    if let Some(pcrs) = run_cmd("tpm2_pcrread", &["sha256:0,1,2,3,4,5,6,7"]) {
        components.push(PufComponent {
            name: "tpm_pcr_bank_sha256".to_string(),
            value: pcrs.into_bytes(),
            entropy_bits: 64,
            stable: true,
        });
    }
    if let Some(rand) = run_cmd("tpm2_getrandom", &["16"]) {
        components.push(PufComponent {
            name: "tpm_getrandom_probe".to_string(),
            value: rand.into_bytes(),
            entropy_bits: 16,
            stable: false,
        });
    }
    if let Some(quote) = run_cmd(
        "tpm2_quote",
        &[
            "-Q",
            "-l",
            "sha256:0,1,2,3,4,5,6,7",
            "-q",
            "nucleusdb",
            "-m",
            "-",
        ],
    ) {
        components.push(PufComponent {
            name: "tpm_quote_probe".to_string(),
            value: quote.into_bytes(),
            entropy_bits: 48,
            stable: true,
        });
    }
    if components.is_empty() {
        return None;
    }
    Some(build_result(PufTier::ServerTpm, components))
}

pub struct TpmPuf;

impl DevicePuf for TpmPuf {
    fn collect() -> Option<PufResult> {
        collect()
    }

    fn challenge(nonce: &[u8]) -> Option<ChallengeResponse> {
        let p = collect()?;
        Some(challenge_response(
            PufTier::ServerTpm,
            &p.fingerprint,
            nonce,
        ))
    }

    fn verify(reference: &[u8; 32], threshold: u32) -> Option<VerifyResult> {
        let p = collect()?;
        Some(verify_fingerprint(&p.fingerprint, reference, threshold))
    }

    fn tier() -> PufTier {
        PufTier::ServerTpm
    }
}
