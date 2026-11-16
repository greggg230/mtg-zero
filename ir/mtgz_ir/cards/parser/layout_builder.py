"""Builder dispatch for the non-normal card layouts (cube layouts bucket).

Runs first in ``source._build_printing_unpinned`` and returns ``None``
for every card that isn't one of these layouts, so the per-type builders
see exactly the cards they saw before:

* double-faced cards (``layout`` ``transform`` / ``modal_dfc``) --
  :mod:`mtgz_ir.cards.parser.dfc_builder`;
* split cards with two instant / sorcery halves (``layout == "split"``;
  Rooms are left to ``room_builder``) --
  :mod:`mtgz_ir.cards.parser.split_builder`;
* Sagas (any card with the Saga subtype, including a DFC's Saga front
  face) -- :mod:`mtgz_ir.cards.parser.saga_builder`;
* Classes (the Class subtype) -- :mod:`mtgz_ir.cards.parser.class_builder`;
* levelers (``layout == "leveler"``) --
  :mod:`mtgz_ir.cards.parser.leveler_builder`;
* adventurer and preparation cards (``layout`` ``adventure`` /
  ``prepare``) -- :mod:`mtgz_ir.cards.parser.adventure_builder`;
* flip cards (``layout == "flip"``; roto P6) --
  :mod:`mtgz_ir.cards.parser.flip_builder`.

A multi-face card of any other layout (or one whose faces don't build) is
never claimed by the single-face builders: its top-level record has no
oracle text, so they would build a vanilla creature with the combined
mana cost (``source._build_printing_unpinned`` stops it).
"""

from __future__ import annotations

from mtgz_ir.cards.base import CardPrinting


def _build_layout_printing(card: dict) -> CardPrinting | None:
    from mtgz_ir.cards.parser.dfc_builder import _build_dfc_printing
    from mtgz_ir.cards.parser.adventure_builder import (
        _build_adventure_printing,
        _build_prepare_printing,
    )
    from mtgz_ir.cards.parser.class_builder import _build_class_printing
    from mtgz_ir.cards.parser.leveler_builder import _build_leveler_printing
    from mtgz_ir.cards.parser.saga_builder import _build_saga_printing
    from mtgz_ir.cards.parser.split_builder import _build_split_printing
    from mtgz_ir.cards.parser.flip_builder import _build_flip_printing

    for builder in (
        _build_dfc_printing, _build_split_printing, _build_saga_printing,
        _build_class_printing, _build_leveler_printing,
        _build_adventure_printing, _build_prepare_printing,
        _build_flip_printing,
    ):
        built = builder(card)
        if built is not None:
            return built
    return None
