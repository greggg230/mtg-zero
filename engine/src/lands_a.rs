//! The Legacy Lands deck's cards, lane A (ENGINE.md 270.1): state-triggered
//! abilities (CR 603.8; Dark Depths' "When Dark Depths has no ice counters
//! on it, sacrifice it. If you do, create Marit Lage, ...").
//!
//! CR 603.8: "A state-triggered ability triggers whenever the game state
//! matches a given condition. ... A state-triggered ability doesn't trigger
//! again until the ability has resolved, has been countered, or has
//! otherwise left the stack. Then, if the object with the ability is still
//! in the same zone and the game state still matches its trigger condition,
//! the ability will trigger again." The engine checks the conditions every
//! time it would put triggered abilities on the stack (`Game::sba_check`,
//! CR 117.5) and as the cleanup step looks for anything to do (514.3a); an
//! instance of the ability that triggered and hasn't left the stack yet is
//! one waiting (`Game::waiting`) or on the stack from that object, so no
//! record of its own is kept: an instance countered (Consign to Memory)
//! leaves the stack, and the next check triggers it again (the ruling).
//! Readings (ENGINE.md 270.1): no condition of the tables can become true and
//! then false again between two checks (a counter of the kind is only
//! removed during a resolution, a copy effect only begins or ends at one),
//! so checking at those points is exact; "the ability" is the object's
//! state trigger with the same text (a Thespian's Stage that became a copy
//! of Dark Depths again has the same ability).

use crate::db::{EventKind, TrigPred};
use crate::game::{Game, StackKind};
use crate::ids::{CardIdx, ObjRef};
use crate::triggers::{EvCtx, PendingTrigger};

impl Game {
    /// CR 603.8: every state-triggered ability whose condition holds and
    /// that hasn't triggered already (one waiting, or on the stack, from
    /// the same object with the same text) triggers: it waits to be put on
    /// the stack (603.3) with the other triggered abilities.
    pub(crate) fn check_state_triggers(&mut self) {
        if !self.db.state_triggers {
            return;
        }
        let bit = 1u64 << EventKind::State as u32;
        let perms: Vec<CardIdx> = self.battlefield.clone();
        for c in perms {
            let o = &self.objs[c.i()];
            let def = o.def;
            let d = self.db.def(def);
            // (A permanent's abilities only: not one that lost all of them,
            // CR 613.1f, nor a nonbasic land that is a Mountain, 305.7.)
            if d.trig_mask & bit == 0 || self.lost_abilities(c) || (self.moon_game && self.moon_on(c)) {
                continue;
            }
            let (controller, r) = (o.controller, ObjRef { idx: c, inc: o.inc });
            for slot in 0..d.delayed_base as usize {
                let t = &d.triggers[slot];
                if t.event != EventKind::State || !self.state_condition_holds(&t.pred, c, controller) {
                    continue;
                }
                if self.state_trigger_pending(r, t) {
                    continue;
                }
                self.note(|g| format!("{}'s state trigger triggers", g.name(c)));
                self.waiting.push(PendingTrigger { controller, source: r, source_def: def, slot: slot as u8, ev: EvCtx::default() });
            }
        }
    }

    /// Would a state-triggered ability trigger now (the cleanup step's
    /// check, CR 514.3a)?
    pub(crate) fn state_triggers_due(&self) -> bool {
        if !self.db.state_triggers {
            return false;
        }
        let bit = 1u64 << EventKind::State as u32;
        self.battlefield.iter().any(|&c| {
            let o = &self.objs[c.i()];
            let d = self.db.def(o.def);
            if d.trig_mask & bit == 0 || self.lost_abilities(c) || (self.moon_game && self.moon_on(c)) {
                return false;
            }
            let r = ObjRef { idx: c, inc: o.inc };
            d.triggers[..d.delayed_base as usize]
                .iter()
                .any(|t| t.event == EventKind::State && self.state_condition_holds(&t.pred, c, o.controller) && !self.state_trigger_pending(r, t))
        })
    }

    /// The condition of a state trigger of permanent `c`: its source matches
    /// a filter ("has no ice counters on it").
    fn state_condition_holds(&self, p: &TrigPred, c: CardIdx, controller: crate::ids::PlayerId) -> bool {
        match p {
            TrigPred::Source(f) => {
                let ctx = crate::cast::Ctx { controller, source: Some(self.obj_ref(c)) };
                self.card_matches(f, c, &ctx)
            }
            other => unreachable!("a state trigger's condition {other:?} (lower/lands_a.rs makes Source only)"),
        }
    }

    /// Has this object's state trigger `t` triggered and not left the stack
    /// (an instance waiting, or one on the stack)?
    fn state_trigger_pending(&self, r: ObjRef, t: &crate::db::TriggerDef) -> bool {
        let same = |def: crate::ids::DefId, slot: u8| self.db.def(def).triggers.get(slot as usize) == Some(t);
        self.waiting.iter().any(|w| w.source == r && same(w.source_def, w.slot))
            || self.stack.iter().any(|s| match s.kind {
                StackKind::Ability { ability, source } => match ability.slot {
                    crate::db::AbilitySlot::Trigger(k) => source == r && same(ability.def, k),
                    _ => false,
                },
                StackKind::Spell { .. } => false,
            })
    }
}
