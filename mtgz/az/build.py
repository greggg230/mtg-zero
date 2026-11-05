"""Build an engine position from a description, and describe one (docs/ENGINE.md 95).

For playing real Magic Online games with the network: at each decision our seat faces, the MTGO
adapter describes the position as MTGO shows it to our seat (a dict: the schema of ENGINE.md 95.2,
`EXAMPLE` below), the engine builds it - the decklists partitioned over the description, what our seat
cannot see dealt from the seed - and the usual search runs on the built game (`legal()`,
`encode(viewer)`, `export_state()`, `search_tree(...)`, `zero.parity.search_rs`, ...)::

    import mtgz.az.build as B
    g = B.build_game(description, seed=7)          # an engine Game at the described decision
    g.legal(); g.encode(description['viewer'])
    B.describe_from_engine(g, 0)                   # the description of an engine position (the inverse)

A decision inside a cast or a resolution (the opponent's Daze resolving: pay {1}?) is the priority
before it plus `"then": ["Pass priority"]`: the description's `then` lists the actions (option labels as
`export_state` shows them, or action ids) taken from the built decision on, before the deal.

`describe_from_engine` needs a game whose spec has `"describe": true` (`build_game`'s games have it;
for another game use `spec_json(describe=True)`): the engine's `export_state` then carries each seat's
description (`{"error": ...}` where the schema does not reach: a decision inside a cast or a
resolution, a rare state; the error names it).

Kept light: `json` only until a game is made (the engine is imported then).
"""
import json

from . import decklists

# The documented example (ENGINE.md 95.2): the opponent (P1) cast Lightning Bolt at our Dragon's Rage
# Channeler; we (P0) hold priority with Daze and Force of Will. Places name objects: b<i> the i-th
# permanent of `battlefield`, g<p>.<i> a graveyard card, x<i> an exile card, s<i> a stack item.
EXAMPLE = {
    "viewer": 0, "turn": 5, "active": 1, "step": "main1", "priority": 0, "passes": 0, "starting": 1,
    "decision": "priority",
    "players": [
        {"life": 18, "hand": ["Daze", "Force of Will", "Brainstorm",
                              {"name": "Lightning Bolt", "known_to_opponent": True}],
         "library_size": 51, "library_known": [{"pos": 0, "name": "Ponder", "known_by": [0]}],
         "graveyard": ["Mishra's Bauble", "Ponder"], "spells_game": 2},
        {"life": 19, "hand_size": 4, "hand": ["Wasteland"], "library_size": 51,
         "library_known": [{"pos": 0, "known_by": [1]}, {"pos": 1, "known_by": [1]}],
         "graveyard": ["Brainstorm"], "lands_played": 1, "spells_cast": 1, "noncreature_cast": 1,
         "spells_game": 3},
    ],
    "battlefield": [
        {"name": "Volcanic Island", "owner": 1, "tapped": True, "entered": 1},
        {"name": "Island", "owner": 0, "entered": 2},
        {"name": "Polluted Delta", "owner": 1, "entered": 3},
        {"name": "Dragon's Rage Channeler", "owner": 0, "entered": 2, "control_since": 2},
        {"name": "Volcanic Island", "owner": 0, "entered": 4},
        {"name": "Volcanic Island", "owner": 1, "tapped": True, "entered": 5, "control_since": 5},
    ],
    "exile": [],
    "stack": [{"spell": "Lightning Bolt", "controller": 1, "targets": [{"req": 0, "obj": "b3"}]}],
}


def engine():
    """The engine module (`mtgz._engine`, registered by `mtgz.az`)."""
    import mtgz.az  # noqa: F401  (registers the package)
    from zero.games import engine as e
    return e()


def mirror_spec():
    """The Delver mirror's spec (the preset delver-mirror's), as a dict."""
    return json.loads(decklists.mirror_spec(decklists.IZZET_DELVER, max_turns=60))


def spec_json(description=None, spec=None, p1_deck=None, describe=True):
    """The `mtg` spec (JSON) of a built game: `spec` (a dict; default the Delver mirror's) with player 1's
    deck replaced by `p1_deck` (card names, 60+), the description as its `build` block, and `describe`."""
    sp = dict(mirror_spec() if spec is None else spec)
    if p1_deck is not None:
        sp['decks'] = dict(sp['decks'], p1=list(p1_deck))
    if description is not None:
        sp['build'] = description
    sp.pop('describe', None)
    if describe:
        sp['describe'] = True
    return json.dumps(sp, separators=(',', ':'))


def build_game(description, seed=0, spec=None, p1_deck=None, describe=True):
    """An engine Game at the described position (ENGINE.md 95): its unknown cards dealt from `seed`
    (the description's `deal_seed`, if it has one, wins). A description that does not fit its decks, or
    names something the schema does not build, is a ValueError naming it."""
    return engine().Game('mtg', spec_json(description, spec, p1_deck, describe), 2, int(seed))


def describe_from_engine(game, viewer):
    """The description of `game`'s position from `viewer`'s view (a dict; ENGINE.md 95). `game`'s spec
    must have `"describe": true`. A position the schema does not reach is a ValueError naming why."""
    st = json.loads(game.export_state())
    if 'describe' not in st:
        raise ValueError('describe_from_engine: the game\'s spec has no "describe": true (spec_json(describe=True))')
    d = st['describe'][viewer]
    if 'error' in d:
        raise ValueError(d['error'])
    return d


def viewer_view(state, viewer, decides):
    """`export_state` (a dict) as `viewer` may see it: card ids (slot numbers) become places, the log
    goes, the opponent's hand is its size and the cards the viewer knows, libraries their known positions
    (names only where the viewer knows them), the decision's options only when the viewer decides.
    Two games the viewer cannot tell apart have equal views (the round-trip test's check (c))."""
    ids = {}
    for p in range(2):
        for i, c in enumerate(state['players'][p]['battlefield']):
            ids[c['id']] = f'bf{p}.{i}'
    item = {x['stack']: i for i, x in enumerate(state['stack'])}

    def names(cards):
        return [[c['name'], c['owner'], c.get('token')] for c in cards]

    out = {k: state[k] for k in ('v', 'turn', 'active', 'step', 'priority', 'starting', 'monarch',
                                 'initiative', 'over', 'winner', 'reason', 'match') if k in state}
    if 'command' in state:
        out['command'] = names(state['command'])
    players = []
    for p in range(2):
        pl, kn = state['players'][p], state['known'][p]
        m = {k: pl[k] for k in ('life', 'library', 'lands_played', 'mulligans', 'pool', 'poison', 'energy',
                                'lost') if k in pl}
        m['hand_size'] = len(pl['hand'])
        if p == viewer:
            m['hand'] = sorted(c['name'] for c in pl['hand'])
        m['hand_known_to_other'] = sorted(x[1] for x in kn['hand'])
        m['graveyard'] = names(pl['graveyard'])
        m['exile'] = names(pl['exile'])
        bf = []
        for c in pl['battlefield']:
            c = {k: v for k, v in c.items() if k != 'id'}
            for k in ('attacks', 'blocking', 'attached'):
                if k in c:
                    c[k] = ids.get(c[k], '?')
            bf.append(c)
        m['battlefield'] = bf
        m['library_known'] = [[e[0], e[1], e[2] if e[1] & (1 << viewer) else None] for e in kn['library']]
        players.append(m)
    out['players'] = players
    stack = []
    for x in state['stack']:
        x = {k: v for k, v in x.items() if k not in ('stack', 'id')}
        if 'targets' in x:
            ts = []
            for t in x['targets']:
                t = {k: v for k, v in t.items() if k != 'id'}
                if 'stack' in t:
                    t['stack'] = item.get(t['stack'])
                ts.append(t)
            x['targets'] = ts
        stack.append(x)
    out['stack'] = stack
    d = state.get('decision')
    if d:
        m = {'player': d['player'], 'kind': d['kind']}
        if decides:
            m['options'] = [[o['a'], o['t']] for o in d['options']]
            for k in ('task', 'source'):
                if k in d:
                    m[k] = d[k]
        if 'for' in d:
            m['for'] = item.get(d['for'])
        out['decision'] = m
    return out


def round_trip(game, viewer, seed=0, spec=None, p1_deck=None):
    """Describe `game` (its spec with "describe": true) from `viewer`'s view, build the description and
    compare (ENGINE.md 95.4): the legal options when the viewer decides, the encoding (bitwise), the
    viewer's export_state view, the description of the built game. -> (description, [differences]);
    a position the schema does not reach raises ValueError."""
    import numpy as np
    d = describe_from_engine(game, viewer)
    b = build_game(d, seed, spec, p1_deck)
    bad = []
    decides = game.to_move == viewer
    if decides and list(game.legal()) != list(b.legal()):
        bad.append(f'legal: {list(game.legal())} vs {list(b.legal())}')
    ea, eb = np.asarray(game.encode(viewer), np.float32), np.asarray(b.encode(viewer), np.float32)
    if not np.array_equal(ea.view(np.uint32), eb.view(np.uint32)):
        bad.append(f'encode: {int((ea != eb).sum())} floats differ')
    va = viewer_view(json.loads(game.export_state()), viewer, decides)
    vb = viewer_view(json.loads(b.export_state()), viewer, decides)
    if va != vb:
        bad.append('export_state view')
    if describe_from_engine(b, viewer) != d:
        bad.append('redescribe')
    return d, bad
