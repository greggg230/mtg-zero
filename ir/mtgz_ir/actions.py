"""Action types, mana payment, and action application."""

from __future__ import annotations

from dataclasses import dataclass

from mtgz_ir.cards import ManaCost
from mtgz_ir.effects.vocab import plain
from mtgz_ir.game import (
    GameObject, GameState, StackItem, Zone, effective_colors,
    effective_land_produces, has_card_type, is_artifact_now,
)
from mtgz_ir.keyword_rules import (
    filter_targetable, targets_all_illegal_on_resolution,
)


def _format_log_targets(state: GameState, targets: tuple) -> str:
    """Return a ``" targeting Goblin Runner, Player 1"`` suffix for a
    log line announcing a spell or triggered ability going on the
    stack. Returns an empty string when there are no targets so the
    caller can unconditionally append it.

    Permanent ids resolve via the battlefield; spell/ability ids fall
    back to the stack (rare but possible for triggers that target
    in-flight spells). Unknown ids stringify as ``"?{kind}#{id}"`` —
    visible enough that a missing target shows up as a UI bug rather
    than a silent gap."""
    if not targets:
        return ""
    parts: list[str] = []
    for kind, tid in targets:
        if kind == "player":
            parts.append(f"Player {tid}")
            continue
        if kind == "stack":
            # Counter-spell-style targets: ``tid`` is a stack INDEX,
            # not an object id. Resolve via state.stack[index].
            if 0 <= tid < len(state.stack):
                parts.append(state.stack[tid].source.printing.name)
                continue
        obj = next((o for o in state.battlefield if o.id == tid), None)
        if obj is not None:
            parts.append(obj.printing.name)
            continue
        on_stack = next((s for s in state.stack if s.source.id == tid), None)
        if on_stack is not None:
            parts.append(on_stack.source.printing.name)
            continue
        if kind == "exile_card":
            in_exile = next(
                (c for p in state.players for c in p.exile if c.id == tid),
                None,
            )
            if in_exile is not None:
                parts.append(f"{in_exile.printing.name} (exiled)")
                continue
        parts.append(f"?{kind}#{tid}")
    return " targeting " + ", ".join(parts)


@dataclass(frozen=True)
class Action:
    pass


@dataclass(frozen=True)
class PassPriority(Action):
    pass


@dataclass(frozen=True)
class PlayLand(Action):
    card_id: int


@dataclass(frozen=True)
class CastSpell(Action):
    card_id: int
    # If True, cast from hand for the card's warp alt-cost (and trigger an
    # end-of-turn exile). If False (default), cast at the normal cost — from
    # hand OR from the "warped" exile zone if the card is sitting there.
    use_warp: bool = False
    # If True, pay the card's kicker cost on top of its base cost.
    # Only legal when the card has a kicker_cost.
    kicked: bool = False
    # If True, cast without paying any mana cost (Weftwalking's "first
    # spell each player casts during each of their turns may be cast
    # without paying its mana cost"). Mutually exclusive with use_warp
    # and kicked. Targets/additional costs are still paid; only the
    # mana cost is skipped.
    cast_for_free: bool = False
    # If True, cast from hand for the card's Impending alt-cost (the
    # Duskmourn Overlord cycle's deferred-cast mechanic). The card
    # enters the battlefield with ``printing.impending_count`` time
    # counters and ``impending_active=True`` — its printed creature
    # type is suppressed via ``is_creature_now`` until the last time
    # counter is removed at the controller's end step. Sorcery-speed
    # only (gated in ``legal_actions``); mutually exclusive with
    # ``use_warp``, ``kicked``, and ``cast_for_free``. The Overlord's
    # printed cost remains castable for the normal "enters as a
    # creature" line — flipping ``use_impending`` selects which arm
    # runs.
    use_impending: bool = False
    # DSK Rooms: when casting a Room card, the controller picks one of
    # the two half-names; the Room is cast for that half's mana cost
    # and enters with that half unlocked. ``None`` for non-Room
    # spells. legal_actions emits one ``CastSpell(card_id,
    # cast_room_half=name)`` per half whose cost is payable.
    cast_room_half: "str | None" = None
    # Flashback (CR 702.34): cast the card from its owner's graveyard
    # for its flashback cost; it is exiled when it leaves the stack.
    use_flashback: bool = False
    # Buyback (CR 702.27): pay the optional buyback cost; the spell
    # returns to its owner's hand as it resolves.
    use_buyback: bool = False
    # Plot (CR 702.170): NOT a cast — the plot special action (pay the
    # plot cost, exile the card from hand face up). Encoded as a
    # CastSpell variant so the existing action vocabulary covers it.
    use_plot: bool = False
    # Escape (CR 702.138): cast from the graveyard for the escape cost.
    use_escape: bool = False
    # Dash (CR 702.109): cast from hand for the dash cost; the permanent
    # gains haste and returns to hand at the next end step.
    use_dash: bool = False
    # Evoke (CR 702.74; ``mtgz_ir.evoke``): cast from hand for the evoke
    # cost; the permanent is sacrificed as it enters.
    use_evoke: bool = False
    # Cast from the graveyard under a W/G graveyard permission
    # (``mtgz_ir.graveyard_casting_wg``): "retrace" or "paragon".
    graveyard_permission: "str | None" = None
    # Bestow (CR 702.103): cast from hand for the bestow cost as an Aura
    # spell enchanting a creature.
    use_bestow: bool = False
    # Emerge (CR 702.119): cast from hand for the emerge alternative cost,
    # sacrificing a creature / artifact (U/B cube creatures).
    use_emerge: bool = False
    # Cast the card as its named other face (``mtgz_ir.layouts``): one
    # half of a split card (CR 709.3) or a modal DFC's castable back face
    # (CR 712.11). ``None`` casts the card's own (front) printing.
    cast_face: "str | None" = None
    # Suspend (CR 702.62a): NOT a cast -- the special action (pay the
    # suspend cost, exile the card from hand with N time counters;
    # ``roto_mana.suspend_card``), encoded as a CastSpell variant like
    # Plot.
    use_suspend: bool = False


@dataclass(frozen=True)
class UnlockRoomHalf(Action):
    """Pay a Room's locked-half mana cost to unlock it (sorcery speed).

    Per CR 715.3-4: "As a sorcery, the controller of an unlocked Room
    on the battlefield with at least one locked door may unlock a
    locked door of that Room by paying its mana cost as though they
    were casting an enchantment spell." This is NOT a spell — the
    unlock action doesn't go on the stack. The mana cost is paid the
    same way; on apply, the named half is added to ``unlocked_halves``
    on the Room permanent and any newly-active triggered abilities on
    the entering half (e.g. "When you unlock this door, ..." — modeled
    as ENTERS predicates with ``room_half`` filter) fire as the half
    becomes unlocked.

    Future: Eerie observers ("Whenever you fully unlock a Room, ...")
    look for the transition to a fully-unlocked state — for now we
    only emit a ``"Room fully unlocked"`` log line at that transition.
    """

    card_id: int
    half_name: str


@dataclass(frozen=True)
class ActivateAbility(Action):
    """Activate the N-th activated ability on a permanent the player
    controls. Pays the cost and puts the ability on the stack. Ineligibility
    (can't pay, creature summoning-sick for tap-cost abilities, etc.) is
    filtered in ``legal_actions`` and silently no-ops here as defense.
    """

    source_id: int
    ability_index: int


@dataclass(frozen=True)
class ActivateStation(Action):
    """Activate the Station ability of a Spacecraft.

    Cost: tap ``crew_id`` (another untapped creature the player controls).
    Effect: add charge counters to ``spacecraft_id`` equal to the crew's
    power. Sorcery-speed only (enforced by ``legal_actions``).
    """

    spacecraft_id: int
    crew_id: int


@dataclass(frozen=True)
class TurnFaceUp(Action):
    """Turn a face-down permanent face up by paying its underlying
    card's mana cost. CR 701.34 / 707.4: the controller may turn it
    face up at any time as a special action by revealing what it is
    and paying its mana cost. Currently only relevant for Manifest
    Dread / Morph face-down permanents — we accept the action only
    when the underlying ``printing`` is a creature card (CR 701.34
    explicitly: the manifest-side rules require the face-down card to
    be a creature card to turn it face up for its mana cost).

    Special action — does not use the stack. Costs are paid
    immediately and the flip resolves at once.
    """

    card_id: int


@dataclass(frozen=True)
class DeclareAttackers(Action):
    """Declare a set of creatures as attackers during the declare-attackers step.

    Ineligible ids (summoning-sick creatures, tapped creatures, etc.) are
    silently filtered during application.
    """
    attacker_ids: frozenset[int] = frozenset()


@dataclass(frozen=True)
class DeclareBlockers(Action):
    """Declare block assignments during the declare-blockers step.

    ``assignments`` is a tuple of (blocker_id, attacker_id) pairs (tuples
    instead of a dict so the action itself is hashable). Illegal pairs are
    silently filtered during application.
    """
    assignments: tuple[tuple[int, int], ...] = ()


def _untapped_lands(state: GameState, player_id: int) -> list[GameObject]:
    """Untapped lands ``player_id`` controls that automatic cost payment
    may tap, in the order payment should consider them.

    Two printing-level refinements (both no-ops for printings that
    leave the fields at their defaults, i.e. every pre-cube printing):

    - ``mana_only_from_produces``: the land pays only through its
      effective ``produces`` set, so a land with none (a fetch land,
      Gaea's Cradle — whose mana abilities carry costs or scale) is
      not an automatic mana source at all.
    - ``produces_life_cost``: lands that charge life per tap sort
      after the free lands (stable), and only as many of them are
      offered as the controller can pay for while staying above 0
      life.
    """
    lands = [
        o for o in state.battlefield
        if o.controller_id == player_id and o.printing.is_land and not o.tapped
    ]
    # "Activated abilities of artifacts [your opponents control] can't be
    # activated." (Null Rod, Clarion Conqueror, Karn, the Great Creator)
    # locks an artifact land's mana ability too -- under Mycosynth
    # Lattice, every land is an artifact.
    from mtgz_ir.creature_statics_wg import any_activation_locks

    if any_activation_locks(state):
        from mtgz_ir.creature_statics_wg import activation_locked

        lands = [o for o in lands if not activation_locked(state, o)]
    # Hot path (search rollouts): legacy printings skip both refinements.
    if not any(
        o.printing.mana_only_from_produces or o.printing.produces_life_cost
        for o in lands
    ):
        return lands
    lands = [o for o in lands if _land_auto_pays(state, o)]
    if not any(_land_tap_life_cost(state, o) for o in lands):
        return lands
    free = [o for o in lands if not _land_tap_life_cost(state, o)]
    budget = state.player_by_id(player_id).life - 1
    paid: list[GameObject] = []
    for o in lands:
        cost = _land_tap_life_cost(state, o)
        if not cost:
            continue
        if cost <= budget:
            paid.append(o)
            budget -= cost
    return free + paid


def _land_auto_pays(state: GameState, land: GameObject) -> bool:
    """True if automatic payment may tap ``land`` for mana. Legacy
    printings always may (the historical generic-mana fallback);
    ``mana_only_from_produces`` printings only when they produce
    something."""
    if not land.printing.mana_only_from_produces:
        return True
    return bool(effective_land_produces(state, land))


def _land_tap_life_cost(state: GameState, land: GameObject) -> int:
    """Life paid when ``land`` is tapped for its ``produces`` mana.

    A layer-4 type rewrite (Magus of the Moon, Dryad of the Ilysian
    Grove) replaces the land's printed mana abilities with its basic
    land types' intrinsic ``{T}: Add`` — those are free, so the life
    cost only applies while the printed ``produces`` is in force."""
    cost = land.printing.produces_life_cost
    if not cost:
        return 0
    if effective_land_produces(state, land) != land.printing.produces:
        return 0
    return cost


def _coin_matches_color(coin: "ManaCoin", color: str) -> bool:
    """True if ``coin`` can pay for a ``{color}`` requirement (``"S"``:
    an {S} pip, paid only by snow mana)."""
    if color == "S":
        return coin.snow
    if color == "C":
        # {C}: colorless mana only (CR 107.4b).
        return coin.color == "C"
    if coin.color == "any":
        return True
    return coin.color == color


def mana_spendable_as_any_color(state: GameState) -> bool:
    """"Players may spend mana as though it were mana of any color."
    (Mycosynth Lattice; CR 609.4b): a rule modification, active while a
    permanent with the ability is on the battlefield, not phased out, and
    still has its abilities (``roto_board_statics.abilities_suppressed``
    -- unlike the card's layer 4 / 5 effects, a rule modification ends
    when the ability is removed). While it holds, any mana -- colorless
    included -- pays a colored pip; {S} still needs snow mana, and spend
    restrictions still apply."""
    if not state.layered_statics_possible:
        return False
    for o in state.battlefield:
        if not o.printing.mana_spendable_as_any_color or o.phased_out:
            continue
        from mtgz_ir.roto_board_statics import abilities_suppressed

        if not abilities_suppressed(state, o):
            return True
    return False


def _pip_passes(color: str, any_color: bool) -> tuple:
    """The matching passes for one colored pip: exact-color mana first,
    then (under "spend mana as though it were mana of any color") any
    mana. {S} is never widened."""
    if any_color and color != "S":
        return (True, False)
    return (True,)


def _coin_satisfies_context(
    coin: "ManaCoin", spell_kind: "str | frozenset[str]",
) -> bool:
    """True if ``coin``'s spend restriction is compatible with a
    payment in the given context.

    ``spell_kind`` is either a single string (legacy callers) or a
    frozenset of context tags. A single string is treated as a
    one-element set. The coin satisfies the context if its restriction
    is None (unrestricted) or matches at least one of the tags below.

    Known context tags:
      - "spell" (default): only unrestricted coins satisfy.
      - "artifact_spell": adds coins tagged "artifact_spell".
        Set when casting a spell whose printed types include Artifact
        (Steelswarm Operator / Power Depot's first mana ability).
      - "artifact_activation": adds coins tagged "artifact_activation".
        Set when activating an ability on an artifact source
        (Steelswarm Operator's second mana ability: "Spend this mana
        only to activate abilities of artifact sources").
      - "colorless_eldrazi_spell" / "colorless_eldrazi_ability": adds
        coins tagged "colorless_eldrazi" (Eldrazi Temple's second mana
        ability). Set when casting a colorless Eldrazi or activating an
        ability on a colorless-Eldrazi permanent.
      - "legendary_spell": adds coins tagged "legendary_spell". Set
        when casting a legendary spell (Plaza of Heroes's second mana
        ability — "Spend this mana only to cast a legendary spell").
      - "legendary_permanent_ability": adds coins tagged
        "legendary_permanent_ability". Set when activating an ability
        on a legendary permanent (Plaza of Heroes's third mana ability
        — "Add one mana of any color among legendary permanents you
        control"; we read that as "spend only to activate abilities of
        legendary permanents you control", matching how Eldrazi
        Temple's parallel "among colorless Eldrazi" line is modeled).
      - "land_activation": adds coins tagged "land_activation". Set
        when activating an ability on a land source (Sunken Citadel's
        second mana ability — "Spend this mana only to activate
        abilities of land sources").
      - "cast_from_outside_hand": adds coins tagged
        "cast_from_outside_hand" (Mm'menon, the Right Hand's grant —
        "Spend this mana only to cast a spell from anywhere other than
        your hand"). Set in ``_cast_spell`` when the spell is being
        cast from a zone other than the caster's hand (graveyard,
        exile, or top of library).
      - "activated_ability": unrestricted only (a generic
        non-artifact ability activation).
    """
    if coin.spend_restriction is None:
        return True
    if isinstance(spell_kind, str):
        kinds: frozenset[str] = frozenset({spell_kind})
    else:
        kinds = spell_kind
    if (
        coin.spend_restriction == "artifact_spell"
        and "artifact_spell" in kinds
    ):
        return True
    if (
        coin.spend_restriction == "artifact_activation"
        and "artifact_activation" in kinds
    ):
        return True
    if (
        coin.spend_restriction == "artifact_spell_or_activation"
        and ("artifact_spell" in kinds or "artifact_activation" in kinds)
    ):
        return True
    if (
        coin.spend_restriction == "colorless_eldrazi"
        and (
            "colorless_eldrazi_spell" in kinds
            or "colorless_eldrazi_ability" in kinds
        )
    ):
        return True
    if (
        coin.spend_restriction == "legendary_spell"
        and "legendary_spell" in kinds
    ):
        return True
    if (
        coin.spend_restriction == "legendary_permanent_ability"
        and "legendary_permanent_ability" in kinds
    ):
        return True
    if (
        coin.spend_restriction == "land_activation"
        and "land_activation" in kinds
    ):
        return True
    if (
        coin.spend_restriction == "cast_from_outside_hand"
        and "cast_from_outside_hand" in kinds
    ):
        return True
    # Vintage Roto P3 restriction tags (The Mightstone and Weakstone, ...).
    from mtgz_ir.effects.generic.roto_mana import roto_coin_satisfies

    return roto_coin_satisfies(coin, kinds)


def effective_warp_cost(
    state: GameState, controller_id: int, card: GameObject,
) -> "tuple[ManaCost, str] | None":
    """Return the effective warp cost (and its printed string) for
    casting ``card`` from hand via warp, or ``None`` if no warp is
    available.

    The warp cost is, in priority order:
    1. The card's own printed ``warp_cost`` (innate warp).
    2. The first matching grant from a battlefield permanent the
       controller controls (``hand_ability_grants``). For Tannuk,
       Steadfast Second this lets artifact / red-creature cards in
       hand be warp-cast for the granted ``{2}{R}``.

    If both apply (rare), the innate cost wins — granting an
    alternative cost on top of an alternative cost is exotic and
    not in the current pool. Future cards with this overlap can
    refine the priority here.
    """
    if card.printing.warp_cost is not None:
        return (card.printing.warp_cost, card.printing.warp_cost_str)
    for permanent in state.battlefield:
        if permanent.controller_id != controller_id:
            continue
        for grant in permanent.printing.hand_ability_grants:
            if grant.granted_warp_cost is None:
                continue
            if grant.card_filter.matches_card(state, card):
                return (grant.granted_warp_cost, grant.granted_warp_cost_str)
    return None


def _phyrexian_split_affordable(
    state: GameState, player_id: int, cost: ManaCost, life: int,
    spell_kind: "str | frozenset[str]" = "spell",
    *, convoke: bool = False, sac_for_mana: bool = False,
) -> bool:
    """Is ``cost`` payable when each Phyrexian pip may be paid with its
    color or 2 life (CR 107.4f / 119.4: life may be paid only while the
    life total is at least the amount)? Tries every life/mana split —
    printed costs carry at most a few Phyrexian pips."""
    pips = cost.phyrexian
    n = len(pips)
    for mask in range(1 << n):
        life_pips = bin(mask).count("1")
        if life < 2 * life_pips:
            continue
        mana_pips = [pips[i] for i in range(n) if not (mask >> i) & 1]
        if can_pay_cost(
            state, player_id, cost.with_colored(mana_pips),
            spell_kind, convoke=convoke, sac_for_mana=sac_for_mana,
        ):
            return True
    return False


def _settle_phyrexian_pips(
    state: GameState, player_id: int, label: str, cost: ManaCost,
    spell_kind: "frozenset[str]",
) -> ManaCost:
    """Decide, pip by pip, whether each Phyrexian symbol is paid with 2
    life or with mana of its color, pay the life, and return the cost
    with the mana-paid pips folded into its colored requirement.

    When both options keep the rest of the cost payable the caster
    chooses via ``choose_yes_no`` (yes = pay 2 life); otherwise the only
    workable option is taken.
    """
    if not cost.phyrexian:
        return cost
    p = state.player_by_id(player_id)
    settled_mana: list[str] = []
    life_paid = 0
    pips = list(cost.phyrexian)
    for i, color in enumerate(pips):
        rest = ManaCost(
            generic=cost.generic, colored=cost.colored, x_pips=cost.x_pips,
            hybrid=cost.hybrid, phyrexian=tuple(pips[i + 1:]),
        )
        life_left = p.life - life_paid
        mana_ok = _phyrexian_split_affordable(
            state, player_id,
            ManaCost(
                generic=rest.generic,
                colored=rest.with_colored(settled_mana + [color]).colored,
                x_pips=rest.x_pips, hybrid=rest.hybrid,
                phyrexian=rest.phyrexian,
            ),
            life_left, spell_kind, sac_for_mana=True,
        )
        life_ok = life_left >= 2 and _phyrexian_split_affordable(
            state, player_id,
            ManaCost(
                generic=rest.generic,
                colored=rest.with_colored(settled_mana).colored,
                x_pips=rest.x_pips, hybrid=rest.hybrid,
                phyrexian=rest.phyrexian,
            ),
            life_left - 2, spell_kind, sac_for_mana=True,
        )
        pay_life = life_ok
        if life_ok and mana_ok and p.agent is not None:
            pay_life = p.agent.choose_yes_no(
                state, player_id,
                f"Pay 2 life for {{{color}/P}} ({label})?",
            )
        if pay_life:
            life_paid += 2
        else:
            settled_mana.append(color)
    if life_paid:
        p.life -= life_paid
        state.log.append(
            f"P{player_id} pays {life_paid} life for Phyrexian mana "
            f"(life: {p.life})"
        )
    return ManaCost(
        generic=cost.generic, colored=cost.with_colored(settled_mana).colored,
        x_pips=cost.x_pips, hybrid=cost.hybrid,
    )


def can_pay_cost(
    state: GameState, player_id: int, cost: ManaCost,
    spell_kind: "str | frozenset[str]" = "spell",
    *, convoke: bool = False, sac_for_mana: bool = False,
    improvise: bool = False,
) -> bool:
    """Check whether the player can pay ``cost`` by consuming floating
    mana in their pool (respecting spend restrictions for ``spell_kind``)
    then tapping untapped lands.

    ``spell_kind`` accepts a single string (legacy) or a frozenset of
    context tags — see ``_coin_satisfies_context`` for the supported
    tags.

    When ``convoke=True``, the controller's untapped creatures are
    pre-applied as convoke contributions before checking pool + lands.
    Convoke is optional and only ever makes a spell cheaper, so for
    legality we use the maximum convoke contribution.

    When ``sac_for_mana=True``, the controller's sac-for-mana mana
    abilities (Treasure-style "Sacrifice this: Add ...") are
    optimistically counted as available coverage. Like convoke, this
    is upper-bound: the cast pipeline activates only what's needed.
    """
    if cost.phyrexian:
        return _phyrexian_split_affordable(
            state, player_id, cost, state.player_by_id(player_id).life,
            spell_kind=spell_kind, convoke=convoke, sac_for_mana=sac_for_mana,
        )
    if convoke:
        cost = _convoke_max_contribution(state, player_id, cost)
    if improvise:
        cost = _improvise_max_contribution(state, player_id, cost)
    if sac_for_mana:
        cost = _sac_for_mana_max_contribution(state, player_id, cost)
    p = state.player_by_id(player_id)
    usable_pool = [
        c for c in p.mana_pool
        if _coin_satisfies_context(c, spell_kind)
    ]
    lands = _untapped_lands(state, player_id)
    if _can_pay_from(state, cost, usable_pool, lands):
        return True
    if state.continuous_effects:
        # "any time you could activate a mana ability, you may pay 1
        # life. If you do, add {C}." (Channel; Vintage Roto P3).
        from mtgz_ir.effects.generic.roto_mana import life_mana_units_needed

        if life_mana_units_needed(
            state, player_id, cost, usable_pool, lands,
        ) is not None:
            return True
    # "Whenever you tap a <Forest> for mana, add an additional {G}"
    # (Nissa, Who Shakes the World): tapping such lands first yields
    # extra coins -- try pre-tapping 1..all of them.
    extra = extra_mana_grants_for_tapped_lands(state, player_id)
    if not extra:
        return False
    for n_tapped in range(1, len(_extra_mana_lands(state, lands, extra)) + 1):
        pool2, lands2 = _virtual_pretap(state, usable_pool, lands, extra, n_tapped)
        if _can_pay_from(state, cost, pool2, lands2):
            return True
    return False


def extra_mana_grants_for_tapped_lands(
    state: GameState, player_id: int,
) -> list[tuple[str, str]]:
    """``(land subtype, color)`` pairs from "Whenever you tap a
    <Subtype> for mana, add an additional {<color>}" statics
    (``CardPrinting.extra_mana_when_you_tap_land_subtype``) on
    permanents ``player_id`` controls that still have their abilities."""
    out: list[tuple[str, str]] = []
    for o in state.battlefield:
        grants = o.printing.extra_mana_when_you_tap_land_subtype
        if not grants or o.controller_id != player_id:
            continue
        from mtgz_ir.game import _source_layer6_wiped

        if state.continuous_effects and _source_layer6_wiped(state, o):
            continue
        out.extend(grants)
    if state.continuous_effects:
        # "Until end of turn, whenever a player taps an Island for mana,
        # that player adds an additional {U}." (High Tide; Vintage Roto P3).
        from mtgz_ir.effects.generic.roto_mana import ueot_land_tap_mana_grants

        out.extend(ueot_land_tap_mana_grants(state))
    # "Whenever enchanted Forest is tapped for mana, its controller adds an
    # additional one mana of the chosen color." (Utopia Sprawl): keyed by
    # the enchanted land's id.
    from mtgz_ir.effects.generic.roto_mana import enchanted_land_mana_grants

    out.extend(enchanted_land_mana_grants(state, player_id))
    return out


def _extra_mana_lands(state, lands, extra) -> list:
    """The untapped lands a tap-for-mana grant applies to, ordered so
    lands with the fewest other colors are tapped first."""
    from mtgz_ir.game import effective_subtypes

    subtypes = {s for s, _ in extra if isinstance(s, str)}
    # Per-land grants are keyed by the land's id (Utopia Sprawl).
    land_ids = {s for s, _ in extra if isinstance(s, int)}
    hits = [
        l for l in lands
        if l.id in land_ids or subtypes & set(effective_subtypes(state, l))
    ]
    hits.sort(key=lambda l: len(effective_land_produces(state, l)))
    return hits


def _extra_coins_for_land(state, land, extra) -> list[str]:
    """Colors of the coins tapping ``land`` yields under ``extra``: its
    own mana (the grant's color when it can make it) plus one per
    matching grant."""
    from mtgz_ir.game import effective_subtypes

    subs = set(effective_subtypes(state, land))
    produces = effective_land_produces(state, land)
    bonus = [color for s, color in extra if s in subs or s == land.id]
    own = bonus[0] if bonus and bonus[0] in produces else (
        next(iter(sorted(produces)), "C")
    )
    return [own] + bonus


def _virtual_pretap(state, usable_pool, lands, extra, n_tapped):
    from mtgz_ir.game import ManaCoin

    chosen = _extra_mana_lands(state, lands, extra)[:n_tapped]
    ids = {l.id for l in chosen}
    pool = list(usable_pool)
    for l in chosen:
        pool.extend(ManaCoin(color=c) for c in _extra_coins_for_land(state, l, extra))
    return pool, [l for l in lands if l.id not in ids]


def _snow_mana_lands(state, lands, used_lands) -> list:
    """Untapped snow lands that make mana -- the lands that can pay an
    {S} pip (CR 107.4h; Vintage Roto P3)."""
    return [
        l for l in lands
        if "Snow" in l.printing.supertypes
        and effective_land_produces(state, l)
        and l.id not in used_lands
    ]


def _can_pay_from(
    state: GameState, cost: ManaCost, usable_pool: list, lands: list,
) -> bool:
    """The pool-then-lands affordability core of ``can_pay_cost``."""
    used_coins: set[int] = set()
    used_lands: set[int] = set()
    any_color = mana_spendable_as_any_color(state)
    # Pay colored requirements first — prefer pool coins, then lands
    # (exact color first; any mana second while it may be spent as
    # though it were any color).
    for color, count in cost.colored:
        need = count
        for exact in _pip_passes(color, any_color):
            if need == 0:
                break
            for idx, coin in enumerate(usable_pool):
                if idx in used_coins:
                    continue
                if not exact or _coin_matches_color(coin, color):
                    used_coins.add(idx)
                    need -= 1
                    if need == 0:
                        break
        if need == 0:
            continue
        if color == "S":
            matching = _snow_mana_lands(state, lands, used_lands)
        else:
            matching = [
                l for l in lands
                if color in effective_land_produces(state, l)
                and l.id not in used_lands
            ]
            if any_color:
                matching += [
                    l for l in lands
                    if l.id not in used_lands and l not in matching
                ]
        if len(matching) < need:
            return False
        for l in matching[:need]:
            used_lands.add(l.id)
    # Pay generic + hybrid with remaining pool coins (any color) +
    # remaining lands. Hybrid mana symbols ({W/U}, {U/R}, ...) are
    # treated as generic-1 in the affordability check (engine
    # simplification documented in ``ManaCost.hybrid``: any mana pays
    # them, the strict color restriction isn't enforced). The pip
    # count must still be included in the bound — without this, a
    # {U/R} cost would be silently free.
    remaining_pool = sum(
        1 for idx, coin in enumerate(usable_pool)
        if idx not in used_coins and coin.spend_restriction != "colored_only"
    )
    remaining_lands = sum(1 for l in lands if l.id not in used_lands)
    needed = cost.generic + len(cost.hybrid)
    return remaining_pool + remaining_lands >= needed


def apply_cost_reductions(
    printing: "CardPrinting",
    state: GameState,
    caster_id: int,
    base_cost: ManaCost,
    chosen_targets: "tuple[tuple[str, int], ...] | None" = None,
    card: "GameObject | None" = None,
) -> ManaCost:
    """Walk ``printing.cost_reductions`` and return a ManaCost with
    generic mana reduced accordingly. Each rule's
    :class:`mtgz_ir.effects.conditions.Condition` is evaluated via its
    ``matches`` method; when matched, the rule's unit ``amount`` is
    multiplied by ``condition.multiplier`` (default 1) so variable-
    amount rules (Affinity, ``PerDifferentlyNamedLand``) scale.

    External reductions: also walks every permanent the caster
    controls on the battlefield and applies any
    ``external_cost_reductions`` that match (Uthros Psionicist, Sami).
    Externals stack with the spell's own reductions — a 5-cost spell
    with a 3-less self-reduction and a 2-less external ends up at 0
    generic.
    """
    cost = base_cost
    # "<Filter> spells cost {N} more to cast." (Thalia; cube W/G
    # bucket): increases apply before reductions (CR 601.2f).
    from mtgz_ir.creature_statics_wg import spell_cost_increase

    increase = spell_cost_increase(state, printing, caster_id, card=card)
    if increase:
        cost = ManaCost(
            generic=cost.generic + increase, colored=cost.colored,
            x_pips=cost.x_pips, hybrid=cost.hybrid, phyrexian=cost.phyrexian,
        )
    cost = _apply_external_cost_reductions(state, caster_id, cost)
    for rule in printing.cost_reductions:
        reduction = _evaluate_cost_reduction(
            rule, state, caster_id, chosen_targets=chosen_targets,
        )
        if reduction > 0:
            cost = cost.reduced(reduction)
            state.log.append(
                f"cost reduced by {reduction} ({type(rule.condition).__name__})"
            )
    return cost


def _evaluate_cost_reduction(
    rule, state: GameState, caster_id: int,
    chosen_targets: "tuple[tuple[str, int], ...] | None" = None,
) -> int:
    """Resolve a single ``CostReduction`` rule to a generic-mana amount
    against current game state. Shared by spell-cast cost reductions
    (``apply_cost_reductions``) and ability-cost reductions
    (``_effective_ability_mana_cost``).

    Single dispatch through the structural ``Condition`` API:

      - ``rule.condition.matches(state, caster_id, chosen_targets)``
        gates the reduction.
      - ``rule.condition.multiplier(state, caster_id, chosen_targets)``
        scales the unit ``rule.amount`` for variable-amount rules
        (default ``1`` for boolean conditions).

    ``chosen_targets`` is the spell's chosen targets when invoked from
    the cast pipeline. ``None`` means the caller doesn't have target
    context (``legal_actions``' affordability probe; ability
    activations); target-dependent conditions evaluate optimistically
    against the battlefield in that mode.
    """
    if rule.condition.matches(state, caster_id, chosen_targets):
        return rule.amount * rule.condition.multiplier(
            state, caster_id, chosen_targets,
        )
    return 0


def _apply_external_cost_reductions(
    state: GameState, caster_id: int, cost: "ManaCost",
) -> "ManaCost":
    """Walk the caster's battlefield for permanents that grant external
    cost reductions to other spells. Each permanent's
    ``external_cost_reductions`` are evaluated against the same caster
    state used by spell-local reductions. Reductions stack (multiple
    permanents granting the same reduction sum together).

    Same structural dispatch as ``_evaluate_cost_reduction``: each
    external rule's ``condition.matches`` gates the reduction and
    ``condition.multiplier`` scales the unit amount (Sami's affinity
    counts artifacts via ``PerControlled(IsArtifact()).multiplier``).
    External rules don't read chosen targets — the spell hasn't been
    cast yet — so ``chosen_targets`` is left at the default ``None``."""
    for permanent in state.battlefield:
        if permanent.controller_id != caster_id:
            continue
        if permanent.phased_out:
            continue
        externals = getattr(
            permanent.printing, "external_cost_reductions", ()
        )
        for ext in externals:
            if not ext.condition.matches(state, caster_id):
                continue
            reduction = ext.amount * ext.condition.multiplier(
                state, caster_id,
            )
            if reduction > 0:
                cost = cost.reduced(reduction)
                state.log.append(
                    f"cost reduced by {reduction} from "
                    f"{permanent.printing.name} "
                    f"({type(ext.condition).__name__})"
                )
    return cost


def _x_upper_bound(
    state: GameState, player_id: int, base_cost: ManaCost
) -> int:
    """Max X the caster can afford for ``base_cost`` (which still has its
    ``x_pips`` set). Iterates upward from 0 until a payment with the next
    X would exceed available mana. Practical X values are small (0–6),
    so linear probing is fine.
    """
    x = 0
    while True:
        trial = ManaCost(
            generic=base_cost.generic + base_cost.x_pips * (x + 1),
            colored=base_cost.colored,
        )
        if not can_pay_cost(state, player_id, trial):
            return x
        x += 1


def _choose_and_apply_x(
    state: GameState, player_id: int, card: GameObject, cost: ManaCost
) -> int:
    """Ask the caster's agent for an X value, bounded by what they can
    actually pay. Returns the chosen X (clamped into ``[0, upper_bound]``)
    and logs the choice.
    """
    from mtgz_ir.roto_restrictions import any_roto_statics, max_x_allowed

    p = state.player_by_id(player_id)
    upper_bound = _x_upper_bound(state, player_id, cost)
    if card.zone != Zone.BATTLEFIELD and any_roto_statics(state):
        # A mana-value cap on spells (roto P1: Lavinia, Azorius Renegade).
        x_cap = max_x_allowed(state, player_id, card)
        if x_cap is not None:
            upper_bound = min(upper_bound, x_cap)
    if p.agent is None:  # pragma: no cover - games run with agents set
        chosen = 0
    else:
        chosen = p.agent.choose_x(
            state, player_id, card.printing.name, upper_bound
        )
    if chosen < 0:
        chosen = 0
    if chosen > upper_bound:
        chosen = upper_bound
    state.log.append(f"P{player_id} chooses X={chosen} for {card.printing.name}")
    return chosen


def _convoke_candidates(
    state: GameState, player_id: int
) -> list[GameObject]:
    """Untapped creatures ``player_id`` controls. CR 702.51 doesn't
    care about summoning sickness because tapping a creature for
    convoke isn't an activated ability."""
    from mtgz_ir.game import is_creature_now
    return [
        o for o in state.battlefield
        if o.controller_id == player_id
        and is_creature_now(o)
        and not o.tapped
    ]


def _improvise_candidates(
    state: GameState, player_id: int
) -> list[GameObject]:
    """Untapped artifacts ``player_id`` controls (CR 702.126a)."""
    return [
        o for o in state.battlefield
        if o.controller_id == player_id
        and is_artifact_now(state, o)
        and not o.tapped
    ]


def _improvise_max_contribution(
    state: GameState, player_id: int, cost: ManaCost
) -> ManaCost:
    """``cost`` with its generic part lowered by one per untapped
    artifact (improvise pays only generic mana)."""
    n = min(cost.generic, len(_improvise_candidates(state, player_id)))
    if n == 0:
        return cost
    return ManaCost(
        generic=cost.generic - n, colored=cost.colored,
        x_pips=cost.x_pips, hybrid=cost.hybrid, phyrexian=cost.phyrexian,
    )


def _apply_improvise(
    state: GameState, player_id: int, cost: ManaCost,
) -> ManaCost:
    """Improvise during cost payment (CR 702.126a / 601.2h): untapped
    artifacts are tapped, each paying {1} of the generic part. The engine
    picks (``creature_casting_ub.improvise_tap_order``: artifacts without
    mana abilities first) rather than asking the agent -- see
    docs/SEARCH_LIMITATIONS.md."""
    from mtgz_ir.creature_casting_ub import improvise_tap_order

    candidates = _improvise_candidates(state, player_id)
    if cost.generic == 0 or not candidates:
        return cost
    chosen = improvise_tap_order(candidates)
    from mtgz_ir.turn import tap_permanent

    by_id = {o.id: o for o in candidates}
    generic = cost.generic
    for oid in dict.fromkeys(chosen):
        obj = by_id.get(oid)
        if obj is None or obj.tapped or generic == 0:
            continue
        tap_permanent(state, obj)
        generic -= 1
        state.log.append(
            f"P{player_id} taps {obj.printing.name} for improvise"
        )
    return ManaCost(
        generic=generic, colored=cost.colored, x_pips=cost.x_pips,
        hybrid=cost.hybrid, phyrexian=cost.phyrexian,
    )


def _cost_is_zero(cost: ManaCost) -> bool:
    """True when a ManaCost has no remaining generic or colored requirement."""
    return cost.generic == 0 and not any(n > 0 for _, n in cost.colored)


def _apply_convoke_pip(
    state: GameState, creature: GameObject,
    generic: int, colored: list[tuple[str, int]],
) -> tuple[int, list[tuple[str, int]], bool]:
    """Apply one creature's convoke contribution to ``(generic, colored)``.

    Per CR 702.51: each tapped creature pays {1} OR one mana of one of
    its colors. Greedy: prefer paying a still-owed colored pip the
    creature matches; otherwise drop {1} from generic. If the creature
    can't pay anything (cost already 0), returns the inputs unchanged
    with ``applied=False`` so the caller can skip the tap.
    """
    from mtgz_ir.game import effective_colors
    cols = effective_colors(state, creature)
    for i, (color, n) in enumerate(colored):
        if n > 0 and color in cols:
            new_colored = list(colored)
            new_colored[i] = (color, n - 1)
            return generic, new_colored, True
    if generic > 0:
        return generic - 1, colored, True
    return generic, colored, False


def _convoke_max_contribution(
    state: GameState, player_id: int, cost: ManaCost
) -> ManaCost:
    """Lower ``cost`` by the maximum convoke contribution available.

    Used by the affordability check (``can_pay_cost(..., convoke=True)``)
    to compute "cheapest possible cost after convoke". Convoke is
    always optional and only ever makes a spell cheaper, so the
    legality check uses the upper-bound contribution. The actual cast
    pipeline taps whatever subset the agent chooses.
    """
    if _cost_is_zero(cost):
        return cost
    candidates = _convoke_candidates(state, player_id)
    generic = cost.generic
    colored = list(cost.colored)
    for o in candidates:
        if generic == 0 and all(n == 0 for _, n in colored):
            break
        generic, colored, _applied = _apply_convoke_pip(
            state, o, generic, colored,
        )
    return ManaCost(
        generic=generic,
        colored=tuple((c, n) for c, n in colored if n > 0),
        x_pips=cost.x_pips,
    )


def _apply_convoke(
    state: GameState, player_id: int, cost: ManaCost,
) -> ManaCost:
    """Run the convoke prompt during cost-payment (CR 601.2g/h with
    702.51). Asks the controller's agent which untapped creatures to
    tap, then taps each one and reduces the cost by one pip per
    creature. Returns the post-convoke cost for ``auto_tap_for_cost``
    to finish paying from pool + lands.

    A creature tapped for convoke can't separately activate its own
    sac-or-tap mana ability for the same cast (it's already tapped),
    matching the CR-required mutual exclusion that flagged Eldrazi
    Scion + convoke spells in the original design audit.
    """
    p = state.player_by_id(player_id)
    if p.agent is None:
        return cost
    if _cost_is_zero(cost):
        return cost
    candidates = _convoke_candidates(state, player_id)
    if not candidates:
        return cost
    candidate_ids = [o.id for o in candidates]
    chosen_ids = p.agent.choose_convoke_creatures(
        state, player_id, candidate_ids,
        cost.generic, list(cost.colored),
    )
    if not chosen_ids:
        return cost
    from mtgz_ir.turn import tap_permanent
    by_id = {o.id: o for o in candidates}
    seen: set[int] = set()
    generic = cost.generic
    colored = list(cost.colored)
    for cid in chosen_ids:
        if cid in seen:
            continue
        seen.add(cid)
        creature = by_id.get(cid)
        if creature is None or creature.tapped:
            continue
        if generic == 0 and all(n == 0 for _, n in colored):
            break
        generic, colored, applied = _apply_convoke_pip(
            state, creature, generic, colored,
        )
        if not applied:
            continue
        tap_permanent(state, creature)
        state.log.append(
            f"P{player_id} taps {creature.printing.name} for convoke"
        )
    return ManaCost(
        generic=generic,
        colored=tuple((c, n) for c, n in colored if n > 0),
        x_pips=cost.x_pips,
    )


def _classify_sac_for_mana(
    state: GameState, source: GameObject,
) -> "list[tuple[int, tuple[tuple[str, int], ...]]]":
    """For ``source``, list its qualifying sac-for-mana mana abilities
    as ``(ability_idx, produces)`` pairs.

    A qualifying ability:
      - is a mana ability (``is_mana_ability=True``)
      - costs the source's sacrifice (``sacrifice_self=True``)
      - has no other resource costs (no mana cost, no extra sacs, no
        life payment) — Treasure-style "Sacrifice this: Add ..." or
        "{T}, Sacrifice this: Add ..." both qualify
      - resolves into ``AddManaToPool`` with concrete ``produces``
    """
    from mtgz_ir.effects.generic import AddManaToPool
    from mtgz_ir.game import effective_activated_abilities
    out: list[tuple[int, tuple[tuple[str, int], ...]]] = []
    for idx, ability in enumerate(effective_activated_abilities(state, source)):
        if not ability.is_mana_ability:
            continue
        c = ability.cost
        if not c.sacrifice_self:
            continue
        if c.mana is not None and c.mana.total > 0:
            continue
        if (
            c.sacrifice_another_creature_or_artifact
            or c.sacrifice_another_artifact_or_land
            or c.sacrifice_another_artifact
            or c.sacrifice_food
            or c.sacrifice_a_land
            or c.sacrifice_a_desert
            or c.remove_plus_one_counter_from_your_creature
            or c.exile_granting_equipment
            or c.exile_self
            or c.tap_permanent_you_control
            or c.exile_artifact_from_graveyard
            or c.exile_cards_from_graveyard
            or c.pay_life
            or c.discard_a_card
            or c.pay_energy
            or c.discard_hand
            or c.sacrifice_subtype
            or c.sacrifice_nontoken_artifact
            or c.sacrifice_a_permanent_type
            or c.remove_counters_from_self_kind
            or c.crew
            or c.sacrifice_n_nonland_permanents
        ):
            continue
        # ``Activate only as an instant.`` (Lion's Eye Diamond): the
        # ability can't be activated mid-payment of another spell.
        if ability.activate_only_as_instant:
            continue
        if not isinstance(ability.effect, AddManaToPool):
            continue
        out.append((idx, ability.effect.produces))
    return out


def _sac_for_mana_sources(
    state: GameState, player_id: int,
) -> "list[tuple[GameObject, int, tuple[tuple[str, int], ...]]]":
    """Battlefield sources controlled by ``player_id`` whose
    sac-for-mana mana abilities can fire as part of the current cast.
    Already-tapped sources can still fire (a tap-then-sac ability needs
    {T} which a tapped source can't pay; ``_can_fire_now`` filters that).
    """
    from mtgz_ir.creature_statics_wg import activation_locked, any_activation_locks

    locks = any_activation_locks(state)
    sources: list[tuple[GameObject, int, tuple[tuple[str, int], ...]]] = []
    for o in state.battlefield:
        if o.controller_id != player_id:
            continue
        if o.phased_out:
            continue
        if locks and activation_locked(state, o):
            continue
        for idx, produces in _classify_sac_for_mana(state, o):
            if any(b.printing.roto_statics for b in state.battlefield):
                # A named lock (Phyrexian Revoker) forbids this mana
                # ability (roto P1).
                from mtgz_ir.game import effective_activated_abilities
                from mtgz_ir.roto_restrictions import activation_prohibited

                abilities = effective_activated_abilities(state, o)
                if activation_prohibited(state, o, abilities[idx]):
                    continue
            sources.append((o, idx, produces))
    return sources


def _can_fire_sac_for_mana(state: GameState, source: GameObject, ability_idx: int) -> bool:
    """True if ``source``'s ``ability_idx`` ability can be activated
    right now to add mana. Gates the {T} cost on the source being
    untapped (sacrifice and any-cost-zero parts always work)."""
    from mtgz_ir.game import effective_activated_abilities
    abilities = effective_activated_abilities(state, source)
    if ability_idx >= len(abilities):
        return False
    if abilities[ability_idx].cost.tap_self and source.tapped:
        return False
    return True


def _produces_satisfies(
    produces: "tuple[tuple[str, int], ...]", color: str,
    any_color: bool = False,
) -> bool:
    """True if ``produces`` (one activation's mana output) covers a
    requirement for one pip of ``color``. ``"any"`` matches any color;
    a colored requirement is satisfied only by exact match or "any"
    -- or by any output while mana may be spent as though it were any
    color (``any_color``, Mycosynth Lattice); a generic requirement
    (color="" / None) is satisfied by any output.
    """
    if color == "S":
        return False  # sacrifice-for-mana sources aren't snow
    for c, n in produces:
        if n <= 0:
            continue
        if color == "" or color is None or any_color:
            return True
        if c == color or c == "any":
            return True
    return False


def _produces_total(produces: "tuple[tuple[str, int], ...]") -> int:
    return sum(n for _, n in produces)


def _sac_for_mana_max_contribution(
    state: GameState, player_id: int, cost: ManaCost,
) -> ManaCost:
    """Lower ``cost`` by the maximum coverage available from the
    caster's sac-for-mana mana abilities. Used by the affordability
    check (``can_pay_cost(..., sac_for_mana=True)``). Each source can
    fire at most once per cast; greedy color-first matching mirrors
    the runtime ``_activate_sac_for_mana_for_cost`` allocator.
    """
    if _cost_is_zero(cost):
        return cost
    sources = _sac_for_mana_sources(state, player_id)
    if not sources:
        return cost
    consumed: set[int] = set()
    generic = cost.generic
    colored = list(cost.colored)
    any_color = mana_spendable_as_any_color(state)
    # Pay colored first.
    for i, (color, n) in enumerate(colored):
        need = n
        while need > 0:
            chosen_idx: "int | None" = None
            for sidx, (src, ai, produces) in enumerate(sources):
                if src.id in consumed:
                    continue
                if not _can_fire_sac_for_mana(state, src, ai):
                    continue
                if _produces_satisfies(produces, color, any_color):
                    chosen_idx = sidx
                    break
            if chosen_idx is None:
                break
            consumed.add(sources[chosen_idx][0].id)
            need -= 1
        colored[i] = (color, need)
    # Then generic.
    while generic > 0:
        chosen_idx = None
        for sidx, (src, ai, produces) in enumerate(sources):
            if src.id in consumed:
                continue
            if not _can_fire_sac_for_mana(state, src, ai):
                continue
            if _produces_total(produces) > 0:
                chosen_idx = sidx
                break
        if chosen_idx is None:
            break
        consumed.add(sources[chosen_idx][0].id)
        generic -= 1
    return ManaCost(
        generic=generic,
        colored=tuple((c, n) for c, n in colored if n > 0),
        x_pips=cost.x_pips,
    )


def _activate_sac_for_mana_for_cost(
    state: GameState, player_id: int, cost: ManaCost,
    spell_kind: "frozenset[str]",
) -> ManaCost:
    """During 601.2g cost payment, activate the caster's sac-for-mana
    mana abilities to cover any shortfall left after pool + lands.

    Returns the (still-unpaid) cost — typically equal to ``cost``
    unmodified, since auto_tap_for_cost runs after this and consumes
    the freshly-pooled mana. We don't pay the cost here; we just push
    coins into the pool.

    Greedy: for each unmet colored pip, find a sac source whose mana
    output covers that color and activate it. For generic shortfall,
    any sac source qualifies. Only fires when the cost can't already
    be paid from pool + lands — Treasures aren't blown up unless
    they're actually needed.
    """
    if _cost_is_zero(cost):
        return cost
    if can_pay_cost(state, player_id, cost, spell_kind=spell_kind):
        return cost
    sources = _sac_for_mana_sources(state, player_id)
    if not sources:
        return cost
    # Ask the agent which sources to activate. Default returns every
    # candidate in engine order, matching the prior auto-greedy
    # behavior. The agent's ordered list is iterated only while the
    # cost is still unpayable — once we can pay, remaining picks are
    # skipped (Treasures aren't blown up unnecessarily).
    p = state.player_by_id(player_id)
    candidates = [(src.id, produces) for src, _idx, produces in sources]
    if p.agent is None:
        ordered_ids = [oid for oid, _produces in candidates]
    else:
        ordered_ids = p.agent.choose_sac_for_mana(
            state, player_id, candidates,
            cost.generic, list(cost.colored),
        )
    by_id = {
        src.id: (src, idx, produces)
        for src, idx, produces in sources
    }
    seen: set[int] = set()
    for oid in ordered_ids:
        if oid in seen:
            continue
        seen.add(oid)
        entry = by_id.get(oid)
        if entry is None:
            continue
        src, idx, produces = entry
        if not _can_fire_sac_for_mana(state, src, idx):
            continue
        if can_pay_cost(state, player_id, cost, spell_kind=spell_kind):
            break
        # Skip sources whose mana output can't reduce the still-unmet
        # cost — a Treasure-{C} doesn't help against a colored-only
        # {R} cost. The agent's ordered list provides preference;
        # this gate keeps Treasures from being blown up uselessly
        # even if the agent listed them.
        any_color = mana_spendable_as_any_color(state)
        helps_color = any(
            n > 0 and _produces_satisfies(produces, color, any_color)
            for color, n in cost.colored
        )
        helps_generic = (
            cost.generic > 0 and _produces_total(produces) > 0
        )
        if not (helps_color or helps_generic):
            continue
        state.log.append(
            f"P{player_id} activates {src.printing.name}'s "
            f"sac-for-mana ability to pay cost"
        )
        _activate_ability(state, player_id, src.id, idx)
    return cost


def auto_tap_for_cost(
    state: GameState, player_id: int, cost: ManaCost,
    spell_kind: "str | frozenset[str]" = "spell",
    color_sources: "list | None" = None,
) -> list[GameObject]:
    """Pay ``cost`` by consuming floating mana first (respecting spend
    restrictions for ``spell_kind``), then tapping lands for the
    remainder. Returns the list of lands tapped. Mutates the pool
    and the tapped lands.

    ``spell_kind`` accepts a single string (legacy) or a frozenset of
    context tags — see ``_coin_satisfies_context`` for the supported
    tags.

    ``color_sources`` (optional out-list, Sunburst): one frozenset per
    mana unit spent, holding the colors that unit could have been
    (a fixed-color coin / a colored pip -> that color; an ``any`` coin
    -> WUBRG; a land paying generic -> the colors it can produce;
    colorless -> empty). ``max_distinct_colors`` turns it into the
    number of colors of mana spent.
    """
    if cost.phyrexian:
        # Unsettled Phyrexian pips (a payment path that didn't run
        # ``_settle_phyrexian_pips``) are paid with mana of their color.
        cost = cost.with_colored(cost.phyrexian)
    track = color_sources is not None
    p = state.player_by_id(player_id)
    extra = extra_mana_grants_for_tapped_lands(state, player_id)
    if extra:
        _pretap_lands_for_extra_mana(state, player_id, cost, spell_kind, extra)
    if state.continuous_effects:
        # Channel's life-for-{C} (Vintage Roto P3): only for the part of
        # the cost the pool and lands can't cover.
        from mtgz_ir.effects.generic.roto_mana import pay_life_for_mana

        pay_life_for_mana(state, player_id, cost, spell_kind)
    lands = _untapped_lands(state, player_id)
    used_coin_ids: list[int] = []  # indices into p.mana_pool
    used_lands: set[int] = set()
    tapped: list[GameObject] = []
    any_color = mana_spendable_as_any_color(state)
    # Consume pool coins by preference for colored requirements (exact
    # color first; any mana second while it may be spent as though it
    # were any color -- Mycosynth Lattice).
    for color, count in cost.colored:
        need = count
        for exact in _pip_passes(color, any_color):
            if need == 0:
                break
            for idx, coin in enumerate(p.mana_pool):
                if idx in used_coin_ids:
                    continue
                if not _coin_satisfies_context(coin, spell_kind):
                    continue
                if exact and not _coin_matches_color(coin, color):
                    continue
                used_coin_ids.append(idx)
                if track:
                    # The mana spent is the coin's own color (CR 106.1b).
                    color_sources.append(
                        frozenset({color}) if exact
                        else _WUBRG_SET if coin.color == "any"
                        else frozenset({coin.color}) - {"C"}
                    )
                need -= 1
                if need == 0:
                    break
        if need == 0:
            continue
        if color == "S":
            matching = _snow_mana_lands(state, lands, used_lands)
        else:
            matching = [
                l for l in lands
                if color in effective_land_produces(state, l)
                and l.id not in used_lands
            ]
            if any_color:
                matching += [
                    l for l in lands
                    if l.id not in used_lands and l not in matching
                ]
        for l in matching[:need]:
            used_lands.add(l.id)
            tapped.append(l)
            if track:
                produces = frozenset(effective_land_produces(state, l))
                color_sources.append(
                    produces & _WUBRG_SET
                    if color == "S" or color not in produces
                    else frozenset({color})
                )
    # Generic cost (including hybrid pips, treated as generic-1 per
    # the engine simplification): pool first, then lands. The hybrid
    # contribution to the bound was missing here, mirroring the same
    # bug previously in ``can_pay_cost``.
    generic_need = cost.generic + len(cost.hybrid)
    for idx, coin in enumerate(p.mana_pool):
        if generic_need == 0:
            break
        if idx in used_coin_ids:
            continue
        if not _coin_satisfies_context(coin, spell_kind):
            continue
        if coin.spend_restriction == "colored_only":
            # "This mana can't be spent to pay generic mana costs."
            # (Jegantha, the Wellspring; Vintage Roto P3).
            continue
        used_coin_ids.append(idx)
        if track:
            color_sources.append(
                _WUBRG_SET if coin.color == "any"
                else frozenset({coin.color}) - {"C"}
            )
        generic_need -= 1
    remaining_lands = [l for l in lands if l.id not in used_lands]
    for l in remaining_lands[:generic_need]:
        tapped.append(l)
        if track:
            color_sources.append(
                frozenset(effective_land_produces(state, l)) & _WUBRG_SET
            )
    # Remove consumed coins from the pool (high index first to keep
    # lower indices stable).
    for idx in sorted(used_coin_ids, reverse=True):
        del p.mana_pool[idx]
    # Route through tap_permanent so becomes_tapped triggers fire
    # (Chrome Companion, Grove of the Burnwillows, etc.).
    from mtgz_ir.turn import tap_permanent
    for l in tapped:
        # "{T}, Pay N life: Add ..." lands (Horizon Canopy cycle): the
        # life is part of the mana ability's cost, paid as it's tapped.
        life_cost = _land_tap_life_cost(state, l)
        tap_permanent(state, l)
        if l.printing.tapped_for_mana_triggers:
            # "Whenever you tap this land for mana" (Vintage Roto P3).
            from mtgz_ir.effects.generic.roto_mana import (
                push_tapped_for_mana_triggers,
            )

            push_tapped_for_mana_triggers(state, player_id, l)
        if life_cost:
            p.life -= life_cost
            state.log.append(
                f"P{player_id} pays {life_cost} life for "
                f"{l.printing.name}'s mana (life: {p.life})"
            )
    return tapped


_WUBRG_SET: frozenset[str] = frozenset({"W", "U", "B", "R", "G"})


def _pretap_lands_for_extra_mana(
    state: GameState, player_id: int, cost: ManaCost, spell_kind, extra,
) -> None:
    """Nissa, Who Shakes the World's triggered mana ability during
    automatic payment: while ``cost`` isn't payable from the pool plus
    the untapped lands, tap a land a "whenever you tap a <Subtype> for
    mana" grant applies to and float its mana plus the extra mana (CR
    605.1b: a triggered mana ability resolves immediately). Extra mana
    left over floats for the rest of the turn (the engine empties pools
    at cleanup)."""
    from mtgz_ir.game import ManaCoin
    from mtgz_ir.turn import tap_permanent

    p = state.player_by_id(player_id)
    while True:
        usable = [c for c in p.mana_pool if _coin_satisfies_context(c, spell_kind)]
        lands = _untapped_lands(state, player_id)
        if _can_pay_from(state, cost, usable, lands):
            return
        candidates = _extra_mana_lands(state, lands, extra)
        if not candidates:
            return
        land = candidates[0]
        coins = _extra_coins_for_land(state, land, extra)
        tap_permanent(state, land)
        p.mana_pool.extend(ManaCoin(color=c) for c in coins)
        state.log.append(
            f"P{player_id} taps {land.printing.name} for "
            f"{''.join('{' + c + '}' for c in coins)} (extra mana)"
        )


def max_distinct_colors(color_sources: list) -> int:
    """Maximum number of distinct colors assignable to the spent mana
    units (bipartite matching unit -> color; Sunburst counts "colors of
    mana spent", and each flexible unit's color is its controller's
    choice, taken here to maximize the count)."""
    match: dict[str, int] = {}

    def _augment(i: int, seen: set[str]) -> bool:
        for c in color_sources[i]:
            if c in seen:
                continue
            seen.add(c)
            if c not in match or _augment(match[c], seen):
                match[c] = i
                return True
        return False

    for i in range(len(color_sources)):
        _augment(i, set())
    return len(match)


def apply_action(state: GameState, player_id: int, action: Action) -> None:
    if isinstance(action, PassPriority):
        return
    if isinstance(action, PlayLand):
        _play_land(state, player_id, action.card_id)
        return
    if isinstance(action, CastSpell) and action.use_plot:
        _plot_card(state, player_id, action.card_id)
        return
    if isinstance(action, CastSpell) and action.use_suspend:
        from mtgz_ir.effects.generic.roto_mana import suspend_card

        suspend_card(state, player_id, action.card_id)
        return
    if isinstance(action, CastSpell):
        _cast_spell(
            state, player_id, action.card_id,
            use_warp=action.use_warp, kicked=action.kicked,
            cast_for_free=action.cast_for_free,
            use_impending=action.use_impending,
            cast_room_half=action.cast_room_half,
            use_flashback=action.use_flashback,
            use_buyback=action.use_buyback,
            use_escape=action.use_escape,
            use_dash=action.use_dash,
            use_evoke=action.use_evoke,
            graveyard_permission=action.graveyard_permission,
            use_bestow=action.use_bestow,
            use_emerge=action.use_emerge,
            cast_face=action.cast_face,
        )
        return
    if isinstance(action, UnlockRoomHalf):
        _unlock_room_half(state, player_id, action.card_id, action.half_name)
        return
    if isinstance(action, DeclareAttackers):
        _declare_attackers(state, player_id, action.attacker_ids)
        return
    if isinstance(action, DeclareBlockers):
        _declare_blockers(state, player_id, action.assignments)
        return
    if isinstance(action, ActivateStation):
        _activate_station(state, player_id, action.spacecraft_id, action.crew_id)
        return
    if isinstance(action, ActivateAbility):
        _activate_ability(
            state, player_id, action.source_id, action.ability_index
        )
        return
    if isinstance(action, TurnFaceUp):
        _turn_face_up(state, player_id, action.card_id)
        return
    raise ValueError(f"unknown action: {action!r}")


def _turn_face_up(state: GameState, player_id: int, card_id: int) -> None:
    """Turn a face-down permanent face up. Pays the mana cost from
    ``can_pay_cost`` and clears ``face_down``. CR 701.34: only legal
    when the underlying printing is a creature card; otherwise no-ops
    (legal_actions gates this — defensive here).
    """
    obj = next(
        (
            o for o in state.battlefield
            if o.id == card_id
            and o.controller_id == player_id
            and o.face_down
        ),
        None,
    )
    if obj is None:
        return
    if not obj.printing.is_creature:
        return
    from mtgz_ir.effects.generic.roto_layers import turning_face_up_prevented

    if turning_face_up_prevented(state, obj):  # pragma: no cover - gated
        return
    cost_obj = obj.printing.mana_cost
    if not can_pay_cost(state, player_id, cost_obj):
        return
    auto_tap_for_cost(state, player_id, cost_obj, spell_kind="activated_ability")
    obj.face_down = False
    state.log.append(
        f"P{player_id} turns {obj.printing.name} face up"
    )


def _play_land(state: GameState, player_id: int, card_id: int) -> None:
    p = state.player_by_id(player_id)
    # Source may be hand (normal play), exile (impulse-cast lands), or
    # graveyard (Icetill Explorer's "you may play lands from your
    # graveyard" — gated in legal_actions on the controller having any
    # battlefield permanent with grants_play_lands_from_graveyard=True).
    card = next((c for c in p.hand if c.id == card_id), None)
    if card is not None:
        p.hand.remove(card)
        # Modal DFC (CR 712.12): played as its land back face.
        back = card.printing.mdfc_back_face
        if not card.printing.is_land and back is not None and back.is_land:
            card.mdfc_front_printing = card.printing
            card.printing = back
    elif (card := next(
        (c for c in p.graveyard if c.id == card_id), None
    )) is not None:
        p.graveyard.remove(card)
        # Serra Paragon's once-per-turn permission covers the land only
        # when no unlimited "play lands from your graveyard" does.
        from mtgz_ir.graveyard_casting_wg import (
            consume_paragon,
            paragon_land_playable,
        )

        unlimited = p.may_play_from_graveyard_this_turn or any(
            o.controller_id == player_id
            and o.printing.grants_play_lands_from_graveyard
            for o in state.battlefield
        )
        if not unlimited and paragon_land_playable(state, player_id):
            consume_paragon(state, player_id, card)
    elif p.library and p.library[0].id == card_id:
        # Bolas's Citadel: "You may play lands … from the top of your
        # library" (legal_actions gates on the grant).
        card = p.library.pop(0)
    else:
        card = next(
            (c for c in p.exile if c.id == card_id), None
        )
        exile_zone = p.exile
        if card is None or card.exile_play_permission is not None:
            # A land granted an exile play permission (Laelia, Golos,
            # ...) — possibly in another player's exile.
            from mtgz_ir.effects.generic.cube_creatures_rmc import (
                find_permitted_exile_card,
            )

            permitted, perm_owner = find_permitted_exile_card(
                state, player_id, card_id,
            )
            if permitted is not None:
                card, exile_zone = permitted, perm_owner.exile
        if card is None:  # pragma: no cover - defensive; legal_actions gates
            raise ValueError(
                f"land {card_id} not playable by P{player_id}"
            )
        exile_zone.remove(card)
        card.playable_from_exile_this_turn = False
        card.play_from_exile_requires_kavu = False
        card.exile_play_permission = None
    # Track whether the land was played from Lightstall exile (enters
    # tapped regardless of printing) before clearing the flag.
    lightstall_land = card.cast_from_exile_with_lightstall_surcharge
    card.cast_from_exile_with_lightstall_surcharge = False
    card.zone = Zone.BATTLEFIELD
    card.controller_id = player_id
    apply_land_etb_replacements(
        state, player_id, card, force_tapped=lightstall_land,
    )
    card.entered_battlefield_turn = state.turn_number
    state.battlefield.append(card)
    p.lands_played_this_turn += 1
    p.lands_entered_this_turn += 1
    state.log.append(f"P{player_id} plays {card.printing.name}")
    # ETB triggers on the land itself (Command Bridge's
    # sacrifice-unless-tap rider). Push them on the stack like any
    # other ETB. Mirrors put_into_battlefield's call site for
    # creatures/artifacts. Lands go through _play_land, not
    # put_into_battlefield, so we have to call this here.
    # ``_trigger_etb_abilities`` itself dispatches ``Event.ENTERS``,
    # which fires every predicate-style landfall trigger
    # (``IsLand() & Controlled(YOU)``); we no longer need to call
    # ``fire_landfall_triggers`` here as well — that was the
    # legacy-only landfall path and the dispatcher handles the
    # migrated abilities.
    _trigger_etb_abilities(state, card)
    # Phase-8 ``Event.PLAYS_LAND`` dispatch (via
    # ``fire_opponent_plays_land_triggers``, kept named for backward-
    # compatible direct test access). The function's internal legacy
    # walk over ``opponent_plays_land`` string-key abilities is now
    # dead — every land-play observer in the static pool migrated to
    # predicate shape (Burgeoning's ``Controlled(by=OPPONENT,
    # subject="controller")`` and The Endstone's land-half
    # ``Controlled(YOU, subject="controller")``). Both ride the same
    # PLAYS_LAND dispatch sweep.
    from mtgz_ir.turn import fire_opponent_plays_land_triggers
    fire_opponent_plays_land_triggers(state, player_id, land=card)


_BASIC_LAND_TYPES: tuple[str, ...] = (
    "Plains", "Island", "Swamp", "Mountain", "Forest",
)


def apply_land_etb_replacements(
    state: GameState, player_id: int, card: "GameObject",
    *, force_tapped: bool = False,
) -> None:
    """Apply a land's printed "as/enters" replacement effects (CR 614.1c-d
    / 614.12) to ``card`` as it enters under ``player_id``'s control:
    sets ``card.tapped`` and runs the pay-life / copy choices.

    Called while ``card`` is NOT yet on the battlefield, so conditions
    like "unless you control two or fewer other lands" see only the
    other permanents. Shared by the land-drop path (``_play_land``)
    and effects that put a land onto the battlefield (fetch lands), so
    a fetched surveil land / triome / shock land honors its own
    printed entry rules. ``force_tapped`` models external "enters
    tapped" grants (Lightstall Inquisitor's exile-play rider).
    """
    p = state.player_by_id(player_id)
    from mtgz_ir.creature_statics_wg import forced_to_enter_tapped
    from mtgz_ir.roto_board_statics import land_enters_without_rules_text

    if land_enters_without_rules_text(state, card):
        # A nonbasic land entering under Blood Moon / Magus has already
        # lost its own "as/enters" replacements (Vintage Roto P5): only
        # external ones apply.
        card.tapped = force_tapped or forced_to_enter_tapped(
            state, card, player_id,
        )
        return
    # "Enters tapped" covers Planets + most non-basic utility lands.
    card.tapped = bool(card.printing.enters_tapped)
    # Lightstall Inquisitor: lands played from exile via this grant
    # always enter tapped.
    if force_tapped:
        card.tapped = True
    # Duskmourn horror duals: "This land enters tapped unless a
    # player has 13 or less life." Deterministic state check at ETB,
    # no agent choice. The land enters untapped when any player's
    # life total is at or below the threshold; otherwise tapped.
    low_life_threshold = (
        card.printing.enters_tapped_unless_any_player_life_at_most
    )
    if low_life_threshold > 0:
        any_low = any(pl.life <= low_life_threshold for pl in state.players)
        card.tapped = not any_low
    # "This land enters with N <kind> counters on it." (Gemstone Mine).
    for kind, n in card.printing.enters_with_named_counters:
        from mtgz_ir.game import (
            _DEDICATED_COUNTER_FIELDS, add_counters, set_named_counters,
        )
        if kind in _DEDICATED_COUNTER_FIELDS:
            add_counters(state, card, kind, n)
        else:
            set_named_counters(card, kind, n)
    # Generic "This land enters tapped unless <condition>." (fast
    # lands, check lands). Deterministic state check, no agent choice.
    unless = card.printing.enters_tapped_unless
    if unless is not None:
        card.tapped = not unless.matches(state, player_id)
        state.log.append(
            f"{card.printing.name} enters "
            f"{'tapped' if card.tapped else 'untapped'}"
        )
    # "As this land enters, choose a basic land type." (Multiversal
    # Passage). Chosen before the "Then you may pay 2 life" payment
    # below, per the printed order.
    if card.printing.enters_choosing_basic_land_type:
        types = list(_BASIC_LAND_TYPES)
        idx = 0
        if p.agent is not None:
            picked = p.agent.choose_mode(state, player_id, types)
            if isinstance(picked, int) and 0 <= picked < len(types):
                idx = picked
        card.chosen_basic_land_type = types[idx]
        state.log.append(
            f"{card.printing.name}: P{player_id} chooses {types[idx]}"
        )
    # Shock land: optional life payment on ETB. Agent decides; if they
    # can't/won't pay, the land enters tapped.
    life_cost = card.printing.shock_land_life_cost
    if life_cost > 0 and p.agent is not None:
        pay = p.agent.choose_pay_shock_life(state, player_id, life_cost)
        if pay:
            p.life -= life_cost
            state.log.append(
                f"P{player_id} pays {life_cost} life for "
                f"{card.printing.name} (life: {p.life})"
            )
        else:
            card.tapped = True
            state.log.append(
                f"P{player_id} declines shock life — "
                f"{card.printing.name} enters tapped"
            )
    # Echoing Deeps: "You may have this land enter tapped as a copy of
    # any land card in a graveyard, except it's a Cave in addition to
    # its other types." Agent target-selection over graveyard lands.
    if card.printing.optional_copy_land_from_graveyard and p.agent is not None:
        _offer_echoing_deeps_copy(state, player_id, card)
    # "Nonbasic lands your opponents control enter tapped." (Archon of
    # Emeria; ``mtgz_ir.creature_statics_wg``).
    if forced_to_enter_tapped(state, card, player_id):
        card.tapped = True
    # Vintage Roto P3 "As this land enters, ..." choices (Cavern of Souls).
    from mtgz_ir.effects.generic.roto_mana import apply_roto_land_as_enters

    apply_roto_land_as_enters(state, player_id, card)


def _offer_echoing_deeps_copy(
    state: GameState, player_id: int, card: "GameObject",
) -> None:
    """Ask the controller's agent whether to have ``card`` enter as a
    copy of any land card in any graveyard, with "Cave" injected.

    Agent hook: reuses ``choose_target`` with kind
    ``"graveyard_land_card"``. Returning None (or no legal targets)
    declines — ``card`` enters normally.

    On accept, ``card.printing`` is replaced with a fresh CardPrinting
    derived from the target's, with "Cave" added to its subtypes if
    absent. The card enters tapped.
    """
    import dataclasses as _dc

    candidates: list[tuple[str, int]] = [
        ("graveyard_land_card", c.id)
        for pp in state.players
        for c in pp.graveyard
        if c.printing.is_land
    ]
    if not candidates:
        return
    p = state.player_by_id(player_id)
    chosen = p.agent.choose_target(state, player_id, candidates)
    if chosen is None:
        return
    kind, target_id = chosen
    if kind != "graveyard_land_card":
        return
    target = None
    for pp in state.players:
        for c in pp.graveyard:
            if c.id == target_id:
                target = c
                break
        if target is not None:
            break
    if target is None:
        return
    subtypes = tuple(target.printing.subtypes)
    if "Cave" not in subtypes:
        subtypes = subtypes + ("Cave",)
    card.printing = _dc.replace(target.printing, subtypes=subtypes)
    card.tapped = True
    state.log.append(
        f"Echoing Deeps enters as a copy of {target.printing.name} "
        f"(Cave added), tapped"
    )


def _choose_modes_at_cast(
    state: GameState, player_id: int, card: GameObject,
    base_cost: "ManaCost | None" = None,
) -> tuple[int, ...]:
    """CR 601.2e: pick mode(s) for a modal spell at cast time, before
    total cost is determined. Returns the chosen mode indices in
    resolution order, or ``()`` for non-modal spells.

    For "Choose one" returns a 1-tuple. For "Choose one or both"
    returns a 1- or 2-tuple. Resolution runs the chosen modes via
    ``state.resolving_modes`` instead of prompting the agent again
    at resolve time.
    """
    from mtgz_ir.effects.combinators import ModalChooseOne
    from mtgz_ir.effects.generic.multi_modal import ChooseModes
    spell_effect = card.printing.spell_effect
    if isinstance(spell_effect, ChooseModes):
        # "Choose two" / "Choose three (repeats)" / "one or more"
        # (Escalate, Spree): count, repeats and extra-cost affordability
        # are enforced by the effect.
        return spell_effect.choose_at_cast(
            state, player_id, card,
            base_cost if base_cost is not None else card.printing.mana_cost,
        )
    if not isinstance(spell_effect, ModalChooseOne):
        return ()
    p = state.player_by_id(player_id)
    if p.agent is None:
        return (0,)
    if spell_effect.or_both:
        order = p.agent.choose_modes(
            state, player_id,
            [m.describe() for m in spell_effect.modes],
        )
        seen: set[int] = set()
        clean: list[int] = []
        for idx in order:
            if 0 <= idx < len(spell_effect.modes) and idx not in seen:
                clean.append(idx)
                seen.add(idx)
        if not clean:
            clean = [0]
        return tuple(clean)
    idx = p.agent.choose_mode(
        state, player_id,
        [m.describe() for m in spell_effect.modes],
    )
    idx = max(0, min(idx, len(spell_effect.modes) - 1))
    return (idx,)


def _effective_spell_effect_for_targets(
    spell_effect, mode_indices: tuple[int, ...],
):
    """Given a spell's printed effect and the cast-time mode picks,
    return the effect that drives target selection.

    For non-modal spells, passes through. For ``ModalChooseOne`` with
    one mode picked, returns that mode's effect. For ``or_both`` with
    multiple picks where at most one mode targets, returns a
    ``Sequence`` of the chosen modes (target selection on the single
    targeted sub-effect). When more than one chosen mode targets we
    fall back to the printed ``ModalChooseOne`` (its union-of-modes
    legal_targets / target_count_range), since ``Sequence`` doesn't
    support multi-targeted sub-effects today.
    """
    from mtgz_ir.effects.combinators import ModalChooseOne, Sequence
    if not isinstance(spell_effect, ModalChooseOne):
        return spell_effect
    if not mode_indices:
        return spell_effect
    if len(mode_indices) == 1:
        return spell_effect.modes[mode_indices[0]]
    chosen = tuple(spell_effect.modes[i] for i in mode_indices)
    targeted_count = sum(1 for m in chosen if m.requires_target())
    if targeted_count <= 1:
        return Sequence(effects=chosen)
    return spell_effect


def cast_time_effect(
    spell_effect, *, kicked: bool = False, alt_cost_paid: bool = False,
    x_value: "int | None" = None,
):
    """The effect whose targets are chosen for a cast, given how it's
    being cast. Effects whose target shape depends on a cast-time
    choice ("If this spell was kicked, exile target nonland permanent
    instead" — Tear Asunder; overload's "target" -> "each"; "up to X
    target ..." — Pest Infestation) expose ``cast_time_shape(kicked=,
    alt_cost_paid=, x_value=)``; a ``Sequence`` is shaped per
    sub-effect; everything else is its own shape."""
    from mtgz_ir.effects.combinators import Sequence

    if isinstance(spell_effect, Sequence):
        shaped = tuple(
            cast_time_effect(
                e, kicked=kicked, alt_cost_paid=alt_cost_paid, x_value=x_value,
            )
            for e in spell_effect.effects
        )
        if shaped == spell_effect.effects:
            return spell_effect
        return Sequence(effects=shaped)
    shape = getattr(spell_effect, "cast_time_shape", None)
    if shape is None:
        return spell_effect
    return shape(kicked=kicked, alt_cost_paid=alt_cost_paid, x_value=x_value)


def _targets_depend_on_x(spell_effect) -> bool:
    """Whether the spell's target count depends on its X ("up to X
    target ..."), so X must be announced before targets (CR 601.2b)."""
    from mtgz_ir.effects.combinators import Sequence

    if isinstance(spell_effect, Sequence):
        return any(_targets_depend_on_x(e) for e in spell_effect.effects)
    return bool(getattr(spell_effect, "count_from_x", False))


def _select_spell_targets(
    state: GameState, player_id: int, card: GameObject,
    mode_indices: tuple[int, ...] = (),
    x_hint: "int | None" = None,
    *, kicked: bool = False, alt_cost_paid: bool = False,
) -> tuple[tuple[str, int], ...]:
    """Pick target(s) for a spell about to be cast. Mirrors CR
    601.2c-d (mode/target choices happen BEFORE total cost is
    determined). Called from ``_cast_spell`` ahead of
    ``apply_cost_reductions`` so target-dependent cost riders (Dark
    Endurance) can see the choice.

    Handles both regular ``spell_effect`` targets (single- and
    multi-target via ``Effect.target_count_range``) and aura
    enchant-target picks. ``mode_indices`` carries the cast-time
    modal pick so target selection runs against the chosen mode's
    targets, not the union over all modes. Returns an empty tuple
    for non-targeting spells.
    """
    p = state.player_by_id(player_id)
    targets: tuple[tuple[str, int], ...] = ()
    spell_effect = card.printing.spell_effect
    from mtgz_ir.effects.generic.multi_modal import ChooseModes
    if isinstance(spell_effect, ChooseModes):
        # Per-mode-instance targets (CR 601.2c); modes were picked above.
        return spell_effect.select_targets(
            state, player_id, mode_indices, x_value=x_hint, card=card,
        )
    if spell_effect is not None:
        spell_effect = cast_time_effect(
            spell_effect, kicked=kicked, alt_cost_paid=alt_cost_paid,
            x_value=x_hint,
        )
        spell_effect = _effective_spell_effect_for_targets(
            spell_effect, mode_indices,
        )
        min_t, max_t = spell_effect.target_count_range()
        # Divided damage (CR 601.2d): each target must get at least 1,
        # so a known total caps the number of targets.
        division_total = getattr(spell_effect, "division_total", None)
        if division_total is not None and x_hint is not None:
            max_t = min(max_t, max(1, division_total(state, player_id, x_hint)))
        if max_t > 0:
            from mtgz_ir.game import drop_protected_players
            legal = drop_protected_players(state, filter_targetable(
                state, spell_effect.legal_targets(state, player_id),
                card, player_id,
            ))
            chosen_list: list[tuple[str, int]] = []
            remaining = list(legal)
            slot_kinds = spell_effect.target_slot_kinds()
            # A later target that depends on an earlier one ("two target
            # permanents that share a card type" -- roto P4's Bilbo).
            per_slot = getattr(spell_effect, "legal_targets_for_slot", None)
            if not getattr(
                spell_effect, "slot_targets_depend_on_chosen", False,
            ):
                per_slot = None
            for slot_idx in range(max_t):
                if per_slot is not None:
                    pool = [
                        t for t in per_slot(
                            state, player_id, slot_idx,
                            chosen=tuple(chosen_list),
                        )
                        if t in remaining
                    ]
                elif slot_kinds and slot_idx < len(slot_kinds):
                    needed = slot_kinds[slot_idx]
                    pool = [t for t in remaining if t[0] == needed]
                else:
                    pool = remaining
                if not pool:
                    break
                pick = p.agent.choose_target(state, player_id, pool)
                # CR 601.2c: when the agent must pick (slot_idx < min_t)
                # a None / out-of-range answer is illegal — coerce to
                # the first legal pool entry. Above min_t, the agent's
                # decline is honored as "stop adding optional targets"
                # since target_count_range allows fewer than max_t.
                # Removing the picked target from ``remaining`` keeps
                # subsequent slots different (CR 115.1b).
                if pick is None or pick not in pool:
                    if slot_idx < min_t:
                        pick = pool[0]
                    else:
                        break
                chosen_list.append(pick)
                remaining.remove(pick)
            targets = tuple(chosen_list)
    # Auras pick their enchanted-target at cast time. The target is
    # recorded on the StackItem and consumed at resolution to set
    # attached_to. ``aura_enchant_type`` picks the scope: "creature"
    # (default) or "artifact" (Hardlight Containment).
    # ``aura_yours_only`` further restricts the legal pool to permanents
    # the caster controls — the printed "Enchant creature you control"
    # line. Sheltered by Ghosts ships this; default off mirrors the
    # plain "Enchant creature" wording.
    if card.bestowed:
        # Bestow (CR 702.103): an Aura spell with enchant creature.
        from mtgz_ir.game import is_creature_now

        legal = filter_targetable(state, [
            ("creature", o.id) for o in state.battlefield
            if is_creature_now(o)
        ], card, player_id)
        if legal:
            chosen = p.agent.choose_target(state, player_id, legal)
            targets = (chosen if chosen in legal else legal[0],)
        return targets
    if card.printing.is_aura:
        enchant_type = plain(card.printing.aura_enchant_type)
        yours_only = card.printing.aura_yours_only
        if enchant_type == "creature_card_in_graveyard":
            # Animate Dead: "Enchant creature card in a graveyard".
            from mtgz_ir.effects.generic.enchantment_mechanics import (
                graveyard_creature_card_targets,
            )
            legal = graveyard_creature_card_targets(state)
        elif enchant_type == "artifact":
            legal = [
                (enchant_type, o.id)
                for o in state.battlefield
                if is_artifact_now(state, o)
                and (not yours_only or o.controller_id == player_id)
            ]
        elif enchant_type in ("creature_or_spacecraft", "artifact_or_creature"):
            legal = [
                (enchant_type, o.id) for o in state.battlefield
                if _aura_compound_kind_matches(state, enchant_type, o)
                and (not yours_only or o.controller_id == player_id)
            ]
        elif enchant_type == "land":
            # "Enchant Forest" (Vintage Roto P3: Utopia Sprawl).
            from mtgz_ir.effects.generic.roto_mana import enchantable_land

            legal = [
                ("land", o.id) for o in state.battlefield
                if enchantable_land(state, card.printing, o)
                and (not yours_only or o.controller_id == player_id)
            ]
        else:
            legal = [
                ("creature", o.id)
                for o in state.battlefield
                if o.printing.is_creature
                and (not yours_only or o.controller_id == player_id)
            ]
        legal = filter_targetable(state, legal, card, player_id)
        if legal:
            chosen = p.agent.choose_target(state, player_id, legal)
            if chosen is not None:
                targets = (chosen,)
    return targets


def _aura_compound_kind_matches(state: GameState, kind: str, obj) -> bool:
    """"Enchant creature or Spacecraft" / "Enchant artifact or creature"
    (``EnchantKind``): a Spacecraft is legal below its station threshold
    too (CR 303.4c, 722.2b)."""
    from mtgz_ir.game import is_creature_now

    if kind == "creature_or_spacecraft":
        return is_creature_now(obj) or "Spacecraft" in obj.printing.subtypes
    return is_creature_now(obj) or is_artifact_now(state, obj)


def _plot_card(state: GameState, player_id: int, card_id: int) -> None:
    """The plot special action (CR 702.170a): pay the card's plot cost
    and exile it from hand face up, remembering the turn. It doesn't use
    the stack and isn't a spell cast."""
    from mtgz_ir.game import reveal_to_all

    p = state.player_by_id(player_id)
    card = next((c for c in p.hand if c.id == card_id), None)
    if card is None or card.printing.plot_cost is None:  # pragma: no cover - legal_actions gates
        raise ValueError(f"card {card_id} not plottable by P{player_id}")
    cost = _activate_sac_for_mana_for_cost(
        state, player_id, card.printing.plot_cost, frozenset({"spell"}),
    )
    auto_tap_for_cost(state, player_id, cost, spell_kind=frozenset({"spell"}))
    p.hand.remove(card)
    card.zone = Zone.EXILE
    card.plotted_turn = state.turn_number
    p.exile.append(card)
    reveal_to_all(state, card)
    state.log.append(
        f"P{player_id} plots {card.printing.name} "
        f"({card.printing.plot_cost_str})"
    )


def _pay_teamwork(
    state: GameState, player_id: int, card: GameObject, power_needed: int,
) -> bool:
    """Teamwork N: if the caster's untapped creatures have total power
    >= N and their agent agrees (``choose_yes_no``), tap creatures
    (picked via ``choose_permanent_to_tap``) until their total power
    reaches N. Returns whether the cost was paid."""
    from mtgz_ir.game import effective_power, is_creature_now
    from mtgz_ir.turn import tap_permanent

    candidates = [
        o for o in state.battlefield
        if o.controller_id == player_id and is_creature_now(o) and not o.tapped
    ]
    if sum(max(0, effective_power(state, o)) for o in candidates) < power_needed:
        return False
    agent = state.player_by_id(player_id).agent
    if agent is not None and not agent.choose_yes_no(
        state, player_id,
        f"cast {card.printing.name} using teamwork (tap creatures with "
        f"total power {power_needed} or more)",
    ):
        return False
    total = 0
    while total < power_needed and candidates:
        ids = [o.id for o in candidates]
        pick = agent.choose_permanent_to_tap(state, player_id, ids) if agent else None
        chosen = next((o for o in candidates if o.id == pick), candidates[0])
        candidates.remove(chosen)
        total += max(0, effective_power(state, chosen))
        tap_permanent(state, chosen)
    state.log.append(f"P{player_id} casts {card.printing.name} using teamwork")
    return True


def _choose_replicate_times(
    state: GameState, player_id: int, card: GameObject, cost: ManaCost,
) -> tuple[int, ManaCost]:
    """Ask the caster how many times to pay ``card``'s replicate cost
    (``choose_x`` over the affordable range) and return ``(times,
    cost + times * replicate)``."""
    from mtgz_ir.alt_casting import cost_plus
    from mtgz_ir.cards import parse_mana_cost

    rep = parse_mana_cost(card.printing.replicate_cost)
    upper = 0
    total = cost
    while upper < 10:
        nxt = cost_plus(total, rep)
        if not can_pay_cost(state, player_id, nxt, sac_for_mana=True):
            break
        total = nxt
        upper += 1
    times = upper
    agent = state.player_by_id(player_id).agent
    if agent is not None and upper > 0:
        times = max(0, min(upper, agent.choose_x(
            state, player_id, f"{card.printing.name} (replicate)", upper,
        )))
    for _ in range(times):
        cost = cost_plus(cost, rep)
    if times:
        state.log.append(
            f"P{player_id} replicates {card.printing.name} x{times}"
        )
    return times, cost


def _roto_top_of_library_cast_allowed(state, player_id, card) -> bool:
    from mtgz_ir.effects.generic.roto_mana import top_of_library_cast_allowed

    return top_of_library_cast_allowed(state, player_id, card)


def _cast_spell(
    state: GameState, player_id: int, card_id: int,
    use_warp: bool = False, kicked: bool = False,
    cast_for_free: bool = False,
    use_impending: bool = False,
    cast_room_half: "str | None" = None,
    use_flashback: bool = False,
    use_buyback: bool = False,
    use_escape: bool = False,
    use_dash: bool = False,
    use_evoke: bool = False,
    graveyard_permission: "str | None" = None,
    use_bestow: bool = False,
    use_emerge: bool = False,
    alt_mana_cost: "ManaCost | None" = None,
    cast_face: "str | None" = None,
) -> None:
    p = state.player_by_id(player_id)
    from mtgz_ir.legal import _is_sorcery_timing

    if cast_face is not None:
        # Split half / modal DFC back face (``mtgz_ir.layouts``): the
        # spell has only that face's characteristics (CR 709.3a /
        # 712.11a), so the card turns to it before anything reads its
        # printing; it turns back when it leaves the stack for any zone
        # but the battlefield (``move_spell_off_stack``) or leaves the
        # battlefield.
        from mtgz_ir.layouts import face_named, find_face_card, turn_to_face

        face_card = find_face_card(state, player_id, card_id)
        face = (
            face_named(face_card.printing, cast_face)
            if face_card is not None else None
        )
        if face is None:  # pragma: no cover - legal_actions gates
            raise ValueError(
                f"card {card_id} has no castable face {cast_face!r}"
            )
        turn_to_face(face_card, face)

    # Necromancy's "if you cast it any time a sorcery couldn't have been
    # cast" reads the timing before the spell moves to the stack.
    sorcery_timing_at_cast = _is_sorcery_timing(state, player_id)
    # Set when an exile play permission lets this cast skip its mana
    # cost (so a pitch alternative cost is never demanded).
    free_by_permission = False

    # A spell can be cast from the hand (normal or warp cost), from
    # the "warped" / impulse-playable exile zone (normal cost), or —
    # when a permanent grants it — from the top of the library
    # (Mm'menon, the Right Hand). We resolve the source zone up
    # front so the rest of the flow treats it uniformly.
    # ``cast_from_outside_hand`` flags non-hand origins for the
    # spend-restriction tag set composed below (Mm'menon's blue mana
    # ability is usable only for these casts).
    cast_from_outside_hand = False
    card = next((c for c in p.hand if c.id == card_id), None)
    if use_escape:
        # Escape (CR 702.138): cast from the graveyard for the escape
        # cost; the "exile N other cards" part is paid with the other
        # additional costs below.
        from mtgz_ir.alt_casting import escape_terms as _escape_terms

        card = next((c for c in p.graveyard if c.id == card_id), None)
        terms = (
            _escape_terms(state, player_id, card) if card is not None else None
        )
        if terms is None:  # pragma: no cover - legal_actions gates
            raise ValueError(f"card {card_id} not escape-castable by P{player_id}")
        cost, escape_exile_n = terms
        cast_from_outside_hand = True
        p.graveyard.remove(card)
        card.cast_via_warp = False
        card.cast_via_hand = False
        card.flashback_granted_until_eot = False
    elif use_flashback:
        # Flashback (CR 702.34): cast from the graveyard for the
        # flashback cost. ``legal_actions`` gates the offer; the card
        # is exiled when it leaves the stack (``move_spell_off_stack``).
        from mtgz_ir.alt_casting import flashback_cost as _fb_cost

        card = next((c for c in p.graveyard if c.id == card_id), None)
        if card is None or _fb_cost(card) is None:  # pragma: no cover - legal_actions gates
            raise ValueError(
                f"card {card_id} not flashback-castable by P{player_id}"
            )
        cost = _fb_cost(card)
        cast_from_outside_hand = True
        p.graveyard.remove(card)
        card.flashback_granted_until_eot = False
        card.cast_via_flashback = True
        card.cast_via_warp = False
        card.cast_via_hand = False
    elif card is not None:
        # Cost selection cascade: Room half wins first (Rooms are always
        # cast for one half's cost, never the printing's combined
        # mana_cost). Then Impending alt-cost (DSK Overlord cycle).
        # Then Warp alt-cost (EoE). Default is the normal mana_cost.
        if cast_room_half is not None and card.printing.is_room:
            half = next(
                h for h in card.printing.room_halves
                if h.name == cast_room_half
            )
            cost = half.mana_cost
        elif use_emerge:
            from mtgz_ir.creature_casting_ub import pay_emerge_sacrifice

            cost = pay_emerge_sacrifice(state, player_id, card)
        elif alt_mana_cost is not None:
            # An alternative cost chosen by an effect (Miracle).
            cost = alt_mana_cost
        elif use_impending:
            cost = card.printing.impending_cost
        elif use_dash:
            # Dash (CR 702.109): an alternative cost from hand.
            cost = card.printing.dash_cost
        elif use_evoke:
            from mtgz_ir.evoke import evoke_mana_cost as _evoke_mana_cost

            cost = _evoke_mana_cost(card.printing)
        elif use_bestow:
            cost = card.printing.bestow_cost
        elif use_warp:
            # Innate warp first, then battlefield-granted warp
            # (Tannuk, Steadfast Second's hand-warp grant). The
            # ``effective_warp_cost`` helper consolidates the lookup
            # so legal-actions enumeration and the cast pipeline
            # agree on the cost.
            warp_lookup = effective_warp_cost(state, player_id, card)
            cost = warp_lookup[0] if warp_lookup is not None else None
        else:
            cost = card.printing.mana_cost
        p.hand.remove(card)
        # Mark the spell as warp-cast so it triggers the end-step exile once
        # it resolves onto the battlefield.
        card.cast_via_warp = bool(use_warp)
        # Mark the spell as impending-cast so put_into_battlefield
        # initializes ``time_counters`` and sets ``impending_active``.
        # Stored on the card itself (not the StackItem) because the
        # ETB handler in put_into_battlefield reads from the
        # GameObject as it transitions BATTLEFIELD; mirrors the
        # ``cast_via_warp`` precedent. Cleared at ETB after the
        # counter init to avoid double-application if a future
        # effect blinks the permanent.
        card.cast_via_impending = bool(use_impending)
        card.evoked = bool(use_evoke)
        card.bestowed = bool(use_bestow)
        # Cast-from-hand provenance flag. Distinct from ``cast_via_warp``
        # / ``cast_via_impending``: a warp-from-hand or impending-from-
        # hand cast still originated from the hand (the printed cost
        # alternative is paid by the caller in either case). The two
        # paths that DON'T set this flag are warp-from-graveyard
        # (Timeline Culler) and cast-from-exile / cast-from-top-of-
        # library, both handled in the elif/else branches below.
        # Consumed by the ``enters_with_minus_one_counters_from_hand_
        # cast`` ETB replacement (Patched Plaything).
        card.cast_via_hand = True
        if use_warp:
            state.spell_warped_this_turn = True
            # Pay the warp ability's "Pay N life" rider, if any (Timeline
            # Culler). legal_actions has already gated p.life > cost.
            life_cost = card.printing.warp_additional_life_cost
            if life_cost > 0:
                p.life -= life_cost
                state.log.append(
                    f"P{player_id} pays {life_cost} life for warp "
                    f"(life: {p.life})"
                )
    elif graveyard_permission is not None:
        # Retrace (Six) / Serra Paragon: cast from the graveyard for the
        # normal cost; the permission's extra cost is paid below.
        card = next((c for c in p.graveyard if c.id == card_id), None)
        if card is None:  # pragma: no cover - legal_actions gates
            raise ValueError(f"card {card_id} not in P{player_id}'s graveyard")
        cost = card.printing.mana_cost
        cast_from_outside_hand = True
        p.graveyard.remove(card)
        card.cast_via_warp = False
        card.cast_via_hand = False
        card.flashback_granted_until_eot = False
        from mtgz_ir.graveyard_casting_wg import pay_graveyard_permission

        pay_graveyard_permission(state, player_id, card, graveyard_permission)
    elif (
        not use_warp
        and (
            yw_card := next(
                (c for c in p.graveyard if c.id == card_id), None
            )
        ) is not None
        and (
            p.may_play_from_graveyard_this_turn
            or yw_card.castable_from_graveyard_this_turn
            or _mayhem_permission(state, player_id, yw_card)
            or _lurrus_permission_for(state, player_id, yw_card) is not None
        )
    ):
        # Yawgmoth's Will / a per-card "you may cast that card this turn"
        # permission (Emry): cast from the graveyard for the normal cost.
        # Otherwise a Lurrus-style once-per-turn permission (spent now).
        if _mayhem_permission(state, player_id, yw_card) and not (
            p.may_play_from_graveyard_this_turn
            or yw_card.castable_from_graveyard_this_turn
        ):
            # Mayhem: cast for its mayhem cost (its mana cost).
            state.log.append(
                f"P{player_id} casts {yw_card.printing.name} for its mayhem "
                "cost"
            )
        elif not (
            p.may_play_from_graveyard_this_turn
            or yw_card.castable_from_graveyard_this_turn
        ):
            granter = _lurrus_permission_for(state, player_id, yw_card)
            granter.graveyard_cast_used_turn = state.turn_number
            state.log.append(
                f"{granter.printing.name} lets P{player_id} cast "
                f"{yw_card.printing.name} from the graveyard"
            )
        card = yw_card
        cost = card.printing.mana_cost
        cast_from_outside_hand = True
        # ``legal_actions`` never offers a free graveyard cast: a
        # ``cast_for_free`` one comes from an effect's "cast it without
        # paying its mana cost" (``cast_now``; Dreadhorde Arcanist), so no
        # alternative or pitch cost is demanded.
        free_by_permission = cast_for_free
        p.graveyard.remove(card)
        card.cast_via_warp = False
        card.cast_via_hand = False
        card.flashback_granted_until_eot = False
        card.castable_from_graveyard_this_turn = False
    elif (
        use_warp
        and (
            grave_card := next(
                (c for c in p.graveyard if c.id == card_id), None
            )
        ) is not None
        and grave_card.printing.warp_castable_from_graveyard
    ):
        # Warp-cast from the graveyard (Timeline Culler).
        card = grave_card
        cost = card.printing.warp_cost
        cast_from_outside_hand = True
        p.graveyard.remove(card)
        card.cast_via_warp = True
        # Defensive clear: a card that previously sat on the battlefield
        # (and therefore had ``cast_via_hand`` set) carries the flag
        # into the graveyard. Reset it here so the cast-from-hand-only
        # ETB hooks don't fire on a graveyard-origin cast.
        card.cast_via_hand = False
        state.spell_warped_this_turn = True
        life_cost = card.printing.warp_additional_life_cost
        if life_cost > 0:
            p.life -= life_cost
            state.log.append(
                f"P{player_id} pays {life_cost} life for warp from "
                f"graveyard (life: {p.life})"
            )
    else:
        # Cast from exile: legal for (a) cards previously exiled by warp
        # (warped=True), (b) cards marked impulse-playable this turn
        # (playable_from_exile_this_turn=True), (c) cards exiled by
        # Possibility Technician (play_from_exile_requires_kavu=True),
        # provided the controller still controls a Kavu, or (d) cards
        # exiled by Lightstall Inquisitor's ETB
        # (cast_from_exile_with_lightstall_surcharge=True) which carry a
        # {1} surcharge applied below. All pay normal cost except (d).
        # (legal_actions enforces the Kavu-control gate on (c); the
        # predicate here just authorizes the cast site.)
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            exile_permission_cast_cost,
            find_permitted_exile_card,
        )

        permitted, perm_owner = find_permitted_exile_card(
            state, player_id, card_id,
        )
        card = None if permitted is not None else next(
            (
                c for c in p.exile
                if c.id == card_id and (
                    c.warped
                    or c.playable_from_exile_this_turn
                    or c.play_from_exile_requires_kavu
                    or c.cast_from_exile_with_lightstall_surcharge
                    or c.plotted_turn >= 0
                )
            ),
            None,
        )
        if permitted is not None:
            # Exile play permission (Ragavan, Laelia, Golos, Thor, ...):
            # the card may sit in any player's exile; a free permission
            # arrives with ``cast_for_free`` (which zeroes the cost below).
            card = permitted
            cost = exile_permission_cast_cost(card, card.exile_play_permission)
            # A free permission never demands a pitch alternative cost.
            free_by_permission = card.exile_play_permission.free
            cast_from_outside_hand = True
            perm_owner.exile.remove(card)
            card.exiled_face_down = False  # left exile (CR 400.7)
            card.exile_play_permission = None
            card.playable_from_exile_this_turn = False
        elif card is not None:
            cost = card.printing.mana_cost
            # Lightstall surcharge: "each spell cast this way costs {1}
            # more to cast" — add 1 generic on top of the printed cost.
            # This runs before apply_cost_reductions so reductions can
            # offset the surcharge (standard MTG layer order).
            if card.cast_from_exile_with_lightstall_surcharge:
                cost = ManaCost(
                    generic=cost.generic + 1 + card.exile_cast_extra_surcharge,
                    colored=cost.colored,
                    x_pips=cost.x_pips,
                )
                card.exile_cast_extra_surcharge = 0
            cast_from_outside_hand = True
            p.exile.remove(card)
            card.exiled_face_down = False  # left exile (CR 400.7)
            card.warped = False
            card.playable_from_exile_this_turn = False
            card.play_from_exile_requires_kavu = False
            card.cast_from_exile_with_lightstall_surcharge = False
            card.plotted_turn = -1
            card.cast_via_warp = False
            # Cast from exile is NOT a cast from hand — clear any stale
            # provenance flag from a prior hand-cast so cast-from-hand-
            # only ETB hooks don't fire on this resolution.
            card.cast_via_hand = False
        else:
            # Cast from top of library (Mm'menon grant). Legal only
            # for the topmost card, and only if the controller has a
            # permanent granting it. legal_actions also requires the
            # card to be an artifact.
            citadel = bool(p.library) and p.library[0].id == card_id and any(
                o.printing.grants_play_from_top_paying_life
                for o in state.battlefield
                if o.controller_id == player_id
            )
            if citadel:
                # Bolas's Citadel: "pay life equal to its mana value
                # rather than pay its mana cost" — an alternative cost,
                # so the mana cost becomes zero (X is 0; additional
                # costs such as kicker are still paid in mana below).
                card = p.library.pop(0)
                life = card.printing.mana_cost.total
                p.life -= life
                state.log.append(
                    f"P{player_id} pays {life} life to cast "
                    f"{card.printing.name} from the top of their library "
                    f"(life: {p.life})"
                )
                cost = ManaCost()
                cast_from_outside_hand = True
                card.cast_via_warp = False
                card.cast_via_hand = False
            elif (
                p.library
                and p.library[0].id == card_id
                and (
                    any(
                        o.printing.grants_cast_artifact_from_top_of_library
                        for o in state.battlefield
                        if o.controller_id == player_id
                    )
                    # Vintage Roto P3 grants (Mystic Forge).
                    or _roto_top_of_library_cast_allowed(
                        state, player_id, p.library[0],
                    )
                )
            ):
                card = p.library.pop(0)
                cost = card.printing.mana_cost
                cast_from_outside_hand = True
                card.cast_via_warp = False
                # Cast from top of library is NOT a cast from hand.
                card.cast_via_hand = False
            else:  # pragma: no cover - defensive (legal_actions gates)
                raise ValueError(
                    f"card {card_id} not castable by P{player_id}"
                )

    # Schedule the warp end-step exile as a proper delayed triggered
    # ability (CR 603.7). The trigger fires once on the controller's
    # next end step — same turn for sorcery-speed warps cast on the
    # caster's own turn (offset 0), next turn for instant-speed warps
    # cast on an opponent's turn (offset 1, when caster becomes
    # active). At end step, ``run_delayed_end_step_triggers`` wraps
    # this entry as a synthetic ``TriggeredAbility`` and pushes it
    # onto the stack via ``push_triggered_ability``; players then get
    # priority and the standard ``resolve_top_of_stack`` path drains
    # it (logged as "<source>'s delayed trigger goes on the stack").
    # ``WarpExileSelf`` re-checks the battlefield by id at resolve
    # time — if the warp permanent has already left the battlefield
    # (countered, died, bounced, etc.), the resolution is a no-op
    # per CR 603.6c.
    if use_warp:
        from mtgz_ir.effects import WarpExileSelf

        offset = 0 if state.active_player().id == player_id else 1
        state.delayed_triggers.append({
            "fire_at_turn": state.turn_number + offset,
            "controller_id": player_id,
            "source_id": card.id,
            "effect": WarpExileSelf(),
        })

    # CR 601.2e: pick modes BEFORE 601.2f cost determination so a
    # mode-dependent cost rider (or a target picked in the chosen
    # mode) can feed into the discount layer. Returns () for non-modal
    # spells. Stored on the StackItem and replayed via
    # state.resolving_modes during resolution so the modal effect
    # doesn't re-prompt.
    # Printed alternative costs (CR 118.9: Force of Will, Daze, Snuff
    # Out, Baleful Mastery, Overload, ...) ride the same "cast without
    # paying its mana cost" action variant as the pitch cost below: a
    # ``cast_for_free`` cast of a card with ``alternative_costs`` (and no
    # free-cast permission) is cast for one of them. Announced before
    # modes and targets (CR 601.2b) since Overload changes the targets.
    alt_cost_index: int | None = None
    from mtgz_ir.cards.spell_costs import alternative_costs_for

    # Printed alternative costs plus granted ones (Dream Halls).
    alternative_costs = alternative_costs_for(state, player_id, card)
    if cast_for_free and alternative_costs and not free_by_permission:
        from mtgz_ir.alt_casting import free_cast_permission as _free_perm
        from mtgz_ir.cards.spell_costs import select_alternative_cost

        if not _free_perm(state, player_id, card):
            alt_cost_index = select_alternative_cost(state, player_id, card)
    # "If this spell's alternative cost was paid" (Baleful Mastery,
    # Overload) means the card's own printed one, not a granted one.
    printed_alt_cost_paid = (
        alt_cost_index is not None
        and alt_cost_index < len(card.printing.alternative_costs)
    )
    chosen_modes: tuple[int, ...] = _choose_modes_at_cast(
        state, player_id, card, base_cost=cost,
    )
    # Select target(s) BEFORE applying cost reductions. CR 601.2c-f
    # orders modes/targets BEFORE total-cost determination, so any
    # "costs {N} less if it targets <X>" rider (Dark Endurance) needs
    # the chosen target visible to the cost-reduction layer. The
    # ``targets`` tuple flows into ``apply_cost_reductions`` below and
    # later onto the StackItem unchanged.
    # "Pay X life" additional cost: X is announced with the other
    # cast-time choices (CR 601.2b, before targets) and paid with the
    # total cost below; the resolving effect reads it as X.
    life_x_paid: int | None = None
    if card.printing.additional_cost_pay_x_life:
        from mtgz_ir.alt_casting import choose_pay_x_life as _choose_life_x

        life_x_paid = _choose_life_x(state, player_id, card)
    # Vintage Roto P3: "discard X cards" (Firestorm) announces X here too.
    from mtgz_ir.effects.generic.roto_mana import choose_additional_cost_x

    discard_x = choose_additional_cost_x(state, player_id, card)
    # "up to X target ...": X is announced before targets (CR 601.2b).
    early_x: "int | None" = None
    if (
        cost is not None and cost.x_pips > 0 and not cast_for_free
        and _targets_depend_on_x(card.printing.spell_effect)
    ):
        early_x = _choose_and_apply_x(state, player_id, card, cost)
    targets: tuple[tuple[str, int], ...] = _select_spell_targets(
        state, player_id, card, mode_indices=chosen_modes,
        x_hint=(
            life_x_paid if life_x_paid is not None
            else discard_x if discard_x is not None
            else early_x if early_x is not None
            else (0 if card.printing.mana_cost.x_pips == 0 else None)
        ),
        kicked=kicked, alt_cost_paid=printed_alt_cost_paid,
    )

    # Apply cost reductions (e.g., "costs {3} less if you've cast another
    # spell this turn"). Warp costs are reduced too — the rules don't
    # special-case this.
    from mtgz_ir.creature_casting_ub import casting_origin as _casting_origin

    # Vintage Roto P3: "you may sacrifice any number of <permanents>. This
    # spell costs {1} less for each" (Rottenmouth Viper) -- the sacrifices
    # happen as the total cost is determined.
    from mtgz_ir.effects.generic.roto_mana import optional_sacrifice_for_cost

    with _casting_origin(
        cast_from_outside_hand, from_graveyard=card.zone == Zone.GRAVEYARD,
        from_exile=card.zone == Zone.EXILE,
    ), optional_sacrifice_for_cost(state, player_id, card, cost, targets):
        cost = apply_cost_reductions(
            card.printing, state, player_id, cost, chosen_targets=targets,
            card=card,
        )
    # Kicker: add the optional kicker cost onto the generic+colored
    # portion before X is resolved.
    if kicked and card.printing.kicker_cost is not None:
        kc = card.printing.kicker_cost
        merged_colored: dict[str, int] = dict(cost.colored)
        for color, n in kc.colored:
            merged_colored[color] = merged_colored.get(color, 0) + n
        cost = ManaCost(
            generic=cost.generic + kc.generic,
            colored=tuple(sorted(merged_colored.items())),
            # A kicker with {X} (Thieving Skydiver) adds its X pips.
            x_pips=cost.x_pips + kc.x_pips,
        )
        state.log.append(
            f"P{player_id} kicks {card.printing.name} for "
            f"{card.printing.kicker_cost_str}"
        )
        kick_count = 1
        if card.printing.multikicker:
            # Multikicker (CR 702.33c): pay the kicker cost any number of
            # additional times (the agent picks how many via choose_x).
            from mtgz_ir.effects.generic.cube_creatures_rmc import (
                choose_extra_kicks,
            )

            cost, extra = choose_extra_kicks(state, player_id, card, cost)
            kick_count += extra
    else:
        kick_count = 0
    # Squad / Offspring (U/B cube creatures): optional additional costs
    # the caster opts into while casting.
    if (
        card.printing.squad_cost is not None
        or card.printing.offspring_cost is not None
    ):
        from mtgz_ir.creature_casting_ub import offer_creature_additional_costs

        cost = offer_creature_additional_costs(state, player_id, card, cost)
    # Phyrexian mana (CR 107.4f): settle each {C/P} pip as 2 life or
    # one mana of its color before the rest of the payment.
    phyrexian_life_units = 0
    card.phyrexian_life_pips = 0
    if cost.phyrexian:
        _spell_kinds_for_phyrexian: set[str] = {"spell"}
        if cast_from_outside_hand:
            _spell_kinds_for_phyrexian.add("cast_from_outside_hand")
        _total_before_settle = cost.total
        cost = _settle_phyrexian_pips(
            state, player_id, card.printing.name, cost,
            frozenset(_spell_kinds_for_phyrexian),
        )
        # Pips paid with life leave the total (Trinisphere still counts
        # them, roto P1); Compleated (CR 702.150a, roto P4) reads how many
        # pips were paid with life.
        phyrexian_life_units = _total_before_settle - cost.total
        card.phyrexian_life_pips = phyrexian_life_units
    # Buyback (CR 702.27): the optional additional cost joins the total
    # cost before X is resolved.
    if use_buyback and card.printing.buyback_cost is not None:
        from mtgz_ir.alt_casting import cost_plus as _cost_plus

        cost = _cost_plus(cost, card.printing.buyback_cost)
        state.log.append(
            f"P{player_id} pays buyback {card.printing.buyback_cost_str} "
            f"for {card.printing.name}"
        )
    # Splice onto <subtype> (CR 702.47): reveal cards from hand and add
    # their splice costs; their effects resolve after the spell's own.
    spliced: tuple = ()
    if card.printing.subtypes and not cast_for_free:
        from mtgz_ir.alt_casting import offer_splices as _offer_splices

        cost, spliced = _offer_splices(
            state, player_id, card, cost, frozenset({"spell"}),
        )
    # Spree (CR 702.172): each chosen mode adds its own mana cost.
    mode_mana_cost = getattr(card.printing.spell_effect, "mode_mana_cost", None)
    if mode_mana_cost is not None and chosen_modes:
        from mtgz_ir.alt_casting import cost_plus as _cost_plus_spree

        cost = _cost_plus_spree(cost, mode_mana_cost(chosen_modes))
    # Replicate (CR 702.56): pay the replicate cost any number of times.
    replicate_times = 0
    if card.printing.replicate_cost and not cast_for_free:
        replicate_times, cost = _choose_replicate_times(
            state, player_id, card, cost,
        )
    # X-cost spells: ask the agent how much X to pay, fold X*x_pips into
    # generic, then zero x_pips so the payment step sees a concrete cost.
    x_value = 0
    if cost.x_pips > 0:
        x_value = (
            early_x if early_x is not None
            else _choose_and_apply_x(state, player_id, card, cost)
        )
        # "X can't be 0" (legal_actions only offers the cast when the
        # minimum is affordable).
        x_value = max(x_value, card.printing.x_min)
        cost = ManaCost(
            generic=cost.generic + cost.x_pips * x_value,
            colored=cost.colored,
            x_pips=0,
        )
    # "Cast without paying its mana cost" (Weftwalking's static
    # alt-cast). Zero out the cost; auto_tap_for_cost still runs but
    # has nothing to pay. Additional non-mana costs (sacrifices, etc.)
    # are paid normally. X is treated as 0 (per CR 601.2f).
    if cast_for_free:
        cost = ManaCost(generic=0, colored=(), x_pips=0)
        x_value = 0
        # A pitch alternative cost (Pyrokinesis) is paid unless a free
        # permission (Weftwalking / the card's own condition) applies.
        from mtgz_ir.alt_casting import (
            free_cast_permission as _free_permission,
            pay_pitch_cost as _pay_pitch,
        )

        if (
            card.printing.alt_cost_exile_from_hand is not None
            and not free_by_permission
            and alt_cost_index is None
            and not _free_permission(state, player_id, card)
        ):
            _pay_pitch(state, player_id, card)
        if alt_cost_index is not None:
            # A printed alternative cost may itself cost mana (Baleful
            # Mastery's {1}{B}, Overload); its other parts are paid with
            # the additional costs below.
            cost = alternative_costs[alt_cost_index].mana_cost()
        # Cost increases still apply to a spell cast without paying its
        # mana cost (CR 601.2f -- Thalia on a pitched Force of Will).
        from mtgz_ir.creature_statics_wg import spell_cost_increase

        with _casting_origin(
            cast_from_outside_hand, from_graveyard=card.zone == Zone.GRAVEYARD,
            from_exile=card.zone == Zone.EXILE,
        ):
            free_tax = spell_cost_increase(
                state, card.printing, player_id, card=card,
            )
        if free_tax:
            from mtgz_ir.alt_casting import cost_plus as _cost_plus

            cost = _cost_plus(cost, ManaCost(generic=free_tax))
    # Trinisphere (roto P1): a floor on the final total cost, applied
    # after every other cost change; Phyrexian pips paid with life are
    # part of that total.
    from mtgz_ir.roto_restrictions import apply_minimum_spell_cost

    cost = apply_minimum_spell_cost(
        state, cost, already_paid=0 if cast_for_free else phyrexian_life_units,
        log=True,
    )
    # Evoke's "Exile a <filter> card from your hand" cost (CR 702.74).
    if use_evoke:
        from mtgz_ir.evoke import pay_evoke_exile as _pay_evoke_exile

        _pay_evoke_exile(state, player_id, card)
        state.log.append(f"P{player_id} evokes {card.printing.name}")
    # Build the set of spend-restriction context tags for this cast.
    # A spell can match more than one (e.g., a legendary artifact
    # spell satisfies both "artifact_spell" and "legendary_spell"
    # restrictions on coins in the pool).
    spell_kinds: set[str] = {"spell"}
    if is_artifact_now(state, card):
        spell_kinds.add("artifact_spell")
    # Eldrazi Temple's second mana ability produces coins usable only
    # for colorless-Eldrazi costs. Detect colorless-Eldrazi casts here
    # ("Eldrazi" in subtypes + colorless: its effective colors -- devoid
    # and Mycosynth Lattice included -- are empty).
    if (
        "Eldrazi" in card.printing.subtypes
        and not effective_colors(state, card)
    ):
        spell_kinds.add("colorless_eldrazi_spell")
    # Plaza of Heroes's second mana ability produces coins usable only
    # to cast a legendary spell.
    if card.printing.is_legendary:
        spell_kinds.add("legendary_spell")
    # Mm'menon, the Right Hand's grant produces coins usable only for
    # casts from outside the controller's hand (graveyard, exile, top
    # of library — set above when we resolved the spell's source zone).
    if cast_from_outside_hand:
        spell_kinds.add("cast_from_outside_hand")
    # Vintage Roto P3 tags (Cavern of Souls' creature-type mana).
    from mtgz_ir.effects.generic.roto_mana import roto_spell_kinds

    spell_kinds |= roto_spell_kinds(card)
    # CR 601.2g (mana abilities) precedes 601.2h (pay total cost). Sac
    # Treasures and other "sacrifice: Add ..." mana abilities fire
    # FIRST, filling the pool. Convoke is part of paying the cost
    # (CR 702.51b: tapped creatures "pay for" {1} or one mana of
    # their color), so it runs afterwards. The ordering matters when
    # a single source could be used for both — e.g. an Eldrazi Scion
    # with "{T}, Sacrifice this: Add {C}" can EITHER sac for mana
    # here OR convoke below, never both: sacing removes it before
    # convoke runs; if it survives this step, convoke taps it and
    # the {T} portion of the sac cost is no longer payable.
    # Delve (CR 702.66): exile graveyard cards to pay generic mana
    # before the mana payment proper.
    if card.printing.has_delve:
        from mtgz_ir.alt_casting import pay_delve as _pay_delve

        cost = _pay_delve(state, player_id, card, cost)
    cost = _activate_sac_for_mana_for_cost(
        state, player_id, cost, frozenset(spell_kinds),
    )
    # Convoke (CR 702.51): part of cost payment at 601.2h. Tapping a
    # creature this way pays for {1} or one mana of its color.
    # ``cast_for_free`` zeroed the cost above, so convoke is a no-op
    # there.
    if card.printing.has_convoke:
        cost = _apply_convoke(state, player_id, cost)
    if card.printing.has_improvise:
        cost = _apply_improvise(state, player_id, cost)
    haste_coins_before = sum(
        1 for c in p.mana_pool if c.grants_haste_to_creature_spell
    )
    uncounterable_coins_before = sum(
        1 for c in p.mana_pool if c.makes_spell_uncounterable
    )
    # Colors of mana spent (Sunburst, Converge): every cast records
    # them — Prismatic Ending reads the count on the stack.
    color_sources: list = []
    auto_tap_for_cost(
        state, player_id, cost, spell_kind=frozenset(spell_kinds),
        color_sources=color_sources,
    )
    # Delighted Halfling: "... and that spell can't be countered."
    card.cast_uncounterable = uncounterable_coins_before > sum(
        1 for c in p.mana_pool if c.makes_spell_uncounterable
    )
    # Arena of Glory: "If any of that mana is spent on a creature spell,
    # it gains haste until end of turn." A UEOT grant on the spell's
    # object carries onto the permanent it becomes (same id).
    if haste_coins_before and card.printing.is_creature:
        haste_coins_after = sum(
            1 for c in p.mana_pool if c.grants_haste_to_creature_spell
        )
        if haste_coins_after < haste_coins_before:
            state.add_continuous_effect(
                source_id=card.id, layer=6, kind="grant_keyword",
                target_id=card.id, params=("haste",),
                expires_at="cleanup",
                description=f"{card.printing.name} gains haste UEOT (mana)",
            )
            state.log.append(
                f"{card.printing.name} gains haste until end of turn"
            )
    # Sunburst (CR 702.44) / Converge (CR 207.2c): remember how many
    # colors of mana were spent; ``put_into_battlefield`` places that
    # many counters on a Sunburst permanent, and converge spells read
    # it while on the stack.
    card.sunburst_colors_spent = max_distinct_colors(color_sources)
    # Total mana spent: one unit per mana actually paid (after convoke /
    # improvise / delve, which pay without mana, and after Phyrexian pips
    # paid with life were taken out) -- "if no mana was spent to cast it"
    # (Vexing Bauble, roto P1), "if five or more mana was spent to cast
    # that spell" (Exhibition Tidecaller, roto P3).
    card.mana_spent_to_cast = len(color_sources)
    # Which colors each spent unit could have been ("if {G}{G} was spent
    # to cast it", Wistfulness); the color assignment is settled when a
    # condition first reads it (``roto_mana.spent_mana_choice``).
    card.mana_spent_color_sources = tuple(color_sources)
    card.spent_color_choice = None
    # Additional cost: sacrifice a creature or artifact. Legal_actions
    # already gated casting on a valid sac target existing; here we
    # just pay by calling sacrifice() on the agent's pick.
    if card.printing.additional_cost_sacrifice_creature_or_artifact:
        sac_options = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and (o.printing.is_creature or is_artifact_now(state, o))
        ]
        if sac_options:
            from mtgz_ir.turn import sacrifice as _sacrifice

            chosen = sac_options[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, player_id, [o.id for o in sac_options]
                )
                chosen = next(
                    (o for o in sac_options if o.id == pick_id),
                    sac_options[0],
                )
            state.log.append(
                f"additional cost: P{player_id} sacrifices "
                f"{chosen.printing.name}"
            )
            _sacrifice(state, chosen)
    # Additional cost: sacrifice a creature or enchantment. Parallel
    # path to the creature-or-artifact branch above; same agent-pick
    # plumbing, different eligible-permanent filter (an Enchantment in
    # ``card_types`` covers both bare enchantments and Auras).
    if card.printing.additional_cost_sacrifice_creature_or_enchantment:
        sac_options = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and (
                o.printing.is_creature
                or "Enchantment" in o.printing.card_types
            )
        ]
        if sac_options:
            from mtgz_ir.turn import sacrifice as _sacrifice

            chosen = sac_options[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, player_id, [o.id for o in sac_options]
                )
                chosen = next(
                    (o for o in sac_options if o.id == pick_id),
                    sac_options[0],
                )
            state.log.append(
                f"additional cost: P{player_id} sacrifices "
                f"{chosen.printing.name}"
            )
            _sacrifice(state, chosen)
    spell_cost_paid_objects: list = []
    # Additional cost: sacrifice a permanent matching the printing's
    # ``additional_cost_sacrifice_filter`` (Tinker / Natural Order /
    # Crop Rotation).
    if card.printing.additional_cost_sacrifice_filter is not None:
        from mtgz_ir.alt_casting import (
            pay_additional_sacrifice as _pay_additional_sacrifice,
        )

        sacrificed = _pay_additional_sacrifice(state, player_id, card)
        if sacrificed is not None:
            # "the sacrificed creature's mana value" (Burnt Offering)
            # reads it from ``state.resolving_cost_paid_objects``.
            spell_cost_paid_objects.append(sacrificed)
    if card.printing.zone_abilities:
        # Roto P2: the non-mana part of a flashback cost (Cabal Therapy),
        # the optional bargain cost (Beseech the Mirror).
        from mtgz_ir.effects.generic.roto_zones import pay_zone_cast_costs

        pay_zone_cast_costs(state, player_id, card, use_flashback=use_flashback)
    if use_escape and escape_exile_n:
        from mtgz_ir.alt_casting import pay_escape_exile as _pay_escape_exile

        _pay_escape_exile(state, player_id, card, escape_exile_n)
    if card.printing.additional_cost_exile_from_graveyard:
        from mtgz_ir.creature_casting_ub import pay_exile_from_graveyard_cost

        pay_exile_from_graveyard_cost(state, player_id, card)
    if life_x_paid is not None:
        p.life -= life_x_paid
        x_value = life_x_paid
        state.log.append(
            f"additional cost: P{player_id} pays {life_x_paid} life "
            f"(life: {p.life})"
        )
    if discard_x is not None:
        from mtgz_ir.effects.generic.roto_mana import pay_discard_x

        pay_discard_x(state, player_id, card, discard_x)
        x_value = discard_x
    # Printed alternative cost: pay its non-mana parts (life, exile a
    # card from hand, return / sacrifice a land).
    if alt_cost_index is not None:
        from mtgz_ir.cards.spell_costs import pay_alternative_cost_components

        pay_alternative_cost_components(
            state, player_id, card, alternative_costs[alt_cost_index],
        )
    # Teamwork: optionally tap creatures with enough total power.
    card.optional_additional_cost_paid = False
    if card.printing.teamwork_power:
        card.optional_additional_cost_paid = _pay_teamwork(
            state, player_id, card, card.printing.teamwork_power,
        )
    # Escalate (CR 702.120): its cost once per mode beyond the first.
    pay_escalate = getattr(card.printing.spell_effect, "pay_escalate", None)
    if pay_escalate is not None:
        pay_escalate(state, player_id, chosen_modes)
    # One-of additional cost ("discard a card or pay 3 life").
    if card.printing.additional_cost_options:
        from mtgz_ir.cards.spell_costs import pay_additional_cost_option

        pay_additional_cost_option(state, player_id, card)
    # Additional cost: designate a creature you control or a warped
    # creature card you own in exile (Close Encounter). Ungated by
    # sacrifice/tap — just a choice. The chosen id lands on the StackItem
    # for the effect to read at resolve.
    additional_cost_choice_id: int | None = None
    if card.printing.additional_cost_choose_your_creature:
        options = [
            o for o in state.battlefield
            if o.controller_id == player_id and o.printing.is_creature
        ]
        options.extend(
            o for o in p.exile
            if o.owner_id == player_id
            and o.warped
            and o.printing.is_creature
        )
        if options:
            chosen_id = options[0].id
            if p.agent is not None:
                pick = p.agent.choose_sacrifice(
                    state, player_id, [o.id for o in options]
                )
                if pick in {o.id for o in options}:
                    chosen_id = pick
            additional_cost_choice_id = chosen_id
            chosen = next(o for o in options if o.id == chosen_id)
            state.log.append(
                f"additional cost: P{player_id} designates "
                f"{chosen.printing.name}"
            )
    card.zone = Zone.STACK
    card.controller_id = player_id
    if card.prepare_copy_of >= 0:
        # Casting a prepare spell's copy: the creature stops being prepared.
        from mtgz_ir.layouts import prepare_copy_cast

        prepare_copy_cast(state, card)

    # Divided damage (CR 601.2d): announce the split now that X is known.
    division: tuple[int, ...] = ()
    effect_for_division = card.printing.spell_effect
    division_total_fn = getattr(effect_for_division, "division_total", None)
    if division_total_fn is not None and targets:
        from mtgz_ir.effects.generic.divided_damage import choose_damage_division

        total = division_total_fn(state, player_id, x_value)
        if total < len(targets):
            targets = targets[:total]
        division = choose_damage_division(
            state, player_id, card.printing.name, len(targets), total,
        )

    # Targets were already selected up-front (before cost reductions)
    # so cost-reduction rules that key off the chosen target — e.g.
    # Dark Endurance's "costs {1} less if it targets a blocking
    # creature" — could see the choice.
    # Ward counters "unless its controller pays": a spell that can't be
    # countered (Long Goodbye: "This includes by the ward ability.") is
    # unaffected, so nothing is paid (CR 702.21a / 101.2).
    from types import SimpleNamespace as _Probe

    from mtgz_ir.effects.generic.stack_control import CounterTargetSpell as _CTS

    countered = not _CTS._is_uncounterable(
        state, _Probe(source=card, controller_id=player_id),
    ) and _apply_ward_check(state, player_id, targets)
    card.cast_without_sorcery_timing = (
        card.printing.sacrifice_at_cleanup_if_cast_without_sorcery_timing
        and not sorcery_timing_at_cast
    )
    state.stack.append(
        StackItem(
            source=card, controller_id=player_id, targets=targets,
            countered=countered, x_value=x_value, kicked=kicked,
            alt_cost_paid=printed_alt_cost_paid,
            cost_paid_objects=tuple(spell_cost_paid_objects),
            additional_cost_choice_id=additional_cost_choice_id,
            chosen_modes=chosen_modes,
            cast_room_half=cast_room_half,
            buyback_paid=bool(
                use_buyback and card.printing.buyback_cost is not None
            ),
            division=division,
            spliced=spliced,
            alt_cost=(
                "dash" if use_dash else "escape" if use_escape else ""
            ),
            kick_count=kick_count,
        )
    )
    if targets:
        # "Whenever ... becomes the target of a spell" (cube creatures).
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            fire_becomes_target,
        )

        fire_becomes_target(state, targets, card, player_id)
    mode = " (warp)" if use_warp else ""
    if use_flashback:
        mode = " (flashback)"
    if alt_cost_index is not None:
        mode = " (alternative cost)"
    if use_dash:
        mode = " (dash)"
    if use_escape:
        mode = " (escape)"
    if cast_room_half is not None:
        mode = f" ({cast_room_half})"
    state.log.append(
        f"P{player_id} casts {card.printing.name}{mode}"
        + _format_log_targets(state, targets)
    )

    # Storm (CR 702.40): the cast trigger copies the spell once per
    # spell cast before it this turn (every player's spells).
    if card.printing.has_storm:
        from mtgz_ir.effects.base import TriggeredAbility
        from mtgz_ir.effects.generic.spell_copies import StormCopySpell
        from mtgz_ir.effects.predicates import Always, Event as _StormEvent

        storm_count = sum(pl.spells_cast_this_turn for pl in state.players)
        push_triggered_ability(
            state,
            source=card,
            ability=TriggeredAbility(
                event=_StormEvent.IS_CAST, predicate=Always(),
                effect=StormCopySpell(
                    spell_id=card.id, count=storm_count,
                    printing=card.printing,
                ),
            ),
            ability_index=-1,
            controller_id=player_id,
            label="storm",
        )
    # Replicate: "When you cast this spell, copy it for each time you
    # paid its replicate cost. You may choose new targets for the
    # copies."
    if replicate_times:
        from mtgz_ir.effects.generic.spell_copies import _choose_copy_targets

        for _ in range(replicate_times):
            push_spell_copy(
                state, card.printing, player_id,
                _choose_copy_targets(state, player_id, card.printing, targets),
                x_value=x_value,
            )

    # "When you cast this spell, ..." (CR 603.2 on the stack: the
    # spell's own IS_CAST abilities — Emrakul, cascade). The battlefield
    # IS_CAST dispatch below never sees them (the spell isn't a
    # permanent), so they're stacked here with the spell as source.
    if card.printing.triggered_abilities:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            fire_this_spell_cast_triggers,
        )

        fire_this_spell_cast_triggers(state, card, player_id)
    # Track cast-count for "whenever you cast your Nth spell" triggers.
    p.spells_cast_this_turn += 1
    p.spells_cast_this_game += 1
    # The colors of the spells cast this turn (Veil of Summer).
    from mtgz_ir.roto_board_statics import note_spell_cast

    note_spell_cast(state, p, card)
    # Track noncreature-spell casts for the Deafening Silence
    # restriction ("each player can't cast more than one noncreature
    # spell each turn").
    if not card.printing.is_creature:
        p.noncreature_spells_cast_this_turn += 1
    # Phase-8 predicate-style dispatch: fire the IS_CAST event for
    # every spell cast. The legacy ``second_spell_each_turn`` /
    # ``cast_spell_or_play_land`` (spell half) /
    # ``cast_artifact_spell`` per-key walks were collapsed into this
    # single dispatch when the last in-pool static migrated to
    # predicate shape — every spell-cast observer now flows through
    # the dispatcher's predicate-evaluation loop. The
    # ``Controlled(YOU, subject="controller")`` and
    # ``IsArtifact(subject="spell")`` and ``SpellsCastThisTurn(n=2)``
    # branches all evaluate against the same subject mapping in a
    # single sweep over the battlefield.
    from mtgz_ir.effects.predicates import Event as _Event
    dispatch_event(
        state, _Event.IS_CAST, label="is-cast",
        spell=card, controller=p,
    )
    if not card.printing.is_creature:
        fire_prowess_triggers(state, player_id)


# Prowess (CR 702.108): "Whenever you cast a noncreature spell, this
# creature gets +1/+1 until end of turn." Modeled as a keyword so a
# granted prowess (a token "with prowess", an Equipment rider) triggers
# exactly like a printed one: the cast pipeline walks every permanent
# the caster controls whose effective keywords include ``"prowess"``
# and stacks one trigger each. The ability index 2000 keeps the
# synthesized trigger off the printed-ability once-per-turn ledger.
_PROWESS_ABILITY_INDEX = 2000


def _prowess_ability():
    from mtgz_ir.effects import TriggeredAbility
    from mtgz_ir.effects.generic.pump import PumpSelfUntilEOT
    from mtgz_ir.effects.predicates import Event as _Event

    return TriggeredAbility(
        event=_Event.IS_CAST,
        effect=PumpSelfUntilEOT(power_bonus=1, toughness_bonus=1),
    )


def fire_prowess_triggers(state: GameState, caster_id: int) -> None:
    from mtgz_ir.game import effective_keywords

    for obj in list(state.battlefield):
        if obj.controller_id != caster_id:
            continue
        if "prowess" not in effective_keywords(state, obj):
            continue
        push_triggered_ability(
            state,
            source=obj,
            ability=_prowess_ability(),
            ability_index=_PROWESS_ABILITY_INDEX,
            controller_id=caster_id,
            label="prowess",
        )


# Persist (CR 702.79a): "When this creature dies, if it had no -1/-1
# counters on it, return it to the battlefield under its owner's
# control with a -1/-1 counter on it." ``turn.move_off_battlefield``
# reads the LKI (keyword + counter state) and calls this after the
# object reached the graveyard.
_PERSIST_ABILITY_INDEX = 2001


def push_persist_trigger(state: GameState, dead: GameObject) -> None:
    from mtgz_ir.effects import TriggeredAbility
    from mtgz_ir.effects.generic.keyword_actions import ReturnPersistedCard
    from mtgz_ir.effects.predicates import Event as _Event

    push_triggered_ability(
        state,
        source=dead,
        ability=TriggeredAbility(
            event=_Event.DIES, effect=ReturnPersistedCard(),
        ),
        ability_index=_PERSIST_ABILITY_INDEX,
        controller_id=dead.owner_id,
        label="persist",
    )


def _apply_ward_check(
    state: GameState, caster_id: int,
    targets: tuple[tuple[str, int], ...],
) -> bool:
    """Apply Ward at cast time. For each permanent target controlled by an
    opponent with Ward {cost}, the caster auto-pays the cost by tapping
    additional lands. If they can't pay, the spell is countered and this
    returns True.

    Simplified: no pay-or-counter decision — caster always pays if they
    can. No player-choice surface; Ward is essentially an additional mana
    cost on top of the spell's printed cost.

    Looks at permanent-shaped target kinds (``creature``, ``permanent``,
    ``artifact``, etc.) — anything that resolves to a battlefield object.
    Skips ``player`` and ``stack`` targets. The effective ward cost
    aggregates printed ward and any ward riders granted by attached
    Auras/Equipment (Hardlight Containment).
    """
    from mtgz_ir.game import effective_ward_cost

    _NON_PERMANENT_KINDS = {"player", "stack", "spell", "card"}
    for kind, tid in targets:
        if kind in _NON_PERMANENT_KINDS:
            continue
        obj = next((o for o in state.battlefield if o.id == tid), None)
        if obj is None:
            continue
        if obj.controller_id == caster_id:
            continue  # ward doesn't fire on your own permanents
        # Ward — Discard a card at random (Alpharael, Stonechosen).
        if obj.printing.ward_discard_random:
            p = state.player_by_id(caster_id)
            if not p.hand:
                state.log.append(
                    f"Spell countered by {obj.printing.name}'s Ward "
                    f"(hand empty, can't discard)"
                )
                return True
            card = state.rng.choice(p.hand)
            p.hand.remove(card)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            state.log.append(
                f"P{caster_id} discards {card.printing.name} at random "
                f"to pay Ward for {obj.printing.name}"
            )
            continue
        # Ward — Discard a card (chosen by caster). Sunset Saboteur.
        if obj.printing.ward_discard_chosen:
            p = state.player_by_id(caster_id)
            if not p.hand:
                state.log.append(
                    f"Spell countered by {obj.printing.name}'s Ward "
                    f"(hand empty, can't discard)"
                )
                return True
            hand_ids = [c.id for c in p.hand]
            chosen_id = p.agent.choose_discard(state, caster_id, hand_ids)
            card = next(c for c in p.hand if c.id == chosen_id)
            p.hand.remove(card)
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            state.log.append(
                f"P{caster_id} discards {card.printing.name} "
                f"to pay Ward for {obj.printing.name}"
            )
            continue
        # Ward—Pay N life (Sedgemoor Witch). Auto-paid like the other
        # ward costs; a caster whose life total is below N can't pay.
        # Granted instances (Hexing Squelcher's 'Other creatures you
        # control have "Ward—Pay 2 life."') add up with the printed one.
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            effective_ward_life_cost,
        )

        ward_life = effective_ward_life_cost(state, obj)
        if ward_life:
            p = state.player_by_id(caster_id)
            n = ward_life
            if p.life < n:
                state.log.append(
                    f"Spell countered by {obj.printing.name}'s Ward "
                    f"(can't pay {n} life)"
                )
                return True
            p.life -= n
            state.log.append(
                f"P{caster_id} pays {n} life for {obj.printing.name}'s Ward"
            )
            continue
        ward, ward_str = effective_ward_cost(state, obj)
        if ward is None:
            continue
        if can_pay_cost(state, caster_id, ward):
            auto_tap_for_cost(state, caster_id, ward)
            state.log.append(
                f"P{caster_id} pays Ward {ward_str} "
                f"for {obj.printing.name}"
            )
        else:
            state.log.append(
                f"Spell countered by {obj.printing.name}'s Ward "
                f"{ward_str} (unpaid)"
            )
            return True
    return False


def _effective_ability_mana_cost(
    state: GameState, source: "GameObject", ability,
) -> ManaCost:
    """Return the mana portion of ``ability.cost`` after applying any
    ability-level cost reductions:

      - The Dominion Bracelet's "costs {X} less, where X is this
        creature's power" (flag ``mana_reduced_by_source_power``).
      - Conditional ``CostReduction`` rules attached to the ability via
        ``ActivatedAbility.cost_reductions``. Routed through the same
        ``_evaluate_cost_reduction`` helper that spell-cost reductions
        use, so any structural ``Condition`` supported there works
        here too — Starport Security's ``ControlPermanent(IsCreature
        () & HasCounter("+1/+1"))`` and Thaumaton Torpedo's
        ``AttackedWithThisTurn(HasSubtype("Spacecraft"))`` are the
        live examples.

    Clamps the generic portion at zero.
    """
    from mtgz_ir.game import effective_power
    from mtgz_ir.creature_statics_wg import ability_cost_increase

    cost = ability.cost.mana
    # Named-card activation tax (Anointed Peacekeeper).
    increase = ability_cost_increase(state, source, ability)
    if increase:
        base = cost if cost is not None else ManaCost()
        cost = ManaCost(
            generic=base.generic + increase, colored=base.colored,
            x_pips=base.x_pips, hybrid=base.hybrid, phyrexian=base.phyrexian,
        )
    if cost is None:
        return cost
    if ability.cost.mana_reduced_by_source_power:
        reduction = effective_power(state, source)
        if reduction > 0:
            cost = cost.reduced(reduction)
    for rule in getattr(ability, "cost_reductions", ()):
        reduction = _evaluate_cost_reduction(
            rule, state, source.controller_id,
        )
        if reduction > 0:
            cost = cost.reduced(reduction)
            state.log.append(
                f"ability cost reduced by {reduction} "
                f"({type(rule.condition).__name__})"
            )
    if is_artifact_now(state, source) and source in state.battlefield:
        from mtgz_ir.game import _source_layer6_wiped

        reduction = sum(
            o.printing.artifact_ability_cost_reduction
            for o in state.battlefield
            if o.printing.artifact_ability_cost_reduction
            and o.controller_id == source.controller_id
            and not _source_layer6_wiped(state, o)
        )
        if reduction:
            # Forensic Gadgeteer: never below one mana.
            total = cost.total
            reduction = min(reduction, cost.generic, max(0, total - 1))
            cost = cost.reduced(reduction)
    # Zirda-style "Abilities you activate ... cost {N} less" statics.
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        activation_cost_reduction,
    )

    return activation_cost_reduction(state, source.controller_id, ability, cost)


def _choose_ability_targets(
    state: GameState, player_id: int, source: "GameObject", effect,
) -> "tuple[tuple[str, int], ...]":
    """Pick targets for an activated ability's ``effect`` via the
    controller's agent (multi-slot aware; mirrors the inline selection
    in ``_activate_ability``). Returns ``()`` when nothing is chosen."""
    if not effect.requires_target():
        return ()
    state.resolving_source = source
    try:
        legal = effect.legal_targets(state, player_id)
    finally:
        state.resolving_source = None
    _, max_t = effect.target_count_range()
    if max_t <= 0:
        max_t = 1
    if not legal:
        return ()
    p = state.player_by_id(player_id)
    chosen_list: list[tuple[str, int]] = []
    remaining = list(legal)
    slot_kinds = effect.target_slot_kinds()
    for slot_idx in range(max_t):
        if slot_kinds and slot_idx < len(slot_kinds):
            pool = [t for t in remaining if t[0] == slot_kinds[slot_idx]]
        else:
            pool = remaining
        if not pool:
            break
        pick = (
            p.agent.choose_target(state, player_id, pool)
            if p.agent is not None else pool[0]
        )
        if pick is None or pick not in pool:
            break
        chosen_list.append(pick)
        remaining.remove(pick)
    return tuple(chosen_list)


def _activate_hand_ability(
    state: GameState, player_id: int, card: "GameObject", ability_index: int,
) -> None:
    """Activate ``card.printing.hand_activated_abilities[ability_index]``
    from ``player_id``'s hand (CR 602.2; Cycling 702.29, Channel).

    Cost payment order: mana (auto-tap, with the ability's own cost
    reductions), then "Discard this card" — the card moves to its
    owner's graveyard as a discard, firing discard triggers. Targets
    are chosen before the discard (CR 601.2c precedes 601.2h). The
    ability then goes on the stack with the (now-graveyard) card as its
    source; abilities exist independently of their source (CR 113.7a).
    """
    from mtgz_ir.game import Zone
    from mtgz_ir.turn import fire_opponent_discards_triggers

    abilities = card.printing.hand_activated_abilities
    if ability_index < 0 or ability_index >= len(abilities):
        return
    ability = abilities[ability_index]
    if (
        ability.activate_only_if is not None
        and not ability.activate_only_if.matches(state, player_id)
    ):
        return
    p = state.player_by_id(player_id)
    from mtgz_ir.creature_statics_wg import ability_cost_increase

    if ability.cost.mana is not None or ability_cost_increase(state, card, ability):
        mana = _effective_ability_mana_cost(state, card, ability)
        if not can_pay_cost(state, player_id, mana):
            return
        auto_tap_for_cost(state, player_id, mana, spell_kind="spell")
    targets = _choose_ability_targets(state, player_id, card, ability.effect)
    cost_paid_objects: list[GameObject] = []
    if ability.cost.extra_costs:
        from mtgz_ir.effects.generic.cube_creatures_rmc import pay_extra_costs

        cost_paid_objects.extend(
            pay_extra_costs(state, player_id, card, ability.cost)
        )
    if ability.cost.return_unblocked_attacker:
        # Ninjutsu (CR 702.49a): return an unblocked attacker you
        # control to its owner's hand as part of the cost.
        from mtgz_ir.legal import unblocked_attackers
        from mtgz_ir.turn import move_off_battlefield

        options = unblocked_attackers(state, player_id)
        if not options:  # pragma: no cover - legal_actions gates
            return
        returned = options[0]
        if p.agent is not None:
            pick = p.agent.choose_sacrifice(
                state, player_id, [o.id for o in options],
            )
            returned = next((o for o in options if o.id == pick), returned)
        state.log.append(
            f"P{player_id} returns unblocked {returned.printing.name} to "
            f"hand (ninjutsu cost)"
        )
        move_off_battlefield(state, returned, "hand")
        cost_paid_objects.append(returned)
    if ability.cost.discard_self:
        p.hand.remove(card)
        card.zone = Zone.GRAVEYARD
        p.graveyard.append(card)
        state.log.append(
            f"P{player_id} discards {card.printing.name} "
            f"(cost for its ability)"
        )
        fire_opponent_discards_triggers(state, player_id, card=card)
        cost_paid_objects.append(card)
    elif ability.cost.exile_self:
        # "Exile this card from your hand" (Simian Spirit Guide shape).
        p.hand.remove(card)
        card.zone = Zone.EXILE
        p.exile.append(card)
        state.log.append(
            f"P{player_id} exiles {card.printing.name} from hand "
            f"(cost for its ability)"
        )
        cost_paid_objects.append(card)
    if ability.is_mana_ability:
        # CR 605.3a: mana abilities resolve immediately.
        state.resolving_source = card
        try:
            ability.effect.resolve(state, player_id, targets)
        finally:
            state.resolving_source = None
        return
    state.stack.append(
        StackItem(
            source=card,
            controller_id=player_id,
            ability=ability,
            targets=targets,
            cost_paid_objects=tuple(cost_paid_objects),
        )
    )
    state.log.append(
        f"P{player_id} activates {card.printing.name}'s ability from hand"
        + _format_log_targets(state, targets)
    )
    if targets:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            fire_becomes_target,
        )

        fire_becomes_target(state, targets, card, player_id)


def _mayhem_permission(state: GameState, player_id: int, card) -> bool:
    from mtgz_ir.layouts import mayhem_castable

    return mayhem_castable(state, player_id, card)


def _lurrus_permission_for(state: GameState, player_id: int, card):
    from mtgz_ir.effects.generic.cube_creatures_rmc import lurrus_castable

    return lurrus_castable(state, player_id, card)


def _activate_graveyard_ability(
    state: GameState, player_id: int, card: "GameObject", ability_index: int,
) -> None:
    """Activate ``card.printing.graveyard_activated_abilities[ability_index]``
    from ``player_id``'s graveyard (Seasoned Pyromancer, Otharri).

    Cost order: mana (auto-tap), targets, then the composable cost atoms
    (``extra_costs`` — "Exile this card from your graveyard", "Tap an
    untapped Rebel you control"). The ability goes on the stack with the
    card as its source even if the cost moved it (CR 113.7a).
    """
    abilities = card.printing.graveyard_activated_abilities
    if ability_index < 0 or ability_index >= len(abilities):
        return
    ability = abilities[ability_index]
    if ability.cost.mana is not None:
        mana = _effective_ability_mana_cost(state, card, ability)
        if not can_pay_cost(state, player_id, mana):
            return
        auto_tap_for_cost(state, player_id, mana, spell_kind="spell")
    targets = _choose_ability_targets(state, player_id, card, ability.effect)
    cost_paid_objects: list[GameObject] = []
    if ability.cost.extra_costs:
        from mtgz_ir.effects.generic.cube_creatures_rmc import pay_extra_costs

        cost_paid_objects.extend(
            pay_extra_costs(state, player_id, card, ability.cost)
        )
    state.stack.append(
        StackItem(
            source=card,
            controller_id=player_id,
            ability=ability,
            targets=targets,
            cost_paid_objects=tuple(cost_paid_objects),
        )
    )
    state.log.append(
        f"P{player_id} activates {card.printing.name}'s ability from the "
        f"graveyard" + _format_log_targets(state, targets)
    )
    if targets:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            fire_becomes_target,
        )

        fire_becomes_target(state, targets, card, player_id)


def _activate_ability(
    state: GameState, player_id: int, source_id: int, ability_index: int
) -> None:
    """Pay an activated ability's cost and put it on the stack."""
    from mtgz_ir.turn import move_off_battlefield

    source = next((o for o in state.battlefield if o.id == source_id), None)
    if source is not None:
        # "Activated abilities of <types> can't be activated." (Clarion
        # Conqueror; legal_actions never offers these).
        from mtgz_ir.creature_statics_wg import activation_locked

        if activation_locked(state, source):  # pragma: no cover - gated
            return
    if source is None:
        # Hand-resident source: Cycling / Channel style abilities that
        # function from the owner's hand (``hand_activated_abilities``).
        hand_card = next(
            (c for c in state.player_by_id(player_id).hand
             if c.id == source_id),
            None,
        )
        if hand_card is not None:
            _activate_hand_ability(
                state, player_id, hand_card, ability_index,
            )
            return
        # Graveyard-resident source (Seasoned Pyromancer, Otharri).
        grave_card = next(
            (c for c in state.player_by_id(player_id).graveyard
             if c.id == source_id),
            None,
        )
        if grave_card is not None:
            _activate_graveyard_ability(
                state, player_id, grave_card, ability_index,
            )
            return
        # Exile-resident source (Greater Gargadon while suspended;
        # Vintage Roto P3).
        from mtgz_ir.effects.generic.roto_mana import activate_exile_ability

        activate_exile_ability(state, player_id, source_id, ability_index)
        return
    if source.controller_id != player_id:
        return
    from mtgz_ir.game import effective_activated_abilities
    abilities = effective_activated_abilities(state, source)
    if ability_index < 0 or ability_index >= len(abilities):
        return
    ability = abilities[ability_index]
    if (
        ability.activate_only_if is not None
        and not ability.activate_only_if.matches(state, player_id)
    ):
        return

    # Accumulate every discrete GameObject paid as part of this
    # activation's cost (sacrificed permanents, exiled-from-graveyard
    # cards, equipment exiled to grant the ability, etc.). Threaded
    # onto the StackItem so resolution can reference "the sacrificed
    # artifact" / "the discarded card" via
    # ``state.resolving_cost_paid_objects`` rather than re-deriving it
    # from a graveyard heuristic. Mana / tap costs do not contribute
    # — they pay no discrete object the resolving body could reference.
    cost_paid_objects: list[GameObject] = []

    # Pay mana first (auto-tap lands). Dominion Bracelet's granted
    # ability reduces its generic cost by the equipped creature's
    # effective power. For abilities with an X in the cost (Blast
    # Zone's {X}{X}: put X charge counters on this land), ask the
    # agent for X, fold X*x_pips into generic, and stash the chosen
    # X on the StackItem so the resolving effect can read it via
    # state.resolving_x_value.
    ability_x_value = 0
    from mtgz_ir.creature_statics_wg import ability_cost_increase

    if ability.cost.mana is not None or ability_cost_increase(state, source, ability):
        mana = _effective_ability_mana_cost(state, source, ability)
        if mana.x_pips > 0:
            ability_x_value = _choose_and_apply_x(
                state, player_id, source, mana,
            )
            mana = ManaCost(
                generic=mana.generic + mana.x_pips * ability_x_value,
                colored=mana.colored,
                x_pips=0,
            )
        # Build the set of activation-context spend-restriction tags.
        # Eldrazi Temple's second mana ability lets the colorless
        # Eldrazi owner pay for activations on their Eldrazi.
        # Steelswarm Operator's second mana ability lets the
        # controller pay for activations on artifact sources. Plaza of
        # Heroes's third mana ability ("among legendary permanents you
        # control") is read as "spend only to activate abilities of
        # legendary permanents you control".
        # "activated_ability" marks the payment as an activation, not a
        # cast (The Mightstone and Weakstone's mana pays for it).
        ability_spell_kinds: set[str] = {"spell", "activated_ability"}
        if (
            "Eldrazi" in source.printing.subtypes
            and not effective_colors(state, source)
        ):
            ability_spell_kinds.add("colorless_eldrazi_ability")
        if is_artifact_now(state, source):
            ability_spell_kinds.add("artifact_activation")
        if source.printing.is_legendary:
            ability_spell_kinds.add("legendary_permanent_ability")
        if source.printing.is_land:
            ability_spell_kinds.add("land_activation")
        if ability.cost.waterbend:
            from mtgz_ir.layouts import pay_waterbend

            mana = pay_waterbend(
                state, player_id, source, mana, frozenset(ability_spell_kinds),
            )
        if mana.phyrexian:
            # Phyrexian pips in an activation cost ({9}{G/P}: Etali,
            # Primal Conqueror) are paid with 2 life or mana of their
            # color, like a spell's (CR 107.4f).
            mana = _settle_phyrexian_pips(
                state, player_id, source.printing.name, mana,
                frozenset(ability_spell_kinds),
            )
        auto_tap_for_cost(
            state, player_id, mana,
            spell_kind=frozenset(ability_spell_kinds),
        )

    # Tap-self (if cost requires it).
    if ability.cost.tap_self:
        from mtgz_ir.turn import tap_permanent
        tap_permanent(state, source)
    if ability.cost.exert_self:
        source.exerted = True
        state.log.append(f"P{player_id} exerts {source.printing.name}")

    # "Tap X untapped artifacts you control" cost (Secluded Starforge).
    # X = how many untapped artifacts the activator chooses to tap. The
    # chosen count is bounded by the number of untapped artifacts they
    # control (excluding the source, which isn't an artifact here but is
    # excluded defensively) and routed through the same choose_x agent
    # hook {X} mana costs use. The engine taps the lowest-id-first X
    # artifacts deterministically. The chosen X is stashed into
    # ability_x_value so it rides the StackItem and the resolving
    # power_bonus_from_x pump reads it via state.resolving_x_value.
    if ability.cost.tap_x_untapped_artifacts_you_control:
        from mtgz_ir.turn import tap_permanent

        p = state.player_by_id(player_id)
        candidates = sorted(
            (
                o for o in state.battlefield
                if o.controller_id == player_id
                and o.id != source.id
                and is_artifact_now(state, o)
                and not o.tapped
            ),
            key=lambda o: o.id,
        )
        upper_bound = len(candidates)
        if p.agent is not None:
            chosen_n = p.agent.choose_x(
                state, player_id, source.printing.name, upper_bound
            )
        else:  # pragma: no cover - games run with agents set
            chosen_n = 0
        if chosen_n < 0:
            chosen_n = 0
        if chosen_n > upper_bound:
            chosen_n = upper_bound
        for o in candidates[:chosen_n]:
            tap_permanent(state, o)
            cost_paid_objects.append(o)
        ability_x_value = chosen_n
        state.log.append(
            f"P{player_id} taps {chosen_n} untapped artifact(s) "
            f"(cost for {source.printing.name}'s ability)"
        )

    # Roto P4: a "-X" loyalty cost -- choose X and remove X loyalty.
    if ability.cost.loyalty_x:
        from mtgz_ir.effects.generic.roto_walkers import (
            choose_and_pay_loyalty_x,
        )

        ability_x_value = choose_and_pay_loyalty_x(state, player_id, source)

    # Target selection for the effect (if it targets). Source is exposed
    # via state.resolving_source during selection so effects with
    # self-exclusion filters (e.g. "another target creature") can read
    # it. Cleared after selection; re-set at resolve_top_of_stack.
    # When the cost sacrifices the source, exclude the source from
    # legal targets — Strip Mine, Petrified Field, etc. can't target
    # themselves because they're sacrificed as part of cost.
    # Supports multi-target via Effect.target_count_range() → (min, max)
    # (Nesting Grounds's "from target permanent you control onto a
    # second target permanent" picks two targets per activation).
    targets: tuple[tuple[str, int], ...] = ()
    # "Up to one target" abilities (count_min 0) report
    # ``requires_target() == False`` but still offer a target choice;
    # the agent may decline (``choose_target`` -> None). Mirrors the
    # spell / trigger paths, which key on ``target_count_range``.
    # "Untap X target lands" (Candelabra) takes exactly X targets.
    x_targets = (
        getattr(ability.effect, "target_count_is_x", False)
        and ability_x_value > 0
    )
    if (
        ability.effect.requires_target()
        or ability.effect.target_count_range()[1] > 0
        or x_targets
    ):
        state.resolving_source = source
        try:
            from mtgz_ir.game import drop_protected_players
            legal = drop_protected_players(
                state, ability.effect.legal_targets(state, player_id),
            )
        finally:
            state.resolving_source = None
        legal = filter_targetable(state, legal, source, player_id)
        if ability.cost.sacrifice_self or ability.cost.exile_self:
            legal = [t for t in legal if t[1] != source.id]
        _, max_t = ability.effect.target_count_range()
        if max_t <= 0:
            max_t = 1  # back-compat with older requires_target-only effects
        if getattr(ability.effect, "target_count_is_x", False):
            # "Untap X target lands" (Candelabra of Tawnos): the target
            # count is the X paid above.
            max_t = ability_x_value
        if legal:
            p = state.player_by_id(player_id)
            chosen_list: list[tuple[str, int]] = []
            remaining = list(legal)
            slot_kinds = ability.effect.target_slot_kinds()
            # Effects whose target slots carry DIFFERENT filters
            # ("target artifact you control ... another target artifact
            # or creature you control") enumerate each slot themselves.
            per_slot = getattr(
                ability.effect, "legal_targets_for_slot", None,
            )
            for slot_idx in range(max_t):
                if per_slot is not None:
                    pool = [
                        t for t in (
                            per_slot(
                                state, player_id, slot_idx,
                                chosen=tuple(chosen_list),
                            )
                            if getattr(
                                ability.effect,
                                "slot_targets_depend_on_chosen", False,
                            )
                            else per_slot(state, player_id, slot_idx)
                        )
                        if t not in chosen_list
                        and not (
                            (ability.cost.sacrifice_self
                             or ability.cost.exile_self)
                            and t[1] == source.id
                        )
                    ]
                elif slot_kinds and slot_idx < len(slot_kinds):
                    needed = slot_kinds[slot_idx]
                    pool = [t for t in remaining if t[0] == needed]
                else:
                    pool = remaining
                if not pool:
                    break
                if p.agent is not None:
                    pick = p.agent.choose_target(
                        state, player_id, pool,
                    )
                else:
                    pick = pool[0]
                if pick is None or pick not in pool:
                    break
                chosen_list.append(pick)
                if pick in remaining:
                    remaining.remove(pick)
            targets = tuple(chosen_list)

    # "Sacrifice another ..." paid BEFORE sacrifice-self so the source
    # stays on the battlefield and is excluded from the option set.
    # legal_actions gates activation on a valid sac existing, so the
    # option list is non-empty when we reach this code.
    if (
        ability.cost.sacrifice_another_creature_or_artifact
        or ability.cost.sacrifice_another_artifact_or_land
        or ability.cost.sacrifice_another_artifact
    ):
        from mtgz_ir.turn import sacrifice as _sacrifice

        p = state.player_by_id(player_id)
        if ability.cost.sacrifice_another_creature_or_artifact:
            def _is_valid_sac(o):
                return o.printing.is_creature or is_artifact_now(state, o)
        elif ability.cost.sacrifice_another_artifact_or_land:
            def _is_valid_sac(o):
                return is_artifact_now(state, o) or o.printing.is_land
        else:
            def _is_valid_sac(o):
                return is_artifact_now(state, o)
        options = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and o.id != source.id
            and _is_valid_sac(o)
        ]
        chosen = options[0]
        if p.agent is not None:
            pick_id = p.agent.choose_sacrifice(
                state, player_id, [o.id for o in options]
            )
            chosen = next(
                (o for o in options if o.id == pick_id), options[0]
            )
        state.log.append(
            f"P{player_id} sacrifices {chosen.printing.name} "
            f"(cost for {source.printing.name}'s ability)"
        )
        _sacrifice(state, chosen)
        cost_paid_objects.append(chosen)

    # "Remove a +1/+1 counter from a creature you control" cost. The
    # agent picks which creature via choose_sacrifice (reusing the
    # existing sacrifice-picker agent hook since the shape is the
    # same: pick an id from a list of battlefield ids). Decrements by
    # one; legal_actions gates on at least one such creature existing.
    if ability.cost.remove_plus_one_counter_from_your_creature:
        p = state.player_by_id(player_id)
        options = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and o.printing.is_creature
            and o.plus_one_counters > 0
        ]
        chosen = options[0]
        if p.agent is not None:
            pick_id = p.agent.choose_sacrifice(
                state, player_id, [o.id for o in options]
            )
            chosen = next(
                (o for o in options if o.id == pick_id), options[0]
            )
        chosen.plus_one_counters -= 1
        state.log.append(
            f"P{player_id} removes a +1/+1 counter from "
            f"{chosen.printing.name} (now {chosen.plus_one_counters}) "
            f"(cost for {source.printing.name}'s ability)"
        )

    # "Remove a <kind> counter from this <permanent>" (Gemstone Mine).
    if ability.cost.remove_named_counter_from_self:
        from mtgz_ir.game import named_counter_count, set_named_counters
        kind = ability.cost.remove_named_counter_from_self
        have = named_counter_count(source, kind)
        if have < 1:  # pragma: no cover - legal_actions gates this
            return
        set_named_counters(source, kind, have - 1)
        state.log.append(
            f"P{player_id} removes a {kind} counter from "
            f"{source.printing.name} ({have - 1} left)"
        )

    if ability.cost.pay_energy:
        p = state.player_by_id(player_id)
        p.energy -= ability.cost.pay_energy
        state.log.append(
            f"P{player_id} pays {ability.cost.pay_energy} energy "
            f"(cost for {source.printing.name}'s ability)"
        )

    # "Pay N life" cost (Mana Confluence etc.). Straight life debit.
    if ability.cost.pay_life:
        p = state.player_by_id(player_id)
        p.life -= ability.cost.pay_life
        state.log.append(
            f"P{player_id} pays {ability.cost.pay_life} life "
            f"(cost for {source.printing.name}'s ability; life: {p.life})"
        )

    # Planeswalker loyalty cost (CR 606). Signed subtraction handles
    # both directions uniformly: positive cost.loyalty (a "-N" ability)
    # decrements loyalty_counters; negative (a "+N" ability) increments.
    # Zero ("0:" ability) is a no-op on the counter but still records
    # the once-per-turn ledger below. Mirrors Rust's apply commit at
    # state_ops_actions.rs (``obj.loyalty_counters -= cost.loyalty``).
    # The loyalty-at-0 SBA (turn.py 704.5i) catches a drop to 0 on the
    # next SBA pass — the commit itself is unconditional (CR 606.5).
    if ability.cost.loyalty != 0:
        source.loyalty_counters -= ability.cost.loyalty
        state.log.append(
            f"P{player_id} {'pays' if ability.cost.loyalty > 0 else 'adds'} "
            f"{abs(ability.cost.loyalty)} loyalty "
            f"({source.printing.name} now at {source.loyalty_counters})"
        )
    # CR 606.3 once-per-turn-per-walker ledger. Recorded for EVERY
    # loyalty ability of a planeswalker source (including the "0"
    # ability, where cost.loyalty == 0). Keyed on (controller, source)
    # — per-walker, not per-ability. ``legal_actions`` reads this set
    # to gate ALL of the walker's abilities off for the rest of the
    # turn. Mirrors Rust's ledger insert at state_ops_actions.rs.
    # Gated on the source being a planeswalker so a non-walker activated
    # ability (which never carries loyalty) doesn't pollute the ledger.
    if source.printing.is_planeswalker:
        key = (player_id, source.id)
        extra = source.extra_loyalty_activations
        if (
            key in state.loyalty_activated_this_turn
            and extra and extra[0] == state.turn_number and extra[1] > 0
        ):
            # An additional activation granted this turn (Comet).
            source.extra_loyalty_activations = (extra[0], extra[1] - 1)
        state.loyalty_activated_this_turn.add(key)

    # "Sacrifice a land" cost (Dust Bowl). Agent picks; excludes source.
    if ability.cost.sacrifice_a_land:
        from mtgz_ir.turn import sacrifice as _sacrifice

        p = state.player_by_id(player_id)
        options = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and o.printing.is_land
            and o.id != source.id
        ]
        if options:
            chosen = options[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, player_id, [o.id for o in options]
                )
                chosen = next(
                    (o for o in options if o.id == pick_id), options[0]
                )
            state.log.append(
                f"P{player_id} sacrifices {chosen.printing.name} "
                f"(cost for {source.printing.name}'s ability)"
            )
            _sacrifice(state, chosen)
            cost_paid_objects.append(chosen)

    # "Sacrifice a Desert" cost (Scavenger Grounds). Subtype-filtered.
    if ability.cost.sacrifice_a_desert:
        from mtgz_ir.game import effective_subtypes
        from mtgz_ir.turn import sacrifice as _sacrifice

        p = state.player_by_id(player_id)
        options = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and "Desert" in effective_subtypes(state, o)
        ]
        if options:
            chosen = options[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, player_id, [o.id for o in options]
                )
                chosen = next(
                    (o for o in options if o.id == pick_id), options[0]
                )
            state.log.append(
                f"P{player_id} sacrifices {chosen.printing.name} "
                f"(cost for {source.printing.name}'s ability)"
            )
            _sacrifice(state, chosen)
            cost_paid_objects.append(chosen)

    # "Exile this equipment" cost (Dominion Bracelet). Walk the
    # source's attached equipment with a matching grant flag and exile
    # one. Legal_actions already gated on at least one existing.
    if ability.cost.exile_granting_equipment:
        granter = next(
            (
                eq for eq in state.battlefield
                if eq.attached_to == source.id
                and eq.printing.grants_control_opponent_ability_when_equipped
            ),
            None,
        )
        if granter is not None:
            from mtgz_ir.turn import move_off_battlefield
            state.log.append(
                f"P{player_id} exiles {granter.printing.name} "
                f"(cost for {source.printing.name}'s ability)"
            )
            move_off_battlefield(state, granter, destination="exile")
            cost_paid_objects.append(granter)

    # "Sacrifice a Food" cost (Ragost). Agent picks via choose_sacrifice.
    # Food status read through effective_subtypes so Ragost-granted
    # Foods count. legal_actions gates on at least one legal Food
    # being available.
    if ability.cost.sacrifice_food:
        from mtgz_ir.game import effective_subtypes
        from mtgz_ir.turn import sacrifice as _sacrifice

        p = state.player_by_id(player_id)
        options = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and "Food" in effective_subtypes(state, o)
        ]
        chosen = options[0]
        if p.agent is not None:
            pick_id = p.agent.choose_sacrifice(
                state, player_id, [o.id for o in options]
            )
            chosen = next(
                (o for o in options if o.id == pick_id), options[0]
            )
        state.log.append(
            f"P{player_id} sacrifices {chosen.printing.name} "
            f"(Food cost for {source.printing.name}'s ability)"
        )
        _sacrifice(state, chosen)
        cost_paid_objects.append(chosen)

    # "Tap an untapped permanent you control" cost (Gene Pollinator).
    # Agent picks an untapped permanent they control (excluding source).
    # legal_actions gates on at least one such permanent existing.
    if ability.cost.tap_permanent_you_control:
        from mtgz_ir.turn import tap_permanent

        p = state.player_by_id(player_id)
        options = [
            o for o in state.battlefield
            if o.controller_id == player_id
            and o.id != source.id
            and not o.tapped
            and (
                not ability.cost.tap_permanent_type
                or has_card_type(state, o, ability.cost.tap_permanent_type)
            )
        ]
        chosen = options[0]
        if p.agent is not None:
            pick_id = p.agent.choose_permanent_to_tap(
                state, player_id, [o.id for o in options]
            )
            if pick_id is not None:
                chosen = next(
                    (o for o in options if o.id == pick_id), options[0]
                )
        state.log.append(
            f"P{player_id} taps {chosen.printing.name} "
            f"(cost for {source.printing.name}'s ability)"
        )
        tap_permanent(state, chosen)
        cost_paid_objects.append(chosen)

    # "Exile N cards from your graveyard" cost (Psychic Frog). The agent
    # picks each card; legal_actions gates on N cards being there.
    if ability.cost.exile_cards_from_graveyard:
        p = state.player_by_id(player_id)
        for _ in range(ability.cost.exile_cards_from_graveyard):
            if not p.graveyard:  # pragma: no cover - gated by legal_actions
                break
            chosen_card = p.graveyard[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, player_id, [c.id for c in p.graveyard]
                )
                chosen_card = next(
                    (c for c in p.graveyard if c.id == pick_id), chosen_card
                )
            p.graveyard.remove(chosen_card)
            chosen_card.zone = Zone.EXILE
            p.exile.append(chosen_card)
            cost_paid_objects.append(chosen_card)
        state.log.append(
            f"P{player_id} exiles {ability.cost.exile_cards_from_graveyard} "
            f"cards from graveyard (cost for {source.printing.name}'s ability)"
        )

    # "Exile an artifact card from your graveyard" cost (Rust Harvester).
    # Agent picks which artifact card from graveyard to exile.
    # legal_actions gates on at least one artifact card in graveyard.
    if ability.cost.exile_artifact_from_graveyard:
        p = state.player_by_id(player_id)
        options = [
            card for card in p.graveyard
            if is_artifact_now(state, card)
        ]
        chosen_card = options[0]
        if p.agent is not None:
            pick_id = p.agent.choose_sacrifice(
                state, player_id, [c.id for c in options]
            )
            chosen_card = next(
                (c for c in options if c.id == pick_id), options[0]
            )
        state.log.append(
            f"P{player_id} exiles {chosen_card.printing.name} from graveyard "
            f"(cost for {source.printing.name}'s ability)"
        )
        p.graveyard.remove(chosen_card)
        p.exile.append(chosen_card)
        cost_paid_objects.append(chosen_card)

    if ability.cost.discard_a_card:
        from mtgz_ir.turn import fire_opponent_discards_triggers

        p = state.player_by_id(player_id)
        if not p.hand:
            return
        hand_ids = [c.id for c in p.hand]
        pick_id = hand_ids[0]
        if p.agent is not None:
            pick_id = p.agent.choose_discard(state, player_id, hand_ids)
        discarded = next(
            (c for c in p.hand if c.id == pick_id), p.hand[0],
        )
        p.hand.remove(discarded)
        discarded.zone = Zone.GRAVEYARD
        p.graveyard.append(discarded)
        cost_paid_objects.append(discarded)
        state.log.append(
            f"P{player_id} discards {discarded.printing.name} "
            f"(cost for {source.printing.name}'s ability)"
        )
        fire_opponent_discards_triggers(state, player_id, card=discarded)
    # Crew N cost (CR 702.122): tap untapped creatures with total power
    # N or more. The activator picks one creature at a time until the
    # total is reached; legal_actions already guaranteed it's reachable.
    if ability.cost.saddle:
        from mtgz_ir.creature_casting_ub import pay_saddle_cost

        cost_paid_objects.extend(
            pay_saddle_cost(state, player_id, source, ability.cost.saddle)
        )

    if ability.cost.crew:
        from mtgz_ir.game import crew_candidates, crew_power
        from mtgz_ir.turn import tap_permanent

        p = state.player_by_id(player_id)
        chosen_crew: list = []
        total = 0
        while total < ability.cost.crew:
            options = [
                o for o in crew_candidates(state, player_id, source)
                if o not in chosen_crew
            ]
            if not options:
                break
            pick = options[0]
            if p.agent is not None:
                pick_id = p.agent.choose_permanent_to_tap(
                    state, player_id, [o.id for o in options],
                )
                pick = next((o for o in options if o.id == pick_id), options[0])
            chosen_crew.append(pick)
            total += crew_power(state, pick)
        for o in chosen_crew:
            tap_permanent(state, o)
            cost_paid_objects.append(o)
        state.log.append(
            f"P{player_id} crews {source.printing.name} with "
            f"{', '.join(o.printing.name for o in chosen_crew)}"
        )

    # "Sacrifice N nonland permanents" cost (Bolas's Citadel). Picked one
    # at a time; the source is a legal choice; legal_actions guaranteed
    # at least N exist.
    if ability.cost.sacrifice_n_nonland_permanents:
        from mtgz_ir.turn import sacrifice as _sacrifice

        p = state.player_by_id(player_id)
        picked: list = []
        for _ in range(ability.cost.sacrifice_n_nonland_permanents):
            options = [
                o for o in state.battlefield
                if o.controller_id == player_id
                and not o.printing.is_land
                and o not in picked
            ]
            if not options:
                break
            chosen = options[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, player_id, [o.id for o in options],
                )
                chosen = next((o for o in options if o.id == pick_id), options[0])
            picked.append(chosen)
        for o in picked:
            state.log.append(
                f"P{player_id} sacrifices {o.printing.name} "
                f"(cost for {source.printing.name}'s ability)"
            )
            _sacrifice(state, o)
            cost_paid_objects.append(o)

    # "Remove N <kind> counters from this <permanent>" cost (Staff of
    # the Storyteller, Wishclaw Talisman, Reckoner Bankbuster).
    if ability.cost.remove_counters_from_self_kind:
        from mtgz_ir.game import remove_counters

        kind = ability.cost.remove_counters_from_self_kind
        removed = remove_counters(
            source, kind, ability.cost.remove_counters_from_self_amount,
        )
        state.log.append(
            f"P{player_id} removes {removed} {kind} counter(s) from "
            f"{source.printing.name} (cost)"
        )

    # "Sacrifice a <Subtype>" / "Sacrifice a nontoken artifact" costs
    # (Retrofitter Foundry, Thopter Foundry). The source itself is a
    # legal choice when it qualifies. legal_actions gates on one existing.
    if (
        ability.cost.sacrifice_subtype
        or ability.cost.sacrifice_nontoken_artifact
        or ability.cost.sacrifice_a_permanent_type
    ):
        from mtgz_ir.game import effective_subtypes
        from mtgz_ir.turn import sacrifice as _sacrifice

        p = state.player_by_id(player_id)
        if ability.cost.sacrifice_a_permanent_type:
            # "Sacrifice a creature" (Goblin Bombardment).
            from mtgz_ir.effects.generic.enchantment_mechanics import (
                sacrifice_type_candidates,
            )
            options = sacrifice_type_candidates(
                state, player_id, ability.cost.sacrifice_a_permanent_type,
            )
        elif ability.cost.sacrifice_subtype:
            wanted_sub = ability.cost.sacrifice_subtype
            options = [
                o for o in state.battlefield
                if o.controller_id == player_id
                and wanted_sub in effective_subtypes(state, o)
            ]
        else:
            options = [
                o for o in state.battlefield
                if o.controller_id == player_id
                and is_artifact_now(state, o) and not o.printing.is_token
            ]
        if options:
            chosen = options[0]
            if p.agent is not None:
                pick_id = p.agent.choose_sacrifice(
                    state, player_id, [o.id for o in options]
                )
                chosen = next(
                    (o for o in options if o.id == pick_id), options[0]
                )
            state.log.append(
                f"P{player_id} sacrifices {chosen.printing.name} "
                f"(cost for {source.printing.name}'s ability)"
            )
            _sacrifice(state, chosen)
            cost_paid_objects.append(chosen)

    # "Discard your hand" cost (Lion's Eye Diamond). Every card in the
    # activator's hand goes to the graveyard; an empty hand is a legal
    # zero-card payment. Discard observers fire once per card.
    if ability.cost.discard_hand:
        from mtgz_ir.turn import fire_opponent_discards_triggers

        p = state.player_by_id(player_id)
        discarded = list(p.hand)
        p.hand.clear()
        for card in discarded:
            card.zone = Zone.GRAVEYARD
            p.graveyard.append(card)
            state.log.append(
                f"P{player_id} discards {card.printing.name} "
                f"(cost for {source.printing.name}'s ability)"
            )
            fire_opponent_discards_triggers(state, player_id, card=card)

    # Composable cost atoms (``ActivationCost.extra_costs``): sacrifice
    # N filtered permanents, remove counters from the source, tap
    # filtered permanents. Paid after target selection, before
    # exile-self / sacrifice-self.
    if ability.cost.extra_costs:
        # Vintage Roto P3: "Pay X {E}" whose X is the targeted card's
        # mana value (Chthonian Nightmare) sets the ability's X.
        from mtgz_ir.effects.generic.roto_mana import x_from_cost_atoms

        ability_x_value = x_from_cost_atoms(
            state, player_id, ability, targets, ability_x_value,
        )
        from mtgz_ir.effects.generic.cube_creatures_rmc import pay_extra_costs

        cost_paid_objects.extend(
            pay_extra_costs(state, player_id, source, ability.cost)
        )
    # Exhaust (CR 702.177): latch this ability index on the object.
    if ability.exhaust:
        source.exhausted_abilities = source.exhausted_abilities | {ability_index}
    # Boast (CR 702.142): once each turn.
    if ability.boast:
        source.boasted_turn = state.turn_number

    # Exile-self and sacrifice-self are paid LAST so the source is still
    # on the battlefield for target selection. exile_self moves the source
    # to exile (not the graveyard); sacrifice_self triggers graveyard SBAs.
    if ability.cost.exile_self:
        from mtgz_ir.turn import move_off_battlefield

        state.log.append(
            f"P{player_id} exiles {source.printing.name}"
        )
        move_off_battlefield(state, source, destination="exile")

    # Sacrifice-self is paid LAST so the source is still on the battlefield
    # for target selection — the Lander case where the ability's effect
    # doesn't reference the source but the controller needs to still own it.
    if ability.cost.sacrifice_self:
        from mtgz_ir.turn import sacrifice as _sacrifice

        state.log.append(
            f"P{player_id} sacrifices {source.printing.name}"
        )
        _sacrifice(state, source)

    # ``Activate only once.`` rider — DSK Haunted Screen's third
    # activation. Latch the per-source ``has_activated_once_only_
    # ability`` flag now (after the cost is paid and before the
    # ability is resolved or pushed) so a second activation of the
    # same ability — even before this one resolves off the stack — is
    # rejected by ``legal_actions``'s gate against the flag.
    if ability.activate_only_once:
        source.has_activated_once_only_ability = True

    # Mana abilities bypass the stack — resolve the effect immediately
    # (MTG rule 605). Source is exposed via resolving_source for any
    # effect that needs it. Mana abilities can carry a discrete-object
    # cost (Gene Pollinator's "Tap an untapped permanent you control"),
    # so the cost-paid objects need to be visible to the resolving
    # body via state.resolving_cost_paid_objects.
    if ability.is_mana_ability:
        pool_before = len(state.player_by_id(player_id).mana_pool)
        state.resolving_source = source
        state.resolving_cost_paid_objects = tuple(cost_paid_objects)
        # Mana abilities with an {X} cost (Wizard's Rockets' "Add X
        # mana in any combination of colors") read the paid X through
        # the same ``resolving_x_value`` channel stack-resolved
        # abilities use.
        prior_x = state.resolving_x_value
        state.resolving_x_value = ability_x_value
        try:
            ability.effect.resolve(state, player_id, targets)
        finally:
            state.resolving_source = None
            state.resolving_cost_paid_objects = ()
            state.resolving_x_value = prior_x
        state.log.append(
            f"P{player_id} activates {source.printing.name}'s mana ability"
        )
        # Vintage Roto P3: what the mana ability just produced (snow
        # mana from a snow source, ...).
        from mtgz_ir.effects.generic.roto_mana import after_mana_ability

        after_mana_ability(
            state, player_id, source, pool_before,
            tapped=ability.cost.tap_self,
        )
        if ability.cost.tap_self:
            # "Whenever you tap a creature for mana, add an additional
            # {G}." (Badgermole Cub; a triggered mana ability).
            from mtgz_ir.creature_statics_wg import extra_mana_for_creature_tap

            extra_mana_for_creature_tap(state, player_id, source)
        return

    # Put the ability on the stack using the existing StackItem shape.
    state.stack.append(
        StackItem(
            source=source,
            controller_id=player_id,
            ability=ability,
            targets=targets,
            x_value=ability_x_value,
            cost_paid_objects=tuple(cost_paid_objects),
        )
    )
    state.log.append(
        f"P{player_id} activates {source.printing.name}'s ability"
        + _format_log_targets(state, targets)
    )
    if targets:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            fire_becomes_target,
        )

        fire_becomes_target(state, targets, source, player_id)
    # "Whenever an opponent activates an ability of ..." (roto P1:
    # Harsh Mentor). Mana abilities returned above.
    from mtgz_ir.effects.predicates import Event as _ActEvent

    dispatch_event(
        state, _ActEvent.ACTIVATES_ABILITY, label="activation",
        source=source, controller=state.player_by_id(player_id),
    )


def _activate_station(
    state: GameState, player_id: int, spacecraft_id: int, crew_id: int
) -> None:
    """Validate and apply a Station activation. Silently no-ops on invalid
    inputs (crew isn't eligible, spacecraft isn't under our control, etc.)."""
    from mtgz_ir.game import effective_power

    spacecraft = next(
        (o for o in state.battlefield if o.id == spacecraft_id), None
    )
    crew = next((o for o in state.battlefield if o.id == crew_id), None)
    if spacecraft is None or crew is None:
        return
    if not spacecraft.printing.has_station:
        return
    if spacecraft.controller_id != player_id:
        return
    if crew.controller_id != player_id:
        return
    if crew.id == spacecraft.id:
        return  # "another creature"
    if not crew.printing.is_creature:
        return  # spacecraft can't crew another spacecraft in Stage 3
    if crew.tapped:
        return
    if crew.summoning_sick:
        # Activated abilities with tap in their cost can't be used on
        # summoning-sick creatures, even for Station.
        return

    from mtgz_ir.turn import tap_permanent

    from mtgz_ir.game import add_charge_counters
    crew_base_power = effective_power(state, crew)
    # Check if any permanent the crew's controller controls has
    # ``grants_toughness_as_station_power_for_creatures_you_control``
    # AND the crew has toughness > power — if so, use toughness.
    from mtgz_ir.game import effective_toughness as _eff_t
    crew_station_power = crew_base_power
    controller_id = crew.controller_id
    for permanent in state.battlefield:
        if permanent.controller_id != controller_id:
            continue
        if not permanent.printing.grants_toughness_as_station_power_for_creatures_you_control:
            continue
        t = _eff_t(state, crew)
        if t > crew_base_power:
            crew_station_power = t
        break
    tap_permanent(state, crew)
    added = add_charge_counters(state, spacecraft, crew_station_power)
    state.log.append(
        f"{crew.printing.name} stations {spacecraft.printing.name} "
        f"(+{added} counters, now {spacecraft.charge_counters})"
    )


def _unlock_room_half(
    state: GameState, player_id: int, card_id: int, half_name: str,
) -> None:
    """Pay the named half's mana cost and unlock it on the Room.

    CR 715.3-4: unlocking is NOT a spell — the unlock action doesn't
    use the stack. Pay the cost the same way as casting (auto-tap
    lands), then add the half to ``unlocked_halves`` and fire any
    ``"When you unlock this door"`` triggers (which we model as
    ENTERS-event triggers tagged with the half-name; we re-dispatch
    ENTERS for the source so those gates fire).

    Silently no-ops on invalid inputs (not a Room, half already
    unlocked, not the controller, etc.) — legal_actions has gated
    these.
    """
    p = state.player_by_id(player_id)
    obj = next((o for o in state.battlefield if o.id == card_id), None)
    if obj is None or obj.controller_id != player_id:
        return
    if not obj.printing.is_room:
        return
    if half_name in obj.unlocked_halves:
        return
    half = next(
        (h for h in obj.printing.room_halves if h.name == half_name), None,
    )
    if half is None:
        return
    if not can_pay_cost(state, player_id, half.mana_cost):
        return
    auto_tap_for_cost(state, player_id, half.mana_cost)
    obj.unlocked_halves = obj.unlocked_halves | {half_name}
    state.log.append(
        f"P{player_id} unlocks {obj.printing.name}: door {half_name}"
    )
    fully_unlocked = (
        len(obj.unlocked_halves) == len(obj.printing.room_halves)
    )
    if fully_unlocked:
        state.log.append(
            f"{obj.printing.name}: Room fully unlocked"
        )
    # Fire any triggers gated to this newly-unlocked half. We model
    # "When you unlock this door, ..." as TriggeredAbility entries
    # with ``room_half=<half-name>`` and a self-targeting gate; the
    # ``effective_triggered_abilities`` iterator filters to the
    # currently-unlocked set, so a now-unlocked half's triggers
    # become reachable here. We dispatch a synthetic ENTERS event
    # over only the just-unlocked half's triggers so unrelated
    # ENTERS observers (other Rooms, ETB anthems) aren't re-fired.
    from mtgz_ir.effects.predicates import Event as _Event
    _fire_unlock_door_triggers(state, obj, half_name)
    # Duskmourn "Eerie" mechanic: whenever a Room becomes fully
    # unlocked (transition from a single-half set to the all-halves
    # set), every Eerie trigger with this Room under the trigger's
    # controller observes a ROOM_FULLY_UNLOCKED event. The dispatch
    # is anchored on the unlock-action transition (NOT on every door
    # unlock) so the per-half "When you unlock this door" path stays
    # disjoint from the whole-Room observer path. The event subject
    # ``room`` is the Room ``GameObject`` itself; predicates layer
    # ``Controlled(YOU, subject="room")`` to gate "you fully unlock
    # a Room" vs an opponent's transition.
    if fully_unlocked:
        dispatch_event(
            state,
            _Event.ROOM_FULLY_UNLOCKED,
            label="room-fully-unlocked",
            room=obj,
        )


def _fire_unlock_door_triggers(
    state: GameState, obj: "GameObject", half_name: str,
) -> None:
    """Fire triggered abilities tagged with ``room_half=half_name`` on
    ``obj``. Used by the unlock-action and by spell-resolution to
    fire "When you unlock this door, ..." triggers when a Room enters
    with a half pre-unlocked.

    Each matching trigger goes on the stack via
    ``push_triggered_ability`` so it gets a normal response window.
    """
    from mtgz_ir.effects.predicates import Event as _Event
    from mtgz_ir.game import effective_triggered_abilities

    # Build the ability index over the FULL printing (matches the
    # index space ``effective_triggered_abilities`` produces) so the
    # once-per-turn ledger and other index-keyed bookkeeping stay
    # consistent with the rest of the engine.
    abilities = effective_triggered_abilities(state, obj)
    for idx, ab in enumerate(abilities):
        if getattr(ab, "room_half", None) != half_name:
            continue
        # Only ENTERS-event triggers represent "When you unlock this
        # door" — other event types (DIES, ATTACKS, ...) should fire
        # only on their actual events and not at unlock time.
        if ab.event != _Event.ENTERS:
            continue
        # Self-target gate (the trigger is "when ~ enters" / "when you
        # unlock this door" — the subject is the source itself). We
        # fire with subject=obj.
        push_triggered_ability(
            state,
            source=obj,
            ability=ab,
            ability_index=idx,
            controller_id=obj.controller_id,
            label=f"unlock-{half_name}",
        )


def _declare_attackers(
    state: GameState, player_id: int, attacker_ids: frozenset[int]
) -> None:
    from mtgz_ir.combat import declare_attackers, eligible_attackers

    from mtgz_ir.designations import forced_attackers

    eligible_ids = {o.id for o in eligible_attackers(state, player_id)}
    valid = [oid for oid in attacker_ids if oid in eligible_ids]
    # Goaded creatures attack each combat if able (CR 701.38a).
    for oid in forced_attackers(state, player_id):
        if oid not in valid:
            valid.append(oid)
    declare_attackers(state, valid)


def _declare_blockers(
    state: GameState,
    player_id: int,
    assignments: tuple[tuple[int, int], ...],
) -> None:
    from mtgz_ir.combat import can_block, declare_blockers, eligible_blockers
    from mtgz_ir.game import effective_keywords

    eligible_ids = {o.id for o in eligible_blockers(state, player_id)}
    attackers = {o.id: o for o in state.battlefield if o.attacking}
    blockers_on_bf = {o.id: o for o in state.battlefield}

    validated: dict[int, int] = {}
    for blocker_id, attacker_id in assignments:
        if blocker_id not in eligible_ids:
            continue
        if attacker_id not in attackers:
            continue
        if blocker_id not in blockers_on_bf:
            continue
        if not can_block(state, blockers_on_bf[blocker_id], attackers[attacker_id]):
            continue
        validated[blocker_id] = attacker_id

    # Menace: if an attacker has menace and was assigned exactly one blocker,
    # the block is illegal — drop it so the attacker goes through unblocked.
    blockers_by_attacker: dict[int, list[int]] = {}
    for bid, aid in validated.items():
        blockers_by_attacker.setdefault(aid, []).append(bid)
    for aid, blocker_ids in blockers_by_attacker.items():
        # "can't be blocked except by N or more creatures" generalizes
        # menace (N=2).
        needed = attackers[aid].printing.min_blockers
        if "menace" in effective_keywords(state, attackers[aid]):
            needed = max(needed, 2)
        if len(blocker_ids) < needed:
            for bid in blocker_ids:
                validated.pop(bid, None)
            continue
        # "Enchanted creature can't be blocked by more than one creature"
        # (Meltstrider's Resolve; CR 509.1b): blocks past the first are
        # dropped.
        if len(blocker_ids) > 1 and any(
            o.attached_to == aid
            and o.printing.attached_cant_be_blocked_by_more_than_one
            for o in state.battlefield
        ):
            for bid in sorted(blocker_ids)[1:]:
                validated.pop(bid, None)

    # ``This creature must be blocked if able.`` enforcement (CR 509.1c).
    # For each attacker carrying the static requirement, if the
    # defending player didn't already assign a blocker to it, scan the
    # eligible-blockers pool for an unused defender that could legally
    # block (per ``can_block``). If one exists, force the assignment.
    # The scan is deterministic — first by id — so the engine's choice
    # is reproducible from the seed; the agent's intent for OTHER
    # attackers is preserved (we only force the missing assignment).
    used_blocker_ids = set(validated.keys())
    sorted_attacker_items = sorted(attackers.items(), key=lambda kv: kv[0])
    for aid, attacker in sorted_attacker_items:
        if not attacker.printing.must_be_blocked_if_able:
            continue
        if aid in {a for a in validated.values()}:
            continue
        for blocker_id in sorted(eligible_ids):
            if blocker_id in used_blocker_ids:
                continue
            if blocker_id not in blockers_on_bf:
                continue
            blocker = blockers_on_bf[blocker_id]
            if not can_block(state, blocker, attacker):
                continue
            validated[blocker_id] = aid
            used_blocker_ids.add(blocker_id)
            break

    declare_blockers(state, validated)


def _send_to_graveyard_or_library(state: GameState, card) -> None:
    """Route a non-permanent spell (or failed-cost / countered card)
    to its owner's graveyard, honouring CR 614 zone-redirect
    replacement effects.

    Routes through :func:`mtgz_ir.effects.modifiers
    .dispatch_replacement` on ``WOULD_GO_TO_GRAVEYARD`` with a base
    ``destination="graveyard"`` payload. Nexus-of-Fate-shape
    printings carry a SELF-scope ``StaticAbility`` whose
    ``replacement_effect=ReplacementEffect(WOULD_GO_TO_GRAVEYARD,
    Replace(new_destination="library"))`` is collected by the
    dispatcher's moving-card walker (the card is mid-flight at
    dispatch — not on the battlefield); the dispatcher applies the
    transform and this site reads the post-replacement destination
    off the payload to route the card.
    """
    from mtgz_ir.effects.modifiers import dispatch_replacement
    from mtgz_ir.effects.predicates import Event
    from mtgz_ir.game import restore_mdfc_front

    # A split half / DFC face turns back to the card's front printing
    # (CR 709.3a / 712.8a).
    restore_mdfc_front(card)
    owner = state.player_by_id(card.owner_id)
    if owner.graveyard_exile_replacement_this_turn:
        # Yawgmoth's Will: exiled instead of going to the graveyard.
        card.zone = Zone.EXILE
        owner.exile.append(card)
        state.log.append(f"{card.printing.name} is exiled instead")
        return
    from mtgz_ir.effects.generic.roto_zones import (
        exile_instead_of_graveyard,
        graveyard_exile_instead_applies,
    )

    if graveyard_exile_instead_applies(state, card):
        # Roto P2 exile-instead replacements (Rest in Peace, ...).
        exile_instead_of_graveyard(state, card)
        return
    from mtgz_ir.game import exile_with_void_counter, void_exile_replacement_active

    if not card.is_spell_copy and void_exile_replacement_active(
        state, card.owner_id,
    ):
        exile_with_void_counter(state, card)
        return
    out = dispatch_replacement(
        state, Event.WOULD_GO_TO_GRAVEYARD,
        affected_player_id=card.owner_id,
        subjects={"card": card},
        payload={"destination": "graveyard"},
    )
    destination = out.get("destination", "graveyard")
    if destination == "library":
        from mtgz_ir.game import shuffle_library
        card.zone = Zone.LIBRARY
        owner.library.append(card)
        shuffle_library(state, owner.id)
        from mtgz_ir.turn import note_cards_put_into_library

        note_cards_put_into_library(state, owner.id)
        state.log.append(
            f"{card.printing.name} is shuffled into P{card.owner_id}'s "
            f"library instead of graveyard"
        )
        return
    card.zone = Zone.GRAVEYARD
    owner.graveyard.append(card)


def push_spell_copy(
    state: GameState, printing, controller_id: int,
    targets: tuple = (), x_value: int = 0,
) -> "GameObject":
    """Put a copy of a spell onto the stack (CR 707.10) controlled by
    ``controller_id`` with ``targets``. The copy is a fresh
    ``GameObject`` flagged ``is_spell_copy`` — it is not cast (no cast
    triggers, not counted as a spell cast) and ceases to exist when it
    leaves the stack."""
    obj = state.new_object(printing, owner_id=controller_id, zone=Zone.STACK)
    obj.is_spell_copy = True
    state.stack.append(
        StackItem(
            source=obj, controller_id=controller_id,
            targets=tuple(targets), x_value=x_value,
        )
    )
    state.log.append(
        f"P{controller_id} puts a copy of {printing.name} on the stack"
        + _format_log_targets(state, tuple(targets))
    )
    # "Whenever you cast or copy ..." (magecraft) observers.
    from mtgz_ir.effects.generic.cube_creatures_rmc import (
        fire_becomes_target,
        fire_copy_triggers,
    )

    fire_copy_triggers(state, obj, controller_id)
    fire_becomes_target(state, tuple(targets), obj, controller_id)
    return obj


def move_spell_off_stack(
    state: GameState, card, destination: str = "graveyard",
) -> None:
    """Route a spell card leaving the stack without entering the
    battlefield (resolved instant / sorcery, countered spell, spell
    returned to hand).

    * A spell cast with flashback is exiled instead of going anywhere
      else (CR 702.34a).
    * ``destination="hand"`` returns the card to its owner's hand
      (buyback on resolution, Reprieve) and reveals it.
    * Otherwise the card goes to its owner's graveyard through
      ``_send_to_graveyard_or_library`` (so zone-change replacements
      such as Nexus of Fate's shuffle still apply).
    """
    if card.is_spell_copy:
        state.log.append(f"copy of {card.printing.name} ceases to exist")
        return
    if any(ce.target_id == card.id for ce in state.continuous_effects):
        # CR 400.7: the card becomes a new object in its next zone, so
        # effects on the spell end (a suspended creature spell's haste
        # when it's countered, Arena of Glory's haste).
        state.continuous_effects = [
            ce for ce in state.continuous_effects if ce.target_id != card.id
        ]
        state._invalidate_layered_cache()
    from mtgz_ir.game import restore_mdfc_front

    # A split half / DFC face turns back to the card's front printing
    # in every zone but the stack and battlefield (CR 709.3a / 712.8a).
    restore_mdfc_front(card)
    owner = state.player_by_id(card.owner_id)
    if card.exile_if_put_into_graveyard_from_stack and destination != "hand":
        card.exile_if_put_into_graveyard_from_stack = False
        card.zone = Zone.EXILE
        owner.exile.append(card)
        state.log.append(
            f"{card.printing.name} is exiled instead of going to the graveyard"
        )
        return
    card.exile_if_put_into_graveyard_from_stack = False
    if card.cast_via_flashback:
        card.cast_via_flashback = False
        card.zone = Zone.EXILE
        owner.exile.append(card)
        state.log.append(
            f"{card.printing.name} is exiled (flashback)"
        )
        return
    if destination == "hand":
        from mtgz_ir.game import reveal_to_all

        card.zone = Zone.HAND
        owner.hand.append(card)
        reveal_to_all(state, card)
        state.log.append(
            f"{card.printing.name} returns to P{card.owner_id}'s hand"
        )
        return
    if destination == "exile":
        # "If that spell is countered this way, exile it instead ..."
        card.zone = Zone.EXILE
        owner.exile.append(card)
        state.log.append(f"{card.printing.name} is exiled instead")
        return
    if destination == "library_top":
        # Memory Lapse: "... put it on top of its owner's library".
        from mtgz_ir.game import reveal_to_all

        card.zone = Zone.LIBRARY
        owner.library.insert(0, card)
        from mtgz_ir.turn import note_cards_put_into_library

        note_cards_put_into_library(state, owner.id)
        reveal_to_all(state, card)
        state.log.append(
            f"{card.printing.name} is put on top of P{card.owner_id}'s library"
        )
        return
    if destination != "graveyard":
        raise ValueError(f"unknown spell destination: {destination!r}")
    _send_to_graveyard_or_library(state, card)


def drain_stack(state: GameState) -> None:
    """Resolve every StackItem until empty, interleaving SBAs.

    Engine-level utility used by tests that fire triggers via direct calls
    (``declare_attackers``, ``declare_blockers``, ``put_into_battlefield``,
    etc.) and then need to observe the trigger's *resolved* effects. The
    real engine drains the stack via the priority loop; tests that bypass
    priority must drain explicitly.
    """
    from mtgz_ir.turn import state_based_actions
    while state.stack:
        resolve_top_of_stack(state)
        state_based_actions(state)


def resolve_top_of_stack(state: GameState) -> None:
    """Resolve the top stack item, then report any creature-token
    creation it performed (see ``_flush_creature_token_creations``).

    ``state.resolving_item_controller_id`` names the controller of the
    resolving spell / ability for the duration (restored afterwards, so a
    nested resolution unwinds correctly) -- read by "spells and abilities
    your opponents control can't cause you to ..." protections
    (``mtgz_ir.player_protections``)."""
    prior = state.resolving_item_controller_id
    state.resolving_item_controller_id = (
        state.stack[-1].controller_id if state.stack else None
    )
    try:
        _resolve_top_of_stack_item(state)
    finally:
        state.resolving_item_controller_id = prior
    _flush_creature_token_creations(state)


def _flush_creature_token_creations(state: GameState) -> None:
    """Dispatch ``Event.CREATURE_TOKENS_CREATED`` once per player who
    created one or more creature tokens during the resolution that just
    finished ("Whenever you create one or more creature tokens" —
    Staff of the Storyteller). ``put_into_battlefield`` latches
    ``Player.pending_creature_tokens_created`` for every creature token
    entering; this flush batches them per resolution."""
    for p in state.players:
        if not p.pending_creature_tokens_created:
            continue
        p.pending_creature_tokens_created = False
        from mtgz_ir.effects.predicates import Event as _Event
        dispatch_event(
            state, _Event.CREATURE_TOKENS_CREATED,
            label="creature-tokens-created", player=p,
        )


def _resolve_top_of_stack_item(state: GameState) -> None:
    item = state.stack.pop()

    # Countered spells: no effects, no ETBs; the card goes to its owner's
    # graveyard (permanents and non-permanents alike) per rule 701.5a.
    if item.countered:
        state.log.append(f"{item.source.printing.name} is countered")
        if item.ability is not None:
            # A countered activated / triggered ability is simply removed
            # from the stack (CR 701.5a); its source stays put.
            return
        move_spell_off_stack(
            state, item.source, destination=item.countered_destination,
        )
        return

    # Triggered- or activated-ability resolution. StackItem.ability holds
    # either a TriggeredAbility or an ActivatedAbility — distinguish by
    # type so the log accurately names which kind resolved.
    if item.ability is not None:
        from mtgz_ir.effects import ActivatedAbility

        kind = (
            "activated"
            if isinstance(item.ability, ActivatedAbility)
            else "triggered"
        )
        # CR 608.2b: every target became illegal (hexproof / shroud /
        # protection gained in response) -> the ability doesn't resolve
        # -- unless it says it still does (Gilded Drake; roto P4).
        if targets_all_illegal_on_resolution(
            state, item.targets, item.source, item.controller_id,
        ) and not getattr(
            item.ability.effect, "still_resolves_if_targets_illegal", False,
        ):
            state.log.append(
                f"{item.source.printing.name}'s {kind} ability fizzles "
                f"(all targets illegal)"
            )
            return
        state.log.append(
            f"{item.source.printing.name}'s {kind} ability resolves"
        )
        state.resolving_source = item.source
        state.resolving_x_value = item.x_value
        state.resolving_kicked = item.kicked
        state.resolving_additional_cost_choice_id = (
            item.additional_cost_choice_id
        )
        state.resolving_combat_damage_amount = item.combat_damage_amount
        state.resolving_combat_damage_source_controller_id = (
            item.combat_damage_source_controller_id
        )
        state.resolving_counter_amount = item.counter_amount
        state.resolving_event_subject_id = item.event_subject_id
        state.resolving_modes = item.chosen_modes
        state.resolving_cost_paid_objects = item.cost_paid_objects
        state.last_exile_recall_count = 0
        try:
            item.ability.effect.resolve(state, item.controller_id, item.targets)
        finally:
            state.resolving_source = None
            state.resolving_x_value = 0
            state.resolving_kicked = False
            state.resolving_additional_cost_choice_id = None
            state.resolving_combat_damage_amount = 0
            state.resolving_combat_damage_source_controller_id = None
            state.resolving_counter_amount = 0
            state.resolving_event_subject_id = -1
            state.resolving_modes = ()
            state.resolving_cost_paid_objects = ()
            state.last_exile_recall_count = 0
        return

    # Spell resolution
    card = item.source
    # CR 608.2b: a spell whose every target became illegal (hexproof /
    # shroud / protection gained in response) doesn't resolve and goes
    # to its owner's graveyard.
    if targets_all_illegal_on_resolution(
        state, item.targets, card, item.controller_id,
    ):
        state.log.append(
            f"{card.printing.name} fizzles (all targets illegal)"
        )
        _send_to_graveyard_or_library(state, card)
        return
    state.resolving_x_value = item.x_value
    state.resolving_kicked = item.kicked
    state.resolving_alt_cost_paid = item.alt_cost_paid
    state.resolving_additional_cost_choice_id = item.additional_cost_choice_id
    state.resolving_modes = item.chosen_modes
    state.resolving_cost_paid_objects = item.cost_paid_objects
    state.resolving_spell = card
    try:
        if card.printing.is_permanent and not (
            card.printing.is_instant or card.printing.is_sorcery
        ):
            # Aura: check the cast-time target is still a legal creature/
            # artifact depending on aura_enchant_type. If not, the Aura
            # fizzles to its owner's graveyard without entering.
            if (
                card.printing.is_aura
                and card.printing.aura_enchant_type
                == "creature_card_in_graveyard"
            ):
                # Animate Dead: enters enchanting the targeted creature
                # card in a graveyard (its enters trigger returns it).
                from mtgz_ir.effects.generic.enchantment_mechanics import (
                    graveyard_creature_card_targets,
                )
                if not item.targets or item.targets[0] not in (
                    graveyard_creature_card_targets(state)
                ):
                    state.log.append(
                        f"{card.printing.name} fizzles (no legal enchant "
                        f"target)"
                    )
                    _send_to_graveyard_or_library(state, card)
                    return
                card.attached_to = item.targets[0][1]
                state.log.append(
                    f"{card.printing.name} enters enchanting a card in a "
                    f"graveyard"
                )
            elif card.printing.is_aura:
                enchant_type = plain(card.printing.aura_enchant_type)
                target_obj = None
                if item.targets:
                    kind, target_id = item.targets[0]
                    if kind == enchant_type:
                        if enchant_type in (
                            "creature_or_spacecraft", "artifact_or_creature",
                        ):
                            target_obj = next(
                                (o for o in state.battlefield
                                 if o.id == target_id
                                 and _aura_compound_kind_matches(
                                     state, enchant_type, o,
                                 )),
                                None,
                            )
                        elif enchant_type == "artifact":
                            target_obj = next(
                                (o for o in state.battlefield
                                 if o.id == target_id
                                 and is_artifact_now(state, o)),
                                None,
                            )
                        elif enchant_type == "land":
                            from mtgz_ir.effects.generic.roto_mana import (
                                enchantable_land,
                            )

                            target_obj = next(
                                (o for o in state.battlefield
                                 if o.id == target_id
                                 and enchantable_land(
                                     state, card.printing, o,
                                 )),
                                None,
                            )
                        else:
                            target_obj = next(
                                (o for o in state.battlefield
                                 if o.id == target_id
                                 and o.printing.is_creature),
                                None,
                            )
                if target_obj is None:
                    state.log.append(
                        f"{card.printing.name} fizzles (no legal enchant target)"
                    )
                    _send_to_graveyard_or_library(state, card)
                    return
                card.attached_to = target_obj.id
                state.log.append(
                    f"{card.printing.name} enters attached to "
                    f"{target_obj.printing.name}"
                )
                # Static "you control enchanted permanent" applies
                # immediately when the aura enters — BEFORE any ETB
                # triggers go on the stack. Static abilities are
                # continuous, not stack-based, so the swap kicks in
                # the moment the aura is attached. Tractor Beam.
                apply_aura_static_grants_on_attach(
                    state, card, target_obj, item.controller_id,
                )
            elif card.bestowed:
                # Bestow (CR 702.103e): attach if the target is still a
                # legal creature; otherwise it resolves as a creature.
                from mtgz_ir.game import is_creature_now as _icn

                host = None
                if item.targets:
                    host = next(
                        (o for o in state.battlefield
                         if o.id == item.targets[0][1] and _icn(o)),
                        None,
                    )
                if host is not None:
                    card.attached_to = host.id
                    state.log.append(
                        f"{card.printing.name} enters bestowed on "
                        f"{host.printing.name}"
                    )
                else:
                    card.bestowed = False
                    state.log.append(
                        f"{card.printing.name} resolves as a creature "
                        f"(bestow target gone)"
                    )
            else:
                state.log.append(
                    f"{card.printing.name} resolves onto the battlefield"
                )
                if card.printing.enters_as_copy_of_creature:
                    apply_clone_enters_as_copy_replacement(
                        state, card, item.controller_id,
                    )
                elif card.printing.enters_as_copy_types:
                    from mtgz_ir.creature_casting_ub import (
                        apply_copy_on_entry,
                    )

                    apply_copy_on_entry(state, card, item.controller_id)
                # DSK Rooms: the half cast determines which half is
                # unlocked when the Room enters. Set it BEFORE
                # ``put_into_battlefield`` so any "When you unlock
                # this door" trigger (modeled as ENTERS w/
                # room_half=A) sees the correct unlocked-set.
                if card.printing.is_room and item.cast_room_half is not None:
                    card.unlocked_halves = frozenset({item.cast_room_half})
                    state.log.append(
                        f"{card.printing.name}: unlocked door "
                        f"{item.cast_room_half}"
                    )
            # Dash / escape provenance (cube creatures bucket) for the
            # ETB riders in ``put_into_battlefield``.
            card.dashed = item.alt_cost == "dash"
            card.escaped = item.alt_cost == "escape"
            card.entered_via_cast = not card.is_spell_copy
            card.kick_count = item.kick_count
            put_into_battlefield(state, card, item.controller_id)
            if card.cast_without_sorcery_timing:
                # Necromancy: cast when a sorcery couldn't have been ->
                # its controller sacrifices it at the next cleanup step.
                card.cast_without_sorcery_timing = False
                from mtgz_ir.effects.generic.enchantment_mechanics import (
                    schedule_sacrifice_at_next_cleanup,
                )
                schedule_sacrifice_at_next_cleanup(state, card)
            return
        if card.printing.is_instant or card.printing.is_sorcery:
            state.log.append(f"{card.printing.name} resolves")
            zone_before = card.zone
            if card.printing.spell_effect is not None:
                state.resolving_spell = card
                state.resolving_division = item.division
                try:
                    card.printing.spell_effect.resolve(
                        state, item.controller_id, item.targets
                    )
                    # Spliced effects follow the spell's own (CR 702.47b).
                    state.resolving_division = ()
                    for spliced_printing, spliced_targets in item.spliced:
                        spliced_printing.spell_effect.resolve(
                            state, item.controller_id, spliced_targets,
                        )
                finally:
                    state.resolving_spell = None
                    state.resolving_division = ()
            if card.zone != zone_before:
                # The spell's own effect already moved it (Time
                # Spiral's "Exile Time Spiral.").
                return
            from mtgz_ir.layouts import exile_on_adventure, is_adventure_cast

            if is_adventure_cast(card) and not card.is_spell_copy:
                # CR 715.4: an Adventure is exiled as it resolves.
                exile_on_adventure(state, card, item.controller_id)
                return
            if (
                card.printing.rebound and card.cast_via_hand
                and not card.is_spell_copy
            ):
                # Roto P4: Rebound (CR 702.88a) exiles it as it resolves.
                from mtgz_ir.effects.generic.roto_walkers import (
                    exile_with_rebound,
                )

                exile_with_rebound(state, card, item.controller_id)
                return
            # Non-permanent spells go to their owner's graveyard —
            # unless a zone-change replacement routes them elsewhere
            # (Nexus of Fate shuffles back in),
            # buyback returns them to hand, or flashback exiles them.
            move_spell_off_stack(
                state, card,
                destination="hand" if item.buyback_paid else "graveyard",
            )
            return
        raise ValueError(f"don't know how to resolve {card.printing.name}")
    finally:
        state.resolving_spell = None
        state.resolving_x_value = 0
        state.resolving_kicked = False
        state.resolving_alt_cost_paid = False
        state.resolving_additional_cost_choice_id = None
        state.resolving_modes = ()
        state.resolving_cost_paid_objects = ()


def apply_aura_static_grants_on_attach(
    state: GameState, aura: "GameObject", target: "GameObject",
    controller_id: int,
) -> None:
    """Apply an aura's continuous-while-attached statics to its
    target the moment the aura enters attached. Static abilities are
    continuous (not stack-based), so they take effect immediately —
    before any ETB triggers go on the stack.

    Models the following printing flags by writing matching
    registry entries with ``expires_at=None`` (static-while-attached;
    ``move_off_battlefield`` drops them via
    ``remove_continuous_effects_from_source(only_static=True)``):

    - ``attached_grants_control`` → layer 2 ``"control_change"``
      (Tractor Beam). Most-recent timestamp wins; baseline captured
      on the first grant for revert order.
    - ``attached_sets_base_pt`` → layer 7 sublayer 1 ``"set_base_pt"``
      (Lignify). Overrides base P/T while attached.
    - ``attached_grants_lose_all_abilities`` → layer 6
      ``"lose_all_abilities"`` (Lignify). Suppresses every printed
      and granted ability/keyword/trigger.
    - ``attached_grants_subtype`` → layer 4 ``"add_subtype"``
      (Lignify granting "Treefolk").

    Each is independent — a printing may set any combination.
    """
    from mtgz_ir.game import effective_controller

    if aura.printing.attached_grants_control:
        if effective_controller(state, target) != controller_id:
            # First control-grant on this target: capture the baseline.
            has_prior_grant = any(
                ce.layer == 2
                and ce.kind == "control_change"
                and ce.target_id == target.id
                for ce in state.continuous_effects
            )
            if not has_prior_grant:
                target.controller_id_before_any_aura_swap = target.controller_id
            state.add_continuous_effect(
                source_id=aura.id,
                layer=2,
                kind="control_change",
                target_id=target.id,
                params=(controller_id,),
                description=(
                    f"{aura.printing.name} grants P{controller_id} "
                    f"control of {target.printing.name}"
                ),
            )
            target.controller_id = effective_controller(state, target)
            state.log.append(
                f"P{controller_id} gains control of "
                f"{target.printing.name} via {aura.printing.name}"
            )

    if aura.printing.attached_sets_base_pt is not None:
        power, toughness = aura.printing.attached_sets_base_pt
        state.add_continuous_effect(
            source_id=aura.id,
            layer=7,
            sublayer=1,
            kind="set_base_pt",
            target_id=target.id,
            params=(power, toughness),
            description=(
                f"{aura.printing.name} sets {target.printing.name} "
                f"base P/T to {power}/{toughness}"
            ),
        )

    if aura.printing.attached_grants_lose_all_abilities:
        state.add_continuous_effect(
            source_id=aura.id,
            layer=6,
            kind="lose_all_abilities",
            target_id=target.id,
            params=(),
            description=(
                f"{aura.printing.name}: {target.printing.name} loses "
                f"all abilities"
            ),
        )

    for ability in aura.printing.attached_grants_activated_abilities:
        state.add_continuous_effect(
            source_id=aura.id,
            layer=6,
            kind="grant_activated_ability",
            target_id=target.id,
            params=(ability,),
            description=(
                f"{aura.printing.name}: {target.printing.name} has "
                f"an activated ability"
            ),
        )

    if aura.printing.attached_grants_subtype is not None:
        sub = aura.printing.attached_grants_subtype
        state.add_continuous_effect(
            source_id=aura.id,
            layer=4,
            kind="add_subtype",
            target_id=target.id,
            params=(sub,),
            description=(
                f"{aura.printing.name}: {target.printing.name} is "
                f"a {sub}"
            ),
        )

    for card_type in aura.printing.attached_grants_card_types:
        # Layer 4 ``add_type`` (Unable to Scream's "is a ... artifact
        # creature ... in addition to its other types"): read by
        # ``game.effective_card_types``; a Creature entry also sets the
        # ``permanent_is_creature`` cache (rebuilt by
        # ``game.sync_creature_type_cache`` when the Aura leaves).
        state.add_continuous_effect(
            source_id=aura.id,
            layer=4,
            kind="add_type",
            target_id=target.id,
            params=(card_type.lower(),),
            description=(
                f"{aura.printing.name}: {target.printing.name} is "
                f"a {card_type.lower()}"
            ),
        )
        if card_type == "Creature":
            target.permanent_is_creature = True


def apply_clone_enters_as_copy_replacement(
    state: GameState, clone: "GameObject", controller_id: int,
) -> None:
    """Clone-style ETB replacement: 'this creature may enter as a
    copy of any creature on the battlefield.' Called during spell
    resolution AFTER the resolves-onto-battlefield log line and
    BEFORE ``put_into_battlefield``, so the clone is still off-
    battlefield when the controller picks a target.

    Asks the controller's agent to choose a creature on the
    battlefield. If chosen, snapshots ``original_printing`` (so the
    clone remembers what it was) and replaces ``printing`` with the
    target's printing (CR 706.2 copiable values — auras, counters,
    animations, and other continuous effects on the target are NOT
    copied since they don't live on the printing). Writes a layer-1
    ``"is_copy"`` registry entry sourced by the clone with
    ``expires_at=None``; ``move_off_battlefield(only_static=True)``
    drops the entry when the clone leaves play (the entry is
    informational since the printing mutation is the source of
    truth for cached reads).

    If no creature is on the battlefield or the agent declines, the
    clone enters as its printed self (no mutation, no log).
    """
    from mtgz_ir.game import is_creature_now

    if not clone.printing.enters_as_copy_of_creature:
        return
    # ``is_creature_now`` (not just ``printing.is_creature``) so an
    # animated manland is a legal copy target. The clone still copies
    # the manland's printing — which is the LAND printing — so the
    # clone enters as a land. See test_clone_of_animated_manland.
    candidates: list[tuple[str, int]] = [
        ("creature", o.id)
        for o in state.battlefield
        if is_creature_now(o)
    ]
    if not candidates:
        return
    p = state.players[controller_id]
    chosen = p.agent.choose_target(state, controller_id, candidates)
    if chosen is None:
        return
    _, target_id = chosen
    target = next(
        (o for o in state.battlefield if o.id == target_id), None,
    )
    if target is None:
        return
    if clone.original_printing is None:
        clone.original_printing = clone.printing
    target_printing = target.printing
    clone.printing = target_printing
    state.add_continuous_effect(
        source_id=clone.id,
        layer=1,
        kind="is_copy",
        target_id=clone.id,
        params=(target_printing,),
        description=(
            f"{clone.original_printing.name} is a copy of "
            f"{target_printing.name}"
        ),
    )
    state.log.append(
        f"{clone.original_printing.name} enters as a copy of "
        f"{target_printing.name}"
    )


def put_into_battlefield(
    state: GameState, obj: "GameObject", controller_id: int
) -> None:
    """Move ``obj`` onto the battlefield and fire any ETB triggers.

    Shared by spell resolution and token-creation effects so both flows go
    through the same state transition and trigger pipeline.

    After the object's own ETB triggers, fires "whenever another artifact
    you control enters" triggers on other permanents the controller
    controls if the entering object is an artifact.
    """
    # Containment Priest (cube W/G bucket): "If a nontoken creature
    # would enter and it wasn't cast, exile it instead."
    from mtgz_ir.creature_statics_wg import exile_instead_of_entering

    if exile_instead_of_entering(state, obj):
        return
    # Mox Diamond-style self-replacement (CR 614.1c): "If this would
    # enter, you may discard a land card instead. If you do, put it onto
    # the battlefield. If you don't, put it into its owner's graveyard."
    if obj.printing.enters_only_by_discarding_land:
        if not _discard_land_to_enter(state, obj, controller_id):
            return
    obj.entered_from_zone = obj.zone.value if obj.zone is not None else ""
    obj.zone = Zone.BATTLEFIELD
    obj.controller_id = controller_id
    # A permanent entering the battlefield is a new object (CR 400.7):
    # it carries no named counters except those it enters with, and no
    # harnessed designation.
    obj.named_counters = ()
    obj.harnessed = False
    if obj.printing.enters_with_named_counters:
        from mtgz_ir.game import add_counters
        for kind, amount in obj.printing.enters_with_named_counters:
            add_counters(state, obj, kind, amount)
    if obj.printing.enters_with_plus_one_counters_per is not None:
        from mtgz_ir.game import add_counters
        per = obj.printing.enters_with_plus_one_counters_per
        prior = getattr(state, "resolving_controller_id", None)
        prior_src = state.resolving_source
        try:
            state.resolving_controller_id = controller_id
            state.resolving_source = obj
            n = sum(
                1 for o in state.battlefield
                if o is not obj and per.test(o, state)
            )
        finally:
            state.resolving_controller_id = prior
            state.resolving_source = prior_src
        if n:
            add_counters(state, obj, "+1/+1", n)
    # Sunburst (CR 702.44a): a +1/+1 counter per color of mana spent to
    # cast it if it's a creature, otherwise a charge counter. Zero when
    # it wasn't cast (``sunburst_colors_spent`` is only set by the cast).
    if obj.printing.sunburst:
        from mtgz_ir.game import add_counters
        n = obj.sunburst_colors_spent
        obj.sunburst_colors_spent = 0
        add_counters(
            state, obj, "+1/+1" if obj.printing.is_creature else "charge", n,
        )
    # ``enters_tapped`` is the printing-level flag flipped by the
    # land-builder (Planets, utility lands), the parser's new
    # creature-side ``This creature enters tapped.`` recognizer
    # (Piranha Fly), and the equivalent recognizers on other card
    # types. The land path (``play_land``) reads the same field at its
    # own ETB call site; mirroring the read here keeps both paths
    # honoring the printed replacement effect uniformly. Defaults to
    # ``False`` for every existing creature/spell that hasn't opted
    # in, so the change is a no-op for the pre-existing pool.
    obj.tapped = bool(obj.printing.enters_tapped)
    if not obj.tapped and state.battlefield:
        # "<Types> your opponents control enter tapped." (Spider-Woman).
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            enters_tapped_by_opponent_static,
        )

        obj.tapped = enters_tapped_by_opponent_static(state, obj, controller_id)
    if obj.printing.is_land:
        # "Nonbasic lands your opponents control enter tapped." (Archon
        # of Emeria; ``mtgz_ir.creature_statics_wg``).
        from mtgz_ir.creature_statics_wg import forced_to_enter_tapped

        if forced_to_enter_tapped(state, obj, controller_id):
            obj.tapped = True
    # Slice 14 (TTT Pin 2): summoning sickness is a creature-only
    # property per CR 302.1 + 612.6 — non-creature permanents
    # (Equipment, Aura, Enchantment, Artifact, planeswalker) do not
    # have summoning sickness. The pre-Slice-14 unconditional stamp
    # diverged from Rust's `move_source_post_resolve` (state_ops_
    # resolve.rs:335), which correctly gates the stamp on `&&
    # creature`. Aligning Python to the CR-canonical behavior.
    # Both branches are explicit because the GameObject dataclass
    # default is `summoning_sick: bool = True` — relying on that
    # default for non-creatures would preserve the divergence.
    # Vehicles are noncreature permanents that can become creatures;
    # CR 302.6 applies to them once crewed, so they enter "sick" too
    # (only read while the Vehicle is a creature: attacking and {T}
    # costs check ``is_creature_now`` first).
    if obj.printing.is_creature or "Vehicle" in obj.printing.subtypes:
        obj.summoning_sick = True
    else:
        obj.summoning_sick = False
    # Impending: a spell cast for its impending cost enters with N
    # time counters and is suppressed-as-non-creature until the
    # last counter is removed. The cast site set
    # ``cast_via_impending`` on the GameObject; consume the flag
    # here (so a future blink-and-recast under the printed cost
    # doesn't reactivate the suppression) and initialize the
    # impending state. The end-step turn-based action
    # ``run_impending_decrement_turn_based_action`` decrements
    # ``time_counters`` and clears ``impending_active`` when the
    # last counter is removed.
    if obj.cast_via_impending and obj.printing.impending_count > 0:
        obj.time_counters = obj.printing.impending_count
        obj.impending_active = True
    obj.cast_via_impending = False
    # Cast-from-hand-only ETB replacement: enters with N -1/-1 counters
    # if cast from hand (Patched Plaything's "if you cast it from your
    # hand" rider). Read from the GameObject's ``cast_via_hand`` flag
    # which is set in ``_cast_spell``'s cast-from-hand branch and
    # cleared by every other cast path. Applied BEFORE the +1/+1
    # counter ETB replacements below so the SBA-driven pairwise
    # cancellation has both kinds of counters in place at the next
    # SBA pass — same ordering rule as printed multi-counter ETB
    # replacements. Always cleared after read so a future blink doesn't
    # re-apply the counters under the printed cost.
    if (
        obj.cast_via_hand
        and obj.printing.enters_with_minus_one_counters_from_hand_cast > 0
    ):
        from mtgz_ir.game import add_minus_one_counters
        add_minus_one_counters(
            state, obj,
            obj.printing.enters_with_minus_one_counters_from_hand_cast,
        )
    # Persistent provenance for "if you cast it from your hand" on its
    # enters trigger (Amped Raptor; Vintage Roto P3).
    obj.entered_from_hand_cast = (
        obj.cast_via_hand and obj.entered_from_zone == Zone.STACK.value
    )
    obj.cast_via_hand = False
    if obj.printing.zone_abilities:
        # Roto P2 entry hook: exile-instead sources apply only from now
        # on; "enters with a counter for each ..." replacements.
        from mtgz_ir.effects.generic.roto_zones import on_enter_battlefield

        on_enter_battlefield(state, obj)
    # A "void" graveyard replacement source (Dauthi Voidwalker) applies
    # only to cards put into its opponents' graveyards from now on.
    if obj.printing.void_exile_replacement:
        for pl in state.players:
            if pl.id != controller_id:
                pl.void_graveyard_snapshot = frozenset(
                    c.id for c in pl.graveyard
                )
    # Squad / Offspring payments belong to the spell that resolved into
    # this permanent; any other entry is a new object that paid nothing.
    if state.resolving_spell is not obj:
        obj.squad_paid_count = 0
        obj.offspring_paid = False
        obj.entered_x_value = 0
    else:
        obj.entered_x_value = state.resolving_x_value
    obj.saddled = False   # a new object is never saddled
    # Death-tracker-gated ETB replacement: enters with one +1/+1
    # counter on it if a creature died this turn (Cackling Slasher).
    # Read from the global ``state.creatures_died_this_turn`` tracker
    # which the ``move_off_battlefield`` site increments on every
    # creature death and the cleanup step resets. Predates the
    # ``add_plus_one_counters`` site below so the Loading-Zone-style
    # doubler interacts with this counter the same way it does with
    # ``enters_with_counters_from_x``.
    if (
        obj.printing.is_creature
        and obj.printing
        .enters_with_plus_one_counter_if_creature_died_this_turn
        and state.creatures_died_this_turn > 0
    ):
        from mtgz_ir.game import add_plus_one_counters
        add_plus_one_counters(state, obj, 1)
    # Snapshot the moment of ETB for static-effect timestamp use
    # (Magus / Dryad layer-4 synthesizing in effective_subtypes,
    # Painter's Servant layer-5, Humility layer-6/7b). Bump
    # ``next_timestamp`` past this snapshot so subsequent registry
    # writes (including the synthesizing-side's expectation of a
    # higher timestamp) are strictly later in the layer walks.
    obj.etb_timestamp = state.next_timestamp
    state.next_timestamp += 1
    # Painter's Servant: "As Painter's Servant enters, choose a
    # color." The chosen color is captured on the GameObject and
    # consumed by ``effective_colors`` when synthesizing the
    # layer-5 ``add_colors`` entry sourced by this permanent.
    if obj.printing.paints_all_objects_chosen_color_at_etb:
        chosen = state.players[controller_id].agent.choose_mana_color(
            state, controller_id, ["W", "U", "B", "R", "G"],
        )
        if chosen in ("W", "U", "B", "R", "G"):
            obj.painter_chosen_color = chosen
            state.log.append(
                f"{obj.printing.name} enters; chose color {chosen}"
            )
    # "As this creature enters, choose a player." (True-Name Nemesis).
    if obj.printing.chooses_card_name_as_enters:
        # Anointed Peacekeeper (cube W/G bucket).
        from mtgz_ir.creature_statics_wg import choose_card_name_as_enters

        choose_card_name_as_enters(state, obj, controller_id)
    if obj.printing.roto_statics:
        # "As this enters, choose a card name." (roto P1: Pithing
        # Needle, Phyrexian Revoker, Disruptor Flute, Meddling Mage).
        from mtgz_ir.roto_restrictions import (
            choose_card_name_as_enters as _roto_choose_name,
        )

        _roto_choose_name(state, obj, controller_id)
    obj.chosen_parity = None
    obj.modes_chosen = ()
    if obj.printing.chooses_parity_as_enters:
        agent = state.player_by_id(controller_id).agent
        idx = agent.choose_mode(state, controller_id, ["odd", "even"]) if agent else 0
        obj.chosen_parity = "even" if idx == 1 else "odd"
        state.log.append(f"{obj.printing.name} enters; chose {obj.chosen_parity}")
    if obj.printing.chooses_player_as_enters:
        options = [("player", pl.id) for pl in state.players if not pl.has_lost]
        agent = state.player_by_id(controller_id).agent
        pick = agent.choose_target(state, controller_id, options) if agent else None
        if pick not in options:
            # Default: an opponent (the only sensible choice for the
            # protection/punisher cards that ask).
            pick = next(
                (o for o in options if o[1] != controller_id), options[0],
            )
        obj.chosen_player_id = pick[1]
        state.log.append(f"{obj.printing.name} enters; chose P{pick[1]}")
    if obj.printing.enters_choosing_color:
        # "As this <permanent> enters, choose a color." (Vintage Roto P3).
        from mtgz_ir.effects.generic.roto_mana import choose_color_as_enters

        choose_color_as_enters(state, obj, controller_id)
    # (Kavu observer trigger fires below, after own ETB.)
    # "Enters with +1/+1 counters equal to X" replacement effects. Applied
    # before triggered abilities so any ETB that reads the creature's
    # state sees the counters already in place.
    # Murktide Regent's two abilities are card-IR data only (the mtg-zero
    # Rust engine plays them; SEARCH_LIMITATIONS, "Legacy pool" 1): fail
    # loudly rather than enter without its counters or its trigger.
    if obj.printing.enters_with_counter_per_card_exiled_with_it is not None or any(
        getattr(t, "event", None) is not None and t.event.value == "leaves_graveyard"
        for t in obj.printing.triggered_abilities
    ):
        raise NotImplementedError(
            f"{obj.printing.name}: 'enters with a counter for each card exiled with it' "
            "and 'leaves your graveyard' triggers are card-IR data only (not played here)"
        )
    if obj.printing.enters_with_counter_per_kick and obj.kick_count > 0:
        from mtgz_ir.game import add_plus_one_counters
        add_plus_one_counters(state, obj, obj.kick_count)
    mode = obj.printing.enters_with_counters_from_x
    if mode == "x":
        from mtgz_ir.game import add_plus_one_counters
        add_plus_one_counters(state, obj, state.resolving_x_value)
    elif mode == "x_charge":
        # "This artifact enters with X charge counters on it." (Chalice
        # of the Void; X is 0 unless it was cast).
        from mtgz_ir.game import add_counters
        add_counters(state, obj, "charge", obj.entered_x_value)
    elif mode == "mana_spent":
        from mtgz_ir.game import add_plus_one_counters
        mc = obj.printing.mana_cost
        add_plus_one_counters(
            state, obj,
            state.resolving_x_value
            + mc.generic
            + sum(n for _, n in mc.colored),
        )
    # External anthems that grant extra ETB counters to creatures:
    # "Each creature you control enters with an additional +1/+1 counter
    # on it for each land that entered the battlefield under your control
    # this turn" (Bioengineered Future). Applied per source on the
    # controller's battlefield — multiple sources stack.
    if obj.printing.is_creature:
        controller = state.player_by_id(controller_id)
        anthem_sources = sum(
            1 for o in state.battlefield
            if o.controller_id == controller_id
            and getattr(
                o.printing,
                "grants_etb_counter_per_land_entered_this_turn",
                False,
            )
        )
        if anthem_sources and controller.lands_entered_this_turn:
            from mtgz_ir.game import add_plus_one_counters
            add_plus_one_counters(
                state, obj,
                anthem_sources * controller.lands_entered_this_turn,
            )
    # Planeswalker: initialize loyalty counters to the printed starting
    # loyalty. Only applied the first time the card enters the
    # battlefield (loyalty_counters starts at 0).
    if obj.printing.is_planeswalker and obj.printing.loyalty is not None:
        obj.loyalty_counters = obj.printing.loyalty
        if obj.phyrexian_life_pips:
            # Roto P4: Compleated enters with fewer loyalty counters.
            from mtgz_ir.effects.generic.roto_walkers import apply_compleated

            apply_compleated(state, obj)
    # "As long as ~ entered this turn" stamp (The Wandering Emperor).
    obj.entered_battlefield_turn = state.turn_number
    if obj.printing.creature_during_your_turn:
        # Kaito, Bane of Nightmares (cube glue bucket).
        from mtgz_ir.cube_glue_statics import on_enters_battlefield

        on_enters_battlefield(state, obj)
    # Devour land N: as ``obj`` enters, agent may sacrifice any number
    # of lands; obj enters with N counters per land sacrificed.
    # Simplification: the random baseline doesn't ramp Famished's own
    # counters — see docstring on CardPrinting.devour_land_multiplier.
    if obj.printing.devour_land_multiplier > 0:
        _apply_devour(state, obj, controller_id)
    if obj.printing.daybound or obj.printing.nightbound:
        # Daybound (CR 702.145b): at night it enters transformed.
        from mtgz_ir.layouts import day_night_enters

        day_night_enters(state, obj)
    state.battlefield.append(obj)
    if obj.printing.enters_prepared:
        # "This creature enters prepared."
        from mtgz_ir.layouts import become_prepared

        become_prepared(state, obj)
    if (obj.printing.daybound or obj.printing.nightbound) and not state.day_night:
        # CR 702.145c/f: controlling a daybound (nightbound) permanent
        # while it's neither day nor night makes it day (night).
        from mtgz_ir.layouts import set_day_night

        set_day_night(state, "day" if obj.printing.daybound else "night")
    # Track land ETBs for "lands entered under your control this turn"
    # (Bioengineered Future). Distinct from lands_played_this_turn — this
    # also covers basics fetched via Lander tokens or other search-and-
    # put-onto-battlefield effects that don't go through _play_land.
    if obj.printing.is_land:
        state.player_by_id(controller_id).lands_entered_this_turn += 1
    if obj.printing.is_token and obj.printing.is_creature:
        state.player_by_id(controller_id).pending_creature_tokens_created = True
    if is_artifact_now(state, obj):
        # Track per-player "an artifact entered under your control
        # this turn" for static-ability gates like Mechan Shieldmate's
        # conditional attack-bypass for defenders. Counts artifact
        # creatures and artifact tokens too. Reset at cleanup.
        state.player_by_id(
            controller_id
        ).artifacts_entered_under_your_control_this_turn += 1
    if obj.dashed:
        # Dash (CR 702.109a): haste while on the battlefield + a delayed
        # return-to-hand at the beginning of the next end step.
        from mtgz_ir.effects.generic.cube_creatures_rmc import apply_dash_etb

        apply_dash_etb(state, obj, controller_id)
    _trigger_etb_abilities(state, obj)
    pending_exile = obj.copied_card_to_exile
    if pending_exile is not None:
        obj.copied_card_to_exile = None
        from mtgz_ir.creature_casting_ub import push_exile_copied_card

        push_exile_copied_card(state, obj, pending_exile, controller_id)


def _discard_land_to_enter(
    state: GameState, obj: "GameObject", controller_id: int,
) -> bool:
    """Resolve Mox Diamond's entry replacement. Returns ``True`` when a
    land card was discarded (the permanent enters), ``False`` when the
    controller declined or had no land card (the card goes to its
    owner's graveyard instead)."""
    from mtgz_ir.turn import fire_opponent_discards_triggers

    p = state.player_by_id(controller_id)
    options = [c.id for c in p.hand if c.printing.is_land]
    picked = None
    if options and p.agent is not None:
        picked = p.agent.choose_card_from_hand(state, controller_id, options)
    if picked in options:
        card = next(c for c in p.hand if c.id == picked)
        p.hand.remove(card)
        card.zone = Zone.GRAVEYARD
        p.graveyard.append(card)
        state.log.append(
            f"P{controller_id} discards {card.printing.name} so "
            f"{obj.printing.name} can enter"
        )
        fire_opponent_discards_triggers(state, controller_id, card=card)
        return True
    obj.zone = Zone.GRAVEYARD
    state.player_by_id(obj.owner_id).graveyard.append(obj)
    state.log.append(
        f"{obj.printing.name} is put into its owner's graveyard "
        f"(no land card discarded)"
    )
    return False


def _apply_devour(
    state: GameState, obj: "GameObject", controller_id: int
) -> None:
    """Devour land N: ask the agent (via ``choose_devour_lands``) for
    the subset of lands to sacrifice; sacrifice those and add
    N * (count sacrificed) +1/+1 counters to ``obj``.
    """
    from mtgz_ir.turn import sacrifice as _sacrifice

    p = state.player_by_id(controller_id)
    if p.agent is None:
        return
    land_ids = [
        o.id for o in state.battlefield
        if o.controller_id == controller_id and o.printing.is_land
    ]
    if not land_ids:
        return
    picks = p.agent.choose_devour_lands(state, controller_id, land_ids)
    valid_picks = [pid for pid in picks if pid in set(land_ids)]
    sacrificed = 0
    for pid in valid_picks:
        victim = next(
            (o for o in state.battlefield if o.id == pid), None
        )
        if victim is None or not victim.printing.is_land:
            continue
        _sacrifice(state, victim)
        sacrificed += 1
    if sacrificed > 0:
        from mtgz_ir.game import add_plus_one_counters
        counters = obj.printing.devour_land_multiplier * sacrificed
        add_plus_one_counters(state, obj, counters)
        state.log.append(
            f"{obj.printing.name} devours {sacrificed} land(s) and "
            f"enters with {counters} +1/+1 counters"
        )


def push_triggered_ability(
    state: GameState,
    *,
    source: "GameObject",
    ability,
    ability_index: int,
    controller_id: int,
    label: str,
    combat_damage_amount: int = 0,
    combat_damage_source_controller_id: int | None = None,
    counter_amount: int = 0,
    fixed_targets: tuple[tuple[str, int], ...] | None = None,
    event_subject_id: int = -1,
) -> bool:
    """Push a triggered ability onto the stack, picking targets if needed.

    Centralizes the trigger-firing path so every triggered ability gets a
    response window instead of resolving immediately at the fire site.

    - ``label``: short descriptor used in the log (e.g. ``"dies"``,
      ``"attacks"``). The log line reads
      ``f"{source.name}'s {label} trigger goes on the stack" + targets``.
    - ``ability_index``: index into ``source.printing.triggered_abilities``;
      used by ``should_fire_trigger`` for the ``once_per_turn`` ledger.
    - ``combat_damage_amount``: stashed on the StackItem for
      combat-damage triggers so the resolving effect can read
      ``state.resolving_combat_damage_amount``.
    - ``fixed_targets``: when the trigger's targets are determined by the
      event itself (e.g. "the player who was hit"), pass them here and
      target selection is skipped. Otherwise the controller's agent picks
      from ``effect.legal_targets`` as the ability goes on the stack.

    Returns True when the ability lands on the stack, False when it's
    gated off (tier/once-per-turn/void) or fizzles (no legal targets).
    """
    from mtgz_ir.turn import should_fire_trigger

    if not should_fire_trigger(state, source.id, ability_index, ability):
        return False
    effect = ability.effect
    if fixed_targets is not None:
        targets: tuple[tuple[str, int], ...] = fixed_targets
    else:
        targets = ()
        min_t, max_t = effect.target_count_range()
        dynamic = getattr(effect, "dynamic_target_count_range", None)
        if dynamic is not None:
            # "each of up to X targets, where X is ..." (Batroc).
            min_t, max_t = dynamic(state, source)
        if max_t > 0:
            state.resolving_source = source
            state.resolving_combat_damage_amount = combat_damage_amount
            try:
                from mtgz_ir.game import drop_protected_players
                legal = drop_protected_players(
                    state, effect.legal_targets(state, controller_id),
                )
            finally:
                state.resolving_source = None
                state.resolving_combat_damage_amount = 0
            legal = filter_targetable(state, legal, source, controller_id)
            if not legal and min_t > 0:
                state.log.append(
                    f"{source.printing.name}'s {label} trigger has no "
                    f"legal targets; fizzles"
                )
                return False
            controller = state.player_by_id(controller_id)
            chosen_list: list[tuple[str, int]] = []
            remaining = list(legal)
            slot_kinds = effect.target_slot_kinds()
            for slot_idx in range(max_t):
                # Per-slot kind enforcement: when the effect declares
                # ``target_slot_kinds`` (e.g., Diplomatic Relations:
                # ``("my_creature", "creature")``), restrict the agent's
                # choices for THIS slot to entries of the declared kind.
                # Empty tuple keeps the legacy flat-pool behavior.
                if slot_kinds and slot_idx < len(slot_kinds):
                    needed = slot_kinds[slot_idx]
                    pool = [t for t in remaining if t[0] == needed]
                else:
                    pool = remaining
                if not pool:
                    break
                if controller.agent is None:
                    # **Tier-C target-seating sub-substrate (Python
                    # mirror of the Rust shared push-time seat).** The
                    # production-deck bench loader seats NO agent
                    # (``controller.agent is None``). Historically this
                    # ``break``s → the trigger fizzles (``count_min>=1``)
                    # or no-ops (``count_min==0``); that fizzle/no-op is
                    # exactly why targeted safe triggers were
                    # ``_SAFE_TRIGGERS``-declined (Rust's targeted arms
                    # would pick a real first-eligible target and apply
                    # the effect → a guaranteed cross-engine
                    # divergence). For a lever-admitted TARGET-SEATED
                    # safe trigger ONLY (the ``_is_target_seated_safe_
                    # trigger`` gate keeps this surgical — every other
                    # no-agent targeted trigger keeps its existing
                    # ``break``/fizzle), deterministically pick
                    # ``pool[0]``. ``pool`` is the un-kind-filtered
                    # ``remaining`` (lever cards have
                    # ``slot_kinds()==()``), whose ``[0]`` is exactly
                    # ``Effect._enumerate_for_filter``'s first entry =
                    # ``legal_targets[0]`` = the SAME object Rust's
                    # ``first_legal_trigger_target`` seats (smallest
                    # bf-index filter-passing object; the cross-engine
                    # harness keeps bf insertion order + ids in
                    # lockstep). This is the "behave as if a
                    # ``_PickFirstLegalTargetAgent`` were seated, WITHOUT
                    # an agent" contract. Documented in
                    # ``docs/SEARCH_LIMITATIONS.md`` §2.3 (the
                    # deterministic-first no-agent target replaces the
                    # CR 603.3d controller choice — same class as every
                    # ``_PickFirst*`` convention the campaign already
                    # documents).
                    from mtgz_ir.parity.trajectory import (
                        _is_target_seated_safe_trigger,
                    )
                    if _is_target_seated_safe_trigger(source, ability):
                        chosen_list.append(pool[0])
                        remaining.remove(pool[0])
                        continue
                    break  # pragma: no cover
                pick = controller.agent.choose_target(
                    state, controller_id, pool
                )
                if pick is None or pick not in pool:
                    break
                chosen_list.append(pick)
                remaining.remove(pick)
            if len(chosen_list) < min_t:  # pragma: no cover
                state.log.append(
                    f"{source.printing.name}'s {label} trigger had no "
                    f"target chosen; fizzles"
                )
                return False
            targets = tuple(chosen_list)
    state.stack.append(
        StackItem(
            source=source,
            controller_id=controller_id,
            ability=ability,
            targets=targets,
            combat_damage_amount=combat_damage_amount,
            combat_damage_source_controller_id=(
                combat_damage_source_controller_id
            ),
            counter_amount=counter_amount,
            event_subject_id=event_subject_id,
        )
    )
    state.log.append(
        f"{source.printing.name}'s {label} trigger goes on the stack"
        + _format_log_targets(state, targets)
    )
    if targets and fixed_targets is None:
        from mtgz_ir.effects.generic.cube_creatures_rmc import (
            fire_becomes_target,
        )

        fire_becomes_target(state, targets, source, controller_id)
    # "Whenever a permanent entering causes a triggered ability to
    # trigger, ..." (roto P1: Strict Proctor).
    if ability.event == "enters":
        from mtgz_ir.roto_restrictions import fire_entering_trigger_watchers

        fire_entering_trigger_watchers(state, len(state.stack) - 1)
    return True


def dispatch_event(
    state: GameState,
    event,
    *,
    label: str | None = None,
    extra_observers: "list | None" = None,
    combat_damage_amount: int = 0,
    combat_damage_source_controller_id: int | None = None,
    counter_amount: int = 0,
    fixed_targets: tuple[tuple[str, int], ...] | None = None,
    **subjects,
) -> None:
    """Centralized predicate-trigger dispatcher.

    Walks every battlefield permanent and fires any predicate-style
    triggered ability whose ``event`` matches ``event`` and whose
    ``predicate`` evaluates true against ``subjects`` and the source.
    String-key (legacy) abilities are NOT fired here — they continue
    flowing through the per-event ``_fire_*`` paths until migration
    is complete. Both styles coexist; ``__post_init__`` on
    ``TriggeredAbility`` enforces that no single ability sets both.

    ``label`` is the human-readable string attached to the resulting
    stack item (defaults to the event's value, e.g. ``"enters"``).
    ``subjects`` is the named subject mapping for the event — see
    ``EVENT_SUBJECTS`` in ``effects/predicates.py``.

    ``extra_observers`` is an additional list of ``GameObject`` whose
    triggered abilities should be checked alongside the battlefield.
    Used by DIES / LEAVES_BATTLEFIELD dispatch to include the dearly-
    departed (already moved off the battlefield by the caller) as a
    candidate observer for its own self-triggers (``IsSelf()`` against
    the dying object). Permanents on the battlefield that ALSO appear
    in ``extra_observers`` are not double-counted.

    ``combat_damage_amount`` threads the damage amount through to each
    pushed StackItem so resolving effects can read
    ``state.resolving_combat_damage_amount``. Used by the DEALT_DAMAGE
    dispatch (Pain for All's echo, Cryoshatter's destroy-on-damage,
    etc.); zero for events that don't carry damage. ``counter_amount``
    is the analogous channel for the COUNTERS_ADDED event
    (Terrasymbiosis's "draw that many cards" rider). ``fixed_targets``
    forwards a pre-determined target tuple to every pushed StackItem
    (used by DISCARDS_CARD to expose the discarding player as
    ``("player", id)`` so Entropic Battlecruiser's "they lose 3 life"
    half can damage them without needing target selection).

    For ``Event.ENTERS`` the per-source firings are multiplied by
    ``etb_trigger_multiplier`` (Starfield Vocalist's "triggers an
    additional time" replacement). The legacy ``_fire_*`` paths apply
    the same multiplier — keeping the dispatcher consistent means a
    landfall trigger migrated to the predicate path keeps doubling
    under Starfield, the same way the ``etb`` self-trigger path does
    in ``_trigger_etb_abilities``.
    """
    from mtgz_ir.effects.predicates import Event as _Event
    from mtgz_ir.game import etb_trigger_multiplier

    if label is None:
        label = event.value
    # Snapshot the battlefield: triggers that ETB new permanents during
    # dispatch must not themselves see those new permanents on the
    # original event's pass.
    permanents = list(state.battlefield)
    # Command-zone emblems (CR 114) are an additional trigger SOURCE:
    # their abilities (e.g. Tezzeret's emblem "at the beginning of
    # combat on your turn, …") fire from the command zone, not the
    # battlefield. Scan them alongside battlefield permanents so the
    # emblem's predicate-style triggered abilities observe the event.
    # Empty for almost every state (no emblems), so this is a cheap
    # no-op on the common path. Mirrors Rust's dispatch_event emblem-
    # source extension.
    if state.emblems:
        permanents.extend(state.emblems)
    if extra_observers:
        seen_ids = {p.id for p in permanents}
        for extra in extra_observers:
            if extra.id not in seen_ids:
                permanents.append(extra)
                seen_ids.add(extra.id)
    from mtgz_ir.game import _source_layer6_wiped
    # Lands under Blood Moon / Magus have no rules-text abilities (CR
    # 305.7; Vintage Roto P5).
    from mtgz_ir.roto_board_statics import lands_without_rules_text

    moon_lands = lands_without_rules_text(state)
    for source in permanents:
        # "Loses all abilities" (layer 6 -- Oko's +1, Lignify-style
        # Auras): the permanent's PRINTED triggered abilities no longer
        # exist, so they observe nothing (granted triggers re-added after
        # the wipe ride ``effective_triggered_abilities`` elsewhere).
        if (
            state.continuous_effects and _source_layer6_wiped(state, source)
        ) or (
            source.left_without_abilities
            and source.zone != Zone.BATTLEFIELD
        ) or source.id in moon_lands:
            continue
        for ability_index, ability in enumerate(
            source.printing.triggered_abilities
        ):
            if ability.functions_in_graveyard:
                continue
            if not ability.matches_event(event, subjects, source, state):
                continue
            # Counter-creature-combat-damage aggregated observer: fired by
            # the dedicated aggregated path in
            # ``combat._fire_combat_damage_to_player_triggers``, not by the
            # per-source dispatch here. Skip it so it doesn't double-fire
            # (once via dispatch_event per connecting source, and again via
            # the aggregated path per (controller, target_player) pair).
            if event is _Event.DEALS_COMBAT_DAMAGE:
                from mtgz_ir.combat import (
                    _is_counter_creature_combat_damage_observer,
                )
                if _is_counter_creature_combat_damage_observer(ability):
                    continue
            # DSK Rooms: an ability tagged with a room_half is only
            # active when that half is in the source's
            # ``unlocked_halves``. Locked halves' triggers never
            # observe events. (Non-Room abilities have ``room_half ==
            # None`` and are never gated.)
            if (
                ability.room_half is not None
                and ability.room_half not in source.unlocked_halves
            ):
                continue
            firings = (
                etb_trigger_multiplier(
                    state, source.controller_id,
                    ability_source=source,
                    entering_obj=subjects.get("obj"),
                )
                if event is _Event.ENTERS else 1
            )
            # Effects whose objects are fixed by the triggering event
            # itself (not targets of the trigger) bind them here --
            # e.g. "gain control of those permanents" for "Whenever you
            # cast a spell that targets one or more permanents".
            ability_fixed_targets = fixed_targets
            bind = getattr(ability.effect, "targets_from_event", None)
            if bind is not None:
                ability_fixed_targets = bind(state, subjects)
            # "that spell" / "that creature" back-references (cube
            # creatures bucket): the effect names which subject it keeps.
            subject_bind = getattr(ability.effect, "event_subject_id", None)
            ability_event_subject_id = (
                subject_bind(subjects) if subject_bind is not None else -1
            )
            for _ in range(firings):
                push_triggered_ability(
                    state,
                    source=source,
                    ability=ability,
                    ability_index=ability_index,
                    controller_id=source.controller_id,
                    label=label,
                    combat_damage_amount=combat_damage_amount,
                    combat_damage_source_controller_id=(
                        combat_damage_source_controller_id
                    ),
                    counter_amount=counter_amount,
                    fixed_targets=ability_fixed_targets,
                    event_subject_id=ability_event_subject_id,
                )
    # Triggered abilities granted by attached Equipment (Kaldra
    # Compleat) belong to the equipped creature: evaluate them with the
    # host as their source.
    for equipment in permanents:
        granted = equipment.printing.equipment_granted_triggers
        if not granted or equipment.attached_to is None:
            continue
        host = next(
            (o for o in state.battlefield if o.id == equipment.attached_to),
            None,
        )
        if host is None:
            continue
        for ability in granted:
            if not ability.matches_event(event, subjects, host, state):
                continue
            push_triggered_ability(
                state,
                source=host,
                ability=ability,
                ability_index=-1,
                controller_id=host.controller_id,
                label=label,
                combat_damage_amount=combat_damage_amount,
                combat_damage_source_controller_id=(
                    combat_damage_source_controller_id
                ),
                counter_amount=counter_amount,
                fixed_targets=fixed_targets,
            )
    # Triggered abilities a board rule grants every creature (The
    # Tabernacle at Pendrell Vale; Vintage Roto P5).
    from mtgz_ir.roto_board_statics import fire_granted_triggers

    fire_granted_triggers(
        state, event, subjects, label, push_triggered_ability, extra_observers,
    )
    _dispatch_graveyard_triggers(state, event, label, subjects)


def _dispatch_graveyard_triggers(
    state: GameState, event, label: str, subjects: dict,
) -> None:
    """Fire ``functions_in_graveyard`` triggered abilities of cards in
    graveyards (Sword of the Meek). The event's ``obj`` subject (the
    entering creature for ENTERS) is handed to the ability as its fixed
    target so "that creature" can address it."""
    for p in state.players:
        for card in list(p.graveyard):
            if not card.printing.has_graveyard_triggers:
                continue
            for ability_index, ability in enumerate(
                card.printing.triggered_abilities
            ):
                if not ability.functions_in_graveyard:
                    continue
                if card not in p.graveyard:
                    continue
                if not ability.matches_event(event, subjects, card, state):
                    continue
                obj = subjects.get("obj")
                fixed = (
                    (("creature", obj.id),)
                    if obj is not None and hasattr(obj, "printing") else None
                )
                push_triggered_ability(
                    state,
                    source=card,
                    ability=ability,
                    ability_index=ability_index,
                    controller_id=card.owner_id,
                    label=label,
                    fixed_targets=fixed,
                )


def _trigger_etb_abilities(state: GameState, creature: "GameObject") -> None:
    """Push every ENTERS-event triggered ability that matches when
    ``creature`` enters the battlefield.

    All ETB-family triggers (this-creature-enters, landfall,
    artifact-enters-you-control, kavu-enters-you-control, etc.) flow
    through ``dispatch_event(state, Event.ENTERS, obj=creature)``. The
    dispatcher walks every battlefield permanent, evaluates each
    ability's predicate against the entering object, and pushes
    matched abilities onto the stack via ``push_triggered_ability``.

    Phase 2 migrated every static card from the legacy string-key
    ``trigger="etb"`` (and the family of ``"landfall"`` /
    ``"artifact_enters_you_control"`` / etc. keys) to the predicate
    shape, so the legacy per-key ``_fire_*`` paths are gone — there
    are no string-key abilities left to fire.
    """
    from mtgz_ir.effects.predicates import Event

    if creature.printing.is_saga:
        # CR 714.3a: a Saga enters with a lore counter (chapter I).
        from mtgz_ir.layouts import saga_enters

        saga_enters(state, creature)
    dispatch_event(state, Event.ENTERS, label="enters", obj=creature)
