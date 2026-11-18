"""`python -m mtgz.az.h2h`: zero.h2h of the pinned zero-core (third_party/zero-core) for MTG Zero.
Imported, this module IS zero.h2h (same object: settings, state and patches are shared).

`--publish EXPERIMENT [--title T] [--a-label ...]` (mtgz.az.site_h2h, docs/ENGINE.md 193): when the match ends, its
result (the JSON line zero.h2h prints) goes to delverai's Head-to-heads tab as a match of h2h.<EXPERIMENT>. Paired
matches only (`--paired`: the tab shows paired ones); the post's prerequisites are checked before the games start."""
import sys

import mtgz.az  # noqa: F401  (registers the game package)
from zero import h2h as _core


class _Tee:
    """stdout, with every line written also kept (the result is the last JSON line)."""

    def __init__(self, out):
        self.out, self.buf = out, []

    def write(self, s):
        self.buf.append(s)
        return self.out.write(s)

    def flush(self):
        self.out.flush()

    def __getattr__(self, k):
        return getattr(self.out, k)


def main(argv=None):
    import json
    from mtgz.az import site_h2h as S
    o, rest = S.pop_args(sys.argv[1:] if argv is None else argv)
    if not o.publish:
        return _core.main(rest)
    if '--paired' not in rest:
        raise SystemExit('--publish: a paired match only (--paired): the Head-to-heads tab shows paired matches')
    S.check(o)
    tee = _Tee(sys.stdout)
    sys.stdout = tee
    try:
        _core.main(rest)
    finally:
        sys.stdout = tee.out
    lines = [x for x in ''.join(tee.buf).splitlines() if x.strip().startswith('{')]
    if not lines:
        raise SystemExit('--publish: the match printed no result')
    if S.publish_result(json.loads(lines[-1]), o) != 0:
        sys.exit(S.EXIT_PUBLISH_FAILED)


if __name__ == '__main__':
    main()
else:
    sys.modules[__name__] = _core
