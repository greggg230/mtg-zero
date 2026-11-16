"""Real Magic cards from outside the EoE pool, used to exercise
layered-effects machinery (CR 613) that no current EoE card stresses.

Not registered in ``NONLAND_POOL`` / ``all_printings`` — these aren't
draftable, deckable, or castable through the normal card pool. Tests
import the printings directly to construct GameObjects.

Currently:

- ``LIGNIFY`` (Lorwyn) — "Enchant creature. Enchanted creature is a
  Treefolk creature with base power and toughness 0/4 and has no
  abilities." Combines layer 4 (add subtype "Treefolk"), layer 6
  (lose all abilities), and layer 7 sublayer 1 (set base P/T 0/4).
- ``CLONE`` (Alpha) — "You may have Clone enter as a copy of any
  creature on the battlefield." Layer 1 copy effect: at ETB the
  controller picks a target creature, and the clone takes that
  creature's PRINTED characteristics (CR 706.2 copiable values —
  not auras, counters, or current animations). Writes a layer-1
  ``"is_copy"`` registry entry so the layered system records the
  copy event; the entry is dropped when the clone leaves play.
- ``CYTOSHAPE`` (Saviors of Kamigawa, simplified) — "Until end
  of turn, target creature becomes a copy of another target
  creature." Sorcery, layer-1 copy effect with
  ``expires_at="cleanup"``. The cleanup-time printing revert is
  wired via ``revert_printing_if_no_layer1_copies`` called from
  ``run_cleanup_step`` after expiring registry entries.
- ``MAGICAL_HACK`` (Alpha) — "Change the text of target spell or
  permanent by replacing all instances of one basic land type with
  another." Sorcery, layer 3 text-changing. The demo targets a
  basic land and rewrites its basic-land-type subtype, which flows
  through ``effective_subtypes`` and ``effective_land_produces`` so
  the rewritten land taps for the new color. Persistent (no
  ``expires_at``) — text changes survive cleanup per CR 612.
- ``SLEIGHT_OF_MIND`` (Alpha) — "Change the text of target spell
  or permanent by replacing all instances of one color word with
  another." Sorcery, layer 3 text-changing. The demo rewrites
  ``protection_from_color`` on the target — pair with White
  Knight to flip its protection from black to a different color.
  Same persistent semantics as Magical Hack.
- ``WHITE_KNIGHT`` (Alpha) — 2/2 white Knight with first strike and
  protection from black. Used as the protection-target so layer 5
  color changes have observable consequences in combat.
- ``SCUTTLEMUTT`` (Shards of Alara) — 3/3 Scarecrow with
  ``{T}: Target creature becomes the color or colors of your choice
  until end of turn.`` Layer 5 color-changing effect that writes a
  ``"set_colors"`` registry entry with ``expires_at="cleanup"``.
- ``DRYAD_OF_THE_ILYSIAN_GROVE`` (Theros Beyond Death) — 1/5 Dryad
  with "Lands you control are every basic land type in addition
  to their other types." Layer 4 add_subtype contribution
  synthesized in ``effective_subtypes`` so it interacts correctly
  by timestamp with Magus of the Moon / Blood Moon's "set basic
  land subtype to Mountain" layer-4 effect. Magus of the Moon
  already lives in the EoE Special Guests pool, so the famous
  Dryad-vs-Blood-Moon timestamp interaction can be tested by
  pairing this Dryad with the existing Magus printing.
- ``BLOOD_MOON`` (The Dark) — Enchantment "Nonbasic lands are
  Mountains." Same layer-4 set_basic_land_subtype mechanism as
  Magus of the Moon but printed as an enchantment.
- ``PAINTERS_SERVANT`` (Shadowmoor) — Artifact "As Painter's
  Servant enters, choose a color. All cards that aren't on the
  battlefield, spells, and permanents are the chosen color in
  addition to their other colors." Wires via the
  ``paints_all_objects_chosen_color_at_etb`` printing flag and
  the per-instance ``GameObject.painter_chosen_color`` set at
  ETB. ``effective_colors`` synthesizes a layer-5 ``add_colors``
  entry from each Painter source.
- ``HUMILITY`` (Tempest) — Enchantment "All creatures lose all
  abilities and have base power and toughness 1/1." The canonical
  reference card for the layer system. Wires via the
  ``wipes_all_creature_abilities`` (layer 6) and
  ``sets_all_creature_pt_to_1_1`` (layer 7b) printing flags;
  ``_layer6_entries_for`` and ``_registry_set_base_pt`` synthesize
  per-creature entries from each Humility source.
- ``URBORG_TOMB_OF_YAWGMOTH`` (Planar Chaos) — Legendary Land
  with printed Swamp subtype and "Each land is a Swamp in
  addition to its other types." The famous test case for the
  CR 613.7 within-layer dependency rule: Blood Moon + Urborg
  always resolves to Mountain regardless of timestamps because
  Urborg's ability depends on Blood Moon's effect (Blood Moon
  rewrites Urborg's basic land type, which wipes Urborg's static
  via CR 305.6).
"""

from __future__ import annotations

from mtgz_ir.effects import (
    CytoshapeBecomeCopyUEOT,
    MagicalHackTextChangeEffect, ScuttlemuttColorChangeUEOT,
    SleightOfMindTextChangeEffect,
)
from mtgz_ir.effects.base import ActivatedAbility, ActivationCost
from mtgz_ir.cards.base import (
    CardPrinting, ManaCost, make_artifact, make_aura, make_creature,
    make_enchantment, make_sorcery, parse_mana_cost,
)


LIGNIFY: CardPrinting = make_aura(
    "Lignify", "{1}{G}",
    attached_sets_base_pt=(0, 4),
    attached_grants_lose_all_abilities=True,
    attached_grants_subtype="Treefolk",
)


CLONE: CardPrinting = make_creature(
    "Clone", "{3}{U}", 0, 0,
    enters_as_copy_of_creature=True,
)


CYTOSHAPE: CardPrinting = make_sorcery(
    "Cytoshape", "{1}{G}",
    effect=CytoshapeBecomeCopyUEOT(),
)


MAGICAL_HACK: CardPrinting = make_sorcery(
    "Magical Hack", "{U}",
    effect=MagicalHackTextChangeEffect(),
)


SLEIGHT_OF_MIND: CardPrinting = make_sorcery(
    "Sleight of Mind", "{U}",
    effect=SleightOfMindTextChangeEffect(),
)


WHITE_KNIGHT: CardPrinting = make_creature(
    "White Knight", "{W}{W}", 2, 2,
    keywords={"first strike"},
    protection_from_color="B",
)


SCUTTLEMUTT: CardPrinting = make_creature(
    "Scuttlemutt", "{4}", 3, 3,
    subtypes=("Scarecrow",),
    activated_abilities=(
        ActivatedAbility(
            cost=ActivationCost(tap_self=True),
            effect=ScuttlemuttColorChangeUEOT(),
        ),
    ),
)


DRYAD_OF_THE_ILYSIAN_GROVE: CardPrinting = make_creature(
    "Dryad of the Ilysian Grove", "{2}{G}", 1, 5,
    subtypes=("Nymph", "Dryad"),
    grants_all_basic_land_types_to_lands_you_control=True,
)


BLOOD_MOON: CardPrinting = make_enchantment(
    "Blood Moon", "{2}{R}",
    nonbasic_lands_are_mountains=True,
)


PAINTERS_SERVANT: CardPrinting = make_artifact(
    "Painter's Servant", "{2}",
    paints_all_objects_chosen_color_at_etb=True,
)


HUMILITY: CardPrinting = make_enchantment(
    "Humility", "{2}{W}",
    wipes_all_creature_abilities=True,
    sets_all_creature_pt_to_1_1=True,
)


# Urborg, Tomb of Yawgmoth: Legendary Land — Swamp with the static
# "Each land is a Swamp in addition to its other types." Wired via
# the ``grants_basic_land_type_to_all_lands`` printing flag — the
# ``effective_subtypes`` synthesis adds the Swamp subtype to every
# land on the battlefield, suppressed if Urborg itself has been
# rewritten by Blood Moon / Magus (CR 613.7 dependency).
URBORG_TOMB_OF_YAWGMOTH: CardPrinting = CardPrinting(
    name="Urborg, Tomb of Yawgmoth",
    mana_cost_str="",
    mana_cost=ManaCost(),
    type_line="Legendary Land — Swamp",
    supertypes=("Legendary",),
    card_types=("Land",),
    subtypes=("Swamp",),
    produces=frozenset({"B"}),
    grants_basic_land_type_to_all_lands="Swamp",
)
