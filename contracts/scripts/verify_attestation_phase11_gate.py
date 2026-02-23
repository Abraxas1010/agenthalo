#!/usr/bin/env python3
"""Verify Phase 11 attestation release-gate artifacts with fail-closed semantics."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Dict, List


def _load_json(path: Path) -> Dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def _expect_ok_json(path: Path, label: str, errors: List[str]) -> Dict[str, Any]:
    if not path.exists():
        errors.append(f"missing {label}: {path}")
        return {}
    obj = _load_json(path)
    if obj.get("ok") is not True:
        errors.append(f"{label} ok is not true: {path}")
    return obj


def _to_int(value: Any) -> int:
    if isinstance(value, bool):
        raise ValueError("bool is not a valid integer value")
    if isinstance(value, int):
        return value
    if isinstance(value, str):
        text = value.strip()
        if text.startswith(("0x", "0X")):
            return int(text, 16)
        return int(text)
    raise ValueError(f"unsupported integer value type: {type(value).__name__}")


def _check_optional_mode(run_dir: Path, errors: List[str], notes: List[str]) -> None:
    promo = run_dir / "promotion_optional"
    if not promo.exists():
        errors.append(f"missing optional promotion directory: {promo}")
        return

    promotion_report = _expect_ok_json(promo / "promotion_report.json", "promotion_report", errors)
    _expect_ok_json(promo / "verifier_report.json", "verifier_report", errors)

    gate_path = promo / "gate" / "gate_report.json"
    if not gate_path.exists():
        errors.append(f"missing gate_report: {gate_path}")
    else:
        gate = _load_json(gate_path)
        if gate.get("overall") != "PASS":
            errors.append(f"gate overall is not PASS: {gate.get('overall')!r}")

    _expect_ok_json(promo / "evidence_bundle" / "bundle_report.json", "bundle_report", errors)
    _expect_ok_json(promo / "offline_replay_report.json", "offline_replay_report", errors)

    retention = promo / "retention_report_dry_run.json"
    if retention.exists():
        ret_obj = _load_json(retention)
        if ret_obj.get("ok") is not True:
            errors.append(f"retention_report_dry_run ok is not true: {retention}")
    else:
        notes.append("retention_report_dry_run.json missing (optional in optional mode)")

    if promotion_report and str(promotion_report.get("base_mode")) != "optional":
        errors.append("optional mode expected promotion_report.base_mode == 'optional'")


def _check_required_mode(run_dir: Path, errors: List[str]) -> None:
    req = run_dir / "rehearsal_required"
    if not req.exists():
        errors.append(f"missing required rehearsal directory: {req}")
        return

    rehearsal = _expect_ok_json(req / "rehearsal_report.json", "rehearsal_report", errors)
    promotion = _expect_ok_json(req / "promotion_report.json", "promotion_report", errors)

    _expect_ok_json(req / "offline_replay_report.json", "offline_replay_report", errors)
    _expect_ok_json(req / "retention_report.json", "retention_report", errors)
    _expect_ok_json(req / "evidence_bundle" / "bundle_report.json", "bundle_report", errors)

    if promotion and str(promotion.get("base_mode")) != "required":
        errors.append("required mode expected promotion_report.base_mode == 'required'")

    if rehearsal:
        if str(rehearsal.get("promotion_dir", "")) and Path(str(rehearsal.get("promotion_dir"))).resolve() != req.resolve():
            errors.append("rehearsal_report.promotion_dir does not match rehearsal_required directory")


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify Phase 11 release-gate artifacts")
    parser.add_argument("--run-dir", required=True, help="Phase 11 run directory")
    parser.add_argument("--mode", choices=["optional", "required"], default="optional")
    parser.add_argument("--output", help="Optional JSON report output path")
    args = parser.parse_args()

    run_dir = Path(args.run_dir).resolve()
    errors: List[str] = []
    notes: List[str] = []

    if not run_dir.exists():
        raise SystemExit(f"run directory does not exist: {run_dir}")

    fuzz = _expect_ok_json(run_dir / "mutation_fuzz_report.json", "mutation_fuzz_report", errors)
    if fuzz:
        try:
            total_cases = _to_int(fuzz.get("total_cases", -1))
        except (TypeError, ValueError) as exc:
            errors.append(f"mutation_fuzz_report total_cases invalid: {exc}")
        else:
            if total_cases < 6:
                errors.append(f"mutation_fuzz_report total_cases too small: {total_cases}")

    if args.mode == "required":
        _check_required_mode(run_dir, errors)
    else:
        _check_optional_mode(run_dir, errors, notes)

    report = {
        "schema": "nucleusdb/attestation-phase11-gate-verifier/v1",
        "run_dir": str(run_dir),
        "mode": args.mode,
        "ok": len(errors) == 0,
        "errors": errors,
        "notes": notes,
    }
    payload = json.dumps(report, indent=2, sort_keys=True)
    print(payload)
    if args.output:
        out = Path(args.output)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(payload + "\n", encoding="utf-8")
    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
