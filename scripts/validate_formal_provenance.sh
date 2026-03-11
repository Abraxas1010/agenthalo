#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
HEYTING_ROOT="${HEYTING_ROOT:-}"

if [[ -n "$HEYTING_ROOT" ]]; then
  git -C "$HEYTING_ROOT" fetch origin --quiet
  python3 "$SCRIPT_DIR/formal_provenance_resolver.py" validate --repo-root "$REPO_ROOT" --heyting-root "$HEYTING_ROOT"
else
  python3 "$SCRIPT_DIR/formal_provenance_resolver.py" validate --repo-root "$REPO_ROOT"
fi
