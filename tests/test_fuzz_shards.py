"""tools/fuzz_shards.py (docs/ENGINE.md 165): the sharded fuzz's ranges tile the run's seeds, a shard is positive
evidence only with its own range's FUZZ_TOTALS and one passed test, and the merge refuses a gap, an overlap, a count
that does not add up or a skip some shards did not see. Nothing is built here: shard outputs are canned. The engine
side (the merged totals through the same end-of-run checks) is engine/tests/fuzz.rs::fuzz_shard_totals, run by the
tool itself."""
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import fuzz_shards as F  # noqa: E402

OK = 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 1.00s\n'


def shard(kind, first, n, totals=None, extra=''):
    j = F.Job(f'{kind}[{first},{first + n})', [], {}, 'log')
    t = {'kind': kind, 'first': first, 'n': n, 'totals': totals or {'is_walks': 3, 'max_power': first}}
    lines = [f'{n} games: wins [1, 1], draws 0'] if kind == 'games' else [f'{n} matches: ...']
    j.out = '\n'.join(lines + [f'FUZZ_TOTALS {json.dumps(t)}', 'FUZZ_SHARD=1: the end-of-run checks ...', extra]) + '\n' + OK
    j.rc = 0
    return j, t


def test_the_ranges_tile_the_seeds():
    assert F.split_ranges(0, 2000, 8) == [(k * 250, 250) for k in range(8)]
    r = F.split_ranges(5, 10, 3)
    assert r == [(5, 4), (9, 3), (12, 3)] and sum(n for _, n in r) == 10
    assert F.split_ranges(0, 3, 8) == [(0, 1), (1, 1), (2, 1)]          # never an empty shard
    assert F.split_ranges(0, 0, 8) == []


def test_a_shard_is_evidence_only_for_its_own_range():
    j, _ = shard('games', 250, 250)
    assert F.shard_problem(j, 'games', 250, 250)[0] is None
    assert 'names [250, +250), not [0, +250)' in F.shard_problem(j, 'games', 0, 250)[0]
    j.out = j.out.replace('FUZZ_SHARD=1:', 'x')
    assert 'ran its own end-of-run checks' in F.shard_problem(j, 'games', 250, 250)[0]
    j, _ = shard('games', 0, 250)
    j.out = j.out.replace('250 games: wins', '300 games: wins')
    assert '"N games: wins"' in F.shard_problem(j, 'games', 0, 250)[0]
    j, _ = shard('games', 0, 250)
    j.rc, j.out = 101, "thread 'fuzz_random_games' panicked:\nseed 17 failed (rerun: FUZZ_FIRST=17 FUZZ_GAMES=1 FUZZ_LOG=1): x\n"
    assert 'seed 17 failed' in F.shard_problem(j, 'games', 0, 250)[0]
    j, _ = shard('games', 0, 250)
    j.out = j.out.replace(OK, OK.replace('1 passed', '0 passed'))
    assert 'test result' in F.shard_problem(j, 'games', 0, 250)[0]
    assert 'never ran' in F.shard_problem(F.Job('x', [], {}, 'log'), 'games', 0, 1)[0]


def test_the_merge_sums_and_checks_coverage():
    parts = [shard('games', f, 250, {'is_walks': 2, 'max_power': f})[1] for f in (0, 250, 500, 750)]
    m, probs = F.merge('games', parts, 0, 1000)
    assert not probs and m == {'n': 1000, 'totals': {'is_walks': 8, 'max_power': 750}}
    m, probs = F.merge('games', parts[:2] + parts[3:], 0, 1000)
    assert any('gap at seed 500' in p for p in probs)
    m, probs = F.merge('games', parts + [parts[1]], 0, 1000)
    assert any('overlap' in p for p in probs)
    skip = [{'kind': 'matches', 'first': 0, 'n': 10, 'skipped': 'games only'}, {'kind': 'matches', 'first': 10, 'n': 10, 'skipped': 'games only'}]
    m, probs = F.merge('matches', skip, 0, 20)
    assert not probs and m['skipped'] == 'games only'
    m, probs = F.merge('matches', [skip[0], {'kind': 'matches', 'first': 10, 'n': 10, 'totals': {}}], 0, 20)
    assert any('disagree on a skip' in p for p in probs)
    bad = [dict(parts[0], totals={'x': -1})]
    assert any('not a count' in p for p in F.merge('games', bad, 0, 250)[1])


def test_a_shards_settings_are_its_own(monkeypatch):
    monkeypatch.setenv('FUZZ_SKIP', '1,2,3')
    monkeypatch.setenv('DIGEST_WRITE', '1')
    env = F.child_env({'FUZZ_FIRST': '250'})
    assert 'FUZZ_SKIP' not in env and 'DIGEST_WRITE' not in env and env['FUZZ_FIRST'] == '250'
