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
