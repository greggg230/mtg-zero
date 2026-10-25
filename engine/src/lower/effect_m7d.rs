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
        _ => return false,
    }
    true
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
