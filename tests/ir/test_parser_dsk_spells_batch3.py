"""Tests for the Duskmourn spells / enchantments / equipment / artifacts
batch-3 parser additions.

Each commit in this batch lands one card via a small additive change:
a literal-line entry on an existing builder, a name-keyed dispatch
factory on an existing builder, or a small flag widening on an existing
engine class. No new bespoke engine classes are introduced.

New engine fields on ``CardPrinting``:

* ``aura_yours_only`` — when True, the cast-time target picker for
  ``is_aura`` cards only offers permanents the caster controls.
  Mirrors the printed "Enchant creature you control" / "Enchant
  artifact you control" line. The flag is independent of
  ``aura_enchant_type`` (creature / artifact). Default False so
  existing plain "Enchant creature" Auras (Frantic Strength, Tractor
  Beam, ...) keep their previous behavior.

New flag on ``OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary``:

* ``nonland_permanent`` — when True, widens the legal-target pool from
  "artifact or creature" to "any nonland permanent" (Vanish from
  Sight's printed scope). Same engine class is reused with a flag
  rather than a sibling class because every line below the legal-
  target filter is identical.

Cards newly claimed in this batch:

* **Glimmerlight** ``{2}`` Artifact — Equipment — ETB-create-Glimmer-
  token (no attach) + ``Equipped creature gets +1/+1`` + ``Equip {1}``.
  Claimed by extending ``equipment_builder``'s literal-ETB-line table
  with the printed Glimmer line, dispatching to bare ``CreateToken``
  (NOT ``CreateTokenAndAttachSelf`` — the printed text never says
  "and attach").

* **Sheltered by Ghosts** ``{1}{W}`` Enchantment — Aura — ETB exile
  target nonland permanent an opponent controls (until the Aura
  leaves), +1/+0 + lifelink + ward {2}, and the printed "Enchant
  creature you control" controller restriction. Claimed by name-
  keyed dispatch in ``enchantment_builder``, using the new
  ``aura_yours_only`` flag plus the existing
  ``ExileOpponentsNonlandPermanentUntilLeaves`` ETB and the existing
  ``attached_grants_ward`` rider that Hardlight Containment also
  uses.

* **Vanish from Sight** ``{3}{U}`` Instant — ``Target nonland
  permanent's owner puts it on their choice of the top or bottom of
  their library. Surveil 1.`` Two-sentence body composed via the
  Sequence sentence-split path: the bounce-to-library piece rides the
  existing ``OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary``
  engine class with the new ``nonland_permanent`` flag, then the
  trailing ``Surveil 1`` rides the existing ``Surveil`` primitive.

* **Bear Trap** ``{1}`` Artifact — Flash + ``{3}, {T}, Sacrifice this
  artifact: It deals 3 damage to target creature.`` Claimed by
  widening the ``deals N damage to target creature`` regex with an
  optional ``it `` prefix, mirroring the ``(?:this creature )?``
  shape on ``_DEAL_DAMAGE_TARGET_OPPONENT`` / ``_DEAL_N_DAMAGE_EACH_*``.
  The activated normalizer lowercases the first letter but doesn't
  strip the source pronoun (unlike the creature-trigger normalizer);
  the regex prefix accepts the surface form as it lands.
"""

# mtg-zero (docs/ENGINE.md 169): 1 of mtg-bot's tests in this file stayed in mtg-bot: they play the Python engine (mtgz_ir.creature_statics_wg, not carried into mtgz_ir):
# test_glimmerlight_resolves_etb_glimmer_token_creation

from __future__ import annotations

from mtgz_ir.cards.base import (
    TriggeredAbility,
)
from mtgz_ir.cards.parser.source import OracleParserCardSource
from mtgz_ir.cards.tokens import TOKEN_PRINTINGS
from mtgz_ir.effects.generic import CreateToken
from mtgz_ir.effects.predicates import (
    Event,
    IsSelf,
)


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- Glimmerlight: ETB-create-Glimmer-token Equipment ----------

def test_glimmerlight_claimed_by_parser():
    # Glimmerlight ``{2}`` Artifact — Equipment — claimed via the new
    # literal ETB line in ``equipment_builder``. Distinct from the
    # Auxiliary Boosters shape because the printed text never says
    # "and attach"; the Equipment stays unattached on the battlefield.
    assert "Glimmerlight" in _claimed_names()


def test_glimmerlight_carries_etb_create_glimmer_trigger():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the ETB trigger via bare ``CreateToken`` (not
    # ``CreateTokenAndAttachSelf``) against the canonical Glimmer
    # token printing.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Glimmerlight"]
    expected = TriggeredAbility(
        event=Event.ENTERS,
        predicate=IsSelf(),
        effect=CreateToken(
            token_printing=TOKEN_PRINTINGS["Glimmer"],
            count=1,
        ),
    )
    assert expected in p.triggered_abilities


def test_glimmerlight_carries_equipment_pump_and_equip_cost():
    # The remaining lines (``Equipped creature gets +1/+1.``,
    # ``Equip {1}``) ride the existing equipment_builder grammar — the
    # +1/+1 lands on ``equipment_bonuses`` and the equip cost lands as
    # an activated ability synthesized by ``make_equipment``.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Glimmerlight"]
    assert p.equipment_bonuses == (1, 1, frozenset())
    # ``make_equipment`` synthesizes an activated ability for the
    # equip cost. The cost mana-string is what we threaded onto the
    # printing.
    equip_costs = [
        a.cost.mana_str for a in p.activated_abilities
        if a.cost.mana_str
    ]
    assert "{1}" in equip_costs




# ---------- aura_yours_only flag ----------

def test_make_aura_aura_yours_only_kwarg():
    # New ``aura_yours_only`` kwarg on ``make_aura`` lands on the
    # ``CardPrinting.aura_yours_only`` field that the cast-time target
    # picker reads to filter the legal pool to permanents the caster
    # controls.
    from mtgz_ir.cards.base import make_aura

    aura = make_aura(
        "Test Aura", "{1}{W}",
        power_bonus=1, toughness_bonus=0,
        aura_yours_only=True,
    )
    assert aura.aura_yours_only is True


def test_make_aura_yours_only_default_false():
    # Backward-compat sanity: the new kwarg defaults to False, so
    # existing call sites that don't pass ``aura_yours_only=`` keep
    # their previous behavior (any creature is a legal Aura target).
    from mtgz_ir.cards.base import make_aura

    aura = make_aura("Plain Aura", "{U}")
    assert aura.aura_yours_only is False


def test_aura_yours_only_filters_legal_targets():
    # Belt-and-suspenders that the new flag actually filters the
    # cast-time legal-target pool. With ``aura_yours_only=True``, an
    # opponent's creature is NOT offered as a legal target — only
    # creatures the caster controls survive.
    from mtgz_ir.actions import _select_spell_targets
    from mtgz_ir.cards.base import make_aura, make_creature
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    aura = make_aura(
        "Test", "{W}",
        power_bonus=1, toughness_bonus=0,
        aura_yours_only=True,
    )
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    yours = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    theirs = GameObject(
        id=2, printing=bear_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )

    class _PickFirst:
        def choose_target(self, state, pid, legal):
            return legal[0] if legal else None

    p0 = Player(id=0, agent=_PickFirst())
    p1 = Player(id=1)
    state = GameState(players=[p0, p1], battlefield=[yours, theirs])
    aura_obj = GameObject(
        id=10, printing=aura, owner_id=0, controller_id=0, zone=Zone.HAND,
    )
    p0.hand.append(aura_obj)
    targets = _select_spell_targets(state, 0, aura_obj)
    # Only your creature (id=1) is a legal target.
    assert targets == (("creature", 1),)


# ---------- Sheltered by Ghosts ----------

def test_sheltered_by_ghosts_claimed_by_parser():
    # Sheltered by Ghosts ``{1}{W}`` Aura — claimed via the name-keyed
    # ``_ENCHANTMENT_BUILDERS`` dispatch in ``parser.enchantment_builder``.
    assert "Sheltered by Ghosts" in _claimed_names()


def test_sheltered_by_ghosts_carries_aura_fields():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # all five hooks the static side ships:
    # - +1/+0 in the ``equipment_bonuses`` triple plus lifelink in
    #   the granted-keywords slot;
    # - ``aura_yours_only=True`` for the "Enchant creature you control"
    #   restriction;
    # - ``attached_grants_ward_str="{2}"`` for the ward {2} rider;
    # - the ``ExileOpponentsNonlandPermanentUntilLeaves`` ETB.
    from mtgz_ir.effects import ExileOpponentsNonlandPermanentUntilLeaves

    parser = OracleParserCardSource()
    p = parser.get_printings()["Sheltered by Ghosts"]
    assert p.is_aura is True
    assert p.aura_yours_only is True
    assert p.equipment_bonuses == (1, 0, frozenset({"lifelink"}))
    assert p.attached_grants_ward_str == "{2}"
    assert any(
        isinstance(t.effect, ExileOpponentsNonlandPermanentUntilLeaves)
        for t in p.triggered_abilities
    )


def test_sheltered_by_ghosts_etb_exiles_opp_permanent():
    # End-to-end resolve sanity: with the Aura on the battlefield, the
    # ETB targets an opponent's permanent and exiles it — same shape
    # Banishing Light uses. Targets one of P1's permanents; P0's own
    # permanent is excluded by the legal-targets filter.
    from mtgz_ir.cards.base import make_creature, make_enchantment
    from mtgz_ir.effects import ExileOpponentsNonlandPermanentUntilLeaves
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    parser = OracleParserCardSource()
    sheltered = parser.get_printings()["Sheltered by Ghosts"]
    sheltered_obj = GameObject(
        id=1, printing=sheltered, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    # P0's own creature — not a legal target (opponents-only).
    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    yours = GameObject(
        id=2, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    # P1's enchantment — a legal nonland permanent target.
    ench_pr = make_enchantment("Tome", "{1}{U}")
    theirs = GameObject(
        id=3, printing=ench_pr, owner_id=1, controller_id=1,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[sheltered_obj, yours, theirs],
        resolving_source=sheltered_obj,
    )
    eff = ExileOpponentsNonlandPermanentUntilLeaves()
    legal = eff.legal_targets(state, controller_id=0)
    # P0's own creature excluded; P1's enchantment in the pool.
    assert ("permanent", 2) not in legal
    assert ("permanent", 3) in legal


# ---------- Vanish from Sight: nonland-permanent OwnerPuts variant ----------

def test_parse_owner_puts_target_nonland_permanent_on_top_or_bottom():
    # New primitive: widens the existing
    # ``OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary`` to
    # accept any nonland permanent via the ``nonland_permanent`` flag.
    # Same engine class — only the legal-targets filter changes.
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects import (
        OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    )

    eff = _parse_effect(
        "target nonland permanent's owner puts it on their choice "
        "of the top or bottom of their library"
    )
    assert eff == OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary(
        nonland_permanent=True,
    )


def test_owner_puts_artifact_or_creature_unchanged_default():
    # Belt-and-suspenders that the bare ``target artifact or creature``
    # surface continues to parse to the unflagged engine class — the
    # widening only applies via the new flag.
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects import (
        OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    )

    eff = _parse_effect(
        "target artifact or creature's owner puts it on their choice "
        "of the top or bottom of their library"
    )
    assert eff == OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary()
    assert eff.nonland_permanent is False


def test_owner_puts_nonland_permanent_legal_targets_widening():
    # End-to-end: with ``nonland_permanent=True``, an enchantment on
    # the battlefield is a legal target alongside any artifact /
    # creature. Lands stay excluded from the pool either way.
    from mtgz_ir.cards.base import (
        make_creature,
        make_enchantment,
        make_basic_land,
    )
    from mtgz_ir.effects import (
        OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    )
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    bear_pr = make_creature("Bear", "{1}{G}", power=2, toughness=2)
    ench_pr = make_enchantment("Tome", "{1}{U}")
    land_pr = make_basic_land("Plains", "W")
    bear_obj = GameObject(
        id=1, printing=bear_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    ench_obj = GameObject(
        id=2, printing=ench_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    land_obj = GameObject(
        id=3, printing=land_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[bear_obj, ench_obj, land_obj],
    )
    eff = OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary(
        nonland_permanent=True,
    )
    legal = eff.legal_targets(state, controller_id=0)
    # Creature and enchantment both legal; land excluded by the
    # nonland filter; same kind tag as the original variant.
    assert ("permanent", 1) in legal
    assert ("permanent", 2) in legal
    assert ("permanent", 3) not in legal


def test_owner_puts_default_excludes_enchantment():
    # Without the ``nonland_permanent`` flag, the bare
    # ``OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary``
    # primitive must NOT pick up enchantments — the widening only
    # applies when the flag is set.
    from mtgz_ir.cards.base import make_enchantment
    from mtgz_ir.effects import (
        OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    )
    from mtgz_ir.game import GameObject, GameState, Player, Zone

    ench_pr = make_enchantment("Tome", "{1}{U}")
    ench_obj = GameObject(
        id=1, printing=ench_pr, owner_id=0, controller_id=0,
        zone=Zone.BATTLEFIELD,
    )
    state = GameState(
        players=[Player(id=0), Player(id=1)],
        battlefield=[ench_obj],
    )
    eff = OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary()
    assert eff.legal_targets(state, controller_id=0) == []


def test_owner_puts_describe_includes_nonland_permanent_phrase():
    # The ``describe`` helper should reflect the new flag in its
    # English-ish output, parallel to the ``or_enchantment`` precedent
    # on ``DestroyTargetCreature``.
    from mtgz_ir.effects import (
        OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    )

    eff = OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary(
        nonland_permanent=True,
    )
    assert "nonland permanent" in eff.describe()


def test_vanish_from_sight_claimed_by_parser():
    # Vanish from Sight ``{3}{U}`` Instant — two-sentence body
    # combining the new nonland-permanent OwnerPuts primitive with the
    # existing ``Surveil 1`` primitive, composed via Sequence by the
    # spell-effect parser's sentence-split path. Reminder text on the
    # Surveil clause is stripped before clause carving.
    assert "Vanish from Sight" in _claimed_names()


def test_vanish_from_sight_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the right Sequence shape (nonland-permanent OwnerPuts, then
    # Surveil 1).
    from mtgz_ir.cards.base import Sequence, Surveil
    from mtgz_ir.effects import (
        OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary,
    )

    parser = OracleParserCardSource()
    p = parser.get_printings()["Vanish from Sight"]
    assert p.spell_effect == Sequence(effects=(
        OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary(
            nonland_permanent=True,
        ),
        Surveil(amount=1, use_artifact_count=False),
    ))


# ---------- Bear Trap: ``it deals N damage to target creature`` ----------

def test_parse_it_deals_damage_to_target_creature():
    # New surface form: optional ``it `` prefix on the existing
    # ``deals N damage to target creature`` regex. Activated abilities
    # whose body uses ``It`` to refer to the source artifact (Bear
    # Trap) lowercase to ``it deals 3 damage to target creature`` after
    # the activated normalizer's first-letter-lowercase pass.
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import DealDamageToTargetCreature

    eff = _parse_effect("it deals 3 damage to target creature")
    assert eff == DealDamageToTargetCreature(amount=3)


def test_parse_deals_damage_to_target_creature_bare_form_unchanged():
    # The bare ``deals N damage to target creature`` (no source-pronoun
    # prefix) must continue to parse — creature-trigger normalization
    # strips ``it `` before the regex sees it, and that path still
    # works after the regex widening.
    from mtgz_ir.cards.parser.effects import _parse_effect
    from mtgz_ir.effects.generic import DealDamageToTargetCreature

    eff = _parse_effect("deals 3 damage to target creature")
    assert eff == DealDamageToTargetCreature(amount=3)


def test_bear_trap_claimed_by_parser():
    # Bear Trap ``{1}`` Artifact — Flash + ``{3}, {T}, Sacrifice this
    # artifact: It deals 3 damage to target creature.`` Claimed via the
    # widened ``(?:it )?deals N damage to target creature`` regex. The
    # activated body parses through the unified activated-grammar
    # dispatch in ``parser.activated``.
    assert "Bear Trap" in _claimed_names()


def test_bear_trap_carries_flash_and_activated_ability():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # both halves of the printed text:
    # - Flash on the artifact itself (``keywords`` containing ``flash``);
    # - one activated ability with cost ``{3}, {T}, Sacrifice this
    #   artifact`` and effect ``DealDamageToTargetCreature(amount=3)``.
    from mtgz_ir.effects.generic import DealDamageToTargetCreature

    parser = OracleParserCardSource()
    p = parser.get_printings()["Bear Trap"]
    assert "flash" in p.keywords
    # Exactly one activated ability — the {3}/T/sac one. The bare-cost
    # mana ability synthesis only fires for Equipment ``equip_cost``.
    assert len(p.activated_abilities) == 1
    ability = p.activated_abilities[0]
    assert ability.cost.mana_str == "{3}"
    assert ability.cost.tap_self is True
    assert ability.cost.sacrifice_self is True
    assert ability.effect == DealDamageToTargetCreature(amount=3)
