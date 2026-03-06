# Skill: skill-maintenance

> **Trigger:** add skill, register skill, symlink skills, update skill, skill inventory, skill validation
> **Category:** infrastructure
> **Audience:** Internal (hardwired) + External (controlling agent)

## Credo Alignment

Reference: `.agents/CREDO.md`

This skill serves **Imperative V: Respect the Collaboration** — skills are shared resources; maintaining them well serves all agents.

---

## Purpose

Make skills discoverable and safe to reuse by all agents. This skill covers the lifecycle of skill management: adding, updating, validating, and removing skills.

---

## When to Use

- You added a new `.agents/skills/<name>/SKILL.md` and want it discoverable
- You need to update an existing skill's triggers or content
- You want to verify all skills are properly linked and documented

## When NOT to Use

| Scenario | Use Instead | Why |
|----------|-------------|-----|
| Creating a brand new skill from scratch | `skill-authoring` | Authoring covers full creation process |
| Understanding the orchestrator | `orchestrator-quickstart` | Domain-specific skill |
| Adding a Rust module or API endpoint | `Docs/ARCHITECTURE.md` | Not a skill concern |

---

## Skill Lifecycle (Order Matters)

### 1. Create Skill

Add `.agents/skills/<skill-name>/SKILL.md` with:
- Header block (Trigger, Category, Audience)
- CREDO alignment
- Workflow instructions
- Knowledge graph (`.kg.md`)

### 2. Create Symlinks

```bash
ln -s ../../.agents/skills/<skill-name> .claude/skills/<skill-name>
ln -s ../../.agents/skills/<skill-name> .codex/skills/<skill-name>
ln -s ../../.agents/skills/<skill-name> .gemini/skills/<skill-name>
```

### 3. Update AGENTS.md

Add a row to the skill routing table:

```markdown
| `skill-name` | trigger1, trigger2, trigger3 | category |
```

### 4. Verify Discovery

```bash
# Check symlinks resolve
ls -la .claude/skills/<skill-name>/SKILL.md
ls -la .codex/skills/<skill-name>/SKILL.md
ls -la .gemini/skills/<skill-name>/SKILL.md

# Check AGENTS.md has the entry
grep "skill-name" AGENTS.md
```

---

## Updating Existing Skills

When modifying a skill:

1. Edit `.agents/skills/<skill-name>/SKILL.md` (the source of truth)
2. If triggers changed, update the routing table in `AGENTS.md`
3. Symlinks don't need updating — they point to the directory, not the file

---

## Removing Skills

1. Remove the skill directory: `rm -r .agents/skills/<skill-name>`
2. Remove all symlinks:
   ```bash
   rm .claude/skills/<skill-name>
   rm .codex/skills/<skill-name>
   rm .gemini/skills/<skill-name>
   ```
3. Remove the row from `AGENTS.md` skill routing table

---

## Validation Checklist

Run this when adding or modifying skills:

- [ ] `SKILL.md` exists in `.agents/skills/<name>/`
- [ ] Knowledge graph (`.kg.md`) exists alongside `SKILL.md`
- [ ] Symlinks exist in `.claude/skills/`, `.codex/skills/`, `.gemini/skills/`
- [ ] All symlinks resolve (not dangling)
- [ ] `AGENTS.md` routing table includes the skill
- [ ] CREDO alignment is stated
- [ ] `CLAUDE.md`, `CODEX.md`, `GEMINI.md` all symlink to `AGENTS.md`

### Quick Validation Script

```bash
echo "=== Skill Validation ==="
for skill in .agents/skills/*/SKILL.md; do
  name=$(basename $(dirname "$skill"))
  echo -n "$name: "
  # Check symlinks
  ok=true
  for surface in .claude .codex .gemini; do
    if [ ! -L "$surface/skills/$name" ]; then
      echo -n "MISSING $surface "
      ok=false
    fi
  done
  # Check KG
  if [ ! -f ".agents/skills/$name/$name.kg.md" ]; then
    echo -n "MISSING KG "
    ok=false
  fi
  # Check AGENTS.md
  if ! grep -q "$name" AGENTS.md; then
    echo -n "MISSING FROM AGENTS.md "
    ok=false
  fi
  $ok && echo "OK" || echo ""
done
```

---

## Commit Hygiene

Stage only what you changed:

```bash
# New skill
git add .agents/skills/<skill-name>/
git add .claude/skills/<skill-name> .codex/skills/<skill-name> .gemini/skills/<skill-name>
git add AGENTS.md

# Updated skill
git add .agents/skills/<skill-name>/SKILL.md
git add AGENTS.md  # only if routing table changed
```

---

## Related Skills

- `skill-authoring` — Full creation process for new skills
- `orchestrator-quickstart` — Example of a well-structured skill
