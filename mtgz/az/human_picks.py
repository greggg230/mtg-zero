"""The draft network's agreement with held-out human picks (DESIGN.md 2.3.8, docs/ENGINE.md 45): a
logged curiosity, never optimised and never a training signal (DESIGN.md 0: human data is a
yardstick only). zero-core runs it on every evaluated iteration of a `team_draft` run
(`GamePackage.evals`) and logs it in metrics.jsonl under `extras.human_picks`; the dashboard
charts it ("Other yardsticks").

The positions: whole drafts of the CubeCobra test split (docs/data_m1.md 5: held out by draft;
`tools/data/08_heldout_drafts.py` keeps one seat's 3 x 15 picks consistent with an 8-seat pod,
`$MTGZ_HUMAN_PICKS`, default E:/mtg-zero/data/splits/cubecobra_heldout_8seat.json), the first
`$MTGZ_HUMAN_PICKS_DRAFTS` (default 100) of them. Each pick is a 4v4 `team_draft` position of
seat 0 built from the draft so far (the spec key `history`: its packs at its picks and its
picks; ENGINE.md 45). The drafts come from many versions of the cube: a card that is not in
the current cube (face names and accents matched) is replaced, consistently within the draft, by
a random current card the draft does not hold (seeded per draft: the same positions every
iteration). A position counts when the human's pick is a current card and the pack holds at
least two current cards; the network's choice is its policy's top card among the pack's current
cards (no search), as a human's choice was among them.

Reported: `value` = top-1 agreement, `n` positions, `drafts` (`failed`: positions the event could
not hold, none expected; none at all is an error, which zero-core logs), and on the same positions `refs`:
`random` (1 / the current cards of the pack, averaged) and `strongest card` (M1's human pick
strength, `cubecobra_bt_strength` of data/powerlsv_card_values.json: the `consensus` drafter's
pick; fit on the whole CubeCobra corpus, test split included, so it is an optimistic reference;
M1's own held-out baselines: random 21.9%, strongest card 42.3% top-1). Skipped (None) for
another game, another pod than 4v4, or no data file.
"""
import functools
import json
import os

import numpy as np

PLAYERS = 8
BASICS = ('Plains', 'Island', 'Swamp', 'Mountain', 'Forest')
CAPTION = ('The draft network\'s top-1 pick (its policy, no search) against held-out human picks of '
           'CubeCobra PowerLSV drafts (cards of older cube versions replaced); a curiosity, never '
           'optimised. Dashed: random and the strongest card by human pick strength on the same picks.')


def data_path():
    return os.environ.get('MTGZ_HUMAN_PICKS', r'E:\mtg-zero\data\splits\cubecobra_heldout_8seat.json')


def n_drafts():
    return int(os.environ.get('MTGZ_HUMAN_PICKS_DRAFTS', '100'))


def _key(name):
    """A name's matching key: ASCII letters only (the data's and the IR's accented names are each
    mangled in their own way), lower case."""
    return ''.join(ch for ch in name.encode('ascii', 'ignore').decode().lower() if ch.isalnum())


@functools.lru_cache(1)
def cube():
    """(identity -> name, the draftable identities, name key -> identity incl. face names)."""
    from . import LAYOUT_DRAFT
    cards = LAYOUT_DRAFT['cards']
    draft = [i for i, n in enumerate(cards) if n and n not in BASICS]
    by = {}
    for i in draft:
        by.setdefault(_key(cards[i]), i)
    for i in draft:
        for f in cards[i].split(' // '):
            by.setdefault(_key(f), i)
    return cards, draft, by


@functools.lru_cache(1)
def strength():
    """M1's human pick strength per identity (-inf: none)."""
    from . import ROOT
    cards, _, _ = cube()
    with open(os.path.join(ROOT, 'data', 'powerlsv_card_values.json'), encoding='utf-8') as f:
        v = json.load(f)
    ni, si = v['columns'].index('name'), v['columns'].index('cubecobra_bt_strength')
    st = {r[ni]: r[si] for r in v['rows'] if r[si] is not None}
    return np.array([st.get(n, -np.inf) if n else -np.inf for n in cards], dtype=np.float64)


@functools.lru_cache(2)
def positions(path, n):
    """The drafts as current cards and the positions that count: (drafts [(views, picks) as current
    names], positions [(draft, pick k, eligible identities, the human's identity)])."""
    with open(path, encoding='utf-8') as f:
        data = json.load(f)
    cards, draft, by = cube()
    drafts, pos = [], []
    for j, d in enumerate(data['drafts'][:n]):
        names = sorted({x for v in d['views'] for x in v})
        ident, used = {}, set()
        for x in names:                                   # a current card, once per draft
            i = by.get(_key(x))
            if i is not None and i not in used:
                ident[x] = i
                used.add(i)
        real = set(ident)
        missing = [x for x in names if x not in real]
        rng = np.random.default_rng(10_000 + j)
        free = [i for i in draft if i not in used]
        for x, i in zip(missing, rng.choice(free, size=len(missing), replace=False).tolist()):
            ident[x] = i
        views = [[cards[ident[x]] for x in v] for v in d['views']]
        picks = [cards[ident[x]] for x in d['picks']]
        drafts.append((views, picks))
        for k, (v, p) in enumerate(zip(d['views'], d['picks'])):
            elig = sorted(ident[x] for x in v if x in real)
            if p in real and len(elig) >= 2:
                pos.append((j, k, elig, ident[p]))
    return drafts, pos


def spec(drafts, j, k):
    views, picks = drafts[j]
    return json.dumps({'history': {'views': views[:k + 1], 'picks': picks[:k]}})


def agreement(run, it, batch=256):
    """`GamePackage.evals` entry: the top-1 agreement of checkpoint `it` (module docs)."""
    c = run.cfg
    path = data_path()
    if c['game'] != 'team_draft' or list(c['players']) != [PLAYERS] or not os.path.exists(path):
        return None
    import torch
    from zero import games as G
    from zero.net import load
    from zero.run import model_path
    drafts, pos = positions(path, n_drafts())
    dev = c['infer_device'] if c['infer_device'] != 'cuda' or torch.cuda.is_available() else 'cpu'
    m, _ = load(model_path(run.run, it), dev)
    E, st = G.engine(), strength()
    hits = cons = rnd = 0.0
    bad = []
    n = 0
    for b in range(0, len(pos), batch):
        chunk, xs = [], []
        for p in pos[b:b + batch]:
            try:
                xs.append(np.asarray(E.Game('team_draft', spec(drafts, p[0], p[1]), PLAYERS, 0).encode(0)))
                chunk.append(p)
            except ValueError as e:              # a position the event cannot hold: counted, reported
                bad.append(f'draft {p[0]} pick {p[1]}: {e}')
        if not chunk:
            continue
        x = np.stack(xs)
        with torch.no_grad():
            pol = m(torch.from_numpy(x).to(dev), torch.full((len(chunk),), PLAYERS, device=dev))[0].float().cpu().numpy()
        n += len(chunk)
        for (_, _, elig, h), row in zip(chunk, pol):
            e = np.asarray(elig)
            hits += e[int(np.argmax(row[e]))] == h
            cons += e[int(np.argmax(st[e]))] == h
            rnd += 1.0 / len(e)
    if not n:
        raise ValueError(f'no position of {path} could be built ({len(bad)} failed; first: {bad[:1]})')
    out = {'value': round(hits / n, 4), 'label': 'top-1 agreement with held-out human picks', 'pct': True,
           'n': n, 'drafts': len(drafts), 'se': round(float(np.sqrt(max(hits / n * (1 - hits / n), 1e-9) / n)), 4),
           'refs': {'random': round(rnd / n, 4), 'strongest card': round(cons / n, 4)}, 'caption': CAPTION}
    if bad:
        out.update(failed=len(bad), failed_first=bad[0][:300])
    return out


EVALS = {'human_picks': agreement}
