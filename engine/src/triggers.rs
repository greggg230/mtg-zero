//! Events and triggered abilities (CR 603; ENGINE.md 8).

use crate::db::{AbilityRef, AbilitySlot, EventKind, TrigFilter};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, StackItem, StackKind};
use crate::ids::{opp, DefId, ObjRef, PlayerId};

/// Something that happened, with last-known information for look-back
/// triggers (603.10a).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Event {
    Enters { obj: ObjRef, controller: PlayerId },
    Dies { old: ObjRef, def: DefId, controller: PlayerId },
}

/// A triggered ability waiting to be put on the stack (603.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PendingTrigger {
    pub controller: PlayerId,
    /// The source object (possibly gone: last known information).
    pub source: ObjRef,
    pub source_def: DefId,
    pub slot: u8,
}

impl Game {
    /// Record the triggers an event sets off (603.2). Only "this object"
    /// triggers exist yet; triggers of other permanents watching the event
    /// iterate the battlefield here later.
    pub(crate) fn event(&mut self, ev: Event) {
        let (kind, subject, def, controller) = match ev {
            Event::Enters { obj, controller } => (EventKind::Enters, obj, self.objs[obj.idx.i()].def, controller),
            Event::Dies { old, def, controller } => (EventKind::Dies, old, def, controller),
        };
        for (i, t) in self.db.def(def).triggers.iter().enumerate() {
            if t.event == kind && t.filter == TrigFilter::SelfObj {
                self.waiting.push(PendingTrigger { controller, source: subject, source_def: def, slot: i as u8 });
            }
        }
    }

    /// Put waiting triggers on the stack in APNAP order (603.3b): the active
    /// player's first, each player ordering their own; targets are chosen as
    /// each goes on the stack (603.3d).
    pub(crate) fn put_triggers(&mut self) {
        if self.waiting.is_empty() {
            return;
        }
        let ap = self.turn.active;
        let p = if self.waiting.iter().any(|t| t.controller == ap) { ap } else { opp(ap) };
        let mine: Vec<usize> = (0..self.waiting.len()).filter(|&i| self.waiting[i].controller == p).collect();
        let same = |a: &PendingTrigger, b: &PendingTrigger| a.source_def == b.source_def && a.slot == b.slot;
        let pick = match self.answer.take() {
            Some(Choice::Trigger(k)) => mine[k as usize],
            Some(other) => unreachable!("trigger answer {other:?}"),
            None => {
                let first = self.waiting[mine[0]];
                if mine.iter().all(|&i| same(&self.waiting[i], &first)) {
                    mine[0]
                } else {
                    // Identical triggers are offered once (their order cannot
                    // matter while their effects ignore the source's state).
                    let mut opts = Vec::new();
                    for (k, &i) in mine.iter().enumerate() {
                        if mine[..k].iter().any(|&j| same(&self.waiting[j], &self.waiting[i])) {
                            continue;
                        }
                        assert!((k as u16) < act::TRIGGER_N, "too many waiting triggers");
                        opts.push(Opt { action: act::TRIGGER + k as u16, choice: Choice::Trigger(k as u8) });
                    }
                    self.ask(Task::PutTriggers, p, DecisionKind::OrderTriggers, opts);
                    return;
                }
            }
        };
        let t = self.waiting.remove(pick);
        self.agenda.push(Task::PutTriggers);
        let ability = AbilityRef { def: t.source_def, slot: AbilitySlot::Trigger(t.slot), mode: None };
        let ab = self.db.ability(ability);
        if !self.targets_available(ab, t.controller, None) {
            self.note(|_| "trigger removed: no legal target".to_string());
            return; // 603.3d
        }
        let id = self.new_stack_id();
        self.stack.push(StackItem {
            id,
            kind: StackKind::Ability { ability, source: t.source },
            controller: t.controller,
            targets: Vec::new(),
            cost_option: 0,
            modes: Vec::new(),
        });
        self.note(|g| format!("trigger of {} goes on the stack", g.db.def(t.source_def).name));
        if ab.has_targets() {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
        }
        if ab.modes.is_some() {
            self.agenda.push(Task::ChooseModes { id }); // CR 700.2b, 603.3c
        }
    }
}
