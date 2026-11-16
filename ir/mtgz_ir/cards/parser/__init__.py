"""Oracle-text parser sub-package.

Decomposes Scryfall JSON into ``CardPrinting``s by recognizing
keyword lines, triggered abilities, activated abilities, static
abilities, printing-level keywords (Warp / Ward), and per-card-type
builder shapes (creature / spell / land). One module per concern so
parallel work on different grammars stays in disjoint files.

Public surface: ``OracleParserCardSource`` re-exported here. Module
internals are imported directly from their per-concern files where
needed (or via the legacy ``mtgz_ir.cards.parser_source`` shim).
"""

from mtgz_ir.cards.parser.source import OracleParserCardSource

__all__ = ["OracleParserCardSource"]
