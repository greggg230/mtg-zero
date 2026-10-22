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
                let n = self.amount(f, *n).max(0) as u16;
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
            Ext::MayDiscard { skip } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} discards {}", g.name(c)));
                        self.move_card(c, Zone::Graveyard, Pos::Top);
                        Flow::Next
                    }
                    Some(Choice::Pass) => Flow::Jump(*skip),
                    Some(other) => unreachable!("discard answer {other:?}"),
                    None => {
                        if self.players[p as usize].hand.is_empty() {
                            return Flow::Jump(*skip);
                        }
                        let mut opts = self.hand_options(p);
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                        Flow::Suspend
                    }
                }
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
