"""pytest setup for the card-IR pipeline's tests (docs/ENGINE.md 170): import ``mtgz_ir`` from ``ir/``.

The tests came from mtg-bot with its parser (renamed ``mtg_bot`` -> ``mtgz_ir``). They run on the
parser source (``OracleParserCardSource``), as mtg-bot's own conftest made them; ``MTG_BOT_CARD_SOURCE``
is the seam it kept for another source.
"""
import os
import sys

import pytest

IR = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), 'ir')
if IR not in sys.path:
    sys.path.insert(0, IR)

from mtgz_ir.cards.source import card_source_from_env, get_active_source, set_active_source  # noqa: E402


@pytest.fixture(scope="session", autouse=True)
def _swap_card_source_from_env():
    requested = card_source_from_env(os.environ.get("MTG_BOT_CARD_SOURCE"))
    original = get_active_source()
    set_active_source(requested)
    try:
        yield
    finally:
        set_active_source(original)
