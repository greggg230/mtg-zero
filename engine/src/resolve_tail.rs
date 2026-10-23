//! The `Ext` instructions of the single-card tail (M7c, ENGINE.md 28).

use crate::db::{types, Counter, Filter, Mod};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, DefId, PlayerId, Target, Zone};
use crate::layers::{Affected, Until};
use crate::program::{Ext, On};
use crate::resolve::{Flow, Frame};

fn yes_no() -> Vec<Opt> {
    vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }]
}

impl Game {
    pub(crate) fn exec_tail(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::RemoveAllCountersAddMana { counter } => self.remove_all_add_mana(f, *counter),
            Ext::LetDrawElseMill { tgt, counter } => self.let_draw_else_mill(f, *tgt, *counter),
            Ext::ExileRandomGraveyardPlayable => {
                let p = f.controller;
                let gy: Vec<CardIdx> = self.graveyard_cards(p).collect();
                if gy.is_empty() {
                    return Flow::Next;
                }
                // A card at random (a public chance key: the graveyard is
                // public, the pick is chance).
                let k = self.rng.below(gy.len() as u32) as usize;
                self.emit_public(crate::hidden::KEY_DIE, p, k as u64 + 1);
                let c = gy[k];
                self.note(|g| format!("P{p} exiles {} at random", g.name(c)));
                let card = self.move_card(c, Zone::Exile, Pos::Top);
                self.permissions.push(crate::game::Permission {
                    card,
                    player: p,
                    turn: Some(self.turn.number),
                    kind: crate::game::PermKind::Play { free: false, cast_only: false, any_mana: false, attacked_subtype: None },
                });
                Flow::Next
            }
            Ext::NameExileRevealUntil { n } => self.name_exile_reveal_until(f, *n),
            Ext::Balance => self.balance(f),
            Ext::MillAttackPump { n, kws } => {
                let p = f.controller;
                for _ in 0..*n {
                    let Some(&top) = self.players[p as usize].library.last() else { break };
                    self.move_card(top, Zone::Graveyard, Pos::Top);
                }
                // CR 205.2a: card types among the cards (counted now, locked
                // in, 608.2h).
                let t = self.graveyard_cards(p).fold(0u16, |m, c| m | self.types_of(c));
                let x = t.count_ones() as i32;
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    let r = self.obj_ref(c);
                    self.add_effect(Affected::Obj(r), Mod::MustAttackPlayer, Until::EndOfTurn, p);
                    if !kws.is_empty() {
                        self.add_effect(Affected::Obj(r), Mod::Keywords(*kws), Until::EndOfTurn, p);
                    }
                    if x > 0 {
                        self.add_effect(Affected::Obj(r), Mod::Pt(x, x), Until::EndOfTurn, p);
                    }
                }
                Flow::Next
            }
            Ext::SacrificeTutorPayDifference => self.transmute(f),
            Ext::LookTopRepeatPayLife { n, life } => self.look_repeat_pay_life(f, *n, *life),
            Ext::LootMayCastFree { counter, n } => self.loot_may_cast_free(f, *counter, *n),
            Ext::ExileFromHandTaxed { whose, filter, tax } => {
                let Some(owner) = self.who(f, *whose) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{} exiles {} from P{owner}'s hand", f.controller, g.name(c)));
                        let card = self.move_card(c, Zone::Exile, Pos::Top);
                        self.permissions.push(crate::game::Permission {
                            card,
                            player: owner,
                            turn: None,
                            kind: crate::game::PermKind::Taxed(*tax),
                        });
                        Flow::Next
                    }
                    Some(_) => Flow::Next,
                    None => {
                        let mut opts = self.seen_hand_options(f, owner, filter);
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        self.ask(Task::Run(f.clone()), f.controller, DecisionKind::ChooseDiscard, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::ExileHandCardOrTargetUntil { whose, filter, tgt } => {
                let Some(owner) = self.who(f, *whose) else { return Flow::Next };
                let Some(&src) = self.objs_on(f, &On::Source).first() else { return Flow::Next };
                let _ = src;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        let back = self.objs[c.i()].zone;
                        self.note(|g| format!("P{} exiles {}", f.controller, g.name(c)));
                        let card = self.move_card(c, Zone::Exile, Pos::Top);
                        self.links.push(crate::game::Link { source: f.source, card, until: true, back, until_monarch: None });
                        Flow::Next
                    }
                    Some(_) => Flow::Next,
                    None => {
                        let p = f.controller;
                        let mut opts = self.seen_hand_options(f, owner, filter);
                        for c in self.objs_on(f, &On::Target(*tgt)) {
                            opts.push(Opt { action: self.perm_action(p, c), choice: Choice::Card(c) });
                        }
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseDiscard, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::ExileUntilNonlandCounterOrCast => self.exile_until_nonland_counter_or_cast(f),
            other => unreachable!("not a tail instruction: {other:?}"),
        }
    }

    /// `Ext::RemoveAllCountersAddMana`: `f.i` = 0 before the counters are
    /// removed, then 1 + the mana added so far; `f.x` = the counters
    /// removed; `f.floor` = the last colour chosen.
    fn remove_all_add_mana(&mut self, f: &mut Frame, counter: Counter) -> Flow {
        let p = f.controller;
        match self.answer.take() {
            Some(Choice::Number(k)) => {
                let mut v = [0u8; 6];
                v[k as usize] = 1;
                self.add_mana(p, &v);
                f.floor = k as DefId;
                f.i += 1;
            }
            Some(other) => unreachable!("colour answer {other:?}"),
            None if f.i == 0 => {
                let n = match self.objs_on(f, &On::Source).first() {
                    Some(&c) => {
                        let n = self.counters_on(c, counter);
                        self.remove_counters(c, counter, n);
                        n
                    }
                    None => 0,
                };
                self.note(|_| format!("P{p} removes {n} counter(s) for mana"));
                f.x = n as i32;
                f.i = 1;
                f.floor = 0;
            }
            None => {}
        }
        if (f.i as i32 - 1) >= f.x {
            return Flow::Next;
        }
        let opts = (f.floor..5).map(|k| Opt { action: act::NUMBER + k, choice: Choice::Number(k as u8) }).collect();
        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseColor, opts);
        Flow::Suspend
    }

    fn let_draw_else_mill(&mut self, f: &mut Frame, tgt: u8, counter: Counter) -> Flow {
        let p = f.controller;
        let Some(Target::Player(q)) = self.targets_of(f, tgt).into_iter().next() else { return Flow::Next };
        match self.answer.take() {
            Some(Choice::Yes) => {
                self.note(|_| format!("P{q} lets P{p} draw"));
                self.draw(p);
            }
            Some(_) => {
                // X: the counters on the source, as it last existed on the
                // battlefield if it left (CR 608.2h).
                let x = self.amount(f, crate::program::Amount::SourceCounters(counter)).max(0);
                let mut mv = 0i32;
                for _ in 0..x {
                    let Some(&top) = self.players[p as usize].library.last() else { break };
                    mv += self.def_of(top).mana_value() as i32;
                    self.move_card(top, Zone::Graveyard, Pos::Top);
                }
                self.note(|_| format!("P{p} mills {x}; P{q} loses {mv}"));
                if mv > 0 {
                    self.lose_life(q, mv);
                }
            }
            None => {
                self.ask(Task::Run(f.clone()), q, DecisionKind::YesNo, yes_no());
                return Flow::Suspend;
            }
        }
        Flow::Next
    }

    fn name_exile_reveal_until(&mut self, f: &mut Frame, n: u8) -> Flow {
        let p = f.controller;
        let named: Option<DefId> = match self.answer.take() {
            Some(Choice::Def(d)) => Some(d),
            Some(Choice::Pass) => None,
            Some(other) => unreachable!("name answer {other:?}"),
            None => {
                // A name among the namer's library cards, or `PASS`: a name
                // of no card there (ENGINE.md 24.5).
                let lib = self.library_sorted(p);
                let mut opts = Vec::new();
                for (i, &c) in lib.iter().enumerate() {
                    let d = self.objs[c.i()].def;
                    if i > 0 && self.objs[lib[i - 1].i()].def == d {
                        continue;
                    }
                    assert!((i as u16) < act::LIBRARY_N, "library slot {i} out of range");
                    opts.push(Opt { action: act::LIBRARY + i as u16, choice: Choice::Def(d) });
                }
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::Run(f.clone()), p, DecisionKind::NameCard, opts);
                return Flow::Suspend;
            }
        };
        let name = named.map(|d| self.db.def(d).name.clone());
        self.note(|_| format!("P{p} names {}", name.as_deref().unwrap_or("a card not in their library")));
        // Exile the top n (face up: each revealed as it moves).
        for _ in 0..n {
            let Some(&top) = self.players[p as usize].library.last() else { break };
            self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
            self.move_card(top, Zone::Exile, Pos::Top);
        }
        // Then reveal until a card with the name (CR 701.20a): it goes into
        // the hand, the others revealed are exiled.
        loop {
            let Some(&top) = self.players[p as usize].library.last() else { break };
            self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
            self.reveal(top);
            if name.as_deref() == Some(self.name(top)) {
                self.move_card(top, Zone::Hand, Pos::Top);
                break;
            }
            self.move_card(top, Zone::Exile, Pos::Top);
        }
        Flow::Next
    }

    /// Balance. `f.i`: 2 * stage (+1 once the stage's minimum is set in
    /// `f.x`); stages: lands, cards in hand, creatures. `f.picked`: the
    /// permanents chosen to be sacrificed in the current stage.
    fn balance(&mut self, f: &mut Frame) -> Flow {
        let by = f.controller;
        loop {
            let stage = f.i / 2;
            if stage >= 3 {
                return Flow::Next;
            }
            let filter = match stage {
                0 => Some(Filter::Type(types::LAND)),
                2 => Some(Filter::Type(types::CREATURE)),
                _ => None,
            };
            let count = |g: &Game, p: PlayerId| -> usize {
                match &filter {
                    Some(fl) => g.count_controlled(p, fl) as usize,
                    None => g.players[p as usize].hand.len(),
                }
            };
            if let Some(ch) = self.answer.take() {
                let Choice::Card(c) = ch else { unreachable!("balance answer {ch:?}") };
                if filter.is_some() {
                    f.picked.push(c);
                } else {
                    // A card discarded at once (never hidden while another
                    // decision waits).
                    self.discard(c);
                }
            }
            if f.i % 2 == 0 {
                f.x = (0..2u8).map(|p| count(self, p)).min().unwrap() as i32;
                f.i += 1;
                f.picked.clear();
            }
            // APNAP (CR 101.4): the active player chooses first. A player
            // shielded from the controller's effects ("can't cause you to
            // discard cards or sacrifice permanents") keeps everything.
            let ap = self.turn.active;
            let mut asker = None;
            for p in [ap, 1 - ap] {
                if self.shielded_from(p, by) {
                    continue;
                }
                let mine = f.picked.iter().filter(|&&c| self.objs[c.i()].controller == p).count();
                if count(self, p) - mine > f.x as usize {
                    asker = Some(p);
                    break;
                }
            }
            match asker {
                Some(p) => {
                    let opts: Vec<Opt> = match &filter {
                        Some(fl) => {
                            // A set: increasing slot order, leaving enough.
                            let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                            let floor = f
                                .picked
                                .iter()
                                .filter(|&&c| self.objs[c.i()].controller == p)
                                .map(|&c| self.perm_action(p, c))
                                .max();
                            let cands: Vec<CardIdx> = self
                                .permanents_of(p)
                                .filter(|&c| self.card_matches(fl, c, &ctx) && !f.picked.contains(&c))
                                .collect();
                            let mine = f.picked.iter().filter(|&&c| self.objs[c.i()].controller == p).count();
                            let still = count(self, p) - mine - f.x as usize;
                            let mut acts: Vec<(u16, CardIdx)> =
                                cands.iter().map(|&c| (self.perm_action(p, c), c)).filter(|&(a, _)| floor.is_none_or(|fl| a > fl)).collect();
                            acts.sort();
                            let len = acts.len();
                            acts.into_iter()
                                .enumerate()
                                .filter(|&(k, _)| len - k >= still)
                                .map(|(_, (a, c))| Opt { action: a, choice: Choice::Card(c) })
                                .collect()
                        }
                        None => self.hand_options(p),
                    };
                    let kind = if filter.is_some() { DecisionKind::Sacrifice } else { DecisionKind::Discard };
                    self.ask(Task::Run(f.clone()), p, kind, opts);
                    return Flow::Suspend;
                }
                None => {
                    // Everyone has chosen: the permanents are sacrificed at
                    // once.
                    let chosen = std::mem::take(&mut f.picked);
                    if !chosen.is_empty() {
                        self.begin_batch();
                        for c in chosen {
                            if self.objs[c.i()].zone == Zone::Battlefield {
                                self.sacrifice(c);
                            }
                        }
                        self.end_batch();
                    }
                    f.i += 1;
                }
            }
        }
    }

    /// Transmute Artifact. `f.i`: 0 = sacrifice, 1 = search, 2 = pay or
    /// not; `f.x` = the sacrificed artifact's mana value; `f.picked[0]` =
    /// the card found.
    fn transmute(&mut self, f: &mut Frame) -> Flow {
        let p = f.controller;
        let artifact = Filter::Type(types::ARTIFACT);
        if f.i == 0 {
            match self.answer.take() {
                Some(Choice::Card(c)) => {
                    // (As it last existed: CR 608.2h.)
                    f.x = self.mana_value_of(c) as i32;
                    self.sacrifice(c);
                    f.i = 1;
                }
                Some(other) => unreachable!("sacrifice answer {other:?}"),
                None => {
                    let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                    let opts: Vec<Opt> = self
                        .permanents_of(p)
                        .filter(|&c| self.card_matches(&artifact, c, &ctx))
                        .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                        .collect();
                    if opts.is_empty() {
                        return Flow::Next; // nothing sacrificed: "if you do" fails
                    }
                    self.ask(Task::Run(f.clone()), p, DecisionKind::Sacrifice, opts);
                    return Flow::Suspend;
                }
            }
        }
        if f.i == 1 {
            match self.answer.take() {
                Some(Choice::Def(d)) => {
                    let c = self.library_sorted(p).into_iter().find(|&c| self.objs[c.i()].def == d).expect("found card");
                    self.scramble(p, &[], false, true);
                    self.note(|g| format!("P{p} finds {}", g.name(c)));
                    if (self.def_of(c).mana_value() as i32) <= f.x {
                        self.put_onto_battlefield(c, p);
                        self.shuffle_library(p);
                        return Flow::Next;
                    }
                    // While the payment is decided the card waits revealed
                    // on top of the shuffled library (the shuffle comes
                    // early: nothing in between touches the library), so no
                    // determinize resamples it and its place says nothing
                    // (ENGINE.md 28).
                    self.shuffle_library(p);
                    self.move_card(c, Zone::Library, Pos::Top);
                    self.reveal(c);
                    f.picked = vec![c];
                    f.i = 2;
                }
                Some(Choice::Pass) => {
                    self.shuffle_library(p);
                    return Flow::Next;
                }
                Some(other) => unreachable!("search answer {other:?}"),
                None => {
                    let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                    let opts = self.search_options(p, &artifact, true, &ctx);
                    self.ask(Task::Run(f.clone()), p, DecisionKind::SearchLibrary, opts);
                    return Flow::Suspend;
                }
            }
        }
        // f.i == 2: "you may pay {X}, where X is the difference."
        let c = f.picked[0];
        let diff = (self.def_of(c).mana_value() as i32 - f.x).clamp(0, 99) as u8;
        let cost = crate::mana::ManaCost { generic: diff, ..Default::default() };
        match self.answer.take() {
            Some(Choice::Yes) => {
                self.note(|_| format!("P{p} pays {cost}"));
                let mut back = f.clone();
                back.i = 3;
                self.agenda.push(Task::Run(back));
                self.agenda.push(Task::PayMana(crate::pay::PayMana::new(p, cost)));
                Flow::Suspend
            }
            Some(_) => {
                self.move_card(c, Zone::Graveyard, Pos::Top);
                Flow::Next
            }
            None if f.i == 3 => {
                if self.objs[c.i()].zone == Zone::Library {
                    self.put_onto_battlefield(c, p);
                }
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

    /// Lim-Dul's Vault: `f.i` = 0 before the first look.
    fn look_repeat_pay_life(&mut self, f: &mut Frame, n: u8, life: u8) -> Flow {
        let p = f.controller;
        match self.answer.take() {
            Some(Choice::Yes) => {
                self.lose_life(p, life as i32);
                // On the bottom (in the pile's order: an order the player
                // could choose; the library is shuffled at the end, ENGINE.md
                // 28), then the next n.
                for c in std::mem::take(&mut f.pile) {
                    self.move_card(c, Zone::Library, Pos::Bottom);
                }
                self.look_top(f, p, n, false);
            }
            Some(_) => {
                // "Then shuffle and put the last cards you looked at on
                // top": the rest of the library in a random order nobody
                // knows; the pile stays on top for the next op to order.
                let rest: Vec<CardIdx> =
                    self.players[p as usize].library.iter().copied().filter(|c| !f.pile.contains(c)).collect();
                self.put_on_bottom_randomly(p, rest);
                return Flow::Next;
            }
            None if f.i == 0 => {
                self.look_top(f, p, n, false);
                f.i = 1;
            }
            None => {}
        }
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        // CR 119.4: life can be paid only up to the life total.
        if self.players[p as usize].life >= life as i32 {
            opts.push(Opt { action: act::YES, choice: Choice::Yes });
        }
        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
        Flow::Suspend
    }

    /// Malcolm: `f.i` = 0 before the draw, 1 = discard asked, 2 = the cast
    /// offered (`f.picked[0]` the discarded card).
    fn loot_may_cast_free(&mut self, f: &mut Frame, counter: Counter, n: u8) -> Flow {
        let p = f.controller;
        if f.i == 0 {
            self.draw(p);
            f.i = 1;
        }
        if f.i == 1 {
            match self.answer.take() {
                Some(Choice::Card(c)) => {
                    let r = self.discard_to(c);
                    f.i = 2;
                    let enough = self.amount(f, crate::program::Amount::SourceCounters(counter)) >= n as i32;
                    match r {
                        Some(card) if enough => f.picked = vec![card],
                        _ => return Flow::Next,
                    }
                }
                Some(other) => unreachable!("discard answer {other:?}"),
                None => {
                    if self.players[p as usize].hand.is_empty() {
                        return Flow::Next;
                    }
                    let opts = self.hand_options(p);
                    self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                    return Flow::Suspend;
                }
            }
        }
        let card = f.picked[0];
        match self.answer.take() {
            Some(Choice::Yes) => {
                // The rest of the effect waits for the cast (CR 608.2g).
                let mut back = f.clone();
                back.pc += 1;
                back.i = 0;
                back.floor = 0;
                back.picked.clear();
                self.agenda.push(Task::Run(back));
                self.cast_free_now(p, card);
                Flow::Suspend
            }
            Some(_) => Flow::Next,
            None => {
                if self.objs[card.i()].zone != Zone::Graveyard || !self.can_cast_free_now(p, card) {
                    return Flow::Next;
                }
                self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, yes_no());
                Flow::Suspend
            }
        }
    }

    /// The options of a choice of a card matching `filter` in `owner`'s hand,
    /// which the controller has seen: a `LIBRARY` slot of the hand sorted by
    /// definition, one per definition.
    fn seen_hand_options(&self, f: &Frame, owner: PlayerId, filter: &Filter) -> Vec<Opt> {
        let p = f.controller;
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
        opts
    }

    /// Black Widow: `f.picked[0]` = the nonland card exiled (none: empty).
    fn exile_until_nonland_counter_or_cast(&mut self, f: &mut Frame) -> Flow {
        let p = f.controller;
        match self.answer.take() {
            Some(Choice::Yes) => {
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    self.add_counters(c, Counter::P1P1, 1);
                }
                return Flow::Next;
            }
            Some(_) => {
                if let Some(&c) = f.picked.first() {
                    if self.objs[c.i()].zone == Zone::Exile {
                        let card = self.obj_ref(c);
                        self.permissions.push(crate::game::Permission {
                            card,
                            player: p,
                            turn: Some(self.turn.number),
                            kind: crate::game::PermKind::Play { free: false, cast_only: true, any_mana: true, attacked_subtype: None },
                        });
                    }
                }
                return Flow::Next;
            }
            None => {}
        }
        if f.i == 0 {
            f.i = 1;
            f.picked.clear();
            if let Some(q) = f.ev.player {
                // Until a nonland card (CR 701.20a), each revealed as it is
                // exiled.
                while let Some(&top) = self.players[q as usize].library.last() {
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, q);
                    self.move_card(top, Zone::Exile, Pos::Top);
                    if !self.def_of(top).is(types::LAND) {
                        f.picked.push(top);
                        break;
                    }
                }
            }
        }
        // "You may put a +1/+1 counter on it. If you don't, you may cast the
        // card": yes = the counter (only with the source still there), no =
        // the permission.
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        if !self.objs_on(f, &On::Source).is_empty() {
            opts.push(Opt { action: act::YES, choice: Choice::Yes });
        }
        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
        Flow::Suspend
    }

    /// Discard `c`; the card it became in the graveyard (none if it went
    /// elsewhere: madness).
    fn discard_to(&mut self, c: CardIdx) -> Option<CardIdx> {
        self.discard(c);
        (self.objs[c.i()].zone == Zone::Graveyard).then_some(c)
    }
}
