"""Effect-phrase recognizers added for the PowerLSV cube artifact
bucket (mana rocks, utility artifacts, Equipment, Vehicles).

``_parse_artifact_effect_phrase`` is consulted by the unified
``effects._parse_effect`` dispatcher (just before compound splitting),
so every recognizer here is available to triggered, activated and
spell bodies alike. Each recognizer is a parameterized regex over one
printed shape; recursion into inner effect phrases goes back through
``effects._parse_effect`` (imported lazily to avoid the import cycle).
"""

from __future__ import annotations

import re

from mtgz_ir.effects.base import Effect
from mtgz_ir.effects.generic import (
    AddManaInAnyCombination,
    AddNManaOfChosenColor,
    AddOneManaOfColorAmongControlled,
    AddOneManaOfImprintedColors,
    FlipCoinIfLost,
    ImprintCardFromHand,
    SourceDealsDamageToController,
    UntapSelf,
)


_CARDINALS: dict[str, int] = {
    "a": 1, "an": 1, "one": 1, "two": 2, "three": 3, "four": 4,
    "five": 5, "six": 6, "seven": 7, "eight": 8, "nine": 9, "ten": 10,
}
_CARDINAL = r"(?:\d+|one|two|three|four|five|six|seven|eight|nine|ten)"


def _amount(token: str) -> int:
    return int(token) if token.isdigit() else _CARDINALS[token]


# Source-subject surface for "this <permanent>" self references that
# the parser normalizes to (activated bodies keep "this artifact";
# trigger bodies may use "it").
_SELF_SUBJECT = (
    r"(?:this (?:artifact|creature|land|enchantment|permanent|Equipment"
    r"|Vehicle)|it)"
)

# "Add N mana of any one color." (Black Lotus, Lion's Eye Diamond,
# Coveted Jewel). -> ``AddNManaOfChosenColor(amount=N)``: one color
# chosen, N coins of it. N >= 2 — the single-coin wording is printed
# as "one mana of any color" (a different recognizer).
_ADD_N_MANA_OF_ANY_ONE_COLOR = re.compile(
    rf"^add (?!one )({_CARDINAL}) mana of any one color$"
)

# "Add N mana in any combination of <colors>." — each coin's color is
# chosen independently. ``<colors>`` is either the word ``colors``
# (WUBRG) or a brace list ``{U}, {B}, and/or {R}``. N may be ``X``
# (the X paid for the ability).
_ADD_N_MANA_IN_ANY_COMBINATION = re.compile(
    rf"^add ({_CARDINAL}|X) mana in any combination of "
    r"(colors|(?:\{[WUBRG]\}, )+(?:and/or )?\{[WUBRG]\}"
    r"|\{[WUBRG]\} and/or \{[WUBRG]\})$"
)

# "This artifact deals N damage to you." — self-damage rider. The
# subject is optional because trigger bodies arrive with the ``it``
# pronoun already stripped by ``_normalize_creature_effect_phrase``.
_SOURCE_DEALS_N_DAMAGE_TO_YOU = re.compile(
    rf"^(?:{_SELF_SUBJECT} )?deals ({_CARDINAL}) damage to you$"
)

# "Untap this artifact." — untap the source permanent.
_UNTAP_SELF = re.compile(
    r"^untap this (?:artifact|creature|land|permanent|Vehicle)$"
)

# "Flip a coin. If you lose the flip, <effect>." — Mana Crypt.
_FLIP_COIN_IF_LOSE = re.compile(
    r"^flip a coin\. If you lose the flip, (.+)$"
)


# "Add one mana of any color among [legendary] <types> you control." —
# Mox Amber (legendary creatures and planeswalkers). ``permanents`` is
# not accepted here: Plaza of Heroes' "legendary permanents" wording is
# claimed earlier by ``effects._ADD_ANY_MANA_AMONG_GROUP`` (the same
# ``AddOneManaOfColorAmongControlled`` class).
_ADD_ONE_MANA_AMONG_TYPES = re.compile(
    r"^add one mana of any color among (legendary )?"
    r"(creatures and planeswalkers|creatures|planeswalkers|artifacts"
    r"|enchantments) you control$"
)
_TYPE_GROUPS: dict[str, tuple[str, ...]] = {
    "creatures and planeswalkers": ("Creature", "Planeswalker"),
    "creatures": ("Creature",),
    "planeswalkers": ("Planeswalker",),
    "artifacts": ("Artifact",),
    "enchantments": ("Enchantment",),
}

# "You may exile <card descriptor> from your hand." — the Imprint ETB
# -> ``ImprintCardFromHand`` linking the exiled card to the source.
# Chrome Mox's "a nonartifact, nonland card" keeps its type-exclusion
# form; any other descriptor ("an instant card with mana value 2 or
# less", Isochron Scepter) goes through the card-descriptor grammar.
_MAY_EXILE_CARD_FROM_HAND_IMPRINT = re.compile(
    r"^you may exile (?P<desc>an? .+? card(?: with mana value \d+ or less)?)"
    r" from your hand$"
)
_NON_TYPES_ONLY_DESCRIPTOR = re.compile(
    r"^a ((?:non[a-z]+, )*non[a-z]+) card$"
)

# "Add one mana of any of the exiled card's colors." — Chrome Mox.
_ADD_ONE_MANA_OF_EXILED_CARDS_COLORS = re.compile(
    r"^add one mana of any of the exiled card's colors$"
)


def _parse_artifact_effect_phrase(phrase: str) -> Effect | None:
    """Return an ``Effect`` for a normalized effect phrase matching one
    of the artifact-bucket shapes, else ``None``."""
    from mtgz_ir.cards.parser.effects import _parse_effect

    m = _ADD_N_MANA_OF_ANY_ONE_COLOR.match(phrase)
    if m is not None:
        return AddNManaOfChosenColor(amount=_amount(m.group(1)))
    m = _ADD_N_MANA_IN_ANY_COMBINATION.match(phrase)
    if m is not None:
        colors = m.group(2)
        options = (
            ("W", "U", "B", "R", "G") if colors == "colors"
            else tuple(re.findall(r"\{([WUBRG])\}", colors))
        )
        if m.group(1) == "X":
            return AddManaInAnyCombination(options=options, amount_is_x=True)
        if colors == "colors":
            # "Add N mana in any combination of colors" is the base
            # grammar's shape (N ``any`` coins, Manamorphose).
            return None
        return AddManaInAnyCombination(
            amount=_amount(m.group(1)), options=options,
        )
    m = _SOURCE_DEALS_N_DAMAGE_TO_YOU.match(phrase)
    if m is not None:
        return SourceDealsDamageToController(amount=_amount(m.group(1)))
    if _UNTAP_SELF.match(phrase) is not None:
        return UntapSelf()
    m = _FLIP_COIN_IF_LOSE.match(phrase)
    if m is not None:
        inner = _parse_effect(m.group(1))
        if inner is None:
            return None
        return FlipCoinIfLost(on_loss=inner)
    m = _ADD_ONE_MANA_AMONG_TYPES.match(phrase)
    if m is not None:
        legendary = m.group(1) is not None
        return AddOneManaOfColorAmongControlled(
            card_types=_TYPE_GROUPS[m.group(2)],
            legendary=legendary,
            group_description=f"{m.group(1) or ''}{m.group(2)}",
        )
    m = _MAY_EXILE_CARD_FROM_HAND_IMPRINT.match(phrase)
    if m is not None:
        desc = m.group("desc")
        nm = _NON_TYPES_ONLY_DESCRIPTOR.match(desc)
        if nm is not None:
            excluded = tuple(
                w[len("non"):].capitalize() for w in nm.group(1).split(", ")
            )
            return ImprintCardFromHand(excluded_types=excluded)
        from mtgz_ir.cards.parser.card_flow_grammar import (
            parse_card_descriptor,
        )

        card_filter = parse_card_descriptor(desc)
        if card_filter is None:
            return None
        return ImprintCardFromHand(card_filter=card_filter)
    if _ADD_ONE_MANA_OF_EXILED_CARDS_COLORS.match(phrase) is not None:
        return AddOneManaOfImprintedColors()
    untap = _parse_untap_phrase(phrase)
    if untap is not None:
        return untap
    lib = _parse_library_and_delay_phrase(phrase)
    if lib is not None:
        return lib
    ctl = _parse_control_and_graveyard_phrase(phrase)
    if ctl is not None:
        return ctl
    veh = _parse_vehicle_phrase(phrase)
    if veh is not None:
        return veh
    two = _parse_counter_condition_and_two_tokens(phrase)
    if two is not None:
        return two
    cnt = _parse_counter_and_protection_phrase(phrase)
    if cnt is not None:
        return cnt
    that = _parse_that_player_phrase(phrase)
    if that is not None:
        return that
    pal = _parse_palantir_body(phrase)
    if pal is not None:
        return pal
    har = _parse_harness_phrase(phrase)
    if har is not None:
        return har
    ex = _parse_exiled_with_phrase(phrase)
    if ex is not None:
        return ex
    gy = _parse_graveyard_return_phrase(phrase)
    if gy is not None:
        return gy
    return _parse_create_then_attach_phrase(phrase)


# "Untap [another] target <type>." — Voltaic Key (artifact), Manifold
# Key (another artifact). -> unified ``Untap`` over a ``TargetSpec``;
# ``another`` excludes the source via ``~IsSource()``. ``land`` and
# ``permanent`` keep their existing dedicated recognizers in
# ``effects.py`` (``UntapTargetLand`` / ``UntapTargetOtherPermanent``).
_UNTAP_TARGET_TYPE = re.compile(
    r"^untap (another )?target (artifact|creature)$"
)

# "Tap target <type>." — Skateboard's ETB (``permanent``). -> unified
# ``Tap`` over a ``TargetSpec``. (``tap target creature`` is kept out on
# purpose: ``test_parser_effects_batch13`` pins it as unsupported.)
_TAP_TARGET_TYPE = re.compile(
    r"^tap target (permanent|artifact|land)$"
)

# "Untap X target lands." — Candelabra of Tawnos.
_UNTAP_X_TARGET_TYPE = re.compile(r"^untap X target (lands|artifacts|creatures)$")


def _type_predicate(word: str):
    from mtgz_ir.effects.object_predicates import (
        IsArtifact, IsCreature, IsLand,
    )
    return {
        "artifact": IsArtifact(), "artifacts": IsArtifact(),
        "creature": IsCreature(), "creatures": IsCreature(),
        "land": IsLand(), "lands": IsLand(),
    }[word]


def _parse_untap_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.effects.generic import Tap, Untap, UntapXTargets
    from mtgz_ir.effects.object_predicates import IsPlayer, IsSource
    from mtgz_ir.effects.targets import TargetSpec

    m = _TAP_TARGET_TYPE.match(phrase)
    if m is not None:
        word = m.group(1)
        pred = ~IsPlayer() if word == "permanent" else _type_predicate(word)
        return Tap(targets=(TargetSpec(pred),))
    m = _UNTAP_TARGET_TYPE.match(phrase)
    if m is not None:
        pred = _type_predicate(m.group(2))
        if m.group(1):
            pred = pred & ~IsSource()
        return Untap(targets=(TargetSpec(pred),))
    m = _UNTAP_X_TARGET_TYPE.match(phrase)
    if m is not None:
        return UntapXTargets(targets=(
            TargetSpec(_type_predicate(m.group(1)), 0, float("inf")),
        ))
    return None


# "Look at the top card of target player's library." (Mishra's Bauble)
_LOOK_AT_TOP_CARD_OF_TARGET_PLAYERS_LIBRARY = re.compile(
    r"^look at the top card of target player's library$"
)
# "Look at a card at random in target player's hand." (Urza's Bauble)
_LOOK_AT_RANDOM_CARD_IN_TARGET_PLAYERS_HAND = re.compile(
    r"^look at a card at random in target player's hand$"
)
# "[You ]<effect> at the beginning of the next turn's upkeep." — the
# Baubles' delayed draw. The inner effect parses recursively.
_AT_NEXT_TURNS_UPKEEP = re.compile(
    r"^(?:you )?(.+) at the beginning of the next turn's upkeep$"
)
# "Put this artifact on top of its owner's library." (Sensei's Top)
_PUT_SELF_ON_TOP_OF_OWNERS_LIBRARY = re.compile(
    r"^put this (?:artifact|creature|permanent) on top of its owner's "
    r"library$"
)
# Memory Jar's two-sentence wheel. Literal apart from the draw count:
# the second sentence's "each card they exiled this way" is anaphoric
# to the first, so the pair is one recognizer (only card in the pool
# with this shape).
_EXILE_HANDS_DRAW_N_RETURN_AT_END_STEP = re.compile(
    rf"^each player exiles all cards from their hand face down and draws "
    rf"({_CARDINAL}) cards\. At the beginning of the next end step, each "
    r"player discards their hand and returns to their hand each card they "
    r"exiled this way$"
)
# Grindstone's repeat-while-sharing-a-color mill. Literal (only card in
# the pool with a repeat-this-process mill).
_GRINDSTONE_MILL = (
    "target player mills two cards. If two cards that share a color were "
    "milled this way, repeat this process"
)
# "You may tap or untap target <type>." (Sewer-veillance Cam)
_MAY_TAP_OR_UNTAP_TARGET = re.compile(
    r"^you may tap or untap target (creature|artifact|land|permanent)$"
)


def _parse_library_and_delay_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import (
        EachPlayerExilesHandDrawsNReturnsLater,
        LookAtRandomCardInTargetPlayersHand,
        LookAtTopCardOfTargetPlayersLibrary,
        MayTapOrUntapTarget,
        MillTwoRepeatWhileShareColor,
        PutSourceOnTopOfOwnersLibrary,
        ScheduleDelayedEffect,
    )
    from mtgz_ir.effects.object_predicates import IsPlayer
    from mtgz_ir.effects.targets import TargetSpec

    if _LOOK_AT_TOP_CARD_OF_TARGET_PLAYERS_LIBRARY.match(phrase):
        return LookAtTopCardOfTargetPlayersLibrary()
    if _LOOK_AT_RANDOM_CARD_IN_TARGET_PLAYERS_HAND.match(phrase):
        return LookAtRandomCardInTargetPlayersHand()
    m = _AT_NEXT_TURNS_UPKEEP.match(phrase)
    if m is not None:
        inner = _parse_effect(m.group(1))
        if inner is None or inner.requires_target():
            return None
        return ScheduleDelayedEffect(inner=inner, step="upkeep")
    if _PUT_SELF_ON_TOP_OF_OWNERS_LIBRARY.match(phrase):
        return PutSourceOnTopOfOwnersLibrary()
    m = _EXILE_HANDS_DRAW_N_RETURN_AT_END_STEP.match(phrase)
    if m is not None:
        return EachPlayerExilesHandDrawsNReturnsLater(amount=_amount(m.group(1)))
    if phrase == _GRINDSTONE_MILL:
        return MillTwoRepeatWhileShareColor()
    m = _MAY_TAP_OR_UNTAP_TARGET.match(phrase)
    if m is not None:
        word = m.group(1)
        pred = ~IsPlayer() if word == "permanent" else _type_predicate(word)
        return MayTapOrUntapTarget(targets=(TargetSpec(pred),))
    return None


# "Each opponent sacrifices N <type>s of their choice." (Portal to
# Phyrexia: three creatures).
_EACH_OPPONENT_SACRIFICES_N = re.compile(
    rf"^each opponent sacrifices ({_CARDINAL}) "
    r"(creatures|artifacts|enchantments|lands) of their choice$"
)
_PLURAL_TYPES: dict[str, str] = {
    "creatures": "Creature", "artifacts": "Artifact",
    "enchantments": "Enchantment", "lands": "Land",
}
# "Put target creature card from a graveyard onto the battlefield under
# your control[. It's a <Subtype> in addition to its other types]."
_REANIMATE_FROM_A_GRAVEYARD = re.compile(
    r"^put target creature card from a graveyard onto the battlefield "
    r"under your control(?:\. It's an? ([A-Z][a-z]+) in addition to its "
    r"other types)?$"
)
# "An opponent gains control of this artifact." (Wishclaw Talisman)
_OPPONENT_GAINS_CONTROL_OF_SELF = re.compile(
    r"^an opponent gains control of this (?:artifact|creature|permanent)$"
)


def _parse_control_and_graveyard_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.effects.generic import (
        EachOpponentSacrificesN,
        OpponentGainsControlOfSource,
        ReanimateTargetCreatureCardFromAGraveyard,
    )

    m = _EACH_OPPONENT_SACRIFICES_N.match(phrase)
    if m is not None:
        return EachOpponentSacrificesN(
            amount=_amount(m.group(1)), card_type=_PLURAL_TYPES[m.group(2)],
        )
    m = _REANIMATE_FROM_A_GRAVEYARD.match(phrase)
    if m is not None:
        return ReanimateTargetCreatureCardFromAGraveyard(
            add_subtype=m.group(1) or "",
        )
    if _OPPONENT_GAINS_CONTROL_OF_SELF.match(phrase):
        return OpponentGainsControlOfSource()
    return None


# "You may have this <permanent> become an artifact creature until end
# of turn." (The Fantasticar).
_MAY_BECOME_ARTIFACT_CREATURE_UEOT = re.compile(
    r"^you may have this (?:creature|artifact|Vehicle) become an artifact "
    r"creature until end of turn$"
)
# "Create a token that's a copy of target token you control." (Esika's
# Chariot)
_CREATE_TOKEN_COPY_OF_TARGET_TOKEN = re.compile(
    r"^create a token that's a copy of target token you control$"
)
# "You may <X>. If you do, <Y>." — generic optional-cost-then-effect
# (Smuggler's Copter's loot, The Fantasticar's sacrifice). Consulted as
# a FALLBACK only (after every specialized "you may …" recognizer in
# ``effects.py`` had its chance) via ``_parse_artifact_effect_fallback``.
_MAY_X_IF_YOU_DO_Y = re.compile(r"^you may (.+?)\. If you do, (.+)$")


def _parse_vehicle_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.effects.combinators import ApplyEffect, MayDo
    from mtgz_ir.effects.durations import EndOfTurn
    from mtgz_ir.effects.generic import (
        BecomeType,
        CreateTokenCopyOfTargetTokenYouControl,
    )

    if _MAY_BECOME_ARTIFACT_CREATURE_UEOT.match(phrase):
        return MayDo(inner=ApplyEffect(
            BecomeType(types_to_add=("creature",)), ends_at=EndOfTurn(),
        ))
    if _CREATE_TOKEN_COPY_OF_TARGET_TOKEN.match(phrase):
        return CreateTokenCopyOfTargetTokenYouControl()
    return None


def _parse_artifact_effect_fallback(phrase: str) -> Effect | None:
    """Last-resort recognizers tried at the very end of
    ``effects._parse_effect`` (after every specialized shape failed)."""
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.combinators import IfYouDo, MayDo

    m = _MAY_X_IF_YOU_DO_Y.match(phrase)
    if m is not None:
        do = _parse_effect(m.group(1))
        then = _parse_effect(m.group(2))
        if do is None or then is None:
            return None
        if do.requires_target() and then.requires_target():
            return None
        return IfYouDo(do_action=MayDo(inner=do), then_action=then)
    return None


# "If there are no <kind> counters on this <permanent>, <effect>." —
# Reckoner Bankbuster's second sentence (its leading "Then" is stripped
# by the sentence splitter).
_IF_NO_COUNTERS_ON_SELF = re.compile(
    r"^if there are no ([a-z]+) counters on this "
    r"(?:Vehicle|artifact|creature|permanent), (.+)$"
)
# "Create <token A> and <token B>." — two different tokens in one
# clause (Reckoner Bankbuster: "a Treasure token and a 1/1 colorless
# Pilot creature token with …"). Each half is re-parsed as its own
# ``create …`` phrase; the split point is the first " and " for which
# both halves parse (token descriptors may contain " and " themselves).
_CREATE_TWO_TOKENS = re.compile(r"^create (an? .+) and (an? .+ tokens?.*)$")


def _parse_counter_condition_and_two_tokens(phrase: str) -> Effect | None:
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.combinators import Sequence
    from mtgz_ir.effects.generic import IfSourceHasNoCounters

    m = _IF_NO_COUNTERS_ON_SELF.match(phrase)
    if m is not None:
        inner = _parse_effect(m.group(2))
        if inner is None:
            return None
        return IfSourceHasNoCounters(kind=m.group(1), inner=inner)
    if _CREATE_TWO_TOKENS.match(phrase) is not None:
        body = phrase[len("create "):]
        start = 0
        while True:
            idx = body.find(" and ", start)
            if idx < 0:
                return None
            first, second = body[:idx], body[idx + len(" and "):]
            start = idx + 1
            if not second.startswith(("a ", "an ")):
                continue
            a = _parse_effect("create " + first)
            b = _parse_effect("create " + second)
            if a is not None and b is not None:
                if a.requires_target() or b.requires_target():
                    return None
                return Sequence(effects=(a, b))
    return None


_SELF_NOUN = r"this (?:artifact|creature|Vehicle|Equipment|permanent)"
# "Put a/N <kind> counter(s) on this <permanent>." — named counters on
# the source (The One Ring's burden, Staff of the Storyteller's story,
# Palantír's influence, Coalition Relic's charge). ``+1/+1`` / ``-1/-1``
# keep their dedicated recognizers (the ``[a-z]+`` kind excludes them).
_PUT_COUNTERS_ON_SELF = re.compile(
    rf"^put ({_CARDINAL}|a|an) ([a-z]+) counters? on {_SELF_NOUN}$"
)
# "Draw a card for each <kind> counter on this <permanent>."
_DRAW_PER_COUNTER_ON_SELF = re.compile(
    rf"^draw a card for each ([a-z]+) counter on {_SELF_NOUN}$"
)
# "You lose N life for each <kind> counter on this <permanent>."
_LOSE_LIFE_PER_COUNTER_ON_SELF = re.compile(
    rf"^you lose ({_CARDINAL}) life for each ([a-z]+) counter on {_SELF_NOUN}$"
)
# "You gain protection from everything until your next turn."
_GAIN_PROTECTION_FROM_EVERYTHING = re.compile(
    r"^you gain protection from everything until your next turn$"
)


# Counter kinds whose "put … on this" phrase already has a dedicated
# recognizer / engine class elsewhere (loyalty -> PutLoyaltyCounterOnSelf,
# pinned by the Tezzeret parity bridge); left to that grammar.
_COUNTER_KINDS_WITH_OWN_GRAMMAR = frozenset({
    "loyalty", "stun", "time", "lore", "flying",
})


def _parse_counter_and_protection_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.effects.generic import (
        DrawCardsPerCounterOnSource,
        GainProtectionFromEverythingUntilYourNextTurn,
        LoseLifePerCounterOnSource,
        PutChargeCountersOnSelf,
        PutNamedCountersOnSelf,
    )

    m = _PUT_COUNTERS_ON_SELF.match(phrase)
    if m is not None and m.group(2) not in _COUNTER_KINDS_WITH_OWN_GRAMMAR:
        amount = _amount(m.group(1))
        if m.group(2) == "charge":
            return PutChargeCountersOnSelf(amount=amount)
        return PutNamedCountersOnSelf(kind=m.group(2), amount=amount)
    m = _DRAW_PER_COUNTER_ON_SELF.match(phrase)
    if m is not None:
        return DrawCardsPerCounterOnSource(kind=m.group(1))
    m = _LOSE_LIFE_PER_COUNTER_ON_SELF.match(phrase)
    if m is not None:
        return LoseLifePerCounterOnSource(
            kind=m.group(2), per=_amount(m.group(1)),
        )
    if _GAIN_PROTECTION_FROM_EVERYTHING.match(phrase):
        return GainProtectionFromEverythingUntilYourNextTurn()
    return None


# Coveted Jewel's control-swap body (literal apart from the draw count:
# the only card in the pool that hands itself to "that player").
_THAT_PLAYER_DRAWS_AND_TAKES_SELF = re.compile(
    rf"^that player draws ({_CARDINAL}) cards and gains control of this "
    r"artifact\. Untap it$"
)


def _parse_that_player_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.effects.generic import ThatPlayerDrawsAndGainsControlOfSource

    m = _THAT_PLAYER_DRAWS_AND_TAKES_SELF.match(phrase)
    if m is not None:
        return ThatPlayerDrawsAndGainsControlOfSource(amount=_amount(m.group(1)))
    return None


# Palantír of Orthanc's end-step body (after self-name substitution).
# The second and third sentences are anaphoric ("that player", "those
# cards"), so the body is recognized whole; the counter kind and scry
# count are captured. Only card in the pool with this shape.
_PALANTIR_BODY = re.compile(
    rf"^put an? ([a-z]+) counter on {_SELF_NOUN} and scry ({_CARDINAL})\. "
    r"Then target opponent may have you draw a card\. If that player "
    r"doesn't, you mill X cards, where X is the number of ([a-z]+) counters "
    rf"on {_SELF_NOUN}, and that player loses life equal to the total mana "
    r"value of those cards$"
)


def _parse_palantir_body(phrase: str) -> Effect | None:
    from mtgz_ir.effects.combinators import Sequence
    from mtgz_ir.effects.generic import (
        PutNamedCountersOnSelf,
        Scry,
        TargetOpponentMayLetYouDrawElseMill,
    )

    m = _PALANTIR_BODY.match(phrase)
    if m is None or m.group(1) != m.group(3):
        return None
    return Sequence(effects=(
        PutNamedCountersOnSelf(kind=m.group(1), amount=1),
        Scry(amount=_amount(m.group(2))),
        TargetOpponentMayLetYouDrawElseMill(kind=m.group(3)),
    ))


# "Harness <this permanent>." (The Mind Stone, after self-name
# substitution).
_HARNESS_SELF = re.compile(rf"^harness {_SELF_NOUN}$")
# The Mind Stone's ∞ body — blink up to one other nonland permanent you
# control ("that card" anaphora keeps the two halves together).
_FLICKER_OTHER_NONLAND_YOU_CONTROL = re.compile(
    r"^exile up to one (?P<other>other )?target nonland permanent you "
    r"control, then return that card to the battlefield under its owner's "
    r"control$"
)


def _parse_harness_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.effects.generic import (
        FlickerUpToOneOtherTargetNonlandPermanentYouControl,
        HarnessSelf,
    )

    if _HARNESS_SELF.match(phrase):
        return HarnessSelf()
    m = _FLICKER_OTHER_NONLAND_YOU_CONTROL.match(phrase)
    if m is not None:
        # Displacer Kitten omits "other": the source may flicker itself.
        return FlickerUpToOneOtherTargetNonlandPermanentYouControl(
            other=m.group("other") is not None,
        )
    return None


# Currency Converter's two bodies. The discard-trigger body is optional
# for Currency Converter ("you may") and mandatory for Necropotence.
_MAY_EXILE_THAT_CARD_FROM_YOUR_GRAVEYARD = re.compile(
    r"^(?P<may>you may )?exile that card from your graveyard$"
)
_RETURN_EXILED_WITH_SELF_THEN_TOKEN = re.compile(
    rf"^put a card exiled with {_SELF_NOUN} into its owner's graveyard\. "
    r"If it's a land card, (.+)\. If it's a nonland card, (.+)$"
)


def _parse_exiled_with_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import (
        MayExileThatCardFromYourGraveyard,
        ReturnExiledWithSourceToGraveyardThenToken,
    )

    m = _MAY_EXILE_THAT_CARD_FROM_YOUR_GRAVEYARD.match(phrase)
    if m is not None:
        if m.group("may"):
            return MayExileThatCardFromYourGraveyard()
        return MayExileThatCardFromYourGraveyard(optional=False)
    m = _RETURN_EXILED_WITH_SELF_THEN_TOKEN.match(phrase)
    if m is not None:
        land = _parse_effect(m.group(1))
        nonland = _parse_effect(m.group(2))
        if land is None or nonland is None:
            return None
        if land.requires_target() or nonland.requires_target():
            return None
        return ReturnExiledWithSourceToGraveyardThenToken(
            land_effect=land, nonland_effect=nonland,
        )
    return None


# Sword of the Meek's graveyard trigger body.
_RETURN_THIS_CARD_THEN_ATTACH = re.compile(
    r"^you may return this card from your graveyard to the battlefield, "
    r"then attach it to that creature$"
)


def _parse_graveyard_return_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.effects.combinators import MayDo
    from mtgz_ir.effects.generic import (
        ReturnThisCardFromGraveyardThenAttachToThatCreature,
    )

    if _RETURN_THIS_CARD_THEN_ATTACH.match(phrase):
        return MayDo(inner=ReturnThisCardFromGraveyardThenAttachToThatCreature())
    return None


def _functions_from_graveyard(effect) -> bool:
    """True if ``effect`` (or the inner effect of a ``MayDo``) is a
    graveyard-functioning body (its trigger works from the graveyard)."""
    inner = getattr(effect, "inner", effect)
    return bool(getattr(inner, "functions_from_graveyard", False))



# "Create <token>. You may attach this Equipment to it." — Cori-Steel
# Cutter's Flurry body; the create half parses through the unified
# token grammar.
_CREATE_TOKEN_MAY_ATTACH_SELF = re.compile(
    r"^(create .+ tokens?(?: with .+)?)\. You may attach this Equipment to it$"
)


def _parse_create_then_attach_phrase(phrase: str) -> Effect | None:
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import CreateToken, CreateTokenThenMayAttachSelf

    m = _CREATE_TOKEN_MAY_ATTACH_SELF.match(phrase)
    if m is None:
        return None
    create = _parse_effect(m.group(1))
    if not isinstance(create, CreateToken):
        return None
    return CreateTokenThenMayAttachSelf(create=create)
