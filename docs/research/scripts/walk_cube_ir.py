"""Walk every PowerLSV-cube CardPrinting's object graph and inventory the
classes, flags and serialization blockers a Rust card-IR interpreter would
need.

Read-only against the mtg-bot repo. Run from the mtg-bot repo root:

    PYTHONPATH=src .venv-py313/Scripts/python.exe \
        "C:/Users/Gregg Keithley/mtg-zero/docs/research/scripts/walk_cube_ir.py"

Writes ``card_ir_inventory.json`` next to this script's parent dir
(``docs/research/``).
"""

from __future__ import annotations

import collections
import dataclasses
import enum
import functools
import inspect
import json
import math
import pickle
import re
import sys
import types
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parents[1]

from mtg_bot.cards.parser.source import OracleParserCardSource, _load_scryfall_data  # noqa: E402
from mtg_bot.cards.base import CardPrinting, ManaCost  # noqa: E402
from mtg_bot.effects.base import (  # noqa: E402
    ActivatedAbility, ActivationCost, Effect, StaticAbility, TriggeredAbility,
)
from mtg_bot.effects.conditions import Condition  # noqa: E402
from mtg_bot.effects.object_predicates import ObjectPredicate  # noqa: E402
from mtg_bot.effects.state_predicates import StatePredicate  # noqa: E402
from mtg_bot.effects.predicates import TriggerPredicate  # noqa: E402
from mtg_bot.effects.modifiers import (  # noqa: E402
    ContinuousModifier, EventPattern, EventTransform, LayerPayload,
    ReplacementEffect,
)
from mtg_bot.effects.targets import SlotRef, TargetSpec  # noqa: E402
from mtg_bot.effects.target_filter import OneOf  # noqa: E402
from mtg_bot.effects.formulas import Formula  # noqa: E402
from mtg_bot.effects.durations import Duration  # noqa: E402

# Classes whose *fields* are behaviour switches the engine reads directly
# (as opposed to parameters of a class's own resolve()).  Every non-default
# field on these is a unit of engine work.
RECORD_CLASSES = {
    "mtg_bot.cards.base.CardPrinting",
    "mtg_bot.cards.base.RoomHalf",
    "mtg_bot.effects.base.ActivationCost",
    "mtg_bot.effects.base.ActivatedAbility",
    "mtg_bot.effects.base.StaticAbility",
    "mtg_bot.effects.base.TriggeredAbility",
}
# Pure data carriers: present on nearly every card, trivial to port.
CORE_DATA = {
    "mtg_bot.cards.base.CardPrinting",
    "mtg_bot.cards.base.ManaCost",
    "mtg_bot.effects.base.ActivationCost",
    "mtg_bot.effects.base.ActivatedAbility",
    "mtg_bot.effects.base.StaticAbility",
    "mtg_bot.effects.base.TriggeredAbility",
}


BASIC_NAMES = {"Plains", "Island", "Swamp", "Mountain", "Forest", "Wastes"}


def qual(cls) -> str:
    return f"{cls.__module__}.{cls.__qualname__}"


def category(cls) -> str:
    mro = inspect.getmro(cls) if isinstance(cls, type) else ()
    if issubclass(cls, enum.Enum):
        return "enum"
    if cls is CardPrinting or cls.__name__ in ("RoomHalf", "HandAbilityGrant"):
        return "card"
    if cls is ManaCost:
        return "cost"
    if issubclass(cls, TriggeredAbility):
        return "trigger"
    if issubclass(cls, StaticAbility):
        return "static"
    if issubclass(cls, ActivatedAbility):
        return "activated"
    if issubclass(cls, ActivationCost):
        return "cost"
    if issubclass(cls, Effect):
        return "effect"
    if issubclass(cls, Condition):
        return "condition"
    if issubclass(cls, (ObjectPredicate, StatePredicate, TriggerPredicate)):
        return "predicate"
    if issubclass(cls, (TargetSpec, SlotRef, OneOf)):
        return "target"
    if issubclass(cls, (ContinuousModifier, LayerPayload, Duration)):
        return "modifier"
    if issubclass(cls, (EventTransform, ReplacementEffect, EventPattern)):
        return "transform"
    if issubclass(cls, Formula):
        return "formula"
    name = cls.__name__
    # Duck-typed fallbacks (no common base class).
    if hasattr(cls, "payable") and hasattr(cls, "pay"):
        return "cost"
    if "Cost" in name:
        return "cost"
    if hasattr(cls, "resolve"):
        return "effect"
    if hasattr(cls, "matches") or hasattr(cls, "test"):
        return "predicate"
    if "Target" in name:
        return "target"
    return "other"


def _src_lines(cls) -> int:
    try:
        return len(inspect.getsourcelines(cls)[0])
    except (OSError, TypeError):
        return 0


def _src_loc(cls) -> str:
    try:
        f = inspect.getsourcefile(cls) or ""
        line = inspect.getsourcelines(cls)[1]
        f = f.replace("\\", "/")
        i = f.find("src/mtg_bot/")
        return f"{f[i:] if i >= 0 else f}:{line}"
    except (OSError, TypeError):
        return ""


def _default_of(f: dataclasses.Field):
    if f.default is not dataclasses.MISSING:
        return f.default
    if f.default_factory is not dataclasses.MISSING:  # type: ignore[misc]
        try:
            return f.default_factory()  # type: ignore[misc]
        except Exception:
            return dataclasses.MISSING
    return dataclasses.MISSING


def _is_default(val, default) -> bool:
    if default is dataclasses.MISSING:
        return False
    if val is default:
        return True
    try:
        return type(val) is type(default) and bool(val == default)
    except Exception:
        return False


def _vtype(v) -> str:
    if v is None:
        return "None"
    if isinstance(v, bool):
        return "bool"
    if isinstance(v, enum.Enum):
        return "enum:" + type(v).__name__
    if isinstance(v, (int, float, str)):
        if isinstance(v, float) and math.isinf(v):
            return "float(inf)"
        return type(v).__name__
    if isinstance(v, (tuple, list)):
        return type(v).__name__
    if isinstance(v, (frozenset, set)):
        return type(v).__name__
    if isinstance(v, dict):
        return "dict"
    if isinstance(v, type):
        return "CLASS-OBJECT"
    if callable(v) and not hasattr(v, "__dataclass_fields__"):
        return "CALLABLE"
    return "obj:" + type(v).__name__


class Walker:
    def __init__(self, card_names: set[str]):
        self.card_names = card_names
        self.classes: dict[str, dict] = {}
        self.class_objs: dict[str, type] = {}
        self.global_seen: set[int] = set()
        self.blockers: list[dict] = []
        self.name_strings: list[dict] = []
        # feature -> set(cards)
        self.features: dict[str, set] = collections.defaultdict(set)
        self.card_features: dict[str, set] = collections.defaultdict(set)
        self.card_classes: dict[str, set] = collections.defaultdict(set)
        self.max_depth: dict[str, int] = {}
        self.opaque_strings: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
        self.keep_alive: list = []

    def _cls_entry(self, cls) -> dict:
        q = qual(cls)
        e = self.classes.get(q)
        if e is None:
            e = {
                "class": cls.__qualname__,
                "module": cls.__module__,
                "category": category(cls),
                "dataclass": dataclasses.is_dataclass(cls),
                "frozen": bool(
                    dataclasses.is_dataclass(cls)
                    and getattr(cls, "__dataclass_params__", None)
                    and cls.__dataclass_params__.frozen
                ),
                "n_declared_fields": (
                    len(dataclasses.fields(cls)) if dataclasses.is_dataclass(cls) else None
                ),
                "source_lines": _src_lines(cls),
                "source": _src_loc(cls),
                "defines_resolve": "resolve" in cls.__dict__,
                "cards": set(),
                "instances": 0,
                "references": 0,
                "fields": collections.defaultdict(
                    lambda: {"types": collections.Counter(), "nondefault_cards": set()}
                ),
            }
            self.classes[q] = e
            self.class_objs[q] = cls
        return e

    def _flag(self, kind, card, path, obj, detail=""):
        self.blockers.append(
            {"kind": kind, "card": card, "path": path, "detail": detail
             or repr(obj)[:160]}
        )

    def walk_card(self, name: str, printing: CardPrinting):
        self.keep_alive.append(printing)
        visited: set[int] = set()
        self._walk(name, printing, "CardPrinting", visited, 0, parent_field=None)

    def _walk(self, card, obj, path, visited, depth, parent_field):
        self.max_depth[card] = max(self.max_depth.get(card, 0), depth)
        if obj is None or isinstance(obj, (bool, int, float, bytes)):
            return
        if isinstance(obj, str) and not isinstance(obj, enum.Enum):
            if obj in self.card_names and obj not in BASIC_NAMES and parent_field not in (
                "CardPrinting.name", "RoomHalf.name",
            ):
                self.name_strings.append(
                    {"card": card, "path": path, "value": obj,
                     "self_reference": obj == card}
                )
            return
        if isinstance(obj, enum.Enum):
            e = self._cls_entry(type(obj))
            e["cards"].add(card)
            e["references"] += 1
            self.card_classes[card].add(qual(type(obj)))
            self.features["enum:" + type(obj).__name__ + "." + obj.name].add(card)
            self.card_features[card].add("enum:" + type(obj).__name__ + "." + obj.name)
            return
        # --- serialization blockers --------------------------------
        if isinstance(obj, re.Pattern):
            self._flag("regex", card, path, obj)
            return
        if isinstance(obj, functools.partial):
            self._flag("functools.partial", card, path, obj,
                       f"partial({getattr(obj.func, '__qualname__', obj.func)})")
            self._walk(card, obj.func, path + ".func", visited, depth + 1, None)
            for i, a in enumerate(obj.args):
                self._walk(card, a, f"{path}.args[{i}]", visited, depth + 1, None)
            return
        if isinstance(obj, types.MethodType):
            self._flag("bound_method", card, path, obj,
                       f"{type(obj.__self__).__name__}.{obj.__func__.__name__}")
            return
        if isinstance(obj, types.FunctionType):
            kind = "lambda" if obj.__name__ == "<lambda>" else "function"
            if obj.__closure__:
                kind += "+closure"
            self._flag(kind, card, path, obj,
                       f"{obj.__module__}.{obj.__qualname__}")
            return
        if isinstance(obj, (types.BuiltinFunctionType, types.BuiltinMethodType)):
            self._flag("builtin_callable", card, path, obj)
            return
        if isinstance(obj, type):
            self._flag("class_object", card, path, obj, qual(obj))
            return
        # --- containers --------------------------------------------
        if isinstance(obj, (tuple, list, frozenset, set)):
            if id(obj) in visited:
                return
            visited.add(id(obj))
            items = obj if isinstance(obj, (tuple, list)) else sorted(obj, key=repr)
            for i, it in enumerate(items):
                self._walk(card, it, f"{path}[{i}]", visited, depth + 1, parent_field)
            return
        if isinstance(obj, dict):
            if id(obj) in visited:
                return
            visited.add(id(obj))
            for k, v in obj.items():
                self._walk(card, k, f"{path}{{key}}", visited, depth + 1, None)
                self._walk(card, v, f"{path}[{k!r}]", visited, depth + 1, None)
            return
        # --- structured objects ------------------------------------
        cls = type(obj)
        e = self._cls_entry(cls)
        e["cards"].add(card)
        e["references"] += 1
        q = qual(cls)
        self.card_classes[card].add(q)
        if id(obj) not in self.global_seen:
            self.global_seen.add(id(obj))
            e["instances"] += 1
        if id(obj) in visited:
            return
        visited.add(id(obj))
        if q not in CORE_DATA:
            self.features["class:" + q].add(card)
            self.card_features[card].add("class:" + q)

        cname = cls.__name__
        if dataclasses.is_dataclass(obj):
            fields = dataclasses.fields(obj)
            names = {f.name for f in fields}
            for f in fields:
                val = getattr(obj, f.name)
                d = _default_of(f)
                fe = e["fields"][f.name]
                fe["types"][_vtype(val)] += 1
                nondefault = not _is_default(val, d)
                if nondefault:
                    fe["nondefault_cards"].add(card)
                    if q in RECORD_CLASSES and q not in (
                        "mtg_bot.effects.base.ActivatedAbility",
                    ) and f.name not in (
                        "name", "mana_cost_str", "mana_cost", "type_line",
                        "supertypes", "card_types", "subtypes", "power",
                        "toughness", "produces", "keywords",
                        "triggered_abilities", "static_abilities",
                        "activated_abilities", "spell_effect", "effect",
                        "cost", "condition", "predicate", "event",
                        "mana", "mana_str", "trigger", "scope",
                        "replacement_effect", "continuous_effect",
                    ):
                        feat = f"field:{cname}.{f.name}"
                        self.features[feat].add(card)
                        self.card_features[card].add(feat)
                    if q == "mtg_bot.effects.base.ActivatedAbility" and f.name not in (
                        "cost", "effect"):
                        feat = f"field:{cname}.{f.name}"
                        self.features[feat].add(card)
                        self.card_features[card].add(feat)
                # Opaque string keys the engine dispatches on.
                if q == "mtg_bot.effects.base.TriggeredAbility" and f.name == "trigger" and val:
                    self.features["trigger_key:" + val].add(card)
                    self.card_features[card].add("trigger_key:" + val)
                    self.opaque_strings["TriggeredAbility.trigger"][val] += 1
                if q == "mtg_bot.effects.base.StaticAbility" and f.name == "scope":
                    self.features["static_scope:" + val].add(card)
                    self.card_features[card].add("static_scope:" + val)
                    self.opaque_strings["StaticAbility.scope"][val] += 1
                if q == "mtg_bot.cards.base.CardPrinting" and f.name == "keywords":
                    for kw in val:
                        self.features["keyword:" + kw].add(card)
                        self.card_features[card].add("keyword:" + kw)
                if isinstance(val, str) and val and q not in (
                    "mtg_bot.cards.base.CardPrinting", "mtg_bot.cards.base.RoomHalf",
                ) and f.name not in ("mana_str", "trigger", "scope"):
                    self.opaque_strings[f"{cname}.{f.name}"][val] += 1
                self._walk(card, val, f"{path}.{f.name}", visited, depth + 1,
                           f"{cname}.{f.name}")
            extra = {
                k: v for k, v in getattr(obj, "__dict__", {}).items() if k not in names
            }
            for k, v in extra.items():
                e["fields"]["(non-field) " + k]["types"][_vtype(v)] += 1
                self._walk(card, v, f"{path}.{k}", visited, depth + 1, None)
            return
        attrs = {}
        if hasattr(obj, "__dict__"):
            attrs.update(vars(obj))
        for slot in getattr(cls, "__slots__", ()) or ():
            if hasattr(obj, slot):
                attrs[slot] = getattr(obj, slot)
        if not attrs and not hasattr(obj, "__dict__"):
            self._flag("opaque_object", card, path, obj, qual(cls))
        for k, v in attrs.items():
            fe = e["fields"][k]
            fe["types"][_vtype(v)] += 1
            fe["nondefault_cards"].add(card)
            self._walk(card, v, f"{path}.{k}", visited, depth + 1, f"{cname}.{k}")


def greedy_unlock(card_feats: dict[str, set], universe_count: dict[str, int]):
    """Card-greedy staging: repeatedly adopt the card needing the fewest
    not-yet-implemented features (ties: features shared by more cards
    first). Returns the curve and the order in which features land."""
    done_feats: set = set()
    remaining = dict(card_feats)
    order = []
    curve = []
    unlocked = 0
    while remaining:
        best = None
        for c, fs in remaining.items():
            missing = fs - done_feats
            score = (len(missing), -sum(universe_count[f] for f in missing), c)
            if best is None or score < best[0]:
                best = (score, c, missing)
        _, c, missing = best
        for f in sorted(missing, key=lambda f: -universe_count[f]):
            done_feats.add(f)
            order.append(f)
        # Every card now fully covered unlocks at once.
        newly = [cc for cc, fs in remaining.items() if fs <= done_feats]
        for cc in newly:
            del remaining[cc]
        unlocked += len(newly)
        curve.append({"features": len(done_feats), "cards": unlocked,
                      "added": sorted(missing), "unlocked": sorted(newly)})
    return order, curve


def main():
    raw = _load_scryfall_data("cube")
    src = OracleParserCardSource(pool="cube")
    printings = src.get_printings()
    card_names = set(printings)
    # Names of every card in the whole repo pool too (for name-string scan).
    try:
        all_names = {c["name"] for c in _load_scryfall_data("all")}
    except Exception:
        all_names = set()
    w = Walker(card_names | all_names)
    pickle_fail = []
    for name in sorted(printings):
        p = printings[name]
        w.walk_card(name, p)
        try:
            pickle.dumps(p)
        except Exception as ex:  # noqa: BLE001
            pickle_fail.append({"card": name, "error": f"{type(ex).__name__}: {ex}"[:200]})

    n_cards = len(printings)
    basics = {c["name"] for c in raw if "Basic" in c.get("type_line", "")}

    classes_out = []
    for q, e in w.classes.items():
        fields_out = {}
        for fname, fe in e["fields"].items():
            fields_out[fname] = {
                "types": dict(fe["types"]),
                "nondefault_cards": len(fe["nondefault_cards"]),
                "example_cards": sorted(fe["nondefault_cards"])[:5],
            }
        classes_out.append({
            "qualname": q,
            "class": e["class"],
            "module": e["module"],
            "category": e["category"],
            "core_data": q in CORE_DATA,
            "dataclass": e["dataclass"],
            "frozen": e["frozen"],
            "n_declared_fields": e["n_declared_fields"],
            "n_fields_used_nondefault": sum(
                1 for fe in fields_out.values() if fe["nondefault_cards"]
            ),
            "defines_resolve": e["defines_resolve"],
            "source": e["source"],
            "source_lines": e["source_lines"],
            "cube_cards": len(e["cards"]),
            "instances": e["instances"],
            "references": e["references"],
            "example_cards": sorted(e["cards"])[:8],
            "fields": fields_out,
        })
    classes_out.sort(key=lambda x: (-x["cube_cards"], x["qualname"]))

    feat_count = {f: len(cs) for f, cs in w.features.items()}
    card_feats = {c: set(fs) for c, fs in w.card_features.items()}
    for c in printings:
        card_feats.setdefault(c, set())
    order, curve = greedy_unlock(card_feats, feat_count)

    # Class-only staging (ignore flags/keywords/keys) for comparison.
    card_cls = {c: {f for f in fs if f.startswith("class:")} for c, fs in card_feats.items()}
    cls_count = {f: n for f, n in feat_count.items() if f.startswith("class:")}
    order_cls, curve_cls = greedy_unlock(card_cls, cls_count)

    def milestones(curve):
        out = {}
        for pct in (25, 50, 60, 70, 80, 90, 95, 100):
            need = math.ceil(n_cards * pct / 100)
            for pt in curve:
                if pt["cards"] >= need:
                    out[f"{pct}%"] = {"features": pt["features"], "cards": pt["cards"]}
                    break
        return out

    blockers_by_kind = collections.Counter(b["kind"] for b in w.blockers)
    blocker_cards = collections.defaultdict(set)
    for b in w.blockers:
        blocker_cards[b["kind"]].add(b["card"])

    out = {
        "generated_by": "docs/research/scripts/walk_cube_ir.py",
        "repo_commit": sys.argv[1] if len(sys.argv) > 1 else None,
        "pool": "cube",
        "n_raw_records": len(raw),
        "n_claimed": n_cards,
        "n_basics": len(basics),
        "n_classes": len(classes_out),
        "n_classes_by_category": dict(collections.Counter(c["category"] for c in classes_out)),
        "n_features": len(feat_count),
        "n_features_by_kind": dict(collections.Counter(f.split(":", 1)[0] for f in feat_count)),
        "max_graph_depth": max(w.max_depth.values()) if w.max_depth else 0,
        "pickle_failures": pickle_fail,
        "blockers_by_kind": dict(blockers_by_kind),
        "blocker_cards_by_kind": {k: sorted(v) for k, v in blocker_cards.items()},
        "blockers": w.blockers,
        "card_name_strings": w.name_strings,
        "opaque_strings": {k: dict(v) for k, v in w.opaque_strings.items()},
        "classes": classes_out,
        "features": dict(sorted(feat_count.items(), key=lambda kv: (-kv[1], kv[0]))),
        "feature_cards": {f: sorted(cs) for f, cs in w.features.items()},
        "card_features": {c: sorted(fs) for c, fs in card_feats.items()},
        "staging_all_features": {
            "milestones": milestones(curve),
            "order": order,
            "steps": curve,
        },
        "staging_classes_only": {
            "milestones": milestones(curve_cls),
            "order": order_cls,
            "steps": curve_cls,
        },
    }
    (OUT_DIR / "card_ir_inventory.json").write_text(
        json.dumps(out, indent=1, default=str), encoding="utf-8"
    )
    print("cards", n_cards, "classes", len(classes_out), "features", len(feat_count))
    print("by category", out["n_classes_by_category"])
    print("feature kinds", out["n_features_by_kind"])
    print("blockers", dict(blockers_by_kind))
    print("pickle failures", len(pickle_fail))
    print("name strings", len(w.name_strings))
    print("milestones(all)", out["staging_all_features"]["milestones"])
    print("milestones(cls)", out["staging_classes_only"]["milestones"])


if __name__ == "__main__":
    main()
