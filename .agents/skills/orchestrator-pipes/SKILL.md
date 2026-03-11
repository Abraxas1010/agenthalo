# Skill: orchestrator-pipes

> **Trigger:** pipe tasks, task dag, chain agents, transform output, pipe transform, agent pipeline, orchestrator graph
> **Category:** orchestrator
> **Audience:** Internal (hardwired) + External (controlling agent)

## Purpose

Guide for creating task-graph pipes that chain output from one agent's task into another agent's input, with optional transforms. This is how you build multi-agent workflows.

---

## Core Concept

A pipe creates a DAG edge: when the source task completes, its output is transformed and sent as a new task to the target agent.

```
Agent A: task-123 (complete) ──[transform]──> Agent B: task-456 (auto-created)
```

---

## Creating a Pipe

Use `orchestrator_pipe`:

```json
{
  "source_task_id": "task-1772765810-25076429",
  "target_agent_id": "orch-1772765858-4536a0b9",
  "transform": "claude_answer",
  "task_prefix": "Summarize this: "
}
```

**Timing matters:**
- If the source task is **already complete**: the pipe immediately creates and runs the target task. Response includes the `task_id` of the generated task.
- If the source task is **still running**: the pipe is registered as a pending edge. When the task completes, the follow-up task is auto-dispatched. Response has `status: "linked"` and `task_id: null`.

---

## Available Transforms

| Transform | Syntax | Behavior |
|-----------|--------|----------|
| Identity | `"identity"` or omit | Pass raw output through unchanged |
| Claude Answer | `"claude_answer"` | Extract the `answer` field (parsed assistant text); falls back to raw output |
| JSON Extract | `"json_extract:.path.to.field"` | Dot-notation JSON path extraction (supports `field[0]` array indexing) |
| Prefix | `"prefix:Your prefix text "` | Prepend text to the output |
| Suffix | `"suffix: your suffix"` | Append text to the output |

**Aliases:** `"assistant_answer"` is accepted as an alias for `"claude_answer"`.

### Combining transform + task_prefix

The `task_prefix` parameter adds a `Prefix` transform that runs BEFORE the main transform. This lets you compose instructions with extracted data:

```json
{
  "source_task_id": "task-abc",
  "target_agent_id": "orch-reviewer",
  "transform": "claude_answer",
  "task_prefix": "Review this code and find bugs:\n\n"
}
```

Result: `"Review this code and find bugs:\n\n<claude's answer from task-abc>"`

### JSON Extract Examples

```json
// Extract .result from JSON output
{"transform": "json_extract:.result"}

// Extract nested field
{"transform": "json_extract:.data.summary"}

// Extract array element
{"transform": "json_extract:.items[0].name"}
```

---

## Multi-Agent Pipeline Pattern

### Pattern: Code Review Pipeline

```
1. Launch writer (claude) and reviewer (claude)
2. Send coding task to writer (wait: true)
3. Pipe writer's answer to reviewer with prefix "Review this code:"
4. Read reviewer's result
```

```python
# Pseudocode
writer = orchestrator_launch(agent="claude", agent_name="writer")
reviewer = orchestrator_launch(agent="claude", agent_name="reviewer")

# Step 1: Write code
task = orchestrator_send_task(
    agent_id=writer.agent_id,
    task="Write a Python function to compute fibonacci",
    wait=True
)

# Step 2: Pipe to reviewer
pipe_result = orchestrator_pipe(
    source_task_id=task.task_id,
    target_agent_id=reviewer.agent_id,
    transform="claude_answer",
    task_prefix="Review this code for bugs and suggest improvements:\n\n"
)

# Step 3: Read reviewer's result
review = orchestrator_get_result(
    task_id=pipe_result.task_id,
    wait=True,
    timeout_secs=120
)
```

### Pattern: Shell → Claude Analysis

```
1. Launch shell and analyst (claude)
2. Send data-gathering command to shell
3. Pipe shell output to analyst with analysis instructions
```

```json
// Step 1: Shell gathers data
{"agent_id": "orch-shell", "task": "wc -l src/**/*.rs", "wait": true}

// Step 2: Pipe to analyst
{
  "source_task_id": "task-shell-result",
  "target_agent_id": "orch-analyst",
  "transform": "identity",
  "task_prefix": "Analyze these line counts and identify the largest files:\n\n"
}
```

---

## Inspecting the Graph

Use `orchestrator_graph` (no params) to see the full task DAG:

```json
{
  "graph": {
    "nodes": {
      "task-123": {"task_id": "task-123", "agent_id": "orch-writer", "status": "complete", "depends_on": []},
      "task-456": {"task_id": "task-456", "agent_id": "orch-reviewer", "status": "running", "depends_on": []}
    },
    "edges": [
      {
        "source_task_id": "task-123",
        "target_agent_id": "orch-reviewer",
        "transform": {"claude_answer": null},
        "generated_task_id": "task-456"
      }
    ]
  },
  "node_count": 2,
  "edge_count": 1,
  "nodes_shape": "object_map"
}
```

**CRITICAL:** `graph.nodes` is an **object/map** keyed by `task_id`, NOT an array. Iterate with `.items()` / `Object.entries()`, not with array indexing.

---

## Cycle Prevention

The orchestrator rejects edges that would create cycles at the **agent** level:

- `A → B → A` is rejected (agent A sends to B, B sends back to A)
- `A → A` is rejected (self-loop)
- `A → B` and `A → C` is allowed (fan-out)
- `A → C` and `B → C` is allowed (fan-in)

Cycle detection uses reachability analysis on the agent-level dependency graph, not just direct edges.

---

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Unknown `source_task_id` | Returns error: "unknown source_task_id" |
| Unknown `target_agent_id` | Edge is created (target checked at dispatch time) |
| Target agent is stopped | Follow-up task fails with "agent is stopped" |
| Target agent is busy | Follow-up task fails with "agent is busy" |
| Unknown transform syntax | Returns error: "unknown pipe transform" |
| Cycle detected | Returns error: "pipe edge would introduce a cycle" |
