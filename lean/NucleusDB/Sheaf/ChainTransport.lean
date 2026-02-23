import NucleusDB.PaymentChannels.MultiChain.Graph

/-!
# NucleusDB.Sheaf.ChainTransport

Standalone chain-indexed transport primitives for sheaf-style gluing.
-/

namespace NucleusDB.Sheaf

open NucleusDB.PaymentChannels.MultiChain

universe u v

/-- Per-chain carrier family with a shared comparison type and round-trip contracts. -/
structure ChainTransport (Carrier : ChainId → Type u) (Shared : Type v) where
  toShared : ∀ chain, Carrier chain → Shared
  fromShared : ∀ chain, Shared → Carrier chain
  /-- RT-1: round-trip identity on chain-local states. -/
  rt1 : ∀ chain (x : Carrier chain), fromShared chain (toShared chain x) = x
  /-- RT-2: projecting decoded data back to `Shared` is identity. -/
  rt2 : ∀ chain (s : Shared), toShared chain (fromShared chain s) = s

namespace ChainTransport

variable {Carrier : ChainId → Type u} {Shared : Type v}

/-- Forward transport between any two chains via the shared projection. -/
def forward (T : ChainTransport Carrier Shared) (src dst : ChainId) :
    Carrier src → Carrier dst :=
  fun x => T.fromShared dst (T.toShared src x)

/-- Backward transport between any two chains via the shared projection. -/
def backward (T : ChainTransport Carrier Shared) (src dst : ChainId) :
    Carrier dst → Carrier src :=
  fun y => T.fromShared src (T.toShared dst y)

theorem backward_forward (T : ChainTransport Carrier Shared)
    (src dst : ChainId) (x : Carrier src) :
    backward T src dst (forward T src dst x) = x := by
  dsimp [backward, forward]
  rw [T.rt2 dst (T.toShared src x), T.rt1 src x]

end ChainTransport

end NucleusDB.Sheaf
