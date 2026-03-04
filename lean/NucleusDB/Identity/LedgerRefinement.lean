import NucleusDB.Identity.LedgerChain

namespace HeytingLean
namespace NucleusDB
namespace Identity

/-- Rust-side witness for `verify_chain` projected into the Lean identity model. -/
structure RustIdentityChainWitness where
  entries : List IdentityLedgerEntrySpec
  deriving DecidableEq, Repr

def rustVerifyChainAccepts (w : RustIdentityChainWitness) : Prop :=
  wellFormedIdentityChain w.entries

theorem rust_verify_chain_refines_spec (w : RustIdentityChainWitness) :
    rustVerifyChainAccepts w ↔ wellFormedIdentityChain w.entries := by
  rfl

end Identity
end NucleusDB
end HeytingLean
