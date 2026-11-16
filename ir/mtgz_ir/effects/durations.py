"""Duration value types for continuous effects.

Each ``Duration`` translates to the ``(expires_at, expires_at_player_id)``
pair stored on a ``ContinuousEffect`` registry entry. Use them with the
``ApplyEffect(inner, ends_at=...)`` combinator (see ``combinators.py``)
to wrap an inner ``Effect`` and rewrite the lifetime of every registry
entry the inner adds.

All four finite durations are wired end-to-end:

- ``EndOfTurn`` (``"cleanup"``) — dropped at the active player's
  cleanup step in ``run_cleanup_step``.
- ``EndOfYourNextTurn`` (``"controller_untap"`` keyed on the
  controller) — dropped at the controller's next untap step in
  ``run_untap_step``. Exercised end-to-end by
  ``PumpCreaturesOpponentsControlUntilEOT``'s
  ``until_caster_next_turn=True`` keyword path, and by the
  registry-walk reader in ``_compute_effective_pt`` for P/T pumps
  scoped ``until your next turn`` (e.g. Mouth of the Storm).
- ``EndOfCombat`` (``"end_of_combat"``) — dropped at the
  end-of-combat step in ``clear_combat_state`` (combat.py). No
  production card uses this duration today; the wiring is in place
  so a future card can route through ``ApplyEffect(<inner>,
  EndOfCombat())`` without further plumbing.
- ``Permanent`` (``None``) — stays in the registry until the source
  leaves play (per ``move_off_battlefield``).

``UntilSourceLeaves`` is currently an alias for ``Permanent`` — see
its class docstring for when to differentiate.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Duration:
    """Base class for the lifetime of a continuous effect.

    Subclasses translate to ``(expires_at, expires_at_player_id)`` —
    the pair stored on a ``ContinuousEffect`` registry record.
    ``expires_at`` is the dispatch key consumed by
    ``state.expire_continuous_effects(...)`` calls in turn-based
    actions (cleanup step, controller's untap step). The
    ``expires_at_player_id`` is set when the lifetime is
    controller-relative.
    """

    def to_expires(
        self, controller_id: int,
    ) -> "tuple[str | None, int | None]":
        raise NotImplementedError

    def describe(self) -> str:
        raise NotImplementedError


@dataclass(frozen=True)
class Permanent(Duration):
    """Default lifetime: the entry persists until its source leaves play
    (``expires_at=None``)."""

    def to_expires(
        self, controller_id: int,
    ) -> "tuple[str | None, int | None]":
        return (None, None)

    def describe(self) -> str:
        return "permanently"


@dataclass(frozen=True)
class EndOfTurn(Duration):
    """``until end of turn``: dropped at the active player's cleanup
    step. Maps to ``expires_at="cleanup"`` — the same key used by every
    legacy UEOT registry write today."""

    def to_expires(
        self, controller_id: int,
    ) -> "tuple[str | None, int | None]":
        return ("cleanup", None)

    def describe(self) -> str:
        return "until end of turn"


@dataclass(frozen=True)
class EndOfYourNextTurn(Duration):
    """``until your next turn``: dropped at the start of the controller's
    next untap step. Maps to ``expires_at="controller_untap"`` with the
    controller id stored in ``expires_at_player_id`` — the lifetime used
    by Mouth-of-the-Storm-style pumps (P/T) and keyword grants under the
    same scope. ``run_untap_step`` calls
    ``state.expire_continuous_effects("controller_untap", player_id=ap.id)``
    to drop these at the right moment. Exercised end-to-end by
    ``PumpCreaturesOpponentsControlUntilEOT``'s
    ``until_caster_next_turn=True`` keyword grant via ``ApplyEffect``."""

    def to_expires(
        self, controller_id: int,
    ) -> "tuple[str | None, int | None]":
        return ("controller_untap", controller_id)

    def describe(self) -> str:
        return "until your next turn"


@dataclass(frozen=True)
class EndOfCombat(Duration):
    """``until end of combat``: dropped at the end-of-combat step.
    Maps to ``expires_at="end_of_combat"`` — consumed by
    ``clear_combat_state`` in ``combat.py`` via
    ``state.expire_continuous_effects("end_of_combat")``. End-of-
    combat is not controller-relative (combat is a single set of
    steps within the active player's turn), so
    ``expires_at_player_id`` is unused.

    No production card routes through this duration today; the
    wiring exists so a future card with ``until end of combat``
    oracle text can use ``ApplyEffect(<inner>, EndOfCombat())``
    without further plumbing."""

    def to_expires(
        self, controller_id: int,
    ) -> "tuple[str | None, int | None]":
        return ("end_of_combat", None)

    def describe(self) -> str:
        return "until end of combat"


@dataclass(frozen=True)
class UntilSourceLeaves(Duration):
    """``for as long as <source> is on the battlefield``: currently
    equivalent to ``Permanent`` — both translate to
    ``(expires_at=None, expires_at_player_id=None)``. The registry's
    ``move_off_battlefield`` path drops every entry whose
    ``source_id`` left the battlefield, which already implements the
    "source leaves" semantics for the typical case where the
    effect's source IS the registry entry's source.

    Defined as a distinct ``Duration`` value type for grammar
    fidelity (so ``ApplyEffect(<inner>, UntilSourceLeaves())`` reads
    correctly at the call site) and as the differentiation point for
    future cards that need source-leave semantics distinct from
    registry-source-leave — e.g. an effect whose source object is
    different from the registry entry's ``source_id``. When such a
    card lands, change ``to_expires`` to return a new dispatch key
    (e.g. ``"effect_source_leaves"``) and add a hook in
    ``move_off_battlefield`` (or a parallel teardown site) to drop
    matching entries when the effect's source — not the registry's
    source — leaves play."""

    def to_expires(
        self, controller_id: int,
    ) -> "tuple[str | None, int | None]":
        return (None, None)

    def describe(self) -> str:
        return "for as long as this source is on the battlefield"
