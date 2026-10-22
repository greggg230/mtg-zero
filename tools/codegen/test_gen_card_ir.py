"""``card_ir/src/generated.rs`` is up to date with the checked-in schema.

Run: ``python -m pytest tools/codegen`` (any Python >= 3.10, no deps
beyond pytest).
"""

from pathlib import Path

import gen_card_ir

ROOT = Path(__file__).resolve().parents[2]


def test_generated_rust_is_current():
    rc = gen_card_ir.main([
        "--schema", str(ROOT / "data/cards/powerlsv.schema.json"),
        "--out", str(ROOT / "card_ir/src/generated.rs"),
        "--check",
    ])
    assert rc == 0, "rerun tools/codegen/gen_card_ir.py (see docs/card_ir.md)"


def test_camel_and_field_names():
    assert gen_card_ir.camel("cube_walkers.IsPermanent") == "CubeWalkersIsPermanent"
    assert gen_card_ir.camel("BEGINNING_OF_UPKEEP") == "BeginningOfUpkeep"
    assert gen_card_ir.camel("_Controller") == "Controller"
    assert gen_card_ir.camel("SELF") == "Self_"
    assert gen_card_ir.field_ident("type") == "r#type"
    assert gen_card_ir.field_ident("amount") == "amount"
