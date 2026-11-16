"""Parser grammar for the roto cards that change card types and colors
(CR 613 layers 4 and 5): Mycosynth Lattice, Liquimetal Coating, Unable to
Scream and Karn, the Great Creator (``docs/EFFECTIVE_CHARACTERISTICS_
DESIGN.md``).

Entry points, each the last fallback of a shared dispatcher (so no phrase
an older recognizer claims changes meaning), switched off for printings
that exist only in the EoE-era dumps through the shared
``creature_rmc_grammar`` guard:

* :data:`LAYER_LINE_RECOGNIZERS` -- whole oracle lines, appended to P5's
  ``roto_board._LINE_RECOGNIZERS`` (artifact / creature / cube
  enchantment builders): "All permanents are <type>s in addition to their
  other types.", the all-objects color line, "Players may spend mana as
  though it were mana of any color.", Unable to Scream's two Aura lines;
* :func:`parse_roto_layer_effect` -- effect phrases (after P6 in
  ``effects._parse_effect``): "target <np> becomes a[n] <types> ...",
  Karn's -2 wish-or-exile body;
* :func:`parse_all_objects_color_line` -- shared with the creature
  grammar's Painter's Servant line (one parameterized recognizer for both
  printed surfaces).

Karn's static ("Activated abilities of artifacts your opponents control
can't be activated.") is the activation-lock recognizer of the W/G creature
lines, widened with the controller scope (``creature_wg_lines.
_parse_activation_lock_line``) and reached from the planeswalker builder.
"""

from __future__ import annotations

import re

from mtgz_ir.effects.base import Effect


def _disabled() -> bool:
    from mtgz_ir.cards.parser.creature_rmc_grammar import _DISABLED

    return _DISABLED[0]


# Plural card-type words of a type-adding line -> the card type (only the
# types an effect may add: ``game.LAYER4_ADDABLE_CARD_TYPES``).
_ADDABLE_PLURAL = {"artifacts": "Artifact", "creatures": "Creature"}
_ADDABLE_SINGULAR = {"artifact": "Artifact", "creature": "Creature"}


# ---------------------------------------------------------------------------
# Whole lines
# ---------------------------------------------------------------------------

# "All <permanents> are <type>s in addition to their other types."
# (Mycosynth Lattice). The subject is captured, but only "permanents" is
# accepted: no other subject ("All Forests and all Saprolings are ...",
# Life and Limb) is in any pool, and each would need its own subject
# filter in ``game._layer4_added_card_types``.
_ALL_X_ARE_TYPE_RE = re.compile(
    r"^All (?P<subject>[a-z]+) are (?P<types>[a-z]+) in addition to their "
    r"other types\.$"
)


def _all_permanents_are_type(line: str):
    from mtgz_ir.cards.parser.roto_board import BoardLine

    m = _ALL_X_ARE_TYPE_RE.match(line)
    if m is None or m.group("subject") != "permanents":
        return None
    card_type = _ADDABLE_PLURAL.get(m.group("types"))
    if card_type is None:
        return None
    return BoardLine(fields={"all_permanents_gain_card_type": card_type})


# "All cards that aren't on the battlefield, spells, and permanents are
# <colorless | the chosen color in addition to their other colors>."
# (Mycosynth Lattice / Painter's Servant): the two printed surfaces of one
# shape, differing only in the color clause.
_ALL_OBJECTS_COLOR_RE = re.compile(
    r"^All cards that aren't on the battlefield, spells, and permanents are "
    r"(?P<color>colorless|the chosen color in addition to their other "
    r"colors)\.$"
)


def parse_all_objects_color_line(line: str) -> "dict | None":
    """Printing fields for the all-objects color line, or ``None``:
    ``all_objects_colorless`` (layer-5 set to no colors) or
    ``paints_all_objects_chosen_color_at_etb`` (layer-5 add the chosen
    color; the "As this enters, choose a color." line records it)."""
    m = _ALL_OBJECTS_COLOR_RE.match(line)
    if m is None:
        return None
    if m.group("color") == "colorless":
        return {"all_objects_colorless": True}
    return {"paints_all_objects_chosen_color_at_etb": True}


def _all_objects_color(line: str):
    from mtgz_ir.cards.parser.roto_board import BoardLine

    fields = parse_all_objects_color_line(line)
    if fields is None:
        return None
    return BoardLine(fields=fields)


# "Players may spend mana as though it were mana of any color." --
# literal: Mycosynth Lattice is the only card in any pool printing the
# player-wide form (Opposition Agent's "you may spend mana as though it
# were mana of any color to cast them" is a per-card permission, parsed
# with its own line). Generalize when a second player-wide card lands.
_PLAYERS_SPEND_MANA_AS_ANY_COLOR = (
    "Players may spend mana as though it were mana of any color."
)


def _players_spend_mana_as_any_color(line: str):
    from mtgz_ir.cards.parser.roto_board import BoardLine

    if line != _PLAYERS_SPEND_MANA_AS_ANY_COLOR:
        return None
    return BoardLine(fields={"mana_spendable_as_any_color": True})


# "Enchanted creature [loses all abilities and ]is a[n] [<Subtype> ]
# <artifact creature | artifact | creature> with base power and toughness
# P/T in addition to its other types." (Unable to Scream).
_ENCHANTED_IS_TYPE_RE = re.compile(
    r"^Enchanted creature (?:(?P<lose>loses all abilities) and )?is an? "
    r"(?:(?P<subtype>[A-Z][a-z]+) )?(?P<types>artifact creature|artifact|"
    r"creature) with base power and toughness (?P<p>\d+)/(?P<t>\d+) in "
    r"addition to its other types\.$"
)


def _enchanted_is_type(line: str):
    from mtgz_ir.cards.parser.roto_board import BoardLine

    m = _ENCHANTED_IS_TYPE_RE.match(line)
    if m is None:
        return None
    fields: dict = {
        "attached_grants_card_types": tuple(
            _ADDABLE_SINGULAR[w] for w in m.group("types").split()
        ),
        "attached_sets_base_pt": (int(m.group("p")), int(m.group("t"))),
    }
    if m.group("subtype"):
        fields["attached_grants_subtype"] = m.group("subtype")
    if m.group("lose"):
        fields["attached_grants_lose_all_abilities"] = True
    return BoardLine(fields=fields)


# "As long as enchanted creature is face down, it can't be turned face
# up." -- literal: Unable to Scream is the only card in any pool with a
# can't-be-turned-face-up restriction. Generalize when a second lands.
_FACE_DOWN_CANT_BE_TURNED_FACE_UP = (
    "As long as enchanted creature is face down, it can't be turned face up."
)


def _cant_be_turned_face_up(line: str):
    from mtgz_ir.cards.parser.roto_board import BoardLine

    if line != _FACE_DOWN_CANT_BE_TURNED_FACE_UP:
        return None
    return BoardLine(fields={"attached_prevents_turning_face_up": True})


LAYER_LINE_RECOGNIZERS = (
    _all_permanents_are_type,
    _all_objects_color,
    _players_spend_mana_as_any_color,
    _enchanted_is_type,
    _cant_be_turned_face_up,
)


# ---------------------------------------------------------------------------
# Effect phrases
# ---------------------------------------------------------------------------

# "[until your next turn, ][up to one ]target <np> becomes a[n] <types>[
# with power and toughness each equal to its mana value][ in addition to
# its other types][ until end of turn]" (Liquimetal Coating; Karn, the
# Great Creator's +1). The duration is either the leading "until your next
# turn," or the trailing "until end of turn" -- exactly one of them.
_TARGET_BECOMES_TYPE_RE = re.compile(
    r"^(?P<lead>until your next turn, )?(?P<upto>up to one )?target "
    r"(?P<np>.+?) becomes an? (?P<types>artifact creature|artifact|"
    r"creature)(?P<mv> with power and toughness each equal to its mana "
    r"value)?(?P<addition> in addition to its other types)?"
    r"(?P<trail> until end of turn)?$"
)


def _target_becomes_types(phrase: str) -> "Effect | None":
    m = _TARGET_BECOMES_TYPE_RE.match(phrase)
    if m is None:
        return None
    if bool(m.group("lead")) == bool(m.group("trail")):
        return None  # no duration, or two
    types = tuple(_ADDABLE_SINGULAR[w] for w in m.group("types").split())
    if types == ("Artifact",) and not m.group("addition"):
        # "becomes an artifact" without "in addition to its other types"
        # would SET the card type (CR 205.1a) -- no such card in any pool.
        return None
    if types == ("Creature",) and not m.group("addition"):
        return None
    from mtgz_ir.cards.parser.roto_board import permanent_phrase
    from mtgz_ir.effects.durations import EndOfTurn, EndOfYourNextTurn
    from mtgz_ir.effects.generic.roto_layers import TargetBecomesTypes
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = permanent_phrase(m.group("np"))
    if target_filter is None:
        return None
    return TargetBecomesTypes(
        targets=(TargetSpec(target_filter, 0 if m.group("upto") else 1, 1),),
        types_to_add=types,
        pt_equal_to_mana_value=bool(m.group("mv")),
        duration=EndOfYourNextTurn() if m.group("lead") else EndOfTurn(),
    )


# "you may reveal <a card descriptor> you own from outside the game or
# choose a face-up <card descriptor> you own in exile. put that card into
# your hand" (Karn, the Great Creator's -2) -- the "or face-up in exile"
# sibling of P4's wish shape (``roto_walkers._WISH_RE``). Both descriptors
# must name the same cards.
_WISH_OR_FACE_UP_EXILE_RE = re.compile(
    r"^you may reveal (?P<desc>an? .+? card) you own from outside the game "
    r"or choose a face-up (?P<desc2>.+? card) you own in exile\. "
    r"[Pp]ut that card into your hand$"
)


def _wish_or_face_up_exile(phrase: str) -> "Effect | None":
    m = _WISH_OR_FACE_UP_EXILE_RE.match(phrase)
    if m is None:
        return None
    desc = m.group("desc")
    article, _, rest = desc.partition(" ")
    if rest != m.group("desc2"):
        return None
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
    from mtgz_ir.effects.generic.roto_layers import (
        PutCardFromOutsideTheGameOrFaceUpExileIntoHand,
    )

    card_filter = parse_card_descriptor(desc)
    if card_filter is None:
        return None
    return PutCardFromOutsideTheGameOrFaceUpExileIntoHand(card_filter=card_filter)


_EFFECT_RECOGNIZERS = (_target_becomes_types, _wish_or_face_up_exile)


def parse_roto_layer_effect(phrase: str) -> "Effect | None":
    """An effect phrase every other grammar declined, or ``None``."""
    if _disabled():
        return None
    for recognizer in _EFFECT_RECOGNIZERS:
        eff = recognizer(phrase)
        if eff is not None:
            return eff
    return None


__all__ = [
    "LAYER_LINE_RECOGNIZERS",
    "parse_all_objects_color_line",
    "parse_roto_layer_effect",
]
