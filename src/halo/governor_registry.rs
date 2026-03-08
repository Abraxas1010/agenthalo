//! Runtime governor registry and telemetry surfaces.

use crate::halo::governor::{GovernorConfig, GovernorState, StabilityReport};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

const SPARKLINE_CAPACITY: usize = 60;

#[derive(Clone, Debug, Serialize)]
pub struct GovernorSnapshot {
    pub instance_id: String,
    pub epsilon: f64,
    pub target: f64,
    pub measured_signal: f64,
    pub error: f64,
    pub lyapunov: f64,
    pub regime: String,
    pub gamma: Option<f64>,
    pub contraction_bound: Option<f64>,
    pub stable: bool,
    pub oscillating: bool,
    pub gain_violated: bool,
    pub clamp_active: bool,
    pub formal_basis: String,
    pub sparkline: Vec<f64>,
    pub last_updated_unix: u64,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GovernorObservation {
    pub epsilon: f64,
    pub error: f64,
    pub lyapunov: f64,
    pub oscillating: bool,
}

#[derive(Debug, Default)]
struct GovernorTelemetry {
    measured_signal: f64,
    error: f64,
    lyapunov: f64,
    oscillating: bool,
    clamp_active: bool,
    sparkline: VecDeque<f64>,
    last_updated_unix: u64,
}

struct GovernorEntry {
    state: Arc<Mutex<GovernorState>>,
    telemetry: Mutex<GovernorTelemetry>,
}

#[derive(Default)]
pub struct GovernorRegistry {
    instances: RwLock<HashMap<String, Arc<GovernorEntry>>>,
}

static GLOBAL_GOVERNOR_REGISTRY: OnceLock<Arc<GovernorRegistry>> = OnceLock::new();

impl GovernorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, config: GovernorConfig) -> Result<(), String> {
        let mut instances = self
            .instances
            .write()
            .map_err(|e| format!("governor registry write lock poisoned: {e}"))?;
        if instances.contains_key(&config.instance_id) {
            return Err(format!(
                "governor instance `{}` already registered",
                config.instance_id
            ));
        }
        let instance_id = config.instance_id.clone();
        instances.insert(
            instance_id,
            Arc::new(GovernorEntry {
                state: Arc::new(Mutex::new(GovernorState::new(config))),
                telemetry: Mutex::new(GovernorTelemetry::default()),
            }),
        );
        Ok(())
    }

    pub fn get(&self, instance_id: &str) -> Option<Arc<Mutex<GovernorState>>> {
        let instances = self.instances.read().ok()?;
        instances.get(instance_id).map(|entry| entry.state.clone())
    }

    pub fn observe(
        &self,
        instance_id: &str,
        measured_signal: f64,
    ) -> Result<GovernorObservation, String> {
        let entry = self
            .instances
            .read()
            .map_err(|e| format!("governor registry read lock poisoned: {e}"))?
            .get(instance_id)
            .cloned()
            .ok_or_else(|| format!("governor instance `{instance_id}` not registered"))?;

        let mut state = entry
            .state
            .lock()
            .map_err(|e| format!("governor state lock poisoned: {e}"))?;
        let epsilon = state.step(measured_signal);
        let error = state.error(measured_signal);
        let lyapunov = state
            .last_lyapunov
            .unwrap_or_else(|| state.lyapunov(measured_signal));
        let oscillating = state.oscillating;
        let clamp_active = state.clamp_active;
        drop(state);

        let mut telemetry = entry
            .telemetry
            .lock()
            .map_err(|e| format!("governor telemetry lock poisoned: {e}"))?;
        telemetry.measured_signal = measured_signal;
        telemetry.error = error;
        telemetry.lyapunov = lyapunov;
        telemetry.oscillating = oscillating;
        telemetry.clamp_active = clamp_active;
        telemetry.last_updated_unix = now_unix();
        telemetry.sparkline.push_back(epsilon);
        while telemetry.sparkline.len() > SPARKLINE_CAPACITY {
            telemetry.sparkline.pop_front();
        }

        Ok(GovernorObservation {
            epsilon,
            error,
            lyapunov,
            oscillating: telemetry.oscillating,
        })
    }

    pub fn soft_reset(&self, instance_id: &str) -> Result<(), String> {
        let entry = self
            .instances
            .read()
            .map_err(|e| format!("governor registry read lock poisoned: {e}"))?
            .get(instance_id)
            .cloned()
            .ok_or_else(|| format!("governor instance `{instance_id}` not registered"))?;
        {
            let mut state = entry
                .state
                .lock()
                .map_err(|e| format!("governor state lock poisoned: {e}"))?;
            state.reset();
        }
        let mut telemetry = entry
            .telemetry
            .lock()
            .map_err(|e| format!("governor telemetry lock poisoned: {e}"))?;
        telemetry.oscillating = false;
        telemetry.error = 0.0;
        telemetry.lyapunov = 0.0;
        Ok(())
    }

    pub fn validate_all(&self) -> Vec<(String, Result<StabilityReport, String>)> {
        let instances = match self.instances.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        instances
            .iter()
            .map(|(id, entry)| {
                let result = entry
                    .state
                    .lock()
                    .map_err(|e| format!("governor state lock poisoned: {e}"))
                    .and_then(|state| state.validate_params());
                (id.clone(), result)
            })
            .collect()
    }

    pub fn snapshot_all(&self) -> Vec<GovernorSnapshot> {
        let instances = match self.instances.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut snapshots = instances
            .iter()
            .filter_map(|(id, entry)| self.snapshot_entry(id, entry).ok())
            .collect::<Vec<_>>();
        snapshots.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
        snapshots
    }

    pub fn snapshot_one(&self, instance_id: &str) -> Result<GovernorSnapshot, String> {
        let entry = self
            .instances
            .read()
            .map_err(|e| format!("governor registry read lock poisoned: {e}"))?
            .get(instance_id)
            .cloned()
            .ok_or_else(|| format!("governor instance `{instance_id}` not registered"))?;
        self.snapshot_entry(instance_id, &entry)
    }

    fn snapshot_entry(
        &self,
        instance_id: &str,
        entry: &Arc<GovernorEntry>,
    ) -> Result<GovernorSnapshot, String> {
        let state = entry
            .state
            .lock()
            .map_err(|e| format!("governor state lock poisoned: {e}"))?
            .clone();
        let telemetry = entry
            .telemetry
            .lock()
            .map_err(|e| format!("governor telemetry lock poisoned: {e}"))?;
        let validation = state.validate_params();
        let (gamma, contraction_bound, regime, gain_violated, warning) = match validation {
            Ok(report) => (
                Some(report.gamma),
                Some(report.contraction_bound),
                state.regime_label(),
                false,
                state.formal_warning(),
            ),
            Err(err) => (
                None,
                None,
                "outside formal regime".to_string(),
                true,
                Some(err),
            ),
        };
        Ok(GovernorSnapshot {
            instance_id: instance_id.to_string(),
            epsilon: state.epsilon,
            target: state.config.target,
            measured_signal: telemetry.measured_signal,
            error: telemetry.error,
            lyapunov: telemetry.lyapunov,
            regime,
            gamma,
            contraction_bound,
            stable: !gain_violated && !telemetry.oscillating,
            oscillating: telemetry.oscillating,
            gain_violated,
            clamp_active: telemetry.clamp_active,
            formal_basis: state.config.formal_basis.clone(),
            sparkline: telemetry.sparkline.iter().copied().collect(),
            last_updated_unix: telemetry.last_updated_unix,
            warning,
        })
    }
}

pub fn install_global_registry(registry: Arc<GovernorRegistry>) {
    let _ = GLOBAL_GOVERNOR_REGISTRY.set(registry);
}

pub fn global_registry() -> Option<Arc<GovernorRegistry>> {
    GLOBAL_GOVERNOR_REGISTRY.get().cloned()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::governor::GovernorConfig;

    fn registry_with_one() -> GovernorRegistry {
        let registry = GovernorRegistry::new();
        registry
            .register(GovernorConfig {
                instance_id: "gov-proxy".to_string(),
                alpha: 0.01,
                beta: 0.05,
                dt: 1.0,
                eps_min: 1.0,
                eps_max: 50.0,
                target: 2.0,
                formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.lyapunov_descent"
                    .to_string(),
            })
            .expect("register");
        registry
    }

    #[test]
    fn observe_updates_snapshot() {
        let registry = registry_with_one();
        let observation = registry.observe("gov-proxy", 2.2).expect("observe");
        assert!(observation.epsilon >= 1.0);
        let snapshot = registry.snapshot_one("gov-proxy").expect("snapshot");
        assert_eq!(snapshot.instance_id, "gov-proxy");
        assert!(!snapshot.sparkline.is_empty());
        assert!(snapshot.regime.contains("engineering multi-step"));
    }

    #[test]
    fn validate_all_reports_registered_instance() {
        let registry = registry_with_one();
        let reports = registry.validate_all();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].1.is_ok());
    }

    #[test]
    fn soft_reset_clears_from_rest_memory() {
        let registry = registry_with_one();
        registry.observe("gov-proxy", 3.0).expect("observe");
        registry.soft_reset("gov-proxy").expect("reset");
        let handle = registry.get("gov-proxy").expect("handle");
        let state = handle.lock().expect("state lock");
        assert_eq!(state.e_prev, 0.0);
        assert!(state.is_from_rest());
    }
}
