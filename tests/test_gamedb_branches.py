"""tools/gamedb.py reads branch games (zero-core records/2; docs/gamedb.md "Branch games", docs/ENGINE.md 227): a branch
shares its game's gid and seed and its first `branch.from` actions; a merged near-tie branch ends at a position of a game
in progress with its game's result. Random-move Delver games (imported as a self-play iteration) with branch records
made from them: their game ids differ from their games', their games rows carry the branch columns, a merged branch's
result is checked against its game's (a wrong one, or a missing game, fails loudly), its winner and method are the
borrowed result's, its events and combo rows count only from its branch point, and the behaviour series leave branch
games out unless asked. Needs duckdb and pyarrow (skipped without)."""
import base64
import gzip
import hashlib
import json
import os
import random
import sys

import pytest

pytest.importorskip('duckdb')
pytest.importorskip('pyarrow')

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az  # noqa: E402,F401
from mtgz import _engine as E  # noqa: E402
import gamedb as GDB  # noqa: E402
from zero import gamerec as GR  # noqa: E402

from test_combo_stats import DELVER  # noqa: E402
from test_gamedb import combo_file, table  # noqa: E402


def b64(acts):
    return base64.b64encode(memoryview(__import__('array').array('H', acts)).tobytes()).decode()


def branch_record(parent, spec_txt, frm, rng, merge_after=None, result=None):
    """A branch of `parent`: its first `frm` actions, then random ones (the first another than the game's) to the end -
    or, with `merge_after`, stopped after that many own decisions at a game in progress, merged with the game's result
    (or `result`)."""
    acts = GR.actions_of(parent)
    g = E.Game('mtg', spec_txt, int(parent['n']), int(parent['seed']))
    for a in acts[:frm]:
        g.step(int(a))
    own, decisions, alt = [], 0, None
    while g.to_move is not None:
        legal = list(g.legal())
        if merge_after is not None and decisions >= merge_after and len(legal) > 1:
            break
        if not own and len(legal) > 1:
            a = rng.choice([x for x in legal if x != acts[frm]])
            alt = a
        else:
            a = rng.choice(legal)
        decisions += len(legal) > 1
        own.append(a)
        g.step(a)
    all_acts = list(acts[:frm]) + own
    rec = dict(parent, actions=b64(all_acts), n_actions=len(all_acts))
    if merge_after is None:
        rec['result'] = [float(x) for x in g.result()]
        rec['over'] = True
    else:
        assert g.to_move is not None
        rec['result'] = list(result if result is not None else parent['result'])
        rec['over'] = True
    rec['branch'] = {'from': frm, 'at': 0, 'of': 1, 'moves': 1, 'alt': alt, 'gap': 0.0125, 'tag': 1,
                     'merged': None if merge_after is None else merge_after}
    return rec


def build_run(tmp_path, extra, seed=5, games=4):
    """A pseudo-run of `games` random Delver games (iteration 1) plus the records `extra(parents, spec)` returns."""
    os.makedirs(tmp_path, exist_ok=True)
    rng = random.Random(seed)
    p = str(tmp_path / 'g.b000.games.json.gz')
    combo_file(p, (DELVER, DELVER), seed, games, rng)
    run = str(tmp_path / 'run')
    GDB.import_combo(run, [p], it=1, log=lambda *a: None)
    d = GR.iter_dir(run, 1)
    name = next(iter(GR.read_done(run, 1)['files']))
    path = os.path.join(d, name)
    lines = gzip.open(path, 'rt', encoding='utf-8').read().splitlines()
    specs = {json.loads(ln)['spec']: json.loads(ln)['json'] for ln in lines if '"kind": "spec"' in ln or '"kind":"spec"' in ln}
    parents = [json.loads(ln) for ln in lines if '"gid"' in ln]
    spec_txt = specs[parents[0]['spec']]
    lines += [json.dumps(r) for r in extra(parents, spec_txt, random.Random(seed + 1))]
    with gzip.open(path, 'wt', encoding='utf-8') as f:
        f.write('\n'.join(lines) + '\n')
    return run, parents, spec_txt


def long_parents(parents):
    return [p for p in parents if p['n_actions'] > 120 and p['over']]


@pytest.fixture(scope='module')
def branched(tmp_path_factory):
    tmp = tmp_path_factory.mktemp('gdb_branch')
    made = {}

    def extra(parents, spec_txt, rng):
        ps = long_parents(parents)
        assert len(ps) >= 2
        a, b = ps[0], ps[1]
        made['played'] = branch_record(a, spec_txt, a['n_actions'] // 3, rng)
        made['merged'] = branch_record(b, spec_txt, b['n_actions'] // 2, rng, merge_after=3)
        made['parents'] = (a, b)
        # (more played branches, for the series: one of each other long game, from a quarter of its actions)
        made['more'] = [branch_record(p, spec_txt, p['n_actions'] // 4, rng) for p in ps[2:]]
        return [made['played'], made['merged']] + made['more']

    run, parents, spec_txt = build_run(tmp, extra, games=10)
    db = str(tmp / 'db')
    GDB.ingest(run, db, sample=1.0, log=lambda *a: None)
    return run, db, parents, spec_txt, made


def test_a_branch_game_id_is_its_own_and_a_game_id_is_unchanged():
    rec = {'it': 3, 'src': 'w0', 'gid': 12, 'seed': 99, 'n_actions': 140}
    old = int.from_bytes(hashlib.blake2b(b'3|w0|12|99|140', digest_size=8).digest(), 'little') & ((1 << 63) - 1)
    assert GDB.game_id(rec) == old
    br = dict(rec, branch={'from': 40})
    assert GDB.game_id(br) != old
    assert GDB.game_id(br) != GDB.game_id(dict(rec, branch={'from': 41}))


def test_branch_games_carry_their_columns_and_a_merged_one_its_games_result(branched):
    run, db, parents, spec_txt, made = branched
    con = GDB.connect(db)
    games = table(con, 'SELECT * FROM games ORDER BY game_id')
    assert sum(1 for g in games if g['is_branch']) == 2 + len(made['more'])
    assert all(g['branch_from'] is None for g in games if not g['is_branch'])
    played = next(g for g in games if g['is_branch'] and g['branch_merged'] is None)
    merged = next(g for g in games if g['branch_merged'] is not None)
    pb = made['played']['branch']
    assert (played['branch_from'], played['branch_alt'], played['branch_tag']) == (pb['from'], pb['alt'], 1)
    assert abs(played['branch_gap'] - 0.0125) < 1e-9 and played['method'] != 'merged'
    mb = made['merged']['branch']
    assert merged['branch_merged'] == 3 and merged['branch_from'] == mb['from']
    a, b = made['parents']
    parent_row = next(g for g in games if not g['is_branch'] and g['gid'] == b['gid'])
    assert merged['method'] == 'merged' and merged['reason'] == 'Merged'
    assert merged['winner'] == parent_row['winner'] and (merged['score0'], merged['score1']) == (parent_row['score0'], parent_row['score1'])
    assert merged['turns'] <= parent_row['turns']
    # the branch and its game: the same gid and seed, other game ids
    assert len({g['game_id'] for g in games}) == len(games)


def test_a_branch_counts_its_rows_from_its_branch_point(branched):
    run, db, parents, spec_txt, made = branched
    con = GDB.connect(db)
    for which in ('played', 'merged'):
        rec = made[which]
        gid = GDB.game_id(dict(rec, it=1, src=rec['src']))
        ev = table(con, f'SELECT action, seq FROM events WHERE game_id = {gid} ORDER BY seq')
        frm = rec['branch']['from']
        assert ev and min(e['action'] for e in ev) >= frm
        assert [e['seq'] for e in ev] == list(range(len(ev)))
        # the combo rows: the full replay's from the branch point (a replay of the whole line, the cut at `from`)
        whole = dict(rec)
        whole.pop('branch')
        if which == 'merged':
            whole['over'] = False
        import combo_stats as C
        decks, mains = GDB.seat_decks(json.loads(spec_txt), rec.get('setup'))
        _, events_all, combo_all = GDB.replay_full(E, C, spec_txt, whole, decks, mains) if which == 'played' else (None, None, None)
        if which == 'played':
            n_casts = sum(1 for e in events_all if e[5] == 'cast' and e[1] >= frm)
            got = table(con, f"SELECT count(*) AS n FROM events WHERE game_id = {gid} AND type = 'cast'")[0]['n']
            assert got == n_casts
            b_all = [x for x in combo_all['bauble'] if x['action'] >= frm]
            got_b = table(con, f'SELECT count(*) AS n FROM bauble WHERE game_id = {gid}')[0]['n']
            assert got_b == len(b_all)


def test_a_merged_branch_with_another_result_or_no_game_fails_loudly(tmp_path):
    def wrong(parents, spec_txt, rng):
        p = long_parents(parents)[0]
        flip = [1.0 - x for x in p['result']]
        return [branch_record(p, spec_txt, p['n_actions'] // 2, rng, merge_after=2, result=flip)]
    run, _, _ = build_run(tmp_path / 'a', wrong)
    with pytest.raises(RuntimeError, match='merged branch'):
        GDB.ingest(run, str(tmp_path / 'a' / 'db'), sample=0.0, log=lambda *a: None)

    def orphan(parents, spec_txt, rng):
        p = long_parents(parents)[0]
        return [dict(branch_record(p, spec_txt, p['n_actions'] // 2, rng, merge_after=2), gid=999)]
    run, _, _ = build_run(tmp_path / 'b', orphan)
    with pytest.raises(RuntimeError, match="without its game's record"):
        GDB.ingest(run, str(tmp_path / 'b' / 'db'), sample=0.0, log=lambda *a: None)


def test_the_behaviour_series_leave_branch_games_out_unless_asked(branched):
    run, db, parents, spec_txt, made = branched
    con = GDB.connect(db)
    all_b = table(con, 'SELECT count(*) AS n FROM bauble')[0]['n']
    games_b = table(con, 'SELECT count(*) AS n FROM bauble JOIN games USING (game_id) WHERE NOT is_branch')[0]['n']
    branch_b = all_b - games_b
    assert branch_b > 0, 'the branches activated no Bauble: pick another seed'
    s0 = GDB.bauble_blunder_series(run, db=db)
    s1 = GDB.bauble_blunder_series(run, db=db, include_branches=True)
    tot = lambda s: sum(r['activations'] for r in s if r['bauble'] == 'all')  # noqa: E731
    assert tot(s0) == games_b and tot(s1) == all_b
    # the Doomsday series counts games: the branches only when asked
    d0 = GDB.doomsday_series(run, db=db, deck=DELVER)
    d1 = GDB.doomsday_series(run, db=db, deck=DELVER, include_branches=True)
    n = lambda s: sum(r['games'] for r in s)  # noqa: E731
    nb = 2 + len(made['more'])
    assert n(d1) == n(d0) + 2 * nb      # (each game counts once per seat with the deck: a mirror, two seats)
