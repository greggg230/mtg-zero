"""tools/delver_review_apply.py: the review page's verdicts onto the canary library (docs/blunders.md 6a, "review")."""
import json
import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'tools'))
import delver_review_apply as R  # noqa: E402


def lib():
    sp = lambda sid, kind, review=None: {'id': sid, 'kind': kind, 'review': review}  # noqa: E731
    return {'format': 'delver-canary-library/1',
            'spots': [sp('a:1', 'old', {'verdict': 'keep', 'by': 'user', 'date': '2026-11-02'}), sp('b:1', 'new'),
                      sp('b:2', 'new'), sp('b:3', 'new'), sp('b:4', 'new'), sp('c:1', 'other')],
            'reference': {'scores': {'32': {'spots': {'a:1': 1.0, 'c:1': 0.5}}}}, 'dropped': {'x:0': 'gone'},
            'audit_user': {'date': '2026-11-02'}}


def test_keep_drop_unsure_and_pending():
    L = lib()
    ver = {'a:1': {'verdict': 'drop'}, 'b:1': {'verdict': 'keep', 'note': ' fine '}, 'b:2': {'verdict': 'drop', 'note': 'n'},
           'b:3': {'verdict': 'unsure'}, 'b:4': {'verdict': None}}
    s, pending = R.apply(L, ver, '2026-11-18', kinds={'new'}, label='bauble')
    by = {sp['id']: sp for sp in L['spots']}
    assert by['a:1']['review']['verdict'] == 'keep'                          # an earlier verdict is never rewritten
    assert by['b:1']['review'] == {'verdict': 'keep', 'by': 'user', 'date': '2026-11-18', 'note': 'fine'}
    assert 'b:2' not in by and L['dropped']['b:2'] == "the user's audit: drop (n)"
    assert by['b:3']['review']['verdict'] == 'unsure' and by['b:4']['review'] is None and pending == ['b:4']
    assert by['c:1']['review'] is None                                         # outside --kinds
    assert s == {'verdicts': 3, 'kept': 1, 'dropped': 1, 'held': 1, 'still_to_review': 1, 'kinds': ['new']}
    assert L['audit_user']['bauble'] == s and L['audit_user']['date'] == '2026-11-02'


def test_loud_failures():
    with pytest.raises(SystemExit, match='name no library spot'):
        R.apply(lib(), {'zz:9': {'verdict': 'keep'}}, '2026-11-18')
    with pytest.raises(SystemExit, match='reference'):
        R.apply(lib(), {'c:1': {'verdict': 'drop'}}, '2026-11-18')
    with pytest.raises(SystemExit, match='unknown verdict'):
        R.apply(lib(), {'b:1': {'verdict': 'maybe'}}, '2026-11-18')
    with pytest.raises(SystemExit, match='no unreviewed spot'):
        R.apply(lib(), {}, '2026-11-18', kinds={'nope'})


def test_reads_saved_documents(tmp_path):
    d = tmp_path / 'verdicts'
    d.mkdir()
    (d / 'b%3A1.json').write_text(json.dumps({'data': {'verdict': 'keep'}}), encoding='utf-8')
    (d / 'b_2.json').write_text(json.dumps({'id': 'b:2', 'data': {'verdict': 'drop'}}), encoding='utf-8')
    assert R.read_verdicts(str(tmp_path)) == {'b:1': {'verdict': 'keep'}, 'b:2': {'verdict': 'drop'}}
    f = tmp_path / 'list.json'
    f.write_text(json.dumps([{'id': 'b:3', 'data': {'verdict': 'unsure'}}]), encoding='utf-8')
    assert R.read_verdicts(str(f)) == {'b:3': {'verdict': 'unsure'}}
