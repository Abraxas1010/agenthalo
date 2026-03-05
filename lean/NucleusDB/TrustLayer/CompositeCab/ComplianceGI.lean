import Mathlib.Order.Closure
import NucleusDB.TrustLayer.CompositeCab.NucleusCompliance

/-!
# NucleusDB.TrustLayer.CompositeCab.ComplianceGI

Galois-insertion surface for compliance-state closure.

This transfers the closure/Galois-insertion theorem family from the
NucleusPOD abstraction into the concrete `ComplianceState` nucleus.
-/

namespace NucleusDB.TrustLayer.CompositeCab

/-- Closure operator induced by the composite-CAB verification nucleus. -/
abbrev complianceClosure : ClosureOperator ComplianceState :=
  verificationNucleus.toClosureOperator

/-- Galois insertion for closed compliance states. -/
abbrev complianceGaloisInsertion :
    GaloisInsertion complianceClosure.toCloseds (↑) :=
  complianceClosure.gi

/-- If `t` is closed, closure over `s` lies below `t` exactly when `s ≤ t`. -/
theorem compliance_closure_le_iff_le_of_closed {s t : ComplianceState}
    (ht : complianceClosure t = t) :
    complianceClosure s ≤ t ↔ s ≤ t := by
  have htClosed : complianceClosure.IsClosed t :=
    (complianceClosure.isClosed_iff).2 ht
  exact htClosed.closure_le_iff

end NucleusDB.TrustLayer.CompositeCab
