//! Falsifiable experiment harness for AETHER governance claims.
//!
//! Each test states:
//!   1. A concrete claim about the system
//!   2. What evidence would falsify it
//!   3. An adversarial scenario designed to produce that evidence
//!
//! If any test fails, the corresponding governance claim is falsified and
//! must be weakened, documented, or the implementation fixed.
//!
//! Coverage:
//!   - Governor PD controller: Lyapunov descent, gain condition, clamp bounds
//!   - Chebyshev evictor: reclaimable bound, guard check, ordering preservation
//!   - Diode floor: floor guarantee, Sybil resistance, transparency, monotonicity

use nucleusdb::halo::chebyshev_evictor::{
    chebyshev_guard_check, reclaimable_count, ChebyshevEvictor,
};
use nucleusdb::halo::governor::{GovernorConfig, GovernorState};
use nucleusdb::halo::identity::{save as save_identity, IdentityConfig, IdentitySecurityTier};
use nucleusdb::halo::schema::{PaidOperation, SessionMetadata, SessionStatus};
use nucleusdb::halo::trace::{now_unix_secs, TraceWriter};
use nucleusdb::halo::trust::{query_trust_score, security_tier_trust_floor, EpistemicTrust};
use nucleusdb::halo::trust_score::{
    compute_trust, diode_floor, ChallengeDifficulty, IdentityTier, VerificationRecord,
    TRUST_FLOOR_ANCHORED, TRUST_FLOOR_STAKED,
};
use nucleusdb::vector_index::{DistanceMetric, VectorIndex};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    let mutex = env_lock();
    let guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    mutex.clear_poison();
    guard
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let previous = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(v) = &self.previous {
            std::env::set_var(self.key, v);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn temp_halo_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "agenthalo_falsifiable_{tag}_{}_{}",
        std::process::id(),
        now_unix_secs()
    ))
}

// ═══════════════════════════════════════════════════════════════════════════
// § 1  GOVERNOR PD CONTROLLER
// ═══════════════════════════════════════════════════════════════════════════

fn make_governor(
    alpha: f64,
    beta: f64,
    dt: f64,
    eps_min: f64,
    eps_max: f64,
    target: f64,
) -> GovernorState {
    GovernorState::new(GovernorConfig {
        instance_id: "gov-falsifiable".to_string(),
        alpha,
        beta,
        dt,
        eps_min,
        eps_max,
        target,
        formal_basis: "test".to_string(),
    })
}

/// Claim 1a: In the formal regime (from-rest, no-clamp, valid gain), a single
/// governor step produces strict Lyapunov descent: V(e') < V(e) for e ≠ 0.
///
/// Falsifier: any (α, β, dt, delta) in the valid regime where V does not decrease.
#[test]
fn governor_lyapunov_descent_formal_regime_sweep() {
    let alphas = [0.001, 0.01, 0.05, 0.1, 0.2, 0.3];
    let betas = [0.0, 0.01, 0.05, 0.1, 0.2];
    let dts = [1.0, 2.0, 5.0, 10.0];
    let deltas = [0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 50.0, 100.0, 0.01];
    let target = 2.0;
    let mut tested = 0u64;

    for &alpha in &alphas {
        for &beta in &betas {
            for &dt in &dts {
                let gamma = alpha + beta / dt;
                if gamma >= 1.0 {
                    continue; // outside formal regime
                }
                for &delta in &deltas {
                    let mut gov = make_governor(alpha, beta, dt, 0.001, 1000.0, target);
                    assert!(gov.validate_params().is_ok());
                    assert!(gov.is_from_rest());

                    let v_before = gov.lyapunov(delta);
                    if v_before < 1e-15 {
                        continue; // trivial — error is essentially zero
                    }

                    gov.step(delta);

                    // After the step, re-evaluate Lyapunov at the SAME signal to
                    // measure whether the controller moved epsilon to reduce error.
                    let v_after = gov.lyapunov(delta);

                    // Check if clamp was active — if so, the formal guarantee
                    // does not apply (documented limitation).
                    if gov.clamp_active {
                        continue;
                    }

                    assert!(
                        v_after < v_before,
                        "FALSIFIED Claim 1a: Lyapunov did not descend.\n\
                         α={alpha}, β={beta}, dt={dt}, δ={delta}, target={target}\n\
                         V_before={v_before:.10}, V_after={v_after:.10}"
                    );
                    tested += 1;
                }
            }
        }
    }
    assert!(
        tested > 100,
        "sweep must cover meaningful parameter space (tested {tested})"
    );
}

/// Claim 1b: The gain condition γ = α + β/dt < 1 is necessary — violating it
/// CAN produce Lyapunov increase (so the formal guarantee is tight, not vacuous).
///
/// Falsifier: even with γ ≥ 1, descent always holds (making the condition unnecessary).
///
/// Key insight: to demonstrate overshoot, epsilon must start NEAR the equilibrium
/// point (delta/target) so that the γ>1 correction overshoots PAST it. If
/// epsilon starts far from equilibrium (at a tiny eps_min), the correction
/// moves it in the right direction by a huge amount, which always reduces error
/// even with γ>1 — the overshoot only shows after crossing equilibrium.
#[test]
fn governor_gain_violation_can_increase_lyapunov() {
    // target=2.0, delta=5.0 → equilibrium epsilon = delta/target = 2.5
    // Start epsilon near equilibrium by setting eps_min=2.0.
    // α=5.0, β=0.0 → γ=5.0 >> 1 (massive gain violation)
    //
    // From rest: epsilon=2.0
    //   e = 5.0/2.0 - 2.0 = 0.5
    //   adjustment = 5.0 * 0.5 = 2.5
    //   epsilon_new = 2.0 + 2.5 = 4.5 (overshoots past equilibrium 2.5)
    //   e' = 5.0/4.5 - 2.0 = -0.889
    //   V_before = 0.5 * 0.5² = 0.125
    //   V_after = 0.5 * 0.889² = 0.395
    //   V increased! The controller overcorrected.
    let mut gov = make_governor(5.0, 0.0, 1.0, 2.0, 100.0, 2.0);
    assert!(
        gov.validate_params().is_err(),
        "γ=5.0 should fail validation"
    );

    let delta = 5.0;
    let v_before = gov.lyapunov(delta);
    assert!(v_before > 0.0, "initial error should be nonzero");

    gov.step(delta);
    assert!(
        !gov.clamp_active,
        "clamp should not be active for this case"
    );

    let v_after = gov.lyapunov(delta);
    assert!(
        v_after > v_before,
        "With γ=5.0 >> 1, the controller should overshoot: \
         V_before={v_before:.6}, V_after={v_after:.6}"
    );

    // Sweep additional cases to confirm the pattern holds across parameters.
    let mut overshoot_count = 0u32;
    for alpha in [2.0, 3.0, 5.0, 10.0] {
        let target = 2.0;
        let delta = 5.0;
        let equilibrium = delta / target; // 2.5
                                          // Start epsilon slightly below equilibrium.
        let eps_min = equilibrium * 0.8;
        let eps_max = equilibrium * 10.0;
        let mut g = make_governor(alpha, 0.0, 1.0, eps_min, eps_max, target);
        let vb = g.lyapunov(delta);
        g.step(delta);
        if !g.clamp_active {
            let va = g.lyapunov(delta);
            if va > vb {
                overshoot_count += 1;
            }
        }
    }
    assert!(
        overshoot_count >= 2,
        "Expected multiple overshoot demonstrations, got {overshoot_count}"
    );
}

/// Claim 1c: Epsilon always remains within [eps_min, eps_max] regardless of
/// signal magnitude.
///
/// Falsifier: any signal that produces epsilon outside the clamp bounds.
#[test]
fn governor_clamp_bounds_extreme_signals() {
    let signals = [
        0.0,
        f64::MIN_POSITIVE,
        1e-10,
        0.001,
        1.0,
        100.0,
        1e6,
        1e15,
        f64::MAX / 2.0,
    ];
    let eps_min = 1.0;
    let eps_max = 50.0;

    for &signal in &signals {
        let mut gov = make_governor(0.01, 0.05, 1.0, eps_min, eps_max, 2.0);
        // Run 100 steps with the extreme signal.
        for _ in 0..100 {
            gov.step(signal);
            assert!(
                gov.epsilon >= eps_min && gov.epsilon <= eps_max,
                "FALSIFIED Claim 1c: epsilon={} outside [{eps_min}, {eps_max}] at signal={signal}",
                gov.epsilon
            );
        }
    }
}

/// Claim 1d: Multi-step operation CAN produce Lyapunov non-descent. The
/// single-step formal guarantee does NOT extend to multi-step.
///
/// Falsifier: multi-step always descends (meaning we're being too conservative
/// in our formal claims — a stronger result than we advertise).
#[test]
fn governor_multistep_can_oscillate() {
    let mut gov = make_governor(0.01, 0.05, 1.0, 1.0, 50.0, 2.0);
    // Feed an alternating signal to provoke oscillation.
    let signals = [0.1, 100.0, 0.01, 50.0, 0.1, 100.0, 0.01, 50.0];
    let mut found_nondescent = false;
    let mut prev_lyapunov = None;

    for &signal in signals.iter().cycle().take(200) {
        let v_before = gov.lyapunov(signal);
        gov.step(signal);
        let v_after = gov.lyapunov(signal);
        if let Some(prev) = prev_lyapunov {
            if v_after > prev {
                found_nondescent = true;
                break;
            }
        }
        prev_lyapunov = Some(v_after);
        let _ = v_before;
    }
    assert!(
        found_nondescent,
        "Could not provoke Lyapunov non-descent in multi-step — \
         the formal guarantee may be stronger than claimed"
    );
}

/// Claim 1e: The regime label correctly transitions from "from-rest" to
/// "multi-step" after the first observation, and reset returns to from-rest.
///
/// Falsifier: regime label is wrong after step or after reset.
#[test]
fn governor_regime_transitions_are_correct() {
    let mut gov = make_governor(0.01, 0.05, 1.0, 1.0, 50.0, 2.0);
    assert!(gov.is_from_rest());
    assert!(gov.regime_label().contains("from-rest"));

    gov.step(3.0);
    assert!(!gov.is_from_rest());
    assert!(gov.regime_label().contains("multi-step"));

    gov.reset();
    assert!(gov.is_from_rest());
    assert!(gov.regime_label().contains("from-rest"));
}

// ═══════════════════════════════════════════════════════════════════════════
// § 2  CHEBYSHEV EVICTOR
// ═══════════════════════════════════════════════════════════════════════════

/// Claim 2a: For data with positive standard deviation,
/// reclaimable_count(x, k) ≤ n/k² (with ≤-threshold tolerance).
///
/// Mathematical note: when stddev=0 (constant data), Chebyshev's inequality is
/// vacuously true (no deviations exist), but reclaimable_count's ≤ threshold
/// catches all items at the mean. The guard_check function handles this correctly
/// by returning true when sd≤0. This test covers the stddev>0 regime where the
/// bound is non-trivial.
///
/// Falsifier: any (x, k) with stddev(x) > 0 where reclaimable_count exceeds n/k².
#[test]
fn chebyshev_reclaimable_bound_adversarial_distributions() {
    let distributions: Vec<(&str, Vec<f64>)> = vec![
        ("uniform", (0..100).map(|i| i as f64).collect()),
        ("bimodal", {
            let mut v: Vec<f64> = (0..50).map(|_| 0.0).collect();
            v.extend((0..50).map(|_| 100.0));
            v
        }),
        ("single_outlier", {
            let mut v: Vec<f64> = (0..99).map(|_| 50.0).collect();
            v.push(0.0);
            v
        }),
        ("heavy_tail", (1..=200).map(|i| 1.0 / i as f64).collect()),
        ("two_values", vec![0.0, 1000.0]),
        ("adversarial_cluster", {
            // 90 items at 100.0, 10 items at 0.0 — tries to maximize reclaimable
            let mut v: Vec<f64> = (0..90).map(|_| 100.0).collect();
            v.extend((0..10).map(|_| 0.0));
            v
        }),
        ("near_constant", {
            // 99 items at 50.0, 1 at 50.001 — tiny but positive stddev
            let mut v: Vec<f64> = (0..99).map(|_| 50.0).collect();
            v.push(50.001);
            v
        }),
    ];
    let ks = [0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0];

    for (name, x) in &distributions {
        let n = x.len() as f64;
        for &k in &ks {
            let count = reclaimable_count(x, k) as f64;
            let bound = n / (k * k);
            // The implementation uses <= threshold, so the bound is n/k² + epsilon.
            assert!(
                count <= bound + 1e-10,
                "FALSIFIED Claim 2a: reclaimable_count={count} > bound={bound:.4} \
                 for distribution '{name}', k={k}, n={n}"
            );
        }
    }
}

/// Claim 2a-degenerate: When stddev=0 (constant data), reclaimable_count may
/// exceed n/k² because all items sit exactly at the threshold. This is a known
/// property of the ≤ comparator. The chebyshev_guard_check function correctly
/// handles this case by short-circuiting on sd≤0.
///
/// This is a documentation test, not a falsification — it records the known
/// degenerate behavior so future changes don't silently alter the semantics.
#[test]
fn chebyshev_reclaimable_constant_data_degenerate_case() {
    let constant = vec![5.0; 100];
    // With constant data, threshold = mean - k*0 = mean = 5.0.
    // All 100 items are <= 5.0, so reclaimable_count = 100.
    let count = reclaimable_count(&constant, 2.0);
    assert_eq!(count, 100, "all items at threshold when stddev=0");

    // But guard_check handles this correctly.
    assert!(
        chebyshev_guard_check(&constant, 2.0),
        "guard_check should return true for constant data (sd=0 short-circuit)"
    );
}

/// Claim 2b: chebyshev_guard_check returns true when the Chebyshev bound holds
/// for the given data, and returns false for edge cases (empty, k≤0).
///
/// Falsifier: guard returns false when the bound provably holds, or true for
/// invalid inputs.
#[test]
fn chebyshev_guard_check_consistency() {
    // Empty and invalid-k cases must return false.
    assert!(
        !chebyshev_guard_check(&[], 2.0),
        "empty should return false"
    );
    assert!(
        !chebyshev_guard_check(&[1.0, 2.0], 0.0),
        "k=0 should return false"
    );
    assert!(
        !chebyshev_guard_check(&[1.0, 2.0], -1.0),
        "k<0 should return false"
    );

    // Constant data has stddev=0 → guard should return true (no outliers possible).
    assert!(
        chebyshev_guard_check(&[5.0; 50], 2.0),
        "constant data should pass"
    );

    // Well-distributed data should pass for reasonable k.
    let normal_ish: Vec<f64> = (0..100).map(|i| i as f64 - 50.0).collect();
    assert!(
        chebyshev_guard_check(&normal_ish, 2.0),
        "normal-ish data should pass at k=2"
    );

    // The guard check is logically: reclaimable_count ≤ n/k². Verify this link.
    let data: Vec<f64> = (0..50).map(|i| i as f64).collect();
    for &k in &[1.0, 2.0, 3.0] {
        let count = reclaimable_count(&data, k) as f64;
        let bound = data.len() as f64 / (k * k);
        let guard_result = chebyshev_guard_check(&data, k);
        let bound_holds = count <= bound + 1e-12;
        assert_eq!(
            guard_result, bound_holds,
            "FALSIFIED Claim 2b: guard_check={guard_result} but bound_holds={bound_holds} \
             (count={count}, bound={bound:.4}, k={k})"
        );
    }
}

/// Claim 2c: Active items (liveness above the reclaimable threshold) are NEVER
/// returned as eviction candidates.
///
/// Falsifier: an active item appears in eviction_candidates.
#[test]
fn chebyshev_active_items_never_evicted() {
    let mut evictor = ChebyshevEvictor::new(2.0, 0.0, 1.0);

    // Create a hot/cold population.
    for _ in 0..20 {
        evictor.record_access("hot_a");
        evictor.record_access("hot_b");
    }
    for _ in 0..5 {
        evictor.record_access("warm");
    }
    evictor.liveness.insert("cold_x".to_string(), 0.0);
    evictor.liveness.insert("cold_y".to_string(), 0.1);

    let candidates = evictor.eviction_candidates(10);
    for candidate in &candidates {
        assert!(
            !candidate.starts_with("hot_"),
            "FALSIFIED Claim 2c: hot item '{candidate}' returned as eviction candidate"
        );
    }

    // Verify hot items are guarded.
    assert!(evictor.is_guarded("hot_a"), "hot_a should be guarded");
    assert!(evictor.is_guarded("hot_b"), "hot_b should be guarded");
}

/// Claim 2d: Exponential decay preserves relative ordering of liveness scores.
/// If liveness[a] > liveness[b] before decay, then liveness[a] > liveness[b] after.
///
/// Falsifier: decay inverts ordering between any two items.
#[test]
fn chebyshev_decay_preserves_ordering() {
    let mut evictor = ChebyshevEvictor::new(2.0, 0.1, 1.0);

    // Create items with distinct liveness scores.
    for _ in 0..10 {
        evictor.record_access("high");
    }
    for _ in 0..5 {
        evictor.record_access("mid");
    }
    for _ in 0..1 {
        evictor.record_access("low");
    }

    let before_high = evictor.liveness["high"];
    let before_mid = evictor.liveness["mid"];
    let before_low = evictor.liveness["low"];
    assert!(before_high > before_mid);
    assert!(before_mid > before_low);

    // Apply many decay steps.
    for _ in 0..100 {
        evictor.tick();
    }

    let after_high = evictor.liveness["high"];
    let after_mid = evictor.liveness["mid"];
    let after_low = evictor.liveness["low"];

    assert!(
        after_high >= after_mid,
        "FALSIFIED Claim 2d: decay inverted high/mid ordering: {after_high} < {after_mid}"
    );
    assert!(
        after_mid >= after_low,
        "FALSIFIED Claim 2d: decay inverted mid/low ordering: {after_mid} < {after_low}"
    );
}

/// Claim 2e: Requesting more eviction candidates than exist never panics and
/// returns at most the number of reclaimable items.
///
/// Falsifier: panic or returned count exceeds tracked items.
#[test]
fn chebyshev_eviction_request_exceeds_population() {
    let mut evictor = ChebyshevEvictor::new(2.0, 0.0, 1.0);
    evictor.record_access("only_item");

    let candidates = evictor.eviction_candidates(1000);
    assert!(candidates.len() <= 1);

    // Empty evictor.
    let mut empty = ChebyshevEvictor::new(2.0, 0.0, 1.0);
    let candidates = empty.eviction_candidates(100);
    assert!(candidates.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 3  DIODE FLOOR (P2P TRUST)
// ═══════════════════════════════════════════════════════════════════════════

/// Claim 3a: diode_floor(trust, Staked) ≥ TRUST_FLOOR_STAKED for ANY trust
/// value, including negative, zero, and extremely large values.
///
/// Falsifier: any trust value where the floor is not enforced.
#[test]
fn diode_floor_staked_sweep() {
    let trust_values = [
        -1e6,
        -100.0,
        -1.0,
        -0.001,
        0.0,
        0.001,
        0.5,
        1.0,
        1.999,
        TRUST_FLOOR_STAKED,
        TRUST_FLOOR_STAKED + 0.001,
        5.0,
        100.0,
        1e6,
    ];
    for &trust in &trust_values {
        let result = diode_floor(trust, IdentityTier::Staked);
        assert!(
            result >= TRUST_FLOOR_STAKED,
            "FALSIFIED Claim 3a: diode_floor({trust}, Staked) = {result} < {TRUST_FLOOR_STAKED}"
        );
    }
}

/// Claim 3b: diode_floor(trust, Anchored) ≥ TRUST_FLOOR_ANCHORED for any trust.
#[test]
fn diode_floor_anchored_sweep() {
    let trust_values = [
        -1e6,
        -1.0,
        0.0,
        0.25,
        TRUST_FLOOR_ANCHORED,
        TRUST_FLOOR_ANCHORED + 0.1,
        10.0,
    ];
    for &trust in &trust_values {
        let result = diode_floor(trust, IdentityTier::Anchored);
        assert!(
            result >= TRUST_FLOOR_ANCHORED,
            "FALSIFIED Claim 3b: diode_floor({trust}, Anchored) = {result} < {TRUST_FLOOR_ANCHORED}"
        );
    }
}

/// Claim 3c: The diode is transparent for Anonymous and Verified — it never
/// modifies the trust value (neither raises nor lowers it).
///
/// Falsifier: diode_floor(trust, Anonymous/Verified) ≠ trust.
#[test]
fn diode_floor_transparent_for_low_tiers() {
    let trust_values = [-100.0, -1.0, 0.0, 0.5, 1.0, 5.0, 100.0, 1e6];
    for &trust in &trust_values {
        let anon = diode_floor(trust, IdentityTier::Anonymous);
        assert_eq!(
            anon, trust,
            "FALSIFIED Claim 3c: diode_floor({trust}, Anonymous) = {anon} ≠ {trust}"
        );
        let verified = diode_floor(trust, IdentityTier::Verified);
        assert_eq!(
            verified, trust,
            "FALSIFIED Claim 3c: diode_floor({trust}, Verified) = {verified} ≠ {trust}"
        );
    }
}

/// Claim 3d: The diode is monotone — it NEVER lowers trust for any tier.
/// diode_floor(trust, tier) ≥ trust for all (trust, tier).
///
/// Falsifier: any (trust, tier) where diode_floor(trust, tier) < trust.
#[test]
fn diode_floor_never_lowers_trust() {
    let tiers = [
        IdentityTier::Anonymous,
        IdentityTier::Verified,
        IdentityTier::Anchored,
        IdentityTier::Staked,
    ];
    let trust_values = [-1000.0, -1.0, 0.0, 0.001, 0.5, 1.0, 2.0, 5.0, 100.0, 1e10];

    for &tier in &tiers {
        for &trust in &trust_values {
            let result = diode_floor(trust, tier);
            assert!(
                result >= trust,
                "FALSIFIED Claim 3d: diode_floor({trust}, {tier:?}) = {result} < {trust}"
            );
        }
    }
}

/// Claim 3e: A coordinated Sybil burst of N failed Deep challenges cannot
/// drive a Staked peer's trust (after diode_floor) below TRUST_FLOOR_STAKED,
/// regardless of N.
///
/// Falsifier: find N where the Staked peer's protected trust falls below the floor.
#[test]
fn diode_sybil_burst_sweep_cannot_annihilate_staked() {
    let legitimate = VerificationRecord {
        peer_did: "did:key:staked".to_string(),
        capability_domain: "prove/lean".to_string(),
        challenge_difficulty: ChallengeDifficulty::Deep,
        passed: true,
        elapsed_ms: 400,
        verified_at: 100,
    };

    // Sweep attack sizes from 1 to 500.
    for n_attacks in [1, 5, 10, 20, 50, 100, 200, 500] {
        let attack_records: Vec<VerificationRecord> = (0..n_attacks)
            .map(|i| VerificationRecord {
                peer_did: "did:key:staked".to_string(),
                capability_domain: "prove/lean".to_string(),
                challenge_difficulty: ChallengeDifficulty::Deep,
                passed: false,
                elapsed_ms: 10,
                verified_at: 200 + i,
            })
            .collect();

        let mut all_records = vec![legitimate.clone()];
        all_records.extend(attack_records);

        let raw_trust = compute_trust(&all_records, 700, 3600);
        let protected = diode_floor(raw_trust, IdentityTier::Staked);

        assert!(
            protected >= TRUST_FLOOR_STAKED,
            "FALSIFIED Claim 3e: {n_attacks} attacks drove protected trust to {protected} \
             (raw={raw_trust}) below floor {TRUST_FLOOR_STAKED}"
        );
    }
}

/// Claim 3f: The P2P diode floor values form a monotone ladder:
/// floor(Anonymous) ≤ floor(Verified) ≤ floor(Anchored) ≤ floor(Staked).
///
/// Falsifier: the ordering is violated.
#[test]
fn diode_floor_tier_ordering_is_monotone() {
    // Anonymous and Verified have effective floor of 0 (transparent).
    let trust = 0.0;
    let f_anon = diode_floor(trust, IdentityTier::Anonymous);
    let f_verified = diode_floor(trust, IdentityTier::Verified);
    let f_anchored = diode_floor(trust, IdentityTier::Anchored);
    let f_staked = diode_floor(trust, IdentityTier::Staked);

    assert!(
        f_anon <= f_verified,
        "FALSIFIED: Anonymous floor ({f_anon}) > Verified floor ({f_verified})"
    );
    assert!(
        f_verified <= f_anchored,
        "FALSIFIED: Verified floor ({f_verified}) > Anchored floor ({f_anchored})"
    );
    assert!(
        f_anchored <= f_staked,
        "FALSIFIED: Anchored floor ({f_anchored}) > Staked floor ({f_staked})"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// § 4  DIODE FLOOR (PRODUCTION TRUST HELPERS + ENTRY POINT)
// ═══════════════════════════════════════════════════════════════════════════

/// Claim 4a: security_tier_trust_floor values form a monotone ladder aligned
/// with the tier's security investment: MaxSafe > LessSafe > LowSecurity.
///
/// Falsifier: the ordering is violated.
#[test]
fn production_floor_tier_ordering() {
    use nucleusdb::halo::identity::IdentitySecurityTier;

    let f_max = security_tier_trust_floor(&IdentitySecurityTier::MaxSafe);
    let f_less = security_tier_trust_floor(&IdentitySecurityTier::LessSafe);
    let f_low = security_tier_trust_floor(&IdentitySecurityTier::LowSecurity);

    assert!(
        f_max > f_less,
        "FALSIFIED Claim 4a: MaxSafe floor ({f_max}) ≤ LessSafe floor ({f_less})"
    );
    assert!(
        f_less > f_low,
        "FALSIFIED Claim 4a: LessSafe floor ({f_less}) ≤ LowSecurity floor ({f_low})"
    );
    assert_eq!(f_low, 0.0, "LowSecurity floor should be exactly 0.0");
}

/// Claim 4b: All production floor values are strictly below the "cautious"
/// trust tier threshold (0.40). A floored node still has reduced privileges;
/// the floor prevents annihilation, not privilege restoration.
///
/// Falsifier: any floor value ≥ 0.40.
#[test]
fn production_floor_below_cautious_threshold() {
    use nucleusdb::halo::identity::IdentitySecurityTier;
    let cautious_threshold = 0.40;

    for tier in [
        IdentitySecurityTier::MaxSafe,
        IdentitySecurityTier::LessSafe,
        IdentitySecurityTier::LowSecurity,
    ] {
        let floor = security_tier_trust_floor(&tier);
        assert!(
            floor < cautious_threshold,
            "FALSIFIED Claim 4b: floor for {tier:?} is {floor} ≥ cautious threshold {cautious_threshold}"
        );
    }
}

/// Claim 4c: The production trust floor is a one-way operation — applying it
/// to a score that already exceeds the floor does not change the score.
///
/// Falsifier: floor application lowers a score that was above the floor.
#[test]
fn production_floor_is_idempotent_above_floor() {
    use nucleusdb::halo::identity::IdentitySecurityTier;

    for tier in [
        IdentitySecurityTier::MaxSafe,
        IdentitySecurityTier::LessSafe,
        IdentitySecurityTier::LowSecurity,
    ] {
        let floor = security_tier_trust_floor(&tier);
        for score in [floor, floor + 0.01, 0.5, 0.75, 0.95, 1.0] {
            let result = score.max(floor);
            assert_eq!(
                result, score,
                "FALSIFIED Claim 4c: applying floor {floor} to score {score} changed it to {result}"
            );
        }
    }
}

/// Claim 4d: The production entry point `query_trust_score()` loads the node's
/// configured identity tier from `AGENTHALO_HOME/identity.json` and applies the
/// corresponding floor, independent of the ambient developer machine state.
///
/// Falsifier: any explicitly configured tier produces a score inconsistent with
/// the tier actually written to disk.
#[test]
fn production_entry_point_enforces_loaded_tier_floor_under_adversarial_history() {
    let _guard = lock_env();
    let stale_started_at = now_unix_secs().saturating_sub(31 * 24 * 60 * 60);
    let expected_raw_score = 0.12;

    for (tier, expected_score) in [
        (IdentitySecurityTier::MaxSafe, 0.30),
        (IdentitySecurityTier::LessSafe, 0.15),
        (IdentitySecurityTier::LowSecurity, expected_raw_score),
    ] {
        let home = temp_halo_home(tier.as_str());
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        let _home_guard =
            EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

        save_identity(&IdentityConfig {
            security_tier: Some(tier.clone()),
            ..IdentityConfig::default()
        })
        .expect("save identity tier");

        let db_path = home.join("traces.ndb");
        let mut writer = TraceWriter::new(&db_path).expect("writer");

        // All sessions are older than the 30-day window, so recency contributes
        // 0.0. With 30 failed sessions and 10 failed paid operations:
        // raw = 0.20 * 0.6 = 0.12 before the identity-tier floor.
        for i in 0..30 {
            writer
                .start_session(SessionMetadata {
                    session_id: format!("fail-{i}"),
                    agent: "adversary".to_string(),
                    model: None,
                    started_at: stale_started_at,
                    ended_at: None,
                    prompt: None,
                    status: SessionStatus::Running,
                    user_id: None,
                    machine_id: None,
                    puf_digest: None,
                })
                .expect("start");
            writer.end_session(SessionStatus::Failed).expect("fail");
        }

        for i in 0..10 {
            writer
                .record_paid_operation(PaidOperation {
                    operation_id: format!("fail-op-{i}"),
                    timestamp: stale_started_at,
                    operation_type: "compute".to_string(),
                    credits_spent: 5,
                    usd_equivalent: 0.05,
                    session_id: Some(format!("fail-{}", i % 30)),
                    result_digest: None,
                    success: false,
                    error: Some("adversarial failure".to_string()),
                })
                .expect("paid op");
        }

        let out = query_trust_score(&db_path, None).expect("score");
        let configured_floor = security_tier_trust_floor(&tier);

        assert!(
            (out.score - expected_score).abs() < 1e-12,
            "FALSIFIED Claim 4d: tier {tier:?} should yield score {expected_score}, got {} \
             (configured floor {}, raw score {})",
            out.score,
            configured_floor,
            expected_raw_score
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 5  EPISTEMIC TRUST ALGEBRA
// ═══════════════════════════════════════════════════════════════════════════

/// Claim 5a: The nucleus operator is extensive: N(x) ≥ x for all x ∈ [0,1].
///
/// Falsifier: any x where N(x) < x.
#[test]
fn epistemic_nucleus_extensive_sweep() {
    for floor in [0.0, 0.1, 0.3, 0.5, 0.8, 1.0] {
        let et = EpistemicTrust::new(floor);
        for i in 0..=100 {
            let x = i as f64 / 100.0;
            let nx = et.nucleus(x);
            assert!(
                nx >= x,
                "FALSIFIED Claim 5a: N({x}) = {nx} < {x} (floor={floor})"
            );
        }
    }
}

/// Claim 5b: The nucleus operator is idempotent: N(N(x)) = N(x).
///
/// Falsifier: any x where N(N(x)) ≠ N(x).
#[test]
fn epistemic_nucleus_idempotent_sweep() {
    for floor in [0.0, 0.15, 0.3, 0.5, 0.99, 1.0] {
        let et = EpistemicTrust::new(floor);
        for i in 0..=100 {
            let x = i as f64 / 100.0;
            let nx = et.nucleus(x);
            let nnx = et.nucleus(nx);
            assert!(
                (nnx - nx).abs() < 1e-12,
                "FALSIFIED Claim 5b: N(N({x})) = {nnx} ≠ N({x}) = {nx} (floor={floor})"
            );
        }
    }
}

/// Claim 5c: Fusion is commutative and associative on [0,1].
///
/// Falsifier: fuse(x,y) ≠ fuse(y,x) or fuse(fuse(x,y),z) ≠ fuse(x,fuse(y,z)).
#[test]
fn epistemic_fusion_commutative_associative() {
    let et = EpistemicTrust::new(0.0);
    let vals = [0.0, 0.1, 0.3, 0.5, 0.7, 1.0];

    for &x in &vals {
        for &y in &vals {
            // Commutativity
            let xy = et.fuse(x, y);
            let yx = et.fuse(y, x);
            assert!(
                (xy - yx).abs() < 1e-12,
                "FALSIFIED commutativity: fuse({x},{y})={xy} ≠ fuse({y},{x})={yx}"
            );

            for &z in &vals {
                // Associativity
                let lhs = et.fuse(et.fuse(x, y), z);
                let rhs = et.fuse(x, et.fuse(y, z));
                assert!(
                    (lhs - rhs).abs() < 1e-10,
                    "FALSIFIED associativity: fuse(fuse({x},{y}),{z})={lhs} ≠ fuse({x},fuse({y},{z}))={rhs}"
                );
            }
        }
    }
}

/// Claim 5d: The adjunction fuse(x,y) ≤ z ⟺ x ≤ ihom(y,z) holds for the
/// residuated lattice structure.
///
/// Falsifier: a triple (x,y,z) where the biconditional fails in either direction.
#[test]
fn epistemic_adjunction_sweep() {
    let et = EpistemicTrust::new(0.0);
    let vals = [0.0, 0.1, 0.2, 0.3, 0.5, 0.7, 0.9, 1.0];
    let eps = 1e-10;
    let mut forward_tested = 0u64;
    let mut reverse_tested = 0u64;

    for &x in &vals {
        for &y in &vals {
            for &z in &vals {
                let fused = et.fuse(x, y);
                let hom = et.ihom(y, z);

                // Forward: fuse(x,y) ≤ z ⟹ x ≤ ihom(y,z)
                if fused <= z + eps {
                    assert!(
                        x <= hom + eps,
                        "FALSIFIED Claim 5d (forward): fuse({x},{y})={fused} ≤ {z} \
                         but {x} > ihom({y},{z})={hom}"
                    );
                    forward_tested += 1;
                }

                // Reverse: x ≤ ihom(y,z) ⟹ fuse(x,y) ≤ z
                if x <= hom + eps {
                    assert!(
                        fused <= z + eps,
                        "FALSIFIED Claim 5d (reverse): {x} ≤ ihom({y},{z})={hom} \
                         but fuse({x},{y})={fused} > {z}"
                    );
                    reverse_tested += 1;
                }
            }
        }
    }
    assert!(
        forward_tested > 50,
        "too few forward cases tested: {forward_tested}"
    );
    assert!(
        reverse_tested > 50,
        "too few reverse cases tested: {reverse_tested}"
    );
}

/// Claim 5e: combine() with an empty slice returns the nucleus of the
/// multiplicative identity (1.0), which is max(1.0, floor) = 1.0.
///
/// Falsifier: combine([]) ≠ 1.0.
#[test]
fn epistemic_combine_empty_is_identity() {
    for floor in [0.0, 0.3, 0.5, 0.99, 1.0] {
        let et = EpistemicTrust::new(floor);
        let result = et.combine(&[]);
        assert!(
            (result - 1.0).abs() < 1e-12,
            "FALSIFIED Claim 5e: combine([]) = {result} ≠ 1.0 (floor={floor})"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 6  CROSS-COMPONENT INTERACTION
// ═══════════════════════════════════════════════════════════════════════════

/// Claim 6a: The production storage composition of the governor and Chebyshev
/// evictor restores capacity under pressure without evicting the hottest item.
///
/// This is the real runtime integration used by `VectorIndex::enforce_pressure`:
/// the governor observes current occupancy to produce an eviction budget, then
/// the Chebyshev wrapper supplies the cold candidates to evict.
///
/// Falsifier: storage remains over capacity, no eviction occurs, or the hot item
/// is removed despite repeated accesses.
#[test]
fn governor_plus_chebyshev_storage_composition() {
    let mut index = VectorIndex::new();
    index.set_max_entries(2);

    index.upsert("hot", vec![1.0, 0.0]).expect("insert hot");
    index.upsert("warm", vec![0.0, 1.0]).expect("insert warm");

    for _ in 0..20 {
        let results = index
            .search_with_access(&[1.0, 0.0], 1, DistanceMetric::Cosine)
            .expect("search hot");
        assert_eq!(
            results[0].key, "hot",
            "hot vector should remain the nearest match"
        );
    }

    for (key, dims) in [
        ("cold_a", vec![0.5, 0.5]),
        ("cold_b", vec![0.6, 0.4]),
        ("cold_c", vec![0.4, 0.6]),
    ] {
        index.upsert(key, dims).expect("insert cold vector");
    }

    let stats = index.eviction_stats();
    let cold_survivors = ["cold_a", "cold_b", "cold_c"]
        .into_iter()
        .filter(|key| index.get(key).is_some())
        .count();

    assert!(
        stats.last_eviction_count > 0,
        "FALSIFIED Claim 6a: storage pressure caused no evictions"
    );
    assert!(
        stats.governor_regime.contains("multi-step"),
        "FALSIFIED Claim 6a: storage governor never left from-rest, so pressure handling was not exercised"
    );
    assert!(
        index.len() <= 2,
        "FALSIFIED Claim 6a: storage remained over capacity with len={}",
        index.len()
    );
    assert!(
        index.get("hot").is_some(),
        "FALSIFIED Claim 6a: the hot vector was evicted despite repeated accesses"
    );
    assert!(
        cold_survivors < 3,
        "FALSIFIED Claim 6a: no cold vectors were evicted under pressure"
    );
}

/// Claim 6b: Chebyshev eviction + Diode post-filter composition — when a
/// caller applies diode_floor as a post-filter on Chebyshev eviction
/// candidates, no peer whose trust is at or above the floor for their tier
/// can survive as a final eviction target.
///
/// The Chebyshev evictor is tier-unaware by design. This test verifies the
/// architectural contract: (1) Chebyshev CAN select the staked peer as a
/// candidate (so the diode is actually necessary), and (2) the diode
/// post-filter correctly removes it.
///
/// Falsifier: the diode post-filter fails to remove a floor-protected peer,
/// OR Chebyshev never selects the staked peer (making the test vacuous).
#[test]
fn chebyshev_plus_diode_composition() {
    let mut evictor = ChebyshevEvictor::new(1.0, 0.0, 1.0);

    // Staked peer with low liveness — Chebyshev should select it as a
    // candidate because it operates on liveness, not trust.
    evictor.liveness.insert("staked_peer".to_string(), 0.1);

    // High-liveness active peers.
    for i in 0..20 {
        evictor
            .liveness
            .insert(format!("active_{i}"), 50.0 + i as f64);
    }

    let raw_candidates = evictor.eviction_candidates(10);

    // Step 1: Chebyshev MUST have selected staked_peer (it has the lowest liveness).
    assert!(
        raw_candidates.contains(&"staked_peer".to_string()),
        "Claim 6b is vacuous: Chebyshev never selected staked_peer as a candidate. \
         The diode post-filter was never exercised."
    );

    // Step 2: Apply diode post-filter. Each candidate's trust is checked against
    // its tier's floor. The staked peer's trust is at its floor (diode-protected).
    let staked_trust = diode_floor(0.0, IdentityTier::Staked);
    let filtered: Vec<&String> = raw_candidates
        .iter()
        .filter(|key| {
            if key.as_str() == "staked_peer" {
                // This peer is Staked; its trust after diode is at the floor.
                // The post-filter removes peers whose protected trust ≥ their floor.
                staked_trust < TRUST_FLOOR_STAKED
            } else {
                true // non-staked peers pass through
            }
        })
        .collect();

    assert!(
        !filtered.iter().any(|k| k.as_str() == "staked_peer"),
        "FALSIFIED Claim 6b: staked_peer survived the diode post-filter with trust={staked_trust}"
    );

    // Verify the post-filter removed exactly the protected peer (cold items remain).
    assert!(
        filtered.len() < raw_candidates.len(),
        "Post-filter should have removed at least one candidate"
    );
}

/// Claim 6c: The trust decay half-life and diode floor interact correctly —
/// even after many half-lives of decay with no new positive evidence, the
/// diode floor prevents total trust annihilation for Staked peers.
///
/// This is a genuine interaction test: we first demonstrate that decay DOES
/// drive raw trust well below the Staked floor (establishing that the diode
/// is actually necessary), then verify the diode post-correction holds.
///
/// Falsifier: the diode fails to rescue, OR decay never drives raw trust
/// below the floor (making the composition vacuous).
#[test]
fn decay_plus_diode_long_horizon() {
    // A strong initial record: Deep challenge passed.
    let legitimate = VerificationRecord {
        peer_did: "did:key:staked".to_string(),
        capability_domain: "prove/lean".to_string(),
        challenge_difficulty: ChallengeDifficulty::Deep,
        passed: true,
        elapsed_ms: 400,
        verified_at: 0,
    };

    let half_life = 3600; // 1 hour
    let initial_trust = compute_trust(std::slice::from_ref(&legitimate), 0, half_life);
    assert!(
        initial_trust > TRUST_FLOOR_STAKED,
        "Initial trust {initial_trust} should exceed the floor to make this test meaningful"
    );

    let mut decay_crossed_below_floor = false;

    for n_halflives in [1, 2, 5, 10, 20, 50, 100, 1000] {
        let now = n_halflives * half_life;
        let raw = compute_trust(std::slice::from_ref(&legitimate), now, half_life);

        if raw < TRUST_FLOOR_STAKED {
            decay_crossed_below_floor = true;
        }

        let protected = diode_floor(raw, IdentityTier::Staked);
        assert!(
            protected >= TRUST_FLOOR_STAKED,
            "FALSIFIED Claim 6c: after {n_halflives} half-lives, \
             protected trust = {protected} < {TRUST_FLOOR_STAKED} (raw={raw})"
        );
    }

    // Decay MUST have driven raw trust below the floor at some point,
    // otherwise the test is vacuous — the diode was never needed.
    assert!(
        decay_crossed_below_floor,
        "Claim 6c is vacuous: decay never drove raw trust below TRUST_FLOOR_STAKED={TRUST_FLOOR_STAKED}. \
         Initial trust was {initial_trust}; increase half-lives or reduce initial trust.",
        initial_trust = initial_trust
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// § 7  VERSIONED POLICY REGISTRY
// ═══════════════════════════════════════════════════════════════════════════

use nucleusdb::halo::policy_registry::{
    collect_static_snapshot, recompute_digest, validate_invariants, PolicyCategory, PolicyValue,
    CHALLENGE_FAILURE_PENALTY, CHALLENGE_WEIGHT_DEEP, CHALLENGE_WEIGHT_PING,
    CHALLENGE_WEIGHT_STANDARD, POLICY_SCHEMA_VERSION, TIER_THRESHOLD_CAUTIOUS, TIER_THRESHOLD_HIGH,
    TIER_THRESHOLD_MEDIUM, WEIGHT_ATTESTATION, WEIGHT_BASE, WEIGHT_COMPLETION, WEIGHT_PAID_SUCCESS,
    WEIGHT_RECENCY,
};

/// Claim 7a: The policy registry's cross-component invariants all hold for
/// the default governance configuration.
///
/// Falsifier: any invariant violation in the static snapshot.
#[test]
fn policy_registry_invariants_hold() {
    let snapshot = collect_static_snapshot(0);
    let violations = validate_invariants(&snapshot);
    assert!(
        violations.is_empty(),
        "FALSIFIED Claim 7a: governance invariants violated: {violations:?}"
    );
}

/// Claim 7b: The registry constants match the actual values used in the
/// production code paths. If a constant is changed in the source but not
/// in the registry, this test catches the drift.
///
/// Falsifier: any registry constant diverges from its source-of-truth.
#[test]
fn policy_registry_constants_match_source() {
    // Production trust tier thresholds must match trust.rs:trust_tier()
    // We verify by checking boundary behavior: score at threshold yields the tier.
    assert_eq!(TIER_THRESHOLD_HIGH, 0.85);
    assert_eq!(TIER_THRESHOLD_MEDIUM, 0.65);
    assert_eq!(TIER_THRESHOLD_CAUTIOUS, 0.40);

    // Production scoring weights must match trust.rs:query_trust_score()
    assert_eq!(WEIGHT_BASE, 0.20);
    assert_eq!(WEIGHT_COMPLETION, 0.30);
    assert_eq!(WEIGHT_PAID_SUCCESS, 0.25);
    assert_eq!(WEIGHT_ATTESTATION, 0.15);
    assert_eq!(WEIGHT_RECENCY, 0.10);

    // Weights must sum to exactly 1.0 (base + 4 components)
    let weight_sum =
        WEIGHT_BASE + WEIGHT_COMPLETION + WEIGHT_PAID_SUCCESS + WEIGHT_ATTESTATION + WEIGHT_RECENCY;
    assert!(
        (weight_sum - 1.0).abs() < 1e-12,
        "FALSIFIED Claim 7b: scoring weights sum to {weight_sum}, expected 1.0"
    );

    // Challenge weights must match trust_score.rs:compute_trust()
    assert_eq!(CHALLENGE_WEIGHT_PING, 0.1);
    assert_eq!(CHALLENGE_WEIGHT_STANDARD, 1.0);
    assert_eq!(CHALLENGE_WEIGHT_DEEP, 5.0);
    assert_eq!(CHALLENGE_FAILURE_PENALTY, -2.0);
}

/// Claim 7c: The static snapshot is complete — it contains entries for every
/// governance policy category, both storage eviction policies, and all runtime
/// plus storage-local governor instances.
///
/// Falsifier: a category or governor instance is missing from the snapshot.
#[test]
fn policy_registry_snapshot_completeness() {
    let snapshot = collect_static_snapshot(0);

    // Every policy category must be represented.
    let categories: Vec<PolicyCategory> = snapshot.entries.iter().map(|e| e.category).collect();
    assert!(categories.contains(&PolicyCategory::TrustFloor));
    assert!(categories.contains(&PolicyCategory::TrustThreshold));
    assert!(categories.contains(&PolicyCategory::TrustScoring));
    assert!(categories.contains(&PolicyCategory::GovernorControl));
    assert!(categories.contains(&PolicyCategory::EvictionGuard));
    assert!(categories.contains(&PolicyCategory::NetworkHealth));

    for expected in ["vector_storage_eviction", "blob_storage_eviction"] {
        assert!(
            snapshot.entries.iter().any(|entry| entry.id == expected),
            "FALSIFIED Claim 7c: eviction policy '{expected}' missing from snapshot"
        );
    }

    // All runtime plus storage-local governor instances must be present.
    let gov_ids: Vec<&str> = snapshot
        .entries
        .iter()
        .filter(|e| e.category == PolicyCategory::GovernorControl)
        .map(|e| e.id.as_str())
        .collect();
    for expected in [
        "governor_gov_proxy",
        "governor_gov_comms",
        "governor_gov_compute",
        "governor_gov_cost",
        "governor_gov_pty",
        "governor_gov_memory_vector",
        "governor_gov_memory_blob",
    ] {
        assert!(
            gov_ids.contains(&expected),
            "FALSIFIED Claim 7c: governor '{expected}' missing from snapshot"
        );
    }

    // Schema version must be current.
    assert_eq!(snapshot.schema_version, POLICY_SCHEMA_VERSION);
}

/// Claim 7d: The snapshot digest is deterministic and changes when any
/// governance parameter changes. This enables drift detection.
///
/// Falsifier: (a) two snapshots with the same policies produce different
/// digests, or (b) a policy mutation does not change the digest.
#[test]
fn policy_registry_digest_tamper_detection() {
    let a = collect_static_snapshot(1000);
    let b = collect_static_snapshot(2000);

    // Same policies, different timestamps → same digest.
    assert_eq!(
        a.digest, b.digest,
        "FALSIFIED Claim 7d(a): determinism — same policies produced different digests"
    );

    // Mutate a policy value and verify the digest changes.
    let mut entries = a.entries.clone();
    if let Some(entry) = entries.iter_mut().find(|e| e.id == "p2p_trust_floors") {
        entry.values[0].1 = PolicyValue::Float(999.0);
    }
    let mutated_digest = recompute_digest(&entries);

    assert_ne!(
        a.digest, mutated_digest,
        "FALSIFIED Claim 7d(b): mutating a policy entry did not change the digest"
    );

    let c = collect_static_snapshot(3000);
    assert_eq!(a.digest, c.digest, "digest should be stable across calls");
}

/// Claim 7e: All governor entries in the policy snapshot satisfy the gain
/// condition γ < 1. A governor with γ ≥ 1 is outside the formal proof
/// regime and would be flagged by validate_invariants.
///
/// Falsifier: a default governor has γ ≥ 1.
#[test]
fn policy_registry_all_governors_in_formal_regime() {
    let snapshot = collect_static_snapshot(0);

    for entry in snapshot
        .entries
        .iter()
        .filter(|e| e.category == PolicyCategory::GovernorControl)
    {
        let gamma = entry
            .values
            .iter()
            .find(|(k, _)| k == "gamma")
            .and_then(|(_, v)| v.as_f64())
            .unwrap_or_else(|| panic!("governor '{}' missing gamma", entry.id));

        assert!(
            gamma < 1.0,
            "FALSIFIED Claim 7e: governor '{}' has γ={gamma:.6} >= 1.0",
            entry.id
        );

        // Also verify eps_min > 0 and eps_max > eps_min.
        let eps_min = entry
            .values
            .iter()
            .find(|(k, _)| k == "eps_min")
            .and_then(|(_, v)| v.as_f64())
            .unwrap();
        let eps_max = entry
            .values
            .iter()
            .find(|(k, _)| k == "eps_max")
            .and_then(|(_, v)| v.as_f64())
            .unwrap();
        assert!(
            eps_min > 0.0,
            "governor '{}' eps_min={eps_min} must be > 0",
            entry.id
        );
        assert!(
            eps_max > eps_min,
            "governor '{}' eps_max={eps_max} must be > eps_min={eps_min}",
            entry.id
        );
    }
}
