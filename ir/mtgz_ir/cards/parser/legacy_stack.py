"""Spell effect phrases of the Legacy cards outside the cube (pool
``legacy``): mtg-zero's Legacy Doomsday deck, its lane B (Misdirection;
Flusterstorm and Cavern of Souls parse through the roto grammars already).

* ``change the target of target <spell phrase> with a single target`` ->
  :class:`~mtgz_ir.effects.generic.legacy_stack.
  ChangeTheTargetOfTargetWithASingleTarget` (CR 115.7a, 115.9a). The spell
  phrase is the counterspells' (``parse_spell_phrase``: "spell", "instant
  or sorcery spell", ...).

  Not claimed (yet): "... target spell or ability with a single target"
  (Willbender's, Untimely Malfunction's shape; the class's ``kinds`` can
  hold it). Claiming it would change Untimely Malfunction's printing in the
  roto, eoe and all pools: today its modal parses lossily, to its first
  mode alone (``DestroyTargetArtifact``; its third mode, "one or two target
  creatures can't block this turn", has no grammar), and with the second
  mode claimed the card stops parsing at all. The legacy pool's lane must
  leave every other pool's printings as they are: the shape waits for the
  fix of that modal.

Hooked as the very last fallback of ``effects._parse_effect`` (normalized
phrases: no trailing period, lower-cased first letter), so no phrase another
grammar claims changes.
"""

from __future__ import annotations

import re

from mtgz_ir.effects.base import Effect

_CHANGE_THE_TARGET_RE = re.compile(
    r"^change the target of target (?P<what>.+) with a single target$"
)
_OR_ABILITY = " or ability"


def _change_the_target(phrase: str) -> Effect | None:
    m = _CHANGE_THE_TARGET_RE.match(phrase)
    if m is None:
        return None
    from mtgz_ir.cards.parser.target_phrases import parse_spell_phrase
    from mtgz_ir.effects.generic.legacy_stack import (
        ChangeTheTargetOfTargetWithASingleTarget,
    )
    from mtgz_ir.effects.vocab import StackObjectKind

    what = m.group("what")
    if what.endswith(_OR_ABILITY):
        # (Module docs: the "or ability" shape is left unclaimed.)
        return None
    ok, spell_filter = parse_spell_phrase(what)
    if not ok:
        return None
    return ChangeTheTargetOfTargetWithASingleTarget(
        kinds=(StackObjectKind.SPELL,), spell_filter=spell_filter,
    )


_EFFECT_RECOGNIZERS: tuple = (_change_the_target,)


def parse_legacy_stack_effect(phrase: str) -> Effect | None:
    """A legacy-pool stack effect phrase (normalized), or ``None``."""
    for recognizer in _EFFECT_RECOGNIZERS:
        eff = recognizer(phrase)
        if eff is not None:
            return eff
    return None


__all__ = ["parse_legacy_stack_effect"]
