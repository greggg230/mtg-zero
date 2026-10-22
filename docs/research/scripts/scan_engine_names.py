"""Find card-name string literals in mtg-bot source (engine + parser),
and where printing-level flag fields are consumed.

Read-only. Run from the mtg-bot repo root:

    PYTHONPATH=src .venv-py313/Scripts/python.exe \
        "C:/Users/Gregg Keithley/mtg-zero/docs/research/scripts/scan_engine_names.py"

Writes ``scripts/_engine_name_scan.json``.
"""

from __future__ import annotations

import ast
import collections
import json
import re
from pathlib import Path

from mtg_bot.cards.parser.source import _load_scryfall_data

OUT_DIR = Path(__file__).resolve().parents[1]
REPO = Path.cwd()
SRC = REPO / "src" / "mtg_bot"

BASIC = {"Plains", "Island", "Swamp", "Mountain", "Forest", "Wastes"}
# Card names that are also common English words / subtypes and produce
# noise when they appear as string literals.
AMBIGUOUS = BASIC | {
    "Treasure", "Food", "Clue", "Map", "Blood", "Gold", "Shard",
}

EXCLUDE_DIRS = (
    "training", "training_ui", "launcher", "cli", "worker_daemon",
    "static", "templates", "search", "__pycache__",
)


def card_names() -> tuple[set[str], set[str]]:
    cube = {c["name"] for c in _load_scryfall_data("cube")}
    allp = {c["name"] for c in _load_scryfall_data("all")}
    # Faces too.
    for c in _load_scryfall_data("all"):
        for f in c.get("card_faces") or ():
            if f.get("name"):
                allp.add(f["name"])
    return cube, allp


def docstring_nodes(tree) -> set[int]:
    ids = set()
    for node in ast.walk(tree):
        if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef,
                             ast.AsyncFunctionDef)):
            body = getattr(node, "body", [])
            if body and isinstance(body[0], ast.Expr) and isinstance(
                getattr(body[0], "value", None), ast.Constant
            ) and isinstance(body[0].value.value, str):
                ids.add(id(body[0].value))
    return ids


def classify_use(parents: list) -> str:
    """Rough classification of how a string constant is used."""
    for p in reversed(parents[-4:]):
        if isinstance(p, ast.Compare):
            return "compare"
        if isinstance(p, ast.Dict):
            return "dict_key_or_value"
        if isinstance(p, (ast.Set, ast.Tuple, ast.List)):
            continue
        if isinstance(p, ast.Call):
            fn = p.func
            name = getattr(fn, "id", None) or getattr(fn, "attr", None)
            return f"call:{name}"
        if isinstance(p, ast.Assign):
            return "assign"
    return "other"


def scan(path: Path, names: set[str]):
    try:
        src = path.read_text(encoding="utf-8")
        tree = ast.parse(src)
    except Exception:
        return []
    docs = docstring_nodes(tree)
    hits = []

    def visit(node, parents):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            if id(node) not in docs and node.value in names:
                hits.append((node.lineno, node.value, classify_use(parents)))
        for child in ast.iter_child_nodes(node):
            visit(child, parents + [node])

    visit(tree, [])
    return hits


def main():
    cube, allp = card_names()
    names = (cube | allp) - AMBIGUOUS
    results = []
    for path in sorted(SRC.rglob("*.py")):
        rel = path.relative_to(REPO).as_posix()
        if any(f"/{d}/" in rel or rel.split("/")[-1].startswith(d) for d in EXCLUDE_DIRS):
            continue
        layer = "parser" if "/cards/parser/" in rel else (
            "cards" if "/cards/" in rel else (
                "effects" if "/effects/" in rel else "engine"))
        for line, value, use in scan(path, names):
            results.append({
                "file": rel, "line": line, "name": value, "use": use,
                "layer": layer, "in_cube": value in cube,
            })
    by_layer = collections.Counter(r["layer"] for r in results)
    by_file = collections.Counter(r["file"] for r in results)
    cube_by_layer = collections.Counter(r["layer"] for r in results if r["in_cube"])

    # Where printing-level flags are consumed.
    inv = json.loads((OUT_DIR / "card_ir_inventory.json").read_text(encoding="utf-8"))
    flag_feats = [f for f in inv["features"] if f.startswith("field:")]
    engine_files = [
        p for p in SRC.rglob("*.py")
        if "/cards/parser/" not in p.as_posix()
        and not any(f"/{d}" in p.as_posix() for d in EXCLUDE_DIRS)
    ]
    texts = {p.relative_to(REPO).as_posix(): p.read_text(encoding="utf-8") for p in engine_files}
    flag_sites = {}
    for feat in flag_feats:
        cls, field = feat[len("field:"):].split(".", 1)
        pat = re.compile(r"\b" + re.escape(field) + r"\b")
        sites = {}
        for rel, txt in texts.items():
            n = 0
            for ln, l in enumerate(txt.splitlines(), 1):
                s = l.strip()
                if s.startswith("#"):
                    continue
                if pat.search(l):
                    n += 1
            if n:
                sites[rel] = n
        flag_sites[feat] = {
            "cube_cards": inv["features"][feat],
            "n_files": len(sites),
            "n_lines": sum(sites.values()),
            "files": dict(sorted(sites.items(), key=lambda kv: -kv[1])),
        }

    out = {
        "n_hits": len(results),
        "by_layer": dict(by_layer),
        "cube_hits_by_layer": dict(cube_by_layer),
        "by_file": dict(by_file.most_common()),
        "hits": results,
        "flag_consumers": flag_sites,
    }
    (OUT_DIR / "scripts" / "_engine_name_scan.json").write_text(json.dumps(out, indent=1), encoding="utf-8")
    print("hits", len(results), dict(by_layer), "cube-only:", dict(cube_by_layer))
    for f, n in by_file.most_common(40):
        print(n, f)


if __name__ == "__main__":
    main()
