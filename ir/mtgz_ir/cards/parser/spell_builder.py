"""Instant / sorcery ``CardPrinting`` builder.

Single-line spells are parsed straight via the unified spell-effect
phrase parser. Multi-line spells split on ``\\n`` into clauses and
combine them via one of three combinators — ``ConditionalIfVoid`` for
the Plasma-Bolt-shaped Void rider, ``ModalChooseOne`` for Scryfall's
``Choose one —`` modal shape, otherwise ``Sequence`` if every
clause parses as a plain spell-effect sentence. Any unparseable
clause skips the whole spell.

A leading clause that is just an unimplemented spell-only keyword
plus its parenthesized reminder text (currently only ``Convoke``)
is dropped before the remaining clauses are parsed — the static
side ships these spells without modeling the keyword, so the
parser must do the same to stay in parity. See
``_SPELL_KEYWORD_PREAMBLE_RE`` for the exact recognized shapes.

Cost-reduction rider clauses (``This spell costs {N} less to cast
if <condition>.``) are likewise stripped from the effect-clause
stream and surfaced separately on the ``CardPrinting`` via the
``cost_reductions`` kwarg. The ``_detect_cost_reductions`` helper
scans the original text for those rules; ``_parse_spell_oracle_
text`` drops the matching clauses before combinator dispatch.

The modal branch handles two Scryfall printed header shapes:
``Choose one —`` (single-mode pick) and ``Choose one or both —``
(controller may resolve either mode alone or both). Each is followed
by two or more mode lines that begin with ``• `` (bullet + space)
and contain a single sentence parseable by the unified spell-effect
grammar. The ``Choose one or both`` shape constructs
``ModalChooseOne(or_both=True)`` so the engine routes mode-pick
through the agent's ``choose_modes`` (plural) hook. ``Choose two``,
multi-line modes, and ``up to two target`` mode-counts remain out of
scope; the parser declines those spells until the engine grows the
matching machinery.
"""

from __future__ import annotations

import dataclasses
import re

from mtgz_ir.cards.base import (
    parse_mana_cost,
    CardPrinting,
    CommuneWithEvilEffect,
    ConditionalIfVoid,
    CostReduction,
    CounterTargetSpell,
    CreateToken,
    ExileTargetCreature,
    DealDamageAnyTarget,
    DealNDamageToEachCreature,
    DestroyTargetArtifact,
    DestroyTargetCreature,
    DestroyTargetNonlandPermanent,
    DrawCards,
    EachOpponentLosesLife,
    Effect,
    LoseLife,
    ManifestDread,
    ModalChooseOne,
    OptionalSacArtifactForEffect,
    PumpCreaturesYouControlUntilEOT,
    PumpTargetCreatureUntilEOT,
    ReturnCreatureFromYourGraveyardToBattlefield,
    ReturnCreatureFromYourGraveyardToHand,
    Sequence,
    make_instant,
    make_sorcery,
)
from mtgz_ir.cards.parser.cost_reduction import _parse_cost_reduction_line
from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
from mtgz_ir.cards.parser.conditional_phrases import combine_instead_rider
from mtgz_ir.cards.parser.modal_spells import parse_multi_modal
from mtgz_ir.cards.parser.spell_cost_lines import (
    apply_overload,
    detect_spell_line_fields,
    is_spell_cost_line,
)
from mtgz_ir.cards.parser.precise_grammar import (
    legacy_parity_pinned,
    precise_grammar_active,
)
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.type_line import _parse_type_line
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.base import StaticAbility, StaticScope
from mtgz_ir.effects.conditions import FirstSpellYouCastThisGame
from mtgz_ir.effects.modifiers import (
    EventPattern,
    Replace,
    ReplacementEffect,
)
from mtgz_ir.effects.predicates import Event


# Modal spell headers: ``Choose one —`` (single-mode pick) or
# ``Choose one or both —`` (controller may resolve either mode alone
# or both — currently only Scour for Scrap in EoE). Both em-dashes are
# U+2014. ``Choose two`` and other mode-counts remain out of scope.
_MODAL_CHOOSE_ONE_HEADER_RE = re.compile(r"^Choose one —$")
_MODAL_CHOOSE_ONE_OR_BOTH_HEADER_RE = re.compile(r"^Choose one or both —$")
# A mode line begins with U+2022 BULLET + a single space, then the
# mode's effect sentence. Trailing period is preserved here and stripped
# downstream by the spell-effect-phrase normalizer.
_MODAL_MODE_LINE_RE = re.compile(r"^• (.+)$")


# Plasma-Bolt-shaped Void rider: a second clause of the form
# "Void — <Name> deals M damage instead if a nonland permanent left the
# battlefield this turn or a spell was warped this turn."
# Pairs with a preceding "<Name> deals N damage to any target." clause
# to form a ConditionalIfVoid switching the damage amount.
_SPELL_VOID_DAMAGE_REPLACEMENT = (
    r"^Void — {name} deals (\d+) damage instead "
    r"if a nonland permanent left the battlefield this turn "
    r"or a spell was warped this turn\.$"
)


# Tragic-Trajectory-shaped Void rider: a second clause of the form
# "Void — That creature gets ±M/±M' until end of turn instead if a
# nonland permanent left the battlefield this turn or a spell was
# warped this turn."
# Pairs with a preceding "Target creature gets ±N/±N' until end of
# turn." clause to form a ConditionalIfVoid switching the pump
# bonuses. The carry-over subject ``That creature`` refers back to
# the target chosen on the first clause; the engine reuses the
# same target when resolving either branch (single-target spell).
_SPELL_VOID_PUMP_REPLACEMENT = re.compile(
    r"^Void — That creature gets ([+-]\d+)/([+-]\d+) "
    r"until end of turn instead "
    r"if a nonland permanent left the battlefield this turn "
    r"or a spell was warped this turn\.$"
)


# Hymn-of-the-Faller-shaped Void "adds-a-draw" rider: a second clause
# of the form "Void — If a nonland permanent left the battlefield this
# turn or a spell was warped this turn, draw another card."
# Pairs with ANY base-clause Sequence to append a
# ``ConditionalIfVoid(then=DrawCards(1), else=None)`` to the Sequence.
# Unlike the replacement helpers above (which swap one effect for
# another), this rider ADDS an extra draw on top of the base effects
# when the Void state is active.
_SPELL_VOID_ADDS_DRAW_RE = re.compile(
    r"^Void — If a nonland permanent left the battlefield this turn "
    r"or a spell was warped this turn, draw another card\.$"
)


# Void "instead" rider: ``Void — If a nonland permanent left the
# battlefield this turn or a spell was warped this turn, instead
# <then-effect>.`` (Decode Transmissions). The "instead" keyword
# indicates the rider REPLACES the preceding base effect entirely —
# distinct from the "adds-draw" rider (which appends an effect) and
# from the damage/pump replacement helpers (which swap one specific
# field). The then-branch phrase after ``instead `` is parsed by the
# unified spell-effect grammar; if it doesn't parse, the rider is
# rejected and the spell stays in the bespoke table.
#
# Two-clause shape:
#   base clause → parses via ``_parse_spell_effect_phrase``
#   Void-instead rider → captures the then-branch for parsing
# Combined: ``ConditionalIfVoid(then=then_effect, else=base_effect)``
_SPELL_VOID_INSTEAD_EFFECT_RE = re.compile(
    r"^Void — If a nonland permanent left the battlefield this turn "
    r"or a spell was warped this turn, instead (.+)\.$"
)


# Delirium-rider drop regexes — recognizers for ``Delirium — …``
# clauses that the parser silently drops because the engine doesn't
# implement the Delirium graveyard-card-type check today.
#
# Three distinct shapes exist in the EoE/Duskmourn pool:
#
#   1. **Effect rider** — ``Delirium — If there are four or more card
#      types among cards in your graveyard, <effect>.``  The effect
#      is an upside that is lost entirely when Delirium support is
#      absent; no CostReduction is produced.  (Impossible Inferno,
#      Violent Urge, Demonic Counsel.)
#
#   2. **Cost-reduction rider** — ``Delirium — This spell costs {N}
#      less to cast as long as there are four or more card types among
#      cards in your graveyard.``  Would be a CostReduction but the
#      ``four_or_more_card_types_in_graveyard`` condition isn't wired
#      into ``_evaluate_cost_reduction``; drop cleanly so no spurious
#      CostReduction rule is attached to the printing.  (Drag to the
#      Roots.)
#
#   3. **Modal-header upgrade** — ``Delirium — Choose one. If there
#      are four or more card types among cards in your graveyard, choose
#      one or more instead.``  The header is replaced by the bare
#      ``Choose one —`` semantics; the mode lines that follow still
#      parse normally via ``_parse_modal_choose_one``.
#      (Let's Play a Game.)
#
# All three shapes start with ``Delirium — `` and the remainder
# differs enough that three regexes (rather than one catch-all) are
# cleaner; each regex's sole purpose is to match that shape so the
# caller knows to drop the clause.
_DELIRIUM_EFFECT_RIDER_RE = re.compile(
    r"^Delirium — If there are four or more card types among "
    r"cards in your graveyard, "
)
_DELIRIUM_COST_REDUCTION_RIDER_RE = re.compile(
    r"^Delirium — This spell costs \{\d+\} less to cast as long as "
    r"there are four or more card types among cards in your graveyard\.$"
)
_DELIRIUM_MODAL_HEADER_RE = re.compile(
    r"^Delirium — Choose one\. "
    r"If there are four or more card types among cards in your graveyard, "
    r"choose one or more instead\.$"
)


def _is_delirium_drop_clause(clause: str) -> bool:
    """Return ``True`` if ``clause`` is a Delirium-prefix line that
    the parser drops as an unimplemented mechanic.

    Two shapes are recognized for dropping:
      - Effect rider: ``Delirium — If there are four or more card
        types …, <effect>.``  (Impossible Inferno, Violent Urge,
        Demonic Counsel) — upside dropped because the engine has no
        Delirium graveyard-card-type check.
      - Cost-reduction rider: ``Delirium — This spell costs {N} less
        to cast as long as there are four or more card types ….``
        (Drag to the Roots) — dropped so no spurious ``CostReduction``
        rule is attached (this drop happens BEFORE the cost-reduction-
        stripping pass so the line never reaches ``_detect_cost_
        reductions``).

    The modal-header variant (``Delirium — Choose one. If there are
    four or more card types …, choose one or more instead.``) is NOT
    dropped here; instead ``_parse_modal_choose_one`` recognizes it
    directly and treats it as a plain ``Choose one —`` header (the
    Delirium upgrade to "choose one or more" is silently discarded
    since the engine has no Delirium-conditional modal-count combinator
    today).
    """
    c = clause.strip()
    return (
        _DELIRIUM_EFFECT_RIDER_RE.match(c) is not None
        or _DELIRIUM_COST_REDUCTION_RIDER_RE.match(c) is not None
    )


# Unimplemented secondary-rider regexes — recognizers for clauses that
# the parser drops because the engine lacks the plumbing to model them.
# Unlike the Delirium multi-clause dropper (which strips an entire line
# from the clause stream before any parsing starts), these patterns
# match the SECOND sentence of a two-clause spell body.  When the
# FIRST sentence parses cleanly as a standalone effect and the second
# matches one of these patterns, the two-clause path returns just the
# first-sentence effect (same simplification the bespoke entries make).
#
# Five shapes exist in the EoE/DSK pool:
#
#   1. **Controller-manifest rider** — ``Its controller manifests
#      dread.``  (entry: "Destroy target creature. Its controller
#      manifests dread." → DestroyTargetCreature()).  The engine's
#      ManifestDread primitive resolves for the *casting* controller;
#      adding a ``for_controller_of_target=True`` path would require
#      non-trivial wiring through the targeting/resolution pipeline.
#
#   2. **Death-trigger manifest rider** — ``When it dies this turn,
#      manifest dread.``  (entry: "Target creature gets +3/+0 until
#      end of turn. When it dies this turn, manifest dread." →
#      PumpTargetCreatureUntilEOT(3, 0)).  The engine has no UEOT-
#      scoped per-target death-trigger registry; adding one for a
#      single card's rider would be a non-trivial widening.
#
#   3. **Optional-graveyard-return rider** — ``You may return a
#      permanent card from your graveyard to your hand.``  (entry:
#      "Manifest dread.\n You may return a permanent card from your
#      graveyard to your hand." → ManifestDread()).  Every existing
#      ``Return*FromGraveyardToHand`` primitive is creature-typed;
#      widening to "any permanent card" with optionality for a single
#      card's rider is a non-trivial engine change.
#
#   4. **Mana-spent conditional draw** — ``If the amount of mana spent
#      to cast that spell was less than its mana value, you draw a
#      card.``  (entry: Unravel, "Counter target spell. If the
#      amount…" → CounterTargetSpell()).  The engine doesn't track
#      per-spell cast-time mana accounting.
#
#   5. **Subtype-filtered UEOT keyword grant** — ``<Subtype>s you
#      control gain <keywords> until end of turn.``  (entry: Midnight
#      Mayhem, "Create three 1/1 red Gremlin creature tokens. Gremlins
#      you control gain menace, lifelink, and haste until end of
#      turn." → CreateToken(Gremlin, count=3)).  The existing
#      ``PumpCreaturesYouControlUntilEOT`` applies to ALL your
#      creatures; a subtype-filtered variant would need a new optional
#      kwarg or a sibling primitive for one card's rider.
#
#   6. **Exile-on-death rider** — ``If that creature or planeswalker
#      would die this turn, exile it instead.``  (entry: Scorching
#      Dragonfire, "Scorching Dragonfire deals 3 damage to target
#      creature or planeswalker. If that creature or planeswalker
#      would die this turn, exile it instead." →
#      DealDamageToTargetCreature(amount=3)).  The engine has no
#      UEOT-scoped "exile-instead-of-die" replacement-effect registry
#      for targeted permanents; adding one for a single card's rider
#      is a non-trivial engine widening.
#
#   7. **"If they do" pay-or-counter consequent** — ``If they do,
#      <effect>.``  (entries: Divert Disaster, Don't Make a Sound).
#      The ``Unless its controller pays {N}`` escape clause in the
#      first sentence is recognized as ``CounterTargetSpell()`` via
#      the new ``_COUNTER_TARGET_SPELL_UNLESS_PAYS`` recognizer; the
#      resulting second clause ``If they do, you create a Lander
#      token.`` / ``If they do, surveil 2.`` has no standalone
#      primitive because the engine doesn't model the pay-cost-or-
#      counter branching that determines whether "they do" applies.
#
#   8. **Conditional reanimate-this-way rider** — ``If a creature
#      card is put into a graveyard this way, return it to the
#      battlefield under your control.``  (entry: Come Back Wrong).
#      The engine has no "if destroyed-this-way creature card was
#      put into graveyard, reanimate it" conditional reanimate
#      primitive; adding one would require wiring back to the
#      resolution context of the triggering Destroy effect.
#
#   9. **Delayed end-step sacrifice trigger** — ``Sacrifice it at
#      the beginning of your next end step.``  (entry: Come Back
#      Wrong).  The engine has no per-object one-shot delayed
#      end-step sacrifice trigger registry.
_ITS_CONTROLLER_MANIFESTS_DREAD_RE = re.compile(
    r"^Its controller manifests dread\.$"
)
_WHEN_IT_DIES_THIS_TURN_MANIFEST_DREAD_RE = re.compile(
    r"^When it dies this turn, manifest dread\.$"
)
_YOU_MAY_RETURN_PERMANENT_CARD_FROM_GY_TO_HAND_RE = re.compile(
    r"^You may return a permanent card from your graveyard to your hand\.$"
)
_MANA_SPENT_CONDITIONAL_DRAW_RE = re.compile(
    r"^If the amount of mana spent to cast that spell was less than "
    r"its mana value, you draw a card\.$"
)
# Subtype-filtered UEOT keyword grant — ``<Subtype>s you control gain
# <keywords> until end of turn.``  The subtype group anchors on a
# capitalized word followed by ``s you control gain``.  The negative
# lookahead ``(?!Creatures )`` excludes the bare generic plural
# ``Creatures you control gain …`` surface, which IS modelable via
# the existing ``PumpCreaturesYouControlUntilEOT`` recognizer and
# must not be dropped.  Only subtype-specific plurals (``Gremlins``,
# ``Vampires``, etc.) are dropped because the engine has no subtype-
# filtered UEOT pump primitive today.
_SUBTYPE_CREATURES_YOU_CONTROL_GAIN_KEYWORDS_UEOT_RE = re.compile(
    r"^(?!Creatures )[A-Z][a-z]+s you control gain .+ until end of turn\.$"
)
# Exile-on-death rider — ``If that creature or planeswalker would die
# this turn, exile it instead.``  (Scorching Dragonfire).  The engine
# has no UEOT-scoped "exile-instead-of-die" replacement-effect
# registry for targeted permanents.
_EXILE_ON_DEATH_THIS_TURN_INSTEAD_RE = re.compile(
    r"^If that creature(?: or planeswalker)? would die this turn, "
    r"exile it instead\.$"
)
# Pay-or-counter consequent rider — ``If they do, <effect>.``
# (Divert Disaster, Don't Make a Sound).  The ``unless controller
# pays {N}`` first sentence is recognized as ``CounterTargetSpell()``
# via ``_COUNTER_TARGET_SPELL_UNLESS_PAYS``, but the consequent
# "if they do" clause has no standalone primitive because the engine
# doesn't model which pay-cost-or-counter branch fires.
_IF_THEY_DO_RIDER_RE = re.compile(r"^If they do, .+\.$")
# Conditional reanimate-this-way rider — ``If a creature card is put
# into a graveyard this way, return it to the battlefield under your
# control.``  (Come Back Wrong).  The engine has no "destroyed-this-
# way conditional reanimate" primitive.
_IF_CREATURE_CARD_PUT_INTO_GY_THIS_WAY_RETURN_IT_RE = re.compile(
    r"^If a creature card is put into a graveyard this way, "
    r"return it to the battlefield under your control\.$"
)
# Delayed end-step sacrifice trigger — ``Sacrifice it at the beginning
# of your next end step.``  (Come Back Wrong).  The engine has no per-
# object one-shot delayed end-step sacrifice trigger registry.
_SACRIFICE_IT_AT_BEGINNING_OF_NEXT_END_STEP_RE = re.compile(
    r"^Sacrifice it at the beginning of your next end step\.$"
)


def _is_unimplemented_secondary_rider(clause: str) -> bool:
    """Return ``True`` if ``clause`` is an unimplemented rider sentence
    that the parser drops because the engine lacks the plumbing to model
    it.

    Nine shapes are recognized:
      - ``Its controller manifests dread.`` (controller-manifest rider)
      - ``When it dies this turn, manifest dread.`` (death-trigger
        manifest rider)
      - ``You may return a permanent card from your graveyard to your
        hand.`` (optional graveyard-return rider)
      - ``If the amount of mana spent to cast that spell was less than
        its mana value, you draw a card.`` (mana-spent conditional draw)
      - ``<Subtype>s you control gain <keywords> until end of turn.``
        (subtype-filtered UEOT keyword grant)
      - ``If that creature or planeswalker would die this turn, exile it
        instead.`` (exile-on-death rider)
      - ``If they do, <effect>.`` (pay-or-counter consequent; the first
        sentence must be a ``unless controller pays {N}`` counterspell)
      - ``If a creature card is put into a graveyard this way, return it
        to the battlefield under your control.`` (conditional reanimate-
        this-way rider; Come Back Wrong)
      - ``Sacrifice it at the beginning of your next end step.``
        (delayed end-step sac trigger; Come Back Wrong)

    Used in the two-clause path and the general N-clause rider-drop
    path in ``_parse_spell_oracle_text``: when the FIRST clause parses
    cleanly and all remaining clauses match these patterns, the first
    clause's effect is returned (mirroring the simplification the
    bespoke entries already make).
    """
    c = clause.strip()
    return (
        _ITS_CONTROLLER_MANIFESTS_DREAD_RE.match(c) is not None
        or _WHEN_IT_DIES_THIS_TURN_MANIFEST_DREAD_RE.match(c) is not None
        or _YOU_MAY_RETURN_PERMANENT_CARD_FROM_GY_TO_HAND_RE.match(c) is not None
        or _SUBTYPE_CREATURES_YOU_CONTROL_GAIN_KEYWORDS_UEOT_RE.match(c) is not None
        or _EXILE_ON_DEATH_THIS_TURN_INSTEAD_RE.match(c) is not None
        or _IF_THEY_DO_RIDER_RE.match(c) is not None
        or _IF_CREATURE_CARD_PUT_INTO_GY_THIS_WAY_RETURN_IT_RE.match(c) is not None
        or _SACRIFICE_IT_AT_BEGINNING_OF_NEXT_END_STEP_RE.match(c) is not None
    )


_IF_THEY_DO_EFFECT_RE = re.compile(r"^If they do, (?P<body>.+)$")


def _parse_counterspell_rider(clause: str, first: Effect, name: str) -> Effect | None:
    """A counterspell's second sentence, folded into its ``CounterSpell``.

    * ``If they do, <effect>.`` after "counter target spell unless its
      controller pays {N}" (Divert Disaster): ``CounterSpell.if_paid``,
      the effect for the counterspell's controller when the tax is paid.
      ("you create ..." loses its subject: the controller is "you".)
    * ``If the amount of mana spent to cast that spell was less than its
      mana value, <effect>.`` (Unravel): ``if_mana_spent_below_mv``.

    ``None`` when ``first`` isn't a counterspell or the clause isn't one
    of these (the pinned legacy spells, Don't Make a Sound, keep their
    bare ``CounterTargetSpell`` and drop the rider).
    """
    from mtgz_ir.effects.generic.counterspells import CounterSpell
    from mtgz_ir.effects.generic.stack_control import CounterTargetSpell

    c = clause.strip()
    if isinstance(first, CounterTargetSpell) and not first.filter_types:
        first = CounterSpell()
    if not isinstance(first, CounterSpell):
        return None
    m = _IF_THEY_DO_EFFECT_RE.match(c)
    if m is not None and first.unless_pays and first.if_paid is None:
        body = m.group("body")
        if body.startswith("you "):
            body = body[4:5].upper() + body[5:]
        inner = _parse_spell_effect_phrase(body, name)
        if inner is None or inner.requires_target():
            return None
        return dataclasses.replace(first, if_paid=inner)
    if _MANA_SPENT_CONDITIONAL_DRAW_RE.match(c) is not None:
        return dataclasses.replace(first, if_mana_spent_below_mv=DrawCards(amount=1))
    return None


# Self-shuffle rider clause — the recognizer for the Scryfall-printed
# self-shuffle replacement a spell's oracle text may carry as a separate
# oracle line: **Nexus form** — ``If <Name> would be put into a graveyard
# from anywhere, reveal <Name> and shuffle it into its owner's library
# instead.`` (Nexus of Fate). It is handled by a SELF-scope
# ``StaticAbility`` carrying a ``ReplacementEffect(WOULD_GO_TO_GRAVEYARD,
# Replace(new_destination="library"))`` on the spell printing (wired via
# ``_detect_shuffle_self_statics``), so it contributes no ``Effect`` and
# must be dropped from the effect-clause stream before the combinator
# dispatch runs.
#
# Green Sun's Zenith's ``Shuffle <Name> into its owner's library.`` is
# NOT a replacement: it is the last instruction of the spell's effect,
# done only as the spell resolves (CR 608.2c; countered, discarded or
# milled, the card goes to the graveyard). It stays in the clause stream
# and parses to ``ShuffleThisSpellIntoOwnersLibrary`` (the name is
# normalized to "this spell" by ``_normalize_spell_effect_phrase``,
# as Time Spiral's "Exile <Name>.").
#
# The regex accepts any card name (an arbitrary non-empty word sequence
# between the anchors), so a future card with the same mechanic claims
# automatically without adding a new literal key. The
# ``_detect_shuffle_self_statics`` helper uses a name-interpolated
# ``re.search`` against the FULL oracle text, which fires correctly
# regardless of whether the self-shuffle line is dropped here — the
# dropping only affects the effect-clause stream, not the StaticAbility
# detection.
_SELF_SHUFFLE_NEXUS_RE = re.compile(
    r"^If .+ would be put into a graveyard from anywhere, "
    r"reveal .+ and shuffle it into its owner's library instead\.$"
)


def _is_self_shuffle_clause(clause: str) -> bool:
    """Return ``True`` if ``clause`` is a recognized self-shuffle rider
    that the parser drops from the effect-clause stream.

    One shape is recognized (after reminder-text stripping): ``If <Name>
    would be put into a graveyard from anywhere, reveal <Name> and
    shuffle it into its owner's library instead.`` (Nexus of Fate form).

    It is modeled by the SELF-scope StaticAbility carrying a
    ``ReplacementEffect(WOULD_GO_TO_GRAVEYARD,
    Replace(new_destination="library"))`` on the spell printing (wired
    via ``_detect_shuffle_self_statics``), so it contributes no
    ``Effect`` and should be excluded from the effect-clause stream.
    Dropping here lets the remaining clause(s) parse through the
    unified per-clause grammar rather than failing the whole spell
    because the replacement can't parse as a standalone ``Effect``.
    Green Sun's Zenith's "Shuffle <Name> into its owner's library." is
    an instruction of the effect and is not dropped.
    """
    return _SELF_SHUFFLE_NEXUS_RE.match(clause.strip()) is not None


# Sentence-boundary splitter for multi-sentence oracle-text lines.
# Some EoE printings pack two effect sentences onto a single oracle
# line separated by ``. `` rather than ``\n`` (e.g. Radiant Strike's
# ``"Destroy target artifact or tapped creature. You gain 3 life."``).
# We carve the line at the period-then-uppercase boundary so each
# sentence flows through the per-clause parser independently.
#
# The negative lookahead keeps the splitter from breaking the
# ``You may pay/sacrifice/discard {COST}. If you do, <inner-effect>.``
# and ``You may sacrifice an artifact. When you do, <inner-effect>.``
# families of compound primitives — those are matched as a single
# phrase by ``_YOU_MAY_PAY_THEN_EFFECT`` /
# ``_OPTIONAL_DISCARD_THEN_DRAW`` / ``_OPTIONAL_SAC_ANOTHER_FOR_*`` /
# ``_YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO`` down in ``effects``, and the
# embedded sentence boundary inside those phrases is required (the
# regexes match a literal ``\. `` between the cost and the consequence).
#
# Both "If you do," and "When you do," are guarded because Lithobraking
# prints "Then you may sacrifice an artifact. When you do, …" — without
# the "When you do," guard the splitter would separate the two-sentence
# compound and the per-clause grammar would fail on the second piece.
#
# Decimal numbers and abbreviations don't appear in MTG oracle text,
# so a simple ``period + whitespace + uppercase`` boundary suffices —
# no need for a fuller sentence tokenizer.
_SENTENCE_BOUNDARY_RE = re.compile(
    r"(?<=\.)\s+(?=[A-Z])(?!(?:If|When) you do,)"
)


def _split_sentences(line: str) -> list[str]:
    """Split a single oracle-text line at sentence boundaries.

    Returns ``[line]`` unchanged when the line contains no internal
    boundary. Otherwise returns the list of per-sentence pieces, each
    with surrounding whitespace stripped. Empty pieces are filtered
    out so a stray double-period or trailing whitespace doesn't
    inject blank clauses.

    The boundary is ``period + whitespace + uppercase letter``, with
    a negative lookahead for ``If you do,`` and ``When you do,`` so the
    ``You may pay/sacrifice/discard {COST}. If you do, …`` and
    ``You may sacrifice an artifact. When you do, …`` compound
    primitives don't get split — those are matched as a single phrase
    by the effect grammar and the embedded sentence boundary inside
    them is load-bearing.
    """
    pieces = [p.strip() for p in _SENTENCE_BOUNDARY_RE.split(line)]
    return [p for p in pieces if p]


# Spell-only keyword preamble: a leading clause that's just a static
# keyword followed by its parenthesized reminder text. The keyword
# line itself contributes no ``Effect`` — its behavior is wired via
# a flag on ``CardPrinting`` (e.g. Convoke → ``has_convoke=True``).
# Stripping this preamble lets the remaining clauses parse as a
# single-effect or Sequence spell.
#
# Some keywords here may be intentionally unimplemented (the engine
# treats the spell as if the keyword weren't there). Add a name only
# after confirming the matching ``cards/eoe_spells.py`` entry sets
# the corresponding printing flag — otherwise dropping the preamble
# would silently diverge from the static implementation.
_SPELL_PREAMBLE_KEYWORDS = ("Convoke",)
_SPELL_KEYWORD_PREAMBLE_RE = re.compile(
    r"^(?:" + "|".join(re.escape(k) for k in _SPELL_PREAMBLE_KEYWORDS)
    + r") \(.+\)\.?$"
)


# Kicker preamble: a leading clause of the form
# ``Kicker <cost> (You may pay an additional <cost> as you cast this
# spell.)``. The preamble itself contributes no ``Effect`` — its cost
# is wired onto the ``CardPrinting`` via the ``kicker`` kwarg, which
# the engine uses to allow the caster to pay the additional cost at
# cast time and the spell-effect to read ``state.resolving_kicked``
# at resolve time. The cost is captured as a mana-cost string (e.g.
# ``"{1}{U}"``) and threaded onto the printing verbatim.
#
# This shape is its own preamble (separate from the
# ``_SPELL_PREAMBLE_KEYWORDS`` whitelist) because Kicker carries a
# parameter (the cost), unlike Convoke which is a bare keyword.
_KICKER_PREAMBLE_RE = re.compile(
    r"^Kicker (?P<cost>(?:\{[^}]+\})+) \(.+\)\.?$"
)


# Additional-cost preamble: a leading clause of the form
# ``As an additional cost to cast this spell, <cost-clause>.``. The
# preamble itself contributes no ``Effect`` — its behavior is wired via
# a flag on ``CardPrinting`` (e.g. ``additional_cost_sacrifice_creature_
# or_artifact=True`` for Embrace Oblivion / Scrounge for Eternity).
# Stripping this preamble lets the remaining clauses parse as a
# single-effect or Sequence spell.
#
# The cost-clause body is matched against a small whitelist of
# recognized cost shapes; each maps to a specific printing-flag dict.
# Unrecognized cost clauses leave the spell unclaimed (the preamble
# isn't dropped) so the parser doesn't silently diverge from the
# static implementation.
_ADDITIONAL_COST_PREAMBLE_RE = re.compile(
    r"^As an additional cost to cast this spell, (.+)\.$"
)
# Recognized additional-cost bodies → printing flags. Keys are the
# captured ``(.+)`` group (without trailing period); values are the
# dict of flags applied to the resulting ``CardPrinting`` via
# ``make_instant`` / ``make_sorcery``.
_ADDITIONAL_COST_BODY_FLAGS: dict[str, dict] = {
    "sacrifice an artifact or creature": {
        "additional_cost_sacrifice_creature_or_artifact": True,
    },
    "choose a creature you control or a warped creature card "
    "you own in exile": {
        "additional_cost_choose_your_creature": True,
    },
    # Final Vengeance (Duskmourn) — "sacrifice a creature or
    # enchantment" parallel to Embrace Oblivion's
    # "sacrifice an artifact or creature".
    "sacrifice a creature or enchantment": {
        "additional_cost_sacrifice_creature_or_enchantment": True,
    },
}


# ---------------------------------------------------------------------
# Casting-substrate keyword lines + additional costs (cube spells-B)
# ---------------------------------------------------------------------
#
# Keyword lines that change HOW a spell is cast rather than what it does.
# Each is recognized after reminder-text stripping, removed from the
# effect-clause stream, and threaded onto the ``CardPrinting`` fields the
# cast pipeline (``actions._cast_spell`` / ``legal.py`` via
# ``mtgz_ir.alt_casting``) consumes:
#
#   ``Flashback {cost}`` -> ``flashback_cost``   (CR 702.34)
#   ``Delve``            -> ``has_delve``         (CR 702.66)
#   ``Buyback {cost}``   -> ``buyback_cost``      (CR 702.27)
#   ``Storm``            -> ``has_storm``         (CR 702.40)
_SPELL_CASTING_KEYWORD_LINE_RE = re.compile(
    r"^(?:(?P<fb>Flashback) (?P<fb_cost>(?:\{[^}]+\})+)|(?P<delve>Delve)|"
    r"(?P<storm>Storm)|"
    r"(?P<bb>Buyback) (?P<bb_cost>(?:\{[^}]+\})+))$"
)


# "If <condition>, you may cast it / this spell without paying its mana
# cost." — a conditional alternative (free) cast (CR 118.9: Once Upon a
# Time; Massacre's "If an opponent controls a Plains and you control a
# Swamp") -> ``free_cast_condition``.
_FREE_CAST_IF_LINE_RE = re.compile(
    r"^If (?P<cond>.+), you may cast (?:it|this spell) without paying its "
    r"mana cost\.$"
)


_FREE_CAST_CONDITIONS = {
    "this spell is the first spell you've cast this game": (
        FirstSpellYouCastThisGame()
    ),
}


# One clause of a control condition: "an opponent controls a[n] <Subtype>"
# / "you control a[n] <Subtype>" (Massacre and its Nemesis cycle: Submerge,
# Pulverize, Thwart ...). The subtype is a permanent's as it is (CR 205.3,
# 613.1d: ``HasSubtype`` reads the effective subtypes).
_CONTROL_SUBTYPE_CLAUSE_RE = re.compile(
    r"^(?P<who>an opponent controls|you control) an? (?P<sub>[A-Z][a-z]+)$"
)


def _free_cast_condition(text: str):
    """The condition of a free-cast line: a fixed phrase of
    ``_FREE_CAST_CONDITIONS``, or one or more control clauses joined by
    " and " (each ``_CONTROL_SUBTYPE_CLAUSE_RE``; all must hold)."""
    cond = _FREE_CAST_CONDITIONS.get(text)
    if cond is not None:
        return cond
    from mtgz_ir.effects.conditions import ControlPermanent, OpponentsControlAtLeast
    from mtgz_ir.effects.object_predicates import HasSubtype

    parts = []
    for clause in text.split(" and "):
        m = _CONTROL_SUBTYPE_CLAUSE_RE.match(clause)
        if m is None:
            return None
        pred = HasSubtype(m.group("sub"))
        if m.group("who") == "you control":
            parts.append(ControlPermanent(pred))
        else:
            parts.append(OpponentsControlAtLeast(pred, 1))
    out = parts[0]
    for p in parts[1:]:
        out = out & p
    return out


# "You may exile a <filter> card from your hand rather than pay this
# spell's mana cost." — pitch alternative cost (Pyrokinesis) ->
# ``alt_cost_exile_from_hand``.
_PITCH_ALT_COST_LINE_RE = re.compile(
    r"^You may exile (?P<desc>an? .+? card) from your hand rather than "
    r"pay this spell's mana cost\.$"
)


# "Escape—{cost}, Exile N other cards from your graveyard." (CR
# 702.138) -> ``escape_cost`` / ``escape_exile_count``.
_ESCAPE_LINE_RE = re.compile(
    r"^Escape—(?P<cost>(?:\{[^}]+\})+), [Ee]xile (?P<n>\w+) other cards "
    r"from your graveyard\.$"
)


# "Plot {cost}" (CR 702.170) -> ``plot_cost``.
_PLOT_LINE_RE = re.compile(r"^Plot (?P<cost>(?:\{[^}]+\})+)$")


# "Splice onto <subtype> {cost}" (CR 702.47) -> ``splice_onto`` /
# ``splice_cost``.
_SPLICE_LINE_RE = re.compile(
    r"^Splice onto (?P<sub>[A-Z][a-z]+) (?P<cost>(?:\{[^}]+\})+)$"
)


def _casting_keyword_fields(clause: str) -> dict | None:
    """``CardPrinting`` fields for a casting-keyword line, or ``None``
    when ``clause`` (reminder text already stripped) isn't one."""
    from mtgz_ir.cards.base import parse_mana_cost

    m = _ESCAPE_LINE_RE.match(clause.strip())
    if m is not None:
        from mtgz_ir.cards.parser.effects import _SMALL_CARDINAL_WORDS

        tok = m.group("n")
        n = int(tok) if tok.isdigit() else _SMALL_CARDINAL_WORDS.get(tok)
        if n is None:
            return None
        cost = m.group("cost")
        return {
            "escape_cost": parse_mana_cost(cost), "escape_cost_str": cost,
            "escape_exile_count": n,
        }
    m = _PLOT_LINE_RE.match(clause.strip())
    if m is not None:
        cost = m.group("cost")
        return {"plot_cost": parse_mana_cost(cost), "plot_cost_str": cost}
    m = _SPLICE_LINE_RE.match(clause.strip())
    if m is not None:
        cost = m.group("cost")
        return {
            "splice_onto": m.group("sub"),
            "splice_cost": parse_mana_cost(cost),
            "splice_cost_str": cost,
        }
    m = _PITCH_ALT_COST_LINE_RE.match(clause.strip())
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor

        f = parse_card_descriptor(m.group("desc"))
        if f is None:
            return None
        return {"alt_cost_exile_from_hand": f}

    m = _FREE_CAST_IF_LINE_RE.match(clause.strip())
    if m is not None:
        cond = _free_cast_condition(m.group("cond"))
        if cond is None:
            return None
        return {"free_cast_condition": cond}
    m = _SPELL_CASTING_KEYWORD_LINE_RE.match(clause.strip())
    if m is None:
        # Roto P2: flashback paid by sacrifice (Cabal Therapy).
        from mtgz_ir.cards.parser.roto_zones import (
            roto_zones_casting_keyword_fields,
        )

        fields = roto_zones_casting_keyword_fields(clause.strip())
        if fields is not None:
            return fields
        # Roto P4 casting keywords (Rebound).
        from mtgz_ir.cards.parser.roto_walkers import parse_roto_casting_keyword

        return parse_roto_casting_keyword(clause.strip())
    if m.group("fb"):
        cost = m.group("fb_cost")
        return {
            "flashback_cost": parse_mana_cost(cost),
            "flashback_cost_str": cost,
        }
    if m.group("delve"):
        return {"has_delve": True}
    if m.group("storm"):
        return {"has_storm": True}
    cost = m.group("bb_cost")
    return {"buyback_cost": parse_mana_cost(cost), "buyback_cost_str": cost}


# Additional-cost bodies modeled by the casting substrate (beyond the
# flag whitelist above): ``sacrifice <permanent descriptor>`` (Tinker's
# "an artifact", Natural Order's "a green creature", Crop Rotation's "a
# land") and ``pay X life`` (Toxic Deluge).
_ADDITIONAL_COST_SACRIFICE_BODY_RE = re.compile(r"^sacrifice (?P<desc>an? .+)$")


def _substrate_additional_cost_fields(body: str) -> dict | None:
    from mtgz_ir.cards.parser.card_flow_grammar import (
        parse_permanent_descriptor,
    )

    if body == "pay X life":
        return {"additional_cost_pay_x_life": True}
    m = _ADDITIONAL_COST_SACRIFICE_BODY_RE.match(body)
    if m is not None:
        f = parse_permanent_descriptor(m.group("desc"))
        if f is not None:
            return {"additional_cost_sacrifice_filter": f}
    # Vintage Roto P3 bodies ("discard X cards").
    from mtgz_ir.cards.parser.roto_mana import roto_additional_cost_fields

    return roto_additional_cost_fields(body)


# "<effect> instead if <condition>." replacement rider, optionally
# labelled with an ability word ("Threshold — "). Pairs with the
# preceding base clause into ``IfConditionElse(condition, then=<rider
# effect>, else=<base>)``. The condition grammar lives in
# ``_parse_instead_if_condition``.
_INSTEAD_IF_RIDER_RE = re.compile(
    r"^(?:[A-Z][a-z]+ — )?(?P<effect>.+?) instead if (?P<cond>.+)\.$"
)
_CARDS_IN_YOUR_GRAVEYARD_COND_RE = re.compile(
    r"^there are (?P<n>\w+) or more cards in your graveyard$"
)


def _parse_instead_if_condition(cond: str):
    from mtgz_ir.cards.parser.effects import _SMALL_CARDINAL_WORDS
    from mtgz_ir.effects.state_predicates import CardsInGraveyardAtLeast

    m = _CARDS_IN_YOUR_GRAVEYARD_COND_RE.match(cond)
    if m is not None:
        tok = m.group("n")
        n = int(tok) if tok.isdigit() else _SMALL_CARDINAL_WORDS.get(tok)
        if n is not None:
            return CardsInGraveyardAtLeast(n=n)
    return None


def _parse_instead_if_rider(
    clause: str, base: Effect, name: str,
) -> Effect | None:
    """``Threshold — Add {B}{B}{B}{B}{B} instead if there are seven or
    more cards in your graveyard.`` paired with base ``Add {B}{B}{B}.``
    -> ``IfConditionElse(CardsInGraveyardAtLeast(7), then=Add 5,
    else=Add 3)``. ``None`` if the rider or its condition doesn't
    parse."""
    from mtgz_ir.effects.combinators import IfConditionElse

    m = _INSTEAD_IF_RIDER_RE.match(clause.strip())
    if m is None:
        return None
    condition = _parse_instead_if_condition(m.group("cond"))
    if condition is None:
        return None
    then_effect = _parse_spell_effect_phrase(m.group("effect"), name)
    if then_effect is None:
        return None
    return IfConditionElse(
        condition=condition, then_effect=then_effect, else_effect=base,
    )


def _is_additional_cost_preamble(clause: str) -> bool:
    """Return ``True`` if ``clause`` is a recognized additional-cost
    preamble (``As an additional cost to cast this spell, <cost>.``
    where ``<cost>`` is one of the whitelisted cost bodies). The
    caller drops these preamble clauses before parsing the rest of
    the spell. Unrecognized cost bodies return ``False`` so the
    spell stays unclaimed rather than silently dropping the cost.
    """
    m = _ADDITIONAL_COST_PREAMBLE_RE.match(clause.strip())
    if m is None:
        return False
    return (
        m.group(1) in _ADDITIONAL_COST_BODY_FLAGS
        or _substrate_additional_cost_fields(m.group(1)) is not None
    )


def _is_substrate_cost_line(line: str) -> bool:
    """Whether a printing line is owned by the casting substrate
    (additional-cost preambles in the flag table / substrate fields,
    casting keywords incl. the plain pitch alternative cost), so the
    spells-A ``spell_cost_lines`` detector must not also model it."""
    if _is_additional_cost_preamble(line):
        return True
    clause = _strip_reminder_text(line.strip())
    return bool(clause) and _casting_keyword_fields(clause) is not None


def _additional_cost_flags(clause: str) -> dict:
    """Return the printing-flag dict for a recognized additional-cost
    preamble ``clause``. Returns an empty dict if the clause isn't a
    recognized preamble (caller should ``_is_additional_cost_preamble``
    first to gate the call). Keeps the per-shape flag mapping
    centralized in ``_ADDITIONAL_COST_BODY_FLAGS`` so adding a new
    cost shape is a one-line entry there plus a one-line dispatch
    here is the only edit needed.
    """
    m = _ADDITIONAL_COST_PREAMBLE_RE.match(clause.strip())
    if m is None:
        return {}
    return _ADDITIONAL_COST_BODY_FLAGS.get(m.group(1), {})


def _is_spell_keyword_preamble(clause: str) -> bool:
    """Return ``True`` if ``clause`` is a recognized unimplemented
    spell-keyword preamble (keyword name + parenthesized reminder
    text). The caller drops these preamble clauses before parsing the
    rest of the spell. See ``_SPELL_PREAMBLE_KEYWORDS`` for the
    whitelist."""
    return _SPELL_KEYWORD_PREAMBLE_RE.match(clause.strip()) is not None


def _is_kicker_preamble(clause: str) -> bool:
    """Return ``True`` if ``clause`` is a Kicker preamble of the form
    ``Kicker <cost> (You may pay an additional <cost> as you cast this
    spell.)``. The caller drops these preamble clauses before parsing
    the rest of the spell. The cost is extracted separately by
    ``_kicker_cost`` and threaded onto the resulting ``CardPrinting``
    via the ``kicker`` kwarg.
    """
    return _KICKER_PREAMBLE_RE.match(clause.strip()) is not None


def _kicker_cost(clause: str) -> str | None:
    """Return the kicker cost string (e.g. ``"{1}{U}"``) for a
    recognized Kicker preamble ``clause``, or ``None`` if the clause
    isn't a Kicker preamble. The cost is the verbatim mana-cost
    string captured between ``Kicker `` and the parenthesized reminder
    — the engine accepts the same surface form on the
    ``CardPrinting.kicker_cost_str`` field, so no further parsing is
    needed here.
    """
    m = _KICKER_PREAMBLE_RE.match(clause.strip())
    if m is None:
        return None
    return m.group("cost")


def _parse_void_damage_replacement(
    phrase: str, name: str, base_effect: Effect
) -> Effect | None:
    """If ``phrase`` is a Void rider replacing the damage amount of
    ``base_effect``, return the void-branch ``Effect`` (same shape as
    base, new amount). Returns ``None`` otherwise.

    Currently only handles ``DealDamageAnyTarget`` as the base — the
    only damage shape that pairs with a Void rider in the EoE pool we
    support today (Plasma Bolt). Extend per-card as new pairings show up.
    """
    if not isinstance(base_effect, DealDamageAnyTarget):
        return None
    name_re = re.escape(name)
    m = re.match(
        _SPELL_VOID_DAMAGE_REPLACEMENT.format(name=name_re), phrase.strip()
    )
    if m is None:
        return None
    return DealDamageAnyTarget(amount=int(m.group(1)))


def _parse_void_instead_effect_rider(
    phrase: str, base_effect: Effect, name: str
) -> Effect | None:
    """If ``phrase`` is a Void "instead" rider (``Void — If a nonland
    permanent left the battlefield this turn or a spell was warped
    this turn, instead <then-effect>.``), parse the then-branch and
    return ``ConditionalIfVoid(then=then_effect, else=base_effect)``.
    Returns ``None`` if the phrase doesn't match or the then-branch
    doesn't parse.

    The "instead" keyword means the rider replaces the base effect
    entirely when Void is active (distinct from the adds-draw rider
    which appends, and from the damage/pump replacement helpers which
    swap a specific field). The ``else_effect`` is the original
    ``base_effect`` — the spell resolves as the base when Void is not
    active.

    Currently supports any parseable then-branch; no restriction on
    the base-effect type. The only EoE/Duskmourn card using this shape
    today is Decode Transmissions (base = ``Sequence(DrawCards(2),
    LoseLife(2))``, then = ``Sequence(DrawCards(2), EachOpponentLosesLife
    (2))`` parsed from ``"you draw two cards and each opponent loses 2
    life"``). Extend as new "instead" Void riders ship.
    """
    m = _SPELL_VOID_INSTEAD_EFFECT_RE.match(phrase.strip())
    if m is None:
        return None
    then_phrase = m.group(1).strip()
    then_effect = _parse_spell_effect_phrase(then_phrase, name)
    if then_effect is None:
        return None
    return ConditionalIfVoid(
        then_effect=then_effect,
        else_effect=base_effect,
    )


def _is_void_adds_draw_rider(phrase: str) -> bool:
    """Return ``True`` if ``phrase`` is the Void "adds-a-draw" rider
    from Hymn of the Faller:
    ``"Void — If a nonland permanent left the battlefield this turn
    or a spell was warped this turn, draw another card."``

    This rider runs an *additional* ``DrawCards(1)`` on top of the
    base effects (not a replacement like the damage/pump helpers).
    When paired with a Sequence base effect, the caller wraps the
    base Sequence with an appended
    ``ConditionalIfVoid(then=DrawCards(1), else=None)`` clause.
    """
    return _SPELL_VOID_ADDS_DRAW_RE.match(phrase.strip()) is not None


def _parse_void_pump_replacement(
    phrase: str, base_effect: Effect
) -> Effect | None:
    """If ``phrase`` is a Void rider replacing the pump bonuses of
    ``base_effect``, return the void-branch ``Effect`` (same shape as
    base, new bonuses). Returns ``None`` otherwise.

    Currently only handles ``PumpTargetCreatureUntilEOT`` as the base —
    the only pump shape that pairs with a Void rider in the EoE pool
    we support today (Tragic Trajectory). The carry-over subject
    ``That creature`` refers back to the target chosen on the first
    clause; the engine reuses the same target when resolving either
    branch (single-target spell).

    Mirrors the structure of ``_parse_void_damage_replacement`` —
    keeping the two as separate helpers (vs one polymorphic function)
    keeps the per-shape regex literal explicit and the per-shape
    engine class wiring obvious. Extend per-card as new pairings
    surface; e.g. a future Void rider replacing
    ``DealDamageToTargetCreature`` would land as a third helper.

    The ``base_effect``'s bonuses are not used to compute the void
    branch — only the type is matched. The void branch's bonuses
    come entirely from the rider clause itself.
    """
    if not isinstance(base_effect, PumpTargetCreatureUntilEOT):
        return None
    m = _SPELL_VOID_PUMP_REPLACEMENT.match(phrase.strip())
    if m is None:
        return None
    return PumpTargetCreatureUntilEOT(
        power_bonus=int(m.group(1)),
        toughness_bonus=int(m.group(2)),
    )


# Grandfathered users of the exactly-one-parseable-mode fallback below.
# The fallback drops the unparsed modes, which the parser's
# no-silent-no-op rule forbids for new cards; these printings claimed
# through it before that rule was enforced and are pinned by existing
# tests. Any other modal spell whose modes don't ALL parse stays
# unclaimed (e.g. Suplex, whose "exile target artifact" mode parses
# but whose damage + exile-if-it-would-die mode does not).
#
# These printings are ALSO parsed with the pre-extension grammar
# (``effects_ext.legacy_grammar``): their claimed shape is pinned by the
# EoE cross-engine parity probes, so the cube-era extension recognizers
# must not change it (Break Down the Door would otherwise become a full
# three-mode ModalChooseOne the Rust bridge doesn't seat).
_LEGACY_PARTIAL_MODAL_CLAIMS: frozenset[str] = frozenset({
    "Break Down the Door",
    "Get Out",
    "Sheoldred's Edict",
    "Untimely Malfunction",
})


def _parse_modal_choose_one(
    clauses: list[str], name: str
) -> Effect | None:
    """If ``clauses`` is a ``Choose one —`` or ``Choose one or both —``
    modal-spell shape, return the corresponding ``ModalChooseOne``
    effect; otherwise ``None``.

    Recognized shapes (from Scryfall printed text):

      ``Choose one —``               (or ``Choose one or both —``)
      ``• <mode 1 sentence>``
      ``• <mode 2 sentence>``
      ``• <mode 3 sentence>``  (optional)
      ...

    Each mode line is parsed by ``_parse_spell_effect_phrase`` after
    stripping the leading ``• `` (bullet + space).

    **All-or-nothing rule (normal path):** if all modes parse, returns
    ``ModalChooseOne(modes=(...), or_both=or_both)`` so the engine routes
    mode-pick through the agent's ``choose_modes`` hook.

    **Exactly-one-parseable fallback (parser-is-right):** if the mode
    lines are all well-formed bullets but EXACTLY ONE mode parses (the
    rest return ``None``), the parser returns that one mode's effect
    directly rather than ``ModalChooseOne``. This matches the same
    "parser-is-right" simplification the whole-spell exact-text table
    already ships for Get Out (mode 1 = counter creature/enchantment;
    mode 2 unrecognized) and Untimely Malfunction (mode 1 = destroy
    artifact; modes 2–3 unrecognized). The engine can only model the
    parseable mode anyway, so presenting the spell as a non-modal spell
    with that fixed effect is the most faithful available representation
    given the current grammar.

    If TWO OR MORE modes parse but not all, the function returns ``None``
    (the spell stays unclaimed) — a multi-mode partial parse can't be
    simplified to a single fixed effect without ambiguity about WHICH
    parseable mode to use.

    A header by itself (zero mode lines) or a header with exactly one
    mode line is rejected — printed modal spells always have at least
    two modes. ``Choose two`` and other variants don't match either
    header regex and fall through. The ``or both`` shape constructs
    ``ModalChooseOne(or_both=True)`` so the engine routes mode-pick
    through the agent's ``choose_modes`` (plural) hook.
    """
    if not clauses:
        return None
    if _MODAL_CHOOSE_ONE_HEADER_RE.match(clauses[0]) is not None:
        or_both = False
    elif _MODAL_CHOOSE_ONE_OR_BOTH_HEADER_RE.match(clauses[0]) is not None:
        or_both = True
    elif _DELIRIUM_MODAL_HEADER_RE.match(clauses[0]) is not None:
        # ``Delirium — Choose one. If there are four or more card types
        # among cards in your graveyard, choose one or more instead.``
        # (Let's Play a Game). Treated as a bare ``Choose one —`` shape
        # (``or_both=False``) — the Delirium upgrade to "choose one or
        # more instead" is dropped because the engine has no Delirium-
        # conditional modal-count combinator today.  The mode lines that
        # follow still parse normally through the unified per-mode
        # grammar; the printed Delirium threshold is silently discarded.
        or_both = False
    else:
        return None
    mode_lines = clauses[1:]
    if len(mode_lines) < 2:
        return None
    # Try to parse every mode. Collect (index, effect) pairs for the
    # modes that parse cleanly; track which are malformed bullets.
    parsed_modes: list[Effect] = []
    malformed = False
    for line in mode_lines:
        m = _MODAL_MODE_LINE_RE.match(line)
        if m is None:
            # Bullet not recognized → whole modal shape is unclaimed
            # (a non-bullet line inside a modal body is unexpected and
            # may indicate a multi-line mode, which we don't support).
            malformed = True
            break
        if name in _LEGACY_PARTIAL_MODAL_CLAIMS:
            from mtgz_ir.cards.parser.effects_ext import legacy_grammar

            with legacy_grammar():
                eff = _parse_spell_effect_phrase(m.group(1), name)
        else:
            eff = _parse_spell_effect_phrase(m.group(1), name)
        if eff is not None:
            parsed_modes.append(eff)
        # else: append None placeholder? No — we only collect parseable modes.
    if malformed:
        return None
    if len(parsed_modes) == len(mode_lines):
        # All modes parse → full ModalChooseOne.
        return ModalChooseOne(modes=tuple(parsed_modes), or_both=or_both)
    if (
        len(parsed_modes) == 1
        and not or_both
        and name in _LEGACY_PARTIAL_MODAL_CLAIMS
    ):
        # Exactly one mode parseable (and this is a "Choose one —" spell,
        # not "Choose one or both —") → parser-is-right: return that mode's
        # effect directly. Same simplification as Get Out (mode 1 = counter
        # creature/enchantment spell; mode 2 unrecognized) and Untimely
        # Malfunction (mode 1 = destroy artifact; modes 2–3 unrecognized).
        # The "or both" guard ensures "Choose one or both —" spells stay
        # unclaimed when not all modes parse — losing the or_both semantics
        # would silently misbehave if the parseable mode is returned alone.
        return parsed_modes[0]
    # Zero parseable, or 2+ parseable but not all, or or_both with any
    # unrecognized mode → unclaimed.
    return None


# Historical note: a whole-spell exact-text dispatch table
# (``_WHOLE_SPELL_EXACT_TEXT``) used to live here for spells whose
# entire oracle text was a single sentence-block modeled with a
# dedicated parameter-free engine class. Every entry was migrated
# to per-clause / multi-sentence-literal recognizers in
# ``effects.py``; the dispatch dict and its ``_try_whole_spell_exact_text``
# wrapper were retired in retire-empty-bespoke-shells once the table
# sat empty.


def _parse_spell_oracle_text(text: str, name: str) -> Effect | None:
    """Parse a spell's full oracle text into a single ``Effect``.

    Single-line: parsed by ``_parse_spell_effect_phrase`` directly.
    Multi-line: each ``\\n``-separated clause is parsed; clauses are
    combined either via ``ConditionalIfVoid`` (when the second clause
    is a Void replacement of the first damage effect), via
    ``ModalChooseOne`` (Scryfall ``Choose one —`` or
    ``Choose one or both —`` shape), or via ``Sequence`` (every clause
    is a plain spell-effect sentence).
    Leading clauses that are unimplemented spell-keyword preambles
    (e.g. Vote Out's Convoke) are dropped before combinator dispatch
    so they don't fail the per-clause parse. ``This spell costs {N}
    less to cast if <condition>.`` cost-reduction riders are likewise
    dropped here — the rule is captured separately by
    ``_detect_cost_reductions`` and threaded onto the resulting
    ``CardPrinting`` via the ``cost_reductions`` kwarg.

    A single oracle-text line that packs two effect sentences
    separated by ``. `` (Radiant Strike's
    ``"Destroy target artifact or tapped creature. You gain 3 life."``,
    Sami's Curiosity's ``"You gain 2 life. Create a Lander token."``)
    is further carved at the sentence boundary by ``_split_sentences``
    after reminder-text stripping and cost-reduction dropping. Each
    resulting sentence is then a clause in its own right — the
    Sequence combinator path picks them up the same way it would for
    a printing that put each sentence on its own ``\\n``-separated
    line. Returns ``None`` if any surviving clause fails to parse.
    """
    with legacy_parity_pinned(name):
        return _parse_spell_oracle_text_body(text, name)


def _parse_spell_oracle_text_body(text: str, name: str) -> Effect | None:
    """Body of :func:`_parse_spell_oracle_text`, run under the
    Rust-parity pin for the legacy spells it applies to."""
    clauses = [c.strip() for c in text.split("\n") if c.strip()]
    # Vintage Roto P3 header shapes (Jeska's Will's commander clause).
    from mtgz_ir.cards.parser.roto_mana import rewrite_spell_clauses

    clauses = rewrite_spell_clauses(clauses)
    # Drop leading spell-keyword preambles BEFORE per-clause reminder
    # stripping. The preamble shape (``"Convoke (...)"``) is recognized
    # via the trailing parenthesized reminder, so we have to detect
    # those preamble clauses while their parentheses are still intact.
    while clauses and _is_spell_keyword_preamble(clauses[0]):
        clauses = clauses[1:]
    # Drop leading recognized additional-cost preambles. The preamble
    # itself contributes no ``Effect`` — the corresponding printing
    # flag is threaded onto the resulting ``CardPrinting`` separately
    # via ``_detect_additional_cost_flags`` (called from
    # ``_build_spell_printing``). Unrecognized cost bodies leave the
    # preamble in place; the spell will fail to parse downstream and
    # stay unclaimed rather than silently dropping the cost.
    while clauses and _is_additional_cost_preamble(clauses[0]):
        clauses = clauses[1:]
    # Drop alternative-cost (incl. Overload) / one-of additional-cost /
    # "This spell can't be countered." lines (``spell_cost_lines``);
    # ``_build_spell_printing`` threads them onto the printing via
    # ``detect_spell_line_fields``.
    clauses = [c for c in clauses if not is_spell_cost_line(c)]
    # Drop a leading Kicker preamble. The kicker cost is threaded onto
    # the ``CardPrinting`` separately via ``_detect_kicker_cost``
    # (called from ``_build_spell_printing``); the preamble line
    # itself contributes no ``Effect``.
    while clauses and _is_kicker_preamble(clauses[0]):
        clauses = clauses[1:]
    # Now drop per-clause parenthesized reminder text. Empty results
    # (a clause that was nothing but a parenthetical) are filtered out.
    clauses = [
        stripped
        for stripped in (_strip_reminder_text(c) for c in clauses)
        if stripped
    ]
    # Drop casting-keyword lines (Flashback / Delve / Buyback) — they
    # change how the spell is cast, not what it does; the fields are
    # threaded onto the printing by ``_detect_casting_substrate_fields``.
    clauses = [c for c in clauses if _casting_keyword_fields(c) is None]
    # Drop Delirium-prefix rider clauses BEFORE the cost-reduction pass.
    # These ``Delirium — …`` lines are unimplemented mechanics that the
    # parser drops entirely — doing so before the cost-reduction pass
    # ensures that Delirium cost-reduction lines (Drag to the Roots's
    # ``Delirium — This spell costs {2} less to cast as long as …``)
    # are not forwarded to ``_detect_cost_reductions`` and do not
    # produce spurious ``CostReduction`` rules on the printing.
    clauses = [c for c in clauses if not _is_delirium_drop_clause(c)]
    # Drop recognized cost-reduction rider clauses — they don't
    # contribute an ``Effect`` (the engine consumes them at cast time
    # via ``CostReduction`` rules on the printing). ``_build_spell_
    # printing`` calls ``_detect_cost_reductions`` separately to thread
    # those rules onto the resulting ``CardPrinting``.
    clauses = [
        c for c in clauses if _parse_cost_reduction_line(c) is None
    ]
    # Carve any line that packs two effect sentences on a single
    # ``. ``-separated line into independent clauses. Done AFTER
    # reminder-text stripping (so quoted/parenthesized inner periods
    # don't fire) and AFTER cost-reduction dropping (so a multi-
    # sentence rider — none ship today, but defensive — never gets
    # half-dropped). Single-sentence clauses round-trip unchanged.
    #
    # A clause whose full multi-sentence text already matches a per-
    # clause grammar primitive (e.g. Orbital Plunge's
    # ``X deals 6 damage to target creature. If excess damage was
    # dealt this way, create a Lander token.`` → the dedicated multi-
    # sentence recognizer in ``effects.py``) is kept whole — splitting
    # would carve the rider's anaphoric ``If`` clause into a piece
    # that has no standalone primitive. Per-clause recognizers that
    # match the unsplit text take precedence; only clauses that fail
    # to parse whole are split into per-sentence pieces.
    new_clauses: list[str] = []
    for c in clauses:
        if _parse_spell_effect_phrase(c, name) is not None:
            new_clauses.append(c)
        elif _DELIRIUM_MODAL_HEADER_RE.match(c) is not None:
            # The Delirium modal header ``Delirium — Choose one. If
            # there are four or more card types …, choose one or more
            # instead.`` is a two-sentence clause that must NOT be
            # split: splitting would carve it into a bare
            # ``Delirium — Choose one.`` fragment (which the modal
            # parser won't recognize) and a dangling ``If …``
            # fragment. Keep it whole so ``_parse_modal_choose_one``
            # can match the full header against
            # ``_DELIRIUM_MODAL_HEADER_RE`` on the next pass.
            new_clauses.append(c)
        elif _MODAL_MODE_LINE_RE.match(c) is not None:
            # Modal mode lines (``• <sentence>``) must NOT be sentence-
            # split, even when the mode sentence contains an embedded
            # period (e.g. Warping Wail mode 3:
            # ``• Create a 1/1 colorless Eldrazi Scion creature token.
            # It has "Sacrifice this token: Add {C}."``) — splitting
            # would carve the second sentence into an unrecognizable
            # fragment that makes the modal shape unclaimable. Keep mode
            # lines whole so ``_parse_modal_choose_one`` can strip the
            # leading ``• `` and parse the full sentence as a mode.
            new_clauses.append(c)
        else:
            new_clauses.extend(_split_sentences(c))
    clauses = new_clauses
    # Drop recognized self-shuffle rider clauses — they contribute no
    # ``Effect`` (the engine consumes them via the SELF-scope
    # StaticAbility carrying the
    # ``ReplacementEffect(WOULD_GO_TO_GRAVEYARD,
    # Replace(new_destination="library"))`` wired by
    # ``_detect_shuffle_self_statics``): ``If <Name> would be put into a
    # graveyard from anywhere, reveal <Name> and shuffle it into its
    # owner's library instead.`` (Nexus of Fate). Green Sun's Zenith's
    # ``Shuffle <Name> into its owner's library.`` is an instruction of
    # its effect and stays (``ShuffleThisSpellIntoOwnersLibrary``).
    # Dropping happens AFTER the sentence-splitting pass so a rider split
    # off a single oracle line by ``_split_sentences`` is visible here
    # for dropping. Must also happen BEFORE the ``len(clauses) == 1``
    # short-circuit so spells whose only surviving clause after dropping is
    # the main effect (Nexus of Fate: take-extra-turn) return that effect
    # cleanly via the single-clause path rather than trying to Sequence it
    # with an empty clause list.
    clauses = [c for c in clauses if not _is_self_shuffle_clause(c)]
    if not clauses:
        return None
    if len(clauses) == 1:
        return _parse_spell_effect_phrase(clauses[0], name)
    # Two-clause Void-replacement shape (Plasma Bolt's damage rider,
    # Tragic Trajectory's pump rider). The dispatch tries each
    # supported base/rider pairing in turn and picks the first that
    # both parses the base-clause cleanly AND matches the rider's
    # surface form for that base shape.
    if len(clauses) == 2:
        first = _parse_spell_effect_phrase(clauses[0], name)
        if first is not None:
            void_then = _parse_void_damage_replacement(
                clauses[1], name, first
            )
            if void_then is None:
                void_then = _parse_void_pump_replacement(clauses[1], first)
            if void_then is not None:
                return ConditionalIfVoid(
                    then_effect=void_then, else_effect=first
                )
            # Hymn-of-the-Faller-shaped "adds-a-draw" Void rider:
            # the rider APPENDS a ConditionalIfVoid(then=DrawCards(1),
            # else=None) to the base effect. The base must be a
            # Sequence (multi-step spell body); the appended conditional
            # extends the Sequence rather than replacing any step.
            # Tried after the replacement helpers since the adds-draw
            # shape is orthogonal (it doesn't depend on the type of
            # ``first``).
            if _is_void_adds_draw_rider(clauses[1]):
                # Flatten any top-level Sequence so the result is a
                # single flat Sequence (not a nested Sequence-of-
                # Sequences). The base clause parses via the compound
                # splitter which may itself return a Sequence (e.g.
                # Hymn of the Faller's ``Surveil 1, then you draw a
                # card and lose 1 life.`` → ``Sequence(Sequence(Surveil,
                # DrawCards), LoseLife)``). Flattening keeps the
                # produced effect structurally identical to the static-
                # registry model (a single flat Sequence whose last
                # element is the ConditionalIfVoid).
                def _flatten(eff: Effect) -> tuple[Effect, ...]:
                    if isinstance(eff, Sequence):
                        flat: list[Effect] = []
                        for e in eff.effects:
                            flat.extend(_flatten(e))
                        return tuple(flat)
                    return (eff,)
                base_effects = _flatten(first)
                return Sequence(effects=(
                    *base_effects,
                    ConditionalIfVoid(
                        then_effect=DrawCards(amount=1),
                        else_effect=None,
                    ),
                ))
            # Void "instead" rider: the rider replaces the base effect
            # entirely when Void is active (Decode Transmissions's
            # ``Void — ... instead you draw two cards and each opponent
            # loses 2 life.`` rider). ``_parse_void_instead_effect_rider``
            # parses the then-branch and returns a ConditionalIfVoid
            # whose else-branch is the base effect. Tried after the
            # adds-draw rider since the "instead" keyword is a distinct
            # surface form that the adds-draw matcher doesn't fire on.
            instead_eff = _parse_void_instead_effect_rider(
                clauses[1], first, name
            )
            if instead_eff is not None:
                return instead_eff
            # Unimplemented secondary-rider drop: when the first clause
            # parses cleanly and the second clause is a known-
            # unimplemented rider (controller-manifest, death-trigger-
            # manifest, optional-graveyard-return), return just the
            # first-clause effect — mirroring the simplification the
            # bespoke entries already make (entries 23, 28, 34).
            # "<effect> instead if <condition>." replacement rider
            # (Cabal Ritual's Threshold line).
            instead_if = _parse_instead_if_rider(clauses[1], first, name)
            if instead_if is not None:
                return instead_if
            counter_rider = _parse_counterspell_rider(clauses[1], first, name)
            if counter_rider is not None:
                return counter_rider
            if _is_unimplemented_secondary_rider(clauses[1]):
                return first
    # General N-clause unimplemented-rider drop: if the first clause
    # parses cleanly AND all remaining clauses (2 through N) are
    # unimplemented secondary riders, return the first-clause effect.
    # Handles spells like Come Back Wrong whose three-sentence body
    # has one parseable base clause + two unimplemented rider clauses
    # (conditional reanimate + delayed-sacrifice trigger).  Only
    # reaches here when ``len(clauses) >= 3`` (the ``len == 2`` path
    # above already handles the two-clause case).
    if len(clauses) >= 3:
        first_multi = _parse_spell_effect_phrase(clauses[0], name)
        if first_multi is not None and all(
            _is_unimplemented_secondary_rider(c) for c in clauses[1:]
        ):
            return first_multi
    # Modal shape: ``Choose one —`` / ``Choose one or both —`` header
    # line + ≥2 bullet mode lines. Tried before the Sequence fallback
    # because Sequence would uselessly attempt to parse the modal
    # header as a spell-effect sentence, fail, and return None —
    # losing any modal spell whose modes are individually parseable.
    # Multi-mode shapes (Choose two / three with repeats / one or more
    # with Escalate / Spree) — ``modal_spells``.
    if precise_grammar_active():
        multi = parse_multi_modal(clauses, name)
        if multi is not None:
            return multi
    modal = _parse_modal_choose_one(clauses, name)
    if modal is not None:
        return modal
    # Base clause + "... instead if <condition>" rider line (Unholy
    # Heat's Delirium, Fatal Push's Revolt) — ``conditional_phrases``.
    if precise_grammar_active():
        instead = combine_instead_rider(clauses, name)
        if instead is not None:
            return instead
    # Sequence shape: every clause must be a plain spell-effect sentence.
    parsed: list[Effect] = []
    for clause in clauses:
        eff = _parse_spell_effect_phrase(clause, name)
        if eff is None:
            return None
        parsed.append(eff)
    return Sequence(effects=tuple(parsed))


def _detect_preamble_flags(text: str) -> dict:
    """Scan ``text``'s leading keyword-preamble clauses and return a
    dict of ``CardPrinting`` flags they imply (e.g.
    ``{"has_convoke": True}`` when "Convoke (...)" leads the oracle
    text). Mirrors the preamble-stripping in
    ``_parse_spell_oracle_text``."""
    flags: dict = {}
    for clause in [c.strip() for c in text.split("\n") if c.strip()]:
        if not _is_spell_keyword_preamble(clause):
            break
        if clause.startswith("Convoke"):
            flags["has_convoke"] = True
    return flags


def _detect_additional_cost_flags(text: str) -> dict:
    """Scan ``text``'s leading additional-cost preamble clauses and
    return a dict of ``CardPrinting`` flags they imply (e.g.
    ``{"additional_cost_sacrifice_creature_or_artifact": True}`` when
    ``"As an additional cost to cast this spell, sacrifice an artifact
    or creature."`` leads the oracle text).

    Mirrors the preamble-stripping in ``_parse_spell_oracle_text`` —
    iterates through leading clauses while each is a recognized
    additional-cost preamble, accumulates the flags, and stops at the
    first non-preamble clause. Multiple stacked additional-cost
    preambles aren't a printed shape today, but the loop handles them
    naturally if a future card ships them.

    Walks past keyword-preamble clauses (the existing Convoke shape)
    in case a future card stacks both shapes (Convoke + additional
    cost). Today no card does, but skipping them keeps the dispatcher
    order-insensitive: the flag set returned doesn't depend on
    whether the spell author put the additional-cost line first or
    after the keyword preamble.
    """
    flags: dict = {}
    for clause in [c.strip() for c in text.split("\n") if c.strip()]:
        if _is_spell_keyword_preamble(clause):
            continue
        if not _is_additional_cost_preamble(clause):
            break
        flags.update(_additional_cost_flags(clause))
    return flags


# Self-shuffle replacement: a clause of the form "If <Name> would be put
# into a graveyard from anywhere, reveal <Name> and shuffle it into its
# owner's library instead." (Nexus of Fate; Blightsteel Colossus on a
# permanent). The parser models it as a SELF-scope
# ``StaticAbility(replacement_effect=ReplacementEffect(
# WOULD_GO_TO_GRAVEYARD, Replace(new_destination="library")))`` on the
# printing — so detecting the rider in a card's oracle text threads the
# StaticAbility onto the resulting ``CardPrinting`` without contributing
# an ``Effect``. Green Sun's Zenith's "Shuffle <Name> into its owner's
# library." is not a replacement (it is done only as the spell
# resolves): it is the ``ShuffleThisSpellIntoOwnersLibrary`` step of the
# spell's effect.
#
# The shape interpolates the card's own name and is matched after
# escaping the name. Non-matching cards (no shuffle rider in their
# text) round-trip through ``_detect_shuffle_self_statics`` with an
# empty tuple.
_SHUFFLE_SELF_NEXUS_RE_TEMPLATE = (
    r"If {name} would be put into a graveyard from anywhere, "
    r"reveal {name} and shuffle it into its owner's library instead\."
)


def _detect_shuffle_self_statics(
    text: str, name: str,
) -> tuple[StaticAbility, ...]:
    """Return a one-element tuple containing a SELF-scope
    ``StaticAbility`` carrying a ``ReplacementEffect(WOULD_GO_TO_
    GRAVEYARD, Replace(new_destination="library"))`` if ``text``
    contains a recognized self-shuffle replacement for ``name``; an
    empty tuple otherwise.

    One shape is recognized: ``If <Name> would be put into a graveyard
    from anywhere, reveal <Name> and shuffle it into its owner's library
    instead.`` (Nexus of Fate). Appears as the spell's second oracle
    line.

    The matcher uses ``re.search`` (not ``re.match``) so it finds the
    rider regardless of where in the spell text it appears. Each
    pattern interpolates ``re.escape(name)`` so a card name with regex
    metacharacters (e.g. apostrophes, periods) doesn't break the
    match. Non-matching texts return an empty tuple.

    The returned ``StaticAbility`` rides on the spell printing's
    ``static_abilities`` tuple. The dispatcher's moving-card walker
    in :func:`mtgz_ir.effects.modifiers._collect_replacement_effects`
    visits ``card.printing.static_abilities`` for events that pass a
    ``"card"`` subject (today: ``WOULD_GO_TO_GRAVEYARD`` from
    ``actions._send_to_graveyard_or_library``); the implicit
    ``card.owner_id == affected_player_id`` gate keys the replacement
    on the spell's owner.
    """
    name_re = re.escape(name)
    patterns = (_SHUFFLE_SELF_NEXUS_RE_TEMPLATE.format(name=name_re),)
    for p in patterns:
        if re.search(p, text) is not None:
            return (StaticAbility(
                scope=StaticScope.SELF,
                replacement_effect=ReplacementEffect(
                    event_pattern=EventPattern(
                        event=Event.WOULD_GO_TO_GRAVEYARD,
                    ),
                    transform=Replace(new_destination="library"),
                ),
            ),)
    return ()


def _detect_kicker_cost(text: str) -> str | None:
    """Return the Kicker cost string (e.g. ``"{1}{U}"``) if ``text``
    has a leading Kicker preamble, else ``None``. Mirrors the
    preamble-stripping in ``_parse_spell_oracle_text``: walks past
    the existing keyword (Convoke) and additional-cost preambles —
    today no card stacks Kicker behind another preamble, but the
    walk keeps the dispatcher order-insensitive should a future card
    do so. Returns the cost of the FIRST recognized Kicker preamble;
    multi-Kicker spells (which would carry stacked Kicker lines)
    aren't a printed shape.
    """
    for clause in [c.strip() for c in text.split("\n") if c.strip()]:
        if _is_spell_keyword_preamble(clause):
            continue
        if _is_additional_cost_preamble(clause):
            continue
        if _is_kicker_preamble(clause):
            return _kicker_cost(clause)
        break
    return None


def _detect_cost_reductions(text: str) -> tuple[CostReduction, ...]:
    """Return every cost-reduction rule implied by ``text``'s recognized
    ``This spell costs {N} less to cast if <condition>.`` clauses, in
    the order they appear. Reminder text is stripped per clause
    before recognition, so a rider buried after a parenthesized
    flavor still matches.

    Mirrors the rider-dropping pass in ``_parse_spell_oracle_text``;
    callers pass the result via the ``cost_reductions`` kwarg on
    ``make_instant`` / ``make_sorcery``.
    """
    rules: list[CostReduction] = []
    for raw in text.split("\n"):
        clause = _strip_reminder_text(raw)
        if not clause:
            continue
        rule = _parse_cost_reduction_line(clause)
        if rule is not None:
            rules.append(rule)
    return tuple(rules)


# Modal DFC land back face (CR 712): "As this land enters, you may pay
# N life. If you don't, it enters tapped." + "{T}: Add {C}." — the
# pathway-style MDFC land. ``shock_land_life_cost`` carries the optional
# life payment (the same ETB choice shock lands use).
_MDFC_LAND_PAY_LIFE_LINE_RE = re.compile(
    r"^As this land enters, you may pay (?P<n>\d+) life\. If you don't, it "
    r"enters tapped\.$"
)
_MDFC_LAND_TAP_ADD_LINE_RE = re.compile(r"^\{T\}: Add \{(?P<c>[WUBRGC])\}\.$")


def _build_mdfc_land_face(face: dict) -> CardPrinting | None:
    """Build the land back face of a modal DFC, or ``None`` if its text
    isn't the pay-life-or-tapped + single-color tap shape."""
    supertypes, card_types, subtypes = _parse_type_line(face.get("type_line", ""))
    if "Land" not in card_types:
        return None
    lines = [ln.strip() for ln in face.get("oracle_text", "").split("\n") if ln.strip()]
    if len(lines) != 2:
        return None
    life = _MDFC_LAND_PAY_LIFE_LINE_RE.match(lines[0])
    tap = _MDFC_LAND_TAP_ADD_LINE_RE.match(lines[1])
    if life is None or tap is None:
        return None
    return CardPrinting(
        name=face["name"],
        mana_cost_str="",
        mana_cost=parse_mana_cost(""),
        type_line=face["type_line"],
        supertypes=tuple(supertypes),
        card_types=tuple(card_types),
        subtypes=tuple(subtypes),
        produces=frozenset({tap.group("c")}),
        shock_land_life_cost=int(life.group("n")),
    )


def _build_mdfc_spell_printing(card: dict) -> CardPrinting | None:
    """Modal DFC whose front is an instant / sorcery and whose back is a
    land (Sink into Stupor // Soporific Springs): the front face is the
    castable spell, with ``mdfc_back_face`` set to the land face. The
    printing keeps the full card name (the pool / deck key)."""
    import dataclasses

    faces = card.get("card_faces") or []
    if len(faces) != 2:
        return None
    front = _build_spell_printing({
        "name": faces[0]["name"],
        "mana_cost": faces[0].get("mana_cost", ""),
        "type_line": faces[0].get("type_line", ""),
        "oracle_text": faces[0].get("oracle_text", ""),
    })
    if front is None:
        return None
    back = _build_mdfc_land_face(faces[1])
    if back is None:
        return None
    return dataclasses.replace(front, name=card["name"], mdfc_back_face=back)


def _build_spell_printing(card: dict) -> CardPrinting | None:
    """Try to produce a ``CardPrinting`` for an instant or sorcery
    ``card`` (a Scryfall record). Returns ``None`` if the card is
    neither an instant nor a sorcery, or if its oracle text doesn't
    parse into a supported single-effect or composite shape.
    """
    if card.get("layout") == "modal_dfc":
        return _build_mdfc_spell_printing(card)
    supertypes, card_types, subtypes = _parse_type_line(
        card.get("type_line", "")
    )
    if "Instant" not in card_types and "Sorcery" not in card_types:
        return None
    text = card.get("oracle_text", "").strip()
    if not text:
        return None
    # Vintage Roto P5: spell lines that are rules of the card itself
    # ("Cast this spell only before the combat damage step.").
    from mtgz_ir.cards.parser.roto_board import claim_spell_lines

    text, roto_fields = claim_spell_lines(text)
    effect = _parse_spell_oracle_text(text, card["name"])
    if effect is not None:
        effect = apply_overload(
            effect, text, card["name"], _parse_spell_oracle_text,
        )
    if effect is None:
        return None
    factory = make_instant if "Instant" in card_types else make_sorcery
    kicker = _detect_kicker_cost(text)
    kwargs: dict = {}
    if kicker is not None:
        kwargs["kicker"] = kicker
    shuffle_self_statics = _detect_shuffle_self_statics(text, card["name"])
    printing = factory(
        name=card["name"],
        cost=card["mana_cost"],
        effect=effect,
        cost_reductions=_detect_cost_reductions(text),
        static_abilities=shuffle_self_statics,
        **_detect_preamble_flags(text),
        **_detect_additional_cost_flags(text),
        **kwargs,
    )
    substrate = _detect_casting_substrate_fields(text)
    if shuffle_self_statics:
        # "If <Name> would be put into a graveyard from anywhere, ...
        # instead" (Nexus of Fate): the printing's from-anywhere watch
        # flag, which the zone-change code reads before applying the
        # replacement (as the creature grammar sets it for Blightsteel).
        substrate["watched_in_graveyard"] = True
    # Spells-A printing lines the substrate doesn't own: alternative
    # costs (incl. Overload), one-of additional costs, Replicate,
    # Teamwork, "This spell can't be countered." (``spell_cost_lines``).
    substrate.update(detect_spell_line_fields(
        text, legacy_line=_is_substrate_cost_line,
    ))
    if subtypes or supertypes:
        # "Instant — Arcane": the factories print a bare type line; keep
        # the printed subtypes / supertypes (splice keys off Arcane).
        substrate.update(
            type_line=card.get("type_line", printing.type_line),
            supertypes=tuple(supertypes),
            subtypes=tuple(subtypes),
        )
    if "Kindred" in card_types:
        # "Kindred Instant — Eldrazi" (CR 308): Kindred is a card type
        # too (graveyard card-type counts); Vintage Roto P5.
        substrate["card_types"] = tuple(card_types)
    if substrate:
        import dataclasses

        printing = dataclasses.replace(printing, **substrate)
    if roto_fields:
        from mtgz_ir.cards.parser.roto_board import apply_board_fields

        printing = apply_board_fields(printing, roto_fields)
    return printing


def _detect_casting_substrate_fields(text: str) -> dict:
    """``CardPrinting`` fields for the casting-substrate lines in
    ``text``: casting keywords (Flashback / Delve / Buyback) anywhere,
    plus leading additional-cost preambles modeled by the substrate
    (sacrifice-a-<permanent>, pay X life). Mirrors the clause dropping
    in ``_parse_spell_oracle_text``."""
    fields: dict = {}
    leading = True
    for raw in [c.strip() for c in text.split("\n") if c.strip()]:
        if leading and _is_additional_cost_preamble(raw):
            body = _ADDITIONAL_COST_PREAMBLE_RE.match(raw).group(1)
            if body not in _ADDITIONAL_COST_BODY_FLAGS:
                fields.update(_substrate_additional_cost_fields(body) or {})
            continue
        if leading and _is_spell_keyword_preamble(raw):
            continue
        clause = _strip_reminder_text(raw)
        kw = _casting_keyword_fields(clause) if clause else None
        if kw is not None:
            fields.update(kw)
            continue
        leading = False
    return fields
