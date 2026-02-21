namespace HeytingLean
namespace NucleusDB
namespace Transparency

/-- Signed tree head model (abstracted root/signature payloads). -/
structure STH where
  size : Nat
  root : String
  sig : String

/-- Commit leaf tracked in append-only transparency log. -/
structure CommitLeaf where
  height : Nat
  prevStateRoot : String
  stateRoot : String
  deltaDigest : String
  certDigest : String
  sheafCoherenceDigest : String

/-- Extension relation over signed tree heads. -/
def Extends (old new : STH) : Prop :=
  old.size ≤ new.size

theorem extends_refl (h : STH) : Extends h h :=
  Nat.le_refl h.size

theorem extends_trans {a b c : STH} :
    Extends a b → Extends b c → Extends a c :=
  Nat.le_trans

end Transparency
end NucleusDB
end HeytingLean
