import NucleusDB.Sheaf.ChainTransport

/-!
# NucleusDB.Sheaf.ChainGluing

Standalone gluing condition and glue witness over chain-indexed transport.
-/

namespace NucleusDB.Sheaf

open NucleusDB.PaymentChannels.MultiChain

universe u v

/-- Gluing condition: two chain-local sections agree after projection to shared space. -/
def ChainGluingCondition {Carrier : ChainId → Type u} {Shared : Type v}
    (T : ChainTransport Carrier Shared)
    (c1 c2 : ChainId) (x : Carrier c1) (y : Carrier c2) : Prop :=
  T.toShared c1 x = T.toShared c2 y

/-- Glue two local sections by choosing their common shared value. -/
noncomputable def chainGlue {Carrier : ChainId → Type u} {Shared : Type v}
    (T : ChainTransport Carrier Shared)
    (c1 c2 : ChainId) (x : Carrier c1) (y : Carrier c2)
    (_h : ChainGluingCondition T c1 c2 x y) : Shared :=
  T.toShared c1 x

theorem chainGlue_spec {Carrier : ChainId → Type u} {Shared : Type v}
    (T : ChainTransport Carrier Shared)
    (c1 c2 : ChainId) (x : Carrier c1) (y : Carrier c2)
    (h : ChainGluingCondition T c1 c2 x y) :
    chainGlue T c1 c2 x y h = T.toShared c2 y := by
  simpa [chainGlue, ChainGluingCondition] using h

end NucleusDB.Sheaf
