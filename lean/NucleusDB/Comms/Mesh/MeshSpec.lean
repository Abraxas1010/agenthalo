/-!
# Mesh Network Formal Specification

T21: Mesh connectivity — if a connection exists, both peers are reachable.
Mesh reachability is symmetric (undirected graph semantics).
-/

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Mesh

/-- Peer identity on the mesh network. -/
structure PeerId where
  agentId : String
  didUri : Option String
  deriving DecidableEq, Repr

/-- Mesh network as a graph of peers with undirected connections. -/
structure MeshNetwork where
  peers : List PeerId
  connections : List (PeerId × PeerId)
  deriving Repr

/-- A peer is reachable if there exists a connection in either direction. -/
def reachable (net : MeshNetwork) (src dst : PeerId) : Prop :=
  (src, dst) ∈ net.connections ∨ (dst, src) ∈ net.connections

/-- Mesh communication is symmetric: if A can reach B, B can reach A. -/
theorem mesh_reachability_symmetric (net : MeshNetwork) (a b : PeerId) :
    reachable net a b → reachable net b a := by
  intro h
  cases h with
  | inl h => exact Or.inr h
  | inr h => exact Or.inl h

/-- T21: A peer on the mesh can reach all other connected peers. -/
theorem mesh_connectivity (net : MeshNetwork) (a b : PeerId)
    (h : (a, b) ∈ net.connections) : reachable net a b :=
  Or.inl h

/-- Reachability is reflexive when a self-loop exists. -/
theorem mesh_reachability_reflexive (net : MeshNetwork) (a : PeerId)
    (h : (a, a) ∈ net.connections) : reachable net a a :=
  Or.inl h

/-- If both (a, b) and (b, c) are in connections, both pairs are reachable. -/
theorem mesh_two_hop_reachable (net : MeshNetwork) (a b c : PeerId)
    (hab : (a, b) ∈ net.connections) (hbc : (b, c) ∈ net.connections) :
    reachable net a b ∧ reachable net b c :=
  ⟨Or.inl hab, Or.inl hbc⟩

/-- Adding a connection preserves existing reachability. -/
theorem mesh_add_connection_preserves (net : MeshNetwork) (a b x y : PeerId)
    (h : reachable net a b) :
    reachable { net with connections := (x, y) :: net.connections } a b := by
  cases h with
  | inl h => exact Or.inl (List.mem_cons_of_mem _ h)
  | inr h => exact Or.inr (List.mem_cons_of_mem _ h)

/-- Adding a connection makes the new endpoints reachable. -/
theorem mesh_add_connection_reaches (net : MeshNetwork) (x y : PeerId) :
    reachable { net with connections := (x, y) :: net.connections } x y := by
  left
  exact List.Mem.head _

/-- An empty network has no reachable pairs. -/
theorem mesh_empty_unreachable (a b : PeerId) :
    ¬ reachable { peers := [], connections := [] } a b := by
  intro h
  cases h with
  | inl h => exact nomatch h
  | inr h => exact nomatch h

/-- Peer membership: a peer registered in the network. -/
def peerRegistered (net : MeshNetwork) (p : PeerId) : Prop :=
  p ∈ net.peers

/-- A connected peer must be registered for well-formed networks. -/
def wellFormedConnections (net : MeshNetwork) : Prop :=
  ∀ p q, (p, q) ∈ net.connections →
    peerRegistered net p ∧ peerRegistered net q

/-- In a well-formed network, reachable peers are registered. -/
theorem reachable_peers_registered (net : MeshNetwork)
    (hwf : wellFormedConnections net) (a b : PeerId)
    (h : reachable net a b) :
    peerRegistered net a ∧ peerRegistered net b := by
  cases h with
  | inl h =>
    exact hwf a b h
  | inr h =>
    have ⟨hb, ha⟩ := hwf b a h
    exact ⟨ha, hb⟩

end Mesh
end Comms
end NucleusDB
end HeytingLean
