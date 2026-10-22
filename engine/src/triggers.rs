//! Events and triggered abilities (CR 603; ENGINE.md 8).

use crate::db::{AbilityRef, AbilitySlot, EventKind, Filter, Slot, TrigPred};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, StackItem, StackKind};
use crate::ids::{opp, CardIdx, DefId, ObjRef, PlayerId, Target, Zone};

/// A subject of an event: an object (as it was when the event happened)
/// or a player.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Subj {
    None,
    Obj(ObjRef),
    Player(PlayerId),
}

/// Something that happened (603.2), with its subjects (`EventKind`
/// documents them) and an amount (damage dealt).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Event {
    pub kind: EventKind,
    pub a: Subj,
    pub b: Subj,
    pub amount: u16,
}

impl Event {
    pub(crate) fn obj(kind: EventKind, r: ObjRef) -> Event {
        Event { kind, a: Subj::Obj(r), b: Subj::None, amount: 0 }
    }
    pub(crate) fn player(kind: EventKind, p: PlayerId) -> Event {
        Event { kind, a: Subj::Player(p), b: Subj::None, amount: 0 }
    }
    fn subject(&self, s: Slot) -> Subj {
        match self.kind.slot_index(s) {
            Some(0) => self.a,
            Some(_) => self.b,
            None => Subj::None,
        }
    }
}

/// Last known information of an object that left the battlefield earlier
/// in the current simultaneous move: its leave-the-battlefield abilities
/// still see the others leave (CR 603.10a).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Lki {
    pub old: ObjRef,
    pub def: DefId,
    pub controller: PlayerId,
}

/// What a triggered ability remembers of its event for resolution: "that
/// player", "that creature", "that much" (CR 603.7c-like references).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct EvCtx {
    pub player: Option<PlayerId>,
    pub obj: Option<ObjRef>,
    pub amount: u16,
}

/// A triggered ability waiting to be put on the stack (603.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PendingTrigger {
    pub controller: PlayerId,
    /// The source object (possibly gone: last known information).
    pub source: ObjRef,
    pub source_def: DefId,
    pub slot: u8,
    /// The event's subjects and amount.
    pub ev: EvCtx,
}

/// An object whose abilities may trigger on an event.
#[derive(Clone, Copy)]
struct Observer {
    r: ObjRef,
    def: DefId,
    controller: PlayerId,
}

impl Game {
    /// Record the triggers an event sets off (603.2): abilities of
    /// permanents on the battlefield, of objects that left in the same
    /// simultaneous move (look back, 603.10a), and of a spell being cast
    /// ("when you cast this spell"). Leave-the-battlefield events are
    /// recorded before the object moves, so they see the game as it was.
    pub(crate) fn event(&mut self, ev: Event) {
        let bit = 1u32 << ev.kind as u32;
        let mut obs: Vec<Observer> = Vec::new();
        for &c in &self.battlefield {
            let o = &self.objs[c.i()];
            if self.db.def(o.def).trig_mask & bit != 0 {
                obs.push(Observer { r: ObjRef { idx: c, inc: o.inc }, def: o.def, controller: o.controller });
            }
        }
        if matches!(ev.kind, EventKind::Dies | EventKind::LeavesBattlefield | EventKind::IsSacrificed) {
            for l in &self.leaving {
                if self.db.def(l.def).trig_mask & bit != 0 {
                    obs.push(Observer { r: l.old, def: l.def, controller: l.controller });
                }
            }
        }
        if ev.kind == EventKind::IsCast {
            if let Subj::Obj(r) = ev.a {
                let o = &self.objs[r.idx.i()];
                if self.db.def(o.def).trig_mask & bit != 0 {
                    obs.push(Observer { r, def: o.def, controller: o.controller });
                }
            }
        }
        if obs.is_empty() {
            return;
        }
        let ctx = EvCtx {
            player: [ev.a, ev.b].iter().find_map(|s| match s {
                Subj::Player(p) => Some(*p),
                _ => None,
            }),
            obj: [ev.a, ev.b].iter().find_map(|s| match s {
                Subj::Obj(r) => Some(*r),
                _ => None,
            }),
            amount: ev.amount,
        };
        for o in obs {
            for (i, t) in self.db.def(o.def).triggers.iter().enumerate() {
                if t.event == ev.kind && self.pred_matches(&t.pred, &ev, o) {
                    self.waiting.push(PendingTrigger {
                        controller: o.controller,
                        source: o.r,
                        source_def: o.def,
                        slot: i as u8,
                        ev: ctx,
                    });
                }
            }
        }
    }

    fn pred_matches(&self, p: &TrigPred, ev: &Event, src: Observer) -> bool {
        match p {
            TrigPred::Always => true,
            TrigPred::And(a, b) => self.pred_matches(a, ev, src) && self.pred_matches(b, ev, src),
            TrigPred::Or(a, b) => self.pred_matches(a, ev, src) || self.pred_matches(b, ev, src),
            TrigPred::Not(a) => !self.pred_matches(a, ev, src),
            TrigPred::IsSelf(s) => match ev.subject(*s) {
                Subj::Obj(r) => r == src.r,
                // "At the beginning of your upkeep": the source's own
                // event (the IR's IsSelf on a player event).
                Subj::Player(_) => *s == Slot::Default,
                Subj::None => false,
            },
            TrigPred::Subject(s, f) => {
                let ctx = crate::cast::Ctx { controller: src.controller, source: Some(src.r) };
                match ev.subject(*s) {
                    Subj::Obj(r) => self.live(r) && self.subject_matches(f, r.idx, &ctx),
                    Subj::Player(q) => self.filter_ok(f, Target::Player(q), &ctx),
                    Subj::None => false,
                }
            }
            TrigPred::Controlled(s, you) => {
                let who = match ev.subject(*s) {
                    Subj::Obj(r) => self.objs[r.idx.i()].controller,
                    Subj::Player(q) => q,
                    Subj::None => return false,
                };
                (who == src.controller) == *you
            }
            TrigPred::If(c) | TrigPred::State(c) => self.cond_holds(c, src.controller),
            TrigPred::FormerHost(s) => match ev.subject(*s) {
                // Evaluated as the host leaves (look back): the Equipment
                // is still attached to it.
                Subj::Obj(r) => self.objs[src.r.idx.i()].attached == Some(r),
                _ => false,
            },
        }
    }

    /// A subject filter on an object in any public zone (a spell being
    /// cast, a permanent): its characteristics.
    fn subject_matches(&self, f: &Filter, c: CardIdx, ctx: &crate::cast::Ctx) -> bool {
        match self.objs[c.i()].zone {
            Zone::Battlefield | Zone::Stack => self.card_matches(f, c, ctx),
            _ => false,
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
        // Triggers of the same ability whose effects ignore the event's
        // subjects are interchangeable.
        let db = self.db;
        let same = |a: &PendingTrigger, b: &PendingTrigger| {
            a.source_def == b.source_def
                && a.slot == b.slot
                && (a.ev == b.ev || !db.def(a.source_def).triggers[a.slot as usize].uses_event)
        };
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
        let t = self.waiting[pick];
        self.agenda.push(Task::PutTriggers);
        let ability = AbilityRef { def: t.source_def, slot: AbilitySlot::Trigger(t.slot), mode: None };
        let ab = self.db.ability(ability);
        // (Checked while the trigger still waits: its source's last known
        // information is found through it.)
        let ok = self.targets_available(ab, t.controller, Some(t.source));
        self.waiting.remove(pick);
        if !ok {
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
            ev: t.ev,
        });
        self.note(|g| format!("trigger of {} goes on the stack", g.db.def(t.source_def).name));
        if ab.has_targets() {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
        }
        if ab.modes.is_some() {
            self.agenda.push(Task::ChooseModes { id }); // CR 700.2b, 603.3c
        }
    }

    /// CR 603.4: a triggered ability with an intervening "if" does nothing
    /// on resolution unless the condition still holds.
    pub(crate) fn intervening_holds(&self, ability: AbilityRef, controller: PlayerId) -> bool {
        let AbilitySlot::Trigger(i) = ability.slot else { return true };
        let t = &self.db.def(ability.def).triggers[i as usize];
        t.pred.intervening().into_iter().all(|c| self.cond_holds(c, controller))
    }

    /// Start a simultaneous move of several objects off the battlefield
    /// (SBAs, "destroy all", a cost that sacrifices several): each one's
    /// leave abilities also see the others leave (603.10a).
    pub(crate) fn begin_batch(&mut self) {
        self.leaving.clear();
        self.batching = true;
    }

    pub(crate) fn end_batch(&mut self) {
        self.leaving.clear();
        self.batching = false;
    }
}
