"""Keep only the Edge of Eternities set's cards in mtg-bot's ``eoe`` pool export.

mtg-bot's ``eoe`` pool is four Scryfall dumps: Edge of Eternities, Stellar Sights,
the EoE Special Guests and Duskmourn (its engine's old test fixtures). The sealed
target is the cards an EoE play booster can hold: the first three dumps, plus the five basic
lands. This drops the Duskmourn-only cards from ``eoe.ir.json`` in place. The schema
is untouched (it was built over the whole export, so ``schema_sha256`` stays valid).

    python tools/codegen/filter_eoe_ir.py --mtg-bot-data <mtg-bot>/data --ir data/cards/eoe.ir.json
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

SET_FILES = ("edge_of_eternities.json", "stellar_sights.json", "special_guests_eoe.json")
BASICS = {"Plains", "Island", "Swamp", "Mountain", "Forest"}


def set_names(data: Path) -> set[str]:
    names: set[str] = set()
    for f in SET_FILES:
        doc = json.loads((data / f).read_text(encoding="utf-8"))
        cards = doc["data"] if isinstance(doc, dict) and "data" in doc else doc
        names |= {c["name"] for c in cards}
    return names | BASICS


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--mtg-bot-data", type=Path, required=True)
    ap.add_argument("--ir", type=Path, required=True)
    a = ap.parse_args()
    want = set_names(a.mtg_bot_data)
    doc = json.loads(a.ir.read_text(encoding="utf-8"))
    before = len(doc["cards"])
    doc["cards"] = [c for c in doc["cards"] if c["name"] in want]
    have = {c["name"] for c in doc["cards"]}
    missing = sorted(want - have)
    with a.ir.open("w", encoding="utf-8", newline="") as f:
        json.dump(doc, f, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    print(f"eoe.ir.json: {before} -> {len(doc['cards'])} cards; set cards {len(want)}; "
          f"without IR: {missing}")
    if not doc["cards"]:
        raise SystemExit("no card kept: wrong --mtg-bot-data?")


if __name__ == "__main__":
    main()
