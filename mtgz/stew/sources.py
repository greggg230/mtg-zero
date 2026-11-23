"""The stew's distillation sources files (docs/ENGINE.md 239.1 is the contract, 239.2 how the stew fills it).

* **A deck stew X** (round g): warm start = its version g; one source per specialist whose pairing holds X - teacher
  = that specialist's version g + 1 (its checkpoint after the round's turn), data = the turn's newest shards
  (`snapshot`), select {"actor": X} (the rows where X's pilot acts), weight = the opponent deck's field share (the
  mirror's: X's own).
* **The main stew** (round g): warm start = its version g; per specialist {A, B} and acting deck Z of it, a source on
  the same shards with select {"actor": Z} and teacher = deck stew Z's version g + 1, weight = the cell's field weight
  share(Z) x share(other); plus **retention**: teacher = the main stew's version g, data = the archive of the source
  run's window (taken at init), select {"not_pairing": [...]} = the rows of the field's uncovered cells (no specialist)
  - weight `config.retention_share` (the batches' share; the others scaled to the rest).

Weights are relative (zero.distill normalizes them over the sources). Paths are absolute: the sources files live in
the stew's directory, never in the repo.
"""
import json
import os
import shutil

from . import config as C


def run_config_of(dirs):
    """The run config.json beside the data of a snapshot's iteration directories (<x>/data/iter_N -> <x>/config.json):
    the row selector's context (zero-core docs/az.md 76: a source's "config")."""
    return os.path.join(os.path.dirname(os.path.dirname(dirs[0])), 'config.json').replace('\\', '/')


def sources_doc(init, sources, train, heldout='auto'):
    return {'student': {'init': init}, 'sources': sources, 'heldout': heldout, 'train': dict(train)}


def deck_sources(st, cfg, g, snaps, x):
    """Deck stew `x`'s sources file of round `g` (dict). `snaps`: {(a, b): [data dir, ...]} of the round."""
    decks = st.decks()
    sh = C.shares(decks)
    pairs = C.pairings(decks, st.candidates())
    out = []
    for a, b in pairs:
        if x not in (a, b):
            continue
        if (a, b) not in snaps:
            raise RuntimeError(f'deck stew {x}: no shards of specialist {a}|{b} in round {g}')
        opp = b if a == x else a
        out.append({'name': C.spec_name(a, b), 'teacher': st.ckpt(C.spec_name(a, b), g + 1),
                    'data': list(snaps[(a, b)]), 'config': run_config_of(snaps[(a, b)]), 'select': {'actor': x},
                    'weight': round(sh[opp], 6)})
    if not out:
        raise RuntimeError(f'deck stew {x}: no specialist of round {g} plays it (candidates {st.candidates()})')
    return sources_doc(st.ckpt(C.deck_net(x), g), out, cfg['recipe']['distill'])


def archive_decks(archive_cfg):
    """The deck names of the archive's run spec (a metagame's), or None (a spec without named decks)."""
    try:
        spec = json.loads(archive_cfg.get('spec') or '{}')
    except ValueError:
        return None
    m = spec.get('metagame') if isinstance(spec, dict) else None
    if not isinstance(m, dict):
        return None
    return [d.get('name') for d in m.get('decks') or []]


def retention_select(decks, pairs, arch):
    """The retention source's selector over the archive (whose run's decks are `arch`): every unordered pairing of
    the archive's decks that is NOT an uncovered cell of the field (covered by a specialist, or with a deck outside
    the field) excluded -> ({"not_pairing": [...]}, [the served uncovered cells]) or (None, []) when it would select
    nothing."""
    want = set()
    for x, y in C.uncovered_cells(decks, pairs):
        if x in arch and y in arch:
            want.add(tuple(sorted((x, y))))
    if not want:
        return None, []
    excl = []
    for i, x in enumerate(arch):
        for y in arch[i:]:
            if tuple(sorted((x, y))) not in want:
                excl.append([x, y])
    served = [c for c in C.uncovered_cells(decks, pairs) if tuple(sorted(c)) in want]
    return ({'not_pairing': excl} if excl else {}), served


def main_sources(st, cfg, g, snaps, archive_dir, archive_cfg):
    """The main stew's sources file of round `g` (dict) and a note on the retention source."""
    decks = st.decks()
    sh = C.shares(decks)
    pairs = C.pairings(decks, st.candidates())
    spec_rows = []
    for a, b in pairs:
        if (a, b) not in snaps:
            raise RuntimeError(f'main stew: no shards of specialist {a}|{b} in round {g}')
        for z in ([a] if a == b else [a, b]):
            spec_rows.append({'name': f'main:{a}|{b}@{z}', 'teacher': st.ckpt(C.deck_net(z), g + 1),
                              'data': list(snaps[(a, b)]), 'config': run_config_of(snaps[(a, b)]),
                              'select': {'actor': z}, 'w': C.cell_weight(sh, a, b)})
    cov = sum(s['w'] for s in spec_rows)
    arch = archive_decks(archive_cfg) if archive_cfg is not None else None
    unc = C.uncovered_cells(decks, pairs)
    note = None
    sel, served = (None, [])
    if unc:
        if arch is None:
            if cfg['recipe']['retention_share'] not in (0, 0.0):
                raise RuntimeError('main stew: the field has uncovered cells but the archive\'s run spec names no '
                                   'decks (not a metagame): no retention rows can be selected (retention_share 0 '
                                   'to go without)')
        else:
            sel, served = retention_select(decks, pairs, arch)
    r = C.retention_share(cfg, decks, pairs, served=(lambda x, y: (x, y) in served)) if sel is not None else 0.0
    unserved = [c for c in unc if c not in served]
    if unserved:
        note = (f'uncovered cells without retention rows (their decks are not in the archive): '
                f'{[C.pair_key(*c) for c in unserved]}')
    out = []
    for s in spec_rows:
        w = s.pop('w')
        s['weight'] = round((1.0 - r) * w / cov, 6)
        out.append(s)
    if sel is not None and r > 0:
        out.append({'name': 'retention', 'teacher': st.ckpt('main', g),
                    'data': [os.path.join(archive_dir, 'data').replace('\\', '/')],
                    'config': os.path.join(archive_dir, 'config.json').replace('\\', '/'), 'select': sel,
                    'weight': round(r, 6)})
    return sources_doc(st.ckpt('main', g), out, cfg['recipe']['distill']), note


# ------------------------------------------------------------------ the shards a round distils from
def iter_dirs(run):
    d = os.path.join(run, 'data')
    out = {}
    if os.path.isdir(d):
        for f in os.listdir(d):
            if f.startswith('iter_') and f[5:].isdigit():
                out[int(f[5:])] = os.path.join(d, f)
    return out


def pick_iters(run, lo, hi, n):
    """The newest `n` complete data iterations of `run` in (lo, hi] (a DONE.json in the directory) -> [iter]."""
    its = sorted((i for i, p in iter_dirs(run).items() if lo < i <= hi and os.path.exists(os.path.join(p, 'DONE.json'))),
                 reverse=True)
    return sorted(its[:n])


def _place(src, dst, mode):
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    if os.path.exists(dst):
        if os.path.getsize(dst) == os.path.getsize(src):
            return False
        os.remove(dst)
    if mode == 'link':
        try:
            os.link(src, dst)
            return True
        except OSError:
            pass
    tmp = dst + '.part'
    shutil.copy2(src, tmp)
    os.replace(tmp, dst)
    return True


def copy_iters(run, its, dest, mode='link'):
    """`run`'s data iterations `its` into dest/data/iter_NNNN/ (its shards and DONE.json; hard links when `mode`
    'link' and the volume allows, else copies) and its config.json into dest/ (the row selector reads the run's
    config.json next to its data) -> {'iters', 'shards', 'rows'?, 'dirs'}. A run's later pruning cannot take the rows
    away from a phase that has not used them yet. RuntimeError when an iteration has no shard."""
    from zero.data import shard_paths
    dirs, n = [], 0
    for i in its:
        src = os.path.join(run, 'data', f'iter_{i:04d}')
        shards = shard_paths(src)
        if not shards:
            raise RuntimeError(f'{src}: no shards')
        dd = os.path.join(dest, 'data', f'iter_{i:04d}')
        for p in shards + [os.path.join(src, 'DONE.json')]:
            if os.path.exists(p):
                _place(p, os.path.join(dd, os.path.basename(p)), mode)
        n += len(shards)
        dirs.append(dd)
    os.makedirs(dest, exist_ok=True)
    _place(os.path.join(run, 'config.json'), os.path.join(dest, 'config.json'), 'copy')
    return {'iters': list(its), 'shards': n, 'dirs': [d.replace('\\', '/') for d in dirs]}


def rows_of(dirs):
    from zero.data import shard_paths, shard_rows
    return sum(shard_rows(p) for d in dirs for p in shard_paths(d))
