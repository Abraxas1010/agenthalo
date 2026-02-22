# Contributing

## Scope

NucleusDB prioritizes correctness, fail-closed behavior, and reproducibility.
Contributions should preserve those properties.

## Local Setup

```bash
cargo build
cargo test
```

## Pull Request Expectations

- keep changes scoped and reviewable
- add or update tests for behavior changes
- preserve fail-closed semantics in verification paths
- avoid adding hidden defaults that reduce safety
- update docs for user-visible interface changes

## Commit Conventions

- use clear, scoped commit messages
- avoid bundling unrelated changes in one commit

## Quality Gate

Before opening a PR:

```bash
cargo fmt
cargo test
```

## Security-Sensitive Changes

For crypto, transparency, witness auth, or persistence code:

- document threat model impact
- include regression tests for tamper/failure paths
- avoid weakening existing policy checks
