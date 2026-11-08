//! Edge of Eternities lane C (ENGINE.md 99.4): void, warp, spells cast this
//! turn, cost reductions, graveyard / exile / library / hand effects. Each
//! arm names every field of its node (no `..`) and lowers onto ops named
//! after rules actions, or `deny`s what it does not implement.

use super::effect::{lower_effect, lower_targets};
use super::Lowerer;
use crate::db::{types, Cond, Discount, Filter};
use crate::eoe_c::EoeC;
use crate::program::{Amount, Builder, Ext, Op, PileDest, Who};
use card_ir::{cats, nodes};

fn and(a: Filter, b: Filter) -> Filter {
    Filter::And(Box::new(a), Box::new(b))
}

fn not(a: Filter) -> Filter {
    Filter::Not(Box::new(a))
}

/// "If <cond>, <then>[; otherwise <else>]": one target described in both
/// branches is one target (as `IfConditionElse`, Unholy Heat).
pub(crate) fn if_else(l: &mut Lowerer, b: &mut Builder, cond: Cond, then: &cats::Effect, other: Option<&cats::Effect>) {
    if b.reuse.is_some() {
        l.unsupported("ConditionalIfVoid(nested in a branch)");
        return;
    }
    let j = b.op(Op::JumpUnless { cond, to: 0 });
    let t0 = b.targets.len();
    lower_effect(l, b, then);
    let t1 = b.targets.len();
    match other {
        None => b.patch(j),
        Some(e) => {
            let end = b.op(Op::Jump { to: 0 });
            b.patch(j);
            b.reuse = Some(t0);
            b.reuse_failed = false;
            lower_effect(l, b, e);
            if b.reuse_failed || b.reuse != Some(t1) {
                l.unsupported("ConditionalIfVoid.targets(differ between branches)");
            }
            b.reuse = None;
            b.patch(end);
        }
    }
}

/// Lane C's effect classes; false if `e` is not one of them.
pub(crate) fn lower_effect_eoe_c(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // Void (an ability word): "<effect>. Void - if a nonland permanent
        // left the battlefield this turn or a spell was warped this turn,
        // instead <effect>" / "..., <more>".
        E::ConditionalIfVoid(nodes::ConditionalIfVoid { then_effect, else_effect }) => {
            if_else(l, b, Cond::Void, then_effect, else_effect.as_deref());
        }
        // "Sacrifice this unless a nonland permanent left the battlefield
        // this turn or a spell was warped this turn."
        E::SacrificeSelfUnlessVoidActive(nodes::SacrificeSelfUnlessVoidActive {}) => {
            let j = b.op(Op::JumpUnless { cond: Cond::Void, to: 0 });
            let end = b.op(Op::Jump { to: 0 });
            b.patch(j);
            b.op(Op::Ext(Ext::SacrificeSource));
            b.patch(end);
        }
        // "Each opponent discards a card" (CR 701.9a: of their choice).
        E::EachOpponentDiscards(nodes::EachOpponentDiscards { count }) => {
            let n = l.int("EachOpponentDiscards.count", *count, 1, 9) as u8;
            b.op(Op::Discard { who: Who::Opponent, n });
        }
        // "Each opponent discards a card. Each opponent who can't loses n
        // life." (Who can't has no card in hand: the same check before.)
        E::EachOpponentDiscardsOrLosesLife(nodes::EachOpponentDiscardsOrLosesLife { life_if_cant }) => {
            let life = l.int("EachOpponentDiscardsOrLosesLife.life_if_cant", *life_if_cant, 1, 99) as u8;
            b.op(Op::Ext(Ext::EoeC(EoeC::LoseLifeIfHandEmpty { who: Who::Opponent, n: life })));
            b.op(Op::Discard { who: Who::Opponent, n: 1 });
        }
        // "Each opponent mills n cards" (CR 701.13a).
        E::EachOpponentMillsCards(nodes::EachOpponentMillsCards { amount }) => {
            let n = l.int("EachOpponentMillsCards.amount", *amount, 1, 60);
            b.op(Op::Ext(Ext::Mill { who: Who::Opponent, n: Amount::Fixed(n) }));
        }
        // "Exile target creature" (its target spec as written).
        E::ExileTargetCreature(nodes::ExileTargetCreature { targets }) => {
            if let Some(t) = lower_targets(l, b, targets, "ExileTargetCreature") {
                b.op(Op::Exile { tgt: t });
            }
        }
        // "Exile target creature with power or toughness n or less": the
        // target spec says it; `max_value` must agree with the default the
        // spec was written for.
        E::ExileTargetCreatureWithPowerOrToughnessAtMost(nodes::ExileTargetCreatureWithPowerOrToughnessAtMost { targets, max_value }) => {
            let spec_n = l.int("ExileTargetCreatureWithPowerOrToughnessAtMost.max_value", *max_value, 0, 99);
            if let Some(t) = lower_targets(l, b, targets, "ExileTargetCreatureWithPowerOrToughnessAtMost") {
                let want = and(Filter::Type(types::CREATURE), Filter::Or(vec![Filter::PowerAtMost(spec_n), Filter::ToughnessAtMost(spec_n)]));
                if b.targets[t as usize].filter != want {
                    l.unsupported("ExileTargetCreatureWithPowerOrToughnessAtMost(target spec and max_value differ)");
                }
                b.op(Op::Exile { tgt: t });
            }
        }
        // "Exile [up to one] target creature [an opponent controls / you
        // control] [that isn't a <subtype>] until this leaves the
        // battlefield" (CR 610.3, a linked exile). One target: the class
        // has no target count (docs/ir_defects.md 60).
        E::ExileTargetCreatureUntilSourceLeaves(nodes::ExileTargetCreatureUntilSourceLeaves { excluded_subtypes, opponents_only, yours_only }) => {
            let mut f = Filter::Type(types::CREATURE);
            for s in excluded_subtypes {
                let k = l.subtype(s);
                f = and(f, not(Filter::Subtype(k)));
            }
            if *opponents_only && *yours_only {
                l.unsupported("ExileTargetCreatureUntilSourceLeaves(opponents_only and yours_only)");
            }
            if *opponents_only {
                f = and(f, Filter::Controller(false));
            }
            if *yours_only {
                f = and(f, Filter::Controller(true));
            }
            let t = b.target(f, 1, 1);
            b.op(Op::Ext(Ext::ExileLinked { tgt: t, until: true }));
        }
        // "Exile all graveyards" (every card in every graveyard, at once).
        E::ExileAllGraveyards(nodes::ExileAllGraveyards {}) => {
            b.op(Op::Ext(Ext::EoeC(EoeC::ExileAllGraveyards)));
        }
        // "Put target card from a graveyard on the bottom of its owner's
        // library" (CR 401.4: its owner's).
        E::PutCardFromGraveyardOnBottomOfLibrary(nodes::PutCardFromGraveyardOnBottomOfLibrary {}) => {
            let t = b.target(Filter::InGraveyard(Box::new(Filter::Any)), 1, 1);
            b.op(Op::Ext(Ext::EoeC(EoeC::GraveyardCardToBottom { tgt: t })));
        }
        // "Look at the top n cards of your library. Put one into your hand
        // [revealed] and the rest into your graveyard."
        E::LookTopNPutOneInHandRestToGraveyard(nodes::LookTopNPutOneInHandRestToGraveyard { n, reveal_chosen }) => {
            let n = l.int("LookTopNPutOneInHandRestToGraveyard.n", *n, 1, 20) as u8;
            b.op(Op::LookTop { who: Who::You, n, reveal: false });
            b.op(Op::PileTo { who: Who::You, dest: PileDest::Hand, filter: Filter::Any, min: 1, max: 1, rest: false, reveal: *reveal_chosen });
            b.op(Op::PileTo { who: Who::You, dest: PileDest::Graveyard, filter: Filter::Any, min: 0, max: 0, rest: true, reveal: false });
        }
        // Starwinder's "you may draw that many cards" (a combat damage
        // trigger: the event's damage). The class is that ability (its
        // docstring: the "may" only simplified away in Python).
        E::DrawCardsEqualToResolvingCombatDamage(nodes::DrawCardsEqualToResolvingCombatDamage {}) => {
            let at = b.op(Op::May { who: Who::You, skip: 0 });
            b.op(Op::Draw { who: Who::You, n: Amount::EventAmount });
            b.patch(at);
        }
        // "Shuffle your hand and graveyard into your library" (CR 701.20).
        E::ShuffleZonesIntoLibrary(nodes::ShuffleZonesIntoLibrary { zones }) => {
            use card_ir::enums::ZoneName;
            let (mut hand, mut graveyard) = (false, false);
            for z in zones {
                match z {
                    ZoneName::Hand => hand = true,
                    ZoneName::Graveyard => graveyard = true,
                    other => l.unsupported(format!("ShuffleZonesIntoLibrary.zones({other:?})")),
                }
            }
            if !hand && !graveyard {
                l.unsupported("ShuffleZonesIntoLibrary.zones(none)");
            }
            b.op(Op::ShuffleIntoLibrary { who: Who::You, hand, graveyard });
        }
        // "Draw a card for each multicolored permanent you control" (CR
        // 105.2a: two or more colours).
        E::DrawCardPerMulticoloredPermanent(nodes::DrawCardPerMulticoloredPermanent {}) => {
            b.op(Op::Draw { who: Who::You, n: Amount::MulticoloredPermanents });
        }
        // "Draw X cards, where X is the number of differently named lands you
        // control" (CR 201.2: names).
        E::DrawCardsByConditionMultiplier(nodes::DrawCardsByConditionMultiplier { condition, description_fragment }) => {
            let _ = description_fragment; // display text
            match &**condition {
                cats::Condition::PerDifferentlyNamedLand(nodes::PerDifferentlyNamedLand {}) => {
                    b.op(Op::Draw { who: Who::You, n: Amount::DifferentlyNamedLands });
                }
                other => l.unsupported(format!("DrawCardsByConditionMultiplier.condition({})", other.tag())),
            }
        }
        // "Exile the top X cards of your library, where X is one plus the
        // mana value of the sacrificed artifact. You may play those cards
        // this turn."
        E::ExileTopXOfLibraryMayPlayThisTurn(nodes::ExileTopXOfLibraryMayPlayThisTurn { amount }) => {
            let n = match amount.as_deref() {
                Some(cats::Formula::MVOfCostPaidObjectPlus(nodes::MVOfCostPaidObjectPlus { constant })) => {
                    if !l.sac_cost {
                        l.unsupported("Formula:MVOfCostPaidObjectPlus(no sacrifice cost)");
                    }
                    Amount::CostMvPlus(l.int("MVOfCostPaidObjectPlus.constant", *constant, 0, 20) as u8)
                }
                Some(other) => {
                    l.unsupported(format!("Formula:{}", other.tag()));
                    return true;
                }
                None => {
                    l.unsupported("ExileTopXOfLibraryMayPlayThisTurn.amount(missing)");
                    return true;
                }
            };
            b.op(Op::Ext(Ext::EoeC(EoeC::ExileTopPlayThisTurn { n })));
        }
        // "Each opponent exiles a card from their hand and may play that
        // card for as long as it remains exiled. Each spell cast this way
        // costs {1} more to cast. Each land played this way enters tapped."
        E::EachOpponentExilesCardFromHandWithCastPermission(nodes::EachOpponentExilesCardFromHandWithCastPermission {}) => {
            b.op(Op::Ext(Ext::EoeC(EoeC::ExileFromOwnHandPlayable { who: Who::Opponent, tax: 1 })));
        }
        _ => return false,
    }
    true
}

/// A spell's own cost reduction ("this spell costs {n} less to cast if
/// ...", CR 601.2f) of lane C, or `None` (recorded) for another.
pub(crate) fn cost_reduction(l: &mut Lowerer, c: &cats::Condition, n: u8) -> Option<Discount> {
    use cats::Condition as C;
    let cond = match c {
        // Void.
        C::VoidActiveCondition(nodes::VoidActiveCondition {}) => Cond::Void,
        // "If you've cast another spell this turn": the cost is determined
        // before the spell becomes cast (CR 601.2f before 601.2i), so the
        // spells cast so far are the others.
        C::AnotherSpellCastThisTurn(nodes::AnotherSpellCastThisTurn {}) => Cond::SpellsCastAtLeast(1),
        // "If your opponents control n or more <permanents>".
        C::OpponentsControlAtLeast(nodes::OpponentsControlAtLeast { filter, n: k }) => {
            let f = super::filter::lower_pred(l, filter)?;
            Cond::OpponentsControlAtLeast(f, l.int("OpponentsControlAtLeast.n", *k, 1, 99) as u8)
        }
        // "During your turn".
        C::DuringYourTurn(nodes::DuringYourTurn {}) => Cond::YourTurn,
        // "If you control a <permanent>".
        C::ControlPermanent(nodes::ControlPermanent { filter }) => Cond::ControlAtLeast(super::filter::lower_pred(l, filter)?, 1),
        // "If you attacked with a <creature> this turn": the attackers
        // declared this turn (CR 508.1), by their cards' characteristics.
        C::AttackedWithThisTurn(nodes::AttackedWithThisTurn { filter }) => match super::filter::lower_pred(l, filter)? {
            Filter::Subtype(k) => Cond::AttackedWithSubtype(k),
            _ => {
                l.unsupported("AttackedWithThisTurn.filter(not a subtype)");
                return None;
            }
        },
        other => {
            l.unsupported(format!("CostReduction.condition({})", other.tag()));
            return None;
        }
    };
    Some(Discount::If(cond, n))
}

/// A reduction a permanent gives the spells its controller casts
/// ("spells you cast ...", CR 601.2f) of lane C, or `None` (recorded).
pub(crate) fn spell_discount(l: &mut Lowerer, c: &cats::Condition, n: u8) -> Option<Discount> {
    use cats::Condition as C;
    match c {
        // "The nth spell you cast each turn costs {n} less to cast": as its
        // cost is determined, n - 1 spells were cast (CR 601.2f, 601.2i).
        C::NthSpellThisTurn(nodes::NthSpellThisTurn { n: k }) => {
            let k = l.int("NthSpellThisTurn.n", *k, 1, 99) as u8;
            Some(Discount::If(Cond::SpellsCastThisTurn { n: k - 1, noncreature: false }, n))
        }
        // "Spells you cast have affinity for <permanents>" (CR 702.41a: {1}
        // less for each one you control).
        C::PerControlled(nodes::PerControlled { filter }) => Some(Discount::PerControlled(super::filter::lower_pred(l, filter)?, n)),
        other => {
            l.unsupported(format!("ExternalCostReduction.condition({})", other.tag()));
            None
        }
    }
}
