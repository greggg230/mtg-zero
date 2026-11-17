"""tools/gamedb.py (docs/gamedb.md, docs/ENGINE.md 187): game records of random-move Legacy games (the open decklists)
ingested into the database agree with tools/combo_stats.py's scan of the same games game by game; the light replay's
games rows are the full replay's; sampling is deterministic by game id; ingest is incremental and idempotent; the views
and prune; a game that does not replay fails the ingest loudly. Needs duckdb and pyarrow (skipped without)."""
import collections
import gzip
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
import combo_stats as C  # noqa: E402
import gamedb as GDB  # noqa: E402
from zero import gamerec as GR  # noqa: E402

from test_combo_stats import DD, DELVER, SNS, random_row  # noqa: E402


def combo_file(path, decks, seed, games, rng):
    spec = C.deck_spec(*decks)
    rows = [random_row(spec, seed, gid, rng) for gid in range(games)]
    rec = {'format': C.FORMAT, 'label': os.path.basename(path).split('.')[0], 'decks': list(decks), 'sims': [1, 1],
           'spec': spec, 'driver_seed': seed, 'batch': 0, 'games': games, 'wall_s': 1.0, 'cpu_s': 1.0, 'failures': 0,
           'rows': rows}
    with gzip.open(path, 'wt', encoding='utf-8') as f:
        json.dump(rec, f)
    return rec


@pytest.fixture(scope='module')
def probe(tmp_path_factory):
    base = tmp_path_factory.mktemp('gamedb')
    rng = random.Random(7)
    files = []
    for k, decks in enumerate(((DD, DELVER), (DD, SNS), (SNS, DELVER))):
        p = str(base / f'g{k}.b000.games.json.gz')
        combo_file(p, decks, 100 + k, 40, rng)
        files.append(p)
    run = str(base / 'run')
    idx = GDB.import_combo(run, files, it=1, log=lambda *a: None)
    db = str(base / 'db')
    GDB.ingest(run, db, sample=1.0, log=lambda *a: None)
    return base, run, db, files, idx


def table(con, sql):
    cur = con.execute(sql)
    cols = [d[0] for d in cur.description]
    return [dict(zip(cols, r)) for r in cur.fetchall()]


def test_the_database_agrees_with_combo_stats_scan(probe):
    base, run, db, files, idx = probe
    specs, recs = GR.read_iteration(run, 1)
    by = {r['gid']: dict(r, it=1, src='combo') for r in recs}
    truth = {}
    for path in files:
        rec = json.load(gzip.open(path, 'rt', encoding='utf-8'))
        meta = {k: v for k, v in rec.items() if k != 'rows'}
        for row in rec['rows']:
            truth[(os.path.basename(path), row['gid'])] = C.scan_row(E, meta, row)
    con = GDB.connect(db)
    games = {g['game_id']: g for g in table(con, 'SELECT * FROM games')}
    dd = collections.defaultdict(list)
    for x in table(con, 'SELECT * FROM doomsday ORDER BY idx'):
        dd[x['game_id']].append(x)
    sp = collections.defaultdict(list)
    for x in table(con, 'SELECT * FROM spells ORDER BY idx'):
        sp[x['game_id']].append(x)
    resolved = 0
    for e in idx:
        s = truth[(e['file'], e['gid'])]
        gid = GDB.game_id(by[e['game']])
        g = games[gid]
        assert (g['method'], g['winner'], g['turns'], g['starting'], [g['mull0'], g['mull1']], [g['deck0'], g['deck1']]) == \
            (s['method'], s['winner'], s['turn'], s['starting'], s['mulligans'], s['decks'])
        mine = [{'p': x['caster'], 'turn': x['turn'], 'pile': list(x['pile']), 'hand': list(x['hand']),
                 'choices': list(x['choices']), 'oracle_in_pile': x['oracle_in_pile'], 'ff': x['oracle']} for x in dd[gid]]
        assert mine == [{k: x[k] for k in ('p', 'turn', 'pile', 'hand', 'choices', 'oracle_in_pile', 'ff')}
                        for x in s['doomsday']]
        assert [(x['card'], x['player'], x['outcome'], x['countered_by']) for x in sp[gid]] == \
            [(x['card'], x['p'], x['outcome'], x['by']) for x in s['spells']]
        resolved += len(mine)
    assert resolved > 0, 'a Doomsday resolved in the random games'
    n = con.execute('SELECT count(*) FROM doomsday_piles').fetchone()[0]
    assert n == resolved == sum(len(t['doomsday']) for t in truth.values())
    # the pile cards view: one row per card of each pile, in order
    k = con.execute('SELECT count(*) FROM doomsday_pile_cards').fetchone()[0]
    assert k == sum(len(x['pile']) for t in truth.values() for x in t['doomsday'])
    first = con.execute('SELECT card FROM doomsday_pile_cards WHERE pos = 1 ORDER BY game_id, idx').fetchall()
    assert len(first) == sum(1 for t in truth.values() for x in t['doomsday'] if x['pile'])
    # casts: every cast line
    casts = con.execute('SELECT count(*) FROM casts').fetchone()[0]
    assert casts == sum(c[2] for t in truth.values() for c in t['casts'])


def test_light_rows_are_the_full_rows_and_sampling_is_deterministic(probe):
    base, run, db, files, idx = probe
    light = str(base / 'db_light')
    GDB.ingest(run, light, sample=0.0, log=lambda *a: None)
    cols = 'game_id, starting, turns, mull0, mull1, life0, life1, winner, reason, method, score0, score1, decisions'
    a = GDB.connect(db).execute(f'SELECT {cols} FROM games ORDER BY game_id').fetchall()
    b = GDB.connect(light).execute(f'SELECT {cols} FROM games ORDER BY game_id').fetchall()
    assert a == b and len(a) == 120
    assert GDB.connect(light).execute('SELECT count(*) FROM events').fetchone()[0] == 0
    half = str(base / 'db_half')
    GDB.ingest(run, half, sample=0.5, log=lambda *a: None)
    full = {g for (g,) in GDB.connect(half).execute('SELECT game_id FROM games WHERE detailed').fetchall()}
    assert 30 < len(full) < 90 and full == {g for (g, *_) in a if GDB.sampled(g, 0.5)}
    assert {g for (g, *_) in a if GDB.sampled(g, 0.25)} <= full, 'a smaller share is a subset'


def test_the_doomsday_series(probe):
    base, run, db, files, idx = probe
    con = GDB.connect(db)
    rows = GDB.doomsday_series(run, 0, db=db)
    assert len(rows) == 1 and rows[0]['iter'] == 1 and rows[0]['opp_deck'] is None
    r = rows[0]
    piles = table(con, "SELECT oracle, count(*) AS n FROM doomsday_piles WHERE caster_deck = 'doomsday_8027957' GROUP BY 1")
    by = {p['oracle']: p['n'] for p in piles}
    assert r['resolutions'] == sum(by.values()) > 0
    assert (r['own_pile'], r['forced'], r['in_hand'], r['not_findable'], r['missed']) == tuple(
        by.get(k, 0) for k in ('voluntary', 'forced', 'in_hand', 'not_findable', 'missed'))
    g = table(con, "SELECT count(*) AS n, count(*) FILTER (WHERE outcome = 'win' AND method = 'oracle') AS o FROM game_results "
                   "WHERE deck = 'doomsday_8027957'")[0]
    assert (r['games'], r['oracle_wins']) == (g['n'], g['o']) and r['detailed_games'] == r['games']
    opp = GDB.doomsday_series(run, 0, by_opponent=True, db=db)
    assert {x['opp_deck'] for x in opp} == {DELVER, SNS} and sum(x['resolutions'] for x in opp) == r['resolutions']
    assert GDB.doomsday_series(run, 2, db=db) == []


def test_ingest_is_incremental_idempotent_and_prunes(probe):
    base, run, db, files, idx = probe
    st = GDB.read_state(db)
    assert list(st['iterations']) == ['1']
    GDB.ingest(run, db, log=lambda *a: None)       # nothing new
    assert GDB.read_state(db)['iterations']['1']['time'] == st['iterations']['1']['time']
    # a second iteration of records: only it is ingested
    GDB.import_combo(run, files[:1], it=2, log=lambda *a: None)
    GDB.ingest(run, db, log=lambda *a: None)
    st2 = GDB.read_state(db)
    assert sorted(st2['iterations']) == ['1', '2'] and st2['iterations']['1']['time'] == st['iterations']['1']['time']
    con = GDB.connect(db)
    assert con.execute('SELECT count(*) FROM games WHERE iter = 2').fetchone()[0] == 40
    # prune: iteration 1 is older than the newest 1: a sample kept, its events with it; raw records deleted
    GDB.prune(run, db, keep_iters=1, old_sample=0.5, delete_raw=True, log=lambda *a: None)
    con = GDB.connect(db)
    kept = {g for (g,) in con.execute('SELECT game_id FROM games WHERE iter = 1').fetchall()}
    assert 20 < len(kept) < 100 and all(GDB.sampled(g, 0.5) for g in kept)
    ev = {g for (g,) in con.execute('SELECT DISTINCT game_id FROM events WHERE iter = 1').fetchall()}
    assert ev <= kept
    assert con.execute('SELECT count(*) FROM games WHERE iter = 2').fetchone()[0] == 40
    assert not os.path.exists(GR.iter_dir(run, 1)) and not os.path.exists(GR.iter_dir(run, 2))


def test_a_game_that_does_not_replay_fails_loudly(tmp_path):
    rng = random.Random(3)
    p = str(tmp_path / 'g.b000.games.json.gz')
    combo_file(p, (DELVER, DELVER), 5, 3, rng)
    run = str(tmp_path / 'run')
    GDB.import_combo(run, [p], it=1, log=lambda *a: None)
    # tamper with a record: another seed
    d = GR.iter_dir(run, 1)
    name = next(iter(GR.read_done(run, 1)['files']))
    lines = gzip.open(os.path.join(d, name), 'rt', encoding='utf-8').read().splitlines()
    k = next(i for i, ln in enumerate(lines) if '"gid"' in ln)
    o = json.loads(lines[k])
    o['seed'] = int(o['seed']) ^ 12345
    lines[k] = json.dumps(o)
    with gzip.open(os.path.join(d, name), 'wt', encoding='utf-8') as f:
        f.write('\n'.join(lines) + '\n')
    with pytest.raises(RuntimeError, match='game 0'):
        GDB.ingest(run, str(tmp_path / 'db'), sample=0.0, log=lambda *a: None)

