"""Effect-phrase recognizers for the PowerLSV cube enchantment bucket
(``feature/cube-enchantments``).

:func:`parse_enchantment_phrase` is registered as a fallback recognizer
in :mod:`mtgz_ir.cards.parser.precise_grammar`, so it only runs on
normalized phrases (no trailing period, lowercased first letter) that
every earlier grammar rejected — it can never change how an
already-claimed printing parses.

Shapes:

* ``add one mana of any type the sacrificed land could produce`` ->
  :class:`AddOneManaSacrificedLandCouldProduce` (Squandered Resources).
* ``exile [up to one ]target <noun> until this <permanent> leaves the
  battlefield`` -> :class:`ExileTargetUntilSourceLeaves` (Touch the
  Spirit Realm), any noun the compositional noun-phrase parser reads.
* ``exile [up to one ][another|other ]target <noun>. Return it|that card
  to the battlefield under its owner's control at the beginning of the
  next end step`` (and the "At the beginning of the next end step,
  return that card ... [If it entered under your control, put a +1/+1
  counter on this creature]" wording) ->
  :class:`ExileTargetReturnAtNextEndStep` (Touch the Spirit Realm's
  Channel ability).
* ``each player returns to the battlefield all cards they own exiled
  with <it|this permanent>`` -> :class:`EachPlayerReturnsCardsExiledWithSource`
  (Parallax Wave).
* Oath of Druids' upkeep body -> :class:`OathRevealUntilCreature`.

:func:`parse_reanimation_aura_line` claims whole oracle lines of the
reanimation-Aura family (Animate Dead, Necromancy) for the cube
enchantment builder.
"""

from __future__ import annotations

import re

from mtgz_ir.effects.base import Effect


_SACRIFICED_LAND_MANA = "add one mana of any type the sacrificed land could produce"

_SELF_NOUN = r"this (?:enchantment|artifact|creature|permanent)"

_EXILE_UNTIL_SOURCE_LEAVES = re.compile(
    rf"^exile (?P<upto>up to one )?target (?P<noun>.+?) until {_SELF_NOUN} "
    rf"leaves the battlefield$"
)

# Also Flickerwisp's "exile another target permanent. Return that card
# ..." and Phelia's "exile up to one other target nonland permanent. At
# the beginning of the next end step, return that card ... If it entered
# under your control, put a +1/+1 counter on <this>." (cube W/G bucket).
_EXILE_RETURN_AT_NEXT_END_STEP = re.compile(
    r"^exile (?P<upto>up to one )?(?:(?P<other>another|other) )?target "
    r"(?P<noun>.+?)\. (?:[Rr]eturn (?:it|that card) to the battlefield under "
    r"its owner's control at the beginning of the next end step|At the "
    r"beginning of the next end step, return (?:it|that card) to the "
    r"battlefield under its owner's control(?P<counter>\. If it entered "
    r"under your control, put a \+1/\+1 counter on this creature)?)$"
)

_EACH_PLAYER_RETURNS_EXILED_WITH = re.compile(
    rf"^each player returns to the battlefield all cards they own exiled "
    rf"with (?:it|{_SELF_NOUN})$"
)

# Oath of Druids (only card in the pool with the "player who controls
# more creatures" target + reveal-until-creature body; the pieces are
# parameterized on the revealed card type so a second "reveal until a
# <type> card" printing needs no new class).
_OATH_BODY = re.compile(
    r"^that player chooses target player who controls more creatures than "
    r"they do and is their opponent\. The first player may reveal cards "
    r"from the top of their library until they reveal an? (?P<type>[a-z]+) "
    r"card\. If the first player does, that player puts that card onto the "
    r"battlefield and all other cards revealed this way into their "
    r"graveyard$"
)


def parse_enchantment_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.cards.parser.effects_ext import _target_spec
    from mtgz_ir.effects.generic.enchantment_mechanics import (
        AddOneManaSacrificedLandCouldProduce,
        EachPlayerReturnsCardsExiledWithSource,
        ExileTargetReturnAtNextEndStep,
        ExileTargetUntilSourceLeaves,
        OathRevealUntilCreature,
    )

    if phrase == _SACRIFICED_LAND_MANA:
        return AddOneManaSacrificedLandCouldProduce()
    m = _EXILE_UNTIL_SOURCE_LEAVES.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), bool(m.group("upto")))
        if spec is None:
            return None
        return ExileTargetUntilSourceLeaves(targets=(spec,))
    m = _EXILE_RETURN_AT_NEXT_END_STEP.match(phrase)
    if m is not None:
        spec = _target_spec(m.group("noun"), bool(m.group("upto")))
        if spec is None:
            return None
        if m.group("other"):
            from dataclasses import replace

            from mtgz_ir.effects.object_predicates import IsSource

            spec = replace(spec, filter=spec.filter & ~IsSource())
        return ExileTargetReturnAtNextEndStep(
            targets=(spec,),
            counter_on_source_if_yours=bool(m.group("counter")),
        )
    if _EACH_PLAYER_RETURNS_EXILED_WITH.match(phrase):
        return EachPlayerReturnsCardsExiledWithSource()
    m = _OATH_BODY.match(phrase)
    if m is not None:
        return OathRevealUntilCreature(card_type=m.group("type"))
    return None


# The reanimation-Aura enters/leaves paragraph. Two printed variants:
# the Aura that already enchants a graveyard card (Animate Dead) and the
# non-Aura enchantment that becomes one and targets (Necromancy).
_REANIMATION_TRIGGER_LINE = re.compile(
    r"^When this (?P<self>Aura|enchantment) enters, if it's on the "
    r"battlefield, (?:"
    r"it loses \"enchant creature card in a graveyard\" and gains \"enchant "
    r"creature put onto the battlefield with this Aura\.\" Return enchanted "
    r"creature card to the battlefield under your control and attach this "
    r"Aura to it"
    r"|it becomes an Aura with \"enchant creature put onto the battlefield "
    r"with [^\"]+\.\" Put (?P<target>target) creature card from a graveyard "
    r"onto the battlefield under your control and attach this enchantment "
    r"to it"
    r")\. When this (?P=self) leaves the battlefield, that creature's "
    r"controller sacrifices it\.$"
)

# Necromancy's cast permission + instant-speed rider. Literal: the only
# "as though it had flash ... next cleanup step" card in the pool.
_AS_THOUGH_FLASH_CLEANUP_SACRIFICE_LINE = (
    "You may cast this spell as though it had flash. If you cast it any "
    "time a sorcery couldn't have been cast, the controller of the "
    "permanent it becomes sacrifices it at the beginning of the next "
    "cleanup step."
)


def parse_reanimation_aura_line(line: str):
    """Whole-line recognizer for the reanimation-Aura family. Returns
    ``(triggers, printing_fields)`` or ``None``.

    * The enters/leaves paragraph -> an ``ENTERS`` trigger with
      :class:`ReanimateAndAttachSource` (``from_target`` for the
      "Put target creature card from a graveyard" variant) and a
      ``LEAVES_BATTLEFIELD`` trigger with
      :class:`LinkedCreatureControllerSacrifices`.
    * Necromancy's cast line -> ``flash`` (cast permission) plus
      ``sacrifice_at_cleanup_if_cast_without_sorcery_timing``.
    """
    from mtgz_ir.effects.base import TriggeredAbility
    from mtgz_ir.effects.generic.enchantment_mechanics import (
        LinkedCreatureControllerSacrifices,
        ReanimateAndAttachSource,
    )
    from mtgz_ir.effects.predicates import Event, IsSelf

    if line == _AS_THOUGH_FLASH_CLEANUP_SACRIFICE_LINE:
        return (), {
            "flash": True,
            "sacrifice_at_cleanup_if_cast_without_sorcery_timing": True,
        }
    m = _REANIMATION_TRIGGER_LINE.match(line)
    if m is None:
        return None
    triggers = (
        TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(),
            effect=ReanimateAndAttachSource(
                from_target=m.group("target") is not None,
            ),
        ),
        TriggeredAbility(
            event=Event.LEAVES_BATTLEFIELD, predicate=IsSelf(),
            effect=LinkedCreatureControllerSacrifices(),
        ),
    )
    return triggers, {}


__all__ = ["parse_enchantment_phrase", "parse_reanimation_aura_line"]
