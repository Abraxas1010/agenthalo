use crate::halo::nym;
use crate::license::{compliance_inputs_from_pcn_witness, CompliancePublicInputs};
use crate::pcn::compliance_witness;
use crate::protocol::NucleusDb;
use crate::puf::{collect_auto, PufTier};
use crate::transparency::ct6962::hex_encode;
use std::fmt::{Display, Formatter};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum TrustBridgeError {
    PufUnavailable,
    InvalidTier(u8),
    CastNotFound,
    CastCommandFailed(String),
    Parse(String),
    MissingPrivateKeyEnv(String),
}

impl Display for TrustBridgeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PufUnavailable => {
                write!(f, "PUF unavailable; cannot prepare attestation payload")
            }
            Self::InvalidTier(t) => write!(f, "invalid tier {t}; expected 1..4"),
            Self::CastNotFound => write!(f, "`cast` command not found"),
            Self::CastCommandFailed(msg) => write!(f, "cast command failed: {msg}"),
            Self::Parse(msg) => write!(f, "failed to parse cast output: {msg}"),
            Self::MissingPrivateKeyEnv(name) => write!(
                f,
                "missing private key env var `{name}` required for on-chain submission"
            ),
        }
    }
}

impl std::error::Error for TrustBridgeError {}

#[derive(Clone, Debug)]
pub struct PreparedAttestation {
    pub puf_digest: [u8; 32],
    pub puf_digest_hex: String,
    pub puf_tier: u8,
    pub puf_tier_label: String,
    pub replay_seq: u64,
    pub feasibility_root_hex: String,
    pub public_signals: Vec<String>,
    pub compliance_inputs: CompliancePublicInputs,
}

#[derive(Clone, Debug)]
pub struct AgentOnchainStatus {
    pub verified: bool,
    pub active: Option<bool>,
    pub puf_digest: Option<String>,
    pub tier: Option<u8>,
    pub last_attestation: Option<u64>,
    pub last_replay_seq: Option<u64>,
    pub raw_verify: String,
    pub raw_status: String,
}

#[derive(Clone, Debug)]
pub enum CastSigner {
    PrivateKey(String),
    Keystore {
        path: String,
        password_file: Option<String>,
    },
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn puf_tier_to_u8(tier: PufTier) -> u8 {
    match tier {
        PufTier::Consumer => 1,
        PufTier::Server => 2,
        PufTier::ServerTpm => 3,
        PufTier::Dgx => 4,
    }
}

pub fn puf_tier_label(tier: u8) -> &'static str {
    match tier {
        1 => "consumer",
        2 => "server",
        3 => "server_tpm",
        4 => "dgx",
        _ => "unknown",
    }
}

pub fn prepare_attestation_bundle(
    db: &NucleusDb,
    tier_override: Option<u8>,
) -> Result<PreparedAttestation, TrustBridgeError> {
    let puf = collect_auto().ok_or(TrustBridgeError::PufUnavailable)?;
    let puf_digest = puf.fingerprint;
    let tier = match tier_override {
        Some(v @ 1..=4) => v,
        Some(v) => return Err(TrustBridgeError::InvalidTier(v)),
        None => puf_tier_to_u8(puf.tier),
    };

    let witness = compliance_witness(db);
    let compliance_inputs = compliance_inputs_from_pcn_witness(&witness, Some(puf_digest));
    let public_signals = encode_public_signals(&puf_digest, tier, compliance_inputs.replay_seq);

    Ok(PreparedAttestation {
        puf_digest,
        puf_digest_hex: hex_encode(&puf_digest),
        puf_tier: tier,
        puf_tier_label: puf_tier_label(tier).to_string(),
        replay_seq: compliance_inputs.replay_seq,
        feasibility_root_hex: compliance_inputs.feasibility_root.clone(),
        public_signals,
        compliance_inputs,
    })
}

pub fn encode_public_signals(puf_digest: &[u8; 32], tier: u8, replay_seq: u64) -> Vec<String> {
    let mut out = Vec::with_capacity(6);
    for chunk in puf_digest.chunks(8).take(4) {
        let mut limb = [0u8; 8];
        limb.copy_from_slice(chunk);
        out.push(u64::from_le_bytes(limb).to_string());
    }
    out.push(tier.to_string());
    out.push(replay_seq.to_string());
    out
}

pub fn cast_array(signals: &[String]) -> String {
    format!("[{}]", signals.join(","))
}

pub fn build_attest_command_preview(
    contract_address: &str,
    rpc_url: &str,
    proof_hex: &str,
    public_signals: &[String],
    private_key_env: &str,
) -> String {
    build_attest_command_preview_with_private_key_env(
        contract_address,
        rpc_url,
        proof_hex,
        public_signals,
        private_key_env,
    )
}

pub fn build_attest_command_preview_with_private_key_env(
    contract_address: &str,
    rpc_url: &str,
    proof_hex: &str,
    public_signals: &[String],
    private_key_env: &str,
) -> String {
    format!(
        "cast send --async --rpc-url {rpc_url} --private-key ${private_key_env} {contract_address} \"attestAndPay(bytes,uint256[])\" {proof_hex} '{}'",
        cast_array(public_signals)
    )
}

pub fn build_attest_command_preview_with_keystore(
    contract_address: &str,
    rpc_url: &str,
    proof_hex: &str,
    public_signals: &[String],
    keystore_path: &str,
    keystore_password_file: Option<&str>,
) -> String {
    let mut cmd = format!("cast send --async --rpc-url {rpc_url} --keystore {keystore_path}",);
    if let Some(path) = keystore_password_file {
        cmd.push_str(&format!(" --password-file {path}"));
    }
    cmd.push_str(&format!(
        " {contract_address} \"attestAndPay(bytes,uint256[])\" {proof_hex} '{}'",
        cast_array(public_signals)
    ));
    cmd
}

pub fn send_attestation(
    rpc_url: &str,
    contract_address: &str,
    proof_hex: &str,
    public_signals: &[String],
    signer: &CastSigner,
) -> Result<Option<String>, TrustBridgeError> {
    let mut args = vec![
        "send".to_string(),
        "--async".to_string(),
        "--rpc-url".to_string(),
        rpc_url.to_string(),
    ];
    match signer {
        CastSigner::PrivateKey(v) => {
            args.push("--private-key".to_string());
            args.push(v.clone());
        }
        CastSigner::Keystore {
            path,
            password_file,
        } => {
            args.push("--keystore".to_string());
            args.push(path.clone());
            if let Some(p) = password_file {
                args.push("--password-file".to_string());
                args.push(p.clone());
            }
        }
    }
    args.extend_from_slice(&[
        contract_address.to_string(),
        "attestAndPay(bytes,uint256[])".to_string(),
        proof_hex.to_string(),
        cast_array(public_signals),
    ]);
    let out = run_cast(&args)?;
    Ok(extract_hash(&out))
}

pub fn verify_agent_onchain(
    rpc_url: &str,
    contract_address: &str,
    agent_address: &str,
) -> Result<AgentOnchainStatus, TrustBridgeError> {
    let verify_out = run_cast(&[
        "call".to_string(),
        "--rpc-url".to_string(),
        rpc_url.to_string(),
        contract_address.to_string(),
        "verifyAgent(address)(bool)".to_string(),
        agent_address.to_string(),
    ])?;
    let verified = parse_bool_output(&verify_out)?;

    let status_out = run_cast(&[
        "call".to_string(),
        "--rpc-url".to_string(),
        rpc_url.to_string(),
        contract_address.to_string(),
        "agentStatus(address)(bool,bytes32,uint8,uint64,uint64)".to_string(),
        agent_address.to_string(),
    ])?;
    let parsed = parse_agent_status_output(&status_out);

    Ok(AgentOnchainStatus {
        verified,
        active: parsed.active,
        puf_digest: parsed.puf_digest,
        tier: parsed.tier,
        last_attestation: parsed.last_attestation,
        last_replay_seq: parsed.last_replay_seq,
        raw_verify: verify_out,
        raw_status: status_out,
    })
}

pub fn load_private_key_env(name: &str) -> Result<String, TrustBridgeError> {
    std::env::var(name).map_err(|_| TrustBridgeError::MissingPrivateKeyEnv(name.to_string()))
}

fn run_cast(args: &[String]) -> Result<String, TrustBridgeError> {
    let mut cmd = Command::new("cast");
    cmd.args(args);
    nym::apply_proxy_env_for_cast(&mut cmd, args).map_err(TrustBridgeError::CastCommandFailed)?;
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            TrustBridgeError::CastNotFound
        } else {
            TrustBridgeError::CastCommandFailed(e.to_string())
        }
    })?;

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(TrustBridgeError::CastCommandFailed(format!(
            "status={} stdout=`{}` stderr=`{}`",
            out.status, stdout, stderr
        )));
    }
    Ok(if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{stdout}\n{stderr}")
    })
}

fn parse_bool_output(raw: &str) -> Result<bool, TrustBridgeError> {
    let t = raw.trim().to_ascii_lowercase();
    if t.contains("true") {
        return Ok(true);
    }
    if t.contains("false") {
        return Ok(false);
    }
    if let Some(hex) = t.strip_prefix("0x") {
        let nz = hex.chars().any(|c| c != '0');
        return Ok(nz);
    }
    if let Ok(v) = t.parse::<u64>() {
        return Ok(v != 0);
    }
    Err(TrustBridgeError::Parse(format!(
        "boolean result expected, got `{raw}`"
    )))
}

fn extract_hash(raw: &str) -> Option<String> {
    raw.split(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '(' | ')' | '"' | '\'' | ';'))
        .find(|tok| {
            tok.len() == 66
                && tok.starts_with("0x")
                && tok[2..].chars().all(|c| c.is_ascii_hexdigit())
        })
        .map(|s| s.to_string())
}

#[derive(Default)]
struct ParsedAgentStatus {
    active: Option<bool>,
    puf_digest: Option<String>,
    tier: Option<u8>,
    last_attestation: Option<u64>,
    last_replay_seq: Option<u64>,
}

fn parse_agent_status_output(raw: &str) -> ParsedAgentStatus {
    let mut parsed = ParsedAgentStatus::default();
    let normalized = raw.replace(['(', ')', ',', '\n'], " ");
    let mut nums = Vec::new();
    for tok in normalized.split_whitespace() {
        let lower = tok.to_ascii_lowercase();
        if parsed.active.is_none() && (lower == "true" || lower == "false") {
            parsed.active = Some(lower == "true");
            continue;
        }
        if parsed.puf_digest.is_none()
            && tok.len() == 66
            && tok.starts_with("0x")
            && tok[2..].chars().all(|c| c.is_ascii_hexdigit())
        {
            parsed.puf_digest = Some(tok.to_string());
            continue;
        }
        if let Ok(v) = tok.parse::<u64>() {
            nums.push(v);
        }
    }
    if parsed.tier.is_none() {
        parsed.tier = nums.first().copied().map(|v| v as u8);
    }
    if parsed.last_attestation.is_none() {
        parsed.last_attestation = nums.get(1).copied();
    }
    if parsed.last_replay_seq.is_none() {
        parsed.last_replay_seq = nums.get(2).copied();
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_public_signals_has_six_fields() {
        let digest = [0x11u8; 32];
        let sigs = encode_public_signals(&digest, 3, 42);
        assert_eq!(sigs.len(), 6);
        assert_eq!(sigs[4], "3");
        assert_eq!(sigs[5], "42");
    }

    #[test]
    fn cast_array_format_is_stable() {
        let arr = cast_array(&["1".to_string(), "2".to_string(), "3".to_string()]);
        assert_eq!(arr, "[1,2,3]");
    }

    #[test]
    fn parse_bool_variants() {
        assert!(parse_bool_output("true").expect("bool parse"));
        assert!(!parse_bool_output("false").expect("bool parse"));
        assert!(parse_bool_output("0x1").expect("bool parse"));
        assert!(!parse_bool_output("0x0").expect("bool parse"));
    }

    #[test]
    fn parse_agent_status_tuple() {
        let raw = "(true, 0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, 4, 100, 101)";
        let parsed = parse_agent_status_output(raw);
        assert_eq!(parsed.active, Some(true));
        assert_eq!(
            parsed.puf_digest,
            Some("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string())
        );
        assert_eq!(parsed.tier, Some(4));
        assert_eq!(parsed.last_attestation, Some(100));
        assert_eq!(parsed.last_replay_seq, Some(101));
    }

    #[test]
    fn extract_hash_from_output() {
        let raw =
            "transactionHash 0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let got = extract_hash(raw);
        assert_eq!(
            got,
            Some("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string())
        );
    }
}
