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
            On::Made => match f.made {
                Some(c) if self.objs[c.i()].zone == Zone::Battlefield => vec![c],
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

    /// Create delayed triggered ability `k` of the ability's card (CR 603.7).
    fn add_delayed(&mut self, f: &Frame, k: u8, when: crate::db::DelayedWhen, ev: crate::triggers::EvCtx) {
        let base = self.db.def(f.source_def).delayed_base;
        self.delayed.push(crate::game::Delayed {
            when,
            controller: f.controller,
            source: f.source,
            source_def: f.source_def,
            slot: base + k,
            ev,
        });
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
            Ext::DealDivided { to } => {
                let src = self.frame_damage_source(f);
                let mut k = 0;
                for (i, &(r, t)) in f.targets.clone().iter().enumerate() {
                    if r != *to {
                        continue;
                    }
                    let share = f.division.get(k).copied().unwrap_or(0) as i32;
                    k += 1;
                    let live = match t {
                        Target::Obj(o) => self.live(o),
                        Target::Player(_) => true,
                    };
                    // CR 608.2b: an illegal target is not dealt its share.
                    if f.legal & (1 << i) != 0 && live {
                        self.deal_damage(src, t, share);
                    }
                }
                Flow::Next
            }
            Ext::SpellToLibraryOwnerChooses { tgt } => {
                let spells: Vec<CardIdx> = self
                    .targets_of(f, *tgt)
                    .into_iter()
                    .filter_map(|t| match t {
                        Target::Obj(o) if self.objs[o.idx.i()].zone == Zone::Stack => Some(o.idx),
                        _ => None,
                    })
                    .collect();
                let Some(&c) = spells.first() else { return Flow::Next };
                let owner = self.objs[c.i()].owner;
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        self.move_card(c, Zone::Library, Pos::Top);
                        Flow::Next
                    }
                    Some(_) => {
                        self.move_card(c, Zone::Library, Pos::Bottom);
                        Flow::Next
                    }
                    None => {
                        let opts =
                            vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                        self.ask(Task::Run(f.clone()), owner, DecisionKind::TopOrBottom, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::JumpUnlessAlt { to } => {
                if f.alt {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            Ext::SacrificeSource => {
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    self.sacrifice(c);
                }
                Flow::Next
            }
            Ext::ExileLinked { tgt, until } => {
                // CR 610.3c: if the source left first, nothing is exiled.
                if *until && self.objs_on(f, &On::Source).is_empty() {
                    return Flow::Next;
                }
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    let back = self.objs[o.idx.i()].zone;
                    let card = self.move_card(o.idx, Zone::Exile, Pos::Top);
                    self.links.push(crate::game::Link { source: f.source, card, until: *until, back });
                }
                Flow::Next
            }
            Ext::ExileFromHandLinked { chooser, whose, filter, optional, until } => {
                let (Some(p), Some(owner)) = (self.who(f, *chooser), self.who(f, *whose)) else { return Flow::Next };
                if *until && self.objs_on(f, &On::Source).is_empty() {
                    return Flow::Next;
                }
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} exiles {} from P{owner}'s hand", g.name(c)));
                        let card = self.move_card(c, Zone::Exile, Pos::Top);
                        self.links.push(crate::game::Link { source: f.source, card, until: *until, back: Zone::Hand });
                        Flow::Next
                    }
                    Some(Choice::Pass) => Flow::Next,
                    Some(other) => unreachable!("hand choice {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                        let hand = self.hand_sorted(owner);
                        let mut opts = Vec::new();
                        for (k, &c) in hand.iter().enumerate() {
                            let d = self.objs[c.i()].def;
                            if k > 0 && self.objs[hand[k - 1].i()].def == d {
                                continue;
                            }
                            debug_assert!(self.knows(p, c), "the chooser has seen the hand");
                            if self.card_matches(filter, c, &ctx) {
                                assert!((k as u16) < act::LIBRARY_N, "hand slot {k} out of range");
                                opts.push(Opt { action: act::LIBRARY + k as u16, choice: Choice::Card(c) });
                            }
                        }
                        // With nothing to choose the decision is still asked
                        // when optional (the hand is hidden from the other
                        // player); a required choice of nothing ends it.
                        if opts.is_empty() && !*optional {
                            return Flow::Next;
                        }
                        if *optional || opts.is_empty() {
                            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseDiscard, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::ReturnLinkedToHand => {
                let mine: Vec<crate::game::Link> = self.links.iter().copied().filter(|l| l.source == f.source).collect();
                self.links.retain(|l| l.source != f.source);
                for l in mine {
                    if self.live(l.card) && self.objs[l.card.idx.i()].zone == Zone::Exile {
                        self.move_card(l.card.idx, Zone::Hand, Pos::Top);
                    }
                }
                Flow::Next
            }
            Ext::Delay { k, when, about } => {
                if *about == crate::program::About::Made && f.made.is_none() {
                    return Flow::Next; // nothing to be about
                }
                let ev = match about {
                    crate::program::About::Nothing => Default::default(),
                    crate::program::About::Made => crate::triggers::EvCtx {
                        obj: f.made.map(|c| self.obj_ref(c)),
                        ..Default::default()
                    },
                    crate::program::About::Remembered => {
                        crate::triggers::EvCtx { amount: f.x.max(0) as u16, ..Default::default() }
                    }
                };
                self.add_delayed(f, *k, *when, ev);
                Flow::Next
            }
            Ext::ExileThenDelay { tgt, k, when } => {
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    let card = self.move_card(o.idx, Zone::Exile, Pos::Top);
                    // A token ceases to exist (704.5d); the delayed trigger
                    // then finds nothing.
                    let ev = crate::triggers::EvCtx { obj: Some(card), ..Default::default() };
                    self.add_delayed(f, *k, *when, ev);
                }
                Flow::Next
            }
            Ext::ReturnEventCard => {
                f.made = None;
                if let Some(r) = f.ev.obj {
                    let z = self.objs[r.idx.i()].zone;
                    if self.live(r) && matches!(z, Zone::Exile | Zone::Graveyard) {
                        let owner = self.objs[r.idx.i()].owner;
                        self.put_onto_battlefield(r.idx, owner);
                        f.made = Some(r.idx);
                    }
                }
                Flow::Next
            }
            Ext::JumpUnlessMadeYours { to } => {
                let yours = f.made.is_some_and(|c| {
                    self.objs[c.i()].zone == Zone::Battlefield && self.objs[c.i()].controller == f.controller
                });
                if yours {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            Ext::SacrificeEventObj => {
                if let Some(&c) = self.objs_on(f, &On::EventObj).first() {
                    self.sacrifice(c);
                }
                Flow::Next
            }
            Ext::ExileEventObj => {
                if let Some(&c) = self.objs_on(f, &On::EventObj).first() {
                    self.move_card(c, Zone::Exile, Pos::Top);
                }
                Flow::Next
            }
            Ext::AddColorless { n } => {
                let n = self.amount(f, *n).clamp(0, 255) as u8;
                self.players[f.controller as usize].pool.amt[5] =
                    self.players[f.controller as usize].pool.amt[5].saturating_add(n);
                Flow::Next
            }
            Ext::RememberSpellMv { tgt } => {
                f.x = 0;
                for t in self.targets_of(f, *tgt) {
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone == Zone::Stack {
                            f.x = self.def_of(o.idx).mana_value() as i32;
                            break;
                        }
                    }
                }
                Flow::Next
            }
            Ext::TopOfGraveyardToBattlefield { filter } => {
                f.made = None;
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let top = self.players[f.controller as usize]
                    .graveyard
                    .iter()
                    .rev()
                    .copied()
                    .find(|&c| self.card_matches(filter, c, &ctx));
                if let Some(c) = top {
                    let owner = self.objs[c.i()].owner;
                    self.put_onto_battlefield(c, owner);
                    f.made = Some(c);
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
