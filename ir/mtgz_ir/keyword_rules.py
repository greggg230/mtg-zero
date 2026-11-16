"""Rules consumers for the evasion / protection keyword family.

Hexproof (CR 702.11), shroud (CR 702.18) and protection (CR 702.16)
all restrict what may *target* a permanent; protection additionally
restricts damage, blocking and attachment (the "DEBT" mnemonic:
Damage is prevented, it can't be Enchanted / Equipped by, Blocked by,
or Targeted by sources with the stated quality).

The keywords themselves live in ``effective_keywords`` like every other
keyword, so printed, anthem-granted, Equipment-granted and
until-end-of-turn grants all reach these helpers through one walk.
Protection is spelled ``"protection from <quality>"`` in the keyword
set (``"protection from red"``, ``"protection from creatures"``,
``"protection from everything"``, ...); the printing-level
``protection_from_color`` field (layer-3 color-word rewritable, see
``effective_protection_from_colors``) is folded in as well.

The engine's target enumerators (``Effect.legal_targets`` overrides)
are per-effect, so the targeting restriction is applied at the
chokepoints that turn an enumerated pool into a choice: spell casting,
ability activation, triggered-ability stacking, and the legality
checks in ``legal.py``. Resolution re-checks the same rule via
:func:`targets_all_illegal_on_resolution` (CR 608.2b).
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Iterable

from mtgz_ir.keyword_vocab import ProtectionQuality

if TYPE_CHECKING:  # pragma: no cover
    from mtgz_ir.game import GameObject, GameState


PROTECTION_PREFIX = "protection from "

COLOR_WORD_TO_LETTER: dict[str, str] = {
    "white": "W", "blue": "U", "black": "B", "red": "R", "green": "G",
}
LETTER_TO_COLOR_WORD: dict[str, str] = {
    v: k for k, v in COLOR_WORD_TO_LETTER.items()
}

# Qualities a protection keyword may name: the closed
# ``keyword_vocab.ProtectionQuality`` vocabulary (colors, color counts,
# "everything", card types, the player qualities of CR 702.16j resolved
# against the protected object in ``is_protected_from``, and Emrakul's
# "spells that are one or more colors"). The parser refuses any other
# quality instead of minting a keyword no consumer understands.
PROTECTION_QUALITIES: frozenset[str] = frozenset(q.value for q in ProtectionQuality)
assert set(COLOR_WORD_TO_LETTER) <= PROTECTION_QUALITIES

# "hexproof from <quality>" keyword spelling (CR 702.11d).
HEXPROOF_FROM_PREFIX = "hexproof from "

# Target kinds whose integer id is NOT a battlefield object id (stack
# indexes, player ids, card-in-other-zone ids).
_NON_PERMANENT_TARGET_KINDS = frozenset({"player", "stack", "spell", "card"})


def protection_keyword(quality: str) -> str:
    """Engine keyword spelling for ``protection from <quality>``."""
    if quality not in PROTECTION_QUALITIES:
        raise ValueError(f"unknown protection quality: {quality!r}")
    return PROTECTION_PREFIX + quality


def protection_qualities(state: "GameState", obj: "GameObject") -> frozenset[str]:
    """Every quality ``obj`` currently has protection from."""
    from mtgz_ir.game import effective_keywords, effective_protection_from_colors

    quals: set[str] = set()
    for kw in effective_keywords(state, obj):
        if kw.startswith(PROTECTION_PREFIX):
            quals.add(kw[len(PROTECTION_PREFIX):])
    for letter in effective_protection_from_colors(state, obj):
        word = LETTER_TO_COLOR_WORD.get(letter)
        if word is not None:
            quals.add(word)
    return frozenset(quals)


def _source_colors(state: "GameState", source: "GameObject") -> frozenset[str]:
    from mtgz_ir.game import effective_colors

    return effective_colors(state, source)


def _source_is_type(state: "GameState", source: "GameObject", card_type: str) -> bool:
    from mtgz_ir.game import has_card_type

    return has_card_type(state, source, card_type)


_TYPE_QUALITIES: dict[str, str] = {
    "creatures": "Creature",
    "artifacts": "Artifact",
    "enchantments": "Enchantment",
    "instants": "Instant",
    "sorceries": "Sorcery",
    "planeswalkers": "Planeswalker",
}


def source_has_quality(state: "GameState", source: "GameObject", quality: str) -> bool:
    """Whether ``source`` has the quality a protection keyword names."""
    if quality == "everything":
        return True
    if quality == "spells that are one or more colors":
        from mtgz_ir.game import Zone

        is_spell = source.zone == Zone.STACK or source is state.resolving_spell
        return is_spell and bool(_source_colors(state, source))
    if quality in _TYPE_QUALITIES:
        return _source_is_type(state, source, _TYPE_QUALITIES[quality])
    colors = _source_colors(state, source)
    if quality in COLOR_WORD_TO_LETTER:
        return COLOR_WORD_TO_LETTER[quality] in colors
    if quality == "colorless":
        return not colors
    if quality == "multicolored":
        return len(colors) >= 2
    if quality == "monocolored":
        return len(colors) == 1
    return False


def is_protected_from(
    state: "GameState", obj: "GameObject", source: "GameObject | None",
) -> bool:
    """True when ``obj`` has protection from a quality ``source`` has."""
    if source is None:
        return False
    for q in protection_qualities(state, obj):
        if q == "the chosen player":
            if (
                obj.chosen_player_id is not None
                and source.controller_id == obj.chosen_player_id
            ):
                return True
        elif q == "each of your opponents":
            if source.controller_id != obj.controller_id:
                return True
        elif source_has_quality(state, source, q):
            return True
    return False


def can_be_targeted_by(
    state: "GameState",
    obj: "GameObject",
    source: "GameObject | None",
    controller_id: int,
) -> bool:
    """CR 702.11b / 702.18a / 702.16b: may ``controller_id``'s spell or
    ability whose source is ``source`` target ``obj``?"""
    from mtgz_ir.game import effective_keywords

    kws = effective_keywords(state, obj)
    if "shroud" in kws:
        return False
    if "hexproof" in kws and obj.controller_id != controller_id:
        return False
    if state.continuous_effects:
        # "can't be the target of spells or abilities your opponents
        # control this turn" (Vines of Vastwood; Vintage Roto P5).
        from mtgz_ir.roto_board_statics import untargetable_by

        if untargetable_by(state, obj, controller_id):
            return False
    if obj.controller_id != controller_id and source is not None:
        # "hexproof from <quality>" (CR 702.11d; Veil of Summer).
        for kw in kws:
            if kw.startswith(HEXPROOF_FROM_PREFIX) and source_has_quality(
                state, source, kw[len(HEXPROOF_FROM_PREFIX):],
            ):
                return False
    return not is_protected_from(state, obj, source)


def _battlefield_object(state: "GameState", target: tuple[str, int]):
    kind, tid = target
    if kind in _NON_PERMANENT_TARGET_KINDS:
        return None
    for o in state.battlefield:
        if o.id == tid:
            return o
    return None


def filter_targetable(
    state: "GameState",
    targets: Iterable[tuple[str, int]],
    source: "GameObject | None",
    controller_id: int,
) -> list[tuple[str, int]]:
    """Drop battlefield targets that hexproof / shroud / protection
    make illegal for this source + controller. Non-permanent targets
    (players, stack items, cards in other zones) pass through."""
    from mtgz_ir.roto_board_statics import player_hexproof_from_source

    out: list[tuple[str, int]] = []
    for t in targets:
        obj = _battlefield_object(state, t)
        if obj is not None and not can_be_targeted_by(state, obj, source, controller_id):
            continue
        if t[0] == "player" and player_hexproof_from_source(
            state, t[1], source, controller_id,
        ):
            continue
        out.append(t)
    return out


def targets_all_illegal_on_resolution(
    state: "GameState",
    targets: tuple[tuple[str, int], ...],
    source: "GameObject | None",
    controller_id: int,
) -> bool:
    """CR 608.2b: a spell or ability whose every target has become an
    illegal target doesn't resolve. Only the targeting restrictions
    this module owns are re-checked (a target that left the battlefield
    keeps its pre-existing per-effect handling)."""
    if not targets:
        return False
    for t in targets:
        obj = _battlefield_object(state, t)
        if obj is None:
            return False
        if can_be_targeted_by(state, obj, source, controller_id):
            return False
    return True


def current_damage_source(state: "GameState") -> "GameObject | None":
    """The object dealing non-combat damage right now: the resolving
    ability's source, else the resolving spell."""
    if state.resolving_source is not None:
        return state.resolving_source
    return state.resolving_spell


def blocked_by_protection(
    state: "GameState", attacker: "GameObject", blocker: "GameObject",
) -> bool:
    """CR 702.16f: an attacker with protection from a quality can't be
    blocked by creatures with that quality."""
    return is_protected_from(state, attacker, blocker)


def attachment_illegal_by_protection(
    state: "GameState", attachment: "GameObject", host: "GameObject",
) -> bool:
    """CR 702.16c-d: a permanent with protection can't be enchanted by
    Auras / equipped by Equipment that have the protected quality."""
    return is_protected_from(state, host, attachment)
