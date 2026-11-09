"""The Rust port of the EoE sealed-pool generator against the Python reference (docs/ENGINE.md
101.1; mtgz/sealed/collation.py, docs/sealed_data.md 3): the same boosters, slot by slot, for
2,000 seeds per collation (and the options), through the engine module. Needs `mtgz._engine`
(tools/build_py.sh)."""
import concurrent.futures as cf
import os

from mtgz.sealed import collation as K
from mtgz.sealed import engine as R

SEEDS = 2000
# (Large and edge seeds too: the reference masks a seed to 64 bits.)
EXTRA = [2**32 - 1, 2**32, 2**63 - 1, 2**63, 2**64 - 1, 0x9E3779B97F4A7C15]


def _reference(args):
    collation, seeds, kw = args
    out = []
    for s in seeds:
        p = K.generate(s, collation=collation, **kw)
        out.append((p.boosters, p.promo, p.cards))
    return out


def _compare(collation, seeds, **kw):
    seeds = list(seeds)
    chunks = [seeds[i::16] for i in range(16)]
    workers = min(16, os.cpu_count() or 1)
    with cf.ProcessPoolExecutor(workers) as ex:
        ref = {}
        for chunk, res in zip(chunks, ex.map(_reference, [(collation, c, kw) for c in chunks])):
            ref.update(zip(chunk, res))
    for s in seeds:
        r = R.pool(s, collation=collation, **kw)
        boosters, promo, cards = ref[s]
        assert r['boosters'] == boosters, (collation, s, kw)
        assert (r['promo'], r['cards']) == (promo, cards), (collation, s, kw)


def test_paper_pools_match_the_reference():
    _compare('paper', list(range(SEEDS)) + EXTRA)
    _compare('paper', range(300), prerelease_promo=True)
    _compare('paper', range(300), paradox_haze=True, boosters=24)


def test_arena_pools_match_the_reference():
    _compare('arena', list(range(SEEDS)) + EXTRA)
    _compare('arena', range(100), paradox_haze=True, boosters=12)


def test_the_options_are_checked_as_the_reference():
    import pytest
    with pytest.raises(ValueError):
        R.pool(1, collation='mtgo')
    with pytest.raises(ValueError):
        R.pool(1, collation='arena', prerelease_promo=True)
