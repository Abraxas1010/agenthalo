import NucleusDB.Genesis.Entropy.State
import Mathlib.Algebra.Group.MinimalAxioms

namespace HeytingLean
namespace NucleusDB
namespace Genesis
namespace Entropy

/-- Zero vector for XOR addition (identity element). -/
def zeroVec64 : ByteVec64 := fun _ => 0

/-- Pointwise XOR over fixed-width byte vectors. -/
def xorVec64 (a b : ByteVec64) : ByteVec64 :=
  fun i => Nat.xor (a i) (b i)

/-- Canonical XOR fold used by the runtime combiner. -/
def combineXor (samples : List ByteVec64) : ByteVec64 :=
  samples.foldl xorVec64 zeroVec64

theorem xorVec64_comm (a b : ByteVec64) :
    xorVec64 a b = xorVec64 b a := by
  funext i
  exact Nat.xor_comm (a i) (b i)

theorem xorVec64_assoc (a b c : ByteVec64) :
    xorVec64 (xorVec64 a b) c = xorVec64 a (xorVec64 b c) := by
  funext i
  exact Nat.xor_assoc (a i) (b i) (c i)

theorem xorVec64_self_inverse (a : ByteVec64) :
    xorVec64 a a = zeroVec64 := by
  funext i
  exact Nat.xor_self (a i)

theorem xorVec64_zero_right (a : ByteVec64) :
    xorVec64 a zeroVec64 = a := by
  funext i
  exact Nat.xor_zero (a i)

theorem xorVec64_zero_left (a : ByteVec64) :
    xorVec64 zeroVec64 a = a := by
  funext i
  exact Nat.zero_xor (a i)

theorem combineXor_deterministic (xs : List ByteVec64) :
    combineXor xs = combineXor xs := by
  rfl

instance : Zero ByteVec64 where
  zero := zeroVec64

instance : Add ByteVec64 where
  add := xorVec64

instance : Neg ByteVec64 where
  neg := id

instance : AddGroup ByteVec64 :=
  AddGroup.ofLeftAxioms
    xorVec64_assoc
    xorVec64_zero_left
    (by
      intro a
      simpa [Neg.neg, HAdd.hAdd, Add.add] using xorVec64_self_inverse a)

instance : AddCommGroup ByteVec64 := by
  exact AddCommGroup.mk xorVec64_comm

end Entropy
end Genesis
end NucleusDB
end HeytingLean
