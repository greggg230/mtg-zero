"""Observable answers through the bindings (ENGINE.md 149, zero-core docs/search.md 3.18): `mtg` declares them, a
forced pass of priority (`KEY_AUTO`, Priority, PASS) is the other player's observable answer and the mover's own key
as it is, and zero.play's reference tree (`PySearch.follow`) is never reused across one."""
import numpy as np

from mtgz.az import games as G

SPEC = '{"max_turns": 60}'
KEY_AUTO = 4


def kind(k):
    return k >> 56


def player(k):
    return (k >> 48) & 0xFF


def payload(k):
    return k & ((1 << 48) - 1)


def test_a_forced_pass_is_the_other_players_observable_answer():
    E = G.engine()
    g = E.Game('mtg', SPEC, 2, 3)
    assert g.answers and not E.Game('toy', '{}', 2, 1).answers
    rng = np.random.default_rng(3)
    passes = others = 0
    while not g.over:
        legal = list(g.legal())
        if g.decision_tag == 1:                       # 1 + DecisionKind::Priority: PASS is always an option
            assert 0 in legal
        for k in g.step(int(legal[rng.integers(len(legal))])):
            if kind(k) != KEY_AUTO:
                assert g.key_obs(k, 0) == g.key_obs(k, 1) == 'plain'
                continue
            q = player(k)
            assert g.key_obs(k, q) == 'plain'         # its own answer: what it knows
            if payload(k) == 0:                       # Priority << 16 | PASS
                assert g.key_obs(k, 1 - q) == 'answer'
                passes += 1
            else:
                assert g.key_obs(k, 1 - q) == 'opaque'
                others += 1
    assert passes > 0, (passes, others)


def test_the_reference_tree_is_never_reused_across_an_answer():
    import random

    from zero.play import PySearch

    E = G.engine()
    g = E.Game('mtg', SPEC, 2, 5)
    rng = np.random.default_rng(5)
    for _ in range(4000):
        if g.over:
            break
        legal = list(g.legal())
        p = g.to_move
        before = g.clone()
        a = int(legal[rng.integers(len(legal))])
        tag = before.decision_tag
        key = g.step(a)
        if g.over or g.to_move != p or not any(kind(k) == KEY_AUTO and player(k) != p and payload(k) == 0 for k in key):
            continue
        t = PySearch(before, type('S', (), {'redet': True})(), random.Random(1))
        # (the reference tree would look its child up; with an answer in the key it never does)
        t.advance = lambda *args: (_ for _ in ()).throw(AssertionError('advance called across an answer'))
        assert not t.follow(p, p, a, tag, key, 0, g)
        return
    raise AssertionError('no forced pass of the other player followed by the mover again')
