import Mathlib.Data.Finset.Lattice.Fold
import NucleusDB.PaymentChannels.EVMAdapter.SettlementOps
import NucleusDB.PaymentChannels.EVMAdapter.SettlementSemantics

namespace NucleusDB
namespace PaymentChannels
namespace EVMAdapter

open NucleusDB.Contracts.Model
open Sym2

/-!
# Blockchain.PaymentChannels.EVMAdapter.SeamTheorem

Phase 2.4: seam theorems connecting the concrete EVM settlement semantics to the abstract
graph-level operations (`SettlementOps`).
-/

namespace SeamTheorem

open SettlementOps
open SettlementSemantics

abbrev Extracted (cfg : ExtractorConfig) (s : EVMState) : ChannelGraph Address :=
  extractChannelGraph cfg s

/-!
## Concrete post-states (core computations)
-/

def openState (cfg : ExtractorConfig) (caller : Address) (s : EVMState) (u v : Address) (cap : Cap) : EVMState :=
  let channelId : Nat := s.storage cfg.registryContract registryLenSlot
  let s₁ := setRegistryLength (state := s) cfg.registryContract (channelId + 1)
  let r : ChannelRecord :=
    { participant1 := u
      participant2 := v
      capacity := cap
      status := .Open }
  let s₂ := putChannelRecord s₁ cfg.channelContract channelId r
  EVMState.updateBalance s₂ caller (fun b => b - cap)

def closeState (cfg : ExtractorConfig) (caller : Address) (s : EVMState) (channelId : Nat) (r : ChannelRecord) : EVMState :=
  let r' : ChannelRecord := { r with status := .Closed }
  let s₁ := putChannelRecord s cfg.channelContract channelId r'
  EVMState.updateBalance s₁ caller (fun b => b + r.capacity)

def spliceState (cfg : ExtractorConfig) (caller : Address) (s : EVMState) (channelId : Nat) (r : ChannelRecord) (newCap : Cap) : EVMState :=
  let oldCap := r.capacity
  if oldCap ≤ newCap then
    let delta := newCap - oldCap
    let r' : ChannelRecord := { r with capacity := newCap }
    let s₁ := putChannelRecord s cfg.channelContract channelId r'
    EVMState.updateBalance s₁ caller (fun b => b - delta)
  else
    let delta := oldCap - newCap
    let r' : ChannelRecord := { r with capacity := newCap }
    let s₁ := putChannelRecord s cfg.channelContract channelId r'
    EVMState.updateBalance s₁ caller (fun b => b + delta)

/-!
## Helper lemmas
-/

namespace Lemmas

lemma statusOfVal_statusToVal (st : ChannelStatus) :
    ChannelRecord.statusOfVal (ChannelRecord.statusToVal st) = st := by
  cases st <;> rfl

lemma isValidAddress_iff {a : Address} :
    SettlementSemantics.isValidAddress a ↔ ∃ n, addrVal? a = some n ∧ n ≠ 0 ∧ addrOfVal n = a := by
  unfold SettlementSemantics.isValidAddress
  cases h : addrVal? a with
  | none =>
      simp
  | some n =>
      simp

lemma addrVal_ne_zero_of_isValidAddress {a : Address} (h : SettlementSemantics.isValidAddress a) :
    addrVal a ≠ 0 := by
  rcases (isValidAddress_iff (a := a)).1 h with ⟨n, hn, hn0, _⟩
  have : addrVal a = n := by
    simp [addrVal, hn]
  simpa [this] using hn0

lemma addrOfVal_addrVal_of_isValidAddress {a : Address} (h : SettlementSemantics.isValidAddress a) :
    addrOfVal (addrVal a) = a := by
  rcases (isValidAddress_iff (a := a)).1 h with ⟨n, hn, _hn0, hEq⟩
  have : addrVal a = n := by
    simp [addrVal, hn]
  simpa [this] using hEq

lemma getChannelRecord_updateBalance (s : EVMState) (addr : Address) (f : Cap → Cap) (contract : Address) (channelId : Nat) :
    getChannelRecord (EVMState.updateBalance s addr f) contract channelId =
      getChannelRecord s contract channelId := by
  rfl

lemma channelSlot_ne_registryLenSlot (channelId field : Nat) :
    channelSlot channelId field ≠ registryLenSlot := by
  simp [channelSlot, registryLenSlot]

lemma getChannelRecord_setRegistryLength (s : EVMState) (registry contract : Address) (n channelId : Nat) :
    getChannelRecord (setRegistryLength s registry n) contract channelId =
      getChannelRecord s contract channelId := by
  classical
  by_cases hcr : contract = registry
  · subst hcr
    -- The updated key is `registryLenSlot = 0`, while all `channelSlot` keys are ≥ 100.
    simp [getChannelRecord, setRegistryLength, EVMState.withStorage, registryLenSlot, channelSlot, ChannelFields.p1,
      ChannelFields.p2, ChannelFields.cap, ChannelFields.status]
  ·
    simp [getChannelRecord, setRegistryLength, EVMState.withStorage, hcr]

lemma getChannelRecord_putChannelRecord_self (s : EVMState) (contract : Address) (channelId : Nat) (r : ChannelRecord)
    (hP1 : SettlementSemantics.isValidAddress r.participant1)
    (hP2 : SettlementSemantics.isValidAddress r.participant2) :
    getChannelRecord (putChannelRecord s contract channelId r) contract channelId = some r := by
  classical
  cases r with
  | mk p1 p2 cap st =>
      have hP1' : addrVal p1 ≠ 0 := addrVal_ne_zero_of_isValidAddress (a := p1) hP1
      have hP2' : addrVal p2 ≠ 0 := addrVal_ne_zero_of_isValidAddress (a := p2) hP2
      have hEq1 : addrOfVal (addrVal p1) = p1 := addrOfVal_addrVal_of_isValidAddress (a := p1) hP1
      have hEq2 : addrOfVal (addrVal p2) = p2 := addrOfVal_addrVal_of_isValidAddress (a := p2) hP2
      simp [getChannelRecord, putChannelRecord, EVMState.withStorage, ChannelRecord.statusToVal, ChannelRecord.statusOfVal,
        ChannelFields.p1, ChannelFields.p2, ChannelFields.cap, ChannelFields.status, channelSlot,
        hP1', hP2', hEq1, hEq2]
      cases st <;> rfl

lemma channelSlot_lt_channelSlot_p1_of_lt (id newId f : Nat) (hid : id < newId) (hf : f < 10) :
    channelSlot id f < channelSlot newId ChannelFields.p1 := by
  have hlt : id * 10 + f < newId * 10 := by
    have hsucc : id.succ ≤ newId := Nat.succ_le_iff.2 hid
    have hmul : id.succ * 10 ≤ newId * 10 := Nat.mul_le_mul_right 10 hsucc
    have hlt' : id * 10 + f < id.succ * 10 := by
      have : id * 10 + f < id * 10 + 10 := Nat.add_lt_add_left hf (id * 10)
      simpa [Nat.succ_mul, Nat.add_assoc] using this
    exact Nat.lt_of_lt_of_le hlt' hmul
  simpa [channelSlot, ChannelFields.p1, Nat.add_assoc, Nat.add_left_comm, Nat.add_comm] using
    Nat.add_lt_add_left hlt 100

lemma getChannelRecord_putChannelRecord_of_lt (s : EVMState) (contract : Address) {id newId : Nat}
    (hid : id < newId) (r : ChannelRecord) :
    getChannelRecord (putChannelRecord s contract newId r) contract id =
      getChannelRecord s contract id := by
  classical
  -- Each key for `id` is strictly below the entire block for `newId`,
  -- hence unaffected by all four writes for `newId`.
  have hIdP1 : channelSlot id ChannelFields.p1 < channelSlot newId ChannelFields.p1 :=
    channelSlot_lt_channelSlot_p1_of_lt id newId ChannelFields.p1 hid (by decide)
  have hIdP2 : channelSlot id ChannelFields.p2 < channelSlot newId ChannelFields.p1 :=
    channelSlot_lt_channelSlot_p1_of_lt id newId ChannelFields.p2 hid (by decide)
  have hIdCap : channelSlot id ChannelFields.cap < channelSlot newId ChannelFields.p1 :=
    channelSlot_lt_channelSlot_p1_of_lt id newId ChannelFields.cap hid (by decide)
  have hIdSt : channelSlot id ChannelFields.status < channelSlot newId ChannelFields.p1 :=
    channelSlot_lt_channelSlot_p1_of_lt id newId ChannelFields.status hid (by decide)

  have hLeP2 : channelSlot newId ChannelFields.p1 ≤ channelSlot newId ChannelFields.p2 := by
    simp [channelSlot, ChannelFields.p1, ChannelFields.p2]
  have hLeCap : channelSlot newId ChannelFields.p1 ≤ channelSlot newId ChannelFields.cap := by
    simp [channelSlot, ChannelFields.p1, ChannelFields.cap]
  have hLeSt : channelSlot newId ChannelFields.p1 ≤ channelSlot newId ChannelFields.status := by
    simp [channelSlot, ChannelFields.p1, ChannelFields.status]

  have hP1neP1 : channelSlot id ChannelFields.p1 ≠ channelSlot newId ChannelFields.p1 := Nat.ne_of_lt hIdP1
  have hP1neP2 : channelSlot id ChannelFields.p1 ≠ channelSlot newId ChannelFields.p2 :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdP1 hLeP2)
  have hP1neCap : channelSlot id ChannelFields.p1 ≠ channelSlot newId ChannelFields.cap :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdP1 hLeCap)
  have hP1neSt : channelSlot id ChannelFields.p1 ≠ channelSlot newId ChannelFields.status :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdP1 hLeSt)

  have hP2neP1 : channelSlot id ChannelFields.p2 ≠ channelSlot newId ChannelFields.p1 := Nat.ne_of_lt hIdP2
  have hP2neP2 : channelSlot id ChannelFields.p2 ≠ channelSlot newId ChannelFields.p2 :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdP2 hLeP2)
  have hP2neCap : channelSlot id ChannelFields.p2 ≠ channelSlot newId ChannelFields.cap :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdP2 hLeCap)
  have hP2neSt : channelSlot id ChannelFields.p2 ≠ channelSlot newId ChannelFields.status :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdP2 hLeSt)

  have hCapneP1 : channelSlot id ChannelFields.cap ≠ channelSlot newId ChannelFields.p1 := Nat.ne_of_lt hIdCap
  have hCapneP2 : channelSlot id ChannelFields.cap ≠ channelSlot newId ChannelFields.p2 :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdCap hLeP2)
  have hCapneCap : channelSlot id ChannelFields.cap ≠ channelSlot newId ChannelFields.cap :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdCap hLeCap)
  have hCapneSt : channelSlot id ChannelFields.cap ≠ channelSlot newId ChannelFields.status :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdCap hLeSt)

  have hStneP1 : channelSlot id ChannelFields.status ≠ channelSlot newId ChannelFields.p1 := Nat.ne_of_lt hIdSt
  have hStneP2 : channelSlot id ChannelFields.status ≠ channelSlot newId ChannelFields.p2 :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdSt hLeP2)
  have hStneCap : channelSlot id ChannelFields.status ≠ channelSlot newId ChannelFields.cap :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdSt hLeCap)
  have hStneSt : channelSlot id ChannelFields.status ≠ channelSlot newId ChannelFields.status :=
    Nat.ne_of_lt (Nat.lt_of_lt_of_le hIdSt hLeSt)

  -- Evaluate the four storage reads used by `getChannelRecord .. id`.
  let s' := putChannelRecord s contract newId r
  have hp1 :
      s'.storage contract (channelSlot id ChannelFields.p1) =
        s.storage contract (channelSlot id ChannelFields.p1) := by
    simp [s', putChannelRecord, EVMState.withStorage,
      hP1neP1, hP1neP2, hP1neCap, hP1neSt]
  have hp2 :
      s'.storage contract (channelSlot id ChannelFields.p2) =
        s.storage contract (channelSlot id ChannelFields.p2) := by
    simp [s', putChannelRecord, EVMState.withStorage,
      hP2neP1, hP2neP2, hP2neCap, hP2neSt]
  have hcap :
      s'.storage contract (channelSlot id ChannelFields.cap) =
        s.storage contract (channelSlot id ChannelFields.cap) := by
    simp [s', putChannelRecord, EVMState.withStorage,
      hCapneP1, hCapneP2, hCapneCap, hCapneSt]
  have hst :
      s'.storage contract (channelSlot id ChannelFields.status) =
        s.storage contract (channelSlot id ChannelFields.status) := by
    simp [s', putChannelRecord, EVMState.withStorage,
      hStneP1, hStneP2, hStneCap, hStneSt]
  -- Now rewrite `getChannelRecord` using those equalities.
  simp [getChannelRecord, s', hp1, hp2, hcap, hst]

lemma getChannelRecord_putChannelRecord_of_gt (s : EVMState) (contract : Address) {id newId : Nat}
    (hid : newId < id) (r : ChannelRecord) :
    getChannelRecord (putChannelRecord s contract newId r) contract id =
      getChannelRecord s contract id := by
  classical
  -- Each key for `id` is strictly above the entire block for `newId`,
  -- hence unaffected by all four writes for `newId`.
  have hNewP1 : channelSlot newId ChannelFields.p1 < channelSlot id ChannelFields.p1 :=
    channelSlot_lt_channelSlot_p1_of_lt newId id ChannelFields.p1 hid (by decide)
  have hNewP2 : channelSlot newId ChannelFields.p2 < channelSlot id ChannelFields.p1 :=
    channelSlot_lt_channelSlot_p1_of_lt newId id ChannelFields.p2 hid (by decide)
  have hNewCap : channelSlot newId ChannelFields.cap < channelSlot id ChannelFields.p1 :=
    channelSlot_lt_channelSlot_p1_of_lt newId id ChannelFields.cap hid (by decide)
  have hNewSt : channelSlot newId ChannelFields.status < channelSlot id ChannelFields.p1 :=
    channelSlot_lt_channelSlot_p1_of_lt newId id ChannelFields.status hid (by decide)

  have hLeP2 : channelSlot id ChannelFields.p1 ≤ channelSlot id ChannelFields.p2 := by
    simp [channelSlot, ChannelFields.p1, ChannelFields.p2]
  have hLeCap : channelSlot id ChannelFields.p1 ≤ channelSlot id ChannelFields.cap := by
    simp [channelSlot, ChannelFields.p1, ChannelFields.cap]
  have hLeSt : channelSlot id ChannelFields.p1 ≤ channelSlot id ChannelFields.status := by
    simp [channelSlot, ChannelFields.p1, ChannelFields.status]

  have hP1neP1 : channelSlot id ChannelFields.p1 ≠ channelSlot newId ChannelFields.p1 := (Nat.ne_of_lt hNewP1).symm
  have hP1neP2 : channelSlot id ChannelFields.p1 ≠ channelSlot newId ChannelFields.p2 := (Nat.ne_of_lt hNewP2).symm
  have hP1neCap : channelSlot id ChannelFields.p1 ≠ channelSlot newId ChannelFields.cap := (Nat.ne_of_lt hNewCap).symm
  have hP1neSt : channelSlot id ChannelFields.p1 ≠ channelSlot newId ChannelFields.status := (Nat.ne_of_lt hNewSt).symm

  have hP2neP1 :
      channelSlot id ChannelFields.p2 ≠ channelSlot newId ChannelFields.p1 :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewP1 hLeP2)).symm
  have hP2neP2 :
      channelSlot id ChannelFields.p2 ≠ channelSlot newId ChannelFields.p2 :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewP2 hLeP2)).symm
  have hP2neCap :
      channelSlot id ChannelFields.p2 ≠ channelSlot newId ChannelFields.cap :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewCap hLeP2)).symm
  have hP2neSt :
      channelSlot id ChannelFields.p2 ≠ channelSlot newId ChannelFields.status :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewSt hLeP2)).symm

  have hCapneP1 :
      channelSlot id ChannelFields.cap ≠ channelSlot newId ChannelFields.p1 :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewP1 hLeCap)).symm
  have hCapneP2 :
      channelSlot id ChannelFields.cap ≠ channelSlot newId ChannelFields.p2 :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewP2 hLeCap)).symm
  have hCapneCap :
      channelSlot id ChannelFields.cap ≠ channelSlot newId ChannelFields.cap :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewCap hLeCap)).symm
  have hCapneSt :
      channelSlot id ChannelFields.cap ≠ channelSlot newId ChannelFields.status :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewSt hLeCap)).symm

  have hStneP1 :
      channelSlot id ChannelFields.status ≠ channelSlot newId ChannelFields.p1 :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewP1 hLeSt)).symm
  have hStneP2 :
      channelSlot id ChannelFields.status ≠ channelSlot newId ChannelFields.p2 :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewP2 hLeSt)).symm
  have hStneCap :
      channelSlot id ChannelFields.status ≠ channelSlot newId ChannelFields.cap :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewCap hLeSt)).symm
  have hStneSt :
      channelSlot id ChannelFields.status ≠ channelSlot newId ChannelFields.status :=
    (Nat.ne_of_lt (Nat.lt_of_lt_of_le hNewSt hLeSt)).symm

  -- Evaluate the four storage reads used by `getChannelRecord .. id`.
  let s' := putChannelRecord s contract newId r
  have hp1 :
      s'.storage contract (channelSlot id ChannelFields.p1) =
        s.storage contract (channelSlot id ChannelFields.p1) := by
    simp [s', putChannelRecord, EVMState.withStorage,
      hP1neP1, hP1neP2, hP1neCap, hP1neSt]
  have hp2 :
      s'.storage contract (channelSlot id ChannelFields.p2) =
        s.storage contract (channelSlot id ChannelFields.p2) := by
    simp [s', putChannelRecord, EVMState.withStorage,
      hP2neP1, hP2neP2, hP2neCap, hP2neSt]
  have hcap :
      s'.storage contract (channelSlot id ChannelFields.cap) =
        s.storage contract (channelSlot id ChannelFields.cap) := by
    simp [s', putChannelRecord, EVMState.withStorage,
      hCapneP1, hCapneP2, hCapneCap, hCapneSt]
  have hst :
      s'.storage contract (channelSlot id ChannelFields.status) =
        s.storage contract (channelSlot id ChannelFields.status) := by
    simp [s', putChannelRecord, EVMState.withStorage,
      hStneP1, hStneP2, hStneCap, hStneSt]
  -- Now rewrite `getChannelRecord` using those equalities.
  simp [getChannelRecord, s', hp1, hp2, hcap, hst]

lemma getChannelRecord_putChannelRecord_of_ne (s : EVMState) (contract : Address) {id newId : Nat}
    (hid : id ≠ newId) (r : ChannelRecord) :
    getChannelRecord (putChannelRecord s contract newId r) contract id =
      getChannelRecord s contract id := by
  classical
  rcases lt_trichotomy id newId with hidlt | rfl | hidgt
  · exact getChannelRecord_putChannelRecord_of_lt (s := s) (contract := contract) (id := id) (newId := newId) hidlt r
  · cases hid rfl
  · exact getChannelRecord_putChannelRecord_of_gt (s := s) (contract := contract) (id := id) (newId := newId) hidgt r

end Lemmas

/-!
## Open consistency
-/

theorem settlementStep_open_consistent (cfg : ExtractorConfig)
    (caller : Address) (s : EVMState) (u v : Address) (cap : Cap)
    (hValidU : SettlementSemantics.isValidAddress u)
    (hValidV : SettlementSemantics.isValidAddress v)
    (huv : u ≠ v) (hcap : cap ≠ 0)
    (hFresh : Sym2.mk (u, v) ∉ (Extracted cfg s).edges)
    (hCaller : caller = u ∨ caller = v)
    (hFunds : SettlementSemantics.callerHasFunds s caller cap) :
    SettlementSemantics.settlementStep cfg caller s (.open u v cap) = .ok (openState cfg caller s u v cap) ∧
      SettlementOps.graphOpen (Extracted cfg s) u v cap = .ok (Extracted cfg (openState cfg caller s u v cap)) := by
  constructor
  ·
    have hAccess : ¬ (caller ≠ u ∧ caller ≠ v) := by
      intro h
      rcases hCaller with rfl | rfl
      · exact h.1 rfl
      · exact h.2 rfl
    simp [SettlementSemantics.settlementStep, openState, hValidU, hValidV, huv, hcap, hFresh, hAccess, hFunds]
  ·
    -- Abstract open succeeds.
    have hOpenOk :
        SettlementOps.graphOpen (Extracted cfg s) u v cap =
          .ok
            { edges := insert (s(u, v)) (Extracted cfg s).edges
              cap := fun e => if e = s(u, v) then cap else (Extracted cfg s).cap e
              loopless := by
                intro e he
                rcases (Finset.mem_insert.mp he) with rfl | heOld
                · simpa [Sym2.mk_isDiag_iff] using huv
                · exact (Extracted cfg s).loopless e heOld } := by
      simp [SettlementOps.graphOpen, huv, hFresh]
    -- Show the extracted graph of the concrete post-state has the same edges and cap function.
    let newId : Nat := s.storage cfg.registryContract registryLenSlot
    let s₁ := setRegistryLength (state := s) cfg.registryContract (newId + 1)
    let rNew : ChannelRecord :=
      { participant1 := u
        participant2 := v
        capacity := cap
        status := .Open }
    let s₂ := putChannelRecord s₁ cfg.channelContract newId rNew

    have hRecNew : getChannelRecord s₂ cfg.channelContract newId = some rNew := by
      -- The just-written channel record is readable back.
      simpa [s₂] using
        (Lemmas.getChannelRecord_putChannelRecord_self (s := s₁) (contract := cfg.channelContract)
          (channelId := newId) (r := rNew) hValidU hValidV)

    have hLen :
        (openState cfg caller s u v cap).storage cfg.registryContract registryLenSlot = newId + 1 := by
      have h0p1 : (registryLenSlot : Nat) ≠ channelSlot newId ChannelFields.p1 := by
        exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot newId ChannelFields.p1)
      have h0p2 : (registryLenSlot : Nat) ≠ channelSlot newId ChannelFields.p2 := by
        exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot newId ChannelFields.p2)
      have h0cap : (registryLenSlot : Nat) ≠ channelSlot newId ChannelFields.cap := by
        exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot newId ChannelFields.cap)
      have h0st : (registryLenSlot : Nat) ≠ channelSlot newId ChannelFields.status := by
        exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot newId ChannelFields.status)
      by_cases hEq : cfg.registryContract = cfg.channelContract
      ·
        -- Same contract: `putChannelRecord` touches only `channelSlot` keys, never `registryLenSlot`.
        have h0p1' : (0 : Nat) ≠ channelSlot (s.storage cfg.channelContract 0) ChannelFields.p1 := by
          exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot (s.storage cfg.channelContract 0) ChannelFields.p1)
        have h0p2' : (0 : Nat) ≠ channelSlot (s.storage cfg.channelContract 0) ChannelFields.p2 := by
          exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot (s.storage cfg.channelContract 0) ChannelFields.p2)
        have h0cap' : (0 : Nat) ≠ channelSlot (s.storage cfg.channelContract 0) ChannelFields.cap := by
          exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot (s.storage cfg.channelContract 0) ChannelFields.cap)
        have h0st' : (0 : Nat) ≠ channelSlot (s.storage cfg.channelContract 0) ChannelFields.status := by
          exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot (s.storage cfg.channelContract 0) ChannelFields.status)
        simp [openState, setRegistryLength, registryLenSlot, EVMState.updateBalance, EVMState.withBalance,
          EVMState.withStorage, putChannelRecord, hEq, h0p1', h0p2', h0cap', h0st', newId]
      ·
        -- Different contracts: `putChannelRecord` does not affect `cfg.registryContract` storage at all.
        simp [openState, setRegistryLength, registryLenSlot, EVMState.updateBalance, EVMState.withBalance,
          EVMState.withStorage, putChannelRecord, hEq, newId]

    have hIdsOpen :
        cfg.ids (openState cfg caller s u v cap) = Finset.range (newId + 1) := by
      simp [ExtractorConfig.ids, channelIds, hLen]

    have hRecOld (id : Nat) (hid : id < newId) :
        getChannelRecord s₂ cfg.channelContract id = getChannelRecord s cfg.channelContract id := by
      -- Balance updates do not affect storage; `s₂` has only storage updates.
      have h1 :
          getChannelRecord s₂ cfg.channelContract id =
            getChannelRecord s₁ cfg.channelContract id := by
        simpa [s₂] using
          (Lemmas.getChannelRecord_putChannelRecord_of_lt (s := s₁) (contract := cfg.channelContract) (id := id)
            (newId := newId) hid rNew)
      have h2 :
          getChannelRecord s₁ cfg.channelContract id =
            getChannelRecord s cfg.channelContract id := by
        simpa [s₁] using
          (Lemmas.getChannelRecord_setRegistryLength (s := s) (registry := cfg.registryContract) (contract := cfg.channelContract)
            (n := newId + 1) (channelId := id))
      simp [h1, h2]

    -- Edges: new id contributes exactly the new edge; old ids are unchanged.
    have hEdges :
        (Extracted cfg (openState cfg caller s u v cap)).edges =
          insert (s(u, v)) (Extracted cfg s).edges := by
      classical
      -- Unfold `Extracted` and reduce to `Extractor.extractedEdges`.
      apply Finset.ext
      intro e
      constructor
      · intro he
        -- membership in extracted edges gives a witness channel id
        have he' : e ∈ Extractor.extractedEdges cfg (openState cfg caller s u v cap) := by
          simpa [Extracted, extractChannelGraph] using he
        rcases (Finset.mem_biUnion).1 he' with ⟨id, hid, heId⟩
        -- compute the id bound from `hid : id ∈ range (newId+1)`
        have hidLt : id ≤ newId := by
          have hidRange : id ∈ Finset.range (newId + 1) := by
            simpa [hIdsOpen] using hid
          exact Nat.lt_succ_iff.1 (Finset.mem_range.1 hidRange)
        rcases (Nat.lt_or_eq_of_le hidLt) with hidlt | rfl
        · -- old id
          have : e ∈ Extractor.edgeSetOfId cfg s id := by
            -- reduce edgeSetOfId via getChannelRecord equality
            have hRec : getChannelRecord (openState cfg caller s u v cap) cfg.channelContract id =
                getChannelRecord s cfg.channelContract id := by
              -- strip balance update
              have hb :
                  getChannelRecord (openState cfg caller s u v cap) cfg.channelContract id =
                    getChannelRecord s₂ cfg.channelContract id := by
                rfl
              have hb' :
                  getChannelRecord s₂ cfg.channelContract id =
                    getChannelRecord s cfg.channelContract id := hRecOld id hidlt
              simp [hb, hb']
            -- unfold edgeSetOfId for both states
            -- if the record is the same, membership is the same.
            simpa [Extractor.edgeSetOfId, hRec] using heId
          -- lift to extractedEdges on the old state
          have hidOld : id ∈ cfg.ids s := by
            -- `id < newId` implies `id ∈ range newId`.
            exact Finset.mem_range.2 hidlt
          have : e ∈ Extractor.extractedEdges cfg s :=
            (Finset.mem_biUnion).2 ⟨id, hidOld, this⟩
          -- finish
          have : e ∈ (Extracted cfg s).edges := by
            simpa [Extracted, extractChannelGraph] using this
          exact Finset.mem_insert_of_mem this
        · -- new id: membership forces `e = s(u,v)`
          have : e = s(u, v) := by
            -- edgeSetOfId for `newId` is a singleton
            have hRec : getChannelRecord (openState cfg caller s u v cap) cfg.channelContract newId = some rNew := by
              -- strip balance update
              simpa [openState, s₁, s₂, newId, rNew] using hRecNew
            -- unfold edgeSetOfId
            have : e ∈ ({Extractor.edgeOfRecord rNew} : Finset (Sym2 Address)) := by
              simpa [Extractor.edgeSetOfId, hRec, rNew, Extractor.edgeOfRecord, huv] using heId
            simpa using (Finset.mem_singleton.1 this)
          subst this
          exact Finset.mem_insert_self _ _
      · intro he
        -- membership in the insert splits
        rcases (Finset.mem_insert).1 he with rfl | heOld
        · -- new edge uses the new id
          have hidNew : newId ∈ cfg.ids (openState cfg caller s u v cap) := by
            simp [hIdsOpen]
          have heSet : s(u, v) ∈ Extractor.edgeSetOfId cfg (openState cfg caller s u v cap) newId := by
            have hRec : getChannelRecord (openState cfg caller s u v cap) cfg.channelContract newId = some rNew := by
              simpa [openState, s₁, s₂, newId, rNew] using hRecNew
            simp [Extractor.edgeSetOfId, hRec, rNew, Extractor.edgeOfRecord, huv]
          have : s(u, v) ∈ Extractor.extractedEdges cfg (openState cfg caller s u v cap) :=
            (Finset.mem_biUnion).2 ⟨newId, hidNew, heSet⟩
          simpa [Extracted, extractChannelGraph] using this
        · -- old edge: reuse the old witness id for membership
          have heOld' : e ∈ Extractor.extractedEdges cfg s := by
            simpa [Extracted, extractChannelGraph] using heOld
          rcases (Finset.mem_biUnion).1 heOld' with ⟨id, hidId, heId⟩
          have hidLt : id < newId := by
            -- from `hidId : id ∈ range newId`
            exact Finset.mem_range.1 hidId
          have hidNew : id ∈ cfg.ids (openState cfg caller s u v cap) := by
            have : id < newId + 1 := Nat.lt_of_lt_of_le hidLt (Nat.le_succ newId)
            have : id ∈ Finset.range (newId + 1) := Finset.mem_range.2 this
            simpa [hIdsOpen] using this
          have heId' : e ∈ Extractor.edgeSetOfId cfg (openState cfg caller s u v cap) id := by
            have hRec :
                getChannelRecord (openState cfg caller s u v cap) cfg.channelContract id =
                  getChannelRecord s cfg.channelContract id := by
              -- strip balance update
              have hb :
                  getChannelRecord (openState cfg caller s u v cap) cfg.channelContract id =
                    getChannelRecord s₂ cfg.channelContract id := by
                rfl
              have hb' :
                  getChannelRecord s₂ cfg.channelContract id =
                    getChannelRecord s cfg.channelContract id := hRecOld id hidLt
              simp [hb, hb']
            simpa [Extractor.edgeSetOfId, hRec] using heId
          have : e ∈ Extractor.extractedEdges cfg (openState cfg caller s u v cap) :=
            (Finset.mem_biUnion).2 ⟨id, hidNew, heId'⟩
          simpa [Extracted, extractChannelGraph] using this

    -- Cap: compute the sup over IDs as `sup_insert` after `range_succ`.
    have hCap :
        (Extracted cfg (openState cfg caller s u v cap)).cap =
          fun e => if e = s(u, v) then cap else (Extracted cfg s).cap e := by
      classical
      funext e
      -- Abbreviations for the per-ID contributions.
      let fNew : Nat → Cap := fun channelId =>
        match getChannelRecord (openState cfg caller s u v cap) cfg.channelContract channelId with
        | none => 0
        | some r => if r.status = .Open ∧ Extractor.edgeOfRecord r = e then r.capacity else 0
      let fOld : Nat → Cap := fun channelId =>
        match getChannelRecord s cfg.channelContract channelId with
        | none => 0
        | some r => if r.status = .Open ∧ Extractor.edgeOfRecord r = e then r.capacity else 0

      have hIdsOld : cfg.ids s = Finset.range newId := by
        simp [ExtractorConfig.ids, channelIds, newId]

      have hSupOld : (Finset.range newId).sup fNew = (Finset.range newId).sup fOld := by
        apply Finset.sup_congr rfl
        intro id hidRange
        have hidLt : id < newId := Finset.mem_range.1 hidRange
        have hRec :
            getChannelRecord (openState cfg caller s u v cap) cfg.channelContract id =
              getChannelRecord s cfg.channelContract id := by
          have hb :
              getChannelRecord (openState cfg caller s u v cap) cfg.channelContract id =
                getChannelRecord s₂ cfg.channelContract id := by
            rfl
          have hb' :
              getChannelRecord s₂ cfg.channelContract id =
                getChannelRecord s cfg.channelContract id := hRecOld id hidLt
          simp [hb, hb']
        simp [fNew, fOld, hRec]

      -- Expand the new cap as a `sup` over the inserted new ID.
      have hCapNew :
          (Extracted cfg (openState cfg caller s u v cap)).cap e =
            fNew newId ⊔ (Extracted cfg s).cap e := by
        have hSupOld' :
            (Finset.range newId).sup (fun channelId =>
                match getChannelRecord (openState cfg caller s u v cap) cfg.channelContract channelId with
                | none => 0
                | some r =>
                    if r.status = ChannelStatus.Open ∧ Extractor.edgeOfRecord r = e then r.capacity else 0) =
              (Finset.range newId).sup (fun channelId =>
                match getChannelRecord s cfg.channelContract channelId with
                | none => 0
                | some r =>
                    if r.status = ChannelStatus.Open ∧ Extractor.edgeOfRecord r = e then r.capacity else 0) := by
          simpa [fNew, fOld] using hSupOld
        -- Rewrite the new cap as a `sup` over the inserted new ID.
        simp [Extracted, extractChannelGraph, Extractor.extractedCap, hIdsOpen, hIdsOld, Finset.range_add_one]
        -- Replace the old `sup` over the (unchanged) IDs.
        simpa using
          congrArg
            (fun x =>
              max
                (match getChannelRecord (openState cfg caller s u v cap) cfg.channelContract newId with
                | none => 0
                | some r =>
                    if r.status = ChannelStatus.Open ∧ Extractor.edgeOfRecord r = e then r.capacity else 0)
                x)
            hSupOld'

      -- Compute the contribution of the new record.
      have hNewVal : fNew newId = (if e = s(u, v) then cap else 0) := by
        -- Replace the record read at `newId` by `rNew`.
        have : getChannelRecord (openState cfg caller s u v cap) cfg.channelContract newId = some rNew := by
          simpa [openState, s₁, s₂, newId, rNew] using hRecNew
        simp [fNew, this, rNew, Extractor.edgeOfRecord, eq_comm]

      by_cases he : e = s(u, v)
      · subst he
        -- Old cap on the fresh edge is 0.
        have hOldZero : (Extracted cfg s).cap (s(u, v)) = 0 := by
          have : Extractor.extractedCap cfg s (s(u, v)) = 0 := by
            classical
            unfold Extractor.extractedCap
            change
              ((cfg.ids s).sup fun channelId =>
                  match getChannelRecord s cfg.channelContract channelId with
                  | none => 0
                  | some r =>
                      if r.status = ChannelStatus.Open ∧ Extractor.edgeOfRecord r = s(u, v) then
                        r.capacity
                      else
                        0) =
                (⊥ : Cap)
            rw [Finset.sup_eq_bot_iff]
            intro id hid
            cases hRec : getChannelRecord s cfg.channelContract id with
            | none =>
                simp
            | some r =>
                by_cases hCond : r.status = .Open ∧ Extractor.edgeOfRecord r = s(u, v)
                ·
                  have hLoop : r.participant1 ≠ r.participant2 := by
                    intro hEq
                    have hDiag : (Extractor.edgeOfRecord r).IsDiag := by
                      simp [Extractor.edgeOfRecord, hEq]
                    have : (s(u, v)).IsDiag := by
                      simpa [hCond.2] using hDiag
                    have : u = v := by
                      simpa [Sym2.mk_isDiag_iff] using this
                    exact huv this
                  have hEdgeSet : s(u, v) ∈ Extractor.edgeSetOfId cfg s id := by
                    simpa [Extractor.edgeSetOfId, hRec, hCond.1, hLoop] using (Finset.mem_singleton.2 hCond.2.symm)
                  have hEdge : s(u, v) ∈ (Extracted cfg s).edges := by
                    have : s(u, v) ∈ Extractor.extractedEdges cfg s :=
                      (Finset.mem_biUnion).2 ⟨id, hid, hEdgeSet⟩
                    simpa [Extracted, extractChannelGraph] using this
                  exact (hFresh hEdge).elim
                ·
                  simp [hCond]
          simpa [Extracted, extractChannelGraph] using this
        -- Combine.
        simp [hCapNew, hNewVal, hOldZero]
      ·
        -- For other edges, the new record contributes 0.
        simp [hCapNew, hNewVal, he]

    -- Assemble graph equality from data fields (proof irrelevance handles `loopless`).
    have hGraph :
        Extracted cfg (openState cfg caller s u v cap) =
          { edges := insert (s(u, v)) (Extracted cfg s).edges
            cap := fun e => if e = s(u, v) then cap else (Extracted cfg s).cap e
            loopless := by
              intro e he
              rcases (Finset.mem_insert.mp he) with rfl | heOld
              · simpa [Sym2.mk_isDiag_iff] using huv
              · exact (Extracted cfg s).loopless e heOld } := by
      -- Use `cases` + proof irrelevance.
      cases G' : Extracted cfg (openState cfg caller s u v cap) with
      | mk edges capFn loopless =>
          have hEdges' : edges = insert (s(u, v)) (Extracted cfg s).edges := by
            simpa [G'] using hEdges
          have hCap' : capFn = fun e => if e = s(u, v) then cap else (Extracted cfg s).cap e := by
            simpa [G'] using hCap
          subst hEdges'
          subst hCap'
          have : loopless =
              (by
                intro e he
                rcases (Finset.mem_insert.mp he) with rfl | heOld
                · simpa [Sym2.mk_isDiag_iff] using huv
                · exact (Extracted cfg s).loopless e heOld) := by
            apply Subsingleton.elim
          cases this
          rfl

    -- Finish by rewriting the abstract result with the extracted equality.
    simp [hOpenOk, hGraph]

/-!
## Close consistency
-/

theorem settlementStep_close_consistent (cfg : ExtractorConfig)
    (caller : Address) (s : EVMState) (u v : Address) (channelId : Nat) (r : ChannelRecord)
    (hP1 : SettlementSemantics.isValidAddress r.participant1)
    (hP2 : SettlementSemantics.isValidAddress r.participant2)
    (huv : u ≠ v)
    (hGet : SettlementSemantics.getOpenChannelRecord? cfg s u v = some (channelId, r))
    (hRec : getChannelRecord s cfg.channelContract channelId = some r)
    (hStatus : r.status = .Open)
    (hEdge : Extractor.edgeOfRecord r = Sym2.mk (u, v))
    (hId : channelId ∈ cfg.ids s)
    (hUnique : ∀ id ∈ cfg.ids s, id ≠ channelId → Sym2.mk (u, v) ∉ Extractor.edgeSetOfId cfg s id)
    (hAccess : caller = r.participant1 ∨ caller = r.participant2) :
    SettlementSemantics.settlementStep cfg caller s (.close u v) = .ok (closeState cfg caller s channelId r) ∧
      SettlementOps.graphClose (Extracted cfg s) u v = .ok (Extracted cfg (closeState cfg caller s channelId r)) := by
  constructor
  ·
    have hAccess' : ¬(caller ≠ r.participant1 ∧ caller ≠ r.participant2) := by
      intro h
      cases hAccess with
      | inl hEq => exact h.1 hEq
      | inr hEq => exact h.2 hEq
    simp [SettlementSemantics.settlementStep, hGet, closeState, hAccess']
  ·
    classical
    let edge : Sym2 Address := Sym2.mk (u, v)
    let rClosed : ChannelRecord := { r with status := .Closed }
    let sClose : EVMState := closeState cfg caller s channelId r

    have hLoop : r.participant1 ≠ r.participant2 := by
      intro hEq
      have hDiag : (Extractor.edgeOfRecord r).IsDiag := by
        simp [Extractor.edgeOfRecord, hEq]
      have : edge.IsDiag := by
        simpa [edge, hEdge] using hDiag
      have : u = v := by
        simpa [Sym2.mk_isDiag_iff, edge] using this
      exact huv this

    have hMem : edge ∈ (Extracted cfg s).edges := by
      have hEdgeSet : edge ∈ Extractor.edgeSetOfId cfg s channelId := by
        simpa [Extractor.edgeSetOfId, hRec, hStatus, hLoop, edge] using (Finset.mem_singleton.2 hEdge.symm)
      have : edge ∈ Extractor.extractedEdges cfg s :=
        (Finset.mem_biUnion).2 ⟨channelId, hId, hEdgeSet⟩
      simpa [Extracted, extractChannelGraph] using this

    have hCloseOk :
        SettlementOps.graphClose (Extracted cfg s) u v =
          .ok
            { edges := (Extracted cfg s).edges.erase edge
              cap := fun e => if e = edge then 0 else (Extracted cfg s).cap e
              loopless := by
                intro e he
                exact (Extracted cfg s).loopless e (Finset.mem_of_mem_erase he) } := by
      simp [SettlementOps.graphClose, hMem, edge]

    have hRecClosed : getChannelRecord sClose cfg.channelContract channelId = some rClosed := by
      have :
          getChannelRecord (putChannelRecord s cfg.channelContract channelId rClosed) cfg.channelContract channelId =
            some rClosed := by
        simpa using
          (Lemmas.getChannelRecord_putChannelRecord_self (s := s) (contract := cfg.channelContract)
            (channelId := channelId) (r := rClosed) hP1 hP2)
      simpa [sClose, closeState, rClosed, Lemmas.getChannelRecord_updateBalance] using this

    have hIdsClose : cfg.ids sClose = cfg.ids s := by
      have hLenClose :
          sClose.storage cfg.registryContract registryLenSlot = s.storage cfg.registryContract registryLenSlot := by
        by_cases hEq : cfg.registryContract = cfg.channelContract
        ·
          have h0p1 : (0 : Nat) ≠ channelSlot channelId ChannelFields.p1 := by
            exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot channelId ChannelFields.p1)
          have h0p2 : (0 : Nat) ≠ channelSlot channelId ChannelFields.p2 := by
            exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot channelId ChannelFields.p2)
          have h0cap : (0 : Nat) ≠ channelSlot channelId ChannelFields.cap := by
            exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot channelId ChannelFields.cap)
          have h0st : (0 : Nat) ≠ channelSlot channelId ChannelFields.status := by
            exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot channelId ChannelFields.status)
          simp [sClose, closeState, registryLenSlot, EVMState.updateBalance, EVMState.withBalance, EVMState.withStorage,
            putChannelRecord, hEq, h0p1, h0p2, h0cap, h0st]
        ·
          simp [sClose, closeState, registryLenSlot, EVMState.updateBalance, EVMState.withBalance, EVMState.withStorage,
            putChannelRecord, hEq]
      simp [ExtractorConfig.ids, channelIds, hLenClose]

    have hRecOther (id : Nat) (hne : id ≠ channelId) :
        getChannelRecord sClose cfg.channelContract id = getChannelRecord s cfg.channelContract id := by
      have :
          getChannelRecord (putChannelRecord s cfg.channelContract channelId rClosed) cfg.channelContract id =
            getChannelRecord s cfg.channelContract id := by
        simpa using
          (Lemmas.getChannelRecord_putChannelRecord_of_ne (s := s) (contract := cfg.channelContract) (id := id)
            (newId := channelId) hne rClosed)
      simpa [sClose, closeState, rClosed, Lemmas.getChannelRecord_updateBalance] using this

    have hEdgesClose :
        (Extracted cfg sClose).edges = (Extracted cfg s).edges.erase edge := by
      classical
      apply Finset.ext
      intro e
      constructor
      · intro heNew
        have heNew' : e ∈ Extractor.extractedEdges cfg sClose := by
          simpa [Extracted, extractChannelGraph] using heNew
        rcases (Finset.mem_biUnion).1 heNew' with ⟨id, hid, heSet⟩
        have hidOld : id ∈ cfg.ids s := by
          simpa [hIdsClose] using hid
        by_cases hEqId : id = channelId
        ·
          subst id
          have : False := by
            simp [Extractor.edgeSetOfId, hRecClosed, rClosed] at heSet
          exact False.elim this
        ·
          have heSetOld : e ∈ Extractor.edgeSetOfId cfg s id := by
            have hRecEq :
                getChannelRecord sClose cfg.channelContract id =
                  getChannelRecord s cfg.channelContract id :=
              hRecOther id hEqId
            simpa [Extractor.edgeSetOfId, hRecEq] using heSet
          have heOld' : e ∈ Extractor.extractedEdges cfg s :=
            (Finset.mem_biUnion).2 ⟨id, hidOld, heSetOld⟩
          have heOld : e ∈ (Extracted cfg s).edges := by
            simpa [Extracted, extractChannelGraph] using heOld'
          have hneEdge : e ≠ edge := by
            intro hEqE
            subst hEqE
            exact (hUnique id hidOld hEqId) heSetOld
          exact Finset.mem_erase.2 ⟨hneEdge, heOld⟩
      · intro heOld
        rcases (Finset.mem_erase).1 heOld with ⟨hneEdge, heOldMem⟩
        have heOld' : e ∈ Extractor.extractedEdges cfg s := by
          simpa [Extracted, extractChannelGraph] using heOldMem
        rcases (Finset.mem_biUnion).1 heOld' with ⟨id, hid, heSet⟩
        have hidNew : id ∈ cfg.ids sClose := by
          simpa [hIdsClose] using hid
        have hEqId : id ≠ channelId := by
          intro hEqId
          subst id
          have hEqEdge : e = Extractor.edgeOfRecord r := by
            simpa [Extractor.edgeSetOfId, hRec, hStatus, hLoop] using heSet
          have : e = edge := by
            simpa [edge] using (hEqEdge.trans hEdge)
          exact hneEdge this
        have heSetNew : e ∈ Extractor.edgeSetOfId cfg sClose id := by
          have hRecEq : getChannelRecord sClose cfg.channelContract id = getChannelRecord s cfg.channelContract id :=
            hRecOther id hEqId
          simpa [Extractor.edgeSetOfId, hRecEq] using heSet
        have : e ∈ Extractor.extractedEdges cfg sClose :=
          (Finset.mem_biUnion).2 ⟨id, hidNew, heSetNew⟩
        simpa [Extracted, extractChannelGraph] using this

    have hCapClose :
        (Extracted cfg sClose).cap = fun e => if e = edge then 0 else (Extracted cfg s).cap e := by
      classical
      funext e
      by_cases he : e = edge
      · subst he
        -- No remaining open records contribute to `edge`.
        have : Extractor.extractedCap cfg sClose edge = 0 := by
          unfold Extractor.extractedCap
          change
            ((cfg.ids sClose).sup fun channelId =>
                match getChannelRecord sClose cfg.channelContract channelId with
                | none => 0
                | some r =>
                    if r.status = ChannelStatus.Open ∧ Extractor.edgeOfRecord r = edge then r.capacity else 0) =
              (⊥ : Cap)
          rw [Finset.sup_eq_bot_iff]
          intro id hid
          have hidOld : id ∈ cfg.ids s := by
            simpa [hIdsClose] using hid
          by_cases hEqId : id = channelId
          ·
            subst hEqId
            simp [hRecClosed, rClosed]
          ·
            have hRecEq : getChannelRecord sClose cfg.channelContract id = getChannelRecord s cfg.channelContract id :=
              hRecOther id hEqId
            cases hR : getChannelRecord s cfg.channelContract id with
            | none =>
                simp [hRecEq, hR]
            | some r' =>
                by_cases hCond : r'.status = ChannelStatus.Open ∧ Extractor.edgeOfRecord r' = edge
                ·
                  have hLoop' : r'.participant1 ≠ r'.participant2 := by
                    intro hEq
                    have : (Extractor.edgeOfRecord r').IsDiag := by
                      simp [Extractor.edgeOfRecord, hEq]
                    have : edge.IsDiag := by
                      simpa [hCond.2] using this
                    have : u = v := by
                      simpa [Sym2.mk_isDiag_iff, edge] using this
                    exact huv this
                  have : edge ∈ Extractor.edgeSetOfId cfg s id := by
                    simpa [Extractor.edgeSetOfId, hR, hCond.1, hLoop', edge] using (Finset.mem_singleton.2 hCond.2.symm)
                  exact (hUnique id hidOld hEqId) this |>.elim
                ·
                  simp [hRecEq, hR, hCond]
        simpa [Extracted, extractChannelGraph, edge] using this
      ·
        -- For other edges, the closed record contributes 0 in both states.
        have : Extractor.extractedCap cfg sClose e = Extractor.extractedCap cfg s e := by
          unfold Extractor.extractedCap
          -- Same ID set.
          have hIds : cfg.ids sClose = cfg.ids s := hIdsClose
          simp [hIds]
          apply Finset.sup_congr rfl
          intro id hid
          by_cases hEqId : id = channelId
          ·
            subst id
            have hRec' : getChannelRecord sClose cfg.channelContract channelId = some rClosed := hRecClosed
            have hRecOld : getChannelRecord s cfg.channelContract channelId = some r := hRec
            have hEdgeNe : Extractor.edgeOfRecord r ≠ e := by
              intro hEq
              apply he
              calc
                e = Extractor.edgeOfRecord r := by simpa using hEq.symm
                _ = edge := hEdge
            simp [hRec', hRecOld, rClosed, hStatus, hEdgeNe]
          ·
            have hRecEq : getChannelRecord sClose cfg.channelContract id = getChannelRecord s cfg.channelContract id :=
              hRecOther id hEqId
            simp [hRecEq]
        simpa [Extracted, extractChannelGraph, he, edge] using this

    have hGraph :
        Extracted cfg sClose =
          { edges := (Extracted cfg s).edges.erase edge
            cap := fun e => if e = edge then 0 else (Extracted cfg s).cap e
            loopless := by
              intro e he
              exact (Extracted cfg s).loopless e (Finset.mem_of_mem_erase he) } := by
      cases G' : Extracted cfg sClose with
      | mk edges capFn loopless =>
          have hEdges' : edges = (Extracted cfg s).edges.erase edge := by
            simpa [G', sClose] using hEdgesClose
          have hCap' : capFn = fun e => if e = edge then 0 else (Extracted cfg s).cap e := by
            simpa [G', sClose] using hCapClose
          subst hEdges'
          subst hCap'
          have : loopless =
              (by
                intro e he
                exact (Extracted cfg s).loopless e (Finset.mem_of_mem_erase he)) := by
            apply Subsingleton.elim
          cases this
          rfl

    simp [hCloseOk, hGraph, sClose]

/-!
## Splice consistency
-/

theorem settlementStep_splice_consistent (cfg : ExtractorConfig)
    (caller : Address) (s : EVMState) (u v : Address) (channelId : Nat) (r : ChannelRecord) (newCap : Cap)
    (hP1 : SettlementSemantics.isValidAddress r.participant1)
    (hP2 : SettlementSemantics.isValidAddress r.participant2)
    (huv : u ≠ v)
    (hGet : SettlementSemantics.getOpenChannelRecord? cfg s u v = some (channelId, r))
    (hRec : getChannelRecord s cfg.channelContract channelId = some r)
    (hStatus : r.status = .Open)
    (hEdge : Extractor.edgeOfRecord r = Sym2.mk (u, v))
    (hId : channelId ∈ cfg.ids s)
    (hUnique : ∀ id ∈ cfg.ids s, id ≠ channelId → Sym2.mk (u, v) ∉ Extractor.edgeSetOfId cfg s id)
    (hAccess : caller = r.participant1 ∨ caller = r.participant2)
    (hNewCap : newCap ≠ 0)
    (hFunds : SettlementSemantics.callerHasFunds s caller (newCap - r.capacity)) :
    SettlementSemantics.settlementStep cfg caller s (.splice u v newCap) =
        .ok (spliceState cfg caller s channelId r newCap) ∧
      SettlementOps.graphSplice (Extracted cfg s) u v newCap =
        .ok (Extracted cfg (spliceState cfg caller s channelId r newCap)) := by
  constructor
  ·
    have hAccess' : ¬(caller ≠ r.participant1 ∧ caller ≠ r.participant2) := by
      intro h
      cases hAccess with
      | inl hEq => exact h.1 hEq
      | inr hEq => exact h.2 hEq
    by_cases hLe : r.capacity ≤ newCap
    ·
      simp [SettlementSemantics.settlementStep, hGet, hAccess', hNewCap, spliceState, hLe, hFunds]
    ·
      simp [SettlementSemantics.settlementStep, hGet, hAccess', hNewCap, spliceState, hLe]
  ·
    classical
    let edge : Sym2 Address := Sym2.mk (u, v)
    let rNew : ChannelRecord := { r with capacity := newCap }
    let sSplice : EVMState := spliceState cfg caller s channelId r newCap

    have hLoop : r.participant1 ≠ r.participant2 := by
      intro hEq
      have hDiag : (Extractor.edgeOfRecord r).IsDiag := by
        simp [Extractor.edgeOfRecord, hEq]
      have : edge.IsDiag := by
        simpa [edge, hEdge] using hDiag
      have : u = v := by
        simpa [Sym2.mk_isDiag_iff, edge] using this
      exact huv this

    have hMem : edge ∈ (Extracted cfg s).edges := by
      have hEdgeSet : edge ∈ Extractor.edgeSetOfId cfg s channelId := by
        simpa [Extractor.edgeSetOfId, hRec, hStatus, hLoop, edge] using (Finset.mem_singleton.2 hEdge.symm)
      have : edge ∈ Extractor.extractedEdges cfg s :=
        (Finset.mem_biUnion).2 ⟨channelId, hId, hEdgeSet⟩
      simpa [Extracted, extractChannelGraph] using this

    have hSpliceOk :
        SettlementOps.graphSplice (Extracted cfg s) u v newCap =
          .ok { (Extracted cfg s) with cap := fun e => if e = edge then newCap else (Extracted cfg s).cap e } := by
      simp [SettlementOps.graphSplice, hMem, edge]

    have hP1New : SettlementSemantics.isValidAddress rNew.participant1 := by
      simpa [rNew] using hP1
    have hP2New : SettlementSemantics.isValidAddress rNew.participant2 := by
      simpa [rNew] using hP2

    have hRecWrite :
        getChannelRecord (putChannelRecord s cfg.channelContract channelId rNew) cfg.channelContract channelId = some rNew := by
      simpa using
        (Lemmas.getChannelRecord_putChannelRecord_self (s := s) (contract := cfg.channelContract)
          (channelId := channelId) (r := rNew) hP1New hP2New)

    have hRecNew : getChannelRecord sSplice cfg.channelContract channelId = some rNew := by
      by_cases hLe : r.capacity ≤ newCap <;>
        simpa [sSplice, spliceState, hLe, rNew, Lemmas.getChannelRecord_updateBalance] using hRecWrite

    have hIdsSplice : cfg.ids sSplice = cfg.ids s := by
      have hLenSplice :
          sSplice.storage cfg.registryContract registryLenSlot = s.storage cfg.registryContract registryLenSlot := by
        by_cases hEq : cfg.registryContract = cfg.channelContract
        ·
          have h0p1 : (0 : Nat) ≠ channelSlot channelId ChannelFields.p1 := by
            exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot channelId ChannelFields.p1)
          have h0p2 : (0 : Nat) ≠ channelSlot channelId ChannelFields.p2 := by
            exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot channelId ChannelFields.p2)
          have h0cap : (0 : Nat) ≠ channelSlot channelId ChannelFields.cap := by
            exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot channelId ChannelFields.cap)
          have h0st : (0 : Nat) ≠ channelSlot channelId ChannelFields.status := by
            exact (ne_comm).2 (Lemmas.channelSlot_ne_registryLenSlot channelId ChannelFields.status)
          by_cases hLe : r.capacity ≤ newCap <;>
            simp [sSplice, spliceState, hLe, registryLenSlot, EVMState.updateBalance, EVMState.withBalance,
              EVMState.withStorage, putChannelRecord, hEq, h0p1, h0p2, h0cap, h0st]
        ·
          by_cases hLe : r.capacity ≤ newCap <;>
            simp [sSplice, spliceState, hLe, registryLenSlot, EVMState.updateBalance, EVMState.withBalance,
              EVMState.withStorage, putChannelRecord, hEq]
      simp [ExtractorConfig.ids, channelIds, hLenSplice]

    have hRecOther (id : Nat) (hne : id ≠ channelId) :
        getChannelRecord sSplice cfg.channelContract id = getChannelRecord s cfg.channelContract id := by
      by_cases hLe : r.capacity ≤ newCap
      ·
        have :
            getChannelRecord (putChannelRecord s cfg.channelContract channelId rNew) cfg.channelContract id =
              getChannelRecord s cfg.channelContract id := by
          simpa using
            (Lemmas.getChannelRecord_putChannelRecord_of_ne (s := s) (contract := cfg.channelContract) (id := id)
              (newId := channelId) hne rNew)
        simpa [sSplice, spliceState, hLe, rNew, Lemmas.getChannelRecord_updateBalance] using this
      ·
        have :
            getChannelRecord (putChannelRecord s cfg.channelContract channelId rNew) cfg.channelContract id =
              getChannelRecord s cfg.channelContract id := by
          simpa using
            (Lemmas.getChannelRecord_putChannelRecord_of_ne (s := s) (contract := cfg.channelContract) (id := id)
              (newId := channelId) hne rNew)
        simpa [sSplice, spliceState, hLe, rNew, Lemmas.getChannelRecord_updateBalance] using this

    have hEdgesSplice : (Extracted cfg sSplice).edges = (Extracted cfg s).edges := by
      classical
      apply Finset.ext
      intro e
      constructor
      · intro heNew
        have heNew' : e ∈ Extractor.extractedEdges cfg sSplice := by
          simpa [Extracted, extractChannelGraph] using heNew
        rcases (Finset.mem_biUnion).1 heNew' with ⟨id, hid, heSet⟩
        have hidOld : id ∈ cfg.ids s := by
          simpa [hIdsSplice] using hid
        by_cases hEqId : id = channelId
        ·
          subst id
          have hEqEdge : e = edge := by
            have hEq : e = Extractor.edgeOfRecord rNew := by
              simpa [Extractor.edgeSetOfId, hRecNew, rNew, hStatus, hLoop] using heSet
            have hEdgeNew' : Extractor.edgeOfRecord rNew = edge := by
              simpa [rNew, edge] using hEdge
            exact hEq.trans hEdgeNew'
          subst hEqEdge
          exact hMem
        ·
          have heSetOld : e ∈ Extractor.edgeSetOfId cfg s id := by
            have hRecEq : getChannelRecord sSplice cfg.channelContract id = getChannelRecord s cfg.channelContract id :=
              hRecOther id hEqId
            simpa [Extractor.edgeSetOfId, hRecEq] using heSet
          have : e ∈ Extractor.extractedEdges cfg s :=
            (Finset.mem_biUnion).2 ⟨id, hidOld, heSetOld⟩
          simpa [Extracted, extractChannelGraph] using this
      · intro heOld
        have heOld' : e ∈ Extractor.extractedEdges cfg s := by
          simpa [Extracted, extractChannelGraph] using heOld
        rcases (Finset.mem_biUnion).1 heOld' with ⟨id, hid, heSet⟩
        have hidNew : id ∈ cfg.ids sSplice := by
          simpa [hIdsSplice] using hid
        by_cases hEqId : id = channelId
        ·
          subst id
          have hEqEdge : e = edge := by
            simpa [Extractor.edgeSetOfId, hRec, hStatus, hLoop, hEdge, edge] using heSet
          subst hEqEdge
          have hEdgeSetNew : edge ∈ Extractor.edgeSetOfId cfg sSplice channelId := by
            have hEdgeNew' : Extractor.edgeOfRecord rNew = edge := by
              simpa [rNew, edge] using hEdge
            simpa [Extractor.edgeSetOfId, hRecNew, rNew, hStatus, hLoop, edge] using
              (Finset.mem_singleton.2 hEdgeNew'.symm)
          have : edge ∈ Extractor.extractedEdges cfg sSplice :=
            (Finset.mem_biUnion).2 ⟨channelId, hidNew, hEdgeSetNew⟩
          simpa [Extracted, extractChannelGraph] using this
        ·
          have heSetNew : e ∈ Extractor.edgeSetOfId cfg sSplice id := by
            have hRecEq : getChannelRecord sSplice cfg.channelContract id = getChannelRecord s cfg.channelContract id :=
              hRecOther id hEqId
            simpa [Extractor.edgeSetOfId, hRecEq] using heSet
          have : e ∈ Extractor.extractedEdges cfg sSplice :=
            (Finset.mem_biUnion).2 ⟨id, hidNew, heSetNew⟩
          simpa [Extracted, extractChannelGraph] using this

    have hCapSplice :
        (Extracted cfg sSplice).cap = fun e => if e = edge then newCap else (Extracted cfg s).cap e := by
      classical
      funext e
      by_cases he : e = edge
      · subst he
        have hEdgeNew : Extractor.edgeOfRecord rNew = edge := by
          simpa [rNew, edge] using hEdge
        have hIdSplice : channelId ∈ cfg.ids sSplice := by
          simpa [hIdsSplice] using hId

        have hSupLe : Extractor.extractedCap cfg sSplice edge ≤ newCap := by
          unfold Extractor.extractedCap
          rw [Finset.sup_le_iff]
          intro id hid
          have hidOld : id ∈ cfg.ids s := by
            simpa [hIdsSplice] using hid
          by_cases hEqId : id = channelId
          ·
            subst id
            simp [hRecNew, rNew, hStatus, edge]
            have hEqEdge :
                Extractor.edgeOfRecord
                    { participant1 := r.participant1, participant2 := r.participant2, capacity := newCap,
                      status := ChannelStatus.Open } =
                  edge := by
              simpa [Extractor.edgeOfRecord, edge] using hEdge
            simp [hEqEdge, edge]
          ·
            have hRecEq : getChannelRecord sSplice cfg.channelContract id = getChannelRecord s cfg.channelContract id :=
              hRecOther id hEqId
            cases hR : getChannelRecord s cfg.channelContract id with
            | none =>
                simp [hRecEq, hR]
            | some r' =>
                by_cases hCond : r'.status = .Open ∧ Extractor.edgeOfRecord r' = edge
                ·
                  have hLoop' : r'.participant1 ≠ r'.participant2 := by
                    intro hEq
                    have : (Extractor.edgeOfRecord r').IsDiag := by
                      simp [Extractor.edgeOfRecord, hEq]
                    have : edge.IsDiag := by
                      simpa [hCond.2] using this
                    have : u = v := by
                      simpa [Sym2.mk_isDiag_iff, edge] using this
                    exact huv this
                  have : edge ∈ Extractor.edgeSetOfId cfg s id := by
                    simpa [Extractor.edgeSetOfId, hR, hCond.1, hLoop', edge] using (Finset.mem_singleton.2 hCond.2.symm)
                  exact (hUnique id hidOld hEqId) this |>.elim
                ·
                  simp [hRecEq, hR, hCond]

        have hLeSup : newCap ≤ Extractor.extractedCap cfg sSplice edge := by
          unfold Extractor.extractedCap
          have hEqEdge :
              Extractor.edgeOfRecord
                  { participant1 := r.participant1, participant2 := r.participant2, capacity := newCap,
                    status := ChannelStatus.Open } =
                edge := by
            simpa [Extractor.edgeOfRecord, edge] using hEdge
          have hLe :=
            (Finset.le_sup (s := cfg.ids sSplice) (f := fun channelId =>
              match getChannelRecord sSplice cfg.channelContract channelId with
              | none => 0
              | some r =>
                  if r.status = ChannelStatus.Open ∧ Extractor.edgeOfRecord r = edge then r.capacity else 0) hIdSplice)
          simpa [hRecNew, rNew, hStatus, hEqEdge, edge] using hLe

        have hEqCap : Extractor.extractedCap cfg sSplice edge = newCap :=
          le_antisymm hSupLe hLeSup
        simpa [Extracted, extractChannelGraph, edge] using hEqCap
      ·
        -- For other edges, the updated record contributes 0 in both states.
        have : Extractor.extractedCap cfg sSplice e = Extractor.extractedCap cfg s e := by
          unfold Extractor.extractedCap
          have hIds : cfg.ids sSplice = cfg.ids s := hIdsSplice
          simp [hIds]
          apply Finset.sup_congr rfl
          intro id hid
          by_cases hEqId : id = channelId
          ·
            subst id
            have hEdgeNe : Extractor.edgeOfRecord r ≠ e := by
              intro hEq
              apply he
              calc
                e = Extractor.edgeOfRecord r := by simpa using hEq.symm
                _ = edge := by simpa [edge] using hEdge
            have hEdgeNeNew :
                Extractor.edgeOfRecord
                    { participant1 := r.participant1, participant2 := r.participant2, capacity := newCap,
                      status := ChannelStatus.Open } ≠
                  e := by
              simpa [Extractor.edgeOfRecord] using hEdgeNe
            simp [hRecNew, hRec, rNew, hStatus, hEdgeNe, hEdgeNeNew]
          ·
            have hRecEq : getChannelRecord sSplice cfg.channelContract id = getChannelRecord s cfg.channelContract id :=
              hRecOther id hEqId
            simp [hRecEq]
        simpa [Extracted, extractChannelGraph, he, edge] using this

    have hGraph :
        Extracted cfg sSplice =
          { (Extracted cfg s) with cap := fun e => if e = edge then newCap else (Extracted cfg s).cap e } := by
      cases G' : Extracted cfg sSplice with
      | mk edges capFn loopless =>
          have hEdges' : edges = (Extracted cfg s).edges := by
            simpa [G', sSplice] using hEdgesSplice
          have hCap' : capFn = fun e => if e = edge then newCap else (Extracted cfg s).cap e := by
            simpa [G', sSplice] using hCapSplice
          subst hEdges'
          subst hCap'
          have : loopless = (Extracted cfg s).loopless := by
            apply Subsingleton.elim
          cases this
          rfl

    simp [hSpliceOk, hGraph, sSplice]

/-!
## Topology change properties
-/

theorem topology_change_possible (cfg : ExtractorConfig)
    (caller : Address) (s : EVMState) (u v : Address) (cap : Cap)
    (hValidU : SettlementSemantics.isValidAddress u)
    (hValidV : SettlementSemantics.isValidAddress v)
    (huv : u ≠ v) (hcap : cap ≠ 0)
    (hFresh : Sym2.mk (u, v) ∉ (Extracted cfg s).edges)
    (hCaller : caller = u ∨ caller = v)
    (hFunds : SettlementSemantics.callerHasFunds s caller cap) :
    ∃ (c : SettlementOps.Call) (s' : EVMState),
      SettlementSemantics.settlementStep cfg caller s c = .ok s' ∧
      Extracted cfg s' ≠ Extracted cfg s := by
  refine ⟨.open u v cap, openState cfg caller s u v cap, ?_, ?_⟩
  · exact (settlementStep_open_consistent cfg caller s u v cap hValidU hValidV huv hcap hFresh hCaller hFunds).1
  ·
    classical
    intro hEq
    let edge : Sym2 Address := Sym2.mk (u, v)
    have hGraphOpen :=
      (settlementStep_open_consistent cfg caller s u v cap hValidU hValidV huv hcap hFresh hCaller hFunds).2
    have hOpenOk :
        SettlementOps.graphOpen (Extracted cfg s) u v cap =
          .ok
            { edges := insert edge (Extracted cfg s).edges
              cap := fun e => if e = edge then cap else (Extracted cfg s).cap e
              loopless := by
                intro e he
                rcases (Finset.mem_insert.mp he) with rfl | heOld
                · simpa [Sym2.mk_isDiag_iff] using huv
                · exact (Extracted cfg s).loopless e heOld } := by
      simp [SettlementOps.graphOpen, huv, hFresh, edge]
    have hOkEq := by
      simpa [hOpenOk] using hGraphOpen
    have hEdgesEq :
        (Extracted cfg (openState cfg caller s u v cap)).edges = insert edge (Extracted cfg s).edges := by
      injection hOkEq with hEqGraph
      simpa [Extracted, extractChannelGraph] using hEqGraph.symm
    have : edge ∈ (Extracted cfg (openState cfg caller s u v cap)).edges := by
      simp [hEdgesEq]
    have : edge ∈ (Extracted cfg s).edges := by
      simpa [hEq] using this
    exact (hFresh this).elim

theorem no_free_lunch (cfg : ExtractorConfig)
    (caller : Address) (s s' : EVMState) (c : SettlementOps.Call) :
    SettlementSemantics.settlementStep cfg caller s c = .ok s' →
    Extracted cfg s' ≠ Extracted cfg s →
    s' ≠ s := by
  intro hStep hGraphDiff hEq
  have _ := hStep
  have _ := c
  apply hGraphDiff
  simp [hEq]

end SeamTheorem

end EVMAdapter
end PaymentChannels
end NucleusDB
