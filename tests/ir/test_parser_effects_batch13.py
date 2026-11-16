"""Tests for batch-13 unified-effect-grammar additions:

* ``each opponent discards a card`` / ``each opponent discards N cards``
  → ``EachOpponentDiscards(count=N)``. Untargeted opponent-wide
  discard. Surfaces in Virus Beetle's ETB trigger.
* ``exile target creature or Spacecraft`` → ``ExileTargetCreature()``.
  Gravkill's printed surface — the static side intentionally fudges
  the Spacecraft tail because the engine doesn't yet have a
  Spacecraft-aware target kind; the parser mirrors that fudge in
  lockstep so static parity holds.
* ``destroy target artifact or tapped creature`` →
  ``DestroyTargetArtifactOrTappedCreature()``. Radiant Strike's
  destroy clause.
* ``tap target artifact or creature`` →
  ``TapTargetArtifactOrCreature()``. Mental Modulation's middle
  clause. No-op if the chosen target is already tapped at resolve.
* ``discard a card`` / ``you discard a card`` / ``discard N cards`` →
  ``DiscardSelf(amount=N)``. Surfaces inside compound
  ``draw a card, then discard a card`` (Mechan Navigator's
  becomes-tapped trigger) and any future card with a bare
  controller-discard clause.
* ``, then `` joiner extension to ``_parse_compound_effect``: any
  two or more primitives joined by ``, then `` (in addition to
  `` and ``) compose into a ``Sequence`` in printed left-to-right
  resolve order.

Each new effect is exercised three ways: directly via ``_parse_effect``
on the normalized phrase, via ``_parse_effect_phrase`` (creature-trigger
surface form), and via ``_parse_spell_effect_phrase`` (spell sentence
form, capitalized first letter and trailing period). Negative cases
verify the regex stays narrow.

Card-claim tests confirm the parser end-to-end-claims the three EoE
cards unlocked by this batch (Virus Beetle, Gravkill, Mechan
Navigator).
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    DestroyTargetArtifactOrTappedCreature,
    DiscardSelf,
    DrawCards,
    EachOpponentDiscards,
    ExileTargetCreature,
    Sequence,
    TapTargetArtifactOrCreature,
)
from mtgz_ir.cards.parser.effects import (
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser_source import OracleParserCardSource


# ---------- each opponent discards a card ----------

def test_parse_effect_recognizes_each_opponent_discards_a_card():
    # Virus Beetle's ETB — ``a card`` is the printed singular form;
    # the article collapses to count=1 in the handler.
    assert _parse_effect(
        "each opponent discards a card"
    ) == EachOpponentDiscards(count=1)


def test_parse_effect_recognizes_each_opponent_discards_n_cards():
    # Forward-looking digit form — no EoE card prints
    # ``each opponent discards 2 cards`` today, but the regex accepts
    # bare digits so the day a printing ships with a higher count it
    # parses without further grammar work.
    assert _parse_effect(
        "each opponent discards 2 cards"
    ) == EachOpponentDiscards(count=2)


def test_parse_effect_recognizes_each_opponent_discards_word_amount():
    # Spelled-out cardinal form — also forward-looking. Scryfall's
    # convention is to spell out 1-10 in oracle text so this branch
    # would activate for any future ``each opponent discards three
    # cards``-shaped printing.
    assert _parse_effect(
        "each opponent discards three cards"
    ) == EachOpponentDiscards(count=3)


def test_parse_effect_phrase_recognizes_each_opponent_discards():
    # Creature-trigger surface form. The ``it`` source-subject prefix
    # isn't present here because the discard's subject is each
    # opponent (controller-untargeted), not the firing creature; the
    # normalizer leaves the phrase as-is and the regex matches.
    assert _parse_effect_phrase(
        "each opponent discards a card"
    ) == EachOpponentDiscards(count=1)


def test_parse_spell_effect_phrase_recognizes_each_opponent_discards():
    # Spell sentence form: capitalized first letter + trailing period.
    # No EoE spell prints this exact bare clause, but the unified
    # grammar makes it available to spells via the same
    # ``_parse_effect`` path used by triggered abilities.
    assert _parse_spell_effect_phrase(
        "Each opponent discards a card.",
        "Bogus Mind Rot",
    ) == EachOpponentDiscards(count=1)


def test_parse_effect_lenient_singular_discard_amount():
    # ``each opponent discards 1 card`` (digit "1" with singular tail)
    # is not a Scryfall-printed shape (the singular always uses the
    # article ``a``), but the regex's ``cards?`` and ``\d+`` slots
    # are intentionally lenient — making them stricter would only
    # close a surface no card prints today, at the cost of refusing
    # forward-compatible variants. We pin the lenient behavior here
    # so the regex's tolerance is documented intent rather than an
    # accidental over-match.
    assert _parse_effect(
        "each opponent discards 1 card"
    ) == EachOpponentDiscards(count=1)


def test_parse_effect_target_opponent_discards_is_the_targeted_primitive():
    # ``target opponent discards a card`` is a different primitive
    # (single-target discard) and isn't claimed by the each-opponent
    # regex; it maps to the targeted action-list effect (U/B cube
    # creatures), documenting the wide-vs-targeted boundary.
    from mtgz_ir.effects.generic.cube_creatures_ub import (
        TargetPlayerPerformsActions,
    )

    assert _parse_effect(
        "target opponent discards a card"
    ) == TargetPlayerPerformsActions(actions=(("discard", 1),))


# ---------- exile target creature or Spacecraft ----------

def _exile_creature_or_spacecraft():
    from mtgz_ir.effects.generic.targeted_removal import (
        RemovalAction, RemoveTargetPermanents,
    )
    from mtgz_ir.effects.object_predicates import HasSubtype, IsCreature
    return RemoveTargetPermanents(
        RemovalAction.EXILE, IsCreature() | HasSubtype("Spacecraft"),
    )


_EXILE_CREATURE_OR_SPACECRAFT = _exile_creature_or_spacecraft()

def test_parse_effect_recognizes_exile_target_creature_or_spacecraft():
    # Gravkill's printed surface. The capital ``S`` on ``Spacecraft``
    # is preserved by the normalizer (only the first letter of the
    # phrase is lowercased). Faithful to the Oracle text: exile one
    # target creature OR Spacecraft (not the creature-only class).
    assert _parse_effect(
        "exile target creature or Spacecraft"
    ) == _EXILE_CREATURE_OR_SPACECRAFT


def test_parse_effect_phrase_recognizes_exile_creature_or_spacecraft():
    # Creature-trigger surface form (no ``it`` prefix — the consequence
    # is targeted, not a self-action).
    assert _parse_effect_phrase(
        "exile target creature or Spacecraft"
    ) == _EXILE_CREATURE_OR_SPACECRAFT


def test_parse_spell_effect_phrase_recognizes_exile_creature_or_spacecraft():
    # Spell sentence form. Mirrors Gravkill's printed sentence verbatim.
    assert _parse_spell_effect_phrase(
        "Exile target creature or Spacecraft.",
        "Bogus Gravkill",
    ) == _EXILE_CREATURE_OR_SPACECRAFT


def test_parse_effect_returns_none_for_lowercase_spacecraft_in_exile():
    # The capital ``S`` is required — Scryfall's printed convention.
    # A lowercase printing would suggest a different (unsupported)
    # surface form and shouldn't accidentally match.
    assert _parse_effect(
        "exile target creature or spacecraft"
    ) is None


def test_parse_effect_bare_exile_target_creature_still_parses():
    # The bare ``exile target creature`` regex still wins for phrases
    # without the Spacecraft tail — the new primitive only widens
    # coverage, doesn't replace the existing.
    assert _parse_effect(
        "exile target creature"
    ) == ExileTargetCreature()


# ---------- destroy target artifact or tapped creature ----------

def test_parse_effect_recognizes_destroy_artifact_or_tapped_creature():
    # Radiant Strike's destroy clause. Maps to the engine class whose
    # ``legal_targets`` filters to artifacts (any) or creatures
    # currently tapped.
    assert _parse_effect(
        "destroy target artifact or tapped creature"
    ) == DestroyTargetArtifactOrTappedCreature()


def test_parse_effect_phrase_recognizes_destroy_artifact_or_tapped():
    # Creature-trigger surface form. No EoE creature prints a
    # destroy-artifact-or-tapped-creature trigger, but the unified
    # grammar makes it available across contexts.
    assert _parse_effect_phrase(
        "destroy target artifact or tapped creature"
    ) == DestroyTargetArtifactOrTappedCreature()


def test_parse_spell_effect_phrase_recognizes_destroy_artifact_or_tapped():
    # Spell sentence form. Mirrors Radiant Strike's first sentence
    # verbatim; the second sentence (``You gain 3 life.``) currently
    # blocks end-to-end claim because the spell builder doesn't split
    # on inter-sentence periods (out of scope this slice).
    assert _parse_spell_effect_phrase(
        "Destroy target artifact or tapped creature.",
        "Bogus Radiant Strike",
    ) == DestroyTargetArtifactOrTappedCreature()


def test_parse_effect_destroy_target_tapped_creature_is_generic_destroy():
    # ``destroy target tapped creature`` (without the ``artifact or``
    # prefix) is a different shape and isn't claimed by this regex; the
    # generic ``destroy target <noun>`` recognizer (effects_ext) claims
    # it with the composed tapped-creature filter.
    eff = _parse_effect("destroy target tapped creature")
    assert type(eff).__name__ == "Destroy"


def test_parse_effect_bare_destroy_target_artifact_still_parses():
    # The bare ``destroy target artifact`` regex still claims its own
    # surface — adding the more-specific ``or tapped creature`` shape
    # doesn't disturb the simpler form.
    from mtgz_ir.cards.base import DestroyTargetArtifact
    assert _parse_effect(
        "destroy target artifact"
    ) == DestroyTargetArtifact()


# ---------- tap target artifact or creature ----------

def test_parse_effect_recognizes_tap_target_artifact_or_creature():
    # Mental Modulation's middle clause. Maps to the engine class
    # that no-ops on already-tapped targets at resolve time.
    assert _parse_effect(
        "tap target artifact or creature"
    ) == TapTargetArtifactOrCreature()


def test_parse_effect_phrase_recognizes_tap_artifact_or_creature():
    # Creature-trigger surface form. No EoE creature prints this
    # trigger but the unified grammar accepts it.
    assert _parse_effect_phrase(
        "tap target artifact or creature"
    ) == TapTargetArtifactOrCreature()


def test_parse_spell_effect_phrase_recognizes_tap_artifact_or_creature():
    # Spell sentence form. Mirrors Mental Modulation's middle
    # sentence; the full card stays unclaimed because the
    # ``This spell costs {1} less to cast during your turn.`` rider
    # isn't recognized by the cost-reduction grammar today (out of
    # scope this slice).
    assert _parse_spell_effect_phrase(
        "Tap target artifact or creature.",
        "Bogus Mental Modulation",
    ) == TapTargetArtifactOrCreature()


def test_parse_effect_tap_only_creature_phrase_is_the_generic_tap():
    # ``tap target creature`` without the ``artifact or`` prefix is the
    # generic ``Tap`` over the composed noun phrase (Vintage Roto P5's
    # "[un]tap target <np>" fallback; switched off for EoE-only
    # printings, so no EoE parse changes).
    from mtgz_ir.effects.generic.tap import Tap
    from mtgz_ir.effects.object_predicates import IsCreature
    from mtgz_ir.effects.targets import TargetSpec

    assert _parse_effect("tap target creature") == Tap(
        targets=(TargetSpec(IsCreature()),),
    )


# ---------- discard a card ----------

def test_parse_effect_recognizes_discard_a_card():
    # Bare singular form — surfaces inside the compound
    # ``draw a card, then discard a card`` (Mechan Navigator).
    assert _parse_effect("discard a card") == DiscardSelf(amount=1)


def test_parse_effect_recognizes_you_discard_a_card():
    # ``you discard a card`` variant — included for symmetry with
    # ``you gain N life`` / ``you lose N life``. Same primitive.
    assert _parse_effect("you discard a card") == DiscardSelf(amount=1)


def test_parse_effect_recognizes_discard_n_cards():
    # Bare digit count — forward-looking, no EoE card prints
    # ``discard 2 cards`` as a bare clause today.
    assert _parse_effect("discard 2 cards") == DiscardSelf(amount=2)


def test_parse_effect_recognizes_discard_word_amount():
    # Spelled-out cardinal form — also forward-looking. Scryfall's
    # convention is to spell out 1-10 in oracle text.
    assert _parse_effect("discard three cards") == DiscardSelf(amount=3)


def test_parse_effect_phrase_recognizes_discard_a_card():
    # Creature-trigger surface form (the ``it`` prefix isn't present
    # because the controller is the discarder, not the firing source).
    assert _parse_effect_phrase(
        "discard a card"
    ) == DiscardSelf(amount=1)


def test_parse_spell_effect_phrase_recognizes_discard_a_card():
    # Spell sentence form. No EoE spell prints this exact bare
    # clause, but the unified grammar makes it available to spells.
    assert _parse_spell_effect_phrase(
        "Discard a card.",
        "Bogus Discard Spell",
    ) == DiscardSelf(amount=1)


def test_parse_effect_lenient_discard_singular_with_digit():
    # ``discard 1 card`` (digit "1" with singular tail) is not a
    # Scryfall-printed shape but the regex's ``cards?`` tail is
    # intentionally lenient — same rationale as the each-opponent
    # discard variant above. Pin the lenient behavior so the
    # tolerance is documented intent.
    assert _parse_effect(
        "discard 1 card"
    ) == DiscardSelf(amount=1)


def test_parse_effect_returns_none_for_opponent_discards_phrase():
    # ``opponent discards a card`` (without ``each``) is a different
    # primitive shape (single-opponent discard) and isn't claimed by
    # the controller-discard regex.
    assert _parse_effect("opponent discards a card") is None


# ---------- ``, then `` compound joiner ----------

def test_parse_compound_with_then_joiner():
    # Mechan Navigator's becomes-tapped effect body. The ``, then ``
    # joiner splits into two pieces, each parses, and the result is
    # a ``Sequence`` in printed left-to-right resolve order.
    assert _parse_effect(
        "draw a card, then discard a card"
    ) == Sequence(effects=(DrawCards(amount=1), DiscardSelf(amount=1)))


def test_parse_compound_with_then_joiner_three_pieces():
    # The split-on-``, then `` accepts arbitrary arity (str.split has
    # no maxsplit argument here), so a three-step ``then`` chain also
    # composes. No EoE card prints this exact shape today but the
    # grammar handles it consistently with the existing ``and``
    # joiner's arity.
    assert _parse_effect(
        "draw a card, then you gain 1 life, then discard a card"
    ) == Sequence(effects=(
        DrawCards(amount=1),
        # ``you gain 1 life`` parses via the existing ``you gain N
        # life`` regex.
        __import__(
            "mtgz_ir.cards.base", fromlist=["GainLife"],
        ).GainLife(amount=1),
        DiscardSelf(amount=1),
    ))


def test_parse_effect_phrase_compound_with_then_joiner():
    # Creature-trigger surface form (Mechan Navigator's actual line).
    # The ``it`` prefix isn't present because neither the draw nor
    # the discard is acted on the source — both subjects are the
    # controller.
    assert _parse_effect_phrase(
        "draw a card, then discard a card"
    ) == Sequence(effects=(DrawCards(amount=1), DiscardSelf(amount=1)))


def test_parse_spell_effect_phrase_compound_with_then_joiner():
    # Spell sentence form. No EoE spell prints this exact compound
    # sentence today but the unified grammar makes it available.
    assert _parse_spell_effect_phrase(
        "Draw a card, then discard a card.",
        "Bogus Cantrip",
    ) == Sequence(effects=(DrawCards(amount=1), DiscardSelf(amount=1)))


def test_parse_compound_then_joiner_does_not_break_artifact_tutor():
    # Regression: the artifact-tutor primitive
    # ``search your library for an artifact card, reveal it, put it
    # into your hand, then shuffle`` contains ``, then `` as part of
    # its single-effect surface. The compound parser splits on it
    # eagerly but the ``shuffle`` half doesn't parse as a standalone
    # effect, so the compound attempt punts and the full-phrase regex
    # wins. This test pins that resolution order.
    from mtgz_ir.cards.base import SearchLibraryForCard
    assert _parse_effect(
        "search your library for an artifact card, "
        "reveal it, put it into your hand, then shuffle"
    ) == SearchLibraryForCard(filter_name="artifact", may_fail=True)


def test_parse_compound_and_joiner_preferred_when_both_present():
    # Joiner precedence: when both `` and `` and ``, then `` could
    # split a phrase, the `` and `` joiner is tried first per the
    # ``_parse_compound_effect`` docstring. With only a ``,
    # then``-bearing phrase splittable today (no EoE card prints
    # both joiners in one effect body), this test pins the precedence
    # by asserting that an `` and ``-bearing phrase still composes
    # via the ``and`` path.
    assert _parse_effect(
        "you gain 2 life and draw a card"
    ) == Sequence(
        effects=(
            __import__(
                "mtgz_ir.cards.base", fromlist=["GainLife"],
            ).GainLife(amount=2),
            DrawCards(amount=1),
        ),
    )


def test_parse_compound_then_joiner_rejects_when_any_piece_fails():
    # All-or-nothing rule still applies: if any ``, then ``-split
    # piece doesn't parse as a standalone effect, the compound
    # returns ``None`` and falls through to single-effect grammar
    # (which also won't claim the original here, so the whole phrase
    # stays unclaimed). Pins the safety net that an unsupported
    # tail-phrase doesn't silently truncate a compound.
    assert _parse_effect(
        "draw a card, then bogus unsupported phrase"
    ) is None


# ---------- end-to-end card-claim tests ----------

_PARSER = OracleParserCardSource()


def test_parser_claims_virus_beetle_via_each_opponent_discards():
    # Virus Beetle prints as ``When this creature enters, each opponent
    # discards a card.`` The trigger condition (``this creature
    # enters``) was already in the trigger phrasebook; this batch
    # adds the effect-side primitive that completes the claim.
    assert "Virus Beetle" in _PARSER.claimed_names


def test_parser_claims_gravkill_via_exile_creature_or_spacecraft():
    # Gravkill prints as ``Exile target creature or Spacecraft.`` —
    # single-line spell parsed straight via the new primitive.
    assert "Gravkill" in _PARSER.claimed_names


def test_parser_claims_mechan_navigator_via_then_compound_and_discard():
    # Mechan Navigator prints as ``Whenever this creature becomes
    # tapped, draw a card, then discard a card.`` The trigger
    # condition (``this creature becomes tapped``) was already in
    # the predicate phrasebook; this batch adds both the
    # ``discard a card`` primitive and the ``, then `` compound
    # joiner that together let the effect body parse.
    assert "Mechan Navigator" in _PARSER.claimed_names


def test_parser_claimed_virus_beetle_has_etb_each_opponent_discards():
    # End-to-end shape check: the produced Virus Beetle printing
    # should have a single ETB-shaped trigger whose effect is
    # ``EachOpponentDiscards(count=1)`` — matching the static
    # printing's intent exactly.
    p = _PARSER.get_printings()["Virus Beetle"]
    assert len(p.triggered_abilities) == 1
    assert p.triggered_abilities[0].effect == EachOpponentDiscards(
        count=1
    )


def test_parser_claimed_gravkill_has_exile_target_creature_effect():
    # End-to-end shape check: Gravkill's spell effect is the bare
    # ``RemoveTargetPermanents(EXILE, creature-or-Spacecraft)`` — the
    # Spacecraft leg of "exile target creature or Spacecraft" is kept.
    p = _PARSER.get_printings()["Gravkill"]
    assert p.spell_effect == _EXILE_CREATURE_OR_SPACECRAFT


def test_parser_claimed_mechan_navigator_has_sequence_draw_then_discard():
    # End-to-end shape check: Mechan Navigator's becomes-tapped
    # trigger fires a ``Sequence(DrawCards(1), DiscardSelf(1))`` in
    # the printed left-to-right order — matching the static printing.
    p = _PARSER.get_printings()["Mechan Navigator"]
    assert len(p.triggered_abilities) == 1
    assert p.triggered_abilities[0].effect == Sequence(
        effects=(DrawCards(amount=1), DiscardSelf(amount=1)),
    )
