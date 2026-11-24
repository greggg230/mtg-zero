//! Lane B of the Legacy Lands cards (ENGINE.md 270.2): dredge (CR 702.52),
//! a replacement of a card draw. Named after the rules, never after a card.
//! (Yavimaya's land type is EoE W3's subtype grant, `Game::granted_subtype_ts`;
//! Exploration's additional land is `CardDef::extra_lands`; Carnivorous
//! Cultivator // Enroot is a preparation card, ENGINE.md 26.7.)

/// The printing facts lane B adds (`CardDef::lands_b`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct LandsBDef {
    /// "Dredge N" (CR 702.52a): N, 0 = none. A static ability of the card
    /// while it is in a graveyard.
    pub dredge: u8,
}

use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, PlayerId, Zone};

impl Game {
    /// The cards in `p`'s graveyard whose dredge ability could replace a draw
    /// of `p`'s now (CR 702.52a: a static ability of the card while it is in
    /// its owner's graveyard - "your graveyard", 702.52a; 702.52b: not with
    /// fewer than N cards in `p`'s library), bottom first.
    pub(crate) fn dredgers(&self, p: PlayerId) -> Vec<CardIdx> {
        if !self.db.dredge {
            return Vec::new();
        }
        let lib = self.players[p as usize].library.len();
        self.players[p as usize]
            .graveyard
            .iter()
            .copied()
            .filter(|&c| {
                let n = self.def_of(c).lands_b.dredge as usize;
                n > 0 && n <= lib
            })
            .collect()
    }

    /// Is a dredge question due before `p`'s next draw: a dredge ability that
    /// could replace it, the draw allowed at all (a draw "each opponent can't
    /// draw" is not replaced: Narset's and Leovold's rulings)?
    pub(crate) fn dredge_due(&self, p: PlayerId) -> bool {
        self.db.dredge && self.turn.number > 0 && !self.dredgers(p).is_empty() && !self.draw_disallowed(p)
    }

    /// Queue the dredge question about `p`'s next draw, then the `left`
    /// draws of its instruction and, `step`, the draw step's beginning event
    /// (`Task::DrawRest`), in the current event group (CR 121.6b: a draw
    /// replaced within a sequence of draws is completed before the sequence
    /// resumes).
    pub(crate) fn queue_dredge(&mut self, p: PlayerId, left: u32, step: bool) {
        if left > 0 || step {
            self.agenda.push(Task::DrawRest { p, n: left, ev: self.ev_seq, step });
        }
        self.agenda.push(Task::Dredge { p, ev: self.ev_seq, step: false, inst: false });
    }

    /// `Task::Dredge`: `p` chooses whether a dredge ability of a card in
    /// their graveyard replaces their next draw (CR 702.52a, 616.1: the
    /// drawer applies the replacements of their draw in the order they
    /// choose; one draw is replaced by one dredge ability at most, the
    /// card's ruling). `ChooseGraveyardCard`: `LIBRARY` + the card's slot in
    /// the graveyard list = that card's dredge; `PASS` = not to dredge (the
    /// draw goes on: its other replacements, then the draw; `inst`: the
    /// instruction's replacements, then its draws, each asked again).
    pub(crate) fn dredge_choice(&mut self, p: PlayerId, ev: u32, step: bool, inst: bool) {
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                let saved = self.ev_seq;
                self.ev_seq = ev;
                self.dredge(p, c);
                if inst && step {
                    self.event(crate::triggers::Event::player(crate::db::EventKind::BeginningOfDrawStep, p));
                }
                self.ev_seq = saved;
            }
            Some(Choice::Pass) => {
                let saved = self.ev_seq;
                self.ev_seq = ev;
                if inst {
                    let cut = self.draw_instruction_replaced(p, 1, step);
                    self.draw_cut(p, cut, step);
                } else if let Some(card) = self.draw(p) {
                    // (The miracle question, before the instruction's draws
                    // left: the `Task::DrawRest` under this task.)
                    self.queue_miracle(p, card, 0, false);
                }
                self.ev_seq = saved;
            }
            Some(other) => unreachable!("dredge answer {other:?}"),
            None => {
                let cards = self.dredgers(p);
                assert!(!cards.is_empty() && !self.draw_disallowed(p), "a dredge question with no dredge ability that applies");
                let mut opts: Vec<Opt> = cards.into_iter().map(|c| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) }).collect();
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::Dredge { p, ev, step, inst }, p, DecisionKind::ChooseGraveyardCard, opts);
            }
        }
    }

    /// Dredge N with card `c` in `p`'s graveyard (CR 702.52a): instead of
    /// drawing, `p` mills N cards and returns `c` from their graveyard to
    /// their hand (a public move: everyone saw the card). Nothing else
    /// happens between (the card's ruling: no player acts until the card is
    /// in the hand and the cards milled). Not a draw: no card is drawn
    /// (no "draws a card" event, the turn's draw count unchanged; 614.6).
    pub(crate) fn dredge(&mut self, p: PlayerId, c: CardIdx) {
        let n = self.def_of(c).lands_b.dredge as usize;
        debug_assert!(self.objs[c.i()].zone == Zone::Graveyard && self.objs[c.i()].owner == p && n <= self.players[p as usize].library.len());
        self.note(|g| format!("P{p} dredges {} ({n}) instead of drawing", g.name(c)));
        self.mill(p, n, false);
        if self.objs[c.i()].zone == Zone::Graveyard {
            self.move_card(c, Zone::Hand, Pos::Top);
        }
    }
}
