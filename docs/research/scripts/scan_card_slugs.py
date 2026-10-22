"""Find card-specific identifiers/strings (snake_case or CamelCase slugs of
card names) in mtg-bot runtime code (everything except cards/parser).

Read-only. Run from the mtg-bot repo root with PYTHONPATH=src.
Prints a summary and writes scripts/_engine_slug_scan.json.
"""
import ast, json, re, collections
from pathlib import Path
from mtg_bot.cards.parser.source import _load_scryfall_data

OUT = Path(__file__).resolve().parents[1]
SRC = Path("src/mtg_bot")
EXCL = ("training", "training_ui", "launcher", "cli", "worker_daemon", "static",
        "templates", "search", "parity", "__pycache__", "cards/parser", "ui.py",
        "interactive.py", "simulate.py", "recommend.py", "agents.py", "features.py",
        "replay.py", "art.py", "review_render.py", "pool_coverage.py",
        "unclaimed_lines.py", "demo_layers.py")

def slugs(name):
    words = re.findall(r"[A-Za-z0-9]+", name.replace("'", ""))
    if not words:
        return None, None
    snake = "_".join(w.lower() for w in words)
    camel = "".join(w[:1].upper() + w[1:].lower() for w in words)
    return snake, camel

cube = {c["name"] for c in _load_scryfall_data("cube")}
allp = {c["name"] for c in _load_scryfall_data("all")}
pats = {}
for n in cube | allp:
    first = n.split(" // ")[0]
    s, c = slugs(first.split(",")[0])  # "Tezzeret, Cruel Captain" -> "tezzeret"
    s2, c2 = slugs(first)
    for cand in {s, s2}:
        if cand and len(cand) >= 9 and "_" in cand:
            pats.setdefault(cand, set()).add(n)
    for cand in {c, c2}:
        if cand and len(cand) >= 10 and sum(ch.isupper() for ch in cand) >= 2:
            pats.setdefault(cand, set()).add(n)
rx = re.compile(r"(?<![A-Za-z0-9])(" + "|".join(sorted(map(re.escape, pats), key=len, reverse=True)) + r")", re.I)
hits = []
for p in sorted(SRC.rglob("*.py")):
    rel = p.as_posix()
    if any(e in rel for e in EXCL):
        continue
    for ln, line in enumerate(p.read_text(encoding="utf-8").splitlines(), 1):
        code = line.split("#", 1)[0]
        for m in rx.finditer(code):
            tok = m.group(1)
            key = next((k for k in pats if k.lower() == tok.lower()), tok)
            names = sorted(pats.get(key, ()))
            hits.append({"file": rel, "line": ln, "token": tok, "cards": names,
                         "in_cube": any(n in cube for n in names), "code": line.strip()[:160]})
by_file = collections.Counter(h["file"] for h in hits)
print(len(hits), "hits;", sum(h["in_cube"] for h in hits), "cube")
for f, n in by_file.most_common(): print(n, f)
(OUT / "scripts" / "_engine_slug_scan.json").write_text(json.dumps(hits, indent=1), encoding="utf-8")
