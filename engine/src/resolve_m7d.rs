//! M7d: the last cube cards that needed engine subsystems (ENGINE.md 31):
//! special actions (plot, turning a face-down permanent face up), and the
//! runtime parts of the cards' new rules.

use crate::db::{ActivatedDef, Special};
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, PlayerId, Zone};
use crate::mana::ManaCost;

impl Game {
    /// The mana a special action costs `p` (CR 116.2): plot's cost; turning a
    /// face-down permanent face up, the mana cost of its card (701.40b).
    pub(crate) fn special_mana(&self, _p: PlayerId, card: CardIdx, ad: &ActivatedDef) -> ManaCost {
        match ad.special {
            Some(Special::TurnFaceUp) => self.face_down_cost(card).unwrap_or_default(),
            _ => ad.cost.mana,
        }
    }

    /// `Task::Special`: the cost of special action `sp` was paid; it happens
    /// now, without using the stack (CR 116.2).
    pub(crate) fn special_action(&mut self, p: PlayerId, card: CardIdx, sp: Special) {
        match sp {
            Special::Plot => {
                // CR 702.170a: exile it from hand; it becomes a plotted card,
                // which its owner may cast without paying its mana cost
                // during their main phase while the stack is empty on any
                // later turn (702.170d; a sorcery's own timing, the lowering
                // checks).
                if self.objs[card.i()].zone != Zone::Hand {
                    return;
                }
                self.note(|g| format!("P{p} plots {}", g.name(card)));
                let r = self.move_card(card, Zone::Exile, Pos::Top);
                if self.objs[card.i()].zone == Zone::Exile {
                    self.permissions.push(crate::game::Permission {
                        card: r,
                        player: p,
                        turn: Some(crate::resolve_tail2::PERM_LATER | self.turn.number),
                        kind: crate::game::PermKind::Play { free: true, cast_only: true, any_mana: false, only_if: crate::game::PlayIf::Always },
                    });
                }
            }
            Special::TurnFaceUp => self.turn_face_up(card),
        }
    }
}

// ---- face-down permanents (CR 708; ENGINE.md 31) -------------------------

impl Game {
    /// The card a face-down permanent is, if `c` is one (its slot's `def` is
    /// the face-down definition, CR 708.2a).
    #[inline]
    pub(crate) fn face_down_id(&self, c: CardIdx) -> Option<crate::ids::DefId> {
        if self.face_down_ids.is_empty() || self.objs[c.i()].def != self.db.face_down_def() {
            return None;
        }
        self.face_down_ids.iter().find(|x| x.0 == c).map(|x| x.1)
    }

    /// The identity of slot `c`: its definition, or for a face-down
    /// permanent the card it is (CR 708.2; what knowledge and `determinize`
    /// deal with, ENGINE.md 10, 31).
    pub fn ident(&self, c: CardIdx) -> crate::ids::DefId {
        self.face_down_id(c).unwrap_or(self.objs[c.i()].def)
    }

    /// Set slot `c`'s identity (`determinize`): a face-down permanent's card.
    pub(crate) fn set_ident(&mut self, c: CardIdx, d: crate::ids::DefId) {
        if !self.face_down_ids.is_empty() && self.objs[c.i()].def == self.db.face_down_def() {
            if let Some(x) = self.face_down_ids.iter_mut().find(|x| x.0 == c) {
                x.1 = d;
                return;
            }
        }
        self.objs[c.i()].def = d;
    }

    /// Is `c` a face-down permanent?
    pub fn is_face_down(&self, c: CardIdx) -> bool {
        self.face_down_id(c).is_some()
    }

    /// The card a face-down permanent is, if `c` is one.
    pub(crate) fn face_down_def(&self, c: CardIdx) -> Option<&'static crate::db::CardDef> {
        self.face_down_id(c).map(|d| self.db.def(d))
    }

    /// The mana cost to turn a face-down permanent face up (CR 701.40b: a
    /// manifested creature card, for its mana cost; X is 0), if it can be.
    pub(crate) fn face_down_cost(&self, c: CardIdx) -> Option<ManaCost> {
        let d = self.face_down_def(c)?;
        d.is(crate::db::types::CREATURE).then_some(d.cost)
    }

    /// A face-down permanent's identity becomes public (CR 708.8 turned
    /// face up, 708.9 leaving the battlefield): every player who didn't know
    /// it sees it (clouds lose it, as for a card leaving a hidden zone), a
    /// public reveal key; it is its card again.
    fn reveal_face_down(&mut self, c: CardIdx, known: u8) -> Option<crate::ids::DefId> {
        let i = self.face_down_ids.iter().position(|x| x.0 == c)?;
        let real = self.face_down_ids[i].1;
        let owner = self.objs[c.i()].owner;
        self.knowledge_leaves(c, owner, Zone::Battlefield, real, known);
        self.face_down_ids.remove(i);
        self.emit_public(crate::hidden::KEY_REVEAL, owner, real as u64);
        self.objs[c.i()].known = 0b11;
        Some(real)
    }

    /// Turn a face-down permanent face up (CR 708.8: the same permanent, its
    /// copiable values now its card's; no enter abilities).
    pub(crate) fn turn_face_up(&mut self, c: CardIdx) {
        if self.objs[c.i()].zone != Zone::Battlefield {
            return;
        }
        let known = self.objs[c.i()].known;
        let Some(real) = self.reveal_face_down(c, known) else { return };
        self.set_face(c, real);
        self.note(|g| format!("{} is turned face up", g.name(c)));
    }

    /// A face-down permanent that just left the battlefield (it left as a
    /// face-down 2/2, CR 603.10a) is revealed and is its card again (708.9).
    /// `known`: its knowledge bits as it left.
    pub(crate) fn unface(&mut self, c: CardIdx, known: u8) {
        if let Some(real) = self.reveal_face_down(c, known) {
            self.objs[c.i()].def = real;
        }
    }

    /// CR 708.5: `p`, who now controls face-down permanent `c`, may look at
    /// it.
    pub(crate) fn look_face_down(&mut self, c: CardIdx, p: PlayerId) {
        if self.objs[c.i()].known & (1 << p) == 0 {
            self.learn(p, c);
            let d = self.ident(c);
            self.emit_private(crate::hidden::KEY_LOOK, p, d as u64);
        }
    }

    /// Manifest dread (CR 701.62a): look at the top two cards of your
    /// library, put one onto the battlefield face down as a 2/2 creature
    /// (manifest, 701.40a) and the other into your graveyard. Choice:
    /// `PutOntoBattlefield` (the pile's cards by definition).
    pub(crate) fn manifest_dread(&mut self, f: &mut crate::resolve::Frame) -> crate::resolve::Flow {
        use crate::decision::{act, Choice, DecisionKind, Opt};
        use crate::flow::Task;
        use crate::resolve::Flow;
        let p = f.controller;
        let pick = match self.answer.take() {
            Some(Choice::Card(c)) => Some(c),
            Some(other) => unreachable!("manifest answer {other:?}"),
            None => None,
        };
        if pick.is_none() {
            if f.pile.is_empty() {
                self.look_top(f, p, 2, false);
            }
            let pile: Vec<CardIdx> = f.pile.iter().copied().filter(|&c| self.objs[c.i()].zone == Zone::Library).collect();
            if pile.is_empty() {
                f.pile.clear();
                return Flow::Next;
            }
            let mut sorted = pile.clone();
            sorted.sort_by_key(|c| (self.objs[c.i()].def, c.0));
            let mut opts = Vec::new();
            for (k, &c) in sorted.iter().enumerate() {
                if k > 0 && self.objs[sorted[k - 1].i()].def == self.objs[c.i()].def {
                    continue;
                }
                opts.push(Opt { action: act::LIBRARY + k as u16, choice: Choice::Card(c) });
            }
            self.ask(Task::Run(f.clone()), p, DecisionKind::PutOntoBattlefield, opts);
            return Flow::Suspend;
        }
        let c = pick.unwrap();
        let rest: Vec<CardIdx> = f.pile.iter().copied().filter(|&x| x != c && self.objs[x.i()].zone == Zone::Library).collect();
        f.pile.clear();
        if self.objs[c.i()].zone == Zone::Library {
            self.note(|_| format!("P{p} manifests a card"));
            self.entering_face_down = true;
            self.put_onto_battlefield(c, p);
            self.entering_face_down = false;
            f.made = (self.objs[c.i()].zone == Zone::Battlefield).then_some(c);
        }
        for x in rest {
            self.move_card(x, Zone::Graveyard, Pos::Top);
        }
        Flow::Next
    }
}

// ---- opening-hand actions (CR 103.6) ----------------------------------------

impl Game {
    /// Cards of `p`'s deck with an opening-hand action `p` may take (the
    /// starting player can't take Gemstone Caverns').
    fn opening_ok(&self, p: PlayerId, c: CardIdx) -> bool {
        self.def_of(c).opening.as_ref().is_some_and(|o| !o.not_starting || p != self.turn.starting)
    }

    /// `Task::Opening`: `p` may begin the game with a card of their opening
    /// hand on the battlefield (CR 103.6; Gemstone Caverns: "with a luck
    /// counter on it. If you do, exile a card from your hand"), one card per
    /// decision (`ChooseHandCard`, `PASS` = no more), then the other player.
    /// Asked whenever `p`'s deck has such a card (decklists are open,
    /// ENGINE.md 10: the question says nothing about the hand).
    pub(crate) fn opening_actions(&mut self, p: PlayerId, exile: u8) {
        use crate::decision::{act, Choice, DecisionKind, Opt};
        use crate::flow::Task;
        match self.answer.take() {
            Some(Choice::Card(c)) if exile > 0 => {
                self.note(|g| format!("P{p} exiles {} from hand", g.name(c)));
                self.move_card(c, Zone::Exile, Pos::Top);
                self.agenda.push(Task::Opening { p, exile: exile - 1 });
                return;
            }
            Some(Choice::Card(c)) => {
                let o = self.def_of(c).opening.clone().expect("an opening-hand action");
                self.note(|g| format!("P{p} begins the game with {} on the battlefield", g.name(c)));
                // (With its counters as it enters, CR 614.1c.)
                self.entering_counters = o.counters.iter().map(|&(k, n)| (k, n as crate::num::Count)).collect();
                self.put_onto_battlefield(c, p);
                self.entering_counters.clear();
                self.agenda.push(Task::Opening { p, exile: o.exile });
                return;
            }
            Some(Choice::Pass) => {
                self.opening_next(p);
                return;
            }
            Some(other) => unreachable!("opening answer {other:?}"),
            None => {}
        }
        if exile > 0 {
            let opts = self.hand_options(p);
            if opts.is_empty() {
                self.agenda.push(Task::Opening { p, exile: 0 });
            } else {
                self.ask(Task::Opening { p, exile }, p, DecisionKind::ExileCost, opts);
            }
            return;
        }
        // (With hidden decks: by the pool the other player knows, ENGINE.md
        // 36.11.)
        let deck = match self.deck_question(p, |d| d.opening.is_some()) {
            Some(known) => known,
            None => self.objs.iter().enumerate().any(|(i, o)| o.owner == p && self.opening_ok(p, CardIdx(i as u16))),
        };
        if !deck {
            self.opening_next(p);
            return;
        }
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        opts.extend(self.hand_options(p).into_iter().filter(|o| matches!(o.choice, Choice::Card(c) if self.opening_ok(p, c))));
        self.ask(Task::Opening { p, exile: 0 }, p, DecisionKind::ChooseHandCard, opts);
    }

    fn opening_next(&mut self, p: PlayerId) {
        if p == self.turn.starting {
            self.agenda.push(crate::flow::Task::Opening { p: crate::ids::opp(p), exile: 0 });
        }
    }
}

// ---- Dauthi Voidwalker (CR 614.1a, 702.28) ---------------------------------

impl Game {
    /// The counter a card `owner` owns is exiled with instead of going to
    /// their graveyard: a permanent an opponent of `owner` controls says so
    /// (Dauthi Voidwalker).
    /// (A Voidwalker leaving in the same simultaneous move still applies:
    /// the replacement exists as the event begins, CR 614.1a.)
    pub(crate) fn void_counter_for(&self, owner: PlayerId) -> Option<crate::db::Counter> {
        let q = crate::ids::opp(owner);
        self.permanents_of(q).find_map(|c| self.abilities(c).void_exile).or_else(|| {
            if !self.batching {
                return None;
            }
            self.leaving.iter().find_map(|l| if l.controller == q { self.db.def(l.def).void_exile } else { None })
        })
    }
}

// ---- the `Ext::M7d` instructions -------------------------------------------

impl Game {
    pub(crate) fn exec_m7d(&mut self, f: &mut crate::resolve::Frame, op: &crate::program::M7d) -> crate::resolve::Flow {
        use crate::decision::{act, Choice, DecisionKind, Opt};
        use crate::flow::Task;
        use crate::program::M7d;
        use crate::resolve::Flow;
        match op {
            M7d::ExiledWithCounterPlayFree { counter } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        if self.objs[c.i()].zone == Zone::Exile {
                            let r = self.obj_ref(c);
                            self.note(|g| format!("P{p} may play {} this turn without paying its mana cost", g.name(c)));
                            self.permissions.push(crate::game::Permission {
                                card: r,
                                player: p,
                                turn: Some(self.turn.number),
                                kind: crate::game::PermKind::Play { free: true, cast_only: false, any_mana: false, only_if: crate::game::PlayIf::Always },
                            });
                        }
                        Flow::Next
                    }
                    Some(other) => unreachable!("exiled-card answer {other:?}"),
                    None => {
                        let q = crate::ids::opp(p);
                        let cands: Vec<CardIdx> = self
                            .exile
                            .iter()
                            .copied()
                            .filter(|&c| self.objs[c.i()].owner == q && self.counters_on(c, *counter) > 0)
                            .take(act::EXILE_N as usize)
                            .collect();
                        if cands.is_empty() {
                            return Flow::Next;
                        }
                        let opts = cands.iter().enumerate().map(|(k, &c)| Opt { action: act::EXILE + k as u16, choice: Choice::Card(c) }).collect();
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseExiled, opts);
                        Flow::Suspend
                    }
                }
            }
            M7d::ExileTargetCounterIfPermanent { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    let crate::ids::Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone != Zone::Graveyard {
                        continue;
                    }
                    // "If it was a permanent card" (as it last existed, CR
                    // 608.2h: in the graveyard).
                    let permanent = self.def_of(o.idx).is_permanent_card();
                    self.move_card(o.idx, Zone::Exile, Pos::Top);
                    if permanent && self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield {
                        self.add_counters(f.source.idx, crate::db::Counter::P1P1, 1);
                    }
                }
                Flow::Next
            }
            M7d::GainClassLevel { level } => {
                // CR 716.2a: the Class's level becomes N (the same object,
                // as a transform: counters, damage and effects stay). A copy
                // of a Class doesn't (no such copy in the cube).
                if self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield && !self.is_copy(f.source.idx) {
                    let c = f.source.idx;
                    let d = self.def_of(c);
                    if d.class_level < *level {
                        if let Some(&face) = d.levels.get(*level as usize - 2) {
                            self.set_face(c, face);
                            self.note(|g| format!("{} becomes level {level}", g.name(c)));
                            let r = self.obj_ref(c);
                            let ev = crate::triggers::Event {
                                kind: crate::db::EventKind::ClassLevel,
                                a: crate::triggers::Subj::Obj(r),
                                b: crate::triggers::Subj::None,
                                amount: *level as u32,
                            };
                            self.event(ev);
                        }
                    }
                }
                Flow::Next
            }
            M7d::BecomeCreatureIfNot { tgt, p, t, subtype } => {
                let ctl = f.controller;
                for c in self.objs_on(f, &crate::program::On::Target(*tgt)) {
                    if self.is_creature(c) {
                        continue;
                    }
                    let r = self.obj_ref(c);
                    let ty = crate::db::Mod::Types { add: crate::db::types::CREATURE, subtypes: vec![*subtype], replace: false };
                    self.add_effect(crate::layers::Affected::Obj(r), ty, crate::layers::Until::Forever, ctl);
                    self.add_effect(crate::layers::Affected::Obj(r), crate::db::Mod::SetPt(*p, *t), crate::layers::Until::Forever, ctl);
                }
                Flow::Next
            }
            M7d::RevealTopLandElseHand => {
                let p = f.controller;
                if let Some(&top) = self.players[p as usize].library.last() {
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                    self.reveal(top);
                    if self.def_of(top).is(crate::db::types::LAND) {
                        self.put_onto_battlefield(top, p);
                    } else {
                        self.move_card(top, Zone::Hand, Pos::Top);
                    }
                }
                Flow::Next
            }
            M7d::ShuffleExileTopPlayFree => {
                let p = f.controller;
                self.shuffle_library(p);
                if let Some(&top) = self.players[p as usize].library.last() {
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                    let r = self.move_card(top, Zone::Exile, Pos::Top);
                    if self.objs[top.i()].zone == Zone::Exile {
                        self.note(|g| format!("P{p} may play {} this turn without paying its mana cost", g.name(top)));
                        self.permissions.push(crate::game::Permission {
                            card: r,
                            player: p,
                            turn: Some(self.turn.number),
                            kind: crate::game::PermKind::Play { free: true, cast_only: false, any_mana: false, only_if: crate::game::PlayIf::Always },
                        });
                    }
                }
                Flow::Next
            }
            M7d::OppExilesThreeMayCastOne { tgt } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        // The rest of the effect waits for the cast (CR
                        // 608.2g).
                        let mut back = f.clone();
                        back.pc += 1;
                        back.i = 0;
                        back.picked.clear();
                        self.agenda.push(Task::Run(back));
                        self.cast_free_now(p, c);
                        return Flow::Suspend;
                    }
                    Some(_) => {
                        f.picked.clear();
                        return Flow::Next;
                    }
                    None => {}
                }
                if f.picked.is_empty() {
                    let Some(q) = self.who(f, crate::program::Who::TargetPlayer(*tgt)) else { return Flow::Next };
                    let mut exiled = Vec::new();
                    // The top card of their library (revealed as it moves).
                    if let Some(&top) = self.players[q as usize].library.last() {
                        self.force_library_card(top, crate::hidden::KEY_REVEAL, q);
                        exiled.push(top);
                    }
                    // A card at random from their graveyard (a public
                    // chance key) and from their hand (keyed by its reveal).
                    let gy: Vec<CardIdx> = self.graveyard_cards(q).collect();
                    let from_gy = (!gy.is_empty()).then(|| {
                        let k = self.rng.below(gy.len() as u32) as usize;
                        self.emit_public(crate::hidden::KEY_DIE, q, k as u64 + 1);
                        gy[k]
                    });
                    let sorted = self.hand_sorted(q);
                    let from_hand = (!sorted.is_empty()).then(|| match self.forced_hand_pick(q, &sorted) {
                        Some(c) => c,
                        None => sorted[self.rng.below(sorted.len() as u32) as usize],
                    });
                    exiled.extend(from_gy);
                    exiled.extend(from_hand);
                    for &c in &exiled {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                    f.picked = exiled.into_iter().filter(|&c| self.objs[c.i()].zone == Zone::Exile).collect();
                }
                // Asked whenever a card exiled this way is still there with
                // a spell face (public), even if none can be cast now:
                // castability counts the caster's hidden hand (a spree's
                // mode costs paid with a Spirit Guide; ENGINE.md 10's second
                // rule, 71.3).
                if !self.may_cast_during_resolution(p) || !self.exile.iter().any(|c| f.picked.contains(c) && self.has_spell_face(*c)) {
                    f.picked.clear();
                    return Flow::Next;
                }
                let cands: Vec<CardIdx> = self.exile.iter().copied().filter(|c| f.picked.contains(c) && self.can_cast_free_now(p, *c)).collect();
                let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                opts.extend(cands.iter().take(act::EXILE_N as usize).enumerate().map(|(k, &c)| Opt { action: act::EXILE + k as u16, choice: Choice::Card(c) }));
                self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseExiled, opts);
                Flow::Suspend
            }
            M7d::ManifestDread => self.manifest_dread(f),
            M7d::ShuffleThisSpellIntoLibrary => {
                // (Only the spell itself, still on the stack: CR 608.2n then
                // has nothing to put into a graveyard.)
                let c = f.source.idx;
                if self.live(f.source) && self.objs[c.i()].zone == Zone::Stack {
                    let owner = self.objs[c.i()].owner;
                    self.move_card(c, Zone::Library, crate::game::Pos::Top);
                    if self.objs[c.i()].zone == Zone::Library {
                        self.shuffle_library(owner);
                        self.note(|g| format!("{} is shuffled into P{owner}'s library", g.name(c)));
                    }
                }
                Flow::Next
            }
            M7d::TriggerThisTurn { k } => {
                let slot = self.db.def(f.source_def).delayed_base + k;
                self.temp_triggers.push(crate::game::TempTrigger {
                    controller: f.controller,
                    source: f.source,
                    source_def: f.source_def,
                    slot,
                    this_turn: true,
                    obj: None,
                });
                Flow::Next
            }
            M7d::TriggerThisTurnFor { k, on } => {
                let slot = self.db.def(f.source_def).delayed_base + k;
                for c in self.objs_on(f, on) {
                    let obj = Some(self.obj_ref(c));
                    self.temp_triggers.push(crate::game::TempTrigger {
                        controller: f.controller,
                        source: f.source,
                        source_def: f.source_def,
                        slot,
                        this_turn: true,
                        obj,
                    });
                }
                Flow::Next
            }
            M7d::UnattachSource => {
                if self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield {
                    let c = f.source.idx;
                    if self.objs[c.i()].attached.take().is_some() {
                        self.note(|g| format!("{} becomes unattached", g.name(c)));
                    }
                }
                Flow::Next
            }
        }
    }
}

// ---- granted triggered abilities with a limit (Nadu) -----------------------

impl Game {
    /// How many times this turn the triggered ability granted by a static
    /// ability of definition `def` triggered for object `r`.
    pub(crate) fn granted_triggered(&self, r: crate::ids::ObjRef, def: crate::ids::DefId, src: crate::ids::ObjRef) -> u8 {
        let turn = self.turn.number as u16;
        self.marks
            .iter()
            .find_map(|&(o, m)| match m {
                crate::game::Mark::Triggered { def: d, turn: t, n, src: i, .. } if o == r && d == def && t == turn && i == src => Some(n),
                _ => None,
            })
            .unwrap_or(0)
    }

    pub(crate) fn note_granted_trigger(&mut self, r: crate::ids::ObjRef, def: crate::ids::DefId, slot: u8, src: crate::ids::ObjRef) {
        let turn = self.turn.number as u16;
        for (o, m) in self.marks.iter_mut() {
            if let crate::game::Mark::Triggered { def: d, slot: s, turn: t, n, src: i } = m {
                if *o == r && *d == def && *s == slot {
                    if *t == turn && *i == src {
                        *n += 1;
                    } else {
                        *t = turn;
                        *i = src;
                        *n = 1;
                    }
                    return;
                }
            }
        }
        self.marks.push((r, crate::game::Mark::Triggered { def, slot, turn, n: 1, src }));
    }
}

// ---- a card name chosen as a permanent enters (Anointed Peacekeeper) -------

impl Game {
    /// The name chosen as `r` entered (`NO_NAME`: a name of no card here).
    pub(crate) fn chosen_name(&self, r: crate::ids::ObjRef) -> Option<crate::ids::DefId> {
        self.marks.iter().find_map(|&(o, m)| match m {
            crate::game::Mark::ChosenName(d) if o == r => Some(d),
            _ => None,
        })
    }

    /// `Task::AsEnters` for "as this enters, look at an opponent's hand, then
    /// choose any card name" (CR 614.12): the look first (once: re-running
    /// the asker must ask the same question, ENGINE.md 4), then `NameCard`
    /// over the names of the opponent's cards (decklists are open: the
    /// names are the same in every world; with hidden decks the names of
    /// their pool the chooser knows, ENGINE.md 36.11), `PASS` = a name of no
    /// card in the game.
    pub(crate) fn choose_name_as_enters(&mut self, card: CardIdx) {
        use crate::decision::{act, Choice, DecisionKind, Opt};
        use crate::flow::Task;
        let p = self.objs[card.i()].controller;
        let q = crate::ids::opp(p);
        let r = self.obj_ref(card);
        let looked = self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::LookedHand);
        if !looked {
            self.reveal_hand(q, Some(p));
            self.marks.push((r, crate::game::Mark::LookedHand));
            self.agenda.push(Task::AsEnters { card });
            return;
        }
        match self.answer.take() {
            Some(Choice::Def(d)) => {
                self.marks.push((r, crate::game::Mark::ChosenName(d)));
                self.note(|g| format!("chosen name: {}", g.db.def(d).name));
            }
            Some(Choice::Pass) => self.marks.push((r, crate::game::Mark::ChosenName(crate::game::NO_NAME))),
            Some(other) => unreachable!("name answer {other:?}"),
            None => {
                // With hidden decks (ENGINE.md 36.11) the opponent's deck is
                // not known: the names are those of their pool the chooser
                // knows, and of their cards the chooser knows here (the same
                // in every world of the chooser's).
                let hidden = self.known_pools[q as usize].is_some();
                let mut defs: Vec<crate::ids::DefId> = (0..self.objs.len())
                    .map(|i| CardIdx(i as u16))
                    .filter(|&c| self.objs[c.i()].owner == q && !self.is_token(c) && !self.is_copy_obj(c) && self.objs[c.i()].zone != Zone::Nowhere)
                    .filter(|&c| !hidden || self.knows(p, c))
                    .map(|c| self.ident_card(c))
                    .collect();
                if let Some(known) = &self.known_pools[q as usize] {
                    defs.extend(known.iter().copied());
                    for k in self.clouds.iter().filter(|k| k.viewer == p && k.owner == q) {
                        defs.extend(k.defs.iter().map(|&d| self.db.def(d).card_def(d)));
                    }
                }
                defs.sort_unstable();
                defs.dedup();
                let mut opts: Vec<Opt> = defs
                    .iter()
                    .take(act::LIBRARY_N as usize)
                    .enumerate()
                    .map(|(i, &d)| Opt { action: act::LIBRARY + i as u16, choice: Choice::Def(d) })
                    .collect();
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::AsEnters { card }, p, DecisionKind::NameCard, opts);
            }
        }
    }

    /// The card a slot is (its front face's definition; a face-down
    /// permanent's own card).
    pub(crate) fn ident_card(&self, c: CardIdx) -> crate::ids::DefId {
        let d = self.ident(c);
        self.db.def(d).card_def(d)
    }

    /// The tax of names chosen for `card`: a spell `caster` casts (an
    /// opponent of the chooser's), else an activated ability of `card` as a
    /// source (anyone's).
    pub(crate) fn named_tax(&self, card: CardIdx, caster: Option<PlayerId>) -> u8 {
        let mut tax = 0u8;
        for &c in &self.battlefield {
            let d = self.abilities(c);
            let n = if caster.is_some() { d.named_spell_tax } else { d.named_ability_tax };
            if n == 0 || caster.is_some_and(|p| self.objs[c.i()].controller == p) {
                continue;
            }
            let Some(nd) = self.chosen_name(self.obj_ref(c)) else { continue };
            if nd != crate::game::NO_NAME && self.name(card) == self.db.def(nd).name {
                tax = tax.saturating_add(n);
            }
        }
        tax
    }
}

// ---- emerge (CR 702.119) -----------------------------------------------------

impl Game {
    /// Does one of `c`'s mana abilities sacrifice it (Lotus Petal)? Such a
    /// permanent, sacrificed for emerge, can't also pay that way.
    pub(crate) fn sacrifices_itself_for_mana(&self, c: CardIdx) -> bool {
        self.abilities(c).activated.iter().any(|a| a.mana().is_some() && a.cost.sac_self)
    }

    /// The permanents `p` could sacrifice to cast `card` with its emerge
    /// alternative cost `k`, cast the way `from` says: those leaving the
    /// rest of the total cost payable (CR 601.2f-h), the reduced mana and
    /// the cost's other parts (`alt_rest`: with retrace, its land discard,
    /// 702.81a; ENGINE.md 40.9).
    pub(crate) fn emerge_options(
        &self,
        p: PlayerId,
        card: CardIdx,
        k: usize,
        classes: &[crate::pay::Class],
        from: crate::game::CastFrom,
    ) -> Vec<CardIdx> {
        let a = &self.def_of(card).alt_costs[k];
        let Some(filter) = &a.emerge else { return Vec::new() };
        // (Emerge's own rest is empty, so no sacrifice changes whether the
        // rest's non-mana parts can be paid: checked once, without mana.)
        let Some(rest) = self.alt_rest(card, from, k) else { return Vec::new() };
        if !self.cost_payable(p, card, rest, &Default::default(), classes, Some(card)) {
            return Vec::new();
        }
        let s = crate::db::SacCost { filter: filter.clone(), n: 1, another: false };
        let mana = self.alt_mana_via(card, p, from, k, &Default::default());
        let keep = crate::pay::Keep { allow: self.spend_allow_from(card, from), ..Self::keep_for(card, rest, None) };
        self.sacrifice_candidates(p, card, &s)
            .into_iter()
            .filter(|&x| {
                let m = mana.less_generic(self.mana_value_of(x));
                let without = if self.sacrifices_itself_for_mana(x) { Some(x) } else { Some(card) };
                m.is_zero() || self.can_pay_without(p, &m, classes, &keep, without)
            })
            .collect()
    }

    /// `Task::Emerge`: choose the permanent to sacrifice (a `Sacrifice`
    /// decision, before the cost is determined, CR 702.119, 601.2f), or
    /// sacrifice it after the mana is paid (601.2h).
    pub(crate) fn emerge(&mut self, id: u32, sac: bool) {
        use crate::decision::{Choice, DecisionKind, Opt};
        use crate::flow::Task;
        let Some(pos) = self.item_index(id) else { return };
        let crate::game::StackKind::Spell { card } = self.stack[pos].kind else { return };
        let p = self.stack[pos].controller;
        if sac {
            if let Some(r) = self.stack[pos].noted.first().copied() {
                if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield {
                    self.note(|g| format!("P{p} sacrifices {} for emerge", g.name(r.idx)));
                    self.sacrifice(r.idx);
                }
            }
            return;
        }
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                let r = self.obj_ref(c);
                self.stack[pos].noted = vec![r];
            }
            Some(other) => unreachable!("emerge answer {other:?}"),
            None => {
                let k = self.stack[pos].alt as usize - 1;
                let from = self.stack[pos].from;
                let classes = self.mana_classes(p, None);
                let opts: Vec<Opt> = self
                    .emerge_options(p, card, k, &classes, from)
                    .into_iter()
                    .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                    .collect();
                self.ask(Task::Emerge { id, sac }, p, DecisionKind::Sacrifice, opts);
            }
        }
    }
}

// ---- the top card of a library, looked at and played (Bolas's Citadel) -------

impl Game {
    /// "You may look at the top card of your library any time": each player
    /// controlling such a permanent knows their top card (a private look,
    /// keyed and forced like any look at a library's top). True when a
    /// queued outcome (`force_chance`) gave a top card another identity.
    pub(crate) fn look_tops(&mut self) -> bool {
        let mut forced = false;
        for p in 0..2u8 {
            let Some(&top) = self.players[p as usize].library.last() else { continue };
            if self.objs[top.i()].known & (1 << p) != 0 || !self.permanents_of(p).any(|c| self.abilities(c).look_top) {
                continue;
            }
            // A top card that changed while its player casts a spell (the
            // spell cast from the top) is seen only once the cast is done
            // (CR 601.2; ruling 2012f8f3).
            let casting = self.agenda.iter().any(|t| {
                matches!(t, crate::flow::Task::CastDone { id } if self.item_index(*id).is_some_and(|i| self.stack[i].controller == p))
            });
            if casting {
                continue;
            }
            let was = self.objs[top.i()].def;
            self.force_library_card(top, crate::hidden::KEY_LOOK, p);
            self.learn(p, top);
            let d = self.objs[top.i()].def;
            forced |= d != was;
            self.emit_private(crate::hidden::KEY_LOOK, p, d as u64);
        }
        forced
    }

    /// May `p` play `card`, the top card of their library, with a permanent
    /// that lets them (Bolas's Citadel)?
    pub(crate) fn top_play_ok(&self, p: PlayerId, card: CardIdx) -> bool {
        self.seen & crate::game::SEEN_TOP != 0
            && self.objs[card.i()].owner == p
            && self.players[p as usize].library.last() == Some(&card)
            && self.permanents_of(p).any(|c| self.abilities(c).play_top_life)
    }

    /// Priority: play or cast the top card of `p`'s library (`LIBRARY` + 0),
    /// any of its faces (a split card's halves, CR 709.3; an MDFC's or an
    /// Adventure's other face, 712.11b, 715.3; `ChooseFace` follows when two
    /// can).
    pub(crate) fn top_options(&self, p: PlayerId, classes: &[crate::pay::Class], sorcery_ok: bool, land_ok: bool, opts: &mut Vec<crate::decision::Opt>) {
        use crate::decision::{act, Opt, FACE_CAST, FACE_PLAY};
        let Some(&top) = self.players[p as usize].library.last() else { return };
        if !self.top_play_ok(p, top) {
            return;
        }
        let def = self.def_of(top);
        let front = if def.halves.is_some() {
            self.split_left_options(p, top, sorcery_ok, classes)
        } else if def.is(crate::db::types::LAND) {
            if land_ok {
                FACE_PLAY
            } else {
                0
            }
        } else if self.castable_via(p, top, crate::game::CastFrom::LibraryTop, classes, sorcery_ok) {
            FACE_CAST
        } else {
            0
        };
        let other = if def.other_face().is_some() { self.other_face_options(p, top, sorcery_ok, land_ok, classes) } else { 0 };
        if let Some(choice) = crate::faces::face_choice(top, front | other) {
            opts.push(Opt { action: act::LIBRARY, choice });
        }
    }
}

// ---- a granted "sacrifice this: add <mana>" (Rain of Filth) -----------------

impl Game {
    /// The mana a permanent's granted "sacrifice this: add <mana>" makes, if
    /// it has one (`Mod::GainSacMana`).
    pub(crate) fn sac_mana_of(&self, c: CardIdx) -> crate::mana::ManaMask {
        let r = self.obj_ref(c);
        self.effects.iter().fold(0, |m, e| match e.m {
            crate::db::Mod::GainSacMana(k) if e.on == crate::layers::Affected::Obj(r) => m | k,
            _ => m,
        })
    }

    /// The sources sacrificing permanent `c` (ENGINE.md 18, 31): Rain of
    /// Filth's granted "sacrifice this: add {B}" (`SAC_FOR`), and the mana
    /// abilities in `sac_by` (permanent, ability) that sacrifice another
    /// permanent: Squandered Resources' "one mana of any type that land
    /// could produce" (`SQ_FOR`, CR 106.7), Orcish Lumberjack's three
    /// (`LJ_FOR`, the Lumberjack tapped too). Each is one more `ACTIVATE` slot
    /// after `c`'s own abilities (none past the fourth: the slots, B). The
    /// card's own tap abilities combine with them (tap it for mana, then
    /// sacrifice it: CR 601.2g).
    pub(crate) fn sac_classes(
        &self,
        p: PlayerId,
        c: CardIdx,
        locks: u16,
        rain: bool,
        sac_by: &[(CardIdx, u8)],
        creature_bonus: [u8; 6],
    ) -> Vec<crate::pay::Class> {
        use crate::pay::{Class, Src, Yield, LJ_FOR, SAC, SAC_FOR, SQ_FOR};
        let mut out = Vec::new();
        let def = self.objs[c.i()].def;
        let mut k = self.abilities(c).activated.len();
        let mut add = |out: &mut Vec<Class>, slot, yld, share: u8| {
            if k < 4 {
                let cl = Class { sac_self: true, res: SAC, slot_id: k as u8, share, ..Class::of(def, Src { card: c, slot }, yld) };
                out.push(cl);
                k += 1;
            }
        };
        if rain && !self.activation_locks_hit(c, locks) {
            let mask = self.sac_mana_of(c);
            if mask != 0 {
                add(&mut out, SAC_FOR, Yield::One(crate::mana::Unit::free(mask)), 0);
            }
        }
        // (One source per kind: the first able; Lumberjacks share the
        // Forests, `Class::share` of them.)
        let mut squandered = false;
        let mut lumber: Option<(CardIdx, u8, u8)> = None;
        for &(by, i) in sac_by {
            if by == c {
                continue;
            }
            let ad = &self.abilities(by).activated[i as usize];
            let Some(s) = &ad.cost.sacrifice else { continue };
            let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(by)) };
            if !self.card_matches(&s.filter, c, &ctx) {
                continue;
            }
            match ad.mana().map(|m| &m.prod) {
                Some(crate::db::ManaProd::SacrificedLand) => squandered = true,
                Some(_) => match &mut lumber {
                    Some((_, _, n)) => *n += 1,
                    None => lumber = Some((by, i, 1)),
                },
                None => {}
            }
        }
        if squandered {
            let mask = self.could_produce(c);
            if mask != 0 {
                add(&mut out, SQ_FOR, Yield::One(crate::mana::Unit::free(mask)), 0);
            }
        }
        if let Some((by, i, n)) = lumber {
            let ad = &self.abilities(by).activated[i as usize];
            let m = ad.mana().expect("a mana ability");
            // ("Whenever you tap a creature for mana": the Lumberjack's.)
            let extra = if ad.cost.tap && self.is_creature(by) { creature_bonus } else { [0; 6] };
            let mut prods = crate::pay::productions(self, p, &m.prod, Some(by));
            for v in prods.iter_mut() {
                for t in 0..6 {
                    v[t] += extra[t];
                }
            }
            if !prods.is_empty() {
                add(&mut out, LJ_FOR, Yield::Multi { prods, units: None }, n);
            }
        }
        out
    }

    fn activation_locks_hit(&self, c: CardIdx, locks: u16) -> bool {
        locks != 0 && self.types_of(c) & locks != 0
    }

    /// Activate the source `src` sacrificing its permanent (`SAC_FOR`,
    /// `SQ_FOR`, `LJ_FOR`): its mana is the payment's.
    pub(crate) fn activate_sac_for(&mut self, src: crate::pay::Src) {
        let c = src.card;
        let p = self.objs[c.i()].controller;
        if src.slot == crate::pay::SAC_FOR {
            self.note(|g| format!("P{p} sacrifices {} for mana", g.name(c)));
        } else {
            // The ability sacrificing it (Squandered Resources, Orcish
            // Lumberjack): one able now (a Lumberjack untapped).
            let locks = self.activation_locks();
            let squandered = src.slot == crate::pay::SQ_FOR;
            let by = self.permanents_of(p).find_map(|x| {
                self.abilities(x).activated.iter().enumerate().find_map(|(i, ad)| {
                    let ok = ad.cost.sacrifice.is_some()
                        && ad.mana().is_some_and(|m| matches!(m.prod, crate::db::ManaProd::SacrificedLand) == squandered)
                        && x != c
                        && self.activation_ready(p, x, ad, locks);
                    ok.then_some((x, i as u8))
                })
            });
            match by {
                Some((x, i)) => {
                    self.note(|g| format!("P{p} activates {} sacrificing {} for mana", g.name(x), g.name(c)));
                    let ad = self.source_def(x).activated_def(crate::db::AbilitySlot::Activated(i));
                    if ad.cost.tap {
                        self.tap(x);
                    }
                }
                None => debug_assert!(false, "no source to sacrifice {c:?}"),
            }
        }
        if self.objs[c.i()].zone == Zone::Battlefield {
            self.sacrifice(c);
        }
    }
}
