//! Edge of Eternities, W6 (ENGINE.md 99.16): the lowering of the set's
//! remaining cards. Same rules as `effect.rs`: every field named, the rest
//! denied; each lowers onto ops named after rules actions.

use super::Lowerer;
use crate::eoe_w6::W6;
use crate::program::{Builder, Ext, Op, SpellDest};
use card_ir::cats;

/// "Counter target spell. If the amount of mana spent to cast that spell
/// was less than its mana value, <effect>" (Unravel; CR 601.2h, 202.3): the
/// condition is read just before the counter (the spell's last known
/// information on the stack is the same, 608.2h), the effect follows the
/// counter either way it went (a spell that can't be countered is still
/// "that spell").
pub(crate) fn counter_then_if_spent_below_mv(l: &mut Lowerer, b: &mut Builder, tgt: u8, dest: SpellDest, effect: &cats::Effect) {
    let check = b.op(Op::Ext(Ext::EoeW6(W6::JumpUnlessSpentBelowMv { tgt, to: 0 })));
    b.op(Op::Counter { tgt, dest });
    super::effect::lower_effect(l, b, effect);
    let end = b.op(Op::Jump { to: 0 });
    b.patch(check);
    b.op(Op::Counter { tgt, dest });
    b.patch(end);
}

/// "Artifacts you control have '{T}: Add {U}. Spend this mana only to cast
/// a spell from anywhere other than your hand.'" (Mm'menon, the Right Hand;
/// CR 613.1f, 605.1a, 106.6): the granted mana ability is a latent ability of
/// the card (`CardDef::latent`: it never has it itself), given to each
/// artifact its controller controls by W3's grant (`W3Def::grants_activated`,
/// `Game::granted_activated`: in layer 6, at the granter's timestamp).
pub(crate) fn printing_w6_grants(l: &mut Lowerer, def: &mut crate::db::CardDef, blue_to_artifacts: bool) {
    use crate::db::{ActEffect, ActivatedDef, Cost, Filter, ManaAbility, ManaProd};
    if !blue_to_artifacts {
        return;
    }
    let ability = ActivatedDef {
        cost: Cost { tap: true, ..Cost::default() },
        effect: ActEffect::Mana(ManaAbility {
            prod: ManaProd::Fixed([0, 1, 0, 0, 0, 0]),
            damage_to_you: 0,
            sac_if_no: None,
            only: crate::db::SPEND_OUTSIDE_HAND,
            uncounterable: false,
            haste: false,
            opp_life: 0,
        }),
        sorcery: false,
        instant_only: false,
        exhaust: false,
        only_if: None,
        your_turn: false,
        loyalty: None,
        discounts: Vec::new(),
        boast: false,
        x: false,
        special: None,
    };
    let yours = Filter::And(Box::new(Filter::Type(crate::db::types::ARTIFACT)), Box::new(Filter::Controller(true)));
    let k = def.activated.len();
    def.activated.push(ability);
    def.latent += 1;
    def.w3.grants_activated.push((yours, k as u8));
    // (A granted mana ability as a payment source and at priority is
    // workstream B2's payment work: refused until it is in.)
    l.unsupported("CardPrinting.grants_blue_mana_ability_to_your_artifacts(a granted mana ability as a payment source: workstream B2)");
}

/// Modular's triggered ability (CR 702.43a): "When this permanent is put
/// into a graveyard from the battlefield, you may put a +1/+1 counter on
/// target artifact creature for each +1/+1 counter on this permanent" (the
/// counters it had as it left, CR 608.2h, 122.8; whatever its card types
/// then, Power Depot's ruling). One per instance (702.43b).
pub(crate) fn modular_trigger() -> crate::db::TriggerDef {
    use crate::db::{types, EventKind, Filter, Slot, TrigPred};
    let mut b = Builder::default();
    let t = b.target(Filter::And(Box::new(Filter::Type(types::ARTIFACT)), Box::new(Filter::Type(types::CREATURE))), 1, 1);
    let may = b.op(Op::May { who: crate::program::Who::You, skip: 0 });
    b.op(Op::Ext(Ext::PutCounters {
        on: crate::program::On::Target(t),
        counter: crate::db::Counter::P1P1,
        n: crate::program::Amount::SourceCounters(crate::db::Counter::P1P1),
    }));
    b.patch(may);
    crate::db::TriggerDef { event: EventKind::Dies, pred: TrigPred::IsSelf(Slot::Default), ability: b.finish(), uses_event: false }
}

/// A replacement effect of a static ability (CR 614.1a) that W6 lowers into
/// `repl`; `false` if it is not one of these (the caller goes on).
pub(crate) fn lower_replacement_w6(
    l: &mut Lowerer,
    event: &card_ir::enums::Event,
    transform: &cats::EventTransform,
    subject_filters: &[(String, Box<cats::ObjectPredicate>)],
    affects: &card_ir::enums::AffectedPlayers,
    condition: Option<&cats::Condition>,
    repl: &mut super::statics::Repl,
) -> bool {
    use card_ir::enums;
    match (event, transform) {
        // "If one or more counters would be put on a creature, Spacecraft,
        // or Planet you control, twice that many of each of those kinds of
        // counters are put on it instead" (Loading Zone; CR 614.1a, 122.6):
        // every kind of counter; "you control" is `affects` (the recipient's
        // controller).
        (enums::Event::WouldPlaceCounter, cats::EventTransform::MultiplyCount(card_ir::nodes::MultiplyCount { factor })) => {
            if *factor != 2 {
                l.unsupported(format!("ReplacementEffect(WouldPlaceCounter, MultiplyCount {factor})"));
            }
            if condition.is_some() || *affects != enums::AffectedPlayers::You {
                l.unsupported("ReplacementEffect(WouldPlaceCounter, conditional or not yours)");
            }
            match subject_filters {
                [(s, p)] if s == "obj" => {
                    if let Some(f) = super::filter::lower_pred(l, p) {
                        repl.w6_counter_doubler = Some(f);
                    }
                }
                _ => l.unsupported("ReplacementEffect(WouldPlaceCounter, recipients other than one object filter)"),
            }
            true
        }
        // "If a permanent entering the battlefield causes a triggered
        // ability of a permanent you control to trigger, that ability
        // triggers an additional time" (Starfield Vocalist; CR 603.2d): the
        // ability's source a permanent ("you control" is `affects`), the
        // trigger caused by a permanent entering.
        (enums::Event::TriggeredAbilityTriggers, cats::EventTransform::AddInstances(card_ir::nodes::AddInstances { delta })) => {
            if *delta != 1 {
                l.unsupported(format!("ReplacementEffect(TriggeredAbilityTriggers, AddInstances {delta})"));
            }
            if condition.is_some() || *affects != enums::AffectedPlayers::You {
                l.unsupported("ReplacementEffect(TriggeredAbilityTriggers, conditional or not yours)");
            }
            let permanent = |l: &mut Lowerer, p: &cats::ObjectPredicate| {
                super::filter::lower_pred(l, p) == Some(crate::db::Filter::Type(crate::db::types::PERMANENT))
            };
            let ok = match subject_filters {
                [(a, pa), (b, pb)] => a == "ability_source" && b == "entering_obj" && permanent(l, pa) && permanent(l, pb),
                _ => false,
            };
            if !ok {
                l.unsupported("ReplacementEffect(TriggeredAbilityTriggers, not a permanent's ability that a permanent entering causes)");
            }
            repl.w6_trigger_doubler = true;
            true
        }
        _ => false,
    }
}

/// Lower a W6 effect class; `false` if the class is not one of these.
pub(crate) fn lower_effect_eoe_w6(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use card_ir::nodes;
    use cats::Effect as E;
    let _ = &l;
    match e {
        // "When this Aura enters, tap enchanted permanent." (Tractor Beam.)
        E::TapEnchantedPermanent(nodes::TapEnchantedPermanent {}) => {
            b.op(Op::Ext(Ext::EoeW6(W6::TapHost)));
        }
        // "You may draw that many cards" (Terrasymbiosis: the number of
        // +1/+1 counters its trigger's event put, the event's amount; the
        // class's reading, as Starwinder's combat-damage sibling).
        E::DrawCardsEqualToResolvingCounterAmount(nodes::DrawCardsEqualToResolvingCounterAmount {}) => {
            let at = b.op(Op::May { who: crate::program::Who::You, skip: 0 });
            b.op(Op::Draw { who: crate::program::Who::You, n: crate::program::Amount::EventAmount });
            b.patch(at);
        }
        // "{2}, {T}: This land becomes a copy of target land, except it has
        // this ability" (Thespian's Stage: the class's meaning; its target,
        // a land, re-derived as docs/ir_defects.md 11 says).
        E::BecomesCopyOfTargetLand(nodes::BecomesCopyOfTargetLand {}) => {
            let tgt = b.target(crate::db::Filter::Type(crate::db::types::LAND), 1, 1);
            b.op(Op::Ext(Ext::EoeW6(W6::BecomeCopyKeeping { tgt })));
        }
        // "Move a counter from target permanent you control onto a second
        // target permanent" (Nesting Grounds; CR 122.5): two targets, the
        // second another (115.3).
        E::MoveCounterBetweenTargets(nodes::MoveCounterBetweenTargets { targets }) => {
            if let Some((from, to)) = super::effect_copies::two_targets(l, b, targets, "MoveCounterBetweenTargets") {
                b.op(Op::Ext(Ext::EoeW6(W6::MoveCounter { from, to })));
            }
        }
        _ => return false,
    }
    true
}
