"""Shared fixtures for Vintage Roto (``roto`` pool) card tests.

``roto_card(name)`` returns the raw record from the roto pool's files
(the Scryfall dump ``data/cube_roto.json`` or the hand-authored custom
cards ``data/cube_roto_custom.json``); ``printing(name)`` builds it
through the parser source's own claim path (``build_card_record``) and
asserts the card is claimed. Arena/agent helpers are re-exported from the cube
artifact helpers so roto tests read like the cube bucket tests.

Roto cards are part of the default ``all`` pool since 2026-10-03, so
the default card source holds them too; these helpers build one record
through the claim path without depending on the active source.
"""

from __future__ import annotations

from functools import lru_cache

from mtgz_ir.cards.base import CardPrinting
from mtgz_ir.cards.parser.source import build_card_record
from mtgz_ir.cards.pools import load_pool_cards

from _cube_artifact_helpers import (  # noqa: F401  (re-exports)
    ScriptedAgent,
    agent,
    arena,
    put,
    to_graveyard,
    to_hand,
    to_library,
)

@lru_cache(maxsize=1)
def _roto_index() -> dict[str, dict]:
    return {c["name"]: c for c in load_pool_cards("roto")}


def roto_card(name: str) -> dict:
    return _roto_index()[name]


@lru_cache(maxsize=None)
def printing(name: str) -> CardPrinting:
    built = build_card_record(roto_card(name))
    assert built is not None, f"{name} should be claimed by the parser"
    return built
