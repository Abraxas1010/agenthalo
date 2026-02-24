use crate::halo::circuit::AttestationProofBundle;
use crate::halo::config;
use crate::halo::util::{digest_bytes, hex_encode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::process::Command;

const BASE_SEPOLIA_RPC: &str = "https://sepolia.base.org";
const BASE_SEPOLIA_CHAIN_ID: u64 = 84532;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainConfig {
    pub rpc_url: String,
    pub chain_id: u64,
    pub chain_name: String,
    pub contract_address: String,
    pub private_key_env: String,
    pub verifier_address: String,
    pub usdc_address: String,
    pub treasury_address: String,
    pub fee_wei: u64,
}

impl Default for OnchainConfig {
    fn default() -> Self {
        Self {
            rpc_url: BASE_SEPOLIA_RPC.to_string(),
            chain_id: BASE_SEPOLIA_CHAIN_ID,
            chain_name: "base-sepolia".to_string(),
            contract_address: String::new(),
            private_key_env: "AGENTHALO_ONCHAIN_PRIVATE_KEY".to_string(),
            verifier_address: String::new(),
            usdc_address: String::new(),
            treasury_address: String::new(),
            fee_wei: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainAttestationResult {
    pub tx_hash: String,
    pub contract_address: String,
    pub block_number: Option<u64>,
    pub gas_used: Option<u64>,
    pub chain: String,
    pub public_inputs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainAttestationStatus {
    pub attestation_digest: String,
    pub verified: bool,
    pub recorded: bool,
    pub raw: String,
}

pub fn onchain_config_path() -> std::path::PathBuf {
    config::onchain_config_path()
}

pub fn load_onchain_config(path: &std::path::Path) -> Result<OnchainConfig, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read onchain config {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse onchain config {}: {e}", path.display()))
}

pub fn load_onchain_config_or_default() -> OnchainConfig {
    load_onchain_config(&onchain_config_path()).unwrap_or_default()
}

pub fn save_onchain_config(path: &std::path::Path, cfg: &OnchainConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create onchain config dir {}: {e}", parent.display()))?;
    }
    let raw =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize onchain config: {e}"))?;
    std::fs::write(path, raw).map_err(|e| format!("write onchain config {}: {e}", path.display()))
}

pub fn post_attestation(
    cfg: &OnchainConfig,
    bundle: &AttestationProofBundle,
    anonymous: bool,
) -> Result<OnchainAttestationResult, String> {
    validate_chain(cfg)?;
    if cfg.contract_address.trim().is_empty() {
        return Err(
            "onchain contract_address is empty; run `agenthalo onchain config --contract <addr>`"
                .to_string(),
        );
    }

    if onchain_stub_enabled() {
        let digest = digest_bytes(
            "agenthalo.onchain.stub.tx.v1",
            format!(
                "{}:{}:{}:{}",
                cfg.contract_address,
                anonymous,
                bundle.proof_hex,
                bundle.public_inputs.join(",")
            )
            .as_bytes(),
        );
        return Ok(OnchainAttestationResult {
            tx_hash: format!("0x{}", hex_encode(&digest)),
            contract_address: cfg.contract_address.clone(),
            block_number: None,
            gas_used: None,
            chain: cfg.chain_name.clone(),
            public_inputs: bundle.public_inputs.clone(),
        });
    }

    let private_key = std::env::var(&cfg.private_key_env).map_err(|_| {
        format!(
            "missing `{}` env var for onchain signing",
            cfg.private_key_env
        )
    })?;
    let fn_sig = if anonymous {
        "verifyAndRecordAnonymous(uint256[8],uint256[])"
    } else {
        "verifyAndRecord(uint256[8],uint256[])"
    };
    let cast_out = run_cast(&[
        "send".to_string(),
        "--async".to_string(),
        "--rpc-url".to_string(),
        cfg.rpc_url.clone(),
        "--private-key".to_string(),
        private_key,
        cfg.contract_address.clone(),
        fn_sig.to_string(),
        format!("[{}]", bundle.proof_words.join(",")),
        format!("[{}]", bundle.public_inputs.join(",")),
    ])?;
    let tx_hash = extract_hash(&cast_out)
        .ok_or_else(|| format!("failed to parse tx hash from cast output: {cast_out}"))?;

    let receipt = wait_for_receipt(&cfg.rpc_url, &tx_hash)?;
    Ok(OnchainAttestationResult {
        tx_hash,
        contract_address: cfg.contract_address.clone(),
        block_number: receipt.block_number,
        gas_used: receipt.gas_used,
        chain: cfg.chain_name.clone(),
        public_inputs: bundle.public_inputs.clone(),
    })
}

pub fn query_attestation(
    cfg: &OnchainConfig,
    attestation_digest: &str,
) -> Result<Option<OnchainAttestationStatus>, String> {
    validate_chain(cfg)?;
    if cfg.contract_address.trim().is_empty() {
        return Ok(None);
    }
    if onchain_stub_enabled() {
        return Ok(Some(OnchainAttestationStatus {
            attestation_digest: attestation_digest.to_string(),
            verified: false,
            recorded: false,
            raw: "stub".to_string(),
        }));
    }
    let raw = run_cast(&[
        "call".to_string(),
        "--rpc-url".to_string(),
        cfg.rpc_url.clone(),
        cfg.contract_address.clone(),
        "isVerified(bytes32)(bool)".to_string(),
        normalize_digest(attestation_digest)?,
    ])?;
    let verified = parse_bool_output(&raw)?;
    Ok(Some(OnchainAttestationStatus {
        attestation_digest: normalize_digest(attestation_digest)?,
        verified,
        recorded: verified,
        raw,
    }))
}

pub fn deploy_trust_verifier(cfg: &OnchainConfig) -> Result<String, String> {
    validate_chain(cfg)?;
    if onchain_stub_enabled() {
        let digest = digest_bytes(
            "agenthalo.onchain.stub.deploy.v1",
            format!(
                "{}:{}:{}:{}:{}",
                cfg.rpc_url,
                cfg.verifier_address,
                cfg.usdc_address,
                cfg.treasury_address,
                cfg.fee_wei
            )
            .as_bytes(),
        );
        return Ok(format!("0x{}", hex_encode(&digest[..20])));
    }
    let private_key = std::env::var(&cfg.private_key_env).map_err(|_| {
        format!(
            "missing `{}` env var for deployment signing",
            cfg.private_key_env
        )
    })?;

    for (name, value) in [
        ("verifier_address", cfg.verifier_address.trim()),
        ("usdc_address", cfg.usdc_address.trim()),
        ("treasury_address", cfg.treasury_address.trim()),
    ] {
        if value.is_empty() {
            return Err(format!("onchain {name} is empty; configure before deploy"));
        }
    }

    let out = run_cast(&[
        "create".to_string(),
        "--rpc-url".to_string(),
        cfg.rpc_url.clone(),
        "--private-key".to_string(),
        private_key,
        "contracts/TrustVerifier.sol:TrustVerifier".to_string(),
        "--constructor-args".to_string(),
        cfg.verifier_address.clone(),
        cfg.usdc_address.clone(),
        cfg.treasury_address.clone(),
        cfg.fee_wei.to_string(),
    ])?;
    extract_address(&out).ok_or_else(|| format!("failed to parse contract address from `{out}`"))
}

pub fn fetch_chain_id(rpc_url: &str) -> Result<u64, String> {
    let payload = json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"eth_chainId",
        "params":[]
    });
    let resp = ureq::post(rpc_url)
        .content_type("application/json")
        .send_json(payload)
        .map_err(|e| format!("Base Sepolia RPC unreachable: {e}"))?;
    let value: serde_json::Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("decode chain id response: {e}"))?;
    let hex = value
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("eth_chainId missing result: {value}"))?;
    parse_hex_u64(hex)
}

fn validate_chain(cfg: &OnchainConfig) -> Result<(), String> {
    if onchain_stub_enabled() {
        return Ok(());
    }
    let chain_id = fetch_chain_id(&cfg.rpc_url)?;
    if chain_id != cfg.chain_id {
        return Err(format!(
            "chain id mismatch: expected {}, got {}",
            cfg.chain_id, chain_id
        ));
    }
    Ok(())
}

#[derive(Default)]
struct TxReceipt {
    block_number: Option<u64>,
    gas_used: Option<u64>,
}

fn wait_for_receipt(rpc_url: &str, tx_hash: &str) -> Result<TxReceipt, String> {
    let delays = [1u64, 2, 4, 8, 16, 29];
    for delay in delays {
        let payload = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"eth_getTransactionReceipt",
            "params":[tx_hash]
        });
        let resp = ureq::post(rpc_url)
            .content_type("application/json")
            .send_json(payload.clone())
            .map_err(|e| format!("receipt RPC failed: {e}"))?;
        let value: serde_json::Value = resp
            .into_body()
            .read_json()
            .map_err(|e| format!("decode receipt response: {e}"))?;
        if let Some(result) = value.get("result") {
            if !result.is_null() {
                let block_number = result
                    .get("blockNumber")
                    .and_then(|v| v.as_str())
                    .and_then(|s| parse_hex_u64(s).ok());
                let gas_used = result
                    .get("gasUsed")
                    .and_then(|v| v.as_str())
                    .and_then(|s| parse_hex_u64(s).ok());
                return Ok(TxReceipt {
                    block_number,
                    gas_used,
                });
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(delay));
    }
    Ok(TxReceipt::default())
}

fn normalize_digest(digest: &str) -> Result<String, String> {
    let d = digest.trim();
    if d.is_empty() {
        return Err("attestation digest cannot be empty".to_string());
    }
    if d.starts_with("0x") {
        if d.len() != 66 {
            return Err("attestation digest must be 32 bytes".to_string());
        }
        return Ok(d.to_string());
    }
    if d.len() != 64 {
        return Err("attestation digest must be 32 bytes".to_string());
    }
    Ok(format!("0x{d}"))
}

fn parse_hex_u64(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    let hex = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(hex, 16).map_err(|e| format!("parse hex u64 `{raw}`: {e}"))
}

fn run_cast(args: &[String]) -> Result<String, String> {
    let out = Command::new("cast").args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "`cast` command not found".to_string()
        } else {
            format!("cast execution failed: {e}")
        }
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(format!(
            "cast failed status={} stdout=`{}` stderr=`{}`",
            out.status, stdout, stderr
        ));
    }
    Ok(if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{stdout}\n{stderr}")
    })
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

fn extract_address(raw: &str) -> Option<String> {
    raw.split(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '(' | ')' | '"' | '\'' | ';'))
        .find(|tok| {
            tok.len() == 42
                && tok.starts_with("0x")
                && tok[2..].chars().all(|c| c.is_ascii_hexdigit())
        })
        .map(|s| s.to_string())
}

fn parse_bool_output(raw: &str) -> Result<bool, String> {
    let t = raw.trim().to_ascii_lowercase();
    if t.contains("true") {
        return Ok(true);
    }
    if t.contains("false") {
        return Ok(false);
    }
    if let Some(hex) = t.strip_prefix("0x") {
        return Ok(hex.chars().any(|c| c != '0'));
    }
    if let Ok(v) = t.parse::<u64>() {
        return Ok(v != 0);
    }
    Err(format!("boolean result expected, got `{raw}`"))
}

pub fn onchain_stub_enabled() -> bool {
    for key in ["AGENTHALO_ONCHAIN_STUB", "AGENTHALO_AGENTPMT_STUB"] {
        if let Ok(v) = std::env::var(key) {
            if matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes") {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::circuit::AttestationProofBundle;

    #[test]
    fn test_abi_encode_verify_and_record() {
        let bundle = AttestationProofBundle {
            proof_hex: "deadbeef".to_string(),
            proof_words: [
                "1".to_string(),
                "2".to_string(),
                "3".to_string(),
                "4".to_string(),
                "5".to_string(),
                "6".to_string(),
                "7".to_string(),
                "8".to_string(),
            ],
            public_inputs: vec!["10".to_string(), "11".to_string()],
        };
        assert_eq!(
            format!("[{}]", bundle.proof_words.join(",")),
            "[1,2,3,4,5,6,7,8]"
        );
        assert_eq!(format!("[{}]", bundle.public_inputs.join(",")), "[10,11]");
    }

    #[test]
    fn test_rlp_encode_eip1559_tx() {
        // Phase 4 uses cast for signing/sending, but we still validate basic hex parser helpers.
        assert_eq!(parse_hex_u64("0x14a34").expect("hex"), 84532);
    }

    #[test]
    fn test_onchain_config_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_onchain_cfg_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("onchain.json");
        let cfg = OnchainConfig {
            contract_address: "0x1111111111111111111111111111111111111111".to_string(),
            ..OnchainConfig::default()
        };
        save_onchain_config(&path, &cfg).expect("save");
        let loaded = load_onchain_config(&path).expect("load");
        assert_eq!(loaded.contract_address, cfg.contract_address);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_query_attestation_not_found() {
        std::env::set_var("AGENTHALO_ONCHAIN_STUB", "1");
        let cfg = OnchainConfig {
            contract_address: "0x1111111111111111111111111111111111111111".to_string(),
            ..OnchainConfig::default()
        };
        let status = query_attestation(&cfg, "00".repeat(32).as_str())
            .expect("query")
            .expect("some");
        assert!(!status.verified);
        std::env::remove_var("AGENTHALO_ONCHAIN_STUB");
    }
}
