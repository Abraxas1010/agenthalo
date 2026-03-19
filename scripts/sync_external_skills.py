#!/usr/bin/env python3
"""Sync external skills from workspace profile into agent skill directories.

Reads the active workspace profile's external skill source injections and
copies skill directories into .claude/skills/, .codex/skills/, .gemini/skills/
as READ-ONLY copies. Agents can see and load these skills but cannot modify
the source files.

Local skills (symlinks to ../../.agents/skills/) take precedence and are
never overwritten.

Usage:
    python3 scripts/sync_external_skills.py [--json] [--dry-run] [--clean]
"""

import argparse
import json
import os
import shutil
import stat
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AGENT_DIRS = [".claude/skills", ".codex/skills", ".gemini/skills"]
MARKER_FILE = ".external_source"


def load_profile():
    profile_dir = os.path.expanduser("~/.agenthalo/workspace_profiles")
    active_path = os.path.expanduser("~/.agenthalo/active_workspace_profile")
    name = "default"
    if os.path.exists(active_path):
        name = open(active_path).read().strip() or "default"
    path = os.path.join(profile_dir, f"{name}.json")
    if os.path.exists(path):
        with open(path) as f:
            return json.load(f)
    return {"injections": []}


def skill_sources(profile):
    sources = []
    for inj in profile.get("injections", []):
        if "skills" in inj.get("target", ""):
            src = os.path.expanduser(inj["source"])
            if os.path.isdir(src):
                sources.append(src)
    return sources


def is_external_copy(path):
    """Check if a directory is an external readonly copy (has our marker file)."""
    return os.path.isfile(os.path.join(path, MARKER_FILE))


def is_local_skill(path):
    """Check if a path is a local skill (symlink to ../../.agents/skills/)."""
    if os.path.islink(path):
        target = os.readlink(path)
        return target.startswith("../../.agents/skills/")
    return False


def set_readonly_recursive(path):
    """Set a directory tree to read-only (r-x for dirs, r-- for files)."""
    for root, dirs, files in os.walk(path, topdown=False):
        for f in files:
            fp = os.path.join(root, f)
            os.chmod(fp, stat.S_IRUSR | stat.S_IRGRP | stat.S_IROTH)  # 0o444
        for d in dirs:
            dp = os.path.join(root, d)
            os.chmod(dp, stat.S_IRUSR | stat.S_IXUSR | stat.S_IRGRP | stat.S_IXGRP)  # 0o550
    os.chmod(path, stat.S_IRUSR | stat.S_IXUSR | stat.S_IRGRP | stat.S_IXGRP)  # 0o550


def restore_write_recursive(path):
    """Restore write permissions so we can modify/delete."""
    for root, dirs, files in os.walk(path, topdown=True):
        try:
            os.chmod(root, 0o755)
        except OSError:
            pass
        for f in files:
            try:
                os.chmod(os.path.join(root, f), 0o644)
            except OSError:
                pass


def sync(dry_run=False):
    profile = load_profile()
    sources = skill_sources(profile)
    created = 0
    updated = 0
    skipped = 0
    removed = 0
    results = []

    for src_dir in sources:
        for entry in os.scandir(src_dir):
            if not entry.is_dir() or entry.name.startswith(".") or entry.name == "__pycache__":
                continue
            for agent_dir in AGENT_DIRS:
                target = os.path.join(REPO, agent_dir, entry.name)

                # Local skill takes precedence
                if is_local_skill(target):
                    skipped += 1
                    continue

                # If it's an existing external copy, check if source is newer
                if os.path.isdir(target) and is_external_copy(target):
                    # Simple freshness: compare SKILL.md mtime
                    src_skill = os.path.join(entry.path, "SKILL.md")
                    dst_skill = os.path.join(target, "SKILL.md")
                    if os.path.exists(src_skill) and os.path.exists(dst_skill):
                        if os.path.getmtime(src_skill) <= os.path.getmtime(dst_skill):
                            skipped += 1
                            continue
                    # Needs update — remove old copy
                    if not dry_run:
                        restore_write_recursive(target)
                        shutil.rmtree(target)
                    updated += 1
                elif os.path.exists(target) or os.path.islink(target):
                    # Some other kind of entry — don't touch
                    skipped += 1
                    continue

                if not dry_run:
                    shutil.copytree(entry.path, target)
                    # Write marker file before setting readonly
                    with open(os.path.join(target, MARKER_FILE), "w") as f:
                        f.write(json.dumps({
                            "source": entry.path,
                            "synced_at": __import__("time").time(),
                        }))
                    set_readonly_recursive(target)
                created += 1
                results.append({"action": "copy_readonly", "target": target, "source": entry.path})

    # Clean stale external copies (source dir no longer exists or not in profile)
    all_external_names = set()
    for src_dir in sources:
        if os.path.isdir(src_dir):
            for entry in os.scandir(src_dir):
                if entry.is_dir() and not entry.name.startswith("."):
                    all_external_names.add(entry.name)

    for agent_dir in AGENT_DIRS:
        full = os.path.join(REPO, agent_dir)
        if not os.path.isdir(full):
            continue
        for entry in os.scandir(full):
            if entry.is_dir() and is_external_copy(entry.path):
                if entry.name not in all_external_names:
                    if not dry_run:
                        restore_write_recursive(entry.path)
                        shutil.rmtree(entry.path)
                    removed += 1
                    results.append({"action": "remove_stale", "target": entry.path})

    return {
        "created": created,
        "updated": updated,
        "skipped": skipped,
        "removed_stale": removed,
        "sources": sources,
        "agent_dirs": {
            d: len([e for e in os.scandir(os.path.join(REPO, d)) if e.is_dir() or e.is_symlink()])
            for d in AGENT_DIRS
            if os.path.isdir(os.path.join(REPO, d))
        },
        "details": results[:20],  # Truncate for readability
    }


def clean():
    """Remove all external skill copies from agent directories."""
    removed = 0
    for agent_dir in AGENT_DIRS:
        full = os.path.join(REPO, agent_dir)
        if not os.path.isdir(full):
            continue
        for entry in os.scandir(full):
            if entry.is_dir() and is_external_copy(entry.path):
                restore_write_recursive(entry.path)
                shutil.rmtree(entry.path)
                removed += 1
    return removed


def main():
    parser = argparse.ArgumentParser(description="Sync external skills into agent directories (readonly)")
    parser.add_argument("--json", action="store_true", help="Output JSON")
    parser.add_argument("--dry-run", action="store_true", help="Don't modify filesystem")
    parser.add_argument("--clean", action="store_true", help="Remove all external skill copies")
    args = parser.parse_args()

    if args.clean:
        removed = clean()
        if args.json:
            print(json.dumps({"cleaned": removed}))
        else:
            print(f"Cleaned {removed} external skill copies")
        return

    result = sync(dry_run=args.dry_run)
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        prefix = "[DRY RUN] " if args.dry_run else ""
        print(f"{prefix}External skill sync: {result['created']} created, {result['updated']} updated, {result['skipped']} skipped, {result['removed_stale']} stale removed")
        for d, count in result["agent_dirs"].items():
            print(f"  {d}: {count} skills")


if __name__ == "__main__":
    main()
