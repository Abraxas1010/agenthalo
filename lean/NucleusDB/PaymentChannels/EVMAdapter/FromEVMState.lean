import NucleusDB.Contracts.Model
import NucleusDB.PaymentChannels.Graph

namespace NucleusDB
namespace PaymentChannels
namespace EVMAdapter

/-!
# Blockchain.PaymentChannels.EVMAdapter.FromEVMState

Thin integration seam between an (eventual) EVM semantics and the PCN geometry layer.

At this stage we only fix:
- the vertex type for extracted graphs (`Address := String`); and
- an abstract extractor `EVMState → ChannelGraph Address`.

No concrete EVM operational semantics is assumed here.
-/

open NucleusDB.Contracts.Model

/-- Vertices in the extracted PCN graph: on-chain addresses. -/
abbrev Vertex := NucleusDB.Contracts.Model.Address

/-- An adapter that extracts a PCN channel graph from an abstract EVM state. -/
structure Adapter (EVMState : Type) where
  ChannelGraphOfEVMState : EVMState → ChannelGraph Vertex

end EVMAdapter
end PaymentChannels
end NucleusDB

