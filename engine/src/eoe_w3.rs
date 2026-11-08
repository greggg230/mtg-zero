//! Edge of Eternities, workstream W3 (ENGINE.md 99.13): combat and
//! abilities. The ops, costs and rules the set's last combat cards add:
//! named after the rules actions, each citing its rule; never after a card.

use crate::db::{Cost, Counter, Filter};
use crate::game::Game;
use crate::ids::{CardIdx, PlayerId};

impl Game {
    /// May card `c` (in a graveyard, or a card a cost is about to put there)
    /// pay `cost`'s "exile <n> <type> cards from your graveyard": its
    /// printed card types (a card in a graveyard has its printed
    /// characteristics, CR 400.7).
    pub(crate) fn gy_cost_card(&self, cost: &Cost, c: CardIdx) -> bool {
        cost.exile_graveyard_types == 0 || self.def_of(c).types & cost.exile_graveyard_types != 0
    }

    /// The permanents `p` controls from which `card`'s cost "remove a <k>
    /// counter from a <f> you control" may remove one (CR 118.3): those
    /// matching `f` with such a counter (the source itself too).
    pub(crate) fn remove_counter_candidates(&self, p: PlayerId, card: CardIdx, f: &Filter, k: Counter) -> Vec<CardIdx> {
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
        self.permanents_of(p).filter(|&c| self.counters_on(c, k) > 0 && self.card_matches(f, c, &ctx)).collect()
    }
}

impl Game {
    /// The X of stack item `id` when `cost` says "tap X untapped <permanents>
    /// you control" (the permanents owed), else 0.
    pub(crate) fn tap_x_n(&self, id: u32, cost: &Cost) -> usize {
        if cost.tap_x.is_none() {
            return 0;
        }
        self.item_index(id).map_or(0, |pos| self.stack[pos].x as usize)
    }
}
