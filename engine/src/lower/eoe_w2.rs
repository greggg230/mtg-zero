//! Edge of Eternities W2, mana and lands (ENGINE.md 99.12): the effect
//! classes of its cards. Same rules as `effect.rs`: every field named, the
//! rest denied; classes named after a card carry no fields (their meaning is
//! the class's Oracle text, docs/ir_defects.md 11) and lower onto ops named
//! after rules actions.

use super::Lowerer;
use crate::db::{types, Filter};
use crate::eoe_w2::EoeW2;
use crate::program::{Amount, Builder, Ext, Op, PileDest, Who, PILE_ANY};
use card_ir::{cats, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

fn w2(b: &mut Builder, op: EoeW2) -> usize {
    b.op(Op::Ext(Ext::EoeW2(op)))
}

/// The effect classes W2 lowers; false for any other.
pub(crate) fn lower_effect_eoe_w2(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Destroy each nonland permanent with mana value equal to the number
        // of charge counters on this land" (Blast Zone: sacrificed as the
        // cost, so as it last existed, CR 608.2h; 701.8).
        E::DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters(nodes::DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters {}) => {
            let k = super::eoe_a::charge(l);
            let filter = and(and(Filter::Type(types::PERMANENT), Filter::Not(Box::new(Filter::Type(types::LAND)))), Filter::MvIsSourceCounters(k));
            b.op(Op::DestroyAll { filter });
        }
        // "That creature's controller gains control of this land" (Contested
        // War Zone, the creature that dealt combat damage: the event's).
        E::ChangeControllerToCombatDamageSource(nodes::ChangeControllerToCombatDamageSource {}) => {
            w2(b, EoeW2::EventObjControllerGainsSource);
        }
        // "When this creature enters, look at the top X cards of your
        // library, where X is this creature's power. Put any number of land
        // cards from among them onto the battlefield tapped, then shuffle"
        // (Famished Worldsire: the cards are looked at, not revealed).
        E::FamishedWorldsireETBEffect(nodes::FamishedWorldsireETBEffect {}) => {
            b.op(Op::Ext(Ext::LookTopN { who: Who::You, n: Amount::SourcePower }));
            b.op(Op::PileTo { who: Who::You, dest: PileDest::BattlefieldTapped, filter: Filter::Type(types::LAND), min: 0, max: PILE_ANY, rest: false, reveal: false });
            b.op(Op::Shuffle { who: Who::You });
        }
        _ => return false,
    }
    true
}
