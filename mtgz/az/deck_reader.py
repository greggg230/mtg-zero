"""The deck reader's target and declared inputs (docs/ENGINE.md 102.4).

With hidden decklists (EoE sealed, docs/ENGINE.md 102) the network predicts the opponent's unseen
deck. zero-core's readers (branch `feat/deck-reader`, its docs/az.md 36) are a branch that reads
declared raw inputs, predicts an auxiliary target and feeds the trunk. For `mtg`:

* the target is the auxiliary target `deck` (`MtgAz::aux_target`'s second segment, the package's
  `aux`): the frame opponent's unseen cards - their hand, library and face-down cards the decider
  does not know exactly - as counts per card identity (1024), a multinomial whose total is public;
* the inputs are `deck_reader_v6.json` (`encode::deck_reader_inputs`, checked in by the engine's
  tests): layout v6 ranges of what the viewer has seen of the opponent's cards (the opponent's
  battlefield, graveyard, exile, known hand, hand clouds, known library, earlier reveals and
  phased-out permanents, by identity) and the public counts (the game's and the opponent's
  globals, their deck size, their v5 globals) - bags and global blocks only, as zero-core's reader
  requires.

`READERS` is the package declaration zero-core's reader takes (`GamePackage(readers=...)`). The
pinned zero-core has no readers yet, so the package does not pass it; wiring it is the reader's
merge (a run then sets `aux_deck_w` > 0 and `reader_deck` 1). Every EoE card encodes as identity
`UNKNOWN` until an EoE layout gives the set identities (docs/ENGINE.md 101.5), so the target and
these bags mean something for EoE only from that layout on (they do for the cube's decks now).
"""
import json
import os

PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'deck_reader_v6.json')


def declaration(path=PATH):
    """The checked-in declaration: {target, layout, features, inputs [[offset, count], ...], names}."""
    with open(path) as f:
        return json.load(f)


def readers(hidden=256, enter='trunk', belief=False, path=PATH):
    """`{'mtg': [{target, inputs, hidden, enter, belief}]}`: zero-core's `readers` package entry
    (its docs/az.md 36.2) for the deck target."""
    d = declaration(path)
    return {'mtg': [{'target': d['target'], 'inputs': [list(r) for r in d['inputs']], 'hidden': hidden,
                     'enter': enter, 'belief': belief}]}


READERS = readers()
