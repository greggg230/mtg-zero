"""The fixed evaluation deck set (docs/ENGINE.md 21.3).

zero-core plays self-play and evaluation with one spec. MTG Zero wants self-play on new random
decks every game but evaluation (vs the yardstick, vs random, Elo matches between checkpoints)
on a fixed deck set, so win rates and Elo compare across iterations. `patch(zero.run)` makes
every driver the loop creates without recording samples (evaluation matches; self-play always
records) use `eval_spec(spec)`: the spec with "decks": "eval" (one of the spec's `eval_decks`
pairs of the evaluation set `eval_seed`, chosen by the game seed). A spec with explicit
decklists is left alone. `mtgz.az.run` applies it. (Listed for zero-core: a run setting for the
evaluation spec would replace this patch.)

Kept light (no torch / numpy): `json` only.
"""
import json


def eval_spec(spec):
    """`spec` (the mtg game's JSON) with the evaluation deck set; the same rule as the engine's
    `MtgSpec::eval_json`."""
    s = json.loads(spec) if spec and spec.strip() else {}
    if not isinstance(s, dict):
        raise ValueError(f'mtg spec is not a JSON object: {spec!r}')
    if s.get('decks', 'random') == 'random':
        s['decks'] = 'eval'
    return json.dumps(s, separators=(',', ':'))


def patch(run):
    """Make `run.Run.driver` use the evaluation spec for drivers that record nothing (once)."""
    Run = run.Run
    if getattr(Run.driver, '_mtgz_eval_decks', False):
        return
    orig = Run.driver

    def driver(self, seats, seed, record=False, **kw):
        c = self.cfg
        if record or c.get('game') != 'mtg':
            return orig(self, seats, seed, record=record, **kw)
        spec = c['spec']
        c['spec'] = eval_spec(spec)
        try:
            return orig(self, seats, seed, record=record, **kw)
        finally:
            c['spec'] = spec

    driver._mtgz_eval_decks = True
    driver.__doc__ = (orig.__doc__ or '') + '\n\n(MTG Zero: evaluation drivers use the evaluation deck set.)'
    Run.driver = driver
