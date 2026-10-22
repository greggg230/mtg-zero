//! The `Ext` instructions of the effect VM (ENGINE.md 7): continuous
//! effects, counters, tapping, attaching, fighting, milling.

use crate::db::{Mod, Prot};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, Target, Zone};
use crate::layers::Affected;
use crate::program::{Ext, On};
use crate::resolve::{Flow, Frame};

impl Game {
    /// The objects an `Ext` op acts on, in canonical order (permanents on
    /// the battlefield only).
    pub(crate) fn objs_on(&self, f: &Frame, on: &On) -> Vec<CardIdx> {
        let on_bf = |r: crate::ids::ObjRef| self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield;
        match on {
            On::Target(i) => self
                .targets_of(f, *i)
                .into_iter()
                .filter_map(|t| match t {
                    Target::Obj(r) if on_bf(r) => Some(r.idx),
                    _ => None,
                })
                .collect(),
            On::Source => {
                if on_bf(f.source) {
                    vec![f.source.idx]
                } else {
                    Vec::new()
                }
            }
            On::EventObj => match f.ev.obj {
                Some(r) if on_bf(r) => vec![r.idx],
                _ => Vec::new(),
            },
            On::Each(filter) => {
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                self.battlefield.iter().copied().filter(|&c| self.card_matches(filter, c, &ctx)).collect()
            }
        }
    }

    /// The end of `Ext::Choose`: act on every chosen permanent at once.
    fn finish_choose(&mut self, f: &mut Frame, what: crate::program::ChooseAct) -> Flow {
        let chosen: Vec<CardIdx> = std::mem::take(&mut f.picked)
            .into_iter()
            .filter(|&c| self.objs[c.i()].zone == Zone::Battlefield)
            .collect();
        match what {
            crate::program::ChooseAct::Untap => {
                for c in chosen {
                    self.untap(c);
                }
            }
            crate::program::ChooseAct::Destroy => {
                self.begin_batch();
                for c in chosen {
                    self.destroy(c);
                }
                self.end_batch();
            }
        }
        Flow::Next
    }

    pub(crate) fn exec_ext(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::Apply { on, mods, until } => {
                for c in self.objs_on(f, on) {
                    let r = self.obj_ref(c);
                    for m in mods {
                        self.add_effect(Affected::Obj(r), m.clone(), *until, f.controller);
                    }
                    self.note(|g| format!("{} gets {mods:?} ({until:?})", g.name(c)));
                }
                Flow::Next
            }
            Ext::ApplyRules { filter, who, m, until } => {
                if let Some(p) = self.who(f, *who) {
                    self.add_effect(Affected::Group(filter.clone()), m.clone(), *until, p);
                }
                Flow::Next
            }
            Ext::ProtectionChoice { on, colorless, until } => {
                let objs = self.objs_on(f, on);
                if objs.is_empty() {
                    return Flow::Next;
                }
                match self.answer.take() {
                    Some(Choice::Number(k)) => {
                        let prot = if k == 5 {
                            Prot { colorless: true, ..Prot::default() }
                        } else {
                            Prot { colors: 1 << k, ..Prot::default() }
                        };
                        for c in objs {
                            let r = self.obj_ref(c);
                            self.add_effect(Affected::Obj(r), Mod::Protection(prot), *until, f.controller);
                        }
                        Flow::Next
                    }
                    Some(other) => unreachable!("colour answer {other:?}"),
                    None => {
                        let n = if *colorless { 6 } else { 5 };
                        let opts =
                            (0..n).map(|k| Opt { action: act::NUMBER + k, choice: Choice::Number(k as u8) }).collect();
                        self.ask(Task::Run(f.clone()), f.controller, DecisionKind::ChooseColor, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::PutCounters { on, counter, n } => {
                let n = self.amount(f, *n).clamp(0, u16::MAX as i32) as u16;
                for c in self.objs_on(f, on) {
                    self.add_counters(c, *counter, n);
                }
                Flow::Next
            }
            Ext::Tap { on } => {
                for c in self.objs_on(f, on) {
                    self.tap(c);
                }
                Flow::Next
            }
            Ext::Untap { on } => {
                for c in self.objs_on(f, on) {
                    self.untap(c);
                }
                Flow::Next
            }
            Ext::Attach { tgt } => {
                let eq = self.objs_on(f, &On::Source);
                let host = self.objs_on(f, &On::Target(*tgt));
                // CR 701.3b, 301.5c: an Equipment that is also a creature
                // can't equip; nothing happens if either is gone.
                if let (Some(&e), Some(&h)) = (eq.first(), host.first()) {
                    let q = self.damage_source_of(e);
                    if !self.is_creature(e) && self.is_creature(h) && e != h && !self.protected_from(h, &q) {
                        let hr = self.obj_ref(h);
                        self.objs[e.i()].attached = Some(hr);
                        self.note(|g| format!("{} attached to {}", g.name(e), g.name(h)));
                    }
                }
                Flow::Next
            }
            Ext::Fight { tgt } => {
                let me = self.objs_on(f, &On::Source);
                let other = self.objs_on(f, &On::Target(*tgt));
                // CR 701.14b: no damage if either is gone or not a creature.
                if let (Some(&a), Some(&b)) = (me.first(), other.first()) {
                    if self.is_creature(a) && self.is_creature(b) {
                        let (sa, sb) = (self.damage_source_of(a), self.damage_source_of(b));
                        let (pa, pb) = (self.power(a), self.power(b));
                        let (ra, rb) = (self.obj_ref(a), self.obj_ref(b));
                        // Both deal damage at once (701.14a).
                        self.deal_damage(sa, Target::Obj(rb), pa);
                        if a != b {
                            self.deal_damage(sb, Target::Obj(ra), pb);
                        }
                    }
                }
                Flow::Next
            }
            Ext::Mill { who, n } => {
                if let Some(p) = self.who(f, *who) {
                    for _ in 0..self.amount(f, *n).max(0) {
                        let Some(&top) = self.players[p as usize].library.last() else { break };
                        self.move_card(top, Zone::Graveyard, Pos::Top);
                    }
                }
                Flow::Next
            }
            Ext::JumpUnlessSource { filter, to } => {
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let ok = self.live(f.source)
                    && self.objs[f.source.idx.i()].zone == Zone::Battlefield
                    && self.card_matches(filter, f.source.idx, &ctx);
                if ok {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            Ext::DiscardThen { skip, optional } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} discards {}", g.name(c)));
                        self.discard(c);
                        Flow::Next
                    }
                    Some(Choice::Pass) => Flow::Jump(*skip),
                    Some(other) => unreachable!("discard answer {other:?}"),
                    None => {
                        if self.players[p as usize].hand.is_empty() {
                            return Flow::Jump(*skip);
                        }
                        let mut opts = self.hand_options(p);
                        if *optional {
                            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::Choose { who, filter, min, max, act: what } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Card(c)) => f.picked.push(c),
                    Some(Choice::Pass) => return self.finish_choose(f, *what),
                    Some(other) => unreachable!("choose answer {other:?}"),
                    None => {}
                }
                // The next pick comes after the last one in canonical order
                // (each set of permanents is reachable once).
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let floor = f.picked.last().map(|&c| self.objs[c.i()].timestamp);
                let cands: Vec<CardIdx> = self
                    .battlefield
                    .iter()
                    .copied()
                    .filter(|&c| floor.is_none_or(|t| self.objs[c.i()].timestamp > t))
                    .filter(|&c| self.card_matches(filter, c, &ctx))
                    .collect();
                if f.picked.len() as u8 >= *max || cands.is_empty() {
                    return self.finish_choose(f, *what);
                }
                let mut opts: Vec<Opt> =
                    cands.iter().map(|&c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) }).collect();
                if f.picked.len() as u8 >= *min {
                    opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                }
                self.ask(Task::Run(f.clone()), p, DecisionKind::ChoosePermanent, opts);
                Flow::Suspend
            }
            Ext::Flicker { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone != Zone::Battlefield {
                            continue;
                        }
                        let owner = self.objs[o.idx.i()].owner;
                        self.move_card(o.idx, Zone::Exile, Pos::Top);
                        // CR 111.7: a token that left can't come back.
                        if !self.def_of(o.idx).token {
                            self.put_onto_battlefield(o.idx, owner);
                        }
                    }
                }
                Flow::Next
            }
            Ext::DoubleCounters { on, counter } => {
                for c in self.objs_on(f, on) {
                    let n = self.counters_on(c, *counter);
                    self.add_counters(c, *counter, n);
                }
                Flow::Next
            }
            Ext::FlipCoin { won } => {
                let p = f.controller;
                let win = match self.forced_coin(p) {
                    Some(w) => w,
                    None => self.rng.below(2) == 0,
                };
                self.emit_public(crate::hidden::KEY_COIN, p, win as u64);
                self.note(|_| format!("P{p} {} the flip", if win { "wins" } else { "loses" }));
                if win {
                    Flow::Jump(*won)
                } else {
                    Flow::Next
                }
            }
            Ext::JumpUnlessLkiYours { to } => {
                if f.lki.is_some_and(|l| l.1 == f.controller) {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            Ext::MayAttachToMade => {
                let eq = self.objs_on(f, &On::Source);
                let token = f.made.filter(|&t| self.objs[t.i()].zone == Zone::Battlefield);
                let (Some(&e), Some(t)) = (eq.first(), token) else { return Flow::Next };
                if self.is_creature(e) || !self.is_creature(t) {
                    return Flow::Next;
                }
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        let tr = self.obj_ref(t);
                        if !self.protected_from(t, &self.damage_source_of(e)) {
                            self.objs[e.i()].attached = Some(tr);
                        }
                        Flow::Next
                    }
                    Some(_) => Flow::Next,
                    None => {
                        let opts = vec![
                            Opt { action: act::PASS, choice: Choice::Pass },
                            Opt { action: act::YES, choice: Choice::Yes },
                        ];
                        self.ask(Task::Run(f.clone()), f.controller, DecisionKind::YesNo, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::Explore { on } => {
                // The creature is chosen when the op starts; the yes/no is
                // asked after the reveal (the card stays on top meanwhile).
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        if let Some(c) = f.picked.pop() {
                            if self.objs[c.i()].zone == Zone::Library {
                                self.move_card(c, Zone::Graveyard, Pos::Top);
                            }
                        }
                        return Flow::Next;
                    }
                    Some(_) => {
                        f.picked.clear();
                        return Flow::Next;
                    }
                    None => {}
                }
                if !f.picked.is_empty() {
                    // Re-asked after a refresh: the revealed card is pending.
                    let opts =
                        vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                    let owner = self.objs[f.picked[0].i()].owner;
                    self.ask(Task::Run(f.clone()), owner, DecisionKind::YesNo, opts);
                    return Flow::Suspend;
                }
                let Some(&c) = self.objs_on(f, on).first() else { return Flow::Next };
                let p = self.objs[c.i()].controller;
                let Some(&top) = self.players[p as usize].library.last() else {
                    // CR 701.44c: nothing revealed is not a land.
                    self.add_counters(c, crate::db::Counter::P1P1, 1);
                    return Flow::Next;
                };
                self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                self.reveal(top);
                if self.def_of(top).is(crate::db::types::LAND) {
                    self.move_card(top, Zone::Hand, Pos::Top);
                    return Flow::Next;
                }
                self.add_counters(c, crate::db::Counter::P1P1, 1);
                f.picked.push(top);
                let opts = vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                Flow::Suspend
            }
            Ext::FromGraveyard { tgt, to_battlefield, yours, tapped } => {
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone != Zone::Graveyard {
                        continue;
                    }
                    f.x = self.def_of(o.idx).mana_value() as i32;
                    if *to_battlefield {
                        let ctl = if *yours { f.controller } else { self.objs[o.idx.i()].owner };
                        self.put_onto_battlefield(o.idx, ctl);
                        if *tapped {
                            self.tap(o.idx);
                        }
                    } else {
                        self.move_card(o.idx, Zone::Hand, Pos::Top);
                    }
                }
                Flow::Next
            }
            Ext::EachPutFromGraveyard { filter } => {
                // f.i: 0 = the active player chooses, 1 = the other, 2 = put.
                let order = [self.turn.active, crate::ids::opp(self.turn.active)];
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        f.picked.push(c);
                        f.i += 1;
                    }
                    Some(other) => unreachable!("graveyard choice {other:?}"),
                    None => {}
                }
                while f.i < 2 {
                    let p = order[f.i as usize];
                    let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                    let mut opts: Vec<Opt> = Vec::new();
                    for &c in &self.players[p as usize].graveyard {
                        if self.card_matches(filter, c, &ctx) {
                            opts.push(Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) });
                        }
                    }
                    if opts.is_empty() {
                        f.i += 1;
                        continue;
                    }
                    self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseGraveyardCard, opts);
                    return Flow::Suspend;
                }
                let chosen = std::mem::take(&mut f.picked);
                for c in chosen {
                    let owner = self.objs[c.i()].owner;
                    if self.objs[c.i()].zone == Zone::Graveyard {
                        self.put_onto_battlefield(c, owner);
                    }
                }
                Flow::Next
            }
            Ext::ExileGraveyard { who } => {
                if let Some(p) = self.who(f, *who) {
                    for c in self.players[p as usize].graveyard.clone() {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                }
                Flow::Next
            }
            Ext::RevealTopToHand { who } => {
                f.x = 0;
                if let Some(p) = self.who(f, *who) {
                    if let Some(&top) = self.players[p as usize].library.last() {
                        self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                        self.reveal(top);
                        f.x = self.def_of(top).mana_value() as i32;
                        self.move_card(top, Zone::Hand, Pos::Top);
                    }
                }
                Flow::Next
            }
        }
    }
}
