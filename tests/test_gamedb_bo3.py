"""Best-of-three matches in the game database (docs/gamedb.md "Best-of-three matches"): a match recorded as one chained
contest (zero-core records/1: its episode boundaries `{"next": conceding seat or null}` in its steps, one episode per
game, docs/ENGINE.md 121) is replayed game by game - a `games` row per game with its match id, its game number and each
seat's 60 as played (its sideboarding against the main, dealt or chosen), its events tagged with the game, the Readers
fed one game at a time -, light rows equal to the full ones, ingest idempotent, and a database ingested before (its
chained contests skipped, its files without the match columns) read and filled in. Random-move Delver mirror matches with
per-seat dealt decks and random concessions, no network. Needs duckdb and pyarrow (skipped without)."""
import collections
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
from mtgz.az import decklists  # noqa: E402
import gamedb as GDB  # noqa: E402
from zero import gamerec as GR  # noqa: E402

QUIET = dict(log=lambda *a: None)
SPEC = decklists.mirror_bo3_spec(max_turns=10, game_episodes=True,
                                 dealt_decks={'p': 0.5, 'swaps': [1, 8], 'per_seat': True})
MAIN = json.loads(SPEC)['decks']['p0']


def play_match(seed, rng, concede_p, episodes):
    """A random-move match recorded as the drivers record a chained contest -> (record, what each game was: its number,
    its 60s (the match block's at its end), the seats that sideboarded before it, its winner, conceded)."""
    g = E.Game('mtg', SPEC, 2, seed)
    steps, eps, made = [], [], []
    sb = set()
    while True:
        while g.to_move is not None:
            st = json.loads(g.export_state())
            d = st.get('decision') or {}
            if d.get('kind') == 'Sideboard':
                sb.add(d['player'])
            elif g.can_concede and rng.random() < concede_p:
                break
            a = rng.choice(list(g.legal()))
            steps.append(a)
            g.step(a)
        end = json.loads(g.export_state())
        if g.to_move is None and g.aborted:
            made.append({'game_no': end['match']['game'], 'decks': end['match']['decks'], 'sb': sb, 'winner': None,
                         'conceded': None, 'aborted': True})
            eps.append({'over': False, 'aborted': True, 'resigned': None, 'result': [0.5, 0.5]})
            over, res = False, [0.5, 0.5]
            break
        conceded = None if g.to_move is None else g.to_move
        w = (1 - conceded) if conceded is not None else end.get('winner')
        made.append({'game_no': end['match']['game'], 'decks': end['match']['decks'], 'sb': sb, 'winner': w,
                     'conceded': conceded, 'aborted': False})
        gres = [float(x) for x in g.result()][:2] if conceded is None else [float(s == w) for s in (0, 1)]
        eps.append({'over': True, 'aborted': False, 'resigned': None if conceded is None else w, 'result': gres})
        nxt, res = g.next_episode(conceded)
        if nxt is None:
            over, res = True, [float(x) for x in res][:2]
            break
        steps.append({'next': conceded})
        g = nxt
        sb = set()
    rec = {'v': 1, 'gid': seed % 1000, 'seed': seed, 'spec': GR.spec_key(SPEC), 'n': 2, 'actions': None,
           'steps': steps, 'n_actions': len(steps), 'setup': json.dumps({'names': ['delver', 'delver']}),
           'over': over, 'aborted': not over, 'resigned': None, 'result': res, 'decisions': None, 'rounds': None,
           'nets': None}
    if episodes:
        rec['episodes'] = eps
    return rec, made


@pytest.fixture(scope='module')
def bo3(tmp_path_factory):
    base = tmp_path_factory.mktemp('gamedb_bo3')
    rng = random.Random(5)
    run = str(base / 'runs' / 'r')
    w = GR.Records(run)
    items, made = [], {}
    for k in range(8):
        seed = E.game_seed_of(77, k, False)
        rec, m = play_match(seed, rng, 0.004 if k % 2 else 0.0, episodes=k % 3 == 0)
        items.append((rec, SPEC))
        made[rec['gid']] = (rec, m)
    w.add(1, 'local', items)
    w.close(1)
    db = str(base / 'db')
    GDB.ingest(run, db, sample=1.0, **QUIET)
    light = str(base / 'light')
    GDB.ingest(run, light, sample=0.0, **QUIET)
    return base, run, db, light, made


def table(con, sql):
    cur = con.execute(sql)
    cols = [d[0] for d in cur.description]
    return [dict(zip(cols, r)) for r in cur.fetchall()]


def plan(decks):
    have = collections.Counter()
    for c, n in decks:
        have[c] += n
    main = collections.Counter(MAIN)
    return (sorted(f'{n} {c}' for c, n in have.items() if n), sorted(f'{have[c] - main[c]} {c}' for c in have if have[c] > main[c]),
            sorted(f'{main[c] - have[c]} {c}' for c in main if main[c] > have[c]))


def test_the_matches_are_split_into_their_games(bo3):
    base, run, db, light, made = bo3
    con = GDB.connect(db)
    games = table(con, 'SELECT * FROM games ORDER BY gid, game_no')
    by = collections.defaultdict(list)
    for g in games:
        by[g['gid']].append(g)
    assert set(by) == set(made)
    assert any(len(m) == 3 for _, m in made.values()) and any(x['conceded'] is not None for _, m in made.values() for x in m)
    for gid, rows in by.items():
        rec, m = made[gid]
        mid = GDB.game_id(dict(rec, it=1, src='local'))
        assert [r['game_no'] for r in rows] == [x['game_no'] for x in m] == list(range(1, len(m) + 1))
        assert {r['match_id'] for r in rows} == {mid} and {r['match_games'] for r in rows} == {len(m)}
        assert [r['game_id'] for r in rows] == [GDB.match_game_id(mid, x['game_no']) for x in m]
        wins = [0, 0]
        for r, x in zip(rows, m):
            assert (r['pre_wins0'], r['pre_wins1']) == tuple(wins)
            assert r['winner'] == x['winner'] and r['resigned'] == (x['conceded'] is not None)
            assert r['aborted'] == x['aborted'] and r['over'] == (not x['aborted'])
            if x['winner'] is not None:
                wins[x['winner']] += 1
            # the sideboard plan: the engine's 60 at the game's end, against the main
            for p in (0, 1):
                sixty, ins, outs = plan(x['decks'][p])
                assert sorted(r[f'deck{p}_60']) == sixty and sum(int(c.split(' ')[0]) for c in sixty) == 60
                assert sorted(r[f'deck{p}_in']) == ins and sorted(r[f'deck{p}_out']) == outs
                # dealt or chosen: a dealt seat never sideboards (its 60 dealt), a chosen one always decides
                if x['game_no'] == 1:
                    assert r[f'deck{p}_mode'] == 'main' and not ins and not outs
                else:
                    assert r[f'deck{p}_mode'] == ('chosen' if p in x['sb'] else 'dealt')
                    assert r[f'deck{p}_dealt'] == (p not in x['sb'])
        assert len({(r['deck0_dealt'], r['deck1_dealt']) for r in rows}) == 1
        res = rec['result']
        mw = None if not rec['over'] or res[0] == res[1] else int(res[1] > res[0])
        assert {r['match_winner'] for r in rows} == {mw} and {r['match_result'] for r in rows} == {f'{wins[0]}-{wins[1]}'}
    st = GDB.read_state(db)
    assert st['iterations']['1']['matches'] == len(made) and 'chained_skipped' not in st['iterations']['1']
    # a seat's results per game of the match (the view)
    gr = table(con, 'SELECT game_no, deck_mode, count(*) AS n FROM game_results GROUP BY ALL')
    assert {x['deck_mode'] for x in gr} == {'main', 'dealt', 'chosen'}


def test_events_are_tagged_with_their_game_and_read_per_game(bo3):
    base, run, db, light, made = bo3
    con = GDB.connect(db)
    bad = table(con, 'SELECT count(*) AS n FROM events e JOIN games g USING (game_id) WHERE e.game_no <> g.game_no')
    assert bad[0]['n'] == 0
    assert table(con, 'SELECT count(*) AS n FROM events WHERE game_id NOT IN (SELECT game_id FROM games)')[0]['n'] == 0
    # each game's events from its own actions: action -1 (its lines before its first), then 0 .. its actions
    rng = table(con, 'SELECT g.game_id, g.n_actions, min(e.action) AS lo, max(e.action) AS hi, list(e.seq ORDER BY e.seq) AS '
                     'seqs FROM events e JOIN games g USING (game_id) GROUP BY ALL')
    for x in rng:
        assert x['lo'] >= -1 and x['hi'] <= x['n_actions'] and x['seqs'] == list(range(len(x['seqs'])))
    # the sideboarding's choices: before games 2 and 3 only, of the seats that chose, without a turn of their own
    sb = table(con, "SELECT e.game_no, e.player, e.turn, g.deck0_mode, g.deck1_mode FROM events e JOIN games g "
                    "USING (game_id) WHERE e.type = 'choice' AND e.detail LIKE '%\"Sideboard\"%'")
    assert sb and all(x['game_no'] >= 2 and x['turn'] is None for x in sb)
    assert all((x['deck0_mode'], x['deck1_mode'])[x['player']] == 'chosen' for x in sb)
    # every game's turns begin again: a later game's first turn line is turn 1
    t = table(con, "SELECT game_no, min(turn) AS t FROM events WHERE type = 'turn' GROUP BY 1")
    assert {x['game_no'] for x in t} >= {1, 2} and all(x['t'] <= 2 for x in t)
    # the casts view carries the game number
    assert table(con, 'SELECT count(*) AS n FROM casts WHERE game_no IS NULL')[0]['n'] == 0
    # the Bauble Reader read each game alone: its activations' actions inside their game
    for x in table(con, 'SELECT b.action, g.n_actions FROM bauble b JOIN games g USING (game_id)'):
        assert 0 <= x['action'] < x['n_actions']


def test_the_light_rows_are_the_full_rows(bo3):
    base, run, db, light, made = bo3
    skip = {'detailed', 'lines_lost', 'last_attackers'}
    a = table(GDB.connect(db), 'SELECT * FROM games ORDER BY game_id')
    b = table(GDB.connect(light), 'SELECT * FROM games ORDER BY game_id')
    assert len(a) == len(b) and all(x['detailed'] for x in a) and not any(x['detailed'] for x in b)
    for x, y in zip(a, b):
        assert {k: v for k, v in x.items() if k not in skip} == {k: v for k, v in y.items() if k not in skip}
    assert table(GDB.connect(light), 'SELECT count(*) AS n FROM events')[0]['n'] == 0


def test_ingest_is_idempotent_and_fills_in_an_earlier_databases_skipped_matches(bo3):
    base, run, db, light, made = bo3
    before = table(GDB.connect(db), 'SELECT * FROM games ORDER BY game_id')
    ev = table(GDB.connect(db), 'SELECT count(*) AS n FROM events')[0]['n']
    logs = []
    GDB.ingest(run, db, sample=1.0, log=logs.append)
    assert any('nothing new' in x for x in logs)
    GDB.ingest(run, db, sample=1.0, redo=True, **QUIET)
    assert table(GDB.connect(db), 'SELECT * FROM games ORDER BY game_id') == before
    assert table(GDB.connect(db), 'SELECT count(*) AS n FROM events')[0]['n'] == ev
    # an earlier gamedb skipped the chained contests (`chained_skipped`): ingested again
    st = GDB.read_state(db)
    st['iterations']['1']['chained_skipped'] = len(made)
    GDB.write_state(db, st)
    logs = []
    GDB.ingest(run, db, sample=1.0, log=logs.append)
    assert any('best-of-three matches' in x for x in logs)
    assert table(GDB.connect(db), 'SELECT * FROM games ORDER BY game_id') == before


def test_an_earlier_databases_files_read_as_single_games(bo3, tmp_path):
    import pyarrow.parquet as pq
    base, run, db, light, made = bo3
    old = str(tmp_path / 'old')
    for t in ('games', 'events'):
        src = os.path.join(db, t, 'iter=0001', 'part.parquet')
        tb = pq.read_table(src)
        drop = [c for c in tb.column_names if c in ('game_no',) or c.startswith('match_') or c.startswith('pre_wins')
                or (c.startswith('deck') and '_' in c)]
        os.makedirs(os.path.join(old, t, 'iter=0001'))
        pq.write_table(tb.drop(drop), os.path.join(old, t, 'iter=0001', 'part.parquet'))
    con = GDB.connect(old)
    g = table(con, 'SELECT game_id, match_id, game_no, match_games, winner, match_winner, match_result, deck0_in FROM games')
    assert g and all(x['match_id'] == x['game_id'] and x['game_no'] == 1 and x['match_games'] == 1 for x in g)
    assert all(x['match_winner'] == x['winner'] and x['deck0_in'] is None for x in g)
    assert {x['match_result'] for x in g} <= {'1-0', '0-1', '0-0'}
    assert table(con, 'SELECT DISTINCT game_no FROM events') == [{'game_no': 1}]
    assert table(con, 'SELECT count(*) AS n FROM game_results')[0]['n'] == 2 * len(g)


def test_a_match_that_does_not_replay_fails_loudly(tmp_path):
    rng = random.Random(9)
    rec, m = play_match(E.game_seed_of(78, 0, False), rng, 0.0, episodes=False)
    i = next(k for k, a in enumerate(rec['steps']) if isinstance(a, dict))
    rec['steps'][i] = {'next': 0}                  # (a game the rules ended, recorded as a concession)
    run = str(tmp_path / 'r')
    w = GR.Records(run)
    w.add(1, 'local', [(rec, SPEC)])
    w.close(1)
    with pytest.raises(RuntimeError, match='conceded'):
        GDB.ingest(run, str(tmp_path / 'db'), **QUIET)


def test_the_sideboard_plan():
    sixty, ins, outs = GDB.sideboard_plan([['A', 3], ['B', 4], ['C', 0], ['D', 2]], ['A'] * 4 + ['B'] * 4 + ['C'] * 1)
    assert sixty == ['3 A', '4 B', '2 D'] and ins == ['2 D'] and outs == ['1 A', '1 C']
    assert GDB.sideboard_plan([['A', 4]], None) == (['4 A'], None, None)
    assert GDB.match_game_id(5, 1) != GDB.match_game_id(5, 2) and GDB.match_game_id(5, 1) < (1 << 63)
