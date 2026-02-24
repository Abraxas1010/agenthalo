use nucleusdb::halo::agentpmt::{
    self, agentpmt_config_path, is_agentpmt_configured, save_agentpmt_config, AgentPmtClient,
    AgentPmtConfig,
};
use nucleusdb::halo::auth::{
    is_authenticated, load_credentials, oauth_login, resolve_api_key, save_credentials, Credentials,
};
use nucleusdb::halo::config;
use nucleusdb::halo::detect::AgentType;
use nucleusdb::halo::runner::AgentRunner;
use nucleusdb::halo::schema::{SessionMetadata, SessionStatus};
use nucleusdb::halo::trace::{now_unix_secs, TraceWriter};
use nucleusdb::halo::{generic_agents_allowed, viewer, wrap};
use std::io::{self, Write};

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
    let db_path = config::db_path();
    viewer::print_costs(&db_path, monthly)
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
            println!("Credit history not yet implemented (Phase 1).");
            Ok(())
        }
        _ => Err("usage: agenthalo credits [balance|add|history]".to_string()),
    }
}

fn cmd_attest(args: &[String]) -> Result<(), String> {
    let client = require_agentpmt()?;
    let anonymous = args.iter().any(|a| a == "--anonymous");
    let session_id = args
        .iter()
        .position(|a| a == "--session")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let op = if anonymous { "attest_anon" } else { "attest" };
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!("Cost: {} credits (${:.2})", cost, cost as f64 * 0.01);

    let can_afford = client.can_afford(cost)?;
    if !can_afford {
        return Err("insufficient credits. Run: agenthalo credits add".to_string());
    }

    let result = client.deduct(op, 1)?;
    if !result.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}. Run: agenthalo credits add",
            result.remaining_credits, cost
        ));
    }

    println!("Attestation not yet implemented (Phase 1). Credits deducted successfully.");
    if let Some(id) = session_id {
        println!("Session: {id}");
    }
    println!("Remaining credits: {}", result.remaining_credits);
    Ok(())
}

fn cmd_audit(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "usage: agenthalo audit <contract.sol> [--size small|medium|large]".to_string(),
        );
    }
    let contract = &args[0];
    let size = args
        .iter()
        .position(|a| a == "--size")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("small");

    let op = match size {
        "small" => "audit_small",
        "medium" => "audit_medium",
        "large" => "audit_large",
        _ => return Err("size must be small, medium, or large".to_string()),
    };
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    println!(
        "Audit cost ({size}): {cost} credits (${:.2})",
        cost as f64 * 0.01
    );

    let client = require_agentpmt()?;
    let can_afford = client.can_afford(cost)?;
    if !can_afford {
        return Err("insufficient credits. Run: agenthalo credits add".to_string());
    }
    let result = client.deduct(op, 1)?;
    if !result.success {
        return Err("insufficient credits. Run: agenthalo credits add".to_string());
    }

    println!("Audit of {contract} not yet implemented (Phase 1). Credits deducted.");
    println!("Remaining credits: {}", result.remaining_credits);
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
            println!("  agentpmt            {pmt}   (mandatory payment rail)");
            println!("  p2pclaw             NOT CONFIGURED   (optional marketplace)");
            println!("  agentpmt-workflows  NOT CONFIGURED   (optional challenges)");
            Ok(())
        }
        "enable" => {
            let name = args
                .get(1)
                .ok_or_else(|| "usage: agenthalo addon enable <name>".to_string())?;
            match name.as_str() {
                "p2pclaw" => {
                    println!("P2PCLAW add-on integration is not yet implemented (Phase 3).");
                    Ok(())
                }
                "agentpmt-workflows" => {
                    println!("AgentPMT workflows add-on is not yet implemented (Phase 3).");
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
            println!("Disabled {name} add-on.");
            Ok(())
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

            let can_afford = client.can_afford(cost)?;
            if !can_afford {
                return Err("insufficient credits. Run: agenthalo credits add".to_string());
            }
            let result = client.deduct(op, 1)?;
            if !result.success {
                return Err("insufficient credits. Run: agenthalo credits add".to_string());
            }

            println!("License purchase not yet fully implemented (Phase 1). Credits deducted.");
            println!("Remaining credits: {}", result.remaining_credits);
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
        "agenthalo 0.1.0\n\nCommands:\n  run [--agent-name NAME] [--model MODEL] <agent> [args...]\n                             Run agent with recording\n  login [github|google|api]  Authenticate via OAuth or API key\n  config set-key <key>       Save API key\n  config set-agentpmt-key <key>\n                             Save AgentPMT API key\n  config show                Show effective config\n  traces [session-id]        List sessions or show session detail\n  costs [--month]            Show cost summaries\n  credits [balance|add|history]\n                             Check or add AgentPMT credits\n  attest [--session ID] [--anonymous]\n                             Attest session on-chain (paid)\n  audit <contract.sol> [--size small|medium|large]\n                             Audit smart contract (paid)\n  license [status|buy <tier>]\n                             Manage NucleusDB license\n  addon [list|enable|disable] [name]\n                             Manage optional add-ons\n  wrap <agent>|--all         Add shell aliases\n  unwrap <agent>|--all       Remove shell aliases\n  version                    Print version\n  help                       Show this help\n\nEnvironment:\n  AGENTHALO_HOME\n  AGENTHALO_DB_PATH\n  AGENTHALO_API_KEY\n  AGENTHALO_ALLOW_GENERIC=1   Enable paid-tier custom agent wrapping\n  AGENTHALO_NO_TELEMETRY=1    (default behavior: zero telemetry)\n  AGENTHALO_AGENTPMT_STUB=1   Enable local stub mode for AgentPMT credits"
    );
}
