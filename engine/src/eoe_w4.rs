//! Edge of Eternities, W4 (ENGINE.md 99.14): exile, graveyard, copies and
//! control. The ops W4's cards add, named after the rules actions, each
//! citing its rule; never after a card.

use crate::db::Counter;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::Game;
use crate::ids::{CardIdx, Target, Zone};
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
    /// "Return <targets> from your graveyard to the battlefield. Put a
    /// <counter> on each of them": every legal target of `tgt` still in a
    /// graveyard is put onto the battlefield under the controller's control,
    /// together (one instruction, as Exhume's), then a counter of kind
    /// `counter` is put on each that entered (CR 122.1; after they enter,
    /// not as they enter, 614.1c).
    GraveyardTargetsOntoBattlefield { tgt: u8, counter: Option<Counter> },
}

impl EoeW4 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeW4::OfferDiscardRandom { who, .. } => matches!(who, Who::EventPlayer),
            EoeW4::GraveyardTargetsOntoBattlefield { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            EoeW4::OfferDiscardRandom { if_paid, .. } => Some(if_paid),
            EoeW4::GraveyardTargetsOntoBattlefield { .. } => None,
        }
    }
}

impl Game {
    /// The mana value of a target object (0 for a player or an ability).
    fn target_mv(&self, t: Target) -> u32 {
        match t {
            Target::Obj(r) => self.mana_value_of(r.idx) as u32,
            Target::Player(_) | Target::Stack(_) => 0,
        }
    }

    /// "Up to n targets with total mana value m or less" (CR 601.2c): the
    /// targets are chosen one at a time, and only a candidate that keeps
    /// the chosen ones' total within `total` is offered. That is the
    /// announced set's check: every set so built is within the total, and
    /// every set within it can be built (mana values are never negative, so
    /// each part of a legal set is within the total too, in any order). The
    /// targets are cards in a graveyard, whose mana values nothing changes
    /// there, so the total holds as the spell resolves (608.2b checks each
    /// target on its own).
    pub(crate) fn retain_within_total_mv(&self, total: u8, chosen: &[Target], cands: &mut Vec<Target>) {
        let sum: u32 = chosen.iter().map(|&t| self.target_mv(t)).sum();
        cands.retain(|&t| sum + self.target_mv(t) <= total as u32);
    }

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
            EoeW4::GraveyardTargetsOntoBattlefield { tgt, counter } => {
                let mut entered: Vec<CardIdx> = Vec::new();
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone != Zone::Graveyard {
                        continue;
                    }
                    self.put_onto_battlefield(o.idx, f.controller);
                    if self.objs[o.idx.i()].zone == Zone::Battlefield {
                        entered.push(o.idx);
                    }
                }
                if let Some(k) = counter {
                    for c in entered {
                        self.add_counters(c, *k, 1);
                    }
                }
                Flow::Next
            }
        }
    }
}
