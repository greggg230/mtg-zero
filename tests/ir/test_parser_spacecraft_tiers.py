"""Parser support for Spacecraft tier-gated triggered abilities and
tier-gated anthem statics — slices 4 and 5 of the Spacecraft builder.

Slice 1 (``test_parser_spacecraft_builder.py``) covers the bare
Station + single-tier-keyword shape and the negative cases for
malformed grammar. Slice 2/3 (``test_parser_spacecraft_bodies.py``)
adds standalone self-ETB and self-ATTACKS trigger lines. This file
focuses on the tier-gated dispatch arms:

- ``"<N>+ | When this Spacecraft enters, <effect>."`` and ``"<N>+ |
  Whenever this Spacecraft attacks, <effect>."`` tier lines whose
  body parses through ``_parse_clause`` to a self-ETB or self-ATTACKS
  trigger. The threshold ``N`` lands on
  ``TriggeredAbility.tier_threshold``.
- ``"<N>+ | <anthem-static>."`` tier lines whose body parses to an
  anthem-scope ``StaticAbility`` (``CREATURES_YOU_CONTROL`` /
  ``OTHER_CREATURES_YOU_CONTROL``). The threshold ``N`` lands on
  ``StaticAbility.tier_threshold``. Lumen-Class Frigate's
  ``2+ | Other creatures you control get +1/+1.`` is the EoE printing
  in this shape.

The companion **multi-tier static keyword** arm (multiple
``"<N>+ | <evergreen-keywords>"`` lines on one card) was considered
and intentionally NOT implemented in slice 4: the engine's
``StaticAbility.applies_to`` requires the target be a creature now,
and every real EoE printing's lower-tier keyword line falls below
the Station threshold (where the Spacecraft is still an artifact, not
a creature), so a self-scoped ``StaticAbility`` with ``tier_threshold``
would never fire. Modeling those grants correctly needs an engine-side
change to ``stationed_keywords`` and is left for a future slice. The
builder explicitly skips multi-tier-keyword cards (synthetic test
below pins that bail-out path) so the half-modeled shape can't slip
through. Slice 5's anthem-static arm doesn't run into this problem
because anthem scopes target *other* creatures and fire as soon as the
Spacecraft has enough charge counters regardless of whether it's
animated.

The multi-tier-trigger printings (Synthesizer Labship, Entropic
Battlecruiser, Dawnsire) all carry at least one body whose effect or
trigger phrase is outside the unified grammar. So every positive
synthetic-record claim in this file pins the new arm directly; the
real-card claims still come from slice 1/2/3, plus Lumen-Class
Frigate via slice 5.
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
from mtgz_ir.effects.generic import Surveil
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


def test_build_spacecraft_printing_claims_tier_gated_etb_trigger():
    """A ``"<N>+ | When this Spacecraft enters, <effect>."`` tier
    line builds a ``TriggeredAbility(event=ENTERS, predicate=IsSelf(),
    effect=..., tier_threshold=N)`` so the engine fires the trigger
    only when the Spacecraft has at least ``N`` charge counters
    (``TriggeredAbility.tier_threshold`` already gates this in the
    engine)."""
    card = {
        "name": "Hand-Crafted Tier-Gated ETB Spacecraft",
        "mana_cost": "{2}{B}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 6+.)\n"
            "4+ | When this Spacecraft enters, surveil 2.\n"
            "6+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 6
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ENTERS
    assert trig.predicate == IsSelf()
    assert trig.effect == Surveil(amount=2)
    assert trig.tier_threshold == 4


def test_build_spacecraft_printing_claims_tier_gated_attack_trigger():
    """A ``"<N>+ | Whenever this Spacecraft attacks, <effect>."`` tier
    line builds a ``TriggeredAbility(event=ATTACKS, predicate=IsSelf(),
    effect=..., tier_threshold=N)``. Mirrors the ETB-tier-gated arm
    via the second of the two events the Spacecraft body grammar
    accepts."""
    card = {
        "name": "Hand-Crafted Tier-Gated Attack Spacecraft",
        "mana_cost": "{3}{R}",
        "type_line": "Artifact — Spacecraft",
        "power": "4",
        "toughness": "4",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 7+.)\n"
            "5+ | Whenever this Spacecraft attacks, surveil 2.\n"
            "7+ | Flying, haste"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 7
    assert printing.stationed_keywords == frozenset({"flying", "haste"})
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ATTACKS
    assert trig.predicate == IsSelf()
    assert trig.effect == Surveil(amount=2)
    assert trig.tier_threshold == 5


def test_build_spacecraft_printing_tier_gated_trigger_parity():
    """Per-card parity: the parser-built printing for a tier-gated
    trigger equals the static-registry analogue."""
    card = {
        "name": "Hand-Crafted Tier-Gated Trigger Parity Spacecraft",
        "mana_cost": "{2}{U}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "4",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 5+.)\n"
            "3+ | When this Spacecraft enters, surveil 2.\n"
            "5+ | Flying"
        ),
    }
    parsed = _build_spacecraft_printing(card)
    expected = make_spacecraft(
        "Hand-Crafted Tier-Gated Trigger Parity Spacecraft", "{2}{U}",
        2, 4,
        station_threshold=5,
        stationed_keywords={"flying"},
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=Surveil(amount=2),
                tier_threshold=3,
            ),
        ),
    )
    assert parsed == expected


def test_build_spacecraft_printing_tier_gated_plus_standalone_trigger():
    """A card with both a tier-gated trigger (lower-tier line) and a
    standalone non-tier-gated trigger (separate line, not on a tier)
    builds both. The non-tier-gated trigger keeps
    ``tier_threshold=0``; the tier-gated one carries its tier
    threshold. Pins that the two arms compose, and that the standalone
    trigger lands first in deterministic order regardless of where the
    tier-line trigger is printed in the oracle text."""
    card = {
        "name": "Hand-Crafted Mixed-Trigger Spacecraft",
        "mana_cost": "{4}{B}",
        "type_line": "Artifact — Spacecraft",
        "power": "4",
        "toughness": "4",
        "oracle_text": (
            "When this Spacecraft enters, surveil 2.\n"
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 6+.)\n"
            "3+ | Whenever this Spacecraft attacks, surveil 2.\n"
            "6+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 6
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 2
    by_event = {t.event: t for t in printing.triggered_abilities}
    enters = by_event[Event.ENTERS]
    attacks = by_event[Event.ATTACKS]
    assert enters.tier_threshold == 0
    assert enters.effect == Surveil(amount=2)
    assert attacks.tier_threshold == 3
    assert attacks.effect == Surveil(amount=2)
    # Non-tier-gated trigger sorts first in the deterministic
    # ordering, ahead of the tier-gated one.
    assert printing.triggered_abilities[0].tier_threshold == 0
    assert printing.triggered_abilities[1].tier_threshold == 3


def test_build_spacecraft_printing_skips_tier_gated_unparseable_effect():
    """A tier-gated trigger whose body parses as a recognized trigger
    phrase but whose effect phrase is outside the unified effect
    grammar (here a synthetic ``draw a card for each blue creature
    you control`` body — no per-blue-creature primitive lives in the
    grammar today) falls through ``_parse_clause`` and the whole card
    skips. Locks in that the tier-gated arm never silently drops the
    effect — same fall-through invariant the standalone-trigger arm
    holds. The previous version of this test used Infinite Guideline
    Station's ``draw a card for each multicolored permanent you
    control`` body as the unparseable phrase; that surface now claims
    through the unified grammar (de-bespoke batch 4), so the synthetic
    was swapped to a still-unparseable shape."""
    card = {
        "name": "Hand-Crafted Tier-Gated Unparseable Spacecraft",
        "mana_cost": "{4}{U}",
        "type_line": "Artifact — Spacecraft",
        "power": "4",
        "toughness": "5",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 7+.)\n"
            "3+ | Whenever this Spacecraft attacks, draw a card for "
            "each blue creature you control.\n"
            "7+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_claims_tier_gated_anthem_static_body():
    """A tier-line body that parses as an anthem-scope static
    (``CREATURES_YOU_CONTROL`` / ``OTHER_CREATURES_YOU_CONTROL``) is
    claimed: the static gets ``tier_threshold=N`` stamped on it and
    lands in ``static_abilities``. Lumen-Class Frigate's
    ``2+ | Other creatures you control get +1/+1.`` is the EoE printing
    in this shape — pins the slice-5 grammar extension that admits
    anthem statics alongside the tier-gated trigger arm.

    Self-scope statics (``As long as ..., this creature gets +N/+N.``)
    remain rejected because their faithfulness story still needs
    engine work; ``test_build_spacecraft_printing_skips_tier_gated_self_static_body``
    pins that boundary."""
    from mtgz_ir.cards.base import StaticAbility, StaticScope
    card = {
        "name": "Hand-Crafted Tier-Gated Static Spacecraft",
        "mana_cost": "{1}{W}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 6+.)\n"
            "2+ | Other creatures you control get +1/+1.\n"
            "6+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 6
    assert printing.stationed_keywords == frozenset({"flying"})
    assert printing.triggered_abilities == ()
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, PowerToughnessBonus,
    )
    # PR-D pilot: pure P/T anthems are now ContinuousModifier-shaped;
    # the tier_threshold rider stamps onto the same StaticAbility,
    # carried alongside the continuous_effect.
    assert printing.static_abilities == (
        StaticAbility(
            scope=StaticScope.OTHER_CREATURES_YOU_CONTROL,
            tier_threshold=2,
            continuous_effect=ContinuousModifier(
                layer=7,
                payload=PowerToughnessBonus(
                    power_bonus=1, toughness_bonus=1,
                ),
            ),
        ),
    )


def test_build_spacecraft_printing_skips_tier_gated_self_static_body():
    """Defensive: a tier-line body that parses as a self-scope static
    (``As long as ..., this creature gets +N/+N.``) is rejected — the
    slice-5 anthem-static arm is restricted to anthem scopes only,
    because the engine's ``StaticAbility.applies_to`` semantics for
    SELF-scoped grants on an unanimated Spacecraft are the unresolved
    case the slice-4 docstring reasoning is about. Pins the boundary
    so a future card with this shape doesn't half-claim."""
    card = {
        "name": "Hand-Crafted Tier-Gated Self-Static Spacecraft",
        "mana_cost": "{2}{R}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 6+.)\n"
            "2+ | This creature gets +2/+2 as long as you control "
            "an artifact.\n"
            "6+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_tier_gated_non_self_trigger():
    """Defensive: a tier-line trigger body that parses as a non-self
    trigger (here ``Whenever a land you control enters``) is rejected
    even though the effect parses, because tier-gated body triggers
    only make sense for self-targeted Spacecraft body shapes."""
    card = {
        "name": "Hand-Crafted Tier-Gated Landfall Spacecraft",
        "mana_cost": "{2}{G}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 5+.)\n"
            "3+ | Whenever a land you control enters, surveil 2.\n"
            "5+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_tier_gated_other_event():
    """Defensive: a tier-line self-trigger on an event other than
    ENTERS or ATTACKS is rejected — same restriction the standalone-
    trigger arm enforces. Here a synthetic ``becomes tapped`` self-
    trigger parses through the unified grammar but isn't a Spacecraft
    body shape we claim."""
    card = {
        "name": "Hand-Crafted Tier-Gated Tap Spacecraft",
        "mana_cost": "{2}{U}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 5+.)\n"
            "3+ | Whenever this creature becomes tapped, surveil 2.\n"
            "5+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_two_tier_gated_same_event_same_tier():
    """Two tier-gated trigger lines on the same event at the same
    threshold would silently drop one — bail. (Distinct thresholds
    on the same event are allowed; that's exercised by the multi-
    tier-gated trigger composition test below.)"""
    card = {
        "name": "Hand-Crafted Dup-Tier-Gated Spacecraft",
        "mana_cost": "{3}{B}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "3",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 5+.)\n"
            "3+ | When this Spacecraft enters, surveil 2.\n"
            "3+ | When this Spacecraft enters, surveil 2.\n"
            "5+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_claims_two_tier_gated_distinct_thresholds():
    """Two tier-gated triggers on the same event at *different*
    thresholds compose: each lands as a separate ``TriggeredAbility``
    with its own ``tier_threshold``. The engine fires whichever ones
    pass their gate independently."""
    card = {
        "name": "Hand-Crafted Multi-Tier-Gated Spacecraft",
        "mana_cost": "{3}{U}",
        "type_line": "Artifact — Spacecraft",
        "power": "3",
        "toughness": "5",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 8+.)\n"
            "3+ | Whenever this Spacecraft attacks, surveil 2.\n"
            "5+ | Whenever this Spacecraft attacks, surveil 2.\n"
            "8+ | Flying"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert printing.station_threshold == 8
    assert printing.stationed_keywords == frozenset({"flying"})
    assert len(printing.triggered_abilities) == 2
    thresholds = sorted(t.tier_threshold for t in printing.triggered_abilities)
    assert thresholds == [3, 5]
    for t in printing.triggered_abilities:
        assert t.event == Event.ATTACKS
        assert t.predicate == IsSelf()
        assert t.effect == Surveil(amount=2)


def test_build_spacecraft_printing_skips_tier_gated_unknown_clause():
    """A tier-line body that fails the keyword check AND fails the
    unified clause parser (``_parse_clause`` returns ``None``) skips
    the whole card. Locks in the bail-on-unknown invariant for the
    tier-gated arm."""
    card = {
        "name": "Hand-Crafted Tier-Gated Garbage Spacecraft",
        "mana_cost": "{2}",
        "type_line": "Artifact — Spacecraft",
        "power": "2",
        "toughness": "2",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 4+.)\n"
            "2+ | this is not any recognized clause body shape\n"
            "4+ | Flying"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_skips_multi_tier_keyword_card():
    """Two pure-keyword tier lines on the same card (synthetic shape:
    the engine has no path to apply lower-tier keyword grants on a
    Spacecraft, see module docstring) skips the whole card. Pins the
    explicit bail-out so a future printing with this shape doesn't
    half-claim by accident.

    (The slice-1 ``test_build_spacecraft_printing_skips_multi_tier_card``
    test pins the same shape with the lower tier carrying a non-keyword
    body — this test exercises the strictly-keyword-on-both-tiers case.)
    """
    card = {
        "name": "Hand-Crafted Multi-Tier-Keyword Spacecraft",
        "mana_cost": "{3}{R}",
        "type_line": "Artifact — Spacecraft",
        "power": "4",
        "toughness": "4",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 8+.)\n"
            "5+ | Flying\n"
            "8+ | Flying, haste"
        ),
    }
    assert _build_spacecraft_printing(card) is None


def test_build_spacecraft_printing_legendary_with_tier_gated_trigger():
    """A Legendary Spacecraft with a tier-gated ETB trigger forwards
    the ``Legendary`` supertype through ``make_spacecraft(
    legendary=True)`` just like the single-tier Legendary case. Pins
    that the new tier-gated arm composes with the Legendary supertype
    forwarding."""
    card = {
        "name": "Hand-Crafted Legendary Tier-Gated Spacecraft",
        "mana_cost": "{2}{W}{U}",
        "type_line": "Legendary Artifact — Spacecraft",
        "power": "4",
        "toughness": "5",
        "oracle_text": (
            "Station (Tap another creature you control: Put charge "
            "counters equal to its power on this Spacecraft. Station "
            "only as a sorcery. It's an artifact creature at 7+.)\n"
            "4+ | When this Spacecraft enters, surveil 2.\n"
            "7+ | Flying, lifelink"
        ),
    }
    printing = _build_spacecraft_printing(card)
    assert printing is not None
    assert "Legendary" in printing.supertypes
    assert printing.station_threshold == 7
    assert printing.stationed_keywords == frozenset({"flying", "lifelink"})
    assert len(printing.triggered_abilities) == 1
    trig = printing.triggered_abilities[0]
    assert trig.event == Event.ENTERS
    assert trig.predicate == IsSelf()
    assert trig.effect == Surveil(amount=2)
    assert trig.tier_threshold == 4


def test_source_still_claims_existing_spacecraft():
    """The slice-1/2/3 real-card claims (Galvanizing Sawship,
    Wedgelight Rammer, Wurmwall Sweeper) must continue to be claimed
    after the slice-4 grammar extension. Acts as a regression gate:
    the new tier-gated dispatch must not introduce a fall-through
    that breaks the simpler shapes."""
    source = OracleParserCardSource()
    claimed = set(source.claimed_names)
    assert "Galvanizing Sawship" in claimed
    assert "Wedgelight Rammer" in claimed
    assert "Wurmwall Sweeper" in claimed


def test_source_claims_real_multi_tier_spacecraft():
    """Lumen-Class Frigate, Synthesizer Labship, Entropic Battlecruiser,
    and Dawnsire, Sunstar Dreadnought are all claimed by the unified
    oracle-text grammar (not via the bespoke dispatch table). Lumen-Class
    Frigate was claimed by slice 5 (tier-gated anthem static); the other
    three were migrated from the bespoke table by the current batch after
    the effect grammar was extended to cover their respective shapes."""
    source = OracleParserCardSource()
    claimed = set(source.claimed_names)
    assert "Lumen-Class Frigate" in claimed
    assert "Synthesizer Labship" in claimed
    assert "Entropic Battlecruiser" in claimed
    assert "Dawnsire, Sunstar Dreadnought" in claimed
