"""Activated-ability grammar.

An activated-ability line of the form ``{cost}: <effect>.`` where
``cost`` is one of:

  * One or more brace-wrapped mana symbols (``{1}{R}``, ``{2}``, …) —
    pure mana cost.
  * ``{T}`` — pure tap cost (no mana).
  * ``{T}, {mana}`` — tap plus a mana cost. The two pieces are
    comma-separated.
  * ``Sacrifice this creature`` / ``Sacrifice this artifact`` —
    pure sacrifice-self cost (the engine field ``sacrifice_self``
    covers both wordings; the only difference is which oracle phrase
    Scryfall uses depending on the source's card type).
  * ``{T}, Sacrifice this creature`` (or reversed) — tap plus
    sacrifice-self.
  * ``{mana}, Sacrifice this creature`` (or reversed) — mana plus
    sacrifice-self.
  * ``Pay N life`` — pure pay-life cost. Mana Confluence-style.
  * ``{T}, Pay N life`` (or reversed) — tap plus pay-life.
  * ``{mana}, Pay N life`` (or reversed) — mana plus pay-life.
  * ``Sacrifice another creature or artifact`` — pure
    sacrifice-another-creature-or-artifact cost. Matches Umbral
    Collar Zealot's ``Sacrifice another creature or artifact:
    Surveil 1.`` activated ability.
  * ``Remove a +1/+1 counter from a creature you control`` — pure
    counter-removal cost (Sunstar Chaplain pays this with mana
    today; the standalone form is accepted for symmetry with the
    other atoms). Maps to
    ``ActivationCost.remove_plus_one_counter_from_your_creature``.
  * Three-piece comma-separated combinations, order-insensitive.
    Each piece must classify as one of the recognized atoms (mana,
    tap, sacrifice-self, sacrifice-another-creature-or-artifact,
    sacrifice-another-artifact-or-land, sacrifice-a-food,
    sacrifice-a-land, pay-life, remove-+1/+1-counter). At most one
    mana piece, one tap, one sacrifice-flavor, one pay-life, and one
    counter-removal — anything else (duplicates, unknown atoms,
    4+ pieces) returns ``None``.
    Naturally observed three-piece shapes in EoE are
    ``{N}, {T}, Sacrifice this artifact`` (Nutrient Block,
    Dubious Delicacy, All-Fates Scroll, Thaumaton Torpedo),
    ``{N}, {T}, Sacrifice a Food`` (Ragost),
    ``{N}, {T}, Sacrifice another artifact or land``
    (Slagdrill Scrapper), and ``{N}, {T}, Sacrifice a land``
    (Kavaron, Memorial World back face).

``<effect>`` is a single sentence parsed via the unified
``_parse_effect`` after lowercase + period stripping. Cost shapes
not on the supported list ("tap another permanent", 4+ comma
pieces, exile-from-graveyard, tap-X-other, etc.) cause the line to
fall through and the card to be skipped. The
"Remove a +1/+1 counter from a creature you control" cost atom IS
supported via the bag-merge classifier; multi-target counter-removal
("remove N counters from <subset>") and counter-removal phrases
that target the source itself are still out of scope.

After a successful parse, the resulting ``ActivatedAbility`` is
inspected: if its effect is a mana ability (e.g. ``AddManaToPool``
producing mana with no targets), the ability is rebuilt with
``is_mana_ability=True`` so the engine bypasses the stack at
activation time per CR 605.

The ``Activate only as a sorcery.`` rider has two surface shapes:

  * **Inline** — ``{cost}: <effect>. Activate only as a sorcery.`` is a
    single oracle line ending in the rider. ``_parse_activated_ability_line``
    strips the trailing rider and parses the remainder as a normal
    activation, returning an ``ActivatedAbility(..., sorcery_speed=True)``.
    This is the only shape EoE actually uses.
  * **Standalone** — a separate ``Activate only as a sorcery.`` line that
    attaches to the immediately preceding activated ability on the same
    card. Recognized by ``_is_sorcery_rider_line`` and handled by the
    creature builder, not here. Currently unused by EoE but kept for
    completeness.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, replace

from mtgz_ir.cards.base import (
    ActivatedAbility,
    ActivationCost,
    AddManaToPool,
    CostReduction,
    Sequence,
    parse_mana_cost,
)
from mtgz_ir.effects.generic import (
    AddChargeCounterManaOfChosenColor,
    AddManaInAnyCombination,
    AddManaOfColorPerCount,
    AddNManaOfChosenColor,
    AddOneManaOfColorAmongControlled,
    AddOneManaOfImprintedColors,
    AddOneOfManaCombinations,
)
from mtgz_ir.effects.generic.enchantment_mechanics import (
    AddOneManaSacrificedLandCouldProduce,
)
from mtgz_ir.cards.parser.effects import _parse_effect
from mtgz_ir.effects.conditions import (
    AttackedWithThisTurn,
    Condition,
    ControlPermanent,
    DeliriumActive,
    HandSizeExactly,
    PerControlled,
    PerDifferentlyNamedLand,
)
from mtgz_ir.effects.object_predicates import (
    HasCounter,
    HasSubtype,
    IsCreature,
    IsLegendary,
)


# The effect may end in a quoted ability whose closing quote follows
# the period (Reckoner Bankbuster's Pilot token: ``… with "This token
# crews Vehicles as though its power were 2 greater."``).
_ACTIVATED_ABILITY_LINE = re.compile(
    r"^([^:]+): (.+\.\"?)$"
)
_WATERBEND_PREFIX = "Waterbend "

# A bare mana cost is one or more brace-wrapped mana symbols, no commas.
_MANA_COST_ONLY = re.compile(r"^(\{[^}]+\})+$")

# Pure sacrifice-self cost pieces. Scryfall uses "Sacrifice this
# creature" on creatures and "Sacrifice this artifact" on
# artifacts (and "Sacrifice this token" inside reminder text on
# token-creating cards). Both wordings map to the same engine
# field ``ActivationCost.sacrifice_self`` — the source's card
# type at activation time picks the legal interpretation. We do
# NOT recognize "Sacrifice this token" here: it appears only
# inside parenthesized reminder text and is handled by the
# token printing definition, not by oracle-text parsing.
_SACRIFICE_SELF_PIECES = frozenset({
    "Sacrifice this creature",
    "Sacrifice this artifact",
    # Lands' self-reference (Horizon Canopy's "{1}, {T}, Sacrifice this
    # land: Draw a card.", Wasteland, the fetch lands).
    # (Also Rain of Filth's granted "Sacrifice this land: Add {B}.")
    "Sacrifice this land",
    # Enchantments' self-reference (Momentum Breaker; roto P6).
    "Sacrifice this enchantment",
})

# "Exile this land" / "Exile this creature" / "Exile this artifact" —
# exile-self cost (Plaza of Heroes uses "Exile this land"). The exact
# Scryfall oracle phrase depends on the source's card type; all map to
# ``ActivationCost.exile_self``.
_EXILE_SELF_PIECES = frozenset({
    "Exile this land",
    "Exile this creature",
    "Exile this artifact",
})

# "Tap an untapped permanent you control" — Gene Pollinator's mana
# ability cost atom. Exact literal from Scryfall oracle text.
_TAP_PERMANENT_YOU_CONTROL_PIECE = "Tap an untapped permanent you control"

# "Exile an artifact card from your graveyard" — Rust Harvester's
# activated ability cost atom. Exact literal from Scryfall oracle text.
_EXILE_ARTIFACT_FROM_GRAVEYARD_PIECE = (
    "Exile an artifact card from your graveyard"
)
_DISCARD_A_CARD_PIECE = "Discard a card"

# Pure sacrifice-another-creature-or-artifact cost piece. The exact
# literal phrase from Umbral Collar Zealot's
# ``Sacrifice another creature or artifact: Surveil 1.`` Stored as a
# single literal because (a) Scryfall always prints this exact
# wording and (b) the engine has a dedicated cost field
# ``ActivationCost.sacrifice_another_creature_or_artifact`` for it.
_SACRIFICE_ANOTHER_CREATURE_OR_ARTIFACT_PIECE = (
    "Sacrifice another creature or artifact"
)

# Other "sacrifice another / sacrifice a <subtype>" pieces with their
# own engine fields. Each maps a recognized literal to the
# ``ActivationCost`` boolean field it sets. Mostly seen in three-piece
# costs alongside {T} and a mana piece.
_NAMED_SACRIFICE_PIECES: dict[str, str] = {
    "Sacrifice another artifact or land": (
        "sacrifice_another_artifact_or_land"
    ),
    "Sacrifice another artifact": "sacrifice_another_artifact",
    "Sacrifice a Food": "sacrifice_food",
    "Sacrifice a land": "sacrifice_a_land",
    "Sacrifice a Desert": "sacrifice_a_desert",
}

# Pay-life cost piece. ``Pay 1 life``, ``Pay 2 life``, etc. The
# integer is captured into the ``pay_life`` field on ActivationCost.
_PAY_LIFE_PIECE = re.compile(r"^Pay (\d+) life$")
_PAY_ENERGY_PIECE = re.compile(r"^Pay ((?:\{E\})+)$")

# "Remove a +1/+1 counter from a creature you control" cost piece —
# Sunstar Chaplain's activated ability prints exactly this literal as
# the second of two cost atoms (``{2}, Remove a +1/+1 counter from a
# creature you control: Tap target artifact or creature.``). Stored
# as a single literal because Scryfall always prints this exact
# wording and the engine has a dedicated boolean field
# ``ActivationCost.remove_plus_one_counter_from_your_creature`` for
# it (validated at activation time by ``legal_actions``: gates on at
# least one of the controller's creatures having a +1/+1 counter).
_REMOVE_PLUS_ONE_COUNTER_PIECE = (
    "Remove a +1/+1 counter from a creature you control"
)

# "Tap X untapped artifacts you control" — Secluded Starforge's
# second ability. A variable-count tap cost: X is the number of
# untapped artifacts the activator chooses to tap, and that same X
# feeds the +X/+0 pump via the resolving-x-value channel (see
# ``ActivationCost.tap_x_untapped_artifacts_you_control`` and the
# ``power_bonus_from_x`` pump). A literal because Scryfall prints this
# exact wording and the engine has a dedicated boolean field; only
# card in the pool with this shape (broader "tap X <permanent-class>"
# grammar deferred until a second example exists).
_TAP_X_UNTAPPED_ARTIFACTS_PIECE = "Tap X untapped artifacts you control"

# "Discard this card" — the hand-activated cost atom of Cycling /
# Channel (``ActivationCost.discard_self``). Only accepted by
# ``_parse_activated_ability_line(..., from_hand=True)``: an ability
# with this cost functions from its owner's hand and must be routed to
# ``CardPrinting.hand_activated_abilities``, never to the battlefield
# ``activated_abilities`` (whose activation path doesn't pay it).
_DISCARD_THIS_CARD_PIECE = "Discard this card"

# "Exile three cards from your graveyard" (Psychic Frog) —
# ``ActivationCost.exile_cards_from_graveyard``: the activator exiles
# any N cards of their choice from their own graveyard.
_EXILE_N_CARDS_FROM_GRAVEYARD_PIECE = re.compile(
    r"^Exile (\w+) cards from your graveyard$"
)
_EXILE_N_WORDS = {
    "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "seven": 7,
}

# "Exert this land" / "Exert this creature" (CR 701.39) —
# ``ActivationCost.exert_self`` (Arena of Glory).
_EXERT_SELF_PIECES = frozenset({
    "Exert this land", "Exert this creature", "Exert this artifact",
})

# "Remove a <kind> counter from this land" — a counter kind without a
# dedicated engine field (``GameObject.named_counters``; Gemstone Mine's
# "mining"). Kinds that DO have dedicated fields are rejected so they
# can never be silently split across two stores.
_REMOVE_NAMED_COUNTER_FROM_SELF_PIECE = re.compile(
    r"^Remove an? (?P<kind>[a-z]+) counter from this "
    r"(?:land|artifact|creature|enchantment|permanent)$"
)
_DEDICATED_COUNTER_KINDS = frozenset(
    {"charge", "time", "stun", "loyalty", "lore", "luck"}
)

# "Sacrifice a <Subtype>" — Retrofitter Foundry ("Sacrifice a Servo",
# "Sacrifice a Thopter"). Capitalized single-word subtypes only; the
# lowercase card-type forms ("Sacrifice a land") and the subtypes with
# their own dedicated fields (Food, Desert) are matched earlier.
_SACRIFICE_A_SUBTYPE_PIECE = re.compile(r"^Sacrifice an? ([A-Z][a-z]+)$")

# "Sacrifice a nontoken artifact" — Thopter Foundry.
_SACRIFICE_NONTOKEN_ARTIFACT_PIECE = "Sacrifice a nontoken artifact"

# "Sacrifice a <card type>" — lowercase card-type forms without a
# dedicated field (Goblin Bombardment's "Sacrifice a creature"). Maps to
# ``ActivationCost.sacrifice_a_permanent_type``. "land" keeps its
# dedicated ``sacrifice_a_land`` field (matched earlier).
_SACRIFICE_A_PERMANENT_TYPE_PIECE = re.compile(
    r"^Sacrifice an? (creature|artifact|enchantment|planeswalker)$"
)

# "Remove a/N <kind> counter(s) from this <permanent>" — self counter
# removal (Staff of the Storyteller, Wishclaw Talisman, Reckoner
# Bankbuster, Pentad Prism).
_REMOVE_COUNTERS_FROM_SELF_PIECE = re.compile(
    r"^Remove (a|an|one|two|three|\d+) ([a-z]+) counters? from this "
    r"(?:artifact|creature|Vehicle|Equipment|permanent)$"
)
_COST_CARDINALS: dict[str, int] = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}

# "Sacrifice N nonland permanents" — Bolas's Citadel (ten).
_SACRIFICE_N_NONLAND_PERMANENTS_PIECE = re.compile(
    r"^Sacrifice (two|three|four|five|six|seven|eight|nine|ten|\d+) "
    r"nonland permanents$"
)

# "Discard your hand" — Lion's Eye Diamond's cost atom. Maps to
# ``ActivationCost.discard_hand``; the whole hand (possibly empty) is
# discarded when the cost is paid.
_DISCARD_HAND_PIECE = "Discard your hand"


@dataclass(frozen=True)
class _CostPiece:
    """Tagged classification of a single comma-separated cost piece.

    Exactly one of the per-kind fields is populated. ``kind`` names
    which: ``"mana"`` → ``mana_str`` set; ``"tap"`` → no payload;
    ``"sacrifice_self"`` → no payload; ``"pay_life"`` → ``amount``
    set; ``"named_sac"`` → ``field`` set to the ``ActivationCost``
    attribute name; ``"remove_plus_one_counter"`` → no payload (sets
    the ``remove_plus_one_counter_from_your_creature`` flag at build
    time). The classifier returns ``None`` for any piece that doesn't
    match a recognized atom; the caller treats that as a hard skip.
    """

    kind: str
    mana_str: str = ""
    amount: int = 0
    field: str = ""
    # ``kind == "extra"``: a composable cost atom for
    # ``ActivationCost.extra_costs`` (cube creatures bucket grammar,
    # ``creature_rmc_grammar.parse_rmc_cost_piece``).
    atom: object = None


def _classify_cost_piece(piece: str) -> _CostPiece | None:
    piece_class = _classify_builtin_cost_piece(piece)
    if piece_class is not None:
        return piece_class
    # Fallback: composable cost atoms (sacrifice N filtered permanents,
    # remove +1/+1 counters from this creature, tap untapped filtered
    # permanents, exile this card from your graveyard). Tried only when
    # no dedicated atom above claims the piece.
    from mtgz_ir.cards.parser.creature_rmc_grammar import parse_rmc_cost_piece

    atom = parse_rmc_cost_piece(piece)
    if atom is None:
        # Vintage Roto P3 cost atoms ("Pay X {E}").
        from mtgz_ir.cards.parser.roto_mana import (
            parse_roto_cost_piece as parse_roto_mana_cost_piece,
        )

        atom = parse_roto_mana_cost_piece(piece)
    if atom is None:
        # Roto P2 cost atoms (return this permanent to its owner's hand).
        from mtgz_ir.cards.parser.roto_zones import parse_roto_zones_cost_piece

        atom = parse_roto_zones_cost_piece(piece)
    if atom is not None:
        return _CostPiece(kind="extra", atom=atom)
    # Roto P1 cost pieces ("Remove a counter from this creature",
    # "Sacrifice this enchantment").
    from mtgz_ir.cards.parser.roto_stack import parse_roto_cost_piece

    roto = parse_roto_cost_piece(piece)
    if roto is not None:
        kind, atom = roto
        return _CostPiece(kind=kind, atom=atom)
    # Vintage Roto P5 cost atoms.
    from mtgz_ir.cards.parser.roto_board import (
        parse_roto_cost_piece as parse_roto_board_cost_piece,
    )

    return parse_roto_board_cost_piece(piece)


def _classify_builtin_cost_piece(piece: str) -> _CostPiece | None:
    """Classify a single cost piece (already stripped) into a
    ``_CostPiece``. Returns ``None`` if the piece doesn't match
    any recognized atom."""
    if piece == "{T}":
        return _CostPiece(kind="tap")
    if piece in _SACRIFICE_SELF_PIECES:
        return _CostPiece(kind="sacrifice_self")
    if piece in _EXILE_SELF_PIECES:
        return _CostPiece(kind="exile_self")
    if piece == _TAP_PERMANENT_YOU_CONTROL_PIECE:
        return _CostPiece(kind="tap_permanent_you_control")
    if piece == "Tap an untapped artifact you control":
        return _CostPiece(kind="tap_permanent_you_control", field="Artifact")
    if piece == _EXILE_ARTIFACT_FROM_GRAVEYARD_PIECE:
        return _CostPiece(kind="exile_artifact_from_graveyard")
    if piece == _DISCARD_A_CARD_PIECE:
        return _CostPiece(kind="discard_a_card")
    if piece == _SACRIFICE_ANOTHER_CREATURE_OR_ARTIFACT_PIECE:
        return _CostPiece(
            kind="named_sac",
            field="sacrifice_another_creature_or_artifact",
        )
    if piece in _NAMED_SACRIFICE_PIECES:
        return _CostPiece(
            kind="named_sac", field=_NAMED_SACRIFICE_PIECES[piece]
        )
    if piece == _REMOVE_PLUS_ONE_COUNTER_PIECE:
        return _CostPiece(kind="remove_plus_one_counter")
    if piece == _TAP_X_UNTAPPED_ARTIFACTS_PIECE:
        return _CostPiece(kind="tap_x_untapped_artifacts")
    if piece == _DISCARD_THIS_CARD_PIECE:
        return _CostPiece(kind="discard_self")
    if piece in _EXERT_SELF_PIECES:
        return _CostPiece(kind="exert_self")
    named = _REMOVE_NAMED_COUNTER_FROM_SELF_PIECE.match(piece)
    if named is not None and named.group("kind") not in _DEDICATED_COUNTER_KINDS:
        return _CostPiece(kind="remove_named_counter", field=named.group("kind"))
    exile_n = _EXILE_N_CARDS_FROM_GRAVEYARD_PIECE.match(piece)
    if exile_n is not None:
        n = _EXILE_N_WORDS.get(exile_n.group(1))
        if n is None:
            return None
        return _CostPiece(kind="exile_n_from_graveyard", amount=n)
    energy_match = _PAY_ENERGY_PIECE.match(piece)
    if energy_match is not None:
        return _CostPiece(
            kind="pay_energy", amount=energy_match.group(1).count("{E}"),
        )
    # Dedicated-field kinds ("Remove a charge counter from this
    # artifact" — Pentad Prism) and multi-counter / Vehicle forms go
    # through ``_REMOVE_COUNTERS_FROM_SELF_PIECE`` below, whose cost is
    # paid via ``game.remove_counters`` (dedicated fields stay the only
    # store for their kinds).
    if piece == _DISCARD_HAND_PIECE:
        return _CostPiece(kind="discard_hand")
    if piece == _SACRIFICE_NONTOKEN_ARTIFACT_PIECE:
        return _CostPiece(kind="sacrifice_nontoken_artifact")
    type_match = _SACRIFICE_A_PERMANENT_TYPE_PIECE.match(piece)
    if type_match is not None:
        return _CostPiece(
            kind="sacrifice_a_permanent_type", field=type_match.group(1),
        )
    n_match = _SACRIFICE_N_NONLAND_PERMANENTS_PIECE.match(piece)
    if n_match is not None:
        token = n_match.group(1)
        return _CostPiece(
            kind="sacrifice_n_nonland_permanents",
            amount=int(token) if token.isdigit() else _COST_CARDINALS[token],
        )
    sub_match = _SACRIFICE_A_SUBTYPE_PIECE.match(piece)
    if sub_match is not None:
        return _CostPiece(kind="sacrifice_subtype", field=sub_match.group(1))
    counter_match = _REMOVE_COUNTERS_FROM_SELF_PIECE.match(piece)
    if counter_match is not None:
        token = counter_match.group(1)
        amount = int(token) if token.isdigit() else _COST_CARDINALS[token]
        return _CostPiece(
            kind="remove_counters_from_self",
            field=counter_match.group(2), amount=amount,
        )
    pay_life_match = _PAY_LIFE_PIECE.match(piece)
    if pay_life_match is not None:
        return _CostPiece(
            kind="pay_life", amount=int(pay_life_match.group(1))
        )
    if _MANA_COST_ONLY.match(piece) is not None:
        # Validate by parsing — malformed brace expressions like
        # ``{2 R}`` slip past the regex but blow up here.
        try:
            parse_mana_cost(piece)
        except ValueError:
            return None
        return _CostPiece(kind="mana", mana_str=piece)
    return None


def _build_cost_from_pieces(
    pieces: list[_CostPiece],
) -> ActivationCost | None:
    """Combine classified cost pieces into an ``ActivationCost``.

    Validates that no kind appears more than once: a cost like
    ``{2}, {2}, {T}`` (two mana pieces) or ``Sacrifice this
    creature, Sacrifice a Food`` (two sacrifice flavors) is
    nonsensical and returns ``None``. Sacrifice-self and
    named-sacrifice are treated as members of the same
    "sacrifice-flavor" exclusion group, since a single activation
    cost can only spell out one sacrifice clause. The
    "remove a +1/+1 counter from a creature you control" piece is
    its own exclusion group — distinct from sacrifice (it doesn't
    move a permanent to the graveyard) and distinct from pay-life,
    so a single cost may pair it with mana / tap / sacrifice / etc.
    but not list it twice.
    """
    mana_str: str | None = None
    tap_self = False
    sacrifice_self = False
    exile_self = False
    tap_permanent_you_control = False
    exile_artifact_from_graveyard = False
    pay_life = 0
    named_sac_field: str | None = None
    sacrifice_seen = False
    remove_plus_one_counter = False
    tap_x_untapped_artifacts = False
    discard_self = False
    remove_named_counter = ""
    exert_self = False
    discard_a_card = False
    pay_energy = 0
    discard_hand = False
    sacrifice_subtype = ""
    sacrifice_nontoken_artifact = False
    sacrifice_a_permanent_type = ""
    remove_counter_kind = ""
    remove_counter_amount = 0
    sacrifice_n_nonland = 0
    extra_costs: list = []
    exile_n_from_graveyard = 0
    tap_permanent_type = ""
    for p in pieces:
        if p.kind == "extra":
            extra_costs.append(p.atom)
            continue
        if p.kind == "mana":
            if mana_str is not None:
                return None
            mana_str = p.mana_str
        elif p.kind == "tap":
            if tap_self:
                return None
            tap_self = True
        elif p.kind == "sacrifice_self":
            if sacrifice_seen:
                return None
            sacrifice_self = True
            sacrifice_seen = True
        elif p.kind == "exile_self":
            if sacrifice_seen or exile_self:
                return None
            exile_self = True
            sacrifice_seen = True  # exile-self and sac-self are mutually exclusive
        elif p.kind == "tap_permanent_you_control":
            if tap_permanent_you_control:
                return None
            tap_permanent_you_control = True
            tap_permanent_type = p.field or ""
        elif p.kind == "exile_artifact_from_graveyard":
            if exile_artifact_from_graveyard:
                return None
            exile_artifact_from_graveyard = True
        elif p.kind == "named_sac":
            if sacrifice_seen:
                return None
            named_sac_field = p.field
            sacrifice_seen = True
        elif p.kind == "pay_life":
            if pay_life:
                return None
            pay_life = p.amount
        elif p.kind == "remove_plus_one_counter":
            if remove_plus_one_counter:
                return None
            remove_plus_one_counter = True
        elif p.kind == "tap_x_untapped_artifacts":
            if tap_x_untapped_artifacts:
                return None
            tap_x_untapped_artifacts = True
        elif p.kind == "discard_self":
            if discard_self:
                return None
            discard_self = True
        elif p.kind == "exert_self":
            if exert_self:
                return None
            exert_self = True
        elif p.kind == "remove_named_counter":
            if remove_named_counter:
                return None
            remove_named_counter = p.field
        elif p.kind == "discard_a_card":
            if discard_a_card:
                return None
            discard_a_card = True
        elif p.kind == "pay_energy":
            if pay_energy:
                return None
            pay_energy = p.amount
        elif p.kind == "discard_hand":
            if discard_hand:
                return None
            discard_hand = True
        elif p.kind == "sacrifice_subtype":
            if sacrifice_seen:
                return None
            sacrifice_subtype = p.field
            sacrifice_seen = True
        elif p.kind == "sacrifice_nontoken_artifact":
            if sacrifice_seen:
                return None
            sacrifice_nontoken_artifact = True
            sacrifice_seen = True
        elif p.kind == "sacrifice_a_permanent_type":
            if sacrifice_seen:
                return None
            sacrifice_a_permanent_type = p.field
            sacrifice_seen = True
        elif p.kind == "sacrifice_n_nonland_permanents":
            if sacrifice_seen:
                return None
            sacrifice_n_nonland = p.amount
            sacrifice_seen = True
        elif p.kind == "remove_counters_from_self":
            if remove_counter_kind:
                return None
            remove_counter_kind = p.field
            remove_counter_amount = p.amount
        elif p.kind == "exile_n_from_graveyard":
            if exile_n_from_graveyard:
                return None
            exile_n_from_graveyard = p.amount
        else:  # pragma: no cover — defensive; classifier never emits this
            return None
    kwargs: dict = {}
    if mana_str is not None:
        kwargs["mana"] = parse_mana_cost(mana_str)
        kwargs["mana_str"] = mana_str
    if tap_self:
        kwargs["tap_self"] = True
    if sacrifice_self:
        kwargs["sacrifice_self"] = True
    if exile_self:
        kwargs["exile_self"] = True
    if tap_permanent_you_control:
        kwargs["tap_permanent_you_control"] = True
        if tap_permanent_type:
            kwargs["tap_permanent_type"] = tap_permanent_type
    if exile_artifact_from_graveyard:
        kwargs["exile_artifact_from_graveyard"] = True
    if pay_life:
        kwargs["pay_life"] = pay_life
    if named_sac_field is not None:
        kwargs[named_sac_field] = True
    if remove_plus_one_counter:
        kwargs["remove_plus_one_counter_from_your_creature"] = True
    if tap_x_untapped_artifacts:
        kwargs["tap_x_untapped_artifacts_you_control"] = True
    if remove_named_counter:
        kwargs["remove_named_counter_from_self"] = remove_named_counter
    if exert_self:
        kwargs["exert_self"] = True
    if discard_self:
        if (
            tap_self or sacrifice_seen or remove_named_counter or exert_self
            or remove_counter_kind
        ):
            # A card in hand can't also be tapped / sacrificed.
            return None
        kwargs["discard_self"] = True
    if discard_a_card:
        kwargs["discard_a_card"] = True
    if pay_energy:
        kwargs["pay_energy"] = pay_energy
    if discard_hand:
        kwargs["discard_hand"] = True
    if sacrifice_subtype:
        kwargs["sacrifice_subtype"] = sacrifice_subtype
    if sacrifice_nontoken_artifact:
        kwargs["sacrifice_nontoken_artifact"] = True
    if sacrifice_a_permanent_type:
        kwargs["sacrifice_a_permanent_type"] = sacrifice_a_permanent_type
    if sacrifice_n_nonland:
        kwargs["sacrifice_n_nonland_permanents"] = sacrifice_n_nonland
    if remove_counter_kind:
        kwargs["remove_counters_from_self_kind"] = remove_counter_kind
        kwargs["remove_counters_from_self_amount"] = remove_counter_amount
    if extra_costs:
        kwargs["extra_costs"] = tuple(extra_costs)
    if exile_n_from_graveyard:
        kwargs["exile_cards_from_graveyard"] = exile_n_from_graveyard
    return ActivationCost(**kwargs)


def _parse_activation_cost(cost_str: str) -> ActivationCost | None:
    """Parse the cost segment of an activated-ability line into an
    ``ActivationCost``.

    Single-piece (no comma) shapes:

    * Pure mana: ``{1}{R}`` etc. → ``ActivationCost(mana=…, mana_str=…)``.
    * Tap-only: ``{T}`` → ``ActivationCost(tap_self=True)``.
    * Pure sacrifice-self: ``Sacrifice this creature`` /
      ``Sacrifice this artifact`` →
      ``ActivationCost(sacrifice_self=True)``.
    * Pure sacrifice-another-creature-or-artifact: ``Sacrifice another
      creature or artifact`` → ``ActivationCost(
      sacrifice_another_creature_or_artifact=True)``.
    * Pure pay-life: ``Pay 1 life`` → ``ActivationCost(pay_life=1)``.

    Multi-piece (comma-separated) shapes — order-insensitive: each
    piece is independently classified by ``_classify_cost_piece`` and
    the bag is merged via ``_build_cost_from_pieces``. Recognized
    atoms are mana / tap / sacrifice-self / pay-life /
    remove-+1/+1-counter / one of the named-sacrifice flavors
    (``Sacrifice another artifact or land``,
    ``Sacrifice another artifact``, ``Sacrifice a Food``,
    ``Sacrifice a land``, ``Sacrifice a Desert``,
    ``Sacrifice another creature or artifact``).

    Two-piece combinations: tap + mana, tap + sacrifice-self, mana +
    sacrifice-self, tap + pay-life, mana + pay-life (in either
    order).

    Three-piece combinations: any valid bag with at most one of each
    kind. The naturally observed shapes in EoE are
    ``{N}, {T}, Sacrifice this artifact`` (and variants with a
    different sacrifice phrase: ``Sacrifice a Food``,
    ``Sacrifice another artifact or land``, ``Sacrifice a land``).
    Three-piece ``{T}, Pay N life, Sacrifice this creature`` and
    other tap/pay-life/sac combos are also accepted by the same
    bag-merge logic.

    Anything else returns ``None``: 4+ pieces, an unrecognized atom,
    a duplicate (e.g. two mana pieces), or a malformed mana
    expression. The caller skips the card on a ``None`` return.
    """
    cost_str = cost_str.strip()
    if "," not in cost_str:
        # Single-piece path. Empty string → None (defensive; the
        # activated-ability line regex won't normally hand us one).
        if not cost_str:
            return None
        piece = _classify_cost_piece(cost_str)
        if piece is None:
            return None
        return _build_cost_from_pieces([piece])
    raw_pieces = [p.strip() for p in cost_str.split(",")]
    # Any number of pieces, each a recognized atom at most once (Susur
    # Secundi's ``{1}{B}, {T}, Pay 2 life, Sacrifice a creature``).
    classified: list[_CostPiece] = []
    for raw in raw_pieces:
        piece = _classify_cost_piece(raw)
        if piece is None:
            return None
        classified.append(piece)
    return _build_cost_from_pieces(classified)


# ``this <permanent type> `` subject ahead of ``deals`` in an activated
# ability's body — the source itself (see ``_parse_activated_ability_line``).
_SELF_SUBJECT_DEALS = re.compile(
    r"^this (?:enchantment|artifact|creature|land|permanent) (?=deals )"
)


def _normalize_activated_effect_phrase(phrase: str) -> str:
    """Strip the trailing period and lowercase the first letter so the
    activated-ability surface form (``Put a +1/+1 counter on target
    creature.``) matches the unified normalized grammar."""
    phrase = phrase.strip()
    if phrase.endswith("."):
        phrase = phrase[:-1]
    if phrase and phrase[0].isupper():
        phrase = phrase[0].lower() + phrase[1:]
    return phrase


# Inline sorcery-speed rider: ``Activate only as a sorcery.`` glued to
# the end of the same line as the activation, with a single space
# separator after the effect's terminating period. This is the only
# shape EoE actually uses (e.g. Intrepid Tenderfoot's
# ``{3}: Put a +1/+1 counter on this creature. Activate only as a sorcery.``).
_INLINE_SORCERY_RIDER_SUFFIX = " Activate only as a sorcery."


# Inline once-per-game rider: ``Activate only once.`` glued to the end
# of the same line as the activation, with a single space separator
# after the effect's terminating period. DSK Haunted Screen's third
# activation is the only EoE/DSK printing today. The rider is stripped
# here and lifts to ``activate_only_once=True`` on the resulting
# ``ActivatedAbility`` — the legality gate in ``legal_actions`` then
# filters out re-activations once the source's
# ``has_activated_once_only_ability`` flag is latched at activation
# time.
_INLINE_ACTIVATE_ONLY_ONCE_RIDER_SUFFIX = " Activate only once."


# Inline activation-restriction rider: ``Activate only if <condition>.``
# glued to the end of the activation line (CR 602.5b). The condition
# phrase is translated by ``_parse_activate_only_if_condition``; an
# unknown phrase rejects the whole line (never a silently-dropped
# gate). Library of Alexandria: "{T}: Draw a card. Activate only if you
# have exactly seven cards in hand."
_INLINE_ACTIVATE_ONLY_IF_RIDER = re.compile(r" Activate only if (.+)\.$")
_HAND_SIZE_EXACTLY_PHRASE = re.compile(
    r"^you have exactly (?P<n>\w+) cards? in hand$"
)
_ACTIVATE_ONLY_IF_NUMBER_WORDS: dict[str, int] = {
    "zero": 0, "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}
# Inline timing / condition riders that restrict WHEN an ability may be
# activated. Each is glued to the end of the activation line after the
# effect's terminating period and lifts to a field on the resulting
# ``ActivatedAbility`` / ``ActivationCost``:
#
# * ``Activate only as an instant.`` -> ``activate_only_as_instant``
#   (Lion's Eye Diamond).
# * ``Activate only during your turn.`` ->
#   ``activate_only_during_your_turn`` (Wishclaw Talisman).
# * ``Activate only if you control <N> or more artifacts.`` ->
#   ``ActivationCost.requires_own_artifact_count_at_least=N`` (Mox
#   Opal's Metalcraft; the same legal-actions gate Inventors' Fair's
#   land-builder wiring uses).
_INLINE_ACTIVATE_ONLY_AS_INSTANT_RIDER_SUFFIX = " Activate only as an instant."
_INLINE_ACTIVATE_ONLY_DURING_YOUR_TURN_RIDER_SUFFIX = (
    " Activate only during your turn."
)
_INLINE_ACTIVATE_ONLY_IF_CONTROL_N_ARTIFACTS_RIDER = re.compile(
    r" Activate only if you control (\d+|one|two|three|four|five|six|seven"
    r"|eight|nine|ten) or more artifacts\.$"
)
# ``Activate only if you control a creature with power <N> or greater.``
# -> ``ActivationCost.requires_own_creature_power_at_least=N`` (Fanatic
# of Rhonas's Ferocious mana ability; the legal-actions gate the utility
# lands' ``_PAID_EFFECT_RIDERS`` entry already uses).
_INLINE_ACTIVATE_ONLY_IF_CONTROL_CREATURE_POWER_RIDER = re.compile(
    r" Activate only if you control a creature with power (\d+|one|two"
    r"|three|four|five|six|seven|eight|nine|ten) or greater\.$"
)
# ``<Ability Word> — {cost}: <effect>.`` — an ability word (CR 207.2c:
# no rules meaning; Mox Opal's ``Metalcraft —``) or a flavor word in
# front of an activation. Stripped before the cost/effect split; the
# lookahead requires a ``{`` mana/tap symbol right after the dash so
# em-dashes elsewhere never match. ``Exhaust —`` is NOT an ability word
# but a keyword ability (CR 702.177: "Activate each exhaust ability only
# once") whose restriction isn't modeled, so it is never stripped — the
# line stays unparsed rather than losing the restriction.
_ACTIVATED_ABILITY_WORD_PREFIX = re.compile(
    r"^(?!Exhaust )[A-Z][A-Za-z]*(?: [A-Za-z]+)* — (?=\{)"
)
_RIDER_CARDINALS: dict[str, int] = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5,
    "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}


def _parse_activate_only_if_condition(phrase: str) -> "Condition | None":
    """Map the ``<condition>`` of an ``Activate only if <condition>.``
    rider to a ``Condition`` evaluated for the activator, or ``None``."""
    m = _HAND_SIZE_EXACTLY_PHRASE.match(phrase)
    if m is not None:
        n = _ACTIVATE_ONLY_IF_NUMBER_WORDS.get(m.group("n"))
        if n is None and m.group("n").isdigit():
            n = int(m.group("n"))
        if n is None:
            return None
        return HandSizeExactly(n)
    if phrase == (
        "there are four or more card types among cards in your graveyard"
    ):
        # Delirium (Shifting Woodland's copy ability).
        return DeliriumActive()
    # Vintage Roto P3 riders ("you control an artifact").
    from mtgz_ir.cards.parser.roto_mana import parse_activate_only_if_condition

    return parse_activate_only_if_condition(phrase)


# Inline cost-reduction rider for activated abilities:
# ``This ability costs {N} less to activate if <condition>.`` glued to
# the end of the activation line, with a single space separator after
# the effect's terminating period. The first EoE printing is Starport
# Security's ``{3}{W}, {T}: Tap another target creature. This ability
# costs {2} less to activate if you control a creature with a +1/+1
# counter on it.``
#
# The captured ``N`` is the digit reduction; the condition phrase
# (everything after ``activate if`` and before the trailing period)
# must appear in the closed ``_ACTIVATED_COST_REDUCTION_CONDITIONS``
# whitelist to produce a recognized rule. An unknown condition phrase
# falls through and the whole line stays unrecognized — never silently
# producing a no-op rule the engine ignores. The rider never appears
# without a condition (every EoE printing today gates the reduction
# on something), so the regex requires it.
_INLINE_COST_REDUCTION_RIDER = re.compile(
    r" This ability costs \{(\d+)\} less to activate if (.+)\.$"
)

# Variable-amount X-based cost-reduction rider for activated abilities:
# ``This ability costs {X} less to activate, where X is the number of
# differently named lands you control.``
# Mirrors the spell-side literal in
# ``mtgz_ir.cards.parser.cost_reduction._COST_REDUCTION_LINE_X_PER_
# DIFFERENTLY_NAMED_LAND`` but adapts the surface to the activated-
# ability wording ("to activate" instead of "to cast"). Survey Mechan
# is the only EoE printing with this exact surface; broader grammar
# deferred until a second activation with a varying piece ships.
# Maps to ``CostReduction(amount=1, condition=PerDifferentlyNamedLand
# ())`` — the engine's ``apply_cost_reductions`` multiplies the unit
# amount by the condition's ``multiplier`` (the count of distinctly
# named lands the controller controls at activation time), giving the
# correct X-based discount.
# Per-permanent cost-reduction rider: ``This ability costs {N} less to
# activate for each <noun> you control.`` — the Channel lands (Boseiju,
# Who Endures / Otawara, Soaring City: "for each legendary creature you
# control"). Maps to ``CostReduction(amount=N,
# condition=PerControlled(<pred>))``; the engine multiplies ``N`` by the
# count. The noun is looked up in ``_PER_CONTROLLED_COST_REDUCTION_NOUNS``
# (unknown noun rejects the line).
_INLINE_PER_CONTROLLED_COST_REDUCTION_RIDER = re.compile(
    r" This ability costs \{(\d+)\} less to activate for each (.+) you "
    r"control\.$"
)
_PER_CONTROLLED_COST_REDUCTION_NOUNS: dict[str, object] = {
    "legendary creature": IsCreature() & IsLegendary(),
}

_INLINE_X_COST_REDUCTION_RIDER_DIFFERENTLY_NAMED_LAND = (
    " This ability costs {X} less to activate, where X is the number "
    "of differently named lands you control."
)


# Surface phrase → structural ``Condition`` mapping for the activated-
# ability cost-reduction rider. Each phrase decomposes to a tree of
# ``ObjectPredicate`` atoms wrapped in a ``Condition`` variant; the
# engine consumes the ``Condition`` directly via the single-dispatch
# path in ``apply_cost_reductions`` / ``_evaluate_cost_reduction``.
# Mirrors the per-spell whitelist in
# ``mtgz_ir.cards.parser.cost_reduction`` but lives here because the
# activated-ability rider has its own surface shape (``... to activate
# if ...`` rather than ``... to cast if ...``) and condition vocabulary
# (no overlap with EoE printings observed today).
_ACTIVATED_COST_REDUCTION_CONDITIONS: dict[str, "Condition"] = {
    # Starport Security — "you control a creature with a +1/+1 counter
    # on it" decomposes to a ``ControlPermanent(...)`` over the
    # battlefield filtered to creatures with a ``+1/+1`` counter.
    "you control a creature with a +1/+1 counter on it": (
        ControlPermanent(IsCreature() & HasCounter("+1/+1"))
    ),
    # Thaumaton Torpedo — "you attacked with a Spacecraft this turn"
    # reads through the generalized ``Player.attacked_this_turn`` latch
    # (every declared attacker is appended; the ``Spacecraft`` filter
    # is applied at evaluation time by the condition).
    "you attacked with a Spacecraft this turn": (
        AttackedWithThisTurn(HasSubtype("Spacecraft"))
    ),
}


def _is_mana_ability_effect(effect) -> bool:
    """Return True iff ``effect`` is a mana ability for the purposes of
    CR 605: it produces mana and does not target.

    ``AddManaToPool`` covers fixed-color coins (``add {U}`` → ``("U", 1)``)
    and the at-resolution choose-color coin (``add one mana of any color``
    → ``("any", 1)``, used by All-Fates Scroll's ``{T}`` ability).

    ``AddChargeCounterManaOfChosenColor`` covers The Eternity Elevator's
    tier-gated ``{T}: Add X mana of any one color, where X is the number
    of charge counters on this Spacecraft.`` — X is determined at
    resolution time rather than parse time, but the ability still mints
    mana into the pool with no targets, satisfying CR 605's mana-ability
    criterion. Marking it ``is_mana_ability=True`` causes the engine to
    bypass the stack on activation, matching the printed behavior.

    ``AddNManaOfChosenColor`` covers the choose-one-of-N-colors mana
    abilities (Haunted Screen's ``{T}: Add {W} or {B}.`` and ``{T}, Pay
    1 life: Add {G}, {U}, or {R}.``). The controller picks one of the
    listed colors at resolution; like ``AddManaToPool`` the ability
    mints mana with no targets, satisfying CR 605.

    The other "permanent-color" / "land-could-produce" mana effects also
    belong here in principle but aren't yet emitted by the activated-ability
    grammar (each requires its own surface-form regex first).

    Effects that would be excluded if they ever appeared (defense in
    depth): anything reporting ``requires_target() == True`` is not a
    mana ability per the rules — even if it adds mana, the targeting
    forces the ability to use the stack. Today none of ``AddManaToPool``,
    ``AddChargeCounterManaOfChosenColor``, or ``AddNManaOfChosenColor``
    targets, but the explicit guard keeps future grammar additions
    honest.
    """
    # A Sequence headed by a mana production whose remaining steps don't
    # target is still a mana ability (CR 605.1a) — Gemstone Mine's
    # "Add one mana of any color. If there are no mining counters on
    # this land, sacrifice it."
    if (
        isinstance(effect, Sequence)
        and effect.effects
        and _is_mana_ability_effect(effect.effects[0])
        and not effect.requires_target()
    ):
        return True
    if isinstance(effect, _MANA_PRODUCING_EFFECTS):
        if effect.requires_target():
            return False
        return True
    # "Add {G}{G}. If you've completed a dungeon, add six {G} instead."
    # (Undermountain Adventurer): both branches produce mana.
    from mtgz_ir.effects.combinators import IfConditionElse

    if isinstance(effect, IfConditionElse):
        return (
            effect.else_effect is not None
            and _is_mana_ability_effect(effect.then_effect)
            and _is_mana_ability_effect(effect.else_effect)
        )
    # CR 605.1a: an activated ability that could add mana and doesn't
    # target is a mana ability even when it does other things too —
    # the Talismans' ``Add {U} or {B}. This artifact deals 1 damage to
    # you.`` is a ``Sequence`` of a mana primitive and a self-damage
    # rider, and still bypasses the stack.
    if isinstance(effect, Sequence):
        if any(e.requires_target() for e in effect.effects):
            return False
        return any(
            isinstance(e, _MANA_PRODUCING_EFFECTS) for e in effect.effects
        )
    return False


# Mana-producing effect classes recognized by ``_is_mana_ability_effect``.
_MANA_PRODUCING_EFFECTS: tuple[type, ...] = (
    AddManaToPool,
    AddChargeCounterManaOfChosenColor,
    AddManaOfColorPerCount,
    AddNManaOfChosenColor,
    AddOneOfManaCombinations,
    AddManaInAnyCombination,
    AddOneManaOfColorAmongControlled,
    AddOneManaOfImprintedColors,
    AddOneManaSacrificedLandCouldProduce,
)


def _parse_activated_ability_line(
    line: str, *, from_hand: bool = False,
) -> ActivatedAbility | None:
    """Return an ``ActivatedAbility`` for a ``{cost}: <effect>.`` line,
    or ``None`` if the line shape doesn't match, the cost shape isn't
    one of the supported pure-mana / tap / tap+mana forms, or the
    effect doesn't parse via the unified grammar.

    Also accepts the inline sorcery-speed rider variant
    ``{cost}: <effect>. Activate only as a sorcery.`` — the trailing
    rider is stripped, the remainder is parsed as a normal activation,
    and the resulting ability is marked ``sorcery_speed=True``.

    Inline cost-reduction rider variant
    ``{cost}: <effect>. This ability costs {N} less to activate if
    <condition>.`` is also accepted: the rider is stripped, the
    captured ``N`` and condition phrase are translated into a
    ``CostReduction`` attached to the ability's ``cost_reductions``
    tuple. An unknown condition phrase rejects the whole line so the
    card stays unclaimed rather than silently dropping the gate.

    If the parsed effect is a mana ability (see
    ``_is_mana_ability_effect``), the returned ``ActivatedAbility`` is
    marked ``is_mana_ability=True`` so the engine bypasses the stack
    on activation per CR 605.
    """
    stripped = _ACTIVATED_ABILITY_WORD_PREFIX.sub("", line.strip(), count=1)
    sorcery_speed = False
    if stripped.endswith(_INLINE_SORCERY_RIDER_SUFFIX):
        # Drop the rider but keep the effect's terminating period so
        # the standard ``{cost}: <effect>.`` shape still matches.
        stripped = stripped[: -len(_INLINE_SORCERY_RIDER_SUFFIX)]
        sorcery_speed = True
    activate_only_once = False
    if stripped.endswith(_INLINE_ACTIVATE_ONLY_ONCE_RIDER_SUFFIX):
        # Drop the rider but keep the effect's terminating period so
        # the standard ``{cost}: <effect>.`` shape still matches.
        # Disjoint with the sorcery rider above (no card prints both
        # ``Activate only as a sorcery.`` AND ``Activate only once.``
        # today), so the two checks operate independently.
        stripped = stripped[: -len(_INLINE_ACTIVATE_ONLY_ONCE_RIDER_SUFFIX)]
        activate_only_once = True
    activate_only_as_instant = False
    activate_only_during_your_turn = False
    requires_artifact_count = 0
    requires_creature_power = 0
    # The timing / condition riders are independent of each other and of
    # the sorcery / once riders above; strip whichever trail the line
    # (at most one of each).
    while True:
        if stripped.endswith(_INLINE_ACTIVATE_ONLY_AS_INSTANT_RIDER_SUFFIX):
            if activate_only_as_instant:
                return None
            stripped = stripped[
                : -len(_INLINE_ACTIVATE_ONLY_AS_INSTANT_RIDER_SUFFIX)
            ]
            activate_only_as_instant = True
            continue
        if stripped.endswith(
            _INLINE_ACTIVATE_ONLY_DURING_YOUR_TURN_RIDER_SUFFIX
        ):
            if activate_only_during_your_turn:
                return None
            stripped = stripped[
                : -len(_INLINE_ACTIVATE_ONLY_DURING_YOUR_TURN_RIDER_SUFFIX)
            ]
            activate_only_during_your_turn = True
            continue
        artifact_rider = _INLINE_ACTIVATE_ONLY_IF_CONTROL_N_ARTIFACTS_RIDER.search(
            stripped
        )
        if artifact_rider is not None:
            if requires_artifact_count:
                return None
            token = artifact_rider.group(1)
            requires_artifact_count = (
                int(token) if token.isdigit() else _RIDER_CARDINALS[token]
            )
            stripped = stripped[: artifact_rider.start()]
            continue
        power_rider = (
            _INLINE_ACTIVATE_ONLY_IF_CONTROL_CREATURE_POWER_RIDER.search(stripped)
        )
        if power_rider is not None:
            if requires_creature_power:
                return None
            token = power_rider.group(1)
            requires_creature_power = (
                int(token) if token.isdigit() else _RIDER_CARDINALS[token]
            )
            stripped = stripped[: power_rider.start()]
            continue
        break
    activate_only_if = None
    only_if_match = _INLINE_ACTIVATE_ONLY_IF_RIDER.search(stripped)
    if only_if_match is not None:
        activate_only_if = _parse_activate_only_if_condition(
            only_if_match.group(1).strip()
        )
        if activate_only_if is None:
            return None
        stripped = stripped[: only_if_match.start()]
    cost_reductions: tuple[CostReduction, ...] = ()
    # Variable-amount X-based differently-named-land rider — checked
    # BEFORE the numeric ``{N}`` rider because ``{X}`` can't be captured
    # by the ``\{(\d+)\}`` regex, so the two regexes are disjoint; the
    # literal check is an exact suffix match and runs in O(1).
    if stripped.endswith(_INLINE_X_COST_REDUCTION_RIDER_DIFFERENTLY_NAMED_LAND):
        cost_reductions = (
            CostReduction(
                amount=1,
                condition=PerDifferentlyNamedLand(),
            ),
        )
        stripped = stripped[
            : -len(_INLINE_X_COST_REDUCTION_RIDER_DIFFERENTLY_NAMED_LAND)
        ]
    per_match = _INLINE_PER_CONTROLLED_COST_REDUCTION_RIDER.search(stripped)
    if per_match is not None:
        pred = _PER_CONTROLLED_COST_REDUCTION_NOUNS.get(
            per_match.group(2).strip()
        )
        if pred is None:
            return None
        cost_reductions = (
            CostReduction(
                amount=int(per_match.group(1)),
                condition=PerControlled(pred),
            ),
        )
        stripped = stripped[: per_match.start()]
    rider_match = _INLINE_COST_REDUCTION_RIDER.search(stripped)
    if rider_match is not None:
        condition_phrase = rider_match.group(2).strip()
        condition = _ACTIVATED_COST_REDUCTION_CONDITIONS.get(
            condition_phrase
        )
        if condition is None:
            # Unknown condition phrase — reject the whole line rather
            # than silently dropping a gate the static registry does
            # honor. A future EoE printing with a new condition phrase
            # surfaces a parser miss until the whitelist is extended.
            return None
        cost_reductions = (
            CostReduction(
                amount=int(rider_match.group(1)),
                condition=condition,
            ),
        )
        # Strip the rider — the rider regex's leading literal space
        # comes RIGHT AFTER the effect's terminating period, so the
        # remaining prefix already ends with the period and the
        # standard ``{cost}: <effect>.`` shape still matches without
        # any further fix-up.
        stripped = stripped[: rider_match.start()]
    waterbend = stripped.startswith(_WATERBEND_PREFIX)
    if waterbend:
        # "Waterbend {N}: <effect>" -- the cost is {N}, payable with help
        # from untapped artifacts and creatures.
        stripped = stripped[len(_WATERBEND_PREFIX):]
    m = _ACTIVATED_ABILITY_LINE.match(stripped)
    if m is None:
        return None
    cost = _parse_activation_cost(m.group(1))
    if cost is None:
        return None
    if waterbend:
        if cost.mana is None or cost.tap_self:
            return None
        cost = replace(cost, waterbend=True)
    # ``from_hand`` lines (Cycling / Channel bodies) must carry the
    # "Discard this card" cost atom, and only they may: a battlefield
    # ability can't pay it.
    if cost.discard_self != from_hand:
        return None
    if requires_artifact_count:
        cost = replace(
            cost, requires_own_artifact_count_at_least=requires_artifact_count,
        )
    if requires_creature_power:
        cost = replace(
            cost, requires_own_creature_power_at_least=requires_creature_power,
        )
    phrase = _normalize_activated_effect_phrase(m.group(2))
    effect = _parse_effect(phrase)
    if effect is None and phrase.startswith("it deals "):
        # ``It deals N damage …`` — in an activated ability the pronoun
        # is the source (Pyrite Spellbomb's ``{R}, Sacrifice this
        # artifact: It deals 2 damage to any target.``). Retry without
        # it so the body reaches the same subject-less damage grammar
        # trigger bodies use (the creature-trigger normalizer strips
        # ``it `` the same way). Tried second so pronoun-bearing literal
        # bodies (Survey Mechan) keep matching first.
        effect = _parse_effect(phrase[len("it "):])
    if effect is None:
        # ``This <permanent> deals N damage …`` — the same source-subject
        # strip for a self-named subject (Goblin Bombardment's
        # ``Sacrifice a creature: This enchantment deals 1 damage to any
        # target.``). Only reached when the whole body failed to parse.
        subject = _SELF_SUBJECT_DEALS.match(phrase)
        if subject is not None:
            effect = _parse_effect(phrase[subject.end():])
    if effect is None:
        # A multi-sentence body whose first sentence carries an in-
        # sentence ``, then `` joiner (Wishclaw Talisman's "search your
        # library for a card, put it into your hand, then shuffle. An
        # opponent gains control of this artifact") defeats the compound
        # splitter's joiner precedence; retry sentence by sentence. Only
        # reached when the whole body failed to parse.
        from mtgz_ir.cards.parser.effects import (
            _parse_effect_pieces,
            _split_effect_sentences,
        )
        sentences = _split_effect_sentences(phrase)
        if len(sentences) >= 2:
            effect = _parse_effect_pieces(sentences)
    if effect is None:
        return None
    ability = ActivatedAbility(
        cost=cost, effect=effect, sorcery_speed=sorcery_speed,
        cost_reductions=cost_reductions,
        activate_only_once=activate_only_once,
        activate_only_if=activate_only_if,
        activate_only_as_instant=activate_only_as_instant,
        activate_only_during_your_turn=activate_only_during_your_turn,
    )
    if _is_mana_ability_effect(effect):
        ability = replace(ability, is_mana_ability=True)
    return ability


# A standalone ``Activate only as a sorcery.`` clause attaches to the
# immediately preceding activated ability on the same card and marks
# it sorcery-speed. The exact-match recognizer is intentionally narrow:
# any other rider phrasing falls through and causes the card to skip.
# EoE doesn't actually use this shape (the rider is always inline — see
# ``_INLINE_SORCERY_RIDER_SUFFIX`` above), but the standalone recognizer
# is kept in place as a harmless safety net for any future printings.
_SORCERY_RIDER_LINE = "Activate only as a sorcery."


def _is_sorcery_rider_line(line: str) -> bool:
    """Return True iff ``line`` is the standalone sorcery-speed rider
    ``Activate only as a sorcery.`` (whitespace-stripped, exact match)."""
    return line.strip() == _SORCERY_RIDER_LINE
