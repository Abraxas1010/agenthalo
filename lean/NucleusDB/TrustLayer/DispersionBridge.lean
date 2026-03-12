import Mathlib
import Lean.Data.Json
import NucleusDB.Comms.Protocol.TrustSpec

/-!
# NucleusDB.TrustLayer.DispersionBridge

Float-backed mirror of the trust-density dispersion consumer surface.

The canonical real-valued proofs live in the Heyting repository's
`nucleusdb_trust_dispersion_20260312` project (commit 0973d83c20). This module
does not import those theorem sources; it mirrors the exported Float formulas
for dashboard and runtime consumption inside NucleusDB.
-/

namespace HeytingLean
namespace NucleusDB
namespace TrustLayer

open Lean
open HeytingLean.NucleusDB.Comms.Protocol

/-- Trust-medium parameters mirrored at runtime Float precision. -/
structure TrustDispersionConfig where
  trustElasticity : Float
  infoFriction : Float
  candidateRho : Float
  deriving Repr, ToJson, FromJson

/-- Float mirror of the quadratic dispersion law `ω² = c² k² + D² k⁴`. -/
def dispersionOmegaSquared (cfg : TrustDispersionConfig) (k : Float) : Float :=
  cfg.trustElasticity ^ 2 * k ^ 2 + cfg.infoFriction ^ 2 * k ^ 4

/-- Wave/dispersive balance point `k_c = c / D`. -/
def crossoverWavenumber (cfg : TrustDispersionConfig) : Float :=
  cfg.trustElasticity / cfg.infoFriction

def waveLikeLabel : String := "wave-like"

def dispersionDominatedLabel : String := "dispersion-dominated"

/-- Regime label used by the dashboard/runtime consumer surface. -/
def perturbationRegime (cfg : TrustDispersionConfig) (k : Float) : String :=
  if k < crossoverWavenumber cfg then waveLikeLabel else dispersionDominatedLabel

/-- JSON-shaped stability export aligned with the Heyting bridge surface. -/
structure StabilityReport where
  wavenumberK : Float
  omegaSquared : Float
  trustElasticity : Float
  infoFriction : Float
  candidateRho : Float
  crossoverK : Float
  regime : String
  deriving Repr, ToJson, FromJson

def mkStabilityReport (cfg : TrustDispersionConfig) (k : Float) : StabilityReport :=
  { wavenumberK := k
    omegaSquared := dispersionOmegaSquared cfg k
    trustElasticity := cfg.trustElasticity
    infoFriction := cfg.infoFriction
    candidateRho := cfg.candidateRho
    crossoverK := crossoverWavenumber cfg
    regime := perturbationRegime cfg k }

/-- A localized trust event classified by its spatial frequency. -/
structure TrustPerturbation where
  wavenumberK : Float
  trustDelta : Float
  regime : String
  deriving Repr, ToJson, FromJson

def classifyPerturbation (cfg : TrustDispersionConfig) (k trustDelta : Float) :
    TrustPerturbation :=
  { wavenumberK := k
    trustDelta := trustDelta
    regime := perturbationRegime cfg k }

/-- Float mirror of `TrustSpec.computeTrust`, tracked in runtime units. -/
def runtimeTrustScore (records : List VerificationRecord) (now halfLife : Nat) : Float :=
  Float.ofInt (computeTrust records now halfLife) / 10.0

/-- The routing threshold used by the existing trust consumer surface. -/
def routingReady (records : List VerificationRecord) (now halfLife : Nat) : Bool :=
  decide ((1 : Rat) / 2 ≤ runtimeTrustApprox records now halfLife)

/-- Flat consumer payload joining trust scoring with the dispersion report. -/
structure TrustDispersionConsumerReport extends StabilityReport where
  trustScore : Float
  routingReady : Bool
  trustDelta : Float
  deriving Repr, ToJson, FromJson

def mkTrustDispersionConsumerReport
    (cfg : TrustDispersionConfig)
    (records : List VerificationRecord)
    (now halfLife : Nat)
    (k trustDelta : Float) :
    TrustDispersionConsumerReport :=
  let report := mkStabilityReport cfg k
  { wavenumberK := report.wavenumberK
    omegaSquared := report.omegaSquared
    trustElasticity := report.trustElasticity
    infoFriction := report.infoFriction
    candidateRho := report.candidateRho
    crossoverK := report.crossoverK
    regime := report.regime
    trustScore := runtimeTrustScore records now halfLife
    routingReady := routingReady records now halfLife
    trustDelta := trustDelta }

/-- Demo config matching the audited Heyting trust-dispersion example. -/
def demoConfig : TrustDispersionConfig :=
  { trustElasticity := 0.1
    infoFriction := 0.025
    candidateRho := 0.99 }

def pingOnlyRecords : List VerificationRecord :=
  [{ difficulty := ChallengeDifficulty.ping, passed := true, verifiedAt := 0 }]

def deepProofRecords : List VerificationRecord :=
  [{ difficulty := ChallengeDifficulty.deep, passed := true, verifiedAt := 0 }]

theorem demo_crossover_is_four :
    (crossoverWavenumber demoConfig == 4.0) = true := by
  native_decide

theorem wave_regime_below_crossover :
    (mkStabilityReport demoConfig 2.0).regime = waveLikeLabel := by
  native_decide

theorem dispersion_regime_at_crossover :
    (mkStabilityReport demoConfig 4.0).regime = dispersionDominatedLabel := by
  native_decide

/-- Exact Float fingerprint of the `k = 2` mirror (`0.050000` when rendered). -/
theorem omega_k2_matches_mirrored_formula :
    (dispersionOmegaSquared demoConfig 2.0).toRatParts =
      some (7205759403792795, -57) := by
  native_decide

/-- Exact Float fingerprint of the `k = 4` mirror (`0.320000` when rendered). -/
theorem omega_k4_matches_mirrored_formula :
    (dispersionOmegaSquared demoConfig 4.0).toRatParts =
      some (5764607523034236, -54) := by
  native_decide

theorem omega_monotone_on_demo_points :
    dispersionOmegaSquared demoConfig 2.0 < dispersionOmegaSquared demoConfig 4.0 := by
  native_decide

theorem ping_only_records_not_routing_ready :
    routingReady pingOnlyRecords 0 1 = false := by
  native_decide

theorem deep_proof_records_are_routing_ready :
    routingReady deepProofRecords 0 1 = true := by
  native_decide

theorem integrated_consumer_report_carries_trust_score :
    ((mkTrustDispersionConsumerReport demoConfig deepProofRecords 0 1 2.0 0.125).trustScore == 5.0) = true := by
  native_decide

theorem integrated_consumer_report_preserves_wave_regime :
    (mkTrustDispersionConsumerReport demoConfig deepProofRecords 0 1 2.0 0.125).regime =
      waveLikeLabel := by
  native_decide

end TrustLayer
end NucleusDB
end HeytingLean
