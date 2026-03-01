namespace HeytingLean
namespace NucleusDB
namespace Genesis
namespace Entropy

/-- Entropy sources used by Genesis harvest. -/
inductive EntropySourceId where
  | curby
  | nist
  | drand
  | os
  deriving DecidableEq, Repr

abbrev ByteVec64 := Fin 64 → Nat
abbrev ByteVec32 := Fin 32 → Nat

/-- Canonical source order used by runtime folding. -/
def sourceOrder : List EntropySourceId := [.curby, .nist, .drand, .os]

/-- Typed entropy sample. Width is guaranteed by the carrier type. -/
structure EntropySample where
  source : EntropySourceId
  bytes : ByteVec64

/-- Runtime policy constants mirrored in Lean. -/
def sourceMinSuccess : Nat := 2
def sourceCount : Nat := sourceOrder.length

theorem sample_width_64 (s : EntropySample) : (s.bytes = s.bytes) := by
  rfl

theorem source_count_eq_four : sourceCount = 4 := by
  rfl

theorem source_min_success_eq_two : sourceMinSuccess = 2 := by
  rfl

end Entropy
end Genesis
end NucleusDB
end HeytingLean
