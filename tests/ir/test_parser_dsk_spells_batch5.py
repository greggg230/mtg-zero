"""Tests for the Duskmourn spells / enchantments batch-5 parser additions.

Each commit in this batch lands one or more cards via a small additive
change to the parser's whole-spell exact-text dispatch table or via a
new bespoke engine class. No existing engine classes are modified
beyond additive flag widenings; the parser's job here is to claim
spells whose printed shapes don't fit the per-clause grammar but DO
admit a clean simplification onto an existing primitive (parser-is-
right precedent: Unravel / Divert Disaster) or a one-off bespoke
engine class (Plasma Bolt / Cut Propulsion precedent).

Cards claimed in this batch:

* **Scorching Dragonfire** ``{1}{R}`` Instant — 3 damage to target
  creature/planeswalker plus a "die-this-turn-exile-instead" rider.
  Whole-spell dispatch simplifies to bare
  ``DealDamageToTargetCreature(amount=3)``; the planeswalker target
  scope and the die-replacement rider are dropped per the parser-is-
  right precedent set by Unravel / Divert Disaster (drop unmodelable
  rider, keep the base mechanic).

* **Unwanted Remake** ``{W}`` Instant — Destroy target creature plus
  a "Its controller manifests dread" rider. Whole-spell dispatch
  simplifies to bare ``DestroyTargetCreature()``; the controller-
  side manifest-dread rider is dropped because the engine's
  ``ManifestDread`` primitive resolves at the resolving controller
  (the spell's caster), not at the destroyed permanent's controller.

* **Jump Scare** ``{W}`` Instant — Pump target creature +2/+2 with
  flying plus a "becomes a Horror enchantment creature in addition
  to its other types" type-change rider. Whole-spell dispatch
  simplifies to bare
  ``PumpTargetCreatureUntilEOT(power=2, toughness=2,
  granted_keywords={"flying"})``; the UEOT type-change rider is
  dropped because the engine doesn't model type-line additions on
  targeted creatures.
"""

# mtg-zero (docs/ENGINE.md 170): 4 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.agents, mtgz_ir.combat, not carried into mtgz_ir):
# test_coordinated_clobbering_resolve_one_attacker, test_coordinated_clobbering_resolve_two_attackers, test_coordinated_clobbering_skip_friendly_lost_control, test_vengeful_possession_optional_looter_fires

from __future__ import annotations

from mtgz_ir.cards.parser.source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- Scorching Dragonfire ----------

def test_scorching_dragonfire_claimed_by_parser():
    # Scorching Dragonfire ``{1}{R}`` Instant — claimed via the new
    # whole-spell exact-text dispatch entry that simplifies the
    # printed text (3 damage to creature/planeswalker + die-this-
    # turn-exile rider) to bare ``DealDamageToTargetCreature(amount=3)``.
    assert "Scorching Dragonfire" in _claimed_names()


def test_scorching_dragonfire_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``DealDamageToTargetCreature(amount=3)`` engine
    # class — base mechanic preserved, planeswalker target widening
    # and die-replacement rider dropped (parser-is-right precedent).
    from mtgz_ir.cards.base import DealDamageToTargetCreature

    parser = OracleParserCardSource()
    p = parser.get_printings()["Scorching Dragonfire"]
    assert p.spell_effect == DealDamageToTargetCreature(amount=3)


def test_scorching_dragonfire_is_instant():
    # Type-line carries Instant — not Sorcery — so the spell can be
    # cast in response to an opponent's combat / spell.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Scorching Dragonfire"]
    assert "Instant" in p.card_types


# ---------- Unwanted Remake ----------

def test_unwanted_remake_claimed_by_parser():
    # Unwanted Remake ``{W}`` Instant — claimed via the new
    # whole-spell exact-text dispatch entry that simplifies the
    # printed text (destroy target creature + opp-controller-
    # manifests-dread rider) to bare ``DestroyTargetCreature()``.
    # Reminder text inside the trailing parenthetical is stripped
    # before dispatch.
    assert "Unwanted Remake" in _claimed_names()


def test_unwanted_remake_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``DestroyTargetCreature()`` engine class — base
    # mechanic preserved, opp-side manifest-dread rider dropped
    # (parser-is-right precedent).
    from mtgz_ir.cards.base import DestroyTargetCreature

    parser = OracleParserCardSource()
    p = parser.get_printings()["Unwanted Remake"]
    assert p.spell_effect == DestroyTargetCreature()


def test_unwanted_remake_is_instant():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Unwanted Remake"]
    assert "Instant" in p.card_types


# ---------- Jump Scare ----------

def test_jump_scare_claimed_by_parser():
    # Jump Scare ``{W}`` Instant — originally claimed via whole-spell
    # exact-text dispatch; migrated to per-clause grammar in
    # bespoke-spells2 via the new ``_PUMP_LEADING_UEOT_KEYWORDS_BECOMES_TYPE``
    # recognizer in ``effects.py``. The leading "Until end of turn,"
    # adverb is handled by the new regex; the type-change rider
    # ("becomes a Horror enchantment creature in addition to its other
    # types") is dropped per parser-is-right precedent.
    assert "Jump Scare" in _claimed_names()


def test_jump_scare_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified pump engine class — power/toughness bonus and
    # flying granted-keyword preserved, type-change rider dropped.
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT

    parser = OracleParserCardSource()
    p = parser.get_printings()["Jump Scare"]
    assert p.spell_effect == PumpTargetCreatureUntilEOT(
        power_bonus=2,
        toughness_bonus=2,
        granted_keywords=frozenset({"flying"}),
    )


def test_jump_scare_is_instant():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Jump Scare"]
    assert "Instant" in p.card_types


# ---------- ThreatenTargetCreatureUEOT (the threaten core extracted
# from the former VengefulPossessionEffect / SystemsOverrideEffect
# duplicates) ----------

def test_threaten_target_creature_ueot_describe():
    from mtgz_ir.effects import ThreatenTargetCreatureUEOT

    desc = ThreatenTargetCreatureUEOT().describe()
    assert "gain control" in desc
    assert "untap" in desc
    assert "haste" in desc


def test_threaten_target_creature_ueot_creatures_only():
    # The threaten primitive only targets creatures. (The wider
    # artifact-or-creature scope used by Systems Override is a
    # separate primitive.)
    from mtgz_ir.cards.base import make_artifact, make_creature
    from mtgz_ir.game import GameState, Player, Zone
    from mtgz_ir.effects import ThreatenTargetCreatureUEOT
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear = GameObject(
        id=1, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    art_pr = make_artifact("Plain Artifact", "{2}")
    art = GameObject(
        id=2, printing=art_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[bear, art],
    )
    legal = ThreatenTargetCreatureUEOT().legal_targets(state, 0)
    ids = {t[1] for t in legal}
    assert bear.id in ids
    assert art.id not in ids


def test_threaten_target_creature_ueot_resolves_gain_control_untap_haste():
    # End-to-end resolve sanity: target opponent's tapped creature
    # gets controlled by us, untaps, and gains haste UEOT.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import ThreatenTargetCreatureUEOT
    from mtgz_ir.game import (
        GameObject, GameState, Player, Zone, effective_keywords,
    )

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear = GameObject(
        id=1, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD, tapped=True,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[bear],
    )
    ThreatenTargetCreatureUEOT().resolve(
        state, controller_id=0, targets=(("creature", bear.id),)
    )
    assert bear.controller_id == 0
    assert bear.tapped is False
    assert "haste" in effective_keywords(state, bear)


# ---------- Vengeful Possession end-to-end behavior (looter actually
# fires through the parser-built Sequence) ----------



def test_vengeful_possession_optional_looter_empty_hand_fizzles():
    # When the controller has no cards to discard, the rider's "if
    # you do" gate fizzles — no draw. The IfYouDo combinator reads
    # the bool returned by DiscardSelf (False on empty hand) and
    # skips the DrawCards follow-up.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    bear = GameObject(
        id=1, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    junk_pr = make_creature("Junk", "{1}", power=1, toughness=1)
    library_a = GameObject(
        id=3, printing=junk_pr, owner_id=0, controller_id=0,
        zone=Zone.LIBRARY,
    )
    p0 = Player(id=0)
    p0.library = [library_a]
    # Empty hand.
    state = GameState(
        players=[p0, Player(id=1)],
        battlefield=[bear],
    )
    parser = OracleParserCardSource()
    spell_effect = parser.get_printings()["Vengeful Possession"].spell_effect
    spell_effect.resolve(
        state, controller_id=0, targets=(("creature", bear.id),)
    )
    assert bear.controller_id == 0
    # Library card is NOT drawn — the gated draw fizzled with the
    # empty-hand discard.
    assert library_a in p0.library
    assert library_a not in p0.hand


# ---------- Vengeful Possession parser claim ----------

def test_vengeful_possession_claimed_by_parser():
    # Vengeful Possession ``{2}{R}`` Sorcery — claimed via the per-clause
    # grammar's multi-sentence literal in ``effects.py``. The full
    # four-sentence body (Threaten core + optional-looter rider) maps to
    # ``Sequence(ThreatenTargetCreatureUEOT(),
    #            IfYouDo(MayDo(DiscardSelf(amount=1)),
    #                    DrawCards(amount=1)))`` —
    # two generic primitives, no per-card bespoke class.
    assert "Vengeful Possession" in _claimed_names()


def test_vengeful_possession_spell_effect_is_decomposed_sequence():
    # The spell_effect is a Sequence of two generic primitives — the
    # threaten core and the looter rider — not a per-card bespoke
    # class. This is the architectural anchor: future cards that print
    # the threaten core or the looter rider get them for free.
    from mtgz_ir.effects import (
        DiscardSelf, DrawCards, IfYouDo, MayDo,
        Sequence, ThreatenTargetCreatureUEOT,
    )

    parser = OracleParserCardSource()
    p = parser.get_printings()["Vengeful Possession"]
    assert isinstance(p.spell_effect, Sequence)
    assert p.spell_effect.effects == (
        ThreatenTargetCreatureUEOT(),
        IfYouDo(
            do_action=MayDo(DiscardSelf(amount=1)),
            then_action=DrawCards(amount=1),
        ),
    )


def test_vengeful_possession_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Vengeful Possession"]
    assert "Sorcery" in p.card_types


def test_vengeful_possession_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the full four-sentence body
    # and decomposes into Sequence(threaten, IfYouDo(MayDo(discard),
    # draw)).
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    from mtgz_ir.effects import (
        DiscardSelf, DrawCards, IfYouDo, MayDo,
        Sequence, ThreatenTargetCreatureUEOT,
    )
    body = (
        "Gain control of target creature until end of turn. "
        "Untap it. It gains haste until end of turn. "
        "You may discard a card. If you do, draw a card."
    )
    eff = _parse_spell_effect_phrase(body, "Vengeful Possession")
    assert eff == Sequence((
        ThreatenTargetCreatureUEOT(),
        IfYouDo(
            do_action=MayDo(DiscardSelf(amount=1)),
            then_action=DrawCards(amount=1),
        ),
    ))


# ---------- ExorciseEffect engine class ----------

def test_exorcise_effect_exists():
    # New bespoke engine class wired in ``mtgz_ir.effects.bespoke``
    # and exposed via ``mtgz_ir.effects``.
    from mtgz_ir.effects import ExorciseEffect

    eff = ExorciseEffect()
    assert eff.requires_target() is True


def test_exorcise_describe():
    from mtgz_ir.effects import ExorciseEffect

    desc = ExorciseEffect().describe()
    assert "exile" in desc
    assert "artifact" in desc
    assert "enchantment" in desc
    assert "power 4" in desc


def test_exorcise_legal_targets_include_artifact():
    # Artifacts are unconditionally legal regardless of P/T.
    from mtgz_ir.cards.base import make_artifact
    from mtgz_ir.effects import ExorciseEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    art_pr = make_artifact("Plain Artifact", "{2}")
    art = GameObject(
        id=1, printing=art_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[art],
    )
    legal = ExorciseEffect().legal_targets(state, 0)
    ids = {t[1] for t in legal}
    assert art.id in ids


def test_exorcise_legal_targets_include_enchantment():
    # Pure enchantments are unconditionally legal regardless of P/T.
    from mtgz_ir.cards.base import make_enchantment
    from mtgz_ir.effects import ExorciseEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    ench_pr = make_enchantment("Plain Enchantment", "{1}{W}")
    ench = GameObject(
        id=1, printing=ench_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[ench],
    )
    legal = ExorciseEffect().legal_targets(state, 0)
    ids = {t[1] for t in legal}
    assert ench.id in ids


def test_exorcise_legal_targets_include_4_power_creature():
    # Creatures with power >= 4 are legal; power < 4 creatures aren't.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import ExorciseEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    big_pr = make_creature("Big Bear", "{3}{G}", power=4, toughness=4)
    big = GameObject(
        id=1, printing=big_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    small_pr = make_creature(
        "Small Bear", "{1}{G}", power=2, toughness=2
    )
    small = GameObject(
        id=2, printing=small_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[big, small],
    )
    legal = ExorciseEffect().legal_targets(state, 0)
    ids = {t[1] for t in legal}
    assert big.id in ids
    assert small.id not in ids


def test_exorcise_resolves_exile():
    # End-to-end: resolving against a legal artifact target moves
    # it from battlefield to exile.
    from mtgz_ir.cards.base import make_artifact
    from mtgz_ir.effects import ExorciseEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    art_pr = make_artifact("Plain Artifact", "{2}")
    art = GameObject(
        id=1, printing=art_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[art],
    )
    ExorciseEffect().resolve(
        state, controller_id=0, targets=(("permanent", art.id),)
    )
    assert art not in state.battlefield


def test_exorcise_no_target_noop():
    # Defensive: empty targets resolve without raising.
    from mtgz_ir.effects import ExorciseEffect
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    ExorciseEffect().resolve(state, controller_id=0, targets=())


def test_exorcise_wrong_kind_noop():
    # Defensive: non-permanent target kind resolves without raising.
    from mtgz_ir.effects import ExorciseEffect
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    ExorciseEffect().resolve(
        state, controller_id=0, targets=(("creature", 999),)
    )


def test_exorcise_missing_target_noop():
    # Defensive: a target id that doesn't exist resolves without
    # raising.
    from mtgz_ir.effects import ExorciseEffect
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    ExorciseEffect().resolve(
        state, controller_id=0, targets=(("permanent", 999),)
    )


def test_exorcise_recheck_at_resolve_for_debuffed_creature():
    # If a 4-power creature gets debuffed below 4 between cast and
    # resolve, the resolve recheck fizzles the exile rather than
    # silently exiling a no-longer-legal target.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import ExorciseEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    big_pr = make_creature("Big Bear", "{3}{G}", power=4, toughness=4)
    big = GameObject(
        id=1, printing=big_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
        # Simulate a debuff dropping effective_power below 4 mid-stack.
        temp_power_bonus=-2,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[big],
    )
    ExorciseEffect().resolve(
        state, controller_id=0, targets=(("permanent", big.id),)
    )
    # Target stays on battlefield — recheck failed.
    assert big in state.battlefield


# ---------- Exorcise parser claim ----------

def test_exorcise_claimed_by_parser():
    # Exorcise ``{1}{W}`` Sorcery -- claimed by the compositional
    # removal grammar (``removal_phrases.parse_targeted_removal`` over
    # ``target_phrases.parse_permanent_phrase``).
    assert "Exorcise" in _claimed_names()


def _exorcise_filter():
    from mtgz_ir.effects.object_predicates import (
        HasPowerAtLeast, IsArtifact, IsCreature, IsEnchantment,
    )

    return (IsArtifact() | IsEnchantment()) | (IsCreature() & HasPowerAtLeast(4))


def test_exorcise_composes_from_the_generic_exile_grammar():
    from mtgz_ir.effects.generic.targeted_removal import RemoveTargetPermanents

    parser = OracleParserCardSource()
    p = parser.get_printings()["Exorcise"]
    assert p.spell_effect == RemoveTargetPermanents(
        action="exile", target_filter=_exorcise_filter(),
    )


def test_exorcise_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Exorcise"]
    assert "Sorcery" in p.card_types


def test_exorcise_targets_and_exiles_through_the_generic_primitive():
    from mtgz_ir.cards.base import make_artifact, make_creature
    from mtgz_ir.game import GameState, Player, Zone

    parser = OracleParserCardSource()
    effect = parser.get_printings()["Exorcise"].spell_effect
    state = GameState()
    state.players.append(Player(id=0))
    state.players.append(Player(id=1))
    rock = state.new_object(make_artifact("Rock", "{1}"), owner_id=1, zone=Zone.BATTLEFIELD)
    small = state.new_object(make_creature("Small", "{1}", 3, 3), owner_id=1, zone=Zone.BATTLEFIELD)
    big = state.new_object(make_creature("Big", "{4}", 4, 4), owner_id=1, zone=Zone.BATTLEFIELD)
    for o in (rock, small, big):
        state.battlefield.append(o)
    legal = {t[1] for t in effect.legal_targets(state, 0)}
    assert legal == {rock.id, big.id}
    effect.resolve(state, 0, (("permanent", big.id),))
    assert big not in state.battlefield
    assert big in state.players[1].exile


# ---------- CoordinatedClobberingEffect engine class ----------

def test_coordinated_clobbering_effect_exists():
    # New bespoke engine class wired in ``mtgz_ir.effects.bespoke``
    # next to ``DiplomaticRelationsEffect``. Imports cleanly and is
    # exposed via ``mtgz_ir.effects``.
    from mtgz_ir.effects import CoordinatedClobberingEffect

    eff = CoordinatedClobberingEffect()
    assert eff.requires_target() is True


def test_coordinated_clobbering_describe():
    from mtgz_ir.effects import CoordinatedClobberingEffect

    desc = CoordinatedClobberingEffect().describe()
    assert "tap" in desc
    assert "damage" in desc
    assert "power" in desc


def test_coordinated_clobbering_target_count_range():
    # Min 2 (one friendly + one opp), max 3 (two friendlies + one
    # opp). The opp slot is mandatory first; slot 2 is optional.
    from mtgz_ir.effects import CoordinatedClobberingEffect

    eff = CoordinatedClobberingEffect()
    assert eff.target_count_range() == (2, 3)
    assert eff.target_slot_kinds() == (
        "creature", "my_creature", "my_creature"
    )


def test_coordinated_clobbering_legal_targets_filters():
    # Friendly slot pool: only untapped creatures the caster
    # controls. Opp slot pool: any creature the opponent controls.
    # Returned as a single concatenated list (theirs + mine) so the
    # cast-pipeline slot-kind filter routes each pick to the right
    # slot.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import CoordinatedClobberingEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    my_untapped = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    my_tapped = GameObject(
        id=2, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD, tapped=True,
    )
    opp_creature = GameObject(
        id=3, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[my_untapped, my_tapped, opp_creature],
    )
    legal = CoordinatedClobberingEffect().legal_targets(state, 0)
    assert ("my_creature", my_untapped.id) in legal
    # Tapped friendly is excluded.
    assert ("my_creature", my_tapped.id) not in legal
    assert ("creature", opp_creature.id) in legal


def test_coordinated_clobbering_legal_targets_empty_no_friendlies():
    # If the caster has no untapped friendly creatures, the spell
    # can't be cast (legal_targets returns []).
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import CoordinatedClobberingEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    opp_creature = GameObject(
        id=1, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[opp_creature],
    )
    assert CoordinatedClobberingEffect().legal_targets(state, 0) == []


def test_coordinated_clobbering_legal_targets_empty_no_opp_creatures():
    # If the opponent has no creatures, the spell can't be cast.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import CoordinatedClobberingEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    my_bear = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[my_bear],
    )
    assert CoordinatedClobberingEffect().legal_targets(state, 0) == []






def test_coordinated_clobbering_no_target_noop():
    # Defensive: empty targets resolve without raising.
    from mtgz_ir.effects import CoordinatedClobberingEffect
    from mtgz_ir.game import GameState, Player

    state = GameState(players=[Player(id=0), Player(id=1)])
    CoordinatedClobberingEffect().resolve(
        state, controller_id=0, targets=()
    )




def test_coordinated_clobbering_skip_opp_lost_control():
    # If the opp target's controller switched to caster between
    # cast and resolve, the damage is skipped (the target became
    # the caster's own creature). Friendlies still tap.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import CoordinatedClobberingEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    my_a = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    # ``opp`` switched control back to P0 (the caster).
    opp = GameObject(
        id=2, printing=bear_pr, owner_id=1, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[my_a, opp],
    )
    CoordinatedClobberingEffect().resolve(
        state, controller_id=0,
        targets=(
            ("creature", opp.id),
            ("my_creature", my_a.id),
        ),
    )
    assert my_a.tapped is True
    # No damage applied to the (now-friendly) opp target.
    assert opp.damage == 0


# ---------- Coordinated Clobbering parser claim ----------

def test_coordinated_clobbering_claimed_by_parser():
    # Coordinated Clobbering ``{G}`` Sorcery — migrated from the
    # whole-spell exact-text tier to the per-clause grammar: the new
    # multi-sentence literal in ``effects.py`` claims the two-sentence
    # body as ``CoordinatedClobberingEffect()``.
    assert "Coordinated Clobbering" in _claimed_names()


def test_coordinated_clobbering_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the two-sentence body (tap 1-2
    # friendly creatures; they each deal power damage to an opp creature)
    # as ``CoordinatedClobberingEffect()``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    from mtgz_ir.effects import CoordinatedClobberingEffect
    body = (
        "Tap one or two target untapped creatures you control. "
        "They each deal damage equal to their power to target "
        "creature an opponent controls."
    )
    eff = _parse_spell_effect_phrase(body, "Coordinated Clobbering")
    assert eff == CoordinatedClobberingEffect()


def test_coordinated_clobbering_engine_class():
    from mtgz_ir.effects import CoordinatedClobberingEffect

    parser = OracleParserCardSource()
    p = parser.get_printings()["Coordinated Clobbering"]
    assert p.spell_effect == CoordinatedClobberingEffect()


def test_coordinated_clobbering_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Coordinated Clobbering"]
    assert "Sorcery" in p.card_types
