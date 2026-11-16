# Parser project rules

The parser converts printed oracle text directly into engine
`CardPrinting` objects via per-line recognizers. The goal is a
generic translator: every shape of printed text maps to a parameterized
recognizer, and adding a new card that fits an existing shape costs
zero code.

## Two granularities of recognizer

1. **Parameterized recognizers** (preferred) — regex-based templates
   that capture varying parts of a printed shape. One recognizer covers
   every card with that shape, present and future.

   Examples:
   - `_EQUIPPED_CREATURE_GETS_RE` captures `+N/+M` plus a comma-joined
     keyword rider. Matches every Equipment with that surface.
   - `_PUMP_AND_GAIN_KEYWORDS_UEOT` captures four subject prefixes,
     the bonus, and the keyword body in one regex.
   - Table-based dispatches like `_EXTERNAL_COST_REDUCTION_LINES`
     where adding a new surface is one entry, not a new function.

2. **Literal-line recognizers** — exact-string matches against a
   printed oracle sentence. One literal covers exactly one card.

   Examples (the parser pool today): `_EQUIPPED_ATTACKS_MICROSIZER_LINE`,
   `_EQUIPPED_GETS_AND_HAS_DOMINION_BRACELET_LINE`.

## When a literal-line recognizer is acceptable

A literal is fine ONLY when:

- Exactly one card in the current data pool fits that printed surface,
  AND
- The recognizer's docstring or surrounding comment explicitly notes
  why it isn't generalized today (typically: "only card in the pool
  with this shape; broader grammar deferred until a second example
  exists").

A literal that has no comment about deferred generalization is a
review smell.

## What to do when adding a card with a similar shape

When you're about to add a new recognizer and you think "this looks
like a more general version of the existing
`_EQUIPPED_ATTACKS_MICROSIZER_LINE`":

1. **Stop and look at the existing recognizer first.** Don't add a
   second literal next to the first.
2. Identify the underlying *shape* both cards share — the parts that
   stay constant across cards and the parts that vary.
3. Replace the literal with a parameterized regex whose capture groups
   correspond to the varying parts. Inner phrases (`<effect>`,
   `<quoted-activated-ability>`, etc.) parse through the unified
   `_parse_effect_phrase` / `_parse_activated_ability_line` grammars
   recursively, so the parameterized recognizer claims any future card
   with that shape automatically.
4. The original card claimed by the literal must still claim through
   the new parameterized recognizer; verify with the existing tests.
5. Delete the literal constant.

Example: today `_EQUIPPED_ATTACKS_MICROSIZER_LINE` is a literal because
Atomic Microsizer is the only EoE Equipment with a quoted attack
trigger that doesn't fit the existing equipped-attacks shapes (Saw,
Baseball Bat, Hylderblade are each their own literal). When a SECOND
similar card lands, the right move is `r"^Whenever equipped creature
attacks, (.+)\.$"` with the captured tail dispatched through the
unified phrase grammar — replacing all four literals with one regex.

## Smells to look for during review

- A regex with **zero capture groups** that matches exactly one card →
  smell. Either generalize, or land a literal with the deferred-
  generalization comment.
- Two literal-line constants whose strings differ only in a parameter
  → merge into one regex.
- A new recognizer that duplicates an existing recognizer's structure
  with one field swapped → that field is a parameter; add it to the
  existing recognizer instead of cloning.
- A bespoke factory in `_BESPOKE_*` whose oracle text is matched by an
  existing parameterized recognizer → migrate by removing the bespoke
  entry and verifying the unified path claims it.

## Faithful to oracle text

Recognizers must reflect what is **printed**, not what the static
registry shipped. If a bespoke factory adds behavior the printed text
doesn't say (e.g. a synthetic ETB auto-attach that's nowhere on the
card), drop the spurious behavior when migrating; update affected
tests to match the faithful behavior.

If the printed text contains a clause the engine doesn't model (e.g.
"This spell can't be countered."), the right answer is to **implement
the mechanic** in the engine, not silently drop the clause as a
parser-side no-op. A no-op recognizer is the same kind of dishonest
shortcut as a synthetic-behavior factory — both make the parser drift
from the printed surface.
