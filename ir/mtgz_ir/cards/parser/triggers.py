"""Triggered-ability grammar.

A triggered-ability line of the form ``When/Whenever <condition>,
<effect>.`` decomposes into a condition phrase and an effect phrase.
Each is handled by its own parser — the condition by
``_parse_trigger_phrase`` (a literal-phrase lookup against
``_TRIGGER_PHRASES`` and ``_PREDICATE_TRIGGER_PHRASES``), the effect
by ``_parse_effect_phrase`` from ``effects``. Adding a new condition
or effect grows its respective grammar without touching the line-
level pattern.

Two trigger shapes coexist during the predicate-trigger migration:

- **String-key style** (legacy): the condition phrase maps to a flat
  string key (``"dies"``, ``"attacks"``, …) and the engine's per-event
  ``_fire_*`` functions hardcode-match it. Phases later than this one
  will migrate the remaining string-key keys to the predicate path.
- **Predicate style** (new): the condition phrase maps to an
  ``(Event, Predicate)`` pair and the engine's centralized
  ``dispatch_event`` walks every triggered ability looking for a
  matching ``event`` whose ``predicate`` evaluates true. Phase 2
  migrated the ENTERS-event family (``etb`` / ``landfall`` /
  ``artifact_enters_you_control`` and the qualified variants); phase 4
  migrated the DIES / LEAVES_BATTLEFIELD families (this-creature-dies,
  this-creature-leaves-the-battlefield, counter-creature-dies, and
  the broader your-creature-or-artifact-dies / another-creature-dies-
  or-exiled observer patterns); phase 5 migrated the ATTACKS family
  (this-creature-attacks); phase 6 migrated the turn-structure family
  (``beginning_of_upkeep`` / ``beginning_of_combat`` /
  ``beginning_of_end_step`` and the ``each``-end-step variant) — these
  trigger keys aren't produced by oracle-text parsing (every
  turn-structure trigger is wired directly in the static-card
  registries) so this module's parser tables stay unchanged for that
  phase, but the engine-side predicate dispatch now subsumes the
  string-key path. Phase 7 migrated the BECOMES_TAPPED family
  (this-creature-becomes-tapped — parsed from oracle text — plus the
  aura ``enchanted_permanent_becomes_tapped`` variant which is only
  built statically) and the aura/equipment relational ATTACKS variants
  (``enchanted_creature_attacks``, ``equipped_creature_attacks``) plus
  the aura DEALT_DAMAGE family (``enchanted_creature_dealt_damage``)
  — all built directly in static-card registries via the new
  ``IsHost`` predicate, so the parser tables grow only by the
  ``this creature becomes tapped`` entry moving from
  ``_TRIGGER_PHRASES`` to ``_PREDICATE_TRIGGER_PHRASES``. Batch 11
  added the type-qualified self-ETB / self-attack family
  (``this Spacecraft enters``, ``this artifact enters``,
  ``this enchantment enters``, ``this Aura enters``,
  ``this Spacecraft attacks``) — every entry maps to the same
  ``IsSelf()`` predicate as the pre-existing ``"this creature
  enters"`` / ``"this creature attacks"`` siblings because the
  predicate's object-identity check is type-blind. The phrases are
  forward-looking grammar coverage today: every EoE printing that
  carries them is a non-creature card type that doesn't flow through
  the creature-builder ``_parse_clause`` pipeline.

Some triggered-ability lines are prefixed by an *ability-word* label
followed by an em-dash (e.g. ``Landfall — Whenever a land you
control enters, …``). Ability words are reminder labels with no
rules consequence — the substance is the ``Whenever …`` clause that
follows. ``_strip_ability_word_prefix`` strips any such label
generically so the body parses through the normal grammar; we never
need to enumerate ability words individually.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.base import (
    ConditionalIfCastFromHand,
    ConditionalIfSourcePowerAtLeast,
    ConditionalIfYouGainedLifeAtLeast,
    ConditionalOnTappedCreatures,
    DrawCards,
    DrawCardsEqualToResolvingCombatDamage,
    DrawCardsEqualToResolvingCounterAmount,
    Effect,
    IfGainedLifeThisTurnUntapSelf,
    PumpSelfUntilEOT,
    PumpTargetCreatureUntilEOT,
    PutPlusOneCounterOnSelf,
    PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.effects import (
    _SMALL_CARDINAL_PATTERN,
    _parse_amount,
    _parse_effect_phrase,
)
from mtgz_ir.effects.generic.eoe_fidelity import CountersPutBy, LeftBattlefieldTo
from mtgz_ir.effects.vocab import ZoneName
from mtgz_ir.effects.object_predicates import (
    HasCardType,
    HasCounter,
    HasKeyword,
    HasPowerAtLeast,
    HasPowerAtMost,
    HasPowerToughness,
    HasSubtype,
    IsArtifact,
    IsCreature,
    IsEnchantment,
    IsLand,
    IsNontoken,
    IsPlayer,
    IsTapped,
)
from mtgz_ir.effects.state_predicates import (
    AnyPlayerControlsNoCreatures,
    CardsInGraveyardAtLeast,
    CompletedADungeon,
    Delirium,
    GainedLifeThisTurn,
    HasCitysBlessing,
    HasInitiative,
    IsMonarch,
    LoneAttackerForYou,
    OpponentControlsMoreLandsThanYou,
    NoncreatureSpellsCastThisTurn,
    SpellsCastThisTurn,
    VoidActive,
)
from mtgz_ir.effects.state_predicates import LandsPlayedThisTurnAtLeast
from mtgz_ir.effects.generic.cube_creatures_rmc import OneOrMoreBatch
from mtgz_ir.effects.predicates import (
    OPPONENT,
    YOU,
    Always,
    Controlled,
    Event,
    FirstAttackThisTurn,
    HostAttacksWithAnother,
    IsFormerHost,
    IsHost,
    IsSelf,
    Predicate,
    Source,
    StateBound,
    Subject,
)


_TRIGGERED_ABILITY_LINE = re.compile(
    r"^(?:When|Whenever) (.+?), (.+)\.\"?$"
)
# The optional trailing ``\"`` allows oracle lines whose effect phrase
# ends with a quoted ability description — e.g. Station Monitor's
# "…create a Drone token with flying and "This token can block only
# creatures with flying."" where the outer closing double-quote follows
# the period inside the quoted sub-text.  Standard trigger lines ending
# with bare ``.`` are unchanged (the ``\"?`` matches zero quotes).


# ETB-replacement-style "enters with counters" line.
#
# Scryfall prints these as a standalone sentence — ``This creature
# enters with a +1/+1 counter on it.`` (Dockworker Drone) or with a
# numeric variant like ``This creature enters with two +1/+1 counters
# on it.`` Mechanically these are static replacement effects (CR 614),
# but the engine doesn't model replacements today; the static-card
# convention is to express them as a ``TriggeredAbility(event=ENTERS,
# predicate=IsSelf(), effect=PutPlusOneCounterOnSelf(amount=N))`` pair
# — see Dockworker Drone in ``cards/eoe_creatures.py`` for the
# canonical static layout. The parser mirrors that convention here:
# the line shape is recognized as a triggered-ability source even
# though its surface text doesn't carry a ``When``/``Whenever``
# keyword, and the produced ``TriggeredAbility`` matches the static
# entry shape verbatim.
#
# The body capture group covers everything between ``enters with`` and
# the trailing period. ``_parse_enters_with_body`` translates the body
# to an ``Effect`` — currently the bare ``a +1/+1 counter on it`` form,
# the spelled-out / digit ``N +1/+1 counters on it`` form (forward-
# looking — no EoE printing uses N>1 today, but the grammar shape is
# uniform and trivially extends), and the per-other-creature-or-
# artifact scaling form (Luxknight Breacher).
_WHEN_AND_WHENEVER_LINE = re.compile(
    r"^When (.+?) and whenever (.+?), (.+)\.$"
)
_ENTERS_WITH_REPLACEMENT_LINE = re.compile(
    r"^This creature enters with (.+)\.$"
)


# "Enters with N +1/+1 counters on it" — the bare counter-creation
# body. The amount tail accepts ``a`` (= 1), spelled-out small
# cardinals (``two`` etc.), and bare digits; ``a`` requires the
# singular ``counter`` tail, the others require the plural
# ``counters`` tail. Maps to ``PutPlusOneCounterOnSelf(amount=N)``.
_ENTERS_WITH_PLUS_ONE_COUNTERS_BODY = re.compile(
    rf"^(?:a \+1/\+1 counter|"
    rf"(\d+|{_SMALL_CARDINAL_PATTERN}) \+1/\+1 counters) on it$"
)


# "Enters with a +1/+1 counter on it for each other creature and/or
# artifact you control" — Luxknight Breacher's printed body. Maps to
# ``PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact()``, the same
# engine class the static registry uses. The phrase is matched as a
# literal (no captures): the per-other-creature-or-artifact scaling
# is the engine class's whole semantics, so there are no parameters
# to extract.
_ENTERS_WITH_PLUS_ONE_PER_OTHER_CREATURE_OR_ARTIFACT_BODY = re.compile(
    r"^a \+1/\+1 counter on it for each other "
    r"creature and/or artifact you control$"
)


def _parse_enters_with_body(body: str) -> Effect | None:
    """Translate the body of an ``enters with`` line to the engine
    ``Effect`` the static registry would carry on the same printing.

    Returns ``None`` if the body shape isn't claimed — the caller
    propagates the rejection up to ``_parse_triggered_ability_line``,
    which surfaces it as a parse failure for the whole oracle line.

    Tried in most-specific-first order: the per-other-creature-or-
    artifact scaling shape carries a ``for each ...`` tail that the
    bare counter-creation body's regex doesn't tolerate, but ordering
    explicitly here keeps the precedence intent visible.
    """
    if _ENTERS_WITH_PLUS_ONE_PER_OTHER_CREATURE_OR_ARTIFACT_BODY.match(
        body
    ) is not None:
        return PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact()
    m = _ENTERS_WITH_PLUS_ONE_COUNTERS_BODY.match(body)
    if m is not None:
        # Group 1 is the captured count word/digit for the plural
        # ``N +1/+1 counters`` shape; the bare ``a +1/+1 counter``
        # alternative has no capture, so a missing group resolves
        # to amount=1.
        amount = _parse_amount(m.group(1)) if m.group(1) else 1
        return PutPlusOneCounterOnSelf(amount=amount)
    return None


# Intervening-if shape: ``When/Whenever <condition>, if <gate>, <effect>.``
# (CR 603.4). The gate sits BETWEEN the trigger condition and the
# effect, gated on game state at both trigger time and resolve time.
# We rely on the regex's non-greedy ``<condition>`` and ``<gate>`` to
# stop at the first ``, `` boundary so the leading clause stays
# minimal; the effect captures the rest of the sentence up to the
# trailing period. A line that doesn't carry an ``if`` middle clause
# falls through to ``_TRIGGERED_ABILITY_LINE`` unchanged.
_TRIGGERED_ABILITY_INTERVENING_IF_LINE = re.compile(
    r"^(?:When|Whenever) (.+?), if (.+?), (.+)\.$"
)


# Intervening-while shape: ``When/Whenever <condition> while <gate>,
# <effect>.`` Mechanically equivalent to the middle-clause
# intervening-if shape (CR 603.4) — the gate sits BETWEEN the trigger
# condition and the effect, gated on game state at trigger time. The
# surface uses ``while`` instead of ``, if`` and the gate isn't
# delimited by a leading ``, `` boundary; the gate captures everything
# between `` while `` and the trailing ``, ``. Hand That Feeds prints
# ``Delirium — Whenever this creature attacks while there are four or
# more card types among cards in your graveyard, it gets +2/+0 and
# gains menace until end of turn.`` — the ``while`` clause is the
# canonical Delirium gate phrase, same predicate as the
# ``_INTERVENING_IF_PHRASES`` middle-clause table reuses.
#
# Tried AFTER the standard ``, if <gate>, `` intervening-if regex so a
# line carrying both shapes (none today) claims via the more-explicit
# ``, if `` shape first; the ``while`` shape's ``<condition>`` capture
# is non-greedy and stops at the first `` while `` boundary, keeping
# the trigger condition's body intact.
_TRIGGERED_ABILITY_INTERVENING_WHILE_LINE = re.compile(
    r"^(?:When|Whenever) (.+?) while (.+?), (.+)\.$"
)


# Trailing-if shape: ``When/Whenever <condition>, <effect> if <gate>.``
# Mechanically equivalent to the middle-clause intervening-if shape
# above (CR 603.4) — the gate sits AFTER the effect rather than
# between the trigger condition and the effect, but the trigger-time
# game-state check is the same. The Delirium "tail-rider" surface
# (``... if you have four or more card types among cards in your
# graveyard.``) is the canonical example; future trailing gates can
# add entries to the gate phrasebook without touching this regex.
#
# The condition capture is non-greedy to stop at the first ``, ``
# boundary; the effect capture is non-greedy too because the gate
# clause is identified by its leading `` if `` and trailing period.
# We anchor the gate at `` if `` (with surrounding word boundaries
# in the spelling — leading space, trailing period) so an ``if``
# inside the effect itself (as part of e.g. a sub-clause future
# extension) doesn't accidentally trigger this branch.
#
# Tried AFTER the middle-clause intervening-if regex so any line
# carrying both ``, if <gate>,`` AND ``... if <gate>.`` claims via
# the more-specific middle shape first. No DSK printing today
# combines the two; this ordering is purely defensive.
_TRIGGERED_ABILITY_TRAILING_IF_LINE = re.compile(
    r"^(?:When|Whenever) (.+?), (.+?) if (.+)\.$"
)


# Turn-structure trigger line shape: ``At the beginning of <when>,
# <effect>.`` Distinct from the ``When``/``Whenever`` shape above
# because the trigger word is ``At`` and the condition body is a
# fixed-phrase time descriptor (``your end step``, ``each end step``,
# etc.) rather than a free-form event clause. The ``<when>`` capture
# is non-greedy to stop at the first ``, `` boundary so the time
# descriptor stays minimal; the effect captures the rest of the
# sentence up to the trailing period.
#
# Two intervening-clause variants live alongside this base shape:
#
# - **Tapped-creatures effect-wrapper** (Frontline War-Rager / Flight-
#   Deck Coordinator / Sami / Sunstar Chaplain / Vaultguard Trooper /
#   Dawnstrike Vanguard): ``At the beginning of <when>, if you control
#   two or more tapped creatures, <effect>.`` The middle ``if`` clause
#   doesn't AND a predicate onto the trigger event the way the
#   standard ``_TRIGGERED_ABILITY_INTERVENING_IF_LINE`` does — the
#   static registry wraps the inner effect in
#   ``ConditionalOnTappedCreatures(effect=inner, min_count=2)`` and
#   leaves the trigger predicate at ``Controlled(YOU)``. Mirroring that
#   here keeps parser/static parity exact.
# - **Trailing-if-gained-life clause** (Haliya, Guided by Light): ``At
#   the beginning of <when>, <effect> if you've gained N or more life
#   this turn.`` The ``if`` rides AFTER the effect (not before, and
#   not in the middle) and the static wraps the inner effect in
#   ``ConditionalIfYouGainedLifeAtLeast(threshold=N, inner=...)``.
#
# Both variants are tried before the bare shape so the more-specific
# clause structure wins. Each variant has its own regex below.
_AT_THE_BEGINNING_OF_LINE = re.compile(
    r"^At the beginning of (.+?), (.+)\.$"
)


# Void ability-word prefix specifically — when the trigger line starts
# with ``Void — At the beginning of <when>, …`` the prefix is a marker
# that the produced ``TriggeredAbility`` must carry
# ``requires_void=True`` (mirroring the static-registry layout — see
# Insatiable Skittermaw, Voidforged Titan, Kavaron Skywarden,
# Hylderblade in ``cards/eoe_creatures.py``). The body of every Void
# ``At the beginning of <when>`` line in the EoE pool also carries the
# matching ``if a nonland permanent left the battlefield this turn or
# a spell was warped this turn, `` middle clause; that clause is
# elided here because the engine's ``requires_void`` flag is the
# canonical gate (the gate phrase and the flag are 1:1 by
# construction — every Void rider in print today uses the same gate
# wording).
#
# The standard ``_strip_ability_word_prefix`` helper only strips an
# ability-word prefix in front of ``When``/``Whenever`` (its lookahead
# guards against accidental matches in type-line strings), so the
# ``Void — At …`` shape doesn't flow through it. Detecting the prefix
# explicitly here keeps the Void-flag handling local to the new line
# shape — extending the generic helper would also mean coupling its
# strip semantics to a new "this prefix sets a flag" side effect,
# which is harder to keep readable.
_VOID_PREFIX = "Void — "


# Void-rider middle clause — the "intervening-if" surface text on a
# Void ``At the beginning of <when>`` line. The body parser strips
# this prefix when it follows a ``Void — `` line marker; the
# ``requires_void=True`` flag on the produced ``TriggeredAbility``
# replaces the gate's predicate-side semantics.
_VOID_GATE_PREFIX = (
    "if a nonland permanent left the battlefield this turn or "
    "a spell was warped this turn, "
)


# Duskmourn "Delirium — " ability-word prefix on ``At the beginning of
# <when>, if there are four or more card types among cards in your
# graveyard, <effect>.`` lines (e.g. Osseous Sticktwister). The
# ``_strip_ability_word_prefix`` helper only strips prefixes in front of
# ``When``/``Whenever`` bodies; the ``Delirium — At the beginning of``
# shape doesn't flow through it, so we detect the prefix explicitly
# here. Mirrors the ``_VOID_PREFIX`` / ``_SURVIVAL_PREFIX`` precedent —
# same structural shape, different predicate semantics: Void sets a
# ``requires_void`` flag, Survival ANDs ``IsSelf() & IsTapped()`` onto
# the predicate, and Delirium ANDs ``StateBound(Delirium())`` onto it.
_DELIRIUM_PREFIX = "Delirium — "

# Delirium intervening-if gate clause on ``At the beginning of <when>``
# trigger lines. The body parser strips this prefix and AND-s
# ``StateBound(Delirium())`` onto the trigger's base predicate so the
# resulting ``TriggeredAbility`` fires only when the source's controller
# has four or more distinct card types in their graveyard at the
# beginning of the relevant step (CR 603.4 intervening-if check).
# Mirrors the ``_VOID_GATE_PREFIX`` / ``_SURVIVAL_GATE_PREFIX``
# precedent — same strip-and-compose pattern, different predicate.
_DELIRIUM_AT_BEGINNING_GATE_PREFIX = (
    "if there are four or more card types among cards in "
    "your graveyard, "
)


# Trailing "if you've gained N or more life this turn" suffix on the
# body of an ``At the beginning of <when>, <inner-effect> if
# you've gained N or more life this turn.`` line. When the body
# ends with this phrase (after the inner effect), the inner effect
# is wrapped in ``ConditionalIfYouGainedLifeAtLeast(threshold=N,
# inner=...)`` and the trigger predicate is left at whatever the
# time-condition phrasebook yielded (typically ``Controlled(YOU)``).
#
# This mirrors the static-registry shape for Haliya, Guided by Light
# exactly: the static wraps ``DrawCards(1)`` in
# ``ConditionalIfYouGainedLifeAtLeast(threshold=3, inner=...)``. The
# gate sits AFTER the effect text, so the standard middle-clause
# intervening-if regex can't pick it up — detecting it here as a
# suffix keeps the body parser local. The threshold N is captured;
# the inner effect is the prefix before ``" if you've gained "``.
#
# Currently only the ``draw a card`` inner-effect form ships in the
# EoE/DSK pool (Haliya, Guided by Light). The parser recursively
# calls ``_parse_effect_phrase`` on the inner piece so any effect
# the grammar supports can serve as the inner body; the grammar
# extension is free.
_GAINED_LIFE_TRAILING_IF_SUFFIX = re.compile(
    r"^(.+) if you've gained (\d+) or more life this turn$"
)


# Tapped-creatures effect-wrapper prefix on the body of an
# ``At the beginning of <when>, <body>.`` line. When the body starts
# with this exact phrase, the inner effect (the tail after the comma)
# is wrapped in ``ConditionalOnTappedCreatures(effect=inner,
# min_count=2)`` and the trigger predicate is left at whatever the
# time-condition phrasebook yielded (typically ``Controlled(YOU)``).
#
# This mirrors the static-registry shape exactly: see Frontline War-
# Rager / Flight-Deck Coordinator / Sami / Sunstar Chaplain /
# Vaultguard Trooper / Dawnstrike Vanguard in
# ``cards/eoe_creatures.py``. The static does NOT layer the gate as a
# predicate AND on the trigger event — it wraps the effect — so the
# parser can't use the standard intervening-if predicate path here.
#
# ``min_count`` is fixed at 2 because every EoE printing today uses
# the "two or more" threshold; the engine class accepts an arbitrary
# min_count, and a future printing with "three or more tapped
# creatures" would land its own prefix entry rather than parameterizing
# this one (parser/static parity is easier when the surface text and
# the engine constant match 1:1).
_TAPPED_CREATURES_GATE_PREFIX = "if you control two or more tapped creatures, "


# ``if you gained life this turn, untap this creature`` — Ragost, Deft
# Gastronaut's beginning-of-each-end-step body: "At the beginning of each
# end step, if you gained life this turn, untap Ragost." After name-
# substitution the body is "if you gained life this turn, untap this
# creature". Detected as a prefix; the entire body collapses to
# ``IfGainedLifeThisTurnUntapSelf()`` which encodes both the life-gain
# check and the untap action in one resolve step, matching the static-
# registry layout exactly. Only Ragost in the EoE/DSK pool today; a
# second card with this shape (different inner effect, different life-gain
# threshold) would require decomposing into a separate
# ``ConditionalIfGainedLifeThisTurn(threshold=N, inner=…)`` primitive —
# deferred until a second example lands.
_IF_GAINED_LIFE_GATE_BODY = "if you gained life this turn, untap this creature"
_IF_GAINED_LIFE_GATE_BODY_PERMANENT = (
    "if you gained life this turn, untap this permanent"
)


# DSK Survival ability-word prefix — when the trigger line starts with
# ``Survival — At the beginning of <when>, …`` the prefix is a marker
# that the produced ``TriggeredAbility`` is the Survival post-combat-
# main fire. The ability word itself carries no rules consequence
# (CR 207.2c — ability words are flavor-only labels), so we strip the
# prefix and let the body parse through the same path as any other
# ``At the beginning of …`` line. Detecting it explicitly here keeps
# the strip surface narrow: the generic ``_strip_ability_word_prefix``
# helper above only removes ability-word prefixes in front of
# ``When``/``Whenever`` bodies (its lookahead guards against accidental
# matches in type-line strings), so the ``Survival — At …`` shape
# doesn't flow through it. Mirrors the ``Void — `` precedent
# (``_VOID_PREFIX``) — same shape, no flag because Survival's gate is
# the standard ``IsTapped`` intervening-if predicate, not an engine-
# wide flag like ``requires_void``.
_SURVIVAL_PREFIX = "Survival — "


# Survival-rider middle clause — the "intervening-if" surface text on a
# Survival ``At the beginning of your second main phase`` line. The
# body parser strips this prefix and AND-s the
# ``IsSelf() & IsTapped()`` predicate onto the trigger's base
# predicate, so the resulting ``TriggeredAbility`` fires only when the
# source is tapped at the start of its controller's second main phase.
# Two surface variants exist in print: the canonical
# ``"if this creature is tapped, "`` (every non-named printing) and a
# named-self variant where the printing's own short name appears in
# place of ``"this creature"`` (Kona, Rescue Beastie / Rip, Spawn
# Hunter). The named variant is normalized into the canonical
# ``"this creature"`` form by ``_substitute_self_name`` before this
# lookup runs, so we only need to recognize the canonical phrase here.
_SURVIVAL_GATE_PREFIX = "if this creature is tapped, "


# Generic intervening-if prefix on the body of an
# ``At the beginning of <when>, if <gate>, <inner>.`` line. When the
# body starts with ``if `` and the comma-bounded gate phrase is
# claimed by ``_INTERVENING_IF_PHRASES``, the gate's predicate is
# AND-ed onto the trigger's base predicate and the inner body parses
# through the unified ``_parse_effect_phrase`` path. Mirrors the
# parallel ``When/Whenever <condition>, if <gate>, <effect>.``
# dispatch in ``_parse_triggered_ability_line`` exactly — same
# phrasebook, same predicate-AND composition. Tried AFTER the
# more-specific Void / Survival / Delirium / tapped-creatures
# branches so those keep their dedicated handling; a body that
# starts with ``if `` but whose gate phrase isn't in the phrasebook
# is rejected outright (we never silently drop a printed gate).
#
# The non-greedy comma split stops at the FIRST ``, `` boundary so
# the gate phrase stays minimal; the ``inner`` capture takes the
# rest of the body. A body like ``if a player controls no creatures,
# sacrifice this creature, then put a creature card …`` carves into
# gate ``"a player controls no creatures"`` and inner ``"sacrifice
# this creature, then put a creature card …"`` — the inner's own
# ``, then `` stays inside the inner capture so the compound parser
# downstream can split it as a ``Sequence``.
_AT_BEGINNING_INTERVENING_IF = re.compile(
    r"^if (.+?), (.+)$"
)


# Time-condition phrases for the ``At the beginning of`` line shape →
# tuple of (Event, Predicate). The condition is the verbatim oracle
# phrase between ``At the beginning of`` and the first ``, ``
# boundary — ``your end step``, ``each end step``, ``your upkeep``,
# etc. Each entry maps to the matching turn-structure event and the
# controller-gate predicate appropriate for the surface text:
# ``your X`` reads as "this is MY phase/step" via ``Controlled(YOU)``
# (the predicate matches a Player subject by ``player.id ==
# source.controller_id``), and ``each X`` reads as "any player's
# phase/step" via ``Always()`` (Ragost's untap-self trigger fires on
# every end step regardless of whose it is).
#
# Today only ``BEGINNING_OF_END_STEP`` mappings are populated — every
# unclaimed creature in the EoE pool with a turn-structure trigger
# uses the end-step variant. The ``BEGINNING_OF_UPKEEP`` and
# ``BEGINNING_OF_COMBAT`` events are reachable when a future printing
# carries them; the phrasebook structure scales to those without
# further regex work.
_AT_BEGINNING_OF_PHRASES: dict[str, tuple[Event, Predicate]] = {
    "your end step": (Event.BEGINNING_OF_END_STEP, Controlled(YOU)),
    "each end step": (Event.BEGINNING_OF_END_STEP, Always()),
    # ``At the beginning of your upkeep`` — Thrumming Hivepool's
    # two-Sliver-token minting trigger. Surface form pairs the
    # ``BEGINNING_OF_UPKEEP`` event with the ``Controlled(YOU)``
    # controller gate (fires only during the controller's own upkeep).
    # Mirrors the ``your end step`` / ``each X`` precedent: ``your X``
    # reads as ``Controlled(YOU)``; an ``each upkeep`` variant would map
    # to ``Always()``, but no card today carries that form so only the
    # controller-gated entry is wired.
    "your upkeep": (Event.BEGINNING_OF_UPKEEP, Controlled(YOU)),
    # White Plume Adventurer (cube W/G bucket): each opponent's upkeep.
    # Abhorrent Oculus: fires in every opponent's upkeep.
    "each opponent's upkeep": (
        Event.BEGINNING_OF_UPKEEP, Controlled(OPPONENT),
    ),
    # ``At the beginning of combat on your turn`` — Molecular Modifier's
    # pump-and-grant trigger and Ouroboroid's mass-counter trigger.
    # Surface form pairs the ``BEGINNING_OF_COMBAT`` event with the
    # ``Controlled(YOU)`` controller gate (the trigger fires only on
    # the controller's own combat). Mirrors the ``your end step``
    # / ``each X`` precedent: ``your X`` reads as
    # ``Controlled(YOU)``; an ``each X`` variant would map to
    # ``Always()``, but no EoE printing today carries
    # ``each combat`` so only the controller-gated form is wired.
    "combat on your turn": (
        Event.BEGINNING_OF_COMBAT, Controlled(YOU),
    ),
    # ``At the beginning of your second main phase`` — DSK Survival
    # ability-word trigger. Every printed Survival creature in
    # Duskmourn carries the same surface shape:
    # ``Survival — At the beginning of your second main phase, if this
    # creature is tapped, <effect>.`` The ``Controlled(YOU)`` arm gates
    # to the active player's own post-combat main; the ``if this
    # creature is tapped`` intervening-if clause is handled separately
    # by ``_parse_at_the_beginning_of_line`` (it AND-s an
    # ``IsSelf() & IsTapped()`` predicate onto the trigger). Mirrors
    # the ``your end step`` / ``combat on your turn`` precedent.
    "your second main phase": (
        Event.BEGINNING_OF_POSTCOMBAT_MAIN, Controlled(YOU),
    ),
    # ``At the beginning of your first main phase`` (Coalition Relic) --
    # the precombat main phase (CR 505.1a); dispatched by both turn
    # loops before main-phase priority opens.
    "your first main phase": (
        Event.BEGINNING_OF_PRECOMBAT_MAIN, Controlled(YOU),
    ),
    # ``At the beginning of your draw step`` — Mana Vault's tapped-tax
    # trigger. Dispatched by ``turn.run_draw_step`` after the draw.
    "your draw step": (Event.BEGINNING_OF_DRAW_STEP, Controlled(YOU)),
    # Cube enchantments: ``At the beginning of the end step`` (Underworld
    # Breach) is every end step, the same event as ``each end step``;
    # ``At the beginning of each player's upkeep`` (Oath of Druids)
    # fires on every upkeep regardless of whose turn it is.
    "the end step": (Event.BEGINNING_OF_END_STEP, Always()),
    "each player's upkeep": (Event.BEGINNING_OF_UPKEEP, Always()),
}


def _parse_at_beginning_of_phrase(
    phrase: str,
) -> tuple[Event, Predicate] | None:
    return _AT_BEGINNING_OF_PHRASES.get(phrase.strip())


# Trailing once-per-turn rider on a triggered-ability line. Scryfall
# prints this as a second sentence glued to the trigger body with a
# single space separator: ``<trigger sentence>. This ability triggers
# only once each turn.`` Mechan Assembler (and Nanoform Sentinel,
# whose effect is out of scope today) carry this rider. Stripping the
# suffix and setting ``once_per_turn=True`` on the resulting
# ``TriggeredAbility`` mirrors the static registry's
# ``TriggeredAbility(..., once_per_turn=True)`` shape exactly. The
# engine field already exists; this is purely a parser-grammar
# addition.
_ONCE_PER_TURN_RIDER_SUFFIX = " This ability triggers only once each turn."
# Alternative once-per-turn rider phrasing — Scryfall prints
# Terrasymbiosis with "Do this only once each turn." (where "this"
# refers to the trigger's effect) instead of the standard "This
# ability triggers only once each turn." surface. Mechanically
# equivalent: both lift to ``once_per_turn=True`` on the produced
# ``TriggeredAbility``. The strip below in ``_parse_triggered_
# ability_line`` tries both suffixes — whichever appears wins.
_DO_THIS_ONCE_PER_TURN_RIDER_SUFFIX = " Do this only once each turn."


# Trigger-condition phrases → engine-level trigger key (legacy
# string-key shape). Pre-phase-2 this dict held every recognized
# condition; phase 2 lifted the ENTERS-event family into
# ``_PREDICATE_TRIGGER_PHRASES`` below. Phases 4-9 migrated every
# remaining parsed trigger phrase off the string-key path — this
# table is now empty. The lookup helper still exists so future
# trigger phrases that haven't yet been promoted to the predicate
# shape have a place to land.
_TRIGGER_PHRASES: dict[str, str] = {}


# Trigger-condition phrases for the predicate-trigger path → tuple of
# (Event, Predicate). Phase 2 migrated the ENTERS-event family —
# every condition phrase here corresponds to an ETB-family trigger
# whose static cards now express the firing condition via a
# ``(event=Event.ENTERS, predicate=...)`` ``TriggeredAbility``.
#
# The ``"another"`` variants for artifact/etc. enters use the same
# event but layer ``~IsSelf()`` onto the predicate so the source
# permanent is excluded from the firing pool — the engine no longer
# needs special-case "exclude the entering object" logic at the
# fire site for these.
_PREDICATE_TRIGGER_PHRASES: dict[
    str,
    tuple[Event, Predicate] | tuple[Event, Predicate, bool],
] = {
    # ``Whenever this creature enters`` — the source's own ETB.
    "this creature enters": (Event.ENTERS, IsSelf()),
    # Land-enters ("landfall"). The ``Landfall —`` ability-word label,
    # when present in oracle text, is stripped by
    # ``_strip_ability_word_prefix`` before this lookup runs.
    "a land you control enters": (
        Event.ENTERS, Subject(None, IsLand()) & Controlled(YOU),
    ),
    # Artifact-enters-you-control. Both ``an artifact you control
    # enters`` (Mm'menon, Uthros Exile) and ``another artifact you
    # control enters`` (Mechan Assembler, Oreplate Pangolin) collapse
    # to the same predicate. The legacy
    # ``_fire_artifact_enters_triggers`` engine helper excluded the
    # entering object from the firing pool unconditionally, so the
    # ``a`` vs ``another`` wording was a surface distinction without
    # behavioral impact — preserving that here keeps the migration
    # behavior-identical to the legacy fire path.
    "an artifact you control enters": (
        Event.ENTERS, Subject(None, IsArtifact()) & Controlled(YOU) & ~IsSelf(),
    ),
    "another artifact you control enters": (
        Event.ENTERS, Subject(None, IsArtifact()) & Controlled(YOU) & ~IsSelf(),
    ),
    # ``Whenever an opponent plays a land`` — Burgeoning. The
    # ``Event.PLAYS_LAND`` dispatch carries ``("land", "controller")``
    # subjects (see `effects/predicates.py` ``_EVENT_SUBJECTS``); the
    # ``Controlled(OPPONENT, subject="controller")`` predicate gates the
    # firing pool to land-plays whose acting player is an opponent of
    # the source's controller.
    "an opponent plays a land": (
        Event.PLAYS_LAND, Controlled(by=OPPONENT, subject="controller"),
    ),
    # ``Whenever you play a land`` — Fastbond. The acting player is the
    # source's controller; the source itself is never a land here.
    "you play a land": (
        Event.PLAYS_LAND, Controlled(YOU, subject="controller"),
    ),
    # ``When you play another land`` — City of Traitors. Same
    # ``Event.PLAYS_LAND`` dispatch, gated on the acting player being
    # the source's controller and the played land NOT being the source
    # itself ("another": the source's own land drop dispatches too).
    "you play another land": (
        Event.PLAYS_LAND,
        Controlled(YOU, subject="controller") & ~IsSelf(subject="land"),
    ),
    # ``When this creature dies`` — the source's own death (Gravpack
    # Monoist's create-Robot trigger and the synthetic ``draw 2 cards``
    # death triggers used in parser tests). Maps to the centralized
    # DIES dispatch path; the dying object is included as a candidate
    # observer via ``extra_observers`` so ``IsSelf()`` matches even
    # though the object has been moved off the battlefield.
    "this creature dies": (Event.DIES, IsSelf()),
    # ``When this creature leaves the battlefield`` (Anticausal Vestige).
    # The compound ``enters or leaves`` form (Cryogen Relic) is a
    # different shape and intentionally stays out of this entry's
    # scope.
    "this creature leaves the battlefield": (
        Event.LEAVES_BATTLEFIELD, IsSelf(),
    ),
    # ``When this creature dies or is put into exile`` (Syr Vondam,
    # Sunstar Exemplar — used in the intervening-while shape "while its
    # power is 4 or greater"). Both death and exile are zone changes
    # that fire LEAVES_BATTLEFIELD; collapsing both to that event with
    # IsSelf() matches the static-registry convention (the engine fires
    # LEAVES_BATTLEFIELD on both graveyard and exile moves). Only one
    # LEAVES+IsSelf trigger is produced — the "or is put into exile"
    # arm doesn't generate a second independent trigger.
    # ``LeftBattlefieldTo`` narrows the leave to the two printed
    # destinations (a bounce or a library move doesn't trigger it;
    # mtg-zero docs/ir_defects.md 80).
    "this creature dies or is put into exile": (
        Event.LEAVES_BATTLEFIELD,
        IsSelf() & LeftBattlefieldTo(zones=(ZoneName.GRAVEYARD, ZoneName.EXILE)),
    ),
    # ``Whenever another creature you control dies or is put into exile``
    # (Syr Vondam, Sunstar Exemplar — the ally-leaves observer trigger).
    # Both death and exile fire LEAVES_BATTLEFIELD; collapsing the two
    # zone changes onto that event preserves parity with the
    # static-registry's single LEAVES_BATTLEFIELD+Subject trigger for
    # this card. The ``~IsSelf()`` excludes Syr Vondam itself (the
    # "another" qualifier); ``Subject(None, IsCreature())`` and
    # ``Controlled(YOU)`` gate the firing pool to creatures the source's
    # controller controls.
    "another creature you control dies or is put into exile": (
        Event.LEAVES_BATTLEFIELD,
        Subject(None, IsCreature()) & Controlled(YOU) & ~IsSelf()
        & LeftBattlefieldTo(zones=(ZoneName.GRAVEYARD, ZoneName.EXILE)),
    ),
    # ``When this artifact is put into a graveyard from the
    # battlefield`` (Nutrient Block, Chromatic Star, Wizard's Rockets).
    # The graveyard-from-battlefield zone change is ``Event.DIES``, the
    # event every "put into a graveyard from the battlefield" observer
    # uses (Titania's lands): ``move_off_battlefield`` dispatches it for
    # artifacts (and lands) as well as creatures, with the departed
    # object as an observer of its own ``IsSelf()`` trigger. Not
    # LEAVES_BATTLEFIELD, which would also fire on exile / hand /
    # library moves.
    "this artifact is put into a graveyard from the battlefield": (
        Event.DIES, IsSelf(),
    ),
    # ``Whenever this creature attacks`` — the source's own attack
    # (Spacecraft attack triggers, Stellar Sights creatures, etc.).
    # ``Event.ATTACKS`` carries a single subject ``"attacker"``;
    # ``IsSelf()`` resolves it via ``_resolve_subject``'s single-
    # subject default. Phase 5 migrated every ``trigger="attacks"``
    # static card to this predicate shape.
    "this creature attacks": (Event.ATTACKS, IsSelf()),
    # ``Whenever this creature attacks for the first time each turn``
    # (Fear of Missing Out) -- an attack in an additional combat phase
    # doesn't trigger it again.
    "this creature attacks for the first time each turn": (
        Event.ATTACKS, IsSelf() & FirstAttackThisTurn(),
    ),
    # ``Whenever this creature becomes tapped`` — the source's own
    # tap (Starfighter Pilot, Nanoform Sentinel, Astrobiologist,
    # Sterling Hippogriff, etc.). ``Event.BECOMES_TAPPED`` carries a
    # single subject ``"obj"``; ``IsSelf()`` resolves it via the
    # single-subject default. Phase 7 migrated every
    # ``trigger="becomes_tapped"`` static card to this predicate
    # shape.
    "this creature becomes tapped": (Event.BECOMES_TAPPED, IsSelf()),
    # ``Whenever a creature you control with a +1/+1 counter on it
    # dies`` (Meltstrider Eulogist). Predicate: another creature you
    # control bearing a +1/+1 counter — the firing source's own death
    # is naturally excluded because predicate matching considers the
    # SUBJECT (dying object), and the most common static reading of
    # this phrase is "any creature you control" including self. The
    # engine fires once per death observer; if a printing also wants
    # to gate self-firing, add an explicit ``& ~IsSelf()`` in its
    # static definition (Meltstrider Eulogist's printed text doesn't
    # have that qualifier so we keep it broad).
    "a creature you control with a +1/+1 counter on it dies": (
        Event.DIES,
        Subject(None, IsCreature() & HasCounter("+1/+1")) & Controlled(YOU),
    ),
    # ``Whenever you cast an artifact spell`` — Pinnacle Emissary's
    # "whenever you cast an artifact spell, create a Drone token."
    # ``Event.IS_CAST`` carries subjects ``("spell", "controller")``;
    # ``Subject("spell", IsArtifact())`` matches the spell being cast
    # and ``Controlled(YOU, subject="controller")`` gates on it being
    # cast by the source's controller (vs an opponent).
    "you cast an artifact spell": (
        Event.IS_CAST,
        Subject("spell", IsArtifact()) & Controlled(YOU, subject="controller"),
    ),
    # The prowess-condition family (Dragon's Rage Channeler "Whenever
    # you cast a noncreature spell"; the instant-or-sorcery and
    # creature-spell siblings).
    "you cast a noncreature spell": (
        Event.IS_CAST,
        Subject("spell", ~HasCardType("Creature"))
        & Controlled(YOU, subject="controller"),
    ),
    "you cast a creature spell": (
        Event.IS_CAST,
        Subject("spell", HasCardType("Creature"))
        & Controlled(YOU, subject="controller"),
    ),
    "you cast an instant or sorcery spell": (
        Event.IS_CAST,
        Subject("spell", HasCardType("Instant") | HasCardType("Sorcery"))
        & Controlled(YOU, subject="controller"),
    ),
    # ``Whenever a nontoken artifact you control enters`` — Weapons
    # Manufacturing's trigger. ``Event.ENTERS`` carries ``("source",)``;
    # ``IsNontoken()`` excludes token artifacts (Landers, Drones, etc.)
    # so the enchantment doesn't go wide on its own token enters;
    # ``~IsSelf()`` prevents the enchantment itself (an Enchantment, not
    # an artifact) from triggering — belt-and-suspenders guard matching
    # the bespoke factory's explicit predicate.
    "a nontoken artifact you control enters": (
        Event.ENTERS,
        Subject(None, IsArtifact() & IsNontoken()) & Controlled(YOU) & ~IsSelf(),
    ),
    # ``Whenever you cast your second spell each turn`` — phase 8
    # migrated from ``trigger="second_spell_each_turn"`` to the
    # predicate path. ``Event.IS_CAST`` carries subjects ``("spell",
    # "controller")``; the ``Controlled(YOU, subject="controller")``
    # half gates "you cast" (vs an opponent), and
    # ``SpellsCastThisTurn(n=2)`` latches the moment the controller's
    # running total flips to 2 — mirroring the legacy
    # ``p.spells_cast_this_turn == 2`` gate at the
    # ``actions._fire_second_spell_triggers`` call site. Used by
    # Illvoi Operative, Sunstar Lightsmith, Cosmogrand Zenith,
    # Station Monitor.
    "you cast your second spell each turn": (
        Event.IS_CAST,
        Controlled(YOU, subject="controller") & StateBound(SpellsCastThisTurn(n=2)),
    ),
    # ``Whenever you cast a noncreature spell`` (The Fantasticar) — the
    # spell subject must be noncreature, cast by the source's controller.
    # Coveted Jewel — once per blocker declaration, when an opponent's
    # attacking creatures attacking you are (partly) unblocked.
    "one or more creatures an opponent controls attack you and aren't "
    "blocked": (
        Event.ATTACKERS_UNBLOCKED,
        Controlled(OPPONENT, subject="player")
        & Controlled(YOU, subject="defender"),
    ),
    # Kaldra Compleat's granted ability (the damaged creature is the
    # event's fixed target).
    "this creature deals combat damage to a creature": (
        Event.DEALS_COMBAT_DAMAGE_TO_CREATURE, IsSelf(subject="source"),
    ),
    # Sword of the Meek — a creature you control with P/T exactly 1/1.
    "a 1/1 creature you control enters": (
        Event.ENTERS,
        Subject(None, IsCreature() & HasPowerToughness(1, 1)) & Controlled(YOU),
    ),
    # Currency Converter — the discarded card is the fixed target.
    "you discard a card": (
        Event.CARD_DISCARDED, Controlled(YOU, subject="player"),
    ),
    # Staff of the Storyteller — batched per resolving stack item.
    "you create one or more creature tokens": (
        Event.CREATURE_TOKENS_CREATED, Controlled(YOU),
    ),
    "you cast a noncreature spell": (
        Event.IS_CAST,
        Subject("spell", ~IsCreature()) & Controlled(YOU, subject="controller"),
    ),
    # ``Whenever you cast your fourth noncreature spell each turn`` (The
    # Fantasticar). Latches on the controller's running noncreature
    # count, gated on the triggering spell itself being noncreature.
    **{
        f"you cast your {word} noncreature spell each turn": (
            Event.IS_CAST,
            Subject("spell", ~IsCreature())
            & Controlled(YOU, subject="controller")
            & StateBound(NoncreatureSpellsCastThisTurn(n=n)),
        )
        for word, n in (("second", 2), ("third", 3), ("fourth", 4))
    },
    # ``Whenever you sacrifice another creature or artifact`` — phase
    # 8 migrated from ``trigger="sacrifice_creature_or_artifact"``.
    # ``Event.IS_SACRIFICED`` carries the sacrificed permanent as
    # ``"obj"``; the predicate filters to creature-or-artifact subjects
    # under your control, and ``~IsSelf()`` excludes the source's own
    # sacrifice (matching the legacy ``other.id == obj.id: continue``
    # skip in ``turn.sacrifice``). Used by Lightless Evangel.
    "you sacrifice another creature or artifact": (
        Event.IS_SACRIFICED,
        Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU) & ~IsSelf(),
    ),
    # ``Whenever this creature deals combat damage to a player``
    # (Illvoi Infiltrator). Phase 9 migrated from
    # ``trigger="combat_damage_to_player"``. ``Event.DEALS_COMBAT_DAMAGE``
    # carries subjects ``("source", "target")`` — the source is the
    # damage-dealing permanent, the target is the damaged player or
    # creature. ``IsSelf(subject="source")`` ties the trigger to the
    # source's own damage-dealing; ``IsPlayer(subject="target")`` gates
    # "to a player" vs "to a creature". The broader ``a creature you
    # control deals combat damage`` variant (Starwinder) stays out of
    # the parser today — it's built directly in the static-card
    # registry — but a future addition would compose
    # ``IsCreature(subject="source") & Controlled(YOU,
    # subject="source") & IsPlayer(subject="target")`` here.
    "this creature deals combat damage to a player": (
        Event.DEALS_COMBAT_DAMAGE,
        IsSelf(subject="source") & Subject("target", IsPlayer()),
    ),
    # ``Whenever this creature blocks a creature with flying``
    # (Skystinger). Phase 9 migrated from
    # ``trigger="blocks_creature_with_flying"``. ``Event.BLOCKS``
    # carries subjects ``("blocker", "attacker")``;
    # ``IsSelf(subject="blocker")`` ties the trigger to the source
    # being the blocker; ``HasKeyword("flying", subject="attacker")``
    # gates on the attacker having flying right now (printed,
    # granted, or temp). Composes naturally to other block-filter
    # triggers — ``HasKeyword("trample", subject="attacker")`` etc.
    # — when those land in the card pool.
    "this creature blocks a creature with flying": (
        Event.BLOCKS,
        IsSelf(subject="blocker") & Subject("attacker", HasKeyword("flying")),
    ),
    # ``Whenever you put one or more +1/+1 counters on a creature you
    # control`` (Terrasymbiosis). ``Event.COUNTERS_ADDED`` carries the
    # subject ``"obj"`` (the permanent that received the counters);
    # the single-subject default resolves ``Subject(None, IsCreature())
    # & Controlled(YOU)`` against ``obj``, gating the trigger to a
    # creature under the source's controller; ``CountersPutBy(YOU)``
    # says who put them: you (CR 122.6; mtg-zero docs/ir_defects.md
    # 31: an opponent's effect putting counters on your creature
    # doesn't trigger it). The "one or more"
    # wording is implicit in the event semantics — the dispatcher
    # latches the aggregate counter amount so the resolving effect can
    # read it via ``state.resolving_counter_amount`` (see
    # ``DrawCardsEqualToResolvingCounterAmount``). The "Do this only
    # once each turn." rider is parsed separately by
    # ``_DO_THIS_ONCE_PER_TURN_RIDER_SUFFIX`` below and lifts to
    # ``once_per_turn=True`` on the produced ``TriggeredAbility``.
    "you put one or more +1/+1 counters on a creature you control": (
        Event.COUNTERS_ADDED,
        Subject(None, IsCreature()) & Controlled(YOU) & CountersPutBy(YOU),
    ),
    # ``Whenever a creature you control dies`` (Sothera, the
    # Supervoid). Batch 9. Broader than the existing
    # ``a creature you control with a +1/+1 counter on it dies``
    # phrase — no counter qualifier, fires on every death of a
    # creature you control. The static-card registry does NOT layer
    # ``~IsSelf()`` on Sothera's printed text, so the source itself
    # dying still triggers (a creature you control includes itself
    # while it's still on the battlefield at the moment its death
    # event resolves through the dispatcher's ``extra_observers``
    # path).
    "a creature you control dies": (
        Event.DIES, Subject(None, IsCreature()) & Controlled(YOU),
    ),
    # ``Whenever you sacrifice an artifact`` (Biotech Specialist).
    # Batch 9. Distinct from the existing
    # ``you sacrifice another creature or artifact`` (Lightless
    # Evangel) phrase — narrower type filter (artifact only) and the
    # ``an``-vs-``another`` surface distinction. The static registry
    # for Biotech layers ``~IsSelf()`` for symmetry with the
    # ``another`` family even though Biotech itself is a non-artifact
    # creature (so the source can never be the sacrificed artifact);
    # mirroring that here keeps parser/static parity exact and
    # leaves room for a future artifact-source card to print the
    # same phrase without re-introducing self-fire.
    "you sacrifice an artifact": (
        Event.IS_SACRIFICED,
        Subject(None, IsArtifact()) & Controlled(YOU) & ~IsSelf(),
    ),
    # ``Whenever a creature you control deals combat damage to a
    # player`` (Starwinder). Batch 9. Broader than the existing
    # ``this creature deals combat damage to a player`` phrase
    # (Illvoi Infiltrator) — fires for any creature you control,
    # not just the source. ``Event.DEALS_COMBAT_DAMAGE`` carries
    # subjects ``("source", "target")``;
    # ``IsCreature(subject="source") & Controlled(YOU,
    # subject="source")`` gates "a creature you control" on the
    # damage dealer; ``IsPlayer(subject="target")`` gates "to a
    # player" vs "to a creature". Mirrors the static registry's
    # Starwinder predicate exactly.
    "a creature you control deals combat damage to a player": (
        Event.DEALS_COMBAT_DAMAGE,
        Subject("source", IsCreature())
        & Controlled(YOU, subject="source")
        & Subject("target", IsPlayer()),
    ),
    # ``Whenever a nontoken artifact creature you control deals combat
    # damage to a player`` (Virulent Silencer). Batch 13. Stricter
    # variant of the existing ``a creature you control deals combat
    # damage to a player`` (Starwinder) — adds ``IsArtifact()`` and
    # ``IsNontoken()`` to the source filter so only nontoken artifact
    # creatures the controller controls satisfy the trigger. The
    # predicate fires once per connecting source via
    # ``Event.DEALS_COMBAT_DAMAGE``; the effect gives the target
    # player two poison counters. The damaged player is pre-wired as
    # ``fixed_targets`` by ``_fire_combat_damage_to_player_triggers``
    # so ``PutPoisonCountersOnTargetPlayer`` can read ``targets[0]``
    # at resolve time without a target-selection prompt.
    "a nontoken artifact creature you control deals combat damage to a player": (
        Event.DEALS_COMBAT_DAMAGE,
        Subject("source", IsCreature() & IsArtifact() & IsNontoken())
        & Controlled(YOU, subject="source")
        & Subject("target", IsPlayer()),
    ),
    # ``Whenever one or more creatures you control with +1/+1 counters
    # on them deal combat damage to a player`` (Haliya, Ascendant Cadet).
    # Counter-filtered variant of the "one or more creatures you control
    # deal combat damage to a player" shape below: one trigger per
    # combat damage event however many of them connect ("one or more",
    # CR 603.2c) and damaged player, and again in a later combat damage
    # step the same turn (a first-strike step, an additional combat): the
    # per-source dispatch is batched by ``OneOrMoreBatch``, not capped
    # with ``once_per_turn`` (Forth Eorlingas!'s shape below).
    "one or more creatures you control with +1/+1 counters on them deal combat damage to a player": (
        Event.DEALS_COMBAT_DAMAGE,
        Subject("source", IsCreature() & HasCounter("+1/+1"))
        & Controlled(YOU, subject="source")
        & Subject("target", IsPlayer())
        & OneOrMoreBatch(per_target_player=True),
    ),
    # ``Whenever one or more creatures you control deal combat damage
    # to a player`` (Elegy Acolyte; Locker Room — DSK Room half). Same
    # per-event predicate as the ``a creature you control deals combat
    # damage to a player`` Starwinder entry above, batched: one trigger
    # per combat damage event and damaged player ("one or more", CR
    # 603.2c), not once each turn (``OneOrMoreBatch``, as Forth
    # Eorlingas! below, whose "one or more players" is one batch).
    "one or more creatures you control deal combat damage to a player": (
        Event.DEALS_COMBAT_DAMAGE,
        Subject("source", IsCreature())
        & Controlled(YOU, subject="source")
        & Subject("target", IsPlayer())
        & OneOrMoreBatch(per_target_player=True),
    ),
    # ``... deal combat damage to one or more players`` (Forth
    # Eorlingas!'s this-turn trigger): one trigger per combat damage
    # event, however many creatures connect ("one or more", CR 603.2c),
    # and again in a later combat damage step the same turn: the
    # per-source dispatch is batched by ``OneOrMoreBatch`` (no second
    # instance while one from the same source waits on the stack), not
    # capped with ``once_per_turn``.
    "one or more creatures you control deal combat damage to one or more players": (
        Event.DEALS_COMBAT_DAMAGE,
        Subject("source", IsCreature())
        & Controlled(YOU, subject="source")
        & Subject("target", IsPlayer())
        & OneOrMoreBatch(),
    ),
    # ``Whenever a nontoken creature you control with a +1/+1
    # counter on it dies`` (Rayblade Trooper). Batch 9. Stricter
    # variant of the existing ``a creature you control with a
    # +1/+1 counter on it dies`` (Meltstrider Eulogist) — adds
    # ``IsNontoken()`` so token creatures dying don't satisfy the
    # trigger. Mirrors the static registry's Rayblade predicate
    # exactly.
    "a nontoken creature you control with a +1/+1 counter on it dies": (
        Event.DIES,
        Subject(None, IsCreature() & IsNontoken() & HasCounter("+1/+1"))
        & Controlled(YOU),
    ),
    # ``Whenever another creature or artifact you control enters``
    # (Weftstalker Ardent). Batch 9. Compound type qualifier — the
    # entering object can be either a creature OR an artifact under
    # your control, with the source itself excluded via ``~IsSelf()``
    # (the ``another`` qualifier). Mirrors the static registry's
    # Weftstalker predicate exactly. The ``a creature or artifact you
    # control enters`` (no ``another``) variant is not in the EoE
    # pool so it isn't added here — would be the same predicate
    # following the ``an artifact you control enters`` collapse
    # precedent if it ever ships.
    "another creature or artifact you control enters": (
        Event.ENTERS,
        Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU) & ~IsSelf(),
    ),
    # Batch 11 — type-qualified self-ETB / self-attack family. Every
    # entry in this group maps to the same ``IsSelf()`` predicate as
    # the pre-existing ``"this creature enters"`` / ``"this creature
    # attacks"`` entries: the source's identity is what matters, not
    # what type qualifier the printed text uses. The predicate's
    # object-identity check is type-blind, so the parser collapses
    # ``"this Spacecraft enters"`` / ``"this artifact enters"`` /
    # ``"this enchantment enters"`` / ``"this Aura enters"`` /
    # ``"this Spacecraft attacks"`` onto the same shape as the
    # creature-typed sibling.
    #
    # The EoE printings that carry these phrases today are all non-
    # creature card types (Spacecraft are artifacts; the Aura/
    # Enchantment/Artifact phrasings appear on their respective non-
    # creature card types) and don't currently flow through the
    # creature-builder ``_parse_clause`` pipeline that calls this
    # phrasebook — so the additions are forward-looking grammar
    # coverage in the same shape as batch 10's ``"it's your turn"``
    # static-condition entry. Adding them keeps the parser ready for
    # reprints and for future creature-typed printings (e.g. an
    # artifact-creature whose oracle text says ``"this artifact
    # enters"``) without lying about engine semantics. Every entry's
    # static-card sibling already builds via ``IsSelf()`` directly
    # (see ``cards/eoe_spells.py`` Pinnacle Starcage / Tractor Beam /
    # Larval Scoutlander / Specimen Freighter etc.), so parser/
    # static-registry parity holds out of the box.
    "this Spacecraft enters": (Event.ENTERS, IsSelf()),
    "this artifact enters": (Event.ENTERS, IsSelf()),
    "this enchantment enters": (Event.ENTERS, IsSelf()),
    "this Aura enters": (Event.ENTERS, IsSelf()),
    # Cube enchantments' own leaves triggers (Parallax Wave, Animate
    # Dead, Necromancy): the same type-blind ``IsSelf()`` as
    # ``this creature leaves the battlefield``.
    "this enchantment leaves the battlefield": (
        Event.LEAVES_BATTLEFIELD, IsSelf(),
    ),
    "this Aura leaves the battlefield": (
        Event.LEAVES_BATTLEFIELD, IsSelf(),
    ),
    # Lands print "this land" as their self-reference (surveil lands'
    # "When this land enters, surveil 1."); same type-blind ``IsSelf()``.
    "this land enters": (Event.ENTERS, IsSelf()),
    "this land attacks": (Event.ATTACKS, IsSelf()),
    "this Spacecraft attacks": (Event.ATTACKS, IsSelf()),
    # Cube Equipment / Vehicles: same type-blind ``IsSelf()`` family.
    "this Equipment enters": (Event.ENTERS, IsSelf()),
    "this Vehicle enters": (Event.ENTERS, IsSelf()),
    "this Vehicle attacks": (Event.ATTACKS, IsSelf()),
    # Equipment relational triggers (the Equipment is the source, its
    # host the subject). ``equipped creature dies`` reads the host
    # recorded at the moment it left (``IsFormerHost`` — the Equipment
    # is already unattached when DIES dispatches); Skullclamp.
    "equipped creature attacks": (Event.ATTACKS, IsHost(subject="attacker")),
    "equipped creature dies": (Event.DIES, IsFormerHost()),
    # Glimmer Lens — once per attack declaration, when the host and at
    # least one other creature you control attack.
    "equipped creature and at least one other creature attack": (
        Event.YOU_ATTACK, Controlled(YOU) & HostAttacksWithAnother(),
    ),
    # Batch 12 — Aura relational trigger family. Three trigger conditions
    # that fire based on what happens to the Aura's enchanted host:
    #
    # - ``"enchanted creature attacks"`` → Chorale of the Void's reanimate
    #   trigger. Maps to the same ``IsHost(subject="attacker")`` predicate
    #   the bespoke factory wires directly; moving it to the phrasebook
    #   makes it available to the unified Aura oracle-text parser.
    #
    # - ``"enchanted creature is dealt damage"`` → Cracked Skull /
    #   Sporogenic Infection / Cryoshatter's destroy-host trigger.
    #   ``Event.DEALT_DAMAGE`` carries the damaged subject; ``IsHost()``
    #   gates on the subject being the source Aura's enchanted permanent.
    #   The ``"enchanted creature becomes tapped"`` shape is handled by
    #   the compound recognizer for Cryoshatter's two-condition line in
    #   ``enchantment_builder``; it isn't in this phrasebook because no
    #   oracle text today uses it as a standalone ``When ... becomes
    #   tapped`` clause.
    #
    # Both share the ``IsHost()`` predicate pattern that the engine's
    # DEALT_DAMAGE and ATTACKS dispatch already uses for bespoke Aura
    # cards; the phrasebook additions are the parser-layer wiring only.
    "enchanted creature attacks": (Event.ATTACKS, IsHost(subject="attacker")),
    "enchanted creature is dealt damage": (Event.DEALT_DAMAGE, IsHost()),
    # ``Whenever you attack`` — DSK's once-per-declare-attackers trigger
    # family (Razorkin Hordecaller, Most Valuable Slayer, Lionheart
    # Glimmer). Distinct from ``"this creature attacks"`` (which fires
    # per-attacker via ``Event.ATTACKS``): ``Event.YOU_ATTACK`` carries
    # a single Player subject (``"player"``) and fires exactly ONCE per
    # declare-attackers pass when the active player has attacked with
    # at least one creature. ``Controlled(YOU)`` against a Player
    # subject reads as "the attacking player is the source's controller"
    # — same shape used by the turn-structure events
    # (``BEGINNING_OF_UPKEEP`` etc.). The trigger doesn't multiply with
    # attacker count, so the per-pass dispatch is the right semantic
    # for the printed phrasing.
    "you attack": (Event.YOU_ATTACK, Controlled(YOU)),
    # ``Whenever a creature you control attacks alone`` — Widow's
    # Walk's once-per-pass attack-alone trigger. Same once-per-
    # declare-attackers ``Event.YOU_ATTACK`` family as ``"you
    # attack"`` above (one fire per pass, not per attacker), with
    # the additional ``StateBound(LoneAttackerForYou())`` predicate
    # gating firing on exactly one creature attacking AND that lone
    # attacker being controlled by the source's controller. The
    # ``YOU_ATTACK`` event fires once after the entire pass — when
    # every attacker has flipped to ``attacking=True`` — giving the
    # state predicate the post-declaration battlefield to count;
    # using ``Event.ATTACKS`` here would falsely match the first of
    # N attackers before its siblings are declared. Effect-side
    # rewrite below in ``_parse_triggered_ability_line`` handles the
    # anaphoric ``"it gets …"`` body: the parser's default normalize-
    # and-self-pump path lands on ``PumpSelfUntilEOT`` (which would
    # pump the Room, not the attacker), so a context-dependent
    # rewrite swaps it to ``PumpTargetCreatureUntilEOT(attacking_only=
    # True, you_control_only=True)`` whose legal-target pool collapses
    # to the lone attacker via the predicate's uniqueness guarantee.
    "a creature you control attacks alone": (
        Event.YOU_ATTACK,
        Controlled(YOU) & StateBound(LoneAttackerForYou()),
    ),
    # ``Whenever an opponent discards a card`` — Entropic Battlecruiser's
    # 1+ tier trigger. ``Event.DISCARDS_CARD`` carries a single subject
    # ``"player"`` (the discarding player); ``Controlled(by=OPPONENT)``
    # gates on the subject being an opponent of the source's controller
    # so the trigger only fires when an OPPONENT discards. The discarding
    # player's id is pre-wired as ``fixed_targets`` by
    # ``fire_opponent_discards_triggers`` so the resolving effect
    # (``TargetPlayerLosesLife``) can damage them without cast-time
    # target selection.
    "an opponent discards a card": (
        Event.DISCARDS_CARD, Controlled(by=OPPONENT),
    ),
    # ``Whenever another creature you control with power 2 or less
    # enters`` — Vicious Clown's pump-self trigger and Irreverent
    # Gremlin's discard-and-draw trigger both print this exact
    # condition phrase. The predicate filters the entering subject to
    # creatures the source's controller controls whose effective power
    # is at most 2, with ``~IsSelf()`` excluding the source itself
    # (the ``another`` qualifier). The threshold is parameterized via
    # ``HasPowerAtMost(n=2)`` so a future printing with a different
    # threshold (``power 3 or less`` etc.) lands as a separate phrase
    # entry rather than a regex-driven dispatch — same pattern as
    # the existing fixed-text trigger phrasebook.
    "another creature you control with power 2 or less enters": (
        Event.ENTERS,
        Subject(None, IsCreature() & HasPowerAtMost(n=2)) & Controlled(YOU) & ~IsSelf(),
    ),
    # ``When you unlock this door`` — Duskmourn Room door triggers. The
    # canonical static-side wiring uses ``Event.ENTERS`` + ``IsSelf()``
    # routed through the Room's per-half ``unlocked_halves`` gate (see
    # ``_unlock_room_half`` / ``_fire_unlock_door_triggers`` in
    # ``actions.py``) — same predicate shape as ``"this creature
    # enters"``. The room builder threads each half's flattened
    # ``TriggeredAbility`` onto the parent printing with ``room_half``
    # tagged so the engine fires the door trigger only when its half
    # is unlocked (cast-time for the cast half, ``UnlockRoomHalf`` for
    # the other). This phrasebook entry is the unified-grammar entry
    # point — every Room half whose body reads ``"When you unlock this
    # door, <effect>"`` flows through the same parser path the rest of
    # the trigger grammar uses.
    "you unlock this door": (Event.ENTERS, IsSelf()),
}


# Compound trigger-condition phrases for the predicate-trigger path → a
# tuple of (Event, Predicate) pairs, one per condition arm. A single
# oracle line whose condition is ``X or Y`` (e.g. Thawbringer's
# ``When this creature enters or dies``) decomposes into multiple
# ``TriggeredAbility`` entries — one per arm — sharing the same effect.
# This mirrors the static-registry layout, where Thawbringer's two
# events live as two separate ``TriggeredAbility`` entries on the
# printing.
#
# The compound table is consulted BEFORE the single ``_PREDICATE_
# TRIGGER_PHRASES`` lookup so the more-specific compound shape wins.
# An entry here can co-exist with single-shape entries for either arm
# (``"this creature enters"`` and ``"this creature dies"`` both already
# live in ``_PREDICATE_TRIGGER_PHRASES`` — the compound entry below
# composes them without re-implementing either arm's predicate).
_COMPOUND_PREDICATE_TRIGGER_PHRASES: dict[
    str, tuple[tuple[Event, Predicate], ...]
] = {
    # ``When this creature enters or dies`` (Thawbringer) — fires once
    # on entry and once on death. The two arms share the same effect at
    # the line level; the parser produces one ``TriggeredAbility`` per
    # arm so the engine's per-event dispatch sees them as independent
    # observers (which matches the static-registry's two-entry layout).
    "this creature enters or dies": (
        (Event.ENTERS, IsSelf()),
        (Event.DIES, IsSelf()),
    ),
    # ``Whenever this creature enters or attacks`` — fires once on
    # entry and once on each attack. Surfaces as the named-self form
    # ``"Whenever <name> enters or attacks"`` on Haliya, Ascendant
    # Cadet and Syr Vondam, the Lucent (the parser does not yet
    # substitute the printing's own name for ``"this creature"`` in
    # trigger-condition phrases, so neither claims end-to-end today —
    # this entry is the trigger-phrase half of those pending claims).
    # The two arms share the same effect at the line level; the parser
    # produces one ``TriggeredAbility`` per arm to mirror the static-
    # registry's two-entry layout (Haliya / Syr Vondam each carry two
    # separate ``TriggeredAbility`` entries with identical effects in
    # their static definitions).
    "this creature enters or attacks": (
        (Event.ENTERS, IsSelf()),
        (Event.ATTACKS, IsSelf()),
    ),
    # ``Whenever this permanent enters or attacks`` — fires once on
    # entry and once on each attack. The ``"this permanent"`` surface
    # form appears on the Duskmourn Overlord cycle (Enchantment
    # Creatures with the Impending alt-cost) whose oracle text reads
    # ``"Whenever this permanent enters or attacks, …"`` rather than
    # ``"this creature"`` because the permanent isn't always a creature
    # (while impending-suppressed it has time counters and its printed
    # creature type is suspended). The predicate is identical to the
    # ``"this creature enters or attacks"`` entry above — ``IsSelf()``
    # is type-blind and only checks object identity — so the two
    # entries share the same (ENTERS, IsSelf) / (ATTACKS, IsSelf) pair.
    # Mirroring the static-registry's two-``TriggeredAbility`` layout
    # for the Overlord cycle: one ENTERS entry and one ATTACKS entry
    # per printing, both with ``IsSelf()``.
    "this permanent enters or attacks": (
        (Event.ENTERS, IsSelf()),
        (Event.ATTACKS, IsSelf()),
    ),
    # ``When this artifact enters or leaves the battlefield`` (Cryogen
    # Relic) — fires once on entry and once on leave. Mirrors the
    # static-registry layout for Cryogen Relic, which carries two
    # separate ``TriggeredAbility`` entries (ENTERS+IsSelf, then
    # LEAVES_BATTLEFIELD+IsSelf) with the same effect. The type-blind
    # ``IsSelf()`` predicate handles the artifact identity check —
    # mirrors the ``"this artifact enters"`` entry above.
    "this artifact enters or leaves the battlefield": (
        (Event.ENTERS, IsSelf()),
        (Event.LEAVES_BATTLEFIELD, IsSelf()),
    ),
    # Smuggler's Copter — "Whenever this Vehicle attacks or blocks".
    "this Vehicle attacks or blocks": (
        (Event.ATTACKS, IsSelf()),
        (Event.BLOCKS, IsSelf(subject="blocker")),
    ),
    # Duskmourn "Eerie" ability-word trigger — fires both when an
    # enchantment under your control enters AND when you fully unlock
    # a Room. The ``Eerie — `` ability-word prefix strips through the
    # generic ``_strip_ability_word_prefix`` helper before this lookup
    # runs (no Eerie-specific prefix code needed; ability words have
    # no rules consequence per CR 207.2c). The printed surface joins
    # the two arms with ``and whenever`` rather than ``or`` —
    # mechanically a single oracle line produces two independent
    # ``TriggeredAbility`` entries (one per event) that share the
    # same effect, mirroring the existing Thawbringer ``enters or
    # dies`` precedent.
    #
    # Arm 1 (ENTERS): ``IsEnchantment() & Controlled(YOU)`` — any
    # enchantment under the source's controller entering, including
    # Rooms (which carry ``"Enchantment"`` in ``card_types``) and the
    # source itself if it's an Enchantment Creature (Fear of Infinity).
    # No ``~IsSelf()`` because the printed text is "an enchantment you
    # control enters" with no "another" qualifier.
    #
    # Arm 2 (ROOM_FULLY_UNLOCKED): ``Controlled(YOU, subject="room")``
    # — the Room that just transitioned to all-halves-unlocked is
    # under the source's controller. The dispatch fires from
    # ``_unlock_room_half`` at the unlock-action transition; the
    # event subject ``"room"`` is the Room ``GameObject`` itself.
    "an enchantment you control enters and whenever you fully unlock a Room": (
        (Event.ENTERS, Subject(None, IsEnchantment()) & Controlled(YOU)),
        (Event.ROOM_FULLY_UNLOCKED, Controlled(YOU, subject="room")),
    ),
    # ``Whenever this creature or another Kavu you control enters``
    # (Possibility Technician). Two arms — self-ETB and Kavu-observer
    # — both fire ENTERS with different predicates but the same effect.
    # Produces two ``TriggeredAbility`` entries mirroring the bespoke's
    # two-entry layout so the legacy ``"etb"`` / ``"kavu_you_control_
    # enters"`` test keys both apply.
    "this creature or another Kavu you control enters": (
        (Event.ENTERS, IsSelf()),
        (Event.ENTERS, Subject(None, HasSubtype("Kavu")) & Controlled(YOU) & ~IsSelf()),
    ),
    # ``Whenever you play a land or cast a spell`` (The Endstone) —
    # composite play-land-or-cast-spell trigger. Two arms fire on the
    # same line, each carrying the same effect at the line level; the
    # parser produces one ``TriggeredAbility`` per arm so the engine's
    # per-event dispatch sees them as independent observers (matching
    # the static registry's two-entry layout). ``Event.PLAYS_LAND``
    # carries ``("land", "controller")`` subjects and ``Event.IS_CAST``
    # carries ``("spell", "controller")``; both are gated on
    # ``Controlled(YOU, subject="controller")`` so the trigger fires
    # only on the source controller's own plays/casts (not opponents').
    "you play a land or cast a spell": (
        (Event.PLAYS_LAND, Controlled(YOU, subject="controller")),
        (Event.IS_CAST, Controlled(YOU, subject="controller")),
    ),
}


# Trigger-condition phrases that read as ``X or Y dies/enters`` but
# whose two arms collapse onto a SINGLE predicate in the static
# registry — the wording is inclusive (``this creature or another
# <subset> you control``) and the static carries one
# ``TriggeredAbility`` whose predicate covers both halves at once.
# Distinct from ``_COMPOUND_PREDICATE_TRIGGER_PHRASES`` (which
# produces one ``TriggeredAbility`` per arm) — these collapsed
# phrases produce exactly one ability.
#
# The dispatcher's ``extra_observers=[dead]`` plumbing at the DIES
# fire site is what lets the source's own death satisfy a
# ``Controlled(YOU)`` predicate even though the dying object has
# already been moved off the battlefield by the time the trigger
# fires; the ``this creature or another …`` wording is explicit
# about including self in the firing pool.
_COLLAPSED_PREDICATE_TRIGGER_PHRASES: dict[
    str, tuple[Event, Predicate]
] = {
    # ``Whenever this creature or another creature or artifact you
    # control dies`` (Susurian Voidborn). The two arms (``this
    # creature`` + ``another creature or artifact you control``)
    # collapse to a single ``Controlled(YOU) & (creature OR artifact)``
    # predicate — no ``~IsSelf()`` because the leading ``this creature``
    # arm explicitly includes self. Mirrors the static registry's
    # Susurian Voidborn predicate exactly.
    "this creature or another creature or artifact you control dies": (
        Event.DIES,
        Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU),
    ),
    # ``Whenever this creature or another Kavu you control enters``
    # (Possibility Technician). The two arms (``this creature`` +
    # ``another Kavu you control``) collapse to a single
    # ``Subject(None, HasSubtype("Kavu")) & Controlled(YOU)`` predicate
    # — the source is itself a Kavu, so ``IsSelf()`` is subsumed by the
    # Kavu subtype check; no ``~IsSelf()`` because ``this creature``
    # explicitly includes self. The bespoke modeled this as two separate
    # ``TriggeredAbility`` entries with identical effects; the collapsed
    # form produces one, firing once per entering Kavu you control.
    # ``Whenever this creature or another creature or artifact you
    # control enters`` (Haliya, Guided by Light). The two arms
    # (``this creature`` + ``another creature or artifact you
    # control``) collapse to a single ``Controlled(YOU) & (creature
    # OR artifact)`` predicate — no ``~IsSelf()`` because the oracle
    # text's leading "Haliya" arm explicitly includes the source itself.
    # The bespoke table used ``~IsSelf()`` (incorrectly excluding self-
    # entry); the faithful reading includes it, matching the printed
    # oracle: "Whenever Haliya or another creature or artifact you
    # control enters".
    "this creature or another creature or artifact you control enters": (
        Event.ENTERS,
        Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU),
    ),
}


def _parse_collapsed_predicate_trigger_phrase(
    phrase: str,
) -> tuple[Event, Predicate] | None:
    return _COLLAPSED_PREDICATE_TRIGGER_PHRASES.get(phrase.strip())


def _parse_compound_predicate_trigger_phrase(
    phrase: str,
) -> tuple[tuple[Event, Predicate], ...] | None:
    return _COMPOUND_PREDICATE_TRIGGER_PHRASES.get(phrase.strip())


# Intervening-if gate phrases → predicate. The phrase is the verbatim
# oracle text between ``if`` and the trailing comma of the middle
# clause (after stripping surrounding whitespace). Each predicate is a
# game-state check evaluated against the source's controller and the
# current battlefield — none of these gates depend on a per-event
# subject, so they compose with the trigger's existing event predicate
# via plain ``And``. The dispatcher's predicate evaluation handles the
# trigger-time gate (CR 603.4 trigger check); the matching resolve-
# time recheck on the same predicate is not yet wired through the
# resolve pipeline, so a state change between trigger and resolve is
# a known gap (the existing static cards using these gates rely on
# the same trigger-time-only check today via ``requires_void`` /
# ``ConditionalIf*`` effect wrappers, which are themselves single-
# point gates).
#
# The "Void —" ability-word prefix on Alpharael Stonechosen / Roving
# Actuator strips through ``_strip_ability_word_prefix`` before this
# lookup runs, so the ``"a nonland permanent left the battlefield
# this turn or a spell was warped this turn"`` gate phrase here covers
# both prefixed and bare wordings of the Void rider.
_INTERVENING_IF_PHRASES: dict[str, Predicate] = {
    "an opponent controls more lands than you": (
        StateBound(OpponentControlsMoreLandsThanYou())
    ),
    (
        "a nonland permanent left the battlefield this turn "
        "or a spell was warped this turn"
    ): StateBound(VoidActive()),
    # Duskmourn "Delirium" gate — the canonical printed phrasing of
    # the four-card-types-in-graveyard threshold. Pairs with the
    # generic ``Delirium — `` ability-word prefix that strips through
    # ``_strip_ability_word_prefix`` (no Delirium-specific helper
    # needed; the prefix has no rules consequence per CR 207.2c).
    # Mirrors the ``VoidActive()`` gate's role for the EoE Void
    # mechanic — same shape, different trigger-time game-state check.
    "there are four or more card types among cards in your graveyard": (
        StateBound(Delirium())
    ),
    # Sothera, the Supervoid's end-step intervening-if gate:
    # "if a player controls no creatures" — at least one (non-eliminated)
    # player controls zero creatures on the battlefield. Models the
    # printed pivot phrase that fires Sothera's end-step recall only
    # when somebody has been wiped of creatures. The matching
    # ``StateBound(AnyPlayerControlsNoCreatures())`` predicate evaluates
    # at trigger time per CR 603.4 (the engine's intervening-if dispatch
    # is trigger-time only today; the resolve-time recheck is enforced
    # inside the effect-body primitives so the printed gate stays
    # honest at both checkpoints).
    "a player controls no creatures": (
        StateBound(AnyPlayerControlsNoCreatures())
    ),
    # DSK Enduring cycle: "When this creature dies, if it was a
    # creature, return it to the battlefield under its owner's
    # control. It's an enchantment." Every Enduring creature in DSK
    # carries the same intervening-if gate, paired with the cycle's
    # return-as-enchantment effect body.
    #
    # The gate predicate is ``IsCreature()`` — it evaluates against
    # the dispatched subject (the dying object) via its printing's
    # ``card_types``. Every Enduring printing is an Enchantment
    # Creature, so the gate is naturally true on the first death
    # (the printing has "Creature"). After the effect returns the
    # card "as an enchantment", the engine's ``is_creature_now``
    # short-circuits to False (via the new
    # ``enduring_returned_as_enchantment`` flag) so the
    # ``_fire_dies_triggers`` ``was_creature`` gate at the call site
    # in ``move_off_battlefield`` already prevents a second death
    # from re-firing the trigger. The gate predicate composed here
    # is therefore behaviorally redundant in the engine today, but
    # we register it so parser/static parity stays exact and the
    # gate is in place if a future engine change ever loosened the
    # call-site ``was_creature`` check.
    "it was a creature": Subject(None, IsCreature()),
    # Vaultborn Tyrant's dies trigger (cube W/G bucket): "if it's not a
    # token" -- the dying object (the trigger's subject) isn't a token.
    "it's not a token": Subject(None, IsNontoken()),
    # Designation gates (``mtgz_ir.designations``): Feywild Caretaker /
    # Passageway Seer ``if you have the initiative``, the monarch
    # family ``if you're the monarch``, White Plume Adventurer-style
    # ``if you've completed a dungeon``.
    "you have the initiative": StateBound(HasInitiative()),
    "you're the monarch": StateBound(IsMonarch()),
    "you've completed a dungeon": StateBound(CompletedADungeon()),
    # Cube W/G bucket: Ocelot Pride's end-step gate / city's blessing.
    "you gained life this turn": StateBound(GainedLifeThisTurn()),
    "you have the city's blessing": StateBound(HasCitysBlessing()),
    # Threshold (``Threshold —`` ability word strips upstream).
    "there are seven or more cards in your graveyard": (
        StateBound(CardsInGraveyardAtLeast(7))
    ),
    # Mana Vault: "At the beginning of your draw step, if this artifact
    # is tapped, …" — the source-state gate (the event subject is the
    # active player, so the check reads the source via ``Source``).
    "this artifact is tapped": Source(IsTapped()),
    # Fastbond: "if it wasn't the first land you played this turn" —
    # checked after the triggering land play is counted.
    "it wasn't the first land you played this turn": (
        StateBound(LandsPlayedThisTurnAtLeast(2))
    ),
}


# Trailing-if gate phrases — the gate clause that follows the effect
# in the ``When/Whenever <condition>, <effect> if <gate>.`` shape.
# Distinct from ``_INTERVENING_IF_PHRASES`` (middle-clause gates)
# because the printed phrasing differs: middle-clause gates lead
# with an existential subject (``there are four or more card
# types``), trailing gates lead with the controller's pronoun
# (``you have four or more card types``). The predicate semantics
# are identical — both trigger-time checks delegate to the same
# ``Delirium()`` evaluation — but the surface text doesn't reuse,
# so we keep separate phrasebooks rather than registering both
# phrasings under one entry and risking a maintainer assuming
# every middle-clause phrase has a trailing twin (it doesn't —
# Void / opponent-more-lands gates have no printed trailing form).
_TRAILING_IF_PHRASES: dict[str, Predicate] = {
    "you have four or more card types among cards in your graveyard": (
        StateBound(Delirium())
    ),
}


def _parse_intervening_if_phrase(phrase: str) -> Predicate | None:
    pred = _INTERVENING_IF_PHRASES.get(phrase.strip())
    if pred is not None:
        return pred
    # Cube creatures (R / multicolor / colorless) bucket gates.
    from mtgz_ir.cards.parser.creature_rmc_grammar import (
        parse_rmc_intervening_if,
    )

    pred = parse_rmc_intervening_if(phrase.strip())
    if pred is not None:
        return pred
    # Cube W/G creature bucket gates ("When you cast this spell" riders).
    from mtgz_ir.cards.parser.creature_wg_effects import parse_wg_intervening_if

    pred = parse_wg_intervening_if(phrase.strip())
    if pred is not None:
        return pred
    # Cube layouts bucket gates ("if you control N or more lands").
    from mtgz_ir.cards.parser.layout_face_grammar import (
        parse_face_intervening_if,
    )

    pred = parse_face_intervening_if(phrase.strip())
    if pred is not None:
        return pred
    # Roto P6 gates ("if it has dealt N or more damage to that player").
    from mtgz_ir.cards.parser.roto_keywords import parse_p6_intervening_if

    return parse_p6_intervening_if(phrase.strip())


# Intervening-``while`` gate phrases. The DSK Delirium "while X"
# shape (Hand That Feeds) reuses the same surface text as the
# middle-clause ``, if X,`` shape, so the phrasebook is identical
# today — a single registered entry for the four-or-more-card-types
# threshold. Kept as a separate dict (vs. routing the ``while`` lookup
# into ``_INTERVENING_IF_PHRASES``) so future ``while``-only or
# ``if``-only gate phrasings land cleanly on their own table without
# silently widening the other surface form's grammar.
_INTERVENING_WHILE_PHRASES: dict[str, Predicate] = {
    "there are four or more card types among cards in your graveyard": (
        StateBound(Delirium())
    ),
    # Emberwilde Captain (roto P6).
    "you're the monarch": StateBound(IsMonarch()),
    # Syr Vondam, Sunstar Exemplar: "When this creature dies or is put
    # into exile while its power is 4 or greater, ...": part of the
    # trigger event, the leaving creature's power as it last existed on
    # the battlefield (CR 603.10a, 603.6c), not a check on resolution
    # (mtg-zero docs/ir_defects.md 80).
    "its power is 4 or greater": Subject(None, HasPowerAtLeast(4)),
}


def _parse_intervening_while_phrase(phrase: str) -> Predicate | None:
    return _INTERVENING_WHILE_PHRASES.get(phrase.strip())


# Intervening-``while`` gates whose semantics are best expressed as an
# effect wrapper rather than a predicate AND-ed onto the trigger event
# predicate. Each entry maps a gate phrase to a factory function
# ``(inner_effect) -> Effect`` that wraps the parsed inner effect in the
# appropriate conditional wrapper class. The trigger's event and predicate
# stay unchanged (no AND); the gate is checked at resolve time via the
# wrapper's own logic.
#
# This table is consulted AFTER ``_INTERVENING_WHILE_PHRASES`` in the
# dispatch: if the gate phrase is a predicate gate it wins; if it's only
# in this table the effect-wrapper path runs instead. The two tables are
# mutually exclusive by construction (each phrase appears in at most one).
#
# (Syr Vondam's ``"its power is 4 or greater"`` was an entry here, read on
# resolution; it is a predicate gate in ``_INTERVENING_WHILE_PHRASES`` now,
# part of the trigger event. No phrase needs a wrapper today.)
_INTERVENING_WHILE_EFFECT_WRAPPERS: dict[str, "callable"] = {}


def _parse_intervening_while_effect_wrapper(phrase: str):
    """Look up ``phrase`` in ``_INTERVENING_WHILE_EFFECT_WRAPPERS``.

    Returns the factory callable ``(inner_effect) -> Effect`` for a
    recognized gate phrase, or ``None`` when the phrase isn't claimed.
    The factory wraps the parsed inner effect in the appropriate
    conditional wrapper class at dispatch time.
    """
    return _INTERVENING_WHILE_EFFECT_WRAPPERS.get(phrase.strip())


# Intervening-``if`` gates whose semantics are best expressed as an
# effect wrapper rather than a predicate AND-ed onto the trigger event
# predicate. Same shape as ``_INTERVENING_WHILE_EFFECT_WRAPPERS`` but
# for the ``, if <gate>,`` middle-clause surface (vs ``while <gate>``).
# Each entry maps a gate phrase to a factory ``(inner) -> Effect`` that
# wraps the parsed inner effect; the trigger's event and predicate
# stay unchanged.
#
# This table is consulted AFTER ``_INTERVENING_IF_PHRASES`` in the
# dispatch: predicate gates win when both tables claim a phrase (today
# they're mutually exclusive by construction).
#
# ``"you cast it"`` — Weftwalking's ETB middle clause: "When this
# enchantment enters, if you cast it, shuffle your hand and graveyard
# into your library, then draw seven cards." Wraps the inner effect in
# ``ConditionalIfCastFromHand(inner)``; today the wrapper is a
# structural no-op because the engine puts enchantments onto the
# battlefield only via casting, but we register the wrapper so the
# printed gate isn't silently dropped — same fidelity discipline the
# Void/Survival/Delirium ability-word gates follow.
_INTERVENING_IF_EFFECT_WRAPPERS: dict[str, "callable"] = {
    "you cast it": lambda inner: ConditionalIfCastFromHand(inner_effect=inner),
}


def _parse_intervening_if_effect_wrapper(phrase: str):
    """Look up ``phrase`` in ``_INTERVENING_IF_EFFECT_WRAPPERS``.

    Returns the factory ``(inner_effect) -> Effect`` for a recognized
    gate phrase, or ``None`` when the phrase isn't claimed. Used by
    the intervening-if dispatch as a fallback after the predicate
    phrasebook misses, mirroring the parallel ``while`` path.
    """
    return _INTERVENING_IF_EFFECT_WRAPPERS.get(phrase.strip())


def _parse_trailing_if_phrase(phrase: str) -> Predicate | None:
    """Look up ``phrase`` (a trailing-``if`` gate clause from the
    ``... if <gate>.`` tail) in ``_TRAILING_IF_PHRASES``. Returns the
    matching ``Predicate`` or ``None`` when the phrase isn't claimed.
    """
    return _TRAILING_IF_PHRASES.get(phrase.strip())


# Ability-word prefix on a triggered-ability line. An ability word is
# one or more capitalized words followed by `` — `` and a triggered-
# ability body. Per the comprehensive rules ability words have no
# rules consequence, so we strip whatever label appears without
# enumerating them — Landfall, Magecraft, Constellation, Domain, etc.
# all flow through the same path. The lookahead requires
# ``When``/``Whenever`` after the dash so type-line strings (e.g.
# ``Creature — Dragon``) and other em-dash uses don't accidentally
# match. Flavor words (CR 207.2d -- "Terror from the Deep —") share the
# shape and are equally rules-free.
_ABILITY_WORD_PREFIX = re.compile(
    r"^[A-Z][A-Za-z']*(?: (?:[A-Z][A-Za-z']*|of|the|and|from))* — "
    r"(?=When |Whenever )"
)


def _strip_ability_word_prefix(line: str) -> str:
    """Strip a generic ``<Ability Word> — `` prefix from ``line`` if
    one is present in front of a ``When``/``Whenever`` body.

    Ability words carry no rules meaning, so the parser doesn't need
    to know which ability word it's looking at — the substance lives
    entirely in the ``Whenever …`` clause that follows. Returns
    ``line`` unchanged when no such prefix is present.
    """
    return _ABILITY_WORD_PREFIX.sub("", line, count=1)


def _parse_trigger_phrase(phrase: str) -> str | None:
    return _TRIGGER_PHRASES.get(phrase.strip())


def _parse_predicate_trigger_phrase(
    phrase: str,
) -> tuple[Event, Predicate] | None:
    """Look up ``phrase`` in the predicate-trigger phrasebook.

    Returns ``(Event, Predicate)`` for a recognized phrase or ``None``
    otherwise. Some entries' value form is a 3-tuple
    ``(Event, Predicate, once_per_turn)`` carrying an implicit
    aggregated semantic (Locker Room's "one or more creatures you
    control deal combat damage to a player" is the canonical example,
    one fire per combat regardless of how many sources connect). The
    once-per-turn flag is consulted via the sibling
    ``_predicate_trigger_phrase_is_once_per_turn`` helper so this
    primary lookup keeps its long-standing ``(Event, Predicate)`` shape
    for direct test consumers — the public surface stays stable while
    the dispatcher gains the new flag through the parallel helper.
    """
    raw = _PREDICATE_TRIGGER_PHRASES.get(phrase.strip())
    if raw is None:
        # Parameterized fallbacks ("you draw a card", "you cast a[n]
        # <filter> spell") -- see ``effects_ext.parse_ext_trigger_phrase``.
        from mtgz_ir.cards.parser.effects_ext import parse_ext_trigger_phrase

        ext = parse_ext_trigger_phrase(phrase.strip())
        if ext is not None:
            return ext
        # Cube creatures (R / multicolor / colorless) bucket conditions.
        from mtgz_ir.cards.parser.creature_rmc_grammar import (
            parse_rmc_trigger_phrase,
        )

        rmc = parse_rmc_trigger_phrase(phrase.strip())
        if rmc is not None:
            return rmc
        # Draw-observer conditions (U/B creature bucket).
        from mtgz_ir.cards.parser.cube_creatures_ub import (
            parse_ub_trigger_phrase,
        )

        ub = parse_ub_trigger_phrase(phrase.strip())
        if ub is not None:
            return ub
        # Cube layouts bucket conditions ("transforms into", ...).
        from mtgz_ir.cards.parser.layout_grammar import (
            parse_layout_trigger_phrase,
        )

        layout = parse_layout_trigger_phrase(phrase.strip())
        if layout is not None:
            return layout
        # Vintage Roto P5 conditions ("becomes blocked", ...).
        from mtgz_ir.cards.parser.roto_board import (
            parse_roto_trigger_phrase as parse_roto_board_trigger_phrase,
        )

        board = parse_roto_board_trigger_phrase(phrase.strip())
        if board is not None:
            return board
        # Roto P4 conditions ("this creature becomes the target of ...").
        from mtgz_ir.cards.parser.roto_walkers import parse_roto_trigger_phrase

        walker = parse_roto_trigger_phrase(phrase.strip())
        if walker is not None:
            return walker
        # Roto P6 conditions (becomes the target, ...).
        from mtgz_ir.cards.parser.roto_keywords import parse_p6_trigger_phrase

        return parse_p6_trigger_phrase(phrase.strip())
    return raw[0], raw[1]


def _predicate_trigger_phrase_is_once_per_turn(phrase: str) -> bool:
    """Return whether the predicate-trigger phrasebook entry for
    ``phrase`` carries an implicit ``once_per_turn=True`` rider.

    Returns ``False`` if the phrase isn't in the phrasebook OR if the
    value form is the legacy 2-tuple ``(Event, Predicate)``. Returns
    ``True`` only for entries whose value is a 3-tuple
    ``(Event, Predicate, True)`` — the printed-wording-aggregated
    surfaces (Locker Room's "one or more creatures you control deal
    combat damage to a player" is the only such entry today). The
    dispatcher OR-combines this flag with the trailing-rider
    ``This ability triggers only once each turn.`` flag stripped
    earlier — both are honest sources of ``once_per_turn=True`` and
    either alone is sufficient for the final ``TriggeredAbility``.
    """
    raw = _PREDICATE_TRIGGER_PHRASES.get(phrase.strip())
    if raw is None or len(raw) < 3:
        return False
    return bool(raw[2])


def _substitute_self_name(line: str, name: str) -> str:
    """Replace the printing's own name (and the short-form name for
    legendary commas) with ``"this creature"`` inside the line.

    Scryfall oracle text refers to a legendary creature's source by
    its short name (the substring before the first comma — ``Haliya``
    on Haliya, Ascendant Cadet, ``Syr Vondam`` on Syr Vondam, the
    Lucent). Non-legendary printings refer to themselves either as
    ``"this creature"`` (the canonical form already accepted by every
    trigger-phrase table) or by their full printed name. This helper
    rewrites both the full and short forms uniformly, so the
    downstream condition / effect grammar only needs the canonical
    surface.

    The substitution is whole-word (regex word-boundary anchored) so
    a substring match inside another token can't fire — e.g. a
    hypothetical name ``"Sami"`` doesn't accidentally match inside
    ``"sample"``. The full name is tried before the short form so a
    printing whose full name appears verbatim in the oracle text
    (rare; usually the short form is used) still picks up the more-
    specific replacement first.
    """
    original = line
    full_pattern = r"\b" + re.escape(name) + r"\b"
    line = re.sub(full_pattern, "this creature", line)
    short = name.split(",", 1)[0]
    if short and short != name:
        short_pattern = r"\b" + re.escape(short) + r"\b"
        line = re.sub(short_pattern, "this creature", line)
    elif " of " in name:
        # "Loran of the Third Path" is referred to as "Loran" (the
        # epithet form of a legendary name without a comma). The
        # negative lookahead keeps any remaining "<Name> of ..." text
        # (another card's name) intact.
        epithet = name.split(" of ", 1)[0]
        line = re.sub(
            r"\b" + re.escape(epithet) + r"\b(?! of )", "this creature", line,
        )
    # A plural-named creature ("Cloak and Dagger enter", "... leave the
    # battlefield") takes plural verbs; after the rewrite to the
    # singular "this creature" restore subject-verb agreement so the
    # canonical phrase tables match. Only rewritten names are touched
    # ("you may have this creature enter as a copy" is a bare infinitive,
    # hence the ``have`` lookbehind).
    if line == original:
        return line
    return _PLURAL_SELF_VERB_RE.sub(r"this creature \1s", line)


_PLURAL_SELF_VERB_RE = re.compile(
    r"(?<!have )\bthis creature (enter|leave|attack|block|die|deal|fight|"
    r"become)\b"
)


def _parse_at_the_beginning_of_line(
    match: "re.Match[str]",
    requires_void: bool = False,
    is_survival: bool = False,
    is_delirium: bool = False,
) -> TriggeredAbility | None:
    """Body parser for ``At the beginning of <when>, <effect>.`` lines.

    ``match`` is the result of ``_AT_THE_BEGINNING_OF_LINE.match(line)``
    — group 1 is the time-condition phrase (``your end step`` etc.) and
    group 2 is the effect tail (the rest of the sentence between the
    first ``, `` and the trailing period). The time phrase resolves via
    ``_parse_at_beginning_of_phrase`` to the matching ``(Event,
    Predicate)`` pair; the effect tail is parsed via
    ``_parse_effect_phrase`` (the same primitive every other trigger
    body uses, so the unified effect grammar stays the single source
    of truth).

    ``requires_void`` is True when the calling site already detected a
    leading ``Void — `` prefix on the original line. In that case the
    body MUST start with the verbatim ``_VOID_GATE_PREFIX`` middle
    clause — the gate phrase is elided (the engine's
    ``requires_void`` flag is the canonical gate) and the remaining
    inner effect parses through the unified grammar. A void-flagged
    line whose body does NOT carry the matching gate is rejected
    outright: the printed Void rider is always paired with the gate
    phrase, and a missing gate would mean we're misreading the line.

    ``is_delirium`` is True when the calling site already detected a
    leading ``Delirium — `` ability-word prefix on the original line.
    In that case the body MUST start with the verbatim
    ``_DELIRIUM_AT_BEGINNING_GATE_PREFIX`` middle clause — the gate
    phrase is stripped and ``StateBound(Delirium())`` is AND-ed onto
    the trigger's base predicate so the ability fires only when the
    source's controller has four or more distinct card types in their
    graveyard (CR 603.4 intervening-if check). Mirrors the Survival
    predicate-compose precedent: the gate becomes a trigger-time
    predicate component rather than an engine flag.

    Returns ``None`` if any piece doesn't parse — the line has no
    other valid interpretation (unlike the When/Whenever shape, which
    has multiple line-level fallbacks); rejecting here propagates as a
    parse failure for the whole oracle line so the surrounding card
    stays unclaimed rather than silently dropping the trigger.
    """
    when_match = _parse_at_beginning_of_phrase(match.group(1))
    if when_match is None:
        return None
    event, predicate = when_match
    body = match.group(2)
    # Void rider — the line was preceded by a ``Void — `` prefix. The
    # body must carry the verbatim void-gate middle clause; strip it
    # and parse the remainder as a bare effect. The
    # ``requires_void=True`` flag on the produced ``TriggeredAbility``
    # is the canonical gate — mirrors the static-registry layout for
    # Insatiable Skittermaw / Voidforged Titan / Kavaron Skywarden /
    # Hylderblade exactly. Tried BEFORE the tapped-creatures wrapper
    # below because the two prefixes are disjoint (no printing today
    # combines a Void rider with a tapped-creatures gate), but
    # ordering ``requires_void`` first keeps the Void path's
    # "elide gate, set flag" handling local.
    if requires_void:
        if not body.startswith(_VOID_GATE_PREFIX):
            return None
        inner_phrase = body[len(_VOID_GATE_PREFIX):]
        inner = _parse_effect_phrase(inner_phrase)
        if inner is None:
            return None
        return TriggeredAbility(
            event=event, predicate=predicate, effect=inner,
            requires_void=True,
        )
    # Survival rider — the line was preceded by a ``Survival — ``
    # prefix. The body must carry the verbatim ``if this creature is
    # tapped, `` middle clause; strip it and parse the remainder as a
    # bare effect. The intervening-if gate becomes the predicate
    # ``IsSelf() & IsTapped()``, AND-ed onto the trigger's base
    # predicate (``Controlled(YOU)``) so the firing condition reads
    # "the source's controller is the active player AND the source is
    # tapped" — matching CR 603.4 trigger-time intervening-if check on
    # the dispatched event. Tried BEFORE the tapped-creatures effect-
    # wrapper below because the two prefixes are distinguished by the
    # caller's ``is_survival`` flag (no printing today combines a
    # Survival rider with a tapped-creatures gate). Mirrors the Void
    # rider precedent above structurally.
    if is_survival:
        if not body.startswith(_SURVIVAL_GATE_PREFIX):
            return None
        inner_phrase = body[len(_SURVIVAL_GATE_PREFIX):]
        inner = _parse_effect_phrase(inner_phrase)
        if inner is None:
            return None
        return TriggeredAbility(
            event=event,
            predicate=predicate & IsSelf() & Source(IsTapped()),
            effect=inner,
        )
    # Delirium — At the beginning of <when> rider: the body must start
    # with the canonical Delirium intervening-if gate phrase. Strip the
    # gate clause and AND ``StateBound(Delirium())`` onto the trigger's
    # base predicate so the ability fires only when the source's
    # controller has four or more distinct card types in their graveyard
    # (CR 603.4 trigger-time check). Mirrors the Survival predicate-
    # compose precedent: no engine flag, just a composed predicate.
    # The gate is required on every ``Delirium — At the beginning of``
    # line in print today; a delirium-prefixed line whose body does NOT
    # carry the matching gate is rejected outright (same discipline as
    # Survival).
    if is_delirium:
        if not body.startswith(_DELIRIUM_AT_BEGINNING_GATE_PREFIX):
            return None
        inner_phrase = body[len(_DELIRIUM_AT_BEGINNING_GATE_PREFIX):]
        inner = _parse_effect_phrase(inner_phrase)
        if inner is None:
            return None
        return TriggeredAbility(
            event=event,
            predicate=predicate & StateBound(Delirium()),
            effect=inner,
        )
    # Tapped-creatures effect-wrapper variant: ``if you control two or
    # more tapped creatures, <inner-effect>``. Detect the prefix, parse
    # the inner-effect tail through the unified effect grammar, and
    # wrap in ``ConditionalOnTappedCreatures``. Tried BEFORE the bare
    # body parse so the more-specific shape wins. Mirrors the static-
    # registry layout exactly — the gate is an effect wrapper, NOT a
    # predicate AND on the trigger event.
    if body.startswith(_TAPPED_CREATURES_GATE_PREFIX):
        inner_phrase = body[len(_TAPPED_CREATURES_GATE_PREFIX):]
        inner = _parse_effect_phrase(inner_phrase)
        if inner is None:
            return None
        return TriggeredAbility(
            event=event, predicate=predicate,
            effect=ConditionalOnTappedCreatures(
                effect=inner, min_count=2,
            ),
        )
    # ``if you gained life this turn, untap this creature`` — Ragost,
    # Deft Gastronaut's body after name-substitution. Both the
    # "this creature" and "this permanent" surface forms collapse to the
    # same ``IfGainedLifeThisTurnUntapSelf()`` engine class that handles
    # the life-gain check and the untap in one resolve step. Tried BEFORE
    # the bare body parse so the more-specific full-body match wins.
    if body in (_IF_GAINED_LIFE_GATE_BODY, _IF_GAINED_LIFE_GATE_BODY_PERMANENT):
        return TriggeredAbility(
            event=event, predicate=predicate,
            effect=IfGainedLifeThisTurnUntapSelf(),
        )
    # Trailing-if-gained-life variant: ``<inner-effect> if you've
    # gained N or more life this turn``. Detect the suffix, parse the
    # inner effect through the unified grammar, and wrap in
    # ``ConditionalIfYouGainedLifeAtLeast(threshold=N, inner=...)``.
    # Tried BEFORE the bare body parse so the more-specific shape wins.
    # Mirrors the static-registry layout for Haliya, Guided by Light
    # exactly: the gate sits AFTER the effect text and is an effect
    # wrapper (not a predicate AND on the trigger event). The inner
    # effect parses via ``_parse_effect_phrase`` so any supported effect
    # can serve as the body; ``draw a card`` is the only EoE consumer
    # today but the grammar extension is free.
    gained_m = _GAINED_LIFE_TRAILING_IF_SUFFIX.match(body)
    if gained_m is not None:
        inner_phrase = gained_m.group(1)
        threshold = int(gained_m.group(2))
        inner = _parse_effect_phrase(inner_phrase)
        if inner is None:
            return None
        return TriggeredAbility(
            event=event, predicate=predicate,
            effect=ConditionalIfYouGainedLifeAtLeast(
                threshold=threshold, inner=inner,
            ),
        )
    # Generic intervening-if gate: ``if <gate>, <inner>``. Tried after
    # the dedicated Void / Survival / Delirium / tapped-creatures
    # branches above so those keep their bespoke handling; this
    # branch covers any other gate phrase claimed by
    # ``_INTERVENING_IF_PHRASES``. Sothera, the Supervoid's end-step
    # gate ``if a player controls no creatures`` lands here; a future
    # ``At the beginning of your end step, if <some other gate>, ...``
    # printing reuses this branch automatically once its gate phrase
    # ships in the phrasebook. An ``if ``-prefixed body whose gate
    # phrase isn't in the phrasebook is rejected outright — we never
    # silently drop a printed gate clause.
    if body.startswith("if "):
        iif_m = _AT_BEGINNING_INTERVENING_IF.match(body)
        if iif_m is not None:
            gate_phrase = iif_m.group(1)
            inner_phrase = iif_m.group(2)
            gate_pred = _parse_intervening_if_phrase(gate_phrase)
            if gate_pred is None:
                return None
            inner = _parse_effect_phrase(inner_phrase)
            if inner is None:
                return None
            return TriggeredAbility(
                event=event,
                predicate=predicate & gate_pred,
                effect=inner,
            )
    effect = _parse_effect_phrase(body)
    if effect is None:
        return None
    return TriggeredAbility(
        event=event, predicate=predicate, effect=effect,
    )


def _parse_triggered_ability_line(
    line: str, *, name: str | None = None,
) -> TriggeredAbility | tuple[TriggeredAbility, ...] | None:
    """Parse a single triggered-ability oracle line.

    Returns a single ``TriggeredAbility`` for a standard
    single-condition line, a tuple of ``TriggeredAbility`` (one per
    arm) for a compound-condition line whose phrase is claimed by
    ``_COMPOUND_PREDICATE_TRIGGER_PHRASES`` (e.g. Thawbringer's
    ``When this creature enters or dies``), or ``None`` if no shape
    matches.

    ``name``, when supplied, is the card's own printed name. Name
    references in the oracle text — both the full printed name and
    the short form (the substring before the first comma, used for
    legendary creatures) — are rewritten to the canonical
    ``"this creature"`` form before condition / effect lookup, so a
    printing whose oracle text reads ``"Whenever Haliya enters or
    attacks, …"`` flows through the same trigger-phrase table as
    the un-named ``"Whenever this creature enters or attacks, …"``
    surface. Cards that print themselves only as ``"this creature"``
    pass through unchanged (the substitution is a no-op when the
    name doesn't appear in the line).

    The clause layer (``_parse_clause``) normalizes both shapes into
    the ``triggers`` tuple on ``_ParsedClause`` so callers downstream
    don't need to special-case the compound path.
    """
    if name is not None:
        line = _substitute_self_name(line, name)
    line = _strip_ability_word_prefix(line)
    # ETB-replacement-style ``This creature enters with …`` line. This
    # surface shape doesn't carry a ``When``/``Whenever`` keyword and so
    # would never match the standard trigger-line regex below; checking
    # it up front turns the line into a ``TriggeredAbility(event=ENTERS,
    # predicate=IsSelf(), effect=...)`` mirroring the static-registry
    # convention for "enters with counters" cards (the engine doesn't
    # model replacement effects, so static and parser both express the
    # printed replacement as an ETB trigger). A failed body lookup
    # propagates as ``None`` from this branch — we don't fall through
    # to the When/Whenever paths, because an unparseable ``enters
    # with`` body has no other valid interpretation as a trigger line.
    enters_with_match = _ENTERS_WITH_REPLACEMENT_LINE.match(line)
    if enters_with_match is not None:
        effect = _parse_enters_with_body(enters_with_match.group(1))
        if effect is None:
            return None
        return TriggeredAbility(
            event=Event.ENTERS, predicate=IsSelf(), effect=effect,
        )
    # Trailing once-per-turn rider: ``<trigger sentence>. This ability
    # triggers only once each turn.`` Strip the suffix BEFORE running
    # any sentence-shape regex so the trigger body parses normally; the
    # ``once_per_turn=True`` flag is glued onto whichever
    # ``TriggeredAbility`` the body produces (intervening-if path or
    # plain-trigger path). Doing the strip up front keeps both shapes
    # supporting the rider with a single source of truth — the
    # alternative (rider-aware variants of each line regex) would
    # double the grammar without behavioral benefit.
    once_per_turn = False
    if line.endswith(_ONCE_PER_TURN_RIDER_SUFFIX):
        line = line[: -len(_ONCE_PER_TURN_RIDER_SUFFIX)]
        once_per_turn = True
    elif line.endswith(_DO_THIS_ONCE_PER_TURN_RIDER_SUFFIX):
        line = line[: -len(_DO_THIS_ONCE_PER_TURN_RIDER_SUFFIX)]
        once_per_turn = True
    # Turn-structure ``At the beginning of <when>, <effect>.`` shape.
    # Tried before the standard ``When``/``Whenever`` paths because the
    # leading word is ``At`` and the line regex below wouldn't match it
    # anyway, but ordering by surface form keeps the dispatch narrative
    # readable. The body parser handles bare and intervening-clause
    # variants in one place — if any variant claims the line, return its
    # produced ``TriggeredAbility``; otherwise fall through to the
    # When/Whenever paths below (a future ``At ...`` shape that doesn't
    # parse here would still get a chance via the bare regex if it
    # happened to fit, though no such surface exists today).
    #
    # A leading ``Void — `` prefix is detected here and stripped before
    # the regex match so the body parser can see the bare ``At the
    # beginning of …`` shape. The flag is threaded through to the body
    # parser, which sets ``requires_void=True`` on the produced
    # ``TriggeredAbility`` and elides the matching void-gate middle
    # clause from the effect body. The standard
    # ``_strip_ability_word_prefix`` helper above only strips ability-
    # word prefixes in front of ``When``/``Whenever`` (its lookahead
    # guards against accidental matches in type-line strings), so the
    # ``Void —`` strip for ``At …`` shapes lives here instead.
    at_line = line
    requires_void = False
    # Only consume the ``Void — `` prefix if it's followed by an
    # ``At the beginning of`` body — the parser still needs the
    # prefix in place for the When/Whenever Void paths handled below
    # (those go through the generic ``_strip_ability_word_prefix``
    # which has already run; in the When/Whenever case the prefix is
    # already gone by the time we get here, but if a future helper
    # rearranges that order this guard keeps the two paths from
    # stealing each other's lines).
    #
    # Two ``Void — At the beginning of …`` shapes co-exist:
    #
    # 1. The standard intervening-if Void rider (``Void — At …, if
    #    <void-gate>, <effect>``): the gate phrase appears as a
    #    middle clause, the trigger fires only while Void is active,
    #    and the produced ``TriggeredAbility`` carries
    #    ``requires_void=True``. Used by Insatiable Skittermaw,
    #    Voidforged Titan, Kavaron Skywarden, Hylderblade.
    #
    # 2. The "sacrifice unless" Void rider (``Void — At …, sacrifice
    #    this <subject> unless <void-gate>``): the gate phrase
    #    appears in an UNLESS clause as part of the effect, the
    #    trigger fires every turn, and the sacrifice itself is
    #    gated on Void via the resolving effect class
    #    (``SacrificeSelfUnlessVoidActive``). Used by Chorale of
    #    the Void.
    #
    # Per CR 207.2c the ``Void — `` ability word has no rules
    # consequence on its own; the ``requires_void=True`` flag rides
    # the BODY-side void gate phrase, not the prefix. We strip the
    # prefix in both shapes (so the ``At …`` regex matches) but only
    # set ``requires_void=True`` for shape 1, detected by the
    # canonical gate phrase appearing as a middle clause in the
    # body. Shape 2 has no leading gate so the gate-bearing branch
    # in the body parser is skipped and dispatch falls through to
    # plain effect parsing (where the
    # ``SacrificeSelfUnlessVoidActive``-bound recognizer claims the
    # body).
    if (
        at_line.startswith(_VOID_PREFIX)
        and at_line[len(_VOID_PREFIX):].startswith("At the beginning of ")
    ):
        at_line = at_line[len(_VOID_PREFIX):]
        # Set ``requires_void`` only when the body carries the
        # canonical middle-clause Void gate. Without this guard, the
        # "sacrifice unless" surface (Chorale) would route through
        # the gate-required path and reject for missing the gate
        # prefix at the start of the body.
        requires_void = _VOID_GATE_PREFIX in at_line
    # ``Survival — `` ability-word prefix on an ``At the beginning of``
    # body. Strip the prefix and thread the flag through to the body
    # parser, which will then expect the canonical ``if this creature
    # is tapped, `` middle clause and AND
    # ``IsSelf() & IsTapped()`` onto the trigger predicate. Mirrors
    # the ``Void —`` strip above structurally — both prefixes attach
    # to a turn-structure trigger and modify how the body parses, but
    # ``Survival`` lands its gate as a predicate (no engine flag) while
    # ``Void`` sets the ``requires_void`` flag.
    is_survival = False
    if (
        at_line.startswith(_SURVIVAL_PREFIX)
        and at_line[len(_SURVIVAL_PREFIX):].startswith("At the beginning of ")
    ):
        at_line = at_line[len(_SURVIVAL_PREFIX):]
        is_survival = True
    # ``Delirium — `` ability-word prefix on an ``At the beginning of``
    # body. Strip the prefix and thread the flag through to the body
    # parser, which will then expect the canonical Delirium gate clause
    # ``_DELIRIUM_AT_BEGINNING_GATE_PREFIX`` and AND
    # ``StateBound(Delirium())`` onto the trigger predicate. Mirrors
    # the ``Survival —`` strip above structurally — both prefixes land
    # their gates as predicate components (no engine flag). The
    # ``_strip_ability_word_prefix`` lookahead is ``(?=When |Whenever )``
    # so ``Delirium — At the beginning of`` doesn't flow through it;
    # detecting the prefix explicitly here keeps the strip narrow.
    is_delirium = False
    if (
        at_line.startswith(_DELIRIUM_PREFIX)
        and at_line[len(_DELIRIUM_PREFIX):].startswith("At the beginning of ")
    ):
        at_line = at_line[len(_DELIRIUM_PREFIX):]
        is_delirium = True
    at_match = _AT_THE_BEGINNING_OF_LINE.match(at_line)
    if at_match is not None:
        parsed = _parse_at_the_beginning_of_line(
            at_match,
            requires_void=requires_void,
            is_survival=is_survival,
            is_delirium=is_delirium,
        )
        if parsed is not None:
            return parsed
        return None
    # ``When <condition> and whenever <condition>, <effect>.`` -- one
    # ability printed with two trigger conditions (Orcish Bowmasters'
    # "When this creature enters and whenever an opponent draws a card
    # ..."). Each condition becomes its own ``TriggeredAbility`` arm
    # sharing the effect, like the compound "enters or dies" table.
    both = _WHEN_AND_WHENEVER_LINE.match(line)
    if both is not None:
        effect = _parse_effect_phrase(both.group(3))
        if effect is None:
            return None
        arms = []
        for condition in (both.group(1), both.group(2)):
            pred_match = _parse_predicate_trigger_phrase(condition)
            if pred_match is None:
                return None
            from mtgz_ir.cards.parser.cube_creatures_ub import (
                bind_event_player_anaphora,
            )

            arms.append(TriggeredAbility(
                event=pred_match[0],
                predicate=pred_match[1],
                effect=bind_event_player_anaphora(pred_match[0], effect),
                once_per_turn=once_per_turn,
            ))
        return tuple(arms)
    # Intervening-if shape (CR 603.4) tried first: a recognized gate
    # phrase between the trigger condition and the effect collapses to
    # a single ``TriggeredAbility`` with the gate predicate AND-ed onto
    # the event's base predicate. If the gate phrase isn't recognized
    # the line is rejected outright (we don't fall back to the
    # no-intervening-if shape because the surface text would then
    # silently lose its gate, producing an over-firing trigger). This
    # matters: an unrecognized gate is a parser failure to claim the
    # card, not a license to drop the gate.
    iif_match = _TRIGGERED_ABILITY_INTERVENING_IF_LINE.match(line)
    if iif_match is not None:
        condition = iif_match.group(1)
        gate_phrase = iif_match.group(2)
        gate_pred = _parse_intervening_if_phrase(gate_phrase)
        if gate_pred is not None:
            effect = _parse_effect_phrase(iif_match.group(3))
            if effect is None:
                return None
            # Only the predicate-style trigger path supports an AND-able
            # gate predicate; the legacy string-key path can't carry a
            # predicate at all, so a gated trigger must resolve to an
            # event that's already on the predicate side. The two table
            # lookups below preserve the same precedence as the no-gate
            # path; if only the legacy string-key table claims the
            # condition phrase we decline rather than silently drop the
            # gate.
            pred_match = _parse_predicate_trigger_phrase(condition)
            if pred_match is None:
                return None
            event, base_predicate = pred_match
            phrase_once = _predicate_trigger_phrase_is_once_per_turn(
                condition,
            )
            return TriggeredAbility(
                event=event,
                predicate=base_predicate & gate_pred,
                effect=effect,
                once_per_turn=once_per_turn or phrase_once,
            )
        # Effect-wrapper gate fallback: the ``if`` clause isn't a
        # predicate AND but instead wraps the resolved inner effect at
        # resolve time (e.g. Weftwalking's ``if you cast it`` gate maps
        # to ``ConditionalIfCastFromHand``). Mirrors the parallel
        # ``while`` path's effect-wrapper fallback. Tried AFTER the
        # predicate-gate path so explicit predicate gates win; when the
        # gate phrase isn't in either phrasebook the line is rejected
        # outright (same don't-silently-drop-the-gate discipline as the
        # predicate path).
        wrapper_factory = _parse_intervening_if_effect_wrapper(gate_phrase)
        if wrapper_factory is None:
            return None
        inner_effect = _parse_effect_phrase(iif_match.group(3))
        if inner_effect is None:
            return None
        pred_match = _parse_predicate_trigger_phrase(condition)
        if pred_match is None:
            return None
        event, base_predicate = pred_match
        phrase_once = _predicate_trigger_phrase_is_once_per_turn(condition)
        return TriggeredAbility(
            event=event,
            predicate=base_predicate,
            effect=wrapper_factory(inner_effect),
            once_per_turn=once_per_turn or phrase_once,
        )
    # Intervening-``while`` shape: ``When/Whenever <condition> while
    # <gate>, <effect>.`` Same trigger-time gate semantics as the
    # middle-clause intervening-if path (CR 603.4) — same predicate
    # AND-ed onto the same base trigger predicate — but the printed
    # surface text uses ``while`` instead of ``, if``. Hand That
    # Feeds is the canonical example. Tried AFTER the ``, if `` regex
    # so a line carrying both shapes (none today) claims via the
    # more-explicit form first; an unrecognized while-gate rejects
    # outright (same don't-silently-drop-the-gate discipline as
    # the ``, if `` path).
    iwhile_match = _TRIGGERED_ABILITY_INTERVENING_WHILE_LINE.match(line)
    if iwhile_match is not None:
        condition = iwhile_match.group(1)
        gate_phrase = iwhile_match.group(2)
        gate_pred = _parse_intervening_while_phrase(gate_phrase)
        if gate_pred is not None:
            effect = _parse_effect_phrase(iwhile_match.group(3))
            if effect is None:
                return None
            pred_match = _parse_predicate_trigger_phrase(condition)
            if pred_match is None:
                return None
            event, base_predicate = pred_match
            phrase_once = _predicate_trigger_phrase_is_once_per_turn(condition)
            return TriggeredAbility(
                event=event,
                predicate=base_predicate & gate_pred,
                effect=effect,
                once_per_turn=once_per_turn or phrase_once,
            )
        # Effect-wrapper gate: the ``while`` clause isn't a predicate
        # AND but instead wraps the resolved inner effect at resolve time
        # (e.g. Syr Vondam's "while its power is 4 or greater" gate maps
        # to ``ConditionalIfSourcePowerAtLeast``). Tried AFTER the
        # predicate-gate path so explicit predicate gates win; when the
        # gate phrase isn't in either phrasebook the line is rejected
        # outright (same don't-silently-drop-the-gate discipline as the
        # predicate path).
        wrapper_factory = _parse_intervening_while_effect_wrapper(gate_phrase)
        if wrapper_factory is None:
            return None
        inner_effect = _parse_effect_phrase(iwhile_match.group(3))
        if inner_effect is None:
            return None
        pred_match = _parse_predicate_trigger_phrase(condition)
        if pred_match is None:
            return None
        event, base_predicate = pred_match
        phrase_once = _predicate_trigger_phrase_is_once_per_turn(condition)
        return TriggeredAbility(
            event=event,
            predicate=base_predicate,
            effect=wrapper_factory(inner_effect),
            once_per_turn=once_per_turn or phrase_once,
        )
    # Trailing-if shape: ``When/Whenever <condition>, <effect> if
    # <gate>.`` Mechanically equivalent to the middle-clause
    # intervening-if path above — same predicate AND-ed onto the same
    # base trigger predicate. The Delirium "tail-rider" surface
    # (``... if you have four or more card types among cards in your
    # graveyard.``) is the canonical example. Tried AFTER the middle-
    # clause path so a line carrying both forms claims the more-
    # specific middle shape first; an unrecognized trailing gate
    # rejects the whole line (same don't-silently-drop-the-gate
    # discipline as the middle path).
    tif_match = _TRIGGERED_ABILITY_TRAILING_IF_LINE.match(line)
    if tif_match is not None:
        condition = tif_match.group(1)
        effect_phrase = tif_match.group(2)
        gate_phrase = tif_match.group(3)
        gate_pred = _parse_trailing_if_phrase(gate_phrase)
        if gate_pred is not None:
            effect = _parse_effect_phrase(effect_phrase)
            if effect is None:
                return None
            pred_match = _parse_predicate_trigger_phrase(condition)
            if pred_match is None:
                return None
            event, base_predicate = pred_match
            phrase_once = _predicate_trigger_phrase_is_once_per_turn(
                condition,
            )
            return TriggeredAbility(
                event=event,
                predicate=base_predicate & gate_pred,
                effect=effect,
                once_per_turn=once_per_turn or phrase_once,
            )
        # The trailing-if regex over-matches: any trigger line whose
        # effect contains a sub-clause beginning `` if `` (e.g. an ``if
        # you do, …`` rider on a future printing) will land here even
        # though the trailing-``if`` interpretation isn't intended. If
        # the gate phrase isn't claimed by the trailing-if phrasebook
        # we fall through to the bare ``_TRIGGERED_ABILITY_LINE``
        # parse below rather than rejecting outright — distinct from
        # the middle-clause path's reject-on-unknown-gate discipline,
        # because the middle-clause regex anchors on `` if `` flanked
        # by `` , `` boundaries that are far less likely to appear
        # incidentally inside an effect body.
    m = _TRIGGERED_ABILITY_LINE.match(line)
    if m is None:
        return None
    condition = m.group(1)
    effect = _parse_effect_phrase(m.group(2))
    if effect is None:
        return None
    # Context-dependent effect rewrite for "draw that many cards":
    # the unified ``_DRAW_THAT_MANY_CARDS`` regex always returns
    # ``DrawCardsEqualToResolvingCombatDamage`` because that's the
    # most common surface in the EoE pool. When the surrounding
    # trigger condition is a COUNTERS_ADDED phrase (Terrasymbiosis's
    # "you put one or more +1/+1 counters on a creature you control,
    # you may draw that many cards"), rewrite the effect to the
    # counter-amount sibling so it reads
    # ``state.resolving_counter_amount`` instead of
    # ``state.resolving_combat_damage_amount`` at fire time. The
    # rewrite is local to the predicate-trigger dispatch — no other
    # ``draw that many cards`` surface in the pool needs the swap.
    # Future printings whose condition phrase parses to COUNTERS_ADDED
    # get the same routing for free.
    pred_match_for_rewrite = _parse_predicate_trigger_phrase(condition)
    if (
        pred_match_for_rewrite is not None
        and pred_match_for_rewrite[0] == Event.COUNTERS_ADDED
        and isinstance(effect, DrawCardsEqualToResolvingCombatDamage)
    ):
        effect = DrawCardsEqualToResolvingCounterAmount()
    # Context-dependent effect rewrite for ``"a creature you control
    # attacks alone"`` triggers: the printed body's anaphoric ``"it
    # gets +N/+M and gains <kw> until end of turn"`` refers to the
    # lone attacker (not the source). The unified effect grammar
    # strips the leading ``"it "`` and lands on
    # ``PumpSelfUntilEOT`` (which pumps the source — wrong, since
    # the source is the Room/permanent, not the attacker). Rewrite
    # to ``PumpTargetCreatureUntilEOT(attacking_only=True,
    # you_control_only=True)`` whose legal-target pool collapses to
    # the lone attacker — the predicate's
    # ``StateBound(LoneAttackerForYou())`` already guarantees
    # exactly one creature is attacking, so the targeted pump
    # trivially picks it. Same context-rewrite precedent as the
    # COUNTERS_ADDED → DrawCardsEqualToResolvingCounterAmount swap
    # above. Future printings whose condition phrase reads
    # ``"a creature you control attacks alone"`` get the same
    # routing for free.
    if (
        condition == "a creature you control attacks alone"
        and isinstance(effect, PumpSelfUntilEOT)
    ):
        effect = PumpTargetCreatureUntilEOT(
            power_bonus=effect.power_bonus,
            toughness_bonus=effect.toughness_bonus,
            granted_keywords=effect.granted_keywords,
            attacking_only=True,
            you_control_only=True,
        )
    # Compound predicate-trigger lookup wins when the condition phrase
    # is claimed by the multi-arm table (e.g. ``"this creature enters
    # or dies"`` → two ``(Event, Predicate)`` arms). The single-arm
    # tables below are consulted only if the compound lookup misses,
    # so a future addition that overlaps a compound and a single
    # phrase resolves predictably to the compound shape — matching the
    # surface-text intent ("X or Y" should fire on both events, not
    # silently collapse to one).
    compound_match = _parse_compound_predicate_trigger_phrase(condition)
    if compound_match is not None:
        return tuple(
            TriggeredAbility(event=event, predicate=predicate, effect=effect)
            for event, predicate in compound_match
        )
    # Collapsed-arm predicate-trigger lookup: the condition reads as
    # ``X or Y`` but the two arms share an event AND collapse onto a
    # single predicate (no ``~IsSelf()`` — the wording explicitly
    # includes self). Produces ONE ``TriggeredAbility`` whose
    # predicate covers both arms, mirroring the static registry's
    # single-entry layout for cards like Susurian Voidborn. Tried
    # AFTER the multi-arm compound table (so a phrase that wants
    # two separate abilities still gets them) and BEFORE the single
    # phrase table (so the inclusive wording wins over an
    # accidentally-overlapping single-arm match).
    collapsed_match = _parse_collapsed_predicate_trigger_phrase(condition)
    if collapsed_match is not None:
        event, predicate = collapsed_match
        return TriggeredAbility(
            event=event,
            predicate=predicate,
            effect=effect,
            once_per_turn=once_per_turn,
        )
    # Predicate-style trigger lookup wins when the condition phrase is
    # claimed by both tables — phase 2 only registers ENTERS-family
    # phrases on the predicate side, so today the two tables are
    # disjoint, but precedence here makes the migration order safe to
    # extend in subsequent phases.
    pred_match = _parse_predicate_trigger_phrase(condition)
    if pred_match is not None:
        event, predicate = pred_match
        phrase_once = _predicate_trigger_phrase_is_once_per_turn(condition)
        from mtgz_ir.cards.parser.cube_creatures_ub import (
            bind_event_player_anaphora,
        )

        return TriggeredAbility(
            event=event,
            predicate=predicate,
            effect=bind_event_player_anaphora(event, effect),
            once_per_turn=once_per_turn or phrase_once,
        )
    trigger = _parse_trigger_phrase(condition)
    if trigger is None:
        return None
    return TriggeredAbility(
        trigger=trigger, effect=effect, once_per_turn=once_per_turn,
    )
