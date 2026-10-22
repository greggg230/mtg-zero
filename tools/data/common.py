"""Shared paths and small helpers for the M1 draft-data pipeline.

All scripts in tools/data/ are meant to be run with the project venv:
    C:\\Users\\Gregg Keithley\\mtg-zero\\.venv\\Scripts\\python.exe tools\\data\\<script>.py

Large data lives on E: (never committed). Small, committed outputs live
under mtg-zero/data/.
"""
from __future__ import annotations

import json
import os
from pathlib import Path

REPO_ROOT = Path(r"C:\Users\Gregg Keithley\mtg-zero")
COMMITTED_DATA = REPO_ROOT / "data"

E_ROOT = Path(r"E:\mtg-zero\data")
RAW = E_ROOT / "raw"
RAW_17LANDS = RAW / "17lands"
RAW_CUBECOBRA = RAW / "cubecobra"
LANDS_DIR = E_ROOT / "17lands"
CUBECOBRA_DIR = E_ROOT / "cubecobra"
CARDS_DIR = E_ROOT / "cards"
SPLITS_DIR = E_ROOT / "splits"

POWERLSV_CUBE_ID = "786d3a10-43bd-438e-acf4-bb150b881254"

MTG_BOT_CUBE_JSON = Path(
    r"C:\Users\Gregg Keithley\Documents\Claude\Projects\mtg-bot\data\cube_power_lsv.json"
)


def ensure_dirs() -> None:
    for d in (RAW_17LANDS, RAW_CUBECOBRA, LANDS_DIR, CUBECOBRA_DIR, CARDS_DIR, SPLITS_DIR, COMMITTED_DATA):
        d.mkdir(parents=True, exist_ok=True)


def load_json(path: Path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def dump_json(obj, path: Path, indent: int = 2) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f, indent=indent, ensure_ascii=False)


def free_gb(drive: str = "E:\\") -> float:
    import shutil

    usage = shutil.disk_usage(drive)
    return usage.free / (1024 ** 3)


def check_disk_or_die(min_gb: float = 50.0, drive: str = "E:\\") -> None:
    g = free_gb(drive)
    if g < min_gb:
        raise SystemExit(
            f"STOP: {drive} has only {g:.1f} GB free (< {min_gb} GB floor). "
            "Not proceeding; report this to the user."
        )
    print(f"[disk] {drive} free: {g:.1f} GB (floor {min_gb} GB)")
