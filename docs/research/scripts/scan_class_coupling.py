"""For every class reachable from the cube printings, count references to
its name in runtime code OUTSIDE its defining module (excluding the parser,
parity, search, training, UI).  isinstance() hits are counted separately:
they are engine-side type dispatch that an IR interpreter must replicate.
Read-only; run from the mtg-bot repo root."""
import json, re, collections
from pathlib import Path
OUT = Path(__file__).resolve().parents[1]
inv = json.loads((OUT / "card_ir_inventory.json").read_text(encoding="utf-8"))
SRC = Path("src/mtg_bot")
EXCL = ("/training", "/launcher", "/cli/", "worker_daemon", "/static", "/templates",
        "/search/", "/parity/", "/cards/parser/", "ui.py", "interactive.py",
        "review_render.py", "pool_coverage.py", "unclaimed_lines.py", "recommend.py",
        "agents.py", "features.py", "__init__.py")
files = {p.as_posix(): p.read_text(encoding="utf-8") for p in SRC.rglob("*.py")
         if not any(e in p.as_posix() for e in EXCL)}
res = {}
for c in inv["classes"]:
    name = c["class"]
    if c["core_data"] or name in ("Event",):
        continue
    mod_file = "src/" + c["module"].replace(".", "/") + ".py"
    pat = re.compile(r"\b" + re.escape(name) + r"\b")
    ipat = re.compile(r"isinstance\([^)]*\b" + re.escape(name) + r"\b")
    refs = collections.Counter(); isn = collections.Counter()
    for f, txt in files.items():
        if f == mod_file:
            continue
        for line in txt.splitlines():
            code = line.split("#", 1)[0]
            if pat.search(code):
                refs[f] += 1
                if ipat.search(code):
                    isn[f] += 1
    res[c["qualname"]] = {"cube_cards": c["cube_cards"], "external_refs": sum(refs.values()),
                          "isinstance_refs": sum(isn.values()), "isinstance_files": dict(isn)}
tot_isn = sum(v["isinstance_refs"] for v in res.values())
n_isn = sum(1 for v in res.values() if v["isinstance_refs"])
print("classes with external isinstance dispatch:", n_isn, "total isinstance sites:", tot_isn)
for q, v in sorted(res.items(), key=lambda kv: -kv[1]["isinstance_refs"])[:40]:
    print(v["isinstance_refs"], q.split(".")[-1], v["cube_cards"], list(v["isinstance_files"].items())[:4])
(OUT / "scripts" / "_class_coupling.json").write_text(json.dumps(res, indent=1), encoding="utf-8")
