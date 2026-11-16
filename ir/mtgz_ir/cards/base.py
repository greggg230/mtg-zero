"""Card printings, mana cost parsing, basic lands, and the Stage-1 synthetic pool.

Real Edge of Eternities cards are sitting in `data/edge_of_eternities.json` but
none are playable yet — they all require keywords/triggers that Stage 1 doesn't
support. Stage 1 plays with invented vanilla creatures instead.
"""

from __future__ import annotations

import random
import re
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from mtgz_ir.effects.conditions import Condition

from mtgz_ir.keyword_vocab import KeywordSpelling, validate_keywords
from mtgz_ir.effects.deck_conditions import Companion  # noqa: F401  (CardPrinting.companion)
from mtgz_ir.effects import (
    ActivatedAbility,
    ActivationCost,
    ConditionalIfCastFromHand,
    ConditionalIfOpponentHasMoreLands,
    ConditionalIfSourcePowerAtLeast,
    ConditionalIfVoid,
    ConditionalIfYouControlAtLeast,
    ConditionalIfYouGainedLifeAtLeast,
    ControlTargetOpponentNextTurn,
    DealDamageToControllerAndAddMana,
    BecomeLandCreatureUEOT,
    BecomesCopyOfTargetLand,
    ChangeControllerToCombatDamageSource,
    CopyTargetInstantOrSorceryYouControl,
    CreateTokenCopyOfTargetCreatureYouControl,
    DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters,
    DestroyTargetLand,
    ExileAllGraveyards,
    ExileTargetAttackingCreatureUntilEndStep,
    GrantLifeToEachOpponent,
    GrantUnblockableUEOT,
    LockOrUnlockDoorOfTargetRoomYouControl,
    PutBasicLandFromHandOntoBattlefieldTapped,
    ReturnTargetLandFromGraveyardToHand,
    SacrificeNLandsYouControl,
    UntapTargetLand,
    Sequence,
    AddManaToPool,
    AddNManaOfChosenColor,
    ConditionalOnTappedCreatures,
    CounterTargetSpell,
    CreateToken,
    DealDamage,
    DealDamageAnyTarget,
    DealDamageEachOpponent,
    DealDamageEqualToSourcePowerAnyTarget,
    DefendingPlayerLosesHalfLifeRoundedUp,
    DealDamageToOpponentCreature,
    DealDamageToOpponentCreatureByArtifactCount,
    DealDamageToOpponentCreatureByCardsInHand,
    DealDamageToTargetCreature,
    DealDamageToTargetOpponent,
    DealDamageToTargetTappedCreature,
    DestroyAllCreatures,
    DestroyEnchantedPermanent,
    DestroyTargetArtifact,
    DestroyTargetCreature,
    DestroyTargetNonlandPermanent,
    DestroyUpToOneTargetNonlandPermanent,
    ExileOpponentsNonlandPermanentUntilLeaves,
    EnchantedCreatureFightsTargetEffect,
    EnduringDiesReturnAsEnchantmentEffect,
    KavaronHarrierAttackEffect,
    PainForAllDamageEchoEffect,
    PainForAllETBEffect,
    PerigeeBeckonerDiesReturnTappedEffect,
    PossibilityTechnicianETBEffect,
    TerritorialBruntarLandfallEffect,
    TractorBeamETBEffect,
    ReanimateOneExiledWithSourceWithCounters,
    SacrificeSelfPreservingExileLink,
    SacrificeSelfUnlessTapPermanent,
    SacrificeSelfUnlessVoidActive,
    RovingActuatorETBEffect,
    PutStunCounterOnTappedCreature,
    TapAndStunUpToOneTargetCreature,
    TapTargetOpponentPermanentAndStunIt,
    ExileTargetCreatureUntilSourceLeaves,
    SeedshipImpactEffect,
    MovePlusOneCounterBetweenPermanentsYouControl,
    MoveSelfCountersToTargetCreatureYouControl,
    RegenerateTargetCreature,
    UntapTargetOtherPermanent,
    PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact,
    PutChargeCountersOnSelf,
    PutChargeCountersOnTargetSpacecraftOrPlanetYouControl,
    PutChargeCountersOnTargetSpacecraftYouControl,
    DiscardSelf,
    DrawCards,
    DrawCardsEqualToResolvingCombatDamage,
    DrawCardsEqualToResolvingCounterAmount,
    EachOpponentDiscards,
    EachOpponentExilesCardFromHandWithCastPermission,
    EachOpponentExilesOwnCreature,
    EachOpponentMillsCards,
    EachOpponentSacrificesCreature,
    EachOpponentSacsOrDiscardsOrTakesSourcePowerDamage,
    EachOpponentLosesLife,
    EachPlayerDraws,
    Effect,
    ExileEachOpponentsGraveyard,
    ExileTargetCardFromGraveyard,
    ExileTargetCreature,
    ExileTargetCreatureWithPowerOrToughnessAtMost,
    GainControlOfTargetSliverPermanently,
    GainLife,
    IfGainedLifeThisTurnUntapSelf,
    LoseLife,
    PutCardFromGraveyardOnBottomOfLibrary,
    PumpCreaturesOpponentsControlUntilEOT,
    PumpCreaturesYouControlUntilEOT,
    PutPlusOneCounterOnOpponentsCreature,
    PumpSelfUntilEOT,
    PumpSelfXPowerUEOT,
    PhaseOutEquippedCreature,
    PreventAllCombatDamageThisTurn,
    PumpTargetArtifactOrCreatureUntilEOT,
    PumpTargetCreatureUntilEOT,
    PutPlusOneCounterOnCreatureYouControl,
    PutPlusOneCounterOnEachCreatureYouControl,
    PutPlusOneCounterOnEachOfUpToNTargetCreatures,
    PutPlusOneCounterOnSelf,
    PutPlusOneCounterOnTargetCreature,
    PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl,
    PutPoisonCountersOnTargetPlayer,
    ReturnAllCreaturesFromYourGraveyardToBattlefield,
    ReturnCreatureFromYourGraveyardToBattlefield,
    ReturnCreatureFromYourGraveyardToHand,
    ReturnTargetCreatureToOwnersHand,
    ReturnTargetPermanentYouControlToOwnersHand,
    ReturnUpToNCreaturesFromYourGraveyardToHand,
    ReturnUpToNNonlandPermanentCardsFromGraveyardToHand,
    ReturnUpToNSmallCreaturesFromGraveyardWithCounters,
    ReturnUpToNTargetCreaturesToOwnersHands,
    DestroyTargetNonlandPermanentWithLanderForController,
    DestroyTargetArtifactOrTappedCreature,
    TapAnotherTargetCreature,
    TapTargetArtifactOrCreature,
    DepressurizeEffect,
    DestroyTargetArtifactEnchantmentOrFlyingCreature,
    CutPropulsionEffect,
    DiplomaticRelationsEffect,
    DealDamageToTargetCreatureSpacecraftBoosted,
    DealDamageToTargetCreatureWithExcessLanderRider,
    ExileAllArtifactsWithManaValueAtMost,
    ExileAllMatchingUntilSourceLeaves,
    CreateNTokensFromLastExileRecall,
    MoveEachExiledWithSourceToOwnersGraveyard,
    PutEachCreatureCardExiledWithSourceOntoBattlefieldUnderYourControl,
    SacrificeSelf,
    AnticausalVestigeLeavesEffect,
    AddManaToPool,
    AddOneManaFromControlledLandsProductions,
    AddOneManaOfChosenPermanentColor,
    AlpharaelDreamingAcolyteDiscardEffect,
    ArabellaAttackEffect,
    HouseCartographerSurvivalEffect,
    AstelliReclaimerETBEffect,
    AtomicMicrosizerAttackEffect,
    AttachToTargetCreatureYouControl,
    EquipToTargetCreatureYouControl,
    BiosynthicBurstEffect,
    BoilerbilgesRipperETBEffect,
    CreateLanderAndScheduleSacrifice,
    CreateTokenAndAttachSelf,
    CloseEncounterEffect,
    CommuneWithEvilEffect,
    ConsultTheStarChartsEffect,
    CreateTokenPerMulticoloredPermanent,
    DrawCardPerMulticoloredPermanent,
    DyadrineAttackEffect,
    EachOpponentDiscardsOrLosesLife,
    FamishedWorldsireETBEffect,
    FanaticOfTheHarrowingETBEffect,
    PutPlusOneCountersOnEachCreatureYouControlEqualToSourcePower,
    IfYouDo,
    MayDo,
    OptionalSacLandOrLanderForBasicLandRamp,
    OptionalPutCreatureFromHandOntoBattlefield,
    OptionalPutLandFromHandOntoBattlefield,
    CreateXTokensOfTargetPermanentWithHasteEndStepSac,
    DealNDamageToEachCreature,
    DestroyTargetNoncreaturePermanent,
    DiscardHandThenDrawN,
    DoubleTargetCreaturesPowerUEOT,
    PumpTargetOtherCreatureByXOverXWhereXIsSourcePower,
    TargetOpponentLosesLife,
    TargetOpponentLosesNYouGainN,
    TargetPlayerLosesLife,
    ThreatenTargetCreatureUEOT,
    ExileAllCreaturesAndSpacecraft,
    OptionalSacArtifactForEffect,
    PutLoyaltyCounterOnSelf,
    TargetArtifactBecomesCreatureUEOT,
    ReturnTargetArtifactCardFromYourGraveyardToHand,
    SetBasePTUntilEOT,
    SetBasePTConditionalUntilEOT,
    SetLifeToHalfStartingRoundedUp,
    ShuffleZonesIntoLibrary,
    SwitchSourcePowerToughnessUEOT,
    CoordinatedClobberingEffect,
    DealSelfMVDamageToEachCreature,
    ExorciseEffect,
    SingularityRuptureEffect,
    SystemsOverrideEffect,
    TerrapactIntimidatorETBEffect,
    TerminalVelocityEffect,
    ZeroPointBalladEffect,
    ModalChooseOne,
    FocusFireEffect,
    OptionalManaCostThenEffect,
    OptionalSacAnotherForEffect,
    OptionalSacAnotherForPumpSelfUEOT,
    OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    OwnerPutsTargetWarpedExileCardOnBottomOfLibrary,
    SelfMill,
    MillTargetPlayer,
    SearchLibraryForBasicLandTapped,
    SearchLibraryForCard,
    SearchLibraryForEquipmentToBattlefield,
    SearchLibraryForGreenCreatureWithMVAtMostX,
    TakeExtraTurnAfterThisOne,
    SelfcraftMechanETBEffect,
    TannukEnsignLandfallEffect,
    Sequence,
    StaticAbility,
    StaticScope,
    LookAtTopNRevealOrCounter,
    LookTopNPutOneInHandRestToGraveyard,
    ManifestDread,
    ManifestDreadThenPutPlusOneCountersOnIt,
    MutinousMassacreEffect,
    Surveil,
    TargetOpponentDiscards,
    TargetOpponentDiscardsCasterChoice,
    TriggeredAbility,
)
from mtgz_ir.effects.pt_formulas import PTFormula
from mtgz_ir.effects.vocab import EnchantKind, XCounterMode, ZoneName, coerce


COLORS: tuple[str, ...] = ("W", "U", "B", "R", "G")

# All valid two-color hybrid pairs (both orderings so look-up is symmetric).
# Standard ten allied/enemy hybrid pairs plus the Phyrexian-hybrid pairs
# ({W/P}, {U/P}, etc.) are handled separately in parse_mana_cost.
_HYBRID_PAIRS: frozenset[frozenset[str]] = frozenset(
    frozenset(pair)
    for pair in [
        ("W", "U"), ("U", "B"), ("B", "R"), ("R", "G"), ("G", "W"),
        ("W", "B"), ("U", "R"), ("B", "G"), ("R", "W"), ("G", "U"),
    ]
)


@dataclass(frozen=True)
class ManaCost:
    generic: int = 0
    colored: tuple[tuple[str, int], ...] = ()
    # Number of {X} symbols in the printed cost. The actual generic mana
    # paid at cast time is ``generic + x_pips * X`` where X is the
    # caster's chosen value. Stored separately so a card's "templated"
    # cost stays distinct from its paid cost.
    x_pips: int = 0
    # Hybrid mana symbols, e.g. {U/R}, stored as a tuple of (color_a,
    # color_b) pairs in canonical alphabetical order. Each hybrid symbol
    # has a mana value of 1 and can be paid with either color (or
    # generically — engine simplification: treated as generic-1 in
    # payment so the existing payment loop needs no changes). Both colors
    # of each hybrid symbol contribute to the card's color identity via
    # the ``colors`` property.
    hybrid: tuple[tuple[str, str], ...] = ()
    # Phyrexian mana symbols ({U/P}), one color letter per symbol
    # (CR 107.4f): each can be paid with one mana of its color or with
    # 2 life. MV 1 each; the color counts toward the card's colors.
    # The cast pipeline settles each pip (life vs. mana) before paying
    # (``actions._settle_phyrexian_pips``); ``can_pay_cost`` checks
    # every life/mana split for affordability.
    phyrexian: tuple[str, ...] = ()

    @property
    def total(self) -> int:
        return (
            self.generic + sum(n for _, n in self.colored)
            + len(self.hybrid) + len(self.phyrexian)
        )

    @property
    def colors(self) -> frozenset[str]:
        hybrid_colors: frozenset[str] = frozenset(
            c for pair in self.hybrid for c in pair
        )
        return (
            frozenset(c for c, _ in self.colored if c not in ("S", "C")) | hybrid_colors
            | frozenset(self.phyrexian)
        )

    def with_colored(self, extra: "tuple[str, ...] | list[str]") -> "ManaCost":
        """Return a copy with one more colored pip per entry of
        ``extra`` and no Phyrexian pips (they've been settled)."""
        merged: dict[str, int] = dict(self.colored)
        for c in extra:
            merged[c] = merged.get(c, 0) + 1
        return ManaCost(
            generic=self.generic,
            colored=tuple(sorted(merged.items())),
            x_pips=self.x_pips,
            hybrid=self.hybrid,
        )

    def reduced(self, amount: int) -> "ManaCost":
        """Return a new ManaCost with ``amount`` generic mana removed
        (minimum 0; colored costs unchanged — MTG rule 601.2f). The
        ``x_pips`` template is preserved: cost reductions only touch
        the generic side, so an X-spell stays an X-spell after a
        discount (CR 601.2f reduces generic mana, leaving {X} symbols
        intact for the controller to choose later)."""
        if amount <= 0:
            return self
        return ManaCost(
            generic=max(0, self.generic - amount),
            colored=self.colored,
            x_pips=self.x_pips,
            hybrid=self.hybrid,
            phyrexian=self.phyrexian,
        )


@dataclass(frozen=True)
class CostReduction:
    """A rule that reduces the generic-mana portion of a spell's cost.

    ``condition`` is a structural :class:`mtgz_ir.effects.conditions.
    Condition` evaluated against game state at cast time. ``amount`` is
    the unit generic reduction; the engine multiplies it by
    ``condition.multiplier(state, caster_id, chosen_targets)`` so
    variable-amount conditions (Fungal Colossus's
    ``PerDifferentlyNamedLand``, Affinity-style ``PerControlled``) scale
    automatically without a separate flag.
    """

    amount: int
    condition: "Condition"


@dataclass(frozen=True)
class ExternalCostReduction:
    """Static cost-reduction granted by a permanent on the battlefield
    to OTHER spells the controller casts. Lives on a permanent's
    printing; ``apply_cost_reductions`` scans the caster's battlefield
    for these whenever they cast a spell.

    Distinct from ``CostReduction`` (which is baked into the spell
    being cast and reduces ITS own cost). Externals come from a
    different permanent than the spell, and apply to spells they
    don't share an id with.

    ``condition`` is a structural :class:`mtgz_ir.effects.conditions.
    Condition` — the same grammar the spell-local ``CostReduction``
    uses. Variable-amount externals (Sami, Wildcat Captain's "Spells
    you cast have affinity for artifacts" → ``PerControlled(IsArtifact
    ())``) scale via the condition's ``multiplier``.
    """

    amount: int
    condition: "Condition"


def parse_mana_cost(s: str, colorless_pips: bool = False) -> ManaCost:
    """``colorless_pips``: carry ``{C}`` as the pseudo-color ``"C"`` in
    ``colored`` (CR 107.4b: payable only with colorless mana; payment:
    ``actions._coin_matches_color``) instead of folding it into
    ``generic`` (the lossy default most builders still use, mtg-zero
    docs/ir_defects.md 5; Mirrorpool's costs opt in, entry 67)."""
    if not s:
        return ManaCost()
    generic = 0
    colored: dict[str, int] = {}
    x_pips = 0
    hybrid_list: list[tuple[str, str]] = []
    phyrexian_list: list[str] = []
    for sym in re.findall(r"\{([^}]+)\}", s):
        if sym.isdigit():
            generic += int(sym)
        elif sym in COLORS:
            colored[sym] = colored.get(sym, 0) + 1
        elif sym == "C" and colorless_pips:
            colored["C"] = colored.get("C", 0) + 1
        elif sym == "C":
            generic += 1
        elif sym == "S":
            # Snow mana {S} (CR 107.4h): one mana from a snow source.
            # Carried as the pseudo-color "S" in ``colored`` (payment:
            # ``actions._coin_matches_color`` / the snow-land branch);
            # it is not a color (``ManaCost.colors`` skips it).
            colored["S"] = colored.get("S", 0) + 1
        elif sym == "X":
            x_pips += 1
        elif "/" in sym:
            # Hybrid mana symbol: {W/U}, {U/R}, {B/G}, etc.
            # Also handles Phyrexian hybrid {W/P}, {U/P}, etc.
            parts = sym.split("/")
            if len(parts) == 2:
                a, b = parts[0], parts[1]
                if b == "P" and a in COLORS:
                    # Phyrexian mana {W/P} etc. (CR 107.4f): MV 1, paid
                    # with one mana of the color or 2 life.
                    phyrexian_list.append(a)
                elif a in COLORS and b in COLORS:
                    # Standard two-color hybrid: store canonical (alpha) order.
                    pair = tuple(sorted((a, b)))
                    hybrid_list.append((pair[0], pair[1]))
                else:
                    raise ValueError(f"unsupported mana symbol: {{{sym}}}")
            else:
                raise ValueError(f"unsupported mana symbol: {{{sym}}}")
        else:
            raise ValueError(f"unsupported mana symbol: {{{sym}}}")
    return ManaCost(
        generic=generic,
        colored=tuple(sorted(colored.items())),
        x_pips=x_pips,
        hybrid=tuple(hybrid_list),
        phyrexian=tuple(phyrexian_list),
    )


# Card types that make a card a permanent card (``CardPrinting.
# is_permanent``; a card-type tuple's form for effective types).
PERMANENT_CARD_TYPES: tuple[str, ...] = (
    "Creature", "Artifact", "Land", "Enchantment", "Planeswalker",
)


def is_permanent_card_types(card_types: "tuple[str, ...]") -> bool:
    """Whether a card with these card types is a permanent card."""
    for t in PERMANENT_CARD_TYPES:
        if t in card_types:
            return True
    return False


@dataclass(frozen=True)
class HandCardFilter:
    """Predicate for which cards in a player's hand a hand-ability
    grant applies to.

    A grant's filter is a disjunction of typed predicates: each entry
    is a ``(card_type, optional_color)`` pair. A hand card matches the
    filter if it matches at least one entry.

    Tannuk, Steadfast Second's "Artifact cards and red creature cards"
    encodes as::

        HandCardFilter((("Artifact", None), ("Creature", "R")))

    — any artifact card OR any red creature card. The ``None`` color
    means "no color restriction" (matches by type only).

    The filter is structural rather than free-text so the engine can
    evaluate it at cast time. Adding a new card with a similar shape
    (e.g. "blue instant cards in your hand have flashback {U}") is
    one new ``HandCardFilter`` literal — no engine changes.
    """

    predicates: tuple[tuple[str, "str | None"], ...] = ()

    def matches(
        self, printing: "CardPrinting", *,
        colors: "frozenset[str] | None" = None,
    ) -> bool:
        """``colors`` overrides the printed colors with a game object's
        effective ones (:meth:`matches_card`)."""
        for card_type, color in self.predicates:
            if card_type not in printing.card_types:
                continue
            if color is None:
                return True
            if color in (
                printing.mana_cost.colors if colors is None else colors
            ):
                return True
        return False

    def matches_card(self, state, card) -> bool:
        """:meth:`matches` against a card in hand: its color is its
        effective color (CR 105.2 / layer 5 -- Mycosynth Lattice makes a
        red card in hand colorless); a card in hand has its printed card
        types."""
        from mtgz_ir.game import effective_colors

        return self.matches(card.printing, colors=effective_colors(state, card))


@dataclass(frozen=True)
class HandAbilityGrant:
    """A static effect granting an alternative casting ability to
    cards in the controller's hand that match a filter.

    Currently models granted-warp (Tannuk, Steadfast Second).
    Extension points (kept as tagged optional fields rather than a
    union so adding a new granted ability is a single new field
    rather than a new type):

    - ``granted_warp_cost``: cards in hand matching the filter become
      warp-castable for this cost (paid like a printed warp cost,
      including the end-step exile after resolution).

    Future granted abilities (flash, flashback, suspend, cycling)
    each become one optional field; ``legal.py`` adds an enumeration
    branch keyed on the field; ``actions.py`` reads the granted
    parameter at cast time. No new type per ability.
    """

    card_filter: HandCardFilter
    granted_warp_cost: "ManaCost | None" = None
    granted_warp_cost_str: str = ""


def _validate_static_abilities(owner: str, statics: object) -> None:
    """Enforce the declared ``tuple[StaticAbility, ...]`` shape.

    Every consumer of a printing's ``static_abilities`` iterates it and
    then reads ``StaticAbility`` members off each entry
    (``applies_to`` / ``granted_keywords`` / ``continuous_effect`` /
    ``replacement_effect``). Nothing used to enforce the shape, so a
    builder that handed the slot a BARE ability-shaped object — a lone
    ``ContinuousModifier`` (the payload that normally rides *inside* a
    ``StaticAbility``), a lone ``StaticAbility``, or a list — produced a
    printing that looked fine until some unrelated layered read walked
    the battlefield. The failure then surfaced as

        TypeError: 'ContinuousModifier' object is not iterable

    raised from :func:`mtgz_ir.effects.modifiers.
    walk_continuous_modifiers` / :func:`mtgz_ir.game._static_bonuses`
    — an opaque crash, arbitrarily far (in time and in stack) from the
    construction site that caused it, and one that drops a whole
    self-play game.

    Validating at construction turns that into a loud, located failure
    naming the card and the offending type. No coercion
    (``feedback_no_fallback_hybrids``): a bad shape is a bug in the
    builder, not something to silently wrap in a 1-tuple.
    """
    if not isinstance(statics, tuple):
        raise TypeError(
            f"{owner!r}: static_abilities must be a tuple of "
            f"StaticAbility, got {type(statics).__name__} "
            f"({statics!r}). A bare ContinuousModifier belongs in "
            f"StaticAbility(continuous_effect=...), not in the "
            f"static_abilities slot."
        )
    for index, ability in enumerate(statics):
        if not isinstance(ability, StaticAbility):
            raise TypeError(
                f"{owner!r}: static_abilities[{index}] must be a "
                f"StaticAbility, got {type(ability).__name__} "
                f"({ability!r})."
            )


@dataclass(frozen=True)
class RoomHalf:
    """One door/half of a Duskmourn Room card (CR 207.2g + 715).

    A Room is an enchantment with two halves; each has its own name,
    mana cost, and abilities. When a Room spell is cast, it's cast for
    one half's mana cost — that half enters unlocked, the other locked.
    The owner can later unlock the locked half at sorcery speed by
    paying its mana cost (see ``UnlockRoomHalf`` action).

    Locked halves' abilities are inert. The
    ``effective_triggered_abilities`` / ``effective_activated_abilities``
    iterators filter out abilities whose ``room_half`` name is not in
    the source's ``GameObject.unlocked_halves``. The static-ability
    iterator does the same.

    The ``triggered_abilities`` / ``static_abilities`` /
    ``activated_abilities`` here are also flattened onto the parent
    ``CardPrinting`` (with ``room_half`` set on each) so the existing
    ability iterators work without per-call-site special-casing.
    """

    name: str
    mana_cost_str: str
    mana_cost: ManaCost
    triggered_abilities: tuple["TriggeredAbility", ...] = ()
    static_abilities: tuple["StaticAbility", ...] = ()
    activated_abilities: tuple["ActivatedAbility", ...] = ()

    def __post_init__(self) -> None:
        _validate_static_abilities(
            f"RoomHalf {self.name}", self.static_abilities,
        )


@dataclass(frozen=True)
class CardPrinting:
    name: str
    mana_cost_str: str
    mana_cost: ManaCost
    type_line: str
    supertypes: tuple[str, ...]
    card_types: tuple[str, ...]
    subtypes: tuple[str, ...]
    power: int | None = None
    toughness: int | None = None
    produces: frozenset[str] = frozenset()
    keywords: frozenset[KeywordSpelling] = frozenset()
    triggered_abilities: tuple[TriggeredAbility, ...] = ()
    # For instants and sorceries: the effect that runs when the spell
    # resolves. After resolution the spell goes to its owner's graveyard.
    # None for creatures, lands, and spells whose effect is implicit (rare).
    spell_effect: Effect | None = None
    # Edge of Eternities "warp" alt-cost. If set, the card may be cast from
    # hand for this cost instead of its normal cost; when cast via warp it's
    # exiled at the next end step and can then be cast from exile at its
    # normal cost on a later turn.
    warp_cost: ManaCost | None = None
    warp_cost_str: str = ""
    # Additional life cost paid alongside the warp mana cost. When > 0,
    # warp-casting deducts this life from the caster on top of paying
    # warp_cost. Matches Timeline Culler's "Warp—{B}, Pay 2 life."
    # legal_actions checks the controller has > life loss available.
    warp_additional_life_cost: int = 0
    # When True, the card is also castable via its warp ability from
    # the graveyard, not just the hand. Matches Timeline Culler's
    # "You may cast this card from your graveyard using its warp ability."
    warp_castable_from_graveyard: bool = False
    # Duskmourn "Impending N — {alt-cost}" alt-cost. If set, the card
    # may be cast from hand for ``impending_cost`` instead of its
    # normal cost; when cast that way, it enters with
    # ``impending_count`` time counters and is suppressed-as-non-
    # creature (``impending_active=True``) until the last counter is
    # removed at the controller's end step. The Overlord cycle
    # (Duskmourn) is the printing-source: each Overlord is an
    # Enchantment Creature with both a normal cost and an Impending
    # cost. When cast for the Impending cost the permanent's printed
    # creature type is suppressed via ``is_creature_now`` reading
    # ``GameObject.impending_active`` — the printing's
    # ``card_types`` still includes "Creature" but
    # ``is_creature_now`` returns False so combat / target /
    # creature-only triggers don't fire. The end-step turn-based
    # action ``run_impending_decrement_turn_based_action`` removes
    # one time counter from each impending-active permanent the
    # active player controls; when the count hits 0 it clears
    # ``impending_active`` and the permanent becomes a creature.
    impending_cost: ManaCost | None = None
    impending_cost_str: str = ""
    impending_count: int = 0
    # "Kicker {cost}" — an optional additional mana cost the caster may
    # pay while casting. If paid, the StackItem is marked kicked and
    # the resolving effect can branch on state.resolving_kicked.
    kicker_cost: ManaCost | None = None
    kicker_cost_str: str = ""
    # Planeswalker starting loyalty. None on non-planeswalkers. When
    # the planeswalker enters the battlefield it starts with this many
    # loyalty counters on it (GameObject.loyalty_counters).
    loyalty: int | None = None
    # Devour land N: "As this creature enters, you may sacrifice any
    # number of lands. It enters with N times that many +1/+1 counters
    # on it." Non-zero enables the replacement in put_into_battlefield.
    # 0 = no devour. Matches Famished Worldsire (N=3).
    devour_land_multiplier: int = 0
    # EoE Spacecraft Station mechanic. For a Spacecraft card: once it has
    # station_threshold or more charge counters on it, it becomes an artifact
    # creature with its printed P/T and gains stationed_keywords. None on
    # non-Spacecraft cards.
    station_threshold: int | None = None
    stationed_keywords: frozenset[KeywordSpelling] = frozenset()
    # Saga (Enchantment — Saga). When > 0, this card is a Saga
    # with the given final chapter number. Sagas would get a lore
    # counter at the start of each precombat main phase as a
    # turn-based action, with chapter-numbered triggered
    # abilities firing when the lore counter advances (those
    # mechanics aren't modeled in scope). The state-based action
    # (CR 704.5r in audit numbering) sacrifices a saga whose
    # ``lore_counters >= chapter_count``. 0 = not a saga.
    chapter_count: int = 0
    # Ward {cost}: when this creature becomes the target of a spell or
    # ability an opponent controls, counter that spell/ability unless the
    # controller pays this cost. We simplify to "caster auto-pays if they
    # can; otherwise the spell is countered at cast time" — no player
    # choice, no pay-or-counter trigger on the stack.
    ward_cost: ManaCost | None = None
    ward_cost_str: str = ""
    # "Enchanted/equipped permanent has ward {cost}." Aura/Equipment grant.
    # When this printing is attached to another permanent, the enchanted
    # permanent gains ward {cost} on top of any printed ward. Combined by
    # game.effective_ward_cost and read by actions._apply_ward_check.
    # Hardlight Containment's "Enchanted permanent has ward {1}" rider.
    attached_grants_ward_cost: ManaCost | None = None
    attached_grants_ward_str: str = ""
    # "You may play an additional land on each of your turns." Non-zero
    # value contributes to the land-per-turn cap via legal_actions.
    # Icetill Explorer has this at 1.
    grants_additional_land_per_turn: int = 0
    # "You may play lands from your graveyard." When True on any
    # permanent the controller controls, legal_actions also lists
    # land cards in the controller's graveyard as PlayLand actions
    # (still subject to the land-per-turn cap). Matches Icetill
    # Explorer. _play_land removes the card from the graveyard.
    grants_play_lands_from_graveyard: bool = False
    # "Enchanted/equipped permanent doesn't untap during its
    # controller's untap step." Tractor Beam's don't-untap clause.
    # When True on an aura/equipment attached to a permanent, the
    # untap step skips that permanent.
    attached_prevents_untap: bool = False
    # "You control enchanted permanent." Tractor Beam's static
    # control-grant clause. When True on an aura, the moment it
    # enters the battlefield attached to a target,
    # ``apply_aura_static_grants_on_attach`` writes a layer-2
    # ``"control_change"`` entry to ``state.continuous_effects`` —
    # applied BEFORE any ETB triggers go on the stack, since static
    # abilities are continuous (not stack-based).
    # ``effective_controller`` derives the target's controller from
    # the registry (most-recent timestamp wins), and
    # ``move_off_battlefield`` drops the entry when the aura leaves
    # play. Supports nested swaps in CR-613 timestamp order.
    attached_grants_control: bool = False
    # Lignify-style aura flags. When set on an Aura's printing,
    # ``apply_aura_static_grants_on_attach`` writes corresponding
    # registry entries on attach so the layered registry models the
    # full Lignify effect (CR 613, layers 4 + 6 + 7b):
    #   - ``attached_sets_base_pt`` — layer 7 sublayer 1 set-base-PT,
    #     params ``(power, toughness)``. Overrides the enchanted
    #     permanent's base power and toughness while attached.
    #   - ``attached_grants_lose_all_abilities`` — layer 6
    #     ``"lose_all_abilities"``. Suppresses every printed and
    #     granted ability/keyword/trigger on the enchanted permanent.
    #   - ``attached_grants_subtype`` — layer 4 ``"add_subtype"``
    #     adding the named subtype to the enchanted permanent (e.g.
    #     "Treefolk" for Lignify).
    # All three live with ``expires_at=None`` so they end when the
    # source aura leaves play (``remove_continuous_effects_from_source(
    # only_static=True)`` drops them in ``move_off_battlefield``).
    attached_sets_base_pt: "tuple[int, int] | None" = None
    attached_grants_lose_all_abilities: bool = False
    # "Enchanted creature ... can't be blocked by more than one creature"
    # (Meltstrider's Resolve; CR 509.1b: a blocking restriction, the
    # opposite of menace).
    attached_cant_be_blocked_by_more_than_one: bool = False
    attached_grants_subtype: "str | None" = None
    # "Enchanted creature ... is a Toy artifact creature ... in addition to
    # its other types." (Unable to Scream): layer-4 ``add_type`` entries
    # on the enchanted permanent while the Aura is attached (``"Artifact"``
    # / ``"Creature"`` -- the addable card types, see
    # ``game.LAYER4_ADDABLE_CARD_TYPES``).
    attached_grants_card_types: tuple = ()
    # "As long as enchanted creature is face down, it can't be turned
    # face up." (Unable to Scream; ``effects.generic.roto_layers.
    # turning_face_up_prevented``).
    attached_prevents_turning_face_up: bool = False
    # "Enchanted <permanent> has "<activated ability>"" (Tarmogoyf Nest;
    # roto integration): layer 6 ``"grant_activated_ability"`` entries on
    # the enchanted permanent while the Aura is attached.
    attached_grants_activated_abilities: tuple = ()
    # Clone-style ETB replacement: "You may have this creature enter
    # as a copy of any creature on the battlefield." When True, the
    # spell-resolution path prompts the controller for a target
    # creature and (if chosen) snapshots ``original_printing`` and
    # mutates ``printing`` to the target's PRINTED characteristics
    # (CR 706.2 copiable values). A layer-1 ``"is_copy"`` registry
    # entry is also written with ``expires_at=None`` and
    # ``source_id=target_id=clone.id`` so the layered system records
    # the copy event; the entry is dropped when the clone leaves
    # play. If no legal target exists, or the controller declines,
    # the clone enters as itself (its printed self).
    enters_as_copy_of_creature: bool = False
    # "The first time you would create one or more tokens each turn,
    # you may instead create that many tokens that are copies of
    # enchanted permanent." Moonlit Meditation. Aura-only flag.
    replaces_first_tokens_with_copies: bool = False
    # "You may look at the top card of your library any time."
    # Mm'menon, the Right Hand. In simulation both players have perfect
    # information, so this flag is informational; it faithfully records
    # the printed oracle line without adding engine-invisible behavior.
    grants_look_at_top_of_library: bool = False
    # "You may cast artifact spells from the top of your library."
    # Mm'menon, the Right Hand. legal_actions offers a CastSpell for
    # the controller's top-of-library card when it's an artifact and
    # this flag is present on any permanent they control.
    grants_cast_artifact_from_top_of_library: bool = False
    # "The first spell each player casts during each of their turns
    # may be cast without paying its mana cost." Weftwalking. When any
    # permanent on the battlefield has this flag, legal_actions offers
    # a free-cost CastSpell variant (cast_for_free=True) to the active
    # player whenever they have not yet cast a spell this turn.
    # _cast_spell skips cost payment when cast_for_free is True.
    grants_first_spell_each_turn_free: bool = False
    # "Artifacts you control are Foods in addition to their other
    # types." Ragost, Deft Gastronaut. Consumed by
    # game.effective_subtypes to inject "Food" into artifact subtypes.
    grants_food_to_your_artifacts: bool = False
    # "Artifacts you control have '{2}, {T}, Sacrifice this artifact:
    # You gain 3 life.'" Ragost's companion ability-grant clause.
    # Consumed by game.effective_activated_abilities to append the
    # pre-built RAGOST_FOOD_GRANTED_ABILITY to artifact activations.
    grants_food_ability_to_your_artifacts: bool = False
    # "Artifacts you control have '{T}: Add {U}. Spend this mana only
    # to cast a spell from anywhere other than your hand.'" Mm'menon,
    # the Right Hand's static-grants-activated-ability clause.
    # Consumed by game.effective_activated_abilities to append the
    # pre-built MMNEMON_BLUE_MANA_GRANTED_ABILITY to artifact
    # activations.
    grants_blue_mana_ability_to_your_artifacts: bool = False
    # "X cards in your hand have <ability>." Static effects that grant
    # an alternative casting ability (warp today; flashback, suspend,
    # cycling, etc. as the pool grows) to cards in the controller's
    # hand matching a filter. Each ``HandAbilityGrant`` carries a
    # ``HandCardFilter`` (which hand cards are affected) plus the
    # granted ability's parameter (currently just ``granted_warp_cost``).
    #
    # Tannuk, Steadfast Second is the canonical example: "Artifact
    # cards and red creature cards in your hand have warp {2}{R}."
    # encodes as ``(HandAbilityGrant(HandCardFilter((("Artifact", None),
    # ("Creature", "R"))), granted_warp_cost=parse_mana_cost("{2}{R}"),
    # granted_warp_cost_str="{2}{R}"),)``.
    #
    # The legal-actions enumerator walks each battlefield permanent
    # the controller controls and offers warp-cast actions for matching
    # hand cards (see ``legal.py``); the cast pipeline reads the granted
    # cost from the matching grant via ``effective_warp_cost``.
    hand_ability_grants: tuple[HandAbilityGrant, ...] = ()
    # Equipment flag. When this equipment is attached to a creature,
    # that creature gains an activated ability to control the next
    # turn of a target opponent (The Dominion Bracelet). The full
    # ability is DOMINION_BRACELET_GRANTED_ABILITY.
    grants_control_opponent_ability_when_equipped: bool = False
    # Ward—Discard a card at random (non-mana ward). Alpharael, Stonechosen.
    # When True, the caster must randomly discard a card to pay the ward;
    # if their hand is empty, the spell is countered.
    ward_discard_random: bool = False
    # Ward—Discard a card (chosen, non-mana ward). Sunset Saboteur.
    # When True, the caster picks (via ``Agent.choose_discard``) which card
    # to discard from their hand to pay the ward; if their hand is empty,
    # the spell is countered.
    ward_discard_chosen: bool = False
    # ``Ward—Pay N life.`` — the caster of a spell targeting this
    # permanent pays N life or the spell is countered (CR 702.21a;
    # life can be paid only if the caster's life total is >= N,
    # CR 119.4). 0 = no life-cost ward.
    ward_life_cost: int = 0
    # Equipment: continuous bonuses granted to the equipped creature.
    # Tuple (power_bonus, toughness_bonus, granted_keywords). None on
    # non-Equipment cards. Consumed by game._static_bonuses.
    equipment_bonuses: tuple[int, int, frozenset[str]] | None = None
    # Shock-land-style optional ETB life cost. When nonzero, _play_land
    # asks the controller's agent whether to pay; yes pays the life and
    # enters untapped, no enters tapped. 0 means "no choice — enters
    # per its usual rules".
    shock_land_life_cost: int = 0
    # When True, this card always enters the battlefield tapped
    # (Planet-style utility lands and other "enters tapped" permanents).
    enters_tapped: bool = False
    # "This artifact doesn't untap during your untap step." (Grim
    # Monolith, Basalt Monolith, Mana Vault, Time Vault). Honored by
    # ``turn.run_untap_step``: the permanent itself stays tapped through
    # its controller's untap step. Other untap effects (its own
    # ``{N}: Untap this artifact.`` ability, Voltaic Key) still work.
    doesnt_untap_during_untap_step: bool = False
    # Mox Diamond: "If this artifact would enter, you may discard a land
    # card instead. If you do, put this artifact onto the battlefield.
    # If you don't, put it into its owner's graveyard." Replacement
    # effect applied at the top of ``actions.put_into_battlefield``.
    enters_only_by_discarding_land: bool = False
    # Time Vault: "If you would begin your turn while this artifact is
    # tapped, you may skip that turn instead. If you do, untap this
    # artifact." Replacement applied by
    # ``simulate._advance_to_next_active_player`` when the controller's
    # turn would begin.
    may_skip_turn_to_untap: bool = False
    # "Equipped creature gets +P/+T for each <types> you control."
    # (Nettlecyst: ``(("Artifact", "Enchantment"), 1, 1)``). Added to the
    # equipped creature's layer-7c bonus by ``game._static_bonuses``,
    # counting permanents with ANY of the listed card types controlled
    # by the Equipment's controller (each permanent counted once).
    equipment_bonus_per_controlled: "tuple[tuple[str, ...], int, int] | None" = None
    # "This token crews Vehicles as though its power were N greater."
    # (Reckoner Bankbuster's Pilot token). Added to the creature's power
    # when it is tapped to pay a Crew cost.
    crew_power_bonus: int = 0
    # Fast-path flag for ``actions._dispatch_graveyard_triggers``: True
    # iff some triggered ability has ``functions_in_graveyard`` (Sword
    # of the Meek). Set by the builder alongside those abilities.
    has_graveyard_triggers: bool = False
    # Sunburst (CR 702.44, Pentad Prism): enters with a charge counter
    # (+1/+1 counter if a creature) per color of mana spent to cast it.
    sunburst: bool = False
    # Bolas's Citadel: "You may play lands and cast spells from the top
    # of your library. If you cast a spell this way, pay life equal to
    # its mana value rather than pay its mana cost." Honored by
    # ``legal_actions`` (offers the top card) and ``_cast_spell`` /
    # ``_play_land`` (source zone + life payment).
    grants_play_from_top_paying_life: bool = False
    # Triggered abilities an Equipment grants to the equipped creature
    # (Kaldra Compleat: "… and has 'Whenever this creature deals combat
    # damage to a creature, exile that creature.'"). ``dispatch_event``
    # evaluates them with the equipped creature as their source.
    equipment_granted_triggers: tuple = ()
    # Duskmourn "horror dual" cycle (Bleeding Woods, Etched Cornfield,
    # …). When > 0, the land enters tapped unless any player's life
    # total is at or below this threshold at ETB time. No agent choice
    # — the condition is a deterministic state check. 0 disables.
    enters_tapped_unless_any_player_life_at_most: int = 0
    # Echoing Deeps: "You may have this land enter tapped as a copy of
    # any land card in a graveyard, except it's a Cave in addition to
    # its other types." When True, _play_land offers the controller's
    # agent a graveyard-land target. If they pick one, the played
    # land's printing is replaced with a Cave-injected copy of the
    # target and it enters tapped. If they decline (or no legal
    # target exists), the land enters per its normal rules.
    optional_copy_land_from_graveyard: bool = False
    # Generic "This land enters tapped unless <condition>." ETB
    # replacement (CR 614.1d / 614.12). ``None`` disables. When set, the
    # land enters tapped iff ``condition.matches(state, controller_id)``
    # is False, evaluated BEFORE the land joins the battlefield (so
    # "unless you control two or fewer other lands" counts only the
    # other lands). Applied by ``actions.apply_land_etb_replacements`` on
    # both the land-drop path and effect-driven puts (fetch lands).
    # Fast lands (``ControlsAtMost(IsLand(), 2)``), check lands
    # (``ControlPermanent(HasSubtype("Mountain"))``) and the basic-land
    # variant (``ControlPermanent(IsLand() & IsBasic())``).
    enters_tapped_unless: "Condition | None" = None
    # When True, ``produces`` is the EXHAUSTIVE set of mana this land's
    # cost-free (or life-cost, see ``produces_life_cost``) ``{T}`` mana
    # abilities make, and the land contributes to automatic cost
    # payment only through that set. The legacy default (False) keeps
    # the engine's historical fallback where every untapped land can
    # pay one generic even with an empty ``produces`` — pre-existing
    # printings rely on it for their dropped ``{T}: Add {C}.`` lines.
    # The line-composed land grammar sets this True and writes
    # colorless production explicitly as ``"C"``; a land whose only
    # mana abilities carry extra costs (Gaea's Cradle, Mishra's
    # Workshop, Gemstone Mine) or that has no mana ability at all
    # (fetch lands) therefore never taps for free phantom mana.
    mana_only_from_produces: bool = False
    # Life paid each time this land is tapped for its ``produces``
    # mana: "{T}, Pay 1 life: Add {G} or {W}." (Horizon Canopy cycle)
    # folds into ``produces={G, W}`` + ``produces_life_cost=1``.
    # Automatic payment charges the life per tapped land and treats the
    # land as unavailable once paying would drop its controller to 0
    # (the same "don't pay to death" gate ``legal_actions`` applies to
    # explicit ``pay_life`` activations). 0 = free.
    produces_life_cost: int = 0
    # "{T}: Add {C}. If <this> has a <kind> counter on it, instead add
    # one mana of any color." (Gemstone Caverns): ``(kind, colors)`` --
    # while the permanent has at least one ``kind`` counter, automatic
    # payment taps it for ``colors`` instead of ``produces``
    # (``game.effective_land_produces``). The explicit mana ability is
    # the matching ``IfConditionElse(SourceHasCounter(kind), ...)``, so
    # both payment paths read the same counter.
    produces_with_counter: "tuple[str, frozenset[str]] | None" = None
    # CR 103.6 opening-hand action ("If this card is in your opening
    # hand ..., you may begin the game with it on the battlefield ...");
    # an ``mtgz_ir.opening_hand.BeginGameOnBattlefield`` run by
    # ``simulate.setup_game`` after the final keep. None for every other
    # card.
    opening_hand_action: "object | None" = None
    # Activated abilities that function while this card is in its
    # owner's hand (CR 602.2 + 702.29 Cycling / 702.140 Channel). Each
    # ability's cost carries ``discard_self=True``; ``legal_actions``
    # offers ``ActivateAbility(source_id=<hand card>, ability_index=i)``
    # indexing into THIS tuple, and ``actions._activate_ability``
    # resolves a hand-resident source against it.
    hand_activated_abilities: tuple["ActivatedAbility", ...] = ()
    # "As this land enters, choose a basic land type." — the entry-time
    # choice (``actions.apply_land_etb_replacements`` asks the agent via
    # ``choose_mode`` over the five type names and stores it on
    # ``GameObject.chosen_basic_land_type``) ...
    enters_choosing_basic_land_type: bool = False
    # ... and "This land is the chosen type." — the static that makes
    # the permanent that type (``effective_subtypes``; its intrinsic
    # mana ability follows via ``effective_land_produces``).
    is_chosen_basic_land_type: bool = False
    # "This <permanent> enters with N <kind> counters on it." — ETB
    # replacement ``((kind, n), ...)``: Gemstone Mine's mining counters
    # (lands, ``actions.apply_land_etb_replacements``), Wishclaw
    # Talisman's wish / Reckoner Bankbuster's charge counters (every
    # other entry, ``actions.put_into_battlefield``). Kinds with a
    # dedicated ``GameObject`` field (charge, ...) land in that field.
    enters_with_named_counters: tuple = ()
    # "This creature enters with a +1/+1 counter on it for each <...> you
    # control" (Luxknight Breacher): an ``ObjectPredicate`` over the
    # permanents counted as it enters (CR 614.1c, 122.6; it isn't on the
    # battlefield yet, so "other" never counts itself).
    enters_with_plus_one_counters_per: "object | None" = None
    # Additional cost to cast: sacrifice a creature or artifact the
    # caster controls (other than any source-of-the-spell). Matches
    # Embrace Oblivion's "As an additional cost to cast this spell,
    # sacrifice an artifact or creature."
    additional_cost_sacrifice_creature_or_artifact: bool = False
    # Additional cost to cast: sacrifice a creature or enchantment the
    # caster controls. Matches Final Vengeance's "As an additional cost
    # to cast this spell, sacrifice a creature or enchantment."
    # Parallel to ``additional_cost_sacrifice_creature_or_artifact``;
    # both flags route through the same ``_additional_costs_payable``
    # gate in ``legal.py`` and the same pay-time sacrifice path in
    # ``actions.py``, just with the eligible-permanent filter set per
    # the printed cost body.
    additional_cost_sacrifice_creature_or_enchantment: bool = False
    # Additional cost to cast: designate one of your creatures. The
    # chosen creature's id is stored on the StackItem and exposed via
    # state.resolving_additional_cost_choice_id for the effect to use
    # (e.g. Close Encounter's damage-equal-to-power).
    additional_cost_choose_your_creature: bool = False
    # Cost reductions: rules that lower this spell's generic mana cost
    # when specific conditions hold at cast time (e.g., "costs {3} less
    # if you've cast another spell this turn"). Consumed by
    # actions.apply_cost_reductions().
    cost_reductions: tuple[CostReduction, ...] = ()
    # Static cost-reduction effects this permanent provides to the
    # controller's OTHER spells while it's on the battlefield (e.g.,
    # Uthros Psionicist: "the second spell you cast each turn costs
    # {2} less"). apply_cost_reductions() walks the caster's battlefield
    # and applies any matching externals on top of the spell's own
    # cost_reductions.
    external_cost_reductions: tuple[ExternalCostReduction, ...] = ()
    # Continuous abilities that buff other permanents while this permanent
    # is on the battlefield (anthems, keyword grants).
    static_abilities: tuple[StaticAbility, ...] = ()
    # Abilities the controller may activate by paying a cost.
    activated_abilities: tuple["ActivatedAbility", ...] = ()
    # "Enters with +1/+1 counters" replacement effects tied to the cast-time
    # choice of X. Consumed by actions.put_into_battlefield.
    #   ``"x"`` — enters with state.resolving_x_value counters (Broodguard).
    #   ``"mana_spent"`` — enters with counters equal to total mana paid,
    #     i.e. X + base generic + colored pips (Dyadrine).
    #   None — no such replacement.
    enters_with_counters_from_x: "XCounterMode | None" = None
    # External-anthem flag. While this permanent is on the battlefield,
    # each creature its controller plays enters with one additional +1/+1
    # counter for each land that entered the battlefield under that
    # controller this turn (read from Player.lands_entered_this_turn).
    # Multiple sources stack. Matches Bioengineered Future. Consumed in
    # actions.put_into_battlefield.
    grants_etb_counter_per_land_entered_this_turn: bool = False
    # ETB replacement: when this creature would enter the battlefield, if
    # ``state.creatures_died_this_turn > 0`` it enters with one +1/+1
    # counter on it instead. Matches Cackling Slasher's
    # ``This creature enters with a +1/+1 counter on it if a creature
    # died this turn.`` clause. Consumed in
    # ``actions.put_into_battlefield``; the death tracker increments in
    # ``turn.move_off_battlefield`` whenever any creature is graveyarded
    # and resets at cleanup. The check uses ``> 0`` rather than ``>= 1``
    # to defend against any future signed-int regressions, but the
    # tracker is always non-negative.
    enters_with_plus_one_counter_if_creature_died_this_turn: bool = False
    # ETB replacement: when this creature is cast from its controller's
    # hand and resolves onto the battlefield, it enters with N -1/-1
    # counters on it. Matches Patched Plaything's
    # ``This creature enters with two -1/-1 counters on it if you cast
    # it from your hand.`` clause. Consumed in
    # ``actions.put_into_battlefield``; the cast-from-hand origin is
    # recorded on the GameObject via ``cast_via_hand`` at cast time
    # (``actions._cast_spell``). 0 disables the replacement.
    enters_with_minus_one_counters_from_hand_cast: int = 0
    # Aura enchant restriction. Auras default to enchanting creatures;
    # Hardlight Containment enchants artifacts (and our machinery
    # wouldn't find a legal target otherwise). Accepts "creature" or
    # "artifact". Only consumed when is_aura is True.
    aura_enchant_type: EnchantKind = EnchantKind.CREATURE
    # Aura target controller restriction — when True, the cast-time
    # target picker for ``is_aura`` cards only offers permanents the
    # caster controls. Mirrors the printed "Enchant creature you
    # control" / "Enchant artifact you control" line — the printed
    # restriction is independent of ``aura_enchant_type``. Default
    # False (the default Scryfall "Enchant creature" wording allows
    # any creature). Sheltered by Ghosts is the first DSK Aura that
    # carries this restriction in the parser pool.
    aura_yours_only: bool = False
    # Dynamic power/toughness formula: a ``PTFormula``
    # (``mtgz_ir.effects.pt_formulas``) whose ``base_pt(state, obj)``
    # replaces the printed power/toughness inside effective_power and
    # effective_toughness (``game._dynamic_base_pt``): CDAs such as
    # ``CardTypesInAllGraveyardsPT`` (Lhurgoyfs), ``CardsYouOwnInExilePT``
    # (Cosmogoyf), ``LandsYouControlPT`` (Harmonious Grovestrider),
    # ``DomainPT`` (Territorial Kavu), ``PowerEqualsCount`` (Adeline),
    # leveler ranges (``LevelerPT``) and Emissary Escort's +X/+0
    # (``PowerPlusGreatestManaValueOfOtherArtifactsPT``).
    # None = use printed P/T (default).
    dynamic_pt_formula: "PTFormula | None" = None
    # Spell-cast restriction: when any permanent with this flag is on
    # the battlefield (regardless of controller), each player may cast
    # at most one noncreature spell per turn. Matches Deafening Silence.
    # Enforced by legal.legal_actions — noncreature CastSpell actions
    # aren't proposed once the caster's
    # Player.noncreature_spells_cast_this_turn is already ≥ 1.
    restricts_noncreature_spells_to_one_per_turn: bool = False
    # Narset, Parter of Veils: "Each opponent can't draw more than one
    # card each turn." While a permanent with this flag is on the
    # battlefield, ``turn.draw_card`` skips any draw by an opponent of
    # its controller who has already drawn a card this turn
    # (``Player.cards_drawn_this_turn``).
    opponents_cant_draw_more_than_one_card_each_turn: bool = False
    # Teferi, Time Raveler: "Each opponent can cast spells only any time
    # they could cast a sorcery." While a permanent with this flag is on
    # the battlefield, ``legal.legal_actions`` offers an opponent of its
    # controller no spell casts outside that opponent's sorcery timing
    # (instants and flash included).
    opponents_cast_spells_only_at_sorcery_speed: bool = False
    # The Wandering Emperor: "As long as ~ entered this turn, you may
    # activate her loyalty abilities any time you could cast an
    # instant." ``legal.legal_actions`` lifts the sorcery-timing gate
    # on this permanent's loyalty abilities while
    # ``GameObject.entered_battlefield_turn == state.turn_number``
    # (the once-per-turn loyalty gate still applies).
    loyalty_abilities_at_instant_speed_if_entered_this_turn: bool = False
    # Type-change flag: while any permanent with this flag is on the
    # battlefield, every nonbasic land's mana-production colors are
    # overridden to {R} and its activated abilities are suppressed —
    # the Magus of the Moon "Nonbasic lands are Mountains" static.
    # Consumed by game.effective_land_produces and
    # game.effective_activated_abilities. Simplification: subtypes
    # aren't rewritten, and already-animated manlands keep their
    # animated creature state (no layers yet).
    nonbasic_lands_are_mountains: bool = False
    # Dryad of the Ilysian Grove: "Lands you control are every basic
    # land type in addition to their other types." A printing with
    # this flag, while on the battlefield, contributes synthetic
    # layer-4 ``"add_subtype"`` entries (one per basic land type) to
    # ``effective_subtypes`` for every land its controller controls.
    # Synthesizing on-the-fly avoids ETB/leave plumbing; the
    # source's ``etb_timestamp`` is used so within-layer timestamp
    # order vs Blood Moon / Magus of the Moon matches CR 613.4.
    grants_all_basic_land_types_to_lands_you_control: bool = False
    # Urborg, Tomb of Yawgmoth / Yavimaya, Cradle of Growth: "Each
    # land is a [basic land type] in addition to its other types."
    # When this is set on a permanent on the battlefield,
    # ``effective_subtypes`` synthesizes a layer-4 ``"add_subtype"``
    # entry on every land, sourced by this permanent. The CR 613.7
    # dependency rule kicks in here: if the source itself is a
    # land that's been rewritten by a layer-4
    # ``"set_basic_land_subtype"`` effect (Blood Moon / Magus),
    # its ability is wiped at layer 4 (CR 305.6) and the
    # contribution doesn't apply. This is the famous Blood Moon
    # beats Urborg ruling.
    grants_basic_land_type_to_all_lands: "str | None" = None
    # Painter's Servant: "As Painter's Servant enters, choose a
    # color. All cards that aren't on the battlefield, spells, and
    # permanents are the chosen color in addition to their other
    # colors." When True on a permanent's printing,
    # ``effective_colors`` synthesizes a layer-5 ``"add_colors"``
    # entry on every battlefield permanent (and spell) with the
    # source's chosen color. The chosen color is captured at ETB
    # time on ``GameObject.painter_chosen_color``. The source's
    # ``etb_timestamp`` is the synthesized entry's timestamp so it
    # interleaves correctly with Scuttlemutt-style set_colors per
    # CR 613 layer-5 timestamp rules.
    paints_all_objects_chosen_color_at_etb: bool = False
    # Mycosynth Lattice: "All permanents are artifacts in addition to
    # their other types." -- the card type (``"Artifact"``) every
    # permanent gains while this is on the battlefield. A layer-4 effect
    # synthesized per read by ``game.effective_card_types`` (the Magus /
    # Dryad precedent), so it needs no enter / leave plumbing.
    all_permanents_gain_card_type: "str | None" = None
    # Mycosynth Lattice: "All cards that aren't on the battlefield,
    # spells, and permanents are colorless." -- a layer-5 ``set_colors``
    # (no colors) entry on every object, synthesized by
    # ``game.effective_colors`` with this source's ``etb_timestamp``
    # (Painter's Servant's shape with "colorless" for "the chosen color
    # in addition to their other colors").
    all_objects_colorless: bool = False
    # Mycosynth Lattice: "Players may spend mana as though it were mana
    # of any color." -- a rule modification (no layer): while a source
    # with this flag still has its abilities, cost payment lets any mana
    # pay a colored pip (``actions.mana_spendable_as_any_color``).
    mana_spendable_as_any_color: bool = False
    # "As this creature enters, choose a player." (True-Name Nemesis) —
    # the chosen player's id lands on ``GameObject.chosen_player_id``
    # (read by the ``protection from the chosen player`` quality).
    chooses_player_as_enters: bool = False
    # Found Footage (Duskmourn): "You may look at face-down creatures
    # your opponents control any time." Today the engine doesn't model
    # hidden information for face-down permanents (the controller can
    # already inspect any face-down GameObject in the battlefield list),
    # so this flag is informational only — it carries the printed
    # static for fidelity but has no engine consequence. Reserved for
    # future hidden-information modeling.
    looks_at_opponents_face_down_creatures: bool = False
    # Humility (Tempest): "All creatures lose all abilities and have
    # base power and toughness 1/1." Decomposed into two flags so a
    # future card with only one half (just the wipe, just the 1/1)
    # composes cleanly:
    # - ``wipes_all_creature_abilities``: synthesizes a layer-6
    #   ``"lose_all_abilities"`` entry on every creature, sourced
    #   by this permanent with its ``etb_timestamp``.
    # - ``sets_all_creature_pt_to_1_1``: synthesizes a layer-7
    #   sublayer-1 ``"set_base_pt"`` entry with ``params=(1, 1)``
    #   on every creature, same source/timestamp.
    # Both contributions interleave with per-creature registry
    # entries (Lignify, anthem grants) by timestamp in the usual
    # CR 613 layer walks.
    wipes_all_creature_abilities: bool = False
    sets_all_creature_pt_to_1_1: bool = False
    # "This creature must be blocked if able." (CR 509.1c). When True,
    # while this creature is attacking, the defending player must
    # assign at least one of their untapped creatures that could
    # legally block it (per ``combat.can_block``) to block it. Enforced
    # in ``actions._declare_blockers``: if no validated blocker is
    # assigned to the attacker and at least one eligible defender
    # exists that could block it, the engine picks one (deterministic
    # — first by id) and adds the assignment. Differs from "menace" /
    # "unblockable" / per-block restrictions: those LIMIT the blocker
    # set; this requirement OBLIGATES at least one block. Matches DSK's
    # Fear of Being Hunted's static rider.
    must_be_blocked_if_able: bool = False
    # "This spell can't be countered." When True on a spell's printing,
    # ``CounterTargetSpell.legal_targets`` and ``CounterTargetSpell.resolve``
    # both skip this stack item — it appears untargetable by counter effects
    # and the counter fizzles silently even if somehow targeted. Matches
    # Frenzied Baloth's first line. The flag lives on the printing (static
    # declaration) and is checked against the stack item's source printing
    # at targeting time. CR 101.2 rule: a spell with this clause can't be
    # countered by spells or abilities.
    spell_is_uncounterable: bool = False
    # "Combat damage can't be prevented." When True on a creature's printing,
    # the global ``state.prevent_combat_damage_this_turn`` flag is bypassed
    # for damage events sourced by this creature. Checked per-attacker and
    # per-blocker in ``combat._resolve_damage_step``. Matches Frenzied
    # Baloth's fourth line.
    combat_damage_cant_be_prevented: bool = False
    # "Each creature you control with toughness greater than its power assigns
    # combat damage equal to its toughness rather than its power." When True
    # on a permanent's printing, ``combat._effective_combat_damage_amount``
    # checks this flag on all permanents the attacking/blocking creature's
    # controller controls and uses toughness instead of power when the
    # creature's toughness exceeds its power. CR 510.1b replacement effect.
    # Matches Tapestry Warden's second oracle line.
    grants_toughness_as_damage_for_creatures_you_control: bool = False
    # "Each creature you control with toughness greater than its power stations
    # permanents using its toughness rather than its power." When True on a
    # permanent's printing, ``actions._station_spacecraft`` checks this flag
    # and uses toughness instead of effective_power when adding charge counters
    # for a qualifying crew creature. Matches Tapestry Warden's third oracle
    # line.
    grants_toughness_as_station_power_for_creatures_you_control: bool = False
    # Protection from a single color (CR 702.16). When set on a
    # creature's printing, ``combat.can_block`` rejects any block
    # declared by an attacker (or against an attacker) whose
    # effective color includes this one. Matches the classic White
    # Knight ("Protection from black") / Black Knight ("Protection
    # from white") shape. Read via
    # ``game.effective_protection_from_colors`` so future text-/
    # ability-changing layers (Layer 3 color-word rewrites, Layer 6
    # ability grants/removals) compose cleanly. Single color only
    # for now; extend to a frozenset if real cards need protection
    # from multiple colors at once.
    protection_from_color: str | None = None
    # Color indicator / token color (CR 204, CR 111.4): colors an
    # object has that its mana cost doesn't show — a "1/1 white Spirit
    # creature token" is white. ``game.effective_colors`` unions this
    # with ``mana_cost.colors`` as the layer-5 base.
    color_indicator: frozenset[str] = frozenset()
    # Convoke (CR 702.51): "Your creatures can help cast this spell.
    # Each creature you tap while casting this spell pays for {1} or
    # one mana of that creature's color." Wires the per-cast prompt
    # in ``_cast_spell``: after cost determination (601.2f) and
    # before paying the remaining cost from the mana pool / lands,
    # the controller's agent picks creatures to tap. Each tapped
    # creature pays {1} (or one of its colors) toward the cost. A
    # tapped creature can't separately activate its own
    # ``{T}, sacrifice: Add ...`` mana ability for the same cast,
    # so Convoke and a sac-for-mana ability on the same source are
    # mutually exclusive — matches CR.
    has_convoke: bool = False
    # Block restriction: "This creature can block only creatures with
    # flying." Sibling concept to reach (which loosens what *flying*
    # attackers can be blocked by). Matches the EoE Drone token printed
    # on Pinnacle Emissary, Station Monitor, etc. Consumed by
    # combat.can_block — when True on the blocker's printing, the
    # blocker may only declare blocks against attackers with flying.
    can_only_block_creatures_with_flying: bool = False
    # Basic-landcycling. CR 702.29 typecycling: while this card is in
    # the controller's hand, they may pay the cycling cost and discard
    # this card to search their library for a basic-land card of the
    # named subtype, reveal it, put it into their hand, then shuffle.
    # Tuple shape is ``(land_subtype, mana_cost_str)`` — e.g. DSK's
    # Plainscycling {2} cycle prints as ``("Plains", "{2}")``,
    # Islandcycling {2} as ``("Island", "{2}")``, etc. ``None`` when
    # the card has no landcycling.
    #
    # This field is descriptive metadata; the ACTIVATABLE ability lives
    # on ``hand_activated_abilities`` (the oracle parser's source layer peels every
    # cycling-family line into a hand-activated ``ActivatedAbility``
    # with a ``discard_self`` cost and fills this tuple from it too).
    landcycling: tuple[str, str] | None = None
    # ---- Spell casting substrate (cube spells-B bucket) -------------
    # "Flashback {cost}" (CR 702.34): the card may be cast from its
    # owner's graveyard for ``flashback_cost``; a spell cast this way
    # is exiled instead of going anywhere else when it leaves the
    # stack. ``legal_actions`` offers ``CastSpell(id, use_flashback=True)``
    # for graveyard cards carrying it (instants at instant speed,
    # everything else at sorcery speed).
    flashback_cost: ManaCost | None = None
    flashback_cost_str: str = ""
    # "Delve" (CR 702.66): each card exiled from the caster's graveyard
    # while casting pays for {1}. Handled at cost payment in
    # ``actions._cast_spell`` (count chosen via ``choose_x``).
    has_delve: bool = False
    # "Buyback {cost}" (CR 702.27): optional additional cost; if paid,
    # the spell returns to its owner's hand as it resolves instead of
    # going to the graveyard. Offered as ``CastSpell(id,
    # use_buyback=True)``.
    buyback_cost: ManaCost | None = None
    buyback_cost_str: str = ""
    # "As an additional cost to cast this spell, sacrifice a <filter>."
    # A ``mtgz_ir.effects.generic.card_flow.CardFilter`` over the
    # caster's permanents (Tinker: artifact; Natural Order: green
    # creature; Crop Rotation: land). ``None`` = no such cost.
    additional_cost_sacrifice_filter: "object | None" = None
    # "As an additional cost to cast this spell, pay X life." The
    # caster picks X at cast time (``choose_x``); X is stored on the
    # StackItem's ``x_value`` and read at resolve through
    # ``state.resolving_x_value`` (Toxic Deluge).
    additional_cost_pay_x_life: bool = False
    # "If <condition>, you may cast it without paying its mana cost."
    # (Once Upon a Time: ``FirstSpellYouCastThisGame``). A
    # ``mtgz_ir.effects.conditions.Condition`` evaluated for the caster;
    # when it holds ``legal_actions`` also offers the
    # ``CastSpell(cast_for_free=True)`` variant.
    free_cast_condition: "object | None" = None
    # "Storm" (CR 702.40): when cast, copy it for each spell cast before
    # it this turn (``actions._cast_spell`` pushes the trigger).
    has_storm: bool = False
    # "You may exile a <filter> card from your hand rather than pay this
    # spell's mana cost." (Pyrokinesis) — an alternative cost offered as
    # ``CastSpell(cast_for_free=True)``; a ``CardFilter`` over the other
    # cards in hand.
    alt_cost_exile_from_hand: "object | None" = None
    # Roto P4: how many such cards the pitch cost exiles ("exile two blue
    # cards from your hand" -- Commandeer).
    alt_cost_exile_from_hand_count: int = 1
    # "Splice onto <subtype> {cost}" (CR 702.47): while casting a spell
    # with that subtype, this card may be revealed from hand and its
    # splice cost paid to add its effects to that spell.
    splice_onto: "str | None" = None
    splice_cost: ManaCost | None = None
    splice_cost_str: str = ""
    # Modal double-faced card (CR 712): the back face's printing when it
    # is a land the card may be played as (Sink into Stupor // Soporific
    # Springs). ``legal_actions`` offers ``PlayLand`` for the card from
    # hand; ``actions._play_land`` turns it to this face. When the back
    # face is a castable permanent (The Sensational She-Hulk), the card
    # may instead be cast as that face (``CastSpell(cast_face=<name>)``,
    # ``mtgz_ir.layouts``).
    mdfc_back_face: "CardPrinting | None" = None
    # --- Non-normal layouts (cube layouts bucket; ``mtgz_ir.layouts``) --
    # Transforming double-faced card (CR 712 / 701.28): the back face's
    # printing. ``layouts.transform_permanent`` turns a permanent to this
    # face (and back); ``layouts.put_onto_battlefield_transformed`` puts
    # the card onto the battlefield with it up. Also set on a modal DFC
    # whose front face carries a "Transform ~" ability (the back is then
    # both castable and transformable).
    transform_back_face: "CardPrinting | None" = None
    # The colors of a DFC back face (CR 204: its color indicator; no
    # indicator = colorless). A transforming back face borrows the front
    # face's mana cost for its mana value (CR 712.8e), so its colors
    # can't be read off that cost: ``game.effective_colors`` uses this
    # set instead of ``mana_cost.colors`` when it isn't ``None``.
    face_colors: "frozenset[str] | None" = None
    # Daybound / nightbound (CR 702.145): the front face of a daybound
    # werewolf has ``daybound``, its back face ``nightbound``. The
    # day/night cycle (``layouts.run_day_night_check``) transforms them.
    daybound: bool = False
    nightbound: bool = False
    # Class enchantments (CR 716): this printing's class level (1 for
    # the card as printed), and the printings for each further level in
    # order (level 2, level 3, ...). Each level printing carries every
    # ability of the levels below it plus its own; gaining a level turns
    # the permanent to that printing (``layouts.gain_class_level``).
    class_level: int = 0
    class_levels: tuple["CardPrinting", ...] = ()
    # Split card (CR 709): the two halves' printings. The card's own
    # printing carries the combined characteristics it has outside the
    # stack (CR 709.4); it's cast as one half
    # (``CastSpell(cast_face=<half name>)``), which turns the spell to
    # that half's printing until it leaves the stack.
    split_faces: tuple["CardPrinting", ...] = ()
    # "Each nonland card in your graveyard has mayhem. The mayhem cost is
    # equal to its mana cost." (Green Goblin's Goblin Formula): while its
    # controller controls this permanent, they may cast a nonland card
    # from their graveyard for its mana cost if they discarded it this
    # turn (``layouts.mayhem_castable``).
    grants_mayhem_to_graveyard: bool = False
    # Adventurer card (CR 715): the Adventure half's printing (an instant
    # or sorcery with the Adventure subtype). The card may be cast as it
    # (``CastSpell(cast_face=<name>)``); as that spell resolves it's
    # exiled "on an adventure" and its controller may cast the creature
    # from exile (``layouts.exile_on_adventure``).
    adventure_face: "CardPrinting | None" = None
    # Preparation card (Secrets of Strixhaven "prepare"): the printing of
    # the creature's prepare spell. As the creature becomes prepared its
    # controller creates a copy of this spell in exile they may cast while
    # the creature stays prepared (``layouts.become_prepared``).
    prepare_spell: "CardPrinting | None" = None
    # "This creature enters prepared."
    enters_prepared: bool = False
    # "Plot {cost}" (CR 702.170): as a sorcery, pay the plot cost and
    # exile the card from hand; on a later turn it may be cast from
    # exile without paying its mana cost, as a sorcery. The special
    # action is ``CastSpell(id, use_plot=True)``; the later cast is
    # ``CastSpell(id, cast_for_free=True)`` from exile.
    plot_cost: ManaCost | None = None
    plot_cost_str: str = ""
    # "Escape—{cost}, Exile N other cards from your graveyard." (CR
    # 702.138): cast from the graveyard for ``escape_cost`` plus exiling
    # ``escape_exile_count`` other graveyard cards. Offered as
    # ``CastSpell(id, use_escape=True)``.
    escape_cost: ManaCost | None = None
    escape_cost_str: str = ""
    escape_exile_count: int = 0
    # Granted escape (Underworld Breach): "Each nonland card in your
    # graveyard has escape. The escape cost is equal to the card's mana
    # cost plus exile N other cards from your graveyard." Nonzero N
    # grants that escape to every nonland card in this permanent's
    # controller's graveyard (``alt_casting.escape_terms``).
    grants_graveyard_escape_exile_count: int = 0
    # Necromancy: "If you cast it any time a sorcery couldn't have been
    # cast, the controller of the permanent it becomes sacrifices it at
    # the beginning of the next cleanup step."
    sacrifice_at_cleanup_if_cast_without_sorcery_timing: bool = False
    # ── Cube creatures (R / multicolor / colorless bucket) ──
    # Activated abilities that function from the owner's graveyard
    # (CR 113.6k): Seasoned Pyromancer's "{3}{R}{R}, Exile this card
    # from your graveyard: ...", Otharri's "Return this card from your
    # graveyard to the battlefield tapped". Offered by ``legal_actions``
    # as ``ActivateAbility(source_id=<graveyard card>, ability_index)``.
    graveyard_activated_abilities: tuple["ActivatedAbility", ...] = ()
    # "Dash {cost}" (CR 702.109): cast from hand for ``dash_cost``; the
    # permanent gains haste and returns to its owner's hand at the
    # beginning of the next end step. Offered as
    # ``CastSpell(id, use_dash=True)``.
    dash_cost: ManaCost | None = None
    dash_cost_str: str = ""
    # "If damage would be dealt to <this creature>, instead that damage
    # is dealt, but all other damage already dealt to him is healed."
    # (Wolverine, Fierce Fighter): marked damage is cleared right
    # before new damage is marked (``cube_creatures_rmc.heal_before_damage``).
    heals_other_damage_when_dealt_damage: bool = False
    # Fast-path flag for ``cube_creatures_rmc.sweep_graveyard_watch``:
    # the card has a "... put into a graveyard from anywhere"
    # replacement or trigger (Blightsteel Colossus, Emrakul).
    watched_in_graveyard: bool = False
    # "Whenever one or more cards are put into exile from your library
    # and/or your graveyard" watcher (Laelia) — fast-path flag for
    # ``cube_creatures_rmc.sweep_library_graveyard_exile_watch``.
    watches_library_graveyard_exile: bool = False
    # Multikicker (CR 702.33c): the kicker cost (``kicker_cost``) may be
    # paid any number of times; the count rides ``StackItem.kick_count``
    # onto ``GameObject.kick_count``.
    multikicker: bool = False
    # "<This> enters with a +1/+1 counter on it for each time it was
    # kicked." (Batroc the Leaper)
    enters_with_counter_per_kick: bool = False
    # ── Cube W/G creature bucket (casting / printing mechanics) ──────
    # Evoke (CR 702.74): "Evoke—<cost>". The alternative cost is either
    # mana (``evoke_cost``) or "Exile a <filter> card from your hand"
    # (``evoke_exile_from_hand``, a ``CardFilter`` — Solitude / Endurance
    # / the Modern Horizons 2 Incarnations). Offered as
    # ``CastSpell(id, use_evoke=True)`` (``mtgz_ir.evoke``); the printing
    # also carries the evoke sacrifice trigger ("When this permanent
    # enters, if its evoke cost was paid, its controller sacrifices it").
    evoke_cost: ManaCost | None = None
    evoke_cost_str: str = ""
    evoke_exile_from_hand: "object | None" = None
    # Rules-restricting statics (``mtgz_ir.creature_statics_wg``):
    # "Your opponents can't cast spells during your turn." (Voice of
    # Victory) and "Each player can't cast more than one spell each
    # turn." / "Nonbasic lands your opponents control enter tapped."
    # (Archon of Emeria).
    opponents_cant_cast_spells_during_your_turn: bool = False
    each_player_one_spell_per_turn: bool = False
    opponents_nonbasic_lands_enter_tapped: bool = False
    # "<Filter> spells cost {N} more to cast." (Thalia, Guardian of
    # Thraben): ``((CardFilter, N), ...)`` applied to every player's
    # matching spells while this permanent is on the battlefield.
    spell_cost_increases: tuple = ()
    # "<This> can't be blocked by <creatures with ...>." (Questing
    # Beast): an ObjectPredicate over would-be blockers.
    cant_be_blocked_by: "object | None" = None
    # "Combat damage that would be dealt by creatures you control can't
    # be prevented." (Questing Beast).
    your_creatures_combat_damage_cant_be_prevented: bool = False
    # "Activated abilities of <types> can't be activated." (Clarion
    # Conqueror): the card types whose permanents' activated abilities
    # (mana abilities included) are locked while this is on the
    # battlefield.
    locks_activated_abilities_of: tuple = ()
    # "Activated abilities of <types> your opponents control can't be
    # activated." (Karn, the Great Creator): the same lock, applied only to
    # permanents an opponent of this permanent's controller controls.
    locks_opponents_activated_abilities_of: tuple = ()
    # "If a nontoken creature would enter and it wasn't cast, exile it
    # instead." (Containment Priest).
    exiles_uncast_nontoken_creatures_entering: bool = False
    # Graveyard play permissions (``mtgz_ir.graveyard_casting_wg``):
    # Six's "During your turn, nonland permanent cards in your graveyard
    # have retrace." and Serra Paragon's once-per-turn land / permanent
    # spell (mana value <= N) from your graveyard.
    grants_retrace_to_nonland_permanents_during_your_turn: bool = False
    grants_once_per_turn_graveyard_play_mv: int = 0
    # "<Cards> in your graveyard have retrace." on a permanent or emblem
    # (Wrenn and Six's emblem: instant and sorcery cards) -- a
    # ``CardFilter`` over the controller's graveyard cards, always on.
    grants_retrace_to: "object | None" = None
    # "Spells and abilities your opponents control can't cause you to
    # discard cards or sacrifice permanents." (Tamiyo, Collector of
    # Tales) -- ``mtgz_ir.player_protections``.
    opponents_cant_make_you_discard_or_sacrifice: bool = False
    # "Whenever you tap a <Subtype> for mana, add an additional {C}."
    # (Nissa, Who Shakes the World) -- ``(subtype, color)`` pairs read
    # by automatic payment (``actions.extra_mana_grants_for_tapped_lands``).
    extra_mana_when_you_tap_land_subtype: tuple = ()
    # "During your turn, as long as <this> has one or more loyalty
    # counters on <it>, <it>'s a P/T <Subtypes> creature [and has
    # <keywords>]." (Kaito, Bane of Nightmares) -- ``(power, toughness,
    # subtypes, keywords)``, applied at each of the controller's turns
    # (``mtgz_ir.cube_glue_statics``). ``None`` when absent.
    creature_during_your_turn: (
        "tuple[int, int, tuple[str, ...], frozenset[KeywordSpelling]] | None"
    ) = None
    # Anointed Peacekeeper: "As this creature enters, look at an
    # opponent's hand, then choose any card name." plus the named-card
    # taxes -- opponents' spells with the chosen name cost {N} more;
    # activated abilities (not mana abilities) of sources with the
    # chosen name cost {M} more (``creature_statics_wg``).
    chooses_card_name_as_enters: bool = False
    named_spell_cost_increase: int = 0
    named_ability_cost_increase: int = 0
    # Reconfigure (CR 702.151; Lion Sash): the printing also carries the
    # attach / unattach activated abilities; while attached to a
    # creature it isn't a creature (``game.is_creature_now``).
    reconfigure_cost: "ManaCost | None" = None
    reconfigure_cost_str: str = ""
    # "Equipped creature gets +P/+T for each <kind> counter on this
    # Equipment." -- ``(kind, P, T)`` (Lion Sash).
    equipment_bonus_per_own_counter: tuple = ()
    # Bestow (CR 702.103; Springheart Nantuko): cast as an Aura with
    # enchant creature for ``bestow_cost`` (``CastSpell(use_bestow=True)``);
    # ``bestow_enchanted_bonus`` is the printed "Enchanted creature gets
    # +P/+T." while it's attached.
    bestow_cost: "ManaCost | None" = None
    bestow_cost_str: str = ""
    bestow_enchanted_bonus: tuple = ()
    # "Whenever you tap a creature for mana, add an additional {G}."
    # (Badgermole Cub): the extra coin colors (a triggered mana ability).
    extra_mana_when_you_tap_creature_for_mana: tuple = ()
    # ── U/B cube creatures ──────────────────────────────────────────
    # "This creature can't be blocked except by N or more creatures."
    # (Troll of Khazad-dûm, N=3): a block by fewer than N creatures is
    # illegal -- the menace check generalized (menace is N=2).
    min_blockers: int = 0
    # "Squad {cost}" (CR 702.157): an additional cost payable any number
    # of times; one token copy per payment when it enters.
    squad_cost: ManaCost | None = None
    squad_cost_str: str = ""
    # "Offspring {cost}" (CR 702.175): an optional additional cost; if
    # paid, a 1/1 token copy when it enters.
    offspring_cost: ManaCost | None = None
    offspring_cost_str: str = ""
    # Improvise (CR 702.126): each untapped artifact the caster taps
    # while paying pays for {1}.
    has_improvise: bool = False
    # "If a card would be put into an opponent's graveyard from
    # anywhere, instead exile it with a void counter on it." (Dauthi
    # Voidwalker). See ``game.void_exile_replacement_active``.
    void_exile_replacement: bool = False
    # Roto P2 zone abilities (replacements, graveyard abilities): frozen specs
    # from ``effects.generic.roto_zones`` (``GraveyardExileInstead`` --
    # Rest in Peace / Leyline of the Void / Sanctifier en-Vec, ...),
    # read by that module's hooks.
    zone_abilities: tuple = ()
    # "Emerge [from artifact] {cost}" (CR 702.119): alternative cost --
    # sacrifice a creature (or artifact) and pay the emerge cost reduced
    # by that permanent's mana value.
    emerge_cost: ManaCost | None = None
    emerge_cost_str: str = ""
    emerge_sacrifice_type: str = "Creature"
    # "Miracle {cost}" (CR 702.94): if it's the first card its owner drew
    # this turn, they may reveal it and cast it for this cost when the
    # miracle trigger resolves.
    miracle_cost: ManaCost | None = None
    miracle_cost_str: str = ""
    # "As an additional cost to cast this spell, exile N cards from your
    # graveyard." (Abhorrent Oculus)
    additional_cost_exile_from_graveyard: int = 0
    # "Activated abilities of artifacts you control cost {N} less to
    # activate. This effect can't reduce the mana in that cost to less
    # than one mana." (Forensic Gadgeteer)
    artifact_ability_cost_reduction: int = 0
    # "As <this> enters, choose odd or even." (Gollum, Riddle Master)
    chooses_parity_as_enters: bool = False
    # "X can't be 0." (Thieving Skydiver's "Kicker {X}. X can't be 0."):
    # the least X the caster may choose.
    x_min: int = 0
    # "This creature can't block or be blocked by non-<Subtype>
    # creatures." (Wan Shi Tong's Spirit tokens)
    blocks_only_with_subtype: str = ""
    # Clone family beyond ``enters_as_copy_of_creature`` (U/B cube):
    # "You may have this creature enter as a copy of any <types> on the
    # battlefield" (``enters_as_copy_types``, e.g. ("Artifact",
    # "Creature") for Phyrexian Metamorph) or "... any creature card in a
    # graveyard" (``enters_as_copy_zone="graveyard"``, Superior
    # Spider-Man); the "except ..." riders: an added card type, a kept
    # name, a set base P/T and added subtypes; and "When you do, exile
    # that card." (``enters_as_copy_exiles_card``).
    enters_as_copy_types: tuple[str, ...] = ()
    enters_as_copy_zone: ZoneName = ZoneName.BATTLEFIELD
    enters_as_copy_add_type: str = ""
    enters_as_copy_keep_name: bool = False
    enters_as_copy_base_pt: "tuple[int, int] | None" = None
    enters_as_copy_add_subtypes: tuple[str, ...] = ()
    enters_as_copy_exiles_card: bool = False
    # Roto P4: "... except ... and it has <keywords> / "<ability>""
    # riders (Phantasmal Image's granted becomes-the-target trigger).
    enters_as_copy_add_triggers: tuple = ()
    enters_as_copy_add_keywords: frozenset[KeywordSpelling] = frozenset()
    # "... any creature on the battlefield with mana value less than or
    # equal to the amount of mana spent to cast this creature" (Mockingbird;
    # roto integration): candidates are capped by
    # ``GameObject.mana_spent_to_cast`` (0 unless it was cast).
    enters_as_copy_max_mv_mana_spent: bool = False
    # Roto P4: Rebound (CR 702.88) -- cast from hand, the spell is exiled
    # as it resolves and may be cast free at its owner's next upkeep
    # (``roto_walkers.exile_with_rebound``).
    rebound: bool = False
    # DSK Rooms: a tuple of two ``RoomHalf`` entries when this printing
    # is a Room. Empty for non-Room cards. The Room's overall
    # ``mana_cost`` mirrors one half's cost at print time, but at cast
    # time the caster picks WHICH half to cast (and pays that half's
    # cost). The Room enters with that half ``unlocked``; the other is
    # locked. ``UnlockRoomHalf`` later pays the second half's cost to
    # unlock it. Each half's abilities are also flattened onto this
    # printing's ``triggered_abilities`` / ``static_abilities`` /
    # ``activated_abilities`` with each ability's ``room_half`` field
    # set to the half-name, so the existing iterators (filtered through
    # ``effective_*_abilities``) gate them by ``unlocked_halves``.
    room_halves: tuple[RoomHalf, ...] = ()
    # --- Spell costs (cube spells A) -------------------------------
    # Printed alternative costs ("You may <cost> rather than pay this
    # spell's mana cost", CR 118.9): Force of Will, Daze, Snuff Out,
    # Mine Collapse, Baleful Mastery, Overload ... — tuple of
    # ``mtgz_ir.cards.spell_costs.AlternativeCost``. Cast through the
    # ``CastSpell(cast_for_free=True)`` variant, shared with the pitch
    # cost above (``spell_costs.select_alternative_cost``).
    alternative_costs: tuple = ()
    # One-of additional costs ("As an additional cost to cast this
    # spell, discard a card or pay 3 life."): tuple of
    # ``spell_costs.AdditionalCostOption``; the caster pays exactly one
    # payable arm (``spell_costs.pay_additional_cost_option``).
    additional_cost_options: tuple = ()
    # Replicate (CR 702.56): "When you cast this spell, copy it for each
    # time you paid its replicate cost." Mana-cost string ("{U}"); the
    # caster picks how many times to pay at cast time.
    replicate_cost: str = ""
    # Teamwork N: "As an additional cost to cast this spell, you may
    # tap any number of untapped creatures you control with total power
    # N or more." Optional; recorded on the spell object as
    # ``GameObject.optional_additional_cost_paid``.
    teamwork_power: int = 0
    # Roto P1 (stack & restrictions): rules-modifying statics read by
    # ``mtgz_ir.roto_restrictions`` -- conditional spell taxes, the
    # Trinisphere cost floor, can't-cast / can't-activate prohibitions
    # (named, player-scoped, mana-value capped). A tuple of that
    # module's static descriptors; empty on every other printing.
    roto_statics: tuple = ()
    # --- Vintage Roto P3: mana, costs, cast permissions -------------
    # "As this land enters, choose a creature type." (Cavern of Souls):
    # the choice lands on ``GameObject.chosen_creature_type``
    # (``effects.generic.roto_mana.choose_creature_type_as_enters``).
    enters_choosing_creature_type: bool = False
    # "You may cast artifact spells and colorless spells from the top of
    # your library." (Mystic Forge): the spell kinds ("artifact",
    # "colorless") castable from the top of the controller's library. The
    # artifact-only grant keeps ``grants_cast_artifact_from_top_of_library``.
    cast_from_top_of_library_kinds: tuple = ()
    # Suspend N--<cost> (CR 702.62): ``suspend_count`` time counters,
    # ``suspend_cost`` paid by the special action
    # (``CastSpell(id, use_suspend=True)``, ``roto_mana.suspend_card``).
    suspend_count: int = 0
    suspend_cost: "ManaCost | None" = None
    suspend_cost_str: str = ""
    # A nonland card printed with no mana cost (Lotus Bloom): it can't be
    # cast by paying its mana cost (CR 202.1b / 601.2f), only through an
    # alternative cost such as "without paying its mana cost".
    has_no_mana_cost: bool = False
    # Activated abilities that function while the card is in exile
    # (Greater Gargadon's "Activate only if this card is suspended."),
    # offered by ``roto_mana.roto_legal_actions`` and activated through
    # ``roto_mana.activate_exile_ability``.
    exile_activated_abilities: tuple = ()
    # "Enchant Forest" (Utopia Sprawl): with ``aura_enchant_type="land"``,
    # the land subtype the enchanted land must have.
    aura_enchant_subtype: str = ""
    # "As this <permanent> enters, choose a color." -> GameObject.chosen_color
    enters_choosing_color: bool = False
    # "Whenever enchanted <land> is tapped for mana, its controller adds an
    # additional one mana of the chosen color." (Utopia Sprawl).
    enchanted_land_adds_chosen_color: bool = False
    # "As an additional cost to cast this spell, discard X cards."
    # (Firestorm): X is announced before targets and becomes the spell's X.
    additional_cost_discard_x: bool = False
    # "As an additional cost to cast this spell, you may sacrifice any
    # number of <permanents>." (Rottenmouth Viper): the ObjectPredicate
    # over sacrificeable permanents; the matching "costs {1} less for each
    # permanent sacrificed this way" is a ``cost_reductions`` entry.
    optional_sacrifice_any_number: "object | None" = None
    # Cast permissions (``effects.generic.roto_mana``): "You may cast
    # spells from your hand without paying their mana costs."
    # (Omniscience); "Any player may cast creature spells with mana value
    # N or less without paying their mana costs and as though they had
    # flash." (Aluren: N); "Rather than pay the mana cost for a spell, its
    # controller may discard a card that shares a color with that
    # spell." (Dream Halls, ``spell_costs.alternative_costs_for``).
    grants_free_casts_from_hand: bool = False
    free_flash_creature_casts_max_mv: "int | None" = None
    grants_discard_alternative_cost: bool = False
    # Mana triggers: "Whenever you tap this land for mana, <effect>."
    # (Forbidden Orchard; ``TriggeredAbility`` entries stacked by
    # ``roto_mana.after_mana_ability`` / automatic payment) and "Whenever
    # you tap a nonland permanent for mana, add one mana of any type that
    # permanent produced." (Kinnan, Bonder Prodigy).
    tapped_for_mana_triggers: tuple = ()
    extra_mana_when_you_tap_nonland_for_mana: bool = False
    # Vintage Roto P5 (board-state prison & layers): static abilities
    # that change the board's rules -- attack / block restrictions,
    # untap-step limits, ... -- as rule objects read through
    # ``mtgz_ir.roto_board_statics`` (which honors "loses all
    # abilities" and Blood Moon's ability loss for the source).
    board_rules: tuple = ()
    # Escape whose exile part is "Exile any number of other cards from
    # your graveyard with N or more card types among them." (Nethergoyf):
    # N; 0 = the plain "Exile <count> other cards" form
    # (``escape_exile_count``). Read by ``alt_casting``'s escape helpers.
    escape_exile_card_types: int = 0
    # --- Conspiracies (CR 313; roto P6) ----------------------------
    # The static abilities of a conspiracy card (``card_types ==
    # ("Conspiracy",)``), which function from the command zone, where
    # the card starts the game face up (CR 313.2): tuple of
    # ``mtgz_ir.conspiracies`` rules (``YouAreTheStartingPlayer``,
    # ``MinimumDeckSizeChange``) applied by ``simulate.setup_game``.
    conspiracy_rules: tuple = ()
    # Printed static prevention / replacement effects modeled by roto
    # P6: ``effects.generic.roto_damage.PreventAllDamageToSelf`` ("Prevent
    # all damage that would be dealt to <this creature>.", read by the
    # damage routing hooks) and
    # ``roto_keywords.FewerMinusOneCountersOnYourCreatures`` (Vizier of
    # Remedies, read by ``game.add_minus_one_counters``).
    replacement_statics: tuple = ()
    # Companion (CR 702.139): "Companion — <condition>", a keyword
    # ability that functions outside the game, with its deck condition
    # (``effects.deck_conditions``). Data for the card IR only: this
    # engine doesn't play companions (no game code reads it).
    companion: "Companion | None" = None
    # "This creature enters with a +1/+1 counter on it for each instant and
    # sorcery card exiled with it." (Murktide Regent): an ObjectPredicate
    # over the cards exiled to pay the cost of the spell that became this
    # permanent (its delve; CR 607.2q links the two abilities, 614.1c: it
    # enters with them). Not cast, or no matching card exiled: no counter.
    # Data for the card IR only: this engine doesn't play it (entering
    # with it raises; SEARCH_LIMITATIONS, "Legacy pool" 1).
    enters_with_counter_per_card_exiled_with_it: "object | None" = None

    def __post_init__(self) -> None:
        # Runs for every construction path, including
        # ``dataclasses.replace`` (the runtime copy/token printing
        # rewrites in ``actions``/``effects.generic``), so a malformed
        # ``static_abilities`` can never reach the battlefield.
        _validate_static_abilities(self.name, self.static_abilities)
        # String discriminants are closed vocabularies
        # (``mtgz_ir.effects.vocab``).
        object.__setattr__(self, "aura_enchant_type", EnchantKind(self.aura_enchant_type))
        object.__setattr__(
            self, "enters_with_counters_from_x",
            coerce(XCounterMode, self.enters_with_counters_from_x),
        )
        object.__setattr__(self, "enters_as_copy_zone", ZoneName(self.enters_as_copy_zone))
        # Keyword sets hold engine keyword spellings from the closed
        # vocabulary (``mtgz_ir.keyword_vocab``).
        validate_keywords(self.name, self.keywords)
        validate_keywords(self.name, self.stationed_keywords)
        validate_keywords(self.name, self.enters_as_copy_add_keywords)
        if self.creature_during_your_turn is not None:
            validate_keywords(self.name, self.creature_during_your_turn[3])
        if self.dynamic_pt_formula is not None and not isinstance(
            self.dynamic_pt_formula, PTFormula,
        ):
            raise TypeError(
                f"{self.name}: dynamic_pt_formula must be a PTFormula, got "
                f"{self.dynamic_pt_formula!r}"
            )

    @property
    def is_land(self) -> bool:
        return "Land" in self.card_types

    @property
    def is_creature(self) -> bool:
        return "Creature" in self.card_types

    @property
    def is_basic_land(self) -> bool:
        return self.is_land and "Basic" in self.supertypes

    @property
    def is_legendary(self) -> bool:
        return "Legendary" in self.supertypes

    @property
    def is_token(self) -> bool:
        return "Token" in self.supertypes

    @property
    def is_instant(self) -> bool:
        return "Instant" in self.card_types

    @property
    def is_sorcery(self) -> bool:
        return "Sorcery" in self.card_types

    @property
    def is_artifact(self) -> bool:
        return "Artifact" in self.card_types

    @property
    def is_enchantment(self) -> bool:
        return "Enchantment" in self.card_types

    @property
    def is_spacecraft(self) -> bool:
        return "Spacecraft" in self.subtypes

    @property
    def has_station(self) -> bool:
        """Station (CR 702.184a): every Spacecraft, and a card whose
        printing carries a ``station_threshold`` (the planets, lands with
        station and no power/toughness box)."""
        return self.station_threshold is not None or self.is_spacecraft

    @property
    def is_aura(self) -> bool:
        return "Aura" in self.subtypes

    @property
    def is_saga(self) -> bool:
        return self.chapter_count > 0

    @property
    def is_permanent(self) -> bool:
        """Resolves onto the battlefield when cast."""
        return is_permanent_card_types(self.card_types)

    @property
    def is_planeswalker(self) -> bool:
        return "Planeswalker" in self.card_types

    @property
    def is_room(self) -> bool:
        """A Duskmourn Room (Enchantment — Room) — has two halves."""
        return bool(self.room_halves)

    @property
    def is_conspiracy(self) -> bool:
        """A conspiracy card (CR 313): never a permanent, never cast;
        it starts the game in the command zone."""
        return "Conspiracy" in self.card_types


_KNOWN_SUPERTYPES = {"Basic", "Legendary", "Snow", "World", "Token"}


def _parse_type_line(
    type_line: str,
) -> tuple[tuple[str, ...], tuple[str, ...], tuple[str, ...]]:
    left, _, right = type_line.partition("—")
    left_words = left.strip().split()
    subtypes = tuple(right.strip().split()) if right else ()
    supertypes = tuple(w for w in left_words if w in _KNOWN_SUPERTYPES)
    card_types = tuple(w for w in left_words if w not in _KNOWN_SUPERTYPES)
    return supertypes, card_types, subtypes


def make_creature(
    name: str,
    cost: str,
    power: int,
    toughness: int,
    keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    triggered_abilities: tuple[TriggeredAbility, ...] = (),
    static_abilities: tuple[StaticAbility, ...] = (),
    activated_abilities: tuple[ActivatedAbility, ...] = (),
    warp: str | None = None,
    legendary: bool = False,
    ward: str | None = None,
    ward_discard_random: bool = False,
    ward_discard_chosen: bool = False,
    ward_life_cost: int = 0,
    chooses_player_as_enters: bool = False,
    grants_additional_land_per_turn: int = 0,
    grants_play_lands_from_graveyard: bool = False,
    grants_look_at_top_of_library: bool = False,
    grants_cast_artifact_from_top_of_library: bool = False,
    grants_food_to_your_artifacts: bool = False,
    grants_food_ability_to_your_artifacts: bool = False,
    grants_blue_mana_ability_to_your_artifacts: bool = False,
    hand_ability_grants: tuple[HandAbilityGrant, ...] = (),
    artifact: bool = False,
    cost_reductions: tuple[CostReduction, ...] = (),
    external_cost_reductions: tuple[ExternalCostReduction, ...] = (),
    enters_with_counters_from_x: str | None = None,
    subtypes: tuple[str, ...] = (),
    dynamic_pt_formula: "PTFormula | None" = None,
    devour_land_multiplier: int = 0,
    nonbasic_lands_are_mountains: bool = False,
    grants_all_basic_land_types_to_lands_you_control: bool = False,
    warp_additional_life_cost: int = 0,
    warp_castable_from_graveyard: bool = False,
    enters_as_copy_of_creature: bool = False,
    protection_from_color: str | None = None,
    impending: str | None = None,
    impending_count: int = 0,
    enchantment: bool = False,
    enters_tapped: bool = False,
    must_be_blocked_if_able: bool = False,
    landcycling: tuple[str, str] | None = None,
    hand_activated_abilities: tuple["ActivatedAbility", ...] = (),
    enters_with_plus_one_counter_if_creature_died_this_turn: bool = False,
    enters_with_minus_one_counters_from_hand_cast: int = 0,
    spell_is_uncounterable: bool = False,
    combat_damage_cant_be_prevented: bool = False,
    grants_toughness_as_damage_for_creatures_you_control: bool = False,
    grants_toughness_as_station_power_for_creatures_you_control: bool = False,
) -> CardPrinting:
    prefix = "Legendary " if legendary else ""
    artifact_word = "Artifact " if artifact else ""
    enchantment_word = "Enchantment " if enchantment else ""
    subtype_suffix = f" — {' '.join(subtypes)}" if subtypes else ""
    type_line = f"{prefix}{artifact_word}{enchantment_word}Creature{subtype_suffix}"
    supertypes, card_types, parsed_subtypes = _parse_type_line(type_line)
    subtypes = parsed_subtypes
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        power=power,
        toughness=toughness,
        keywords=frozenset(keywords),
        ward_cost=parse_mana_cost(ward) if ward else None,
        ward_cost_str=ward or "",
        ward_discard_random=ward_discard_random,
        ward_discard_chosen=ward_discard_chosen,
        ward_life_cost=ward_life_cost,
        chooses_player_as_enters=chooses_player_as_enters,
        grants_additional_land_per_turn=grants_additional_land_per_turn,
        grants_play_lands_from_graveyard=grants_play_lands_from_graveyard,
        grants_look_at_top_of_library=grants_look_at_top_of_library,
        grants_cast_artifact_from_top_of_library=(
            grants_cast_artifact_from_top_of_library
        ),
        grants_food_to_your_artifacts=grants_food_to_your_artifacts,
        grants_food_ability_to_your_artifacts=(
            grants_food_ability_to_your_artifacts
        ),
        grants_blue_mana_ability_to_your_artifacts=(
            grants_blue_mana_ability_to_your_artifacts
        ),
        hand_ability_grants=hand_ability_grants,
        triggered_abilities=tuple(triggered_abilities),
        static_abilities=tuple(static_abilities),
        activated_abilities=tuple(activated_abilities),
        warp_cost=parse_mana_cost(warp) if warp else None,
        warp_cost_str=warp or "",
        cost_reductions=tuple(cost_reductions),
        external_cost_reductions=tuple(external_cost_reductions),
        enters_with_counters_from_x=enters_with_counters_from_x,
        dynamic_pt_formula=dynamic_pt_formula,
        devour_land_multiplier=devour_land_multiplier,
        nonbasic_lands_are_mountains=nonbasic_lands_are_mountains,
        grants_all_basic_land_types_to_lands_you_control=(
            grants_all_basic_land_types_to_lands_you_control
        ),
        warp_additional_life_cost=warp_additional_life_cost,
        warp_castable_from_graveyard=warp_castable_from_graveyard,
        enters_as_copy_of_creature=enters_as_copy_of_creature,
        protection_from_color=protection_from_color,
        impending_cost=parse_mana_cost(impending) if impending else None,
        impending_cost_str=impending or "",
        impending_count=impending_count,
        enters_tapped=enters_tapped,
        must_be_blocked_if_able=must_be_blocked_if_able,
        landcycling=landcycling,
        hand_activated_abilities=tuple(hand_activated_abilities),
        enters_with_plus_one_counter_if_creature_died_this_turn=(
            enters_with_plus_one_counter_if_creature_died_this_turn
        ),
        enters_with_minus_one_counters_from_hand_cast=(
            enters_with_minus_one_counters_from_hand_cast
        ),
        spell_is_uncounterable=spell_is_uncounterable,
        combat_damage_cant_be_prevented=combat_damage_cant_be_prevented,
        grants_toughness_as_damage_for_creatures_you_control=(
            grants_toughness_as_damage_for_creatures_you_control
        ),
        grants_toughness_as_station_power_for_creatures_you_control=(
            grants_toughness_as_station_power_for_creatures_you_control
        ),
    )


def cost(
    mana: str = "",
    tap_self: bool = False,
    sacrifice_self: bool = False,
    remove_plus_one_counter_from_your_creature: bool = False,
) -> ActivationCost:
    """Small constructor for ``ActivationCost`` that handles mana parsing."""
    return ActivationCost(
        remove_plus_one_counter_from_your_creature=(
            remove_plus_one_counter_from_your_creature
        ),
        mana=parse_mana_cost(mana) if mana else None,
        mana_str=mana,
        tap_self=tap_self,
        sacrifice_self=sacrifice_self,
    )


def make_spacecraft(
    name: str,
    cost: str,
    power: int = 0,
    toughness: int = 0,
    station_threshold: int | None = None,
    stationed_keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    legendary: bool = False,
    triggered_abilities: tuple[TriggeredAbility, ...] = (),
    static_abilities: tuple[StaticAbility, ...] = (),
    activated_abilities: tuple[ActivatedAbility, ...] = (),
) -> CardPrinting:
    """Build a Spacecraft card (Artifact — Spacecraft).

    Spacecraft enter as non-creature artifacts. Once they accumulate
    ``station_threshold`` charge counters they become artifact creatures
    with the printed ``power``/``toughness`` and ``stationed_keywords``.
    Tier-gated static abilities (like Lumen-Class Frigate's "2+ anthem")
    use StaticAbility(tier_threshold=N).

    ``station_threshold=None`` produces a Spacecraft that never animates
    into a creature (charge counters accumulate but ``is_stationed``
    always returns False). Used for The Eternity Elevator, which has no
    printed P/T and is permanently a non-creature artifact.

    ``power`` and ``toughness`` default to 0 for non-animating Spacecraft;
    they have no game effect when ``station_threshold`` is None.
    """
    prefix = "Legendary " if legendary else ""
    type_line = f"{prefix}Artifact — Spacecraft"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        power=power,
        toughness=toughness,
        station_threshold=station_threshold,
        stationed_keywords=frozenset(stationed_keywords),
        triggered_abilities=tuple(triggered_abilities),
        static_abilities=tuple(static_abilities),
        activated_abilities=tuple(activated_abilities),
    )


def make_instant(
    name: str, cost: str, effect: Effect,
    cost_reductions: tuple[CostReduction, ...] = (),
    kicker: str | None = None,
    additional_cost_choose_your_creature: bool = False,
    additional_cost_sacrifice_creature_or_enchantment: bool = False,
    static_abilities: tuple["StaticAbility", ...] = (),
    has_convoke: bool = False,
) -> CardPrinting:
    type_line = "Instant"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        spell_effect=effect,
        cost_reductions=tuple(cost_reductions),
        kicker_cost=parse_mana_cost(kicker) if kicker else None,
        kicker_cost_str=kicker or "",
        additional_cost_choose_your_creature=(
            additional_cost_choose_your_creature
        ),
        additional_cost_sacrifice_creature_or_enchantment=(
            additional_cost_sacrifice_creature_or_enchantment
        ),
        static_abilities=tuple(static_abilities),
        has_convoke=has_convoke,
    )


def make_sorcery(
    name: str, cost: str, effect: Effect,
    cost_reductions: tuple[CostReduction, ...] = (),
    additional_cost_sacrifice_creature_or_artifact: bool = False,
    additional_cost_sacrifice_creature_or_enchantment: bool = False,
    static_abilities: tuple["StaticAbility", ...] = (),
    has_convoke: bool = False,
) -> CardPrinting:
    type_line = "Sorcery"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        spell_effect=effect,
        cost_reductions=tuple(cost_reductions),
        additional_cost_sacrifice_creature_or_artifact=(
            additional_cost_sacrifice_creature_or_artifact
        ),
        additional_cost_sacrifice_creature_or_enchantment=(
            additional_cost_sacrifice_creature_or_enchantment
        ),
        static_abilities=tuple(static_abilities),
        has_convoke=has_convoke,
    )


def make_artifact(
    name: str,
    cost: str,
    triggered_abilities: tuple[TriggeredAbility, ...] = (),
    static_abilities: tuple[StaticAbility, ...] = (),
    activated_abilities: tuple[ActivatedAbility, ...] = (),
    legendary: bool = False,
    keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    cost_reductions: tuple[CostReduction, ...] = (),
    paints_all_objects_chosen_color_at_etb: bool = False,
    subtype: str | None = None,
    looks_at_opponents_face_down_creatures: bool = False,
    enters_tapped: bool = False,
    doesnt_untap_during_untap_step: bool = False,
    grants_play_lands_from_graveyard: bool = False,
) -> CardPrinting:
    """Build a non-creature, non-Spacecraft Artifact permanent.

    ``subtype`` (e.g. ``"Clue"``, ``"Food"``) appends an ``- <subtype>``
    suffix to the type line and lands on the printing's ``subtypes``
    tuple. Defaults to None (no subtype). DSK Found Footage is the
    first Clue subtype in the engine pool.

    ``looks_at_opponents_face_down_creatures`` mirrors the Duskmourn
    Found Footage static ``You may look at face-down creatures your
    opponents control any time.`` The flag is informational today —
    the engine doesn't model hidden information for face-down
    permanents (a controller can already inspect any face-down
    permanent), so this flag has no engine consequence beyond fidelity
    of the printing's static ability list. Reserved for future hidden-
    information modeling.
    """
    prefix = "Legendary " if legendary else ""
    if subtype is not None:
        type_line = f"{prefix}Artifact — {subtype}"
    else:
        type_line = f"{prefix}Artifact"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        keywords=frozenset(keywords),
        triggered_abilities=tuple(triggered_abilities),
        static_abilities=tuple(static_abilities),
        activated_abilities=tuple(activated_abilities),
        cost_reductions=tuple(cost_reductions),
        paints_all_objects_chosen_color_at_etb=(
            paints_all_objects_chosen_color_at_etb
        ),
        looks_at_opponents_face_down_creatures=(
            looks_at_opponents_face_down_creatures
        ),
        enters_tapped=enters_tapped,
        doesnt_untap_during_untap_step=doesnt_untap_during_untap_step,
        grants_play_lands_from_graveyard=grants_play_lands_from_graveyard,
    )


def make_equipment(
    name: str,
    cost: str,
    power_bonus: int = 0,
    toughness_bonus: int = 0,
    granted_keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    triggered_abilities: tuple[TriggeredAbility, ...] = (),
    activated_abilities: tuple[ActivatedAbility, ...] = (),
    equip_cost: str | None = None,
    grants_control_opponent_ability_when_equipped: bool = False,
    legendary: bool = False,
) -> CardPrinting:
    """Build an Artifact — Equipment card.

    ``power_bonus``/``toughness_bonus``/``granted_keywords`` define the
    continuous bonus granted to the equipped creature via the Equipment
    subsystem. ``keywords`` are on the Equipment itself (e.g., Flash).

    ``equip_cost`` (e.g. ``"{3}"``) wires the printed "Equip {N}" activated
    ability per rule 702.6: a sorcery-speed activated ability whose cost is
    the printed mana cost and whose effect re-attaches this Equipment to a
    target creature its controller controls. Omit it for Equipment that
    only auto-attaches via an ETB trigger and has no printed Equip cost.
    The synthesized ability is appended after any explicitly supplied
    ``activated_abilities``.
    """
    prefix = "Legendary " if legendary else ""
    type_line = f"{prefix}Artifact — Equipment"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    activated = tuple(activated_abilities)
    if equip_cost is not None:
        activated = activated + (
            ActivatedAbility(
                cost=ActivationCost(
                    mana=parse_mana_cost(equip_cost),
                    mana_str=equip_cost,
                ),
                effect=EquipToTargetCreatureYouControl(),
                sorcery_speed=True,
            ),
        )
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        keywords=frozenset(keywords),
        triggered_abilities=tuple(triggered_abilities),
        activated_abilities=activated,
        grants_control_opponent_ability_when_equipped=(
            grants_control_opponent_ability_when_equipped
        ),
        equipment_bonuses=(
            power_bonus, toughness_bonus, frozenset(granted_keywords)
        ),
    )


def make_aura(
    name: str,
    cost: str,
    power_bonus: int = 0,
    toughness_bonus: int = 0,
    granted_keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    triggered_abilities: tuple[TriggeredAbility, ...] = (),
    aura_enchant_type: str = "creature",
    aura_yours_only: bool = False,
    attached_prevents_untap: bool = False,
    attached_grants_control: bool = False,
    attached_sets_base_pt: "tuple[int, int] | None" = None,
    attached_grants_lose_all_abilities: bool = False,
    attached_cant_be_blocked_by_more_than_one: bool = False,
    attached_grants_subtype: "str | None" = None,
    replaces_first_tokens_with_copies: bool = False,
    attached_grants_ward: str | None = None,
) -> CardPrinting:
    """Build an Enchantment — Aura card that attaches to a creature.

    ``power_bonus``/``toughness_bonus``/``granted_keywords`` define the
    continuous bonus granted to the enchanted creature; we reuse the
    Equipment bonus machinery (equipment_bonuses) so effective_* helpers
    pick them up automatically.

    ``keywords`` are on the Aura ITSELF (e.g. Flash on Frantic Strength /
    Shardmage's Rescue / Growing Dread). Mirrors ``make_equipment``'s
    same-named kwarg and routes through the same ``CardPrinting.keywords``
    field that gates instant-speed casting in ``legal.py``.

    ``attached_grants_ward`` (e.g. ``"{1}"``) wires the "Enchanted
    permanent has ward {cost}" rider — Hardlight Containment.

    Simplified Aura behavior: cast-time target must be a creature; on
    resolution the Aura enters attached to that target (or goes to the
    graveyard if the target is no longer legal). When the enchanted
    creature leaves the battlefield, the Aura is sent to the graveyard
    (unlike Equipment, which falls off but stays).
    """
    type_line = "Enchantment — Aura"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        triggered_abilities=tuple(triggered_abilities),
        keywords=frozenset(keywords),
        equipment_bonuses=(
            power_bonus, toughness_bonus, frozenset(granted_keywords)
        ),
        aura_enchant_type=aura_enchant_type,
        aura_yours_only=aura_yours_only,
        attached_prevents_untap=attached_prevents_untap,
        attached_grants_control=attached_grants_control,
        attached_sets_base_pt=attached_sets_base_pt,
        attached_grants_lose_all_abilities=attached_grants_lose_all_abilities,
        attached_cant_be_blocked_by_more_than_one=attached_cant_be_blocked_by_more_than_one,
        attached_grants_subtype=attached_grants_subtype,
        replaces_first_tokens_with_copies=replaces_first_tokens_with_copies,
        attached_grants_ward_cost=(
            parse_mana_cost(attached_grants_ward)
            if attached_grants_ward else None
        ),
        attached_grants_ward_str=attached_grants_ward or "",
    )


def make_planeswalker(
    name: str,
    cost: str,
    loyalty: int,
    subtype: str | None,
    triggered_abilities: tuple[TriggeredAbility, ...] = (),
    static_abilities: tuple[StaticAbility, ...] = (),
    activated_abilities: tuple[ActivatedAbility, ...] = (),
    legendary: bool = True,
    keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    opponents_cant_draw_more_than_one_card_each_turn: bool = False,
    opponents_cast_spells_only_at_sorcery_speed: bool = False,
    loyalty_abilities_at_instant_speed_if_entered_this_turn: bool = False,
) -> CardPrinting:
    """Build a Legendary Planeswalker card. Loyalty is the printed
    starting loyalty; it initializes ``GameObject.loyalty_counters``
    when the planeswalker enters the battlefield. SBAs send it to
    graveyard at 0 loyalty.

    ``subtype`` may be ``None`` / empty for a planeswalker printed with
    no planeswalker type (The Wandering Emperor's bare "Legendary
    Planeswalker" type line). ``keywords`` carries printed keyword
    lines (Flash). The remaining flags are the planeswalker statics
    documented on ``CardPrinting``.

    Loyalty-activated abilities ARE modeled via ``activated_abilities``
    carrying ``ActivationCost.loyalty`` (signed cost) + ``sorcery_speed``
    + the once-per-turn-per-walker gate (CR 606.3). Tezzeret, Cruel
    Captain's 0 / −3 abilities are wired; the −7 emblem is deferred (no
    emblem-object substrate — see docs/SEARCH_LIMITATIONS.md).
    Simplifications still in place: damage redirection to planeswalkers
    isn't modeled; attacking a planeswalker isn't modeled. Triggered
    abilities on the walker still fire normally.
    """
    prefix = "Legendary " if legendary else ""
    type_line = (
        f"{prefix}Planeswalker — {subtype}" if subtype
        else f"{prefix}Planeswalker"
    )
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        keywords=frozenset(keywords),
        triggered_abilities=tuple(triggered_abilities),
        static_abilities=tuple(static_abilities),
        activated_abilities=tuple(activated_abilities),
        loyalty=loyalty,
        opponents_cant_draw_more_than_one_card_each_turn=(
            opponents_cant_draw_more_than_one_card_each_turn
        ),
        opponents_cast_spells_only_at_sorcery_speed=(
            opponents_cast_spells_only_at_sorcery_speed
        ),
        loyalty_abilities_at_instant_speed_if_entered_this_turn=(
            loyalty_abilities_at_instant_speed_if_entered_this_turn
        ),
    )


def make_emblem(
    name: str,
    triggered_abilities: tuple[TriggeredAbility, ...] = (),
    static_abilities: tuple[StaticAbility, ...] = (),
) -> CardPrinting:
    """Build the synthetic ``CardPrinting`` backing an emblem (CR 114).

    An emblem "has no characteristics other than the abilities defined
    by the effect that created it" (CR 114.3): no mana cost, no card
    types, no power/toughness, no color. We model it as a synthetic
    ``CardPrinting`` whose only meaningful content is the
    ``triggered_abilities`` / ``static_abilities`` the creating effect
    stamps on it — the same ability shapes a permanent carries, so the
    existing trigger-dispatch + resolve machinery works unchanged once
    the emblem object is scanned as a trigger source. The ``name`` is a
    short descriptor (e.g. ``"Tezzeret, Cruel Captain emblem"``) used in
    log lines and snapshots.

    Mirrors Rust's emblem-printing construction (the ``CreateEmblem``
    effect registers an analogous synthetic printing). Card-type set is
    empty: ``is_creature`` / ``is_artifact`` / ``is_planeswalker`` all
    read False, the SBA walks skip it (it's never on the battlefield),
    and it never reads as a permanent.
    """
    supertypes, card_types, subtypes = _parse_type_line("")
    return CardPrinting(
        name=name,
        mana_cost_str="",
        mana_cost=parse_mana_cost(""),
        type_line="",
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        triggered_abilities=tuple(triggered_abilities),
        static_abilities=tuple(static_abilities),
    )


def make_enchantment(
    name: str,
    cost: str,
    triggered_abilities: tuple[TriggeredAbility, ...] = (),
    static_abilities: tuple[StaticAbility, ...] = (),
    activated_abilities: tuple[ActivatedAbility, ...] = (),
    legendary: bool = False,
    restricts_noncreature_spells_to_one_per_turn: bool = False,
    grants_first_spell_each_turn_free: bool = False,
    warp: str | None = None,
    grants_etb_counter_per_land_entered_this_turn: bool = False,
    nonbasic_lands_are_mountains: bool = False,
    wipes_all_creature_abilities: bool = False,
    sets_all_creature_pt_to_1_1: bool = False,
    ward: str | None = None,
) -> CardPrinting:
    """Build a non-Aura Enchantment permanent. Casts onto the battlefield
    like any other permanent and sits there firing triggers or providing
    static bonuses. Auras (enchant creature) aren't modeled yet.

    ``ward`` (e.g. ``"{2}"``) wires the printed Ward keyword on the
    enchantment ITSELF (Trapped in the Screen). Mirrors the same-named
    kwarg on ``make_creature`` and routes through the same
    ``CardPrinting.ward_cost`` / ``ward_cost_str`` fields the engine's
    ward-check helpers (``effective_ward_cost``,
    ``_apply_ward_check``) already read. Auras carry their own
    ``attached_grants_ward`` kwarg on ``make_aura`` for the "Enchanted
    permanent has ward" rider; that targets the enchanted permanent
    rather than the Aura itself, so the two kwargs stay distinct.
    """
    prefix = "Legendary " if legendary else ""
    type_line = f"{prefix}Enchantment"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str=cost,
        mana_cost=parse_mana_cost(cost),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        triggered_abilities=tuple(triggered_abilities),
        static_abilities=tuple(static_abilities),
        activated_abilities=tuple(activated_abilities),
        restricts_noncreature_spells_to_one_per_turn=(
            restricts_noncreature_spells_to_one_per_turn
        ),
        grants_first_spell_each_turn_free=(
            grants_first_spell_each_turn_free
        ),
        warp_cost=parse_mana_cost(warp) if warp else None,
        warp_cost_str=warp or "",
        grants_etb_counter_per_land_entered_this_turn=(
            grants_etb_counter_per_land_entered_this_turn
        ),
        nonbasic_lands_are_mountains=nonbasic_lands_are_mountains,
        wipes_all_creature_abilities=wipes_all_creature_abilities,
        sets_all_creature_pt_to_1_1=sets_all_creature_pt_to_1_1,
        ward_cost=parse_mana_cost(ward) if ward else None,
        ward_cost_str=ward or "",
    )


def make_room(
    half_a: RoomHalf,
    half_b: RoomHalf,
    legendary: bool = False,
) -> CardPrinting:
    """Build a Duskmourn Room card (``Enchantment — Room // Enchantment — Room``).

    The combined-card name is ``"<A.name> // <B.name>"`` (Scryfall
    convention). The printing-level ``mana_cost`` and ``mana_cost_str``
    are the SUM of the two halves' costs — used only for things like
    ``cmc`` reporting; cast time uses the chosen half's cost via
    ``CastSpell``-with-half resolution. Each half's abilities are
    flattened onto the printing's ``triggered_abilities`` /
    ``static_abilities`` / ``activated_abilities`` tuples with
    ``room_half`` tagged so the engine's effective-ability iterators
    can gate them on ``unlocked_halves``.

    Simplification: the cast flow takes the controller's chosen half's
    cost; the half-choice is wired in ``_cast_spell`` via the
    ``cast_room_half`` argument on ``CastSpell``.
    """
    name = f"{half_a.name} // {half_b.name}"
    prefix = "Legendary " if legendary else ""
    type_line = f"{prefix}Enchantment — Room"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    # Tag each half's abilities with its ``room_half`` name and flatten
    # them onto the parent printing. Use ``dataclasses.replace`` so the
    # frozen dataclass instances on the half (which the half itself
    # also retains, untagged) get a tagged copy on the parent.
    from dataclasses import replace as _dc_replace

    triggered: list[TriggeredAbility] = []
    static: list[StaticAbility] = []
    activated: list[ActivatedAbility] = []
    for half in (half_a, half_b):
        for t in half.triggered_abilities:
            triggered.append(_dc_replace(t, room_half=half.name))
        for s in half.static_abilities:
            static.append(_dc_replace(s, room_half=half.name))
        for a in half.activated_abilities:
            activated.append(_dc_replace(a, room_half=half.name))
    # Combined mana cost: stored as a string of the two halves joined
    # by ``//`` (matches Scryfall's printed form). The parsed
    # ``mana_cost`` uses the COMBINED cost (sum of the two halves) so
    # any caller that asks for printing.mana_cost.total gets the
    # full-card mana value. Cast-time payment doesn't touch this
    # printing-level cost — it uses the chosen half's cost directly.
    combined_str = f"{half_a.mana_cost_str} // {half_b.mana_cost_str}"
    summed_generic = half_a.mana_cost.generic + half_b.mana_cost.generic
    summed_colored: dict[str, int] = {}
    for color, n in half_a.mana_cost.colored:
        summed_colored[color] = summed_colored.get(color, 0) + n
    for color, n in half_b.mana_cost.colored:
        summed_colored[color] = summed_colored.get(color, 0) + n
    summed_cost = ManaCost(
        generic=summed_generic,
        colored=tuple(sorted(summed_colored.items())),
    )
    return CardPrinting(
        name=name,
        mana_cost_str=combined_str,
        mana_cost=summed_cost,
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        triggered_abilities=tuple(triggered),
        static_abilities=tuple(static),
        activated_abilities=tuple(activated),
        room_halves=(half_a, half_b),
    )


def make_token_creature(
    name: str,
    power: int,
    toughness: int,
    subtypes: tuple[str, ...] = (),
    keywords: frozenset[str] | set[str] | tuple[str, ...] = (),
    artifact: bool = False,
    activated_abilities: tuple[ActivatedAbility, ...] = (),
    can_only_block_creatures_with_flying: bool = False,
    legendary: bool = False,
    static_abilities: tuple[StaticAbility, ...] = (),
    colors: frozenset[str] | set[str] | tuple[str, ...] = (),
    enchantment: bool = False,
) -> CardPrinting:
    """Build a CardPrinting representing a creature token.

    Tokens have no mana cost; they're created by effects rather than cast.
    The "Token" supertype distinguishes them so the engine can remove them
    from any non-battlefield zone per MTG rule 704.5d. Setting
    ``artifact=True`` adds the Artifact card type (e.g. for EoE's colorless
    Robot tokens). ``can_only_block_creatures_with_flying`` plumbs the
    block-restriction flag onto the resulting printing — used by the EoE
    Drone token. ``legendary=True`` adds the Legendary supertype (Minsc &
    Boo's "Boo, a legendary 1/1 red Hamster creature token") so the legend
    rule applies to it.
    """
    subtypes_str = " ".join(subtypes)
    legend = "Legendary " if legendary else ""
    extra_type = ("Artifact " if artifact else "") + (
        "Enchantment " if enchantment else ""
    )
    type_line = (
        f"{legend}Token {extra_type}Creature — {subtypes_str}"
        if subtypes else f"{legend}Token {extra_type}Creature"
    )
    supertypes, card_types, subtypes_tup = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes_tup,
        power=power,
        toughness=toughness,
        keywords=frozenset(keywords),
        activated_abilities=tuple(activated_abilities),
        static_abilities=tuple(static_abilities),
        can_only_block_creatures_with_flying=(
            can_only_block_creatures_with_flying
        ),
        color_indicator=frozenset(colors),
    )


def make_planet_land(
    name: str, color: str,
    station_threshold: int | None = None,
    activated_abilities: tuple = (),
) -> CardPrinting:
    """EoE Planet land: enters tapped, taps for a single color, and has
    station (CR 702.184a) when ``station_threshold`` is set: its highest
    striation, "N+ | <ability>" (the ability carries ``tier_threshold``).
    A planet has no power/toughness box (CR 722.1), so it never becomes
    a creature (``game.is_stationed``).
    """
    type_line = f"Land — Planet"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        produces=frozenset({color}),
        enters_tapped=True,
        station_threshold=station_threshold,
        activated_abilities=tuple(activated_abilities),
    )


def make_colorless_land(
    name: str, subtypes_str: str = "", legendary: bool = False,
) -> CardPrinting:
    """A non-basic land that taps for {C} only. Useful for utility
    lands whose activated abilities aren't modeled — taps-for-generic
    still works via auto_tap_for_cost's generic-mana fallback.
    """
    prefix = "Legendary " if legendary else ""
    type_line = (
        f"{prefix}Land — {subtypes_str}" if subtypes_str else f"{prefix}Land"
    )
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        produces=frozenset(),  # generic-only
    )


def make_utility_land(
    name: str,
    produces: "frozenset[str]" = frozenset(),
    subtypes_str: str = "",
    legendary: bool = False,
    triggered_abilities: tuple = (),
    activated_abilities: tuple = (),
    enters_tapped: bool = False,
    keywords: "frozenset[str] | set[str] | tuple[str, ...]" = (),
    optional_copy_land_from_graveyard: bool = False,
    enters_tapped_unless_any_player_life_at_most: int = 0,
    enters_tapped_unless: "Condition | None" = None,
    mana_only_from_produces: bool = False,
    produces_life_cost: int = 0,
    hand_activated_abilities: tuple = (),
    shock_land_life_cost: int = 0,
    enters_choosing_basic_land_type: bool = False,
    is_chosen_basic_land_type: bool = False,
    enters_with_named_counters: tuple = (),
    produces_with_counter: "tuple[str, frozenset[str]] | None" = None,
    opening_hand_action: "object | None" = None,
) -> CardPrinting:
    """Build a non-basic land with arbitrary activated/triggered
    abilities and an explicit produces set. Used by Stellar Sights."""
    prefix = "Legendary " if legendary else ""
    type_line = (
        f"{prefix}Land — {subtypes_str}" if subtypes_str else f"{prefix}Land"
    )
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        produces=frozenset(produces),
        triggered_abilities=tuple(triggered_abilities),
        activated_abilities=tuple(activated_abilities),
        enters_tapped=enters_tapped,
        keywords=frozenset(keywords),
        optional_copy_land_from_graveyard=optional_copy_land_from_graveyard,
        enters_tapped_unless_any_player_life_at_most=(
            enters_tapped_unless_any_player_life_at_most
        ),
        enters_tapped_unless=enters_tapped_unless,
        mana_only_from_produces=mana_only_from_produces,
        produces_life_cost=produces_life_cost,
        hand_activated_abilities=tuple(hand_activated_abilities),
        shock_land_life_cost=shock_land_life_cost,
        enters_choosing_basic_land_type=enters_choosing_basic_land_type,
        is_chosen_basic_land_type=is_chosen_basic_land_type,
        enters_with_named_counters=tuple(enters_with_named_counters),
        produces_with_counter=produces_with_counter,
        opening_hand_action=opening_hand_action,
    )


def make_shock_land(
    name: str, colors: tuple[str, str], subtypes_str: str,
    life_cost: int = 2,
) -> CardPrinting:
    """Build a shock-land-style dual. ``subtypes_str`` is the type-line
    suffix (e.g. "Forest Island"). The land produces both ``colors`` and
    enters tapped unless the controller pays ``life_cost`` life on ETB.
    """
    type_line = f"Land — {subtypes_str}"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        produces=frozenset(colors),
        shock_land_life_cost=life_cost,
    )


def make_basic_land(name: str, color: str, type_line: str | None = None) -> CardPrinting:
    # The five basics carry their own name as their land type. Wastes is the
    # exception: a Basic Land with no subtype, so its caller passes the
    # printed type line rather than inventing a "Wastes" land type (which
    # Domain and "basic land type" counts would otherwise see).
    if type_line is None:
        type_line = f"Basic Land — {name}"
    supertypes, card_types, subtypes = _parse_type_line(type_line)
    return CardPrinting(
        name=name,
        mana_cost_str="",
        mana_cost=ManaCost(),
        type_line=type_line,
        supertypes=supertypes,
        card_types=card_types,
        subtypes=subtypes,
        produces=frozenset({color}),
    )


# Pre-built activated ability granted to artifacts by Ragost, Deft
# Gastronaut's static "{2}, {T}, Sacrifice this artifact: You gain 3
# life." Exposed here so game.effective_activated_abilities can append
# it without each call-site re-importing effect classes.
RAGOST_FOOD_GRANTED_ABILITY = ActivatedAbility(
    cost=ActivationCost(
        mana=parse_mana_cost("{2}"),
        mana_str="{2}",
        tap_self=True,
        sacrifice_self=True,
    ),
    effect=GainLife(amount=3),
)


# Pre-built activated ability granted to artifacts by Mm'menon, the
# Right Hand's static: "Artifacts you control have '{T}: Add {U}.
# Spend this mana only to cast a spell from anywhere other than your
# hand.'" The "outside-of-hand" spend restriction is enforced via the
# ``"cast_from_outside_hand"`` coin tag — see
# ``actions._coin_satisfies_context``.
MMNEMON_BLUE_MANA_GRANTED_ABILITY = ActivatedAbility(
    cost=ActivationCost(tap_self=True),
    effect=AddManaToPool(
        produces=(("U", 1),),
        spend_restriction="cast_from_outside_hand",
    ),
    is_mana_ability=True,
)


# Pre-built activated ability granted to the equipped creature by
# The Dominion Bracelet: "{15}, Exile The Dominion Bracelet: You
# control target opponent during their next turn. This ability costs
# {X} less to activate, where X is this creature's power. Activate
# only as a sorcery."
DOMINION_BRACELET_GRANTED_ABILITY = ActivatedAbility(
    cost=ActivationCost(
        mana=parse_mana_cost("{15}"),
        mana_str="{15}",
        exile_granting_equipment=True,
        mana_reduced_by_source_power=True,
    ),
    effect=ControlTargetOpponentNextTurn(),
    sorcery_speed=True,
)
