//! The `Ext` instructions of the single-card tail, part 2 (M7c, ENGINE.md
//! 29).

use crate::db::types;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, PlayerId, Target, Zone};
use crate::program::{Ext, On};
use crate::resolve::{Flow, Frame};

/// `Permission::turn` with this bit: "until the end of your next turn", the
/// low bits the permission player's turn count it ends with.
pub const PERM_UNTIL_TURNS: u32 = 1 << 31;

fn yes_no() -> Vec<Opt> {
    vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }]
}

impl Game {
    pub(crate) fn exec_tail2(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::SacrificeN { who, filter, n } => self.sacrifice_n(f, *who, filter, *n),
            Ext::FromGraveyardAs { tgt, mods } => {
                f.made = None;
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone != Zone::Graveyard {
                        continue;
                    }
                    self.entering_mods = mods.clone();
                    let r = self.put_onto_battlefield(o.idx, f.controller);
                    self.entering_mods.clear();
                    if self.objs[r.idx.i()].zone == Zone::Battlefield {
                        f.made = Some(r.idx);
                    }
                }
                Flow::Next
            }
            Ext::ReturnEventCardAs { mods } => {
                f.made = None;
                if let Some(r) = f.ev.obj {
                    let o = &self.objs[r.idx.i()];
                    // The card where it went (a leave trigger's own event),
                    // or, for "when this dies", the card it became in the
                    // graveyard (one move later).
                    let here = self.live(r) && matches!(o.zone, Zone::Exile | Zone::Graveyard)
                        || o.zone == Zone::Graveyard && o.inc == r.inc.wrapping_add(1);
                    if here && !self.is_token(r.idx) {
                        let owner = self.objs[r.idx.i()].owner;
                        self.entering_mods = mods.clone();
                        self.put_onto_battlefield(r.idx, owner);
                        self.entering_mods.clear();
                        f.made = Some(r.idx);
                    }
                }
                Flow::Next
            }
            Ext::Harness => {
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    let r = self.obj_ref(c);
                    if !self.marks.contains(&(r, crate::game::Mark::Harnessed)) {
                        self.marks.push((r, crate::game::Mark::Harnessed));
                        self.note(|g| format!("{} is harnessed", g.name(c)));
                    }
                }
                Flow::Next
            }
            Ext::ExilePlayUntilNextTurn { tgt } => {
                let p = f.controller;
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone != Zone::Graveyard {
                        continue;
                    }
                    let card = self.move_card(o.idx, Zone::Exile, Pos::Top);
                    // "Until the end of your next turn" (the turn count it
                    // ends with: this player's next turn).
                    let until = self.players[p as usize].turns + 1;
                    self.permissions.push(crate::game::Permission {
                        card,
                        player: p,
                        turn: Some(PERM_UNTIL_TURNS | until),
                        kind: crate::game::PermKind::Play { free: false, cast_only: false, any_mana: false, attacked_subtype: None },
                    });
                }
                Flow::Next
            }
            Ext::ExileEventCardLinked { optional } => {
                let p = f.controller;
                let Some(r) = f.ev.obj else { return Flow::Next };
                if !self.live(r) || self.objs[r.idx.i()].zone != Zone::Graveyard || self.objs[r.idx.i()].owner != p {
                    return Flow::Next;
                }
                if *optional {
                    match self.answer.take() {
                        Some(Choice::Yes) => {}
                        Some(_) => return Flow::Next,
                        None => {
                            self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, yes_no());
                            return Flow::Suspend;
                        }
                    }
                }
                let card = self.move_card(r.idx, Zone::Exile, Pos::Top);
                self.links.push(crate::game::Link { source: f.source, card, until: false, back: Zone::Graveyard, until_monarch: None });
                Flow::Next
            }
            Ext::LinkedToGraveyardThenToken { land, nonland } => {
                let p = f.controller;
                f.made = None;
                let pick = match self.answer.take() {
                    Some(Choice::Card(c)) => c,
                    Some(other) => unreachable!("linked card answer {other:?}"),
                    None => {
                        let cands = self.linked_cards(f.source);
                        if cands.is_empty() {
                            return Flow::Next;
                        }
                        let opts = cands
                            .iter()
                            .take(act::EXILE_N as usize)
                            .enumerate()
                            .map(|(k, &c)| Opt { action: act::EXILE + k as u16, choice: Choice::Card(c) })
                            .collect();
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseExiled, opts);
                        return Flow::Suspend;
                    }
                };
                let is_land = self.def_of(pick).is(types::LAND);
                self.move_card(pick, Zone::Graveyard, Pos::Top);
                let token = if is_land { *land } else { *nonland };
                let def = self.db.token_def(token);
                let made = self.create_tokens(def, p, 1, false);
                f.made = made.first().copied();
                f.made_all = made;
                Flow::Next
            }
            other => unreachable!("not a tail-2 op: {other:?}"),
        }
    }

    /// The cards exiled with `source` still there (CR 607.2a), in link
    /// order.
    pub(crate) fn linked_cards(&self, source: crate::ids::ObjRef) -> Vec<CardIdx> {
        self.links
            .iter()
            .filter(|l| l.source == source && self.live(l.card) && self.objs[l.card.idx.i()].zone == Zone::Exile)
            .map(|l| l.card.idx)
            .collect()
    }

    /// Is a permission's turn bound still open for `player` now (`turn`
    /// exact, or `PERM_UNTIL_TURNS`: through the end of their k-th turn)?
    pub(crate) fn perm_turn_ok(&self, turn: Option<u32>, player: PlayerId) -> bool {
        match turn {
            None => true,
            Some(t) if t & PERM_UNTIL_TURNS != 0 => {
                let k = t & !PERM_UNTIL_TURNS;
                let n = self.players[player as usize].turns;
                n < k || n == k && self.turn.active == player
            }
            Some(t) => t == self.turn.number,
        }
    }

    /// `who` sacrifices n permanents matching `filter` (a set chosen one
    /// permanent per decision in increasing slot order, then sacrificed at
    /// once, CR 701.21a). `f.picked`: the permanents chosen so far.
    fn sacrifice_n(&mut self, f: &mut Frame, who: crate::program::Who, filter: &crate::db::Filter, n: u8) -> Flow {
        let Some(p) = self.who(f, who) else { return Flow::Next };
        if self.shielded_from(p, f.controller) {
            return Flow::Next;
        }
        match self.answer.take() {
            Some(Choice::Card(c)) => f.picked.push(c),
            Some(other) => unreachable!("sacrifice answer {other:?}"),
            None => {}
        }
        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
        let all: Vec<CardIdx> = self.permanents_of(p).filter(|&c| self.card_matches(filter, c, &ctx)).collect();
        let done = if all.len() <= n as usize {
            // No choice: all of them.
            all
        } else if f.picked.len() >= n as usize {
            std::mem::take(&mut f.picked)
        } else {
            let floor = f.picked.iter().map(|&c| self.perm_action(p, c)).max();
            let mut acts: Vec<(u16, CardIdx)> = all
                .iter()
                .filter(|c| !f.picked.contains(c))
                .map(|&c| (self.perm_action(p, c), c))
                .filter(|&(a, _)| floor.is_none_or(|fl| a > fl))
                .collect();
            acts.sort();
            // Leave enough after this pick to reach n.
            let still = n as usize - f.picked.len();
            let len = acts.len();
            let opts: Vec<Opt> = acts
                .into_iter()
                .enumerate()
                .filter(|&(k, _)| len - k >= still)
                .map(|(_, (a, c))| Opt { action: a, choice: Choice::Card(c) })
                .collect();
            self.ask(Task::Run(f.clone()), p, DecisionKind::Sacrifice, opts);
            return Flow::Suspend;
        };
        f.picked.clear();
        if !done.is_empty() {
            self.begin_batch();
            for c in done {
                if self.objs[c.i()].zone == Zone::Battlefield {
                    self.note(|g| format!("P{p} sacrifices {}", g.name(c)));
                    self.sacrifice(c);
                }
            }
            self.end_batch();
        }
        Flow::Next
    }
}
