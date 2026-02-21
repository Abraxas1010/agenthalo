import NucleusDB.Core.Certificates

namespace HeytingLean
namespace NucleusDB
namespace Security

/-- Abstract commit record in the semantic model. -/
structure AbstractCommit where
  height : Nat
  prevRoot : Nat
  nextRoot : Nat

/-- Concrete commit record emitted by runtime/logging layers. -/
structure ConcreteCommit where
  height : Nat
  prevRoot : Nat
  nextRoot : Nat
  sthSize : Nat

/-- Refinement relation from concrete records to abstract semantics. -/
def Refines (a : AbstractCommit) (c : ConcreteCommit) : Prop :=
  c.height = a.height ∧
  c.prevRoot = a.prevRoot ∧
  c.nextRoot = a.nextRoot ∧
  c.sthSize = c.height

theorem refines_height
    {a : AbstractCommit} {c : ConcreteCommit}
    (h : Refines a c) :
    c.height = a.height :=
  h.1

theorem refines_sth_matches_height
    {a : AbstractCommit} {c : ConcreteCommit}
    (h : Refines a c) :
    c.sthSize = c.height :=
  h.2.2.2

/-- Soundness bridge: certificate-valid transitions can be exported with a refinement witness. -/
theorem certificate_to_refinement
    {State Delta Auth : Type}
    {apply : State → Delta → State}
    {policy : Core.AuthorizationPolicy State Delta Auth}
    (cert : Core.CommitCertificate State Delta Auth apply policy)
    (height prevRoot nextRoot : Nat) :
    let a : AbstractCommit := {
      height := height
      prevRoot := prevRoot
      nextRoot := nextRoot
    }
    let c : ConcreteCommit := {
      height := height
      prevRoot := prevRoot
      nextRoot := nextRoot
      sthSize := height
    }
    Core.verifyCommitCertificate cert → Refines a c := by
  intro a c _
  exact ⟨rfl, rfl, rfl, rfl⟩

end Security
end NucleusDB
end HeytingLean
