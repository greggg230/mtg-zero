"""Duskmourn Room ``CardPrinting`` builder.

CR 207.2g + 715: a Room is an enchantment with two halves; each half
has its own name, mana cost, and rules text. When a Room spell is
cast, the controller picks one half to cast for that half's mana
cost; the Room enters the battlefield with that half unlocked. The
other half is locked. The controller can later (sorcery speed) pay
the locked half's mana cost to unlock it.

Scryfall stores Rooms as a single card with a ``card_faces`` array
of two faces. Each face has its own ``mana_cost``, ``oracle_text``,
``name``, and ``type_line`` (always ``Enchantment — Room``). This
builder splits the two faces, parses each into a ``RoomHalf``
(name + mana cost + per-half abilities), and glues them together
via ``make_room`` into the parent ``CardPrinting``.

Single claim path: each face's oracle text walks through the unified
per-line grammar (``_parse_clause`` + ``_parse_triggered_ability_line``).
Door triggers (``"When you unlock this door, …"``) flow through
the predicate-trigger-phrasebook entry ``"you unlock this door"``
which maps to ``(Event.ENTERS, IsSelf())`` — the same shape the
per-half static-side wiring uses, gated through the Room's
per-half ``unlocked_halves`` set by ``make_room`` tagging the
flattened ability with ``room_half=<half-name>``.

Per-half oracle-text lines that map to engine-unmodeled mechanics
(trigger doubling, optional alt-cost permission, etc.) live on the
``_ROOM_HONEST_NOOP_LINES`` allowlist below; the half emits as
cost-and-name only for those lines. Same precedent as
``_NONAURA_HONEST_NOOP_LINES`` in ``enchantment_builder``.

Combined-card names that the unified-grammar path is allowed to
claim live on ``_UNIFIED_ROOM_NAMES``. Rooms outside the allowlist
are simply not claimed (they pass through the type-gate and return
``None``) — same silent-skip discipline the rest of the parser
applies to unclaimed cards. The allowlist serves as documentation
of "these Rooms have been audited end-to-end and the unified-path
output matches the printed surface".
"""

from __future__ import annotations

from mtgz_ir.cards.base import (
    CardPrinting,
    RoomHalf,
    StaticAbility,
    TriggeredAbility,
    make_room,
    parse_mana_cost,
)
from mtgz_ir.cards.parser.clause import _parse_clause
from mtgz_ir.cards.parser.text import _strip_reminder_text


# Combined-card names that the unified-grammar path is allowed to
# claim. Restricting the oracle-text path to an explicit allowlist
# preserves the audit-trail discipline: a Room whose oracle text
# happens to match the per-line grammar today but whose engine
# semantics weren't yet validated end-to-end stays unclaimed until
# it's been audited and added here. The bespoke dispatch that this
# allowlist used to gate is gone — all four prior bespoke entries
# (Charred Foyer / Mirror Room / Unholy Annex / Derelict Attic)
# migrated to the unified path.
_UNIFIED_ROOM_NAMES: frozenset[str] = frozenset({
    "Bottomless Pool // Locker Room",
    "Charred Foyer // Warped Space",
    "Derelict Attic // Widow's Walk",
    "Funeral Room // Awakening Hall",
    "Glassworks // Shattered Yard",
    "Grand Entryway // Elegant Rotunda",
    "Meat Locker // Drowned Diner",
    "Mirror Room // Fractured Realm",
    "Moldering Gym // Weight Room",
    "Restricted Office // Lecture Hall",
    "Roaring Furnace // Steaming Sauna",
    "Surgical Suite // Hospital Room",
    "Ticket Booth // Tunnel of Hate",
    "Underwater Tunnel // Slimy Aquarium",
    "Unholy Annex // Ritual Chamber",
})


# Per-half oracle-text lines that map to engine-unmodeled mechanics
# and emit as honest no-ops on the produced ``RoomHalf``: the half
# still exists with cost-and-name only, but the line contributes no
# trigger / static / activated to the emitted abilities tuple. Same
# permission-not-offered / engine-doesn't-model-this discipline as
# ``_NONAURA_HONEST_NOOP_LINES`` in ``enchantment_builder``.
#
# Each entry is the full printed line (after reminder-text stripping)
# of a half whose entire body is a single un-modeled clause. When a
# new mechanic lands in the engine that promotes one of these lines
# to an actual recognizer, drop the entry — the recognizer will
# claim it on the standard ``_parse_clause`` path. Keeping the
# allowlist explicit (rather than silently dropping every
# unrecognized clause) preserves the "honest miss" discipline:
# adding a new Room half whose body the parser doesn't recognize
# rejects loudly rather than silently dropping clauses.
_ROOM_HONEST_NOOP_LINES: frozenset[str] = frozenset({
    # Fractured Realm: trigger doubler — CR 603.10b ``if [trigger
    # condition], that ability triggers an additional time``. The
    # engine doesn't model trigger doubling, so the printed effect is
    # an honest no-op permission-style miss — single fire is the
    # rules-correct base; the doubler would only ever ADD benefits.
    # Same family as Lecture Hall's hexproof-on-permanents (which
    # parses through the anthem grammar) and Warped Space's optional
    # alt-cost permission.
    "If a triggered ability of a permanent you control triggers, "
    "that ability triggers an additional time.",
    # Warped Space: optional alternative-cost permission ``you may
    # pay {0} for a spell you cast from exile``. The spell-casting
    # pipeline doesn't expose alt-costs as a choice yet, so a player
    # who could "elect" the {0} cost simply pays the printed mana
    # cost instead. Same permission-not-offered shape as Steaming
    # Sauna's "no maximum hand size" and Lecture Hall's hexproof-on-
    # permanents — not modelling the optional benefit doesn't break
    # rules, it just makes the player miss a free spell. Once the
    # alt-cost machinery lands the static gains a real effect via
    # the ``room_half`` lock gate and this entry drops.
    "Once each turn, you may pay {0} rather than pay the mana cost "
    "for a spell you cast from exile.",
})


def _build_half_from_face(face: dict) -> RoomHalf | None:
    """Parse one Scryfall face dict (``name``, ``mana_cost``,
    ``oracle_text``) into a ``RoomHalf``.

    Walks the face's oracle text line-by-line, strips reminder text,
    and dispatches each non-empty line through ``_parse_clause``. The
    half's name is threaded through to the trigger parser so a future
    "When you unlock <Half-Name>" surface (the parser's
    ``_substitute_self_name`` rewrite) lines up with the canonical
    ``"this creature"`` form — today every Room prints
    ``"this door"`` so the substitution is a no-op.

    Returns ``None`` if any non-empty line fails to parse, so an
    unrecognized line surfaces as a parser miss for the whole face
    rather than a silently-dropped clause.

    Lines on ``_ROOM_HONEST_NOOP_LINES`` are silently dropped — they
    map to engine-unmodeled mechanics (trigger doubling, optional
    alt-cost permission, etc.) and the half emits as cost-and-name
    only, mirroring the bespoke factory's pattern of producing a
    cost-and-name-only ``RoomHalf`` for un-modeled partner halves.

    Triggered abilities AND static abilities are threaded onto the
    produced ``RoomHalf``. Keywords and activated abilities on a
    half aren't exercised by any unified-path Room today — those
    surface as a parse failure (``None``) so the bespoke dispatch
    can keep owning halves with those shapes.
    """
    name = face.get("name", "")
    mana_cost_str = face.get("mana_cost", "")
    triggers: list[TriggeredAbility] = []
    statics: list[StaticAbility] = []
    for raw_line in face.get("oracle_text", "").split("\n"):
        line = _strip_reminder_text(raw_line)
        if not line:
            continue
        if line in _ROOM_HONEST_NOOP_LINES:
            continue
        clause = _parse_clause(line, name=name)
        if clause is None:
            return None
        if clause.triggers:
            triggers.extend(clause.triggers)
            continue
        if clause.statics:
            statics.extend(clause.statics)
            continue
        # Keywords / activated on a Room half aren't claimed by the
        # unified path yet — surface as a miss so the bespoke
        # dispatch can keep owning these shapes.
        return None
    return RoomHalf(
        name=name,
        mana_cost_str=mana_cost_str,
        mana_cost=parse_mana_cost(mana_cost_str),
        triggered_abilities=tuple(triggers),
        static_abilities=tuple(statics),
    )


def _build_room_printing_from_faces(card: dict) -> CardPrinting | None:
    """Build a ``CardPrinting`` for a Room card via per-face oracle-text
    parsing.

    Returns ``None`` if the card has fewer than two faces or any face
    fails to parse — the caller falls through to the bespoke dispatch.
    """
    faces = card.get("card_faces", [])
    if len(faces) != 2:
        return None
    half_a = _build_half_from_face(faces[0])
    if half_a is None:
        return None
    half_b = _build_half_from_face(faces[1])
    if half_b is None:
        return None
    return make_room(half_a, half_b)


def _build_room_printing(card: dict) -> CardPrinting | None:
    """Claim a DSK Room card.

    Routes Rooms in ``_UNIFIED_ROOM_NAMES`` through the unified
    per-line grammar path (``_build_room_printing_from_faces``).
    Returns ``None`` for non-Room cards, for Rooms outside the
    allowlist, or for Rooms in the allowlist whose unified-path
    parse fails (a parse failure is a regression — the allowlist
    promises the unified path claims the card).

    Type-line gate: Scryfall's combined Room type_line is
    ``"Enchantment — Room // Enchantment — Room"``. We accept any
    type_line containing ``"Room"`` to avoid false negatives if the
    formatting drifts.
    """
    type_line = card.get("type_line", "")
    if "Room" not in type_line:
        return None
    name = card.get("name", "")
    if name not in _UNIFIED_ROOM_NAMES:
        return None
    return _build_room_printing_from_faces(card)
