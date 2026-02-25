## Summary

- What changed
- Why it changed

## Component

- [ ] Core (protocol, immutable, persistence)
- [ ] SQL
- [ ] MCP server
- [ ] AgentHALO
- [ ] Dashboard (web UI / API)
- [ ] Contracts (Solidity)
- [ ] Formal spec (Lean 4)
- [ ] Docs / CI

## Verification

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets`
- [ ] `cargo test`
- [ ] Contract tests pass (if applicable): `cd contracts && forge test`

## Safety Checklist

- [ ] Fail-closed behavior preserved
- [ ] Persistence/transparency behavior reviewed
- [ ] No weakening of verification or auth checks
- [ ] Docs updated for interface changes

## Notes

Additional context, risks, or follow-up items.
