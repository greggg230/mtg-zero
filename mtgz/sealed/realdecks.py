"""Win % against real players' decks: a yardstick of our deckbuilding (docs/ENGINE.md 113).

**A yardstick only (DESIGN.md 0): measured and logged, never a training signal.** Nothing here feeds self-play, the deck
choice, the deck evaluator's training, model selection or best.pt (the user, 2026-11-11: "it's useful for evaluation ...
It should not be a target for deck building").

**The field** (`Universe`, `RealDecks`): the decks 17lands players built in Edge of Eternities Premier Sealed (best of one;
data/sealed/eoe_17lands_sealed.json.gz, 17lands public data, CC BY 4.0). A pool counts when the engine supports every card
of it and it is complete (the 78 cards of six Arena boosters, every build's row holding all of them); a pool that is not is
skipped whole and counted with the reason (the unsupported cards named) - never a dropped card. Its deck is the player's
main 40-card build: the 40-card build with the most games (then the lowest build index); a pool without a 40-card build
is skipped and counted. The counted pools in a fixed order (sorted by 17lands id, then by mix(seed, salt, position)): the
first `pools` are the **test pools**, every later one gives the field its deck. Game k plays field deck k: every
evaluation of one setting meets the same opponents in the same games.

**Ours, paired** (docs/ENGINE.md 104): per test pool i, A_i = our deck of the pool (`builder`: the builder's best guess,
the network's value pick, a deck evaluator's pick, or any callable - `builder_of`) and B_i = the player's main build of
the same pool. Pool i plays games k = i * games .. (i + 1) * games - 1; game k has one seed, one field opponent, one seat
for the deck under test (k % 2) and one chooser of play or draw ((k // 2) % 2), the same for A_i and B_i; A_i and B_i
are laid out slot by slot (`compare.slots`: their shared cards in the same library slots, "order": "given"), and each is
played in its own driver with the same driver seed and queue, so the pilot's streams agree too. The pilot is one network
checkpoint on both seats (the deck under test and the field), with its run's evaluation seat and its run's evaluation
spec's sealed prior; `heuristic` (option C) plays the same games with the heuristic bot on both seats.

**Reported**: A (our deck's score against the field), B (the human deck's), D = A - B (our deckbuilding against the
player's, against real decks), each with its standard error over the games (`se`: this fixed set of pools and opponents,
what moves between evaluations) and over the pools (`se_pools`: the pools as a sample of 17lands pools; the games of a pool
averaged first); the paired statistics of `compare.paired_stats` (var_ratio, same / better / worse); games an engine error
stopped (removed for both decks, their specs kept) and unfinished games.

    python -m mtgz.sealed.realdecks CKPT [CKPT ...] [--pools 64] [--games 2] [--builder best] [--device cuda] [--out DIR]
    python -m mtgz.sealed.realdecks field [--pools 64]

In a run (`mtgz.az`'s package: the extra evaluation `real_decks`, off by default): run settings `real_decks_every` (0 =
off; else the evaluated iterations that are multiples of it), `real_decks_pools`, `real_decks_games`, `real_decks_builder`;
logged under `extras.real_decks` in metrics.jsonl (or evals.jsonl when the evaluation is remote).
"""
import argparse
import collections
import functools
import json
import math
import os
import sys
import time

from . import cards as C
from . import compare as K
from . import engine as R

RULE = ('a yardstick only (DESIGN.md 0): measured and logged, never a training signal, deck choice, evaluator target, '
        'model selection or the best checkpoint')
LICENSE = '17lands public data, CC BY 4.0 (https://www.17lands.com/public_datasets)'
SOURCE = '17lands EOE Premier Sealed (best of one), game_data_public.EOE.Sealed.csv.gz'
ORDER_SALT = 0x2EA1_DEC5_0000_0001
GAME_SALT = 0x2EA1_DEC5_0000_0002
DRIVER_SALT = 0x2EA1_DEC5_0000_0003
CAND_SALT = 0x2EA1_DEC5_0000_0004
DEFAULTS = {'pools': 64, 'games': 2, 'seed': 0, 'builder': 'best', 'k': 32, 'worlds': 8}
LABEL = 'our deck vs real players\' decks (17lands)'


# ----------------------------------------------------------------------------- the field and the test pools

def main_build(pool):
    """The player's main 40-card build of a pool: the 40-card build with the most games, then the lowest index (None
    without one)."""
    b40 = [b for b in pool.builds if b.deck_size == 40]
    return min(b40, key=lambda b: (-len(b.games), b.index)) if b40 else None


def deck_of(pool, build):
    """A 17lands build as a comparison deck: {'deck', 'sideboard', 'colors', 'pool', 'build', 'record'}."""
    from . import yardstick as Y
    return {'deck': Y.deck(build), 'sideboard': Y.sideboard(pool, build),
            'colors': build.main_colors + (('+' + build.splash_colors) if build.splash_colors else ''),
            'pool': pool.id, 'build': build.index, 'record': [build.wins, build.losses]}


@functools.lru_cache(4)
def universe(event='Sealed'):
    """The 17lands pools that count (module docs), sorted by id, and what was skipped: {'pools': [Pool], 'counts': {...}}.
    Reasons in order: a card the engine does not support (the pool's cards named), an incomplete pool, no 40-card build."""
    from . import seventeen_lands as L
    from . import yardstick as Y
    pools = L.load(event)
    table = Y.lookup(pools)
    ok, unsupported, cards, in_deck, incomplete, no40, sizes = [], 0, collections.Counter(), 0, 0, 0, collections.Counter()
    for p in sorted(pools, key=lambda p: p.id):
        u = Y.unsupported(p, table)
        if u:
            unsupported += 1
            cards.update(u)
            b = main_build(p)
            in_deck += b is not None and any(n in b.deck for n in u)
            continue
        if p.incomplete:
            incomplete += 1
            continue
        if main_build(p) is None:
            no40 += 1
            sizes.update(b.deck_size for b in p.builds)
            continue
        ok.append(p)
    counts = {'event': event, 'pools': len(pools), 'builds': sum(len(p.builds) for p in pools), 'counted': len(ok),
              'skipped': {'unsupported': {'pools': unsupported, 'cards': dict(cards.most_common()),
                                          'main_deck_holds_one': in_deck},
                          'incomplete': incomplete,
                          'no_40_card_build': {'pools': no40, 'build_sizes': dict(sorted(sizes.items()))}}}
    return {'pools': ok, 'counts': counts}


class RealDecks:
    """The test pools and the field of one setting (module docs): `pools` test pools, `games` games each (even: both
    seats), `seed` the order; `event` 17lands' 'Sealed' (Premier, best of one)."""

    def __init__(self, pools=DEFAULTS['pools'], games=DEFAULTS['games'], seed=DEFAULTS['seed'], event='Sealed'):
        if pools < 1 or games < 1:
            raise ValueError(f'real decks: pools {pools}, games {games}: at least one of each')
        u = universe(event)
        order = sorted(range(len(u['pools'])), key=lambda i: (K.mix(seed, ORDER_SALT, i), i))
        ps = [u['pools'][i] for i in order]
        if pools * games > len(ps) - pools:
            raise ValueError(f'real decks: {pools} pools x {games} games need {pools * games} field decks, the field has '
                             f'{len(ps) - pools}')
        self.pools, self.games, self.seed, self.event = int(pools), int(games), int(seed), event
        self.test = ps[:pools]
        self.field_pools = ps[pools:]
        self.counts = u['counts']

    def human(self, i):
        p = self.test[i]
        return deck_of(p, main_build(p))

    def opponent(self, k):
        p = self.field_pools[k]
        return deck_of(p, main_build(p))

    def describe(self):
        return {'source': SOURCE, 'license': LICENSE, 'rule': RULE, 'event': self.event, 'seed': self.seed,
                'test_pools': self.pools, 'games_per_pool': self.games, 'field_decks_used': self.pools * self.games,
                'field_decks': len(self.field_pools), 'universe': self.counts,
                'deck': 'the player\'s main 40-card build (the most games, then the lowest build index)'}


# ----------------------------------------------------------------------------- our builder

def _cand(b, name, how):
    return {'name': name, 'deck': list(b['deck']), 'sideboard': list(b['sideboard']),
            'colors': b['colors'] + (('+' + b['splash']) if b.get('splash') else ''), 'lands': b.get('lands'), 'how': how}


def candidates(pool_names, seed, k=32, collation='arena'):
    """The deck choice's candidates of a pool (docs/ENGINE.md 107.2): candidate 0 the builder's best guess, 1..k-1 the
    randomised builder from generators of their own (seeded by `seed`)."""
    spec = {'sealed': {'set': 'eoe', 'collation': collation, 'p0': {'pool': list(pool_names)},
                       'deck_choice': {'k': int(k), 'explore': 0.0}}, 'max_turns': 60, 'seed': int(seed)}
    return R.candidates(spec, seed)[0]['candidates']


def builder_of(spec, pilot=None, k=DEFAULTS['k'], worlds=DEFAULTS['worlds'], collation='arena'):
    """Our builder: fn(pool names, seed) -> our deck ({'deck', 'sideboard', 'colors', 'how', ...}).

    * 'best': the builder's best guess (docs/ENGINE.md 107.1; a function of the pool);
    * 'value': the pilot network's value pick among the deck choice's k candidates (run 2's deck choice, 107.2;
      `compare.value_pick`, `worlds` per candidate);
    * 'evaluator:PATH': a saved deck evaluator's top pick among the k candidates (S4a, docs/ENGINE.md 108:
      `evaluator.rank`; the first best on a tie);
    * 'module:attr' (any other): a callable fn(pool names, seed) -> {'deck', 'sideboard', ...} - another builder plugged
      in (e.g. a run's evaluator choice in self-play)."""
    if spec == 'best':
        return lambda names, seed: _cand(candidates(names, seed, 2, collation)[0], 'ours', 'best guess')
    if spec == 'value':
        if pilot is None or not pilot.is_net:
            raise ValueError("builder 'value' needs a network pilot")

        def value(names, seed):
            j, cands = K.value_pick(names, pilot, k, worlds, seed, collation)
            return _cand(cands[j], 'ours', f'value pick {j} of {k}')
        return value
    if spec.startswith('evaluator:'):
        from . import evaluator as EV
        e = EV.Evaluator.load(spec[len('evaluator:'):])

        def pick(names, seed):
            cands = candidates(names, seed, k, collation)
            s = [float(x) for x in EV.rank(names, cands, e)]
            j = max(range(len(cands)), key=lambda i: (s[i], -i))
            return _cand(cands[j], 'ours', f'{e.kind} evaluator pick {j} of {k}')
        return pick
    if ':' in spec:
        import importlib
        mod, _, attr = spec.partition(':')
        fn = getattr(importlib.import_module(mod), attr)
        return lambda names, seed: dict(_cand(fn(names, seed), 'ours', spec))
    raise ValueError(f"builder {spec!r}: 'best', 'value', 'evaluator:PATH' or 'module:attr'")


def check_build(pool, deck):
    """Our deck is a 40-card build of the pool (every nonbasic card within the pool's count): loudly otherwise."""
    have = collections.Counter(pool.cards)
    need = collections.Counter(n for n in deck if n not in C.BASICS)
    over = {n: c for n, c in need.items() if c > have.get(n, 0)}
    if len(deck) != 40 or over:
        raise ValueError(f'pool {pool.id}: our deck is not a 40-card build of it ({len(deck)} cards, beyond the pool '
                         f'{over})')


# ----------------------------------------------------------------------------- one game's spec

def base_spec(spec):
    """The spec keys a real-deck game keeps from a run's evaluation spec (JSON or dict): its sealed prior (collation,
    boosters, builder settings, best_share: how the network's search deals the opponent's hidden deck) and the game's
    top-level settings (max_turns, ...); never its decks (seats, deck choice, the evaluation set)."""
    s = json.loads(spec) if isinstance(spec, str) else dict(spec or {})
    sealed = {k: v for k, v in (s.get('sealed') or {}).items() if k not in ('p0', 'p1', 'deck_choice')}
    sealed['set'] = 'eoe'
    top = {k: v for k, v in s.items() if k not in ('sealed', 'decks', 'eval_seed', 'eval_decks', 'seed', 'chooser')}
    top.setdefault('max_turns', 60)
    return {'sealed': sealed, **top}


def game_spec(base, deck, opp, seed_k, hero, chooser):
    """One game: `deck` (laid out, "order": "given") in seat `hero`, the field's `opp` in the other."""
    sealed = dict(base['sealed'])
    sealed[f'p{hero}'] = {'deck': list(deck['deck']), 'sideboard': list(deck['sideboard']), 'order': 'given'}
    sealed[f'p{1 - hero}'] = {'deck': list(opp['deck']), 'sideboard': list(opp['sideboard'])}
    return dict(base, sealed=sealed, seed=int(seed_k), chooser=int(chooser))


# ----------------------------------------------------------------------------- playing

class Player:
    """Who plays both seats: a seat dict (zero.games) and its evaluators ({key: evaluator}; none for a bot), the driver's
    settings (threads, groups, max_steps, sparse, concurrency cap) and a `check` called while driving (a run's PAUSE)."""

    def __init__(self, seat, evaluators, threads=4, groups=2, max_steps=20_000, sparse=False, conc=None, check=None,
                 describe=None):
        self.seat, self.evaluators = seat, dict(evaluators)
        self.threads, self.groups, self.max_steps, self.sparse = int(threads), int(groups), int(max_steps), bool(sparse)
        self.conc, self.check, self.description = conc, check, describe

    def play(self, specs, driver_seed, tags=None):
        """Every spec as one game, in order (game id = its place), in one driver -> {tag: row} (`tags`: integers, default
        the places). An engine error in a game raises RuntimeError naming its tag ('queued setup "<tag>"')."""
        G = K._games()
        from zero.infer import drive
        js = [json.dumps(s, separators=(',', ':')) for s in specs]
        kw = {'sparse': True} if self.sparse else {}
        conc = max(1, min(len(js), int(self.conc or len(js))))
        drv = G.Driver('mtg_eoe', [[self.seat, self.seat]], spec=js[0], concurrency=conc, seed=int(driver_seed),
                       threads=self.threads, groups=self.groups if self.evaluators else 1, record=False,
                       max_steps=self.max_steps, rolling=True, queue_only=True, **kw)
        drv.resume(K.BIG, 0)
        for t, s in zip(range(len(js)) if tags is None else tags, js):
            drv.enqueue(str(int(t)), s)
        rows = {}
        while True:
            drive(drv, self.evaluators, check=self.check)
            for r in drv.take_results():
                rows[int(r['tag'])] = r
            if not drv.queued and not drv.in_flight()['games']:
                break
            time.sleep(0.01)
        drv.end_phase()
        del drv
        return rows


def _stats(xs):
    m, se, _ = K.mean_se(xs)
    return round(m, 4), round(se, 4)


def summary(games):
    """[(pool, k, a, b)] -> A, B and D = A - B with their standard errors over the games and over the pools."""
    a = [g[2] for g in games]
    b = [g[3] for g in games]
    ps = K.paired_stats(a, b)
    by = collections.defaultdict(list)
    for p, _, x, y in games:
        by[p].append((x, y))
    pa = [sum(x for x, _ in v) / len(v) for v in by.values()]
    pb = [sum(y for _, y in v) / len(v) for v in by.values()]
    pd = [x - y for x, y in zip(pa, pb)]
    d_m, d_se = _stats(pd)
    return {'n': ps['n'], 'pools': len(by),
            'a': ps['a'], 'a_se': ps['a_se'], 'a_se_pools': _stats(pa)[1],
            'b': ps['b'], 'b_se': ps['b_se'], 'b_se_pools': _stats(pb)[1],
            'diff': ps['diff'], 'diff_se': ps['se'], 'diff_ci95': ps['ci95'], 'diff_se_pools': d_se,
            'diff_ci95_pools': [round(d_m - K.Z95 * d_se, 4), round(d_m + K.Z95 * d_se, 4)],
            'var_ratio': ps['var_ratio'], 'same': ps['same'], 'a_better': ps['a_better'], 'b_better': ps['b_better']}


def measure(rd, builder, player, base, log=None, builder_name=None, detail=True):
    """One evaluation (module docs): our decks (`builder`) and the human decks of `rd`'s test pools against its field,
    `player` on both seats, the games' spec keys from `base` (`base_spec`). -> the result dict."""
    t0 = time.time()
    ours, human, laid = [], [], []
    for i, pool in enumerate(rd.test):
        from . import yardstick as Y
        o = builder(Y._expand(pool.cards), K.mix(rd.seed, CAND_SALT, i))
        check_build(pool, o['deck'])
        h = rd.human(i)
        la, lb = K.slots([o['deck'], h['deck']])
        ours.append(o)
        human.append(h)
        laid.append((dict(o, deck=la), dict(h, deck=lb)))
    t_build = time.time() - t0
    ks = list(range(rd.pools * rd.games))
    failed = []
    dseed = K.mix(rd.seed, DRIVER_SALT)

    def specs(side):
        return [game_spec(base, laid[k // rd.games][side], rd.opponent(k), K.mix(rd.seed, GAME_SALT, k), k % 2,
                          K.chooser_of(k)) for k in ks]
    while True:
        got = []
        try:
            for side in (0, 1):
                got.append(player.play(specs(side), dseed, ks))
        except RuntimeError as e:
            import re
            m = re.search(r'queued setup "(\d+)"', str(e))
            if not m:
                raise
            k = int(m.group(1))
            g = ks.index(k)
            failed.append({'k': k, 'pool': rd.test[k // rd.games].id, 'deck': 'ours' if len(got) == 0 else 'human',
                           'error': str(e)[:2000], 'spec': specs(len(got))[g], 'driver_seed': dseed, 'game_id': g})
            if log:
                log(f'# real decks: game {k} failed (left out for both decks): {str(e)[:300]}')
            ks = [x for x in ks if x != k]
            continue
        break
    games, unfinished, turns, decisions = [], [0, 0], 0, 0
    for k in ks:
        ra, rb = got[0][k], got[1][k]
        hero = k % 2                    # game k's deck under test sits in seat k % 2 (a removed game shifts no seat)
        for s, r in enumerate((ra, rb)):
            unfinished[s] += (not r['over']) or bool(r.get('aborted'))
            turns += r.get('rounds', 0)
            decisions += r.get('decisions', 0)
        games.append((k // rd.games, k, float(ra['result'][hero]), float(rb['result'][hero])))
    out = summary(games)
    out.update(label=LABEL, value=out['a'], pct=True, refs={'the human deck': out['b']},
               builder=builder_name, games_per_pool=rd.games, field_decks=len(ks), failed=len(failed),
               unfinished_ours=unfinished[0], unfinished_human=unfinished[1], turns=round(turns / max(1, 2 * len(ks)), 2),
               searched=decisions, build_s=round(t_build, 1),
               seconds=round(time.time() - t0, 1), source='17lands EOE Premier Sealed', license=LICENSE)
    if detail:
        per = []
        for i in range(rd.pools):
            mine = [g for g in games if g[0] == i]
            per.append({'pool': rd.test[i].id, 'human': {k: human[i][k] for k in ('colors', 'build', 'record')},
                        'ours': {k: ours[i].get(k) for k in ('colors', 'lands', 'how')},
                        'shared_slots': K.shared_slots(*[x['deck'] for x in laid[i]]),
                        'games': len(mine), 'a_points': sum(g[2] for g in mine),
                        'b_points': sum(g[3] for g in mine)})
        out['per_pool'] = per
        out['failed_games'] = failed
    return out


# ----------------------------------------------------------------------------- a run's extra evaluation

def run_options(cfg):
    """The real-deck settings of a run's config -> (pools, games, builder)."""
    pools = int(cfg.get('real_decks_pools', DEFAULTS['pools']))
    games = int(cfg.get('real_decks_games', DEFAULTS['games']))
    return pools, games, str(cfg.get('real_decks_builder', DEFAULTS['builder']) or 'best')


class _RunPilot:
    """A run's evaluation seat as a `compare.Pilot` for the value pick (`builder_of('value')`)."""
    is_net = True

    def __init__(self, seat, evaluators):
        self._seat, self._ev = seat, evaluators

    def seat(self):
        return dict(self._seat)

    def evaluators(self):
        return self._ev


def run_eval(run, it):
    """`GamePackage.evals` entry of `mtg_eoe` (zero-core runs it on the evaluated iterations that are multiples of
    `real_decks_every`, never at 0: GamePackage.evals_every): checkpoint `it` against real players' decks with the
    run's evaluation seat at `eval_sims` on both seats, the run's evaluation spec's sealed prior, the run's driver
    settings. Compact: no per-pool detail (the command line has it). A yardstick only (module docs)."""
    c = run.cfg
    if c['game'] != 'mtg_eoe':
        return None
    from zero import package
    from zero.run import model_path
    pools, games, bname = run_options(c)
    seat = run.nn_seat('m', c['eval_sims'])
    evs = {'m': run.evaluator(model_path(run.run, it))}
    player = Player(seat, evs, threads=c['threads'], groups=c['groups'], max_steps=c['max_steps'],
                    sparse=bool(c.get('sparse')), conc=c['conc'], check=lambda: run.check('real decks'))
    builder = builder_of(bname, _RunPilot(seat, evs))
    base = base_spec(c.get('eval_spec') or package.active().eval_spec_of(c['game'], c['spec']))
    out = measure(RealDecks(pools, games), builder, player, base, log=run.log, builder_name=bname, detail=False)
    out['sims'] = c['eval_sims']
    return out


EVALS = {'real_decks': run_eval}


# ----------------------------------------------------------------------------- the command line

def run_config(ckpt):
    """The checkpoint's run settings (runs/<run>/config.json next to models/, over zero.run's DEFAULTS and the run's
    preset, as resolve_config builds them; DEFAULTS and the EoE evaluation spec without one) -> (cfg, found)."""
    import mtgz.az  # noqa: F401
    from zero.run import DEFAULTS as D, PRESETS
    p = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(ckpt))), 'config.json')
    saved = {}
    if os.path.exists(p):
        with open(p, encoding='utf-8') as f:
            saved = json.load(f)
    cfg = dict(D)
    cfg.update(PRESETS.get(saved.get('preset') or 'eoe-1', {}))
    cfg.update(saved)
    return cfg, bool(saved)


def net_player(ckpt, cfg, sims, device, threads, groups):
    """The run's evaluation seat (zero.run.Run.nn_seat of its settings) of checkpoint `ckpt` at `sims`, its network on
    `device`."""
    from types import SimpleNamespace

    from zero.infer import NetEvaluator
    from zero.net import load
    from zero.run import Run
    seat = Run.nn_seat(SimpleNamespace(cfg=cfg), 'net', sims)
    m, _ = load(ckpt, device, game='mtg_eoe')
    ev = NetEvaluator(m, device)
    return Player(seat, {'net': ev}, threads=threads, groups=groups, max_steps=cfg['max_steps'],
                  sparse=bool(cfg.get('sparse')) and device != 'cpu', conc=cfg.get('conc'),
                  describe={'checkpoint': os.path.abspath(ckpt), 'sims': sims, 'device': device,
                            'seat': 'the run\'s evaluation seat (zero.run.Run.nn_seat)'})


def heuristic_player(threads, max_steps=20_000):
    G = K._games()
    return Player(G.seat_dict('heuristic'), {}, threads=threads, groups=1, max_steps=max_steps, describe='heuristic')


def main(argv=None):
    ap = argparse.ArgumentParser(description='win % against real players\' decks (docs/ENGINE.md 113; a yardstick only)')
    ap.add_argument('checkpoints', nargs='+', help="checkpoints (runs/<run>/models/iter_NNNN.pt), or 'field' alone: the "
                                                   'field and the test pools of the setting')
    ap.add_argument('--pools', type=int, default=DEFAULTS['pools'], help='test pools')
    ap.add_argument('--games', type=int, default=DEFAULTS['games'], help='games per test pool and deck (even: both seats)')
    ap.add_argument('--seed', type=int, default=DEFAULTS['seed'], help='the order of the pools (the test pools and field)')
    ap.add_argument('--builder', default=None, help="our builder: 'best' (the default; the builder's best guess), "
                                                    "'value', 'evaluator:PATH', 'module:attr'; default: the run's "
                                                    'real_decks_builder setting, else best')
    ap.add_argument('--k', type=int, default=DEFAULTS['k'], help='candidates of the value / evaluator pick')
    ap.add_argument('--worlds', type=int, default=DEFAULTS['worlds'], help='the value pick\'s worlds per candidate')
    ap.add_argument('--sims', type=int, default=None, help="simulations (default: the run's eval_sims)")
    ap.add_argument('--device', default='cpu')
    ap.add_argument('--threads', type=int, default=4, help='driver search threads')
    ap.add_argument('--groups', type=int, default=2, help='driver groups (network batches in flight)')
    ap.add_argument('--torch-threads', type=int, default=None, help='torch threads (CPU inference)')
    ap.add_argument('--heuristic', action='store_true', help='also C: the same games with the heuristic on both seats')
    ap.add_argument('--out', default=None, help='a directory: one JSON file per checkpoint (<run>-iter_NNNN.json)')
    ap.add_argument('--json', default=None, help='one checkpoint: its JSON file')
    ap.add_argument('--event', default='Sealed', choices=['Sealed'])
    a = ap.parse_args(argv)

    def log(s):
        print(s, file=sys.stderr, flush=True)
    if a.torch_threads:
        import torch
        torch.set_num_threads(a.torch_threads)
    rd = RealDecks(a.pools, a.games, a.seed, a.event)
    if a.checkpoints == ['field']:
        d = rd.describe()
        d['test_pools_ids'] = [p.id for p in rd.test]
        print(json.dumps(d, indent=1))
        return 0
    outs = []
    for ck in a.checkpoints:
        cfg, found = run_config(ck)
        if cfg['game'] != 'mtg_eoe':
            raise SystemExit(f'{ck}: game {cfg["game"]!r}, not mtg_eoe')
        sims = a.sims or cfg['eval_sims']
        player = net_player(ck, cfg, sims, a.device, a.threads, a.groups)
        bname = a.builder or str(cfg.get('real_decks_builder') or 'best')
        builder = builder_of(bname, _RunPilot(player.seat, player.evaluators), a.k, a.worlds)
        base = base_spec(cfg.get('eval_spec') or cfg.get('spec') or '{}')
        log(f'# {ck}: {rd.pools} pools x {a.games} games, builder {bname}, sims {sims}, {a.device}')
        out = measure(rd, builder, player, base, log=log, builder_name=bname)
        out.update(checkpoint=os.path.abspath(ck), iter=_iter_of(ck), sims=sims, pilot=player.description,
                   run_config_found=found, spec=base, setting=rd.describe())
        if a.heuristic:
            out['heuristic'] = measure(rd, builder, heuristic_player(a.threads, cfg['max_steps']), base, log=log,
                                       builder_name=bname, detail=False)
        log('# ' + json.dumps({k: out[k] for k in ('iter', 'a', 'a_se', 'b', 'b_se', 'diff', 'diff_se', 'diff_se_pools',
                                                     'n', 'failed', 'seconds')}))
        outs.append(out)
        path = a.json if a.json and len(a.checkpoints) == 1 else None
        if a.out:
            os.makedirs(a.out, exist_ok=True)
            run = os.path.basename(os.path.dirname(os.path.dirname(os.path.abspath(ck))))
            path = os.path.join(a.out, f'{run}-{os.path.splitext(os.path.basename(ck))[0]}.json')
        if path:
            tmp = path + '.tmp'
            with open(tmp, 'w', encoding='utf-8') as f:
                json.dump(out, f, indent=1)
            os.replace(tmp, path)
    print(json.dumps(outs[0] if len(outs) == 1 else outs), flush=True)
    return 0


def _iter_of(path):
    import re
    m = re.search(r'iter_(\d+)', os.path.basename(path))
    return int(m.group(1)) if m else None


if __name__ == '__main__':
    sys.exit(main())
