"""The forced find (docs/ENGINE.md 180): a self-play training aid for a combo deck whose network does not yet know its
combo. The `mtg` spec's `"forced_find": {"spell": "Doomsday", "card": "Thassa's Oracle", "p": p, "unless_in_hand": true}`
(constructed best-of-one; the engine's `engine/src/forced_find.rs`) makes a share p of the spell's resolutions put the
card in the pile: only the pile's last pick is touched, and only when the player chose no such card, has none in hand
and one can be found - its one option is then the card, which the engine answers itself (no decision reaches zero-core,
so nothing is searched or recorded for it). Every other pick and the order are the network's. The draw comes from the
game's seed and the resolution's index (every machine and every world agree); nothing of it is encoded.

Self-play only. The evaluation spec drops it (`evaldecks.eval_spec`, the engine's `MtgSpec::eval_json`), the recorded
matches and the sideboard census drop it (`strip`), and the measuring tools refuse a spec that carries it (`refuse`:
keep_h2h, the yardsticks' best of one given as `bo1_spec`, the cells yardstick, `tools/combo_stats.py play`). A run
whose own `eval_spec` carries it is refused at its first self-play phase.

The schedule (run setting `forced_find_schedule`, a dict; {} = off: the spec's own p every iteration): p moves linearly
from `start` (at iteration `from` and before) to `end` (at `from` + `iters` and after); `iters` 0 (or absent) holds
`start` (then `end` must be absent or equal). E.g. `{"start": 1.0, "end": 0.0, "from": 0, "iters": 100}`: every
resolution forced at first, none after 100 iterations. zero-core's `selfplay_specs` hook (its docs/az.md 40;
`decklists.selfplay_spec` chains it after the matchup model's) writes iteration `it`'s p into the self-play spec - a pure
function of the run's settings and `it`, so the local driver and the farm's definition of the iteration play one spec.
zero-core's remote self-play sends the phase's spec to the Mac's workers (its docs/az.md 52): a scheduled run can
self-play there too. A farm worker built before the setting existed refuses it (docs/ENGINE.md 181.5, 182.3).

Kept light (no torch / numpy): `json` only.
"""
import json

KEY = 'forced_find'
# the warm-up's setting (docs/ENGINE.md 180): Doomsday's pile must hold Thassa's Oracle
DOOMSDAY_ORACLE = {'spell': 'Doomsday', 'card': "Thassa's Oracle", 'unless_in_hand': True}
SETTINGS = {'forced_find_schedule': {}}
SCHEDULE_KEYS = ('start', 'end', 'from', 'iters')


def with_forced_find(spec, p, spell=DOOMSDAY_ORACLE['spell'], card=DOOMSDAY_ORACLE['card'], unless_in_hand=True):
    """`spec` (JSON) with `"forced_find"` at share `p` (in [0, 1]). JSON."""
    p = float(p)
    if not 0.0 <= p <= 1.0:
        raise ValueError(f'forced_find p {p}: a share in [0, 1]')
    s = json.loads(spec)
    s[KEY] = {'spell': spell, 'card': card, 'p': p, 'unless_in_hand': bool(unless_in_hand)}
    return json.dumps(s, separators=(',', ':'))


def strip(spec):
    """`spec` (JSON) without `"forced_find"` (a self-play setting); a spec without it is returned unchanged."""
    if not spec or not spec.strip():
        return spec
    s = json.loads(spec)
    if not isinstance(s, dict) or KEY not in s:
        return spec
    del s[KEY]
    return json.dumps(s, separators=(',', ':'))


def refuse(spec, who):
    """ValueError when `spec` (JSON or dict) carries the forced find: `who` measures, and the forced find is a self-play
    training aid (docs/ENGINE.md 180)."""
    s = json.loads(spec) if isinstance(spec, str) else (spec or {})
    if isinstance(s, dict) and KEY in s:
        raise ValueError(f'{who}: the spec carries "forced_find" (a self-play training aid, docs/ENGINE.md 180); '
                         f'measurements play every pick of the network\'s own - pass the evaluation spec '
                         f'(mtgz.az.evaldecks.eval_spec drops it)')


def check_schedule(sched):
    """The schedule dict, checked (module docs); {} or None = off. Returns it normalized ({} when off)."""
    if not sched:
        return {}
    if not isinstance(sched, dict):
        raise ValueError(f'forced_find_schedule {sched!r}: a dict {{"start", "end", "from", "iters"}} or {{}}')
    bad = [k for k in sched if k not in SCHEDULE_KEYS]
    if bad:
        raise ValueError(f'forced_find_schedule: unknown keys {bad} ({", ".join(SCHEDULE_KEYS)})')
    if 'start' not in sched:
        raise ValueError('forced_find_schedule: "start" (p at the start) is required')
    for k in ('start', 'end'):
        if k in sched and (isinstance(sched[k], bool) or not isinstance(sched[k], (int, float)) or not 0 <= sched[k] <= 1):
            raise ValueError(f'forced_find_schedule.{k} {sched[k]!r}: a share in [0, 1]')
    for k in ('from', 'iters'):
        if k in sched and (isinstance(sched[k], bool) or not isinstance(sched[k], int) or sched[k] < 0):
            raise ValueError(f'forced_find_schedule.{k} {sched[k]!r}: a non-negative integer')
    if not sched.get('iters') and 'end' in sched and sched['end'] != sched['start']:
        raise ValueError('forced_find_schedule: "end" differs from "start" with no "iters" to move over')
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

    class ForcedFindRefused(EncodingMismatch):
        """The forced find's settings refused (docs/ENGINE.md 180)."""

    return ForcedFindRefused(msg)


def selfplay_spec(R, it, spec=None):
    """Iteration `it`'s self-play spec with the schedule's p (`spec`: the spec so far - the matchup model's, or None for
    the run's), or None when the schedule is off (the spec so far stands). Refuses a schedule without the option in the
    spec, and a run whose `eval_spec` carries the option."""
    c = R.cfg
    try:
        if c.get('eval_spec'):
            refuse(c['eval_spec'], f'iteration {it}: the run\'s eval_spec')
        sched = check_schedule(c.get('forced_find_schedule'))
    except ValueError as e:
        raise _refused(str(e)) from None
    if not sched:
        return None
    base = spec or c.get('spec') or ''
    s = json.loads(base) if base.strip() else {}
    if KEY not in s:
        raise _refused('forced_find_schedule needs a self-play spec with "forced_find" (the spell and the card; '
                       'docs/ENGINE.md 180)')
    p = p_at(sched, it)
    s[KEY] = dict(s[KEY], p=p)
    if _LOGGED.get(getattr(R, 'run', None)) != p and hasattr(R, 'log'):
        _LOGGED[getattr(R, 'run', None)] = p
        R.log(f'iter {it} self-play: forced_find p = {p} ({s[KEY]["spell"]} finds {s[KEY]["card"]}; '
              f'forced_find_schedule {sched})')
    return json.dumps(s, separators=(',', ':'))
