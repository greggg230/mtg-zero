//! M7b part 3b (ENGINE.md 27): the instructions for spells and abilities on
//! the stack as targets (countering an ability, gaining control of a spell),
//! ward's "counter that spell or ability unless that player pays", and the
//! effects of the graveyard abilities' cards. The initiative and dungeons
//! are in `dungeon.rs`.

use crate::db::{types, Mod};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, StackKind};
use crate::ids::{Target, Zone};
use crate::layers::{Affected, Until};
use crate::program::Ext;
use crate::resolve::{Flow, Frame};

impl Game {
    pub(crate) fn exec_stack(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::SourceOfTargetLosesAbilities { tgt } => {
                // CR 611.2b: "for as long as this creature remains on the
                // battlefield" ends at once if it already left.
                if !(self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield) {
                    return Flow::Next;
                }
                for t in self.targets_of(f, *tgt) {
                    let Target::Stack(id) = t else { continue };
                    let Some(StackKind::Ability { source, .. }) = self.stack_item(id).map(|s| s.kind) else { continue };
                    // "An ability of an artifact, creature, or planeswalker":
                    // that permanent, still the same object.
                    let perm = self.live(source)
                        && self.objs[source.idx.i()].zone == Zone::Battlefield
                        && self.is_type(source.idx, types::ARTIFACT | types::CREATURE | types::PLANESWALKER);
                    if perm {
                        self.note(|g| format!("{} loses all abilities", g.name(source.idx)));
                        self.add_effect(Affected::Obj(source), Mod::LoseAllAbilities, Until::WhileOnBattlefield(f.source), f.controller);
                    }
                }
                Flow::Next
            }
            Ext::GainControlOfSpell { tgt } => {
                let p = f.controller;
                let spells: Vec<u32> = self
                    .targets_of(f, *tgt)
                    .into_iter()
                    .filter_map(|t| match t {
                        Target::Obj(o) if self.objs[o.idx.i()].zone == Zone::Stack => {
                            self.stack.iter().find(|s| s.kind == StackKind::Spell { card: o.idx }).map(|s| s.id)
                        }
                        _ => None,
                    })
                    .collect();
                let Some(&id) = spells.first() else { return Flow::Next };
                let Some(pos) = self.item_index(id) else { return Flow::Next };
                let StackKind::Spell { card } = self.stack[pos].kind else { return Flow::Next };
                // The spell's controller changes (its permanent enters under
                // the new controller, CR 608.3a); then they may choose new
                // targets for it (115.7), each a decision.
                self.stack[pos].controller = p;
                self.objs[card.i()].controller = p;
                self.note(|g| format!("P{p} gains control of {}", g.name(card)));
                if self.stack[pos].targets.is_empty() {
                    return Flow::Next;
                }
                let mut back = f.clone();
                back.pc += 1;
                back.i = 0;
                back.floor = 0;
                self.agenda.push(Task::Run(back));
                self.agenda.push(Task::CopyTargets { id, k: 0, copy: false });
                Flow::Suspend
            }
            Ext::CounterEventItem => {
                let id = f.ev.item;
                let t = match self.stack_item(id).map(|s| s.kind) {
                    Some(StackKind::Spell { card }) => Target::Obj(self.obj_ref(card)),
                    Some(StackKind::Ability { .. }) => Target::Stack(id),
                    None => return Flow::Next,
                };
                self.counter(t, crate::program::SpellDest::Graveyard);
                Flow::Next
            }
            Ext::OfferPayOther { who, life, discard, if_paid } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        // CR 119.4: paying life.
                        self.note(|_| format!("P{p} pays {life} life"));
                        self.lose_life(p, *life as i32);
                        Flow::Jump(*if_paid)
                    }
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} discards {} to pay", g.name(c)));
                        self.discard(c);
                        Flow::Jump(*if_paid)
                    }
                    Some(_) => Flow::Next,
                    None => {
                        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                        if *discard {
                            // A card of their choice (their own hand: the
                            // options are hand slots, ENGINE.md 10).
                            opts.extend(self.hand_options(p));
                        } else if self.players[p as usize].life >= *life as i32 {
                            opts.push(Opt { action: act::YES, choice: Choice::Yes });
                        }
                        let kind = if *discard { DecisionKind::Discard } else { DecisionKind::YesNo };
                        self.ask(Task::Run(f.clone()), p, kind, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::PileToBattlefield { filter } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        f.pile.retain(|&x| x != c);
                        if self.objs[c.i()].zone == Zone::Library {
                            self.put_onto_battlefield(c, p);
                            f.made = (self.objs[c.i()].zone == Zone::Battlefield).then_some(c);
                        }
                        return Flow::Next;
                    }
                    Some(other) => unreachable!("pile answer {other:?}"),
                    None => {}
                }
                f.made = None;
                let ctx = crate::cast::Ctx { controller: p, source: Some(f.source), spell: false };
                let mut sorted = f.pile.clone();
                sorted.sort_by_key(|c| (self.objs[c.i()].def, c.0));
                let mut opts = Vec::new();
                for (k, &c) in sorted.iter().enumerate() {
                    if k > 0 && self.objs[sorted[k - 1].i()].def == self.objs[c.i()].def {
                        continue;
                    }
                    if self.objs[c.i()].zone == Zone::Library && self.card_matches(filter, c, &ctx) {
                        assert!((k as u16) < act::LIBRARY_N, "pile slot {k} out of range");
                        opts.push(Opt { action: act::LIBRARY + k as u16, choice: Choice::Card(c) });
                    }
                }
                if opts.is_empty() {
                    return Flow::Next;
                }
                self.ask(Task::Run(f.clone()), p, DecisionKind::PutOntoBattlefield, opts);
                Flow::Suspend
            }
            Ext::GetExperience { n } => {
                let pl = &mut self.players[f.controller as usize];
                pl.experience = pl.experience.saturating_add(*n as u16);
                Flow::Next
            }
            Ext::CreateTokensN { token, n, tapped, attacking } => {
                let n = self.amount(f, *n).clamp(0, 64) as u8;
                if *attacking {
                    return self.exec_ext(f, &Ext::CreateTokensAttacking { token: *token, n, tapped: *tapped });
                }
                let def = self.db.token_def(*token);
                let made = self.create_tokens(def, f.controller, n as u32, *tapped);
                f.made = made.first().copied();
                f.made_all = made;
                Flow::Next
            }
            Ext::DiscardThenDraw { discard, draw, types: by_type } => {
                // `f.i`: cards discarded so far; `f.x`: nonland ones, or
                // the card types among them (a mask until the end).
                let p = f.controller;
                if f.i == 0 && self.answer.is_none() {
                    f.x = 0;
                }
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        let t = self.def_of(c).types;
                        if *by_type {
                            f.x |= t as i32;
                        } else if t & types::LAND == 0 {
                            f.x += 1;
                        }
                        self.discard(c);
                        f.i += 1;
                    }
                    Some(other) => unreachable!("discard answer {other:?}"),
                    None => {}
                }
                let n = self.amount(f, *discard).max(0) as u32;
                if (f.i as u32) < n && !self.players[p as usize].hand.is_empty() {
                    let opts = self.hand_options(p);
                    self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                    return Flow::Suspend;
                }
                if *by_type {
                    f.x = (f.x as u32).count_ones() as i32;
                }
                self.draw_cards(p, *draw as u32);
                Flow::Next
            }
            Ext::SourceFromGraveyard { tapped } => {
                f.made = None;
                if self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Graveyard {
                    let c = f.source.idx;
                    let owner = self.objs[c.i()].owner;
                    self.put_onto_battlefield(c, owner);
                    if *tapped {
                        self.tap(c);
                    }
                    f.made = (self.objs[c.i()].zone == Zone::Battlefield).then_some(c);
                }
                Flow::Next
            }
            Ext::TakeInitiative { .. } | Ext::Venture { .. } => self.exec_dungeon(f, op),
            other => unreachable!("not a stack instruction: {other:?}"),
        }
    }
}
