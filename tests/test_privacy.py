"""No private details in tracked files (tools/privacy_check.py, AGENTS.md "Private details"; gate.py runs it first in
every tier), and this repo's copies of zero-core's privacy check and local-settings readers are the pinned core's
(the rules and their tests live in zero-core: its tests/test_privacy.py)."""
import os
import subprocess
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORE = os.path.join(ROOT, 'third_party', 'zero-core')


def test_the_tracked_files_carry_no_private_details():
    """The whole checkout, with this machine's local patterns when it has them (a fresh clone: the generic set)."""
    if not os.path.exists(os.path.join(ROOT, '.git')):
        pytest.skip('not a git checkout')
    r = subprocess.run([sys.executable, os.path.join(ROOT, 'tools', 'privacy_check.py'), '--root', ROOT],
                       capture_output=True, text=True, timeout=600)
    assert r.returncode == 0, r.stdout[-4000:] + r.stderr[-2000:]
    assert ', 0 finding(s)' in r.stdout


@pytest.mark.parametrize('mine,core', [
    ('tools/privacy_check.py', 'tools/privacy_check.py'),
    ('tools/local_env.sh', 'tools/local_env.sh'),
    ('tools/privacy_patterns.example.txt', 'tools/privacy_patterns.example.txt'),
])
def test_the_copies_are_the_pinned_cores(mine, core):
    def read(p):
        with open(p, encoding='utf-8') as f:
            return f.read().replace('\r\n', '\n')
    assert read(os.path.join(ROOT, mine)) == read(os.path.join(CORE, core))


def test_local_config_is_the_pinned_cores_with_its_note():
    """tools/local_config.py (for tools that run before the submodule exists) = zero.local_config + one note."""
    def read(p):
        with open(p, encoding='utf-8') as f:
            return f.read().replace('\r\n', '\n')
    mine, core = read(os.path.join(ROOT, 'tools', 'local_config.py')), read(os.path.join(CORE, 'python', 'zero',
                                                                                        'local_config.py'))
    head = core.split('\n', 1)[0] + '\n'
    assert mine.startswith(head)
    note_end = mine.index('\n\n', len(head) + 1) + 2          # the note is the docstring's second paragraph
    assert head + '\n' + mine[note_end:] == core
