#!/usr/bin/env python3
"""Verify attestation economics gate artifacts with fail-closed semantics."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Dict, List, Tuple


def _load_json(path: Path) -> Dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def _to_int(value: Any, default: int = -1) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _check_gate_report(report: Dict[str, Any], require_base: bool) -> Tuple[List[str], List[str]]:
    errors: List[str] = []
    notes: List[str] = []

    checks = report.get("checks")
    if not isinstance(checks, dict):
        errors.append("gate_report.checks must be an object")
        return errors, notes

    required_local = ["private_key_response_check", "forge_test", "local_e2e"]
    for key in required_local:
        if checks.get(key) != "PASS":
            errors.append(f"gate local check failed: {key}={checks.get(key)!r}")

    overall = report.get("overall")
    if overall != "PASS":
        errors.append(f"gate overall is not PASS: {overall!r}")

    base_mode = str(report.get("base_mode", "optional"))
    base_pre = checks.get("base_preflight")
    base_e2e = checks.get("base_e2e")
    base_reason = report.get("base_reason")

    if require_base or base_mode == "required":
        if base_pre != "PASS":
            errors.append(f"required Base preflight not PASS: {base_pre!r}")
        if base_e2e != "PASS":
            errors.append(f"required Base e2e not PASS: {base_e2e!r}")
    else:
        if base_pre == "SKIP" or base_e2e == "SKIP":
            notes.append(f"base checks skipped in mode={base_mode} reason={base_reason}")

    return errors, notes


def _check_base_evidence(evidence: Dict[str, Any]) -> List[str]:
    errors: List[str] = []

    chain_id = evidence.get("chain_id")
    if chain_id != 84532:
        errors.append(f"base evidence chain_id mismatch: expected 84532 got {chain_id!r}")

    verification = evidence.get("verification")
    if not isinstance(verification, dict):
        errors.append("base evidence verification section missing")
    else:
        if verification.get("verify_agent") is not True:
            errors.append("base evidence verify_agent is not true")
        if _to_int(verification.get("receipt_status"), default=-1) != 1:
            errors.append(
                f"base evidence receipt_status is not 1: {verification.get('receipt_status')!r}"
            )

    economics = evidence.get("economics")
    if not isinstance(economics, dict):
        errors.append("base evidence economics section missing")
    else:
        fee = _to_int(economics.get("fee_wei"), default=-1)
        delta = _to_int(economics.get("delta"), default=-2)
        if fee < 0 or delta < 0:
            errors.append("base evidence economics fields missing/invalid")
        elif fee != delta:
            errors.append(f"base evidence fee/delta mismatch: fee_wei={fee}, delta={delta}")

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify NucleusDB attestation gate artifacts and optional Base evidence"
    )
    parser.add_argument("--run-dir", required=True, help="Path to gate run directory")
    parser.add_argument(
        "--require-base",
        action="store_true",
        help="Require Base evidence checks regardless of gate mode",
    )
    parser.add_argument("--output", help="Optional path to write verifier JSON report")
    args = parser.parse_args()

    run_dir = Path(args.run_dir).resolve()
    gate_report_path = run_dir / "gate_report.json"

    errors: List[str] = []
    notes: List[str] = []
    gate_report: Dict[str, Any] | None = None
    base_evidence: Dict[str, Any] | None = None
    base_evidence_path: Path | None = None

    if not gate_report_path.exists():
        errors.append(f"missing gate report: {gate_report_path}")
    else:
        gate_report = _load_json(gate_report_path)
        e, n = _check_gate_report(gate_report, args.require_base)
        errors.extend(e)
        notes.extend(n)

    if gate_report is not None:
        maybe_base = gate_report.get("base_evidence_file")
        if isinstance(maybe_base, str) and maybe_base.strip():
            base_evidence_path = Path(maybe_base)
            if not base_evidence_path.is_absolute():
                base_evidence_path = (run_dir / base_evidence_path).resolve()
            if not base_evidence_path.exists():
                errors.append(f"base evidence file missing: {base_evidence_path}")
            else:
                base_evidence = _load_json(base_evidence_path)
        elif args.require_base:
            errors.append("required base evidence is not referenced in gate report")

    if args.require_base and base_evidence is not None:
        errors.extend(_check_base_evidence(base_evidence))

    result: Dict[str, Any] = {
        "schema": "nucleusdb/attestation-gate-verifier-report/v1",
        "run_dir": str(run_dir),
        "gate_report": str(gate_report_path),
        "base_evidence": str(base_evidence_path) if base_evidence_path else None,
        "require_base": bool(args.require_base),
        "ok": len(errors) == 0,
        "errors": errors,
        "notes": notes,
    }

    payload = json.dumps(result, indent=2, sort_keys=True)
    print(payload)

    if args.output:
        out = Path(args.output)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(payload + "\n", encoding="utf-8")

    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
