"""The card-IR pipeline in mtg-zero (docs/ENGINE.md 169): regenerating at the current inputs reproduces
the checked-in IR files, the schema and card_ir/src/generated.rs byte for byte."""
import importlib.util
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def _tool():
    spec = importlib.util.spec_from_file_location("card_ir_tool", os.path.join(ROOT, "tools", "card_ir.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_regeneration_reproduces_the_checked_in_files_byte_for_byte(capsys):
    tool = _tool()
    assert tool.check() == [], capsys.readouterr().out
    out = capsys.readouterr().out
    for rel in ("data/cards/powerlsv.ir.json", "data/cards/legacy.ir.json", "data/cards/eoe.ir.json",
                "data/cards/powerlsv.schema.json", "card_ir/src/generated.rs"):
        assert f"identical {rel}" in out


def test_the_check_names_a_file_that_differs(monkeypatch, capsys):
    tool = _tool()
    real = tool.checked_in
    monkeypatch.setattr(tool, "checked_in",
                        lambda rel: real(rel) + b" " if rel == "data/cards/legacy.ir.json" else real(rel))
    assert tool.check() == ["data/cards/legacy.ir.json"]
    assert tool.main(["check"]) == 1


def test_the_schema_keeps_the_first_exporters_module_labels():
    from mtgz_ir.cards import ir

    assert ir.schema_module("mtgz_ir.effects.base") == "mtg_bot.effects.base"
    assert ir.schema_module("mtgz_ir.keyword_vocab") == "mtg_bot.keyword_vocab"
    assert ir.schema_module("test_card_ir") == "test_card_ir"      # a class outside the package: as it is
