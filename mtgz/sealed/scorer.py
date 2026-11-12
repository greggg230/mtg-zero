"""The engine's scorer of the deck evaluator's export, checked (docs/ENGINE.md 112).

The format is `mtgz.sealed.evaluator`'s torch-free export (`Evaluator.export(path)`, `python -m mtgz.sealed.evaluator
export`; `"format": "mtgz-deck-evaluator-export"` version 1, specified in that module's docstring and ENGINE.md 111.6):
kinds linear, set and pair, the card table in the EoE registry's order, the deck features formula by formula. Its
numpy reference is `evaluator.ReferenceScorer`. The engine reads the same file (`engine/src/az/sealed/scorer.rs`: the
sealed spec's deck choice by an evaluator; a pair export scored against its stored field) and must score every deck as
the reference does (to float rounding: f64 both) and as the torch model does (within `TOLERANCE`). This module checks it:

    python -m mtgz.sealed.scorer check EXPORT.json [--evaluator EVALUATOR.pt] [--decks 2000]
    python -m mtgz.sealed.scorer fixtures engine/tests/data      # the engine tests' random evaluators and their scores

`check` scores candidate decks of generated pools (what the deck choice scores) with the engine, the reference and, given
the .pt, torch, and fails past the tolerance. A version's identity is the sha256 of the export's bytes (`sha256_file`).
"""
import argparse
import hashlib
import json
import os
import sys

import numpy as np

KINDS = ('linear', 'set', 'pair')
# the largest |engine - torch| accepted (the engine and the reference compute in f64, torch in f32); the engine and the
# reference agree to float rounding (REFERENCE_TOLERANCE)
TOLERANCE = 1e-5
REFERENCE_TOLERANCE = 1e-9


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def engine_scores(path, decks):
    """The engine's scores of decks (lists of names) with the export at `path` (`mtgz._engine.sealed_scores`)."""
    from mtgz import _engine
    return np.array(json.loads(_engine.sealed_scores(os.path.abspath(path), json.dumps([list(d) for d in decks]))))


def engine_info(path):
    """The engine's reading of the export: {"sha256", "kind", "model", "members", "cards", "field_decks"}."""
    from mtgz import _engine
    return json.loads(_engine.sealed_scorer_info(os.path.abspath(path)))


def sample_decks(n, seed=0, k=8, collation='arena'):
    """Decks to compare on: the deck choice's candidates of generated pools (the best guess and randomised builds: what
    the choice scores) - `n` of them from pools seeded from `seed`."""
    from . import engine as R
    out = []
    ps = int(seed)
    while len(out) < n:
        spec = {'sealed': {'set': 'eoe', 'collation': collation, 'deck_choice': {'k': int(k), 'explore': 0.0}},
                'hidden_decklists': True, 'max_turns': 60}
        for seat in R.candidates(spec, ps):
            out.extend(c['deck'] for c in seat['candidates'])
        ps += 1
    return out[:n]


def compare(path, decks, evaluator=None):
    """The engine, the export's numpy reference and (given the .pt `evaluator`) torch on the same decks -> the largest
    absolute differences (a pair export: each against its stored field)."""
    from .evaluator import Evaluator, ReferenceScorer
    ref = ReferenceScorer(path)
    r = ref.score(decks)
    g = engine_scores(path, decks)
    info = engine_info(path)
    if info['sha256'] != sha256_file(path):
        raise RuntimeError(f'the engine read {path} as {info["sha256"]}, not its sha256')
    out = {'decks': len(decks), 'kind': info['kind'], 'model': info['model'], 'members': info['members'],
           'field_decks': info['field_decks'], 'sha256': info['sha256'], 'reference_engine': float(np.abs(r - g).max()),
           'tolerance': TOLERANCE, 'reference_tolerance': REFERENCE_TOLERANCE, 'mean_score': float(g.mean()),
           'score_sd': float(g.std())}
    if evaluator is not None:
        t = Evaluator.load(evaluator).score(decks)
        out.update(torch_engine=float(np.abs(t - g).max()), torch_reference=float(np.abs(t - r).max()))
    return out


def failed(r):
    """Why a `compare` result fails, or None."""
    if r['reference_engine'] > REFERENCE_TOLERANCE:
        return f'the engine differs from the reference by {r["reference_engine"]} (> {REFERENCE_TOLERANCE})'
    if r.get('torch_engine', 0.0) > TOLERANCE:
        return f'the engine differs from torch by {r["torch_engine"]} (> {TOLERANCE})'
    return None


def random_evaluator(kind, seed=0, members=2, d=4, k=3, hidden=8, base='linear', dim=3, field=24, decks=None):
    """An `Evaluator` of random weights (every parameter drawn: none left at its zero init) with a scaler fitted on
    `decks` (default: candidate decks of generated pools) and, for a pair evaluator (`base`, `dim`), a stored field of
    `field` candidate decks: the engine's test fixtures (`fixtures`) and checks."""
    import torch
    from . import evaluator as E
    decks = decks if decks is not None else sample_decks(64, seed=seed)
    X = E.counts_matrix(decks)
    scaler = E.Scaler(E.deck_features(X))
    sizes = {'d': d, 'k': k, 'hidden': hidden}
    hyper = sizes if kind == 'set' else ({'base': base, 'dim': dim, **(sizes if base == 'set' else {})} if kind == 'pair'
                                         else {})
    gen = torch.Generator().manual_seed(int(seed))
    nets = []
    for _ in range(members):
        net = E.make_net(kind, X.shape[1], len(E.FEATURES), **hyper)
        with torch.no_grad():
            for prm in net.parameters():
                prm.copy_(torch.randn(prm.shape, generator=gen) * 0.3)
        nets.append(net.eval())
    fx = E.counts_matrix(sample_decks(field, seed=seed + 500)) if kind == 'pair' else None
    return E.Evaluator(kind, nets, scaler, hyper, {'fixture': True, 'seed': int(seed)}, field=fx,
                       field_meta={'fixture': True} if kind == 'pair' else None)


FIXTURES = {'linear': {}, 'set': {}, 'pair': {'base': 'set', 'dim': 3}, 'pair_linear': {'base': 'linear', 'dim': 0}}


def fixtures(out_dir, seed=0, n_check=48):
    """The engine's test data (engine/tests/data/): `scorer_<name>.json`, random evaluators exported (linear, set, a
    pair over a set strength with a matchup term, a Bradley-Terry pair over a linear strength), and `scorer_check.json`
    ({"decks": [[names], ...], <name>: [torch's scores], ...}; one JSON line) -> {file: sha256}. Reproducible byte for
    byte (seeded weights, the export's compact JSON)."""
    decks = [sorted(d) for d in sample_decks(n_check, seed=seed + 1000)]
    out, check = {}, {'decks': decks}
    for name, kw in FIXTURES.items():
        kind = 'pair' if name.startswith('pair') else name
        e = random_evaluator(kind, seed=seed, **kw)
        p = os.path.join(out_dir, f'scorer_{name}.json')
        e.export(p)
        out[os.path.basename(p)] = sha256_file(p)
        check[name] = [float(x) for x in e.score(decks)]
    data = json.dumps(check, separators=(',', ':'), sort_keys=True).encode('utf-8')   # (one line: no newline to convert)
    with open(os.path.join(out_dir, 'scorer_check.json'), 'wb') as f:
        f.write(data)
    out['scorer_check.json'] = hashlib.sha256(data).hexdigest()
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    pc = sub.add_parser('check', help='the engine against the reference (and torch) on candidate decks')
    pc.add_argument('export')
    pc.add_argument('--evaluator', default=None, help='the saved evaluator (.pt) the export is of: torch too')
    pc.add_argument('--decks', type=int, default=2000)
    pc.add_argument('--seed', type=int, default=0)
    pf = sub.add_parser('fixtures', help='write the engine\'s test data (random evaluators and torch\'s scores)')
    pf.add_argument('out_dir')
    a = ap.parse_args(argv)
    if a.cmd == 'fixtures':
        r = fixtures(a.out_dir)
        print(json.dumps(r))
        return r
    r = compare(a.export, sample_decks(a.decks, a.seed), a.evaluator)
    print(json.dumps(r))
    why = failed(r)
    if why:
        raise SystemExit(why)
    return r


if __name__ == '__main__':
    main(sys.argv[1:])
