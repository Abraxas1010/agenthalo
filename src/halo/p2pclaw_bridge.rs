use crate::halo::addons;
use crate::halo::config;
use crate::halo::p2pclaw::{self, HiveEvent, Investigation, P2PClawConfig, Paper};
use crate::halo::p2pclaw_mcp::P2PClawMcpManager;
use crate::halo::p2pclaw_verify;
use crate::halo::trace::{now_unix_secs, record_paid_operation_for_halo};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const STATE_VERSION: u32 = 1;
const MAX_RECENT_EVENT_KEYS: usize = 256;
const MAX_RETAINED_LOOP_REPORTS: usize = 16;
const SHUTDOWN_SLEEP_SLICE_MS: u64 = 250;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
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
    pub force_repeat_actions: bool,
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
            force_repeat_actions: false,
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
    pub dropped_reports: u64,
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
    pub last_published_briefing_sha256: Option<String>,
    pub last_tau_sync_response: Option<serde_json::Value>,
    pub recent_event_keys: Vec<String>,
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
            last_published_briefing_sha256: None,
            last_tau_sync_response: None,
            recent_event_keys: Vec::new(),
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
    pub config_path: String,
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

struct StateLock {
    #[allow(dead_code)]
    file: File,
}

impl StateLock {
    fn acquire() -> Result<Self, String> {
        config::ensure_halo_dir()?;
        let path = config::p2pclaw_bridge_lock_path();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("open bridge lock {}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if rc != 0 {
                return Err(format!(
                    "lock bridge state {}: {}",
                    path.display(),
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(Self { file })
    }
}

struct LockedBridgeState {
    _lock: StateLock,
    state: BridgePersistentState,
}

impl LockedBridgeState {
    fn load() -> Result<Self, String> {
        let lock = StateLock::acquire()?;
        let path = state_path();
        let state = if !path.exists() {
            BridgePersistentState::default()
        } else {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("read bridge state {}: {e}", path.display()))?;
            serde_json::from_str(&raw)
                .map_err(|e| format!("parse bridge state {}: {e}", path.display()))?
        };
        Ok(Self { _lock: lock, state })
    }

    fn persist(&self) -> Result<(), String> {
        write_state_unlocked(&self.state)
    }
}

pub fn state_path() -> PathBuf {
    config::p2pclaw_bridge_state_path()
}

pub fn config_path() -> PathBuf {
    config::p2pclaw_bridge_config_path()
}

fn validate_bridge_config(cfg: &BridgeConfig) -> Result<(), String> {
    if cfg.min_poll_interval_secs == 0 {
        return Err("bridge min_poll_interval_secs must be > 0".to_string());
    }
    if cfg.max_poll_interval_secs < cfg.min_poll_interval_secs {
        return Err("bridge max_poll_interval_secs must be >= min_poll_interval_secs".to_string());
    }
    if cfg.heartbeat_interval_secs == 0 {
        return Err("bridge heartbeat_interval_secs must be > 0".to_string());
    }
    if cfg.event_limit == 0 {
        return Err("bridge event_limit must be > 0".to_string());
    }
    if cfg.preview_items == 0 {
        return Err("bridge preview_items must be > 0".to_string());
    }
    Ok(())
}

pub fn load_config() -> Result<BridgeConfig, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(BridgeConfig::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read bridge config {}: {e}", path.display()))?;
    let cfg: BridgeConfig = serde_json::from_str(&raw)
        .map_err(|e| format!("parse bridge config {}: {e}", path.display()))?;
    validate_bridge_config(&cfg)?;
    Ok(cfg)
}

pub fn save_config(cfg: &BridgeConfig) -> Result<(), String> {
    validate_bridge_config(cfg)?;
    config::ensure_halo_dir()?;
    let path = config_path();
    let raw = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("serialize bridge config {}: {e}", path.display()))?;
    write_atomic_private_file(&path, &raw)
}

pub fn load_state() -> Result<BridgePersistentState, String> {
    Ok(LockedBridgeState::load()?.state)
}

pub fn save_state(state: &BridgePersistentState) -> Result<(), String> {
    let _lock = StateLock::acquire()?;
    write_state_unlocked(state)
}

pub fn status(
    cfg: Option<&P2PClawConfig>,
    include_mcp_tools: bool,
) -> Result<BridgeStatus, String> {
    let state = load_state()?;
    let bridge_config = load_config()?;
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
        config_path: config_path().display().to_string(),
        state_path: state_path().display().to_string(),
        bridge_config,
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
    let bridge_cfg = load_config()?;
    let now = now_unix_secs();
    let state_before = load_state()?;
    let briefing = p2pclaw::get_briefing(cfg);
    let investigations = p2pclaw::list_investigations(cfg);
    let mempool = p2pclaw::list_mempool(cfg);
    let events = p2pclaw::poll_events(
        cfg,
        state_before.last_event_since,
        Some(bridge_cfg.event_limit),
    );

    let briefing = match briefing {
        Ok(value) => value,
        Err(err) => {
            persist_failure(&bridge_cfg, now, err.clone())?;
            return Err(err);
        }
    };
    let investigations = match investigations {
        Ok(value) => value,
        Err(err) => {
            persist_failure(&bridge_cfg, now, err.clone())?;
            return Err(err);
        }
    };
    let mempool = match mempool {
        Ok(value) => value,
        Err(err) => {
            persist_failure(&bridge_cfg, now, err.clone())?;
            return Err(err);
        }
    };
    let events = match events {
        Ok(value) => value,
        Err(err) => {
            persist_failure(&bridge_cfg, now, err.clone())?;
            return Err(err);
        }
    };

    let mcp_sidecar_available = P2PClawMcpManager::is_available();
    let (mcp_tools, mcp_error) = if options.include_mcp_tools {
        match inspect_mcp_tools(cfg) {
            Ok(tools) => (tools, None),
            Err(err) => (Vec::new(), Some(err)),
        }
    } else {
        (Vec::new(), None)
    };

    let briefing_sha256 = sha256_hex(briefing.as_bytes());
    let mut actions = Vec::new();
    let mut locked = LockedBridgeState::load()?;
    let mut state_after = locked.state.clone();
    state_after.version = STATE_VERSION;
    state_after.last_run_at = Some(now);
    state_after.polls_total += 1;
    state_after.briefing_polls_total += 1;
    state_after.investigation_polls_total += 1;
    state_after.mempool_polls_total += 1;
    state_after.event_polls_total += 1;
    state_after.last_briefing_sha256 = Some(briefing_sha256.clone());
    state_after.local_compute_cycles = state_after.local_compute_cycles.saturating_add(
        1 + investigations.len() as u64 + mempool.len() as u64 + events.len() as u64,
    );

    let unique_events = dedup_events(&events, &state_after.recent_event_keys);
    state_after.investigations_seen_total += investigations.len() as u64;
    state_after.mempool_seen_total += mempool.len() as u64;
    state_after.events_seen_total += unique_events.len() as u64;
    if let Some(max_ts) = events.iter().filter_map(|e| e.timestamp).max() {
        state_after.last_event_since = Some(max_ts);
    }
    extend_recent_event_keys(&mut state_after.recent_event_keys, &unique_events);

    let verification_title = format!(
        "[AgentHALO Bridge] investigations={} mempool={} events={}",
        investigations.len(),
        mempool.len(),
        unique_events.len()
    );
    let verification_content = build_summary_markdown(
        cfg,
        &bridge_cfg,
        &briefing,
        &investigations,
        &mempool,
        &unique_events,
    );
    let verification = if options.publish_summary {
        Some(p2pclaw_verify::verify_paper(
            &p2pclaw_verify::VerificationRequest {
                title: verification_title.clone(),
                content: verification_content.clone(),
                claims: vec![],
                agent_id: Some(cfg.agent_id.clone()),
            },
        ))
    } else {
        None
    };

    if let Some(paper_id) = normalize_option(options.validate_paper_id.as_deref()) {
        let already_validated = state_after.last_validation_paper_id.as_deref() == Some(paper_id);
        if already_validated && !options.force_repeat_actions {
            actions.push(BridgeAction {
                kind: "skip_validate_paper".to_string(),
                target: Some(paper_id.to_string()),
                detail: "skipping duplicate validation for the most recently validated paper"
                    .to_string(),
                dry_run: options.dry_run,
            });
        } else if options.dry_run {
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
            match p2pclaw::validate_paper(
                cfg,
                paper_id,
                options.validate_approve,
                options.validate_occam_score,
            ) {
                Ok(result) => {
                    state_after.validations_total += 1;
                    state_after.hive_compute_cycles += 1;
                    state_after.last_validation_paper_id = result
                        .paper_id
                        .clone()
                        .or_else(|| Some(paper_id.to_string()));
                    let detail = serde_json::to_string(&result)
                        .unwrap_or_else(|_| "validation submitted".to_string());
                    let trace_error = trace_bridge_operation(
                        "p2pclaw_validate_paper",
                        state_after.last_validation_paper_id.clone(),
                        true,
                        None,
                    )
                    .err();
                    actions.push(BridgeAction {
                        kind: "validate_paper".to_string(),
                        target: state_after.last_validation_paper_id.clone(),
                        detail: append_trace_note(detail, trace_error),
                        dry_run: false,
                    });
                }
                Err(err) => {
                    let _ = trace_bridge_operation(
                        "p2pclaw_validate_paper",
                        Some(paper_id.to_string()),
                        false,
                        Some(err.clone()),
                    );
                    state_after.consecutive_failures += 1;
                    state_after.last_error = Some(err.clone());
                    state_after.current_backoff_secs =
                        failure_backoff_secs(state_after.current_backoff_secs, &bridge_cfg);
                    state_after.next_poll_not_before = Some(now + state_after.current_backoff_secs);
                    locked.state = state_after;
                    locked.persist()?;
                    return Err(err);
                }
            }
        }
    }

    if options.publish_summary {
        let already_published =
            state_after.last_published_briefing_sha256.as_deref() == Some(briefing_sha256.as_str());
        if already_published && !options.force_repeat_actions {
            actions.push(BridgeAction {
                kind: "skip_publish_summary".to_string(),
                target: state_after.last_publication_paper_id.clone(),
                detail: "skipping duplicate summary publication because the briefing digest is unchanged".to_string(),
                dry_run: options.dry_run,
            });
        } else {
            let verification = verification
                .clone()
                .expect("verification is present when publishing");
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
                match p2pclaw::publish_paper(cfg, &verification_title, &verification_content) {
                    Ok(result) => {
                        state_after.publications_total += 1;
                        state_after.hive_compute_cycles += 1;
                        state_after.last_publication_paper_id = result.paper_id.clone();
                        state_after.last_published_briefing_sha256 = Some(briefing_sha256.clone());
                        let detail = serde_json::to_string(&result)
                            .unwrap_or_else(|_| "publication submitted".to_string());
                        let trace_error = trace_bridge_operation(
                            "p2pclaw_publish_summary",
                            state_after
                                .last_publication_paper_id
                                .clone()
                                .or_else(|| Some(briefing_sha256.clone())),
                            true,
                            None,
                        )
                        .err();
                        actions.push(BridgeAction {
                            kind: "publish_summary".to_string(),
                            target: state_after.last_publication_paper_id.clone(),
                            detail: append_trace_note(detail, trace_error),
                            dry_run: false,
                        });
                    }
                    Err(err) => {
                        let _ = trace_bridge_operation(
                            "p2pclaw_publish_summary",
                            Some(briefing_sha256.clone()),
                            false,
                            Some(err.clone()),
                        );
                        state_after.consecutive_failures += 1;
                        state_after.last_error = Some(err.clone());
                        state_after.current_backoff_secs =
                            failure_backoff_secs(state_after.current_backoff_secs, &bridge_cfg);
                        state_after.next_poll_not_before =
                            Some(now + state_after.current_backoff_secs);
                        locked.state = state_after;
                        locked.persist()?;
                        return Err(err);
                    }
                }
            }
        }
    }

    if let Some(message) = normalize_option(options.chat_message.as_deref()) {
        let channel = normalize_option(options.chat_channel.as_deref());
        if options.dry_run {
            actions.push(BridgeAction {
                kind: "send_chat".to_string(),
                target: channel.map(str::to_string),
                detail: format!("would send bridge chat message: {message}"),
                dry_run: true,
            });
        } else {
            match p2pclaw::send_chat(cfg, message, channel) {
                Ok(_) => {
                    state_after.chat_messages_total += 1;
                    state_after.hive_compute_cycles += 1;
                    let trace_error = trace_bridge_operation(
                        "p2pclaw_bridge_chat",
                        channel.map(str::to_string),
                        true,
                        None,
                    )
                    .err();
                    actions.push(BridgeAction {
                        kind: "send_chat".to_string(),
                        target: channel.map(str::to_string),
                        detail: append_trace_note("chat message sent".to_string(), trace_error),
                        dry_run: false,
                    });
                }
                Err(err) => {
                    let _ = trace_bridge_operation(
                        "p2pclaw_bridge_chat",
                        channel.map(str::to_string),
                        false,
                        Some(err.clone()),
                    );
                    state_after.consecutive_failures += 1;
                    state_after.last_error = Some(err.clone());
                    state_after.current_backoff_secs =
                        failure_backoff_secs(state_after.current_backoff_secs, &bridge_cfg);
                    state_after.next_poll_not_before = Some(now + state_after.current_backoff_secs);
                    locked.state = state_after;
                    locked.persist()?;
                    return Err(err);
                }
            }
        }
    }

    let had_activity = !unique_events.is_empty()
        || !investigations.is_empty()
        || !mempool.is_empty()
        || actions
            .iter()
            .any(|action| !action.kind.starts_with("skip_"));
    state_after.current_backoff_secs =
        next_backoff_secs(state_after.current_backoff_secs, &bridge_cfg, had_activity);
    state_after.next_poll_not_before = Some(now + state_after.current_backoff_secs);
    state_after.last_success_at = Some(now);
    state_after.consecutive_failures = 0;
    state_after.last_error = None;

    locked.state = state_after.clone();
    locked.persist()?;

    Ok(BridgeRunReport {
        dry_run: options.dry_run,
        state_path: state_path().display().to_string(),
        bridge_config: bridge_cfg.clone(),
        state_before,
        state_after,
        briefing_sha256,
        briefing_chars: briefing.chars().count(),
        investigations_seen: investigations.len(),
        mempool_seen: mempool.len(),
        events_seen: unique_events.len(),
        preview_investigations: preview_investigations(&investigations, bridge_cfg.preview_items),
        preview_mempool: preview_papers(&mempool, bridge_cfg.preview_items),
        preview_events: preview_events(&unique_events, bridge_cfg.preview_items),
        mcp_sidecar_available,
        mcp_tools,
        mcp_error,
        actions,
    })
}

pub fn run_loop(
    cfg: &P2PClawConfig,
    options: BridgeLoopOptions,
) -> Result<BridgeLoopReport, String> {
    run_loop_with_shutdown(cfg, options, shutdown_flag())
}

#[doc(hidden)]
pub fn run_loop_with_shutdown(
    cfg: &P2PClawConfig,
    options: BridgeLoopOptions,
    shutdown: Arc<AtomicBool>,
) -> Result<BridgeLoopReport, String> {
    let started = Instant::now();
    let bridge_cfg = load_config()?;
    let mut reports = Vec::new();
    let mut iterations = 0u64;
    let mut dropped_reports = 0u64;
    let mut total_sleep_secs = 0u64;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        iterations += 1;
        let report = run_once(cfg, options.run.clone())?;
        if reports.len() == MAX_RETAINED_LOOP_REPORTS {
            reports.remove(0);
            dropped_reports += 1;
        }
        reports.push(report);

        if options.heartbeat || options.report_tau_sync {
            let mut locked = LockedBridgeState::load()?;
            let mut state = locked.state.clone();
            let heartbeat_result = if options.heartbeat {
                maybe_send_heartbeat(cfg, &bridge_cfg, options.run.dry_run, &mut state)
            } else {
                Ok(())
            };
            let tau_result = if heartbeat_result.is_ok() && options.report_tau_sync {
                maybe_report_tau_sync(cfg, options.run.dry_run, &mut state)
            } else {
                Ok(())
            };
            locked.state = state;
            locked.persist()?;
            heartbeat_result?;
            tau_result?;
        }
        if options
            .max_iterations
            .map(|max| iterations >= max)
            .unwrap_or(false)
        {
            break;
        }

        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let sleep_secs = if options.respect_backoff {
            let state = load_state()?;
            let now = now_unix_secs();
            state
                .next_poll_not_before
                .and_then(|deadline| deadline.checked_sub(now))
                .unwrap_or(bridge_cfg.min_poll_interval_secs)
        } else {
            0
        };
        if sleep_secs > 0 {
            total_sleep_secs += sleep_secs;
            sleep_with_shutdown(Duration::from_secs(sleep_secs), shutdown.clone());
        }
    }

    Ok(BridgeLoopReport {
        iterations,
        reports,
        dropped_reports,
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
        .map(|item| format!("- {}", escape_markdown_inline(&item)))
        .collect::<Vec<_>>()
        .join("\n");
    let mempool_preview = preview_papers(mempool, bridge_cfg.preview_items)
        .into_iter()
        .map(|item| format!("- {}", escape_markdown_inline(&item)))
        .collect::<Vec<_>>()
        .join("\n");
    let events_preview = preview_events(events, bridge_cfg.preview_items)
        .into_iter()
        .map(|item| format!("- {}", escape_markdown_inline(&item)))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "# Abstract\nThis bridge digest summarizes the current P2PCLAW state observed by AgentHALO. It records the current briefing, investigation count, mempool size, and event activity so another agent can reproduce the same operational picture.\n\n# Introduction\nAgent `{}` collected this snapshot through the real P2PCLAW REST and MCP integration. The digest is intended as an operator-visible bridge report rather than a formal theorem proof.\n\n# Methodology\nWe fetched the live briefing, investigations list, mempool entries, and recent hive events. The report uses the existing AgentHALO structural verifier before publication.\n\n# Results\nInvestigations observed: {}\nMempool papers observed: {}\nRecent events observed: {}\n\n## Investigation Preview\n{}\n\n## Mempool Preview\n{}\n\n## Event Preview\n{}\n\n# Discussion\nThis report preserves a reproducible snapshot of bridge-visible activity. It does not claim kernel proof replay; it documents current network state with explicit structure.\n\n# Appendix: Briefing Digest\n{}\n",
        escape_markdown_inline(&cfg.agent_id),
        investigations.len(),
        mempool.len(),
        events.len(),
        blank_if_empty(&investigations_preview),
        blank_if_empty(&mempool_preview),
        blank_if_empty(&events_preview),
        render_text_block(&truncate(briefing, 2400)),
    )
}

fn preview_investigations(items: &[Investigation], limit: usize) -> Vec<String> {
    items
        .iter()
        .take(limit)
        .map(|item| {
            item.title
                .as_deref()
                .or(item.id.as_deref())
                .unwrap_or("<unnamed investigation>")
                .to_string()
        })
        .collect()
}

fn preview_papers(items: &[Paper], limit: usize) -> Vec<String> {
    items
        .iter()
        .take(limit)
        .map(|item| {
            item.title
                .as_deref()
                .or(item.paper_id.as_deref())
                .unwrap_or("<untitled paper>")
                .to_string()
        })
        .collect()
}

fn preview_events(items: &[HiveEvent], limit: usize) -> Vec<String> {
    items
        .iter()
        .take(limit)
        .map(|item| {
            item.kind
                .as_deref()
                .or_else(|| item.event_id.as_deref())
                .map(str::to_string)
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
        // No work recorded yet; use a neutral midpoint rather than divide by zero.
        0.5
    } else {
        state.hive_compute_cycles as f64 / total as f64
    }
}

fn nash_compliant(state: &BridgePersistentState) -> bool {
    state.hive_compute_cycles > 0
        && state.local_compute_cycles > 0
        && compute_split_ratio(state) >= 0.4
}

fn maybe_send_heartbeat(
    cfg: &P2PClawConfig,
    bridge_cfg: &BridgeConfig,
    dry_run: bool,
    state: &mut BridgePersistentState,
) -> Result<(), String> {
    let now = now_unix_secs();
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
        if let Err(err) = p2pclaw::send_chat(cfg, &message, Some("operations")) {
            let _ = trace_bridge_operation(
                "p2pclaw_bridge_heartbeat",
                Some("operations".to_string()),
                false,
                Some(err.clone()),
            );
            return Err(err);
        }
        state.chat_messages_total += 1;
        state.hive_compute_cycles += 1;
        let _ = trace_bridge_operation(
            "p2pclaw_bridge_heartbeat",
            Some("operations".to_string()),
            true,
            None,
        );
    }
    state.last_heartbeat_at = Some(now);
    Ok(())
}

fn maybe_report_tau_sync(
    cfg: &P2PClawConfig,
    dry_run: bool,
    state: &mut BridgePersistentState,
) -> Result<(), String> {
    let now = now_unix_secs();
    let response = if dry_run {
        serde_json::json!({
            "status": "dry_run",
            "agent_id": cfg.agent_id,
            "compute_cycles": state.hive_compute_cycles,
        })
    } else {
        match p2pclaw::report_tau_tick(cfg, state.hive_compute_cycles) {
            Ok(response) => {
                state.hive_compute_cycles += 1;
                let _ = trace_bridge_operation(
                    "p2pclaw_bridge_tau_sync",
                    Some(cfg.agent_id.clone()),
                    true,
                    None,
                );
                response
            }
            Err(err) => {
                state.last_tau_sync_error = Some(err.clone());
                let _ = trace_bridge_operation(
                    "p2pclaw_bridge_tau_sync",
                    Some(cfg.agent_id.clone()),
                    false,
                    Some(err.clone()),
                );
                return Err(err);
            }
        }
    };
    state.last_tau_sync_at = Some(now);
    state.tau_reports_total += 1;
    state.last_tau_sync_response = Some(response);
    state.last_tau_sync_error = None;
    Ok(())
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
        return input.to_string();
    }
    let boundary = input
        .char_indices()
        .take_while(|(idx, _)| *idx < max_len)
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    if boundary == 0 {
        "...".to_string()
    } else {
        format!("{}...", &input[..boundary])
    }
}

fn write_state_unlocked(state: &BridgePersistentState) -> Result<(), String> {
    let path = state_path();
    let raw = serde_json::to_string_pretty(state)
        .map_err(|e| format!("serialize bridge state {}: {e}", path.display()))?;
    write_atomic_private_file(&path, &raw)
}

fn write_atomic_private_file(path: &PathBuf, raw: &str) -> Result<(), String> {
    config::ensure_halo_dir()?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod 600 {}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

fn persist_failure(bridge_cfg: &BridgeConfig, now: u64, err: String) -> Result<(), String> {
    let mut locked = LockedBridgeState::load()?;
    locked.state.version = STATE_VERSION;
    locked.state.last_run_at = Some(now);
    locked.state.polls_total += 1;
    locked.state.consecutive_failures += 1;
    locked.state.last_error = Some(err);
    locked.state.current_backoff_secs =
        failure_backoff_secs(locked.state.current_backoff_secs, bridge_cfg);
    locked.state.next_poll_not_before = Some(now + locked.state.current_backoff_secs);
    locked.persist()
}

fn dedup_events(events: &[HiveEvent], recent_keys: &[String]) -> Vec<HiveEvent> {
    let mut seen = recent_keys
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut unique = Vec::new();
    for event in events {
        let key = event_key(event);
        if seen.insert(key) {
            unique.push(event.clone());
        }
    }
    unique
}

fn event_key(event: &HiveEvent) -> String {
    if let Some(id) = event.event_id.as_deref().filter(|v| !v.trim().is_empty()) {
        return format!("id:{id}");
    }
    let kind = event.kind.as_deref().unwrap_or("unknown");
    let ts = event.timestamp.unwrap_or(0);
    format!("kind:{kind}:ts:{ts}")
}

fn extend_recent_event_keys(target: &mut Vec<String>, events: &[HiveEvent]) {
    let mut queue = target.drain(..).collect::<VecDeque<_>>();
    for event in events {
        let key = event_key(event);
        if queue.contains(&key) {
            continue;
        }
        queue.push_back(key);
        while queue.len() > MAX_RECENT_EVENT_KEYS {
            let _ = queue.pop_front();
        }
    }
    *target = queue.into_iter().collect();
}

fn escape_markdown_inline(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '!'
            | '>' | '|' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn render_text_block(input: &str) -> String {
    let mut lines = input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(|line| format!("    {}", line.replace('\t', "    ")))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push("    <empty briefing>".to_string());
    }
    lines.join("\n")
}

fn append_trace_note(detail: String, trace_error: Option<String>) -> String {
    match trace_error {
        Some(err) => format!("{detail} (trace warning: {err})"),
        None => detail,
    }
}

fn trace_bridge_operation(
    operation_type: &str,
    result_digest: Option<String>,
    success: bool,
    error: Option<String>,
) -> Result<(), String> {
    record_paid_operation_for_halo(operation_type, 0, None, result_digest, success, error)
}

fn shutdown_flag() -> Arc<AtomicBool> {
    static SHUTDOWN: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    SHUTDOWN
        .get_or_init(|| {
            let flag = Arc::new(AtomicBool::new(false));
            #[cfg(unix)]
            {
                let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, flag.clone());
                let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, flag.clone());
            }
            flag
        })
        .clone()
}

fn sleep_with_shutdown(duration: Duration, shutdown: Arc<AtomicBool>) {
    let deadline = Instant::now() + duration;
    while !shutdown.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(now);
        thread::sleep(remaining.min(Duration::from_millis(SHUTDOWN_SLEEP_SLICE_MS)));
    }
}

fn sha256_hex(input: &[u8]) -> String {
    hex::encode(Sha256::digest(input))
}
