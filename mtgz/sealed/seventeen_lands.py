"""The 17lands Edge of Eternities sealed pools, builds and games (docs/sealed_data.md 1).

**A yardstick only (DESIGN.md 0): never a training target, reward or prior.** It measures: a pool
generator's statistics against pools Arena really opened, and later our builds against the human
builds of the same pools.

Source: 17lands' public game data (CC BY 4.0, https://www.17lands.com/public_datasets),
`game_data_public.EOE.Sealed.csv.gz` (Premier Sealed, best-of-one) and
`game_data_public.EOE.TradSealed.csv.gz` (Traditional Sealed, best-of-three). One CSV row is one
game of one build of one pool (`draft_id`), with per card `deck_<name>` / `sideboard_<name>`
counts and the game's `opening_hand_<name>` / `drawn_<name>` / `tutored_<name>` counts.

`build()` turns the CSVs into `data/sealed/eoe_17lands_sealed.json.gz` (compact: card indices into
one name table); `load()` reads it back as `Pool` / `Build` / `Game` objects:

* `Pool.cards`: the pool's nonbasic cards (name -> count). 17lands counts a basic land only in a
  deck (Arena's basics are free and unlimited), never in a sideboard, so deck + sideboard minus
  basics is the opened pool. When a pool's rows disagree (launch-day rows where 17lands did not
  yet see a Special Guest card), the pool is the per-card maximum over its rows and the build that
  missed a card lists it in `Build.short`.
* `Build`: one distinct (`build_index`, deck, sideboard) of the pool: the deck with its basics, the
  sideboard (stored as the pool minus the deck, checked when built), the colours 17lands
  assigned, the games in order. In TradSealed, sideboarding between the games of a match makes a
  new `build_index`, so a match can span builds (`Pool.match_record()`).
* `Game`: match / game number, won, on the play, mulligans, the opponent's colours and
  mulligans, turns, ranks, and the cards in the opening hand, drawn and tutored.

`build_draft_packs()` / `load_draft_packs()`: the fresh packs (pick 0 of each pack) of the first
2,006 drafts of `draft_data_public.EOE.PremierDraft.csv.gz` (its first 20 MB): 6,018 packs as
Arena opened them, the evidence for Arena's collation (docs/sealed_data.md 4.3).

    python -m mtgz.sealed.seventeen_lands build --sealed S.csv.gz --trad T.csv.gz
    python -m mtgz.sealed.seventeen_lands packs --draft D.csv.gz.part --byte-range 0-20000000
    python -m mtgz.sealed.seventeen_lands report
"""
import argparse
import collections
import csv
import datetime as dt
import functools
import gzip
import hashlib
import io
import json
import os
import sys
import zlib
from dataclasses import dataclass, field

from . import cards as C

DATASET = os.path.join(C.DATA, 'eoe_17lands_sealed.json.gz')
DRAFT_PACKS = os.path.join(C.DATA, 'eoe_17lands_draft_packs.json.gz')
FORMAT = 'mtgz-17lands-sealed'
SOURCE_URL = 'https://17lands-public.s3.amazonaws.com/analysis_data/game_data/game_data_public.EOE.{event}.csv.gz'
DRAFT_URL = 'https://17lands-public.s3.amazonaws.com/analysis_data/draft_data/draft_data_public.EOE.PremierDraft.csv.gz'
EVENTS = ('Sealed', 'TradSealed')
CARD_TABLES = ('deck', 'sideboard', 'opening_hand', 'drawn', 'tutored')
RULE = 'yardstick only (DESIGN.md 0): never a training target, reward or prior'


# ---------------------------------------------------------------------------------------------
# Building the compact dataset from 17lands' CSVs.

def _int(s):
    return int(float(s)) if s not in ('', None) else 0


def _bool(s):
    if s in ('True', 'true', '1', '1.0'):
        return 1
    if s in ('False', 'false', '0', '0.0'):
        return 0
    raise ValueError(f'not a boolean: {s!r}')


def _opt(s):
    return None if s in ('', 'None', None) else s


def _expand(counts):
    """{index: count} -> sorted index list with repeats."""
    return [i for i in sorted(counts) for _ in range(counts[i])]


def _sideboard(pool, deck, short, basics):
    """{index: count} of the pool minus the deck's nonbasic cards minus `short`."""
    sb = dict(pool)
    for i in list(deck) + list(short):
        if i not in basics:
            sb[i] -= 1
            if sb[i] < 0:
                raise ValueError('a deck holds a card its pool does not')
    return {i: k for i, k in sb.items() if k}


def _sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def _read_event(path, event, names, index):
    """One CSV -> (pool records, source facts). `names` / `index` is the shared card table, grown
    as new names appear."""
    with gzip.open(path, 'rt', encoding='utf-8', newline='') as f:
        r = csv.reader(f)
        head = next(r)
        col = {c: i for i, c in enumerate(head)}
        raw = [c[len('deck_'):] for c in head if c.startswith('deck_')]
        for n in raw:
            for t in CARD_TABLES:
                if f'{t}_{n}' not in col:
                    raise ValueError(f'{path}: {t}_{n} missing')
        unmapped = sorted(n for n in raw if C.map_name(n) is None)
        mapped = [C.map_name(n) or n for n in raw]
        for n in mapped:
            if n not in index:
                index[n] = len(names)
                names.append(n)
        idx = [index[n] for n in mapped]
        tcols = {t: [col[f'{t}_{n}'] for n in raw] for t in CARD_TABLES}
        basics = {index[b] for b in C.BASICS if b in index}
        drafts = collections.OrderedDict()
        rows = 0
        times = []
        for row in r:
            rows += 1
            if row[col['expansion']] != 'EOE' or row[col['event_type']] != event:
                raise ValueError(f'{path}: row {rows} is {row[col["expansion"]]} {row[col["event_type"]]}')
            tabs = {}
            for t in CARD_TABLES:
                d = {}
                for i, ci in zip(idx, tcols[t]):
                    k = _int(row[ci])
                    if k:
                        d[i] = d.get(i, 0) + k
                tabs[t] = d
            dr = drafts.setdefault(row[col['draft_id']], {
                'time': row[col['draft_time']],
                'user': [_opt(row[col['user_n_games_bucket']]), _opt(row[col['user_game_win_rate_bucket']])],
                'rows': []})
            times.append(row[col['draft_time']])
            game = [_int(row[col['match_number']]), _int(row[col['game_number']]), _bool(row[col['won']]),
                    _bool(row[col['on_play']]), _int(row[col['num_mulligans']]), _int(row[col['opp_num_mulligans']]),
                    row[col['opp_colors']], _int(row[col['num_turns']]), _opt(row[col['rank']]),
                    _opt(row[col['opp_rank']]), _expand(tabs['opening_hand']), _expand(tabs['drawn']),
                    _expand(tabs['tutored'])]
            dr['rows'].append((_int(row[col['build_index']]), tabs['deck'], tabs['sideboard'],
                               row[col['main_colors']], row[col['splash_colors']], game))
    pools = []
    for draft_id, dr in drafts.items():
        pool = {}
        for _, deck, sb, *_ in dr['rows']:
            for i, k in (collections.Counter(deck) + collections.Counter(sb)).items():
                if i not in basics:
                    pool[i] = max(pool.get(i, 0), k)
        builds = collections.OrderedDict()
        for b, deck, sb, main, splash, game in dr['rows']:
            key = (b, tuple(sorted(deck.items())), tuple(sorted(sb.items())))
            bd = builds.get(key)
            if bd is None:
                seen = collections.Counter(deck) + collections.Counter(sb)
                short = {i: k - seen.get(i, 0) for i, k in pool.items() if k > seen.get(i, 0)}
                bd = builds[key] = {'index': b, 'deck': _expand(deck), 'colors': main, 'splash': splash,
                                    'games': []}
                if short:
                    bd['short'] = _expand(short)
                if any(i in basics for i in sb):
                    raise ValueError(f'{draft_id}: a basic land in a sideboard')
                # The sideboard is not stored: it is the pool minus the deck's nonbasics minus short.
                if _sideboard(pool, bd['deck'], bd.get('short', ()), basics) != dict(sb):
                    raise ValueError(f'{draft_id}: build {b} sideboard is not pool - deck - short')
            bd['games'].append(game)
        for bd in builds.values():
            bd['games'].sort(key=lambda g: (g[0], g[1]))
        pools.append({'id': draft_id, 'time': dr['time'], 'user': dr['user'], 'pool': _expand(pool),
                      'builds': list(builds.values())})
    pools.sort(key=lambda p: (p['time'], p['id']))
    facts = {'url': SOURCE_URL.format(event=event), 'bytes': os.path.getsize(path), 'sha256': _sha256(path),
             'rows': rows, 'pools': len(pools), 'draft_time': [min(times), max(times)] if times else None,
             'unmapped': unmapped}
    return pools, facts


def build(paths, out=DATASET):
    """paths: {event: csv.gz path}. Writes the compact dataset (gzip, fixed mtime: the same input
    gives the same bytes) and returns its summary."""
    names, index = [], {}
    doc = {'format': FORMAT, 'version': 1, 'set': 'EOE', 'rule': RULE,
           'license': '17lands public data, CC BY 4.0 (https://www.17lands.com/public_datasets)',
           'built': dt.date.today().isoformat(), 'sources': {}, 'cards': names, 'events': {}}
    for event in EVENTS:
        if event in paths:
            doc['events'][event], doc['sources'][event] = _read_event(paths[event], event, names, index)
    doc['not_in_ir'] = sorted(n for n in names if n not in C.ir_names())
    raw = json.dumps(doc, separators=(',', ':'), ensure_ascii=False).encode('utf-8')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, 'wb') as f, gzip.GzipFile(fileobj=f, mode='wb', mtime=0, filename='') as g:
        g.write(raw)
    _doc.cache_clear()
    return {'out': out, 'bytes': os.path.getsize(out), 'json_bytes': len(raw),
            'events': {e: s for e, s in doc['sources'].items()}, 'not_in_ir': doc['not_in_ir']}


def _gunzip_prefix(path):
    """The text of a gzip file that may be cut off (a byte-range download), up to its last whole
    line."""
    d = zlib.decompressobj(16 + zlib.MAX_WBITS)
    parts = []
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            try:
                parts.append(d.decompress(chunk))
            except zlib.error:
                break
    text = b''.join(parts).decode('utf-8', 'ignore')
    return text[:text.rfind('\n') + 1]


def build_draft_packs(path, out=DRAFT_PACKS, byte_range=None):
    """The fresh packs (pick 0 of each pack: the 13 cards as Arena opened them) of a 17lands
    PremierDraft draft_data file, whole or its first bytes (`byte_range` records which). Arena
    collation evidence for docs/sealed_data.md 4.3: draft and sealed open the same boosters."""
    r = csv.reader(io.StringIO(_gunzip_prefix(path)))
    head = next(r)
    col = {c: i for i, c in enumerate(head)}
    raw = [c[len('pack_card_'):] for c in head if c.startswith('pack_card_')]
    unmapped = sorted(n for n in raw if C.map_name(n) is None)
    names = [C.map_name(n) or n for n in raw]
    cols = [col['pack_card_' + n] for n in raw]
    packs, rows, drafts = [], 0, set()
    for row in r:
        if len(row) != len(head):
            break                       # a cut-off last row
        rows += 1
        drafts.add(row[col['draft_id']])
        if row[col['pick_number']] == '0':
            cards = [i for i, ci in enumerate(cols) for _ in range(_int(row[ci]))]
            packs.append([row[col['draft_id']], _int(row[col['pack_number']]), row[col['draft_time']], cards])
    doc = {'format': FORMAT + '-packs', 'version': 1, 'set': 'EOE', 'rule': RULE,
           'license': '17lands public data, CC BY 4.0 (https://www.17lands.com/public_datasets)',
           'built': dt.date.today().isoformat(),
           'source': {'url': DRAFT_URL, 'byte_range': byte_range, 'bytes': os.path.getsize(path),
                      'sha256': _sha256(path), 'rows': rows, 'drafts': len(drafts), 'packs': len(packs),
                      'unmapped': unmapped},
           'cards': names, 'packs': packs}
    raw_json = json.dumps(doc, separators=(',', ':'), ensure_ascii=False).encode('utf-8')
    with open(out, 'wb') as f, gzip.GzipFile(fileobj=f, mode='wb', mtime=0, filename='') as g:
        g.write(raw_json)
    return {'out': out, 'out_bytes': os.path.getsize(out), **doc['source']}


def load_draft_packs(path=DRAFT_PACKS):
    """[{'draft', 'pack', 'time', 'cards': {name: count}}] of the fresh PremierDraft packs."""
    with gzip.open(path, 'rt', encoding='utf-8') as f:
        doc = json.load(f)
    if doc.get('format') != FORMAT + '-packs':
        raise ValueError(f'{path}: not a {FORMAT}-packs file')
    names = doc['cards']
    return [{'draft': d, 'pack': p, 'time': t, 'cards': _counts(names, idx)} for d, p, t, idx in doc['packs']]


# ---------------------------------------------------------------------------------------------
# Loading.

@dataclass
class Game:
    match: int
    game: int
    won: bool
    on_play: bool
    mulligans: int
    opp_mulligans: int
    opp_colors: str
    turns: int
    rank: str | None
    opp_rank: str | None
    opening_hand: tuple
    drawn: tuple
    tutored: tuple


@dataclass
class Build:
    index: int
    deck: dict            # name -> count, basics included
    sideboard: dict       # name -> count (never basics)
    main_colors: str
    splash_colors: str
    games: tuple
    short: dict = field(default_factory=dict)   # pool cards these rows did not record

    @property
    def wins(self):
        return sum(g.won for g in self.games)

    @property
    def losses(self):
        return sum(not g.won for g in self.games)

    @property
    def deck_size(self):
        return sum(self.deck.values())

    @property
    def basics(self):
        return sum(k for n, k in self.deck.items() if n in C.BASICS)


@dataclass
class Pool:
    id: str
    event: str
    time: str
    cards: dict           # name -> count, nonbasic
    builds: tuple
    user_games_bucket: str | None
    user_win_rate_bucket: str | None

    @property
    def size(self):
        return sum(self.cards.values())

    @property
    def wins(self):
        return sum(b.wins for b in self.builds)

    @property
    def losses(self):
        return sum(b.losses for b in self.builds)

    def match_record(self):
        """Best-of-three (TradSealed): (matches won, lost, incomplete). A match's games can span
        builds (sideboarding between games makes a new `build_index`), so this groups the games
        of every build by match number; a match whose recorded games reach two wins or two
        losses counts, any other is incomplete."""
        by = collections.defaultdict(lambda: [0, 0])
        for b in self.builds:
            for g in b.games:
                by[g.match][0 if g.won else 1] += 1
        won = sum(1 for w, l in by.values() if w >= 2)
        lost = sum(1 for w, l in by.values() if l >= 2)
        return won, lost, len(by) - won - lost

    @property
    def incomplete(self):
        """Some row of the pool missed a card (17lands launch-day Special Guests), or the pool is
        short of the 78 nonbasic cards of six 13-card Arena boosters."""
        return self.size != 78 or any(b.short for b in self.builds)

    def not_in_ir(self):
        """The pool's cards outside the engine's card table (Paradox Haze)."""
        return sorted(n for n in self.cards if n not in C.ir_names())

    def unsupported(self, supported=None):
        """The pool's cards the engine does not support (`cards.supported()` by default)."""
        s = C.supported() if supported is None else supported
        return sorted(n for n in self.cards if n not in s)


@functools.lru_cache(2)
def _doc(path):
    with gzip.open(path, 'rt', encoding='utf-8') as f:
        doc = json.load(f)
    if doc.get('format') != FORMAT:
        raise ValueError(f'{path}: not a {FORMAT} file')
    return doc


def _counts(names, idx):
    c = {}
    for i in idx:
        c[names[i]] = c.get(names[i], 0) + 1
    return c


def load(event='Sealed', path=DATASET):
    """The event's pools ('Sealed' or 'TradSealed'), oldest first."""
    doc = _doc(path)
    names = doc['cards']
    out = []
    basics = {names.index(b) for b in C.BASICS if b in names}
    for p in doc['events'][event]:
        pool = collections.Counter(p['pool'])
        builds = []
        for b in p['builds']:
            games = tuple(Game(g[0], g[1], bool(g[2]), bool(g[3]), g[4], g[5], g[6], g[7], g[8], g[9],
                               tuple(names[i] for i in g[10]), tuple(names[i] for i in g[11]),
                               tuple(names[i] for i in g[12])) for g in b['games'])
            sb = _sideboard(pool, b['deck'], b.get('short', ()), basics)
            builds.append(Build(b['index'], _counts(names, b['deck']), _counts(names, _expand(sb)),
                                b['colors'], b['splash'], games, _counts(names, b.get('short', []))))
        out.append(Pool(p['id'], event, p['time'], _counts(names, p['pool']), tuple(builds), p['user'][0],
                        p['user'][1]))
    return out


def sources(path=DATASET):
    d = _doc(path)
    return {'sources': d['sources'], 'not_in_ir': d['not_in_ir'], 'built': d['built'], 'cards': len(d['cards'])}


def report(pools, supported=None):
    """Counts for docs/sealed_data.md: sizes, records, mapping and engine support."""
    s = C.supported() if supported is None else supported
    builds = [b for p in pools for b in p.builds]
    games = [g for b in builds for g in b.games]
    deck_unsup = [b for b in builds if any(n not in s for n in b.deck)]
    return {
        'pools': len(pools),
        'builds': len(builds),
        'games': len(games),
        'pool_size': dict(sorted(collections.Counter(p.size for p in pools).items())),
        'incomplete_pools': sum(p.incomplete for p in pools),
        'deck_size': dict(sorted(collections.Counter(b.deck_size for b in builds).items())),
        'game_win_rate': round(sum(g.won for g in games) / max(1, len(games)), 4),
        'pools_with_card_not_in_ir': sum(bool(p.not_in_ir()) for p in pools),
        'supported_cards': len(s),
        'pools_fully_supported': sum(not p.unsupported(s) for p in pools),
        'builds_with_unsupported_deck_card': len(deck_unsup),
        'unsupported_pool_cards': dict(collections.Counter(n for p in pools for n in p.unsupported(s)).most_common()),
    }


def main(argv=None):
    ap = argparse.ArgumentParser(description='17lands EoE sealed yardstick (docs/sealed_data.md 1)')
    sub = ap.add_subparsers(dest='cmd', required=True)
    b = sub.add_parser('build', help='CSV.gz files -> the compact dataset')
    b.add_argument('--sealed', help='game_data_public.EOE.Sealed.csv.gz')
    b.add_argument('--trad', help='game_data_public.EOE.TradSealed.csv.gz')
    b.add_argument('--out', default=DATASET)
    k = sub.add_parser('packs', help='a (cut-off) PremierDraft draft_data gzip -> its fresh packs')
    k.add_argument('--draft', required=True, help='draft_data_public.EOE.PremierDraft.csv.gz, or its first bytes')
    k.add_argument('--byte-range', help='the range downloaded, e.g. 0-20000000 (recorded)')
    k.add_argument('--out', default=DRAFT_PACKS)
    r = sub.add_parser('report', help='summarise the committed dataset')
    r.add_argument('--path', default=DATASET)
    a = ap.parse_args(argv)
    if a.cmd == 'packs':
        print(json.dumps(build_draft_packs(a.draft, a.out, a.byte_range), indent=1))
    elif a.cmd == 'build':
        paths = {e: p for e, p in (('Sealed', a.sealed), ('TradSealed', a.trad)) if p}
        if not paths:
            ap.error('give --sealed and/or --trad')
        print(json.dumps(build(paths, a.out), indent=1))
    else:
        print(json.dumps(sources(a.path), indent=1))
        for e in _doc(a.path)['events']:
            rep = report(load(e, a.path))
            print(e, json.dumps(rep, indent=1))


if __name__ == '__main__':
    sys.exit(main())
