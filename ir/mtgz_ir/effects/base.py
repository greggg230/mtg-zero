"""Core effect infrastructure — base Effect class, Target alias,
ability dataclasses (ActivationCost / ActivatedAbility / StaticAbility /
TriggeredAbility), and shared private helpers used across effect modules.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from enum import StrEnum
from typing import TYPE_CHECKING

from mtgz_ir.effects.vocab import CardTypeWord, coerce
from mtgz_ir.keyword_vocab import KeywordSpelling

if TYPE_CHECKING:
    from mtgz_ir.cards import CostReduction, ManaCost
    from mtgz_ir.effects.conditions import Condition
    from mtgz_ir.effects.modifiers import (
        ContinuousModifier, ReplacementEffect,
    )
    from mtgz_ir.effects.targets import TargetSlot
    from mtgz_ir.game import GameState


Target = tuple[str, int]



class Effect:
    """Base class for ability effects.

    Untargeted effects override ``resolve``; targeted effects come in
    two flavours during the PR #3 → PR #4 phased migration:

    1. **New-shape effects** carry a non-empty
       ``targets: tuple[TargetSlot, ...]`` field describing each
       target slot via the
       :mod:`mtgz_ir.effects.targets` grammar (``TargetSpec`` /
       ``SlotRef``). The default ``requires_target()`` /
       ``legal_targets()`` / ``target_count_range()`` implementations
       below consult ``self.targets`` to answer the targeting
       pipeline's questions.
    2. **Old-shape effects** (destroy / exile / tap / pump / +1+1 /
       return-to-hand / …) leave ``targets`` at its empty default
       and override ``requires_target()`` / ``legal_targets()``
       directly. Those overrides keep working unchanged because
       ``targets`` is empty — the default impls below short-circuit
       to "no target" and the per-class overrides take precedence.
       PR #4 migrates those families to the new shape.

    ``resolve`` receives a tuple of chosen targets (empty for
    untargeted effects). New-shape effects iterate the chosen targets
    in slot order; back-reference ``SlotRef`` slots are NOT in the
    targets tuple — the resolve path looks the parent slot's resolved
    target back up in-place.
    """

    # Per-effect target list. Empty for old-shape effects whose
    # targeting machinery lives on per-class ``requires_target`` /
    # ``legal_targets`` overrides; non-empty for new-shape effects
    # whose targeting is driven structurally through this field.
    # Class-level default — subclasses opt in by setting their own
    # ``targets`` field (typically as a frozen-dataclass field).
    targets: "tuple[TargetSlot, ...]" = ()

    def requires_target(self) -> bool:
        """Whether this effect needs at least one chosen target to resolve.

        Default: True iff any ``TargetSpec`` in ``self.targets`` has a
        ``count_min >= 1``. ``SlotRef`` slots don't count toward
        target-required because they're back-references, not fresh
        prompts. Old-shape effects override this directly and the
        default never fires for them (their ``targets`` is empty).
        """
        from mtgz_ir.effects.targets import TargetSpec
        for slot in self.targets:
            if isinstance(slot, TargetSpec) and slot.count_min >= 1:
                return True
        return False

    def legal_targets(
        self, state: "GameState", controller_id: int
    ) -> list[Target]:
        """Enumerate legal targets for the FIRST ``TargetSpec`` slot.

        Default behaviour: walks the battlefield + players for objects
        satisfying the slot's ``filter``, returning each as the
        engine's ``("creature"|"player"|"permanent", id)`` tuple form.
        Old-shape effects override this and the default never fires;
        new-shape effects with multiple ``TargetSpec`` slots use
        ``legal_targets_for_slot`` (below).

        Empty list means no targets are needed.
        """
        if not self.targets:
            return []
        from mtgz_ir.effects.targets import TargetSpec
        # Find the first TargetSpec slot — any leading ``SlotRef`` is
        # nonsense (no slot to back-reference) but defensively skip.
        for slot in self.targets:
            if isinstance(slot, TargetSpec):
                return self._enumerate_for_filter(state, controller_id, slot.filter)
        return []

    @staticmethod
    def _enumerate_for_filter(
        state: "GameState", controller_id: int, target_filter
    ) -> list[Target]:
        """Walk battlefield + players, returning legal targets for
        ``target_filter``. Pure helper — used by both
        :meth:`legal_targets` and per-slot enumeration in multi-slot
        new-shape effects.

        Pins ``state.resolving_controller_id`` to ``controller_id``
        for the duration of the walk so controller-aware atoms
        (``ControlledByOpponent`` / ``ControlledByYou``) can resolve
        "you" / "an opponent" without the predicate test signature
        having to grow. Restores the prior value on exit so nested
        enumerations don't see a stale id.
        """
        from mtgz_ir.game import is_creature_now

        prior_controller = getattr(state, "resolving_controller_id", None)
        try:
            state.resolving_controller_id = controller_id
            results: list[Target] = []
            for o in state.battlefield:
                if not target_filter.test(o, state):
                    continue
                # Pick the engine "kind" tag for the object. Creatures
                # use the legal-now check (``is_creature_now``) so
                # impending-suppressed Overlords aren't valid creature
                # targets even if their printed type carries Creature.
                if is_creature_now(o):
                    results.append(("creature", o.id))
                elif "Planeswalker" in o.printing.card_types:
                    results.append(("planeswalker", o.id))
                elif "Battle" in o.printing.card_types:
                    results.append(("battle", o.id))
                else:
                    results.append(("permanent", o.id))
            # Players are not on the battlefield list — walk them
            # separately so an "any target" filter (which accepts
            # IsPlayer) picks them up. Filters that don't admit
            # players (``IsCreature`` etc.) reject every player via
            # their own ``test`` and the loop is a cheap no-op.
            for p in state.players:
                if p.has_lost:
                    continue
                if target_filter.test(p, state):
                    results.append(("player", p.id))
            return results
        finally:
            state.resolving_controller_id = prior_controller

    def target_count_range(self) -> tuple[int, int]:
        """Return (min, max) number of TARGETS (not slots).

        Default for new-shape effects: sums ``count_min`` and
        ``count_max`` across every ``TargetSpec`` in ``self.targets``;
        ``SlotRef`` slots contribute zero. Old-shape effects override
        this and the default never fires.

        For backward-compat with old-shape effects whose
        ``requires_target()`` returns True without overriding this:
        returns ``(1, 1)`` so the "old default" stays correct.
        """
        from mtgz_ir.effects.targets import TargetSpec
        if not self.targets:
            return (1, 1) if self.requires_target() else (0, 0)
        total_min = 0
        total_max: float = 0
        for slot in self.targets:
            if isinstance(slot, TargetSpec):
                total_min += slot.count_min
                if slot.count_max == math.inf:
                    total_max = math.inf
                elif total_max != math.inf:
                    total_max += slot.count_max
        # Cap to int when finite (callers expect ints for the legacy
        # one-spec common case); leave math.inf intact for the "any
        # number of target X" form.
        if total_max == math.inf:
            return (total_min, total_max)  # type: ignore[return-value]
        return (total_min, int(total_max))

    def target_slot_kinds(self) -> tuple[str, ...]:
        """Per-slot required target kinds, for effects with multiple
        distinct-kind slots (e.g., Diplomatic Relations targets BOTH
        a friendly creature ``"my_creature"`` AND an opponent's
        creature ``"creature"``). Returning a non-empty tuple opts in
        to per-slot kind enforcement: the targeting pipeline filters
        the legal-targets pool to only entries of the matching kind
        when picking each slot, preventing illegal-kind picks (e.g.,
        a friendly creature being chosen for an opp-creature slot).

        Empty tuple (default) keeps the legacy flat-kind behavior:
        all entries in ``legal_targets`` are picked from a shared
        pool, regardless of kind. Used by single-target effects and
        flexible effects whose only kind is implicit.

        When non-empty, the tuple length should equal
        ``target_count_range()[1]`` (max).
        """
        return ()

    def resolve(
        self, state: "GameState", controller_id: int, targets: tuple[Target, ...] = ()
    ) -> None:
        raise NotImplementedError

    def describe(self) -> str:
        raise NotImplementedError



# Type alias for an ordered run of effects executed back-to-back. Slot
# indices in ``TargetSpec.distinct_from`` and ``SlotRef.idx`` are 0-
# based across the EffectSequence, NOT across the larger card. A modal
# wrapper resets the scope for each mode.
EffectSequence = tuple[Effect, ...]


@dataclass(frozen=True)
class ModalEffect(Effect):
    """A modal wrapper around several alternative effect sequences.

    Models "Choose one — / Choose two —" / "Choose any number —"
    cards. ``pick`` is the printed number of modes the caster
    chooses (``1`` for "Choose one"; ``2`` for "Choose two"; etc.).
    ``modes`` is a tuple of :data:`EffectSequence` — each mode is its
    own sequence, with its own slot scope.

    ``repeats_allowed=True`` models the "the same mode may be chosen
    more than once" rider on Charm-style cards; default False matches
    "different modes" semantics.

    Out of scope this PR: actual mode-selection plumbing in the
    targeting pipeline. This dataclass exists so the architectural
    grammar is complete (PR #3 design table covers modal in the
    layered types) — there are no engine consumers in this PR.
    """

    pick: int = 1
    modes: tuple[EffectSequence, ...] = ()
    repeats_allowed: bool = False

    def describe(self) -> str:
        # Coarse human-readable summary; the full mode text lives on
        # each sub-effect. ``pick`` of 1 reads as "Choose one"; 2 as
        # "Choose two"; etc.
        words = {1: "one", 2: "two", 3: "three"}
        n = words.get(self.pick, str(self.pick))
        joined = " | ".join(
            "; ".join(eff.describe() for eff in seq)
            for seq in self.modes
        )
        return f"Choose {n} — {joined}"



def _multicolored_permanents_count(
    state: "GameState", controller_id: int
) -> int:
    """Count permanents the controller controls that are two or more
    colors (``game.effective_colors``: layer 5 -- Mycosynth Lattice makes
    them all colorless). Matches the "multicolored permanent" filter
    used by Infinite Guideline Station.
    """
    from mtgz_ir.game import effective_colors

    count = 0
    for o in state.battlefield:
        if o.controller_id != controller_id:
            continue
        if len(effective_colors(state, o)) >= 2:
            count += 1
    return count



def _apply_damage(state: "GameState", target: Target, amount: int) -> None:
    """Apply ``amount`` damage to ``target``. Used by damage effects.

    Stage-3 scope: no source-tracking (so deathtouch on a pinger wouldn't
    mark its target lethal yet — that'd layer in when we thread damage
    sources through the ETB / ability pipeline).

    For damage to a creature, also fires
    ``enchanted_creature_dealt_damage`` triggers on attached auras
    (Cryoshatter, Pain for All) — mirrors the combat-damage path in
    ``combat.py``. The aura's TriggeredAbility effects resolve
    immediately, with the aura set as ``state.resolving_source`` and the
    damage amount in ``state.resolving_combat_damage_amount``.
    """
    kind, target_id = target
    # Roto P6: prevention / redirection effects (Black Panther, Nomads
    # en-Kor); redirected damage is dealt from inside the router.
    from mtgz_ir.effects.generic.roto_damage import (
        damage_cant_be_prevented, route_damage,
    )

    amount = route_damage(state, target, amount)
    if amount <= 0:
        return
    if kind == "player":
        from mtgz_ir.game import player_damage_prevented
        if player_damage_prevented(state, target_id):
            return
        p = state.player_by_id(target_id)
        from mtgz_ir.effects.generic.roto_keywords import life_after_damage

        p.life = life_after_damage(state, p, amount)  # Angel's Grace floor
        state.log.append(f"P{target_id} takes {amount} damage (life: {p.life})")
        _fire_deals_damage(state, amount)
        from mtgz_ir.effects.generic.roto_keywords import after_damage_to_player
        from mtgz_ir.keyword_rules import current_damage_source

        after_damage_to_player(state, current_damage_source(state), target_id, amount)
    elif kind == "creature":
        obj = next((o for o in state.battlefield if o.id == target_id), None)
        if obj is not None:
            from mtgz_ir.keyword_rules import (
                current_damage_source, is_protected_from,
            )

            source = current_damage_source(state)
            if is_protected_from(state, obj, source) and not (
                damage_cant_be_prevented(state)
            ):
                state.log.append(
                    f"{amount} damage to {obj.printing.name} from "
                    f"{source.printing.name} is prevented (protection)"
                )
                return
            if obj.printing.heals_other_damage_when_dealt_damage:
                from mtgz_ir.effects.generic.cube_creatures_rmc import (
                    heal_before_damage,
                )

                heal_before_damage(obj)
            obj.damage += amount
            obj.damaged_this_turn = True
            if obj.printing.is_planeswalker:
                # CR 120.3c + 120.3e: a planeswalker creature (Kaito
                # during your turn) both loses loyalty and is marked.
                obj.loyalty_counters -= amount
            state.log.append(f"{obj.printing.name} takes {amount} damage")
            # Lazy-imported to avoid an effects <-> combat import cycle.
            from mtgz_ir.combat import _fire_enchanted_dealt_damage_triggers

            _fire_enchanted_dealt_damage_triggers(state, obj, amount)
            _fire_deals_damage(state, amount)
    elif kind == "planeswalker":
        # CR 120.3c: damage dealt to a planeswalker removes that many
        # loyalty counters from it. The 0-loyalty SBA (704.5i) buries it.
        obj = next((o for o in state.battlefield if o.id == target_id), None)
        if obj is not None:
            obj.loyalty_counters -= amount
            state.log.append(
                f"{obj.printing.name} takes {amount} damage "
                f"(loyalty: {obj.loyalty_counters})"
            )
            _fire_deals_damage(state, amount)


def _fire_deals_damage(state: "GameState", amount: int) -> None:
    """Noncombat damage: "whenever <source> deals damage" observers of
    the object dealing it (``keyword_rules.current_damage_source``)."""
    from mtgz_ir.keyword_rules import current_damage_source
    from mtgz_ir.layouts import fire_deals_damage

    fire_deals_damage(state, current_damage_source(state), amount)



def _find_moonlit_replacement_source(
    state: "GameState", controller_id: int,
):
    """Return an attached aura with replaces_first_tokens_with_copies
    controlled by ``controller_id`` whose enchanted permanent is also
    under the controller's control, or None.
    """
    for o in state.battlefield:
        if o.controller_id != controller_id:
            continue
        if not o.printing.replaces_first_tokens_with_copies:
            continue
        if o.attached_to is None:
            continue
        return o
    return None



@dataclass(frozen=True)
class ActivationCost:
    """Cost to activate an activated ability.

    Composable pieces: mana cost, tap the source, sacrifice the source.
    "Tap another creature" style costs aren't covered here — Station
    already has its own Action for that specialised case.
    """

    mana: "ManaCost | None" = None
    mana_str: str = ""  # display form like "{2}"
    tap_self: bool = False
    sacrifice_self: bool = False
    # "Exile [the source permanent]" — Plaza of Heroes's fourth ability.
    # The source is exiled as a cost, not sacrificed, so it goes to the
    # exile zone rather than the graveyard. Paid last (same ordering as
    # sacrifice_self) so target selection and other cost payments can
    # still read the source from the battlefield.
    exile_self: bool = False
    # "Discard a card" — the activator discards a card of their choice
    # from hand as a cost (Blood token, Iron-Shield Elf). Legal only
    # with a nonempty hand.
    discard_a_card: bool = False
    # "Pay {E}{E}" — spend N energy counters (CR 122.1). Legal only with
    # at least N energy.
    pay_energy: int = 0
    # "Tap an untapped permanent you control" — Gene Pollinator's mana
    # ability. The activating player picks any other untapped permanent
    # they control (not the source) and taps it as a cost.
    tap_permanent_you_control: bool = False
    # Narrows ``tap_permanent_you_control`` to a card type: "Tap an
    # untapped artifact you control" (Urza, Lord High Artificer) ->
    # ``"Artifact"``. Empty = any permanent.
    tap_permanent_type: str = ""
    # "Exile an artifact card from your graveyard" — Rust Harvester's
    # activated ability. The activating player exiles one artifact card
    # from their graveyard as a cost. Legal only when the graveyard
    # contains at least one artifact card.
    exile_artifact_from_graveyard: bool = False
    # "Exile three cards from your graveyard" — Psychic Frog. The
    # activating player exiles N cards of their choice from their own
    # graveyard; legal only with at least N cards there.
    exile_cards_from_graveyard: int = 0
    # Ninjutsu's "Return an unblocked attacker you control to hand"
    # (CR 702.49a). Legal only in the declare blockers step with an
    # unblocked attacker the activator controls.
    return_unblocked_attacker: bool = False
    # "Sacrifice another creature or artifact" — the caster picks a
    # legal target via choose_sacrifice. Cannot be the source itself.
    sacrifice_another_creature_or_artifact: bool = False
    # "Sacrifice another artifact or land" — as above but with a land
    # filter. Matches Slagdrill Scrapper.
    sacrifice_another_artifact_or_land: bool = False
    # "Sacrifice another artifact" — strict artifact-only filter.
    # Matches Memorial Vault. Cannot be the source itself.
    sacrifice_another_artifact: bool = False
    # "Remove a +1/+1 counter from a creature you control" — Sunstar
    # Chaplain's activated ability. Fails if no such creature exists.
    remove_plus_one_counter_from_your_creature: bool = False
    # "Sacrifice a Food" — Ragost, Deft Gastronaut's activated. Any
    # permanent with the Food subtype (including those granted Food
    # by Ragost's own static) is a legal sacrifice. Can sac the
    # source itself if the source has Food — but Ragost the Lobster
    # isn't an artifact so he's not a Food.
    sacrifice_food: bool = False
    # "Exile [the granting equipment]" — The Dominion Bracelet. When
    # True, the cost includes exiling the equipment attached to the
    # ability's source that granted this ability.
    exile_granting_equipment: bool = False
    # "This ability costs {X} less to activate, where X is this
    # creature's power." The Dominion Bracelet. Reduces the ability's
    # mana cost generic portion by the source's effective power at
    # activation time.
    mana_reduced_by_source_power: bool = False
    # Pay N life. Mana Confluence's "{T}, Pay 1 life: Add one mana
    # of any color." Paid at activation; gated by current life total.
    pay_life: int = 0
    # "Sacrifice a land" — Dust Bowl. Any land the controller controls
    # (excluding the source) is a legal sac.
    sacrifice_a_land: bool = False
    # "Sacrifice a Desert" — Scavenger Grounds. Subtype-filtered sac;
    # the source itself is eligible if it's a Desert.
    sacrifice_a_desert: bool = False
    # "Activate only if you control a creature with power 4 or
    # greater." Bonders' Enclave. Legal-actions gate only.
    requires_own_creature_power_at_least: int = 0
    # "Activate only if you control three or more artifacts."
    # Inventors' Fair's tutor activation. Legal-actions gate only:
    # the activation is removed from legal moves when the activator
    # controls fewer than N artifacts.
    requires_own_artifact_count_at_least: int = 0
    # "Activate only if you control a <T1> or a <T2>." Duskmourn Verge
    # cycle (Blazemire Verge, Floodfarm Verge, …). The tuple holds the
    # land-subtype names (e.g. ``("Plains", "Island")``); legal-actions
    # checks at least one of those subtypes appears on a permanent the
    # activator controls. Empty tuple disables the gate. Order does not
    # matter; matching is OR'd across the tuple.
    requires_control_of_subtype: tuple[str, ...] = ()
    # "Tap X untapped artifacts you control" — Secluded Starforge's
    # second ability. X is the number of untapped artifacts the
    # activating player CHOOSES to tap as part of the cost (X may be 0,
    # in which case the +X/+0 pump is a no-op). Bounded by the count of
    # untapped artifacts the player controls. The chosen X is threaded
    # onto the StackItem's ``x_value`` (the same channel ``{X}`` mana
    # costs use) so the resolving ``power_bonus_from_x`` pump reads it
    # via ``state.resolving_x_value``. The activating player picks the
    # count via the existing ``choose_x`` agent hook; the engine taps
    # the lowest-id-first X untapped artifacts deterministically.
    tap_x_untapped_artifacts_you_control: bool = False
    # "Discard your hand" — Lion's Eye Diamond. The activator discards
    # every card in their hand as part of the cost (an empty hand is a
    # legal, zero-card payment). Discard triggers fire per card.
    discard_hand: bool = False
    # "Sacrifice a <Subtype>" (Retrofitter Foundry's "Sacrifice a Servo"
    # / "Sacrifice a Thopter"). Any permanent the activator controls
    # with that (effective) subtype is a legal sacrifice, the source
    # included if it has the subtype. Empty string disables.
    sacrifice_subtype: str = ""
    # "Sacrifice a nontoken artifact" (Thopter Foundry). Any nontoken
    # artifact the activator controls, the source itself included.
    sacrifice_nontoken_artifact: bool = False
    # "Remove N <kind> counter(s) from this <permanent>" (Staff of the
    # Storyteller's story counter, Wishclaw Talisman's wish counter,
    # Reckoner Bankbuster's charge counter, Pentad Prism). ``kind`` is
    # the printed counter name; legal only while the source has at least
    # ``remove_counters_from_self_amount`` such counters.
    remove_counters_from_self_kind: str = ""
    remove_counters_from_self_amount: int = 0
    # Crew N (CR 702.122): "Tap any number of untapped creatures you
    # control with total power N or more". The activator picks the
    # creatures one at a time (``Agent.choose_permanent_to_tap``) until
    # the total reaches N; summoning-sick creatures may crew. A
    # creature's ``crew_power_bonus`` (Reckoner Bankbuster's Pilot:
    # "crews Vehicles as though its power were 2 greater") is added.
    crew: int = 0
    # Saddle N (CR 702.171): tap any number of other untapped creatures
    # you control with total power N or more (plain power -- crew-only
    # bonuses don't apply).
    saddle: int = 0
    # "Sacrifice N nonland permanents" (Bolas's Citadel: ten). The
    # activator picks them one at a time (``choose_sacrifice``); the
    # source itself is a legal choice.
    sacrifice_n_nonland_permanents: int = 0
    # **Planeswalker loyalty cost (CR 606).** Signed loyalty cost.
    # Positive values are PAY costs (the walker LOSES N loyalty
    # counters); negative values are ADD costs (the walker GAINS |N|
    # loyalty counters). Defaults to 0 (no loyalty cost). Convention:
    # a printed "+2" ability stamps ``loyalty=-2``; a printed "-3"
    # ability stamps ``loyalty=3``; a printed "0" ability stamps
    # ``loyalty=0``. This mirrors Rust's ``ActivationCost.loyalty: i32``
    # (cards.rs) sign convention exactly.
    #
    # Emission (``legal.py``): positive ``loyalty`` gates on the
    # source's ``loyalty_counters`` — the walker needs at least
    # ``cost.loyalty`` counters to afford. Negative / zero are always
    # affordable. Plus the once-per-turn-per-walker gate (CR 606.3,
    # via ``state.loyalty_activated_this_turn``).
    #
    # Apply (``actions._activate_ability``): ``obj.loyalty_counters -=
    # cost.loyalty`` (signed subtraction handles both directions), then
    # the (controller_id, source_id) pair is recorded in
    # ``state.loyalty_activated_this_turn``. The loyalty-at-0 SBA
    # (``turn.py`` 704.5i) catches a walker that drops to 0.
    loyalty: int = 0
    # "-X" loyalty cost (Ugin, the Spirit Dragon; Jace, the Perfected
    # Mind): X is chosen as the ability is activated (0 .. the walker's
    # loyalty), that many loyalty counters are removed, and X rides the
    # stack item as ``state.resolving_x_value``
    # (``roto_walkers.choose_and_pay_loyalty_x``).
    loyalty_x: bool = False
    # "Discard this card" — the hand-activated cost atom of Cycling
    # (CR 702.29a: "[Cost], Discard this card: Draw a card.") and
    # Channel (CR 702.140 ability word over the same shape). Only
    # meaningful on abilities listed in
    # ``CardPrinting.hand_activated_abilities``: the source is the card
    # in its owner's hand, and paying the cost moves it to the
    # graveyard (a discard — discard triggers fire) before the ability
    # goes on the stack.
    discard_self: bool = False
    # "Remove a <kind> counter from this <permanent>" for a
    # ``GameObject.named_counters`` kind (Gemstone Mine: "mining").
    # ``legal_actions`` requires at least one such counter on the
    # source; ``_activate_ability`` removes one. "" = no such cost.
    remove_named_counter_from_self: str = ""
    # "Exert this <permanent>" (CR 701.39) — sets
    # ``GameObject.exerted`` so the source skips its controller's next
    # untap step (Arena of Glory).
    exert_self: bool = False
    # "Sacrifice a <card type>" for a lowercase card type
    # (Goblin Bombardment's "Sacrifice a creature"). Any permanent the
    # activator controls of that (current) type is a legal sacrifice,
    # the source included when it qualifies. "" = no such cost. The
    # dedicated land field (``sacrifice_a_land``, Dust Bowl) keeps its
    # own source-excluding semantics.
    sacrifice_a_permanent_type: "CardTypeWord | None" = None
    # Composable cost atoms beyond the dedicated boolean fields above
    # (cube creatures R/multicolor/colorless bucket): each entry is an
    # object exposing ``payable(state, player_id, source) -> bool``,
    # ``pay(state, player_id, source) -> list[GameObject]`` (the objects
    # paid, threaded onto ``StackItem.cost_paid_objects``) and
    # ``describe() -> str``. ``legal_actions`` gates activation on every
    # atom being payable; ``_activate_ability`` pays them after target
    # selection and before sacrifice-self / exile-self. See
    # ``mtgz_ir.effects.generic.cube_creatures_rmc`` ("Sacrifice a
    # Forest", "Sacrifice five Treasures", "Remove a +1/+1 counter from
    # this creature", "Tap an untapped Rebel you control", "Exile this
    # card from your graveyard").
    extra_costs: tuple = ()
    # Waterbend {N} (Avatar: The Last Airbender): "While paying a
    # waterbend cost, you can tap your untapped artifacts and creatures to
    # help. Each one pays for {1}." (``layouts.pay_waterbend``).
    waterbend: bool = False

    def __post_init__(self) -> None:
        object.__setattr__(self, "sacrifice_a_permanent_type", coerce(CardTypeWord, self.sacrifice_a_permanent_type or None))

    def describe(self) -> str:
        parts: list[str] = []
        for atom in self.extra_costs:
            parts.append(atom.describe())
        if self.mana_str:
            parts.append(self.mana_str)
        if self.discard_self:
            parts.append("Discard this card")
        if self.exert_self:
            parts.append("Exert ~")
        if self.remove_named_counter_from_self:
            parts.append(
                f"Remove a {self.remove_named_counter_from_self} counter "
                f"from ~"
            )
        if self.tap_self:
            parts.append("{T}")
        if self.sacrifice_self:
            parts.append("Sacrifice ~")
        if self.exile_self:
            parts.append("Exile ~")
        if self.tap_permanent_you_control:
            noun = (self.tap_permanent_type or "permanent").lower()
            parts.append(f"Tap an untapped {noun} you control")
        if self.exile_artifact_from_graveyard:
            parts.append("Exile an artifact card from your graveyard")
        if self.return_unblocked_attacker:
            parts.append("Return an unblocked attacker you control to hand")
        if self.exile_cards_from_graveyard:
            parts.append(
                f"Exile {self.exile_cards_from_graveyard} cards from your "
                "graveyard"
            )
        if self.sacrifice_another_creature_or_artifact:
            parts.append("Sacrifice another creature or artifact")
        if self.sacrifice_another_artifact_or_land:
            parts.append("Sacrifice another artifact or land")
        if self.sacrifice_another_artifact:
            parts.append("Sacrifice another artifact")
        if self.remove_plus_one_counter_from_your_creature:
            parts.append("Remove a +1/+1 counter from a creature you control")
        if self.sacrifice_food:
            parts.append("Sacrifice a Food")
        if self.exile_granting_equipment:
            parts.append("Exile this equipment")
        if self.mana_reduced_by_source_power:
            parts.append("(costs {X} less where X is this creature's power)")
        if self.pay_life:
            parts.append(f"Pay {self.pay_life} life")
        if self.sacrifice_a_land:
            parts.append("Sacrifice a land")
        if self.sacrifice_a_desert:
            parts.append("Sacrifice a Desert")
        if self.tap_x_untapped_artifacts_you_control:
            parts.append("Tap X untapped artifacts you control")
        if self.discard_hand:
            parts.append("Discard your hand")
        if self.sacrifice_subtype:
            parts.append(f"Sacrifice a {self.sacrifice_subtype}")
        if self.sacrifice_a_permanent_type:
            parts.append(f"Sacrifice a {self.sacrifice_a_permanent_type}")
        if self.sacrifice_nontoken_artifact:
            parts.append("Sacrifice a nontoken artifact")
        if self.crew:
            parts.append(f"Crew {self.crew}")
        if self.saddle:
            parts.append(f"Saddle {self.saddle}")
        if self.sacrifice_n_nonland_permanents:
            parts.append(
                f"Sacrifice {self.sacrifice_n_nonland_permanents} nonland "
                f"permanents"
            )
        if self.remove_counters_from_self_kind:
            parts.append(
                f"Remove {self.remove_counters_from_self_amount} "
                f"{self.remove_counters_from_self_kind} counter(s) from ~"
            )
        if self.loyalty:
            # Printed-sign convention: positive cost.loyalty is a "-N"
            # ability, negative is "+N".
            if self.loyalty > 0:
                parts.append(f"-{self.loyalty}")
            else:
                parts.append(f"+{-self.loyalty}")
        return ", ".join(parts) if parts else "0"



@dataclass(frozen=True)
class ActivatedAbility:
    """An ability players can activate by paying ``cost``. On activation the
    ability goes on the stack; when it resolves, ``effect`` runs.

    ``sorcery_speed=True`` limits activation to main-phase-with-empty-stack
    timing (matches "Activate only as a sorcery").

    ``is_mana_ability=True`` short-circuits the stack: the effect
    resolves immediately on activation (real MTG rule 605 — mana
    abilities don't use the stack). Pair with an AddManaToPool effect.

    ``cost_reductions`` applies conditional generic-mana reductions to
    THIS ability's mana cost (analogous to ``CardPrinting.cost_reductions``
    for spell casts). Used by Thaumaton Torpedo's "this ability costs
    {3} less to activate if you attacked with a Spacecraft this turn".
    Walked by ``_effective_ability_mana_cost``; stored as a tuple of
    ``CostReduction`` (typed via ``Any`` here to dodge the cards-module
    import cycle).
    """

    cost: ActivationCost
    effect: Effect
    sorcery_speed: bool = False
    is_mana_ability: bool = False
    # Conditional generic-mana reductions to this ability's cost.
    # Mirrors CardPrinting.cost_reductions but applied at activation
    # time by actions._effective_ability_mana_cost. Matches Starport
    # Security's "this ability costs {2} less to activate if you
    # control a creature with a +1/+1 counter on it." and Thaumaton
    # Torpedo's "this ability costs {3} less to activate if you
    # attacked with a Spacecraft this turn."
    cost_reductions: tuple["CostReduction", ...] = ()
    # DSK Rooms: when this ability lives on a Room half, this is the
    # half-name (matches a key in ``CardPrinting.room_halves``).
    # ``effective_activated_abilities`` filters out abilities whose
    # ``room_half`` is not in the source's ``unlocked_halves``; the
    # half is locked, so its abilities are inert. ``None`` for
    # non-Room abilities — they're always active.
    room_half: "str | None" = None
    # EoE Spacecraft tier gate: this activated ability is only usable
    # when the source has ``tier_threshold`` or more charge counters.
    # 0 = always usable (default). Mirrors ``StaticAbility.tier_threshold``.
    # ``effective_activated_abilities`` filters out gated abilities when
    # the source doesn't meet the threshold. Used by The Eternity
    # Elevator's ``20+ | {T}: Add X mana of any one color`` ability.
    tier_threshold: int = 0
    # ``Activate only once.`` rider — DSK Haunted Screen's third
    # activation. When True, the ability is legal only while the
    # source's ``has_activated_once_only_ability`` flag is False; the
    # flag is latched at activation time (in ``_activate_ability``)
    # so a second activation (instant- or sorcery-speed) is rejected
    # by ``legal_actions`` even before the first activation has
    # resolved off the stack. The flag is per-``GameObject``, so a
    # blink-and-recast (a fresh ``GameObject`` instance) gets a fresh
    # one-shot per CR 706.2.
    activate_only_once: bool = False
    # "Activate only if <condition>." rider (CR 602.5b) — Library of
    # Alexandria's "Activate only if you have exactly seven cards in
    # hand." A ``Condition`` evaluated from the activator's viewpoint
    # (``condition.matches(state, activator_id)``); ``legal_actions``
    # omits the activation while it's False and ``_activate_ability``
    # refuses it defensively. ``None`` = no restriction.
    activate_only_if: "Condition | None" = None
    # ``Activate only as an instant.`` rider (Lion's Eye Diamond). The
    # ability may be activated only when its controller could cast an
    # instant — i.e. while holding priority. Every activation in this
    # engine already happens through ``legal_actions`` at priority, so
    # the rider's observable consequence is that the ability may NOT
    # be activated in the middle of paying another spell's cost: the
    # sac-for-mana auto-payment path (``_classify_sac_for_mana``)
    # skips abilities carrying this flag.
    activate_only_as_instant: bool = False
    # ``Activate only during your turn.`` rider (Wishclaw Talisman).
    # ``legal_actions`` filters the ability out unless the activator is
    # the active player.
    activate_only_during_your_turn: bool = False
    # Exhaust (CR 702.177): "Activate each exhaust ability only once."
    # Tracked per ability index on ``GameObject.exhausted_abilities`` so
    # a permanent with several exhaust abilities (Loot, the Pathfinder)
    # can use each of them once; a new object (CR 400.7) starts fresh.
    exhaust: bool = False
    # Boast (CR 702.142): "Activate only if this creature attacked this
    # turn and only once each turn." Gated in ``legal_actions`` on the
    # source being among its controller's attackers this turn and
    # ``GameObject.boasted_turn`` differing from the current turn.
    boast: bool = False

    def describe(self) -> str:
        prefix = "Exhaust — " if self.exhaust else ""
        if self.boast:
            prefix = "Boast — "
        return f"{prefix}{self.cost.describe()}: {self.effect.describe()}"



class ScopeKind(StrEnum):
    """Which permanents a ``StaticAbility`` affects (see ``StaticScope``)."""

    # Only the source itself.
    SELF = "self"
    # Creatures the source's controller controls.
    CREATURES_YOU_CONTROL = "creatures_you_control"
    # ... other than the source.
    OTHER_CREATURES_YOU_CONTROL = "other_creatures_you_control"
    # ... with a +1/+1 counter on them.
    CREATURES_YOU_CONTROL_WITH_COUNTER = "creatures_you_control_with_counter"
    # Other tapped legendary creatures you control (The Seriema).
    OTHER_TAPPED_LEGENDARY_CREATURES_YOU_CONTROL = (
        "other_tapped_legendary_creatures_you_control"
    )
    # "[Other ]<Subtype>s you control" (Magda's Dwarves, Slivers):
    # creatures with ``StaticScope.subtype`` among their effective
    # subtypes; ``StaticScope.other`` excludes the source.
    SUBTYPE_YOU_CONTROL = "subtype_you_control"
    # "<Card type>s you control" (``StaticScope.card_type``): the only
    # kind that reaches noncreature permanents (Nissa, Who Shakes the
    # World's emblem: "Lands you control have indestructible.").
    CARD_TYPE_YOU_CONTROL = "card_type_you_control"


@dataclass(frozen=True)
class StaticScope:
    """Scope of a static ability — which permanents it affects.

    ``kind`` picks the family; ``subtype`` / ``other`` parameterize
    ``SUBTYPE_YOU_CONTROL`` and ``card_type`` parameterizes
    ``CARD_TYPE_YOU_CONTROL``. The common scopes are class constants
    (``StaticScope.SELF``, ``StaticScope.CREATURES_YOU_CONTROL``, ...).
    """

    kind: ScopeKind
    subtype: str = ""
    other: bool = False
    card_type: str = ""

    def __post_init__(self) -> None:
        object.__setattr__(self, "kind", ScopeKind(self.kind))
        if (self.kind is ScopeKind.SUBTYPE_YOU_CONTROL) != bool(self.subtype):
            raise ValueError(f"subtype must be set exactly for SUBTYPE_YOU_CONTROL: {self!r}")
        if (self.kind is ScopeKind.CARD_TYPE_YOU_CONTROL) != bool(self.card_type):
            raise ValueError(f"card_type must be set exactly for CARD_TYPE_YOU_CONTROL: {self!r}")
        if self.other and self.kind is not ScopeKind.SUBTYPE_YOU_CONTROL:
            raise ValueError(f"other is only a SUBTYPE_YOU_CONTROL parameter: {self!r}")

    @staticmethod
    def card_type_you_control(card_type: str) -> "StaticScope":
        return StaticScope(ScopeKind.CARD_TYPE_YOU_CONTROL, card_type=card_type)

    @staticmethod
    def subtype_you_control(subtype: str, *, other: bool) -> "StaticScope":
        return StaticScope(ScopeKind.SUBTYPE_YOU_CONTROL, subtype=subtype, other=other)

    def describe(self) -> str:
        if self.kind is ScopeKind.SUBTYPE_YOU_CONTROL:
            return f"{'Other ' if self.other else ''}{self.subtype}s you control"
        if self.kind is ScopeKind.CARD_TYPE_YOU_CONTROL:
            return f"{self.card_type}s you control"
        return {
            ScopeKind.SELF: "self",
            ScopeKind.CREATURES_YOU_CONTROL: "Creatures you control",
            ScopeKind.OTHER_CREATURES_YOU_CONTROL: "Other creatures you control",
            ScopeKind.CREATURES_YOU_CONTROL_WITH_COUNTER: (
                "Creatures you control with a +1/+1 counter on them"
            ),
            ScopeKind.OTHER_TAPPED_LEGENDARY_CREATURES_YOU_CONTROL: (
                "Other tapped legendary creatures you control"
            ),
        }[self.kind]


StaticScope.SELF = StaticScope(ScopeKind.SELF)
StaticScope.CREATURES_YOU_CONTROL = StaticScope(ScopeKind.CREATURES_YOU_CONTROL)
StaticScope.OTHER_CREATURES_YOU_CONTROL = StaticScope(
    ScopeKind.OTHER_CREATURES_YOU_CONTROL,
)
StaticScope.CREATURES_YOU_CONTROL_WITH_COUNTER = StaticScope(
    ScopeKind.CREATURES_YOU_CONTROL_WITH_COUNTER,
)
StaticScope.OTHER_TAPPED_LEGENDARY_CREATURES_YOU_CONTROL = StaticScope(
    ScopeKind.OTHER_TAPPED_LEGENDARY_CREATURES_YOU_CONTROL,
)
# Slivers you control — Thrumming Hivepool.
StaticScope.SLIVERS_YOU_CONTROL = StaticScope.subtype_you_control(
    "Sliver", other=False,
)



@dataclass(frozen=True)
class StaticAbility:
    """A continuous ability on a permanent that grants keywords and/or P/T
    bonuses to matching permanents while the source is on the battlefield.

    Stage 3 supports anthem-style scopes. Consumers (effective_keywords,
    effective_power, effective_toughness) walk the battlefield at query
    time and fold in applicable bonuses — no layers system yet, so the
    ordering-of-continuous-effects subtleties from the comprehensive rules
    don't apply.
    """

    scope: StaticScope
    granted_keywords: frozenset[KeywordSpelling] = frozenset()
    power_bonus: int = 0
    toughness_bonus: int = 0
    # Tiered abilities on Spacecraft: the ability is only active when the
    # source has ``tier_threshold`` or more charge counters. 0 = always on.
    tier_threshold: int = 0
    # DSK Rooms: when this static lives on a Room half, this is the
    # half-name (matches a key in ``CardPrinting.room_halves``). While
    # the half is locked (not in ``unlocked_halves``), the ability is
    # inert. ``None`` for non-Room statics — always active.
    room_half: "str | None" = None
    # Player-scope static: source's controller has no maximum hand
    # size (CR 402.2 default 7-card cleanup discard suspended).
    # Steaming Sauna (DSK Room half) prints ``You have no maximum
    # hand size.`` Gated by ``room_half`` while the half is locked.
    # ``run_cleanup_step`` scans the battlefield for active statics
    # carrying this flag whose source's controller is the active
    # player and skips the cleanup-discard loop when found, mirroring
    # CR 402.2's "no maximum hand size" replacement of the discard-
    # to-7 rule. The flag is independent of the creature-target
    # ``applies_to`` walk — this static doesn't grant keywords or
    # P/T bonuses to creatures, so the existing scope/applies_to
    # plumbing isn't exercised.
    grants_no_max_hand_size: bool = False
    # "Creature spells you control can't be countered." When True on a
    # permanent's static ability, ``CounterTargetSpell.legal_targets``
    # skips creature spells whose controller matches this permanent's
    # controller, and ``CounterTargetSpell.resolve`` is a no-op for such
    # targets. The effect is continuous (CR 101.2): while the source is
    # on the battlefield, every creature spell the controller puts on the
    # stack is shielded. Scope is implicitly the casting player's side;
    # ``applies_to`` is not used (this static applies to spells on the
    # stack, not permanents on the battlefield). Matches Frenzied Baloth's
    # third oracle line.
    grants_creature_spells_uncounterable: bool = False
    # "Spells you control can't be countered." (Hexing Squelcher) — the
    # all-spells sibling of ``grants_creature_spells_uncounterable``;
    # read by ``CounterTargetSpell._is_uncounterable``.
    grants_spells_uncounterable: bool = False
    # 'Other creatures you control have "Ward—Pay N life."' (Hexing
    # Squelcher): each creature this static ``applies_to`` gains an
    # additional ward instance costing N life. Summed with the printed
    # ``ward_life_cost`` by ``cube_creatures_rmc.effective_ward_life_cost``.
    grants_ward_life_cost: int = 0
    # "... attacks each combat if able." (Dragon's Rage Channeler's
    # delirium clause): every creature this static ``applies_to`` is
    # forced into the attack declaration while able
    # (``designations.forced_attackers``).
    must_attack_each_combat: bool = False
    # "<Types> your opponents control enter tapped." (Spider-Woman):
    # card types (``"Artifact"``, ``"Creature"``) of permanents entering
    # under an opponent's control that enter tapped.
    opponents_enter_tapped_types: tuple = ()
    # "Abilities you activate that aren't mana abilities cost {N} less to
    # activate. This effect can't reduce the mana in that cost to less
    # than one mana." (Zirda) — ``cube_creatures_rmc.activation_cost_reduction``.
    activation_cost_reduction: int = 0
    # "Once during each of your turns, you may cast a permanent spell
    # with mana value N or less from your graveyard." (Lurrus): N, or -1.
    graveyard_permanent_cast_mv_max: int = -1
    # 'Creatures you control have "Whenever this creature becomes the
    # target of a spell or ability, <effect>. This ability triggers only
    # N times each turn."' (Nadu): the granted trigger's effect, stacked
    # by ``cube_creatures_rmc.fire_becomes_target``; N (0 = no cap).
    granted_becomes_target_effect: "Effect | None" = None
    granted_trigger_limit_per_turn: int = 0
    # ─── Modifier-grammar slots (additive; PR D consumers and later) ───
    # Optional structural condition gating ``applies_to``. When
    # present, evaluated against ``(state, source.controller_id,
    # chosen_targets=None)``. The source of truth for every
    # "as long as" rider on the static surface — printed conditions
    # like "as long as you control an artifact" or "during your turn"
    # ride here as ``ControlPermanent(IsArtifact())`` /
    # ``DuringYourTurn()`` rather than per-field boolean flags. ``None``
    # (default) is "no condition" and the gate short-circuits to
    # always-on.
    condition: "Condition | None" = None
    # Optional CR 614 replacement effect carried by this static.
    # Collected by :func:`mtgz_ir.effects.modifiers
    # ._collect_replacement_effects` when ``StaticAbility`` is
    # exposed via the printing's static-abilities surface (PR-D
    # migrations move from per-printing boolean flags to native
    # ``ReplacementEffect`` declarations carried here). Unused
    # today — empty for every existing card.
    replacement_effect: "ReplacementEffect | None" = None
    # Optional CR 613 continuous-effect declaration carried by this
    # static. Renamed from the original ``ContinuousEffect`` to
    # ``ContinuousModifier`` to avoid colliding with the pre-existing
    # ``mtgz_ir.game.ContinuousEffect`` (the runtime registry-entry
    # shape — every active layered effect lives in
    # ``GameState.continuous_effects`` as one of those). The PR-D
    # continuous-effect dispatcher walks every active
    # ``StaticAbility`` with this slot populated and folds the
    # ``ContinuousModifier.payload`` into the matching object's
    # effective state at the declared layer. Unused today — empty
    # for every existing card.
    continuous_effect: "ContinuousModifier | None" = None
    # "gets +N/+M for each <X> you control": when True the
    # ``power_bonus`` / ``toughness_bonus`` are PER UNIT of
    # ``condition.multiplier(state, controller)`` (a variable-amount
    # condition such as ``PerControlled(IsArtifact())``) instead of
    # flat. Read by ``game._static_bonuses``. Karn, Scion of Urza's
    # Construct token.
    bonus_per_condition_count: bool = False
    # "Skip your draw step." (Necropotence, Yawgmoth's Bargain): the
    # source's controller skips their draw step while this is active
    # (``roto_turns.skips_draw_step``, consulted by ``turn.next_step``).
    skips_your_draw_step: bool = False

    def __post_init__(self) -> None:
        if not isinstance(self.scope, StaticScope):
            raise TypeError(f"StaticAbility.scope must be a StaticScope, got {self.scope!r}")

    def applies_to(self, source, target, state: "GameState | None" = None) -> bool:
        """True if this ability, on ``source``, currently buffs ``target``."""
        from mtgz_ir.game import is_creature_now

        kind = self.scope.kind
        # Creatures only (except the card-type scope, which can name a
        # noncreature type), under the source's controller.
        if not is_creature_now(target):
            if kind is not ScopeKind.CARD_TYPE_YOU_CONTROL:
                return False
        if target.controller_id != source.controller_id:
            return False
        # Spacecraft tier gate — source needs enough charge counters.
        if self.tier_threshold > 0 and source.charge_counters < self.tier_threshold:
            return False
        # Optional structural condition gate. Evaluated against
        # ``(state, source.controller_id)``; the source of truth for
        # every "as long as" rider on the static surface today. ``None``
        # (default) is "no condition" and short-circuits to the existing
        # always-on behaviour.
        if self.condition is not None:
            if state is None:
                return False
            # A source-aware condition ("cards exiled with this
            # creature" -- Keen-Eyed Curator) reads the static's source.
            source_check = getattr(self.condition, "matches_for_source", None)
            if source_check is not None:
                if not source_check(state, source):
                    return False
            elif not self.condition.matches(
                state, source.controller_id, chosen_targets=None,
            ):
                return False
        if kind is ScopeKind.SELF:
            return target.id == source.id
        if kind is ScopeKind.OTHER_CREATURES_YOU_CONTROL:
            return target.id != source.id
        if kind is ScopeKind.CREATURES_YOU_CONTROL:
            return True
        if kind is ScopeKind.CREATURES_YOU_CONTROL_WITH_COUNTER:
            return target.plus_one_counters > 0
        if kind is ScopeKind.OTHER_TAPPED_LEGENDARY_CREATURES_YOU_CONTROL:
            return (
                target.id != source.id
                and target.tapped
                and "Legendary" in target.printing.supertypes
            )
        if kind is ScopeKind.SUBTYPE_YOU_CONTROL:
            # "[Other ]<Subtype>s you control ..." (Magda's Dwarf lord,
            # Sliver lords). Read effective_subtypes so a creature granted
            # the subtype by a layer-4 effect (Conspiracy-style) is picked
            # up; ``state`` is optional in this signature, and without it
            # we fall back to the printing to keep the static probe usable
            # in card-list contexts.
            if self.scope.other and target.id == source.id:
                return False
            if state is not None:
                from mtgz_ir.game import effective_subtypes
                return self.scope.subtype in effective_subtypes(state, target)
            return self.scope.subtype in target.printing.subtypes
        if kind is ScopeKind.CARD_TYPE_YOU_CONTROL:
            card_type = self.scope.card_type
            if card_type == "Creature":
                return is_creature_now(target)
            if state is not None:
                from mtgz_ir.game import has_card_type
                return has_card_type(state, target, card_type)
            return card_type in target.printing.card_types
        raise AssertionError(f"unhandled scope kind {kind!r}")  # pragma: no cover

    def describe(self) -> str:
        who = self.scope.describe()
        parts: list[str] = []
        if self.power_bonus or self.toughness_bonus:
            sign_p = "+" if self.power_bonus >= 0 else ""
            sign_t = "+" if self.toughness_bonus >= 0 else ""
            parts.append(
                f"get {sign_p}{self.power_bonus}/{sign_t}{self.toughness_bonus}"
            )
        if self.granted_keywords:
            kws = ", ".join(sorted(self.granted_keywords))
            parts.append(f"have {kws}")
        return f"{who} {' and '.join(parts)}." if parts else f"{who} (no effect)."



@dataclass(frozen=True)
class TriggeredAbility:
    """An ability that fires on a specific event.

    Two ways to specify what fires this ability:

    - **String-key style** (legacy): ``trigger`` is a string like
      ``"etb"`` / ``"landfall"`` / ``"dies"`` and the engine's
      per-event ``_fire_*`` functions hardcode-match it. Every static
      card today uses this path.
    - **Predicate style** (new): ``event`` is an ``Event`` enum value
      and ``predicate`` filters which firings of that event the
      ability cares about (subject type, controller, counters, etc.).
      The centralized dispatcher walks every triggered ability and
      fires the ones whose ``event`` matches and whose predicate
      evaluates true.

    Both styles coexist during migration. A given ability sets EXACTLY
    ONE of the two (the unused fields stay at their defaults). Phase 1
    only wires the ENTERS event family on the predicate path; the rest
    of the trigger taxonomy stays on the string-key path until later
    phases migrate them.

    ``requires_void`` is True for EoE "Void —" abilities, which only fire when
    ``is_void_active(state)`` (a nonland permanent left the battlefield this
    turn or a spell was warped this turn).
    """

    trigger: str = ""
    effect: "Effect | None" = None
    requires_void: bool = False
    # "This ability triggers only once each turn." When True, the firing
    # site records (source_id, ability_index) in state.once_per_turn_fired
    # and suppresses subsequent fires within the same turn. Reset at
    # cleanup.
    once_per_turn: bool = False
    # EoE Spacecraft tier gate: ability is active only when the source has
    # ``tier_threshold`` or more charge counters. 0 = always on. Mirrors
    # ``StaticAbility.tier_threshold``. Consumed by should_fire_trigger.
    tier_threshold: int = 0
    # Predicate-style trigger fields. ``event`` is an ``Event`` enum
    # value (None means string-key style); ``predicate`` is the filter
    # over event subjects, game state, and the source object. When
    # ``event`` is set, the engine's centralized dispatcher fires this
    # ability whenever the named event happens AND ``predicate``
    # evaluates true (or the predicate is None, which is treated as
    # ``Always``). String-key fields (``trigger``) are ignored on the
    # predicate path.
    event: "Event | None" = None
    predicate: "Predicate | None" = None
    # DSK Rooms: when this triggered ability lives on a Room half,
    # this is the half-name (matches a key in
    # ``CardPrinting.room_halves``). While the half is locked (not in
    # ``unlocked_halves``), the ability is inert and won't fire.
    # ``None`` for non-Room triggers — always active.
    room_half: "str | None" = None
    # True for abilities that function only while their source card is
    # in its owner's graveyard (Sword of the Meek's "Whenever a 1/1
    # creature you control enters, you may return this card from your
    # graveyard …"). ``actions.dispatch_event`` scans graveyards for these
    # and never fires them from the battlefield.
    functions_in_graveyard: bool = False

    def __post_init__(self) -> None:
        if self.event is not None and self.trigger:
            raise ValueError(
                "TriggeredAbility cannot specify both `trigger` (string-key "
                "style) and `event` (predicate style); pick one."
            )

    def matches_event(
        self,
        event: "Event",
        subjects: "Mapping[str, object]",
        source: "object",
        state: "object",
    ) -> bool:
        """Predicate-style match check.

        Returns True iff this ability is on the predicate path
        (``self.event is not None``), the event matches, and the
        predicate evaluates true (or is unset, meaning ``Always``).
        Returns False for string-key abilities — those fire via the
        legacy per-event ``_fire_*`` paths.
        """
        if self.event is None:
            return False
        if self.event != event:
            return False
        if self.predicate is None:
            return True
        return self.predicate.matches(state, subjects, source)

    def describe(self) -> str:
        # Predicate-style abilities (``event`` is set, ``trigger`` is
        # empty) get a coarse human prefix derived from the event +
        # whatever predicate clauses we can recognize; the
        # printing-level oracle text is still the source of truth, this
        # is just for log/UI display. Fine-grained shapes
        # (Burgeoning's "an opponent plays a land" vs The Endstone's
        # "you play a land") aren't structurally distinguishable from
        # describe() alone since we'd need source context — the
        # PLAYS_LAND prefix uses the broad "an opponent plays a land"
        # form to keep parity with the legacy key it migrated from
        # (the only PLAYS_LAND-rider that exposes describe() in tests
        # today is Burgeoning, which is the opponent-form).
        if self.event is not None and not self.trigger:
            from mtgz_ir.effects.object_predicates import IsArtifact
            from mtgz_ir.effects.state_predicates import SpellsCastThisTurn
            from mtgz_ir.effects.predicates import (
                Controlled, OPPONENT, StateBound, Subject, YOU,
            )
            event_prefix_map = {
                "enters": "When ~ enters the battlefield",
                "dies": "When ~ dies",
                "leaves_battlefield": "When ~ leaves the battlefield",
                "becomes_tapped": "Whenever ~ becomes tapped",
                "attacks": "Whenever ~ attacks",
                "dealt_damage": "Whenever ~ is dealt damage",
                "is_cast": "Whenever you cast a spell",
                "is_sacrificed": "Whenever you sacrifice another permanent",
                "plays_land": "Whenever an opponent plays a land",
                "discards_card": "Whenever an opponent discards a card",
                "counters_added": (
                    "Whenever you put one or more +1/+1 counters on a "
                    "creature you control"
                ),
                "beginning_of_upkeep": "At the beginning of your upkeep",
                "beginning_of_combat": (
                    "At the beginning of combat on your turn"
                ),
                "beginning_of_end_step": (
                    "At the beginning of your end step"
                ),
                "beginning_of_precombat_main": (
                    "At the beginning of your first main phase"
                ),
                "beginning_of_postcombat_main": (
                    "At the beginning of your second main phase"
                ),
                # Phase 9 added these. The DEALS_COMBAT_DAMAGE prefix
                # uses the broad self-form ("Whenever ~ deals combat
                # damage to a player") to match Illvoi Infiltrator's
                # printed text — the broader observer variants
                # (Starwinder's "a creature you control deals…",
                # Contested War Zone's "a creature deals combat damage
                # to you") are structurally distinct on the predicate
                # but indistinguishable here without source context, so
                # the self-form is the default and per-card describe
                # rides on the printed oracle text rather than this
                # prefix. Same trade-off as the PLAYS_LAND prefix above.
                "deals_combat_damage": (
                    "Whenever ~ deals combat damage to a player"
                ),
                "blocks": (
                    "Whenever ~ blocks a creature with flying"
                ),
            }
            # IS_CAST + IsArtifact("spell") → "Whenever you cast an
            # artifact spell" (Pinnacle Emissary). IS_CAST +
            # SpellsCastThisTurn(n=2) → "Whenever you cast your second
            # spell each turn".
            if self.event.value == "is_cast" and self.predicate is not None:
                from mtgz_ir.effects.predicates import And

                def _leaves(p):
                    if isinstance(p, And):
                        return _leaves(p.left) + _leaves(p.right)
                    return [p]

                leaves = _leaves(self.predicate)
                if any(
                    isinstance(lf, StateBound)
                    and isinstance(lf.inner, SpellsCastThisTurn)
                    and lf.inner.n == 2
                    for lf in leaves
                ):
                    prefix = (
                        "Whenever you cast your second spell each turn"
                    )
                elif any(
                    isinstance(lf, Subject)
                    and lf.slot == "spell"
                    and isinstance(lf.inner, IsArtifact)
                    for lf in leaves
                ):
                    prefix = "Whenever you cast an artifact spell"
                else:
                    prefix = event_prefix_map.get(
                        self.event.value, self.event.value,
                    )
            elif self.event.value == "saga_chapter" and hasattr(
                self.predicate, "n",
            ):
                prefix = f"Chapter {self.predicate.n}"
            elif self.event.value == "class_level_gained" and hasattr(
                self.predicate, "n",
            ):
                prefix = f"When this Class becomes level {self.predicate.n}"
            else:
                prefix = event_prefix_map.get(
                    self.event.value, self.event.value,
                )
            head = f"{prefix}, {self.effect.describe()}."
            if self.requires_void:
                return "Void — " + head
            return head
        prefix = {
            "etb": "When ~ enters the battlefield",
            "beginning_of_end_step": "At the beginning of your end step",
            "beginning_of_combat": "At the beginning of combat on your turn",
            "beginning_of_upkeep": "At the beginning of your upkeep",
            "dies": "When ~ dies",
            "landfall": "Landfall — Whenever a land you control enters",
            "becomes_tapped": "Whenever ~ becomes tapped",
            "enchanted_permanent_becomes_tapped": (
                "Whenever enchanted permanent becomes tapped"
            ),
            "second_spell_each_turn": (
                "Whenever you cast your second spell each turn"
            ),
            "artifact_enters_you_control": (
                "Whenever another artifact you control enters"
            ),
            "creature_or_artifact_enters_you_control": (
                "Whenever another creature or artifact you control enters"
            ),
            "kavu_you_control_enters": (
                "Whenever this creature or another Kavu you control enters"
            ),
            "attacks": "Whenever ~ attacks",
            "leaves_battlefield": "When ~ leaves the battlefield",
            "nontoken_artifact_enters_you_control": (
                "Whenever a nontoken artifact you control enters"
            ),
            "creature_you_control_with_counter_dies": (
                "Whenever a creature you control with a +1/+1 counter on "
                "it dies"
            ),
            "nontoken_creature_you_control_with_counter_dies": (
                "Whenever a nontoken creature you control with a +1/+1 "
                "counter on it dies"
            ),
            "sacrifice_creature_or_artifact": (
                "Whenever you sacrifice another creature or artifact"
            ),
            "combat_damage_to_player": (
                "Whenever this creature deals combat damage to a player"
            ),
            "your_creature_combat_damage_to_player": (
                "Whenever a creature you control deals combat damage "
                "to a player"
            ),
            "your_counter_creature_combat_damage_to_player_aggregated": (
                "Whenever one or more creatures you control with +1/+1 "
                "counters on them deal combat damage to a player"
            ),
            "creature_deals_combat_damage_to_you": (
                "Whenever a creature deals combat damage to you"
            ),
            "another_creature_you_control_dies_or_exiled": (
                "Whenever another creature you control dies or is put "
                "into exile"
            ),
            "counters_added_to_your_creature": (
                "Whenever you put one or more +1/+1 counters on a "
                "creature you control"
            ),
            "opponent_plays_land": "Whenever an opponent plays a land",
        }.get(self.trigger, self.trigger)
        head = f"{prefix}, {self.effect.describe()}."
        if self.requires_void:
            return "Void — " + head
        return head


