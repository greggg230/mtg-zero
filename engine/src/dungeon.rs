//! The initiative (CR 725) and the Undercity dungeon (309, 701.49; M7b part
//! 3b, ENGINE.md 27.4).
//!
//! The initiative is a designation (`Game::initiative`). Its three inherent
//! triggered abilities have no source (725.2): they are the triggers of a
//! rules definition named [`INITIATIVE`] and name a slot in no zone as their
//! source (`Game::rules_src`). A player's dungeon is an object in the command
//! zone (309.3) whose definition is [`UNDERCITY`]; its venture marker's room
//! is `Game::dungeons[p].room`. Moving the marker into a room makes that
//! room's ability trigger (309.4c: a triggered ability on the stack whose
//! source is the dungeon). A dungeon whose marker is on its bottommost room
//! is removed from the game once no room ability of it waits or is on the
//! stack (704.5t): its owner completes it (309.7). Both definitions are built
//! by the lowering (`lower/effect_stack.rs`) for the cards that take the
//! initiative.

use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, DefId, ObjRef, PlayerId, Zone};
use crate::program::Ext;
use crate::resolve::{Flow, Frame};
use crate::triggers::{EvCtx, PendingTrigger};

/// The rules definitions' names.
pub(crate) const UNDERCITY: &str = "Undercity";
pub(crate) const INITIATIVE: &str = "The initiative";

/// Undercity's rooms, top to bottom; a room's ability is the dungeon
/// definition's trigger of the same index.
pub const ROOMS: [&str; 9] =
    ["Secret Entrance", "Forge", "Lost Well", "Trap!", "Arena", "Stash", "Archives", "Catacombs", "Throne of the Dead Three"];

/// The rooms a room leads to (CR 309.4b: its arrows).
pub(crate) const EXITS: [&[u8]; 9] = [&[1, 2], &[3, 4], &[4, 5], &[6], &[6, 7], &[7], &[8], &[8], &[]];

/// The bottommost room (CR 309.4a).
pub(crate) const BOTTOM: u8 = 8;

/// The initiative's triggered abilities (indices of its definition's
/// triggers).
pub(crate) const TAKEN: u8 = 0;
pub(crate) const UPKEEP: u8 = 1;
pub(crate) const DAMAGED: u8 = 2;

impl Game {
    fn rules_def(&self, name: &str) -> DefId {
        self.db.token_by_name(name).unwrap_or_else(|| panic!("no rules definition {name} (no card takes the initiative)"))
    }

    /// Does `p` have the initiative?
    pub fn has_initiative(&self, p: PlayerId) -> bool {
        self.initiative == Some(p)
    }

    /// The player with the initiative, if any (CR 725).
    pub fn initiative(&self) -> Option<PlayerId> {
        self.initiative
    }

    /// `p`'s dungeon: (the room of the venture marker if `p` has a dungeon
    /// in the command zone, dungeons completed).
    pub fn dungeon(&self, p: PlayerId) -> (Option<u8>, u8) {
        let d = &self.dungeons[p as usize];
        (d.obj.map(|_| d.room), d.completed)
    }

    /// One of the initiative's triggered abilities triggers for `p` (CR
    /// 725.2: no source; controlled by the player named), about `ev`.
    pub(crate) fn initiative_trigger(&mut self, p: PlayerId, slot: u8, ev: EvCtx) {
        let def = self.rules_def(INITIATIVE);
        let src = match self.rules_src {
            Some(c) => c,
            None => {
                let c = self.add_card(def, p, Zone::Nowhere);
                self.rules_src = Some(c);
                c
            }
        };
        let source = self.obj_ref(src);
        self.waiting.push(PendingTrigger { controller: p, source, source_def: def, slot, ev });
    }

    /// `p` takes the initiative (CR 725.2-3: any other player stops having
    /// it); "whenever a player takes the initiative, that player ventures
    /// into Undercity" triggers (also for a player who had it).
    pub(crate) fn take_initiative(&mut self, p: PlayerId) {
        self.initiative = Some(p);
        self.note(|_| format!("P{p} takes the initiative"));
        self.initiative_trigger(p, TAKEN, EvCtx { player: Some(p), ..Default::default() });
    }

    /// The venture marker of `p` moves into `room`: that room's ability
    /// triggers (CR 309.4c, 309.5).
    fn enter_room(&mut self, p: PlayerId, room: u8) {
        self.dungeons[p as usize].room = room;
        let obj = self.dungeons[p as usize].obj.expect("a dungeon");
        let source = self.obj_ref(obj);
        let def = self.objs[obj.i()].def;
        self.note(|_| format!("P{p} ventures into {}", ROOMS[room as usize]));
        self.waiting.push(PendingTrigger { controller: p, source, source_def: def, slot: room, ev: EvCtx::default() });
    }

    /// `p` puts Undercity into the command zone with the venture marker on
    /// its topmost room (CR 701.49a, d).
    fn new_dungeon(&mut self, p: PlayerId) {
        let def = self.rules_def(UNDERCITY);
        let c = self.add_card(def, p, Zone::Command);
        self.command.push(c);
        self.dungeons[p as usize].obj = Some(c);
        self.enter_room(p, 0);
    }

    /// `p` completes their dungeon: it is removed from the game (CR 309.7,
    /// 704.5t, 701.49c).
    fn complete_dungeon(&mut self, p: PlayerId) {
        let d = &mut self.dungeons[p as usize];
        let Some(c) = d.obj.take() else { return };
        d.completed = d.completed.saturating_add(1);
        d.room = 0;
        self.note(|_| format!("P{p} completes Undercity"));
        self.move_card(c, Zone::Nowhere, Pos::Top);
    }

    /// CR 704.5t: the players whose venture marker is on the bottommost room
    /// of their dungeon while no room ability of it waits to be put on the
    /// stack or is on the stack.
    pub(crate) fn finished_dungeons(&self) -> Vec<PlayerId> {
        (0..2u8)
            .filter(|&p| {
                let d = &self.dungeons[p as usize];
                let Some(c) = d.obj else { return false };
                if d.room != BOTTOM {
                    return false;
                }
                let r: ObjRef = self.obj_ref(c);
                !self.waiting.iter().any(|w| w.source == r)
                    && !self.stack.iter().any(|s| matches!(s.kind, crate::game::StackKind::Ability { source, .. } if source == r))
            })
            .collect()
    }

    /// 704.5t for `ps` (state-based actions).
    pub(crate) fn remove_finished_dungeons(&mut self, ps: &[PlayerId]) {
        for &p in ps {
            self.complete_dungeon(p);
        }
    }

    /// A dungeon object (in the command zone, not an emblem)?
    pub(crate) fn is_dungeon(&self, c: CardIdx) -> bool {
        self.dungeons.iter().any(|d| d.obj == Some(c))
    }

    pub(crate) fn exec_dungeon(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::TakeInitiative { who } => {
                if let Some(p) = self.who(f, *who) {
                    self.take_initiative(p);
                }
                Flow::Next
            }
            Ext::Venture { who } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Number(r)) => {
                        self.enter_room(p, r);
                        return Flow::Next;
                    }
                    Some(other) => unreachable!("room answer {other:?}"),
                    None => {}
                }
                let d = self.dungeons[p as usize];
                match d.obj {
                    // CR 701.49a: no dungeon: Undercity, topmost room.
                    None => self.new_dungeon(p),
                    // 701.49c: on the bottommost room (its ability still on
                    // the stack): complete it, then a new one.
                    Some(_) if d.room == BOTTOM => {
                        self.complete_dungeon(p);
                        self.new_dungeon(p);
                    }
                    // 701.49b: an adjacent room, the player's choice.
                    Some(_) => {
                        let exits = EXITS[d.room as usize];
                        if let [only] = exits {
                            self.enter_room(p, *only);
                        } else {
                            let opts = exits.iter().map(|&r| Opt { action: act::NUMBER + r as u16, choice: Choice::Number(r) }).collect();
                            self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseRoom, opts);
                            return Flow::Suspend;
                        }
                    }
                }
                Flow::Next
            }
            other => unreachable!("not a dungeon instruction: {other:?}"),
        }
    }
}
