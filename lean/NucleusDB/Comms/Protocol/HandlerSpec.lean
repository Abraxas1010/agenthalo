namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

/-- Built-in message kinds currently handled by the runtime DIDComm handler. -/
inductive MessageKind where
  | ping
  | ack
  | taskSend
  | taskCancel
  | taskStatus
  | agentCardRequest
  | agentCardResponse
  | unsupported
  deriving DecidableEq, Repr

/-- Built-in handler transition relation. `none` means no automatic reply. -/
def builtinResponse : MessageKind → Option MessageKind
  | .ping => some .ack
  | .taskSend => some .taskStatus
  | .taskCancel => some .taskStatus
  | .agentCardRequest => some .agentCardResponse
  | .ack => none
  | .taskStatus => none
  | .agentCardResponse => none
  | .unsupported => none

theorem ping_routes_to_ack :
    builtinResponse .ping = some .ack := by
  rfl

theorem task_send_routes_to_task_status :
    builtinResponse .taskSend = some .taskStatus := by
  rfl

theorem task_cancel_routes_to_task_status :
    builtinResponse .taskCancel = some .taskStatus := by
  rfl

theorem unsupported_has_no_builtin_response :
    builtinResponse .unsupported = none := by
  rfl

end Protocol
end Comms
end NucleusDB
end HeytingLean
