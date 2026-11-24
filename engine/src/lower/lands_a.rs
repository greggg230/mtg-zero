//! The Legacy Lands deck's cards, lane A (ENGINE.md 270.1): the lowering of
//! Dark Depths' state trigger ("When Dark Depths has no ice counters on it,
//! sacrifice it. If you do, create Marit Lage, ..."), Bojuka Bog's "exile
//! target player's graveyard" and Maze of Ith's "untap target attacking
//! creature. Prevent all combat damage that would be dealt to and dealt by
//! that creature this turn". Same rules as `effect.rs`: every field named,
//! the rest denied; each lowers onto ops named after rules actions. (Dark
//! Depths' ice counters are `CardPrinting.enters_with_named_counters`, its
//! "{3}: Remove an ice counter" `lower/hogaak_c.rs`' `RemoveCountersFromSource`;
//! Ghost Quarter's rider is `lower/effect.rs`' `RemoveTargetPermanents`;
//! Thespian's Stage and Blast Zone lower through EoE W6 and W2.)

use super::effect::lower_ability;
use super::Lowerer;
use crate::db::{EventKind, Filter, Mod, TrigPred, TriggerDef};
use crate::layers::Until;
use crate::program::{Builder, Ext, On, Op, Who};
use card_ir::{cats, nodes};

/// A card's state-triggered abilities (CR 603.8; the IR's `board_rules`):
/// "When <its source has no <kind> counters on it>, <effect>" -> a
/// [`TriggerDef`] of [`EventKind::State`] whose predicate is its source's
/// condition (`TrigPred::Source`), checked by `Game::check_state_triggers`.
pub(crate) fn lower_board_rules(l: &mut Lowerer, rules: &[cats::StateTrigger]) -> Vec<TriggerDef> {
    let mut out = Vec::new();
    for r in rules {
        let cats::StateTrigger::StateTrigger(nodes::StateTrigger { condition, ability }) = r;
        let cats::HasNoCountersOfKind::HasNoCountersOfKind(nodes::HasNoCountersOfKind { kind }) = &**condition;
        if kind.is_empty() {
            l.unsupported("HasNoCountersOfKind.kind(empty)");
            continue;
        }
        let counter = super::effect_ext::counter_by_name(l, kind);
        let cats::TriggeredAbility::TriggeredAbility(nodes::TriggeredAbility {
            effect,
            requires_void,
            once_per_turn,
            tier_threshold,
            event,
            predicate,
            room_half,
            functions_in_graveyard,
        }) = &**ability;
        // (A state trigger has no event; the condition is the rule's.)
        l.deny("StateTrigger.ability.requires_void", requires_void);
        l.deny("StateTrigger.ability.once_per_turn", once_per_turn);
        l.deny("StateTrigger.ability.tier_threshold", tier_threshold);
        l.deny("StateTrigger.ability.event", event);
        l.deny("StateTrigger.ability.predicate", predicate);
        l.deny("StateTrigger.ability.room_half", room_half);
        l.deny("StateTrigger.ability.functions_in_graveyard", functions_in_graveyard);
        let Some(e) = effect else {
            l.unsupported("StateTrigger.ability.effect(missing)");
            continue;
        };
        let ability = lower_ability(l, e);
        if ability.uses_event() {
            l.unsupported("StateTrigger.ability.effect(reads an event)");
        }
        out.push(TriggerDef { event: EventKind::State, pred: TrigPred::Source(Filter::NoCounters(counter)), ability, uses_event: false });
    }
    out
}

pub(crate) fn lower_effect_lands_a(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Sacrifice it. If you do, <then>" (Dark Depths' state trigger: "it"
        // is the ability's source, CR 701.21a: only if it is still that
        // permanent and its controller is the ability's).
        E::SacrificeSourceThen(nodes::SacrificeSourceThen { then_effect }) => {
            let at = b.op(Op::Ext(Ext::SacrificeSourceThen { skip: 0 }));
            if let Some(t) = then_effect {
                super::effect::lower_effect(l, b, t);
            }
            b.patch(at);
        }
        // "Exile target player's graveyard" (Bojuka Bog; CR 406.2): any
        // player, its controller too.
        E::ExileTargetPlayersGraveyard(nodes::ExileTargetPlayersGraveyard {}) => {
            let t = b.target(Filter::Player, 1, 1);
            b.op(Op::Ext(Ext::ExileGraveyard { who: Who::TargetPlayer(t) }));
        }
        // "Untap target attacking creature. Prevent all combat damage that
        // would be dealt to and dealt by that creature this turn" (Maze of
        // Ith; CR 615.1a, 510.1): one prevention effect on that object, for
        // the turn (514.2).
        E::PreventCombatDamageToAndByTarget(nodes::PreventCombatDamageToAndByTarget { targets, untap }) => {
            let Some(t) = super::effect::lower_targets(l, b, targets, "PreventCombatDamageToAndByTarget") else { return true };
            if *untap {
                b.op(Op::Untap { tgt: t });
            }
            b.op(Op::Ext(Ext::Apply { on: On::Target(t), mods: vec![Mod::PreventCombatDamageToAndBy], until: Until::EndOfTurn }));
        }
        _ => return false,
    }
    true
}
