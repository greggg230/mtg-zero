"""Tests for the intervening-if grammar in the trigger-line parser.

Adds support for the ``When/Whenever <condition>, if <gate>, <effect>.``
shape (CR 603.4). The middle ``if <gate>,`` clause looks up a closed
set of game-state gate phrases in ``_INTERVENING_IF_PHRASES`` and
AND-s the resulting ``Predicate`` onto the trigger's base event
predicate. Two gate phrases land in this batch:

* ``"an opponent controls more lands than you"`` — Sunstar
  Expansionist's ETB. Predicate ``StateBound(OpponentControlsMoreLandsThanYou())``.
* ``"a nonland permanent left the battlefield this turn or a spell
  was warped this turn"`` — the Void rider on Roving Actuator (ETB)
  and Alpharael, Stonechosen (attack). Predicate ``StateBound(VoidActive())``,
  which delegates to the engine's ``is_void_active`` helper so the
  trigger-time check matches the ``requires_void`` static-flag
  semantics already in place.

The parser handles BOTH a bare ``When/Whenever`` line and an
``<AbilityWord> — When/Whenever`` line: the generic ability-word
strip (``_strip_ability_word_prefix``) runs first, so a ``Void —``
prefix is removed before the intervening-if regex runs. We test both
shapes.

An unrecognized gate phrase causes the line to be rejected outright
— the parser does not silently fall back to the no-gate shape, which
would produce an over-firing trigger by dropping the gate. We test
that explicitly so a future maintainer doesn't loosen the rejection.

End-to-end card claims: none of Sunstar Expansionist / Roving
Actuator / Alpharael, Stonechosen are claimed by the parser today
(each has at least one other unparseable clause — landfall pump,
bespoke graveyard-copy, half-life damage). This batch ships the
intervening-if plumbing so those claims unblock as the surrounding
grammar grows.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DrawCards,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.effects import _parse_effect_phrase
from mtgz_ir.cards.parser.triggers import (
    _INTERVENING_IF_PHRASES,
    _parse_intervening_if_phrase,
    _parse_triggered_ability_line,
)
from mtgz_ir.effects.state_predicates import (
    OpponentControlsMoreLandsThanYou,
    VoidActive,
)
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
    StateBound,
)


# ---------- _parse_intervening_if_phrase: positive lookups ----------


def test_intervening_if_phrasebook_includes_opponent_more_lands():
    assert (
        _parse_intervening_if_phrase("an opponent controls more lands than you")
        == StateBound(OpponentControlsMoreLandsThanYou())
    )


def test_intervening_if_phrasebook_includes_void_active():
    phrase = (
        "a nonland permanent left the battlefield this turn or a spell "
        "was warped this turn"
    )
    assert _parse_intervening_if_phrase(phrase) == StateBound(VoidActive())


def test_intervening_if_phrasebook_strips_surrounding_whitespace():
    # Defensive — the regex captures verbatim what's between ``if `` and
    # the trailing ``, ``, but stray inner whitespace (e.g. a duplicated
    # space) would still show up. The lookup helper strips outer
    # whitespace; this guards against an accidental change that drops
    # the strip.
    assert (
        _parse_intervening_if_phrase("  an opponent controls more lands than you  ")
        == StateBound(OpponentControlsMoreLandsThanYou())
    )


def test_intervening_if_phrasebook_rejects_unknown_phrase():
    assert _parse_intervening_if_phrase("the moons of endor are aligned") is None


# ---------- _parse_triggered_ability_line: end-to-end ----------


def test_intervening_if_etb_opponent_more_lands():
    line = (
        "When this creature enters, if an opponent controls more "
        "lands than you, draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & StateBound(OpponentControlsMoreLandsThanYou()),
        effect=DrawCards(amount=1),
    )


def test_intervening_if_etb_with_lander_create_effect():
    # Sunstar Expansionist's ETB shape. The card itself isn't claimed
    # end-to-end (its second line — Landfall pump — isn't in the
    # grammar), but the ETB clause must parse to the predicate-style
    # trigger we want with whatever ``CreateToken``-shaped effect the
    # effect grammar produces for the Lander phrase. Parity vs static
    # is enforced separately in ``test_parser_parity`` once the card
    # claims end-to-end.
    line = (
        "When this creature enters, if an opponent controls more "
        "lands than you, create a Lander token."
    )
    expected_effect = _parse_effect_phrase("create a Lander token")
    assert expected_effect is not None
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & StateBound(OpponentControlsMoreLandsThanYou()),
        effect=expected_effect,
    )


def test_intervening_if_void_attack_predicate():
    # Bare ``Whenever this creature attacks, if <Void gate>, <effect>.``
    # The Void ability-word prefix is tested separately below — here
    # we exercise the no-prefix shape to keep the regex paths
    # independent.
    line = (
        "Whenever this creature attacks, if a nonland permanent left "
        "the battlefield this turn or a spell was warped this turn, "
        "draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsSelf() & StateBound(VoidActive()),
        effect=DrawCards(amount=1),
    )


def test_intervening_if_void_ability_word_prefix_stripped():
    # The ``Void —`` ability-word prefix is generic-stripped (no
    # rules consequence per CR 207.2c); the surviving body parses
    # the same way the bare attack form does. This mirrors the
    # ``Landfall —`` prefix-strip behavior already exercised by the
    # creature ETB tests.
    line = (
        "Void — Whenever this creature attacks, if a nonland permanent "
        "left the battlefield this turn or a spell was warped this "
        "turn, draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsSelf() & StateBound(VoidActive()),
        effect=DrawCards(amount=1),
    )


def test_intervening_if_void_etb_predicate():
    # Roving Actuator ETB shape. Same Void gate as the attack variant,
    # different event.
    line = (
        "Void — When this creature enters, if a nonland permanent left "
        "the battlefield this turn or a spell was warped this turn, "
        "draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & StateBound(VoidActive()),
        effect=DrawCards(amount=1),
    )


def test_intervening_if_unknown_gate_rejected():
    # An unrecognized gate phrase causes the WHOLE line to be rejected.
    # The parser must NOT silently fall back to the no-gate shape (which
    # would over-fire the trigger by dropping the gate). The condition
    # phrase ``this creature enters`` IS recognized; the rejection is
    # specifically because the gate is not.
    line = (
        "When this creature enters, if the moons of endor are aligned, "
        "draw a card."
    )
    assert _parse_triggered_ability_line(line) is None


def test_intervening_if_unknown_condition_rejected():
    # Recognized gate but unknown trigger condition — the condition
    # phrase doesn't claim either the predicate-style or string-key
    # table, so the line is rejected. Guards against accidentally
    # building a bare-predicate ``TriggeredAbility`` whose event field
    # is unset.
    line = (
        "Whenever this creature waltzes, if an opponent controls more "
        "lands than you, draw a card."
    )
    assert _parse_triggered_ability_line(line) is None


def test_no_intervening_if_path_unaffected():
    # Sanity: the no-gate shape still parses through the regular
    # path. A regression here would mean the new intervening-if regex
    # is shadowing every plain trigger line.
    line = "When this creature enters, draw a card."
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ENTERS, predicate=IsSelf(),
        effect=DrawCards(amount=1),
    )


# ---------- phrasebook integrity ----------


def test_intervening_if_phrasebook_keys_match_predicate_types():
    # Every phrase in the table maps to a Predicate instance — defensive
    # check that a future addition doesn't accidentally land a tuple
    # or a string in there. Mirrors the equivalent integrity check on
    # ``_PREDICATE_TRIGGER_PHRASES``.
    from mtgz_ir.effects.predicates import (
        Predicate,
    )

    for phrase, pred in _INTERVENING_IF_PHRASES.items():
        assert isinstance(pred, Predicate), (
            f"phrase {phrase!r} maps to non-Predicate {pred!r}"
        )


# ---------- predicate evaluation: VoidActive / OpponentControlsMoreLandsThanYou ----------


def _two_player_state():
    """Minimal two-player ``GameState`` used for predicate evaluation
    tests. Mirrors the fixture style in ``tests/test_actions.py``."""
    from mtgz_ir.game import GameState, Player

    s = GameState()
    s.players.append(Player(id=0))
    s.players.append(Player(id=1))
    s.active_player_idx = 0
    return s


class _StubSource:
    """Tiny stand-in for a battlefield ``GameObject`` that exposes only
    the attributes the predicates read (``controller_id``). Avoids the
    full GameObject construction cost for these unit-level checks."""

    def __init__(self, controller_id: int) -> None:
        self.controller_id = controller_id


def test_void_active_predicate_reflects_engine_helper():
    # The predicate delegates to ``is_void_active``; mark a nonland
    # permanent as having left this turn (the helper checks
    # ``state.nonland_permanent_left_battlefield_this_turn``) and the
    # predicate flips from False to True. Source/subjects are inert
    # for this gate.
    s = _two_player_state()
    src = _StubSource(controller_id=0)
    pred = StateBound(VoidActive())
    assert pred.matches(s, {}, src) is False
    s.permanent_left_battlefield_this_turn = True
    assert pred.matches(s, {}, src) is True
    # spell-warped flank also flips it.
    s.permanent_left_battlefield_this_turn = False
    s.spell_warped_this_turn = True
    assert pred.matches(s, {}, src) is True


def test_opponent_more_lands_predicate_counts_battlefield():
    # Stand up two synthetic land permanents on the battlefield: one
    # under each player. With equal counts the predicate is False;
    # adding a second land for opponent flips it to True.
    from mtgz_ir.cards.base import (
        CardPrinting, parse_mana_cost,
    )
    from mtgz_ir.game import GameObject

    s = _two_player_state()
    src = _StubSource(controller_id=0)
    pred = StateBound(OpponentControlsMoreLandsThanYou())
    assert pred.matches(s, {}, src) is False  # no lands at all

    land_printing = CardPrinting(
        name="Plains",
        mana_cost_str="",
        mana_cost=parse_mana_cost(""),
        type_line="Basic Land — Plains",
        supertypes=("Basic",),
        card_types=("Land",),
        subtypes=("Plains",),
    )
    from mtgz_ir.game import Zone
    s.battlefield.append(GameObject(
        id=100, printing=land_printing, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    ))
    s.battlefield.append(GameObject(
        id=101, printing=land_printing, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    ))
    assert pred.matches(s, {}, src) is False  # equal counts
    s.battlefield.append(GameObject(
        id=102, printing=land_printing, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    ))
    assert pred.matches(s, {}, src) is True  # opponent ahead


def test_opponent_more_lands_predicate_skips_lost_players():
    # A player who has already lost shouldn't count toward the
    # ``opponent has more lands`` check. Defensive — keeps the
    # predicate stable in the multiplayer/elimination edge case.
    from mtgz_ir.cards.base import CardPrinting, parse_mana_cost
    from mtgz_ir.game import GameObject

    s = _two_player_state()
    src = _StubSource(controller_id=0)
    pred = StateBound(OpponentControlsMoreLandsThanYou())
    land_printing = CardPrinting(
        name="Plains",
        mana_cost_str="",
        mana_cost=parse_mana_cost(""),
        type_line="Basic Land — Plains",
        supertypes=("Basic",),
        card_types=("Land",),
        subtypes=("Plains",),
    )
    from mtgz_ir.game import Zone
    s.battlefield.append(GameObject(
        id=100, printing=land_printing, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    ))
    s.players[1].has_lost = True
    assert pred.matches(s, {}, src) is False
