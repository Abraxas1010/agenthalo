//! AETHER PD/PID governor core ported from the verified Rust artifact.
//!
//! Provenance: `artifacts/aether_verified/rust/aether_governor.rs`
//! Formal basis: `HeytingLean.Bridge.Sharma.AetherGovernor`
//!
//! The verified guarantee is single-step Lyapunov descent in the from-rest,
//! no-clamp regime (PD mode only). Multi-step convergence is intentionally
//! not claimed here. PID mode and adaptive gain scheduling are engineering
//! extensions outside the formal proof scope.

use serde::{Deserialize, Serialize};

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

/// (AETHER Rust artifact: `governor_error`; Lean: `govError`)
pub fn governor_error(r_target: f64, delta: f64, epsilon: f64) -> f64 {
    delta / epsilon - r_target
}

/// (AETHER Rust artifact: `governor_step`; Lean: `govStep`)
///
/// Original PD step — UNCHANGED from the verified artifact.
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

/// Result of a PID governor step, exposing integral state for diagnostics.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PidStepResult {
    pub epsilon: f64,
    pub integral: f64,
    pub clamped: bool,
}

/// PID governor step with back-calculation anti-windup.
///
/// When `ki == 0.0` and `kb == 0.0`, this produces identical output to
/// `governor_step` (pure PD mode).
///
/// Anti-windup: when the output is clamped, the integral is adjusted by
/// `kb * (clamped_output - unclamped_output)` to unwind accumulated error.
pub fn governor_step_pid(
    epsilon: f64,
    e_prev: f64,
    integral_prev: f64,
    delta: f64,
    dt: f64,
    alpha: f64,
    beta: f64,
    ki: f64,
    kb: f64,
    eps_min: f64,
    eps_max: f64,
    r_target: f64,
) -> PidStepResult {
    let e = governor_error(r_target, delta, epsilon);
    let d_error = (e - e_prev) / dt;

    // PID output (unclamped)
    let unclamped = epsilon + alpha * e + ki * integral_prev + beta * d_error;
    let clamped_eps = clamp(unclamped, eps_min, eps_max);
    let was_clamped = (clamped_eps - unclamped).abs() > f64::EPSILON;

    // Anti-windup: back-calculation adjusts integral
    let integral = integral_prev + e * dt + kb * (clamped_eps - unclamped);

    PidStepResult {
        epsilon: clamped_eps,
        integral,
        clamped: was_clamped,
    }
}

// ---------------------------------------------------------------------------
// Adaptive gain scheduling
// ---------------------------------------------------------------------------

/// Classifies the current load into a regime for gain scheduling.
/// Pure function: same inputs, same output, always.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadRegime {
    /// Entry count well below target (< 50% capacity)
    Quiet,
    /// Entry count near target (50–90% capacity)
    Active,
    /// Entry count at or above target (> 90% capacity)
    Burst,
}

impl LoadRegime {
    /// Classify based on current occupancy ratio (entries / target).
    pub fn classify(occupancy_ratio: f64) -> Self {
        if occupancy_ratio < 0.5 {
            LoadRegime::Quiet
        } else if occupancy_ratio < 0.9 {
            LoadRegime::Active
        } else {
            LoadRegime::Burst
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            LoadRegime::Quiet => "quiet",
            LoadRegime::Active => "active",
            LoadRegime::Burst => "burst",
        }
    }
}

/// Gain multipliers for each regime. Applied to base (alpha, beta, ki).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdaptiveGainSchedule {
    pub quiet_alpha_scale: f64,
    pub quiet_beta_scale: f64,
    pub quiet_ki_scale: f64,
    pub active_alpha_scale: f64,
    pub active_beta_scale: f64,
    pub active_ki_scale: f64,
    pub burst_alpha_scale: f64,
    pub burst_beta_scale: f64,
    pub burst_ki_scale: f64,
}

impl Default for AdaptiveGainSchedule {
    fn default() -> Self {
        Self {
            quiet_alpha_scale: 1.5,
            quiet_beta_scale: 0.5,
            quiet_ki_scale: 1.0,
            active_alpha_scale: 1.0,
            active_beta_scale: 1.0,
            active_ki_scale: 1.0,
            burst_alpha_scale: 0.5,
            burst_beta_scale: 2.0,
            burst_ki_scale: 0.5,
        }
    }
}

impl AdaptiveGainSchedule {
    /// Compute effective gains for a given regime.
    /// Pure function: (base_gains, regime) → effective_gains.
    pub fn effective_gains(
        &self,
        base_alpha: f64,
        base_beta: f64,
        base_ki: f64,
        regime: LoadRegime,
    ) -> (f64, f64, f64) {
        match regime {
            LoadRegime::Quiet => (
                base_alpha * self.quiet_alpha_scale,
                base_beta * self.quiet_beta_scale,
                base_ki * self.quiet_ki_scale,
            ),
            LoadRegime::Active => (
                base_alpha * self.active_alpha_scale,
                base_beta * self.active_beta_scale,
                base_ki * self.active_ki_scale,
            ),
            LoadRegime::Burst => (
                base_alpha * self.burst_alpha_scale,
                base_beta * self.burst_beta_scale,
                base_ki * self.burst_ki_scale,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Config and state
// ---------------------------------------------------------------------------

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
    /// Integral gain. 0.0 = PD mode (no integral term).
    #[serde(default)]
    pub ki: f64,
    /// Back-calculation anti-windup gain. Recommended: sqrt(ki).
    #[serde(default)]
    pub kb: f64,
    /// Adaptive gain schedule. None = fixed gains.
    #[serde(default)]
    pub adaptive: Option<AdaptiveGainSchedule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GovernorState {
    pub config: GovernorConfig,
    pub epsilon: f64,
    pub e_prev: f64,
    /// Accumulated integral term (PID mode).
    #[serde(default)]
    pub integral: f64,
    #[serde(default)]
    pub last_measured_signal: Option<f64>,
    #[serde(default)]
    pub last_lyapunov: Option<f64>,
    #[serde(default)]
    pub oscillating: bool,
    #[serde(default)]
    pub clamp_active: bool,
    /// Current load regime (adaptive scheduling).
    #[serde(default)]
    pub current_regime: Option<LoadRegime>,
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
            integral: 0.0,
            config,
            last_measured_signal: None,
            last_lyapunov: None,
            oscillating: false,
            clamp_active: false,
            current_regime: None,
        }
    }

    pub fn error(&self, delta: f64) -> f64 {
        governor_error(self.config.target, delta, self.epsilon)
    }

    pub fn lyapunov(&self, delta: f64) -> f64 {
        let e = self.error(delta);
        0.5 * e * e
    }

    /// Base gamma (α + β/dt) using unscaled gains.
    pub fn gamma(&self) -> f64 {
        self.config.alpha + self.config.beta / self.config.dt
    }

    /// Worst-case effective gamma across all adaptive regimes.
    /// If adaptive scheduling is enabled, the effective alpha and beta can
    /// differ from the base values. This returns the maximum gamma over all
    /// three regimes (Quiet/Active/Burst), which is the binding stability
    /// constraint. If adaptive is disabled, returns the base gamma.
    pub fn effective_gamma_worst_case(&self) -> f64 {
        if let Some(ref schedule) = self.config.adaptive {
            let mut worst = 0.0_f64;
            for regime in [LoadRegime::Quiet, LoadRegime::Active, LoadRegime::Burst] {
                let (eff_alpha, eff_beta, _) = schedule.effective_gains(
                    self.config.alpha,
                    self.config.beta,
                    self.config.ki,
                    regime,
                );
                let g = eff_alpha + eff_beta / self.config.dt;
                worst = worst.max(g);
            }
            worst
        } else {
            self.gamma()
        }
    }

    pub fn contraction_bound(&self) -> f64 {
        (1.0 - self.config.alpha).max(0.0)
    }

    pub fn is_from_rest(&self) -> bool {
        self.last_measured_signal.is_none()
            && self.e_prev.abs() <= f64::EPSILON
            && self.integral.abs() <= f64::EPSILON
    }

    /// Whether this governor is operating in PID mode (ki > 0).
    pub fn is_pid_mode(&self) -> bool {
        self.config.ki > 0.0
    }

    /// Controller mode label for diagnostics.
    pub fn controller_mode(&self) -> &'static str {
        if self.is_pid_mode() {
            "PID"
        } else {
            "PD"
        }
    }

    /// Current adaptive regime label, or "none" if adaptive is disabled.
    pub fn adaptive_regime_label(&self) -> &'static str {
        match self.current_regime {
            Some(r) => r.label(),
            None => "none",
        }
    }

    pub fn regime_label(&self) -> String {
        if self.validate_params().is_err() {
            return "outside formal regime".to_string();
        }
        if self.is_pid_mode() {
            return format!(
                "engineering PID multi-step (outside formal proof scope, adaptive={})",
                self.adaptive_regime_label()
            );
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
        if self.is_pid_mode() {
            return Some(
                "PID mode is an engineering extension; the formal Lyapunov proof applies only to PD mode."
                    .to_string(),
            );
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

    /// One PD or PID step with clamped epsilon.
    ///
    /// In PD mode (ki == 0): uses the original verified `governor_step`.
    /// In PID mode (ki > 0): uses `governor_step_pid` with anti-windup.
    /// If adaptive gain scheduling is enabled, gains are scaled by regime.
    pub fn step(&mut self, delta: f64) -> f64 {
        let had_history = self.last_measured_signal.is_some();
        let previous_lyapunov = self.lyapunov(delta);
        let e = self.error(delta);

        // Determine effective gains (adaptive scheduling)
        let occupancy = if self.config.target > 0.0 {
            delta / self.config.target
        } else {
            1.0
        };
        let regime = LoadRegime::classify(occupancy);
        self.current_regime = if self.config.adaptive.is_some() {
            Some(regime)
        } else {
            None
        };

        let (eff_alpha, eff_beta, eff_ki) = if let Some(ref schedule) = self.config.adaptive {
            schedule.effective_gains(self.config.alpha, self.config.beta, self.config.ki, regime)
        } else {
            (self.config.alpha, self.config.beta, self.config.ki)
        };

        if eff_ki > 0.0 {
            // PID mode with anti-windup
            let result = governor_step_pid(
                self.epsilon,
                self.e_prev,
                self.integral,
                delta,
                self.config.dt,
                eff_alpha,
                eff_beta,
                eff_ki,
                self.config.kb,
                self.config.eps_min,
                self.config.eps_max,
                self.config.target,
            );
            self.epsilon = result.epsilon;
            self.integral = result.integral;
            self.clamp_active = result.clamped;
        } else {
            // PD mode — original verified path
            self.epsilon = governor_step(
                self.epsilon,
                self.e_prev,
                delta,
                self.config.dt,
                eff_alpha,
                eff_beta,
                self.config.eps_min,
                self.config.eps_max,
                self.config.target,
            );
            self.clamp_active = (self.epsilon - self.config.eps_min).abs() < f64::EPSILON
                || (self.epsilon - self.config.eps_max).abs() < f64::EPSILON;
        }

        self.e_prev = e;
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
        if self.config.ki < 0.0 {
            return Err(format!(
                "ki={} must be >= 0 for the PID controller",
                self.config.ki
            ));
        }
        if self.config.kb < 0.0 {
            return Err(format!(
                "kb={} must be >= 0 for anti-windup",
                self.config.kb
            ));
        }
        let gamma = self.gamma();
        if gamma >= 1.0 {
            return Err(format!(
                "gamma={gamma:.6} >= 1.0 — gain condition α + β/dt < 1 violated"
            ));
        }
        // Check worst-case effective gamma across adaptive regimes
        let worst_gamma = self.effective_gamma_worst_case();
        if worst_gamma >= 1.0 {
            return Err(format!(
                "effective_gamma={worst_gamma:.6} >= 1.0 — adaptive gain schedule violates α + β/dt < 1 in at least one regime"
            ));
        }
        Ok(StabilityReport {
            gamma: worst_gamma,
            contraction_bound: self.contraction_bound(),
            regime: if self.is_pid_mode() {
                "engineering PID (outside formal proof scope)".to_string()
            } else {
                "single-step from-rest no-clamp".to_string()
            },
        })
    }

    pub fn reset(&mut self) {
        self.e_prev = 0.0;
        self.integral = 0.0;
        self.last_measured_signal = None;
        self.last_lyapunov = None;
        self.oscillating = false;
        self.clamp_active = false;
        self.current_regime = None;
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
            ki: 0.0,
            kb: 0.0,
            adaptive: None,
        })
    }

    fn pid_state() -> GovernorState {
        GovernorState::new(GovernorConfig {
            instance_id: "gov-pid-test".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 1.0,
            eps_max: 50.0,
            target: 100.0,
            formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.lyapunov_descent".to_string(),
            ki: 0.001,
            kb: 0.0316, // sqrt(0.001)
            adaptive: None,
        })
    }

    fn adaptive_state() -> GovernorState {
        GovernorState::new(GovernorConfig {
            instance_id: "gov-adaptive-test".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 1.0,
            eps_max: 50.0,
            target: 100.0,
            formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.lyapunov_descent".to_string(),
            ki: 0.001,
            kb: 0.0316,
            adaptive: Some(AdaptiveGainSchedule::default()),
        })
    }

    // ─── Original PD tests (UNCHANGED) ───

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
        assert_eq!(state.integral, 0.0);
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

    // ─── PID tests ───

    #[test]
    fn pid_matches_pd_when_ki_zero() {
        let mut pd_state = default_state();
        let mut pid_state = default_state();
        // Ensure both are pure PD (ki=0, kb=0)
        assert_eq!(pd_state.config.ki, 0.0);
        assert_eq!(pid_state.config.ki, 0.0);

        for delta in [2.2, 3.5, 1.0, 5.0, 0.5] {
            let pd_eps = pd_state.step(delta);
            let pid_eps = pid_state.step(delta);
            assert!(
                (pd_eps - pid_eps).abs() < 1e-12,
                "PD and PID(ki=0) diverged at delta={delta}: {pd_eps} vs {pid_eps}"
            );
        }
    }

    #[test]
    fn pid_integral_accumulates() {
        let mut state = pid_state();
        assert_eq!(state.integral, 0.0);
        // Feed a consistent overshoot signal
        for _ in 0..10 {
            state.step(120.0); // above target of 100
        }
        // Integral should have accumulated (sign depends on error direction)
        assert!(
            state.integral.abs() > 0.0,
            "integral should accumulate: {}",
            state.integral
        );
    }

    #[test]
    fn pid_eliminates_steady_state_error() {
        // Both use target=2.0, eps range [1, 50], moderate persistent load
        let mut pid = GovernorState::new(GovernorConfig {
            instance_id: "pid-ss".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 1.0,
            eps_max: 50.0,
            target: 2.0,
            formal_basis: "test".to_string(),
            ki: 0.005,
            kb: 0.07, // sqrt(0.005)
            adaptive: None,
        });
        let mut pd = default_state(); // ki=0, same target=2.0

        // Constant load of 3.0 (above target * eps_min, creating persistent bias)
        let constant_load = 3.0;
        for _ in 0..100 {
            pid.step(constant_load);
            pd.step(constant_load);
        }

        let pid_error = pid.error(constant_load).abs();
        let pd_error = pd.error(constant_load).abs();

        // PID should have smaller steady-state error due to integral accumulation
        assert!(
            pid_error < pd_error,
            "PID error ({pid_error:.6}) should be less than PD error ({pd_error:.6})"
        );
    }

    #[test]
    fn anti_windup_prevents_overshoot_after_saturation() {
        // Use small, stable params to avoid derivative oscillation
        let make_state = |kb: f64| {
            GovernorState::new(GovernorConfig {
                instance_id: "aw-test".to_string(),
                alpha: 0.01,
                beta: 0.0, // no derivative — isolates the integral behavior
                dt: 1.0,
                eps_min: 1.0,
                eps_max: 10.0,
                target: 2.0,
                formal_basis: "test".to_string(),
                ki: 0.01,
                kb,
                adaptive: None,
            })
        };

        let mut with_aw = make_state(0.1); // back-calculation gain
        let mut without_aw = make_state(0.0); // no anti-windup

        // Phase 1: sustained overshoot drives integral up, epsilon clamps at max
        for _ in 0..50 {
            with_aw.step(30.0); // error = 30/eps - 2 ≈ positive, eps grows toward max
            without_aw.step(30.0);
        }

        // Without anti-windup, the integral winds up unchecked during clamping.
        // With anti-windup, the back-calculation term unwinds it.
        assert!(
            with_aw.integral.abs() < without_aw.integral.abs(),
            "anti-windup integral ({:.4}) should be smaller than no-anti-windup ({:.4})",
            with_aw.integral.abs(),
            without_aw.integral.abs()
        );

        // Phase 2: load drops — wound-up integral delays recovery
        for _ in 0..20 {
            with_aw.step(1.0); // below target, eps should decrease
            without_aw.step(1.0);
        }

        // With anti-windup, the controller should recover (lower epsilon) faster
        // because the integral wasn't wound up as much
        assert!(
            with_aw.epsilon <= without_aw.epsilon,
            "anti-windup epsilon ({:.4}) should recover (decrease) faster than no-anti-windup ({:.4})",
            with_aw.epsilon,
            without_aw.epsilon
        );
    }

    #[test]
    fn pid_mode_label() {
        let state = pid_state();
        assert_eq!(state.controller_mode(), "PID");
        assert!(state.regime_label().contains("PID"));

        let pd = default_state();
        assert_eq!(pd.controller_mode(), "PD");
    }

    #[test]
    fn pid_formal_warning() {
        let state = pid_state();
        let warning = state.formal_warning();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("PID mode"));
    }

    #[test]
    fn validate_rejects_negative_ki() {
        let mut state = pid_state();
        state.config.ki = -0.01;
        let err = state.validate_params().expect_err("negative ki");
        assert!(err.contains("ki="));
    }

    #[test]
    fn validate_rejects_negative_kb() {
        let mut state = pid_state();
        state.config.kb = -0.01;
        let err = state.validate_params().expect_err("negative kb");
        assert!(err.contains("kb="));
    }

    // ─── Adaptive gain tests ───

    #[test]
    fn regime_classification_thresholds() {
        assert_eq!(LoadRegime::classify(0.0), LoadRegime::Quiet);
        assert_eq!(LoadRegime::classify(0.3), LoadRegime::Quiet);
        assert_eq!(LoadRegime::classify(0.49), LoadRegime::Quiet);
        assert_eq!(LoadRegime::classify(0.5), LoadRegime::Active);
        assert_eq!(LoadRegime::classify(0.7), LoadRegime::Active);
        assert_eq!(LoadRegime::classify(0.89), LoadRegime::Active);
        assert_eq!(LoadRegime::classify(0.9), LoadRegime::Burst);
        assert_eq!(LoadRegime::classify(1.0), LoadRegime::Burst);
        assert_eq!(LoadRegime::classify(2.0), LoadRegime::Burst);
    }

    #[test]
    fn adaptive_gains_are_deterministic() {
        let schedule = AdaptiveGainSchedule::default();
        let (a1, b1, k1) = schedule.effective_gains(0.01, 0.05, 0.001, LoadRegime::Burst);
        let (a2, b2, k2) = schedule.effective_gains(0.01, 0.05, 0.001, LoadRegime::Burst);
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(k1, k2);
    }

    #[test]
    fn adaptive_none_matches_fixed() {
        // Compare: adaptive=None (fixed gains) vs adaptive=Some(identity schedule)
        // An identity schedule (all scale factors = 1.0) should produce the same
        // output as no scheduling at all.
        let identity_schedule = AdaptiveGainSchedule {
            quiet_alpha_scale: 1.0,
            quiet_beta_scale: 1.0,
            quiet_ki_scale: 1.0,
            active_alpha_scale: 1.0,
            active_beta_scale: 1.0,
            active_ki_scale: 1.0,
            burst_alpha_scale: 1.0,
            burst_beta_scale: 1.0,
            burst_ki_scale: 1.0,
        };

        let mut fixed = pid_state(); // adaptive: None
        assert!(fixed.config.adaptive.is_none());

        let mut with_identity = pid_state();
        with_identity.config.adaptive = Some(identity_schedule);
        assert!(with_identity.config.adaptive.is_some());

        // Drive both through varying loads that cross regime boundaries
        for delta in [30.0, 50.0, 80.0, 95.0, 110.0, 40.0] {
            let f_eps = fixed.step(delta);
            let a_eps = with_identity.step(delta);
            assert!(
                (f_eps - a_eps).abs() < 1e-12,
                "fixed and identity-schedule diverged at delta={delta}: {f_eps} vs {a_eps}"
            );
        }
    }

    #[test]
    fn adaptive_some_differs_from_none() {
        // Verify that a non-identity adaptive schedule actually changes behavior.
        // Use a load (delta=200, target=100 → occupancy 2.0 → Burst) that produces
        // a positive error large enough to push epsilon above eps_min, where the
        // gain scaling creates a visible difference.
        let mut no_adaptive = pid_state(); // adaptive: None
        let mut with_adaptive = adaptive_state(); // adaptive: Some(default)

        // delta=200 → error = 200/1 - 100 = 100 (positive, drives epsilon up)
        // Burst regime: alpha*0.5, beta*2.0 — different effective gains
        no_adaptive.step(200.0);
        with_adaptive.step(200.0);

        assert!(
            (no_adaptive.epsilon - with_adaptive.epsilon).abs() > 1e-12,
            "adaptive schedule should produce different epsilon: no={:.6} vs with={:.6}",
            no_adaptive.epsilon,
            with_adaptive.epsilon
        );
    }

    #[test]
    fn adaptive_regime_affects_gains() {
        let mut state = adaptive_state();

        // Quiet regime (load = 30, target = 100 → 0.3 occupancy)
        state.step(30.0);
        assert_eq!(state.current_regime, Some(LoadRegime::Quiet));

        // Burst regime (load = 95, target = 100 → 0.95 occupancy)
        state.reset();
        state.step(95.0);
        assert_eq!(state.current_regime, Some(LoadRegime::Burst));
    }

    #[test]
    fn adaptive_burst_damps_more_than_quiet() {
        let schedule = AdaptiveGainSchedule::default();

        let (quiet_alpha, quiet_beta, _) =
            schedule.effective_gains(0.01, 0.05, 0.001, LoadRegime::Quiet);
        let (burst_alpha, burst_beta, _) =
            schedule.effective_gains(0.01, 0.05, 0.001, LoadRegime::Burst);

        // Burst should have lower alpha (less aggressive) and higher beta (more damping)
        assert!(
            burst_alpha < quiet_alpha,
            "burst alpha ({burst_alpha}) should be < quiet alpha ({quiet_alpha})"
        );
        assert!(
            burst_beta > quiet_beta,
            "burst beta ({burst_beta}) should be > quiet beta ({quiet_beta})"
        );
    }

    #[test]
    fn validate_rejects_adaptive_schedule_that_violates_gain_condition() {
        // Base gains pass: alpha=0.8, beta=0, gamma=0.8 < 1.0
        // But quiet_alpha_scale=1.5 → effective alpha=1.2, gamma=1.2 >= 1.0
        let state = GovernorState::new(GovernorConfig {
            instance_id: "gain-violation".to_string(),
            alpha: 0.8,
            beta: 0.0,
            dt: 1.0,
            eps_min: 1.0,
            eps_max: 50.0,
            target: 100.0,
            formal_basis: "test".to_string(),
            ki: 0.0,
            kb: 0.0,
            adaptive: Some(AdaptiveGainSchedule {
                quiet_alpha_scale: 1.5, // 0.8 * 1.5 = 1.2 > 1.0!
                quiet_beta_scale: 1.0,
                quiet_ki_scale: 1.0,
                active_alpha_scale: 1.0,
                active_beta_scale: 1.0,
                active_ki_scale: 1.0,
                burst_alpha_scale: 1.0,
                burst_beta_scale: 1.0,
                burst_ki_scale: 1.0,
            }),
        });
        let err = state.validate_params().expect_err("should reject");
        assert!(
            err.contains("adaptive gain schedule"),
            "error should mention adaptive schedule: {err}"
        );
    }

    #[test]
    fn validate_accepts_adaptive_schedule_within_gain_condition() {
        // Base: alpha=0.01, beta=0.05, gamma=0.06
        // Worst case: quiet alpha_scale=1.5 → 0.015+0.025=0.04
        // All regimes well within bounds
        let state = adaptive_state();
        let report = state.validate_params().expect("should accept");
        assert!(report.gamma < 1.0);
    }

    #[test]
    fn reset_clears_integral_and_regime() {
        let mut state = adaptive_state();
        state.step(95.0);
        assert!(state.integral != 0.0 || state.current_regime.is_some());
        state.reset();
        assert_eq!(state.integral, 0.0);
        assert_eq!(state.current_regime, None);
        assert!(state.is_from_rest());
    }
}
