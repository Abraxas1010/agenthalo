namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Privacy

/-- Abstract native mixnet runtime state exported by nym_native status. -/
structure NativeMixnetState where
  enabled : Bool
  connected : Bool
  inboundRegistered : Bool
  coverTrafficActive : Bool
  deriving DecidableEq, Repr

/-- Native outbound messaging is permitted only when connected. -/
def nativeSendAllowed (s : NativeMixnetState) : Prop :=
  s.connected = true

/-- SURB reply availability is gated by the same connection predicate. -/
def surbReplyAllowed (s : NativeMixnetState) : Prop :=
  s.connected = true

/-- If native transport is disabled, native send is not allowed. -/
theorem disabled_native_blocks_send
    (s : NativeMixnetState) :
    s.connected = false → ¬ nativeSendAllowed s := by
  intro hConn hAllowed
  unfold nativeSendAllowed at hAllowed
  rw [hConn] at hAllowed
  cases hAllowed

/-- A connected native state permits outbound send. -/
theorem connected_native_allows_send
    (s : NativeMixnetState) (h : s.connected = true) :
    nativeSendAllowed s := by
  simpa [nativeSendAllowed] using h

/-- SURB reply and outbound send are equivalent gates in the runtime model. -/
theorem surb_reply_gate_equivalent_to_send_gate
    (s : NativeMixnetState) :
    surbReplyAllowed s ↔ nativeSendAllowed s := by
  unfold surbReplyAllowed nativeSendAllowed
  rfl

/-- Runtime invariant exported by the native transport state machine. -/
def nativeStateInvariant (s : NativeMixnetState) : Prop :=
  s.coverTrafficActive = true → s.enabled = true

/-- Under the runtime invariant, active cover traffic implies native enabled. -/
theorem cover_traffic_requires_enabled
    (s : NativeMixnetState)
    (hInv : nativeStateInvariant s)
    (hCover : s.coverTrafficActive = true) :
    s.enabled = true := by
  exact hInv hCover

end Privacy
end Comms
end NucleusDB
end HeytingLean
