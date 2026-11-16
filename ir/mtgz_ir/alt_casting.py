"""Alternative and additional casting costs shared by the cast pipeline
(``actions._cast_spell``) and legal-move enumeration (``legal.py``).

Covers the generic keyword costs the PowerLSV cube's instants and
sorceries print:

* **Flashback** (CR 702.34) — cast from the graveyard for the printed
  flashback cost; the card is exiled when it leaves the stack.
* **Delve** (CR 702.66) — exile cards from your graveyard as you cast
  the spell; each pays for {1}.
* **Buyback** (CR 702.27) — optional additional cost; the spell returns
  to its owner's hand as it resolves.
* **"As an additional cost to cast this spell, sacrifice a <filter>."**
  (Tinker, Natural Order, Crop Rotation) — ``CardPrinting.
  additional_cost_sacrifice_filter``.
* **"As an additional cost to cast this spell, pay X life."** (Toxic
  Deluge) — ``CardPrinting.additional_cost_pay_x_life``.

Every piece is keyed off ``CardPrinting`` fields, so creature / other
permanent printings that carry the same keyword reuse the same path.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from mtgz_ir.cards import ManaCost

if TYPE_CHECKING:
    from mtgz_ir.game import GameObject, GameState


# ---------------------------------------------------------------------
# Additional sacrifice cost
# ---------------------------------------------------------------------


def permanent_matches_filter(state: "GameState", obj: "GameObject", card_filter) -> bool:
    """Test a battlefield permanent against a ``CardFilter``.

    Card types and supertypes read the printing, except ``Creature``
    which reads ``is_creature_now`` (an animated land is a creature;
    an impending Overlord is not). Color reads ``effective_colors``.
    """
    from mtgz_ir.game import effective_colors, has_card_type, is_creature_now

    pr = obj.printing
    if card_filter.any_types:
        ok = False
        for t in card_filter.any_types:
            if t == "Creature":
                if is_creature_now(obj):
                    ok = True
                    break
            elif t == "Permanent" or has_card_type(state, obj, t):
                ok = True
                break
        if not ok:
            return False
    for t in card_filter.excluded_types:
        if t == "Creature":
            if is_creature_now(obj):
                return False
        elif has_card_type(state, obj, t):
            return False
    if card_filter.max_mv is not None and pr.mana_cost.total > card_filter.max_mv:
        return False
    if card_filter.color is not None and card_filter.color not in effective_colors(state, obj):
        return False
    if card_filter.supertype is not None and card_filter.supertype not in pr.supertypes:
        return False
    if card_filter.subtype is not None:
        from mtgz_ir.game import effective_subtypes

        if card_filter.subtype not in effective_subtypes(state, obj):
            return False
    return True


def sacrifice_cost_options(
    state: "GameState", player_id: int, card: "GameObject",
) -> list["GameObject"]:
    """Permanents ``player_id`` controls that can pay ``card``'s
    additional sacrifice cost (empty when the card has none)."""
    f = card.printing.additional_cost_sacrifice_filter
    if f is None:
        return []
    return [
        o for o in state.battlefield
        if o.controller_id == player_id
        and permanent_matches_filter(state, o, f)
    ]


def additional_costs_payable(
    state: "GameState", player_id: int, card: "GameObject",
) -> bool:
    """Gate for the casting-substrate additional costs. A sacrifice
    cost needs at least one eligible permanent; "pay X life" is always
    payable (X may be 0)."""
    if card.printing.additional_cost_sacrifice_filter is not None:
        if not sacrifice_cost_options(state, player_id, card):
            return False
    n = card.printing.additional_cost_exile_from_graveyard
    if n and sum(
        1 for c in state.player_by_id(player_id).graveyard if c.id != card.id
    ) < n:
        return False
    return True


def pay_additional_sacrifice(
    state: "GameState", player_id: int, card: "GameObject",
) -> "GameObject | None":
    """Pay ``card``'s additional sacrifice cost (agent pick via
    ``choose_sacrifice``). Returns the sacrificed permanent, or ``None``
    when the card has no such cost."""
    options = sacrifice_cost_options(state, player_id, card)
    if not options:
        return None
    from mtgz_ir.turn import sacrifice

    p = state.player_by_id(player_id)
    chosen = options[0]
    if p.agent is not None:
        pick = p.agent.choose_sacrifice(state, player_id, [o.id for o in options])
        chosen = next((o for o in options if o.id == pick), options[0])
    state.log.append(
        f"additional cost: P{player_id} sacrifices {chosen.printing.name}"
    )
    sacrifice(state, chosen)
    return chosen


# ---------------------------------------------------------------------
# Pay X life
# ---------------------------------------------------------------------


def pay_x_life_upper_bound(state: "GameState", player_id: int, card: "GameObject") -> int:
    """Largest X offered for a "pay X life" additional cost.

    CR 119.4 lets a player pay any amount of life up to their total;
    paying their whole life total loses the game, so the offered range
    stops one short of it. When the spell's effect can say how much X
    is ever useful (``Effect.useful_x_cap``), larger X values are
    dominated and pruned (documented in ``docs/SEARCH_LIMITATIONS.md``).
    """
    p = state.player_by_id(player_id)
    bound = max(0, p.life - 1)
    eff = card.printing.spell_effect
    cap_fn = getattr(eff, "useful_x_cap", None)
    if cap_fn is not None:
        cap = cap_fn(state, player_id)
        if cap is not None:
            bound = min(bound, max(0, cap))
    return bound


def choose_pay_x_life(state: "GameState", player_id: int, card: "GameObject") -> int:
    """Ask the caster for X (``choose_x``) for a "pay X life" cost."""
    p = state.player_by_id(player_id)
    upper = pay_x_life_upper_bound(state, player_id, card)
    x = upper
    if p.agent is not None:
        x = p.agent.choose_x(state, player_id, card.printing.name, upper)
    x = max(0, min(upper, x))
    state.log.append(f"P{player_id} chooses to pay X={x} life for {card.printing.name}")
    return x


# ---------------------------------------------------------------------
# Delve
# ---------------------------------------------------------------------


def _delvable_cards(state: "GameState", player_id: int, card: "GameObject") -> list:
    p = state.player_by_id(player_id)
    return [c for c in p.graveyard if c.id != card.id]


def delve_affordability_cost(
    state: "GameState", player_id: int, card: "GameObject", cost: ManaCost,
) -> ManaCost:
    """Upper-bound cost for legality checks: delve as much generic mana
    as the graveyard allows (delve only ever makes a spell cheaper)."""
    if not card.printing.has_delve:
        return cost
    return cost.reduced(len(_delvable_cards(state, player_id, card)))


def pay_delve(
    state: "GameState", player_id: int, card: "GameObject", cost: ManaCost,
) -> ManaCost:
    """Exile cards from the caster's graveyard to pay generic mana.

    The number of cards is chosen via ``choose_x`` (bounded by the
    generic portion and the graveyard size). Cards without flashback
    are exiled first, oldest first — which specific cards get delved is
    not offered as a choice (``docs/SEARCH_LIMITATIONS.md``). Returns
    the remaining cost.
    """
    if not card.printing.has_delve:
        return cost
    from mtgz_ir.game import Zone

    pool = _delvable_cards(state, player_id, card)
    upper = min(cost.generic, len(pool))
    if upper <= 0:
        return cost
    p = state.player_by_id(player_id)
    n = upper
    if p.agent is not None:
        n = p.agent.choose_x(state, player_id, f"{card.printing.name} (delve)", upper)
    n = max(0, min(upper, n))
    if n == 0:
        return cost
    ordered = sorted(
        pool, key=lambda c: c.printing.flashback_cost is not None,
    )
    exiled = ordered[:n]
    for c in exiled:
        p.graveyard.remove(c)
        c.zone = Zone.EXILE
        p.exile.append(c)
    state.log.append(
        f"P{player_id} delves {n} card(s) for {card.printing.name}"
    )
    return cost.reduced(n)


# ---------------------------------------------------------------------
# Buyback
# ---------------------------------------------------------------------


def cost_plus(cost: ManaCost, extra: "ManaCost | None") -> ManaCost:
    """Merge an additional mana cost (buyback / kicker) into ``cost``."""
    if extra is None:
        return cost
    merged: dict[str, int] = dict(cost.colored)
    for color, n in extra.colored:
        merged[color] = merged.get(color, 0) + n
    return ManaCost(
        generic=cost.generic + extra.generic,
        colored=tuple(sorted(merged.items())),
        x_pips=cost.x_pips + extra.x_pips,
        hybrid=cost.hybrid + extra.hybrid,
        phyrexian=cost.phyrexian + extra.phyrexian,
    )


# ---------------------------------------------------------------------
# Flashback
# ---------------------------------------------------------------------


def flashback_cost(card: "GameObject") -> "ManaCost | None":
    """The flashback cost ``card`` can currently be cast for from its
    owner's graveyard, or ``None``: the printed flashback cost, else a
    granted "flashback cost equal to its mana cost" (the Flashback
    card)."""
    if card.printing.flashback_cost is not None:
        return card.printing.flashback_cost
    if card.flashback_granted_until_eot:
        return card.printing.mana_cost
    return None


# ---------------------------------------------------------------------
# Pitch alternative cost + free-cast permissions
# ---------------------------------------------------------------------


def pitch_options(state: "GameState", player_id: int, card: "GameObject") -> list:
    """Other hand cards that can pay ``card``'s "exile a <filter> card
    from your hand rather than pay this spell's mana cost"."""
    f = card.printing.alt_cost_exile_from_hand
    if f is None:
        return []
    p = state.player_by_id(player_id)
    options = [c for c in p.hand if c.id != card.id and f.matches_card(state, c)]
    # "Exile two <filter> cards" (Commandeer): payable only with enough.
    if len(options) < card.printing.alt_cost_exile_from_hand_count:
        return []
    return options


def free_cast_permission(state: "GameState", player_id: int, card: "GameObject") -> bool:
    """Is ``card`` castable without paying its mana cost with no further
    cost: Weftwalking's first-spell grant or the card's own
    ``free_cast_condition`` (Once Upon a Time)?"""
    p = state.player_by_id(player_id)
    if (
        state.players[state.active_player_idx].id == player_id
        and p.spells_cast_this_turn == 0
        and any(o.printing.grants_first_spell_each_turn_free for o in state.battlefield)
    ):
        return True
    cond = card.printing.free_cast_condition
    if cond is not None and cond.matches(state, player_id):
        return True
    # Vintage Roto P3 static permissions (Omniscience, Aluren).
    from mtgz_ir.effects.generic.roto_mana import static_free_cast_permission

    return static_free_cast_permission(state, player_id, card)


def pay_pitch_cost(state: "GameState", player_id: int, card: "GameObject") -> None:
    """Exile the pitched card (picked through ``choose_discard`` — the
    "which hand card do I give up" decision)."""
    from mtgz_ir.game import Zone, reveal_to_all

    options = pitch_options(state, player_id, card)
    if not options:  # pragma: no cover - legal_actions gates
        raise ValueError(f"no card to exile for {card.printing.name}")
    p = state.player_by_id(player_id)
    for _ in range(card.printing.alt_cost_exile_from_hand_count):
        chosen = options[0]
        if p.agent is not None:
            pick = p.agent.choose_discard(state, player_id, [c.id for c in options])
            chosen = next((c for c in options if c.id == pick), options[0])
        options.remove(chosen)
        p.hand.remove(chosen)
        chosen.zone = Zone.EXILE
        p.exile.append(chosen)
        reveal_to_all(state, chosen)
        state.log.append(
            f"P{player_id} exiles {chosen.printing.name} to cast "
            f"{card.printing.name} without paying its mana cost"
        )


# ---------------------------------------------------------------------
# Splice onto <subtype> (CR 702.47)
# ---------------------------------------------------------------------


def offer_splices(
    state: "GameState", player_id: int, card: "GameObject", cost: ManaCost,
    spell_kinds: "frozenset[str]",
) -> tuple[ManaCost, tuple]:
    """While casting ``card``, offer each other hand card whose
    ``splice_onto`` matches one of the spell's subtypes (each at most
    once). A splice is offered only if the grown total cost stays
    payable and the spliced effect's targets (if any) exist; the caster
    decides via ``choose_yes_no`` and picks the spliced targets with
    ``choose_target``. Returns ``(cost including splice costs,
    ((printing, targets), ...))``."""
    from mtgz_ir.actions import can_pay_cost

    p = state.player_by_id(player_id)
    subtypes = set(card.printing.subtypes)
    if not subtypes or p.agent is None:
        return cost, ()
    spliced: list = []
    for other in list(p.hand):
        pr = other.printing
        if other.id == card.id or pr.splice_onto not in subtypes:
            continue
        if pr.splice_cost is None or pr.spell_effect is None:
            continue
        grown = cost_plus(cost, pr.splice_cost)
        if not can_pay_cost(state, player_id, grown, spell_kinds, sac_for_mana=True):
            continue
        eff = pr.spell_effect
        splice_targets: tuple = ()
        if eff.requires_target():
            legal = eff.legal_targets(state, player_id)
            if not legal:
                continue
        if not p.agent.choose_yes_no(
            state, player_id, f"Splice {pr.name} onto {card.printing.name}?",
        ):
            continue
        if eff.requires_target():
            pick = p.agent.choose_target(state, player_id, legal)
            if pick is None or pick not in legal:
                pick = legal[0]
            splice_targets = (pick,)
        from mtgz_ir.game import reveal_to_all

        reveal_to_all(state, other)
        cost = grown
        spliced.append((pr, splice_targets))
        state.log.append(
            f"P{player_id} splices {pr.name} onto {card.printing.name} "
            f"for {pr.splice_cost_str}"
        )
    return cost, tuple(spliced)


# ---------------------------------------------------------------------
# Escape (CR 702.138)
# ---------------------------------------------------------------------


def escape_terms(
    state: "GameState", player_id: int, card: "GameObject",
) -> "tuple[ManaCost, int] | None":
    """``(mana cost, other-cards-to-exile)`` for escaping ``card`` from
    ``player_id``'s graveyard, or ``None`` if it has no escape.

    The card's own printed escape wins; otherwise a permanent
    ``player_id`` controls with ``grants_graveyard_escape_exile_count``
    (Underworld Breach) grants escape to each nonland card in that
    player's graveyard for its mana cost plus exiling N other cards. A
    card without a mana cost can't be cast this way (its escape cost
    would include an unpayable mana cost). A negative second element is
    the card-type form of the exile part -- "with N or more card types
    among them" (Nethergoyf; ``CardPrinting.escape_exile_card_types``) --
    understood by ``escape_payable`` / ``pay_escape_exile``."""
    pr = card.printing
    if pr.escape_cost is not None:
        if pr.escape_exile_card_types:
            # "... with N or more card types among them": a nonzero
            # marker so the cast pipeline pays the exile part.
            return pr.escape_cost, -pr.escape_exile_card_types
        return pr.escape_cost, pr.escape_exile_count
    if pr.is_land or not pr.mana_cost_str:
        return None
    if card not in state.player_by_id(player_id).graveyard:
        return None
    for o in state.battlefield:
        n = o.printing.grants_graveyard_escape_exile_count
        if n and o.controller_id == player_id:
            return pr.mana_cost, n
    return None


def escape_payable(state: "GameState", player_id: int, card: "GameObject") -> bool:
    """Enough *other* cards in the graveyard for the escape exile part."""
    terms = escape_terms(state, player_id, card)
    if terms is None:
        return False
    p = state.player_by_id(player_id)
    if terms[1] < 0:
        # Nethergoyf: other cards with -terms[1] or more card types.
        from mtgz_ir.effects.card_types import count_card_types

        return count_card_types(
            c for c in p.graveyard if c.id != card.id
        ) >= -terms[1]
    others = sum(1 for c in p.graveyard if c.id != card.id)
    return others >= terms[1]


def pay_escape_exile(
    state: "GameState", player_id: int, card: "GameObject",
    count: int | None = None,
) -> None:
    """Exile ``count`` (default ``escape_exile_count``) other graveyard
    cards (cards without flashback / escape first, oldest first — which
    cards is not offered as a choice, like delve;
    ``docs/SEARCH_LIMITATIONS.md``). A negative ``count`` is the
    card-type form: exile the fewest other cards with ``-count`` or
    more card types among them."""
    from mtgz_ir.game import Zone

    if count is None:
        count = card.printing.escape_exile_count
    p = state.player_by_id(player_id)
    pool = [c for c in p.graveyard if c.id != card.id]
    pool.sort(key=lambda c: (
        c.printing.flashback_cost is not None or c.printing.escape_cost is not None
    ))
    if count < 0:
        # "Exile any number of other cards from your graveyard with N or
        # more card types among them" (Nethergoyf): the fewest cards,
        # greedily, that bring in a new card type each.
        from mtgz_ir.effects.card_types import card_types_of

        need, seen, picked = -count, set(), []
        while len(seen) < need:
            best = max(
                (c for c in pool if c not in picked),
                key=lambda c: len(card_types_of(c) - seen), default=None,
            )
            if best is None or not (card_types_of(best) - seen):
                break
            picked.append(best)
            seen |= card_types_of(best)
        pool, count = picked, len(picked)
    for c in pool[:count]:
        p.graveyard.remove(c)
        c.zone = Zone.EXILE
        p.exile.append(c)
    state.log.append(
        f"P{player_id} exiles {count} other card(s) "
        f"to escape {card.printing.name}"
    )
