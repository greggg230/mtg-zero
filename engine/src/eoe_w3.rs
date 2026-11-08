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
    /// The permanent the source is attached to ("enchanted creature"; as the
    /// source last existed if it left, CR 608.2h) deals damage equal to its
    /// power to each legal target of requirement `tgt` (CR 120.3: it is the
    /// source of the damage).
    HostDamagesPower { tgt: u8 },
    /// The triggered ability's event object ("it", as it last existed if it
    /// left, CR 608.2h) deals `amount` damage to each opponent of the
    /// controller (CR 120.3).
    EventObjDamagesEachOpponent { amount: crate::program::Amount },
    /// "You may remove a <counter> counter from each of `n` <filter>s you
    /// control. If you do, <what follows>" (CR 122.8 removal, 608.2d): the
    /// controller chooses `n` different such permanents (a set, increasing
    /// slot order; `PASS` at the first = don't), then one counter is
    /// removed from each; not done (or impossible: fewer than `n` with
    /// such a counter, no question), continue at `skip`. Choice:
    /// `ChoosePermanent`.
    RemoveCounterFromEachMay { filter: Filter, counter: Counter, n: u8, skip: u16 },
}

impl W3 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            W3::Regenerate { on } | W3::GrantTrigger { on, .. } => matches!(on, On::EventObj),
            W3::CantBeRegenerated(_) | W3::DamageEachSourceMv { .. } | W3::HostDamagesPower { .. } => false,
            W3::EventObjDamagesEachOpponent { .. } => true,
            W3::RemoveCounterFromEachMay { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            W3::Regenerate { .. }
            | W3::CantBeRegenerated(_)
            | W3::GrantTrigger { .. }
            | W3::DamageEachSourceMv { .. }
            | W3::HostDamagesPower { .. }
            | W3::EventObjDamagesEachOpponent { .. } => None,
            W3::RemoveCounterFromEachMay { skip, .. } => Some(skip),
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
    /// `SEEN_W3_*` bits: a card of this game needs the rule (as
    /// `Game::seen`).
    pub seen: u8,
    /// Last known information: the permanent each attached object that left
    /// the battlefield this turn was attached to (CR 608.2h: "enchanted
    /// creature" of an Aura gone; kept with `SEEN_W3_HOST`).
    pub lki_host: Vec<(crate::ids::ObjRef, crate::ids::ObjRef)>,
    /// The amount of mana spent to cast the spell becoming a permanent:
    /// set and taken around one move.
    pub entering_spent: Option<u8>,
}

/// The abilities of a definition that go on the stack (its spell, triggered
/// and activated abilities).
fn abilities_of(d: &crate::db::CardDef) -> impl Iterator<Item = &crate::db::AbilityDef> {
    d.triggers.iter().map(|t| &t.ability).chain(d.spell.iter()).chain(d.activated.iter().filter_map(|a| match &a.effect {
        crate::db::ActEffect::Stack(ab) => Some(ab),
        crate::db::ActEffect::Mana(_) => None,
    }))
}

/// Does a target of the definition read the amount of mana spent to cast
/// its source (`Filter::MvAtMostManaSpent`)?
pub(crate) fn reads_mana_spent(d: &crate::db::CardDef) -> bool {
    fn has(f: &Filter) -> bool {
        match f {
            Filter::MvAtMostManaSpent => true,
            Filter::And(a, b) => has(a) || has(b),
            Filter::Or(v) => v.iter().any(has),
            Filter::Not(a) | Filter::InGraveyard(a) | Filter::Spell(a) | Filter::AttachedTo(a) => has(a),
            _ => false,
        }
    }
    abilities_of(d).any(|ab| ab.targets.iter().any(|t| has(&t.filter)))
}

/// `W3State::seen`: a card reads its host as it last existed.
pub(crate) const SEEN_W3_HOST: u8 = 1;

/// The `SEEN_W3_*` bits of a definition (`W3Def::seen`, set as it is
/// lowered).
pub(crate) fn seen_of(d: &crate::db::CardDef) -> u8 {
    let mut bits = 0;
    for ab in abilities_of(d) {
        for op in &ab.program {
            if let crate::program::Op::Ext(crate::program::Ext::EoeW3(W3::HostDamagesPower { .. })) = op {
                bits |= SEEN_W3_HOST;
            }
        }
    }
    bits
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
            W3::HostDamagesPower { tgt } => {
                let s = f.source;
                let host = if self.live(s) && self.objs[s.idx.i()].zone == crate::ids::Zone::Battlefield {
                    self.objs[s.idx.i()].attached
                } else {
                    self.w3.lki_host.iter().rev().find(|x| x.0 == s).map(|x| x.1)
                };
                let Some(h) = host else { return Flow::Next };
                let (src, power) = self.lki_source(h, f.controller);
                let src = crate::game::DamageSource { controller: src.controller, ..src };
                for t in self.targets_of(f, *tgt) {
                    self.deal_damage(src, t, power);
                }
                Flow::Next
            }
            W3::EventObjDamagesEachOpponent { amount } => {
                let Some(r) = f.ev.obj else { return Flow::Next };
                let n = self.amount(f, *amount);
                let (src, _) = self.lki_source(r, f.controller);
                let opp = crate::ids::opp(f.controller);
                self.deal_damage(src, crate::ids::Target::Player(opp), n);
                Flow::Next
            }
            W3::RemoveCounterFromEachMay { filter, counter, n, skip } => {
                use crate::decision::{act, Choice, DecisionKind, Opt};
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => f.picked.push(c),
                    Some(Choice::Pass) => {
                        f.picked.clear();
                        return Flow::Jump(*skip);
                    }
                    Some(other) => unreachable!("remove-counters answer {other:?}"),
                    None => {}
                }
                let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                let cands: Vec<CardIdx> =
                    self.permanents_of(p).filter(|&c| self.counters_on(c, *counter) > 0 && self.card_matches(filter, c, &ctx)).collect();
                let n = *n as usize;
                if f.picked.is_empty() && cands.len() < n {
                    return Flow::Jump(*skip);
                }
                if f.picked.len() < n {
                    let floor = f.picked.last().map(|&c| self.perm_action(p, c));
                    let rest: Vec<Opt> = cands
                        .iter()
                        .filter(|c| !f.picked.contains(c))
                        .map(|&c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                        .filter(|o| floor.is_none_or(|fl| o.action > fl))
                        .collect();
                    // (Leaving enough later ones for the picks still owed.)
                    let still = n - f.picked.len();
                    let mut opts: Vec<Opt> = rest.iter().enumerate().filter(|(k, _)| rest.len() - k >= still).map(|(_, o)| *o).collect();
                    if f.picked.is_empty() {
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                    }
                    opts.sort_by_key(|o| o.action);
                    self.ask(crate::flow::Task::Run(f.clone()), p, DecisionKind::ChoosePermanent, opts);
                    return Flow::Suspend;
                }
                for c in std::mem::take(&mut f.picked) {
                    if self.objs[c.i()].zone == crate::ids::Zone::Battlefield {
                        self.remove_counters(c, *counter, 1);
                    }
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

    /// `n` more mana was spent to cast spell `card` (CR 601.2h).
    pub(crate) fn note_mana_spent(&mut self, card: CardIdx, n: usize) {
        let r = self.obj_ref(card);
        match self.marks.iter_mut().find(|(o, m)| *o == r && matches!(m, Mark::ManaSpent(_))) {
            Some((_, Mark::ManaSpent(k))) => *k = (*k as usize + n).min(255) as u8,
            _ => self.marks.push((r, Mark::ManaSpent(n.min(255) as u8))),
        }
    }

    /// The amount of mana spent to cast object `r` (a spell, or the permanent
    /// it became; 0 if it wasn't cast, CR 601.2h).
    pub(crate) fn mana_spent_of(&self, r: crate::ids::ObjRef) -> u8 {
        self.marks
            .iter()
            .find_map(|&(o, m)| match m {
                Mark::ManaSpent(n) if o == r => Some(n),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// Spell `idx` became permanent `r`, `n` mana spent to cast it: the
    /// permanent knows it, and enters with that many +1/+1 counters if it
    /// says so (CR 122.6, 614.1c).
    pub(crate) fn w3_entered_spent(&mut self, idx: CardIdx, r: crate::ids::ObjRef, n: u8) {
        self.marks.push((r, Mark::ManaSpent(n)));
        if self.def_of(idx).w3.counters_mana_spent && n > 0 {
            self.add_counters(idx, Counter::P1P1, n as crate::num::Count);
        }
    }

    /// Object `r` as a source of damage and its power: now if it is still
    /// that permanent, else as it last existed on the battlefield (CR
    /// 608.2h, 113.7a; its controller then: `default_controller` if none was
    /// recorded).
    fn lki_source(&self, r: crate::ids::ObjRef, default_controller: PlayerId) -> (crate::game::DamageSource, i32) {
        if self.live(r) && self.objs[r.idx.i()].zone == crate::ids::Zone::Battlefield {
            return (self.damage_source_of(r.idx), self.power(r.idx));
        }
        let src = match self.lki_damage.iter().rev().find(|x| x.0 == r) {
            Some(&(_, d)) => d,
            None => crate::game::DamageSource { controller: default_controller, ..self.damage_source_of(r.idx) },
        };
        let power = match self.objs[r.idx.i()].lki_power {
            Some((inc, p)) if inc == r.inc => p,
            _ => 0,
        };
        (src, power)
    }

    /// Permanent `idx` (object `old`) is leaving the battlefield: the last
    /// known information workstream W3's cards read (`W3State::seen`).
    pub(crate) fn w3_leaves(&mut self, idx: CardIdx, old: crate::ids::ObjRef) {
        if self.w3.seen & SEEN_W3_HOST != 0 {
            if let Some(h) = self.objs[idx.i()].attached {
                self.w3.lki_host.push((old, h));
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

    /// The reservation a "remove a <k> counter from a <permanent> you
    /// control" cost makes in its mana payment (`Keep::sac`): one such
    /// permanent stays (a mana ability that sacrifices it is activated before
    /// the cost is paid, CR 601.2g-h).
    pub(crate) fn remove_counter_reserve(cost: &Cost) -> Option<crate::db::SacCost> {
        let (f, k) = cost.remove_counter_from.as_ref()?;
        let filter = Filter::And(Box::new(f.clone()), Box::new(Filter::HasCounter(*k)));
        Some(crate::db::SacCost { filter, n: 1, another: false })
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
    /// "Tap X untapped <permanents> you control" (CR 601.2h: the taps and
    /// the mana in any order; mana abilities first, 601.2g, so a permanent
    /// tapped for the cost can't make mana for it, and one tapped for mana
    /// can't be tapped for the cost): can `need` more of `rest` (untapped
    /// candidates) be tapped, besides `tapped`, with `card`'s ability's
    /// mana still payable by the other sources? The taps are paid before the
    /// mana (`Game::begin_activate`), as teamwork's are (ENGINE.md 29.6.3).
    pub(crate) fn tap_x_ok(&self, p: PlayerId, card: CardIdx, ad: &crate::db::ActivatedDef, tapped: &[CardIdx], rest: &[CardIdx], need: usize) -> bool {
        if rest.len() < need {
            return false;
        }
        let mana = self.act_mana_x(p, card, ad, 0);
        if mana.is_zero() {
            return true;
        }
        let exclude = Self::mana_exclude(card, ad);
        let classes = self.mana_classes(p, exclude);
        let keep = Self::keep_for(card, &ad.cost, exclude);
        // (Permanents without a mana ability first: they pay nothing.)
        let (with_mana, plain): (Vec<CardIdx>, Vec<CardIdx>) = rest.iter().copied().partition(|&c| self.has_mana_ability(c));
        let m = need.saturating_sub(plain.len());
        let k = with_mana.len().min(12);
        for mask in 0u32..(1 << k) {
            if mask.count_ones() as usize != m {
                continue;
            }
            let out: Vec<CardIdx> = tapped.iter().copied().chain((0..k).filter(|i| mask >> i & 1 != 0).map(|i| with_mana[i])).collect();
            if self.can_pay_with(p, &mana, &self.classes_without(&classes, &out), &keep) {
                return true;
            }
        }
        false
    }

    /// The most X "tap X untapped <permanents> you control" allows for
    /// `card`'s ability `ad` (CR 107.3a, 601.2b).
    pub(crate) fn tap_x_most(&self, p: PlayerId, card: CardIdx, ad: &crate::db::ActivatedDef) -> u8 {
        let Some(f) = &ad.cost.tap_x else { return u8::MAX };
        let t = crate::db::SacCost { filter: f.clone(), n: 0, another: true };
        let cands: Vec<CardIdx> = self.tap_candidates(p, card, &t).collect();
        let mut x = cands.len();
        while x > 0 && !self.tap_x_ok(p, card, ad, &[], &cands, x) {
            x -= 1;
        }
        x.min(255) as u8
    }

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
    /// Its `SEEN_W3_*` bits ([`seen_of`]).
    pub seen: u8,
    /// It reads the amount of mana spent to cast it (CR 601.2h): the
    /// payment records it (`Mark::ManaSpent`).
    pub mana_spent: bool,
    /// "This enters with a number of +1/+1 counters on it equal to the
    /// amount of mana spent to cast it" (CR 122.6, 614.1c).
    pub counters_mana_spent: bool,
}

impl Game {
    /// Does a permanent say "combat damage can't be prevented" (CR 615.12)?
    pub(crate) fn all_combat_unpreventable(&self) -> bool {
        self.battlefield.iter().any(|&c| self.abilities(c).w3.combat_unpreventable_all)
    }
}
