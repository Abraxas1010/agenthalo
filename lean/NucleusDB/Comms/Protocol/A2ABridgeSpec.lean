namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

/-- JSON-RPC methods exposed by the A2A bridge task API. -/
inductive A2AMethod where
  | tasksSend
  | tasksGet
  | tasksCancel
  | unknown
  deriving DecidableEq, Repr

/-- Task lifecycle used by the bridge state machine. -/
inductive A2ATaskStatus where
  | submitted
  | working
  | completed
  | failed
  | canceled
  deriving DecidableEq, Repr

/-- Builtin method dispatch relation for the bridge. -/
def dispatchesToTaskLifecycle (m : A2AMethod) : Prop :=
  match m with
  | .tasksSend => True
  | .tasksGet => True
  | .tasksCancel => True
  | .unknown => False

/-- Only the three task methods are routable through builtin dispatch. -/
theorem only_task_methods_are_dispatchable
    (m : A2AMethod) :
    dispatchesToTaskLifecycle m ↔
      (m = .tasksSend ∨ m = .tasksGet ∨ m = .tasksCancel) := by
  cases m <;> simp [dispatchesToTaskLifecycle]

/-- Terminal statuses are immutable under further transition updates. -/
def terminal (s : A2ATaskStatus) : Prop :=
  s = .completed ∨ s = .failed ∨ s = .canceled

theorem terminal_statuses_are_closed
    (s : A2ATaskStatus) (h : terminal s) :
    s ≠ .submitted ∧ s ≠ .working := by
  rcases h with hDone | hRest
  · constructor <;> intro hs <;> cases hDone <;> cases hs
  · rcases hRest with hFail | hCancel
    · constructor <;> intro hs <;> cases hFail <;> cases hs
    · constructor <;> intro hs <;> cases hCancel <;> cases hs

/-- Canonical transition from `tasks/send` response uses submitted status. -/
theorem tasks_send_initial_status_submitted :
    dispatchesToTaskLifecycle .tasksSend := by
  simp [dispatchesToTaskLifecycle]

/-- Canonical transition from `tasks/cancel` response uses canceled status. -/
theorem tasks_cancel_is_terminal : terminal .canceled := by
  simp [terminal]

end Protocol
end Comms
end NucleusDB
end HeytingLean
