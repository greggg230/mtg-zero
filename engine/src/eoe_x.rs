//! Edge of Eternities, the cross-lane pass (ENGINE.md 99.8): the ops the
//! cards that needed features of no single lane add. Named after the rules
//! actions, each citing its rule; never after a card.

use crate::db::Filter;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::Game;
use crate::resolve::{Flow, Frame};

/// The cross-lane pass's instructions (`Ext::EoeX`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeX {
    /// The controller searches their library for up to `n` cards matching
    /// `filter` (one search: CR 701.23a; a search for cards with a stated
    /// quality may find fewer, 701.23b), puts them onto the battlefield
    /// (tapped with `tapped`), then shuffles (701.20). Choice: a definition
    /// per card, in non-decreasing order (`PASS` = done), as
    /// `Ext::SearchAnyNumber`.
    SearchUpToOntoBattlefield { filter: Filter, n: u8, tapped: bool },
    /// "<Do something> unless you discard a <filter> card" (CR 701.9a): the
    /// controller may discard a card matching `filter` from their hand; if
    /// they do, continue at `to` (past what they avoided). Choice: a
    /// matching hand card, or `PASS` = don't (no decision without one).
    DiscardMatchingOrContinue { filter: Filter, to: u16 },
}

impl EoeX {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeX::SearchUpToOntoBattlefield { .. } | EoeX::DiscardMatchingOrContinue { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            EoeX::DiscardMatchingOrContinue { to, .. } => Some(to),
            EoeX::SearchUpToOntoBattlefield { .. } => None,
        }
    }
}

impl Game {
    /// The end of `EoeX::SearchUpToOntoBattlefield`: the found cards enter,
    /// then the library is shuffled (`finish_search_any`).
    fn finish_search_up_to(&mut self, f: &mut Frame, tapped: bool) -> Flow {
        let p = f.controller;
        f.picked = self.cards_of_defs(p, &std::mem::take(&mut f.picked_defs));
        f.floor = 0;
        self.finish_search_any(f, p, tapped, false);
        Flow::Next
    }

    pub(crate) fn exec_eoe_x(&mut self, f: &mut Frame, op: &EoeX) -> Flow {
        match op {
            EoeX::SearchUpToOntoBattlefield { filter, n, tapped } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Def(d)) => {
                        f.picked_defs.push(d);
                        f.floor = d;
                    }
                    Some(Choice::Pass) => return self.finish_search_up_to(f, *tapped),
                    Some(other) => unreachable!("search answer {other:?}"),
                    None => {}
                }
                if f.picked_defs.len() >= *n as usize {
                    return self.finish_search_up_to(f, *tapped);
                }
                // One definition per decision, in non-decreasing order (each
                // set of cards once); a definition again while a copy is left.
                let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                let lib = self.library_sorted(p);
                let mut opts = Vec::new();
                for (i, &c) in lib.iter().enumerate() {
                    let d = self.objs[c.i()].def;
                    if i > 0 && self.objs[lib[i - 1].i()].def == d {
                        continue;
                    }
                    let left = lib.iter().filter(|&&x| self.objs[x.i()].def == d).count() > f.picked_defs.iter().filter(|&&x| x == d).count();
                    if d >= f.floor && left && self.card_matches(filter, c, &ctx) {
                        assert!((i as u16) < act::LIBRARY_N, "library slot {i} out of range");
                        opts.push(Opt { action: act::LIBRARY + i as u16, choice: Choice::Def(d) });
                    }
                }
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::Run(f.clone()), p, DecisionKind::SearchLibrary, opts);
                Flow::Suspend
            }
            EoeX::DiscardMatchingOrContinue { filter, to } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} discards {}", g.name(c)));
                        self.discard(c);
                        Flow::Jump(*to)
                    }
                    Some(Choice::Pass) => Flow::Next,
                    Some(other) => unreachable!("discard answer {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                        let mut opts: Vec<Opt> = self
                            .hand_options(p)
                            .into_iter()
                            .filter(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(filter, c, &ctx)))
                            .collect();
                        if opts.is_empty() {
                            return Flow::Next;
                        }
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                        Flow::Suspend
                    }
                }
            }
        }
    }
}
