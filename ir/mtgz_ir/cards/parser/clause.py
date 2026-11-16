"""Top-level clause dispatch.

A creature's oracle text is parsed line-by-line. Each line that
isn't a special pre-clause check (Warp, Ward, sorcery-speed rider —
those live in ``printing_keywords`` and ``activated``) goes through
``_parse_clause`` which dispatches to the per-clause-type parsers
in keyword/trigger/activated/static order.

The result, ``_ParsedClause``, is a small dataclass with mutually
exclusive populated fields — the creature builder consumes it by
inspecting which field is set.
"""

from __future__ import annotations

from dataclasses import dataclass

from mtgz_ir.cards.base import (
    ActivatedAbility,
    StaticAbility,
    TriggeredAbility,
)
from mtgz_ir.cards.parser.activated import _parse_activated_ability_line
from mtgz_ir.cards.parser.keywords import _parse_keyword_line
from mtgz_ir.cards.parser.statics import _parse_static_ability_line
from mtgz_ir.cards.parser.triggers import (
    _parse_triggered_ability_line,
    _substitute_self_name,
)


@dataclass(frozen=True)
class _ParsedClause:
    """One oracle-text clause that successfully parsed.

    Fields are mutually exclusive — a clause contributes either one
    or more keyword strings, one or more triggered abilities, an
    activated ability, or a static ability, never more than one
    category. A keyword-only line that lists several keywords joined
    by ``, `` parses to a single ``_ParsedClause`` whose ``keywords``
    tuple holds every keyword on that line. A triggered-ability line
    with a compound condition (e.g. ``When this creature enters or
    dies, …``) parses to a single ``_ParsedClause`` whose ``triggers``
    tuple holds one ``TriggeredAbility`` per condition arm — the
    builder treats them as if each arm appeared on its own oracle
    line. Static-ability lines populate ``statics`` with one or more
    ``StaticAbility`` instances; multi-keyword anthem lines fan out
    into one StaticAbility per keyword (each carrying a layer-6
    ``ContinuousModifier(GrantKeyword)``), which is why the field is
    a tuple rather than a single optional.
    """

    keywords: tuple[str, ...] = ()
    triggers: tuple[TriggeredAbility, ...] = ()
    activated: ActivatedAbility | None = None
    statics: tuple[StaticAbility, ...] = ()

    @property
    def static(self) -> StaticAbility | None:
        """Back-compat shim for callers that still want a single
        static ability. Returns ``None`` if no statics or more than
        one — multi-static clauses (multi-keyword anthems) must be
        consumed via ``statics``.

        Builders that only model single-static clauses can keep using
        ``.static``: a multi-static anthem will surface as ``None``
        here, matching the "out of scope" semantics those builders
        already apply when a clause's static field doesn't carry
        exactly what they expect.
        """
        if len(self.statics) == 1:
            return self.statics[0]
        return None

    @property
    def keyword(self) -> str | None:
        """Back-compat shim for callers that still want a single
        keyword. Returns ``None`` if no keywords or more than one
        — multi-keyword clauses must be consumed via ``keywords``.
        """
        if len(self.keywords) == 1:
            return self.keywords[0]
        return None

    @property
    def trigger(self) -> TriggeredAbility | None:
        """Back-compat shim for callers that still want a single
        triggered ability. Returns ``None`` if no triggers or more
        than one — multi-trigger clauses (compound conditions like
        ``enters or dies``) must be consumed via ``triggers``.

        Builders that only model single-trigger clauses (artifact,
        spacecraft) can keep using ``.trigger``: a multi-trigger line
        will surface as ``None`` here, matching the "out of scope"
        semantics those builders already apply when a clause's
        trigger field doesn't carry exactly what they expect.
        """
        if len(self.triggers) == 1:
            return self.triggers[0]
        return None


def _parse_clause(
    line: str, *, name: str | None = None,
) -> _ParsedClause | None:
    """Return a ``_ParsedClause`` for a recognized line, or ``None``
    if the parser doesn't know how to handle it. ``None`` causes the
    enclosing card to be skipped.

    ``name`` is the card's own printed name; when supplied the
    triggered-ability parser substitutes name references in
    trigger-condition phrases with the canonical ``"this creature"``
    form before lookup, so a printing whose oracle text reads
    ``"Whenever Haliya enters or attacks, …"`` parses through the
    same condition table as ``"Whenever this creature enters or
    attacks, …"``. Other clause grammars (keywords / activated /
    static) don't reference the source's name today so they ignore
    the parameter.
    """
    line = line.strip()
    # Apply self-name substitution early so all downstream parsers
    # (keywords, triggered, activated, static) see the canonical
    # ``"this creature"`` surface rather than a printing's own name.
    # ``_parse_triggered_ability_line`` also applies this substitution
    # internally; the double-application is idempotent (the substituted
    # line no longer contains the name, so the second pass is a no-op).
    # Activated-ability lines (Ragost's "{1},{T},Sacrifice a Food:
    # Ragost deals 3 damage …") need the substitution before the
    # ``_parse_activated_ability_line`` call below; applying it here
    # rather than per-parser keeps the architecture clean.
    if name is not None:
        line = _substitute_self_name(line, name)
    keywords = _parse_keyword_line(line)
    if keywords is not None:
        return _ParsedClause(keywords=keywords)
    triggered = _parse_triggered_ability_line(line, name=name)
    if triggered is not None:
        # Single-condition trigger — wrap in a 1-tuple. Compound-
        # condition triggers (``enters or dies``) take a different
        # path inside ``_parse_triggered_ability_line`` and surface
        # as a tuple of ``TriggeredAbility`` instances; we normalize
        # both shapes here so downstream consumers see a uniform
        # ``triggers`` field.
        if isinstance(triggered, tuple):
            return _ParsedClause(triggers=triggered)
        return _ParsedClause(triggers=(triggered,))
    activated = _parse_activated_ability_line(line)
    if activated is not None:
        return _ParsedClause(activated=activated)
    statics = _parse_static_ability_line(line, name=name)
    if statics:
        return _ParsedClause(statics=statics)
    return None
