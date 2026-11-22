"""The stew's phases and loop (docs/ENGINE.md 239.2). `Stew(dir)` over an `Ops` (mtgz.stew.control; tests: fakes).

Generation g (round g) runs five phases, each tagged '<phase>@g', reading version g of every net and writing version
g + 1:

1. `train`          - each specialist trains `recipe.turn_iters` iterations in turn (round-robin, field order): its
                      supervisor with --iters = its version g's iteration + B from the stew's code, its farm run active
                      only during its turn; version g + 1 = that checkpoint.
2. `distil_decks`   - the turn's newest shards of every specialist linked into rows/gen_<g+1>/ (`sources.copy_iters`),
                      then each deck stew distilled (`sources.deck_sources`; tools/distill.py --sources).
3. `judge_deck_gap` - specialists gen over gen, arms B and A of every covered cell (mtgz.stew.judge); report.
4. `distil_main`    - the main stew distilled from the deck stews on the specialists' rows + the retention source.
5. `judge`          - arm M of every covered cell; the generation's report.

Every phase is idempotent: what it already made (a version recorded in state.json, a valid judge result, a snapshot)
is kept, a kill at any point resumes where it stopped. A phase that did no work (0 iterations, 0 shards, no output,
no job) fails loudly. STOP / PAUSE files in the stew's directory stop the loop at the next safe point (a live
specialist paused at its iteration boundary and its supervisor ended; a distill or judge process stopped: distill
resumes from its partial file); `run` refuses to start while PAUSE is there (remove it to continue) and moves a STOP
to STOP.old (as tools/supervise.py does). `recipe.hold_after` tags pause the stew after that phase. Before the first
GPU phase of a `run` the stew pauses the `yield_runs` at their iteration boundary (tools/pause_run.py --wait) and
their farm runs, and gives them back - only those it paused - when it stops, pauses, holds or fails. It refuses to
start while a supervisor is live that is neither a yield run's nor a specialist's.
"""
import json
import os
import shutil
import time

from . import config as C
from . import judge as J
from . import sources as S
from .state import State, jload, jsave, new, now, sha256, stamp, text_save


class Halt(Exception):
    """The stew stops here (STOP, PAUSE or a hold point): not a failure."""


class Refused(RuntimeError):
    """The stew may not go on (a live supervisor not its own, a yield run that does not pause, a phase without work)."""


POLL_S = 30.0
GRACE_S = 120.0


def _norm(p):
    return os.path.normcase(os.path.abspath(str(p)))


class Stew:
    def __init__(self, stew_dir, ops=None, log=print, cfg=None, poll_s=None):
        self.dir = os.path.abspath(stew_dir)
        self.log = log
        self.cfg = cfg if cfg is not None else C.load(os.path.join(self.dir, 'stew.json'), check_lists=False)
        if ops is None:
            from .control import Ops
            ops = Ops(self.cfg, self.dir, log)
        self.ops = ops
        self.st = State.load(self.dir) if os.path.exists(os.path.join(self.dir, 'state.json')) else None
        self.poll_s = POLL_S if poll_s is None else poll_s
        self._yielded = False
        self._src_cfg = None

    # ---------------------------------------------------------------- paths
    def p(self, *parts):
        return os.path.join(self.dir, *parts)

    def net_path(self, name, gen):
        if name == 'main':
            return self.p('nets', 'main', f'gen_{gen:03d}.pt')
        return self.p('nets', C.deck_dir(name.split(':', 1)[1]), f'gen_{gen:03d}.pt')

    def stop_requested(self):
        return os.path.exists(self.p('STOP')) or os.path.exists(self.p('PAUSE'))

    def say(self, msg):
        self.log(f"{time.strftime('%H:%M:%S')} {msg}")
        with open(self.p('stew.log'), 'a', encoding='utf-8') as f:
            f.write(f"{stamp()} {msg}\n")

    # ---------------------------------------------------------------- the source and the recipe
    def source_cfg(self):
        if self._src_cfg is None:
            p = os.path.join(self.cfg['source']['run'], 'config.json')
            if not os.path.exists(p):
                raise Refused(f'source run: no {p}')
            self._src_cfg = json.load(open(p, encoding='utf-8'))
        return self._src_cfg

    def source_iter(self):
        it = self.cfg['source']['iter']
        if it == 'newest':
            it = self.ops.newest(self.cfg['source']['run'])
            if it is None:
                raise Refused(f"source run {self.cfg['source']['run']}: no checkpoint")
        return int(it)

    def spec_keys(self):
        """max_turns and mulligans of the source's spec (the specialists' and the judge's games keep them)."""
        s = json.loads(self.source_cfg().get('spec') or '{}')
        return {'max_turns': int(s.get('max_turns', 60)), 'mulligans': bool(s.get('mulligans', True))}

    def pair_spec_json(self, p, q, chooser):
        """The judge's spec of seat 0 = p against seat 1 = q (decklists.pair_spec in the recipe's format) with the
        spec's `chooser` (None: the engine's)."""
        from mtgz.az import decklists as D
        r = self.cfg['recipe']
        k = self.spec_keys()
        s = json.loads(D.pair_spec(p, q, fmt=r['format'], max_turns=k['max_turns'], mulligans=k['mulligans'],
                                   game_episodes=bool(r['game_episodes']), start_game=int(r['start_game'])))
        if chooser is not None:
            s['chooser'] = int(chooser)
        return json.dumps(s, separators=(',', ':'))

    def specialist_spec(self, a, b, extras=True):
        """The specialist's self-play spec: the recipe's metagame restricted to its pairing (decks [A, B], weights
        [[0, 1], [1, 0]]: each match A against B, either seat; a mirror decks [A], weights [[1]]) with
        `recipe.spec_extra` over its top level and `recipe.deck_spec_extra[<deck>]` over each deck's entry (self-play
        options such as dealt decks per seat; `extras` False: the plain spec, the specialist's eval_spec)."""
        from mtgz.az import decklists as D
        r = self.cfg['recipe']
        k = self.spec_keys()
        decks, w = ([a], [[1]]) if a == b else ([a, b], [[0, 1], [1, 0]])
        spec = D.metagame_spec(decks, fmt=r['format'], weights=w, max_turns=k['max_turns'], mulligans=k['mulligans'],
                               game_episodes=bool(r['game_episodes']), start_game=int(r['start_game']))
        if not extras or not (r['spec_extra'] or r['deck_spec_extra']):
            return spec
        s = json.loads(spec)
        s.update(r['spec_extra'])
        for d in s['metagame']['decks']:
            d.update(r['deck_spec_extra'].get(d['name']) or {})
        return json.dumps(s, separators=(',', ':'))

    def farm_settings(self):
        """code.farm's pool settings ("pool_build": "published" -> the published build of the code's commit)."""
        f = self.cfg['code'].get('farm')
        if not f:
            return {}
        out = dict(f)
        v = out.get('pool_build')
        if isinstance(v, str) and (v == 'published' or v.startswith('published:')):
            from mtgz.az import farm_builds as FB
            rev = v.split(':', 1)[1] if ':' in v else (self.cfg['code'].get('sha') or 'HEAD')
            e = FB.lookup(rev, self.ops.wt)
            if e is None:
                raise Refused(f'code.farm.pool_build {v!r}: no published farm build of {rev} '
                              f'(python tools/farm_release.py release {rev})')
            for key, val in FB.pool_settings(e).items():
                if key == 'pool_build' or key not in f:
                    out[key] = val
        return out

    def specialist_settings(self, a, b):
        """The specialist's settings: the source run's config.json, then the stew's own (the pairing's spec with the
        recipe's spec extras, the plain one as eval_spec, evaluation off, no publisher / yardsticks / forced find, its
        farm run), then code.farm, recipe.settings (e.g. the matchup model's settings) and recipe.pair_settings ->
        (settings, notes)."""
        src = dict(self.source_cfg())
        r = self.cfg['recipe']
        notes = []
        spec = self.specialist_spec(a, b)
        sspec = json.loads(src.get('spec') or '{}')
        if 'forced_find' in sspec:
            notes.append('forced_find dropped: ' + (
                'the engine refuses it in best-of-three (ENGINE.md 180)' if r['format'] == 'bo3' else
                'a new run would restart its schedule at its start p'))
        fr = C.farm_run(self.cfg['name'], self.st.decks() if self.st else self.cfg['decks'], a, b)
        stew = {'spec': spec, 'eval_spec': self.specialist_spec(a, b, extras=False), 'iters': 0, 'eval_every': 100000,
                'elo_anchors': '', 'eval_remote': '', 'eval_games': 0, 'eval_random_games': 0, 'elo_games': 0,
                'yardsticks': {}, 'publish': {}, 'forced_find_schedule': {},
                'selfplay_remote': ('pool:' + fr) if r['farm'] else ''}
        out = dict(src)
        out.update(stew)
        out.update(self.farm_settings())
        out.update(r['settings'])
        out.update(r['pair_settings'].get(C.pair_key(a, b), {}))
        for k in ('spec', 'eval_spec', 'iters', 'selfplay_remote'):
            out[k] = stew[k]
        out.pop('run', None)
        # (the stew's game with its layout record: the source's config names `mtg` without one = layout v7's mtg_v7)
        from zero import package
        package.set_game(out, r['game'])
        if r['farm'] and not out.get('pool_build'):
            raise Refused(f'specialist {a}|{b}: recipe.farm but no pool_build (code.farm or the source\'s settings)')
        return out, notes

    def judge_play(self):
        """Both sides' play settings in every judge match: the specialists' evaluation seat (zero.h2h.run_rule of a
        specialist's checkpoint - its config's eval_select, belief, leaf view; the deck tilt kept: best of three
        has hidden 60s), its margin_w / redet / eval_cache, max_steps and sparse batches; the judge's sims, device,
        threads, groups, chance_k, precision."""
        from zero import h2h
        pairs = C.pairings(self.st.decks(), self.st.candidates())
        sn = C.spec_name(*pairs[0])
        run = self.st.abs(self.st.net(sn)['run'])
        ck = os.path.join(run, 'models', 'iter_0000.pt')
        cfg = jload(os.path.join(run, 'config.json'), {}) or {}
        j = self.cfg['recipe']['judge']
        return {'seat': h2h.run_rule(ck), 'seat_keys': h2h.run_seat_keys(ck), 'sims': j['sims'],
                'device': j['device'], 'threads': j['threads'], 'groups': j['groups'], 'chance_k': j['chance_k'],
                'precision': j['precision'], 'max_steps': int(cfg.get('max_steps') or 5000),
                'sparse': bool(cfg.get('sparse'))}

    # ---------------------------------------------------------------- init
    @classmethod
    def init(cls, stew_dir, config_path, ops=None, log=print):
        cfg = C.load(config_path)
        stew_dir = os.path.abspath(stew_dir)
        os.makedirs(stew_dir, exist_ok=True)
        sp = os.path.join(stew_dir, 'stew.json')
        have = jload(sp)
        if have is not None and have != cfg:
            raise Refused(f'{sp} exists with another config: a stew\'s config does not change (add-deck moves its '
                          f'field); init another directory')
        jsave(sp, cfg)
        s = cls(stew_dir, ops=ops, log=log, cfg=cfg)
        s._init()
        return s

    def _init(self):
        cfg = self.cfg
        if cfg['code'].get('worktree') or cfg['code'].get('sha'):
            self.say(f"code: {self.ops.wt} at {self.ops.check_code(cfg['code'].get('sha'))[:12]}")
        for r in cfg.get('requires') or []:
            self.say(f'requires: {r}')
        src = cfg['source']['run']
        it = self.source_iter()
        ck = os.path.join(src, 'models', f'iter_{it:04d}.pt')
        if not os.path.isfile(ck):
            raise Refused(f'source: no {ck}')
        for k, p in cfg['seeds'].items():
            if not os.path.isfile(p):
                raise Refused(f'seeds.{k}: no {p}')
        for y in cfg['yield_runs']:
            if not os.path.isfile(os.path.join(y['run'], 'config.json')):
                raise Refused(f"yield_runs: {y['run']} is not a run directory")
        self.source_cfg()
        if self.st is None:
            self.st = State(self.dir, new(cfg))
            self.st.d['source'] = {'run': src.replace('\\', '/'), 'iter': it}
            self.st.save()
        st = self.st
        if st.version('main', 0) is None:
            dst = self.net_path('main', 0)
            # (the stew's game, recipe.game: its gen-0 main stew is the source checkpoint widened onto that game's
            # layout - the meta3 run's v7 network onto v8, docs/ENGINE.md 236; every net below descends from it)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            self.ops.widen(ck, dst, cfg['recipe']['game'], self.p('logs', 'init.log'))
            st.add_version('main', {'gen': 0, 'path': st.rel(dst), 'sha256': sha256(dst), 'time': now(),
                                    'game': cfg['recipe']['game'], 'source_sha256': sha256(ck),
                                    'from': {'run': src.replace('\\', '/'), 'iter': it}})
            st.d['source']['sha256'] = st.version('main', 0)['sha256']
            st.save()
        for d in st.decks():
            self._new_deck_stew(d['name'], 0)
        if 'archive' not in st.d:
            st.d['archive'] = self._archive(src, it)
            st.save()
        for a, b in C.pairings(st.decks(), st.candidates()):
            self._new_specialist(a, b, 0)
        self.say(f"init: {len(st.decks())} decks, {len(st.candidates())} candidate(s), "
                 f"{len(C.pairings(st.decks(), st.candidates()))} specialists; source {src} @ {it}")

    def _copy(self, src, dst):
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        tmp = dst + '.part'
        shutil.copyfile(src, tmp)
        os.replace(tmp, dst)

    def _new_deck_stew(self, x, gen):
        """Deck stew x's version `gen` = a copy of the main stew's version `gen`."""
        st = self.st
        name = C.deck_net(x)
        if st.version(name, gen) is not None:
            return
        src = st.ckpt('main', gen)
        dst = self.net_path(name, gen)
        self._copy(src, dst)
        st.add_version(name, {'gen': gen, 'path': st.rel(dst), 'sha256': sha256(dst), 'time': now(),
                              'parents': [f'main@{gen}']})
        st.save()

    def _archive(self, src, it):
        """The source run's window (its complete data iterations up to `it`) and config.json into archive/: the
        retention source's rows (the source run deletes old shards)."""
        its = S.pick_iters(src, -1, it, 1 << 30)
        unc = C.uncovered_cells(self.st.decks(), C.pairings(self.st.decks(), self.st.candidates()))
        if not its:
            if unc and self.cfg['recipe']['retention_share'] not in (0, 0.0):
                raise Refused(f'source run {src}: no data iterations up to {it} to archive (the retention source of the '
                              f'uncovered cells {[C.pair_key(*c) for c in unc]}; retention_share 0 to go without)')
            return {'iters': [], 'shards': 0}
        self.say(f'archive: {src} data iterations {its[0]}..{its[-1]} ({self.cfg["recipe"]["archive"]})')
        out = S.copy_iters(src, its, self.p('archive'), mode=self.cfg['recipe']['archive'])
        out['dirs'] = [self.st.rel(d) for d in out['dirs']]
        return out

    def _new_specialist(self, a, b, gen):
        """Specialist {a, b}: a NEW run (tools/fork_run.py <seed> runs/spec-a-b: its iteration 0 the seed, no
        inherited window, a fresh optimizer) with its config.json written (`specialist_settings`, resolved as the run
        resolves it) and the seed's heads checked against it; its version `gen` = iteration 0."""
        st = self.st
        sn = C.spec_name(a, b)
        if st.version(sn, gen) is not None:
            return
        seed = self.cfg['seeds'].get(C.pair_key(a, b)) or st.ckpt('main', gen)
        run = self.p('runs', C.spec_dir(a, b))
        ck0 = os.path.join(run, 'models', 'iter_0000.pt')
        if not os.path.exists(ck0):
            self.say(f'{sn}: fork_run {seed} -> {run}')
            self.ops.fork_new_run(seed, run, self.p('logs', 'init.log'), self.cfg['recipe']['game'])
        settings, notes = self.specialist_settings(a, b)
        self.ops.check_spec(settings.get('game', 'mtg'), settings['spec'])
        cfgp = os.path.join(run, 'config.json')
        resolved = self.ops.resolve(run, settings)
        old = jload(cfgp)
        if old is None or ((self.ops.newest(run) or 0) == 0 and old != resolved):
            jsave(cfgp, resolved)
        elif old != resolved:
            diff = sorted(k for k in set(old) | set(resolved) if k != 'iters' and old.get(k) != resolved.get(k))
            if diff:
                self.say(f'{sn}: its run has trained: config.json kept, NOT the recipe settings of now ({diff})')
        self.ops.check_heads(ck0, resolved)
        st.d['nets'][sn] = dict(st.d['nets'].get(sn) or {}, run=st.rel(run),
                                farm_run=C.farm_run(self.cfg['name'], st.decks(), a, b) if self.cfg['recipe']['farm']
                                else None, joined=gen, notes=notes, versions=(st.d['nets'].get(sn) or {}).get('versions', []))
        st.add_version(sn, {'gen': gen, 'iter': 0, 'path': st.rel(ck0), 'sha256': sha256(ck0), 'time': now(),
                            'seed': seed.replace('\\', '/'), 'seed_sha256': sha256(seed)})
        st.save()
        for n in notes:
            self.say(f'{sn}: {n}')

    # ---------------------------------------------------------------- machine: supervisors, yield runs, farm
    def own_runs(self):
        out = {_norm(self.st.abs(n['run'])) for k, n in self.st.d['nets'].items() if k.startswith('spec:')}
        return out

    def check_supervisors(self):
        allowed = (self.own_runs() | {_norm(y['run']) for y in self.cfg['yield_runs']}
                   | {_norm(r) for r in self.cfg.get('beside_runs') or []})
        bad = []
        for s in self.ops.live_supervisors():
            r = s.get('run') or ''
            if r and not os.path.isabs(r) and s.get('cwd'):
                r = os.path.join(s['cwd'], r)
            if not r or _norm(r) not in allowed:
                bad.append(f"{s.get('run')!r} (pid {s.get('pid')})")
        if bad:
            raise Refused(f'live supervisors that are neither the stew\'s specialists nor its yield_runs: '
                          f'{", ".join(sorted(set(bad)))} - the stew uses this machine\'s GPU; stop them, list '
                          f'them in yield_runs, or (a stew off the GPU) in beside_runs')

    def yield_machine(self):
        """Pause the yield runs at their boundary and their farm runs, once per run of the stew (recorded: only what
        the stew paused is given back)."""
        if self._yielded:
            return
        y = self.st.d.setdefault('yield', {'runs': {}, 'farm': {}})
        for e in self.cfg['yield_runs']:
            run = e['run']
            key = run.replace('\\', '/')
            if key not in y['runs']:
                state = self.ops.run_status(run)['state']
                if state in ('paused', 'stopped', None):
                    y['runs'][key] = f'was {state}'
                else:
                    self.say(f'yield: pausing {run} at its iteration boundary ({state})')
                    after = self.ops.pause_boundary(run, self.p('logs', 'yield.log'))
                    if after not in ('paused', 'stopped'):
                        raise Refused(f'yield: {run} did not pause at its boundary (state {after!r}): not starting '
                                      f'GPU work beside it')
                    y['runs'][key] = 'paused by the stew'
                self.st.save()
            for f in e['farm_runs']:
                if f not in y['farm']:
                    old = self.ops.farm_set(f, 'paused')
                    y['farm'][f] = old
                    self.st.save()
                    self.say(f'yield: farm run {f} {old} -> paused')
        self._yielded = True

    def release_machine(self):
        """Give back what the stew paused (only when no specialist of the stew is live); retry pending farm pauses."""
        errs = []
        for f in list(self.st.d.get('farm_to_pause') or []):
            try:
                self.ops.farm_set(f, 'paused')
                self.st.d['farm_to_pause'].remove(f)
            except Exception as e:             # noqa: BLE001 - reported below
                errs.append(f'farm run {f} still active ({e})')
        self.st.save()
        live = [k for k, n in self.st.d['nets'].items() if k.startswith('spec:')
                and self.ops.supervisor(self.st.abs(n['run']))[1]]
        if live:
            self.say(f'release: NOT giving the machine back - specialist(s) live: {live}')
            return errs
        y = self.st.d.get('yield') or {'runs': {}, 'farm': {}}
        for run, how in list(y['runs'].items()):
            if how == 'paused by the stew':
                try:
                    self.say(f'release: resuming {run}')
                    self.ops.resume(run, self.p('logs', 'yield.log'))
                except Exception as e:         # noqa: BLE001
                    errs.append(f'resume {run} FAILED ({e})')
                    continue
            del y['runs'][run]
        for f, old in list(y['farm'].items()):
            if old == 'active':
                try:
                    self.ops.farm_set(f, 'active')
                    self.say(f'release: farm run {f} -> active')
                except Exception as e:         # noqa: BLE001
                    errs.append(f'farm run {f} not re-activated ({e})')
                    continue
            del y['farm'][f]
        self.st.save()
        self._yielded = False
        for e in errs:
            self.say(f'RELEASE PROBLEM: {e}')
        return errs

    # ---------------------------------------------------------------- the phase machine
    def specialists(self, g):
        """The specialists of round g (in the field's pairing order): (a, b, net name)."""
        out = []
        for a, b in C.pairings(self.st.decks(), self.st.candidates()):
            sn = C.spec_name(a, b)
            if self.st.version(sn, g) is None:
                raise Refused(f'{sn}: no version {g} (it joins at {self.st.net(sn).get("joined")})')
            out.append((a, b, sn))
        return out

    def advance(self, start, result=None):
        st = self.st
        tag = st.tag()
        st.log(tag, start, result)
        i = C.PHASES.index(st.d['phase'])
        if i + 1 < len(C.PHASES):
            st.d['phase'] = C.PHASES[i + 1]
        else:
            st.d['phase'] = C.PHASES[0]
            st.d['round'] += 1
        held = tag in self.cfg['recipe']['hold_after'] and not any(h.get('tag') == tag for h in st.d.get('holds', []))
        if held:
            st.d['held'] = {'tag': tag, 'time': now()}
            st.d.setdefault('holds', []).append({'tag': tag, 'time': now()})
            jsave(self.p('PAUSE'), {'time': time.time(), 'by': 'tools/stew.py hold_after', 'tag': tag})
        st.save()
        if held:
            self.say(f'HOLD after {tag} (recipe.hold_after): the stew paused itself - read reports/, then remove '
                     f'{self.p("PAUSE")} and run again')
        return held

    def do_phase(self):
        g, ph = self.st.d['round'], self.st.d['phase']
        if ph == 'train':
            self.apply_pending(g)
        return getattr(self, 'phase_' + ph)(g)

    # ---------------------------------------------------------------- 1. train
    def phase_train(self, g):
        out = {}
        for a, b, sn in self.specialists(g):
            if self.st.version(sn, g + 1) is None:
                self.turn(sn, g)
            out[sn] = self.st.version(sn, g + 1).get('train')
        return out

    def farm_gate(self, sn, run):
        fr = self.st.net(sn).get('farm_run')
        if not fr or not self.cfg['code']['farm_gates']:
            return
        out = self.p('farm_gates', os.path.basename(run))
        cfg = jload(os.path.join(run, 'config.json'), {}) or {}
        sm = jload(os.path.join(out, 'summary.json'), {}) or {}
        same = lambda x: os.path.normcase(os.path.normpath(str(x or '').replace('\\', '/')))  # noqa: E731
        if sm.get('passed') and same(sm.get('worker')) == same(cfg.get('pool_worker')):
            return
        self.say(f'{sn}: farm gates (tools/farm_gates.py) on its config and checkpoint 0')
        rc = self.ops.farm_gates(run, os.path.join(run, 'models', 'iter_0000.pt'), out,
                                 self.p('logs', f'farm_gates-{os.path.basename(run)}.log'))
        sm = jload(os.path.join(out, 'summary.json'), {}) or {}
        if rc != 0 or not sm.get('passed'):
            raise Refused(f'{sn}: the farm gates FAILED (exit {rc}; {out}): no farm turn on a build that does not play '
                          f'its settings exactly (AGENTS.md "Farm compatibility")')

    def turn(self, sn, g):
        st = self.st
        net = st.net(sn)
        run = st.abs(net['run'])
        v = st.version(sn, g)
        t = st.d.get('turn')
        if not t or t.get('spec') != sn or t.get('round') != g:
            t = {'spec': sn, 'round': g, 'start': v['iter'], 'target': v['iter'] + self.cfg['recipe']['turn_iters'],
                 'launched': None}
            st.d['turn'] = t
            st.save()
        target = t['target']
        _, alive = self.ops.supervisor(run)
        have = self.ops.newest(run) or 0
        if have < target or alive:
            if self.stop_requested():
                raise Halt('STOP / PAUSE before the turn')
            self.check_supervisors()
            self.yield_machine()
            self.farm_gate(sn, run)
            fr = net.get('farm_run')
            if fr:
                if fr not in st.d['farm_to_pause']:
                    st.d['farm_to_pause'].append(fr)
                    st.save()
                old = self.ops.farm_set(fr, 'active')
                if old is not None and old != 'active':
                    self.say(f'{sn}: farm run {fr} {old} -> active')
            if not alive:
                self.ops.clear_control(run)
                self.say(f'{sn}: turn {t["start"]} -> {target} (newest {have})')
                self.ops.launch(run, ['--iters', str(target)], self.p('logs', 'launch.log'))
                t['launched'] = now()
                st.save()
            self._wait_turn(sn, run, t)
            have = self.ops.newest(run) or 0
        if fr := net.get('farm_run'):
            self._farm_pause(fr)
        if have < target:
            raise Refused(f'{sn}: its supervisor ended at iteration {have}, not {target} (see {run}/supervisor.log)')
        if have <= t['start']:
            raise Refused(f'{sn}: 0 iterations trained in its turn')
        ck = os.path.join(run, 'models', f'iter_{target:04d}.pt')
        st.add_version(sn, {'gen': g + 1, 'iter': target, 'path': st.rel(ck), 'sha256': sha256(ck), 'time': now(),
                            'parents': [f'{sn}@{g}'], 'train': self.train_summary(run, t['start'], target)})
        st.d['turn'] = None
        st.save()
        self.say(f'{sn}: version {g + 1} = iteration {target}')

    def _farm_pause(self, fr):
        st = self.st
        try:
            self.ops.farm_set(fr, 'paused')
        except Exception as e:                 # noqa: BLE001 - loud: it stays listed, retried at release
            raise Refused(f'farm run {fr}: pause FAILED ({e}); it stays listed in state.json farm_to_pause') from e
        if fr in st.d['farm_to_pause']:
            st.d['farm_to_pause'].remove(fr)
            st.save()

    def _wait_turn(self, sn, run, t):
        launched = t.get('launched') or now()
        while True:
            if self.stop_requested():
                self.say(f'{sn}: STOP / PAUSE - pausing it at its iteration boundary')
                self.ops.pause_boundary(run, self.p('logs', 'turns.log'))
                self.ops.write_stop(run)
                for _ in range(120):
                    if not self.ops.supervisor(run)[1]:
                        break
                    time.sleep(min(self.poll_s, 5))
                if fr := self.st.net(sn).get('farm_run'):
                    self._farm_pause(fr)
                raise Halt(f'{sn} paused at iteration {self.ops.newest(run)}')
            sup, alive = self.ops.supervisor(run)
            have = self.ops.newest(run) or 0
            if have >= t['target'] and not alive:
                return
            if not alive and now() - launched > GRACE_S:
                return                         # (the caller says why: short of the target)
            time.sleep(self.poll_s)

    def train_summary(self, run, lo, hi):
        rows = []
        p = os.path.join(run, 'metrics.jsonl')
        if os.path.exists(p):
            for line in open(p, encoding='utf-8'):
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if lo < int(r.get('iter', -1)) <= hi:
                    rows.append(r)
        sp = [r.get('selfplay') or {} for r in rows]
        games = sum(int(x.get('games') or 0) for x in sp)
        samples = sum(int(x.get('samples') or 0) for x in sp)
        secs = sum(float(r.get('seconds') or 0) for r in rows)
        ep = [x['episodes'] for x in sp if isinstance(x.get('episodes'), dict)]
        eg, ec = sum(int(e.get('games') or 0) for e in ep), sum(int(e.get('contests') or 0) for e in ep)
        return {'from': lo, 'to': hi, 'iters': len(rows), 'games': games, 'samples': samples,
                'rows_per_match': round(samples / games, 1) if games else None,
                'games_per_match': round(eg / ec, 3) if ec else None, 'hours': round(secs / 3600, 2)}

    # ---------------------------------------------------------------- 2. distil the deck stews
    def snapshot(self, g):
        """Every specialist's newest `recipe.distill_iters` data iterations of round g's turn, linked into
        rows/gen_<g+1>/spec-A-B/ -> {(a, b): [data dirs]} (recorded: a resumed phase reuses it)."""
        st = self.st
        rec = st.d.setdefault('rows', {}).setdefault(str(g), {})
        out = {}
        for a, b, sn in self.specialists(g):
            k = C.pair_key(a, b)
            if k not in rec or not all(os.path.isdir(st.abs(d)) for d in rec[k]['dirs']):
                run = st.abs(st.net(sn)['run'])
                lo, hi = st.version(sn, g)['iter'], st.version(sn, g + 1)['iter']
                its = S.pick_iters(run, lo, hi, self.cfg['recipe']['distill_iters'])
                if not its:
                    raise Refused(f'{sn}: no complete data iteration in ({lo}, {hi}] to distil from')
                r = S.copy_iters(run, its, self.p('rows', f'gen_{g + 1:03d}', C.spec_dir(a, b)), mode='link')
                r['dirs'] = [st.rel(d) for d in r['dirs']]
                rec[k] = r
                st.save()
            out[(a, b)] = [st.abs(d).replace('\\', '/') for d in rec[k]['dirs']]
        return out

    def phase_distil_decks(self, g):
        snaps = self.snapshot(g)
        out = {}
        for d in self.st.decks():
            name = C.deck_net(d['name'])
            if self.st.version(name, g + 1) is None:
                self.distil(name, g, S.deck_sources(self.st, self.cfg, g, snaps, d['name']))
            out[name] = self.st.version(name, g + 1).get('distill')
        return out

    def sources_path(self, name, g):
        n = 'main' if name == 'main' else C.deck_dir(name.split(':', 1)[1])
        return self.p('sources', f'gen_{g + 1:03d}-{n}.json')

    def distil(self, name, g, doc, note=None):
        st = self.st
        sp = self.sources_path(name, g)
        if jload(sp) != doc:
            jsave(sp, doc)
        ssha = sha256(sp)
        out = self.net_path(name, g + 1)
        side = jload(out + '.stew.json') or {}
        if not (os.path.exists(out) and side.get('sources_sha256') == ssha):
            if os.path.exists(out):
                os.replace(out, out + f'.stale-{int(time.time())}')
            if self.stop_requested():
                raise Halt(f'STOP / PAUSE before distilling {name}')
            self.check_supervisors()
            self.yield_machine()
            self.say(f'{name}: distil v{g + 1} from {len(doc["sources"])} sources ({sp})')
            from .control import Interrupted
            try:
                rc = self.ops.distill(sp, out, self.cfg['recipe']['distill_cli'],
                                      self.p('logs', f'distill-{os.path.basename(sp)[:-5]}.log'),
                                      stop=self.stop_requested)
            except Interrupted:
                raise Halt(f'{name}: distillation stopped (it resumes from its partial file)') from None
            if rc != 0 or not os.path.exists(out):
                raise Refused(f'{name}: tools/distill.py exit {rc}, {"no " if not os.path.exists(out) else ""}{out}')
            jsave(out + '.stew.json', {'sources': st.rel(sp), 'sources_sha256': ssha, 'time': now()})
        info = distill_summary(self.ops.read_extra(out), doc)
        if info.get('sources_sha256') not in (None, ssha):
            raise Refused(f'{name}: {out} records the sources file {info["sources_sha256"][:16]}, not {ssha[:16]} '
                          f'({sp})')
        empty = [s['name'] for s in info.get('per_source') or [] if s.get('selected', s.get('used')) == 0]
        if empty:
            raise Refused(f'{name}: sources with 0 rows {empty} (a source must select rows)')
        st.add_version(name, {'gen': g + 1, 'path': st.rel(out), 'sha256': sha256(out), 'time': now(),
                              'parents': [f'{name}@{g}'], 'sources': st.rel(sp), 'sources_sha256': ssha,
                              'teachers': sorted({s['teacher'].replace('\\', '/') for s in doc['sources']}),
                              'distill': info, 'note': note})
        st.save()

    # ---------------------------------------------------------------- 3. / 5. judge
    def judge_dir(self, g):
        return self.p('judge', f'gen_{g:03d}')

    def phase_judge_deck_gap(self, g):
        return self.judge_phase('judge_deck_gap', g)

    def phase_judge(self, g):
        return self.judge_phase('judge', g)

    def judge_phase(self, phase, g):
        jcfg = self.cfg['recipe']['judge']
        jobs = J.plan_jobs(phase, self.st, g, jcfg, self.pair_spec_json, self.judge_play())
        if not jobs:
            if phase == 'judge' and not jcfg['main_arms']:
                self.write_report(g)
                return {'jobs': 0, 'note': 'judge.main_arms off'}
            raise Refused(f'{phase}@{g}: no judge job (nothing to measure)')
        if self.stop_requested():
            raise Halt(f'STOP / PAUSE before {phase}@{g}')
        self.check_supervisors()
        self.yield_machine()
        from .control import Interrupted
        n = sum(j['games'] for j in jobs)
        self.say(f'{phase}@{g}: {len(jobs)} jobs, {n} matches ({jcfg["backend"]}, {jcfg["device"]}, '
                 f'{jcfg["procs"]} at once)')
        try:
            res = self.ops.judge_jobs(jobs, self.judge_dir(g), jcfg['procs'], stop=self.stop_requested)
        except Interrupted:
            raise Halt(f'{phase}@{g}: judging stopped (finished jobs are kept)') from None
        played = sum(len(r['rows']) for r in res.values())
        if played != n:
            raise Refused(f'{phase}@{g}: {played} matches of {n}')
        self.write_report(g)
        return {'jobs': len(jobs), 'matches': n}

    # ---------------------------------------------------------------- 4. distil the main stew
    def phase_distil_main(self, g):
        if self.st.version('main', g + 1) is None:
            snaps = self.snapshot(g)
            acfg = jload(self.p('archive', 'config.json'))
            doc, note = S.main_sources(self.st, self.cfg, g, snaps, self.p('archive'), acfg)
            if note:
                self.say(f'main: {note}')
            self.distil('main', g, doc, note)
        return self.st.version('main', g + 1).get('distill')

    # ---------------------------------------------------------------- add-deck
    def request_deck(self, name, share=None, alias=None, candidate=False):
        """add-deck: checked now, joins the field at the next generation's start (`apply_pending`)."""
        st = self.st
        decks = st.decks()
        if name in C.deck_names(decks) or any(p['name'] == name for p in st.d['pending']):
            raise Refused(f'add-deck: {name} is in the field already (or pending)')
        C.check_deck(name, self.cfg['recipe']['format'])
        share = share if share is not None else sum(d['share'] for d in decks) / len(decks)
        if not share > 0:
            raise Refused(f'add-deck: share {share!r} must be > 0')
        alias = alias or C.default_alias(name)
        if not C.ALIAS_RE.match(alias) or alias in {d['alias'] for d in decks}:
            raise Refused(f'add-deck: alias {alias!r} (1-16 of [a-z0-9], not another deck\'s: --alias)')
        new_decks = decks + [{'name': name, 'share': share, 'alias': alias}]
        cand = st.candidates() + ([name] if candidate else [])
        for a, b in C.pairings(new_decks, cand):
            C.farm_run(self.cfg['name'], new_decks, a, b)
        req = {'op': 'add-deck', 'name': name, 'share': share, 'alias': alias, 'candidate': bool(candidate),
               'asked': now()}
        return req

    def add_deck(self, name, share=None, alias=None, candidate=False):
        """add-deck: the request recorded in state.json (pending), or - while a run of the stew is live - as a file
        in requests/ that the run takes at its next phase -> the request."""
        req = self.request_deck(name, share, alias, candidate)
        if self.lock_holder() is not None:
            jsave(self.p('requests', f"add-deck-{int(time.time() * 1000)}.json"), req)
            self.say(f'add-deck {name}: requested (the live run takes it; it joins at the next generation)')
        else:
            self.st.d['pending'].append(req)
            self.st.save()
            self.say(f'add-deck {name}: pending (joins at the start of generation {self.st.d["round"]}'
                     f'{"" if self.st.d["phase"] == "train" and not self.st.d.get("turn") else " + 1"})')
        return req

    def apply_pending(self, g):
        """Pending add-decks join at round g's start (before any turn of it): the deck stew = a copy of the current
        main stew (version g), new specialists seeded from it (or seeds.<pairing>)."""
        st = self.st
        self.read_requests()
        if not st.d['pending']:
            return
        started = st.d.get('turn') or any(st.version(sn, g + 1) for _, _, sn in self._pairs_named())
        if started:
            return
        for req in list(st.d['pending']):
            old = C.pairings(st.decks(), st.candidates())
            st.d['field']['decks'].append({'name': req['name'], 'share': req['share'], 'alias': req['alias']})
            if req['candidate']:
                st.d['field']['candidates'].append(req['name'])
            st.save()
            self._new_deck_stew(req['name'], g)
            for a, b in C.pairings(st.decks(), st.candidates()):
                if (a, b) not in old:
                    self._new_specialist(a, b, g)
            st.d['pending'].remove(req)
            st.d.setdefault('field_log', []).append(dict(req, joined=g, time=now()))
            st.save()
            self.say(f"add-deck: {req['name']} joined at generation {g}" + (' (a candidate)' if req['candidate'] else ''))

    def _pairs_named(self):
        return [(a, b, C.spec_name(a, b)) for a, b in C.pairings(self.st.decks(), self.st.candidates())]

    def read_requests(self):
        d = self.p('requests')
        if not os.path.isdir(d):
            return
        for f in sorted(os.listdir(d)):
            if f.endswith('.json'):
                req = jload(os.path.join(d, f))
                if req and not any(p['name'] == req['name'] for p in self.st.d['pending']):
                    self.st.d['pending'].append(req)
                self.st.save()
                os.replace(os.path.join(d, f), os.path.join(d, f + '.taken'))

    # ---------------------------------------------------------------- the loop
    def lock_path(self):
        return self.p('run.lock')

    def lock_holder(self):
        lk = jload(self.lock_path())
        if not lk:
            return None
        from zero import runctl
        return lk if runctl.alive(lk) else None

    def acquire(self):
        h = self.lock_holder()
        if h is not None and h.get('pid') != os.getpid():
            raise Refused(f"the stew is being run already (pid {h.get('pid')}, {self.lock_path()})")
        import psutil
        jsave(self.lock_path(), {'pid': os.getpid(), 'started': psutil.Process().create_time(), 'time': time.time()})

    def release_lock(self):
        lk = jload(self.lock_path())
        if lk and lk.get('pid') == os.getpid():
            os.remove(self.lock_path())

    def run(self, max_phases=None, only=None):
        """The loop (or one phase: `only` = its name). -> 'held' | 'paused' | 'stopped' | 'done' | 'step'."""
        if os.path.exists(self.p('PAUSE')):
            pz = jload(self.p('PAUSE'), {}) or {}
            raise Refused(f"the stew is paused ({pz.get('by', 'PAUSE')}{', ' + pz['tag'] if pz.get('tag') else ''}): "
                          f"remove {self.p('PAUSE')} to continue")
        if os.path.exists(self.p('STOP')):
            os.replace(self.p('STOP'), self.p('STOP.old'))
        self.acquire()
        how = 'done'
        try:
            if self.st.d.get('held'):
                self.st.d['held'] = None
                self.st.save()
            self.check_supervisors()
            n = 0
            while True:
                if only is not None and self.st.d['phase'] != only:
                    raise Refused(f"step {only}: the stew's next phase is {self.st.tag()} (phases go in order: "
                                  f"{', '.join(C.PHASES)})")
                if self.stop_requested():
                    how = 'paused' if os.path.exists(self.p('PAUSE')) else 'stopped'
                    break
                start = now()
                tag = self.st.tag()
                self.say(f'phase {tag}')
                result = self.do_phase()
                held = self.advance(start, result)
                n += 1
                if held:
                    how = 'held'
                    break
                if only is not None:
                    how = 'step'
                    break
                if max_phases is not None and n >= max_phases:
                    break
        except Halt as e:
            self.say(f'halt: {e}')
            how = 'paused' if os.path.exists(self.p('PAUSE')) else 'stopped'
        except BaseException as e:
            self.st.d.setdefault('errors', []).append({'tag': self.st.tag(), 'time': now(), 'error': repr(e)[:500]})
            self.st.save()
            self.say(f'FAILED in {self.st.tag()}: {e!r}')
            raise
        finally:
            try:
                self.release_machine()
            finally:
                self.release_lock()
        return how

    # ---------------------------------------------------------------- reports
    def write_report(self, g):
        st = self.st
        res = {}
        jd = self.judge_dir(g)
        if os.path.isdir(jd):
            for f in os.listdir(jd):
                if f.endswith('.json') and not f.endswith('.job.json'):
                    r = jload(os.path.join(jd, f))
                    if isinstance(r, dict) and 'rows' in r:
                        res[r['id']] = r
        rep = {'name': self.cfg['name'], 'round': g, 'phase': st.tag(), 'written': stamp(),
               'held': bool(st.d.get('held')), 'field': st.d['field'],
               'nets': {k: {'v%d' % v['gen']: {'path': v['path'], 'sha256': v['sha256'][:16]}
                            for v in n['versions'] if v['gen'] in (g, g + 1)} for k, n in st.d['nets'].items()},
               'training': {}, 'distill': {}, 'judge': J.summarize(st, g, res) if res else {}, 'notes': []}
        for k, n in st.d['nets'].items():
            v = st.version(k, g + 1)
            if v and v.get('train'):
                rep['training'][k] = v['train']
            if v and v.get('distill'):
                rep['distill'][k] = v['distill']
            if v and v.get('note'):
                rep['notes'].append(f'{k}: {v["note"]}')
            for nt in n.get('notes') or []:
                rep['notes'].append(f'{k}: {nt}')
        rep['notes'] = sorted(set(rep['notes']))
        jsave(self.p('reports', f'gen_{g:03d}.json'), rep)
        text_save(self.p('reports', f'gen_{g:03d}.md'), J.report_md(rep))
        return rep

    # ---------------------------------------------------------------- status and plan
    def status(self):
        st = self.st
        L = [f"stew {self.cfg['name']} ({self.dir}): generation {st.d['round']}, next phase {st.tag()}"
             + (f"; HELD after {st.d['held']['tag']}" if st.d.get('held') else '')]
        for f in ('PAUSE', 'STOP'):
            if os.path.exists(self.p(f)):
                L.append(f'  {f} file present')
        h = self.lock_holder()
        L.append(f"  run: {'LIVE, pid ' + str(h['pid']) if h else 'not running'}")
        L.append('  field: ' + ', '.join(f"{d['name']} ({d['share']}{', candidate' if d['name'] in st.candidates() else ''})"
                                        for d in st.decks()))
        for p in st.d['pending']:
            L.append(f"  pending: add-deck {p['name']} (joins at the next generation)")
        for k, n in st.d['nets'].items():
            v = st.latest(k)
            line = f"  {k}: v{v['gen']} {v['path']}"
            if k.startswith('spec:'):
                run = st.abs(n['run'])
                sup, alive = self.ops.supervisor(run)
                line += f" (run newest {self.ops.newest(run)}, supervisor {sup.get('state')!r}{' LIVE' if alive else ''})"
            L.append(line)
        if st.d.get('turn'):
            L.append(f"  turn: {st.d['turn']}")
        y = st.d.get('yield') or {}
        if y.get('runs') or y.get('farm'):
            L.append(f"  yielded: {y}")
        if st.d.get('farm_to_pause'):
            L.append(f"  farm runs to pause: {st.d['farm_to_pause']}")
        if st.d.get('errors'):
            L.append(f"  last error: {st.d['errors'][-1]}")
        reps = sorted(os.listdir(self.p('reports'))) if os.path.isdir(self.p('reports')) else []
        if reps:
            L.append(f"  last report: {self.p('reports', reps[-1])}")
        return '\n'.join(L)

    def plan(self):
        """What the next phase does, with the exact commands (nothing is run or written)."""
        st = self.st
        g, ph = st.d['round'], st.d['phase']
        L = [f'next: {st.tag()}' + (' (then HOLD: recipe.hold_after)' if st.tag() in self.cfg['recipe']['hold_after']
                                    else '')]
        L += [f'requires: {r}' for r in self.cfg.get('requires') or []]
        if self.cfg['yield_runs']:
            L.append('yield first: ' + '; '.join(
                f"python tools/pause_run.py {y['run']} --wait + farm runs {y['farm_runs']} -> paused"
                for y in self.cfg['yield_runs']))
        if ph == 'train':
            if st.d['pending']:
                L.append(f"add-decks joining now: {[p['name'] for p in st.d['pending']]} (their deck stews = copies of "
                         f"main v{g}; new specialists forked from it)")
            for a, b in C.pairings(st.decks(), st.candidates()):
                sn = C.spec_name(a, b)
                v = st.version(sn, g)
                if v is None or st.version(sn, g + 1) is not None:
                    continue
                run = st.abs(st.net(sn)['run'])
                t = st.d.get('turn') or {}
                target = t['target'] if t.get('spec') == sn else v['iter'] + self.cfg['recipe']['turn_iters']
                fr = st.net(sn).get('farm_run')
                L.append(f'{sn}: iterations {v["iter"]} -> {target}' + (f'; farm run {fr} active during the turn' if fr
                                                                        else ''))
                L.append('  ' + self.ops.cmdline(self.ops.launch_cmd(run, ['--iters', str(target)])))
        elif ph in ('distil_decks', 'distil_main'):
            names = ['main'] if ph == 'distil_main' else [C.deck_net(d['name']) for d in st.decks()]
            for name in names:
                if st.version(name, g + 1) is not None:
                    continue
                sp = self.sources_path(name, g)
                L.append(f'{name} v{g + 1}: {self.ops.cmdline(self.ops.distill_cmd(sp, self.net_path(name, g + 1), self.cfg["recipe"]["distill_cli"]))}')
                try:
                    if ph == 'distil_main':
                        doc, note = S.main_sources(st, self.cfg, g, self._plan_snaps(g), self.p('archive'),
                                                   jload(self.p('archive', 'config.json')))
                        if note:
                            L.append(f'  note: {note}')
                    else:
                        doc = S.deck_sources(st, self.cfg, g, self._plan_snaps(g), name.split(':', 1)[1])
                    for s in doc['sources']:
                        L.append(f"  {s['name']}: teacher {s['teacher']}, select {json.dumps(s['select'])}, weight "
                                 f"{s['weight']}, data {len(s['data'])} dirs")
                except (KeyError, RuntimeError) as e:
                    L.append(f'  (sources after the earlier phases: {e})')
        else:
            jcfg = self.cfg['recipe']['judge']
            try:
                jobs = J.plan_jobs(ph, st, g, jcfg, self.pair_spec_json, {'seat': '(the specialists\' evaluation seat)'})
            except KeyError as e:
                L.append(f'  (jobs after the earlier phases: {e})')
                jobs = []
            n = sum(j['games'] for j in jobs)
            kinds = {}
            for j in jobs:
                kinds[j['id'].split('.')[0]] = kinds.get(j['id'].split('.')[0], 0) + j['games']
            L.append(f"{len(jobs)} jobs, {n} best-of-three matches {kinds}; ~{n * jcfg['est_s_per_match'] / 3600 / jcfg['procs']:.1f} "
                     f"h at {jcfg['est_s_per_match']} s a match, {jcfg['procs']} at once ({jcfg['device']})")
            if jobs:
                L.append('  each: ' + self.ops.cmdline(self.ops.judge_cmd(f"judge/gen_{g:03d}/<id>.job.json",
                                                                        f"judge/gen_{g:03d}/<id>.json")))
        return '\n'.join(L)

    def _plan_snaps(self, g):
        rec = (self.st.d.get('rows') or {}).get(str(g)) or {}
        out = {}
        for a, b, sn in self._pairs_named():
            k = C.pair_key(a, b)
            out[(a, b)] = [self.st.abs(d) for d in rec[k]['dirs']] if k in rec else [f'(rows/gen_{g + 1:03d}/{C.spec_dir(a, b)})']
        return out


def distill_summary(extra, doc):
    """The distillation record of a checkpoint's `extra` (zero.distill's --sources record, zero-core docs/az.md 76:
    `distill` {sources_sha256, sources: [{name, teacher_sha256, rows, selected, used, ...}], step, heldout: [{step,
    policy_kl, top1, value_mse, value_mae, margin_mse, sources: {name: agreement}}]}) for the report: the steps and
    each source's weight, rows and last held-out agreement."""
    d = extra.get('distill') if isinstance(extra.get('distill'), dict) else {}
    w = {s['name']: s['weight'] for s in doc['sources']}
    held = [h for h in d.get('heldout') or [] if isinstance(h, dict)]
    last = held[-1] if held else {}
    per = []
    for s in d.get('sources') or []:
        if not isinstance(s, dict):
            continue
        row = {'name': s.get('name'), 'weight': w.get(s.get('name'))}
        for k in ('teacher_sha256', 'rows', 'selected', 'used', 'heldout_rows'):
            if k in s:
                row[k] = s[k]
        ag = (last.get('sources') or {}).get(s.get('name')) or {}
        for k in ('policy_kl', 'top1', 'value_mae', 'value_mse', 'margin_mse'):
            if k in ag:
                row[k] = ag[k]
        per.append(row)
    out = {'sources': len(doc['sources']), 'steps': d.get('step'), 'per_source': per,
           'sources_sha256': d.get('sources_sha256'), 'train_rows': d.get('train_rows'),
           'seconds': d.get('seconds')}
    if last:
        out['heldout'] = {k: last[k] for k in ('policy_kl', 'top1', 'value_mse', 'value_mae', 'margin_mse')
                          if k in last}
    return out
