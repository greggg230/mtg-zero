"""Parser support for Artifact - Spacecraft cards.

Spacecraft are Edge of Eternities-only artifact-type cards that gain
charge counters via the printed ``Station`` ability and animate into
artifact creatures with a printed ``power``/``toughness`` and a set
of stationed keywords once they pass the ``station_threshold``.

The first slice claimed here is the smallest end-to-end shape: a
single Station reminder line plus a single ``"<N>+ | <evergreen-
keyword-list>"`` tier line. Galvanizing Sawship is the only EoE
Spacecraft that fits this shape today; every other printing carries
either an ETB trigger, an attack trigger, a lower tier ability, or a
printed activated ability that lives outside this builder's grammar.
"""

from __future__ import annotations

from mtgz_ir.cards.base import make_spacecraft
from mtgz_ir.cards.parser.spacecraft_builder import (
    _build_spacecraft_printing,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


_GALVANIZING_SAWSHIP_CARD = {
    "name": "Galvanizing Sawship",
    "mana_cost": "{5}{R}",
    "type_line": "Artifact — Spacecraft",
    "power": "6",
    "toughness": "5",
    "oracle_text": (
        "Station (Tap another creature you control: Put charge counters "
        "equal to its power on this Spacecraft. Station only as a "
        "sorcery. It's an artifact creature at 3+.)\n"
        "3+ | Flying, haste"
    ),
}


def test_build_spacecraft_printing_claims_galvanizing_sawship():
    """The Scryfall-shaped Galvanizing Sawship record builds a red
    Spacecraft with station_threshold=3 and stationed_keywords=
    {flying, haste} - the same arguments the static registry passes
    to ``make_spacecraft`` for this card."""
    printing = _build_spacecraft_printing(_GALVANIZING_SAWSHIP_CARD)
    assert printing is not None
    assert printing.name == "Galvanizing Sawship"
    assert "Spacecraft" in printing.subtypes
    assert "Artifact" in printing.card_types
    assert "Legendary" not in printing.supertypes
    assert printing.power == 6
    assert printing.toughness == 5
    assert printing.station_threshold == 3
    assert printing.stationed_keywords == frozenset({"flying", "haste"})
    # No ETB trigger, no static ability, no activated ability on this
    # printing - the only printed text past Station is the keyword
    # tier line.
    assert printing.triggered_abilities == ()
    assert printing.static_abilities == ()
    assert printing.activated_abilities == ()


def test_build_spacecraft_printing_matches_static_make_spacecraft():
    """The parser-built printing must equal what the static registry
    produces via ``make_spacecraft(...)`` - required by the parity
    gate, asserted directly here for fast feedback."""
    parsed = _build_spacecraft_printing(_GALVANIZING_SAWSHIP_CARD)
    expected = make_spacecraft(
        "Galvanizing Sawship", "{5}{R}", 6, 5,
        station_threshold=3,
        stationed_keywords={"flying", "haste"},
    )
    assert parsed == expected


def test_build_spacecraft_printing_supports_single_keyword_tier():
    """A tier line with a single evergreen keyword (no comma) is
    still claimed - the keyword body parser handles a one-element
    list cleanly."""
    card = {
        "name": "Hand-Crafted Lone-Keyword Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
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


def test_build_spacecraft_printing_skips_non_artifact():
    """A non-artifact card (e.g. a creature whose subtype list happens
    to include ``Spacecraft``) must not match - the type-line check
    requires ``Artifact`` in card_types."""
    card = {
        "name": "Hand-Crafted Creature Sub-Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Creature — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": "3+ | Flying",
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_non_spacecraft_artifact():
    """A plain ``Artifact`` (no ``Spacecraft`` subtype) must not match
    even if it happens to have a Station-shaped line in its oracle
    text. The subtype check guards Equipment / Vehicles / plain
    artifacts from this builder's grammar."""
    card = {
        "name": "Hand-Crafted Artifact-Not-Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_claims_legendary_spacecraft():
    """A Legendary Spacecraft whose body fits the supported grammar
    (Station + tier-keyword line, optionally one self-ETB / self-
    ATTACKS trigger) is claimed with ``legendary=True`` forwarded to
    ``make_spacecraft``. The ``Legendary`` supertype lands on the
    printing's ``supertypes`` tuple via ``_parse_type_line`` of the
    type line ``make_spacecraft`` synthesizes.

    No real EoE Legendary Spacecraft fits the supported grammar today
    — every printing carries either a tier-gated trigger, missing
    P/T, or an out-of-grammar effect — so this test pins the synthetic
    shape that would be claimed if a future printing landed inside
    the grammar."""
    card = {
        "name": "Hand-Crafted Legendary Spacecraft",
        "mana_cost": "{1}{W}{W}",
        "type_line": "Legendary Artifact — Spacecraft",
        "power": "5",
        "toughness": "5",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 7+.)\n"
            "7+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.name == "Hand-Crafted Legendary Spacecraft"
    assert "Legendary" in printing.supertypes
    assert "Spacecraft" in printing.subtypes
    assert "Artifact" in printing.card_types
    assert printing.station_threshold == 7
    assert printing.stationed_keywords == frozenset({"flying"})
    assert printing.triggered_abilities == ()
    expected = make_spacecraft(
        "Hand-Crafted Legendary Spacecraft", "{1}{W}{W}", 5, 5,
        station_threshold=7,
        stationed_keywords={"flying"},
        legendary=True,
    )
    assert printing == expected


def test_build_spacecraft_printing_skips_missing_power_toughness():
    """An animating Spacecraft (one with a Station line that includes
    "It's an artifact creature at N+.") must have printed P/T — the
    stationed creature needs real stats. A card with the animating
    Station line but no P/T fields is rejected. Non-animating Spacecraft
    (Station line without the animation clause, like The Eternity
    Elevator) are handled separately via the no-animation Station regex
    and default to power=0/toughness=0."""
    card = {
        "name": "Hand-Crafted PT-less Spacecraft",
        "mana_cost": "{5}",
        "type_line": "Artifact — Spacecraft",
        # power/toughness intentionally omitted
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_etb_with_unparseable_effect():
    """A Spacecraft with an ETB trigger whose effect phrase isn't in
    the unified effect grammar (synthetic ``"draw a card for each blue
    creature you control"`` rider isn't claimed by any
    per-each-non-multicolored recognizer today) falls through
    ``_parse_clause`` and the whole card skips - we never half-claim a
    Spacecraft by silently dropping its ETB."""
    card = {
        "name": "Hand-Crafted Spacecraft With Unparseable ETB",
        "mana_cost": "{5}{W}",
        "type_line": "Artifact — Spacecraft",
        "power": "5",
        "toughness": "6",
        "oracle_text": (
            "When this Spacecraft enters, draw a card for each blue "
            "creature you control.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 10+.)\n"
            "10+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_claims_larval_scoutlander():
    """Larval Scoutlander's printed ETB body — ``you may sacrifice a
    land or Lander. If you do, search your library for up to two basic
    land cards, put them onto the battlefield tapped, then shuffle`` —
    is claimed by the unified effect grammar via the new
    ``_OPTIONAL_SAC_LAND_OR_LANDER_FOR_BASIC_LAND_RAMP`` recognizer
    that wraps ``OptionalSacLandOrLanderForBasicLandRamp(land_count=
    2)``. The captured count tail accepts spelled-out small cardinals
    so a future printing varying the count parses without further
    grammar work."""
    from mtgz_ir.effects.combinators import (
        OptionalSacLandOrLanderForBasicLandRamp,
    )
    from mtgz_ir.effects.predicates import (
        Event,
        IsSelf,
    )

    card = {
        "name": "Larval Scoutlander",
        "mana_cost": "{2}{G}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "When this Spacecraft enters, you may sacrifice a land or "
            "Lander. If you do, search your library for up to two "
            "basic land cards, put them onto the battlefield tapped, "
            "then shuffle.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 7+.)\n"
            "7+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 7
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ENTERS
    assert trig.predicate == IsSelf()
    assert trig.effect == OptionalSacLandOrLanderForBasicLandRamp(
        land_count=2,
    )


def test_build_spacecraft_printing_claims_the_seriema():
    """The Seriema's printed body parses through the unified clause
    grammar to a Legendary Spacecraft with:
    - ETB ``SearchLibraryForCard(filter_name="legendary_creature",
      may_fail=False)``,
    - 7+ tier flying (``stationed_keywords={"flying"}``,
      ``station_threshold=7``),
    - the ``OTHER_TAPPED_LEGENDARY_CREATURES_YOU_CONTROL``
      indestructible static in the 7+ striation (``tier_threshold=7``).

    The static is printed on the line below ``7+ | Flying``, inside the
    7+ striation: CR 722.2, "any abilities printed within the same text
    box striation as a station symbol are part of its static ability"
    (mtg-zero docs/ir_defects.md 340)."""
    from mtgz_ir.cards.base import StaticAbility
    from mtgz_ir.effects.base import StaticScope as _Scope
    from mtgz_ir.effects.generic import SearchLibraryForCard
    from mtgz_ir.effects.predicates import (
        Event,
        IsSelf,
    )

    card = {
        "name": "The Seriema",
        "mana_cost": "{1}{W}{W}",
        "type_line": "Legendary Artifact — Spacecraft",
        "power": "5",
        "toughness": "5",
        "oracle_text": (
            "When The Seriema enters, search your library for a "
            "legendary creature card, reveal it, put it into your "
            "hand, then shuffle.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 7+.)\n"
            "7+ | Flying\n"
            "Other tapped legendary creatures you control have "
            "indestructible."
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert "Legendary" in printing.supertypes
    assert printing.station_threshold == 7
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ENTERS
    assert trig.predicate == IsSelf()
    assert trig.effect == SearchLibraryForCard(
        filter_name="legendary_creature", may_fail=False,
    )
    # PR-D anthem migration: the keyword grant rides on a layer-6
    # ContinuousModifier(GrantKeyword("indestructible")) declaration;
    # the legacy ``granted_keywords`` field is empty.
    from mtgz_ir.effects.modifiers import ContinuousModifier, GrantKeyword
    assert printing.static_abilities == (
        StaticAbility(
            scope=_Scope.OTHER_TAPPED_LEGENDARY_CREATURES_YOU_CONTROL,
            tier_threshold=7,
            continuous_effect=ContinuousModifier(
                layer=6,
                payload=GrantKeyword("indestructible"),
            ),
        ),
    )


def test_build_spacecraft_printing_claims_rescue_skiff():
    """Rescue Skiff's printed ETB body — ``return target creature or
    enchantment card from your graveyard to the battlefield`` — is
    claimed by the unified effect grammar via the
    ``or_enchantment=True`` rider on
    ``ReturnCreatureFromYourGraveyardToBattlefield``. The whole card
    parses through the Spacecraft grammar to the same shape the
    bespoke factory built (Station 10+ flying, ETB graveyard
    reanimation widened to creatures-or-enchantments)."""
    from mtgz_ir.effects.generic import (
        ReturnCreatureFromYourGraveyardToBattlefield,
    )
    from mtgz_ir.effects.predicates import (
        Event,
        IsSelf,
    )

    card = {
        "name": "Rescue Skiff",
        "mana_cost": "{5}{W}",
        "type_line": "Artifact — Spacecraft",
        "power": "5",
        "toughness": "6",
        "oracle_text": (
            "When this Spacecraft enters, return target creature or "
            "enchantment card from your graveyard to the battlefield.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 10+.)\n"
            "10+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 10
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ENTERS
    assert trig.predicate == IsSelf()
    assert trig.effect == ReturnCreatureFromYourGraveyardToBattlefield(
        or_enchantment=True,
    )


def test_build_spacecraft_printing_skips_multi_tier_card():
    """A Spacecraft with more than one *keyword* tier line (synthetic
    ``2+ | Flying`` + ``12+ | Flying, lifelink``) is out of scope —
    the engine's ``stationed_keywords`` is a single-tier set, so
    claiming only the highest tier would silently drop the lower-tier
    keyword grant. Pins the tier-keyword bail-out path; tier-gated
    triggers (slice 4) and tier-gated anthem statics (slice 5) compose
    with a single keyword tier and are tested in
    ``test_parser_spacecraft_tiers``."""
    card = {
        "name": "Hand-Crafted Multi-Tier Spacecraft",
        "mana_cost": "{1}{W}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "5",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 12+.)\n"
            "2+ | Flying\n"
            "12+ | Flying, lifelink"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_unknown_keyword_in_tier():
    """A tier-line keyword that isn't in the evergreen whitelist
    (``_KEYWORD_LINES``) skips the whole card - we must never grant
    a stationed keyword the engine doesn't model."""
    card = {
        "name": "Hand-Crafted Unknown-Keyword Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | fakeword"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_tier_with_ability_body():
    """A tier line whose body isn't a pure evergreen keyword list
    (e.g. ``"2+ | Other creatures you control get +1/+1."``) is a
    static/trigger/activated ability that this slice doesn't claim -
    skip the whole card."""
    card = {
        "name": "Hand-Crafted Anthem-Tier Spacecraft",
        "mana_cost": "{1}{W}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "5",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Other creatures you control get +1/+1."
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_missing_station_line():
    """A Spacecraft with a tier-keyword line but no Station reminder
    line is malformed for this grammar - bail rather than synthesize
    a station_threshold from the tier line alone."""
    card = {
        "name": "Hand-Crafted Stationless Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": "4+ | Flying",
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_missing_tier_line():
    """A Spacecraft with a Station line but no tier-keyword line
    never animates via ``stationed_keywords``. Skip it - the engine
    requires both to model a Spacecraft that becomes a creature."""
    card = {
        "name": "Hand-Crafted Tierless Spacecraft",
        "mana_cost": "{5}",
        "type_line": "Artifact — Spacecraft",
        "power": "0",
        "toughness": "0",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_double_station_line():
    """Defensive: a card with two Station lines is malformed oracle
    text - bail rather than silently overwrite the threshold."""
    card = {
        "name": "Hand-Crafted Double-Station Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_threshold_drift():
    """Defensive: if the Station reminder threshold disagrees with
    the tier-line threshold, treat the printing as a shape outside
    this grammar and bail. No real EoE printing has this drift, so
    the parser shouldn't silently pick one number over the other."""
    card = {
        "name": "Hand-Crafted Drifted-Threshold Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "5+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_source_claims_galvanizing_sawship():
    """End-to-end: ``OracleParserCardSource`` walks the cached EoE
    Scryfall data and ends up claiming Galvanizing Sawship via the
    new spacecraft-builder dispatch arm."""
    source = OracleParserCardSource()
    assert "Galvanizing Sawship" in source.claimed_names
    assert "Galvanizing Sawship" in source.get_printings()
