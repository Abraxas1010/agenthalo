#!/usr/bin/env python3
"""Verify Phase 13 launch-gate artifacts with fail-closed semantics."""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def _load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        data = json.load(f)
    if not isinstance(data, dict):
        raise ValueError(f"{path}: expected JSON object")
    return data


def _check(checks: dict[str, str], key: str, ok: bool) -> None:
    checks[key] = "PASS" if ok else "FAIL"


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


def verify(run_dir: Path, mode: str) -> dict[str, Any]:
    checks: dict[str, str] = {}
    details: dict[str, Any] = {"mode": mode, "run_dir": str(run_dir)}

    phase12_report = run_dir / "phase12" / "phase12_readiness_report.json"
    phase11_report = run_dir / "phase11" / "phase11_gate_report.json"

    _check(checks, "phase12_report_present", phase12_report.is_file())
    _check(checks, "phase11_report_present", phase11_report.is_file())
    if not phase12_report.is_file() or not phase11_report.is_file():
        return {
            "ok": False,
            "checks": checks,
            "details": details,
        }

    p12 = _load_json(phase12_report)
    p11 = _load_json(phase11_report)

    p12_checks = p12.get("checks", {})
    p11_checks = p11.get("checks", {})
    details["phase12_report"] = str(phase12_report)
    details["phase11_report"] = str(phase11_report)

    _check(checks, "phase12_overall_pass", p12.get("overall") == "PASS")
    _check(checks, "phase11_overall_pass", p11.get("overall") == "PASS")

    if mode == "optional":
        _check(checks, "phase12_optional_gate_pass", p12_checks.get("phase11_optional_gate") == "PASS")
        _check(checks, "phase11_optional_path_pass", p11_checks.get("promotion_optional") == "PASS")
        _check(checks, "phase11_mutation_fuzz_pass", p11_checks.get("mutation_fuzz") == "PASS")
        _check(checks, "phase11_verifier_pass", p11_checks.get("phase11_verifier") == "PASS")
    else:
        _check(checks, "phase12_env_preflight_pass", p12_checks.get("env_preflight") == "PASS")
        _check(checks, "phase12_signing_material_pass", p12_checks.get("signing_material") == "PASS")
        _check(checks, "phase12_rpc_chain_pass", p12_checks.get("rpc_chain") == "PASS")
        _check(checks, "phase12_contract_views_pass", p12_checks.get("contract_views") == "PASS")

        _check(checks, "phase11_required_rehearsal_pass", p11_checks.get("rehearsal_required") == "PASS")
        _check(checks, "phase11_mutation_fuzz_pass", p11_checks.get("mutation_fuzz") == "PASS")
        _check(checks, "phase11_verifier_pass", p11_checks.get("phase11_verifier") == "PASS")

        required_dir = run_dir / "phase11" / "rehearsal_required"
        promotion_report = required_dir / "promotion_report.json"
        offline_replay_report = required_dir / "offline_replay_report.json"
        retention_report = required_dir / "retention_report.json"
        gate_report = required_dir / "gate" / "gate_report.json"
        gate_verifier_report = required_dir / "verifier_report.json"
        base_evidence = required_dir / "gate" / "base_e2e_evidence.json"

        _check(checks, "promotion_report_present", promotion_report.is_file())
        _check(checks, "offline_replay_report_present", offline_replay_report.is_file())
        _check(checks, "retention_report_present", retention_report.is_file())
        _check(checks, "gate_report_present", gate_report.is_file())
        _check(checks, "gate_verifier_report_present", gate_verifier_report.is_file())
        _check(checks, "base_evidence_present", base_evidence.is_file())

        if promotion_report.is_file():
            promotion = _load_json(promotion_report)
            _check(checks, "promotion_ok_true", promotion.get("ok") is True)
            _check(checks, "promotion_base_mode_required", promotion.get("base_mode") == "required")

        if offline_replay_report.is_file():
            replay = _load_json(offline_replay_report)
            _check(checks, "offline_replay_ok_true", replay.get("ok") is True)

        if retention_report.is_file():
            retention = _load_json(retention_report)
            _check(checks, "retention_ok_true", retention.get("ok") is True)

        if gate_report.is_file():
            gate = _load_json(gate_report)
            gate_checks = gate.get("checks", {})
            _check(checks, "gate_overall_pass", gate.get("overall") == "PASS")
            _check(checks, "gate_local_forge_pass", gate_checks.get("forge_test") == "PASS")
            _check(checks, "gate_local_e2e_pass", gate_checks.get("local_e2e") == "PASS")
            _check(checks, "gate_base_preflight_pass", gate_checks.get("base_preflight") == "PASS")
            _check(checks, "gate_base_e2e_pass", gate_checks.get("base_e2e") == "PASS")

        if gate_verifier_report.is_file():
            gate_verifier = _load_json(gate_verifier_report)
            _check(checks, "gate_verifier_ok_true", gate_verifier.get("ok") is True)

        if base_evidence.is_file():
            evidence = _load_json(base_evidence)
            verification = evidence.get("verification", {})
            economics = evidence.get("economics", {})
            try:
                chain_id = _to_int(evidence.get("chain_id"))
            except (TypeError, ValueError):
                chain_id = None
            _check(checks, "base_chain_id_84532", chain_id == 84532)
            _check(checks, "base_verify_agent_true", verification.get("verify_agent") is True)
            try:
                receipt_status = _to_int(verification.get("receipt_status"))
            except (TypeError, ValueError):
                receipt_status = None
            _check(checks, "base_receipt_status_1", receipt_status == 1)
            try:
                delta = _to_int(economics.get("delta"))
                fee_wei = _to_int(economics.get("fee_wei"))
                delta_equals_fee = delta == fee_wei
            except (TypeError, ValueError):
                delta_equals_fee = False
            _check(checks, "base_delta_equals_fee", delta_equals_fee)

    ok = all(status == "PASS" for status in checks.values())
    return {
        "ok": ok,
        "checks": checks,
        "details": details,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify Phase 13 launch gate artifacts")
    parser.add_argument("--run-dir", required=True, help="Phase 13 run directory")
    parser.add_argument("--mode", choices=["optional", "required"], required=True)
    parser.add_argument("--output", help="Path to write JSON report")
    args = parser.parse_args()

    run_dir = Path(args.run_dir)
    report = verify(run_dir, args.mode)
    payload = {
        "schema": "nucleusdb/attestation-phase13-launch-verifier/v1",
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "mode": args.mode,
        "run_dir": str(run_dir),
        "ok": report["ok"],
        "checks": report["checks"],
        "details": report["details"],
    }

    text = json.dumps(payload, indent=2, sort_keys=True)
    if args.output:
        out_path = Path(args.output)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0 if payload["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
