"""The fixed evaluation deck set (docs/ENGINE.md 21.3).

MTG Zero self-plays on new random decks every game but evaluates (vs the yardstick, vs random,
Elo matches between checkpoints) on a fixed deck set, so win rates and Elo compare across
iterations: the spec of every match that records nothing is `eval_spec(spec)`, the spec with
"decks": "eval" (one of the spec's `eval_decks` pairs of the evaluation set `eval_seed`, chosen
by the game seed): the game package's evaluation spec for `mtg` (zero-core's
`GamePackage.eval_specs`; a run's `eval_spec` setting overrides it; the presets also set it
explicitly). (M5a patched zero-core's `Run.driver` instead.)

Kept light (no torch / numpy): `json` only.
"""
import json


def eval_spec(spec):
    """`spec` (the mtg game's JSON) with the evaluation deck set; the same rule as the engine's
    `MtgSpec::eval_json`. A spec with explicit decklists is left alone."""
    s = json.loads(spec) if spec and spec.strip() else {}
    if not isinstance(s, dict):
        raise ValueError(f'mtg spec is not a JSON object: {spec!r}')
    if s.get('decks', 'random') == 'random':
        s['decks'] = 'eval'
    return json.dumps(s, separators=(',', ':'))
