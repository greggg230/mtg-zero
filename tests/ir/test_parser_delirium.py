"""Tests for the Duskmourn ``Delirium`` mechanic in the parser.

Delirium is a static threshold MTG keyword: an ability fires (or an
effect resolves) only when the source's controller has four or more
distinct card types among cards in their graveyard. The parser
support lands as:

* ``Delirium`` predicate in ``mtgz_ir.effects.predicates`` — a
  game-state check that walks the controller's graveyard and counts
  distinct card types from the comprehensive-rules taxonomy
  (Creature, Land, Sorcery, Instant, Enchantment, Artifact,
  Planeswalker, Battle, Tribal). Subject-independent, mirrors
  ``VoidActive`` and ``OpponentControlsMoreLandsThanYou``.

* ``_INTERVENING_IF_PHRASES`` entry: the canonical printed gate
  ``"there are four or more card types among cards in your graveyard"``
  maps to ``StateBound(Delirium())``. The Delirium ability-word prefix
  (``Delirium — ``) carries no rules consequence — it strips through
  the existing generic ``_strip_ability_word_prefix`` helper with no
  Delirium-specific code path needed for ``When``/``Whenever`` triggers.

* ``_DELIRIUM_PREFIX`` / ``_DELIRIUM_AT_BEGINNING_GATE_PREFIX`` in
  ``triggers.py`` (bespoke-delirium) — handles the distinct surface
  ``Delirium — At the beginning of <when>, if there are four or more
  card types among cards in your graveyard, <effect>.`` by stripping
  the prefix, consuming the gate clause, and AND-ing
  ``StateBound(Delirium())`` onto the trigger's base predicate (same
  predicate-compose pattern as ``Survival — At the beginning of``).
  The ``_strip_ability_word_prefix`` lookahead guards against
  ``Delirium — At …`` being parsed as a generic ability-word strip
  (it only strips in front of ``When``/``Whenever``), so the prefix is
  detected explicitly here mirroring the ``_VOID_PREFIX`` /
  ``_SURVIVAL_PREFIX`` precedent.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from mtgz_ir.cards.base import DrawCards, TriggeredAbility
from mtgz_ir.cards.parser.triggers import (
    _INTERVENING_IF_PHRASES,
    _TRAILING_IF_PHRASES,
    _parse_intervening_if_phrase,
    _parse_trailing_if_phrase,
    _parse_triggered_ability_line,
)
from mtgz_ir.effects.state_predicates import (
    Delirium,
)
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
    Predicate,
    StateBound,
)


# ---------- _parse_intervening_if_phrase: positive lookups ----------


def test_intervening_if_phrasebook_includes_delirium():
    phrase = "there are four or more card types among cards in your graveyard"
    assert _parse_intervening_if_phrase(phrase) == StateBound(Delirium())


def test_intervening_if_phrasebook_strips_surrounding_whitespace_for_delirium():
    # The lookup helper strips outer whitespace; this guards against an
    # accidental change that drops the strip. Mirrors the equivalent
    # check on the Void / opponent-more-lands gates.
    phrase = (
        "  there are four or more card types among cards in your "
        "graveyard  "
    )
    assert _parse_intervening_if_phrase(phrase) == StateBound(Delirium())


# ---------- _parse_triggered_ability_line: end-to-end ----------


def test_intervening_if_delirium_attack_predicate():
    # Bare ``Whenever this creature attacks, if <Delirium gate>,
    # <effect>.`` — the Delirium ability-word prefix is tested
    # separately below; here we exercise the no-prefix shape so the
    # regex paths stay independent.
    line = (
        "Whenever this creature attacks, if there are four or more "
        "card types among cards in your graveyard, draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


def test_intervening_if_delirium_etb_predicate():
    # Same Delirium gate on an ETB trigger. Different event, identical
    # predicate composition.
    line = (
        "When this creature enters, if there are four or more card "
        "types among cards in your graveyard, draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


def test_intervening_if_delirium_ability_word_prefix_stripped():
    # The ``Delirium —`` ability-word prefix is generic-stripped (no
    # rules consequence per CR 207.2c); the surviving body parses the
    # same way the bare attack form does. Mirrors the ``Landfall —`` /
    # ``Void —`` ability-word strips already exercised by other tests.
    line = (
        "Delirium — Whenever this creature attacks, if there are four "
        "or more card types among cards in your graveyard, draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


def test_intervening_if_delirium_ability_word_prefix_on_etb():
    # ``Delirium — When this creature enters, …`` shape — the
    # ability-word prefix is strip-agnostic to ``When`` vs ``Whenever``.
    line = (
        "Delirium — When this creature enters, if there are four or "
        "more card types among cards in your graveyard, draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


# ---------- _parse_trailing_if_phrase / trailing-``if`` shape ----------


def test_trailing_if_phrasebook_includes_delirium():
    # The trailing-rider phrasing differs from the middle-clause
    # phrasing — ``you have ...`` instead of ``there are ...`` — but
    # the predicate is the same.
    phrase = "you have four or more card types among cards in your graveyard"
    assert _parse_trailing_if_phrase(phrase) == StateBound(Delirium())


def test_trailing_if_phrasebook_strips_surrounding_whitespace():
    phrase = (
        "  you have four or more card types among cards in your "
        "graveyard  "
    )
    assert _parse_trailing_if_phrase(phrase) == StateBound(Delirium())


def test_trailing_if_phrasebook_rejects_unknown_phrase():
    assert _parse_trailing_if_phrase("the moons of endor are aligned") is None


def test_trailing_if_delirium_attack_predicate():
    # Tail-rider shape: ``Whenever <cond>, <effect> if <gate>.``
    # Mechanically equivalent to the middle-clause intervening-if
    # form but with the gate after the effect.
    line = (
        "Whenever this creature attacks, draw a card if you have "
        "four or more card types among cards in your graveyard."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


def test_trailing_if_delirium_etb_predicate():
    line = (
        "When this creature enters, draw a card if you have four or "
        "more card types among cards in your graveyard."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


def test_trailing_if_delirium_with_ability_word_prefix():
    # The ``Delirium —`` ability-word strips through the existing
    # generic helper before the trailing-if regex runs.
    line = (
        "Delirium — Whenever this creature attacks, draw a card "
        "if you have four or more card types among cards in your "
        "graveyard."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ATTACKS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


def test_trailing_if_unknown_gate_falls_through():
    # The trailing-if regex over-matches (any trigger line with a
    # sub-clause beginning `` if `` will hit it). When the gate phrase
    # isn't in ``_TRAILING_IF_PHRASES`` we fall through to the bare
    # ``_TRIGGERED_ABILITY_LINE`` rather than rejecting outright —
    # distinct from the middle-clause discipline. Here we feed a line
    # whose ``if`` is part of an effect body that the bare path also
    # can't claim, and check it returns None (rather than raising).
    line = (
        "Whenever this creature attacks, draw a card if the moons "
        "of endor are aligned."
    )
    assert _parse_triggered_ability_line(line) is None


def test_trailing_if_unknown_condition_rejected():
    # Recognized gate but unknown trigger condition — same rejection
    # discipline as the middle-clause path. A future regression that
    # accidentally builds a trigger with ``event=None`` would surface
    # here.
    line = (
        "Whenever this creature waltzes, draw a card if you have "
        "four or more card types among cards in your graveyard."
    )
    assert _parse_triggered_ability_line(line) is None


def test_trailing_if_unparseable_effect_rejected():
    # Recognized gate AND recognized trigger condition — but the
    # effect body doesn't claim. Reject; don't synthesize a
    # half-resolved trigger.
    line = (
        "Whenever this creature attacks, do something undefined "
        "if you have four or more card types among cards in your "
        "graveyard."
    )
    assert _parse_triggered_ability_line(line) is None


def test_middle_clause_takes_precedence_over_trailing():
    # A line with BOTH a middle-clause ``, if <gate>,`` AND a tail
    # ``... if <other-gate>.`` claims through the middle-clause path
    # first (regex order in ``_parse_triggered_ability_line``). This
    # combination doesn't appear in print today; the test guards the
    # ordering invariant.
    line = (
        "When this creature enters, if there are four or more card "
        "types among cards in your graveyard, draw a card."
    )
    assert _parse_triggered_ability_line(line) == TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf() & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


# ---------- phrasebook integrity ----------


def test_delirium_phrase_lives_in_intervening_if_table():
    phrase = "there are four or more card types among cards in your graveyard"
    assert phrase in _INTERVENING_IF_PHRASES
    assert _INTERVENING_IF_PHRASES[phrase] == StateBound(Delirium())


def test_delirium_phrase_lives_in_trailing_if_table():
    phrase = "you have four or more card types among cards in your graveyard"
    assert phrase in _TRAILING_IF_PHRASES
    assert _TRAILING_IF_PHRASES[phrase] == StateBound(Delirium())


def test_trailing_if_phrasebook_keys_map_to_predicates():
    # Defensive integrity check — mirrors the equivalent assertion on
    # the intervening-if phrasebook. A future addition that
    # accidentally lands a tuple or string in the table fails here.
    for phrase, pred in _TRAILING_IF_PHRASES.items():
        assert isinstance(pred, Predicate), (
            f"trailing phrase {phrase!r} maps to non-Predicate {pred!r}"
        )


# ---------- predicate evaluation ----------


@dataclass
class _FakePrinting:
    card_types: tuple[str, ...] = ()


@dataclass
class _FakeCard:
    printing: _FakePrinting = field(default_factory=_FakePrinting)


@dataclass
class _FakePlayer:
    id: int = 0
    graveyard: list[_FakeCard] = field(default_factory=list)


@dataclass
class _FakeState:
    players: list[_FakePlayer] = field(default_factory=list)

    def player_by_id(self, pid: int) -> _FakePlayer:
        for p in self.players:
            if p.id == pid:
                return p
        raise KeyError(pid)


@dataclass
class _StubSource:
    controller_id: int = 0


def _make_state(types_in_yard: list[tuple[str, ...]]) -> _FakeState:
    """Build a one-player ``_FakeState`` whose graveyard contains one
    fake card per entry in ``types_in_yard``. Each entry is a tuple
    of card types for that one card."""
    p = _FakePlayer(id=0, graveyard=[
        _FakeCard(printing=_FakePrinting(card_types=t))
        for t in types_in_yard
    ])
    return _FakeState(players=[p])


def test_delirium_false_with_empty_graveyard():
    state = _make_state([])
    assert StateBound(Delirium()).matches(state, {}, _StubSource()) is False


def test_delirium_false_with_three_distinct_types():
    state = _make_state([
        ("Creature",),
        ("Land",),
        ("Sorcery",),
    ])
    assert StateBound(Delirium()).matches(state, {}, _StubSource()) is False


def test_delirium_true_with_four_distinct_types():
    state = _make_state([
        ("Creature",),
        ("Land",),
        ("Sorcery",),
        ("Instant",),
    ])
    assert StateBound(Delirium()).matches(state, {}, _StubSource()) is True


def test_delirium_true_with_more_than_four_distinct_types():
    state = _make_state([
        ("Creature",),
        ("Land",),
        ("Sorcery",),
        ("Instant",),
        ("Enchantment",),
        ("Artifact",),
    ])
    assert StateBound(Delirium()).matches(state, {}, _StubSource()) is True


def test_delirium_counts_distinct_types_not_card_count():
    # Eight Creature cards in the yard still count as ONE type. This
    # is the most common false-positive shape — guard against it.
    state = _make_state([("Creature",)] * 8)
    assert StateBound(Delirium()).matches(state, {}, _StubSource()) is False


def test_delirium_multitype_card_contributes_all_its_types():
    # A single Artifact Creature card supplies BOTH Artifact and
    # Creature toward the threshold. With one such card plus a Land
    # and a Sorcery the controller has four distinct types from
    # three cards.
    state = _make_state([
        ("Artifact", "Creature"),
        ("Land",),
        ("Sorcery",),
    ])
    assert StateBound(Delirium()).matches(state, {}, _StubSource()) is True


def test_delirium_ignores_supertypes_and_subtypes():
    # Only card types from the CR 205.2a taxonomy count — supertypes
    # (Legendary, Snow, Basic) and subtypes (Goblin, Equipment) live
    # on different attributes and don't appear in ``card_types``.
    # The ``card_types`` field already segregates them, so a future
    # change that mistakenly reads ``supertypes`` would surface as
    # a regression here. Three real types + a card that supplies
    # only "Legendary" / "Goblin" should NOT cross the threshold.
    state = _make_state([
        ("Creature",),
        ("Land",),
        ("Sorcery",),
        # Non-card-type tokens — even if a future bug ingested these
        # into ``card_types``, the ``Delirium`` predicate filters
        # against the canonical type set and ignores them.
        ("Legendary", "Goblin", "Basic"),
    ])
    assert StateBound(Delirium()).matches(state, {}, _StubSource()) is False


def test_delirium_short_circuits_on_threshold_hit():
    # If a graveyard already has four distinct types in the first few
    # cards, the predicate must return without scanning the rest. We
    # observe this indirectly by mixing in an entry whose card_types
    # tuple is a non-iterable sentinel — the predicate raising would
    # mean it didn't short-circuit before reaching that entry. The
    # test passes both because no exception is raised AND the
    # predicate returns True.
    @dataclass
    class _Bomb:
        printing: object = field(default_factory=lambda: _FakePrinting(
            card_types=("Creature",),
        ))

    p = _FakePlayer(id=0, graveyard=[
        _FakeCard(printing=_FakePrinting(card_types=("Creature",))),
        _FakeCard(printing=_FakePrinting(card_types=("Land",))),
        _FakeCard(printing=_FakePrinting(card_types=("Sorcery",))),
        _FakeCard(printing=_FakePrinting(card_types=("Instant",))),
        _Bomb(),  # would be reached only if we don't short-circuit
    ])
    state = _FakeState(players=[p])
    assert StateBound(Delirium()).matches(state, {}, _StubSource()) is True


def test_delirium_uses_source_controller_not_other_players():
    # The graveyard scan keys off ``source.controller_id``; another
    # player's graveyard with four types must not flip the predicate.
    p_self = _FakePlayer(id=0, graveyard=[
        _FakeCard(printing=_FakePrinting(card_types=("Creature",))),
    ])
    p_opp = _FakePlayer(id=1, graveyard=[
        _FakeCard(printing=_FakePrinting(card_types=("Creature",))),
        _FakeCard(printing=_FakePrinting(card_types=("Land",))),
        _FakeCard(printing=_FakePrinting(card_types=("Sorcery",))),
        _FakeCard(printing=_FakePrinting(card_types=("Instant",))),
    ])
    state = _FakeState(players=[p_self, p_opp])
    src = _StubSource(controller_id=0)
    assert StateBound(Delirium()).matches(state, {}, src) is False
    # Re-host the source under the opponent and the predicate flips.
    src_opp = _StubSource(controller_id=1)
    assert StateBound(Delirium()).matches(state, {}, src_opp) is True


def test_delirium_false_when_source_controller_unknown():
    # Defensive: a source whose controller isn't a known player should
    # evaluate False rather than raise. Mirrors the
    # ``SpellsCastThisTurn`` predicate's KeyError-swallow behavior.
    state = _make_state([
        ("Creature",),
        ("Land",),
        ("Sorcery",),
        ("Instant",),
    ])
    src = _StubSource(controller_id=99)
    assert StateBound(Delirium()).matches(state, {}, src) is False


# ---------- Delirium — At the beginning of <when> parser support ----------
#
# bespoke-delirium adds ``_DELIRIUM_PREFIX`` / ``_DELIRIUM_AT_BEGINNING_
# GATE_PREFIX`` to ``triggers.py`` so any ``Delirium — At the beginning of
# <when>, if there are four or more card types..., <effect>.`` line whose
# effect body is expressible by the unified grammar can claim end-to-end.
# Osseous Sticktwister's multi-sentence body is now decomposed into the
# generic ``EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage``
# primitive (audit-per-opponent-choice), so the entire Delirium-gated
# end-step trigger now routes through the unified grammar — no
# remaining ``_BESPOKE_TRIGGER_LINES`` entry. The synthetic-draw test
# below still pins the gate-strip + predicate-compose mechanics.


def test_delirium_at_beginning_of_end_step_parses_with_draw_effect():
    """``Delirium — At the beginning of your end step, if there are four
    or more card types among cards in your graveyard, draw a card.``
    claims end-to-end through the unified grammar:
    - ``Delirium — `` is detected as a prefix in ``_parse_triggered_ability_line``
    - ``_DELIRIUM_AT_BEGINNING_GATE_PREFIX`` is stripped in
      ``_parse_at_the_beginning_of_line``
    - ``StateBound(Delirium())`` is AND-ed onto ``Controlled(YOU)``
    - ``draw a card`` parses via the standard effect grammar.
    """
    from mtgz_ir.cards.base import DrawCards, TriggeredAbility
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
    from mtgz_ir.effects.predicates import Controlled, Event, YOU

    line = (
        "Delirium — At the beginning of your end step, if there are "
        "four or more card types among cards in your graveyard, draw a card."
    )
    result = _parse_triggered_ability_line(line)
    assert result == TriggeredAbility(
        event=Event.BEGINNING_OF_END_STEP,
        predicate=Controlled(YOU) & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


def test_delirium_at_beginning_of_upkeep_parses_with_draw_effect():
    """Same shape on ``At the beginning of your upkeep`` — the Delirium
    gate wires correctly for any ``_AT_BEGINNING_OF_PHRASES`` time phrase."""
    from mtgz_ir.cards.base import DrawCards, TriggeredAbility
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line
    from mtgz_ir.effects.predicates import Controlled, Event, YOU

    line = (
        "Delirium — At the beginning of your upkeep, if there are "
        "four or more card types among cards in your graveyard, draw a card."
    )
    result = _parse_triggered_ability_line(line)
    assert result == TriggeredAbility(
        event=Event.BEGINNING_OF_UPKEEP,
        predicate=Controlled(YOU) & StateBound(Delirium()),
        effect=DrawCards(amount=1),
    )


def test_delirium_at_beginning_rejected_when_gate_missing():
    """A ``Delirium — At the beginning of your end step, <effect>.`` line
    that does NOT carry the Delirium gate phrase is rejected outright —
    the parser must not silently drop the gate."""
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line

    # Body starts directly with the effect, no gate clause.
    line = (
        "Delirium — At the beginning of your end step, draw a card."
    )
    assert _parse_triggered_ability_line(line) is None


def test_delirium_at_beginning_rejected_when_effect_unparseable():
    """``Delirium — At the beginning of your end step, if there are four
    or more card types..., <bespoke-body>.`` is rejected when the body
    isn't expressible by the unified effect grammar."""
    from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line

    line = (
        "Delirium — At the beginning of your end step, if there are "
        "four or more card types among cards in your graveyard, do something "
        "completely undefined."
    )
    assert _parse_triggered_ability_line(line) is None
