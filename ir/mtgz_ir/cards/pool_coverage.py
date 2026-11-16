"""Parser coverage of a card pool: which cards the oracle-text parser
claims, bucketed so the modeling campaign can be prioritized.

Pure functions over raw Scryfall records + the set of claimed names;
``scripts/measure_cube_coverage.py`` renders the result to
``docs/CUBE_POOL_COVERAGE.md``.

"Claimed" is binary and honest: the parser returns ``None`` for any
card with a single unrecognized oracle line, so a claimed card is
modeled end-to-end at full oracle text (see the parser CLAUDE.md).
"""

from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Iterable, Mapping

if TYPE_CHECKING:  # pragma: no cover - typing only
    from mtgz_ir.cards.unclaimed_lines import LineDiagnosis

PRIMARY_TYPES: tuple[str, ...] = (
    "Planeswalker", "Battle", "Creature", "Instant", "Sorcery",
    "Artifact", "Enchantment", "Land",
)

_REMINDER_RE = re.compile(r"\s*\([^)]*\)")
_NUMBER_RE = re.compile(r"\b\d+\b")
_MANA_RE = re.compile(r"\{[^}]*\}")


def primary_type(card: dict) -> str:
    """First matching primary type of the card's front face."""
    front = card.get("type_line", "").split(" // ")[0]
    for t in PRIMARY_TYPES:
        if t in front:
            return t
    return "Other"


def strip_reminder_text(line: str) -> str:
    return _REMINDER_RE.sub("", line).strip()


def oracle_lines(card: dict) -> list[str]:
    """Reminder-stripped, non-empty oracle lines across all faces."""
    texts: list[str] = []
    if card.get("card_faces"):
        for face in card["card_faces"]:
            texts.append(face.get("oracle_text", ""))
    else:
        texts.append(card.get("oracle_text", ""))
    out: list[str] = []
    for text in texts:
        for raw in text.split("\n"):
            line = strip_reminder_text(raw)
            if line:
                out.append(line)
    return out


def line_shape(line: str, card_name: str) -> str:
    """Normalize an oracle line into a coarse grammar 'shape' so the
    same template printed on many cards counts once: numbers -> N,
    mana symbols -> {M}, the card's own name -> CARDNAME."""
    s = line
    short = card_name.split(" // ")[0]
    for alias in (card_name, short, short.split(",")[0]):
        if alias:
            s = s.replace(alias, "CARDNAME")
    s = _MANA_RE.sub("{M}", s)
    s = _NUMBER_RE.sub("N", s)
    return s


@dataclass
class CoverageSummary:
    pool_name: str
    total: int
    claimed: int
    by_type: dict[str, tuple[int, int]]          # type -> (claimed, total)
    by_layout: dict[str, tuple[int, int]]        # layout -> (claimed, total)
    unclaimed_keywords: list[tuple[str, int]]
    unclaimed_shapes: list[tuple[str, int, str]]  # shape, count, example card
    unclaimed_cards: list[dict] = field(default_factory=list)
    claimed_names: list[str] = field(default_factory=list)
    # Present only when per-line diagnoses were supplied
    # (``mtgz_ir.cards.unclaimed_lines``): shapes of the lines that
    # actually fail, and the count of unclaimed cards per diagnosis kind.
    failing_shapes: list[tuple[str, int, str]] = field(default_factory=list)
    diagnosis_kinds: dict[str, int] = field(default_factory=dict)
    # Authority-list cards with no Scryfall record (not in the pool).
    custom_cards: list[dict] = field(default_factory=list)

    @property
    def pct(self) -> float:
        return 100.0 * self.claimed / self.total if self.total else 0.0


def summarize_coverage(
    cards: Iterable[dict],
    claimed_names: Iterable[str],
    *,
    pool_name: str = "cube",
    top_keywords: int = 40,
    top_shapes: int = 60,
    line_diagnoses: Mapping[str, "LineDiagnosis"] | None = None,
    custom_cards: Iterable[dict] = (),
) -> CoverageSummary:
    cards = list(cards)
    diagnoses = line_diagnoses or {}
    failing_counter: Counter[str] = Counter()
    failing_example: dict[str, str] = {}
    kinds: Counter[str] = Counter()
    claimed = set(claimed_names)
    by_type: dict[str, list[int]] = {}
    by_layout: dict[str, list[int]] = {}
    kw_counter: Counter[str] = Counter()
    shape_counter: Counter[str] = Counter()
    shape_example: dict[str, str] = {}
    unclaimed: list[dict] = []
    n_claimed = 0
    for card in cards:
        name = card["name"]
        is_claimed = name in claimed
        t = primary_type(card)
        layout = card.get("layout", "normal")
        by_type.setdefault(t, [0, 0])[1] += 1
        by_layout.setdefault(layout, [0, 0])[1] += 1
        if is_claimed:
            n_claimed += 1
            by_type[t][0] += 1
            by_layout[layout][0] += 1
            continue
        kw_counter.update(card.get("keywords", []))
        lines = oracle_lines(card)
        for line in lines:
            shape = line_shape(line, name)
            shape_counter[shape] += 1
            shape_example.setdefault(shape, name)
        entry = {
            "name": name,
            "type_line": card.get("type_line", ""),
            "mana_cost": card.get("mana_cost", ""),
            "layout": layout,
            "oracle_lines": lines,
        }
        diag = diagnoses.get(name)
        if diag is not None:
            entry["diagnosis"] = diag.kind
            entry["failing_lines"] = diag.failing_text
            kinds[diag.kind] += 1
            for text in diag.failing_text:
                shape = line_shape(text, name)
                failing_counter[shape] += 1
                failing_example.setdefault(shape, name)
        unclaimed.append(entry)
    shapes = [
        (shape, n, shape_example[shape])
        for shape, n in shape_counter.most_common()
        if n >= 2
    ][:top_shapes]
    failing_shapes = [
        (shape, n, failing_example[shape])
        for shape, n in failing_counter.most_common()
        if n >= 2
    ][:top_shapes]
    return CoverageSummary(
        pool_name=pool_name,
        total=len(cards),
        claimed=n_claimed,
        by_type={k: (v[0], v[1]) for k, v in sorted(by_type.items())},
        by_layout={k: (v[0], v[1]) for k, v in sorted(by_layout.items())},
        unclaimed_keywords=kw_counter.most_common(top_keywords),
        unclaimed_shapes=shapes,
        unclaimed_cards=sorted(
            unclaimed, key=lambda c: (primary_type(c), c["name"]),
        ),
        claimed_names=sorted(claimed & {c["name"] for c in cards}),
        failing_shapes=failing_shapes,
        diagnosis_kinds=dict(sorted(kinds.items())),
        custom_cards=list(custom_cards),
    )


def render_markdown(
    summary: CoverageSummary,
    *,
    script_name: str = "scripts/measure_cube_coverage.py",
) -> str:
    s = summary
    lines: list[str] = []
    lines.append(
        f"Generated by `{script_name}` - DO NOT EDIT MANUALLY. "
        "Run the script to refresh."
    )
    lines.append("")
    lines.append(f"# Parser coverage of the `{s.pool_name}` card pool")
    lines.append("")
    lines.append(
        f"**{s.claimed} of {s.total} cards claimed ({s.pct:.1f}%).** "
        "A card counts as claimed only when every oracle line parses; "
        "there are no partial claims."
    )
    lines.append("")
    lines.append("## By primary type")
    lines.append("")
    lines.append("| Type | Claimed | Total | % |")
    lines.append("|---|---:|---:|---:|")
    for t, (c, n) in s.by_type.items():
        lines.append(f"| {t} | {c} | {n} | {100.0 * c / n:.0f}% |")
    lines.append("")
    lines.append("## By layout")
    lines.append("")
    lines.append("| Layout | Claimed | Total |")
    lines.append("|---|---:|---:|")
    for lay, (c, n) in s.by_layout.items():
        lines.append(f"| {lay} | {c} | {n} |")
    lines.append("")
    lines.append("## Keywords on unclaimed cards")
    lines.append("")
    lines.append(
        "Scryfall `keywords` frequency across unclaimed cards. A keyword "
        "the engine lacks blocks every card carrying it."
    )
    lines.append("")
    lines.append("| Keyword | Unclaimed cards |")
    lines.append("|---|---:|")
    for kw, n in s.unclaimed_keywords:
        lines.append(f"| {kw} | {n} |")
    lines.append("")
    lines.append("## Most common oracle-line shapes on unclaimed cards")
    lines.append("")
    lines.append(
        "Numbers -> `N`, mana symbols -> `{M}`, self-reference -> "
        "`CARDNAME`. Each shape is roughly one parameterized recognizer's "
        "worth of work (some may already parse: a card is unclaimed if "
        "*any* of its lines fails)."
    )
    lines.append("")
    lines.append("| Cards | Shape | Example |")
    lines.append("|---:|---|---|")
    for shape, n, example in s.unclaimed_shapes:
        safe = shape.replace("|", "\\|")
        lines.append(f"| {n} | {safe} | {example} |")
    lines.append("")
    if s.diagnosis_kinds:
        lines.extend(_render_diagnosis_sections(s))
    lines.append("## Unclaimed cards")
    lines.append("")
    current = None
    for card in s.unclaimed_cards:
        t = primary_type(card)
        if t != current:
            current = t
            lines.append(f"### {t}")
            lines.append("")
        text = " / ".join(card["oracle_lines"]) or "(no rules text)"
        text = text.replace("|", "\\|")
        lines.append(
            f"- **{card['name']}** `{card['mana_cost']}` - "
            f"{card['type_line']} - {text}"
        )
        if "diagnosis" in card:
            lines.extend(_render_card_diagnosis(card))
    lines.append("")
    if s.custom_cards:
        lines.extend(_render_custom_cards(s.custom_cards))
    lines.append("## Claimed cards")
    lines.append("")
    lines.append(", ".join(s.claimed_names))
    lines.append("")
    return "\n".join(lines)


_KIND_NOTES = {
    "lines": "the failing lines are the whole blocker",
    "unconfirmed": "no line parses and the card's frame (layout, type) "
                   "couldn't be built to check it doesn't block too",
    "mixed": "lines fail and the rest don't build together either",
    "structure": "every line parses; the card as a whole doesn't build",
    "gate": "every line parses; the name must be opted into a per-name "
            "builder allowlist (cube_enchantments.CUBE_ENCHANTMENT_NAMES)",
}


def _md_escape(text: str) -> str:
    return text.replace("|", "\\|")


def _render_diagnosis_sections(s: CoverageSummary) -> list[str]:
    lines: list[str] = []
    lines.append("## Why cards are unclaimed (per-line probe)")
    lines.append("")
    lines.append(
        "Each unclaimed card's oracle lines were probed one at a time "
        "through the parser's own builder chain "
        "(`mtgz_ir.cards.unclaimed_lines`): a line that builds alone, or "
        "that builds next to every other passing line (riders such as "
        "additional costs), passes. Under each card below, **fails** "
        "lists the lines that don't. The probe is a planning aid, never "
        "a claim."
    )
    lines.append("")
    lines.append("| Diagnosis | Cards | Meaning |")
    lines.append("|---|---:|---|")
    for kind, n in s.diagnosis_kinds.items():
        lines.append(f"| {kind} | {n} | {_KIND_NOTES.get(kind, '')} |")
    lines.append("")
    if s.failing_shapes:
        lines.append("## Most common failing-line shapes")
        lines.append("")
        lines.append(
            "Like the shape table above, but counting only lines the "
            "probe found failing."
        )
        lines.append("")
        lines.append("| Lines | Shape | Example |")
        lines.append("|---:|---|---|")
        for shape, n, example in s.failing_shapes:
            lines.append(f"| {n} | {_md_escape(shape)} | {example} |")
        lines.append("")
    return lines


def _render_card_diagnosis(card: dict) -> list[str]:
    out = [f"  - fails: {_md_escape(text)}"
           for text in card.get("failing_lines", ())]
    if card["diagnosis"] != "lines":
        out.append(f"  - diagnosis: {card['diagnosis']} "
                   f"({_KIND_NOTES[card['diagnosis']]})")
    return out


def _render_custom_cards(custom: list[dict]) -> list[str]:
    lines = ["## Custom cards without a Scryfall record", ""]
    lines.append(
        "Named by the pool's authority list but absent from Scryfall and "
        "without a hand-authored record (`data/*_custom.json`), so they "
        "are in no pool file and not counted above. Their text is the "
        "draft group's own."
    )
    lines.append("")
    for card in custom:
        text = card.get("custom_text") or "(no text available)"
        lines.append(f"- **{card['name']}** - {text}")
    lines.append("")
    return lines
