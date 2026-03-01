import NucleusDB.Genesis.Entropy.State

namespace HeytingLean
namespace NucleusDB
namespace Genesis
namespace Entropy

/-- Pointwise XOR over fixed-width byte vectors. -/
def xorVec64 (a b : ByteVec64) : ByteVec64 :=
  fun i => Nat.xor (a i) (b i)

/-- Canonical XOR fold used by the runtime combiner. -/
def combineXor (samples : List ByteVec64) : ByteVec64 :=
  samples.foldl xorVec64 (fun _ => 0)

theorem xorVec64_comm (a b : ByteVec64) :
    xorVec64 a b = xorVec64 b a := by
  funext i
  exact Nat.xor_comm (a i) (b i)

theorem xorVec64_assoc (a b c : ByteVec64) :
    xorVec64 (xorVec64 a b) c = xorVec64 a (xorVec64 b c) := by
  funext i
  exact Nat.xor_assoc (a i) (b i) (c i)

theorem combineXor_deterministic (xs : List ByteVec64) :
    combineXor xs = combineXor xs := by
  rfl

end Entropy
end Genesis
end NucleusDB
end HeytingLean
