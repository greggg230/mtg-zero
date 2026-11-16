"""Closed string-discriminant vocabularies (``mtgz_ir.effects.vocab``):
each converted field coerces its string to the enum member and rejects a
value outside the vocabulary at construction."""

from __future__ import annotations

import dataclasses
from enum import StrEnum

import pytest

from mtgz_ir.effects import vocab as v
from mtgz_ir.effects.object_predicates import IsCreature


def test_helpers():
    assert v.coerce(v.ZoneName, None) is None
    assert v.coerce(v.ZoneName, "hand") is v.ZoneName.HAND
    assert v.coerce_tuple(v.ZoneName, ("hand", "exile")) == (v.ZoneName.HAND, v.ZoneName.EXILE)
    assert type(v.plain(v.ZoneName.HAND)) is str and v.plain(v.ZoneName.HAND) == "hand"
    assert v.plain(3) == 3 and v.plain(None) is None
    # StrEnum members behave as their values in comparisons, hashing and
    # formatting (the property every existing call site relies on).
    assert v.RemovalAction.EXILE == "exile" and {"exile": 1}[v.RemovalAction.EXILE] == 1
    assert f"{v.RemovalAction.EXILE}" == "exile"


def test_zone_names_match_game_zones():
    from mtgz_ir.game import Zone

    assert {z.value for z in Zone} == {z.value for z in v.ZoneName}


def test_event_subjects_cover_every_dispatched_slot():
    from mtgz_ir.effects.predicates import EVENT_SUBJECTS

    names = {n for slots in EVENT_SUBJECTS.values() for n in slots}
    assert names <= {s.value for s in v.EventSubject}


def _cases():
    from mtgz_ir.cards.base import make_creature
    from mtgz_ir.cards.spell_costs import AdditionalCostOption, AlternativeCost
    from mtgz_ir.effects.base import ActivationCost
    from mtgz_ir.effects.combinators import ConditionalIfYouControlAtLeast
    from mtgz_ir.effects.generic import (
        artifact_mechanics, become_type, card_flow, counterspells,
        cube_creatures_rmc, cube_creatures_ub, enchantment_mechanics,
        graveyard, library_lookat, mana, reanimation, roto_board,
        roto_keywords, roto_stack, roto_walkers, roto_zones, search,
        targeted_removal, zone_moves,
    )
    from mtgz_ir.effects.modifiers import EventPattern, Replace, ReplacementEffect
    from mtgz_ir.effects.predicates import Event
    from mtgz_ir.effects.predicates import Controlled, IsHost, IsSelf, Subject, YOU
    from mtgz_ir.roto_restrictions import CastProhibition

    from mtgz_ir.effects.generic.draw_mill import DrawCards

    draw = DrawCards(amount=1)
    bear = make_creature("Bear", "{1}{G}", 2, 2)
    return [
        (lambda x: dataclasses.replace(bear, aura_enchant_type=x), "aura_enchant_type", "land", v.EnchantKind),
        (lambda x: dataclasses.replace(bear, enters_as_copy_zone=x), "enters_as_copy_zone", "graveyard", v.ZoneName),
        (lambda x: dataclasses.replace(bear, enters_with_counters_from_x=x), "enters_with_counters_from_x", "x_charge", v.XCounterMode),
        (lambda x: ActivationCost(sacrifice_a_permanent_type=x), "sacrifice_a_permanent_type", "artifact", v.CardTypeWord),
        (lambda x: card_flow.SearchLibrary(destination=x), "destination", "top", v.SearchDestination),
        (lambda x: card_flow.LookAtTopCardsPutIntoHand(rest_zone=x), "rest_zone", "graveyard", v.RestZone),
        (lambda x: card_flow.LookAtTopCardsDistribute(piles=(x,)), "piles", "hand", v.DistributePile),
        (lambda x: card_flow.EachPlayerShufflesZonesIntoLibrary(zones=(x,)), "zones", "graveyard", v.ZoneName),
        (lambda x: zone_moves.ShuffleZonesIntoLibrary(zones=(x,)), "zones", "hand", v.ZoneName),
        (lambda x: card_flow.TargetPlayerRevealsHandYouChooseDiscard(destination=x), "destination", "exile", v.RevealedCardFate),
        (lambda x: counterspells.CounterSpell(destination=x), "destination", "library_top", v.SpellDestination),
        (lambda x: graveyard.ReturnFromGraveyard(destination=x), "destination", "battlefield", v.GraveyardReturnDestination),
        (lambda x: reanimation.PutTargetPermanentIntoOwnersLibrary(position=x), "position", "beneath_top_x", v.LibraryPosition),
        (lambda x: reanimation.ReturnCardFromGraveyardToBattlefield(source=x), "source", "top_of_your_graveyard", v.ReanimateSource),
        (lambda x: reanimation.ReturnCardFromGraveyardToBattlefield(haste=x), "haste", "permanent", v.HasteRider),
        (lambda x: targeted_removal.RemoveTargetPermanents(target_filter=IsCreature(), action=x), "action", "bounce", v.RemovalAction),
        (lambda x: targeted_removal.RemoveAllMatching(permanent_filter=IsCreature(), action=x), "action", "exile", v.RemovalAction),
        (lambda x: roto_board.EventPlayerActsPerSourceCounter(action=x, kind="fade", permanent_filter=IsCreature()), "action", "tap", v.PerCounterAction),
        (lambda x: roto_board.EventSubjectDealsDamageToEachOpponent(subject_slot=x), "subject_slot", "attacker", v.EventSubject),
        (lambda x: Replace(new_destination=x), "new_destination", "library", v.ZoneName),
        (lambda x: ReplacementEffect(event_pattern=EventPattern(Event.WOULD_DRAW_CARDS), transform=Replace(new_destination="library"), affects=x), "affects", "opponents", v.AffectedPlayers),
        (lambda x: cube_creatures_rmc.EnteredNotFromZones(zones=(x,)), "zones", "exile", v.ZoneName),
        (lambda x: cube_creatures_rmc.ExileTopCardsWithPlayPermission(duration=x), "duration", "until_your_next_end_step", v.ExilePlayDuration),
        (lambda x: cube_creatures_rmc.ExileTopCardsWithPlayPermission(whose=x), "whose", "that_player", v.ImpulseLibraryOwner),
        (lambda x: cube_creatures_rmc.DealDamageFromEvent(subject_slot=x), "subject_slot", "spell", v.EventSubject),
        (lambda x: cube_creatures_ub.CounterTargetStackObject(kinds=(x,)), "kinds", "activated", v.StackObjectKind),
        (lambda x: cube_creatures_ub.BindEventPlayer(inner=draw, slot=x), "slot", "player", v.EventSubject),
        (lambda x: Subject(x, IsCreature()), "slot", "spell", v.EventSubject),
        (lambda x: IsSelf(x), "subject", "blocker", v.EventSubject),
        (lambda x: IsHost(x), "subject", "attacker", v.EventSubject),
        (lambda x: Controlled(by=YOU, subject=x), "subject", "controller", v.EventSubject),
        (lambda x: roto_zones.DiscardInsteadOfDraw(affects=x), "affects", "you", v.AffectedPlayers),
        (lambda x: roto_zones.CreaturePutIntoGraveyardOf(whose=x), "whose", "opponents", v.GraveyardOwner),
        (lambda x: artifact_mechanics.ScheduleDelayedEffect(inner=draw, step=x), "step", "end", v.DelayedStep),
        (lambda x: mana.AddManaToPool(produces=("C",), spend_restriction=x), "spend_restriction", "colored_only", v.SpendRestriction),
        (lambda x: search.SearchLibraryForCard(filter_name=x), "filter_name", "basic_land", v.LibraryCardFilter),
        (lambda x: library_lookat.LookAtTopNRevealOrCounter(filter_name=x), "filter_name", "creature", v.LibraryCardFilter),
        (lambda x: enchantment_mechanics.OathRevealUntilCreature(card_type=x), "card_type", "artifact", v.CardTypeWord),
        (lambda x: AdditionalCostOption(kind=x), "kind", "pay_life", v.AdditionalCostKind),
        (lambda x: AlternativeCost(turn=x), "turn", "not_yours", v.AlternativeCostTurn),
        (lambda x: become_type.BecomeType(types_to_add=(x,)), "types_to_add", "artifact", v.CardTypeWord),
        (lambda x: roto_stack.CantCastSpellsThisTurn(who=x), "who", "target_player", v.CantCastScope),
        (lambda x: CastProhibition(who=x), "who", "player", v.CastProhibitionScope),
        (lambda x: roto_keywords.PlayerRuleUntilEndOfTurn(kind=x), "kind", "opponents_cant_win", v.PlayerRuleKind),
        (lambda x: roto_keywords.TargetingIs(x), "kind", "ability", v.TargetingSource),
        (lambda x: ConditionalIfYouControlAtLeast(threshold=1, predicate=x, inner=draw), "predicate", "land", v.CardTypeWord),
        (lambda x: roto_walkers.CardsInYourZone(zone=x), "zone", "graveyard", v.ZoneName),
    ]


@pytest.mark.parametrize("case", range(len(_cases())))
def test_field_is_a_closed_vocabulary(case):
    make, field, valid, enum_cls = _cases()[case]
    obj = make(valid)
    value = getattr(obj, field)
    member = value[0] if isinstance(value, tuple) else value
    assert type(member) is enum_cls and member == valid
    assert issubclass(enum_cls, StrEnum)
    with pytest.raises(ValueError):
        make("not_a_" + field)
