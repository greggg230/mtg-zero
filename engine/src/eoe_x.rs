//! Edge of Eternities, the cross-lane pass (ENGINE.md 99.8): the ops the
//! cards that needed features of no single lane add. Named after the rules
//! actions, each citing its rule; never after a card.

use crate::db::Filter;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, PlayerId, Target, Zone};
use crate::resolve::{Flow, Frame};

/// The cross-lane pass's instructions (`Ext::EoeX`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeX {
    /// The controller searches their library for up to `n` cards matching
    /// `filter` (one search: CR 701.23a; a search for cards with a stated
    /// quality may find fewer, 701.23b), puts them onto the battlefield
    /// (tapped with `tapped`), then shuffles (701.20). Choice: a definition
    /// per card, in non-decreasing order (`PASS` = done), as
    /// `Ext::SearchAnyNumber`.
    SearchUpToOntoBattlefield { filter: Filter, n: u8, tapped: bool },
    /// "<Do something> unless you discard a <filter> card" (CR 701.9a): the
    /// controller may discard a card matching `filter` from their hand; if
    /// they do, continue at `to` (past what they avoided). Choice: a
    /// matching hand card, or `PASS` = don't (no decision without one).
    DiscardMatchingOrContinue { filter: Filter, to: u16 },
    /// "Exile <target card from a graveyard>. Copy it. You may cast the copy
    /// without paying its mana cost": the first legal target of `tgt` is
    /// exiled; its controller creates a copy of it in exile (CR 707.12: a
    /// copy of a card is made in the card's zone, then may be cast), which
    /// they may cast now (608.2g, 118.9); not cast, it ceases to exist at
    /// the next state-based check (704.5e). Choice: `YesNo` (`YES` only if
    /// the copy can be cast now).
    ExileTargetCastCopyFree { tgt: u8 },
}

impl EoeX {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeX::SearchUpToOntoBattlefield { .. } | EoeX::DiscardMatchingOrContinue { .. } | EoeX::ExileTargetCastCopyFree { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            EoeX::DiscardMatchingOrContinue { to, .. } => Some(to),
            EoeX::SearchUpToOntoBattlefield { .. } | EoeX::ExileTargetCastCopyFree { .. } => None,
        }
    }
}

impl Game {
    /// Does `p` control a permanent (its abilities as they are now) for
    /// which `has` holds?
    fn controls_with(&self, p: PlayerId, has: impl Fn(&crate::db::CardDef) -> bool) -> bool {
        self.permanents_of(p).any(|x| has(self.abilities(x)))
    }

    /// The combat damage creature `c` assigns (CR 510.1a, 510.1c-d): its
    /// power; its toughness instead if that is greater and its controller
    /// controls a permanent saying "each creature you control with toughness
    /// greater than its power assigns combat damage equal to its toughness
    /// rather than its power" (Tapestry Warden).
    pub(crate) fn combat_damage_amount(&self, c: CardIdx) -> i32 {
        let p = self.power(c);
        if !self.db.watch_toughness {
            return p;
        }
        let t = self.toughness(c);
        if t > p && self.controls_with(self.objs[c.i()].controller, |d| d.toughness_assigns_damage) {
            t
        } else {
            p
        }
    }

    /// The number a station ability reads from creature `c`, on the
    /// battlefield as the ability resolves (CR 702.184a: its power; 702.184c:
    /// its toughness if that is greater and its controller controls a
    /// permanent saying "each creature you control with toughness greater
    /// than its power stations permanents using its toughness").
    pub(crate) fn station_amount(&self, c: CardIdx) -> i32 {
        let p = self.power(c);
        if !self.db.watch_toughness {
            return p;
        }
        let t = self.toughness(c);
        if t > p && self.controls_with(self.objs[c.i()].controller, |d| d.toughness_stations) {
            t
        } else {
            p
        }
    }

    /// [`station_amount`](Self::station_amount) of a creature that left the
    /// battlefield: `power` and its toughness and controller as it last
    /// existed (CR 608.2h; `Game::lki_toughness`).
    pub(crate) fn station_amount_lki(&self, r: crate::ids::ObjRef, power: i32) -> i32 {
        if !self.db.watch_toughness {
            return power;
        }
        match self.lki_toughness.iter().rev().find(|x| x.0 == r) {
            Some(&(_, t, ctl)) if t > power && self.controls_with(ctl, |d| d.toughness_stations) => t,
            _ => power,
        }
    }

    /// The end of `EoeX::SearchUpToOntoBattlefield`: the found cards enter,
    /// then the library is shuffled (`finish_search_any`).
    fn finish_search_up_to(&mut self, f: &mut Frame, tapped: bool) -> Flow {
        let p = f.controller;
        f.picked = self.cards_of_defs(p, &std::mem::take(&mut f.picked_defs));
        f.floor = 0;
        self.finish_search_any(f, p, tapped, false);
        Flow::Next
    }

    /// `p` creates a copy of `card` (in exile) in exile (CR 707.12, 707.2:
    /// its copiable values), owned by `p`; a slot made as a copy, which
    /// ceases to exist anywhere but the stack and the battlefield (704.5e).
    fn copy_card_in_exile(&mut self, card: CardIdx, p: PlayerId) -> CardIdx {
        let d = self.copiable(card).def;
        let copy = self.add_card(d, p, Zone::Nowhere);
        self.copy_objs.push(copy);
        self.token_slot_made(copy);
        self.move_card(copy, Zone::Exile, Pos::Top);
        self.objs[copy.i()].def = d;
        self.note(|g| format!("P{p} copies {}", g.name(card)));
        copy
    }

    pub(crate) fn exec_eoe_x(&mut self, f: &mut Frame, op: &EoeX) -> Flow {
        match op {
            EoeX::SearchUpToOntoBattlefield { filter, n, tapped } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Def(d)) => {
                        f.picked_defs.push(d);
                        f.floor = d;
                    }
                    Some(Choice::Pass) => return self.finish_search_up_to(f, *tapped),
                    Some(other) => unreachable!("search answer {other:?}"),
                    None => {}
                }
                if f.picked_defs.len() >= *n as usize {
                    return self.finish_search_up_to(f, *tapped);
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
                    let left = lib.iter().filter(|&&x| self.objs[x.i()].def == d).count() > f.picked_defs.iter().filter(|&&x| x == d).count();
                    if d >= f.floor && left && self.card_matches(filter, c, &ctx) {
                        assert!((i as u16) < act::LIBRARY_N, "library slot {i} out of range");
                        opts.push(Opt { action: act::LIBRARY + i as u16, choice: Choice::Def(d) });
                    }
                }
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::Run(f.clone()), p, DecisionKind::SearchLibrary, opts);
                Flow::Suspend
            }
            EoeX::ExileTargetCastCopyFree { tgt } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        let copy = f.picked[0];
                        // The rest of the effect waits for the cast (CR
                        // 608.2g).
                        let mut back = f.clone();
                        back.pc += 1;
                        back.i = 0;
                        back.picked.clear();
                        self.agenda.push(Task::Run(back));
                        self.cast_free_now(p, copy);
                        Flow::Suspend
                    }
                    Some(_) => {
                        f.picked.clear();
                        Flow::Next
                    }
                    None => {
                        let Some(Target::Obj(o)) = self.targets_of(f, *tgt).into_iter().next() else { return Flow::Next };
                        if self.objs[o.idx.i()].zone != Zone::Graveyard {
                            return Flow::Next;
                        }
                        self.move_card(o.idx, Zone::Exile, Pos::Top);
                        if self.objs[o.idx.i()].zone != Zone::Exile {
                            return Flow::Next;
                        }
                        let copy = self.copy_card_in_exile(o.idx, p);
                        f.picked = vec![copy];
                        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                        if self.has_spell_face(copy) && self.can_cast_free_now(p, copy) {
                            opts.push(Opt { action: act::YES, choice: Choice::Yes });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                        Flow::Suspend
                    }
                }
            }
            EoeX::DiscardMatchingOrContinue { filter, to } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} discards {}", g.name(c)));
                        self.discard(c);
                        Flow::Jump(*to)
                    }
                    Some(Choice::Pass) => Flow::Next,
                    Some(other) => unreachable!("discard answer {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                        let mut opts: Vec<Opt> = self
                            .hand_options(p)
                            .into_iter()
                            .filter(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(filter, c, &ctx)))
                            .collect();
                        if opts.is_empty() {
                            return Flow::Next;
                        }
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                        Flow::Suspend
                    }
                }
            }
        }
    }
}
