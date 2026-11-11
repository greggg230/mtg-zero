"""tools/sealedai_publish.py (docs/ENGINE.md 107.4): the game log read incrementally (complete lines once, a line being
written later), the games of an unfinished iteration held until its metrics line, the per-iteration card counts the site
sums (only changed iterations uploaded again), the card record's info (the set's cards, the 17lands yardstick with its
license, the draw credit when due), the progress record (aux losses, the Elo pool's fit, the builder's score) and no local
path in anything posted."""
import json
import os
import re
import sys
import types

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import sealedai_publish as P  # noqa: E402
from mtgz.sealed import cardstats as CS  # noqa: E402

# the Worker's check (sealedai worker/index.js PATH_RE): a drive path, a backslash, a home directory, a checkpoint file
PATH_RE = re.compile(r'(^|[^A-Za-z])[A-Za-z]:[\\/]|\\|(^|[\s"\'(])/(Users|home|mnt|tmp|root|var)/|\.pt\b')
DECK_A = {'Plains': 8, 'Forest': 9, 'Icecave Crasher': 2, 'Galactic Wayfarer': 1, 'Not A Set Card': 1}
DECK_B = {'Island': 17, 'Bombard': 2}


def seat(deck, cards, mode, colors):
    return {'mode': mode, 'pool_seed': 1, 'colors': colors, 'splash': '', 'lands': 17, 'candidate': None, 'deck': deck,
            'draws': [[0, 0, c] for c in cards[:7]] + [[2, 1, c] for c in cards[7:]], 'bottom': [], 'mulligans': 0,
            'plays': []}


def game(t, result, phase_spec='sp'):
    a = seat(DECK_A, ['Icecave Crasher'] + ['Plains'] * 6 + ['Galactic Wayfarer'], 'best', 'WG')
    b = seat(DECK_B, ['Island'] * 7, 'explore', 'U')
    return {'v': 2, 'spec': phase_spec, 'seed': 1, 'deck_seed': 1, 'eval': None, 'seats': [a, b], 'searched': [True, True],
            'result': result, 'aborted': False, 'turns': 9, 'time': t, 'actions': 'AAAA', 'n_actions': 2}


def append(path, objs, partial=''):
    with open(path, 'a', encoding='utf-8', newline='') as f:
        f.write(''.join(json.dumps(x) + '\n' for x in objs) + partial)


LATE = json.dumps(game(160.0, [1.0, 0.0]))


def make_run(tmp_path):
    run = tmp_path / 'eoe-test'
    os.makedirs(run / 'games')
    with open(run / 'config.json', 'w') as f:
        json.dump({'preset': 'eoe-1', 'game': 'mtg_eoe', 'games': 600, 'sims': 64, 'arch': 'tokens', 'hidden': 128,
                   'blocks': 4, 'spec': json.dumps({'sealed': {'set': 'eoe', 'collation': 'arena', 'best_share': 0.5},
                                                    'max_turns': 60})}, f)
    pool = P.zelo.active_pool(str(run))[1]                # the run's Elo pool (its evaluation rule's)
    os.makedirs(pool, exist_ok=True)
    with open(os.path.join(pool, 'ratings.json'), 'w') as f:
        json.dump({'anchor': 'heuristic', 'ratings': {'heuristic': {'elo': 1000.0}, 'iter_0001': {'elo': 700.0, 'se': 50.0}}}, f)
    log = str(run / 'games' / 'games-7.jsonl')
    append(log, [{'v': 2, 'spec': 'sp', 'sealed': {'set': 'eoe'}, 'decks': 'random'},
                 {'v': 2, 'spec': 'ev', 'sealed': {'set': 'eoe'}, 'decks': 'eval'},
                 game(10.0, [1.0, 0.0]), game(20.0, [0.0, 1.0]), game(30.0, [1.0, 0.0], 'ev'), game(150.0, [0.5, 0.5])],
           partial=LATE[:40])                         # a line being written
    metrics(run, [{'iter': 1, 'time': 100.0, 'selfplay': {'games': 2, 'rounds': 9.0},
                   'train': {'policy': 0.9, 'value': 0.5, 'aux_deck': 3.6, 'aux_belief': 3.0, 'lr': 0.001},
                   'eval_yard': {'win': 0.2, 'se': 0.04, 'games': 100}, 'elo': {'elo': 690.0, 'se': 40.0}}])
    return str(run), log


def metrics(run, rows):
    with open(os.path.join(run, 'metrics.jsonl'), 'a', encoding='utf-8') as f:
        f.write(''.join(json.dumps(r) + '\n' for r in rows))


def args(**kw):
    a = dict(no_credit=True, no_17lands=False, credit_every=4, credit_window=4, credit_games=None, threads=1, title=None,
             url='http://sealedai.test', dry_run=False)
    a.update(kw)
    return types.SimpleNamespace(**a)


def test_the_log_is_read_once_and_unfinished_iterations_wait(tmp_path):
    run, log = make_run(tmp_path)
    t = P.CardTallies(run)
    assert t.update() == {1}
    one = t.done[1]
    assert (one.games, one.seats, one.hand_games) == (2, 4, 2)          # the evaluation game is not self-play
    assert len(t.pending) == 1 and 'actions' not in t.pending[0]       # iteration 2's game waits for its metrics line
    assert t.update() == set()                                          # nothing new: nothing read twice
    assert one.games == 2
    # the line being written is completed, then iteration 2's metrics line arrives
    append(log, [], partial=LATE[40:] + '\n')
    metrics(run, [{'iter': 2, 'time': 200.0}])
    assert t.update() == {2}
    assert t.done[2].games == 2 and t.pending == []
    assert t.done[2].best == [2, 3]                                     # the best guess: a draw and a win


def test_uploads_send_only_what_changed_and_no_paths(tmp_path, monkeypatch):
    run, log = make_run(tmp_path)
    sent = []
    monkeypatch.setattr(P, 'post', lambda url, path, body, key: sent.append((path, json.loads(json.dumps(body)))) or {'ok': True})
    pub = P.Publisher(run, args(), 'k', log=lambda m: None)
    pub.cards_pass()
    (path, body), = sent
    assert path == '/api/cards/upload' and body['run'] == 'eoe-test' and [b['iter'] for b in body['iters']] == [1]
    info = body['info']
    assert info['fields'] == list(CS.FIELDS) and info['iterations'] == [1, 1] and info['hand_from'] == 1
    names = [c['name'] for c in info['cards']]
    assert names[:len(CS.card_info())] == [c['name'] for c in CS.card_info()] and names[-1] == 'Not A Set Card'
    rows = {names[r[0]]: r[1:] for r in body['iters'][0]['rows']}
    # Icecave Crasher: in A's opening hand in both games (won one): gp, h, oh, gih 2 games, 2 doubled wins; not drawn later
    assert rows['Icecave Crasher'] == [2, 2, 2, 2, 2, 2, 0, 0, 2, 2]
    assert rows['Galactic Wayfarer'] == [2, 2, 2, 2, 0, 0, 2, 2, 2, 2]
    assert rows['Bombard'] == [2, 2, 2, 2, 0, 0, 0, 0, 0, 0]
    y = info['seventeen']
    assert y['metric'] == 'GIH WR' and 'CC BY 4.0' in y['license'] and len(y['cards']['Icecave Crasher']) == 2
    assert info['credit'] is None
    # nothing new: nothing sent; a new iteration: only it
    pub.cards_pass()
    assert len(sent) == 1
    metrics(run, [{'iter': 2, 'time': 200.0}])
    pub.cards_pass()
    assert [b['iter'] for b in sent[-1][1]['iters']] == [2]
    # the progress record
    p = pub.progress_pass()
    assert sent[-1][0] == '/api/progress/upload'
    r1 = [r for r in p['iters'] if r['iter'] == 1][0]
    assert r1['aux_deck'] == 3.6 and r1['elo_fit'] == 700.0 and r1['best_games'] == 2 and r1['best_w2'] == 2
    assert r1['yard_win'] == 0.2 and r1['policy_loss'] == 0.9
    assert p['sealed']['best_share'] == 0.5 and p['settings']['preset'] == 'eoe-1'
    for _, b in sent:
        bad = [s for s in strings(b) if PATH_RE.search(s)]
        assert not bad, bad


def test_the_draw_credit_is_due_at_multiples(tmp_path, monkeypatch):
    run, log = make_run(tmp_path)
    calls = []
    monkeypatch.setattr(P, 'post', lambda *a: {'ok': True})
    monkeypatch.setattr(P.DP, 'latest_checkpoint', lambda run, every: (CK[0], f'iter_{CK[0]:04d}.pt'))
    monkeypatch.setattr(P, 'credit_record', lambda run, it, path, window, *a: calls.append((it, tuple(window))) or {
        'checkpoint_iter': it, 'iters': list(window), 'cards': {}})
    CK = [5]
    pub = P.Publisher(run, args(no_credit=False, credit_every=4), 'k', log=lambda m: None)
    pub.cards_pass()
    assert calls == [(5, (1, 1))] and pub.credit['checkpoint_iter'] == 5      # the first checkpoint seen
    CK[0] = 7
    pub.cards_pass()
    assert len(calls) == 1                                                       # 7: no multiple of 4 crossed
    CK[0] = 9
    pub.cards_pass()
    assert calls[-1][0] == 9                                                     # crossed 8 (9 itself is newest)
    # a broken record (cardcredit's SystemExit) is said loudly; the last credit stays, the cards still go out
    logged = []
    pub.log = logged.append

    def broken(*a):
        raise SystemExit('1 games did not replay to their logged result')
    monkeypatch.setattr(P, 'credit_record', broken)
    CK[0] = 12
    pub.cards_pass()
    assert pub.credit['checkpoint_iter'] == 9 and any('DRAW CREDIT FAILED' in m for m in logged)


def test_dry_run_command_line(tmp_path, capsys):
    run, _ = make_run(tmp_path)
    P.main([run, '--dry-run', '--no-credit'])
    out = capsys.readouterr().out
    assert '(dry run) progress' in out and '(dry run) cards: 1 iterations' in out


def strings(x, out=None):
    out = [] if out is None else out
    if isinstance(x, str):
        out.append(x)
    elif isinstance(x, dict):
        for k, v in x.items():
            strings(k, out)
            strings(v, out)
    elif isinstance(x, list):
        for v in x:
            strings(v, out)
    return out
