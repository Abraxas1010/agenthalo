# OpenClaw Capability Audit

Date: 2026-03-13
Project: `openclaw_parity_removal_20260313`

## Mapping

| OpenClaw feature | AgentHALO equivalent | Status | Evidence |
| --- | --- | --- | --- |
| Multi-channel chat routing | Discord bot in `src/discord/` | partial | Discord already ships; Telegram/Slack adapters remain optional Phase 2 work |
| Persistent conversational memory | NucleusDB memory + orchestrator-local recall bridge | ready | `src/orchestrator/mod.rs` now persists prompt/result turns and recalls prior context for non-shell agents |
| Multi-agent session isolation | Orchestrator agent pool + session manager | ready | `src/orchestrator/agent_pool.rs`, `src/orchestrator/dispatch.rs` |
| Model-agnostic LLM routing | Local/cloud proxy surfaces | ready | Existing Claude/Codex/Gemini/Shell/runtime routing remains intact after OpenClaw removal |
| Skills / plugin ecosystem | MCP tool surfaces | ready | Existing MCP registry replaces OpenClaw-specific tool hub model |
| Proactive scheduling | Native orchestrator delayed task scheduling | ready | `src/orchestrator/mod.rs`, `/orchestrator/schedule`, `orchestrator_schedule_task` |
| Browser automation | Agent/tool responsibility | n/a | No OpenClaw-only requirement retained |
| CLI agent detection + install | Existing CLI/dashboard metadata | ready | OpenClaw removed; remaining supported agents continue through existing metadata surfaces |

## Outcome

OpenClaw's useful capabilities are either already present in AgentHALO or have now been absorbed natively:

- conversation memory is persisted and recalled through the orchestrator path
- delayed task scheduling is exposed without OpenClaw
- the remaining OpenClaw-specific code can be removed without losing required functionality

## Deferred Scope

Telegram and Slack adapters were left out of this commit boundary. The PM marks them optional, and they are not required to complete OpenClaw parity removal safely.

## Verification Notes

Relevant gates completed on this surface:

- `cargo fmt --all`
- `cargo test orchestrator:: --lib -- --nocapture`
- `cargo test --release --test container_tests -- --nocapture`
- `cargo test --release --test dashboard_tests api_orchestrator -- --nocapture`
- `cargo test --release container_lock_status_tool_reports_current_container --lib -- --nocapture`
- `cargo build --release --bin agenthalo --bin agenthalo-mcp-server`

The full `cargo test --release` sweep still has unrelated existing failures in `dashboard_tests` tied to missing local embedding assets and a P2PCLAW verification mismatch. Those failures are outside the OpenClaw removal surface.
