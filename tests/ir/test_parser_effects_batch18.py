"""Tests for batch-18 effect-grammar additions covering creature
becomes-tapped and tapped-creatures-gated end-step trigger bodies.

Three new effect primitives land in this batch:

* ``untap another target permanent`` →
  ``UntapTargetOtherPermanent()``. Nanoform Sentinel's becomes-tapped
  trigger body (paired with the trailing once-per-turn rider at the
  trigger-line level).
* ``another target creature you control gets +X/+X until end of turn,
  where X is this creature's power`` →
  ``PumpTargetOtherCreatureByXOverXWhereXIsSourcePower()``. Hemosymbic
  Mite's becomes-tapped trigger body. The variable-X bonus is fixed to
  the source's effective power at resolve time — encoded entirely by
  the engine class.
* ``put a +1/+1 counter on each creature you control other than this
  creature`` → ``PutPlusOneCounterOnEachCreatureYouControl(amount=1,
  exclude_source=True)``. Dawnstrike Vanguard's tapped-creatures-gated
  end-step trigger inner effect (the ``ConditionalOnTappedCreatures``
  wrapper sits at the trigger-line level, not inside the effect-phrase
  grammar).

Three creatures newly claim through these primitives:

* **Hemosymbic Mite** — single-line becomes-tapped pump-by-X.
* **Nanoform Sentinel** — single-line becomes-tapped untap with the
  trailing once-per-turn rider.
* **Dawnstrike Vanguard** — Lifelink keyword + tapped-creatures-gated
  end-step counter spread.

The shared parametrized parity test in ``tests/test_parser_parity.py``
enforces that each newly claimed ``CardPrinting`` matches the static
one verbatim, so this file focuses on the new effect-phrase grammar
in isolation plus claim assertions.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ConditionalOnTappedCreatures,
    PumpTargetOtherCreatureByXOverXWhereXIsSourcePower,
    PutPlusOneCounterOnEachCreatureYouControl,
    TriggeredAbility,
    UntapTargetOtherPermanent,
)
from mtgz_ir.cards.parser.effects import _parse_effect_phrase
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    IsSelf,
    YOU,
)


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- effect-phrase grammar ----------

def test_parse_untap_another_target_permanent():
    # Nanoform Sentinel's becomes-tapped trigger body. The ``another``
    # qualifier on the legal target is encoded by the engine class
    # reading ``state.resolving_source`` at resolve time — the parser
    # produces the bare engine class with no parameters.
    assert _parse_effect_phrase("untap another target permanent") == (
        UntapTargetOtherPermanent()
    )


def test_parse_pump_other_target_creature_by_x_source_power():
    # Hemosymbic Mite's becomes-tapped trigger body. Variable-X bonus
    # where X is the source's effective power at resolve time. Every
    # aspect of the effect (target restriction to other creatures the
    # controller controls, X-source from ``state.resolving_source``) is
    # encoded by the engine class — the parser produces the bare class.
    phrase = (
        "another target creature you control gets +X/+X until end of "
        "turn, where X is this creature's power"
    )
    assert _parse_effect_phrase(phrase) == (
        PumpTargetOtherCreatureByXOverXWhereXIsSourcePower()
    )


def test_parse_put_counter_on_each_other_creature_you_control():
    # Dawnstrike Vanguard's end-step trigger inner effect (the trigger
    # line wraps this in ``ConditionalOnTappedCreatures`` — the wrapper
    # is applied at the trigger-line level, not inside the effect-
    # phrase grammar). The ``other than this creature`` tail sets the
    # engine class's ``exclude_source=True`` flag.
    phrase = (
        "put a +1/+1 counter on each creature you control other than "
        "this creature"
    )
    assert _parse_effect_phrase(phrase) == (
        PutPlusOneCounterOnEachCreatureYouControl(
            amount=1, exclude_source=True,
        )
    )


# ---------- trigger-line end-to-end ----------

def test_dawnstrike_vanguard_end_step_trigger_line():
    # The full trigger line for Dawnstrike Vanguard composes:
    #   - ``At the beginning of <when>`` shape (your end step → the
    #     ``Controlled(YOU)`` predicate)
    #   - ``if you control two or more tapped creatures`` middle clause
    #     (wraps the inner effect in ``ConditionalOnTappedCreatures``)
    #   - the new ``put a +1/+1 counter on each creature you control
    #     other than this creature`` inner-effect grammar.
    # Mirrors the static-registry layout exactly.
    line = (
        "At the beginning of your end step, if you control two or more "
        "tapped creatures, put a +1/+1 counter on each creature you "
        "control other than this creature."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.BEGINNING_OF_END_STEP,
        predicate=Controlled(YOU),
        effect=ConditionalOnTappedCreatures(
            effect=PutPlusOneCounterOnEachCreatureYouControl(
                amount=1, exclude_source=True,
            ),
            min_count=2,
        ),
    )


def test_nanoform_sentinel_trigger_line_carries_once_per_turn():
    # Nanoform Sentinel's becomes-tapped trigger plus the trailing
    # ``This ability triggers only once each turn.`` rider. The rider
    # strip happens BEFORE the line shape is detected, so the produced
    # ``TriggeredAbility`` carries ``once_per_turn=True`` alongside the
    # new ``UntapTargetOtherPermanent`` effect.
    line = (
        "Whenever this creature becomes tapped, untap another target "
        "permanent. This ability triggers only once each turn."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.BECOMES_TAPPED,
        predicate=IsSelf(),
        effect=UntapTargetOtherPermanent(),
        once_per_turn=True,
    )


def test_hemosymbic_mite_trigger_line():
    # Hemosymbic Mite's becomes-tapped trigger line. The variable-X
    # pump's trigger half is the same ``IsSelf()`` predicate every
    # other ``becomes_tapped`` self-trigger uses; the new content is
    # the effect-phrase grammar covered above.
    line = (
        "Whenever this creature becomes tapped, another target creature "
        "you control gets +X/+X until end of turn, where X is this "
        "creature's power."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.BECOMES_TAPPED,
        predicate=IsSelf(),
        effect=PumpTargetOtherCreatureByXOverXWhereXIsSourcePower(),
    )


# ---------- claim tests (end-to-end via the parser source) ----------

def test_hemosymbic_mite_claimed_by_parser():
    assert "Hemosymbic Mite" in _claimed_names()


def test_nanoform_sentinel_claimed_by_parser():
    assert "Nanoform Sentinel" in _claimed_names()


def test_dawnstrike_vanguard_claimed_by_parser():
    assert "Dawnstrike Vanguard" in _claimed_names()
