"""Unified effect grammar shared by creature triggered abilities and
spells.

Each shape's regex matches the *normalized* effect phrase — no
source-subject prefix (``it`` for creature triggers or ``<Name>``
for spells), no trailing period, first letter lowercased. Surface
preprocessing (the ``_normalize_*`` helpers below) strips the
per-context noise before dispatch, so both contexts share one
effect grammar — adding a new effect here makes it available to
creature triggered abilities and to spells (single-line and
multi-line) at the same time.
"""

from __future__ import annotations

import re

from mtgz_ir.cards.base import (
    AddManaToPool,
    AlpharaelDreamingAcolyteDiscardEffect,
    AnticausalVestigeLeavesEffect,
    AstelliReclaimerETBEffect,
    CounterTargetSpell,
    CreateLanderAndScheduleSacrifice,
    CreateToken,
    CreateTokenPerMulticoloredPermanent,
    DealDamageAnyTarget,
    DealDamageEachOpponent,
    DealDamageEqualToSourcePowerAnyTarget,
    DealDamageToOpponentCreature,
    DealDamageToOpponentCreatureByArtifactCount,
    DealDamageToOpponentCreatureByCardsInHand,
    DealDamageToTargetCreature,
    DealDamageToTargetCreatureSpacecraftBoosted,
    DealDamageToTargetCreatureWithExcessLanderRider,
    DealDamageToTargetOpponent,
    DealDamageToTargetTappedCreature,
    DealNDamageToEachCreature,
    DefendingPlayerLosesHalfLifeRoundedUp,
    DepressurizeEffect,
    DestroyAllCreatures,
    DestroyTargetArtifact,
    DestroyTargetArtifactEnchantmentOrFlyingCreature,
    DestroyTargetArtifactOrTappedCreature,
    DestroyTargetCreature,
    DestroyTargetLand,
    DestroyTargetNoncreaturePermanent,
    DestroyTargetNonlandPermanent,
    DestroyTargetNonlandPermanentWithLanderForController,
    DestroyUpToOneTargetNonlandPermanent,
    DiscardHandThenDrawN,
    DiscardSelf,
    DoubleTargetCreaturesPowerUEOT,
    DrawCardPerMulticoloredPermanent,
    DrawCards,
    DrawCardsEqualToResolvingCombatDamage,
    EachOpponentDiscards,
    EachOpponentExilesOwnCreature,
    EachOpponentLosesLife,
    EachOpponentMillsCards,
    EachOpponentSacrificesCreature,
    EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage,
    EachPlayerDraws,
    Effect,
    EnduringDiesReturnAsEnchantmentEffect,
    CreateNTokensFromLastExileRecall,
    ExileAllArtifactsWithManaValueAtMost,
    ExileAllCreaturesAndSpacecraft,
    ExileAllMatchingUntilSourceLeaves,
    MoveEachExiledWithSourceToOwnersGraveyard,
    PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYourControl,
    ReanimateOneExiledWithSourceWithCounters,
    SacrificeSelf,
    SacrificeSelfPreservingExileLink,
    ExileEachOpponentsGraveyard,
    ExileTargetCardFromGraveyard,
    ExileTargetCreature,
    ExileTargetCreatureUntilSourceLeaves,
    ExileTargetCreatureWithPowerOrToughnessAtMost,
    ExileOpponentsNonlandPermanentUntilLeaves,
    BoilerbilgesRipperETBEffect,
    FamishedWorldsireETBEffect,
    FanaticOfTheHarrowingETBEffect,
    FocusFireEffect,
    GainControlOfTargetSliverPermanently,
    GainLife,
    GrantUnblockableUEOT,
    HouseCartographerSurvivalEffect,
    LookAtTopNRevealOrCounter,
    LookTopNPutOneInHandRestToGraveyard,
    LoseLife,
    ManifestDread,
    ModalChooseOne,
    ManifestDreadThenPutPlusOneCountersOnIt,
    MillTargetPlayer,
    MoveSelfCountersToTargetCreatureYouControl,
    OptionalManaCostThenEffect,
    OptionalPutCreatureFromHandOntoBattlefield,
    OptionalPutLandFromHandOntoBattlefield,
    OptionalSacAnotherForEffect,
    OptionalSacAnotherForPumpSelfUEOT,
    OptionalSacArtifactForEffect,
    OptionalSacLandOrLanderForBasicLandRamp,
    OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    OwnerPutsTargetWarpedExileCardOnBottomOfLibrary,
    PerigeeBeckonerDiesReturnTappedEffect,
    PossibilityTechnicianETBEffect,
    TerrapactIntimidatorETBEffect,
    PreventAllCombatDamageThisTurn,
    PumpCreaturesOpponentsControlUntilEOT,
    PumpCreaturesYouControlUntilEOT,
    PumpSelfUntilEOT,
    PumpTargetArtifactOrCreatureUntilEOT,
    PumpTargetCreatureUntilEOT,
    PumpTargetOtherCreatureByXOverXWhereXIsSourcePower,
    PutCardFromGraveyardOnBottomOfLibrary,
    PutChargeCountersOnTargetSpacecraftOrPlanetYouControl,
    PutStunCounterOnTappedCreature,
    PutPlusOneCounterOnCreatureYouControl,
    PutPlusOneCounterOnEachCreatureYouControl,
    PutPlusOneCounterOnEachOfUpToNTargetCreatures,
    PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower,
    PutPlusOneCounterOnOpponentsCreature,
    PutLoyaltyCounterOnSelf,
    PutPlusOneCounterOnSelf,
    PutPlusOneCounterOnTargetCreature,
    PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl,
    PutPoisonCountersOnTargetPlayer,
    ReturnAllCreaturesFromYourGraveyardToBattlefield,
    ReturnCreatureFromYourGraveyardToBattlefield,
    ReturnCreatureFromYourGraveyardToHand,
    ReturnUpToNCreaturesFromYourGraveyardToHand,
    ReturnUpToNNonlandPermanentCardsFromGraveyardToHand,
    ReturnTargetArtifactCardFromYourGraveyardToHand,
    ReturnTargetCreatureToOwnersHand,
    ReturnTargetPermanentYouControlToOwnersHand,
    ReturnUpToNTargetCreaturesToOwnersHands,
    SearchLibraryForBasicLandTapped,
    SearchLibraryForCard,
    SearchLibraryForEquipmentToBattlefield,
    SearchLibraryForGreenCreatureWithMVAtMostX,
    SeedshipImpactEffect,
    SetBasePTUntilEOT,
    SetBasePTConditionalUntilEOT,
    SetLifeToHalfStartingRoundedUp,
    ShuffleZonesIntoLibrary,
    TakeExtraTurnAfterThisOne,
    TerritorialBruntarLandfallEffect,
    TriggeredAbility,
    SelfMill,
    Sequence,
    SingularityRuptureEffect,
    Surveil,
    TapAndStunUpToOneTargetCreature,
    TapAnotherTargetCreature,
    TapTargetArtifactOrCreature,
    TapTargetOpponentPermanentAndStunIt,
    TargetOpponentDiscardsCasterChoice,
    TargetArtifactBecomesCreatureUEOT,
    DyadrineAttackEffect,
    EachOpponentDiscardsOrLosesLife,
    TargetOpponentLosesLife,
    TargetPlayerLosesLife,
    TannukEnsignLandfallEffect,
    UntapTargetOtherPermanent,
    ThreatenTargetCreatureUEOT,
    BiosynthicBurstEffect,
    SystemsOverrideEffect,
    CutPropulsionEffect,
    DiplomaticRelationsEffect,
    ReturnUpToNSmallCreaturesFromGraveyardWithCounters,
    MutinousMassacreEffect,
    CoordinatedClobberingEffect,
    ZeroPointBalladEffect,
    CreateXTokensOfTargetPermanentWithHasteEndStepSac,
)
from mtgz_ir.cards.parser.keywords import (
    _KEYWORD_LINES,
    _parse_granted_keyword_piece,
)
from mtgz_ir.cards.parser.keyword_actions import (
    _parse_keyword_action_effect,
    _synthesize_creature_token_effect,
)
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.bespoke import (
    ArabellaAttackEffect,
    CloseEncounterEffect,
    EnchantedCreatureFightsTargetEffect,
    PainForAllDamageEchoEffect,
    PainForAllETBEffect,
    RovingActuatorETBEffect,
    TerminalVelocityEffect,
)
from mtgz_ir.effects.combinators import (
    ConditionalIfYouControlSubtype,
    IfNot,
    IfYouDo,
    MayDo,
    SacrificeSelfUnlessVoidActive,
)
from mtgz_ir.effects.conditions import (
    ControlsAnotherCreatureWithPowerAtMost,
    PerDifferentlyNamedLand,
)
from mtgz_ir.effects.formulas import (
    CountControlledMatching,
    MVOfCostPaidObjectPlus,
)
from mtgz_ir.effects.generic import (
    SearchLibraryForLandCardToBattlefield,
    AddManaOfColorPerCount,
    AddOneOfManaCombinations,
    AddChargeCounterManaOfChosenColor,
    AddNManaOfChosenColor,
    BecomeType,
    CreateTokenCopyOfTargetCreatureYouControl,
    DestroyEnchantedPermanent,
    DrawCardsAndLoseLifeEqualToResolvingDamage,
    DrawCardsByConditionMultiplier,
    EachOpponentExilesCardFromHandWithCastPermission,
    ExileTopOfLibraryMayPlayThisTurn,
    ExileTopXOfLibraryMayPlayThisTurn,
    GrantTriggeredAbilityUEOT,
    LockOrUnlockDoorOfTargetRoomYouControl,
    MayHaveSourceDealDamageToTarget,
    PairedTargetPumpDebuffUEOT,
    PeekTopMayBattlefieldIfLandElseHand,
    ReanimateTargetCreatureCardFromDefendersGraveyard,
    ScheduleSelfSacAtNextEndStep,
    TapEnchantedPermanent,
    TargetOpponentLookHandMayDiscard,
    TargetOpponentSacrificesCreatureOtherThanEnchanted,
)
from mtgz_ir.effects.object_predicates import (
    IsArtifact,
    IsCreature as _IsCreature,
    IsEnchantment as _IsEnchantment,
    IsLegendary as _IsLegendary,
    IsPlaneswalker as _IsPlaneswalker,
)
from mtgz_ir.effects.predicates import Event, IsSelf


# Spelled-out small-cardinal lookup for oracle phrasings like
# ``mill three cards``. Scryfall consistently spells out 1–10 as words
# in mill-style effects (e.g. "mill three cards" on Lithic Hierograph).
# Larger amounts use digits, so the table tops out at ten — extend if a
# future card ships an oracle line with "mill twelve cards".
_SMALL_CARDINAL_WORDS: dict[str, int] = {
    "one": 1,
    "two": 2,
    "three": 3,
    "four": 4,
    "five": 5,
    "six": 6,
    "seven": 7,
    "eight": 8,
    "nine": 9,
    "ten": 10,
}
_SMALL_CARDINAL_PATTERN = "|".join(_SMALL_CARDINAL_WORDS)


# ``manifest dread`` (CR 701.55) and its repeated forms ``manifest
# dread N times`` / ``manifest dread twice`` / ``manifest dread three
# times`` / ``manifest dread X times`` — DSK's Phase 3 mechanic. The
# bare form (no count tail) maps to ``ManifestDread(count=1)``; the
# spelled-out / digit forms map to ``ManifestDread(count=N)``; and the
# ``X times`` form maps to ``ManifestDread(count=None)`` so the engine
# class reads ``state.resolving_x_value`` at resolve time. The
# ``twice`` synonym short-circuits to amount 2, matching the same
# convention used for repeated triggers elsewhere in this grammar.
_MANIFEST_DREAD = re.compile(
    r"^manifest dread"
    r"(?:"
    r" (twice)"  # bare ``twice`` (no trailing ``times``)
    r"|"
    r" (X|" + _SMALL_CARDINAL_PATTERN + r"|\d+) times"
    r")?$"
)


# "Prevent all combat damage that would be dealt this turn." — bare
# global static spell effect. Maps to ``PreventAllCombatDamageThisTurn``
# whose ``resolve`` flips ``state.prevent_combat_damage_this_turn`` and
# the engine's combat damage step honors at apply time. Currently only
# Darkness in the EoE pool today, but the surface form is a parameterless
# whole-sentence shape so it lives in the per-clause grammar (rather
# than the whole-spell exact-text tier) per the parser's
# generic-templates-over-literals guidance — adding a future card with
# a similar parameterless global effect lands its own neighboring
# recognizer here.
_PREVENT_ALL_COMBAT_DAMAGE_THIS_TURN = re.compile(
    r"^prevent all combat damage that would be dealt this turn$"
)


# ``manifest dread, then put a +1/+1 counter on that creature`` (and the
# spelled-out / digit ``N +1/+1 counters`` plural variant) — a single
# coupled primitive in the engine (``ManifestDreadThenPutPlusOneCounters
# OnIt``) where ``that creature`` refers to the just-manifested face-
# down 2/2 (NOT a chosen target). The two halves can't be parsed as a
# compound ``Sequence`` because the second half's ``that creature``
# pronoun is anaphoric to the manifest's output, which the splitter
# can't reconstruct from per-piece parses; instead the whole phrase
# matches as one regex and lands on the dedicated engine class. Slimy
# Aquarium (``a +1/+1 counter`` → counter_amount=1) and Weight Room
# (``three +1/+1 counters`` → counter_amount=3) are the EoE/Duskmourn
# Room printings that ship this surface; the singular/plural arms are
# a single regex via ``a |N `` alternation so a future printing with
# a different counter amount lands on the same primitive without
# further grammar work.
_MANIFEST_DREAD_THEN_PUT_PLUS_ONE_COUNTERS_ON_THAT_CREATURE = re.compile(
    rf"^manifest dread, then put "
    rf"(?:a \+1/\+1 counter|"
    rf"(\d+|{_SMALL_CARDINAL_PATTERN}) \+1/\+1 counters)"
    rf" on that creature$"
)


_DEAL_DAMAGE_ANY_TARGET = re.compile(
    r"^deals (\d+) damage to any target$"
)
# "<source> deals damage equal to its power to any target" — Infernal
# Phantom's dies trigger body in DSK (after the ``it `` source-subject
# strip in ``_normalize_creature_effect_phrase``).  Also accepts the
# ``it `` pronoun prefix so that compound effects split by ``, then ``
# in activated-ability context (Rust Harvester's
# ``Put a +1/+1 counter on this creature, then it deals damage equal
# to its power to any target``) also match after the compound splitter
# hands the second piece directly to ``_parse_effect`` without the
# creature-trigger normalizer's ``it ``-strip.  The ``(?:it )?``
# optional group mirrors the same guard on ``_DEAL_DAMAGE_TARGET_CREATURE``
# (which already accepts ``it deals N damage to target creature``).
# Maps to ``DealDamageEqualToSourcePowerAnyTarget`` whose resolve reads
# the source's effective power at fire time and routes through
# ``_apply_damage`` against the chosen target. Distinct from the bare
# ``deals N damage to any target`` shape above (which carries a literal
# integer amount); this primitive uses the source's power as a dynamic
# integer. Same legal-target pool either way.
_DEAL_DAMAGE_EQUAL_TO_SOURCE_POWER_ANY_TARGET = re.compile(
    r"^(?:it )?deals damage equal to its power to any target$"
)
_DEAL_DAMAGE_TARGET_CREATURE = re.compile(
    # Optional ``it `` prefix accepts the activated-ability surface
    # where the source is referred to by pronoun — Bear Trap's
    # ``It deals 3 damage to target creature.`` body uses ``It`` for
    # the source artifact, and the activated normalizer lowercases
    # the first letter but doesn't strip the pronoun (unlike the
    # creature-trigger normalizer's ``it ``-strip). Same shape as
    # the ``(?:this creature )?`` prefix on
    # ``_DEAL_DAMAGE_TARGET_OPPONENT`` / ``_DEAL_N_DAMAGE_EACH_OPPONENT`` /
    # ``_DEAL_N_DAMAGE_EACH_CREATURE``.
    #
    # The optional ``(?: or planeswalker)?`` tail accepts the surface
    # ``deals N damage to target creature or planeswalker`` (Scorching
    # Dragonfire) where the planeswalker scope is a printed widening
    # the engine doesn't model separately yet — the planeswalker pool
    # in EoE is sparse enough that creature-only targeting covers all
    # engine-modeled permanents today. Dropping the planeswalker scope
    # follows the same parser-is-right precedent as Archenemy's Charm
    # mode 1 (``ExileTargetCreature()`` for ``Exile target creature or
    # planeswalker``).
    r"^(?:it )?deals (\d+) damage to target creature(?: or planeswalker)?$"
)
# "<source> deals N damage to up to one target creature [or planeswalker]"
# — Pinnacle Kill-Ship's ETB body (``it deals 10 damage to up to one
# target creature``) and Dawnsire's tier-gated attack trigger (``this
# creature deals 100 damage to up to one target creature or planeswalker``).
# Mirror of the bare ``deals N damage to target creature`` shape with the
# ``up to one`` qualifier promoting the target count range from 1..1 to
# 0..1; maps to ``DealDamageToTargetCreature(amount=N, optional=True)``
# whose engine class flips ``requires_target`` / ``target_count_range``
# accordingly and resolves as a no-op when no target is chosen.
#
# The ``(?:it )?`` prefix is the creature-trigger source-pronoun;
# the ``(?:this creature )?`` prefix accepts the surface where the trigger
# body names the source explicitly (e.g. after
# ``_substitute_self_name`` rewrites ``Dawnsire`` to ``this creature``
# — the normalizer only strips the two-character ``it `` pronoun, not
# the longer ``this creature`` form).
#
# The ``(?: or planeswalker)?`` tail follows the same parser-is-right
# precedent as the bare ``deals N damage to target creature`` regex:
# planeswalkers aren't modeled by the engine today, so the scope
# widening is accepted syntactically but resolved against creature-only
# targets via ``DealDamageToTargetCreature``.
_DEAL_DAMAGE_UP_TO_ONE_TARGET_CREATURE = re.compile(
    r"^(?:it |this creature )?deals (\d+) damage to up to one target creature"
    r"( or planeswalker)?$"
)
# "<source> deals N damage to target creature an opponent controls" —
# Nova Hellkite's ETB. Maps to ``DealDamageToOpponentCreature(amount=N)``
# whose ``legal_targets`` filters to creatures NOT controlled by the
# resolving controller. The ``deals`` form (no ``it`` / ``you`` subject)
# is what survives both creature-trigger normalization (the ``it ``
# prefix-strip) and spell normalization (the ``<Name> `` prefix-strip).
# Tried BEFORE the bare ``deals N damage to target creature`` regex
# which has a ``$`` anchor — phrases that end at ``creature`` with no
# qualifier match the bare form, while phrases with the ``an opponent
# controls`` tail land here.
#
# The optional ``this Room `` prefix accepts the surface where a
# Duskmourn Room half names itself as the damage source — Glassworks'
# door trigger prints ``this Room deals 4 damage to target creature an
# opponent controls`` (the ``this Room`` self-reference is the Room
# equivalent of the ``this creature`` self-reference on creature-source
# triggers). Same legal-target pool either way; the source identity
# check happens through the trigger's ``IsSelf()`` predicate, not the
# effect body.
_DEAL_DAMAGE_TARGET_OPPONENT_CREATURE = re.compile(
    r"^(?:this Room )?deals (\d+) damage to target creature "
    r"an opponent controls$"
)
# "[this Room ]deals damage equal to <count-source> to target creature
# an opponent controls" — Warmaker Gunship (artifacts you control,
# from EoE) and Roaring Furnace (cards in your hand, from DSK Room
# half). Parameterized over the count-source phrase via a captured
# group; the ``_DEAL_DAMAGE_COUNT_SOURCE_DISPATCH`` mapping below
# routes the captured surface text to the matching engine class.
# Same legal-target pool (creatures NOT controlled by the resolving
# controller) as the bare ``deals N damage to target creature an
# opponent controls`` shape above; differs only in the dynamic
# damage amount tied to a runtime count instead of a printed integer.
#
# The optional ``this Room `` prefix accepts the Duskmourn Room-half
# surface where the source names itself as ``this Room`` (mirrors the
# ``(?:this Room )?`` precedent on the integer-amount sibling above).
# Same source-identity check via the trigger's ``IsSelf()`` predicate;
# the prefix is purely surface noise that the regex consumes.
_DEAL_DAMAGE_BY_COUNT_TARGET_OPPONENT_CREATURE = re.compile(
    r"^(?:this Room )?deals damage equal to the number of "
    r"(artifacts you control|cards in your hand) "
    r"to target creature an opponent controls$"
)
# Maps the captured count-source surface text to its engine class.
# Adding a new count source (e.g. ``enchantments you control``) is
# one entry here plus extending the regex's alternation above — no
# new dispatch site required. Both engine classes share the
# ``target creature an opponent controls`` legal-target filter; they
# differ only in WHICH count drives the damage amount at resolve
# time (artifacts on the battlefield vs cards in hand).
_DEAL_DAMAGE_COUNT_SOURCE_DISPATCH: dict[str, type] = {
    "artifacts you control": DealDamageToOpponentCreatureByArtifactCount,
    "cards in your hand": DealDamageToOpponentCreatureByCardsInHand,
}
# "<source> deals X damage to target attacking or blocking creature,
# where X is N plus the number of creatures and/or Spacecraft you
# control." — Focus Fire `{R}` Sorcery: variable-X burn whose X is a
# fixed integer base plus a count of friendly creatures and Spacecraft
# at resolve time. The capture group is the integer base ``N`` (matches
# Focus Fire's printed ``2``); maps to ``FocusFireEffect(base=N)``
# whose ``_amount`` walks ``state.battlefield`` for friendly creatures
# and Spacecraft at resolve time.
#
# Currently only Focus Fire in the EoE pool fits this shape, but the
# regex captures the integer base as a parameter so a future printing
# with a different base (``where X is 3 plus ...`` etc.) lands on the
# same primitive without further grammar work — the
# parameterized-recognizer-preferred form per parser CLAUDE.md.
# Lives in the per-clause grammar (rather than the whole-spell exact-
# text tier) to claim future cards with this shape automatically.
_DEAL_DAMAGE_X_BASE_PLUS_FRIENDLY_CREATURE_OR_SPACECRAFT_COUNT = re.compile(
    r"^deals X damage to target attacking or blocking creature, "
    r"where X is (\d+) plus the number of creatures and/or "
    r"Spacecraft you control$"
)
# "<source> deals N damage to target creature. If excess damage was
# dealt this way, create a Lander token." — Orbital Plunge `{3}{R}`
# Sorcery: a fixed-amount burn whose Lander-creation rider fires only
# when the damage exceeded the target's lethal threshold (overkill).
# Two-sentence shape: the leading sentence is the bare damage; the
# trailing ``If excess damage was dealt this way, create a Lander
# token.`` clause is an anaphoric rider that doesn't parse as a
# standalone effect (no per-clause primitive recognizes the back-
# reference to the damage just dealt). The recognizer matches the
# unsplit two-sentence body verbatim — ``_parse_spell_oracle_text``
# tries the unsplit clause first via ``_parse_spell_effect_phrase``
# before falling through to the per-sentence splitter, so this regex
# claims Orbital Plunge before the splitter can carve the rider into
# an unparseable piece.
#
# The capture group is the integer damage amount ``N`` (matches
# Orbital Plunge's printed ``6``); maps to
# ``DealDamageToTargetCreatureWithExcessLanderRider(amount=N)`` whose
# resolve step deals the damage and gates Lander creation on the
# excess-damage check at resolve time. Currently only Orbital Plunge
# in the EoE pool fits this shape, but the regex captures the integer
# amount as a parameter so a future printing with a different damage
# value lands on the same primitive without further grammar work —
# the parameterized-recognizer-preferred form per parser CLAUDE.md.
_DEAL_DAMAGE_N_TARGET_CREATURE_WITH_EXCESS_LANDER_RIDER = re.compile(
    r"^deals (\d+) damage to target creature\. "
    r"If excess damage was dealt this way, create a Lander token$"
)
# "<source> deals N damage to target creature. It deals M damage
# instead if you control a Spacecraft." — Invasive Maneuvers `{1}{R}`
# Instant: a fixed-amount burn with a Spacecraft-state boost rider.
# Two-sentence shape: the leading sentence is the bare damage; the
# trailing ``It deals M damage instead if you control a Spacecraft.``
# clause is an anaphoric rider that doesn't parse as a standalone
# effect (the ``It deals M damage instead`` substitution-syntax has no
# per-clause primitive). The recognizer matches the unsplit two-
# sentence body verbatim — ``_parse_spell_oracle_text`` tries the
# unsplit clause first via ``_parse_spell_effect_phrase`` before
# falling through to the per-sentence splitter, so this regex claims
# Invasive Maneuvers before the splitter can carve the rider into an
# unparseable piece.
#
# Two capture groups: the base damage ``N`` (matches Invasive
# Maneuvers's printed ``3``) and the boosted damage ``M`` (matches
# the printed ``5``); maps to
# ``DealDamageToTargetCreatureSpacecraftBoosted(base=N, boosted=M)``
# whose resolve step picks ``base`` vs ``boosted`` by inspecting the
# controller's battlefield for a Spacecraft. Currently only Invasive
# Maneuvers in the EoE pool fits this shape, but the regex captures
# both integer amounts as parameters so a future printing with
# different base/boosted values lands on the same primitive without
# further grammar work — the parameterized-recognizer-preferred form
# per parser CLAUDE.md.
_DEAL_DAMAGE_N_TARGET_CREATURE_M_IF_SPACECRAFT = re.compile(
    r"^deals (\d+) damage to target creature\. "
    r"It deals (\d+) damage instead if you control a Spacecraft$"
)
# "<source> deals N damage to target opponent" — Biotech Specialist's
# sacrifice-an-artifact trigger body. Damage targets a player rather
# than a creature; ``DealDamageToTargetOpponent(amount=N)`` filters
# the legal-target pool to opponents (not the source's controller).
#
# The optional ``this creature `` prefix accepts the surface where the
# source is named explicitly inside the trigger body. Scryfall prints
# this form when the trigger's CONDITION already uses ``you`` as the
# subject (Biotech Specialist's ``Whenever you sacrifice an artifact``
# leads into ``this creature deals 2 damage to target opponent`` — the
# explicit ``this creature`` resolves what would otherwise be an
# ambiguous ``it`` reference). The
# ``_normalize_creature_effect_phrase`` strip only consumes ``it ``,
# so we accept the longer subject form here at the regex level
# rather than expanding the normalizer (which would also strip
# ``this creature`` from phrases where it's a self-object reference
# in primitives like ``put a +1/+1 counter on this creature``).
#
# Tried BEFORE the bare ``deals N damage to any target`` regex
# because the ``$`` anchor on the bare regex would not match the
# ``target opponent`` tail anyway, but ordering more-specific before
# more-general is the convention in this dispatcher.
_DEAL_DAMAGE_TARGET_OPPONENT = re.compile(
    r"^(?:this creature )?deals (\d+) damage to target opponent$"
)
# Controller draws cards. The optional ``you `` prefix surfaces in
# compound bodies like ``you draw a card and lose 1 life`` (Voidforged
# Titan, Decode Transmissions, Elegy Acolyte) — the leading ``you`` is
# carried over from the original sentence's subject when the printing
# joins two effects with `` and `` and the splitter would otherwise
# leave the first piece reading as ``you draw a card``. Mirrors the
# ``(?:you )?`` shape on ``_LOSE_N_LIFE`` (the same ``you`` carry-over
# on the other half of the canonical pair) — the ``you`` is purely
# surface noise because the controller is already implicit in every
# effect's resolve signature.
_DRAW_A_CARD = re.compile(r"^(?:you )?draw (?:a|another) card$")
# ``draw N cards`` — bare-amount draw effect. The ``N`` accepts either
# a digit or a spelled-out small cardinal (Glimmerburst's printed
# ``Draw two cards.`` is what introduced the cardinal-word form to
# this primitive; future ``draw three cards`` / ``draw four cards``
# printings parse without further grammar work). The captured token
# routes through ``_parse_amount`` to land as an integer on the engine
# class. Mirrors the digit-or-spelled-cardinal shape on the existing
# ``_MILL_N_CARDS`` and ``_OPTIONAL_DISCARD_THEN_DRAW`` primitives.
_DRAW_N_CARDS = re.compile(
    rf"^(?:you )?draw (\d+|{_SMALL_CARDINAL_PATTERN}) cards$"
)
# "Draw that many cards" — the variable-amount form whose count is
# resolved at fire time from ``state.resolving_combat_damage_amount``.
# Surfaces inside a triggered-ability body whose CONDITION names a
# combat-damage event; the ``that many`` pronoun is anaphoric to the
# damage amount latched on the trigger source. Fear of Failed Tests
# (Duskmourn) is the EoE/Duskmourn surface form: ``Whenever this
# creature deals combat damage to a player, draw that many cards.``
# Starwinder (EoE) prints the same shape with the ``you may`` may-rider:
# ``Whenever a creature you control deals combat damage to a player,
# you may draw that many cards.`` The ``may`` is engine-flattened to
# always-yes inside ``DrawCardsEqualToResolvingCombatDamage`` (see the
# class docstring), so the parser accepts the may-prefix as syntactic
# sugar that lands on the same primitive — same precedent as other
# OptionalXyz simplifications elsewhere in this engine.
# Maps to the existing ``DrawCardsEqualToResolvingCombatDamage``
# engine class — same primitive Starwinder uses via the bespoke
# table. The optional ``you `` / ``you may `` prefixes mirror the
# bare draw shapes above (the carry-over from compound ``you draw``
# sentences); both are accepted to keep the regex shape uniform with
# its bare-amount sibling.
_DRAW_THAT_MANY_CARDS = re.compile(
    r"^(?:you (?:may )?)?draw that many cards$"
)
# "Draw X cards, where X is the number of differently named lands you
# control." — variable-amount draw whose count comes from a structural
# ``Condition``'s ``multiplier``. All-Fates Scroll's sac-draw activated
# body (``{7}, {T}, Sacrifice this artifact: Draw X cards, where X is
# the number of differently named lands you control.``) is the only
# EoE/DSK printing today; broader X-draw grammar (a parameterized
# ``where X is <count phrase>`` family) lands when a second printing
# ships with a different count-phrase mate (``Per controlled <subtype>``
# etc.). For now this is a literal-phrase recognizer dispatching to the
# generic ``DrawCardsByConditionMultiplier(condition=
# PerDifferentlyNamedLand(), description_fragment=…)`` engine class.
_DRAW_X_CARDS_PER_DIFFERENTLY_NAMED_LAND = re.compile(
    r"^draw X cards, where X is the number of differently named lands "
    r"you control$"
)
_YOU_GAIN_N_LIFE = re.compile(r"^you gain (\d+) life$")
# "Put a +1/+1 counter on target creature" — bare-target counter
# (Honor-style). The optional ``attacking `` qualifier narrows the
# legal-target pool to currently-attacking creatures (Hospital
# Room's ``put a +1/+1 counter on target attacking creature`` door
# trigger). When present the captured group flips
# ``attacking_only=True`` on the resulting
# ``PutPlusOneCounterOnTargetCreature`` — same engine flag the
# bespoke factory we're replacing wired in. Generalized rather
# than split into a separate literal regex per the architectural
# guidance: a single parameterized recognizer covers both the bare
# Honor surface and Hospital Room's attacking-target form.
_PUT_PLUS_ONE_COUNTER_ON_TARGET_CREATURE = re.compile(
    r"^put a \+1/\+1 counter on target (attacking )?creature$"
)
# "Put a +1/+1 counter on target creature you control" — Rayblade
# Trooper's ETB. Restricted variant: legal targets are creatures the
# resolving controller controls. Maps to
# ``PutPlusOneCounterOnCreatureYouControl(amount=1)``. Tried BEFORE
# the bare ``target creature`` regex so the more-specific
# ``you control`` shape wins on phrases that carry it.
_PUT_PLUS_ONE_COUNTER_ON_TARGET_CREATURE_YOU_CONTROL = re.compile(
    r"^put a \+1/\+1 counter on target creature you control$"
)
# "Put a +1/+1 counter on target creature an opponent controls" —
# Sunset Saboteur's attack trigger. Mirror image of the
# ``you control`` restriction above: legal targets are creatures NOT
# controlled by the resolving controller. Maps to
# ``PutPlusOneCounterOnOpponentsCreature(amount=1)``. Tried BEFORE
# the bare ``target creature`` regex so the more-specific
# ``an opponent controls`` shape wins on phrases that carry it.
_PUT_PLUS_ONE_COUNTER_ON_TARGET_OPPONENT_CREATURE = re.compile(
    r"^put a \+1/\+1 counter on target creature an opponent controls$"
)
# Parameterized to handle both "put a +1/+1 counter on this creature"
# (amount=1) and "put N +1/+1 counters on this creature" (amount=N).
# The amount capture accepts ``a`` (= 1, singular "counter" tail),
# spelled-out small cardinals (``two``, ``three``, etc., plural
# "counters" tail), and bare digits (plural "counters" tail). The
# singular/plural tail distinction is enforced by alternation so a
# grammatically ill-formed phrase like "put two +1/+1 counter on this
# creature" (missing the 's') doesn't match. Used by Blade of the
# Swarm (two), Terrapact Intimidator (two), Silent Hallcreeper (two),
# and Possessed Goat (three via activated ability).
_PUT_PLUS_ONE_COUNTER_ON_THIS_CREATURE = re.compile(
    rf"^put (a|\d+|{_SMALL_CARDINAL_PATTERN}) \+1/\+1 counters? on this creature$"
)
# Tezzeret, Cruel Captain's artifact-enters trigger body. The phrase
# routes through the same name-substitution pass as the +1/+1 counter
# variant above (``_substitute_self_name`` rewrites ``Tezzeret`` to
# ``this creature`` before lookup), so the planeswalker builder shares
# the unified trigger-body grammar with creature builders. Maps to
# ``PutLoyaltyCounterOnSelf(amount=1)``; the engine class reads
# ``state.resolving_source`` at resolve time and bumps its
# ``loyalty_counters`` field.
_PUT_LOYALTY_COUNTER_ON_THIS_CREATURE = re.compile(
    r"^put a loyalty counter on this creature$"
)
# "Put a +1/+1 counter on each of up to N target creatures" —
# multi-target counter spread (Weftblade Enhancer's ETB uses ``two``).
# The amount tail accepts spelled-out small cardinals (the only form
# that ships in EoE today; ``two`` for Weftblade) plus bare digits so
# a future ``up to three target creatures`` printing parses without
# further grammar work. Maps to
# ``PutPlusOneCounterOnEachOfUpToNTargetCreatures(max_n=N)``.
_PUT_PLUS_ONE_COUNTER_ON_EACH_OF_UP_TO_N_TARGET_CREATURES = re.compile(
    rf"^put a \+1/\+1 counter on each of up to "
    rf"(\d+|{_SMALL_CARDINAL_PATTERN}) target creatures$"
)
# "Put its counters on target creature you control." — Dockworker Drone's
# dies trigger. The ``its`` pronoun refers to the source creature (dying
# permanent). Maps to ``MoveSelfCountersToTargetCreatureYouControl()``,
# whose engine class reads ``state.resolving_source.plus_one_counters``
# and transfers them to the chosen target. Surface form is post-
# normalization (the ``it `` prefix-strip in ``_normalize_creature_
# effect_phrase`` doesn't fire because the phrase starts with the verb
# ``put``, not a pronoun subject — so the ``its`` here survives intact).
_PUT_ITS_COUNTERS_ON_TARGET_CREATURE_YOU_CONTROL = re.compile(
    r"^put its counters on target creature you control$"
)
# "Target creature gets -N/-0 until end of turn. Then if that creature's
# power is 0 or less, destroy it." — Depressurize `{1}{B}` Instant: a
# -power UEOT debuff paired with a state-based conditional destroy on the
# same target. The two-sentence shape is matched as a single literal regex
# because the second sentence's ``that creature`` is anaphoric to the
# debuff target, and the engine class ``DepressurizeEffect(power_penalty=N)``
# folds the debuff and conditional destroy into one atomic resolve step.
# The compound-effect splitter fails because "Then if that creature's
# power is 0 or less, destroy it" doesn't parse as a standalone effect.
#
# Capture group 1 is the negative power bonus (e.g. ``-3`` for
# Depressurize); the toughness bonus is always ``-0`` (no toughness
# change), and the engine class ignores the toughness group. The captured
# integer is negated to derive ``power_penalty``: Depressurize's
# ``-3/-0`` → ``power_penalty=3``.
#
# Currently only Depressurize in the EoE/DSK pool fits this shape; the
# regex captures the power penalty as a parameter so a future printing
# with a different debuff value (e.g. ``-4/-0``) lands on the same
# primitive without further grammar work — the parameterized-recognizer-
# preferred form per parser CLAUDE.md.
_DEPRESSURIZE = re.compile(
    r"^target creature gets (-\d+)/(-0) until end of turn\. "
    r"Then if that creature's power is 0 or less, destroy it$"
)
_DESTROY_TARGET_CREATURE = re.compile(r"^destroy target creature$")
# "Destroy up to one other target creature. If that creature wasn't
# dealt damage this turn, its controller draws N cards." — Faller's
# Faithful's ETB. Single primitive with an embedded follow-up
# sentence; the captured amount lands on
# ``DestroyTargetCreature(draw_if_undamaged_amount=N)`` along with
# ``optional=True`` (the ``up to one`` fragment) and
# ``exclude_self=True`` (the ``other`` fragment). The amount tail
# accepts spelled-out small cardinals (``two`` is what Faller's
# prints) plus bare digits for a future printing.
_DESTROY_UP_TO_ONE_OTHER_TARGET_CREATURE_DRAW_IF_UNDAMAGED = re.compile(
    rf"^destroy up to one other target creature\. "
    rf"If that creature wasn't dealt damage this turn, "
    rf"its controller draws (\d+|{_SMALL_CARDINAL_PATTERN}) cards$"
)
# "Destroy target creature or Spacecraft." — Embrace Oblivion's
# destroy clause. Maps to ``DestroyTargetCreature(or_spacecraft=True)``
# whose ``legal_targets`` extends the bare ``destroy target creature``
# pool to also include unstationed Spacecraft (which aren't
# currently creatures). The ``Spacecraft`` capitalization is verbatim
# from the oracle text — the spell-effect normalizer only lowercases
# the first letter of the phrase, so inner type-name capitalization
# survives.
_DESTROY_TARGET_CREATURE_OR_SPACECRAFT = re.compile(
    r"^destroy target creature or Spacecraft$"
)
# "Destroy target creature or enchantment." — Withering Torment
# (Duskmourn). Maps to ``DestroyTargetCreature(or_enchantment=True)``
# whose ``legal_targets`` extends the bare ``destroy target creature``
# pool to also include any enchantment (Aura subtype or not). The
# enchantment branch emits ``("enchantment", id)`` tuples; the resolve
# accepts either kind and finds the object by id. The lowercase
# ``enchantment`` is the printed surface — unlike ``Spacecraft`` which
# is capitalized in oracle text, ``enchantment`` is lowercased.
_DESTROY_TARGET_CREATURE_OR_ENCHANTMENT = re.compile(
    r"^destroy target creature or enchantment$"
)
_COUNTER_TARGET_SPELL = re.compile(r"^counter target spell$")
# "Counter target spell unless its controller pays {N}." — Mana-Leak
# shape (Divert Disaster, Don't Make a Sound).  The engine doesn't yet
# model the pay-cost-or-counter branching; the ``unless its controller
# pays {N}`` clause is dropped, mapping to bare ``CounterTargetSpell()``.
# Same parser-is-right simplification the static registry already makes
# (both Divert Disaster and Don't Make a Sound ship ``CounterTargetSpell()``
# in the bespoke tier). Tried BEFORE the bare counter-spell regex so
# the more-specific ``unless … pays`` tail matches first; the bare
# regex's ``$`` anchor wouldn't match this longer form anyway.
# The ``If they do,`` consequent sentence is a separate clause dropped
# by ``_is_unimplemented_secondary_rider`` (``_IF_THEY_DO_RIDER_RE``).
_COUNTER_TARGET_SPELL_UNLESS_PAYS = re.compile(
    r"^counter target spell unless its controller pays \{[^}]+\}$"
)
# "Counter target sorcery spell." — Warping Wail's mode 2. Same engine
# class as the bare counterspell, with ``filter_types=frozenset({"Sorcery"})``
# to gate which stack items are legal targets. Tried BEFORE the bare
# ``counter target spell`` regex so the more-specific filter-typed shape
# wins; the bare regex's ``$`` anchor won't match this tail anyway.
# Currently only Warping Wail in the pool fits this shape; the regex
# is a literal (no capture groups) because the filter is a fixed
# ``{"Sorcery"}`` frozenset with no varying piece. A future
# ``counter target instant spell`` or ``counter target creature spell``
# would land as separate sibling literals here (one filter per
# literal), following the precedent of the existing
# ``_COUNTER_TARGET_ARTIFACT_OR_ENCHANTMENT_SPELL`` literal below.
_COUNTER_TARGET_SORCERY_SPELL = re.compile(r"^counter target sorcery spell$")
# "Counter target artifact or enchantment spell." — Annul. Same engine
# class as the bare counterspell, just with a non-empty ``filter_types``
# frozenset that gates which stack items are legal targets. Tried before
# the bare ``counter target spell`` regex below because the bare regex
# requires the phrase to end at ``spell`` with no qualifier.
_COUNTER_TARGET_ARTIFACT_OR_ENCHANTMENT_SPELL = re.compile(
    r"^counter target artifact or enchantment spell$"
)
# "Counter target creature or enchantment spell." — Get Out's mode 1
# (Duskmourn). Same engine class as the bare counterspell, with a
# ``filter_types=frozenset({"Creature", "Enchantment"})`` gate. Tried
# BEFORE the bare ``counter target spell`` regex so the more-specific
# filter shape wins. Both shapes return the same engine class, just
# with different ``filter_types`` frozensets. Sibling to the
# ``artifact or enchantment`` literal above; a future "counter target
# instant spell" or "counter target planeswalker spell" would land as
# additional sibling literals (per the deferred-generalization comment
# on ``_COUNTER_TARGET_SORCERY_SPELL``).
_COUNTER_TARGET_CREATURE_OR_ENCHANTMENT_SPELL = re.compile(
    r"^counter target creature or enchantment spell$"
)
_EXILE_TARGET_CREATURE = re.compile(r"^exile target creature$")
# "Exile target creature or planeswalker." — Archenemy's Charm mode 1.
# Planeswalkers aren't fully modeled in the EoE pool today; maps to the
# same ``ExileTargetCreature()`` engine class used by the bare-creature
# form (same "or planeswalker" scope drop as the static registry's
# Archenemy's Charm mode 1 comment notes). Tried BEFORE the bare
# ``exile target creature`` regex so the more-specific ``or
# planeswalker`` tail wins; the bare regex's ``$`` anchor would never
# match this longer form anyway, but ordering more-specific before
# more-general is the convention here.
# Only Archenemy's Charm fits this exact surface today; the recognizer
# is a literal-line match (no capture groups) since no varying piece
# exists. A future card "exile target creature or enchantment" would
# need a separate literal or a parameterized regex capturing the disjunct
# type.
_EXILE_TARGET_CREATURE_OR_PLANESWALKER = re.compile(
    r"^exile target creature or planeswalker$"
)
# "Exile target creature with power or toughness N or less." — Warping
# Wail's mode 1. Maps to ``ExileTargetCreatureWithPowerOrToughnessAtMost
# (max_value=N)`` whose combined ``power ≤ N OR toughness ≤ N`` filter
# gates both target selection and the at-resolve recheck. The captured
# group is the max value (digit only — the only form printed today).
# Currently only Warping Wail in the pool fits this shape; the regex
# captures the max value as a parameter so a future printing with a
# different threshold (e.g. ``power or toughness 2 or less``) lands on
# the same primitive without further grammar work.
# Tried BEFORE the bare ``exile target creature`` regex so the power/
# toughness-threshold tail wins; the bare regex's ``$`` anchor would
# never match this longer form anyway, but ordering more-specific
# before more-general is the convention here.
_EXILE_TARGET_CREATURE_WITH_POWER_OR_TOUGHNESS_AT_MOST = re.compile(
    r"^exile target creature with power or toughness (\d+) or less$"
)
# "Destroy all tapped creatures." / "Destroy all untapped creatures." —
# Split Up's modal mode lines. Two separate regexes (rather than one
# regex with a captured tap-state qualifier) because the matching engine
# class flag is a per-flag bool — cleaner to set the named bool from the
# matched regex than to dispatch on captured surface text. The bare
# ``destroy all creatures`` (untargeted Wrath) shape isn't used by any
# spell in the current pool, so it's not a regex here yet — add it as a
# sibling if/when one ships.
_DESTROY_ALL_TAPPED_CREATURES = re.compile(
    r"^destroy all tapped creatures$"
)
_DESTROY_ALL_UNTAPPED_CREATURES = re.compile(
    r"^destroy all untapped creatures$"
)
# "Destroy all creatures, then any number of target players each mill
# half their library, rounded down." — Singularity Rupture `{3}{U}{B}{B}`
# Sorcery: a board wrath followed by a multi-target half-library mill on
# any number of chosen players. Maps to the parameterless
# ``SingularityRuptureEffect`` engine class which folds the wrath and
# the per-target floor-divide mill into one resolve step (the cast-
# pipeline target-count range is 0..players-bounded, with each chosen
# target milling ``floor(library_size / 2)`` cards).
#
# Currently only Singularity Rupture in the EoE pool fits this shape, so
# the recognizer is a parameterless literal — broader generalization
# (e.g. ``destroy all creatures, then ... <any-mass-effect>``) is
# deferred until a second card with this two-clause wrath-then-multi-
# target-mass-effect shape exists. Sibling to the tapped/untapped
# wrath branches above; lives in the per-clause grammar (rather than
# the whole-spell exact-text tier) per the parser's
# generic-templates-over-literals guidance.
_SINGULARITY_RUPTURE = re.compile(
    r"^destroy all creatures, then any number of target players "
    r"each mill half their library, rounded down$"
)
# "Destroy all creatures with power N or greater." — Restricted Office's
# door trigger body in DSK ("destroy all creatures with power 3 or
# greater"). Parameterized: the captured group is the minimum power
# threshold (digit only — no card prints a spelled-out power bound on
# this surface). Maps to ``DestroyAllCreatures(min_power=N)`` whose
# ``effective_power`` walk at resolve time correctly accounts for
# layer-7 pump effects and +1/+1 counters when filtering victims.
# Sibling to the tapped/untapped branches above; sets a different
# engine-class flag (``min_power`` rather than the tap-state bools)
# so the dispatch is a separate ``re.match`` branch in the
# ``destroy``-family neighborhood.
_DESTROY_ALL_CREATURES_WITH_POWER_AT_LEAST = re.compile(
    r"^destroy all creatures with power (\d+) or greater$"
)
# "Exile all creatures and Spacecraft." — Beyond the Quiet wrath. Capital
# ``S`` on Spacecraft is preserved verbatim from the oracle text — the
# normalization layer only lowercases the first letter of the phrase, so
# inner type-name capitalization survives. Untargeted (mass) effect.
#
# Note this is matched here as a SINGLE-effect regex, before the
# compound-effect splitter would carve the phrase on `` and `` —
# ``_parse_compound_effect`` runs first in ``_parse_effect`` but punts
# (returns None) when any split-piece fails to parse, and ``Spacecraft``
# alone isn't a recognized effect, so the compound attempt rejects and
# falls through to the single-effect dispatcher where this regex wins.
_EXILE_ALL_CREATURES_AND_SPACECRAFT = re.compile(
    r"^exile all creatures and Spacecraft$"
)
# "Exile all artifacts with mana value N or less." — Ruinous Rampage's
# anti-artifact mode. The captured group is the max mana value (digit
# only — no card prints a spelled-out mana-value bound today). Maps to
# ``ExileAllArtifactsWithManaValueAtMost(max_mv=N)``.
_EXILE_ALL_ARTIFACTS_WITH_MANA_VALUE_AT_MOST = re.compile(
    r"^exile all artifacts with mana value (\d+) or less$"
)
_EACH_OPPONENT_LOSES_N_LIFE = re.compile(
    r"^each opponent loses (\d+) life$"
)
# "Each player draws a card" / "Each player draws N cards" — every
# player (active controller AND opponents) draws ``N`` cards.
# Friendly Teddy's ``When this creature dies, each player draws a
# card.`` is the EoE/Duskmourn surface form. ``a`` collapses to
# amount=1; digits and spelled-out small cardinals go through
# ``_parse_amount`` so a future ``each player draws three cards``
# printing parses without further grammar work. Maps to
# ``EachPlayerDraws(amount=N)`` whose ``resolve`` walks every living
# player and calls ``draw_n_cards``.
_EACH_PLAYER_DRAWS_N_CARDS = re.compile(
    rf"^each player draws "
    rf"(?:a card|(\d+|{_SMALL_CARDINAL_PATTERN}) cards)$"
)
# "Exile each opponent's graveyard" — untargeted graveyard wipe on
# every opponent (Dauntless Scrapbot's ETB compound first half).
# Maps to ``ExileEachOpponentsGraveyard()`` which sweeps every card
# in each opponent's graveyard into exile at resolve time.
_EXILE_EACH_OPPONENTS_GRAVEYARD = re.compile(
    r"^exile each opponent's graveyard$"
)
# "<source> deals N damage to each opponent." — mass burn aimed at
# every opponent (Ruinous Rampage's burn mode). The ``deals`` form
# (no ``it`` / ``you`` subject) is what Scryfall prints for spells
# that name themselves as the subject; the spell-effect normalizer
# strips the ``<Name>`` prefix before this regex sees the phrase, so
# the surface form here is exactly ``deals N damage to each opponent``.
# For creature-source triggered abilities the ``it`` prefix is also
# stripped first, leaving the same canonical ``deals …`` form.
#
# The optional ``this creature `` prefix accepts the surface where the
# trigger body names the source explicitly. Scryfall prints that form
# when the trigger CONDITION already uses a different subject (Weftstalker
# Ardent's ``Whenever another creature or artifact you control enters``
# leads into ``this creature deals 1 damage to each opponent`` — the
# explicit ``this creature`` resolves what would otherwise be an ambiguous
# ``it`` reference to the unrelated entering object). Mirrors the same
# ``(?:this creature )?`` shape on ``_DEAL_DAMAGE_TARGET_OPPONENT``
# (Biotech Specialist's ``Whenever you sacrifice an artifact, this
# creature deals 2 damage to target opponent``) — same justification:
# the normalizer only consumes the ``it `` pronoun, not the longer
# ``this creature`` self-object reference.
#
# The ``this Room `` prefix is the Duskmourn Room equivalent — Shattered
# Yard's end-step trigger prints ``this Room deals 1 damage to each
# opponent`` (the Room is the damage source; ``this Room`` reads the
# same way ``this creature`` does on a creature-source trigger). Same
# legal-target pool either way.
_DEAL_N_DAMAGE_EACH_OPPONENT = re.compile(
    r"^(?:this creature |this Room )?deals (\d+) damage to each opponent$"
)
# "<source> deals N damage to each creature." — untargeted board-wipe
# burn (Pyroclasm in Duskmourn; the same primitive surfaces inside
# Lithobraking's reflexive optional-sac sequence and Extinguisher
# Battleship's ETB sequence in EoE, where it's already exercised by
# the ``DealNDamageToEachCreature`` engine class). Mirrors the
# ``deals N damage to each opponent`` shape: ``deals`` form (no ``it``
# / ``you`` subject) is what Scryfall prints for spells that name
# themselves as the subject; the spell-effect normalizer strips the
# ``<Name>`` prefix before this regex sees the phrase. The optional
# ``this creature `` prefix accepts the surface where a creature-source
# trigger body names the source explicitly (same precedent as
# ``_DEAL_N_DAMAGE_EACH_OPPONENT`` and ``_DEAL_DAMAGE_TARGET_OPPONENT``
# — the ``it `` normalizer only consumes the bare pronoun).
#
# The optional ``this Spacecraft `` prefix accepts the surface where
# the source is a Spacecraft (Extinguisher Battleship's ETB body second
# sentence — ``Then this Spacecraft deals 4 damage to each creature.``;
# the leading ``Then `` connective is stripped by
# ``_split_effect_sentences`` before this regex sees the phrase).
# Mirrors the ``(?:this Room )?`` precedent on
# ``_DEAL_DAMAGE_TARGET_OPPONENT_CREATURE`` — same self-source pronoun
# pattern Rooms use, here applied to the Spacecraft type. Source
# identity is established by the enclosing trigger's ``IsSelf()``
# predicate, not the effect body.
_DEAL_N_DAMAGE_EACH_CREATURE = re.compile(
    r"^(?:this creature |this Spacecraft )?"
    r"deals (\d+) damage to each creature$"
)
# "Take an extra turn after this one." — Nexus of Fate's main effect.
# Maps to the parameterless ``TakeExtraTurnAfterThisOne`` engine class
# which schedules an additional turn for the resolving controller. The
# accompanying self-shuffle rider ("If Nexus of Fate would be put into a
# graveyard from anywhere, reveal Nexus of Fate and shuffle it into its
# owner's library instead.") is detected separately by
# ``_detect_shuffle_self_statics`` in ``spell_builder`` and wired as a
# SELF-scope StaticAbility carrying a ``ReplacementEffect(
# WOULD_GO_TO_GRAVEYARD, Replace(new_destination="library"))`` on the
# spell printing; the self-shuffle clause is dropped from the effect-
# clause stream by ``_is_self_shuffle_clause`` before the Sequence
# combinator runs, so this single-sentence recognizer claims the whole
# spell.
# Currently only Nexus of Fate in the EoE/DSK pool prints this exact
# surface today; the recognizer is a parameterless literal per parser
# CLAUDE.md (one card, no second example yet with a varying piece).
_TAKE_EXTRA_TURN_AFTER_THIS_ONE = re.compile(
    r"^take an extra turn after this one$"
)
# "Search your library for a green creature card with mana value X or
# less, put it onto the battlefield, then shuffle." — Green Sun's Zenith's
# main tutor clause. Maps to the parameterless
# ``SearchLibraryForGreenCreatureWithMVAtMostX`` engine class which reads
# ``state.resolving_x_value`` at resolve time to derive the mana-value cap
# and puts the found creature directly onto the battlefield. The spell's
# second sentence ("Shuffle Green Sun's Zenith into its owner's
# library.") is the next step of its ``Sequence``,
# ``ShuffleThisSpellIntoOwnersLibrary`` (``card_flow_grammar``).
# Currently only Green Sun's Zenith in the EoE pool prints this exact
# X-cost battlefield-tutor surface today; the recognizer is a
# parameterless literal per parser CLAUDE.md (one card, no second example
# yet). A future "search your library for a [color] creature card with
# mana value X or less, put it onto the battlefield, then shuffle" card
# would call for a parameterized regex capturing the color word and
# dispatching to the matching engine class.
_SEARCH_LIBRARY_GREEN_CREATURE_X_TO_BATTLEFIELD = re.compile(
    r"^search your library for a green creature card with mana value X "
    r"or less, put it onto the battlefield, then shuffle$"
)
# "Search your library for a(n) <filter-phrase> card, reveal it, put it
# into your hand, then shuffle." — parameterized tutor-to-hand shape.
# Captures the article (``a`` / ``an``) and the filter phrase so a
# single regex claims every card with this surface regardless of the
# filter type.  The filter phrase is dispatched through
# ``_SEARCH_LIBRARY_REVEAL_HAND_SHUFFLE_FILTER`` to resolve the
# ``SearchLibraryForCard.filter_name`` string and ``may_fail`` flag.
#
# Replaces the three former literal regexes:
#   * ``_SEARCH_LIBRARY_FOR_ARTIFACT_CARD``
#     (artifact, may_fail=True)
#   * ``_SEARCH_LIBRARY_FOR_BASIC_LAND_CARD``
#     (basic_land, may_fail=True)
#   * ``_SEARCH_LIBRARY_FOR_LEGENDARY_CREATURE_CARD``
#     (legendary_creature, may_fail=False)
# and also claims Demonic Counsel's base clause
# (Demon card, may_fail=False) once the per-clause Delirium-rider
# dropper removes the second line from the oracle text.
#
# The full reveal/hand/shuffle tail is required so a tutor with a
# different reward (onto-the-battlefield, into-graveyard) won't
# accidentally match.  Tutors that use ``you may`` before ``search``
# don't match this shape and remain unclaimed unless a separate
# recognizer claims them.
#
# Scryfall uses two wordings for the reveal step:
#   - ``reveal it`` — most tutors (artifact, basic land, legendary
#     creature, Demon …)
#   - ``reveal that card`` — Sliver Overlord's "Search your library for
#     a Sliver card, reveal that card, put it into your hand, then
#     shuffle." Both are semantically identical; the regex accepts
#     either form so the same filter dispatch covers both.
_SEARCH_LIBRARY_REVEAL_HAND_SHUFFLE = re.compile(
    r"^search your library for (an?) (.+) card, "
    r"reveal (?:it|that card), put it into your hand, then shuffle$"
)
# Maps the captured filter phrase to (filter_name, may_fail).  The
# ``may_fail`` is taken from the existing static-registry semantics:
#   - ``may_fail=True``  → original artifact / basic-land literals
#   - ``may_fail=False`` → legendary-creature literal (no "you may")
# Adding a new card whose filter phrase matches an existing entry is
# zero code; adding a new filter phrase is one line here plus
# confirmation the ``SearchLibraryForCard._matches`` dispatch handles
# the new filter_name.
_SEARCH_LIBRARY_REVEAL_HAND_SHUFFLE_FILTER: dict[str, tuple[str, bool]] = {
    # ``an artifact card`` — Scour for Scrap mode 1 and any future
    # artifact tutor in the reveal/hand/shuffle shape.
    "artifact": ("artifact", True),
    # ``a basic land card`` — Spineseeker Centipede's ETB tutor and
    # any future generic basic-land-to-hand tutor.
    "basic land": ("basic_land", True),
    # ``a legendary creature card`` — The Seriema's ETB body; no "you
    # may" rider so may_fail=False (forced fetch).
    "legendary creature": ("legendary_creature", False),
    # ``a Demon card`` — Demonic Counsel's base clause (the Delirium
    # upgrade is stripped as an unimplemented-mechanic rider before
    # this recognizer is tried); no "you may" rider so may_fail=False.
    "Demon": ("demon", False),
    # ``a Sliver card`` — Sliver Overlord's first activated ability:
    # "{3}: Search your library for a Sliver card, reveal that card,
    # put it into your hand, then shuffle." The ``may_fail=True``
    # flag preserves graceful behaviour when the library has no Slivers.
    "Sliver": ("sliver", True),
}
# "Search your library for a basic land card, put it onto the
# battlefield tapped, then shuffle." — Moldering Gym's door trigger
# (the tapped-onto-battlefield reward form). Same fetch-land payload
# as the Lander token's activated ability and the
# ``_FETCH_LAND_BODIES`` table in ``land_builder``; landing the
# surface here lets unified-grammar Room halves (and any future
# triggered/activated-ability surface that prints this exact
# sentence) claim through a single recognizer. Maps to
# ``SearchLibraryForBasicLandTapped()``. Distinct from the
# put-into-your-hand sibling above on the reward tail (battlefield-
# tapped vs hand-with-reveal); the two surfaces are mutually
# exclusive at the regex level so ordering between them doesn't
# matter. Literal-line recognizer with a single capture-less surface
# — Moldering Gym is the only printing in the EoE/DSK pool whose
# triggered-ability body prints this exact sentence today; broader
# grammar (parameterizing the count, ``land`` vs ``basic land``, or
# the tapped-vs-untapped tail) is deferred until a second example
# with a varying piece exists.
_SEARCH_LIBRARY_FOR_BASIC_LAND_CARD_TAPPED_ONTO_BATTLEFIELD = re.compile(
    r"^search your library for a basic land card, "
    r"put it onto the battlefield tapped, then shuffle$"
)
# "Search your library for an Equipment card, put it onto the
# battlefield, then shuffle." — Honored Knight-Captain's activated-
# ability body (sac-self → tutor for an Equipment). Maps to the
# existing ``SearchLibraryForEquipmentToBattlefield()`` engine class.
# Distinct from the ``_SEARCH_LIBRARY_FOR_BASIC_LAND_CARD_TAPPED_ONTO_
# BATTLEFIELD`` sibling above on the filter (Equipment vs basic land)
# AND the tap state (untapped vs tapped); the two shapes are mutually
# exclusive at the regex level so ordering doesn't matter. Literal-
# line recognizer with a single capture-less surface — Honored
# Knight-Captain is the only printing in the EoE/Duskmourn pool whose
# activated body prints this exact sentence today; broader grammar
# (parameterizing the filter-card-type — Aura, Vehicle, etc. — or the
# tapped-vs-untapped tail) is deferred until a second example with a
# varying piece exists.
_SEARCH_LIBRARY_FOR_EQUIPMENT_CARD_ONTO_BATTLEFIELD = re.compile(
    r"^search your library for an Equipment card, "
    r"put it onto the battlefield, then shuffle$"
)
# "Search your library for a <T1> or <T2> card, put it onto the
# battlefield, then shuffle." — the fetch-land body (Arid Mesa,
# Flooded Strand, ... : two basic land types) and Prismatic Vista's
# "a basic land card" variant; optional "tapped" tail. Captures the
# land-kind phrase so every type pairing (and any single type) claims
# through one recognizer, mapping to
# ``SearchLibraryForLandCardToBattlefield(land_types=..., basic_only=...,
# tapped=...)``. The "basic land card ... tapped" surface is matched
# FIRST by the older ``_SEARCH_LIBRARY_FOR_BASIC_LAND_CARD_TAPPED_ONTO_
# BATTLEFIELD`` recognizer (Moldering Gym / Lander parity), so this one
# only ever sees it untapped.
_BASIC_LAND_TYPE_ALT = r"(?:Plains|Island|Swamp|Mountain|Forest)"
_SEARCH_LIBRARY_FOR_LAND_KIND_ONTO_BATTLEFIELD = re.compile(
    r"^search your library for an? "
    r"(?P<kind>basic land|" + _BASIC_LAND_TYPE_ALT
    + r"(?: or " + _BASIC_LAND_TYPE_ALT + r")*) card, "
    r"put it onto the battlefield(?P<tapped> tapped)?, then shuffle$"
)
# "Gain control of target <subtype>." — permanent-theft activated ability
# whose legal-target pool is restricted to permanents with a named subtype.
# Sliver Overlord's second ability "{3}: Gain control of target Sliver.
# (This effect lasts indefinitely.)" is the only printing in the pool
# today; the regex captures the subtype word so a future card with the
# same shape but a different subtype (e.g. "Gain control of target Dragon.")
# claims through the same recognizer.  The captured subtype is dispatched
# through ``_GAIN_CONTROL_TARGET_SUBTYPE_EFFECT``; an unknown subtype
# returns ``None`` so the card stays unclaimed rather than silently
# dropping the restriction.
_GAIN_CONTROL_TARGET_SUBTYPE = re.compile(
    r"^gain control of target (\S+)$"
)
# Subtype word → engine effect mapping for the gain-control-by-subtype
# shape. Each entry is one printed subtype word (as it appears in the
# oracle text after stripping the trailing period and reminder text);
# the value is the engine class used when that subtype is targeted.
_GAIN_CONTROL_TARGET_SUBTYPE_EFFECT: dict[str, "Effect"] = {
    # Sliver Overlord: "Gain control of target Sliver."
    # GainControlOfTargetSliverPermanently targets only creatures with
    # the Sliver subtype and transfers ownership permanently.
    "Sliver": GainControlOfTargetSliverPermanently(),
}
# Token-creation effect grammar. Two related shapes co-exist in Scryfall
# oracle text:
#
# 1. **Stat-bearing creature tokens** — e.g.
#    ``create a tapped 2/2 colorless Robot artifact creature token`` or
#    ``create two 1/1 white Human Soldier creature tokens``. Carries
#    explicit power/toughness, a color word, one or more subtype words,
#    and a ``[artifact ]?creature token[s]?`` tail.
# 2. **Bare-name tokens** — e.g. ``create a Lander token``. No stats /
#    color / "creature" tail; the subtype word IS the token name and
#    fully identifies the registered printing (Lander, Munitions, etc.).
#
# Both paths look up the resolved token name in ``TOKEN_PRINTINGS``. The
# parser is NOT in the business of building token printings on the fly —
# if the registry doesn't already carry an entry for the matched name,
# parsing fails (returns ``None``) and the card stays unclaimed. Adding
# new token shapes is a separate concern handled in ``cards/tokens.py``.
#
# The leading count phrase is one of:
#   - ``a`` / ``an``    → 1
#   - a small spelled-out cardinal (``two``, ``three``, …)
#   - a bare digit
# The optional ``tapped`` keyword sits between the count and the stats
# (or directly before the bare-name in shape 2).
_TOKEN_COUNT_PATTERN = rf"a|an|\d+|{_SMALL_CARDINAL_PATTERN}"
# Color word — one of the five colors plus colorless. Not captured for
# correctness (the registry already encodes color via the printing's
# ``card_types``/``produces``); we just need to consume it so the regex
# matches the surface form.
_TOKEN_COLOR_PATTERN = r"white|blue|black|red|green|colorless"
# Stat-bearing creature-token shape. The captured groups are:
#   1. count word/digit
#   2. literal ``tapped`` if present (None otherwise)
#   3. power
#   4. toughness
#   5. color word (consumed only — see above)
#   6. subtype phrase (one or more space-separated capitalized words)
#   7. literal ``artifact`` if the token is also an artifact
#   8. literal ``enchantment`` if the token is also an enchantment
# The trailing ``creature token[s]?`` is required — bare-name tokens
# (Lander, Munitions) take a different surface form and parse via
# ``_CREATE_BARE_NAME_TOKEN`` below.
#
# Both ``artifact`` and ``enchantment`` qualifiers are consumed but not
# inspected — the registered ``TOKEN_PRINTINGS`` entry encodes the
# token's full type line, so the regex's job is just to recognize the
# surface and look up the subtype name. Glimmer (Duskmourn's
# enchantment-creature token created by Tunnel Surveyor and the other
# Glimmer-token cards) is the first ``enchantment creature`` shape in
# the EoE/Duskmourn pool; ``artifact`` came earlier with Robot/Drone.
# The Oxford comma between the two qualifiers (``2/2 white Cat
# artifact enchantment creature token``) is forward-looking grammar
# coverage — no card today carries both, but the regex shape allows
# either independently or both simultaneously without further work.
_CREATE_CREATURE_TOKEN = re.compile(
    rf"^create ({_TOKEN_COUNT_PATTERN}) (tapped )?(\d+)/(\d+) "
    rf"({_TOKEN_COLOR_PATTERN}) ([A-Z][A-Za-z]*(?: [A-Z][A-Za-z]*)*) "
    rf"(artifact )?(enchantment )?creature tokens?"
    rf"""(?: that's tapped and attacking)?(?:\s+with\s+.+|\.?\s+It has\s+.+)?$""",
    re.DOTALL,
)
# The optional trailing ``(?:\s+with\s+.+|\.?\s+It has\s+.+)?``
# accepts both surface forms that append the token's built-in
# abilities as a reminder:
#   - ``... with flying and 'This token can block only ...'``
#     (Station Monitor's Drone token — the ``with`` form).
#   - ``... Eldrazi Scion creature token. It has "Sacrifice this
#     token: Add {C}."`` (Warping Wail mode 3 — the ``. It has``
#     form, where the period + space + "It has" is a sentence-
#     boundary inside a modal mode line).
# Both forms are already encoded in ``TOKEN_PRINTINGS`` so the
# trailing description is redundant and can be safely ignored by
# the recognizer.
# Named artifact token shape — ``create a [color] artifact token named
# [Name] [with abilities]``.  Weapons Manufacturing prints "create a
# colorless artifact token named Munitions with 'When this token leaves
# the battlefield, it deals 1 damage to any target.'"  The color words
# and type words before ``token named`` are consumed but not validated;
# the registry name (group 3) is the authoritative lookup key.  The
# optional trailing ``with [abilities]`` is ignored — the abilities are
# already encoded in ``TOKEN_PRINTINGS``.
_CREATE_NAMED_ARTIFACT_TOKEN = re.compile(
    rf"^create ({_TOKEN_COUNT_PATTERN}) (tapped )?(?:\w+ )*artifact tokens? named "
    rf"([A-Z][A-Za-z]*)(?: with .+)?$",
    re.DOTALL,
)
# Bare-name token shape — ``create a Lander token`` /
# ``create two Lander tokens``. The captured subtype IS the registry
# key. Optional ``tapped`` sits between the count and the name (no EoE
# card prints a tapped Lander today, but the slot is reserved so a
# future ``create a tapped Lander token`` line parses without further
# grammar work).
_CREATE_BARE_NAME_TOKEN = re.compile(
    rf"^create ({_TOKEN_COUNT_PATTERN}) (tapped )?"
    rf"([A-Z][A-Za-z]*(?: [A-Z][A-Za-z]*)*) tokens?$"
)
# "Create a tapped 2/2 colorless Robot artifact creature token for each
# multicolored permanent you control." — Infinite Guideline Station's
# ETB. Maps to ``CreateTokenPerMulticoloredPermanent(token_printing=
# TOKEN_PRINTINGS["Robot"], tapped=True)`` — same primitive the static
# registry / bespoke factory uses. Literal-line recognizer with a
# zero-capture surface: only Infinite Guideline Station in the EoE
# pool prints this exact shape today, so a parameterized
# ``create a tapped <P>/<T> <color> <Subtype> <type> token for each
# multicolored permanent you control`` regex would have nothing else
# to claim. Deferred generalization: when a second card ships with
# the ``... for each multicolored permanent you control`` token-
# creation tail (or with a sibling per-X gate like ``for each artifact
# you control``), fold both literals into one regex with captured
# count/stats/color/subtype groups dispatching through the existing
# ``_parse_create_token_effect`` body to share the
# ``TOKEN_PRINTINGS`` lookup with the bare-token shapes above.
_CREATE_TAPPED_ROBOT_TOKEN_PER_MULTICOLORED_LINE = re.compile(
    r"^create a tapped 2/2 colorless Robot artifact creature token "
    r"for each multicolored permanent you control$"
)
# "Draw a card for each multicolored permanent you control." —
# Infinite Guideline Station's attack trigger. Maps to
# ``DrawCardPerMulticoloredPermanent()``. Literal-line recognizer
# with a zero-capture surface: only Infinite Guideline Station in the
# EoE pool prints this exact shape today, so a parameterized
# ``<verb> for each multicolored permanent you control`` regex would
# have nothing else to claim. Deferred generalization: when a second
# card ships with a sibling per-X tail on a different verb (e.g.
# ``each opponent loses 1 life for each multicolored permanent you
# control``), fold the per-multicolored-permanent gate into a shared
# tail recognizer dispatching to the matching primitive — same
# approach the existing ``deal damage by artifact count`` recognizer
# took.
_DRAW_CARD_PER_MULTICOLORED_PERMANENT_LINE = re.compile(
    r"^draw a card for each multicolored permanent you control$"
)
# "Surveil X, where X is the number of artifacts you control" —
# Cerebral Download's variable-surveil clause. Maps to
# ``Surveil(use_artifact_count=True)`` whose ``resolve`` reads
# ``len([o for o in state.battlefield if ... is_artifact])`` at fire
# time and surveys that many cards. The ``use_artifact_count`` flag
# substitutes the controller's live artifact count for the printed
# ``X``, so no numeric capture is needed — the surface form is a
# literal match with no varying piece. Currently only Cerebral
# Download in the EoE pool prints this exact shape today; the
# recognizer is a parameterless literal per the CLAUDE.md guidance
# (one card, no second example yet). A future card whose surveil
# count is driven by a different count-source (e.g. ``where X is
# the number of creatures you control``) would land its own sibling
# recognizer and engine-class flag rather than parameterizing this one.
_SURVEIL_X_ARTIFACT_COUNT = re.compile(
    r"^surveil X, where X is the number of artifacts you control$"
)
_SURVEIL_N = re.compile(r"^surveil (\d+)$")
# "Look at the top N cards of your library. Put one into your hand
# and the other|rest into your graveyard." — bare two-sentence
# look-pick-split surface form (NO trailing life-gain rider). Today's
# only EoE/DSK consumer is Codecracker Hound's ETB ("look at the top
# two cards ... and the other into your graveyard", n=2). The "rest"
# tail accepts future cards with n>=3 that print the bare
# look-pick-split shape without the Commune-with-Evil life-gain
# rider (n=2 must use "the other"; n>=3 must use "the rest" per
# English number-agreement; the alternation captures both surfaces
# cleanly).
#
# Maps to ``LookTopNPutOneInHandRestToGraveyard(n=N,
# reveal_chosen=False)`` — the refined-Cycle-1 fold (merge of the
# prior ``LookAtTopTwoSplitHandGraveyard`` engine class into the
# parameterized primitive at ``effects/generic/library_lookat.py``
# with a ``reveal_chosen: bool`` rider). The ``reveal_chosen=False``
# flag preserves Codecracker Hound's printed-text observable (no
# "Reveal that card" clause) and matches Rust's no-reveal resolve at
# ``state_ops_resolve.rs:6366-6394``.
#
# The two-sentence surface is what Scryfall prints — the embedded
# period sits inside the effect phrase rather than being the trailing
# terminator. Compound splits don't apply (the second sentence's
# leading ``Put one`` doesn't parse as a standalone effect — see
# ``_parse_compound_effect``'s all-or-nothing rule), so the regex
# matches the full two-sentence surface as a single primitive.
#
# This recognizer is parameterized per parser CLAUDE.md (one regex
# covers every future card with this bare look-pick-split shape, no
# code change needed).
_LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD = re.compile(
    rf"^look at the top (\d+|{_SMALL_CARDINAL_PATTERN}) cards of your library\. "
    rf"Put one(?: of them)? into your hand and (?:the other|the rest) into your graveyard$"
)
# "Look at the top N cards of your library. Put one of them into your
# hand and the rest into your graveyard. You gain M life." — Commune with
# Evil ``{2}{B}`` (Duskmourn). Three-sentence compound: the first two
# sentences form the look-pick-split mechanic
# (``LookTopNPutOneInHandRestToGraveyard(n=N)``); the third sentence is
# a trailing life-gain rider (``GainLife(amount=M)``). Together they
# produce ``Sequence(LookTopNPutOneInHandRestToGraveyard(N), GainLife(M))``.
#
# Tried BEFORE the bare two-sentence ``_LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD``
# form because the "to your graveyard. You gain" boundary suffix
# distinguishes the shapes; the bare two-sentence graveyard form's ``$``
# anchor would not match the trailing life-gain sentence anyway, but
# more-specific-first is the convention here.
#
# Parameterized over both N (the top-of-library look count) and M (the
# life-gain amount) via capture groups. Only Commune with Evil in the
# EoE/DSK pool prints this combined shape today; the regex captures both
# parameters as integers so a future printing with different N or M
# values (e.g. "Look at the top three cards … You gain 2 life.") lands
# on the same primitive without further grammar work — the parameterized-
# recognizer-preferred form per parser CLAUDE.md.
_LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD_GAIN_M_LIFE = re.compile(
    rf"^look at the top (\d+|{_SMALL_CARDINAL_PATTERN}) cards of your library\. "
    rf"Put one of them into your hand and the rest into your graveyard\. "
    rf"You gain (\d+) life$"
)
# ``look at the top N cards of your library. You may reveal a <filter>
# card from among them and put it into your hand. Put the rest on the
# bottom of your library in a random order[. If you didn't put a card
# into your hand this way, put a +1/+1 counter on this creature]`` —
# parameterized peek-tutor-or-counter shape. Two cards in the EoE/DSK
# pool currently print this body:
#
#   - Living Phone (DSK) — dies trigger; filter "creature card with
#     power N or less"; no counter-rider tail (no ``If you didn't put
#     a card into your hand this way`` sentence).
#   - Pulsar Squadron Ace (EoE) — ETB; filter "Spacecraft card"; the
#     counter-rider tail IS present.
#
# Two filter shapes are captured: a bare type ("Spacecraft") OR
# "creature card with power N or less". Both land on the existing
# ``LookAtTopNRevealOrCounter`` engine class — the captured filter
# token routes to ``filter_name`` ("spacecraft" / "creature"), the
# optional ``max_power`` capture lands on the engine class's
# ``max_power=N`` kwarg (only meaningful for the creature filter; the
# class ignores it for non-creature filters), and the optional
# counter-rider tail flips ``counter_on_miss=True``. Replaces the two
# distinct literals (the prior Pulsar-specific bespoke and the prior
# ``_LOOK_TOP_FIVE_REVEAL_LOW_POWER_CREATURE`` Living Phone literal)
# with a single parameterized recognizer per the parser CLAUDE.md
# guidance.
_LOOK_TOP_N_REVEAL_OR_COUNTER = re.compile(
    rf"^look at the top (\d+|{_SMALL_CARDINAL_PATTERN}) cards of your "
    rf"library\. "
    rf"You may reveal a (Spacecraft|creature) card"
    rf"(?: with power (\d+) or less)? "
    rf"from among them and put it into your hand\. "
    rf"Put the rest on the bottom of your library in a random order"
    rf"(?P<rider>\. If you didn't put a card into your hand this way, "
    rf"put a \+1/\+1 counter on this creature)?$"
)
# Mill is "mill <amount> cards" where the amount is either a digit or
# one of the spelled-out small cardinals (Scryfall printing convention).
# Also handles "mill a card" (the ``a`` singular form prints on Icetill
# Explorer's landfall trigger), aliased to amount=1.
_MILL_N_CARDS = re.compile(
    rf"^mill (\d+|{_SMALL_CARDINAL_PATTERN}) cards$"
)
_MILL_A_CARD = re.compile(r"^mill a card$")
_DESTROY_TARGET_ARTIFACT = re.compile(r"^destroy target artifact$")
# "Destroy target artifact, enchantment, or creature with flying." —
# Shattered Wings's destroy clause. Maps to
# ``DestroyTargetArtifactEnchantmentOrFlyingCreature()`` whose
# ``legal_targets`` filters to artifacts (any), enchantments (any),
# or creatures that currently have flying. The Oxford comma after
# ``enchantment`` is required (matches Scryfall's printed form).
_DESTROY_TARGET_ARTIFACT_ENCHANTMENT_OR_FLYING_CREATURE = re.compile(
    r"^destroy target artifact, enchantment, or creature with flying$"
)
# "destroy target land" (Strip Mine) / "destroy target nonbasic land"
# (Wasteland, Dust Bowl) — the optional ``nonbasic`` qualifier maps to
# ``DestroyTargetLand(nonbasic_only=True)``.
# "Sacrifice this <permanent-type>." as an effect body (not a cost) —
# City of Traitors' "When you play another land, sacrifice this land."
# Maps to ``SacrificeSelf()`` (reads ``state.resolving_source``; no-op
# if the source already left the battlefield).
# "this land becomes a copy of target permanent card in your graveyard
# until end of turn" — Shifting Woodland's delirium ability. Only card in
# the pool with a self-copy-of-graveyard-card shape; broader grammar
# (other self nouns / durations) deferred until a second example exists.
# Maps to ``SourceBecomesCopyOfTargetGraveyardCardUEOT()``.
_SELF_BECOMES_COPY_OF_GRAVEYARD_PERMANENT_UEOT = re.compile(
    r"^this land becomes a copy of target permanent card in your "
    r"graveyard until end of turn$"
)
_SACRIFICE_THIS_PERMANENT = re.compile(
    r"^sacrifice this (?:land|creature|artifact|enchantment|permanent)$"
)
_DESTROY_TARGET_LAND = re.compile(r"^destroy target (?P<nonbasic>nonbasic )?land$")
_DESTROY_TARGET_NONCREATURE_PERMANENT = re.compile(
    r"^destroy target noncreature permanent$"
)
# "Destroy target nonland permanent. Its controller creates a Lander
# token." — Emergency Eject's 2-sentence body. The Lander is created
# by the CONTROLLER OF THE TARGET permanent (not the spell's caster),
# which is the distinguishing feature of the bespoke engine class
# ``DestroyTargetNonlandPermanentWithLanderForController``. Two-sentence
# recognizer: both sentences must be kept together because splitting
# would lose the "its controller" back-reference.
#
# Currently only Emergency Eject in the EoE/DSK pool has this exact
# surface; the recognizer is a literal-line match (no captured groups)
# since no second card with a varying piece (e.g. different permanent
# type, different token) exists today. Per parser CLAUDE.md, a literal
# is acceptable when exactly one card fits and the docstring notes why
# generalization is deferred. A future "Destroy target creature. Its
# controller creates a Lander token." would share the "its controller
# gets the Lander" structure; at that point replace this with a
# parameterized regex capturing the permanent type.
_DESTROY_TARGET_NONLAND_PERMANENT_ITS_CONTROLLER_CREATES_LANDER = re.compile(
    r"^destroy target nonland permanent\. "
    r"Its controller creates a Lander token$"
)
# "Destroy up to one target nonland permanent." — Syr Vondam, Sunstar
# Exemplar's self-leaves trigger: optional-target destroy (0 or 1
# nonland permanents). Maps to ``DestroyUpToOneTargetNonlandPermanent``
# whose TargetSpec carries ``count_min=0`` so the controller may legally
# pass with no target. Distinct from the bare "destroy target" form below
# (which requires exactly 1 target); tried BEFORE the bare form so the
# more-specific "up to one" phrase wins.
_DESTROY_UP_TO_ONE_TARGET_NONLAND_PERMANENT = re.compile(
    r"^destroy up to one target nonland permanent$"
)
# "Destroy target nonland permanent." — Thaumaton Torpedo's activated
# ability. Maps to the existing ``DestroyTargetNonlandPermanent`` engine
# class whose ``legal_targets`` filters the battlefield to non-land
# permanents (creatures, artifacts, enchantments, planeswalkers).
# Distinct from ``destroy target noncreature permanent`` above — that
# one excludes creatures, this one excludes lands.
_DESTROY_TARGET_NONLAND_PERMANENT = re.compile(
    r"^destroy target nonland permanent$"
)
# "Destroy target artifact or enchantment. If its mana value was 2 or
# less, create a Lander token." — Seedship Impact `{1}{G}` Instant: a
# bare destroy on an artifact-or-enchantment target with a small-mana-
# value Lander rider. Two-sentence shape: the leading sentence is the
# bare destroy; the trailing ``If its mana value was 2 or less, create
# a Lander token.`` clause is an anaphoric rider that doesn't parse as
# a standalone effect (the ``its`` back-references the destroyed
# permanent's mana value, with no per-clause primitive recognizing the
# back-reference). The recognizer matches the unsplit two-sentence
# body verbatim — ``_parse_spell_oracle_text`` tries the unsplit
# clause first via ``_parse_spell_effect_phrase`` before falling
# through to the per-sentence splitter, so this regex claims Seedship
# Impact before the splitter can carve the rider into an unparseable
# piece.
#
# Currently only Seedship Impact in the EoE pool fits this shape, so
# the recognizer is a parameterless literal — broader generalization
# (e.g. parameterizing the mana-value threshold or the destroy-target
# type list) is deferred until a second card with a similar two-
# sentence destroy-with-Lander-on-small-mv-rider shape exists. Sibling
# to the destroy-target neighbors above; lives in the per-clause
# grammar (rather than the whole-spell exact-text tier) per the
# parser's generic-templates-over-literals guidance — a neighboring
# recognizer is the natural landing site for the next card with this
# shape rather than expanding the whole-spell tier.
_SEEDSHIP_IMPACT = re.compile(
    r"^destroy target artifact or enchantment\. "
    r"If its mana value was 2 or less, create a Lander token$"
)
# "Add {<color>}." — single coin of a single color. Mana ability surface
# form. The captured letter is one of WUBRG (no colorless single-coin
# add in EoE today; extend to ``[WUBRGC]`` if a future card needs it).
# The surrounding ActivatedAbility built by the activated-grammar layer
# auto-flags ``is_mana_ability=True`` once the resulting effect is
# recognized by ``_is_mana_ability_effect``, so the engine bypasses the
# stack on activation per CR 605.
_ADD_SINGLE_COLOR_MANA = re.compile(r"^add \{([WUBRG])\}$")
# "Add one mana of any color" — single coin of any color, the
# at-resolution choose-a-color mana ability surface form (All-Fates
# Scroll's ``{T}: Add one mana of any color.``). Maps to
# ``AddManaToPool(produces=(("any", 1),))`` — the engine treats the
# ``"any"`` color tag as a chosen-at-mint colored coin (the controller
# names the color when the ability resolves). Distinct from the
# bracketed ``add {<color>}`` shape: the latter mints a fixed-color
# coin known at parse time, this one defers the choice to resolution.
# Mana-source tracking treats ``"any"`` coins as untyped at compile
# time, mirroring ``land_builder``'s ``_fancy_pay_one_life_any_color``
# wiring on Mana Confluence.
_ADD_ONE_MANA_OF_ANY_COLOR = re.compile(r"^add one mana of any color$")
# "Add {<color>}{<color>}." — two coins of a single color (Steelswarm
# Operator's ``{T}: Add {U}{U}`` ability mints two restricted coins of
# the same color). The captured letter is reused — the engine class
# accepts a tuple of (color, amount) coins, so the produced effect is
# ``AddManaToPool(produces=((c, 1), (c, 1)))``. Two coins is what EoE
# prints today; if a future card uses ``Add {U}{U}{U}`` extend the
# regex with another optional brace pair (or generalize to a parser
# loop).
_ADD_TWO_SAME_COLOR_MANA = re.compile(
    r"^add \{([WUBRG])\}\{([WUBRG])\}$"
)
# "Add {C}{C}…" — N colorless-mana coins (The Eternity Elevator's
# ``{T}: Add {C}{C}{C}.`` mints three colorless coins). Captures the
# entire brace sequence; the coin count is derived by counting ``{C}``
# occurrences. The engine's ``AddManaToPool(produces=(("C", N),))``
# represents this as a single (color, count) entry. Tried BEFORE the
# bare single-color regex so the more-specific colorless shape wins;
# the ``$`` anchor keeps a lone ``{C}`` from matching the wrong arm
# (it would fail the colored ``[WUBRG]`` filter anyway, but explicit
# ordering avoids reliance on that side-effect).
_ADD_N_COLORLESS_MANA = re.compile(r"^add ((?:\{C\})+)$")
# "Add X mana of any one color, where X is the number of charge counters
# on this creature" — The Eternity Elevator's tier-gated activated
# ability (``20+ | {T}: Add X mana of any one color, where X is the
# number of charge counters on The Eternity Elevator.``). After
# ``_substitute_self_name`` rewrites the card name to ``this creature``,
# the normalized effect phrase matches this literal regex. Maps to
# ``AddChargeCounterManaOfChosenColor`` whose ``resolve`` reads
# ``state.resolving_source.charge_counters`` at fire time and mints that
# many coins of the controller's chosen color. Only The Eternity Elevator
# in the EoE pool prints this shape; broader grammar deferred until a
# second card with a charge-counter-indexed mana amount appears.
_ADD_X_MANA_ANY_COLOR_PER_CHARGE_COUNTER = re.compile(
    r"^add X mana of any one color, where X is the number of "
    r"charge counters on this creature$"
)
# "Add {<c1>} or {<c2>}." — choose-one-of-two-colors single-coin mana
# ability (Haunted Screen's first ability ``{T}: Add {W} or {B}.``).
# Maps to ``AddNManaOfChosenColor(amount=1, options=(<c1>, <c2>))`` —
# the controller picks one of the two listed colors at resolution and
# the engine class mints a single coin of the chosen color. Distinct
# from the single-color ``add {<color>}`` shape (no choice — fixed
# color) and from the unrestricted ``add one mana of any color`` shape
# (any of W/U/B/R/G). The captured colors order is preserved and lands
# directly on the engine class's ``options`` tuple, faithful to the
# printed list.
_ADD_TWO_COLOR_CHOICE_MANA = re.compile(
    r"^add \{([WUBRG])\} or \{([WUBRG])\}$"
)
# "Add {<c1>}, {<c2>}, or {<c3>}." — choose-one-of-three-colors
# single-coin mana ability (Haunted Screen's second ability ``{T}, Pay
# 1 life: Add {G}, {U}, or {R}.``). Three-color sibling of the
# two-color shape above. Maps to ``AddNManaOfChosenColor(amount=1,
# options=(<c1>, <c2>, <c3>))``. The Oxford-comma "or" before the
# final color matches Scryfall's standard wording for three-element
# disjunctions.
_ADD_THREE_COLOR_CHOICE_MANA = re.compile(
    r"^add \{([WUBRG])\}, \{([WUBRG])\}, or \{([WUBRG])\}$"
)
# "Add {<mana>}. Spend this mana only to <restriction>." —
# restricted-mana ability whose produced coins are tagged with an
# engine-side ``spend_restriction`` string at mint time. EoE wiring:
#
#   * ``Spend this mana only to cast an artifact spell`` →
#     ``"artifact_spell"`` (Steelswarm Operator's first ability).
#   * ``Spend this mana only to activate abilities of artifact
#     sources`` → ``"artifact_activation"`` (Steelswarm Operator's
#     second ability).
#
# Both restrictions are gated at cost-payment time by
# ``_coin_satisfies_context`` in ``actions.py``. The captured mana
# segment accepts either a single ``{<color>}`` or two of the same
# color (``{<color>}{<color>}``) — extend the inner pattern if a future
# printing mints a hybrid- or multi-color restricted batch. Tried
# BEFORE the bare ``add {<color>}`` regex so the qualifier wins; the
# bare regex's ``$`` anchor wouldn't match the multi-sentence shape
# anyway, but ordering more-specific before more-general is the
# convention here.
# "Add {U}{U}, {U}{B}, or {B}{B}." — choose ONE of several fixed mana
# combinations (filter lands: Sunken Ruins). Captured as a comma/or
# list of brace runs; only claimed when at least one option mints two
# or more mana (single-symbol "add {W} or {U}" lists keep their
# ``AddNManaOfChosenColor`` recognizers). Maps to
# ``AddOneOfManaCombinations``.
_MANA_RUN = r"(?:\{[WUBRGC]\})+"
_ADD_ONE_OF_MANA_COMBINATIONS = re.compile(
    r"^add (?P<list>" + _MANA_RUN + r"(?:, " + _MANA_RUN + r")*,? or "
    + _MANA_RUN + r")$"
)
# "Add {<C>} for each <noun> you control." — scaling mana ability
# (Gaea's Cradle: creature; Tolarian Academy: artifact). Maps to
# ``AddManaOfColorPerCount(color, CountControlledMatching(<pred>))``;
# the noun is looked up in ``_ADD_MANA_PER_CONTROLLED_NOUNS`` (unknown
# noun -> ``None``).
_ADD_MANA_FOR_EACH_CONTROLLED = re.compile(
    r"^add \{(?P<color>[WUBRGC])\} for each (?P<noun>.+) you control$"
)
_ADD_MANA_PER_CONTROLLED_NOUNS: dict[str, object] = {
    "creature": _IsCreature(),
    "artifact": IsArtifact(),
}
# "Add {R}{R}. If any of that mana is spent on a creature spell, it gains
# haste until end of turn." — mana tagged with a haste rider (Arena of
# Glory). Parameterized over the brace run; maps to ``AddManaToPool(...,
# grants_haste_to_creature_spell=True)``.
_ADD_MANA_WITH_CREATURE_HASTE_RIDER = re.compile(
    r"^add ((?:\{[WUBRGC]\})+)\. If any of that mana is spent on a "
    r"creature spell, it gains haste until end of turn$"
)
_ADD_MANA_WITH_SPEND_RESTRICTION = re.compile(
    r"^add ((?:\{[WUBRGC]\})+)\. "
    r"Spend this mana only to (.+)$"
)
# "Add one mana of any color. Spend this mana only to <restriction>."
# — restricted any-color mana ability (Plaza of Heroes' second ability:
# "{T}: Add one mana of any color. Spend this mana only to cast a
# legendary spell."). Produces an ``("any", 1)`` coin tagged with
# the spend restriction, identical to how the colored form
# ``_ADD_MANA_WITH_SPEND_RESTRICTION`` works, just with the mana
# segment being ``one mana of any color`` rather than a brace-colored
# expression. The captured tail phrase is looked up in
# ``_SPEND_RESTRICTION_BY_PHRASE``.
_ADD_ANY_COLOR_MANA_WITH_SPEND_RESTRICTION = re.compile(
    r"^add one mana of any color\. Spend this mana only to (.+)$"
)
# "Add one mana of any color among <group> you control." — one mana of a
# color among the permanents of the named group its controller controls
# (none of a color: not that color; none at all: no mana), spent on
# anything. Plaza of Heroes' third ability reads "{T}: Add one mana of
# any color among legendary permanents you control." Modeled as
# ``AddOneManaOfColorAmongControlled(card_types=(), legendary=True)``
# (Mox Amber's class; mtg-zero docs/ir_defects.md 160: it used to be
# read as a spending restriction). The captured group phrase is looked
# up in ``_ADD_MANA_AMONG_GROUPS``.
_ADD_ANY_MANA_AMONG_GROUP = re.compile(
    r"^add one mana of any color among (.+) you control$"
)
# Surface phrase → ``(card_types, legendary)`` of
# ``AddOneManaOfColorAmongControlled`` for the "add one mana of any color
# among <group> you control" shape (Mox Amber's typed groups are claimed
# by ``artifact_effects._ADD_ONE_MANA_AMONG_TYPES``).
_ADD_MANA_AMONG_GROUPS: dict[str, tuple[tuple[str, ...], bool]] = {
    # Plaza of Heroes: "add one mana of any color among legendary
    # permanents you control" (any permanent type, legendary).
    "legendary permanents": ((), True),
}
# Surface phrase → ``ActivationCost.spend_restriction`` string mapping
# for the restricted-mana ability above. Each tail phrase below comes
# from a real EoE printing's oracle text; the engine string is the key
# expected by ``_coin_satisfies_context`` (``actions.py``).
_SPEND_RESTRICTION_BY_PHRASE: dict[str, str] = {
    "cast an artifact spell": "artifact_spell",
    # Mishra's Workshop: "{T}: Add {C}{C}{C}. Spend this mana only to
    # cast artifact spells." — plural surface of the same restriction.
    "cast artifact spells": "artifact_spell",
    "activate abilities of artifact sources": "artifact_activation",
    # Plaza of Heroes line 2: "Spend this mana only to cast a legendary
    # spell." Maps to the same engine tag honoured by
    # ``_coin_satisfies_context`` in actions.py.
    "cast a legendary spell": "legendary_spell",
}
# "Target opponent loses N life" — pure life-loss spell/ability surface.
# TargetOpponentLosesLife filters its legal-targets pool to opponents.
_TARGET_OPPONENT_LOSES_N_LIFE = re.compile(
    r"^target opponent loses (\d+) life$"
)
# "They lose N life" — effect body on an opponent-discards observer
# trigger (Entropic Battlecruiser's ``1+ | Whenever an opponent discards
# a card, they lose 3 life.``). The pronoun ``they`` refers to the
# discarding player, who is threaded through as
# ``fixed_targets=(("player", id),)`` by
# ``fire_opponent_discards_triggers``. Maps to
# ``TargetPlayerLosesLife(amount=N)`` which reads that pre-wired target
# from ``targets[0]`` at resolve time — same engine class the bespoke
# table used. The ``N life`` tail accepts bare digits; no EoE printing
# uses a non-integer life-loss amount in this shape today.
_THEY_LOSE_N_LIFE = re.compile(
    r"^they lose (\d+) life$"
)
# "That player gets N poison counters" — effect body on a combat-damage
# observer trigger (Virulent Silencer's ``Whenever a nontoken artifact
# creature you control deals combat damage to a player, that player gets
# two poison counters.``). The pronoun ``that player`` refers to the
# damaged player, who is pre-wired as
# ``fixed_targets=(("player", id),)`` by the
# ``DEALS_COMBAT_DAMAGE`` dispatch in
# ``combat._fire_combat_damage_to_player_triggers``. Maps to
# ``PutPoisonCountersOnTargetPlayer(amount=N)`` which reads the pre-wired
# target from ``targets[0]`` at resolve time. The ``N`` tail accepts
# spelled-out small cardinals (``two``) and bare digits.
_THAT_PLAYER_GETS_N_POISON_COUNTERS = re.compile(
    rf"^that player gets (\d+|{_SMALL_CARDINAL_PATTERN}) poison counters?$"
)
# "Lock or unlock a door of target Room you control" — Keys to the
# House's sorcery-speed activated body (``{3}, {T}, Sacrifice this
# artifact: Lock or unlock a door of target Room you control. Activate
# only as a sorcery.``). Maps directly to the generic engine class
# ``LockOrUnlockDoorOfTargetRoomYouControl`` (already in
# ``effects.generic`` for re-use). The "Room" subject is preserved with
# its capital R because the activated-ability normalizer only
# lowercases the first letter of the phrase, leaving ``Room`` intact.
# Single-card surface in the EoE/DSK pool today — broader Room-action
# grammar (``unlock a door of target Room`` without the ``lock or``
# disjunction, etc.) deferred until a second printing with this shape
# ships.
_LOCK_OR_UNLOCK_DOOR_TARGET_ROOM_YOU_CONTROL = re.compile(
    r"^lock or unlock a door of target Room you control$"
)
# "Destroy target <noun> an opponent controls. That player may search
# their library for a land card with a basic land type, put it onto the
# battlefield, then shuffle." — Boseiju, Who Endures' channel body. The
# only printing in the pool with the destroy + "that player" land-search
# compensation shape (broader grammar deferred until a second example
# exists); the destroyable noun is captured and looked up in
# ``_DESTROY_OPPONENT_NOUNS``. Maps to
# ``ThenTargetControllerResolves(Destroy(<noun> & ControlledByOpponent),
# SearchLibraryForLandCardToBattlefield(<five basic types>))`` — the
# search's own ``choose_card_from_library`` decline models "may".
_DESTROY_THEN_CONTROLLER_SEARCHES_BASIC_TYPED_LAND = re.compile(
    r"^destroy target (?P<noun>.+) an opponent controls\. That player "
    r"may search their library for a land card with a basic land type, "
    r"put it onto the battlefield, then shuffle$"
)
# "Return target <noun> to its owner's hand." — mandatory single-target
# bounce whose legal-target pool is named by the printed noun phrase.
# The captured noun is looked up in ``_BOUNCE_TARGET_NOUNS`` and maps to
# the unified ``Bounce`` primitive with one ``TargetSpec``; an unknown
# noun returns ``None`` (card stays unclaimed rather than bouncing the
# wrong pool). Karakas: "legendary creature"; Otawara, Soaring City's
# channel body: "artifact, creature, enchantment, or planeswalker".
_RETURN_TARGET_NOUN_TO_OWNERS_HAND = re.compile(
    r"^return target (?P<noun>.+) to its owner's hand$"
)
_BOUNCE_TARGET_NOUNS: dict[str, object] = {
    "legendary creature": _IsCreature() & _IsLegendary(),
    "artifact, creature, enchantment, or planeswalker": (
        IsArtifact() | _IsCreature() | _IsEnchantment() | _IsPlaneswalker()
    ),
}
# "Return up to one other target permanent you control to its owner's
# hand" — Exosuit Savior's ETB. Maps to
# ``ReturnTargetPermanentYouControlToOwnersHand(optional=True,
# exclude_self=True)``. The ``up to one other`` fragment combines the
# ``up to one`` optionality (resolves as a no-op if no target chosen)
# with the ``other`` self-exclusion (filters the trigger source out
# of the legal pool). Lands are legal targets — the engine class's
# ``legal_targets`` doesn't filter on type beyond the controller and
# self gate.
_RETURN_UP_TO_ONE_OTHER_PERMANENT_YOU_CONTROL_TO_OWNERS_HAND = re.compile(
    r"^return up to one other target permanent you control "
    r"to its owner's hand$"
)
# "Return up to one target creature to its owner's hand." — Bottomless
# Pool's door trigger. Optional-target bounce against any creature
# (friend or foe). Maps to
# ``ReturnTargetCreatureToOwnersHand(optional=True)`` — same engine
# class the spell-builder dispatch uses for the mandatory shape, here
# with the ``optional=True`` flag flipped so a no-target resolution
# is a clean no-op fizzle (engine-side
# ``target_count_range = (0, 1)``). Distinct from the ``up to one
# OTHER target permanent you control`` Exosuit Savior shape above on
# both target type (creature only vs any permanent) and controller
# qualifier (any controller vs you-control); the two are mutually
# exclusive at the regex level. Literal-line recognizer with no
# capture groups — Bottomless Pool is the only printing in the
# EoE/DSK pool whose triggered- or activated-ability body prints this
# exact sentence; broader grammar (parameterizing ``up to one`` vs
# bare ``target``, or fold into a single creature-bounce family with
# the existing ``ReturnTargetCreatureToOwnersHand`` paths in
# ``spell_builder``) is deferred until a second example with a
# varying piece exists.
#
# Parameterized over the ``up to one`` optionality: the bare
# ``return target creature to its owner's hand`` (Aether Spellbomb's
# activation) claims through the same recognizer with ``optional``
# left False.
_RETURN_UP_TO_ONE_TARGET_CREATURE_TO_OWNERS_HAND = re.compile(
    r"^return (up to one )?target creature to its owner's hand$"
)
# "Return up to one target nonland permanent to its owner's hand." —
# Unnerving Grasp ``{2}{U}`` Sorcery (Duskmourn). Optional-target bounce
# against any nonland permanent (friend or foe, any controller). Maps to
# ``ReturnTargetCreatureToOwnersHand(allow_nonland_permanent=True,
# optional=True)`` — the ``allow_nonland_permanent=True`` widens the
# legal-target pool from creature-only to any nonland permanent (the same
# flag Desculpting Blast uses for the mandatory variant); ``optional=True``
# flips ``count_min=0`` so a no-target resolution is a clean no-op.
#
# The ``Sequence.target_count_range()`` fix (bespoke-spells3) ensures the
# cast pipeline OFFERS a target for this effect when it appears in a
# Sequence (even though ``requires_target()`` returns False due to
# ``count_min=0``), so ``Sequence(ReturnTargetCreatureToOwnersHand(
# allow_nonland_permanent=True, optional=True), ManifestDread())``
# correctly prompts for an optional bounce target before manifesting.
#
# Literal-line recognizer with no capture groups — Unnerving Grasp is the
# only EoE/DSK printing today with this exact "up to one nonland permanent"
# surface; generalization (parameterizing the ``up to one`` count or the
# ``nonland`` qualifier) is deferred until a second example with a varying
# piece exists.
_RETURN_UP_TO_ONE_TARGET_NONLAND_PERMANENT_TO_OWNERS_HAND = re.compile(
    r"^return up to one target nonland permanent to its owner's hand$"
)
# "Return up to N target non-Spacecraft creatures to their owners'
# hands" — Specimen Freighter's ETB body. Maps to
# ``ReturnUpToNTargetCreaturesToOwnersHands(max_n=N,
# exclude_spacecraft=True)`` whose ``legal_targets`` filters to
# non-Spacecraft creatures and ``target_count_range`` returns
# ``(0, max_n)``. The amount tail accepts the spelled-out small
# cardinal Scryfall prints (``two`` on Specimen Freighter) plus
# bare digits for any future printing carrying a different bound.
# The ``non-Spacecraft`` qualifier is verbatim from the oracle text;
# only Specimen Freighter in the current pool prints this filter, but
# the bound is parameterized so a future ``return up to three target
# non-Spacecraft creatures …`` printing lands on the same regex.
_RETURN_UP_TO_N_NON_SPACECRAFT_CREATURES_TO_OWNERS_HANDS = re.compile(
    rf"^return up to (\d+|{_SMALL_CARDINAL_PATTERN}) target "
    rf"non-Spacecraft creatures to their owners' hands$"
)
# "Return up to N target nonland permanent cards from your graveyard to
# your hand, then return up to M target land cards from your graveyard to
# the battlefield tapped." — Pull Through the Weft `{2}{G}{U}` Sorcery.
#
# A compound sentence joined by ``, then ``; ``_parse_compound_effect``
# would split it into two pieces but fails on the second piece (no
# "return up to N land cards from graveyard to battlefield tapped"
# primitive exists in the engine today). This recognizer matches the
# FULL unsplit sentence and drops the second clause as an unimplemented
# rider — a parser-is-right simplification identical in kind to the
# simplifications the static registry already ships (the old whole-spell
# entry returned bare ``ReturnCreatureFromYourGraveyardToHand()``
# dropping BOTH the "up to two" count AND the "nonland permanent"
# widening; the new per-clause grammar is MORE faithful because it
# preserves the multi-target count and the nonland-permanent scope).
#
# Parameterized: two capture groups capture the "up to N" counts for
# the first and second clauses respectively (the second is consumed but
# the second clause's effect is not modeled). The first count drives
# ``ReturnUpToNNonlandPermanentCardsFromGraveyardToHand(max_n=N)``
# (``min_n=0`` reflects the ``up to`` optionality). Maps to
# ``ReturnUpToNNonlandPermanentCardsFromGraveyardToHand`` from
# ``effects.generic``.
#
# Currently only Pull Through the Weft in the EoE pool fits this
# combined shape; the regex captures both counts as parameters so a
# future printing with different bounds (e.g. ``up to three … then
# return up to one …``) lands on the same primitive without further
# grammar work — the parameterized-recognizer-preferred form per parser
# CLAUDE.md. The second count is unused today but captured to ensure
# the regex matches the full oracle-text surface.
_RETURN_UP_TO_N_NONLAND_PERMANENT_GY_TO_HAND_THEN_LANDS_BF_TAPPED = re.compile(
    rf"^return up to ({_SMALL_CARDINAL_PATTERN}|\d+) target nonland permanent "
    rf"cards from your graveyard to your hand, then return up to "
    rf"({_SMALL_CARDINAL_PATTERN}|\d+) target land cards from your graveyard "
    rf"to the battlefield tapped$"
)
# "Return target artifact or creature card from your graveyard to your
# hand" — Interceptor Mechan's ETB body. Maps to
# ``ReturnCreatureFromYourGraveyardToHand(or_artifact=True)``. Tried
# BEFORE the bare ``return target creature card`` regex so the more-
# specific ``artifact or creature`` widening lands first.
_RETURN_TARGET_ARTIFACT_OR_CREATURE_GRAVEYARD_TO_HAND = re.compile(
    r"^return target artifact or creature card from your graveyard to your hand$"
)
# "Return target creature card from your graveyard to your hand" —
# graveyard-to-hand reanimation surface.
_RETURN_TARGET_CREATURE_GRAVEYARD_TO_HAND = re.compile(
    r"^return target creature card from your graveyard to your hand$"
)
# "Return a creature or Spacecraft card from your graveyard to your
# hand" — Fell Gravship's ETB body second sentence (after the
# ``mill three cards, then`` connective is split off by the compound
# parser). Maps to ``ReturnCreatureFromYourGraveyardToHand(
# or_spacecraft=True)``. The article is ``a`` rather than ``target``
# because Scryfall prints the choice as a non-target selection in
# this position; the engine class's ``requires_target`` /
# ``legal_targets`` still drive the choice through the agent at
# resolve time. Tried BEFORE the bare ``return target creature card``
# regex below so the more-specific ``or Spacecraft`` widening lands
# first; the bare regex's ``$`` anchor on ``hand`` would never match
# the ``or Spacecraft`` tail anyway, but ordering more-specific before
# more-general is the convention in this dispatcher. Mirrors the
# ``or_spacecraft`` widening on ``DestroyTargetCreature`` (Embrace
# Oblivion) and the ``or_enchantment`` widening on the same engine
# class (Withering Torment) — same precedent of adding a flag to an
# existing graveyard / battlefield operator rather than a new engine
# class. Literal-line recognizer with a single-card claim: only Fell
# Gravship in the EoE pool prints this exact ``a creature or
# Spacecraft card`` graveyard surface today; deferred generalization
# would fold this and a future ``return a creature or Spacecraft
# card from your graveyard to the battlefield`` sibling into one
# regex with a captured destination word, dispatching across both
# engine-class destinations.
_RETURN_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_HAND = re.compile(
    r"^return a creature or Spacecraft card "
    r"from your graveyard to your hand$"
)
# "Return target creature card with mana value N or less from your
# graveyard to your hand" — narrowed-by-MV graveyard-to-hand reanimation.
# Savior of the Small's Survival reward (``mana value 3 or less``); the
# threshold is the captured digit. Maps to
# ``ReturnCreatureFromYourGraveyardToHand(max_mv=N)``. Tried BEFORE the
# bare ``return target creature card from your graveyard to your hand``
# regex above so a future printing carrying the MV rider lands here
# rather than on the unbounded sibling. The bare regex still covers
# Interceptor Mechan and any other MV-unrestricted reanimator.
_RETURN_TARGET_CREATURE_GRAVEYARD_TO_HAND_WITH_MV = re.compile(
    r"^return target creature card with mana value (\d+) or less "
    r"from your graveyard to your hand$"
)
# "Return one or two target creature and/or planeswalker cards from
# your graveyard to your hand." — Archenemy's Charm mode 2. The
# "and/or planeswalker" scope drop mirrors the existing "or
# planeswalker" scope drops across the pool (same pending-engine
# support precedent as mode 1's planeswalker exile). Maps to
# ``ReturnUpToNCreaturesFromYourGraveyardToHand(max_n=2, min_n=1)``
# whose ``legal_targets`` is creature-only today; planeswalkers would
# surface through the same code path if they appear in graveyards.
# Literal-line recognizer: only Archenemy's Charm fits this exact
# surface today; the "one or two" count and "and/or planeswalker"
# disjunct are both fixed at their printed values. A future "return
# one or three target creature cards" would carry a different count;
# generalize if/when a second card with a varying piece appears.
_RETURN_ONE_OR_TWO_CREATURE_OR_PLANESWALKER_FROM_GRAVEYARD_TO_HAND = re.compile(
    r"^return one or two target creature and/or planeswalker cards "
    r"from your graveyard to your hand$"
)
# "Return target creature card from your graveyard to the battlefield"
# — graveyard reanimation surface (Xu-Ifit and forward-compat for any
# future "raise dead" oracle line).
_RETURN_TARGET_CREATURE_GRAVEYARD_TO_BATTLEFIELD = re.compile(
    r"^return target creature card from your graveyard to the battlefield$"
)
# "Return target creature or enchantment card from your graveyard to
# the battlefield" — Rescue Skiff's ETB body (the only EoE/DSK printing
# of this exact widened-pool shape today). Maps to
# ``ReturnCreatureFromYourGraveyardToBattlefield(or_enchantment=True)``,
# mirroring the ``or_spacecraft=True`` rider on the to-hand sibling
# (Fell Gravship). Tried BEFORE the bare ``return target creature card
# from your graveyard to the battlefield`` regex below for the more-
# specific-first convention; the bare regex's ``$`` anchor on
# ``battlefield`` would not match the ``or enchantment`` rider anyway.
_RETURN_TARGET_CREATURE_OR_ENCHANTMENT_GRAVEYARD_TO_BATTLEFIELD = re.compile(
    r"^return target creature or enchantment card from your graveyard "
    r"to the battlefield$"
)
# "Return target creature or Spacecraft card from your graveyard to the
# battlefield" — Seedship Broodtender's activated sorcery-speed body.
# Mirrors ``_RETURN_TARGET_CREATURE_OR_ENCHANTMENT_GRAVEYARD_TO_BATTLEFIELD``
# above; the only EoE printing with the Spacecraft widening today.
# Tried BEFORE the bare creature regex below — the bare regex's ``$``
# anchor on ``battlefield`` wouldn't match the ``or Spacecraft`` rider
# anyway, but more-specific-first is the convention.
_RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD = re.compile(
    r"^return target creature or spacecraft card from your graveyard "
    r"to the battlefield$",
    re.IGNORECASE,
)
# "Return target creature card with mana value N or less from your
# graveyard to the battlefield" — narrowed-by-MV reanimation. Surgical
# Suite's door trigger ("with mana value 3 or less") is the EoE/DSK
# surface form; the captured digit lands as the engine class's
# ``max_mv`` rider, same flag the bare regex above leaves unset
# (None = unbounded). Tried BEFORE the bare regex so a phrase
# carrying the MV qualifier wins; the bare regex's ``$`` anchor on
# ``battlefield`` would never match the MV tail anyway, but ordering
# more-specific before more-general is the convention here. Mirrors
# the ``_RETURN_TARGET_CREATURE_GRAVEYARD_TO_HAND_WITH_MV`` shape on
# the to-hand sibling.
_RETURN_TARGET_CREATURE_GRAVEYARD_TO_BATTLEFIELD_WITH_MV = re.compile(
    r"^return target creature card with mana value (\d+) or less "
    r"from your graveyard to the battlefield$"
)
# "Return target creature or Spacecraft card with mana value N or less
# from your graveyard to the battlefield" — combines the ``or_spacecraft``
# widening (Scrounge for Eternity; "with mana value 5 or less") with the
# ``max_mv`` cap (same flag as the bare MV variant above). Tried BEFORE
# both the bare MV variant (which anchors on ``creature card``) and the
# bare ``or Spacecraft`` variant (which anchors on ``battlefield`` with no
# MV qualifier) so the more-specific combined form wins on phrases that
# carry both. The engine class is
# ``ReturnCreatureFromYourGraveyardToBattlefield(or_spacecraft=True,
# max_mv=N)``.
#
# Currently only Scrounge for Eternity in the EoE pool carries both the
# ``or Spacecraft`` widening and the mana-value cap on the to-battlefield
# path; the regex captures the integer bound as a parameter so a future
# printing with a different MV cap (e.g. ``3 or less``) lands on the
# same primitive without further grammar work — the parameterized-
# recognizer-preferred form per parser CLAUDE.md.
_RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV = re.compile(
    r"^return target creature or Spacecraft card with mana value (\d+) or less "
    r"from your graveyard to the battlefield$",
)
# Multi-sentence Xu-Ifit, Osteoharmonist variant — the printed oracle is
# ``Return target creature card from your graveyard to the battlefield.
# It's a Skeleton in addition to its other types and has no abilities.``
# The ``It's a Skeleton …`` rider is intentionally dropped (the engine
# doesn't yet model "has no abilities" suppression nor in-place subtype
# grants on an arriving creature). This mirrors the matching
# simplification in the static registry — the resulting primitive is
# the same bare ``ReturnCreatureFromYourGraveyardToBattlefield`` the
# bare regex above produces. Kept as its own regex (rather than a
# preprocessing strip) so the simplification is local and explicit:
# the only wrapping that matches this surface verbatim is Xu-Ifit's,
# and any future printing with a different rider tail will hit the
# bare regex's ``$`` anchor and surface a parser miss instead of being
# silently dropped.
_RETURN_TARGET_CREATURE_GRAVEYARD_TO_BATTLEFIELD_AS_SKELETON = re.compile(
    r"^return target creature card from your graveyard "
    r"to the battlefield\. "
    r"It's an? (?P<subtype>[A-Z][a-z]+) in addition to its other types"
    r"(?P<no_abilities> and has no abilities)?$"
)
# "Return all creature cards from your graveyard to the battlefield"
# — mass reanimation (Awakening Hall's door trigger; the half-card
# behind Funeral Room // Awakening Hall is the only EoE/DSK printing
# of this exact surface today). Maps to the existing
# ``ReturnAllCreaturesFromYourGraveyardToBattlefield`` engine class.
#
# Literal-line recognizer per the architectural guidance: only one
# card in the data pool fits this exact surface, and the engine has
# no parameterized "all <zone>" sibling primitive yet (no
# ``...ToHand`` mass-recursion engine class), so generalizing across
# destination zones isn't expressible today. Deferred generalization:
# when a future printing adds a sibling shape (e.g.
# ``return all creature cards from your graveyard to your hand``) and
# the engine grows the matching primitive, fold both into one regex
# with a captured destination group rather than copy-paste this
# literal.
_RETURN_ALL_CREATURE_CARDS_GRAVEYARD_TO_BATTLEFIELD = re.compile(
    r"^return all creature cards from your graveyard to the battlefield$"
)
# DSK Enduring cycle dies-trigger body: "return it to the battlefield
# under its owner's control. It's an enchantment." Every Enduring
# creature in DSK (Innocence, Curiosity, Tenacity, Courage, Vitality)
# prints the same self-return body verbatim. The phrase appears as
# the EFFECT tail of an intervening-if line ``When this creature
# dies, if it was a creature, …`` — the gate is handled by the
# ``_INTERVENING_IF_PHRASES`` ``"it was a creature"`` entry; this
# regex matches the bare effect body that lands in the trailing
# group.
#
# The body is two sentences glued together. Reminder text
# (``(It's not a creature.)``) has already been stripped by
# ``_strip_reminder_text`` before line-level parsing, so the
# parenthesized rule reminder doesn't appear here.
#
# Maps to ``EnduringDiesReturnAsEnchantmentEffect()`` — a
# parameterless engine class that reads ``state.resolving_source`` to
# locate the dying card in its owner's graveyard, moves it back onto
# the battlefield under the owner's control, and sets the
# ``enduring_returned_as_enchantment`` flag on the returned permanent
# so the engine's ``is_creature_now`` short-circuits to False.
_ENDURING_DIES_RETURN_AS_ENCHANTMENT = re.compile(
    r"^return it to the battlefield under its owner's control\. "
    r"It's an enchantment$"
)
# "Return target artifact card from your graveyard to your hand" —
# matches Scour for Scrap's mode (parsed via the modal combinator
# elsewhere, but a primitive worth surfacing in the unified grammar).
_RETURN_TARGET_ARTIFACT_GRAVEYARD_TO_HAND = re.compile(
    r"^return target artifact card from your graveyard to your hand$"
)
# "Put target card from a graveyard on the bottom of its owner's
# library" — Chrome Companion's tap-mana activated ability. Untyped
# grave-target (any card in any player's graveyard is legal); the
# resolution moves the chosen card to the bottom of its owner's
# library. Maps to the existing ``PutCardFromGraveyardOnBottomOfLibrary``
# engine class. Surface form is post-normalization (the ``it `` prefix-
# strip in ``_normalize_creature_effect_phrase`` doesn't fire because
# the phrase starts with the verb ``put``, not a pronoun subject — so
# the ``its`` here survives intact, mirroring the ``put its counters
# on …`` precedent).
_PUT_CARD_FROM_GRAVEYARD_ON_BOTTOM_OF_LIBRARY = re.compile(
    r"^put target card from a graveyard "
    r"on the bottom of its owner's library$"
)
# "Exile target card from a graveyard" — Ghost Vacuum's ``{T}`` activated
# ability. Sibling to the bottom-of-library shape above; same
# ``("card_in_graveyard", id)`` target-kind pool, different destination
# zone. Maps to the new ``ExileTargetCardFromGraveyard`` engine class
# whose resolve also tags the exiled card with the source's id via
# ``exiled_by_source`` so a sibling ``{6}, {T}, Sacrifice this artifact``
# recall ability (Ghost Vacuum's second activation) can find every card
# the same source exiled later. The tag is harmless on a source whose
# only graveyard interaction is this exile primitive — no recall.
_EXILE_CARD_FROM_GRAVEYARD = re.compile(
    r"^exile target card from a graveyard$"
)
# "Target creature gets +N/+M until end of turn" or
# "Target creature gets -N/-M until end of turn". Both signs supported
# (PumpTargetCreatureUntilEOT folds positive and negative bonuses
# through the same pump path). Mixed signs (``+1/-1``) are also
# accepted to keep the grammar uniform — no EoE card prints that today
# but the regex stays the simplest correct shape.
_PUMP_TARGET_CREATURE_UEOT = re.compile(
    r"^target creature gets ([+-]\d+)/([+-]\d+) until end of turn$"
)
# "Target creature gets +X/+0 until end of turn" — the variable-X pump
# where X is a cost-payment-determined value (Secluded Starforge's
# "{2},{T}, Tap X untapped artifacts you control: Target creature gets
# +X/+0 until end of turn"). Maps to
# ``PumpTargetCreatureUntilEOT(power_bonus_from_x=True)`` whose resolve
# reads ``state.resolving_x_value``. Only the +X/+0 (toughness 0) shape
# is in the pool today; a +X/+X form would need the regex widened.
_PUMP_TARGET_CREATURE_X_POWER_UEOT = re.compile(
    r"^target creature gets \+X/\+0 until end of turn$"
)
# "Up to one target creature gets ±N/±M until end of turn" — optional
# pump/debuff with no fizzle when the battlefield has no creatures.
# Surfaces on Dubious Delicacy's ETB ("up to one target creature gets
# -3/-3 until end of turn"). Maps to
# ``PumpTargetCreatureUntilEOT(power_bonus=N, toughness_bonus=M,
# optional=True)`` — the ``optional`` flag flips the engine class's
# ``target_count_range`` to ``(0, 1)`` and ``requires_target`` to
# False, so a no-target resolution is a clean no-op rather than a
# fizzle.
_PUMP_UP_TO_ONE_TARGET_CREATURE_UEOT = re.compile(
    r"^up to one target creature gets ([+-]\d+)/([+-]\d+) "
    r"until end of turn$"
)
# "Target creature an opponent controls gets -N/-M until end of turn" —
# pump variant restricted to opposing creatures (Sinister Cryologist's
# ETB debuff). Maps to ``PumpTargetCreatureUntilEOT(opponents_only=True)``
# which gates ``legal_targets`` to creatures NOT controlled by the
# resolving controller. Negative bonuses are typical here (debuff
# semantics) but the regex accepts both signs so a future card that
# prints a positive opponent-only pump (rare but possible) parses
# without further grammar work — symmetric with the bare pump regex
# above.
_PUMP_TARGET_OPPONENT_CREATURE_UEOT = re.compile(
    r"^target creature an opponent controls gets ([+-]\d+)/([+-]\d+) "
    r"until end of turn$"
)
# "Creatures your opponents control get -N/-M until end of turn" /
# "...until your next turn" — mass debuff on every creature an opponent
# controls (Mouth of the Storm's ETB). Maps to
# ``PumpCreaturesOpponentsControlUntilEOT(power_bonus=N, toughness_bonus=M,
# until_caster_next_turn=...)``. The ``until your next turn`` tail
# extends the duration past the casting turn's cleanup; the bare
# ``until end of turn`` tail clears at the normal cleanup step.
_PUMP_OPPONENT_CREATURES_UEOT = re.compile(
    r"^creatures your opponents control get ([+-]\d+)/([+-]\d+) "
    r"until (end of turn|your next turn)$"
)
# "[Other ]creatures you control get +N/+M[ and gain <kw>[ and <kw2>...]]
# until end of turn" — anthem-style mass pump with optional
# source-self exclusion (``other``) and optional grant-keywords rider
# (``and gain <kw>``). Maps to ``PumpCreaturesYouControlUntilEOT``
# with ``exclude_source`` flipped by group-1 capture and
# ``granted_keywords`` populated from the body parsed by
# ``_parse_keyword_grant_body`` against the keyword-line whitelist.
#
# Surface coverage today:
#   * bare ``creatures you control get +N/+M until end of turn`` —
#     Dual-Sun Adepts' ``{5}`` mass pump (no exclusion, no grants).
#   * ``other creatures you control get +1/+0 and gain deathtouch
#     until end of turn`` — Syr Vondam, the Lucent's enters/attacks
#     trigger (exclude_source + granted_keywords={'deathtouch'}).
#
# Both signs accepted for the bonuses (no EoE printing uses negative
# self-mass pump today, but the uniform shape parses one without
# further grammar work). The ``other`` and ``and gain <kw>`` clauses
# are independently optional — every combination flows through the
# same engine class with the corresponding flags/fields populated.
_PUMP_YOUR_CREATURES_UEOT = re.compile(
    r"^(other )?creatures you control get ([+-]\d+)/([+-]\d+)"
    r"(?: and gain (.+))? until end of turn$"
)
# "Creatures you control get +N/+M until end of turn. If it's not your
# turn, untap those creatures." — anthem-style mass pump with a flash-
# untap rider that fires when cast on an opponent's turn (Zealous
# Display ``{2}{W}`` Instant). Two-sentence recognizer: the second
# sentence's ``those creatures`` is anaphoric to the pumped set, so
# the two sentences must be kept together.
#
# Two capture groups: pump power (group 1) and pump toughness (group 2).
# Maps to ``PumpCreaturesYouControlUntilEOT(power_bonus=N,
# toughness_bonus=M, also_untap_if_not_your_turn=True)``.
#
# Currently only Zealous Display in the pool fits this shape; the
# regex captures the amounts as parameters so a future printing with
# different pump values (e.g. "+1/+1 UEOT + untap if not your turn")
# lands on the same primitive without further grammar work.
_PUMP_YOUR_CREATURES_UEOT_UNTAP_IF_NOT_YOUR_TURN = re.compile(
    r"^creatures you control get ([+-]\d+)/([+-]\d+) until end of turn\. "
    r"If it's not your turn, untap those creatures$"
)
# "Target creature you control gets +N/+M until end of turn" —
# you-control-only single-target pump (Zookeeper Mechan's ``{6}{R}``
# sorcery-speed activated ability). Maps to
# ``PumpTargetCreatureUntilEOT(you_control_only=True)`` whose
# ``legal_targets`` filters the standard creature pool to creatures the
# resolving controller controls. Tried BEFORE the bare ``target creature
# gets ... UEOT`` regex so the ``you control`` qualifier wins; the bare
# regex's ``$`` anchor wouldn't match the qualifier anyway, but
# ordering more-specific before more-general is the convention in this
# dispatcher.
_PUMP_TARGET_CREATURE_YOU_CONTROL_UEOT = re.compile(
    r"^target creature you control gets ([+-]\d+)/([+-]\d+) "
    r"until end of turn$"
)
# "Target creature you control gets +N/+M until end of turn. If that
# creature was cast for its warp cost, it also gains <kw> until end
# of turn." — warp-conditional keyword-grant rider on a
# you-control pump (Full Bore ``{1}{R}`` Instant). Two-sentence
# recognizer: the second sentence's ``that creature`` is anaphoric to
# the pump target, so the two sentences must be kept together.
#
# Three capture groups: pump power (group 1), pump toughness (group 2),
# and the keyword body (group 3). Maps to
# ``PumpTargetCreatureUntilEOT(power_bonus=N, toughness_bonus=M,
# you_control_only=True, granted_keywords_if_warp_cast=<kw-set>)``.
#
# Currently only Full Bore in the pool fits this shape; the regex
# captures the amounts and keywords as parameters so a future printing
# with different values (e.g. "+2/+1" or additional keywords) lands on
# the same primitive without further grammar work. Per parser CLAUDE.md,
# a parameterized recognizer is preferred even when only one card in
# the pool fits — the captured groups are the varying pieces.
_PUMP_TARGET_CREATURE_YOU_CONTROL_WARP_KEYWORDS_RIDER_UEOT = re.compile(
    r"^target creature you control gets ([+-]\d+)/([+-]\d+) "
    r"until end of turn\. "
    r"If that creature was cast for its warp cost, it also gains "
    r"(.+) until end of turn$"
)
# "Double the power of target creature you control until end of turn"
# — Mightform Harmonizer's landfall trigger body. The
# ``DoubleTargetCreaturesPowerUEOT`` engine class adds the target's
# current effective power as a temporary power bonus that clears at
# end-of-turn cleanup. Distinct from the ``gets +N/+0`` UEOT pump
# regexes above: the bonus amount is computed dynamically from the
# target's current power at resolve time, not a fixed constant. Maps
# to the engine class with no parameters — the legal-target predicate
# (``target creature you control``) and duration (``until end of
# turn``) are both encoded by the class.
_DOUBLE_TARGET_CREATURE_YOU_CONTROL_POWER_UEOT = re.compile(
    r"^double the power of target creature you control until end of "
    r"turn$"
)
# "Target creature can't be blocked this turn." — bare unblockable-
# UEOT grant on a creature target. Distinct from the larger pump-with-
# unblockable-rider shapes (Atomic Microsizer's attack trigger,
# manland animation's optional unblockable rider) — this primitive
# is the bare make-unblockable surface, with no P/T pump and no
# granted keywords. Matches Enter the Enigma's ``Target creature
# can't be blocked this turn.`` prefix; reusable for any future card
# that prints the same surface.
_TARGET_CREATURE_CANT_BE_BLOCKED_THIS_TURN = re.compile(
    r"^target creature can't be blocked this turn$"
)
# "Another target attacking creature can't be blocked this turn." —
# Clammy Prowler's attack-trigger body. Same engine class as the bare
# unblockable-UEOT primitive but with the ``attacking_only=True`` and
# ``exclude_self=True`` flags flipped — the legal-target pool is
# narrowed at selection time to "creatures currently flagged as
# attacking, excluding the source itself". Tried BEFORE the bare
# regex because the bare regex's ``$`` anchor would never match this
# longer form anyway, but ordering more-specific before more-general
# is the convention here.
_ANOTHER_TARGET_ATTACKING_CREATURE_CANT_BE_BLOCKED_THIS_TURN = re.compile(
    r"^another target attacking creature can't be blocked this turn$"
)
# "Target creature with power N or less can't be blocked this turn." —
# Ragged Playmate's activated ability body. Same engine class as the
# bare unblockable-UEOT primitive with the ``power_at_most=N`` filter
# narrowing the legal-target pool by effective power at selection
# time. Tried BEFORE the bare ``target creature can't be blocked``
# regex so the more-specific power-filter form wins.
_TARGET_CREATURE_WITH_POWER_AT_MOST_CANT_BE_BLOCKED_THIS_TURN = re.compile(
    r"^target creature with power (\d+) or less can't be blocked this turn$"
)
# Controller loses N life. Surface forms ``lose N life`` (post-compound
# split, where the carried-over subject is implicit — Decode
# Transmissions, Elegy Acolyte, Voidforged Titan all print
# ``you draw a card and lose 1 life``) and ``you lose N life`` (the
# explicit form, included for symmetry with ``you gain N life``). Both
# map to the same ``LoseLife`` primitive — the ``you`` is purely
# surface noise here because the controller is already implicit in
# every effect's resolve signature.
_LOSE_N_LIFE = re.compile(r"^(?:you )?lose (\d+) life$")
# "You may pay {COST}. If you do, <inner-effect>." — the ``OptionalManaCost
# ThenEffect`` wrapper. The inner-effect phrase is parsed recursively via
# the unified grammar, so any single-effect (or compound) primitive that
# parses on its own can serve as the ``If you do, …`` payload — e.g.
# ``put a +1/+1 counter on this creature`` (Oreplate Pangolin) or
# ``draw a card and you gain 1 life`` (synthetic compound).
#
# Cost shape is the standard ``{...}`` braced mana-cost mini-language
# (e.g. ``{2}`` or ``{1}{R}``); it's captured verbatim and handed to
# ``OptionalManaCostThenEffect`` which parses it via
# ``parse_mana_cost`` at resolve time.
#
# Note the embedded period after the cost: oracle text always prints
# ``You may pay {N}. If you do, …`` as TWO sentences. The
# normalization layers (``_normalize_creature_effect_phrase`` and
# ``_normalize_spell_effect_phrase``) only strip the OUTER trailing
# period — the inner sentence-boundary period stays as a literal
# match here.
_YOU_MAY_PAY_THEN_EFFECT = re.compile(
    r"^you may pay (\{[^}]+(?:\}\{[^}]+)*\})\. If you do, (.+)$"
)
# "You may sacrifice an artifact. When you do, <inner-effect>." —
# the ``OptionalSacArtifactForEffect(inner_effect=<inner>)`` compound.
# Scryfall prints the consent clause and the consequence as TWO
# sentences joined by ``When you do,``; the ``_SENTENCE_BOUNDARY_RE``
# negative lookahead in ``spell_builder._split_sentences`` prevents
# splitting at that boundary so the entire two-sentence phrase reaches
# this recognizer unsplit.  A leading ``then `` (which Lithobraking
# prints as the second token after a ``Create a Lander token.`` first
# sentence) is accepted via the optional group so the recognizer claims
# the Lithobraking surface after the ``_split_sentences`` carves the
# first sentence off.
#
# The inner-effect tail (``group(1)``) is parsed recursively by
# ``_parse_effect``; the capturing group ends at ``$`` so it must span
# to the end of the (already period-stripped) phrase.  If the inner
# effect fails to parse, the whole recognizer rejects cleanly and the
# spell stays unclaimed (mirrors ``_YOU_MAY_PAY_THEN_EFFECT``'s
# behavior on inner-parse failure).
#
# Note the embedded period: oracle text always prints
# ``You may sacrifice an artifact. When you do, …`` as two sentences;
# the inner period matches literally here (same convention as
# ``_YOU_MAY_PAY_THEN_EFFECT``).
_YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO = re.compile(
    r"^(?:then )?you may sacrifice an artifact\. When you do, (.+)$"
)
# "Target creature gets +N/+M and gains <kw>[ and <kw2>...] until end of
# turn." — pump + grant-keywords compound. The granted-keywords body
# (between ``and gains `` and `` until end of turn``) is one or more
# evergreen-keyword tokens joined by `` and `` and validated against the
# keyword-line whitelist before the parsed effect is returned. Four
# subject-shapes share the same regex via the optional prefix groups:
#
# * Bare ``target creature gets ... UEOT`` (Rig for War, Dark Endurance).
# * ``target creature you control gets ... UEOT`` (Kavaron Turbodrone,
#   Molecular Modifier — activated/triggered abilities not yet wired by
#   the surrounding builders, but the effect primitive is in place).
# * ``another target creature you control gets ... UEOT`` (Hardened
#   Escort — Duskmourn). The ``another`` qualifier excludes the trigger
#   source from the legal pool, encoded by the engine class's
#   ``exclude_self=True`` flag.
# * ``target attacking creature gets ... UEOT`` (Most Valuable Slayer —
#   Duskmourn). The ``attacking`` qualifier narrows the legal pool to
#   creatures currently flagged ``attacking=True``, encoded by the
#   engine class's ``attacking_only=True`` flag. Mutually exclusive with
#   the ``another``/``you control`` qualifiers in print (no DSK card
#   layers them) but the regex captures each group independently so the
#   handler can flip flags orthogonally — adding a hypothetical
#   ``another target attacking creature you control gets ...`` line
#   later wouldn't need further grammar work.
#
# Future cards with multiple keywords (``... and gains first strike and
# reach UEOT``) parse through the same shape. The ``another``,
# ``attacking``, and ``you control`` groups are captured separately so
# the handler can flip the matching engine flags independently —
# ``another`` always co-occurs with ``you control`` in the surface
# forms in print today, but the regex doesn't enforce that pairing
# because the engine's ``exclude_self=True`` flag is meaningful even on
# bare-target effects.
# "Until end of turn, target creature gets +N/+M, gains <kw>, and becomes
# <type-change> in addition to its other types." — Jump Scare (Duskmourn
# ``{W}`` Instant). A leading-UEOT pump with a keyword grant AND an
# unimplemented type-change rider ("becomes a Horror enchantment creature
# in addition to its other types"). The type-change is dropped because the
# engine has no UEOT-scoped type-line addition on targeted creatures; the
# resulting effect is the pure pump + keyword-grant (same simplification
# the static registry ships). Three capture groups:
#   1. power bonus
#   2. toughness bonus
#   3. keyword grant body (the part between "gains " and ", and becomes")
#
# The "and becomes … in addition to its other types" tail anchors the
# regex; a future "until end of turn, target creature gets +N/+M, gains
# <kw>" card WITHOUT a type-change rider would NOT match this regex and
# would instead need a sibling for the bare leading-UEOT shape (none in
# the current pool, so deferred until a second example exists).
#
# Tried BEFORE the bare-trailing-UEOT ``_PUMP_AND_GAIN_KEYWORDS_UEOT``
# regex so the more-specific leading-UEOT + type-change-rider form wins;
# the trailing-UEOT regex's ``until end of turn$`` anchor would never
# match this leading-UEOT surface anyway, but ordering more-specific
# before more-general is the convention here.
_PUMP_LEADING_UEOT_KEYWORDS_BECOMES_TYPE = re.compile(
    r"^until end of turn, target creature gets ([+-]\d+)/([+-]\d+), "
    r"gains (.+?), and becomes .+ in addition to its other types$"
)
_PUMP_AND_GAIN_KEYWORDS_UEOT = re.compile(
    r"^(another )?target (attacking )?creature (you control )?"
    r"gets ([+-]\d+)/([+-]\d+) and gains "
    r"(.+) until end of turn$"
)
# "You may discard a card. If you do, draw N cards." — composes
# ``IfYouDo(MayDo(DiscardSelf(1)), DrawCards(N))`` from the generic
# combinators. The amount tail accepts the Scryfall standard variants
# for the count: the article ``a`` for the singular ``draw a card``,
# spelled-out small cardinals (``draw two cards`` is Melded Moxite's
# printed form), and bare digits for any future card with ``draw 3
# cards``. Only the count is captured from the surface text — the
# rest is wired structurally via the generic combinators.
_OPTIONAL_DISCARD_THEN_DRAW = re.compile(
    rf"^you may discard a card\. If you do, "
    rf"draw (a|\d+|{_SMALL_CARDINAL_PATTERN}) cards?$"
)
# "You may discard your hand. If you do, draw N cards." — Vaultguard
# Trooper's tapped-creatures-gated end-step trigger inner effect (the
# ``ConditionalOnTappedCreatures`` wrapper sits at the trigger-line
# level, not inside the effect-phrase grammar). Distinct from the
# single-card discard-then-draw above: discards the WHOLE hand, not a
# single chosen card. Maps to ``DiscardHandThenDrawN(amount=N)``. The
# amount tail accepts the Scryfall-printed forms — bare digits and
# spelled-out small cardinals (``two`` is what Vaultguard prints);
# ``a`` would collapse to 1 if a future card prints
# ``draw a card`` after a hand discard.
_OPTIONAL_DISCARD_HAND_THEN_DRAW = re.compile(
    rf"^you may discard your hand\. If you do, "
    rf"draw (a|\d+|{_SMALL_CARDINAL_PATTERN}) cards?$"
)
# "You may put a (creature|land) card from your hand onto the
# battlefield." — parameterized over the card-type word. Sledge-Class
# Seedship (creature, attack trigger) and Burgeoning (land, opponent-
# plays-land trigger) both fit this shape; the captured group drives
# the engine-class dispatch:
#   creature → ``OptionalPutCreatureFromHandOntoBattlefield()``
#   land     → ``OptionalPutLandFromHandOntoBattlefield()``
# Both classes delegate the optional choice through the resolving
# player's agent and treat declining as a legal no-op. A future
# printing with a different card-type word (artifact, enchantment, etc.)
# would extend the alternation and add a matching engine class.
_OPTIONAL_PUT_PERMANENT_FROM_HAND_ONTO_BATTLEFIELD = re.compile(
    r"^you may put a (creature|land) card from your hand "
    r"onto the battlefield$"
)
# "You may sacrifice another creature or artifact. If you do, this
# creature gets +N/+M until end of turn." — the
# ``OptionalSacAnotherForPumpSelfUEOT`` wrapper. Mirrors the surface
# form on Comet Crawler's attack trigger. Pump-self-only (no
# ``granted_keywords`` rider parsed today; no EoE printing pairs the
# self-sac pump with an inner keyword grant). Tried BEFORE the more
# general ``OptionalSacAnotherForEffect`` regex so the self-pump shape
# wins and produces the specialized engine class instead of falling
# through.
_OPTIONAL_SAC_ANOTHER_FOR_PUMP_SELF_UEOT = re.compile(
    r"^you may sacrifice another creature or artifact\. If you do, "
    r"this creature gets ([+-]\d+)/([+-]\d+) until end of turn$"
)
# "You may sacrifice a land or Lander. If you do, search your library
# for up to N basic land cards, put them onto the battlefield tapped,
# then shuffle." — Larval Scoutlander's ETB body (the only EoE/DSK
# printing fitting this exact shape today). Maps to
# ``OptionalSacLandOrLanderForBasicLandRamp(land_count=N)`` whose
# resolve handles the optional sac + tutor-tapped sequence via the
# existing combinator. Captures the count tail (spelled-out small
# cardinal, e.g. ``two``) for forward-compat with any future printing
# that varies the count. Literal-line otherwise: the leading "you may
# sacrifice a land or Lander" subject is unique to this card today;
# broader generalization (parameterizing the sacrificed object's
# noun phrase, or merging with the ``OptionalSacAnotherForEffect``
# shape) is deferred until a second printing with a varying
# sacrificed-object surface ships, at which point the right move is a
# parameterized regex over <subject> and <inner-effect> with the
# inner phrase parsed recursively through ``_parse_effect_phrase``.
_OPTIONAL_SAC_LAND_OR_LANDER_FOR_BASIC_LAND_RAMP = re.compile(
    rf"^you may sacrifice a land or Lander\. If you do, "
    rf"search your library for up to "
    rf"(\d+|{_SMALL_CARDINAL_PATTERN}) basic land cards, "
    rf"put them onto the battlefield tapped, then shuffle$"
)
# "You may sacrifice an artifact. When you do, <inner-effect>." —
# the ``OptionalSacArtifactForEffect`` wrapper. Surface form uses
# "When you do" (reflexive trigger) rather than "If you do"; the
# engine class treats both phrasings identically (sacrifice is optional;
# inner effect runs only if the sacrifice is paid). Inner-effect phrase
# is parsed recursively so any parseable effect (or compound) can serve
# as the consequence. Selfcraft Mechan's ETB prints this shape with
# inner "put a +1/+1 counter on target creature and draw a card".
# Tried BEFORE the more general ``_OPTIONAL_SAC_ANOTHER_FOR_EFFECT``
# so the more-specific artifact-only object wins on phrases that carry
# "sacrifice an artifact" (the general regex requires "another creature
# or artifact" and would not match this anyway, but ordering keeps the
# most-specific-first convention intact).
_OPTIONAL_SAC_ARTIFACT_WHEN_YOU_DO_EFFECT = re.compile(
    r"^you may sacrifice an artifact\. When you do, (.+)$"
)
# "You may sacrifice another creature or artifact. If you do, <inner-
# effect>." — the general ``OptionalSacAnotherForEffect`` wrapper.
# Inner-effect phrase is parsed recursively via the unified grammar so
# any single-effect (or compound) primitive that parses on its own can
# serve as the consequence (e.g. ``draw a card`` on Swarm Culler, or a
# ``put a +1/+1 counter on this creature`` on a future printing).
# Failure to parse the inner rejects the whole line so the wrapping
# card stays unclaimed rather than silently dropping the consequence.
_OPTIONAL_SAC_ANOTHER_FOR_EFFECT = re.compile(
    r"^you may sacrifice another creature or artifact\. If you do, (.+)$"
)
# "Each opponent discards a card" / "Each opponent discards N cards" —
# untargeted opponent-wide discard (Virus Beetle's ETB and any other
# wide-discard surface). The amount tail accepts the article ``a`` for
# the singular form (Scryfall's printed shape — every EoE card uses
# ``a card``) plus bare digits and spelled-out small cardinals so a
# future ``each opponent discards two cards`` line parses without
# further grammar work. ``a`` collapses to count=1 in the handler.
_EACH_OPPONENT_DISCARDS = re.compile(
    rf"^each opponent discards (a|\d+|{_SMALL_CARDINAL_PATTERN}) cards?$"
)
# "Each opponent sacrifices a nontoken creature of their choice." —
# Susurian Dirgecraft's ETB trigger body. Untargeted opponent-wide
# sacrifice; each affected opponent picks via their agent's
# ``choose_sacrifice``. Maps to ``EachOpponentSacrificesCreature()``
# whose engine class skips opponents with no legal nontoken creature.
# The ``of their choice`` tail is verbatim oracle text — the standard
# MTG wording that places target choice on the sacrificing player
# rather than the source's controller. No targeted variant exists in
# the current pool, so this is a single literal-line regex with no
# captured groups.
_EACH_OPPONENT_SACRIFICES_CREATURE = re.compile(
    r"^each opponent sacrifices a nontoken creature of their choice$"
)
# "Each opponent chooses a creature they control and exiles it." —
# Sothera, the Supervoid's DIES-trigger body. Each opponent independently
# picks one creature they control via their agent's ``choose_sacrifice``
# (re-used as a generic "pick one of your battlefield creatures" prompt),
# and the chosen object moves to exile with
# ``exiled_by_source = state.resolving_source.id`` so Sothera's end-step
# recall can find "creature card exiled with it". The link is requested
# at the parser site by passing ``link_to_source=True`` on
# ``EachOpponentExilesOwnCreature``. Literal-line recognizer with no
# capture groups — Sothera is the only EoE/DSK card in the pool with
# this exact "each opponent chooses … exiles it" surface today; broader
# generalization deferred until a second example exists (e.g. an effect
# with a different filter on what each opponent must pick).
_EACH_OPPONENT_EXILES_OWN_CREATURE = re.compile(
    r"^each opponent chooses a creature they control and exiles it$"
)
# "Defending player loses half their life, rounded up." —
# Alpharael, Stonechosen's Void attack trigger effect (after the
# ability-word prefix and intervening-if gate are stripped by the
# trigger parser). The engine simplifies "defending player" to
# "each opponent" via ``DefendingPlayerLosesHalfLifeRoundedUp``
# — correct in 2-player, documented approximation in multiplayer
# (per the engine class docstring). Literal-line recognizer:
# Alpharael, Stonechosen is the only card in the pool with this
# exact surface today; broader generalization deferred until a
# second card with a "half life" effect shape ships.
_DEFENDING_PLAYER_LOSES_HALF_LIFE_ROUNDED_UP = re.compile(
    r"^defending player loses half their life, rounded up$"
)
# "Target opponent reveals their hand. You choose a nonland card from
# it. That player discards that card." — directed discard where the
# CASTER picks which card is discarded (Temporal Intervention in EoE).
# Three-sentence multi-sentence recognizer: the three sentences cannot
# be parsed individually because the second and third sentences are
# riders that presuppose the reveal context of the first. Matched as
# a single multi-sentence phrase so ``_parse_spell_oracle_text``'s
# clause-splitter doesn't break it apart before the unified primitive
# can claim it.
#
# Currently only Temporal Intervention in the EoE/DSK pool fits this
# exact three-sentence surface form; the recognizer is a literal-line
# match (no captured groups) since no second card with a varying piece
# (e.g. different filter or reveal restriction) exists today. Per
# parser CLAUDE.md, a literal is acceptable when exactly one card in
# the pool fits and the docstring notes why generalization is deferred.
# A future "Target opponent reveals their hand. You choose a card with
# mana value 2 or less from it. That player discards that card." would
# share the underlying directed-discard shape; at that point, replace
# this literal with a parameterized regex capturing the filter clause.
_TARGET_OPPONENT_REVEALS_HAND_CASTER_CHOICE_DISCARD = re.compile(
    r"^target opponent reveals their hand\. "
    r"You choose a nonland card from it\. "
    r"That player discards that card$"
)
# "Defending player mills N cards" — Specimen Freighter's attack
# trigger body. The defending-player surface is the multiplayer-aware
# wording for an attack-trigger mill; the engine simplifies this to
# "each opponent mills N" via ``EachOpponentMillsCards(amount=N)``,
# which is correct in 2-player and a documented approximation in
# multiplayer (per the engine class docstring). The amount tail
# accepts spelled-out small cardinals (``four`` on Specimen
# Freighter) plus bare digits for any future printing.
_DEFENDING_PLAYER_MILLS_N_CARDS = re.compile(
    rf"^defending player mills (\d+|{_SMALL_CARDINAL_PATTERN}) cards?$"
)
# "Up to one other target artifact you control becomes an artifact
# creature with base power and toughness P/T and gains <keywords>
# until end of turn" — Synthesizer Labship's 2+ tier trigger body.
# Parameterized over power (group 1), toughness (group 2), and
# the keyword body (group 3) so a future printing with different
# stats or keywords claims via the same recognizer. Maps to
# ``TargetArtifactBecomesCreatureUEOT(power=P, toughness=T,
# granted_keywords=frozenset({...}))`` — same engine class the
# bespoke table built directly.
#
# The ``and gains <keywords> until end of turn`` clause is optional so
# a future printing that animates an artifact without keyword grants
# also parses; when the clause is absent the
# ``TargetArtifactBecomesCreatureUEOT`` is constructed with
# ``granted_keywords=frozenset()`` (the class default). The keyword
# body is validated via ``_parse_keyword_grant_body`` and rejection
# returns ``None`` so a card with an unrecognized keyword isn't
# silently misfiled.
_TARGET_ARTIFACT_BECOMES_CREATURE_UEOT = re.compile(
    r"^up to one other target artifact you control becomes an artifact "
    r"creature with base power and toughness (\d+)/(\d+)"
    r"(?: and gains (.+))? until end of turn$"
)
# "target creature has base power and toughness N/N until end of turn"
# — CR layer 7b set-base-PT UEOT.  Parameterized over power (group 1)
# and toughness (group 2) so any future printing with a different
# static P/T value claims via the same recognizer.  Maps to
# ``SetBasePTUntilEOT(power=N, toughness=N)``.
#
# This regex handles the bare single-sentence form.  The two-sentence
# conditional form ("… until end of turn. If you control N or more
# lands, that creature has base power and toughness M/M until end of
# turn instead.") is handled by ``_SET_BASE_PT_CONDITIONAL_UEOT``
# below, which must be tried FIRST because the base regex would match
# the first sentence of the conditional body if the splitter carved it.
# "up to one other target creature has base power and toughness N/N
# until end of turn" — optional + other variant of the bare set-base-PT
# UEOT (Restless Vinestalk's attack trigger). Maps to
# ``SetBasePTUntilEOT(N, N, optional=True, exclude_self=True)``.
_SET_BASE_PT_UP_TO_ONE_OTHER_UEOT = re.compile(
    r"^up to one other target creature has base power and toughness "
    r"(\d+)/(\d+) until end of turn$"
)
# "until end of turn, this land becomes a P/T <c1> [and <c2>] <Subtype>
# creature [with <keywords>]. It's still a land" — creature-land
# animation (Restless Vinestalk: "a 5/5 green and blue Plant creature
# with trample"). Parameterized over P/T, color words, one creature
# subtype and a keyword tail; maps to ``BecomeLandCreatureUEOT(power,
# toughness, granted_keywords, granted_subtype, set_colors)``. Keywords
# outside ``_ANIMATED_LAND_KEYWORDS`` reject the phrase.
_THIS_LAND_BECOMES_CREATURE_UEOT = re.compile(
    r"^until end of turn, this land becomes an? (?P<p>\d+)/(?P<t>\d+) "
    r"(?P<colors>(?:white|blue|black|red|green)"
    r"(?:(?:,| and|, and) (?:white|blue|black|red|green))*) "
    r"(?P<subtype>[A-Z][a-z]+) creature(?: with (?P<kws>[^.]+))?\. "
    r"It's still a land$"
)
_COLOR_WORD_LETTERS: dict[str, str] = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}
_ANIMATED_LAND_KEYWORDS = frozenset({
    "flying", "vigilance", "deathtouch", "hexproof", "double strike",
    "lifelink", "reach", "trample", "first strike", "menace", "haste",
})
_SET_BASE_PT_UEOT = re.compile(
    r"^target creature has base power and toughness (\d+)/(\d+) "
    r"until end of turn$"
)
# Two-sentence conditional form: "target creature has base power and
# toughness N/N until end of turn. If you control M or more lands,
# that creature has base power and toughness P/Q until end of turn
# instead." — Genemorph Imago's landfall trigger.
#
# Parameterized over all four stat values (groups 1–4) and the land
# threshold (group 5, spelled-out or digit) so any future printing with
# different numbers claims without new code.  Maps to
# ``SetBasePTConditionalUntilEOT(default_power, default_toughness,
#                                  land_threshold,
#                                  upgrade_power, upgrade_toughness)``.
#
# Must be tried BEFORE ``_SET_BASE_PT_UEOT`` in ``_parse_effect`` so
# the more-specific two-sentence form wins over a bare single-sentence
# match on the first clause.
_SET_BASE_PT_CONDITIONAL_UEOT = re.compile(
    r"^target creature has base power and toughness (\d+)/(\d+) "
    r"until end of turn\. "
    r"If you control (\d+|" + _SMALL_CARDINAL_PATTERN + r") or more lands, "
    r"that creature has base power and toughness (\d+)/(\d+) "
    r"until end of turn instead$"
)
# ETB exile-until-source-leaves family — Oblivion Ring / Banishing
# Light shape.  Two regexes cover the two engine classes:
#
# 1. ``exile target [nonland permanent | artifact, creature, or
#    enchantment] an opponent controls [with mana value N or less]
#    until this [X] leaves the battlefield``
#    → ``ExileOpponentsNonlandPermanentUntilLeaves(max_mv=N)``
#    "artifact, creature, or enchantment" is treated as a synonym for
#    "nonland permanent" (no planeswalkers/battles in the EoE pool).
#
# 2. ``exile [up to one] target [non-Subtype] creature [an opponent
#    controls] until this [X] leaves the battlefield``
#    → ``ExileTargetCreatureUntilSourceLeaves(excluded_subtypes=...,
#                                              opponents_only=bool)``
#
# The trailing "until this [type] leaves" anchor uses a word-character
# capture for the card type so it handles "creature", "enchantment",
# "Aura", "permanent", and any future type without changes.
_EXILE_OPPONENTS_NONLAND_UNTIL_LEAVES = re.compile(
    r"^exile target (?:nonland permanent|artifact, creature, or enchantment) "
    r"an opponent controls"
    r"(?: with mana value (\d+) or less)?"
    r" until this \w+ leaves the battlefield$"
)
_EXILE_CREATURE_UNTIL_LEAVES = re.compile(
    r"^exile( up to one)? target(?: non-([A-Z][a-z]+))? creature"
    r"( an opponent controls)?"
    r" until this \w+ leaves the battlefield$"
)
# Mass exile-until-source-leaves (Pinnacle Starcage shape) —
# ``exile all <type-list> with mana value N or less until this
# <X> leaves the battlefield``. The type list is one or two of
# {artifacts, creatures} joined by `` and ``; the MV cap is required
# (no printed mass exile-until-leaves without one in the current pool;
# extending the regex to make the cap optional is a one-line change
# when a future printing ships without it). The trailing ``until this
# <X> leaves the battlefield`` anchor uses a word-character capture
# for the card type (matches "artifact", "enchantment", "permanent",
# etc.) like the targeted family. Maps to ``ExileAllMatchingUntil
# SourceLeaves`` which exiles all matching battlefield permanents
# (excluding the source) and tags each with ``exiled_by_source`` so
# the generic on-leave release brings them back when the source
# leaves play.
_EXILE_ALL_MATCHING_UNTIL_LEAVES = re.compile(
    r"^exile all "
    r"(artifacts and creatures|artifacts|creatures)"
    r" with mana value (\d+) or less"
    r" until this \w+ leaves the battlefield$"
)
# "Exile target creature or Spacecraft." — Gravkill's printed surface.
# The static ``cards/eoe_spells.py`` Gravkill entry intentionally fudges
# this to ``ExileTargetCreature()`` because Spacecraft aren't yet a
# legal target type for the engine's exile path; the parser mirrors
# that fudge to stay in parity. When the engine grows a target kind
# that covers Spacecraft, both sides will need to switch in lockstep
# to a Spacecraft-aware engine class.
_EXILE_TARGET_CREATURE_OR_SPACECRAFT = re.compile(
    r"^exile target creature or Spacecraft$"
)
# "Destroy target artifact or tapped creature." — Radiant Strike's
# destroy clause. Maps to ``DestroyTargetArtifactOrTappedCreature()``
# whose ``legal_targets`` filters to artifacts (any) or creatures that
# are currently tapped. The "permanent" target kind is used because
# either an artifact or a creature can be hit; legality is policed by
# the engine class's predicate.
_DESTROY_TARGET_ARTIFACT_OR_TAPPED_CREATURE = re.compile(
    r"^destroy target artifact or tapped creature$"
)
# "Target artifact or creature's owner puts it on their choice of the
# top or bottom of their library." — Lost in Space's bounce-to-library
# clause. Maps to ``OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOf
# Library()`` whose target kind is ``("permanent", id)`` so artifacts
# (any) and creatures (any) are both legal; the chosen target's owner
# (not the spell's controller) decides top vs bottom at resolution.
_OWNER_PUTS_TARGET_ARTIFACT_OR_CREATURE_ON_TOP_OR_BOTTOM = re.compile(
    r"^target artifact or creature's owner puts it on their choice of "
    r"the top or bottom of their library$"
)
# "Target nonland permanent's owner puts it on their choice of the top
# or bottom of their library." — Vanish from Sight's bounce-to-library
# clause. Same engine class as the artifact-or-creature shape with the
# ``nonland_permanent=True`` flag set; the wider target pool is the
# only behavioral difference. The owner-decides top-vs-bottom path,
# token-ceases-to-exist handling, and bounce cleanup are all shared.
_OWNER_PUTS_TARGET_NONLAND_PERMANENT_ON_TOP_OR_BOTTOM = re.compile(
    r"^target nonland permanent's owner puts it on their choice of "
    r"the top or bottom of their library$"
)
# "Tap target artifact or creature." — Mental Modulation's middle
# clause and any future card that ships the same surface form. Maps to
# "Tap enchanted creature." / "Tap enchanted permanent." — Aura ETB
# trigger bodies (Stay Hidden, Stay Silent; Tractor Beam). Both surface
# forms map to ``TapEnchantedPermanent``, which resolves the host via
# ``state.resolving_source.attached_to`` and routes through
# ``mtgz_ir.turn.tap_permanent``. "creature" and "permanent" are
# synonymous here — the Aura's host is always a creature in all current
# EoE/DSK cards that carry this line. Only Aura cards in the pool
# currently carry these surfaces; the grammar addition is forward-looking
# so future Aura cards with the same trigger line claim automatically.
_TAP_ENCHANTED_PERMANENT = re.compile(
    r"^tap enchanted (?:creature|permanent)$"
)
# ``TapTargetArtifactOrCreature()``. No-op if the chosen target is
# already tapped at resolution.
_TAP_TARGET_ARTIFACT_OR_CREATURE = re.compile(
    r"^tap target artifact or creature$"
)
# "Tap another target creature." — Starport Security's activated
# ability. ``another`` excludes the source creature (reads
# ``state.resolving_source`` to filter). Maps to
# ``TapAnotherTargetCreature()``. No-op if the chosen target is already
# tapped at resolution. Tried as a separate primitive from the
# ``tap target artifact or creature`` shape because the engine class
# differs (creature-only legal pool, source-self exclusion) and the
# surface tail (no ``artifact or`` qualifier, ``another`` self-exclusion
# fragment) is also distinct.
_TAP_ANOTHER_TARGET_CREATURE = re.compile(
    r"^tap another target creature$"
)
# "Tap target artifact or creature an opponent controls and put a stun
# counter on it" — Mechanozoa's ETB. Single primitive (both clauses
# apply to the same target) — the ``and`` in this surface is a
# conjunction within ONE effect, not a compound joiner. Maps to
# ``TapTargetOpponentPermanentAndStunIt()``. The compound-effect
# splitter does try ``and`` first and produces two pieces, but
# neither piece is a recognized standalone effect, so the splitter
# rejects and we fall through to this single-effect regex.
_TAP_TARGET_OPPONENT_PERMANENT_AND_STUN_IT = re.compile(
    r"^tap target artifact or creature an opponent controls "
    r"and put a stun counter on it$"
)
# "Put a stun counter on up to one target tapped creature." — Cryogen
# Relic's activated ability. Maps to the existing
# ``PutStunCounterOnTappedCreature`` engine class whose
# ``legal_targets`` filters the battlefield to currently-tapped
# creatures and whose ``resolve`` is a no-op when no target is chosen
# (the ``up to one`` optionality fizzles cleanly if the pool is empty).
_PUT_STUN_COUNTER_UP_TO_ONE_TAPPED = re.compile(
    r"^put a stun counter on up to one target tapped creature$"
)
# "Tap up to one target creature and put N stun counters on it." —
# Meat Locker's door trigger (``put two stun counters``). Same
# tap-and-stun-the-same-target shape as
# ``_TAP_TARGET_OPPONENT_PERMANENT_AND_STUN_IT`` (Mechanozoa) — both
# halves apply to the SAME chosen target, with the ``and`` an
# intra-effect conjunction rather than a compound joiner. Differs
# from the Mechanozoa shape on three axes: target qualifier
# (``up to one`` vs ``target … an opponent controls``), target
# type pool (any creature vs artifact-or-creature opponent-controlled),
# and stun-counter count (parameterized digit/cardinal vs the
# Mechanozoa fixed singular). The captured count tail accepts the
# article ``a`` for the singular form (forward-looking — no current
# card prints ``a stun counter on it`` on this exact surface; that
# shape today only appears with the singular Mechanozoa wording),
# bare digits, and spelled-out small cardinals (``two`` is what Meat
# Locker prints). ``a`` collapses to amount=1 in the handler.
# Maps to ``TapAndStunUpToOneTargetCreature(stun_amount=N)``. The
# compound-effect splitter does try ``and`` first and produces two
# pieces, but neither piece is a recognized standalone effect, so
# the splitter rejects and we fall through to this single-effect
# regex.
_TAP_UP_TO_ONE_TARGET_CREATURE_AND_STUN = re.compile(
    rf"^tap up to one target creature and put "
    rf"(a|\d+|{_SMALL_CARDINAL_PATTERN}) stun counters? on it$"
)
# "Discard a card" / "you discard a card" / "you discard N cards" —
# controller-discards. The bare ``discard a card`` surface is what
# appears post-compound-split for ``draw a card, then discard a card``
# (Mechan Navigator's becomes-tapped trigger); the ``you `` prefix
# variant is included for symmetry with ``you gain N life`` /
# ``you lose N life``. Maps to ``DiscardSelf(amount=N)``. ``a``
# collapses to amount=1 in the handler.
_DISCARD_N_CARDS = re.compile(
    rf"^(?:you )?discard (a|\d+|{_SMALL_CARDINAL_PATTERN}) cards?$"
)
# "Untap another target permanent" — Nanoform Sentinel's becomes-tapped
# trigger. The ``another`` qualifier excludes the source permanent from
# the legal target pool — the engine class
# ``UntapTargetOtherPermanent`` reads ``state.resolving_source`` at
# resolve time to filter the chosen target. Maps to
# ``UntapTargetOtherPermanent()`` with no parameters (the engine class's
# semantics fully specify the legal-target predicate).
_UNTAP_ANOTHER_TARGET_PERMANENT = re.compile(
    r"^untap another target permanent$"
)
# "Another target creature you control gets +X/+X until end of turn,
# where X is this creature's power" — Hemosymbic Mite's becomes-tapped
# trigger. The variable-X bonus is fixed at "the source's effective
# power at resolve time" — both the regex and the engine class hardcode
# the source-power semantics; future cards with a different X-source
# (e.g. "where X is the number of artifacts you control") would land
# their own primitive. Maps to
# ``PumpTargetOtherCreatureByXOverXWhereXIsSourcePower()`` with no
# parameters — every aspect of the effect (target restriction to other
# creatures the controller controls, X-source from
# ``state.resolving_source``) is encoded by the engine class.
_PUMP_OTHER_TARGET_CREATURE_BY_X_OVER_X_SOURCE_POWER = re.compile(
    r"^another target creature you control gets \+X/\+X until end of "
    r"turn, where X is this creature's power$"
)
# "Put a +1/+1 counter on each creature you control" — Atmospheric
# Greenhouse's ETB trigger body. Mass +1/+1-counter spread on every
# creature the controller controls (the Spacecraft is itself an
# artifact, not a creature, when the ETB fires, so ``exclude_source``
# isn't needed here — the source is naturally outside the firing
# pool). Maps to ``PutPlusOneCounterOnEachCreatureYouControl(amount=1)``.
# Distinct from the ``other than this creature`` variant below, which
# carries an explicit source-exclusion tail.
_PUT_PLUS_ONE_COUNTER_ON_EACH_CREATURE_YOU_CONTROL = re.compile(
    r"^put a \+1/\+1 counter on each creature you control$"
)
# "Put X +1/+1 counters on each creature you control, where X is this
# creature's power" — Ouroboroid's beginning-of-combat trigger body.
# Mass per-controller-creature counter spread scaled by the source's
# current power; the X-source phrase ``where X is this creature's
# power`` reads ``state.resolving_source.power`` at resolve time
# (matching the engine class
# ``PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower``).
# Literal-line recognizer with no captures — only Ouroboroid in the
# pool prints this exact shape today; broader grammar deferred until
# a second example exists. A future printing whose X-source phrase
# differs (e.g. ``where X is the number of artifacts you control``)
# would land its own primitive rather than parameterizing this one,
# matching the precedent already documented for
# ``_PUMP_OTHER_TARGET_CREATURE_BY_X_OVER_X_SOURCE_POWER`` (where X is
# this creature's power).
_PUT_X_PLUS_ONE_COUNTERS_ON_EACH_CREATURE_YOU_CONTROL_WHERE_X_IS_SOURCE_POWER = (
    re.compile(
        r"^put X \+1/\+1 counters on each creature you control, "
        r"where X is this creature's power$"
    )
)
# "Put a +1/+1 counter on each creature you control other than this
# creature" — Dawnstrike Vanguard's tapped-creatures-gated end-step
# trigger body. Mass +1/+1-counter spread on each creature the
# controller controls except the source itself. Maps to
# ``PutPlusOneCounterOnEachCreatureYouControl(amount=1,
# exclude_source=True)`` — the ``exclude_source=True`` flag reads
# ``state.resolving_source`` at resolve time to skip the source from
# the firing pool. Distinct from the bare ``put a +1/+1 counter on
# each creature you control`` shape (Atmospheric Greenhouse) above,
# which omits the source-exclusion tail.
_PUT_PLUS_ONE_COUNTER_ON_EACH_OTHER_CREATURE_YOU_CONTROL = re.compile(
    r"^put a \+1/\+1 counter on each creature you control other than "
    r"this creature$"
)
# "This creature gets +N/+M until end of turn" — source pumps itself
# (no separate target). Surfaces on Skystinger's block trigger
# ("Whenever this creature blocks a creature with flying, this
# creature gets +5/+0 until end of turn") and the Landfall self-pump
# family (Icecave Crasher +1/+0, Remnant Elemental +2/+0). Both signs
# are accepted in case a future printing self-debuffs (the engine
# class folds positive and negative bonuses through the same path),
# matching the symmetry already documented for the
# ``_PUMP_TARGET_CREATURE_UEOT`` regex above.
#
# Maps to ``PumpSelfUntilEOT(power_bonus=N, toughness_bonus=M)``.
# Distinct from the ``put a +1/+1 counter on this creature`` shape
# (which produces ``PutPlusOneCounterOnSelf``) — counters persist past
# end of turn while the pump bonus clears at cleanup.
#
# The optional ``this Spacecraft `` prefix accepts the activated-
# ability surface where a Spacecraft body explicitly names its own
# source as ``this Spacecraft`` (Debris Field Crusher's ``{1}{R}: This
# Spacecraft gets +2/+0 until end of turn.`` body, post-activated
# normalization to ``this Spacecraft gets +2/+0 until end of turn``).
# Mirrors the ``(?:this Room )?`` precedent on
# ``_DEAL_DAMAGE_TARGET_OPPONENT_CREATURE`` — same self-source pronoun
# pattern Rooms use, here applied to the Spacecraft type. The source-
# identity check happens through the enclosing ability's wiring (an
# activated ability's source is implicitly the host), not the effect
# body — so the prefix is purely a surface-noise admission.
_PUMP_SELF_UEOT = re.compile(
    r"^(?:this creature |this Spacecraft )?"
    r"gets ([+-]\d+)/([+-]\d+) until end of turn$"
)
# "This creature gets +N/+M and gains <kw>[ and <kw2>...] until end of
# turn" — source pumps itself AND grants itself one or more evergreen
# keywords UEOT. Surfaces on Duskmourn Eerie creatures (Dashing
# Bloodsucker — ``this creature gets +2/+0 and gains lifelink until
# end of turn``). Maps to
# ``PumpSelfUntilEOT(power_bonus=N, toughness_bonus=M,
# granted_keywords=...)`` — same engine class as the bare self-pump,
# with the keyword-grant rider. The granted-keywords body is parsed
# via ``_parse_keyword_grant_body`` and validated against the
# keyword-line whitelist; rejection returns ``None`` so an unknown
# keyword keeps the wrapping card unclaimed (mirrors the discipline
# of every other grant-keywords primitive).
#
# Tried BEFORE the bare ``_PUMP_SELF_UEOT`` regex (its ``$`` anchor
# rejects the ``and gains …`` tail anyway, but ordering more-specific
# before more-general is the convention here).
_PUMP_AND_GAIN_KEYWORDS_SELF_UEOT = re.compile(
    r"^(?:this creature )?gets ([+-]\d+)/([+-]\d+) and gains "
    r"(.+) until end of turn$"
)
# "This creature gains <kw>[ and <kw2>...] until end of turn" — pure
# self-grant (no P/T pump). Surfaces on Duskmourn Eerie creatures
# (Cult Healer — ``this creature gains lifelink until end of turn``).
# Maps to ``PumpSelfUntilEOT(power_bonus=0, toughness_bonus=0,
# granted_keywords=...)`` — same engine class as the pump-and-gain
# shape just with zero bonuses, mirroring the
# ``_GRANT_KEYWORDS_TO_TARGET_CREATURE_UEOT`` precedent for the
# targeted variant. Validated via ``_parse_keyword_grant_body``.
_GAIN_KEYWORDS_SELF_UEOT = re.compile(
    r"^(?:this creature )?gains (.+) until end of turn$"
)
# "Another target creature you control gains <kw>[ and <kw2>...] until
# end of turn." — pure keyword-grant variant with no P/T pump,
# restricted to other creatures the controller controls (Blooming
# Stinger's ETB grants deathtouch to another creature you control).
# Maps to ``PumpTargetCreatureUntilEOT(power_bonus=0, toughness_bonus=0,
# granted_keywords=..., you_control_only=True, exclude_self=True)``.
# Distinct from the existing ``_PUMP_AND_GAIN_KEYWORDS_UEOT`` regex
# which requires a leading ``gets +N/+M and gains`` clause; this one
# is for pure-grant phrases. The granted-keywords body is validated
# against the keyword-line whitelist before the parsed effect is
# returned, mirroring the behavior of the pump-and-gain primitive.
_GRANT_KEYWORDS_TO_ANOTHER_TARGET_CREATURE_YOU_CONTROL_UEOT = re.compile(
    r"^another target creature you control gains (.+) until end of turn$"
)
# "Target creature you control gains <kw> until end of turn. If it
# has a +1/+1 counter on it, draw a card." — keyword-grant on a
# you-control target with a conditional draw rider (Dual-Sun Technique
# ``{1}{W}{U}`` Instant). The two sentences are matched as a single
# multi-sentence recognizer because the second sentence's ``it``
# pronoun is anaphoric to the first sentence's target, so splitting
# them would lose the per-target conditionality — the draw must fire
# on the SAME creature that received the keyword grant, not an
# independently selected target.
#
# Two capture groups: the keyword body (group 1) for the grant, and
# the draw amount is fixed at 1 (the only form printed today).
# Maps to ``PumpTargetCreatureUntilEOT(power_bonus=0,
# toughness_bonus=0, granted_keywords=..., you_control_only=True,
# draw_if_target_has_plus_one_counter=1)``.
#
# Currently only Dual-Sun Technique in the EoE/DSK pool fits this
# exact surface; the recognizer is a literal-line match because no
# second card with a varying piece (e.g. different draw amount or
# different counter condition) exists today. Per parser CLAUDE.md,
# a literal is acceptable when exactly one card fits and the
# docstring explains why generalization is deferred.  A future card
# with ``draw two cards instead`` or ``if it has a loyalty counter``
# would share the underlying shape; at that point replace this with
# a parameterized regex capturing the counter type and draw amount.
_GRANT_KEYWORDS_TO_TARGET_CREATURE_YOU_CONTROL_DRAW_IF_COUNTER_UEOT = re.compile(
    r"^target creature you control gains (.+) until end of turn\. "
    r"If it has a \+1/\+1 counter on it, draw a card$"
)
# "Put N +1/+1 counters on target creature you control. It gains
# <kw> until end of turn." — Archenemy's Charm mode 3. Two-sentence
# recognizer: the second sentence's ``it`` is anaphoric to the
# targeted creature, so both sentences must be kept together.
# Maps to ``PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl
# (amount=N, granted_keywords=...)``. The amount tail accepts digits
# and spelled-out small cardinals; the keyword body is validated via
# ``_parse_keyword_grant_body``.
#
# Currently only Archenemy's Charm in the pool fits this exact shape;
# the regex captures both the amount and the keyword body as parameters
# so a future printing with different values (e.g. "three +1/+1
# counters ... It gains flying and haste") lands for free.
_PUT_PLUS_ONE_COUNTERS_ON_TARGET_CREATURE_YOU_CONTROL_IT_GAINS_UEOT = re.compile(
    rf"^put (\d+|{_SMALL_CARDINAL_PATTERN}) \+1/\+1 counters on "
    rf"target creature you control\. It gains (.+) until end of turn$"
)
# "Target [legendary] [attacking] creature gains <kw>[ and <kw2>...]
# until end of turn." — pure keyword-grant variant with no P/T pump
# and no control restriction (Horrid Vigor's ``Target creature gains
# deathtouch and indestructible until end of turn.`` in DSK; Plaza of
# Heroes' ``Target legendary creature gains hexproof and indestructible
# until end of turn.``). Maps to
# ``PumpTargetCreatureUntilEOT(power_bonus=0, toughness_bonus=0,
# granted_keywords=...)`` — bare-target legal pool, same engine class
# as the pump-and-gain shape just with zero bonuses. Tried AFTER the
# more-specific ``you control``/``opponent controls`` variants so the
# qualified shapes win on phrases that carry their qualifiers; this
# bare-target shape only claims phrases without such qualifiers. The
# granted-keywords body is validated against the keyword-line
# whitelist before the parsed effect is returned, mirroring the
# behavior of the other grant-keywords primitives.
#
# The optional ``attacking `` qualifier narrows the legal-target pool
# to creatures currently flagged ``attacking=True`` — same shape as
# the ``(attacking )?`` group on ``_PUMP_AND_GAIN_KEYWORDS_UEOT``,
# here lifted up to the no-pump grant-only sibling. Tunnel of Hate's
# door-trigger body ``target attacking creature gains double strike
# until end of turn`` is the surface form that motivated the
# parameterization; the engine class's ``attacking_only=True`` flag
# is the same one Hospital Room's pump uses, here flipped on by the
# captured qualifier rather than by a literal-line entry.
#
# The optional ``legendary `` qualifier narrows the legal-target pool
# to creatures whose printing carries the Legendary supertype. Plaza
# of Heroes' "{3},{T},Exile this land: Target legendary creature gains
# hexproof and indestructible until end of turn." is the motivating
# printing. Group 1 → ``legendary`` qualifier; group 2 → ``attacking``
# qualifier; group 3 → keyword body.
_GRANT_KEYWORDS_TO_TARGET_CREATURE_UEOT = re.compile(
    r"^target (legendary )?(attacking )?creature gains (.+) until end of turn$"
)
# "Target artifact or creature gains <kw>[ and <kw2>...] until end of
# turn." — pure keyword-grant on a permanent target whose legal pool
# spans both artifacts and creatures (Reroute Systems mode 0). Maps to
# ``PumpTargetArtifactOrCreatureUntilEOT(granted_keywords=...)`` which
# uses the ``("permanent", id)`` target kind so the chosen permanent
# may be either an artifact (any) or a creature.
#
# No P/T pump captured — the engine class doesn't ship one for this
# surface today (Reroute Systems' +0/+0 default is the only form in
# use). The keyword body is parsed via ``_parse_keyword_grant_body``
# and validated against the keyword-line whitelist; rejection returns
# ``None`` so an unknown keyword keeps the wrapping spell unclaimed.
_PUMP_TARGET_ARTIFACT_OR_CREATURE_GAINS_KEYWORDS_UEOT = re.compile(
    r"^target artifact or creature gains (.+) until end of turn$"
)
# "<source> deals N damage to target tapped creature" — Reroute Systems
# mode 1's tapped-only burn. Maps to ``DealDamageToTargetTappedCreature(
# amount=N)`` whose ``legal_targets`` filters to creatures that are
# currently tapped, with a re-check at resolution in case a between-
# cast-and-resolve untap removes the tapped creature from the legal
# pool. The ``deals`` form (no ``it`` / ``you`` subject) is what
# Scryfall prints for spells that name themselves as the subject; the
# spell-effect normalizer strips the ``<Name>`` prefix before this
# regex sees the phrase, so the surface form here is exactly
# ``deals N damage to target tapped creature``.
_DEAL_DAMAGE_TARGET_TAPPED_CREATURE = re.compile(
    r"^deals (\d+) damage to target tapped creature$"
)
# "Put N charge counters on target Spacecraft or Planet you control" —
# Drill Too Deep mode 0 (and any future card with the same surface).
# Maps to ``PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(amount=
# N)``. Amount is a digit or one of the spelled-out small cardinals
# (Drill Too Deep prints ``Put five charge counters …``); the engine
# class's ``amount`` field is an int either way. The
# ``Spacecraft or Planet`` capitalization is verbatim from the oracle
# text — the spell-effect normalizer only lowercases the first letter
# of the phrase, so inner type-name capitalization survives.
_PUT_CHARGE_COUNTERS_ON_TARGET_SPACECRAFT_OR_PLANET_YOU_CONTROL = re.compile(
    rf"^put (\d+|{_SMALL_CARDINAL_PATTERN}) charge counters on "
    rf"target Spacecraft or Planet you control$"
)
# "Target player mills cards equal to your life total" — Space-Time
# Anomaly's printed form. Maps to
# ``MillTargetPlayer(use_controller_life=True)``. The engine class's
# ``use_controller_life`` flag substitutes the controller's current
# life total for the bare ``amount`` field at resolve time, so no
# numeric capture is needed here — the surface form is a literal
# match.
_MILL_TARGET_PLAYER_EQUAL_TO_YOUR_LIFE = re.compile(
    r"^target player mills cards equal to your life total$"
)
# "Target player mills N cards" — bare-amount mill at a single
# target player. Forward-looking surface form (no EoE card prints
# this exact shape today; Space-Time Anomaly is the variable-amount
# variant above), but the regex is included so a future printing of
# ``target player mills three cards`` parses without further grammar
# work. Tried AFTER the ``equal to your life total`` regex so the
# more-specific shape wins on phrases that carry it.
_MILL_TARGET_PLAYER_N_CARDS = re.compile(
    rf"^target player mills (\d+|{_SMALL_CARDINAL_PATTERN}) cards$"
)
# "Each player creates a <Name> token" — bare-name token-creation
# variant whose subject is each living player (every player creates
# the token under their own control). Surfaces on Edge Rover's
# dies trigger (``each player creates a Lander token``). Maps to
# ``CreateToken(token_printing=..., count=N, each_player=True)``.
# The captured groups mirror the shape of ``_CREATE_BARE_NAME_TOKEN``
# below — count word/digit, optional ``tapped`` slot, and the bare
# subtype name that doubles as the registry key — except the leading
# subject is ``each player creates`` instead of the implicit
# controller-creates default.
_EACH_PLAYER_CREATE_BARE_NAME_TOKEN = re.compile(
    rf"^each player creates ({_TOKEN_COUNT_PATTERN}) (tapped )?"
    rf"([A-Z][A-Za-z]*(?: [A-Z][A-Za-z]*)*) tokens?$"
)
# "Create a Lander token. At the beginning of the end step on your
# next turn, sacrifice that token." — Kav Landseeker's ETB. Single
# primitive that creates a specific Lander then schedules a delayed-
# trigger sac of that exact token at the controller's next end step.
# The two-sentence surface form is captured as a single literal regex
# — the second sentence's ``sacrifice that token`` references the
# token created in the first, so they can't be split into independent
# effects. Tried via the single-effect dispatcher AFTER the compound
# splitter rejects (the splitter tries `` and `` first — there's no
# ``and`` here — and falls through to the sentence-boundary split,
# whose second piece ``at the beginning of the end step on your next
# turn, sacrifice that token`` doesn't standalone-parse, so the
# splitter rejects and the single-effect dispatcher claims the whole
# phrase).
_CREATE_LANDER_AND_SCHEDULE_SAC = re.compile(
    r"^create a Lander token\. "
    r"At the beginning of the end step on your next turn, "
    r"sacrifice that token$"
)
# ``create a token that's a copy of target creature you control``
# (Mirrorpool's mode-2 activation) — bare copy with no extra
# subtype rider. Maps to
# ``CreateTokenCopyOfTargetCreatureYouControl()``.
#
# ``create a token that's a copy of target creature you control,
# except it's a <Subtype> in addition to its other creature types``
# (Mirror Room's door trigger) — same primitive with the
# ``additional_subtype`` rider injecting the captured subtype into the
# copy's printed subtype tuple. The capture group accepts a single
# capitalized subtype word (Reflection is the surface today; broader
# multi-word subtype phrases — e.g. ``"Eldrazi Spawn"`` — would
# extend the alternation if a future printing surfaces them).
#
# A single regex with an optional rider claims both surfaces; the
# bare and ridered forms differ only by the comma-prefixed
# ``except it's a <Subtype> in addition to its other creature types``
# tail, captured optionally so the regex matches with or without it.
_CREATE_TOKEN_COPY_TARGET_CREATURE_RE = re.compile(
    r"^create a token that's a copy of target creature you control"
    r"(?:, except it's a (?P<subtype>[A-Z][A-Za-z]*) "
    r"in addition to its other creature types)?$"
)


# ``create a token that's a copy of target <permanent>, except it's
# legendary`` (Adagia, Windswept Bastion's striation): a token copy whose
# copiable values gain the Legendary supertype (CR 707.9b, 205.4a). The
# target phrase goes through the shared permanent-phrase grammar.
_CREATE_TOKEN_COPY_TARGET_LEGENDARY_RE = re.compile(
    r"^create a token that's a copy of target (?P<np>.+?), except it's "
    r"legendary$"
)


def _create_token_copy_target_legendary(np: str) -> Effect | None:
    from mtgz_ir.cards.parser.target_phrases import parse_permanent_phrase
    from mtgz_ir.effects.generic.roto_walkers import (
        CopyExceptions,
        CreateTokenCopyOfTargetWithExceptions,
    )
    from mtgz_ir.effects.targets import TargetSpec

    target_filter = parse_permanent_phrase(np)
    if target_filter is None:
        return None
    return CreateTokenCopyOfTargetWithExceptions(
        targets=(TargetSpec(filter=target_filter),),
        exceptions=CopyExceptions(add_supertypes=("Legendary",)),
    )


# "search your library for <a ... card> or <a ... card>, reveal it, put it
# into your hand, then shuffle" (Starfield Shepherd): the two card
# descriptors go through the shared card-descriptor grammar.
_SEARCH_EITHER_DESCRIPTOR_TO_HAND = re.compile(
    r"^search your library for (?P<a>an? [^,]+? card(?: with mana value \d+ "
    r"or less)?) or (?P<b>an? [^,]+? card(?: with mana value \d+ or less)?), "
    r"reveal it, put it into your hand, then shuffle$"
)


# ``draw cards equal to the sacrificed creature's power`` (Susur Secundi,
# Void Altar's striation): the power of the creature sacrificed to pay the
# ability's cost, as it last existed on the battlefield (CR 608.2h).
_DRAW_EQUAL_TO_SACRIFICED_POWER = re.compile(
    r"^draw cards equal to the sacrificed creature's power$"
)


def _parse_keyword_grant_body(body: str) -> frozenset[str] | None:
    """Parse the tail of a pump-and-gain phrase — one or more evergreen
    keywords joined by `` and `` — into the frozenset of engine
    spellings consumed by ``PumpTargetCreatureUntilEOT.granted_keywords``.

    Returns ``None`` if any piece is not a recognized standalone keyword
    so the caller can reject the whole pump-and-gain phrase rather than
    silently dropping a granted ability. Mirrors the keyword-line
    whitelist in ``parser.keywords`` (``_KEYWORD_LINES``) — adding a new
    evergreen keyword there automatically makes it grantable here.
    """
    # ``protection from X and from Y`` joins its qualities with
    # `` and from `` — re-glue those before splitting on `` and ``.
    glued = body.replace(" and from ", "\x00")
    pieces = [p.strip().replace("\x00", " and from ") for p in glued.split(" and ")]
    out: set[str] = set()
    for p in pieces:
        parsed = _parse_granted_keyword_piece(p)
        if parsed is None:
            return None
        out.update(parsed)
    return frozenset(out)


def _parse_amount(token: str) -> int:
    """Convert a Scryfall amount token (digit or spelled-out small
    cardinal) into the integer N. Assumes the token already passed a
    regex that matched either ``\\d+`` or one of
    ``_SMALL_CARDINAL_WORDS``."""
    if token.isdigit():
        return int(token)
    return _SMALL_CARDINAL_WORDS[token]


def _parse_token_count(token: str) -> int:
    """Convert a token-count word into the integer N. Adds the singular
    articles ``a`` / ``an`` (= 1) to ``_parse_amount``'s digit-or-
    spelled-out-cardinal coverage. Assumes the token already passed
    ``_TOKEN_COUNT_PATTERN``."""
    if token in ("a", "an"):
        return 1
    return _parse_amount(token)


def _parse_create_token_effect(phrase: str) -> Effect | None:
    """Try to parse ``phrase`` as a token-creation effect (either the
    stat-bearing creature shape, the bare-name shape, the
    each-player bare-name shape, or the
    copy-of-target-creature shape). Returns ``None`` if no shape
    matches OR if the resolved token name has no ``TOKEN_PRINTINGS``
    entry — the parser doesn't fabricate new token printings on
    the fly.
    """
    # Copy-of-target-creature shape (Mirrorpool / Mirror Room) — tried
    # before the stat-bearing shape because the printed surface
    # ``create a token that's a copy of target creature you control``
    # has no <P>/<T> stats and would never match the stat-bearing
    # regex anyway, but ordering more-specific before more-general is
    # the convention here. The single regex covers both the bare form
    # (Mirrorpool's mode-2: no extra subtype rider) and the
    # additional-subtype rider form (Mirror Room: ``..., except it's
    # a Reflection in addition to its other creature types``).
    m = _CREATE_TOKEN_COPY_TARGET_CREATURE_RE.match(phrase)
    if m is not None:
        return CreateTokenCopyOfTargetCreatureYouControl(
            additional_subtype=m.group("subtype"),
        )
    m = _CREATE_TOKEN_COPY_TARGET_LEGENDARY_RE.match(phrase)
    if m is not None:
        return _create_token_copy_target_legendary(m.group("np"))
    m = _CREATE_CREATURE_TOKEN.match(phrase)
    if m is not None:
        count = _parse_token_count(m.group(1))
        tapped = m.group(2) is not None
        # Stats and color word are consumed by the regex but not used
        # to look up the token — the registered ``TOKEN_PRINTINGS``
        # entry is the canonical source of those values. Mismatches
        # between oracle text and registry would be a static-vs-
        # parser parity bug caught downstream.
        subtype = m.group(6)
        if subtype not in TOKEN_PRINTINGS:
            return _synthesize_creature_token_effect(phrase)
        # The registry is keyed by subtype only; a printed token whose
        # stats differ from the registered one (a 3/1 Spirit vs. the
        # registered 1/1 Spirit) is a different token — build it from
        # its printed characteristics instead.
        registered = TOKEN_PRINTINGS[subtype]
        if (registered.power, registered.toughness) != (
            int(m.group(3)), int(m.group(4)),
        ):
            return _synthesize_creature_token_effect(phrase)
        # Nor is one of another color (CR 111.4: a token has the colors
        # its creating effect names): a registered token must carry the
        # printed color as its color indicator.
        printed_colors = frozenset(
            {_COLOR_WORD_LETTERS[m.group(5)]}
            if m.group(5) in _COLOR_WORD_LETTERS else ()
        )
        if printed_colors != registered.color_indicator:
            return _synthesize_creature_token_effect(phrase)
        token_printing = registered
        # ``that's tapped and attacking`` qualifier — token enters declared
        # as an attacker in the current combat. Only meaningful during an
        # attack trigger; the ``attacking`` flag on ``CreateToken`` sets
        # obj.attacking=True and clears summoning sickness in ``resolve``.
        attacking = "that's tapped and attacking" in phrase
        return CreateToken(
            token_printing=token_printing,
            count=count,
            tapped=tapped or attacking,  # attacking tokens are always tapped
            attacking=attacking,
        )
    # Named artifact token shape — ``create a [colors] artifact token
    # named [Name] [with abilities]`` (Weapons Manufacturing's Munitions).
    # Tried BEFORE the bare-name shape because the bare-name regex would
    # accept "create a Munitions token" but NOT the longer "artifact token
    # named …" surface. The abilities description is stripped by the regex.
    m = _CREATE_NAMED_ARTIFACT_TOKEN.match(phrase)
    if m is not None:
        count = _parse_token_count(m.group(1))
        tapped = m.group(2) is not None
        name = m.group(3)
        if name not in TOKEN_PRINTINGS:
            return None
        return CreateToken(
            token_printing=TOKEN_PRINTINGS[name],
            count=count,
            tapped=tapped,
        )
    m = _CREATE_BARE_NAME_TOKEN.match(phrase)
    if m is not None:
        count = _parse_token_count(m.group(1))
        tapped = m.group(2) is not None
        name = m.group(3)
        if name not in TOKEN_PRINTINGS:
            return None
        return CreateToken(
            token_printing=TOKEN_PRINTINGS[name],
            count=count,
            tapped=tapped,
        )
    # ``each player creates a <Name> token`` — every living player
    # creates the bare-name token under their own control. Surfaces
    # on Edge Rover's dies trigger. The captured groups mirror the
    # bare-name shape; the only difference is the leading ``each
    # player creates`` subject (instead of the controller-implicit
    # default) and the resulting ``each_player=True`` flag on the
    # produced ``CreateToken`` engine class.
    m = _EACH_PLAYER_CREATE_BARE_NAME_TOKEN.match(phrase)
    if m is not None:
        count = _parse_token_count(m.group(1))
        tapped = m.group(2) is not None
        name = m.group(3)
        if name not in TOKEN_PRINTINGS:
            return None
        return CreateToken(
            token_printing=TOKEN_PRINTINGS[name],
            count=count,
            tapped=tapped,
            each_player=True,
        )
    return _synthesize_creature_token_effect(phrase)


# Sentence-boundary split for compound effect phrases — period
# followed by whitespace and an uppercase letter, with a negative
# lookahead for ``If you do,`` so the
# ``You may pay/sacrifice/discard {COST}. If you do, …`` family
# stays welded together (the spell-builder splitter applies the same
# guard at the line level; we mirror it here at the effect-phrase
# level so triggered abilities with multi-sentence bodies — e.g.
# Dauntless Scrapbot's ``exile each opponent's graveyard. Create a
# Lander token.`` — split at the right boundary).
_EFFECT_SENTENCE_BOUNDARY = re.compile(r"(?<=\.) (?=[A-Z](?!f you do,)(?!hen you do,))")
# An "if <condition>, <effect>" clause still open at the end of a piece
# (no sentence break after the condition's comma).
_OPEN_CONDITIONAL = re.compile(r"(?:^|\. )(?:[Tt]hen )?[Ii]f [^,.]+, [^.]*$")


def _split_effect_sentences(phrase: str) -> list[str]:
    """Split a normalized effect phrase at internal sentence
    boundaries, preserving the trailing period on the leading piece(s)
    and lowercasing the first letter of each non-leading piece so each
    piece passes back through ``_parse_effect`` in normalized form.

    Returns ``[phrase]`` when no internal sentence boundary exists.
    The leading piece keeps its trailing period (so it can match
    primitives whose surface form ends mid-phrase, like Codecracker
    Hound's ``look at the top two cards of your library. Put one
    into your hand and the other into your graveyard``); we strip the
    trailing period from EVERY piece before returning so each one
    matches the normalized regex shape (no trailing period). The
    embedded period in Codecracker's surface is part of a single
    primitive's literal regex, so its phrase doesn't reach this
    splitter — it claims through the single-effect dispatcher first.

    Non-leading pieces also have a leading ``then `` connective stripped
    when present. Scryfall prints sentence-joined sequenced effects
    with ``Then <effect>.`` as the second sentence (Extinguisher
    Battleship's ETB body — ``destroy target noncreature permanent.
    Then this Spacecraft deals 4 damage to each creature.``); the
    ``Then`` is purely a sequencing connective with the same semantic
    weight as the ``, then `` joiner the compound parser accepts on
    comma-joined surfaces, so stripping it here lands the inner
    sentence on the same single-effect grammar without further
    branching.
    """
    pieces = _EFFECT_SENTENCE_BOUNDARY.split(phrase)
    if len(pieces) == 1:
        return pieces
    out: list[str] = []
    for i, piece in enumerate(pieces):
        piece = piece.strip()
        if not piece:
            continue
        if piece.endswith("."):
            piece = piece[:-1]
        if i > 0 and piece and piece[0].isupper():
            piece = piece[0].lower() + piece[1:]
        if i > 0 and piece.startswith("then "):
            piece = piece[len("then "):]
        out.append(piece)
    return out


def _parse_compound_effect(phrase: str) -> Effect | None:
    """Try to parse ``phrase`` as a compound effect: two or more
    primitive effects joined by `` and ``, by ``, then ``, or by
    sentence boundary (``. ``).

    Splits on whichever joiner appears (`` and `` is preferred — see
    below), parses each piece via ``_parse_effect``, and returns a
    ``Sequence`` only if EVERY piece parses cleanly. Returns ``None``
    if the phrase contains no recognized joiner, or if any piece
    fails to parse — never falls back to single-effect parsing on the
    original (the caller does that).

    The strict all-or-nothing rule keeps us from accidentally
    consuming joiner-bearing single effects (e.g. the
    ``search your library for an artifact card, reveal it, put it into
    your hand, then shuffle`` artifact-tutor primitive whose ``, then
    `` is part of one effect's surface): if a split-and-parse attempt
    fails on any piece, we punt and let the caller try the single-
    effect grammar on the unsplit phrase. The artifact-tutor primitive
    in particular survives because its ``shuffle`` half doesn't parse
    as a standalone effect, so the ``, then `` split rejects and the
    full-phrase regex wins.

    Joiner precedence: `` and `` is tried first; ``, then `` second;
    sentence-boundary (``. ``) last. The order matters because the
    joiners can co-occur (a future ``draw a card and you gain 1 life,
    then discard a card`` would have both ``and`` and ``, then``) and
    splitting on the leftmost-listed joiner preserves the same
    left-to-right resolve order the engine expects from ``Sequence``
    regardless of which joiner the printing chose. The sentence-
    boundary split is last so single-effect primitives whose regex
    contains an internal period (Codecracker Hound's two-sentence
    look-at-top-two phrase) still match through the single-effect
    dispatcher first; only when that fails AND no other joiner
    applies do we carve at sentence boundaries.

    A compound where two or more sub-effects each require a target is
    also rejected (returns ``None``) — ``Sequence`` only supports a
    single targeting sub-effect (the cast-time target-selection
    pipeline is single-choice). A multi-target spell needs a
    different combinator and isn't claimed by this slice.
    """
    # ``you may X and Y`` -- the optional scopes over the whole
    # conjunction, so it must not be carved into ``you may X`` + ``Y``
    # (which would make ``Y`` mandatory). The whole phrase falls through
    # to the ``you may <effect>`` recognizer instead.
    if phrase.startswith("you may ") and " and " in phrase:
        return None
    and_pieces = phrase.split(" and ") if " and " in phrase else []
    if any(_OPEN_CONDITIONAL.search(p) for p in and_pieces[:-1]):
        # "if <condition>, A and B" -- the condition scopes over both
        # halves; carving at " and " would make B unconditional (Cecil,
        # Dark Knight's "... Then if <condition>, untap Cecil and
        # transform it."). Try the other joiners instead.
        and_pieces = []
    if and_pieces:
        eff = _parse_effect_pieces(and_pieces)
        if eff is not None or ", then " not in phrase:
            return eff
        # "X, then Y and Z" (Kavaron, Memorial World's striation: "create
        # a ... token, then creatures you control get +1/+0 and gain haste
        # until end of turn"): the " and " belongs to the second piece.
        pieces = phrase.split(", then ")
    elif ", then " in phrase:
        pieces = phrase.split(", then ")
    elif _EFFECT_SENTENCE_BOUNDARY.search(phrase):
        pieces = _split_effect_sentences(phrase)
        if len(pieces) < 2:
            return None
    else:
        return None
    return _parse_effect_pieces(pieces)


def _parse_effect_pieces(pieces: list[str]) -> Effect | None:
    """Parse every piece of a split compound phrase; ``Sequence`` of the
    results, or ``None`` if any piece fails or more than one targets."""
    parsed: list[Effect] = []
    for piece in pieces:
        eff = _parse_effect(piece)
        if eff is None:
            return None
        parsed.append(eff)
    if sum(1 for e in parsed if e.requires_target()) > 1:
        return None
    return Sequence(effects=tuple(parsed))


# "<body>. If there are no <kind> counters on this land, sacrifice it" —
# a trailing self-sacrifice rider keyed on a ``named_counters`` kind
# (Gemstone Mine's mana ability). The body parses through the unified
# grammar; the rider maps to ``SacrificeSelfIfNoNamedCounters(kind)``
# and the pair to a ``Sequence``.
_IF_NO_NAMED_COUNTERS_SACRIFICE_RIDER = re.compile(
    r"^(?P<body>.+)\. If there are no (?P<kind>[a-z]+) counters on this "
    r"(?:land|artifact|creature|enchantment|permanent), sacrifice it$"
)


def _parse_effect(phrase: str) -> Effect | None:
    """Parse a *normalized* effect phrase into an ``Effect``.

    Layers the compositional grammar in
    :mod:`mtgz_ir.cards.parser.precise_grammar` around the legacy
    per-shape grammar (``_parse_effect_legacy``): a faithful override
    for the few surfaces the legacy grammar accepts lossily, then the
    legacy grammar, then the compositional fallback for phrases the
    legacy grammar rejects.
    """
    from mtgz_ir.cards.parser.precise_grammar import (
        parse_precise_fallback,
        parse_precise_override,
    )

    eff = parse_precise_override(phrase)
    if eff is not None:
        return eff
    eff = _parse_effect_legacy(phrase)
    if eff is not None:
        return eff
    eff = parse_precise_fallback(phrase)
    if eff is not None:
        return eff
    # Cube W/G creature bucket shapes -- the very last fallback, after
    # the compositional grammar, so the base's shapes win.
    from mtgz_ir.cards.parser.creature_wg_effects import parse_wg_effect

    eff = parse_wg_effect(phrase)
    if eff is not None:
        return eff
    # Cube layouts bucket (transform / Saga / Class / leveler shapes) --
    # after every other grammar, so it only claims unparsed phrases.
    from mtgz_ir.cards.parser.layout_grammar import parse_layout_effect

    eff = parse_layout_effect(phrase)
    if eff is not None:
        return eff
    # Cube glue bucket shapes -- after every other bucket.
    from mtgz_ir.cards.parser.cube_glue_grammar import parse_glue_effect

    eff = parse_glue_effect(phrase)
    if eff is not None:
        return eff
    # Roto P1 (stack & restrictions) shapes -- after every cube bucket.
    from mtgz_ir.cards.parser.roto_stack import parse_roto_stack_effect

    eff = parse_roto_stack_effect(phrase)
    if eff is not None:
        return eff
    # Vintage Roto P3 (mana, costs, cast permissions).
    from mtgz_ir.cards.parser.roto_mana import parse_roto_mana_effect

    eff = parse_roto_mana_effect(phrase)
    if eff is not None:
        return eff
    # Vintage Roto P5 (board-state prison & layers).
    from mtgz_ir.cards.parser.roto_board import parse_roto_board_effect

    eff = parse_roto_board_effect(phrase)
    if eff is not None:
        return eff
    # Roto P2 (library, graveyard & hand) shapes.
    from mtgz_ir.cards.parser.roto_zones import parse_roto_zones_effect

    eff = parse_roto_zones_effect(phrase)
    if eff is not None:
        return eff
    # Roto P4 shapes (walkers, copies, control, turns).
    from mtgz_ir.cards.parser.roto_walkers import parse_roto_effect

    eff = parse_roto_effect(phrase)
    if eff is not None:
        return eff
    # Roto P6 (keyword mechanics, face-changing & pregame).
    from mtgz_ir.cards.parser.roto_keywords import parse_p6_effect

    eff = parse_p6_effect(phrase)
    if eff is not None:
        return eff
    # Card types and colors (CR 613 layers 4 / 5): Liquimetal Coating,
    # Karn, the Great Creator.
    from mtgz_ir.cards.parser.roto_layers import parse_roto_layer_effect

    eff = parse_roto_layer_effect(phrase)
    if eff is not None:
        return eff
    # The legacy pool, the very last fallbacks, after every other grammar,
    # so no phrase they claim changes. mtg-zero's Legacy Doomsday deck: lane
    # A's "at the beginning of your next upkeep, <effect>", "pay <cost>. If
    # you don't, <effect>" (Pact of Negation); lane B's "change the target
    # of target spell with a single target" (Misdirection). mtg-zero's Legacy
    # Sneak and Show deck: "deals N damage to each creature and each
    # planeswalker" (Brotherhood's End). The Delver mirror's sideboard:
    # "deals N damage to each creature with / without <keyword>" (Rough //
    # Tumble). Their phrases are disjoint.
    from mtgz_ir.cards.parser.legacy_delver_sb import parse_legacy_delver_sb_effect
    from mtgz_ir.cards.parser.legacy_sneak import parse_legacy_sneak_effect
    from mtgz_ir.cards.parser.legacy_spells import parse_legacy_spell_effect
    from mtgz_ir.cards.parser.legacy_stack import parse_legacy_stack_effect

    eff = parse_legacy_spell_effect(phrase)
    if eff is not None:
        return eff
    eff = parse_legacy_stack_effect(phrase)
    if eff is not None:
        return eff
    eff = parse_legacy_sneak_effect(phrase)
    if eff is not None:
        return eff
    return parse_legacy_delver_sb_effect(phrase)


def _parse_effect_legacy(phrase: str) -> Effect | None:
    """Parse a *normalized* effect phrase into an ``Effect``.

    Normalized form has no source-subject prefix, no trailing period,
    and a lowercased first letter — see ``_normalize_*`` helpers below
    for the per-context surface stripping.

    Compound effects (two or more primitives joined by `` and ``) are
    tried first via ``_parse_compound_effect``; if every split-piece
    parses, returns a ``Sequence``. If the compound attempt fails
    (any piece doesn't parse), falls through to the single-effect
    grammar on the original phrase — preserving the ability of a
    single-effect regex to match a phrase that happens to contain
    `` and ``.
    """
    # Literal multi-sentence effect recognizers — tried BEFORE the
    # single-sentence regex grammar because these phrases span two
    # printed sentences (an internal period + uppercase continuation)
    # that no single-sentence regex would tolerate. Each entry maps
    # verbatim to a bespoke engine class; generalizing the shape would
    # require carving the two sentences apart and recognizing an
    # anaphoric continuation clause (``put that card …`` referring to
    # the land revealed in the prior sentence), which the current
    # sentence-splitting grammar can't model. A second card with the
    # same shape (reveal-until-condition + put-result-elsewhere) would
    # call for a parameterized recognizer at that point.
    #
    # Weftwalking's ETB body composed (after the intervening-if gate
    # ``if you cast it`` is split off by the trigger-line regex):
    # ``shuffle your hand and graveyard into your library, then draw
    # seven cards``. The compound parser's joiner-precedence (`` and ``
    # before ``, then ``) would carve this incorrectly because the
    # inner `` and `` belongs to the zone-list of the shuffle primitive
    # ("your hand and graveyard"), not to a compound effect joiner.
    # Matched here as a single literal so the whole body dispatches in
    # one step to a ``Sequence`` of two generic primitives —
    # ``ShuffleZonesIntoLibrary(zones=("hand", "graveyard"))`` and
    # ``DrawCards(amount=7)``. A future card with a varying piece
    # (different zone list, different draw count) would land its own
    # literal here; broader grammar (e.g. teaching the compound parser
    # to recognize ``shuffle your <zone-list> into your library`` as
    # an atomic primitive whose internal `` and `` is bracketed)
    # deferred until a second example exists.
    m = _IF_NO_NAMED_COUNTERS_SACRIFICE_RIDER.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic import SacrificeSelfIfNoNamedCounters

        body = _parse_effect(m.group("body"))
        if body is None:
            return None
        return Sequence(effects=(
            body, SacrificeSelfIfNoNamedCounters(kind=m.group("kind")),
        ))
    if phrase == (
        "shuffle your hand and graveyard into your library, "
        "then draw seven cards"
    ):
        return Sequence(effects=(
            ShuffleZonesIntoLibrary(zones=("hand", "graveyard")),
            DrawCards(amount=7),
        ))
    # House Cartographer's Survival body (only card in the pool with
    # the "reveal until land" shape; broader grammar deferred until a
    # second example exists).
    # The trigger-line regex strips the outer trailing period before
    # dispatching the effect body; the multi-sentence inner period is
    # preserved, so the phrase here has no trailing period but the
    # internal ". Put…" sentence boundary is intact.
    if phrase == (
        "reveal cards from the top of your library until you reveal a land card. "
        "Put that card into your hand and the rest on the bottom of your library "
        "in a random order"
    ):
        return HouseCartographerSurvivalEffect()
    # Arabella, Abandoned Doll's attack-trigger body: "deals X damage to
    # each opponent and you gain X life, where X is the number of
    # creatures you control with power 2 or less". Literal-line recognizer
    # — only card in the pool with this X-scaled damage+life-gain where X
    # is a power-filtered creature count; broader grammar deferred until a
    # second example exists. Maps to ``ArabellaAttackEffect`` which
    # computes X at resolve time via ``effective_power`` so pump and
    # counter-modifications are reflected. The normalizer strips the
    # leading ``it `` before this lookup runs, matching Arabella's
    # creature-trigger surface.
    if phrase == (
        "deals X damage to each opponent and you gain X life, "
        "where X is the number of creatures you control with power 2 or less"
    ):
        return ArabellaAttackEffect()
    # Anticausal Vestige's LTB body (only EoE card with the
    # "draw + may put permanent MV ≤ lands from hand onto BF tapped"
    # shape; broader grammar deferred until a second example exists).
    # The MV gate is dynamic (evaluated at resolve time against the
    # controller's current land count), which the single-effect grammar
    # can't express as a parameterized recognizer today.
    if phrase == (
        "draw a card, then you may put a permanent card with mana value "
        "less than or equal to the number of lands you control from your "
        "hand onto the battlefield tapped"
    ):
        return AnticausalVestigeLeavesEffect()
    # Astelli Reclaimer's ETB body (only EoE card with the "return
    # noncreature/nonland from GY with MV ≤ mana-spent" shape; broader
    # grammar deferred until a second example exists). The mana-spent
    # gate is dynamic (evaluated against the warp cost or full cost paid
    # at cast time), which the single-effect grammar can't model.
    if phrase == (
        "return target noncreature, nonland permanent card with mana "
        "value X or less from your graveyard to the battlefield, where "
        "X is the amount of mana spent to cast this creature"
    ):
        return AstelliReclaimerETBEffect()
    # Territorial Bruntar's Landfall body (only EoE card with the
    # "exile top until nonland + may cast that card this turn" shape;
    # broader grammar deferred until a second example exists). The
    # anaphoric "that card" in sentence 2 refers to the nonland card
    # exiled in sentence 1, which the sentence-splitting grammar can't
    # model as a standalone clause.
    if phrase == (
        "exile cards from the top of your library until you exile a "
        "nonland card. You may cast that card this turn"
    ):
        return TerritorialBruntarLandfallEffect()
    # Memorial Vault's activation body — "exile the top X cards of
    # your library, where X is one plus the mana value of the
    # sacrificed artifact. You may play those cards this turn."
    # Decomposes cleanly into the generic
    # ``ExileTopXOfLibraryMayPlayThisTurn`` primitive driven by the
    # ``MVOfCostPaidObjectPlus(constant=1)`` formula. The formula reads
    # ``state.resolving_cost_paid_objects[0]`` at resolve time — the
    # ``Sacrifice another artifact`` cost atom populates the tuple via
    # the cost-payment plumbing in ``_activate_ability``. Multi-
    # sentence literal recognizer because the body crosses an internal
    # period (the comma + "where X is …" fragment + ". You may play
    # …" continuation) that no single-sentence regex would tolerate;
    # broader grammar (parameterized "exile the top X cards of your
    # library, where X is <formula>; you may play those cards this
    # turn") deferred until a second card with a varying formula
    # ships. The recognizer maps any future card with this exact
    # printed body to the same composition, so a re-print would claim
    # automatically.
    if phrase == (
        "exile the top X cards of your library, where X is one plus "
        "the mana value of the sacrificed artifact. You may play "
        "those cards this turn"
    ):
        return ExileTopXOfLibraryMayPlayThisTurn(
            amount=MVOfCostPaidObjectPlus(constant=1),
        )
    # Charred Foyer's upkeep trigger body — single-card impulse-cast
    # shape ``exile the top card of your library. You may play it
    # this turn``. Maps to ``ExileTopOfLibraryMayPlayThisTurn(
    # amount=1)`` whose resolve sets ``playable_from_exile_this_turn``
    # on the exiled card (cleared at cleanup of the casting turn via
    # ``turn.run_cleanup``). Multi-sentence literal recognizer because
    # the body crosses an internal period (the dot between sentence 1
    # and ``You may play it this turn``) — no single-sentence regex
    # tolerates that, and the compound splitter would carve the second
    # sentence ``you may play it this turn`` which has no standalone
    # primitive. Only Charred Foyer in the pool today fits this exact
    # surface; broader grammar (a parameterized ``exile the top
    # <count> cards of your library. You may play <pronoun> this
    # turn`` regex driving ``ExileTopOfLibraryMayPlayThisTurn(amount=
    # <count>)``) deferred until a second printing with a varying
    # count or pronoun ships.
    if phrase == (
        "exile the top card of your library. "
        "You may play it this turn"
    ):
        return ExileTopOfLibraryMayPlayThisTurn(amount=1)
    # Unholy Annex's end-step trigger body — three-sentence body
    # composed of an unconditional draw, a Demon-or-not gate, and the
    # alternate "otherwise" branch. Maps to
    # ``Sequence(DrawCards(1), ConditionalIfYouControlSubtype(
    # subtype="Demon",
    # then_effect=Sequence(EachOpponentLosesLife(2), GainLife(2)),
    # else_effect=LoseLife(2)))``. The
    # ``ConditionalIfYouControlSubtype`` combinator reads
    # ``effective_subtypes`` so a creature granted ``Demon`` by a
    # Cytoshape / Conspiracy effect counts the same as a printed Demon
    # (matches every other "if you control a <subtype>" gate in this
    # pool). Multi-sentence literal recognizer because the body crosses
    # two internal periods and the ``Otherwise`` continuation is an
    # anaphoric else-arm referencing the prior gate — the sentence-
    # splitter can't model that as a standalone clause. Only Unholy
    # Annex in the pool today fits this exact "draw + subtype-gate +
    # else" surface; broader grammar (a parameterized recognizer over
    # <unconditional-effect>. If you control a <subtype>, <then>.
    # Otherwise, <else>) deferred until a second example exists, at
    # which point the regex's capture groups would feed the same
    # ``Sequence`` + ``ConditionalIfYouControlSubtype`` composition
    # automatically.
    if phrase == (
        "draw a card. If you control a Demon, "
        "each opponent loses 2 life and you gain 2 life. "
        "Otherwise, you lose 2 life"
    ):
        return Sequence(effects=(
            DrawCards(amount=1),
            ConditionalIfYouControlSubtype(
                subtype="Demon",
                then_effect=Sequence(effects=(
                    EachOpponentLosesLife(amount=2),
                    GainLife(amount=2),
                )),
                else_effect=LoseLife(amount=2),
            ),
        ))
    # Monoist Circuit-Feeder's ETB body — paired-target opposite-polarity
    # pump/debuff with a shared X. The compound-effect splitter can't
    # carve this body cleanly: ``Sequence`` supports only one targeting
    # sub-effect (the cast-time picker is single-prompt) and the two
    # ``and ``-joined halves each prompt for their own creature.
    # Literal-line recognizer mapped to the generic
    # ``PairedTargetPumpDebuffUEOT`` primitive driven by a
    # ``CountControlledMatching(IsArtifact())`` formula. Only card in
    # the pool with this paired-target +X/-X shape today; broader
    # grammar (parameterized "until end of turn, target creature you
    # control gets <delta> and target creature an opponent controls
    # gets <delta>, where X is <formula>") deferred until a second
    # example exists. When that ships, the recognizer becomes a regex
    # whose capture groups feed ``PairedTargetPumpDebuffUEOT``'s
    # constructor — the engine primitive is already parameterized for
    # any pump/debuff polarity and any ``Formula``.
    if phrase == (
        "until end of turn, target creature you control gets +X/+0 "
        "and target creature an opponent controls gets -0/-X, where "
        "X is the number of artifacts you control"
    ):
        return PairedTargetPumpDebuffUEOT(
            formula=CountControlledMatching(IsArtifact()),
            positive_power=1, positive_toughness=0,
            negative_power=0, negative_toughness=-1,
        )
    # Chorale of the Void's attacks-trigger body — reanimate from a
    # defending player's graveyard tapped + attacking. The body is a
    # single printed sentence but the legal-target pool is bound to
    # "defending player" (not a generic graveyard reanimate filter)
    # and the resolved card enters with two combat-state riders
    # simultaneously (tapped + attacking + not summoning-sick), which
    # the standard single-sentence reanimate grammar doesn't model.
    # Literal-line recognizer mapped to the generic
    # ``ReanimateTargetCreatureCardFromDefendersGraveyard`` primitive
    # — only card in the pool with this defender-graveyard reanimate
    # shape today; broader grammar (parameterized "put target
    # creature card from <player-spec> graveyard onto the battlefield
    # under <controller-spec> tapped and attacking") deferred until a
    # second example exists. The 2-player simplification is documented
    # on the engine primitive: the legal-target pool today is "any
    # opponent's graveyard creature card", which matches the printed
    # text exactly in 2-player mode.
    if phrase == (
        "put target creature card from defending player's graveyard "
        "onto the battlefield under your control tapped and attacking"
    ):
        return ReanimateTargetCreatureCardFromDefendersGraveyard()
    # Perigee Beckoner's ETB body (only EoE card with the "pump another
    # target creature you control + grant dies-return-tapped trigger"
    # shape; broader grammar deferred until a second example exists).
    # The quoted granted trigger ("When this creature dies, return it to
    # the battlefield tapped under its owner's control") is an anaphoric
    # sub-clause the inline keyword-grant grammar can't model without a
    # full nested trigger parser. The literal maps directly to the
    # bespoke's PumpTargetCreatureUntilEOT(granted_triggered_ability=...)
    # composition.
    if phrase == (
        "until end of turn, another target creature you control gets "
        "+2/+0 and gains \"When this creature dies, return it to the "
        "battlefield tapped under its owner's control"
    ):
        return PumpTargetCreatureUntilEOT(
            power_bonus=2, toughness_bonus=0,
            you_control_only=True,
            exclude_self=True,
            granted_triggered_ability=TriggeredAbility(
                event=Event.DIES, predicate=IsSelf(),
                effect=PerigeeBeckonerDiesReturnTappedEffect(),
            ),
        )
    # Possibility Technician's ENTERS body (only EoE card with the
    # "exile top card + may play it while controlling a Kavu" shape;
    # broader grammar deferred until a second example exists). The
    # "for as long as that card remains exiled" duration and the
    # "if you control a Kavu" play-condition are anaphoric references
    # that the sentence-splitting grammar can't model as standalone
    # clauses.
    if phrase == (
        "exile the top card of your library. "
        "For as long as that card remains exiled, you may play it "
        "if you control a Kavu"
    ):
        return PossibilityTechnicianETBEffect()
    # Lightstall Inquisitor's ETB body — three-sentence body whose
    # second and third sentences are anaphoric "this way" qualifiers on
    # the cards exiled in sentence 1 (each opponent's exile target).
    # The {1} surcharge on cast spells and the tapped-entry on lands are
    # enforced by the engine-side ``cast_from_exile_with_lightstall_
    # surcharge`` flag the effect class sets; the parser's job here is
    # just to dispatch the printed body to the existing engine primitive.
    # Currently only Lightstall Inquisitor in the EoE pool prints this
    # exact "exile from hand with conditional-cost play permission" three-
    # sentence shape; broader grammar deferred until a second card with
    # this shape ships (a parameterized recognizer would capture the
    # surcharge amount and the tapped-entry rider as separate fields).
    if phrase == (
        "each opponent exiles a card from their hand and may play that "
        "card for as long as it remains exiled. Each spell cast this "
        "way costs {1} more to cast. Each land played this way enters "
        "tapped"
    ):
        return EachOpponentExilesCardFromHandWithCastPermission()
    # Splitskin Doll's ETB body — two sentences: an unconditional draw
    # followed by a "Then discard a card unless you control another
    # creature with power 2 or less" rider. Decomposes into a generic
    # Sequence of two primitives:
    #   - DrawCards(amount=1)
    #   - IfNot(condition=ControlsAnotherCreatureWithPowerAtMost(2),
    #           then=DiscardSelf(amount=1))
    # The "another" excludes ``state.resolving_source`` (the Doll itself,
    # printed 2/1) so the printed surface is honored. Tried BEFORE
    # ``_parse_compound_effect`` because the second sentence's
    # ``discard a card unless you control another creature ...`` shape
    # has no standalone single-effect recognizer in the grammar today —
    # the sentence-splitter would carve at the ``. Then`` boundary and
    # fail to parse the second piece. Only Splitskin Doll in the DSK/EoE
    # pool prints this exact two-sentence "draw + conditional discard
    # gated on another small creature" surface; broader grammar deferred
    # until a second card ships with this shape, at which point the
    # right move is a parameterized recognizer for the bare
    # ``discard a card unless <condition>`` second sentence so the
    # compound splitter claims it without a literal here.
    if phrase == (
        "draw a card. Then discard a card unless you control "
        "another creature with power 2 or less"
    ):
        return Sequence((
            DrawCards(amount=1),
            IfNot(
                condition=ControlsAnotherCreatureWithPowerAtMost(
                    threshold=2,
                ),
                then=DiscardSelf(amount=1),
            ),
        ))
    # "search your library for <a ... card> or <a ... card>, reveal it,
    # put it into your hand, then shuffle" (Starfield Shepherd: "a basic
    # Plains card or a creature card with mana value 1 or less"): ONE
    # search, the player picking a card of either kind as they search
    # (CR 701.19), not a mode chosen beforehand.
    m = _SEARCH_EITHER_DESCRIPTOR_TO_HAND.match(phrase)
    if m is not None:
        from mtgz_ir.cards.parser.card_flow_grammar import parse_card_descriptor
        from mtgz_ir.effects.generic.card_flow import CardFilterAnyOf, SearchLibrary

        first = parse_card_descriptor(m.group("a"))
        second = parse_card_descriptor(m.group("b"))
        if first is not None and second is not None:
            return SearchLibrary(
                card_filter=CardFilterAnyOf(filters=(first, second)),
                destination="hand", reveal=True,
            )
    # Fanatic of the Harrowing's ETB body (only DSK card with the
    # "each player discards + reflexive draw if you did" shape; broader
    # grammar deferred until a second example exists). The reflexive
    # "If you discarded a card this way" gate is an anaphoric reference
    # to the discard step that the sentence-splitting grammar can't
    # model as a standalone clause.
    if phrase == (
        "each player discards a card. "
        "If you discarded a card this way, draw a card"
    ):
        return FanaticOfTheHarrowingETBEffect()
    # Boilerbilges Ripper's ETB body (only DSK card with the
    # "optional sac creature/enchantment + reflexive 2-damage to any
    # target" shape; broader grammar deferred until a second example
    # exists). The "When you do" reflexive gate is an anaphoric reference
    # to the sacrifice step that the sentence-splitting grammar can't
    # model as a standalone clause.
    if phrase == (
        "you may sacrifice another creature or enchantment. "
        "When you do, this creature deals 2 damage to any target"
    ):
        return BoilerbilgesRipperETBEffect()
    # Famished Worldsire's ETB body (only EoE card with the
    # "look at top X cards where X is this creature's power; put land
    # cards onto battlefield tapped; shuffle" shape; broader grammar
    # deferred until a second example exists). The ``where X is this
    # creature's power`` clause and the ``put any number of land cards
    # from among them`` "any number" selection are anaphoric references
    # that the sentence-splitting grammar can't model as standalone
    # clauses; the entire two-sentence surface is matched as a single
    # literal so the engine's ``FamishedWorldsireETBEffect`` is wired
    # in one place.
    if phrase == (
        "look at the top X cards of your library, where X is this "
        "creature's power. Put any number of land cards from among "
        "them onto the battlefield tapped, then shuffle"
    ):
        return FamishedWorldsireETBEffect()
    # Terrapact Intimidator's ETB body (only EoE card with the
    # "opponent may have you create tokens; if not, self-counters"
    # shape; broader grammar deferred until a second example exists).
    # The "If they don't" reflexive gate is an anaphoric reference to
    # the opponent's choice that the sentence-splitting grammar can't
    # model as a standalone clause.
    if phrase == (
        "target opponent may have you create two Lander tokens. "
        "If they don't, put two +1/+1 counters on this creature"
    ):
        return TerrapactIntimidatorETBEffect()
    # Survey Mechan's activated-ability body — two sentences with two
    # independent targets (any target for damage; target player for draw
    # + gain life). The Sequence combinator can't model this because it
    # enforces at most one targeting sub-effect across all sub-effects.
    # Matched as a single literal dispatching to the bespoke
    # ``SurveyMechanActivationEffect`` engine class, which handles both
    # target slots in its own ``target_count_range``/``legal_targets``/
    # ``resolve`` overrides. Only Survey Mechan in the EoE pool has this
    # exact two-target "deal 3 + target player draws 3 + gains 3" shape;
    # broader grammar deferred until a second card with a varying amount
    # (different damage count, different draw count) ships.
    if phrase == (
        "it deals 3 damage to any target. "
        "Target player draws three cards and gains 3 life"
    ):
        from mtgz_ir.effects.bespoke import SurveyMechanActivationEffect
        return SurveyMechanActivationEffect()
    # Entropic Battlecruiser's attack-trigger body — a two-sentence
    # multi-sentence effect whose second sentence is anaphoric to the
    # first ("Each opponent who can't" refers to those who couldn't
    # discard in sentence 1). The sentence-splitting compound parser
    # would carve at the ``. `` boundary and try to parse
    # "Each opponent who can't loses 3 life" as a standalone effect —
    # which fails because "who can't" is an anaphoric qualifier. Matched
    # as a single literal here so the two sentences stay coupled.
    # The phrase arrives here in raw normalized form (no leading pronoun
    # stripped; the trigger regex extracts everything after the comma
    # verbatim, preserving the internal sentence boundary and its
    # capital-letter continuation). Currently only Entropic Battlecruiser
    # in the EoE pool fits this exact surface; broader grammar deferred
    # until a second card with a varying piece (different life amount or
    # different discard count) ships. At that point, replace this literal
    # with a parameterized regex capturing the life amount.
    if phrase == (
        "each opponent discards a card. "
        "Each opponent who can't loses 3 life"
    ):
        return EachOpponentDiscardsOrLosesLife(life_if_cant=3)
    # Dyadrine, Synthesis Amalgam's YOU_ATTACK trigger body. The phrase
    # spans two printed sentences separated by a period (the "If you do"
    # sentence is anaphoric to the optional counter-removal clause in the
    # first). The sentence-splitting grammar would carve at the ``. If``
    # boundary and try to parse each piece independently, but "if you
    # do, draw a card and create a 2/2 colorless Robot artifact creature
    # token" is an anaphoric reflexive that doesn't standalone-parse.
    # Matched as a single literal so both sentences stay coupled in
    # ``DyadrineAttackEffect``. Only Dyadrine in the pool has this exact
    # surface; deferred until a second card with this shape appears.
    if phrase == (
        "you may remove a +1/+1 counter from each of two creatures you "
        "control. If you do, draw a card and create a 2/2 colorless Robot "
        "artifact creature token"
    ):
        return DyadrineAttackEffect()
    # Alpharael, Dreaming Acolyte's discard tail from the ETB sentence
    # split. After ``_split_effect_sentences`` carves "draw two cards"
    # off the front, this piece lands here. It is a "do X unless Y"
    # choice (discard two vs. discard one artifact) modeled by
    # ``AlpharaelDreamingAcolyteDiscardEffect``. Only Alpharael in the
    # pool has this exact surface; deferred until a second card with a
    # similar "discard N unless you discard a <type>" shape appears.
    if phrase == (
        "discard two cards unless you discard an artifact card"
    ):
        return AlpharaelDreamingAcolyteDiscardEffect()
    # Tannuk, Memorial Ensign's landfall trigger body. The phrase spans
    # two printed sentences (damage + conditional draw on the second
    # resolution); the second sentence is anaphoric to the per-source
    # resolution count tracked in ``state.ability_resolutions_this_turn``.
    # The sentence-splitting grammar would carve at the ``. If`` boundary
    # but "if this is the second time this ability has resolved this turn,
    # draw a card" doesn't standalone-parse (no grammar primitive for
    # per-source resolution counting). Matched as a literal so both
    # sentences stay coupled in ``TannukEnsignLandfallEffect``. Only
    # Tannuk, Memorial Ensign in the pool has this exact surface; deferred
    # until a second card with a "Nth-resolution-this-turn" conditional
    # appears.
    if phrase == (
        "this creature deals 1 damage to each opponent. "
        "If this is the second time this ability has resolved this turn, "
        "draw a card"
    ):
        return TannukEnsignLandfallEffect()
    # Biosynthic Burst's three-sentence body — counter + grant-three-keywords
    # + untap on the same target creature you control. The anaphoric "It"/"it"
    # in sentences 2 and 3 refers back to the creature targeted in sentence 1,
    # so the three sentences must be kept together; the sentence-splitting
    # grammar can't model them as independent clauses. Matched here as a
    # single literal so the whole body dispatches to the bespoke engine class
    # in one step. Only Biosynthic Burst in the EoE pool fits this exact
    # three-sentence shape; broader grammar deferred until a second example
    # exists (e.g. a card with different keywords or a different counter type).
    if phrase == (
        "put a +1/+1 counter on target creature you control. "
        "It gains reach, trample, and indestructible until end of turn. "
        "Untap it"
    ):
        return BiosynthicBurstEffect()
    # Roving Actuator's Void ETB body — a three-sentence chain:
    # "Exile up to one target instant or sorcery card with mana value 2
    # or less from your graveyard. Copy it. You may cast the copy without
    # paying its mana cost." The second and third sentences are anaphoric
    # to the card exiled in sentence 1 ("it" and "the copy"), so the three
    # sentences must be kept together; the sentence-splitting grammar can't
    # model them as independent clauses. Matched here as a single literal
    # so the whole body dispatches to ``RovingActuatorETBEffect``. Only
    # Roving Actuator in the EoE pool has this exact three-sentence exile-
    # from-graveyard + copy + cast-free shape; broader grammar deferred
    # until a second example exists (e.g. a card with a different MV cap
    # or a different target type).
    if phrase == (
        "exile up to one target instant or sorcery card with mana value "
        "2 or less from your graveyard. Copy it. You may cast the copy "
        "without paying its mana cost"
    ):
        return RovingActuatorETBEffect(max_mv=2)
    # Vengeful Possession's four-sentence body — the Threaten core (gain
    # control + untap + haste UEOT on the same target creature) plus a
    # trailing optional-looter rider ("You may discard a card. If you do,
    # draw a card."). The anaphoric "it" in sentences 2–3 refers to the
    # target of sentence 1; the optional-looter sentence-pair stays joined
    # by the sentence-splitter's "If you do," negative lookahead. Matched
    # as a single literal so the parser can hand the body to a Sequence
    # of two generic primitives — the Threaten core
    # (``ThreatenTargetCreatureUEOT()``) and the looter rider
    # (``IfYouDo(MayDo(DiscardSelf(amount=1)), DrawCards(amount=1))``)
    # — no per-card class needed. Future cards that print the same
    # Threaten core or the same looter rider get them for free.
    if phrase == (
        "gain control of target creature until end of turn. "
        "Untap it. It gains haste until end of turn. "
        "You may discard a card. If you do, draw a card"
    ):
        return Sequence((
            ThreatenTargetCreatureUEOT(),
            IfYouDo(
                do_action=MayDo(DiscardSelf(amount=1)),
                then_action=DrawCards(amount=1),
            ),
        ))
    # Attack-in-the-Box's attack-trigger body — the optional-pump-then-
    # schedule-sac compound: "you may have it get +4/+0 until end of
    # turn. If you do, sacrifice it at the beginning of the next end
    # step." The "If you do" reflexive in sentence 2 is anaphoric to the
    # optional pump in sentence 1, so the two sentences must stay
    # coupled; the sentence-splitting grammar would carve at the ``. ``
    # boundary and try to parse "if you do, sacrifice it at the
    # beginning of the next end step" as a standalone clause, which
    # has no standalone meaning. Matched as a single literal so the
    # parser can compose the body from generic primitives — no per-card
    # class needed. The capturing structure is
    # ``IfYouDo(MayDo(PumpSelfUntilEOT(...)),
    # ScheduleSelfSacAtNextEndStep())``: ``MayDo`` gates the pump on
    # the controller's ``choose_yes_no``, ``IfYouDo`` fires the sac
    # schedule only when the pump landed, and
    # ``ScheduleSelfSacAtNextEndStep`` schedules a delayed
    # ``SacrificeSourceDelayed`` keyed at the current turn (matching
    # Devastating Onslaught's "next end step" wording). Only Attack-in-
    # the-Box in the EoE/DSK pool has this exact "optional-pump-then-
    # sac-next-end-step" shape; broader grammar deferred until a second
    # example exists (e.g. a card with a different P/T or a different
    # delayed consequence).
    if phrase == (
        "you may have it get +4/+0 until end of turn. "
        "If you do, sacrifice it at the beginning of the next end step"
    ):
        return IfYouDo(
            do_action=MayDo(
                PumpSelfUntilEOT(power_bonus=4, toughness_bonus=0),
            ),
            then_action=ScheduleSelfSacAtNextEndStep(),
        )
    # Wickerfolk Thresher's Delirium attack-trigger body — the three-
    # sentence peek-and-route compound: "look at the top card of your
    # library. If it's a land card, you may put it onto the
    # battlefield. If you don't put the card onto the battlefield, put
    # it into your hand." Sentences 2 and 3 are anaphoric to the
    # peeked card in sentence 1 ("it", "the card"); the conditional
    # branch on land-vs-non-land and on accept-vs-decline are intra-
    # primitive choices that the sentence-splitting grammar can't carve
    # into independent clauses. Matched as a single literal here so the
    # body dispatches to the canonical
    # ``PeekTopMayBattlefieldIfLandElseHand`` primitive — no per-card
    # class needed. Only Wickerfolk Thresher in the DSK pool has this
    # exact peek-and-route shape today; broader grammar deferred until a
    # second example exists.
    if phrase == (
        "look at the top card of your library. "
        "If it's a land card, you may put it onto the battlefield. "
        "If you don't put the card onto the battlefield, put it into "
        "your hand"
    ):
        return PeekTopMayBattlefieldIfLandElseHand()
    # Osseous Sticktwister's Delirium end-step body — the per-opponent
    # three-mode-choice + conditional source-power damage to non-
    # compliers compound: "each opponent may sacrifice a nonland
    # permanent of their choice or discard a card. Then this creature
    # deals damage equal to its power to each opponent who didn't
    # sacrifice a permanent or discard a card this way." Sentence 2 is
    # anaphoric to sentence 1 ("each opponent who didn't ... this way"
    # references the per-opponent sac-or-discard choice from sentence 1),
    # so the two must stay coupled — the sentence-splitter would carve
    # the body and produce a damage clause whose "this way" reference
    # has no antecedent. Matched as a single literal here so the body
    # dispatches to the new generic
    # ``EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage`` primitive
    # — no per-card class needed. The Delirium gate at trigger-time is
    # AND-ed onto the predicate via ``StateBound(Delirium())`` upstream
    # in ``_parse_at_the_beginning_of_line``, so this site only sees the
    # post-gate-strip body. Only Osseous Sticktwister in the EoE/DSK
    # pool has this exact "may sac or discard, else take source-power
    # damage" surface today; broader grammar deferred until a second
    # example exists (e.g. a card with a different damage formula —
    # fixed N, source toughness — at which point the right move is to
    # parameterize the engine primitive's damage-resolver).
    if phrase == (
        "each opponent may sacrifice a nonland permanent of their "
        "choice or discard a card. Then this creature deals damage "
        "equal to its power to each opponent who didn't sacrifice a "
        "permanent or discard a card this way"
    ):
        return EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage()
    # Systems Override's five-sentence body — gain control of target artifact
    # or creature UEOT, untap it, grant haste UEOT (the Threaten core), plus
    # a Spacecraft-conditional charge-counter rider: put ten charge counters
    # on it; if you do, remove ten at the next end step. All five sentences
    # share the same target via anaphoric pronouns ("that permanent", "It",
    # "it", "it"). The Spacecraft conditional and the delayed counter-removal
    # trigger can't be expressed as standalone clauses in the unified grammar,
    # so the full five-sentence body is matched here as a single literal
    # dispatching to the bespoke ``SystemsOverrideEffect``. Only Systems
    # Override (EoE) in the pool has this exact surface; broader grammar
    # deferred until a second card with this shape ships.
    if phrase == (
        "gain control of target artifact or creature until end of turn. "
        "Untap that permanent. It gains haste until end of turn. "
        "If it's a Spacecraft, put ten charge counters on it. "
        "If you do, remove ten charge counters from it at the "
        "beginning of the next end step"
    ):
        return SystemsOverrideEffect()
    # Cut Propulsion's two-sentence body — target creature self-damages by
    # its power; if it has flying, it self-damages twice that much instead.
    # The second sentence is anaphoric ("If that creature has flying" refers
    # to the creature targeted in sentence 1) so the sentence-splitting
    # compound parser can't model them as two independent effects. Matched
    # here as a single literal dispatching to the bespoke engine class that
    # handles both the base-damage and the flying-doubling branch in one
    # resolve step. Only Cut Propulsion (EoE) has this exact surface;
    # broader grammar deferred until a second card with a varying factor
    # or condition ships.
    if phrase == (
        "target creature deals damage to itself equal to its power. "
        "If that creature has flying, "
        "it deals twice that much damage to itself instead"
    ):
        return CutPropulsionEffect()
    # Diplomatic Relations' two-sentence body — pump-and-vigilance on a
    # creature you control, then it deals power damage to a creature an
    # opponent controls. The second sentence's anaphoric "It" refers to
    # the pumped creature from sentence 1; the two must stay coupled so
    # the engine reads the post-pump power when resolving the damage step.
    # Only Diplomatic Relations (EoE) has this exact two-target shape
    # (my creature + opp creature) with a pump-then-deal sequence; broader
    # grammar deferred until a second card with a varying bonus or damage
    # expression ships.
    if phrase == (
        "target creature you control gets +1/+0 and gains vigilance "
        "until end of turn. It deals damage equal to its power to "
        "target creature an opponent controls"
    ):
        return DiplomaticRelationsEffect()
    # Desculpting Blast's two-sentence body — bounce any nonland permanent,
    # plus create a 1/1 Drone token if the bounced permanent was attacking.
    # The second sentence's "it" is anaphoric to the target chosen in
    # sentence 1 (the bounce target), and the Drone-creation rider is
    # conditional on that permanent's attacking status at resolution time.
    # Matched as a single literal so the engine class can inspect the
    # attacking state before the permanent has left the battlefield. Only
    # Desculpting Blast (EoE) has this exact bounce-plus-drone-if-attacking
    # surface; deferred until a second card with a different token type or
    # condition ships.
    if phrase == (
        "return target nonland permanent to its owner's hand. "
        "If it was attacking, create a 1/1 colorless Drone artifact "
        "creature token with flying and \"This token can block only "
        "creatures with flying.\""
    ):
        # The token's printing inline (mtg-zero docs/ir_defects.md 101),
        # parsed by the token grammar from the printed phrase.
        drone = _parse_spell_effect_phrase(
            "Create a 1/1 colorless Drone artifact creature token with "
            "flying and \"This token can block only creatures with "
            "flying.\"", "",
        )
        return ReturnTargetCreatureToOwnersHand(
            allow_nonland_permanent=True,
            if_was_attacking=drone,
        )
    # Scout for Survivors' two-sentence body — return up to three creature
    # cards from your graveyard to the battlefield, where the total mana
    # value of the chosen cards is 3 or less, and put a +1/+1 counter on
    # each. The second sentence ("Put a +1/+1 counter on each of them")
    # is anaphoric to the creatures returned in sentence 1; both halves
    # belong to the same ``ReturnUpToNSmallCreaturesFromGraveyardWithCounters``
    # resolve step. Only Scout for Survivors (EoE) has this exact
    # total-MV-bounded graveyard-return-with-counters surface; deferred
    # until a second card with varying N or MV bound ships.
    if phrase == (
        "return up to three target creature cards with total mana "
        "value 3 or less from your graveyard to the battlefield. "
        "Put a +1/+1 counter on each of them"
    ):
        return ReturnUpToNSmallCreaturesFromGraveyardWithCounters(
            max_n=3, max_total_mv=3,
        )
    # Mutinous Massacre's five-sentence body — choose odd or even parity,
    # destroy each creature with a mana value of the chosen quality, then
    # gain control of all creatures UEOT, untap them, and grant haste UEOT.
    # The "Then" connective, the untap, and the haste clauses are all
    # anaphoric to the mass-wrath step (the creatures that survived);
    # sentence-splitting would produce five disconnected clauses, none
    # of which parse individually as standalone effects. Matched as a
    # single literal dispatching to the bespoke engine class that handles
    # the parity-wrath + mass-steal + untap + haste in one resolve step.
    # Only Mutinous Massacre (EoE) has this exact five-sentence shape;
    # broader grammar deferred until a second card with a varying parity
    # condition or steal target ships.
    if phrase == (
        "choose odd or even. "
        "Destroy each creature with mana value of the chosen quality. "
        "Then gain control of all creatures until end of turn. "
        "Untap them. They gain haste until end of turn"
    ):
        return MutinousMassacreEffect()
    # Coordinated Clobbering's two-sentence body — tap 1–2 untapped
    # creatures you control, and they each deal damage equal to their
    # power to one target creature an opponent controls. The "They each"
    # in sentence 2 is anaphoric to the 1–2 creatures tapped in sentence 1;
    # the engine class threads both target groups through one resolve step
    # (tap slot-1 and optional slot-2, then both deal damage to slot-0).
    # Only Coordinated Clobbering (DSK) has this exact multi-tapper
    # power-damage surface; broader grammar deferred until a second card
    # with a varying tap count or damage expression ships.
    if phrase == (
        "tap one or two target untapped creatures you control. "
        "They each deal damage equal to their power to target "
        "creature an opponent controls"
    ):
        return CoordinatedClobberingEffect()
    # Trial of Agony's three-sentence body — choose two opponent creatures,
    # that player picks one which takes 5 damage, and the other can't block
    # this turn. The engine simplifies to bare DealDamageToTargetCreature(5)
    # because: (1) no "opp_pair" two-target slot-kind exists; (2) no
    # opponent-side mode-pick hook exists for selecting which of the two
    # chosen targets takes the burn; (3) no per-creature "can't block this
    # turn" flag exists. Parser-is-right precedent: Unravel / Divert Disaster
    # / Scorching Dragonfire — claim the bare base mechanic and drop the
    # unimplementable riders. Only Trial of Agony (DSK) has this exact
    # opponent-chooses-target surface; deferred until the engine grows
    # opponent-side target-choice hooks.
    if phrase == (
        "choose two target creatures controlled by the same opponent. "
        "That player chooses one of those creatures. "
        "Trial of Agony deals 5 damage to that creature, "
        "and the other can't block this turn"
    ):
        return DealDamageToTargetCreature(amount=5)
    # Zero Point Ballad's three-sentence body — destroy all creatures with
    # toughness X or less (X = cast-time X value), lose X life, then
    # conditionally return a creature to the battlefield if X ≥ 6. The
    # three sentences share the same X variable which the engine class
    # reads from ``state.resolving_x_value`` at resolve time; they cannot
    # be modeled as separate Sequence steps because each step depends on
    # the same X. The literal "X" in the normalized phrase is matched
    # verbatim (it is not a grammar variable here — the engine class owns
    # the X-reading logic). Only Zero Point Ballad (EoE) has this exact
    # X-wrath + X-life-loss + conditional-reanimate surface; broader
    # grammar deferred until a second card with a similar X-wrath body ships.
    if phrase == (
        "destroy all creatures with toughness X or less. "
        "You lose X life. "
        "If X is 6 or more, return a creature card put into "
        "a graveyard this way to the battlefield under your control"
    ):
        return ZeroPointBalladEffect()
    # Devastating Onslaught's three-sentence body — create X token copies
    # of a target artifact or creature you control, those tokens gain haste
    # UEOT, then sacrifice them at the beginning of the next end step.
    # X is the cast-time X from the {X}{X}{R} mana cost; the engine class
    # reads ``state.resolving_x_value`` at resolve time. The three sentences
    # are tightly coupled: "those tokens" and "them" are anaphoric to the
    # tokens created in sentence 1. The literal "X" in the normalized phrase
    # is matched verbatim. Only Devastating Onslaught (EoE) has this exact
    # X-token-copy + haste + end-step-sac three-sentence surface; deferred
    # until a second card with a varying element ships.
    if phrase == (
        "create X tokens that are copies of target artifact or "
        "creature you control. "
        "Those tokens gain haste until end of turn. "
        "Sacrifice them at the beginning of the next end step"
    ):
        return CreateXTokensOfTargetPermanentWithHasteEndStepSac()
    # Terminal Velocity's two-sentence body — "You may put an artifact or
    # creature card from your hand onto the battlefield. That permanent
    # gains haste, [LTB-damage-trigger], and [end-step-sac-trigger]."
    # The second sentence embeds two full quoted triggered abilities inside
    # a "gains ..., ..., and ..." clause; no per-clause recognizer can
    # decompose that structure. Sentence 1 ("you may put...") also has no
    # standalone per-clause primitive. Both blockers require non-trivial
    # new grammar, so the full two-sentence body is matched as a single
    # literal dispatching to the bespoke engine class. The oracle text
    # ends with `."` (period inside the closing double-quote), so the
    # trailing-period strip in ``_normalize_spell_effect_phrase`` does NOT
    # fire — the normalized phrase retains the trailing `."`. Only
    # Terminal Velocity (EoS) in the pool has this exact surface; broader
    # grammar deferred until a second card with this put-from-hand +
    # quoted-trigger-grants shape ships.
    if phrase == (
        'you may put an artifact or creature card from your hand onto '
        'the battlefield. That permanent gains haste, "When this '
        'permanent leaves the battlefield, it deals damage equal to '
        'its mana value to each creature," and "At the beginning of '
        'your end step, sacrifice this permanent."'
    ):
        return TerminalVelocityEffect()
    # Close Encounter's body after the additional-cost preamble is stripped
    # by ``_parse_spell_oracle_text``'s ``_is_additional_cost_preamble``
    # dropper. The remaining clause ("Close Encounter deals damage equal
    # to the power of the chosen creature or card to target creature.")
    # normalizes — after the card-name prefix strip and trailing-period
    # strip — to the phrase below. No existing recognizer covers
    # "deals damage equal to the power of the chosen creature or card":
    # ``_DEAL_DAMAGE_EQUAL_TO_SOURCE_POWER_ANY_TARGET`` matches "deals
    # damage equal to its power to any target" (source's own power), NOT
    # the "chosen creature or card" additional-cost reference. The engine
    # class reads ``state.resolving_additional_cost_choice_id`` at resolve
    # time to look up the chosen object's power. Only Close Encounter (EoE)
    # in the pool has this exact additional-cost-power damage surface;
    # broader grammar deferred until a second card with a similar
    # chosen-object-power damage shape ships.
    if phrase == (
        "deals damage equal to the power of the chosen creature or "
        "card to target creature"
    ):
        return CloseEncounterEffect()
    # Kavaron Harrier's "create ... that's tapped and attacking. Sacrifice
    # that token at end of combat." two-sentence phrase. The second
    # sentence is anaphoric ("that token" refers to the token created in
    # sentence 1) so the sentence-splitting compound parser can't model
    # this as two standalone effects — both sentences are collapsed into
    # a single ``CreateToken(attacking=True,
    # schedule_end_of_combat_sacrifice=True)`` operation. Tried BEFORE
    # ``_parse_compound_effect`` so the `. S` sentence boundary doesn't
    # split it prematurely. Only Kavaron Harrier in the EoE pool prints
    # this exact "that's tapped and attacking + end-of-combat sac" two-
    # sentence pairing; broader grammar deferred until a second card
    # with a varying piece (different token type, different sac timing)
    # ships.
    if phrase == (
        "create a 2/2 colorless Robot artifact creature token that's "
        "tapped and attacking. Sacrifice that token at end of combat"
    ):
        if "Robot" in TOKEN_PRINTINGS:
            return CreateToken(
                token_printing=TOKEN_PRINTINGS["Robot"],
                count=1,
                tapped=True,
                attacking=True,
                schedule_end_of_combat_sacrifice=True,
            )
    # Pinnacle Starcage's ``{6}{W}{W}`` activation body. Three printed
    # operations:
    #   1. Put each card exiled with this artifact into its owner's
    #      graveyard.
    #   2. For each card moved that way, create a 2/2 colorless Robot
    #      artifact creature token.
    #   3. Sacrifice this artifact.
    # The decomposition uses a Sequence of three generic primitives:
    # ``MoveEachExiledWithSourceToOwnersGraveyard`` (sets
    # ``state.last_exile_recall_count``), ``CreateNTokensFromLastExile
    # Recall(token_name="Robot")`` (reads that count), and
    # ``SacrificeSelf`` (sacrifices the source). The Robot token printing
    # is the same one Kavaron Harrier already uses.
    #
    # Matched as a multi-sentence literal here (rather than via the
    # compound-effect splitter) because the second sentence's count
    # source (``for each card put into a graveyard this way``) is an
    # anaphoric back-reference to the first sentence's recall step —
    # the splitter has no way to thread the count from one sub-effect
    # to the next, and the third sentence is a bare ``Sacrifice this
    # artifact`` body whose printed surface only standalone-parses with
    # the surrounding context. Currently only Pinnacle Starcage in the
    # EoE pool prints this exact three-step shape; broader grammar
    # deferred until a second card with a varying piece (different
    # token type, different sacrifice-self placement) ships, at which
    # point a parameterized recognizer for "<recall>; <create-N-of-
    # <token>>; <sacrifice-self>" would replace this literal.
    if phrase == (
        "put each card exiled with this artifact into its owner's "
        "graveyard, then create a 2/2 colorless Robot artifact creature "
        "token for each card put into a graveyard this way. Sacrifice "
        "this artifact"
    ):
        # The token's printing inline, parsed by the token grammar from
        # the printed phrase (mtg-zero docs/ir_defects.md 65).
        robot = _parse_spell_effect_phrase(
            "Create a 2/2 colorless Robot artifact creature token.", "",
        )
        if robot is not None:
            return Sequence(effects=(
                MoveEachExiledWithSourceToOwnersGraveyard(),
                CreateNTokensFromLastExileRecall(
                    token_name="Robot", token_printing=robot.token_printing,
                ),
                SacrificeSelf(),
            ))
    # Ghost Vacuum's ``{6}, {T}, Sacrifice this artifact`` activation
    # body (the ``Activate only as a sorcery.`` rider is stripped by
    # the activated-ability inline-rider handling before this lookup
    # runs). Two printed operations:
    #   1. Put each creature card exiled with this artifact onto the
    #      battlefield under your control with a flying counter on it.
    #   2. Each of them is a 1/1 Spirit in addition to its other types.
    # Both clauses are anaphoric back-references to the cards moved in
    # sentence 1 ("with a flying counter on it" qualifies the recall
    # step; "Each of them" refers to those same cards). The sentence-
    # splitting grammar would carve at the ``. E`` boundary and try to
    # parse "each of them is a 1/1 Spirit ..." as a standalone effect,
    # which has no standalone primitive. Matched as a single literal
    # so the whole body dispatches in one step to the parameterized
    # ``PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYour
    # Control`` recall primitive with the rider parameters set
    # (``grant_flying_counter=True``,
    # ``grant_subtype_and_pt=("Spirit", 1, 1)``). Currently only Ghost
    # Vacuum in the DSK/EoE pool prints this exact recall+counter+
    # type-grant shape; broader grammar deferred until a second card
    # with a varying piece (different counter, different subtype/PT)
    # ships, at which point a parameterized recognizer capturing the
    # counter name and (subtype, P, T) tuple would replace this literal.
    if phrase == (
        "put each creature card exiled with this artifact onto the "
        "battlefield under your control with a flying counter on it. "
        "Each of them is a 1/1 Spirit in addition to its other types"
    ):
        return PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYourControl(
            grant_flying_counter=True,
            grant_subtype_and_pt=("Spirit", 1, 1),
        )
    # Sothera, the Supervoid's end-step body (after the
    # ``_substitute_self_name`` rewrite turns the short name "Sothera"
    # into "this creature" and the trigger-line regex strips the
    # leading time-condition phrase + the intervening-if gate clause
    # ``if a player controls no creatures, ``):
    #   ``sacrifice this creature, then put a creature card exiled with
    #   it onto the battlefield under your control with two additional
    #   +1/+1 counters on it``
    # Sacrifice first, then the recall: the Oracle text's order (mtg-zero
    # docs/ir_defects.md 180; the cards exiled with Sothera stay linked to
    # it after it leaves, CR 607.2a). Sothera's DIES-trigger sibling tags
    # the opponent-exiled creatures with ``exiled_by_source = sothera.id``,
    # whose on-leave hook (``_release_exiled_until_leaves``) would return
    # every tagged card as Sothera leaves; the sacrifice
    # (``SacrificeSelfPreservingExileLink``) arms its one-shot suppression,
    # so the recall (``ReanimateOneExiledWithSourceWithCounters``) still
    # finds them and the others stay exiled.
    #
    # Multi-effect literal recognizer: only Sothera in the EoE pool
    # ships this exact sac+recall shape today. Deferred generalization
    # until a second card with the shape ships.
    if phrase == (
        "sacrifice this creature, then put a creature card exiled with "
        "it onto the battlefield under your control with two additional "
        "+1/+1 counters on it"
    ):
        return Sequence(effects=(
            SacrificeSelfPreservingExileLink(),
            ReanimateOneExiledWithSourceWithCounters(counter_amount=2),
        ))
    # Requiem Monolith's ``{T}`` activation body (after the inline
    # ``Activate only as a sorcery.`` rider strip in
    # ``_parse_activated_ability_line`` flips ``sorcery_speed=True``
    # at the activated-ability layer). Two anaphoric sentences:
    #   1. ``Until end of turn, target creature gains "Whenever this
    #      creature is dealt damage, you draw that many cards and lose
    #      that much life."`` — the printed quoted ability is itself a
    #      ``TriggeredAbility(event=Event.DEALT_DAMAGE,
    #      predicate=IsSelf("obj"), effect=
    #      DrawCardsAndLoseLifeEqualToResolvingDamage())`` granted
    #      UEOT to the chosen creature via
    #      ``GrantTriggeredAbilityUEOT``.
    #   2. ``That creature's controller may have this artifact deal 1
    #      damage to it.`` — the optional 1-damage ping (the target's
    #      controller chooses, NOT the activator), modeled by
    #      ``MayHaveSourceDealDamageToTarget(amount=1)`` whose
    #      anaphoric "that creature" is the same target the UEOT
    #      grant resolved against. ``Sequence.resolve`` threads the
    #      activator-chosen target tuple through to every sub-effect
    #      so the second sub-effect reads ``targets[0]`` to find "it".
    #
    # Matched as a multi-sentence literal here (rather than via the
    # compound-effect splitter) because the body's anaphoric "that
    # creature" pronoun crosses the sentence boundary AND the embedded
    # quoted-ability text contains both `` and `` and a sentence-final
    # period that would derail the splitter. Currently only Requiem
    # Monolith in the EoE pool prints this exact UEOT-trigger-grant +
    # target-controller-optional-ping shape; broader grammar deferred
    # until a second card with a varying piece (different quoted
    # ability, different ping amount) ships, at which point a
    # parameterized recognizer for "until end of turn, target creature
    # gains <quoted-trigger>. That creature's controller may have
    # <source> deal N damage to it" would replace this literal.
    if phrase == (
        "until end of turn, target creature gains \"Whenever this "
        "creature is dealt damage, you draw that many cards and lose "
        "that much life.\" That creature's controller may have this "
        "artifact deal 1 damage to it"
    ):
        return Sequence(effects=(
            GrantTriggeredAbilityUEOT(
                ability=TriggeredAbility(
                    event=Event.DEALT_DAMAGE,
                    predicate=IsSelf("obj"),
                    effect=DrawCardsAndLoseLifeEqualToResolvingDamage(),
                ),
            ),
            MayHaveSourceDealDamageToTarget(amount=1),
        ))
    # Haunted Screen's ``{7}`` once-only activation body (after the
    # inline ``Activate only once.`` rider strip in
    # ``_parse_activated_ability_line`` flips ``activate_only_once=True``
    # at the activated-ability layer). Two anaphoric sentences:
    #   1. ``Put seven +1/+1 counters on this artifact.`` — the
    #      ``put N +1/+1 counters on this creature`` shape doesn't
    #      cover ``this artifact``; mapped directly to
    #      ``PutPlusOneCounterOnSelf(amount=7)`` here (which reads
    #      ``state.resolving_source`` so the wording difference
    #      doesn't matter at the engine layer).
    #   2. ``It becomes a 0/0 Spirit creature in addition to its
    #      other types.`` — anaphoric "It" refers to the source from
    #      sentence 1; modeled by
    #      ``BecomeType(types_to_add=("creature",),
    #      subtypes_to_add=("Spirit",), base_pt=(0, 0))``. ``BecomeType``
    #      always writes ``expires_at=None`` registry entries so the
    #      type-change persists across turns (matches the printed "in
    #      addition to its other types" — the artifact stays an
    #      artifact). For finite-lifetime versions, wrap with
    #      ``ApplyEffect(BecomeType(...), ends_at=<Duration>)``.
    #
    # Matched as a multi-sentence literal here (rather than via the
    # compound-effect splitter) because the second sentence's "It" is
    # anaphoric to the source artifact named in sentence 1, AND the
    # ``put seven +1/+1 counters on this artifact`` surface isn't
    # claimed by the existing single-sentence ``put N +1/+1 counters
    # on this creature`` recognizer (the ``this creature`` anchor is
    # specific). Currently only Haunted Screen in the DSK/EoE pool
    # prints this exact counter-init + becomes-creature-with-subtype-
    # and-PT shape; broader grammar deferred until a second card with
    # a varying piece (different counter count, different subtype/PT)
    # ships, at which point a parameterized recognizer for "put N
    # +1/+1 counters on <self-anchor>. It becomes a P/T <subtype>
    # creature in addition to its other types" would replace this
    # literal.
    if phrase == (
        "put seven +1/+1 counters on this artifact. "
        "It becomes a 0/0 Spirit creature in addition to its other types"
    ):
        return Sequence(effects=(
            PutPlusOneCounterOnSelf(amount=7),
            BecomeType(
                types_to_add=("creature",),
                subtypes_to_add=("Spirit",),
                base_pt=(0, 0),
            ),
        ))
    # ----------------------------------------------------------------
    # Aura-context effect-phrase recognizers (audit-litdispatch-auras).
    # Each entry routes a single Aura trigger body to its existing
    # engine class via the unified effect grammar. (Historical note:
    # these recognizers were extracted from the now-retired
    # ``_AURA_LITERAL_TRIGGER_LINES`` whole-line dispatch table when
    # the bespoke surface was closed 2026-05-01/02.) The trigger
    # condition phrase (``this Aura enters`` / ``enchanted creature
    # is dealt damage`` / ``At the beginning of your end step``) is
    # already resolved by the predicate-trigger phrasebook in
    # ``triggers.py``; these recognizers complete the round trip so
    # the whole oracle line parses through the standard
    # ``_parse_triggered_ability_line`` path with no dedicated
    # literal-table lookup. Each recognizer is a literal
    # effect-phrase — only one or two cards in the pool ship the
    # matching surface today, and broader generalization is deferred
    # until a third example exists.
    # ----------------------------------------------------------------
    # ``destroy it`` — anaphoric Aura-context body. Used by
    # Cryoshatter's compound becomes-tapped-or-dealt-damage line
    # (handled by the Cryoshatter compound-line recognizer in
    # ``enchantment_builder``), Cracked Skull's DEALT_DAMAGE trigger,
    # and Sporogenic Infection's DEALT_DAMAGE trigger. The "it"
    # refers to the Aura's enchanted permanent;
    # ``DestroyEnchantedPermanent`` reads
    # ``state.resolving_source.attached_to`` and fizzles cleanly when
    # the source isn't an Aura with a host (so this recognizer is
    # safe to register at the global effect-grammar level even though
    # "it" is anaphoric — a non-Aura source's resolution becomes a
    # no-op rather than a wrong behavior). Generalization to a
    # parameterized ``destroy <subject>`` recognizer deferred until a
    # third Aura body with a different subject phrasing
    # (e.g. ``destroy enchanted creature``) ships.
    if phrase == "destroy it":
        return DestroyEnchantedPermanent()
    # ``enchanted creature fights up to one target creature an
    # opponent controls`` — Meltstrider's Resolve's ETB body. Maps
    # to ``EnchantedCreatureFightsTargetEffect`` which reads the
    # Aura's enchanted host as the fighter and lets the controller
    # pick up to one opposing creature as the other fighter; both
    # fighters deal damage equal to their power simultaneously per
    # CR 701.12. Only Meltstrider's Resolve in the pool ships this
    # surface today; broader generalization (a parameterized
    # ``<source-actor> fights <target-pool>`` recognizer) deferred
    # until a second Aura with a similar fight body ships.
    if phrase == (
        "enchanted creature fights up to one target creature an "
        "opponent controls"
    ):
        return EnchantedCreatureFightsTargetEffect()
    # ``enchanted creature deals damage equal to its power to any
    # other target`` — Pain for All's ETB body. Maps to
    # ``PainForAllETBEffect`` which damages the chosen target by the
    # enchanted creature's effective power, excluding the enchanted
    # creature itself from the legal-target pool ("any OTHER
    # target"). Distinct from the source-power surface
    # ``_DEAL_DAMAGE_EQUAL_TO_SOURCE_POWER_ANY_TARGET`` (Infernal
    # Phantom) because the damage source is the Aura's host, not
    # the Aura itself. Generalization deferred until a second Aura
    # with the same "host deals power damage" surface ships.
    if phrase == (
        "enchanted creature deals damage equal to its power to any "
        "other target"
    ):
        return PainForAllETBEffect()
    # ``deals that much damage to each opponent`` — Pain for All's
    # damage-echo body. The printed surface is ``Whenever enchanted
    # creature is dealt damage, it deals that much damage to each
    # opponent.``; ``_normalize_creature_effect_phrase`` strips the
    # leading ``it `` from the body before this lookup runs. Maps to
    # ``PainForAllDamageEchoEffect`` which reads
    # ``state.resolving_combat_damage_amount`` and routes the same
    # amount as life loss to each opponent. Generalization to a
    # ``RelayDamageToEachOpponent`` primitive deferred until a second
    # card ships this damage-relay surface.
    if phrase == "deals that much damage to each opponent":
        return PainForAllDamageEchoEffect()
    # ``look at target player's hand. You may choose a nonland card
    # from it. That player discards that card`` — Cracked Skull's
    # multi-sentence ETB body. The standard trigger parser anchors
    # on a single trailing period, so the body (which itself spans
    # three sentences) reaches this recognizer with two internal
    # periods preserved verbatim. Maps to
    # ``TargetOpponentLookHandMayDiscard`` whose printed ``You may``
    # gate is honored via ``Agent.choose_yes_no`` and whose
    # ``target player`` legality is narrowed to ``target opponent``
    # per the standard EoE/DSK simplification (self-targeting is
    # never strategically correct for the controller, so the
    # narrowing doesn't affect rational play). Only Cracked Skull
    # in the pool today; broader grammar deferred until a second
    # card prints the same look-at-hand-may-directed-discard
    # multi-sentence shape.
    if phrase == (
        "look at target player's hand. "
        "You may choose a nonland card from it. "
        "That player discards that card"
    ):
        return TargetOpponentLookHandMayDiscard()
    # ``target player sacrifices a creature of their choice other
    # than enchanted creature`` — Sporogenic Infection's ETB body.
    # Maps to ``TargetOpponentSacrificesCreatureOtherThanEnchanted``
    # which excludes the Aura's host (read from
    # ``state.resolving_source.attached_to``) from the target's sac
    # options and routes the choice through the target's
    # ``Agent.choose_sacrifice``. ``target player`` is narrowed to
    # ``target opponent`` per the standard EoE/DSK simplification —
    # self-targeting is never strategically correct for the
    # controller. Generalization to a generic
    # ``TargetPlayerSacrifices<filter>`` primitive deferred until a
    # second card with a similar excluded-host sacrifice shape ships.
    if phrase == (
        "target player sacrifices a creature of their choice other "
        "than enchanted creature"
    ):
        return TargetOpponentSacrificesCreatureOtherThanEnchanted()
    # ``sacrifice this Aura unless a nonland permanent left the
    # battlefield this turn or a spell was warped this turn`` —
    # Chorale of the Void's Void end-step rider. The full printed
    # line is ``Void — At the beginning of your end step, sacrifice
    # this Aura unless <void-gate>``; the ``Void — `` ability-word
    # prefix is informative only (CR 207.2c — ability words have no
    # rules consequence) and is consumed by ``triggers.py`` before
    # the body reaches this recognizer. The ``unless`` wording
    # inverts the standard Void-rider semantics: the trigger fires
    # every turn, but the SACRIFICE only happens when Void is
    # inactive — the void check rides the resolving effect class
    # itself rather than the ``requires_void=True`` trigger flag.
    # Maps to ``SacrificeSelfUnlessVoidActive`` which encodes the
    # unless-check at resolve time. Subject phrasing accepts ``Aura``
    # here because that's the only printed surface today; if a
    # future printing uses ``permanent`` / ``enchantment`` /
    # ``creature``, the right move is to parameterize the subject
    # rather than adding a second literal.
    if phrase == (
        "sacrifice this Aura unless a nonland permanent left the "
        "battlefield this turn or a spell was warped this turn"
    ):
        return SacrificeSelfUnlessVoidActive()
    # "you may sacrifice an artifact. When you do, <inner>" —
    # ``OptionalSacArtifactForEffect`` with inner effect parsed recursively.
    # Checked BEFORE ``_parse_compound_effect`` so the ` and ` joiner
    # inside the inner clause (e.g. "put a +1/+1 counter on target
    # creature and draw a card") isn't split at the wrong boundary —
    # the whole phrase must parse as a unit with the inner clause
    # belonging entirely to the sac-conditional rider, not split off
    # as a parallel sequence step.
    m_sac_art = _OPTIONAL_SAC_ARTIFACT_WHEN_YOU_DO_EFFECT.match(phrase)
    if m_sac_art is not None:
        inner_sac = _parse_effect(m_sac_art.group(1))
        if inner_sac is not None:
            return OptionalSacArtifactForEffect(inner_effect=inner_sac)
        # Inner doesn't parse — fall through to the compound/single-effect
        # dispatchers so this phrase can try other parse paths (belt-
        # and-suspenders; in practice the inner always either parses or
        # the card is unclaimed).
    # Artifact-bucket recognizers (cube mana rocks / utility artifacts /
    # Equipment / Vehicles) live in their own module; tried before the
    # compound splitter so their multi-sentence shapes (``flip a coin.
    # If you lose the flip, …``) stay welded together.
    from mtgz_ir.cards.parser.artifact_effects import (
        _parse_artifact_effect_phrase,
    )
    artifact_effect = _parse_artifact_effect_phrase(phrase)
    if artifact_effect is not None:
        return artifact_effect
    compound = _parse_compound_effect(phrase)
    if compound is not None:
        return compound
    if _PREVENT_ALL_COMBAT_DAMAGE_THIS_TURN.match(phrase) is not None:
        return PreventAllCombatDamageThisTurn()
    m = _DEAL_DAMAGE_ANY_TARGET.match(phrase)
    if m is not None:
        return DealDamageAnyTarget(amount=int(m.group(1)))
    # ``deals damage equal to its power to any target`` — Infernal
    # Phantom's dies trigger body in DSK. Mirrors the legal-target pool
    # of the bare ``deals N damage to any target`` shape but reads the
    # damage amount from the source's effective power at fire time
    # rather than a printed integer. Tried right after the bare shape
    # so a future ``deals damage equal to its power to target creature``
    # variant (no DSK printing today) can land its own primitive in the
    # same dispatch neighborhood.
    if _DEAL_DAMAGE_EQUAL_TO_SOURCE_POWER_ANY_TARGET.match(phrase) is not None:
        return DealDamageEqualToSourcePowerAnyTarget()
    # ``deals N damage to target creature an opponent controls`` — Nova
    # Hellkite's ETB. Tried BEFORE the bare ``deals N damage to target
    # creature`` regex because the bare regex's ``$`` anchor would not
    # match this surface anyway, but ordering more-specific before
    # more-general is the convention here.
    m = _DEAL_DAMAGE_TARGET_OPPONENT_CREATURE.match(phrase)
    if m is not None:
        return DealDamageToOpponentCreature(amount=int(m.group(1)))
    # ``[this Room ]deals damage equal to <count-source> to target
    # creature an opponent controls`` — Warmaker Gunship (count-source
    # ``artifacts you control``, EoE) and Roaring Furnace (count-source
    # ``cards in your hand``, DSK Room half). The captured count-source
    # surface text routes through
    # ``_DEAL_DAMAGE_COUNT_SOURCE_DISPATCH`` to the matching engine
    # class; both share the ``target creature an opponent controls``
    # legal-target pool (creatures NOT controlled by the resolving
    # controller) with the dynamic damage amount read at resolve time.
    m = _DEAL_DAMAGE_BY_COUNT_TARGET_OPPONENT_CREATURE.match(phrase)
    if m is not None:
        cls = _DEAL_DAMAGE_COUNT_SOURCE_DISPATCH.get(m.group(1))
        if cls is not None:
            return cls()
    # ``deals X damage to target attacking or blocking creature, where
    # X is N plus the number of creatures and/or Spacecraft you
    # control`` — Focus Fire's body (after the ``<Name> `` prefix
    # strip in ``_normalize_spell_effect_phrase``). The captured
    # integer base parameterizes ``FocusFireEffect(base=N)``; the
    # dynamic +creatures-and-Spacecraft count is read at resolve
    # time inside the engine class. See the regex's docstring for
    # the deferred-generalization plan when a second card with this
    # shape ships.
    m = _DEAL_DAMAGE_X_BASE_PLUS_FRIENDLY_CREATURE_OR_SPACECRAFT_COUNT.match(
        phrase
    )
    if m is not None:
        return FocusFireEffect(base=int(m.group(1)))
    # ``deals N damage to target creature. If excess damage was dealt
    # this way, create a Lander token`` — Orbital Plunge's full two-
    # sentence body. Compound parsing fails on this phrase because the
    # rider's anaphoric ``If excess damage was dealt this way, create
    # a Lander token`` doesn't parse as a standalone effect, so
    # ``_parse_compound_effect`` returns None and dispatch reaches this
    # multi-sentence literal-line recognizer. The captured integer
    # parameterizes ``DealDamageToTargetCreatureWithExcessLanderRider
    # (amount=N)`` whose resolve step gates Lander creation on the
    # excess-damage check at resolve time. See the regex's docstring
    # for the deferred-generalization plan when a second card with
    # this shape ships.
    m = _DEAL_DAMAGE_N_TARGET_CREATURE_WITH_EXCESS_LANDER_RIDER.match(phrase)
    if m is not None:
        return DealDamageToTargetCreatureWithExcessLanderRider(
            amount=int(m.group(1)),
        )
    # ``deals N damage to target creature. It deals M damage instead
    # if you control a Spacecraft`` — Invasive Maneuvers's full two-
    # sentence body. Compound parsing fails on this phrase because the
    # rider's anaphoric ``It deals M damage instead`` substitution-
    # syntax doesn't parse as a standalone effect, so
    # ``_parse_compound_effect`` returns None and dispatch reaches this
    # multi-sentence literal-line recognizer. The captured integers
    # parameterize ``DealDamageToTargetCreatureSpacecraftBoosted(
    # base=N, boosted=M)`` whose resolve step picks ``base`` vs
    # ``boosted`` by inspecting the controller's battlefield for a
    # Spacecraft. See the regex's docstring for the deferred-
    # generalization plan when a second card with this shape ships.
    m = _DEAL_DAMAGE_N_TARGET_CREATURE_M_IF_SPACECRAFT.match(phrase)
    if m is not None:
        return DealDamageToTargetCreatureSpacecraftBoosted(
            base=int(m.group(1)),
            boosted=int(m.group(2)),
        )
    # ``deals N damage to target opponent`` — Biotech Specialist's
    # sacrifice-an-artifact trigger body. Tried BEFORE the bare
    # ``deals N damage to target creature`` regex because the bare
    # regex's ``$`` anchor on ``target creature`` would not match the
    # ``target opponent`` tail anyway, but ordering more-specific
    # before more-general is the convention here. Maps to
    # ``DealDamageToTargetOpponent(amount=N)`` which gates the legal-
    # target pool to opponents (not the source's controller).
    m = _DEAL_DAMAGE_TARGET_OPPONENT.match(phrase)
    if m is not None:
        return DealDamageToTargetOpponent(amount=int(m.group(1)))
    m = _DEAL_DAMAGE_TARGET_CREATURE.match(phrase)
    if m is not None:
        return DealDamageToTargetCreature(amount=int(m.group(1)))
    # ``deals N damage to up to one target creature`` — Pinnacle
    # Kill-Ship's ETB body. Mirrors the bare ``deals N damage to
    # target creature`` shape but with the ``up to one`` qualifier
    # flipping ``DealDamageToTargetCreature.optional`` to True so the
    # engine treats target selection as 0..1 (resolves as a no-op
    # when no creature is chosen). Tried AFTER the bare-target form
    # because the bare regex's ``$`` anchor on ``target creature``
    # wouldn't match the ``up to one target creature`` tail anyway,
    # but ordering more-specific shapes after the more-general bare
    # form is the convention here for damage primitives — the bare
    # form acts as the default and modifiers (``up to one``,
    # ``an opponent controls``, ``tapped``) layer on top via
    # dedicated regexes.
    m = _DEAL_DAMAGE_UP_TO_ONE_TARGET_CREATURE.match(phrase)
    if m is not None:
        if m.group(2):
            # "... up to one target creature or planeswalker" (Dawnsire,
            # Sunstar Dreadnought): the planeswalker stays in the target.
            from mtgz_ir.effects.generic.damage import DealDamage
            from mtgz_ir.effects.object_predicates import (
                IsCreature as _IsCreature,
                IsPlaneswalker as _IsPlaneswalker,
            )
            from mtgz_ir.effects.targets import TargetSpec

            return DealDamage(
                amount=int(m.group(1)),
                targets=(TargetSpec(
                    _IsCreature() | _IsPlaneswalker(),
                    count_min=0, count_max=1,
                ),),
            )
        return DealDamageToTargetCreature(
            amount=int(m.group(1)), optional=True,
        )
    # ``deals N damage to each opponent`` — mass-burn opponents (Ruinous
    # Rampage's burn mode). Tried after the targeted ``deals N damage to
    # any target`` / ``… target creature`` regexes because those bind
    # tighter (the ``each opponent`` tail is an exact literal that won't
    # collide with the targeted shapes).
    m = _DEAL_N_DAMAGE_EACH_OPPONENT.match(phrase)
    if m is not None:
        return DealDamageEachOpponent(amount=int(m.group(1)))
    # ``deals N damage to each creature`` — untargeted board-wipe burn
    # (Pyroclasm). Tried after the targeted ``deals N damage to target
    # creature`` regex above because the bare creature form's ``$``
    # anchor wouldn't match the ``each creature`` tail anyway, but
    # ordering more-specific before more-general is the convention here.
    # Maps to the existing ``DealNDamageToEachCreature`` engine class,
    # already exercised inside Lithobraking's reflexive optional-sac
    # sequence and Extinguisher Battleship's ETB sequence in EoE.
    m = _DEAL_N_DAMAGE_EACH_CREATURE.match(phrase)
    if m is not None:
        return DealNDamageToEachCreature(amount=int(m.group(1)))
    # ``deals N damage to target tapped creature`` — Reroute Systems
    # mode 1's tapped-only burn. Tried AFTER the targeted ``deals N
    # damage to target creature`` regex above so the bare creature
    # form wins on phrases without the ``tapped`` qualifier; this
    # more-specific shape only claims phrases that carry the
    # ``tapped`` tail. Maps to ``DealDamageToTargetTappedCreature``
    # whose ``legal_targets`` gates on ``obj.tapped`` and re-checks
    # at resolution.
    m = _DEAL_DAMAGE_TARGET_TAPPED_CREATURE.match(phrase)
    if m is not None:
        return DealDamageToTargetTappedCreature(amount=int(m.group(1)))
    if _DRAW_CARD_PER_MULTICOLORED_PERMANENT_LINE.match(
        phrase
    ) is not None:
        # ``draw a card for each multicolored permanent you control``
        # — Infinite Guideline Station's attack trigger. Tried BEFORE
        # the bare ``draw a card`` shape because the bare regex's
        # ``draw a card$`` tail wouldn't match the ``for each
        # multicolored permanent you control`` rider anyway, but
        # more-specific before more-general is the convention here.
        # See the regex comment for the deferred-generalization plan.
        return DrawCardPerMulticoloredPermanent()
    if _DRAW_A_CARD.match(phrase) is not None:
        return DrawCards(amount=1)
    m = _DRAW_N_CARDS.match(phrase)
    if m is not None:
        return DrawCards(amount=_parse_amount(m.group(1)))
    if _DRAW_THAT_MANY_CARDS.match(phrase) is not None:
        # ``draw that many cards`` — the resolving-combat-damage form
        # used by Fear of Failed Tests's combat-damage trigger body.
        # Same engine class Starwinder uses via the bespoke table; the
        # source's combat-damage amount is latched at trigger time and
        # read at resolve time, so the engine class needs no
        # parameters.
        return DrawCardsEqualToResolvingCombatDamage()
    if _DRAW_X_CARDS_PER_DIFFERENTLY_NAMED_LAND.match(phrase) is not None:
        # ``draw X cards, where X is the number of differently named
        # lands you control`` — All-Fates Scroll's sac-draw body. Maps
        # to the generic ``DrawCardsByConditionMultiplier`` parameter-
        # ized by the existing ``PerDifferentlyNamedLand`` condition,
        # whose ``multiplier`` returns the count at resolve time. Same
        # condition the spell-side cost-reduction grammar already uses
        # (Fungal Colossus's ``This spell costs {X} less to cast, where
        # X is the number of differently named lands you control.``).
        return DrawCardsByConditionMultiplier(
            condition=PerDifferentlyNamedLand(),
            description_fragment=(
                "where X is the number of differently named lands "
                "you control"
            ),
        )
    m = _YOU_GAIN_N_LIFE.match(phrase)
    if m is not None:
        return GainLife(amount=int(m.group(1)))
    # ``your life total becomes half your starting life total, rounded
    # up`` — The Endstone's end-step trigger body. Single-card literal
    # in the EoE pool; broader grammar (``life total becomes <expr>``
    # with parameterized fraction/rounding) deferred until a second
    # printing with this shape exists. Maps to the generic
    # ``SetLifeToHalfStartingRoundedUp`` engine class which reads
    # ``starting_life`` on the controller's Player at resolve time —
    # no parameters needed today.
    if phrase == (
        "your life total becomes half your starting life total, rounded up"
    ):
        return SetLifeToHalfStartingRoundedUp()
    # ``shuffle your hand and graveyard into your library`` —
    # Weftwalking's ETB body's first half (composed in a Sequence with
    # ``draw seven cards`` via the ``, then `` joiner). Single-card
    # literal in the EoE pool today; the parameterized
    # ``ShuffleZonesIntoLibrary`` engine class accepts any tuple of
    # zones, so a future card with ``shuffle just your graveyard into
    # your library`` lands by adding a sister literal here. Tried as a
    # primitive (not a compound) because the inner `` and `` belongs to
    # the zone-list, not to a compound effect joiner — the compound
    # parser's `` and `` split would otherwise carve this phrase
    # incorrectly.
    if phrase == "shuffle your hand and graveyard into your library":
        return ShuffleZonesIntoLibrary(zones=("hand", "graveyard"))
    # ``put a +1/+1 counter on each of up to N target creatures`` —
    # multi-target counter spread (Weftblade Enhancer). Tried BEFORE
    # the bare-target regex below because the bare regex's ``target
    # creature$`` tail won't match the ``each of up to`` shape anyway,
    # but ordering more-specific before more-general is the convention
    # here. The amount tail accepts digits and spelled-out small
    # cardinals; both go through ``_parse_amount`` to land as the
    # ``max_n`` int on the engine class.
    m = _PUT_PLUS_ONE_COUNTER_ON_EACH_OF_UP_TO_N_TARGET_CREATURES.match(
        phrase
    )
    if m is not None:
        return PutPlusOneCounterOnEachOfUpToNTargetCreatures(
            max_n=_parse_amount(m.group(1)),
        )
    # ``put a +1/+1 counter on target creature you control`` — Rayblade
    # Trooper's ETB. Restricted variant: legal targets are creatures
    # the resolving controller controls. Tried BEFORE the bare ``target
    # creature`` regex (which has a ``$`` anchor and so wouldn't match
    # the ``you control`` tail anyway) so the more-specific shape wins
    # by convention.
    if _PUT_PLUS_ONE_COUNTER_ON_TARGET_CREATURE_YOU_CONTROL.match(
        phrase
    ) is not None:
        return PutPlusOneCounterOnCreatureYouControl(amount=1)
    # ``put a +1/+1 counter on target creature an opponent controls`` —
    # Sunset Saboteur's attack trigger. Mirror image of the ``you
    # control`` restriction above: legal targets are creatures NOT
    # controlled by the resolving controller. Tried BEFORE the bare
    # ``target creature`` regex (which has a ``$`` anchor that
    # wouldn't match this tail anyway) so the more-specific shape wins
    # by convention.
    if _PUT_PLUS_ONE_COUNTER_ON_TARGET_OPPONENT_CREATURE.match(
        phrase
    ) is not None:
        return PutPlusOneCounterOnOpponentsCreature(amount=1)
    m = _PUT_PLUS_ONE_COUNTER_ON_TARGET_CREATURE.match(phrase)
    if m is not None:
        # Group 1 captures the optional ``attacking `` qualifier (None
        # otherwise). When present, ``attacking_only=True`` narrows the
        # legal-target pool to currently-attacking creatures — same
        # engine flag Hospital Room's door-trigger body uses.
        return PutPlusOneCounterOnTargetCreature(
            amount=1, attacking_only=m.group(1) is not None,
        )
    m = _PUT_PLUS_ONE_COUNTER_ON_THIS_CREATURE.match(phrase)
    if m is not None:
        return PutPlusOneCounterOnSelf(amount=_parse_amount(m.group(1)) if m.group(1) != "a" else 1)
    if _PUT_LOYALTY_COUNTER_ON_THIS_CREATURE.match(phrase) is not None:
        return PutLoyaltyCounterOnSelf(amount=1)
    # ``put a +1/+1 counter on each creature you control other than
    # this creature`` — Dawnstrike Vanguard's end-step trigger body
    # (gated by the tapped-creatures wrapper at the trigger-line
    # level, but the inner effect is a plain mass-counter spread).
    # Tried BEFORE the bare-target counter regexes because the bare
    # ones all carry a ``$`` anchor on ``target creature`` etc. that
    # wouldn't match the ``each creature`` subject anyway, but
    # ordering more-specific before more-general is the convention
    # here. Maps to ``PutPlusOneCounterOnEachCreatureYouControl(
    # amount=1, exclude_source=True)`` — mirrors the static-registry
    # layout exactly (the ``other than this creature`` tail is what
    # sets the engine class's ``exclude_source`` flag). Tried BEFORE
    # the bare ``each creature you control`` regex so the more-specific
    # ``other than this creature`` shape wins on phrases that carry it.
    if _PUT_PLUS_ONE_COUNTER_ON_EACH_OTHER_CREATURE_YOU_CONTROL.match(
        phrase
    ) is not None:
        return PutPlusOneCounterOnEachCreatureYouControl(
            amount=1, exclude_source=True,
        )
    # ``put X +1/+1 counters on each creature you control, where X is
    # this creature's power`` — Ouroboroid's beginning-of-combat
    # trigger body. Tried BEFORE the bare
    # ``_PUT_PLUS_ONE_COUNTER_ON_EACH_CREATURE_YOU_CONTROL`` regex —
    # the bare regex's ``$`` anchor would never match the X-form tail
    # anyway, but ordering more-specific before more-general is the
    # convention here. Maps to
    # ``PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower()``
    # with no parameters — every aspect of the effect (X-source from
    # ``state.resolving_source.power``, per-creature spread on every
    # creature the controller controls) is encoded by the engine
    # class.
    if (
        _PUT_X_PLUS_ONE_COUNTERS_ON_EACH_CREATURE_YOU_CONTROL_WHERE_X_IS_SOURCE_POWER
    ).match(phrase) is not None:
        return PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower()
    # ``put a +1/+1 counter on each creature you control`` —
    # Atmospheric Greenhouse's ETB trigger body. Mass +1/+1-counter
    # spread on every creature the controller controls. Tried AFTER
    # the ``other than this creature`` shape above (more-specific
    # before more-general). Maps to
    # ``PutPlusOneCounterOnEachCreatureYouControl(amount=1)`` — the
    # default ``exclude_source=False`` matches the bespoke factory's
    # output for Atmospheric Greenhouse exactly.
    if _PUT_PLUS_ONE_COUNTER_ON_EACH_CREATURE_YOU_CONTROL.match(
        phrase
    ) is not None:
        return PutPlusOneCounterOnEachCreatureYouControl(amount=1)
    # ``destroy target creature or Spacecraft`` — Embrace Oblivion.
    # Tried BEFORE the bare ``destroy target creature`` regex so the
    # Spacecraft-tail variant wins. Both produce the same engine class
    # today (``DestroyTargetCreature``), just with the
    # ``or_spacecraft=True`` flag flipped — the bare regex below would
    # never match this surface anyway (its ``$`` anchor rejects the
    # ``or Spacecraft`` tail) but ordering more-specific before
    # more-general is the convention here.
    if _DESTROY_TARGET_CREATURE_OR_SPACECRAFT.match(phrase) is not None:
        return DestroyTargetCreature(or_spacecraft=True)
    # ``destroy target creature or enchantment`` — Withering Torment
    # (Duskmourn). Tried BEFORE the bare ``destroy target creature``
    # regex so the more-specific subtype-tail variant wins. The bare
    # regex's ``$`` anchor would never match this surface anyway, but
    # ordering more-specific before more-general is the convention here.
    if _DESTROY_TARGET_CREATURE_OR_ENCHANTMENT.match(phrase) is not None:
        return DestroyTargetCreature(or_enchantment=True)
    if _PUT_ITS_COUNTERS_ON_TARGET_CREATURE_YOU_CONTROL.match(
        phrase
    ) is not None:
        # ``put its counters on target creature you control`` — Dockworker
        # Drone's dies trigger. ``its`` refers to the source creature
        # (dying permanent); the engine class reads
        # ``state.resolving_source.plus_one_counters`` at resolve time
        # and transfers them to the chosen target.
        return MoveSelfCountersToTargetCreatureYouControl()
    # ``destroy up to one other target creature. If that creature
    # wasn't dealt damage this turn, its controller draws N cards``
    # — Faller's Faithful's ETB. Tried BEFORE the bare destroy regex
    # below so the longer surface wins. The bare regex's ``$`` anchor
    # would never match this surface anyway, but ordering more-
    # specific before more-general is the convention here.
    m = _DESTROY_UP_TO_ONE_OTHER_TARGET_CREATURE_DRAW_IF_UNDAMAGED.match(
        phrase
    )
    if m is not None:
        return DestroyTargetCreature(
            optional=True,
            exclude_self=True,
            draw_if_undamaged_amount=_parse_amount(m.group(1)),
        )
    # ``target creature gets -N/-0 until end of turn. Then if that
    # creature's power is 0 or less, destroy it`` — Depressurize's
    # two-sentence body. The second sentence's ``that creature`` is
    # anaphoric to the debuff target, so both sentences must be kept
    # together; the compound-effect splitter fails because the rider
    # doesn't parse as a standalone effect. Tried BEFORE the bare
    # ``destroy target creature`` regex so the two-sentence form wins
    # on phrases that carry the conditional-destroy tail; the bare
    # regex's ``$`` anchor would never match this surface anyway, but
    # ordering more-specific before more-general is the convention here.
    # Group 1 is the negative power bonus (e.g. ``-3``); negate it to
    # derive ``power_penalty``.
    m = _DEPRESSURIZE.match(phrase)
    if m is not None:
        power_penalty = abs(int(m.group(1)))
        return DepressurizeEffect(power_penalty=power_penalty)
    if _DESTROY_TARGET_CREATURE.match(phrase) is not None:
        return DestroyTargetCreature()
    # ``another target attacking creature can't be blocked this turn``
    # — Clammy Prowler's attack-trigger body. Same engine class as the
    # bare unblockable-UEOT primitive but with the ``attacking_only``
    # and ``exclude_self`` flags flipped to narrow the legal-target
    # pool at selection time. Tried BEFORE the bare regex because the
    # bare regex's ``$`` anchor would never match this longer form
    # anyway, but ordering more-specific before more-general is the
    # convention here.
    if _ANOTHER_TARGET_ATTACKING_CREATURE_CANT_BE_BLOCKED_THIS_TURN.match(
        phrase
    ) is not None:
        return GrantUnblockableUEOT(attacking_only=True, exclude_self=True)
    # ``target creature with power N or less can't be blocked this
    # turn`` — Ragged Playmate's activated body. Same engine class
    # as the bare unblockable-UEOT primitive with the
    # ``power_at_most=N`` filter narrowing the legal-target pool.
    # Tried BEFORE the bare regex so the more-specific power-filter
    # form wins.
    m = _TARGET_CREATURE_WITH_POWER_AT_MOST_CANT_BE_BLOCKED_THIS_TURN.match(
        phrase
    )
    if m is not None:
        return GrantUnblockableUEOT(power_at_most=int(m.group(1)))
    # ``target creature can't be blocked this turn`` — bare unblockable
    # grant. Pure UEOT primitive on a creature target; writes a layer-6
    # ``grant_keyword "unblockable"`` registry entry at resolve time
    # (via ``ApplyEffect(GrantUnblockable, EndOfTurn())``). Enter the
    # Enigma is the EoE/Duskmourn surface form that introduced this
    # primitive.
    if _TARGET_CREATURE_CANT_BE_BLOCKED_THIS_TURN.match(phrase) is not None:
        return GrantUnblockableUEOT()
    # ``counter target sorcery spell`` — Warping Wail mode 2. Tried
    # BEFORE the bare ``counter target spell`` regex so the more-
    # specific filter-typed shape wins. Maps to
    # ``CounterTargetSpell(filter_types=frozenset({"Sorcery"}))``.
    if _COUNTER_TARGET_SORCERY_SPELL.match(phrase) is not None:
        return CounterTargetSpell(filter_types=frozenset({"Sorcery"}))
    # ``counter target artifact or enchantment spell`` — Annul. Tried
    # BEFORE the bare ``counter target spell`` regex so the more
    # specific filter-typed shape wins. Both shapes return the same
    # engine class, just with different ``filter_types`` frozensets.
    if _COUNTER_TARGET_ARTIFACT_OR_ENCHANTMENT_SPELL.match(
        phrase
    ) is not None:
        return CounterTargetSpell(
            filter_types=frozenset({"Artifact", "Enchantment"})
        )
    # ``counter target creature or enchantment spell`` — Get Out's mode 1
    # (Duskmourn). Sibling to the artifact-or-enchantment literal above;
    # tried BEFORE the bare ``counter target spell`` regex so the filter
    # shape wins. Same engine class, different filter_types frozenset.
    if _COUNTER_TARGET_CREATURE_OR_ENCHANTMENT_SPELL.match(phrase) is not None:
        return CounterTargetSpell(
            filter_types=frozenset({"Creature", "Enchantment"})
        )
    # ``counter target spell unless its controller pays {N}`` — Mana
    # Leak shape (Divert Disaster, Don't Make a Sound). Tried BEFORE
    # the bare ``counter target spell`` regex so this more-specific
    # surface wins. The ``unless … pays`` escape clause is dropped
    # (engine doesn't model pay-cost-or-counter branching today).
    # Maps to bare ``CounterTargetSpell()`` — same simplification the
    # static registry already ships for these two cards.
    if _COUNTER_TARGET_SPELL_UNLESS_PAYS.match(phrase) is not None:
        return CounterTargetSpell()
    if _COUNTER_TARGET_SPELL.match(phrase) is not None:
        return CounterTargetSpell()
    # ETB exile-until-source-leaves shapes (Oblivion Ring / Banishing
    # Light family).  Tried BEFORE the bare exile-target-creature
    # branch so the more-specific "until … leaves" tail anchors
    # correctly and the two families don't accidentally overlap.
    m = _EXILE_OPPONENTS_NONLAND_UNTIL_LEAVES.match(phrase)
    if m is not None:
        max_mv = int(m.group(1)) if m.group(1) else 0
        return ExileOpponentsNonlandPermanentUntilLeaves(max_mv=max_mv)
    m = _EXILE_CREATURE_UNTIL_LEAVES.match(phrase)
    if m is not None:
        excluded = frozenset({m.group(2)}) if m.group(2) else frozenset()
        opponents_only = m.group(3) is not None
        return ExileTargetCreatureUntilSourceLeaves(
            excluded_subtypes=excluded,
            opponents_only=opponents_only,
            up_to_one=m.group(1) is not None,
        )
    # Mass exile-until-source-leaves (Pinnacle Starcage). Captured type
    # list is one of {"artifacts and creatures", "artifacts",
    # "creatures"}; captured MV cap is the integer threshold. Maps to
    # ``ExileAllMatchingUntilSourceLeaves`` with the appropriate boolean
    # flags. Tried right after the targeted variants — the regex
    # anchors on a leading ``exile all`` (vs ``exile target``) so the
    # two families don't overlap, but ordering more-specific before
    # more-general remains the dispatch convention here. Today only
    # Pinnacle Starcage in the EoE pool fits this surface; broader
    # generalization (additional type words, optional MV cap) will land
    # naturally as more printings ship.
    m = _EXILE_ALL_MATCHING_UNTIL_LEAVES.match(phrase)
    if m is not None:
        type_phrase = m.group(1)
        max_mv = int(m.group(2))
        return ExileAllMatchingUntilSourceLeaves(
            match_artifact=("artifacts" in type_phrase),
            match_creature=("creatures" in type_phrase),
            max_mv=max_mv,
        )
    # ``exile target creature or Spacecraft`` — Gravkill's printed
    # surface. Tried BEFORE the bare ``exile target creature`` regex so
    # the Spacecraft-tail variant wins. Both produce the same engine
    # class today (``ExileTargetCreature()``) — the static side fudges
    # the same way pending engine support for Spacecraft as a target
    # kind. When the engine grows that, this primitive should switch to
    # a Spacecraft-aware engine class in lockstep with the static.
    if _EXILE_TARGET_CREATURE_OR_SPACECRAFT.match(phrase) is not None:
        return ExileTargetCreature()
    # ``exile target creature or planeswalker`` — Archenemy's Charm
    # mode 1. Planeswalkers aren't fully modeled; same "or planeswalker"
    # scope drop as the static registry. Tried BEFORE the bare
    # ``exile target creature`` regex.
    if _EXILE_TARGET_CREATURE_OR_PLANESWALKER.match(phrase) is not None:
        return ExileTargetCreature()
    # ``exile target creature with power or toughness N or less`` —
    # Warping Wail mode 1. Tried BEFORE the bare ``exile target
    # creature`` regex so the more-specific power/toughness-threshold
    # tail wins; the bare regex's ``$`` anchor wouldn't match this
    # longer form anyway. Maps to
    # ``ExileTargetCreatureWithPowerOrToughnessAtMost(max_value=N)``.
    m = _EXILE_TARGET_CREATURE_WITH_POWER_OR_TOUGHNESS_AT_MOST.match(phrase)
    if m is not None:
        return ExileTargetCreatureWithPowerOrToughnessAtMost(
            max_value=int(m.group(1)),
        )
    if _EXILE_TARGET_CREATURE.match(phrase) is not None:
        return ExileTargetCreature()
    if _EXILE_ALL_CREATURES_AND_SPACECRAFT.match(phrase) is not None:
        return ExileAllCreaturesAndSpacecraft()
    # ``destroy all tapped creatures`` / ``destroy all untapped
    # creatures`` — Split Up's modal mode lines. Two separate
    # branches because the matching ``DestroyAllCreatures`` engine-
    # class flag is a per-flag bool. The bare ``destroy all creatures``
    # untargeted Wrath shape isn't used by any spell in the current
    # pool — add it as a sibling branch if/when one ships.
    if _DESTROY_ALL_TAPPED_CREATURES.match(phrase) is not None:
        return DestroyAllCreatures(tapped_only=True)
    if _DESTROY_ALL_UNTAPPED_CREATURES.match(phrase) is not None:
        return DestroyAllCreatures(untapped_only=True)
    # ``destroy all creatures, then any number of target players each
    # mill half their library, rounded down`` — Singularity Rupture's
    # full single-sentence body. Maps to the parameterless
    # ``SingularityRuptureEffect`` engine class which folds the wrath
    # and the per-target floor-divide mill into one resolve step. Sole
    # card with this shape today; broader generalization deferred —
    # see ``_SINGULARITY_RUPTURE`` regex comment.
    if _SINGULARITY_RUPTURE.match(phrase) is not None:
        return SingularityRuptureEffect()
    # ``destroy all creatures with power N or greater`` — Restricted
    # Office's door trigger body. Parameterized over the power
    # threshold; maps to ``DestroyAllCreatures(min_power=N)`` whose
    # resolve walks ``effective_power`` so layer-7 pumps and +1/+1
    # counters are accounted for when filtering victims.
    m = _DESTROY_ALL_CREATURES_WITH_POWER_AT_LEAST.match(phrase)
    if m is not None:
        return DestroyAllCreatures(min_power=int(m.group(1)))
    m = _EXILE_ALL_ARTIFACTS_WITH_MANA_VALUE_AT_MOST.match(phrase)
    if m is not None:
        return ExileAllArtifactsWithManaValueAtMost(max_mv=int(m.group(1)))
    # ``take an extra turn after this one`` — Nexus of Fate's single-
    # sentence main effect. Maps to the parameterless
    # ``TakeExtraTurnAfterThisOne`` engine class. The accompanying self-
    # shuffle rider is handled separately by ``_detect_shuffle_self_statics``
    # and dropped from the effect stream by ``_is_self_shuffle_clause`` in
    # ``spell_builder``; this single-sentence recognizer therefore claims
    # the full spell once the rider is stripped. Tried in the
    # single-effect dispatcher (not as a literal in the compound
    # dispatcher) because the rider is dropped at the clause level rather
    # than being joined to this sentence by a compound joiner.
    if _TAKE_EXTRA_TURN_AFTER_THIS_ONE.match(phrase) is not None:
        return TakeExtraTurnAfterThisOne()
    # ``search your library for a green creature card with mana value X
    # or less, put it onto the battlefield, then shuffle`` — Green Sun's
    # Zenith's single-sentence tutor clause. Maps to the parameterless
    # ``SearchLibraryForGreenCreatureWithMVAtMostX`` engine class which
    # reads ``state.resolving_x_value`` at resolve time to derive the MV
    # cap and puts the found creature directly onto the battlefield. The
    # spell's self-shuffle sentence is its own step (see the regex
    # comment for details). Tried BEFORE the hand-tutor family below
    # because the surface form overlaps on the ``search your library for``
    # prefix; the ``put it onto the battlefield`` reward tail is the
    # distinguishing discriminator and the hand-tutor regex's ``reveal it,
    # put it into your hand`` tail wouldn't match this form anyway, but
    # ordering more-specific before more-general is the convention here.
    if _SEARCH_LIBRARY_GREEN_CREATURE_X_TO_BATTLEFIELD.match(phrase) is not None:
        return SearchLibraryForGreenCreatureWithMVAtMostX()
    m = _SEARCH_LIBRARY_REVEAL_HAND_SHUFFLE.match(phrase)
    if m is not None:
        # ``search your library for a(n) <filter-phrase> card, reveal
        # it, put it into your hand, then shuffle`` — parameterized
        # tutor-to-hand shape. The captured filter phrase is looked up
        # in ``_SEARCH_LIBRARY_REVEAL_HAND_SHUFFLE_FILTER`` to resolve
        # both the ``filter_name`` string and the ``may_fail`` flag.
        # Unrecognized filter phrases return ``None`` (spell stays
        # unclaimed) so a future card with a new filter phrase doesn't
        # silently resolve to the wrong engine class.
        #
        # Replaces the three former literal recognizers:
        #   artifact (may_fail=True), basic_land (may_fail=True),
        #   legendary_creature (may_fail=False).
        # Also claims Demonic Counsel's base clause (Demon card,
        # may_fail=False) when the Delirium-rider dropper removes the
        # second oracle line before this recognizer is tried.
        filter_phrase = m.group(2).strip()
        mapping = _SEARCH_LIBRARY_REVEAL_HAND_SHUFFLE_FILTER.get(
            filter_phrase
        )
        if mapping is not None:
            filter_name, may_fail = mapping
            return SearchLibraryForCard(
                filter_name=filter_name, may_fail=may_fail,
            )
    if _SEARCH_LIBRARY_FOR_BASIC_LAND_CARD_TAPPED_ONTO_BATTLEFIELD.match(
        phrase
    ) is not None:
        # ``search your library for a basic land card, put it onto the
        # battlefield tapped, then shuffle`` — Moldering Gym's door
        # trigger. Same payload the Lander token's activated ability
        # uses (``SearchLibraryForBasicLandTapped()``); the duplicate
        # string-keyed entry in ``land_builder``'s
        # ``_FETCH_LAND_BODIES`` is the fetch-land surface for cycling-
        # land bodies and stays narrow to that builder's
        # tap-and-sacrifice activation shape.
        return SearchLibraryForBasicLandTapped()
    if _SEARCH_LIBRARY_FOR_EQUIPMENT_CARD_ONTO_BATTLEFIELD.match(
        phrase
    ) is not None:
        # ``search your library for an Equipment card, put it onto the
        # battlefield, then shuffle`` — Honored Knight-Captain's
        # activated-ability body (sac-self → tutor for Equipment).
        # Maps to the existing ``SearchLibraryForEquipmentToBattlefield``
        # engine class. Distinct from the basic-land-tapped sibling on
        # the filter (Equipment vs basic land) AND the tap state
        # (untapped vs tapped); a future ``put it onto the battlefield
        # tapped`` Equipment tutor would land its own surface and an
        # ``Equipment``-keyed payload class with a ``tapped=True``
        # construction (no DSK printing today carries that shape).
        return SearchLibraryForEquipmentToBattlefield()
    m = _SEARCH_LIBRARY_FOR_LAND_KIND_ONTO_BATTLEFIELD.match(phrase)
    if m is not None:
        # Fetch-land body — see the regex comment.
        kind = m.group("kind")
        return SearchLibraryForLandCardToBattlefield(
            land_types=(
                () if kind == "basic land" else tuple(kind.split(" or "))
            ),
            basic_only=kind == "basic land",
            tapped=m.group("tapped") is not None,
        )
    # ``gain control of target <subtype>`` — permanent-theft activated
    # ability restricted to a named subtype (Sliver Overlord's second
    # ability: "Gain control of target Sliver. (This effect lasts
    # indefinitely.)"). The captured subtype word is dispatched through
    # ``_GAIN_CONTROL_TARGET_SUBTYPE_EFFECT``; an unknown subtype keeps
    # the card unclaimed rather than silently dropping the filter.
    m = _GAIN_CONTROL_TARGET_SUBTYPE.match(phrase)
    if m is not None:
        effect = _GAIN_CONTROL_TARGET_SUBTYPE_EFFECT.get(m.group(1))
        if effect is not None:
            return effect
    m = _EACH_OPPONENT_LOSES_N_LIFE.match(phrase)
    if m is not None:
        return EachOpponentLosesLife(amount=int(m.group(1)))
    m = _EACH_PLAYER_DRAWS_N_CARDS.match(phrase)
    if m is not None:
        # ``each player draws a card`` collapses to amount=1 (the
        # ``a card`` alternative has no capture group, so group(1)
        # is None); digits and spelled-out small cardinals on the
        # plural arm go through ``_parse_amount``. Friendly Teddy's
        # dies trigger uses the ``a card`` form; the plural arm is
        # forward-looking grammar coverage.
        token = m.group(1)
        amount = 1 if token is None else _parse_amount(token)
        return EachPlayerDraws(amount=amount)
    if _EXILE_EACH_OPPONENTS_GRAVEYARD.match(phrase) is not None:
        # ``exile each opponent's graveyard`` — Dauntless Scrapbot's
        # first-half compound ETB. Untargeted graveyard wipe on every
        # opponent.
        return ExileEachOpponentsGraveyard()
    m = _EACH_OPPONENT_DISCARDS.match(phrase)
    if m is not None:
        # ``each opponent discards a card`` / ``each opponent discards
        # N cards`` — controller-untargeted opponent-wide discard.
        # ``a`` collapses to count=1; digits and spelled-out small
        # cardinals go through ``_parse_amount``.
        token = m.group(1)
        count = 1 if token == "a" else _parse_amount(token)
        return EachOpponentDiscards(count=count)
    if _EACH_OPPONENT_SACRIFICES_CREATURE.match(phrase) is not None:
        # ``each opponent sacrifices a nontoken creature of their
        # choice`` — Susurian Dirgecraft's ETB trigger body.
        # Untargeted opponent-wide sacrifice. Maps to
        # ``EachOpponentSacrificesCreature()`` whose engine class
        # walks each opponent and routes through their agent's
        # ``choose_sacrifice`` (skipping opponents with no legal
        # nontoken creature).
        return EachOpponentSacrificesCreature()
    if _EACH_OPPONENT_EXILES_OWN_CREATURE.match(phrase) is not None:
        # ``each opponent chooses a creature they control and exiles
        # it`` — Sothera, the Supervoid's DIES trigger body. Each
        # opponent independently picks one of their creatures via
        # ``choose_sacrifice``; the chosen object is exiled with
        # ``exiled_by_source = state.resolving_source.id`` so Sothera's
        # end-step recall can locate it via the ``creature card exiled
        # with it`` anaphoric reference. Source-link is requested via
        # ``link_to_source=True`` because the bookkeeping is required
        # by the sibling end-step ability — without the link the recall
        # would silently miss its target.
        return EachOpponentExilesOwnCreature(link_to_source=True)
    if _DEFENDING_PLAYER_LOSES_HALF_LIFE_ROUNDED_UP.match(phrase) is not None:
        # ``defending player loses half their life, rounded up`` —
        # Alpharael, Stonechosen's Void attack trigger. The engine
        # simplifies "defending player" to "each opponent" in
        # ``DefendingPlayerLosesHalfLifeRoundedUp`` — correct in
        # 2-player, documented approximation in multiplayer. See
        # the regex comment for the deferred-generalization plan.
        return DefendingPlayerLosesHalfLifeRoundedUp()
    if _TARGET_OPPONENT_REVEALS_HAND_CASTER_CHOICE_DISCARD.match(
        phrase
    ) is not None:
        # ``target opponent reveals their hand. You choose a nonland
        # card from it. That player discards that card`` — Temporal
        # Intervention's directed-discard body (caster picks which
        # card the opponent discards). The three sentences are matched
        # as a single literal-line recognizer because they form one
        # coupled primitive; see the regex comment for the deferred-
        # generalization plan when a second card with a varying piece
        # ships. Target selection (which opponent) is handled by the
        # engine class's ``legal_targets``; card selection (which
        # nonland card in that opponent's hand) is handled by the
        # engine class's ``resolve`` via the caster's
        # ``choose_card_from_opponents_hand`` hook.
        return TargetOpponentDiscardsCasterChoice()
    m = _DEFENDING_PLAYER_MILLS_N_CARDS.match(phrase)
    if m is not None:
        # ``defending player mills N cards`` — Specimen Freighter's
        # attack-trigger body. Maps to ``EachOpponentMillsCards(amount=N)``
        # which is correct in 2-player and a documented approximation
        # in multiplayer (per the engine class docstring). Amount
        # accepts digits and spelled-out small cardinals.
        return EachOpponentMillsCards(amount=_parse_amount(m.group(1)))
    # ``target creature has base power and toughness N/N until end of
    # turn. If you control M or more lands, that creature has base
    # power and toughness P/Q until end of turn instead.`` — Genemorph
    # Imago's landfall trigger.  Tried BEFORE the bare single-sentence
    # ``_SET_BASE_PT_UEOT`` recognizer so the more-specific conditional
    # form wins when both could match the leading clause.
    m = _SET_BASE_PT_CONDITIONAL_UEOT.match(phrase)
    if m is not None:
        default_p = int(m.group(1))
        default_t = int(m.group(2))
        threshold = _parse_amount(m.group(3))
        upgrade_p = int(m.group(4))
        upgrade_t = int(m.group(5))
        return SetBasePTConditionalUntilEOT(
            default_power=default_p,
            default_toughness=default_t,
            land_threshold=threshold,
            upgrade_power=upgrade_p,
            upgrade_toughness=upgrade_t,
        )
    # ``target creature has base power and toughness N/N until end of
    # turn`` — bare single-sentence form (layer 7b UEOT set).
    m = _SET_BASE_PT_UP_TO_ONE_OTHER_UEOT.match(phrase)
    if m is not None:
        return SetBasePTUntilEOT(
            power=int(m.group(1)),
            toughness=int(m.group(2)),
            optional=True,
            exclude_self=True,
        )
    m = _THIS_LAND_BECOMES_CREATURE_UEOT.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic import BecomeLandCreatureUEOT

        kws: frozenset[str] = frozenset()
        if m.group("kws"):
            parts = [
                k.strip()
                for k in re.split(r",\s+|\s+and\s+", m.group("kws"))
                if k.strip()
            ]
            if not parts or any(k not in _ANIMATED_LAND_KEYWORDS for k in parts):
                return None
            kws = frozenset(parts)
        colors = frozenset(
            _COLOR_WORD_LETTERS[w]
            for w in re.findall(r"white|blue|black|red|green", m.group("colors"))
        )
        return BecomeLandCreatureUEOT(
            power=int(m.group("p")),
            toughness=int(m.group("t")),
            granted_keywords=kws,
            granted_subtype=m.group("subtype"),
            set_colors=colors,
        )
    m = _SET_BASE_PT_UEOT.match(phrase)
    if m is not None:
        return SetBasePTUntilEOT(
            power=int(m.group(1)),
            toughness=int(m.group(2)),
        )
    m = _TARGET_ARTIFACT_BECOMES_CREATURE_UEOT.match(phrase)
    if m is not None:
        # ``up to one other target artifact you control becomes an
        # artifact creature with base power and toughness P/T and gains
        # <keywords> until end of turn`` — Synthesizer Labship's 2+
        # tier trigger body. Maps to
        # ``TargetArtifactBecomesCreatureUEOT(power=P, toughness=T,
        # granted_keywords=frozenset({...}))`` — same engine class the
        # bespoke table used directly.
        power = int(m.group(1))
        toughness = int(m.group(2))
        keyword_body = m.group(3)
        if keyword_body is not None:
            keywords = _parse_keyword_grant_body(keyword_body)
            if keywords is None:
                return None
        else:
            keywords = frozenset()
        return TargetArtifactBecomesCreatureUEOT(
            power=power,
            toughness=toughness,
            granted_keywords=keywords,
        )
    # ``create a Lander token. At the beginning of the end step on
    # your next turn, sacrifice that token`` — Kav Landseeker's ETB.
    # Tried BEFORE the bare token-creation dispatcher because the
    # bare ``create a Lander token`` regex would match the first half
    # of this phrase if the compound splitter ran differently, but
    # this single primitive captures the two-sentence body as one
    # effect (the second sentence references the token created in
    # the first, so they can't be split into independent effects).
    if _CREATE_LANDER_AND_SCHEDULE_SAC.match(phrase) is not None:
        return CreateLanderAndScheduleSacrifice()
    if _CREATE_TAPPED_ROBOT_TOKEN_PER_MULTICOLORED_LINE.match(
        phrase
    ) is not None:
        # ``create a tapped 2/2 colorless Robot artifact creature
        # token for each multicolored permanent you control`` —
        # Infinite Guideline Station's ETB. Tried BEFORE the generic
        # ``_parse_create_token_effect`` because that function's
        # ``_CREATE_CREATURE_TOKEN`` regex also matches the head of
        # this phrase (``create a tapped 2/2 colorless Robot artifact
        # creature token``) but stops at the ``$`` anchor before the
        # ``for each multicolored permanent you control`` tail —
        # without this earlier dispatch, the generic shape would
        # silently drop the per-multicolored gate. See the regex
        # comment for the deferred-generalization plan.
        return CreateTokenPerMulticoloredPermanent(
            token_printing=TOKEN_PRINTINGS["Robot"],
            tapped=True,
        )
    token_eff = _parse_create_token_effect(phrase)
    if token_eff is not None:
        return token_eff
    # ``surveil X, where X is the number of artifacts you control`` —
    # Cerebral Download's variable-surveil clause. Tried BEFORE the bare
    # ``surveil N`` regex so the artifact-count form wins on phrases that
    # carry the ``where X is the number of artifacts`` rider; the bare
    # regex's ``$`` anchor wouldn't match this longer form anyway, but
    # ordering more-specific before more-general is the convention here.
    # See the regex comment for the deferred-generalization plan.
    if _SURVEIL_X_ARTIFACT_COUNT.match(phrase) is not None:
        return Surveil(amount=1, use_artifact_count=True)
    m = _SURVEIL_N.match(phrase)
    if m is not None:
        return Surveil(amount=int(m.group(1)))
    keyword_action = _parse_keyword_action_effect(phrase)
    if keyword_action is not None:
        return keyword_action
    m = _LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD_GAIN_M_LIFE.match(phrase)
    if m is not None:
        # ``look at the top N cards of your library. Put one of them
        # into your hand and the rest into your graveyard. You gain M
        # life.`` — Commune with Evil's three-sentence body. Tried
        # BEFORE the bare two-sentence form below so the more-specific
        # three-sentence shape with the life-gain rider wins.
        # Produces Sequence(LookTopNPutOneInHandRestToGraveyard(n),
        # GainLife(m)).
        n_token = m.group(1)
        n = _parse_amount(n_token)
        m_amount = int(m.group(2))
        return Sequence(effects=(
            LookTopNPutOneInHandRestToGraveyard(n=n),
            GainLife(amount=m_amount),
        ))
    m = _LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD.match(phrase)
    if m is not None:
        # ``look at the top N cards of your library. Put one into
        # your hand and (the other|the rest) into your graveyard`` —
        # bare two-sentence look-pick-split form (NO life-gain rider —
        # the more-specific gain-life recognizer above handles
        # Commune-with-Evil's three-sentence body). The refined-
        # Cycle-1 fold merged the prior ``LookAtTopTwoSplitHandGraveyard``
        # bespoke class into ``LookTopNPutOneInHandRestToGraveyard``
        # with a ``reveal_chosen: bool`` rider; Codecracker Hound's
        # n=2 path uses ``reveal_chosen=False`` to preserve the
        # printed-text observable (no "Reveal that card" — matches
        # Rust's no-reveal resolve at state_ops_resolve.rs:6366-6394).
        n = _parse_amount(m.group(1))
        return LookTopNPutOneInHandRestToGraveyard(
            n=n, reveal_chosen=False,
        )
    m = _LOOK_TOP_N_REVEAL_OR_COUNTER.match(phrase)
    if m is not None:
        # Parameterized peek-tutor-or-counter recognizer covering both
        # Living Phone's dies trigger body (creature/power-capped, no
        # rider) and Pulsar Squadron Ace's ETB body (Spacecraft, with
        # the ``If you didn't put a card into your hand this way, put
        # a +1/+1 counter on this creature`` rider). Captures:
        #   group(1) — N (digit or spelled-out cardinal)
        #   group(2) — filter token ("Spacecraft" or "creature")
        #   group(3) — optional max-power cap (only on the creature
        #              filter form)
        #   group("rider") — present iff the +1/+1 counter rider is
        #              printed
        # The engine class's ``max_power`` is ignored for non-creature
        # filters, so passing ``int(group(3) or 0)`` is safe for the
        # Spacecraft path.
        filter_token = m.group(2)
        filter_name = (
            "spacecraft" if filter_token == "Spacecraft" else "creature"
        )
        max_power = int(m.group(3)) if m.group(3) else 0
        return LookAtTopNRevealOrCounter(
            n=_parse_amount(m.group(1)),
            filter_name=filter_name,
            max_power=max_power,
            counter_on_miss=m.group("rider") is not None,
        )
    m = _MILL_N_CARDS.match(phrase)
    if m is not None:
        return SelfMill(amount=_parse_amount(m.group(1)))
    if _MILL_A_CARD.match(phrase) is not None:
        return SelfMill(amount=1)
    if _DESTROY_TARGET_ARTIFACT.match(phrase) is not None:
        return DestroyTargetArtifact()
    if _DESTROY_TARGET_ARTIFACT_ENCHANTMENT_OR_FLYING_CREATURE.match(
        phrase
    ) is not None:
        # ``destroy target artifact, enchantment, or creature with
        # flying`` — Shattered Wings. The engine class's
        # ``legal_targets`` filters to artifacts (any), enchantments
        # (any), or creatures that currently have flying, with a
        # re-check at resolution in case a between-cast-and-resolve
        # ability removes flying. Tried after the bare
        # ``destroy target artifact`` regex would have matched (which
        # this surface text doesn't due to the ``$`` anchor) so the
        # more-specific shape only claims phrases that carry the
        # ``, enchantment, or creature with flying`` tail.
        return DestroyTargetArtifactEnchantmentOrFlyingCreature()
    if _SELF_BECOMES_COPY_OF_GRAVEYARD_PERMANENT_UEOT.match(phrase):
        from mtgz_ir.effects.generic import (
            SourceBecomesCopyOfTargetGraveyardCardUEOT,
        )

        return SourceBecomesCopyOfTargetGraveyardCardUEOT()
    if _SACRIFICE_THIS_PERMANENT.match(phrase) is not None:
        return SacrificeSelf()
    m = _DESTROY_TARGET_LAND.match(phrase)
    if m is not None:
        return DestroyTargetLand(
            nonbasic_only=m.group("nonbasic") is not None,
        )
    if _DESTROY_TARGET_NONCREATURE_PERMANENT.match(phrase) is not None:
        return DestroyTargetNoncreaturePermanent()
    if (
        _DESTROY_TARGET_NONLAND_PERMANENT_ITS_CONTROLLER_CREATES_LANDER.match(
            phrase
        ) is not None
    ):
        # ``destroy target nonland permanent. Its controller creates a
        # Lander token`` — Emergency Eject's 2-sentence body. The Lander
        # goes to the CONTROLLER OF THE TARGET, not the spell's caster.
        # Tried BEFORE the bare ``destroy target nonland permanent``
        # regex so this more-specific 2-sentence form wins on phrases
        # that carry the Lander rider; the bare regex's ``$`` anchor
        # would never match the rider tail anyway, but ordering
        # more-specific before more-general is the convention here.
        return DestroyTargetNonlandPermanentWithLanderForController()
    if _DESTROY_UP_TO_ONE_TARGET_NONLAND_PERMANENT.match(phrase) is not None:
        # ``destroy up to one target nonland permanent`` — Syr Vondam,
        # Sunstar Exemplar's self-leaves trigger. The "up to one" wording
        # makes the target optional (0 or 1); maps to
        # ``DestroyUpToOneTargetNonlandPermanent`` whose TargetSpec carries
        # ``count_min=0``. Tried BEFORE the bare "destroy target" form so
        # the more-specific "up to one" spelling wins.
        return DestroyUpToOneTargetNonlandPermanent()
    if _DESTROY_TARGET_NONLAND_PERMANENT.match(phrase) is not None:
        # ``destroy target nonland permanent`` — Thaumaton Torpedo's
        # activated ability. Tried after ``destroy target noncreature
        # permanent`` because the two surface forms are disjoint
        # (different exclusion words), but ordering more-specific before
        # more-general is the convention here. Maps to
        # ``DestroyTargetNonlandPermanent`` whose ``legal_targets``
        # filters the battlefield to permanents that aren't lands.
        return DestroyTargetNonlandPermanent()
    # ``destroy target artifact or enchantment. If its mana value was 2
    # or less, create a Lander token`` — Seedship Impact's full two-
    # sentence body. Compound parsing fails on this phrase because the
    # rider's anaphoric ``If its mana value was 2 or less`` back-
    # references the destroyed permanent's mana value with no per-
    # clause primitive recognizing the back-reference, so
    # ``_parse_compound_effect`` returns None and dispatch reaches this
    # multi-sentence literal-line recognizer. Maps to the parameterless
    # ``SeedshipImpactEffect`` engine class whose resolve step destroys
    # the artifact-or-enchantment target and gates Lander creation on
    # the destroyed permanent's printed mana value (≤ 2). See the
    # regex's docstring for the deferred-generalization plan when a
    # second card with this shape ships.
    if _SEEDSHIP_IMPACT.match(phrase) is not None:
        return SeedshipImpactEffect()
    if _DESTROY_TARGET_ARTIFACT_OR_TAPPED_CREATURE.match(phrase) is not None:
        # ``destroy target artifact or tapped creature`` — Radiant
        # Strike's destroy clause. The engine class's
        # ``legal_targets`` filters to artifacts (any) plus creatures
        # currently tapped, and re-checks the predicate at resolution
        # in case a between-cast-and-resolve untap removes the tapped
        # creature from the legal pool. Tried after the bare
        # ``destroy target artifact`` regex would have matched (which
        # this surface text doesn't) so the more-specific shape only
        # claims phrases that actually carry the ``or tapped
        # creature`` tail.
        return DestroyTargetArtifactOrTappedCreature()
    # ``tap another target creature`` — Starport Security's activated
    # ability. Tried BEFORE the ``tap target artifact or creature``
    # regex below because the bare regex's ``$`` anchor wouldn't match
    # the ``another`` qualifier anyway, but ordering more-specific
    # before more-general is the convention here.
    if _TAP_ENCHANTED_PERMANENT.match(phrase) is not None:
        # ``tap enchanted creature`` / ``tap enchanted permanent`` — Aura
        # ETB trigger bodies (Stay Hidden, Stay Silent; Tractor Beam). The
        # host is identified via ``state.resolving_source.attached_to`` at
        # resolve time by ``TapEnchantedPermanent``; no target is chosen.
        return TapEnchantedPermanent()
    if _TAP_ANOTHER_TARGET_CREATURE.match(phrase) is not None:
        return TapAnotherTargetCreature()
    if _TAP_TARGET_ARTIFACT_OR_CREATURE.match(phrase) is not None:
        # ``tap target artifact or creature`` — Mental Modulation's
        # middle clause and any future card with the same surface.
        # No-op if the chosen target is already tapped at resolution.
        return TapTargetArtifactOrCreature()
    if _TAP_TARGET_OPPONENT_PERMANENT_AND_STUN_IT.match(phrase) is not None:
        # ``tap target artifact or creature an opponent controls and
        # put a stun counter on it`` — Mechanozoa's ETB. Both clauses
        # apply to the same chosen target — the ``and`` is an
        # intra-effect conjunction, not a compound joiner.
        return TapTargetOpponentPermanentAndStunIt()
    m = _TAP_UP_TO_ONE_TARGET_CREATURE_AND_STUN.match(phrase)
    if m is not None:
        # ``tap up to one target creature and put N stun counters on
        # it`` — Meat Locker's door trigger. Both clauses apply to
        # the same chosen target (the ``and`` is intra-effect, not a
        # compound joiner). The captured count tail collapses ``a``
        # to 1 and routes digits / spelled-out small cardinals
        # through ``_parse_amount`` — same shape as
        # ``_DISCARD_N_CARDS`` and the other count-tail primitives.
        # ``up to one`` resolves as a no-op when no target is chosen
        # (engine-side ``target_count_range = (0, 1)``).
        token = m.group(1)
        amount = 1 if token == "a" else _parse_amount(token)
        return TapAndStunUpToOneTargetCreature(stun_amount=amount)
    if _PUT_STUN_COUNTER_UP_TO_ONE_TAPPED.match(phrase) is not None:
        # ``put a stun counter on up to one target tapped creature`` —
        # Cryogen Relic's activated payload. ``up to one`` resolves as
        # a no-op fizzle when no legal target is chosen; the engine
        # class's ``legal_targets`` filters to currently-tapped
        # creatures.
        return PutStunCounterOnTappedCreature()
    if _UNTAP_ANOTHER_TARGET_PERMANENT.match(phrase) is not None:
        # ``untap another target permanent`` — Nanoform Sentinel's
        # becomes-tapped trigger body. The ``another`` qualifier
        # excludes the source from the legal target pool — encoded
        # by the engine class's reading of ``state.resolving_source``
        # at resolve time, not by a parser-level flag. Maps to
        # ``UntapTargetOtherPermanent()`` with no parameters.
        return UntapTargetOtherPermanent()
    if _OWNER_PUTS_TARGET_ARTIFACT_OR_CREATURE_ON_TOP_OR_BOTTOM.match(
        phrase
    ) is not None:
        # ``target artifact or creature's owner puts it on their
        # choice of the top or bottom of their library`` — Lost in
        # Space's bounce-to-library clause. The chosen target's
        # owner decides top vs bottom at resolution (not the
        # spell's controller). Maps to the engine class with no
        # parameters — the surface form is a literal match.
        return OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary()
    if _OWNER_PUTS_TARGET_NONLAND_PERMANENT_ON_TOP_OR_BOTTOM.match(
        phrase
    ) is not None:
        # ``target nonland permanent's owner puts it on their choice
        # of the top or bottom of their library`` — Vanish from
        # Sight's bounce-to-library clause. Same engine class as the
        # artifact-or-creature shape with the ``nonland_permanent``
        # flag flipped to widen the legal-target pool.
        return OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary(
            nonland_permanent=True,
        )
    # ``add {<mana>}. Spend this mana only to <restriction>`` —
    # restricted-mana ability (Steelswarm Operator). Tried BEFORE the
    # bare ``add {<color>}`` regex so the qualifier wins; the bare
    # regex's ``$`` anchor wouldn't match the multi-sentence shape
    # anyway, but ordering more-specific before more-general is the
    # convention here. The mana segment is parsed by inspecting the
    # captured brace expression — the regex enforces that it's exactly
    # one or two same-color brace pairs but doesn't decompose them
    # into engine coins, so we extract the colors directly.
    m = _ADD_ONE_OF_MANA_COMBINATIONS.match(phrase)
    if m is not None:
        runs = re.findall(_MANA_RUN, m.group("list"))
        combos = []
        for run in runs:
            counts: dict[str, int] = {}
            for c in re.findall(r"\{([WUBRGC])\}", run):
                counts[c] = counts.get(c, 0) + 1
            combos.append(tuple(counts.items()))
        if any(sum(n for _, n in combo) > 1 for combo in combos):
            return AddOneOfManaCombinations(options=tuple(combos))
    m = _ADD_MANA_FOR_EACH_CONTROLLED.match(phrase)
    if m is not None:
        pred = _ADD_MANA_PER_CONTROLLED_NOUNS.get(m.group("noun"))
        if pred is None:
            return None
        return AddManaOfColorPerCount(
            color=m.group("color"),
            amount=CountControlledMatching(pred),
        )
    m = _ADD_MANA_WITH_CREATURE_HASTE_RIDER.match(phrase)
    if m is not None:
        counts: dict[str, int] = {}
        for c in re.findall(r"\{([WUBRGC])\}", m.group(1)):
            counts[c] = counts.get(c, 0) + 1
        return AddManaToPool(
            produces=tuple(counts.items()),
            grants_haste_to_creature_spell=True,
        )
    m = _ADD_MANA_WITH_SPEND_RESTRICTION.match(phrase)
    if m is not None:
        mana_str = m.group(1)
        restriction = _SPEND_RESTRICTION_BY_PHRASE.get(m.group(2))
        if restriction is None:
            return None
        # The mana segment is one or two ``{<color>}`` brace pairs;
        # ``parse_mana_cost`` would unpack them but for restricted-mana
        # abilities the engine wants a tuple of single-coin entries
        # (one ``(color, 1)`` per coin minted), so we walk the brace
        # pairs directly.
        colors = re.findall(r"\{([WUBRGC])\}", mana_str)
        return AddManaToPool(
            produces=tuple((c, 1) for c in colors),
            spend_restriction=restriction,
        )
    # ``add one mana of any color. Spend this mana only to <restriction>``
    # — restricted any-color mana ability (Plaza of Heroes line 2:
    # "Add one mana of any color. Spend this mana only to cast a
    # legendary spell."). Tried BEFORE the bare ``add one mana of any
    # color`` recognizer so the spend-restriction qualifier wins.
    m = _ADD_ANY_COLOR_MANA_WITH_SPEND_RESTRICTION.match(phrase)
    if m is not None:
        tail = m.group(1)
        restriction = _SPEND_RESTRICTION_BY_PHRASE.get(tail)
        if restriction is None:
            # "..., and that spell can't be countered" (Delighted
            # Halfling) -- the cube W/G bucket's restricted-mana shape.
            from mtgz_ir.cards.parser.creature_wg_effects import parse_wg_effect

            return parse_wg_effect(phrase)
        return AddManaToPool(
            produces=(("any", 1),),
            spend_restriction=restriction,
        )
    # ``add one mana of any color among <group> you control`` — one
    # mana of a color among the named group's permanents you control
    # (Plaza of Heroes line 3: "Add one mana of any color among
    # legendary permanents you control."). Tried BEFORE the bare
    # any-color recognizer so the group qualifier wins.
    m = _ADD_ANY_MANA_AMONG_GROUP.match(phrase)
    if m is not None:
        group = m.group(1)
        spec = _ADD_MANA_AMONG_GROUPS.get(group)
        if spec is None:
            return None
        from mtgz_ir.effects.generic.artifact_mechanics import (
            AddOneManaOfColorAmongControlled,
        )

        card_types, legendary = spec
        return AddOneManaOfColorAmongControlled(
            card_types=card_types,
            legendary=legendary,
            group_description=group,
        )
    m = _ADD_TWO_SAME_COLOR_MANA.match(phrase)
    if m is not None:
        # Two same-color coin shape (``add {U}{U}``) — only same-color
        # pairs reach this regex (the inner brace pattern captures both
        # letters and we reject mismatched pairs to keep the engine's
        # produced tuple simple). A mismatched pair like
        # ``add {U}{R}`` falls through to the bare single-color regex
        # and fails its anchor, so the card stays unclaimed (no EoE
        # printing today uses dual-color two-coin add anyway).
        if m.group(1) != m.group(2):
            return None
        c = m.group(1)
        return AddManaToPool(produces=((c, 1), (c, 1)))
    m = _ADD_N_COLORLESS_MANA.match(phrase)
    if m is not None:
        # ``add {C}{C}…`` — N colorless-mana coins (The Eternity
        # Elevator's ``{T}: Add {C}{C}{C}.``). Count the ``{C}``
        # occurrences in the captured sequence; each ``{C}`` is exactly
        # 3 characters so the count is ``len(m.group(1)) // 3``.
        n = m.group(1).count("{C}")
        return AddManaToPool(produces=(("C", n),))
    if _ADD_X_MANA_ANY_COLOR_PER_CHARGE_COUNTER.match(phrase) is not None:
        # ``add X mana of any one color, where X is the number of charge
        # counters on this creature`` — The Eternity Elevator's tier-gated
        # activated ability. Maps to ``AddChargeCounterManaOfChosenColor``
        # whose ``resolve`` reads the source's charge counter count at fire
        # time and mints that many coins of the controller's chosen color.
        return AddChargeCounterManaOfChosenColor()
    # ``add {<c1>} or {<c2>}`` and ``add {<c1>}, {<c2>}, or {<c3>}`` —
    # choose-one-of-N-colors single-coin mana abilities (Haunted Screen's
    # first two activations: ``{T}: Add {W} or {B}.`` and ``{T}, Pay 1
    # life: Add {G}, {U}, or {R}.``). Both produce
    # ``AddNManaOfChosenColor(amount=1, options=(<colors>))`` — same
    # primitive ``land_builder`` uses for filter-land ``add {C} or
    # {D}``-style abilities, just driven by oracle text instead of by
    # name-keyed dispatch. Tried BEFORE the bare ``add {<color>}`` regex
    # so the multi-color choice shape wins; the bare regex's ``$`` anchor
    # wouldn't match the ``or {…}`` tail anyway, but ordering more-
    # specific before more-general is the convention here.
    m = _ADD_TWO_COLOR_CHOICE_MANA.match(phrase)
    if m is not None:
        return AddNManaOfChosenColor(
            amount=1, options=(m.group(1), m.group(2)),
        )
    m = _ADD_THREE_COLOR_CHOICE_MANA.match(phrase)
    if m is not None:
        return AddNManaOfChosenColor(
            amount=1, options=(m.group(1), m.group(2), m.group(3)),
        )
    m = _ADD_SINGLE_COLOR_MANA.match(phrase)
    if m is not None:
        return AddManaToPool(produces=((m.group(1), 1),))
    if _ADD_ONE_MANA_OF_ANY_COLOR.match(phrase) is not None:
        # ``add one mana of any color`` — at-resolution choose-color
        # mana ability. Mints a single ``("any", 1)`` coin; the engine
        # prompts the controller to name the color at resolution. Same
        # production tuple ``land_builder`` builds for Mana Confluence.
        return AddManaToPool(produces=(("any", 1),))
    m = _TARGET_OPPONENT_LOSES_N_LIFE.match(phrase)
    if m is not None:
        return TargetOpponentLosesLife(amount=int(m.group(1)))
    m = _THEY_LOSE_N_LIFE.match(phrase)
    if m is not None:
        # ``they lose N life`` — effect body on an opponent-discards
        # observer trigger (Entropic Battlecruiser's 1+ tier trigger).
        # The "they" pronoun is the event's player (the discarding
        # player), not a target: ``BindEventPlayer`` fixes it when the
        # trigger fires, as the cube's "that player loses N life"
        # triggers (no target is chosen, so hexproof doesn't stop it).
        from mtgz_ir.effects.generic.cube_creatures_ub import BindEventPlayer

        return BindEventPlayer(
            inner=TargetPlayerLosesLife(amount=int(m.group(1))),
        )
    m = _THAT_PLAYER_GETS_N_POISON_COUNTERS.match(phrase)
    if m is not None:
        # ``that player gets N poison counters`` — effect body on a
        # combat-damage observer trigger (Virulent Silencer). The "that
        # player" pronoun refers to the damaged player, who is pre-wired
        # as ``fixed_targets=(("player", id),)`` by the
        # ``DEALS_COMBAT_DAMAGE`` dispatch.
        # ``PutPoisonCountersOnTargetPlayer`` reads the pre-wired target
        # from ``targets[0]`` at resolve time.
        return PutPoisonCountersOnTargetPlayer(amount=_parse_amount(m.group(1)))
    if _RETURN_UP_TO_ONE_OTHER_PERMANENT_YOU_CONTROL_TO_OWNERS_HAND.match(
        phrase
    ) is not None:
        # ``return up to one other target permanent you control to its
        # owner's hand`` — Exosuit Savior's ETB. ``up to one`` →
        # ``optional=True`` (resolves as a no-op if no target chosen);
        # ``other`` → ``exclude_self=True`` (filters the trigger source
        # via ``state.resolving_source``).
        return ReturnTargetPermanentYouControlToOwnersHand(
            optional=True, exclude_self=True,
        )
    m = _DESTROY_THEN_CONTROLLER_SEARCHES_BASIC_TYPED_LAND.match(phrase)
    if m is not None:
        from mtgz_ir.effects.combinators import ThenTargetControllerResolves
        from mtgz_ir.effects.generic import Destroy
        from mtgz_ir.effects.object_predicates import (
            ControlledByOpponent,
            IsBasic,
            IsLand,
        )
        from mtgz_ir.effects.targets import TargetSpec

        if m.group("noun") != "artifact, enchantment, or nonbasic land":
            return None
        pool = (
            IsArtifact() | _IsEnchantment() | (IsLand() & ~IsBasic())
        ) & ControlledByOpponent()
        return ThenTargetControllerResolves(
            first=Destroy(targets=(TargetSpec(pool),)),
            then=SearchLibraryForLandCardToBattlefield(
                land_types=("Plains", "Island", "Swamp", "Mountain", "Forest"),
            ),
        )
    m = _RETURN_TARGET_NOUN_TO_OWNERS_HAND.match(phrase)
    if m is not None and m.group("noun") in _BOUNCE_TARGET_NOUNS:
        # ``return target <noun> to its owner's hand`` — see the regex
        # comment; ``Bounce`` re-verifies the filter at resolution.
        from mtgz_ir.effects.generic import Bounce
        from mtgz_ir.effects.targets import TargetSpec

        return Bounce(
            targets=(TargetSpec(_BOUNCE_TARGET_NOUNS[m.group("noun")]),),
        )
    if _RETURN_UP_TO_ONE_TARGET_NONLAND_PERMANENT_TO_OWNERS_HAND.match(
        phrase
    ) is not None:
        # ``return up to one target nonland permanent to its owner's
        # hand`` — Unnerving Grasp's first clause. More specific than
        # the bare creature form below (``allow_nonland_permanent=True``
        # widens to any nonland permanent; ``optional=True`` keeps the
        # ``count_min=0`` "up to one" fizzle-clean semantics).
        # Tried BEFORE the bare creature form so the nonland-permanent
        # widening wins when present.
        return ReturnTargetCreatureToOwnersHand(
            allow_nonland_permanent=True, optional=True
        )
    m = _RETURN_UP_TO_ONE_TARGET_CREATURE_TO_OWNERS_HAND.match(phrase)
    if m is not None:
        # ``return up to one target creature to its owner's hand`` —
        # Bottomless Pool's door trigger. ``up to one`` →
        # ``optional=True`` (resolves as a no-op fizzle if no target
        # chosen); no controller restriction (any creature, friend or
        # foe, is a legal target). The bare ``target creature`` form
        # (Aether Spellbomb) is the mandatory single-target bounce.
        return ReturnTargetCreatureToOwnersHand(
            optional=m.group(1) is not None,
        )
    m = _RETURN_UP_TO_N_NON_SPACECRAFT_CREATURES_TO_OWNERS_HANDS.match(
        phrase
    )
    if m is not None:
        # ``return up to N target non-Spacecraft creatures to their
        # owners' hands`` — Specimen Freighter's ETB. The
        # ``non-Spacecraft`` qualifier flips
        # ``ReturnUpToNTargetCreaturesToOwnersHands.exclude_spacecraft``
        # to True (legal pool excludes stationed and unstationed
        # Spacecraft alike); ``max_n=N`` is the captured bound. Amount
        # accepts digits and spelled-out small cardinals.
        return ReturnUpToNTargetCreaturesToOwnersHands(
            max_n=_parse_amount(m.group(1)),
            exclude_spacecraft=True,
        )
    m = _RETURN_UP_TO_N_NONLAND_PERMANENT_GY_TO_HAND_THEN_LANDS_BF_TAPPED.match(
        phrase
    )
    if m is not None:
        # ``return up to N target nonland permanent cards from your
        # graveyard to your hand, then return up to M target land cards
        # from your graveyard to the battlefield tapped`` — Pull Through
        # the Weft's full two-compound sentence. The first compound piece
        # (nonland permanent cards to hand) is modeled;  the second
        # compound piece (land cards to battlefield tapped) is dropped as
        # an unimplemented rider — the engine has no "return land card
        # from GY to BF tapped" primitive today.  This produces a more
        # faithful parse than the previous whole-spell entry which
        # simplified to bare ``ReturnCreatureFromYourGraveyardToHand()``
        # (dropping BOTH the multi-target count AND the nonland-permanent
        # scope widening).
        # Both clauses: two target groups, the nonland permanent cards
        # to the hand, then the land cards onto the battlefield tapped
        # (mtg-zero docs/ir_defects.md 49).
        from mtgz_ir.effects.generic.card_flow import CardFilter
        from mtgz_ir.effects.generic.eoe_fidelity import (
            GraveyardReturnGroup,
            ReturnTargetCardsFromYourGraveyard,
        )

        return ReturnTargetCardsFromYourGraveyard(groups=(
            GraveyardReturnGroup(
                card_filter=CardFilter(any_types=("Permanent",), excluded_types=("Land",)),
                min_n=0, max_n=_parse_amount(m.group(1)),
            ),
            GraveyardReturnGroup(
                card_filter=CardFilter(any_types=("Land",)),
                min_n=0, max_n=_parse_amount(m.group(2)),
                destination="battlefield", tapped=True,
            ),
        ))
    # ``return one or two target creature and/or planeswalker cards from
    # your graveyard to your hand`` — Archenemy's Charm mode 2. Maps to
    # ``ReturnUpToNCreaturesFromYourGraveyardToHand(max_n=2, min_n=1)``
    # which targets creature graveyard cards only (planeswalkers dropped
    # per the same scope-drop precedent as mode 1). Tried BEFORE the
    # single-target bare form so the "one or two" multi-target shape wins.
    if (
        _RETURN_ONE_OR_TWO_CREATURE_OR_PLANESWALKER_FROM_GRAVEYARD_TO_HAND.match(
            phrase
        ) is not None
    ):
        # "creature and/or planeswalker cards", one or two of them
        # (mtg-zero docs/ir_defects.md 62).
        from mtgz_ir.effects.generic.card_flow import CardFilter
        from mtgz_ir.effects.generic.eoe_fidelity import (
            GraveyardReturnGroup,
            ReturnTargetCardsFromYourGraveyard,
        )

        return ReturnTargetCardsFromYourGraveyard(groups=(
            GraveyardReturnGroup(
                card_filter=CardFilter(any_types=("Creature", "Planeswalker")),
                min_n=1, max_n=2,
            ),
        ))
    m = _RETURN_TARGET_CREATURE_GRAVEYARD_TO_HAND_WITH_MV.match(phrase)
    if m is not None:
        # ``return target creature card with mana value N or less from
        # your graveyard to your hand`` — Savior of the Small's
        # Survival reward. The MV cap narrows the legal-target pool to
        # graveyard creatures whose printed mana value clears the
        # threshold; ``max_mv=0`` (unset) keeps the original unbounded
        # behaviour for Interceptor Mechan and the bare regex below.
        return ReturnCreatureFromYourGraveyardToHand(max_mv=int(m.group(1)))
    # ``return target artifact or creature card from your graveyard to
    # your hand`` — Interceptor Mechan's ETB. Tried before the bare
    # ``creature card`` regex so the more-specific OR-disjunction wins.
    if _RETURN_TARGET_ARTIFACT_OR_CREATURE_GRAVEYARD_TO_HAND.match(
        phrase
    ) is not None:
        return ReturnCreatureFromYourGraveyardToHand(or_artifact=True)
    if _RETURN_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_HAND.match(
        phrase
    ) is not None:
        # ``return a creature or Spacecraft card from your graveyard
        # to your hand`` — Fell Gravship's ETB body second sentence.
        # Tried before the bare ``return target creature card`` regex
        # below for the more-specific-first convention; the bare
        # regex's ``$`` anchor on ``hand`` would never match the
        # ``or Spacecraft`` rider anyway. ``or_spacecraft=True``
        # widens the engine class's legal-target pool to include
        # Spacecraft cards in the graveyard alongside creature cards,
        # mirroring the ``or_spacecraft`` flag on
        # ``DestroyTargetCreature`` (Embrace Oblivion).
        # Not a target (CR 115.1d: "a ... card"): the card is chosen as
        # the ability resolves, after the mill (mtg-zero
        # docs/ir_defects.md 100).
        from mtgz_ir.effects.generic.card_flow import CardFilter, CardFilterAnyOf
        from mtgz_ir.effects.generic.cube_glue import ReturnChosenGraveyardCardToHand

        return ReturnChosenGraveyardCardToHand(card_filter=CardFilterAnyOf(filters=(
            CardFilter(any_types=("Creature",)),
            CardFilter(subtype="Spacecraft"),
        )))
    if _RETURN_TARGET_CREATURE_GRAVEYARD_TO_HAND.match(phrase) is not None:
        return ReturnCreatureFromYourGraveyardToHand()
    # Xu-Ifit, Osteoharmonist's reanimate-as-Skeleton multi-sentence
    # surface: the returned permanent has the subtype in addition to its
    # other types and (the optional rider) no abilities (mtg-zero
    # docs/ir_defects.md 45).
    m = _RETURN_TARGET_CREATURE_GRAVEYARD_TO_BATTLEFIELD_AS_SKELETON.match(phrase)
    if m is not None:
        from mtgz_ir.effects.generic.card_flow import CardFilter
        from mtgz_ir.effects.generic.eoe_fidelity import (
            ReturnTargetCardFromYourGraveyardToBattlefieldAs,
        )

        return ReturnTargetCardFromYourGraveyardToBattlefieldAs(
            card_filter=CardFilter(any_types=("Creature",)),
            added_subtypes=(m.group("subtype"),),
            loses_all_abilities=m.group("no_abilities") is not None,
        )
    # ``return all creature cards from your graveyard to the
    # battlefield`` — Awakening Hall's door trigger (Funeral Room //
    # Awakening Hall, the only EoE/DSK printing of this exact surface
    # today). Maps to ``ReturnAllCreaturesFromYourGraveyardToBattlefield()``.
    # Literal-line recognizer with a deferred-generalization note on
    # the regex above — fold this and any future ``...to your hand``
    # mass-recursion sibling into one parameterized regex when the
    # engine grows the matching sibling primitive.
    if _RETURN_ALL_CREATURE_CARDS_GRAVEYARD_TO_BATTLEFIELD.match(
        phrase
    ) is not None:
        return ReturnAllCreaturesFromYourGraveyardToBattlefield()
    m = _RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD_WITH_MV.match(
        phrase
    )
    if m is not None:
        # ``return target creature or Spacecraft card with mana value N
        # or less from your graveyard to the battlefield`` — Scrounge
        # for Eternity's reanimation clause. Combines the
        # ``or_spacecraft=True`` widening (legal targets include
        # Spacecraft cards in the graveyard) with the ``max_mv=N`` cap
        # (targets whose printed mana value exceeds N are excluded).
        # Tried BEFORE the bare ``or Spacecraft`` and the bare ``with
        # mana value N`` variants below so the more-specific combined
        # form wins; both sibling regexes' ``$`` anchors don't match
        # this longer surface anyway, but more-specific-first is the
        # convention here.
        return ReturnCreatureFromYourGraveyardToBattlefield(
            or_spacecraft=True,
            max_mv=int(m.group(1)),
        )
    m = _RETURN_TARGET_CREATURE_GRAVEYARD_TO_BATTLEFIELD_WITH_MV.match(phrase)
    if m is not None:
        # ``return target creature card with mana value N or less from
        # your graveyard to the battlefield`` — Surgical Suite's door
        # trigger. The MV cap narrows the legal-target pool to graveyard
        # creatures whose printed mana value clears the threshold;
        # ``max_mv=None`` (the default on the bare regex below) keeps
        # the unbounded behaviour for Xu-Ifit and any other unrestricted
        # reanimator. Mirrors the to-hand sibling's max_mv rider.
        return ReturnCreatureFromYourGraveyardToBattlefield(
            max_mv=int(m.group(1)),
        )
    if _RETURN_TARGET_CREATURE_OR_SPACECRAFT_GRAVEYARD_TO_BATTLEFIELD.match(
        phrase
    ) is not None:
        # ``return target creature or Spacecraft card from your graveyard
        # to the battlefield`` — Seedship Broodtender's activated body.
        # ``or_spacecraft=True`` widens the legal-target pool to include
        # Spacecraft cards in the graveyard alongside creature cards.
        return ReturnCreatureFromYourGraveyardToBattlefield(
            or_spacecraft=True,
        )
    if _RETURN_TARGET_CREATURE_OR_ENCHANTMENT_GRAVEYARD_TO_BATTLEFIELD.match(
        phrase
    ) is not None:
        # ``return target creature or enchantment card from your
        # graveyard to the battlefield`` — Rescue Skiff's ETB body.
        # Tried BEFORE the bare ``return target creature card`` regex
        # below for the more-specific-first convention; the bare regex's
        # ``$`` anchor on ``battlefield`` would not match the
        # ``or enchantment`` rider anyway. ``or_enchantment=True``
        # widens the engine class's legal-target pool to include
        # enchantment cards in the graveyard alongside creature cards,
        # mirroring the ``or_spacecraft`` flag on
        # ``ReturnCreatureFromYourGraveyardToHand`` (Fell Gravship).
        return ReturnCreatureFromYourGraveyardToBattlefield(
            or_enchantment=True,
        )
    if _RETURN_TARGET_CREATURE_GRAVEYARD_TO_BATTLEFIELD.match(
        phrase
    ) is not None:
        return ReturnCreatureFromYourGraveyardToBattlefield()
    if _RETURN_TARGET_ARTIFACT_GRAVEYARD_TO_HAND.match(phrase) is not None:
        return ReturnTargetArtifactCardFromYourGraveyardToHand()
    # DSK Enduring cycle dies-trigger body (Innocence / Curiosity /
    # Tenacity / Courage / Vitality). The phrase ``return it to the
    # battlefield under its owner's control. It's an enchantment``
    # appears verbatim as the effect tail of every Enduring creature's
    # intervening-if dies trigger; the parenthesized
    # ``(It's not a creature.)`` reminder is stripped before the
    # effect body reaches this dispatcher. Maps to the parameterless
    # ``EnduringDiesReturnAsEnchantmentEffect`` engine class.
    if _ENDURING_DIES_RETURN_AS_ENCHANTMENT.match(phrase) is not None:
        return EnduringDiesReturnAsEnchantmentEffect()
    # ``put target card from a graveyard on the bottom of its owner's
    # library`` — Chrome Companion's tap-mana activated ability. Untyped
    # grave-target across all players; resolution moves the chosen card
    # to the bottom of its owner's library. Maps to the existing
    # ``PutCardFromGraveyardOnBottomOfLibrary`` engine class — same
    # primitive the static registry uses for Chrome Companion today, so
    # parser/static parity holds out of the box.
    if _PUT_CARD_FROM_GRAVEYARD_ON_BOTTOM_OF_LIBRARY.match(
        phrase
    ) is not None:
        return PutCardFromGraveyardOnBottomOfLibrary()
    # ``put target exiled card with warp on the bottom of its owner's
    # library`` — Blade of the Swarm's ETB modal mode 2. The target pool
    # is restricted to exiled cards that have a warp cost (cards the
    # engine tracks in ``state.warped_exile``); the chosen card moves
    # to the bottom of its owner's library. Literal recognizer (only
    # card in the current pool with this surface; broader "put target
    # exiled card on bottom of library" grammar deferred until a second
    # example exists). Maps to the existing
    # ``OwnerPutsTargetWarpedExileCardOnBottomOfLibrary`` engine class.
    if phrase == (
        "put target exiled card with warp on the bottom of its owner's library"
    ):
        return OwnerPutsTargetWarpedExileCardOnBottomOfLibrary()
    # ``exile target card from a graveyard`` — Ghost Vacuum's bare
    # tap activation. Sibling to the bottom-of-library shape above;
    # same target-kind pool but the chosen card moves to its owner's
    # exile zone, with the resolving source's id stamped on
    # ``exiled_by_source`` so a sibling recall ability can find every
    # card the same source exiled later (Ghost Vacuum's
    # ``{6}, {T}, Sacrifice this artifact`` recall is the only EoE/DSK
    # consumer of that linkage today).
    if _EXILE_CARD_FROM_GRAVEYARD.match(phrase) is not None:
        return ExileTargetCardFromGraveyard()
    # ``until end of turn, target creature gets +N/+M, gains <kw>, and
    # becomes <type-change> in addition to its other types`` — Jump
    # Scare's leading-UEOT pump + keyword-grant + dropped type-change
    # rider.  Tried BEFORE both the bare-pump regex and the standard
    # ``target creature gets ... and gains ... UEOT`` shape because the
    # leading ``until end of turn, `` adverb means neither trailing-UEOT
    # regex would ever match. The type-change rider is dropped because the
    # engine has no UEOT-scoped type-line addition on targeted creatures.
    m = _PUMP_LEADING_UEOT_KEYWORDS_BECOMES_TYPE.match(phrase)
    if m is not None:
        # Groups: 1=power, 2=toughness, 3=keyword-grant body
        # (everything between "gains " and ", and becomes").
        keywords = _parse_keyword_grant_body(m.group(3))
        if keywords is None:
            return None
        return PumpTargetCreatureUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
            granted_keywords=keywords,
        )
    # Pump + grant-keywords compound is tried BEFORE the bare-pump
    # regex so a phrase like ``+3/+0 and gains first strike and reach
    # until end of turn`` produces the keyword-bearing
    # ``PumpTargetCreatureUntilEOT``. The bare-pump regex below would
    # never match this surface (it requires the stats to be directly
    # followed by ``until end of turn``), but ordering more-specific
    # before more-general is the convention in this dispatcher.
    m = _PUMP_AND_GAIN_KEYWORDS_UEOT.match(phrase)
    if m is not None:
        # Group layout: 1=``another `` prefix, 2=``attacking `` prefix,
        # 3=``you control ``, 4=power, 5=toughness, 6=granted-keywords
        # body. The ``another`` qualifier excludes the trigger source
        # from the legal target pool via ``exclude_self=True``;
        # ``you control`` restricts targets to the controller's own
        # creatures via ``you_control_only=True``; ``attacking``
        # restricts targets to creatures currently flagged
        # ``attacking=True`` via ``attacking_only=True``. All three
        # flags compose so the ``another target creature you control``
        # surface lands with the first two flipped, and a hypothetical
        # ``another target attacking creature you control`` would land
        # with all three flipped.
        keywords = _parse_keyword_grant_body(m.group(6))
        if keywords is None:
            return None
        return PumpTargetCreatureUntilEOT(
            power_bonus=int(m.group(4)),
            toughness_bonus=int(m.group(5)),
            granted_keywords=keywords,
            you_control_only=m.group(3) is not None,
            exclude_self=m.group(1) is not None,
            attacking_only=m.group(2) is not None,
        )
    # ``target creature an opponent controls gets ±N/±M until end of
    # turn`` — opponent-controlled-only debuff/pump (Sinister
    # Cryologist's ETB). Tried BEFORE the bare-pump regex because that
    # bare regex's tail (``until end of turn$``) would never match
    # phrases with the ``an opponent controls`` qualifier in the
    # subject anyway, but ordering more-specific before more-general
    # is the convention here.
    m = _PUMP_TARGET_OPPONENT_CREATURE_UEOT.match(phrase)
    if m is not None:
        return PumpTargetCreatureUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
            opponents_only=True,
        )
    # ``target creature you control gets ±N/±M until end of turn`` —
    # you-control-only single-target pump (Zookeeper Mechan's ``{6}{R}``
    # sorcery-speed activated ability). Tried BEFORE the bare
    # ``target creature gets … UEOT`` regex so the ``you control``
    # qualifier wins; the bare regex's ``$`` anchor wouldn't match the
    # qualifier anyway, but ordering more-specific before more-general
    # is the convention here.
    #
    # Tried BEFORE the bare ``you control gets …`` regex (below) because
    # the bare regex's ``$`` anchor would never match the warp-rider tail
    # anyway, but ordering more-specific before more-general is the
    # convention here.
    m = _PUMP_TARGET_CREATURE_YOU_CONTROL_WARP_KEYWORDS_RIDER_UEOT.match(
        phrase
    )
    if m is not None:
        # ``target creature you control gets +N/+M until end of turn.
        # If that creature was cast for its warp cost, it also gains
        # <kw> until end of turn`` — Full Bore's 2-sentence body.
        # Keyword body validated via ``_parse_keyword_grant_body``;
        # rejection returns None so an unknown keyword keeps the spell
        # unclaimed rather than silently dropping the grant.
        keywords = _parse_keyword_grant_body(m.group(3))
        if keywords is None:
            return None
        return PumpTargetCreatureUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
            you_control_only=True,
            granted_keywords_if_warp_cast=keywords,
        )
    m = _PUMP_TARGET_CREATURE_YOU_CONTROL_UEOT.match(phrase)
    if m is not None:
        return PumpTargetCreatureUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
            you_control_only=True,
        )
    # ``up to one target creature gets ±N/±M until end of turn`` —
    # optional pump/debuff (Dubious Delicacy's ETB ``-3/-3``). The
    # ``up to one`` qualifier flips the engine class's
    # ``target_count_range`` to ``(0, 1)`` and ``requires_target`` to
    # False, so a no-target resolution is a clean no-op rather than a
    # fizzle. Tried BEFORE the bare-pump regex because that bare regex's
    # ``$`` anchor wouldn't match the ``up to one`` prefix anyway, but
    # ordering more-specific before more-general is the convention here.
    m = _PUMP_UP_TO_ONE_TARGET_CREATURE_UEOT.match(phrase)
    if m is not None:
        return PumpTargetCreatureUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
            optional=True,
        )
    if _DOUBLE_TARGET_CREATURE_YOU_CONTROL_POWER_UEOT.match(
        phrase
    ) is not None:
        # ``double the power of target creature you control until end
        # of turn`` — Mightform Harmonizer's landfall trigger. Distinct
        # from the bare ``gets +N/+0`` pump shapes above: the bonus is
        # computed at resolve time from the target's current power
        # rather than a fixed constant. Maps to the engine class with
        # no parameters — the surface form is a literal match.
        return DoubleTargetCreaturesPowerUEOT()
    m = _PUMP_TARGET_CREATURE_X_POWER_UEOT.match(phrase)
    if m is not None:
        # +X/+0 where X is the cost-payment-determined value read from
        # state.resolving_x_value (Secluded Starforge's tap-X-artifacts
        # pump). Tried before the fixed ±N/±M regex (whose digit groups
        # never match the literal "x" anyway, but ordering specific
        # before general is the convention).
        return PumpTargetCreatureUntilEOT(power_bonus_from_x=True)
    m = _PUMP_TARGET_CREATURE_UEOT.match(phrase)
    if m is not None:
        return PumpTargetCreatureUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
        )
    # ``[other ]creatures you control get ±N/±M[ and gain <kw>...]
    # until end of turn`` — anthem-style mass pump on every creature
    # the controller controls, with optional source-self exclusion
    # (``other``) and optional grant-keywords rider. The bare form is
    # Dual-Sun Adepts' ``{5}`` activated ability; the
    # exclude-source-and-grant-keywords form is Syr Vondam, the
    # Lucent's enters/attacks trigger ("other creatures you control
    # get +1/+0 and gain deathtouch until end of turn"). An
    # unrecognized granted-keyword body rejects the whole phrase
    # (returns ``None`` so the caller can fall through) — mirrors the
    # ``_PUMP_AND_GAIN_KEYWORDS_UEOT`` precedent: a missing grant is
    # never silently dropped.
    #
    # Tried BEFORE the bare ``creatures you control get …`` regex so
    # the more-specific 2-sentence body (including the untap-if-not-
    # your-turn rider) wins on phrases that carry it; the bare
    # regex's ``$`` anchor wouldn't match the rider tail anyway, but
    # ordering more-specific before more-general is the convention.
    m = _PUMP_YOUR_CREATURES_UEOT_UNTAP_IF_NOT_YOUR_TURN.match(phrase)
    if m is not None:
        # ``creatures you control get +N/+M until end of turn. If
        # it's not your turn, untap those creatures`` — Zealous
        # Display's 2-sentence body. The second sentence's ``those
        # creatures`` is anaphoric to the pumped set; kept together
        # so the sentence splitter doesn't carve the rider into an
        # unparseable piece. ``also_untap_if_not_your_turn=True``
        # fires the untap step only when the spell resolves on an
        # opponent's turn.
        return PumpCreaturesYouControlUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
            also_untap_if_not_your_turn=True,
        )
    m = _PUMP_YOUR_CREATURES_UEOT.match(phrase)
    if m is not None:
        granted_body = m.group(4)
        granted_keywords: frozenset[str] = frozenset()
        if granted_body is not None:
            parsed_keywords = _parse_keyword_grant_body(granted_body)
            if parsed_keywords is None:
                return None
            granted_keywords = parsed_keywords
        return PumpCreaturesYouControlUntilEOT(
            power_bonus=int(m.group(2)),
            toughness_bonus=int(m.group(3)),
            exclude_source=m.group(1) is not None,
            granted_keywords=granted_keywords,
        )
    # ``creatures your opponents control get ±N/±M until end of turn`` /
    # ``... until your next turn`` — mass debuff/pump on every creature
    # an opponent controls (Mouth of the Storm's ETB uses the
    # ``until your next turn`` tail). The ``until_caster_next_turn``
    # flag flips when the tail matches the longer-duration form;
    # otherwise the bonus clears at the casting turn's cleanup.
    m = _PUMP_OPPONENT_CREATURES_UEOT.match(phrase)
    if m is not None:
        return PumpCreaturesOpponentsControlUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
            until_caster_next_turn=(m.group(3) == "your next turn"),
        )
    # ``this creature gets +N/+M and gains <kw>[ and <kw2>...] until
    # end of turn`` — self-pump combined with a self keyword grant
    # (Dashing Bloodsucker's Eerie body: ``this creature gets +2/+0
    # and gains lifelink until end of turn``). Tried BEFORE the bare
    # ``_PUMP_SELF_UEOT`` regex so the more-specific compound shape
    # wins; the bare regex's ``$`` anchor would reject the
    # ``and gains …`` tail anyway, but ordering more-specific before
    # more-general is the convention here. The keyword body is parsed
    # via ``_parse_keyword_grant_body`` and validated against the
    # keyword-line whitelist; rejection returns ``None`` so an unknown
    # keyword keeps the wrapping card unclaimed.
    m = _PUMP_AND_GAIN_KEYWORDS_SELF_UEOT.match(phrase)
    if m is not None:
        keywords = _parse_keyword_grant_body(m.group(3))
        if keywords is None:
            return None
        return PumpSelfUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
            granted_keywords=keywords,
        )
    # ``this creature gets +N/+M until end of turn`` — source pumps
    # itself UEOT. Tried AFTER the targeted ``target creature gets …
    # UEOT`` regex above because the ``this creature`` subject is a
    # different (source-self) primitive from a targeted bonus. Both
    # signs accepted, mirroring the symmetry of the targeted variant.
    m = _PUMP_SELF_UEOT.match(phrase)
    if m is not None:
        return PumpSelfUntilEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
        )
    # ``this creature gains <kw>[ and <kw2>...] until end of turn`` —
    # pure self-grant (no P/T pump). Cult Healer's Eerie body
    # (``this creature gains lifelink until end of turn``). Maps to
    # ``PumpSelfUntilEOT(power_bonus=0, toughness_bonus=0,
    # granted_keywords=...)`` — same engine class as the self-pump-
    # and-gain shape just with zero bonuses. Tried AFTER both self-
    # pump shapes so a phrase carrying a P/T pump prefix wins, and
    # AFTER the targeted bare-grant regexes (which all carry their
    # own ``target creature`` / ``another target creature you
    # control`` subject — the bare ``this creature`` subject doesn't
    # collide). Validated via ``_parse_keyword_grant_body``.
    m = _GAIN_KEYWORDS_SELF_UEOT.match(phrase)
    if m is not None:
        keywords = _parse_keyword_grant_body(m.group(1))
        if keywords is None:
            return None
        return PumpSelfUntilEOT(
            power_bonus=0,
            toughness_bonus=0,
            granted_keywords=keywords,
        )
    # ``another target creature you control gets +X/+X until end of
    # turn, where X is this creature's power`` — Hemosymbic Mite's
    # becomes-tapped trigger body. Variable-X bonus where X is fixed
    # to the source's effective power at resolve time; the engine
    # class ``PumpTargetOtherCreatureByXOverXWhereXIsSourcePower``
    # reads ``state.resolving_source.power`` and applies the bonus
    # symmetrically (P/T both gain the same X) to the chosen target,
    # which must be a creature the source's controller controls and
    # not be the source itself. Maps to the engine class with no
    # parameters — every aspect of the effect is encoded by the
    # class. Future "where X is …" forms with a different scaling
    # source (e.g. number of artifacts you control) would land their
    # own primitive rather than parameterizing this one.
    if _PUMP_OTHER_TARGET_CREATURE_BY_X_OVER_X_SOURCE_POWER.match(
        phrase
    ) is not None:
        return PumpTargetOtherCreatureByXOverXWhereXIsSourcePower()
    # ``another target creature you control gains <kw>[ and <kw2>...]
    # until end of turn`` — pure keyword grant to another creature you
    # control (Blooming Stinger's ETB grants deathtouch). No P/T pump
    # — only the keyword grant fires. Validated against the keyword-
    # line whitelist via ``_parse_keyword_grant_body``; rejection
    # returns ``None`` so an unknown keyword keeps the wrapping card
    # unclaimed rather than silently dropping the grant.
    #
    # Tried BEFORE the ``another target creature you control gains …``
    # shape so the more-specific 2-sentence body (including the
    # conditional draw rider) wins on phrases that carry it.
    # ``put N +1/+1 counters on target creature you control. It gains
    # <kw> until end of turn`` — Archenemy's Charm mode 3. Two-sentence
    # recognizer: the ``it`` pronoun in sentence 2 refers to the target
    # of sentence 1, so both sentences are kept together. Maps to
    # ``PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl
    # (amount=N, granted_keywords=...)``. Tried BEFORE the draw-if-
    # counter shape so the more-specific counter-count+grant shape wins;
    # the draw-if-counter regex's tail would never match the
    # ``It gains ...`` sentence anyway.
    m = _PUT_PLUS_ONE_COUNTERS_ON_TARGET_CREATURE_YOU_CONTROL_IT_GAINS_UEOT.match(
        phrase
    )
    if m is not None:
        keywords = _parse_keyword_grant_body(m.group(2))
        if keywords is not None:
            return PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl(
                amount=_parse_amount(m.group(1)),
                granted_keywords=keywords,
            )
    m = _GRANT_KEYWORDS_TO_TARGET_CREATURE_YOU_CONTROL_DRAW_IF_COUNTER_UEOT.match(
        phrase
    )
    if m is not None:
        # ``target creature you control gains <kw> until end of turn.
        # If it has a +1/+1 counter on it, draw a card`` — Dual-Sun
        # Technique's 2-sentence body. The keyword body is validated
        # via ``_parse_keyword_grant_body`` (rejection returns None);
        # ``draw_if_target_has_plus_one_counter=1`` fires when the
        # chosen target has at least one +1/+1 counter at resolve time.
        keywords = _parse_keyword_grant_body(m.group(1))
        if keywords is None:
            return None
        return PumpTargetCreatureUntilEOT(
            power_bonus=0,
            toughness_bonus=0,
            granted_keywords=keywords,
            you_control_only=True,
            draw_if_target_has_plus_one_counter=1,
        )
    m = _GRANT_KEYWORDS_TO_ANOTHER_TARGET_CREATURE_YOU_CONTROL_UEOT.match(
        phrase
    )
    if m is not None:
        keywords = _parse_keyword_grant_body(m.group(1))
        if keywords is None:
            return None
        return PumpTargetCreatureUntilEOT(
            power_bonus=0,
            toughness_bonus=0,
            granted_keywords=keywords,
            you_control_only=True,
            exclude_self=True,
        )
    # ``target creature gains <kw>[ and <kw2>...] until end of turn`` —
    # pure keyword grant on a bare creature target (Horrid Vigor's
    # ``Target creature gains deathtouch and indestructible until end
    # of turn.`` in DSK). No control restriction, no P/T pump — only
    # the keyword grant fires. Tried AFTER the ``another target
    # creature you control gains …`` shape above so the more-specific
    # qualifier wins; this bare-target shape only claims phrases that
    # don't carry the ``you control`` / ``another`` / ``an opponent
    # controls`` qualifiers. The keyword body is parsed via
    # ``_parse_keyword_grant_body`` and validated against the
    # keyword-line whitelist; rejection returns ``None`` so an
    # unknown keyword keeps the wrapping spell unclaimed.
    #
    # Group 1 captures the optional ``legendary `` qualifier (None
    # otherwise); group 2 captures the optional ``attacking ``
    # qualifier (None otherwise); group 3 captures the keyword body.
    # When ``legendary`` is present, ``legendary_only=True`` restricts
    # the legal-target pool to creatures with the Legendary supertype
    # (Plaza of Heroes' "Target legendary creature gains hexproof and
    # indestructible until end of turn."). When ``attacking`` is
    # present, ``attacking_only=True`` restricts to attacking creatures.
    m = _GRANT_KEYWORDS_TO_TARGET_CREATURE_UEOT.match(phrase)
    if m is not None:
        keywords = _parse_keyword_grant_body(m.group(3))
        if keywords is None:
            return None
        return PumpTargetCreatureUntilEOT(
            power_bonus=0,
            toughness_bonus=0,
            granted_keywords=keywords,
            legendary_only=m.group(1) is not None,
            attacking_only=m.group(2) is not None,
        )
    # ``target artifact or creature gains <kw>[ and <kw2>...] until
    # end of turn`` — pure keyword grant on a permanent target whose
    # legal pool spans both artifacts and creatures (Reroute Systems
    # mode 0 grants indestructible). Maps to
    # ``PumpTargetArtifactOrCreatureUntilEOT(granted_keywords=...)``.
    # Validated against the keyword-line whitelist; rejection returns
    # ``None`` so an unknown keyword keeps the wrapping spell
    # unclaimed rather than silently dropping the grant.
    m = _PUMP_TARGET_ARTIFACT_OR_CREATURE_GAINS_KEYWORDS_UEOT.match(phrase)
    if m is not None:
        keywords = _parse_keyword_grant_body(m.group(1))
        if keywords is None:
            return None
        return PumpTargetArtifactOrCreatureUntilEOT(
            granted_keywords=keywords,
        )
    m = _PUT_CHARGE_COUNTERS_ON_TARGET_SPACECRAFT_OR_PLANET_YOU_CONTROL.match(
        phrase
    )
    if m is not None:
        # ``put N charge counters on target Spacecraft or Planet you
        # control`` — Drill Too Deep mode 0 and any future card with
        # the same surface. Amount is a digit or one of the spelled-out
        # small cardinals; both go through ``_parse_amount`` to land
        # as an int on the engine class's ``amount`` field.
        return PutChargeCountersOnTargetSpacecraftOrPlanetYouControl(
            amount=_parse_amount(m.group(1)),
        )
    if _MILL_TARGET_PLAYER_EQUAL_TO_YOUR_LIFE.match(phrase) is not None:
        # ``target player mills cards equal to your life total`` —
        # Space-Time Anomaly. Maps to the engine class's
        # ``use_controller_life=True`` flag, which substitutes the
        # controller's current life total for the bare amount at
        # resolve time. Tried BEFORE the bare-amount mill regex so
        # the more-specific shape wins; the bare-amount form would
        # never match this surface anyway (its tail is a digit) but
        # ordering more-specific before more-general is the
        # convention here.
        return MillTargetPlayer(use_controller_life=True)
    m = _MILL_TARGET_PLAYER_N_CARDS.match(phrase)
    if m is not None:
        # ``target player mills N cards`` — bare-amount targeted
        # mill. Forward-looking surface form (no EoE card prints
        # this exact shape today; Space-Time Anomaly uses the
        # variable form above), but the regex accepts digits and
        # spelled-out small cardinals so a future printing parses
        # without further grammar work.
        return MillTargetPlayer(amount=_parse_amount(m.group(1)))
    m = _LOSE_N_LIFE.match(phrase)
    if m is not None:
        return LoseLife(amount=int(m.group(1)))
    # ``manifest dread, then put a +1/+1 counter on that creature`` /
    # ``manifest dread, then put N +1/+1 counters on that creature`` —
    # Slimy Aquarium / Weight Room door triggers. Coupled primitive in
    # the engine (``ManifestDreadThenPutPlusOneCountersOnIt``) where
    # ``that creature`` is anaphoric to the just-manifested face-down
    # 2/2 (NOT a chosen target). Tried BEFORE the bare ``_MANIFEST_DREAD``
    # regex below so the more-specific compound-with-counter shape
    # wins; the bare regex's ``$`` anchor on the count tail wouldn't
    # match this surface anyway, but ordering more-specific before
    # more-general keeps the dispatch narrative readable.
    m = _MANIFEST_DREAD_THEN_PUT_PLUS_ONE_COUNTERS_ON_THAT_CREATURE.match(
        phrase
    )
    if m is not None:
        # Group 1 is the plural counter amount (digit or spelled-out
        # cardinal); ``None`` means the singular ``a +1/+1 counter``
        # arm matched, which collapses to amount=1.
        amount_token = m.group(1)
        counter_amount = (
            1 if amount_token is None else _parse_amount(amount_token)
        )
        return ManifestDreadThenPutPlusOneCountersOnIt(
            counter_amount=counter_amount,
        )
    # ``manifest dread`` (count=1) or ``manifest dread <amount> times`` /
    # ``manifest dread twice`` / ``manifest dread three times`` /
    # ``manifest dread X times``. Maps to
    # ``ManifestDread(count=N)`` — N is 1 for the bare form, 2/3/N for
    # the spelled-out / digit forms, or ``None`` for the X-times form
    # (count is read from ``state.resolving_x_value`` at resolve time).
    # Tried BEFORE the generic compound-effect splitter would carve a
    # phrase like ``manifest dread X times, then put X +1/+1 counters on
    # each of those creatures`` (no card uses that as a top-level
    # primitive today; Valgavoth's Onslaught has it but the current
    # parser doesn't claim it because the second clause's ``those
    # creatures`` carry-over isn't yet supported).
    m = _MANIFEST_DREAD.match(phrase)
    if m is not None:
        twice_token = m.group(1)
        amount_token = m.group(2)
        if twice_token is not None:
            count: int | None = 2
        elif amount_token is None:
            count = 1
        elif amount_token == "X":
            count = None
        else:
            count = _parse_amount(amount_token)
        return ManifestDread(count=count)
    m = _DISCARD_N_CARDS.match(phrase)
    if m is not None:
        # ``discard a card`` / ``you discard a card`` / ``discard N
        # cards`` — controller-discards. Surfaces inside compound
        # ``draw a card, then discard a card`` (Mechan Navigator) and
        # any future card that prints a bare discard clause. ``a``
        # collapses to amount=1; digits and spelled-out small
        # cardinals go through ``_parse_amount``.
        token = m.group(1)
        amount = 1 if token == "a" else _parse_amount(token)
        return DiscardSelf(amount=amount)
    if _DRAW_EQUAL_TO_SACRIFICED_POWER.match(phrase):
        from mtgz_ir.effects.formulas import PowerOfCostPaidObject
        from mtgz_ir.effects.generic.roto_walkers import DrawCardsEqualTo

        return DrawCardsEqualTo(count=PowerOfCostPaidObject())
    m = _YOU_MAY_PAY_THEN_EFFECT.match(phrase)
    if m is not None:
        # Parse the inner-effect phrase recursively. The inner phrase is
        # already in normalized form (lowercased first letter doesn't
        # apply here — Scryfall prints "If you do, <lowercase-inner>"
        # because the inner is a sub-clause, not a sentence start). If
        # the inner doesn't parse, the whole "you may pay …" line
        # rejects so the wrapping card stays unclaimed (rather than
        # silently dropping the consequence).
        inner = _parse_effect(m.group(2))
        if inner is None:
            return None
        return OptionalManaCostThenEffect(
            cost_str=m.group(1), inner_effect=inner,
        )
    m = _YOU_MAY_SAC_ARTIFACT_WHEN_YOU_DO.match(phrase)
    if m is not None:
        # ``(?:then )?you may sacrifice an artifact. When you do,
        # <inner-effect>`` — the ``OptionalSacArtifactForEffect``
        # compound. Mirrors ``_YOU_MAY_PAY_THEN_EFFECT``: parse the
        # inner tail recursively; reject the whole phrase if it doesn't
        # parse (no silent drop). Tried after ``_YOU_MAY_PAY_THEN_EFFECT``
        # because the ``when you do`` consequent is disjoint from the
        # ``if you do`` form above — the two regexes don't overlap.
        inner = _parse_effect(m.group(1))
        if inner is None:
            return None
        return OptionalSacArtifactForEffect(inner_effect=inner)
    m = _OPTIONAL_DISCARD_THEN_DRAW.match(phrase)
    if m is not None:
        # ``you may discard a card. If you do, draw N cards.`` →
        # composes the generic ``IfYouDo(MayDo(DiscardSelf(1)),
        # DrawCards(N))`` shape directly. Replaces the former
        # ``OptionalDiscardCardThenDrawN`` specialty class — the
        # combinators are general enough that we don't need a
        # discard-specific wrapper.
        token = m.group(1)
        amount = 1 if token == "a" else _parse_amount(token)
        return IfYouDo(
            do_action=MayDo(DiscardSelf(amount=1)),
            then_action=DrawCards(amount=amount),
        )
    m = _OPTIONAL_DISCARD_HAND_THEN_DRAW.match(phrase)
    if m is not None:
        # ``you may discard your hand. If you do, draw N cards`` —
        # Vaultguard Trooper's tapped-creatures-gated end-step trigger
        # body. Distinct from the single-card discard above: discards
        # the WHOLE hand, not a single chosen card. Tried AFTER the
        # ``discard a card`` regex above because the two surface forms
        # are disjoint (``a card`` vs ``your hand`` are non-overlapping
        # tails) but ordering ``discard a card`` first matches the
        # convention of bare-shape-before-variant that the rest of
        # this dispatcher follows.
        token = m.group(1)
        amount = 1 if token == "a" else _parse_amount(token)
        return DiscardHandThenDrawN(amount=amount)
    m = _OPTIONAL_PUT_PERMANENT_FROM_HAND_ONTO_BATTLEFIELD.match(phrase)
    if m is not None:
        # ``you may put a (creature|land) card from your hand onto the
        # battlefield`` — Sledge-Class Seedship's attack-trigger body
        # (creature) and Burgeoning's opponent-plays-land trigger body
        # (land). The captured group drives the engine-class dispatch.
        permanent_type = m.group(1)
        if permanent_type == "creature":
            return OptionalPutCreatureFromHandOntoBattlefield()
        if permanent_type == "land":
            return OptionalPutLandFromHandOntoBattlefield()
        # The regex's alternation is closed; this branch is unreachable
        # but keeps the dispatch explicit if the regex grows.
        return None
    m = _OPTIONAL_SAC_ANOTHER_FOR_PUMP_SELF_UEOT.match(phrase)
    if m is not None:
        # More-specific shape (self-pump consequence) — produces the
        # specialized ``OptionalSacAnotherForPumpSelfUEOT`` engine
        # class. Tried BEFORE the general optional-sac regex below.
        return OptionalSacAnotherForPumpSelfUEOT(
            power_bonus=int(m.group(1)),
            toughness_bonus=int(m.group(2)),
        )
    m = _OPTIONAL_SAC_LAND_OR_LANDER_FOR_BASIC_LAND_RAMP.match(phrase)
    if m is not None:
        # ``you may sacrifice a land or Lander. If you do, search your
        # library for up to N basic land cards, put them onto the
        # battlefield tapped, then shuffle.`` — Larval Scoutlander's
        # ETB body. Tried BEFORE the generic
        # ``_OPTIONAL_SAC_ANOTHER_FOR_EFFECT`` so the specialized
        # land-or-Lander shape wins; the generic regex's
        # ``creature or artifact`` subject wouldn't match this anyway,
        # but the more-specific-first ordering is the convention here.
        # Maps to ``OptionalSacLandOrLanderForBasicLandRamp(land_count=
        # N)`` which already implements the optional sac + tutor-tapped
        # sequence as a single combinator.
        return OptionalSacLandOrLanderForBasicLandRamp(
            land_count=_parse_amount(m.group(1)),
        )
    m = _OPTIONAL_SAC_ANOTHER_FOR_EFFECT.match(phrase)
    if m is not None:
        # Generic optional-sac wrapper. The inner consequence is parsed
        # recursively via the unified grammar; if the inner doesn't
        # parse, the whole line rejects so the wrapping card stays
        # unclaimed rather than silently dropping the consequence.
        inner = _parse_effect(m.group(1))
        if inner is None:
            return None
        return OptionalSacAnotherForEffect(inner_effect=inner)
    if _LOCK_OR_UNLOCK_DOOR_TARGET_ROOM_YOU_CONTROL.match(phrase) is not None:
        # ``lock or unlock a door of target Room you control`` — Keys to
        # the House's sorcery-speed activation body. Maps to the generic
        # ``LockOrUnlockDoorOfTargetRoomYouControl`` engine class which
        # picks a Room target the controller controls and toggles one
        # of its doors via ``choose_mode``. The ``Activate only as a
        # sorcery.`` rider on the printed activation is consumed at the
        # activated-ability layer (``sorcery_speed=True``), not in the
        # effect body.
        return LockOrUnlockDoorOfTargetRoomYouControl()
    # Planeswalker-bucket extension recognizers, then the card-flow
    # grammar (targeted draw, tutors, hand disruption, look-at-top
    # selection, wheels, mass zone moves) — both tried after every
    # recognizer above, so no phrase an older recognizer claims changes
    # meaning. Where the two overlap (Brainstorm's put-back, "untap up to
    # N lands", tuck-into-library, look-at-top-may-reveal) the extension
    # recognizer, which landed first, keeps the phrase.
    from mtgz_ir.cards.parser.effects_ext import parse_ext_effect

    ext = parse_ext_effect(phrase)
    if ext is not None:
        return ext
    from mtgz_ir.cards.parser.card_flow_grammar import parse_card_flow_effect

    card_flow = parse_card_flow_effect(phrase)
    if card_flow is not None:
        return card_flow
    # Artifact-bucket last-resort shapes ("you may X. If you do, Y").
    from mtgz_ir.cards.parser.artifact_effects import (
        _parse_artifact_effect_fallback,
    )
    artifact = _parse_artifact_effect_fallback(phrase)
    if artifact is not None:
        return artifact
    # Cube creatures (R / multicolor / colorless) bucket shapes -- last.
    from mtgz_ir.cards.parser.creature_rmc_grammar import parse_rmc_effect

    rmc = parse_rmc_effect(phrase)
    if rmc is not None:
        return rmc
    # U/B-creature bucket grammar (amass, ...).
    from mtgz_ir.cards.parser.cube_creatures_ub import parse_ub_effect

    ub = parse_ub_effect(phrase)
    if ub is not None:
        return ub
    # The source as the printed subject of its own damage clause
    # ("this creature deals 1 damage to any target"): retry through the
    # subject-less ``deals ...`` grammar spells use once their name is
    # stripped. Last resort, so every literal that spells out the
    # subject keeps its match.
    if phrase.startswith("this creature deals "):
        return _parse_effect(phrase[len("this creature "):])
    return None


def _normalize_creature_effect_phrase(phrase: str) -> str:
    """Strip the ``it`` source-subject pronoun if present, leaving the
    normalized phrase ready for ``_parse_effect``."""
    phrase = phrase.strip()
    if phrase.startswith("it "):
        phrase = phrase[len("it "):]
    return phrase


def _normalize_spell_effect_phrase(phrase: str, name: str) -> str:
    """Strip the ``<Name>`` source-subject prefix (if present), trailing
    period, leading ``Then `` connective, and lowercase the first letter
    — leaving the normalized phrase ready for ``_parse_effect``.

    The leading ``Then `` strip handles split sentences produced by
    ``_split_sentences`` in ``spell_builder`` — Scryfall sometimes
    packs two effect sentences on one oracle line separated by
    ``". Then <next-effect>."`` (e.g. Scrounge for Eternity's
    ``"… to the battlefield. Then create a Lander token."``).
    After ``_split_sentences`` carves the boundary, the second piece
    begins with ``"Then "`` which is pure sequencing noise; stripping
    it lets the remaining phrase reach the unified grammar unchanged.

    The ``When you do, <Name> `` internal name-stripping handles
    multi-sentence compound primitives like Lithobraking's
    ``"Then you may sacrifice an artifact. When you do, Lithobraking
    deals 2 damage to each creature."`` — after the ``Then `` outer
    strip, the normalized phrase is
    ``"you may sacrifice an artifact. When you do, Lithobraking deals
    …"``; the name occurs as the SUBJECT of the consequent clause, not
    at the start of the whole phrase. Stripping ``<Name> `` after
    ``"When you do, "`` ensures the inner effect tail
    (``"deals 2 damage to each creature"``) reaches ``_parse_effect``
    in its normalized form.
    """
    phrase = phrase.strip()
    if phrase.endswith("."):
        phrase = phrase[:-1]
    # Strip leading ``Then `` / ``then `` connective — sequencing marker
    # that carries no effect semantics (Scrounge for Eternity's second
    # sentence ``"Then create a Lander token."`` after sentence split).
    if phrase.startswith("Then "):
        phrase = phrase[len("Then "):]
    elif phrase.startswith("then "):
        phrase = phrase[len("then "):]
    prefix = name + " "
    if phrase.startswith(prefix):
        phrase = phrase[len(prefix):]
    # Strip ``<Name> `` when it appears as the subject after a
    # ``". When you do, "`` or ``". If you do, "`` consequent marker.
    # This handles multi-sentence compound primitives (e.g. Lithobraking)
    # where the card name is the grammatical subject of the second
    # sentence's consequent but carries no semantic information that
    # ``_parse_effect`` needs (the source is always the resolving spell).
    for consequent_prefix in (". When you do, ", ". If you do, "):
        marker = consequent_prefix + prefix
        replacement = consequent_prefix
        phrase = phrase.replace(marker, replacement)
    if phrase and phrase[0].isupper():
        phrase = phrase[0].lower() + phrase[1:]
    # ``Exile <Name>.`` — the spell exiling itself as part of its own
    # resolution (Time Spiral). The name is the object, not the
    # subject, so the prefix strip above doesn't apply; rewrite it to
    # the name-agnostic ``exile this spell`` surface.
    if phrase == "exile " + name:
        phrase = "exile this spell"
    # ``Shuffle <Name> into its owner's library.`` -- the same, the last
    # instruction of Green Sun's Zenith.
    if phrase == f"shuffle {name} into its owner's library":
        phrase = "shuffle this spell into its owner's library"
    return phrase


def _parse_effect_phrase(phrase: str) -> Effect | None:
    """Parse a creature-triggered-ability effect phrase. Surface form
    is ``it deals N damage…`` / ``draw a card`` / etc. Delegates to
    the unified ``_parse_effect`` after stripping the ``it`` prefix."""
    return _parse_effect(_normalize_creature_effect_phrase(phrase))


def _parse_spell_effect_phrase(phrase: str, name: str) -> Effect | None:
    """Parse a spell's full-sentence effect text.

    ``phrase`` is one sentence (period included) of oracle text. ``name``
    is the card's own name and is stripped if it appears as the subject.
    Delegates to the unified ``_parse_effect`` after normalization, so
    every effect available to creature triggered abilities is also
    available to spells (and vice versa).

    The compositional grammar is disabled for the Rust-parity-pinned
    legacy spells (see ``precise_grammar.legacy_parity_pinned``).
    """
    from mtgz_ir.cards.parser.precise_grammar import legacy_parity_pinned

    with legacy_parity_pinned(name):
        return _parse_effect(_normalize_spell_effect_phrase(phrase, name))
