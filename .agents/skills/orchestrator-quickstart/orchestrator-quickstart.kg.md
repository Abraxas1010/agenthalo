# Knowledge Graph: orchestrator-quickstart

## Metadata
- domain: orchestrator
- version: 1.0.0
- skill-ref: .agents/skills/orchestrator-quickstart/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| Launch-Task-Result | Pattern | Core 3-step workflow for agent orchestration |
| Agent Session | Concept | Managed agent with agent_id, status (idle/busy/stopped) |
| Task Response | Concept | Contains answer, result, output, error, exit_code, usage |
| Answer Extraction | Pattern | Claude JSON array → clean text via extract_claude_answer |
| Wait Semantics | Concept | wait:true blocks, wait:false returns task_id for polling |
| Timeout Cascade | Pattern | Launch timeout → default; send_task timeout → override |

### Tools
| Entity | Type | Integration |
|--------|------|-------------|
| orchestrator_launch | MCP | Creates managed agent session |
| orchestrator_send_task | MCP | Submits task to agent |
| orchestrator_get_result | MCP | Polls task status/result |
| orchestrator_list | MCP | Lists all agents |
| orchestrator_tasks | MCP | Lists all tasks |
| orchestrator_stop | MCP | Stops agent, releases PTY |

## Relationships
- Launch-Task-Result ENABLES multi-agent workflows
- orchestrator_launch PRODUCES Agent Session
- orchestrator_send_task PRODUCES Task Response
- Answer Extraction RESOLVES Claude JSON array parsing
- Wait Semantics GOVERNS blocking vs async behavior
- Timeout Cascade GOVERNS per-task timeout resolution
- orchestrator_stop REQUIRES agent_id from Agent Session

## Cross-References
- Related skills: orchestrator-pipes, agent-lifecycle, mcp-transport
- CREDO imperatives served: I (Trust — verify results), II (Search — check existing agents)
