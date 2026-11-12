"""The deck reader's target and declared inputs (docs/ENGINE.md 102.4).

With hidden decklists (EoE sealed, docs/ENGINE.md 102) the network predicts the opponent's unseen
deck. zero-core's readers (branch `feat/deck-reader`, its docs/az.md 36) are a branch that reads
declared raw inputs, predicts an auxiliary target and feeds the trunk. For `mtg`:

* the target is the auxiliary target `deck` (`MtgAz::aux_target`'s second segment, the package's
  `aux`): the frame opponent's unseen cards - their hand, library and face-down cards the decider
  does not know exactly - as counts per card identity (1024), a multinomial whose total is public;
* the inputs are `deck_reader_v7.json` for `mtg` (layout v7, docs/ENGINE.md 125) and `deck_reader_v6.json` for
  `mtg_eoe` (layout v6) (`encode::deck_reader_inputs`, checked in by the engine's tests): the layout's ranges of
  what the viewer has seen of the opponent's cards (the opponent's
  battlefield, graveyard, exile, known hand, hand clouds, known library, earlier reveals and
  phased-out permanents, by identity) and the public counts (the game's and the opponent's
  globals, their deck size, their v5 globals) - bags and global blocks only, as zero-core's reader
  requires.

`READERS` is the package declaration zero-core's reader takes (`GamePackage(readers=...)`, passed by
`mtgz.az` since the zero-core pin has readers: docs/ENGINE.md 105.3): a run sets `aux_deck_w` > 0 and
`reader_deck` 1. It declares two readers:

* `mtg`: layout v7's declaration's ranges as they are (the cube's identity space);
* `mtg_eoe` (docs/ENGINE.md 105.5): layout v6's declaration on the EoE layout (`layout_eoe.json`: layout v6, so
  the same offsets, which `eoe_inputs` checks), each bag row narrowed to the EoE identity space's
  identities - its cards (0..320, `engine/src/az/eoe_cards.txt`) and the first 64 token identities (384..;
  the registry has 9) - found by the layout's `bags` block, never by a hard-coded offset; the globals as
  declared. 3,169 inputs instead of 8,281: the other columns are zero in every EoE encoding.
"""
import json
import os

PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'deck_reader_v6.json')
# layout v7's (the `mtg` game's, docs/ENGINE.md 125: its slots are wider, the match block and v5's globals moved)
PATH_V7 = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'deck_reader_v7.json')


def declaration(path=PATH):
    """The checked-in declaration: {target, layout, features, inputs [[offset, count], ...], names}."""
    with open(path) as f:
        return json.load(f)


EOE_CAP = 384            # the EoE space's token identities from here (identity.rs EOE_CAP, docs/ENGINE.md 105)
EOE_TOKEN_READ = 64      # the token identities the EoE reader reads (the registry has 9; eoe_net.rs checks <= 64)
_HERE = os.path.dirname(os.path.abspath(__file__))
_ROOT = os.path.dirname(os.path.dirname(_HERE))


def eoe_cards():
    """The EoE card registry's length (its identities 0..n-1: engine/src/az/eoe_cards.txt)."""
    with open(os.path.join(_ROOT, 'engine', 'src', 'az', 'eoe_cards.txt'), encoding='utf-8') as f:
        return sum(1 for line in f if line.strip())


def eoe_inputs(path=PATH, layout=None):
    """The deck reader's inputs on game `mtg_eoe` (module docs): the declaration's ranges on the EoE layout, each
    whole bag row (a range of one row of the layout's `bags` block) narrowed to the EoE identities [0, cards) and
    [EOE_CAP, EOE_CAP + EOE_TOKEN_READ); every other range as declared."""
    d = declaration(path)
    if layout is None:
        with open(os.path.join(_HERE, 'layout_eoe.json'), encoding='utf-8') as f:
            layout = json.load(f)
    same = layout.get('name') == 'eoe' and d['layout'] == layout['version'] and d['features'] == layout['features']
    assert same, "the deck reader's declaration is not of the EoE layout's shape (docs/ENGINE.md 105.5)"
    (bags,) = layout['bags']
    o0, rows, w = bags['offset'], bags['count'], bags['width']
    n = eoe_cards()
    assert n <= EOE_CAP and EOE_CAP + EOE_TOKEN_READ <= w
    out = []
    for o, k in d['inputs']:
        if o0 <= o < o0 + rows * w:
            assert k == w and (o - o0) % w == 0, f'deck reader input [{o}, {k}] is not a whole bag row'
            out += [[o, n], [o + EOE_CAP, EOE_TOKEN_READ]]
        else:
            out.append([o, k])
    return out


def readers(hidden=256, enter='trunk', belief=False, path=PATH, path_v7=PATH_V7):
    """`{'mtg': [...], 'mtg_eoe': [...]}`, each `[{target, inputs, hidden, enter, belief}]`: zero-core's `readers`
    package entry (its docs/az.md 36.2) for the deck target (module docs): `mtg`'s on layout v7's declaration,
    `mtg_eoe`'s on layout v6's."""
    d = declaration(path)
    d7 = declaration(path_v7)
    assert d7['target'] == d['target'] and d7['layout'] == 7 and d['layout'] == 6

    def reader(inputs):
        return {'target': d['target'], 'inputs': inputs, 'hidden': hidden, 'enter': enter, 'belief': belief}
    return {'mtg': [reader([list(r) for r in d7['inputs']])], 'mtg_eoe': [reader(eoe_inputs(path))]}


READERS = readers()
