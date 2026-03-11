//! Shared telemetry wrappers that feed the governor registry.
//!
//! These helpers centralize EWMA state so multiple runtime surfaces do not
//! accidentally maintain parallel windows for the same governor instance.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct EwmaWindow {
    last_sample: Option<Instant>,
    last_observed: Option<Instant>,
    rate_ewma: f64,
}

static COMMS_WINDOW: OnceLock<Mutex<EwmaWindow>> = OnceLock::new();

pub fn record_comms_batch(batch_size: usize) {
    let Some(registry) = crate::halo::governor_registry::global_registry() else {
        return;
    };
    let state = COMMS_WINDOW.get_or_init(|| Mutex::new(EwmaWindow::default()));
    let now = Instant::now();
    let mut guard = match state.lock() {
        Ok(lock) => lock,
        Err(poisoned) => poisoned.into_inner(),
    };
    let sample = match guard.last_sample {
        Some(previous) => {
            let elapsed = (now - previous).as_secs_f64().max(1e-3);
            batch_size as f64 / elapsed
        }
        None => batch_size as f64,
    };
    guard.last_sample = Some(now);
    guard.last_observed = Some(now);
    guard.rate_ewma = if guard.rate_ewma == 0.0 {
        sample
    } else {
        guard.rate_ewma * 0.9 + sample * 0.1
    };
    let _ = registry.observe("gov-comms", guard.rate_ewma);
}

pub fn soft_reset_comms_if_quiescent(idle_for: Duration) {
    let Some(registry) = crate::halo::governor_registry::global_registry() else {
        return;
    };
    let state = COMMS_WINDOW.get_or_init(|| Mutex::new(EwmaWindow::default()));
    let now = Instant::now();
    let mut guard = match state.lock() {
        Ok(lock) => lock,
        Err(poisoned) => poisoned.into_inner(),
    };
    let is_idle = guard
        .last_observed
        .map(|previous| now.duration_since(previous) >= idle_for)
        .unwrap_or(true);
    if !is_idle {
        return;
    }
    guard.last_sample = None;
    guard.last_observed = None;
    guard.rate_ewma = 0.0;
    let _ = registry.soft_reset("gov-comms");
}
