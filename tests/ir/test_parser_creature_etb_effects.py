"""Tests for the parser ETB-non-token-effect cluster.

Each new effect-grammar entry covers a creature whose only triggered
ability is a ``"When this creature enters, …"`` line whose effect was
previously not in the unified ``_parse_effect`` dispatcher. The cluster:

* ``deals N damage to target creature an opponent controls`` →
  ``DealDamageToOpponentCreature(amount=N)``. Surfaces on Nova
  Hellkite's ETB.
* ``put a +1/+1 counter on target creature you control`` →
  ``PutPlusOneCounterOnCreatureYouControl(amount=1)``. Restricted-
  controller variant of the bare ``put a +1/+1 counter on target
  creature`` primitive. Surfaces on Rayblade Trooper.
* ``target creature an opponent controls gets ±N/±M until end of
  turn`` → ``PumpTargetCreatureUntilEOT(opponents_only=True)``.
  Sinister Cryologist's ETB debuff.
* ``creatures your opponents control get ±N/±M until end of turn`` /
  ``... until your next turn`` →
  ``PumpCreaturesOpponentsControlUntilEOT(power_bonus=N,
  toughness_bonus=M, until_caster_next_turn=...)``. Mass debuff/pump on
  every creature an opponent controls; the ``until your next turn``
  tail flips ``until_caster_next_turn=True`` (Mouth of the Storm).
* ``look at the top two cards of your library. Put one into your hand
  and the other into your graveyard`` →
  ``LookTopNPutOneInHandRestToGraveyard(n=2, reveal_chosen=False)``.
  Codecracker Hound's ETB; the embedded period sits inside the effect
  phrase rather than terminating it (compound splitting doesn't apply
  because the second sentence isn't a standalone effect). Refined-
  Cycle-1 fold collapsed the prior ``LookAtTopTwoSplitHandGraveyard``
  bespoke class into the parameterized primitive.
* ``return up to one other target permanent you control to its
  owner's hand`` → ``ReturnTargetPermanentYouControlToOwnersHand(
  optional=True, exclude_self=True)``. Exosuit Savior's ETB.

Each primitive is exercised via ``_parse_effect`` (normalized) and via
``_parse_effect_phrase`` (creature-trigger surface form) where the
``it`` source-subject pronoun applies. Negative cases pin the regex
boundaries — the bare-target / non-restricted variants must still
produce the unrestricted engine class so the new restricted variants
don't accidentally claim the bare surface.

Card-claim tests confirm the parser end-to-end-claims the EoE creatures
unlocked by this batch: Sinister Cryologist, Mouth of the Storm,
Codecracker Hound, Exosuit Savior, Rayblade Trooper.
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CreateToken,
    DealDamageToOpponentCreature,
    DealDamageToTargetCreature,
    ExileEachOpponentsGraveyard,
    LookTopNPutOneInHandRestToGraveyard,
    PumpCreaturesOpponentsControlUntilEOT,
    PumpTargetCreatureUntilEOT,
    PutPlusOneCounterOnCreatureYouControl,
    PutPlusOneCounterOnEachOfUpToNTargetCreatures,
    PutPlusOneCounterOnTargetCreature,
    ReturnTargetPermanentYouControlToOwnersHand,
    Sequence,
)
from mtgz_ir.cards.parser.effects import (
    _parse_compound_effect,
    _parse_effect,
    _parse_effect_phrase,
    _split_effect_sentences,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS


# ---------- deals N damage to target creature an opponent controls ----------

def test_parse_effect_recognizes_damage_target_opponent_creature():
    # Nova Hellkite's ETB — 1 damage to a target creature an opponent
    # controls. Maps to the engine class with the captured int.
    assert _parse_effect(
        "deals 1 damage to target creature an opponent controls"
    ) == DealDamageToOpponentCreature(amount=1)


def test_parse_effect_recognizes_damage_target_opponent_creature_larger():
    # Forward-symmetric larger-amount form. The regex captures any
    # digit, so a future card with the same surface but higher
    # amount parses without further grammar work.
    assert _parse_effect(
        "deals 5 damage to target creature an opponent controls"
    ) == DealDamageToOpponentCreature(amount=5)


def test_parse_effect_phrase_recognizes_damage_target_opponent_creature():
    # Creature-trigger surface form — the ``it`` source-subject prefix
    # is stripped by ``_normalize_creature_effect_phrase`` before the
    # regex match.
    assert _parse_effect_phrase(
        "it deals 1 damage to target creature an opponent controls"
    ) == DealDamageToOpponentCreature(amount=1)


def test_parse_effect_does_not_collide_with_bare_target_creature_damage():
    # The bare ``deals N damage to target creature`` regex (which
    # produces ``DealDamageToTargetCreature``) shouldn't accidentally
    # fire on the opponent-controlled variant. The bare regex's ``$``
    # anchor means it only matches phrases that END at ``creature``
    # with no qualifier; the opponent-only form has more text after
    # ``creature``, so the two surface shapes are disjoint.
    bare = _parse_effect("deals 1 damage to target creature")
    opp = _parse_effect("deals 1 damage to target creature an opponent controls")
    assert isinstance(bare, DealDamageToTargetCreature)
    assert isinstance(opp, DealDamageToOpponentCreature)


# ---------- put a +1/+1 counter on target creature you control ----------

def test_parse_effect_recognizes_counter_on_target_creature_you_control():
    # Rayblade Trooper's ETB. Maps to the restricted engine class
    # whose ``legal_targets`` filters to creatures the resolving
    # controller controls.
    assert _parse_effect(
        "put a +1/+1 counter on target creature you control"
    ) == PutPlusOneCounterOnCreatureYouControl(amount=1)


def test_parse_effect_phrase_recognizes_counter_on_target_creature_you_control():
    # Creature-trigger surface form — no source-subject pronoun on
    # this phrase (the verb ``put`` starts the clause), so the
    # phrase passes through to ``_parse_effect`` unchanged.
    assert _parse_effect_phrase(
        "put a +1/+1 counter on target creature you control"
    ) == PutPlusOneCounterOnCreatureYouControl(amount=1)


def test_parse_effect_does_not_collide_with_bare_counter_on_target_creature():
    # The bare ``put a +1/+1 counter on target creature`` regex
    # (which produces ``PutPlusOneCounterOnTargetCreature``)
    # shouldn't accidentally fire on the ``you control`` variant.
    # The bare regex's ``$`` anchor means it only matches phrases
    # that END at ``creature`` with no qualifier; the restricted form
    # has ``you control`` after ``creature``, so the two surface
    # shapes are disjoint and produce different engine classes.
    bare = _parse_effect("put a +1/+1 counter on target creature")
    restricted = _parse_effect(
        "put a +1/+1 counter on target creature you control"
    )
    assert isinstance(bare, PutPlusOneCounterOnTargetCreature)
    assert isinstance(restricted, PutPlusOneCounterOnCreatureYouControl)


# ---------- target creature an opponent controls gets ±N/±M UEOT ----------

def test_parse_effect_recognizes_pump_target_opponent_creature_debuff():
    # Sinister Cryologist's ETB — -3/-0 debuff on a target creature an
    # opponent controls. ``opponents_only=True`` flag flips so the
    # engine's ``legal_targets`` filter excludes creatures the
    # resolving controller controls.
    assert _parse_effect(
        "target creature an opponent controls gets -3/-0 until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=-3, toughness_bonus=0, opponents_only=True,
    )


def test_parse_effect_recognizes_pump_target_opponent_creature_positive():
    # Forward-symmetric positive form. No EoE card prints a positive
    # opponent-only pump today, but the regex accepts both signs so a
    # future card lands without further grammar work — symmetric with
    # the bare-pump regex's both-signs convention.
    assert _parse_effect(
        "target creature an opponent controls gets +1/+1 until end of turn"
    ) == PumpTargetCreatureUntilEOT(
        power_bonus=1, toughness_bonus=1, opponents_only=True,
    )


def test_parse_effect_does_not_collide_with_bare_pump_target_creature():
    # The bare ``target creature gets ±N/±M until end of turn`` regex
    # (which produces ``PumpTargetCreatureUntilEOT(opponents_only=
    # False)``) shouldn't fire on the opponent-only variant. The
    # surfaces are disjoint because the bare regex anchors on
    # ``target creature gets``, with no opponent-controls qualifier
    # in between.
    bare = _parse_effect(
        "target creature gets -3/-0 until end of turn"
    )
    opp = _parse_effect(
        "target creature an opponent controls gets -3/-0 until end of turn"
    )
    assert isinstance(bare, PumpTargetCreatureUntilEOT)
    assert isinstance(opp, PumpTargetCreatureUntilEOT)
    assert bare.opponents_only is False
    assert opp.opponents_only is True


# ---------- creatures your opponents control get ±N/±M UEOT ----------

def test_parse_effect_recognizes_mass_pump_opponents_until_eot():
    # Forward-looking ``until end of turn`` tail. No EoE card prints
    # this exact shape today (Mouth of the Storm uses the longer
    # ``until your next turn`` tail), but the regex accepts both
    # tails so a future printing parses without further grammar
    # work. The shorter tail flips ``until_caster_next_turn=False``
    # which clears the bonus at the casting turn's cleanup.
    assert _parse_effect(
        "creatures your opponents control get -2/-0 until end of turn"
    ) == PumpCreaturesOpponentsControlUntilEOT(
        power_bonus=-2, toughness_bonus=0, until_caster_next_turn=False,
    )


def test_parse_effect_recognizes_mass_pump_opponents_until_your_next_turn():
    # Mouth of the Storm's ETB — -3/-0 debuff on every creature an
    # opponent controls, surviving past the casting turn's cleanup.
    # The longer tail flips ``until_caster_next_turn=True`` which
    # routes the bonus through the engine's extended-temp pump path
    # (clears at the start of the resolving controller's NEXT turn).
    assert _parse_effect(
        "creatures your opponents control get -3/-0 until your next turn"
    ) == PumpCreaturesOpponentsControlUntilEOT(
        power_bonus=-3, toughness_bonus=0, until_caster_next_turn=True,
    )


def test_parse_effect_returns_none_for_mass_pump_with_other_tail():
    # An unrecognized tail ("until end of combat" etc.) doesn't match
    # — the regex accepts only the two literal tails. Sanity-check the
    # regex stays narrow.
    assert _parse_effect(
        "creatures your opponents control get -3/-0 until end of combat"
    ) is None


# ---------- look at top two; one to hand, one to graveyard ----------

def test_parse_effect_recognizes_look_at_top_two_split():
    # Codecracker Hound's ETB. The two-sentence surface form (with an
    # embedded period between sentences) matches as a single
    # primitive — ``_parse_compound_effect``'s all-or-nothing rule
    # rejects the split because the second sentence isn't a
    # standalone effect, so we fall through to this regex.
    # Refined-Cycle-1 fold (2026-05-24): the parser produces
    # ``LookTopNPutOneInHandRestToGraveyard(n=2,
    # reveal_chosen=False)`` instead of the prior bespoke
    # ``LookAtTopTwoSplitHandGraveyard()``. ``reveal_chosen=False``
    # preserves Codecracker Hound's printed-text observable (no
    # "Reveal that card" clause).
    assert _parse_effect(
        "look at the top two cards of your library. "
        "Put one into your hand and the other into your graveyard"
    ) == LookTopNPutOneInHandRestToGraveyard(n=2, reveal_chosen=False)


def test_parse_effect_phrase_recognizes_look_at_top_two_split():
    # Creature-trigger surface form — no source-subject pronoun on
    # this phrase (the verb ``look`` starts the clause).
    assert _parse_effect_phrase(
        "look at the top two cards of your library. "
        "Put one into your hand and the other into your graveyard"
    ) == LookTopNPutOneInHandRestToGraveyard(n=2, reveal_chosen=False)


def test_parse_effect_returns_none_for_look_at_top_two_wrong_pile():
    # A surface form with reversed piles or wrong count doesn't match
    # — the regex is a literal phrase. Sanity-check the regex stays
    # narrow.
    assert _parse_effect(
        "look at the top two cards of your library. "
        "Put one into your graveyard and the other into your hand"
    ) is None


# ---------- return up to one other permanent you control to hand ----------

def test_parse_effect_recognizes_return_up_to_one_other_permanent():
    # Exosuit Savior's ETB. The ``up to one other`` fragment combines
    # the optionality (``optional=True``) with the self-exclusion
    # (``exclude_self=True``).
    assert _parse_effect(
        "return up to one other target permanent you control "
        "to its owner's hand"
    ) == ReturnTargetPermanentYouControlToOwnersHand(
        optional=True, exclude_self=True,
    )


def test_parse_effect_phrase_recognizes_return_up_to_one_other_permanent():
    # Creature-trigger surface form — no source-subject pronoun on
    # this phrase.
    assert _parse_effect_phrase(
        "return up to one other target permanent you control "
        "to its owner's hand"
    ) == ReturnTargetPermanentYouControlToOwnersHand(
        optional=True, exclude_self=True,
    )


def test_parse_effect_return_without_other_is_not_the_other_primitive():
    # Without the ``other`` qualifier the ``up to one other`` primitive
    # doesn't claim the surface; the generic ``return [up to one ]target
    # <noun> to its owner's hand`` recognizer (effects_ext) does, WITHOUT
    # the source-exclusion.
    from mtgz_ir.effects.generic.bounce import Bounce
    eff = _parse_effect(
        "return up to one target permanent you control to its owner's hand"
    )
    assert type(eff) is Bounce


# ---------- end-to-end card claims ----------

def _claimed_names() -> set[str]:
    return set(OracleParserCardSource().claimed_names)


def test_sinister_cryologist_claimed_by_parser():
    # Sinister Cryologist `{2}{U}` 2/3 Creature — Jellyfish Wizard.
    # Warp `{U}`. ETB: target creature an opponent controls gets
    # -3/-0 UEOT. Unlocked by the new opponent-controlled-pump
    # primitive.
    assert "Sinister Cryologist" in _claimed_names()


def test_mouth_of_the_storm_claimed_by_parser():
    # Mouth of the Storm `{6}{U}` 6/6 Elemental — Flying, Ward {2}.
    # ETB: creatures your opponents control get -3/-0 until your
    # next turn. Unlocked by the new mass-opponent-pump primitive
    # whose ``until_caster_next_turn`` flag flips on the longer
    # ``until your next turn`` tail.
    assert "Mouth of the Storm" in _claimed_names()


def test_codecracker_hound_claimed_by_parser():
    # Codecracker Hound `{2}{U}` 2/1 Dog. Warp `{2}{U}`. ETB: look
    # at the top two cards of your library; one to hand, one to
    # graveyard. Unlocked by the new look-at-top-two split-pile
    # primitive.
    assert "Codecracker Hound" in _claimed_names()


def test_exosuit_savior_claimed_by_parser():
    # Exosuit Savior `{2}{W}` 2/2 Human Soldier — Flying. ETB:
    # return up to one other target permanent you control to its
    # owner's hand. Unlocked by the new exosuit-savior-style bounce
    # primitive (optional + exclude_self).
    assert "Exosuit Savior" in _claimed_names()


def test_rayblade_trooper_claimed_by_parser():
    # Rayblade Trooper `{2}{W}` 2/2 Human Soldier. Warp `{1}{W}`.
    # ETB: put a +1/+1 counter on target creature you control. Death
    # trigger: nontoken creature you control with a +1/+1 counter on
    # it dies → make a 1/1 Human Soldier token (already supported by
    # the parser's death-trigger family). Unlocked here by the new
    # restricted-target +1/+1 counter primitive.
    assert "Rayblade Trooper" in _claimed_names()


# ---------- exile each opponent's graveyard ----------

def test_parse_effect_recognizes_exile_each_opponents_graveyard():
    # Untargeted graveyard wipe on every opponent. Surfaces in
    # Dauntless Scrapbot's compound ETB ``exile each opponent's
    # graveyard. Create a Lander token``.
    assert _parse_effect(
        "exile each opponent's graveyard"
    ) == ExileEachOpponentsGraveyard()


# ---------- put a +1/+1 counter on each of up to N target creatures ----------

def test_parse_effect_recognizes_counter_on_each_of_up_to_two():
    # Weftblade Enhancer's ETB. Multi-target counter spread; the
    # ``two`` cardinal goes through ``_parse_amount`` to land as the
    # int 2 on the engine class's ``max_n`` field.
    assert _parse_effect(
        "put a +1/+1 counter on each of up to two target creatures"
    ) == PutPlusOneCounterOnEachOfUpToNTargetCreatures(max_n=2)


def test_parse_effect_recognizes_counter_on_each_of_up_to_three():
    # Forward-symmetric larger-amount form. No EoE card prints
    # ``up to three`` today, but the regex accepts both digits and
    # spelled-out small cardinals.
    assert _parse_effect(
        "put a +1/+1 counter on each of up to three target creatures"
    ) == PutPlusOneCounterOnEachOfUpToNTargetCreatures(max_n=3)


def test_parse_effect_recognizes_counter_on_each_of_up_to_digit():
    # Digit form symmetric with the spelled-out form.
    assert _parse_effect(
        "put a +1/+1 counter on each of up to 2 target creatures"
    ) == PutPlusOneCounterOnEachOfUpToNTargetCreatures(max_n=2)


def test_parse_effect_does_not_collide_with_bare_counter_target_creature():
    # The bare ``put a +1/+1 counter on target creature`` regex has
    # a ``$`` anchor on ``creature``; the ``each of up to two target
    # creatures`` shape is anchored on a different verb-object
    # phrasing, so the two surfaces are disjoint.
    bare = _parse_effect("put a +1/+1 counter on target creature")
    multi = _parse_effect(
        "put a +1/+1 counter on each of up to two target creatures"
    )
    assert isinstance(bare, PutPlusOneCounterOnTargetCreature)
    assert isinstance(multi, PutPlusOneCounterOnEachOfUpToNTargetCreatures)


# ---------- sentence-boundary compound split ----------

def test_split_effect_sentences_returns_single_phrase_unchanged():
    # No internal sentence boundary → returns ``[phrase]``. Trailing
    # period is preserved (it's the OUTER terminator, not an internal
    # boundary).
    assert _split_effect_sentences("draw a card") == ["draw a card"]


def test_split_effect_sentences_carves_two_sentences():
    # Period + uppercase-letter boundary → carve. Each piece is
    # stripped, trailing period removed, and the second piece's
    # first letter lowercased so it matches the normalized regex
    # shape (no leading uppercase).
    assert _split_effect_sentences(
        "exile each opponent's graveyard. Create a Lander token"
    ) == ["exile each opponent's graveyard", "create a Lander token"]


def test_split_effect_sentences_preserves_if_you_do_continuation():
    # ``If you do,`` continuation must NOT split — the negative
    # lookahead in the boundary regex keeps the phrase welded
    # together so the ``You may pay {COST}. If you do, …`` family
    # parses through its single primitive.
    assert _split_effect_sentences(
        "you may pay {2}. If you do, draw a card"
    ) == ["you may pay {2}. If you do, draw a card"]


def test_parse_compound_effect_carves_dauntless_scrapbot_etb():
    # Dauntless Scrapbot's compound ETB body. The effect-phrase has
    # no `` and `` and no ``, then ``, so the splitter falls through
    # to the sentence-boundary split path.
    eff = _parse_compound_effect(
        "exile each opponent's graveyard. Create a Lander token"
    )
    assert eff == Sequence(effects=(
        ExileEachOpponentsGraveyard(),
        CreateToken(
            token_printing=TOKEN_PRINTINGS["Lander"],
            count=1,
        ),
    ))


def test_parse_compound_effect_returns_none_for_unparseable_sentence_split():
    # Sentence-boundary split is all-or-nothing — if any piece
    # fails to parse, the whole compound is rejected so the caller
    # can try the single-effect grammar on the unsplit phrase. This
    # protects single-effect primitives whose regex contains an
    # internal period (Codecracker Hound's two-sentence look-at-
    # top-two phrase) from being torn in half by the splitter when
    # the second piece doesn't standalone-parse.
    assert _parse_compound_effect(
        "look at the top two cards of your library. "
        "Put one into your hand"
    ) is None


def test_codecracker_hound_phrase_still_claims_single_primitive():
    # Codecracker Hound's phrase contains an internal period AND the
    # ``and`` joiner. The compound split tries the ``and`` joiner
    # FIRST and rejects (neither piece parses), which lets the
    # single-effect dispatcher claim the whole phrase as the
    # ``LookTopNPutOneInHandRestToGraveyard(n=2, reveal_chosen=False)``
    # primitive (refined-Cycle-1 fold of the prior
    # ``LookAtTopTwoSplitHandGraveyard`` bespoke class). Regression
    # guard for the sentence-boundary split path not stealing this
    # phrase.
    assert _parse_effect(
        "look at the top two cards of your library. "
        "Put one into your hand and the other into your graveyard"
    ) == LookTopNPutOneInHandRestToGraveyard(n=2, reveal_chosen=False)


def test_dauntless_scrapbot_claimed_by_parser():
    # Dauntless Scrapbot `{3}` 3/1 Robot Artifact Creature. ETB:
    # ``exile each opponent's graveyard. Create a Lander token.``
    # Unlocked by the new sentence-boundary compound-effect splitter
    # plus the new ``exile each opponent's graveyard`` primitive
    # (the ``create a Lander token`` half was already supported by
    # the existing bare-name token-creation grammar).
    assert "Dauntless Scrapbot" in _claimed_names()


def test_weftblade_enhancer_claimed_by_parser():
    # Weftblade Enhancer `{5}{W}` 3/4 Drix Artificer. Warp `{2}{W}`.
    # ETB: put a +1/+1 counter on each of up to two target
    # creatures. Unlocked by the new multi-target counter-spread
    # primitive.
    assert "Weftblade Enhancer" in _claimed_names()


# ---------- multi-keyword sentence-case (Flying, haste) ----------

def test_parse_keyword_line_accepts_sentence_case_multi_keyword():
    # Scryfall's printed convention for multi-keyword lines is
    # sentence-case: only the first piece is capitalized, every
    # subsequent piece starts lowercase. ``"Flying, haste"`` is the
    # printed form on Nova Hellkite, Frenzied Baloth, etc.; the
    # parser normalizes the second piece's first letter to upper-
    # case before lookup so the sentence-case form parses.
    from mtgz_ir.cards.parser.keywords import _parse_keyword_line

    assert _parse_keyword_line("Flying, haste") == ("flying", "haste")
    assert _parse_keyword_line("Trample, haste") == ("trample", "haste")
    assert _parse_keyword_line("Vigilance, menace") == (
        "vigilance", "menace",
    )


def test_parse_keyword_line_still_rejects_lowercase_first_piece():
    # The first piece is matched as-is (case-sensitive) — a fully
    # lowercase line like ``"hexproof"`` or ``"flying, haste"``
    # doesn't match because lowercase keyword names only appear
    # mid-sentence (not as a printed keyword line). Regression
    # guard for the existing single-keyword case-sensitivity
    # contract.
    from mtgz_ir.cards.parser.keywords import _parse_keyword_line

    assert _parse_keyword_line("hexproof") is None
    assert _parse_keyword_line("flying, haste") is None


def test_nova_hellkite_claimed_by_parser():
    # Nova Hellkite `{3}{R}{R}` 4/5 Dragon — Flying, haste. Warp
    # `{2}{R}`. ETB: deals 1 damage to target creature an opponent
    # controls. Unlocked by the new opponent-creature damage
    # primitive AND the keyword-line sentence-case fix (the
    # ``Flying, haste`` line is what blocked claiming Nova Hellkite
    # before the keyword fix).
    assert "Nova Hellkite" in _claimed_names()


# ---------- destroy up to one other target creature, draw if undamaged ----------

def test_parse_effect_recognizes_destroy_up_to_one_other_with_draw():
    # Faller's Faithful's ETB. Single primitive with three flags:
    # ``optional=True`` (``up to one``), ``exclude_self=True``
    # (``other``), ``draw_if_undamaged_amount=2`` (the embedded
    # follow-up sentence). The amount tail accepts spelled-out
    # cardinals (``two``) and digits.
    from mtgz_ir.cards.base import DestroyTargetCreature

    assert _parse_effect(
        "destroy up to one other target creature. "
        "If that creature wasn't dealt damage this turn, "
        "its controller draws two cards"
    ) == DestroyTargetCreature(
        optional=True, exclude_self=True, draw_if_undamaged_amount=2,
    )


def test_parse_effect_does_not_collide_with_bare_destroy_target_creature():
    # The bare ``destroy target creature`` regex has a ``$`` anchor
    # so the longer Faller's surface doesn't accidentally match it.
    from mtgz_ir.cards.base import DestroyTargetCreature

    bare = _parse_effect("destroy target creature")
    fallers = _parse_effect(
        "destroy up to one other target creature. "
        "If that creature wasn't dealt damage this turn, "
        "its controller draws two cards"
    )
    assert isinstance(bare, DestroyTargetCreature)
    assert isinstance(fallers, DestroyTargetCreature)
    assert bare.optional is False
    assert bare.exclude_self is False
    assert bare.draw_if_undamaged_amount == 0
    assert fallers.optional is True
    assert fallers.exclude_self is True
    assert fallers.draw_if_undamaged_amount == 2


def test_fallers_faithful_claimed_by_parser():
    # Faller's Faithful `{2}{B}` 3/1 Human Wizard. ETB: destroy up
    # to one other target creature; if undamaged, controller draws
    # two cards. Unlocked by the new destroy-with-undamaged-rider
    # primitive.
    assert "Faller's Faithful" in _claimed_names()


# ---------- tap target opponent permanent + stun ----------

def test_parse_effect_recognizes_tap_opponent_permanent_and_stun():
    # Mechanozoa's ETB. Both clauses apply to the same target — the
    # ``and`` is intra-effect, not a compound joiner. The compound
    # splitter does try the ``and`` joiner first but the two pieces
    # don't standalone-parse, so the splitter rejects and the
    # single-effect dispatcher claims the whole phrase.
    from mtgz_ir.cards.base import TapTargetOpponentPermanentAndStunIt

    assert _parse_effect(
        "tap target artifact or creature an opponent controls "
        "and put a stun counter on it"
    ) == TapTargetOpponentPermanentAndStunIt()


def test_mechanozoa_claimed_by_parser():
    # Mechanozoa `{4}{U}{U}` 5/5 Robot Jellyfish — Artifact Creature.
    # Warp `{2}{U}`. ETB: tap target artifact or creature an
    # opponent controls and put a stun counter on it. Unlocked by
    # the new tap-and-stun primitive.
    assert "Mechanozoa" in _claimed_names()


# ---------- create Lander + delayed sacrifice ----------

def test_parse_effect_recognizes_create_lander_and_schedule_sacrifice():
    # Kav Landseeker's ETB. Two-sentence body where the second
    # sentence references the token created in the first — they
    # can't be split into independent effects, so the whole body
    # is one primitive.
    from mtgz_ir.cards.base import CreateLanderAndScheduleSacrifice

    assert _parse_effect(
        "create a Lander token. "
        "At the beginning of the end step on your next turn, "
        "sacrifice that token"
    ) == CreateLanderAndScheduleSacrifice()


def test_kav_landseeker_claimed_by_parser():
    # Kav Landseeker `{3}{R}` 4/3 Kavu Soldier — Menace. ETB: create
    # a Lander token, sacrifice it at the beginning of the end step
    # on your next turn. Unlocked by the new
    # create-Lander-and-schedule-sac primitive.
    assert "Kav Landseeker" in _claimed_names()
