//! Effects of M7d, the last cube cards that needed engine subsystems
//! (ENGINE.md 31). Same rules as `effect.rs`: every field named, the rest
//! denied.

use super::Lowerer;
use crate::db::{ActEffect, ActivatedDef, Cond, Filter};
use crate::mana::ManaCost;
use crate::program::{Builder, Ext, Op, M7d};
use card_ir::{cats, nodes};

/// Lower an effect class of M7d; `false` if the class is not one of these.
pub(crate) fn lower_effect_m7d(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Choose an exiled card an opponent owns with a void counter on it.
        // You may play it this turn without paying its mana cost." (Dauthi
        // Voidwalker.)
        E::ChooseOpponentsExiledCardWithCounterMayPlayFree(nodes::ChooseOpponentsExiledCardWithCounterMayPlayFree { counter }) => {
            let counter = super::effect_ext::counter_by_name(l, counter);
            b.op(Op::Ext(Ext::M7d(M7d::ExiledWithCounterPlayFree { counter })));
        }
        // "Exile target card from a graveyard. If it was a permanent card, put
        // a +1/+1 counter on this permanent." (Lion Sash.)
        E::ExileGraveyardCardCounterIfPermanent(nodes::ExileGraveyardCardCounterIfPermanent {}) => {
            let tgt = b.target(Filter::InGraveyard(Box::new(Filter::Any)), 1, 1);
            b.op(Op::Ext(Ext::M7d(M7d::ExileTargetCounterIfPermanent { tgt })));
        }
        // Reconfigure (CR 702.151a): "attach to target creature you control"
        // (another: 301.5c) and "unattach" (only while attached: the
        // printing's lowering sets that condition, `reconfigure_abilities`).
        E::ReconfigureAttach(nodes::ReconfigureAttach {}) => {
            let f = Filter::And(
                Box::new(Filter::Type(crate::db::types::CREATURE)),
                Box::new(Filter::And(Box::new(Filter::Controller(true)), Box::new(Filter::Not(Box::new(Filter::Source))))),
            );
            let tgt = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::Attach { tgt }));
        }
        E::ReconfigureUnattach(nodes::ReconfigureUnattach {}) => {
            b.op(Op::Ext(Ext::M7d(M7d::UnattachSource)));
        }
        // "{1}{U}: Level 2" (CR 716.2a: "this Class's level becomes N"; the
        // ability is its level N-1 face's, sorcery-speed in the IR).
        E::GainClassLevel(nodes::GainClassLevel { level }) => {
            let level = l.int("GainClassLevel.level", *level, 2, 9) as u8;
            b.op(Op::Ext(Ext::M7d(M7d::GainClassLevel { level })));
        }
        // "Put three +1/+1 counters on target artifact you control. If it
        // isn't a creature, it becomes a 0/0 Robot creature in addition to
        // its other types." (Does Machines' level 3.)
        E::ThenTargetBecomesCreatureIfNot(nodes::ThenTargetBecomesCreatureIfNot { inner, power, toughness, subtype }) => {
            let Some(inner) = inner.as_deref() else {
                l.unsupported("ThenTargetBecomesCreatureIfNot.inner(missing)");
                return true;
            };
            let n = b.targets.len();
            super::effect::lower_effect(l, b, inner);
            if b.targets.len() != n + 1 || subtype.is_empty() {
                l.unsupported("ThenTargetBecomesCreatureIfNot(not one target)");
                return true;
            }
            let p = l.int("ThenTargetBecomesCreatureIfNot.power", *power, 0, 20);
            let t = l.int("ThenTargetBecomesCreatureIfNot.toughness", *toughness, 0, 20);
            let subtype = l.subtype(subtype);
            b.op(Op::Ext(Ext::M7d(M7d::BecomeCreatureIfNot { tgt: n as u8, p, t, subtype })));
        }
        // "Reveal the top card of your library. If it's a land card, put it
        // onto the battlefield. Otherwise, put it into your hand." (Nadu.)
        E::RevealTopLandToBattlefieldElseHand(nodes::RevealTopLandToBattlefieldElseHand {}) => {
            b.op(Op::Ext(Ext::M7d(M7d::RevealTopLandElseHand)));
        }
        // "Shuffle your library, then exile the top card. Until end of turn,
        // you may play that card without paying its mana cost." (Urza.)
        E::ShuffleThenExileTopMayPlayFree(nodes::ShuffleThenExileTopMayPlayFree {}) => {
            b.op(Op::Ext(Ext::M7d(M7d::ShuffleExileTopPlayFree)));
        }
        // "Target opponent exiles the top card of their library, a card at
        // random from their graveyard, and a card at random from their hand.
        // You may cast a spell from among cards exiled this way without
        // paying its mana cost." (Crabomination.)
        E::TargetOpponentExilesThreeMayCastOneFree(nodes::TargetOpponentExilesThreeMayCastOneFree {}) => {
            let tgt = b.target(Filter::And(Box::new(Filter::Player), Box::new(Filter::Opponent)), 1, 1);
            b.op(Op::Ext(Ext::M7d(M7d::OppExilesThreeMayCastOne { tgt })));
        }
        // "Until end of turn, lands you control gain 'Sacrifice this land: Add
        // {B}.'" (Rain of Filth; the lands locked in, CR 611.2c.)
        E::GrantAbilityToPermanentsYouControlUEOT(nodes::GrantAbilityToPermanentsYouControlUEOT { card_filter, ability }) => {
            let (Some(f), Some(a)) = (card_filter.as_deref(), ability.as_deref()) else {
                l.unsupported("GrantAbilityToPermanentsYouControlUEOT(missing a part)");
                return true;
            };
            let Some(filter) = super::filter::lower_card_filter(l, f) else { return true };
            let Some(ad) = super::activated::lower_activated(l, a, super::activated::From::Battlefield, false) else { return true };
            let mask = match (&ad.effect, ad.cost == crate::db::Cost { sac_self: true, ..crate::db::Cost::default() }) {
                (ActEffect::Mana(m), true) if m.damage_to_you == 0 && m.only == 0 && !m.haste && !m.uncounterable && m.sac_if_no.is_none() => match m.prod {
                    crate::db::ManaProd::One(k) => Some(k),
                    _ => None,
                },
                _ => None,
            };
            let Some(mask) = mask else {
                l.unsupported("GrantAbilityToPermanentsYouControlUEOT.ability(not \"sacrifice this: add one mana\")");
                return true;
            };
            let on = crate::program::On::Each(Filter::And(Box::new(filter), Box::new(Filter::Controller(true))));
            b.op(Op::Ext(Ext::Apply { on, mods: vec![crate::db::Mod::GainSacMana(mask)], until: crate::layers::Until::EndOfTurn }));
        }
        // Manifest dread (CR 701.62a; Abhorrent Oculus).
        E::ManifestDread(nodes::ManifestDread { count }) => {
            if *count != 1 {
                l.unsupported("ManifestDread.count(not 1)");
            }
            b.op(Op::Ext(Ext::M7d(M7d::ManifestDread)));
        }
        // "<first>. When you do, <then>" (Ajani, Nacatl Avenger's 0): `first`
        // happens, then a reflexive triggered ability triggers (CR 603.12);
        // its targets are chosen as it is put on the stack. An `IfCondition`
        // at the head of `then` is the reflexive ability's intervening "if"
        // (603.4: "When you do, if you control a red permanent other than
        // Ajani, ..."), checked as it triggers and as it resolves.
        E::DoThenReflexive(nodes::DoThenReflexive { first, then }) => {
            let (Some(first), Some(then)) = (first.as_deref(), then.as_deref()) else {
                l.unsupported("DoThenReflexive(missing a part)");
                return true;
            };
            let n = b.targets.len();
            super::effect::lower_effect(l, b, first);
            if b.targets.len() != n {
                l.unsupported("DoThenReflexive.first(with targets)");
            }
            let (cond, effect) = match then {
                E::IfCondition(nodes::IfCondition { condition, then }) => match super::filter::lower_condition(l, condition) {
                    Some(c) => (Some(c), &**then),
                    None => return true,
                },
                other => (None, other),
            };
            let mut d = Builder::default();
            super::effect::lower_effect(l, &mut d, effect);
            let k = match cond {
                Some(c) => l.delayed_ability_if(d.finish(), c),
                None => l.delayed_ability(d.finish()),
            };
            b.op(Op::Ext(Ext::Reflexive { k }));
        }
        // "Shuffle <this spell> into its owner's library" as the last
        // instruction of its effect (Green Sun's Zenith, CR 608.2c; only as
        // it resolves: countered, it goes to the graveyard). Checked to be
        // the spell's last instruction (`check_self_shuffle`).
        E::ShuffleThisSpellIntoOwnersLibrary(nodes::ShuffleThisSpellIntoOwnersLibrary {}) => {
            b.op(Op::Ext(Ext::M7d(M7d::ShuffleThisSpellIntoLibrary)));
        }
        // "Whenever <event> this turn, <effect>" (Forth Eorlingas!): a
        // delayed triggered ability with a duration (CR 603.7b), the
        // controller's until the turn ends; each is a delayed-style ability
        // of the card, as "until your next turn, whenever ..." (25.6).
        E::CreateThisTurnTriggeredAbility(nodes::CreateThisTurnTriggeredAbility { triggered_abilities }) => {
            for t in triggered_abilities {
                if let Some(td) = super::trigger::lower_trigger(l, t, 0) {
                    if td.event == crate::db::EventKind::Delayed || !td.pred.intervening().is_empty() {
                        l.unsupported("CreateThisTurnTriggeredAbility(trigger shape)");
                    }
                    l.delayed.push(td);
                    let k = (l.delayed.len() - 1) as u8;
                    b.op(Op::Ext(Ext::M7d(M7d::TriggerThisTurn { k })));
                }
            }
            if triggered_abilities.is_empty() {
                l.unsupported("CreateThisTurnTriggeredAbility(none)");
            }
        }
        _ => return false,
    }
    true
}

/// "Shuffle this spell into its owner's library" (`ShuffleThisSpellIntoLibrary`)
/// is the last instruction of a spell's own effect (CR 608.2c, 608.2n): in a
/// spell ability, last (in each mode); in no other ability.
pub(crate) fn check_self_shuffle(l: &mut Lowerer, def: &crate::db::CardDef) {
    let is = |o: &Op| matches!(o, Op::Ext(Ext::M7d(M7d::ShuffleThisSpellIntoLibrary)));
    let last_only = |ab: &crate::db::AbilityDef| {
        let ok = |p: &[Op]| p.iter().position(is).is_none_or(|i| i + 1 == p.len());
        ok(&ab.program) && ab.modes.as_ref().is_none_or(|m| m.list.iter().all(|x| ok(&x.program)))
    };
    let anywhere = |ab: &crate::db::AbilityDef| ab.program.iter().any(is) || ab.modes.as_ref().is_some_and(|m| m.list.iter().any(|x| x.program.iter().any(is)));
    let spell_ok = def.spell.as_ref().is_none_or(|s| last_only(s) && (def.is(crate::db::types::INSTANT) || def.is(crate::db::types::SORCERY) || !anywhere(s)));
    let other = def.triggers.iter().any(|t| anywhere(&t.ability))
        || def.activated.iter().chain(&def.hand_abilities).chain(&def.gy_abilities).any(|a| matches!(&a.effect, ActEffect::Stack(ab) if anywhere(ab)));
    if !spell_ok || other {
        l.unsupported("ShuffleThisSpellIntoOwnersLibrary(not the last instruction of an instant or sorcery)");
    }
}

/// Reconfigure (CR 702.151a): the printing's `reconfigure_cost` must be the
/// mana cost of its two reconfigure abilities, both sorcery-speed; the
/// unattach one may be activated only while the permanent is attached.
pub(crate) fn reconfigure_abilities(l: &mut Lowerer, activated: &mut [ActivatedDef], cost: ManaCost) {
    let is = |ad: &ActivatedDef, f: &dyn Fn(&Op) -> bool| match &ad.effect {
        ActEffect::Stack(ab) => ab.program.iter().any(f),
        ActEffect::Mana(_) => false,
    };
    let mut found = (false, false);
    for ad in activated.iter_mut() {
        let attach = is(ad, &|o| matches!(o, Op::Ext(Ext::Attach { .. })));
        let unattach = is(ad, &|o| matches!(o, Op::Ext(Ext::M7d(M7d::UnattachSource))));
        if !(attach || unattach) {
            continue;
        }
        if ad.cost.mana != cost || !ad.sorcery || ad.only_if.is_some() {
            l.unsupported("CardPrinting.reconfigure_cost(differs from its abilities)");
        }
        if unattach {
            ad.only_if = Some(Cond::SourceAttached);
            found.1 = true;
        } else {
            found.0 = true;
        }
    }
    if found != (true, true) {
        l.unsupported("CardPrinting.reconfigure_cost(without its two abilities)");
    }
}

/// The triggered ability "creatures you control have" (Nadu, Winged Wisdom):
/// "whenever this creature becomes the target of a spell or ability, <the
/// ability>. This ability triggers only n times each turn" (per creature).
/// It is the granting card's own trigger, for its controller's creatures
/// that have the granted abilities (`TrigPred::Granted`, ENGINE.md 27.3).
pub(crate) fn granted_target_trigger(ability: crate::db::AbilityDef, n: u8) -> crate::db::TriggerDef {
    use crate::db::{EventKind, Slot, TrigPred};
    let creature_yours = Filter::And(Box::new(Filter::Type(crate::db::types::CREATURE)), Box::new(Filter::Controller(true)));
    let subject = TrigPred::And(Box::new(TrigPred::Subject(Slot::Target, creature_yours)), Box::new(TrigPred::Granted(Slot::Target)));
    let pred = TrigPred::And(Box::new(subject), Box::new(TrigPred::TimesPerSubject(Slot::Target, n)));
    let uses_event = ability.uses_event();
    crate::db::TriggerDef { event: EventKind::BecomesTarget, pred, ability, uses_event }
}
