#!/usr/bin/env python3
"""Apply policy-driven retention for attestation promotion bundles."""

from __future__ import annotations

import argparse
import json
import shutil
from datetime import datetime, timezone, timedelta
from pathlib import Path
from typing import Any, Dict, List


def _load_json(path: Path) -> Dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def main() -> int:
    parser = argparse.ArgumentParser(description="Enforce retention policy for attestation bundles")
    parser.add_argument(
        "--policy",
        default="scripts/attestation_evidence_retention_policy_v1.json",
        help="Path to retention policy JSON (relative to contracts dir if not absolute)",
    )
    parser.add_argument("--root-dir", help="Override root directory from policy")
    parser.add_argument("--apply", action="store_true", help="Apply retention (default dry-run)")
    parser.add_argument("--json-report", help="Optional path to write JSON report")
    args = parser.parse_args()

    contracts_dir = Path(__file__).resolve().parent.parent
    policy_path = Path(args.policy)
    if not policy_path.is_absolute():
        policy_path = (contracts_dir / policy_path).resolve()
    policy = _load_json(policy_path)

    root_dir = Path(args.root_dir) if args.root_dir else Path(policy["root_dir"])
    if not root_dir.is_absolute():
        root_dir = (contracts_dir.parent / root_dir).resolve()
    run_glob = str(policy.get("run_dir_glob", "run_*"))
    keep_last = int(policy.get("keep_last", 20))
    max_age_days = int(policy.get("max_age_days", 30))
    trash_subdir = str(policy.get("trash_subdir", ".trash"))
    require_signed_bundle = bool(policy.get("require_signed_bundle", True))
    required_bundle_files = [str(v) for v in policy.get("required_bundle_files", [])]

    root_dir.mkdir(parents=True, exist_ok=True)
    run_dirs = sorted([p for p in root_dir.glob(run_glob) if p.is_dir()], key=lambda p: p.stat().st_mtime, reverse=True)
    cutoff = datetime.now(timezone.utc) - timedelta(days=max_age_days)

    kept: List[str] = []
    removed: List[str] = []
    noncompliant: List[Dict[str, Any]] = []
    dry_run_targets: List[str] = []

    for idx, run_dir in enumerate(run_dirs):
        mtime = datetime.fromtimestamp(run_dir.stat().st_mtime, tz=timezone.utc)
        old = mtime < cutoff
        should_remove = idx >= keep_last and old
        if should_remove:
            if args.apply:
                trash_root = root_dir / trash_subdir / datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
                trash_root.mkdir(parents=True, exist_ok=True)
                dst = trash_root / run_dir.name
                shutil.move(str(run_dir), str(dst))
                removed.append(str(dst))
            else:
                dry_run_targets.append(str(run_dir))
            continue

        kept.append(str(run_dir))
        if require_signed_bundle:
            missing = []
            for rel in required_bundle_files:
                if not (run_dir / rel).exists():
                    missing.append(rel)
            if missing:
                noncompliant.append({"run_dir": str(run_dir), "missing": missing})

    report = {
        "schema": "nucleusdb/attestation-retention-report/v1",
        "policy": str(policy_path),
        "root_dir": str(root_dir),
        "apply": bool(args.apply),
        "kept_count": len(kept),
        "removed_count": len(removed),
        "dry_run_remove_count": len(dry_run_targets),
        "kept": kept,
        "removed": removed,
        "dry_run_targets": dry_run_targets,
        "noncompliant_kept_runs": noncompliant,
        "ok": len(noncompliant) == 0,
    }
    payload = json.dumps(report, indent=2, sort_keys=True)
    print(payload)
    if args.json_report:
        out = Path(args.json_report)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(payload + "\n", encoding="utf-8")

    return 0 if len(noncompliant) == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())

