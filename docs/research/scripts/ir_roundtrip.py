"""Prototype a GENERIC (reflection-based, zero per-class code) JSON IR for
cube CardPrintings and check it round-trips losslessly.

Encoding:
  * dataclass / plain object -> {"$t": "<module>.<Class>", "<field>": ...}
    (dataclasses: declared fields only; plain objects: __dict__)
  * Enum -> {"$e": "<module>.<Enum>", "v": "<NAME>"}
  * tuple -> list; frozenset/set -> {"$fs": [sorted items]} / {"$set": [...]}
  * dict -> {"$d": [[k, v], ...]}
  * float inf -> {"$inf": 1}
Anything else (callables, classes, regexes) raises -> counted as a blocker.

Decoding bypasses __init__/__post_init__ (object.__new__ + setattr) so
classes with custom constructors (OneOf(*filters), EventPattern
normalisation) restore exactly.

Checks per card: decode(encode(p)) == p   (dataclass structural equality)
                 encode(decode(encode(p))) == encode(p)   (fixpoint)
Read-only against mtg-bot. Run from the mtg-bot repo root, PYTHONPATH=src.
"""
from __future__ import annotations

import dataclasses
import enum
import gzip
import importlib
import json
import math
import sys
from pathlib import Path

from mtg_bot.cards.parser.source import OracleParserCardSource

OUT = Path(__file__).resolve().parents[1]
# --sparse: omit dataclass fields equal to their declared default (decoder
# refills them from the class definition).
SPARSE = "--sparse" in sys.argv


class Blocker(Exception):
    pass


def enc(o):
    if o is None or isinstance(o, (bool, int, str)) and not isinstance(o, enum.Enum):
        return o
    if isinstance(o, float):
        if math.isinf(o):
            return {"$inf": 1 if o > 0 else -1}
        return o
    if isinstance(o, enum.Enum):
        return {"$e": f"{type(o).__module__}.{type(o).__qualname__}", "v": o.name}
    if isinstance(o, tuple):
        return [enc(x) for x in o]
    if isinstance(o, list):
        return {"$list": [enc(x) for x in o]}
    if isinstance(o, frozenset):
        return {"$fs": sorted((enc(x) for x in o), key=lambda v: json.dumps(v, sort_keys=True))}
    if isinstance(o, set):
        return {"$set": sorted((enc(x) for x in o), key=lambda v: json.dumps(v, sort_keys=True))}
    if isinstance(o, dict):
        return {"$d": [[enc(k), enc(v)] for k, v in o.items()]}
    if isinstance(o, type) or callable(o) and not hasattr(o, "__dict__"):
        raise Blocker(repr(o))
    cls = type(o)
    if callable(o) and not dataclasses.is_dataclass(o) and type(o).__call__ is not object.__call__:
        # functions, partials, bound methods
        if not hasattr(cls, "__dataclass_fields__") and cls.__module__ in ("builtins", "functools"):
            raise Blocker(repr(o))
    out = {"$t": f"{cls.__module__}.{cls.__qualname__}"}
    if dataclasses.is_dataclass(o):
        for f in dataclasses.fields(o):
            val = getattr(o, f.name)
            if SPARSE and _is_default(f, val):
                continue
            out[f.name] = enc(val)
        declared = {f.name for f in dataclasses.fields(o)}
        extra = {k: v for k, v in getattr(o, "__dict__", {}).items()
                 if k not in declared}
        if extra:
            out["$extra"] = {k: enc(v) for k, v in extra.items()}
    else:
        d = getattr(o, "__dict__", None)
        if d is None:
            raise Blocker(f"opaque {cls}")
        for k, v in d.items():
            out[k] = enc(v)
    return out


_cls_cache: dict[str, type] = {}


def _cls(path: str):
    c = _cls_cache.get(path)
    if c is None:
        mod, _, name = path.rpartition(".")
        # qualname may be nested; walk from the module.
        parts = path.split(".")
        for i in range(len(parts) - 1, 0, -1):
            try:
                m = importlib.import_module(".".join(parts[:i]))
            except ImportError:
                continue
            obj = m
            for p in parts[i:]:
                obj = getattr(obj, p)
            c = obj
            break
        _cls_cache[path] = c
    return c


def _is_default(f, val):
    if f.default is not dataclasses.MISSING:
        d = f.default
    elif f.default_factory is not dataclasses.MISSING:
        d = f.default_factory()
    else:
        return False
    try:
        return type(val) is type(d) and bool(val == d)
    except Exception:
        return False


def _fill_defaults(cls, obj):
    if not dataclasses.is_dataclass(cls):
        return
    for f in dataclasses.fields(cls):
        if f.name in obj.__dict__:
            continue
        if f.default is not dataclasses.MISSING:
            object.__setattr__(obj, f.name, f.default)
        elif f.default_factory is not dataclasses.MISSING:
            object.__setattr__(obj, f.name, f.default_factory())


def dec(v):
    if v is None or isinstance(v, (bool, int, float, str)):
        return v
    if isinstance(v, list):
        return tuple(dec(x) for x in v)
    if "$inf" in v:
        return math.inf * v["$inf"]
    if "$e" in v:
        return _cls(v["$e"])[v["v"]]
    if "$list" in v:
        return [dec(x) for x in v["$list"]]
    if "$fs" in v:
        return frozenset(dec(x) for x in v["$fs"])
    if "$set" in v:
        return set(dec(x) for x in v["$set"])
    if "$d" in v:
        return {dec(k): dec(x) for k, x in v["$d"]}
    cls = _cls(v["$t"])
    obj = object.__new__(cls)
    for k, x in v.items():
        if k in ("$t", "$extra"):
            continue
        object.__setattr__(obj, k, dec(x))
    for k, x in (v.get("$extra") or {}).items():
        object.__setattr__(obj, k, dec(x))
    _fill_defaults(cls, obj)
    return obj


def main():
    printings = OracleParserCardSource(pool="cube").get_printings()
    blockers, neq, nonfix = [], [], []
    total = 0
    sizes = {}
    payload = {}
    for name in sorted(printings):
        p = printings[name]
        try:
            e = enc(p)
        except Blocker as ex:
            blockers.append((name, str(ex)[:200]))
            continue
        s = json.dumps(e, sort_keys=True, separators=(",", ":"))
        sizes[name] = len(s)
        total += len(s)
        payload[name] = e
        back = dec(json.loads(s))
        try:
            if back != p:
                neq.append(name)
        except Exception as ex:  # noqa: BLE001
            neq.append(f"{name} (eq raised {type(ex).__name__})")
        s2 = json.dumps(enc(back), sort_keys=True, separators=(",", ":"))
        if s2 != s:
            nonfix.append(name)
    blob = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    gz = gzip.compress(blob)
    biggest = sorted(sizes.items(), key=lambda kv: -kv[1])[:10]
    result = {
        "cards": len(printings),
        "encoded": len(payload),
        "blockers": blockers,
        "not_equal_after_roundtrip": neq,
        "not_fixpoint": nonfix,
        "json_bytes_total": total,
        "json_bytes_mean": total / max(1, len(sizes)),
        "json_bytes_gzip": len(gz),
        "largest_cards": biggest,
    }
    print(json.dumps(result, indent=1))
    (OUT / "scripts" / ("_roundtrip_result_sparse.json" if SPARSE else "_roundtrip_result.json")).write_text(json.dumps(result, indent=1), encoding="utf-8")
    if "--dump" in sys.argv:
        (OUT / "scripts" / ("_cube_ir_sample_sparse.json" if SPARSE else "_cube_ir_sample.json")).write_text(
            json.dumps({k: payload[k] for k in list(payload)[:3] + ["Lightning Bolt", "Oko, Thief of Crowns", "Snapcaster Mage"] if k in payload}, indent=1),
            encoding="utf-8")


if __name__ == "__main__":
    main()
