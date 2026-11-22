"""The stew (docs/ENGINE.md 239): specialists trained by self-play on one pairing each, deck stews and a main stew
distilled upward from them, judged every generation (mtgz.stew).

  python tools/stew.py init    DIR --config FILE  # check the config, copy the main stew's gen 0 and the deck stews',
                                                  #   archive the source run's window, fork the specialists' runs
  python tools/stew.py plan    DIR                # the next phase and the exact commands it would run (dry run)
  python tools/stew.py status  DIR                # generation, phase, nets, specialists' runs, yield, last report
  python tools/stew.py run     DIR                # the loop: train -> distil_decks -> judge_deck_gap -> distil_main ->
                                                  #   judge -> the next generation; stops at DIR/STOP or DIR/PAUSE
  python tools/stew.py step    DIR PHASE          # one phase by hand (it must be the next one)
  python tools/stew.py add-deck DIR NAME [--candidate] [--share S] [--alias A]   # joins at the next generation
  python tools/stew.py stop    DIR [--pause]      # write DIR/STOP (or PAUSE): the loop stops at its next safe point

Run it from the stew's pinned checkout (`code.worktree` of stew.json; refused from another one unless
--any-checkout): every process it starts runs there. `run` is long: start it hidden in the background (tools/
launch_hidden.py LOG python tools/stew.py run DIR). Exit status: 0 (done, held, paused, stopped), 2 refused, 1 failed.
"""
import argparse
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401  (the pinned core + the game package)
from mtgz.stew import config as C  # noqa: E402
from mtgz.stew.orchestrator import Refused, Stew  # noqa: E402


def same_checkout(cfg):
    wt = cfg['code'].get('worktree')
    return not wt or os.path.normcase(os.path.abspath(wt)) == os.path.normcase(ROOT)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('cmd', choices=['init', 'plan', 'status', 'run', 'step', 'add-deck', 'stop'])
    ap.add_argument('dir', help="the stew's directory")
    ap.add_argument('arg', nargs='?', help='step: the phase; add-deck: the deck (a data/decks/ name)')
    ap.add_argument('--config', help='init: the stew config (JSON, mtgz/stew/config.py)')
    ap.add_argument('--candidate', action='store_true', help='add-deck: the deck is a candidate too')
    ap.add_argument('--share', type=float, default=None, help='add-deck: its field share (default: the mean)')
    ap.add_argument('--alias', default=None, help="add-deck: its short name in farm runs' names")
    ap.add_argument('--pause', action='store_true', help='stop: write PAUSE instead of STOP')
    ap.add_argument('--any-checkout', action='store_true', help="run from a checkout other than code.worktree")
    a = ap.parse_args(argv)
    try:
        sys.stdout.reconfigure(errors='replace')
    except (AttributeError, ValueError):
        pass
    try:
        if a.cmd == 'init':
            if not a.config:
                ap.error('init: --config FILE')
            cfg = C.load(a.config)
            if not same_checkout(cfg) and not a.any_checkout:
                raise Refused(f"run tools/stew.py from the stew's code: {cfg['code']['worktree']}")
            s = Stew.init(a.dir, a.config)
            print(s.status())
            return 0
        s = Stew(a.dir)
        mutating = a.cmd in ('run', 'step', 'add-deck')
        if mutating and not same_checkout(s.cfg) and not a.any_checkout:
            raise Refused(f"run tools/stew.py from the stew's code: {s.cfg['code']['worktree']}")
        if a.cmd == 'status':
            print(s.status())
        elif a.cmd == 'plan':
            print(s.plan())
        elif a.cmd == 'run':
            print(f'stew run: {s.run()}')
            print(s.status())
        elif a.cmd == 'step':
            if a.arg not in C.PHASES:
                ap.error(f'step: a phase ({", ".join(C.PHASES)})')
            print(f'stew step {a.arg}: {s.run(only=a.arg)}')
        elif a.cmd == 'add-deck':
            if not a.arg:
                ap.error('add-deck: the deck name')
            s.add_deck(a.arg, a.share, a.alias, a.candidate)
        elif a.cmd == 'stop':
            from mtgz.stew.state import jsave
            import time
            f = 'PAUSE' if a.pause else 'STOP'
            jsave(os.path.join(a.dir, f), {'time': time.time(), 'by': 'tools/stew.py stop'})
            print(f'{f} written: the stew stops at its next safe point')
        return 0
    except (Refused, C.ConfigError) as e:
        print(f'REFUSED: {e}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
