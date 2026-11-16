"""Parser support for pure-Artifact (non-Creature, non-Equipment, non-
Spacecraft) cards.

The first artifact shape the parser claims is the simplest end-to-end
Edge of Eternities pattern: a typed-self ETB trigger plus a printed
activated ability, both of which the unified ``_parse_clause``
orchestrator already recognizes. Melded Moxite is the only EoE
pure-artifact whose oracle text fits end-to-end today; every other
pure artifact carries a custom effect (Pinnacle Starcage, Requiem
Monolith, Memorial Vault, All-Fates Scroll, The Endstone, Thrumming
Hivepool) or a grammar shape outside the unified parser
(Cryogen Relic's "enters or leaves" composite trigger, Nutrient
Block's "is put into a graveyard" zone-change trigger,
Dubious Delicacy's negative-pump ETB).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CreateToken,
    TriggeredAbility,
    make_artifact,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.artifact_builder import _build_artifact_printing
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.combinators import IfYouDo, MayDo
from mtgz_ir.effects.generic import DiscardSelf, DrawCards, GainLife
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


_MELDED_MOXITE_CARD = {
    "name": "Melded Moxite",
    "mana_cost": "{1}{R}",
    "type_line": "Artifact",
    "oracle_text": (
        "When this artifact enters, you may discard a card. If you "
        "do, draw two cards.\n"
        "{3}, Sacrifice this artifact: Create a tapped 2/2 colorless "
        "Robot artifact creature token."
    ),
}


def test_build_artifact_printing_claims_melded_moxite():
    """The Scryfall-shaped Melded Moxite record builds a red Artifact
    with the typed-self ETB trigger (optional discard then draw two)
    and the {3}, sac activated token-creating ability - the same
    arguments the static registry passes to ``make_artifact`` for
    this card."""
    printing = _build_artifact_printing(_MELDED_MOXITE_CARD)
    assert printing is not None
    assert printing.name == "Melded Moxite"
    assert "Artifact" in printing.card_types
    assert "Creature" not in printing.card_types
    assert "Legendary" not in printing.supertypes
    # ETB trigger uses the canonical IsSelf-ENTERS predicate path with
    # the IfYouDo(MayDo(DiscardSelf(1)), DrawCards(2)) compound — the
    # generic combinators that compose "you may discard a card. If you
    # do, draw two cards."
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=IfYouDo(
                do_action=MayDo(DiscardSelf(amount=1)),
                then_action=DrawCards(amount=2),
            ),
        ),
    )
    # The {3}, Sacrifice this artifact activated ability creates a
    # tapped 2/2 Robot token.
    assert len(printing.activated_abilities) == 1
    assert printing.activated_abilities[0].cost.sacrifice_self
    assert printing.activated_abilities[0].cost.mana_str == "{3}"


def test_build_artifact_printing_matches_static_make_artifact():
    """The parser-built printing must equal what the static registry
    produces via ``make_artifact(...)`` - required by the parity gate,
    asserted directly here for fast feedback."""
    parsed = _build_artifact_printing(_MELDED_MOXITE_CARD)
    expected = make_artifact(
        "Melded Moxite", "{1}{R}",
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=IfYouDo(
                do_action=MayDo(DiscardSelf(amount=1)),
                then_action=DrawCards(amount=2),
            ),
            ),
        ),
        activated_abilities=(
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost("{3}"), mana_str="{3}",
                    sacrifice_self=True,
                ),
                effect=CreateToken(
                    token_printing=TOKEN_PRINTINGS["Robot"],
                    count=1,
                    tapped=True,
                ),
            ),
        ),
    )
    assert parsed == expected


def test_build_artifact_printing_supports_keyword_only_artifact():
    """A pure-keyword artifact (``Indestructible`` alone) builds a
    keyword-only artifact - keyword lines route into the ``keywords``
    bucket on ``make_artifact``."""
    card = {
        "name": "Hand-Crafted Indestructible Brick",
        "mana_cost": "{1}",
        "type_line": "Artifact",
        "oracle_text": "Indestructible",
    }
    printing = _build_artifact_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset({"indestructible"})
    assert printing.triggered_abilities == ()
    assert printing.activated_abilities == ()


def test_build_artifact_printing_supports_legendary_artifact():
    """Legendary pure artifacts route through ``make_artifact(
    legendary=True)`` - the supertype must be preserved on the
    printing rather than silently dropped."""
    card = {
        "name": "Hand-Crafted Legendary Indestructible Relic",
        "mana_cost": "{2}",
        "type_line": "Legendary Artifact",
        "oracle_text": "Indestructible",
    }
    printing = _build_artifact_printing(card)
    assert printing is not None
    assert "Legendary" in printing.supertypes
    assert printing.type_line == "Legendary Artifact"


def test_build_artifact_printing_supports_activated_only_artifact():
    """An artifact whose only oracle line is an activated ability
    (tap-cost gain-life) builds with the activated ability and
    nothing else."""
    card = {
        "name": "Hand-Crafted Life Artifact",
        "mana_cost": "{2}",
        "type_line": "Artifact",
        "oracle_text": (
            "{2}, {T}, Sacrifice this artifact: You gain 3 life."
        ),
    }
    printing = _build_artifact_printing(card)
    assert printing is not None
    assert printing.triggered_abilities == ()
    assert printing.keywords == frozenset()
    assert len(printing.activated_abilities) == 1
    assert printing.activated_abilities[0].effect == GainLife(amount=3)


def test_build_artifact_printing_supports_empty_oracle_text():
    """An artifact with no printed oracle text (a pure vanilla artifact)
    builds cleanly with empty triggered/activated/static buckets."""
    card = {
        "name": "Hand-Crafted Vanilla Artifact",
        "mana_cost": "{3}",
        "type_line": "Artifact",
        "oracle_text": "",
    }
    printing = _build_artifact_printing(card)
    assert printing is not None
    assert printing.triggered_abilities == ()
    assert printing.activated_abilities == ()
    assert printing.static_abilities == ()
    assert printing.keywords == frozenset()


def test_build_artifact_printing_skips_non_artifact():
    """A non-artifact card (a sorcery, instant, enchantment) must not
    match - the type-line check requires ``Artifact`` in card_types."""
    card = {
        "name": "Hand-Crafted Sorcery",
        "mana_cost": "{1}",
        "type_line": "Sorcery",
        "oracle_text": "Indestructible",
    }
    assert _build_artifact_printing(card) is None


def test_build_artifact_printing_skips_artifact_creature():
    """Artifact creatures (e.g. Robots) are claimed by the dedicated
    creature builder - this builder must defer to it rather than
    silently dropping the printed P/T."""
    card = {
        "name": "Hand-Crafted Artifact Creature",
        "mana_cost": "{2}",
        "type_line": "Artifact Creature — Robot",
        "power": "2",
        "toughness": "2",
        "oracle_text": "",
    }
    assert _build_artifact_printing(card) is None


def test_build_artifact_printing_skips_equipment():
    """Equipment is claimed by the dedicated equipment builder with
    its own ``Equipped creature ...`` / ``Equip {N}`` grammar - this
    builder must defer to it even if the body happened to be empty."""
    card = {
        "name": "Hand-Crafted Equipment",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": "Equipped creature gets +1/+1.\nEquip {2}",
    }
    assert _build_artifact_printing(card) is None


def test_build_artifact_printing_skips_spacecraft():
    """Spacecraft is claimed by the dedicated spacecraft builder with
    its own ``Station`` / tier-line grammar - this builder must defer
    to it."""
    card = {
        "name": "Hand-Crafted Spacecraft",
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
    assert _build_artifact_printing(card) is None


def test_build_artifact_printing_skips_unrecognized_line():
    """An artifact whose body contains a clause outside the unified
    grammar AND outside the literal-line bespoke-effect tables skips
    silently - same fall-through behavior as the other builders. We
    must never half-claim a printing by silently dropping abilities the
    parser doesn't model yet.

    (Pinnacle Starcage's exact oracle text IS literal-matched today,
    so a parallel hand-crafted card uses a structurally similar but
    distinct surface form to exercise the fall-through path.)"""
    card = {
        "name": "Hand-Crafted Custom-Exile Artifact",
        "mana_cost": "{1}{W}{W}",
        "type_line": "Artifact",
        "oracle_text": (
            "When this artifact enters, exile every nonland permanent "
            "with mana value 4 or greater until end of turn."
        ),
    }
    assert _build_artifact_printing(card) is None


def test_build_artifact_printing_strips_reminder_text():
    """Reminder text on its own line (a parenthetical with no rules
    consequence) reduces to empty after ``_strip_reminder_text`` and
    contributes nothing rather than failing the whole card."""
    card = {
        "name": "Hand-Crafted Reminder-Heavy Artifact",
        "mana_cost": "{1}",
        "type_line": "Artifact",
        "oracle_text": (
            "Indestructible\n"
            "(this is a flavor parenthetical with no rules text)"
        ),
    }
    printing = _build_artifact_printing(card)
    assert printing is not None
    assert printing.keywords == frozenset({"indestructible"})


def test_build_artifact_printing_routes_static_into_static_bucket():
    """A line that the unified parser classifies as a ``StaticAbility``
    routes into ``make_artifact(static_abilities=...)`` rather than
    being silently dropped. Pure artifacts don't typically print
    static lines (the ``As long as ... this creature gets ...``
    grammar is creature-targeted), but the dispatch arm exists for
    any static the unified grammar later learns to recognize on a
    non-creature permanent."""
    card = {
        "name": "Hand-Crafted Static Artifact",
        "mana_cost": "{2}",
        "type_line": "Artifact",
        "oracle_text": (
            "As long as you control an artifact, this creature gets "
            "+1/+1."
        ),
    }
    printing = _build_artifact_printing(card)
    assert printing is not None
    # PR-D continuous-effects shape: P/T contribution rides on a
    # layer-7c ContinuousModifier(PowerToughnessBonus). The legacy
    # power_bonus field is zero on migrated statics — read from the
    # continuous_effect payload instead.
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, PowerToughnessBonus,
    )
    assert len(printing.static_abilities) == 1
    sa = printing.static_abilities[0]
    assert sa.continuous_effect == ContinuousModifier(
        layer=7,
        payload=PowerToughnessBonus(power_bonus=1, toughness_bonus=1),
    )


def test_source_claims_melded_moxite():
    """End-to-end: ``OracleParserCardSource`` walks the cached EoE
    Scryfall data and ends up claiming Melded Moxite via the new
    artifact-builder dispatch arm."""
    source = OracleParserCardSource()
    assert "Melded Moxite" in source.claimed_names
    assert "Melded Moxite" in source.get_printings()
