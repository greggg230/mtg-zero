"""Compatibility shim for the old flat ``parser_source`` module.

The parser was split into the ``mtgz_ir.cards.parser`` sub-package so
each grammar concern lives in its own file (keywords, triggers,
effects, activated/static/printing-level abilities, type line, and
the per-card-type builders). This shim re-exports every name the old
flat module exposed so existing callers — production code in
``cards/source.py`` and test files importing private helpers —
continue to work unchanged. New code should import directly from the
sub-package.
"""

from __future__ import annotations

from mtgz_ir.cards.parser.activated import (  # noqa: F401
    _is_sorcery_rider_line,
    _normalize_activated_effect_phrase,
    _parse_activated_ability_line,
    _parse_activation_cost,
)
from mtgz_ir.cards.parser.artifact_builder import (  # noqa: F401
    _build_artifact_printing,
)
from mtgz_ir.cards.parser.clause import (  # noqa: F401
    _ParsedClause,
    _parse_clause,
)
from mtgz_ir.cards.parser.creature_builder import (  # noqa: F401
    _build_creature_printing,
)
from mtgz_ir.cards.parser.equipment_builder import (  # noqa: F401
    _build_equipment_printing,
)
from mtgz_ir.cards.parser.effects import (  # noqa: F401
    _normalize_creature_effect_phrase,
    _normalize_spell_effect_phrase,
    _parse_effect,
    _parse_effect_phrase,
    _parse_spell_effect_phrase,
)
from mtgz_ir.cards.parser.keywords import (  # noqa: F401
    _KEYWORD_LINES,
    _parse_keyword_line,
)
from mtgz_ir.cards.parser.land_builder import (  # noqa: F401
    _build_land_printing,
)
from mtgz_ir.cards.parser.printing_keywords import (  # noqa: F401
    _parse_ward_line,
    _parse_warp_line,
)
from mtgz_ir.cards.parser.source import (  # noqa: F401
    OracleParserCardSource,
)
from mtgz_ir.cards.parser.spacecraft_builder import (  # noqa: F401
    _build_spacecraft_printing,
)
from mtgz_ir.cards.parser.spell_builder import (  # noqa: F401
    _build_spell_printing,
    _parse_spell_oracle_text,
    _parse_void_damage_replacement,
)
from mtgz_ir.cards.parser.statics import (  # noqa: F401
    _parse_static_ability_line,
)
from mtgz_ir.cards.parser.triggers import (  # noqa: F401
    _parse_trigger_phrase,
    _parse_triggered_ability_line,
)
from mtgz_ir.cards.parser.type_line import (  # noqa: F401
    _parse_type_line,
)
