"""The `team_draft` game's deferred results in a co-training run (zero-core's
`GamePackage.deferred`, its docs/az.md 11; DESIGN.md 2.3.6, docs/ENGINE.md 45): zero-core parses
nothing, so the game package turns an event's deferred payload into the `mtg` specs of its
matches and the matches' result rows back into the event's `resolve` input.

* `episodes(payload)`: per match of the payload (`TeamDraftAz::deferred_payload`: the event's
  turn cap and format, and per match its seed and two players, the A player first: its deck and
  sideboard, its real first picks, the cards its team saw elsewhere), the `mtg` spec of a drafted
  match: `{"max_turns", "format", "seed", "decks": {"p0", "p0_sideboard", "p0_first_picks",
  "p0_elsewhere", "p1", ...}}` (the A player is `mtg` seat 0; the engine's
  `draft_bots::match_spec` builds the same spec from the event, its tests compare the two
  games). The spec carries the event's match seed, so the match is the event's own.
* `results(payload, rows)`: the matches' result rows in the payload's order (the resolver's
  rows: `result` per seat, `scores` the game wins) -> `[{"result": [a, b], "scores": [...]}, ...]`.
  A malformed result is refused (ValueError): another number of rows than of matches, a row
  that is not a two-player match, a result other than 1/0, 0/1 or 0.5/0.5. A match cut at the
  driver's `max_steps` arrives as a draw (0.5/0.5, `over` false) and counts as one, as zero-core
  counts such an episode. `TeamDraftAz::resolve` checks the results again.

Kept light (json only): the run's coordinator calls these on its threads.
"""
import json

KEYS = ('deck', 'sideboard', 'first_picks', 'elsewhere')


def _payload(payload):
    try:
        v = json.loads(payload)
        ev, ms = v['event'], v['matches']
    except (ValueError, KeyError, TypeError) as e:
        raise ValueError(f'team_draft payload without "event" and "matches" ({e!r}): {payload[:300]}') from None
    if not isinstance(ms, list) or not ms:
        raise ValueError(f'team_draft payload: no matches: {payload[:300]}')
    return ev, ms


def match_spec(event, match):
    """The `mtg` spec (a dict) of one match of a payload."""
    players = match['players']
    if len(players) != 2:
        raise ValueError(f'team_draft payload: a match of {len(players)} players')
    decks = {}
    for p, pl in enumerate(players):
        for k in KEYS:
            decks[f'p{p}' if k == 'deck' else f'p{p}_{k}'] = list(pl[k])
    return {'max_turns': int(event['max_turns']), 'format': event['format'], 'seed': int(match['seed']), 'decks': decks}


def episodes(payload):
    """The `mtg` specs (JSON) of an event's matches, in the payload's order."""
    ev, ms = _payload(payload)
    return [json.dumps(match_spec(ev, m), separators=(',', ':')) for m in ms]


VALID = ((1.0, 0.0), (0.0, 1.0), (0.5, 0.5))


def results(payload, rows):
    """The event's `resolve` JSON from its matches' result rows (in order); malformed: ValueError."""
    _, ms = _payload(payload)
    if len(rows) != len(ms):
        raise ValueError(f'team_draft: {len(rows)} match results for {len(ms)} matches')
    out = []
    for k, r in enumerate(rows):
        res = [float(x) for x in r.get('result', ())]
        if len(res) != 2 or tuple(res) not in VALID:
            raise ValueError(f'team_draft: match {k}: result {r.get("result")!r} (a two-player match: 1/0, 0/1 or 0.5/0.5)')
        out.append({'result': res, 'scores': [float(x) for x in r.get('scores', (0, 0))]})
    return json.dumps(out)


GLUE = {'episodes': episodes, 'results': results}
