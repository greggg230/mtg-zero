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
    /// Each object of `on` gains "<delayed trigger `k` of the source's
    /// card>" (CR 613.1f, layer 6: `Mark::Granted`), until end of turn with
    /// `this_turn`, else for as long as it is that object (400.7).
    GrantTrigger { on: On, k: u8, this_turn: bool },
    /// The source deals damage equal to its mana value to each permanent
    /// matching `filter` ("it deals damage equal to its mana value to each
    /// creature"): the source as it is, or as it last existed on the
    /// battlefield (CR 608.2h, 113.7a: its mana value and its qualities as a
    /// source of damage).
    DamageEachSourceMv { filter: Filter },
}

impl W3 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            W3::Regenerate { on } | W3::GrantTrigger { on, .. } => matches!(on, On::EventObj),
            W3::CantBeRegenerated(_) | W3::DamageEachSourceMv { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            W3::Regenerate { .. } | W3::CantBeRegenerated(_) | W3::GrantTrigger { .. } | W3::DamageEachSourceMv { .. } => None,
        }
    }
}

/// Workstream W3's game state (`Game::w3`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W3State {
    /// "It can't be regenerated" holds for the destruction being done (CR
    /// 701.19c): set and cleared around one op.
    pub no_regen: bool,
    /// A fast path: `Game::marks` may hold a `Mark::Granted` (set when one is
    /// made, recomputed as marks are dropped).
    pub granted: bool,
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
            W3::GrantTrigger { on, k, this_turn } => {
                let slot = self.db.def(f.source_def).delayed_base + k;
                for c in self.objs_on(f, on) {
                    let r = self.obj_ref(c);
                    self.marks.push((r, Mark::Granted { def: f.source_def, slot, this_turn: *this_turn }));
                    self.w3.granted = true;
                }
                Flow::Next
            }
            W3::DamageEachSourceMv { filter } => {
                let r = f.source;
                let on_bf = self.live(r) && self.objs[r.idx.i()].zone == crate::ids::Zone::Battlefield;
                let (src, mv) = if on_bf {
                    (crate::game::DamageSource { controller: f.controller, ..self.damage_source_of(r.idx) }, self.mana_value_of(r.idx))
                } else {
                    // (Recorded as it left: `SEEN_LKI_DAMAGE`, `lki_copy`.)
                    let src = match self.lki_damage.iter().rev().find(|x| x.0 == r) {
                        Some(&(_, d)) => d,
                        None => crate::game::DamageSource { controller: f.controller, ..self.damage_source_of(r.idx) },
                    };
                    let mv = match self.lki_copy.iter().rev().find(|x| x.0 == r) {
                        Some((_, v)) => self.db.def(v.def).mana_value(),
                        None => self.def_of(r.idx).mana_value(),
                    };
                    (src, mv)
                };
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(r) };
                let all: Vec<CardIdx> = self.battlefield.iter().copied().filter(|&c| self.card_matches(filter, c, &ctx)).collect();
                for c in all {
                    let t = crate::ids::Target::Obj(self.obj_ref(c));
                    self.deal_damage(src, t, mv as i32);
                }
                Flow::Next
            }
        }
    }

    /// The objects with triggered abilities effects gave them that may see
    /// event `ev` (`Mark::Granted`): (object, definition, trigger slot,
    /// controller). A permanent on the battlefield that hasn't lost all its
    /// abilities (CR 613.1f), or, for a leave-the-battlefield event, one that
    /// left earlier in the same simultaneous move (603.10a).
    pub(crate) fn granted_observers(&self, ev: &crate::triggers::Event) -> Vec<(crate::ids::ObjRef, crate::ids::DefId, u8, PlayerId)> {
        use crate::db::EventKind as K;
        let leave = matches!(ev.kind, K::Dies | K::LeavesBattlefield | K::IsSacrificed);
        let mut out = Vec::new();
        for &(r, m) in &self.marks {
            let Mark::Granted { def, slot, .. } = m else { continue };
            if self.live(r) && self.objs[r.idx.i()].zone == crate::ids::Zone::Battlefield {
                if !self.lost_abilities(r.idx) {
                    out.push((r, def, slot, self.objs[r.idx.i()].controller));
                }
            } else if leave {
                if let Some(l) = self.leaving.iter().find(|l| l.old == r) {
                    out.push((r, def, slot, l.controller));
                }
            }
        }
        out
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
        self.marks.retain(|&(_, m)| !matches!(m, Mark::RegenShield | Mark::Granted { this_turn: true, .. }));
        self.w3.granted = self.marks.iter().any(|&(_, m)| matches!(m, Mark::Granted { .. }));
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
