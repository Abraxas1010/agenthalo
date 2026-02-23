#!/usr/bin/env python3
"""Independent offline replay verifier for attestation promotion artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path
from typing import Any, Dict, List


def _load_json(path: Path) -> Dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def _to_int(value: Any, default: int = -1) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _sha512(path: Path) -> str:
    h = hashlib.sha512()
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def _check_base_evidence(evidence: Dict[str, Any], errors: List[str]) -> None:
    if evidence.get("chain_id") != 84532:
        errors.append(f"base evidence chain_id mismatch: {evidence.get('chain_id')!r}")
    ver = evidence.get("verification")
    if not isinstance(ver, dict):
        errors.append("base evidence verification section missing")
    else:
        if ver.get("verify_agent") is not True:
            errors.append("base evidence verify_agent is not true")
        if _to_int(ver.get("receipt_status"), default=-1) != 1:
            errors.append(f"base evidence receipt_status invalid: {ver.get('receipt_status')!r}")
    econ = evidence.get("economics")
    if not isinstance(econ, dict):
        errors.append("base evidence economics section missing")
    else:
        fee = _to_int(econ.get("fee_wei"), default=-1)
        delta = _to_int(econ.get("delta"), default=-2)
        if fee < 0 or delta < 0:
            errors.append("base evidence economics fields missing/invalid")
        elif fee != delta:
            errors.append(f"base evidence fee/delta mismatch: fee_wei={fee}, delta={delta}")


def _verify_manifest_signature(bundle_dir: Path, errors: List[str]) -> None:
    manifest = bundle_dir / "manifest.sha512"
    sig = bundle_dir / "manifest.sig"
    pub = bundle_dir / "manifest_pub.pem"
    if not manifest.exists():
        errors.append(f"manifest missing: {manifest}")
        return
    if not sig.exists():
        errors.append(f"signature missing: {sig}")
        return
    if not pub.exists():
        errors.append(f"public key missing: {pub}")
        return
    try:
        subprocess.run(
            [
                "openssl",
                "dgst",
                "-sha256",
                "-verify",
                str(pub),
                "-signature",
                str(sig),
                str(manifest),
            ],
            check=True,
            text=True,
            capture_output=True,
        )
    except Exception as exc:  # noqa: BLE001
        errors.append(f"manifest signature verification failed: {exc}")


def _verify_manifest_hashes(run_dir: Path, bundle_dir: Path, errors: List[str]) -> None:
    manifest = bundle_dir / "manifest.sha512"
    if not manifest.exists():
        errors.append(f"manifest missing: {manifest}")
        return
    for line in manifest.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split("  ", 1)
        if len(parts) != 2:
            errors.append(f"invalid manifest line: {line}")
            continue
        digest, rel = parts
        target = run_dir / rel
        if not target.exists():
            errors.append(f"manifest target missing: {target}")
            continue
        got = _sha512(target)
        if got != digest:
            errors.append(f"manifest digest mismatch for {rel}: expected {digest}, got {got}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Offline replay verifier for attestation promotion")
    parser.add_argument("--promotion-report", required=True, help="Path to promotion_report.json")
    parser.add_argument("--require-signed-bundle", action="store_true")
    parser.add_argument("--output", help="Optional JSON output path")
    args = parser.parse_args()

    errors: List[str] = []
    notes: List[str] = []

    promotion_report_path = Path(args.promotion_report).resolve()
    if not promotion_report_path.exists():
        raise SystemExit(f"promotion report missing: {promotion_report_path}")
    run_dir = promotion_report_path.parent
    promotion = _load_json(promotion_report_path)

    if promotion.get("ok") is not True:
        errors.append("promotion_report ok is not true")
    if promotion.get("schema") != "nucleusdb/attestation-promotion-report/v1":
        errors.append(f"unexpected promotion schema: {promotion.get('schema')!r}")

    gate_report_path = Path(promotion.get("gate_report", ""))
    verifier_report_path = Path(promotion.get("verifier_report", ""))
    if not gate_report_path.is_absolute():
        gate_report_path = (run_dir / gate_report_path).resolve()
    if not verifier_report_path.is_absolute():
        verifier_report_path = (run_dir / verifier_report_path).resolve()

    if not gate_report_path.exists():
        errors.append(f"gate report missing: {gate_report_path}")
    if not verifier_report_path.exists():
        errors.append(f"verifier report missing: {verifier_report_path}")

    gate: Dict[str, Any] = {}
    verifier: Dict[str, Any] = {}
    if gate_report_path.exists():
        gate = _load_json(gate_report_path)
        checks = gate.get("checks", {})
        for key in ("private_key_response_check", "forge_test", "local_e2e"):
            if checks.get(key) != "PASS":
                errors.append(f"gate local check failed: {key}={checks.get(key)!r}")
        if gate.get("overall") != "PASS":
            errors.append(f"gate overall is not PASS: {gate.get('overall')!r}")

    if verifier_report_path.exists():
        verifier = _load_json(verifier_report_path)
        if verifier.get("ok") is not True:
            errors.append("verifier report ok is not true")
        if verifier.get("errors"):
            errors.append(f"verifier reported errors: {verifier.get('errors')!r}")

    base_mode = str(promotion.get("base_mode", "required"))
    base_ref = gate.get("base_evidence_file")
    if base_mode == "required":
        if not isinstance(base_ref, str) or not base_ref.strip():
            errors.append("required mode but no base evidence reference in gate report")
        else:
            base_path = Path(base_ref)
            if not base_path.is_absolute():
                base_path = (gate_report_path.parent / base_path).resolve()
            if not base_path.exists():
                errors.append(f"required base evidence file missing: {base_path}")
            else:
                _check_base_evidence(_load_json(base_path), errors)
    elif isinstance(base_ref, str) and base_ref.strip():
        base_path = Path(base_ref)
        if not base_path.is_absolute():
            base_path = (gate_report_path.parent / base_path).resolve()
        if base_path.exists():
            _check_base_evidence(_load_json(base_path), errors)
        else:
            errors.append(f"base evidence referenced but missing: {base_path}")
    else:
        notes.append(f"base evidence skipped in mode={base_mode}")

    bundle_dir = run_dir / "evidence_bundle"
    if bundle_dir.exists():
        _verify_manifest_hashes(run_dir, bundle_dir, errors)
        if args.require_signed_bundle:
            _verify_manifest_signature(bundle_dir, errors)
    elif args.require_signed_bundle:
        errors.append(f"signed evidence bundle required but missing: {bundle_dir}")

    result = {
        "schema": "nucleusdb/attestation-promotion-offline-replay/v1",
        "promotion_report": str(promotion_report_path),
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
