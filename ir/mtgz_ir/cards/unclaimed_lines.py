"""Per-line diagnosis of why the oracle parser leaves a card unclaimed.

The parser claims a card only when every oracle line parses, and on
failure it says nothing about *which* line failed. This module answers
that by probing the same builder chain with edited copies of the card's
raw Scryfall record:

1. **Units.** Each face's oracle text splits into lines; a modal bullet
   (``•``) travels with the line that introduces it (``Choose one --``),
   so a mode list is probed as one unit. Pure-reminder lines are dropped
   (the parser strips reminder text anyway).
2. **Isolation.** Build a copy whose only rules text is one unit (every
   other line on every face blanked). A unit that builds alone passes.
3. **Add-back.** A unit that fails alone may be a rider that only parses
   next to its effect ("As an additional cost to cast this spell, ...",
   "Activate only as a sorcery."). It is retried together with every
   passing unit; if that builds, it passes in context. When the card's
   own lines can't serve as that companion, an optional caller-supplied
   *witness* line known to parse on the type (``"Draw a card."``) does,
   provided the witness builds on that very card.
4. **Confirm.** Build the card from the passing units only.

   - ``kind == "lines"`` (``confirmed``): some units fail, and the card
     builds without them (next to the witness when no own unit passes):
     the failing units are the whole blocker.
   - ``kind == "unconfirmed"``: no unit passes and no usable witness
     exists (multi-face layouts, unsupported card types), so the frame
     itself couldn't be checked -- it may block too.
   - ``kind == "mixed"``: some units fail *and* the passing units don't
     build together -- a cross-line or structural blocker on top.
   - ``kind == "structure"``: every unit parses, yet the card doesn't
     build -- the layout, type line, mana cost or the combination of
     lines isn't modeled.

A probe is a diagnosis aid for planning grammar work, never a claim:
nothing here feeds the parser or the card source.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable

from mtgz_ir.cards.pool_coverage import strip_reminder_text

# ``build(card_record) -> bool``: does the parser claim this record?
BuildFn = Callable[[dict], bool]

_BULLET = "•"


@dataclass(frozen=True)
class Unit:
    """One probe unit: a line (plus any bullets it introduces) on one
    face (``face is None`` for single-faced records)."""

    face: int | None
    raw: str          # as printed, reminder text included
    text: str         # reminder-stripped, for display


@dataclass
class LineDiagnosis:
    name: str
    kind: str           # "lines" | "unconfirmed" | "mixed" | "structure"
    failing: list[Unit] = field(default_factory=list)
    passing: list[Unit] = field(default_factory=list)
    # The card claims once the failing units are removed.
    confirmed: bool = False

    @property
    def failing_text(self) -> list[str]:
        return [u.text for u in self.failing]


def split_units(card: dict) -> list[Unit]:
    """The record's oracle text as probe units, in printed order."""
    if card.get("card_faces"):
        texts = [(i, f.get("oracle_text") or "")
                 for i, f in enumerate(card["card_faces"])]
    else:
        texts = [(None, card.get("oracle_text") or "")]
    units: list[Unit] = []
    for face, text in texts:
        for raw in text.split("\n"):
            stripped = strip_reminder_text(raw)
            if not stripped:
                continue
            prev = units[-1] if units else None
            if stripped.startswith(_BULLET) and prev is not None and prev.face == face:
                units[-1] = Unit(face, f"{prev.raw}\n{raw}",
                                 f"{prev.text} {stripped}")
                continue
            units.append(Unit(face, raw, stripped))
    return units


def with_units(card: dict, units: list[Unit]) -> dict:
    """A copy of ``card`` whose rules text is exactly ``units``."""
    variant = dict(card)
    if card.get("card_faces"):
        faces = [dict(f) for f in card["card_faces"]]
        for i, face in enumerate(faces):
            face["oracle_text"] = "\n".join(u.raw for u in units if u.face == i)
        variant["card_faces"] = faces
    else:
        variant["oracle_text"] = "\n".join(u.raw for u in units)
    return variant


def _order(units: list[Unit], all_units: list[Unit]) -> list[Unit]:
    keep = set(units)
    return [u for u in all_units if u in keep]


def diagnose(
    card: dict, build: BuildFn, *, witness: str | None = None,
) -> LineDiagnosis:
    """Why ``card`` is unclaimed (see the module docstring).

    ``witness`` is an optional line known to parse on the card's type
    (``"Draw a card."`` for an instant). Many types never build without
    *some* rules text, so a keyword or rider probed on an otherwise
    text-less card fails even when the grammar supports it. When a
    single-faced card's own lines can't serve as that companion, units
    are retried next to the witness -- but only if the witness builds
    on this very card, so a bad witness can't rescue anything."""
    units = split_units(card)
    passing = [u for u in units if build(with_units(card, [u]))]
    failing = [u for u in units if u not in passing]
    rescued = [
        u for u in failing
        if build(with_units(card, _order(passing + [u], units)))
    ]
    passing = _order(passing + rescued, units)
    failing = [u for u in failing if u not in rescued]
    w = _usable_witness(card, build, witness)
    if w is not None and failing:
        rescued = [
            u for u in failing
            if build(with_units(card, _order(passing + [u], units) + [w]))
        ]
        passing = _order(passing + rescued, units)
        failing = [u for u in failing if u not in rescued]
    # The card's frame (type line, layout, cost) plus its passing lines
    # builds -- next to the witness when the type can't build text-less.
    confirmed = bool(failing) and (
        build(with_units(card, passing))
        or (w is not None and build(with_units(card, passing + [w])))
    )
    if not failing:
        kind = "structure"
    elif confirmed:
        kind = "lines"
    elif not passing:
        kind = "unconfirmed"
    else:
        kind = "mixed"
    return LineDiagnosis(card["name"], kind, failing, passing, confirmed)


def _usable_witness(card: dict, build: BuildFn, witness: str | None) -> Unit | None:
    if witness is None or card.get("card_faces"):
        return None
    unit = Unit(None, witness, witness)
    return unit if build(with_units(card, [unit])) else None
