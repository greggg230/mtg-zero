"""The card-IR pipeline: Oracle text -> card IR -> card_ir's Rust types (docs/ENGINE.md 169, docs/card_ir.md).

The parser (``ir/mtgz_ir``, moved from mtg-bot) reads the pools' Scryfall dumps in ``ir/data`` and
encodes every parsed card as the card IR, with one schema over the three pools. This tool writes the
files mtg-zero checks in, all from the current inputs:

    data/cards/powerlsv.ir.json     the PowerLSV cube (pool ``cube``)
    data/cards/legacy.ir.json       constructed play's legacy cards (pool ``legacy``)
    data/cards/eoe.ir.json          the Edge of Eternities sealed cards (pool ``eoe``, filtered)
    data/cards/powerlsv.schema.json the schema the three were encoded with
    card_ir/src/generated.rs        the Rust types generated from that schema

Commands (stdlib Python 3.11+, from the repo root; no Rust build needed):

    python tools/card_ir.py export      # regenerate the five files in place
    python tools/card_ir.py check       # regenerate into a temp dir: identical to the checked-in files?
    python tools/card_ir.py roundtrip   # every printing decodes back to an equal object

``check`` compares byte for byte (a checkout whose git turns LF into CRLF is read back as LF: the
generated files are LF) and prints each file's sha256; it exits 1 on any difference. The test
``tests/ir/test_pipeline.py`` runs it.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import sys
import tempfile
from contextlib import redirect_stdout
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
IR_DIR = ROOT / "ir"
DATA = IR_DIR / "data"
CARDS = ROOT / "data" / "cards"
GENERATED = ROOT / "card_ir" / "src" / "generated.rs"
POOLS = ("cube", "legacy", "eoe")
SCHEMA_NAME = "powerlsv.schema.json"
IR_NAMES = ("powerlsv.ir.json", "legacy.ir.json", "eoe.ir.json")

for p in (IR_DIR, ROOT / "tools" / "codegen"):
    if str(p) not in sys.path:
        sys.path.insert(0, str(p))


def build(out: Path) -> dict[str, bytes]:
    """Export the three pools into ``out`` and return every output file's bytes, keyed by its
    path relative to the repo root (``data/cards/...``, ``card_ir/src/generated.rs``)."""
    from mtgz_ir import export
    import filter_eoe_ir
    import gen_card_ir

    with redirect_stdout(io.StringIO()):
        summary = export.export_ir(list(POOLS), out)
    eoe = out / "eoe.ir.json"
    text, report = filter_eoe_ir.filter_text(eoe.read_text(encoding="utf-8"), DATA)
    eoe.write_bytes(text.encode("utf-8"))
    code, sha = gen_card_ir.generate((out / SCHEMA_NAME).read_text(encoding="utf-8"), SCHEMA_NAME)
    if sha != summary["schema_sha256"]:
        raise SystemExit(f"generated.rs's schema hash {sha} != the export's {summary['schema_sha256']}")
    files = {f"data/cards/{n}": (out / n).read_bytes() for n in (*IR_NAMES, SCHEMA_NAME)}
    files["card_ir/src/generated.rs"] = code.encode("utf-8")
    print(f"export: pools {', '.join(POOLS)}; schema {sha[:12]}; {report}")
    return files


def checked_in(rel: str) -> bytes | None:
    p = ROOT / rel
    return p.read_bytes().replace(b"\r\n", b"\n") if p.exists() else None


def cmd_export(_args) -> int:
    with tempfile.TemporaryDirectory(prefix="card-ir-") as tmp:
        files = build(Path(tmp))
    for rel, data in files.items():
        if checked_in(rel) == data:
            print(f"unchanged {rel}")
            continue
        (ROOT / rel).write_bytes(data)
        print(f"wrote     {rel} ({len(data)} bytes)")
    return 0


def check() -> list[str]:
    """Regenerate and compare; the differences (empty when identical)."""
    with tempfile.TemporaryDirectory(prefix="card-ir-") as tmp:
        files = build(Path(tmp))
    bad = []
    for rel, data in files.items():
        have = checked_in(rel)
        same = have == data
        print(f"{'identical' if same else 'DIFFERS  '} {rel} sha256 {hashlib.sha256(data).hexdigest()[:16]} "
              f"({len(data)} bytes)")
        if not same:
            bad.append(rel)
    return bad


def cmd_check(_args) -> int:
    bad = check()
    if bad:
        print(f"card IR check FAILED: {len(bad)} file(s) differ from a regeneration: {bad} "
              "(rerun `python tools/card_ir.py export` and commit, or fix the input)", file=sys.stderr)
        return 1
    print("card IR check PASSED: the checked-in files are byte for byte the regeneration")
    return 0


def cmd_roundtrip(_args) -> int:
    from mtgz_ir import export

    problems = export.roundtrip(list(POOLS))
    for p in problems:
        print(p, file=sys.stderr)
    return 1 if problems else 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("export", help="regenerate the IR files, the schema and generated.rs in place")
    sub.add_parser("check", help="regenerate into a temp dir and compare byte for byte")
    sub.add_parser("roundtrip", help="decode(encode(printing)) == printing for every pool")
    args = ap.parse_args(argv)
    return {"export": cmd_export, "check": cmd_check, "roundtrip": cmd_roundtrip}[args.cmd](args)


if __name__ == "__main__":
    raise SystemExit(main())
