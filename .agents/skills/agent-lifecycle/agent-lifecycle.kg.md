# Knowledge Graph: agent-lifecycle

## Metadata
- domain: orchestrator
- version: 1.0.0
- skill-ref: .agents/skills/agent-lifecycle/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Agent Kinds
| Entity | Type | Description |
|--------|------|-------------|
| claude | Agent Kind | `claude --print --output-format json --verbose --dangerously-skip-permissions` |
| codex | Agent Kind | `codex exec --full-auto --json --skip-git-repo-check` |
| gemini | Agent Kind | `gemini --yolo` |
| openclaw | Agent Kind | `openclaw run --non-interactive` |
| shell | Agent Kind | `sh -c` (NOT `-lc`) |

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| Agent State Machine | Pattern | Idle → Busy → Idle or Stopped |
| Vault Resolution | Pattern | `vault:provider` prefix resolves encrypted API keys |
| Env Removal | Pattern | Specific env vars stripped per agent kind to prevent recursion |
| PTY Session Per Task | Pattern | Each task creates fresh PTY — no state leaks |
| Task Retention | Concept | 24h retention, max 2000 tasks, oldest-first pruning |
| Graceful Stop | Pattern | SIGINT → wait 1s → force terminate |

### Tools
| Entity | Type | Integration |
|--------|------|-------------|
| orchestrator_launch | MCP | Creates agent with kind, capabilities, timeout |
| orchestrator_stop | MCP | Graceful or force stop |

## Relationships
- Agent Kind DETERMINES CLI command and static args
- Agent State Machine GOVERNS task acceptance (only Idle accepts tasks)
- Vault Resolution ENABLES secure API key injection
- Env Removal PREVENTS agent recursion (e.g., CLAUDECODE for claude)
- PTY Session Per Task ENSURES clean environment between tasks
- shell kind USES `sh -c` NOT `sh -lc` (prevents login shell stalls)
- Graceful Stop SENDS SIGINT first, THEN force after 1s
- Task Retention LIMITS storage to 24h / 2000 tasks

## Cross-References
- Related skills: orchestrator-quickstart, orchestrator-pipes, halo-trace-inspection
- CREDO imperatives served: I (Trust — secure vault resolution), V (Collaborate — clean agent lifecycle)
