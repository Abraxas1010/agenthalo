#!/usr/bin/env python3
"""Resolve NucleusDB formal provenance references exactly.

This script provides two workflows:

1. `validate`: confirm that every canonical Heyting FQN and every local mirror
   FQN resolves to an actual declaration, using namespace-aware parsing rather
   than short-name grep matches.
2. `certificate-plan`: emit the exact canonical theorem inventory needed by the
   certificate generator, including declaration lines and statement hashes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
from dataclasses import dataclass
from typing import Iterable


DECL_PREFIX = r"(?:(?:noncomputable|private|protected)\s+)*"
GREP_DECL_PREFIX = r"((noncomputable|private|protected)[[:space:]]+)*"
DECL_RE = re.compile(
    rf"^\s*(?:@\[[^\]]+\]\s*)*{DECL_PREFIX}(theorem|def|abbrev|structure)\s+([A-Za-z0-9_']+)(?:\s|$)"
)
PAIR_RE = re.compile(
    r'\(\s*"[^"]+"\s*,\s*"([^"]+)"\s*,\s*(?:Some\("([^"]+)"\)|None)\s*,?\s*\)',
    re.S,
)


@dataclass(frozen=True)
class DeclarationMatch:
    full_name: str
    kind: str
    file_path: str
    line_no: int
    decl_line: str


@dataclass
class NamespaceFrame:
    parts: list[str]


def run(cmd: list[str], *, input_text: str | None = None, check: bool = True) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(
        cmd,
        input=input_text,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(f"command failed ({proc.returncode}): {' '.join(cmd)}\n{proc.stderr}")
    return proc


def default_heyting_root(repo_root: pathlib.Path) -> pathlib.Path:
    env = pathlib.Path(pathlib.Path.home()).joinpath("Work", "heyting")
    candidates = [
        pathlib.Path(str(os.environ.get("HEYTING_ROOT", ""))).expanduser()
        if os.environ.get("HEYTING_ROOT")
        else None,
        repo_root.parent / "heyting",
        env,
    ]
    for candidate in candidates:
        if candidate and (candidate / "lean/HeytingLean").exists():
            return candidate
    raise SystemExit(
        "Could not locate the Heyting repo. Set HEYTING_ROOT or place a sibling `heyting/` checkout next to this repo."
    )


def parse_declarations(text: str, file_path: str, short_name: str) -> list[DeclarationMatch]:
    matches: list[DeclarationMatch] = []
    namespace_frames: list[NamespaceFrame] = []

    def current_namespace() -> list[str]:
        out: list[str] = []
        for frame in namespace_frames:
            out.extend(frame.parts)
        return out

    for line_no, raw in enumerate(text.splitlines(), start=1):
        stripped = raw.strip()
        if not stripped or stripped.startswith("--"):
            continue

        if stripped.startswith("namespace "):
            _, _, ns = stripped.partition("namespace ")
            parts = [part for part in ns.split(".") if part]
            if parts:
                namespace_frames.append(NamespaceFrame(parts))
            continue

        if stripped == "end":
            if namespace_frames:
                namespace_frames.pop()
            continue

        if stripped.startswith("end "):
            _, _, ns = stripped.partition("end ")
            parts = [part for part in ns.split(".") if part]
            if parts:
                suffix: list[str] = []
                pop_count = 0
                for frame in reversed(namespace_frames):
                    pop_count += 1
                    suffix = frame.parts + suffix
                    if suffix == parts:
                        del namespace_frames[-pop_count:]
                        break
                else:
                    if namespace_frames:
                        namespace_frames.pop()
            elif namespace_frames:
                namespace_frames.pop()
            continue

        decl = DECL_RE.match(raw)
        if decl and decl.group(2) == short_name:
            full_name = ".".join(current_namespace() + [short_name])
            matches.append(
                DeclarationMatch(
                    full_name=full_name,
                    kind=decl.group(1),
                    file_path=file_path,
                    line_no=line_no,
                    decl_line=stripped,
                )
            )

    return matches


def canonical_candidate_files(heyting_root: pathlib.Path, short_name: str) -> list[str]:
    pattern = (
        rf"^[[:space:]]*(@\[[^]]+\][[:space:]]*)*{GREP_DECL_PREFIX}"
        rf"(theorem|def|abbrev|structure)[[:space:]]+{re.escape(short_name)}([[:space:]]|$)"
    )
    proc = run(
        [
            "git",
            "-C",
            str(heyting_root),
            "grep",
            "-l",
            "-E",
            pattern,
            "origin/master",
            "--",
            "lean/HeytingLean",
        ],
        check=False,
    )
    if proc.returncode not in (0, 1):
        raise RuntimeError(proc.stderr)
    files: list[str] = []
    for line in proc.stdout.splitlines():
        entry = line.strip()
        if not entry:
            continue
        if ":" in entry:
            _, _, entry = entry.partition(":")
        files.append(entry)
    return files


def local_candidate_files(repo_root: pathlib.Path, short_name: str) -> list[pathlib.Path]:
    pattern = (
        rf"^[[:space:]]*(@\[[^]]+\][[:space:]]*)*{GREP_DECL_PREFIX}"
        rf"(theorem|def|abbrev|structure)[[:space:]]+{re.escape(short_name)}([[:space:]]|$)"
    )
    proc = run(
        [
            "rg",
            "-l",
            "-g",
            "*.lean",
            pattern,
            str(repo_root / "lean/NucleusDB"),
        ],
        check=False,
    )
    if proc.returncode not in (0, 1):
        raise RuntimeError(proc.stderr)
    return [pathlib.Path(line.strip()) for line in proc.stdout.splitlines() if line.strip()]


def resolve_canonical(heyting_root: pathlib.Path, full_name: str) -> list[DeclarationMatch]:
    short_name = full_name.rsplit(".", 1)[-1]
    matches: list[DeclarationMatch] = []
    for file_path in canonical_candidate_files(heyting_root, short_name):
        proc = run(
            ["git", "-C", str(heyting_root), "show", f"origin/master:{file_path}"],
            check=True,
        )
        matches.extend(parse_declarations(proc.stdout, file_path, short_name))
    return [match for match in matches if match.full_name == full_name]


def resolve_local(repo_root: pathlib.Path, full_name: str) -> list[DeclarationMatch]:
    short_name = full_name.rsplit(".", 1)[-1]
    matches: list[DeclarationMatch] = []
    for file_path in local_candidate_files(repo_root, short_name):
        matches.extend(parse_declarations(file_path.read_text(), str(file_path.relative_to(repo_root)), short_name))
    return [match for match in matches if match.full_name == full_name]


def load_references(repo_root: pathlib.Path) -> tuple[set[str], set[str]]:
    rust_files = [
        repo_root / "src/security.rs",
        repo_root / "src/transparency/ct6962.rs",
        repo_root / "src/vc/ipa.rs",
        repo_root / "src/sheaf/coherence.rs",
        repo_root / "src/protocol.rs",
    ]
    canonical: set[str] = set()
    local: set[str] = set()

    for path in rust_files:
        text = path.read_text()
        for canon, loc in PAIR_RE.findall(text):
            canonical.add(canon)
            if loc:
                local.add(loc)

    cfg = json.loads((repo_root / "configs/proof_gate.json").read_text())
    for reqs in cfg.get("requirements", {}).values():
        for req in reqs:
            canonical.add(req["required_theorem"])

    return canonical, local


def unique_requirements(repo_root: pathlib.Path) -> list[str]:
    cfg = json.loads((repo_root / "configs/proof_gate.json").read_text())
    out: list[str] = []
    seen: set[str] = set()
    for tool in sorted(cfg.get("requirements", {})):
        for req in cfg["requirements"][tool]:
            theorem = req["required_theorem"]
            if theorem not in seen:
                seen.add(theorem)
                out.append(theorem)
    return out


def configured_expected_commits(repo_root: pathlib.Path) -> set[str]:
    cfg = json.loads((repo_root / "configs/proof_gate.json").read_text())
    commits: set[str] = set()
    for reqs in cfg.get("requirements", {}).values():
        for req in reqs:
            commit_hash = req.get("expected_commit_hash")
            if commit_hash:
                commits.add(commit_hash)
    return commits


def require_exactly_one(matches: list[DeclarationMatch], full_name: str, label: str) -> DeclarationMatch:
    if not matches:
        raise SystemExit(f"Missing {label}: {full_name}")
    if len(matches) > 1:
        details = ", ".join(f"{m.file_path}:{m.line_no}" for m in matches)
        raise SystemExit(f"Ambiguous {label}: {full_name} -> {details}")
    return matches[0]


def cmd_validate(repo_root: pathlib.Path, heyting_root: pathlib.Path) -> int:
    canonical, local = load_references(repo_root)
    missing_canonical: list[str] = []
    missing_local: list[str] = []
    stale_commit_error: str | None = None

    current_commit = run(["git", "-C", str(heyting_root), "rev-parse", "origin/master"]).stdout.strip()
    expected_commits = configured_expected_commits(repo_root)
    if not expected_commits:
        stale_commit_error = "No expected_commit_hash values are configured in proof_gate.json."
    elif expected_commits != {current_commit}:
        stale_commit_error = (
            "Pinned expected_commit_hash values do not match the live Heyting origin/master commit. "
            f"configured={sorted(expected_commits)} live={current_commit}"
        )

    for full_name in sorted(canonical):
        if not resolve_canonical(heyting_root, full_name):
            missing_canonical.append(full_name)
    for full_name in sorted(local):
        if not resolve_local(repo_root, full_name):
            missing_local.append(full_name)

    if missing_canonical:
        print("Missing canonical declarations:")
        for item in missing_canonical:
            print(f"  - {item}")
    if missing_local:
        print("Missing local mirror declarations:")
        for item in missing_local:
            print(f"  - {item}")
    if stale_commit_error:
        print(f"Commit staleness check failed: {stale_commit_error}")

    print(f"Checked {len(canonical)} canonical refs and {len(local)} local refs.")
    if missing_canonical or missing_local or stale_commit_error:
        return 1
    print("All formal provenance references resolve exactly.")
    return 0


def cmd_certificate_plan(repo_root: pathlib.Path, heyting_root: pathlib.Path) -> int:
    commit_hash = run(["git", "-C", str(heyting_root), "rev-parse", "origin/master"]).stdout.strip()
    plan = []
    for theorem in unique_requirements(repo_root):
        match = require_exactly_one(resolve_canonical(heyting_root, theorem), theorem, "canonical theorem")
        statement_hash = hashlib.sha256(match.decl_line.encode("utf-8")).hexdigest()
        plan.append(
            {
                "theorem": theorem,
                "source_file": match.file_path,
                "line_no": match.line_no,
                "decl_line": match.decl_line,
                "statement_hash": statement_hash,
                "commit_hash": commit_hash,
            }
        )
    json.dump(plan, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "mode",
        choices=["validate", "certificate-plan"],
        help="Validation mode or certificate generation planning mode.",
    )
    parser.add_argument(
        "--repo-root",
        default=str(pathlib.Path(__file__).resolve().parents[1]),
        help="NucleusDB repo root.",
    )
    parser.add_argument(
        "--heyting-root",
        default=None,
        help="Heyting repo root. Defaults to HEYTING_ROOT, ../heyting, or ~/Work/heyting.",
    )
    args = parser.parse_args(list(argv) if argv is not None else None)

    repo_root = pathlib.Path(args.repo_root).resolve()
    heyting_root = (
        pathlib.Path(args.heyting_root).expanduser().resolve()
        if args.heyting_root
        else default_heyting_root(repo_root)
    )

    if args.mode == "validate":
        return cmd_validate(repo_root, heyting_root)
    return cmd_certificate_plan(repo_root, heyting_root)


if __name__ == "__main__":
    raise SystemExit(main())
