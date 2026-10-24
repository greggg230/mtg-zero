//! The M7b `Ext` instructions (ENGINE.md 24): emblems, loyalty, control
//! changes, die rolls, casting during resolution, searches for any number
//! of cards, naming a card, ninjutsu.

use crate::db::types;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{opp, CardIdx, DefId, PlayerId, Target, Zone};
use crate::program::{Ext, On};
use crate::resolve::{Flow, Frame};

impl Game {
    pub(crate) fn exec_walker(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::CreateEmblem { emblem } => {
                let def = self.db.token_def(*emblem);
                self.create_emblem(def, f.controller);
                Flow::Next
            }
            Ext::RemoveCounters { on, counter, n } => {
                let n = self.amount(f, *n).clamp(0, u16::MAX as i32) as u16;
                for c in self.objs_on(f, on) {
                    self.remove_counters(c, *counter, n);
                }
                Flow::Next
            }
            Ext::GainControl { tgt } => {
                for c in self.objs_on(f, &On::Target(*tgt)) {
                    self.change_control(c, f.controller);
                }
                Flow::Next
            }
            Ext::ExchangeControl { a, b } => {
                // CR 701.12a-b: both or nothing; nothing if one player
                // controls both.
                let x = self.objs_on(f, &On::Target(*a)).first().copied();
                let y = self.objs_on(f, &On::Target(*b)).first().copied();
                if let (Some(x), Some(y)) = (x, y) {
                    let (cx, cy) = (self.objs[x.i()].controller, self.objs[y.i()].controller);
                    if cx != cy && x != y {
                        self.change_control(x, cy);
                        self.change_control(y, cx);
                    }
                }
                Flow::Next
            }
            Ext::GainControlOfEventSpellTargets { filter } => {
                // The permanents the spell targeted as this triggered
                // (`StackItem::noted`), whether or not that spell is still
                // on the stack.
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let won: Vec<CardIdx> = f
                    .noted
                    .iter()
                    .filter(|&&r| {
                        self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield && self.card_matches(filter, r.idx, &ctx)
                    })
                    .map(|r| r.idx)
                    .collect();
                for c in won {
                    self.change_control(c, f.controller);
                }
                Flow::Next
            }
            Ext::RollDie { sides, table } => {
                // CR 706: a public chance key (`KEY_DIE`, payload = the
                // result).
                let p = f.controller;
                let n = match self.forced_die(p, *sides) {
                    Some(n) => n,
                    None => 1 + self.rng.below(*sides as u32) as u8,
                };
                self.emit_public(crate::hidden::KEY_DIE, p, n as u64);
                self.note(|_| format!("P{p} rolls {n}"));
                match table.iter().find(|&&(lo, hi, _)| (lo..=hi).contains(&n)) {
                    Some(&(_, _, pc)) => Flow::Jump(pc),
                    None => Flow::Next,
                }
            }
            Ext::ExtraLoyaltyActivations { n } => {
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    let turn = self.turn.number as u16;
                    let o = &mut self.objs[c.i()];
                    if o.pw_turn != turn {
                        o.pw_turn = turn;
                        o.pw_used = 0;
                        o.pw_extra = 0;
                    }
                    o.pw_extra = o.pw_extra.saturating_add(*n);
                }
                Flow::Next
            }
            Ext::ChooseFromGraveyardToHand { filter } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        if self.objs[c.i()].zone == Zone::Graveyard {
                            self.move_card(c, Zone::Hand, Pos::Top);
                        }
                        Flow::Next
                    }
                    Some(other) => unreachable!("graveyard choice {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                        let opts: Vec<Opt> = self
                            .graveyard_cards(p)
                            .filter(|&c| self.card_matches(filter, c, &ctx))
                            .map(|c| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) })
                            .collect();
                        if opts.is_empty() {
                            return Flow::Next;
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseGraveyardCard, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::DamageChosen { amount } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Target(t)) => {
                        let n = self.amount(f, *amount);
                        let src = self.frame_damage_source(f);
                        self.deal_damage(src, t, n);
                        Flow::Next
                    }
                    Some(other) => unreachable!("chosen-damage answer {other:?}"),
                    None => {
                        // "A creature or player", chosen as it resolves (CR
                        // 115.10: not a target).
                        let mut opts = vec![
                            Opt { action: act::PLAYER, choice: Choice::Target(Target::Player(p)) },
                            Opt { action: act::PLAYER + 1, choice: Choice::Target(Target::Player(opp(p))) },
                        ];
                        for c in self.battlefield.clone() {
                            if self.is_creature(c) {
                                let r = self.obj_ref(c);
                                opts.push(Opt { action: self.perm_action(p, c), choice: Choice::Target(Target::Obj(r)) });
                            }
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::Target, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::ExileTopMayCast { skip } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        let card = f.picked[0];
                        // The rest of the effect waits for the cast (CR
                        // 608.2g): the cast's tasks run first.
                        let mut back = f.clone();
                        back.pc = *skip;
                        back.i = 0;
                        back.floor = 0;
                        back.picked.clear();
                        self.agenda.push(Task::Run(back));
                        self.cast_now(p, card);
                        return Flow::Suspend;
                    }
                    Some(_) => {
                        f.picked.clear();
                        return Flow::Next;
                    }
                    None => {}
                }
                if f.picked.is_empty() {
                    let Some(&top) = self.players[p as usize].library.last() else { return Flow::Next };
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                    self.reveal(top);
                    self.move_card(top, Zone::Exile, Pos::Top);
                    f.picked = vec![top];
                }
                let card = f.picked[0];
                if self.objs[card.i()].zone != Zone::Exile || !self.can_cast_now(p, card) {
                    f.picked.clear();
                    return Flow::Next;
                }
                let opts = vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                Flow::Suspend
            }
            Ext::ExileLibrary { who } => {
                if let Some(q) = self.who(f, *who) {
                    // Top first; each card is revealed as it moves.
                    let lib: Vec<CardIdx> = self.players[q as usize].library.iter().rev().copied().collect();
                    for c in lib {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                }
                Flow::Next
            }
            Ext::LookAtTopMayBottom { who } => {
                let Some(q) = self.who(f, *who) else { return Flow::Next };
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        if let Some(&top) = self.players[q as usize].library.last() {
                            self.note(|_| format!("P{p} puts the top card of P{q}'s library on the bottom"));
                            self.move_card(top, Zone::Library, Pos::Bottom);
                        }
                        return Flow::Next;
                    }
                    Some(_) => return Flow::Next,
                    None => {}
                }
                let Some(&top) = self.players[q as usize].library.last() else { return Flow::Next };
                if f.i == 0 {
                    // The look (keyed once; remembered only where its owner
                    // knows it too, ENGINE.md 23.7).
                    self.force_library_card(top, crate::hidden::KEY_LOOK, p);
                    if p == q || self.knows(q, top) {
                        self.learn(p, top);
                    }
                    let d = self.objs[top.i()].def;
                    self.emit_private(crate::hidden::KEY_LOOK, p, d as u64);
                    f.i = 1;
                }
                let opts = vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                Flow::Suspend
            }
            Ext::NinjutsuEnter => {
                let src = f.source;
                if !self.live(src) || self.objs[src.idx.i()].zone != Zone::Hand {
                    return Flow::Next;
                }
                let c = src.idx;
                let p = f.controller;
                self.put_onto_battlefield(c, p);
                if self.objs[c.i()].zone == Zone::Battlefield {
                    self.tap(c);
                    // CR 702.49c, 508.4a, 506.3a: attacking what the returned
                    // creature attacked, if that is still there; a
                    // noncreature permanent is never attacking.
                    let in_combat = self.turn.active == p
                        && matches!(
                            self.turn.step,
                            crate::game::Step::DeclareBlockers
                                | crate::game::Step::FirstStrikeDamage
                                | crate::game::Step::CombatDamage
                                | crate::game::Step::EndCombat
                        );
                    let dest_ok = match f.attack_as {
                        Some(None) => true,
                        Some(Some(r)) => {
                            self.live(r)
                                && self.objs[r.idx.i()].zone == Zone::Battlefield
                                && self.objs[r.idx.i()].controller == opp(p)
                                && self.is_type(r.idx, types::PLANESWALKER)
                        }
                        None => false,
                    };
                    if in_combat && dest_ok && self.is_creature(c) {
                        self.objs[c.i()].attacking = true;
                        self.objs[c.i()].attacks = f.attack_as.flatten();
                        self.note(|g| format!("{} enters attacking", g.name(c)));
                    }
                }
                Flow::Next
            }
            Ext::RevealTopOpponentChooses { n, counter } => {
                let p = f.controller;
                let q = opp(p);
                let chosen = match self.answer.take() {
                    Some(Choice::Card(c)) => Some(c),
                    Some(other) => unreachable!("revealed-card answer {other:?}"),
                    None => {
                        if f.i == 0 {
                            self.look_top(f, p, *n, true);
                            f.i = 1;
                        }
                        match f.pile.len() {
                            0 => return Flow::Next,
                            1 => Some(f.pile[0]),
                            _ => {
                                let mut sorted = f.pile.clone();
                                sorted.sort_by_key(|c| (self.objs[c.i()].def, c.0));
                                let mut opts = Vec::new();
                                for (k, &c) in sorted.iter().enumerate() {
                                    if k > 0 && self.objs[sorted[k - 1].i()].def == self.objs[c.i()].def {
                                        continue;
                                    }
                                    opts.push(Opt { action: act::LIBRARY + k as u16, choice: Choice::Card(c) });
                                }
                                self.ask(Task::Run(f.clone()), q, DecisionKind::PutInHand, opts);
                                return Flow::Suspend;
                            }
                        }
                    }
                };
                if let Some(c) = chosen {
                    f.pile.retain(|&x| x != c);
                    self.move_card(c, Zone::Hand, Pos::Top);
                }
                for x in std::mem::take(&mut f.pile) {
                    self.move_card(x, Zone::Exile, Pos::Top);
                    self.add_counters(x, *counter, 1);
                }
                Flow::Next
            }
            Ext::ExiledWithCounterToHand { counter } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        if self.objs[c.i()].zone == Zone::Exile {
                            self.move_card(c, Zone::Hand, Pos::Top);
                        }
                        Flow::Next
                    }
                    Some(other) => unreachable!("exiled-card answer {other:?}"),
                    None => {
                        let cands: Vec<CardIdx> = self
                            .exile
                            .iter()
                            .copied()
                            .filter(|&c| self.objs[c.i()].owner == p && self.counters_on(c, *counter) > 0)
                            .take(act::EXILE_N as usize)
                            .collect();
                        if cands.is_empty() {
                            return Flow::Next;
                        }
                        let opts = cands
                            .iter()
                            .enumerate()
                            .map(|(k, &c)| Opt { action: act::EXILE + k as u16, choice: Choice::Card(c) })
                            .collect();
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseExiled, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::SacrificeThenReflexive { filter, k } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        // Its power as it last existed (CR 608.2h).
                        let power = self.power(c).max(0);
                        let r = self.obj_ref(c);
                        self.note(|g| format!("P{p} sacrifices {}", g.name(c)));
                        self.sacrifice(c);
                        let base = self.db.def(f.source_def).delayed_base;
                        self.waiting.push(crate::triggers::PendingTrigger {
                            controller: p,
                            source: f.source,
                            source_def: f.source_def,
                            slot: base + k,
                            ev: crate::triggers::EvCtx {
                                obj: Some(r),
                                amount: power.min(u16::MAX as i32) as u16,
                                ..Default::default()
                            },
                        });
                        Flow::Next
                    }
                    Some(other) => unreachable!("sacrifice answer {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                        let opts: Vec<Opt> = self
                            .permanents_of(p)
                            .filter(|&c| self.card_matches(filter, c, &ctx))
                            .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                            .collect();
                        if opts.is_empty() {
                            return Flow::Next;
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::Sacrifice, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::JumpUnlessEventObj { filter, to } => {
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                if f.ev.obj.is_some_and(|r| self.card_matches(filter, r.idx, &ctx)) {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            Ext::SearchAnyNumber { filter, tapped, exile_free } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Def(d)) => {
                        let c = self
                            .library_sorted(p)
                            .into_iter()
                            .find(|&c| self.objs[c.i()].def == d && !f.picked.contains(&c))
                            .expect("a card of the chosen definition");
                        f.picked.push(c);
                        f.floor = d;
                    }
                    Some(Choice::Pass) => {
                        self.finish_search_any(f, p, *tapped, *exile_free);
                        return Flow::Next;
                    }
                    Some(other) => unreachable!("search answer {other:?}"),
                    None => {}
                }
                // One definition per decision, in non-decreasing order (each
                // set of cards once); a definition again while a copy is left.
                let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                let lib = self.library_sorted(p);
                let mut opts = Vec::new();
                for (i, &c) in lib.iter().enumerate() {
                    let d = self.objs[c.i()].def;
                    if i > 0 && self.objs[lib[i - 1].i()].def == d {
                        continue;
                    }
                    let left = lib.iter().any(|&x| self.objs[x.i()].def == d && !f.picked.contains(&x));
                    if d >= f.floor && left && self.card_matches(filter, c, &ctx) {
                        assert!((i as u16) < act::LIBRARY_N, "library slot {i} out of range");
                        opts.push(Opt { action: act::LIBRARY + i as u16, choice: Choice::Def(d) });
                    }
                }
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::Run(f.clone()), p, DecisionKind::SearchLibrary, opts);
                Flow::Suspend
            }
            Ext::NameRevealTopToHand { n, nonland } => {
                let p = f.controller;
                let named: Option<DefId> = match self.answer.take() {
                    Some(Choice::Def(d)) => Some(d),
                    Some(Choice::Pass) => None,
                    Some(other) => unreachable!("name answer {other:?}"),
                    None => {
                        // A name among the namer's library cards (open
                        // decklists: the library's contents are known to its
                        // owner, never its order), or `PASS`: a name of no
                        // card there (ENGINE.md 24).
                        let lib = self.library_sorted(p);
                        let mut opts = Vec::new();
                        for (i, &c) in lib.iter().enumerate() {
                            let d = self.objs[c.i()].def;
                            if i > 0 && self.objs[lib[i - 1].i()].def == d {
                                continue;
                            }
                            if *nonland && self.db.def(d).is(types::LAND) {
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
                self.look_top(f, p, *n, true);
                for c in std::mem::take(&mut f.pile) {
                    if name.as_deref() == Some(self.name(c)) {
                        self.move_card(c, Zone::Hand, Pos::Top);
                    } else {
                        self.move_card(c, Zone::Graveyard, Pos::Top);
                    }
                }
                Flow::Next
            }
            Ext::FlashSorceries => {
                self.players[f.controller as usize].flash_sorceries = true;
                Flow::Next
            }
            other => unreachable!("not a walker instruction: {other:?}"),
        }
    }

    /// The end of `Ext::SearchAnyNumber`: the found cards go where the
    /// effect says, then the library is shuffled (CR 701.19, 701.20).
    pub(crate) fn finish_search_any(&mut self, f: &mut Frame, p: PlayerId, tapped: bool, exile_free: bool) {
        let found = std::mem::take(&mut f.picked);
        // Which cards were taken is private until they are revealed: the
        // other player loses track of the library (ENGINE.md 10).
        self.scramble(p, &[], false, true);
        for c in found {
            if self.objs[c.i()].zone != Zone::Library {
                continue;
            }
            if exile_free {
                let card = self.move_card(c, Zone::Exile, Pos::Top);
                self.permissions.push(crate::game::Permission {
                    card,
                    player: p,
                    turn: Some(self.turn.number),
                    kind: crate::game::PermKind::Play { free: true, cast_only: true, any_mana: false, attacked_subtype: None },
                });
            } else {
                self.put_onto_battlefield(c, p);
                if tapped {
                    self.tap(c);
                }
            }
        }
        self.shuffle_library(p);
    }

    /// Could `p` cast `card` (in exile) now, during the resolution of an
    /// effect that lets them (CR 608.2g: timing permissions of its card type
    /// are ignored; "can cast spells only any time they could cast a
    /// sorcery" is not: the stack isn't empty)?
    pub(crate) fn can_cast_now(&self, p: PlayerId, card: CardIdx) -> bool {
        if self.def_of(card).is(types::LAND) {
            return false;
        }
        if self.permanents_of(opp(p)).any(|c| self.abilities(c).opp_sorcery_speed) {
            return false;
        }
        if !self.may_cast_spells(p) {
            return false;
        }
        let classes = self.mana_classes(p, None);
        let from = crate::game::CastFrom::Permission { free: false, any_mana: false };
        self.castable_via(p, card, from, &classes, true)
    }

    /// Cast `card` now without paying its mana cost (CR 608.2g, 118.9).
    pub(crate) fn cast_free_now(&mut self, p: PlayerId, card: CardIdx) {
        let r = self.obj_ref(card);
        self.permissions.push(crate::game::Permission {
            card: r,
            player: p,
            turn: Some(self.turn.number),
            kind: crate::game::PermKind::Play { free: true, cast_only: true, any_mana: false, attacked_subtype: None },
        });
        self.begin_cast(p, card);
    }

    /// Cast `card` from exile now (an effect lets `p` do so as it
    /// resolves): a one-time permission, then the usual casting process.
    pub(crate) fn cast_now(&mut self, p: PlayerId, card: CardIdx) {
        let r = self.obj_ref(card);
        self.permissions.push(crate::game::Permission {
            card: r,
            player: p,
            turn: Some(self.turn.number),
            kind: crate::game::PermKind::Play { free: false, cast_only: true, any_mana: false, attacked_subtype: None },
        });
        self.begin_cast(p, card);
    }

    /// The queued outcome of a die roll by `p` (`force_chance`), if the next
    /// queued key is one with a possible result.
    pub(crate) fn forced_die(&self, p: PlayerId, sides: u8) -> Option<u8> {
        let k = *self.forced.get(self.forced_at)?;
        let n = crate::hidden::key_payload(k);
        (crate::hidden::key_kind(k) == crate::hidden::KEY_DIE && crate::hidden::key_player(k) == p && (1..=sides as u64).contains(&n))
            .then_some(n as u8)
    }

    /// "Spells and abilities your opponents control can't cause you to
    /// discard cards or sacrifice permanents" protects `victim` from an
    /// effect controlled by `by`.
    pub(crate) fn shielded_from(&self, victim: PlayerId, by: PlayerId) -> bool {
        victim != by && self.permanents_of(victim).any(|c| self.abilities(c).no_forced_discard_sac)
    }
}

