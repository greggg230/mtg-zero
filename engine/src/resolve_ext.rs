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
            On::MadeAll => f.made_all.iter().copied().filter(|&c| self.objs[c.i()].zone == Zone::Battlefield).collect(),
            On::Each(filter) => {
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source), spell: false };
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

    /// An op's duration as it resolves: "for as long as <the source>
    /// remains on the battlefield" names the source, and is `None` (the
    /// effect does nothing) if it already left (CR 611.2b).
    fn until_for(&self, f: &Frame, until: crate::layers::Until) -> Option<crate::layers::Until> {
        use crate::layers::Until;
        match until {
            Until::WhileSource => (self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield)
                .then_some(Until::WhileOnBattlefield(f.source)),
            u => Some(u),
        }
    }

    pub(crate) fn exec_ext(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::Apply { on, mods, until } => {
                let Some(until) = self.until_for(f, *until) else { return Flow::Next };
                for c in self.objs_on(f, on) {
                    let r = self.obj_ref(c);
                    for m in mods {
                        self.add_effect(Affected::Obj(r), m.clone(), until, f.controller);
                    }
                    self.note(|g| format!("{} gets {mods:?} ({until:?})", g.name(c)));
                }
                Flow::Next
            }
            Ext::ApplyRules { filter, who, m, until } => {
                let Some(until) = self.until_for(f, *until) else { return Flow::Next };
                if let Some(p) = self.who(f, *who) {
                    self.add_effect(Affected::Group(filter.clone()), m.clone(), until, p);
                }
                Flow::Next
            }
            Ext::ProtectionChoice { on, colorless, until } => {
                let objs = self.objs_on(f, on);
                let Some(until) = self.until_for(f, *until) else { return Flow::Next };
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
                            self.add_effect(Affected::Obj(r), Mod::Protection(prot), until, f.controller);
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
                    // (An Equipment with reconfigure may, CR 301.5c.)
                    if (!self.is_creature(e) || self.def_of(e).reconfigure) && self.is_creature(h) && e != h && !self.protected_from(h, &q) {
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
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source), spell: false };
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
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source), spell: false };
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
                        if !self.is_token(o.idx) {
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
                f.made = None;
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
                        // (It becomes the made object: "with a counter on it".)
                        if self.objs[o.idx.i()].zone == Zone::Battlefield {
                            f.made = Some(o.idx);
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
                    let ctx = crate::cast::Ctx { controller: p, source: Some(f.source), spell: false };
                    let mut opts: Vec<Opt> = Vec::new();
                    for c in self.graveyard_cards(p) {
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
                        Target::Stack(_) => false,
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
                        self.ask(Task::Run(f.clone()), owner, DecisionKind::YesNo, opts);
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
                    self.links.push(crate::game::Link { source: f.source, card, until: *until, back, until_monarch: None });
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
                        self.links.push(crate::game::Link { source: f.source, card, until: *until, back: Zone::Hand, until_monarch: None });
                        Flow::Next
                    }
                    Some(Choice::Pass) => Flow::Next,
                    Some(other) => unreachable!("hand choice {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source), spell: false };
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
                if *about == crate::program::About::EachMade {
                    // One about each object made ("sacrifice it": each token
                    // a doubled creation made).
                    for c in f.made_all.clone() {
                        let ev = crate::triggers::EvCtx { obj: Some(self.obj_ref(c)), ..Default::default() };
                        self.add_delayed(f, *k, *when, ev);
                    }
                    return Flow::Next;
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
                    crate::program::About::EachMade => unreachable!("handled above"),
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
                            f.x = self.mana_value_of(o.idx) as i32;
                            break;
                        }
                    }
                }
                Flow::Next
            }
            Ext::TopOfGraveyardToBattlefield { filter } => {
                f.made = None;
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source), spell: false };
                let top = self
                    .graveyard_cards(f.controller)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .find(|&c| self.card_matches(filter, c, &ctx));
                if let Some(c) = top {
                    let owner = self.objs[c.i()].owner;
                    self.put_onto_battlefield(c, owner);
                    f.made = Some(c);
                }
                Flow::Next
            }
            Ext::ApplyPtPerCount { on, filter, p, t } => {
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source), spell: false };
                let n = self.battlefield.iter().filter(|&&c| self.card_matches(filter, c, &ctx)).count() as i32;
                let (dp, dt) = ((*p as i32 * n).clamp(-999, 999), (*t as i32 * n).clamp(-999, 999));
                for c in self.objs_on(f, on) {
                    let r = self.obj_ref(c);
                    self.add_effect(Affected::Obj(r), Mod::Pt(dp, dt), crate::layers::Until::EndOfTurn, f.controller);
                }
                Flow::Next
            }
            Ext::TapOrUntap { on } => {
                let Some(&c) = self.objs_on(f, on).first() else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Number(0)) => self.tap(c),
                    Some(Choice::Number(1)) => self.untap(c),
                    Some(Choice::Pass) => {}
                    Some(other) => unreachable!("tap/untap answer {other:?}"),
                    None => {
                        let opts = vec![
                            Opt { action: act::NUMBER, choice: Choice::Number(0) },
                            Opt { action: act::NUMBER + 1, choice: Choice::Number(1) },
                            Opt { action: act::PASS, choice: Choice::Pass },
                        ];
                        self.ask(Task::Run(f.clone()), f.controller, DecisionKind::ChooseMode, opts);
                        return Flow::Suspend;
                    }
                }
                Flow::Next
            }
            Ext::CreateTokensFor { who, token, n } => {
                if let Some(p) = self.who(f, *who) {
                    let def = self.db.token_def(*token);
                    self.create_tokens(def, p, *n as u32, false);
                }
                Flow::Next
            }
            Ext::CreateTokensAttacking { token, n, tapped } => {
                use crate::game::Step;
                let def = self.db.token_def(*token);
                let in_combat = matches!(
                    self.turn.step,
                    Step::DeclareAttackers | Step::DeclareBlockers | Step::FirstStrikeDamage | Step::CombatDamage
                );
                let made = self.create_tokens(def, f.controller, *n as u32, *tapped);
                for &c in &made {
                    if in_combat && f.controller == self.turn.active && self.objs[c.i()].zone == Zone::Battlefield {
                        self.objs[c.i()].attacking = true;
                    }
                }
                // CR 508.4: their controller chooses what each attacks (asked
                // before the rest of the effect, first token first).
                for &c in made.iter().rev() {
                    if self.objs[c.i()].attacking {
                        self.agenda.push(Task::AttackTarget { card: c });
                    }
                }
                f.made = made.first().copied();
                f.made_all = made;
                Flow::Next
            }
            Ext::GraveyardToBottomRandom { who } => {
                if let Some(p) = self.who(f, *who) {
                    let cards = self.players[p as usize].graveyard.clone();
                    self.put_on_bottom_randomly(p, cards);
                }
                Flow::Next
            }
            Ext::PileRestBottomRandom => {
                let pile = std::mem::take(&mut f.pile);
                if let Some(&c) = pile.first() {
                    let owner = self.objs[c.i()].owner;
                    self.put_on_bottom_randomly(owner, pile);
                }
                Flow::Next
            }
            Ext::DamageFrom { event_obj, amount, tgt, no_lifegain } => {
                let n = self.amount(f, *amount);
                let src = if *event_obj {
                    match f.ev.obj {
                        Some(r) if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield => {
                            self.damage_source_of(r.idx)
                        }
                        // Gone: its characteristics as a card, last controlled
                        // by the ability's controller (CR 608.2h, simplified).
                        Some(r) => crate::game::DamageSource { controller: f.controller, ..self.damage_source_of(r.idx) },
                        None => return Flow::Next,
                    }
                } else {
                    self.frame_damage_source(f)
                };
                for t in self.targets_of(f, *tgt) {
                    let dealt = self.deal_damage(src, t, n);
                    if let (true, Target::Player(p)) = (*no_lifegain && dealt > 0, t) {
                        self.players[p as usize].cant_gain_life = true;
                        self.note(|_| format!("P{p} can't gain life for the rest of the game"));
                    }
                }
                Flow::Next
            }
            Ext::CounterOrDoubleOnSecond { on } => {
                let n = match self.resolved.iter_mut().find(|x| x.0 == f.source) {
                    Some(x) => {
                        x.1 = x.1.saturating_add(1);
                        x.1
                    }
                    None => {
                        self.resolved.push((f.source, 1));
                        1
                    }
                };
                for c in self.objs_on(f, on) {
                    if n == 2 {
                        let have = self.counters_on(c, crate::db::Counter::P1P1);
                        self.add_counters(c, crate::db::Counter::P1P1, have);
                    } else {
                        self.add_counters(c, crate::db::Counter::P1P1, 1);
                    }
                }
                Flow::Next
            }
            Ext::SacrificeThen { filter, optional, skip } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} sacrifices {}", g.name(c)));
                        self.sacrifice(c);
                        Flow::Next
                    }
                    Some(Choice::Pass) => Flow::Jump(*skip),
                    Some(other) => unreachable!("sacrifice answer {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source), spell: false };
                        let mut opts: Vec<Opt> = self
                            .permanents_of(p)
                            .filter(|&c| self.card_matches(filter, c, &ctx))
                            .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                            .collect();
                        if opts.is_empty() {
                            return Flow::Jump(*skip);
                        }
                        if *optional {
                            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::Sacrifice, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::MayRemoveCounterThen { skip } => {
                let Some(&c) = self.objs_on(f, &On::Source).first() else { return Flow::Jump(*skip) };
                let kinds = self.counters_of(c);
                match self.answer.take() {
                    Some(Choice::Number(k)) => {
                        self.remove_counters(c, kinds[k as usize].0, 1);
                        Flow::Next
                    }
                    Some(Choice::Pass) => Flow::Jump(*skip),
                    Some(other) => unreachable!("counter answer {other:?}"),
                    None => {
                        if kinds.is_empty() {
                            return Flow::Jump(*skip);
                        }
                        let mut opts: Vec<Opt> = (0..kinds.len().min(16))
                            .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) })
                            .collect();
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        self.ask(Task::Run(f.clone()), f.controller, DecisionKind::ChooseMode, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::Reflexive { k } => {
                let base = self.db.def(f.source_def).delayed_base;
                // An intervening "if" (CR 603.4): it triggers only if the
                // condition holds now (and is checked again as it resolves).
                let t = &self.db.def(f.source_def).triggers[(base + k) as usize];
                if !t.pred.intervening().into_iter().all(|c| self.cond_holds_from(c, f.controller, Some(f.source))) {
                    self.note(|_| "reflexive trigger: its condition doesn't hold".to_string());
                    return Flow::Next;
                }
                self.waiting.push(crate::triggers::PendingTrigger {
                    controller: f.controller,
                    source: f.source,
                    source_def: f.source_def,
                    slot: base + k,
                    ev: f.ev,
                });
                Flow::Next
            }
            Ext::ExileTopWithPermission { who, n, kind, this_turn } => {
                let Some(q) = self.who(f, *who) else { return Flow::Next };
                for _ in 0..*n {
                    let Some(&top) = self.players[q as usize].library.last() else { break };
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, q);
                    self.reveal(top);
                    let card = self.move_card(top, Zone::Exile, Pos::Top);
                    self.permissions.push(crate::game::Permission {
                        card,
                        player: f.controller,
                        turn: this_turn.then_some(self.turn.number),
                        kind: *kind,
                    });
                }
                Flow::Next
            }
            Ext::GrantFlashback { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone == Zone::Graveyard {
                        let player = self.objs[o.idx.i()].owner;
                        self.permissions.push(crate::game::Permission {
                            card: o,
                            player,
                            turn: Some(self.turn.number),
                            kind: crate::game::PermKind::Flashback,
                        });
                    }
                }
                Flow::Next
            }
            Ext::SacrificeSourceUnlessEscaped => {
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    if !self.objs[c.i()].escaped {
                        self.sacrifice(c);
                    }
                }
                Flow::Next
            }
            Ext::ReturnEventObjToHand => {
                if let Some(&c) = self.objs_on(f, &On::EventObj).first() {
                    self.move_card(c, Zone::Hand, Pos::Top);
                }
                Flow::Next
            }
            Ext::Amass { token, army, subtype, n } => {
                let p = f.controller;
                let armies: Vec<CardIdx> = self.permanents_of(p).filter(|&c| self.has_subtype(c, *army)).collect();
                let target = match self.answer.take() {
                    Some(Choice::Card(c)) => c,
                    Some(other) => unreachable!("army answer {other:?}"),
                    None => {
                        let mut armies = armies;
                        if armies.is_empty() {
                            // (Two of them with a token doubler: one is chosen.)
                            let def = self.db.token_def(*token);
                            self.create_tokens(def, p, 1, false);
                            armies = self.permanents_of(p).filter(|&c| self.has_subtype(c, *army)).collect();
                        }
                        match armies[..] {
                            [] => return Flow::Next,
                            [one] => one,
                            _ => {
                                let opts =
                                    armies.iter().map(|&c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) }).collect();
                                self.ask(Task::Run(f.clone()), p, DecisionKind::ChoosePermanent, opts);
                                return Flow::Suspend;
                            }
                        }
                    }
                };
                self.add_counters(target, crate::db::Counter::P1P1, *n as u16);
                if !self.has_subtype(target, *subtype) {
                    let r = self.obj_ref(target);
                    let m = Mod::Types { add: 0, subtypes: vec![*subtype], replace: false };
                    self.add_effect(Affected::Obj(r), m, crate::layers::Until::Forever, p);
                }
                Flow::Next
            }
            Ext::ReturnDiedWithCounter { counter } => {
                let Some(r) = f.ev.obj else { return Flow::Next };
                let o = &self.objs[r.idx.i()];
                // The card it became in the graveyard (one move later).
                if o.zone == Zone::Graveyard && o.inc == r.inc.wrapping_add(1) && !self.is_token(r.idx) {
                    let owner = o.owner;
                    self.put_onto_battlefield(r.idx, owner);
                    if self.objs[r.idx.i()].zone == Zone::Battlefield {
                        self.add_counters(r.idx, *counter, 1);
                    }
                }
                Flow::Next
            }
            Ext::LinkedOwnersCreateMvToken { token } => {
                let cards: Vec<crate::ids::ObjRef> =
                    self.links.iter().filter(|l| l.source == f.source).map(|l| l.card).collect();
                let def = self.db.token_def(*token);
                for card in cards {
                    let owner = self.objs[card.idx.i()].owner;
                    let x = self.def_of(card.idx).mana_value() as i32;
                    for t in self.create_tokens(def, owner, 1, false) {
                        if self.objs[t.i()].zone == Zone::Battlefield {
                            let r = self.obj_ref(t);
                            self.add_effect(Affected::Obj(r), Mod::SetPt(x, x), crate::layers::Until::Forever, owner);
                        }
                    }
                }
                Flow::Next
            }
            Ext::Monstrosity { n } => {
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    if !self.objs[c.i()].monstrous {
                        self.add_counters(c, crate::db::Counter::P1P1, *n as u16);
                        self.objs[c.i()].monstrous = true;
                        let r = self.obj_ref(c);
                        self.event(crate::triggers::Event::obj(crate::db::EventKind::BecomesMonstrous, r));
                    }
                }
                Flow::Next
            }
            Ext::CreateTokenAttachSource { token } => {
                let def = self.db.token_def(*token);
                // (With a token doubler, it is attached to the first one.)
                let Some(&t) = self.create_tokens(def, f.controller, 1, false).first() else { return Flow::Next };
                if let Some(&eq) = self.objs_on(f, &On::Source).first() {
                    if self.objs[t.i()].zone == Zone::Battlefield {
                        let host = self.obj_ref(t);
                        self.objs[eq.i()].attached = Some(host);
                    }
                }
                Flow::Next
            }
            Ext::LookTopN { who, n } => {
                if let Some(p) = self.who(f, *who) {
                    let n = self.amount(f, *n).clamp(0, 60) as u8;
                    if n > 0 {
                        self.look_top(f, p, n, false);
                    }
                }
                Flow::Next
            }
            Ext::WinGame => {
                // CR 104.2b: an effect states that a player wins.
                let p = f.controller;
                self.note(|_| format!("P{p} wins the game"));
                self.players[crate::ids::opp(p) as usize].lost = true;
                self.outcome = Some(crate::game::Outcome { winner: Some(p), reason: crate::game::EndReason::Effect });
                Flow::Next
            }
            Ext::BounceAll => {
                let all: Vec<CardIdx> = self.battlefield.clone();
                self.begin_batch();
                for c in all {
                    if self.objs[c.i()].zone == Zone::Battlefield {
                        self.move_card(c, Zone::Hand, Pos::Top);
                    }
                }
                self.end_batch();
                Flow::Next
            }
            Ext::VoteExile { filter } => {
                // f.i: votes cast so far (the controller, then the other).
                let order = [f.controller, crate::ids::opp(f.controller)];
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        f.picked.push(c);
                        f.i += 1;
                    }
                    Some(other) => unreachable!("vote answer {other:?}"),
                    None => {}
                }
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source), spell: false };
                while f.i < 2 {
                    let voter = order[f.i as usize];
                    let opts: Vec<Opt> = self
                        .battlefield
                        .iter()
                        .copied()
                        .filter(|&c| self.card_matches(filter, c, &ctx))
                        .map(|c| Opt { action: self.perm_action(voter, c), choice: Choice::Card(c) })
                        .collect();
                    if opts.is_empty() {
                        f.i += 1;
                        continue;
                    }
                    self.ask(Task::Run(f.clone()), voter, DecisionKind::ChoosePermanent, opts);
                    return Flow::Suspend;
                }
                // Two votes: one permanent with both, or both tied with one.
                let mut chosen = std::mem::take(&mut f.picked);
                chosen.dedup();
                chosen.sort_by_key(|c| self.objs[c.i()].timestamp);
                chosen.dedup();
                self.begin_batch();
                for c in chosen {
                    if self.objs[c.i()].zone == Zone::Battlefield {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                }
                self.end_batch();
                Flow::Next
            }
            Ext::MillTwoWhileShareColor { who } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                loop {
                    let mut milled = Vec::new();
                    for _ in 0..2 {
                        let Some(&top) = self.players[p as usize].library.last() else { break };
                        self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                        self.reveal(top);
                        self.move_card(top, Zone::Graveyard, Pos::Top);
                        milled.push(top);
                    }
                    let share = milled.len() == 2 && self.def_of(milled[0]).colors & self.def_of(milled[1]).colors != 0;
                    if !share {
                        break;
                    }
                }
                Flow::Next
            }
            Ext::SacMadeUnlessPayReduced { n } => {
                let Some(c) = f.made.filter(|&c| self.objs[c.i()].zone == Zone::Battlefield) else { return Flow::Next };
                let p = f.controller;
                let cost = self.def_of(c).cost.less_generic(*n);
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        let mut back = f.clone();
                        back.pc += 1;
                        back.i = 0;
                        back.floor = 0;
                        self.agenda.push(Task::Run(back));
                        self.agenda.push(Task::PayMana(crate::pay::PayMana::new(p, cost)));
                        Flow::Suspend
                    }
                    Some(_) => {
                        self.sacrifice(c);
                        Flow::Next
                    }
                    None => {
                        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                        if self.can_pay(p, &cost) {
                            opts.push(Opt { action: act::YES, choice: Choice::Yes });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::RevealUntilToBattlefield { who, types } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Yes) => {}
                    Some(_) => return Flow::Next,
                    None => {
                        let opts = vec![
                            Opt { action: act::PASS, choice: Choice::Pass },
                            Opt { action: act::YES, choice: Choice::Yes },
                        ];
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                        return Flow::Suspend;
                    }
                }
                // Reveal from the top until a card of the types (CR
                // 701.20a); the revealed cards stay in the library until
                // then.
                let lib: Vec<CardIdx> = self.players[p as usize].library.iter().rev().copied().collect();
                let mut revealed = Vec::new();
                let mut found = None;
                for c in lib {
                    self.force_library_card(c, crate::hidden::KEY_REVEAL, p);
                    self.reveal(c);
                    if self.def_of(c).types & types != 0 {
                        found = Some(c);
                        break;
                    }
                    revealed.push(c);
                }
                if let Some(c) = found {
                    self.put_onto_battlefield(c, p);
                }
                for c in revealed {
                    self.move_card(c, Zone::Graveyard, Pos::Top);
                }
                Flow::Next
            }
            Ext::GetEnergy { n } => {
                let pl = &mut self.players[f.controller as usize];
                pl.energy = pl.energy.saturating_add(*n as u16);
                Flow::Next
            }
            Ext::PayEnergyDamage { tgt } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Number(k)) => {
                        self.players[p as usize].energy -= k as u16;
                        let src = self.frame_damage_source(f);
                        for t in self.targets_of(f, *tgt) {
                            self.deal_damage(src, t, k as i32);
                        }
                        Flow::Next
                    }
                    Some(other) => unreachable!("energy answer {other:?}"),
                    None => {
                        let most = self.players[p as usize].energy.min(act::NUMBER_N - 1);
                        let opts = (0..=most).map(|k| Opt { action: act::NUMBER + k, choice: Choice::Number(k as u8) }).collect();
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseMode, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::MayPayEnergyThen { n, skip } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        self.players[p as usize].energy -= *n as u16;
                        Flow::Next
                    }
                    Some(_) => Flow::Jump(*skip),
                    None => {
                        if self.players[p as usize].energy < *n as u16 {
                            return Flow::Jump(*skip);
                        }
                        let opts = vec![
                            Opt { action: act::PASS, choice: Choice::Pass },
                            Opt { action: act::YES, choice: Choice::Yes },
                        ];
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::PileRestExilePlayable => {
                for c in std::mem::take(&mut f.pile) {
                    self.reveal(c);
                    let card = self.move_card(c, Zone::Exile, Pos::Top);
                    self.permissions.push(crate::game::Permission {
                        card,
                        player: f.controller,
                        turn: Some(self.turn.number),
                        kind: crate::game::PermKind::Play { free: false, cast_only: false, any_mana: false, attacked_subtype: None },
                    });
                }
                Flow::Next
            }
            Ext::LookAtTopOf { who } => {
                let Some(q) = self.who(f, *who) else { return Flow::Next };
                let p = f.controller;
                if let Some(&top) = self.players[q as usize].library.last() {
                    self.force_library_card(top, crate::hidden::KEY_LOOK, p);
                    // Knowledge of a library card its owner doesn't know
                    // can't be kept: the owner's determinize may change it
                    // (ENGINE.md 10). The look is keyed; it isn't remembered.
                    if p == q || self.knows(q, top) {
                        self.learn(p, top);
                    }
                    let d = self.objs[top.i()].def;
                    self.emit_private(crate::hidden::KEY_LOOK, p, d as u64);
                    self.note(|_| format!("P{p} looks at the top card of P{q}'s library"));
                }
                Flow::Next
            }
            Ext::PutInLibraryAtX { tgt } => {
                let depth = f.spell_x;
                for t in self.targets_of(f, *tgt) {
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone == Zone::Battlefield {
                            self.move_card(o.idx, Zone::Library, Pos::FromTop(depth));
                        }
                    }
                }
                Flow::Next
            }
            Ext::SacrificeSourceThen { skip } => match self.objs_on(f, &On::Source).first() {
                Some(&c) => {
                    self.sacrifice(c);
                    Flow::Next
                }
                None => Flow::Jump(*skip),
            },
            Ext::MayMillThenTakeCreature => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        // Mill that many (the milled cards are the pile).
                        let n = f.ev.amount as usize;
                        let mut milled = Vec::new();
                        for _ in 0..n {
                            let Some(&top) = self.players[p as usize].library.last() else { break };
                            self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                            self.reveal(top);
                            self.move_card(top, Zone::Graveyard, Pos::Top);
                            milled.push(top);
                        }
                        f.pile = milled;
                        f.i = 1;
                    }
                    Some(Choice::Card(c)) => {
                        self.move_card(c, Zone::Hand, Pos::Top);
                        f.pile.clear();
                        return Flow::Next;
                    }
                    Some(Choice::Pass) => {
                        f.pile.clear();
                        return Flow::Next;
                    }
                    Some(other) => unreachable!("mill answer {other:?}"),
                    None => {}
                }
                if f.i == 0 {
                    let opts = vec![
                        Opt { action: act::PASS, choice: Choice::Pass },
                        Opt { action: act::YES, choice: Choice::Yes },
                    ];
                    self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                    return Flow::Suspend;
                }
                // "You may put a creature card from among them into your hand."
                let mut opts: Vec<Opt> = f
                    .pile
                    .iter()
                    .copied()
                    .filter(|&c| self.objs[c.i()].zone == Zone::Graveyard && self.def_of(c).is(crate::db::types::CREATURE))
                    .map(|c| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) })
                    .collect();
                if opts.is_empty() {
                    f.pile.clear();
                    return Flow::Next;
                }
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseGraveyardCard, opts);
                Flow::Suspend
            }
            Ext::LinkedCreaturesToBattlefield { flying, becomes } => {
                let cards: Vec<crate::ids::ObjRef> = self
                    .links
                    .iter()
                    .filter(|l| l.source == f.source)
                    .map(|l| l.card)
                    .filter(|r| self.live(*r) && self.objs[r.idx.i()].zone == Zone::Exile)
                    .filter(|r| self.def_of(r.idx).is(crate::db::types::CREATURE))
                    .collect();
                let p = f.controller;
                for r in cards {
                    let new = self.put_onto_battlefield(r.idx, p);
                    if self.objs[r.idx.i()].zone != Zone::Battlefield {
                        continue;
                    }
                    if *flying {
                        self.add_counters(r.idx, crate::db::Counter::Kw(crate::db::Kw::Flying), 1);
                    }
                    if let Some((sub, pw, tg)) = becomes {
                        let m = Mod::Types { add: 0, subtypes: vec![*sub], replace: false };
                        self.add_effect(Affected::Obj(new), m, crate::layers::Until::Forever, p);
                        self.add_effect(Affected::Obj(new), Mod::SetPt(*pw, *tg), crate::layers::Until::Forever, p);
                    }
                }
                Flow::Next
            }
            Ext::LookAtRandomHandCard { who } => {
                let Some(q) = self.who(f, *who) else { return Flow::Next };
                let p = f.controller;
                let sorted = self.hand_sorted(q);
                if sorted.is_empty() {
                    return Flow::Next;
                }
                let forced = self
                    .forced_def(crate::hidden::KEY_LOOK, p)
                    .and_then(|d| sorted.iter().copied().find(|&c| self.objs[c.i()].def == d));
                let c = forced.unwrap_or_else(|| sorted[self.rng.below(sorted.len() as u32) as usize]);
                // Its owner knows their hand: the looker may remember it.
                self.learn(p, c);
                let d = self.objs[c.i()].def;
                self.emit_private(crate::hidden::KEY_LOOK, p, d as u64);
                self.note(|_| format!("P{p} looks at a card at random in P{q}'s hand"));
                Flow::Next
            }
            Ext::RevealTopOnePerType { n } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        f.picked.push(c);
                        f.pile.retain(|&x| x != c);
                    }
                    Some(Choice::Pass) => {
                        f.i = 2;
                    }
                    Some(other) => unreachable!("reveal answer {other:?}"),
                    None => {}
                }
                if f.i == 0 {
                    self.look_top(f, p, *n, true);
                    f.picked.clear();
                    f.i = 1;
                }
                if f.i == 1 {
                    // A further card is offered if every chosen card can
                    // still stand for a different card type (a matching).
                    let mut seen = Vec::new();
                    let mut opts = Vec::new();
                    let mut sorted = f.pile.clone();
                    sorted.sort_by_key(|c| (self.objs[c.i()].def, c.0));
                    for (k, &c) in sorted.iter().enumerate() {
                        let d = self.objs[c.i()].def;
                        if seen.contains(&d) {
                            continue;
                        }
                        seen.push(d);
                        let mut set: Vec<u16> = f.picked.iter().map(|&x| self.def_of(x).types).collect();
                        set.push(self.def_of(c).types);
                        if crate::resolve_ext::types_matchable(&set) {
                            opts.push(Opt { action: act::LIBRARY + k as u16, choice: Choice::Card(c) });
                        }
                    }
                    if !opts.is_empty() {
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        self.ask(Task::Run(f.clone()), p, DecisionKind::PutInHand, opts);
                        return Flow::Suspend;
                    }
                }
                // The chosen cards to hand; the rest at random to the bottom.
                for c in std::mem::take(&mut f.picked) {
                    self.move_card(c, Zone::Hand, Pos::Top);
                }
                let rest = std::mem::take(&mut f.pile);
                self.put_on_bottom_randomly(p, rest);
                Flow::Next
            }
            Ext::GiveSourceToOpponent => {
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    let q = crate::ids::opp(f.controller);
                    self.change_control(c, q);
                }
                Flow::Next
            }
            Ext::ExileEventObj2 => {
                if let Some(r) = f.ev.obj2 {
                    if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield {
                        self.move_card(r.idx, Zone::Exile, Pos::Top);
                    }
                }
                Flow::Next
            }
            Ext::JumpUnlessSourceNoCounters { counter, to } => {
                let n = match self.objs_on(f, &On::Source).first() {
                    Some(&c) => self.counters_on(c, *counter),
                    None => self.lki_counters.iter().filter(|x| x.0 == f.source && x.1 == *counter).map(|x| x.2).sum(),
                };
                if n == 0 {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            Ext::GraveyardCastPermission { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone == Zone::Graveyard {
                        self.permissions.push(crate::game::Permission {
                            card: o,
                            player: f.controller,
                            turn: Some(self.turn.number),
                            kind: crate::game::PermKind::Play { free: false, cast_only: true, any_mana: false, attacked_subtype: None },
                        });
                    }
                }
                Flow::Next
            }
            Ext::BecomeMonarch => {
                self.set_monarch(f.controller);
                Flow::Next
            }
            Ext::ExileUntilMonarch { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    let card = self.move_card(o.idx, Zone::Exile, Pos::Top);
                    self.links.push(crate::game::Link {
                        source: f.source,
                        card,
                        until: false,
                        back: Zone::Battlefield,
                        until_monarch: Some(f.controller),
                    });
                }
                Flow::Next
            }
            Ext::RevealUntilCreatureAttacking => {
                use crate::game::Step;
                let p = f.controller;
                let lib: Vec<CardIdx> = self.players[p as usize].library.iter().rev().copied().collect();
                let mut rest = Vec::new();
                let mut found = None;
                for c in lib {
                    self.force_library_card(c, crate::hidden::KEY_REVEAL, p);
                    self.reveal(c);
                    if self.def_of(c).is(crate::db::types::CREATURE) {
                        found = Some(c);
                        break;
                    }
                    rest.push(c);
                }
                if let Some(c) = found {
                    self.put_onto_battlefield(c, p);
                    if self.objs[c.i()].zone == Zone::Battlefield {
                        self.tap(c);
                        let in_combat = matches!(
                            self.turn.step,
                            Step::DeclareAttackers | Step::DeclareBlockers | Step::FirstStrikeDamage | Step::CombatDamage
                        );
                        if in_combat && p == self.turn.active && self.is_creature(c) {
                            self.objs[c.i()].attacking = true;
                            // CR 508.4: its controller chooses what it attacks.
                            self.agenda.push(Task::AttackTarget { card: c });
                        }
                    }
                }
                self.put_on_bottom_randomly(p, rest);
                Flow::Next
            }
            Ext::ExileThisSpell => {
                let c = f.source.idx;
                if self.live(f.source) && self.objs[c.i()].zone == Zone::Stack {
                    self.move_card(c, Zone::Exile, Pos::Top);
                }
                Flow::Next
            }
            Ext::TokensCreated => {
                // (For the player who created them: "each player other than
                // its controller creates" too.)
                if let Some(c) = f.made.filter(|&c| self.def_of(c).is(crate::db::types::CREATURE)) {
                    let p = self.objs[c.i()].controller;
                    self.event(crate::triggers::Event::player(crate::db::EventKind::CreatureTokensCreated, p));
                }
                Flow::Next
            }
            Ext::CreateEmblem { .. }
            | Ext::RemoveCounters { .. }
            | Ext::GainControl { .. }
            | Ext::ExchangeControl { .. }
            | Ext::GainControlOfEventSpellTargets { .. }
            | Ext::RollDie { .. }
            | Ext::ExtraLoyaltyActivations { .. }
            | Ext::ChooseFromGraveyardToHand { .. }
            | Ext::DamageChosen { .. }
            | Ext::ExileTopMayCast { .. }
            | Ext::ExileLibrary { .. }
            | Ext::LookAtTopMayBottom { .. }
            | Ext::NinjutsuEnter
            | Ext::RevealTopOpponentChooses { .. }
            | Ext::ExiledWithCounterToHand { .. }
            | Ext::SacrificeThenReflexive { .. }
            | Ext::JumpUnlessEventObj { .. }
            | Ext::SearchAnyNumber { .. }
            | Ext::NameRevealTopToHand { .. }
            | Ext::FlashSorceries => self.exec_walker(f, op),
            Ext::TransformSource
            | Ext::ExileReturnTransformed { .. }
            | Ext::OnceMay { .. }
            | Ext::GraveyardCastThenExile { .. }
            | Ext::TriggerUntilYourNextTurn { .. }
            | Ext::EachExileUntilNonlandCastFree
            | Ext::Poison { .. }
            | Ext::DiscardUpToThenDraw { .. }
            | Ext::KeepOneOfEachType { .. }
            | Ext::Connive { .. } => self.exec_faces(f, op),
            Ext::SourceOfTargetLosesAbilities { .. }
            | Ext::GainControlOfSpell { .. }
            | Ext::CounterEventItem
            | Ext::OfferPayOther { .. }
            | Ext::TakeInitiative { .. }
            | Ext::Venture { .. }
            | Ext::PileToBattlefield { .. }
            | Ext::GetExperience { .. }
            | Ext::CreateTokensN { .. }
            | Ext::DiscardThenDraw { .. }
            | Ext::SourceFromGraveyard { .. } => self.exec_stack(f, op),
            Ext::RemoveAllCountersAddMana { .. }
            | Ext::LetDrawElseMill { .. }
            | Ext::ExileRandomGraveyardPlayable
            | Ext::NameExileRevealUntil { .. }
            | Ext::Balance
            | Ext::MillAttackPump { .. }
            | Ext::SacrificeTutorPayDifference
            | Ext::LookTopRepeatPayLife { .. }
            | Ext::LootMayCastFree { .. }
            | Ext::ExileFromHandTaxed { .. }
            | Ext::ExileHandCardOrTargetUntil { .. }
            | Ext::ExileUntilNonlandCounterOrCast
            | Ext::ExileAllCreaturesIncubate { .. }
            | Ext::GrantLeaveTrigger { .. }
            | Ext::SwapWithGraveyardCard { .. } => self.exec_tail(f, op),
            Ext::SacrificeN { .. }
            | Ext::FromGraveyardAs { .. }
            | Ext::ReturnEventCardAs { .. }
            | Ext::Harness
            | Ext::ExilePlayUntilNextTurn { .. }
            | Ext::ExileEventCardLinked { .. }
            | Ext::LinkedToGraveyardThenToken { .. }
            | Ext::PlayFromGraveyardThisTurn
            | Ext::GraveyardToExileThisTurn
            | Ext::Cascade
            | Ext::CopyTokensEnteredThisTurn
            | Ext::MillThenLandToHand { .. }
            | Ext::MayCastFromGraveyardNow { .. }
            | Ext::ExtraCombat
            | Ext::WarpExile
            | Ext::MiracleCast
            | Ext::Airbend { .. } => self.exec_tail2(f, op),
            Ext::ApplyPtX { .. }
            | Ext::ExileAll { .. }
            | Ext::TokensFor { .. }
            | Ext::PlayerProtection
            | Ext::ExileTopUntilEndStep { .. }
            | Ext::ShuffleSourceIntoLibrary
            | Ext::AttachTargetToSource { .. }
            | Ext::RemoveCounterOrSacrifice { .. }
            | Ext::LinkedToBattlefieldOwners
            | Ext::BecomeSaddled
            | Ext::AttachMadeToEventObj
            | Ext::LinkedCreatureToBattlefield { .. }
            | Ext::OwnerPutsSecondOrBottom { .. }
            | Ext::EventPlayerTakesSourceUntapped
            | Ext::ExileTopFaceDownPlayable
            | Ext::HandsFaceDownLinked
            | Ext::LinkedFaceDownToHand
            | Ext::PlayLinkedFaceDownFree { .. }
            | Ext::ReanimateAttach { .. }
            | Ext::SacrificeReanimated => self.exec_tail3(f, op),
            Ext::M7d(x) => self.exec_m7d(f, x),
            Ext::CreateTokenCopy { .. }
            | Ext::BecomeCopy { .. }
            | Ext::CopySpell { .. }
            | Ext::ExileEventCard
            | Ext::BecomePrepared { .. } => {
                self.exec_copies(f, op)
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

/// Can each card (by its card types) stand for a different card type
/// (Atraxa: "for each card type, a card of that type")? A bipartite
/// matching of cards to the eight card types.
pub(crate) fn types_matchable(cards: &[u16]) -> bool {
    fn assign(i: usize, cards: &[u16], used: u16) -> bool {
        if i == cards.len() {
            return true;
        }
        (0..16).any(|b| {
            let bit = 1u16 << b;
            cards[i] & bit != 0 && used & bit == 0 && assign(i + 1, cards, used | bit)
        })
    }
    cards.len() <= 8 && assign(0, cards, 0)
}
