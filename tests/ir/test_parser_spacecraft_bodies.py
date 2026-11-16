"""Parser support for Spacecraft body abilities — slice 2: a single
``"When this Spacecraft enters, <effect>."`` ETB trigger alongside the
Station + tier-keyword pair already claimed by slice 1.

Slice 1 (``test_parser_spacecraft_builder.py``) covers the bare
Station + tier-keyword shape and the negative cases for cards that
fall outside the body-less grammar. This file focuses on the new
ETB-trigger arm: the cards it newly claims (Wedgelight Rammer,
Wurmwall Sweeper) plus the negatives that keep adjacent body-ability
shapes — multiple triggers, attack triggers, non-trigger clauses —
out of scope for this slice.

Wedgelight Rammer's ``"create a 2/2 colorless Robot artifact creature
token"`` and Wurmwall Sweeper's ``"surveil 2"`` are both effects whose
phrase already resolves through ``_parse_effect_phrase``, so the
parser-built printing equals the static one byte-for-byte without any
new effect-grammar work — the only new code lives in the spacecraft
builder.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    TriggeredAbility,
    make_spacecraft,
)
from mtgz_ir.cards.parser.spacecraft_builder import (
    _build_spacecraft_printing,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.effects.generic import (
    CreateToken,
    Surveil,
)
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS


_WEDGELIGHT_RAMMER_CARD = {
    "name": "Wedgelight Rammer",
    "mana_cost": "{3}{W}",
    "type_line": "Artifact — Spacecraft",
    "power": "3",
    "toughness": "4",
    "oracle_text": (
        "When this Spacecraft enters, create a 2/2 colorless Robot "
        "artifact creature token.\n"
        "Station (Tap another creature you control: Put charge counters "
        "equal to its power on this Spacecraft. Station only as a "
        "sorcery. It's an artifact creature at 9+.)\n"
        "9+ | Flying, first strike"
    ),
}

_WURMWALL_SWEEPER_CARD = {
    "name": "Wurmwall Sweeper",
    "mana_cost": "{2}",
    "type_line": "Artifact — Spacecraft",
    "power": "2",
    "toughness": "2",
    "oracle_text": (
        "When this Spacecraft enters, surveil 2.\n"
        "Station (Tap another creature you control: Put charge counters "
        "equal to its power on this Spacecraft. Station only as a "
        "sorcery. It's an artifact creature at 4+.)\n"
        "4+ | Flying"
    ),
}


def test_build_spacecraft_printing_claims_wedgelight_rammer():
    """The Wedgelight Rammer Scryfall record builds a white Spacecraft
    with station_threshold=9, stationed_keywords={flying, first strike},
    and a single self-ETB ``CreateToken(Robot)`` trigger - exactly what
    the static registry passes to ``make_spacecraft``."""
    printing = _build_spacecraft_printing(_WEDGELIGHT_RAMMER_CARD)
    assert printing is not None
    assert printing.name == "Wedgelight Rammer"
    assert "Spacecraft" in printing.subtypes
    assert "Artifact" in printing.card_types
    assert printing.power == 3
    assert printing.toughness == 4
    assert printing.station_threshold == 9
    assert printing.stationed_keywords == frozenset(
        {"flying", "first strike"}
    )
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ENTERS
    assert trig.predicate == IsSelf()
    assert trig.effect == CreateToken(
        token_printing=TOKEN_PRINTINGS["Robot"], count=1,
    )
    # No statics, no activated abilities - this slice claims only the
    # ETB-trigger shape.
    assert printing.static_abilities == ()
    assert printing.activated_abilities == ()


def test_build_spacecraft_printing_claims_wurmwall_sweeper():
    """Wurmwall Sweeper's surveil-2 ETB is a different effect family
    (graveyard-mill / library-look) than Wedgelight Rammer's token
    create, but flows through the same ``_parse_clause`` arm and lands
    as a single ``TriggeredAbility(event=ENTERS, predicate=IsSelf(),
    effect=Surveil(amount=2))``."""
    printing = _build_spacecraft_printing(_WURMWALL_SWEEPER_CARD)
    assert printing is not None
    assert printing.name == "Wurmwall Sweeper"
    assert printing.station_threshold == 4
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ENTERS
    assert trig.predicate == IsSelf()
    assert trig.effect == Surveil(amount=2)


def test_build_spacecraft_printing_matches_static_wedgelight_rammer():
    """Per-card parity with the canonical static registry. The parser-
    built printing must equal what ``make_spacecraft`` produces with
    the same arguments - the parity gate in ``test_parser_parity.py``
    asserts the same property at the source level, but pinning it
    here gives faster feedback when the static card and the parser
    drift."""
    parsed = _build_spacecraft_printing(_WEDGELIGHT_RAMMER_CARD)
    expected = make_spacecraft(
        "Wedgelight Rammer", "{3}{W}", 3, 4,
        station_threshold=9,
        stationed_keywords={"flying", "first strike"},
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=CreateToken(
                    token_printing=TOKEN_PRINTINGS["Robot"], count=1,
                ),
            ),
        ),
    )
    assert parsed == expected


def test_build_spacecraft_printing_matches_static_wurmwall_sweeper():
    """Same byte-equality parity check for Wurmwall Sweeper - covers
    the surveil-effect arm of the new trigger handling."""
    parsed = _build_spacecraft_printing(_WURMWALL_SWEEPER_CARD)
    expected = make_spacecraft(
        "Wurmwall Sweeper", "{2}", 2, 2,
        station_threshold=4,
        stationed_keywords={"flying"},
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=Surveil(amount=2),
            ),
        ),
    )
    assert parsed == expected


def test_build_spacecraft_printing_claims_attack_trigger():
    """A Spacecraft with a single ``Whenever this Spacecraft attacks,
    <effect>.`` line whose effect resolves through ``_parse_clause``
    (here ``surveil 2``) is now claimed alongside the Station + tier-
    keyword pair. The trigger builds as ``TriggeredAbility(event=
    ATTACKS, predicate=IsSelf(), effect=Surveil(amount=2))``.

    No real EoE Spacecraft uses surveil on attack — Specimen
    Freighter (defender-mill), Sledge-Class Seedship (free creature
    into play), and the others use effects that aren't yet in the
    effect grammar — so this synthetic card pins the new attack-
    trigger arm directly."""
    card = {
        "name": "Hand-Crafted Attack-Trigger Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Whenever this Spacecraft attacks, surveil 2.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 4
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ATTACKS
    assert trig.predicate == IsSelf()
    assert trig.effect == Surveil(amount=2)


def test_build_spacecraft_printing_skips_two_attack_triggers():
    """Two ``Whenever this Spacecraft attacks, ...`` lines on the
    same card would silently drop one if we kept only the first.
    Defensive: at most one attack trigger per card; a second one
    skips the whole card. No EoE Spacecraft prints two attack
    triggers today, so this is forward-looking."""
    card = {
        "name": "Hand-Crafted Double-Attack Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Whenever this Spacecraft attacks, surveil 2.\n"
            "Whenever this Spacecraft attacks, surveil 2.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_two_etb_triggers():
    """A Spacecraft with two ETB trigger lines is out of scope for this
    slice - claiming only the first would silently drop the second.
    No EoE Spacecraft prints two ETB lines today, but the defensive
    skip means a future printing won't be half-claimed."""
    card = {
        "name": "Hand-Crafted Double-ETB Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "When this Spacecraft enters, surveil 2.\n"
            "When this Spacecraft enters, create a 2/2 colorless Robot "
            "artifact creature token.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_claims_etb_plus_attack_trigger():
    """A Spacecraft that pairs a self-ETB with a self-ATTACKS trigger
    (Specimen Freighter-shaped: ETB returns creatures, attack trigger
    mills the defender) is now claimed when both effects resolve
    through the unified parser. Both triggers land on
    ``triggered_abilities`` in the order they appear on the card —
    the engine doesn't depend on order between distinct events, but
    pinning iteration order keeps the parsed printing byte-equal to
    a static-registry analogue. The attack trigger is printed below
    ``9+ | Flying``, in the 9+ striation (CR 722.2), so it carries
    ``tier_threshold=9`` (mtg-zero docs/ir_defects.md 340).

    Specimen Freighter itself stays unclaimed today: its attack-
    trigger effect (``defending player mills four cards``) isn't in
    the effect grammar yet. This synthetic card uses ``surveil 2``
    on both lines so both clauses resolve and the dual-trigger arm
    is exercised end to end."""
    card = {
        "name": "Hand-Crafted ETB-Plus-Attack Spacecraft",
        "mana_cost": "{5}{U}",
        "type_line": "Artifact — Spacecraft",
        "power": "4",
        "toughness": "3",
        "oracle_text": (
            "When this Spacecraft enters, surveil 2.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 9+.)\n"
            "9+ | Flying\n"
            "Whenever this Spacecraft attacks, surveil 2."
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 9
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 2
    events = {t.event for t in printing.triggered_abilities}
    assert events == {Event.ENTERS, Event.ATTACKS}
    for trig in printing.triggered_abilities:
        assert trig.predicate == IsSelf()
        assert trig.effect == Surveil(amount=2)
    expected = make_spacecraft(
        "Hand-Crafted ETB-Plus-Attack Spacecraft", "{5}{U}", 4, 3,
        station_threshold=9,
        stationed_keywords={"flying"},
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=Surveil(amount=2),
            ),
            TriggeredAbility(
                event=Event.ATTACKS, predicate=IsSelf(),
                effect=Surveil(amount=2), tier_threshold=9,
            ),
        ),
    )
    assert printing == expected


def test_build_spacecraft_printing_skips_clause_returning_static():
    """A Spacecraft whose body line parses through ``_parse_clause`` to
    something other than a trigger (e.g. a bare keyword line that the
    keyword parser claims, or a static ability) is not the shape this
    slice claims. The builder accepts only triggers from
    ``_parse_clause``; anything else skips the whole card to avoid
    silently dropping or mis-wiring the ability."""
    # A standalone "Flying" line on a Spacecraft is grammatically odd
    # (Spacecraft grant keywords through tier lines, not bare lines),
    # but ``_parse_keyword_line`` will recognize it. The builder must
    # reject the non-trigger clause.
    card = {
        "name": "Hand-Crafted Bare-Keyword-Line Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Flying\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_source_claims_wedgelight_rammer_and_wurmwall_sweeper():
    """End-to-end: ``OracleParserCardSource`` walks the cached EoE
    Scryfall data and now claims both new Spacecraft alongside the
    slice-1 Galvanizing Sawship via the extended spacecraft-builder
    arm."""
    source = OracleParserCardSource()
    claimed = set(source.claimed_names)
    assert "Wedgelight Rammer" in claimed
    assert "Wurmwall Sweeper" in claimed
    # Slice-1 claim still in place.
    assert "Galvanizing Sawship" in claimed




def test_build_spacecraft_printing_claims_attack_trigger_spacecraft():
    """Specimen Freighter, Sledge-Class Seedship, Entropic Battlecruiser,
    Infinite Guideline Station, and Dawnsire, Sunstar Dreadnought all
    carry attack triggers whose effect phrases were outside the unified
    effect grammar in earlier batches. All five are now claimed by the
    unified oracle-text grammar (Entropic Battlecruiser and Dawnsire via
    the current batch's grammar extensions; the others via earlier slices),
    so they must be present in ``claimed_names``."""
    source = OracleParserCardSource()
    claimed = set(source.claimed_names)
    assert "Specimen Freighter" in claimed
    assert "Sledge-Class Seedship" in claimed
    assert "Entropic Battlecruiser" in claimed
    assert "Infinite Guideline Station" in claimed
    assert "Dawnsire, Sunstar Dreadnought" in claimed


def test_build_spacecraft_printing_skips_attack_trigger_unparseable_card():
    """Synthetic counterpart of the source-level skip above: a hand-
    crafted Spacecraft with a parseable Station + tier-keyword pair
    but whose attack-trigger effect phrase isn't in the effect
    grammar (a synthetic ``draw a card for each blue creature you
    control`` rider — no per-blue-creature primitive lives in the
    unified grammar today) returns ``None`` from the builder. The
    attack-trigger line falls through ``_parse_clause`` (no matching
    trigger phrase + effect combo) and the per-line dispatch bails on
    the first unrecognized line. The previous version of this test
    used Infinite Guideline Station's
    ``draw a card for each multicolored permanent you control`` body
    as the unparseable phrase; that surface now claims through the
    unified grammar (de-bespoke batch 4), so the synthetic was
    swapped to a still-unparseable shape."""
    card = {
        "name": "Hand-Crafted Unparseable-Attack Spacecraft",
        "mana_cost": "{5}{U}",
        "type_line": "Artifact — Spacecraft",
        "power": "4",
        "toughness": "7",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 9+.)\n"
            "9+ | Flying\n"
            "Whenever this Spacecraft attacks, draw a card for each "
            "blue creature you control."
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_other_event_self_trigger():
    """Defensive: a self-targeted trigger on an event other than
    ENTERS or ATTACKS (here a synthetic ``Whenever this creature
    becomes tapped`` line, which the trigger grammar accepts as
    ``Event.BECOMES_TAPPED, IsSelf()``) is not a Spacecraft body
    shape we claim. The builder restricts to the two events we
    explicitly model so a future printing whose effect happens to
    parse can't sneak through with a mis-wired trigger event."""
    card = {
        "name": "Hand-Crafted Self-Tapped Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Whenever this creature becomes tapped, surveil 2.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_non_self_trigger():
    """Defensive: a trigger whose predicate isn't ``IsSelf()`` (here
    a synthetic ``Whenever a land you control enters, surveil 2.``
    line, which parses as ``Event.ENTERS, IsLand() & Controlled(YOU)``)
    is some other observer event — not a Spacecraft body trigger.
    The builder rejects it so a future printing that happens to
    parse this shape can't be mis-claimed as a Spacecraft self-ETB."""
    card = {
        "name": "Hand-Crafted Landfall-Ish Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Whenever a land you control enters, surveil 2.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_legendary_with_etb_trigger():
    """A Legendary Spacecraft that pairs the supported Station +
    tier-keyword shape with a single self-ETB trigger whose effect is
    in the grammar is claimed end to end with ``legendary=True``.
    This pins the cross-arm interaction (the Legendary supertype
    forwarding doesn't disrupt trigger handling)."""
    card = {
        "name": "Hand-Crafted Legendary ETB Spacecraft",
        "mana_cost": "{1}{U}{U}",
        "type_line": "Legendary Artifact — Spacecraft",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "When this Spacecraft enters, surveil 2.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 5+.)\n"
            "5+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert "Legendary" in printing.supertypes
    assert printing.station_threshold == 5
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ENTERS
    assert trig.predicate == IsSelf()
    assert trig.effect == Surveil(amount=2)
    expected = make_spacecraft(
        "Hand-Crafted Legendary ETB Spacecraft", "{1}{U}{U}", 3, 3,
        station_threshold=5,
        stationed_keywords={"flying"},
        legendary=True,
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=Surveil(amount=2),
            ),
        ),
    )
    assert printing == expected
