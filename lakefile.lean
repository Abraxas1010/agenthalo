import Lake
open Lake DSL

package nucleusdb where
  leanOptions := #[
    ⟨`autoImplicit, false⟩
  ]

@[default_target]
lean_lib NucleusDB where
  srcDir := "lean"
