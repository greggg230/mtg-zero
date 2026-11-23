"""`python -m mtgz.az.keep_h2h`: the sideboarding yardstick of a constructed best-of-three (docs/ENGINE.md 130, 141) - a
head-to-head where side B never sideboards (it keeps its 60 at every sideboarding: `PASS` at its first sideboarding
decision) and side A searches its sideboarding as it searches everything else.

  python -m mtgz.az.keep_h2h runs/<run>/models/iter_NNNN.pt [B.pt] --games 400 --sims 64 --device cuda
      [--arms informed|uninformed|both] [--spec SPEC_JSON] [--seed 0 --threads 4 --groups 2 --conc N --max-steps 60000]

B defaults to A: one network, sideboarding against keeping. The spec defaults to the delver-mirror-bo3 preset's
(post-sideboard games only, one episode per game, the pinned 75s: `mtgz.az.SPEC_DELVER_BO3`); any constructed
best-of-three spec of `mtg` works. `--publish EXPERIMENT [--title T]` posts the result to delverai's
Head-to-heads tab when it ends (mtgz.az.site_h2h, docs/ENGINE.md 193). B's seat is the spec's `"keep_deck"` (an engine setting: its seat's only
sideboarding option is `PASS`). zero.h2h alternates the seats in one driver with one spec, so the seat that keeps
would alternate between the sides; here the games are two drivers, A in seat 0 (`"keep_deck": [1]`) and A in seat 1
(`"keep_deck": [0]`), half the games each, with the same driver seed: game k of each half has the same seed (the
same drawn game 1 and deals, the sides' seats swapped), the pairs of zero.h2h's `--paired`. Both sides search with
their checkpoint's run's evaluation settings (zero.h2h's `run_rule`: eval_select, belief tilts, root search, ...),
no noise, no temperature.

**The arms** (docs/ENGINE.md 141). `informed` (the default, the measure): A's worlds know B keeps
(`"keep_known": true`) - B's deck in them is the one B plays, the registered main, never dealt - so A's games won
measure what sideboarding is worth against a player who keeps, with A's search modelling that player as it is.
`uninformed` (docs/ENGINE.md 130.3's yardstick): A's worlds deal B's 60 from the constructed prior as any
sideboarding player's - B's real deck in about one world in sixteen - so A also pays for searching against decks B
does not play. `both` plays both arms on the same seeds; the difference of their `per_game` is that mis-modelling
cost. B's own worlds (A's deck dealt from the prior) are the same in both arms; no encoding differs.

Prints zero.h2h's summary as one JSON line: A's match score (a match counts its drawn game 1 with
`"start_game": 2`), its paired se, Elo, and `per_game` - A's games won (games 2 and 3): the measure of what
sideboarding is worth - plus `keeps`: 'b' and `arm`; with `--arms both` the informed arm's summary with the
uninformed one under `uninformed` and `per_game_gap` (informed - uninformed games won, a share of the games).
"""
import argparse
import json
import time

from mtgz.az import decklists  # noqa: F401  (the package's registration first)
import mtgz.az as A
from zero import games as G
from zero import h2h as H
from zero.infer import drive

ARMS = ('informed', 'uninformed')


def half_specs(spec, known=True):
    """The two halves' specs: A in seat 0 with seat 1 keeping, A in seat 1 with seat 0 keeping; `known` (the informed
    arm): A's worlds know B keeps (`"keep_known": true`, docs/ENGINE.md 141). Refused unless a constructed best-of-three
    spec without its own `keep_deck` or `keep_known` and without `dealt_decks` (a self-play setting, docs/ENGINE.md 154)."""
    s = json.loads(spec)
    from mtgz.az.forced_find import refuse
    refuse(s, 'keep_h2h')
    if 'dealt_decks' in s:
        raise ValueError('keep_h2h: the spec deals decks (dealt_decks, docs/ENGINE.md 154: a self-play setting); '
                         "the yardstick plays every seat's own sideboarding - pass the run's evaluation spec")
    if not (s.get('constructed') and s.get('format') == 'bo3'):
        raise ValueError('keep_h2h: a constructed best-of-three spec ("constructed": true, "format": "bo3"; docs/ENGINE.md 130)')
    for k in ('keep_deck', 'keep_known'):
        if k in s:
            raise ValueError(f'keep_h2h: the spec names {k} itself (the tool sets it per half and arm)')
    extra = {'keep_known': True} if known else {}
    return [json.dumps(dict(s, keep_deck=[1 - a_seat], **extra), separators=(',', ':')) for a_seat in (0, 1)]


def _arm(game, spec, known, seats, evs, games, seed, threads, groups, conc, max_steps, rec=None):
    """One arm's rows: the two halves' drivers (A in seat k of half k), A's seat gid % 2 (zero.h2h's rows). `rec`:
    (sink, A's player, B's player, arm) - every game a game record (docs/gamedb.md; the games are the same)."""
    rows = []
    for k, (specs, sts) in enumerate(zip(half_specs(spec, known), (seats, seats[::-1]))):
        n = games // 2
        drv = G.Driver(game, [list(sts)] * n, spec=specs, concurrency=conc or min(n, 1024), seed=seed, threads=threads,
                       groups=groups, max_steps=max_steps)
        if rec is not None:
            drv.set_game_log(True)
        drive(drv, evs)
        got = drv.take_results()
        if rec is not None:
            sink, pa, pb, arm = rec
            sink.add_rows(got, lambda g, k=k: [pa, pb] if k == 0 else [pb, pa],
                          {'arm': arm, 'half': k, 'keeps': 'b', 'driver_seed': int(seed)})
        for r in got:
            r = dict(r)
            # (A's seat is gid % 2, as zero.h2h's rows: half k's game g is game 2g + k, the pair of game g)
            r['gid'] = 2 * r['gid'] + k
            rows.append(r)
    return rows


def keep_match(a, b=None, games=400, sims=64, chance_k=2, c_puct=1.0, seed=0, threads=4, groups=2, device='cpu',
               spec=None, conc=None, max_steps=60_000, seat_a=None, seat_b=None, arms='informed', records=None):
    """Checkpoint `a` (sideboards) against `b` (keeps its deck; None: `a`), `games` matches (even) per arm. -> dict
    (zero.h2h's summary, paired: A in seat `gid % 2`, games 2k and 2k + 1 a pair). `arms`: 'informed' (the measure:
    A's worlds know B keeps), 'uninformed' (B's deck dealt from the constructed prior in A's worlds) or 'both' (the
    informed summary, the uninformed one under 'uninformed' and their per-game gap). `seat_a` / `seat_b`: a side's
    move-rule seat settings (None: its checkpoint's run's evaluation settings, `run_rule`). `records`: a game records
    directory (docs/gamedb.md: every match a record under <records>/keep_h2h/misc/), None: none."""
    if games < 2 or games % 2:
        raise ValueError(f'keep_h2h: an even number of games, at least 2 ({games})')
    if arms not in ARMS + ('both',):
        raise ValueError(f"keep_h2h: arms 'informed', 'uninformed' or 'both' ({arms!r})")
    b = b or a
    spec = A.SPEC_DELVER_BO3 if spec is None else spec
    half_specs(spec)
    ma, mb, game = H._load_pair(a, b, device, None)
    if game not in ('mtg', 'mtg_v7'):
        raise ValueError(f'keep_h2h: game mtg or mtg_v7 (constructed best-of-three, layout v8 or v7), not {game}')
    sa, sb = H._seats(a, b, ma, mb, game, sims, c_puct, chance_k, seat_a, seat_b, None, None)
    evs = H._evaluators(ma, mb, device, None)
    sink = None
    if records:
        from zero import gamerec as GR
        sink = GR.MatchSink(records, 'keep_h2h', meta={'tool': 'keep_h2h'})
        pa, pb = GR.net_player(a, None, sa, H._tag(sa)), GR.net_player(b, None, sb, H._tag(sb))
    out = {}
    for arm in (ARMS if arms == 'both' else (arms,)):
        t0 = time.time()
        rows = _arm(game, spec, arm == 'informed', (sa, sb), evs, games, seed, threads, groups, conc, max_steps,
                    (sink, pa, pb, arm) if sink is not None else None)
        o = dict({'a': a, 'b': b, 'keeps': 'b', 'arm': arm, 'game': game, 'sims': sims, 'chance_k': chance_k},
                 **H.summary(rows, paired=True))
        o.update(seconds=round(time.time() - t0, 1), select_a=H._tag(sa), select_b=H._tag(sb), spec=json.loads(spec))
        out[arm] = o
    if arms != 'both':
        return out[arms]
    res = dict(out['informed'], uninformed=out['uninformed'])
    pi, pu = out['informed'].get('per_game'), out['uninformed'].get('per_game')
    if pi and pu:
        res['per_game_gap'] = round(pi['win'] - pu['win'], 4)
    return res


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('a', help='the checkpoint that sideboards (searches every decision)')
    ap.add_argument('b', nargs='?', default=None, help='the checkpoint that keeps its deck (default: a)')
    ap.add_argument('--games', type=int, default=400, help='matches (even; half with A in each seat) per arm')
    ap.add_argument('--arms', default='informed', choices=ARMS + ('both',),
                    help="informed (the measure: A's worlds know B keeps), uninformed (ENGINE.md 130.3's), or both")
    ap.add_argument('--sims', type=int, default=64)
    ap.add_argument('--chance-k', type=int, default=2)
    ap.add_argument('--c-puct', type=float, default=1.0)
    ap.add_argument('--seed', type=int, default=0)
    ap.add_argument('--threads', type=int, default=4)
    ap.add_argument('--groups', type=int, default=2)
    ap.add_argument('--device', default='cpu', help="network device ('cuda' works; the GPU is shared)")
    ap.add_argument('--spec', default=None, help='a constructed best-of-three spec (default: the delver-mirror-bo3 preset\'s)')
    ap.add_argument('--conc', type=int, default=None)
    ap.add_argument('--max-steps', type=int, default=60_000, help='a match\'s step cap (the preset\'s: up to three games)')
    ap.add_argument('--torch-threads', type=int, default=1)
    ap.add_argument('--records', default=None, metavar='DIR',
                    help='keep every match as a game record under DIR/keep_h2h/misc/ (docs/gamedb.md); the same games')
    from mtgz.az import site_h2h as S
    S.add_args(ap)
    a = ap.parse_args(argv)
    S.check(a)
    import torch
    torch.set_num_threads(max(1, a.torch_threads))
    try:
        out = keep_match(a.a, a.b, a.games, a.sims, a.chance_k, a.c_puct, a.seed, a.threads, a.groups, a.device,
                         a.spec, a.conc, a.max_steps, arms=a.arms, records=a.records)
    except ValueError as e:
        ap.error(str(e))
    print(json.dumps(out), flush=True)
    if a.publish and publish(out, a) != 0:
        raise SystemExit(S.EXIT_PUBLISH_FAILED)


def publish(out, o):
    """The result to delverai's Head-to-heads tab (`--publish`, mtgz.az.site_h2h, docs/ENGINE.md 193): A sideboards, B
    keeps; one checkpoint on both sides by default, so the sides' default names say which does what."""
    from mtgz.az import site_h2h as S
    o.a_label = o.a_label or f"{S.checkpoint_label(out['a'])} (sideboards)"
    o.b_label = o.b_label or f"{S.checkpoint_label(out['b'])} (keeps its 60)"
    pg = out.get('per_game') or {}
    gap = f", {100 * out['per_game_gap']:+.1f} points over the uninformed arm" if out.get('per_game_gap') is not None else ''
    note = (f"Keep-your-60 yardstick ({out.get('arm')} arm): B keeps its main 60 at every sideboarding, A searches its "
            f"sideboarding. A's score is its match score (best of three, post-sideboard games)"
            + (f"; A's games won in games 2 and 3: {100 * pg['win']:.1f}%{gap}" if pg.get('win') is not None else '') + '.')
    o.publish_note = note + (' ' + o.publish_note if o.publish_note else '')
    return S.publish_result(out, o, name=S.default_key(out['a'], out['b'], prefix=f"keep-{out.get('arm')}-"))


if __name__ == '__main__':
    main()
