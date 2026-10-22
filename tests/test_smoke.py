"""The MTG smoke run (preset mtg-smoke, CPU, the built-in heuristic bot as the yardstick): one
iteration end to end through `python -m mtgz.az.run` on zero-core. The full 3-iteration preset is
part of zero-core's tools/smoke_all.sh."""
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def test_mtg_smoke_iteration(tmp_path):
    run = tmp_path / 'smoke'
    p = subprocess.run([sys.executable, '-m', 'mtgz.az.run', '--run', str(run), '--preset', 'mtg-smoke', '--iters', '1'],
                       cwd=ROOT, capture_output=True, text=True, timeout=900,
                       env=dict(os.environ, PYTHONIOENCODING='utf-8'))
    assert p.returncode == 0, p.stdout[-3000:] + p.stderr[-3000:]
    m = [json.loads(line) for line in open(run / 'metrics.jsonl')]
    assert [r['iter'] for r in m] == [0, 1]
    assert m[1]['selfplay']['games'] == 16 and m[1]['selfplay']['samples'] > 0 and m[1]['selfplay']['unfinished'] == 0
    assert m[1]['eval_yard']['opp'] == 'heuristic' and m[1]['eval_yard']['games'] == 8
    assert m[1]['eval_random']['games'] == 4
    cfg = json.load(open(run / 'config.json'))
    assert cfg['game'] == 'mtg' and json.loads(cfg['spec'])['max_turns'] == 60
