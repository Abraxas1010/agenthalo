namespace HeytingLean
namespace NucleusDB
namespace Adversarial

universe u v w

/-- Runtime witness signature algorithms used by NucleusDB. -/
inductive SignatureAlgorithm
  | ed25519
  | mlDsa65
deriving DecidableEq, Repr

/-- Witness-authorization model with abstract signature verification. -/
structure WitnessModel (Witness : Type u) (Msg : Type v) (Sig : Type w) where
  verify : Witness → Msg → Sig → Prop
  threshold : Nat

/-- Signature set over witness identities. -/
structure SigEntry (Witness : Type u) (Sig : Type v) where
  who : Witness
  sig : Sig

/-- Quorum predicate: enough signature entries are present. -/
def quorum
    {Witness : Type u} {Msg : Type v} {Sig : Type w}
    (M : WitnessModel Witness Msg Sig)
    (msg : Msg)
    (entries : List (SigEntry Witness Sig)) : Prop :=
  M.threshold ≤ entries.length ∧
    ∀ e ∈ entries, M.verify e.who msg e.sig

/-- Simple runtime migration window model for witness signature algorithms. -/
def acceptsDuringRotation
    (oldAlg newAlg active : SignatureAlgorithm)
    (duringWindow : Bool) : Prop :=
  if duringWindow then active = oldAlg ∨ active = newAlg else active = newAlg

theorem acceptsDuringRotation_outside
    (oldAlg newAlg active : SignatureAlgorithm)
    (h : acceptsDuringRotation oldAlg newAlg active false) :
    active = newAlg := by
  simpa [acceptsDuringRotation] using h

end Adversarial
end NucleusDB
end HeytingLean
