"""The stew's real operations (docs/ENGINE.md 239.2): everything that starts a process, touches a run's control files
or calls the farm coordinator. The orchestrator talks to an `Ops`; tests give it a fake with the same methods.

Every process runs from the stew's code (`code.worktree`, default this checkout; `code.python`, default this
interpreter) with the stew's environment (`recipe.env` over this process's, CUDA_VISIBLE_DEVICES dropped first as
tools/ab.py does): runs never train from a checkout being edited (AGENTS.md "Live runs").

* runs: tools/fork_run.py <seed> <run> (a NEW run whose iteration 0 is the seed), the run's config resolved as
  zero.run resolves it, fork_run's `check_heads`, tools/launch_hidden.py + tools/supervise.py <run> --iters N
  (hidden, detached), tools/pause_run.py <run> --wait (the iteration boundary) / --resume, zero.runctl's states and
  live supervisors;
* the farm coordinator (zero.farm_def.Client, the admin token): a farm run's state paused / active (the whole body
  as GET returns it with only `state` changed, alternates' GET-only `problems` dropped: tools/ab.py farm_pause);
  tools/farm_gates.py and the worker's check-config on a specialist's config and checkpoint (tools/ab.py farm_gates);
* distill: tools/distill.py --sources FILE OUT.pt [recipe.distill_cli] (--resume when OUT.pt.partial exists);
* the judge: `python -m mtgz.stew.judge play JOB OUT`, `judge.procs` at once; with `judge.backend` 'farm' one
  runner process, `python -m mtgz.stew.judge_farm run PLAN` (farm match campaigns, docs/ENGINE.md 249).

A long process is polled; `stop()` true (the stew's STOP or PAUSE) kills it with its tree and raises Interrupted -
distill resumes from its partial file, a judge job is played again.
"""
import json
import shutil
import os
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


class Interrupted(Exception):
    """The stew's STOP or PAUSE arrived while a process ran (the process was stopped)."""


class OpsError(RuntimeError):
    pass


class FarmRefused(OpsError):
    """The farm cannot play a phase's judge jobs exactly (the runner's reason); nothing was put."""


def kill_tree(pid):
    try:
        import psutil
        p = psutil.Process(pid)
        kids = p.children(recursive=True)
        for k in kids:
            try:
                k.kill()
            except psutil.Error:
                pass
        p.kill()
    except Exception:                          # noqa: BLE001 - gone already
        pass


class Ops:
    def __init__(self, cfg, stew_dir, log=print):
        code = cfg['code']
        self.wt = os.path.abspath(code['worktree'] or ROOT)
        self.py = code['python'] or sys.executable
        self.extra_env = dict(cfg['recipe']['env'])
        self.dir = stew_dir
        self.log = log
        self._client = None

    # ---------------------------------------------------------------- processes
    def env(self):
        e = dict(os.environ)
        e.pop('CUDA_VISIBLE_DEVICES', None)
        e.update(self.extra_env)
        e.setdefault('PYTHONIOENCODING', 'utf-8')
        return e

    def check_code(self, sha=None):
        """The stew's checkout at `sha` with its engine built -> its HEAD."""
        head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=self.wt, text=True).strip()
        if sha and not head.startswith(sha):
            raise OpsError(f'{self.wt} is at {head[:12]}, not {sha} (the stew\'s pinned code)')
        mt = os.path.join(self.wt, 'mtgz')
        if not any(f.startswith('_engine') and f.endswith(('.pyd', '.so')) for f in os.listdir(mt)):
            raise OpsError(f'{self.wt}: no mtgz/_engine build (bash tools/build_py.sh there)')
        return head

    def cmdline(self, cmd):
        return subprocess.list2cmdline(cmd)

    def run(self, cmd, log_path=None, check=True, timeout=None):
        """A short process in the stew's code -> CompletedProcess (its output in `log_path` when given)."""
        if log_path:
            os.makedirs(os.path.dirname(os.path.abspath(log_path)), exist_ok=True)
            with open(log_path, 'a', encoding='utf-8') as f:
                f.write(f'\n# {time.strftime("%Y-%m-%d %H:%M:%S")} {self.cmdline(cmd)}\n')
                f.flush()
                r = subprocess.run(cmd, cwd=self.wt, env=self.env(), stdin=subprocess.DEVNULL, stdout=f,
                                   stderr=subprocess.STDOUT, timeout=timeout)
        else:
            r = subprocess.run(cmd, cwd=self.wt, env=self.env(), stdin=subprocess.DEVNULL, capture_output=True,
                               text=True, timeout=timeout)
        if check and r.returncode != 0:
            raise OpsError(f'exit {r.returncode}: {self.cmdline(cmd)}' + (f' (log {log_path})' if log_path else
                                                                           f': {(r.stdout or "")[-400:]}{(r.stderr or "")[-400:]}'))
        return r

    def run_long(self, cmd, log_path, stop=None, poll=2.0):
        """A long process (distill, a judge job), polled: -> its exit status; Interrupted when `stop()` turned true
        (the process tree killed first)."""
        os.makedirs(os.path.dirname(os.path.abspath(log_path)), exist_ok=True)
        with open(log_path, 'a', encoding='utf-8') as f:
            f.write(f'\n# {time.strftime("%Y-%m-%d %H:%M:%S")} {self.cmdline(cmd)}\n')
            f.flush()
            kw = {}
            if os.name == 'nt':
                kw['creationflags'] = getattr(subprocess, 'CREATE_NO_WINDOW', 0) | getattr(
                    subprocess, 'BELOW_NORMAL_PRIORITY_CLASS', 0)
            p = subprocess.Popen(cmd, cwd=self.wt, env=self.env(), stdin=subprocess.DEVNULL, stdout=f,
                                 stderr=subprocess.STDOUT, **kw)
            while True:
                try:
                    return p.wait(timeout=poll)
                except subprocess.TimeoutExpired:
                    pass
                if stop is not None and stop():
                    kill_tree(p.pid)
                    p.wait()
                    raise Interrupted(self.cmdline(cmd))

    # ---------------------------------------------------------------- runs
    def fork_new_run(self, seed, run_dir, log_path, game=None):
        """tools/fork_run.py <seed> <run> [--game G]: a NEW run whose iteration 0 is the seed widened onto game G's
        layout (exact on rows without the new tokens; an older layout's network - layout v7's `mtg_v7` - moves onto
        the current one, docs/ENGINE.md 236)."""
        self.run([self.py, 'tools/fork_run.py', seed, run_dir] + (['--game', game] if game else []), log_path)

    def widen(self, ckpt, dst, game, log_path):
        """Network `ckpt` on game `game`'s layout as the file `dst`: fork_run's new-run path into `dst`.fork (kept,
        the provenance) and its iteration 0 copied to `dst` - the stew's gen-0 main stew from a source of an older
        layout (the meta3 run's v7 checkpoint onto v8: the user, 2026-11-22)."""
        fork = dst + '.fork'
        ck0 = os.path.join(fork, 'models', 'iter_0000.pt')
        if not os.path.exists(ck0):
            self.fork_new_run(ckpt, fork, log_path, game)
        tmp = dst + '.part'
        shutil.copyfile(ck0, tmp)
        os.replace(tmp, dst)

    def resolve(self, run_dir, saved):
        """The config a run of `saved` resolves to at its start (zero.run: DEFAULTS < preset < saved < flags; no flags)."""
        import mtgz.az  # noqa: F401
        from zero import run as R
        return R.resolve_config(R.parse(['--run', run_dir]), dict(saved))

    def check_spec(self, game, spec):
        """The engine parses the spec (a game made from it): its refusal (an option this build does not have, e.g.
        dealt decks in a metagame before they were built) raised now, not at the run's first start."""
        import mtgz.az  # noqa: F401
        from zero import games as G
        G.engine().Game(game, G.spec_json(spec), 2, 0)

    def check_heads(self, ckpt, cfg):
        """fork_run's check_heads of the seed against the specialist's settings: net.HeadMissing reported, never
        patched around (a root belief needs the auxiliary head of the game's targets)."""
        import mtgz.az  # noqa: F401
        from zero.net import load
        from zero.tools.fork_run import check_heads
        m, _ = load(ckpt, game=cfg.get('game'))
        check_heads(m, cfg, cfg.get('game'), ckpt)

    def launch(self, run_dir, args, log_path):
        """The run's supervisor, hidden and detached (tools/launch_hidden.py), returns at once."""
        cmd = [self.py, 'tools/launch_hidden.py', os.path.join(run_dir, 'supervisor_console.txt'), self.py,
               'tools/supervise.py', run_dir] + list(args)
        self.run(cmd, log_path)
        return cmd

    def launch_cmd(self, run_dir, args):
        return [self.py, 'tools/launch_hidden.py', os.path.join(run_dir, 'supervisor_console.txt'), self.py,
                'tools/supervise.py', run_dir] + list(args)

    def supervisor(self, run_dir):
        """(supervisor_state.json, alive) of a run."""
        import mtgz  # noqa: F401
        from zero import runctl
        st = runctl.read_state(run_dir, 'supervisor') or {}
        return st, runctl.alive(st)

    def newest(self, run_dir):
        d = os.path.join(run_dir, 'models')
        its = [int(f[5:-3]) for f in os.listdir(d) if f.startswith('iter_') and f.endswith('.pt') and f[5:-3].isdigit()
               ] if os.path.isdir(d) else []
        return max(its) if its else None

    def run_status(self, run_dir):
        import mtgz  # noqa: F401
        from zero import runctl
        return runctl.run_status(run_dir)

    def pause_boundary(self, run_dir, log_path, timeout=3600):
        """tools/pause_run.py <run> --wait (PAUSE_NEXT: the current iteration finishes) -> the run's state after."""
        self.run([self.py, 'tools/pause_run.py', run_dir, '--wait', '--timeout', str(timeout)], log_path,
                 timeout=timeout + 600)
        return self.run_status(run_dir)['state']

    def resume(self, run_dir, log_path):
        self.run([self.py, 'tools/pause_run.py', run_dir, '--resume'], log_path)
        return self.run_status(run_dir)['state']

    def clear_control(self, run_dir):
        """A specialist's leftover PAUSE / PAUSE_NEXT / STOP (the stew's own: written when it stopped a turn)."""
        import mtgz  # noqa: F401
        from zero import runctl
        runctl.request_resume(run_dir)
        p = os.path.join(run_dir, runctl.STOP)
        if os.path.exists(p):
            os.replace(p, p + '.old')

    def write_stop(self, run_dir):
        with open(os.path.join(run_dir, 'STOP'), 'w', encoding='utf-8') as f:
            f.write(json.dumps({'time': time.time(), 'by': 'tools/stew.py'}))

    def live_supervisors(self):
        """zero.runctl.live_supervisors() with each one's working directory (a relative run path is its)."""
        import mtgz  # noqa: F401
        import psutil
        from zero import runctl
        out = []
        for s in runctl.live_supervisors():
            try:
                s = dict(s, cwd=psutil.Process(int(s['pid'])).cwd()) if s.get('pid') else dict(s)
            except Exception:                  # noqa: BLE001 - gone, or not ours to read
                s = dict(s)
            out.append(s)
        return out

    # ---------------------------------------------------------------- the farm coordinator
    def client(self):
        if self._client is None:
            import mtgz  # noqa: F401
            from zero import farm_def as FD
            self._client = FD.Client()
        return self._client

    def farm_state(self, name):
        """A farm run's state on the coordinator, or None when it does not exist (yet: created by the pool's first
        PUT)."""
        r = self.client().get_or_none(f'/v1/admin/run/{name}')
        return None if r is None else r.get('state')

    def farm_set(self, name, state):
        """A farm run's state -> 'paused' | 'active' (the whole body as GET returns it, only `state` changed) -> its
        state before, or None when the run does not exist."""
        cl = self.client()
        body = cl.get_or_none(f'/v1/admin/run/{name}')
        if body is None:
            return None
        old = body.get('state')
        if old == state:
            return old
        from mtgz.az.farm_admin import put_body      # (GET's read-only notes cut: PUT refuses them)
        cl.call('PUT', f'/v1/admin/run/{name}', body=put_body(body, state=state))
        return old

    def farm_gates(self, run_dir, ckpt, out_dir, log_path):
        """The worker's check-config of the run's settings, then tools/farm_gates.py on its config and `ckpt` with its
        pool_worker -> exit status (0: passed); summary.json in `out_dir` (tools/ab.py farm_gates)."""
        cfg = json.load(open(os.path.join(run_dir, 'config.json'), encoding='utf-8'))
        w = str(cfg.get('pool_worker') or '')
        os.makedirs(out_dir, exist_ok=True)
        p = subprocess.run([w, 'check-config', os.path.join(run_dir, 'config.json'), '--json'], capture_output=True,
                           text=True, stdin=subprocess.DEVNULL)
        try:
            failures = json.loads(p.stdout).get('failures')
        except ValueError:
            failures = [f'check-config exit {p.returncode}: {(p.stdout + p.stderr)[-300:]}']
        from .state import jsave
        jsave(os.path.join(out_dir, 'check_config.json'), {'worker': w, 'failures': failures})
        if failures:
            self.log(f'farm gates: the worker refuses the settings of {run_dir}: {failures}')
            return 3
        cmd = [self.py, 'tools/farm_gates.py', '--config', os.path.join(run_dir, 'config.json'), '--checkpoint', ckpt,
               '--worker', w, '--out', out_dir]
        return self.run(cmd, log_path, check=False).returncode

    # ---------------------------------------------------------------- distill and the judge
    def distill_cmd(self, sources, out, cli):
        cmd = [self.py, 'tools/distill.py', '--sources', sources, out] + list(cli)
        if os.path.exists(out + '.partial') and '--resume' not in cmd:
            cmd.append('--resume')
        return cmd

    def distill(self, sources, out, cli, log_path, stop=None):
        return self.run_long(self.distill_cmd(sources, out, cli), log_path, stop)

    def read_extra(self, ckpt):
        """A checkpoint's `extra` (the distillation's record)."""
        import torch
        d = torch.load(ckpt, map_location='cpu', weights_only=False)
        return d.get('extra') or {} if isinstance(d, dict) else {}

    def judge_cmd(self, job_path, out_path):
        return [self.py, '-m', 'mtgz.stew.judge', 'play', job_path, out_path]

    def judge_farm_cmd(self, plan_path):
        return [self.py, '-m', 'mtgz.stew.judge_farm', 'run', plan_path]

    def judge_farm(self, plan, stop=None):
        """A phase's judge jobs as farm campaigns (mtgz.stew.judge_farm, docs/ENGINE.md 249): the runner process on
        `plan` (written to <judge dir>/farm/<phase>.plan.json) -> {id: result}. FarmRefused when the farm cannot play
        the jobs exactly (the runner's exit 2: nothing was put), OpsError when a campaign failed, Interrupted on
        `stop()` - the runner is killed, its campaigns stay on the coordinator and the next call resumes them."""
        from . import judge_farm as JF
        from .state import jload, jsave
        jdir = plan['dir']
        fdir = os.path.join(jdir, 'farm')
        pp = os.path.join(fdir, f"{plan.get('phase') or 'judge'}.plan.json")
        lp = os.path.join(fdir, f"{plan.get('phase') or 'judge'}.log")
        jsave(pp, plan)
        rc = self.run_long(self.judge_farm_cmd(pp), lp, stop, poll=2.0)
        if rc != 0:
            try:
                with open(lp, encoding='utf-8', errors='replace') as f:
                    tail = [x.strip() for x in f.read().splitlines() if x.strip()][-1][-900:]
            except (OSError, IndexError):
                tail = ''
            if rc == 2:
                raise FarmRefused(tail[len('REFUSED: '):] if tail.startswith('REFUSED: ') else tail)
            raise OpsError(f'judge on the farm: exit {rc} (log {lp}): {tail}')
        out = {j['id']: jload(os.path.join(jdir, j['id'] + '.json')) for j in plan['jobs']}
        bad = [j['id'] for j in plan['jobs'] if not JF.valid_result(j, out[j['id']])]
        if bad:
            raise OpsError(f'judge on the farm: jobs without a valid farm result: {bad}')
        return out

    def judge_jobs(self, jobs, jdir, procs, stop=None):
        """Every job without a valid result in `jdir` played (`procs` processes at once) -> {id: result}; a failed job
        raises OpsError after the others finished; Interrupted on `stop()`."""
        from . import judge as J
        from .state import jload, jsave
        os.makedirs(jdir, exist_ok=True)
        todo = []
        for j in jobs:
            out = os.path.join(jdir, j['id'] + '.json')
            if not J.valid_result(j, jload(out)):
                todo.append(j)
        errs, lock, halt = [], threading.Lock(), threading.Event()

        def one(j):
            jp = os.path.join(jdir, j['id'] + '.job.json')
            out = os.path.join(jdir, j['id'] + '.json')
            jsave(jp, j)
            try:
                rc = self.run_long(self.judge_cmd(jp, out), os.path.join(jdir, j['id'] + '.log'),
                                   lambda: halt.is_set() or (stop is not None and stop()))
            except Interrupted:
                halt.set()
                return
            except Exception as e:             # noqa: BLE001 - the job failed to run: said below
                with lock:
                    errs.append(f"{j['id']}: {e!r}")
                return
            if rc != 0 or not J.valid_result(j, jload(out)):
                with lock:
                    errs.append(f"{j['id']}: exit {rc} (log {j['id']}.log)")
        it = iter(todo)

        def worker():
            while not halt.is_set():
                with lock:
                    j = next(it, None)
                if j is None:
                    return
                self.log(f"judge: {j['id']} ({j['games']} matches): {j['label']}")
                one(j)
        ths = [threading.Thread(target=worker) for _ in range(max(1, int(procs)))]
        for t in ths:
            t.start()
        for t in ths:
            t.join()
        if halt.is_set() and stop is not None and stop():
            raise Interrupted('judge')
        if errs:
            raise OpsError('judge jobs failed: ' + '; '.join(errs))
        out = {j['id']: jload(os.path.join(jdir, j['id'] + '.json')) for j in jobs}
        bad = [i for i, j in zip(out, jobs) if not J.valid_result(j, out[i])]
        if bad:
            raise OpsError(f'judge jobs without a valid result: {bad}')
        return out
