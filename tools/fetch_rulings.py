"""Fetch the official rulings of the PowerLSV cube's cards from Scryfall (M8d).

Writes `data/rulings/powerlsv.rulings.json`: one entry per cube card (cube order),
with its name, oracle_id and rulings (date, source, text), the rulings sorted by
date then text so the file is stable.

Re-runnable and cached: each card's raw Scryfall response is kept in
`data/rulings/.cache/<oracle_id>.json` (gitignored); a cached card is not fetched
again unless `--refresh` is given. Scryfall's limits are respected: at most
~8 requests per second, a User-Agent and an Accept header.

    python tools/fetch_rulings.py [--cube PATH] [--refresh]
"""

import argparse
import json
import os
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_CUBE = os.path.join(
    os.path.expanduser("~"), "Documents", "Claude", "Projects", "mtg-bot", "data", "cube_power_lsv.json"
)
OUT_DIR = os.path.join(ROOT, "data", "rulings")
CACHE = os.path.join(OUT_DIR, ".cache")
OUT = os.path.join(OUT_DIR, "powerlsv.rulings.json")
HEADERS = {"User-Agent": "mtg-zero-rulings/1.0 (engine tests)", "Accept": "application/json"}


def fetch(url):
    for attempt in range(5):
        try:
            req = urllib.request.Request(url, headers=HEADERS)
            with urllib.request.urlopen(req, timeout=30) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code == 429 or e.code >= 500:
                time.sleep(2.0 * (attempt + 1))
                continue
            raise
        except urllib.error.URLError:
            time.sleep(2.0 * (attempt + 1))
    raise RuntimeError(f"failed to fetch {url}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cube", default=DEFAULT_CUBE)
    ap.add_argument("--refresh", action="store_true")
    a = ap.parse_args()
    cube = json.load(open(a.cube, encoding="utf-8"))
    os.makedirs(CACHE, exist_ok=True)
    out, fetched = [], 0
    for c in cube:
        path = os.path.join(CACHE, c["oracle_id"] + ".json")
        if a.refresh or not os.path.exists(path):
            raw = fetch(c["rulings_uri"])
            with open(path, "w", encoding="utf-8") as f:
                json.dump(raw, f)
            fetched += 1
            time.sleep(0.125)
        raw = json.load(open(path, encoding="utf-8"))
        if raw.get("object") != "list":
            sys.exit(f"{c['name']}: unexpected response {raw.get('object')}")
        rulings = sorted(
            (
                {"date": r["published_at"], "source": r["source"], "text": r["comment"]}
                for r in raw["data"]
                if r.get("oracle_id", c["oracle_id"]) == c["oracle_id"]
            ),
            key=lambda r: (r["date"], r["text"]),
        )
        out.append({"name": c["name"], "oracle_id": c["oracle_id"], "rulings": rulings})
    if len(out) != len(cube):
        sys.exit("card count mismatch")
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        json.dump(out, f, indent=1, ensure_ascii=False)
        f.write("\n")
    n = sum(len(c["rulings"]) for c in out)
    print(f"{len(out)} cards, {n} rulings ({fetched} fetched, {len(out) - fetched} cached) -> {OUT}")


if __name__ == "__main__":
    main()
