#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


# Bundled scripts live at <root>/python/living_agent/living_agent_common.py in
# both the source checkout and the packaged AgentHALO layout.
_SCRIPT_DIR = Path(__file__).resolve().parent
_ROOT_CANDIDATE = _SCRIPT_DIR.parents[1]

AGENTHALO_ROOT = Path(
    os.environ.get("AGENTHALO_ROOT", str(_ROOT_CANDIDATE))
).resolve()
REPO_ROOT = Path(os.environ.get("HEYTING_ROOT", str(AGENTHALO_ROOT))).resolve()

DEFAULT_LIVING_AGENT_ROOT = Path(os.environ.get("LIVING_AGENT_ROOT", "/tmp/the-living-agent"))
DEFAULT_AGENTHALO_HOME = Path(
    os.environ.get(
        "AGENTHALO_HOME",
        os.environ.get("NUCLEUSDB_HOME", str(Path.home() / ".agenthalo")),
    )
).resolve()
DEFAULT_NUCLEUSDB_ROOT = Path(os.environ.get(
    "NUCLEUSDB_ROOT",
    str(AGENTHALO_ROOT) if AGENTHALO_ROOT.exists() else "/home/abraxas/Work/nucleusdb",
))

_BUNDLED_ARTIFACT_ROOT = AGENTHALO_ROOT / "artifacts" / "living_agent"
_LEGACY_ARTIFACT_ROOT = DEFAULT_LIVING_AGENT_ROOT / "heyting_artifacts"
_default_artifact = str(DEFAULT_AGENTHALO_HOME / "living_agent_artifacts")
DEFAULT_ARTIFACT_ROOT = Path(
    os.environ.get("HEYTING_ARTIFACT_DIR", _default_artifact)
).resolve()
DEFAULT_GRID_ROOT = Path(
    os.environ.get("HEYTING_GRID_ROOT", str(DEFAULT_ARTIFACT_ROOT / "verified_grid"))
).resolve()
TOKEN_RE = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.-]*")
_SEEDED_TARGETS: set[str] = set()


def bundled_artifact_root() -> Path | None:
    for candidate in (_BUNDLED_ARTIFACT_ROOT, REPO_ROOT / "artifacts" / "living_agent"):
        if candidate.is_dir():
            return candidate
    return _LEGACY_ARTIFACT_ROOT if _LEGACY_ARTIFACT_ROOT.is_dir() else None


def ensure_seed_artifacts(artifact_root: Path | None = None) -> Path:
    target = (artifact_root or DEFAULT_ARTIFACT_ROOT).resolve()
    target_key = str(target)
    if target_key in _SEEDED_TARGETS:
        return target
    seed = bundled_artifact_root()
    target.mkdir(parents=True, exist_ok=True)
    if not seed or seed.resolve() == target:
        _SEEDED_TARGETS.add(target_key)
        return target
    copy_targets = [
        "paper_embeddings.json",
        "paper_embeddings.npz",
        "sns_model_info.json",
        "verified_grid",
    ]
    for rel in copy_targets:
        src = seed / rel
        dst = target / rel
        if not src.exists():
            continue
        if src.is_dir():
            if not dst.is_dir():
                shutil.copytree(src, dst)
        else:
            if not dst.exists():
                shutil.copy2(src, dst)
    _SEEDED_TARGETS.add(target_key)
    return target


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=False)
        handle.write("\n")


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def normalize_whitespace(text: str) -> str:
    if text is None:
        return ""
    if not isinstance(text, str):
        try:
            text = json.dumps(text, sort_keys=True)
        except TypeError:
            text = str(text)
    return re.sub(r"\s+", " ", text).strip()


def tokenize(text: str) -> list[str]:
    return [tok.lower() for tok in TOKEN_RE.findall(text)]


def python_runtime() -> str:
    env_override = os.environ.get("LIVING_AGENT_EMBED_PYTHON")
    if env_override:
        return env_override
    venv_python = DEFAULT_LIVING_AGENT_ROOT / ".venv" / "bin" / "python"
    if venv_python.exists():
        return str(venv_python)
    return sys.executable


def ensure_module_runtime(module_name: str) -> None:
    if importlib.util.find_spec(module_name) is not None:
        return
    preferred = python_runtime()
    if preferred == sys.executable:
        return
    os.execv(preferred, [preferred] + sys.argv)


def run(
    argv: list[str],
    *,
    cwd: Path | None = None,
    input_text: str | None = None,
    env: dict[str, str] | None = None,
    check: bool = True,
    timeout_secs: int | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        argv,
        cwd=str(cwd or REPO_ROOT),
        input=input_text,
        text=True,
        capture_output=True,
        env=env,
        check=check,
        timeout=timeout_secs,
    )


def run_json(
    argv: list[str],
    *,
    cwd: Path | None = None,
    input_text: str | None = None,
) -> Any:
    proc = run(argv, cwd=cwd, input_text=input_text, check=False)
    if proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(argv)}\n"
            f"stdout:\n{proc.stdout}\n"
            f"stderr:\n{proc.stderr}"
        )
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            f"command returned non-JSON output: {' '.join(argv)}\n{proc.stdout}"
        ) from exc
