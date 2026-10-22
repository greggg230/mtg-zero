//! Triggered abilities: events and trigger predicates -> [`TriggerDef`]
//! (CR 603; ENGINE.md 8).

use super::effect::lower_ability;
use super::filter::lower_pred;
use super::Lowerer;
use crate::db::{types, Cond, EventKind, Filter, Slot, TrigPred, TriggerDef};
use card_ir::{cats, enums, nodes};

pub(crate) fn lower_trigger(l: &mut Lowerer, t: &cats::TriggeredAbility, card_types: u16) -> Option<TriggerDef> {
    let cats::TriggeredAbility::TriggeredAbility(nodes::TriggeredAbility {
        effect,
        requires_void,
        once_per_turn,
        tier_threshold,
        event,
        predicate,
        room_half,
        functions_in_graveyard,
    }) = t;
    l.deny("TriggeredAbility.requires_void", requires_void);
    l.deny("TriggeredAbility.once_per_turn", once_per_turn);
    l.deny("TriggeredAbility.tier_threshold", tier_threshold);
    l.deny("TriggeredAbility.room_half", room_half);
    l.deny("TriggeredAbility.functions_in_graveyard", functions_in_graveyard);
    use enums::Event as E;
    let ev = match event {
        Some(E::Enters) => Some(EventKind::Enters),
        Some(E::Dies) => Some(EventKind::Dies),
        Some(E::LeavesBattlefield) => Some(EventKind::LeavesBattlefield),
        Some(E::Attacks) => Some(EventKind::Attacks),
        Some(E::YouAttack) => Some(EventKind::YouAttack),
        Some(E::DealsCombatDamage) => Some(EventKind::DealsCombatDamage),
        Some(E::IsCast) => Some(EventKind::IsCast),
        Some(E::DrawsCard) => Some(EventKind::DrawsCard),
        Some(E::BecomesTapped) => Some(EventKind::BecomesTapped),
        Some(E::IsSacrificed) => Some(EventKind::IsSacrificed),
        Some(E::BeginningOfUpkeep) => Some(EventKind::BeginningOfUpkeep),
        Some(E::BeginningOfDrawStep) => Some(EventKind::BeginningOfDrawStep),
        Some(E::BeginningOfPrecombatMain) => Some(EventKind::BeginningOfPrecombatMain),
        Some(E::BeginningOfCombat) => Some(EventKind::BeginningOfCombat),
        Some(E::BeginningOfEndStep) => Some(EventKind::BeginningOfEndStep),
        Some(E::DealtDamage) => Some(EventKind::DealtDamage),
        Some(other) => {
            l.unsupported(format!("Event:{other:?}"));
            None
        }
        None => {
            l.unsupported("TriggeredAbility.event(missing)");
            None
        }
    };
    let pred = match predicate.as_deref() {
        Some(p) => lower_trig_pred(l, p),
        None => {
            l.unsupported("TriggeredAbility.predicate(missing)");
            None
        }
    };
    let ability = match effect {
        Some(e) => lower_ability(l, e),
        None => {
            l.unsupported("TriggeredAbility.effect(missing)");
            return None;
        }
    };
    let (event, mut pred) = (ev?, pred?);
    // "When this creature dies" (CR 700.4): it was a creature as it left;
    // the IR's `dies` event covers any permanent put into a graveyard from
    // the battlefield, with typed subjects for the others.
    if event == EventKind::Dies && pred == TrigPred::IsSelf(Slot::Default) && card_types & types::CREATURE != 0 {
        pred = TrigPred::And(Box::new(pred), Box::new(TrigPred::Subject(Slot::Default, Filter::Type(types::CREATURE))));
    }
    if let Some(s) = slots_used(&pred).into_iter().find(|&s| event.slot_index(s).is_none()) {
        l.unsupported(format!("TriggerPredicate.subject({s:?} of {event:?})"));
    }
    let uses_event = ability.uses_event();
    Some(TriggerDef { event, pred, ability, uses_event })
}

/// Every subject slot a predicate names (checked against the event).
fn slots_used(p: &TrigPred) -> Vec<Slot> {
    match p {
        TrigPred::And(a, b) | TrigPred::Or(a, b) => {
            let mut v = slots_used(a);
            v.extend(slots_used(b));
            v
        }
        TrigPred::Not(a) => slots_used(a),
        TrigPred::IsSelf(s) | TrigPred::Subject(s, _) | TrigPred::Controlled(s, _) => vec![*s],
        TrigPred::Always | TrigPred::If(_) | TrigPred::State(_) => Vec::new(),
    }
}

fn slot(l: &mut Lowerer, s: &Option<enums::EventSubject>) -> Option<Slot> {
    use enums::EventSubject as S;
    Some(match s {
        None => Slot::Default,
        Some(S::Obj) => Slot::Obj,
        Some(S::Attacker) => Slot::Attacker,
        Some(S::Source) => Slot::Source,
        Some(S::Target) => Slot::Target,
        Some(S::Spell) => Slot::Spell,
        Some(S::Controller) => Slot::Controller,
        Some(S::Player) => Slot::Player,
        Some(other) => {
            l.unsupported(format!("TriggerPredicate.subject({other:?})"));
            return None;
        }
    })
}

fn lower_trig_pred(l: &mut Lowerer, p: &cats::TriggerPredicate) -> Option<TrigPred> {
    use cats::TriggerPredicate as P;
    let b = Box::new;
    match p {
        P::Always(nodes::Always {}) => Some(TrigPred::Always),
        P::And(nodes::And { left, right }) => {
            let a = lower_trig_pred(l, left);
            let c = lower_trig_pred(l, right);
            Some(TrigPred::And(b(a?), b(c?)))
        }
        P::Or(nodes::Or { left, right }) => {
            let a = lower_trig_pred(l, left);
            let c = lower_trig_pred(l, right);
            Some(TrigPred::Or(b(a?), b(c?)))
        }
        P::Not(nodes::Not { inner }) => Some(TrigPred::Not(b(lower_trig_pred(l, inner)?))),
        P::IsSelf(nodes::IsSelf { subject }) => Some(TrigPred::IsSelf(slot(l, subject)?)),
        P::IsThisSpell(nodes::IsThisSpell {}) => Some(TrigPred::IsSelf(Slot::Spell)),
        P::Subject(nodes::Subject { slot: s, inner }) => {
            let s = slot(l, s);
            let f = lower_pred(l, inner);
            Some(TrigPred::Subject(s?, f?))
        }
        P::Controlled(nodes::Controlled { by, subject }) => {
            let you = *by == enums::Controller::You;
            Some(TrigPred::Controlled(slot(l, subject)?, you))
        }
        P::StateBound(nodes::StateBound { inner }) => {
            let (c, intervening) = lower_state_pred(l, inner)?;
            Some(if intervening { TrigPred::If(c) } else { TrigPred::State(c) })
        }
        other => {
            l.unsupported(format!("TriggerPredicate:{}", other.tag()));
            None
        }
    }
}

/// A state predicate for the source's controller -> a [`Cond`], and
/// whether it is an intervening "if" (CR 603.4) rather than part of the
/// trigger event ("your second spell each turn").
pub(crate) fn lower_state_pred(l: &mut Lowerer, p: &cats::StatePredicate) -> Option<(Cond, bool)> {
    use cats::StatePredicate as S;
    Some(match p {
        S::CardsInGraveyardAtLeast(nodes::CardsInGraveyardAtLeast { n }) => {
            (Cond::GraveyardAtLeast(l.int("CardsInGraveyardAtLeast.n", *n, 0, 99) as u8), true)
        }
        S::Delirium(nodes::Delirium { min_card_types }) => {
            (Cond::Delirium(l.int("Delirium.min_card_types", *min_card_types, 1, 9) as u8), true)
        }
        S::GainedLifeThisTurn(nodes::GainedLifeThisTurn {}) => (Cond::GainedLifeThisTurn, true),
        S::OpponentHasMoreCardsInHand(nodes::OpponentHasMoreCardsInHand {}) => (Cond::OpponentHasMoreCards, true),
        S::YouControlAtLeast(nodes::YouControlAtLeast { n, card_type }) => {
            let Some(bits) = types::from_name(card_type) else {
                l.unsupported(format!("YouControlAtLeast.card_type({card_type})"));
                return None;
            };
            (Cond::ControlAtLeast(Filter::Type(bits), l.int("YouControlAtLeast.n", *n, 0, 99) as u8), true)
        }
        S::SpellsCastThisTurn(nodes::SpellsCastThisTurn { n }) => {
            let n = l.int("SpellsCastThisTurn.n", *n, 1, 99) as u8;
            (Cond::SpellsCastThisTurn { n, noncreature: false }, false)
        }
        S::NoncreatureSpellsCastThisTurn(nodes::NoncreatureSpellsCastThisTurn { n }) => {
            let n = l.int("NoncreatureSpellsCastThisTurn.n", *n, 1, 99) as u8;
            (Cond::SpellsCastThisTurn { n, noncreature: true }, false)
        }
        other => {
            l.unsupported(format!("StatePredicate:{}", other.tag()));
            return None;
        }
    })
}
