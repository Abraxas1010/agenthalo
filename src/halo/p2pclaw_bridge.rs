use crate::halo::addons;
use crate::halo::config;
use crate::halo::p2pclaw::{self, HiveEvent, Investigation, P2PClawConfig, Paper};
use crate::halo::p2pclaw_mcp::P2PClawMcpManager;
use crate::halo::p2pclaw_verify;
use crate::halo::trace::now_unix_secs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const STATE_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeConfig {
    pub min_poll_interval_secs: u64,
    pub max_poll_interval_secs: u64,
    pub heartbeat_interval_secs: u64,
    pub event_limit: u64,
    pub preview_items: usize,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            min_poll_interval_secs: 15,
            max_poll_interval_secs: 300,
            heartbeat_interval_secs: 60,
            event_limit: 50,
            preview_items: 5,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeRunOptions {
    pub dry_run: bool,
    pub include_mcp_tools: bool,
    pub publish_summary: bool,
    pub validate_paper_id: Option<String>,
    pub validate_approve: bool,
    pub validate_occam_score: Option<f64>,
    pub chat_message: Option<String>,
    pub chat_channel: Option<String>,
}

impl Default for BridgeRunOptions {
    fn default() -> Self {
        Self {
            dry_run: true,
            include_mcp_tools: false,
            publish_summary: false,
            validate_paper_id: None,
            validate_approve: true,
            validate_occam_score: None,
            chat_message: None,
            chat_channel: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeLoopOptions {
    pub run: BridgeRunOptions,
    pub max_iterations: Option<u64>,
    pub respect_backoff: bool,
    pub heartbeat: bool,
    pub report_tau_sync: bool,
}

impl Default for BridgeLoopOptions {
    fn default() -> Self {
        Self {
            run: BridgeRunOptions::default(),
            max_iterations: None,
            respect_backoff: true,
            heartbeat: false,
            report_tau_sync: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BridgeLoopReport {
    pub iterations: u64,
    pub reports: Vec<BridgeRunReport>,
    pub final_state: BridgePersistentState,
    pub total_sleep_secs: u64,
    pub elapsed_ms: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgePersistentState {
    pub version: u32,
    pub last_run_at: Option<u64>,
    pub last_success_at: Option<u64>,
    pub last_heartbeat_at: Option<u64>,
    pub last_tau_sync_at: Option<u64>,
    pub next_poll_not_before: Option<u64>,
    pub current_backoff_secs: u64,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
    pub last_tau_sync_error: Option<String>,
    pub last_event_since: Option<u64>,
    pub last_validation_paper_id: Option<String>,
    pub last_publication_paper_id: Option<String>,
    pub last_briefing_sha256: Option<String>,
    pub last_tau_sync_response: Option<serde_json::Value>,
    pub polls_total: u64,
    pub briefing_polls_total: u64,
    pub investigation_polls_total: u64,
    pub mempool_polls_total: u64,
    pub event_polls_total: u64,
    pub tau_reports_total: u64,
    pub validations_total: u64,
    pub publications_total: u64,
    pub chat_messages_total: u64,
    pub investigations_seen_total: u64,
    pub mempool_seen_total: u64,
    pub events_seen_total: u64,
    pub hive_compute_cycles: u64,
    pub local_compute_cycles: u64,
}

impl Default for BridgePersistentState {
    fn default() -> Self {
        let cfg = BridgeConfig::default();
        Self {
            version: STATE_VERSION,
            last_run_at: None,
            last_success_at: None,
            last_heartbeat_at: None,
            last_tau_sync_at: None,
            next_poll_not_before: None,
            current_backoff_secs: cfg.min_poll_interval_secs,
            consecutive_failures: 0,
            last_error: None,
            last_tau_sync_error: None,
            last_event_since: None,
            last_validation_paper_id: None,
            last_publication_paper_id: None,
            last_briefing_sha256: None,
            last_tau_sync_response: None,
            polls_total: 0,
            briefing_polls_total: 0,
            investigation_polls_total: 0,
            mempool_polls_total: 0,
            event_polls_total: 0,
            tau_reports_total: 0,
            validations_total: 0,
            publications_total: 0,
            chat_messages_total: 0,
            investigations_seen_total: 0,
            mempool_seen_total: 0,
            events_seen_total: 0,
            hive_compute_cycles: 0,
            local_compute_cycles: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeAction {
    pub kind: String,
    pub target: Option<String>,
    pub detail: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeRunReport {
    pub dry_run: bool,
    pub state_path: String,
    pub bridge_config: BridgeConfig,
    pub state_before: BridgePersistentState,
    pub state_after: BridgePersistentState,
    pub briefing_sha256: String,
    pub briefing_chars: usize,
    pub investigations_seen: usize,
    pub mempool_seen: usize,
    pub events_seen: usize,
    pub preview_investigations: Vec<String>,
    pub preview_mempool: Vec<String>,
    pub preview_events: Vec<String>,
    pub mcp_sidecar_available: bool,
    pub mcp_tools: Vec<String>,
    pub mcp_error: Option<String>,
    pub actions: Vec<BridgeAction>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeStatus {
    pub enabled: bool,
    pub configured: bool,
    pub state_path: String,
    pub bridge_config: BridgeConfig,
    pub state: BridgePersistentState,
    pub capabilities: Vec<String>,
    pub compute_split_ratio: f64,
    pub nash_compliant: bool,
    pub p2p_config: Option<P2PClawConfig>,
    pub mcp_sidecar_available: bool,
    pub mcp_tools: Option<Vec<String>>,
    pub mcp_error: Option<String>,
}

pub fn state_path() -> PathBuf {
    config::p2pclaw_bridge_state_path()
}

pub fn load_state() -> Result<BridgePersistentState, String> {
    let path = state_path();
    if !path.exists() {
        return Ok(BridgePersistentState::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read bridge state {}: {e}", path.display()))?;
    let state: BridgePersistentState = serde_json::from_str(&raw)
        .map_err(|e| format!("parse bridge state {}: {e}", path.display()))?;
    Ok(state)
}

pub fn save_state(state: &BridgePersistentState) -> Result<(), String> {
    config::ensure_halo_dir()?;
    let path = state_path();
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(state)
        .map_err(|e| format!("serialize bridge state {}: {e}", path.display()))?;
    std::fs::write(&tmp, raw).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod 600 {}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

pub fn status(
    cfg: Option<&P2PClawConfig>,
    include_mcp_tools: bool,
) -> Result<BridgeStatus, String> {
    let state = load_state()?;
    let enabled = addons::is_enabled("p2pclaw").unwrap_or(false);
    let capabilities = detect_capabilities(cfg);
    let mut mcp_tools = None;
    let mut mcp_error = None;
    let mcp_sidecar_available = P2PClawMcpManager::is_available();
    if include_mcp_tools {
        if let Some(cfg) = cfg {
            match inspect_mcp_tools(cfg) {
                Ok(tools) => mcp_tools = Some(tools),
                Err(err) => mcp_error = Some(err),
            }
        } else if mcp_sidecar_available {
            mcp_error = Some("p2pclaw config required before probing MCP sidecar".to_string());
        }
    }
    Ok(BridgeStatus {
        enabled,
        configured: cfg.is_some(),
        state_path: state_path().display().to_string(),
        bridge_config: BridgeConfig::default(),
        compute_split_ratio: compute_split_ratio(&state),
        nash_compliant: nash_compliant(&state),
        state,
        capabilities,
        p2p_config: cfg.cloned(),
        mcp_sidecar_available,
        mcp_tools,
        mcp_error,
    })
}

pub fn run_once(cfg: &P2PClawConfig, options: BridgeRunOptions) -> Result<BridgeRunReport, String> {
    let bridge_cfg = BridgeConfig::default();
    let now = now_unix_secs();
    let state_before = load_state()?;
    let mut state_after = state_before.clone();
    state_after.version = STATE_VERSION;
    state_after.last_run_at = Some(now);
    state_after.polls_total += 1;

    let outcome = (|| -> Result<BridgeRunReport, String> {
        let briefing = p2pclaw::get_briefing(cfg)?;
        let investigations = p2pclaw::list_investigations(cfg)?;
        let mempool = p2pclaw::list_mempool(cfg)?;
        let events = p2pclaw::poll_events(
            cfg,
            state_before.last_event_since,
            Some(bridge_cfg.event_limit),
        )?;

        state_after.briefing_polls_total += 1;
        state_after.investigation_polls_total += 1;
        state_after.mempool_polls_total += 1;
        state_after.event_polls_total += 1;
        state_after.investigations_seen_total += investigations.len() as u64;
        state_after.mempool_seen_total += mempool.len() as u64;
        state_after.events_seen_total += events.len() as u64;
        state_after.hive_compute_cycles +=
            (investigations.len() + mempool.len() + events.len()).max(1) as u64;
        state_after.last_briefing_sha256 = Some(sha256_hex(briefing.as_bytes()));
        if let Some(max_ts) = events.iter().filter_map(|e| e.timestamp).max() {
            state_after.last_event_since = Some(max_ts);
        }

        let mut actions = Vec::new();
        if let Some(paper_id) = normalize_option(options.validate_paper_id.as_deref()) {
            if options.dry_run {
                actions.push(BridgeAction {
                    kind: "validate_paper".to_string(),
                    target: Some(paper_id.to_string()),
                    detail: format!(
                        "would validate paper `{paper_id}` with approve={} occam_score={:?}",
                        options.validate_approve, options.validate_occam_score
                    ),
                    dry_run: true,
                });
            } else {
                let result = p2pclaw::validate_paper(
                    cfg,
                    paper_id,
                    options.validate_approve,
                    options.validate_occam_score,
                )?;
                state_after.validations_total += 1;
                state_after.last_validation_paper_id = result
                    .paper_id
                    .clone()
                    .or_else(|| Some(paper_id.to_string()));
                actions.push(BridgeAction {
                    kind: "validate_paper".to_string(),
                    target: state_after.last_validation_paper_id.clone(),
                    detail: serde_json::to_string(&result)
                        .unwrap_or_else(|_| "validation submitted".to_string()),
                    dry_run: false,
                });
            }
        }

        if options.publish_summary {
            let title = format!(
                "[AgentHALO Bridge] investigations={} mempool={} events={}",
                investigations.len(),
                mempool.len(),
                events.len()
            );
            let content = build_summary_markdown(
                cfg,
                &bridge_cfg,
                &briefing,
                &investigations,
                &mempool,
                &events,
            );
            let verification = p2pclaw_verify::verify_paper(&p2pclaw_verify::VerificationRequest {
                title: title.clone(),
                content: content.clone(),
                claims: vec![],
                agent_id: Some(cfg.agent_id.clone()),
            });
            state_after.local_compute_cycles += 1;
            if options.dry_run {
                actions.push(BridgeAction {
                    kind: "publish_summary".to_string(),
                    target: None,
                    detail: format!(
                        "would publish verified summary (verified={}, proof_hash={})",
                        verification.verified, verification.proof_hash
                    ),
                    dry_run: true,
                });
            } else {
                let result = p2pclaw::publish_paper(cfg, &title, &content)?;
                state_after.publications_total += 1;
                state_after.last_publication_paper_id = result.paper_id.clone();
                actions.push(BridgeAction {
                    kind: "publish_summary".to_string(),
                    target: state_after.last_publication_paper_id.clone(),
                    detail: serde_json::to_string(&result)
                        .unwrap_or_else(|_| "publication submitted".to_string()),
                    dry_run: false,
                });
            }
        }

        if let Some(message) = normalize_option(options.chat_message.as_deref()) {
            let channel = normalize_option(options.chat_channel.as_deref());
            if options.dry_run {
                actions.push(BridgeAction {
                    kind: "send_chat".to_string(),
                    target: channel.map(str::to_string),
                    detail: format!("would send bridge chat message: {}", message),
                    dry_run: true,
                });
            } else {
                p2pclaw::send_chat(cfg, message, channel)?;
                state_after.chat_messages_total += 1;
                actions.push(BridgeAction {
                    kind: "send_chat".to_string(),
                    target: channel.map(str::to_string),
                    detail: "chat message sent".to_string(),
                    dry_run: false,
                });
            }
        }

        let mcp_sidecar_available = P2PClawMcpManager::is_available();
        let (mcp_tools, mcp_error) = if options.include_mcp_tools {
            match inspect_mcp_tools(cfg) {
                Ok(tools) => (tools, None),
                Err(err) => (Vec::new(), Some(err)),
            }
        } else {
            (Vec::new(), None)
        };

        let activity = investigations.len() + mempool.len() + events.len() + actions.len();
        state_after.current_backoff_secs =
            next_backoff_secs(state_after.current_backoff_secs, &bridge_cfg, activity > 0);
        state_after.next_poll_not_before = Some(now + state_after.current_backoff_secs);
        state_after.last_success_at = Some(now);
        state_after.consecutive_failures = 0;
        state_after.last_error = None;

        Ok(BridgeRunReport {
            dry_run: options.dry_run,
            state_path: state_path().display().to_string(),
            bridge_config: bridge_cfg.clone(),
            state_before,
            state_after: state_after.clone(),
            briefing_sha256: sha256_hex(briefing.as_bytes()),
            briefing_chars: briefing.len(),
            investigations_seen: investigations.len(),
            mempool_seen: mempool.len(),
            events_seen: events.len(),
            preview_investigations: preview_investigations(
                &investigations,
                bridge_cfg.preview_items,
            ),
            preview_mempool: preview_papers(&mempool, bridge_cfg.preview_items),
            preview_events: preview_events(&events, bridge_cfg.preview_items),
            mcp_sidecar_available,
            mcp_tools,
            mcp_error,
            actions,
        })
    })();

    match outcome {
        Ok(report) => {
            save_state(&state_after)?;
            Ok(report)
        }
        Err(err) => {
            state_after.consecutive_failures += 1;
            state_after.last_error = Some(err.clone());
            state_after.current_backoff_secs =
                failure_backoff_secs(state_after.current_backoff_secs, &bridge_cfg);
            state_after.next_poll_not_before = Some(now + state_after.current_backoff_secs);
            save_state(&state_after)?;
            Err(err)
        }
    }
}

pub fn run_loop(
    cfg: &P2PClawConfig,
    options: BridgeLoopOptions,
) -> Result<BridgeLoopReport, String> {
    let started = Instant::now();
    let mut reports = Vec::new();
    let mut iterations = 0u64;
    let mut total_sleep_secs = 0u64;

    loop {
        iterations += 1;
        let report = run_once(cfg, options.run.clone())?;
        reports.push(report);

        if options.heartbeat {
            maybe_send_heartbeat(cfg, options.run.dry_run)?;
        }
        if options.report_tau_sync {
            maybe_report_tau_sync(cfg, options.run.dry_run)?;
        }

        if options
            .max_iterations
            .map(|max| iterations >= max)
            .unwrap_or(true)
        {
            break;
        }

        if options.respect_backoff {
            let state = load_state()?;
            let now = now_unix_secs();
            let sleep_secs = state
                .next_poll_not_before
                .and_then(|deadline| deadline.checked_sub(now))
                .unwrap_or_else(|| BridgeConfig::default().min_poll_interval_secs);
            if sleep_secs > 0 {
                total_sleep_secs += sleep_secs;
                thread::sleep(Duration::from_secs(sleep_secs));
            }
        }
    }

    Ok(BridgeLoopReport {
        iterations,
        reports,
        final_state: load_state()?,
        total_sleep_secs,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

fn inspect_mcp_tools(cfg: &P2PClawConfig) -> Result<Vec<String>, String> {
    let mut manager = P2PClawMcpManager::new(&cfg.endpoint_url, &cfg.agent_id, &cfg.agent_name);
    manager.start()?;
    let mut tools: Vec<String> = manager
        .list_tools()?
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    tools.sort();
    Ok(tools)
}

fn build_summary_markdown(
    cfg: &P2PClawConfig,
    bridge_cfg: &BridgeConfig,
    briefing: &str,
    investigations: &[Investigation],
    mempool: &[Paper],
    events: &[HiveEvent],
) -> String {
    let investigations_preview = preview_investigations(investigations, bridge_cfg.preview_items)
        .into_iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mempool_preview = preview_papers(mempool, bridge_cfg.preview_items)
        .into_iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    let events_preview = preview_events(events, bridge_cfg.preview_items)
        .into_iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "# Abstract\nThis bridge digest summarizes the current P2PCLAW state observed by AgentHALO. It records the current briefing, investigation count, mempool size, and event activity so another agent can reproduce the same operational picture.\n\n# Introduction\nAgent `{}` collected this snapshot through the real P2PCLAW REST and MCP integration. The digest is intended as an operator-visible bridge report rather than a formal theorem proof.\n\n# Methodology\nWe fetched the live briefing, investigations list, mempool entries, and recent hive events. The report uses the existing AgentHALO structural verifier before publication.\n\n# Results\nInvestigations observed: {}\nMempool papers observed: {}\nRecent events observed: {}\n\n## Investigation Preview\n{}\n\n## Mempool Preview\n{}\n\n## Event Preview\n{}\n\n# Discussion\nThis report preserves a reproducible snapshot of bridge-visible activity. It does not claim kernel proof replay; it documents current network state with explicit structure.\n\n# Appendix: Briefing Digest\n```text\n{}\n```\n",
        cfg.agent_id,
        investigations.len(),
        mempool.len(),
        events.len(),
        blank_if_empty(&investigations_preview),
        blank_if_empty(&mempool_preview),
        blank_if_empty(&events_preview),
        truncate(briefing, 2400),
    )
}

fn preview_investigations(items: &[Investigation], limit: usize) -> Vec<String> {
    items
        .iter()
        .take(limit)
        .map(|item| {
            item.title
                .clone()
                .or_else(|| item.id.clone())
                .unwrap_or_else(|| "<unnamed investigation>".to_string())
        })
        .collect()
}

fn preview_papers(items: &[Paper], limit: usize) -> Vec<String> {
    items
        .iter()
        .take(limit)
        .map(|item| {
            item.title
                .clone()
                .or_else(|| item.paper_id.clone())
                .unwrap_or_else(|| "<untitled paper>".to_string())
        })
        .collect()
}

fn preview_events(items: &[HiveEvent], limit: usize) -> Vec<String> {
    items
        .iter()
        .take(limit)
        .map(|item| {
            item.kind
                .clone()
                .or_else(|| item.timestamp.map(|ts| format!("event@{ts}")))
                .unwrap_or_else(|| "<unknown event>".to_string())
        })
        .collect()
}

fn next_backoff_secs(current: u64, cfg: &BridgeConfig, had_activity: bool) -> u64 {
    if had_activity {
        cfg.min_poll_interval_secs
    } else {
        failure_backoff_secs(current, cfg)
    }
}

fn failure_backoff_secs(current: u64, cfg: &BridgeConfig) -> u64 {
    let base = if current == 0 {
        cfg.min_poll_interval_secs
    } else {
        current
    };
    cmp::min(
        cfg.max_poll_interval_secs,
        cmp::max(cfg.min_poll_interval_secs, base.saturating_mul(2)),
    )
}

fn detect_capabilities(cfg: Option<&P2PClawConfig>) -> Vec<String> {
    let mut capabilities = vec![
        "briefing".to_string(),
        "investigations".to_string(),
        "mempool".to_string(),
        "events".to_string(),
        "publish_summary".to_string(),
        "validate_paper".to_string(),
        "chat".to_string(),
    ];
    if cfg.is_some() {
        capabilities.push("configured".to_string());
    }
    if P2PClawMcpManager::is_available() {
        capabilities.push("mcp_sidecar".to_string());
    }
    capabilities
}

fn compute_split_ratio(state: &BridgePersistentState) -> f64 {
    let total = state.hive_compute_cycles + state.local_compute_cycles;
    if total == 0 {
        0.5
    } else {
        state.hive_compute_cycles as f64 / total as f64
    }
}

fn nash_compliant(state: &BridgePersistentState) -> bool {
    compute_split_ratio(state) >= 0.4
}

fn maybe_send_heartbeat(cfg: &P2PClawConfig, dry_run: bool) -> Result<(), String> {
    let bridge_cfg = BridgeConfig::default();
    let now = now_unix_secs();
    let mut state = load_state()?;
    let due = state
        .last_heartbeat_at
        .map(|last| now.saturating_sub(last) >= bridge_cfg.heartbeat_interval_secs)
        .unwrap_or(true);
    if !due {
        return Ok(());
    }
    if !dry_run {
        let message = format!(
            "bridge-heartbeat agent={} ratio={:.3} polls={} validations={} publications={}",
            cfg.agent_id,
            compute_split_ratio(&state),
            state.polls_total,
            state.validations_total,
            state.publications_total
        );
        p2pclaw::send_chat(cfg, &message, Some("operations"))?;
        state.chat_messages_total += 1;
    }
    state.last_heartbeat_at = Some(now);
    save_state(&state)
}

fn maybe_report_tau_sync(cfg: &P2PClawConfig, dry_run: bool) -> Result<(), String> {
    let now = now_unix_secs();
    let mut state = load_state()?;
    let response = if dry_run {
        serde_json::json!({
            "status": "dry_run",
            "agent_id": cfg.agent_id,
            "compute_cycles": state.hive_compute_cycles,
        })
    } else {
        p2pclaw::report_tau_tick(cfg, state.hive_compute_cycles)?
    };
    state.last_tau_sync_at = Some(now);
    state.tau_reports_total += 1;
    state.last_tau_sync_response = Some(response);
    state.last_tau_sync_error = None;
    save_state(&state)
}

fn normalize_option(input: Option<&str>) -> Option<&str> {
    input.map(str::trim).filter(|s| !s.is_empty())
}

fn blank_if_empty(input: &str) -> String {
    if input.trim().is_empty() {
        "- none".to_string()
    } else {
        input.to_string()
    }
}

fn truncate(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        input.to_string()
    } else {
        format!("{}...", &input[..max_len])
    }
}

fn sha256_hex(input: &[u8]) -> String {
    hex::encode(Sha256::digest(input))
}
