"""``mtgz_ir.cards.unclaimed_lines`` -- per-line diagnosis of unclaimed
cards, against fake builders (grammar-independent) plus one real-parser
smoke test."""

from __future__ import annotations

from mtgz_ir.cards import unclaimed_lines as ul


def _card(text: str, **extra) -> dict:
    return {"name": "Probe", "type_line": "Instant", "oracle_text": text, **extra}


def _lines(card: dict) -> list[str]:
    if card.get("card_faces"):
        out: list[str] = []
        for face in card["card_faces"]:
            out += [ln for ln in face["oracle_text"].split("\n") if ln]
        return out
    return [ln for ln in card["oracle_text"].split("\n") if ln]


def _builder(known: set[str], *, needs_any: bool = True,
             requires: dict[str, str] | None = None):
    """Fake builder: every line must be in ``known``; ``needs_any``
    rejects text-less records; ``requires[rider] = partner`` makes a
    rider parse only next to its partner."""
    requires = requires or {}

    def build(card: dict) -> bool:
        lines = _lines(card)
        if needs_any and not lines:
            return False
        for ln in lines:
            if ln not in known:
                return False
            partner = requires.get(ln)
            if partner is not None and partner not in lines:
                return False
        return True

    return build


# ------------------------------------------------------------------ units


def test_split_units_strips_reminders_and_groups_bullets():
    card = _card(
        "Flying (This creature can't be blocked except by flyers.)\n"
        "(Pure reminder line.)\n"
        "Choose one —\n• Draw a card.\n• Gain 2 life.\n"
        "Scry 1."
    )
    units = ul.split_units(card)
    assert [u.text for u in units] == [
        "Flying",
        "Choose one — • Draw a card. • Gain 2 life.",
        "Scry 1.",
    ]
    assert units[1].raw == "Choose one —\n• Draw a card.\n• Gain 2 life."
    assert all(u.face is None for u in units)


def test_split_units_walks_faces_and_never_groups_across_them():
    card = {
        "name": "Two Face", "type_line": "Creature // Creature",
        "card_faces": [
            {"oracle_text": "Flying"},
            {"oracle_text": "• orphan bullet\nTrample"},
            {},
        ],
    }
    units = ul.split_units(card)
    assert [(u.face, u.text) for u in units] == [
        (0, "Flying"), (1, "• orphan bullet"), (1, "Trample"),
    ]


def test_with_units_rewrites_text_per_face():
    card = {"name": "X", "card_faces": [{"oracle_text": "A\nB"}, {"oracle_text": "C"}]}
    units = ul.split_units(card)
    variant = ul.with_units(card, [units[1], units[2]])
    assert [f["oracle_text"] for f in variant["card_faces"]] == ["B", "C"]
    assert card["card_faces"][0]["oracle_text"] == "A\nB"  # original untouched
    single = _card("A\nB")
    assert ul.with_units(single, [ul.split_units(single)[1]])["oracle_text"] == "B"


# -------------------------------------------------------------- diagnose


def test_diagnose_confirmed_lines():
    card = _card("Draw a card.\nSplit second\nScry 1.")
    d = ul.diagnose(card, _builder({"Draw a card.", "Scry 1."}))
    assert d.kind == "lines" and d.confirmed
    assert d.failing_text == ["Split second"]
    assert [u.text for u in d.passing] == ["Draw a card.", "Scry 1."]


def test_diagnose_rescues_riders_in_context():
    card = _card("As an additional cost, sacrifice a creature.\nDraw two cards.\nBogus.")
    build = _builder(
        {"As an additional cost, sacrifice a creature.", "Draw two cards."},
        requires={"As an additional cost, sacrifice a creature.": "Draw two cards."},
    )
    d = ul.diagnose(card, build)
    assert d.failing_text == ["Bogus."]
    assert d.kind == "lines" and d.confirmed


def test_diagnose_witness_rescues_keyword_and_confirms_frame():
    card = _card("Kicker {W}\nTarget player can't cast spells.")
    build = _builder({"Kicker {W}", "Draw a card."},
                     requires={"Kicker {W}": "Draw a card."})
    without = ul.diagnose(card, build)
    assert without.kind == "unconfirmed"
    assert without.failing_text == ["Kicker {W}", "Target player can't cast spells."]
    with_w = ul.diagnose(card, build, witness="Draw a card.")
    assert with_w.failing_text == ["Target player can't cast spells."]
    assert with_w.kind == "lines" and with_w.confirmed


def test_diagnose_ignores_a_witness_that_does_not_build_on_the_card():
    card = _card("Bogus.")
    d = ul.diagnose(card, _builder(set()), witness="Draw a card.")
    assert d.kind == "unconfirmed" and not d.confirmed
    assert d.failing_text == ["Bogus."]


def test_diagnose_skips_witness_for_multi_face_cards():
    card = {"name": "MF", "type_line": "Creature // Sorcery",
            "card_faces": [{"oracle_text": "Bogus."}, {"oracle_text": ""}]}
    calls: list[dict] = []

    def build(c):
        calls.append(c)
        return False

    d = ul.diagnose(card, build, witness="Draw a card.")
    assert d.kind == "unconfirmed"
    assert all("Draw a card." not in str(c) for c in calls)


def test_diagnose_mixed_when_passing_lines_do_not_combine():
    card = _card("A.\nB.\nBogus.")

    def build(c):
        lines = _lines(c)
        return bool(lines) and set(lines) <= {"A.", "B."} and len(lines) == 1

    d = ul.diagnose(card, build)
    assert d.kind == "mixed" and not d.confirmed
    assert d.failing_text == ["Bogus."]


def test_diagnose_structure_when_every_line_parses():
    card = _card("A.\nB.")

    def build(c):
        return len(_lines(c)) == 1  # each alone, never together

    d = ul.diagnose(card, build)
    assert d.kind == "structure" and d.failing == [] and not d.confirmed


def test_diagnose_real_parser_smoke():
    """Against the real builder chain: a known-good instant with one
    invented line fails exactly that line."""
    from mtgz_ir.cards.parser.source import build_card_record

    card = {
        "name": "Probe Bolt", "type_line": "Instant", "mana_cost": "{R}",
        "layout": "normal", "colors": ["R"],
        "oracle_text": "Probe Bolt deals 3 damage to any target.\n"
                       "Zorble the frobnitz.",
    }

    def claims(c):
        return build_card_record(c) is not None

    d = ul.diagnose(card, claims, witness="Draw a card.")
    assert d.failing_text == ["Zorble the frobnitz."]
    assert d.kind == "lines" and d.confirmed
