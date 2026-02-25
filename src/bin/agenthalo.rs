use nucleusdb::halo::addons;
use nucleusdb::halo::agentpmt;
use nucleusdb::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use nucleusdb::halo::audit::{audit_contract_file, save_audit_result, AuditSize};
use nucleusdb::halo::auth::{
    is_authenticated, load_credentials, oauth_login, resolve_api_key, save_credentials, Credentials,
};
use nucleusdb::halo::circuit::{
    load_or_setup_attestation_keys_with_policy, proof_words_json_array, prove_attestation,
    public_inputs_json_array, verify_attestation_proof,
};
use nucleusdb::halo::circuit_policy::CircuitPolicy;
use nucleusdb::halo::config;
use nucleusdb::halo::detect::AgentType;
use nucleusdb::halo::onchain::{
    deploy_trust_verifier, load_onchain_config_or_default, onchain_config_path, post_attestation,
    query_attestation, save_onchain_config, signer_mode_label, SignerMode,
};
use nucleusdb::halo::pq::{has_wallet, keygen_pq, sign_pq_payload};
use nucleusdb::halo::runner::AgentRunner;
use nucleusdb::halo::schema::{SessionMetadata, SessionStatus};
use nucleusdb::halo::trace::{
    list_sessions, now_unix_secs, record_paid_operation_for_halo, TraceWriter,
};
use nucleusdb::halo::trust::query_trust_score;
use nucleusdb::halo::util::digest_json;
use nucleusdb::halo::x402;
use nucleusdb::halo::{generic_agents_allowed, viewer, wrap};
use nucleusdb::license;
use std::io::{self, Write};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = run(args);
    if let Err(e) = code {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    if args.len() < 2 {
        print_usage();
        return Err("missing command".to_string());
    }

    match args[1].as_str() {
        "run" => cmd_run(&args[2..]),
        "login" => cmd_login(&args[2..]),
        "config" => cmd_config(&args[2..]),
        "status" => cmd_status(&args[2..]),
        "traces" => cmd_traces(&args[2..]),
        "costs" => cmd_costs(&args[2..]),
        "export" => cmd_export(&args[2..]),
        "attest" => cmd_attest(&args[2..]),
        "audit" => cmd_audit(&args[2..]),
        "keygen" => cmd_keygen(&args[2..]),
        "sign" => cmd_sign(&args[2..]),
        "trust" => cmd_trust(&args[2..]),
        "vote" => cmd_vote(&args[2..]),
        "sync" => cmd_sync(&args[2..]),
        "onchain" => cmd_onchain(&args[2..]),
        "protocol" => cmd_protocol(&args[2..]),
        "addon" => cmd_addon(&args[2..]),
        "license" => cmd_license(&args[2..]),
        "x402" => cmd_x402(&args[2..]),
        "wrap" => cmd_wrap(&args[2..]),
        "unwrap" => cmd_unwrap(&args[2..]),
        "version" | "--version" | "-V" => {
            println!("agenthalo 0.2.0");
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => {
            print_usage();
            Err(format!("unknown command: {other}"))
        }
    }
}

fn cmd_run(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "usage: agenthalo run [--agent-name NAME] [--model MODEL] <agent-command> [args...]"
                .to_string(),
        );
    }

    config::ensure_halo_dir()?;
    let creds_path = config::credentials_path();
    if !is_authenticated(&creds_path) {
        return Err(
            "not authenticated. Run `agenthalo login` or set AGENTHALO_API_KEY.".to_string(),
        );
    }

    // Parse --agent-name and --model flags before the command.
    let mut agent_name_override: Option<String> = None;
    let mut model_override: Option<String> = None;
    let mut cmd_start = 0;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--agent-name" {
            if let Some(name) = args.get(i + 1) {
                agent_name_override = Some(name.clone());
                i += 2;
                cmd_start = i;
                continue;
            }
        } else if args[i] == "--model" {
            if let Some(model) = args.get(i + 1) {
                model_override = Some(model.clone());
                i += 2;
                cmd_start = i;
                continue;
            }
        } else {
            break;
        }
        i += 1;
    }

    let cmd_args = &args[cmd_start..];
    if cmd_args.is_empty() {
        return Err(
            "usage: agenthalo run [--agent-name NAME] [--model MODEL] <agent-command> [args...]"
                .to_string(),
        );
    }

    let command = cmd_args[0].clone();
    let mut runner = AgentRunner::new(command.clone(), cmd_args[1..].to_vec());
    if let Some(ref name) = agent_name_override {
        runner = runner.with_agent_name(name);
    }
    if matches!(runner.agent_type(), AgentType::Generic(_)) && !generic_agents_allowed() {
        return Err(
            "custom agent commands are disabled in free tier. Set AGENTHALO_ALLOW_GENERIC=1 to enable paid-tier behavior.".to_string(),
        );
    }

    let db_path = config::db_path();
    let mut writer = TraceWriter::new(&db_path)?;

    let now = now_unix_secs();
    let session_id = format!("sess-{now}-{}", std::process::id());
    let creds = load_credentials(&creds_path).unwrap_or_default();
    let model = model_override.or_else(|| infer_model(cmd_args));
    let meta = SessionMetadata {
        session_id: session_id.clone(),
        agent: runner.agent_type().as_str().to_string(),
        model: model.clone(),
        started_at: now,
        ended_at: None,
        prompt: infer_prompt(cmd_args),
        status: SessionStatus::Running,
        user_id: creds.user_id,
        machine_id: std::env::var("HOSTNAME").ok(),
        puf_digest: None,
    };

    writer.start_session(meta)?;
    let (exit_code, detected_model) = runner.run(&mut writer)?;
    // If the adapter detected a model from the stream, update the session.
    if let Some(ref dm) = detected_model {
        if model.is_none() {
            writer.update_session_model(dm);
        }
    }
    let status = if exit_code == 0 {
        SessionStatus::Completed
    } else {
        SessionStatus::Failed
    };
    let summary = writer.end_session(status)?;

    println!(
        "Recorded session {} events={} cost=${:.4}",
        session_id, summary.event_count, summary.estimated_cost_usd
    );

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

fn cmd_login(args: &[String]) -> Result<(), String> {
    config::ensure_halo_dir()?;
    let creds_path = config::credentials_path();

    let choice = if let Some(first) = args.first() {
        first.to_ascii_lowercase()
    } else {
        println!("How would you like to authenticate?");
        println!("  1. GitHub (recommended)");
        println!("  2. Google");
        println!("  3. API key (manual)");
        print!("> ");
        io::stdout()
            .flush()
            .map_err(|e| format!("flush stdout: {e}"))?;
        read_line_trimmed()?
    };

    let creds = match choice.as_str() {
        "1" | "github" => oauth_login("github")?,
        "2" | "google" => oauth_login("google")?,
        "3" | "api" | "apikey" | "api_key" => {
            print!("Enter API key: ");
            io::stdout()
                .flush()
                .map_err(|e| format!("flush stdout: {e}"))?;
            let key = read_line_trimmed()?;
            if key.trim().is_empty() {
                return Err("API key cannot be empty".to_string());
            }
            Credentials {
                api_key: Some(key),
                oauth_token: None,
                oauth_provider: None,
                user_id: None,
                created_at: now_unix_secs(),
            }
        }
        other => return Err(format!("unknown login mode: {other}")),
    };

    save_credentials(&creds_path, &creds)?;
    println!("Authentication saved: {}", creds_path.display());
    Ok(())
}

fn cmd_config(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "usage: agenthalo config set-key <key> | tool-proxy enable|disable|status|refresh | show"
                .to_string(),
        );
    }
    config::ensure_halo_dir()?;
    let creds_path = config::credentials_path();

    match args[0].as_str() {
        "set-key" => {
            let key = if let Some(k) = args.get(1).cloned() {
                k
            } else {
                print!("Enter API key: ");
                io::stdout()
                    .flush()
                    .map_err(|e| format!("flush stdout: {e}"))?;
                read_line_trimmed()?
            };
            if key.trim().is_empty() {
                return Err("API key cannot be empty".to_string());
            }
            let mut creds = load_credentials(&creds_path).unwrap_or_default();
            creds.api_key = Some(key);
            creds.created_at = now_unix_secs();
            save_credentials(&creds_path, &creds)?;
            println!("API key saved at {}", creds_path.display());
            Ok(())
        }
        "tool-proxy" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "enable" => {
                    let path = agentpmt::agentpmt_config_path();
                    let mut cfg = agentpmt::load_or_default();
                    cfg.enabled = true;
                    cfg.updated_at = now_unix_secs();
                    if let Some(tag) = args.get(2) {
                        cfg.budget_tag = Some(tag.clone());
                    }
                    agentpmt::save_config(&path, &cfg)?;
                    println!("AgentPMT tool proxy enabled.");
                    if let Some(ref tag) = cfg.budget_tag {
                        println!("Budget tag: {tag}");
                    }
                    Ok(())
                }
                "disable" => {
                    let path = agentpmt::agentpmt_config_path();
                    let mut cfg = agentpmt::load_or_default();
                    cfg.enabled = false;
                    cfg.updated_at = now_unix_secs();
                    agentpmt::save_config(&path, &cfg)?;
                    println!("AgentPMT tool proxy disabled.");
                    Ok(())
                }
                "status" => {
                    let cfg = agentpmt::load_or_default();
                    println!("TOOL_PROXY_ENABLED={}", cfg.enabled);
                    println!(
                        "BUDGET_TAG={}",
                        cfg.budget_tag.as_deref().unwrap_or("(none)")
                    );
                    println!(
                        "MCP_ENDPOINT={}",
                        cfg.mcp_endpoint.as_deref().unwrap_or("(default)")
                    );
                    Ok(())
                }
                "refresh" => {
                    let catalog = agentpmt::refresh_tool_catalog()?;
                    println!(
                        "AgentPMT tool catalog refreshed: {} tools -> {}",
                        catalog.tools.len(),
                        agentpmt::tool_catalog_path().display()
                    );
                    Ok(())
                }
                _ => Err(
                    "usage: agenthalo config tool-proxy [enable|disable|status|refresh]"
                        .to_string(),
                ),
            }
        }
        "show" => {
            println!("AGENTHALO_HOME={}", config::halo_dir().display());
            println!("DB_PATH={}", config::db_path().display());
            println!("CREDENTIALS={}", creds_path.display());
            println!("PRICING={}", config::pricing_path().display());
            let has_auth = is_authenticated(&creds_path) || resolve_api_key(&creds_path).is_some();
            println!("AUTHENTICATED={has_auth}");
            let pmt_cfg = agentpmt::load_or_default();
            println!("TOOL_PROXY_ENABLED={}", pmt_cfg.enabled);
            println!(
                "TOOL_PROXY_BUDGET_TAG={}",
                pmt_cfg.budget_tag.as_deref().unwrap_or("(none)")
            );
            println!("PQ_WALLET_PRESENT={}", has_wallet());
            let addons_cfg = addons::load_or_default();
            println!("ADDON_P2PCLAW_ENABLED={}", addons_cfg.p2pclaw_enabled);
            println!(
                "ADDON_AGENTPMT_WORKFLOWS_ENABLED={}",
                addons_cfg.agentpmt_workflows_enabled
            );
            Ok(())
        }
        other => Err(format!("unknown config command: {other}")),
    }
}

fn cmd_traces(args: &[String]) -> Result<(), String> {
    let json_mode = args.iter().any(|a| a == "--json");
    let db_path = config::db_path();
    let session = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());
    if json_mode {
        viewer::print_traces_json(&db_path, session)
    } else {
        viewer::print_traces(&db_path, session)
    }
}

fn cmd_costs(args: &[String]) -> Result<(), String> {
    let monthly = args.iter().any(|a| a == "--month");
    let paid = args.iter().any(|a| a == "--paid");
    let json_mode = args.iter().any(|a| a == "--json");
    let db_path = config::db_path();
    if json_mode && !paid {
        viewer::print_costs_json(&db_path, monthly)
    } else if paid {
        viewer::print_paid_costs(&db_path, monthly)
    } else {
        viewer::print_costs(&db_path, monthly)
    }
}

fn cmd_status(_args: &[String]) -> Result<(), String> {
    let db_path = config::db_path();
    let creds_path = config::credentials_path();
    let json_mode = _args.iter().any(|a| a == "--json");

    if json_mode {
        let has_auth = is_authenticated(&creds_path) || resolve_api_key(&creds_path).is_some();
        let pmt_cfg = agentpmt::load_or_default();

        // Build a combined status JSON with auth + trace overview.
        let sessions = list_sessions(&db_path).unwrap_or_default();
        let session_count = sessions.len();
        let latest = sessions.first().cloned();
        let mut total_cost = 0.0f64;
        let mut total_tokens = 0u64;
        for s in &sessions {
            if let Ok(Some(summary)) =
                nucleusdb::halo::trace::session_summary(&db_path, &s.session_id)
            {
                total_cost += summary.estimated_cost_usd;
                total_tokens += summary.total_input_tokens + summary.total_output_tokens;
            }
        }
        let out = serde_json::json!({
            "authenticated": has_auth,
            "tool_proxy_enabled": pmt_cfg.enabled,
            "session_count": session_count,
            "total_cost_usd": total_cost,
            "total_tokens": total_tokens,
            "latest_session": latest,
            "db_path": db_path.to_string_lossy(),
            "version": "0.2.0",
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out)
                .map_err(|e| format!("serialize status json: {e}"))?
        );
    } else {
        let has_auth = is_authenticated(&creds_path) || resolve_api_key(&creds_path).is_some();
        println!("AgentHALO v0.2.0");
        println!("  Authenticated: {has_auth}");
        viewer::print_status(&db_path, false)?;
    }
    Ok(())
}

fn cmd_export(args: &[String]) -> Result<(), String> {
    let session_id = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .ok_or_else(|| "usage: agenthalo export <session-id> [--out <path>]".to_string())?;
    let out_path = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1));

    let db_path = config::db_path();
    let export = viewer::export_session_json(&db_path, session_id)?;
    let json_str =
        serde_json::to_string_pretty(&export).map_err(|e| format!("serialize export: {e}"))?;

    if let Some(path) = out_path {
        std::fs::write(path, &json_str).map_err(|e| format!("write export to {path}: {e}"))?;
        println!("Exported session {session_id} to {path}");
    } else {
        println!("{json_str}");
    }
    Ok(())
}

fn cmd_attest(args: &[String]) -> Result<(), String> {
    let anonymous = args.iter().any(|a| a == "--anonymous");
    let onchain = args.iter().any(|a| a == "--onchain");
    let requested_session_id = args
        .iter()
        .position(|a| a == "--session")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let db_path = config::db_path();
    let resolved_session_id = resolve_session_id(&db_path, requested_session_id.as_deref())?;
    let op = match (onchain, anonymous) {
        (true, true) => "attest_onchain_anon",
        (true, false) => "attest_onchain",
        (false, true) => "attest_anon",
        (false, false) => "attest",
    };

    let attestation = attest_session(
        &db_path,
        AttestationRequest {
            session_id: resolved_session_id.clone(),
            anonymous,
        },
    );
    match attestation {
        Ok(mut result) => {
            if onchain {
                let cfg = load_onchain_config_or_default();
                let (pk, vk, key_info) =
                    load_or_setup_attestation_keys_with_policy(None, cfg.circuit_policy.clone())?;
                let proof_bundle = prove_attestation(&pk, &result)?;
                let local_verified = verify_attestation_proof(&vk, &proof_bundle)?;
                if !local_verified {
                    return Err("local Groth16 verification failed".to_string());
                }
                let post = post_attestation(&cfg, &proof_bundle, anonymous)?;
                result.proof_type = "groth16-bn254".to_string();
                result.groth16_proof = Some(proof_words_json_array(&proof_bundle));
                result.groth16_public_inputs = Some(public_inputs_json_array(&proof_bundle));
                result.tx_hash = Some(post.tx_hash.clone());
                result.contract_address = Some(post.contract_address.clone());
                result.block_number = post.block_number;
                result.chain = Some(post.chain);
                println!(
                    "On-chain attestation posted. tx_hash={} contract={} (pk={}, vk={}, metadata={}, policy={})",
                    post.tx_hash,
                    post.contract_address,
                    key_info.pk_path,
                    key_info.vk_path,
                    key_info.metadata_path,
                    key_info.policy.as_str()
                );
            }

            let save_path = save_attestation(&resolved_session_id, &result)?;
            println!("Attestation successful.");
            println!("Session: {resolved_session_id}");
            println!("Attestation file: {}", save_path.display());
            println!(
                "{}",
                serde_json::to_string_pretty(&result)
                    .map_err(|e| format!("serialize attestation output: {e}"))?
            );
            record_paid_operation_for_halo(
                op,
                0,
                Some(resolved_session_id),
                Some(result.attestation_digest.clone()),
                true,
                None,
            )?;
            Ok(())
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(
                op,
                0,
                Some(resolved_session_id),
                None,
                false,
                Some(e.clone()),
            );
            Err(format!("attestation failed: {e}"))
        }
    }
}

fn cmd_audit(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "usage: agenthalo audit <contract.sol> [--size small|medium|large]".to_string(),
        );
    }
    let contract = &args[0];
    let size_name = args
        .iter()
        .position(|a| a == "--size")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("small");
    let size = AuditSize::parse(size_name)?;

    let op = match size {
        AuditSize::Small => "audit_small",
        AuditSize::Medium => "audit_medium",
        AuditSize::Large => "audit_large",
    };

    let audit = audit_contract_file(Path::new(contract), size);
    match audit {
        Ok(result) => {
            let save_path = save_audit_result(&result)?;
            println!("Audit completed: {}", result.contract_path);
            println!(
                "Findings: {} | Risk score: {:.3} | Digest: {}",
                result.findings.len(),
                result.risk_score,
                result.attestation_digest
            );
            if result.findings.is_empty() {
                println!("No findings detected.");
            } else {
                println!("Findings:");
                for finding in &result.findings {
                    println!(
                        "  - [{:?}] {}: {} ({})",
                        finding.severity,
                        finding.category,
                        finding.description,
                        finding
                            .line_range
                            .map(|(a, b)| format!("lines {a}-{b}"))
                            .unwrap_or_else(|| "line n/a".to_string())
                    );
                }
            }
            println!("Audit file: {}", save_path.display());
            record_paid_operation_for_halo(
                op,
                0,
                None,
                Some(result.contract_hash.clone()),
                true,
                None,
            )?;
            Ok(())
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, 0, None, None, false, Some(e.clone()));
            Err(format!("audit failed: {e}"))
        }
    }
}

fn cmd_keygen(args: &[String]) -> Result<(), String> {
    if !args.iter().any(|a| a == "--pq") {
        return Err("usage: agenthalo keygen --pq [--force]".to_string());
    }
    let force = args.iter().any(|a| a == "--force");
    let result = keygen_pq(force)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result)
            .map_err(|e| format!("serialize keygen output: {e}"))?
    );
    Ok(())
}

fn cmd_sign(args: &[String]) -> Result<(), String> {
    if !args.iter().any(|a| a == "--pq") {
        return Err("usage: agenthalo sign --pq (--message TEXT | --file PATH)".to_string());
    }
    if !has_wallet() {
        return Err(
            "no PQ wallet found. Run: agenthalo keygen --pq (or --force to rotate)".to_string(),
        );
    }

    let message_arg = args
        .iter()
        .position(|a| a == "--message")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let file_arg = args
        .iter()
        .position(|a| a == "--file")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let (payload, payload_kind, payload_hint) = match (message_arg, file_arg) {
        (Some(_), Some(_)) => {
            return Err("choose one payload source: --message or --file".to_string());
        }
        (Some(m), None) => (
            m.into_bytes(),
            "message".to_string(),
            Some("inline".to_string()),
        ),
        (None, Some(path)) => {
            let bytes =
                std::fs::read(&path).map_err(|e| format!("read payload file {}: {e}", path))?;
            (bytes, "file".to_string(), Some(path))
        }
        (None, None) => {
            return Err("usage: agenthalo sign --pq (--message TEXT | --file PATH)".to_string());
        }
    };

    let op = "sign_pq";
    match sign_pq_payload(&payload, &payload_kind, payload_hint) {
        Ok((envelope, save_path)) => {
            println!("PQ signing successful.");
            println!("Signature file: {}", save_path.display());
            println!(
                "{}",
                serde_json::to_string_pretty(&envelope)
                    .map_err(|e| format!("serialize signature output: {e}"))?
            );
            record_paid_operation_for_halo(
                op,
                0,
                None,
                Some(envelope.signature_digest.clone()),
                true,
                None,
            )?;
            Ok(())
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, 0, None, None, false, Some(e.clone()));
            Err(format!("signing failed: {e}"))
        }
    }
}

fn cmd_trust(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("query");
    match sub {
        "query" | "score" => {
            let session_id = args
                .iter()
                .position(|a| a == "--session")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let op = "trust_query";

            let db_path = config::db_path();
            match query_trust_score(&db_path, session_id.as_deref()) {
                Ok(score) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&score)
                            .map_err(|e| format!("serialize trust output: {e}"))?
                    );
                    record_paid_operation_for_halo(
                        op,
                        0,
                        session_id,
                        Some(score.digest.clone()),
                        true,
                        None,
                    )?;
                    Ok(())
                }
                Err(e) => {
                    let _ = record_paid_operation_for_halo(
                        op,
                        0,
                        session_id,
                        None,
                        false,
                        Some(e.clone()),
                    );
                    Err(format!("trust query failed: {e}"))
                }
            }
        }
        _ => Err("usage: agenthalo trust [query|score] [--session ID]".to_string()),
    }
}

fn cmd_vote(args: &[String]) -> Result<(), String> {
    let proposal_id = args
        .iter()
        .position(|a| a == "--proposal")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| {
            "usage: agenthalo vote --proposal <id> --choice <yes|no|abstain> [--reason TEXT]"
                .to_string()
        })?;
    let choice = args
        .iter()
        .position(|a| a == "--choice")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| {
            "usage: agenthalo vote --proposal <id> --choice <yes|no|abstain> [--reason TEXT]"
                .to_string()
        })?;
    if !matches!(choice.as_str(), "yes" | "no" | "abstain") {
        return Err("choice must be yes, no, or abstain".to_string());
    }
    let reason = args
        .iter()
        .position(|a| a == "--reason")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let op = "vote";
    let vote_id = uuid::Uuid::new_v4().to_string();
    let payload = serde_json::json!({
        "vote_id": vote_id,
        "proposal_id": proposal_id,
        "choice": choice,
        "reason": reason,
        "timestamp": now_unix_secs()
    });
    let result_digest = digest_json("agenthalo.vote.v1", &payload)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "ok",
            "operation": "vote",
            "result_digest": result_digest,
            "vote": payload
        }))
        .map_err(|e| format!("serialize vote output: {e}"))?
    );
    record_paid_operation_for_halo(op, 0, None, Some(result_digest), true, None)?;
    Ok(())
}

fn cmd_sync(args: &[String]) -> Result<(), String> {
    let target = args
        .iter()
        .position(|a| a == "--target")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "cloudflare".to_string());
    let op = "sync";

    let db_path = config::db_path();
    let synced_sessions = list_sessions(&db_path)?.len() as u64;
    let payload = serde_json::json!({
        "sync_id": uuid::Uuid::new_v4().to_string(),
        "target": target,
        "sessions_considered": synced_sessions,
        "timestamp": now_unix_secs(),
        "mode": "delta-sync"
    });
    let result_digest = digest_json("agenthalo.sync.v1", &payload)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "ok",
            "operation": "sync",
            "result_digest": result_digest,
            "sync": payload
        }))
        .map_err(|e| format!("serialize sync output: {e}"))?
    );
    record_paid_operation_for_halo(op, 0, None, Some(result_digest), true, None)?;
    Ok(())
}

fn cmd_onchain(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "config" => {
            let mut cfg = load_onchain_config_or_default();
            if let Some(v) = args
                .iter()
                .position(|a| a == "--rpc-url")
                .and_then(|i| args.get(i + 1))
            {
                cfg.rpc_url = v.to_string();
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--chain-id")
                .and_then(|i| args.get(i + 1))
            {
                cfg.chain_id = v
                    .parse::<u64>()
                    .map_err(|e| format!("invalid --chain-id: {e}"))?;
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--contract")
                .and_then(|i| args.get(i + 1))
            {
                cfg.contract_address = v.to_string();
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--private-key-env")
                .and_then(|i| args.get(i + 1))
            {
                cfg.private_key_env = v.to_string();
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--signer-mode")
                .and_then(|i| args.get(i + 1))
            {
                cfg.signer_mode = match v.trim().to_ascii_lowercase().as_str() {
                    "private_key_env" | "private-key-env" | "private" => SignerMode::PrivateKeyEnv,
                    "keystore" => SignerMode::Keystore,
                    other => {
                        return Err(format!(
                            "invalid --signer-mode `{other}` (expected private_key_env|keystore)"
                        ))
                    }
                };
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--keystore-path")
                .and_then(|i| args.get(i + 1))
            {
                cfg.keystore_path = Some(v.to_string());
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--keystore-password-file")
                .and_then(|i| args.get(i + 1))
            {
                cfg.keystore_password_file = Some(v.to_string());
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--circuit-policy")
                .and_then(|i| args.get(i + 1))
            {
                cfg.circuit_policy = CircuitPolicy::parse(v)?;
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--verifier")
                .and_then(|i| args.get(i + 1))
            {
                cfg.verifier_address = v.to_string();
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--usdc")
                .and_then(|i| args.get(i + 1))
            {
                cfg.usdc_address = v.to_string();
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--treasury")
                .and_then(|i| args.get(i + 1))
            {
                cfg.treasury_address = v.to_string();
            }
            if let Some(v) = args
                .iter()
                .position(|a| a == "--fee-wei")
                .and_then(|i| args.get(i + 1))
            {
                cfg.fee_wei = v
                    .parse::<u64>()
                    .map_err(|e| format!("invalid --fee-wei: {e}"))?;
            }
            save_onchain_config(&onchain_config_path(), &cfg)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&cfg)
                    .map_err(|e| format!("serialize onchain config: {e}"))?
            );
            Ok(())
        }
        "deploy" => {
            let mut cfg = load_onchain_config_or_default();
            let deployed = deploy_trust_verifier(&cfg)?;
            cfg.contract_address = deployed.clone();
            save_onchain_config(&onchain_config_path(), &cfg)?;
            println!("Deployed TrustVerifier at {deployed}");
            Ok(())
        }
        "verify" => {
            let digest = args.get(1).ok_or_else(|| {
                "usage: agenthalo onchain verify <attestation-digest>".to_string()
            })?;
            let cfg = load_onchain_config_or_default();
            let status = query_attestation(&cfg, digest)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "config": {
                        "rpc_url": cfg.rpc_url,
                        "chain_id": cfg.chain_id,
                        "chain_name": cfg.chain_name,
                        "contract_address": cfg.contract_address,
                        "signer_mode": signer_mode_label(&cfg),
                        "private_key_env": cfg.private_key_env,
                        "keystore_path": cfg.keystore_path,
                        "keystore_password_file": cfg.keystore_password_file,
                        "circuit_policy": cfg.circuit_policy.as_str(),
                    },
                    "status": status
                }))
                .map_err(|e| format!("serialize onchain verify output: {e}"))?
            );
            Ok(())
        }
        "status" => {
            let cfg = load_onchain_config_or_default();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "rpc_url": cfg.rpc_url,
                    "chain_id": cfg.chain_id,
                    "chain_name": cfg.chain_name,
                    "contract_address": cfg.contract_address,
                    "signer_mode": signer_mode_label(&cfg),
                    "private_key_env": cfg.private_key_env,
                    "keystore_path": cfg.keystore_path,
                    "keystore_password_file": cfg.keystore_password_file,
                    "circuit_policy": cfg.circuit_policy.as_str(),
                    "verifier_address": cfg.verifier_address,
                    "usdc_address": cfg.usdc_address,
                    "treasury_address": cfg.treasury_address,
                    "fee_wei": cfg.fee_wei
                }))
                .map_err(|e| format!("serialize onchain status: {e}"))?
            );
            Ok(())
        }
        _ => Err("usage: agenthalo onchain [config|deploy|verify|status] ...".to_string()),
    }
}

fn cmd_protocol(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    match sub {
        "privacy-pool-create" => cmd_protocol_privacy_pool_create(&args[1..]),
        "privacy-pool-withdraw" => cmd_protocol_privacy_pool_withdraw(&args[1..]),
        "pq-bridge-transfer" => cmd_protocol_bridge_transfer(&args[1..]),
        _ => Err("usage: agenthalo protocol [privacy-pool-create|privacy-pool-withdraw|pq-bridge-transfer] ...".to_string()),
    }
}

fn cmd_protocol_privacy_pool_create(args: &[String]) -> Result<(), String> {
    if !addons::is_enabled("agentpmt-workflows")? {
        return Err(
            "agentpmt-workflows add-on is required. Run: agenthalo addon enable agentpmt-workflows"
                .to_string(),
        );
    }
    let chain = args
        .iter()
        .position(|a| a == "--chain")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "base-sepolia".to_string());
    let asset = args
        .iter()
        .position(|a| a == "--asset")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "USDC".to_string());
    let denomination = args
        .iter()
        .position(|a| a == "--denomination")
        .and_then(|i| args.get(i + 1))
        .ok_or_else(|| {
            "usage: agenthalo protocol privacy-pool-create --denomination <u64> [--chain NAME] [--asset SYMBOL]"
                .to_string()
        })?
        .parse::<u64>()
        .map_err(|e| format!("invalid denomination: {e}"))?;

    let op = "privacy_pool_create";
    let payload = serde_json::json!({
        "pool_id": format!("pool-{}", uuid::Uuid::new_v4()),
        "chain": chain,
        "asset": asset,
        "denomination": denomination,
        "timestamp": now_unix_secs(),
        "status": "created"
    });
    let result_digest = digest_json("agenthalo.privacy_pool.create.v1", &payload)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "ok",
            "operation": op,
            "result_digest": result_digest,
            "pool": payload
        }))
        .map_err(|e| format!("serialize pool create output: {e}"))?
    );
    record_paid_operation_for_halo(op, 0, None, Some(result_digest), true, None)?;
    Ok(())
}

fn cmd_protocol_privacy_pool_withdraw(args: &[String]) -> Result<(), String> {
    if !addons::is_enabled("agentpmt-workflows")? {
        return Err(
            "agentpmt-workflows add-on is required. Run: agenthalo addon enable agentpmt-workflows"
                .to_string(),
        );
    }
    let pool_id = args
        .iter()
        .position(|a| a == "--pool-id")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| {
            "usage: agenthalo protocol privacy-pool-withdraw --pool-id <id> --recipient <addr> [--amount u64]"
                .to_string()
        })?;
    let recipient = args
        .iter()
        .position(|a| a == "--recipient")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| {
            "usage: agenthalo protocol privacy-pool-withdraw --pool-id <id> --recipient <addr> [--amount u64]"
                .to_string()
        })?;
    let amount = args
        .iter()
        .position(|a| a == "--amount")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|e| format!("invalid amount: {e}"))?
        .unwrap_or(1);

    let op = "privacy_pool_withdraw";
    let payload = serde_json::json!({
        "withdrawal_id": format!("wd-{}", uuid::Uuid::new_v4()),
        "pool_id": pool_id,
        "recipient": recipient,
        "amount": amount,
        "timestamp": now_unix_secs(),
        "status": "submitted"
    });
    let result_digest = digest_json("agenthalo.privacy_pool.withdraw.v1", &payload)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "ok",
            "operation": op,
            "result_digest": result_digest,
            "withdrawal": payload
        }))
        .map_err(|e| format!("serialize pool withdraw output: {e}"))?
    );
    record_paid_operation_for_halo(op, 0, None, Some(result_digest), true, None)?;
    Ok(())
}

fn cmd_protocol_bridge_transfer(args: &[String]) -> Result<(), String> {
    if !addons::is_enabled("p2pclaw")? {
        return Err("p2pclaw add-on is required. Run: agenthalo addon enable p2pclaw".to_string());
    }
    let from_chain = args
        .iter()
        .position(|a| a == "--from")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| {
            "usage: agenthalo protocol pq-bridge-transfer --from <chain> --to <chain> --asset <symbol> --amount <u64> --recipient <addr>"
                .to_string()
        })?;
    let to_chain = args
        .iter()
        .position(|a| a == "--to")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| {
            "usage: agenthalo protocol pq-bridge-transfer --from <chain> --to <chain> --asset <symbol> --amount <u64> --recipient <addr>"
                .to_string()
        })?;
    let asset = args
        .iter()
        .position(|a| a == "--asset")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| {
            "usage: agenthalo protocol pq-bridge-transfer --from <chain> --to <chain> --asset <symbol> --amount <u64> --recipient <addr>"
                .to_string()
        })?;
    let amount = args
        .iter()
        .position(|a| a == "--amount")
        .and_then(|i| args.get(i + 1))
        .ok_or_else(|| {
            "usage: agenthalo protocol pq-bridge-transfer --from <chain> --to <chain> --asset <symbol> --amount <u64> --recipient <addr>"
                .to_string()
        })?
        .parse::<u64>()
        .map_err(|e| format!("invalid amount: {e}"))?;
    let recipient = args
        .iter()
        .position(|a| a == "--recipient")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| {
            "usage: agenthalo protocol pq-bridge-transfer --from <chain> --to <chain> --asset <symbol> --amount <u64> --recipient <addr>"
                .to_string()
        })?;

    let op = "pq_bridge_transfer";
    let payload = serde_json::json!({
        "transfer_id": format!("xfer-{}", uuid::Uuid::new_v4()),
        "from_chain": from_chain,
        "to_chain": to_chain,
        "asset": asset,
        "amount": amount,
        "recipient": recipient,
        "timestamp": now_unix_secs(),
        "status": "submitted"
    });
    let result_digest = digest_json("agenthalo.pq_bridge.transfer.v1", &payload)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "ok",
            "operation": op,
            "result_digest": result_digest,
            "transfer": payload
        }))
        .map_err(|e| format!("serialize bridge transfer output: {e}"))?
    );
    record_paid_operation_for_halo(op, 0, None, Some(result_digest), true, None)?;
    Ok(())
}

fn cmd_addon(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "list" => {
            let pmt_cfg = agentpmt::load_or_default();
            let pmt = if pmt_cfg.enabled {
                "ENABLED"
            } else {
                "DISABLED"
            };
            let cfg = addons::load_or_default();
            let p2pclaw = if cfg.p2pclaw_enabled {
                "ENABLED"
            } else {
                "DISABLED"
            };
            let workflows = if cfg.agentpmt_workflows_enabled {
                "ENABLED"
            } else {
                "DISABLED"
            };
            println!("  tool-proxy (AgentPMT)  {pmt}   (third-party tool access via AgentPMT)");
            println!("  p2pclaw               {p2pclaw}        (optional marketplace)");
            println!("  agentpmt-workflows    {workflows}        (optional challenges)");
            Ok(())
        }
        "enable" => {
            let name = args
                .get(1)
                .ok_or_else(|| "usage: agenthalo addon enable <name>".to_string())?;
            match name.as_str() {
                "p2pclaw" => {
                    addons::set_enabled("p2pclaw", true)?;
                    println!("Enabled p2pclaw add-on.");
                    Ok(())
                }
                "agentpmt-workflows" => {
                    addons::set_enabled("agentpmt-workflows", true)?;
                    println!("Enabled agentpmt-workflows add-on.");
                    Ok(())
                }
                "tool-proxy" => {
                    let path = agentpmt::agentpmt_config_path();
                    let mut cfg = agentpmt::load_or_default();
                    cfg.enabled = true;
                    cfg.updated_at = now_unix_secs();
                    agentpmt::save_config(&path, &cfg)?;
                    println!("Enabled AgentPMT tool proxy.");
                    Ok(())
                }
                _ => Err(format!(
                    "unknown add-on: {name}. Available: p2pclaw, agentpmt-workflows, tool-proxy"
                )),
            }
        }
        "disable" => {
            let name = args
                .get(1)
                .ok_or_else(|| "usage: agenthalo addon disable <name>".to_string())?;
            match name.as_str() {
                "p2pclaw" => {
                    addons::set_enabled("p2pclaw", false)?;
                    println!("Disabled {name} add-on.");
                    Ok(())
                }
                "agentpmt-workflows" => {
                    addons::set_enabled("agentpmt-workflows", false)?;
                    println!("Disabled {name} add-on.");
                    Ok(())
                }
                "tool-proxy" => {
                    let path = agentpmt::agentpmt_config_path();
                    let mut cfg = agentpmt::load_or_default();
                    cfg.enabled = false;
                    cfg.updated_at = now_unix_secs();
                    agentpmt::save_config(&path, &cfg)?;
                    println!("Disabled AgentPMT tool proxy.");
                    Ok(())
                }
                _ => Err(format!(
                    "unknown add-on: {name}. Available: p2pclaw, agentpmt-workflows, tool-proxy"
                )),
            }
        }
        _ => Err("usage: agenthalo addon [list|enable|disable] [name]".to_string()),
    }
}

fn cmd_license(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "status" => {
            println!("License: Community (free)");
            println!("To upgrade, obtain a CAB certificate from the AgentPMT marketplace.");
            println!("Then: agenthalo license verify <path-to-certificate.json>");
            Ok(())
        }
        "verify" => {
            let path = args.get(1).ok_or_else(|| {
                "usage: agenthalo license verify <path-to-certificate.json>".to_string()
            })?;
            match license::load_and_verify(Path::new(path)) {
                Ok(level) => {
                    println!("License certificate valid.");
                    println!("License level: {}", level.label());
                    Ok(())
                }
                Err(e) => Err(format!("license verification failed: {e}")),
            }
        }
        _ => Err("usage: agenthalo license [status|verify <certificate.json>]".to_string()),
    }
}

fn cmd_x402(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "status" => {
            let cfg = x402::load_x402_config();
            println!("x402direct integration");
            println!("  enabled:          {}", cfg.enabled);
            println!("  preferred network: {}", cfg.preferred_network);
            println!("  max auto-approve:  {} USDC", cfg.max_auto_approve as f64 / 1_000_000.0);
            if let Some(upc) = &cfg.upc_contract_address {
                println!("  UPC contract:      {upc}");
            } else {
                println!("  UPC contract:      (not configured)");
            }
            println!("\nSupported networks:");
            println!("  Base mainnet  (eip155:8453)  USDC: {}", x402::BASE_MAINNET.usdc_address);
            println!("  Base Sepolia  (eip155:84532) USDC: {}", x402::BASE_SEPOLIA.usdc_address);
            Ok(())
        }
        "enable" => {
            let mut cfg = x402::load_x402_config();
            cfg.enabled = true;
            x402::save_x402_config(&cfg)?;
            println!("x402direct integration enabled.");
            Ok(())
        }
        "disable" => {
            let mut cfg = x402::load_x402_config();
            cfg.enabled = false;
            x402::save_x402_config(&cfg)?;
            println!("x402direct integration disabled.");
            Ok(())
        }
        "config" => {
            // Parse --upc-contract, --network, --max-auto-approve flags.
            let mut cfg = x402::load_x402_config();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--upc-contract" => {
                        i += 1;
                        cfg.upc_contract_address =
                            Some(args.get(i).ok_or("--upc-contract requires value")?.clone());
                    }
                    "--network" => {
                        i += 1;
                        let net = args.get(i).ok_or("--network requires value")?;
                        if !matches!(net.as_str(), "base" | "base-sepolia") {
                            return Err(format!("unknown network: {net} (expected base or base-sepolia)"));
                        }
                        cfg.preferred_network = net.clone();
                    }
                    "--max-auto-approve" => {
                        i += 1;
                        let v: u64 = args
                            .get(i)
                            .ok_or("--max-auto-approve requires value")?
                            .parse()
                            .map_err(|_| "--max-auto-approve must be integer (base units)")?;
                        cfg.max_auto_approve = v;
                    }
                    other => return Err(format!("unknown x402 config flag: {other}")),
                }
                i += 1;
            }
            x402::save_x402_config(&cfg)?;
            println!("x402 config saved.");
            Ok(())
        }
        "check" => {
            // Read JSON from stdin or --body argument.
            let body = if let Some(pos) = args.iter().position(|a| a == "--body") {
                args.get(pos + 1)
                    .ok_or("--body requires value")?
                    .clone()
            } else {
                let mut buf = String::new();
                io::stdin()
                    .read_line(&mut buf)
                    .map_err(|e| format!("read stdin: {e}"))?;
                buf
            };
            let req = x402::parse_x402_response(&body)?;
            let result = x402::validate_payment_request(&req);
            let out = serde_json::to_string_pretty(&result)
                .map_err(|e| format!("serialize result: {e}"))?;
            println!("{out}");
            Ok(())
        }
        "pay" => {
            let body = if let Some(pos) = args.iter().position(|a| a == "--body") {
                args.get(pos + 1)
                    .ok_or("--body requires value")?
                    .clone()
            } else {
                let mut buf = String::new();
                io::stdin()
                    .read_line(&mut buf)
                    .map_err(|e| format!("read stdin: {e}"))?;
                buf
            };
            let option_id = args
                .iter()
                .position(|a| a == "--option")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());

            let req = x402::parse_x402_response(&body)?;
            let cfg = x402::load_x402_config();
            let result = x402::execute_payment(&cfg, &req, option_id)?;
            let out = serde_json::to_string_pretty(&serde_json::json!({
                "status": "ok",
                "payment": result,
            }))
            .map_err(|e| format!("serialize result: {e}"))?;
            println!("{out}");
            record_paid_operation_for_halo(
                "x402_pay",
                result.amount,
                None,
                Some(result.transaction_hash),
                true,
                None,
            )?;
            Ok(())
        }
        "balance" => {
            let cfg = x402::load_x402_config();
            if !cfg.enabled {
                return Err("x402 payments are disabled. Run: agenthalo x402 enable".to_string());
            }
            let (address, balance) = x402::check_usdc_balance(&cfg)?;
            println!("x402 Wallet");
            println!("  address: {address}");
            println!(
                "  balance: {:.6} USDC ({} base units)",
                balance as f64 / 1_000_000.0,
                balance
            );
            println!("  network: {}", cfg.preferred_network);
            Ok(())
        }
        _ => Err(
            "usage: agenthalo x402 [status|enable|disable|config|check|pay|balance]\n  config flags: --upc-contract <addr> --network <base|base-sepolia> --max-auto-approve <units>\n  pay flags: --body <json> [--option <id>]\n  balance: no flags required"
                .to_string(),
        ),
    }
}

fn cmd_wrap(args: &[String]) -> Result<(), String> {
    let rc = wrap::detect_shell_rc();
    if args.first().map(|s| s.as_str()) == Some("--all") {
        for agent in ["claude", "codex", "gemini"] {
            wrap::wrap_agent(agent, &rc)?;
        }
        println!("Wrapped claude/codex/gemini in {}", rc.display());
        return Ok(());
    }

    let agent = args
        .first()
        .map(|s| s.as_str())
        .ok_or_else(|| "usage: agenthalo wrap <agent>|--all".to_string())?;
    wrap::wrap_agent(agent, &rc)?;
    println!("Wrapped {agent} in {}", rc.display());
    Ok(())
}

fn cmd_unwrap(args: &[String]) -> Result<(), String> {
    let rc = wrap::detect_shell_rc();
    if args.first().map(|s| s.as_str()) == Some("--all") {
        for agent in ["claude", "codex", "gemini"] {
            wrap::unwrap_agent(agent, &rc)?;
        }
        println!("Unwrapped claude/codex/gemini in {}", rc.display());
        return Ok(());
    }

    let agent = args
        .first()
        .map(|s| s.as_str())
        .ok_or_else(|| "usage: agenthalo unwrap <agent>|--all".to_string())?;
    wrap::unwrap_agent(agent, &rc)?;
    println!("Unwrapped {agent} in {}", rc.display());
    Ok(())
}

fn infer_model(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i + 1 < args.len() {
        let k = &args[i];
        if k == "--model" || k == "-m" {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

fn infer_prompt(args: &[String]) -> Option<String> {
    if args.len() <= 1 {
        return None;
    }
    let joined = args[1..].join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn read_line_trimmed() -> Result<String, String> {
    let mut s = String::new();
    io::stdin()
        .read_line(&mut s)
        .map_err(|e| format!("read stdin: {e}"))?;
    Ok(s.trim().to_string())
}

fn print_usage() {
    println!(
        "agenthalo 0.2.0\n\nCommands:\n  run [--agent-name NAME] [--model MODEL] <agent> [args...]\n                             Run agent with recording (model auto-detected from stream)\n  login [github|google|api]  Authenticate via OAuth or API key\n  config set-key <key>       Save API key\n  config tool-proxy [enable|disable|status|refresh]\n                             Manage AgentPMT tool proxy integration\n  config show                Show effective config\n  status [--json]            Show recording status, session count, and total cost\n  traces [session-id] [--json]\n                             List sessions or show session detail\n  costs [--month] [--paid] [--json]\n                             Show model costs or operation usage\n  export <session-id> [--out <path>]\n                             Export full session as standalone JSON\n  attest [--session ID] [--anonymous] [--onchain]\n                             Build attestation (Merkle default, Groth16+onchain when --onchain)\n  audit <contract.sol> [--size small|medium|large]\n                             Run Solidity static audit\n  keygen --pq [--force]      Generate/rotate ML-DSA wallet\n  sign --pq (--message TEXT | --file PATH)\n                             Create detached ML-DSA signature\n  trust [query|score] [--session ID]\n                             Query trust score\n  vote --proposal ID --choice yes|no|abstain [--reason TEXT]\n                             Submit governance vote intent\n  sync [--target cloudflare|local]\n                             Run sync operation\n  onchain [config|deploy|verify|status] ...\n                             Config fields: --signer-mode private_key_env|keystore --keystore-path --keystore-password-file --circuit-policy dev|production\n  protocol privacy-pool-create --denomination <u64> [--chain NAME] [--asset SYMBOL]\n                             Create privacy pool request (workflows add-on)\n  protocol privacy-pool-withdraw --pool-id <id> --recipient <addr> [--amount u64]\n                             Submit privacy withdrawal request (workflows add-on)\n  protocol pq-bridge-transfer --from <chain> --to <chain> --asset <symbol> --amount <u64> --recipient <addr>\n                             Submit PQ bridge transfer request (p2pclaw add-on)\n  license [status|verify <certificate.json>]\n                             Check or verify CAB license certificate\n  x402 [status|enable|disable|config|check|pay|balance]\n                             x402direct stablecoin payment integration\n                             config flags: --upc-contract <addr> --network <base|base-sepolia> --max-auto-approve <units>\n                             pay: --body <402-json> [--option <id>]\n                             balance: check USDC wallet balance\n  addon [list|enable|disable] [name]\n                             Manage optional add-ons (p2pclaw, agentpmt-workflows, tool-proxy)\n  wrap <agent>|--all         Add shell aliases\n  unwrap <agent>|--all       Remove shell aliases\n  version                    Print version\n  help                       Show this help\n\nEnvironment:\n  AGENTHALO_HOME\n  AGENTHALO_DB_PATH\n  AGENTHALO_API_KEY\n  AGENTHALO_ALLOW_GENERIC=1   Enable paid-tier custom agent wrapping\n  AGENTHALO_NO_TELEMETRY=1    (default behavior: zero telemetry)\n  AGENTHALO_ONCHAIN_STUB=1    Disable real RPC posting and return deterministic stub tx hashes"
    );
}
