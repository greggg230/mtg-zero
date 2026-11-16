"""Enchantment card builders for Edge of Eternities + Duskmourn.

Every claim flows through one of two oracle-text grammar paths — there
are no remaining name-keyed bespoke factories:
- ``_build_aura_via_oracle_text`` for Enchantment — Aura cards
  (gated by ``_UNIFIED_AURA_NAMES``).
- ``_build_enchantment_via_oracle_text`` for non-Aura enchantments
  (gated by ``_UNIFIED_ENCHANTMENT_NAMES``).

The historical ``_ENCHANTMENT_BUILDERS`` dispatch table and its 21
``_build_*`` factory functions were retired on 2026-05-01 — every
card those factories used to claim now claims through the unified
oracle-text path or via Scryfall set-code-keyed dispatch elsewhere.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.base import (
    CardPrinting,
    TriggeredAbility,
    make_aura,
    make_enchantment,
    parse_mana_cost,
)
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects import (
    CreateToken,
    DestroyEnchantedPermanent,
    EnchantedCreatureFightsTargetEffect,
    ExileOpponentsNonlandPermanentUntilLeaves,
    ExileTargetCreatureUntilSourceLeaves,
    PainForAllDamageEchoEffect,
    PainForAllETBEffect,
    ReanimateTargetCreatureCardFromDefendersGraveyard,
    SacrificeSelfUnlessVoidActive,
    TapEnchantedPermanent,
    TargetOpponentLookHandMayDiscard,
    TargetOpponentSacrificesCreatureOtherThanEnchanted,
    TractorBeamETBEffect,
)
from mtgz_ir.effects.base import StaticAbility, StaticScope
from mtgz_ir.effects.modifiers import (
    EventPattern,
    MultiplyCount,
    ReplacementEffect,
)
from mtgz_ir.effects.object_predicates import (
    IsArtifact,
    IsCreature,
    IsLand,
    IsNontoken,
)
from mtgz_ir.effects.predicates import (
    YOU,
    Controlled,
    Event,
    IsHost,
    IsSelf,
    Subject,
)
from mtgz_ir.cards.parser.printing_keywords import (
    _parse_restricts_noncreature_spells_line,
    _parse_ward_line,
    _parse_warp_line,
)
from mtgz_ir.cards.parser.keywords import _parse_keyword_line
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.triggers import _parse_triggered_ability_line


# ---------------------------------------------------------------------------
# Aura oracle-text parser
# ---------------------------------------------------------------------------
#
# Each of the recognizers below handles ONE printed surface shape found on
# Aura oracle lines. They are tried in order by ``_build_aura_via_oracle_
# text``; any unrecognized line causes the card to be rejected (returns
# None) so the silent-skip discipline stays intact.
#
# ``"Enchant X"`` line → (aura_enchant_type, aura_yours_only) --------
#
# Four printed shapes in the EoE/DSK pool:
#
#   "Enchant creature"                       → ("creature", False)
#   "Enchant creature you control"           → ("creature", True)
#   "Enchant artifact you control"           → ("artifact", True)
#   "Enchant creature or Spacecraft"         → ("creature_or_spacecraft", False)
#   "Enchant artifact or creature you control" → ("artifact_or_creature", True)
#      (mtg-zero docs/ir_defects.md 29, 30: a Spacecraft below its station
#      threshold is a noncreature artifact, still legal to enchant.)
#
_ENCHANT_LINE = re.compile(
    r"^Enchant "
    r"(?:(creature you control)"          # group 1: yours-only creature
    r"|(artifact you control)"            # group 2: yours-only artifact
    r"|(creature or Spacecraft)"          # group 3: creature-or-spacecraft
    r"|(artifact or creature you control)"# group 4: artifact-or-creature yours
    r"|(creature)"                        # group 5: plain creature
    r")$"
)


def _parse_enchant_line(
    line: str,
) -> tuple[str, bool] | None:
    """Return ``(aura_enchant_type, aura_yours_only)`` for a recognized
    ``Enchant X`` line, else ``None``.

    The two engine parameters drive the cast-time target picker
    (``actions._select_spell_targets``). Simplifications for compound
    enchant targets ("creature or Spacecraft", "artifact or creature
    you control") are documented on the ``_ENCHANT_LINE`` regex comment.
    """
    m = _ENCHANT_LINE.match(line.strip())
    if m is None:
        return None
    if m.group(1):   # "creature you control"
        return ("creature", True)
    if m.group(2):   # "artifact you control"
        return ("artifact", True)
    if m.group(3):   # "creature or Spacecraft"
        return ("creature_or_spacecraft", False)
    if m.group(4):   # "artifact or creature you control"
        return ("artifact_or_creature", True)
    # m.group(5) — plain "creature"
    return ("creature", False)


# ``"Enchanted (creature|permanent) gets ±N/±M[ and has/loses ...]"``
# line recognizer. Maps to ``power_bonus``, ``toughness_bonus``,
# ``granted_keywords``, ``attached_grants_lose_all_abilities``, and
# ``attached_grants_ward_str`` on ``make_aura``.
#
# The P/T part uses signed integer notation: ``+2/+2``, ``-5/-0``,
# ``+1/+0``, ``-3/-0``. The optional rider is one of:
#
#   and has <keyword>[, keyword]*[ and ward {cost}]
#   and has <keyword> and <keyword>[ and ward {cost}]
#   and can't be blocked by more than one creature
#   and loses all abilities
#
# The ``and loses all abilities`` form also sets
# ``attached_grants_lose_all_abilities=True``; it may appear alone or
# paired with a P/T modifier (Duskmourn's Domination has ``-3/-0 and
# loses all abilities``).
_ENCHANTED_GETS_RE = re.compile(
    r"^Enchanted (?:creature|permanent) gets "
    r"([+-]\d+)/([+-]\d+)"               # groups 1+2: power / toughness
    r"(?: and (.+))?$"                   # group 3: optional rider (no trailing period)
)
# Rider: "loses all abilities" → attached_grants_lose_all_abilities
_RIDER_LOSES_ALL_ABILITIES = "loses all abilities"
# Rider: "can't be blocked by more than one creature" → a blocking
# restriction (CR 509.1b; not menace, its opposite; mtg-zero
# docs/ir_defects.md 28).
_RIDER_MAX_ONE_BLOCKER = "can't be blocked by more than one creature"
# Ward in "has ... and ward {cost}" rider
_RIDER_WARD_RE = re.compile(r"^(.*?) and (ward \{[^}]+\})$")
# "has <keywords>" rider prefix
_RIDER_HAS_PREFIX = "has "


def _parse_enchanted_gets_line(
    line: str,
) -> (
    "tuple[int, int, frozenset[str], bool, str | None, bool] | None"
):
    """Parse an ``Enchanted (creature|permanent) gets ±N/±M ...`` line.

    Returns ``(power_bonus, toughness_bonus, granted_keywords,
    lose_all_abilities, ward_cost_str_or_None, max_one_blocker)`` on
    success, else ``None``.

    Handles the following rider shapes (after the P/T modifier):
    - No rider → zero bonuses, empty keywords, no lose-all, no ward.
    - ``and has <keyword>`` → granted keyword.
    - ``and has <kw1> and <kw2>`` → two granted keywords.
    - ``and has <kw> and ward {cost}`` → keyword + ward grant.
    - ``and can't be blocked by more than one creature`` → max one
      blocker.
    - ``and loses all abilities`` → lose-all flag.
    """
    line = line.rstrip(".")
    m = _ENCHANTED_GETS_RE.match(line)
    if m is None:
        return None
    power_bonus = int(m.group(1))
    toughness_bonus = int(m.group(2))
    rider = m.group(3)  # None if no rider

    granted: set[str] = set()
    lose_all = False
    ward_cost: str | None = None

    if rider is None:
        return (power_bonus, toughness_bonus, frozenset(granted),
                lose_all, ward_cost, False)

    # "loses all abilities" rider (may stand alone)
    if rider == _RIDER_LOSES_ALL_ABILITIES:
        return (power_bonus, toughness_bonus, frozenset(granted),
                True, ward_cost, False)

    # "can't be blocked by more than one creature"
    if rider == _RIDER_MAX_ONE_BLOCKER:
        return (power_bonus, toughness_bonus, frozenset(granted),
                lose_all, ward_cost, True)

    # "has ..." rider — strip the "has " prefix then parse keywords
    if not rider.startswith(_RIDER_HAS_PREFIX):
        return None
    keyword_part = rider[len(_RIDER_HAS_PREFIX):]

    # Check for "... and ward {cost}" suffix
    ward_m = _RIDER_WARD_RE.match(keyword_part)
    if ward_m:
        keyword_part = ward_m.group(1)
        ward_line = ward_m.group(2)  # "ward {cost}"
        # Parse "ward {cost}" via the standard ward-line parser but
        # capitalize for it to match "Ward {cost}" form.
        ward_parsed = _parse_ward_line(ward_line.capitalize())
        if ward_parsed is None:
            return None
        ward_cost = ward_parsed

    # Split remaining keyword_part on " and " to handle multi-keyword
    kw_pieces = [p.strip() for p in keyword_part.split(" and ")]
    for kw in kw_pieces:
        # Normalize first letter to upper case for _parse_keyword_line
        kw_cap = kw[:1].upper() + kw[1:] if kw else kw
        parsed = _parse_keyword_line(kw_cap)
        if parsed is None:
            return None
        granted.update(parsed)

    return (power_bonus, toughness_bonus, frozenset(granted),
            lose_all, ward_cost, False)


# Aura-level static-flag lines -----------------------------------------

# "Enchanted (creature|permanent) has ward {cost}." (standalone ward grant,
# no P/T modifier) → attached_grants_ward.
# Hardlight Containment prints "Enchanted permanent has ward {1}." as its
# own oracle line. Distinct from the "and ward {cost}" inline rider on a
# "gets +N/+M and has ... and ward {cost}" line; the inline rider is parsed
# by ``_parse_enchanted_gets_line``'s ``_RIDER_WARD_RE`` logic, while this
# standalone form is its own line recognizer. Both feed the same
# ``attached_grants_ward`` kwarg on ``make_aura``.
_ENCHANTED_HAS_WARD_LINE = re.compile(
    r"^Enchanted (?:creature|permanent) has "
    r"(?:ward (\{[^}]+\}))\.$"
)
# "You control enchanted creature/permanent." → attached_grants_control
_YOU_CONTROL_ENCHANTED_LINE = re.compile(
    r"^You control enchanted (?:creature|permanent)\.$"
)
# "Enchanted creature/permanent doesn't untap during its controller's
# untap step." → attached_prevents_untap
_DOESNT_UNTAP_LINE = re.compile(
    r"^Enchanted (?:creature|permanent) doesn't untap during its "
    r"controller's untap step\.$"
)
# "Enchanted creature/permanent loses all abilities." (standalone, no P/T)
_LOSES_ALL_ABILITIES_LINE = re.compile(
    r"^Enchanted (?:creature|permanent) loses all abilities\.$"
)
# "The first time you would create one or more tokens each turn, you may
# instead create that many tokens that are copies of enchanted permanent."
# → replaces_first_tokens_with_copies=True (Moonlit Meditation). Literal
# — only card in the pool with this surface today; deferred until a second
# example with this shape exists.
_REPLACES_FIRST_TOKENS_LINE = (
    "The first time you would create one or more tokens each turn, "
    "you may instead create that many tokens that are copies of "
    "enchanted permanent."
)
# Honest no-op lines that appear on migrated Aura cards but map to
# engine-unenforced metadata or unoffered options. Each is recognized and
# silently dropped so the card can still claim its other lines.
#
# "As long as this Aura entered this turn, enchanted creature has
# hexproof." (Shardmage's Rescue) — hexproof is unenforced metadata per
# rooms-batch6; the conditional grant is likewise a no-op.
#
# Activated-ability lines beginning with "{cost}:" that are dropped as
# honest no-ops (Stay Hidden's shuffle-and-manifest, etc.) — matched
# by the cost-prefix pattern below.
_AURA_HONEST_NOOP_LINES = frozenset({
    # Shardmage's Rescue: conditional hexproof (no-op per rooms-batch6)
    "As long as this Aura entered this turn, enchanted creature "
    "has hexproof.",
})

# Activated abilities on Auras that are dropped as honest no-ops
# (the option isn't offered via ``legal.py``; same permission-not-offered
# shape Lecture Hall ships per dsk-rooms-batch6).
_ACTIVATED_ABILITY_PREFIX = re.compile(r"^\{[^}]+\}.*:.*$")

# "When enchanted creature becomes tapped or is dealt damage, destroy it."
# (Cryoshatter) — compound trigger line: produces TWO TriggeredAbility
# entries (one BECOMES_TAPPED + IsHost(), one DEALT_DAMAGE + IsHost()) both
# with DestroyEnchantedPermanent(). Only Cryoshatter in the pool has this
# exact surface today; deferred generalization until a second card with a
# compound Aura-trigger line (becomes-tapped-or-dealt-damage → destroy)
# appears.
_CRYOSHATTER_COMPOUND_LINE = (
    "When enchanted creature becomes tapped or is dealt damage, "
    "destroy it."
)

# Historical note: a literal-trigger-line dispatch table
# (``_AURA_LITERAL_TRIGGER_LINES``) used to live here for Aura trigger
# surfaces that didn't yet flow through the unified
# ``_parse_triggered_ability_line`` grammar. Every entry was migrated
# to an Aura-context effect-phrase recognizer in
# ``mtgz_ir.cards.parser.effects._parse_effect`` paired with the
# matching condition phrase already in ``_PREDICATE_TRIGGER_PHRASES``.
# The migrated bodies → engine class:
#
# - ``destroy it`` → ``DestroyEnchantedPermanent`` (Cryoshatter +
#   Cracked Skull + Sporogenic Infection)
# - ``enchanted creature fights up to one target creature an opponent
#   controls`` → ``EnchantedCreatureFightsTargetEffect`` (Meltstrider's
#   Resolve)
# - ``enchanted creature deals damage equal to its power to any other
#   target`` → ``PainForAllETBEffect``
# - ``deals that much damage to each opponent`` →
#   ``PainForAllDamageEchoEffect``
# - ``look at target player's hand. You may choose a nonland card
#   from it. That player discards that card`` →
#   ``TargetOpponentLookHandMayDiscard`` (Cracked Skull)
# - ``target player sacrifices a creature of their choice other than
#   enchanted creature`` →
#   ``TargetOpponentSacrificesCreatureOtherThanEnchanted``
#   (Sporogenic Infection)
# - ``sacrifice this Aura unless a nonland permanent left the
#   battlefield this turn or a spell was warped this turn`` →
#   ``SacrificeSelfUnlessVoidActive`` (Chorale of the Void's "Void —"
#   end-step rider; per CR 207.2c the ``Void — `` ability-word prefix
#   is informative-only, and the audit reworked the void-prefix
#   handling in ``triggers.py`` to set ``requires_void=True`` only
#   when the body carries the canonical middle-clause Void gate).
#
# Chorale of the Void — fully decomposed in audit-monoist-chorale.
# The attacks-trigger body is claimed by a literal multi-sentence
# recognizer in ``effects._parse_effect`` mapping to the generic
# ``ReanimateTargetCreatureCardFromDefendersGraveyard`` primitive;
# the trigger header (``Whenever enchanted creature attacks, …``)
# parses via the unified ``_parse_triggered_ability_line`` grammar.
#
# 2-player simplification is preserved on the engine primitive
# (legal-target pool = any opponent's graveyard creature card),
# matching the printed "defending player" exactly in 2-player and
# approximating it in multi-defender / multiplayer / planeswalker-
# defender contexts. When the engine grows a per-attacker
# defending-player slot (state.resolving_combat_target_player or
# similar), the primitive's ``legal_targets`` will narrow to that
# specific player without an API change.

# Allowlist of names that may claim through the Aura oracle-text path.
# Mirrors ``_UNIFIED_ENCHANTMENT_NAMES`` — explicit opt-in per name.
# Adding a name here routes it through ``_build_aura_via_oracle_text``.
_UNIFIED_AURA_NAMES: frozenset[str] = frozenset({
    # EoE Auras — high priority (simple grammar)
    "Cryoshatter",        # -5/-0 + compound becomes-tapped-or-dealt-damage trigger
    "Frantic Strength",   # Flash + Enchant creature + +2/+2 + trample
    "Hardlight Containment",  # Enchant artifact you control + ETB exile + ward {1}
    "Meltstrider's Resolve",  # Enchant creature you control + ETB fights + +0/+2 + menace
    "Pain for All",       # Enchant creature you control + ETB deals damage + echo
    # DSK Auras — high priority
    "Cracked Skull",       # ETB look-at-hand-may-discard + dealt-damage-destroy
    "Duskmourn's Domination",  # Enchant creature + control + -3/-0 + lose-all
    "Shardmage's Rescue",  # Flash + Enchant creature you control + hexproof(noop) + +1/+1
    "Sheltered by Ghosts", # Enchant creature you control + ETB exile + +1/+0 + lifelink + ward {2}
    "Sporogenic Infection",  # ETB sac + dealt-damage-destroy
    "Stay Hidden, Stay Silent",  # ETB tap + doesn't-untap + activated(noop)
    # EoE Auras — medium priority
    "Moonlit Meditation",  # Enchant artifact or creature you control + token-replace
    "Tractor Beam",        # Enchant creature or Spacecraft + ETB tap + control + doesn't-untap
    # Chorale of the Void — attacks trigger
    # (ReanimateTargetCreatureCardFromDefendersGraveyard via literal
    # multi-sentence recognizer in effects._parse_effect, audit-monoist-
    # chorale) + Void end-step rider (SacrificeSelfUnlessVoidActive
    # via the unified intervening-if grammar).
    "Chorale of the Void",
})


def _build_aura_via_oracle_text(card: dict) -> CardPrinting | None:
    """Claim an Enchantment — Aura card whose oracle text parses through
    the recognized Aura line grammar.

    Gated on ``_UNIFIED_AURA_NAMES`` so only explicitly migrated cards
    can reach this path. Returns ``None`` for any card with an
    unrecognized line so the silent-skip discipline stays intact.

    The parser walks the oracle text line by line and accumulates:
    - ``keywords`` — keyword abilities on the Aura itself (Flash).
    - ``aura_enchant_type``, ``aura_yours_only`` — from the "Enchant X"
      line.
    - ``power_bonus``, ``toughness_bonus``, ``granted_keywords`` — from
      "Enchanted creature gets ±N/±M[ and has ...]" static lines.
    - ``attached_grants_control`` — from "You control enchanted
      creature/permanent." lines.
    - ``attached_prevents_untap`` — from the "doesn't untap during its
      controller's untap step." line.
    - ``attached_grants_lose_all_abilities`` — from "Enchanted creature
      loses all abilities." standalone lines or the "and loses all
      abilities" rider on a "gets ±N/±M" line.
    - ``replaces_first_tokens_with_copies`` — from the Moonlit Meditation
      literal line.
    - ``attached_grants_ward_str`` — from the "ward {cost}" inline rider
      on "Enchanted creature gets ... and has ... and ward {cost}." lines.
    - ``triggered_abilities`` — from triggered-ability lines parsed
      through ``_parse_triggered_ability_line`` plus the Cryoshatter
      compound line (two triggers from one surface line).

    Lines that are honest no-ops (conditional hexproof, unoffered
    activated abilities) are recognized and silently dropped; any
    line that doesn't fit a recognized shape causes the card to be
    rejected outright (returns ``None``).
    """
    name = card.get("name", "")
    if name not in _UNIFIED_AURA_NAMES:
        return None

    text = card.get("oracle_text", "")

    # Aura fields accumulated across lines
    aura_keywords: set[str] = set()
    aura_enchant_type: str = "creature"
    aura_yours_only: bool = False
    power_bonus: int = 0
    toughness_bonus: int = 0
    granted_keywords: set[str] = set()
    attached_grants_control: bool = False
    attached_prevents_untap: bool = False
    attached_grants_lose_all_abilities: bool = False
    attached_cant_be_blocked_by_more_than_one: bool = False
    replaces_first_tokens_with_copies: bool = False
    attached_grants_ward_str: str | None = None
    triggers: list[TriggeredAbility] = []

    for raw_line in text.split("\n"):
        line = _strip_reminder_text(raw_line)
        if not line:
            continue

        # ----------------------------------------------------------------
        # Honest no-op lines — recognized and dropped
        # ----------------------------------------------------------------
        if line in _AURA_HONEST_NOOP_LINES:
            continue
        # Activated-ability lines (cost: effect) dropped as unoffered
        # optional actions (same permission-not-offered shape Lecture Hall
        # ships per dsk-rooms-batch6).
        if _ACTIVATED_ABILITY_PREFIX.match(line):
            continue

        # ----------------------------------------------------------------
        # Keyword lines on the Aura itself (Flash, etc.)
        # ----------------------------------------------------------------
        kw_parsed = _parse_keyword_line(line)
        if kw_parsed is not None:
            aura_keywords.update(kw_parsed)
            continue

        # ----------------------------------------------------------------
        # "Enchant X" line → aura_enchant_type + aura_yours_only
        # ----------------------------------------------------------------
        enchant_parsed = _parse_enchant_line(line)
        if enchant_parsed is not None:
            aura_enchant_type, aura_yours_only = enchant_parsed
            continue

        # ----------------------------------------------------------------
        # Cryoshatter compound trigger: two events, one oracle line.
        # "When enchanted creature becomes tapped or is dealt damage,
        # destroy it." → BECOMES_TAPPED + IsHost() and
        #                 DEALT_DAMAGE + IsHost() both with
        #                 DestroyEnchantedPermanent.
        # Checked BEFORE the standard triggered-ability parser so this
        # specialized compound line wins over the generic single-trigger
        # path (which would fail to parse the compound condition phrase
        # anyway — the standard parser doesn't recognize "becomes tapped
        # or is dealt damage" as a compound event).
        # ----------------------------------------------------------------
        if line == _CRYOSHATTER_COMPOUND_LINE:
            triggers.append(TriggeredAbility(
                event=Event.BECOMES_TAPPED,
                predicate=IsHost(),
                effect=DestroyEnchantedPermanent(),
            ))
            triggers.append(TriggeredAbility(
                event=Event.DEALT_DAMAGE,
                predicate=IsHost(),
                effect=DestroyEnchantedPermanent(),
            ))
            continue

        # ----------------------------------------------------------------
        # Standard triggered-ability lines (When/Whenever ...)
        # ----------------------------------------------------------------
        parsed_trigger = _parse_triggered_ability_line(line, name=name)
        if parsed_trigger is not None:
            if isinstance(parsed_trigger, tuple):
                triggers.extend(parsed_trigger)
            else:
                triggers.append(parsed_trigger)
            continue

        # ----------------------------------------------------------------
        # Aura static lines — "Enchanted creature/permanent gets ..."
        # ----------------------------------------------------------------
        gets_parsed = _parse_enchanted_gets_line(line)
        if gets_parsed is not None:
            pb, tb, gkw, lose_all, ward, max_one = gets_parsed
            power_bonus += pb
            toughness_bonus += tb
            granted_keywords.update(gkw)
            if lose_all:
                attached_grants_lose_all_abilities = True
            if max_one:
                attached_cant_be_blocked_by_more_than_one = True
            if ward is not None:
                attached_grants_ward_str = ward
            continue

        # ----------------------------------------------------------------
        # "Enchanted (creature|permanent) has ward {cost}." (standalone)
        # ----------------------------------------------------------------
        ward_m = _ENCHANTED_HAS_WARD_LINE.match(line)
        if ward_m is not None:
            attached_grants_ward_str = ward_m.group(1)
            continue

        # ----------------------------------------------------------------
        # "You control enchanted creature/permanent."
        # ----------------------------------------------------------------
        if _YOU_CONTROL_ENCHANTED_LINE.match(line):
            attached_grants_control = True
            continue

        # ----------------------------------------------------------------
        # "Enchanted ... doesn't untap during its controller's untap step."
        # ----------------------------------------------------------------
        if _DOESNT_UNTAP_LINE.match(line):
            attached_prevents_untap = True
            continue

        # ----------------------------------------------------------------
        # "Enchanted ... loses all abilities." (standalone)
        # ----------------------------------------------------------------
        if _LOSES_ALL_ABILITIES_LINE.match(line):
            attached_grants_lose_all_abilities = True
            continue

        # ----------------------------------------------------------------
        # Moonlit Meditation token-replace static (literal)
        # ----------------------------------------------------------------
        if line.rstrip(".") == _REPLACES_FIRST_TOKENS_LINE.rstrip("."):
            replaces_first_tokens_with_copies = True
            continue

        # Unrecognized line — reject the card
        return None

    return make_aura(
        name=card["name"],
        cost=card.get("mana_cost", ""),
        power_bonus=power_bonus,
        toughness_bonus=toughness_bonus,
        granted_keywords=frozenset(granted_keywords),
        keywords=frozenset(aura_keywords),
        triggered_abilities=tuple(triggers),
        aura_enchant_type=aura_enchant_type,
        aura_yours_only=aura_yours_only,
        attached_prevents_untap=attached_prevents_untap,
        attached_grants_control=attached_grants_control,
        attached_grants_lose_all_abilities=attached_grants_lose_all_abilities,
        attached_cant_be_blocked_by_more_than_one=attached_cant_be_blocked_by_more_than_one,
        replaces_first_tokens_with_copies=replaces_first_tokens_with_copies,
        attached_grants_ward=attached_grants_ward_str,
    )


def _build_enchantment_printing(card: dict) -> CardPrinting | None:
    """Claim an Enchantment card via name-keyed dispatch, falling back to
    a printing-flag-only oracle-text path for cards whose entire oracle
    text consists of recognized printing-level static lines (Deafening
    Silence's noncreature-spell restriction etc.).

    Returns the canonical ``CardPrinting`` if either path claims the
    card. Returns ``None`` otherwise so the builder chain continues.
    The exclusion guard rejects hybrid card types (Creature /
    Land / Artifact); their own builders own those.
    """
    type_line = card.get("type_line", "")
    if "Enchantment" not in type_line:
        return None
    # Exclude hybrid card types — let their own builders handle them.
    for exclusion in ("Creature", "Land", "Artifact"):
        if exclusion in type_line:
            return None
    # Aura oracle-text path tried before the non-Aura oracle-text path
    # so "Aura" in the type line routes to the right grammar.
    if "Aura" in type_line:
        aura = _build_aura_via_oracle_text(card)
        if aura is not None:
            return aura
    built = _build_enchantment_via_oracle_text(card)
    if built is not None:
        return built
    # PowerLSV cube enchantments: line-composed grammar, explicit
    # per-name opt-in (``cube_enchantments.CUBE_ENCHANTMENT_NAMES``).
    from mtgz_ir.cards.parser.cube_enchantments import (
        build_cube_enchantment,
    )

    return build_cube_enchantment(card)


# Allowlist of names that may claim through the generic oracle-text
# fallback path below. Mirrors the room builder's
# ``_UNIFIED_ROOM_NAMES`` precedent — incremental migration: only
# names listed here can be reached by ``_build_enchantment_via_
# oracle_text``, so adding a new name to the allowlist is the
# explicit opt-in for that card. Other enchantments still take the
# name-dispatched bespoke factory path (or fall through to silent
# skip).
# ---------------------------------------------------------------------------
# Non-Aura enchantment oracle-text static recognizers
# ---------------------------------------------------------------------------
#
# Literal lines whose corresponding engine flag is a boolean or string field
# on ``make_enchantment``. Each is checked in ``_build_enchantment_via_
# oracle_text`` before the trigger parser so the flag accumulates before
# any trigger lines are processed. Adding a new flag-line is a one-entry
# addition here + one kwarg name in the call to ``make_enchantment`` below.

# "If one or more counters would be put on a creature, Spacecraft, or
# Planet you control, twice that many of each of those kinds of
# counters are put on it instead." → SELF-scope StaticAbility carrying
# a ReplacementEffect(WOULD_PLACE_COUNTER, MultiplyCount(2)). Only
# card in the pool with this exact counter-doubler surface; deferred
# generalization until a second counter-doubler enchantment lands.
_COUNTER_DOUBLER_LINE = (
    "If one or more counters would be put on a creature, Spacecraft, or "
    "Planet you control, twice that many of each of those kinds of "
    "counters are put on it instead."
)

# "Each creature you control enters with an additional +1/+1 counter on it
# for each land that entered the battlefield under your control this turn."
# → grants_etb_counter_per_land_entered_this_turn=True. (Bioengineered
# Future.) Only card in the pool with this surface today; deferred until
# a second example exists.
_GRANTS_ETB_COUNTER_PER_LAND_LINE = (
    "Each creature you control enters with an additional +1/+1 counter "
    "on it for each land that entered the battlefield under your control "
    "this turn."
)

# "The first spell each player casts during each of their turns may be cast
# without paying its mana cost." → grants_first_spell_each_turn_free=True.
# (Weftwalking.) Only card in the pool with this surface; deferred until a
# second example exists.
_GRANTS_FIRST_SPELL_FREE_LINE = (
    "The first spell each player casts during each of their turns may be "
    "cast without paying its mana cost."
)

# Honest no-op lines for non-Aura enchantments. Lines that appear on
# migrated cards but map to inert / unoffered / engine-unmodeled mechanics.
# The recognizer silently drops them so the remaining lines still parse.
#
# - Leyline of Mutation's two lines are both inert in this engine:
#   opening-hand replacement and the alt-cost offer are not modeled.
#   Both are therefore honest no-ops per the permission-not-offered
#   precedent (same shape as Lecture Hall / Shardmage's Rescue).
_NONAURA_HONEST_NOOP_LINES: frozenset[str] = frozenset({
    # Leyline of Mutation: opening-hand replacement (engine never places
    # it on the battlefield at game start — not modeled).
    "If this card is in your opening hand, you may begin the game "
    "with it on the battlefield.",
    # Leyline of Mutation: alt-cost offer (not wired in legal.py —
    # same permission-not-offered shape Lecture Hall ships).
    "You may pay {W}{U}{B}{R}{G} rather than pay the mana cost for "
    "spells you cast.",
})


# Cards in this set are allowed to produce a cost-and-name-only
# ``CardPrinting`` even when ``_build_enchantment_via_oracle_text``
# recognizes no actionable lines. Every line on these cards is either
# in ``_NONAURA_HONEST_NOOP_LINES`` or is otherwise inert; the printing
# sits on the battlefield doing nothing, which is rules-correct given
# that none of its surface clauses is wired through the engine today.
# Honest-no-op discipline: Leyline of Mutation prints "Each
# nonbasic land is a basic land of the chosen type" — no surface
# the engine wires today, so the card emits as a cost-and-name-only
# printing rather than dropping silently.
_UNIFIED_ENCHANTMENT_HONEST_NOOP_NAMES: frozenset[str] = frozenset({
    "Leyline of Mutation",
})


_UNIFIED_ENCHANTMENT_NAMES: frozenset[str] = frozenset({
    "Deafening Silence",
    "Burgeoning",
    # Banishing Light / Seam Rip / Trapped in the Screen — ETB exile-until-
    # source-leaves shapes.  The effect phrases are now in the grammar
    # (``_EXILE_OPPONENTS_NONLAND_UNTIL_LEAVES`` in effects.py).
    "Banishing Light",
    "Seam Rip",
    "Trapped in the Screen",
    # Eusocial Engineering — landfall trigger + warp cost. The trigger
    # "a land you control enters" is in PREDICATE_TRIGGER_PHRASES and
    # the Robot token creation is in TOKEN_PRINTINGS; warp support was
    # added to _build_enchantment_via_oracle_text in wave 5.
    "Eusocial Engineering",
    # Weapons Manufacturing — nontoken-artifact-enters trigger + named
    # Munitions token. Trigger phrase and named-token recognizer added
    # in wave 5.
    "Weapons Manufacturing",
    # Wave 1 (bespoke-enchantments) non-Aura additions:
    # Loading Zone — counter-doubler static + warp. The counter-doubler
    # line is recognized as a literal; warp parses via _parse_warp_line.
    "Loading Zone",
    # Bioengineered Future — ETB create-Lander trigger (parseable) +
    # grants_etb_counter_per_land_entered_this_turn static (literal).
    "Bioengineered Future",
    # Leyline of Mutation — both printed lines are honest no-ops.
    # The printing sits inert, same shape as the bespoke factory.
    "Leyline of Mutation",
    # Weftwalking — grants_first_spell_each_turn_free static (literal)
    # + ETB trigger via the unified grammar.
    "Weftwalking",
    # Terrasymbiosis — COUNTERS_ADDED trigger now claims through the
    # unified clause grammar via the ``"you put one or more +1/+1
    # counters on a creature you control"`` predicate trigger phrase
    # plus the ``Do this only once each turn.`` rider strip. The
    # context-dependent effect rewrite in ``_parse_triggered_ability_
    # line`` swaps the default ``DrawCardsEqualToResolvingCombatDamage``
    # to ``DrawCardsEqualToResolvingCounterAmount`` when the trigger
    # event is COUNTERS_ADDED.
    "Terrasymbiosis",
    # Sothera, the Supervoid — DIES trigger decomposed into the
    # generic ``EachOpponentExilesOwnCreature(link_to_source=True)``
    # primitive (audit-per-opponent-choice). The end-step trigger
    # decomposed into the generic
    # ``Sequence(ReanimateOneExiledWithSourceWithCounters(
    # counter_amount=2), SacrificeSelfPreservingExileLink())`` plus
    # the ``StateBound(AnyPlayerControlsNoCreatures())`` intervening-if
    # gate (audit-sothera-foundfootage). Both triggers now claim
    # through the unified parser grammar.
    "Sothera, the Supervoid",
})


def _build_enchantment_via_oracle_text(
    card: dict,
) -> CardPrinting | None:
    """Fallback path: claim an Enchantment whose entire oracle text
    parses through recognized line shapes — printing-flag statics
    (Deafening Silence) or triggered-ability lines (Burgeoning).

    Gated on ``_UNIFIED_ENCHANTMENT_NAMES`` so the path only attempts
    cards that have been explicitly migrated. Returns ``None`` for
    any card with an unrecognized line so the silent-skip path
    mirrors the rest of the parser.

    Printing-flag statics recognized (in addition to warp, ward, and
    restricts_noncreature_spells):
    - ``_COUNTER_DOUBLER_LINE`` → SELF-scope StaticAbility(
      replacement_effect=ReplacementEffect(WOULD_PLACE_COUNTER,
      MultiplyCount(2)))
    - ``_GRANTS_ETB_COUNTER_PER_LAND_LINE`` →
      ``grants_etb_counter_per_land_entered_this_turn=True``
    - ``_GRANTS_FIRST_SPELL_FREE_LINE`` →
      ``grants_first_spell_each_turn_free=True``
    - ``_NONAURA_HONEST_NOOP_LINES`` → silently dropped (honest no-ops)
    """
    name = card.get("name", "")
    if name not in _UNIFIED_ENCHANTMENT_NAMES:
        return None

    text = card.get("oracle_text", "")
    legendary = "Legendary" in card.get("type_line", "")
    restricts_noncreature_spells = False
    grants_etb_counter_per_land = False
    grants_first_spell_free = False
    ward: str | None = None
    warp: str | None = None
    triggers: list[TriggeredAbility] = []
    statics: list[StaticAbility] = []

    for raw_line in text.split("\n"):
        line = _strip_reminder_text(raw_line)
        if not line:
            continue
        # Honest no-op lines — recognized and dropped
        if line in _NONAURA_HONEST_NOOP_LINES:
            continue
        if _parse_restricts_noncreature_spells_line(line):
            restricts_noncreature_spells = True
            continue
        ward_cost = _parse_ward_line(line)
        if ward_cost is not None:
            ward = ward_cost
            continue
        warp_cost = _parse_warp_line(line)
        if warp_cost is not None:
            warp = warp_cost
            continue
        # Counter-doubler static (Loading Zone). Builds a SELF-scope
        # StaticAbility carrying a ReplacementEffect that multiplies
        # the WOULD_PLACE_COUNTER count by 2 when the recipient is
        # under the source's controller; the dispatcher's implicit
        # "source.controller_id == affected_player_id" gate handles
        # the printed "you control" qualifier.
        if line == _COUNTER_DOUBLER_LINE:
            # The recipients: a creature, Spacecraft, or Planet (the
            # "you control" half is the dispatcher's controller gate);
            # every kind of counter (mtg-zero docs/ir_defects.md 25).
            from mtgz_ir.effects.object_predicates import HasSubtype, IsCreature

            statics.append(StaticAbility(
                scope=StaticScope.SELF,
                replacement_effect=ReplacementEffect(
                    event_pattern=EventPattern(
                        event=Event.WOULD_PLACE_COUNTER,
                        subject_filters={
                            "obj": IsCreature() | HasSubtype("Spacecraft")
                            | HasSubtype("Planet"),
                        },
                    ),
                    transform=MultiplyCount(factor=2),
                ),
            ))
            continue
        # ETB-counter-per-land static (Bioengineered Future)
        if line == _GRANTS_ETB_COUNTER_PER_LAND_LINE:
            grants_etb_counter_per_land = True
            continue
        # First-spell-free static (Weftwalking)
        if line == _GRANTS_FIRST_SPELL_FREE_LINE:
            grants_first_spell_free = True
            continue
        parsed_trigger = _parse_triggered_ability_line(line, name=name)
        if parsed_trigger is not None:
            if isinstance(parsed_trigger, tuple):
                triggers.extend(parsed_trigger)
            else:
                triggers.append(parsed_trigger)
            continue
        # Anything else — Aura grammar, activated abilities, unrecognized
        # statics — falls through to the silent-skip path.
        return None

    has_any_recognized = (
        restricts_noncreature_spells
        or triggers
        or statics
        or grants_etb_counter_per_land
        or grants_first_spell_free
        or ward is not None
        or warp is not None
    )
    if not has_any_recognized and name not in _UNIFIED_ENCHANTMENT_HONEST_NOOP_NAMES:
        # Nothing recognized — leave the card for a future grammar
        # slice rather than emit a blank Enchantment. Exception: cards
        # in _UNIFIED_ENCHANTMENT_HONEST_NOOP_NAMES are explicitly
        # all-no-op and still emit a cost-and-name-only printing.
        return None

    return make_enchantment(
        name=card["name"],
        cost=card.get("mana_cost", ""),
        legendary=legendary,
        restricts_noncreature_spells_to_one_per_turn=(
            restricts_noncreature_spells
        ),
        static_abilities=tuple(statics),
        grants_etb_counter_per_land_entered_this_turn=(
            grants_etb_counter_per_land
        ),
        grants_first_spell_each_turn_free=grants_first_spell_free,
        ward=ward,
        warp=warp,
        triggered_abilities=tuple(triggers),
    )
