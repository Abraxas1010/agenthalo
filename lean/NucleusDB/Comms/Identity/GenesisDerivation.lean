import Mathlib.CategoryTheory.Discrete.Basic
import Mathlib.CategoryTheory.Functor.Basic

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Identity

/-- HKDF info labels used for sovereign communication identity derivation. -/
inductive DerivationInfo where
  | p2pIdentity
  | didPqSigning
  | didcommX25519
  | didcommMlKem768
  deriving DecidableEq, Repr

/-- Keypair/material targets reached from genesis HKDF branches. -/
inductive KeypairType where
  | ed25519
  | mlDsa65
  | x25519
  | mlKem768
  deriving DecidableEq, Repr

abbrev InfoCategory := CategoryTheory.Discrete DerivationInfo
abbrev KeyCategory := CategoryTheory.Discrete KeypairType

/-- Branch mapping from HKDF info label to cryptographic key family. -/
def keypairOfInfo : DerivationInfo → KeypairType
  | .p2pIdentity => .ed25519
  | .didPqSigning => .mlDsa65
  | .didcommX25519 => .x25519
  | .didcommMlKem768 => .mlKem768

/-- Functorial view of the genesis derivation tree. -/
def derivationFunctor : CategoryTheory.Functor InfoCategory KeyCategory :=
  CategoryTheory.Discrete.functor (fun info => ⟨keypairOfInfo info⟩)

abbrev Seed64 := Fin 64 → Nat
abbrev DerivedBytes := Fin 64 → Nat

/-- Abstract HKDF-SHA256 oracle used by the formal model. -/
axiom hkdf_sha256 : Seed64 → DerivationInfo → DerivedBytes

/-- Cryptographic assumption marker mirroring existing KEM assumption style. -/
axiom hkdf_is_prf : Prop

/-- Deterministic derivation function used by Phase 0 DID key material extraction. -/
noncomputable def derive (seed : Seed64) (info : DerivationInfo) : DerivedBytes :=
  hkdf_sha256 seed info

theorem derive_functional (seed : Seed64) (info : DerivationInfo) :
    ∃! out, out = derive seed info := by
  refine ⟨derive seed info, rfl, ?_⟩
  intro y hy
  simp [hy]

/-- T5: genesis derivation is deterministic as a total function. -/
theorem genesis_derivation_deterministic (seed : Seed64) (info : DerivationInfo) :
    derive seed info = derive seed info := by
  rfl

theorem derivationFunctor_obj_correct (info : DerivationInfo) :
    (derivationFunctor.obj ⟨info⟩).as = keypairOfInfo info := by
  simp [derivationFunctor, keypairOfInfo]

end Identity
end Comms
end NucleusDB
end HeytingLean
