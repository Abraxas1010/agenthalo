//! AETHER PD governor core ported from the verified Rust artifact.
//!
//! Provenance: `artifacts/aether_verified/rust/aether_governor.rs`
//! Formal basis: `HeytingLean.Bridge.Sharma.AetherGovernor`
//!
//! The verified guarantee is single-step Lyapunov descent in the from-rest,
//! no-clamp regime. Multi-step convergence is intentionally not claimed here.

use serde::{Deserialize, Serialize};

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

/// (AETHER Rust artifact: `governor_error`; Lean: `govError`)
pub fn governor_error(r_target: f64, delta: f64, epsilon: f64) -> f64 {
    delta / epsilon - r_target
}

/// (AETHER Rust artifact: `governor_step`; Lean: `govStep`)
pub fn governor_step(
    epsilon: f64,
    e_prev: f64,
    delta: f64,
    dt: f64,
    alpha: f64,
    beta: f64,
    eps_min: f64,
    eps_max: f64,
    r_target: f64,
) -> f64 {
    let e = governor_error(r_target, delta, epsilon);
    let d_error = (e - e_prev) / dt;
    let adjustment = alpha * e + beta * d_error;
    clamp(epsilon + adjustment, eps_min, eps_max)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GovernorConfig {
    pub instance_id: String,
    pub alpha: f64,
    pub beta: f64,
    pub dt: f64,
    pub eps_min: f64,
    pub eps_max: f64,
    pub target: f64,
    pub formal_basis: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GovernorState {
    pub config: GovernorConfig,
    pub epsilon: f64,
    pub e_prev: f64,
    #[serde(default)]
    pub last_measured_signal: Option<f64>,
    #[serde(default)]
    pub last_lyapunov: Option<f64>,
    #[serde(default)]
    pub oscillating: bool,
    #[serde(default)]
    pub clamp_active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StabilityReport {
    pub gamma: f64,
    pub contraction_bound: f64,
    pub regime: String,
}

impl GovernorState {
    pub fn new(config: GovernorConfig) -> Self {
        Self {
            epsilon: config.eps_min,
            e_prev: 0.0,
            config,
            last_measured_signal: None,
            last_lyapunov: None,
            oscillating: false,
            clamp_active: false,
        }
    }

    pub fn error(&self, delta: f64) -> f64 {
        governor_error(self.config.target, delta, self.epsilon)
    }

    pub fn lyapunov(&self, delta: f64) -> f64 {
        let e = self.error(delta);
        0.5 * e * e
    }

    pub fn gamma(&self) -> f64 {
        self.config.alpha + self.config.beta / self.config.dt
    }

    pub fn contraction_bound(&self) -> f64 {
        (1.0 - self.config.alpha).max(0.0)
    }

    pub fn is_from_rest(&self) -> bool {
        self.last_measured_signal.is_none() && self.e_prev.abs() <= f64::EPSILON
    }

    pub fn regime_label(&self) -> String {
        if self.validate_params().is_err() {
            return "outside formal regime".to_string();
        }
        if self.is_from_rest() && !self.clamp_active {
            "single-step from-rest no-clamp".to_string()
        } else if self.is_from_rest() {
            "from-rest clamp-active (outside formal proof)".to_string()
        } else {
            "engineering multi-step (formal scope exited after first observation)".to_string()
        }
    }

    pub fn formal_warning(&self) -> Option<String> {
        if self.validate_params().is_err() {
            return None;
        }
        if self.is_from_rest() && !self.clamp_active {
            None
        } else if self.is_from_rest() {
            Some("Clamp activity has exited the single-step formal proof regime.".to_string())
        } else if self.oscillating {
            Some(
                "Multi-step operation is empirical only and has shown Lyapunov non-descent."
                    .to_string(),
            )
        } else {
            Some(
                "Multi-step operation is empirical only; the single-step from-rest proof no longer applies."
                    .to_string(),
            )
        }
    }

    /// One PD step with clamped epsilon.
    ///
    /// Formal limitation: after this step, `e_prev` is no longer zero, so the
    /// single-step from-rest proof regime has been exited unless `reset()` is
    /// called explicitly.
    pub fn step(&mut self, delta: f64) -> f64 {
        let had_history = self.last_measured_signal.is_some();
        let previous_lyapunov = self.lyapunov(delta);
        let e = self.error(delta);
        self.epsilon = governor_step(
            self.epsilon,
            self.e_prev,
            delta,
            self.config.dt,
            self.config.alpha,
            self.config.beta,
            self.config.eps_min,
            self.config.eps_max,
            self.config.target,
        );
        self.e_prev = e;
        self.clamp_active = (self.epsilon - self.config.eps_min).abs() < f64::EPSILON
            || (self.epsilon - self.config.eps_max).abs() < f64::EPSILON;
        let lyapunov = self.lyapunov(delta);
        self.oscillating = had_history && lyapunov > previous_lyapunov;
        self.last_measured_signal = Some(delta);
        self.last_lyapunov = Some(lyapunov);
        self.epsilon
    }

    pub fn validate_params(&self) -> Result<StabilityReport, String> {
        if !(self.config.alpha > 0.0) {
            return Err(format!(
                "alpha={} must be > 0 for the PD controller",
                self.config.alpha
            ));
        }
        if self.config.beta < 0.0 {
            return Err(format!(
                "beta={} must be >= 0 for the PD controller",
                self.config.beta
            ));
        }
        if self.config.dt < 1.0 {
            return Err(format!(
                "dt={} < 1.0 — outside formal guarantee regime",
                self.config.dt
            ));
        }
        if !(self.config.eps_min > 0.0) {
            return Err(format!("eps_min={} must be > 0", self.config.eps_min));
        }
        if self.config.eps_max <= self.config.eps_min {
            return Err(format!(
                "eps_max={} must be > eps_min={}",
                self.config.eps_max, self.config.eps_min
            ));
        }
        let gamma = self.gamma();
        if gamma >= 1.0 {
            return Err(format!(
                "gamma={gamma:.6} >= 1.0 — gain condition α + β/dt < 1 violated"
            ));
        }
        Ok(StabilityReport {
            gamma,
            contraction_bound: self.contraction_bound(),
            regime: "single-step from-rest no-clamp".to_string(),
        })
    }

    pub fn reset(&mut self) {
        self.e_prev = 0.0;
        self.last_measured_signal = None;
        self.last_lyapunov = None;
        self.oscillating = false;
        self.clamp_active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> GovernorState {
        GovernorState::new(GovernorConfig {
            instance_id: "gov-test".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 1.0,
            eps_max: 50.0,
            target: 2.0,
            formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.lyapunov_descent".to_string(),
        })
    }

    #[test]
    fn validate_params_accepts_formal_regime() {
        let state = default_state();
        let report = state.validate_params().expect("valid regime");
        assert!(report.gamma < 1.0);
        assert_eq!(report.regime, "single-step from-rest no-clamp");
    }

    #[test]
    fn validate_params_rejects_bad_gain() {
        let mut state = default_state();
        state.config.alpha = 0.8;
        state.config.beta = 0.3;
        let err = state.validate_params().expect_err("invalid gamma");
        assert!(err.contains("gain condition"));
    }

    #[test]
    fn single_step_from_rest_can_reduce_lyapunov() {
        let mut state = default_state();
        let before = state.lyapunov(2.2);
        state.step(2.2);
        let after = state.lyapunov(2.2);
        assert!(
            after < before,
            "expected Lyapunov descent: {before} -> {after}"
        );
    }

    #[test]
    fn step_respects_clamp_bounds() {
        let mut state = default_state();
        state.epsilon = state.config.eps_max;
        state.step(10_000.0);
        assert!(state.epsilon <= state.config.eps_max);

        state.epsilon = state.config.eps_min;
        state.e_prev = 100.0;
        state.step(0.0);
        assert!(state.epsilon >= state.config.eps_min);
    }

    #[test]
    fn reset_returns_to_from_rest_state() {
        let mut state = default_state();
        state.step(3.0);
        assert_ne!(state.e_prev, 0.0);
        state.reset();
        assert_eq!(state.e_prev, 0.0);
        assert!(state.is_from_rest());
        assert_eq!(state.regime_label(), "single-step from-rest no-clamp");
    }

    #[test]
    fn regime_exits_after_first_observation() {
        let mut state = default_state();
        assert!(state.is_from_rest());
        state.step(2.2);
        assert!(!state.is_from_rest());
        assert!(state.regime_label().contains("engineering multi-step"));
    }
}
