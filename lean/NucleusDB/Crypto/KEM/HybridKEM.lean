import Mathlib.Data.Real.Basic

namespace HeytingLean
namespace NucleusDB
namespace Crypto
namespace KEM

/-!
# NucleusDB.Crypto.KEM.HybridKEM

Self-contained local mirror of the assumption-scoped hybrid KEM surface used by
the Rust runtime provenance layer.
-/

/-- Abstract KEM interface for the local mirror. -/
structure KEMScheme where
  PublicKey : Type
  SecretKey : Type
  Ciphertext : Type
  SharedSecret : Type
  keygen : Unit → PublicKey × SecretKey
  encaps : PublicKey → Ciphertext × SharedSecret
  decaps : SecretKey → Ciphertext → Option SharedSecret

/-- Minimal adversary surface for the local IND-CCA mirror. -/
structure INDCCAAdversary (_K : KEMScheme) where
  advantage : ℝ
  advantage_nonneg : 0 ≤ advantage

/-- Local advantage function. -/
def IND_CCA_Advantage (K : KEMScheme) (A : INDCCAAdversary K) : ℝ :=
  A.advantage

/-- Security witness for the local IND-CCA surface. -/
structure INDCCASecurityWitness (K : KEMScheme) where
  bound : INDCCAAdversary K → ℝ
  sound : ∀ A, IND_CCA_Advantage K A ≤ bound A

/-- Assumption-scoped IND-CCA predicate. -/
def IND_CCA (K : KEMScheme) : Prop :=
  Nonempty (INDCCASecurityWitness K)

/-- Hybrid combiner: keygen/encaps/decaps in product. -/
def hybridKEM (K1 K2 : KEMScheme) : KEMScheme where
  PublicKey := K1.PublicKey × K2.PublicKey
  SecretKey := K1.SecretKey × K2.SecretKey
  Ciphertext := K1.Ciphertext × K2.Ciphertext
  SharedSecret := K1.SharedSecret × K2.SharedSecret
  keygen := fun () =>
    let (pk1, sk1) := K1.keygen ()
    let (pk2, sk2) := K2.keygen ()
    ((pk1, pk2), (sk1, sk2))
  encaps := fun (pk1, pk2) =>
    let (ct1, ss1) := K1.encaps pk1
    let (ct2, ss2) := K2.encaps pk2
    ((ct1, ct2), (ss1, ss2))
  decaps := fun (sk1, sk2) (ct1, ct2) =>
    match K1.decaps sk1 ct1, K2.decaps sk2 ct2 with
    | some ss1, some ss2 => some (ss1, ss2)
    | _, _ => none

/-- Explicit left-component reduction surface. -/
def LeftReductionStatement (K1 K2 : KEMScheme) : Prop :=
  ∀ A : INDCCAAdversary (hybridKEM K1 K2),
    ∃ B : INDCCAAdversary K1,
      IND_CCA_Advantage (hybridKEM K1 K2) A ≤ IND_CCA_Advantage K1 B

/-- Explicit right-component reduction surface. -/
def RightReductionStatement (K1 K2 : KEMScheme) : Prop :=
  ∀ A : INDCCAAdversary (hybridKEM K1 K2),
    ∃ B : INDCCAAdversary K2,
      IND_CCA_Advantage (hybridKEM K1 K2) A ≤ IND_CCA_Advantage K2 B

/-- Named reduction bundle. -/
structure ReductionAssumptionBundle (K1 K2 : KEMScheme) where
  left : LeftReductionStatement K1 K2
  right : RightReductionStatement K1 K2

/-- Predicate asserting that the bundle has been documented. -/
def DocumentedReductionAssumptions (K1 K2 : KEMScheme) : Prop :=
  Nonempty (ReductionAssumptionBundle K1 K2)

theorem hybrid_security_of_left (K1 K2 : KEMScheme)
    (hRed : LeftReductionStatement K1 K2) :
    IND_CCA K1 → IND_CCA (hybridKEM K1 K2) := by
  classical
  intro hSec
  rcases hSec with ⟨sec⟩
  refine ⟨{
    bound := fun A => sec.bound (Classical.choose (hRed A))
    sound := ?_
  }⟩
  intro A
  have hChosen := Classical.choose_spec (hRed A)
  exact le_trans hChosen (sec.sound _)

theorem hybrid_security_of_right (K1 K2 : KEMScheme)
    (hRed : RightReductionStatement K1 K2) :
    IND_CCA K2 → IND_CCA (hybridKEM K1 K2) := by
  classical
  intro hSec
  rcases hSec with ⟨sec⟩
  refine ⟨{
    bound := fun A => sec.bound (Classical.choose (hRed A))
    sound := ?_
  }⟩
  intro A
  have hChosen := Classical.choose_spec (hRed A)
  exact le_trans hChosen (sec.sound _)

theorem hybrid_security_of_or (K1 K2 : KEMScheme)
    (hLeft : LeftReductionStatement K1 K2)
    (hRight : RightReductionStatement K1 K2) :
    (IND_CCA K1 ∨ IND_CCA K2) → IND_CCA (hybridKEM K1 K2) := by
  intro h
  cases h with
  | inl h1 => exact hybrid_security_of_left K1 K2 hLeft h1
  | inr h2 => exact hybrid_security_of_right K1 K2 hRight h2

theorem hybrid_security_of_documentedAssumptions (K1 K2 : KEMScheme)
    (hBundle : DocumentedReductionAssumptions K1 K2) :
    (IND_CCA K1 ∨ IND_CCA K2) → IND_CCA (hybridKEM K1 K2) := by
  intro h
  rcases hBundle with ⟨bundle⟩
  exact hybrid_security_of_or K1 K2 bundle.left bundle.right h

/-- Abstract source of key material. -/
structure KeySource where
  Key : Type
  gen : Unit → Key

/-- Minimal structural usability for a key source. -/
def KeySourceUsable (S : KeySource) : Prop :=
  Nonempty S.Key

theorem keySourceUsable (S : KeySource) : KeySourceUsable S := by
  exact ⟨S.gen ()⟩

/-- Product combiner for key sources. -/
def hybridKeySource (S1 S2 : KeySource) : KeySource where
  Key := S1.Key × S2.Key
  gen := fun () => (S1.gen (), S2.gen ())

theorem hybridKey_usable_of_left (S1 S2 : KeySource) :
    KeySourceUsable S1 → KeySourceUsable (hybridKeySource S1 S2) := by
  intro h1
  exact ⟨(Classical.choice h1, S2.gen ())⟩

theorem hybridKey_usable_of_right (S1 S2 : KeySource) :
    KeySourceUsable S2 → KeySourceUsable (hybridKeySource S1 S2) := by
  intro h2
  exact ⟨(S1.gen (), Classical.choice h2)⟩

theorem hybridKey_usable_of_or (S1 S2 : KeySource) :
    (KeySourceUsable S1 ∨ KeySourceUsable S2) → KeySourceUsable (hybridKeySource S1 S2) := by
  intro h
  cases h with
  | inl h1 => exact hybridKey_usable_of_left S1 S2 h1
  | inr h2 => exact hybridKey_usable_of_right S1 S2 h2

end KEM
end Crypto
end NucleusDB
end HeytingLean
