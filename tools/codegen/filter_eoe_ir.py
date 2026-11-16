"""Keep only the Edge of Eternities set's cards in the card-IR export's ``eoe`` pool.

The ``eoe`` pool (ir/mtgz_ir/cards/pools.py) is four Scryfall dumps: Edge of Eternities, Stellar
Sights, the EoE Special Guests and Duskmourn (the old Python engine's test fixtures). The sealed
target is the cards an EoE play booster can hold: the first three dumps, plus the five basic
lands. This drops the Duskmourn-only cards from ``eoe.ir.json``. The schema is untouched (it was
built over the whole export, so ``schema_sha256`` stays valid). ``tools/card_ir.py export`` runs it;
by hand:

    python tools/codegen/filter_eoe_ir.py --ir data/cards/eoe.ir.json [--data ir/data]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

SET_FILES = ("edge_of_eternities.json", "stellar_sights.json", "special_guests_eoe.json")
BASICS = {"Plains", "Island", "Swamp", "Mountain", "Forest"}
DEFAULT_DATA = Path(__file__).resolve().parents[2] / "ir" / "data"


def set_names(data: Path) -> set[str]:
    names: set[str] = set()
    for f in SET_FILES:
        doc = json.loads((data / f).read_text(encoding="utf-8"))
        cards = doc["data"] if isinstance(doc, dict) and "data" in doc else doc
        names |= {c["name"] for c in cards}
    return names | BASICS


def filter_text(ir_text: str, data: Path) -> tuple[str, str]:
    """The ``eoe`` document's JSON text -> (the filtered document's text, a one-line report)."""
    want = set_names(data)
    doc = json.loads(ir_text)
    before = len(doc["cards"])
    doc["cards"] = [c for c in doc["cards"] if c["name"] in want]
    if not doc["cards"]:
        raise SystemExit("no card kept: wrong --data?")
    have = {c["name"] for c in doc["cards"]}
    missing = sorted(want - have)
    text = json.dumps(doc, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return text, (f"eoe.ir.json: {before} -> {len(doc['cards'])} cards; set cards {len(want)}; "
                  f"without IR: {missing}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--data", type=Path, default=DEFAULT_DATA, help="the Scryfall dumps (default ir/data)")
    ap.add_argument("--ir", type=Path, required=True)
    a = ap.parse_args()
    text, report = filter_text(a.ir.read_text(encoding="utf-8"), a.data)
    with a.ir.open("w", encoding="utf-8", newline="") as f:
        f.write(text)
    print(report)


if __name__ == "__main__":
    main()
