"""``mtgz_ir cards`` -- card-pool data exports.

Actions::

    mtgz_ir cards export-ir --pool cube --out DIR [--name powerlsv]
    mtgz_ir cards export-ir --pool cube legacy --out DIR
    mtgz_ir cards roundtrip --pool cube roto eoe all

``export-ir`` writes the pool's card IR (``<name>.ir.json``) and the
schema it was encoded with (``<name>.schema.json``); see
``docs/CARD_IR.md``. Several pools are encoded with ONE schema, built over
all their printings (so one generated consumer, mtg-zero's ``card_ir``
crate, reads every document): each pool's ``<stem>.ir.json``, and the
schema once, as ``<first stem>.schema.json``. ``roundtrip`` checks that every printing of each
pool decodes back to an identical object (types included) and that
re-encoding is a fixpoint; it exits 1 on any difference.
"""

from __future__ import annotations

import argparse
import gzip
import json
import sys
from pathlib import Path
from typing import Optional, Sequence

# File stem per pool when ``--name`` is not given.
POOL_FILE_NAMES = {"cube": "powerlsv", "roto": "vintage_roto"}


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="mtgz_ir cards",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = parser.add_subparsers(dest="action", required=True)
    exp = sub.add_parser("export-ir", help="Write a pool's card IR + schema.")
    exp.add_argument(
        "--pool", nargs="+", default=["cube"],
        help="cube | roto | eoe | legacy | all (default cube); several pools share one schema",
    )
    exp.add_argument("--out", type=Path, default=Path("."), help="Output directory.")
    exp.add_argument(
        "--name", default=None,
        help="File stem (default: powerlsv for cube, vintage_roto for roto, else the pool); one pool only.",
    )
    rt = sub.add_parser("roundtrip", help="Check decode(encode(p)) == p for pools.")
    rt.add_argument("--pool", nargs="+", default=["cube", "roto", "eoe", "all"])
    return parser


def export_ir(pool: str | Sequence[str], out: Path, name: str | None = None) -> dict:
    """Write ``<name>.ir.json`` and ``<name>.schema.json`` into ``out``;
    returns a summary. Several pools: one schema over all their printings,
    each pool's document, the schema written once under the first stem."""
    from mtgz_ir.cards import ir

    pools = [pool] if isinstance(pool, str) else list(pool)
    if not pools or len(set(pools)) != len(pools):
        raise ValueError(f"export-ir: pools must be distinct and non-empty: {pools}")
    if name is not None and len(pools) > 1:
        raise ValueError("export-ir: --name names one pool's files")
    printings = {p: ir.pool_printings(p) for p in pools}
    schema = None
    if len(pools) > 1:
        both = [pr[n] for p in pools for pr in [printings[p]] for n in sorted(pr)]
        schema = ir.build_schema(both)
    out.mkdir(parents=True, exist_ok=True)
    docs = []
    for i, p in enumerate(pools):
        doc, sch = ir.build_document(p, printings[p], schema)
        stem = name or POOL_FILE_NAMES.get(p, p)
        ir_path = out / f"{stem}.ir.json"
        text = ir.dumps(doc)
        ir_path.write_text(text, encoding="utf-8", newline="\n")
        data = text.encode("utf-8")
        entry = {
            "pool": p,
            "cards": len(doc["cards"]),
            "ir_bytes": len(data),
            "ir_gzip_bytes": len(gzip.compress(data, mtime=0)),
            "ir_path": str(ir_path),
        }
        if i == 0:
            schema_path = out / f"{stem}.schema.json"
            schema_path.write_text(
                json.dumps(sch.to_json(), sort_keys=True, indent=1, ensure_ascii=False) + "\n",
                encoding="utf-8", newline="\n",
            )
            summary = {"classes": len(sch.classes), "schema_sha256": doc["schema_sha256"],
                       "schema_path": str(schema_path)}
        docs.append(entry)
    if len(pools) == 1:
        return {**docs[0], **summary}
    return {**summary, "documents": docs}


def roundtrip(pools: Sequence[str]) -> list[str]:
    """Problems found (empty when every pool round-trips)."""
    from mtgz_ir.cards import ir

    problems: list[str] = []
    for pool in pools:
        printings = ir.pool_printings(pool)
        doc, schema = ir.build_document(pool, printings)
        text = ir.dumps(doc)
        back = ir.decode_document(json.loads(text), schema)
        bad = sorted(n for n in printings if not ir.strict_equal(printings[n], back[n]))
        if bad:
            problems.append(f"{pool}: {len(bad)} printings differ after decode: {bad[:5]}")
        again, _ = ir.build_document(pool, back, schema)
        if ir.dumps(again) != text:
            problems.append(f"{pool}: re-encoding the decoded printings is not a fixpoint")
        print(f"{pool}: {len(printings)} cards, {len(schema.classes)} classes, "
              f"{len(text)} bytes, {'ok' if not bad else 'MISMATCH'}")
    return problems


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = _build_parser().parse_args(list(argv) if argv is not None else None)
    if args.action == "export-ir":
        summary = export_ir(args.pool, args.out, args.name)
        print(json.dumps(summary, indent=1))
        return 0
    problems = roundtrip(args.pool)
    for p in problems:
        print(p, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
