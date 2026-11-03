"""The Delver mirror's archive of start positions for self-play (zero-core's `selfplay_archive`, its
docs/az.md 24; docs/ENGINE.md 87): every position replayed and checked on THIS checkout's engine, or
refused, loudly, with its reason.

    python tools/delver_archive.py --out archive.json [--report archive_report.json]
        [--library data/delver_canary_library.json ...] [--library-ref blunders ...]
        [--play C:/Users/Gregg Keithley/delver-lab/data/play] [--reports .../delver-lab/data/reports]
        [--turns 3-8] [--run-config <run>/config.json | --preset delver-mirror] [--deal actual]
        [--canary all | none | half-a | half-b]

Sources (each entry an mtg spec `start` block, ENGINE.md 76: the source game's seed and root actions, the
library orders nobody knows dealt again per game by `deal` "actual" from the driver's game seed):

* **canary**: the canary library's spots (`data/delver_canary_library.json`; `--library-ref REF` reads
  that file at a git ref, e.g. the `blunders` branch's newer spots; spots in several files once, by id).
  The library stores the raw steps from the deal (single-option decisions included): replayed so,
  checked by the library's own fingerprint (`delver_blunders.spot_sig`: turn, step, decider, options,
  the decider's hand, life), the single-option steps dropped for the `start` block (it steps them itself).
  A run that trains on these spots can no longer be scored on them: `--canary none` leaves them out,
  `half-a` / `half-b` keeps one half (by the id's sha1) and lists the other as held out.
* **human**: the Play tab's games (`<play>/<id>/game.json`: seed + root actions; the human's and the AI's
  moves): the whole game replayed and its final state compared with its recorded view (`view.json`:
  turn, step, active, priority, life, library and hand counts, the human's hand, every public zone with
  ids, tapped and counters, the stack) - a game played on another engine build (the Play tab's early
  games: view-known, float-mana) does not reach it and is skipped - then one position a turn of
  `--turns`: the turn's first decision with two options or more.
* **report**: the Play tab's reports (`<reports>/*.json`: a game and its action count), checked against
  the report's own view of the position.

Every entry is then rebuilt through its `start` block (deal "none") and must equal the source replay's
state exactly (the engine's whole exported state but the `lab` and `log` fields); with deal "actual" it
must start, the same decider, the same fingerprint. Positions with the same (seed, actions) are kept
once. The spec checked against is the run's (`--run-config`, else the preset's); a source played with
another spec (other decks, turn limit) is skipped. Nothing is ever dropped silently: the report lists
every skipped source with its reason, and the archive carries the counts.
"""
import argparse
import collections
import glob
import hashlib
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import delver_blunders as DB  # noqa: E402  (load_engine, spot_sig, the canary library)

FORMAT = 'zero-archive/1'
DELVER_LAB = 'C:/Users/Gregg Keithley/delver-lab/data'
LIBRARY_REL = 'data/delver_canary_library.json'


def canon(obj):
    return json.dumps(obj, sort_keys=True, separators=(',', ':'))


def spec_key(spec):
    """A spec compared without its `log` (logging only) and `start`."""
    s = json.loads(spec) if isinstance(spec, str) else dict(spec)
    s.pop('log', None)
    s.pop('start', None)
    return canon(s)


def state_of(g):
    st = json.loads(g.export_state())
    st.pop('lab', None)
    st.pop('log', None)
    return st


def auto_step(g):
    while not g.over and len(g.legal()) == 1:
        g.step(int(g.legal()[0]))


# ------------------------------------------------------------------------------------------- the view
def _objs(zone, hidden=False):
    out = []
    for c in zone or []:
        if hidden or c.get('back'):
            out.append('?')
            continue
        out.append([c.get('id'), c.get('name'), bool(c.get('tapped')), canon(c.get('counters') or {})])
    return out


def view_print(st, you):
    """The comparable part of a Play view's state (or an engine state seen by `you`): what both show."""
    ps = []
    for p in st['players']:
        mine = p.get('id') == you
        ps.append([p.get('id'), p.get('life'), p.get('library') if isinstance(p.get('library'), int) else len(p.get('library') or []),
                   len(p.get('hand') or []), _objs(p.get('hand')) if mine else None, _objs(p.get('graveyard')),
                   _objs(p.get('exile')), _objs(p.get('battlefield'))])
    stack = [[s.get('id'), s.get('name') or s.get('text')] for s in st.get('stack') or []]
    return [st.get('turn'), st.get('active'), st.get('step'), st.get('priority'), ps, stack]


# ----------------------------------------------------------------------------------------- the sources
def library_spots(paths, refs):
    """[(spot, file)] of every canary library, each id once (the first file that has it)."""
    out, seen = [], set()
    files = []
    for p in paths:
        files.append((p, DB.load_library(p)))
    for ref in refs:
        txt = subprocess.run(['git', '-C', ROOT, 'show', f'{ref}:{LIBRARY_REL}'], capture_output=True, text=True,
                             encoding='utf-8')
        if txt.returncode:
            raise SystemExit(f'--library-ref {ref}: {txt.stderr.strip()}')
        files.append((f'{ref}:{LIBRARY_REL}', json.loads(txt.stdout)))
    for name, L in files:
        if not L or L.get('format') != DB.LIBRARY_FORMAT:
            raise SystemExit(f'{name}: not a {DB.LIBRARY_FORMAT} file')
        for sp in L['spots']:
            if sp['id'] not in seen:
                seen.add(sp['id'])
                out.append((sp, name))
    return out


def run_spec_of(model_path):
    """The spec of the run a Play game's model came from (its config.json beside models/)."""
    run = os.path.dirname(os.path.dirname(model_path))
    p = os.path.join(run, 'config.json')
    if not os.path.exists(p):
        return None
    with open(p, encoding='utf-8') as f:
        return json.load(f).get('spec')


class Builder:
    def __init__(self, E, spec, deal, turns):
        self.E, self.spec, self.deal = E, spec, deal
        self.key = spec_key(spec)
        self.t0, self.t1 = turns
        self.entries, self.skipped, self.held_out = [], [], []
        self.seen = {}

    def skip(self, kind, source, reason, **kw):
        self.skipped.append(dict(kind=kind, source=source, reason=reason, **kw))

    def start_spec(self, seed, actions, deal):
        s = json.loads(self.spec)
        s['start'] = {'seed': int(seed), 'actions': [int(a) for a in actions], 'deal': deal}
        return json.dumps(s)

    def add(self, kind, source, seed, actions, ref_state, extra):
        """Entry from the source game's (seed, root actions) whose replay's state is `ref_state`: rebuilt
        through its start block (deal none: the same state exactly; the deal of the archive: it starts,
        same decider and fingerprint). -> True, or False with the source skipped."""
        k = (int(seed), tuple(int(a) for a in actions))
        if k in self.seen:
            self.seen[k]['also'].append(source)
            return True
        try:
            g0 = self.E.Game('mtg', self.start_spec(seed, actions, 'none'), 2, 0)
        except Exception as e:                                   # noqa: BLE001
            self.skip(kind, source, f'its start block does not start: {e}', **extra)
            return False
        if state_of(g0) != ref_state:
            self.skip(kind, source, 'its start block replays to another state than its source replay', **extra)
            return False
        sig = DB.spot_sig(json.loads(g0.export_state()))
        try:
            g1 = self.E.Game('mtg', self.start_spec(seed, actions, self.deal), 2, 12345)
        except Exception as e:                                   # noqa: BLE001
            self.skip(kind, source, f'deal {self.deal!r} does not start: {e}', **extra)
            return False
        if g1.to_move != g0.to_move or DB.spot_sig(json.loads(g1.export_state())) != sig:
            self.skip(kind, source, f'deal {self.deal!r} changed what the decider sees', **extra)
            return False
        st = ref_state
        e = {'id': f'{kind}:{source}', 'kind': kind, 'source': source, 'turn': st.get('turn'), 'step': st.get('step'),
             'decider': int(g0.to_move), 'sig': sig, 'actions': len(actions), 'also': [],
             'patch': {'start': {'seed': int(seed), 'actions': [int(a) for a in actions], 'deal': self.deal}}, **extra}
        self.entries.append(e)
        self.seen[k] = e
        return True

    # ---- canary spots
    def canary(self, spots, which='all'):
        """`which`: 'all', 'none', or 'half-a' / 'half-b' - the spots whose id's sha1 is even / odd (the other
        half held out: listed in the report, e.g. as a clean canary set for an A/B whose arm trains on these)."""
        for sp, fname in spots:
            src = sp['id']
            extra = {'class': sp['kind'], 'file': fname}
            half = 'half-a' if int(hashlib.sha1(src.encode()).hexdigest(), 16) % 2 == 0 else 'half-b'
            if which == 'none' or (which.startswith('half') and half != which):
                self.held_out.append({'id': src, 'class': sp['kind'], 'half': half})
                continue
            if spec_key(sp['spec']) != self.key:
                self.skip('canary', src, 'played with another spec than the run\'s', **extra)
                continue
            try:
                sj = json.loads(self.spec)
                g = self.E.Game('mtg', json.dumps(sj), 2, int(sp['seed']))
                roots = []
                for a in sp['actions']:
                    if g.over:
                        raise RuntimeError('the game is over before the spot')
                    legal = [int(x) for x in g.legal()]
                    if int(a) not in legal:
                        raise RuntimeError(f'action {a} is not legal (legal {legal})')
                    if len(legal) > 1:
                        roots.append(int(a))
                    g.step(int(a))
                auto_step(g)
            except Exception as e:                               # noqa: BLE001
                self.skip('canary', src, f'does not replay on this engine: {e}', **extra)
                continue
            st = json.loads(g.export_state())
            if sp.get('sig') and DB.spot_sig(st) != sp['sig']:
                self.skip('canary', src, 'replays to another position on this engine (its fingerprint differs)', **extra)
                continue
            if g.to_move != sp['decider']:
                self.skip('canary', src, f'the decider is P{g.to_move}, the library says P{sp["decider"]}', **extra)
                continue
            self.add('canary', src, sp['seed'], roots, state_of(g), extra)

    # ---- the Play tab's games and reports
    def replay_play(self, rec, keep):
        """The Play record replayed from its seed (each root action after the start's auto-steps) ->
        (final game, {k: game after root action k (1-based) for k in keep}, [turn after each action]);
        raises at an action the engine does not offer."""
        g = self.E.Game('mtg', self.spec, 2, int(rec['seed']))
        auto_step(g)
        kept, turns = {}, []
        for i, a in enumerate(rec.get('actions') or []):
            if g.over or int(a) not in [int(x) for x in g.legal()]:
                raise RuntimeError(f'action {i} ({a}) is not legal')
            g.step(int(a))
            auto_step(g)
            turns.append(None if g.over else json.loads(g.export_state()).get('turn'))
            if (i + 1) in keep:
                kept[i + 1] = g.clone()
        return g, kept, turns

    def play(self, play_dir, rep_dir):
        reports = collections.defaultdict(list)
        if rep_dir and os.path.isdir(rep_dir):
            for p in sorted(glob.glob(os.path.join(rep_dir, '*.json'))):
                try:
                    with open(p, encoding='utf-8') as f:
                        R = json.load(f)
                except (OSError, ValueError) as e:
                    self.skip('report', os.path.basename(p), f'unreadable: {e}')
                    continue
                reports[R.get('game')].append(R)
        done = set()
        for p in sorted(glob.glob(os.path.join(play_dir, '*', 'game.json'))):
            gid = os.path.basename(os.path.dirname(p))
            try:
                with open(p, encoding='utf-8') as f:
                    rec = json.load(f)
                with open(os.path.join(os.path.dirname(p), 'view.json'), encoding='utf-8') as f:
                    view = json.load(f)
            except (OSError, ValueError) as e:
                self.skip('human', gid, f'unreadable record: {e}')
                continue
            m = rec.get('model') or {}
            extra = {'model': f"{m.get('run', '?')}@{m.get('iter', '?')}", 'game': gid}
            rs = run_spec_of(m.get('path') or '')
            if rs is not None and spec_key(rs) != self.key:
                self.skip('human', gid, 'played with another spec than the run\'s', **extra)
                continue
            if not rec.get('actions'):
                self.skip('human', gid, 'no moves recorded', **extra)
                continue
            n = int(view.get('n', len(rec['actions'])))
            if n != len(rec['actions']) + int(rec.get('vn') or 0):
                self.skip('human', gid, f'its view is of {n} answers, the record of {len(rec["actions"])} + {rec.get("vn")}',
                          **extra)
                continue
            try:
                final, _, turns = self.replay_play(rec, ())
            except Exception as e:                               # noqa: BLE001
                self.skip('human', gid, f'does not replay on this engine: {e}', **extra)
                continue
            you = int(rec['you'])
            if view_print(json.loads(final.export_state()), you) != view_print(view.get('state') or {}, you):
                self.skip('human', gid, 'replays to another state than its recorded view on this engine (played on '
                          'another engine build)', **extra)
                continue
            done.add(gid)
            # one position a turn: the turn's first decision (after the root action that reached it)
            first = {}
            for k, t in enumerate(turns, start=1):
                if t is not None and self.t0 <= t <= self.t1 and t not in first:
                    first[t] = k
            if not first:
                self.skip('human', gid, f'no decision in turns {self.t0}-{self.t1} (the game ended or stopped at turn '
                          f'{max([t for t in turns if t is not None], default=0)})', **extra)
            want = set(first.values())
            reps = []
            for R in reports.get(gid, []):
                k = int(R.get('step', R.get('n', 0)))
                if not 1 <= k <= len(turns):
                    self.skip('report', R.get('id'), f'at action {k} of a game of {len(turns)}', game=gid)
                    continue
                reps.append((R, k))
                want.add(k)
            _, kept, _ = self.replay_play(rec, want)
            for t, k in sorted(first.items()):
                self.add('human', f'{gid}@t{t}', rec['seed'], rec['actions'][:k], state_of(kept[k]), dict(extra))
            for R, k in reps:
                g = kept[k]
                if view_print(json.loads(g.export_state()), you) != view_print((R.get('view') or {}).get('state') or {}, you):
                    self.skip('report', R.get('id'), 'its position replays to another state than the report\'s view', game=gid)
                    continue
                self.add('report', R.get('id'), rec['seed'], rec['actions'][:k], state_of(g),
                         {'game': gid, 'text': R.get('text'), 'model': extra['model']})
        for gid, rs in reports.items():
            if gid not in done:
                for R in rs:
                    self.skip('report', R.get('id'), f'its game {gid} did not replay (or is missing)', game=gid)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out', required=True)
    ap.add_argument('--report', default=None, help='the build report (JSON; default <out>.report.json)')
    ap.add_argument('--library', nargs='*', default=[DB.LIBRARY_PATH])
    ap.add_argument('--library-ref', nargs='*', default=[], help='git refs whose data/delver_canary_library.json joins')
    ap.add_argument('--play', default=os.path.join(DELVER_LAB, 'play'))
    ap.add_argument('--reports', default=os.path.join(DELVER_LAB, 'reports'))
    ap.add_argument('--turns', default='3-8')
    ap.add_argument('--run-config', default=None, help="a run's config.json: its spec (default: --preset's)")
    ap.add_argument('--preset', default='delver-mirror')
    ap.add_argument('--deal', default='actual', choices=['actual', 'uniform', 'none'])
    ap.add_argument('--canary', default='all', choices=['all', 'none', 'half-a', 'half-b'],
                    help="the canary spots in the archive: all, none, or one half by their id's sha1 (the other held out)")
    a = ap.parse_args(argv)
    E = DB.load_engine(ROOT)
    from zero.games import spec_json
    if a.run_config:
        with open(a.run_config, encoding='utf-8') as f:
            spec = spec_json(json.load(f)['spec'])
    else:
        from mtgz.az import PRESETS
        spec = spec_json(PRESETS[a.preset]['spec'])
    t0, t1 = (int(x) for x in a.turns.split('-'))
    B = Builder(E, spec, a.deal, (t0, t1))
    t = time.time()
    B.canary(library_spots(a.library, a.library_ref), a.canary)
    if a.play and os.path.isdir(a.play):
        B.play(a.play, a.reports)
    counts = dict(collections.Counter(e['kind'] for e in B.entries))
    skipped = dict(collections.Counter(s['kind'] for s in B.skipped))
    if not B.entries:
        raise SystemExit('no archive entry replays on this engine: nothing written')
    info = DB.engine_info(ROOT)
    out = {'format': FORMAT, 'game': 'mtg', 'base_spec': spec, 'deal': a.deal, 'turns': [t0, t1],
           'built': {'date': time.strftime('%Y-%m-%d %H:%M'), 'engine': info, 'seconds': round(time.time() - t, 1)},
           'counts': counts, 'skipped_counts': skipped, 'canary': a.canary,
           'canary_held_out': [h['id'] for h in B.held_out], 'entries': B.entries}
    os.makedirs(os.path.dirname(os.path.abspath(a.out)) or '.', exist_ok=True)
    with open(a.out, 'w', encoding='utf-8', newline='\n') as f:
        json.dump(out, f, indent=1)
    rp = a.report or os.path.splitext(a.out)[0] + '.report.json'
    with open(rp, 'w', encoding='utf-8', newline='\n') as f:
        json.dump({'entries': counts, 'skipped': B.skipped, 'skipped_counts': skipped, 'canary_held_out': B.held_out,
                   'duplicates': {e['id']: e['also'] for e in B.entries if e['also']}}, f, indent=1)
    print(f'{a.out}: {len(B.entries)} entries {counts}; {len(B.skipped)} sources skipped {skipped} (reasons in {rp})')
    for s in B.skipped:
        print(f'  SKIPPED {s["kind"]} {s["source"]}: {s["reason"]}')
    return out


if __name__ == '__main__':
    main()
