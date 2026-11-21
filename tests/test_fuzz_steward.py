"""The fuzz steward's pacing and decisions (tools/fuzz_steward.py, docs/ENGINE.md 171): pure functions only - no
coordinator, Mac, process or run is touched."""
import hashlib
import os
import re
import sys
import xml.dom.minidom

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import fuzz_steward as S  # noqa: E402
import fuzz_farm as FF  # noqa: E402

CFG = dict(S.CONFIG)
D = 1_794_787_200          # a UTC midnight (2026-11-16)
Q = 50_000


def test_day_start_and_name():
    assert D % S.DAY == 0
    assert S.day_start(D + 3600) == D and S.day_start(D - 1) == D - S.DAY
    assert S.day_name(D) == '2026-11-16' and S.day_name(D - 1) == '2026-11-15'


def test_pace_is_uniform_through_the_day():
    assert S.pace_target(Q, D, 0) == 0
    assert S.pace_target(Q, D + 6 * 3600, 0) == Q / 4
    assert S.pace_target(Q, D + 12 * 3600, 0) == Q / 2
    assert abs(S.pace_target(Q, D + S.DAY - 1, 0) - Q) < 1
    assert S.day_quota(Q, D + 5, 0) == Q


def test_a_builds_first_day_is_prorated_from_its_start():
    start = D + 18 * 3600                      # built at 18:00 UTC
    assert S.day_quota(Q, start + 60, start) == Q / 4
    assert S.pace_target(Q, start, start) == 0
    assert S.pace_target(Q, start + 3 * 3600, start) == Q / 8
    # the next day it has the whole quota from midnight
    assert S.day_quota(Q, D + S.DAY + 1, start) == Q
    assert S.pace_target(Q, D + S.DAY + 6 * 3600, start) == Q / 4


def test_deficit_hours_counts_in_flight_seeds():
    t = D + 12 * 3600                          # pace 25,000
    assert S.deficit_hours(Q, t, 0, 25_000) == 0
    assert abs(S.deficit_hours(Q, t, 0, 25_000 - Q / 24) - 1) < 1e-9
    assert S.deficit_hours(Q, t, 0, 20_000, in_flight=5_000) == 0
    assert S.deficit_hours(Q, t, 0, 30_000) < 0


def test_mode_quotas_and_the_most_behind_mode():
    modes = S.DELVER_RUN_MODES
    qs = S.mode_quotas(modes, Q)
    assert abs(sum(qs.values()) - Q) < 1e-6 and qs['dd2g'] == Q * 0.5
    t = D + 12 * 3600
    done = {'dd2g': 12_500, 'dd2m': 6_250, 'g': 3_750, 'hg': 0}      # every mode on pace but hidden-games
    alias, h = S.most_behind(modes, Q, t, 0, done)
    assert alias == 'hg' and abs(h - 12) < 1e-9
    alias, _ = S.most_behind(modes, Q, t, 0, done, {'hg': 2_500})   # in flight counts
    assert alias in ('dd2g', 'dd2m', 'g', 'hg')
    assert S.most_behind(modes, Q, t, 0, done, {'hg': 2_500})[1] == 0
    # equals: the first listed; the boosted mode is kept unless another is clearly further behind
    assert S.most_behind(modes, Q, t, 0, {})[0] == 'dd2g'
    assert S.most_behind(modes, Q, t, 0, {}, current='g')[0] == 'g'
    assert S.most_behind(modes, Q, t, 0, {'g': 3_750}, current='g')[0] == 'dd2g'        # g on pace, the rest 12 h behind


def test_the_farm_share_levels_and_no_flapping():
    c = CFG
    assert S.farm_share(0, None, c) is None and S.farm_share(-3, c['boost_share'], c) is None
    assert S.farm_share(0.5, None, c) == c['side_share']
    assert S.farm_share(1.5, None, c) == c['boost_share']
    # boosted stays boosted until back on pace
    assert S.farm_share(0.2, c['boost_share'], c) == c['boost_share']
    assert S.farm_share(0.2, c['side_share'], c) == c['side_share']
    assert c['side_share'] == 0.25 and c['boost_share'] > c['side_share']


def test_local_fill_in_mac_first_then_the_desktop():
    c = CFG
    now = D + 10 * 3600
    # on pace or a little behind: nothing local
    assert S.local_decision(0, now, now - 9999, None, 3, c) == {'mac': False, 'desktop': False}
    assert S.local_decision(1.5, now, now - 9999, None, 3, c) == {'mac': False, 'desktop': False}
    # behind, but the farm boost is new: give it time
    assert S.local_decision(2.5, now, now - 60, None, 3, c)['mac'] is False
    # behind and boosted long enough: the Mac
    assert S.local_decision(2.5, now, now - c['local_after_s'], None, 3, c) == {'mac': True, 'desktop': False}
    # no farm PC online: the Mac at once
    assert S.local_decision(2.5, now, None, None, 0, c)['mac'] is True
    # far behind, the Mac at work long enough: the desktop too ...
    d = S.local_decision(3.5, now, now - 9999, now - c['local_after_s'], 0, c)
    assert d == {'mac': True, 'desktop': True}
    # ... but not while a live run is slowed, nor before the Mac has had its time, nor when turned off
    assert S.local_decision(3.5, now, now - 9999, now - c['local_after_s'], 0, c, desktop_ok=False)['desktop'] is False
    assert S.local_decision(3.5, now, now - 9999, now - 60, 0, c)['desktop'] is False
    assert S.local_decision(3.5, now, now - 9999, now - 9999, 0, dict(c, desktop=False))['desktop'] is False
    assert S.local_decision(3.5, now, now - 9999, None, 0, dict(c, mac=False)) == {'mac': False, 'desktop': False}


def test_starved_compares_with_the_median_before_local_work():
    secs = [400, 420, 410, 430, 405, 415, 425]
    assert not S.starved(secs + [440], 1.10, 10)                 # +6%
    assert S.starved(secs + [470], 1.10, 10)                     # +14%
    assert S.starved(secs + [420], 1.10, 10, current_elapsed=480)  # the iteration in progress is already long
    assert not S.starved(secs, 1.10, 10, current_elapsed=100)
    assert S.starved([500, 500, 500], 1.10, 10, baseline=400.0)  # the baseline from before the desktop began
    assert not S.starved([400, 410], 1.10, 10)                   # too little history: never starved


def test_ranges():
    r = S.add_range([], 10, 20)
    r = S.add_range(r, 30, 40)
    r = S.add_range(r, 20, 30)
    assert r == [[10, 40]] and S.count_ranges(r) == 30
    assert S.count_ranges(S.add_range([[0, 5]], 10, 12)) == 7


def test_failure_ids_are_the_fuzz_farms():
    site, msg = 'engine/src/pay.rs:123', ['pending answer 17 for slot 3', 'board: ...']
    fid, key = S.failure_id(site, msg)
    assert key == FF.failure_key(site, msg) and fid == hashlib.sha1(key.encode()).hexdigest()[:12]
    assert S.failure_id(site, ['pending answer 99 for slot 4'])[0] == fid      # digits masked
    assert S.failure_id('engine/src/pay.rs:124', msg)[0] != fid


def test_campaign_names_are_coordinator_slugs():
    for target in ('main', 'delver-bo3-pb3', 'Some_Run.Name-with-a-very-long-name-that-goes-on-and-on'):
        n = S.campaign_name(target, 'b309fa08e252da135d4213811094894fefa8c6b1', 'dd2m')
        assert re.fullmatch(r'[a-z0-9][a-z0-9-]{0,62}', n), n
    assert S.campaign_name('main', 'ebf322d5aaaa', 'g') == 'fz-main-ebf322d-g'


def test_the_alert_is_short_and_names_no_machine():
    e = {'id': 'abcdef012345', 'kind': 'fuzz', 'mode': 'delver_dealt2-games', 'seed': 100012345, 'target': 'main',
         'engine_commit': 'ebf322d5' + '0' * 32, 'site': 'C:\\Users\\someone\\x\\engine\\src\\pay.rs:12'}
    t = S.alert_text(e)
    assert len(t) < 200 and 'pay.rs:12' in t and 'abcdef012345' in t
    assert 'Users' not in t and 'someone' not in t
    t2 = S.alert_text(dict(e, kind='selfplay', target='delver-bo3-pb3'))
    assert 'self-play (delver-bo3-pb3)' in t2


def test_the_logon_task_runs_forever_as_this_user():
    x = S.task_xml(['C:/py/pythonw.exe', 'C:/r/tools/fuzz_steward.py', 'run', '--quiet'], 'C:/r')
    doc = xml.dom.minidom.parseString(x.replace('encoding="UTF-16"', 'encoding="UTF-8"').encode())
    get = lambda tag: doc.getElementsByTagName(tag)[0].firstChild.data  # noqa: E731
    assert get('ExecutionTimeLimit') == 'PT0S' and get('LogonType') == 'InteractiveToken'
    assert get('RunLevel') == 'LeastPrivilege' and get('MultipleInstancesPolicy') == 'IgnoreNew'
    assert get('Command') == 'C:/py/pythonw.exe' and 'fuzz_steward.py run --quiet' in get('Arguments')
    assert doc.getElementsByTagName('LogonTrigger')


def test_a_selfplay_repro_is_confirmed_only_on_exit_101(tmp_path):
    """docs/ENGINE.md 172's replay: 101 reproduced, 0 not, 2 cannot replay (no process but the fake repro is started)."""
    import types
    a = types.SimpleNamespace(state=str(tmp_path / 'st'), quiet=True, dry_run=True, once=True, status_file=None,
                              runs_glob=str(tmp_path / 'runs' / '*' / 'supervisor_state.json'))
    st = S.Steward(a)
    st.state['targets']['r'] = {'run_wt': str(tmp_path)}
    for code, want in ((101, True), (0, False), (2, None), (1, None)):
        e = {'kind': 'selfplay', 'target': 'r', 'sources': ['pool'], 'repro': f'python -c "raise SystemExit({code})"'}
        assert st.confirm(e)[0] is want, code
    # a record of 167.6 (no repro): confirmed when it happened on the owner's machines, else unknown
    assert st.confirm({'kind': 'selfplay', 'target': 'r', 'sources': ['local'], 'repro': None})[0] is True
    assert st.confirm({'kind': 'selfplay', 'target': 'r', 'sources': ['pool'], 'repro': None})[0] is None


def test_a_chunk_failure_is_replayed_over_its_whole_chunk(tmp_path, monkeypatch):
    """docs/ENGINE.md 209: a failure of a chunk's own checks after its last seed (the rules checks' verdict, what the mode
    must have exercised) is reported at the chunk's first seed with "(the chunk: FUZZ_GAMES=n)"; the steward replayed that
    one seed, which passes (13f02539e054, e940641803b8). The replay is the whole chunk, cut as one chunk of its seeds."""
    import json
    import subprocess
    import types
    assert S.chunk_count('(at 6a44afdbf1ef) FUZZ_FIRST=100042326 FUZZ_GAMES=1 FUZZ_LOG=1 cargo test --profile fuzz -p engine '
                         '--test fuzz fuzz_random_games -- --exact --nocapture (the chunk: FUZZ_GAMES=1000)') == 1000
    assert S.chunk_count('FUZZ_POOL=delver_dealt2 FUZZ_FIRST=1 FUZZ_GAMES=1 cargo test (the chunk: FUZZ_GAMES=239)') == 239
    assert S.chunk_count('DRAFT_FUZZ_FIRST=5 DRAFT_FUZZ=1 cargo test (the chunk: DRAFT_FUZZ=50)') == 50
    assert S.chunk_count('FUZZ_FIRST=100061919 FUZZ_GAMES=1 FUZZ_LOG=1 cargo test') == 1
    assert S.chunk_count(None) == 1
    a = types.SimpleNamespace(state=str(tmp_path / 'st'), quiet=True, dry_run=True, once=True, status_file=None,
                              runs_glob=str(tmp_path / 'runs' / '*' / 'supervisor_state.json'))
    st = S.Steward(a)
    mode = dict(S.MAIN_MODES[0])
    st.state['targets']['main'] = {'build_id': 'b1', 'exe': str(tmp_path / 'mtgz-worker.exe'), 'modes': [mode]}
    seen = []

    def fake_run(argv, **kw):
        seen.append(argv)
        f = {'seed': 100042326, 'mode': 'games', 'site': 'engine/src/fuzz/games.rs:360', 'panic': 'x', 'repro': 'r'}
        return types.SimpleNamespace(returncode=0, stdout='FUZZ_LOCAL_FAILURE ' + json.dumps(f) + '\n', stderr='')

    monkeypatch.setattr(subprocess, 'run', fake_run)
    e = {'kind': 'fuzz', 'id': 'x1', 'target': 'main', 'build': 'b1', 'mode': 'games', 'seed': 100042326,
         'site': 'engine/src/fuzz/games.rs:360', 'repro': 'FUZZ_FIRST=100042326 FUZZ_GAMES=1 cargo test (the chunk: FUZZ_GAMES=1000)'}
    ok, note = st.confirm(e)
    assert ok is True and 'the chunk of 1000 seeds from 100042326' in note, note
    argv = seen[-1]
    val = lambda k: argv[argv.index(k) + 1]  # noqa: E731
    assert (val('--first'), val('--count'), val('--chunk'), val('--threads')) == ('100042326', '1000', '1000', '1')
    # a seed's own failure: that seed alone
    st.confirm(dict(e, repro='FUZZ_FIRST=100042326 FUZZ_GAMES=1 cargo test', site='engine/src/hidden.rs:1336'))
    argv = seen[-1]
    assert (val('--count'), val('--chunk')) == ('1', '1')


# ---------------------------------------------------------------- docs/ENGINE.md 219: one engine, one set; the Macs

def test_a_crash_site_is_the_same_on_windows_and_a_mac():
    """zero-core's exit_code_text writes a Windows crash as the NTSTATUS and a Unix one as -signal: the steward keys
    both to one name (the codes the worker's test faults gave on each platform, docs/ENGINE.md 219.3)."""
    so = ["thread 'main' has overflowed its stack", 'fatal runtime error: stack overflow']
    assert S.neutral_site('crash exit 3221225725', so) == S.neutral_site('crash exit -6', so) == 'crash stack overflow'
    assert S.neutral_site('crash exit 3221226505', []) == S.neutral_site('crash exit -6', []) == 'crash abort'
    assert S.neutral_site('crash exit 3221225477') == S.neutral_site('crash exit -11') == 'crash access violation'
    assert S.neutral_site('crash exit 101') == 'crash exit 101'           # (unknown codes stay as they are)
    for site in ('engine/src/hidden.rs:1336', 'HANG games', 'unknown', None):
        assert S.neutral_site(site, so) == site
    assert S.failure_id('crash exit 3221226505', ['x'])[0] == S.failure_id('crash exit -6', ['x'])[0]
    assert S.failure_id('crash exit 3221226505', ['x'])[0] != S.failure_id('crash exit 3221225477', ['x'])[0]


def _steward(tmp_path, dry=False):
    import types
    a = types.SimpleNamespace(state=str(tmp_path / 'st'), quiet=True, dry_run=dry, once=True, status_file=None,
                              runs_glob=str(tmp_path / 'runs' / '*' / 'supervisor_state.json'))
    return S.Steward(a)


def _targets(st, monkeypatch, found, hashes):
    """discover() answers `found` ({key: commit}); engine_hash by `hashes` ({commit: hash})."""
    monkeypatch.setattr(st, 'discover', lambda: {k: {'commit': c, 'kind': 'main' if k == 'main' else 'run',
                                                     'modes': S.MAIN_MODES, 'run_dir': None, 'run_wt': None}
                                                 for k, c in found.items()})
    monkeypatch.setattr(S, 'engine_hash', lambda c, cwd=None: hashes[c])


def _fake_farm(st, monkeypatch):
    """Builds found at once (one per engine), campaigns 'made' in memory, every put recorded."""
    puts, made = [], []
    monkeypatch.setattr(st, 'find_build', lambda c, platform='windows-x64': (
        (f'{platform[:3]}{S.engine_hash(c)}'.ljust(24, '0')[:24], f'X:/{platform}/{c}/mtgz-worker') ))

    def ensure(key, cur):
        for m in cur['modes']:
            name = S.campaign_name(key, cur['commit'], m['alias'])
            if cur['campaigns'].get(m['alias']) != name:
                cur['campaigns'][m['alias']] = name
                made.append(name)
    monkeypatch.setattr(st, 'ensure_campaigns', ensure)
    monkeypatch.setattr(st, 'put_campaign', lambda name, **ch: puts.append((name, ch)) or True)
    monkeypatch.setattr(st, 'stop_local', lambda *a, **k: None)
    return puts, made


def test_targets_of_one_engine_share_one_set_of_campaigns(tmp_path, monkeypatch):
    """main and the arms pinned to its engine: main's campaigns only (the arms follow); an owner's run that ends hands
    its build and campaigns to a follower; a later run of a retired owner's engine resumes those campaigns instead of
    making four new ones (docs/ENGINE.md 219.1)."""
    st = _steward(tmp_path)
    puts, made = _fake_farm(st, monkeypatch)
    H = {'m1': 'E1', 'a1': 'E1', 'a2': 'E1', 'r1': 'E2', 'r2': 'E2', 'r3': 'E2'}
    _targets(st, monkeypatch, {'main': 'm1', 'arm-a': 'a1', 'arm-b': 'a2'}, H)
    st.ensure_targets(1000)
    T = st.state['targets']
    assert len(made) == 4 and all(n.startswith('fz-main-') for n in made)
    assert T['arm-a']['follows'] == 'main' and T['arm-b']['follows'] == 'main' and not T['arm-a']['campaigns']
    # a run on another engine owns its own; a second run of that engine follows it
    _targets(st, monkeypatch, {'main': 'm1', 'arm-a': 'a1', 'arm-b': 'a2', 'r-one': 'r1', 'r-two': 'r2'}, H)
    st.ensure_targets(2000)
    assert len(made) == 8 and T['r-two']['follows'] == 'r-one' and T['r-one']['campaigns']
    # r-one ends: r-two takes its build and campaigns over (nothing paused, nothing new)
    owned = dict(T['r-one']['campaigns'])
    _targets(st, monkeypatch, {'main': 'm1', 'arm-a': 'a1', 'arm-b': 'a2', 'r-two': 'r2'}, H)
    st.ensure_targets(3000)
    assert T['r-one']['retired'] == 3000 and T['r-one']['handed_to'] == 'r-two' and not T['r-one']['campaigns']
    assert T['r-two']['campaigns'] == owned and not T['r-two'].get('follows') and T['r-two']['build_id'] == T['r-one']['build_id']
    assert len(made) == 8 and not [p for p in puts if p[1].get('state') == 'paused']
    # r-two ends too (no follower left): its campaigns are paused
    _targets(st, monkeypatch, {'main': 'm1', 'arm-a': 'a1', 'arm-b': 'a2'}, H)
    st.ensure_targets(4000)
    assert sorted(p[0] for p in puts if p[1].get('state') == 'paused') == sorted(owned.values())
    # r-three on that engine later: it takes those campaigns back and resumes them
    _targets(st, monkeypatch, {'main': 'm1', 'arm-a': 'a1', 'arm-b': 'a2', 'r-three': 'r3'}, H)
    st.ensure_targets(5000)
    assert T['r-three']['campaigns'] == owned and len(made) == 8
    assert sorted(p[0] for p in puts if p[1].get('state') == 'active') == sorted(owned.values())
    # an arm moved to another engine builds its own; check() skips followers (one pace per engine)
    H['a9'] = 'E9'
    _targets(st, monkeypatch, {'main': 'm1', 'arm-a': 'a9', 'arm-b': 'a2', 'r-three': 'r3'}, H)
    st.ensure_targets(6000)
    assert not T['arm-a'].get('follows') and T['arm-a']['engine_hash'] == 'E9' and len(made) == 12
    assert T['arm-b']['follows'] == 'main'


def test_two_owners_of_one_engine_from_an_older_state_merge(tmp_path, monkeypatch):
    """A state from before docs/ENGINE.md 219 (main and a run on one engine, four campaigns each): main's stay, the
    run's are paused and it follows main."""
    st = _steward(tmp_path)
    puts, made = _fake_farm(st, monkeypatch)
    camps = lambda k: {m['alias']: f'fz-{k}-abc-{m["alias"]}' for m in S.MAIN_MODES}  # noqa: E731
    for k in ('main', 'r1'):
        st.state['targets'][k] = {'commit': 'c1', 'engine_hash': 'E1', 'build_id': 'b' * 24, 'built_at': 1 if k == 'r1' else 2,
                                  'modes': S.MAIN_MODES, 'campaigns': camps(k), 'shares': {}, 'kind': 'run'}
    _targets(st, monkeypatch, {'main': 'c1', 'r1': 'c1'}, {'c1': 'E1'})
    st.ensure_targets(1000)
    T = st.state['targets']
    assert T['r1']['follows'] == 'main' and not T['r1']['campaigns'] and T['main']['campaigns'] == camps('main')
    assert sorted(p[0] for p in puts if p[1].get('state') == 'paused') == sorted(camps('r1').values())
    assert made == []


def test_find_build_never_takes_another_platforms_build(tmp_path, monkeypatch):
    """A macOS fuzz build of the commit beside the Windows one: each platform finds its own (docs/ENGINE.md 219)."""
    import json as J
    big = tmp_path / 'big'
    monkeypatch.setattr(S.local_config, 'big_disk_path', lambda *p: str(big.joinpath(*p)))
    monkeypatch.setattr(S, 'engine_hash', lambda c, cwd=None: c[:4])
    ids = {}
    for plat, entry, where in (('macos-arm64', 'mtgz-worker', ('cargo-target', 'mtg-zero', 'aaa', 'farm-build', 'dist', 'm')),
                               (None, 'mtgz-worker.exe', ('cargo-target', 'mtg-zero', 'zzz', 'farm-build', 'dist', 'w'))):
        d = big.joinpath(*where)
        d.mkdir(parents=True)
        (d / entry).write_bytes(b'x')
        man = {'kind': 'worker', 'profile': 'fuzz', 'engine_commit': 'c' * 40, 'entry': entry, **({'platform': plat} if plat else {})}
        raw = J.dumps(man).encode()
        (d / 'manifest.json').write_bytes(raw)
        ids[plat or 'windows-x64'] = hashlib.sha256(raw).hexdigest()[:24]
    st = _steward(tmp_path)
    assert st.find_build('c' * 40)[0] == ids['windows-x64']
    assert st.find_build('c' * 40, 'macos-arm64')[0] == ids['macos-arm64']
    # the release's copies of registered builds (<big disk>/zero-farm-builds/<id>/) are looked in too
    reg = big / 'zero-farm-builds' / 'r'
    reg.mkdir(parents=True)
    (reg / 'mtgz-worker').write_bytes(b'x')
    raw = J.dumps({'kind': 'worker', 'profile': 'fuzz', 'engine_commit': 'd' * 40, 'entry': 'mtgz-worker',
                   'platform': 'macos-arm64'}).encode()
    (reg / 'manifest.json').write_bytes(raw)
    assert st.find_build('d' * 40, 'macos-arm64')[0] == hashlib.sha256(raw).hexdigest()[:24]
    assert st.find_build('d' * 40) == (None, None)


def test_the_owner_gets_a_macos_build_and_its_campaigns_list_it(tmp_path, monkeypatch):
    """docs/ENGINE.md 219.2: no macOS fuzz build of the engine -> one is built (on the Mac through farm_build.py); a
    failure is said and tried again only after mac_retry_h; once ready, every campaign whose mode macOS may play gets it
    (farm_campaign.py update --alternate), the libm-refused mode stays Windows-only; a failure on a Mac's build keys
    to its platform."""
    import types
    st = _steward(tmp_path)
    monkeypatch.setattr(S.local_config, 'get', lambda k, d=None: 'user@mac' if k == 'MTGZ_MAC' else d)
    monkeypatch.setattr(st, 'checkout', lambda key, commit: str(tmp_path))
    spawned, runs = [], []
    monkeypatch.setattr(st, 'spawn', lambda jid, argv, cwd, log, env=None: spawned.append((jid, argv)) or
                        st.procs.__setitem__(jid, types.SimpleNamespace(poll=lambda: 0, returncode=0)))
    mac = {'have': None}
    monkeypatch.setattr(st, 'find_build', lambda c, platform='windows-x64': (mac['have'], 'X:/m/mtgz-worker')
                        if platform == 'macos-arm64' and mac['have'] else (None, None))

    def run(argv, **kw):
        runs.append(argv)
        refused = 'fuzz-check' in argv and 'delver_dealt2-matches' in argv
        return types.SimpleNamespace(returncode=1 if refused else 0, stdout='', stderr='refused: the libm' if refused else '')
    monkeypatch.setattr(S.subprocess, 'run', run)
    cur = {'commit': 'c' * 40, 'engine_hash': 'E1', 'build_id': 'w' * 24, 'exe': 'X:/w/mtgz-worker.exe', 'modes': S.MAIN_MODES,
           'campaigns': {m['alias']: f'fz-main-ccccccc-{m["alias"]}' for m in S.MAIN_MODES}, 'shares': {}}
    st.state['targets']['main'] = cur
    st.ensure_mac('main', cur, 1000)
    assert cur['mac']['state'] == 'building' and spawned[-1][0] == 'macbuild:main'
    assert spawned[-1][1][-7:] == ['--profile', 'fuzz', '--platform', 'macos-arm64', '--min-agent', '0.3.0', '--publish']
    st.poll_mac_builds(1100)                       # the build ended with no build found: failed, said
    assert cur['mac']['state'] == 'failed' and cur['mac']['failed_at'] == 1100
    st.ensure_mac('main', cur, 1200)               # not again before mac_retry_h
    assert len(spawned) == 1
    st.ensure_mac('main', cur, 1100 + 6 * 3600)
    assert len(spawned) == 2 and cur['mac']['tries'] == 2
    mac['have'] = 'm' * 24
    st.poll_mac_builds(30000)
    assert cur['mac'] == {'state': 'ready', 'build_id': 'm' * 24, 'dir': 'X:/m', 'since': 30000}
    assert 'm' * 24 in st.state['published']
    st.ensure_mac('main', cur, 30001)
    updates = [r for r in runs if 'update' in r]
    assert sorted(r[r.index('update') + 1] for r in updates) == sorted(f'fz-main-ccccccc-{a}' for a in ('g', 'hg', 'dd2g'))
    assert all(r[-2:] == ['--alternate', 'm' * 24] for r in updates)
    assert sorted(cur['mac_alts']) == ['dd2g', 'g', 'hg'] and cur['mac_modes']['dd2m'] != True  # noqa: E712
    n = len(runs)
    st.ensure_mac('main', cur, 30002)              # nothing more to do: no fuzz-check, no update
    assert len(runs) == n
    assert st.platform_of_build(cur, 'm' * 24) == 'macos-arm64' and st.platform_of_build(cur, 'w' * 24) == 'windows-x64'
    # a follower never builds one
    st.state['targets']['arm'] = {'follows': 'main', 'commit': 'c' * 40, 'build_id': 'w' * 24, 'modes': S.MAIN_MODES}
    st.ensure_mac('arm', st.state['targets']['arm'], 40000)
    assert len(spawned) == 2 and 'mac' not in st.state['targets']['arm']


def test_injected_faults_key_the_same_on_windows_and_a_mac():
    """docs/ENGINE.md 219.3, measured: the worker's test faults (a panic, an abort, a stack overflow, a hang at seeds
    502/505/508/511) in a Windows fuzz build and a macOS one of the same commit - each side's fuzz-final.json failures
    as recorded (the panic's path separators and the crash exit codes differ) key to the same steward ids;
    tools/farm_gates.py's part (d) passes them and fails a real difference."""
    import farm_gates as G
    rep = lambda s: f'(at 1e7416c5b4a3) ZERO_FARM_TEST_FAULT={G.fault_spec(500)} FUZZ_FIRST={s} FUZZ_GAMES=1 cargo test'  # noqa: E731
    hang = 'no exit after 20 s of running time (CPU; the limit is 20 s a seed): seeds 511..511'
    win = {'mode': 'games', 'failures': [
        {'seed': 502, 'site': 'third_party/zero-core/crates/worker/src/fuzz.rs:346', 'repro': rep(502),
         'panic': "thread 'zero-worker-main' panicked at third_party\\zero-core\\crates\\worker\\src\\fuzz.rs:346:49:\n"
                  'ZERO_FARM_TEST_FAULT: an injected panic at seed 502'},
        {'seed': 505, 'site': 'crash exit 3221226505', 'panic': '', 'repro': rep(505)},
        {'seed': 508, 'site': 'crash exit 3221225725', 'panic': "thread 'zero-worker-main' (167152) has overflowed its stack",
         'repro': rep(508)},
        {'seed': 511, 'site': 'HANG games', 'panic': hang, 'repro': rep(511)}]}
    mac = {'mode': 'games', 'failures': [
        {'seed': 502, 'site': 'third_party/zero-core/crates/worker/src/fuzz.rs:346', 'repro': rep(502),
         'panic': "thread 'zero-worker-main' panicked at third_party/zero-core/crates/worker/src/fuzz.rs:346:49:\n"
                  'ZERO_FARM_TEST_FAULT: an injected panic at seed 502'},
        {'seed': 505, 'site': 'crash exit -6', 'panic': '', 'repro': rep(505)},
        {'seed': 508, 'site': 'crash exit -6', 'repro': rep(508),
         'panic': "thread 'zero-worker-main' (61545387) has overflowed its stack\nfatal runtime error: stack overflow, aborting"},
        {'seed': 511, 'site': 'HANG games', 'panic': hang, 'repro': rep(511)}]}
    here, there = G.fault_failures(win), G.fault_failures(mac)
    res = G.compare_fault_failures(here, there, 500)
    assert res['passed'] and res['different'] == [], res
    assert [here[s]['neutral'] for s in sorted(here)] == ['third_party/zero-core/crates/worker/src/fuzz.rs:346', 'crash abort',
                                                         'crash stack overflow', 'HANG games']
    # (the raw sites alone - the key before docs/ENGINE.md 219 - differ for both crashes)
    old = lambda f: FF.failure_key(f['site'], S.failure_message(f['panic']))  # noqa: E731
    assert [old(a) == old(b) for a, b in zip(win['failures'], mac['failures'])] == [True, False, False, True]
    # a real difference fails: another site, a missing failure, an extra one
    bad = dict(mac, failures=[dict(mac['failures'][0], site='engine/src/hidden.rs:1')] + mac['failures'][1:])
    assert not G.compare_fault_failures(here, G.fault_failures(bad), 500)['passed']
    assert not G.compare_fault_failures(here, G.fault_failures(dict(mac, failures=mac['failures'][:3])), 500)['passed']
    extra = dict(mac, failures=mac['failures'] + [dict(mac['failures'][0], seed=503)])
    assert G.compare_fault_failures(here, G.fault_failures(extra), 500)['different'] == ['503']
