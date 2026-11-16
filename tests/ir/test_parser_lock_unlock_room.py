"""``lock or unlock a door of target Room you control`` effect-grammar
recognizer.

Keys to the House's sorcery-speed activated body now claims through
the unified ``_parse_effect`` grammar instead of the artifact-builder
literal-line table. The recognizer maps directly to the existing
generic ``LockOrUnlockDoorOfTargetRoomYouControl`` engine class — no
new engine wiring; only the parser-side dispatch was bespoke before
this round.

The activated-ability cost atoms (``{3}, {T}, Sacrifice this
artifact``) and the inline ``Activate only as a sorcery.`` rider also
ride the existing unified activation grammar — the entire activation
line is now derivable from oracle text.
"""

from __future__ import annotations

from mtgz_ir.cards.base import ActivatedAbility, ActivationCost, parse_mana_cost
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.effects.generic import LockOrUnlockDoorOfTargetRoomYouControl


def test_lock_or_unlock_door_target_room_parses():
    """The bare normalized phrase parses to the generic engine class
    with no parameters."""
    eff = _parse_effect("lock or unlock a door of target Room you control")
    assert eff == LockOrUnlockDoorOfTargetRoomYouControl()


def test_lock_or_unlock_room_capitalization_matters():
    """The word ``Room`` is preserved with its printed capitalization
    by ``_normalize_activated_effect_phrase`` (only the first letter
    of the phrase is lowercased). A lowercase ``room`` doesn't claim —
    Scryfall always prints the type word capitalized in this clause."""
    assert _parse_effect("lock or unlock a door of target room you control") is None


def test_keys_to_the_house_activation_line_end_to_end():
    """The full Keys to the House body line parses end-to-end via
    ``_parse_activated_ability_line`` — cost (``{3}, {T}, Sacrifice
    this artifact``), body (``LockOrUnlockDoorOfTargetRoomYouControl``),
    and the inline sorcery-speed rider all derive from oracle text."""
    ab = _parse_activated_ability_line(
        "{3}, {T}, Sacrifice this artifact: Lock or unlock a door of "
        "target Room you control. Activate only as a sorcery."
    )
    assert ab is not None
    assert ab.cost == ActivationCost(
        mana=parse_mana_cost("{3}"),
        mana_str="{3}",
        tap_self=True,
        sacrifice_self=True,
    )
    assert ab.effect == LockOrUnlockDoorOfTargetRoomYouControl()
    assert ab.sorcery_speed is True
    assert ab.is_mana_ability is False
