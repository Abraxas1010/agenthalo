import NucleusDB.Contracts.Model
import NucleusDB.PaymentChannels.EVMAdapter.Extractor
import NucleusDB.PaymentChannels.EVMAdapter.SettlementOps

namespace NucleusDB
namespace PaymentChannels
namespace EVMAdapter

open Sym2
open NucleusDB.Contracts.Model

/-!
# Blockchain.PaymentChannels.EVMAdapter.SettlementSemantics

Concrete transition semantics for a minimal on-chain settlement contract, operating on `EVMState`.

This implements a *single-contract* step function that can open/close/splice channels in the
designated channel contract storage, and updates account balances to model funds being locked/unlocked.
-/

namespace SettlementSemantics

abbrev Call := SettlementOps.Call

inductive Error where
  | invalidAddress (addr : Address)
  | loopNotAllowed
  | alreadyOpen
  | channelMissing
  | channelNotOpen
  | accessDenied (caller : Address)
  | insufficientFunds
  | invalidCapacity
  deriving DecidableEq, Repr

structure Tx where
  caller : Address
  call : Call
  deriving DecidableEq, Repr

def isValidAddress (a : Address) : Prop :=
  match addrVal? a with
  | none => False
  | some n => n ≠ 0 ∧ addrOfVal n = a

instance (a : Address) : Decidable (isValidAddress a) := by
  unfold isValidAddress
  cases h : addrVal? a with
  | none =>
      exact isFalse (by intro hValid; cases hValid)
  | some n =>
      infer_instance

def findChannelId? (cfg : ExtractorConfig) (s : EVMState) (u v : Address) : Option Nat :=
  let e := Sym2.mk (u, v)
  (List.range (s.storage cfg.registryContract registryLenSlot)).find? fun channelId =>
    match getChannelRecord s cfg.channelContract channelId with
    | none => False
    | some r => Extractor.edgeOfRecord r = e

def findOpenChannelId? (cfg : ExtractorConfig) (s : EVMState) (u v : Address) : Option Nat :=
  let e := Sym2.mk (u, v)
  (List.range (s.storage cfg.registryContract registryLenSlot)).find? fun channelId =>
    match getChannelRecord s cfg.channelContract channelId with
    | none => False
    | some r => r.status = .Open ∧ Extractor.edgeOfRecord r = e

def getOpenChannelRecord? (cfg : ExtractorConfig) (s : EVMState) (u v : Address) : Option (Nat × ChannelRecord) :=
  match findOpenChannelId? cfg s u v with
  | none => none
  | some channelId =>
      match getChannelRecord s cfg.channelContract channelId with
      | none => none
      | some r =>
          if r.status = .Open ∧ Extractor.edgeOfRecord r = Sym2.mk (u, v) then
            some (channelId, r)
          else
            none

def callerHasFunds (s : EVMState) (caller : Address) (amount : Wei) : Prop :=
  amount ≤ (s.accounts caller).balance

instance (s : EVMState) (caller : Address) (amount : Wei) : Decidable (callerHasFunds s caller amount) := by
  unfold callerHasFunds
  infer_instance

def settlementStep (cfg : ExtractorConfig) (caller : Address) (s : EVMState) :
    Call → Except Error EVMState
  | .open u v cap =>
      if ¬ isValidAddress u then
        .error (.invalidAddress u)
      else if ¬ isValidAddress v then
        .error (.invalidAddress v)
      else if u = v then
        .error .loopNotAllowed
      else if cap = 0 then
        .error .invalidCapacity
      else if (Sym2.mk (u, v)) ∈ (extractChannelGraph cfg s).edges then
        .error .alreadyOpen
      else if caller ≠ u ∧ caller ≠ v then
        .error (.accessDenied caller)
      else if ¬ callerHasFunds s caller cap then
        .error .insufficientFunds
      else
        let channelId : Nat := s.storage cfg.registryContract registryLenSlot
        let s₁ := setRegistryLength (state := s) cfg.registryContract (channelId + 1)
        let r : ChannelRecord :=
          { participant1 := u
            participant2 := v
            capacity := cap
            status := .Open }
        let s₂ := putChannelRecord s₁ cfg.channelContract channelId r
        let s₃ := EVMState.updateBalance s₂ caller (fun b => b - cap)
        .ok s₃
  | .close u v =>
      match getOpenChannelRecord? cfg s u v with
      | none => .error .channelMissing
      | some (channelId, r) =>
          if caller ≠ r.participant1 ∧ caller ≠ r.participant2 then
            .error (.accessDenied caller)
          else
            let r' : ChannelRecord := { r with status := .Closed }
            let s₁ := putChannelRecord s cfg.channelContract channelId r'
            let s₂ := EVMState.updateBalance s₁ caller (fun b => b + r.capacity)
            .ok s₂
  | .splice u v newCap =>
      match getOpenChannelRecord? cfg s u v with
      | none => .error .channelMissing
      | some (channelId, r) =>
          if caller ≠ r.participant1 ∧ caller ≠ r.participant2 then
            .error (.accessDenied caller)
          else
            if newCap = 0 then
              .error .invalidCapacity
            else
            -- Balance delta: if increasing capacity, caller must fund the increase; if decreasing, caller is refunded.
            let oldCap := r.capacity
            if oldCap ≤ newCap then
              let delta := newCap - oldCap
              if ¬ callerHasFunds s caller delta then
                .error .insufficientFunds
              else
                let r' : ChannelRecord := { r with capacity := newCap }
                let s₁ := putChannelRecord s cfg.channelContract channelId r'
                let s₂ := EVMState.updateBalance s₁ caller (fun b => b - delta)
                .ok s₂
            else
              let delta := oldCap - newCap
              let r' : ChannelRecord := { r with capacity := newCap }
              let s₁ := putChannelRecord s cfg.channelContract channelId r'
              let s₂ := EVMState.updateBalance s₁ caller (fun b => b + delta)
              .ok s₂

def step (cfg : ExtractorConfig) (s : EVMState) (tx : Tx) : Except Error EVMState :=
  settlementStep cfg tx.caller s tx.call

def model (cfg : ExtractorConfig) : ContractModel EVMState Tx Error :=
  { init := EVMState.empty
    step := step cfg }

end SettlementSemantics

end EVMAdapter
end PaymentChannels
end NucleusDB
