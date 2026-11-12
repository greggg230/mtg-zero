"""The deck evaluator as a file the engine reads (docs/ENGINE.md 112): the export of a trained `mtgz.sealed.evaluator`
ensemble to JSON, its numpy reference scorer, and the checks that the engine's Rust scorer (`engine/src/az/sealed/
scorer.rs`, the sealed spec's deck choice by an evaluator) scores every deck as the torch model does.

**The format** (`FORMAT` 'mtgz-deck-scorer', version 1; one JSON object, written once and never changed - a run's
versions are immutable files, named by their content's sha256):

    {"format": "mtgz-deck-scorer", "version": 1,
     "kind": "linear" | "set",                    # the model of every member (mtgz.sealed.evaluator.make_net)
     "cards": [{"name", "land", "basic", "creature", "mv", "colours": "WU", "basic_colour": "W" | ""}, ...],
                                                  # the card table: the evaluator's vocab in its order, with the
                                                  # attributes the deck features read (evaluator.card_table)
     "features": [names, ...],                    # evaluator.FEATURES, in order (the engine checks the list)
     "scaler": {"mean": [F], "std": [F]},         # the deck features' standardisation
     "hyper": {"d", "k", "hidden"},               # the set network's sizes ({} for linear)
     "members": [{"bias": b, "wf": [F], "wc": [V],          # linear and set
                  "emb": [[d] x V], "u": [[k] x V],           # set only
                  "mlp": [{"w": [[d + F] x hidden], "b": [hidden]}, {"w": [[hidden] x 1], "b": [1]}]}, ...],
     "combine": "mean_sigmoid",                   # P(win) = the members' mean of sigmoid(z)
     "source": {"evaluator": path, "sha256": of the .pt, "meta": the evaluator's meta}}

A deck's score (`score_counts`, as `Evaluator.score_counts`): counts X over the cards (an unknown card is an error),
the 35 deck features of `evaluator.deck_features` from the card attributes, scaled; per member z = bias + Fz . wf +
X . wc, and for `set` also + mlp([X @ emb / 40, Fz]) (Linear, ReLU, Linear) + 0.5 (|X @ u|^2 - X . sum_k u^2) / 40;
P(win against the field) = mean over members of sigmoid(z). The file carries everything the engine needs: the card
attributes are written out, not re-read from the engine's tables, so the Rust features are the Python ones by
construction. A **pairwise kind** (P(A beats B)) is not in version 1; the engine refuses any kind it does not know
(docs/ENGINE.md 112 lists what it will need).

    python -m mtgz.sealed.scorer export EVALUATOR.pt OUT.json     # a .pt of mtgz.sealed.evaluator -> the format
    python -m mtgz.sealed.scorer check OUT.json --evaluator EVALUATOR.pt [--decks 2000]
                                                                 # torch vs numpy vs the engine, max |diff|
"""
import argparse
import hashlib
import json
import os
import sys

import numpy as np

FORMAT = 'mtgz-deck-scorer'
VERSION = 1
KINDS = ('linear', 'set')
# the largest |torch - engine| the check accepts (the engine computes in f64, torch in f32)
TOLERANCE = 1e-5


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def _jsonable(x):
    if isinstance(x, dict):
        return {str(k): _jsonable(v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return [_jsonable(v) for v in x]
    if isinstance(x, np.generic):
        return x.item()
    if isinstance(x, float) and not np.isfinite(x):
        return None
    return x


def _floats(t):
    """A tensor or array -> nested lists of Python floats (each the exact double of the float32 value)."""
    a = np.asarray(t.detach().cpu().numpy() if hasattr(t, 'detach') else t, np.float32)
    return a.astype(np.float64).tolist()


def document(evaluator, source=None):
    """An `mtgz.sealed.evaluator.Evaluator` -> the format's dict (module docs)."""
    from . import evaluator as E
    if evaluator.kind not in KINDS:
        raise ValueError(f'evaluator kind {evaluator.kind!r}: the engine scores {KINDS} (docs/ENGINE.md 112)')
    if tuple(evaluator.names) != E.vocab():
        raise ValueError('the evaluator\'s card table is not this checkout\'s (vocab)')
    t = E.card_table()
    cards = []
    for i, n in enumerate(evaluator.names):
        cards.append({'name': n, 'land': int(t['land'][i]), 'basic': int(t['basic'][i]), 'creature': int(t['creature'][i]),
                      'mv': float(t['mv'][i]), 'colours': ''.join(c for j, c in enumerate(E.COLOURS) if t['colour'][i, j]),
                      'basic_colour': ''.join(c for j, c in enumerate(E.COLOURS) if t['basic_colour'][i, j])})
    members = []
    for net in evaluator.nets:
        st = net.state_dict()
        m = {'bias': float(st['bias'].reshape(-1)[0]), 'wf': _floats(st['wf']), 'wc': _floats(st['wc'])}
        if evaluator.kind == 'set':
            m['emb'] = _floats(st['emb'])
            m['u'] = _floats(st['u'])
            m['mlp'] = [{'w': _floats(st['mlp.0.weight']), 'b': _floats(st['mlp.0.bias'])},
                        {'w': _floats(st['mlp.2.weight']), 'b': _floats(st['mlp.2.bias'])}]
        members.append(m)
    return {'format': FORMAT, 'version': VERSION, 'kind': evaluator.kind, 'cards': cards, 'features': list(E.FEATURES),
            'scaler': {'mean': _floats(evaluator.scaler.mean), 'std': _floats(evaluator.scaler.std)},
            'hyper': dict(evaluator.hyper) if evaluator.kind == 'set' else {}, 'members': members,
            'combine': 'mean_sigmoid', 'source': _jsonable(source or {})}


def write(doc, out):
    """Write the format's dict to `out` atomically (a temporary file renamed into place) -> its sha256. The bytes
    are a function of the dict (compact separators, keys in the dict's order): one evaluator, one file."""
    data = json.dumps(doc, separators=(',', ':'), allow_nan=False).encode('utf-8')
    d = os.path.dirname(os.path.abspath(out))
    os.makedirs(d, exist_ok=True)
    tmp = os.path.join(d, f'.{os.path.basename(out)}.{os.getpid()}.tmp')
    with open(tmp, 'wb') as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, out)
    return hashlib.sha256(data).hexdigest()


def export(evaluator_path, out):
    """An evaluator file (.pt, `Evaluator.save`) -> the format at `out` -> its sha256."""
    from .evaluator import Evaluator
    e = Evaluator.load(evaluator_path)
    src = {'evaluator': os.path.abspath(evaluator_path).replace(os.sep, '/'), 'sha256': sha256_file(evaluator_path),
           'meta': e.meta}
    return write(document(e, src), out)


def load(path):
    with open(path, encoding='utf-8') as f:
        doc = json.load(f)
    check(doc, path)
    return doc


def check(doc, where='scorer'):
    """The format's structural checks (the engine makes the same ones, loudly)."""
    if doc.get('format') != FORMAT or doc.get('version') != VERSION:
        raise ValueError(f'{where}: not an {FORMAT} file of version {VERSION}')
    if doc['kind'] not in KINDS:
        raise ValueError(f'{where}: kind {doc["kind"]!r} ({KINDS})')
    from .evaluator import FEATURES
    if tuple(doc['features']) != FEATURES:
        raise ValueError(f'{where}: its deck features are not this checkout\'s')
    if not doc['members']:
        raise ValueError(f'{where}: no members')


# ----------------------------------------------------------------------------- the numpy reference scorer

def counts(doc, decks):
    """Decks (lists of names or {name: count}) -> float64 counts [n, V] over the file's cards; an unknown name is an
    error."""
    ix = {c['name']: i for i, c in enumerate(doc['cards'])}
    X = np.zeros((len(decks), len(ix)))
    for r, d in enumerate(decks):
        for n, k in (d.items() if isinstance(d, dict) else ((n, 1) for n in d)):
            if n not in ix:
                raise ValueError(f'{n!r}: not a card of the scorer\'s table')
            X[r, ix[n]] += k
    return X


def features(doc, X):
    """The deck features (evaluator.deck_features) of counts X from the file's card attributes, float64, unscaled."""
    C = 'WUBRG'
    cards = doc['cards']
    land = np.array([c['land'] for c in cards], float)
    basic = np.array([c['basic'] for c in cards], float)
    creature = np.array([c['creature'] for c in cards], float)
    mv = np.array([c['mv'] for c in cards], float)
    colour = np.array([[ch in c['colours'] for ch in C] for c in cards], float)
    bcol = np.array([[ch in c['basic_colour'] for ch in C] for c in cards], float)
    spell = 1 - land
    n = X.shape[0]
    spells = X @ spell
    creatures = X @ (creature * spell)
    curve = np.stack([X @ (spell * b) for b in (mv <= 1, mv == 2, mv == 3, mv == 4, mv == 5, mv >= 6)], 1)
    per = X @ (colour * spell[:, None])
    top = np.argsort(-per, axis=1, kind='stable')[:, :2]
    main = np.zeros_like(per)
    np.put_along_axis(main, top, 1.0, axis=1)
    main *= per > 0
    outside = ((colour[None] * (1 - main[:, None, :])).sum(-1) > 0).astype(float)
    pairs = [(a, b) for a in range(5) for b in range(a + 1, 5)]
    F = np.concatenate([np.stack([X @ land, X @ (land * (1 - basic)), creatures, spells - creatures], 1), curve,
                        ((X @ (spell * mv)) / np.maximum(spells, 1))[:, None], (per > 0).sum(1)[:, None].astype(float),
                        (X * outside * spell[None]).sum(1)[:, None], (X @ (spell * (colour.sum(1) == 0)))[:, None], per,
                        X @ bcol, (main.sum(1) == 1)[:, None].astype(float),
                        np.stack([main[:, a] * main[:, b] for a, b in pairs], 1)], 1)
    assert F.shape == (n, len(doc['features']))
    return F


def logits(doc, X):
    """Each member's z [members, n] (float64)."""
    Fz = (features(doc, X) - np.array(doc['scaler']['mean'])) / np.array(doc['scaler']['std'])
    out = []
    for m in doc['members']:
        z = m['bias'] + Fz @ np.array(m['wf']) + X @ np.array(m['wc'])
        if doc['kind'] == 'set':
            h = X @ np.array(m['emb']) / 40.0
            x = np.concatenate([h, Fz], 1)
            l1, l2 = m['mlp']
            x = np.maximum(x @ np.array(l1['w']).T + np.array(l1['b']), 0.0)
            z = z + (x @ np.array(l2['w']).T + np.array(l2['b']))[:, 0]
            u = np.array(m['u'])
            s = X @ u
            z = z + 0.5 * ((s * s).sum(1) - X @ (u * u).sum(1)) / 40.0
        out.append(z)
    return np.stack(out)


def score_counts(doc, X):
    """P(win against the field) of count rows X: the members' mean of sigmoid(z)."""
    return (1.0 / (1.0 + np.exp(-logits(doc, np.asarray(X, float))))).mean(0)


def score(doc, decks):
    return score_counts(doc, counts(doc, decks))


# ----------------------------------------------------------------------------- the engine's scorer, and the check

def engine_scores(path, decks):
    """The engine's scores of decks (lists of names) with the file at `path` (`mtgz._engine.sealed_scores`)."""
    from mtgz import _engine
    return np.array(json.loads(_engine.sealed_scores(os.path.abspath(path), json.dumps([list(d) for d in decks]))))


def engine_info(path):
    """The engine's reading of the file: {"sha256", "kind", "members", "cards"}."""
    from mtgz import _engine
    return json.loads(_engine.sealed_scorer_info(os.path.abspath(path)))


def sample_decks(n, seed=0, k=8, collation='arena'):
    """Decks to compare on: the deck choice's candidates of generated pools (the best guess and randomised builds:
    what the choice scores) - `n` of them from pools seeded from `seed`."""
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


def compare(path, evaluator_path, decks):
    """torch (the evaluator), numpy (this module) and the engine on the same decks -> the max absolute differences."""
    from .evaluator import Evaluator
    doc = load(path)
    e = Evaluator.load(evaluator_path)
    t = e.score(decks)
    r = score(doc, decks)
    g = engine_scores(path, decks)
    info = engine_info(path)
    if info['sha256'] != sha256_file(path):
        raise RuntimeError(f'the engine read {path} as {info["sha256"]}, not its sha256')
    return {'decks': len(decks), 'torch_numpy': float(np.abs(t - r).max()), 'torch_engine': float(np.abs(t - g).max()),
            'numpy_engine': float(np.abs(r - g).max()), 'tolerance': TOLERANCE, 'sha256': info['sha256'],
            'kind': doc['kind'], 'members': len(doc['members']), 'mean_score': float(t.mean()),
            'score_sd': float(t.std())}


def random_evaluator(kind, seed=0, members=2, d=4, k=3, hidden=8, decks=None):
    """An `Evaluator` of random weights (every parameter drawn, none left at its zero init) with a scaler fitted on
    `decks` (default: candidate decks of generated pools): the engine's test fixtures (`fixtures`) and checks."""
    import torch
    from . import evaluator as E
    decks = decks if decks is not None else sample_decks(64, seed=seed)
    X = E.counts_matrix(decks)
    scaler = E.Scaler(E.deck_features(X))
    hyper = {'d': d, 'k': k, 'hidden': hidden} if kind == 'set' else {}
    gen = torch.Generator().manual_seed(int(seed))
    nets = []
    for _ in range(members):
        net = E.make_net(kind, X.shape[1], len(E.FEATURES), **hyper)
        with torch.no_grad():
            for prm in net.parameters():
                prm.copy_(torch.randn(prm.shape, generator=gen) * 0.3)
        nets.append(net.eval())
    return E.Evaluator(kind, nets, scaler, hyper, {'fixture': True, 'seed': int(seed)})


def fixtures(out_dir, seed=0, n_check=48):
    """The engine's test data (engine/tests/data/): `scorer_linear.json` and `scorer_set.json` (random evaluators in
    the format) and `scorer_check.json` ({"decks": [[names], ...], "linear": [torch's scores], "set": [...]}) ->
    {file: sha256}."""
    decks = sample_decks(n_check, seed=seed + 1000)
    out, check = {}, {'decks': [sorted(d) for d in decks]}
    for kind in KINDS:
        e = random_evaluator(kind, seed=seed)
        out[f'scorer_{kind}.json'] = write(document(e, {'fixture': True}), os.path.join(out_dir, f'scorer_{kind}.json'))
        check[kind] = [float(x) for x in e.score(check['decks'])]
    data = json.dumps(check, separators=(',', ':'), sort_keys=True).encode('utf-8')   # (one line: no newline to convert)
    with open(os.path.join(out_dir, 'scorer_check.json'), 'wb') as f:
        f.write(data)
    out['scorer_check.json'] = hashlib.sha256(data).hexdigest()
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    pe = sub.add_parser('export', help='an evaluator (.pt) -> the engine\'s JSON format')
    pe.add_argument('evaluator')
    pe.add_argument('out')
    pc = sub.add_parser('check', help='torch vs numpy vs the engine on candidate decks')
    pc.add_argument('scorer')
    pc.add_argument('--evaluator', required=True)
    pc.add_argument('--decks', type=int, default=2000)
    pc.add_argument('--seed', type=int, default=0)
    pf = sub.add_parser('fixtures', help='write the engine\'s test data (random evaluators and torch\'s scores)')
    pf.add_argument('out_dir')
    a = ap.parse_args(argv)
    if a.cmd == 'fixtures':
        r = fixtures(a.out_dir)
        print(json.dumps(r))
        return r
    if a.cmd == 'export':
        h = export(a.evaluator, a.out)
        print(json.dumps({'out': a.out, 'sha256': h}))
        return h
    r = compare(a.scorer, a.evaluator, sample_decks(a.decks, a.seed))
    print(json.dumps(r))
    if max(r['torch_engine'], r['torch_numpy'], r['numpy_engine']) > TOLERANCE:
        raise SystemExit(f'the scorers differ by more than {TOLERANCE}')
    return r


if __name__ == '__main__':
    main(sys.argv[1:])
