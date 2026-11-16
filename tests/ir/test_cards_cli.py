"""``mtgz_ir.export`` (card IR export / round-trip check)."""

from __future__ import annotations

import json

import pytest

from mtgz_ir import export as cards_cli


def test_export_ir_writes_document_and_schema(tmp_path, capsys):
    rc = cards_cli.main(["export-ir", "--pool", "cube", "--out", str(tmp_path)])
    assert rc == 0
    summary = json.loads(capsys.readouterr().out)
    assert summary["cards"] == 546 and summary["pool"] == "cube"
    doc = json.loads((tmp_path / "powerlsv.ir.json").read_text(encoding="utf-8"))
    schema = json.loads((tmp_path / "powerlsv.schema.json").read_text(encoding="utf-8"))
    assert doc["schema_sha256"] == summary["schema_sha256"]
    assert schema["format"] == "mtg-bot-card-ir-schema"
    assert 0 < summary["ir_gzip_bytes"] < summary["ir_bytes"]


def test_export_ir_name_override(tmp_path):
    # mtg-zero: mtg-bot's CLI dispatcher (``mtg_bot cards ...``, ``python -m mtg_bot``) stayed there;
    # the export is ``mtgz_ir.export`` and tools/card_ir.py (docs/ENGINE.md 169).
    cards_cli.main(["export-ir", "--pool", "cube", "--out", str(tmp_path), "--name", "x"])
    assert (tmp_path / "x.ir.json").exists() and (tmp_path / "x.schema.json").exists()


def test_roundtrip_reports_ok(capsys):
    assert cards_cli.main(["roundtrip", "--pool", "cube"]) == 0
    assert "cube: 546 cards" in capsys.readouterr().out


def test_roundtrip_reports_mismatches(monkeypatch):
    from mtgz_ir.cards import ir

    monkeypatch.setattr(ir, "strict_equal", lambda a, b: False)
    problems = cards_cli.roundtrip(["cube"])
    assert problems and "differ after decode" in problems[0]
    assert cards_cli.main(["roundtrip", "--pool", "cube"]) == 1


def test_unknown_action_is_an_argparse_error():
    with pytest.raises(SystemExit):
        cards_cli.main(["frobnicate"])
