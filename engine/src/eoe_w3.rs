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
    /// Each object of `on` phases out (CR 702.26b, k), and with it what is
    /// attached to it (indirectly, 702.26e).
    PhaseOut { on: On },
}

impl W3 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            W3::Regenerate { on } | W3::GrantTrigger { on, .. } => matches!(on, On::EventObj),
            W3::CantBeRegenerated(_) | W3::DamageEachSourceMv { .. } | W3::HostDamagesPower { .. } => false,
            W3::EventObjDamagesEachOpponent { .. } => true,
            W3::RemoveCounterFromEachMay { .. } => false,
            W3::PhaseOut { on } => matches!(on, On::EventObj),
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
            | W3::EventObjDamagesEachOpponent { .. }
            | W3::PhaseOut { .. } => None,
            W3::RemoveCounterFromEachMay { skip, .. } => Some(skip),
        }
    }
}

/// An activated ability another permanent grants a permanent
/// (`Game::granted_activated`): ability `slot` of definition `def`, with the
/// Equipment granting it if it is an Equipment's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Grant {
    pub def: crate::ids::DefId,
    pub slot: u8,
    pub equipment: Option<CardIdx>,
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
    /// Last known information: the latest "loses all abilities" timestamp of
    /// each permanent with a granted triggered ability that left the
    /// battlefield this turn (CR 603.10a).
    pub lki_loss: Vec<(crate::ids::ObjRef, u32)>,
    /// The phased-out permanents (zone `PhasedOut`, CR 702.26b), in the order
    /// they phased out.
    pub phased: Vec<CardIdx>,
    /// Those of them that phased out indirectly (CR 702.26e): they phase in
    /// with the permanent they are attached to (702.26h).
    pub indirect: Vec<CardIdx>,
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
/// `W3State::seen`: a card grants activated abilities to other permanents.
pub(crate) const SEEN_W3_GRANT_ACT: u8 = 2;
/// `W3State::seen`: a card gives other permanents a subtype.
pub(crate) const SEEN_W3_SUBTYPE: u8 = 4;

/// The `SEEN_W3_*` bits of a definition (`W3Def::seen`, set as it is
/// lowered).
pub(crate) fn seen_of(d: &crate::db::CardDef) -> u8 {
    let mut bits = if d.w3.grants_activated.is_empty() { 0 } else { SEEN_W3_GRANT_ACT };
    if !d.w3.grants_subtype.is_empty() {
        bits |= SEEN_W3_SUBTYPE;
    }
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
                // (One effect, one timestamp, CR 613.7a.)
                self.next_ts += 1;
                let ts = self.next_ts;
                for c in self.objs_on(f, on) {
                    let r = self.obj_ref(c);
                    self.marks.push((r, Mark::Granted { def: f.source_def, slot, this_turn: *this_turn, ts }));
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
            W3::PhaseOut { on } => {
                for c in self.objs_on(f, on) {
                    if self.objs[c.i()].zone == crate::ids::Zone::Battlefield {
                        self.phase_out(c);
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
        // (When its abilities were lost, for its granted leave triggers'
        // look-back, CR 603.10a.)
        if self.w3.granted && self.marks.iter().any(|&(o, m)| o == old && matches!(m, Mark::Granted { .. })) {
            if let Some(l) = self.loss_ts(idx) {
                self.w3.lki_loss.push((old, l));
            }
        }
        if self.w3.seen & SEEN_W3_HOST != 0 {
            if let Some(h) = self.objs[idx.i()].attached {
                self.w3.lki_host.push((old, h));
            }
        }
    }

    /// The objects with triggered abilities effects gave them that may see
    /// event `ev` (`Mark::Granted`): (object, definition, trigger slot,
    /// controller). A permanent on the battlefield that hasn't lost all its
    /// abilities after the grant (CR 613.1f, 613.7: a "loses all abilities"
    /// effect with a later timestamp removes it), or, for a
    /// leave-the-battlefield event, one that left earlier in the same
    /// simultaneous move, as it was then (603.10a; `W3State::lki_loss`).
    pub(crate) fn granted_observers(&self, ev: &crate::triggers::Event) -> Vec<(crate::ids::ObjRef, crate::ids::DefId, u8, PlayerId)> {
        use crate::db::EventKind as K;
        let leave = matches!(ev.kind, K::Dies | K::LeavesBattlefield | K::IsSacrificed);
        let mut out = Vec::new();
        for &(r, m) in &self.marks {
            let Mark::Granted { def, slot, ts, .. } = m else { continue };
            if self.live(r) && self.objs[r.idx.i()].zone == crate::ids::Zone::Battlefield {
                if self.loss_ts(r.idx).is_none_or(|l| ts > l) {
                    out.push((r, def, slot, self.objs[r.idx.i()].controller));
                }
            } else if leave {
                let lost = self.w3.lki_loss.iter().rev().find(|x| x.0 == r).is_some_and(|&(_, l)| ts < l);
                if let Some(l) = self.leaving.iter().find(|l| l.old == r) {
                    if !lost {
                        out.push((r, def, slot, l.controller));
                    }
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
    /// 514.2): regeneration shields, granted triggered abilities until end
    /// of turn; damage is removed from phased-out permanents too.
    pub(crate) fn end_turn_w3(&mut self) {
        if !self.marks.is_empty() {
            self.marks.retain(|&(_, m)| !matches!(m, Mark::RegenShield | Mark::Granted { this_turn: true, .. }));
            self.w3.granted = self.marks.iter().any(|&(_, m)| matches!(m, Mark::Granted { .. }));
        }
        for i in 0..self.w3.phased.len() {
            let c = self.w3.phased[i];
            let o = &mut self.objs[c.i()];
            o.damage = 0;
            o.deathtouched = false;
        }
    }

    /// Permanent `c` phases out (CR 702.26b, k): it and what is attached to
    /// it (indirectly, 702.26e; recursively) are treated as though they
    /// don't exist, without changing zones (702.26d: no leave or enter
    /// triggers, the same objects). A creature leaves combat (506.4).
    pub(crate) fn phase_out(&mut self, c: CardIdx) {
        let mut all = vec![c];
        let mut k = 0;
        while k < all.len() {
            let host = self.obj_ref(all[k]);
            for &x in &self.battlefield {
                if self.objs[x.i()].attached == Some(host) && !all.contains(&x) {
                    all.push(x);
                }
            }
            k += 1;
        }
        for (i, &x) in all.iter().enumerate() {
            self.battlefield.retain(|&y| y != x);
            let o = &mut self.objs[x.i()];
            o.zone = crate::ids::Zone::PhasedOut;
            o.attacking = false;
            o.blocked = false;
            o.blocking = None;
            o.attacks = None;
            self.static_count(x, -1);
            self.w3.phased.push(x);
            if i > 0 {
                self.w3.indirect.push(x);
            }
            self.note(|g| format!("{} phases out", g.name(x)));
        }
        self.effects_changed();
    }

    /// The static-ability fast paths count permanent `c` (`d` = 1) or not
    /// (-1) as it phases in or out (`Game::n_static`, `n_self_types`).
    fn static_count(&mut self, c: CardIdx, d: i32) {
        let def = self.def_of(c);
        if def.statics.is_empty() {
            return;
        }
        let typed = crate::layers::self_typed(def);
        self.n_static = (self.n_static as i32 + d) as _;
        if typed {
            self.n_self_types = (self.n_self_types as i32 + d) as _;
        }
    }

    /// The untap step of `p` (CR 502.1): the permanents that phased out under
    /// `p`'s control directly phase in, each with what phased out with it
    /// (702.26h), attached again; the same objects (702.26d), in their
    /// timestamp order among the battlefield's.
    pub(crate) fn phase_in_untap(&mut self, p: PlayerId) {
        let mut back: Vec<CardIdx> =
            self.w3.phased.iter().copied().filter(|&c| self.objs[c.i()].controller == p && !self.w3.indirect.contains(&c)).collect();
        let mut k = 0;
        while k < back.len() {
            let host = self.obj_ref(back[k]);
            for &x in &self.w3.phased {
                if self.w3.indirect.contains(&x) && self.objs[x.i()].attached == Some(host) && !back.contains(&x) {
                    back.push(x);
                }
            }
            k += 1;
        }
        if back.is_empty() {
            return;
        }
        for &x in &back {
            self.w3.phased.retain(|&y| y != x);
            self.w3.indirect.retain(|&y| y != x);
            self.objs[x.i()].zone = crate::ids::Zone::Battlefield;
            self.static_count(x, 1);
            let ts = self.objs[x.i()].timestamp;
            let at = self.battlefield.iter().position(|&y| self.objs[y.i()].timestamp > ts).unwrap_or(self.battlefield.len());
            self.battlefield.insert(at, x);
            self.note(|g| format!("{} phases in", g.name(x)));
        }
        self.effects_changed();
        // A daybound permanent that phased out by day exists again at night:
        // it transforms at once (CR 702.145c; nightbound by day, 702.145f).
        self.day_night_any_time();
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

    /// The activated abilities other permanents grant permanent `c` (CR
    /// 613.1f), the granters in battlefield (timestamp) order, each one's
    /// abilities in its order; they follow the places of `c`'s card's own
    /// abilities. Two kinds:
    ///
    /// * An attached Equipment's "equipped creature has '<ability>'" (CR
    ///   301.5; `CardDef::grants_equipped`, a face holding the abilities,
    ///   ENGINE.md 99.14): none if the Equipment lost its abilities (the
    ///   grant exists only while its static ability does, 613.8a); its
    ///   timestamp is the Equipment's, renewed as it became attached
    ///   (613.7e).
    /// * "Artifacts you control have '<ability>'" (`W3Def::grants_activated`,
    ///   Ragost, ENGINE.md 99.13): the ability applied in layer 4 already (the
    ///   Food type), so it applies in layer 6 even if the granter loses its
    ///   abilities (613.6), read from its definition; its timestamp is the
    ///   granter's (613.7a).
    ///
    /// A "loses all abilities" effect on `c` with a later timestamp than a
    /// grant removes it, one with an earlier timestamp doesn't (613.7).
    pub(crate) fn granted_activated(&self, c: CardIdx) -> Vec<Grant> {
        let mut out = Vec::new();
        if self.objs[c.i()].zone != crate::ids::Zone::Battlefield {
            return out;
        }
        let r = self.obj_ref(c);
        let loss = self.loss_ts(c);
        let statics = self.w3.seen & SEEN_W3_GRANT_ACT != 0;
        for &g in &self.battlefield {
            let ts = self.objs[g.i()].timestamp;
            if loss.is_some_and(|l| ts < l) {
                continue;
            }
            if self.objs[g.i()].attached == Some(r) {
                if let Some(face) = self.abilities(g).grants_equipped {
                    for k in 0..self.db.def(face).activated.len() {
                        out.push(Grant { def: face, slot: k as u8, equipment: Some(g) });
                    }
                }
            }
            if statics {
                let d = self.db.def(self.objs[g.i()].def);
                if d.w3.grants_activated.is_empty() {
                    continue;
                }
                let ctx = crate::cast::Ctx { controller: self.objs[g.i()].controller, source: Some(self.obj_ref(g)) };
                for (f, k) in &d.w3.grants_activated {
                    if self.card_matches(f, c, &ctx) {
                        out.push(Grant { def: self.objs[g.i()].def, slot: *k, equipment: None });
                    }
                }
            }
        }
        out
    }

    /// Where activated ability `slot` of permanent `c` is defined: its own
    /// definition, or (a place past its own abilities) a granted ability's
    /// (`granted_activated`), with the Equipment that grants it if one does
    /// (its "exile <the Equipment>" cost, ENGINE.md 99.14).
    pub(crate) fn activated_home(&self, c: CardIdx, slot: crate::db::AbilitySlot) -> (crate::ids::DefId, crate::db::AbilitySlot, Option<CardIdx>) {
        let def = self.objs[c.i()].def;
        if let crate::db::AbilitySlot::Activated(i) = slot {
            let own = self.db.def(def).activated.len();
            if i as usize >= own {
                if let Some(g) = self.granted_activated(c).get(i as usize - own) {
                    return (g.def, crate::db::AbilitySlot::Activated(g.slot), g.equipment);
                }
            }
        }
        (def, slot, None)
    }

    /// The definition that has activated ability `slot` of permanent `c`
    /// (`activated_home`'s first part; tests, the list block's token).
    pub fn activated_home_pub(&self, c: CardIdx, slot: crate::db::AbilitySlot) -> crate::ids::DefId {
        self.activated_home(c, slot).0
    }

    /// The timestamps (CR 613.7a: its source's) of the static abilities on
    /// the battlefield that give permanent `c` subtype `s` ("artifacts you
    /// control are Foods in addition to their other types", 613.1d), sorted.
    /// Whether one applies to `c` reads `c`'s card types after layer 4: the
    /// grant depends on any effect that changes whether `c` is an artifact
    /// (613.8a), which applies first whatever the timestamps; it changes
    /// only subtypes, so nothing recurses. (A layer 4 effect of a static
    /// ability applies even if its source loses its abilities in layer 6,
    /// 613.6: the card's own definition.)
    pub(crate) fn granted_subtype_ts(&self, c: CardIdx, s: u16) -> Vec<u32> {
        let mut out = Vec::new();
        for &g in &self.battlefield {
            let d = self.db.def(self.objs[g.i()].def);
            for (f, k) in &d.w3.grants_subtype {
                if *k != s {
                    continue;
                }
                let ctx = crate::cast::Ctx { controller: self.objs[g.i()].controller, source: Some(self.obj_ref(g)) };
                if self.card_matches(f, c, &ctx) {
                    out.push(self.objs[g.i()].timestamp);
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Does permanent `c` have subtype `s` after layer 4 (CR 613.1d), with
    /// `has` its printed answer and `grants` the timestamps of the static
    /// abilities that add `s` to it: the effects in timestamp order (613.7),
    /// its own type-changing statics at its own timestamp (as
    /// `Game::has_subtype` applies them, before the effects), each grant
    /// adding `s` at its place; an effect that sets the subtypes later
    /// removes it, an earlier one doesn't.
    pub(crate) fn subtype_with_grants(&self, c: CardIdx, s: u16, mut has: bool, grants: &[u32]) -> bool {
        let mut k = 0;
        let own_ts = self.objs[c.i()].timestamp;
        while k < grants.len() && grants[k] < own_ts {
            has = true;
            k += 1;
        }
        if self.n_self_types > 0 {
            for m in self.own_type_mods(c) {
                if let crate::db::Mod::Types { subtypes, replace, .. } = m {
                    has = if *replace { subtypes.contains(&s) } else { has || subtypes.contains(&s) };
                }
            }
        }
        let r = self.obj_ref(c);
        for e in self.effects.iter().filter(|e| e.on == crate::layers::Affected::Obj(r)) {
            match &e.m {
                crate::db::Mod::Types { .. } | crate::db::Mod::BecomeTypes { .. } => {}
                _ => continue,
            }
            while k < grants.len() && grants[k] < e.ts {
                has = true;
                k += 1;
            }
            match &e.m {
                crate::db::Mod::Types { subtypes, replace, .. } => {
                    if *replace {
                        has = subtypes.contains(&s);
                    } else if subtypes.contains(&s) {
                        has = true;
                    }
                }
                // CR 205.1a: its other card types and their subtypes gone.
                crate::db::Mod::BecomeTypes { subtypes, .. } => has = subtypes.contains(&s),
                _ => {}
            }
        }
        has || k < grants.len()
    }

    /// The places of permanent `c`'s activated abilities: its card's own,
    /// then those other permanents grant it.
    pub(crate) fn ability_slots_total(&self, c: CardIdx) -> usize {
        self.db.def(self.objs[c.i()].def).activated.len() + self.granted_activated(c).len()
    }

    /// Is `slot` of permanent `c` the page of its abilities past its slots
    /// (the fourth, when it has more abilities than `ACTIVATE` slots)?
    pub(crate) fn paged_slot(&self, c: CardIdx, slot: crate::db::AbilitySlot) -> bool {
        let last = crate::activate::ACTIVATE_SLOTS - 1;
        slot == crate::db::AbilitySlot::Activated(last as u8)
            && self.objs[c.i()].zone == crate::ids::Zone::Battlefield
            && self.ability_slots_total(c) > crate::activate::ACTIVATE_SLOTS
    }

    /// `p` chooses which activated ability of permanent `card` to activate,
    /// among those it may activate now from its fourth on (CR 602.2, 117.1b;
    /// `Task::ChooseAbility`): `LIBRARY` + place - 3, `Game::ask_paged` past
    /// the 64 list ids (`page`; ENGINE.md 100.2). The list block holds each
    /// option's ability: the definition that has it (the card's own, or the
    /// face granting it, `Game::activated_home`) and its index there.
    /// Re-entrant: the options come from the game as it is.
    pub(crate) fn choose_ability(&mut self, p: PlayerId, card: CardIdx, page: crate::decision::Page) {
        use crate::decision::{act, Choice, DecisionKind};
        match self.answer.take() {
            Some(Choice::Page(next)) => {
                self.agenda.push(crate::flow::Task::ChooseAbility { p, card, page: next });
                return;
            }
            Some(Choice::Ability(c, slot)) => {
                self.begin_activate(p, c, slot);
                return;
            }
            Some(other) => unreachable!("ability answer {other:?}"),
            None => {}
        }
        let classes = self.mana_classes(p, None);
        let sorcery_ok = self.turn.active == p && self.turn.step.is_main() && self.stack.is_empty();
        let locks = self.activation_locks();
        let live = self.permanent_activations(p, card, &classes, sorcery_ok, locks);
        let from = (crate::activate::ACTIVATE_SLOTS - 1) as u8;
        let items: Vec<(u32, Choice)> = live
            .iter()
            .filter(|&&i| i >= from)
            .map(|&i| ((i - from) as u32, Choice::Ability(card, crate::db::AbilitySlot::Activated(i))))
            .collect();
        self.ask_paged(crate::flow::Task::ChooseAbility { p, card, page }, page, p, DecisionKind::ChooseMode, act::LIBRARY, act::LIBRARY_N, &items, Vec::new());
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
    /// The reservation "tap X untapped <permanents> you control" makes in
    /// its ability's mana payment (`Keep::sac` with `Keep::sac_taps`): the
    /// mana abilities are activated first (CR 601.2g) and the cost paid after
    /// (601.2h), so a source whose activation taps or sacrifices one of those
    /// permanents (its own {T} or sacrifice, another's "tap an untapped
    /// artifact you control" or "sacrifice an artifact") uses it up; X of
    /// them must stay untapped. Any mana ability that doesn't (Pentad
    /// Prism's counters) is free. None for X = 0.
    pub(crate) fn tap_x_reserve(cost: &Cost, x: u8) -> Option<crate::db::SacCost> {
        let f = cost.tap_x.as_ref()?;
        if x == 0 {
            return None;
        }
        let filter = Filter::And(Box::new(f.clone()), Box::new(Filter::Not(Box::new(Filter::Tapped))));
        Some(crate::db::SacCost { filter, n: x, another: true })
    }

    /// The most X "tap X untapped <permanents> you control" allows for
    /// `card`'s ability `ad` (CR 107.3a, 601.2b): as many of them as stay
    /// untapped while the mana is paid (`tap_x_reserve`).
    pub(crate) fn tap_x_most(&self, p: PlayerId, card: CardIdx, ad: &crate::db::ActivatedDef) -> u8 {
        if ad.cost.tap_x.is_none() {
            return u8::MAX;
        }
        let Some(all) = Self::tap_x_reserve(&ad.cost, 1) else { return 0 };
        let cands = self.sacrifice_candidates(p, card, &all).len();
        let mana = self.act_mana_x(p, card, ad, 0);
        let exclude = Self::mana_exclude(card, ad);
        let classes = self.mana_classes(p, exclude);
        let mut x = 0usize;
        while x < cands.min(255) {
            if !mana.is_zero() {
                let r = Self::tap_x_reserve(&ad.cost, (x + 1) as u8).unwrap();
                let keep = crate::pay::Keep { sac: Some((r, card)), sac_taps: true, ..Self::keep_for(card, &ad.cost, exclude) };
                if !self.can_pay_without(p, &mana, &classes, &keep, exclude) {
                    break;
                }
            }
            x += 1;
        }
        x as u8
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
    /// "<Permanents matching the filter> have '<activated ability k of this
    /// card>'" (CR 613.1f, layer 6; the filter's "you" is this permanent's
    /// controller): latent abilities of the card, which it doesn't have
    /// itself (`CardDef::latent`).
    pub grants_activated: Vec<(Filter, u8)>,
    /// "<Permanents matching the filter> are <subtype>s in addition to their
    /// other types" (CR 205.1b, 613.1d: layer 4; the filter reads card types
    /// and control only, so nothing recurses).
    pub grants_subtype: Vec<(Filter, u16)>,
}

impl Game {
    /// Does a permanent say "combat damage can't be prevented" (CR 615.12)?
    pub(crate) fn all_combat_unpreventable(&self) -> bool {
        self.battlefield.iter().any(|&c| self.abilities(c).w3.combat_unpreventable_all)
    }
}
