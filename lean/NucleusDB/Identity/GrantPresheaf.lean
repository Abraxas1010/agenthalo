namespace HeytingLean
namespace NucleusDB
namespace Identity

/-- POD key-pattern namespace for identity sharing. -/
abbrev KeyPattern := String

/-- Pattern inclusion relation matching runtime-style key semantics:
`*` includes everything, exact equality includes itself, and `stem/*`
includes any key with the same `stem` prefix. -/
def PatternLe (Q P : KeyPattern) : Prop :=
  P = "*"
    ∨ Q = P
    ∨ ∃ stem qTail, P = stem ++ "/*" ∧ Q = stem ++ qTail

theorem PatternLe.refl (P : KeyPattern) : PatternLe P P := by
  exact Or.inr (Or.inl rfl)

/-- Presheaf over identity key patterns. -/
structure IdentityPresheaf where
  carrier : KeyPattern → Type
  restrict : ∀ P Q, PatternLe Q P → carrier P → carrier Q
  restrictId : ∀ P (h : PatternLe P P) (σ : carrier P), restrict P P h σ = σ
  restrictCompose :
    ∀ P Q R (h₁ : PatternLe Q P) (h₂ : PatternLe R Q) (h₃ : PatternLe R P) (σ : carrier P),
      restrict Q R h₂ (restrict P Q h₁ σ) = restrict P R h₃ σ

/-- Concrete identity-grant section payload. -/
structure GrantSection where
  visibleKeys : List String
  deriving DecidableEq, Repr

/-- Canonical identity grant presheaf where restrictions are identity maps
    under runtime-style inclusion witnesses. -/
def grantPresheaf : IdentityPresheaf where
  carrier := fun _ => GrantSection
  restrict := by
    intro _ _ _ σ
    exact σ
  restrictId := by
    intro _ _ σ
    rfl
  restrictCompose := by
    intro _ _ _ _ _ _ σ
    rfl

/-- Revocation in the base model is irreversible for the same snapshot:
    once a key is removed from a section, restriction cannot re-introduce it. -/
theorem revocation_irreversible_local
    (P : KeyPattern) (σ : GrantSection) (removed : String)
    (hRemoved : removed ∉ (σ.visibleKeys.erase removed))
    (h : PatternLe P P) :
    removed ∉ (grantPresheaf.restrict P P h { visibleKeys := σ.visibleKeys.erase removed }).visibleKeys := by
  simpa using hRemoved

end Identity
end NucleusDB
end HeytingLean
