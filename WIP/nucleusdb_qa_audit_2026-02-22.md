# NucleusDB QA Audit — 2026-02-22

Status: PASS
Scope: structural + user-facing workflows (CLI, SQL script mode, HTTP API, MCP, TUI)
Commit under test: working tree based on `128bcdf`

## Structural QA

- `cargo fmt`: PASS
- `cargo test`: PASS
  - unit: 4/4
  - integration: 29/29
  - keymap: 3/3
  - persistence compat: 2/2
  - sql: 5/5
  - cli smoke: 2/2
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS

## User-Oriented QA

### CLI + SQL + Persistence

Flow executed:
1. `nucleusdb create --db <tmp> --backend merkle`
2. `nucleusdb sql --db <tmp> <script.sql>`
3. `nucleusdb status --db <tmp>`
4. `nucleusdb export --db <tmp>`
5. negative case: `VERIFY;`

Result: PASS
- Mixed SQL script with standard SQL + custom commands now executes correctly.
- `VERIFY;` returns clear usage error.
- Status shows human-readable UTC timestamp.
- Snapshot/WAL remains consistent.

### HTTP API (production policy)

Flow executed:
1. start server with `--policy production`
2. `GET /v1/health`
3. register tenant with secure `witness_seed` and `witness_signing_algorithm=ml_dsa65`
4. commit and query
5. negative auth case

Result: PASS
- Health/readiness OK.
- Registration/commit/query successful under production-safe config.
- Fail-closed auth behavior confirmed.

### MCP (stdio JSON-RPC)

Flow executed:
1. initialize handshake
2. `tools/list`
3. `tools/call` for `nucleusdb_help`
4. `tools/call` negative test for `nucleusdb_create_database` with missing required `db_path`

Result: PASS
- `serverInfo.name = nucleusdb`
- 11 tools listed (including `nucleusdb_help`)
- Help tool returns SQL/backend/policy usage guidance
- Missing required `db_path` is rejected

### TUI Smoke

Flow executed:
- TTY-backed launch with timeout

Result: PASS
- Renders all core UI frame elements and status tab without panic in TTY context.

## Issues Found During Audit

### Fixed immediately

1. Mixed SQL scripts containing custom commands (e.g., `VERIFY`) failed parsing in batch mode.
- Root cause: parser path handled whole script before custom command dispatch.
- Fix: add statement splitter with quote awareness and execute script statement-by-statement.
- Files: `src/sql/executor.rs`
- Regression test: `sql_script_mixes_custom_and_standard_statements` in `tests/sql_tests.rs`

2. Strict clippy gate failed.
- Root causes: nested-if lint + function argument count + test initialization style.
- Fixes:
  - collapse nested if in `execute_select`
  - targeted `#[allow(clippy::too_many_arguments)]` on two public API functions where signatures are explicit by design
  - convert `ParameterSet` test setup to struct update syntax
- Files: `src/sql/executor.rs`, `src/multitenant.rs`, `src/security.rs`, `tests/end_to_end.rs`

## Final Verdict

PASS (fail-closed, reproducible, and user-facing paths verified).
