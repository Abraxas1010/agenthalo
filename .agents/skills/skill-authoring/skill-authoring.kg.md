# Knowledge Graph: skill-authoring

## Metadata
- domain: infrastructure
- version: 1.0.0
- skill-ref: .agents/skills/skill-authoring/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| Skill Structure | Pattern | SKILL.md + .kg.md + optional references/examples |
| CREDO Alignment | Pattern | Every skill must reference an Imperative from .agents/CREDO.md |
| Knowledge Graph | Concept | Machine-queryable .kg.md companion file with entities and relationships |
| Symlink Topology | Pattern | .agents/skills/ → .claude/skills/, .codex/skills/, .gemini/skills/ |
| AGENTS.md Routing Table | Concept | Central discovery table mapping skills to trigger keywords |
| Well-Formed Outcome | Pattern | Outcome + Evidence + Context + Resources + Ecology |

### Process Steps
| Entity | Type | Description |
|--------|------|-------------|
| Search First | Step | Check existing skills before creating (Imperative II) |
| Define Outcome | Step | Specify what the skill achieves and how to verify |
| Create Structure | Step | mkdir + touch SKILL.md + .kg.md |
| Write Content | Step | Required sections + CREDO alignment |
| Create KG | Step | Entities, tools, relationships |
| Create Symlinks | Step | Link into .claude/, .codex/, .gemini/ |
| Update AGENTS.md | Step | Add routing table row |
| Validate | Step | Checklist verification |

## Relationships
- Skill Structure CONTAINS SKILL.md and Knowledge Graph
- CREDO Alignment REQUIRED_BY every skill
- Knowledge Graph ENABLES machine-queryable discovery
- Symlink Topology ENABLES multi-agent discovery
- AGENTS.md Routing Table ENABLES trigger-based skill matching
- Search First PREVENTS duplicate skill creation
- Well-Formed Outcome DEFINES success criteria

## Cross-References
- Related skills: skill-maintenance
- CREDO imperatives served: II (Search Before Building), V (Respect the Collaboration)
