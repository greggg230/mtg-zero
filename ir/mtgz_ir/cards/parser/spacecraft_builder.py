"""Artifact - Spacecraft ``CardPrinting`` builder.

Spacecraft are an Edge of Eternities-only card type printed as
``Artifact - Spacecraft``. They enter as non-creature artifacts and
become artifact creatures with a printed ``power``/``toughness`` and
a set of stationed keywords once they accumulate a printed number of
charge counters via the ``Station`` ability. The static registry
models them through ``make_spacecraft(name, cost, power, toughness,
station_threshold, stationed_keywords, ...)``.

The printed oracle text follows a uniform skeleton:

- One ``Station (Tap another creature you control: Put charge counters
  equal to its power on this Spacecraft. Station only as a sorcery.
  It's an artifact creature at <N>+.)`` line. The ``<N>`` here is
  always the same number that gates the highest tier-keyword line and
  also the engine's ``station_threshold``.
- Zero or more "tier" lines of the form ``"<N>+ | <body>"``. The
  highest tier is always the keyword grant that the engine encodes as
  ``stationed_keywords`` and ``station_threshold``. Lower tiers are
  static, triggered, or activated abilities gated on charge counters
  via ``StaticAbility.tier_threshold`` / ``TriggeredAbility.tier_threshold``.
- Zero or more "ETB" / standalone trigger lines (``"When this
  Spacecraft enters, ..."``, ``"Whenever this Spacecraft attacks,
  ..."``, etc.) and/or a printed activated ability.

**Striations (CR 722.2-722.3).** "Any abilities printed within the same
text box striation as a station symbol are part of its static
ability", and a striation runs from its station symbol to the next one
or to the bottom of the text box. Scryfall's oracle text writes a
striation's first ability on the ``"<N>+ | "`` line and each further
ability on a line of its own *after* it: The Seriema's ``"7+ | Flying"``
then ``"Other tapped legendary creatures you control have
indestructible."``. Such a line is in the N+ striation, so its ability
carries ``tier_threshold=N`` exactly as a body on the ``"<N>+ | "`` line
does. Only the lines printed before the first station symbol (the
enters triggers, The Eternity Elevator's ``{T}: Add {C}{C}{C}``, the
Station keyword line itself) are untiered (CR 722.4). Every EoE
Spacecraft prints its Station line directly above its first striation.
(Until mtg-zero docs/ir_defects.md 340 these continuation lines were
read as untiered: six cards had a striation's ability at all times.)

This builder grew in several slices:

- **Slice 1** claimed the simplest end-to-end shape: the Station
  reminder line plus a single ``"<N>+ | <evergreen-keyword-list>"``
  tier line whose threshold matches the Station threshold. The
  keyword list is comma-joined evergreens from ``_KEYWORD_LINES``.
- **Slice 2** extends the grammar to claim a single
  ``"When this Spacecraft enters, <effect>."`` ETB trigger alongside
  the Station + tier-keyword pair. The trigger line is parsed by the
  unified ``_parse_clause`` orchestrator — the type-qualified self-ETB
  predicate-trigger phrase ``this Spacecraft enters`` already lives in
  ``triggers._PREDICATE_TRIGGER_PHRASES`` (added in batch 11), and any
  ETB whose effect phrase resolves through ``effects._parse_effect_phrase``
  becomes a ``TriggeredAbility`` we hand straight to
  ``make_spacecraft(triggered_abilities=...)``. Spacecraft whose ETB
  effect is outside the effect grammar (e.g. ``return target creature
  or enchantment card from your graveyard``) still fall through.
- **Slice 3** extended the trigger arm two ways. First,
  ``Whenever this Spacecraft attacks, <effect>.`` lines are accepted
  alongside the self-ETB shape — the type-qualified self-attack
  predicate-trigger phrase ``this Spacecraft attacks`` lives in the
  same ``_PREDICATE_TRIGGER_PHRASES`` table the self-ETB phrase uses,
  so the only change is the per-line dispatch widening to admit the
  ``Event.ATTACKS, IsSelf()`` shape. Second, the defensive Legendary
  skip is dropped — ``_parse_type_line``'s ``Legendary`` supertype is
  forwarded to ``make_spacecraft(legendary=True)`` so a Legendary
  Spacecraft whose body falls inside the supported grammar can be
  claimed.
- **Slice 4** extends the ``"<N>+ | <body>"`` tier-line
  arm to claim **tier-gated triggered abilities**. A tier-line body
  that isn't a pure evergreen keyword list is re-tried through
  ``_parse_clause``; if it parses to a self-ETB or self-ATTACKS
  ``TriggeredAbility`` the trigger is added to the Spacecraft's
  trigger list with ``tier_threshold=N`` so the engine's
  ``TriggeredAbility.tier_threshold`` gate fires only when the
  Spacecraft has enough charge counters. Multi-tier static
  **keyword** grants — the Synthesizer Labship shape — were
  considered as a companion arm but rejected: the engine's
  ``StaticAbility.applies_to`` requires the target be a creature now,
  and every real EoE printing's lower-tier keyword line falls below
  the Station threshold (i.e. while the Spacecraft is still an
  artifact, not a creature), so a self-scoped ``StaticAbility`` with
  ``tier_threshold=N`` would never fire. Modeling those grants
  correctly needs an engine-side change to ``stationed_keywords``
  (tier-keyed keyword sets) and is left for a future slice.
- **Slice 5** (this batch) extends the tier-line arm a third way to
  claim **tier-gated anthem statics**. A tier-line body that re-parses
  through ``_parse_clause`` to a non-self-scoped ``StaticAbility``
  (``CREATURES_YOU_CONTROL`` or ``OTHER_CREATURES_YOU_CONTROL``) gets
  ``tier_threshold=N`` stamped on it and lands in
  ``make_spacecraft(static_abilities=...)``. Lumen-Class Frigate's
  ``2+ | Other creatures you control get +1/+1.`` is the EoE printing
  in this shape; the slice-4 rejection of tier-gated keyword statics
  doesn't apply because anthem scopes target *other* permanents and
  fire as soon as the Spacecraft has enough charge counters
  irrespective of whether the Spacecraft itself is animated.
- **Slice 6** migrated the three remaining bespoke entries whose effects
  were previously outside the unified grammar: Entropic Battlecruiser,
  Synthesizer Labship, and Dawnsire, Sunstar Dreadnought. Three
  effect-grammar additions unlocked them:

  1. ``"they lose N life"`` → ``TargetPlayerLosesLife(amount=N)`` (for
     DISCARDS_CARD observer effects where "they" is the pre-wired target).
  2. Multi-sentence literal for ``"each opponent discards a card. Each
     opponent who can't loses N life"`` → ``EachOpponentDiscardsOrLosesLife``.
  3. Parameterized regex for ``"up to one other target artifact you control
     becomes an artifact creature with base power and toughness P/T and
     gains <keywords> until end of turn"`` →
     ``TargetArtifactBecomesCreatureUEOT``.

  Additionally the trigger-condition phrasebook gains
  ``"an opponent discards a card"`` → ``(Event.DISCARDS_CARD,
  Controlled(OPPONENT))`` and the tier-gated trigger arm in this builder
  is widened to accept three new (event, predicate) shapes:
  ``DISCARDS_CARD + Controlled(OPPONENT)`` (Entropic Battlecruiser),
  ``BEGINNING_OF_COMBAT + Controlled(YOU)`` (Synthesizer Labship), and
  ``YOU_ATTACK + Controlled(YOU)`` (Dawnsire). The
  ``_DEAL_DAMAGE_UP_TO_ONE_TARGET_CREATURE`` regex is also extended
  to accept a leading ``(?:this creature )?`` prefix and a trailing
  ``(?: or planeswalker)?`` suffix so Dawnsire's
  ``this creature deals 100 damage to up to one target creature or
  planeswalker`` effect phrase claims correctly.

- **Slice 7** migrated The Eternity Elevator (the last remaining
  bespoke entry) into the unified grammar. Three grammar gaps were
  closed:

  1. **Missing P/T**: non-animating Spacecraft have no printed P/T in
     the Scryfall record; the builder now falls back to ``power=0,
     toughness=0`` instead of bailing, matching ``make_spacecraft``'s
     defaults for the non-creature case.
  2. **Non-animating Station line**: a second regex
     ``_STATION_LINE_NO_ANIMATION_RE`` matches the Station reminder text
     without the ``"It's an artifact creature at N+."`` sub-clause.
     Matching sets ``station_seen=True`` but leaves
     ``station_threshold=None`` so the engine never animates the card.
  3. **Tier-gated activated ability**: the ``"N+ | {cost}: <effect>"``
     tier-line arm is widened to accept an activated ability (in addition
     to the existing keyword / trigger / anthem-static arms). The
     activation is tagged with ``tier_threshold=N`` so
     ``effective_activated_abilities`` hides it below the threshold.
     Two new effect-grammar entries support this:
     - ``_ADD_N_COLORLESS_MANA`` matches ``add {C}{C}…`` and produces
       ``AddManaToPool(produces=(("C", N),))`` — covering the Elevator's
       always-on ``{T}: Add {C}{C}{C}`` ability as well.
     - ``_ADD_X_MANA_ANY_COLOR_PER_CHARGE_COUNTER`` matches the normalized
       ``add X mana of any one color, where X is the number of charge
       counters on this creature`` phrase and produces
       ``AddChargeCounterManaOfChosenColor``.
     ``_is_mana_ability_effect`` in ``activated.py`` is widened to also
     return True for ``AddChargeCounterManaOfChosenColor`` so the engine
     bypasses the stack on activation per CR 605.

  After this slice every Spacecraft printing was claimed by the
  unified grammar; the bespoke dispatch table was retired in
  retire-empty-bespoke-shells.

Anything outside the supported shape - lower-tier static/activated
abilities, multiple tier-keyword lines (single-keyword-tier only is
claimed today), an ``X+`` body that isn't a bare keyword list and
isn't a self-trigger, activated, or anthem-static, or a missing Station
line - falls through and the Spacecraft is silently skipped, mirroring
the creature/spell/land/equipment builders.

Spacecraft live in their own module rather than extending the artifact/
equipment builders because the printed grammar (Station reminder text
+ tiered ``N+ |`` body lines) is structurally distinct from
``Equipped creature ...`` / ``Equip {N}`` and from plain artifact
oracle text. Folding those shapes together would require either
disjoint per-line regexes or per-builder branching that obscures the
narrow grammar each one actually claims.
"""

from __future__ import annotations

import dataclasses
import re

from mtgz_ir.cards.base import (
    ActivatedAbility,
    CardPrinting,
    StaticAbility,
    StaticScope,
    TriggeredAbility,
    make_spacecraft,
)
from mtgz_ir.cards.parser.clause import _parse_clause
from mtgz_ir.cards.parser.keywords import _KEYWORD_LINES
from mtgz_ir.cards.parser.type_line import _parse_type_line
from mtgz_ir.effects.predicates import OPPONENT, Controlled, Event, IsSelf, YOU, Predicate


# The Station reminder line. Scryfall prints it as a single line:
#   "Station (Tap another creature you control: Put charge counters
#    equal to its power on this Spacecraft. Station only as a sorcery.
#    It's an artifact creature at <N>+.)"
# We capture the threshold integer inside the parenthetical so we can
# cross-check it against the highest tier line.
_STATION_LINE_RE = re.compile(
    r"^Station \(Tap another creature you control: "
    r"Put charge counters equal to its power on this Spacecraft\. "
    r"Station only as a sorcery\. "
    r"It's an artifact creature at (\d+)\+\.\)$"
)

# Non-animating Station reminder line — same as above but without the
# "It's an artifact creature at N+." sub-clause. Scryfall prints this
# form on Spacecraft that never become creatures (e.g. The Eternity
# Elevator whose station_threshold is None — it accumulates charge
# counters via Station but never animates). The absence of the
# animation clause is the distinguishing marker; we record
# station_threshold=None for these Spacecraft.
_STATION_LINE_NO_ANIMATION_RE = re.compile(
    r"^Station \(Tap another creature you control: "
    r"Put charge counters equal to its power on this Spacecraft\. "
    r"Station only as a sorcery\.\)$"
)


# A tier line: ``"<N>+ | <body>"``. The body for the highest-tier
# keyword line is a comma-joined evergreen keyword list (e.g.
# ``"Flying"``, ``"Flying, haste"``). Lower-tier ability bodies use
# the same prefix; this batch (slice 4) re-tries those bodies through
# the unified clause parser to claim self-ETB / self-ATTACKS triggers
# tagged with ``tier_threshold=N``.
_TIER_LINE_RE = re.compile(r"^(\d+)\+ \| (.+)$")


def _parse_tier_keyword_body(body: str) -> frozenset[str] | None:
    """Parse a tier line body that's a comma-joined evergreen keyword
    list (``"Flying"``, ``"Flying, haste"``) into the frozen set of
    engine spellings. Returns ``None`` if any piece is not a recognized
    evergreen - the enclosing tier line then re-tries through the
    triggered-ability arm and, failing that, the Spacecraft is
    silently skipped.

    Mirrors the equipment-builder rider parser: tier-keyword bodies
    use sentence-case for everything past the first piece (``"Flying,
    haste"``), so we normalize each piece to title-case for the
    ``_KEYWORD_LINES`` lookup.
    """
    pieces = [p.strip() for p in body.split(",")]
    normalized = [p[:1].upper() + p[1:] for p in pieces]
    if not all(p in _KEYWORD_LINES for p in normalized):
        return None
    return frozenset(_KEYWORD_LINES[p] for p in normalized)


# Valid (event, predicate) pairs for tier-gated trigger lines in the
# Spacecraft body grammar. Self-ETB and self-attack share the ``IsSelf()``
# predicate. Three player-scoped or opponent-observer variants:
#
# - ``DISCARDS_CARD + Controlled(OPPONENT)``: ``"Whenever an opponent
#   discards a card"`` printed on Entropic Battlecruiser's 1+ tier line
#   (fires once per card discarded by any opponent of the source's
#   controller; the discarding player's id is pre-wired as a fixed target
#   by ``fire_opponent_discards_triggers``).
# - ``YOU_ATTACK + Controlled(YOU)``: ``"Whenever you attack"`` printed on
#   Dawnsire, Sunstar Dreadnought's 10+ tier line (fires once per declare-
#   attackers step when you attack with any creature).
# - ``BEGINNING_OF_COMBAT + Controlled(YOU)``: ``"At the beginning of
#   combat on your turn"`` printed on Synthesizer Labship's 2+ tier line
#   (fires at the beginning of the active player's combat step).
_VALID_SPACECRAFT_TIER_TRIGGER_EVENTS: tuple[
    tuple[Event, Predicate], ...
] = (
    (Event.ENTERS, IsSelf()),
    (Event.ATTACKS, IsSelf()),
    (Event.DISCARDS_CARD, Controlled(by=OPPONENT)),
    (Event.YOU_ATTACK, Controlled(YOU)),
    (Event.BEGINNING_OF_COMBAT, Controlled(YOU)),
)


def _is_valid_spacecraft_tier_trigger(trig: "TriggeredAbility") -> bool:
    """Return True when ``trig`` is a trigger type this builder claims
    on tier-gated lines: self-ETB, self-attack, opponent-discards-observer,
    player-attack, or beginning-of-combat-on-your-turn.

    The non-``IsSelf()`` shapes are added in this batch to support
    Entropic Battlecruiser's ``1+ | Whenever an opponent discards a card, …``
    (DISCARDS_CARD + Controlled(OPPONENT)), Dawnsire's
    ``10+ | Whenever you attack, …`` (YOU_ATTACK + Controlled(YOU)), and
    Synthesizer Labship's ``2+ | At the beginning of combat on your turn, …``
    (BEGINNING_OF_COMBAT + Controlled(YOU)) tier lines respectively.
    """
    return any(
        trig.event == event and trig.predicate == predicate
        for event, predicate in _VALID_SPACECRAFT_TIER_TRIGGER_EVENTS
    )


def _build_spacecraft_printing(card: dict) -> CardPrinting | None:
    """Try to produce a ``CardPrinting`` for a Spacecraft ``card`` (a
    Scryfall record). Returns ``None`` if the card is not a Spacecraft
    or if its oracle text contains anything outside the narrow shape
    this builder supports:

    - a Station reminder line, plus
    - exactly one ``"<N>+ | <evergreen-keyword-list>"`` tier line
      whose threshold matches the Station threshold (this becomes
      ``stationed_keywords`` and ``station_threshold``).
    - zero or more ``"<M>+ | <self-trigger body>"`` tier lines whose
      body parses through ``_parse_clause`` to a self-ETB or self-
      ATTACKS ``TriggeredAbility``. The threshold ``M`` lands on
      ``TriggeredAbility.tier_threshold`` so the engine fires the
      trigger only once the Spacecraft has enough charge counters.
    - at most one ``"When this Spacecraft enters, <effect>."`` line
      whose effect resolves through ``_parse_clause`` to a
      ``TriggeredAbility(event=ENTERS, predicate=IsSelf())``, and
    - at most one ``"Whenever this Spacecraft attacks, <effect>."``
      line whose effect resolves through ``_parse_clause`` to a
      ``TriggeredAbility(event=ATTACKS, predicate=IsSelf())``, and
    - any number of ``"{cost}: <effect>."`` activated-ability lines
      whose cost / effect resolve through ``_parse_clause`` to an
      ``ActivatedAbility``. Debris Field Crusher's
      ``{1}{R}: This Spacecraft gets +2/+0 until end of turn.`` is the
      EoE printing in this shape; the ``this Spacecraft`` self-source
      pronoun is admitted by the unified ``_PUMP_SELF_UEOT`` regex
      alongside the canonical ``this creature`` form.

    Legendary Spacecraft (The Seriema, Dawnsire, Infinite Guideline
    Station, The Eternity Elevator) are not skipped — the ``Legendary``
    supertype, when present in the type line, is forwarded to
    ``make_spacecraft(legendary=True)``. Today no EoE Legendary
    Spacecraft body falls inside this grammar (each carries an effect
    outside the effect grammar, missing P/T, or a tier-gated trigger
    whose effect isn't claimed), so accepting the ``Legendary``
    supertype is forward-looking grammar coverage; per-card parity
    stays exact because the static registry builds those cards with
    ``legendary=True`` directly.
    """
    supertypes, card_types, subtypes = _parse_type_line(
        card.get("type_line", "")
    )
    if "Artifact" not in card_types:
        return None
    if "Spacecraft" not in subtypes:
        return None

    name = card.get("name", "")
    legendary = "Legendary" in supertypes
    pt_missing = False
    try:
        power = int(card["power"])
        toughness = int(card["toughness"])
    except (KeyError, ValueError):
        # Non-animating Spacecraft (e.g. The Eternity Elevator) have no
        # power/toughness box (CR 722.1): they never become creatures, and
        # have no power or toughness in any zone, so both stay unset.
        # Animating Spacecraft (all others in the EoE pool) always have P/T
        # fields; a missing field on one of those is a data error and the card
        # must be rejected after we confirm the Station line is non-animating.
        power = None
        toughness = None
        pt_missing = True

    text = card.get("oracle_text", "")
    # Spacecraft oracle text is line-structured but we don't strip
    # reminder text here: the Station line *is* parenthesized reminder
    # text in its entirety, and stripping it would make the line
    # disappear. We match it literally instead.
    lines = [line for line in text.split("\n") if line.strip()]

    station_threshold: int | None = None
    station_seen: bool = False  # sentinel: True once any Station line is consumed
    tier_threshold: int | None = None
    stationed_keywords: frozenset[str] | None = None
    triggers: list[TriggeredAbility] = []
    activateds: list[ActivatedAbility] = []
    statics: list[StaticAbility] = []
    # The striation the current line is printed in (CR 722.2): the
    # threshold of the last ``"<N>+ | "`` line seen, ``None`` above the
    # first station symbol. A striation runs to the next station symbol or
    # to the bottom of the text box, so a line without its own ``"<N>+ |"``
    # prefix after a tier line is one of that striation's abilities.
    striation: int | None = None

    for line in lines:
        m = _STATION_LINE_RE.match(line)
        if m is not None:
            if station_seen:
                # Two Station lines on one card would be malformed -
                # bail rather than silently keep the second.
                return None
            station_seen = True
            station_threshold = int(m.group(1))
            continue
        if _STATION_LINE_NO_ANIMATION_RE.match(line) is not None:
            # Non-animating Station line (no "It's an artifact creature
            # at N+." sub-clause). station_threshold stays None to signal
            # that this Spacecraft never animates; station_seen is flipped
            # True so the final "was any Station line present?" check
            # doesn't reject the card.
            if station_seen:
                return None
            station_seen = True
            # station_threshold remains None — no animation
            continue
        m = _TIER_LINE_RE.match(line)
        if m is not None:
            tier_n = int(m.group(1))
            body = m.group(2)
            striation = tier_n
            kw = _parse_tier_keyword_body(body)
            if kw is not None:
                if tier_threshold is not None:
                    # A second keyword tier line means a multi-tier
                    # keyword Spacecraft (e.g. a synthetic
                    # ``"5+ | Flying" + "10+ | Flying, lifelink"``).
                    # Out of scope: the engine's ``stationed_keywords``
                    # is a single-tier set, and modeling lower-tier
                    # keyword grants via ``StaticAbility`` doesn't
                    # work because ``StaticAbility.applies_to``
                    # requires the target be a creature now (so a
                    # sub-station-threshold keyword grant never fires;
                    # an at-or-above-threshold one would need a
                    # different engine model). Skip rather than
                    # half-claim.
                    return None
                tier_threshold = tier_n
                stationed_keywords = kw
                continue
            # Body wasn't a pure evergreen keyword list — try the
            # unified clause parser for the tier-gated trigger or
            # tier-gated anthem-static arm. Forward the printing's own
            # name so a tier-line trigger that references the source by
            # its printed name (named-self short form, e.g.
            # ``"Whenever Infinite Guideline Station attacks"``) is
            # substituted to the canonical ``this Spacecraft`` form
            # before the trigger phrase lookup.
            clause = _parse_clause(body, name=name)
            if clause is None:
                # Activated / unknown shape — out of scope.
                return None
            if clause.statics:
                # Tier-gated anthem static. Lumen-Class Frigate's
                # ``2+ | Other creatures you control get +1/+1.`` is
                # the EoE printing in this shape: an anthem-scope
                # static ability whose targets are creatures *other
                # than* the Spacecraft, gated on the Spacecraft having
                # at least ``N`` charge counters. The reasoning the
                # tier-keyword arm above rejects multi-tier-keyword
                # bodies (engine ``StaticAbility.applies_to`` requires
                # the Spacecraft itself be a creature for SELF-scoped
                # grants) does NOT apply here: anthem scopes target
                # other permanents on the battlefield, so the gate
                # fires as soon as the Spacecraft accumulates enough
                # charge counters regardless of whether it's animated.
                # Restrict to the anthem scopes the static parser
                # produces today (CREATURES_YOU_CONTROL /
                # OTHER_CREATURES_YOU_CONTROL); SELF-scoped statics
                # remain rejected because their faithfulness story is
                # the unresolved one (would need engine work for
                # tier-gated stationed-keyword grants, deferred to a
                # future slice). A multi-keyword anthem fans out into
                # multiple StaticAbility instances; the tier_threshold
                # is stamped on each so the engine gate fires uniformly
                # across the keyword grants.
                for static_ab in clause.statics:
                    if static_ab.scope not in (
                        StaticScope.OTHER_CREATURES_YOU_CONTROL,
                        StaticScope.CREATURES_YOU_CONTROL,
                    ):
                        return None
                    statics.append(
                        dataclasses.replace(static_ab, tier_threshold=tier_n)
                    )
                continue
            if clause.activated is not None:
                # Tier-gated activated ability. The Eternity Elevator's
                # ``20+ | {T}: Add X mana of any one color, …`` is the EoE
                # printing in this shape. Tag the ability with the tier
                # threshold so ``effective_activated_abilities`` hides it
                # when the Spacecraft has fewer than N charge counters.
                activateds.append(
                    dataclasses.replace(clause.activated, tier_threshold=tier_n)
                )
                continue
            if clause.trigger is None:
                # The clause parsed but as a non-trigger, non-activated, and
                # non-anthem (granted-keyword line, self-scope static). Self-
                # scope statics on Spacecraft aren't modeled by this builder
                # yet — skip rather than silently drop the ability.
                return None
            trig = clause.trigger
            if not _is_valid_spacecraft_tier_trigger(trig):
                # Defensive: only the three trigger shapes we claim in
                # tier-gated lines are accepted:
                # - ENTERS + IsSelf() (self-ETB tier line)
                # - ATTACKS + IsSelf() (self-attack tier line)
                # - YOU_ATTACK + Controlled(YOU) (player-attack tier line;
                #   Dawnsire's "Whenever you attack" which is oracle-text
                #   faithful to the YOU_ATTACK event rather than the
                #   ATTACKS+IsSelf shape the bespoke table used)
                return None
            if any(
                t.event == trig.event and t.tier_threshold == tier_n
                for t in triggers
            ):
                # A second tier-gated trigger at the same threshold on
                # the same event is out of scope; claiming only the
                # first would silently drop the second.
                return None
            triggers.append(
                dataclasses.replace(trig, tier_threshold=tier_n)
            )
            continue
        # Try the unified clause parser. This builder accepts at most
        # one self-ETB trigger and at most one self-ATTACKS trigger
        # per card from non-tier lines, plus any number of activated
        # abilities; static-only clauses are still out of scope.
        # Forward the printing's own name so trigger phrases that
        # reference the source by name (Infinite Guideline Station,
        # whose ETB and attack-trigger lines spell out the full name)
        # are substituted to the canonical ``this Spacecraft`` form
        # before the trigger phrase lookup. Same threading the creature
        # builder does for legendary creatures via ``Syr Vondam``.
        clause = _parse_clause(line, name=name)
        if clause is None:
            # Anything the unified parser doesn't recognize - attack
            # or ETB triggers whose effect phrases aren't modeled,
            # activated abilities whose cost or effect grammar we don't
            # support, etc. - falls through and we skip the whole card.
            return None
        # The line's striation (CR 722.2): 0 above the first station
        # symbol (treated normally, CR 722.4), else the threshold of the
        # ``"<N>+ | "`` line it follows.
        line_tier = striation or 0
        if clause.activated is not None:
            # Activated abilities surface as standalone lines on
            # Spacecraft (Debris Field Crusher's
            # ``{1}{R}: This Spacecraft gets +2/+0 until end of turn.``
            # is the EoE printing, in its 8+ striation below
            # ``"8+ | Flying"``). The unified ``_parse_activated_
            # ability_line`` produces an ``ActivatedAbility`` whose
            # cost / effect / mana-ability flag are already wired;
            # it carries the line's striation. Static and trigger arms
            # below stay mutually exclusive: a clause with a non-None
            # ``activated`` field has an empty ``triggers`` tuple by
            # ``_ParsedClause`` construction.
            activateds.append(
                dataclasses.replace(clause.activated, tier_threshold=line_tier)
            )
            continue
        if clause.statics:
            # An anthem static on a line of its own. The Seriema's
            # ``Other tapped legendary creatures you control have
            # indestructible.`` is the EoE printing in this shape: it is
            # printed below ``"7+ | Flying"``, in the 7+ striation, so it
            # carries ``tier_threshold=7`` (CR 722.2: the source has it
            # only with 7 or more charge counters). The tier-line static
            # arm above admits a superset of anthem scopes
            # (``CREATURES_YOU_CONTROL`` / ``OTHER_CREATURES_YOU_CONTROL``);
            # this path additionally admits the narrower
            # ``OTHER_TAPPED_LEGENDARY_CREATURES_YOU_CONTROL`` scope
            # because the engine's ``applies_to`` already filters the
            # target permanent without referencing the source's
            # creature status. SELF-
            # scoped statics remain rejected on the same grounds as
            # the tier-line arm: the engine would require the source
            # be a creature, which a Spacecraft only is at-or-above
            # its station threshold — modeling that correctly needs
            # an engine-side change for tier-gated stationed-keyword
            # grants and is out of scope. A multi-keyword anthem fans
            # out into multiple StaticAbility instances — extend
            # rather than append.
            for static_ab in clause.statics:
                if static_ab.scope == StaticScope.SELF:
                    return None
                statics.append(
                    dataclasses.replace(static_ab, tier_threshold=line_tier)
                )
            continue
        if clause.trigger is None:
            # The clause parsed but as a non-trigger / non-activated /
            # non-static (e.g. a granted-keyword self-keyword line).
            # This builder doesn't model those on Spacecraft yet -
            # skip rather than silently drop the ability.
            return None
        trig = clause.trigger
        if trig.predicate != IsSelf():
            # Defensive: only self-targeted predicates make sense for
            # the Spacecraft body shapes we claim. A trigger whose
            # predicate isn't ``IsSelf()`` is some other observer
            # event whose effect happened to parse — skip rather
            # than mis-wire it.
            return None
        if trig.event not in (Event.ENTERS, Event.ATTACKS):
            # Self-DIES / self-LEAVES / self-BECOMES_TAPPED etc. are
            # not body-trigger shapes any current Spacecraft prints,
            # and accepting them blindly would risk mis-wiring a
            # future printing whose effect happened to parse. Restrict
            # to the two events we explicitly claim.
            return None
        if any(
            t.event == trig.event and t.tier_threshold == line_tier
            for t in triggers
        ):
            # A second trigger line on the same event in the same
            # striation (two ETB lines, two attack-trigger lines) is out
            # of scope — claiming only the first would silently drop the
            # second. Triggers in distinct striations on the same event
            # remain allowed.
            return None
        # The attack triggers printed below a ``"<N>+ | <keywords>"`` line
        # (Entropic Battlecruiser 8+, Infinite Guideline Station 12+,
        # Sledge-Class Seedship 7+, Specimen Freighter 9+) are in that
        # striation: they trigger only with N or more charge counters.
        triggers.append(dataclasses.replace(trig, tier_threshold=line_tier))

    # A Station line is required: a Spacecraft with no Station line is
    # malformed. Non-animating Spacecraft (station_threshold=None, e.g.
    # The Eternity Elevator) have seen a Station line but have no
    # keyword-tier line — that's fine; they carry only activated abilities
    # and never animate. Animating Spacecraft (station_threshold is not
    # None) require a keyword-tier line to wire up stationed_keywords.
    if not station_seen:
        return None
    if station_threshold is None:
        # A station card without a power/toughness box (The Eternity
        # Elevator): it has station all the same (CR 702.184a). Its
        # ``station_threshold`` is its highest striation (no P/T box
        # there, so it never becomes a creature: ``game.is_stationed``).
        tiers = [
            a.tier_threshold
            for a in (*triggers, *statics, *activateds)
            if a.tier_threshold
        ]
        if not tiers or not pt_missing:
            return None
        station_threshold = max(tiers)
    elif station_threshold is not None:
        # Animating Spacecraft: keyword-tier line is mandatory, and P/T
        # must be present (missing P/T on an animating card is a data
        # error — the stationed creature would have no meaningful stats).
        if tier_threshold is None or stationed_keywords is None:
            return None
        if pt_missing:
            return None
        # The Station reminder text encodes the same threshold as the
        # highest (and here only) keyword tier line on every printing in
        # the current EoE pool. Treat any drift as a signal that we're
        # looking at a shape outside this grammar and bail.
        if station_threshold != tier_threshold:
            return None

    # Order the triggers deterministically: non-tier-gated triggers
    # (``tier_threshold=0``) first in printed order, followed by tier-
    # gated triggers sorted by tier threshold ascending. This keeps the
    # parsed printing byte-equal to the static-registry analogue
    # regardless of where the printed text places the tier-line vs the
    # standalone-trigger lines.
    non_tier_triggers = [t for t in triggers if t.tier_threshold == 0]
    tier_triggers = sorted(
        (t for t in triggers if t.tier_threshold != 0),
        key=lambda t: (t.tier_threshold, t.event.value if hasattr(t.event, "value") else 0),
    )
    ordered_triggers = tuple(non_tier_triggers) + tuple(tier_triggers)

    return make_spacecraft(
        name=card["name"],
        cost=card["mana_cost"],
        power=power,
        toughness=toughness,
        station_threshold=station_threshold,
        stationed_keywords=stationed_keywords if stationed_keywords is not None else frozenset(),
        triggered_abilities=ordered_triggers,
        static_abilities=tuple(statics),
        activated_abilities=tuple(activateds),
        legendary=legendary,
    )
