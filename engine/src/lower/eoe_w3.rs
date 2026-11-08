//! Edge of Eternities, workstream W3 (ENGINE.md 99.13): combat and
//! abilities. The costs, effect classes and printing flags of the set's last
//! combat cards. Same rules as `effect.rs`: every field named, the rest
//! denied. Most classes are named after a card and carry no fields (their
//! meaning is the class's Oracle text, mtg-bot's docstring;
//! docs/ir_defects.md 11): each lowers onto ops named after rules actions,
//! its targets derived here.

use super::Lowerer;
use crate::db::{types, Cost, Counter, EventKind, Filter, Slot, TrigPred};
use crate::eoe_w3::W3;
use crate::program::{Amount, Builder, Ext, On, Op, Who};
use card_ir::{cats, nodes};

/// The cost atoms of `ActivationCost` this workstream implements.
pub(crate) struct CostW3 {
    pub exile_artifact_from_graveyard: bool,
    pub remove_plus_one_counter_from_your_creature: bool,
    pub tap_x_untapped_artifacts_you_control: bool,
}

/// Lower the cost atoms of [`CostW3`] into `cost`.
pub(crate) fn lower_cost_w3(l: &mut Lowerer, cost: &mut Cost, c: CostW3) {
    let CostW3 { exile_artifact_from_graveyard, remove_plus_one_counter_from_your_creature, tap_x_untapped_artifacts_you_control } = c;
    // "Exile an artifact card from your graveyard" (CR 118.3, 602.2b: a card
    // chosen as the cost is paid; its printed types, 400.7).
    if exile_artifact_from_graveyard {
        if cost.exile_graveyard > 0 {
            l.unsupported("ActivationCost.exile_artifact_from_graveyard(with another graveyard exile)");
        }
        cost.exile_graveyard = 1;
        cost.exile_graveyard_types = types::ARTIFACT;
    }
    // "Remove a +1/+1 counter from a creature you control" (CR 118.3,
    // 602.2b).
    if remove_plus_one_counter_from_your_creature {
        if cost.crew > 0 {
            l.unsupported("ActivationCost.remove_plus_one_counter_from_your_creature(with crew)");
        }
        cost.remove_counter_from = Some((Filter::Type(types::CREATURE), Counter::P1P1));
    }
    // "Tap X untapped artifacts you control" (CR 107.3a, 601.2b: X is
    // announced as the ability is activated; the effect must read it).
    if tap_x_untapped_artifacts_you_control {
        if l.act_x || cost.crew > 0 {
            l.unsupported("ActivationCost.tap_x_untapped_artifacts_you_control(with {X} or crew)");
        }
        l.act_x = true;
        cost.tap_x = Some(Filter::Type(types::ARTIFACT));
    }
}

fn w3(b: &mut Builder, op: W3) -> usize {
    b.op(Op::Ext(Ext::EoeW3(op)))
}

/// `op` (a destruction), with "it can't be regenerated" if `no_regen` (CR
/// 701.19c: the flag is set around it).
pub(crate) fn cant_be_regenerated(b: &mut Builder, no_regen: bool, op: Op) {
    if no_regen {
        w3(b, W3::CantBeRegenerated(true));
    }
    b.op(op);
    if no_regen {
        w3(b, W3::CantBeRegenerated(false));
    }
}

/// Lower an effect class of workstream W3; `false` if the class is not one
/// of these.
pub(crate) fn lower_effect_w3(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Regenerate target Insect, Rat, Spider, or Squirrel" (CR 701.19a;
        // the target: a permanent with one of the subtypes).
        E::RegenerateTargetCreature(nodes::RegenerateTargetCreature { allowed_subtypes }) => {
            let filter = if allowed_subtypes.is_empty() {
                Filter::Type(types::CREATURE)
            } else {
                Filter::Or(allowed_subtypes.iter().map(|s| Filter::Subtype(l.subtype(s))).collect())
            };
            let t = b.target(filter, 1, 1);
            w3(b, W3::Regenerate { on: On::Target(t) });
        }
        // "Until end of turn, target creature gains '<triggered ability>'"
        // (CR 613.1f; Requiem Monolith).
        E::GrantTriggeredAbilityUEOT(nodes::GrantTriggeredAbilityUEOT { ability, you_control_only, opponents_only }) => {
            let Some(ta) = ability.as_deref() else {
                l.unsupported("GrantTriggeredAbilityUEOT.ability(missing)");
                return true;
            };
            let mut f = Filter::Type(types::CREATURE);
            if *you_control_only {
                f = Filter::And(Box::new(f), Box::new(Filter::Controller(true)));
            }
            if *opponents_only {
                f = Filter::And(Box::new(f), Box::new(Filter::Controller(false)));
            }
            let t = b.target(f, 1, 1);
            grant_triggered_ability(l, b, On::Target(t), ta, true, types::CREATURE);
        }
        // "You draw that many cards and lose that much life" (a "whenever
        // this is dealt damage" ability: the event's amount, CR 120.3;
        // "you": the ability's controller).
        E::DrawCardsAndLoseLifeEqualToResolvingDamage(nodes::DrawCardsAndLoseLifeEqualToResolvingDamage {}) => {
            b.op(Op::Draw { who: Who::You, n: Amount::EventAmount });
            b.op(Op::LoseLife { who: Who::You, n: Amount::EventAmount });
        }
        // "That creature's controller may have <this> deal n damage to it"
        // (the target of the sentence before, the ability's only one; the
        // creature's controller chooses, CR 608.2d; the source deals it,
        // as it last existed if it left, 608.2h).
        E::MayHaveSourceDealDamageToTarget(nodes::MayHaveSourceDealDamageToTarget { amount }) => {
            if b.targets.len() != 1 {
                l.unsupported("MayHaveSourceDealDamageToTarget(not one target before it)");
                return true;
            }
            let n = l.int("MayHaveSourceDealDamageToTarget.amount", *amount, 1, 20);
            let no = b.op(Op::May { who: Who::ControllerOfTarget(0), skip: 0 });
            b.op(Op::DealDamage { amount: Amount::Fixed(n), to: 0 });
            b.patch(no);
        }
        // "You may put an artifact or creature card from your hand onto the
        // battlefield. That permanent gains haste, 'When this permanent leaves
        // the battlefield, it deals damage equal to its mana value to each
        // creature,' and 'At the beginning of your end step, sacrifice this
        // permanent.'" (Terminal Velocity; no duration: for as long as it is
        // that permanent, CR 611.2a, 400.7.)
        E::TerminalVelocityEffect(nodes::TerminalVelocityEffect {}) => {
            let f = Filter::Or(vec![Filter::Type(types::ARTIFACT), Filter::Type(types::CREATURE)]);
            b.op(Op::PutFromHand { who: Who::You, filter: f, tapped: false, optional: true });
            let mut haste = crate::db::KwSet::default();
            haste.insert(crate::db::Kw::Haste);
            b.op(Op::Ext(Ext::Apply { on: On::Made, mods: vec![crate::db::Mod::Keywords(haste)], until: crate::layers::Until::Forever }));
            let mut d = Builder::default();
            w3(&mut d, W3::DamageEachSourceMv { filter: Filter::Type(types::CREATURE) });
            let leaves = d.finish();
            grant_trigger_def(l, b, On::Made, EventKind::LeavesBattlefield, TrigPred::IsSelf(Slot::Default), leaves, false);
            let mut d = Builder::default();
            d.op(Op::Ext(Ext::SacrificeSource));
            let sac = d.finish();
            grant_trigger_def(l, b, On::Made, EventKind::BeginningOfEndStep, TrigPred::Controlled(Slot::Player, true), sac, false);
        }
        // "When this Aura enters, enchanted creature deals damage equal to
        // its power to any other target" (Pain for All; CR 115.4 "any
        // target", other than that creature; it is the source, 120.3).
        E::PainForAllETBEffect(nodes::PainForAllETBEffect {}) => {
            let any = Filter::Or(vec![Filter::Type(types::CREATURE), Filter::Type(types::PLANESWALKER), Filter::Type(types::BATTLE), Filter::Player]);
            let t = b.target(Filter::And(Box::new(any), Box::new(Filter::Not(Box::new(Filter::HostOfSource)))), 1, 1);
            w3(b, W3::HostDamagesPower { tgt: t });
        }
        // "Whenever enchanted creature is dealt damage, it deals that much
        // damage to each opponent" (Pain for All; CR 120.3: that creature is
        // the source).
        E::PainForAllDamageEchoEffect(nodes::PainForAllDamageEchoEffect {}) => {
            w3(b, W3::EventObjDamagesEachOpponent { amount: Amount::EventAmount });
        }
        _ => return false,
    }
    true
}

/// "<Objects of `on`> gain '<a triggered ability on `event` with `pred`
/// doing `ability`>'" (CR 613.1f), as [`grant_triggered_ability`] for an
/// ability the class names in its words.
fn grant_trigger_def(l: &mut Lowerer, b: &mut Builder, on: On, event: EventKind, pred: TrigPred, ability: crate::db::AbilityDef, this_turn: bool) {
    let uses_event = ability.uses_event();
    l.delayed.push(crate::db::TriggerDef { event, pred, ability, uses_event });
    let k = (l.delayed.len() - 1) as u8;
    w3(b, W3::GrantTrigger { on, k, this_turn });
}

/// "<Objects of `on`> gain '<triggered ability `t`>'" (CR 613.1f: layer 6),
/// until end of turn (`this_turn`) or for good: the ability is a delayed
/// trigger of the card (it triggers only as granted), its event and
/// predicate lowered as a printed one's (`card_types`: what the granted
/// object is, for "when this creature dies"). EoE W3's general mechanism
/// (ENGINE.md 99.13).
pub(crate) fn grant_triggered_ability(l: &mut Lowerer, b: &mut Builder, on: On, t: &cats::TriggeredAbility, this_turn: bool, card_types: u16) {
    let cats::TriggeredAbility::TriggeredAbility(nodes::TriggeredAbility { once_per_turn, functions_in_graveyard, .. }) = t;
    // (Granted to a permanent; "only once each turn" would share the
    // object's own abilities' once-a-turn record.)
    l.deny("granted_triggered_ability.functions_in_graveyard", functions_in_graveyard);
    l.deny("granted_triggered_ability.once_per_turn", once_per_turn);
    let Some(td) = super::trigger::lower_trigger(l, t, card_types) else { return };
    if td.event == crate::db::EventKind::Delayed {
        l.unsupported("granted_triggered_ability(a delayed event)");
        return;
    }
    l.delayed.push(td);
    let k = (l.delayed.len() - 1) as u8;
    w3(b, W3::GrantTrigger { on, k, this_turn });
}
