"""Every game the system plays in the game database (docs/gamedb.md, docs/ENGINE.md 190): records of every kind under a
run (self-play, evaluation, the yardsticks' cells / baselines / best-of-one - their farm jobs' traced games too -, the
publisher's replays) ingested with their kind and both players in their seats, the views carrying them, the Doomsday
series still self-play's alone; the engine identity (a unit of another engine or of none refused loudly, handed to
another checkout with --engine-checkout); the yardsticks' recording context (kind, iteration, labels, each match's
pairing and chooser) and the publisher replays' game seeds (zero.play's). Random-move Legacy games, no network, no
process started. Needs duckdb and pyarrow (skipped without)."""
import base64
import gzip
import json
import os
import random
import struct
import sys

import pytest

pytest.importorskip('duckdb')
pytest.importorskip('pyarrow')

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import mtgz.az  # noqa: E402,F401
from mtgz import _engine as E  # noqa: E402
from mtgz.az import yardsticks as Y  # noqa: E402
import combo_stats as C  # noqa: E402
import gamedb as GDB  # noqa: E402
from zero import gamerec as GR  # noqa: E402

from test_combo_stats import DD, DELVER, SNS  # noqa: E402
from test_gamedb import combo_file  # noqa: E402

QUIET = dict(log=lambda *a: None)


def random_game(spec, seed, rng, max_moves=3000):
    g = E.Game('mtg', spec, 2, seed)
    acts = []
    while g.to_move is not None and len(acts) < max_moves:
        a = rng.choice(list(g.legal()))
        acts.append(a)
        g.step(a)
    over = g.to_move is None
    return g, acts, over, ([float(x) for x in g.result()] if over else [0.5, 0.5])


def match_items(kind, it, decks, n, seed, rng, players):
    """n random games of `decks` as game records of `kind` -> [(record, spec)]."""
    spec = C.deck_spec(*decks)
    out = []
    for gid in range(n):
        s = E.game_seed_of(seed, gid, False)
        g, acts, over, res = random_game(spec, s, rng)
        rec = {'v': 1, 'kind': kind, 'it': it, 'src': 'local', 'gid': gid, 'seed': s, 'spec': GR.spec_key(spec), 'n': 2,
               'actions': base64.b64encode(struct.pack(f'<{len(acts)}H', *acts)).decode(), 'n_actions': len(acts),
               'setup': json.dumps({'names': list(decks)}), 'over': over, 'aborted': False, 'resigned': None,
               'result': res, 'decisions': None, 'rounds': None,
               'players': players if gid % 2 == 0 else players[::-1], 'meta': {'pair': list(decks)}}
        out.append((rec, spec))
    return out


CK = {'ckpt': 'r/iter_0032', 'sha': 'aaaaaaaaaaaa', 'label': 'iter_0032', 'sims': 64, 'rule': 'cq'}
REF = {'ckpt': 'r/iter_0000', 'sha': 'bbbbbbbbbbbb', 'label': 'gen0', 'sims': 64, 'rule': 'cq'}
ANC = {'ckpt': 'fork0', 'sha': 'cccccccccccc', 'label': 'anchor:fork0', 'sims': 64, 'rule': 'cq'}


@pytest.fixture(scope='module')
def kinds(tmp_path_factory):
    base = tmp_path_factory.mktemp('gamedb_all')
    rng = random.Random(11)
    run = str(base / 'runs' / 'r')
    p = str(base / 'sp.b000.games.json.gz')
    combo_file(p, (DD, DELVER), 101, 20, rng)
    GDB.import_combo(run, [p], it=32, **QUIET)                   # (self-play: iteration 32)
    rec = os.path.join(run, 'records')
    GR.MatchSink(rec, 'cells', it=32).add(match_items('cells', 32, (DD, DELVER), 12, 7, rng, [CK, REF]))
    GR.MatchSink(rec, 'cells', it=48).add(match_items('cells', 48, (DD, SNS), 6, 8, rng, [CK, REF]))
    GR.MatchSink(rec, 'anchor', it=32).add(match_items('anchor', 32, (DD, DD), 8, 9, rng, [CK, ANC]))
    db = str(base / 'db')
    GDB.ingest(run, db, **QUIET)
    return base, run, db


def q(db, sql):
    con = GDB.connect(db)
    cur = con.execute(sql)
    cols = [d[0] for d in cur.description]
    return [dict(zip(cols, r)) for r in cur.fetchall()]


def test_every_kind_is_ingested_with_its_players(kinds):
    base, run, db = kinds
    by = {(r['kind'], r['iter']): r['n'] for r in q(db, 'SELECT kind, iter, count(*) AS n FROM games GROUP BY ALL')}
    assert by == {('selfplay', 32): 20, ('cells', 32): 12, ('cells', 48): 6, ('anchor', 32): 8}
    st = GDB.read_state(db)
    assert list(st['iterations']) == ['32'] and len(st['units']) == 3 and not st['refused']
    # the cells games of generation 32: the checkpoint against the reference, its seat alternating
    g = q(db, "SELECT gid, p0_label, p1_label, p0_ckpt, p0_sha, p0_sims, p0_rule, deck0, meta FROM games "
              "WHERE kind = 'cells' AND iter = 32 ORDER BY gid")
    assert [(x['p0_label'], x['p1_label']) for x in g[:2]] == [('iter_0032', 'gen0'), ('gen0', 'iter_0032')]
    assert g[0]['p0_ckpt'] == 'r/iter_0032' and g[0]['p0_sha'] == 'aaaaaaaaaaaa' and g[0]['p0_sims'] == 64
    assert g[0]['p0_rule'] == 'cq' and g[0]['deck0'] == DD and json.loads(g[0]['meta'])['pair'] == [DD, DELVER]
    # anchor games by opponent: one row per seat of the checkpoint
    a = q(db, "SELECT player_label, opp_label, count(*) AS n FROM game_results WHERE kind = 'anchor' "
              "AND player_label LIKE 'iter_%' GROUP BY ALL")
    assert a == [{'player_label': 'iter_0032', 'opp_label': 'anchor:fork0', 'n': 8}]
    # self-play's seats: the network its samples were searched with (none in imported games)
    assert q(db, "SELECT count(*) AS n FROM games WHERE kind = 'selfplay' AND p0_ckpt IS NULL")[0]['n'] == 20
    # Doomsday piles in evaluation vs self-play: the kind on every view
    piles = q(db, 'SELECT kind, count(*) AS n FROM doomsday_piles GROUP BY 1')
    assert {x['kind'] for x in piles} <= {'selfplay', 'cells', 'anchor'}
    assert q(db, 'SELECT count(*) AS n FROM casts')[0]['n'] == sum(
        x['n'] for x in q(db, 'SELECT kind, count(*) AS n FROM casts GROUP BY 1'))


def test_the_doomsday_series_stays_self_plays(kinds):
    base, run, db = kinds
    rows = GDB.doomsday_series(run, 0, db=db)
    assert [r['iter'] for r in rows] == [32] and rows[0]['games'] == 20      # (no cells or anchor game in it)


def test_ingest_of_other_units_is_incremental(kinds):
    base, run, db = kinds
    st = GDB.read_state(db)
    GDB.ingest(run, db, **QUIET)
    assert GDB.read_state(db)['units'] == st['units']
    GR.MatchSink(os.path.join(run, 'records'), 'bo1', it=48).add(
        match_items('bo1', 48, (DELVER, DELVER), 4, 3, random.Random(2), [CK, REF]))
    GDB.ingest(run, db, **QUIET)
    st2 = GDB.read_state(db)
    assert len(st2['units']) == 4 and all(st2['units'][k] == v for k, v in st['units'].items())


def test_another_engine_and_no_engine_are_refused_loudly(tmp_path, monkeypatch):
    rng = random.Random(5)
    rec = str(tmp_path / 'rec')
    other = {'pkg': 'mtgz', 'commit': 'c0ffee' * 6 + 'c0ff', 'core': 'beef' * 10, 'engine': '0123456789abcdef'}
    GR.MatchSink(rec, 'h2h', build=other).add(match_items('h2h', None, (DELVER, DELVER), 2, 1, rng, [CK, REF]))
    GR.MatchSink(rec, 'h2h').add(match_items('h2h', None, (DELVER, DELVER), 2, 2, rng, [CK, REF]))
    # a file without a build line (zero-core before docs/az.md 59.4)
    items = match_items('combo', None, (DD, DD), 2, 3, rng, [CK, CK])
    d = os.path.join(rec, 'combo', 'misc')
    os.makedirs(d)
    with gzip.open(os.path.join(d, 'old.jsonl.gz'), 'wt', encoding='utf-8') as f:
        f.write(json.dumps({'v': 1, 'kind': 'spec', 'spec': items[0][0]['spec'], 'json': items[0][1]}) + '\n')
        for r, _ in items:
            f.write(json.dumps(r) + '\n')
    db = str(tmp_path / 'db')
    lines = []
    st = GDB.ingest(None, db, records=rec, log=lines.append)
    assert sum(u['games'] for u in st['units'].values()) == 2           # (this engine's file only)
    why = {k.split('/')[0]: v['why'] for k, v in st['refused'].items()}
    assert why == {'m=h2h.misc': 'other_engine', 'm=combo.misc': 'unidentified'}
    assert any('REFUSED (other_engine)' in x and 'c0ffeec0ffee' in x and '--engine-checkout' in x for x in lines)
    with pytest.raises(SystemExit) as e:                                  # (the CLI: exit 3 while units are refused)
        GDB.main(['ingest', '--records', rec, '--db', db])
    assert e.value.code == 3
    # --engine-checkout: the refused units of that checkout's engine handed to its gamedb, the same database
    calls = []
    monkeypatch.setattr(GDB, 'checkout_engine', lambda p: dict(other))
    import subprocess
    monkeypatch.setattr(subprocess, 'run', lambda cmd, cwd=None: calls.append((cmd, cwd)) or
                        type('R', (), {'returncode': 0})())
    GDB.ingest(None, db, records=rec, engine_checkout=str(tmp_path / 'old'), **QUIET)
    (cmd, cwd), = calls
    assert cmd[2:4] == ['ingest', '--db'] and '--only-engine' in cmd and other['engine'] in cmd
    assert cwd == str(tmp_path / 'old') and '--records' in cmd
    # --allow-unidentified (from a checkout of the engine that played them): ingested
    st = GDB.ingest(None, db, records=rec, allow_unidentified=True, **QUIET)
    assert sum(u['games'] for u in st['units'].values()) == 4 and list(st['refused']) == [
        k for k in st['refused'] if k.startswith('m=h2h')]
    # --only-engine of another engine: refused before anything (the delegate's guard)
    with pytest.raises(SystemExit, match='only-engine'):
        GDB.ingest(None, db, records=rec, only_engine='feedfeedfeedfeed', **QUIET)
    assert GDB.judge([{'engine': 'dirty', 'module': 'm'}], {'engine': 'x', 'module': 'm'})[1] == 'dirty'


def test_the_engine_paths_are_the_artifact_caches_rust_inputs():
    import zcache
    assert tuple(mtgz.az.ENGINE_PATHS) == tuple(zcache.RUST_PATHS)
    b = GDB.my_build()
    assert b['engine'] and b['commit'] and b['core']


def test_publisher_replays_are_indexed_with_zero_plays_game_seeds(tmp_path):
    from zero.play import PyDriver
    spec = C.deck_spec(DD, DELVER)
    d = PyDriver('mtg', 3, lambda k: ['random', 'random'], spec=spec, concurrency=3, seed=77)
    for slot in d.active:
        g = E.Game('mtg', spec, 2, GDB.replay_seed(77, slot.gid))
        assert g.export_state() == slot.game.export_state()
    rng = random.Random(4)
    rd = tmp_path / 'run' / 'replays'
    rd.mkdir(parents=True)
    for k in range(3):
        g, acts, over, res = random_game(spec, GDB.replay_seed(77, k), rng)
        rep = {'format': 'zero-replay/1', 'game': 'mtg', 'spec': spec, 'seed': 77, 'game_index': k, 'title': f'G #{k}',
               'source': {'iter': 64}, 'frames': [{'a': a} for a in acts], 'result': res, 'over': over, 'rounds': 0,
               'seats': [{'name': 'iter_0064', 'kind': 'nn', 'run': 'r', 'iter': 64, 'sims': 128},
                         {'name': 'iter_0064', 'kind': 'nn', 'run': 'r', 'iter': 64, 'sims': 128}]}
        if k < 2:
            rep['build'] = GDB.my_build()                # (the third: a replay without game_records - not indexed)
        with gzip.open(rd / f'g_{k:03d}.json.gz', 'wt', encoding='utf-8') as f:
            json.dump(rep, f)
    lines = []
    db = str(tmp_path / 'db')
    GDB.ingest(str(tmp_path / 'run'), db, log=lines.append)
    g = q(db, "SELECT kind, iter, p0_ckpt, p0_sims, deck0, executor FROM games ORDER BY gid")
    assert g == [{'kind': 'publisher', 'iter': 64, 'p0_ckpt': 'r/iter_0064', 'p0_sims': 128, 'deck0': 'p0',
                  'executor': 'publisher'}] * 2
    assert any('1 replay(s) without an engine id' in x for x in lines)


def test_the_yardsticks_record_under_their_kind_with_each_matchs_pairing(tmp_path, monkeypatch):
    """The local cells: each chooser match recorded under the measurement's sink (kind, iteration, the checkpoint
    and the reference as A and B) with its pairing and chooser; a baseline under 'cells_base'; nothing without a
    records directory."""
    from zero import h2h
    seen = []

    def fake_match(a, b, games, sims, seed=0, spec=None, **kw):
        s = GR.active()
        seen.append(None if s is None else (s.kind, s.it, s.labels, dict(s.meta)))
        return {'rows': [{'gid': g, 'result': [1.0, 0.0]} for g in range(games)], 'unfinished': 0, 'select_a': 'v',
                'a_score': 0.5}
    monkeypatch.setattr(h2h, 'match', fake_match)
    ck, ref = tmp_path / 'c.pt', tmp_path / 'r.pt'
    ck.write_bytes(b'c')
    ref.write_bytes(b'r')
    D, S, N = Y.CELL_DECKS
    ys = Y.normalize({'bo1_games': 0, 'blunders': False, 'cells_games': 8, 'cells_refs': ['r=' + str(ref)],
                      'cells_boot': 0})
    rec = str(tmp_path / 'records')
    Y.measure_cells_core('cells', str(ck), 16, str(ref), 'r', (N, N), 8, ys, {}, str(tmp_path / 'cache'), records=rec)
    assert seen == [('cells', 16, {'a': 'iter_0016', 'b': 'r'}, {'ref': 'r', 'pair': [N, N], 'chooser': c})
                    for c in (0, 1)]
    seen.clear()
    Y.measure_base_core(str(ref), 'r', (D, S), 8, ys, {}, str(tmp_path / 'cache'), records=rec)
    assert seen == [('cells_base', None, {'a': 'r', 'b': 'r'}, {'ref': 'r', 'pair': [D, S], 'chooser': c})
                    for c in (0, 1)]
    seen.clear()
    Y.measure_cells_core('cells_gen', str(ck), 32, str(ref), 'iter_0016', (D, D), 8, ys, {}, str(tmp_path / 'cache'))
    assert seen == [None, None]                                          # (no records directory: nothing)
    assert Y.records_dir('x', {'game_records': 1}) == os.path.join('x', 'records')
    assert Y.records_dir('x', {}) is None


def test_a_farm_jobs_traced_games_become_records(tmp_path):
    """The farm backends' rows carry each game's traced record (eval_trace): written as records of the measurement's
    kind (A = net 1 in seat gid % 2) that the database replays."""
    rng = random.Random(9)
    spec = C.deck_spec(DELVER, SNS)
    rows = []
    for gid in range(4):
        s = E.game_seed_of(33, gid, True)
        g, acts, over, res = random_game(spec, s, rng)
        rows.append({'gid': gid, 'rec': {'gid': gid, 'n': 2, 'seed': s, 'steps': acts, 'over': over, 'aborted': False,
                                         'resigned': None, 'rounds': 0, 'setup': json.dumps({'names': [DELVER, SNS]}),
                                         'result': [struct.unpack('<I', struct.pack('<f', x))[0] for x in res]}})
    ck, ref = tmp_path / 'c.pt', tmp_path / 'o.pt'
    ck.write_bytes(b'c')
    ref.write_bytes(b'o')
    sink = Y._sink(str(tmp_path / 'rec'), 'bo1', 16, 'iter_0016', 'pb1')
    Y._farm_records(sink, rows, spec, str(ck), str(ref), 64, 'cq', {'backend': 'farm'})
    with pytest.raises(RuntimeError, match='no record'):
        Y._farm_records(sink, [{'gid': 0}], spec, str(ck), str(ref), 64, 'cq')
    db = str(tmp_path / 'db')
    GDB.ingest(None, db, records=str(tmp_path / 'rec'), **QUIET)
    g = q(db, 'SELECT gid, kind, iter, p0_label, p1_label, executor, deck0 FROM games ORDER BY gid')
    assert [(x['p0_label'], x['p1_label']) for x in g] == [('iter_0016', 'pb1'), ('pb1', 'iter_0016')] * 2
    assert {(x['kind'], x['iter'], x['executor'], x['deck0']) for x in g} == {('bo1', 16, 'farm', DELVER)}


def test_every_query_of_the_docs_runs(kinds):
    """docs/gamedb.md's example queries (the ten and the kinds' five) run on a database of every kind."""
    import re
    base, run, db = kinds
    text = open(os.path.join(ROOT, 'docs', 'gamedb.md'), encoding='utf-8').read()
    blocks = re.findall(r'```sql\n(.*?)```', text, re.S)
    stmts = [x.strip() for b in blocks for x in b.split(';') if x.strip() and not
             all(ln.strip().startswith('--') for ln in x.strip().splitlines())]
    assert len(stmts) == 15
    con = GDB.connect(db)
    for sql in stmts:
        con.execute(sql).fetchall()
