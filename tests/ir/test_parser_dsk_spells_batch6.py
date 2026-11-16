"""Tests for the Duskmourn spells / enchantments batch-6 parser additions.

Each commit in this batch lands one or more cards via a small additive
change to the parser's whole-spell exact-text dispatch table or via a
new bespoke engine class. No existing engine classes are modified
beyond additive flag widenings; the parser's job here is to claim
spells whose printed shapes don't fit the per-clause grammar but DO
admit a clean simplification onto an existing primitive (parser-is-
right precedent: Unravel / Divert Disaster) or a one-off bespoke
engine class (Plasma Bolt / Cut Propulsion precedent).

Cards claimed in this batch:

* **Trial of Agony** ``{R}`` Sorcery — multi-target opp-choice burn
  with a "can't block this turn" rider on the unhit creature. Whole-
  spell dispatch simplifies to bare
  ``DealDamageToTargetCreature(amount=5)``; the multi-target opp-
  choice slot kind and the can't-block rider are dropped per the
  parser-is-right precedent set by Scorching Dragonfire (drop
  unmodelable rider, keep the base mechanic).

* **Under the Skin** ``{2}{G}`` Sorcery — ``Manifest dread.`` plus
  an optional ``You may return a permanent card from your graveyard
  to your hand.`` rider. Whole-spell dispatch simplifies to bare
  ``ManifestDread(count=1)``; the optional graveyard-return rider is
  dropped because no per-clause primitive widens to "any nonland
  permanent card from graveyard to hand with optionality"
  (parser-is-right precedent).

* **Midnight Mayhem** ``{2}{R}{W}`` Sorcery — three Gremlin tokens +
  a UEOT subtype-filtered keyword grant ("Gremlins you control gain
  menace, lifelink, and haste until end of turn"). Whole-spell
  dispatch simplifies to bare
  ``CreateToken(Gremlin, count=3)``; the subtype-filtered keyword
  grant is dropped because the existing
  ``PumpCreaturesYouControlUntilEOT`` widens to ALL your creatures
  (no subtype filter), and adding one for a single card's rider
  would over-engineer (parser-is-right precedent: Terminal Velocity).

* **Get Out** ``{U}{U}`` Instant — two-mode modal whose mode 2
  ("Return one or two target creatures and/or enchantments you own
  to your hand.") has no per-mode primitive. Whole-spell dispatch
  simplifies to mode 1's
  ``CounterTargetSpell(filter_types=frozenset({"Creature",
  "Enchantment"}))``; mode 2 is dropped per the parser-is-right
  precedent set by Archenemy's Charm (claim a well-modeled mode of a
  multi-mode modal rather than leaving the spell unclaimed).

* **Untimely Malfunction** ``{1}{R}`` Instant — three-mode modal
  whose modes 2 ("Change the target of target spell or ability...")
  and 3 ("One or two target creatures can't block this turn.") have
  no per-mode primitives. Whole-spell dispatch simplifies to mode 1's
  ``DestroyTargetArtifact()``; modes 2 and 3 are dropped per the
  parser-is-right precedent set by Get Out / Archenemy's Charm.

* **Unnerving Grasp** ``{2}{U}`` Sorcery — two-clause: ``Return up
  to one target nonland permanent to its owner's hand.`` followed by
  ``Manifest dread.``. Whole-spell dispatch simplifies to bare
  ``ManifestDread(count=1)``; the optional up-to-one bounce is
  dropped because no per-clause primitive parses the
  ``up to one target nonland permanent`` shape, and a Sequence
  composition would silently no-op the bounce at every cast (same
  parser-is-right precedent as Under the Skin: drop the optional
  secondary rider, keep the manifest base).

* **Commune with Evil** ``{2}{B}`` Sorcery — claimed via a new
  bespoke ``CommuneWithEvilEffect`` engine class that folds the
  three-step single-resolve effect (look at top 4, put one into
  hand, rest into graveyard, gain 3 life) into a single resolve.
  Mirrors ``LookTopNPutOneInHandRestToGraveyard(n=2,
  reveal_chosen=False)`` (Codecracker Hound, after the refined-
  Cycle-1 fold of the prior ``LookAtTopTwoSplitHandGraveyard`` class)
  in shape but generalizes the 2-card split to a 4-card single-pick
  split with a trailing 3-life-gain rider.

* **Turn Inside Out** ``{R}`` Instant — pump-target-creature +3/+0
  UEOT plus a "When it dies this turn, manifest dread."
  death-triggered rider. Whole-spell dispatch simplifies to bare
  ``PumpTargetCreatureUntilEOT(power_bonus=3, toughness_bonus=0)``;
  the death-triggered manifest-dread rider is dropped because the
  engine has no UEOT-scoped per-target death-trigger plumbing
  (parser-is-right precedent: Jump Scare / Scorching Dragonfire).
"""

from __future__ import annotations

from mtgz_ir.cards.parser.source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- Trial of Agony ----------

def test_trial_of_agony_claimed_by_parser():
    # Trial of Agony ``{R}`` Sorcery — migrated from the whole-spell
    # exact-text tier to the per-clause grammar: the new multi-sentence
    # literal in ``effects.py`` claims the three-sentence body and
    # simplifies to ``DealDamageToTargetCreature(amount=5)``
    # (parser-is-right: opp-choice targeting and can't-block rider dropped).
    assert "Trial of Agony" in _claimed_names()


def test_trial_of_agony_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``DealDamageToTargetCreature(amount=5)`` engine
    # class — base mechanic preserved, multi-target opp-choice slot
    # kind and can't-block rider dropped (parser-is-right precedent).
    from mtgz_ir.cards.base import DealDamageToTargetCreature

    parser = OracleParserCardSource()
    p = parser.get_printings()["Trial of Agony"]
    assert p.spell_effect == DealDamageToTargetCreature(amount=5)


def test_trial_of_agony_parses_via_per_clause_grammar():
    # Belt-and-suspenders: the per-clause grammar's multi-sentence
    # literal in ``effects.py`` claims the three-sentence body as
    # ``DealDamageToTargetCreature(amount=5)``.
    from mtgz_ir.cards.parser.effects import _parse_spell_effect_phrase
    from mtgz_ir.cards.base import DealDamageToTargetCreature
    body = (
        "Choose two target creatures controlled by the same opponent. "
        "That player chooses one of those creatures. Trial of Agony "
        "deals 5 damage to that creature, and the other can't block "
        "this turn."
    )
    eff = _parse_spell_effect_phrase(body, "Trial of Agony")
    assert eff == DealDamageToTargetCreature(amount=5)


def test_trial_of_agony_is_sorcery():
    # Type-line carries Sorcery — not Instant — so the spell can only
    # be cast at sorcery speed.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Trial of Agony"]
    assert "Sorcery" in p.card_types


def test_trial_of_agony_mana_cost():
    # Mono-red ``{R}`` mana cost — confirms the spell carries the
    # printed cost through the factory.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Trial of Agony"]
    assert p.mana_cost_str == "{R}"


# ---------- Under the Skin ----------

def test_under_the_skin_claimed_by_parser():
    # Under the Skin ``{2}{G}`` Sorcery — claimed via the new whole-
    # spell exact-text dispatch entry that simplifies the printed text
    # (manifest dread + optional grave-return rider) to bare
    # ``ManifestDread(count=1)``. Reminder text inside the manifest
    # parenthetical is stripped before dispatch.
    assert "Under the Skin" in _claimed_names()


def test_under_the_skin_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``ManifestDread(count=1)`` engine class — base
    # mechanic preserved, optional graveyard-return rider dropped
    # (parser-is-right precedent).
    from mtgz_ir.cards.base import ManifestDread

    parser = OracleParserCardSource()
    p = parser.get_printings()["Under the Skin"]
    assert p.spell_effect == ManifestDread(count=1)


def test_under_the_skin_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Under the Skin"]
    assert "Sorcery" in p.card_types


def test_under_the_skin_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Under the Skin"]
    assert p.mana_cost_str == "{2}{G}"


# ---------- Midnight Mayhem ----------

def test_midnight_mayhem_claimed_by_parser():
    # Midnight Mayhem ``{2}{R}{W}`` Sorcery — claimed via the new
    # whole-spell exact-text dispatch entry that simplifies the
    # printed text (3 Gremlin tokens + Gremlin-only UEOT pump) to
    # bare ``CreateToken(Gremlin, count=3)``. The trailing
    # parenthesized menace reminder text is stripped before dispatch.
    assert "Midnight Mayhem" in _claimed_names()


def test_midnight_mayhem_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``CreateToken(Gremlin, count=3)`` engine class —
    # base mechanic preserved (3 Gremlin tokens), UEOT keyword grant
    # rider dropped (parser-is-right precedent).
    from mtgz_ir.cards.base import CreateToken
    from mtgz_ir.cards.tokens import TOKEN_PRINTINGS

    from dataclasses import replace

    parser = OracleParserCardSource()
    p = parser.get_printings()["Midnight Mayhem"]
    # The printed "red Gremlin" colour is the token's colour indicator.
    assert p.spell_effect == CreateToken(
        token_printing=replace(TOKEN_PRINTINGS["Gremlin"], color_indicator=frozenset({"R"})), count=3,
    )


def test_midnight_mayhem_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Midnight Mayhem"]
    assert "Sorcery" in p.card_types


def test_midnight_mayhem_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Midnight Mayhem"]
    assert p.mana_cost_str == "{2}{R}{W}"


# ---------- Get Out ----------

def test_get_out_claimed_by_parser():
    # Get Out ``{U}{U}`` Instant — now claimed via the per-clause modal
    # grammar (migrated from the whole-spell exact-text table in
    # bespoke-spells3). The new "exactly-one-parseable-mode" fallback
    # in ``_parse_modal_choose_one`` returns mode 1's effect when mode 2
    # is unrecognized.
    assert "Get Out" in _claimed_names()


def test_get_out_engine_class():
    # Mode 1 "Counter target creature or enchantment spell." parses as
    # ``CounterTargetSpell(filter_types=frozenset({"Creature",
    # "Enchantment"}))``. Mode 2 unrecognized → exactly-one fallback.
    from mtgz_ir.cards.base import CounterTargetSpell

    parser = OracleParserCardSource()
    p = parser.get_printings()["Get Out"]
    assert p.spell_effect == CounterTargetSpell(
        filter_types=frozenset({"Creature", "Enchantment"}),
    )


def test_get_out_is_instant():
    # Type-line carries Instant — not Sorcery — so the spell can be
    # cast in response to an opponent's spell on the stack (the whole
    # point of a counterspell).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Get Out"]
    assert "Instant" in p.card_types


def test_get_out_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Get Out"]
    assert p.mana_cost_str == "{U}{U}"


# ---------- Untimely Malfunction ----------

def test_untimely_malfunction_claimed_by_parser():
    # Untimely Malfunction ``{1}{R}`` Instant — now claimed via the per-
    # clause modal grammar (migrated from whole-spell exact-text in
    # bespoke-spells3). The "exactly-one-parseable-mode" fallback returns
    # mode 1's effect when modes 2 and 3 are unrecognized.
    assert "Untimely Malfunction" in _claimed_names()


def test_untimely_malfunction_engine_class():
    # Mode 1 "Destroy target artifact." parses as ``DestroyTargetArtifact()``.
    # Modes 2 and 3 unrecognized → exactly-one fallback returns mode 1.
    from mtgz_ir.cards.base import DestroyTargetArtifact

    parser = OracleParserCardSource()
    p = parser.get_printings()["Untimely Malfunction"]
    assert p.spell_effect == DestroyTargetArtifact()


def test_untimely_malfunction_is_instant():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Untimely Malfunction"]
    assert "Instant" in p.card_types


def test_untimely_malfunction_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Untimely Malfunction"]
    assert p.mana_cost_str == "{1}{R}"


# ---------- Unnerving Grasp ----------

def test_unnerving_grasp_claimed_by_parser():
    # Unnerving Grasp ``{2}{U}`` Sorcery — now claimed via the per-clause
    # grammar (migrated from the whole-spell exact-text table in
    # bespoke-spells3).
    assert "Unnerving Grasp" in _claimed_names()


def test_unnerving_grasp_engine_class():
    # In bespoke-spells3, Unnerving Grasp was migrated from the whole-spell
    # exact-text table to the per-clause grammar. The new
    # ``_RETURN_UP_TO_ONE_TARGET_NONLAND_PERMANENT_TO_OWNERS_HAND`` recognizer
    # and the ``Sequence.target_count_range()`` fix produce
    # ``Sequence(ReturnTargetCreatureToOwnersHand(allow_nonland_permanent=True,
    # optional=True), ManifestDread(count=1))`` — more faithful than the
    # previous ``ManifestDread(count=1)`` simplification (which dropped the
    # optional bounce entirely).
    from mtgz_ir.cards.base import ManifestDread, ReturnTargetCreatureToOwnersHand, Sequence

    parser = OracleParserCardSource()
    p = parser.get_printings()["Unnerving Grasp"]
    assert p.spell_effect == Sequence(effects=(
        ReturnTargetCreatureToOwnersHand(allow_nonland_permanent=True, optional=True),
        ManifestDread(count=1),
    ))


def test_unnerving_grasp_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Unnerving Grasp"]
    assert "Sorcery" in p.card_types


def test_unnerving_grasp_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Unnerving Grasp"]
    assert p.mana_cost_str == "{2}{U}"


# ---------- CommuneWithEvilEffect engine class ----------

def test_commune_with_evil_effect_exists():
    # New bespoke engine class wired in ``mtgz_ir.effects.bespoke``
    # next to ``ConsultTheStarChartsEffect``. Imports cleanly and is
    # exposed via ``mtgz_ir.effects``.
    from mtgz_ir.effects import CommuneWithEvilEffect

    eff = CommuneWithEvilEffect()
    # Untargeted spell — controller-only effect with no target picker.
    assert eff.requires_target() is False


def test_commune_with_evil_describe():
    from mtgz_ir.effects import CommuneWithEvilEffect

    desc = CommuneWithEvilEffect().describe()
    assert "top four" in desc
    assert "hand" in desc
    assert "graveyard" in desc
    assert "3 life" in desc


def test_commune_with_evil_resolves_split_and_life_gain():
    # End-to-end: 4-card library, agent picks card-3 to hand; cards 0,
    # 1, 2 go to graveyard; controller gains 3 life.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import CommuneWithEvilEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    state = GameState(
        players=[
            Player(id=0, life=20),
            Player(id=1, life=20),
        ],
    )
    # Stock 4 cards in P0's library.
    cards = []
    for i in range(4):
        c = GameObject(
            id=100 + i, printing=bear_pr, owner_id=0, controller_id=0,
            zone=Zone.LIBRARY,
        )
        state.players[0].library.append(c)
        cards.append(c)

    # Stub agent that always picks the third examined card (id 102).
    class PickThird:
        def choose_card_from_library(self, state, player_id, option_ids):
            return 102

        def __getattr__(self, name):
            raise AttributeError(name)

    state.players[0].agent = PickThird()
    CommuneWithEvilEffect().resolve(state, controller_id=0)
    # Card 102 went to hand; the other three to graveyard.
    assert cards[2] in state.players[0].hand
    assert cards[0] in state.players[0].graveyard
    assert cards[1] in state.players[0].graveyard
    assert cards[3] in state.players[0].graveyard
    # Library now empty (we examined all four).
    assert state.players[0].library == []
    # Controller gained 3 life (20 → 23).
    assert state.players[0].life == 23


def test_commune_with_evil_no_agent_default_picks_top():
    # Without an agent seated, the controller's pick defaults to the
    # top card (matches the existing
    # ``LookTopNPutOneInHandRestToGraveyard``/Codecracker Hound
    # convention for unit-test paths). The other three examined cards
    # go to graveyard.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import CommuneWithEvilEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    state = GameState(
        players=[
            Player(id=0, life=20),
            Player(id=1, life=20),
        ],
    )
    cards = []
    for i in range(4):
        c = GameObject(
            id=200 + i, printing=bear_pr, owner_id=0, controller_id=0,
            zone=Zone.LIBRARY,
        )
        state.players[0].library.append(c)
        cards.append(c)
    state.players[0].agent = None
    CommuneWithEvilEffect().resolve(state, controller_id=0)
    # Top card (cards[0]) went to hand; the other three to graveyard.
    assert cards[0] in state.players[0].hand
    assert cards[1] in state.players[0].graveyard
    assert cards[2] in state.players[0].graveyard
    assert cards[3] in state.players[0].graveyard
    assert state.players[0].life == 23


def test_commune_with_evil_short_library_uses_what_is_available():
    # Library has only 2 cards — the spell examines both; one to hand,
    # one to graveyard. Life gain still fires.
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import CommuneWithEvilEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    state = GameState(
        players=[
            Player(id=0, life=20),
            Player(id=1, life=20),
        ],
    )
    cards = []
    for i in range(2):
        c = GameObject(
            id=300 + i, printing=bear_pr, owner_id=0, controller_id=0,
            zone=Zone.LIBRARY,
        )
        state.players[0].library.append(c)
        cards.append(c)
    state.players[0].agent = None
    CommuneWithEvilEffect().resolve(state, controller_id=0)
    assert cards[0] in state.players[0].hand
    assert cards[1] in state.players[0].graveyard
    assert state.players[0].library == []
    assert state.players[0].life == 23


def test_commune_with_evil_empty_library_only_gains_life():
    # Library has 0 cards — the spell skips the look entirely (no
    # cards to examine) and fires the life-gain rider regardless.
    from mtgz_ir.effects import CommuneWithEvilEffect
    from mtgz_ir.game import GameState, Player

    state = GameState(
        players=[Player(id=0, life=20), Player(id=1, life=20)],
    )
    CommuneWithEvilEffect().resolve(state, controller_id=0)
    assert state.players[0].life == 23


def test_commune_with_evil_agent_invalid_pick_falls_back_to_top():
    # If the agent picks an id not in the examined pool, the spell
    # falls back to the top card going to hand (defensive: matches
    # ``LookTopNPutOneInHandRestToGraveyard``'s default-on-invalid
    # convention — same default convention previously documented on
    # the now-folded LookAtTopTwoSplitHandGraveyard — prevents agent
    # bugs from desynchronizing the resolve).
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.effects import CommuneWithEvilEffect
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    state = GameState(
        players=[Player(id=0, life=20), Player(id=1, life=20)],
    )
    cards = []
    for i in range(4):
        c = GameObject(
            id=400 + i, printing=bear_pr, owner_id=0, controller_id=0,
            zone=Zone.LIBRARY,
        )
        state.players[0].library.append(c)
        cards.append(c)

    class BogusPick:
        def choose_card_from_library(self, state, player_id, option_ids):
            return 99999  # not in the pool

    state.players[0].agent = BogusPick()
    CommuneWithEvilEffect().resolve(state, controller_id=0)
    assert cards[0] in state.players[0].hand
    assert cards[1] in state.players[0].graveyard
    assert cards[2] in state.players[0].graveyard
    assert cards[3] in state.players[0].graveyard


# ---------- Commune with Evil parser claim ----------

def test_commune_with_evil_claimed_by_parser():
    # Commune with Evil ``{2}{B}`` Sorcery — now claimed via the
    # per-clause grammar (migrated from the whole-spell exact-text
    # table in bespoke-spells3).
    assert "Commune with Evil" in _claimed_names()


def test_commune_with_evil_engine_class():
    # In bespoke-spells3, Commune with Evil was migrated from the
    # whole-spell exact-text table to the per-clause grammar. The new
    # ``_LOOK_TOP_N_PUT_ONE_IN_HAND_REST_TO_GRAVEYARD_GAIN_M_LIFE``
    # recognizer produces ``Sequence(LookTopNPutOneInHandRestToGraveyard(4),
    # GainLife(3))`` — more faithful than the previous atomic
    # ``CommuneWithEvilEffect()`` class.
    from mtgz_ir.cards.base import GainLife, LookTopNPutOneInHandRestToGraveyard, Sequence

    parser = OracleParserCardSource()
    p = parser.get_printings()["Commune with Evil"]
    assert p.spell_effect == Sequence(effects=(
        LookTopNPutOneInHandRestToGraveyard(n=4),
        GainLife(amount=3),
    ))


def test_commune_with_evil_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Commune with Evil"]
    assert "Sorcery" in p.card_types


def test_commune_with_evil_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Commune with Evil"]
    assert p.mana_cost_str == "{2}{B}"


# ---------- Turn Inside Out ----------

def test_turn_inside_out_claimed_by_parser():
    # Turn Inside Out ``{R}`` Instant — claimed via the new whole-
    # spell exact-text dispatch entry that simplifies the printed
    # text (+3/+0 UEOT + death-triggered manifest-dread rider) to
    # bare ``PumpTargetCreatureUntilEOT(power_bonus=3,
    # toughness_bonus=0)``. Reminder text inside the manifest
    # parenthetical is stripped before dispatch.
    assert "Turn Inside Out" in _claimed_names()


def test_turn_inside_out_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified pump engine class — power bonus preserved,
    # death-triggered manifest-dread rider dropped (parser-is-right
    # precedent).
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT

    parser = OracleParserCardSource()
    p = parser.get_printings()["Turn Inside Out"]
    assert p.spell_effect == PumpTargetCreatureUntilEOT(
        power_bonus=3,
        toughness_bonus=0,
    )


def test_turn_inside_out_is_instant():
    # Type-line carries Instant — not Sorcery — so the spell can be
    # cast in response to opponent's combat for a +3/+0 buff.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Turn Inside Out"]
    assert "Instant" in p.card_types


def test_turn_inside_out_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Turn Inside Out"]
    assert p.mana_cost_str == "{R}"
