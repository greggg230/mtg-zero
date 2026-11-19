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
