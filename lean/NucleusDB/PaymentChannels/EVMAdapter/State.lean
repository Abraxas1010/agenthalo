import Mathlib.Algebra.BigOperators.Group.Finset.Basic
import NucleusDB.Contracts.Model
import NucleusDB.PaymentChannels.Basic

namespace NucleusDB
namespace PaymentChannels
namespace EVMAdapter

open scoped BigOperators
open NucleusDB.Contracts.Model

/-!
# Blockchain.PaymentChannels.EVMAdapter.State

Minimal concrete EVM state model used for extracting a payment-channel graph.

This is intentionally *not* a full EVM operational semantics: it tracks only
account balances and contract storage, plus minimal block metadata.
For a full EVM formalization reference, see `github.com/NethermindEth/EVMYulLean`.
-/

abbrev Wei := Cap
abbrev StorageKey := Nat
abbrev StorageValue := Nat
abbrev BlockNumber := Nat

structure AccountState where
  balance : Wei
  nonce : Nat := 0
  deriving DecidableEq, Repr

structure EVMState where
  accounts : Address → AccountState
  /-- Contract storage: unset slots are semantically `0`. -/
  storage : Address → StorageKey → StorageValue
  blockNumber : BlockNumber
  timestamp : Nat

namespace EVMState

def empty : EVMState :=
  { accounts := fun _ => { balance := 0, nonce := 0 }
    storage := fun _ _ => 0
    blockNumber := 0
    timestamp := 0 }

def withBalance (s : EVMState) (addr : Address) (bal : Wei) : EVMState :=
  { s with
    accounts := fun a =>
      if a = addr then
        { s.accounts a with balance := bal }
      else
        s.accounts a }

def updateBalance (s : EVMState) (addr : Address) (f : Wei → Wei) : EVMState :=
  withBalance s addr (f (s.accounts addr).balance)

def withStorage (s : EVMState) (contract : Address) (k : StorageKey) (v : StorageValue) : EVMState :=
  { s with
    storage := fun a k' =>
      if a = contract then
        if k' = k then v else s.storage a k'
      else
        s.storage a k' }

def totalBalanceOn (s : EVMState) (addrs : Finset Address) : Wei :=
  ∑ a ∈ addrs, (s.accounts a).balance

end EVMState

/-!
## Channel storage layout
-/

inductive ChannelStatus
  | Open
  | Closed
  deriving DecidableEq, Repr

structure ChannelRecord where
  participant1 : Address
  participant2 : Address
  capacity : Wei
  status : ChannelStatus
  deriving DecidableEq, Repr

namespace ChannelRecord

def statusToVal : ChannelStatus → StorageValue
  | .Open => 0
  | .Closed => 1

def statusOfVal : StorageValue → ChannelStatus
  | 0 => .Open
  | _ => .Closed

end ChannelRecord

/-!
We model EVM storage values as `Nat`. In this lightweight model, addresses stored in channel
records are encoded as a `Nat` and decoded back to an `Address` by printing the number.

This is sufficient for demos where addresses are numeric strings like `"1"`, `"2"`, `"3"`.
-/

def addrVal? (a : Address) : Option StorageValue :=
  a.toNat?

def addrVal (a : Address) : StorageValue :=
  (addrVal? a).getD 0

def addrOfVal (v : StorageValue) : Address :=
  toString v

/-!
### Slot encoding

We use a simple arithmetic scheme rather than Keccak; this is *not* meant to model Solidity ABI.
-/

def channelSlot (channelId : Nat) (field : Nat) : StorageKey :=
  100 + channelId * 10 + field

namespace ChannelFields

def p1 : Nat := 0
def p2 : Nat := 1
def cap : Nat := 2
def status : Nat := 3

end ChannelFields

def getChannelRecord (state : EVMState) (contract : Address) (channelId : Nat) : Option ChannelRecord :=
  let p1v := state.storage contract (channelSlot channelId ChannelFields.p1)
  let p2v := state.storage contract (channelSlot channelId ChannelFields.p2)
  let cap := state.storage contract (channelSlot channelId ChannelFields.cap)
  let st := state.storage contract (channelSlot channelId ChannelFields.status)
  if p1v = 0 ∨ p2v = 0 then
    none
  else
    some
      { participant1 := addrOfVal p1v
        participant2 := addrOfVal p2v
        capacity := cap
        status := ChannelRecord.statusOfVal st }

def putChannelRecord (state : EVMState) (contract : Address) (channelId : Nat) (r : ChannelRecord) : EVMState :=
  state
    |>.withStorage contract (channelSlot channelId ChannelFields.p1) (addrVal r.participant1)
    |>.withStorage contract (channelSlot channelId ChannelFields.p2) (addrVal r.participant2)
    |>.withStorage contract (channelSlot channelId ChannelFields.cap) r.capacity
    |>.withStorage contract (channelSlot channelId ChannelFields.status) (ChannelRecord.statusToVal r.status)

/-!
### Channel registry

We model a registry contract as storing the number of channel IDs that exist, and use the
convention that channel IDs are exactly `0,1,...,(len-1)`.
-/

def registryLenSlot : StorageKey := 0

def channelIds (state : EVMState) (registry : Address) : Finset Nat :=
  Finset.range (state.storage registry registryLenSlot)

def setRegistryLength (state : EVMState) (registry : Address) (n : Nat) : EVMState :=
  state.withStorage registry registryLenSlot n

end EVMAdapter
end PaymentChannels
end NucleusDB
