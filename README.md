<img src="assets/Apoth3osis.webp" alt="Apoth3osis Logo" width="140"/>

<sub><strong>Our tech stack is ontological:</strong><br>
<strong>Hardware — Physics</strong><br>
<strong>Software — Mathematics</strong><br><br>
<strong>Our engineering workflow is simple:</strong> discover, build, grow, learn & teach</sub>

---

<sub>
<strong>Acknowledgment</strong><br>
We humbly thank the collective intelligence of humanity for providing the technology and culture we cherish. We do our best to properly reference the authors of the works utilized herein, though we may occasionally fall short. Our formalization acts as a reciprocal validation—confirming the structural integrity of their original insights while securing the foundation upon which we build. In truth, all creative work is derivative; we stand on the shoulders of those who came before, and our contributions are simply the next link in an unbroken chain of human ingenuity.
</sub>

---

# NucleusDB

[![License: Apoth3osis License Stack v1](https://img.shields.io/badge/License-Apoth3osis%20License%20Stack%20v1-blue.svg)](LICENSE.md)

**Verifiable database with vector commitments, post-quantum signatures, and Certificate Transparency.**

## What Is NucleusDB

NucleusDB is a verifiable database engine with append-only transparency proofs, multi-tenant RBAC, witness signatures, and cryptographic query verification.

## Features

- Vector commitment backends: `ipa`, `kzg`, `binary_merkle`
- RFC 6962-style transparency tree and consistency verification
- Multi-tenant access control with `Reader`/`Writer`/`Admin` roles
- WAL + checkpoint persistence with replay validation
- Evidence generation and replay verification utilities

## Installation

```bash
git clone https://github.com/Abraxas1010/nucleusdb.git
cd nucleusdb
cargo build
```

## Quick Start

```bash
cargo run --bin nucleusdb-server -- 127.0.0.1:8088 production
cargo test
```

## SQL Reference

Planned subset includes `INSERT`, `SELECT`, `UPDATE`, `DELETE`, `SHOW STATUS`, `SHOW HISTORY`, `COMMIT`, and `CHECKPOINT`.

## CLI Usage

Phase 1 includes extraction and server mode. Full CLI/REPL commands are scheduled for the next phase.

## TUI

A ratatui terminal interface is planned for the next phase (`nucleusdb-tui`).

## MCP Server

A Rust MCP server is planned for the next phase (`nucleusdb-mcp`).

## HTTP API

Current server binary exposes tenant registration, query, commit, snapshot/checkpoint, principal registration, list, and health endpoints.

## Architecture

Core modules cover protocol, commitments, transparency, witness signatures, persistence, audit, RBAC, and HTTP API.

## Formal Specifications

Lean formal modules are planned to be copied in the formal-spec phase.

## Security

- ML-DSA-65 default witness signing
- Binary Merkle backend available for post-quantum-safe hash-based commitments
- Constant-time comparisons for authentication checks

## License

[Apoth3osis License Stack v1](LICENSE.md)
