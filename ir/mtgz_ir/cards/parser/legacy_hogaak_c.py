"""Lines of the Legacy cards outside the cube (pool ``legacy``): mtg-zero's
Legacy Hogaak Tempo deck (MTGGoldfish 8027954), lane C (ENGINE.md 220.3).

Stitcher's Supplier ("When this creature enters or dies, mill three cards")
parses through the existing compound-trigger and ``SelfMill`` grammars.
The other three needed:

* Dark Matter Manipulator's "This creature gets +2/+0 for every seven cards
  in your graveyard": the roto ``_SELF_GETS_PER_GRAVEYARD`` recognizer
  (Knight of the Reliquary's) takes "for every <N> [<type>] cards" too,
  ``CardsInYourGraveyard(per=N)``.
* Moonshadow's "This creature enters with six -1/-1 counters on it": the
  creature builder's enters-with-counters recognizer takes -1/-1 counters
  and counts to ten. Its "remove a -1/-1 counter from this creature": the
  Dark Depths recognizer (``RemoveCountersFromSource``) takes the +1/+1 and
  -1/-1 kinds. Its trigger condition: :func:`parse_hogaak_c_trigger_phrase`
  below (a new event, ``Event.CARD_PUT_INTO_GRAVEYARD``), and its "while
  this creature has a -1/-1 counter on it" through the intervening-"while"
  path: :func:`parse_source_has_counter_gate`.
* Dryad Arbor ("Land Creature — Forest Dryad"): the creature builder knew
  only the artifact and enchantment card types beside creature, so its land
  type and its Forest's intrinsic "{T}: Add {G}" (CR 305.6) were dropped:
  :func:`as_land_creature` (docs/ir_defects.md 440).
"""

from __future__ import annotations

import re
from dataclasses import replace

# "one or more <noun> cards are put into <whose> graveyard from anywhere"
# (Moonshadow): the condition part of a "Whenever ..." line.
_CARDS_PUT_INTO_GRAVEYARD_RE = re.compile(
    r"^one or more (?:(?P<noun>permanent|creature|land|artifact|enchantment|"
    r"nonland|noncreature) )?cards are put into (?P<whose>your|an opponent's|a) "
    r"graveyard from anywhere$"
)
# "this <permanent> has a <kind> counter on it" (the gate of "Whenever ...
# while this creature has a -1/-1 counter on it, ...").
_SOURCE_HAS_COUNTER_RE = re.compile(
    r"^this (?:creature|artifact|enchantment|land|permanent) has an? "
    r"(?P<kind>[a-z]+|[+-]1/[+-]1) counter on it$"
)

# The colour of each basic land type's intrinsic mana ability (CR 305.6).
_BASIC_LAND_TYPE_COLORS = {
    "Plains": "W", "Island": "U", "Swamp": "B", "Mountain": "R", "Forest": "G",
}


def _card_noun(noun: str | None):
    from mtgz_ir.effects.object_predicates import (
        IsArtifact, IsCreature, IsEnchantment, IsLand,
    )
    from mtgz_ir.effects.value_predicates import IsPermanentCard

    if noun is None:
        return None
    return {
        "permanent": IsPermanentCard(),
        "creature": IsCreature(),
        "land": IsLand(),
        "artifact": IsArtifact(),
        "enchantment": IsEnchantment(),
        "nonland": ~IsLand(),
        "noncreature": ~IsCreature(),
    }[noun]


def parse_source_has_counter_gate(phrase: str):
    """``Source(HasCounter(kind))`` for "this creature has a <kind> counter
    on it", else ``None``: the gate of "Whenever <event> while this creature
    has a -1/-1 counter on it, ..." (Moonshadow) is part of the trigger
    condition, the source's state as the event happens (as it is
    immediately after the event, CR 603.10), not an intervening "if" (CR
    603.4 is about "if" only): it isn't checked again on resolution."""
    from mtgz_ir.effects.object_predicates import HasCounter
    from mtgz_ir.effects.predicates import Source

    m = _SOURCE_HAS_COUNTER_RE.match(phrase)
    if m is None:
        return None
    return Source(HasCounter(m.group("kind")))


def parse_hogaak_c_trigger_phrase(phrase: str):
    """``(Event, Predicate)`` for "one or more <noun> cards are put into
    <whose> graveyard from anywhere", else ``None``.

    "One or more": once per event however many cards it puts there (CR
    603.2c; ``OneOrMoreBatch``). The event's subjects are the graveyard's
    owner and the card as it is there: "from anywhere" never looks back in
    time (CR 603.6c, 603.10).
    """
    from mtgz_ir.effects.generic.cube_creatures_rmc import OneOrMoreBatch
    from mtgz_ir.effects.predicates import (
        OPPONENT, YOU, Controlled, Event, Subject,
    )

    m = _CARDS_PUT_INTO_GRAVEYARD_RE.match(phrase)
    if m is None:
        return None
    pred = OneOrMoreBatch()
    noun = _card_noun(m.group("noun"))
    if noun is not None:
        pred = Subject("card", noun) & pred
    whose = m.group("whose")
    if whose == "your":
        pred = Controlled(YOU, subject="player") & pred
    elif whose == "an opponent's":
        pred = Controlled(OPPONENT, subject="player") & pred
    return (Event.CARD_PUT_INTO_GRAVEYARD, pred)


def as_land_creature(printing, card: dict):
    """A creature printing whose printed type line has the land card type
    too ("Land Creature — Forest Dryad", Dryad Arbor), with that type: the
    card types and type line as printed, and the intrinsic mana ability of
    each basic land type it has (CR 305.6: "an object with the land card
    type and a basic land type has the intrinsic ability '{T}: Add [mana
    symbol]', even if the text box doesn't actually contain that text") as
    ``produces``, its only mana (``mana_only_from_produces``). It is played
    only as a land (CR 305.9) and is affected by summoning sickness (302.6):
    rules, not text. ``None`` (not claimed) if the printed type line names
    other card types than the printing's plus Land."""
    from mtgz_ir.cards.parser.type_line import _parse_type_line

    supertypes, card_types, subtypes = _parse_type_line(card.get("type_line", ""))
    if "Land" not in card_types or "Land" in printing.card_types:
        return None
    if set(card_types) != set(printing.card_types) | {"Land"}:
        return None
    if tuple(subtypes) != tuple(printing.subtypes) or tuple(supertypes) != tuple(printing.supertypes):
        return None
    produces = frozenset(
        _BASIC_LAND_TYPE_COLORS[s] for s in subtypes if s in _BASIC_LAND_TYPE_COLORS
    )
    return replace(
        printing,
        type_line=card["type_line"],
        card_types=tuple(card_types),
        produces=produces,
        mana_only_from_produces=True,
    )
