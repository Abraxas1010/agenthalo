#!/usr/bin/env python3
"""Create a signed attestation evidence bundle for a promotion run."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import tarfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List


def _sha512_file(path: Path) -> str:
    h = hashlib.sha512()
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def _load_json(path: Path) -> Dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def _run(cmd: List[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, check=True, text=True, capture_output=True)


def _resolve_artifacts(run_dir: Path) -> List[Path]:
    required = [
        run_dir / "promotion_report.json",
        run_dir / "verifier_report.json",
        run_dir / "gate" / "gate_report.json",
    ]
    missing = [p for p in required if not p.exists()]
    if missing:
        raise FileNotFoundError(
            "missing required promotion artifacts: " + ", ".join(str(m) for m in missing)
        )

    gate = _load_json(run_dir / "gate" / "gate_report.json")
    files = list(required)
    maybe_base = gate.get("base_evidence_file")
    if isinstance(maybe_base, str) and maybe_base.strip():
        base = Path(maybe_base)
        if not base.is_absolute():
            base = (run_dir / base).resolve()
        if not base.exists():
            raise FileNotFoundError(f"base_evidence_file missing: {base}")
        files.append(base)
    return files


def _sign_manifest(manifest: Path, key: Path, pub: Path, sig_out: Path) -> None:
    _run(
        [
            "openssl",
            "dgst",
            "-sha256",
            "-sign",
            str(key),
            "-out",
            str(sig_out),
            str(manifest),
        ]
    )
    _run(
        [
            "openssl",
            "dgst",
            "-sha256",
            "-verify",
            str(pub),
            "-signature",
            str(sig_out),
            str(manifest),
        ]
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Bundle and sign attestation promotion artifacts")
    parser.add_argument("--run-dir", required=True, help="Promotion run directory")
    parser.add_argument("--signing-key", help="PEM private key for manifest signing")
    parser.add_argument("--public-key", help="PEM public key for signature verification")
    parser.add_argument(
        "--require-signing",
        action="store_true",
        help="Fail if signing inputs are missing",
    )
    parser.add_argument("--output", help="Optional JSON report output path")
    args = parser.parse_args()

    run_dir = Path(args.run_dir).resolve()
    bundle_dir = run_dir / "evidence_bundle"
    bundle_files_dir = bundle_dir / "files"
    bundle_dir.mkdir(parents=True, exist_ok=True)
    bundle_files_dir.mkdir(parents=True, exist_ok=True)

    files = _resolve_artifacts(run_dir)
    rel_entries: List[str] = []
    for src in files:
        rel = src.resolve().relative_to(run_dir)
        rel_entries.append(str(rel))
        dst = bundle_files_dir / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)

    rel_entries.sort()
    manifest = bundle_dir / "manifest.sha512"
    with manifest.open("w", encoding="utf-8") as f:
        for rel in rel_entries:
            digest = _sha512_file(run_dir / rel)
            f.write(f"{digest}  {rel}\n")

    tar_path = bundle_dir / "evidence_bundle.tar.gz"
    with tarfile.open(tar_path, "w:gz") as tf:
        for rel in rel_entries:
            tf.add(run_dir / rel, arcname=rel)
        tf.add(manifest, arcname="manifest.sha512")

    signed = False
    sig_path = bundle_dir / "manifest.sig"
    pub_copy_path = bundle_dir / "manifest_pub.pem"
    if args.signing_key and args.public_key:
        key = Path(args.signing_key).resolve()
        pub = Path(args.public_key).resolve()
        if not key.exists():
            raise FileNotFoundError(f"signing key missing: {key}")
        if not pub.exists():
            raise FileNotFoundError(f"public key missing: {pub}")
        _sign_manifest(manifest, key, pub, sig_path)
        shutil.copy2(pub, pub_copy_path)
        signed = True
    elif args.require_signing:
        raise RuntimeError("--require-signing set but signing key/public key were not provided")

    metadata = {
        "schema": "nucleusdb/attestation-evidence-bundle/v1",
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "run_dir": str(run_dir),
        "artifact_files": rel_entries,
        "manifest": str(manifest),
        "bundle_tar": str(tar_path),
        "signed": signed,
        "signature_file": str(sig_path) if signed else None,
        "public_key_file": str(pub_copy_path) if signed else None,
    }
    metadata_path = bundle_dir / "bundle_metadata.json"
    metadata_path.write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    report = {
        "ok": True,
        "bundle_dir": str(bundle_dir),
        "manifest": str(manifest),
        "bundle_tar": str(tar_path),
        "signed": signed,
        "metadata": str(metadata_path),
    }
    payload = json.dumps(report, indent=2, sort_keys=True)
    print(payload)
    if args.output:
        out = Path(args.output)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(payload + "\n", encoding="utf-8")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

