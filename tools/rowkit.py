"""Network rows for the analysis tools (docs/ENGINE.md 125; zero-core docs/search.md 2.15): on a layout v7 build
a row of the game `mtg` is a head, a tail of tokens and pointer overrides; on a layout v5 / v6 build (an older
engine and its zero-core, e.g. a run's pinned checkout through a tool's --engine) it is the head alone, and every
helper here takes the call of before. A tool never drops a row's tail or overrides: the evaluator gets them
(`tail=` / `ov=`), or the game has none.

* `kind(game)`: the active engine's `zero.mcts.Rows` of the game, None without variable-length rows.
* `row(g, viewer, rk, opts=False)` -> (x, rr): `viewer`'s row of game `g` - x its head (f32), rr its (tail,
  overrides) or None; `opts`: the decider's own row, its overrides against `g.legal()`.
* `simulate(t, g, c_puct, chance_k=0, root_view=False, rk=None, margin_w=0.0)` -> None or (player, x, acts, vx,
  rr, vrr): a Rust tree's leaf (`SearchTree.simulate`, with `rows=True` where the game has them), its value row
  `vx` (root view, else None) and both rows' (tail, overrides).
* `kw(rk, rrs)`: `NetEvaluator.evaluate` / `.aux` / `.reader`'s `tail=` / `ov=` for a batch of rows (`rrs`: each
  row's rr from `row` or `simulate`), {} without.
* `evaluate(ev, xs, acts, rk=None, rrs=None, extra=None)`: `ev.evaluate` of rows `xs` with options `acts` (one
  list per row; [] for a value-only or belief row), every row of two players.
"""
import numpy as np


def kind(game='mtg'):
    """The game's variable-length rows (`zero.mcts.Rows`) on the active engine, None for an engine or game without
    them (the info of an older engine has no 'tails' key)."""
    from zero import games as G
    info = G.info(game)
    if not (info.get('tails') or info.get('option_pointers')):
        return None
    from zero.mcts import rows_of
    return rows_of(info)


_NO_OV = (np.zeros(0, np.int32), np.zeros(0, np.int64))


def row(g, viewer, rk, opts=False):
    """`viewer`'s row of `g` -> (x f32 [F], rr): rr (tail, overrides) as `zero.mcts.leaf_row` gives them, None for a
    game without variable-length rows (`rk` None: `encode`, the call of before)."""
    if rk is None:
        return np.asarray(g.encode(viewer), np.float32), None
    from zero.mcts import leaf_row
    x, t, ov = leaf_row(g, viewer, rk, opts)
    return np.asarray(x, np.float32), (t, ov)


def simulate(t, g, c_puct, chance_k=0, root_view=False, rk=None, margin_w=0.0):
    """One simulation of Rust tree `t` -> None (no leaf) or (player, x, acts, vx, rr, vrr): vx the viewer's value-only
    row (root view; None otherwise), rr / vrr the leaf row's and the value row's (tail, overrides) (None without
    variable-length rows; a value row's overrides are empty)."""
    if rk is not None:
        r = t.simulate(g, c_puct, chance_k, margin_w, root_view=root_view, rows=True)
        if r is None:
            return None
        player, x, acts, vx, rows = r
        tail, ov, vtail = rows if rows is not None else (None, None, None)
        vrr = None
        if vx is not None:
            vrr = (vtail, _NO_OV if rk.pointers else None)
        return player, x, acts, vx, (tail, ov), vrr
    if root_view:
        r = t.simulate(g, c_puct, chance_k, margin_w, root_view=True)
        if r is None:
            return None
        player, x, acts, vx = r
        return player, x, acts, vx, None, None
    r = t.simulate(g, c_puct, chance_k, margin_w)
    if r is None:
        return None
    player, x, acts = r
    return player, x, acts, None, None, None


def kw(rk, rrs):
    """The evaluator's variable-length arguments for rows whose (tail, overrides) are `rrs` ({} without)."""
    if rk is None:
        return {}
    from zero.play import row_arrays
    from zero.rows import Overrides, Tail, eval_kw
    ra = row_arrays([rr if rr is not None else (None, None) for rr in rrs], rk)
    return eval_kw(Tail.from_arrays(ra, rk.widths) if rk.widths else None, Overrides.from_arrays(ra) if rk.pointers else None)


def evaluate(ev, xs, acts, rk=None, rrs=None, extra=None):
    """`ev.evaluate` of rows `xs` (heads) with options `acts` (a list per row), two players each; `rrs` the rows'
    (tail, overrides) (`rk` None: none) -> what the evaluator returns (priors flat by `acts`, values, margins, ...)."""
    acts = [list(a) for a in acts]
    off = np.zeros(len(acts) + 1, np.int32)
    off[1:] = np.cumsum([len(a) for a in acts])
    flat = np.asarray([a for r in acts for a in r], np.int32)
    x = np.ascontiguousarray(np.stack([np.asarray(r, np.float32) for r in xs]))
    return ev.evaluate(x, np.full(len(acts), 2, np.int32), off, flat, extra, **kw(rk, rrs if rrs is not None else [None] * len(acts)))


def sample_kw(rk, s, rows, options=True):
    """The evaluator's `tail=` / `ov=` for sample rows `rows` of sample dict `s` (a run's or a driver's: `t_off` ... /
    `ov_off` ...; {} without): their overrides when they are evaluated with their own options (`options`), none
    when as value-only rows."""
    if rk is None or not any(k in s for k in ('t_off', 'ov_off')):
        return {}
    from zero.rows import Overrides, Tail, eval_kw
    rows = np.asarray(rows, np.int64)
    tail = Tail.from_arrays(s, rk.widths).take(rows) if rk.widths else None
    ov = None
    if rk.pointers:
        ov = Overrides.from_arrays(s).take(rows) if options else Overrides.empty(len(rows))
    return eval_kw(tail, ov)


def train_kw(model, s, rows, x, device='cpu'):
    """A token network's forward `tail=` / `ov=` for sample rows `rows` of `s` evaluated with their own options (the
    overrides resolved against them, `zero.rows.resolve`; `x` the rows' heads, f32 [B, F]) - as a run's window gives
    them (`zero.data.Window.row_inputs`); {} for a network without tails or pointers."""
    from zero import rows as R
    if not (getattr(model, 'tails', None) or getattr(model, 'option_pointers', False)):
        return {}
    lay = model.config['layout']
    rows = np.asarray(rows, np.int64)
    off = np.asarray(s['opt_off'], np.int64)
    lens = off[rows + 1] - off[rows]
    acts = np.concatenate([np.zeros(0, np.int64)] + [np.asarray(s['opt_action'][off[r]:off[r + 1]], np.int64) for r in rows])
    boff = np.concatenate([np.zeros(1, np.int64), np.cumsum(lens)])
    widths = R.layout_tails(lay)
    tail = R.Tail.from_arrays(s, widths).take(rows) if model.tails else None
    out = {}
    if tail is not None:
        out['tail'] = tail.to_torch(device)
    if model.option_pointers:
        ov = R.Overrides.from_arrays(s).take(rows)
        out['ov'] = R.ov_torch(R.resolve(tail, ov, boff, acts, lay, present=R.presence(np.asarray(x), lay)), device)
    return out


def aux(ev, xs, rk=None, rrs=None, batch=256):
    """`ev.aux` (the auxiliary head) of rows `xs` (heads; two players each) with their tails (`rrs`; `rk` None: none),
    in batches -> {name: float32 [len(xs), width]}."""
    out = {}
    for a in range(0, len(xs), batch):
        part = xs[a:a + batch]
        k = kw(rk, (rrs if rrs is not None else [None] * len(xs))[a:a + batch])
        x = np.ascontiguousarray(np.stack([np.asarray(r, np.float32) for r in part]))
        res = ev.aux(x, np.full(len(part), 2), **({'tail': k['tail']} if 'tail' in k else {}))
        for name, v in res.items():
            out.setdefault(name, []).append(v)
    return {name: np.concatenate(v) for name, v in out.items()}
