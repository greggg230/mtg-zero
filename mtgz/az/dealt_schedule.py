"""The dealt share's schedule (docs/ENGINE.md 254): the `"dealt_decks"` p of a constructed best-of-three self-play spec
(docs/ENGINE.md 154, 168.1: the share of seats whose 60 is dealt from the 75 instead of chosen) moved over a run's
iterations - high while the network learns to play every card of its 75, low once the matchup model's choice table
(docs/ENGINE.md 168) teaches the sideboarding, so most seats then play the 60s the table chooses.

The schedule (run setting `dealt_decks_schedule`, a dict; {} = off: the spec's own p every iteration) is the forced
find's shape (`mtgz.az.forced_find`): p moves linearly from `start` (at iteration `from` and before) to `end` (at `from`
+ `iters` and after); `iters` 0 (or absent) holds `start` (then `end` must be absent or equal). E.g. `{"start": 0.85,
"end": 0.15, "from": 64, "iters": 128}`: 0.85 through iteration 64, 0.15 from iteration 192. zero-core's
`selfplay_specs` hook (its docs/az.md 40; `decklists.selfplay_spec` chains it after the matchup model's table and before
the forced find's) writes iteration `it`'s p into the self-play spec - a pure function of the run's settings and `it`,
so the local driver, the Mac's workers and the farm's definition of the iteration play one spec.

A floor, not zero: the engine's `dealt_decks.p` is a share in (0, 1], and so are the schedule's `start` and `end`.
The matchup model's retrain reads the worth of a card from the seats that played another 60 than the table's; with no
dealt seat it would have nothing to compare and its table would freeze.

Kept light (no torch / numpy): `json` only.
"""
import json

KEY = 'dealt_decks'
SETTINGS = {'dealt_decks_schedule': {}}
SCHEDULE_KEYS = ('start', 'end', 'from', 'iters')


def check_schedule(sched):
    """The schedule dict, checked (module docs); {} or None = off. Returns it normalized ({} when off)."""
    if not sched:
        return {}
    if not isinstance(sched, dict):
        raise ValueError(f'dealt_decks_schedule {sched!r}: a dict {{"start", "end", "from", "iters"}} or {{}}')
    bad = [k for k in sched if k not in SCHEDULE_KEYS]
    if bad:
        raise ValueError(f'dealt_decks_schedule: unknown keys {bad} ({", ".join(SCHEDULE_KEYS)})')
    if 'start' not in sched:
        raise ValueError('dealt_decks_schedule: "start" (p at the start) is required')
    for k in ('start', 'end'):
        if k in sched and (isinstance(sched[k], bool) or not isinstance(sched[k], (int, float)) or not 0 < sched[k] <= 1):
            raise ValueError(f'dealt_decks_schedule.{k} {sched[k]!r}: a share in (0, 1] (a floor above 0: the engine refuses '
                             f'dealt_decks.p 0, and the matchup model learns from the dealt seats)')
    for k in ('from', 'iters'):
        if k in sched and (isinstance(sched[k], bool) or not isinstance(sched[k], int) or sched[k] < 0):
            raise ValueError(f'dealt_decks_schedule.{k} {sched[k]!r}: a non-negative integer')
    if not sched.get('iters') and 'end' in sched and sched['end'] != sched['start']:
        raise ValueError('dealt_decks_schedule: "end" differs from "start" with no "iters" to move over')
    return dict(sched)


def p_at(sched, it):
    """The schedule's p at iteration `it` (module docs), rounded to 6 decimals; None when the schedule is off."""
    sched = check_schedule(sched)
    if not sched:
        return None
    start = float(sched['start'])
    end = float(sched.get('end', start))
    frm, iters = int(sched.get('from', 0)), int(sched.get('iters', 0))
    if iters <= 0 or it <= frm:
        return round(start, 6)
    f = min(1.0, (it - frm) / iters)
    return round(start + (end - start) * f, 6)


_LOGGED = {}


def _refused(msg):
    """A refusal of the run's settings (zero.run's EncodingMismatch: exit status 78, not restarted - as
    decklists.selfplay_spec's keep_deck refusal)."""
    from zero.run import EncodingMismatch

    class DealtScheduleRefused(EncodingMismatch):
        """The dealt share's schedule refused (docs/ENGINE.md 254)."""

    return DealtScheduleRefused(msg)


def selfplay_spec(R, it, spec=None):
    """Iteration `it`'s self-play spec with the schedule's p (`spec`: the spec so far - the matchup model's, or None for
    the run's), or None when the schedule is off (the spec so far stands). Refuses a malformed schedule and one
    without dealt decks in the spec."""
    c = R.cfg
    try:
        sched = check_schedule(c.get('dealt_decks_schedule'))
    except ValueError as e:
        raise _refused(str(e)) from None
    if not sched:
        return None
    base = spec or c.get('spec') or ''
    s = json.loads(base) if base.strip() else {}
    if not isinstance(s.get(KEY), dict):
        raise _refused('dealt_decks_schedule needs a self-play spec with "dealt_decks" (its swaps and seats; '
                       'docs/ENGINE.md 154, 168.1)')
    p = p_at(sched, it)
    s[KEY] = dict(s[KEY], p=p)
    if _LOGGED.get(getattr(R, 'run', None)) != p and hasattr(R, 'log'):
        _LOGGED[getattr(R, 'run', None)] = p
        R.log(f'iter {it} self-play: dealt_decks p = {p} (dealt_decks_schedule {sched})')
    return json.dumps(s, separators=(',', ':'))
