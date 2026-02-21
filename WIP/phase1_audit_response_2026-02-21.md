# NucleusDB Phase 1 Audit Response

Date: 2026-02-21
Baseline commit: `9fec132d50b004e3433936f51afa98a86a5f22f9`
Audit status: PASS

## Resolution

The Phase 1 audit is accepted in full. The baseline is treated as locked for extraction/foundation scope.

Re-verified before proceeding:
- `cargo test --lib`: 4/4 PASS
- `cargo test --test end_to_end`: 29/29 PASS
- No `heyting` references in repository sources/docs
- No `kernel_ffi` references in repository
- Height/WAL consistency remains correct:
  - `src/protocol.rs`: `height = (self.entries.len() as u64) + 1`
  - `src/persistence.rs`: `seq = db.entries.len() as u64`
  - `src/persistence.rs`: `event.entry.height != key_seq` guard

## Notes Addressed

1. `rmcp` + `schemars` omitted in Phase 1:
   - Decision: keep omitted until Phase 4 MCP implementation.
   - Rationale: stubs compile without MCP runtime deps; avoids premature dependency surface.

2. Phase 2+ deps in `Cargo.toml`:
   - Decision: retained (`sqlparser`, `clap`, `rustyline`, `ratatui`, `crossterm`) to allow immediate Phase 2/3/5 work without dependency churn.

3. `keymap: KeyMap` not present in `NucleusDb`:
   - Decision: implement in Phase 2 as planned with persistence integration.

## Phase 2 Entry Criteria (Now Satisfied)

- Phase 1 audit accepted and documented.
- Baseline tests passing on standalone repo.
- No blocked extraction defects.
- KZG trusted setup fixture present and tracked.

## Phase 2 First Deliverables

1. Add `src/keymap.rs` with deterministic key→index mapping semantics.
2. Extend `NucleusDb` with `keymap: KeyMap` and initialize in constructors.
3. Persist/restore keymap in snapshot and WAL metadata with backward-compatible serde defaults.
4. Add SQL module skeleton and initial executor tests (`INSERT`, `SELECT`, `COMMIT`, `SHOW STATUS`).

