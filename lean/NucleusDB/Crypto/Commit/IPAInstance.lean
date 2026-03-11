import Mathlib.Algebra.BigOperators.Group.Finset.Basic
import Mathlib.Algebra.BigOperators.Pi
import Mathlib.Data.Fin.Basic
import Mathlib.Data.Real.Basic

/-!
# NucleusDB.Crypto.Commit.IPAInstance

Self-contained local mirror of the Pedersen-shaped vector-commitment surface
used by the Rust provenance layer.
-/

namespace HeytingLean
namespace NucleusDB
namespace Crypto
namespace Commit

namespace Vec

structure Scheme where
  Idx : Type
  Val : Type
  Com : Type
  Proof : Type
  Rand : Type
  commit : (Idx → Val) → Rand → Com
  openAt : (Idx → Val) → Rand → Idx → Proof
  verifyAt : Com → Idx → Val → Proof → Prop

def OpenCorrect (S : Scheme) : Prop :=
  ∀ (v : S.Idx → S.Val) (r : S.Rand) (i : S.Idx),
    S.verifyAt (S.commit v r) i (v i) (S.openAt v r i)

def OpenSound (S : Scheme) : Prop :=
  ∀ (v : S.Idx → S.Val) (r : S.Rand) (i : S.Idx) (y : S.Val) (π : S.Proof),
    S.verifyAt (S.commit v r) i y π → y = v i

def VerificationConsistencyAt (S : Scheme) : Prop :=
  ∀ (v₁ v₂ : S.Idx → S.Val) (r₁ r₂ : S.Rand) (i : S.Idx),
    S.verifyAt (S.commit v₁ r₁) i (v₁ i) (S.openAt v₁ r₁ i) ∧
      S.verifyAt (S.commit v₂ r₂) i (v₂ i) (S.openAt v₂ r₂ i)

theorem verificationConsistencyAt_of_openCorrect
    (S : Scheme) (h : OpenCorrect S) :
    VerificationConsistencyAt S := by
  intro v₁ v₂ r₁ r₂ i
  exact ⟨h v₁ r₁ i, h v₂ r₂ i⟩

structure SecurityProps (S : Scheme) where
  bindingAt : Prop
  verificationConsistencyAt : Prop
  computationalHidingAt : Prop
  extractable : Prop

end Vec

namespace IPAInstance

open scoped BigOperators

structure Params (n : Nat) (G : Type) [AddCommGroup G] where
  generators : Fin n → G
  blindingGenerator : G

def commitVector {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G) (v : Fin n → Int) : G :=
  ∑ i, v i • P.generators i

def pedersenCommit {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G) (v : Fin n → Int) (r : Int) : G :=
  r • P.blindingGenerator + commitVector P v

def Binding {n : Nat} {G : Type} [AddCommGroup G] (P : Params n G) : Prop :=
  Function.Injective (fun vr : (Fin n → Int) × Int => pedersenCommit P vr.1 vr.2)

structure DLogAdversary {Inst : Type} (P : Inst) where
  advantage : ℝ
  advantage_nonneg : 0 ≤ advantage
  advantage_le_one : advantage ≤ 1

def DLogAdvantage {Inst : Type} {P : Inst} (A : DLogAdversary P) : ℝ :=
  A.advantage

structure DLogHardnessWitness {Inst : Type} (P : Inst) where
  ε : ℝ
  ε_nonneg : 0 ≤ ε
  sound : ∀ A : DLogAdversary P, DLogAdvantage A ≤ ε

def DLogHardness {Inst : Type} (P : Inst) : Prop :=
  Nonempty (DLogHardnessWitness P)

structure HidingAdversary {Inst : Type} (P : Inst) (n : Nat) where
  left : Fin n → Int
  right : Fin n → Int
  openedIndex : Fin n
  agreeAtOpened : left openedIndex = right openedIndex
  advantage : ℝ
  advantage_nonneg : 0 ≤ advantage
  advantage_le_one : advantage ≤ 1

def HidingAdvantage {Inst : Type} {P : Inst} {n : Nat}
    (A : HidingAdversary P n) : ℝ :=
  A.advantage

structure ComputationalHidingWitness {Inst : Type} (P : Inst) (n : Nat) where
  ε : ℝ
  ε_nonneg : 0 ≤ ε
  sound : ∀ A : HidingAdversary P n, HidingAdvantage A ≤ ε

def ComputationalHiding {Inst : Type} (P : Inst) (n : Nat) : Prop :=
  Nonempty (ComputationalHidingWitness P n)

structure DLogReductionStatement {Inst : Type} (P : Inst) (n : Nat) where
  loss : ℝ
  loss_nonneg : 0 ≤ loss
  transport :
    ∀ A : HidingAdversary P n,
      ∃ B : DLogAdversary P,
        HidingAdvantage A ≤ DLogAdvantage B + loss

theorem computationalHiding_of_dlog
    {Inst : Type} {P : Inst} {n : Nat}
    (hHard : DLogHardness P)
    (hRed : DLogReductionStatement P n) :
    ComputationalHiding P n := by
  rcases hHard with ⟨hard⟩
  refine ⟨{
    ε := hard.ε + hRed.loss
    ε_nonneg := add_nonneg hard.ε_nonneg hRed.loss_nonneg
    sound := ?_
  }⟩
  intro A
  rcases hRed.transport A with ⟨B, hAB⟩
  exact le_trans hAB (add_le_add (hard.sound B) le_rfl)

def scheme {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G) : Vec.Scheme :=
  { Idx := Fin n
    Val := Int
    Com := G
    Proof := (Fin n → Int) × Int
    Rand := Int
    commit := pedersenCommit P
    openAt := fun v r _ => (v, r)
    verifyAt := fun c i y π => pedersenCommit P π.1 π.2 = c ∧ π.1 i = y }

theorem openCorrect
    {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G) :
    Vec.OpenCorrect (scheme P) := by
  intro v r i
  simp [scheme]

theorem openSound_of_binding
    {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G) (hBind : Binding P) :
    Vec.OpenSound (scheme P) := by
  intro v r i y π h
  have h' : pedersenCommit P π.1 π.2 = pedersenCommit P v r ∧ π.1 i = y := by
    simpa [scheme] using h
  rcases h' with ⟨hCommit, hValue⟩
  have hEq : π = (v, r) := hBind hCommit
  have hVector : π.1 = v := congrArg Prod.fst hEq
  simpa [hVector] using hValue.symm

def basisGenerator {n : Nat} (j : Fin n) : Fin n → Int :=
  fun i => if i = j then 1 else 0

theorem basisExpansion
    {n : Nat} (v : Fin n → Int) :
    (∑ i, v i • basisGenerator i) = v := by
  funext j
  classical
  have hsum : (∑ i, (v i • basisGenerator i) j) = v j := by
    rw [Finset.sum_eq_single j]
    · simp [basisGenerator]
    · intro x _ hx
      have hx' : j ≠ x := by simpa [eq_comm] using hx
      simp [basisGenerator, Pi.smul_apply, hx']
    · simp [basisGenerator]
  simpa [Pi.smul_apply] using hsum

theorem pairBasisExpansion
    {n : Nat} (v : Fin n → Int) :
    (∑ i, (v i • basisGenerator i, (0 : Int))) = (v, 0) := by
  apply Prod.ext
  ·
    have hfst :
        (∑ i, (v i • basisGenerator i, (0 : Int))).1 = ∑ i, v i • basisGenerator i := by
      simpa using
        (Prod.fst_sum (s := Finset.univ) (f := fun i => (v i • basisGenerator i, (0 : Int))))
    rw [hfst, basisExpansion]
  ·
    have hsnd : (∑ i, (v i • basisGenerator i, (0 : Int))).2 = (0 : Int) := by
      simpa using
        (Prod.snd_sum (s := Finset.univ) (f := fun i => (v i • basisGenerator i, (0 : Int))))
    exact hsnd

def basisBlind {n : Nat} : (Fin n → Int) × Int :=
  (fun _ => 0, 1)

def demoParams (n : Nat) : Params n ((Fin n → Int) × Int) where
  generators := fun i => (basisGenerator i, 0)
  blindingGenerator := basisBlind

theorem demo_commitVector_eq
    {n : Nat} (v : Fin n → Int) :
    commitVector (demoParams n) v = (v, 0) := by
  simpa [commitVector, demoParams] using pairBasisExpansion v

theorem demo_pedersenCommit_eq
    {n : Nat} (v : Fin n → Int) (r : Int) :
    pedersenCommit (demoParams n) v r = (v, r) := by
  rw [pedersenCommit, demo_commitVector_eq]
  ext j
  · simp [demoParams, basisBlind]
  · simp [demoParams, basisBlind]

theorem demo_binding (n : Nat) :
    Binding (demoParams n) := by
  intro vr₁ vr₂ h
  rcases vr₁ with ⟨v₁, r₁⟩
  rcases vr₂ with ⟨v₂, r₂⟩
  have hEq :
      (v₁, r₁) = (v₂, r₂) := by
    calc
      (v₁, r₁) = pedersenCommit (demoParams n) v₁ r₁ := (demo_pedersenCommit_eq v₁ r₁).symm
      _ = pedersenCommit (demoParams n) v₂ r₂ := h
      _ = (v₂, r₂) := demo_pedersenCommit_eq v₂ r₂
  simpa using hEq

theorem verificationConsistencyAt_of_openCorrect
    {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G) :
    Vec.VerificationConsistencyAt (scheme P) := by
  exact Vec.verificationConsistencyAt_of_openCorrect (scheme P) (openCorrect P)

theorem demo_verificationConsistencyAt (n : Nat) :
    Vec.VerificationConsistencyAt (scheme (demoParams n)) := by
  exact verificationConsistencyAt_of_openCorrect (demoParams n)

def securityProps
    {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G) :
    Vec.SecurityProps (scheme P) where
  bindingAt := Binding P
  verificationConsistencyAt := Vec.VerificationConsistencyAt (scheme P)
  computationalHidingAt := ComputationalHiding P n
  extractable := False

theorem computationalHiding_of_dlogReduction
    {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G)
    (hHard : DLogHardness P)
    (hRed : DLogReductionStatement P n) :
    ComputationalHiding P n := by
  exact computationalHiding_of_dlog hHard hRed

theorem computationalHiding_field_of_dlogReduction
    {n : Nat} {G : Type} [AddCommGroup G]
    (P : Params n G)
    (hHard : DLogHardness P)
    (hRed : DLogReductionStatement P n) :
    (securityProps P).computationalHidingAt := by
  exact computationalHiding_of_dlogReduction P hHard hRed

def demoSecurityProps (n : Nat) :
    Vec.SecurityProps (scheme (demoParams n)) where
  bindingAt := Binding (demoParams n)
  verificationConsistencyAt := Vec.VerificationConsistencyAt (scheme (demoParams n))
  computationalHidingAt := False
  extractable := False

end IPAInstance
end Commit
end Crypto
end NucleusDB
end HeytingLean
