//! Edge of Eternities, workstream W3 (ENGINE.md 99.13): combat and
//! abilities. The ops, costs and rules the set's last combat cards add:
//! named after the rules actions, each citing its rule; never after a card.

use crate::db::{Cost, Counter, Filter};
use crate::game::{Game, Mark};
use crate::ids::{CardIdx, PlayerId};
use crate::program::On;
use crate::resolve::{Flow, Frame};

/// Workstream W3's instructions (`Ext::EoeW3`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum W3 {
    /// Regenerate each object of `on` (CR 701.19a): a regeneration shield
    /// on it (`Mark::RegenShield`) until the turn ends.
    Regenerate { on: On },
    /// "It can't be regenerated" (CR 701.19c) for the destructions of the
    /// next op: set (true) before it, cleared (false) after.
    CantBeRegenerated(bool),
}

impl W3 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            W3::Regenerate { on } => matches!(on, On::EventObj),
            W3::CantBeRegenerated(_) => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            W3::Regenerate { .. } | W3::CantBeRegenerated(_) => None,
        }
    }
}

/// Workstream W3's game state (`Game::w3`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W3State {
    /// "It can't be regenerated" holds for the destruction being done (CR
    /// 701.19c): set and cleared around one op.
    pub no_regen: bool,
}

impl Game {
    pub(crate) fn exec_eoe_w3(&mut self, f: &mut Frame, op: &W3) -> Flow {
        match op {
            W3::Regenerate { on } => {
                for c in self.objs_on(f, on) {
                    let r = self.obj_ref(c);
                    self.marks.push((r, Mark::RegenShield));
                    self.note(|g| format!("a regeneration shield on {}", g.name(c)));
                }
                Flow::Next
            }
            W3::CantBeRegenerated(on) => {
                self.w3.no_regen = *on;
                Flow::Next
            }
        }
    }

    /// Permanent `c` would be destroyed: a regeneration shield on it
    /// replaces the destruction (CR 701.19a, 614.8: "instead remove all
    /// damage marked on it and its controller taps it; if it's an attacking
    /// or blocking creature, remove it from combat"), using the shield up;
    /// not if it can't be regenerated (701.19c). Did one?
    pub(crate) fn regenerated(&mut self, c: CardIdx) -> bool {
        if self.w3.no_regen {
            return false;
        }
        let r = self.obj_ref(c);
        let Some(i) = self.marks.iter().position(|&(o, m)| o == r && m == Mark::RegenShield) else { return false };
        self.marks.remove(i);
        let o = &mut self.objs[c.i()];
        o.damage = 0;
        o.deathtouched = false;
        // CR 506.4: removed from combat.
        o.attacking = false;
        o.blocked = false;
        o.blocking = None;
        o.attacks = None;
        self.tap(c);
        self.note(|g| format!("{} regenerates", g.name(c)));
        true
    }

    /// The "this turn" facts of workstream W3 end in the cleanup step (CR
    /// 514.2): regeneration shields.
    pub(crate) fn end_turn_w3(&mut self) {
        self.marks.retain(|&(_, m)| m != Mark::RegenShield);
    }
}

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

/// The printing facts of workstream W3 (`CardDef::w3`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W3Def {
    /// "Combat damage can't be prevented" (CR 615.12): no combat damage
    /// is, while it is on the battlefield (Frenzied Baloth).
    pub combat_unpreventable_all: bool,
    /// "Creature spells you control can't be countered" (CR 101.2).
    pub creature_spells_uncounterable: bool,
}

impl Game {
    /// Does a permanent say "combat damage can't be prevented" (CR 615.12)?
    pub(crate) fn all_combat_unpreventable(&self) -> bool {
        self.battlefield.iter().any(|&c| self.abilities(c).w3.combat_unpreventable_all)
    }
}
