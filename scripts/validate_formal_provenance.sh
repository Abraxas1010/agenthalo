#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
HEYTING_ROOT="${HEYTING_ROOT:-/home/abraxas/Work/heyting}"

git -C "$HEYTING_ROOT" fetch origin --quiet

python3 - "$HEYTING_ROOT" "$REPO_ROOT" <<'PY'
import json
import pathlib
import re
import subprocess
import sys

heyting = pathlib.Path(sys.argv[1])
repo = pathlib.Path(sys.argv[2])

rust_files = [
    repo / "src/security.rs",
    repo / "src/transparency/ct6962.rs",
    repo / "src/vc/ipa.rs",
    repo / "src/sheaf/coherence.rs",
    repo / "src/protocol.rs",
]
config_path = repo / "configs/proof_gate.json"

pair_re = re.compile(
    r'\(\s*"[^"]+"\s*,\s*"([^"]+)"\s*,\s*(?:Some\("([^"]+)"\)|None)\s*,?\s*\)',
    re.S,
)

def git_decl_exists(full_path: str) -> bool:
    short = full_path.split('.')[-1]
    patterns = [
        f"theorem {short}",
        f"def {short}",
        f"abbrev {short}",
        f"structure {short}",
    ]
    for pattern in patterns:
        cmd = ["git", "-C", str(heyting), "grep", "-n", pattern, "origin/master", "--", "lean/HeytingLean"]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode == 0 and proc.stdout.strip():
            return True
    return False


def local_decl_exists(full_path: str) -> bool:
    short = full_path.split('.')[-1]
    patterns = [
        rf"\btheorem\s+{re.escape(short)}(\s|$)",
        rf"\bdef\s+{re.escape(short)}(\s|$)",
        rf"\babbrev\s+{re.escape(short)}(\s|$)",
        rf"\bstructure\s+{re.escape(short)}(\s|$)",
    ]
    for pattern in patterns:
        proc = subprocess.run(
            ["rg", "-n", "-g", "*.lean", pattern, str(repo / "lean/NucleusDB")],
            capture_output=True,
            text=True,
        )
        if proc.returncode == 0 and proc.stdout.strip():
            return True
    return False

canonical = set()
local = set()
for path in rust_files:
    text = path.read_text()
    for canon, loc in pair_re.findall(text):
        canonical.add(canon)
        if loc:
            local.add(loc)

cfg = json.loads(config_path.read_text())
for reqs in cfg.get("requirements", {}).values():
    for req in reqs:
        canonical.add(req["required_theorem"])

missing_canonical = sorted(p for p in canonical if not git_decl_exists(p))
missing_local = sorted(p for p in local if not local_decl_exists(p))

if missing_canonical:
    print("Missing canonical theorems:")
    for item in missing_canonical:
        print(f"  - {item}")
if missing_local:
    print("Missing local mirror theorems:")
    for item in missing_local:
        print(f"  - {item}")

print(f"Checked {len(canonical)} canonical refs and {len(local)} local refs.")
if missing_canonical or missing_local:
    sys.exit(1)
print("All formal provenance references resolve.")
PY
