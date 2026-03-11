# Skill: orchestrator-quickstart

> **Trigger:** orchestrate agents, launch agent, send task, multi-agent, orchestrator tutorial, agent coordination
> **Category:** orchestrator
> **Audience:** Internal (hardwired) + External (controlling agent)

## Purpose

Step-by-step guide for launching managed agents, sending tasks, reading results, and building task DAGs via the AgentHALO orchestrator MCP tools.

---

## Prerequisites

The orchestrator runs inside `nucleusdb-mcp` (HTTP mode) or `agenthalo dashboard` (embedded).

```bash
# HTTP mode (standalone)
nucleusdb-mcp --http --port 9876 --no-auth --db /tmp/experiment.ndb

# Embedded mode (dashboard includes orchestrator)
agenthalo dashboard --port 3100
```

---

## Workflow: Launch → Task → Result

### Step 1: Launch an Agent

Use `orchestrator_launch` to create a managed agent session.

```json
{
  "agent": "claude",
  "agent_name": "reviewer",
  "timeout_secs": 600,
  "trace": true,
  "capabilities": ["memory_read", "memory_write"]
}
```

**Agent kinds:** `claude`, `codex`, `gemini`, `openclaw`, `shell`

**Response gives you `agent_id`** — save it, you need it for every subsequent call:
```json
{
  "agent_id": "orch-1772765418-d08b7ac4",
  "status": "idle",
  "agent": "claude",
  "agent_name": "reviewer",
  "capabilities": ["memory_read", "memory_write"]
}
```

### Step 2: Send a Task

Use `orchestrator_send_task` with the `agent_id` from step 1.

```json
{
  "agent_id": "orch-1772765418-d08b7ac4",
  "task": "Read src/main.rs and list any bugs",
  "wait": true,
  "timeout_secs": 120
}
```

**Critical: `wait` parameter behavior:**
- `wait: true` (default) — blocks until task completes/fails/times out, then returns the full result
- `wait: false` — returns immediately with `status: "running"` and a `task_id` for polling

### Step 3: Read the Result

The response from `orchestrator_send_task` (when `wait: true`) or `orchestrator_get_result` contains:

```json
{
  "task_id": "task-1772765810-25076429",
  "agent_id": "orch-1772765418-d08b7ac4",
  "status": "complete",
  "answer": "The extracted assistant answer (Claude-specific)",
  "result": "Full raw output from the agent (may include ANSI, JSON, etc.)",
  "output": "Same as result (backward-compatible alias)",
  "error": null,
  "exit_code": 0,
  "input_tokens": 1500,
  "output_tokens": 800,
  "cost_usd": 0.042
}
```

**CRITICAL — Which field to read:**

| Field | Content | When to use |
|-------|---------|-------------|
| `answer` | Extracted assistant text (Claude: parsed from JSON output) | **Prefer this** — clean, human-readable |
| `result` | Full raw output (may contain JSON arrays, ANSI codes, etc.) | When `answer` is null or you need raw data |
| `output` | Identical to `result` (backward-compatible alias) | Legacy — prefer `result` |
| `error` | Error message if task failed/timed out | Only populated on failure |

### Step 4: Poll Async Tasks

If you used `wait: false`, poll with `orchestrator_get_result`:

```json
{
  "task_id": "task-1772765810-25076429",
  "wait": true,
  "timeout_secs": 60
}
```

### Step 5: Stop Agent

When done, stop the agent to release its PTY:

```json
{
  "agent_id": "orch-1772765418-d08b7ac4",
  "force": false
}
```

`force: false` sends SIGINT first (graceful). `force: true` kills immediately.

---

## Common Mistakes (from live testing)

### 1. Shell agents: use short-lived commands only

Shell agents use `sh -c "<your task>"` — each task spawns a new `sh -c` process. The task string IS the shell command:
```json
{"task": "echo hello && date && hostname"}
```

Do NOT send multi-line scripts or interactive commands. For long work, use a Claude or Codex agent.

### 2. Claude output is a JSON array

Claude CLI with `--output-format json` emits a single JSON array `[{...},{...}]`, not one JSON object per line. The orchestrator's `extract_claude_answer` handles this — use the `answer` field.

### 3. Agent reuse across tasks

An agent can run multiple sequential tasks. After a task completes, the agent returns to `idle` status and can accept another `orchestrator_send_task`. You do NOT need to launch a new agent per task.

However, agents cannot run concurrent tasks — sending a task to a `busy` agent returns an error.

### 4. Timeout semantics

- `timeout_secs` on `orchestrator_launch` sets the DEFAULT per-task timeout for that agent
- `timeout_secs` on `orchestrator_send_task` OVERRIDES the default for that specific task
- Maximum: 3600 seconds (1 hour)
- On timeout: task status becomes `"timeout"`, error field explains

---

## Quick Reference: All Orchestrator MCP Tools

| Tool | Purpose | Key params |
|------|---------|------------|
| `orchestrator_launch` | Create managed agent | `agent`, `agent_name`, `timeout_secs`, `trace`, `capabilities` |
| `orchestrator_send_task` | Submit task to agent | `agent_id`, `task`, `wait`, `timeout_secs` |
| `orchestrator_get_result` | Poll task status/result | `task_id`, `wait`, `timeout_secs` |
| `orchestrator_pipe` | Create DAG edge between tasks | `source_task_id`, `target_agent_id`, `transform` |
| `orchestrator_list` | List all agents | (no params) |
| `orchestrator_tasks` | List all tasks | (no params) |
| `orchestrator_graph` | Task graph snapshot | (no params) |
| `orchestrator_stop` | Stop agent, finalize session | `agent_id`, `force` |

---

## Valid Capabilities

| Capability | Meaning |
|------------|---------|
| `*` | All capabilities (superuser) |
| `memory_read` | Read from NucleusDB |
| `memory_write` | Write to NucleusDB |
| `sql_read` | Execute SQL SELECT |
| `sql_write` | Execute SQL INSERT/UPDATE/DELETE |
| `container_launch` | Launch Docker containers |
| `orchestrator_pipe` | Create pipe edges in task graph |

Default (if empty): `["memory_read", "memory_write"]`
