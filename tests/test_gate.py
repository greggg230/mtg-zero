"""tools/gate.py's optin-core build-only step (docs/ENGINE.md 90): it names the other games explicitly
(found beside the main checkout from any worktree), requires an "ok <game>: build" line for each, and
never shares this repo's cargo target dir. Nothing is built here."""
import os
import sys
import types

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import gate as G  # noqa: E402

GAMES = ('glassroad-zero', 'caverna-zero', 'kingdombuilder-zero')
PIN = 'aa10733df214c6464ca5c940018b9b41712b54c1'

# what the gate's step printed on e098aff (smoke_all found no game from the worktree's submodule)
NO_GAMES = """=== core: cargo test
test result: ok. 123 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.54s

=== smoke_all summary (core aa10733)
  ok    core: cargo test (4 s)
  ok    core: build zero._engine (17 s)
  ok    core: pytest (703 s)
GREEN
"""


def built(names, core='aa10733', fail=()):
    lines = [f'=== {n}: worktree E:/zero-smoke/x/20261104-082647-{n}-wt with zero-core {core}' for n in names]
    lines += ['', '=== smoke_all summary (core aa10733)']
    for n in names:
        lines.append(f'  {"FAIL" if n in fail else "ok  "}  {n}: build {n[:2]}z._engine (30 s)')
        lines.append(f'  skip  {n}: cargo test, pytest, smoke run (--build-only)')
    lines.append(f'  games: {len(names)} of {len(names)} checked ({" ".join(names)})')
    lines.append(f'{len(fail)} step(s) FAILED' if fail else 'GREEN')
    return '\n'.join(lines) + '\n'


def test_the_checker_wants_every_game_built():
    check = G.make_build_only_checker(list(GAMES), PIN)
    ok, reason, _ = check(NO_GAMES, 0)
    assert not ok and 'glassroad-zero, caverna-zero, kingdombuilder-zero' in reason and 'built: none' in reason
    assert check(built(GAMES), 0)[0]
    ok, reason, _ = check(built(GAMES[:2]), 0)
    assert not ok and 'kingdombuilder-zero' in reason
    assert not check(built(GAMES, fail=('caverna-zero',)), 1)[0]
    assert not check(built(GAMES), 1)[0]                       # every game built, but some step failed
    ok, reason, _ = check(built(GAMES, core='1234567'), 0)     # built against another core
    assert not ok and 'with zero-core aa10733df214' in reason


@pytest.fixture
def fake_main(tmp_path, monkeypatch):
    """This checkout as a worktree whose main checkout is tmp/mtg-zero (its .git the common dir)."""
    monkeypatch.setattr(G, 'ROOT', str(tmp_path / 'mtg-zero-wt' / 'gate'))
    monkeypatch.setattr(G, 'git', lambda args, cwd=None, check=True: str(tmp_path / 'mtg-zero' / '.git')
                        if '--git-common-dir' in args and cwd is None else '')
    for g in GAMES:
        (tmp_path / g / 'third_party' / 'zero-core').mkdir(parents=True)
    return tmp_path


def test_default_games_are_found_beside_the_main_checkout(fake_main):
    assert G.default_other_games() == [str(fake_main / g) for g in GAMES]
    os.rename(fake_main / GAMES[2], fake_main / 'moved')
    with pytest.raises(G.GateError, match='kingdombuilder-zero'):
        G.default_other_games()


def test_the_step_names_the_games_and_has_its_own_targets(fake_main, tmp_path):
    ctx = types.SimpleNamespace(core_pin=PIN)
    step = G.build_only_step(ctx, types.SimpleNamespace(other_games=None))
    assert step.argv[-3:] == [str(fake_main / g) for g in GAMES] and '--with-this-core' in step.argv
    assert step.env['SKIP_CORE'] == '1' and step.env['SMOKE_TARGET'] == 'mtgz-gate-gate'
    assert step.env['SMOKE_TARGET'] in step.env['SMOKE_DIR'] and step.env['CARGO_TARGET_DIR'] is None
    with pytest.raises(G.GateError, match='no such checkout'):
        G.build_only_step(ctx, types.SimpleNamespace(other_games=[str(tmp_path / 'nope')]))


def test_a_none_env_value_unsets_the_variable(tmp_path):
    ctx = types.SimpleNamespace(logs_dir=str(tmp_path), sha_short='0' * 12, tier='t', root=str(tmp_path),
                                step_timeout=60, base_env={'CARGO_TARGET_DIR': 'E:/shared', 'SMOKE_X': 'a'})
    probe = [sys.executable, '-c', 'import os; print("ctd=%s x=%s" % (os.environ.get("CARGO_TARGET_DIR"), '
             'os.environ.get("SMOKE_X")))']
    seen = {}
    step = G.Step('probe', probe, env={'CARGO_TARGET_DIR': None},
                  checker=lambda out, rc: (seen.setdefault('out', out) is not None, '', {}))
    assert G.run_step(ctx, step).passed
    assert 'ctd=None x=a' in seen['out']
