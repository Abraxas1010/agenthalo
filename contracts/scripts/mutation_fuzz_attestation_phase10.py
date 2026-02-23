#!/usr/bin/env python3
"""Mutation fuzz harness for Phase 9/10 attestation promotion artifacts."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Callable, Dict, List, Tuple


def _run(cmd: List[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=str(cwd), text=True, capture_output=True, check=False)


def _load(path: Path) -> Dict:
    return json.loads(path.read_text(encoding="utf-8"))


def _dump(path: Path, obj: Dict) -> None:
    path.write_text(json.dumps(obj, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Mutation fuzz for attestation promotion artifacts")
    parser.add_argument(
        "--contracts-dir",
        default=".",
        help="contracts directory (default: current working directory)",
    )
    parser.add_argument("--output", help="Optional path to write JSON report")
    args = parser.parse_args()
    contracts_dir = Path(args.contracts_dir).resolve()

    with tempfile.TemporaryDirectory(prefix="nucleusdb_phase10_fuzz_") as tmp:
        tmpdir = Path(tmp)
        out_dir = tmpdir / "promotion_run"

        # Baseline promotion (optional mode for offline dev reproducibility).
        env = dict(os.environ)
        env["BASE_MODE"] = "optional"
        env["OUT_DIR"] = str(out_dir)
        p = subprocess.run(
            ["./scripts/promote_attestation_release.sh"],
            cwd=str(contracts_dir),
            env=env,
            text=True,
            capture_output=True,
            check=False,
        )
        if p.returncode != 0:
            print(p.stdout)
            print(p.stderr)
            raise SystemExit("baseline promote_attestation_release.sh failed")

        promotion_report = out_dir / "promotion_report.json"
        if not promotion_report.exists():
            raise SystemExit(f"baseline promotion report missing: {promotion_report}")

        # Create ephemeral signing keypair and signed bundle for offline replay checks.
        key = tmpdir / "bundle_signing_key.pem"
        pub = tmpdir / "bundle_signing_pub.pem"
        kp = _run(["openssl", "genrsa", "-out", str(key), "2048"], contracts_dir)
        if kp.returncode != 0:
            raise SystemExit(f"openssl genrsa failed: {kp.stderr}")
        pp = _run(["openssl", "rsa", "-in", str(key), "-pubout", "-out", str(pub)], contracts_dir)
        if pp.returncode != 0:
            raise SystemExit(f"openssl pubout failed: {pp.stderr}")

        bundle = _run(
            [
                "python3",
                "./scripts/bundle_attestation_evidence.py",
                "--run-dir",
                str(out_dir),
                "--signing-key",
                str(key),
                "--public-key",
                str(pub),
                "--require-signing",
            ],
            contracts_dir,
        )
        if bundle.returncode != 0:
            raise SystemExit(f"baseline bundle signing failed: {bundle.stderr}")

        baseline_verify = _run(
            [
                "python3",
                "./scripts/verify_attestation_gate_artifacts.py",
                "--run-dir",
                str(out_dir / "gate"),
            ],
            contracts_dir,
        )
        if baseline_verify.returncode != 0:
            raise SystemExit(f"baseline verifier failed: {baseline_verify.stdout}\n{baseline_verify.stderr}")

        baseline_replay = _run(
            [
                "python3",
                "./scripts/replay_attestation_promotion_offline.py",
                "--promotion-report",
                str(promotion_report),
                "--require-signed-bundle",
            ],
            contracts_dir,
        )
        if baseline_replay.returncode != 0:
            raise SystemExit(f"baseline replay failed: {baseline_replay.stdout}\n{baseline_replay.stderr}")

        Mutation = Tuple[str, Callable[[Path], None], List[str]]
        mutations: List[Mutation] = []

        def m_gate_local_fail(run_dir: Path) -> None:
            gate = _load(run_dir / "gate" / "gate_report.json")
            gate["checks"]["forge_test"] = "FAIL"
            _dump(run_dir / "gate" / "gate_report.json", gate)

        mutations.append(
            (
                "gate_local_check_flip",
                m_gate_local_fail,
                [
                    "python3",
                    "./scripts/verify_attestation_gate_artifacts.py",
                    "--run-dir",
                    "{RUN}/gate",
                ],
            )
        )

        def m_gate_overall_fail(run_dir: Path) -> None:
            gate = _load(run_dir / "gate" / "gate_report.json")
            gate["overall"] = "FAIL"
            _dump(run_dir / "gate" / "gate_report.json", gate)

        mutations.append(
            (
                "gate_overall_flip",
                m_gate_overall_fail,
                [
                    "python3",
                    "./scripts/verify_attestation_gate_artifacts.py",
                    "--run-dir",
                    "{RUN}/gate",
                ],
            )
        )

        def m_verifier_ok_false(run_dir: Path) -> None:
            vr = _load(run_dir / "verifier_report.json")
            vr["ok"] = False
            _dump(run_dir / "verifier_report.json", vr)

        mutations.append(
            (
                "verifier_ok_flip",
                m_verifier_ok_false,
                [
                    "python3",
                    "./scripts/replay_attestation_promotion_offline.py",
                    "--promotion-report",
                    "{RUN}/promotion_report.json",
                    "--require-signed-bundle",
                ],
            )
        )

        def m_promotion_mode_required(run_dir: Path) -> None:
            pr = _load(run_dir / "promotion_report.json")
            pr["base_mode"] = "required"
            _dump(run_dir / "promotion_report.json", pr)

        mutations.append(
            (
                "promotion_base_mode_flip",
                m_promotion_mode_required,
                [
                    "python3",
                    "./scripts/replay_attestation_promotion_offline.py",
                    "--promotion-report",
                    "{RUN}/promotion_report.json",
                    "--require-signed-bundle",
                ],
            )
        )

        def m_manifest_hash_tamper(run_dir: Path) -> None:
            manifest = run_dir / "evidence_bundle" / "manifest.sha512"
            lines = manifest.read_text(encoding="utf-8").splitlines()
            if not lines:
                raise RuntimeError("manifest unexpectedly empty")
            digest, rest = lines[0].split("  ", 1)
            flipped = ("0" if digest[0] != "0" else "1") + digest[1:]
            lines[0] = f"{flipped}  {rest}"
            manifest.write_text("\n".join(lines) + "\n", encoding="utf-8")

        mutations.append(
            (
                "manifest_digest_flip",
                m_manifest_hash_tamper,
                [
                    "python3",
                    "./scripts/replay_attestation_promotion_offline.py",
                    "--promotion-report",
                    "{RUN}/promotion_report.json",
                    "--require-signed-bundle",
                ],
            )
        )

        def m_signature_remove(run_dir: Path) -> None:
            (run_dir / "evidence_bundle" / "manifest.sig").unlink(missing_ok=True)

        mutations.append(
            (
                "manifest_signature_remove",
                m_signature_remove,
                [
                    "python3",
                    "./scripts/replay_attestation_promotion_offline.py",
                    "--promotion-report",
                    "{RUN}/promotion_report.json",
                    "--require-signed-bundle",
                ],
            )
        )

        results = []
        for case_name, mutate, check_cmd_tmpl in mutations:
            case_dir = tmpdir / f"case_{case_name}"
            shutil.copytree(out_dir, case_dir)
            mutate(case_dir)
            check_cmd = [a.replace("{RUN}", str(case_dir)) for a in check_cmd_tmpl]
            r = _run(check_cmd, contracts_dir)
            passed = r.returncode != 0  # expect fail-closed rejection
            results.append(
                {
                    "case": case_name,
                    "expected": "reject",
                    "returncode": r.returncode,
                    "ok": passed,
                    "stdout": r.stdout[-600:],
                    "stderr": r.stderr[-600:],
                }
            )

        ok = all(r["ok"] for r in results)
        report = {
            "schema": "nucleusdb/attestation-phase10-mutation-fuzz/v1",
            "ok": ok,
            "total_cases": len(results),
            "results": results,
        }
        payload = json.dumps(report, indent=2, sort_keys=True)
        print(payload)
        if args.output:
            out = Path(args.output)
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(payload + "\n", encoding="utf-8")
        return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
