"""``mtgz_ir.cards.review_render`` + ``scripts/render_cube_card_review.py``."""

# mtg-zero (docs/ENGINE.md 170): 1 of mtg-bot's tests in this file stayed in mtg-bot: they run mtg-bot scripts that stayed there:
# test_script_end_to_end

from __future__ import annotations

import dataclasses
import enum
import importlib.util
from dataclasses import dataclass, field
from pathlib import Path

import pytest

from mtgz_ir.cards import review_render as rr
from mtgz_ir.cards.base import ManaCost, make_creature
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.pools import load_pool_cards

REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "render_cube_card_review.py"


@pytest.fixture(scope="module")
def printings():
    return OracleParserCardSource(pool="cube").get_printings()


@pytest.fixture(scope="module")
def raw_cards():
    return {c["name"]: c for c in load_pool_cards("cube")}


def _summary(printings, name):
    s = rr.summarize_printing(printings[name])
    return s, s.to_text()


# ------------------------------------------------------------- naming


@pytest.mark.parametrize("cls, expected", [
    ("DealDamageToTargetCreature", "Deal damage to target creature"),
    ("RevealTopCardToHandLoseLifeEqualMV", "Reveal top card to hand lose life equal MV"),
    ("PumpSelfXPowerUEOT", "Pump self X power UEOT"),
    ("TractorBeamETBEffect", "Tractor beam ETB effect"),
    ("_Controller", "Controller"),
    ("Put2Counters", "Put 2 counters"),
    ("___", "___"),
])
def test_humanize_class_name(cls, expected):
    assert rr.humanize_class_name(cls) == expected


def test_humanize_field_name():
    assert rr.humanize_field_name("_power_bonus") == "power bonus"


def test_format_mana_cost_every_symbol_family():
    cost = ManaCost(
        generic=2, colored=(("G", 1), ("W", 2)), x_pips=1,
        hybrid=(("R", "W"),), phyrexian=("U",),
    )
    assert rr.format_mana_cost(cost) == "{X}{2}{R/W}{U/P}{W}{W}{G}"
    assert rr.format_mana_cost(ManaCost()) == "{0}"


# ------------------------------------------------ real cube printings


def test_keyword_only_creature_synthetic():
    p = make_creature("Test Bird", "{1}{U}", 1, 1, keywords=("flying", "haste"))
    s = rr.summarize_printing(p)
    assert [x.title for x in s.sections] == ["Characteristics", "Keywords"]
    assert "keywords: flying, haste" in s.to_text()
    assert s.has_rules_content


def test_keyword_creature_real(printings):
    s, text = _summary(printings, "Baleful Strix")
    assert "keywords: deathtouch, flying" in text
    assert "power/toughness: 1/1" in text
    assert "colors: UB" in text
    assert "Draw cards (amount: 1)" in text


def test_triggered_creature(printings):
    s, text = _summary(printings, "Dark Confidant")
    trig = s.section("Triggered abilities")
    assert trig is not None
    body = "\n".join(trig.lines)
    assert "event: beginning of upkeep" in body
    # a truthy default (Controlled.by = YOU) is shown, not hidden
    assert "predicate: Controlled (by: you)" in body
    # the effect renders last, after the trigger condition
    assert body.rstrip().endswith("effect: Reveal top card to hand lose life equal MV")
    assert "mana cost: {1}{B} (MV 2)" in text


def test_planeswalker_loyalty_abilities(printings):
    s, text = _summary(printings, "Jace, the Mind Sculptor")
    assert s.section("Activated abilities") is None
    loyalty = "\n".join(s.section("Loyalty abilities").lines)
    for label in ("[+2]", "[0]", "[-1]", "[-12]"):
        assert f"- {label} Activated ability" in loyalty
    assert "Draw cards (amount: 3)" in loyalty
    assert "starting loyalty: 3" in text


def test_activated_artifact(printings):
    s, text = _summary(printings, "Grim Monolith")
    assert "Add mana to pool (produces: (C, 3))" in text
    assert "cost: Activation cost (tap self)" in text
    assert "cost: Activation cost (mana: {4})" in text
    assert "mana str" not in text  # duplicate of the ManaCost, dropped
    assert "doesnt untap during untap step" in text
    assert "is mana ability" in text


def test_transform_dfc(printings):
    s, text = _summary(printings, "Jace, Vryn's Prodigy // Jace, Telepath Unbound")
    assert [label for label, _ in s.faces] == ["transform back face"]
    back = s.faces[0][1]
    assert back.name == "Jace, Telepath Unbound"
    assert back.section("Loyalty abilities") is not None
    assert "transform back face: Jace, Telepath Unbound" in text
    assert "Cards in your graveyard at least (n: 5)" in text


def test_split_card(printings):
    s, text = _summary(printings, "Life // Death")
    assert [label for label, _ in s.faces] == ["split faces 1", "split faces 2"]
    assert [f.name for _, f in s.faces] == ["Life", "Death"]
    assert "Your permanents become creatures UEOT" in text


def test_saga_chapters_and_tokens(printings):
    s, text = _summary(printings, "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki")
    for n in (1, 2, 3):
        assert f"Chapter reached (n: {n})" in text
    assert "chapter count: 3" in text
    tokens = s.section("Tokens created")
    assert tokens is not None
    assert any("Goblin Shaman" in t for t in s.tokens)
    assert any(t.startswith("Treasure") for t in s.tokens)
    assert "token printing: Printing: Goblin Shaman" in text
    assert s.faces[0][0] == "transform back face"


def test_alt_cost_and_aura(printings):
    _, text = _summary(printings, "Lingering Souls")
    assert "flashback cost: {1}{B}" in text
    assert "flashback cost str" not in text
    _, text = _summary(printings, "Animate Dead")
    assert "aura enchant type: creature_card_in_graveyard" in text


def test_every_cube_card_has_a_summary(printings, raw_cards):
    empty = []
    for name, p in printings.items():
        s = rr.summarize_printing(p)
        assert s.section("Characteristics") is not None
        if not s.has_rules_content:
            empty.append(name)
    # Basics carry only characteristics (+ produces); Cogwork Librarian's
    # only text is draft-matters. Anything else here means the walker
    # dropped a field family.
    assert set(empty) <= {
        "Plains", "Island", "Swamp", "Mountain", "Forest", "Cogwork Librarian",
    }


def test_modeled_colors_devoid_and_face_colors():
    p = make_creature("Grey", "{2}{R}", 2, 2, keywords=("devoid",))
    assert rr.modeled_colors(p) == frozenset()
    back = dataclasses.replace(p, keywords=frozenset(), face_colors=frozenset({"G"}))
    assert rr.modeled_colors(back) == frozenset({"G"})
    painted = dataclasses.replace(back, color_indicator=frozenset({"U"}))
    text = rr.summarize_printing(painted).to_text()
    assert "colors: UG" in text and "color indicator: U" in text


def test_printed_cost_mismatch_is_shown():
    p = make_creature("Odd", "{1}{G}", 1, 1)
    p = dataclasses.replace(p, mana_cost_str="{2}{G}")
    assert "printed string: {2}{G}" in rr.summarize_printing(p).to_text()


def test_dynamic_pt_and_produces():
    p = make_creature("Goyf", "{1}{G}", 0, 1)
    from mtgz_ir.effects.pt_formulas import LandsYouControlPT, PTFormula

    text = rr.summarize_printing(dataclasses.replace(p, dynamic_pt_formula=LandsYouControlPT())).to_text()
    assert "dynamic P/T:" in text and "Lands you control PT" in text

    @dataclass(frozen=True)
    class Formula(PTFormula):
        base: int
        parts: tuple = ()

    obj = Formula(1, (Formula(2),))
    text = rr.summarize_printing(dataclasses.replace(p, dynamic_pt_formula=obj)).to_text()
    assert "dynamic P/T:" in text and "Formula" in text
    land = dataclasses.replace(p, produces=frozenset({"G", "W"}))
    assert "produces: WG" in rr.summarize_printing(land).to_text()


def test_non_printing_face_object_renders():
    @dataclass(frozen=True)
    class Half:
        name: str
        cost: int = 0

    p = make_creature("Room-ish", "{1}", 1, 1)
    p = dataclasses.replace(p, room_halves=(Half("Door A", 2),))
    s = rr.summarize_printing(p)
    label, face = s.faces[0]
    assert label == "room halves 1"
    assert face.name == "Door A"
    assert "cost: 2" in face.to_text()


# -------------------------------------------------- generic walker


class Color(enum.Enum):
    DEEP_RED = "r"


@dataclass
class Node:
    label: str
    child: "Node | None" = None
    kids: list = field(default_factory=list)


@dataclass(frozen=True)
class Params:
    required: int
    flag: bool = False
    zone: str = "battlefield"
    off: int = -1
    amount: int = 1
    ratio: float = 0.0
    raw: bytes = b""
    color: Color = Color.DEEP_RED
    table: dict = field(default_factory=dict)
    nested: tuple = ()


class Plain:
    def __init__(self):
        self.visible = 3
        self._private = 4


class Slotted:
    __slots__ = ("a", "b", "_c")

    def __init__(self):
        self.a = 1
        self._c = 2


class Opaque:
    __slots__ = ()


def test_non_default_fields_rules():
    items = dict(rr.non_default_fields(Params(required=5)))
    # required fields and meaningful defaults are shown, "off" defaults hidden
    assert items == {
        "required": 5, "zone": "battlefield", "amount": 1,
        "color": Color.DEEP_RED,
    }
    assert dict(rr.non_default_fields(Plain())) == {"visible": 3}
    assert dict(rr.non_default_fields(Slotted())) == {"a": 1}
    assert rr.non_default_fields(Opaque()) == []


def test_render_object_inline_block_and_values():
    obj = Params(
        required=7, flag=True, ratio=0.5, raw=b"x",
        table={"k": (1, 2), "inner": Params(required=1)},
        nested=((1, ()), ["a", "b"], {"m": 1}, Node("n")),
    )
    lines = rr.render_object(obj)
    text = "\n".join(lines)
    assert lines[0] == "Params"
    assert "  required: 7" in text
    assert "  flag" in lines  # True booleans render as the bare field name
    assert "ratio: 0.5" in text and "raw: b'x'" in text
    assert "color: deep red" in text
    assert "table:" in text and "k: 1, 2" in text
    assert "inner: Params (required: 1; zone: battlefield; amount: 1; color: deep red)" in text
    assert "- 1, (none)" in text
    assert "- a, b" in text
    assert "- group:" in text and "m: 1" in text
    assert "- Node (label: n)" in text


def test_render_object_cycle_and_empty_values():
    a = Node("a")
    a.child = a
    text = "\n".join(rr.render_object(a))
    assert "child: Node (cycle)" in text
    assert rr.format_scalar(None) == "none"
    assert rr.format_scalar("") == '""'
    assert rr.format_scalar("a\nb") == "a / b"
    assert rr.format_scalar(False) == "no"
    assert rr.format_scalar_container(()) == "(none)"
    assert rr.format_scalar_container(frozenset({"b", "a"})) == "a, b"


def test_render_object_long_inline_falls_back_to_block():
    obj = Params(required=1, zone="z" * 150)
    lines = rr.render_object(obj)
    assert lines[0] == "Params" and len(lines) > 1


def test_loyalty_label_default():
    assert rr.loyalty_label(object()) == "[0] "


# ------------------------------------------------------ review flags


def _card(oracle, keywords=(), name="Test Card", faces=None):
    card = {"name": name, "oracle_text": oracle, "keywords": list(keywords)}
    if faces:
        card["card_faces"] = faces
    return card


def _bird():
    return make_creature("Test Card", "{1}{U}", 1, 1, keywords=("flying",))


def test_flags_dropped_line_and_number():
    card = _card("Flying\nWhenever a player sacrifices seven permanents, gain 3 life.")
    flags = rr.review_flags(card, _bird(), "Keywords: flying")
    kinds = [(f.kind, f.detail) for f in flags]
    assert ("line", "Whenever a player sacrifices seven permanents, gain 3 life.") in kinds
    assert ("number", "3") in kinds and ("number", "7") in kinds
    assert all(f.describe() for f in flags)


def test_flags_quiet_when_model_matches():
    card = _card("Flying\nDraw two cards. (Reminder 9.)", keywords=["Flying"])
    assert rr.review_flags(card, _bird(), "flying\nDraw cards (amount: 2)") == []


def test_flags_number_word_one_not_demanded():
    card = _card("Add one mana of any color.")
    assert rr.review_flags(card, _bird(), "Add mana to pool (produces: any)") == []


def test_flags_reminder_text_explains_keyword_line():
    card = _card(
        "Cycling {2} ({2}, Discard this card: Draw a card.)", keywords=["Cycling"],
    )
    model = "hand activated abilities: discard self; Draw cards (amount: 1); mana: {2}"
    assert rr.review_flags(card, _bird(), model) == []
    flags = rr.review_flags(card, _bird(), "mana: {2}")
    assert [f.kind for f in flags] == ["line", "keyword"]
    assert "Cycling" in flags[1].describe()


def test_flags_keyword_skips_ability_words_and_umbrella_tags():
    card = _card(
        "Landfall — Whenever a land you control enters, draw a card.",
        keywords=["Landfall", "Typecycling"],
    )
    assert rr.review_flags(card, _bird(), "event: land enters; Draw cards") == []


def test_flags_keyword_present_in_model_text_or_keywords():
    card = _card("Flash\nWard {2}", keywords=["Flash", "Ward"])
    p = dataclasses.replace(_bird(), keywords=frozenset({"flash"}))
    assert rr.review_flags(card, p, "keywords: flash\nward cost: {2}") == []


def test_flags_use_all_faces_and_ignore_name_words():
    faces = [
        {"name": "Front Thing", "oracle_text": "Front Thing gets bigger."},
        {"name": "Back", "oracle_text": ""},
    ]
    card = _card("", name="Front Thing // Back", faces=faces)
    flags = rr.review_flags(card, _bird(), "bigger")
    assert flags == []
    flags = rr.review_flags(card, _bird(), "unrelated")
    assert [f.detail for f in flags] == ["Front Thing gets bigger."]


@pytest.mark.parametrize("line, label", [
    ("This card is banned.", "deck construction"),
    ("A deck can have any number of cards named Test Card.", "deck construction"),
    ("A deck can have up to seven cards named Test Card.", "deck construction"),
    ("Test Card can be your commander.", "commander format"),
    ("Choose a Background", "commander format"),
    ("Partner", "commander format"),
    ("Friends forever", "commander format"),
    ("Draft Test Card face up.", "draft"),
    ("As you draft a card, you may draft an additional card from that "
     "booster pack. If you do, put this card into that booster pack.", "draft"),
])
def test_no_game_effect_line_classes(line, label):
    assert rr.no_game_effect_line_class(line, "Test Card") == label


@pytest.mark.parametrize("line", [
    "Draw a card.",
    "Other Card can be your commander.",  # another card's name
    "A deck can have any number of cards named Other Card.",
    "Partner with Other Card",  # has an in-game ETB half
    "Companion — Each permanent card in your starting deck has mana value 2 or less.",
])
def test_lines_outside_the_classes_are_not_acknowledged(line):
    assert rr.no_game_effect_line_class(line, "Test Card") is None


def test_acknowledged_lines_produce_no_hints():
    card = _card(
        "Choose a Background (You can have a Background as a second commander.)\n"
        "Test Card can be your commander.\n"
        "Draft Test Card face up.\n"
        "A deck can have up to seven cards named Test Card.\n"
        "Flying",
        keywords=["Choose a background", "Flying"],
    )
    assert rr.review_flags(card, _bird(), "keywords: flying") == []
    # A real dropped line next to them is still flagged.
    card = _card("This card is banned.\nWhenever you win, sacrifice everything.")
    assert [f.kind for f in rr.review_flags(card, _bird(), "flying")] == ["line"]


def test_modeled_keywords_walks_faces(printings):
    p = printings["Brazen Borrower // Petty Theft"]
    assert {"flash", "flying"} <= rr.modeled_keywords(p)
    back = make_creature("B", "{1}", 1, 1, keywords=("first strike",))
    front = dataclasses.replace(_bird(), transform_back_face=back)
    assert "first strike" in rr.modeled_keywords(front)


def test_oracle_faces_pt_and_loyalty():
    faces = rr.oracle_faces({
        "name": "X", "card_faces": [
            {"name": "A", "power": "2", "toughness": "3", "oracle_text": "a"},
            {"name": "B", "loyalty": "4", "oracle_text": "b"},
        ],
    })
    assert [f["pt"] for f in faces] == ["2/3", "loyalty 4"]


# ----------------------------------------- doc bullets + test matching


_DOC = """# Title

Intro paragraph naming nothing.

## Section One

- **Jace the Mind Sculptor** brainstorms without the stack.
  continuation line naming Grim Monolith.
- Flash is granted by the keyword flash (lowercase must not match).
1. numbered item mentions LIFE // DEATH.

**2. Bold numbered.** Mentions Forestcycling but not the basic.
### Sub

Plain paragraph about Petty Theft.
"""


def test_parse_doc_bullets():
    bullets = rr.parse_doc_bullets(_DOC)
    assert bullets[0] == rr.DocBullet("Title", "Intro paragraph naming nothing.")
    assert bullets[1].heading == "Section One"
    assert "continuation line" in bullets[1].text
    assert len(bullets) == 6
    assert bullets[-1].heading == "Sub"


def test_card_aliases():
    assert rr.card_aliases({"name": "Jace, the Mind Sculptor"}) == [
        "Jace, the Mind Sculptor", "Jace the Mind Sculptor",
    ]
    split = {"name": "Life // Death", "card_faces": [{"name": "Life"}, {"name": "Death"}]}
    assert rr.card_aliases(split) == ["Life // Death"]
    adv = {"name": "Brazen Borrower // Petty Theft",
           "card_faces": [{"name": "Brazen Borrower"}, {"name": "Petty Theft"}]}
    assert rr.card_aliases(adv) == ["Brazen Borrower // Petty Theft", "Brazen Borrower"]


def test_match_doc_bullets_case_rules():
    cards = [
        {"name": "Jace, the Mind Sculptor"}, {"name": "Grim Monolith"},
        {"name": "Flash"}, {"name": "Forest"},
        {"name": "Life // Death", "card_faces": [{"name": "Life"}, {"name": "Death"}]},
        {"name": "Brazen Borrower // Petty Theft",
         "card_faces": [{"name": "Brazen Borrower"}, {"name": "Petty Theft"}]},
    ]
    hits = rr.match_doc_bullets(cards, rr.parse_doc_bullets(_DOC))
    assert hits["Jace, the Mind Sculptor"] == [1]
    assert hits["Grim Monolith"] == [1]
    assert hits["Flash"] == [2]           # capitalized only, once per bullet
    assert hits["Life // Death"] == [3]   # multi-word: case-insensitive
    assert "Forest" not in hits           # inside "Forestcycling"
    assert "Brazen Borrower // Petty Theft" not in hits  # back face not matched
    assert rr.match_doc_bullets([], []) == {}


def test_match_test_files():
    cards = [{"name": "Grim Monolith"}, {"name": "Flash"}]
    files = {
        "tests/a.py": 'p = source["Grim Monolith"]\nx = "Grim Monolith"\nflash = 1',
        "tests/b.py": "FlashBack; _Flash; Flash",
    }
    hits = rr.match_test_files(cards, files)
    assert hits == {"Grim Monolith": {"tests/a.py": 2}, "Flash": {"tests/b.py": 1}}
    assert rr.match_test_files([], files) == {}


def test_load_test_files(tmp_path):
    (tmp_path / "tests" / "sub").mkdir(parents=True)
    (tmp_path / "tests" / "sub" / "test_x.py").write_text("Sol Ring", encoding="utf-8")
    (tmp_path / "tests" / "notes.txt").write_text("ignored", encoding="utf-8")
    assert rr.load_test_files(tmp_path / "tests", tmp_path) == {
        "tests/sub/test_x.py": "Sol Ring",
    }


# ----------------------------------------------------------- the page


def _mini_reviews(printings, raw_cards):
    cards = [
        raw_cards["Grim Monolith"],
        raw_cards["Lim-Dûl's Vault"],
        {"name": "<Evil & Card>", "type_line": "Creature", "oracle_text": "Flying",
         "mana_cost": "{W}", "color_identity": ["W", "U"], "keywords": []},
        raw_cards["Life // Death"],
    ]
    bullets = [rr.DocBullet("Head <b>", "Grim Monolith untaps <slowly>.")]
    files = {f"tests/t{i}.py": "Grim Monolith" for i in range(30)}
    return rr.build_reviews(cards, printings, bullets=bullets, test_files=files)


def test_build_reviews(printings, raw_cards):
    reviews = _mini_reviews(printings, raw_cards)
    assert [r.name for r in reviews][0] == "<Evil & Card>"
    evil = reviews[0]
    assert evil.summary is None and evil.flags[0].kind == "unclaimed"
    assert "Not claimed" in evil.flags[0].describe()
    assert evil.color_identity == "WU"
    assert evil.anchor == "card-evil-card"
    grim = next(r for r in reviews if r.name == "Grim Monolith")
    assert len(grim.shortcuts) == 1 and len(grim.tests) == 30
    assert grim.primary_type == "Artifact"
    assert rr.most_flagged(reviews)[0].name == "<Evil & Card>"
    plain = rr.build_reviews([raw_cards["Grim Monolith"]], printings)
    assert plain[0].shortcuts == [] and plain[0].tests == []


def test_render_html_escapes_and_is_self_contained(printings, raw_cards):
    page = rr.render_html(_mini_reviews(printings, raw_cards))
    assert page.startswith("<!DOCTYPE html>")
    assert '<meta charset="utf-8">' in page
    assert "&lt;Evil &amp; Card&gt;" in page and "<Evil & Card>" not in page
    assert "untaps &lt;slowly&gt;" in page and "Head &lt;b&gt;" in page
    assert "Lim-Dûl&#x27;s Vault" in page
    assert "http://" not in page and "https://" not in page
    assert page.count('<details class="card"') == 4
    assert "5 more files" in page  # 30 test files, 25 shown
    assert "Automatic review hints (heuristic, not verdicts)" in page
    assert 'id="f-shortcut"' in page and 'id="f-flags"' in page
    assert "No SEARCH_LIMITATIONS.md paragraph names this card." in page
    assert "No test file under tests/ names this card." in page


def test_render_markdown(printings, raw_cards):
    md = rr.render_review_markdown(_mini_reviews(printings, raw_cards))
    assert md.startswith("Generated by `scripts/render_cube_card_review.py`")
    assert "## Grim Monolith" in md
    assert "(not claimed by the parser)" in md
    assert "[Life] {G} Sorcery" in md
    assert "Tests: 30 files, 30 mentions" in md and "and 20 more" in md
    assert "- *Head <b>*: Grim Monolith untaps <slowly>." in md
    empty = rr.render_review_markdown([])
    assert "(none)" in empty
    long = rr._md_excerpt("word " * 200)
    assert long.endswith("...") and len(long) == 240


# --------------------------------------------------------- the script


@pytest.fixture(scope="module")
def script():
    spec = importlib.util.spec_from_file_location(
        "_render_cube_card_review_under_test", SCRIPT_PATH,
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod




def test_test_counts_pluralize():
    assert "<p>1 file, 1 mention.</p>" in rr._tests_html([("tests/a.py", 1)])
    assert "<p>2 files, 5 mentions.</p>" in rr._tests_html(
        [("tests/a.py", 4), ("tests/b.py", 1)],
    )
