//! Edge of Eternities, lane D (ENGINE.md 99.5): the `Ext::EoeD`
//! instructions, and the game facts they read ("dealt damage this turn").

use crate::db::{AbilityDef, CardDef, Filter};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Mark};
use crate::ids::{CardIdx, ObjRef, Target, Zone};
use crate::program::{EoeD, Ext, Op};
use crate::resolve::{Flow, Frame};

/// Does a program read "dealt damage this turn" (`CardDb::watch_damage`)?
fn program_reads_damage(ab: &AbilityDef) -> bool {
    let op_reads = |op: &Op| match op {
        Op::JumpUnlessTarget { filter, .. } => filter_reads_damage(filter),
        _ => false,
    };
    ab.program.iter().any(op_reads)
        || ab.targets.iter().any(|t| filter_reads_damage(&t.filter))
        || ab.modes.as_ref().is_some_and(|m| m.list.iter().any(program_reads_damage))
}

fn filter_reads_damage(f: &Filter) -> bool {
    match f {
        Filter::DealtDamageThisTurn => true,
        Filter::And(a, b) => filter_reads_damage(a) || filter_reads_damage(b),
        Filter::Or(v) => v.iter().any(filter_reads_damage),
        Filter::Not(x) => filter_reads_damage(x),
        _ => false,
    }
}

/// Does a definition read "dealt damage this turn" (its triggered and
/// activated abilities, its spell)?
pub(crate) fn reads_dealt_damage(d: &CardDef) -> bool {
    d.triggers.iter().any(|t| program_reads_damage(&t.ability))
        || d.spell.as_ref().is_some_and(program_reads_damage)
        || d.activated.iter().any(|a| matches!(&a.effect, crate::db::ActEffect::Stack(ab) if program_reads_damage(ab)))
}

impl Game {
    /// Permanent `r` was dealt damage (`Mark::DealtDamage`, this turn).
    pub(crate) fn note_dealt_damage(&mut self, r: ObjRef) {
        let t = self.turn.number as u16;
        match self.marks.iter_mut().find(|(o, m)| *o == r && matches!(m, Mark::DealtDamage(_))) {
            Some(x) => x.1 = Mark::DealtDamage(t),
            None => self.marks.push((r, Mark::DealtDamage(t))),
        }
    }

    /// Was permanent `c` dealt damage this turn (as this object)?
    pub(crate) fn dealt_damage_this_turn(&self, c: CardIdx) -> bool {
        let r = self.obj_ref(c);
        let t = self.turn.number as u16;
        self.marks.iter().any(|&(o, m)| o == r && m == Mark::DealtDamage(t))
    }

    pub(crate) fn exec_eoe_d(&mut self, f: &mut Frame, op: &EoeD) -> Flow {
        match op {
            EoeD::PowerDamageByTarget { by, to, twice_if } => {
                let Some(src) = self.targets_of(f, *by).into_iter().find_map(|t| match t {
                    Target::Obj(o) if self.objs[o.idx.i()].zone == Zone::Battlefield => Some(o),
                    _ => None,
                }) else {
                    return Flow::Next;
                };
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let k = if twice_if.as_ref().is_some_and(|x| self.card_matches(x, src.idx, &ctx)) { 2 } else { 1 };
                let n = self.power(src.idx).max(0).saturating_mul(k);
                let dmg = self.damage_source_of(src.idx);
                let victims = match to {
                    None => vec![Target::Obj(src)],
                    Some(t) => self.targets_of(f, *t),
                };
                for v in victims {
                    self.deal_damage(dmg, v, n);
                }
                Flow::Next
            }
            EoeD::CountControlled { filter, plus } => {
                f.x = plus.saturating_add(self.count_controlled(f.controller, filter) as i32);
                Flow::Next
            }
            EoeD::ApplyPtTimes { on, p, t, n, until } => {
                let n = self.amount(f, *n);
                let (dp, dt) = (*p as i32 * n, *t as i32 * n);
                if dp != 0 || dt != 0 {
                    let m = crate::db::Mod::Pt(dp, dt);
                    self.exec_ext(f, &Ext::Apply { on: on.clone(), mods: vec![m], until: *until });
                }
                Flow::Next
            }
            EoeD::GrantDiesTriggerThisTurn { on, k } => {
                let slot = self.db.def(f.source_def).delayed_base + k;
                for c in self.objs_on(f, on) {
                    let r = self.obj_ref(c);
                    self.marks.push((r, Mark::DiesTriggerThisTurn { def: f.source_def, slot }));
                }
                Flow::Next
            }
            EoeD::ReturnEventCardTapped => {
                f.made = None;
                if let Some(r) = f.ev.obj {
                    let z = self.objs[r.idx.i()].zone;
                    if self.live(r) && matches!(z, Zone::Exile | Zone::Graveyard) {
                        let owner = self.objs[r.idx.i()].owner;
                        self.entering_tapped = true;
                        self.put_onto_battlefield(r.idx, owner);
                        self.entering_tapped = false;
                        f.made = Some(r.idx);
                    }
                }
                Flow::Next
            }
            EoeD::LifeBecomesHalfStarting { who } => {
                if let Some(p) = self.who(f, *who) {
                    let half = (self.spec.starting_life as i64 + 1).div_euclid(2);
                    let diff = half - self.players[p as usize].life as i64;
                    if diff > 0 {
                        self.gain_life(p, crate::num::amount(diff));
                    } else if diff < 0 {
                        self.lose_life(p, crate::num::amount(-diff));
                    }
                }
                Flow::Next
            }
            EoeD::MillHalfLibrary { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    if let Target::Player(p) = t {
                        let n = self.players[p as usize].library.len() / 2;
                        self.mill(p, n, false);
                    }
                }
                Flow::Next
            }
            EoeD::LoseHalfLife { who } => {
                if let Some(p) = self.who(f, *who) {
                    let life = self.players[p as usize].life.max(0) as i64;
                    self.lose_life(p, crate::num::amount((life + 1) / 2));
                }
                Flow::Next
            }
            EoeD::DestroyCreaturesToughnessAtMostX => {
                let x = f.spell_x as i32;
                let all: Vec<CardIdx> =
                    self.battlefield.iter().copied().filter(|&c| self.is_creature(c) && self.toughness(c) <= x).collect();
                self.begin_batch();
                for &c in &all {
                    self.destroy(c);
                }
                self.end_batch();
                // (A token ceases to exist there, CR 111.7: not a card.)
                f.picked = all.into_iter().filter(|&c| self.objs[c.i()].zone == Zone::Graveyard && !self.is_token(c)).collect();
                Flow::Next
            }
            EoeD::ReturnOneDestroyedIfXAtLeast { n } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        if self.objs[c.i()].zone == Zone::Graveyard {
                            self.put_onto_battlefield(c, p);
                        }
                        return Flow::Next;
                    }
                    Some(other) => unreachable!("graveyard choice {other:?}"),
                    None => {}
                }
                if (f.spell_x) < *n {
                    return Flow::Next;
                }
                let opts: Vec<Opt> = f
                    .picked
                    .iter()
                    .copied()
                    .filter(|&c| self.objs[c.i()].zone == Zone::Graveyard)
                    .map(|c| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) })
                    .collect();
                if opts.is_empty() {
                    return Flow::Next;
                }
                let mut opts = opts;
                opts.sort_by_key(|o| o.action);
                self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseGraveyardCard, opts);
                Flow::Suspend
            }
            EoeD::SacrificeSourceUnlessTap { filter } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.tap(c);
                        return Flow::Next;
                    }
                    Some(Choice::Pass) => {
                        self.sacrifice_frame_source(f);
                        return Flow::Next;
                    }
                    Some(other) => unreachable!("sacrifice-unless-tap answer {other:?}"),
                    None => {}
                }
                let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                let cands: Vec<CardIdx> = self
                    .permanents_of(p)
                    .filter(|&c| !self.objs[c.i()].tapped && self.card_matches(filter, c, &ctx))
                    .collect();
                if cands.is_empty() {
                    self.sacrifice_frame_source(f);
                    return Flow::Next;
                }
                let mut opts: Vec<Opt> = cands.iter().map(|&c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) }).collect();
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                opts.sort_by_key(|o| o.action);
                self.ask(Task::Run(f.clone()), p, DecisionKind::ChoosePermanent, opts);
                Flow::Suspend
            }
        }
    }

    /// Combat damage from `src` is prevented ("prevent all combat damage
    /// that would be dealt this turn", `Mod::PreventCombatDamage`, CR
    /// 615.1a) unless it can't be ("can't be prevented": Questing
    /// Beast's `combat_unpreventable`).
    pub(crate) fn combat_damage_prevented(&self, src: &crate::game::DamageSource) -> bool {
        let Some(o) = src.obj else { return false };
        self.live(o)
            && self.objs[o.idx.i()].zone == Zone::Battlefield
            && self.flags_of(o.idx).prevent_combat_damage
            && !self.combat_unpreventable(src.controller)
    }

    /// Sacrifice the resolving ability's source if it is still that
    /// permanent under the ability's controller's control (CR 701.21a).
    fn sacrifice_frame_source(&mut self, f: &Frame) {
        let s = f.source;
        if self.live(s) && self.objs[s.idx.i()].zone == Zone::Battlefield && self.objs[s.idx.i()].controller == f.controller {
            self.sacrifice(s.idx);
        }
    }
}
