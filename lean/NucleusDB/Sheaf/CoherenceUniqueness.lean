import NucleusDB.Sheaf.Coherence

/-!
# NucleusDB.Sheaf.CoherenceUniqueness

Additional coherence results transferred from the NucleusPOD sheaf family:

- restricted amalgamation uniqueness
- transport exactness (`forward` then inverse `forward`)
-/

namespace HeytingLean
namespace NucleusDB
namespace Sheaf

open _root_.NucleusDB.PaymentChannels.MultiChain

universe u

/-- Restricted amalgamation uniqueness:
if two candidates match all sections under `restrict`, they agree under `restrict`.

Note: this is uniqueness after applying `restrict`, not `a1 = a2`; full uniqueness
would require injectivity of `LensPresheaf.restrict`, which is intentionally
not assumed in this model.
-/
theorem amalgamation_unique {A : Type u}
    (w : CoherenceWitness A)
    (hNonEmpty : w.family.sections ≠ [])
    (a1 a2 : A)
    (h1 : ∀ s ∈ w.family.sections, w.F.restrict s = w.F.restrict a1)
    (h2 : ∀ s ∈ w.family.sections, w.F.restrict s = w.F.restrict a2) :
    w.F.restrict a1 = w.F.restrict a2 := by
  rcases List.exists_mem_of_ne_nil w.family.sections hNonEmpty with ⟨s, hs⟩
  calc
    w.F.restrict a1 = w.F.restrict s := by
      symm
      exact h1 s hs
    _ = w.F.restrict a2 := h2 s hs

/-- Exact transport round-trip through shared representation. -/
theorem transport_forward_backward {A : Type u}
    (T : _root_.NucleusDB.Sheaf.ChainTransport (fun _ : ChainId => A) A)
    (src dst : ChainId)
    (x : A) :
    _root_.NucleusDB.Sheaf.ChainTransport.forward T dst src
      (_root_.NucleusDB.Sheaf.ChainTransport.forward T src dst x) = x := by
  dsimp [_root_.NucleusDB.Sheaf.ChainTransport.forward]
  rw [T.rt2 dst (T.toShared src x), T.rt1 src x]

end Sheaf
end NucleusDB
end HeytingLean
