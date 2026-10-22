"""Step 1: download the 17lands "Cube - Powered" public dataset.

Downloads draft_data and game_data for both PremierDraft and TradDraft, plus
cards.csv, from the public (anonymous, CC BY 4.0) 17lands S3 bucket. Writes a
manifest with source URLs, download timestamp and sha256 so the M1 doc can
cite exact provenance.

Rerun: just run this script again; it overwrites the raw files and manifest
with a fresh download date. 17lands updates these files periodically (the
Cube - Powered PremierDraft file covers 2025-10-30 to 2025-11-19 as of the
2026-10-21 pull; check 17lands.com/public_datasets for newer date ranges).
"""
from __future__ import annotations

import datetime as dt
import hashlib
import sys

import requests

sys.path.insert(0, str(__file__).rsplit("\\", 1)[0])
from common import RAW_17LANDS, dump_json, ensure_dirs  # noqa: E402

BASE = "https://17lands-public.s3.amazonaws.com/analysis_data"
FORMATS = ["PremierDraft", "TradDraft"]
TABLES = ["draft_data", "game_data"]


def sha256_of(path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def download(url: str, dest) -> None:
    print(f"GET {url}")
    with requests.get(url, stream=True, timeout=120) as r:
        r.raise_for_status()
        with open(dest, "wb") as f:
            for chunk in r.iter_content(1 << 20):
                f.write(chunk)
    print(f"  -> {dest} ({dest.stat().st_size / 1e6:.1f} MB)")


def main() -> None:
    ensure_dirs()
    manifest = {"downloaded_at": dt.datetime.now(dt.timezone.utc).isoformat(), "files": []}

    for table in TABLES:
        for fmt in FORMATS:
            fname = f"{table}_public.Cube_-_Powered.{fmt}.csv.gz"
            url = f"{BASE}/{table}/{fname}"
            dest = RAW_17LANDS / fname
            download(url, dest)
            manifest["files"].append(
                {"url": url, "path": str(dest), "bytes": dest.stat().st_size, "sha256": sha256_of(dest)}
            )

    cards_url = f"{BASE}/cards/cards.csv"
    cards_dest = RAW_17LANDS / "cards.csv"
    download(cards_url, cards_dest)
    manifest["files"].append(
        {"url": cards_url, "path": str(cards_dest), "bytes": cards_dest.stat().st_size, "sha256": sha256_of(cards_dest)}
    )

    dump_json(manifest, RAW_17LANDS / "manifest.json")
    print("Wrote manifest:", RAW_17LANDS / "manifest.json")


if __name__ == "__main__":
    main()
