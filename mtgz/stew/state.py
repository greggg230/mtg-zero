"""state.json (docs/ENGINE.md 239.2): the stew's lineage and phase machine, written atomically.

    {"v": 1, "name", "created", "updated",
     "round": g, "phase": "train" | "distil_decks" | "judge_deck_gap" | "distil_main" | "judge",
     "field": {"decks": [{"name", "share", "alias"}], "candidates": [...]},     # the live field (add-deck moves it)
     "nets": {"main": {"versions": [V, ...]}, "deck:<X>": {...},
              "spec:<A>|<B>": {"run", "farm_run", "joined", "versions": [...]}},
     "turn": {"spec", "start", "target", "launched"} | null,                  # the training turn under way
     "pending": [{"op": "add-deck", "name", "share", "alias", "candidate", "asked"}],
     "yield": {"runs": {"<run>": "paused by the stew" | "was paused"}, "farm": {"<farm run>": "<its state before>"}},
     "farm_to_pause": ["<farm run>", ...],                                     # specialists' farm runs still active
     "held": {"tag", "time"} | null,
     "history": [{"tag", "start", "end", "result"}]}

A version V of a net: {"gen": k, "path", "sha256", "time", ...} - the main stew's and deck stews' with "parents"
(warm start), "sources" (the sources file), "sources_sha256", "teachers"; a specialist's with "iter" (its run's
checkpoint). Generation g of the loop (round g: phases tagged '<phase>@g') reads version g of every net and writes
version g + 1; version 0 is the start (the source run's checkpoint, its copies, the seeds). Paths inside the stew
directory are kept relative to it.
"""
import hashlib
import json
import os
import time

V = 1


def now():
    return round(time.time(), 1)


def stamp(t=None):
    return time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(t if t is not None else time.time()))


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for blk in iter(lambda: f.read(1 << 22), b''):
            h.update(blk)
    return h.hexdigest()


def jload(path, default=None):
    try:
        with open(path, encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def jsave(path, obj):
    """Atomically: a temporary file beside it, then os.replace (a kill leaves the old file or the new one)."""
    d = os.path.dirname(os.path.abspath(path))
    os.makedirs(d, exist_ok=True)
    tmp = f'{path}.{os.getpid()}.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='') as f:
        json.dump(obj, f, indent=1)
        f.write('\n')
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


def text_save(path, text):
    d = os.path.dirname(os.path.abspath(path))
    os.makedirs(d, exist_ok=True)
    tmp = f'{path}.{os.getpid()}.tmp'
    with open(tmp, 'w', encoding='utf-8', newline='') as f:
        f.write(text)
    os.replace(tmp, path)


def new(cfg):
    t = now()
    return {'v': V, 'name': cfg['name'], 'created': t, 'updated': t, 'round': 0, 'phase': 'train',
            'field': {'decks': [dict(d) for d in cfg['decks']], 'candidates': list(cfg['candidates'])},
            'nets': {}, 'turn': None, 'pending': [], 'yield': {'runs': {}, 'farm': {}}, 'farm_to_pause': [],
            'held': None, 'history': []}


class State:
    """state.json of a stew directory: `d` the dict; `save()` writes it atomically."""

    def __init__(self, stew_dir, d):
        self.dir = os.path.abspath(stew_dir)
        self.d = d

    @property
    def path(self):
        return os.path.join(self.dir, 'state.json')

    @classmethod
    def load(cls, stew_dir):
        p = os.path.join(stew_dir, 'state.json')
        d = jload(p)
        if d is None:
            raise FileNotFoundError(f'{p}: no stew state (tools/stew.py init first)')
        if d.get('v') != V:
            raise ValueError(f'{p}: state version {d.get("v")!r}, this checkout reads {V}')
        return cls(stew_dir, d)

    def save(self):
        self.d['updated'] = now()
        jsave(self.path, self.d)

    # ---------------------------------------------------------------- paths
    def rel(self, p):
        """A path for state.json: relative to the stew directory when inside it, else absolute (forward slashes)."""
        a = os.path.abspath(p)
        try:
            r = os.path.relpath(a, self.dir)
        except ValueError:                      # (another drive)
            r = None
        if r is not None and not r.startswith('..'):
            return r.replace('\\', '/')
        return a.replace('\\', '/')

    def abs(self, p):
        return os.path.normpath(p if os.path.isabs(p) else os.path.join(self.dir, p))

    # ---------------------------------------------------------------- nets
    def net(self, name):
        return self.d['nets'][name]

    def version(self, name, gen):
        """Net `name`'s version `gen` (dict) or None."""
        for v in self.d['nets'].get(name, {}).get('versions', []):
            if v['gen'] == gen:
                return v
        return None

    def latest(self, name):
        vs = self.d['nets'].get(name, {}).get('versions', [])
        return max(vs, key=lambda v: v['gen']) if vs else None

    def add_version(self, name, v):
        n = self.d['nets'].setdefault(name, {'versions': []})
        n['versions'] = [x for x in n['versions'] if x['gen'] != v['gen']] + [v]
        n['versions'].sort(key=lambda x: x['gen'])

    def ckpt(self, name, gen):
        v = self.version(name, gen)
        if v is None:
            raise KeyError(f'{name}: no version {gen}')
        return self.abs(v['path'])

    # ---------------------------------------------------------------- the field
    def decks(self):
        return self.d['field']['decks']

    def candidates(self):
        return self.d['field']['candidates']

    def tag(self):
        return f"{self.d['phase']}@{self.d['round']}"

    def log(self, tag, start, result=None):
        self.d['history'].append({'tag': tag, 'start': start, 'end': now(), 'result': result})
