import NucleusDB.Core.EpistemicTrust
import NucleusDB.Core.EvidenceFusion

namespace HeytingLean
namespace NucleusDB
namespace Core
namespace NucleusBridge

/-- Local bridge name for the epistemic-trust floor theorem. -/
theorem nucleus_combine_floor_bound
    (t : Core.EpistemicTrust) (values : List Rat) :
    t.floor ≤ Core.combine t values := by
  exact Core.combine_floor_respected t values

/-- Local bridge name for the order-independence of Bayesian evidence chaining. -/
theorem vUpdate_chain_comm
    (priorOddsFalseOverTrue : Rat)
    {left right : List Core.EvidenceLikelihood}
    (hperm : List.Perm left right) :
    Core.combineEvidence priorOddsFalseOverTrue left =
      Core.combineEvidence priorOddsFalseOverTrue right := by
  exact Core.combineEvidence_comm priorOddsFalseOverTrue hperm

end NucleusBridge
end Core
end NucleusDB
end HeytingLean
