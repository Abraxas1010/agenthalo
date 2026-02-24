use crate::halo::config;
use crate::halo::trace::now_unix_secs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuditSize {
    Small,
    Medium,
    Large,
}

impl AuditSize {
    pub fn parse(input: &str) -> Result<Self, String> {
        match input.trim().to_ascii_lowercase().as_str() {
            "small" => Ok(Self::Small),
            "medium" => Ok(Self::Medium),
            "large" => Ok(Self::Large),
            other => Err(format!(
                "invalid audit size: {other} (expected small|medium|large)"
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AuditSize::Small => "small",
            AuditSize::Medium => "medium",
            AuditSize::Large => "large",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditRequest {
    pub contract_path: String,
    pub size: AuditSize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditFinding {
    pub severity: FindingSeverity,
    pub category: String,
    pub description: String,
    pub line_range: Option<(usize, usize)>,
    pub recommendation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AuditResult {
    pub contract_path: String,
    pub contract_hash: String,
    pub file_size_bytes: u64,
    pub findings: Vec<AuditFinding>,
    pub risk_score: f64,
    pub timestamp: u64,
    pub audit_level: String,
    pub checks_performed: Vec<String>,
    pub attestation_digest: String,
}

#[derive(Clone, Debug)]
struct FunctionWindow {
    name: String,
    start_line: usize,
    end_line: usize,
    signature: String,
    visibility: Option<String>,
    modifiers: Vec<String>,
    body_lines: Vec<(usize, String)>,
}

impl FunctionWindow {
    fn is_public_or_external(&self) -> bool {
        matches!(
            self.visibility.as_deref(),
            Some("public") | Some("external")
        )
    }

    fn has_non_reentrant_guard(&self) -> bool {
        self.modifiers
            .iter()
            .any(|m| m.eq_ignore_ascii_case("nonreentrant"))
    }

    fn has_access_control(&self) -> bool {
        self.modifiers.iter().any(|m| {
            let lower = m.to_ascii_lowercase();
            lower.contains("owner") || lower.contains("admin") || lower.contains("role")
        }) || self
            .body_lines
            .iter()
            .take(6)
            .any(|(_, line)| is_sender_guard_line(line))
    }

    fn is_read_only(&self) -> bool {
        let sig = self.signature.to_ascii_lowercase();
        sig.contains(" view") || sig.contains(" pure")
    }
}

pub fn audit_contract_file(path: &Path, size: AuditSize) -> Result<AuditResult, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("read contract {}: {e}", path.display()))?;
    let request = AuditRequest {
        contract_path: path.display().to_string(),
        size,
    };
    audit_contract_source(&source, request)
}

pub fn audit_contract_source(source: &str, request: AuditRequest) -> Result<AuditResult, String> {
    let contract_hash = hex_encode(Sha256::digest(source.as_bytes()).as_slice());
    let timestamp = now_unix_secs();
    let lines: Vec<String> = source.lines().map(|l| l.to_string()).collect();
    let functions = parse_functions(&lines);
    let mut checks_performed = vec![
        "reentrancy-cei".to_string(),
        "unchecked-external-call".to_string(),
        "tx-origin-auth".to_string(),
        "selfdestruct-access-control".to_string(),
        "pragma-safety".to_string(),
        "overflow-pre-0.8".to_string(),
    ];
    if matches!(request.size, AuditSize::Medium | AuditSize::Large) {
        checks_performed.extend_from_slice(&[
            "access-control-state-change".to_string(),
            "delegatecall-storage-collision".to_string(),
            "front-running-timestamp-number".to_string(),
            "flash-loan-style-path".to_string(),
            "state-change-event-emission".to_string(),
        ]);
    }
    if matches!(request.size, AuditSize::Large) {
        checks_performed.extend_from_slice(&[
            "cross-function-reentrancy-path".to_string(),
            "token-standard-compliance".to_string(),
            "gas-griefing-loops".to_string(),
            "proxy-initializer-protection".to_string(),
            "centralization-risk".to_string(),
        ]);
    }

    let mut findings = Vec::new();
    findings.extend(check_pragma_safety(&lines));
    findings.extend(check_overflow_pre_08(&lines));
    findings.extend(check_tx_origin(&lines));
    findings.extend(check_selfdestruct(&lines, &functions));
    findings.extend(check_unchecked_external_calls(&lines));
    findings.extend(check_reentrancy_cei(&functions));

    if matches!(request.size, AuditSize::Medium | AuditSize::Large) {
        findings.extend(check_access_control(&functions));
        findings.extend(check_delegatecall_storage_collision(&lines));
        findings.extend(check_front_running_patterns(&functions));
        findings.extend(check_flash_loan_paths(&functions));
        findings.extend(check_missing_event_emission(&functions));
    }
    if matches!(request.size, AuditSize::Large) {
        findings.extend(check_cross_function_reentrancy(&functions));
        findings.extend(check_token_compliance(&lines));
        findings.extend(check_gas_griefing(&functions));
        findings.extend(check_proxy_initializer(&lines, &functions));
        findings.extend(check_centralization_risk(&functions));
    }

    sort_findings(&mut findings);
    let risk_score = compute_risk_score(&findings);
    let attestation_digest = digest_audit(&contract_hash, risk_score, timestamp);

    Ok(AuditResult {
        contract_path: request.contract_path,
        contract_hash,
        file_size_bytes: source.len() as u64,
        findings,
        risk_score,
        timestamp,
        audit_level: request.size.as_str().to_string(),
        checks_performed,
        attestation_digest,
    })
}

pub fn save_audit_result(result: &AuditResult) -> Result<PathBuf, String> {
    config::ensure_halo_dir()?;
    config::ensure_audits_dir()?;
    let path = config::audits_dir().join(format!("{}.json", result.contract_hash));
    let raw =
        serde_json::to_vec_pretty(result).map_err(|e| format!("serialize audit result: {e}"))?;
    std::fs::write(&path, raw)
        .map_err(|e| format!("write audit result {}: {e}", path.display()))?;
    Ok(path)
}

fn check_pragma_safety(lines: &[String]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let l = line.trim();
        if !l.starts_with("pragma solidity") {
            continue;
        }
        if l.contains('^') || l.contains(">=") || l.contains('<') || l.contains('*') {
            out.push(AuditFinding {
                severity: FindingSeverity::Low,
                category: "floating-pragma".to_string(),
                description: "Floating pragma detected; compiler version is not pinned".to_string(),
                line_range: Some((idx + 1, idx + 1)),
                recommendation:
                    "Pin the pragma to an exact compiler version (e.g., pragma solidity 0.8.21;)"
                        .to_string(),
            });
        }
    }
    out
}

fn check_overflow_pre_08(lines: &[String]) -> Vec<AuditFinding> {
    let pragma = lines
        .iter()
        .find_map(|line| line.trim().strip_prefix("pragma solidity "))
        .unwrap_or_default()
        .to_string();
    let (major, minor) = parse_solidity_version(&pragma);
    if major > 0 || minor >= 8 {
        return Vec::new();
    }
    for (idx, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("//") || t.starts_with("pragma ") {
            continue;
        }
        if looks_like_arithmetic_state_update(t) {
            return vec![AuditFinding {
                severity: FindingSeverity::Medium,
                category: "overflow-pre-0.8".to_string(),
                description: "Arithmetic in Solidity <0.8 can overflow/underflow without checks"
                    .to_string(),
                line_range: Some((idx + 1, idx + 1)),
                recommendation:
                    "Upgrade compiler to >=0.8 or use SafeMath-style checked arithmetic".to_string(),
            }];
        }
    }
    Vec::new()
}

fn check_tx_origin(lines: &[String]) -> Vec<AuditFinding> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("tx.origin"))
        .map(|(idx, _)| AuditFinding {
            severity: FindingSeverity::High,
            category: "tx-origin-auth".to_string(),
            description: "tx.origin used for authorization; vulnerable to phishing/proxy attacks"
                .to_string(),
            line_range: Some((idx + 1, idx + 1)),
            recommendation: "Use msg.sender for authorization checks instead of tx.origin"
                .to_string(),
        })
        .collect()
}

fn check_selfdestruct(lines: &[String], functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if !line.contains("selfdestruct") {
            continue;
        }
        let protected = function_for_line(functions, idx + 1)
            .map(|f| f.has_access_control())
            .unwrap_or(false);
        if !protected {
            out.push(AuditFinding {
                severity: FindingSeverity::High,
                category: "selfdestruct-unprotected".to_string(),
                description: "selfdestruct appears without an obvious access-control guard"
                    .to_string(),
                line_range: Some((idx + 1, idx + 1)),
                recommendation: "Restrict destructive operations to explicit admin/owner controls"
                    .to_string(),
            });
        }
    }
    out
}

fn check_unchecked_external_calls(lines: &[String]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if !looks_like_external_call(line) {
            continue;
        }
        let trimmed = line.trim();
        let checked_inline = trimmed.contains("require(") || trimmed.contains("assert(");
        let assigned = trimmed.contains('=');
        if !(checked_inline || assigned) {
            out.push(AuditFinding {
                severity: FindingSeverity::High,
                category: "unchecked-external-call".to_string(),
                description: "Low-level external call result is not checked".to_string(),
                line_range: Some((idx + 1, idx + 1)),
                recommendation: "Capture call result and enforce success via require/assert"
                    .to_string(),
            });
        }
    }
    out
}

fn check_reentrancy_cei(functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for function in functions {
        if !function.is_public_or_external() || function.is_read_only() {
            continue;
        }
        let first_external = function
            .body_lines
            .iter()
            .find(|(_, line)| looks_like_value_transfer(line))
            .map(|(line_no, _)| *line_no);
        let Some(ext_line) = first_external else {
            continue;
        };
        let state_update_after = function
            .body_lines
            .iter()
            .find(|(line_no, line)| *line_no > ext_line && looks_like_state_write(line))
            .map(|(line_no, _)| *line_no);
        if let Some(write_line) = state_update_after {
            out.push(AuditFinding {
                severity: FindingSeverity::High,
                category: "reentrancy-cei-violation".to_string(),
                description: format!(
                    "Function '{}' performs external value transfer before state update",
                    function.name
                ),
                line_range: Some((ext_line, write_line)),
                recommendation: "Apply checks-effects-interactions ordering or add reentrancy guard".to_string(),
            });
        }
    }
    out
}

fn check_access_control(functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for function in functions {
        if !function.is_public_or_external() || function.is_read_only() {
            continue;
        }
        if function.has_access_control() {
            continue;
        }
        if !function
            .body_lines
            .iter()
            .any(|(_, line)| looks_like_state_write(line))
        {
            continue;
        }
        out.push(AuditFinding {
            severity: FindingSeverity::Medium,
            category: "missing-access-control".to_string(),
            description: format!(
                "State-mutating public/external function '{}' has no obvious access control",
                function.name
            ),
            line_range: Some((function.start_line, function.start_line)),
            recommendation: "Require role/owner checks for privileged state changes".to_string(),
        });
    }
    out
}

fn check_delegatecall_storage_collision(lines: &[String]) -> Vec<AuditFinding> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("delegatecall"))
        .map(|(idx, _)| AuditFinding {
            severity: FindingSeverity::Medium,
            category: "delegatecall-storage-collision".to_string(),
            description: "delegatecall detected; storage layout collisions may corrupt state"
                .to_string(),
            line_range: Some((idx + 1, idx + 1)),
            recommendation:
                "Use fixed storage slots (EIP-1967/diamond storage) and audit layout compatibility"
                    .to_string(),
        })
        .collect()
}

fn check_front_running_patterns(functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for function in functions {
        let uses_timestamp_or_block = function
            .body_lines
            .iter()
            .any(|(_, line)| line.contains("block.timestamp") || line.contains("block.number"));
        let touches_value = function
            .body_lines
            .iter()
            .any(|(_, line)| looks_like_value_transfer(line));
        if uses_timestamp_or_block && touches_value {
            out.push(AuditFinding {
                severity: FindingSeverity::Medium,
                category: "front-running-timing-dependence".to_string(),
                description: format!(
                    "Function '{}' mixes block metadata with value-sensitive operations",
                    function.name
                ),
                line_range: Some((function.start_line, function.end_line)),
                recommendation: "Avoid price/value logic directly based on block.timestamp/number; use safer oracle windows".to_string(),
            });
        }
    }
    out
}

fn check_flash_loan_paths(functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for function in functions {
        if !function.is_public_or_external() {
            continue;
        }
        let value_transfer_count = function
            .body_lines
            .iter()
            .filter(|(_, line)| looks_like_value_transfer(line))
            .count();
        if value_transfer_count > 0 && !function.has_non_reentrant_guard() {
            out.push(AuditFinding {
                severity: FindingSeverity::Medium,
                category: "flash-loan-style-atomic-path".to_string(),
                description: format!(
                    "Function '{}' performs value transfer without explicit reentrancy guard",
                    function.name
                ),
                line_range: Some((function.start_line, function.end_line)),
                recommendation: "Add explicit reentrancy/atomicity safeguards and invariant checks"
                    .to_string(),
            });
        }
    }
    out
}

fn check_missing_event_emission(functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for function in functions {
        if !function.is_public_or_external() || function.is_read_only() {
            continue;
        }
        let mutates_state = function
            .body_lines
            .iter()
            .any(|(_, line)| looks_like_state_write(line));
        let emits_event = function
            .body_lines
            .iter()
            .any(|(_, line)| line.contains("emit "));
        if mutates_state && !emits_event {
            out.push(AuditFinding {
                severity: FindingSeverity::Low,
                category: "missing-event-emission".to_string(),
                description: format!(
                    "State-mutating function '{}' does not emit an event",
                    function.name
                ),
                line_range: Some((function.start_line, function.end_line)),
                recommendation: "Emit events for externally significant state transitions"
                    .to_string(),
            });
        }
    }
    out
}

fn check_cross_function_reentrancy(functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    let mut dangerous = HashSet::new();
    for function in functions {
        if function
            .body_lines
            .iter()
            .any(|(_, line)| looks_like_value_transfer(line))
        {
            dangerous.insert(function.name.clone());
        }
    }
    if dangerous.is_empty() {
        return out;
    }
    let names: HashSet<String> = functions.iter().map(|f| f.name.clone()).collect();
    for function in functions {
        if !function.is_public_or_external() {
            continue;
        }
        for (_, line) in &function.body_lines {
            for callee in extract_function_calls(line) {
                if names.contains(&callee) && dangerous.contains(&callee) {
                    out.push(AuditFinding {
                        severity: FindingSeverity::Medium,
                        category: "cross-function-reentrancy-path".to_string(),
                        description: format!(
                            "Function '{}' calls '{}' which performs external value transfer",
                            function.name, callee
                        ),
                        line_range: Some((function.start_line, function.end_line)),
                        recommendation: "Audit call-chain CEI ordering and guard state before cross-function external transfers".to_string(),
                    });
                }
            }
        }
    }
    out
}

fn check_token_compliance(lines: &[String]) -> Vec<AuditFinding> {
    let source = lines.join("\n");
    let looks_like_token = source.contains("totalSupply")
        || source.contains("balanceOf")
        || source.contains("transfer(")
        || source.to_ascii_lowercase().contains("erc20");
    if !looks_like_token {
        return Vec::new();
    }
    let mut missing = Vec::new();
    if !source.contains("event Transfer") {
        missing.push("Transfer event");
    }
    if !source.contains("event Approval") {
        missing.push("Approval event");
    }
    if !source.contains("transferFrom(") {
        missing.push("transferFrom function");
    }
    if missing.is_empty() {
        return Vec::new();
    }
    vec![AuditFinding {
        severity: FindingSeverity::Medium,
        category: "token-standard-compliance".to_string(),
        description: format!(
            "Token-like contract may be missing required ERC-20 elements: {}",
            missing.join(", ")
        ),
        line_range: None,
        recommendation:
            "Implement and test full interface/event compliance for the intended token standard"
                .to_string(),
    }]
}

fn check_gas_griefing(functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for function in functions {
        if !function.is_public_or_external() {
            continue;
        }
        for (line_no, line) in &function.body_lines {
            let t = line.trim();
            if (t.starts_with("for ") || t.starts_with("for(") || t.starts_with("while "))
                && line.contains(".length")
            {
                out.push(AuditFinding {
                    severity: FindingSeverity::Medium,
                    category: "gas-griefing-unbounded-loop".to_string(),
                    description: format!(
                        "Loop over dynamic length in externally callable function '{}'",
                        function.name
                    ),
                    line_range: Some((*line_no, *line_no)),
                    recommendation: "Bound loop iterations or split work across transactions"
                        .to_string(),
                });
            }
        }
    }
    out
}

fn check_proxy_initializer(lines: &[String], functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let has_delegatecall = lines.iter().any(|l| l.contains("delegatecall"));
    if !has_delegatecall {
        return Vec::new();
    }
    let initializer = functions
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case("initialize"));
    let Some(initializer) = initializer else {
        return vec![AuditFinding {
            severity: FindingSeverity::Medium,
            category: "proxy-initializer-missing".to_string(),
            description: "delegatecall proxy behavior detected without initialize() guard path"
                .to_string(),
            line_range: None,
            recommendation:
                "Provide explicit initializer function and one-time initialization guard"
                    .to_string(),
        }];
    };
    if !initializer
        .signature
        .to_ascii_lowercase()
        .contains("initializer")
        && !initializer
            .body_lines
            .iter()
            .any(|(_, line)| line.contains("initialized"))
    {
        return vec![AuditFinding {
            severity: FindingSeverity::Medium,
            category: "proxy-initializer-unprotected".to_string(),
            description: "initialize() found but no obvious one-time initialization guard"
                .to_string(),
            line_range: Some((initializer.start_line, initializer.end_line)),
            recommendation: "Use initializer modifiers/flags to block re-initialization"
                .to_string(),
        }];
    }
    Vec::new()
}

fn check_centralization_risk(functions: &[FunctionWindow]) -> Vec<AuditFinding> {
    let mut out = Vec::new();
    for function in functions {
        if !function.has_access_control() {
            continue;
        }
        let name = function.name.to_ascii_lowercase();
        if name.contains("upgrade")
            || name.contains("set")
            || name.contains("withdraw")
            || name.contains("pause")
        {
            out.push(AuditFinding {
                severity: FindingSeverity::Info,
                category: "centralization-risk".to_string(),
                description: format!(
                    "Privileged function '{}' indicates centralized control surface",
                    function.name
                ),
                line_range: Some((function.start_line, function.end_line)),
                recommendation: "Document admin controls and consider multisig/timelock governance"
                    .to_string(),
            });
        }
    }
    out
}

fn sort_findings(findings: &mut [AuditFinding]) {
    findings.sort_by(|a, b| {
        let sa = severity_rank(&a.severity);
        let sb = severity_rank(&b.severity);
        sb.cmp(&sa)
            .then_with(|| start_line(a).cmp(&start_line(b)))
            .then_with(|| a.category.cmp(&b.category))
    });
}

fn severity_rank(severity: &FindingSeverity) -> u8 {
    match severity {
        FindingSeverity::Critical => 5,
        FindingSeverity::High => 4,
        FindingSeverity::Medium => 3,
        FindingSeverity::Low => 2,
        FindingSeverity::Info => 1,
    }
}

fn start_line(finding: &AuditFinding) -> usize {
    finding.line_range.map(|v| v.0).unwrap_or(usize::MAX)
}

fn compute_risk_score(findings: &[AuditFinding]) -> f64 {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for finding in findings {
        let key = match finding.severity {
            FindingSeverity::Critical => "critical",
            FindingSeverity::High => "high",
            FindingSeverity::Medium => "medium",
            FindingSeverity::Low => "low",
            FindingSeverity::Info => "info",
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    let critical = *counts.get("critical").unwrap_or(&0) as f64;
    let high = *counts.get("high").unwrap_or(&0) as f64;
    let medium = *counts.get("medium").unwrap_or(&0) as f64;
    let low = *counts.get("low").unwrap_or(&0) as f64;
    let weighted = critical * 0.4 + high * 0.2 + medium * 0.1 + low * 0.02;
    (weighted / 2.0).clamp(0.0, 1.0)
}

fn digest_audit(contract_hash: &str, risk_score: f64, timestamp: u64) -> String {
    let payload = format!(
        "agenthalo.audit.digest.v1:{contract_hash}:{:.6}:{timestamp}",
        risk_score
    );
    hex_encode(Sha256::digest(payload.as_bytes()).as_slice())
}

fn function_for_line(functions: &[FunctionWindow], line: usize) -> Option<&FunctionWindow> {
    functions
        .iter()
        .find(|f| line >= f.start_line && line <= f.end_line)
}

fn parse_functions(lines: &[String]) -> Vec<FunctionWindow> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let line = lines[idx].trim();
        if !line.contains("function ") {
            idx += 1;
            continue;
        }

        let start = idx + 1;
        let mut sig = line.to_string();
        let mut brace_depth =
            line.matches('{').count() as isize - line.matches('}').count() as isize;
        let mut end = idx;
        let mut body_lines = vec![(idx + 1, lines[idx].clone())];

        while brace_depth <= 0 && end + 1 < lines.len() {
            end += 1;
            let l = lines[end].clone();
            sig.push(' ');
            sig.push_str(l.trim());
            brace_depth += l.matches('{').count() as isize;
            brace_depth -= l.matches('}').count() as isize;
            body_lines.push((end + 1, l));
        }
        while brace_depth > 0 && end + 1 < lines.len() {
            end += 1;
            let l = lines[end].clone();
            brace_depth += l.matches('{').count() as isize;
            brace_depth -= l.matches('}').count() as isize;
            body_lines.push((end + 1, l));
        }

        let name = parse_function_name(&sig).unwrap_or_else(|| format!("function@{start}"));
        let visibility = parse_visibility(&sig);
        let modifiers = parse_modifiers(&sig);

        out.push(FunctionWindow {
            name,
            start_line: start,
            end_line: end + 1,
            signature: sig,
            visibility,
            modifiers,
            body_lines,
        });
        idx = end + 1;
    }
    out
}

fn parse_function_name(signature: &str) -> Option<String> {
    let after = signature.split("function ").nth(1)?;
    let mut name = String::new();
    for ch in after.chars() {
        if ch == '(' || ch.is_whitespace() {
            break;
        }
        name.push(ch);
    }
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn parse_visibility(signature: &str) -> Option<String> {
    let lower = signature.to_ascii_lowercase();
    if lower.contains(" public ") || lower.ends_with(" public") {
        Some("public".to_string())
    } else if lower.contains(" external ") || lower.ends_with(" external") {
        Some("external".to_string())
    } else if lower.contains(" internal ") || lower.ends_with(" internal") {
        Some("internal".to_string())
    } else if lower.contains(" private ") || lower.ends_with(" private") {
        Some("private".to_string())
    } else {
        None
    }
}

fn parse_modifiers(signature: &str) -> Vec<String> {
    let mut out = Vec::new();
    let close_paren = signature.rfind(')').unwrap_or(0);
    let tail = signature.get(close_paren + 1..).unwrap_or_default();
    for token in tail.split(|c: char| c.is_whitespace() || c == '{' || c == ',') {
        let t = token.trim();
        if t.is_empty()
            || matches!(
                t,
                "public"
                    | "private"
                    | "external"
                    | "internal"
                    | "payable"
                    | "view"
                    | "pure"
                    | "returns"
                    | "virtual"
                    | "override"
            )
        {
            continue;
        }
        out.push(t.to_string());
    }
    out
}

fn parse_solidity_version(pragma_tail: &str) -> (u32, u32) {
    let mut digits = String::new();
    for ch in pragma_tail.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }
    let mut parts = digits.split('.');
    let major = parts
        .next()
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    let minor = parts
        .next()
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    (major, minor)
}

fn looks_like_external_call(line: &str) -> bool {
    line.contains(".call(")
        || line.contains(".call{")
        || line.contains(".delegatecall(")
        || line.contains(".delegatecall{")
}

fn looks_like_value_transfer(line: &str) -> bool {
    line.contains(".call{value:")
        || line.contains(".call.value(")
        || line.contains(".transfer(")
        || line.contains(".send(")
}

fn looks_like_state_write(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with("//")
        || t.starts_with("emit ")
        || t.starts_with("require(")
        || t.starts_with("assert(")
    {
        return false;
    }
    if t.contains("==") || t.contains("!=") || t.contains(">=") || t.contains("<=") {
        return false;
    }
    t.contains('=') || t.contains("+=") || t.contains("-=")
}

fn looks_like_arithmetic_state_update(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with("//") || !t.contains('=') {
        return false;
    }
    t.contains('+') || t.contains('-') || t.contains('*')
}

fn is_sender_guard_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("require(")
        && lower.contains("msg.sender")
        && (lower.contains("owner") || lower.contains("admin") || lower.contains("governor"))
}

fn extract_function_calls(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut token = String::new();
    for ch in line.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }
        if ch == '(' && !token.is_empty() {
            if !matches!(
                token.as_str(),
                "if" | "for" | "while" | "require" | "assert" | "emit" | "return"
            ) {
                out.push(token.clone());
            }
        }
        token.clear();
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit_from_source(source: &str, size: AuditSize) -> AuditResult {
        audit_contract_source(
            source,
            AuditRequest {
                contract_path: "inline.sol".to_string(),
                size,
            },
        )
        .expect("audit source")
    }

    #[test]
    fn reentrancy_detection() {
        let source = r#"
            pragma solidity ^0.8.0;
            contract V {
              mapping(address => uint256) public balances;
              function withdraw() public {
                (bool ok,) = msg.sender.call{value: balances[msg.sender]}("");
                require(ok);
                balances[msg.sender] = 0;
              }
            }
        "#;
        let result = audit_from_source(source, AuditSize::Small);
        assert!(result
            .findings
            .iter()
            .any(|f| f.category == "reentrancy-cei-violation"));
    }

    #[test]
    fn clean_contract_low_risk() {
        let source = r#"
            pragma solidity 0.8.21;
            contract Safe {
              mapping(address => uint256) public balances;
              event Withdraw(address indexed user, uint256 amount);
              function withdraw() public {
                uint256 amount = balances[msg.sender];
                balances[msg.sender] = 0;
                (bool ok,) = msg.sender.call{value: amount}("");
                require(ok);
                emit Withdraw(msg.sender, amount);
              }
            }
        "#;
        let result = audit_from_source(source, AuditSize::Small);
        assert!(result.risk_score < 0.2, "risk score {}", result.risk_score);
        assert!(!result
            .findings
            .iter()
            .any(|f| f.category == "reentrancy-cei-violation"));
    }

    #[test]
    fn unchecked_call_detection() {
        let source = r#"
            pragma solidity 0.8.21;
            contract X {
              function ping(address target, bytes calldata data) external {
                target.call(data);
              }
            }
        "#;
        let result = audit_from_source(source, AuditSize::Small);
        assert!(result
            .findings
            .iter()
            .any(|f| f.category == "unchecked-external-call"));
    }

    #[test]
    fn floating_pragma_detection() {
        let floating = "pragma solidity ^0.8.0; contract C {}";
        let pinned = "pragma solidity 0.8.21; contract C {}";
        let with_floating = audit_from_source(floating, AuditSize::Small);
        let with_pinned = audit_from_source(pinned, AuditSize::Small);
        assert!(with_floating
            .findings
            .iter()
            .any(|f| f.category == "floating-pragma"));
        assert!(!with_pinned
            .findings
            .iter()
            .any(|f| f.category == "floating-pragma"));
    }

    #[test]
    fn tx_origin_detection() {
        let source = r#"
            pragma solidity 0.8.21;
            contract T {
              address owner;
              function admin() external {
                require(tx.origin == owner);
              }
            }
        "#;
        let result = audit_from_source(source, AuditSize::Small);
        assert!(result
            .findings
            .iter()
            .any(|f| f.category == "tx-origin-auth"));
    }

    #[test]
    fn selfdestruct_detection() {
        let source = r#"
            pragma solidity 0.8.21;
            contract Boom {
              function kill() external {
                selfdestruct(payable(msg.sender));
              }
            }
        "#;
        let result = audit_from_source(source, AuditSize::Small);
        assert!(result
            .findings
            .iter()
            .any(|f| f.category == "selfdestruct-unprotected"));
    }

    #[test]
    fn size_determines_checks() {
        let source = r#"
            pragma solidity 0.8.21;
            contract A {
              uint256[] public values;
              function expensive() public {
                for (uint256 i = 0; i < values.length; i++) {
                  values[i] = i;
                }
              }
            }
        "#;
        let small = audit_from_source(source, AuditSize::Small);
        let large = audit_from_source(source, AuditSize::Large);
        assert!(large.checks_performed.len() > small.checks_performed.len());
        assert!(large
            .findings
            .iter()
            .any(|f| f.category == "gas-griefing-unbounded-loop"));
    }

    #[test]
    fn audit_result_serialization_roundtrip() {
        let source = "pragma solidity 0.8.21; contract C{}";
        let result = audit_from_source(source, AuditSize::Small);
        let raw = serde_json::to_vec(&result).expect("serialize");
        let restored: AuditResult = serde_json::from_slice(&raw).expect("deserialize");
        assert_eq!(restored.contract_hash, result.contract_hash);
        assert_eq!(restored.audit_level, result.audit_level);
    }
}
