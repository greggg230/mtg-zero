//! Edge of Eternities, W4 (ENGINE.md 99.14): exile, graveyard, copies and
//! control. The ops W4's cards add, named after the rules actions, each
//! citing its rule; never after a card.

use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::Game;
use crate::program::Who;
use crate::resolve::{Flow, Frame};

/// W4's instructions (`Ext::EoeW4`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeW4 {
    /// "... unless that player discards a card at random" (ward, CR
    /// 702.21a; a random discard, 701.9b): `who` may pay; paid, a card of
    /// their hand chosen at random is discarded and the program continues at
    /// `if_paid`. Choice: `YesNo` (`YES` only with a card in hand; asked
    /// with an empty hand too, as ward's life payment is).
    OfferDiscardRandom { who: Who, if_paid: u16 },
}

impl EoeW4 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeW4::OfferDiscardRandom { who, .. } => matches!(who, Who::EventPlayer),
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            EoeW4::OfferDiscardRandom { if_paid, .. } => Some(if_paid),
        }
    }
}

impl Game {
    pub(crate) fn exec_eoe_w4(&mut self, f: &mut Frame, op: &EoeW4) -> Flow {
        match op {
            EoeW4::OfferDiscardRandom { who, if_paid } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        // A card at random (CR 701.9b): the hand's canonical
                        // order makes the pick independent of insertion order;
                        // the discard's reveal key names the card.
                        let sorted = self.hand_sorted(p);
                        if sorted.is_empty() {
                            return Flow::Next;
                        }
                        let c = match self.forced_hand_pick(p, &sorted) {
                            Some(c) => c,
                            None => sorted[self.rng.below(sorted.len() as u32) as usize],
                        };
                        self.note(|g| format!("P{p} discards {} at random to pay", g.name(c)));
                        self.discard(c);
                        Flow::Jump(*if_paid)
                    }
                    Some(_) => Flow::Next,
                    None => {
                        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                        if !self.players[p as usize].hand.is_empty() {
                            opts.push(Opt { action: act::YES, choice: Choice::Yes });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                        Flow::Suspend
                    }
                }
            }
        }
    }
}
