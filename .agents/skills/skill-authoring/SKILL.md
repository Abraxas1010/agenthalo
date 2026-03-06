# Skill: skill-authoring

> **Trigger:** create a skill, write a skill, new skill, improve skill, skill template, skill structure, skill development
> **Category:** infrastructure
> **Audience:** Internal (hardwired) + External (controlling agent)

## Credo Alignment

Reference: `.agents/CREDO.md`

This skill serves **Imperative II: Search Before Building** and **Imperative V: Respect the Collaboration**.

Before creating a new skill:
1. Search for existing skills that might already cover the domain
2. Understand the collaborative context — skills are shared resources
3. Align with the philosophical foundation: skills exist to serve trust-building

---

## Well-Formed Outcome

- **Outcome**: Agent creates a complete, discoverable skill with CREDO alignment and knowledge graph
- **Evidence**: Skill file exists, symlinks resolve, knowledge graph present, CREDO imperative stated
- **Context**: Use when a new workflow or capability needs to be codified for agent discovery
- **Resources**: `.agents/CREDO.md`, this skill's templates, existing skills for reference
- **Ecology**: Fits the skill discovery system; does not duplicate existing skills

---

## When to Use

| Scenario | Why This Skill |
|----------|----------------|
| New workflow needs codifying | Creates a discoverable, structured skill |
| Existing skill needs improvement | Provides quality checklist and structure |
| Agent keeps making the same mistake | Document the correct pattern as a skill |

## When NOT to Use

| Scenario | Use Instead | Why |
|----------|-------------|-----|
| Quick one-off instructions | Inline docs or README | Skills are for reusable patterns |
| Adding an MCP tool | `skill-maintenance` | Different lifecycle |
| Updating existing skill triggers | `skill-maintenance` | Maintenance, not authoring |

---

## Required Skill Sections

Every skill must include:

1. **Header block** — Trigger phrases, Category, Audience
2. **Purpose** — What this skill achieves
3. **Workflow** — Step-by-step with embedded commands/examples
4. **Common Mistakes** — What goes wrong and how to avoid it (if applicable)

### Recommended Additional Sections

- **Well-Formed Outcome** — Outcome, Evidence, Context, Resources, Ecology
- **When to Use / When NOT to Use** — Contrast tables
- **Failure Modes** — Cost of misuse
- **Example Trace** — Concrete successful invocation
- **Related Skills** — Cross-references

---

## Skill Directory Structure

```
.agents/skills/
├── skill-name/
│   ├── SKILL.md              # Core definition (required)
│   ├── skill-name.kg.md      # Knowledge graph (required)
│   ├── references/            # Detailed docs (optional)
│   └── examples/              # Working examples (optional)
```

---

## Knowledge Graph Requirement

Every skill must have a companion `.kg.md` file. This provides machine-queryable structure for the skill's concepts, tools, and relationships.

### Knowledge Graph Template

```markdown
# Knowledge Graph: skill-name

## Metadata
- domain: [domain]
- version: 1.0.0
- skill-ref: .agents/skills/skill-name/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| [concept] | Pattern/Tool/Concept | [what it means] |

### Tools
| Entity | Type | Integration |
|--------|------|-------------|
| [MCP tool or CLI] | MCP/CLI | [how used] |

## Relationships
- [concept] ENABLES [outcome]
- [tool] SUPPORTS [workflow]

## Cross-References
- Related skills: [list]
- CREDO imperatives served: [list]
```

---

## Skill Creation Process

### 1. Search First (Imperative II)

```bash
# Check for existing skills
ls .agents/skills/

# Check AGENTS.md skill routing table
grep -i "<keyword>" AGENTS.md
```

### 2. Define the Outcome

Answer:
- What specific outcome does this skill achieve?
- How will success be verified?
- When should this skill NOT be used?

### 3. Create Structure

```bash
mkdir -p .agents/skills/skill-name
touch .agents/skills/skill-name/{SKILL.md,skill-name.kg.md}
```

### 4. Write SKILL.md

Include the required sections. Reference `.agents/CREDO.md` and state which Imperative(s) the skill serves.

### 5. Create Knowledge Graph

The `.kg.md` file is **required** for every skill.

### 6. Create Symlinks

```bash
# For each agent surface
ln -s ../../.agents/skills/skill-name .claude/skills/skill-name
ln -s ../../.agents/skills/skill-name .codex/skills/skill-name
ln -s ../../.agents/skills/skill-name .gemini/skills/skill-name
```

### 7. Update AGENTS.md

Add a row to the skill routing table in `AGENTS.md`.

### 8. Validate

- [ ] CREDO alignment stated
- [ ] Knowledge graph created
- [ ] Symlinks in `.claude/skills/`, `.codex/skills/`, `.gemini/skills/`
- [ ] AGENTS.md routing table updated
- [ ] Well-formed outcome defined (recommended)

---

## Failure Modes

| Misuse | Consequence | Cost |
|--------|-------------|------|
| Creating duplicate skill | Fragmented knowledge, confusion | Maintenance burden |
| Skipping CREDO alignment | Skill lacks philosophical grounding | Inconsistent behavior |
| Missing knowledge graph | No machine-queryable structure | Limited integration |
| No symlinks | Skill invisible to some agents | Partial discovery |
| Too much in SKILL.md | Slow loading, context waste | Performance hit |

---

## Example Trace

Creating a new skill `dashboard-theming`:

```
Step 1: Search for existing skills
        → ls .agents/skills/ | grep -i theme
        → No existing skill found

Step 2: Create directory + files
        → mkdir -p .agents/skills/dashboard-theming
        → touch SKILL.md dashboard-theming.kg.md

Step 3: Write SKILL.md
        → Header: triggers, category, audience
        → CREDO alignment: Imperative III (optimize)
        → Workflow: step-by-step theming process
        → Common mistakes: forgetting touch assets.rs

Step 4: Create knowledge graph
        → Entities: CSS variables, rust-embed, style.css
        → Relationships: theme REQUIRES rebuild, rebuild REQUIRES touch

Step 5: Create symlinks
        → ln -s in .claude/skills/, .codex/skills/, .gemini/skills/

Step 6: Update AGENTS.md
        → Add row to skill routing table

Output: Complete skill package ready for agent use
```

---

## Related Skills

- `skill-maintenance` — Lifecycle management for existing skills
- `orchestrator-quickstart` — Example of well-structured workflow skill
- `halo-trace-inspection` — Example of detailed technical skill
