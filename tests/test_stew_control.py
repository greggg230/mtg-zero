"""The stew's real operations (mtgz.stew.control, docs/ENGINE.md 239.2) where they can be checked without a run: the
coordinator's body handling on a fake client, a long process stopped by the stew's STOP, the judge's job processes
(a stand-in for the match process). The process tests spawn processes: live_run_hazard."""
import json
import os
import sys
import time

import pytest

import mtgz.az  # noqa: F401
from mtgz.stew import config as C
from mtgz.stew import judge as J
from mtgz.stew.control import Interrupted, Ops, OpsError
from mtgz.stew.state import jload


def ops(tmp):
    cfg = C.normalize({'name': 'x', 'decks': ['izzet_delver_8016541'], 'candidates': ['izzet_delver_8016541'],
                       'source': {'run': 'r'}}, check_lists=False)
    return Ops(cfg, str(tmp), log=lambda m: None)


class FakeClient:
    def __init__(self, runs):
        self.runs = runs
        self.puts = []

    def get_or_none(self, path):
        r = self.runs.get(path.rsplit('/', 1)[1])
        return None if r is None else json.loads(json.dumps(r))

    def call(self, method, path, body=None):
        assert method == 'PUT'
        self.puts.append((path, body))
        self.runs[path.rsplit('/', 1)[1]] = body


def test_farm_set_puts_the_whole_body_with_only_the_state_changed(tmp_path):
    o = ops(tmp_path)
    run = {'game': 'mtg', 'build_id': 'b' * 24, 'state': 'active', 'priority': 1.0, 'selfplay': {'quota': 9},
           'alternates': [{'build_id': 'c' * 24, 'runtime': 'r', 'problems': ['x']}]}
    o._client = FakeClient({'stew-a-b': run})
    assert o.farm_set('stew-a-b', 'paused') == 'active'
    path, body = o._client.puts[0]
    assert path == '/v1/admin/run/stew-a-b' and body['state'] == 'paused' and body['selfplay'] == {'quota': 9}
    assert body['alternates'] == [{'build_id': 'c' * 24, 'runtime': 'r'}]
    assert o.farm_set('stew-a-b', 'paused') == 'paused' and len(o._client.puts) == 1      # nothing to change
    assert o.farm_set('nope', 'paused') is None and o.farm_state('nope') is None


def test_env_drops_cuda_visible_devices_then_applies_the_recipe(tmp_path, monkeypatch):
    monkeypatch.setenv('CUDA_VISIBLE_DEVICES', '-1')
    o = ops(tmp_path)
    assert 'CUDA_VISIBLE_DEVICES' not in o.env()
    o.extra_env = {'CUDA_VISIBLE_DEVICES': '-1', 'ZERO_TOKEN_GRAPHS': '1'}
    assert o.env()['CUDA_VISIBLE_DEVICES'] == '-1' and o.env()['ZERO_TOKEN_GRAPHS'] == '1'


@pytest.mark.live_run_hazard
def test_a_long_process_is_stopped_by_the_stews_stop(tmp_path):
    o = ops(tmp_path)
    t0 = time.time()
    with pytest.raises(Interrupted):
        o.run_long([sys.executable, '-c', 'import time; time.sleep(60)'], str(tmp_path / 'p.log'),
                   stop=lambda: time.time() - t0 > 1.0, poll=0.2)
    assert time.time() - t0 < 30


FAKE_PLAY = r'''
import json, sys
sys.path.insert(0, {root!r})
import mtgz.az
from mtgz.stew import judge as J
from mtgz.stew.state import jsave
job = json.load(open(sys.argv[1], encoding='utf-8'))
if job['id'] == 'fail':
    raise SystemExit(3)
open(sys.argv[2] + '.played', 'a').write('x')
jsave(sys.argv[2], {{'v': 1, 'id': job['id'], 'hash': J.job_hash(job), 'seconds': 0.1, 'unfinished': 0,
                    'rows': [{{'gid': g, 'result': [1.0, 0.0], 'over': True, 'games': 2, 'decisions': 9}}
                             for g in range(job['games'])]}})
'''


@pytest.mark.live_run_hazard
def test_judge_jobs_run_as_processes_and_resume(tmp_path, monkeypatch):
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    script = tmp_path / 'fake_play.py'
    script.write_text(FAKE_PLAY.format(root=root), encoding='utf-8')
    o = ops(tmp_path)
    monkeypatch.setattr(o, 'judge_cmd', lambda job, out: [sys.executable, str(script), job, out])
    jobs = [J._job(f'j{k}', 'cell', 'a.pt', 'b.pt', ('x', 'y'), 0, 'a0', 4, k, '{}', {}, '') for k in range(3)]
    jd = str(tmp_path / 'judge')
    res = o.judge_jobs(jobs, jd, procs=2)
    assert sorted(res) == ['j0', 'j1', 'j2'] and all(len(r['rows']) == 4 for r in res.values())
    assert o.judge_jobs(jobs, jd, procs=2) == res                     # valid results: nothing played again
    assert all(open(os.path.join(jd, f'j{k}.json.played')).read() == 'x' for k in range(3))
    jobs[1] = dict(jobs[1], seed=99)                                  # another job under the same id: played again
    o.judge_jobs(jobs, jd, procs=1)
    assert open(os.path.join(jd, 'j1.json.played')).read() == 'xx'
    with pytest.raises(OpsError, match='fail'):
        o.judge_jobs(jobs + [J._job('fail', 'cell', 'a', 'b', ('x', 'y'), 0, 'a0', 2, 0, '{}', {}, '')], jd, procs=2)
    assert jload(os.path.join(jd, 'j0.json'))['id'] == 'j0'
