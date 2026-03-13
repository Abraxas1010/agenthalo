# OpenClaw Chat Adapters Deferred

Date: 2026-03-13
Project: `openclaw_parity_removal_20260313`

The PM's Telegram and Slack webhook adapters are optional Phase 2 work. They were intentionally deferred in this boundary for two reasons:

1. OpenClaw removal no longer depends on them because AgentHALO already has a working Discord surface and now has native memory + scheduling parity.
2. Removing OpenClaw atomically first keeps the dispatch and dashboard cleanup auditable and reduces the risk of mixing optional new integrations into a deletion-heavy commit.

This means the OpenClaw removal commit is complete for parity-critical behavior, while future chat-adapter work can be landed independently if requested.
