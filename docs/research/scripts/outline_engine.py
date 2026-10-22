"""Dump top-level function/class spans (name, start, length) of mtg-bot
runtime modules. Read-only. Run from the mtg-bot repo root."""
import ast, json, sys
from pathlib import Path
OUT = Path(__file__).resolve().parents[1]
mods = sys.argv[1:]
res = {}
for m in mods:
    p = Path(m)
    tree = ast.parse(p.read_text(encoding="utf-8"))
    items = []
    for n in tree.body:
        if isinstance(n, (ast.FunctionDef, ast.ClassDef, ast.AsyncFunctionDef)):
            items.append((n.name, n.lineno, n.end_lineno - n.lineno + 1,
                          "class" if isinstance(n, ast.ClassDef) else "def"))
    res[m] = items
(OUT / "scripts" / "_engine_outline.json").write_text(json.dumps(res, indent=0), encoding="utf-8")
for m, items in res.items():
    print("==", m, len(items))
    print("  ".join(f"{n}@{l}+{s}" for n, l, s, k in items))
