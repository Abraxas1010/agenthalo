# Skill: agent-lifecycle

> **Trigger:** agent kind, agent type, agent capabilities, agent environment, vault env, shell agent, claude agent, codex agent, gemini agent, openclaw agent, agent command, agent CLI flags
> **Category:** orchestrator
> **Audience:** Internal (hardwired) + External (controlling agent)

## Purpose

Definitive reference for how each agent kind is launched, what CLI flags are injected, how environment variables and vault secrets are resolved, and the agent state machine.

---

## Agent Kinds

| Kind | CLI Command | Static Args | Env Removed |
|------|-------------|-------------|-------------|
| `claude` | `claude` | `--print --output-format json --verbose --dangerously-skip-permissions` | `CLAUDECODE` |
| `codex` | `codex` | `exec --full-auto --json --skip-git-repo-check` | `CODEX_CLI` |
| `gemini` | `gemini` | `--yolo` | (none) |
| `openclaw` | `openclaw` | `run --non-interactive` | (none) |
| `shell` | `sh` | `-c` | `ENV`, `BASH_ENV`, `PROMPT_COMMAND` |

### How the Task Prompt Is Passed

| Kind | Mechanism | Example |
|------|-----------|---------|
| `claude` | Positional arg after static args | `claude --print ... "your prompt"` |
| `codex` | Positional arg after static args | `codex exec --full-auto --json ... "your prompt"` |
| `gemini` | `--prompt` flag | `gemini --yolo --prompt "your prompt"` |
| `openclaw` | `--message` flag | `openclaw run --non-interactive --message "your prompt"` |
| `shell` | Arg to `sh -c` | `sh -c "your prompt"` |

### Model Selection

Pass `model` in the launch request to add `--model <model>` to claude/codex/gemini. Ignored for shell/openclaw.

```json
{"agent": "claude", "agent_name": "fast", "model": "claude-sonnet-4-6", "timeout_secs": 60}
```

---

## Agent State Machine

```
  ┌──────────┐
  │  (new)   │
  └────┬─────┘
       │ orchestrator_launch
       ▼
  ┌──────────┐     orchestrator_send_task      ┌──────────┐
  │   Idle   │ ──────────────────────────────> │   Busy   │
  └──────────┘ <────────────────────────────── └──────────┘
       │            task completes                  │
       │                                            │ task completes/fails
       │ orchestrator_stop                          │
       ▼                                            ▼
  ┌──────────┐                               (returns to Idle)
  │ Stopped  │
  └──────────┘
```

**Key rules:**
- `Idle` → `Busy`: only via `orchestrator_send_task`
- `Busy` → `Idle`: automatic when task completes/fails/times out
- **Cannot send task to `Busy` agent** — returns error "agent is busy"
- **Cannot send task to `Stopped` agent** — returns error "agent is stopped"
- Stopping a `Busy` agent cancels in-flight tasks (marks them as failed)

---

## Environment Variables

### Explicit Environment

Pass `env` in the launch request:

```json
{
  "agent": "claude",
  "agent_name": "worker",
  "env": {
    "CUSTOM_VAR": "value",
    "OPENAI_API_KEY": "vault:openai"
  }
}
```

### Vault Resolution

Values prefixed with `vault:` are resolved from the encrypted API key vault:

| Env value | Resolution |
|-----------|------------|
| `"vault:openai"` | Vault key for provider "openai" |
| `"vault:anthropic"` | Vault key for provider "anthropic" |
| `"vault:google"` | Vault key for provider "google" |
| `"sk-hardcoded"` | Passed as-is (not recommended for production) |

Vault must be initialized (`agenthalo auth` or PQ wallet at `~/.agenthalo/pq_wallet.json`).

### Removed Environment Variables

To prevent nested agent recursion, certain env vars are stripped:
- `CLAUDECODE` removed for `claude` agents (prevents inner Claude from detecting outer Claude)
- `CODEX_CLI` removed for `codex` agents
- `ENV`, `BASH_ENV`, `PROMPT_COMMAND` removed for `shell` agents (prevents login shell init stalls)

If you explicitly set a removed var in `env`, your explicit value takes precedence (removal is skipped).

---

## PTY Session Management

Each task execution creates a new PTY session via `portable-pty`:
- Terminal size: 120 cols x 24 rows
- One reader thread per session (blocking read loop → broadcast channel)
- Session destroyed after task completes or agent stops

### Max Concurrent Agents

- **Hard limit:** 64 managed agents (`MAX_MANAGED_AGENTS`)
- **PTY limit:** 10 concurrent PTY sessions (`PtyManager` default)
- In practice, PTY limit is the binding constraint — only 10 tasks can run simultaneously

### Container Budget

A `ContainerBudget` can restrict launches per orchestrator instance:

| Field | Default | Description |
|-------|---------|-------------|
| `max_agents` | 64 | Total managed agents across all kinds |
| `max_concurrent_busy` | 10 | Maximum agents in `Busy` state simultaneously |
| `allowed_kinds` | (all) | Restrict launch to specific kinds (`shell`, `claude`, etc.) |

When set, `orchestrator_launch` rejects launches that exceed `max_agents` or violate
`allowed_kinds`, and task starts are rejected when `max_concurrent_busy` is reached.

### Working Directory

Pass `working_dir` to control where the agent executes:

```json
{"agent": "claude", "agent_name": "reviewer", "working_dir": "/home/user/project"}
```

- Claude: uses `--cwd` flag (not working_dir)
- Codex/Gemini/OpenClaw: ignored
- Shell: sets the PTY process working directory

---

## Task Lifecycle

### One Agent, Many Tasks

An agent persists across tasks. After a task completes:
1. PTY session is destroyed
2. Agent status returns to `Idle`
3. `tasks_completed` counter increments
4. `total_cost_usd` accumulates
5. Next `orchestrator_send_task` creates a NEW PTY session

This means each task gets a clean environment — no state leaks between tasks.

### Task Retention

- Completed tasks are retained for 24 hours (`TASK_RETENTION_SECS = 86_400`)
- Maximum 2,000 retained tasks (`MAX_TASKS_RETAINED`)
- Pruning by age (oldest first) happens automatically

---

## Stopping Agents

### Graceful Stop (`force: false`)

1. Sends `^C` (byte 0x03 = SIGINT) to the PTY
2. Waits up to 1 second (20 × 50ms polls) for process to exit
3. If still running, terminates the PTY forcefully
4. Destroys the PTY session
5. Cancels all in-flight tasks (marks as failed with "agent stopped")

### Force Stop (`force: true`)

1. Immediately terminates the PTY
2. Destroys the PTY session
3. Cancels all in-flight tasks

### Cleanup Recommendation

Always stop agents when done. Leaked agents consume PTY slots:

```json
// List agents to find active ones
// orchestrator_list → check for agents with status != "stopped"

// Stop each one
{"agent_id": "orch-...", "force": false}
```

---

## Mesh Network Status

When mesh networking is enabled (`NUCLEUSDB_MESH_AGENT_ID` is set):
- `orchestrator_mesh_status` returns peer topology, reachability, and latency
- Cockpit renders a read-only mesh sidebar with peer online/offline state
- Peers are read from the shared peer registry (`/data/mesh/peers.json` by default)

When mesh is disabled, `orchestrator_mesh_status` returns:

```json
{"enabled": false, "self_agent_id": null, "peers": [], "network_name": null}
```

---

## Codex Launch Compatibility Guardrail

Do not reintroduce deprecated Codex flags in docs or code:
- `--quiet`
- `--approval-mode full-auto`

Use the current non-interactive form:

```bash
codex exec --full-auto --json --skip-git-repo-check "your prompt"
```

If Codex CLI changes upstream, verify with `codex --help` and `codex exec --help`
before updating this skill or orchestrator launch templates.
