"""Card IR v1: a lossless, versioned JSON form of parsed ``CardPrinting``s.

The oracle parser turns each card into a tree of frozen dataclasses
(``CardPrinting`` -> abilities -> effects / predicates / conditions /
formulas / costs ...). The card IR serializes that tree *generically*,
with no per-class code, so another engine (the Rust ``card_ir`` crate in
mtg-zero) can load the parser's output as data. Format and versioning
are documented in ``docs/CARD_IR.md``.

Encoding rules (IR v1):

* An object is ``{"$t": <tag>, <field>: <value>, ...}``. The tag is the
  class's ``__qualname__``; a name defined by more than one class in
  ``mtgz_ir.cards`` / ``mtgz_ir.effects`` is qualified with its module's
  last component (``"cube_walkers.IsPermanent"``).
* Sparse: a field equal to its declared default is omitted (the schema
  records every default).
* Tuples are JSON arrays; frozensets are arrays sorted by the canonical
  JSON of their elements; enum members are their (string) values;
  infinity is the string ``"inf"`` (``TargetSpec.count_max``).
* Keyword-typed slots (``KeywordSpelling``) carry the engine's keyword
  spelling in Python and a structured ``Keyword`` object in the IR.
* Derived attributes (``targets`` set in ``__post_init__``) are not
  serialized: the decoder re-runs ``__post_init__`` and a consumer
  recomputes them (see ``DERIVED_ATTRIBUTES``).
* Anything else -- callables, mutable containers, dicts, non-string
  enums, NaN, an undeclared attribute, a value outside its slot's
  type -- is a hard :class:`IRError`.

Every slot has a type in the schema (:func:`build_schema`): the
dataclass annotation, refined by the values observed in the encoded
pool where the annotation is vague (``object``, bare ``tuple``). A
document is ``{"format", "ir_version", "pool", "schema_sha256", "cards":
[{"oracle_id", "name", "printing"}, ...]}``.
"""

from __future__ import annotations

import collections
import dataclasses
import enum
import hashlib
import importlib
import json
import math
import pkgutil
import types
import typing
from dataclasses import dataclass, field
from typing import Any, Iterable, Mapping

IR_VERSION = 1
IR_FORMAT = "mtg-bot-card-ir"
SCHEMA_FORMAT = "mtg-bot-card-ir-schema"
# The format names above and the module labels below are the IR's first exporter's (mtg-bot, whose
# package was ``mtg_bot``): mtg-zero moved the pipeline in as ``mtgz_ir`` (docs/ENGINE.md 170) and kept
# them, because they are part of every document and of the schema's hash (each document's
# ``schema_sha256``, ``card_ir::SCHEMA_SHA256`` and generated.rs's doc comments). The schema names a
# class's or an enum's module as ``mtg_bot.<the module path below the package>``. Changing either is a
# deliberate re-export: a new schema hash, every IR file and generated.rs regenerated.
SCHEMA_MODULE_PACKAGE = "mtg_bot"
TAG = "$t"

# Non-field attributes some classes compute in ``__post_init__`` from
# their fields (``targets`` on eight effects that delegate targeting).
# Never serialized; the decoder re-runs ``__post_init__``.
DERIVED_ATTRIBUTES = frozenset({"targets"})

# Fields that are not part of IR v1. ``TriggeredAbility.trigger`` is the
# legacy string-key trigger path; no parsed card sets it (every trigger is
# ``event`` + ``predicate``), so the IR has no slot for it and a non-empty
# value is an error.
EXCLUDED_FIELDS = frozenset({("mtgz_ir.effects.base.TriggeredAbility", "trigger")})

# Category of a class = the first of these bases in its MRO; otherwise
# "CostAtom" for the duck-typed cost atoms (``payable`` + ``pay``);
# otherwise its topmost ``mtgz_ir`` base class (records such as
# ``CardPrinting`` / ``ManaCost`` are their own category).
CATEGORY_BASES: tuple[tuple[str, str], ...] = (
    ("mtgz_ir.effects.base", "Effect"),
    ("mtgz_ir.effects.object_predicates", "ObjectPredicate"),
    ("mtgz_ir.effects.state_predicates", "StatePredicate"),
    ("mtgz_ir.effects.predicates", "TriggerPredicate"),
    ("mtgz_ir.effects.conditions", "Condition"),
    ("mtgz_ir.effects.formulas", "Formula"),
    ("mtgz_ir.effects.pt_formulas", "PTFormula"),
    ("mtgz_ir.effects.modifiers", "LayerPayload"),
    ("mtgz_ir.effects.modifiers", "EventTransform"),
)
COST_ATOM = "CostAtom"

_TAG_SCAN_PACKAGES = ("mtgz_ir.cards", "mtgz_ir.effects")


class IRError(Exception):
    """A value the card IR cannot represent (hard error)."""


# ---------------------------------------------------------------------------
# Class registry: tags and categories
# ---------------------------------------------------------------------------


def qualname(cls: type) -> str:
    return f"{cls.__module__}.{cls.__qualname__}"


def _import_attr(module: str, name: str):
    obj = importlib.import_module(module)
    for part in name.split("."):
        obj = getattr(obj, part)
    return obj


_AMBIGUOUS: set[str] | None = None


def _ambiguous_short_names() -> set[str]:
    """Class names defined by more than one class in the scanned packages."""
    global _AMBIGUOUS
    if _AMBIGUOUS is None:
        seen: dict[str, set[str]] = collections.defaultdict(set)
        for pkg_name in _TAG_SCAN_PACKAGES:
            pkg = importlib.import_module(pkg_name)
            mods = [pkg_name] + [
                m.name for m in pkgutil.walk_packages(pkg.__path__, pkg_name + ".")
            ]
            for mod_name in mods:
                try:
                    mod = importlib.import_module(mod_name)
                except Exception:  # pragma: no cover - optional deps
                    continue
                for obj in vars(mod).values():
                    if (
                        isinstance(obj, type)
                        and obj.__module__ == mod_name
                        and (dataclasses.is_dataclass(obj) or issubclass(obj, enum.Enum))
                    ):
                        seen[obj.__qualname__].add(qualname(obj))
        _AMBIGUOUS = {n for n, qs in seen.items() if len(qs) > 1}
    return _AMBIGUOUS


def ir_tag(cls: type) -> str:
    """The ``$t`` tag of ``cls`` (also used as the enum name for enums)."""
    name = cls.__qualname__
    if name in _ambiguous_short_names():
        return f"{cls.__module__.rpartition('.')[2]}.{name}"
    return name


_CATEGORY_BASE_CLASSES: list[tuple[type, str]] | None = None


def _category_bases() -> list[tuple[type, str]]:
    global _CATEGORY_BASE_CLASSES
    if _CATEGORY_BASE_CLASSES is None:
        _CATEGORY_BASE_CLASSES = [
            (_import_attr(mod, name), name) for mod, name in CATEGORY_BASES
        ]
    return _CATEGORY_BASE_CLASSES


def category_of(cls: type) -> str:
    for base, name in _category_bases():
        if issubclass(cls, base):
            return name
    if callable(getattr(cls, "payable", None)) and callable(getattr(cls, "pay", None)):
        return COST_ATOM
    roots = [c for c in cls.__mro__ if c.__module__.startswith("mtgz_ir")]
    return (roots[-1] if roots else cls).__qualname__


# ---------------------------------------------------------------------------
# IR types
# ---------------------------------------------------------------------------
#
# Internally a type is a hashable tuple; ``type_to_json`` renders it.
#   ("bool",) ("int",) ("float",) ("str",) ("keyword",) ("null",) ("never",)
#   ("count",)            int, or infinity ("inf") -- TargetSpec.count_max
#   ("enum", tag)         a str-valued Enum
#   ("ref", (cat, ...))   an object whose class is in one of the categories
#   ("opt", T)            T or null
#   ("list", T)           tuple of T
#   ("set", T)            frozenset of T (sorted array)
#   ("tuple", (T, ...))   fixed-arity tuple
#   ("union", (T, ...))   one of several non-object types

NEVER = ("never",)
NULL = ("null",)
_PRIMS = {bool: ("bool",), int: ("int",), float: ("float",), str: ("str",)}


def type_to_json(t: tuple) -> dict:
    k = t[0]
    if k == "enum":
        return {"t": "enum", "enum": t[1]}
    if k == "ref":
        return {"t": "ref", "categories": list(t[1])}
    if k in ("opt", "list", "set"):
        return {"t": k, "of": type_to_json(t[1])}
    if k in ("tuple", "union"):
        return {"t": k, "items": [type_to_json(x) for x in t[1]]}
    return {"t": k}


def _join(a: tuple | None, b: tuple | None) -> tuple | None:
    """Least upper bound of two observed / declared types."""
    if a is None:
        return b
    if b is None or a == b:
        return a
    if a == NEVER:
        return b
    if b == NEVER:
        return a
    if a == NULL:
        return b if b[0] == "opt" else ("opt", b)
    if b == NULL:
        return _join(b, a)
    if a[0] == "opt" or b[0] == "opt":
        ia = a[1] if a[0] == "opt" else a
        ib = b[1] if b[0] == "opt" else b
        return ("opt", _join(ia, ib))
    if a[0] == b[0] == "ref":
        return ("ref", tuple(sorted(set(a[1]) | set(b[1]))))
    if a[0] == b[0] and a[0] in ("list", "set"):
        return (a[0], _join(a[1], b[1]))
    if a[0] == b[0] == "tuple" and len(a[1]) == len(b[1]):
        return ("tuple", tuple(_join(x, y) for x, y in zip(a[1], b[1])))
    if {a[0], b[0]} <= {"list", "tuple"}:
        items = []
        for t in (a, b):
            items.extend(t[1] if t[0] == "tuple" else [t[1]])
        out = NEVER
        for x in items:
            out = _join(out, x)
        return ("list", out)
    if {a, b} <= {("int",), ("count",)}:
        return ("count",)
    if {a, b} <= {("int",), ("float",)}:
        return ("float",)
    if {a, b} <= {("int",), ("float",), ("count",)}:
        return ("count",)
    parts = []
    for t in (a, b):
        parts.extend(t[1] if t[0] == "union" else [t])
    return ("union", tuple(sorted(set(parts), key=repr)))


# ---------------------------------------------------------------------------
# Annotations
# ---------------------------------------------------------------------------


class _Unresolved(Exception):
    pass


def _eval_annotation(raw, module_ns: Mapping[str, Any], extra_ns: Mapping[str, Any]):
    if not isinstance(raw, str):
        return raw
    ns = dict(extra_ns)
    ns.update(module_ns)
    try:
        return eval(raw, ns)  # noqa: S307 - annotations of our own classes
    except Exception as ex:  # NameError for TYPE_CHECKING-only names, ...
        raise _Unresolved(raw) from ex


def _ann_to_type(ann, resolve) -> tuple | None:
    """IR type for an annotation; ``None`` where it is too vague (filled
    from observed values). ``resolve`` evaluates nested string refs."""
    from mtgz_ir.keyword_vocab import KEYWORD_SPELLING

    if isinstance(ann, (str, typing.ForwardRef)):
        try:
            ann = resolve(ann.__forward_arg__ if isinstance(ann, typing.ForwardRef) else ann)
        except _Unresolved:
            return None
        return _ann_to_type(ann, resolve)
    if ann is None or ann is type(None):
        return NULL
    if ann is typing.Any or ann is object:
        return None
    origin = typing.get_origin(ann)
    args = typing.get_args(ann)
    if origin is typing.Annotated:
        if KEYWORD_SPELLING in args[1:]:
            return ("keyword",)
        return _ann_to_type(args[0], resolve)
    if origin is typing.ClassVar:
        return None
    if origin is typing.Union or origin is types.UnionType:
        out = None
        for a in args:
            t = _ann_to_type(a, resolve)
            if t is None:
                # A vague arm makes the union vague (filled from observed
                # values), but ``| None`` still makes it optional.
                return ("opt", None) if type(None) in args else None
            out = _join(out, t)
        return out
    if origin is tuple:
        if not args:
            return ("list", None)
        if len(args) == 2 and args[1] is Ellipsis:
            return ("list", _ann_to_type(args[0], resolve))
        return ("tuple", tuple(_ann_to_type(a, resolve) for a in args))
    if origin in (frozenset, set, typing.AbstractSet):
        return ("set", _ann_to_type(args[0], resolve) if args else None)
    if origin is typing.Literal:
        return ("str",) if all(isinstance(a, str) for a in args) else None
    if origin is not None:
        return None  # dict / list / Callable / Mapping: not IR slots
    if ann in (tuple,):
        return ("list", None)
    if ann in (frozenset, set):
        return ("set", None)
    if ann in _PRIMS:
        return _PRIMS[ann]
    if isinstance(ann, type):
        if issubclass(ann, enum.Enum):
            return ("enum", ir_tag(ann))
        if ann.__module__.startswith("mtgz_ir"):
            return ("ref", (category_of(ann),))
    return None


def _fill(ann_t: tuple | None, obs_t: tuple | None) -> tuple | None:
    """Replace vague (``None``) parts of the annotation type with the
    observed type. A slot only ever observed as ``None`` is ``opt(never)``."""
    if ann_t is None:
        return ("opt", NEVER) if obs_t == NULL else obs_t
    k = ann_t[0]
    if k == "list" and ann_t[1] is None and obs_t is not None and obs_t[0] == "tuple":
        return obs_t  # bare ``tuple`` holding fixed-arity records
    if k in ("list", "set"):
        inner_obs = None
        if obs_t is not None:
            if obs_t[0] == k:
                inner_obs = obs_t[1]
            elif obs_t[0] == "tuple" and k == "list":
                inner_obs = NEVER
                for x in obs_t[1]:
                    inner_obs = _join(inner_obs, x)
        inner = _fill(ann_t[1], inner_obs)
        return (k, inner if inner is not None else NEVER)
    if k == "opt":
        inner_obs = obs_t[1] if obs_t is not None and obs_t[0] == "opt" else (
            None if obs_t in (None, NULL) else obs_t
        )
        inner = _fill(ann_t[1], inner_obs)
        return ("opt", inner if inner is not None else NEVER)
    if k == "tuple":
        obs_items = obs_t[1] if obs_t is not None and obs_t[0] == "tuple" and len(obs_t[1]) == len(ann_t[1]) else [None] * len(ann_t[1])
        return ("tuple", tuple(
            (_fill(a, o) if a is not None else (o if o is not None else NEVER))
            for a, o in zip(ann_t[1], obs_items)
        ))
    if ann_t == ("float",) and obs_t is not None and (
        obs_t == ("count",) or (obs_t[0] == "union" and ("count",) in obs_t[1])
    ):
        return ("count",)  # int | infinity (TargetSpec.count_max)
    return ann_t


def _value_fits(v, t: tuple) -> bool:
    """Whether the value ``v`` is encodable under type ``t`` (deep)."""
    from mtgz_ir.keyword_vocab import is_keyword_spelling

    k = t[0]
    if k == "never":
        return False
    if k == "opt":
        return v is None or _value_fits(v, t[1])
    if k == "union":
        return any(_value_fits(v, x) for x in t[1])
    if k == "list":
        return isinstance(v, tuple) and all(_value_fits(x, t[1]) for x in v)
    if k == "set":
        return isinstance(v, frozenset) and all(_value_fits(x, t[1]) for x in v)
    if k == "tuple":
        return isinstance(v, tuple) and len(v) == len(t[1]) and all(
            _value_fits(x, s) for x, s in zip(v, t[1])
        )
    if k == "keyword":
        return _matches(v, t) and is_keyword_spelling(v)
    return _matches(v, t)


_JSON_KIND = {
    "null": "null", "bool": "bool", "int": "number", "float": "number",
    "count": "number", "str": "string", "enum": "string", "keyword": "object",
    "ref": "object", "list": "array", "set": "array", "tuple": "array",
}


def _check_unambiguous(t: tuple, where: str) -> None:
    """A union's alternatives must be distinguishable from JSON alone."""
    k = t[0]
    if k == "union":
        kinds = [_JSON_KIND.get(x[0], x[0]) for x in t[1]]
        if len(kinds) != len(set(kinds)) or "null" in kinds:
            raise IRError(f"{where}: ambiguous IR type {type_to_json(t)}; tighten the annotation")
        for x in t[1]:
            _check_unambiguous(x, where)
    elif k in ("opt", "list", "set"):
        if k == "opt" and t[1][0] == "opt":
            raise IRError(f"{where}: nested optional {type_to_json(t)}")
        _check_unambiguous(t[1], where)
    elif k == "tuple":
        for x in t[1]:
            _check_unambiguous(x, where)


def _prune_refs(t: tuple, present: set[str]) -> tuple:
    k = t[0]
    if k == "ref":
        cats = tuple(c for c in t[1] if c in present)
        return ("ref", cats) if cats else NEVER
    if k in ("opt", "list", "set"):
        return (k, _prune_refs(t[1], present))
    if k == "tuple":
        return ("tuple", tuple(_prune_refs(x, present) for x in t[1]))
    if k == "union":
        arms = [a for a in (_prune_refs(x, present) for x in t[1]) if a != NEVER]
        return arms[0] if len(arms) == 1 else ("union", tuple(arms))
    return t


def _shape(v) -> tuple:
    """Observed IR type of a Python value (objects -> their category)."""
    if v is None:
        return NULL
    if isinstance(v, enum.Enum):
        return ("enum", ir_tag(type(v)))
    if isinstance(v, bool):
        return ("bool",)
    if isinstance(v, int):
        return ("int",)
    if isinstance(v, float):
        return ("count",) if math.isinf(v) else ("float",)
    if isinstance(v, str):
        return ("str",)
    if isinstance(v, tuple):
        if not v:
            return ("list", NEVER)
        shapes = [_shape(x) for x in v]
        if all(s == shapes[0] for s in shapes):
            return ("list", shapes[0])
        return ("tuple", tuple(shapes))
    if isinstance(v, frozenset):
        out = NEVER
        for x in v:
            out = _join(out, _shape(x))
        return ("set", out)
    return ("ref", (category_of(type(v)),))


# ---------------------------------------------------------------------------
# Walking printings
# ---------------------------------------------------------------------------


def _check_value(v, path: str) -> None:
    """Hard errors for values no IR type covers."""
    if isinstance(v, (list, set, dict, bytes, bytearray)):
        raise IRError(f"{path}: mutable / unsupported container {type(v).__name__}")
    if isinstance(v, float) and math.isnan(v):
        raise IRError(f"{path}: NaN")
    if isinstance(v, enum.Enum) and not isinstance(v.value, str):
        raise IRError(f"{path}: non-string enum {v!r}")
    if isinstance(v, type) or (callable(v) and not hasattr(v, "__dict__")):
        raise IRError(f"{path}: callable / class value {v!r}")
    if isinstance(v, (types.FunctionType, types.MethodType, types.BuiltinFunctionType)):
        raise IRError(f"{path}: callable value {v!r}")


def object_fields(obj) -> list[tuple[str, Any]]:
    """``(name, value)`` of the IR fields of an object (excluded fields
    and derived attributes are skipped; undeclared attributes raise)."""
    cls = type(obj)
    q = qualname(cls)
    if dataclasses.is_dataclass(obj):
        names = [f.name for f in dataclasses.fields(obj)]
        extra = set(getattr(obj, "__dict__", {})) - set(names) - DERIVED_ATTRIBUTES
        if extra:
            raise IRError(f"{q}: undeclared attributes {sorted(extra)}")
        return [(n, getattr(obj, n)) for n in names if (q, n) not in EXCLUDED_FIELDS]
    d = getattr(obj, "__dict__", None)
    if d is None or getattr(cls, "__slots__", None):
        raise IRError(f"{q}: opaque object without __dict__ fields")
    return sorted(d.items())


def iter_objects(root) -> Iterable[tuple[Any, str]]:
    """Every object (dataclass / plain class instance) reachable from
    ``root``, with a path, in deterministic order."""
    stack = [(root, "printing")]
    seen: set[int] = set()
    while stack:
        v, path = stack.pop()
        _check_value(v, path)
        if v is None or isinstance(v, (bool, int, float, str, enum.Enum)):
            continue
        if isinstance(v, (tuple, frozenset)):
            items = v if isinstance(v, tuple) else sorted(v, key=repr)
            for i, x in reversed(list(enumerate(items))):
                stack.append((x, f"{path}[{i}]"))
            continue
        if id(v) in seen:
            continue
        seen.add(id(v))
        yield v, path
        for name, x in reversed(object_fields(v)):
            stack.append((x, f"{path}.{name}"))


# ---------------------------------------------------------------------------
# Schema
# ---------------------------------------------------------------------------


@dataclass
class FieldSchema:
    name: str
    annotation: str | None
    type: tuple
    type_source: str  # "annotation" | "annotation+observed" | "observed"
    required: bool
    default: Any = None  # Python default (a value, not IR JSON)
    default_factory: Any = None
    annotation_mismatch: str | None = None


@dataclass
class ClassSchema:
    cls: type
    tag: str
    category: str
    kind: str  # "dataclass" | "plain"
    fields: list[FieldSchema] = field(default_factory=list)
    has_post_init: bool = False

    def field_map(self) -> dict[str, FieldSchema]:
        return {f.name: f for f in self.fields}


@dataclass
class Schema:
    classes: dict[str, ClassSchema]  # tag -> schema
    enums: dict[str, type]  # tag -> enum class
    by_class: dict[type, ClassSchema]

    def categories(self) -> dict[str, list[str]]:
        out: dict[str, list[str]] = collections.defaultdict(list)
        for tag, cs in self.classes.items():
            out[cs.category].append(tag)
        return {k: sorted(v) for k, v in sorted(out.items())}

    def class_for_tag(self, tag: str) -> ClassSchema:
        cs = self.classes.get(tag)
        if cs is None:
            raise IRError(f"unknown class tag {tag!r}")
        return cs

    def to_json(self) -> dict:
        return schema_to_json(self)

    def sha256(self) -> str:
        blob = json.dumps(self.to_json(), sort_keys=True, separators=(",", ":"))
        return hashlib.sha256(blob.encode()).hexdigest()


def _field_default(f: dataclasses.Field):
    if f.default is not dataclasses.MISSING:
        return True, f.default, None
    if f.default_factory is not dataclasses.MISSING:  # type: ignore[misc]
        return True, None, f.default_factory  # type: ignore[misc]
    return False, None, None


def build_schema(printings: Iterable) -> Schema:
    """Schema of every class reachable from ``printings``: per field the
    annotation, the IR type, and the default. Vague annotations are
    completed from the observed values; an annotation that rejects an
    observed value is recorded as ``annotation_mismatch`` and the
    observed type is used."""
    from mtgz_ir import keyword_vocab as kw_mod

    observed: dict[type, dict[str, tuple]] = collections.defaultdict(dict)
    values: dict[type, dict[str, dict[int, Any]]] = collections.defaultdict(
        lambda: collections.defaultdict(dict)
    )
    classes: set[type] = set()
    enums: set[type] = set()

    def note_enums(v):
        if isinstance(v, enum.Enum):
            enums.add(type(v))
        elif isinstance(v, (tuple, frozenset)):
            for x in v:
                note_enums(x)

    def observe(root):
        for obj, _path in iter_objects(root):
            cls = type(obj)
            classes.add(cls)
            obs = observed[cls]
            vals = values[cls]
            for name, v in object_fields(obj):
                obs[name] = _join(obs.get(name), _shape(v))
                vals[name].setdefault(id(v), v)
                note_enums(v)

    for p in printings:
        observe(p)
    # Object-valued defaults (``Duration = Permanent()``) belong to the
    # schema even when no printing uses them; repeat until closed.
    done: set[type] = set()
    while classes - done:
        for cls in sorted(classes - done, key=qualname):
            done.add(cls)
            if not dataclasses.is_dataclass(cls):
                continue
            for f in dataclasses.fields(cls):
                has_default, default, factory = _field_default(f)
                if has_default and (qualname(cls), f.name) not in EXCLUDED_FIELDS:
                    dv = factory() if factory is not None else default
                    note_enums(dv)
                    if not isinstance(dv, (type(None), bool, int, float, str, enum.Enum)):
                        observe(("default", dv))

    # Keyword slots render through the Keyword dataclass.
    classes.add(kw_mod.Keyword)
    for t in (kw_mod.KeywordKind, kw_mod.ProtectionQuality):
        enums.add(t)

    by_class: dict[type, ClassSchema] = {}
    problems: list[str] = []
    extra_ns: dict[str, Any] = {"typing": typing}
    for name in dir(typing):
        extra_ns.setdefault(name, getattr(typing, name))
    for cls in classes | set(c for c, _ in _category_bases()):
        extra_ns.setdefault(cls.__name__, cls)

    for cls in sorted(classes, key=qualname):
        module_ns = vars(importlib.import_module(cls.__module__))

        def resolve(raw, _m=module_ns):
            return _eval_annotation(raw, _m, extra_ns)

        cs = ClassSchema(
            cls=cls, tag=ir_tag(cls), category=category_of(cls),
            kind="dataclass" if dataclasses.is_dataclass(cls) else "plain",
            has_post_init=hasattr(cls, "__post_init__"),
        )
        obs = observed.get(cls, {})
        if dataclasses.is_dataclass(cls):
            for f in dataclasses.fields(cls):
                if (qualname(cls), f.name) in EXCLUDED_FIELDS:
                    continue
                raw = f.type if isinstance(f.type, str) else getattr(f.type, "__name__", repr(f.type))
                ann_t = _ann_to_type(f.type, resolve)
                obs_t = obs.get(f.name)
                has_default, default, factory = _field_default(f)
                seen = list(values.get(cls, {}).get(f.name, {}).values())
                if has_default:
                    dv = factory() if factory is not None else default
                    obs_t = _join(obs_t, _shape(dv))
                    seen.append(dv)
                t = _fill(ann_t, obs_t)
                source = "annotation" if ann_t is not None and t == ann_t else (
                    "observed" if ann_t is None else "annotation+observed"
                )
                mismatch = None
                if t is None:
                    t = NEVER
                if not all(_value_fits(v, t) for v in seen):
                    mismatch = f"declared {raw}, observed {type_to_json(obs_t)}"
                    t = obs_t if obs_t is not None else NEVER
                    source = "observed"
                    bad = [v for v in seen if not _value_fits(v, t)]
                    if bad:
                        problems.append(
                            f"{qualname(cls)}.{f.name}: cannot type values such as "
                            f"{bad[0]!r} (declared {raw}); tighten the annotation"
                        )
                try:
                    _check_unambiguous(t, f"{qualname(cls)}.{f.name}")
                except IRError as ex:
                    problems.append(str(ex))
                cs.fields.append(FieldSchema(
                    name=f.name, annotation=raw, type=t, type_source=source,
                    required=not has_default, default=default,
                    default_factory=factory, annotation_mismatch=mismatch,
                ))
        else:
            for name in sorted(obs):
                t = obs[name]
                bad = [v for v in values[cls][name].values() if not _value_fits(v, t)]
                if bad:
                    problems.append(f"{qualname(cls)}.{name}: cannot type values such as {bad[0]!r}")
                try:
                    _check_unambiguous(t, f"{qualname(cls)}.{name}")
                except IRError as ex:
                    problems.append(str(ex))
                cs.fields.append(FieldSchema(
                    name=name, annotation=None, type=t,
                    type_source="observed", required=True,
                ))
        by_class[cls] = cs

    # A declared category none of whose classes is reachable (an
    # annotation naming a record no encoded card uses) can hold no value:
    # prune it from ref types (``never`` when nothing is left).
    present = {cs.category for cs in by_class.values()}
    for cs in by_class.values():
        for f in cs.fields:
            f.type = _prune_refs(f.type, present)

    tags = collections.Counter(cs.tag for cs in by_class.values())
    dup = [t for t, n in tags.items() if n > 1]
    if dup:
        problems.append(f"duplicate class tags: {dup}")
    if problems:
        raise IRError("cannot build the IR schema:\n  " + "\n  ".join(problems))
    return Schema(
        classes={cs.tag: cs for cs in by_class.values()},
        enums={ir_tag(e): e for e in sorted(enums, key=qualname)},
        by_class=by_class,
    )


def schema_module(module: str) -> str:
    """The schema's label for a class's module: this package's modules under ``SCHEMA_MODULE_PACKAGE``,
    any other module (a test's probe classes) as it is."""
    package, dot, rest = module.partition(".")
    if package != __name__.partition(".")[0]:
        return module
    return SCHEMA_MODULE_PACKAGE + dot + rest


def schema_to_json(schema: Schema) -> dict:
    enc = Encoder(schema)
    classes = {}
    for tag in sorted(schema.classes):
        cs = schema.classes[tag]
        fields = []
        for f in cs.fields:
            entry: dict[str, Any] = {
                "name": f.name,
                "type": type_to_json(f.type),
                "type_source": f.type_source,
                "required": f.required,
            }
            if f.annotation is not None:
                entry["annotation"] = f.annotation
            if not f.required:
                dv = f.default_factory() if f.default_factory is not None else f.default
                entry["default"] = enc.encode_value(dv, f.type, f"{tag}.{f.name}<default>")
            if f.annotation_mismatch:
                entry["annotation_mismatch"] = f.annotation_mismatch
            fields.append(entry)
        classes[tag] = {
            "module": schema_module(cs.cls.__module__),
            "qualname": cs.cls.__qualname__,
            "category": cs.category,
            "kind": cs.kind,
            "fields": fields,
        }
    enums = {
        tag: {
            "module": schema_module(e.__module__),
            "qualname": e.__qualname__,
            "members": [{"name": m.name, "value": m.value} for m in e],
        }
        for tag, e in sorted(schema.enums.items())
    }
    return {
        "format": SCHEMA_FORMAT,
        "ir_version": IR_VERSION,
        "tag_key": TAG,
        "derived_attributes": sorted(DERIVED_ATTRIBUTES),
        "categories": schema.categories(),
        "classes": classes,
        "enums": enums,
    }


# ---------------------------------------------------------------------------
# Encoding / decoding
# ---------------------------------------------------------------------------


def _canonical(j) -> str:
    return json.dumps(j, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def _is_default(val, f: FieldSchema) -> bool:
    if f.required:
        return False
    d = f.default_factory() if f.default_factory is not None else f.default
    if val is d:
        return True
    try:
        return type(val) is type(d) and bool(val == d)
    except Exception:  # pragma: no cover - exotic __eq__
        return False


class Encoder:
    def __init__(self, schema: Schema):
        self.schema = schema

    def encode_printing(self, printing) -> dict:
        return self.encode_value(printing, ("ref", (category_of(type(printing)),)), "printing")

    def encode_value(self, v, t: tuple, path: str):
        _check_value(v, path)
        k = t[0]
        if k == "opt":
            return None if v is None else self.encode_value(v, t[1], path)
        if k == "union":
            for alt in t[1]:
                if _matches(v, alt):
                    return self.encode_value(v, alt, path)
            raise IRError(f"{path}: {v!r} matches none of {type_to_json(t)}")
        if not _matches(v, t):
            raise IRError(f"{path}: {v!r} is not a {type_to_json(t)}")
        if k in ("bool", "int", "str", "float"):
            return v
        if k == "count":
            if isinstance(v, float) and math.isinf(v):
                return "inf" if v > 0 else "-inf"
            return v
        if k == "enum":
            return v.value
        if k == "keyword":
            from mtgz_ir.keyword_vocab import parse_keyword

            try:
                kw = parse_keyword(v)
            except ValueError as ex:
                raise IRError(f"{path}: {ex}") from None
            return self.encode_object(kw, path)
        if k == "list":
            return [self.encode_value(x, t[1], f"{path}[{i}]") for i, x in enumerate(v)]
        if k == "tuple":
            return [self.encode_value(x, s, f"{path}[{i}]") for i, (x, s) in enumerate(zip(v, t[1]))]
        if k == "set":
            items = [self.encode_value(x, t[1], f"{path}{{}}") for x in v]
            return sorted(items, key=_canonical)
        if k == "ref":
            return self.encode_object(v, path)
        raise IRError(f"{path}: no IR type {t!r}")  # never / null

    def encode_object(self, obj, path: str) -> dict:
        cs = self.schema.by_class.get(type(obj))
        if cs is None:
            raise IRError(f"{path}: class {qualname(type(obj))} is not in the schema")
        fm = cs.field_map()
        out: dict[str, Any] = {TAG: cs.tag}
        values = object_fields(obj)
        if dataclasses.is_dataclass(obj):
            for (q, n) in EXCLUDED_FIELDS:
                if q == qualname(type(obj)):
                    f0 = next(f for f in dataclasses.fields(obj) if f.name == n)
                    has_default, d, fac = _field_default(f0)
                    if getattr(obj, n) != (fac() if fac else d):
                        raise IRError(f"{path}.{n}: field is not part of IR v1 (must stay default)")
        for name, v in values:
            f = fm.get(name)
            if f is None:
                raise IRError(f"{path}: {cs.tag} has no schema field {name!r}")
            if cs.kind == "dataclass" and _is_default(v, f):
                continue
            out[name] = self.encode_value(v, f.type, f"{path}.{name}")
        return out


def _matches(v, t: tuple) -> bool:
    k = t[0]
    if k == "null":
        return v is None
    if k == "bool":
        return isinstance(v, bool)
    if k == "int":
        return isinstance(v, int) and not isinstance(v, (bool, enum.Enum))
    if k == "float":
        return isinstance(v, (int, float)) and not isinstance(v, (bool, enum.Enum))
    if k == "count":
        return isinstance(v, (int, float)) and not isinstance(v, (bool, enum.Enum))
    if k in ("str", "keyword"):
        return isinstance(v, str) and not isinstance(v, enum.Enum)
    if k == "enum":
        return isinstance(v, enum.Enum) and ir_tag(type(v)) == t[1]
    if k in ("list", "tuple"):
        return isinstance(v, tuple) and (k == "list" or len(v) == len(t[1]))
    if k == "set":
        return isinstance(v, frozenset)
    if k == "ref":
        return (
            not isinstance(v, (str, int, float, tuple, frozenset, enum.Enum))
            and v is not None
            and category_of(type(v)) in t[1]
        )
    if k == "opt":
        return v is None or _matches(v, t[1])
    if k == "union":
        return any(_matches(v, x) for x in t[1])
    return False


class Decoder:
    def __init__(self, schema: Schema):
        self.schema = schema

    def decode_printing(self, j):
        return self.decode_object(j, "printing")

    def decode_value(self, j, t: tuple, path: str):
        k = t[0]
        if k == "opt":
            return None if j is None else self.decode_value(j, t[1], path)
        if k == "union":
            for alt in t[1]:
                if _json_matches(j, alt):
                    return self.decode_value(j, alt, path)
            raise IRError(f"{path}: {j!r} matches none of {type_to_json(t)}")
        if k == "bool":
            if not isinstance(j, bool):
                raise IRError(f"{path}: expected bool, got {j!r}")
            return j
        if k == "int":
            if not isinstance(j, int) or isinstance(j, bool):
                raise IRError(f"{path}: expected int, got {j!r}")
            return j
        if k == "float":
            if not isinstance(j, (int, float)) or isinstance(j, bool):
                raise IRError(f"{path}: expected number, got {j!r}")
            return j
        if k == "count":
            if j == "inf":
                return math.inf
            if j == "-inf":
                return -math.inf
            if not isinstance(j, (int, float)) or isinstance(j, bool):
                raise IRError(f"{path}: expected number or 'inf', got {j!r}")
            return j
        if k == "str":
            if not isinstance(j, str):
                raise IRError(f"{path}: expected string, got {j!r}")
            return j
        if k == "enum":
            e = self.schema.enums.get(t[1])
            if e is None:
                raise IRError(f"{path}: unknown enum {t[1]!r}")
            try:
                return e(j)
            except ValueError:
                raise IRError(f"{path}: {j!r} is not a {t[1]} value") from None
        if k == "keyword":
            return self.decode_object(j, path).spelling
        if k in ("list", "set"):
            if not isinstance(j, list):
                raise IRError(f"{path}: expected array, got {j!r}")
            items = [self.decode_value(x, t[1], f"{path}[{i}]") for i, x in enumerate(j)]
            return tuple(items) if k == "list" else frozenset(items)
        if k == "tuple":
            if not isinstance(j, list) or len(j) != len(t[1]):
                raise IRError(f"{path}: expected {len(t[1])}-array, got {j!r}")
            return tuple(self.decode_value(x, s, f"{path}[{i}]") for i, (x, s) in enumerate(zip(j, t[1])))
        if k == "ref":
            obj = self.decode_object(j, path)
            if category_of(type(obj)) not in t[1]:
                raise IRError(f"{path}: {type(obj).__name__} is not one of {list(t[1])}")
            return obj
        raise IRError(f"{path}: cannot decode type {t!r}")

    def decode_object(self, j, path: str):
        if not isinstance(j, dict) or TAG not in j:
            raise IRError(f"{path}: expected an object with {TAG!r}, got {j!r}")
        cs = self.schema.class_for_tag(j[TAG])
        fm = cs.field_map()
        unknown = set(j) - set(fm) - {TAG}
        if unknown:
            raise IRError(f"{path}: {cs.tag} has no fields {sorted(unknown)}")
        obj = object.__new__(cs.cls)
        for f in cs.fields:
            if f.name in j:
                v = self.decode_value(j[f.name], f.type, f"{path}.{f.name}")
            elif f.required:
                raise IRError(f"{path}: {cs.tag} is missing required field {f.name!r}")
            else:
                v = f.default_factory() if f.default_factory is not None else f.default
            object.__setattr__(obj, f.name, v)
        if cs.kind == "dataclass":
            for (q, n) in EXCLUDED_FIELDS:
                if q == qualname(cs.cls):
                    f0 = next(f for f in dataclasses.fields(cs.cls) if f.name == n)
                    has_default, d, fac = _field_default(f0)
                    object.__setattr__(obj, n, fac() if fac else d)
            if cs.has_post_init:
                obj.__post_init__()
        return obj


def _json_matches(j, t: tuple) -> bool:
    k = t[0]
    if k == "null":
        return j is None
    if k == "bool":
        return isinstance(j, bool)
    if k in ("int",):
        return isinstance(j, int) and not isinstance(j, bool)
    if k in ("float", "count"):
        return (isinstance(j, (int, float)) and not isinstance(j, bool)) or (k == "count" and j in ("inf", "-inf"))
    if k in ("str", "enum"):
        return isinstance(j, str)
    if k in ("list", "set", "tuple"):
        return isinstance(j, list)
    if k in ("ref", "keyword"):
        return isinstance(j, dict)
    if k == "opt":
        return j is None or _json_matches(j, t[1])
    return False


# ---------------------------------------------------------------------------
# Strict structural equality (types included)
# ---------------------------------------------------------------------------


def strict_equal(a, b) -> bool:
    """``a == b`` with exact types at every node (a ``StrEnum`` member is
    not equal to its string value here) and identical non-field
    attributes."""
    if type(a) is not type(b):
        return False
    if isinstance(a, float) and math.isnan(a):  # pragma: no cover
        return False
    if a is None or isinstance(a, (bool, int, float, str, enum.Enum)):
        return a == b
    if isinstance(a, tuple):
        return len(a) == len(b) and all(strict_equal(x, y) for x, y in zip(a, b))
    if isinstance(a, frozenset):
        if len(a) != len(b):
            return False
        rb = list(b)
        for x in a:
            for i, y in enumerate(rb):
                if strict_equal(x, y):
                    del rb[i]
                    break
            else:
                return False
        return True
    da = dict(getattr(a, "__dict__", {}))
    db = dict(getattr(b, "__dict__", {}))
    if da.keys() != db.keys():
        return False
    return all(strict_equal(da[k], db[k]) for k in da)


# ---------------------------------------------------------------------------
# Documents
# ---------------------------------------------------------------------------


def _oracle_ids(pool: str) -> dict[str, dict]:
    from mtgz_ir.cards.parser.source import _load_scryfall_data

    out: dict[str, dict] = {}
    for c in _load_scryfall_data(pool):
        out.setdefault(c["name"], c)
    return out


def pool_printings(pool: str) -> dict:
    from mtgz_ir.cards.parser.source import OracleParserCardSource

    return OracleParserCardSource(pool=pool).get_printings()


def build_document(pool: str, printings: Mapping | None = None, schema: Schema | None = None) -> tuple[dict, Schema]:
    """The IR document for ``pool`` and the schema it was encoded with."""
    if printings is None:
        printings = pool_printings(pool)
    if schema is None:
        schema = build_schema(printings[n] for n in sorted(printings))
    raw = _oracle_ids(pool)
    enc = Encoder(schema)
    cards = []
    for name in sorted(printings):
        rec = raw.get(name, {})
        entry: dict[str, Any] = {
            "oracle_id": rec.get("oracle_id"),
            "name": name,
        }
        if rec.get("custom_card"):
            entry["custom"] = True
        entry["printing"] = enc.encode_printing(printings[name])
        cards.append(entry)
    doc = {
        "format": IR_FORMAT,
        "ir_version": IR_VERSION,
        "pool": pool,
        "schema_sha256": schema.sha256(),
        "cards": cards,
    }
    return doc, schema


def decode_document(doc: Mapping, schema: Schema) -> dict:
    """``{name: CardPrinting}`` from an IR document."""
    if doc.get("format") != IR_FORMAT:
        raise IRError(f"not a card IR document: format={doc.get('format')!r}")
    if doc.get("ir_version") != IR_VERSION:
        raise IRError(f"unsupported ir_version {doc.get('ir_version')!r} (this decoder reads {IR_VERSION})")
    if doc.get("schema_sha256") != schema.sha256():
        raise IRError("document was encoded with a different schema")
    dec = Decoder(schema)
    return {c["name"]: dec.decode_printing(c["printing"]) for c in doc["cards"]}


def dumps(j) -> str:
    """Canonical JSON text of an IR document or schema."""
    return json.dumps(j, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


__all__ = [
    "DERIVED_ATTRIBUTES",
    "Decoder",
    "Encoder",
    "IRError",
    "IR_FORMAT",
    "IR_VERSION",
    "SCHEMA_FORMAT",
    "Schema",
    "build_document",
    "build_schema",
    "category_of",
    "decode_document",
    "dumps",
    "ir_tag",
    "schema_to_json",
    "strict_equal",
]
