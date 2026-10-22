//! Effects -> programs. One arm per supported IR class; each arm names every
//! field of its node and lowers onto the shared ops (ENGINE.md 3, 7).

use super::filter::{lower_card_filter, lower_condition_or_state, lower_pred, lower_pred_or_oneof};
use super::Lowerer;
use crate::db::{types, AbilityDef, Filter, ManaProd};
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
                enums::SpellDestination::Exile => SpellDest::Exile,
                enums::SpellDestination::Hand => SpellDest::Hand,
            };
            let tgt = spell_target(b, inner);
            if unless_pays.is_empty() {
                b.op(Op::Counter { tgt, dest });
            } else {
                match ManaCost::parse(unless_pays) {
                    Some(cost) => {
                        let at = b.op(Op::OfferPay { who: Who::ControllerOfTarget(tgt), cost, if_paid: 0 });
                        b.op(Op::Counter { tgt, dest });
                        b.patch(at);
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
            let mut any = 0u8;
            for (sym, n) in produces {
                let n = l.int("AddManaToPool.produces", *n, 0, 20) as u8;
                match letter_index(sym) {
                    Some(i) => mana[i] += n,
                    None if sym == "any" => any += n,
                    None => l.unsupported(format!("AddManaToPool.produces({sym})")),
                }
            }
            if any > 0 {
                if mana.iter().any(|&n| n > 0) {
                    l.unsupported("AddManaToPool.produces(any mixed with fixed)");
                }
                // "N mana in any combination of colors" (Manamorphose).
                let prod = ManaProd::Combo { mask: crate::mana::COLORS, n: any };
                b.op(Op::AddManaChoice { who: Who::You, prod });
            } else {
                b.op(Op::AddMana { who: Who::You, mana });
            }
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
            // Riders read the removed object's last known information.
            let lki_rider = match rider.as_deref() {
                None => None,
                Some(cats::RemovalRider::ControllerGainsLifeEqualToPower(nodes::ControllerGainsLifeEqualToPower {})) => {
                    Some(Op::GainLife { who: Who::LkiController, n: Amount::LkiPower })
                }
                Some(other) => {
                    l.unsupported(format!("RemovalRider:{}", other.tag()));
                    None
                }
            };
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
            // "Destroy target creature if it has mana value 2 or less": a
            // check on resolution, not a targeting restriction.
            let guard = match only_if {
                Some(p) => {
                    if b.targets[tgt as usize].max != 1 {
                        l.unsupported("RemoveTargetPermanents.only_if(several targets)");
                    }
                    lower_pred(l, p).map(|filter| b.op(Op::JumpUnlessTarget { tgt, filter, to: 0 }))
                }
                None => None,
            };
            if lki_rider.is_some() {
                b.op(Op::Remember { tgt });
            }
            match action {
                enums::RemovalAction::Destroy => {
                    b.op(Op::Destroy { tgt });
                }
                enums::RemovalAction::Exile => {
                    b.op(Op::Exile { tgt });
                }
                enums::RemovalAction::Bounce => {
                    b.op(Op::ReturnToHand { tgt });
                }
            }
            if let Some(op) = lki_rider {
                b.op(op);
            }
            if let Some(at) = guard {
                b.patch(at);
            }
        }
        E::Destroy(nodes::Destroy { targets }) => {
            if let Some(tgt) = lower_targets(l, b, targets, "Destroy") {
                b.op(Op::Destroy { tgt });
            }
        }
        E::DestroyTargetLand(nodes::DestroyTargetLand { targets, nonbasic_only }) => {
            // The IR repeats the filter in `targets`; check it agrees.
            let Some(tgt) = lower_targets(l, b, targets, "DestroyTargetLand") else { return };
            let land = Filter::Type(types::LAND);
            let want = if *nonbasic_only {
                Filter::And(
                    Box::new(land),
                    Box::new(Filter::Not(Box::new(Filter::Supertype(crate::db::supertypes::BASIC)))),
                )
            } else {
                land
            };
            if b.targets[tgt as usize].filter != want {
                l.unsupported("DestroyTargetLand.targets(differ from nonbasic_only)");
            }
            b.op(Op::Destroy { tgt });
        }
        E::RemoveAllMatching(nodes::RemoveAllMatching {
            permanent_filter,
            action,
            can_be_regenerated,
            max_mana_value_from_x,
            rider,
            label,
        }) => {
            let _ = label; // display text
            l.deny("RemoveAllMatching.max_mana_value_from_x", max_mana_value_from_x);
            l.deny("RemoveAllMatching.rider", rider);
            // Regeneration does not exist yet: "can't be regenerated" is
            // the only form that changes nothing (CR 701.15).
            l.deny_ne(
                "RemoveAllMatching.can_be_regenerated",
                can_be_regenerated,
                &false,
            );
            let Some(filter) = lower_pred(l, permanent_filter) else { return };
            match action {
                enums::RemovalAction::Destroy => {
                    b.op(Op::DestroyAll { filter });
                }
                other => l.unsupported(format!("RemoveAllMatching.action({other:?})")),
            }
        }
        E::GainLife(nodes::GainLife { amount }) => {
            let n = Amount::Fixed(l.int("GainLife.amount", *amount, 0, 99));
            b.op(Op::GainLife { who: Who::You, n });
        }
        E::LoseLife(nodes::LoseLife { amount }) => {
            let n = Amount::Fixed(l.int("LoseLife.amount", *amount, 0, 99));
            b.op(Op::LoseLife { who: Who::You, n });
        }
        E::Bounce(nodes::Bounce { targets }) => {
            if let Some(tgt) = lower_targets(l, b, targets, "Bounce") {
                b.op(Op::ReturnToHand { tgt });
            }
        }
        E::ReturnTargetCreatureToOwnersHand(nodes::ReturnTargetCreatureToOwnersHand {
            targets,
            allow_nonland_permanent,
            create_drone_if_was_attacking,
            optional,
        }) => {
            l.deny("ReturnTargetCreatureToOwnersHand.allow_nonland_permanent", allow_nonland_permanent);
            l.deny("ReturnTargetCreatureToOwnersHand.create_drone_if_was_attacking", create_drone_if_was_attacking);
            l.deny("ReturnTargetCreatureToOwnersHand.optional", optional);
            if let Some(tgt) = lower_targets(l, b, targets, "ReturnTargetCreatureToOwnersHand") {
                b.op(Op::ReturnToHand { tgt });
            }
        }
        E::ReturnTargetSpellToOwnersHand(nodes::ReturnTargetSpellToOwnersHand {}) => {
            let tgt = spell_target(b, Filter::Any);
            b.op(Op::ReturnToHand { tgt });
        }
        E::Untap(nodes::Untap { targets }) => {
            if let Some(tgt) = lower_targets(l, b, targets, "Untap") {
                b.op(Op::Untap { tgt });
            }
        }
        E::DealDamage(nodes::DealDamage { amount, targets }) => {
            let amount = Amount::Fixed(l.int("DealDamage.amount", *amount, 0, 99));
            if let Some(to) = lower_targets(l, b, targets, "DealDamage") {
                b.op(Op::DealDamage { amount, to });
            }
        }
        E::DiscardSelf(nodes::DiscardSelf { amount }) => {
            let n = l.int("DiscardSelf.amount", *amount, 0, 20) as u8;
            b.op(Op::Discard { who: Who::You, n });
        }
        E::EachPlayerDiscardsHand(nodes::EachPlayerDiscardsHand {}) => {
            b.op(Op::DiscardHand { who: Who::Active });
            b.op(Op::DiscardHand { who: Who::NonActive });
        }
        E::EachPlayerDraws(nodes::EachPlayerDraws { amount }) => {
            let n = Amount::Fixed(l.int("EachPlayerDraws.amount", *amount, 0, 99));
            b.op(Op::Draw { who: Who::Active, n });
            b.op(Op::Draw { who: Who::NonActive, n });
        }
        E::YouAndTargetOpponentEachDraw(nodes::YouAndTargetOpponentEachDraw { amount }) => {
            let n = Amount::Fixed(l.int("YouAndTargetOpponentEachDraw.amount", *amount, 0, 99));
            let t = b.target(Filter::And(Box::new(Filter::Player), Box::new(Filter::Opponent)), 1, 1);
            b.op(Op::Draw { who: Who::You, n });
            b.op(Op::Draw { who: Who::TargetPlayer(t), n });
        }
        E::TargetPlayerTakesExtraTurn(nodes::TargetPlayerTakesExtraTurn {}) => {
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::ExtraTurn { who: Who::TargetPlayer(t) });
        }
        E::PutTargetPermanentIntoOwnersLibrary(nodes::PutTargetPermanentIntoOwnersLibrary {
            card_filter,
            position,
            controller_gains_life,
        }) => {
            let Some(f) = lower_card_filter(l, card_filter) else { return };
            let depth = match position {
                enums::LibraryPosition::SecondFromTop => 1,
                other => {
                    l.unsupported(format!("PutTargetPermanentIntoOwnersLibrary.position({other:?})"));
                    return;
                }
            };
            let tgt = b.target(f, 1, 1);
            let life = l.int("PutTargetPermanentIntoOwnersLibrary.controller_gains_life", *controller_gains_life, 0, 99);
            if life > 0 {
                b.op(Op::Remember { tgt });
            }
            b.op(Op::PutInLibrary { tgt, depth });
            if life > 0 {
                b.op(Op::GainLife { who: Who::LkiController, n: Amount::Fixed(life) });
            }
        }
        E::IfConditionElse(nodes::IfConditionElse { condition, then_effect, else_effect }) => {
            let Some(cond) = lower_condition_or_state(l, condition) else { return };
            if b.reuse.is_some() {
                l.unsupported("IfConditionElse(nested in a branch)");
                return;
            }
            let j = b.op(Op::JumpUnless { cond, to: 0 });
            let t0 = b.targets.len();
            lower_effect(l, b, then_effect);
            let t1 = b.targets.len();
            match else_effect {
                None => b.patch(j),
                Some(e) => {
                    let end = b.op(Op::Jump { to: 0 });
                    b.patch(j);
                    // One target described in both branches (Unholy Heat).
                    b.reuse = Some(t0);
                    b.reuse_failed = false;
                    lower_effect(l, b, e);
                    if b.reuse_failed || b.reuse != Some(t1) {
                        l.unsupported("IfConditionElse.targets(differ between branches)");
                    }
                    b.reuse = None;
                    b.patch(end);
                }
            }
        }
        E::MayDo(nodes::MayDo { inner, prompt }) => {
            let _ = prompt; // display text
            let at = b.op(Op::May { who: Who::You, skip: 0 });
            lower_effect(l, b, inner);
            b.patch(at);
        }
        other => l.unsupported(format!("Effect:{}", other.tag())),
    }
}
