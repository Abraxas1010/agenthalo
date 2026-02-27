use nucleusdb::dashboard;
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
        "vault" => cmd_vault(&args[2..]),
        "identity" => cmd_identity(&args[2..]),
        "x402" => cmd_x402(&args[2..]),
        "wrap" => cmd_wrap(&args[2..]),
        "unwrap" => cmd_unwrap(&args[2..]),
        "setup" => cmd_setup(&args[2..]),
        "dashboard" => cmd_dashboard(&args[2..]),
        "doctor" => cmd_doctor(&args[2..]),
        "version" | "--version" | "-V" => {
            println!("agenthalo 0.3.0");
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
            "usage: agenthalo config set-key <key> | set-agentpmt-key <key> | tool-proxy enable|disable|status|refresh|endpoint <url>|clear-endpoint | show".to_string(),
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
        "set-agentpmt-key" => {
            let key = if let Some(k) = args.get(1).cloned() {
                k
            } else {
                print!("Enter AgentPMT bearer token: ");
                io::stdout()
                    .flush()
                    .map_err(|e| format!("flush stdout: {e}"))?;
                read_line_trimmed()?
            };
            if key.trim().is_empty() {
                return Err("AgentPMT token cannot be empty".to_string());
            }
            let path = agentpmt::agentpmt_config_path();
            let mut cfg = agentpmt::load_or_default();
            cfg.auth_token = Some(key);
            cfg.updated_at = now_unix_secs();
            agentpmt::save_config(&path, &cfg)?;
            println!("AgentPMT bearer token saved at {}", path.display());
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
                        agentpmt::resolved_mcp_endpoint(&cfg)
                    );
                    println!("AUTH_CONFIGURED={}", agentpmt::has_bearer_token());
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
                "endpoint" => {
                    let endpoint = args
                        .get(2)
                        .ok_or_else(|| "usage: agenthalo config tool-proxy endpoint <url>".to_string())?;
                    if endpoint.trim().is_empty() {
                        return Err("endpoint cannot be empty".to_string());
                    }
                    let path = agentpmt::agentpmt_config_path();
                    let mut cfg = agentpmt::load_or_default();
                    cfg.mcp_endpoint = Some(endpoint.trim().to_string());
                    cfg.updated_at = now_unix_secs();
                    agentpmt::save_config(&path, &cfg)?;
                    println!("AgentPMT MCP endpoint set to {}", endpoint.trim());
                    Ok(())
                }
                "clear-endpoint" => {
                    let path = agentpmt::agentpmt_config_path();
                    let mut cfg = agentpmt::load_or_default();
                    cfg.mcp_endpoint = None;
                    cfg.updated_at = now_unix_secs();
                    agentpmt::save_config(&path, &cfg)?;
                    println!("AgentPMT MCP endpoint override cleared.");
                    Ok(())
                }
                _ => Err(
                    "usage: agenthalo config tool-proxy [enable|disable|status|refresh|endpoint <url>|clear-endpoint]".to_string(),
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
            "version": "0.3.0",
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out)
                .map_err(|e| format!("serialize status json: {e}"))?
        );
    } else {
        let has_auth = is_authenticated(&creds_path) || resolve_api_key(&creds_path).is_some();
        println!("AgentHALO v0.3.0");
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

fn cmd_vault(args: &[String]) -> Result<(), String> {
    use nucleusdb::halo::config;
    use nucleusdb::halo::vault;

    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");
    let pq_wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();

    if !pq_wallet_path.exists() {
        return Err("PQ wallet not found. Generate one first: agenthalo keygen --pq".to_string());
    }

    let v =
        vault::Vault::open(&pq_wallet_path, &vault_path).map_err(|e| format!("open vault: {e}"))?;

    match sub {
        "list" => {
            let keys = v.list_keys().map_err(|e| format!("list keys: {e}"))?;
            println!("Vault keys:");
            for k in &keys {
                let status = if k.configured {
                    "configured"
                } else {
                    "not set"
                };
                let tested = if k.tested { " (tested)" } else { "" };
                println!(
                    "  {:<14} {:<18} {}{}",
                    k.provider, k.env_var, status, tested
                );
            }
            Ok(())
        }
        "set" => {
            let provider = args
                .get(1)
                .ok_or("usage: agenthalo vault set <provider> [key]")?;
            let key = if let Some(k) = args.get(2) {
                k.clone()
            } else {
                // Read from stdin.
                let mut buf = String::new();
                std::io::stdin()
                    .read_line(&mut buf)
                    .map_err(|e| format!("read key from stdin: {e}"))?;
                buf.trim().to_string()
            };
            if key.is_empty() {
                return Err("key must not be empty".to_string());
            }
            let env_var = vault::provider_default_env_var(provider);
            v.set_key(provider, &env_var, &key)
                .map_err(|e| format!("set key: {e}"))?;
            println!("Vault: {provider} key stored (env_var={env_var})");
            Ok(())
        }
        "delete" => {
            let provider = args
                .get(1)
                .ok_or("usage: agenthalo vault delete <provider>")?;
            v.delete_key(provider)
                .map_err(|e| format!("delete key: {e}"))?;
            println!("Vault: {provider} key deleted");
            Ok(())
        }
        "test" => {
            let provider = args
                .get(1)
                .ok_or("usage: agenthalo vault test <provider>")?;
            let key = v.get_key(provider).map_err(|e| format!("get key: {e}"))?;
            println!("Testing {provider} key...");
            // Mask key for display.
            let masked = if key.len() > 8 {
                format!("{}...{}", &key[..4], &key[key.len() - 4..])
            } else {
                "****".to_string()
            };
            println!("  Key: {masked}");
            println!("  (Full test requires running dashboard)");
            Ok(())
        }
        _ => Err(
            "usage: agenthalo vault [list|set <provider> [key]|delete <provider>|test <provider>]"
                .to_string(),
        ),
    }
}

fn supported_social_providers() -> &'static [&'static str] {
    &[
        "google",
        "github",
        "microsoft",
        "discord",
        "apple",
        "facebook",
    ]
}

fn is_supported_social_provider(provider: &str) -> bool {
    let normalized = nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
    supported_social_providers().contains(&normalized.as_str())
}

fn parse_boolish(input: &str) -> Result<bool, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" => Ok(true),
        "0" | "false" | "no" | "off" | "disabled" => Ok(false),
        other => Err(format!(
            "invalid boolean value: {other} (expected true/false)"
        )),
    }
}

fn parse_identity_tier(input: &str) -> Option<nucleusdb::halo::identity::IdentitySecurityTier> {
    match input.trim().to_ascii_lowercase().as_str() {
        "max-safe" | "max_safe" | "maxsafe" => {
            Some(nucleusdb::halo::identity::IdentitySecurityTier::MaxSafe)
        }
        "less-safe" | "less_safe" | "lesssafe" | "balanced" | "a_little_rebellious" => {
            Some(nucleusdb::halo::identity::IdentitySecurityTier::LessSafe)
        }
        "low-security" | "low_security" | "low" | "why-bother" => {
            Some(nucleusdb::halo::identity::IdentitySecurityTier::LowSecurity)
        }
        _ => None,
    }
}

fn identity_tier_label(tier: &nucleusdb::halo::identity::IdentitySecurityTier) -> &'static str {
    match tier {
        nucleusdb::halo::identity::IdentitySecurityTier::MaxSafe => "max-safe",
        nucleusdb::halo::identity::IdentitySecurityTier::LessSafe => "less-safe",
        nucleusdb::halo::identity::IdentitySecurityTier::LowSecurity => "low-security",
    }
}

fn default_identity_tier_label() -> &'static str {
    nucleusdb::halo::identity::default_security_tier_str()
}

fn store_social_token(provider: &str, token: &str) -> Result<String, String> {
    use nucleusdb::halo::vault;

    let normalized = nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
    let vault_provider = format!("social_{normalized}");
    let env_var = vault::provider_default_env_var(&vault_provider);
    let pq_wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();

    if pq_wallet_path.exists() {
        if let Ok(v) = vault::Vault::open(&pq_wallet_path, &vault_path) {
            v.set_key(&vault_provider, &env_var, token)?;
            return Ok("vault".to_string());
        }
    }

    // Fallback for environments without PQ wallet/vault.
    let creds_path = config::credentials_path();
    let mut creds = load_credentials(&creds_path).unwrap_or_default();
    creds.oauth_provider = Some(normalized);
    creds.oauth_token = Some(token.to_string());
    creds.created_at = now_unix_secs();
    save_credentials(&creds_path, &creds)?;
    Ok("credentials".to_string())
}

fn clear_social_token(provider: &str) -> Result<(), String> {
    use nucleusdb::halo::vault;

    let normalized = nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
    let vault_provider = format!("social_{normalized}");
    let pq_wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();

    if pq_wallet_path.exists() {
        if let Ok(v) = vault::Vault::open(&pq_wallet_path, &vault_path) {
            let _ = v.delete_key(&vault_provider);
        }
    }

    let creds_path = config::credentials_path();
    let mut creds = load_credentials(&creds_path).unwrap_or_default();
    if creds.oauth_provider.as_deref() == Some(normalized.as_str()) {
        creds.oauth_provider = None;
        creds.oauth_token = None;
        save_credentials(&creds_path, &creds)?;
    }
    Ok(())
}

fn cmd_identity(args: &[String]) -> Result<(), String> {
    config::ensure_halo_dir()?;
    let sub = args.first().map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "status" => {
            let json_mode = args.iter().any(|a| a == "--json");
            let profile = nucleusdb::halo::profile::load();
            let cfg = nucleusdb::halo::identity::load();
            let projection =
                nucleusdb::halo::identity_ledger::project_ledger_status(now_unix_secs())?;
            let payload = serde_json::json!({
                "profile": profile,
                "identity": cfg,
                "ledger": projection,
            });
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload)
                        .map_err(|e| format!("serialize identity status: {e}"))?
                );
            } else {
                println!("Identity status");
                println!(
                    "  Name: {}",
                    payload["profile"]["display_name"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("(not set)")
                );
                println!(
                    "  Anonymous mode: {}",
                    payload["identity"]["anonymous_mode"]
                        .as_bool()
                        .unwrap_or(false)
                );
                println!(
                    "  Security tier: {}",
                    payload["identity"]["security_tier"]
                        .as_str()
                        .unwrap_or(default_identity_tier_label())
                );
                println!(
                    "  Social ledger: entries={} chain_valid={}",
                    payload["ledger"]["total_entries"].as_u64().unwrap_or(0),
                    payload["ledger"]["chain_valid"].as_bool().unwrap_or(false)
                );
                println!(
                    "  Super secure: passkey={} security_key={} totp={}",
                    payload["identity"]["super_secure"]["passkey_enabled"]
                        .as_bool()
                        .unwrap_or(false),
                    payload["identity"]["super_secure"]["security_key_enabled"]
                        .as_bool()
                        .unwrap_or(false),
                    payload["identity"]["super_secure"]["totp_enabled"]
                        .as_bool()
                        .unwrap_or(false)
                );
                println!("  Use `agenthalo identity status --json` for full details.");
            }
            Ok(())
        }
        "tier" => {
            let tier_sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match tier_sub {
                "status" => {
                    let cfg = nucleusdb::halo::identity::load();
                    let tier = cfg
                        .security_tier
                        .as_ref()
                        .map(identity_tier_label)
                        .unwrap_or(default_identity_tier_label());
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "tier": tier,
                            "configured": cfg.security_tier.is_some(),
                        }))
                        .map_err(|e| format!("serialize identity tier status: {e}"))?
                    );
                    Ok(())
                }
                "set" => {
                    let tier_raw = args.get(2).ok_or_else(|| {
                        "usage: agenthalo identity tier set <max-safe|less-safe|low-security> [--by NAME] [--failures N]".to_string()
                    })?;
                    let tier = parse_identity_tier(tier_raw).ok_or_else(|| {
                        "tier must be one of: max-safe, less-safe, low-security".to_string()
                    })?;
                    let mut applied_by = "cli".to_string();
                    let mut step_failures = 0usize;
                    let mut idx = 3usize;
                    while idx < args.len() {
                        match args[idx].as_str() {
                            "--by" => {
                                idx += 1;
                                applied_by = args.get(idx).ok_or("--by requires a value")?.clone();
                            }
                            "--failures" => {
                                idx += 1;
                                let raw = args.get(idx).ok_or("--failures requires a value")?;
                                step_failures = raw
                                    .parse::<usize>()
                                    .map_err(|_| "--failures must be an integer")?;
                            }
                            other => {
                                return Err(format!("unknown flag for identity tier set: {other}"));
                            }
                        }
                        idx += 1;
                    }
                    let mut cfg = nucleusdb::halo::identity::load();
                    let previous_cfg = cfg.clone();
                    cfg.version = Some(1);
                    cfg.security_tier = Some(tier.clone());
                    nucleusdb::halo::identity::save(&cfg)?;
                    if let Err(e) = nucleusdb::halo::identity_ledger::append_safety_tier_applied(
                        identity_tier_label(&tier),
                        &applied_by,
                        step_failures,
                    ) {
                        let _ = nucleusdb::halo::identity::save(&previous_cfg);
                        return Err(format!(
                            "identity ledger append failed; tier update rolled back: {e}"
                        ));
                    }
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "ok": true,
                            "tier": identity_tier_label(&tier),
                            "applied_by": applied_by,
                            "step_failures": step_failures,
                        }))
                        .map_err(|e| format!("serialize identity tier set output: {e}"))?
                    );
                    Ok(())
                }
                _ => Err(
                    "usage: agenthalo identity tier [status | set <max-safe|less-safe|low-security> [--by NAME] [--failures N]]".to_string()
                ),
            }
        }
        "social" => {
            let social_sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match social_sub {
                "status" => {
                    let json_mode = args.iter().any(|a| a == "--json");
                    let cfg = nucleusdb::halo::identity::load();
                    let projection =
                        nucleusdb::halo::identity_ledger::project_ledger_status(now_unix_secs())?;
                    let payload = serde_json::json!({
                        "providers": cfg.social.providers,
                        "ledger": projection,
                    });
                    if json_mode {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&payload)
                                .map_err(|e| format!("serialize social status: {e}"))?
                        );
                    } else {
                        println!("Social providers");
                        let providers = payload["ledger"]["providers"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        if providers.is_empty() {
                            println!("  (none configured)");
                        } else {
                            for p in providers {
                                let provider = p["provider"].as_str().unwrap_or("unknown");
                                let state = if p["active"].as_bool().unwrap_or(false) {
                                    "active"
                                } else if p["expired"].as_bool().unwrap_or(false) {
                                    "expired"
                                } else {
                                    "inactive"
                                };
                                println!("  {provider:<10} {state}");
                            }
                        }
                        println!(
                            "  Chain valid: {}",
                            payload["ledger"]["chain_valid"].as_bool().unwrap_or(false)
                        );
                    }
                    Ok(())
                }
                "connect" => {
                    let provider = args.get(2).ok_or_else(|| {
                        "usage: agenthalo identity social connect <provider> [token] [--expires-days N] [--source NAME] [--selected true|false]".to_string()
                    })?;
                    if !is_supported_social_provider(provider) {
                        return Err(format!(
                            "unsupported provider: {provider}. Supported: {}",
                            supported_social_providers().join(", ")
                        ));
                    }

                    let mut idx = 3usize;
                    let mut token = String::new();
                    if let Some(candidate) = args.get(idx) {
                        if !candidate.starts_with("--") {
                            token = candidate.clone();
                            idx += 1;
                        }
                    }
                    if token.trim().is_empty() {
                        print!("Enter token for {provider}: ");
                        io::stdout()
                            .flush()
                            .map_err(|e| format!("flush stdout: {e}"))?;
                        token = read_line_trimmed()?;
                    }
                    if token.trim().is_empty() {
                        return Err("token must not be empty".to_string());
                    }

                    let mut expires_days: u64 = 30;
                    let mut source = "cli".to_string();
                    let mut selected = true;
                    while idx < args.len() {
                        match args[idx].as_str() {
                            "--expires-days" => {
                                idx += 1;
                                let raw = args.get(idx).ok_or("--expires-days requires a value")?;
                                expires_days = raw
                                    .parse::<u64>()
                                    .map_err(|_| "--expires-days must be an integer")?
                                    .clamp(1, 365);
                            }
                            "--source" => {
                                idx += 1;
                                source = args.get(idx).ok_or("--source requires a value")?.clone();
                            }
                            "--selected" => {
                                idx += 1;
                                selected = parse_boolish(
                                    args.get(idx).ok_or("--selected requires a value")?,
                                )?;
                            }
                            other => {
                                return Err(format!(
                                    "unknown flag for identity social connect: {other}"
                                ));
                            }
                        }
                        idx += 1;
                    }

                    let provider_norm =
                        nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
                    let storage = store_social_token(&provider_norm, token.trim())?;
                    let now = now_unix_secs();
                    let expires_at = Some(now.saturating_add(expires_days.saturating_mul(86_400)));
                    nucleusdb::halo::identity_ledger::append_social_connect(
                        nucleusdb::halo::identity_ledger::SocialConnectInput {
                            provider: &provider_norm,
                            token: token.trim(),
                            expires_at,
                            source: &source,
                        },
                    )?;
                    let mut cfg = nucleusdb::halo::identity::load();
                    cfg.version = Some(1);
                    let st = cfg.social.providers.entry(provider_norm.clone()).or_default();
                    st.selected = selected;
                    st.expires_at = expires_at;
                    st.source = Some(source.clone());
                    st.last_connected_at = Some(chrono::Utc::now().to_rfc3339());
                    cfg.social.last_updated = Some(chrono::Utc::now().to_rfc3339());
                    nucleusdb::halo::identity::save(&cfg)?;

                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "ok": true,
                            "provider": provider_norm,
                            "storage": storage,
                            "expires_at": expires_at,
                            "selected": selected,
                        }))
                        .map_err(|e| format!("serialize social connect output: {e}"))?
                    );
                    Ok(())
                }
                "revoke" => {
                    let provider = args.get(2).ok_or_else(|| {
                        "usage: agenthalo identity social revoke <provider> [--reason TEXT]"
                            .to_string()
                    })?;
                    if !is_supported_social_provider(provider) {
                        return Err(format!(
                            "unsupported provider: {provider}. Supported: {}",
                            supported_social_providers().join(", ")
                        ));
                    }
                    let mut reason = "operator_requested".to_string();
                    let mut idx = 3usize;
                    while idx < args.len() {
                        match args[idx].as_str() {
                            "--reason" => {
                                idx += 1;
                                reason = args.get(idx).ok_or("--reason requires a value")?.clone();
                            }
                            other => {
                                return Err(format!(
                                    "unknown flag for identity social revoke: {other}"
                                ));
                            }
                        }
                        idx += 1;
                    }
                    let provider_norm =
                        nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
                    clear_social_token(&provider_norm)?;
                    nucleusdb::halo::identity_ledger::append_social_revoke(
                        &provider_norm,
                        Some(reason.as_str()),
                    )?;
                    let mut cfg = nucleusdb::halo::identity::load();
                    if let Some(p) = cfg.social.providers.get_mut(&provider_norm) {
                        p.selected = false;
                        p.expires_at = None;
                        p.source = Some("revoked".to_string());
                    }
                    cfg.social.last_updated = Some(chrono::Utc::now().to_rfc3339());
                    nucleusdb::halo::identity::save(&cfg)?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "ok": true,
                            "provider": provider_norm,
                            "reason": reason,
                        }))
                        .map_err(|e| format!("serialize social revoke output: {e}"))?
                    );
                    Ok(())
                }
                _ => Err(
                    "usage: agenthalo identity social [status [--json] | connect <provider> [token] [--expires-days N] [--source NAME] [--selected true|false] | revoke <provider> [--reason TEXT]]".to_string()
                ),
            }
        }
        "super-secure" | "super_secure" => {
            let super_sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match super_sub {
                "status" => {
                    let json_mode = args.iter().any(|a| a == "--json");
                    let cfg = nucleusdb::halo::identity::load();
                    let payload = serde_json::json!({
                        "passkey_enabled": cfg.super_secure.passkey_enabled,
                        "security_key_enabled": cfg.super_secure.security_key_enabled,
                        "totp_enabled": cfg.super_secure.totp_enabled,
                        "totp_label": cfg.super_secure.totp_label,
                        "last_updated": cfg.super_secure.last_updated,
                    });
                    if json_mode {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&payload)
                                .map_err(|e| format!("serialize super-secure status: {e}"))?
                        );
                    } else {
                        println!("Super-secure status");
                        println!(
                            "  passkey={} security_key={} totp={}",
                            payload["passkey_enabled"].as_bool().unwrap_or(false),
                            payload["security_key_enabled"].as_bool().unwrap_or(false),
                            payload["totp_enabled"].as_bool().unwrap_or(false)
                        );
                        if let Some(label) = payload["totp_label"].as_str() {
                            if !label.trim().is_empty() {
                                println!("  totp_label={label}");
                            }
                        }
                    }
                    Ok(())
                }
                "set" => {
                    let option = args.get(2).ok_or_else(|| {
                        "usage: agenthalo identity super-secure set <passkey|security_key|totp> <true|false> [--label TEXT]".to_string()
                    })?;
                    let enabled_raw = args.get(3).ok_or_else(|| {
                        "usage: agenthalo identity super-secure set <passkey|security_key|totp> <true|false> [--label TEXT]".to_string()
                    })?;
                    let enabled = parse_boolish(enabled_raw)?;
                    let option_norm = option.trim().to_ascii_lowercase();
                    if option_norm != "passkey"
                        && option_norm != "security_key"
                        && option_norm != "totp"
                    {
                        return Err(
                            "option must be one of: passkey, security_key, totp".to_string()
                        );
                    }
                    let mut label: Option<String> = None;
                    let mut idx = 4usize;
                    while idx < args.len() {
                        match args[idx].as_str() {
                            "--label" => {
                                idx += 1;
                                label = Some(args.get(idx).ok_or("--label requires a value")?.clone());
                            }
                            other => {
                                return Err(format!(
                                    "unknown flag for identity super-secure set: {other}"
                                ));
                            }
                        }
                        idx += 1;
                    }

                    let mut cfg = nucleusdb::halo::identity::load();
                    match option_norm.as_str() {
                        "passkey" => cfg.super_secure.passkey_enabled = enabled,
                        "security_key" => cfg.super_secure.security_key_enabled = enabled,
                        "totp" => {
                            cfg.super_secure.totp_enabled = enabled;
                            if let Some(l) = label.clone() {
                                cfg.super_secure.totp_label = Some(l);
                            }
                        }
                        _ => {}
                    }
                    cfg.super_secure.last_updated = Some(chrono::Utc::now().to_rfc3339());
                    nucleusdb::halo::identity::save(&cfg)?;
                    let metadata = if option_norm == "totp" {
                        serde_json::json!({"label": label.clone().unwrap_or_default()})
                    } else {
                        serde_json::json!({})
                    };
                    nucleusdb::halo::identity_ledger::append_super_secure_update(
                        &option_norm,
                        enabled,
                        metadata,
                    )?;

                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "ok": true,
                            "option": option_norm,
                            "enabled": enabled,
                            "totp_label": cfg.super_secure.totp_label,
                            "last_updated": cfg.super_secure.last_updated,
                        }))
                        .map_err(|e| format!("serialize super-secure set output: {e}"))?
                    );
                    Ok(())
                }
                _ => Err(
                    "usage: agenthalo identity super-secure [status [--json] | set <passkey|security_key|totp> <true|false> [--label TEXT]]"
                        .to_string(),
                ),
            }
        }
        _ => Err(
            "usage: agenthalo identity [status [--json] | tier <...> | social <...> | super-secure <...>]"
                .to_string(),
        ),
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

fn cmd_setup(_args: &[String]) -> Result<(), String> {
    config::ensure_halo_dir()?;

    println!();
    println!("  Welcome to Agent H.A.L.O.");
    println!("  Tamper-proof observability for AI agents.");
    println!();
    println!("  How would you like to proceed?");
    println!();
    println!("  1. Open Dashboard (web UI)     — visual setup, analytics, configuration");
    println!("  2. Quick CLI Setup             — terminal-based, for power users");
    println!("  3. Agent-Only (MCP server)     — headless, for AI agent integration");
    println!();
    print!("  > ");
    io::stdout()
        .flush()
        .map_err(|e| format!("flush stdout: {e}"))?;
    let choice = read_line_trimmed()?;

    match choice.as_str() {
        "1" | "dashboard" => {
            cmd_dashboard(&[])?;
        }
        "2" | "cli" => {
            println!();
            cmd_login(&[])?;
            println!();
            cmd_wrap(&["--all".to_string()])?;
            println!();
            cmd_status(&[])?;
        }
        "3" | "agent" | "mcp" => {
            println!();
            println!("  Add this to your agent's MCP configuration:");
            println!();
            println!("  Claude Code (.mcp.json):");
            println!("  {{");
            println!("    \"mcpServers\": {{");
            println!("      \"agenthalo\": {{");
            println!("        \"command\": \"agenthalo-mcp-server\"");
            println!("      }}");
            println!("    }}");
            println!("  }}");
            println!();
            println!("  Codex (.codex/config.toml):");
            println!("  [mcp_servers.agenthalo]");
            println!("  command = \"agenthalo-mcp-server\"");
            println!();
            println!("  Gemini (.gemini/settings.json):");
            println!("  {{");
            println!("    \"mcpServers\": {{");
            println!("      \"agenthalo\": {{");
            println!("        \"command\": \"agenthalo-mcp-server\"");
            println!("      }}");
            println!("    }}");
            println!("  }}");
            println!();
            println!("  Monitor via: agenthalo dashboard");
        }
        _ => {
            return Err(format!("invalid choice: {choice}. Expected 1, 2, or 3."));
        }
    }
    Ok(())
}

fn cmd_dashboard(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_dashboard_usage();
        return Ok(());
    }

    let mut port: u16 = 3100;
    let mut open_browser = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                port = args
                    .get(i)
                    .ok_or("--port requires a value")?
                    .parse()
                    .map_err(|_| "--port must be a valid port number")?;
            }
            "--no-open" => {
                open_browser = false;
            }
            other => return Err(format!("unknown dashboard flag: {other}")),
        }
        i += 1;
    }

    config::ensure_halo_dir()?;

    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("create tokio runtime: {e}"))?;
    rt.block_on(dashboard::serve(port, open_browser))
}

fn print_dashboard_usage() {
    println!(
        "Usage:\n  agenthalo dashboard [--port N] [--no-open]\n\nOptions:\n  --port N     Port for dashboard HTTP server (default: 3100)\n  --no-open    Do not open browser automatically\n  -h, --help   Show this help"
    );
}

fn cmd_doctor(_args: &[String]) -> Result<(), String> {
    println!();
    println!("  Agent H.A.L.O. v0.3.0");
    println!();

    // Authentication
    let creds_path = config::credentials_path();
    let has_auth = is_authenticated(&creds_path) || resolve_api_key(&creds_path).is_some();
    let auth_detail = if has_auth {
        let creds = load_credentials(&creds_path).ok();
        let provider = creds
            .as_ref()
            .and_then(|c| c.oauth_provider.as_deref())
            .unwrap_or("API key");
        let user = creds
            .as_ref()
            .and_then(|c| c.user_id.as_deref())
            .unwrap_or("unknown");
        format!("OK  ({provider}, user: {user})")
    } else {
        "NOT AUTHENTICATED".to_string()
    };
    println!("  Authentication:     {auth_detail}");

    // Trace store
    let db_path = config::db_path();
    if db_path.exists() {
        let sessions = list_sessions(&db_path).unwrap_or_default();
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
        println!(
            "  Trace store:        OK  ({} sessions, {} tokens, ${:.2} total)",
            sessions.len(),
            format_number_inline(total_tokens),
            total_cost
        );
    } else {
        println!("  Trace store:        NOT CREATED");
    }

    // Agent wrapping
    let rc = wrap::detect_shell_rc();
    let rc_content = std::fs::read_to_string(&rc).unwrap_or_default();
    println!("  Agent wrapping:");
    for agent in ["claude", "codex", "gemini"] {
        let marker = format!("# AGENTHALO_WRAP_{}", agent.to_ascii_uppercase());
        let wrapped = rc_content.contains(&marker);
        let label = if wrapped {
            format!("WRAPPED  (alias active in {})", rc.display())
        } else {
            "NOT WRAPPED".to_string()
        };
        println!("    {agent:<17} {label}");
    }

    // x402
    let x402_cfg = x402::load_x402_config();
    if x402_cfg.enabled {
        println!(
            "  x402 payments:      ENABLED  ({}, max ${:.2} auto-approve)",
            x402_cfg.preferred_network,
            x402_cfg.max_auto_approve as f64 / 1_000_000.0
        );
        match x402::check_usdc_balance(&x402_cfg) {
            Ok((_, balance)) => {
                println!(
                    "    wallet balance:   {:.6} USDC",
                    balance as f64 / 1_000_000.0
                );
            }
            Err(_) => {
                println!("    wallet balance:   (unable to check)");
            }
        }
    } else {
        println!("  x402 payments:      DISABLED");
    }

    // AgentPMT
    let pmt_cfg = agentpmt::load_or_default();
    println!(
        "  AgentPMT proxy:     {}",
        if pmt_cfg.enabled {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );

    // PQ wallet
    if has_wallet() {
        println!("  PQ wallet:          OK  (ML-DSA-65)");
    } else {
        println!("  PQ wallet:          NOT CREATED");
    }

    // On-chain
    let onchain_cfg = load_onchain_config_or_default();
    if onchain_cfg.contract_address.is_empty()
        || onchain_cfg.contract_address == "0x0000000000000000000000000000000000000000"
    {
        println!("  On-chain:           NOT CONFIGURED");
    } else {
        println!(
            "  On-chain:           CONFIGURED  ({}, contract {}...)",
            if onchain_cfg.chain_name.is_empty() {
                "unknown chain"
            } else {
                &onchain_cfg.chain_name
            },
            &onchain_cfg.contract_address[..std::cmp::min(10, onchain_cfg.contract_address.len())]
        );
    }

    // License
    println!("  License:            Community (free)");

    // Dashboard
    println!("  Dashboard:          Run `agenthalo dashboard` to start");

    println!();
    println!("  All checks completed.");
    println!();
    Ok(())
}

fn format_number_inline(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
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
        "agenthalo 0.3.0 — Tamper-proof observability for AI agents\n\nGetting started:\n  setup                      Interactive first-run wizard (dashboard, CLI, or MCP)\n  dashboard [--port N] [--no-open]\n                             Launch web dashboard at http://localhost:3100\n  doctor                     Run diagnostic check on all subsystems\n\nAgent recording:\n  run [--agent-name NAME] [--model MODEL] <agent> [args...]\n                             Run agent with recording (model auto-detected from stream)\n  wrap <agent>|--all         Add shell aliases for transparent wrapping\n  unwrap <agent>|--all       Remove shell aliases\n\nAuthentication:\n  login [github|google|api]  Authenticate via OAuth or API key\n  config set-key <key>       Save API key\n  config set-agentpmt-key <key>\n                             Save AgentPMT bearer token\n\nObservability:\n  status [--json]            Show recording status, session count, and total cost\n  traces [session-id] [--json]\n                             List sessions or show session detail\n  costs [--month] [--paid] [--json]\n                             Show model costs or operation usage\n  export <session-id> [--out <path>]\n                             Export full session as standalone JSON\n\nAttestation & trust:\n  attest [--session ID] [--anonymous] [--onchain]\n                             Build attestation (Merkle default, Groth16+onchain when --onchain)\n  audit <contract.sol> [--size small|medium|large]\n                             Run Solidity static audit\n  keygen --pq [--force]      Generate/rotate ML-DSA wallet\n  sign --pq (--message TEXT | --file PATH)\n                             Create detached ML-DSA signature\n  trust [query|score] [--session ID]\n                             Query trust score\n\nVault & credentials:\n  vault list                 Show all provider slots and their status\n  vault set <provider> [key] Store an API key (reads stdin if key omitted)\n  vault delete <provider>    Remove a stored key\n  vault test <provider>      Show masked key info\n  identity status [--json]   Show profile, identity config, and social ledger status\n  identity social ...        Connect/revoke/status for social OAuth providers\n  identity super-secure ...  Set or view passkey/security-key/TOTP flags\n\nPayments:\n  x402 [status|enable|disable|config|check|pay|balance]\n                             x402direct stablecoin payment integration\n\nGovernance & protocol:\n  vote --proposal ID --choice yes|no|abstain [--reason TEXT]\n  sync [--target cloudflare|local]\n  onchain [config|deploy|verify|status] ...\n  protocol privacy-pool-create | privacy-pool-withdraw | pq-bridge-transfer\n\nConfiguration:\n  config show                Show effective config\n  config tool-proxy [enable|disable|status|refresh|endpoint <url>|clear-endpoint]\n  addon [list|enable|disable] [name]\n  license [status|verify <certificate.json>]\n\n  version                    Print version\n  help                       Show this help\n\nEnvironment:\n  AGENTHALO_HOME\n  AGENTHALO_DB_PATH\n  AGENTHALO_API_KEY\n  AGENTHALO_ALLOW_GENERIC=1   Enable paid-tier custom agent wrapping\n  AGENTHALO_NO_TELEMETRY=1    (default behavior: zero telemetry)\n  AGENTHALO_ONCHAIN_STUB=1    Disable real RPC posting and return deterministic stub tx hashes"
    );
}
