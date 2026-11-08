//! Edge of Eternities, W6 (ENGINE.md 99.16): the rules the set's remaining
//! cards need. Named after the rules actions, each citing its rule; never
//! after a card.

use crate::game::Game;
use crate::ids::{Target, Zone};
use crate::resolve::{Flow, Frame};

/// The printing facts W6 adds (`CardDef::w6`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W6Def {
    /// Its `SEEN_W6_*` bits (`seen_of`).
    pub seen: u16,
    /// One of its abilities reads the amount of mana spent to cast another
    /// spell (CR 601.2h; "if the amount of mana spent to cast that spell was
    /// less than its mana value"): every spell's payment is recorded
    /// (`Mark::ManaSpent`) in a table with such a card.
    pub reads_spells_mana_spent: bool,
    /// "If one or more counters would be put on a <filter> you control,
    /// twice that many of each of those kinds of counters are put on it
    /// instead" (CR 614.1a, 122.6): the recipients (the static ability's
    /// controller controls them).
    pub counter_doubler: Option<crate::db::Filter>,
    /// An Aura's "You control enchanted permanent" (CR 303.4e, 613.1b: a
    /// control-changing effect of a static ability, layer 2).
    pub aura_control: bool,
    /// An Aura's "Enchanted permanent doesn't untap during its controller's
    /// untap step" (CR 502.3).
    pub aura_no_untap: bool,
    /// "If a permanent entering the battlefield causes a triggered ability
    /// of a permanent you control to trigger, that ability triggers an
    /// additional time" (CR 603.2d, 603.6a).
    pub trigger_doubler: bool,
}

/// The table facts W6 adds (`CardDb::w6`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W6Db {
    /// A card of the table reads the mana spent to cast any spell: every
    /// payment for a spell is recorded (CR 601.2h).
    pub mana_spent_all: bool,
}

impl W6Db {
    pub(crate) fn of(defs: &[crate::db::CardDef]) -> W6Db {
        W6Db { mana_spent_all: defs.iter().any(|d| d.w6.reads_spells_mana_spent) }
    }
}

/// W6's game state (`Game::w6`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W6State {
    /// `SEEN_W6_*` bits: a card of this game needs the rule (as
    /// `Game::seen`).
    pub seen: u16,
    /// The permanent being put onto the battlefield whose "enters with"
    /// counters are being put on it (CR 122.6, 614.1c), with the first
    /// timestamp of the entries of its event (what entered at the same time:
    /// `Game::group_ts`): set and cleared around its entry.
    pub entering: Option<(crate::ids::CardIdx, u32)>,
    /// The controller of the spell or ability whose instruction is being
    /// carried out (`Game::run_frame`): the player who puts counters its
    /// effect puts (CR 122.6, 608.2): set and restored around each op.
    pub actor: Option<crate::ids::PlayerId>,
    /// The control-changing effects of resolved spells and abilities and of
    /// entering under another player's control, in games with an Aura's
    /// control effect (`SEEN_W6_CONTROL`): the permanent's controller is
    /// worked out from them and the Auras' effects in timestamp order (CR
    /// 613.1b, 613.7; `Game::w6_sync_control`).
    pub control: Vec<ControlFx>,
    /// Set while `w6_sync_control` writes controllers (no recursion).
    pub syncing: bool,
    /// The triggered abilities of permanents that permanents entering in
    /// the current simultaneous entry (`Game::group_ts`, the key) caused to
    /// trigger, with their controllers: a permanent entering later in that
    /// entry with "... triggers an additional time" adds an instance of each
    /// of its controller's (CR 603.2d, its ruling: "a permanent entering
    /// the battlefield at the same time"). Games with such a card only.
    pub entry_triggers: Vec<((u32, u32), crate::triggers::PendingTrigger)>,
}

/// A control-changing effect (CR 613.1b) other than an Aura's: `obj` is
/// controlled by `to` from timestamp `ts` (0: it entered the battlefield
/// under `to`'s control, CR 110.2), until the end of the turn (`eot`, CR
/// 514.2) or for good.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ControlFx {
    pub obj: crate::ids::ObjRef,
    pub to: crate::ids::PlayerId,
    pub ts: u32,
    pub eot: bool,
}

/// `W6State::seen`: a replacement effect multiplies counters put on
/// permanents (`W6Def::counter_doubler`).
pub(crate) const SEEN_W6_COUNTERS: u16 = 1;
/// `W6State::seen`: a triggered ability reads who put +1/+1 counters and
/// how many (`TrigPred::CountersPutBy`): the `CountersAdded` event carries
/// them.
pub(crate) const SEEN_W6_PUTTER: u16 = 2;
/// `W6State::seen`: an Aura's static ability changes control of the
/// permanent it enchants (`W6Def::aura_control`): every control change of
/// the game is recorded (`W6State::control`).
pub(crate) const SEEN_W6_CONTROL: u16 = 4;
/// `W6State::seen`: an Aura says the permanent it enchants doesn't untap
/// (`W6Def::aura_no_untap`).
pub(crate) const SEEN_W6_NO_UNTAP: u16 = 8;
/// `W6State::seen`: a static ability makes abilities that a permanent
/// entering causes to trigger trigger an additional time
/// (`W6Def::trigger_doubler`).
pub(crate) const SEEN_W6_TRIGGERS: u16 = 16;

/// The abilities of a definition that go on the stack (its spell, triggered
/// and activated abilities).
fn abilities_of(d: &crate::db::CardDef) -> impl Iterator<Item = &crate::db::AbilityDef> {
    d.triggers.iter().map(|t| &t.ability).chain(d.spell.iter()).chain(d.activated.iter().filter_map(|a| match &a.effect {
        crate::db::ActEffect::Stack(ab) => Some(ab),
        crate::db::ActEffect::Mana(_) => None,
    }))
}

/// The facts W6 reads off a lowered definition's programs (`W6Def`), after
/// its printing is lowered.
pub(crate) fn finish_def(d: &mut crate::db::CardDef) {
    let mut reads = false;
    for ab in abilities_of(d) {
        for op in &ab.program {
            if let crate::program::Op::Ext(crate::program::Ext::EoeW6(W6::JumpUnlessSpentBelowMv { .. })) = op {
                reads = true;
            }
        }
    }
    d.w6.reads_spells_mana_spent = reads;
    d.w6.seen = seen_of(d);
}

/// The `SEEN_W6_*` bits of a definition.
pub(crate) fn seen_of(d: &crate::db::CardDef) -> u16 {
    let mut bits = 0;
    if d.w6.counter_doubler.is_some() {
        bits |= SEEN_W6_COUNTERS;
    }
    if d.w6.aura_control {
        bits |= SEEN_W6_CONTROL;
    }
    if d.w6.aura_no_untap {
        bits |= SEEN_W6_NO_UNTAP;
    }
    fn puts(p: &crate::db::TrigPred) -> bool {
        use crate::db::TrigPred as T;
        match p {
            T::CountersPutBy(_) => true,
            T::And(a, b) | T::Or(a, b) => puts(a) || puts(b),
            T::Not(a) => puts(a),
            _ => false,
        }
    }
    if d.triggers.iter().any(|t| puts(&t.pred)) {
        bits |= SEEN_W6_PUTTER;
    }
    if d.w6.trigger_doubler {
        bits |= SEEN_W6_TRIGGERS;
    }
    bits
}

/// W6's instructions (`Ext::EoeW6`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum W6 {
    /// Jump unless the amount of mana spent to cast target spell `tgt` was
    /// less than its mana value (CR 601.2h, 202.3; an {X} spell's mana value
    /// counts its X, 202.3e). Read before the spell is countered: the values
    /// are those of the spell as it last existed on the stack (608.2h), the
    /// same before and after it leaves. A copy of a spell wasn't cast: no
    /// mana was spent (707.10).
    JumpUnlessSpentBelowMv { tgt: u8, to: u16 },
    /// "Tap enchanted permanent": the permanent the source (an Aura) is
    /// attached to, or, if the Aura has left, was attached to as it last
    /// existed (CR 608.2h); nothing if that permanent has left.
    TapHost,
    /// "Move a counter from target `from` onto target `to`" (CR 122.5):
    /// remove one counter from the first and put it on the second (which
    /// replacement effects that modify putting counters see, and "whenever
    /// ... counters are put" triggers, its ruling). Nothing happens if
    /// either target is illegal (608.2b; the ruling) or the first has no
    /// counter. Choice: which kind, as it resolves (the ruling of
    /// 2020-04-17), when it has several: `NUMBER` + its place among them in
    /// kind order, paged past the range (`ask_paged`, the window in
    /// `Frame::i`, so every kind is reachable).
    MoveCounter { from: u8, to: u8 },
}

impl W6 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            W6::JumpUnlessSpentBelowMv { .. } | W6::TapHost | W6::MoveCounter { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            W6::JumpUnlessSpentBelowMv { to, .. } => Some(to),
            W6::TapHost | W6::MoveCounter { .. } => None,
        }
    }
}

impl Game {
    /// `n` counters of a kind are about to be put on permanent `c` (CR
    /// 122.6): how many are put once the replacement effects that multiply
    /// them apply (CR 614.1a; Loading Zone). Each such effect of a
    /// permanent whose controller controls `c` and whose recipients match
    /// `c` doubles them: two give four times as many, and so on (its
    /// ruling of 2025-07-25). The affected object's controller would order
    /// them (616.1), but doublings commute, and the lowering refuses any
    /// other counter-modifying replacement: no order to choose. Counters a
    /// permanent enters with (122.6) get only the effects that already
    /// exist (614.12): not those of permanents entering at the same time.
    pub(crate) fn w6_counters_put(&self, c: crate::ids::CardIdx, n: crate::num::Count) -> crate::num::Count {
        if self.objs[c.i()].zone != Zone::Battlefield {
            return n;
        }
        let entering = self.w6.entering.filter(|e| e.0 == c).map(|e| e.1);
        let ctl = self.objs[c.i()].controller;
        let mut n = n;
        for &d in &self.battlefield {
            let Some(f) = &self.abilities(d).w6.counter_doubler else { continue };
            if self.objs[d.i()].controller != ctl || entering.is_some_and(|since| self.objs[d.i()].timestamp >= since) {
                continue;
            }
            let ctx = crate::cast::Ctx { controller: ctl, source: Some(self.obj_ref(d)) };
            if self.card_matches(f, c, &ctx) {
                n = n.saturating_mul(2);
            }
        }
        n
    }

    /// The player who puts counters on permanent `c` now (CR 122.6): for
    /// counters it enters with, its controller (122.6a: no effect of the
    /// table names another player); else the controller of the spell or
    /// ability whose instruction puts them (608.2).
    pub(crate) fn w6_putter(&self, c: crate::ids::CardIdx) -> crate::ids::PlayerId {
        if self.w6.entering.is_some_and(|e| e.0 == c) {
            return self.objs[c.i()].controller;
        }
        self.w6.actor.unwrap_or_else(|| panic!("counters put on {} outside a spell or ability (EoE W6: who puts them?)", self.name(c)))
    }

    /// Does an Aura say "enchanted permanent doesn't untap during its
    /// controller's untap step" of permanent `c` (CR 502.3)?
    pub(crate) fn w6_no_untap(&self, c: crate::ids::CardIdx) -> bool {
        let r = self.obj_ref(c);
        self.battlefield.iter().any(|&a| self.objs[a.i()].attached == Some(r) && self.abilities(a).w6.aura_no_untap)
    }

    /// A control-changing effect with no Aura (CR 613.1b): `p` controls
    /// permanent `c` from now (a new timestamp, 613.7a), until the end of the
    /// turn or for good; then the controllers are worked out again. Not a
    /// permanent `p` doesn't own while `p` has no room for another permanent
    /// (the engine's 96, `Game::change_control`).
    pub(crate) fn w6_control_effect(&mut self, c: crate::ids::CardIdx, p: crate::ids::PlayerId, eot: bool) {
        if self.objs[c.i()].zone != Zone::Battlefield {
            return;
        }
        self.next_ts += 1;
        let fx = ControlFx { obj: self.obj_ref(c), to: p, ts: self.next_ts, eot };
        self.w6.control.push(fx);
        self.w6_sync_control();
    }

    /// The "until end of turn" control-changing effects end (CR 514.2).
    pub(crate) fn w6_end_eot_control(&mut self) {
        self.w6.control.retain(|x| !x.eot);
        self.w6_sync_control();
    }

    /// A permanent entered under `p`'s control (CR 110.2): if `p` isn't its
    /// owner, that is where its control starts (timestamp 0: before every
    /// control-changing effect on it).
    pub(crate) fn w6_entered_under(&mut self, r: crate::ids::ObjRef, p: crate::ids::PlayerId) {
        if self.objs[r.idx.i()].owner != p {
            self.w6.control.push(ControlFx { obj: r, to: p, ts: 0, eot: false });
        }
    }

    /// Work out every permanent's controller (CR 613.1b, 613.7: the control
    /// effects that apply to it, in timestamp order; the latest decides):
    /// its owner, or whoever it entered the battlefield under (110.2), then
    /// the recorded effects (`W6State::control`) and the effects of the Auras
    /// attached to it that say "you control enchanted permanent" (their
    /// timestamp is the Aura's: the later of when it entered and when it
    /// became attached, 613.7e; "you" is the Aura's controller now). An
    /// Aura that lost its abilities (613.1f) or left gives nothing. A
    /// permanent whose controller changes is removed from combat (506.4).
    /// Only in games with such an Aura (`SEEN_W6_CONTROL`).
    pub(crate) fn w6_sync_control(&mut self) {
        if self.w6.seen & SEEN_W6_CONTROL == 0 || self.w6.syncing {
            return;
        }
        self.w6.syncing = true;
        // (Effects on objects that left the battlefield end, CR 400.7.)
        if !self.w6.control.is_empty() {
            let gone: Vec<bool> = self.w6.control.iter().map(|x| !(self.live(x.obj) && matches!(self.objs[x.obj.idx.i()].zone, Zone::Battlefield | Zone::PhasedOut))).collect();
            let mut k = 0;
            self.w6.control.retain(|_| {
                k += 1;
                !gone[k - 1]
            });
        }
        let perms: Vec<crate::ids::CardIdx> = self.battlefield.clone();
        for c in perms {
            if self.objs[c.i()].zone != Zone::Battlefield {
                continue;
            }
            let r = self.obj_ref(c);
            let mut best = (0u32, self.objs[c.i()].owner);
            for x in &self.w6.control {
                if x.obj == r && (x.ts > best.0 || (x.ts == 0 && best.0 == 0)) {
                    best = (x.ts, x.to);
                }
            }
            for &a in &self.battlefield {
                if self.objs[a.i()].attached == Some(r) && self.abilities(a).w6.aura_control {
                    let ts = self.objs[a.i()].timestamp;
                    if ts > best.0 {
                        best = (ts, self.objs[a.i()].controller);
                    }
                }
            }
            let now = self.objs[c.i()].controller;
            let p = best.1;
            if p == now {
                continue;
            }
            if p != self.objs[c.i()].owner && self.perm_room(p) == 0 {
                self.note(|g| format!("P{p} controls the engine's most permanents: no control of {}", g.name(c)));
                continue;
            }
            self.apply_control(c, p);
        }
        self.w6.syncing = false;
    }

    /// The number of permanents `p` controls with "if a permanent entering
    /// the battlefield causes a triggered ability of a permanent you control
    /// to trigger, that ability triggers an additional time" (CR 603.2d):
    /// each adds one instance (two give three in all: its ruling).
    fn w6_trigger_doublers(&self, p: crate::ids::PlayerId) -> usize {
        self.battlefield.iter().filter(|&&c| self.objs[c.i()].controller == p && self.abilities(c).w6.trigger_doubler).count()
    }

    /// A permanent's `Enters` event `ev` is about to be matched (CR 603.6a):
    /// if the permanent says "... triggers an additional time", the
    /// abilities of its controller's permanents that permanents entering
    /// earlier in the same simultaneous entry caused to trigger trigger an
    /// additional time too (the ruling: "a permanent entering at the same
    /// time", which the engine puts onto the battlefield one by one).
    pub(crate) fn w6_entering_doubler(&mut self, ev: &crate::triggers::Event) {
        let key = self.group_ts;
        if self.w6.entry_triggers.first().is_some_and(|x| x.0 != key) {
            self.w6.entry_triggers.clear();
        }
        let crate::triggers::Subj::Obj(r) = ev.a else { return };
        if !(self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield && self.abilities(r.idx).w6.trigger_doubler) {
            return;
        }
        let p = self.objs[r.idx.i()].controller;
        let more: Vec<crate::triggers::PendingTrigger> = self.w6.entry_triggers.iter().filter(|x| x.1.controller == p).map(|x| x.1).collect();
        self.waiting.extend(more);
    }

    /// The triggered ability just added to `Game::waiting` was caused by a
    /// permanent entering the battlefield (an `Enters` event, CR 603.6a: the
    /// abilities starting "when" or "whenever ... enters", the permanent's
    /// own and others', its rulings): if it is an ability of a permanent
    /// (not an emblem's, not a card's in a graveyard), it triggers an
    /// additional time for each such static ability its controller's
    /// permanents have (CR 603.2d: the number of times is worked out, then it
    /// triggers that many times; the instances are separate, their targets
    /// and choices made for each).
    pub(crate) fn w6_entry_triggered(&mut self) {
        let Some(&pt) = self.waiting.last() else { return };
        if !(self.live(pt.source) && self.objs[pt.source.idx.i()].zone == Zone::Battlefield) {
            return;
        }
        let n = self.w6_trigger_doublers(pt.controller);
        for _ in 0..n {
            self.waiting.push(pt);
        }
        let key = self.group_ts;
        if self.w6.entry_triggers.first().is_some_and(|x| x.0 != key) {
            self.w6.entry_triggers.clear();
        }
        self.w6.entry_triggers.push((key, pt));
    }

    pub(crate) fn exec_eoe_w6(&mut self, f: &mut Frame, op: &W6) -> Flow {
        match op {
            W6::JumpUnlessSpentBelowMv { tgt, to } => {
                let below = self.targets_of(f, *tgt).into_iter().any(|t| match t {
                    Target::Obj(o) if self.live(o) && self.objs[o.idx.i()].zone == Zone::Stack => {
                        (self.mana_spent_of(o) as u32) < self.mana_value_of(o.idx) as u32
                    }
                    _ => false,
                });
                if below {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            W6::TapHost => {
                let host = match self.objs_on(f, &crate::program::On::Source).first() {
                    Some(&a) => self.objs[a.i()].attached,
                    None => self.lki_attached.iter().find(|(o, _)| *o == f.source).map(|&(_, h)| h),
                };
                if let Some(h) = host {
                    if self.live(h) && self.objs[h.idx.i()].zone == Zone::Battlefield {
                        self.tap(h.idx);
                    }
                }
                Flow::Next
            }
            W6::MoveCounter { from, to } => {
                use crate::decision::{act, Choice, DecisionKind};
                let (Some(&Target::Obj(a)), Some(&Target::Obj(b))) = (self.targets_of(f, *from).first(), self.targets_of(f, *to).first()) else {
                    return Flow::Next;
                };
                let kinds = self.counters_of(a.idx);
                let k = match self.answer.take() {
                    Some(Choice::Page(p)) => {
                        f.i = p.window as u8;
                        self.agenda.push(crate::flow::Task::Run(f.clone()));
                        return Flow::Suspend;
                    }
                    // (The choice's number is its place in the page shown.)
                    Some(Choice::Number(j)) => f.i as usize * (act::NUMBER_N as usize - 1) + j as usize,
                    Some(other) => unreachable!("move-a-counter answer {other:?}"),
                    None => match kinds.len() {
                        0 => return Flow::Next,
                        1 => 0,
                        _ => {
                            let w = f.i as u32;
                            let per = act::NUMBER_N as u32 - 1;
                            let items: Vec<(u32, Choice)> =
                                (0..kinds.len() as u32).map(|k| (k, Choice::Number(if w == 0 && kinds.len() <= act::NUMBER_N as usize { k } else { k - (k / per) * per } as u8))).collect();
                            let page = crate::decision::Page { range: 0, window: w };
                            self.ask_paged(crate::flow::Task::Run(f.clone()), page, f.controller, DecisionKind::ChooseMode, act::NUMBER, act::NUMBER_N, &items, Vec::new());
                            return Flow::Suspend;
                        }
                    },
                };
                let Some(&(kind, _)) = kinds.get(k) else { return Flow::Next };
                if self.remove_counters(a.idx, kind, 1) == 1 {
                    self.note(|g| format!("a {kind:?} counter moves from {} to {}", g.name(a.idx), g.name(b.idx)));
                    self.add_counters(b.idx, kind, 1);
                }
                Flow::Next
            }
        }
    }
}
