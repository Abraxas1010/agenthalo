# Knowledge Graph: skill-maintenance

## Metadata
- domain: infrastructure
- version: 1.0.0
- skill-ref: .agents/skills/skill-maintenance/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| Skill Lifecycle | Pattern | Create → Symlink → Update AGENTS.md → Validate |
| Symlink Management | Concept | .agents/skills/<name> symlinked into .claude/, .codex/, .gemini/ |
| Routing Table | Concept | AGENTS.md table mapping skills to triggers |
| Validation Checklist | Pattern | SKILL.md + .kg.md + symlinks + AGENTS.md + CREDO |
| Commit Hygiene | Pattern | Stage only changed skill files + AGENTS.md |

### Operations
| Entity | Type | Description |
|--------|------|-------------|
| Add Skill | Operation | Full lifecycle from creation to validation |
| Update Skill | Operation | Edit SKILL.md, update AGENTS.md if triggers changed |
| Remove Skill | Operation | Remove directory, symlinks, and AGENTS.md row |
| Validate Skills | Operation | Run checklist across all skills |

## Relationships
- Skill Lifecycle GOVERNS all skill operations
- Symlink Management ENABLES multi-agent discovery
- Routing Table ENABLES trigger-based matching
- Validation Checklist ENSURES completeness
- Add Skill FOLLOWS Skill Lifecycle
- Update Skill MODIFIES source of truth (SKILL.md)
- Remove Skill CLEANS all references
- Commit Hygiene PREVENTS accidental staging

## Cross-References
- Related skills: skill-authoring
- CREDO imperatives served: V (Respect the Collaboration — maintain shared resources)
