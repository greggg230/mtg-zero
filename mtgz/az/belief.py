"""The belief target (docs/ENGINE.md 66): the decider's opponent's hand, by card identity.

The engine computes it from the TRUE state (`MtgAz::aux_target`, the zero-core auxiliary target
`belief`: counts over the encoding's 1,024 card identities, ENGINE.md 22.2) and zero-core records it
with every self-play sample when a run's `aux_belief_w` is on (its docs/az.md 15); the network's
auxiliary head learns it as a multinomial (a distribution over identities: x the opponent's hand
size, which is public, = the expected count of each card). Never an input: `encode` shows the
decider only what it knows.

This module: the identities' card names (`names`), the replay field (`render`: what zero.replay
shows per network decision of a checkpoint with the head), and the public facts of a position read
back from its features (`public`, `uniform_prior`: the baseline of what the search's determinize
deals today - uniformly over the opponent's cards the decider has not seen).
"""
import functools
import json
import os

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
D = 1024                      # identities (engine/src/az/identity.rs D)
CUBE_CAP = 768                # tokens from here (identity.rs CUBE_CAP)
LEGACY_BASE = 546             # the legacy registry's first identity (identity.rs LEGACY_BASE)
# The feature offsets the baselines read (engine/src/az/encode.rs, layout v5): the bags (0.25 per
# card), the opponent's hand size in the globals (the second player block: hand / 10).
O_BAGS = 384
OPP_HIDDEN, OPP_HAND_KNOWN, OPP_HAND_CLOUD = 8, 9, 10
G_OPP_HAND = 24 + 40 + 1

TARGET = {'name': 'belief', 'width': D, 'loss': 'multinomial'}


@functools.lru_cache(maxsize=1)
def names():
    """Identity -> card name (a list of D; None where no card has the identity): the cube's IR card
    list in order (identities 0..545, data/cards/powerlsv.ir.json), the legacy registry
    (546.., engine/src/az/legacy_cards.txt), the token registry's keys (768.., tokens.txt). A card's
    name as the engine's spectator view (`export_state`) writes it in a hand (tested)."""
    out = [None] * D
    with open(os.path.join(ROOT, 'data', 'cards', 'powerlsv.ir.json'), encoding='utf-8') as f:
        for i, c in enumerate(json.load(f)['cards']):
            out[i] = c['name']
    az = os.path.join(ROOT, 'engine', 'src', 'az')
    for base, file in ((LEGACY_BASE, 'legacy_cards.txt'), (CUBE_CAP, 'tokens.txt')):
        with open(os.path.join(az, file), encoding='utf-8') as f:
            for k, line in enumerate(x for x in (l.rstrip('\r\n') for l in f) if x):
                out[base + k] = line
    return out


def counts_by_name(v, top=None, digits=3):
    """[[name, value], ...] of the non-zero entries of a per-identity vector, largest first."""
    v = np.asarray(v, np.float64)
    nm = names()
    idx = [int(i) for i in np.argsort(-v, kind='stable') if v[i] > 0]
    if top:
        idx = idx[:top]
    return [[nm[i] if nm[i] is not None else f'#{i}', round(float(v[i]), digits)] for i in idx]


def render(pred, state, seat, frame, top=12):
    """zero.replay's frame field of the belief head (the game package's `replay_aux`): {'belief': {'for':
    the seat whose hand is predicted (the decider's opponent), 'n': that seat's hand size in the
    frame's state, 'cards': [[name, expected count], ...] - the head's distribution x n, the `top`
    largest, rounded to 3 places; they sum to about n}}."""
    p = np.asarray(pred['belief'], np.float64)
    opp = 1 - int(seat)
    pl = [x for x in state.get('players', []) if x.get('id') == opp]
    n = len(pl[0].get('hand', [])) if pl else 0
    return {'belief': {'for': opp, 'n': n, 'cards': counts_by_name(p * n, top) if n else []}}


def public(x):
    """The public facts of positions (features [B, F], layout v5) the baselines need -> (opponent hand
    size [B], the opponent's cards the decider has not seen exactly by identity (hand and library:
    the `OPP_HIDDEN` bag) [B, D], their hand cards it knows exactly [B, D], the identities its clouds
    place in their hand [B, D]) as counts."""
    x = np.asarray(x, np.float32)

    def bag(b):
        return np.rint(4.0 * x[:, O_BAGS + b * D:O_BAGS + (b + 1) * D]).astype(np.float32)
    return np.rint(10.0 * x[:, G_OPP_HAND]).astype(np.float32), bag(OPP_HIDDEN), bag(OPP_HAND_KNOWN), bag(OPP_HAND_CLOUD)


def uniform_prior(x):
    """What the search's determinize deals today, as expected counts [B, D] (the baseline): the
    opponent's known hand cards, plus its other hand cards uniform over the cards the decider has not
    seen (each identity's share of the hidden pool). The clouds' partial knowledge (rare: a Brainstorm
    put-back of a card the decider knew) is left out: they are in the pool."""
    n, hidden, known, _ = public(x)
    u = np.maximum(n - known.sum(1), 0.0)
    tot = hidden.sum(1, keepdims=True)
    return known + u[:, None] * hidden / np.maximum(tot, 1e-9)
