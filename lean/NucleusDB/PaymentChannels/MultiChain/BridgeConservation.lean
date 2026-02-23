import Mathlib
import NucleusDB.PaymentChannels.Cuts
import NucleusDB.PaymentChannels.MultiChain.Graph

namespace NucleusDB
namespace PaymentChannels
namespace MultiChain

open Sym2

/-!
# NucleusDB.PaymentChannels.MultiChain.BridgeConservation

Bridge-level conservation laws and bridge-cut accounting.
-/

universe u

structure BridgeState where
  chainA : ChainId
  chainB : ChainId
  lockedA : Cap
  lockedB : Cap
  totalCapacity : Cap
  conservation : lockedA + lockedB = totalCapacity

/-- Moving `delta` liquidity from side A to side B preserves total bridge capacity. -/
theorem bridge_transfer_conserves
    (bs : BridgeState) (delta : Cap)
    (hDelta : delta ≤ bs.lockedA) :
    (bs.lockedA - delta) + (bs.lockedB + delta) = bs.totalCapacity := by
  calc
    (bs.lockedA - delta) + (bs.lockedB + delta)
        = (bs.lockedA - delta) + (delta + bs.lockedB) := by
            simp [Nat.add_assoc, Nat.add_comm]
    _ = ((bs.lockedA - delta) + delta) + bs.lockedB := by
          simp [Nat.add_assoc]
    _ = bs.lockedA + bs.lockedB := by
      simp [Nat.sub_add_cancel hDelta]
    _ = bs.totalCapacity := bs.conservation

variable {V : Type u} [DecidableEq V]

/-- Sum of capacities on bridge edges crossing cut `S`. -/
def bridgeCutCapacity (G : MultiChainGraph V) (S : Finset (ChainAddress V)) : Cap :=
  ∑ e ∈ G.edges,
    match G.edgeType e with
    | .bridge _ _ => if Cuts.IsCut S e then G.cap e else 0
    | .channel _ => 0

end MultiChain
end PaymentChannels
end NucleusDB
