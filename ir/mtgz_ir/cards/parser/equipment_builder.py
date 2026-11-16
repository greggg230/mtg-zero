"""Artifact — Equipment ``CardPrinting`` builder.

Equipment is a narrow, uniform shape on Scryfall: keyword line(s) +
one or more ``Equipped creature`` clauses + an ``Equip {N}`` line +
optional ETB triggers. The grammar this builder claims today covers:

- a single static "``Equipped creature gets +N/+M``" clause, optionally
  with a trailing ``" and has <keyword>[, <keyword>...]"`` rider that
  grants printed evergreen keywords to the equipped creature;
- a single "``Equipped creature has <keyword>``" clause as a standalone
  granted-keyword line;
- a standalone evergreen-keyword line on the Equipment itself (most
  often ``Flash``), routed to ``make_equipment(keywords=...)``;
- a single ETB trigger of the form
  ``"When this Equipment enters, attach it to target creature you
  control."`` (canonical ``AttachToTargetCreatureYouControl`` via
  ``IsSelf`` ENTERS), optionally with a trailing
  ``" That creature gains <keyword> until end of turn."`` rider that
  feeds ``granted_keywords_ueot=`` on the same effect — Squire's
  Lightblade and Illvoi Light Jammer ride this shape;
- a single ETB trigger of the form
  ``"When this Equipment enters, create a 2/2 colorless Robot artifact
  creature token and attach this Equipment to it."`` wired through
  ``CreateTokenAndAttachSelf`` against ``TOKEN_PRINTINGS["Robot"]`` —
  Auxiliary Boosters' bespoke ETB;
- a single ETB trigger of the form
  ``"When this Equipment enters, create a 1/1 white Glimmer
  enchantment creature token."`` wired through bare ``CreateToken``
  against ``TOKEN_PRINTINGS["Glimmer"]`` — Glimmerlight's ETB. Distinct
  from the Auxiliary Boosters shape because the Equipment does NOT
  attach to the created token;
- a single ETB trigger of the form
  ``"When this Equipment enters, manifest dread, then attach this
  Equipment to that creature."`` wired through the bespoke
  ``ManifestDreadThenAttachSelf`` engine class — Duskmourn Equipment
  family (Conductive Machete, Killer's Mask, Cursed Windbreaker,
  Dissection Tools). "That creature" refers back to the just-
  manifested face-down 2/2, parallel to the Slimy Aquarium / Weight
  Room door triggers' ``ManifestDreadThenPutPlusOneCountersOnIt``
  back-reference;
- Hylderblade's Void rider — a literal beginning-of-end-step
  attach-to-target-creature trigger gated on ``requires_void=True``
  (the engine's stand-in for the EoE intervening-if condition);
- Robe of Stars' Astral Projection activation — a flavor-named
  ``"<name> — <cost>: Equipped creature phases out."`` line wired
  into an ``ActivatedAbility`` whose effect is the bespoke
  ``PhaseOutEquippedCreature``;
- a closing ``"Equip {N}"`` line whose mana-cost token feeds
  ``make_equipment(equip_cost=...)``.

Anything outside that shape — ``Equipped creature has
"<granted activated ability>"``, the custom attack trigger on Atomic
Microsizer, or any line the per-line dispatcher doesn't recognize —
falls through and the Equipment is silently skipped, mirroring the
creature/spell/land builders.

These line phrases are equipment-specific ("Equipped creature ...",
"Equip {N}", "When this Equipment enters, ...") and don't fit the
creature-focused unified clause grammar in ``clause.py``, so they're
recognized here with narrow regexes scoped to this builder.
"""

from __future__ import annotations

import re
from dataclasses import replace

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    CardPrinting,
    TriggeredAbility,
    make_equipment,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.keywords import _KEYWORD_LINES, _parse_keyword_line
from mtgz_ir.cards.parser.text import _strip_reminder_text
from mtgz_ir.cards.parser.type_line import _parse_type_line
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.bespoke import (
    AtomicMicrosizerAttackEffect,
    SawAttacksOptionalSacDrawEffect,
)
from mtgz_ir.effects.generic import (
    AttachToTargetCreatureYouControl,
    CreateToken,
    CreateTokenAndAttachSelf,
    ManifestDreadThenAttachSelf,
    PhaseOutEquippedCreature,
    TapUpToOneTargetCreature,
)
from mtgz_ir.effects.predicates import Controlled, Event, IsHost, IsSelf, YOU


# "Equipped creature gets +N/+M" with an optional trailing rider that
# grants one or more printed evergreen keywords. Examples this matches:
#
#   "Equipped creature gets +2/+1 and has reach."
#   "Equipped creature gets +1/+0."
#   "Equipped creature gets +3/+1."
#
# The bonus integers are required (negative bonuses aren't in the EoE
# pool we claim today; intentionally not handled). The trailing
# granted-keyword rider, when present, is a comma-joined list of
# evergreens routed through ``_KEYWORD_LINES`` for normalization to
# the engine's lowercase spelling.
#
# The bonus integers are signed so negative riders (Skullclamp's
# ``+1/-1``) claim through the same regex.
_EQUIPPED_CREATURE_GETS_RE = re.compile(
    r"^Equipped creature gets ([+-]\d+)/([+-]\d+)"
    r"(?: and has (.+?))?\.$"
)


# "Equipped creature gets +N/+M for each <group> you control." —
# Nettlecyst (``artifact and/or enchantment``). Feeds
# ``CardPrinting.equipment_bonus_per_controlled`` (per-unit bonus times
# the number of permanents of the listed types the Equipment's
# controller controls, recomputed continuously).
_EQUIPPED_CREATURE_GETS_PER_CONTROLLED_RE = re.compile(
    r"^Equipped creature gets \+(\d+)/\+(\d+) for each "
    r"(artifact and/or enchantment|artifact|creature|land|enchantment) "
    r"you control\.$"
)
_PER_CONTROLLED_TYPES: dict[str, tuple[str, ...]] = {
    "artifact and/or enchantment": ("Artifact", "Enchantment"),
    "artifact": ("Artifact",),
    "creature": ("Creature",),
    "land": ("Land",),
    "enchantment": ("Enchantment",),
}


# "Equipped creature gets +N/+M and has <keywords>, and "<quoted
# triggered ability>"" — Kaldra Compleat. The keyword list parses like
# every other rider; the quoted ability is granted to the equipped
# creature (``CardPrinting.equipment_granted_triggers``). Only quoted
# abilities in ``_GRANTED_QUOTED_TRIGGERS`` are accepted.
_EQUIPPED_GETS_KEYWORDS_AND_QUOTED_RE = re.compile(
    r'^Equipped creature gets \+(\d+)/\+(\d+) and has (.+), and "(.+)"$'
)


def _granted_quoted_trigger(text: str):
    """Triggered ability for a quoted granted ability, or None.

    Literal table: Kaldra Compleat is the only Equipment in the pool
    granting a quoted trigger; its "that creature" is the damaged
    creature delivered as the DEALS_COMBAT_DAMAGE_TO_CREATURE event's
    fixed target (hence a dedicated entry rather than the unified
    effect grammar, where "that creature" has no general referent)."""
    from mtgz_ir.effects.generic import ExileThatCreature

    if text == (
        "Whenever this creature deals combat damage to a creature, exile "
        "that creature."
    ):
        return TriggeredAbility(
            event=Event.DEALS_COMBAT_DAMAGE_TO_CREATURE,
            predicate=IsSelf(subject="source"),
            effect=ExileThatCreature(),
        )
    return None


# Keyword-action lines that create a creature token and attach the
# Equipment to it (CR 702.92 Living weapon: a 0/0 black Phyrexian Germ;
# CR 702.163 For Mirrodin!: a 2/2 red Rebel). Both are ETB triggers.
_CREATE_AND_ATTACH_KEYWORD_TOKENS: dict[
    str, tuple[str, int, int, tuple[str, ...], frozenset[str]]
] = {
    "Living weapon": (
        "Phyrexian Germ", 0, 0, ("Phyrexian", "Germ"), frozenset({"B"}),
    ),
    "For Mirrodin!": ("Rebel", 2, 2, ("Rebel",), frozenset({"R"})),
}


# "Equipped creature has <keyword>[, <keyword>...]" — granted-keyword-
# only clause with no P/T bonus. The body is a comma-joined evergreen
# list, same as the trailing-rider form on ``_EQUIPPED_CREATURE_GETS_RE``.
_EQUIPPED_CREATURE_HAS_RE = re.compile(
    r"^Equipped creature has (.+?)\.$"
)


# "Equip {N}" — the printed Equip activated ability. The mana-cost
# token (e.g. ``"{3}"``, ``"{2}{W}"``) is captured verbatim and feeds
# ``make_equipment(equip_cost=...)``, which synthesizes the matching
# sorcery-speed ``ActivatedAbility``.
#
# We accept either the bare line or the line followed by Scryfall's
# parenthesized reminder text — the per-line caller strips reminder
# text via ``_strip_reminder_text`` before regex matching, so the
# regex itself only needs to cover the bare form.
_EQUIP_LINE_RE = re.compile(
    r"^Equip (\{[^}]+\}(?:\{[^}]+\})*)$"
)


# Literal ETB attach trigger used by Meltstrider's Gear and several
# other EoE Equipment cards. The exact phrasing is uniform across
# printings, so a literal-string match is enough — no sub-parsing
# needed.
_ETB_ATTACH_LINE = (
    "When this Equipment enters, attach it to target creature you control."
)


# ETB attach with a UEOT-keyword grant rider. Squire's Lightblade
# ("That creature gains first strike until end of turn.") and Illvoi
# Light Jammer ("That creature gains hexproof until end of turn.") use
# this shape. Only single-keyword riders are accepted; the keyword must
# be in the evergreen whitelist. Reminder text on the keyword (e.g.
# Illvoi's parenthetical hexproof gloss) is stripped before this regex
# sees the line.
_ETB_ATTACH_UEOT_RE = re.compile(
    r"^When this Equipment enters, attach it to target creature you "
    r"control\. That creature gains (.+?) until end of turn\.$"
)


# ETB create-token-and-attach trigger used by Auxiliary Boosters. The
# token is a 2/2 colorless Robot artifact creature; the parser wires
# this through ``CreateTokenAndAttachSelf`` against the engine's
# canonical ``Robot`` token printing. The exact phrasing is literal —
# the only Equipment in the EoE pool that creates a token in its ETB.
_ETB_CREATE_ROBOT_AND_ATTACH_LINE = (
    "When this Equipment enters, create a 2/2 colorless Robot artifact "
    "creature token and attach this Equipment to it."
)


# ETB create-Glimmer-token (NO attach) used by Glimmerlight. Distinct
# from the Auxiliary Boosters shape: the printed line ends after the
# token-creation clause and never says "and attach" — the Equipment
# stays unattached on the battlefield. Matched as a literal so the
# bare ``CreateToken(Glimmer, count=1)`` engine class survives, parallel
# to the Robot-and-attach literal above. A future Equipment with a
# different ETB-create-token-no-attach token would land as a sibling
# entry rather than expanding into a parameterized regex (the EoE/DSK
# pool today has exactly one such printing).
_ETB_CREATE_GLIMMER_LINE = (
    "When this Equipment enters, create a 1/1 white Glimmer "
    "enchantment creature token."
)


# ETB manifest-dread-then-attach used by the Duskmourn Equipment family
# (Conductive Machete, Killer's Mask, Cursed Windbreaker, Dissection
# Tools). "That creature" in the printed line refers back to the just-
# manifested face-down 2/2 — same back-reference shape as the Slimy
# Aquarium / Weight Room "manifest dread, then put a +1/+1 counter on
# that creature" door triggers, which is why the engine class
# ``ManifestDreadThenAttachSelf`` parallels
# ``ManifestDreadThenPutPlusOneCountersOnIt``. Matched as a literal so
# the bespoke combined effect survives end-to-end without parser
# guesswork. The Auxiliary Boosters / Glimmerlight literals above are
# the precedent for keeping per-token ETB families as sibling literal
# entries rather than expanding into a parameterized regex.
_ETB_MANIFEST_DREAD_AND_ATTACH_LINE = (
    "When this Equipment enters, manifest dread, then attach this "
    "Equipment to that creature."
)


# Baseball Bat's equipped-attacks "tap up to one target creature."
# trigger. The bare ``tap up to one target creature`` payload is the
# new generic ``TapUpToOneTargetCreature`` engine class — sibling to
# ``TapAnotherTargetCreature`` (Starport Security, strict-1, source-
# excluded) and ``TapTargetArtifactOrCreature`` (Mental Modulation,
# strict-1, broader filter). Each engine class carries its own
# legal-target / target-count contract, so we don't try to fold them
# into a single parameterized class. ``IsHost(subject="attacker")``
# matches the equipped-creature-attacks trigger condition.
_EQUIPPED_ATTACKS_TAP_UP_TO_ONE_LINE = (
    "Whenever equipped creature attacks, tap up to one target creature."
)


# Atomic Microsizer's equipped-attacks trigger — choose-up-to-one
# target creature that becomes unblockable and base 1/1 UEOT. The
# whole printed line maps 1:1 to ``AtomicMicrosizerAttackEffect()``,
# whose resolve threads the unblockable flag and the temp base-P/T
# override onto the chosen target. Only Equipment in the EoE pool
# carrying this exact shape, so a literal-line match is sufficient —
# same pattern as the Saw / Baseball Bat / Hylderblade lines below.
# ``IsHost(subject="attacker")`` matches the equipped-creature-
# attacks trigger condition.
_EQUIPPED_ATTACKS_MICROSIZER_LINE = (
    "Whenever equipped creature attacks, choose up to one target "
    "creature. That creature can't be blocked this turn and has base "
    "power and toughness 1/1 until end of turn."
)


# Saw's equipped-attacks "may sacrifice a permanent other than that
# creature or this Equipment. If you do, draw a card." trigger. The
# legal-sacrifice pool ("permanent other than ...") doesn't fit the
# narrower ``OptionalSacAnotherForEffect`` (creatures-or-artifacts only)
# or the artifact-only ``OptionalSacArtifactForEffect`` shapes — Saw
# accepts ANY permanent the controller controls except the Equipment
# itself and the equipped creature. The bespoke effect class
# ``SawAttacksOptionalSacDrawEffect`` encodes this exclusion plus the
# trailing ``If you do, draw a card.`` rider as one resolve step. The
# only Equipment in the DSK pool today carrying this exact shape, so a
# literal-line match is sufficient.
_EQUIPPED_ATTACKS_SAC_PERMANENT_DRAW_LINE = (
    "Whenever equipped creature attacks, you may sacrifice a permanent "
    "other than that creature or this Equipment. If you do, draw a card."
)


# Hylderblade's Void rider — a beginning-of-end-step trigger that
# attaches the Equipment to a chosen creature, gated on the EoE Void
# intervening-if condition ("a nonland permanent left the battlefield
# this turn or a spell was warped this turn"). The static side encodes
# the intervening-if as ``requires_void=True`` on the
# ``TriggeredAbility``, which is the same gate ``should_fire_trigger``
# checks for every Void rider in the engine. The line shape is literal
# enough that we recognize it as a string match — the only Equipment
# in the pool today carrying a Void rider.
_VOID_END_STEP_ATTACH_LINE = (
    "Void — At the beginning of your end step, if a nonland "
    "permanent left the battlefield this turn or a spell was warped "
    "this turn, attach this Equipment to target creature you control."
)


# The Dominion Bracelet's "equipped creature gets +1/+1 and has
# <quoted-activated-ability>" line. The quoted granted ability is
# ``{15}, Exile The Dominion Bracelet: You control target opponent
# during their next turn.`` plus an X-cost-reduction rider and a
# sorcery-speed restriction. The static side wires this whole shape
# through one printing-level boolean
# (``grants_control_opponent_ability_when_equipped=True``) which
# layers ``DOMINION_BRACELET_GRANTED_ABILITY`` onto the equipped
# creature; the parser mirrors that simplification by matching the
# verbatim line and setting both the +1/+1 bonus and the printing
# flag in one step.
#
# The line is matched as a literal because the embedded mana-cost
# tokens, the long quoted-ability body, and the trailing
# ``Activate only as a sorcery."`` close-quote don't fit any
# generalizable per-clause grammar — only Equipment in the EoE pool
# carrying this shape, same precedent as the Atomic Microsizer
# attack-trigger literal above. Reminder text inside the trailing
# parenthetical is stripped before this regex sees the line.
_EQUIPPED_GETS_AND_HAS_DOMINION_BRACELET_LINE = (
    'Equipped creature gets +1/+1 and has '
    '"{15}, Exile The Dominion Bracelet: '
    'You control target opponent during their next turn. '
    "This ability costs {X} less to activate, "
    "where X is this creature's power. "
    'Activate only as a sorcery."'
)


# Robe of Stars' Astral Projection activation — a flavor-named
# ``"<name> — <cost>: <effect>"`` activated ability whose effect is
# ``"Equipped creature phases out."``. The flavor name on the prefix is
# load-bearing for the printed line shape but doesn't carry any rules
# behavior, so the parser captures only the mana cost. The current EoE
# pool has exactly one Equipment activation of this shape, so a narrow
# regex pinned to the "Equipped creature phases out." effect text is
# enough — broader Equipment-activation grammar is deferred until a
# second example exists.
_PHASE_OUT_ACTIVATION_RE = re.compile(
    r"^[A-Z][A-Za-z ]+ — (\{[^}]+\}(?:\{[^}]+\})*): "
    r"Equipped creature phases out\.$"
)


def _parse_granted_keyword_list(rider: str) -> frozenset[str] | None:
    """Parse a comma-joined evergreen-keyword rider (``"flying"``,
    ``"flying, trample"``, ``"reach"``) into the frozen set of engine
    spellings. Returns ``None`` if any piece is not a recognized
    evergreen — the enclosing Equipment is then silently skipped.

    Reuses ``_KEYWORD_LINES`` so the parser can never grant a keyword
    the engine doesn't model.
    """
    pieces = [
        p.strip() for p in re.split(r", and |, | and ", rider) if p.strip()
    ]
    # Equipment riders use sentence-case ("flying"), unlike standalone
    # keyword lines which use title-case ("Flying"). Normalize to
    # title-case for the lookup so we share the existing whitelist.
    normalized = [p[:1].upper() + p[1:] for p in pieces]
    if not all(p in _KEYWORD_LINES for p in normalized):
        return None
    return frozenset(_KEYWORD_LINES[p] for p in normalized)


def _build_equipment_printing(card: dict) -> CardPrinting | None:
    """Try to produce a ``CardPrinting`` for an Equipment ``card`` (a
    Scryfall record). Returns ``None`` if the card is not an Equipment
    or if its oracle text contains any line outside the narrow shape
    this builder supports.

    Recognized lines are processed in any order; the only constraint is
    that an ``Equip {N}`` line is required (Equipment without a printed
    Equip cost would still need the kwarg-omitted variant of
    ``make_equipment`` and don't appear in the EoE pool today).
    """
    supertypes, card_types, subtypes = _parse_type_line(
        card.get("type_line", "")
    )
    if "Artifact" not in card_types:
        return None
    if "Equipment" not in subtypes:
        return None

    text = card.get("oracle_text", "")
    # Vintage Roto P5: modal activated-ability blocks and ward riders the
    # line grammar below can't see whole.
    from mtgz_ir.cards.parser.roto_board import (
        apply_board_fields,
        claim_equipment_lines,
        parse_roto_board_line_into,
    )

    claimed = claim_equipment_lines(card)
    if claimed is None:
        return None
    text, roto = claimed
    roto_fields: dict = dict(roto.fields)
    power_bonus = 0
    toughness_bonus = 0
    granted_keywords: set[str] = set()
    self_keywords: set[str] = set()
    triggers: list[TriggeredAbility] = []
    activated: list[ActivatedAbility] = []
    equip_cost: str | None = None
    grants_control_opponent_ability = False
    bonus_per_controlled: tuple[tuple[str, ...], int, int] | None = None
    has_graveyard_triggers = False
    granted_triggers: list[TriggeredAbility] = []

    for raw_line in text.split("\n"):
        line = _strip_reminder_text(raw_line)
        if not line:
            continue
        # Equip {N} — the printed activated ability. ``make_equipment``
        # synthesizes the ``ActivatedAbility``, so we just hand it the
        # cost string.
        m = _EQUIP_LINE_RE.match(line)
        if m is not None:
            if equip_cost is not None:
                # A second Equip line would mean a malformed printing
                # or a shape we don't model — bail rather than silently
                # overwrite.
                return None
            equip_cost = m.group(1)
            continue
        # The Dominion Bracelet's "+1/+1 and has <quoted granted ability>"
        # literal line — sets the +1/+1 bonus and the
        # ``grants_control_opponent_ability_when_equipped`` printing flag
        # in one step. Tried BEFORE the generic ``_EQUIPPED_CREATURE_GETS_
        # RE`` so the literal wins on the bracelet's exact surface form;
        # the generic regex's trailing ``\.$`` anchor wouldn't match the
        # close-quote tail anyway, but ordering literal-first keeps the
        # precedence intent visible.
        if line == _EQUIPPED_GETS_AND_HAS_DOMINION_BRACELET_LINE:
            power_bonus = 1
            toughness_bonus = 1
            grants_control_opponent_ability = True
            continue
        # "Equipped creature gets +N/+M[ and has <keyword>...]"
        m = _EQUIPPED_CREATURE_GETS_RE.match(line)
        if m is not None:
            power_bonus = int(m.group(1))
            toughness_bonus = int(m.group(2))
            rider = m.group(3)
            if rider is not None:
                kw = _parse_granted_keyword_list(rider)
                if kw is None:
                    return None
                granted_keywords |= kw
            continue
        # "Equipped creature has <keyword>" — granted-only, no P/T bonus.
        m = _EQUIPPED_CREATURE_HAS_RE.match(line)
        if m is not None:
            kw = _parse_granted_keyword_list(m.group(1))
            if kw is None:
                return None
            granted_keywords |= kw
            continue
        # Literal ETB attach trigger.
        if line == _ETB_ATTACH_LINE:
            triggers.append(
                TriggeredAbility(
                    event=Event.ENTERS, predicate=IsSelf(),
                    effect=AttachToTargetCreatureYouControl(),
                )
            )
            continue
        # ETB attach + UEOT keyword grant on the chosen target.
        m = _ETB_ATTACH_UEOT_RE.match(line)
        if m is not None:
            ueot_kw = _parse_granted_keyword_list(m.group(1))
            if ueot_kw is None:
                return None
            triggers.append(
                TriggeredAbility(
                    event=Event.ENTERS, predicate=IsSelf(),
                    effect=AttachToTargetCreatureYouControl(
                        granted_keywords_ueot=ueot_kw,
                    ),
                )
            )
            continue
        # ETB create-Robot-token-and-attach (Auxiliary Boosters).
        if line == _ETB_CREATE_ROBOT_AND_ATTACH_LINE:
            triggers.append(
                TriggeredAbility(
                    event=Event.ENTERS, predicate=IsSelf(),
                    effect=CreateTokenAndAttachSelf(
                        token_printing=TOKEN_PRINTINGS["Robot"],
                    ),
                )
            )
            continue
        # ETB create-Glimmer-token (no attach) — Glimmerlight.
        if line == _ETB_CREATE_GLIMMER_LINE:
            triggers.append(
                TriggeredAbility(
                    event=Event.ENTERS, predicate=IsSelf(),
                    effect=CreateToken(
                        token_printing=TOKEN_PRINTINGS["Glimmer"],
                        count=1,
                    ),
                )
            )
            continue
        # ETB manifest-dread-then-attach — DSK Equipment family
        # (Conductive Machete, Killer's Mask, Cursed Windbreaker,
        # Dissection Tools). One literal line; the bespoke
        # ``ManifestDreadThenAttachSelf`` engine class folds the
        # manifest and the auto-attach into one resolve step.
        if line == _ETB_MANIFEST_DREAD_AND_ATTACH_LINE:
            triggers.append(
                TriggeredAbility(
                    event=Event.ENTERS, predicate=IsSelf(),
                    effect=ManifestDreadThenAttachSelf(),
                )
            )
            continue
        # Atomic Microsizer's equipped-attacks trigger — choose-up-to-
        # one target creature becomes unblockable + base 1/1 UEOT. The
        # whole printed line maps to ``AtomicMicrosizerAttackEffect()``;
        # ``IsHost(subject="attacker")`` matches the equipped-creature-
        # attacks trigger condition.
        if line == _EQUIPPED_ATTACKS_MICROSIZER_LINE:
            triggers.append(
                TriggeredAbility(
                    event=Event.ATTACKS,
                    predicate=IsHost(subject="attacker"),
                    effect=AtomicMicrosizerAttackEffect(),
                )
            )
            continue
        # Baseball Bat's equipped-attacks tap-up-to-one trigger —
        # generic ``TapUpToOneTargetCreature`` engine class with
        # ``IsHost(subject="attacker")`` matching the equipped-creature-
        # attacks trigger condition. The 0-target choice is honored at
        # cast-time selection (the agent may decline to target).
        if line == _EQUIPPED_ATTACKS_TAP_UP_TO_ONE_LINE:
            triggers.append(
                TriggeredAbility(
                    event=Event.ATTACKS,
                    predicate=IsHost(subject="attacker"),
                    effect=TapUpToOneTargetCreature(),
                )
            )
            continue
        # Saw's equipped-attacks may-sac-permanent draw rider — bespoke
        # effect class wraps the (sacrifice, draw) pair plus the
        # "permanent other than that creature or this Equipment"
        # exclusion. ``IsHost(subject="attacker")`` matches the
        # ``Whenever equipped creature attacks`` trigger condition the
        # same way Atomic Microsizer's bespoke factory wires its attack
        # trigger.
        if line == _EQUIPPED_ATTACKS_SAC_PERMANENT_DRAW_LINE:
            triggers.append(
                TriggeredAbility(
                    event=Event.ATTACKS,
                    predicate=IsHost(subject="attacker"),
                    effect=SawAttacksOptionalSacDrawEffect(),
                )
            )
            continue
        # Hylderblade's Void rider — beginning-of-end-step attach gated
        # on the EoE Void intervening-if condition.
        if line == _VOID_END_STEP_ATTACH_LINE:
            triggers.append(
                TriggeredAbility(
                    event=Event.BEGINNING_OF_END_STEP,
                    predicate=Controlled(YOU),
                    requires_void=True,
                    effect=AttachToTargetCreatureYouControl(),
                )
            )
            continue
        # Robe of Stars' Astral Projection — a flavor-named activated
        # ability whose effect is the bespoke
        # ``PhaseOutEquippedCreature``. Only the mana cost is captured
        # off the line; the prefix flavor name is recognized but
        # discarded. Activated abilities accumulate alongside the
        # synthesized Equip ability ``make_equipment`` appends.
        m = _PHASE_OUT_ACTIVATION_RE.match(line)
        if m is not None:
            cost_str = m.group(1)
            activated.append(
                ActivatedAbility(
                    cost=ActivationCost(
                        mana=parse_mana_cost(cost_str), mana_str=cost_str,
                    ),
                    effect=PhaseOutEquippedCreature(),
                )
            )
            continue
        # Standalone evergreen-keyword line on the Equipment itself
        # (most often "Flash"). Routed through the shared keyword-line
        # parser so multi-keyword sentence-case lines also work.
        kw_tuple = _parse_keyword_line(line)
        if kw_tuple is not None:
            self_keywords.update(kw_tuple)
            continue
        # Kaldra Compleat: keywords + a quoted granted trigger.
        m = _EQUIPPED_GETS_KEYWORDS_AND_QUOTED_RE.match(line)
        if m is not None:
            kw = _parse_granted_keyword_list(m.group(3))
            granted = _granted_quoted_trigger(m.group(4))
            if kw is None or granted is None:
                return None
            power_bonus = int(m.group(1))
            toughness_bonus = int(m.group(2))
            granted_keywords |= kw
            granted_triggers.append(granted)
            continue
        # "Equipped creature gets +N/+M for each <types> you control."
        m = _EQUIPPED_CREATURE_GETS_PER_CONTROLLED_RE.match(line)
        if m is not None:
            if bonus_per_controlled is not None:
                return None
            bonus_per_controlled = (
                _PER_CONTROLLED_TYPES[m.group(3)],
                int(m.group(1)), int(m.group(2)),
            )
            continue
        # Living weapon / For Mirrodin! — ETB create-token-and-attach.
        token_spec = _CREATE_AND_ATTACH_KEYWORD_TOKENS.get(line)
        if token_spec is not None:
            from mtgz_ir.cards.base import make_token_creature
            tname, tp, tt, tsubtypes, tcolors = token_spec
            triggers.append(
                TriggeredAbility(
                    event=Event.ENTERS, predicate=IsSelf(),
                    effect=CreateTokenAndAttachSelf(
                        token_printing=make_token_creature(
                            tname, tp, tt, subtypes=tsubtypes,
                            colors=tcolors,
                        ),
                    ),
                )
            )
            continue
        # Anything else goes through the unified clause grammar
        # (triggered / activated / static lines such as Skateboard's
        # ``When this Equipment enters, tap target permanent.`` or
        # Skullclamp's ``Whenever equipped creature dies, draw two
        # cards.``). A line the unified grammar doesn't know skips the
        # whole card.
        from mtgz_ir.cards.parser.clause import _parse_clause
        clause = _parse_clause(line, name=card["name"])
        if clause is None:
            board = parse_roto_board_line_into(line, card, roto_fields)
            if board is None or board.keywords or board.statics:
                return None
            triggers.extend(board.triggers)
            activated.extend(board.activated)
            continue
        if clause.triggers:
            from mtgz_ir.cards.parser.artifact_effects import (
                _functions_from_graveyard,
            )
            for t in clause.triggers:
                if _functions_from_graveyard(t.effect):
                    t = replace(t, functions_in_graveyard=True)
                    has_graveyard_triggers = True
                triggers.append(t)
            continue
        if clause.activated is not None:
            activated.append(clause.activated)
            continue
        if clause.keywords:
            self_keywords.update(clause.keywords)
            continue
        # Static lines have no Equipment-side bucket today.
        return None

    if equip_cost is None:
        return None

    printing = make_equipment(
        name=card["name"],
        cost=card["mana_cost"],
        legendary="Legendary" in supertypes,
        power_bonus=power_bonus,
        toughness_bonus=toughness_bonus,
        granted_keywords=frozenset(granted_keywords),
        keywords=frozenset(self_keywords),
        triggered_abilities=tuple(triggers) + tuple(roto.triggers),
        activated_abilities=tuple(activated) + tuple(roto.activated),
        equip_cost=equip_cost,
        grants_control_opponent_ability_when_equipped=(
            grants_control_opponent_ability
        ),
    )
    if bonus_per_controlled is not None:
        printing = replace(
            printing, equipment_bonus_per_controlled=bonus_per_controlled,
        )
    if has_graveyard_triggers:
        printing = replace(printing, has_graveyard_triggers=True)
    if granted_triggers:
        printing = replace(
            printing, equipment_granted_triggers=tuple(granted_triggers),
        )
    return apply_board_fields(printing, roto_fields)
