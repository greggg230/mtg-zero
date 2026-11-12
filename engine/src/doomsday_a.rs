//! Legacy Doomsday, lane A (ENGINE.md 124.1): the rules Doomsday, Pact of
//! Negation and Flow State need (Personal Tutor is Mystical Tutor's search).
//! Named after the rules actions, each citing its rule; never after a card.
//!
//! * A search of a library and a graveyard for a number of cards, the rest of
//!   both exiled, the found cards put on top of the library in an order the
//!   searcher chooses (CR 701.23a, 701.23d: "five cards" has no stated
//!   quality, so as many as possible must be found, up to five; Doomsday's
//!   rulings): [`DdA::PileFromGraveyard`], then [`DdA::PileFromLibrary`], then
//!   the existing `Op::PileTo` to the top for the order.
//! * Losing half one's life, rounded up (CR 107.1a; a negative result is 0,
//!   107.1b): [`DdA::LoseHalfLifeUp`].
//! * An effect that says a player loses the game (CR 104.3e):
//!   [`DdA::LoseGame`].
//! * "If there is an instant card and a sorcery card in your graveyard"
//!   (read as the instruction is carried out, CR 608.2c):
//!   [`DdA::JumpUnlessInstantAndSorceryInGraveyard`].

use crate::db::{types, Filter};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{EndReason, Game, Pos};
use crate::ids::{CardIdx, DefId, PlayerId, Zone};
use crate::resolve::{Flow, Frame};

/// Lane A's instructions (`Ext::DdA`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum DdA {
    /// The first part of "search your library and graveyard for `n` cards
    /// and exile the rest; put the chosen cards on top of your library in
    /// any order": which graveyard cards are among the `n`. As many cards as
    /// possible are found (CR 701.23d): `t = min(n, library + graveyard)`,
    /// of which between `t - library` and `min(t, graveyard)` from the
    /// graveyard. Choice: `ChooseGraveyardCard`, one card per decision in
    /// increasing graveyard position (each set once; a pick must leave
    /// enough cards above it for the minimum), `PASS` = no more from the
    /// graveyard once the minimum is reached. The graveyard is public, so
    /// the options are the same in every world (the library's size is
    /// public). The picks wait in `Frame::picked`.
    PileFromGraveyard { n: u8 },
    /// The second part: the rest of the `n` from the library, then the
    /// exile and the move. Choice: `SearchLibrary`, one definition per
    /// decision (a hidden zone: the definition, at the slot of its first
    /// card in the library sorted by definition, ENGINE.md 10), in
    /// non-decreasing order (each multiset once), a definition only while
    /// a copy is left and enough cards from it on remain for the picks
    /// still needed; the picks wait in `Frame::picked_defs` (the searcher's
    /// own worlds may deal the library's cards again before the search
    /// ends). Then the rest of the library (in definition order: its order
    /// was hidden) and of the graveyard are exiled, the graveyard's picks
    /// join the library (everyone sees them go), and the found cards become
    /// the frame's pile, known to the searcher, for `Op::PileTo` to put on
    /// top in the order chosen. The other player loses track of which
    /// library cards were found and of the order: what they knew of those
    /// cards (the graveyard's picks, a library card they knew that wasn't
    /// exiled) becomes "among the library" (a cloud).
    PileFromLibrary { n: u8 },
    /// The controller loses half their life, rounded up (CR 107.1a, 119.3);
    /// none at 0 life or less (107.1b: a negative result is 0).
    LoseHalfLifeUp,
    /// The controller loses the game (CR 104.3e); in a two-player game the
    /// other player wins (104.2a).
    LoseGame,
    /// Unless the controller's graveyard holds an instant card and a sorcery
    /// card, continue at `to`.
    JumpUnlessInstantAndSorceryInGraveyard { to: u16 },
}

impl DdA {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            DdA::PileFromGraveyard { .. }
            | DdA::PileFromLibrary { .. }
            | DdA::LoseHalfLifeUp
            | DdA::LoseGame
            | DdA::JumpUnlessInstantAndSorceryInGraveyard { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            DdA::JumpUnlessInstantAndSorceryInGraveyard { to } => Some(to),
            DdA::PileFromGraveyard { .. } | DdA::PileFromLibrary { .. } | DdA::LoseHalfLifeUp | DdA::LoseGame => None,
        }
    }
}

/// How many cards a search of `p`'s library and graveyard for `n` cards
/// finds (CR 701.23d), and the bounds on how many come from the graveyard.
fn pile_bounds(g: &Game, p: PlayerId, n: u8) -> (usize, usize, usize) {
    let lib = g.player(p).library.len();
    let gy = g.player(p).graveyard.len();
    let t = (n as usize).min(lib + gy);
    (t, t.saturating_sub(lib), t.min(gy))
}

impl Game {
    pub(crate) fn exec_dd_a(&mut self, f: &mut Frame, op: &DdA) -> Flow {
        match *op {
            DdA::PileFromGraveyard { n } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        f.picked.push(c);
                        f.i += 1;
                    }
                    Some(Choice::Pass) => return Flow::Next,
                    Some(other) => unreachable!("graveyard pick answer {other:?}"),
                    None => {
                        if f.i == 0 {
                            f.picked.clear();
                        }
                    }
                }
                let (_, min_gy, max_gy) = pile_bounds(self, p, n);
                let k = f.picked.len();
                if k >= max_gy {
                    return Flow::Next;
                }
                let gy = self.player(p).graveyard.clone();
                let start = match f.picked.last() {
                    Some(&c) => gy.iter().position(|&x| x == c).expect("a picked graveyard card stays there") + 1,
                    None => 0,
                };
                let mut opts: Vec<Opt> = Vec::new();
                for (j, &c) in gy.iter().enumerate().skip(start) {
                    // (Enough cards above it for the picks the minimum
                    // still needs.)
                    if k + (gy.len() - j) >= min_gy {
                        opts.push(Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) });
                    }
                }
                if k >= min_gy {
                    opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                }
                if opts.is_empty() {
                    return Flow::Next;
                }
                self.ask_object_opts(Task::Run(f.clone()), f.page, p, DecisionKind::ChooseGraveyardCard, opts);
                Flow::Suspend
            }
            DdA::PileFromLibrary { n } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Def(d)) => {
                        f.picked_defs.push(d);
                        f.floor = d;
                        f.i += 1;
                    }
                    Some(Choice::Pass) => return self.dd_pile_found(f, p),
                    Some(other) => unreachable!("library pick answer {other:?}"),
                    None => {
                        if f.i == 0 {
                            f.picked_defs.clear();
                        }
                    }
                }
                let (t, _, _) = pile_bounds(self, p, n);
                let need = t.saturating_sub(f.picked.len()).saturating_sub(f.picked_defs.len());
                if need == 0 {
                    return self.dd_pile_found(f, p);
                }
                let lib = self.library_sorted(p);
                let def = |c: CardIdx| self.objs[c.i()].def;
                let mut opts: Vec<Opt> = Vec::new();
                for (i, &c) in lib.iter().enumerate() {
                    let d = def(c);
                    if (i > 0 && def(lib[i - 1]) == d) || d < f.floor {
                        continue;
                    }
                    let copies = lib.iter().filter(|&&x| def(x) == d).count();
                    let taken = f.picked_defs.iter().filter(|&&x| x == d).count();
                    // (Cards from this definition on, not yet found: the
                    // picks still needed must fit.)
                    let from_here = lib.len() - i - f.picked_defs.iter().filter(|&&x| x >= d).count();
                    if copies > taken && from_here >= need {
                        assert!((i as u16) < act::LIBRARY_N, "library slot {i} out of range");
                        opts.push(Opt { action: act::LIBRARY + i as u16, choice: Choice::Def(d) });
                    }
                }
                if opts.is_empty() {
                    // (Never in the searcher's own world: its picks were made
                    // from this library. Another player's world may deal the
                    // library cards those picks named elsewhere; the search
                    // then finds what it can, `PASS`.)
                    opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                }
                self.ask(Task::Run(f.clone()), p, DecisionKind::SearchLibrary, opts);
                Flow::Suspend
            }
            DdA::LoseHalfLifeUp => {
                let p = f.controller;
                let life = self.players[p as usize].life as crate::num::Wide;
                // (CR 107.1a: rounded up; 107.1b: a negative half is 0.)
                let half = if life > 0 { (life + 1) / 2 } else { 0 };
                self.note(|_| format!("P{p} loses half their life, rounded up: {half}"));
                self.lose_life(p, half);
                Flow::Next
            }
            DdA::LoseGame => {
                let p = f.controller;
                self.note(|_| format!("P{p} loses the game"));
                self.players[p as usize].lost = true;
                self.end_game(Some(crate::ids::opp(p)), EndReason::Effect);
                Flow::Next
            }
            DdA::JumpUnlessInstantAndSorceryInGraveyard { to } => {
                let p = f.controller;
                let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                let has = |bits: u16| self.player(p).graveyard.iter().any(|&c| self.card_matches(&Filter::Type(bits), c, &ctx));
                if has(types::INSTANT) && has(types::SORCERY) {
                    Flow::Next
                } else {
                    Flow::Jump(to)
                }
            }
        }
    }

    /// The end of a library and graveyard search for a pile
    /// ([`DdA::PileFromLibrary`]): exile the rest of both, put the found
    /// graveyard cards into the library, and make the found cards the
    /// frame's pile.
    fn dd_pile_found(&mut self, f: &mut Frame, p: PlayerId) -> Flow {
        let lib_found: Vec<CardIdx> = self.cards_of_defs(p, &std::mem::take(&mut f.picked_defs));
        let gy_found: Vec<CardIdx> = std::mem::take(&mut f.picked);
        // Which library cards were found is private: the other player loses
        // track of the library (ENGINE.md 10; as `finish_search`).
        self.scramble(p, &[], false, true);
        // "Exile the rest" (one instruction): the library's other cards in
        // definition order (its order was hidden and stays so), then the
        // graveyard's in their order.
        let lib_rest: Vec<CardIdx> = self.library_sorted(p).into_iter().filter(|c| !lib_found.contains(c)).collect();
        let gy_rest: Vec<CardIdx> = self.player(p).graveyard.iter().copied().filter(|c| !gy_found.contains(c)).collect();
        for c in lib_rest {
            if self.objs[c.i()].zone == Zone::Library {
                self.move_card(c, Zone::Exile, Pos::Top);
            }
        }
        for c in gy_rest {
            if self.objs[c.i()].zone == Zone::Graveyard {
                self.move_card(c, Zone::Exile, Pos::Top);
            }
        }
        self.new_event();
        // "Put the chosen cards on top of your library in any order": the
        // graveyard's join the library (a public move: everyone sees them
        // go), then the order is chosen privately (`Op::PileTo`).
        for &c in &gy_found {
            if self.objs[c.i()].zone == Zone::Graveyard {
                self.move_card(c, Zone::Library, Pos::Top);
            }
        }
        let mut pile: Vec<CardIdx> = lib_found.iter().chain(gy_found.iter()).copied().filter(|&c| self.objs[c.i()].zone == Zone::Library).collect();
        // Until the order is chosen, the found cards wait on top in
        // definition order: where the library's found cards were depended on
        // the library's hidden order, which the searcher's own worlds deal
        // anew (no position may carry it into what the searcher knows).
        pile.sort_by_key(|&c| (self.objs[c.i()].def, c.get()));
        for &c in &pile {
            self.move_card(c, Zone::Library, Pos::Top);
        }
        // The searcher knows each found card (it chose them); the other
        // player knows only that what it knew of them is among them.
        for &c in &pile {
            self.learn(p, c);
        }
        self.scramble(p, &pile, false, false);
        let found: Vec<DefId> = pile.iter().map(|&c| self.objs[c.i()].def).collect();
        self.note(|_| format!("P{p} finds {} card(s) and exiles the rest", found.len()));
        f.pile = pile;
        Flow::Next
    }
}
