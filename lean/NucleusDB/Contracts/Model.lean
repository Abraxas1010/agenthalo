/-!
# NucleusDB.Contracts.Model

Minimal contract-model surface required by the payment-channel EVM adapter.
This file intentionally keeps only the address carrier needed by the adapter.
-/

namespace NucleusDB
namespace Contracts
namespace Model

/-- Abstract on-chain address type used by the PCN EVM adapter. -/
abbrev Address := String

end Model
end Contracts
end NucleusDB
