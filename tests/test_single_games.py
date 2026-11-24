"""Single games (docs/ENGINE.md 256; zero-core docs/az.md 82; zero-farm PROTOCOL 22): how many single games one game
id of an `mtg` spec is - the package hook `id_games` (mtgz.az.single_games: the format asked of the engine), a
best-of-three match counted from the episodes of its record, a match without them not counted - and the campaign
pull's ack carrying the exact counts. No farm, no worker, no network, no GPU."""
import gzip
import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az as A  # noqa: E402
from mtgz import _engine as E  # noqa: E402
from mtgz.az import farm_campaign as C, single_games as SG  # noqa: E402
from zero import farm_def as FD, farm_eval as FE, games as G  # noqa: E402
from zero.infer import drive  # noqa: E402

from test_farm_play import SEED, Fake, Uniform, _campaign, traced  # noqa: E402,F401  (traced: a fixture)

BO1 = A.SPEC
BO1_SPELLED = json.dumps(dict(json.loads(A.SPEC), format='bo1'))
BO3_WHOLE = A.SPEC_BO3                                   # one episode = the whole match
BO3_GAMES = A.SPEC_DELVER_BO3                            # game_episodes: one episode a game
BO3_CONSTRUCTED_WHOLE = json.dumps({k: v for k, v in json.loads(A.SPEC_DELVER_BO3).items()
                                    if k not in ('game_episodes', 'start_game')})


def acks(fake):
    return [json.loads(b) for m, p, b in fake.calls if m == 'POST' and p.endswith('/ack')]


def test_every_mtg_game_declares_the_hook_and_the_draft_does_not():
    pkg = A.PACKAGE
    assert set(pkg.id_games) == {'mtg', 'mtg_v7', 'mtg_eoe'} == set(pkg.specs) - {'team_draft'}
    # (every game the engine module registers is one of the package's, or zero-core's toy)
    with open(os.path.join(ROOT, 'py', 'src', 'lib.rs'), encoding='utf-8') as f:
        src = f.read()
    import re
    assert set(re.findall(r'GameEntry::of::<[^>]+>\("([a-z0-9_]+)"\)', src)) == set(pkg.specs) | {'toy'}
    assert pkg.id_games_of('team_draft', A.SPEC_DRAFT) is None


@pytest.mark.parametrize('game', A.MTG_GAMES)
def test_the_hook_asks_the_engine_for_the_format(game):
    pkg = A.PACKAGE
    # best-of-one, the engine's default and its spelling: an id is one game
    for spec in (BO1, BO1_SPELLED):
        assert SG.spec_format(game, spec) == ('bo1', False)
        assert pkg.id_games_of(game, spec) == 1 and G.single_games(game, spec, {'gid': 0}) == 1
    # best-of-three, one episode the whole match: not known - with or without a row's fields
    for spec in (BO3_WHOLE, BO3_CONSTRUCTED_WHOLE):
        assert SG.spec_format(game, spec) == ('bo3', False)
        assert pkg.id_games_of(game, spec) is None
        assert G.single_games(game, spec, {'gid': 0, 'result': [1.0, 0.0], 'over': True}) is None
    # best-of-three with game_episodes: the hook says "not from the spec"; the row's or the record's episodes count
    assert SG.spec_format(game, BO3_GAMES) == ('bo3', True) and G.chained(game, BO3_GAMES)
    assert pkg.id_games_of(game, BO3_GAMES) is None
    eps = [{'over': True}, {'over': True}]
    assert G.single_games(game, BO3_GAMES, {'gid': 0, 'episodes': eps}) == 2
    assert G.single_games(game, BO3_GAMES, {'gid': 0}) is None
    # what the engine refuses is not answered by a reading of the text
    with pytest.raises(ValueError, match='format must be'):
        SG.id_games(game, json.dumps(dict(json.loads(A.SPEC), format='best-of-three')))


def test_the_eoe_game_is_one_game_an_id():
    assert SG.spec_format('mtg_eoe', A.SPEC_EOE) == ('bo1', False)
    assert A.PACKAGE.id_games_of('mtg_eoe', A.SPEC_EOE) == 1 and G.single_games('mtg_eoe', A.SPEC_EOE, {'gid': 3}) == 1


def test_every_presets_specs_are_counted_or_chained_or_whole_matches():
    """Each preset's self-play and evaluation spec, as the engine reads it: one game an id, or a chained match (its
    episodes are counted), or a best-of-three of one episode (not counted) - the hook raises for none (a preset whose
    spec names a file the user must fill in is not a spec yet)."""
    seen = set()
    for name, p in A.PRESETS.items():
        game = p.get('game', 'mtg')
        if game == 'team_draft':
            continue
        for key in ('spec', 'eval_spec'):
            spec = p.get(key)
            if not spec or (game, spec) in seen or '<the deck evaluator export' in spec:    # (a placeholder path)
                continue
            seen.add((game, spec))
            fmt, chained = SG.spec_format(game, spec)
            want = 1 if fmt == 'bo1' else None
            assert A.PACKAGE.id_games_of(game, spec) == want, (name, key)
            assert chained == G.chained(game, spec) and (fmt == 'bo3' or not chained), (name, key)
    assert seen


def test_a_campaign_pull_acks_the_single_games_of_best_of_one_rows(tmp_path, traced):  # noqa: F811
    spec, rows = traced
    d, st = _campaign(tmp_path, spec)
    fake = Fake('cmp-test')
    cl = FD.Client('https://farm.test', token='t', transport=fake)
    fake.add([rows[0], rows[1], rows[2], rows[3]], [[0, 2]], machine='m1')       # result 1 credits 0, 1
    fake.add([rows[2], rows[3], rows[0]], [[2, 4]], machine='m2')                # result 2 credits 2, 3
    fake.add([rows[4]], [[4, 6]], machine='m1')                                  # result 3: id 5 is not in its file
    for c, n in zip(fake.chunks, (2, 2, 2)):
        c['games'] = n
    assert C.pull(cl, d, st) == 5
    # (a result whose credited ids are not all there gets no entry: never a partial count)
    assert acks(fake) == [{'cursor': 3, 'single_games': [[1, 2], [2, 2]]}]
    assert C._single_games(st, {'games': 2}, [rows[0], rows[1]]) == 2
    assert C._single_games(st, {'games': 3}, [rows[0], rows[1]]) is None
    bad = json.loads(json.dumps(rows[0]))
    bad['digest'] = '0' * 16
    assert C._single_games(st, {'games': 2}, [bad, rows[1]]) is None
    # a coordinator's result without `games` (none does since PROTOCOL rev 5): the ack of before
    d2, st2 = _campaign(tmp_path / 'b', spec)
    fake2 = Fake('cmp-test')
    fake2.add([rows[0], rows[1]], [[0, 2]])
    C.pull(FD.Client('https://farm.test', token='t', transport=fake2), d2, st2)
    assert acks(fake2) == [{'cursor': 1}]


def test_best_of_three_matches_are_counted_from_their_records_episodes(tmp_path):
    """Real Delver best-of-three matches with `game_episodes` (start_game 2: one or two games played an id): each
    record lists its match's episodes, the count is their number - the driver's own count of the games it finished -
    and a campaign's pull of the self-play records file acks the exact total."""
    spec = BO3_GAMES
    seat = G.nn_seat('m', 2, selfplay=True, sims_fast=1, p_full=0.5)
    drv = G.Driver('mtg', [[seat, seat]], spec=spec, concurrency=4, seed=SEED, record=True, threads=2, groups=1,
                   rolling=True)
    drv.set_trace(True)
    rows, texts = [], []
    # Matches 0-3 (the campaign below credits those ids). Which of the rolling driver's matches finish first depends
    # on the games (a change of the constructed table's definitions, ENGINE.md 270.3, finished 0, 2, 3 and 5 first):
    # more are played until those four are done, and only they are kept.
    done = lambda: {int(json.loads(t)['gid']) for t in texts}  # noqa: E731
    for _ in range(50):
        if {0, 1, 2, 3} <= done():
            break
        drv.resume(max(4 - len(texts), 1), 0)
        drive(drv, {'m': Uniform()})
        r, t, _ = drv.take_game_records()
        rows += r
        texts += t
    texts = [t for t in texts if int(json.loads(t)['gid']) < 4]
    rows = [r for r in rows if int(r['gid']) < 4]
    recs = [json.loads(t) for t in texts]
    counts = {int(r['gid']): G.single_games('mtg', spec, r) for r in recs}
    assert sorted(counts) == [0, 1, 2, 3] and all(1 <= n <= 2 for n in counts.values())
    # the driver's rows say the same of every match (the count `selfplay.episodes.games` always was)
    assert {int(r['gid']): len(r['episodes']) for r in rows} == counts
    assert G.single_games_sum('mtg', spec, recs) == sum(counts.values())
    # the same matches without their episodes (what an untraced farm eval row carries): not known
    assert G.single_games('mtg', spec, {'gid': 0, 'result': recs[0]['result']}) is None
    head = {'records': 1, 'game': 'mtg', 'spec': spec, 'aux': False, 'seed': SEED}

    def body(ts):
        return gzip.compress(('\n'.join([json.dumps(head)] + ts) + '\n').encode())

    by = {int(json.loads(t)['gid']): t for t in texts}
    d, st = _campaign(tmp_path, spec, games=4, samples=True)
    fake = Fake('cmp-test')
    fake.add(None, [[0, 3]], kind='selfplay', raw=body([by[0], by[1], by[2]]), machine='m1')
    fake.add(None, [[3, 4]], kind='selfplay', raw=body([by[3]]), machine='m2')
    fake.chunks[0]['games'], fake.chunks[1]['games'] = 3, 1
    assert C.pull(FD.Client('https://farm.test', token='t', transport=fake), d, st) == 4
    assert acks(fake) == [{'cursor': 2, 'single_games': [[1, counts[0] + counts[1] + counts[2]], [2, counts[3]]]}]
    assert C.spot_check(d, st, frac=1.0, engine=E)['checked'] == 4
