"""Parser support for Artifact — Equipment cards.

The first equipment shape the parser claims is the simplest end-to-end
Edge of Eternities pattern: an ETB attach trigger, a single
``Equipped creature gets +N/+M [and has <keyword>]`` static line, and
an ``Equip {N}`` line. Meltstrider's Gear is the canonical example
this batch claims; Hylderblade is included as a "static + Equip with
no ETB" minimal happy-path; the other five EoE Equipment cards stay
out of scope (Void rider, Flash + UEOT keyword grants, custom attack
trigger, create-token-and-attach, granted-quoted-activated-ability).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    TriggeredAbility,
    make_equipment,
)
from mtgz_ir.cards.parser.equipment_builder import _build_equipment_printing
from mtgz_ir.cards.parser_source import OracleParserCardSource
from mtgz_ir.effects.generic import AttachToTargetCreatureYouControl
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


_MELTSTRIDERS_GEAR_CARD = {
    "name": "Meltstrider's Gear",
    "mana_cost": "{G}",
    "type_line": "Artifact — Equipment",
    "oracle_text": (
        "When this Equipment enters, attach it to target creature you "
        "control.\n"
        "Equipped creature gets +2/+1 and has reach.\n"
        "Equip {5} ({5}: Attach to target creature you control. "
        "Equip only as a sorcery.)"
    ),
}


def test_build_equipment_printing_claims_meltstriders_gear():
    """The Scryfall-shaped Meltstrider's Gear record builds a green
    Equipment with the ETB attach trigger, +2/+1 reach bonus, and
    ``Equip {5}`` cost (parenthesized reminder text dropped)."""
    printing = _build_equipment_printing(_MELTSTRIDERS_GEAR_CARD)
    assert printing is not None
    assert printing.name == "Meltstrider's Gear"
    assert "Equipment" in printing.subtypes
    assert "Artifact" in printing.card_types
    # Equipment_bonuses is the (power, toughness, granted_keywords) trio
    # that the engine reads when computing effective_* on the equipped
    # creature.
    assert printing.equipment_bonuses == (2, 1, frozenset({"reach"}))
    # ETB trigger uses the canonical IsSelf-ENTERS predicate path with
    # the AttachToTargetCreatureYouControl effect — the same shape the
    # static side already wires for this card.
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=AttachToTargetCreatureYouControl(),
        ),
    )
    # The synthesized Equip {5} ability is appended to activated_abilities
    # by make_equipment; we don't introspect its internals here, only
    # verify that activated abilities exist (== 1, the synthesized one).
    assert len(printing.activated_abilities) == 1


def test_build_equipment_printing_matches_static_make_equipment():
    """The parser-built printing must equal what the static registry
    produces via ``make_equipment(...)`` — required by the parity gate,
    asserted directly here for fast feedback."""
    parsed = _build_equipment_printing(_MELTSTRIDERS_GEAR_CARD)
    expected = make_equipment(
        "Meltstrider's Gear", "{G}",
        power_bonus=2, toughness_bonus=1,
        granted_keywords=frozenset({"reach"}),
        triggered_abilities=(
            TriggeredAbility(
                event=Event.ENTERS, predicate=IsSelf(),
                effect=AttachToTargetCreatureYouControl(),
            ),
        ),
        equip_cost="{5}",
    )
    assert parsed == expected


def test_build_equipment_printing_supports_no_etb_no_granted_keywords():
    """Smallest happy-path shape: a single ``Equipped creature gets
    +N/+M`` static line plus ``Equip {N}``, no ETB trigger and no
    granted keywords. The builder accepts it and returns an Equipment
    with empty triggered_abilities and empty granted_keywords."""
    card = {
        "name": "Hand-Crafted Bare Blade",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +3/+1.\n"
            "Equip {2}"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    assert printing.equipment_bonuses == (3, 1, frozenset())
    assert printing.triggered_abilities == ()
    assert len(printing.activated_abilities) == 1


def test_build_equipment_printing_supports_granted_only_line():
    """``Equipped creature has <keyword>`` standalone line — granted
    keyword without a P/T bonus. Composes cleanly with a separate
    ``Equipped creature gets +N/+M`` line on the same card."""
    card = {
        "name": "Hand-Crafted Wing Strap",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +1/+1.\n"
            "Equipped creature has flying.\n"
            "Equip {1}"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    assert printing.equipment_bonuses == (1, 1, frozenset({"flying"}))


def test_build_equipment_printing_supports_multiple_granted_keywords():
    """The trailing ``and has <kw>, <kw>...`` rider is comma-joined
    and may grant multiple keywords at once."""
    card = {
        "name": "Hand-Crafted Twin Bonus",
        "mana_cost": "{2}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +2/+2 and has flying, trample.\n"
            "Equip {3}"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    assert printing.equipment_bonuses == (
        2, 2, frozenset({"flying", "trample"})
    )


def test_build_equipment_printing_skips_non_equipment_artifact():
    """A plain ``Artifact`` (no ``Equipment`` subtype) must not match
    even if it happens to have an ``Equipped creature ...`` line in
    its oracle text."""
    card = {
        "name": "Hand-Crafted Artifact",
        "mana_cost": "{1}",
        "type_line": "Artifact",
        "oracle_text": "Equipped creature gets +1/+0.\nEquip {2}",
    }
    assert _build_equipment_printing(card) is None


def test_build_equipment_printing_skips_non_artifact():
    """A non-artifact card (e.g. a creature) must not match — the
    type-line check requires ``Artifact`` in card_types."""
    card = {
        "name": "Hand-Crafted Creature",
        "mana_cost": "{1}",
        "type_line": "Creature — Human",
        "oracle_text": "Equipped creature gets +1/+0.\nEquip {2}",
    }
    assert _build_equipment_printing(card) is None


def test_build_equipment_printing_claims_legendary_equipment():
    """Legendary Equipment with a body the grammar otherwise claims is
    routed through ``make_equipment(legendary=True)``; the supertype
    survives onto the produced ``CardPrinting``."""
    card = {
        "name": "Hand-Crafted Legendary Blade",
        "mana_cost": "{2}",
        "type_line": "Legendary Artifact — Equipment",
        "oracle_text": "Equipped creature gets +1/+1.\nEquip {1}",
    }
    result = _build_equipment_printing(card)
    assert result is not None
    assert "Legendary" in result.supertypes


def test_build_equipment_printing_skips_when_equip_cost_missing():
    """An Equipment with no ``Equip {N}`` line at all is malformed for
    this builder — return None rather than emit an Equipment with no
    Equip activated ability."""
    card = {
        "name": "Hand-Crafted No-Equip",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": "Equipped creature gets +1/+0.",
    }
    assert _build_equipment_printing(card) is None


def test_build_equipment_printing_skips_unknown_granted_keyword():
    """A granted-keyword rider that names a keyword not in the
    evergreen whitelist (``_KEYWORD_LINES``) skips the whole card — we
    must never grant a keyword the engine doesn't model, even silently.
    """
    card = {
        "name": "Hand-Crafted Unknown Keyword",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +1/+0 and has fakeword.\n"
            "Equip {2}"
        ),
    }
    assert _build_equipment_printing(card) is None


def test_build_equipment_printing_skips_unrecognized_line():
    """An Equipment whose body contains a clause outside this builder's
    grammar (here a Void rider) skips silently — same fall-through
    behavior as the creature/spell/land builders."""
    card = {
        "name": "Hand-Crafted Void Equipment",
        "mana_cost": "{B}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +3/+1.\n"
            "Void — At the beginning of your end step, do "
            "something complex.\n"
            "Equip {4}"
        ),
    }
    assert _build_equipment_printing(card) is None


def test_build_equipment_printing_skips_double_equip_line():
    """Defensive: a card with two ``Equip {N}`` lines is malformed
    oracle text — bail rather than silently overwrite the cost."""
    card = {
        "name": "Hand-Crafted Double Equip",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +1/+0.\n"
            "Equip {2}\n"
            "Equip {3}"
        ),
    }
    assert _build_equipment_printing(card) is None


def test_build_equipment_printing_skips_unknown_keyword_in_has_line():
    """A standalone ``Equipped creature has <kw>`` line with a keyword
    not in the evergreen whitelist also skips the whole card — same
    safety as the trailing-rider variant."""
    card = {
        "name": "Hand-Crafted Has Unknown",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +1/+0.\n"
            "Equipped creature has fakeword.\n"
            "Equip {2}"
        ),
    }
    assert _build_equipment_printing(card) is None


def test_build_equipment_printing_ignores_blank_and_reminder_only_lines():
    """Blank lines (after reminder-text stripping) are skipped silently —
    a parenthetical-only line like ``"(reminder)"`` reduces to empty
    and contributes nothing rather than failing the whole card."""
    card = {
        "name": "Hand-Crafted Reminder-Only Line",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +1/+0.\n"
            "(this is a flavor parenthetical with no rules text)\n"
            "Equip {2}"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    assert printing.equipment_bonuses == (1, 0, frozenset())


def test_source_claims_meltstriders_gear():
    """End-to-end: ``OracleParserCardSource`` walks the cached EoE
    Scryfall data and ends up claiming Meltstrider's Gear via the
    new equipment-builder dispatch arm."""
    source = OracleParserCardSource()
    assert "Meltstrider's Gear" in source.claimed_names
    assert "Meltstrider's Gear" in source.get_printings()


# Parser support for the second-batch Equipment shapes — Flash on the
# Equipment itself, ETB attach with a UEOT-keyword grant rider, and the
# ETB create-token-and-attach variant. These three shapes together cover
# Squire's Lightblade, Illvoi Light Jammer, and Auxiliary Boosters.

def test_build_equipment_printing_supports_flash_keyword_line():
    """A standalone evergreen-keyword line on the Equipment itself
    (printed as ``Flash``) is forwarded through ``keywords=...`` —
    the equipment can then be cast at instant speed by the engine."""
    card = {
        "name": "Hand-Crafted Flash Blade",
        "mana_cost": "{1}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Flash\n"
            "Equipped creature gets +1/+0.\n"
            "Equip {2}"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    assert "flash" in printing.keywords
    assert printing.equipment_bonuses == (1, 0, frozenset())


def test_build_equipment_printing_supports_attach_with_ueot_grant_rider():
    """The ETB attach trigger may carry a ``"That creature gains
    <keyword> until end of turn."`` rider that the builder routes into
    ``AttachToTargetCreatureYouControl(granted_keywords_ueot=...)``."""
    card = {
        "name": "Hand-Crafted UEOT Blade",
        "mana_cost": "{W}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "When this Equipment enters, attach it to target creature "
            "you control. That creature gains first strike until end "
            "of turn.\n"
            "Equipped creature gets +1/+0.\n"
            "Equip {3}"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=AttachToTargetCreatureYouControl(
                granted_keywords_ueot=frozenset({"first strike"}),
            ),
        ),
    )


def test_build_equipment_printing_skips_attach_ueot_unknown_keyword():
    """An attach-UEOT rider naming a keyword outside the evergreen
    whitelist skips the whole card — same safety as the static
    ``and has <kw>`` rider."""
    card = {
        "name": "Hand-Crafted Bad UEOT Blade",
        "mana_cost": "{W}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "When this Equipment enters, attach it to target creature "
            "you control. That creature gains fakeword until end of "
            "turn.\n"
            "Equipped creature gets +1/+0.\n"
            "Equip {3}"
        ),
    }
    assert _build_equipment_printing(card) is None


def test_build_equipment_printing_supports_create_robot_token_attach():
    """The Auxiliary Boosters ETB shape — create a 2/2 Robot token and
    attach the Equipment to it — is recognized as a literal-string
    line and wired through ``CreateTokenAndAttachSelf``."""
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
    from mtgz_ir.effects.generic import CreateTokenAndAttachSelf

    card = {
        "name": "Hand-Crafted Robot Boosters",
        "mana_cost": "{4}{W}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "When this Equipment enters, create a 2/2 colorless Robot "
            "artifact creature token and attach this Equipment to it.\n"
            "Equipped creature gets +1/+2 and has flying.\n"
            "Equip {3}"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=CreateTokenAndAttachSelf(
                token_printing=TOKEN_PRINTINGS["Robot"],
            ),
        ),
    )
    assert printing.equipment_bonuses == (1, 2, frozenset({"flying"}))


def test_source_claims_squires_lightblade():
    """End-to-end: Squire's Lightblade is claimed via the Flash + ETB
    attach + UEOT grant dispatch arms."""
    source = OracleParserCardSource()
    assert "Squire's Lightblade" in source.claimed_names


def test_source_claims_illvoi_light_jammer():
    """End-to-end: Illvoi Light Jammer is claimed via Flash + ETB
    attach + UEOT hexproof rider."""
    source = OracleParserCardSource()
    assert "Illvoi Light Jammer" in source.claimed_names


def test_source_claims_auxiliary_boosters():
    """End-to-end: Auxiliary Boosters is claimed via the
    create-Robot-token-and-attach ETB dispatch arm."""
    source = OracleParserCardSource()
    assert "Auxiliary Boosters" in source.claimed_names


def test_build_equipment_printing_supports_hylderblade_void_rider():
    """Hylderblade's literal ``"Void — At the beginning of your end
    step, ..., attach this Equipment to target creature you control."``
    line builds a ``BEGINNING_OF_END_STEP``-event triggered ability
    with ``requires_void=True`` — the engine's stand-in for the EoE
    Void intervening-if check."""
    from mtgz_ir.effects.predicates import (
        Controlled,
        YOU,
    )

    card = {
        "name": "Hylderblade",
        "mana_cost": "{B}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +3/+1.\n"
            "Void — At the beginning of your end step, if a nonland "
            "permanent left the battlefield this turn or a spell was "
            "warped this turn, attach this Equipment to target "
            "creature you control.\n"
            "Equip {4} ({4}: Attach to target creature you control. "
            "Equip only as a sorcery.)"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    assert printing.equipment_bonuses == (3, 1, frozenset())
    assert printing.triggered_abilities == (
        TriggeredAbility(
            event=Event.BEGINNING_OF_END_STEP,
            predicate=Controlled(YOU),
            requires_void=True,
            effect=AttachToTargetCreatureYouControl(),
        ),
    )


def test_source_claims_hylderblade():
    """End-to-end: Hylderblade is claimed via the literal Void
    end-step attach dispatch arm."""
    source = OracleParserCardSource()
    assert "Hylderblade" in source.claimed_names


def test_build_equipment_printing_supports_astral_projection_activation():
    """Robe of Stars' ``Astral Projection — {1}{W}: Equipped creature
    phases out.`` line is parsed as an ``ActivatedAbility`` with the
    bespoke ``PhaseOutEquippedCreature`` effect. The flavor name on
    the prefix is captured by the regex but discarded (the rules text
    after the colon is what carries the behavior)."""
    from mtgz_ir.cards.base import ActivatedAbility, ActivationCost
    from mtgz_ir.cards.base import parse_mana_cost as _parse
    from mtgz_ir.effects.generic import PhaseOutEquippedCreature

    card = {
        "name": "Robe of Stars",
        "mana_cost": "{1}{W}",
        "type_line": "Artifact — Equipment",
        "oracle_text": (
            "Equipped creature gets +0/+3.\n"
            "Astral Projection — {1}{W}: Equipped creature phases out. "
            "(Treat it and anything attached to it as though they don't "
            "exist until its controller's next turn.)\n"
            "Equip {1}"
        ),
    }
    printing = _build_equipment_printing(card)
    assert printing is not None
    # Two activated abilities: Astral Projection (parser-built, index 0)
    # and the synthesized printed Equip {1} ability (appended last by
    # ``make_equipment(equip_cost=...)``).
    assert len(printing.activated_abilities) == 2
    astral = printing.activated_abilities[0]
    assert astral == ActivatedAbility(
        cost=ActivationCost(
            mana=_parse("{1}{W}"), mana_str="{1}{W}",
        ),
        effect=PhaseOutEquippedCreature(),
    )
    equip = printing.activated_abilities[1]
    assert equip.cost.mana_str == "{1}"
    assert equip.sorcery_speed


def test_source_claims_robe_of_stars():
    """End-to-end: Robe of Stars (Special Guests) is claimed via the
    Astral Projection activation dispatch arm."""
    source = OracleParserCardSource()
    assert "Robe of Stars" in source.claimed_names
