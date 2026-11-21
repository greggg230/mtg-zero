"""A farm analysis campaign's alternates and model variants (docs/ENGINE.md 215; mtgz.az.farm_campaign
prepare_alternates): the release index's routes for a build, the run PUT's alternates with each GPU route's variant,
the definition's builds and signed variants, the variants uploaded and registered with each gen, min_agent, the
refusals (another engine, a self-test that fails), none at all said in the state, a campaign from before alternates
unchanged, and the GPU self-test's reading of probe-providers. A fake coordinator and fake signed builds; no farm, GPU or
network is needed (the GPU self-test and the variant export are stubbed where they would run a worker)."""
import hashlib
import json
import os
import subprocess
import sys
from urllib.parse import urlparse

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
import mtgz.az  # noqa: E402,F401
from mtgz.az import farm_campaign as C  # noqa: E402
from zero import farm_def as FD  # noqa: E402

COMMIT = 'e' * 40


def _id(b):
    return hashlib.sha256(b).hexdigest()[:24]


class Farm:
    """Signed manifests (published under the builds directory too: the desktop's copies), the coordinator's desktop API
    for one campaign run (manifests, run, checkpoints, artifacts: every call recorded)."""

    def __init__(self, builds_dir):
        self.dir, self.manifests, self.calls = builds_dir, {}, []

    def add(self, man, exe=None):
        if exe is not None:
            man = dict(man, entry='mtgz-worker.exe',
                       files=[{'path': 'mtgz-worker.exe', 'size': len(exe), 'sha256': hashlib.sha256(exe).hexdigest()}])
        b = json.dumps(man, sort_keys=True).encode()
        bid = _id(b)
        self.manifests[bid] = b
        d = os.path.join(self.dir, bid)
        os.makedirs(d, exist_ok=True)
        with open(os.path.join(d, 'manifest.json'), 'wb') as f:
            f.write(b)
        if exe is not None:
            with open(os.path.join(d, 'mtgz-worker.exe'), 'wb') as f:
                f.write(exe)
        return bid

    def __call__(self, method, url, headers, data):
        u = urlparse(url)
        self.calls.append((method, u.path, data))
        if method == 'GET' and u.path.startswith('/v1/admin/artifact/builds/'):
            return 200, self.manifests[u.path.split('/')[5]]
        if method == 'GET' and u.path.startswith('/v1/admin/run/'):
            return 200, b'{}'
        return 200, b'{"ok": true}'


@pytest.fixture
def farm(tmp_path, monkeypatch):
    bd = tmp_path / 'builds'
    bd.mkdir()
    monkeypatch.setenv('MTGZ_FARM_BUILDS', str(bd))
    monkeypatch.setattr(FD, 'host_platform', lambda: 'windows-x64')     # (the routes' Windows builds run "here")
    f = Farm(str(bd))
    f.cpu = f.add({'manifest': 1, 'kind': 'runtime', 'version': 'cpu'})
    f.cuda_rt = f.add({'manifest': 1, 'kind': 'runtime', 'version': 'cuda', 'gpu': {'vendor': 'nvidia', 'providers': ['cuda']}})
    f.dml_rt = f.add({'manifest': 1, 'kind': 'runtime', 'version': 'dml', 'gpu': {'vendor': 'amd', 'providers': ['dml']}})
    f.mac_rt = f.add({'manifest': 1, 'kind': 'runtime', 'version': 'mac', 'platform': 'macos-arm64'})
    w = {'manifest': 1, 'kind': 'worker', 'game': 'mtg', 'engine_commit': COMMIT}
    f.base = f.add(dict(w, requires_runtime=f.cpu, min_agent='0.1.3'), b'cpu worker')
    f.cuda = f.add(dict(w, requires_runtime=f.cuda_rt, min_agent='0.2.11'), b'cpu worker')
    f.dml = f.add(dict(w, requires_runtime=f.dml_rt, min_agent='0.3.11'), b'cpu worker')
    f.mac = f.add(dict(w, requires_runtime=f.mac_rt, min_agent='0.3.0', platform='macos-arm64'), b'mach-o worker')
    f.old = f.add(dict(w, requires_runtime=f.cuda_rt, min_agent='0.2.11', engine_commit='a' * 40), b'old worker')
    return f


def _index(f, mac='published', cuda=True, dml=True):
    e = {'commit': COMMIT, 'build_id': f.base, 'runtime': f.cpu, 'published': True,
         'cuda': {'build_id': f.cuda, 'runtime': f.cuda_rt} if cuda else None,
         'dml': {'build_id': f.dml, 'runtime': f.dml_rt} if dml else None,
         'mac': None if mac is None else {'release': dict({'build_id': f.mac}, **({'published': 'x'} if mac == 'published' else {}))}}
    with open(os.path.join(f.dir, 'index.json'), 'w', encoding='utf-8') as fh:
        json.dump({'schema': 1, 'builds': [e]}, fh)


def _state(tmp_path, f, alternates=C.RELEASE, variants=C.DEFAULT_VARIANTS, min_agent='0.2.11'):
    gens = {}
    for g in ('1', '2'):
        d = tmp_path / 'exports' / g
        d.mkdir(parents=True)
        (d / 'model.onnx').write_bytes(b'graph ' + g.encode())
        (d / 'reference.npz').write_bytes(b'ref ' + g.encode())
        gens[g] = {'model': str(d / 'model.onnx'), 'sha256': hashlib.sha256(b'graph ' + g.encode()).hexdigest(),
                   'model_size': 7, 'reference': str(d / 'reference.npz'),
                   'reference_sha256': hashlib.sha256(b'ref ' + g.encode()).hexdigest(), 'reference_size': 5,
                   'checkpoint': f'iter_{g}.pt', 'checkpoint_sha256': g * 64}
    st = {'run': 'cmp-alts', 'games': 4, 'batch': 'c1', 'paired': True, 'seed': 0, 'spec': '{}', 'priority': 0.5,
          'seats': [{'net': 1}, {'net': 2}], 'build_id': f.base, 'runtime': f.cpu, 'worker': 'w.exe', 'min_agent': min_agent,
          'games_per_lease': 'auto', 'share': 0.25, 'needs': None, 'gens': gens, 'settings': {'eval_trace': 1},
          'job': {'kind': 'play', 'trace': True, 'samples': False},
          'alt_want': C.alternates_want(alternates, variants)}
    d = str(tmp_path / 'cmp-alts')
    C.save_state(d, st)
    return d, st


@pytest.fixture
def stubs(monkeypatch):
    """The variant export (a file per form), the platform check and the GPU self-test recorded, not run."""
    rec = {'platform': [], 'selftest': [], 'export': []}

    def export_variant(form, g, gen, spec, log, cache=None):
        p = os.path.join(os.path.dirname(g['model']), f'model-{form}.onnx')
        with open(p, 'wb') as fh:
            fh.write(f'{form} of {gen}'.encode())
        rec['export'].append((form, gen))
        return {'form': form, 'model': p, 'sha256': C.file_sha256(p), 'model_size': os.path.getsize(p)}

    def gpu_selftest(exe, pack_dir, provider, model, reference, timeout=900):
        rec['selftest'].append((os.path.basename(os.path.dirname(exe)), provider, os.path.basename(model)))
        if rec.get('fail') == provider:
            raise C.CampaignError(f'{os.path.basename(model)}: the self-test on {provider} and cpu did not pass')
        return {provider: {'max_abs': 1e-3, 'deterministic': True}, 'cpu': {'max_abs': 1e-6, 'deterministic': True}}

    monkeypatch.setattr(C, 'export_variant', export_variant)
    monkeypatch.setattr(C, 'gpu_selftest', gpu_selftest)
    monkeypatch.setattr(C, 'platform_check', lambda st, d, p, log: rec['platform'].append(p))
    return rec


def test_the_release_index_gives_every_route_and_says_what_is_missing(farm):
    _index(farm)
    assert C.release_alternates(farm.base) == ([farm.cuda, farm.dml, farm.mac], [])
    _index(farm, mac='signed', dml=False)
    ids, notes = C.release_alternates(farm.base)
    assert ids == [farm.cuda] and any('DirectML' in n for n in notes) and any('--mac-publish' in n for n in notes)
    ids, notes = C.release_alternates('f' * 24)
    assert ids == [] and 'no published release' in notes[0]
    assert C.alternates_want(None, '') == {'alternates': [], 'variants': ''}
    assert C.alternates_want(f'{farm.cuda},{farm.dml}', 'none')['alternates'] == [farm.cuda, farm.dml]
    with pytest.raises(C.CampaignError, match='build ids'):
        C.alternates_want('nope', '')
    with pytest.raises(C.CampaignError, match='variants'):
        C.alternates_want(C.RELEASE, 'gpu=fp8')


def test_alternates_variants_definition_and_registration(tmp_path, farm, stubs, monkeypatch):
    _index(farm)
    cl = FD.Client('https://farm.test', token='t', transport=farm)
    d, st = _state(tmp_path, farm)
    C.prepare_alternates(cl, d, st, lambda m: None)
    st = C.load_state(d)
    assert st['alternates_ready'] and not st.get('alternates_error')
    assert st['alternates'] == [
        {'build_id': farm.cuda, 'runtime': farm.cuda_rt, 'when': {'vendor': 'nvidia'}, 'model': 'gpu'},
        {'build_id': farm.dml, 'runtime': farm.dml_rt, 'when': {'vendor': 'amd'}, 'model': 'dml'},
        {'build_id': farm.mac, 'runtime': farm.mac_rt, 'when': {'platform': 'macos-arm64'}}]
    assert st['builds'] == [farm.base, farm.cuda, farm.dml, farm.mac]
    assert st['min_agent'] == FD.VARIANT_AGENT, "a run's minimum: the variants' agent, not the DirectML route's"
    assert st['alt_routes'][farm.dml]['min_agent'] == '0.3.11'
    assert stubs['platform'] == ['macos-arm64']
    assert sorted(stubs['export']) == [('fp16', 1), ('fp16', 2), ('matmul', 1), ('matmul', 2)]
    # every graph a GPU route plays, self-tested through that route's own worker and pack; the Mac's never run here
    assert sorted(stubs['selftest']) == [(farm.cuda, 'cuda', 'model-fp16.onnx')] * 2 + [(farm.dml, 'dml', 'model-matmul.onnx')] * 2
    assert set(st['selftests']['1']) == {farm.cuda, farm.dml}
    # the run PUT, the definition, the gens' files and registrations
    body = C.run_body(dict(st, build_game='mtg'))
    assert body['alternates'] == st['alternates'] and body['min_agent'] == FD.VARIANT_AGENT
    fields = C.definition_fields(dict(st, build_game='mtg'))
    assert fields['builds'] == st['builds']
    assert set(fields['checkpoints']['1']['variants']) == {'gpu', 'dml'}
    made = []

    def publish(client, info, **kw):
        made.append(json.load(open(info['path'], encoding='utf-8')))
        return dict(info, published=True)
    monkeypatch.setattr(FD, 'publish', publish)
    C.setup_farm(cl, d, st, lambda m: None, {'defs_dir': str(tmp_path / 'defs')})
    doc = made[0]                                           # (FD.prepare validated it: builds, signed variants)
    assert doc['builds'] == st['builds'] and doc['checkpoints']['2']['variants']['dml']['model_sha256'] == \
        st['gens']['2']['variants']['dml']['sha256']
    puts = [(m, p, json.loads(b) if b and p.startswith('/v1/admin/run/') else None) for m, p, b in farm.calls if m == 'PUT']
    run_puts = [x for x in puts if x[1] == '/v1/admin/run/cmp-alts']
    assert run_puts[0][2]['alternates'] == st['alternates'] and run_puts[0][2]['eval'] is None
    assert run_puts[-1][2]['eval']['batch'] == 'c1' and run_puts[-1][2]['alternates'] == st['alternates']
    arts = [p for m, p, _ in puts if p.startswith('/v1/admin/artifact/')]
    assert '/v1/admin/artifact/ckpt/cmp-alts/1/model-gpu.onnx' in arts and '/v1/admin/artifact/ckpt/cmp-alts/2/model-dml.onnx' in arts
    ck = next(b for m, p, b in puts if p == '/v1/admin/run/cmp-alts/checkpoint/1')
    assert ck['variants'] == {n: {'model': f'ckpt/cmp-alts/1/model-{n}.onnx', 'sha256': st['gens']['1']['variants'][n]['sha256']}
                              for n in ('dml', 'gpu')}


def test_refusals_put_nothing_and_none_is_said(tmp_path, farm, stubs, monkeypatch):
    _index(farm)
    cl = FD.Client('https://farm.test', token='t', transport=farm)
    # another engine's alternate: refused, the reason in the state, nothing put
    d, st = _state(tmp_path / 'a', farm, alternates=[farm.old])
    with pytest.raises(C.CampaignError, match='engine'):
        C.prepare_alternates(cl, d, st, lambda m: None)
    assert 'engine' in C.load_state(d)['alternates_error'] and not C.load_state(d).get('alternates_ready')
    # a GPU self-test that fails refuses the campaign (never a silent CPU-only campaign)
    stubs['fail'] = 'dml'
    d, st = _state(tmp_path / 'b', farm)
    with pytest.raises(C.CampaignError, match='dml'):
        C.prepare_alternates(cl, d, st, lambda m: None)
    assert 'dml' in C.load_state(d)['alternates_error']
    stubs.pop('fail')
    # without variants a GPU route plays (and is self-tested on) the gen's default graph
    stubs['selftest'].clear()
    d, st = _state(tmp_path / 'c', farm, alternates=[farm.cuda], variants='')
    C.prepare_alternates(cl, d, st, lambda m: None)
    st = C.load_state(d)
    assert st['alternates'] == [{'build_id': farm.cuda, 'runtime': farm.cuda_rt, 'when': {'vendor': 'nvidia'}}]
    assert stubs['selftest'] == [(farm.cuda, 'cuda', 'model.onnx')] * 2 and st['min_agent'] == '0.2.11'
    # none: said in the log and the state; the PUT and the definition exactly as before alternates
    for alts, why in ((None, '--no-alternates'), (C.RELEASE, 'no published release')):
        d, st = _state(tmp_path / ('n' + str(alts)), farm, alternates=alts)
        if alts == C.RELEASE:
            st['build_id'] = farm.old
        lines = []
        C.prepare_alternates(cl, d, st, lines.append)
        st = C.load_state(d)
        assert why in st['alternates_note'] and any('CPU worker only' in x for x in lines)
        assert 'alternates' not in C.run_body(dict(st, build_game='mtg'))
        assert C.definition_fields(dict(st, build_game='mtg'))['builds'] == [st['build_id']]
    # settings a Mac cannot replay exactly (a libm refusal): the release's macOS route left out with a note; a caller's
    # list naming it refused
    def libm(st_, d_, p, log):
        raise C.CampaignError(f'the games would replay differently on {p}')
    monkeypatch.setattr(C, 'platform_check', libm)
    d, st = _state(tmp_path / 'm', farm)
    C.prepare_alternates(cl, d, st, lambda m: None)
    st = C.load_state(d)
    assert [x['build_id'] for x in st['alternates']] == [farm.cuda, farm.dml] and farm.mac not in st['builds']
    assert 'left out' in st['alternates_note'] and farm.mac not in st['alt_routes']
    d, st = _state(tmp_path / 'm2', farm, alternates=[farm.cuda, farm.mac])
    with pytest.raises(C.CampaignError, match='replay differently'):
        C.prepare_alternates(cl, d, st, lambda m: None)
    _index(farm, cuda=False, dml=False)                     # (a release whose only route is the refused Mac: none)
    d, st = _state(tmp_path / 'm3', farm)
    lines = []
    C.prepare_alternates(cl, d, st, lines.append)
    st = C.load_state(d)
    assert st['alternates'] == [] and 'left out' in st['alternates_note'] and any('CPU worker only' in x for x in lines)
    _index(farm)
    # a campaign made before alternates (no alt_want): its requests unchanged
    old = {k: v for k, v in st.items() if k not in ('alt_want', 'alternates', 'builds', 'alternates_ready',
                                                     'alternates_note', 'alt_routes', 'variants')}
    assert 'alternates' not in C.run_body(dict(old, build_game='mtg'))


def test_a_route_of_another_platform_is_never_run_here(farm, monkeypatch):
    cl = FD.Client('https://farm.test', token='t', transport=farm)
    assert C.local_build(cl, farm.cuda, 'worker')[1]['requires_runtime'] == farm.cuda_rt
    with pytest.raises(C.CampaignError, match='cannot run it'):
        C.local_build(cl, farm.mac, 'worker')
    with open(os.path.join(farm.dir, farm.dml, 'mtgz-worker.exe'), 'wb') as fh:
        fh.write(b'tampered')
    with pytest.raises(C.CampaignError, match='not the executable'):
        C.local_build(cl, farm.dml, 'worker')
    with pytest.raises(C.CampaignError, match='no copy'):
        C.local_build(cl, 'f' * 24, 'worker')


def test_the_gpu_self_test_reads_probe_providers(monkeypatch):
    def probe(results, providers, rc=0):
        out = json.dumps({'providers': providers, 'results': results})
        monkeypatch.setattr(C.subprocess, 'run', lambda *a, **k: subprocess.CompletedProcess(a, rc, out, 'log'))
    st = lambda ok, m: {'ok': ok, 'selftest': {'max_abs': m, 'deterministic': ok}, 'rows_per_s': 9.0}
    probe([dict(st(True, 4e-3), provider='cuda', gpu={'vendor': 'nvidia'}), dict(st(True, 3e-3), provider='cpu')], ['cuda', 'cpu'])
    out = C.gpu_selftest('w.exe', 'pack', 'cuda', 'model-fp16.onnx', 'ref.npz')
    assert out['cuda'] == {'max_abs': 4e-3, 'deterministic': True, 'rows_per_s': 9.0, 'gpu': 'nvidia'}
    # one of two adapters passing is a pass (the probe tries each)
    probe([dict(st(False, 1.0), provider='dml'), dict(st(True, 4e-6), provider='dml'), dict(st(True, 3e-6), provider='cpu')],
          ['dml', 'cpu'])
    assert C.gpu_selftest('w.exe', 'pack', 'dml', 'm.onnx', 'r.npz')['dml']['max_abs'] == 4e-6
    probe([dict(st(False, 0.9), provider='dml', why='not deterministic'), dict(st(True, 3e-6), provider='cpu')], ['cpu'])
    with pytest.raises(C.CampaignError, match='not deterministic'):
        C.gpu_selftest('w.exe', 'pack', 'dml', 'm.onnx', 'r.npz')
    monkeypatch.setattr(C.subprocess, 'run', lambda *a, **k: subprocess.CompletedProcess(a, 1, '', 'cudnn missing'))
    with pytest.raises(C.CampaignError, match='cudnn missing'):
        C.gpu_selftest('w.exe', 'pack', 'cuda', 'm.onnx', 'r.npz')
