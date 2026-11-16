"""Tests for batch-20 creature additions covering the
multi-trigger / trigger+activated cluster.

This batch grows the parser by claims that pair a triggered ability
with a second card-level component — either an activated ability on
the same printing or a second trigger arm. Each addition lands an
isolated piece of grammar; the per-card parity gate in
``tests/test_parser_parity.py`` enforces full ``CardPrinting``
equivalence so this file only needs the grammar-level assertions
plus end-to-end claim checks.

New effect-grammar primitives:

* ``put target card from a graveyard on the bottom of its owner's
  library`` → ``PutCardFromGraveyardOnBottomOfLibrary()``. Chrome
  Companion's tap-mana activated ability. Surface form is post-
  normalization (the ``it `` prefix-strip in
  ``_normalize_creature_effect_phrase`` doesn't fire because the
  phrase starts with the verb ``put``); the ``its`` pronoun in the
  effect tail survives intact, mirroring the ``put its counters
  on …`` precedent.

Extended primitives:

* ``(?:this creature )?deals N damage to each opponent`` — the bare
  shape was already wired; this batch extends the regex with the
  optional ``this creature `` self-reference so trigger bodies that
  spell out the source explicitly parse. Used by Weftstalker
  Ardent's another-creature-or-artifact ETB trigger.

New printing-keyword grammar:

* ``Ward—Discard a card.`` / ``Ward—Discard a card at random.`` —
  em-dash discard-cost ward riders. Tagged return string from
  ``_parse_ward_discard_line`` (``"chosen"`` / ``"random"``) maps
  1:1 to the matching ``make_creature`` boolean kwarg
  (``ward_discard_chosen`` / ``ward_discard_random``). Unlocks
  Sunset Saboteur (chosen variant) — the random variant is
  forward-looking grammar coverage; the only EoE printing that
  carries it (Alpharael, Stonechosen) is unclaimable today for
  unrelated reasons (the Void-rider trigger-effect grammar isn't
  yet wired for half-life-loss).

New trigger-condition phrases:

* ``this creature or another creature or artifact you control dies``
  — Susurian Voidborn's two-arm-collapsed-to-one trigger condition.
  Distinct from the multi-arm ``_COMPOUND_PREDICATE_TRIGGER_PHRASES``
  (which produces one ``TriggeredAbility`` per arm) — this lookup
  produces a single ability whose predicate covers both arms,
  mirroring the static-registry layout for Susurian Voidborn. The
  inclusive wording (``this creature or another …``) means no
  ``~IsSelf()`` exclusion: the source's own death satisfies the
  predicate via the dispatcher's ``extra_observers=[dead]`` plumbing.

Newly-claimed creatures:

* **Chrome Companion** — ``Whenever this creature becomes tapped,
  you gain 1 life. {2}, {T}: Put target card from a graveyard on
  the bottom of its owner's library.`` The becomes-tapped + gain-1
  trigger half was already wired (BECOMES_TAPPED + IsSelf →
  GainLife); the activated half required the new graveyard-bottom
  primitive. Subtype ``Dog`` is added to the static registry per
  the parser-is-right rule (Scryfall prints the subtype).

* **Weftstalker Ardent** — ``Whenever another creature or artifact
  you control enters, this creature deals 1 damage to each
  opponent. Warp {R}.`` Trigger condition was already in the
  predicate phrasebook (batch 9); the extended ``this creature
  deals N damage to each opponent`` regex closes the last gap.
  Static drift fix: real card is 2/3 ``Drix Artificer`` (the
  registry was carrying the printed power/toughness reversed plus
  no subtypes — corrected per the parser-is-right rule).

* **Susurian Voidborn** — ``Whenever this creature or another
  creature or artifact you control dies, target opponent loses 1
  life and you gain 1 life. Warp {B}.`` Trigger condition lands via
  the new collapsed-arm phrasebook entry; the effect body splits
  through the unified compound-effect splitter into a
  ``Sequence(TargetOpponentLosesLife, GainLife)``. The static
  registry's bespoke ``TargetOpponentLosesNYouGainN`` engine class
  (still defined and still test-covered) is retired from the
  Susurian static in favor of the parser-produced ``Sequence`` —
  same gameplay outcome, no new effect class. Subtypes
  ``Vampire Soldier`` are added per the parser-is-right rule.

* **Sunset Saboteur** — ``Menace. Ward—Discard a card. Whenever
  this creature attacks, put a +1/+1 counter on target creature
  an opponent controls.`` All three lines were previously
  recognized except the em-dash ward-discard rider; the new
  ``_parse_ward_discard_line`` shape unblocks the printing.
  Subtypes ``Human Rogue`` are added per the parser-is-right rule.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    DealDamageEachOpponent,
    GainLife,
    PutCardFromGraveyardOnBottomOfLibrary,
    Sequence,
    TargetOpponentLosesLife,
    TriggeredAbility,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.effects import _parse_effect_phrase
from mtgz_ir.cards.parser.printing_keywords import _parse_ward_discard_line
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.parser.triggers import (
    _parse_collapsed_predicate_trigger_phrase,
    _parse_triggered_ability_line,
)
from mtgz_ir.effects.object_predicates import (
    IsArtifact,
    IsCreature,
)
from mtgz_ir.effects.predicates import (
    Controlled,
    Event,
    IsSelf,
    Subject,
    YOU,
)


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- effect-phrase grammar ----------

def test_parse_put_card_from_graveyard_on_bottom_of_library():
    # New primitive — untyped grave-target across all players. The
    # ``its`` pronoun in the tail survives the creature-effect
    # normalizer (which only strips a leading ``it `` subject), so the
    # full surface form lands at the regex unchanged.
    phrase = (
        "put target card from a graveyard on the bottom of its "
        "owner's library"
    )
    assert _parse_effect_phrase(phrase) == (
        PutCardFromGraveyardOnBottomOfLibrary()
    )


def test_parse_put_card_from_graveyard_on_bottom_rejects_tail_drift():
    # A surface-text variant the parser must NOT silently coerce — the
    # grammar is anchored end-to-end via ``$``. A future printing whose
    # tail differs (``...top of its owner's library``) needs its own
    # primitive rather than collapsing onto this one.
    phrase = (
        "put target card from a graveyard on the top of its "
        "owner's library"
    )
    assert _parse_effect_phrase(phrase) is None


# ---------- activated-line end-to-end ----------

def test_chrome_companion_activated_ability_line():
    # End-to-end: the cost ``{2}, {T}`` decomposes via the bag-merge
    # classifier (one mana piece + one tap), the effect parses through
    # the new primitive, and the produced ``ActivatedAbility`` has no
    # mana-ability flag (``PutCardFromGraveyardOnBottomOfLibrary``
    # targets — CR 605 disqualifies it from the mana-ability fast path
    # regardless of cost).
    line = (
        "{2}, {T}: Put target card from a graveyard on the bottom "
        "of its owner's library."
    )
    parsed = _parse_activated_ability_line(line)
    assert parsed == ActivatedAbility(
        cost=ActivationCost(
            mana=parse_mana_cost("{2}"),
            mana_str="{2}",
            tap_self=True,
        ),
        effect=PutCardFromGraveyardOnBottomOfLibrary(),
    )


# ---------- trigger-line end-to-end ----------

def test_chrome_companion_becomes_tapped_trigger_line():
    # The becomes-tapped + gain-1 trigger half. Already wired in the
    # predicate-trigger phrasebook prior to this batch; asserted here
    # to anchor Chrome Companion's full claim narrative in a single
    # test file.
    line = "Whenever this creature becomes tapped, you gain 1 life."
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.BECOMES_TAPPED,
        predicate=IsSelf(),
        effect=GainLife(amount=1),
    )


# ---------- claim test (end-to-end via the parser source) ----------

def test_chrome_companion_claimed_by_parser():
    # End-to-end: the parser source's ``claimed_names`` includes
    # Chrome Companion. Per-card parity is enforced by
    # ``tests/test_parser_parity.py`` so a successful claim already
    # implies static-registry equivalence.
    assert "Chrome Companion" in _claimed_names()


# ---------- Weftstalker Ardent ----------

def test_parse_this_creature_deals_n_damage_to_each_opponent():
    # The extended self-prefixed shape: ``this creature`` resolves to
    # the source at fire time. Bare and prefixed forms must both map
    # to the same engine class — the prefix is surface noise.
    assert _parse_effect_phrase(
        "this creature deals 1 damage to each opponent"
    ) == DealDamageEachOpponent(amount=1)
    assert _parse_effect_phrase(
        "deals 1 damage to each opponent"
    ) == DealDamageEachOpponent(amount=1)


def test_weftstalker_ardent_etb_trigger_line():
    # End-to-end: condition phrase ``another creature or artifact you
    # control enters`` (batch-9 predicate-phrasebook entry) feeds the
    # newly-extended self-prefixed effect grammar. Mirrors the static
    # registry's Weftstalker entry exactly.
    line = (
        "Whenever another creature or artifact you control enters, "
        "this creature deals 1 damage to each opponent."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.ENTERS,
        predicate=(
            Subject(None, IsCreature() | IsArtifact())
            & Controlled(YOU)
            & ~IsSelf()
        ),
        effect=DealDamageEachOpponent(amount=1),
    )


def test_weftstalker_ardent_claimed_by_parser():
    assert "Weftstalker Ardent" in _claimed_names()


# ---------- Susurian Voidborn ----------

def test_parse_collapsed_creature_or_artifact_dies_phrase():
    # New collapsed-arm trigger phrase: ``this creature or another
    # creature or artifact you control dies`` produces ONE
    # ``(Event, Predicate)`` tuple (not a multi-arm tuple), and the
    # predicate omits ``~IsSelf()`` because the leading
    # ``this creature`` arm explicitly includes self.
    assert _parse_collapsed_predicate_trigger_phrase(
        "this creature or another creature or artifact you control dies"
    ) == (
        Event.DIES,
        Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU),
    )


def test_parse_collapsed_phrase_unknown_returns_none():
    # The lookup is exact-match — a bare ``creature dies`` shape that
    # isn't in the table must surface as ``None`` so the caller can
    # try the single-phrase or compound tables.
    assert _parse_collapsed_predicate_trigger_phrase(
        "a creature dies"
    ) is None


def test_susurian_voidborn_trigger_line_end_to_end():
    # End-to-end: collapsed trigger phrase + compound effect body
    # decompose into a single ``TriggeredAbility`` whose effect is a
    # ``Sequence`` of the two primitives. Mirrors the static
    # registry's Susurian Voidborn entry exactly.
    line = (
        "Whenever this creature or another creature or artifact "
        "you control dies, target opponent loses 1 life and "
        "you gain 1 life."
    )
    parsed = _parse_triggered_ability_line(line)
    assert parsed == TriggeredAbility(
        event=Event.DIES,
        predicate=Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU),
        effect=Sequence(
            effects=(
                TargetOpponentLosesLife(amount=1),
                GainLife(amount=1),
            ),
        ),
    )


def test_susurian_voidborn_claimed_by_parser():
    assert "Susurian Voidborn" in _claimed_names()


# ---------- Sunset Saboteur (Ward—Discard rider) ----------

def test_parse_ward_discard_line_chosen():
    # Sunset Saboteur's printed Ward rider — chosen-discard variant.
    # The recognizer returns the tag string ``"chosen"`` so the
    # caller can map straight to the matching ``make_creature``
    # kwarg. Trailing period is required (matches Scryfall's
    # punctuation convention for printing-level keyword lines).
    assert _parse_ward_discard_line("Ward—Discard a card.") == "chosen"


def test_parse_ward_discard_line_random():
    # Forward-looking — the random-discard variant prints on
    # Alpharael, Stonechosen. The grammar-level recognizer should
    # accept both shapes even though Stonechosen itself isn't
    # claimable today (the Void-rider effect grammar gap).
    assert _parse_ward_discard_line(
        "Ward—Discard a card at random."
    ) == "random"


def test_parse_ward_discard_line_rejects_other_shapes():
    # Bare ``Ward {2}`` — that's the mana-cost shape handled by
    # ``_parse_ward_line``; the discard recognizer must NOT claim
    # it. A future ``Ward—Pay 2 life.`` printing also stays out of
    # scope (only the discard variants are wired).
    assert _parse_ward_discard_line("Ward {2}") is None
    assert _parse_ward_discard_line("Ward—Pay 2 life.") is None
    assert _parse_ward_discard_line("Ward—Discard two cards.") is None


def test_sunset_saboteur_claimed_by_parser():
    assert "Sunset Saboteur" in _claimed_names()
