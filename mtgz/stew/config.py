"""stew.json (docs/ENGINE.md 222.2): the stew's config - its field, candidates, source run, seeds, recipe, code and
the runs it yields to - with its defaults and checks, and the field's arithmetic: the pairings (one specialist per
unordered pairing with at least one candidate, mirrors included), the shares, the cells and their weights.

    {"name": "first",                                   # a slug: farm runs <name>-<alias>-<alias>
     "decks": [{"name": "izzet_delver_8016541", "share": 1, "alias": "delver"}, ...],   # data/decks/ names
     "candidates": ["izzet_delver_8016541"],
     "source": {"run": "<run dir>", "iter": "newest"},  # gen 0 of the main stew + the specialists' recipe
     "seeds": {"<A>|<B>": "<.pt>"},                      # a pairing's own seed (default: the main stew's gen 0)
     "recipe": {"turn_iters": 32, "format": "bo3", "game_episodes": true, "start_game": 1,
                "settings": {...}, "pair_settings": {"<A>|<B>": {...}}, "farm": true,
                "spec_extra": {...}, "deck_spec_extra": {"<deck>": {...}},   # self-play spec keys (dealt decks...)
                "distill": {...}, "distill_cli": ["--device", "cuda"], "distill_iters": 6,
                "retention_share": "field", "judge": {...}, "hold_after": ["judge_deck_gap@0"], "env": {...},
                "archive": "copy"},
     "code": {"worktree": null, "sha": null, "python": null, "farm": {...}, "farm_gates": true},
     "yield_runs": [{"run": "<run dir>", "farm_runs": ["<farm run>", ...]}],
     "beside_runs": ["<run dir>", ...],                  # live runs it may run beside, never touched (a CPU stew)
     "requires": ["..."],                                # what the recipe needs merged first (init and plan say it)
     "notes": "..."}

Kept light: json, os, re only (the decklists module for the deck checks).
"""
import copy
import json
import re

PHASES = ('train', 'distil_decks', 'judge_deck_gap', 'distil_main', 'judge')
SLUG_RE = re.compile(r'^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$')
ALIAS_RE = re.compile(r'^[a-z0-9]{1,16}$')
# what the stew sets in every specialist's settings itself (recipe.settings may not)
RESERVED = ('run', 'spec', 'eval_spec', 'iters', 'selfplay_remote', 'resolver')
# the specialist spec's own keys (recipe.spec_extra may not set them): the format and the restricted metagame
SPEC_OWN = ('constructed', 'format', 'game_episodes', 'start_game', 'max_turns', 'mulligans', 'metagame', 'decks')
DECK_OWN = ('name', 'main', 'sideboard')

DEFAULT_DISTILL = {'steps': 20000, 'batch': 512, 'lr': 5e-4, 'lr_min': 1e-5, 'warmup': 500, 'wd': 1e-4,
                   'teacher_w': 1.0, 'value_q': 0.5, 'margin_w': 0.5, 'eval_every': 1000, 'save_every': 2000,
                   'ram_budget_gb': 20, 'seed': 1}
DEFAULT_JUDGE = {'backend': 'local', 'matches': 400, 'gen_matches': 400, 'sims': 64, 'device': 'cuda', 'threads': 16,
                 'groups': 1, 'procs': 1, 'seed': 0, 'chance_k': 2, 'precision': None, 'gen_over_gen': True,
                 'main_arms': True, 'est_s_per_match': 3.5}
JUDGE_BACKENDS = ('local',)            # ('farm': campaigns later - one backend per comparison)
DEFAULT_RECIPE = {'turn_iters': 32, 'format': 'bo3', 'game_episodes': True, 'start_game': 1, 'settings': {},
                  'pair_settings': {}, 'spec_extra': {}, 'deck_spec_extra': {}, 'farm': True, 'distill': DEFAULT_DISTILL, 'distill_cli': ['--device', 'cuda'],
                  'distill_iters': 6, 'retention_share': 'field', 'judge': DEFAULT_JUDGE, 'hold_after': [], 'env': {},
                  'archive': 'copy'}
DEFAULT_CODE = {'worktree': None, 'sha': None, 'python': None, 'farm': None, 'farm_gates': True}
FARM_KEYS = ('pool_build', 'pool_runtime', 'pool_worker', 'pool_alternates', 'pool_variants', 'pool_min_agent')


class ConfigError(ValueError):
    """stew.json is not a stew config (the message names what is wrong)."""


# ------------------------------------------------------------------ the field's arithmetic
def pair_key(a, b):
    return f'{a}|{b}'


def split_key(k):
    a, sep, b = str(k).partition('|')
    if not sep or not a or not b or '|' in b:
        raise ConfigError(f'pairing {k!r}: "<deck>|<deck>"')
    return a, b


def deck_names(decks):
    return [d['name'] for d in decks]


def canon(decks, a, b):
    """The unordered pairing {a, b} in field order (the deck listed first, first) -> (a, b)."""
    names = deck_names(decks)
    if a not in names or b not in names:
        raise ConfigError(f'pairing {a}|{b}: not both in the field {names}')
    return (a, b) if names.index(a) <= names.index(b) else (b, a)


def pairings(decks, candidates):
    """Every unordered pairing with at least one candidate, mirrors included, in field order -> [(a, b)].
    K candidates in a field of N: K*N - K(K-1)/2 of them."""
    names = deck_names(decks)
    out = []
    for i, x in enumerate(names):
        for y in names[i:]:
            if x in candidates or y in candidates:
                out.append((x, y))
    return out


def shares(decks):
    """{deck: its normalized field share}."""
    tot = float(sum(d['share'] for d in decks))
    return {d['name']: d['share'] / tot for d in decks}


def cells(decks):
    """Every ordered cell (my deck, the opponent's) of the field -> [(x, y)]."""
    names = deck_names(decks)
    return [(x, y) for x in names for y in names]


def cell_weight(sh, x, y):
    """The cell's share of a field game: the chance of piloting x against y."""
    return sh[x] * sh[y]


def covered(pairs, x, y):
    return (x, y) in pairs or (y, x) in pairs


def uncovered_cells(decks, pairs):
    return [(x, y) for x, y in cells(decks) if not covered(pairs, x, y)]


def spec_name(a, b):
    """A specialist's net name in state.json."""
    return f'spec:{a}|{b}'


def spec_dir(a, b):
    return f'spec-{a}-{b}'


def deck_net(x):
    return f'deck:{x}'


def deck_dir(x):
    return f'deck-{x}'


def default_alias(name):
    """A deck's short name for farm run slugs: its name's first word (letters and digits)."""
    w = re.sub(r'[^a-z0-9]', '', name.lower().split('_')[0])
    return w[:16] or 'deck'


def farm_run(name, decks, a, b):
    """The farm run of specialist {a, b}: '<stew>-<alias a>-<alias b>' (a slug of at most 63)."""
    al = {d['name']: d['alias'] for d in decks}
    fr = f'{name}-{al[a]}-{al[b]}'
    if not SLUG_RE.match(fr):
        raise ConfigError(f'farm run {fr!r}: a slug of at most 63 [a-z0-9-] (shorten the stew name or the aliases)')
    return fr


def tag(phase, rnd):
    return f'{phase}@{int(rnd)}'


def parse_tag(t):
    p, sep, r = str(t).partition('@')
    if not sep or p not in PHASES or not r.isdigit():
        raise ConfigError(f'hold point {t!r}: "<phase>@<generation>", phase one of {PHASES}')
    return p, int(r)


# ------------------------------------------------------------------ loading
def _merge(default, given, what):
    if given is None:
        return copy.deepcopy(default)
    if not isinstance(given, dict):
        raise ConfigError(f'{what}: a JSON object')
    out = copy.deepcopy(default)
    out.update(copy.deepcopy(given))
    return out


def _check_decks(decks, fmt, check_lists=True):
    if not isinstance(decks, list) or not decks:
        raise ConfigError('decks: a non-empty list of {"name", "share", "alias"}')
    out, seen, aliases = [], set(), set()
    for d in decks:
        if isinstance(d, str):
            d = {'name': d}
        if not isinstance(d, dict) or not isinstance(d.get('name'), str) or set(d) - {'name', 'share', 'alias'}:
            raise ConfigError(f'decks: {d!r} is not {{"name", "share", "alias"}}')
        share = d.get('share', 1)
        if isinstance(share, bool) or not isinstance(share, (int, float)) or not share > 0:
            raise ConfigError(f"decks: {d['name']}: share {share!r} must be a number > 0")
        alias = d.get('alias') or default_alias(d['name'])
        if not ALIAS_RE.match(alias):
            raise ConfigError(f"decks: {d['name']}: alias {alias!r}: 1-16 of [a-z0-9]")
        if d['name'] in seen:
            raise ConfigError(f"decks: {d['name']} listed twice")
        if alias in aliases:
            raise ConfigError(f"decks: alias {alias!r} twice (give the decks their own \"alias\")")
        seen.add(d['name'])
        aliases.add(alias)
        if check_lists:
            check_deck(d['name'], fmt)
        out.append({'name': d['name'], 'share': share, 'alias': alias})
    return out


def check_deck(name, fmt):
    """A data/decks/ decklist the format can play (best-of-three: a 60 + its sideboard) -> ConfigError if not."""
    from mtgz.az import decklists as D
    if not re.match(r'^[A-Za-z0-9_.-]+$', name):
        raise ConfigError(f'deck {name!r}: a data/decks/ name')
    try:
        D._seat_lists(name, fmt)
    except (OSError, ValueError) as e:
        raise ConfigError(f'deck {name}: {e}') from None


def normalize(cfg, check_lists=True):
    """stew.json -> the config with every default filled in (a new dict). ConfigError naming what is wrong."""
    if not isinstance(cfg, dict):
        raise ConfigError('stew.json: a JSON object')
    known = {'name', 'decks', 'candidates', 'source', 'seeds', 'recipe', 'code', 'yield_runs', 'beside_runs',
             'requires', 'notes'}
    bad = sorted(set(cfg) - known)
    if bad:
        raise ConfigError(f'stew.json: unknown keys {bad} ({sorted(known)})')
    for k in ('name', 'decks', 'candidates', 'source'):
        if k not in cfg:
            raise ConfigError(f'stew.json: no {k!r}')
    out = {'name': cfg['name']}
    if not isinstance(out['name'], str) or not SLUG_RE.match(out['name']) or len(out['name']) > 30:
        raise ConfigError(f"name {cfg['name']!r}: a slug of [a-z0-9-], at most 30 (its farm runs are named after it)")
    r = _merge(DEFAULT_RECIPE, cfg.get('recipe'), 'recipe')
    r['distill'] = _merge(DEFAULT_DISTILL, (cfg.get('recipe') or {}).get('distill'), 'recipe.distill')
    r['judge'] = _merge(DEFAULT_JUDGE, (cfg.get('recipe') or {}).get('judge'), 'recipe.judge')
    bad = sorted(set(r) - set(DEFAULT_RECIPE))
    if bad:
        raise ConfigError(f'recipe: unknown keys {bad} ({sorted(DEFAULT_RECIPE)})')
    if r['format'] not in ('bo1', 'bo3'):
        raise ConfigError(f"recipe.format {r['format']!r}: 'bo1' or 'bo3'")
    if r['format'] == 'bo1':
        if r['game_episodes'] or r['start_game'] != 1:
            raise ConfigError('recipe: game_episodes / start_game are best-of-three settings (format "bo3")')
    elif r['start_game'] != 1:
        raise ConfigError('recipe.start_game: 1 (a specialist of two decks plays game 1: start_game 2 is a mirror\'s, '
                          'ENGINE.md 147.4; the judge plays game 1 with its chooser)')
    if not isinstance(r['turn_iters'], int) or isinstance(r['turn_iters'], bool) or r['turn_iters'] < 1:
        raise ConfigError(f"recipe.turn_iters {r['turn_iters']!r}: a positive integer")
    if not isinstance(r['distill_iters'], int) or r['distill_iters'] < 1:
        raise ConfigError(f"recipe.distill_iters {r['distill_iters']!r}: a positive integer (the newest data iterations "
                          f"of a specialist's turn its deck stews distil from)")
    for k in ('settings',):
        if not isinstance(r[k], dict):
            raise ConfigError(f'recipe.{k}: a JSON object of run settings')
        res = [x for x in RESERVED if x in r[k]]
        if res:
            raise ConfigError(f'recipe.{k} sets {res}: the stew sets those itself (the pairing spec, its iterations, '
                              f'its farm run)')
    if not isinstance(r['pair_settings'], dict):
        raise ConfigError('recipe.pair_settings: {"<A>|<B>": {run settings}}')
    if not isinstance(r['spec_extra'], dict) or [k for k in SPEC_OWN if k in r['spec_extra']]:
        raise ConfigError(f'recipe.spec_extra: keys of the specialists\' self-play spec, none of {SPEC_OWN} (the stew '
                          f'builds those: the restricted metagame of the recipe\'s format)')
    if not isinstance(r['deck_spec_extra'], dict) or not all(
            isinstance(v, dict) and not [k for k in DECK_OWN if k in v] for v in r['deck_spec_extra'].values()):
        raise ConfigError(f'recipe.deck_spec_extra: {{"<deck>": {{keys of its metagame entry, none of {DECK_OWN}}}}}')
    if not isinstance(r['distill_cli'], list) or not all(isinstance(x, str) for x in r['distill_cli']):
        raise ConfigError('recipe.distill_cli: a list of strings (extra tools/distill.py arguments)')
    rs = r['retention_share']
    if rs != 'field' and (isinstance(rs, bool) or not isinstance(rs, (int, float)) or not 0 <= rs < 1):
        raise ConfigError(f'recipe.retention_share {rs!r}: "field" (the uncovered cells\' field share) or a number in '
                          f'[0, 1)')
    if not isinstance(r['env'], dict):
        raise ConfigError('recipe.env: {NAME: value} (the stew\'s processes\' environment)')
    r['env'] = {str(k): str(v) for k, v in r['env'].items()}
    if r['archive'] not in ('copy', 'link'):
        raise ConfigError(f"recipe.archive {r['archive']!r}: 'copy' or 'link'")
    j = r['judge']
    bad = sorted(set(j) - set(DEFAULT_JUDGE))
    if bad:
        raise ConfigError(f'recipe.judge: unknown keys {bad} ({sorted(DEFAULT_JUDGE)})')
    if j['backend'] not in JUDGE_BACKENDS:
        raise ConfigError(f"recipe.judge.backend {j['backend']!r}: {JUDGE_BACKENDS} (farm campaigns are not built yet)")
    for k, m in (('matches', 4), ('gen_matches', 2)):
        if not isinstance(j[k], int) or j[k] < m or j[k] % m:
            raise ConfigError(f'recipe.judge.{k} {j[k]!r}: a positive multiple of {m}')
    for k in ('sims', 'threads', 'groups', 'procs'):
        if not isinstance(j[k], int) or j[k] < 1:
            raise ConfigError(f'recipe.judge.{k} {j[k]!r}: a positive integer')
    bad = sorted(set(r['distill']) - set(DEFAULT_DISTILL))
    if bad:
        raise ConfigError(f'recipe.distill: unknown keys {bad} (the sources file\'s "train" block: '
                          f'{sorted(DEFAULT_DISTILL)})')
    hold = r['hold_after']
    if not isinstance(hold, list):
        raise ConfigError('recipe.hold_after: a list of "<phase>@<generation>"')
    for t in hold:
        parse_tag(t)
    out['recipe'] = r
    decks = _check_decks(cfg['decks'], r['format'], check_lists)
    out['decks'] = decks
    names = deck_names(decks)
    cand = cfg['candidates']
    if not isinstance(cand, list) or not cand or not all(c in names for c in cand) or len(set(cand)) != len(cand):
        raise ConfigError(f'candidates {cand!r}: a non-empty list of distinct decks of the field {names}')
    out['candidates'] = [n for n in names if n in cand]
    src = cfg['source']
    if not isinstance(src, dict) or not src.get('run') or not (src.get('iter', 'newest') == 'newest' or (
            isinstance(src.get('iter'), int) and not isinstance(src.get('iter'), bool) and src['iter'] >= 0)):
        raise ConfigError('source: {"run": DIR, "iter": N | "newest"}')
    out['source'] = {'run': str(src['run']), 'iter': src.get('iter', 'newest')}
    pairs = pairings(decks, out['candidates'])
    seeds = cfg.get('seeds') or {}
    if not isinstance(seeds, dict):
        raise ConfigError('seeds: {"<A>|<B>": "<.pt>"}')
    out['seeds'] = {}
    for k, p in seeds.items():
        a, b = split_key(k)
        c = canon(decks, a, b)
        if c not in pairs:
            raise ConfigError(f'seeds: {k} is no specialist\'s pairing ({[pair_key(*x) for x in pairs]})')
        out['seeds'][pair_key(*c)] = str(p)
    ps = {}
    for k, v in r['pair_settings'].items():
        a, b = split_key(k)
        c = canon(decks, a, b)
        if c not in pairs:
            raise ConfigError(f'recipe.pair_settings: {k} is no specialist\'s pairing')
        if not isinstance(v, dict) or [x for x in RESERVED if x in v]:
            raise ConfigError(f'recipe.pair_settings.{k}: run settings, none of {RESERVED}')
        ps[pair_key(*c)] = v
    r['pair_settings'] = ps
    code = _merge(DEFAULT_CODE, cfg.get('code'), 'code')
    bad = sorted(set(code) - set(DEFAULT_CODE))
    if bad:
        raise ConfigError(f'code: unknown keys {bad} ({sorted(DEFAULT_CODE)})')
    if code['farm'] is not None:
        if not isinstance(code['farm'], dict) or set(code['farm']) - set(FARM_KEYS):
            raise ConfigError(f'code.farm: {{{", ".join(FARM_KEYS)}}} (pool_build "published" = the published build '
                              f'of code.sha, tools/farm_release.py)')
    out['code'] = code
    yr = cfg.get('yield_runs') or []
    if not isinstance(yr, list):
        raise ConfigError('yield_runs: [{"run": DIR, "farm_runs": [...]}]')
    out['yield_runs'] = []
    for y in yr:
        if isinstance(y, str):
            y = {'run': y}
        if not isinstance(y, dict) or not y.get('run') or set(y) - {'run', 'farm_runs'}:
            raise ConfigError(f'yield_runs: {y!r} is not {{"run", "farm_runs"}}')
        fr = list(y.get('farm_runs') or [])
        for f in fr:
            if not isinstance(f, str) or not SLUG_RE.match(f):
                raise ConfigError(f'yield_runs: farm run {f!r} is not a slug')
        out['yield_runs'].append({'run': str(y['run']), 'farm_runs': fr})
    br = cfg.get('beside_runs') or []
    if not isinstance(br, list) or not all(isinstance(x, str) and x for x in br):
        raise ConfigError('beside_runs: [DIR, ...] (live runs the stew runs beside without touching them)')
    out['beside_runs'] = list(br)
    req = cfg.get('requires') or []
    if not isinstance(req, list) or not all(isinstance(x, str) for x in req):
        raise ConfigError('requires: ["..."] (what the recipe needs merged first)')
    out['requires'] = list(req)
    out['notes'] = cfg.get('notes', '')
    bad = [d for d in r['deck_spec_extra'] if d not in names]
    if bad:
        raise ConfigError(f'recipe.deck_spec_extra: {bad} not in the field {names}')
    for a, b in pairs:                          # (the farm runs' names: checked now, not at the first turn)
        farm_run(out['name'], decks, a, b)
    return out


def load(path, check_lists=True):
    with open(path, encoding='utf-8') as f:
        try:
            cfg = json.load(f)
        except ValueError as e:
            raise ConfigError(f'{path}: not JSON ({e})') from None
    return normalize(cfg, check_lists)


def retention_share(cfg, decks, pairs, served=None):
    """The retention source's share of the main stew's batches: recipe.retention_share, or for "field" the field share
    of the uncovered cells the archive can serve (`served(x, y)`: both decks in the archive's field; None: all) among
    the cells the main stew is distilled on (the covered ones and those) - so every cell keeps its field weight. 0 when
    no uncovered cell is served."""
    sh = shares(decks)
    unc = uncovered_cells(decks, pairs)
    serv = [c for c in unc if served is None or served(*c)]
    if not serv:
        return 0.0
    rs = cfg['recipe']['retention_share']
    if rs != 'field':
        return float(rs)
    u = sum(cell_weight(sh, x, y) for x, y in serv)
    lost = sum(cell_weight(sh, x, y) for x, y in unc if (x, y) not in serv)
    return u / (1.0 - lost)
