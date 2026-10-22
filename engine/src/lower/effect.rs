//! Effects -> programs. One arm per supported IR class; each arm names every
//! field of its node and lowers onto the shared ops (ENGINE.md 3, 7).

use super::filter::{lower_pred, lower_pred_or_oneof};
use super::Lowerer;
use crate::db::{types, AbilityDef, Filter};
use crate::mana::{letter_index, ManaCost};
use crate::program::{Amount, Builder, Op, SpellDest, Who};
use card_ir::{cats, enums, generated, nodes, unions, Count};

pub(crate) fn lower_ability(l: &mut Lowerer, e: &cats::Effect) -> AbilityDef {
    let mut b = Builder::default();
    lower_effect(l, &mut b, e);
    b.finish()
}

/// The single `TargetSpec` of an effect -> a target requirement index.
fn lower_targets(l: &mut Lowerer, b: &mut Builder, specs: &[cats::TargetSpec], what: &str) -> Option<u8> {
    if specs.len() != 1 {
        l.unsupported(format!("{what}.targets(len {})", specs.len()));
        return None;
    }
    let (f, min, max) = lower_target_spec(l, &specs[0])?;
    Some(b.target(f, min, max))
}

fn lower_target_spec(l: &mut Lowerer, s: &cats::TargetSpec) -> Option<(Filter, u8, u8)> {
    let cats::TargetSpec::TargetSpec(nodes::TargetSpec { filter, count_min, count_max, distinct_from }) = s;
    l.deny("TargetSpec.distinct_from", distinct_from);
    let min = l.int("TargetSpec.count_min", *count_min, 0, 8) as u8;
    let max = match count_max {
        Count::Finite(n) => l.int("TargetSpec.count_max", *n, 1, 8) as u8,
        Count::Infinite => {
            l.unsupported("TargetSpec.count_max(any number)");
            1
        }
    };
    if min > max {
        l.unsupported("TargetSpec.count_min(> count_max)");
    }
    let f = lower_pred_or_oneof(l, filter)?;
    Some((f, min, max))
}

fn int_amount(l: &mut Lowerer, what: &str, v: &unions::IntOrFormula) -> Amount {
    match v {
        unions::IntOrFormula::Int(n) => Amount::Fixed(l.int(what, *n, 0, 99)),
        unions::IntOrFormula::Formula(f) => {
            l.unsupported(format!("Formula:{}", f.tag()));
            Amount::Fixed(0)
        }
    }
}

fn spell_target(b: &mut Builder, inner: Filter) -> u8 {
    b.target(Filter::Spell(Box::new(inner)), 1, 1)
}

pub(crate) fn lower_effect(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) {
    use cats::Effect as E;
    match e {
        E::Sequence(nodes::Sequence { effects }) => {
            for x in effects {
                lower_effect(l, b, x);
            }
        }
        E::DealDamageAnyTarget(nodes::DealDamageAnyTarget { amount, targets, optional }) => {
            l.deny("DealDamageAnyTarget.optional", optional);
            let amount = Amount::Fixed(l.int("DealDamageAnyTarget.amount", *amount, 0, 99));
            if let Some(to) = lower_targets(l, b, targets, "DealDamageAnyTarget") {
                b.op(Op::DealDamage { amount, to });
            }
        }
        E::DealDamageToTargetCreature(nodes::DealDamageToTargetCreature { amount, targets, optional }) => {
            l.deny("DealDamageToTargetCreature.optional", optional);
            let amount = Amount::Fixed(l.int("DealDamageToTargetCreature.amount", *amount, 0, 99));
            if let Some(to) = lower_targets(l, b, targets, "DealDamageToTargetCreature") {
                b.op(Op::DealDamage { amount, to });
            }
        }
        E::CounterTargetSpell(nodes::CounterTargetSpell { filter_types }) => {
            let mut inner = Filter::Any;
            if !filter_types.is_empty() {
                let mut bits = 0;
                for t in filter_types {
                    match types::from_name(t) {
                        Some(x) => bits |= x,
                        None => l.unsupported(format!("CounterTargetSpell.filter_types({t})")),
                    }
                }
                inner = Filter::Type(bits);
            }
            let tgt = spell_target(b, inner);
            b.op(Op::Counter { tgt, dest: SpellDest::Graveyard });
        }
        E::CounterSpell(nodes::CounterSpell { spell_filter, unless_pays, only_if, destination, label }) => {
            let _ = label; // display text ("noncreature spell")
            l.deny("CounterSpell.only_if", only_if);
            let inner = match spell_filter {
                Some(p) => lower_pred(l, p).unwrap_or(Filter::Any),
                None => Filter::Any,
            };
            let dest = match destination {
                enums::SpellDestination::Graveyard => SpellDest::Graveyard,
                enums::SpellDestination::LibraryTop => SpellDest::LibraryTop,
                other => {
                    l.unsupported(format!("CounterSpell.destination({other:?})"));
                    SpellDest::Graveyard
                }
            };
            let tgt = spell_target(b, inner);
            if unless_pays.is_empty() {
                b.op(Op::Counter { tgt, dest });
            } else {
                match ManaCost::parse(unless_pays) {
                    Some(cost) => {
                        let at = b.op(Op::OfferPay { who: Who::ControllerOfTarget(tgt), cost, if_paid: 0 });
                        b.op(Op::Counter { tgt, dest });
                        let end = b.here();
                        if let Op::OfferPay { if_paid, .. } = &mut b.program[at] {
                            *if_paid = end;
                        }
                    }
                    None => l.unsupported(format!("CounterSpell.unless_pays({unless_pays})")),
                }
            }
        }
        E::DrawCards(nodes::DrawCards { amount }) => {
            let n = Amount::Fixed(l.int("DrawCards.amount", *amount, 0, 99));
            b.op(Op::Draw { who: Who::You, n });
        }
        E::TargetPlayerDraws(nodes::TargetPlayerDraws { amount }) => {
            let n = int_amount(l, "TargetPlayerDraws.amount", amount);
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Draw { who: Who::TargetPlayer(t), n });
        }
        E::PutCardsFromHandOnTopOfLibrary(nodes::PutCardsFromHandOnTopOfLibrary { amount }) => {
            let n = l.int("PutCardsFromHandOnTopOfLibrary.amount", *amount, 0, 20) as u8;
            b.op(Op::PutFromHandOnTop { who: Who::You, n });
        }
        E::AddManaToPool(nodes::AddManaToPool {
            produces,
            spend_restriction,
            grants_haste_to_creature_spell,
            makes_spell_uncounterable,
        }) => {
            l.deny("AddManaToPool.spend_restriction", spend_restriction);
            l.deny("AddManaToPool.grants_haste_to_creature_spell", grants_haste_to_creature_spell);
            l.deny("AddManaToPool.makes_spell_uncounterable", makes_spell_uncounterable);
            let mut mana = [0u8; 6];
            for (sym, n) in produces {
                match letter_index(sym) {
                    Some(i) => mana[i] += l.int("AddManaToPool.produces", *n, 0, 20) as u8,
                    None => l.unsupported(format!("AddManaToPool.produces({sym})")),
                }
            }
            b.op(Op::AddMana { who: Who::You, mana });
        }
        E::TargetPlayerDiscardsAtRandom(nodes::TargetPlayerDiscardsAtRandom { amount }) => {
            let n = int_amount(l, "TargetPlayerDiscardsAtRandom.amount", amount);
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::DiscardRandom { who: Who::TargetPlayer(t), n });
        }
        E::TakeExtraTurnAfterThisOne(nodes::TakeExtraTurnAfterThisOne {}) => {
            b.op(Op::ExtraTurn { who: Who::You });
        }
        E::RemoveTargetPermanents(nodes::RemoveTargetPermanents {
            action,
            target_filter,
            count_min,
            count_max,
            can_be_regenerated,
            rider,
            only_if,
            count_from_x,
            targets,
        }) => {
            l.deny("RemoveTargetPermanents.rider", rider);
            l.deny("RemoveTargetPermanents.only_if", only_if);
            l.deny("RemoveTargetPermanents.count_from_x", count_from_x);
            l.deny_ne(
                "RemoveTargetPermanents.count_min",
                count_min,
                &generated::defaults::d_RemoveTargetPermanents_count_min(),
            );
            l.deny_ne(
                "RemoveTargetPermanents.count_max",
                count_max,
                &generated::defaults::d_RemoveTargetPermanents_count_max(),
            );
            // Regeneration does not exist yet, so "can't be regenerated"
            // would be the only difference; it is a blocker until it does.
            l.deny_ne(
                "RemoveTargetPermanents.can_be_regenerated",
                can_be_regenerated,
                &generated::defaults::d_RemoveTargetPermanents_can_be_regenerated(),
            );
            // The IR repeats the target filter; the two must agree.
            let tf = lower_pred(l, target_filter);
            let Some(tgt) = lower_targets(l, b, targets, "RemoveTargetPermanents") else { return };
            if tf.as_ref() != Some(&b.targets[tgt as usize].filter) {
                l.unsupported("RemoveTargetPermanents.target_filter(differs from targets)");
            }
            match action {
                enums::RemovalAction::Destroy => {
                    b.op(Op::Destroy { tgt });
                }
                enums::RemovalAction::Exile => {
                    b.op(Op::Exile { tgt });
                }
                other => l.unsupported(format!("RemoveTargetPermanents.action({other:?})")),
            }
        }
        other => l.unsupported(format!("Effect:{}", other.tag())),
    }
}
