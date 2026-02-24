#!/usr/bin/env python3
"""Verify AgentHALO Phase 5 deployment + e2e evidence artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any


def _load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while True:
            block = f.read(1024 * 1024)
            if not block:
                break
            h.update(block)
    return h.hexdigest()


def _is_hex_with_prefix(value: Any, nbytes: int) -> bool:
    if not isinstance(value, str):
        return False
    if not value.startswith("0x"):
        return False
    return len(value) == 2 + (nbytes * 2) and all(c in "0123456789abcdefABCDEF" for c in value[2:])


def _is_sha256(value: Any) -> bool:
    return isinstance(value, str) and len(value) == 64 and all(c in "0123456789abcdef" for c in value.lower())


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify AgentHALO Phase 5 artifacts")
    parser.add_argument("--deployment", required=True, help="deploy artifact JSON path")
    parser.add_argument("--e2e", required=True, help="e2e artifact JSON path")
    parser.add_argument(
        "--require-live",
        action="store_true",
        help="fail unless live verification fields indicate successful on-chain attestations",
    )
    args = parser.parse_args()

    deploy_path = Path(args.deployment).resolve()
    e2e_path = Path(args.e2e).resolve()
    errors: list[str] = []
    notes: list[str] = []

    if not deploy_path.exists():
        errors.append(f"missing deployment artifact: {deploy_path}")
    if not e2e_path.exists():
        errors.append(f"missing e2e artifact: {e2e_path}")
    if errors:
        print(json.dumps({"ok": False, "errors": errors}, indent=2))
        return 1

    deploy = _load_json(deploy_path)
    e2e = _load_json(e2e_path)

    if deploy.get("schema") != "agenthalo/phase5/deploy/v1":
        errors.append("deployment schema mismatch")
    if e2e.get("schema") != "agenthalo/phase5/e2e/v1":
        errors.append("e2e schema mismatch")

    if int(deploy.get("chain_id", -1)) != 84532:
        errors.append("deployment chain_id must be 84532")
    if int(e2e.get("chain_id", -1)) != 84532:
        errors.append("e2e chain_id must be 84532")

    if not _is_hex_with_prefix(deploy.get("contract_address"), 20):
        errors.append("deployment contract_address must be 20-byte hex")
    if not _is_hex_with_prefix(deploy.get("tx_hash"), 32):
        errors.append("deployment tx_hash must be 32-byte hex")

    if not _is_sha256(deploy.get("script_sha256")):
        errors.append("deployment script_sha256 must be 64-char hex")
    if not _is_sha256(e2e.get("script_sha256")):
        errors.append("e2e script_sha256 must be 64-char hex")

    if e2e.get("contract_address") != deploy.get("contract_address"):
        errors.append("e2e contract_address does not match deployment contract_address")

    non_anon = e2e.get("non_anonymous") or {}
    anon = e2e.get("anonymous") or {}
    for label, node in (("non_anonymous", non_anon), ("anonymous", anon)):
        if not _is_hex_with_prefix(node.get("attestation_digest"), 32):
            errors.append(f"{label}.attestation_digest must be 32-byte hex")
        if not _is_hex_with_prefix(node.get("tx_hash"), 32):
            errors.append(f"{label}.tx_hash must be 32-byte hex")

    linkage = e2e.get("deployment_evidence_sha256")
    if linkage is not None:
        if not _is_sha256(linkage):
            errors.append("deployment_evidence_sha256 is not a valid sha256 hex string")
        else:
            actual = _sha256(deploy_path)
            if linkage != actual:
                errors.append(
                    "deployment_evidence_sha256 mismatch: "
                    f"expected {actual}, got {linkage}"
                )
    else:
        notes.append("deployment_evidence_sha256 is null")

    if args.require_live:
        if bool(e2e.get("stub_mode")):
            errors.append("require-live set but e2e artifact is marked stub_mode=true")
        for label, node in (("non_anonymous", non_anon), ("anonymous", anon)):
            if node.get("is_verified") is not True:
                errors.append(f"{label}.is_verified must be true in live mode")
            if int(node.get("receipt_status", -1)) != 1:
                errors.append(f"{label}.receipt_status must be 1 in live mode")
            if int(node.get("gas_used", -1)) <= 0:
                errors.append(f"{label}.gas_used must be > 0 in live mode")

    report = {
        "schema": "agenthalo/phase5/artifact-verifier/v1",
        "deployment": str(deploy_path),
        "e2e": str(e2e_path),
        "ok": len(errors) == 0,
        "errors": errors,
        "notes": notes,
    }
    print(json.dumps(report, indent=2))
    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
