"""tools/farm_campaign.py (docs/ENGINE.md 114.5; zero-farm PROTOCOL.md 4, 5.1, 7) against a fake coordinator and
zero-core's farm-release stub: create / update / pause / --from-existing, the build's signed worker found and its
fuzz-check run first, the campaign's definition (jobs.fuzz = the PUT's mode and env, builds = [the build]) signed and
published before the PUT - and only when what it allows changes -, the admin token only in farm-release's
environment, refusals before anything is published. The worker is this checkout's release mtgz-worker in a build
directory of its own (its manifest names it: the worker's build id is that manifest's); skipped without one."""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
sys.path.insert(0, os.path.join(ROOT, 'third_party', 'zero-core', 'tests'))
import farm_campaign as FC  # noqa: E402
import farm_release_stub as STUB  # noqa: E402
from zero import farm_def as FD  # noqa: E402

TOKEN = 'secret-admin-token-campaign'


def _worker():
    exe = 'mtgz-worker.exe' if os.name == 'nt' else 'mtgz-worker'
    cands = [os.environ.get('MTGZ_WORKER', '')]
    if os.environ.get('CARGO_TARGET_DIR'):
        cands.append(os.path.join(os.environ['CARGO_TARGET_DIR'], 'release', exe))
    cands += [os.path.join(ROOT, 'target', 'release', exe), os.path.join('E:/cargo-target/mtg-zero/farm', 'release', exe)]
    return next((c for c in cands if c and os.path.exists(c)), None)


WORKER = _worker()
pytestmark = pytest.mark.skipif(WORKER is None, reason='mtgz-worker is not built (cargo build --release -p mtgz_worker)')


def _build(tmp_path, runtime='9a' * 12):
    """A signed-looking build directory of this checkout's worker -> (dist dir, build_id)."""
    commit = json.loads(subprocess.run([WORKER, 'fuzz-check', '--mode', 'games'], capture_output=True, text=True,
                                       check=True).stdout)['commit']
    dist = tmp_path / 'dist'
    d = dist / 'mtg-test-release'
    d.mkdir(parents=True)
    shutil.copy2(WORKER, d / 'mtgz-worker.exe')
    b = (d / 'mtgz-worker.exe').read_bytes()
    man = {'manifest': 1, 'kind': 'worker', 'version': 'mtg-test-release', 'created': 1, 'game': 'mtg', 'engine_commit': commit,
           'core_commit': '0' * 40, 'profile': 'release', 'aw': 1, 'zero_onnx': [1], 'min_agent': '0.1.3',
           'requires_runtime': runtime, 'entry': 'mtgz-worker.exe',
           'files': [{'path': 'mtgz-worker.exe', 'sha256': hashlib.sha256(b).hexdigest(), 'size': len(b)}]}
    raw = json.dumps(man, indent=2).encode()
    (d / 'manifest.json').write_bytes(raw)
    return str(dist), hashlib.sha256(raw).hexdigest()[:24]


class Fake:
    """GET / PUT /v1/admin/campaign/{id} in memory; a campaign put is accepted only within its current definition
    (registered by the stub farm-release), as a rev 6 coordinator issues leases only within it."""

    def __init__(self, campaigns=None):
        self.campaigns = dict(campaigns or {})
        self.calls = []
        self.defs = os.environ.get('FARM_STUB_STATE')

    def __call__(self, method, url, headers, data):
        assert headers['authorization'] == f'Bearer {TOKEN}'
        path = url.split('://', 1)[1].split('/', 1)[1]
        self.calls.append((method, '/' + path))
        m = re.match(r'^v1/admin/campaign/([a-z0-9-]+)$', path)
        assert m, path
        cid = m.group(1)
        df = STUB.current(self.defs, 'campaign', cid)
        if method == 'GET':
            if cid not in self.campaigns:
                return 404, json.dumps({'error': 'not_found', 'message': cid}).encode()
            return 200, json.dumps(dict(self.campaigns[cid], campaign=cid, cursor=self.campaigns[cid]['seed_base'],
                                        definition=df and {k: df[k] for k in ('version', 'sha256', 'key')})).encode()
        body = json.loads(data)
        assert set(body) == set(FC.FIELDS), body
        assert df, 'a definition before the campaign'
        doc = df['doc']
        assert body['build_id'] in doc['builds'] and doc['jobs']['fuzz'] == {'mode': body['mode'], 'env': body['env']}
        created = cid not in self.campaigns
        self.campaigns[cid] = body
        return 200, json.dumps({'ok': True, 'created': created, 'cursor': body['seed_base']}).encode()


def _run(args, fake, capsys):
    rc = FC.main(args, client=FD.Client('http://fake', token=TOKEN, transport=fake))
    out, err = capsys.readouterr()
    assert TOKEN not in out + err
    return rc, out, err


def test_create_update_pause_and_from_existing(tmp_path, monkeypatch, capsys):
    stub = STUB.setup(monkeypatch, tmp_path, TOKEN)
    dist, bid = _build(tmp_path)
    fake = Fake()
    common = ['--dist', dist, '--key', stub['key'], '--farm-release', stub['exe'], '--defs-dir', str(tmp_path / 'defs')]
    # create: fuzz-check, definition v1, then the PUT
    rc, out, err = _run(['create', 'delver-test', '--build', bid, '--mode', 'delver-games', '--env', 'FUZZ_POOL=delver',
                         '--seed-base', '20000000', '--priority', '0.3', *common], fake, capsys)
    assert rc == 0 and 'fuzz-check delver-games ok' in err
    body = fake.campaigns['delver-test']
    assert body == {'build_id': bid, 'mode': 'delver-games', 'env': {'FUZZ_POOL': 'delver'}, 'seed_base': 20000000,
                    'chunk': 'auto', 'priority': 0.3, 'state': 'active', 'needs': None}
    calls = STUB.calls(stub['log'])
    assert len(calls) == 1 and calls[0]['token_ok'] and not any(TOKEN in x for x in calls[0]['argv'])
    assert calls[0]['argv'][:2] == ['define', os.path.join(str(tmp_path / 'defs'), 'campaign', 'delver-test', '1', 'definition.json')]
    assert calls[0]['argv'][2:] == ['--key', stub['key'], '--publish', '--url', 'http://fake']
    doc, _ = FD.load_local('campaign', 'delver-test', root=str(tmp_path / 'defs'))
    assert (doc['version'], doc['game'], doc['builds'], doc['runtime'], doc['checkpoints']) == (1, 'mtg', [bid], '9a' * 12, {})
    assert doc['jobs'] == {'selfplay': None, 'eval': None, 'fuzz': {'mode': 'delver-games', 'env': {'FUZZ_POOL': 'delver'}}}
    assert doc['expires'] == doc['created'] + 90 * 86400
    assert fake.calls.index(('PUT', '/v1/admin/campaign/delver-test')) == len(fake.calls) - 1
    assert json.loads(out.strip().splitlines()[-1])['definition']['version'] == 1
    # update the priority only: the definition still says this - not re-signed
    rc, out, err = _run(['update', 'delver-test', '--priority', '0.5', *common], fake, capsys)
    assert fake.campaigns['delver-test']['priority'] == 0.5 and len(STUB.calls(stub['log'])) == 1 and 'not re-signed' in err
    # update the env: a new version, then the PUT
    _run(['update', 'delver-test', '--env', 'FUZZ_POOL=delver', 'FUZZ_CHANCE=50', *common], fake, capsys)
    assert fake.campaigns['delver-test']['env'] == {'FUZZ_POOL': 'delver', 'FUZZ_CHANCE': '50'}
    assert len(STUB.calls(stub['log'])) == 2 and STUB.current(stub['state'], 'campaign', 'delver-test')['version'] == 2
    # pause / resume: the state only, no definition
    _run(['pause', 'delver-test', *common], fake, capsys)
    assert fake.campaigns['delver-test']['state'] == 'paused' and len(STUB.calls(stub['log'])) == 2
    _run(['resume', 'delver-test', *common], fake, capsys)
    assert fake.campaigns['delver-test']['state'] == 'active'
    # --from-existing: a live campaign (made before definitions) gets its definition; nothing is put
    fake.campaigns['delver-fuzz-live'] = {'build_id': bid, 'mode': 'delver-games', 'env': {'FUZZ_POOL': 'delver'},
                                          'seed_base': 10000000, 'chunk': 'auto', 'priority': 0.2, 'state': 'active', 'needs': None}
    n_put = sum(1 for c in fake.calls if c[0] == 'PUT')
    rc, out, err = _run(['--from-existing', 'delver-fuzz-live', *common], fake, capsys)
    res = json.loads(out.strip().splitlines()[-1])
    assert rc == 0 and res['definition']['version'] == 1 and res['mode'] == 'delver-games'
    assert sum(1 for c in fake.calls if c[0] == 'PUT') == n_put
    cur = STUB.current(stub['state'], 'campaign', 'delver-fuzz-live')['doc']
    assert cur['builds'] == [bid] and cur['jobs']['fuzz'] == {'mode': 'delver-games', 'env': {'FUZZ_POOL': 'delver'}}
    # --dry-run: the definition and the body printed, nothing signed or put
    k = len(STUB.calls(stub['log']))
    rc, out, err = _run(['create', 'dry', '--build', bid, '--mode', 'games', '--seed-base', '1', '--dry-run', *common], fake, capsys)
    assert len(STUB.calls(stub['log'])) == k and 'dry' not in fake.campaigns
    assert '"definition": 1' in out and '"PUT": "/v1/admin/campaign/dry"' in out


def test_refusals_publish_nothing(tmp_path, monkeypatch, capsys):
    stub = STUB.setup(monkeypatch, tmp_path, TOKEN)
    dist, bid = _build(tmp_path)
    fake = Fake({'have': {'build_id': bid, 'mode': 'games', 'env': {}, 'seed_base': 1, 'chunk': 'auto', 'priority': 0.2,
                          'state': 'active', 'needs': None}})
    common = ['--dist', dist, '--key', stub['key'], '--farm-release', stub['exe'], '--defs-dir', str(tmp_path / 'defs')]
    for args in (['create', 'x', '--build', bid, '--mode', 'delver-gamez', '--seed-base', '1'],     # fuzz-check refuses
                 ['create', 'x', '--build', bid, '--mode', 'games', '--env', 'FUZZ_POOL=nope', '--seed-base', '1'],
                 ['create', 'x', '--build', 'ab' * 12, '--mode', 'games', '--seed-base', '1'],     # no such build here
                 ['create', 'have', '--build', bid, '--mode', 'games', '--seed-base', '1'],        # exists: update it
                 ['update', 'missing', '--priority', '1'],
                 ['pause', 'have', '--priority', '1']):
        with pytest.raises(SystemExit):
            FC.main(args + common, client=FD.Client('http://fake', token=TOKEN, transport=fake))
    capsys.readouterr()
    assert STUB.calls(stub['log']) == [] and set(fake.campaigns) == {'have'}
    assert not [c for c in fake.calls if c[0] == 'PUT']
    # farm-release fails: nothing put; the next try publishes the same version 1 (kept, never published), then puts
    monkeypatch.setenv('FARM_STUB_FAIL', 'the coordinator is down')
    args = ['create', 'x', '--build', bid, '--mode', 'games', '--seed-base', '1', *common]
    with pytest.raises(SystemExit):
        FC.main(args, client=FD.Client('http://fake', token=TOKEN, transport=fake))
    assert 'x' not in fake.campaigns and FD.local_versions('campaign', 'x', str(tmp_path / 'defs')) == [1]
    monkeypatch.delenv('FARM_STUB_FAIL')
    _run(args, fake, capsys)
    paths = [c['argv'][1] for c in STUB.calls(stub['log'])]
    assert len(paths) == 2 and paths[0] == paths[1] and paths[0].endswith(os.path.join('x', '1', 'definition.json'))
    assert fake.campaigns['x']['mode'] == 'games' and STUB.current(stub['state'], 'campaign', 'x')['version'] == 1
