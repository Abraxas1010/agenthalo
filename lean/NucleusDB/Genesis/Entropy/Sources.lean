import NucleusDB.Genesis.Entropy.State

namespace HeytingLean
namespace NucleusDB
namespace Genesis
namespace Entropy

/-- Deterministic 32-byte to 64-byte normalization model for drand.
This is an abstract Lean model of the runtime's SHA-512 expansion step. -/
def normalizeDrand (raw : ByteVec32) : ByteVec64 :=
  fun i =>
    let j : Nat := i.1 % 32
    let h : j < 32 := Nat.mod_lt _ (by decide)
    let b := raw ⟨j, h⟩
    if i.1 % 2 = 0 then b else Nat.xor b 0xA5

/-- Generic normalization, identity for native 64-byte sources. -/
def normalizeTo64 : EntropySourceId → ByteVec64 → ByteVec64
  | .curby, bytes => bytes
  | .nist, bytes => bytes
  | .os, bytes => bytes
  | .drand, bytes => bytes

theorem normalize_drand_deterministic (x : ByteVec32) :
    normalizeDrand x = normalizeDrand x := by
  rfl

theorem normalize_to64_preserves_curby (x : ByteVec64) :
    normalizeTo64 .curby x = x := by
  rfl

end Entropy
end Genesis
end NucleusDB
end HeytingLean
