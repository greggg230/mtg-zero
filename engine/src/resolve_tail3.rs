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

impl Game {
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
            _ => unreachable!("not a tail-3 op: {op:?}"),
        }
    }
}
