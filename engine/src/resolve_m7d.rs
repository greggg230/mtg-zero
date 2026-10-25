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
                        kind: crate::game::PermKind::Play { free: true, cast_only: true, any_mana: false, attacked_subtype: None },
                    });
                }
            }
            Special::TurnFaceUp => self.turn_face_up(card),
        }
    }
}

// ---- face-down permanents (CR 708; ENGINE.md 31) -------------------------

impl Game {
    /// The mana cost of the card a face-down permanent is (CR 701.40b), if it
    /// may be turned face up for it (a creature card).
    pub(crate) fn face_down_cost(&self, _card: CardIdx) -> Option<ManaCost> {
        None
    }

    /// Turn a face-down permanent face up (CR 708.8).
    pub(crate) fn turn_face_up(&mut self, _card: CardIdx) {}

    /// The card a face-down permanent is (its owner's identity), if `c` is
    /// one.
    pub(crate) fn face_down_def(&self, _c: CardIdx) -> Option<&'static crate::db::CardDef> {
        None
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
                self.put_onto_battlefield(c, p);
                if self.objs[c.i()].zone == Zone::Battlefield {
                    for &(k, n) in &o.counters {
                        self.add_counters(c, k, n as u16);
                    }
                }
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
        let deck = self.objs.iter().enumerate().any(|(i, o)| o.owner == p && self.opening_ok(p, CardIdx(i as u16)));
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
                                kind: crate::game::PermKind::Play { free: true, cast_only: false, any_mana: false, attacked_subtype: None },
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
