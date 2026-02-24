use nucleusdb::halo::addons;
use nucleusdb::halo::agentpmt::{
    self, agentpmt_config_path, is_agentpmt_configured, load_agentpmt_config, save_agentpmt_config,
    AgentPmtClient, AgentPmtConfig,
};
use nucleusdb::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use nucleusdb::halo::audit::{audit_contract_file, save_audit_result, AuditSize};
use nucleusdb::halo::auth::{
    is_authenticated, load_credentials, oauth_login, resolve_api_key, save_credentials, Credentials,
};
use nucleusdb::halo::circuit::{
    load_or_setup_attestation_keys, proof_words_json_array, prove_attestation,
    public_inputs_json_array, verify_attestation_proof,
};
use nucleusdb::halo::config;
use nucleusdb::halo::detect::AgentType;
use nucleusdb::halo::onchain::{
    deploy_trust_verifier, load_onchain_config_or_default, onchain_config_path, post_attestation,
    query_attestation, save_onchain_config,
};
use nucleusdb::halo::pq::{has_wallet, keygen_pq, sign_pq_payload};
use nucleusdb::halo::runner::AgentRunner;
use nucleusdb::halo::schema::{SessionMetadata, SessionStatus};
use nucleusdb::halo::trace::{
    list_sessions, now_unix_secs, record_paid_operation_for_halo, TraceWriter,
};
use nucleusdb::halo::trust::query_trust_score;
use nucleusdb::halo::util::digest_json;
use nucleusdb::halo::{generic_agents_allowed, viewer, wrap};
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
        "traces" => cmd_traces(&args[2..]),
        "costs" => cmd_costs(&args[2..]),
        "credits" => cmd_credits(&args[2..]),
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
        "wrap" => cmd_wrap(&args[2..]),
        "unwrap" => cmd_unwrap(&args[2..]),
        "version" | "--version" | "-V" => {
            println!("agenthalo 0.1.0");
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
        model,
        started_at: now,
        ended_at: None,
        prompt: infer_prompt(cmd_args),
        status: SessionStatus::Running,
        user_id: creds.user_id,
        machine_id: std::env::var("HOSTNAME").ok(),
        puf_digest: None,
    };

    writer.start_session(meta)?;
    let exit_code = runner.run(&mut writer)?;
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
            "usage: agenthalo config set-key <key> | set-agentpmt-key <key> | show".to_string(),
        );
    }
    config::ensure_halo_dir()?;
    let creds_path = config::credentials_path();

    match args[0].as_str() {
        "set-key" => {
            // Accept key as arg for scripted use, or prompt interactively.
            // Interactive prompt avoids exposing the key in ps/shell history.
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
                print!("Enter AgentPMT API key: ");
                io::stdout()
                    .flush()
                    .map_err(|e| format!("flush stdout: {e}"))?;
                read_line_trimmed()?
            };
            if key.trim().is_empty() {
                return Err("AgentPMT API key cannot be empty".to_string());
            }
            let cfg = AgentPmtConfig {
                api_key: key,
                cached_balance: None,
                balance_refreshed_at: None,
                history: vec![],
            };
            let path = agentpmt_config_path();
            save_agentpmt_config(&path, &cfg)?;
            println!("AgentPMT key saved at {}", path.display());
            Ok(())
        }
        "show" => {
            println!("AGENTHALO_HOME={}", config::halo_dir().display());
            println!("DB_PATH={}", config::db_path().display());
            println!("CREDENTIALS={}", creds_path.display());
            println!("PRICING={}", config::pricing_path().display());
            let has_auth = is_authenticated(&creds_path) || resolve_api_key(&creds_path).is_some();
            println!("AUTHENTICATED={has_auth}");
            println!("AGENTPMT_CONFIGURED={}", is_agentpmt_configured());
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
    let db_path = config::db_path();
    let session = args.first().map(|s| s.as_str());
    viewer::print_traces(&db_path, session)
}

fn cmd_costs(args: &[String]) -> Result<(), String> {
    let monthly = args.iter().any(|a| a == "--month");
    let paid = args.iter().any(|a| a == "--paid");
    let db_path = config::db_path();
    if paid {
        viewer::print_paid_costs(&db_path, monthly)
    } else {
        viewer::print_costs(&db_path, monthly)
    }
}

fn cmd_credits(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("balance");
    match sub {
        "balance" => {
            let client = require_agentpmt()?;
            let balance = client.balance()?;
            println!(
                "Credits: {} (${:.2})",
                balance.credits,
                balance.credits as f64 * 0.01
            );
            Ok(())
        }
        "add" => {
            let _ = webbrowser::open("https://www.agentpmt.com/billing");
            println!("Opened AgentPMT billing in your browser.");
            println!("Add credits there, then retry your operation.");
            Ok(())
        }
        "history" => {
            let path = agentpmt_config_path();
            let cfg = load_agentpmt_config(&path).map_err(|_| {
                "not connected to AgentPMT. Run: agenthalo config set-agentpmt-key <key>"
                    .to_string()
            })?;
            if cfg.history.is_empty() {
                println!("No local credit history yet.");
                return Ok(());
            }
            println!("Recent credit history (newest first):");
            for entry in cfg.history.iter().rev().take(20) {
                println!(
                    "  ts={} product={} units={} total={} remaining={} tx={}",
                    entry.timestamp,
                    entry.product_slug,
                    entry.units,
                    entry.total_credits,
                    entry.remaining_credits,
                    entry
                        .transaction_id
                        .clone()
                        .unwrap_or_else(|| "none".to_string())
                );
            }
            Ok(())
        }
        _ => Err("usage: agenthalo credits [balance|add|history]".to_string()),
    }
}

fn cmd_attest(args: &[String]) -> Result<(), String> {
    let client = require_agentpmt()?;
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!("Cost: {} credits (${:.2})", cost, cost as f64 * 0.01);

    let deduct_result = client.deduct(op, 1)?;
    if !deduct_result.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
            deduct_result.remaining_credits, cost
        ));
    }

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
                let (pk, vk, key_info) = load_or_setup_attestation_keys(None)?;
                let proof_bundle = prove_attestation(&pk, &result)?;
                let local_verified = verify_attestation_proof(&vk, &proof_bundle)?;
                if !local_verified {
                    return Err("local Groth16 verification failed".to_string());
                }

                let cfg = load_onchain_config_or_default();
                let post = post_attestation(&cfg, &proof_bundle, anonymous)?;
                result.proof_type = "groth16-bn254".to_string();
                result.groth16_proof = Some(proof_words_json_array(&proof_bundle));
                result.groth16_public_inputs = Some(public_inputs_json_array(&proof_bundle));
                result.tx_hash = Some(post.tx_hash.clone());
                result.contract_address = Some(post.contract_address.clone());
                result.block_number = post.block_number;
                result.chain = Some(post.chain);
                println!(
                    "On-chain attestation posted. tx_hash={} contract={} (keys: pk={}, vk={})",
                    post.tx_hash, post.contract_address, key_info.pk_path, key_info.vk_path
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
            println!("Remaining credits: {}", deduct_result.remaining_credits);
            record_paid_operation_for_halo(
                op,
                cost,
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
                cost,
                Some(resolved_session_id),
                None,
                false,
                Some(e.clone()),
            );
            Err(format!("attestation failed after credit deduction: {e}"))
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!(
        "Audit cost ({}): {cost} credits (${:.2})",
        size.as_str(),
        cost as f64 * 0.01
    );

    let client = require_agentpmt()?;
    let deduct_result = client.deduct(op, 1)?;
    if !deduct_result.success {
        return Err("insufficient credits. Run: agenthalo credits add".to_string());
    }

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
            println!("Remaining credits: {}", deduct_result.remaining_credits);
            record_paid_operation_for_halo(
                op,
                cost,
                None,
                Some(result.contract_hash.clone()),
                true,
                None,
            )?;
            Ok(())
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, cost, None, None, false, Some(e.clone()));
            Err(format!("audit failed after credit deduction: {e}"))
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!("Signing cost: {cost} credits (${:.2})", cost as f64 * 0.01);
    let client = require_agentpmt()?;
    let deduct_result = client.deduct(op, 1)?;
    if !deduct_result.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
            deduct_result.remaining_credits, cost
        ));
    }

    match sign_pq_payload(&payload, &payload_kind, payload_hint) {
        Ok((envelope, save_path)) => {
            println!("PQ signing successful.");
            println!("Signature file: {}", save_path.display());
            println!(
                "{}",
                serde_json::to_string_pretty(&envelope)
                    .map_err(|e| format!("serialize signature output: {e}"))?
            );
            println!("Remaining credits: {}", deduct_result.remaining_credits);
            record_paid_operation_for_halo(
                op,
                cost,
                None,
                Some(envelope.signature_digest.clone()),
                true,
                None,
            )?;
            Ok(())
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, cost, None, None, false, Some(e.clone()));
            Err(format!("signing failed after credit deduction: {e}"))
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
            let cost = agentpmt::operation_cost(op).unwrap_or(0);
            println!(
                "Trust query cost: {cost} credits (${:.2})",
                cost as f64 * 0.01
            );
            let client = require_agentpmt()?;
            let deduct_result = client.deduct(op, 1)?;
            if !deduct_result.success {
                return Err(format!(
                    "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
                    deduct_result.remaining_credits, cost
                ));
            }

            let db_path = config::db_path();
            match query_trust_score(&db_path, session_id.as_deref()) {
                Ok(score) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&score)
                            .map_err(|e| format!("serialize trust output: {e}"))?
                    );
                    println!("Remaining credits: {}", deduct_result.remaining_credits);
                    record_paid_operation_for_halo(
                        op,
                        cost,
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
                        cost,
                        session_id,
                        None,
                        false,
                        Some(e.clone()),
                    );
                    Err(format!("trust query failed after credit deduction: {e}"))
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!("Vote cost: {cost} credits (${:.2})", cost as f64 * 0.01);
    let client = require_agentpmt()?;
    let deduct_result = client.deduct(op, 1)?;
    if !deduct_result.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
            deduct_result.remaining_credits, cost
        ));
    }

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
            "remaining_credits": deduct_result.remaining_credits,
            "result_digest": result_digest,
            "vote": payload
        }))
        .map_err(|e| format!("serialize vote output: {e}"))?
    );
    record_paid_operation_for_halo(op, cost, None, Some(result_digest), true, None)?;
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!("Sync cost: {cost} credits (${:.2})", cost as f64 * 0.01);
    let client = require_agentpmt()?;
    let deduct_result = client.deduct(op, 1)?;
    if !deduct_result.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
            deduct_result.remaining_credits, cost
        ));
    }

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
            "remaining_credits": deduct_result.remaining_credits,
            "result_digest": result_digest,
            "sync": payload
        }))
        .map_err(|e| format!("serialize sync output: {e}"))?
    );
    record_paid_operation_for_halo(op, cost, None, Some(result_digest), true, None)?;
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
                    "config": cfg,
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
                serde_json::to_string_pretty(&cfg)
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!(
        "Privacy pool create cost: {cost} credits (${:.2})",
        cost as f64 * 0.01
    );
    let client = require_agentpmt()?;
    let deduct_result = client.deduct(op, 1)?;
    if !deduct_result.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
            deduct_result.remaining_credits, cost
        ));
    }

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
            "remaining_credits": deduct_result.remaining_credits,
            "result_digest": result_digest,
            "pool": payload
        }))
        .map_err(|e| format!("serialize pool create output: {e}"))?
    );
    record_paid_operation_for_halo(op, cost, None, Some(result_digest), true, None)?;
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!(
        "Privacy pool withdraw cost: {cost} credits (${:.2})",
        cost as f64 * 0.01
    );
    let client = require_agentpmt()?;
    let deduct_result = client.deduct(op, 1)?;
    if !deduct_result.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
            deduct_result.remaining_credits, cost
        ));
    }

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
            "remaining_credits": deduct_result.remaining_credits,
            "result_digest": result_digest,
            "withdrawal": payload
        }))
        .map_err(|e| format!("serialize pool withdraw output: {e}"))?
    );
    record_paid_operation_for_halo(op, cost, None, Some(result_digest), true, None)?;
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!(
        "PQ bridge transfer cost: {cost} credits (${:.2})",
        cost as f64 * 0.01
    );
    let client = require_agentpmt()?;
    let deduct_result = client.deduct(op, 1)?;
    if !deduct_result.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
            deduct_result.remaining_credits, cost
        ));
    }

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
            "remaining_credits": deduct_result.remaining_credits,
            "result_digest": result_digest,
            "transfer": payload
        }))
        .map_err(|e| format!("serialize bridge transfer output: {e}"))?
    );
    record_paid_operation_for_halo(op, cost, None, Some(result_digest), true, None)?;
    Ok(())
}

fn cmd_addon(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "list" => {
            let pmt = if is_agentpmt_configured() {
                "ACTIVE"
            } else {
                "NOT CONFIGURED"
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
            println!("  agentpmt            {pmt}   (mandatory payment rail)");
            println!("  p2pclaw             {p2pclaw}        (optional marketplace)");
            println!("  agentpmt-workflows  {workflows}        (optional challenges)");
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
                _ => Err(format!(
                    "unknown add-on: {name}. Available: p2pclaw, agentpmt-workflows"
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
                _ => Err(format!(
                    "unknown add-on: {name}. Available: p2pclaw, agentpmt-workflows"
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
            println!("To upgrade: agenthalo license buy [starter|professional|enterprise]");
            Ok(())
        }
        "buy" => {
            let tier = args.get(1).ok_or_else(|| {
                "usage: agenthalo license buy <starter|professional|enterprise>".to_string()
            })?;
            let op = match tier.as_str() {
                "starter" => "license_starter",
                "professional" => "license_professional",
                "enterprise" => "license_enterprise",
                _ => return Err("tier must be starter, professional, or enterprise".to_string()),
            };

            let client = require_agentpmt()?;
            let cost = agentpmt::operation_cost(op).unwrap_or(0);
            println!(
                "License cost: {cost} credits/month (${:.2}/month)",
                cost as f64 * 0.01
            );

            let result = client.deduct(op, 1)?;
            if !result.success {
                return Err("insufficient credits. Run: agenthalo credits add".to_string());
            }

            println!("License purchase not yet fully implemented (Phase 1). Credits deducted.");
            println!("Remaining credits: {}", result.remaining_credits);
            record_paid_operation_for_halo(op, cost, None, None, true, None)?;
            Ok(())
        }
        _ => Err("usage: agenthalo license [status|buy <tier>]".to_string()),
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
    // For prompt-style invocations, keep a compact textual preview in metadata.
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

fn require_agentpmt() -> Result<AgentPmtClient, String> {
    AgentPmtClient::from_config().ok_or_else(|| {
        "not connected to AgentPMT. Run: agenthalo config set-agentpmt-key <key>".to_string()
    })
}

fn print_usage() {
    println!(
        "agenthalo 0.1.0\n\nCommands:\n  run [--agent-name NAME] [--model MODEL] <agent> [args...]\n                             Run agent with recording\n  login [github|google|api]  Authenticate via OAuth or API key\n  config set-key <key>       Save API key\n  config set-agentpmt-key <key>\n                             Save AgentPMT API key\n  config show                Show effective config\n  traces [session-id]        List sessions or show session detail\n  costs [--month] [--paid]   Show model costs or paid operation usage\n  credits [balance|add|history]\n                             Check or add AgentPMT credits\n  attest [--session ID] [--anonymous] [--onchain]\n                             Build attestation (Merkle default, Groth16+onchain when --onchain)\n  audit <contract.sol> [--size small|medium|large]\n                             Run Solidity static audit (paid)\n  keygen --pq [--force]      Generate/rotate ML-DSA wallet\n  sign --pq (--message TEXT | --file PATH)\n                             Create detached ML-DSA signature (paid)\n  trust [query|score] [--session ID]\n                             Query trust score (paid)\n  vote --proposal ID --choice yes|no|abstain [--reason TEXT]\n                             Submit governance vote intent (paid)\n  sync [--target cloudflare|local]\n                             Run sync operation (paid)\n  onchain [config|deploy|verify|status] ...\n                             Manage and query Base Sepolia attestation contract\n  protocol privacy-pool-create --denomination <u64> [--chain NAME] [--asset SYMBOL]\n                             Create privacy pool request (paid; workflows add-on)\n  protocol privacy-pool-withdraw --pool-id <id> --recipient <addr> [--amount u64]\n                             Submit privacy withdrawal request (paid; workflows add-on)\n  protocol pq-bridge-transfer --from <chain> --to <chain> --asset <symbol> --amount <u64> --recipient <addr>\n                             Submit PQ bridge transfer request (paid; p2pclaw add-on)\n  license [status|buy <tier>]\n                             Manage NucleusDB license\n  addon [list|enable|disable] [name]\n                             Manage optional add-ons\n  wrap <agent>|--all         Add shell aliases\n  unwrap <agent>|--all       Remove shell aliases\n  version                    Print version\n  help                       Show this help\n\nEnvironment:\n  AGENTHALO_HOME\n  AGENTHALO_DB_PATH\n  AGENTHALO_API_KEY\n  AGENTHALO_ALLOW_GENERIC=1   Enable paid-tier custom agent wrapping\n  AGENTHALO_NO_TELEMETRY=1    (default behavior: zero telemetry)\n  AGENTHALO_AGENTPMT_STUB=1   Enable local stub mode for AgentPMT credits\n  AGENTHALO_ONCHAIN_STUB=1    Disable real RPC posting and return deterministic stub tx hashes"
    );
}
