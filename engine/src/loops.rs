//! Loops of optional actions (CR 732.2a; MTR 4.4): no progress is a stop.
//!
//! A player may repeat an optional loop any number of times (CR 732.2a): the
//! engine offers every repetition, with no count. Its sources of unbounded
//! optional repetition are the *loop actions*: activating a non-mana
//! activated ability of a permanent on the battlefield (`ActEffect::Stack`,
//! the permanent's own abilities and those granted to it: Basalt Monolith's
//! "{3}: Untap", Voltaic Key) and answering YES to "may copy this spell"
//! (`Ext::CopySpell { may: true }`: Chain of Smog, Chain Lightning).
//!
//! **The rule.** When player p takes a loop action, the game records
//! (`Game::loop_taken`, for the rest of the turn) the loop hash of the state
//! as p's decider sees it ([`Game::loop_hash`]) with the action
//! ([`LoopAct`]). The action is not offered to p at a decision whose loop
//! hash equals a recorded one for the same action: the state is identical
//! to one in which p took that action, so everything reachable by taking it
//! now was reachable then, and taking it again could only repeat an
//! iteration that made no progress (a loop must stop when it comes back to a
//! previous game state, MTR 4.4). Basalt Monolith thus loops once: after its
//! untap, paid with its own mana, resolves, the game is where it was and the
//! untap is not offered. An iteration that changes anything (a land tapped,
//! a card discarded, life lost, an extra turn) keeps the loop going, with no
//! limit: Grim Monolith untaps as long as lands pay for it.
//!
//! **The board** (the user's decision, 2026-11-09: tournament rules require a
//! player who loops without progressing the board state to choose a
//! different action, MTR 4.4). The hash reads the state without the stack
//! objects loop actions put there ([`loop_product`]: activated abilities of
//! permanents). So holding priority and activating again while the first
//! activation waits is no progress, unless the activation itself changed the
//! board (a cost paid: mana spent, a land tapped, life paid, a card
//! discarded) or something else did (another player's response, a
//! triggered ability, a resolution). Free activations stacked in endless
//! new orders (the EoE fuzz's seeds 1016 and 1024: Wandering Fumarole's
//! "{0}: Switch" and Lavaclaw Reaches' X = 0 pump, held priority) stop after
//! one of each a board; paid ones go on as long as resources are spent.
//!
//! **The hash** is a fixed hash (`DefaultHasher::new()`) of numbers: the
//! state part of `Game::view_of` (`Game::view_state`) for the decider, the
//! view the hidden-information checks compare across determinized worlds (so
//! the hash is the same in every world `determinize` deals for that viewer:
//! the options stay determinize-invariant and tell nothing hidden) and the
//! public-state gate proves every public field changes. Canonicalised so a
//! repeated state hashes equal: the consecutive passes left out (taking a
//! loop action sets them to 0, CR 117.3c), and every timestamp (permanents,
//! continuous effects, copy and control effects, granted triggered
//! abilities) named by its rank among the state's timestamps (a uniform
//! renumbering is the same state; the order is what the rules read, CR
//! 613.7), continuous effects that change nothing (+0/+0, [`inert`]) left
//! out, and an object's power and toughness switches read by their parity
//! (layer 7d holds only switches and each toggles, CR 613.4d: order and pairs
//! with one duration don't matter, so Wandering Fumarole's free "{0}: Switch"
//! comes back to its state after two). Nothing else is dropped: a wrong
//! "equal" would prune a legal option, a missed one only lets a loop run
//! longer. So it also holds what the view
//! leaves out but the rules read: the order of timestamps across kinds, the
//! filters of group effects, deathtouch damage not yet checked, the
//! transformations since an ability was put on the stack, the full events of
//! waiting and delayed triggered abilities, and the last known information
//! of the gone objects that stack items, triggered abilities and frames name
//! (CR 608.2h). For the may-copy decision (mid-resolution) it also holds the
//! suspended frame (the program and op running, every field, objects by
//! public place) and the rest of the agenda. A 64-bit collision between two
//! different states of one turn (records are cleared as a turn begins) has
//! a probability of about n^2 / 2^65 for n states: negligible.
//!
//! The hash is computed only when needed: when a loop action is taken (its
//! record), and at a decision where the decider has a record of that action
//! (once per decision). Its cost is that of building the view (a few
//! thousand numbers at most) and hashing it.

use crate::flow::Task;
use crate::game::{Game, StackItem, StackKind};
use crate::ids::{CardIdx, ObjRef, PlayerId, Target};
use crate::layers::{Affected, Until};
use crate::resolve::Frame;
use crate::testkit::h64;
use crate::triggers::EvCtx;
use std::cell::OnceCell;
use std::hash::{Hash, Hasher};

/// A continuous effect the loop hash doesn't read one by one (its timestamp
/// too): +0/+0 changes nothing, in any layer and at any timestamp (CR 613.4c;
/// Lavaclaw Reaches' "{X}: +X/+0" with X = 0), so a state with it is the same
/// as one without it in every way the rules read; an object's power and
/// toughness switches are read by their parity instead (`switch_parity`).
pub(crate) fn inert(e: &crate::layers::Effect) -> bool {
    matches!(e.m, crate::db::Mod::Pt(0, 0)) || is_object_switch(e)
}

/// A stack object a loop action put there: an activated ability of a
/// permanent (`AbilitySlot::Activated`; the only activated abilities that
/// use the stack and come from the battlefield). The loop hash reads the
/// board without them (module docs, "the board"): stacking another
/// activation while the first waits changes nothing on the board.
pub(crate) fn loop_product(s: &StackItem) -> bool {
    matches!(&s.kind, StackKind::Ability { ability, .. } if matches!(ability.slot, crate::db::AbilitySlot::Activated(_)))
}

/// A switch of one object's power and toughness (layer 7d, CR 613.4d).
fn is_object_switch(e: &crate::layers::Effect) -> bool {
    matches!((&e.m, &e.on), (crate::db::Mod::SwitchPt, Affected::Obj(_)))
}

/// A loop action (module docs).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LoopAct {
    /// Activating ability `slot` (its place among the permanent's own
    /// abilities, then those granted to it) of the permanent at battlefield
    /// position `place`.
    Activate { place: u32, slot: u8 },
    /// YES to "may copy this spell".
    MayCopy,
}

/// The timestamps of a hashed state, ascending: the loop hash names each by
/// its rank (module docs).
pub(crate) struct Canon {
    ts: Vec<u32>,
}

impl Canon {
    /// The rank of timestamp `t` (a value outside the set, never expected:
    /// itself, apart).
    pub(crate) fn rank(&self, t: u32) -> u64 {
        match self.ts.binary_search(&t) {
            Ok(i) => i as u64,
            Err(_) => (1 << 63) | t as u64,
        }
    }
}

impl Game {
    /// Would taking loop action `act` now repeat one `p` took this turn at a
    /// state with the same loop hash (module docs)? `hash` caches this
    /// decision's hash, computed only if `p` has a record of `act`; `frame`:
    /// the suspended frame of a may-copy.
    pub(crate) fn loop_repeats(&self, p: PlayerId, act: LoopAct, frame: Option<&Frame>, hash: &OnceCell<u64>) -> bool {
        if !self.loop_taken.iter().any(|x| x.0 == p && x.2 == act) {
            return false;
        }
        let h = *hash.get_or_init(|| self.loop_hash(p, frame));
        self.loop_taken.iter().any(|&(q, k, a)| q == p && k == h && a == act)
    }

    /// `p` takes loop action `act` at this state: record it (module docs).
    pub(crate) fn note_loop(&mut self, p: PlayerId, act: LoopAct, frame: Option<&Frame>) {
        let h = self.loop_hash(p, frame);
        if !self.loop_taken.contains(&(p, h, act)) {
            self.loop_taken.push((p, h, act));
        }
    }

    /// The loop hash of the state for `p`'s decision (module docs), as `p`'s
    /// decider (CR 722.1) sees it when the decision is shown: `advance`'s
    /// look at a top card a player may look at any time and the controlling
    /// player's view of the controlled player's cards come first (on a
    /// clone; already done at a decision), so the hash checked before
    /// asking and the one recorded at the answer agree.
    pub(crate) fn loop_hash(&self, p: PlayerId, frame: Option<&Frame>) -> u64 {
        let viewer = self.decider_of(p);
        if self.seen & crate::game::SEEN_TOP != 0 || self.turn_controller.is_some() {
            let mut g = self.clone();
            if g.seen & crate::game::SEEN_TOP != 0 && g.outcome.is_none() {
                let _ = g.look_tops();
            }
            g.sync_controller_view();
            g.keys.clear();
            return g.loop_hash_here(viewer, frame);
        }
        self.loop_hash_here(viewer, frame)
    }

    fn loop_hash_here(&self, viewer: PlayerId, frame: Option<&Frame>) -> u64 {
        let stale = self.loop_stale_refs(frame);
        let canon = self.loop_canon(&stale);
        let mut v: Vec<u64> = Vec::with_capacity(512);
        self.view_state(viewer, Some(&canon), &mut v);
        self.loop_extras(&canon, &mut v);
        for &r in &stale {
            self.loop_lki(r, &canon, &mut v);
        }
        if let Some(f) = frame {
            // The suspended frame, then the rest of the resolution (the
            // asking `Task::Run` is off the agenda both when the options are
            // made and when the answer comes).
            v.push(u64::MAX - 30);
            self.frame_canon(f, &mut v);
            for t in &self.agenda {
                match t {
                    Task::Run(fr) => {
                        v.push(u64::MAX - 31);
                        self.frame_canon(fr, &mut v);
                    }
                    Task::FinishResolve { id } => v.extend([u64::MAX - 32, self.stack_pos(*id)]),
                    t => v.push(h64(&std::mem::discriminant(t))),
                }
            }
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        v.hash(&mut h);
        h.finish()
    }

    /// A stack item by its position (99: none).
    fn stack_pos(&self, id: u32) -> u64 {
        self.stack.iter().position(|x| x.id == id).map_or(99, |i| i as u64)
    }

    /// A stack item by its position in the view (`board` false: among all
    /// of them), or in the loop hash's board (`board`: among those that
    /// aren't loop products; 98 names a loop product). 99: none.
    pub(crate) fn view_stack_pos(&self, id: u32, board: bool) -> u64 {
        if !board {
            return self.stack_pos(id);
        }
        let mut k = 0;
        for s in &self.stack {
            if s.id == id {
                return if loop_product(s) { 98 } else { k };
            }
            if !loop_product(s) {
                k += 1;
            }
        }
        99
    }

    /// The gone objects (stale references, CR 400.7) that stack items,
    /// waiting and delayed triggered abilities and the frames name: their
    /// last known information is hashed (`loop_lki`). In order of first
    /// mention.
    fn loop_stale_refs(&self, frame: Option<&Frame>) -> Vec<ObjRef> {
        let mut out: Vec<ObjRef> = Vec::new();
        let mut add = |r: ObjRef| {
            if !self.live(r) && !out.contains(&r) {
                out.push(r);
            }
        };
        let ev = |e: &EvCtx, add: &mut dyn FnMut(ObjRef)| {
            e.obj.into_iter().chain(e.obj2).for_each(&mut *add);
        };
        let tgt = |t: &Target, add: &mut dyn FnMut(ObjRef)| {
            if let Target::Obj(o) = t {
                add(*o);
            }
        };
        for s in &self.stack {
            if let StackKind::Ability { source, .. } = s.kind {
                add(source);
            }
            ev(&s.ev, &mut add);
            s.noted.iter().copied().for_each(&mut add);
            if let Some(Some(a)) = s.attack_as {
                add(a);
            }
            s.targets.iter().for_each(|(_, t)| tgt(t, &mut add));
        }
        for w in &self.waiting {
            add(w.source);
            ev(&w.ev, &mut add);
        }
        for d in &self.delayed {
            add(d.source);
            ev(&d.ev, &mut add);
        }
        let mut frames: Vec<&Frame> = frame.into_iter().collect();
        if frame.is_some() {
            frames.extend(self.agenda.iter().filter_map(|t| match t {
                Task::Run(fr) => Some(fr),
                _ => None,
            }));
        }
        for f in frames {
            add(f.source);
            ev(&f.ev, &mut add);
            f.noted.iter().copied().for_each(&mut add);
            if let Some(Some(a)) = f.attack_as {
                add(a);
            }
            f.targets.iter().for_each(|(_, t)| tgt(t, &mut add));
        }
        out
    }

    /// The timestamps the loop hash ranks (`Canon`): the permanents' (and
    /// the phased-out ones'), the continuous effects', the copy and control
    /// effects', the granted triggered abilities', and when the gone objects
    /// `stale` lost their abilities.
    fn loop_canon(&self, stale: &[ObjRef]) -> Canon {
        let mut ts: Vec<u32> = Vec::new();
        ts.extend(self.battlefield.iter().chain(self.w3.phased.iter()).map(|&c| self.objs[c.i()].timestamp));
        ts.extend(self.effects.iter().filter(|e| !inert(e)).map(|e| e.ts));
        ts.extend(self.copies.iter().map(|x| x.ts));
        ts.extend(self.w6.control.iter().map(|x| x.ts));
        ts.extend(self.marks.iter().filter_map(|(_, m)| match m {
            crate::game::Mark::Granted { ts, .. } => Some(*ts),
            _ => None,
        }));
        ts.extend(self.w3.lki_loss.iter().filter(|x| stale.contains(&x.0)).map(|x| x.1));
        ts.sort_unstable();
        ts.dedup();
        Canon { ts }
    }

    /// A duration, its object by place.
    fn until_canon(&self, u: &Until) -> u64 {
        match u {
            Until::EndOfTurn => 0,
            Until::Forever => 1,
            Until::YourNextTurn => 2,
            Until::WhileSource => 4,
            Until::WhileOnBattlefield(r) => 3000 + self.view_live_place(*r),
        }
    }

    /// An event: its subjects by place, its spell or ability by stack
    /// position.
    fn ev_canon(&self, e: &EvCtx, v: &mut Vec<u64>) {
        let EvCtx { player, obj, obj2, amount, item } = e;
        let lp = |r: &Option<ObjRef>| r.map_or(u64::MAX, |r| self.view_live_place(r));
        v.extend([player.map_or(9, |p| p as u64), lp(obj), lp(obj2), *amount, self.stack_pos(*item)]);
    }

    /// A target by place (a stack item by position; a gone object apart).
    fn target_canon(&self, t: &Target) -> u64 {
        match *t {
            Target::Player(p) => p as u64,
            Target::Obj(o) => ((!self.live(o) as u64) << 32) | (100 + self.view_place(o.idx)),
            Target::Stack(id) => 9000 + self.stack_pos(id),
        }
    }

    /// What the loop hash adds to the view (module docs).
    fn loop_extras(&self, k: &Canon, v: &mut Vec<u64>) {
        v.push(u64::MAX - 20);
        // Timestamps across kinds; deathtouch damage dealt since the last
        // state-based check (CR 704.5h).
        for &c in self.battlefield.iter().chain(self.w3.phased.iter()) {
            let o = &self.objs[c.i()];
            v.extend([k.rank(o.timestamp), o.deathtouched as u64]);
        }
        v.push(u64::MAX - 23);
        // The Enters events an open entry holds (CR 603.6a; a decision asked
        // while its permanents enter, `Game::begin_entering`).
        for e in &self.entries {
            v.extend([e.held.len() as u64, k.rank(e.since), e.base as u64, e.open as u64]);
            v.extend(e.held.iter().flat_map(|&(r, note)| [self.view_live_place(r), note as u64]));
        }
        v.push(u64::MAX - 22);
        // An object's power and toughness switches, by parity (module docs):
        // the objects and durations with an odd number of them.
        let mut switches: Vec<(u64, u64)> = self
            .effects
            .iter()
            .filter(|e| is_object_switch(e))
            .map(|e| match e.on {
                Affected::Obj(r) => (self.view_live_place(r), self.until_canon(&e.until)),
                Affected::Group(_) => unreachable!("is_object_switch"),
            })
            .collect();
        switches.sort_unstable();
        for run in switches.chunk_by(|a, b| a == b) {
            if run.len() % 2 == 1 {
                v.extend([run[0].0, run[0].1]);
            }
        }
        v.push(u64::MAX - 21);
        // Which permanents a group effect affects (CR 611.2c).
        for e in self.effects.iter().filter(|e| !inert(e)) {
            v.push(match &e.on {
                Affected::Group(f) => h64(f),
                Affected::Obj(_) => 0,
            });
        }
        v.push(u64::MAX - 22);
        // Copy effects (CR 707, 613.2): in their timestamp order.
        for x in &self.copies {
            let crate::copies::CopyFx { obj, ts, until, vals, own } = x;
            v.extend([self.view_live_place(*obj), k.rank(*ts), self.until_canon(until), h64(vals), *own as u64]);
        }
        v.push(u64::MAX - 23);
        let mut ctl = self.w6.control.clone();
        ctl.sort_by_key(|x| x.ts);
        v.extend(ctl.iter().map(|x| k.rank(x.ts)));
        v.push(u64::MAX - 24);
        // Stack items: Mox Diamond's note; whether an ability's source
        // transformed since (CR 701.27f). (Loop products left out, as in
        // `view_state`.)
        for s in self.stack.iter().filter(|s| !loop_product(s)) {
            v.push(s.discarded_land as u64);
            v.push(match s.kind {
                StackKind::Ability { source, .. } if self.live(source) => self.objs[source.idx.i()].flips.wrapping_sub(s.flips) as u64,
                _ => u64::MAX,
            });
        }
        v.push(u64::MAX - 25);
        // Triggered abilities: their sources and full events.
        for w in &self.waiting {
            v.push(self.view_live_place(w.source));
            self.ev_canon(&w.ev, v);
        }
        v.push(u64::MAX - 26);
        for d in &self.delayed {
            v.push(self.view_live_place(d.source));
            self.ev_canon(&d.ev, v);
        }
        v.push(u64::MAX - 27);
        for t in &self.temp_triggers {
            v.extend([self.view_live_place(t.source), t.obj.map_or(u64::MAX, |r| self.view_live_place(r))]);
        }
        v.push(u64::MAX - 28);
    }

    /// The last known information of gone object `r` (CR 608.2h).
    fn loop_lki(&self, r: ObjRef, k: &Canon, v: &mut Vec<u64>) {
        v.push(u64::MAX - 40);
        for (x, kind, n) in &self.lki_counters {
            if *x == r {
                v.extend([h64(kind), *n as u64]);
            }
        }
        v.push(u64::MAX - 41);
        for (x, vals) in &self.lki_copy {
            if *x == r {
                v.push(h64(vals));
            }
        }
        v.push(u64::MAX - 42);
        for &(x, h) in &self.lki_attached {
            if x == r {
                v.push(self.view_live_place(h));
            }
        }
        v.push(u64::MAX - 43);
        for (x, d) in &self.lki_damage {
            if *x == r {
                let crate::game::DamageSource { controller, deathtouch, lifelink, infect, spell, colors, types, obj } = d;
                v.extend([*controller as u64, *deathtouch as u64, *lifelink as u64, *infect as u64, *spell as u64]);
                v.extend([h64(colors), *types as u64, obj.map_or(u64::MAX, |o| self.view_live_place(o))]);
            }
        }
        v.push(u64::MAX - 44);
        for &(x, t, c) in &self.lki_toughness {
            if x == r {
                v.extend([t as u64, c as u64]);
            }
        }
        v.push(u64::MAX - 45);
        for &(c, inc, pw) in &self.lki_power {
            if c == r.idx && inc == r.inc {
                v.push(pw as u64);
            }
        }
        v.push(u64::MAX - 46);
        for (x, d, item) in &self.spell_lki {
            if *x == r {
                v.push(*d as u64);
                self.item_canon(item, v);
            }
        }
        v.push(u64::MAX - 47);
        for &(x, h) in &self.w3.lki_host {
            if x == r {
                v.push(self.view_live_place(h));
            }
        }
        v.push(u64::MAX - 48);
        for &(x, ts) in &self.w3.lki_loss {
            if x == r {
                v.push(k.rank(ts));
            }
        }
        v.push(u64::MAX - 49);
    }

    /// A stack item that left the stack (`spell_lki`), every field
    /// (objects by place).
    fn item_canon(&self, s: &StackItem, v: &mut Vec<u64>) {
        let StackItem {
            id: _,
            kind,
            controller,
            targets,
            cost_option,
            modes,
            ev,
            alt,
            from,
            delved,
            delve_floor,
            discarded_land,
            x,
            division,
            attack_as,
            noted,
            flips,
            origin,
            times,
            spliced,
        } = s;
        match *kind {
            StackKind::Spell { card } => v.extend([1, self.view_place(card)]),
            StackKind::Ability { ability, source } => v.extend([2, h64(&ability), self.view_live_place(source)]),
        }
        v.extend([*controller as u64, *cost_option as u64, *alt as u64, h64(from), *delved as u64, *delve_floor as u64]);
        v.extend([*discarded_land as u64, *x as u64, *flips as u64, h64(origin), *times as u64]);
        v.extend(targets.iter().flat_map(|(r, t)| [*r as u64, self.target_canon(t)]));
        v.push(u64::MAX - 50);
        v.extend(modes.iter().map(|&m| m as u64));
        v.push(u64::MAX - 51);
        self.ev_canon(ev, v);
        v.extend(division.iter().copied());
        v.push(u64::MAX - 52);
        v.push(attack_as.map_or(u64::MAX, |a| a.map_or(u64::MAX - 1, |r| self.view_live_place(r))));
        v.extend(noted.iter().map(|&r| self.view_live_place(r)));
        v.push(u64::MAX - 53);
        v.extend(spliced.iter().map(|&d| d as u64));
        v.push(u64::MAX - 54);
    }

    /// A resolution frame, every field (exhaustive: a new field is a compile
    /// error here), objects by public place: the program and op running
    /// (`ability`, `pc`) and its state.
    fn frame_canon(&self, f: &Frame, v: &mut Vec<u64>) {
        let Frame {
            ability,
            pc,
            source,
            source_def,
            controller,
            targets,
            legal,
            i,
            lki,
            pile,
            floor,
            ev,
            x,
            picked,
            picked_defs,
            made,
            alt,
            division,
            spell_x,
            made_all,
            attack_as,
            noted,
            flips,
            page,
            step,
        } = f;
        let lp = |r: ObjRef| self.view_live_place(r);
        let pl = |c: &CardIdx| self.view_place(*c);
        v.extend([h64(ability), *pc as u64, lp(*source), *source_def as u64, *controller as u64, *legal, *i as u64]);
        v.extend(targets.iter().flat_map(|(r, t)| [*r as u64, self.target_canon(t)]));
        v.push(u64::MAX - 60);
        v.push(lki.map_or(u64::MAX, |l| h64(&l)));
        v.extend(pile.iter().map(pl));
        v.push(u64::MAX - 61);
        v.extend([*floor as u64, *x as u64]);
        self.ev_canon(ev, v);
        v.extend(picked.iter().map(pl));
        v.push(u64::MAX - 62);
        v.extend(picked_defs.iter().map(|&d| d as u64));
        v.push(u64::MAX - 63);
        v.extend([made.as_ref().map_or(u64::MAX, pl), *alt as u64, *spell_x as u64]);
        v.extend(division.iter().copied());
        v.push(u64::MAX - 64);
        v.extend(made_all.iter().map(pl));
        v.push(u64::MAX - 65);
        v.push(attack_as.map_or(u64::MAX, |a| a.map_or(u64::MAX - 1, lp)));
        v.extend(noted.iter().map(|&r| lp(r)));
        v.push(u64::MAX - 66);
        // (Transformations of the source since, CR 701.27f.)
        v.push(if self.live(*source) { self.objs[source.idx.i()].flips.wrapping_sub(*flips) as u64 } else { u64::MAX });
        v.extend([h64(page), h64(step)]);
    }
}
