#!/usr/bin/env python3
"""Extract proof-graph data from NucleusDB/AgentHALO Lean 4 files.

Produces a JSON file suitable for the dashboard proof-lattice visualization.
Extracts: theorems, lemmas, defs, structures, instances, examples, axioms.
Computes: family (directory), kind, dependencies (import edges), 3D layout.
"""

import json, math, os, re, sys, hashlib, random
from pathlib import Path
from collections import defaultdict

LEAN_ROOT = Path(__file__).resolve().parent.parent / "lean"
OUT = Path(__file__).resolve().parent.parent / "dashboard" / "proof-lattice.json"

# Patterns for declarations
DECL_RE = re.compile(
    r"^(theorem|lemma|def|noncomputable def|structure|class|instance|example|axiom|abbrev)\s+"
    r"(\S+)",
    re.MULTILINE,
)
IMPORT_RE = re.compile(r"^import\s+(\S+)", re.MULTILINE)


def family_from_path(rel: str) -> str:
    """Map file path to a display family."""
    parts = rel.replace(".lean", "").split("/")
    # Skip NucleusDB prefix
    if parts and parts[0] == "NucleusDB":
        parts = parts[1:]
    if not parts:
        return "Core"
    return parts[0]


def kind_normalize(k: str) -> str:
    if k in ("noncomputable def", "abbrev"):
        return "def"
    return k


def extract_file(path: Path, rel: str):
    """Extract declarations and imports from a Lean file."""
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except Exception:
        return [], []

    decls = []
    for m in DECL_RE.finditer(text):
        kind_raw = m.group(1)
        name = m.group(2)
        line = text[: m.start()].count("\n") + 1
        decls.append({
            "name": name,
            "kind": kind_normalize(kind_raw),
            "line": line,
            "file": rel,
            "family": family_from_path(rel),
        })

    imports = []
    for m in IMPORT_RE.finditer(text):
        imports.append(m.group(1))

    return decls, imports


def stable_hash(s: str) -> float:
    """Deterministic hash -> [0, 1)."""
    h = hashlib.md5(s.encode()).hexdigest()
    return int(h[:8], 16) / 0xFFFFFFFF


def layout_3d(items, families):
    """Assign 3D positions: clustered by family with jitter."""
    fam_list = sorted(set(families))
    fam_idx = {f: i for i, f in enumerate(fam_list)}
    n_fam = max(len(fam_list), 1)

    positions = []
    for it in items:
        fi = fam_idx.get(it["family"], 0)
        # Arrange families on a sphere shell
        phi = (fi / n_fam) * math.pi * 2
        theta = 0.3 + (fi % 5) * 0.25  # latitude spread

        # Base position on sphere
        r = 40
        bx = r * math.sin(theta) * math.cos(phi)
        by = r * math.sin(theta) * math.sin(phi)
        bz = r * math.cos(theta)

        # Jitter based on item name (deterministic)
        jx = (stable_hash(it["name"] + "x") - 0.5) * 18
        jy = (stable_hash(it["name"] + "y") - 0.5) * 18
        jz = (stable_hash(it["name"] + "z") - 0.5) * 18

        positions.append({
            "x": round(bx + jx, 2),
            "y": round(by + jy, 2),
            "z": round(bz + jz, 2),
        })
    return positions


def main():
    lean_files = sorted(LEAN_ROOT.rglob("*.lean"))
    lean_files = [f for f in lean_files if ".lake" not in str(f)]

    all_decls = []
    file_imports = {}  # rel_path -> [import_module]

    for f in lean_files:
        rel = str(f.relative_to(LEAN_ROOT))
        decls, imports = extract_file(f, rel)
        all_decls.extend(decls)
        file_imports[rel] = imports

    # Build edges: file-level import dependencies mapped to declaration pairs
    # Map module name -> file path
    mod_to_file = {}
    for f in lean_files:
        rel = str(f.relative_to(LEAN_ROOT))
        mod = rel.replace("/", ".").replace(".lean", "")
        mod_to_file[mod] = rel

    # Map file -> list of declaration indices
    file_to_decl_idx = defaultdict(list)
    for i, d in enumerate(all_decls):
        file_to_decl_idx[d["file"]].append(i)

    edges = set()
    for src_file, imports in file_imports.items():
        src_indices = file_to_decl_idx.get(src_file, [])
        for imp in imports:
            tgt_file = mod_to_file.get(imp)
            if tgt_file:
                tgt_indices = file_to_decl_idx.get(tgt_file, [])
                if src_indices and tgt_indices:
                    # Connect first decl of importer to first decl of imported
                    edges.add((src_indices[0], tgt_indices[0]))

    # Build kNN edges based on family proximity + name similarity
    # (lightweight: connect items within same family)
    family_groups = defaultdict(list)
    for i, d in enumerate(all_decls):
        family_groups[d["family"]].append(i)

    knn_edges = set()
    for fam, indices in family_groups.items():
        for j in range(len(indices) - 1):
            knn_edges.add((indices[j], indices[j + 1]))
        # Also connect some cross-family edges for visual coherence
        if len(indices) > 3:
            # Connect to a random item in each nearby family
            pass

    all_edges = list(edges | knn_edges)

    families = sorted(set(d["family"] for d in all_decls))
    positions = layout_3d(all_decls, families)

    # 2D positions (projection)
    nodes = []
    for i, d in enumerate(all_decls):
        p3 = positions[i]
        nodes.append({
            "id": i,
            "name": d["name"],
            "family": d["family"],
            "kind": d["kind"],
            "file": d["file"],
            "line": d["line"],
            "x": p3["x"],
            "y": p3["y"],
            "z": p3["z"],
            "importance": 1.0 + (0.5 if d["kind"] == "theorem" else
                                  0.3 if d["kind"] == "lemma" else
                                  0.2 if d["kind"] == "structure" else 0.0),
        })

    result = {
        "nodes": nodes,
        "edges": all_edges,
        "families": families,
        "stats": {
            "total_files": len(lean_files),
            "total_declarations": len(nodes),
            "total_edges": len(all_edges),
            "families": len(families),
            "by_kind": dict(sorted(
                defaultdict(int, {d["kind"]: 0 for d in all_decls}).items()
            )),
        },
    }

    # Count by kind
    kind_counts = defaultdict(int)
    for d in all_decls:
        kind_counts[d["kind"]] += 1
    result["stats"]["by_kind"] = dict(sorted(kind_counts.items()))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w") as f:
        json.dump(result, f)

    print(f"Extracted {len(nodes)} declarations from {len(lean_files)} files")
    print(f"  Families: {', '.join(families)}")
    print(f"  Edges: {len(all_edges)}")
    print(f"  By kind: {dict(kind_counts)}")
    print(f"  Output: {OUT}")


if __name__ == "__main__":
    main()
