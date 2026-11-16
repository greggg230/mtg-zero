"""Tests for the Duskmourn spells / enchantments batch-7 parser additions.

Each commit in this batch lands one or more cards via a small additive
change to the parser's whole-spell exact-text dispatch table. No
existing engine classes are modified beyond additive flag widenings;
the parser's job here is to claim spells whose printed shapes don't
fit the per-clause grammar but DO admit a clean simplification onto an
existing primitive (parser-is-right precedent: Unravel / Divert
Disaster / Trial of Agony / Turn Inside Out).

Cards claimed in this batch:

* **Impossible Inferno** ``{4}{R}`` Instant — 6-damage burn plus a
  Delirium-gated impulse-draw rider. Whole-spell dispatch simplifies
  to bare ``DealDamageToTargetCreature(amount=6)``; the Delirium-
  gated impulse-draw rider is dropped because the spell-builder has
  no Delirium-gated conditional dispatcher today and the second
  clause doesn't parse as a standalone spell-effect phrase
  (parser-is-right precedent: Trial of Agony — keep the base burn,
  drop the unmodelable rider).

* **Violent Urge** ``{R}`` Instant — +1/+0 first-strike UEOT pump
  plus a Delirium-gated double-strike upgrade rider. Whole-spell
  dispatch simplifies to bare ``PumpTargetCreatureUntilEOT(
  power_bonus=1, granted_keywords=frozenset({"first strike"}))``;
  the Delirium-gated double-strike upgrade is dropped per the same
  parser-is-right precedent as Impossible Inferno.

* **Drag to the Roots** ``{2}{B}{G}`` Instant — Delirium-gated
  ``{2}``-cost-reduction rider plus a bare destroy-target-nonland-
  permanent body. Whole-spell dispatch simplifies to bare
  ``DestroyTargetNonlandPermanent()``; the cost-reduction is
  dropped because the cost-reduction parser doesn't recognize the
  ``Delirium — ... as long as ...`` shape and adding the
  ``four_or_more_card_types_in_graveyard`` engine condition would be
  a one-off for this card (parser-is-right precedent: Impossible
  Inferno).

* **Come Back Wrong** ``{2}{B}`` Sorcery — destroy target creature
  with a conditional reanimate-then-sac-next-end-step rider. Whole-
  spell dispatch simplifies to bare ``DestroyTargetCreature()``;
  the conditional reanimation + delayed end-step sacrifice rider
  is dropped because the engine has no "if put into graveyard this
  way, reanimate-and-schedule-end-step-sac" primitive
  (parser-is-right precedent: Trial of Agony).
"""

from __future__ import annotations

from mtgz_ir.cards.parser.source import OracleParserCardSource


def _claimed_names() -> frozenset[str]:
    return frozenset(OracleParserCardSource().claimed_names)


# ---------- Impossible Inferno ----------

def test_impossible_inferno_claimed_by_parser():
    # Impossible Inferno ``{4}{R}`` Instant — claimed via the new
    # whole-spell exact-text dispatch entry that simplifies the
    # printed text (6-damage burn + Delirium-gated impulse-draw
    # rider) to bare ``DealDamageToTargetCreature(amount=6)``.
    assert "Impossible Inferno" in _claimed_names()


def test_impossible_inferno_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``DealDamageToTargetCreature(amount=6)`` engine
    # class — base mechanic preserved (the 6-damage burn), Delirium-
    # gated impulse-draw rider dropped (parser-is-right precedent:
    # Trial of Agony).
    from mtgz_ir.cards.base import DealDamageToTargetCreature

    parser = OracleParserCardSource()
    p = parser.get_printings()["Impossible Inferno"]
    assert p.spell_effect == DealDamageToTargetCreature(amount=6)


def test_impossible_inferno_is_instant():
    # Type-line carries Instant — not Sorcery — so the spell can be
    # cast at instant speed in response to opponent's combat / spells.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Impossible Inferno"]
    assert "Instant" in p.card_types


def test_impossible_inferno_mana_cost():
    # Mono-red ``{4}{R}`` mana cost — confirms the spell carries the
    # printed cost through the factory.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Impossible Inferno"]
    assert p.mana_cost_str == "{4}{R}"


# ---------- Violent Urge ----------

def test_violent_urge_claimed_by_parser():
    # Violent Urge ``{R}`` Instant — claimed via the new whole-spell
    # exact-text dispatch entry that simplifies the printed text
    # (+1/+0 first-strike UEOT + Delirium-gated double-strike
    # upgrade) to bare ``PumpTargetCreatureUntilEOT(power_bonus=1,
    # granted_keywords=frozenset({"first strike"}))``.
    assert "Violent Urge" in _claimed_names()


def test_violent_urge_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified pump engine class — power bonus and first-strike
    # keyword grant preserved, Delirium-gated double-strike upgrade
    # rider dropped (parser-is-right precedent: Impossible Inferno).
    from mtgz_ir.cards.base import PumpTargetCreatureUntilEOT

    parser = OracleParserCardSource()
    p = parser.get_printings()["Violent Urge"]
    assert p.spell_effect == PumpTargetCreatureUntilEOT(
        power_bonus=1,
        toughness_bonus=0,
        granted_keywords=frozenset({"first strike"}),
    )


def test_violent_urge_is_instant():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Violent Urge"]
    assert "Instant" in p.card_types


def test_violent_urge_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Violent Urge"]
    assert p.mana_cost_str == "{R}"


# ---------- Drag to the Roots ----------

def test_drag_to_the_roots_claimed_by_parser():
    # Drag to the Roots ``{2}{B}{G}`` Instant — claimed via the new
    # whole-spell exact-text dispatch entry that simplifies the
    # printed text (Delirium-gated {2} cost reduction + destroy
    # target nonland permanent) to bare
    # ``DestroyTargetNonlandPermanent()``. The cost reduction is
    # dropped because the cost-reduction parser doesn't recognize
    # the ``Delirium — ... as long as ...`` shape.
    assert "Drag to the Roots" in _claimed_names()


def test_drag_to_the_roots_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``DestroyTargetNonlandPermanent()`` engine
    # class — base mechanic preserved (the destroy), Delirium-gated
    # cost reduction dropped (parser-is-right precedent: Impossible
    # Inferno).
    from mtgz_ir.cards.base import DestroyTargetNonlandPermanent

    parser = OracleParserCardSource()
    p = parser.get_printings()["Drag to the Roots"]
    assert p.spell_effect == DestroyTargetNonlandPermanent()


def test_drag_to_the_roots_is_instant():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Drag to the Roots"]
    assert "Instant" in p.card_types


def test_drag_to_the_roots_mana_cost():
    # Full ``{2}{B}{G}`` mana cost — the Delirium cost-reduction
    # rider was dropped during simplification, so the printed cost
    # is what the engine uses (no ``cost_reductions`` rule attached).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Drag to the Roots"]
    assert p.mana_cost_str == "{2}{B}{G}"


def test_drag_to_the_roots_no_cost_reductions():
    # The Delirium cost-reduction was dropped (it isn't a recognized
    # cost-reduction shape), so the printing has no
    # ``cost_reductions`` rule attached.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Drag to the Roots"]
    assert p.cost_reductions == ()


# ---------- Come Back Wrong ----------

def test_come_back_wrong_claimed_by_parser():
    # Come Back Wrong ``{2}{B}`` Sorcery — claimed via the new
    # whole-spell exact-text dispatch entry that simplifies the
    # printed text (destroy target creature + conditional reanimate-
    # to-your-control + delayed end-step sacrifice rider) to bare
    # ``DestroyTargetCreature()``.
    assert "Come Back Wrong" in _claimed_names()


def test_come_back_wrong_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``DestroyTargetCreature()`` engine class — base
    # mechanic preserved (the destroy), conditional reanimation +
    # delayed end-step sacrifice rider dropped (parser-is-right
    # precedent: Trial of Agony — drop the unmodelable rider, keep
    # the base mechanic).
    from mtgz_ir.cards.base import DestroyTargetCreature

    parser = OracleParserCardSource()
    p = parser.get_printings()["Come Back Wrong"]
    assert p.spell_effect == DestroyTargetCreature()


def test_come_back_wrong_is_sorcery():
    # Type-line carries Sorcery — not Instant — so the spell can
    # only be cast at sorcery speed.
    parser = OracleParserCardSource()
    p = parser.get_printings()["Come Back Wrong"]
    assert "Sorcery" in p.card_types


def test_come_back_wrong_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Come Back Wrong"]
    assert p.mana_cost_str == "{2}{B}"


# ---------- Let's Play a Game ----------

def test_lets_play_a_game_claimed_by_parser():
    # Let's Play a Game ``{3}{B}`` Sorcery — claimed via the new
    # whole-spell exact-text dispatch entry that simplifies the
    # printed text (Delirium-gated ``Choose one or more`` upgrade
    # over a base 3-mode ``Choose one`` modal) to a bare 3-mode
    # ``ModalChooseOne``.
    assert "Let's Play a Game" in _claimed_names()


def test_lets_play_a_game_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``ModalChooseOne`` engine class — base 3-mode
    # shape preserved (the three printed modes), Delirium-gated
    # ``choose one or more instead`` upgrade dropped per the
    # parser-is-right precedent set by Get Out / Untimely Malfunction
    # (claim a well-modeled subset of a multi-mode modal rather than
    # leaving the spell unclaimed).
    from mtgz_ir.cards.base import (
        EachOpponentDiscards,
        EachOpponentLosesLife,
        GainLife,
        ModalChooseOne,
        PumpCreaturesOpponentsControlUntilEOT,
        Sequence,
    )

    parser = OracleParserCardSource()
    p = parser.get_printings()["Let's Play a Game"]
    assert p.spell_effect == ModalChooseOne(
        modes=(
            PumpCreaturesOpponentsControlUntilEOT(
                power_bonus=-1, toughness_bonus=-1,
            ),
            EachOpponentDiscards(count=2),
            Sequence(
                effects=(
                    EachOpponentLosesLife(amount=3),
                    GainLife(amount=3),
                ),
            ),
        ),
    )


def test_lets_play_a_game_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Let's Play a Game"]
    assert "Sorcery" in p.card_types


def test_lets_play_a_game_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Let's Play a Game"]
    assert p.mana_cost_str == "{3}{B}"


def test_lets_play_a_game_modal_not_or_both():
    # The simplified modal is bare ``Choose one`` (or_both=False) —
    # not the ``Choose one or both`` shape used by Scour for Scrap.
    # The Delirium-gated upgrade to ``choose one or more instead``
    # would need a new combinator that the parser doesn't have today;
    # keeping the base ``or_both=False`` matches the printed default
    # behavior (without the four-card-types graveyard threshold).
    parser = OracleParserCardSource()
    p = parser.get_printings()["Let's Play a Game"]
    assert p.spell_effect.or_both is False


# ---------- Demonic Counsel ----------

def test_demonic_counsel_claimed_by_parser():
    # Demonic Counsel ``{1}{B}`` Sorcery — claimed via the new
    # whole-spell exact-text dispatch entry that simplifies the
    # printed text (Demon-tutor + Delirium-gated any-card upgrade)
    # to bare ``SearchLibraryForCard(filter_name="demon",
    # may_fail=False)``. The Demon-subtype filter is an additive
    # branch on the existing ``SearchLibraryForCard._matches``
    # dispatcher, mirroring the existing ``sliver`` shape.
    assert "Demonic Counsel" in _claimed_names()


def test_demonic_counsel_engine_class():
    # Belt-and-suspenders that the produced ``CardPrinting`` carries
    # the simplified ``SearchLibraryForCard(filter_name="demon",
    # may_fail=False)`` engine class — base mechanic preserved (the
    # forced Demon tutor), Delirium-gated any-card upgrade dropped
    # (parser-is-right precedent: Impossible Inferno).
    from mtgz_ir.cards.base import SearchLibraryForCard

    parser = OracleParserCardSource()
    p = parser.get_printings()["Demonic Counsel"]
    assert p.spell_effect == SearchLibraryForCard(
        filter_name="demon", may_fail=False,
    )


def test_demonic_counsel_is_sorcery():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Demonic Counsel"]
    assert "Sorcery" in p.card_types


def test_demonic_counsel_mana_cost():
    parser = OracleParserCardSource()
    p = parser.get_printings()["Demonic Counsel"]
    assert p.mana_cost_str == "{1}{B}"


def test_search_library_for_card_demon_filter_matches_demon_creature():
    # Belt-and-suspenders that the additive ``demon`` filter on
    # ``SearchLibraryForCard._matches`` keys off the printing's
    # ``subtypes`` set (mirrors the existing ``sliver`` filter).
    from mtgz_ir.cards.base import SearchLibraryForCard, make_creature

    eff = SearchLibraryForCard(filter_name="demon", may_fail=False)
    demon_pr = make_creature(
        "Test Demon", "{4}{B}{B}", power=6, toughness=6,
        subtypes=("Demon",),
    )
    non_demon_pr = make_creature(
        "Bear", "{1}{G}", power=2, toughness=2,
    )
    assert eff._matches(demon_pr) is True
    assert eff._matches(non_demon_pr) is False


def test_search_library_for_card_demon_filter_describe():
    # The describe string interpolates the filter name; this confirms
    # the additive ``demon`` filter doesn't break the existing
    # describe() shape (no special-casing needed for the new filter).
    from mtgz_ir.cards.base import SearchLibraryForCard

    desc = SearchLibraryForCard(
        filter_name="demon", may_fail=False,
    ).describe()
    assert "demon" in desc
    assert "library" in desc
