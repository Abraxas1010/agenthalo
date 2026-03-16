#!/usr/bin/env python3

from __future__ import annotations

import os
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SKIP_DIRS = {
    ".git",
    ".lake",
    "target",
    "node_modules",
    "dist",
    "build",
    "vendor",
    "__pycache__",
}
TEXT_SUFFIXES = {
    ".md",
    ".toml",
    ".yaml",
    ".yml",
    ".json",
    ".rs",
    ".py",
    ".sh",
    ".js",
    ".ts",
    ".tsx",
    ".css",
    ".html",
    ".lean",
    ".txt",
    ".cff",
}
TEXT_NAMES = {"AGENTS.md", "README", "README.md", "CHANGELOG", "CHANGELOG.md"}


def decode(items: tuple[int, ...]) -> str:
    return "".join(chr(item) for item in items)


TEXT_PATTERNS = tuple(
    pattern.lower()
    for pattern in (
        decode((85, 68, 84)),
        decode((85, 110, 105, 111, 110, 32, 68, 105, 112, 111, 108, 101)),
        decode((65, 108, 45, 77, 97, 121, 97, 104, 105)),
        decode((65, 98, 100, 117, 108, 115, 97, 108, 97, 109)),
        decode((99, 111, 104, 101, 114, 101, 110, 99, 101, 95, 99, 111, 110, 115, 116, 114, 97, 105, 110, 116)),
        decode((99, 111, 104, 101, 114, 101, 110, 99, 101, 95, 112, 101, 114, 115, 105, 115, 116, 101, 110, 99, 101)),
        decode((115, 116, 114, 117, 99, 116, 117, 114, 97, 108, 95, 112, 101, 114, 115, 105, 115, 116, 101, 110, 99, 101)),
        decode((99, 111, 104, 101, 114, 101, 110, 99, 101, 95, 100, 114, 105, 118, 101, 110)),
        decode((99, 111, 104, 101, 114, 101, 110, 99, 101, 95, 102, 105, 101, 108, 100)),
        decode((67, 95, 99, 114, 105, 116)),
        decode((99, 95, 99, 114, 105, 116)),
        decode((99, 111, 104, 101, 114, 101, 110, 99, 101, 95, 116, 104, 114, 101, 115, 104, 111, 108, 100)),
        decode((112, 101, 114, 115, 105, 115, 116, 101, 110, 99, 101, 95, 119, 101, 105, 103, 104, 116)),
        decode((100, 119, 95, 100, 116, 97, 117)),
        decode((112, 101, 114, 115, 105, 115, 116, 101, 110, 99, 101, 95, 100, 121, 110, 97, 109, 105, 99, 115)),
        decode((115, 116, 114, 117, 99, 116, 117, 114, 97, 108, 95, 99, 111, 104, 101, 114, 101, 110, 99, 101)),
        decode((115, 116, 114, 117, 99, 116, 117, 114, 97, 108, 95, 116, 105, 109, 101)),
        decode((99, 108, 111, 99, 107, 95, 114, 97, 116, 101, 95, 102, 105, 101, 108, 100)),
        decode((99, 104, 105, 95, 102, 105, 101, 108, 100)),
        decode((100, 116, 95, 100, 116, 97, 117)),
        decode((114, 101, 97, 99, 116, 105, 111, 110, 95, 100, 105, 102, 102, 117, 115, 105, 111, 110)),
        decode((99, 111, 104, 101, 114, 101, 110, 99, 101, 95, 100, 105, 102, 102, 117, 115, 105, 111, 110)),
        decode((103, 114, 97, 112, 104, 95, 108, 97, 112, 108, 97, 99, 105, 97, 110)),
        decode((84, 119, 111, 67, 108, 111, 99, 107, 80, 114, 111, 106, 101, 99, 116, 105, 111, 110)),
        decode((66, 101, 116, 97, 67, 111, 117, 112, 108, 105, 110, 103)),
        decode((78, 101, 99, 101, 115, 115, 105, 116, 121, 84, 104, 101, 111, 114, 101, 109)),
        decode((116, 97, 117, 95, 101, 112, 111, 99, 104)),
        decode((84, 97, 117, 69, 112, 111, 99, 104)),
    )
)
FILENAME_PATTERNS = tuple(
    pattern.lower()
    for pattern in (
        decode((99, 111, 104, 101, 114, 101, 110, 99, 101, 95, 99, 111, 110, 115, 116, 114, 97, 105, 110, 116)),
        decode((117, 100, 116)),
        decode((97, 108, 95, 109, 97, 121, 97, 104, 105)),
        decode((115, 116, 114, 117, 99, 116, 117, 114, 97, 108, 95, 112, 101, 114, 115, 105, 115, 116, 101, 110, 99, 101)),
        decode((116, 97, 117, 95, 101, 112, 111, 99, 104)),
        decode((116, 119, 111, 95, 99, 108, 111, 99, 107)),
        decode((98, 101, 116, 97, 95, 99, 111, 117, 112, 108, 105, 110, 103)),
    )
)


def is_text_path(path: Path) -> bool:
    return path.suffix.lower() in TEXT_SUFFIXES or path.name in TEXT_NAMES


def iter_repo_files() -> tuple[Path, ...]:
    collected: list[Path] = []
    for root, dirs, files in os.walk(ROOT):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        root_path = Path(root)
        for file_name in files:
            collected.append(root_path / file_name)
    return tuple(collected)


def main() -> int:
    name_hits: list[str] = []
    text_hits: list[str] = []

    for path in iter_repo_files():
        rel = path.relative_to(ROOT)
        lower_name = path.name.lower()
        for pattern in FILENAME_PATTERNS:
            if pattern in lower_name:
                name_hits.append(f"{rel}: filename matches restricted fragment")
                break

        if not is_text_path(path):
            continue
        try:
            content = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        lowered = content.lower()
        for pattern in TEXT_PATTERNS:
            if pattern in lowered:
                text_hits.append(f"{rel}: contains restricted identifier")
                break

    if name_hits or text_hits:
        for line in sorted(name_hits + text_hits):
            print(line)
        return 1

    print("theory boundary scan passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
