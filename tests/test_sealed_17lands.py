"""The EoE sealed card list and the 17lands sealed yardstick (mtgz/sealed, docs/sealed_data.md 1-2).
Pure Python: no engine module needed."""
import collections
import csv
import gzip

import pytest

from mtgz.sealed import cards as C
from mtgz.sealed import seventeen_lands as L


# --- the card list ------------------------------------------------------------------------------

def test_the_set_has_every_sheet_at_its_published_size():
    by = collections.Counter((c['sheet'], c['rarity']) for c in C.eoe_set()['cards'])
    assert by == {('eoe', 'common'): 81, ('eoe', 'uncommon'): 100, ('eoe', 'rare'): 60, ('eoe', 'mythic'): 20,
                  ('eos', 'rare'): 30, ('eos', 'mythic'): 15, ('spg', 'mythic'): 10, ('basic', 'common'): 5}
    t = C.eoe_set()['treatments']
    assert {k: len(v) for k, v in t.items()} == {'viewport': 10, 'triumphant': 16, 'surreal': 14}
    main = {n for n, c in C.cards().items() if c['sheet'] == 'eoe'}
    assert all(r['name'] in main and r['rarity'] == C.cards()[r['name']]['rarity'] for v in t.values() for r in v)


def test_only_paradox_haze_is_outside_the_engine_card_table():
    names = set(C.cards())
    assert C.ir_names() <= names
    assert names - C.ir_names() == {'Paradox Haze'}
    assert all(c['in_ir'] == (c['name'] in C.ir_names()) for c in C.cards().values())


def test_the_support_list_is_read_from_support_rs(tmp_path):
    s = C.supported()
    assert len(s) >= 93 and s <= C.ir_names() and set(C.BASICS) <= s
    bad = tmp_path / 'support.rs'
    bad.write_text('const SUPPORTED: &[&str] = &["A"];\n', encoding='utf-8')
    with pytest.raises(ValueError):
        C.supported(str(bad))
    ok = tmp_path / 'ok.rs'
    ok.write_text('const SUPPORTED_EOE_X: &[&str] = &[\n    "Annul",\n    "Plains",\n];\n'
                  'const SUPPORTED_EOE_Y: &[&str] = &[\n];\n'
                  'fn supported_eoe() -> Vec<String> {\n    let lists = [SUPPORTED_EOE_X, SUPPORTED_EOE_Y];\n}\n',
                  encoding='utf-8')
    assert C.supported(str(ok)) == {'Annul', 'Plains'}


def test_names_map_exactly_by_front_face_or_loosely():
    assert C.map_name('Annul') == 'Annul'
    assert C.map_name('Annul // Something Else') == 'Annul'
    assert C.map_name('Mm’menon, the Right Hand') == "Mm'menon, the Right Hand"
    assert C.map_name('Not A Card') is None


# --- the committed dataset ----------------------------------------------------------------------

@pytest.fixture(scope='module')
def sealed():
    return L.load('Sealed')


@pytest.fixture(scope='module')
def trad():
    return L.load('TradSealed')


def test_the_dataset_holds_every_pool_build_and_game(sealed, trad):
    src = L.sources()
    assert src['sources']['Sealed']['rows'] == 56256 and src['sources']['TradSealed']['rows'] == 7773
    assert src['sources']['Sealed']['unmapped'] == [] and src['sources']['TradSealed']['unmapped'] == []
    assert src['not_in_ir'] == ['Paradox Haze']
    assert len(sealed) == 9765 and len(trad) == 903
    assert sum(len(b.games) for p in sealed for b in p.builds) == 56256
    assert sum(len(b.games) for p in trad for b in p.builds) == 7773
    assert sum(len(p.builds) for p in sealed) == 13787        # 13,780 build indices; 7 split in two


def test_a_pool_is_78_nonbasic_cards_and_basics_are_only_in_decks(sealed, trad):
    sizes = collections.Counter(p.size for p in sealed + trad)
    assert sizes == {78: 9753 + 903, 77: 12}                # 77: a launch-day Special Guest unseen
    assert sum(p.incomplete for p in sealed) == 19
    for p in sealed + trad:
        assert not set(p.cards) & set(C.BASICS)
        for b in p.builds:
            assert not set(b.sideboard) & set(C.BASICS)
            nonbasic = collections.Counter({n: k for n, k in b.deck.items() if n not in C.BASICS})
            assert dict(nonbasic + collections.Counter(b.sideboard) + collections.Counter(b.short)) == p.cards


def test_records_fit_the_arena_events(sealed, trad):
    # Premier Sealed: 7 wins or 3 losses (5 pools exceed it in 17lands' rows); Traditional: 4 / 2.
    over = [p for p in sealed if p.wins > 7 or p.losses > 3]
    assert len(over) <= 5
    assert all(w <= 4 and l <= 2 for w, l, _ in (p.match_record() for p in trad))
    games = [g for p in sealed for b in p.builds for g in b.games]
    assert 0.5 < sum(g.won for g in games) / len(games) < 0.65      # 17lands users win more than half
    assert all(len(g.opening_hand) == 7 for g in games[:2000] if g.mulligans == 0)


def test_pools_with_cards_the_engine_lacks_are_flagged_not_dropped(sealed):
    rep = L.report(sealed, supported=C.ir_names())
    assert rep['pools'] == 9765
    assert rep['pools_with_card_not_in_ir'] == 87
    assert rep['unsupported_pool_cards'] == {'Paradox Haze': 87}
    assert L.report(sealed, supported=frozenset())['pools_fully_supported'] == 0


# --- building from 17lands' CSV -----------------------------------------------------------------

META = ['expansion', 'event_type', 'draft_id', 'draft_time', 'game_time', 'build_index', 'match_number',
        'game_number', 'rank', 'opp_rank', 'main_colors', 'splash_colors', 'on_play', 'num_mulligans',
        'opp_num_mulligans', 'opp_colors', 'num_turns', 'won', 'user_n_games_bucket', 'user_game_win_rate_bucket']
NAMES = ['Annul', 'Bombard', 'Paradox Haze', 'Plains']


def _write_csv(path, rows):
    head = META + [f'{t}_{n}' for t in L.CARD_TABLES for n in NAMES]
    with gzip.open(path, 'wt', encoding='utf-8', newline='') as f:
        w = csv.writer(f)
        w.writerow(head)
        for meta, tables in rows:
            w.writerow([meta.get(k, '') for k in META] +
                       [str(float(tables.get(t, {}).get(n, 0))) for t in L.CARD_TABLES for n in NAMES])


def test_build_turns_rows_into_pools_builds_and_games(tmp_path):
    base = {'expansion': 'EOE', 'event_type': 'Sealed', 'draft_id': 'd1', 'draft_time': '2025-07-29 10:00:00',
            'build_index': '0', 'game_number': '1', 'rank': '', 'opp_rank': 'None', 'main_colors': 'UR',
            'splash_colors': '', 'on_play': 'True', 'num_mulligans': '0', 'opp_num_mulligans': '1',
            'opp_colors': 'WG', 'num_turns': '9', 'user_n_games_bucket': '10', 'user_game_win_rate_bucket': '0.5'}
    rows = [
        # game 1: the tracker misses Paradox Haze (as on launch day)
        (dict(base, match_number='1', won='True'),
         {'deck': {'Annul': 1, 'Plains': 16}, 'sideboard': {'Bombard': 2},
          'opening_hand': {'Annul': 1, 'Plains': 6}, 'drawn': {'Plains': 2}}),
        # game 2: the same build index, Paradox Haze now seen
        (dict(base, match_number='2', won='False', on_play='False'),
         {'deck': {'Annul': 1, 'Plains': 16}, 'sideboard': {'Bombard': 2, 'Paradox Haze': 1}}),
        # game 3: a rebuild
        (dict(base, match_number='3', won='True', build_index='1'),
         {'deck': {'Bombard': 2, 'Plains': 15}, 'sideboard': {'Annul': 1, 'Paradox Haze': 1}}),
    ]
    src = tmp_path / 'game_data_public.EOE.Sealed.csv.gz'
    _write_csv(src, rows)
    out = tmp_path / 'ds.json.gz'
    summary = L.build({'Sealed': str(src)}, str(out))
    assert summary['events']['Sealed']['rows'] == 3 and summary['events']['Sealed']['unmapped'] == []
    assert summary['not_in_ir'] == ['Paradox Haze']
    with open(out, 'rb') as f:
        first = f.read()
    L.build({'Sealed': str(src)}, str(out))
    with open(out, 'rb') as f:
        assert f.read() == first                                   # the same input, the same bytes
    (p,) = L.load('Sealed', str(out))
    assert p.cards == {'Annul': 1, 'Bombard': 2, 'Paradox Haze': 1} and p.size == 4
    assert p.not_in_ir() == ['Paradox Haze'] and p.unsupported(frozenset({'Annul', 'Bombard'})) == ['Paradox Haze']
    assert [b.index for b in p.builds] == [0, 0, 1]
    b0, b0b, b1 = p.builds
    assert b0.short == {'Paradox Haze': 1} and b0.sideboard == {'Bombard': 2} and b0.deck == {'Annul': 1, 'Plains': 16}
    assert b0b.short == {} and b0b.sideboard == {'Bombard': 2, 'Paradox Haze': 1}
    assert b1.deck == {'Bombard': 2, 'Plains': 15} and b1.basics == 15 and b1.deck_size == 17
    g = b0.games[0]
    assert (g.match, g.won, g.on_play, g.mulligans, g.opp_mulligans, g.opp_colors, g.turns, g.rank, g.opp_rank) == \
        (1, True, True, 0, 1, 'WG', 9, None, None)
    assert g.opening_hand == ('Annul',) + ('Plains',) * 6 and g.drawn == ('Plains', 'Plains')
    assert (p.wins, p.losses) == (2, 1) and p.incomplete


def test_build_refuses_a_basic_in_a_sideboard(tmp_path):
    src = tmp_path / 'bad.csv.gz'
    meta = {'expansion': 'EOE', 'event_type': 'Sealed', 'draft_id': 'd', 'draft_time': 't', 'build_index': '0',
            'match_number': '1', 'game_number': '1', 'on_play': 'True', 'won': 'True', 'num_turns': '5'}
    _write_csv(src, [(meta, {'deck': {'Annul': 1}, 'sideboard': {'Plains': 1}})])
    with pytest.raises(ValueError):
        L.build({'Sealed': str(src)}, str(tmp_path / 'o.json.gz'))


def test_the_fresh_draft_packs_are_13_card_arena_boosters():
    packs = L.load_draft_packs()
    assert len(packs) == 6018
    sizes = collections.Counter(sum(p['cards'].values()) for p in packs)
    assert sizes == {13: 5998, 12: 20}
    assert all(not set(p['cards']) & set(C.BASICS) for p in packs)
    assert {p['pack'] for p in packs} == {0, 1, 2}
