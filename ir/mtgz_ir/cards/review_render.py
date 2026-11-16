"""Human-readable "model summary" of a built ``CardPrinting``, plus the
review page (HTML + Markdown) that puts each cube card's oracle text
next to what the parser actually built.

Everything in a model summary is derived by walking the built objects
generically -- dataclass fields, containers, enums -- and printing only
values that differ from the field default. There is no per-card or
per-effect hand-written text, so the summary can't claim a behavior the
printing doesn't carry. Effect classes render as their class name split
into words (``DealDamageToTargetCreature`` -> "Deal damage to target
creature") followed by their non-default fields, nested by indentation.

The page adds three cheap automatic "review hints" per card (oracle
lines sharing no content word with the model, oracle numbers missing
from the model, Scryfall keywords absent from the model). They point a
human at places worth reading; they are not verdicts.

``scripts/render_cube_card_review.py`` does the I/O and writes
``docs/cube_card_review.html`` + ``docs/CUBE_CARD_REVIEW.md``.
"""

from __future__ import annotations

import dataclasses
import enum
import html
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable, Mapping

from mtgz_ir.cards.base import CardPrinting, ManaCost
from mtgz_ir.cards.pool_coverage import primary_type, strip_reminder_text

# --------------------------------------------------------------- naming

_CAMEL_TOKEN_RE = re.compile(r"[A-Z]+(?![a-z])|[A-Z]?[a-z]+|\d+")


def humanize_class_name(name: str) -> str:
    """``DealDamageToTargetCreature`` -> ``Deal damage to target creature``.

    All-caps runs are kept as acronyms (``ETB``, ``UEOT``, ``MV``, ``X``);
    a leading underscore (private class) is dropped."""
    words = _CAMEL_TOKEN_RE.findall(name.lstrip("_"))
    if not words:
        return name
    out: list[str] = []
    for i, w in enumerate(words):
        if w.isupper() or w.isdigit():
            out.append(w)
        elif i == 0:
            out.append(w[0].upper() + w[1:])
        else:
            out.append(w.lower())
    return " ".join(out)


def humanize_field_name(name: str) -> str:
    return name.strip("_").replace("_", " ")


# ------------------------------------------------------------ mana costs

_WUBRG = "WUBRG"


def format_mana_cost(cost: ManaCost) -> str:
    """Render a ``ManaCost`` object back into mana symbols (from the
    object's fields, not the printed string)."""
    parts: list[str] = ["{X}"] * cost.x_pips
    if cost.generic:
        parts.append(f"{{{cost.generic}}}")
    parts.extend(f"{{{a}/{b}}}" for a, b in cost.hybrid)
    parts.extend(f"{{{c}/P}}" for c in cost.phyrexian)
    colored = dict(cost.colored)
    for c in sorted(colored, key=lambda c: (_WUBRG.find(c) % 99, c)):
        parts.extend([f"{{{c}}}"] * colored[c])
    return "".join(parts) or "{0}"


_SYMBOL_RE = re.compile(r"\{([^}]*)\}")


def _mana_symbol_multiset(s: str) -> list[str]:
    return sorted(_SYMBOL_RE.findall(s))


def modeled_colors(printing: CardPrinting) -> frozenset[str]:
    """Base colors the engine derives for this printing (mirrors
    ``game.effective_colors`` before layer-5 effects)."""
    if "devoid" in printing.keywords:
        base: frozenset[str] = frozenset()
    elif printing.face_colors is not None:
        base = frozenset(printing.face_colors)
    else:
        base = printing.mana_cost.colors
    return frozenset(base) | frozenset(printing.color_indicator)


def _color_str(colors: Iterable[str]) -> str:
    cs = sorted(colors, key=lambda c: (_WUBRG.find(c) % 99, c))
    return "".join(cs) if cs else "colorless"


# ------------------------------------------------------- generic walker

_SCALARS = (str, int, float, bool, bytes, type(None))
_INLINE_LIMIT = 110
# Fields rendered last so a reader sees trigger / cost / timing before
# what the ability does.
_LAST_FIELDS = ("effect", "effects", "spell_effect")


def _field_default(f: dataclasses.Field) -> Any:
    if f.default is not dataclasses.MISSING:
        return f.default
    if f.default_factory is not dataclasses.MISSING:  # type: ignore[misc]
        return f.default_factory()  # type: ignore[misc]
    return dataclasses.MISSING


def _is_empty(value: Any) -> bool:
    """"Off" values: None / False / 0 / -1 (the codebase's "disabled"
    sentinel) / empty string / empty container."""
    if value is None or value is False:
        return True
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return value in (0, -1)
    if isinstance(value, (str, bytes, tuple, list, set, frozenset, dict)):
        return len(value) == 0
    return False


def _hidden(value: Any, default: Any, *, strict: bool) -> bool:
    """Whether a field value is left out of the summary.

    ``strict`` (``CardPrinting``): hide anything equal to its default.
    Otherwise (effects, predicates, costs, filters): hide only a
    default that is also an "off" value -- a meaningful default such as
    ``Controlled.by = YOU`` or ``CardsInGraveyardAtLeast.n = 7`` is part
    of what the object does and is shown."""
    if default is dataclasses.MISSING:
        return False
    try:
        equal = bool(value == default)
    except Exception:  # pragma: no cover - exotic __eq__
        return False
    if not equal:
        return False
    return strict or _is_empty(value)


def non_default_fields(obj: Any) -> list[tuple[str, Any]]:
    """``(name, value)`` for every field of ``obj`` worth showing (see
    ``_hidden``). Non-dataclass objects expose every public
    ``__dict__`` / ``__slots__`` attribute (no defaults are knowable
    there, so nothing is hidden). An ``X_str`` display string is dropped
    when its ``X`` sibling is a ``ManaCost`` with the same symbols."""
    out: list[tuple[str, Any]] = []
    if dataclasses.is_dataclass(obj) and not isinstance(obj, type):
        strict = isinstance(obj, CardPrinting)
        for f in dataclasses.fields(obj):
            try:
                value = getattr(obj, f.name)
            except Exception:  # pragma: no cover - broken property
                continue
            if not _hidden(value, _field_default(f), strict=strict):
                out.append((f.name, value))
    else:
        attrs: dict[str, Any] = dict(getattr(obj, "__dict__", {}))
        for slot in getattr(type(obj), "__slots__", ()) or ():
            if isinstance(slot, str) and hasattr(obj, slot):
                attrs.setdefault(slot, getattr(obj, slot))
        out = [(k, v) for k, v in attrs.items() if not k.startswith("_")]
    values = dict(out)
    out = [
        (k, v) for k, v in out
        if not (
            k.endswith("_str")
            and isinstance(values.get(k[:-4]), ManaCost)
            and isinstance(v, str)
            and _mana_symbol_multiset(v)
            == _mana_symbol_multiset(format_mana_cost(values[k[:-4]]))
        )
    ]
    last = [kv for kv in out if kv[0] in _LAST_FIELDS]
    first = [kv for kv in out if kv[0] not in _LAST_FIELDS]
    return first + last


def _is_scalar(v: Any) -> bool:
    return isinstance(v, _SCALARS) or isinstance(v, (enum.Enum, ManaCost))


def _is_scalar_container(v: Any) -> bool:
    if isinstance(v, (tuple, list, set, frozenset)):
        return all(_is_scalar(x) or _is_scalar_container(x) for x in v)
    return False


def format_scalar(v: Any) -> str:
    if isinstance(v, enum.StrEnum):
        # Closed string vocabularies (``effects.vocab``, ``ScopeKind``):
        # show the value the card data spells, as for a plain string.
        return v.value
    if isinstance(v, enum.Enum):
        return str(v.name).lower().replace("_", " ")
    if isinstance(v, bool):
        return "yes" if v else "no"
    if v is None:
        return "none"
    if isinstance(v, ManaCost):
        return format_mana_cost(v)
    if isinstance(v, str):
        return '""' if v == "" else v.replace("\n", " / ")
    if isinstance(v, bytes):
        return repr(v)
    return str(v)


def format_scalar_container(v: Any) -> str:
    if isinstance(v, (set, frozenset)):
        items = sorted(v, key=lambda x: str(x))
    else:
        items = list(v)
    if not items:
        return "(none)"
    parts = []
    for x in items:
        if isinstance(x, (tuple, list, set, frozenset)):
            parts.append(f"({format_scalar_container(x)})" if x else "(none)")
        else:
            parts.append(format_scalar(x))
    return ", ".join(parts)


def _inline_value(v: Any) -> str | None:
    """One-line form of ``v`` when it has one, else ``None``."""
    if _is_scalar(v):
        return format_scalar(v)
    if _is_scalar_container(v):
        return format_scalar_container(v)
    return None


def _format_field_inline(name: str, value: Any) -> str:
    if value is True:
        return humanize_field_name(name)
    return f"{humanize_field_name(name)}: {_inline_value(value)}"


class _Walker:
    """Renders an object graph into indented text lines, collecting
    every nested ``CardPrinting`` it passes (tokens) on the way."""

    def __init__(self) -> None:
        self.tokens: list[CardPrinting] = []

    # A header line for an object, and its body lines (already indented
    # by ``indent + 1`` levels).
    def object_lines(self, obj: Any, indent: int, path: frozenset[int]) -> tuple[str, list[str]]:
        if isinstance(obj, CardPrinting):
            self.tokens.append(obj)
            sub = summarize_printing(obj, _walker=self)
            head = f"Printing: {obj.name}"
            body = [("  " * (indent + 1)) + ln for ln in sub.text_lines()]
            return head, body
        name = humanize_class_name(type(obj).__name__)
        if id(obj) in path:
            return f"{name} (cycle)", []
        path = path | {id(obj)}
        items = non_default_fields(obj)
        if all(_inline_value(v) is not None for _, v in items):
            inline = "; ".join(_format_field_inline(k, v) for k, v in items)
            candidate = f"{name} ({inline})" if inline else name
            if len(candidate) <= _INLINE_LIMIT:
                return candidate, []
        body: list[str] = []
        for k, v in items:
            body.extend(self.field_lines(k, v, indent + 1, path))
        return name, body

    def field_lines(self, name: str, value: Any, indent: int, path: frozenset[int]) -> list[str]:
        pad = "  " * indent
        label = humanize_field_name(name)
        inline = _inline_value(value)
        if inline is not None:
            if value is True:
                return [f"{pad}{label}"]
            return [f"{pad}{label}: {inline}"]
        if isinstance(value, Mapping):
            lines = [f"{pad}{label}:"]
            for k, v in value.items():
                lines.extend(self.field_lines(str(k), v, indent + 1, path))
            return lines
        if isinstance(value, (tuple, list, set, frozenset)):
            lines = [f"{pad}{label}:"]
            lines.extend(self.list_lines(value, indent + 1, path))
            return lines
        head, body = self.object_lines(value, indent, path)
        return [f"{pad}{label}: {head}"] + body

    def list_lines(self, items: Iterable[Any], indent: int, path: frozenset[int]) -> list[str]:
        pad = "  " * indent
        lines: list[str] = []
        for item in items:
            inline = _inline_value(item)
            if inline is not None:
                lines.append(f"{pad}- {inline}")
            elif isinstance(item, (tuple, list, set, frozenset, Mapping)):
                lines.append(f"{pad}- group:")
                if isinstance(item, Mapping):
                    for k, v in item.items():
                        lines.extend(self.field_lines(str(k), v, indent + 1, path))
                else:
                    lines.extend(self.list_lines(item, indent + 1, path))
            else:
                head, body = self.object_lines(item, indent, path)
                lines.append(f"{pad}- {head}")
                lines.extend(body)
        return lines

    def item_block(self, obj: Any, prefix: str = "") -> list[str]:
        """Top-level list item for one ability (indent 0)."""
        head, body = self.object_lines(obj, 0, frozenset())
        return [f"- {prefix}{head}"] + body


def render_object(obj: Any) -> list[str]:
    """Indented text lines for any object (effect, condition, filter...)."""
    walker = _Walker()
    head, body = walker.object_lines(obj, 0, frozenset())
    return [head] + body


# ---------------------------------------------------- printing summary

_CHARACTERISTIC_FIELDS = frozenset({
    "name", "mana_cost_str", "mana_cost", "type_line", "supertypes",
    "card_types", "subtypes", "power", "toughness", "loyalty",
    "color_indicator", "face_colors", "produces", "dynamic_pt_formula",
})
_FACE_FIELDS = (
    "mdfc_back_face", "transform_back_face", "adventure_face",
    "prepare_spell", "split_faces", "class_levels", "room_halves",
)
_HAND_GY_FIELDS = (
    "hand_activated_abilities", "hand_ability_grants",
    "graveyard_activated_abilities",
)
_KEYWORD_EXTRA_FIELDS = frozenset({
    "stationed_keywords", "station_threshold", "protection_from_color",
    "sunburst", "daybound", "nightbound", "multikicker", "landcycling",
})
_COST_EXTRA_FIELDS = frozenset({
    "free_cast_condition", "alt_cost_exile_from_hand",
    "evoke_exile_from_hand", "splice_onto", "x_min", "impending_count",
    "escape_exile_count", "emerge_sacrifice_type", "alternative_costs",
    "additional_cost_options", "spell_cost_increases",
    "cost_reductions", "external_cost_reductions",
})
_SECTION_ORDER = (
    "Characteristics", "Keywords", "Spell effect", "Triggered abilities",
    "Activated abilities", "Loyalty abilities",
    "Hand / graveyard abilities", "Static abilities",
    "Costs, alternative costs and cost modifiers",
    "Layout / chapters / levels", "Other modeled rules fields",
    "Tokens created",
)


def _field_section(name: str) -> str:
    if name == "keywords" or name in _KEYWORD_EXTRA_FIELDS:
        return "Keywords"
    if name.startswith("has_") or name.startswith("ward_"):
        return "Keywords"
    if name == "spell_effect":
        return "Spell effect"
    if name in ("triggered_abilities", "equipment_granted_triggers"):
        return "Triggered abilities"
    if name == "activated_abilities":
        return "Activated abilities"
    if name in _HAND_GY_FIELDS:
        return "Hand / graveyard abilities"
    if name == "static_abilities":
        return "Static abilities"
    if name in _COST_EXTRA_FIELDS or "cost" in name:
        return "Costs, alternative costs and cost modifiers"
    if name in ("chapter_count", "class_level", "enters_prepared"):
        return "Layout / chapters / levels"
    return "Other modeled rules fields"


def loyalty_label(ability: Any) -> str:
    """Printed loyalty symbol for a planeswalker ability, from the
    ``ActivationCost.loyalty`` sign convention (positive = pay)."""
    n = getattr(getattr(ability, "cost", None), "loyalty", 0) or 0
    if n > 0:
        return f"[-{n}] "
    if n < 0:
        return f"[+{-n}] "
    return "[0] "


@dataclass(frozen=True)
class Section:
    title: str
    lines: tuple[str, ...]


@dataclass(frozen=True)
class ModelSummary:
    """Structured summary of one built printing."""

    name: str
    sections: tuple[Section, ...]
    faces: tuple[tuple[str, "ModelSummary"], ...] = ()
    tokens: tuple[str, ...] = ()

    def section(self, title: str) -> Section | None:
        for s in self.sections:
            if s.title == title:
                return s
        return None

    @property
    def has_rules_content(self) -> bool:
        """True when anything beyond bare characteristics was modeled
        (a keyword, an ability, a face, a rules field)."""
        return any(s.title != "Characteristics" for s in self.sections) or bool(self.faces)

    def text_lines(self) -> list[str]:
        out: list[str] = []
        for s in self.sections:
            out.append(f"{s.title}:")
            out.extend("  " + ln for ln in s.lines)
        for label, face in self.faces:
            out.append(f"{label}: {face.name}")
            out.extend("  " + ln for ln in face.text_lines())
        return out

    def to_text(self) -> str:
        return "\n".join(self.text_lines())


def _characteristics(p: CardPrinting) -> list[str]:
    lines = [f"name: {p.name}"]
    cost = format_mana_cost(p.mana_cost)
    line = f"mana cost: {cost} (MV {p.mana_cost.total})"
    printed = p.mana_cost_str
    if printed and _mana_symbol_multiset(printed.replace(" // ", "")) != _mana_symbol_multiset(cost):
        line += f"; printed string: {printed}"
    lines.append(line)
    lines.append(f"type line: {p.type_line}")
    lines.append(
        "types: "
        + " / ".join(
            part for part in (
                " ".join(p.supertypes), " ".join(p.card_types),
                " ".join(p.subtypes),
            ) if part
        )
    )
    if p.power is not None or p.toughness is not None:
        lines.append(f"power/toughness: {p.power}/{p.toughness}")
    if p.dynamic_pt_formula is not None:
        inline = _inline_value(p.dynamic_pt_formula)
        if inline is not None:
            lines.append(f"dynamic P/T: {inline}")
        else:
            lines.append("dynamic P/T:")
            lines.extend("  " + ln for ln in render_object(p.dynamic_pt_formula))
    if p.loyalty is not None:
        lines.append(f"starting loyalty: {p.loyalty}")
    lines.append(f"colors: {_color_str(modeled_colors(p))}")
    if p.color_indicator:
        lines.append(f"color indicator: {_color_str(p.color_indicator)}")
    if p.produces:
        lines.append(f"produces: {_color_str(p.produces)}")
    return lines


def summarize_printing(printing: CardPrinting, *, _walker: _Walker | None = None) -> ModelSummary:
    """Walk ``printing`` into a :class:`ModelSummary`.

    Every non-default ``CardPrinting`` field lands in exactly one
    section (unknown fields in "Other modeled rules fields"), so a new
    field family can never be silently hidden."""
    walker = _walker if _walker is not None else _Walker()
    token_start = len(walker.tokens)
    buckets: dict[str, list[str]] = {"Characteristics": _characteristics(printing)}
    faces: list[tuple[str, ModelSummary]] = []
    is_walker = printing.is_planeswalker
    fields_by_name = dict(non_default_fields(printing))
    if printing.is_aura:
        # "Enchant creature" is the field default, so the strict rule
        # would hide it exactly on the cards it describes.
        fields_by_name.setdefault("aura_enchant_type", printing.aura_enchant_type)

    for fname, value in fields_by_name.items():
        if fname in _CHARACTERISTIC_FIELDS:
            continue
        if fname in _FACE_FIELDS:
            label = humanize_field_name(fname)
            items = value if isinstance(value, tuple) else (value,)
            for i, item in enumerate(items, 1):
                tag = f"{label} {i}" if isinstance(value, tuple) else label
                if isinstance(item, CardPrinting):
                    faces.append((tag, summarize_printing(item, _walker=walker)))
                else:
                    head, body = walker.object_lines(item, 0, frozenset())
                    faces.append((tag, ModelSummary(
                        name=getattr(item, "name", head),
                        sections=(Section("Model", tuple([head] + body)),),
                    )))
            continue
        section = _field_section(fname)
        if section == "Keywords" and fname == "keywords":
            buckets.setdefault(section, []).append(
                "keywords: " + format_scalar_container(value)
            )
            continue
        if section == "Activated abilities" and is_walker:
            lines = buckets.setdefault("Loyalty abilities", [])
            for ab in value:
                lines.extend(walker.item_block(ab, loyalty_label(ab)))
            continue
        if fname in ("triggered_abilities", "activated_abilities",
                     "static_abilities", "hand_activated_abilities",
                     "graveyard_activated_abilities"):
            lines = buckets.setdefault(section, [])
            if fname in _HAND_GY_FIELDS:
                lines.append(f"{humanize_field_name(fname)}:")
            for ab in value:
                lines.extend(walker.item_block(ab))
            continue
        if fname == "spell_effect":
            head, body = walker.object_lines(value, 0, frozenset())
            buckets.setdefault(section, []).extend([head] + body)
            continue
        buckets.setdefault(section, []).extend(
            walker.field_lines(fname, value, 0, frozenset())
        )

    token_names: list[str] = []
    for tok in walker.tokens[token_start:]:
        pt = f" {tok.power}/{tok.toughness}" if tok.power is not None else ""
        kws = f" [{', '.join(sorted(tok.keywords))}]" if tok.keywords else ""
        desc = f"{tok.name} - {tok.type_line}{pt}{kws}"
        if desc not in token_names:
            token_names.append(desc)
    if token_names and _walker is None:
        buckets["Tokens created"] = [f"- {t}" for t in token_names]

    sections = tuple(
        Section(title, tuple(buckets[title]))
        for title in _SECTION_ORDER
        if buckets.get(title)
    )
    return ModelSummary(
        name=printing.name,
        sections=sections,
        faces=tuple(faces),
        tokens=tuple(token_names),
    )


# -------------------------------------------------------- oracle faces


def oracle_faces(card: Mapping[str, Any]) -> list[dict[str, str]]:
    """Per-face printed characteristics + verbatim oracle text."""
    faces = card.get("card_faces") or [card]
    out = []
    for face in faces:
        pt = ""
        if face.get("power") is not None:
            pt = f"{face.get('power')}/{face.get('toughness')}"
        elif face.get("loyalty") is not None:
            pt = f"loyalty {face.get('loyalty')}"
        out.append({
            "name": face.get("name", card.get("name", "")),
            "mana_cost": face.get("mana_cost", ""),
            "type_line": face.get("type_line", ""),
            "oracle_text": face.get("oracle_text", ""),
            "pt": pt,
        })
    return out


# ------------------------------------------------------- review flags

_WORD_RE = re.compile(r"[a-z]+")
_NUMBER_RE = re.compile(r"\d+")
_NUMBER_WORDS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
    "seven": 7, "eight": 8, "nine": 9, "ten": 10, "eleven": 11,
    "twelve": 12, "thirteen": 13, "fifteen": 15, "twenty": 20,
}
# "one" / "a" are too generic in oracle prose ("any one color", "one or
# more") to demand a matching 1 in the model; the model side still
# counts them.
_ORACLE_SKIP_NUMBER_WORDS = frozenset({"one"})
_STOPWORDS = frozenset("""
a an and any are as at be been but by can cant could do does dont each
else for from get gets had has have her here him his how if in into is
isnt it its may more most much must no nor not of off on onto or other
out over same she so some such than that the their them then there
these they this those through to too under until up upon was were what
when whenever where which while who whose will with would you your
yours also only another instead except because during among way ll
""".split())


def _stem(word: str) -> str:
    if len(word) > 4 and word.endswith("s"):
        word = word[:-1]
    return word[:5]


def content_stems(text: str) -> set[str]:
    words = _WORD_RE.findall(text.lower().replace("'s", "").replace("’s", ""))
    return {
        _stem(w) for w in words
        if len(w) >= 4 and w not in _STOPWORDS
    }


def numbers_in(text: str, *, skip_words: frozenset[str] = frozenset()) -> set[int]:
    """Digits plus spelled-out numbers ("three") in ``text``."""
    lowered = text.lower()
    nums = {int(n) for n in _NUMBER_RE.findall(lowered)}
    for w in _WORD_RE.findall(lowered):
        if w in _NUMBER_WORDS and w not in skip_words:
            nums.add(_NUMBER_WORDS[w])
    return nums


def _normalize_keyword(k: str) -> str:
    return re.sub(r"[\s_\-]+", " ", k.strip().lower())


def modeled_keywords(printing: CardPrinting) -> set[str]:
    """Keyword sets of the printing and every nested face."""
    out = {_normalize_keyword(k) for k in printing.keywords}
    out |= {_normalize_keyword(k) for k in printing.stationed_keywords}
    for fname in _FACE_FIELDS:
        value = getattr(printing, fname, None)
        items = value if isinstance(value, tuple) else (value,)
        for item in items:
            if isinstance(item, CardPrinting):
                out |= modeled_keywords(item)
    return out


# Acknowledged no-in-game-effect LINE CLASSES. Each entry is a printed
# sentence shape whose only effect is outside a game this engine plays
# (two players, decks given up front): deck-construction statements,
# commander-format rules and draft-matters text. A line of one of these
# shapes is not a dropped clause, so it produces no review hint (and its
# numbers / keywords aren't demanded of the model). Keyed on the sentence
# shape -- the card's own name is matched as "<name>" -- never on a card
# name list. Add a shape here only when the line truly never matters
# inside a game; anything with an in-game half (Companion's "put it into
# your hand from outside the game", "Partner with <name>"'s ETB search)
# stays out.
NO_GAME_EFFECT_LINE_CLASSES: tuple[tuple[str, re.Pattern[str]], ...] = (
    # "This card is banned." (Gleemox; CR 100.2a / 100.6 deck legality)
    ("deck construction", re.compile(r"^This card is banned\.$")),
    # "A deck can have any number of / up to seven cards named <name>."
    ("deck construction", re.compile(
        r"^A deck can have (?:any number of|up to \w+) cards named "
        r"(?:<name>|this card)\.$"
    )),
    # "<name> can be your commander." (CR 903.3a)
    ("commander format", re.compile(r"^(?:<name>|This card) can be your commander\.$")),
    # "Choose a Background" / "Partner" / "Friends forever" (CR 702.124,
    # 702.175): only which cards may be your commanders.
    ("commander format", re.compile(r"^(?:Choose a Background|Partner|Friends forever)$")),
    # Conspiracy draft-matters: "Draft <name> face up." / "As you draft a
    # card, ..." (Cogwork Librarian; CR 905 draft actions).
    ("draft", re.compile(r"^Draft (?:<name>|this card) face up\.$")),
    ("draft", re.compile(r"^As you draft (?:a card|<name>|this card)\b.*\.$")),
)


def no_game_effect_line_class(line: str, card_name: str = "") -> str | None:
    """The acknowledged no-in-game-effect class of an oracle ``line``
    (reminder text already stripped), or ``None``. The card's own name
    is normalized to ``<name>`` before matching."""
    text = line.strip()
    if card_name:
        text = text.replace(card_name, "<name>")
    for label, pattern in NO_GAME_EFFECT_LINE_CLASSES:
        if pattern.match(text):
            return label
    return None


@dataclass(frozen=True)
class ReviewFlag:
    kind: str       # "line" | "number" | "keyword" | "unclaimed"
    detail: str

    def describe(self) -> str:
        if self.kind == "line":
            return f"Oracle line shares no content word with the model: “{self.detail}”"
        if self.kind == "number":
            return f"Number in oracle text not found in the model: {self.detail}"
        if self.kind == "unclaimed":
            return "Not claimed by the parser: there is no model to review"
        return f"Scryfall keyword not in modeled keywords or model text: {self.detail}"


def _name_words(card: Mapping[str, Any]) -> set[str]:
    names = [card.get("name", "")]
    names += [f.get("name", "") for f in card.get("card_faces") or ()]
    return content_stems(" ".join(names))


_REMINDER_PART_RE = re.compile(r"\(([^)]*)\)")
# A reminder text "explains" a keyword line when it shares at least
# this many content words with the model.
_REMINDER_MIN_OVERLAP = 2


def _reminder_stems(raw_line: str) -> set[str]:
    return content_stems(" ".join(_REMINDER_PART_RE.findall(raw_line)))


def review_flags(
    card: Mapping[str, Any],
    printing: CardPrinting,
    summary_text: str,
) -> list[ReviewFlag]:
    """Cheap hints for a human reviewer (not verdicts):

    * oracle lines (reminder text stripped) none of whose content words
      appear anywhere in the model summary -- a possibly dropped clause.
      A keyword line ("Cycling {2}", "Living weapon") whose own reminder
      text shares >= 2 content words with the model is not flagged: the
      reminder is what the keyword means;
    * numbers (digits or number words) in the oracle text missing from
      the model summary;
    * Scryfall ``keywords`` missing from the modeled keyword sets AND
      from the model text AND not explained (as above) by the reminder
      text of a line naming them. Ability / flavor words (printed as
      ``<word> —``) carry no rules meaning and are skipped.

    Lines of an acknowledged no-in-game-effect class
    (``NO_GAME_EFFECT_LINE_CLASSES``: deck-construction, commander-format
    and draft-matters sentences) are skipped entirely -- no line hint,
    and their numbers and keywords aren't demanded of the model.
    """
    flags: list[ReviewFlag] = []
    model_stems = content_stems(summary_text)
    model_numbers = {abs(n) for n in numbers_in(summary_text)}
    name_stems = _name_words(card)
    raw_lines: list[str] = []
    all_oracle: list[str] = []
    for face in oracle_faces(card):
        for raw in face["oracle_text"].splitlines():
            line = strip_reminder_text(raw)
            if not line:
                continue
            if no_game_effect_line_class(line, face["name"]) or (
                no_game_effect_line_class(line, card.get("name", ""))
            ):
                continue
            raw_lines.append(raw)
            all_oracle.append(line)
            stems = content_stems(line) - name_stems
            if not stems or stems & model_stems:
                continue
            reminder = (_reminder_stems(raw) - name_stems) & model_stems
            if len(reminder) >= _REMINDER_MIN_OVERLAP:
                continue
            flags.append(ReviewFlag("line", line))
    oracle_text = " | ".join(all_oracle)
    for n in sorted(numbers_in(oracle_text, skip_words=_ORACLE_SKIP_NUMBER_WORDS)):
        if n != 0 and n not in model_numbers:
            flags.append(ReviewFlag("number", str(n)))
    kws = modeled_keywords(printing)
    model_lower = _normalize_keyword(summary_text)
    for kw in card.get("keywords") or ():
        if f"{kw} —" in oracle_text:
            continue  # ability word / flavor word
        norm = _normalize_keyword(kw)
        if norm not in _normalize_keyword(oracle_text):
            # Scryfall umbrella tag (e.g. "Typecycling"), never printed --
            # or the keyword of an acknowledged no-in-game-effect line
            # ("Choose a Background"), which ``oracle_text`` leaves out.
            continue
        if norm in kws or norm in model_lower:
            continue
        explained = any(
            len((_reminder_stems(raw) - name_stems) & model_stems)
            >= _REMINDER_MIN_OVERLAP
            for raw in raw_lines
            if norm in _normalize_keyword(raw)
        )
        if not explained:
            flags.append(ReviewFlag("keyword", kw))
    return flags


# --------------------------------------------- shortcut notes + tests

_ITEM_START_RE = re.compile(r"^\s*(?:[-*+]\s+|\d+[.)]\s+|\*\*\d+[.)])")


@dataclass(frozen=True)
class DocBullet:
    heading: str
    text: str


def parse_doc_bullets(markdown: str) -> list[DocBullet]:
    """Split a Markdown doc into paragraph / list-item units, each
    tagged with the ``##`` heading it sits under. Headings themselves
    are not units."""
    bullets: list[DocBullet] = []
    heading = ""
    current: list[str] = []

    def flush() -> None:
        if current:
            text = "\n".join(current).strip()
            if text:
                bullets.append(DocBullet(heading, text))
            current.clear()

    for line in markdown.splitlines():
        if line.startswith("#"):
            flush()
            heading = line.lstrip("#").strip()
            continue
        if not line.strip():
            flush()
            continue
        if _ITEM_START_RE.match(line):
            flush()
        current.append(line)
    flush()
    return bullets


def card_aliases(card: Mapping[str, Any]) -> list[str]:
    """Names a doc / test may use for the card: the full name and the
    front-face name of a multi-face card, plus comma-free variants
    ("Jace the Mind Sculptor"). A single-word front face ("Life" of
    Life // Death) is skipped: it is an ordinary English word."""
    names = [card["name"]]
    faces = card.get("card_faces") or ()
    if faces and " " in faces[0].get("name", ""):
        names.append(faces[0]["name"])
    out: list[str] = []
    for n in names:
        for variant in (n, n.replace(",", "")):
            if variant and variant not in out:
                out.append(variant)
    return out


def _alias_regex(aliases: Iterable[str]) -> re.Pattern[str]:
    """Plain alternation of names, longest first (word boundaries are
    checked by ``_name_hits``)."""
    ordered = sorted(set(aliases), key=len, reverse=True)
    return re.compile("|".join(re.escape(a) for a in ordered))


def _is_multiword(name: str) -> bool:
    return " " in name or "-" in name


def _is_word_char(ch: str) -> bool:
    return ch.isalnum() or ch == "_"


def _name_hits(pat: re.Pattern[str], text: str) -> Iterable[str]:
    """Matched names that stand alone (not inside a longer word). The
    boundary check runs here rather than as regex lookarounds, which
    defeat the engine's literal-prefix scan (~6x slower on tests/)."""
    for m in pat.finditer(text):
        start, end = m.span()
        if start > 0 and _is_word_char(text[start - 1]):
            continue
        if end < len(text) and _is_word_char(text[end]):
            continue
        yield m.group(0)


def _alias_owners(cards: Iterable[Mapping[str, Any]], *, fold_case: bool) -> dict[str, set[str]]:
    owners: dict[str, set[str]] = {}
    for card in cards:
        for a in card_aliases(card):
            key = a.lower() if fold_case else a
            owners.setdefault(key, set()).add(card["name"])
    return owners


def match_doc_bullets(
    cards: Iterable[Mapping[str, Any]],
    bullets: list[DocBullet],
) -> dict[str, list[int]]:
    """card name -> indices of bullets mentioning it.

    Multi-word names match case-insensitively; single-word names
    ("Flash", "Balance", "Island") must match case exactly, or every
    prose use of the word would hit."""
    cards = list(cards)
    owners = _alias_owners(cards, fold_case=True)
    out: dict[str, list[int]] = {}
    if not owners:
        return out
    aliases = [a for c in cards for a in card_aliases(c)]
    multi = _alias_regex(a.lower() for a in aliases if _is_multiword(a))
    single_names = [a for a in aliases if not _is_multiword(a)]
    single = _alias_regex(single_names) if single_names else None
    for i, b in enumerate(bullets):
        hits = list(_name_hits(multi, b.text.lower()))
        if single is not None:
            hits += _name_hits(single, b.text)
        seen: set[str] = set()
        for hit in hits:
            for owner in sorted(owners.get(hit.lower(), ())):
                if owner not in seen:
                    seen.add(owner)
                    out.setdefault(owner, []).append(i)
    return out


def match_test_files(
    cards: Iterable[Mapping[str, Any]],
    files: Mapping[str, str],
) -> dict[str, dict[str, int]]:
    """card name -> {test file path: mention count} (exact-case,
    word-bounded match of the full or front-face name)."""
    cards = list(cards)
    owners = _alias_owners(cards, fold_case=False)
    out: dict[str, dict[str, int]] = {}
    if not owners:
        return out
    pat = _alias_regex(owners)
    for path, text in files.items():
        for hit in _name_hits(pat, text):
            for owner in owners.get(hit, ()):
                per = out.setdefault(owner, {})
                per[path] = per.get(path, 0) + 1
    return out


# ------------------------------------------------------------ the page


@dataclass
class CardReview:
    """Everything the review page shows for one card."""

    name: str
    card: Mapping[str, Any]
    summary: ModelSummary | None          # None = not claimed by the parser
    summary_text: str
    flags: list[ReviewFlag]
    shortcuts: list[DocBullet] = field(default_factory=list)
    tests: list[tuple[str, int]] = field(default_factory=list)

    @property
    def primary_type(self) -> str:
        return primary_type(self.card)

    @property
    def color_identity(self) -> str:
        return "".join(
            c for c in _WUBRG if c in (self.card.get("color_identity") or ())
        )

    @property
    def anchor(self) -> str:
        slug = re.sub(r"[^a-z0-9]+", "-", self.name.lower()).strip("-")
        return f"card-{slug}"


def build_reviews(
    cards: Iterable[Mapping[str, Any]],
    printings: Mapping[str, CardPrinting],
    *,
    bullets: list[DocBullet] | None = None,
    test_files: Mapping[str, str] | None = None,
) -> list[CardReview]:
    """One :class:`CardReview` per raw card, sorted by name."""
    cards = list(cards)
    bullets = bullets or []
    bullet_hits = match_doc_bullets(cards, bullets) if bullets else {}
    test_hits = match_test_files(cards, test_files) if test_files else {}
    reviews: list[CardReview] = []
    for card in cards:
        name = card["name"]
        printing = printings.get(name)
        if printing is None:
            summary = None
            text = ""
            flags = [ReviewFlag("unclaimed", name)]
        else:
            summary = summarize_printing(printing)
            text = summary.to_text()
            flags = review_flags(card, printing, text)
        tests = sorted(
            test_hits.get(name, {}).items(), key=lambda kv: (-kv[1], kv[0]),
        )
        reviews.append(CardReview(
            name=name,
            card=card,
            summary=summary,
            summary_text=text,
            flags=flags,
            shortcuts=[bullets[i] for i in bullet_hits.get(name, [])],
            tests=tests,
        ))
    reviews.sort(key=lambda r: r.name.lower())
    return reviews


def most_flagged(reviews: Iterable[CardReview], n: int = 15) -> list[CardReview]:
    flagged = [r for r in reviews if r.flags]
    flagged.sort(key=lambda r: (-len(r.flags), r.name.lower()))
    return flagged[:n]


def _e(text: Any) -> str:
    return html.escape(str(text), quote=True)


_CSS = """
:root { --fg:#1d1d1f; --muted:#666; --line:#ddd; --bg:#fafafa; --card:#fff;
  --warn:#fff4d6; --warnline:#e0b000; --sc:#eef4ff; --scline:#6b8fd6; }
* { box-sizing: border-box; }
body { font: 14px/1.45 system-ui, -apple-system, Segoe UI, sans-serif;
  color: var(--fg); background: var(--bg); margin: 0; }
header { padding: 12px 16px; background: #222; color: #eee; }
header h1 { font-size: 18px; margin: 0 0 4px; }
header p { margin: 2px 0; color: #ccc; font-size: 13px; }
header a { color: #9cf; }
#controls { position: sticky; top: 0; z-index: 5; background: #fff;
  border-bottom: 1px solid var(--line); padding: 8px 16px;
  display: flex; flex-wrap: wrap; gap: 8px 14px; align-items: center; }
#controls input[type=search] { flex: 1 1 260px; padding: 6px 8px;
  font-size: 15px; border: 1px solid #bbb; border-radius: 6px; }
#controls select, #controls button { padding: 5px 8px; font-size: 14px; }
#count { color: var(--muted); }
main { padding: 8px 16px 40px; }
details.card { background: var(--card); border: 1px solid var(--line);
  border-radius: 8px; margin: 6px 0; }
details.card > summary { cursor: pointer; padding: 8px 10px;
  display: flex; flex-wrap: wrap; gap: 4px 10px; align-items: baseline; }
.name { font-weight: 600; }
.cost, .tl { color: var(--muted); }
.badge { font-size: 12px; border-radius: 10px; padding: 1px 8px;
  border: 1px solid var(--line); }
.badge.flag { background: var(--warn); border-color: var(--warnline); font-weight: 600; }
.badge.sc { background: var(--sc); border-color: var(--scline); }
.body { padding: 0 10px 10px; }
.hints { background: var(--warn); border-left: 4px solid var(--warnline);
  padding: 6px 10px; margin: 4px 0 10px; }
.hints b { display: block; }
.hints ul { margin: 4px 0; padding-left: 20px; }
.cols { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
@media (max-width: 900px) { .cols { grid-template-columns: 1fr; } }
.cols h3, .extra h3 { font-size: 13px; margin: 6px 0 4px; color: var(--muted);
  text-transform: uppercase; letter-spacing: .04em; }
.face { border-left: 3px solid var(--line); padding: 2px 8px; margin: 6px 0; }
.face .fh { font-weight: 600; }
.face .ft { color: var(--muted); font-size: 13px; }
.otext { white-space: pre-wrap; margin: 4px 0; }
pre.model { white-space: pre-wrap; background: #f6f6f6; border: 1px solid #eee;
  border-radius: 6px; padding: 8px; margin: 0; font-size: 12.5px;
  font-family: ui-monospace, Consolas, Menlo, monospace; overflow-x: auto; }
.sc-item { background: var(--sc); border-left: 3px solid var(--scline);
  padding: 4px 8px; margin: 4px 0; white-space: pre-wrap; font-size: 13px; }
.sc-item .sh { font-weight: 600; font-size: 12px; color: #345; display: block; }
ul.tests { margin: 4px 0; padding-left: 20px; columns: 2 320px; font-size: 13px; }
.none { color: var(--muted); font-style: italic; }
.hidden { display: none !important; }
ol.top li { margin: 1px 0; }
"""

_JS = r"""
(function () {
  var cards = Array.prototype.slice.call(document.querySelectorAll('details.card'));
  var index = cards.map(function (c) {
    var parts = [];
    ['.name', '.oracle', '.model'].forEach(function (sel) {
      c.querySelectorAll(sel).forEach(function (el) { parts.push(el.textContent); });
    });
    return parts.join('\n').toLowerCase();
  });
  var q = document.getElementById('q'), ty = document.getElementById('type'),
      co = document.getElementById('color'), fs = document.getElementById('f-shortcut'),
      ff = document.getElementById('f-flags'), count = document.getElementById('count');
  function apply() {
    var needle = q.value.trim().toLowerCase(), shown = 0;
    cards.forEach(function (c, i) {
      var ok = true;
      if (needle && index[i].indexOf(needle) < 0) ok = false;
      if (ok && ty.value && c.dataset.type !== ty.value) ok = false;
      if (ok && co.value) {
        var ci = c.dataset.colors;
        if (co.value === 'C') ok = ci === '';
        else if (co.value === 'M') ok = ci.length > 1;
        else ok = ci.indexOf(co.value) >= 0;
      }
      if (ok && fs.checked && c.dataset.shortcuts === '0') ok = false;
      if (ok && ff.checked && c.dataset.flags === '0') ok = false;
      c.classList.toggle('hidden', !ok);
      if (ok) shown++;
    });
    count.textContent = shown + ' / ' + cards.length + ' cards';
  }
  [q, ty, co, fs, ff].forEach(function (el) {
    el.addEventListener('input', apply); el.addEventListener('change', apply);
  });
  document.getElementById('expand').addEventListener('click', function () {
    cards.forEach(function (c) { if (!c.classList.contains('hidden')) c.open = true; });
  });
  document.getElementById('collapse').addEventListener('click', function () {
    cards.forEach(function (c) { c.open = false; });
  });
  function openHash() {
    if (!location.hash) return;
    var t = document.getElementById(decodeURIComponent(location.hash.slice(1)));
    if (t && t.tagName === 'DETAILS') { t.classList.remove('hidden'); t.open = true; }
  }
  window.addEventListener('hashchange', openHash);
  openHash();
  apply();
})();
"""

_HINT_NOTE = (
    "Automatic review hints are cheap text heuristics, not verdicts: an "
    "oracle line sharing no content word with the model summary, a number "
    "in the oracle text absent from the model, or a Scryfall keyword the "
    "model never mentions. Use them to decide where to read first."
)
_MAX_TEST_FILES_SHOWN = 25


def _face_html(face: Mapping[str, str]) -> str:
    meta = " · ".join(
        x for x in (face["mana_cost"], face["type_line"], face["pt"]) if x
    )
    text = face["oracle_text"] or "(no rules text)"
    return (
        f'<div class="face"><div class="fh">{_e(face["name"])}</div>'
        f'<div class="ft">{_e(meta)}</div>'
        f'<div class="otext">{_e(text)}</div></div>'
    )


def _tests_html(tests: list[tuple[str, int]]) -> str:
    if not tests:
        return '<p class="none">No test file under tests/ names this card.</p>'
    total = sum(n for _, n in tests)

    def items(rows: list[tuple[str, int]]) -> str:
        return "".join(f"<li>{_e(path)} ({n})</li>" for path, n in rows)

    more = ""
    if len(tests) > _MAX_TEST_FILES_SHOWN:
        rest = tests[_MAX_TEST_FILES_SHOWN:]
        more = (
            f"<details><summary>{len(rest)} more files</summary>"
            f'<ul class="tests">{items(rest)}</ul></details>'
        )
    return (
        f"<p>{_plural(len(tests), 'file')}, {_plural(total, 'mention')}.</p>"
        f'<ul class="tests">{items(tests[:_MAX_TEST_FILES_SHOWN])}</ul>{more}'
    )


def _plural(n: int, word: str) -> str:
    return f"{n} {word}{'' if n == 1 else 's'}"


def _card_html(r: CardReview) -> str:
    faces = oracle_faces(r.card)
    cost = r.card.get("mana_cost") or faces[0]["mana_cost"]
    badges = []
    if r.flags:
        badges.append(f'<span class="badge flag">{_plural(len(r.flags), "hint")}</span>')
    if r.shortcuts:
        badges.append(
            f'<span class="badge sc">{_plural(len(r.shortcuts), "shortcut note")}</span>'
        )
    badges.append(f'<span class="badge">{_plural(len(r.tests), "test file")}</span>')
    hints = ""
    if r.flags:
        items = "".join(f"<li>{_e(f.describe())}</li>" for f in r.flags)
        hints = (
            '<div class="hints"><b>Automatic review hints (heuristic, not '
            f"verdicts)</b><ul>{items}</ul></div>"
        )
    if r.summary is not None:
        model = f'<pre class="model">{_e(r.summary_text)}</pre>'
    else:
        model = '<p class="none model">Not claimed by the parser: no model.</p>'
    if r.shortcuts:
        sc = "".join(
            f'<div class="sc-item"><span class="sh">{_e(b.heading)}</span>'
            f"{_e(b.text)}</div>"
            for b in r.shortcuts
        )
    else:
        sc = '<p class="none">No SEARCH_LIMITATIONS.md paragraph names this card.</p>'
    return (
        f'<details class="card" id="{_e(r.anchor)}" data-type="{_e(r.primary_type)}" '
        f'data-colors="{_e(r.color_identity)}" data-shortcuts="{len(r.shortcuts)}" '
        f'data-flags="{len(r.flags)}">'
        f'<summary><span class="name">{_e(r.name)}</span>'
        f'<span class="cost">{_e(cost)}</span>'
        f'<span class="tl">{_e(r.card.get("type_line", ""))}</span>'
        f'{"".join(badges)}</summary><div class="body">{hints}'
        f'<div class="cols"><section class="oracle"><h3>Oracle text</h3>'
        f'{"".join(_face_html(f) for f in faces)}</section>'
        f"<section><h3>Model summary (built CardPrinting)</h3>{model}</section></div>"
        f'<div class="extra"><h3>Documented shortcuts (docs/SEARCH_LIMITATIONS.md)</h3>'
        f"{sc}<h3>Tests mentioning this card</h3>{_tests_html(r.tests)}</div>"
        f"</div></details>"
    )


def render_html(
    reviews: list[CardReview],
    *,
    title: str = "Cube card implementation review",
    generator: str = "scripts/render_cube_card_review.py",
) -> str:
    """The self-contained review page (inline CSS / JS, no network)."""
    n_flag = sum(1 for r in reviews if r.flags)
    n_sc = sum(1 for r in reviews if r.shortcuts)
    types = sorted({r.primary_type for r in reviews})
    type_opts = "".join(f'<option value="{_e(t)}">{_e(t)}</option>' for t in types)
    top = "".join(
        f'<li><a href="#{_e(r.anchor)}">{_e(r.name)}</a> ({len(r.flags)})</li>'
        for r in most_flagged(reviews)
    )
    top_html = (
        f'<details><summary>Most-flagged cards</summary><ol class="top">{top}'
        "</ol></details>" if top else ""
    )
    return (
        '<!DOCTYPE html>\n<html lang="en"><head><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width, initial-scale=1">'
        f"<title>{_e(title)}</title><style>{_CSS}</style></head><body>"
        f"<header><h1>{_e(title)}</h1>"
        f"<p>Generated by <code>{_e(generator)}</code> - do not edit by hand. "
        f"{len(reviews)} cards; {n_flag} carry automatic review hints; {n_sc} are "
        "named in docs/SEARCH_LIMITATIONS.md.</p>"
        "<p>Model summaries are generated by walking each built CardPrinting "
        "(every field that differs from its default; effect classes shown by "
        "name with their parameters), never from hand-written card text.</p>"
        f"<p>{_e(_HINT_NOTE)}</p>{top_html}</header>"
        '<div id="controls">'
        '<input id="q" type="search" placeholder="Search name, oracle text, model text" '
        'aria-label="Search">'
        f'<select id="type" aria-label="Type"><option value="">All types</option>'
        f"{type_opts}</select>"
        '<select id="color" aria-label="Color identity"><option value="">All colors</option>'
        '<option value="W">White</option><option value="U">Blue</option>'
        '<option value="B">Black</option><option value="R">Red</option>'
        '<option value="G">Green</option><option value="M">Multicolor</option>'
        '<option value="C">Colorless</option></select>'
        '<label><input id="f-shortcut" type="checkbox"> has documented shortcut</label>'
        '<label><input id="f-flags" type="checkbox"> has review hint</label>'
        '<button id="expand" type="button">Expand shown</button>'
        '<button id="collapse" type="button">Collapse all</button>'
        '<span id="count"></span></div><main>\n'
        + "\n".join(_card_html(r) for r in reviews)
        + f"\n</main><script>{_JS}</script></body></html>\n"
    )


_MD_EXCERPT = 240
_MD_TESTS_SHOWN = 10


def _md_excerpt(text: str) -> str:
    flat = " ".join(text.split())
    return flat if len(flat) <= _MD_EXCERPT else flat[:_MD_EXCERPT - 3] + "..."


def render_review_markdown(
    reviews: list[CardReview],
    *,
    generator: str = "scripts/render_cube_card_review.py",
) -> str:
    """Plain-Markdown twin of the HTML page (diffable in git)."""
    lines = [
        f"Generated by `{generator}` - DO NOT EDIT MANUALLY. "
        "Run the script to refresh. Interactive version: "
        "`docs/cube_card_review.html`.",
        "",
        "# Cube card implementation review",
        "",
        f"{len(reviews)} cards; {sum(1 for r in reviews if r.flags)} carry "
        "automatic review hints; "
        f"{sum(1 for r in reviews if r.shortcuts)} are named in "
        "`docs/SEARCH_LIMITATIONS.md`.",
        "",
        _HINT_NOTE,
        "",
        "## Most-flagged cards",
        "",
    ]
    top = most_flagged(reviews)
    for r in top:
        lines.append(
            f"- **{r.name}** ({len(r.flags)}): "
            + "; ".join(f.describe() for f in r.flags)
        )
    if not top:
        lines.append("(none)")
    lines.append("")
    for r in reviews:
        lines += [f"## {r.name}", ""]
        head = " - ".join(
            x for x in (r.card.get("mana_cost", ""), r.card.get("type_line", "")) if x
        )
        if head:
            lines += [head, ""]
        if r.flags:
            lines += ["**Review hints (heuristic):**", ""]
            lines += [f"- {f.describe()}" for f in r.flags]
            lines.append("")
        lines += ["Oracle text:", "", "```text"]
        multi = bool(r.card.get("card_faces"))
        for face in oracle_faces(r.card):
            if multi:
                lines.append(
                    f"[{face['name']}] {face['mana_cost']} {face['type_line']}".rstrip()
                )
            lines.append(face["oracle_text"] or "(no rules text)")
        lines += ["```", "", "Model summary:", "", "```text"]
        lines.append(r.summary_text or "(not claimed by the parser)")
        lines += ["```", ""]
        if r.shortcuts:
            lines += ["Documented shortcuts:", ""]
            lines += [f"- *{b.heading}*: {_md_excerpt(b.text)}" for b in r.shortcuts]
            lines.append("")
        total = sum(n for _, n in r.tests)
        shown = ", ".join(f"`{p}` ({n})" for p, n in r.tests[:_MD_TESTS_SHOWN])
        if len(r.tests) > _MD_TESTS_SHOWN:
            shown += f", and {len(r.tests) - _MD_TESTS_SHOWN} more"
        tail = f" - {shown}" if r.tests else ""
        lines += [f"Tests: {_plural(len(r.tests), 'file')}, {_plural(total, 'mention')}{tail}", ""]
    return "\n".join(lines)


def load_test_files(tests_dir: Path, repo_root: Path) -> dict[str, str]:
    """``{repo-relative posix path: text}`` for every ``*.py`` under
    ``tests_dir``."""
    out: dict[str, str] = {}
    for path in sorted(tests_dir.rglob("*.py")):
        rel = path.relative_to(repo_root).as_posix()
        out[rel] = path.read_text(encoding="utf-8", errors="replace")
    return out
