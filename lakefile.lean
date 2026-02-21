import Lake
open Lake DSL

package nucleusdb where
  leanOptions := #[
    ⟨`autoImplicit, false⟩
  ]

lean_lib HeytingLean where
  srcDir := "lean"

@[default_target]
lean_lib NucleusDB where
  srcDir := "lean"
