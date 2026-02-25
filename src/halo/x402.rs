//! x402direct protocol integration — types, parsing, and validation.
//!
//! x402direct is a peer-to-peer stablecoin payment protocol using HTTP 402
//! responses and UPC (Unified Payment Contract) smart contracts on Base.
//!
//! AgentHALO integrates with x402direct in two ways:
//!
//! 1. **Payor (via AgentPMT):** Wrapped agents can pay for x402-protected
//!    resources using the `agentpmt/x402_pay` tool. AgentPMT handles wallet
//!    management, balance checks, and on-chain execution automatically.
//!
//! 2. **Validator (native):** The `x402_check` MCP tool lets agents parse
//!    and validate x402 payment requests locally without sending a transaction.
//!
//! ## Protocol overview
//!
//! ```text
//! 1. Agent requests protected resource
//! 2. Server returns 402 with x402direct JSON (nonce, payment options)
//! 3. Agent (or AgentPMT) executes payment via UPC contract
//! 4. Agent submits tx hash + nonce as proof → server grants access
//! ```
//!
//! See <https://www.x402direct.org> for the full protocol spec.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Protocol version
// ---------------------------------------------------------------------------

pub const X402_VERSION: &str = "direct.1.0.0";

// ---------------------------------------------------------------------------
// Network constants
// ---------------------------------------------------------------------------

/// Known x402direct network configurations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct X402Network {
    pub name: &'static str,
    pub chain_id: u64,
    pub caip2: &'static str,
    pub usdc_address: &'static str,
    pub rpc_url: &'static str,
}

pub const BASE_MAINNET: X402Network = X402Network {
    name: "base",
    chain_id: 8453,
    caip2: "eip155:8453",
    usdc_address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
    rpc_url: "https://mainnet.base.org",
};

pub const BASE_SEPOLIA: X402Network = X402Network {
    name: "base-sepolia",
    chain_id: 84532,
    caip2: "eip155:84532",
    usdc_address: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
    rpc_url: "https://sepolia.base.org",
};

/// Return the known network for a CAIP-2 chain identifier, if any.
pub fn network_for_caip2(caip2: &str) -> Option<&'static X402Network> {
    match caip2 {
        "eip155:8453" => Some(&BASE_MAINNET),
        "eip155:84532" => Some(&BASE_SEPOLIA),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Payment request (402 response body)
// ---------------------------------------------------------------------------

/// A parsed x402direct payment request (the JSON body of an HTTP 402 response).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X402PaymentRequest {
    /// Protocol version, e.g. "direct.1.0.0".
    pub x402version: String,
    /// Unique integer nonce for replay protection.
    pub nonce: u64,
    /// Human-readable description of what's being purchased.
    pub description: String,
    /// The protected resource path.
    pub resource: String,
    /// How to access after payment: "GET" or "POST".
    #[serde(default = "default_access_method")]
    pub access_method: String,
    /// Vendor-specific fields that must be echoed back with the proof.
    #[serde(default)]
    pub additional_required: serde_json::Value,
    /// Available payment methods.
    pub payment_options: Vec<X402PaymentOption>,
}

fn default_access_method() -> String {
    "GET".to_string()
}

/// A single payment option within an x402 request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X402PaymentOption {
    /// Unique identifier for this option (e.g. "po_base_usdc").
    pub id: String,
    /// CAIP-10 formatted recipient address (e.g. "eip155:8453:0x...").
    pub pay_to_address: String,
    /// Token contract address (plain, not CAIP-10).
    pub asset_address: String,
    /// Amount in token's smallest unit (e.g. 1000000 = 1 USDC).
    pub amount_required: u64,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Payment proof (submitted after on-chain payment)
// ---------------------------------------------------------------------------

/// Payment proof submitted to the vendor after executing on-chain payment.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X402PaymentProof {
    pub x402version: String,
    pub nonce: u64,
    pub payment_option_id: String,
    pub transaction_hash: String,
    pub authentication_contract: String,
    /// Flattened additionalRequired fields.
    #[serde(flatten)]
    pub additional_fields: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// CAIP-10 address parsing
// ---------------------------------------------------------------------------

/// Parsed CAIP-10 address components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caip10Address {
    /// Namespace (e.g. "eip155").
    pub namespace: String,
    /// Chain reference (e.g. "8453").
    pub chain_id: String,
    /// Account address (e.g. "0x742d...").
    pub address: String,
}

impl Caip10Address {
    /// CAIP-2 chain identifier (e.g. "eip155:8453").
    pub fn caip2(&self) -> String {
        format!("{}:{}", self.namespace, self.chain_id)
    }
}

/// Parse a CAIP-10 formatted address string.
pub fn parse_caip10(raw: &str) -> Result<Caip10Address, String> {
    let parts: Vec<&str> = raw.split(':').collect();
    if parts.len() != 3 {
        return Err(format!(
            "invalid CAIP-10 address (expected namespace:chainId:address): {raw}"
        ));
    }
    if parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
        return Err(format!("CAIP-10 address has empty component: {raw}"));
    }
    Ok(Caip10Address {
        namespace: parts[0].to_string(),
        chain_id: parts[1].to_string(),
        address: parts[2].to_string(),
    })
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validation result for an x402 payment request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct X402ValidationResult {
    pub valid: bool,
    pub version: String,
    pub nonce: u64,
    pub resource: String,
    pub description: String,
    pub access_method: String,
    pub option_count: usize,
    pub options_summary: Vec<X402OptionSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Summary of a single payment option after validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct X402OptionSummary {
    pub id: String,
    pub chain: String,
    pub recipient: String,
    pub token: String,
    pub amount_base_units: u64,
    pub amount_human: String,
    pub known_network: bool,
    pub known_token: bool,
}

/// Validate an x402 payment request and return a structured summary.
pub fn validate_payment_request(req: &X402PaymentRequest) -> X402ValidationResult {
    let mut warnings = Vec::new();

    // Check version prefix.
    if !req.x402version.starts_with("direct.") {
        return X402ValidationResult {
            valid: false,
            version: req.x402version.clone(),
            nonce: req.nonce,
            resource: req.resource.clone(),
            description: req.description.clone(),
            access_method: req.access_method.clone(),
            option_count: 0,
            options_summary: vec![],
            warnings: vec![],
            error: Some(format!(
                "invalid x402version: expected 'direct.*', got '{}'",
                req.x402version
            )),
        };
    }

    if req.payment_options.is_empty() {
        return X402ValidationResult {
            valid: false,
            version: req.x402version.clone(),
            nonce: req.nonce,
            resource: req.resource.clone(),
            description: req.description.clone(),
            access_method: req.access_method.clone(),
            option_count: 0,
            options_summary: vec![],
            warnings: vec![],
            error: Some("paymentOptions is empty".to_string()),
        };
    }

    let mut summaries = Vec::new();
    for opt in &req.payment_options {
        let caip = parse_caip10(&opt.pay_to_address);
        let (chain_label, recipient) = match &caip {
            Ok(a) => (a.caip2(), a.address.clone()),
            Err(_) => {
                warnings.push(format!(
                    "option '{}': invalid CAIP-10 payToAddress '{}'",
                    opt.id, opt.pay_to_address
                ));
                ("unknown".to_string(), opt.pay_to_address.clone())
            }
        };

        let known_network = caip
            .as_ref()
            .ok()
            .and_then(|a| network_for_caip2(&a.caip2()))
            .is_some();

        let known_token = caip
            .as_ref()
            .ok()
            .and_then(|a| network_for_caip2(&a.caip2()))
            .map(|net| net.usdc_address.eq_ignore_ascii_case(&opt.asset_address))
            .unwrap_or(false);

        if !known_network {
            warnings.push(format!(
                "option '{}': chain '{}' is not a known x402direct network",
                opt.id, chain_label
            ));
        }
        if known_network && !known_token {
            warnings.push(format!(
                "option '{}': token '{}' is not the known USDC address for {}",
                opt.id, opt.asset_address, chain_label
            ));
        }

        // Assume 6-decimal stablecoin for human-readable amount.
        let amount_human = format!("{:.6}", opt.amount_required as f64 / 1_000_000.0);

        summaries.push(X402OptionSummary {
            id: opt.id.clone(),
            chain: chain_label,
            recipient,
            token: opt.asset_address.clone(),
            amount_base_units: opt.amount_required,
            amount_human,
            known_network,
            known_token,
        });
    }

    X402ValidationResult {
        valid: true,
        version: req.x402version.clone(),
        nonce: req.nonce,
        resource: req.resource.clone(),
        description: req.description.clone(),
        access_method: req.access_method.clone(),
        option_count: req.payment_options.len(),
        options_summary: summaries,
        warnings,
        error: None,
    }
}

/// Parse raw JSON (e.g. from a 402 response body) into an X402PaymentRequest.
pub fn parse_x402_response(json_str: &str) -> Result<X402PaymentRequest, String> {
    serde_json::from_str(json_str).map_err(|e| format!("failed to parse x402 response: {e}"))
}

/// Check whether a transaction hash has the expected format (0x + 64 hex chars).
pub fn is_valid_tx_hash(hash: &str) -> bool {
    hash.len() == 66 && hash.starts_with("0x") && hash[2..].chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Config persistence
// ---------------------------------------------------------------------------

/// Per-instance x402 configuration (UPC contract address, preferred network).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct X402Config {
    /// Whether x402 payment handling is enabled.
    pub enabled: bool,
    /// UPC (Unified Payment Contract) proxy address to use for payments.
    #[serde(default)]
    pub upc_contract_address: Option<String>,
    /// Preferred network ("base" or "base-sepolia").
    #[serde(default = "default_preferred_network")]
    pub preferred_network: String,
    /// Maximum amount (in base units) to auto-approve without confirmation.
    #[serde(default = "default_max_auto_approve")]
    pub max_auto_approve: u64,
    /// Environment variable holding the private key for x402 payments.
    #[serde(default = "default_wallet_private_key_env")]
    pub wallet_private_key_env: String,
}

fn default_preferred_network() -> String {
    "base-sepolia".to_string()
}

fn default_max_auto_approve() -> u64 {
    5_000_000 // 5 USDC
}

fn default_wallet_private_key_env() -> String {
    "AGENTHALO_X402_PRIVATE_KEY".to_string()
}

impl Default for X402Config {
    fn default() -> Self {
        Self {
            enabled: false,
            upc_contract_address: None,
            preferred_network: default_preferred_network(),
            max_auto_approve: default_max_auto_approve(),
            wallet_private_key_env: default_wallet_private_key_env(),
        }
    }
}

pub fn x402_config_path() -> std::path::PathBuf {
    crate::halo::config::halo_dir().join("x402.json")
}

pub fn load_x402_config() -> X402Config {
    let path = x402_config_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => X402Config::default(),
    }
}

pub fn save_x402_config(cfg: &X402Config) -> Result<(), String> {
    let path = x402_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create x402 config dir: {e}"))?;
    }
    let raw =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize x402 config: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("write x402 config {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Payment execution
// ---------------------------------------------------------------------------

/// Result of an executed x402 payment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct X402PaymentResult {
    pub success: bool,
    pub transaction_hash: String,
    pub payment_option_id: String,
    pub amount: u64,
    pub amount_human: String,
    pub recipient: String,
    pub network: String,
    pub block_number: Option<u64>,
    pub gas_used: Option<u64>,
    pub proof: X402PaymentProof,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Resolve the wallet address from the configured private key.
pub fn get_wallet_address(cfg: &X402Config) -> Result<String, String> {
    let private_key = std::env::var(&cfg.wallet_private_key_env).map_err(|_| {
        format!(
            "x402 wallet not configured: set {} environment variable",
            cfg.wallet_private_key_env
        )
    })?;
    let env = vec![("ETH_PRIVATE_KEY".to_string(), private_key)];
    let args = vec!["wallet".to_string(), "address".to_string()];
    let out = crate::halo::onchain::run_cast(&args, &env)?;
    crate::halo::onchain::extract_address(&out)
        .ok_or_else(|| format!("failed to parse wallet address from cast output: {out}"))
}

/// Check the USDC balance on the specified network.
pub fn check_usdc_balance(cfg: &X402Config) -> Result<(String, u64), String> {
    let address = get_wallet_address(cfg)?;
    let network = resolve_network(&cfg.preferred_network)?;
    let args = vec![
        "call".to_string(),
        "--rpc-url".to_string(),
        network.rpc_url.to_string(),
        network.usdc_address.to_string(),
        "balanceOf(address)(uint256)".to_string(),
        address.clone(),
    ];
    let out = crate::halo::onchain::run_cast(&args, &[])?;
    let balance = parse_balance_output(&out)?;
    Ok((address, balance))
}

/// Execute an x402 payment: validate, check balance, transfer USDC, wait for receipt.
pub fn execute_payment(
    cfg: &X402Config,
    req: &X402PaymentRequest,
    option_id: Option<&str>,
) -> Result<X402PaymentResult, String> {
    if !cfg.enabled {
        return Err("x402 payments are disabled. Run: agenthalo x402 enable".to_string());
    }

    // Validate the payment request.
    let validation = validate_payment_request(req);
    if !validation.valid {
        return Err(format!(
            "invalid x402 payment request: {}",
            validation.error.unwrap_or_else(|| "unknown".to_string())
        ));
    }

    // Select payment option.
    let option = if let Some(id) = option_id {
        req.payment_options
            .iter()
            .find(|o| o.id == id)
            .ok_or_else(|| format!("payment option '{id}' not found in request"))?
    } else {
        // Pick the first option on a known network with known USDC token.
        req.payment_options
            .iter()
            .find(|o| {
                parse_caip10(&o.pay_to_address)
                    .ok()
                    .and_then(|a| network_for_caip2(&a.caip2()))
                    .map(|net| net.usdc_address.eq_ignore_ascii_case(&o.asset_address))
                    .unwrap_or(false)
            })
            .or_else(|| req.payment_options.first())
            .ok_or_else(|| "no payment options available".to_string())?
    };

    // Resolve the network from the payment option's CAIP-10 address.
    let caip = parse_caip10(&option.pay_to_address)?;
    let network = network_for_caip2(&caip.caip2())
        .ok_or_else(|| format!("unsupported network: {}", caip.caip2()))?;

    // Enforce auto-approve budget limit.
    if option.amount_required > cfg.max_auto_approve {
        return Err(format!(
            "payment amount {} exceeds auto-approve limit {} (configure with: agenthalo x402 config --max-auto-approve <units>)",
            option.amount_required, cfg.max_auto_approve
        ));
    }

    // Get wallet and check balance.
    let private_key = std::env::var(&cfg.wallet_private_key_env).map_err(|_| {
        format!(
            "x402 wallet not configured: set {} environment variable",
            cfg.wallet_private_key_env
        )
    })?;
    let signer_env = vec![("ETH_PRIVATE_KEY".to_string(), private_key)];

    let wallet_address = {
        let args = vec!["wallet".to_string(), "address".to_string()];
        let out = crate::halo::onchain::run_cast(&args, &signer_env)?;
        crate::halo::onchain::extract_address(&out)
            .ok_or_else(|| format!("failed to parse wallet address: {out}"))?
    };

    // Check USDC balance.
    let balance = {
        let args = vec![
            "call".to_string(),
            "--rpc-url".to_string(),
            network.rpc_url.to_string(),
            network.usdc_address.to_string(),
            "balanceOf(address)(uint256)".to_string(),
            wallet_address.clone(),
        ];
        let out = crate::halo::onchain::run_cast(&args, &[])?;
        parse_balance_output(&out)?
    };

    if balance < option.amount_required {
        return Err(format!(
            "insufficient USDC balance: have {} ({:.6} USDC), need {} ({:.6} USDC)",
            balance,
            balance as f64 / 1_000_000.0,
            option.amount_required,
            option.amount_required as f64 / 1_000_000.0
        ));
    }

    // Execute ERC-20 transfer: transfer(address,uint256)
    let recipient = &caip.address;
    let tx_hash = send_erc20_transfer(
        network.rpc_url,
        network.usdc_address,
        recipient,
        option.amount_required,
        &signer_env,
        &wallet_address,
    )?;

    // Wait for receipt.
    let receipt = crate::halo::onchain::wait_for_receipt(network.rpc_url, &tx_hash)?;

    let amount_human = format!("{:.6}", option.amount_required as f64 / 1_000_000.0);

    let proof = X402PaymentProof {
        x402version: req.x402version.clone(),
        nonce: req.nonce,
        payment_option_id: option.id.clone(),
        transaction_hash: tx_hash.clone(),
        authentication_contract: option.asset_address.clone(),
        additional_fields: match &req.additional_required {
            serde_json::Value::Object(m) => m.clone(),
            _ => serde_json::Map::new(),
        },
    };

    Ok(X402PaymentResult {
        success: true,
        transaction_hash: tx_hash,
        payment_option_id: option.id.clone(),
        amount: option.amount_required,
        amount_human,
        recipient: recipient.clone(),
        network: network.name.to_string(),
        block_number: receipt.block_number,
        gas_used: receipt.gas_used,
        proof,
        error: None,
    })
}

fn send_erc20_transfer(
    rpc_url: &str,
    token_address: &str,
    to: &str,
    amount: u64,
    signer_env: &[(String, String)],
    signer_address: &str,
) -> Result<String, String> {
    let nonce_retry_max = 3;
    let mut last_err = String::new();

    for attempt in 0..=nonce_retry_max {
        let mut args = vec![
            "send".to_string(),
            "--async".to_string(),
            "--rpc-url".to_string(),
            rpc_url.to_string(),
            token_address.to_string(),
            "transfer(address,uint256)(bool)".to_string(),
            to.to_string(),
            amount.to_string(),
        ];
        if attempt > 0 {
            let nonce = crate::halo::onchain::fetch_pending_nonce(rpc_url, signer_address)?;
            args.push("--nonce".to_string());
            args.push(nonce.to_string());
        }
        match crate::halo::onchain::run_cast(&args, signer_env) {
            Ok(out) => {
                return crate::halo::onchain::extract_hash(&out)
                    .ok_or_else(|| format!("failed to parse tx hash from cast output: {out}"));
            }
            Err(e) => {
                last_err = e.clone();
                if attempt == nonce_retry_max || !crate::halo::onchain::is_retryable_nonce_error(&e)
                {
                    return Err(e);
                }
                std::thread::sleep(std::time::Duration::from_millis(350 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last_err)
}

fn resolve_network(name: &str) -> Result<&'static X402Network, String> {
    match name {
        "base" => Ok(&BASE_MAINNET),
        "base-sepolia" => Ok(&BASE_SEPOLIA),
        _ => Err(format!(
            "unknown network: {name} (expected 'base' or 'base-sepolia')"
        )),
    }
}

fn parse_balance_output(raw: &str) -> Result<u64, String> {
    let trimmed = raw.trim();
    // cast returns decimal or hex depending on version.
    if let Some(hex) = trimmed.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16)
            .map_err(|e| format!("parse hex balance '{trimmed}': {e}"));
    }
    // Try decimal parse (cast may return "1000000\n" or similar).
    let first_token = trimmed
        .split_whitespace()
        .next()
        .unwrap_or(trimmed)
        .trim_matches(|c: char| !c.is_ascii_digit());
    first_token
        .parse::<u64>()
        .map_err(|e| format!("parse balance '{trimmed}': {e}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_caip10_valid() {
        let addr = parse_caip10("eip155:8453:0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb")
            .expect("valid caip10");
        assert_eq!(addr.namespace, "eip155");
        assert_eq!(addr.chain_id, "8453");
        assert_eq!(addr.address, "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb");
        assert_eq!(addr.caip2(), "eip155:8453");
    }

    #[test]
    fn parse_caip10_invalid() {
        assert!(parse_caip10("0x742d35Cc").is_err());
        assert!(parse_caip10("eip155:8453").is_err());
        assert!(parse_caip10("::").is_err());
        assert!(parse_caip10("").is_err());
    }

    #[test]
    fn network_lookup() {
        assert!(network_for_caip2("eip155:8453").is_some());
        assert!(network_for_caip2("eip155:84532").is_some());
        assert!(network_for_caip2("eip155:1").is_none());
    }

    #[test]
    fn parse_x402_response_valid() {
        let json = r#"{
            "x402version": "direct.1.0.0",
            "nonce": 1234567890,
            "description": "Premium API Access",
            "resource": "/api/premium/data",
            "accessMethod": "GET",
            "additionalRequired": {"userEmail": "user@example.com"},
            "paymentOptions": [
                {
                    "id": "po_base_usdc",
                    "payToAddress": "eip155:8453:0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
                    "assetAddress": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
                    "amountRequired": 1000000,
                    "description": "1 USDC on Base"
                }
            ]
        }"#;
        let req = parse_x402_response(json).expect("parse");
        assert_eq!(req.x402version, "direct.1.0.0");
        assert_eq!(req.nonce, 1234567890);
        assert_eq!(req.payment_options.len(), 1);
        assert_eq!(req.payment_options[0].amount_required, 1000000);
    }

    #[test]
    fn parse_x402_response_invalid() {
        assert!(parse_x402_response("not json").is_err());
        assert!(parse_x402_response("{}").is_err()); // missing required fields
    }

    #[test]
    fn validate_good_request() {
        let req = X402PaymentRequest {
            x402version: "direct.1.0.0".to_string(),
            nonce: 42,
            description: "test".to_string(),
            resource: "/test".to_string(),
            access_method: "GET".to_string(),
            additional_required: serde_json::json!({}),
            payment_options: vec![X402PaymentOption {
                id: "po_base_usdc".to_string(),
                pay_to_address: "eip155:8453:0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb".to_string(),
                asset_address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
                amount_required: 1000000,
                description: Some("1 USDC".to_string()),
            }],
        };
        let result = validate_payment_request(&req);
        assert!(result.valid);
        assert_eq!(result.option_count, 1);
        assert!(result.options_summary[0].known_network);
        assert!(result.options_summary[0].known_token);
        assert_eq!(result.options_summary[0].amount_human, "1.000000");
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn validate_bad_version() {
        let req = X402PaymentRequest {
            x402version: "bad.1.0".to_string(),
            nonce: 1,
            description: "test".to_string(),
            resource: "/test".to_string(),
            access_method: "GET".to_string(),
            additional_required: serde_json::json!({}),
            payment_options: vec![],
        };
        let result = validate_payment_request(&req);
        assert!(!result.valid);
        assert!(result.error.is_some());
    }

    #[test]
    fn validate_empty_options() {
        let req = X402PaymentRequest {
            x402version: "direct.1.0.0".to_string(),
            nonce: 1,
            description: "test".to_string(),
            resource: "/test".to_string(),
            access_method: "GET".to_string(),
            additional_required: serde_json::json!({}),
            payment_options: vec![],
        };
        let result = validate_payment_request(&req);
        assert!(!result.valid);
    }

    #[test]
    fn validate_unknown_chain_warns() {
        let req = X402PaymentRequest {
            x402version: "direct.1.0.0".to_string(),
            nonce: 1,
            description: "test".to_string(),
            resource: "/test".to_string(),
            access_method: "GET".to_string(),
            additional_required: serde_json::json!({}),
            payment_options: vec![X402PaymentOption {
                id: "po_polygon".to_string(),
                pay_to_address: "eip155:137:0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb".to_string(),
                asset_address: "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174".to_string(),
                amount_required: 500000,
                description: None,
            }],
        };
        let result = validate_payment_request(&req);
        assert!(result.valid);
        assert!(!result.warnings.is_empty());
        assert!(!result.options_summary[0].known_network);
    }

    #[test]
    fn validate_wrong_usdc_warns() {
        let req = X402PaymentRequest {
            x402version: "direct.1.0.0".to_string(),
            nonce: 1,
            description: "test".to_string(),
            resource: "/test".to_string(),
            access_method: "GET".to_string(),
            additional_required: serde_json::json!({}),
            payment_options: vec![X402PaymentOption {
                id: "po_base_fake".to_string(),
                pay_to_address: "eip155:8453:0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb".to_string(),
                asset_address: "0x0000000000000000000000000000000000000BAD".to_string(),
                amount_required: 1000000,
                description: None,
            }],
        };
        let result = validate_payment_request(&req);
        assert!(result.valid);
        assert!(!result.options_summary[0].known_token);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("not the known USDC")));
    }

    #[test]
    fn tx_hash_validation() {
        assert!(is_valid_tx_hash(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ));
        assert!(!is_valid_tx_hash("0x123"));
        assert!(!is_valid_tx_hash(
            "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ));
        assert!(!is_valid_tx_hash(
            "0xGGGG567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ));
    }

    #[test]
    fn payment_proof_roundtrip() {
        let proof = X402PaymentProof {
            x402version: "direct.1.0.0".to_string(),
            nonce: 42,
            payment_option_id: "po_base_usdc".to_string(),
            transaction_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .to_string(),
            authentication_contract: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb".to_string(),
            additional_fields: {
                let mut m = serde_json::Map::new();
                m.insert(
                    "userEmail".to_string(),
                    serde_json::json!("test@example.com"),
                );
                m
            },
        };
        let json = serde_json::to_string(&proof).expect("serialize");
        let loaded: X402PaymentProof = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(loaded.nonce, 42);
        assert_eq!(loaded.payment_option_id, "po_base_usdc");
        assert!(loaded.additional_fields.contains_key("userEmail"));
    }

    #[test]
    fn config_default_is_disabled() {
        let cfg = X402Config::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.preferred_network, "base-sepolia");
        assert_eq!(cfg.max_auto_approve, 5_000_000);
        assert_eq!(cfg.wallet_private_key_env, "AGENTHALO_X402_PRIVATE_KEY");
    }

    #[test]
    fn config_roundtrip() {
        let cfg = X402Config {
            enabled: true,
            upc_contract_address: Some("0xabc".to_string()),
            preferred_network: "base".to_string(),
            max_auto_approve: 10_000_000,
            wallet_private_key_env: "MY_KEY".to_string(),
        };
        let json = serde_json::to_string_pretty(&cfg).expect("serialize");
        let loaded: X402Config = serde_json::from_str(&json).expect("deserialize");
        assert!(loaded.enabled);
        assert_eq!(loaded.upc_contract_address.as_deref(), Some("0xabc"));
        assert_eq!(loaded.preferred_network, "base");
        assert_eq!(loaded.wallet_private_key_env, "MY_KEY");
    }

    #[test]
    fn config_legacy_without_wallet_key_deserializes() {
        let json = r#"{"enabled":true,"preferred_network":"base","max_auto_approve":5000000}"#;
        let loaded: X402Config = serde_json::from_str(json).expect("deserialize legacy");
        assert!(loaded.enabled);
        assert_eq!(loaded.wallet_private_key_env, "AGENTHALO_X402_PRIVATE_KEY");
    }

    #[test]
    fn resolve_network_known() {
        assert!(resolve_network("base").is_ok());
        assert!(resolve_network("base-sepolia").is_ok());
        assert!(resolve_network("polygon").is_err());
    }

    #[test]
    fn parse_balance_decimal() {
        assert_eq!(parse_balance_output("1000000").unwrap(), 1_000_000);
        assert_eq!(parse_balance_output("1000000\n").unwrap(), 1_000_000);
        assert_eq!(parse_balance_output("  42  ").unwrap(), 42);
    }

    #[test]
    fn parse_balance_hex() {
        assert_eq!(parse_balance_output("0xF4240").unwrap(), 1_000_000);
    }

    #[test]
    fn execute_payment_rejects_disabled() {
        let cfg = X402Config::default(); // disabled
        let req = X402PaymentRequest {
            x402version: "direct.1.0.0".to_string(),
            nonce: 1,
            description: "test".to_string(),
            resource: "/test".to_string(),
            access_method: "GET".to_string(),
            additional_required: serde_json::json!({}),
            payment_options: vec![X402PaymentOption {
                id: "po_base_usdc".to_string(),
                pay_to_address: "eip155:8453:0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb".to_string(),
                asset_address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
                amount_required: 1_000_000,
                description: None,
            }],
        };
        let err = execute_payment(&cfg, &req, None).expect_err("should fail when disabled");
        assert!(err.contains("disabled"));
    }

    #[test]
    fn execute_payment_rejects_over_budget() {
        let cfg = X402Config {
            enabled: true,
            max_auto_approve: 500_000, // 0.5 USDC
            ..X402Config::default()
        };
        let req = X402PaymentRequest {
            x402version: "direct.1.0.0".to_string(),
            nonce: 1,
            description: "test".to_string(),
            resource: "/test".to_string(),
            access_method: "GET".to_string(),
            additional_required: serde_json::json!({}),
            payment_options: vec![X402PaymentOption {
                id: "po_base_usdc".to_string(),
                pay_to_address: "eip155:8453:0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb".to_string(),
                asset_address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
                amount_required: 1_000_000, // 1 USDC > 0.5 limit
                description: None,
            }],
        };
        let err = execute_payment(&cfg, &req, None).expect_err("should exceed budget");
        assert!(err.contains("exceeds auto-approve limit"));
    }

    #[test]
    fn execute_payment_rejects_invalid_request() {
        let cfg = X402Config {
            enabled: true,
            ..X402Config::default()
        };
        let req = X402PaymentRequest {
            x402version: "bad.version".to_string(),
            nonce: 1,
            description: "test".to_string(),
            resource: "/test".to_string(),
            access_method: "GET".to_string(),
            additional_required: serde_json::json!({}),
            payment_options: vec![],
        };
        let err = execute_payment(&cfg, &req, None).expect_err("should reject invalid");
        assert!(err.contains("invalid x402"));
    }
}
