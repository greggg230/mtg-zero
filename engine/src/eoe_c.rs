//! Edge of Eternities lane C (ENGINE.md 99.4): the ops of void, warp, spells
//! cast this turn and the graveyard / exile / library / hand effects the
//! set adds. Named after the rules actions, each citing its rule.

use crate::game::{Game, Pos};
use crate::ids::{Target, Zone};
use crate::program::Who;
use crate::resolve::{Flow, Frame};

/// Lane C's instructions (`Ext::EoeC`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeC {
    /// "Exile all graveyards": every card in every graveyard is exiled at
    /// once (one event group, CR 603.2c).
    ExileAllGraveyards,
    /// "Put target card from a graveyard on the bottom of its owner's
    /// library" (CR 401.4: its owner's library).
    GraveyardCardToBottom { tgt: u8 },
    /// `who` loses `n` life if they have no card in hand ("each opponent
    /// discards a card; each opponent who can't loses n life": before the
    /// discard, who can't is who has no card).
    LoseLifeIfHandEmpty { who: Who, n: u8 },
}

impl Game {
    pub(crate) fn exec_eoe_c(&mut self, f: &mut Frame, op: &EoeC) -> Flow {
        match op {
            EoeC::ExileAllGraveyards => {
                let mut cards = Vec::new();
                for p in [self.turn.active, crate::ids::opp(self.turn.active)] {
                    cards.extend(self.players[p as usize].graveyard.iter().copied());
                }
                for c in cards {
                    if self.objs[c.i()].zone == Zone::Graveyard {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                }
                Flow::Next
            }
            EoeC::GraveyardCardToBottom { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone == Zone::Graveyard {
                            self.move_card(o.idx, Zone::Library, Pos::Bottom);
                        }
                    }
                }
                Flow::Next
            }
            EoeC::LoseLifeIfHandEmpty { who, n } => {
                if let Some(p) = self.who(f, *who) {
                    if self.players[p as usize].hand.is_empty() {
                        self.lose_life(p, *n as i32);
                    }
                }
                Flow::Next
            }
        }
    }
}
