import Mathlib.Combinatorics.Hall.Basic
import Mathlib.Data.Fintype.BigOperators
import Mathlib.Data.Fintype.Card
import Mathlib.Algebra.Order.BigOperators.Group.Finset
import NucleusDB.PaymentChannels.AlgorithmicCuts

namespace NucleusDB
namespace PaymentChannels

open scoped BigOperators
open Sym2

set_option maxHeartbeats 600000

/-!
# Blockchain.PaymentChannels.CutCompleteness

Phase 6: **completeness** of the cut-interval constraints.

Phase 5 proved: if `w ∈ WG`, then `w` satisfies all cut-interval inequalities.
This file proves the converse: if `w` satisfies all cut-interval inequalities, then `w ∈ WG`.

The proof is by a reduction to Hall's marriage theorem: we match "wealth copies" at vertices to
"capacity tokens" on edges, producing a liquidity assignment whose wealth projection is `w`.
-/

namespace Cuts

universe u

variable {V : Type u} [DecidableEq V]

namespace Completeness

def incidentVerts (G : ChannelGraph V) : Finset V :=
  G.edges.biUnion (fun e => e.toFinset)

lemma mem_incidentVerts_of_mem_edges {G : ChannelGraph V} {e : Sym2 V} (he : e ∈ G.edges) {v : V}
    (hv : v ∈ e) : v ∈ incidentVerts (V := V) G := by
  classical
  unfold incidentVerts
  refine (Finset.mem_biUnion).2 ?_
  refine ⟨e, he, ?_⟩
  simpa [Sym2.mem_toFinset] using hv

lemma isInternal_incidentVerts_of_mem_edges {G : ChannelGraph V} {e : Sym2 V} (he : e ∈ G.edges) :
    IsInternal (incidentVerts (V := V) G) e := by
  classical
  cases e using Sym2.ind with
  | _ u v =>
      have hu : u ∈ incidentVerts (V := V) G :=
        mem_incidentVerts_of_mem_edges (V := V) (G := G) he (v := u) (by simp)
      have hv : v ∈ incidentVerts (V := V) G :=
        mem_incidentVerts_of_mem_edges (V := V) (G := G) he (v := v) (by simp)
      exact (isInternal_mk (V := V) (S := incidentVerts (V := V) G) u v).2 ⟨hu, hv⟩

lemma cutEdges_incidentVerts_eq_empty (G : ChannelGraph V) :
    cutEdges (V := V) (G := G) (incidentVerts (V := V) G) = ∅ := by
  classical
  ext e
  constructor
  · intro he
    have he' := Finset.mem_filter.mp he
    have heEdges : e ∈ G.edges := he'.1
    have hCut : IsCut (V := V) (incidentVerts (V := V) G) e := he'.2
    have hInt : IsInternal (V := V) (incidentVerts (V := V) G) e :=
      isInternal_incidentVerts_of_mem_edges (V := V) (G := G) heEdges
    exact False.elim ((isInternal_not_isCut (V := V) (S := incidentVerts (V := V) G) e) hInt hCut)
  · intro he
    cases he

lemma internalEdges_incidentVerts_eq_edges (G : ChannelGraph V) :
    internalEdges (V := V) (G := G) (incidentVerts (V := V) G) = G.edges := by
  classical
  ext e
  constructor
  · intro he
    exact (Finset.mem_filter.mp he).1
  · intro he
    exact
      Finset.mem_filter.mpr
        ⟨he, isInternal_incidentVerts_of_mem_edges (V := V) (G := G) he⟩

lemma cutCapacity_incidentVerts_eq_zero (G : ChannelGraph V) :
    cutCapacity (V := V) (G := G) (incidentVerts (V := V) G) = 0 := by
  classical
  unfold cutCapacity
  simp [cutEdges_incidentVerts_eq_empty (V := V) (G := G)]

lemma internalCapacity_incidentVerts_eq_sum_cap (G : ChannelGraph V) :
    internalCapacity (V := V) (G := G) (incidentVerts (V := V) G) = ∑ e ∈ G.edges, G.cap e := by
  classical
  unfold internalCapacity
  simp [internalEdges_incidentVerts_eq_edges (V := V) (G := G)]

end Completeness

open Completeness

/-!
## Main theorem: cut constraints are sufficient
-/

theorem mem_WG_of_forall_cutIntervalHolds {G : ChannelGraph V} {w : V → Cap}
    (hCuts : ∀ S : Finset V, cutIntervalHolds (V := V) G w S) :
    w ∈ Wealth.WG (G := G) := by
  classical
  let verts : Finset V := incidentVerts (V := V) G

  -- Singletons have no internal capacity in a loopless graph.
  have hInt_singleton : ∀ v : V, internalCapacity (V := V) (G := G) ({v} : Finset V) = 0 := by
    intro v
    have hEmpty : internalEdges (V := V) (G := G) ({v} : Finset V) = ∅ := by
      ext e
      constructor
      · intro he
        have heEdges : e ∈ G.edges := (Finset.mem_filter.mp he).1
        have hI : IsInternal (V := V) ({v} : Finset V) e := (Finset.mem_filter.mp he).2
        cases e using Sym2.ind with
        | _ a b =>
          have hab : a = v ∧ b = v := by
            simpa using hI
          have habEq : a = b := hab.1.trans hab.2.symm
          have hne : a ≠ b := by
            simpa using (G.ne_of_mem_edges (u := a) (v := b) (by simpa using heEdges))
          exact False.elim (hne habEq)
      · intro he
        cases he
    unfold internalCapacity
    simp [hEmpty]

  -- If `v` is not incident to any edge, then its singleton cut has capacity 0.
  have hCut_singleton_of_not_mem :
      ∀ v : V, v ∉ verts → cutCapacity (V := V) (G := G) ({v} : Finset V) = 0 := by
    intro v hv
    have hCutEmpty : cutEdges (V := V) (G := G) ({v} : Finset V) = ∅ := by
      ext e
      constructor
      · intro he
        have heEdges : e ∈ G.edges := (Finset.mem_filter.mp he).1
        have hC : IsCut (V := V) ({v} : Finset V) e := (Finset.mem_filter.mp he).2
        cases e using Sym2.ind with
        | _ a b =>
            have hvIn : v ∈ s(a, b) := by
              have hC' : (a = v ∧ b ≠ v) ∨ (a ≠ v ∧ b = v) := by
                simpa using hC
              rcases hC' with ⟨rfl, _⟩ | ⟨_, rfl⟩ <;> simp
            exact
              False.elim (hv (mem_incidentVerts_of_mem_edges (V := V) (G := G) heEdges (v := v) hvIn))
      · intro he
        cases he
    unfold cutCapacity
    simp [hCutEmpty]

  -- `w` must vanish off the incident-vertex set.
  have hw_out : ∀ v : V, v ∉ verts → w v = 0 := by
    intro v hv
    have hUpper := (hCuts ({v} : Finset V)).2
    have hle : wealthSum (V := V) w ({v} : Finset V) ≤ 0 := by
      simpa [cutIntervalHolds, hInt_singleton v, hCut_singleton_of_not_mem v hv, add_zero] using hUpper
    have hEq : wealthSum (V := V) w ({v} : Finset V) = 0 := Nat.eq_zero_of_le_zero hle
    simpa [wealthSum] using hEq

  -- Total wealth on `verts` equals total edge capacity.
  have hw_total : wealthSum (V := V) w verts = ∑ e ∈ G.edges, G.cap e := by
    have h := hCuts verts
    have hCut0 : cutCapacity (V := V) (G := G) verts = 0 :=
      cutCapacity_incidentVerts_eq_zero (V := V) (G := G)
    have hInt : internalCapacity (V := V) (G := G) verts = ∑ e ∈ G.edges, G.cap e :=
      internalCapacity_incidentVerts_eq_sum_cap (V := V) (G := G)
    have hle : wealthSum (V := V) w verts ≤ ∑ e ∈ G.edges, G.cap e := by
      have := h.2
      simpa [cutIntervalHolds, hCut0, hInt, add_zero] using this
    have hge : ∑ e ∈ G.edges, G.cap e ≤ wealthSum (V := V) w verts := by
      have := h.1
      simpa [cutIntervalHolds, hCut0, hInt] using this
    exact Nat.le_antisymm hle hge

  -- Copy/token types and the Hall relation.
  let Vert : Type u := {v : V // v ∈ verts}
  let Copy : Type u := Σ v : Vert, Fin (w v.1)
  let Edge : Type u := {e : Sym2 V // e ∈ G.edges}
  let Token : Type u := Σ e : Edge, Fin (G.cap e.1)

  let r : Copy → Token → Prop := fun c t => c.1.1 ∈ t.1.1
  haveI : DecidableRel r := by
    classical
    infer_instance

  -- Card of tokens over a fixed edge.
  have tokensWith_card : ∀ e0 : Edge, ({t : Token | t.1 = e0} : Finset Token).card = G.cap e0.1 := by
    intro e0
    classical
    have hsub :
        Fintype.card {t : Token // t.1 = e0} = ({t : Token | t.1 = e0} : Finset Token).card := by
      simpa using (Fintype.card_subtype (p := fun t : Token => t.1 = e0))
    have heqv : {t : Token // t.1 = e0} ≃ Fin (G.cap e0.1) := by
      refine
        { toFun := fun t => (by simpa [t.2] using t.1.2)
          invFun := fun i => ⟨⟨e0, i⟩, rfl⟩
          left_inv := ?_
          right_inv := ?_ }
      · intro t
        ext
        · cases t with
          | mk t ht =>
              cases t with
              | mk e' i' =>
                  subst ht
                  rfl
        · simp
      · intro i
        rfl
    have hcard : Fintype.card {t : Token // t.1 = e0} = G.cap e0.1 := by
      simpa using (Fintype.card_congr heqv)
    have : ({t : Token | t.1 = e0} : Finset Token).card = Fintype.card {t : Token // t.1 = e0} := hsub.symm
    simpa [hcard] using this

  -- Card of copies over a fixed vertex.
  have copiesWith_card : ∀ v0 : Vert, ({c : Copy | c.1 = v0} : Finset Copy).card = w v0.1 := by
    intro v0
    classical
    have hsub :
        Fintype.card {c : Copy // c.1 = v0} = ({c : Copy | c.1 = v0} : Finset Copy).card := by
      simpa using (Fintype.card_subtype (p := fun c : Copy => c.1 = v0))
    have heqv : {c : Copy // c.1 = v0} ≃ Fin (w v0.1) := by
      refine
        { toFun := fun c => (by simpa [c.2] using c.1.2)
          invFun := fun i => ⟨⟨v0, i⟩, rfl⟩
          left_inv := ?_
          right_inv := ?_ }
      · intro c
        ext
        · cases c with
          | mk c hc =>
              cases c with
              | mk v' i' =>
                  subst hc
                  rfl
        · simp
      · intro i
        rfl
    have hcard : Fintype.card {c : Copy // c.1 = v0} = w v0.1 := by
      simpa using (Fintype.card_congr heqv)
    have : ({c : Copy | c.1 = v0} : Finset Copy).card = Fintype.card {c : Copy // c.1 = v0} := hsub.symm
    simpa [hcard] using this

  -- Count tokens satisfying a predicate on their underlying edge.
  have card_tokens_filter_edge :
      ∀ (p : Sym2 V → Prop) [DecidablePred p],
        ({t : Token | p t.1.1} : Finset Token).card = ∑ e ∈ G.edges, if p e then G.cap e else 0 := by
    intro p _inst
    classical
    let AToken : Finset Token := {t : Token | p t.1.1}
    have hMaps : Set.MapsTo (fun t : Token => t.1) (↑AToken : Set Token) (↑(Finset.univ : Finset Edge) : Set Edge) := by
      intro t ht
      simp
    have hdecomp :
        AToken.card = ∑ e0 ∈ (Finset.univ : Finset Edge), ({t ∈ AToken | t.1 = e0} : Finset Token).card := by
      simpa using
        (Finset.card_eq_sum_card_fiberwise (f := fun t : Token => t.1) (s := AToken)
          (t := (Finset.univ : Finset Edge)) hMaps)

    have hfiber :
        ∀ e0 : Edge, ({t ∈ AToken | t.1 = e0} : Finset Token).card = if p e0.1 then G.cap e0.1 else 0 := by
      intro e0
      by_cases hp : p e0.1
      · have hEq :
            ({t ∈ AToken | t.1 = e0} : Finset Token) = ({t : Token | t.1 = e0} : Finset Token) := by
          ext t
          constructor
          · intro ht
            have htEdge : t.1 = e0 := (Finset.mem_filter.mp ht).2
            simpa using htEdge
          · intro htEdge
            have htEdgeEq : t.1 = e0 := by
              simpa using htEdge
            have htEdgeVal : p t.1.1 := by
              have htVal : t.1.1 = e0.1 := by
                simpa using congrArg Subtype.val htEdgeEq
              simpa [htVal] using hp
            have htA : t ∈ AToken := by
              simpa [AToken] using htEdgeVal
            exact Finset.mem_filter.mpr ⟨htA, htEdgeEq⟩
        rw [hEq]
        simpa [hp] using (tokensWith_card (e0 := e0))
      · have hEq : ({t ∈ AToken | t.1 = e0} : Finset Token) = ∅ := by
          ext t
          constructor
          · intro ht
            have htA : t ∈ AToken := (Finset.mem_filter.mp ht).1
            have htEdgeEq : t.1 = e0 := (Finset.mem_filter.mp ht).2
            have htP : p t.1.1 := by
              simpa [AToken] using htA
            have htVal : t.1.1 = e0.1 := by
              simpa using congrArg Subtype.val htEdgeEq
            exact False.elim (hp (by simpa [htVal] using htP))
          · intro ht
            cases ht
        simp [hEq, hp]

    have hsum :
        (∑ e0 : Edge, if p e0.1 then G.cap e0.1 else 0) = ∑ e ∈ G.edges, if p e then G.cap e else 0 := by
      calc
        (∑ e0 : Edge, if p e0.1 then G.cap e0.1 else 0) =
            ∑ e0 ∈ (Finset.univ : Finset Edge), if p e0.1 then G.cap e0.1 else 0 := by simp
        _ = ∑ e0 ∈ G.edges.attach, if p e0.1 then G.cap e0.1 else 0 := by
              simp [Finset.univ_eq_attach, Edge]
        _ = ∑ e ∈ G.edges, if p e then G.cap e else 0 := by
              simpa using (Finset.sum_attach (s := G.edges) (f := fun e => if p e then G.cap e else 0))
    calc
      ({t : Token | p t.1.1} : Finset Token).card = AToken.card := rfl
      _ = ∑ e0 ∈ (Finset.univ : Finset Edge), ({t ∈ AToken | t.1 = e0} : Finset Token).card := hdecomp
      _ = ∑ e0 ∈ (Finset.univ : Finset Edge), if p e0.1 then G.cap e0.1 else 0 := by
        refine Finset.sum_congr rfl ?_
        intro e0 he0
        simp [hfiber (e0 := e0)]
      _ = ∑ e0 : Edge, if p e0.1 then G.cap e0.1 else 0 := by
        simp
      _ = ∑ e ∈ G.edges, if p e then G.cap e else 0 := hsum

  -- Hall condition for `r`, using the cut upper bounds.
  have hall : ∀ A : Finset Copy, A.card ≤ ({t : Token | ∃ c ∈ A, r c t} : Finset Token).card := by
    intro A
    classical
    let S : Finset V := A.image (fun c : Copy => c.1.1)

    -- A counting bound: `A.card ≤ wealthSum w S`.
    have hA_le : A.card ≤ wealthSum (V := V) w S := by
      have hfiber :
          ∀ v : V, ({c ∈ A | c.1.1 = v} : Finset Copy).card ≤ w v := by
        intro v
        by_cases hv : v ∈ verts
        · let v0 : Vert := ⟨v, hv⟩
          have hSub :
              ({c ∈ A | c.1.1 = v} : Finset Copy) ⊆ ({c : Copy | c.1 = v0} : Finset Copy) := by
            intro c hc
            have hcEq : c.1.1 = v := (Finset.mem_filter.mp hc).2
            have hcVert : c.1 = v0 := by
              apply Subtype.ext
              simpa [v0] using hcEq
            simp [hcVert]
          calc
            ({c ∈ A | c.1.1 = v} : Finset Copy).card ≤ ({c : Copy | c.1 = v0} : Finset Copy).card :=
              Finset.card_le_card hSub
            _ = w v := by
              simpa [v0] using (copiesWith_card (v0 := v0))
        · have hEmpty : ({c ∈ A | c.1.1 = v} : Finset Copy) = ∅ := by
            ext c
            constructor
            · intro hc
              have hcEq : c.1.1 = v := (Finset.mem_filter.mp hc).2
              have hv' : v ∈ verts := by
                simpa [hcEq] using c.1.2
              exact False.elim (hv hv')
            · intro hc
              cases hc
          simp [hEmpty]

      have hdecomp : A.card = ∑ v ∈ S, ({c ∈ A | c.1.1 = v} : Finset Copy).card := by
        simpa [S] using (Finset.card_eq_sum_card_image (f := fun c : Copy => c.1.1) (s := A))
      have hsum : A.card ≤ ∑ v ∈ S, w v := by
        calc
          A.card = ∑ v ∈ S, ({c ∈ A | c.1.1 = v} : Finset Copy).card := hdecomp
          _ ≤ ∑ v ∈ S, w v := by
            refine Finset.sum_le_sum ?_
            intro v hvS
            exact hfiber v
      simpa [wealthSum, S] using hsum

    have hCutsUpper : wealthSum (V := V) w S ≤ internalCapacity G S + cutCapacity G S :=
      (hCuts S).2

    -- Count tokens whose edge is internal-or-cut relative to `S`.
    have hTokCard :
        ({t : Token | IsInternal (V := V) S t.1.1 ∨ IsCut (V := V) S t.1.1} : Finset Token).card
          = internalCapacity G S + cutCapacity G S := by
      have h1 :
          ({t : Token | IsInternal (V := V) S t.1.1 ∨ IsCut (V := V) S t.1.1} : Finset Token).card
            = ∑ e ∈ G.edges, if (IsInternal (V := V) S e ∨ IsCut (V := V) S e) then G.cap e else 0 :=
        card_tokens_filter_edge (p := fun e : Sym2 V => IsInternal (V := V) S e ∨ IsCut (V := V) S e)
      have h2 :
          (∑ e ∈ G.edges, if (IsInternal (V := V) S e ∨ IsCut (V := V) S e) then G.cap e else 0)
            = ∑ e ∈ G.edges, edgeBound G S e := by
        refine Finset.sum_congr rfl ?_
        intro e heE
        by_cases hI : IsInternal (V := V) S e
        · simp [edgeBound, hI]
        · by_cases hC : IsCut (V := V) S e
          · simp [edgeBound, hI, hC]
          · simp [edgeBound, hI, hC]
      have h3 : (∑ e ∈ G.edges, edgeBound G S e) = internalCapacity G S + cutCapacity G S := by
        simpa using (sum_edgeBound_eq (V := V) (G := G) S)
      calc
        ({t : Token | IsInternal (V := V) S t.1.1 ∨ IsCut (V := V) S t.1.1} : Finset Token).card
            = ∑ e ∈ G.edges, if (IsInternal (V := V) S e ∨ IsCut (V := V) S e) then G.cap e else 0 := h1
        _ = ∑ e ∈ G.edges, edgeBound G S e := h2
        _ = internalCapacity G S + cutCapacity G S := h3

    -- These tokens are all neighbors (each has an endpoint in `S`, hence a witness in `A`).
    have hSub :
        ({t : Token | IsInternal (V := V) S t.1.1 ∨ IsCut (V := V) S t.1.1} : Finset Token) ⊆
          ({t : Token | ∃ c ∈ A, r c t} : Finset Token) := by
      intro t ht
      have ht' : IsInternal (V := V) S t.1.1 ∨ IsCut (V := V) S t.1.1 := by
        simpa using ht
      rcases t with ⟨e0, i⟩
      rcases e0 with ⟨e, heEdges⟩
      cases e using Sym2.ind with
      | _ u v =>
        -- token edge is `s(u, v)`
        cases ht' with
        | inl hInt =>
          have huv : u ∈ S ∧ v ∈ S := by
            simpa using hInt
          have huS : u ∈ S := huv.1
          rcases (Finset.mem_image).1 huS with ⟨c, hcA, hcEq⟩
          refine Finset.mem_filter.mpr ?_
          refine ⟨by simp, ⟨c, hcA, ?_⟩⟩
          simp [r, hcEq, Sym2.mem_iff]
        | inr hCut =>
          have hC : (u ∈ S ∧ v ∉ S) ∨ (u ∉ S ∧ v ∈ S) := by
            simpa using hCut
          rcases hC with ⟨huS, _⟩ | ⟨_, hvS⟩
          · rcases (Finset.mem_image).1 huS with ⟨c, hcA, hcEq⟩
            refine Finset.mem_filter.mpr ?_
            refine ⟨by simp, ⟨c, hcA, ?_⟩⟩
            simp [r, hcEq, Sym2.mem_iff]
          · rcases (Finset.mem_image).1 hvS with ⟨c, hcA, hcEq⟩
            refine Finset.mem_filter.mpr ?_
            refine ⟨by simp, ⟨c, hcA, ?_⟩⟩
            simp [r, hcEq, Sym2.mem_iff]

    have hNeighbors :
        internalCapacity G S + cutCapacity G S ≤ ({t : Token | ∃ c ∈ A, r c t} : Finset Token).card := by
      calc
        internalCapacity G S + cutCapacity G S
            = ({t : Token | IsInternal (V := V) S t.1.1 ∨ IsCut (V := V) S t.1.1} : Finset Token).card := by
                symm; exact hTokCard
        _ ≤ ({t : Token | ∃ c ∈ A, r c t} : Finset Token).card := Finset.card_le_card hSub

    exact Nat.le_trans (Nat.le_trans hA_le hCutsUpper) hNeighbors

  rcases (Fintype.all_card_le_filter_rel_iff_exists_injective (r := r)).1 hall with
    ⟨f, hf_inj, hf_rel⟩

  -- Compute `card Copy = card Token` from the `S = verts` cut constraint.
  have hCardCopy : Fintype.card Copy = wealthSum (V := V) w verts := by
    have h1' : Fintype.card Copy = ∑ v : Vert, Fintype.card (Fin (w v.1)) := by
      dsimp [Copy]
      exact (Fintype.card_sigma (α := fun v : Vert => Fin (w v.1)))
    have h1 : Fintype.card Copy = ∑ v : Vert, w v.1 := by
      calc
        Fintype.card Copy = ∑ v : Vert, Fintype.card (Fin (w v.1)) := h1'
        _ = ∑ v : Vert, w v.1 := by simp
    have h2 : (∑ v : Vert, w v.1) = ∑ x ∈ verts, w x := by
      calc
        (∑ v : Vert, w v.1) = ∑ v ∈ (Finset.univ : Finset Vert), w v.1 := by simp
        _ = ∑ v ∈ verts.attach, w v.1 := by
              simp [Finset.univ_eq_attach, Vert]
        _ = ∑ x ∈ verts, w x := by
              simpa using (Finset.sum_attach (s := verts) (f := w))
    simpa [wealthSum] using h1.trans h2

  have hCardToken : Fintype.card Token = ∑ e ∈ G.edges, G.cap e := by
    have h1' : Fintype.card Token = ∑ e : Edge, Fintype.card (Fin (G.cap e.1)) := by
      dsimp [Token]
      exact (Fintype.card_sigma (α := fun e : Edge => Fin (G.cap e.1)))
    have h1 : Fintype.card Token = ∑ e : Edge, G.cap e.1 := by
      calc
        Fintype.card Token = ∑ e : Edge, Fintype.card (Fin (G.cap e.1)) := h1'
        _ = ∑ e : Edge, G.cap e.1 := by simp
    have h2 : (∑ e : Edge, G.cap e.1) = ∑ e ∈ G.edges, G.cap e := by
      calc
        (∑ e : Edge, G.cap e.1) = ∑ e ∈ (Finset.univ : Finset Edge), G.cap e.1 := by simp
        _ = ∑ e ∈ G.edges.attach, G.cap e.1 := by
              simp [Finset.univ_eq_attach, Edge]
        _ = ∑ e ∈ G.edges, G.cap e := by
              simpa using (Finset.sum_attach (s := G.edges) (f := G.cap))
    exact h1.trans h2

  have hcard : Fintype.card Copy = Fintype.card Token := by
    simp [hCardCopy, hCardToken, hw_total]

  have hf_surj : Function.Surjective f := by
    by_contra hns
    have hlt := Fintype.card_lt_of_injective_not_surjective (f := f) hf_inj hns
    exact (Nat.ne_of_lt hlt) hcard

  let equiv : Copy ≃ Token := Equiv.ofBijective f ⟨hf_inj, hf_surj⟩

  -- For every token, its matched copy is adjacent (uses `hf_rel`).
  have hRel_token : ∀ t : Token, r (equiv.symm t) t := by
    intro t
    have hf_eq : f (equiv.symm t) = t := by
      change equiv (equiv.symm t) = t
      exact equiv.apply_symm_apply t
    have := hf_rel (equiv.symm t)
    simpa [hf_eq] using this

  -- Construct a liquidity state by counting matched tokens on each edge endpoint.
  let l : LiquidityFn V :=
    fun e x =>
      if he : e ∈ G.edges then
        if hx : x ∈ verts then
          ({t : Token | t.1 = (⟨e, he⟩ : Edge) ∧ (equiv.symm t).1 = (⟨x, hx⟩ : Vert)} : Finset Token).card
        else
          0
      else
        0

  have hOff : LiquidityFn.OffGraphZero (V := V) G l := by
    intro e he x
    simp [l, he]

  have hNI : LiquidityFn.NonIncidentZero (V := V) l := by
    intro e x hxNot
    by_cases he : e ∈ G.edges
    · by_cases hxVerts : x ∈ verts
      · have hEmpty :
            ({t : Token | t.1 = (⟨e, he⟩ : Edge) ∧ (equiv.symm t).1 = (⟨x, hxVerts⟩ : Vert)} : Finset Token) = ∅ := by
          ext t
          constructor
          · intro ht
            have htEdge : t.1 = (⟨e, he⟩ : Edge) := (by simpa using (And.left (by simpa using ht)))
            have htVert : (equiv.symm t).1 = (⟨x, hxVerts⟩ : Vert) := (by simpa using (And.right (by simpa using ht)))
            have htEdgeVal : t.1.1 = e := by
              simpa using congrArg Subtype.val htEdge
            have hr' : (equiv.symm t).1.1 ∈ t.1.1 := by
              simpa [r] using (hRel_token t)
            have hxIn : x ∈ e := by
              have : (equiv.symm t).1.1 = x := by
                simpa using congrArg Subtype.val htVert
              -- rewrite `t`'s edge and vertex
              simpa [htEdgeVal, this] using hr'
            exact False.elim (hxNot hxIn)
          · intro ht
            cases ht
        simp [l, he, hxVerts, hEmpty]
      · simp [l, he, hxVerts]
    · simp [l, he]

  have hCon : LiquidityFn.Conservation (V := V) G l := by
    intro u v hEdge
    -- endpoints are incident
    have huVerts : u ∈ verts :=
      mem_incidentVerts_of_mem_edges (V := V) (G := G) hEdge (v := u) (by simp)
    have hvVerts : v ∈ verts :=
      mem_incidentVerts_of_mem_edges (V := V) (G := G) hEdge (v := v) (by simp)
    let u0 : Vert := ⟨u, huVerts⟩
    let v0 : Vert := ⟨v, hvVerts⟩
    have huv0 : u0 ≠ v0 := by
      intro hEq
      apply (G.ne_of_mem_edges (u := u) (v := v) hEdge)
      exact congrArg Subtype.val hEq

    let Tedge : Finset Token := {t : Token | t.1 = (⟨s(u, v), hEdge⟩ : Edge)}
    have hTedge_card : Tedge.card = G.cap (s(u, v)) := by
      have := tokensWith_card (e0 := (⟨s(u, v), hEdge⟩ : Edge))
      simpa [Tedge] using this

    have hMaps :
        Set.MapsTo (fun t : Token => (equiv.symm t).1) (↑Tedge : Set Token) (↑({u0, v0} : Finset Vert) : Set Vert) := by
      intro t ht
      have htEdge : t.1 = (⟨s(u, v), hEdge⟩ : Edge) := by simpa [Tedge] using ht
      have htEdgeVal : t.1.1 = s(u, v) := by
        simpa using congrArg Subtype.val htEdge
      have hr' : (equiv.symm t).1.1 ∈ t.1.1 := by
        simpa [r] using (hRel_token t)
      have hxIn : (equiv.symm t).1.1 ∈ s(u, v) := by
        simpa [htEdgeVal] using hr'
      have hxEq : (equiv.symm t).1.1 = u ∨ (equiv.symm t).1.1 = v := by
        simpa [Sym2.mem_iff] using hxIn
      rcases hxEq with hxEq | hxEq
      · have : (equiv.symm t).1 = u0 := by
          apply Subtype.ext
          exact hxEq
        simp [this]
      · have : (equiv.symm t).1 = v0 := by
          apply Subtype.ext
          exact hxEq
        simp [this]

    have hSplit :
        Tedge.card = ∑ x ∈ ({u0, v0} : Finset Vert), ({t ∈ Tedge | (equiv.symm t).1 = x} : Finset Token).card := by
      simpa using (Finset.card_eq_sum_card_fiberwise (f := fun t : Token => (equiv.symm t).1) (s := Tedge)
        (t := ({u0, v0} : Finset Vert)) hMaps)

    have hLu :
        ({t ∈ Tedge | (equiv.symm t).1 = u0} : Finset Token).card = l (s(u, v)) u := by
      classical
      have hSet :
          ({t ∈ Tedge | (equiv.symm t).1 = u0} : Finset Token) =
            ({t : Token | t.1 = (⟨s(u, v), hEdge⟩ : Edge) ∧ (equiv.symm t).1 = u0} : Finset Token) := by
        ext t
        constructor
        · intro ht
          have ht' := Finset.mem_filter.mp ht
          have htEdge : t.1 = (⟨s(u, v), hEdge⟩ : Edge) := by
            simpa [Tedge] using ht'.1
          have : t.1 = (⟨s(u, v), hEdge⟩ : Edge) ∧ (equiv.symm t).1 = u0 := ⟨htEdge, ht'.2⟩
          simpa using this
        · intro ht
          have ht' :
              t.1 = (⟨s(u, v), hEdge⟩ : Edge) ∧ (equiv.symm t).1 = u0 := by
            simpa using ht
          refine Finset.mem_filter.mpr ?_
          refine ⟨?_, ht'.2⟩
          show t ∈ Tedge
          simp [Tedge, ht'.1]
      have hl :
          l (s(u, v)) u =
            ({t : Token | t.1 = (⟨s(u, v), hEdge⟩ : Edge) ∧ (equiv.symm t).1 = u0} : Finset Token).card := by
        simp [l, hEdge, huVerts, u0]
      -- rewrite the LHS finset to match `l`
      simp [hl, hSet]
    have hLv :
        ({t ∈ Tedge | (equiv.symm t).1 = v0} : Finset Token).card = l (s(u, v)) v := by
      classical
      have hSet :
          ({t ∈ Tedge | (equiv.symm t).1 = v0} : Finset Token) =
            ({t : Token | t.1 = (⟨s(u, v), hEdge⟩ : Edge) ∧ (equiv.symm t).1 = v0} : Finset Token) := by
        ext t
        constructor
        · intro ht
          have ht' := Finset.mem_filter.mp ht
          have htEdge : t.1 = (⟨s(u, v), hEdge⟩ : Edge) := by
            simpa [Tedge] using ht'.1
          have : t.1 = (⟨s(u, v), hEdge⟩ : Edge) ∧ (equiv.symm t).1 = v0 := ⟨htEdge, ht'.2⟩
          simpa using this
        · intro ht
          have ht' :
              t.1 = (⟨s(u, v), hEdge⟩ : Edge) ∧ (equiv.symm t).1 = v0 := by
            simpa using ht
          refine Finset.mem_filter.mpr ?_
          refine ⟨?_, ht'.2⟩
          show t ∈ Tedge
          simp [Tedge, ht'.1]
      have hl :
          l (s(u, v)) v =
            ({t : Token | t.1 = (⟨s(u, v), hEdge⟩ : Edge) ∧ (equiv.symm t).1 = v0} : Finset Token).card := by
        simp [l, hEdge, hvVerts, v0]
      simp [hl, hSet]

    -- Assemble the conservation equation.
    have :
        G.cap (s(u, v)) = l (s(u, v)) u + l (s(u, v)) v := by
      have hSplit2 :
          Tedge.card =
            ({t ∈ Tedge | (equiv.symm t).1 = u0} : Finset Token).card +
              ({t ∈ Tedge | (equiv.symm t).1 = v0} : Finset Token).card := by
        calc
          Tedge.card =
              ∑ x ∈ ({u0, v0} : Finset Vert),
                ({t ∈ Tedge | (equiv.symm t).1 = x} : Finset Token).card := hSplit
          _ =
              ({t ∈ Tedge | (equiv.symm t).1 = u0} : Finset Token).card +
                ({t ∈ Tedge | (equiv.symm t).1 = v0} : Finset Token).card := by
                simp [huv0]
      calc
        G.cap (s(u, v)) = Tedge.card := hTedge_card.symm
        _ =
            ({t ∈ Tedge | (equiv.symm t).1 = u0} : Finset Token).card +
              ({t ∈ Tedge | (equiv.symm t).1 = v0} : Finset Token).card := hSplit2
        _ = l (s(u, v)) u + l (s(u, v)) v := by
              rw [hLu, hLv]
    exact this.symm

  have hLG : l ∈ LiquidityFn.LG (V := V) G := by
    exact ⟨hCon, hNI, hOff⟩

  have hPi : Wealth.pi (V := V) G l = w := by
    funext x
    by_cases hxVerts : x ∈ verts
    · -- Count tokens assigned to `x` and partition them by edge.
      let x0 : Vert := ⟨x, hxVerts⟩
      let copiesAt : Finset Copy := {c : Copy | c.1 = x0}
      let tokensAt : Finset Token := {t : Token | (equiv.symm t).1 = x0}

      have hTokensAt : tokensAt = copiesAt.image equiv := by
        ext t
        constructor
        · intro ht
          refine Finset.mem_image.mpr ?_
          refine ⟨equiv.symm t, ?_, ?_⟩
          · simpa [tokensAt, copiesAt] using ht
          · simp
        · intro ht
          rcases Finset.mem_image.mp ht with ⟨c, hc, rfl⟩
          have : (equiv.symm (equiv c)).1 = x0 := by
            simpa [copiesAt] using hc
          simpa [tokensAt] using this

      have tokensAt_card : tokensAt.card = w x := by
        have hc : copiesAt.card = w x := by
          have := copiesWith_card (v0 := x0)
          simpa [copiesAt] using this
        have : tokensAt.card = copiesAt.card := by
          simpa [hTokensAt] using (Finset.card_image_of_injective (s := copiesAt) equiv.injective)
        simp [this, hc]

      have hMaps :
          Set.MapsTo (fun t : Token => t.1) (↑tokensAt : Set Token) (↑(Finset.univ : Finset Edge) : Set Edge) := by
        intro t ht
        simp

      have hDecomp :
          tokensAt.card = ∑ e0 ∈ (Finset.univ : Finset Edge), ({t ∈ tokensAt | t.1 = e0} : Finset Token).card := by
        simpa using
          (Finset.card_eq_sum_card_fiberwise (f := fun t : Token => t.1) (s := tokensAt)
            (t := (Finset.univ : Finset Edge)) hMaps)

      have hFiberTerm :
          ∀ e0 : Edge, ({t ∈ tokensAt | t.1 = e0} : Finset Token).card = l e0.1 x := by
        intro e0
        classical
        have hl :
            l e0.1 x =
              ({t : Token | t.1 = e0 ∧ (equiv.symm t).1 = x0} : Finset Token).card := by
          simp [l, e0.2, hxVerts, x0]
        have hSet :
            ({t ∈ tokensAt | t.1 = e0} : Finset Token) =
              ({t : Token | t.1 = e0 ∧ (equiv.symm t).1 = x0} : Finset Token) := by
          ext t
          constructor
          · intro ht
            have ht' := Finset.mem_filter.mp ht
            have htTokEq : (equiv.symm t).1 = x0 := by
              simpa [tokensAt] using ht'.1
            have : t.1 = e0 ∧ (equiv.symm t).1 = x0 := ⟨ht'.2, htTokEq⟩
            simpa using this
          · intro ht
            have ht' : t.1 = e0 ∧ (equiv.symm t).1 = x0 := by
              simpa using ht
            refine Finset.mem_filter.mpr ?_
            refine ⟨?_, ht'.1⟩
            show t ∈ tokensAt
            simpa [tokensAt] using ht'.2
        simp [hl, hSet]

      have hDecomp' : tokensAt.card = ∑ e0 ∈ (Finset.univ : Finset Edge), l e0.1 x := by
        calc
          tokensAt.card =
              ∑ e0 ∈ (Finset.univ : Finset Edge), ({t ∈ tokensAt | t.1 = e0} : Finset Token).card := hDecomp
          _ = ∑ e0 ∈ (Finset.univ : Finset Edge), l e0.1 x := by
            refine Finset.sum_congr rfl ?_
            intro e0 he0
            rw [hFiberTerm e0]

      have hEdgeSum :
          (∑ e0 ∈ (Finset.univ : Finset Edge), l e0.1 x) = ∑ e ∈ G.edges, l e x := by
        have hUniv : (Finset.univ : Finset Edge) = G.edges.attach := by
          dsimp [Edge]
        calc
          (∑ e0 ∈ (Finset.univ : Finset Edge), l e0.1 x) =
              ∑ e0 ∈ G.edges.attach, l e0.1 x := by
                rw [hUniv]
          _ = ∑ e ∈ G.edges, l e x := by
                simpa using (Finset.sum_attach (s := G.edges) (f := fun e => l e x))

      have : Wealth.pi (V := V) G l x = w x := by
        have : ∑ e ∈ G.edges, l e x = w x := by
          calc
            ∑ e ∈ G.edges, l e x = tokensAt.card := by
              exact (hEdgeSum.symm.trans hDecomp'.symm)
            _ = w x := tokensAt_card
        simpa [Wealth.pi, Wealth.wealth] using this
      exact this
    · -- outside `verts`, both sides are zero
      have hw : w x = 0 := hw_out x hxVerts
      have : Wealth.pi (V := V) G l x = 0 := by
        simp [Wealth.pi, Wealth.wealth, l, hxVerts]
      simp [hw, this]

  exact ⟨l, hLG, hPi⟩

theorem mem_WG_iff_forall_cutIntervalHolds {G : ChannelGraph V} {w : V → Cap} :
    w ∈ Wealth.WG (G := G) ↔ ∀ S : Finset V, cutIntervalHolds (V := V) G w S := by
  constructor
  · intro hw S
    exact cutIntervalHolds_of_mem_WG (V := V) (G := G) (w := w) hw S
  · intro hCuts
    exact mem_WG_of_forall_cutIntervalHolds (V := V) (G := G) (w := w) hCuts

end Cuts

end PaymentChannels
end NucleusDB
