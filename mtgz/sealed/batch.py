"""Batched paired races (docs/ENGINE.md 110): every pool's and every build's games in one process.

A race of `mtgz.sealed.compare` plays each candidate's block of games in its own zero-core driver (one
driver per candidate and block, its seed and game ids fixing the games' streams: ENGINE.md 104.1). Run
one process per pool and each process evaluates its drivers' small batches alone. Here the same drivers
- created exactly as `Comparison._run` creates them, so every game has the seed, spec, seat, opponent and
pilot streams it has in a per-process run - run side by side in one process (`MultiDrive`): every
driver's ready batch is collected and the network answers all of them in one call. A pool's race is
its own (`RaceState`: rounds, the sequential tests, its drops); only the network's calls are shared.

**The answers must not depend on the batch.** The network's usual forward (zero-core
`TokenNet.forward`) packs a batch by its data - rows sorted by token count, chunks of 64 rows padded
to their longest row - and CPU and GPU kernels round differently at other matrix shapes (measured:
priors and values differ at ~1e-6 between a batch alone and merged with others), so a game's moves
could depend on what else is in flight. `RowExact` evaluates every row in a fixed shape: its tokens
padded to its class T = ceil(n / 16) * 16 (n its tokens), the rows of one class `rows` at a time
(the trunk on [rows, T], a short chunk filled with copies of its first row): a row's answer is a
function of the row alone (and of `rows`, the device and its kernels). A batched race with `RowExact`
is then the per-process race with `RowExact` (`--eval rowexact`), bit for bit, whatever the other
pools and builds. `--eval batchwise` evaluates each driver's batch alone with the usual evaluator
instead: the per-process race of today's default, bit for bit, without the big batches.

    python -m mtgz.sealed.compare evalpick ... --pool-seed 2026,2027,2028 --race --batched
"""
import json
import os
import sys
import time

import numpy as np

T_STEP = 16                      # a row's tokens are padded to a multiple of this (zero.infer's T_STEP)


def say(log, msg):
    if log is not None:
        log(msg)


# ----------------------------------------------------------------------------- the row-exact evaluator

class RowExact:
    """A token network's answers row by row (module docs): each row's tokens padded to its class T (a
    multiple of 16), the rows of one class evaluated `rows` at a time (default 512 on CUDA, 16 on the
    CPU) on [rows, T] (a short chunk filled with copies of its first row, whose answers are dropped).
    On CUDA `TokenNet.forward_static` (every slot embedded) through zero-core's `TokenGraphRunner`, one
    CUDA graph per (rows, T) (`rows` a multiple of 64; `graphs` None = on unless ZERO_CUDA_GRAPHS=0, eager
    `forward_static` then: ~30x slower). On the CPU
    (`_forward_packed`) the same trunk and heads, only the present slots embedded (in fixed pieces of
    TOK_ROWS tokens), every kernel on one thread (`torch.set_num_threads(1)`, process-wide: on several,
    MKL splits a product over its threads by shape and a row's answer depends on its place in the
    chunk - measured), the chunks side by side on `workers` threads. The answers are the usual
    forward's up to float32 rounding (other matrix shapes), and a row's answer never depends on the
    batch it came in. Root belief rows (a pilot with a belief tilt) are refused: this evaluator
    computes no auxiliary head."""

    def __init__(self, model, device='cpu', rows=None, graphs=None, workers=None):
        import torch
        from zero.net import TokenNet
        if not isinstance(model, TokenNet):
            raise TypeError(f'RowExact: a token network (zero.net.TokenNet), not {type(model).__name__}')
        self.device = torch.device(device)
        # (default: 512 rows on CUDA - fewer, bigger graph launches: measured 39k rows/s against 27k at 256 and 13k at
        # 64 on the shared RTX 5090; 16 on the CPU, where a short chunk's padding costs)
        rows = (512 if self.device.type == 'cuda' else 16) if rows is None else rows
        if int(rows) < 1:
            raise ValueError(f'RowExact: rows {rows} (at least 1)')
        self.model = model.to(self.device).eval()
        self.rows = int(rows)
        self.pool = None
        self.workers = 1
        if self.device.type == 'cpu':
            # one thread per kernel: MKL splits a matrix product over its threads by shape, so with several a
            # row's answer depends on its place in the chunk (measured: Linear [64, 544] -> 128 on 4 threads,
            # 8 of 64 rows differ when the rows are permuted; none on 1). Chunks run side by side instead,
            # on `workers` threads (torch releases the GIL): the same answers whatever their number
            torch.set_num_threads(1)
            self.workers = max(1, int(workers if workers is not None else min(8, _cpus())))
            if self.workers > 1:
                from concurrent.futures import ThreadPoolExecutor
                self.pool = ThreadPoolExecutor(self.workers, thread_name_prefix='rowexact')
        self.F = model.features
        self.A = model.config['actions']
        self.S = model.seats
        self.pidx = np.asarray(model.present_index(), np.int64)
        self.n_tok = 1 + len(self.pidx)
        has = self.pidx >= 0
        self.flag_col = np.full(self.F, -1, np.int64)
        self.flag_col[self.pidx[has]] = 1 + np.flatnonzero(has)
        self.always = 1 + np.flatnonzero(~has)
        self.runner = None
        if graphs is None:
            graphs = self.device.type == 'cuda' and os.environ.get('ZERO_CUDA_GRAPHS', '1') != '0'
        if graphs:
            from zero.infer import ROW_STEP, TokenGraphRunner
            if self.device.type != 'cuda':
                raise ValueError('RowExact: CUDA graphs need a CUDA device')
            if self.rows % ROW_STEP:
                raise ValueError(f'RowExact: rows {self.rows} with CUDA graphs must be a multiple of {ROW_STEP}')
            self.runner = TokenGraphRunner(self.model)
        self.calls = self.leaves = self.chunks = self.pad_rows = 0
        self.seconds = 0.0

    def describe(self):
        out = {'evaluator': 'rowexact', 'rows': self.rows, 'graphs': self.runner is not None,
               'device': str(self.device), 'token_step': T_STEP}
        if self.device.type == 'cpu':
            out.update(kernel_threads=1, workers=self.workers)
        return out

    # ---- the rows' tokens
    def _presence_sparse(self, xs_off, xs_idx, xs_val):
        B = len(xs_off) - 1
        P = np.zeros((B, self.n_tok), bool)
        P[:, 0] = True
        P[:, self.always] = True
        rows = np.repeat(np.arange(B), np.diff(xs_off))
        col = self.flag_col[np.asarray(xs_idx, np.int64)]
        hit = (col >= 0) & (np.asarray(xs_val) > 0.5)
        P[rows[hit], col[hit]] = True
        return P

    def _presence_dense(self, x):
        from zero.infer import token_presence
        return token_presence(x, self.pidx)

    # ---- the answers
    def __call__(self, lb):
        if lb.sparse:
            return self.evaluate_sparse(lb.xs_off, lb.xs_idx, lb.xs_val, lb.n_features, lb.n_players, lb.opt_off,
                                        lb.opt_action, lb.a.get('extra'))
        return self.evaluate_dense(lb.x, lb.n_players, lb.opt_off, lb.opt_action, lb.a.get('extra'))

    def evaluate_dense(self, x, n_players, opt_off, opt_action, extra=None):
        self._check(x.shape[1], extra)
        return self._evaluate(self._presence_dense(x), x, None, n_players, opt_off, opt_action)

    def evaluate_sparse(self, xs_off, xs_idx, xs_val, n_features, n_players, opt_off, opt_action, extra=None):
        self._check(int(n_features), extra)
        xs_off = np.asarray(xs_off, np.int64)
        sp = (xs_off, np.asarray(xs_idx, np.int64), np.asarray(xs_val, np.float32))
        return self._evaluate(self._presence_sparse(*sp), None, sp, n_players, opt_off, opt_action)

    def _check(self, width, extra):
        if width != self.F:
            from zero.net import EncodingMismatch
            raise EncodingMismatch(f'a leaf batch of {width:,} features per row for a network of {self.F:,}: the network '
                                   f'was built for another encoding than its driver feeds it')
        from zero.protocol import belief_rows
        if len(belief_rows(extra)):
            raise ValueError('RowExact: root belief rows (a pilot with a belief tilt): this evaluator answers no '
                             'auxiliary head (use --eval default or batchwise)')

    def _evaluate(self, P, x, sp, n_players, opt_off, opt_action):
        from zero.infer import segment_softmax_np
        from zero.protocol import MAX_SEATS
        t0 = time.perf_counter()
        n_players = np.asarray(n_players, np.int64)
        opt_off = np.asarray(opt_off, np.int64)
        opt_action = np.asarray(opt_action, np.int64)
        B = P.shape[0]
        cnt = P.sum(1)
        T = -(-cnt // T_STEP) * T_STEP
        nopt = np.diff(opt_off)
        pri = np.zeros(int(opt_off[-1]), np.float32)
        value = np.zeros((B, MAX_SEATS), np.float32)
        margin = np.zeros((B, MAX_SEATS), np.float32)
        R = self.rows
        plan = []
        for t in np.unique(T):
            idx = np.flatnonzero(T == t)
            for c0 in range(0, len(idx), R):
                ch = idx[c0:c0 + R]
                rows = np.concatenate([ch, np.full(R - len(ch), ch[0])]) if len(ch) < R else ch
                self.chunks += 1
                self.pad_rows += R - len(ch)
                plan.append((ch, rows, int(t)))

        def run(c):
            ch, rows, t = c
            return ch, self._launch(rows, len(ch), t, P, cnt, x, sp, n_players, opt_off, opt_action, nopt)
        if self.pool is not None and len(plan) > 1:
            for p in self.pool.map(run, plan):
                self._collect(p, pri, value, margin, opt_off, nopt, segment_softmax_np)
        else:
            pend = []
            for c in plan:
                pend.append(run(c))
                if self.runner is not None and len(pend) >= 4:     # (graphs: a few launches in flight)
                    self._collect(pend.pop(0), pri, value, margin, opt_off, nopt, segment_softmax_np)
            for p in pend:
                self._collect(p, pri, value, margin, opt_off, nopt, segment_softmax_np)
        value[np.arange(MAX_SEATS)[None, :] >= n_players[:, None]] = 0.0
        self.calls += 1
        self.leaves += B
        self.seconds += time.perf_counter() - t0
        return pri, value, margin

    def _chunk_opts(self, ch, opt_off, opt_action, nopt):
        """The options of rows `ch` (in that order): their offsets from 0 and their actions."""
        k = nopt[ch]
        off = np.zeros(len(ch) + 1, np.int64)
        np.cumsum(k, out=off[1:])
        pos = np.repeat(opt_off[ch], k) + (np.arange(off[-1]) - np.repeat(off[:-1], k))
        return off, opt_action[pos], pos

    @staticmethod
    def _rows_sparse(sp, rows):
        """Rows `rows` (in that order, repeats allowed) of sparse rows `sp` -> their (xs_off, xs_idx, xs_val)."""
        xs_off, xs_idx, xs_val = sp
        a = xs_off[rows]
        k = xs_off[rows + 1] - a
        o = np.zeros(len(rows) + 1, np.int64)
        np.cumsum(k, out=o[1:])
        g = np.repeat(a, k) + (np.arange(o[-1]) - np.repeat(o[:-1], k))
        return o, xs_idx[g], xs_val[g]

    def _launch(self, rows, real, t, P, cnt, x, sp, n_players, opt_off, opt_action, nopt):
        """One chunk: rows `rows` (its first `real` are answered), all of token class `t`."""
        import torch
        from zero.infer import token_index
        R = len(rows)
        ch = rows[:real]
        off, acts, pos = self._chunk_opts(ch, opt_off, opt_action, nopt)
        if self.runner is not None:
            xs_off, xs_idx, xs_val = self._rows_sparse(sp, rows) if sp is not None else _sparse_of(x, rows)
            # (the chunk's padding rows answer no options: their answers are dropped)
            off_all = np.concatenate([off, np.full(R - real, off[-1])]).astype(np.int32)
            h = self.runner.launch_sparse(xs_off, xs_idx, xs_val, n_players[rows].astype(np.int64), off_all,
                                          acts.astype(np.int32))
            return ('g', h, pos)
        # (the rows in a buffer of torch's own: its alignment the same for every chunk)
        xt = torch.zeros((R, self.F), dtype=torch.float32)
        xc = xt.numpy()
        if sp is not None:
            o, idx, val = self._rows_sparse(sp, rows)
            xc[np.repeat(np.arange(R), np.diff(o)), idx] = val
        else:
            xc[:] = x[rows, :self.F]
        full = np.zeros((R, t), np.int64)
        token_index(P[rows], full)
        dev = self.device
        with torch.inference_mode():
            args = (xt.to(dev), torch.from_numpy(n_players[rows]).to(dev), torch.from_numpy(full).to(dev),
                    torch.from_numpy(cnt[rows].astype(np.int64)).to(dev))
            if dev.type == 'cpu':
                plog, vlog, mar = self._forward_packed(*args, P[rows])
            else:
                plog, vlog, mar = self.model.forward_static(*args)
            lg = plog[:real].float().cpu().numpy()
            val = torch.softmax(vlog[:real].float(), 1).cpu().numpy()
            mg = mar[:real].float().cpu().numpy()
        r = np.repeat(np.arange(real), np.diff(off))
        return ('e', (lg[r, acts], off, val, mg), pos)

    def _forward_packed(self, x, n_players, full, ntok, P):
        """`TokenNet.forward_static` (the CPU's form): the same trunk and heads on [R, T], the tokens built from the
        present slots only - each segment's present slots embedded in fixed pieces of TOK_ROWS tokens (a token's
        embedding depends on it alone), not every slot of the layout (170 in MTG Zero's v5 layout, ~10-40
        present). `P` [R, 1 + slots]: the rows' tokens (numpy)."""
        import torch
        from zero.net import _read
        m = self.model
        x = x[:, :m.features]
        B, T = full.shape
        rraw = ent = None
        if m.readers is not None:
            rraw, ent = _read(m, x)
        g = m._global(x, ent)
        h = x.new_zeros(B, T, m.d)
        h[:, 0] = g + m.pos[0]
        idt = torch.full((B, T), m.vocab, dtype=torch.long) if m.n_id else None
        cpos = np.cumsum(P, 1) - 1                                   # a present token's packed position
        start = 1
        for s, lin in zip(m.segs, m.seg_in):
            o, k, w = s['offset'], s['count'], s['width']
            bi, si = np.nonzero(P[:, start:start + k])               # row-major: each row's tokens in layout order
            if len(bi):
                bt, st = torch.from_numpy(bi), torch.from_numpy(si)
                rows = x[:, o:o + k * w].reshape(B, k, w)[bt, st]     # [N, w] (a new tensor)
                t = _lin_pieces(lin, rows) + m.pos[start + st]
                i = None
                if s['id'] is not None:
                    i = m._identity(rows, s['id'])
                    t = t + m.emb(i)
                pi = torch.from_numpy(cpos[bi, start + si])
                h[bt, pi] = t
                if idt is not None and i is not None:
                    idt[bt, pi] = i
            start += k
        z, pol = m._trunk(h, ntok, full, idt)
        return m._heads(z, pol, n_players, False, None, rraw)

    def _collect(self, p, pri, value, margin, opt_off, nopt, segment_softmax_np):
        ch, (kind, h, pos) = p
        S = self.S
        if kind == 'g':
            pr, v, m = self.runner.result(h)[:3]
            value[ch, :S] = v[:len(ch), :S]
            margin[ch, :S] = m[:len(ch), :S]
            pri[pos] = pr
            return
        lg, off, val, mg = h
        pri[pos] = segment_softmax_np(lg, off)
        value[ch, :S] = val[:, :S]
        margin[ch, :S] = mg[:, :S]


def _cpus():
    """The logical CPUs this process may run on (its affinity: `--cpus`)."""
    try:
        import psutil
        return len(psutil.Process().cpu_affinity())
    except (ImportError, AttributeError, OSError):
        return os.cpu_count() or 1


TOK_ROWS = 64                    # the CPU's token embeddings: pieces of this many tokens


def _lin_pieces(lin, rows):
    """`lin(rows)` computed TOK_ROWS rows at a time, the last piece padded with zero rows, each piece a buffer of its
    own: a row's output depends on it alone (one shape, one alignment: on one thread, a row's place in the piece
    changes nothing - measured)."""
    import torch
    N, w = rows.shape
    out = []
    for a in range(0, N, TOK_ROWS):
        piece = torch.zeros((TOK_ROWS, w), dtype=rows.dtype)
        piece[:min(TOK_ROWS, N - a)] = rows[a:a + TOK_ROWS]
        out.append(lin(piece))
    return torch.cat(out)[:N]


def _sparse_of(x, rows):
    """Dense rows -> their non-zeros (xs_off, xs_idx, xs_val)."""
    xr = x[rows]
    r, c = np.nonzero(xr)
    off = np.zeros(len(rows) + 1, np.int64)
    np.cumsum(np.bincount(r, minlength=len(rows)), out=off[1:])
    return off, c.astype(np.int64), xr[r, c].astype(np.float32)


# ----------------------------------------------------------------------------- many drivers, one evaluator

class Job:
    """One zero-core driver of a comparison (candidate i's games `ks` of block b), its finished games'
    rows collected as they come (`rows`: tag -> result row)."""
    __slots__ = ('drv', 'n', 'rows', 'error', 'done', 'owner', 'i')

    def __init__(self, drv, n, owner, i):
        self.drv, self.n, self.owner, self.i = drv, n, owner, i
        self.rows, self.error, self.done = {}, None, False


class MultiDrive:
    """Many drivers answered by their evaluators (module docs; `evaluators` {key: evaluator}, the drivers'
    network keys). `mode` 'merged': every ready batch of every driver goes into one call per key of
    `evaluate_sparse` (a `RowExact`: answers per row); 'each': each batch alone through `evaluator(lb)` (the
    usual `NetEvaluator`: the per-process answers bit for bit). `step()` polls every driver, answers what is
    ready, and ends the drivers whose games are all played (`Job.done`) or which failed (`Job.error`, the
    driver's exception: an engine error in one of its games names its queued setup)."""

    def __init__(self, evaluators, mode='merged', log=None):
        if mode not in ('merged', 'each'):
            raise ValueError(f'MultiDrive mode {mode!r}')
        if mode == 'merged':
            bad = [k for k, e in evaluators.items() if not hasattr(e, 'evaluate_sparse')]
            if bad:
                raise ValueError(f'MultiDrive merged: the evaluators of {bad} answer no merged batches (RowExact does)')
        self.evs, self.mode, self.log = dict(evaluators), mode, log
        self.jobs = []
        self.games = 0                    # games finished
        self.calls = self.rows = self.batches = 0
        self.eval_seconds = 0.0
        self.max_rows = 0

    def add(self, job):
        self.jobs.append(job)

    def in_flight(self):
        return sum(j.n - len(j.rows) for j in self.jobs)

    def _end(self, j, err=None):
        j.error = err
        j.done = True
        self.jobs.remove(j)
        try:
            j.drv.end_phase()
        except Exception:                   # noqa: BLE001  (a failed driver: nothing to end)
            pass
        j.drv = None

    def _take(self, j):
        for r in j.drv.take_results():
            j.rows[int(r['tag'])] = r
            self.games += 1

    def step(self):
        """One pass: poll, answer, collect. -> True if any batch was answered or any driver ended."""
        ready = []
        ended = False
        for j in list(self.jobs):
            try:
                while True:
                    lb = j.drv.poll_batch()
                    if lb is None:
                        break
                    if lb.external:
                        raise RuntimeError('an external batch in a race driver')
                    ready.append((j, lb))
                self._take(j)
                if not ready or ready[-1][0] is not j:
                    if j.drv.paused and not j.drv.queued and not j.drv.in_flight()['games']:
                        self._take(j)
                        self._end(j)
                        ended = True
            except RuntimeError as e:
                ready = [(jj, lb) for jj, lb in ready if jj is not j]
                self._end(j, e)
                ended = True
        if not ready:
            return ended
        t0 = time.perf_counter()
        if self.mode == 'each':
            answers = [self.evs[lb.key](lb) for _, lb in ready]
            self.calls += len(ready)
        else:
            answers = [None] * len(ready)
            for key in sorted({lb.key for _, lb in ready}):
                at = [n for n, (_, lb) in enumerate(ready) if lb.key == key]
                for n, ans in zip(at, self._merged(self.evs[key], [ready[n][1] for n in at])):
                    answers[n] = ans
                self.calls += 1
        self.eval_seconds += time.perf_counter() - t0
        self.batches += len(ready)
        n = sum(lb.B for _, lb in ready)
        self.rows += n
        self.max_rows = max(self.max_rows, n)
        for (j, lb), ans in zip(ready, answers):
            if j.done:
                continue
            try:
                j.drv.submit(lb.id, *ans)
            except RuntimeError as e:
                self._end(j, e)
        return True

    @staticmethod
    def _merged(ev, lbs):
        """Every batch in one call of `ev` (sparse batches: the non-zeros concatenated), split back."""
        xo, xi, xv, npl, oo, oa, ex = [np.zeros(1, np.int64)], [], [], [], [np.zeros(1, np.int64)], [], []
        nz = no = 0
        for lb in lbs:
            if not lb.sparse:
                raise ValueError('MultiDrive merged: sparse batches only (drivers made with sparse=True)')
            o = np.asarray(lb.xs_off, np.int64)
            xo.append(o[1:] + nz)
            nz += int(o[-1])
            xi.append(lb.xs_idx)
            xv.append(lb.xs_val)
            npl.append(lb.n_players)
            f = np.asarray(lb.opt_off, np.int64)
            oo.append(f[1:] + no)
            no += int(f[-1])
            oa.append(lb.opt_action)
            ex.append(lb.a['extra'])
        pri, value, margin = ev.evaluate_sparse(np.concatenate(xo), np.concatenate(xi), np.concatenate(xv),
                                                lbs[0].n_features, np.concatenate(npl), np.concatenate(oo),
                                                np.concatenate(oa), np.concatenate(ex))[:3]
        out, r0, p0 = [], 0, 0
        for lb in lbs:
            b, k = lb.B, int(lb.opt_off[-1])
            out.append((np.ascontiguousarray(pri[p0:p0 + k]), np.ascontiguousarray(value[r0:r0 + b]),
                        np.ascontiguousarray(margin[r0:r0 + b])))
            r0 += b
            p0 += k
        return out

    def stats(self):
        return {'games': self.games, 'calls': self.calls, 'batches': self.batches, 'rows': self.rows,
                'rows_per_call': round(self.rows / max(1, self.calls), 1), 'max_rows': self.max_rows,
                'eval_seconds': round(self.eval_seconds, 1)}


# ----------------------------------------------------------------------------- races in one process

class BlockPlay:
    """Block b of a comparison for its candidates `alive`, every candidate's driver in the MultiDrive at
    once. When all have ended: the games recorded (`Comparison.record_block`), or - an engine error in
    one of them - the first failing candidate in `alive` order recorded (`Comparison.record_failure`, as
    `play_block` records it) and the block played again without that game, every candidate."""

    def __init__(self, comp, b, alive, md, threads, t0):
        self.comp, self.b, self.alive, self.md, self.threads, self.t0 = comp, b, list(alive), md, threads, t0
        self.ks = comp.block_games(b)
        self.done = False
        self._start()

    def _start(self):
        self.jobs = {}
        for i in self.alive:
            drv = self.comp.driver(i, self.b, self.ks, threads=self.threads, sparse=True)
            j = Job(drv, len(self.ks), self, i)
            self.jobs[i] = j
            self.md.add(j)

    def poll(self):
        """-> True once the block is recorded."""
        if self.done or not all(j.done for j in self.jobs.values()):
            return self.done
        for i in self.alive:
            e = self.jobs[i].error
            if e is not None:
                k = self.comp.record_failure(i, self.b, self.ks, e)
                self.ks = [x for x in self.ks if x != k]
                self._start()
                return False
        self.comp.record_block(self.b, self.alive, self.ks, {i: j.rows for i, j in self.jobs.items()})
        self.comp.seconds = time.time() - self.t0
        self.done = True
        return True


class PoolRace:
    """One comparison's race (`compare.RaceState`) played round by round in the shared MultiDrive: a
    round's blocks all at once, its tests when they are all recorded."""

    def __init__(self, label, comp, state, md, threads, t0, log=None):
        self.label, self.comp, self.state, self.md, self.threads, self.t0 = label, comp, state, md, threads, t0
        self.log = log
        self.blocks = []
        self.out = None
        self._next_round()

    def _next_round(self):
        self.blocks = [BlockPlay(self.comp, b, self.state.alive, self.md, self.threads, self.t0)
                       for b in self.state.round_blocks()]

    def poll(self):
        """-> True once the race has ended (`out`: its result)."""
        if self.out is not None:
            return True
        if not all(bp.poll() for bp in self.blocks):
            return False
        row = self.state.end_round()
        say(self.log, '# round ' + json.dumps(dict({'pool': self.label}, **row)))
        if self.state.stop is None:
            self._next_round()
            return False
        self.out = self.state.result()
        return True

    def progress(self):
        st = self.state
        return {'stage': st.stage_name(), 'alive': len(st.alive), 'games': len(self.comp.scores[st.alive[0]]),
                'in_round': sum(len(bp.ks) for bp in self.blocks if not bp.done)}


def run_races(entries, evaluators, mode='merged', threads=1, log=None, progress=60.0, on_done=None):
    """Race every comparison of `entries` [(label, comp, state)] at once in one MultiDrive (`state`: a
    `compare.RaceState`, or `compare.FixedState` for a fixed number of games) -> ({label: its result},
    the batched run's statistics). `evaluators`: {key: evaluator} of the comparisons' pilots; `threads`:
    each driver's search threads; `on_done(label, out)` as each race ends; a progress line on `log`
    every `progress` seconds."""
    md = MultiDrive(evaluators, mode=mode, log=log)
    t0 = time.time()
    races = [PoolRace(label, comp, st, md, threads, t0, log) for label, comp, st in entries]
    outs = {}
    last = time.time()
    g_last = 0
    while len(outs) < len(races):
        busy = md.step()
        for r in races:
            if r.label not in outs and r.poll():
                outs[r.label] = r.out
                say(log, '# race ' + json.dumps({'pool': r.label, 'winner': r.out.get('winner'),
                                                 'stop': r.out.get('stop', r.state.stop),
                                                 'games': r.out.get('games', len(r.comp.scores[0])),
                                                 'seconds': round(time.time() - t0, 1)}))
                if on_done is not None:
                    on_done(r.label, r.out)
        now = time.time()
        if log is not None and progress and now - last >= progress:
            st = md.stats()
            say(log, '# progress ' + json.dumps({
                'seconds': round(now - t0, 1), 'games': md.games,
                'games_per_s': round((md.games - g_last) / (now - last), 3),
                'in_flight': md.in_flight(), 'drivers': len(md.jobs), 'rows': st['rows'],
                'rows_per_call': st['rows_per_call'], 'eval_seconds': st['eval_seconds'], 'rss_gb': _rss_gb(),
                'pools': {r.label: r.progress() for r in races if r.label not in outs}}))
            last, g_last = now, md.games
        if not busy:
            time.sleep(0.0005)
    st = md.stats()
    wall = time.time() - t0
    st.update(seconds=round(wall, 1), games_per_s=round(md.games / max(wall, 1e-9), 4), mode=mode,
              driver_threads=threads)
    return outs, st


def _rss_gb():
    try:
        import psutil
        return round(psutil.Process().memory_info().rss / 2 ** 30, 2)
    except (ImportError, OSError):
        return None


def limit_cpus(n):
    """Run this process on its first `n` logical CPUs (psutil's affinity; threads included) -> the CPUs."""
    import psutil
    p = psutil.Process()
    cpus = sorted(p.cpu_affinity())[:int(n)]
    p.cpu_affinity(cpus)
    return cpus


def line_buffered():
    """stderr flushed at every line (progress in a log file as it happens, whatever launched the process)."""
    try:
        sys.stderr.reconfigure(line_buffering=True)
    except (AttributeError, ValueError):
        pass
