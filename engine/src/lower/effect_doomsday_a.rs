//! Legacy Doomsday, lane A (ENGINE.md 110.1): the lowering of Doomsday,
//! Pact of Negation's delayed trigger and Flow State. Same rules as
//! `effect.rs`: every field named, the rest denied; each lowers onto ops
//! named after rules actions. (Pact's "Counter target spell", its delayed
//! trigger's timing and Personal Tutor's search lower through the existing
//! arms: `CounterTargetSpell`, `ScheduleDelayedEffect`, `SearchLibrary`.)

use super::Lowerer;
use crate::db::Filter;
use crate::doomsday_a::DdA;
use crate::mana::ManaCost;
use crate::program::{Builder, Ext, Op, PileDest, Who};
use card_ir::{cats, nodes};

fn dd(op: DdA) -> Op {
    Op::Ext(Ext::DdA(op))
}

/// "The rest on the bottom / on top of your library in any order": every
/// card of the pile, one decision per card (`Op::PileTo`).
fn pile_rest(dest: PileDest) -> Op {
    Op::PileTo { who: Who::You, dest, filter: Filter::Any, min: 0, max: 0, rest: true, reveal: false }
}

/// "Put `k` of them into your hand" (as many as there are, if fewer).
fn pile_to_hand(k: u8) -> Op {
    Op::PileTo { who: Who::You, dest: PileDest::Hand, filter: Filter::Any, min: k, max: k, rest: false, reveal: false }
}

pub(crate) fn lower_effect_dd_a(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Search your library and graveyard for N cards and exile the rest.
        // Put the chosen cards on top of your library in any order. You lose
        // half your life, rounded up." (Doomsday: the class is the whole
        // text, N its count.)
        E::DoomsdayPile(nodes::DoomsdayPile { count }) => {
            let n = l.int("DoomsdayPile.count", *count, 1, 64) as u8;
            b.op(dd(DdA::PileFromGraveyard { n }));
            b.op(dd(DdA::PileFromLibrary { n }));
            b.op(pile_rest(PileDest::Top));
            b.op(dd(DdA::LoseHalfLifeUp));
        }
        // "Look at the top N cards of your library. Put K of them into your
        // hand and the rest on the bottom of your library in any order. If
        // there is an instant card and a sorcery card in your graveyard,
        // instead put K' of them into your hand and the rest on the bottom of
        // your library in any order." (Flow State.) The condition is read as
        // the cards are put into the hand, after the look (CR 608.2c; the
        // look moves nothing, so before or after is the same graveyard).
        E::LookAtTopPutIntoHandMoreIfInstantAndSorcery(nodes::LookAtTopPutIntoHandMoreIfInstantAndSorcery { count, take, take_if }) => {
            let n = l.int("LookAtTopPutIntoHandMoreIfInstantAndSorcery.count", *count, 1, 64) as u8;
            let k = l.int("LookAtTopPutIntoHandMoreIfInstantAndSorcery.take", *take, 1, 64) as u8;
            let k2 = l.int("LookAtTopPutIntoHandMoreIfInstantAndSorcery.take_if", *take_if, 1, 64) as u8;
            b.op(Op::LookTop { who: Who::You, n, reveal: false });
            let plain = b.op(dd(DdA::JumpUnlessInstantAndSorceryInGraveyard { to: 0 }));
            b.op(pile_to_hand(k2));
            let end = b.op(Op::Jump { to: 0 });
            b.patch(plain);
            b.op(pile_to_hand(k));
            b.patch(end);
            b.op(pile_rest(PileDest::Bottom));
        }
        // "Pay <cost>. If you don't, <effect>." (Pact of Negation's delayed
        // trigger): the controller may pay (and may decline even if able,
        // CR 118.12a), activating mana abilities during the payment (605.3a);
        // if they don't, the effect.
        E::PayManaOrElse(nodes::PayManaOrElse { cost, otherwise }) => match ManaCost::parse(cost) {
            Some(cost) => {
                let at = b.op(Op::OfferPay { who: Who::You, cost, if_paid: 0 });
                super::effect::lower_effect(l, b, otherwise);
                b.patch(at);
            }
            None => l.unsupported(format!("PayManaOrElse.cost({cost})")),
        },
        // "You lose the game." (CR 104.3e.)
        E::LoseTheGame(nodes::LoseTheGame {}) => {
            b.op(dd(DdA::LoseGame));
        }
        _ => return false,
    }
    true
}
