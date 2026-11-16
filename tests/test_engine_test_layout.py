"""engine/tests' binaries (docs/ENGINE.md 167.9): `autotests = false`, so a test file runs only if it is a `[[test]]` target of
engine/Cargo.toml or a `#[path = "x.rs"] mod x;` module of one of the suites. A new test file in neither would compile
nowhere and pass silently (a branch from before 165 merged after it adds files the old way): this fails loudly instead,
naming the file and where it goes."""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TESTS = os.path.join(ROOT, 'engine', 'tests')


def targets():
    with open(os.path.join(ROOT, 'engine', 'Cargo.toml'), encoding='utf-8') as f:
        toml = f.read()
    assert re.search(r'^autotests\s*=\s*false', toml, re.M), 'engine/Cargo.toml: autotests = false (ENGINE.md 167.9)'
    return dict(re.findall(r'\[\[test\]\]\s*name\s*=\s*"([^"]+)"\s*path\s*=\s*"tests/([^"]+)\.rs"', toml))


def test_every_test_file_is_a_target_or_a_suite_module():
    t = targets()
    assert all(name == path for name, path in t.items()), f'a [[test]] named unlike its file: {t}'
    modules = {}
    for suite in (n for n in t if n.endswith('_suite')):
        with open(os.path.join(TESTS, f'{suite}.rs'), encoding='utf-8') as f:
            for path, mod in re.findall(r'#\[path = "([^"]+)\.rs"\]\s*mod (\w+);', f.read()):
                assert path == mod, f'{suite}: #[path = "{path}.rs"] mod {mod}'
                assert mod not in modules, f'{mod} is a module of both {modules.get(mod)} and {suite}'
                modules[mod] = suite
    files = sorted(f[:-3] for f in os.listdir(TESTS) if f.endswith('.rs'))
    orphans = [f for f in files if f not in t and f not in modules]
    assert not orphans, (f'engine/tests/{orphans[0]}.rs (and {len(orphans) - 1} more: {orphans[1:]}) runs nowhere: add '
                         f'`#[path = "{orphans[0]}.rs"] mod {orphans[0]};` to its suite (cards_suite, rules_suite, '
                         f'rulings_suite, misc_suite) or a [[test]] to engine/Cargo.toml (ENGINE.md 167.9)')
    both = [m for m in modules if m in t]
    assert not both, f'{both}: both a [[test]] of its own and a suite module (its tests would run twice)'
    missing = [m for m in list(t) + list(modules) if m not in files]
    assert not missing, f'no engine/tests file for {missing}'
