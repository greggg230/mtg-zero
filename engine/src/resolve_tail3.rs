//! The `Ext` instructions of the single-card tail, part 3 (M7c, ENGINE.md
//! 30).

use crate::db::{supertypes, types};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{opp, PlayerId, Zone};
use crate::layers::Affected;
use crate::program::Ext;
use crate::resolve::{Flow, Frame};

/// `Permission::turn` with this bit: "until your next end step", the low
/// bits the permission player's turn count whose end step ends it.
pub const PERM_UNTIL_END_STEP: u32 = 1 << 29;

/// The target requirements of a stack item: its ability's (the chosen
/// modes'), then each spliced card's (CR 702.47b).
pub(crate) fn item_reqs(db: &'static crate::db::CardDb, item: &crate::game::StackItem, ar: crate::db::AbilityRef) -> Vec<&'static crate::db::TargetReq> {
    let mut reqs = db.ability(ar).reqs(&item.modes);
    for &d in &item.spliced {
        if let Some(ab) = &db.def(d).spell {
            reqs.extend(ab.targets.iter());
        }
    }
    reqs
}

impl Game {
    /// Splice may be asked as `p` casts `card` (CR 702.47a): an instant or
    /// sorcery with a subtype some card of `p`'s deck splices onto. Asked
    /// whatever `p`'s hand holds (decklists are open, ENGINE.md 10): the
    /// question says nothing about the hand.
    pub(crate) fn splice_possible(&self, p: PlayerId, card: crate::ids::CardIdx) -> bool {
        let def = self.def_of(card);
        if def.subtypes.is_empty() || !def.is(types::INSTANT | types::SORCERY) || self.is_copy_obj(card) {
            return false;
        }
        self.objs.iter().any(|o| o.owner == p && self.db.def(o.def).splice.is_some_and(|(s, _)| def.subtypes.contains(&s)))
    }

    /// `Task::Splice`: offer each card in hand that splices onto the spell
    /// (one per definition, a copy not spliced yet), whose cost can be paid
    /// with the rest and whose targets exist; reveal and record the one
    /// chosen, then ask again.
    pub(crate) fn choose_splice(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let crate::game::StackKind::Spell { card } = self.stack[pos].kind else { return };
        let p = self.stack[pos].controller;
        // The k-th copy of a definition in hand (canonical order) not yet
        // spliced.
        let copy_of = |g: &Game, d: crate::ids::DefId| {
            let k = g.stack[pos].spliced.iter().filter(|&&x| x == d).count();
            g.hand_sorted_in(p).iter().copied().filter(|&c| g.objs[c.i()].def == d).nth(k)
        };
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                let d = self.objs[c.i()].def;
                let c = copy_of(self, d).unwrap_or(c);
                self.reveal(c);
                self.stack[pos].spliced.push(d);
                self.note(|g| format!("splices {} onto {}", g.name(c), g.name(card)));
                self.agenda.push(Task::Splice { id });
                return;
            }
            Some(Choice::Pass) => return,
            Some(other) => unreachable!("splice answer {other:?}"),
            None => {}
        }
        let subs = self.def_of(card).subtypes.clone();
        let allow = self.spend_allow(card);
        let mut opts = Vec::new();
        for (action, c) in self.hand_slots(p) {
            let d = self.objs[c.i()].def;
            let Some((s, cost)) = self.db.def(d).splice.clone() else { continue };
            if !subs.contains(&s) || copy_of(self, d).is_none() {
                continue;
            }
            let ok_targets = self.db.def(d).spell.as_ref().is_none_or(|ab| self.targets_available(ab, p, Some(self.obj_ref(card))));
            let item = &self.stack[pos];
            if ok_targets && self.can_pay_for(p, &self.announced_cost_pub(card, item, &cost), allow) {
                opts.push(Opt { action, choice: Choice::Card(c) });
            }
        }
        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        self.ask(Task::Splice { id }, p, DecisionKind::ChooseHandCard, opts);
    }

    pub(crate) fn exec_tail3(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::ApplyPtX { on, p, t } => {
                let x = f.spell_x as i32;
                let (dp, dt) = (*p as i32 * x, *t as i32 * x);
                for c in self.objs_on(f, on) {
                    let r = self.obj_ref(c);
                    self.add_effect(Affected::Obj(r), crate::db::Mod::Pt(dp, dt), crate::layers::Until::EndOfTurn, f.controller);
                }
                Flow::Next
            }
            Ext::ExileAll { filter, basics } => {
                // `f.i`: 0 = exile; 1, 2 = the searches of the active and
                // the other player. `f.x`: how many permanents each player
                // controlled (bits 0-7 and 8-15).
                if f.i == 0 {
                    let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                    let all: Vec<_> = self.battlefield.iter().copied().filter(|&c| self.card_matches(filter, c, &ctx)).collect();
                    let mut n = [0i32; 2];
                    for &c in &all {
                        n[self.objs[c.i()].controller as usize] += 1;
                    }
                    f.x = n[0].min(255) | n[1].min(255) << 8;
                    self.begin_batch();
                    for c in all {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                    self.end_batch();
                    if basics.is_none() {
                        return Flow::Next;
                    }
                    f.i = 1;
                    f.picked.clear();
                    f.floor = 0;
                }
                while f.i <= 2 {
                    let q: PlayerId = if f.i == 1 { self.turn.active } else { opp(self.turn.active) };
                    let n = (f.x >> (8 * q as i32)) & 255;
                    if n == 0 {
                        f.i += 1;
                        continue;
                    }
                    let done = match self.answer.take() {
                        Some(Choice::Def(d)) => {
                            let c = self
                                .library_sorted(q)
                                .into_iter()
                                .find(|&c| self.objs[c.i()].def == d && !f.picked.contains(&c))
                                .expect("a card of the chosen definition");
                            f.picked.push(c);
                            f.floor = d;
                            f.picked.len() as i32 >= n
                        }
                        Some(Choice::Pass) => true,
                        Some(other) => unreachable!("search answer {other:?}"),
                        None => {
                            // One definition per decision, in non-decreasing
                            // order (each set once); asked even with no basic
                            // land left (the library is hidden from the
                            // other player, ENGINE.md 10).
                            let lib = self.library_sorted(q);
                            let mut opts = Vec::new();
                            for (i, &c) in lib.iter().enumerate() {
                                let d = self.objs[c.i()].def;
                                if i > 0 && self.objs[lib[i - 1].i()].def == d {
                                    continue;
                                }
                                let def = self.db.def(d);
                                let basic = def.is(types::LAND) && def.supertypes & supertypes::BASIC != 0;
                                let left = lib.iter().any(|&x| self.objs[x.i()].def == d && !f.picked.contains(&x));
                                if d >= f.floor && left && basic {
                                    assert!((i as u16) < act::LIBRARY_N, "library slot {i} out of range");
                                    opts.push(Opt { action: act::LIBRARY + i as u16, choice: Choice::Def(d) });
                                }
                            }
                            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                            self.ask(Task::Run(f.clone()), q, DecisionKind::SearchLibrary, opts);
                            return Flow::Suspend;
                        }
                    };
                    if done {
                        // Done for this player: the cards enter tapped, then
                        // the library is shuffled.
                        self.finish_search_any(f, q, true, false);
                        f.floor = 0;
                        f.i += 1;
                    }
                }
                Flow::Next
            }
            Ext::TokensFor { who, token, n, tapped } => {
                f.made = None;
                if let Some(p) = self.who(f, *who) {
                    let def = self.db.token_def(*token);
                    let made = self.create_tokens(def, p, *n as u32, *tapped);
                    f.made = made.first().copied();
                    f.made_all = made;
                }
                Flow::Next
            }
            Ext::PlayerProtection => {
                // Until the player's next turn begins (their turn count).
                let pl = &mut self.players[f.controller as usize];
                pl.prot_until = pl.turns + 1;
                self.note(|_| format!("P{} gains protection from everything", f.controller));
                Flow::Next
            }
            Ext::ExileTopUntilEndStep { who, n, kind } => {
                let Some(q) = self.who(f, *who) else { return Flow::Next };
                // "Until your next end step": this turn's if it is the
                // controller's turn before its end step, else their next
                // turn's (the controller's turn count it ends in).
                let p = f.controller;
                let mine = self.turn.active == p && self.turn.step < crate::game::Step::End;
                let k = self.players[p as usize].turns + (!mine) as u32;
                for _ in 0..*n {
                    let Some(&top) = self.players[q as usize].library.last() else { break };
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, q);
                    self.reveal(top);
                    let card = self.move_card(top, Zone::Exile, Pos::Top);
                    self.permissions.push(crate::game::Permission { card, player: p, turn: Some(PERM_UNTIL_END_STEP | k), kind: *kind });
                }
                Flow::Next
            }
            Ext::ShuffleSourceIntoLibrary => {
                let r = f.source;
                if self.live(r) && self.objs[r.idx.i()].zone == Zone::Graveyard {
                    let owner = self.objs[r.idx.i()].owner;
                    self.move_card(r.idx, Zone::Library, Pos::Top);
                    self.shuffle_library(owner);
                }
                Flow::Next
            }
            Ext::AttachTargetToSource { tgt } => {
                let host = self.objs_on(f, &crate::program::On::Source);
                let eq = self.objs_on(f, &crate::program::On::Target(*tgt));
                if let (Some(&e), Some(&h)) = (eq.first(), host.first()) {
                    let q = self.damage_source_of(e);
                    let equipment = self.has_subtype(e, self.db.subtype_id("Equipment").unwrap_or(u16::MAX));
                    if equipment && !self.is_creature(e) && self.is_creature(h) && e != h && !self.protected_from(h, &q) {
                        let hr = self.obj_ref(h);
                        self.objs[e.i()].attached = Some(hr);
                        self.note(|g| format!("{} attached to {}", g.name(e), g.name(h)));
                    }
                }
                Flow::Next
            }
            Ext::RemoveCounterOrSacrifice { counter } => {
                if let Some(&c) = self.objs_on(f, &crate::program::On::Source).first() {
                    if self.counters_on(c, *counter) > 0 {
                        self.remove_counters(c, *counter, 1);
                    } else {
                        self.sacrifice(c);
                    }
                }
                Flow::Next
            }
            Ext::LinkedToBattlefieldOwners => {
                let cards = self.linked_cards(f.source);
                self.links.retain(|l| l.source != f.source);
                for c in cards {
                    if self.objs[c.i()].zone == Zone::Exile {
                        let owner = self.objs[c.i()].owner;
                        self.put_onto_battlefield(c, owner);
                    }
                }
                Flow::Next
            }
            Ext::BecomeSaddled => {
                if let Some(&c) = self.objs_on(f, &crate::program::On::Source).first() {
                    let r = self.obj_ref(c);
                    let t = self.turn.number as u16;
                    if !self.marks.contains(&(r, crate::game::Mark::Saddled(t))) {
                        self.marks.push((r, crate::game::Mark::Saddled(t)));
                    }
                    self.note(|g| format!("{} becomes saddled", g.name(c)));
                }
                Flow::Next
            }
            Ext::AttachMadeToEventObj => {
                let (Some(e), Some(h)) = (f.made, f.ev.obj) else { return Flow::Next };
                if self.objs[e.i()].zone == Zone::Battlefield && self.live(h) && self.objs[h.idx.i()].zone == Zone::Battlefield {
                    let q = self.damage_source_of(e);
                    if !self.is_creature(e) && self.is_creature(h.idx) && !self.protected_from(h.idx, &q) {
                        self.objs[e.i()].attached = Some(h);
                        self.note(|g| format!("{} attached to {}", g.name(e), g.name(h.idx)));
                    }
                }
                Flow::Next
            }
            _ => unreachable!("not a tail-3 op: {op:?}"),
        }
    }
}
