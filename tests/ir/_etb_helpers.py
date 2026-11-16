"""Test helpers for the predicate-trigger migration.

Phase 2 migrated the ETB-event family (``"etb"``, ``"landfall"``,
``"artifact_enters_you_control"``, etc.) from string-key
``trigger=`` to the predicate-style ``event=Event.ENTERS,
predicate=...`` shape. Phase 4 extended the same migration to the
DIES / LEAVES_BATTLEFIELD families (``"dies"``,
``"leaves_battlefield"``,
``"creature_you_control_with_counter_dies"``,
``"nontoken_creature_you_control_with_counter_dies"``,
``"another_creature_you_control_dies_or_exiled"``,
``"your_creature_or_artifact_dies"``). Phase 5 migrated the ATTACKS
family — currently just the ``"attacks"`` self-attack key (the
aura/equipment ``"enchanted_creature_attacks"`` and
``"equipped_creature_attacks"`` relational variants stay on the
string-key path until a ``Host`` predicate lands). Phase 6 migrated
the turn-structure family (``"beginning_of_upkeep"``,
``"beginning_of_combat"``, ``"beginning_of_end_step"``, and the
``"beginning_of_each_end_step"`` variant) — these events carry a
Player subject (rather than a permanent), so the inferred-key
mapping pivots on whether the predicate is ``Controlled(YOU)`` (the
"your" variant) or ``Always()`` (the "each" variant — only present
for the end-step event today).

Existing tests that scan trigger-key strings
(``ab.trigger == "etb"``, ``"landfall" in {ab.trigger ...}``) need
to recognize either the legacy or the migrated shape, since some
abilities on a printing may be on the predicate path while others
are still on the string-key path (BLOCKS, deals_combat_damage, etc.).

These helpers paper over the difference for assertion code only —
production code never calls back into them. They are intentionally
narrow: they map a predicate-style ability back to the legacy
string key its (event, predicate) pair is structurally equivalent
to, and otherwise return the literal ``ability.trigger`` field.
"""

from __future__ import annotations

from mtgz_ir.effects.object_predicates import (
    HasCounter,
    HasKeyword,
    HasSubtype,
    IsArtifact,
    IsCreature,
    IsLand,
    IsNontoken,
    IsPlayer,
    ObjectAnd,
    ObjectNot,
    ObjectOr,
)
from mtgz_ir.effects.state_predicates import SpellsCastThisTurn
from mtgz_ir.effects.predicates import (
    OPPONENT,
    YOU,
    Always,
    And,
    Controlled,
    Event,
    IsHost,
    IsSelf,
    Not,
    Or,
    StateBound,
    Subject,
)


def _flatten_and(pred):
    """Flatten a left-associated trigger-level ``And`` tree into a list
    of trigger-level leaves. Subject / Source / StateBound / IsSelf /
    IsHost / Controlled / Not / Or stay as-is — callers that want to
    look INSIDE a Subject's ObjectPredicate tree use
    ``_object_atoms_in`` to walk the inner tree.
    """
    if isinstance(pred, And):
        return _flatten_and(pred.left) + _flatten_and(pred.right)
    return [pred]


def _object_atoms_in(pred):
    """Walk an ObjectPredicate tree (ObjectAnd / ObjectOr / atoms) and
    return the flattened list of atomic ObjectPredicates. Stops at Or
    boundaries (returns the Or itself as a single 'leaf' so callers
    can pattern-match on its operand types — same convention the
    pre-refactor leaf walker used)."""
    if isinstance(pred, ObjectAnd):
        return _object_atoms_in(pred.left) + _object_atoms_in(pred.right)
    return [pred]


def _subject_inner_atoms(leaves, slot=None):
    """Across the given trigger-level leaves, return the flattened set
    of inner ObjectPredicate atoms inside any matching Subject(slot, …)
    binding. ``slot=None`` means "any slot" — useful for the legacy
    keys that didn't disambiguate by slot. Returns a list (preserving
    duplicates) so callers can examine ``isinstance(atom, Or)`` etc."""
    atoms = []
    for leaf in leaves:
        if not isinstance(leaf, Subject):
            continue
        if slot is not None and leaf.slot != slot:
            continue
        atoms.extend(_object_atoms_in(leaf.inner))
    return atoms


def _legacy_key_for_enters(ability) -> str | None:
    """Map a predicate-style ENTERS ability back to its legacy key."""
    leaves = _flatten_and(ability.predicate)
    has_self = any(isinstance(leaf, IsSelf) for leaf in leaves)
    has_not_self = any(
        isinstance(leaf, Not) and isinstance(leaf.inner, IsSelf)
        for leaf in leaves
    )
    has_controlled = any(isinstance(leaf, Controlled) for leaf in leaves)
    inner_atoms = _subject_inner_atoms(leaves)
    inner_types = {type(a) for a in inner_atoms}
    has_land = IsLand in inner_types
    has_artifact = IsArtifact in inner_types
    has_nontoken = IsNontoken in inner_types
    has_creature = IsCreature in inner_types
    kavu_subtype = any(
        isinstance(a, HasSubtype) and a.name == "Kavu"
        for a in inner_atoms
    )
    # Object-tier Or (creature or artifact).
    has_creature_or_artifact_or = any(
        isinstance(a, ObjectOr)
        and {type(a.left), type(a.right)} == {IsCreature, IsArtifact}
        for a in inner_atoms
    )
    # Self ETB.
    if has_self and len(leaves) == 1:
        return "etb"
    # Landfall.
    if has_land and has_controlled and not has_artifact:
        return "landfall"
    # Kavu observer.
    if kavu_subtype and has_controlled and has_not_self:
        return "kavu_you_control_enters"
    # Nontoken artifact.
    if has_artifact and has_nontoken and has_controlled and has_not_self:
        return "nontoken_artifact_enters_you_control"
    # Creature-or-artifact (ObjectOr inside a Subject).
    if has_creature_or_artifact_or and has_controlled and has_not_self:
        return "creature_or_artifact_enters_you_control"
    # Plain artifact.
    if has_artifact and has_controlled and has_not_self and not has_nontoken:
        return "artifact_enters_you_control"
    return None


def _legacy_key_for_dies(ability) -> str | None:
    """Map a predicate-style DIES ability back to its legacy key."""
    leaves = _flatten_and(ability.predicate)
    has_self = any(isinstance(leaf, IsSelf) for leaf in leaves)
    has_not_self = any(
        isinstance(leaf, Not) and isinstance(leaf.inner, IsSelf)
        for leaf in leaves
    )
    has_controlled = any(isinstance(leaf, Controlled) for leaf in leaves)
    inner_atoms = _subject_inner_atoms(leaves)
    inner_types = {type(a) for a in inner_atoms}
    has_creature = IsCreature in inner_types
    has_artifact = IsArtifact in inner_types
    has_nontoken = IsNontoken in inner_types
    has_plus_one_counter = any(
        isinstance(a, HasCounter) and a.kind == "+1/+1"
        for a in inner_atoms
    )
    has_creature_or_artifact_or = any(
        isinstance(a, ObjectOr)
        and {type(a.left), type(a.right)} == {IsCreature, IsArtifact}
        for a in inner_atoms
    )
    # Self dies.
    if has_self and len(leaves) == 1:
        return "dies"
    # Creature you control with counter dies (with optional nontoken).
    if has_creature and has_controlled and has_plus_one_counter:
        if has_nontoken:
            return "nontoken_creature_you_control_with_counter_dies"
        return "creature_you_control_with_counter_dies"
    # Your creature OR artifact dies (Susurian Voidborn / Sothera).
    if has_creature_or_artifact_or and has_controlled and not has_not_self:
        return "your_creature_or_artifact_dies"
    # Sothera-style: creature you control dies (no Or, no counter).
    if (
        has_creature and has_controlled
        and not has_plus_one_counter
        and not has_creature_or_artifact_or
        and not has_nontoken
    ):
        return "your_creature_or_artifact_dies"
    return None


def _legacy_key_for_leaves(ability) -> str | None:
    """Map a predicate-style LEAVES_BATTLEFIELD ability back to its
    legacy key. The ``another_creature_you_control_dies_or_exiled``
    pattern collapses cleanly to LEAVES_BATTLEFIELD since both death
    and exile leave the battlefield."""
    leaves = _flatten_and(ability.predicate)
    has_self = any(isinstance(leaf, IsSelf) for leaf in leaves)
    has_not_self = any(
        isinstance(leaf, Not) and isinstance(leaf.inner, IsSelf)
        for leaf in leaves
    )
    has_controlled = any(isinstance(leaf, Controlled) for leaf in leaves)
    inner_atoms = _subject_inner_atoms(leaves)
    has_creature = IsCreature in {type(a) for a in inner_atoms}
    # Self leaves.
    if has_self and len(leaves) == 1:
        return "leaves_battlefield"
    # Another creature you control dies or is exiled.
    if has_creature and has_controlled and has_not_self:
        return "another_creature_you_control_dies_or_exiled"
    return None


def _legacy_key_for_attacks(ability, source_printing=None) -> str | None:
    """Map a predicate-style ATTACKS ability back to its legacy key."""
    leaves = _flatten_and(ability.predicate)
    if len(leaves) == 1 and isinstance(leaves[0], IsSelf):
        return "attacks"
    # Aura/Equipment "Whenever enchanted/equipped creature attacks":
    # IsHost(subject="attacker") on the aura/Equipment as the source.
    if (
        len(leaves) == 1
        and isinstance(leaves[0], IsHost)
        and leaves[0].subject == "attacker"
    ):
        if source_printing is not None and source_printing.is_aura:
            return "enchanted_creature_attacks"
        return "equipped_creature_attacks"
    return None


def _legacy_key_for_becomes_tapped(ability) -> str | None:
    """Map a predicate-style BECOMES_TAPPED ability back to its legacy
    key.

    Two shapes:
      - ``IsSelf()`` → ``becomes_tapped`` ("this creature becomes
        tapped" on the tapped permanent itself).
      - ``IsHost()`` → ``enchanted_permanent_becomes_tapped`` (Aura
        "whenever enchanted permanent becomes tapped" — Cryoshatter).
    """
    leaves = _flatten_and(ability.predicate)
    if len(leaves) == 1 and isinstance(leaves[0], IsSelf):
        return "becomes_tapped"
    if len(leaves) == 1 and isinstance(leaves[0], IsHost):
        return "enchanted_permanent_becomes_tapped"
    return None


def _legacy_key_for_dealt_damage(ability) -> str | None:
    """Map a predicate-style DEALT_DAMAGE ability back to its legacy
    key. Phase 7 migrated the aura ``enchanted_creature_dealt_damage``
    family via ``IsHost()`` (Cryoshatter, Pain for All)."""
    leaves = _flatten_and(ability.predicate)
    if len(leaves) == 1 and isinstance(leaves[0], IsHost):
        return "enchanted_creature_dealt_damage"
    return None


def _legacy_key_for_is_cast(ability) -> str | None:
    """Map a predicate-style IS_CAST ability back to its legacy key.

    Three shapes recognized:
      - ``Controlled(YOU, "controller")`` alone → spell half of
        ``cast_spell_or_play_land``.
      - ``Subject("spell", IsArtifact()) & Controlled(YOU, "controller")``
        → ``cast_artifact_spell``.
      - ``Controlled(YOU, "controller") & StateBound(SpellsCastThisTurn(n=2))``
        → ``second_spell_each_turn``.
    """
    leaves = _flatten_and(ability.predicate)
    has_artifact_spell = any(
        isinstance(leaf, Subject)
        and leaf.slot == "spell"
        and any(isinstance(a, IsArtifact) for a in _object_atoms_in(leaf.inner))
        for leaf in leaves
    )
    has_controlled_you_controller = any(
        isinstance(leaf, Controlled)
        and leaf.by == YOU
        and leaf.subject == "controller"
        for leaf in leaves
    )
    has_second_spell = any(
        isinstance(leaf, StateBound)
        and isinstance(leaf.inner, SpellsCastThisTurn)
        and leaf.inner.n == 2
        for leaf in leaves
    )
    if has_second_spell and has_controlled_you_controller:
        return "second_spell_each_turn"
    if has_artifact_spell and has_controlled_you_controller:
        return "cast_artifact_spell"
    if (
        has_controlled_you_controller
        and len(leaves) == 1
    ):
        return "cast_spell_or_play_land"
    return None


def _legacy_key_for_plays_land(ability) -> str | None:
    """Map a predicate-style PLAYS_LAND ability back to its legacy key.

    Two shapes:
      - ``Controlled(by=OPPONENT, subject="controller")`` →
        ``opponent_plays_land`` (Burgeoning).
      - ``Controlled(YOU, subject="controller")`` → land half of
        ``cast_spell_or_play_land`` (The Endstone).
    """
    leaves = _flatten_and(ability.predicate)
    if (
        len(leaves) == 1
        and isinstance(leaves[0], Controlled)
        and leaves[0].subject == "controller"
    ):
        if leaves[0].by == OPPONENT:
            return "opponent_plays_land"
        if leaves[0].by == YOU:
            return "cast_spell_or_play_land"
    return None


def _legacy_key_for_discards_card(ability) -> str | None:
    """Map a predicate-style DISCARDS_CARD ability back to its legacy
    key. Only shape today is ``Controlled(by=OPPONENT)`` on the
    discarding-player subject (Entropic Battlecruiser)."""
    leaves = _flatten_and(ability.predicate)
    if (
        len(leaves) == 1
        and isinstance(leaves[0], Controlled)
        and leaves[0].by == OPPONENT
    ):
        return "opponent_discards_a_card"
    return None


def _legacy_key_for_counters_added(ability) -> str | None:
    """Map a predicate-style COUNTERS_ADDED ability back to its legacy
    key. Only shape today is ``Subject(None, IsCreature()) &
    Controlled(YOU) & CountersPutBy(YOU)`` on the receiving creature
    (Terrasymbiosis: counters you put on a creature you control)."""
    from mtgz_ir.effects.generic.eoe_fidelity import CountersPutBy

    leaves = _flatten_and(ability.predicate)
    inner_atoms = _subject_inner_atoms(leaves)
    has_creature = any(isinstance(a, IsCreature) for a in inner_atoms)
    has_controlled = any(isinstance(leaf, Controlled) for leaf in leaves)
    put_by_you = [
        leaf for leaf in leaves
        if isinstance(leaf, CountersPutBy) and leaf.by == YOU
    ]
    if (
        has_creature
        and has_controlled
        and len(put_by_you) == 1
        and len(leaves) == 3
    ):
        return "counters_added_to_your_creature"
    return None


def _legacy_key_for_is_sacrificed(ability) -> str | None:
    """Map a predicate-style IS_SACRIFICED ability back to its legacy
    key.

    Two shapes:
      - ``Subject(None, IsArtifact()) & Controlled(YOU) & ~IsSelf()``
        (Biotech Specialist) → ``sacrifice_artifact``.
      - ``Subject(None, IsCreature() | IsArtifact()) & Controlled(YOU)
        & ~IsSelf()`` (Lightless Evangel) →
        ``sacrifice_creature_or_artifact``.
    """
    leaves = _flatten_and(ability.predicate)
    inner_atoms = _subject_inner_atoms(leaves)
    has_or_creature_artifact = any(
        isinstance(a, ObjectOr)
        and {type(a.left), type(a.right)} == {IsCreature, IsArtifact}
        for a in inner_atoms
    )
    has_artifact = any(isinstance(a, IsArtifact) for a in inner_atoms)
    has_controlled = any(isinstance(leaf, Controlled) for leaf in leaves)
    has_not_self = any(
        isinstance(leaf, Not) and isinstance(leaf.inner, IsSelf)
        for leaf in leaves
    )
    if has_or_creature_artifact and has_controlled and has_not_self:
        return "sacrifice_creature_or_artifact"
    if has_artifact and has_controlled and has_not_self:
        return "sacrifice_artifact"
    return None


def _legacy_key_for_player_phase(ability, your_key: str) -> str | None:
    """Map a predicate-style turn-structure ability back to its legacy
    string key. Used by all three of BEGINNING_OF_UPKEEP /
    BEGINNING_OF_COMBAT / BEGINNING_OF_END_STEP.

    ``Controlled(YOU)`` (with no other clauses) is the "your <phase>"
    form and maps to ``your_key`` — i.e. ``"beginning_of_upkeep"``,
    ``"beginning_of_combat"``, ``"beginning_of_end_step"``. ``Always()``
    paired with BEGINNING_OF_END_STEP is the "each end step" form and
    maps to ``"beginning_of_each_end_step"``; for the upkeep / combat
    events there's no "each" variant in the static-card pool today so
    ``Always()`` simply has no inferred legacy key.
    """
    leaves = _flatten_and(ability.predicate)
    if (
        len(leaves) == 1
        and isinstance(leaves[0], Controlled)
        and leaves[0].subject is None
    ):
        return your_key
    # Intervening-if gate: ``Controlled(YOU) & StateBound(...)`` — the
    # turn-structure predicate is still the "your <phase>" form, just
    # with a state-predicate gate AND-ed on top (Sothera's
    # ``AnyPlayerControlsNoCreatures()`` end-step gate). The base
    # predicate is the same legacy event, so map to the same key.
    if (
        len(leaves) == 2
        and any(
            isinstance(leaf, Controlled) and leaf.subject is None
            for leaf in leaves
        )
        and any(isinstance(leaf, StateBound) for leaf in leaves)
    ):
        return your_key
    if (
        your_key == "beginning_of_end_step"
        and len(leaves) == 1
        and isinstance(leaves[0], Always)
    ):
        return "beginning_of_each_end_step"
    return None


def _legacy_key_for_predicate(ability, source_printing=None) -> str | None:
    """Return the legacy string key a predicate-style ability is
    structurally equivalent to, or ``None`` if no mapping applies.

    The mapping mirrors the rewriting done in
    ``src/mtgz_ir/cards/parser/triggers.py``'s
    ``_PREDICATE_TRIGGER_PHRASES`` table and the static-card
    migrations from phases 2, 4, 5, 6, and 7.

    ``source_printing`` is consulted only by the ATTACKS branch to
    distinguish aura ``enchanted_creature_attacks`` from equipment
    ``equipped_creature_attacks`` (both share the same predicate
    shape ``IsHost(subject="attacker")``); pass ``None`` and ATTACKS
    via IsHost defaults to the equipment key.
    """
    if ability.event is None or ability.predicate is None:
        return None
    if ability.event is Event.ENTERS:
        return _legacy_key_for_enters(ability)
    if ability.event is Event.DIES:
        return _legacy_key_for_dies(ability)
    if ability.event is Event.LEAVES_BATTLEFIELD:
        return _legacy_key_for_leaves(ability)
    if ability.event is Event.ATTACKS:
        return _legacy_key_for_attacks(ability, source_printing)
    if ability.event is Event.BECOMES_TAPPED:
        return _legacy_key_for_becomes_tapped(ability)
    if ability.event is Event.DEALT_DAMAGE:
        return _legacy_key_for_dealt_damage(ability)
    if ability.event is Event.IS_CAST:
        return _legacy_key_for_is_cast(ability)
    if ability.event is Event.IS_SACRIFICED:
        return _legacy_key_for_is_sacrificed(ability)
    if ability.event is Event.PLAYS_LAND:
        return _legacy_key_for_plays_land(ability)
    if ability.event is Event.DISCARDS_CARD:
        return _legacy_key_for_discards_card(ability)
    if ability.event is Event.COUNTERS_ADDED:
        return _legacy_key_for_counters_added(ability)
    if ability.event is Event.BEGINNING_OF_UPKEEP:
        return _legacy_key_for_player_phase(ability, "beginning_of_upkeep")
    if ability.event is Event.BEGINNING_OF_COMBAT:
        return _legacy_key_for_player_phase(ability, "beginning_of_combat")
    if ability.event is Event.BEGINNING_OF_END_STEP:
        return _legacy_key_for_player_phase(ability, "beginning_of_end_step")
    if ability.event is Event.DEALS_COMBAT_DAMAGE:
        return _legacy_key_for_deals_combat_damage(ability)
    if ability.event is Event.BLOCKS:
        return _legacy_key_for_blocks(ability)
    return None


def _legacy_key_for_deals_combat_damage(ability) -> str | None:
    """Map a predicate-style DEALS_COMBAT_DAMAGE ability back to its
    legacy key.

    - ``IsSelf(subject="source") & Subject("target", IsPlayer())`` →
      ``combat_damage_to_player`` (Illvoi Infiltrator).
    - ``Subject("source", IsCreature() & HasCounter("+1/+1")) &
      Controlled(YOU, "source") & Subject("target", IsPlayer())``
      with ``once_per_turn=True`` →
      ``your_counter_creature_combat_damage_to_player_aggregated``
      (Haliya, Ascendant Cadet — grammar-produced predicate form).
    - ``Subject("source", IsCreature()) & Controlled(YOU, "source") &
      Subject("target", IsPlayer())`` →
      ``your_creature_combat_damage_to_player`` (Starwinder).
    - ``Subject("source", IsCreature()) & Controlled(YOU, "target")`` →
      ``creature_deals_combat_damage_to_you`` (Contested War Zone).
    """
    leaves = _flatten_and(ability.predicate)
    has_self_source = any(
        isinstance(leaf, IsSelf) and leaf.subject == "source"
        for leaf in leaves
    )
    has_creature_source = any(
        isinstance(leaf, Subject)
        and leaf.slot == "source"
        and any(isinstance(a, IsCreature) for a in _object_atoms_in(leaf.inner))
        for leaf in leaves
    )
    has_counter_source = any(
        isinstance(leaf, Subject)
        and leaf.slot == "source"
        and any(
            isinstance(a, HasCounter) and a.kind == "+1/+1"
            for a in _object_atoms_in(leaf.inner)
        )
        for leaf in leaves
    )
    has_controlled_you_source = any(
        isinstance(leaf, Controlled)
        and leaf.by == YOU
        and leaf.subject == "source"
        for leaf in leaves
    )
    has_controlled_you_target = any(
        isinstance(leaf, Controlled)
        and leaf.by == YOU
        and leaf.subject == "target"
        for leaf in leaves
    )
    has_player_target = any(
        isinstance(leaf, Subject)
        and leaf.slot == "target"
        and any(isinstance(a, IsPlayer) for a in _object_atoms_in(leaf.inner))
        for leaf in leaves
    )
    if has_self_source and has_player_target:
        return "combat_damage_to_player"
    # Counter-creature-aggregated: once_per_turn + counter filter (Haliya
    # grammar form). Checked before the generic your-creature shape to
    # avoid the more-specific counter shape accidentally claiming the
    # generic key (the counter filter is additive — every counter-creature
    # trigger also satisfies IsCreature, so specificity must win here).
    if (
        has_creature_source
        and has_counter_source
        and has_controlled_you_source
        and has_player_target
        and ability.once_per_turn
    ):
        return "your_counter_creature_combat_damage_to_player_aggregated"
    if (
        has_creature_source
        and has_controlled_you_source
        and has_player_target
    ):
        return "your_creature_combat_damage_to_player"
    if has_creature_source and has_controlled_you_target:
        return "creature_deals_combat_damage_to_you"
    return None


def _legacy_key_for_blocks(ability) -> str | None:
    """Map a predicate-style BLOCKS ability back to its legacy key.

    Only shape today:
    - ``IsSelf(subject="blocker") & Subject("attacker",
      HasKeyword("flying"))`` → ``blocks_creature_with_flying``
      (Skystinger).
    """
    leaves = _flatten_and(ability.predicate)
    has_self_blocker = any(
        isinstance(leaf, IsSelf) and leaf.subject == "blocker"
        for leaf in leaves
    )
    has_flying_attacker = any(
        isinstance(leaf, Subject)
        and leaf.slot == "attacker"
        and any(
            isinstance(a, HasKeyword) and a.name == "flying"
            for a in _object_atoms_in(leaf.inner)
        )
        for leaf in leaves
    )
    if has_self_blocker and has_flying_attacker:
        return "blocks_creature_with_flying"
    return None


def _legacy_or_inferred_key(ability, source_printing=None) -> str:
    """Return ``ability.trigger`` if non-empty, else infer the legacy
    key from the predicate-style fields. Tests that built sets of
    trigger keys for membership checks use this so the assertion logic
    survives the predicate migration intact.

    Pass ``source_printing`` (the ``CardPrinting`` whose
    ``triggered_abilities`` contains the ability) when the inference
    needs to disambiguate aura-vs-equipment shapes — currently just
    the ATTACKS-via-IsHost branch."""
    if ability.trigger:
        return ability.trigger
    inferred = _legacy_key_for_predicate(ability, source_printing)
    return inferred or ""


def _matches_legacy_key(ability, key: str) -> bool:
    """Equivalent to ``ability.trigger == key`` pre-migration; after
    migration also matches the predicate-style ENTERS shape that
    corresponds to ``key``."""
    return _legacy_or_inferred_key(ability) == key
