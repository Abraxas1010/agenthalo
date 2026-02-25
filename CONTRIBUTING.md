# Contributing to NucleusDB

Thank you for your interest in contributing. NucleusDB prioritizes correctness, fail-closed behavior, and reproducibility. Contributions should preserve those properties.

## Getting Started

### Prerequisites

- Rust toolchain (stable, 1.84+)
- For contract work: [Foundry](https://getfoundry.sh/) (`forge`, `cast`, `anvil`)
- For formal specs: [Lean 4](https://leanprover.github.io/) toolchain

### Build and Test

```bash
# Build everything
cargo build

# Run all Rust tests (202 tests)
cargo test

# Run Solidity tests (34 tests, requires Foundry)
cd contracts && forge test

# Build formal specs (requires Lean 4)
lake build NucleusDB

# Check formatting and lints
cargo fmt --check
cargo clippy --all-targets
```

## What to Contribute

### High-Impact Areas

- **Bug fixes** with regression tests
- **New commitment backends** implementing the `VectorCommitment` trait
- **Agent adapters** for additional AI coding agents (AgentHALO)
- **Formal proofs** extending the Lean 4 specification
- **Security audits** and hardening

### Before You Start

For non-trivial changes, open an issue first to discuss the approach. This saves time for both sides.

## Pull Request Process

### Expectations

- Keep changes scoped and reviewable
- Add or update tests for behavior changes
- Preserve fail-closed semantics in verification paths
- Avoid adding hidden defaults that reduce safety
- Update docs for user-visible interface changes

### Commit Conventions

- Use clear, scoped commit messages: `[COMPONENT] description`
  - e.g., `[SQL] reject multi-statement batches in append-only mode`
  - e.g., `[AGENTHALO] add Codex adapter for JSON output parsing`
- Avoid bundling unrelated changes in one commit

### Quality Gate

Before opening a PR:

```bash
cargo fmt
cargo clippy --all-targets
cargo test
```

All three must pass with zero warnings from the changed code.

## Code Organization

| Directory | Contents |
|-----------|----------|
| `src/` | Core library and binary entry points |
| `src/halo/` | AgentHALO observability layer |
| `src/dashboard/` | Web dashboard (axum server, API, rust-embed assets) |
| `dashboard/` | Frontend SPA (HTML, CSS, JS — embedded at compile time) |
| `src/vc/` | Vector commitment backends (Merkle, IPA, KZG) |
| `src/sql/` | SQL parser and executor |
| `src/mcp/` | MCP server (local stdio + remote HTTP) |
| `src/trust/` | On-chain trust attestation |
| `src/puf/` | Hardware PUF integration |
| `contracts/` | Solidity smart contracts (Foundry) |
| `lean/` | Lean 4 formal specifications |
| `tests/` | Integration and end-to-end tests |
| `Docs/` | Extended documentation |

## Security-Sensitive Changes

For crypto, transparency, witness auth, persistence, or AgentHALO credential handling code:

- Document threat model impact
- Include regression tests for tamper/failure paths
- Avoid weakening existing policy checks
- Ensure fail-closed behavior: if verification cannot complete, the operation must fail, not silently succeed

If you're unsure whether a change is security-sensitive, treat it as though it is.

## Reporting Security Issues

See [SECURITY.md](SECURITY.md) for responsible disclosure procedures.

## License

By contributing, you agree that your contributions will be licensed under the [Apoth3osis License Stack v1](LICENSE.md).
