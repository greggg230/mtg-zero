"""Card IR v1 (``mtgz_ir.cards.ir``): lossless round trip over every pool,
schema shape, and the hard errors."""

from __future__ import annotations

import dataclasses
import enum
import json
import math

import pytest

from mtgz_ir.cards import ir
from mtgz_ir.cards.base import make_creature
from mtgz_ir.effects.base import StaticAbility, StaticScope, TriggeredAbility
from mtgz_ir.effects.generic.draw_mill import DrawCards

POOLS = ("cube", "roto", "eoe", "all", "legacy")


@pytest.fixture(scope="module", params=POOLS)
def pool_ir(request):
    pool = request.param
    printings = ir.pool_printings(pool)
    doc, schema = ir.build_document(pool, printings)
    return pool, printings, doc, schema


def test_every_printing_round_trips(pool_ir):
    pool, printings, doc, schema = pool_ir
    text = ir.dumps(doc)
    back = ir.decode_document(json.loads(text), schema)
    assert set(back) == set(printings)
    bad = [n for n in printings if not ir.strict_equal(printings[n], back[n])]
    assert bad == []
    # dataclass equality too, and re-encoding is a fixpoint.
    assert all(printings[n] == back[n] for n in printings)
    again, _ = ir.build_document(pool, back, schema)
    assert ir.dumps(again) == text


def test_document_header_and_card_keys(pool_ir):
    pool, printings, doc, schema = pool_ir
    assert doc["format"] == ir.IR_FORMAT
    assert doc["ir_version"] == ir.IR_VERSION == 1
    assert doc["pool"] == pool
    assert doc["schema_sha256"] == schema.sha256()
    names = [c["name"] for c in doc["cards"]]
    assert names == sorted(printings)
    for c in doc["cards"]:
        assert c["printing"][ir.TAG] == "CardPrinting"
        if c.get("custom"):
            assert c["oracle_id"] is None
        else:
            assert isinstance(c["oracle_id"], str) and len(c["oracle_id"]) == 36


def test_schema_annotations_agree_with_every_value(pool_ir):
    _pool, _printings, _doc, schema = pool_ir
    sj = schema.to_json()
    mismatches = [
        (tag, f["name"], f["annotation_mismatch"])
        for tag, c in sj["classes"].items()
        for f in c["fields"]
        if "annotation_mismatch" in f
    ]
    assert mismatches == []


def test_schema_shape():
    printings = ir.pool_printings("cube")
    schema = ir.build_schema(printings[n] for n in sorted(printings))
    sj = schema.to_json()
    assert sj["format"] == ir.SCHEMA_FORMAT and sj["ir_version"] == 1
    cats = sj["categories"]
    for cat in ("Effect", "ObjectPredicate", "StatePredicate", "TriggerPredicate",
                "Condition", "Formula", "PTFormula", "CostAtom", "LayerPayload",
                "EventTransform", "CardPrinting", "Keyword"):
        assert cats.get(cat), cat
    # Every class is in exactly one category.
    tags = [t for ts in cats.values() for t in ts]
    assert sorted(tags) == sorted(sj["classes"])
    draw = sj["classes"]["DrawCards"]
    assert draw["category"] == "Effect"
    # mtg-zero: the schema keeps the first exporter's module labels (ir.SCHEMA_MODULE_PACKAGE).
    assert draw["module"] == "mtg_bot.effects.generic.draw_mill"
    # Keyword slots are typed, enums carry their full vocabulary.
    printing_fields = {f["name"]: f for f in sj["classes"]["CardPrinting"]["fields"]}
    assert printing_fields["keywords"]["type"] == {"t": "set", "of": {"t": "keyword"}}
    assert printing_fields["aura_enchant_type"]["type"] == {"t": "enum", "enum": "EnchantKind"}
    assert {m["value"] for m in sj["enums"]["KeywordKind"]["members"]} >= {"flying", "annihilator"}
    # Ambiguous short names are module-qualified tags.
    assert "cube_walkers.IsPermanent" in sj["classes"]
    assert "value_predicates.IsPermanent" in sj["classes"]
    # TargetSpec.count_max may be infinite.
    spec = {f["name"]: f for f in sj["classes"]["TargetSpec"]["fields"]}
    assert spec["count_max"]["type"] == {"t": "count"}


def _schema_for(*printings):
    return ir.build_schema(printings)


def test_sparse_fields_and_special_values():
    p = make_creature("Bolt Bear", "{R}", 2, 2, keywords=("annihilator 2", "flying"))
    schema = _schema_for(p)
    enc = ir.Encoder(schema).encode_printing(p)
    # Only non-default fields are present.
    assert enc["power"] == 2 and "loyalty" not in enc
    # Keywords are structured objects, sorted.
    assert enc["keywords"] == [
        {"$t": "Keyword", "kind": "annihilator", "n": 2},
        {"$t": "Keyword", "kind": "flying"},
    ]
    back = ir.Decoder(schema).decode_printing(json.loads(json.dumps(enc)))
    assert ir.strict_equal(back, p)


def test_infinity_round_trips_through_targetspec():
    printings = ir.pool_printings("cube")
    card = printings["Candelabra of Tawnos"]
    schema = _schema_for(card)
    enc = ir.Encoder(schema).encode_printing(card)
    assert '"count_max":"inf"' in ir.dumps(enc)
    back = ir.Decoder(schema).decode_printing(json.loads(ir.dumps(enc)))
    assert ir.strict_equal(back, card)


def _nodes(j, tag):
    if isinstance(j, dict):
        if j.get(ir.TAG) == tag:
            yield j
        for v in j.values():
            yield from _nodes(v, tag)
    elif isinstance(j, list):
        for v in j:
            yield from _nodes(v, tag)


def test_derived_targets_are_recomputed_not_serialized():
    printings = ir.pool_printings("cube")
    card = printings["Boseiju, Who Endures"]  # ThenTargetControllerResolves
    schema = _schema_for(card)
    enc = ir.Encoder(schema).encode_printing(card)
    (node,) = list(_nodes(enc, "ThenTargetControllerResolves"))
    assert "targets" not in node
    back = ir.Decoder(schema).decode_printing(json.loads(ir.dumps(enc)))
    assert ir.strict_equal(back, card)  # includes the recomputed ``targets``
    assert ir.DERIVED_ATTRIBUTES == {"targets"}


def test_hard_errors():
    p = make_creature("Plain Bear", "{G}", 2, 2)
    schema = _schema_for(p)
    enc = ir.Encoder(schema)
    # An undeclared attribute.
    odd = dataclasses.replace(p)
    object.__setattr__(odd, "surprise", 1)
    with pytest.raises(ir.IRError, match="undeclared attributes"):
        enc.encode_printing(odd)
    # A mutable container where a tuple belongs.
    with pytest.raises(ir.IRError, match="mutable"):
        enc.encode_value([1], ("list", ("int",)), "x")
    # A class outside the schema.
    with pytest.raises(ir.IRError, match="not in the schema"):
        enc.encode_object(DrawCards(amount=2), "x")
    # A value of the wrong type for its slot.
    with pytest.raises(ir.IRError, match="is not a"):
        enc.encode_value("2", ("int",), "x")
    with pytest.raises(ir.IRError, match="unknown keyword"):
        enc.encode_value("flyin", ("keyword",), "x")
    # Non-string enums and NaN.
    class Num(enum.Enum):
        A = 1
    with pytest.raises(ir.IRError, match="non-string enum"):
        enc.encode_value(Num.A, ("int",), "x")
    with pytest.raises(ir.IRError, match="NaN"):
        enc.encode_value(float("nan"), ("float",), "x")
    # The legacy string trigger key is not part of IR v1.
    legacy = dataclasses.replace(p, triggered_abilities=(
        TriggeredAbility(trigger="etb", effect=DrawCards(amount=1)),
    ))
    with pytest.raises(ir.IRError, match="not part of IR v1"):
        ir.Encoder(_schema_for(legacy)).encode_printing(legacy)


def test_decoder_rejects_unknown_tags_fields_and_versions():
    p = make_creature("Plain Bear", "{G}", 2, 2, static_abilities=(
        StaticAbility(scope=StaticScope.SELF, power_bonus=1),
    ))
    schema = _schema_for(p)
    dec = ir.Decoder(schema)
    good = ir.Encoder(schema).encode_printing(p)
    with pytest.raises(ir.IRError, match="unknown class tag"):
        dec.decode_printing({**good, ir.TAG: "NoSuchClass"})
    with pytest.raises(ir.IRError, match="has no fields"):
        dec.decode_printing({**good, "no_such_field": 1})
    bad_scope = json.loads(json.dumps(good))
    bad_scope["static_abilities"][0]["scope"]["kind"] = "everyone"
    with pytest.raises(ir.IRError, match="is not a ScopeKind value"):
        dec.decode_printing(bad_scope)
    with pytest.raises(ir.IRError, match="missing required field"):
        dec.decode_printing({ir.TAG: "CardPrinting"})
    doc = {"format": ir.IR_FORMAT, "ir_version": 2, "schema_sha256": schema.sha256(), "cards": []}
    with pytest.raises(ir.IRError, match="unsupported ir_version"):
        ir.decode_document(doc, schema)
    with pytest.raises(ir.IRError, match="different schema"):
        ir.decode_document({**doc, "ir_version": 1, "schema_sha256": "0"}, schema)
    with pytest.raises(ir.IRError, match="not a card IR document"):
        ir.decode_document({**doc, "format": "x"}, schema)


def test_strict_equal_distinguishes_types():
    from mtgz_ir.effects.vocab import ZoneName

    assert ZoneName.HAND == "hand"
    assert not ir.strict_equal(ZoneName.HAND, "hand")
    assert not ir.strict_equal((1,), frozenset({1}))
    assert ir.strict_equal(frozenset({(1, 2), (3,)}), frozenset({(3,), (1, 2)}))
    assert not ir.strict_equal(frozenset({1}), frozenset({2}))


def test_type_joins():
    j = ir._join
    assert j(("int",), ir.NULL) == ("opt", ("int",))
    assert j(("ref", ("A",)), ("ref", ("B",))) == ("ref", ("A", "B"))
    assert j(("int",), ("count",)) == ("count",)
    assert j(("list", ("int",)), ("tuple", (("int",), ("int",)))) == ("list", ("int",))
    assert j(("int",), ("str",))[0] == "union"
    with pytest.raises(ir.IRError, match="ambiguous"):
        ir._check_unambiguous(("union", (("list", ("int",)), ("set", ("int",)))), "x")


# ---------------------------------------------------------------------------
# Synthetic classes: annotation handling and codec error paths
# ---------------------------------------------------------------------------

from dataclasses import dataclass, field  # noqa: E402
from typing import Annotated, ClassVar, Literal  # noqa: E402

from mtgz_ir.effects.vocab import ZoneName  # noqa: E402
from mtgz_ir.keyword_vocab import KeywordSpelling  # noqa: E402


@dataclass(frozen=True)
class _Leaf:
    n: int = 0


@dataclass(frozen=True)
class _Probe:
    flag: bool = False
    ratio: float = 0.5
    zone: ZoneName = ZoneName.HAND
    kw: KeywordSpelling = "flying"
    label: Literal["a", "b"] = "a"
    note: Annotated[str, "doc"] = ""
    pairs: tuple = ()              # bare tuple: completed from values
    tags: frozenset = frozenset()  # bare frozenset
    either: "int | str" = 0
    maybe: "_Leaf | None" = None
    leaf: object = field(default_factory=_Leaf)
    bad: int = 0                   # annotation lies: holds a str below
    unresolved: "NoSuchType" = 0  # noqa: F821
    cv: ClassVar[int] = 3


def test_annotation_completion_and_mismatch_reporting():
    probes = [
        _Probe(pairs=((1, "x"), (2, "y")), tags=frozenset({"p", "q"}), either="s",
               maybe=_Leaf(2), bad="oops"),
        _Probe(),
    ]
    schema = ir.build_schema(probes)
    fields = schema.by_class[_Probe].field_map()
    assert fields["flag"].type == ("bool",)
    assert fields["zone"].type == ("enum", "ZoneName")
    assert fields["kw"].type == ("keyword",)
    assert fields["label"].type == ("str",)
    assert fields["note"].type == ("str",)
    assert fields["pairs"].type == ("list", ("tuple", (("int",), ("str",))))
    assert fields["pairs"].type_source == "annotation+observed"
    assert fields["tags"].type == ("set", ("str",))
    assert fields["either"].type[0] == "union"
    assert fields["maybe"].type == ("opt", ("ref", ("_Leaf",)))
    assert fields["leaf"].type == ("ref", ("_Leaf",))
    assert fields["unresolved"].type == ("int",)
    assert "cv" not in fields
    assert fields["bad"].annotation_mismatch and fields["bad"].type == ("union", (("int",), ("str",)))
    sj = schema.to_json()
    assert any("annotation_mismatch" in f for f in sj["classes"]["_Probe"]["fields"])
    enc = ir.Encoder(schema)
    dec = ir.Decoder(schema)
    for probe in probes:
        j = json.loads(json.dumps(enc.encode_object(probe, "p")))
        assert ir.strict_equal(dec.decode_object(j, "p"), probe)


def test_untypable_and_ambiguous_slots_are_errors():
    @dataclass(frozen=True)
    class Mixed:
        v: object = None

    with pytest.raises(ir.IRError, match="ambiguous"):
        ir.build_schema([Mixed(v=(1,)), Mixed(v=frozenset({1}))])


class _Slotted:
    __slots__ = ("x",)

    def __init__(self):
        self.x = 1


def test_opaque_and_callable_values_are_errors():
    with pytest.raises(ir.IRError, match="opaque"):
        ir.object_fields(_Slotted())
    with pytest.raises(ir.IRError, match="callable"):
        ir._check_value(len, "x")
    with pytest.raises(ir.IRError, match="callable"):
        ir._check_value(_Leaf, "x")


def test_decoder_type_errors():
    schema = ir.build_schema([_Probe(either="s", maybe=_Leaf(2))])
    dec = ir.Decoder(schema)
    for j, t, msg in [
        (1, ("bool",), "expected bool"),
        (True, ("int",), "expected int"),
        ("1", ("float",), "expected number"),
        ([], ("count",), "expected number or 'inf'"),
        (3, ("str",), "expected string"),
        ("nowhere", ("enum", "ZoneName"), "is not a ZoneName value"),
        ("x", ("enum", "NoSuchEnum"), "unknown enum"),
        ({}, ("list", ("int",)), "expected array"),
        ([1], ("tuple", (("int",), ("int",))), "expected 2-array"),
        (None, ("union", (("int",), ("str",))), "matches none"),
        ([1], ("never",), "cannot decode"),
        (5, ("ref", ("_Leaf",)), "expected an object"),
    ]:
        with pytest.raises(ir.IRError, match=msg):
            dec.decode_value(j, t, "x")
    assert dec.decode_value("-inf", ("count",), "x") == -math.inf
    assert dec.decode_value(None, ("opt", ("int",)), "x") is None
    leaf = dec.decode_value({"$t": "_Leaf", "n": 4}, ("ref", ("_Leaf",)), "x")
    assert leaf == _Leaf(4)
    with pytest.raises(ir.IRError, match="is not one of"):
        dec.decode_value({"$t": "_Leaf"}, ("ref", ("_Probe",)), "x")


def test_encoder_type_errors():
    schema = ir.build_schema([_Probe(either="s", maybe=_Leaf(2))])
    enc = ir.Encoder(schema)
    assert enc.encode_value(-math.inf, ("count",), "x") == "-inf"
    assert enc.encode_value(3, ("count",), "x") == 3
    with pytest.raises(ir.IRError, match="matches none"):
        enc.encode_value(1.5, ("union", (("int",), ("str",))), "x")
    with pytest.raises(ir.IRError, match="no IR type"):
        enc.encode_value(None, ("null",), "x")
    with pytest.raises(ir.IRError, match="is not a"):
        enc.encode_value("hand", ("enum", "ZoneName"), "x")  # not coerced
    with pytest.raises(ir.IRError, match="is not a"):
        enc.encode_value(ZoneName.HAND, ("str",), "x")
    odd = _Leaf(1)
    object.__setattr__(odd, "n2", 3)
    with pytest.raises(ir.IRError, match="undeclared"):
        enc.encode_object(odd, "x")


def test_ann_to_type_edge_cases():
    resolve = lambda raw: (_ for _ in ()).throw(ir._Unresolved(raw))  # noqa: E731
    assert ir._ann_to_type(None, resolve) == ir.NULL
    assert ir._ann_to_type(tuple, resolve) == ("list", None)
    assert ir._ann_to_type(frozenset, resolve) == ("set", None)
    assert ir._ann_to_type(dict, resolve) is None
    assert ir._ann_to_type(dict[str, int], resolve) is None
    assert ir._ann_to_type(tuple[int, ...], resolve) == ("list", ("int",))
    assert ir._ann_to_type(Literal[1, 2], resolve) is None
    assert ir._ann_to_type("Whatever", resolve) is None
    assert ir._ann_to_type(object | None, resolve) == ("opt", None)
    assert ir._fill(None, ir.NULL) == ("opt", ir.NEVER)
    assert ir._fill(("list", None), ("list", ("int",))) == ("list", ("int",))
    assert ir._fill(("list", None), None) == ("list", ir.NEVER)
    assert ir._fill(("float",), ("count",)) == ("count",)
    assert ir._prune_refs(("union", (("int",), ("ref", ("Gone",)))), {"X"}) == ("int",)
    with pytest.raises(ir.IRError, match="nested optional"):
        ir._check_unambiguous(("opt", ("opt", ("int",))), "x")
