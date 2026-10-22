//! The agenda runner, the decision API, the turn structure, priority,
//! mulligans and cleanup (ENGINE.md 4, 5, 11).

use crate::decision::{act, Choice, Decision, DecisionKind, Opt};
use crate::game::{EndReason, Game, Outcome, Pos, Step};
use crate::ids::{opp, CardIdx, PlayerId, Zone};
use crate::pay::{PayMana, Src};
use crate::resolve::Frame;
use crate::db::EventKind;
use crate::triggers::Event;

/// One unit of pending work (ENGINE.md 4). The agenda is a stack: the last
/// task runs next. Tasks that ask follow ask-or-consume.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Task {
    // setup (CR 103.5)
    MulliganDeclare { p: PlayerId },
    MulliganRun,
    MulliganBottom { p: PlayerId, left: u8 },
    // turn structure (CR 500-514)
    BeginTurn,
    BeginStep,
    NextStep,
    /// Before priority: SBAs and triggers until stable (CR 117.5), then ask.
    Priority,
    SbaCheck { keep: Vec<CardIdx> },
    PutTriggers,
    PriorityAsk,
    // combat (CR 508-510)
    DeclareAttackers { next: u8 },
    DeclareBlockers { i: u8 },
    CombatDamage { first: bool, att: u8 },
    AssignDamage { att: CardIdx, j: u8, left: u16 },
    // cleanup (CR 514)
    CleanupDiscard,
    CleanupRest,
    // casting and the stack (CR 601, 608)
    ChooseTargets { id: u32, req: u8 },
    CastPay { id: u32 },
    CastDone { id: u32 },
    PayMana(PayMana),
    Resolve,
    Run(Frame),
    FinishResolve { id: u32 },
    // activated abilities (CR 602, 605)
    ActPay { id: u32 },
    PayCosts { id: u32, picked: Vec<CardIdx> },
    CastOption { id: u32 },
    CastAlt { id: u32 },
    Divide { id: u32 },
    ChooseModes { id: u32 },
    ManaNow { src: Src },
    ManaNowPick { p: PlayerId, prods: Vec<[u8; 6]> },
    // entering the battlefield (CR 614.12)
    AsEnters { card: CardIdx },
}

/// Without a decision for this many tasks, the engine is looping.
const TASK_BUDGET: u32 = 2_000_000;

impl Game {
    // ---- the decision API (ENGINE.md 11) -----------------------------------

    pub fn pending_decision(&self) -> Option<&Decision> {
        self.pending.as_ref()
    }

    /// The player to decide; `None` iff the game is over.
    pub fn to_move(&self) -> Option<PlayerId> {
        self.pending.as_ref().map(|d| d.player)
    }

    /// Legal action ids, ascending, no duplicates; empty iff the game is over.
    pub fn legal_actions(&self, out: &mut Vec<u16>) {
        out.clear();
        if let Some(d) = &self.pending {
            out.extend(d.options.iter().map(|o| o.action));
        }
    }

    /// Per-seat result for zero-core's `SearchGame::result`: 1 / 0 for a
    /// win, 0.5 each for a draw; all zero while the game runs.
    pub fn result_values(&self) -> [f32; 2] {
        match self.outcome {
            Some(o) => match o.winner {
                Some(w) => {
                    let mut r = [0.0; 2];
                    r[w as usize] = 1.0;
                    r
                }
                None => [0.5, 0.5],
            },
            None => [0.0; 2],
        }
    }

    /// What `action` means for the pending decision (logs, debugging).
    pub fn describe_action(&self, action: u16) -> Option<String> {
        let d = self.pending.as_ref()?;
        d.choice_of(action).map(|c| format!("{:?} P{}: {}", d.kind, d.player, self.describe_choice(c)))
    }

    /// Take `action` for the pending decision and run to the next decision.
    /// Chance keys revealed on the way are appended to `chance`.
    pub fn apply(&mut self, action: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        let d = self.pending.as_ref().ok_or_else(|| "no decision is pending".to_string())?;
        let choice = d.choice_of(action).ok_or_else(|| format!("illegal action {action} for {:?}", d.kind))?;
        self.pending = None;
        self.answer = Some(choice);
        self.decisions += 1;
        self.advance();
        chance.append(&mut self.keys);
        Ok(())
    }

    /// Run tasks until a decision is pending or the game is over.
    pub(crate) fn advance(&mut self) {
        let mut budget = TASK_BUDGET;
        while self.pending.is_none() && self.outcome.is_none() {
            let t = self.agenda.pop().expect("the agenda is never empty while the game runs");
            self.run_task(t);
            budget -= 1;
            assert!(budget > 0, "engine loop: {TASK_BUDGET} tasks without a decision");
        }
        if self.outcome.is_some() {
            self.pending = None;
            self.answer = None;
            self.agenda.clear();
        }
        self.canon_hand_knowledge();
    }

    /// Ask `player` (ENGINE.md 4): push the asker back and set the decision,
    /// or answer a single option directly (`auto_single`).
    pub(crate) fn ask(&mut self, task: Task, player: PlayerId, kind: DecisionKind, mut options: Vec<Opt>) {
        assert!(!options.is_empty(), "{kind:?} with no options");
        options.sort_by_key(|o| o.action);
        assert!(
            options.windows(2).all(|w| w[0].action < w[1].action),
            "{kind:?}: duplicate action ids {options:?}"
        );
        debug_assert!(options.iter().all(|o| (o.action as usize) < crate::decision::NUM_ACTIONS));
        self.agenda.push(task);
        if options.len() == 1 && self.spec.auto_single && !self.no_auto {
            // The decision is answered here, so no node of the search sees
            // it; that its decider had no alternative is a fact the next
            // signature would otherwise not show (it may depend on hidden
            // cards: no castable spell in hand), so it is keyed (ENGINE.md
            // 10).
            let payload = ((kind as u64) << 16) | options[0].action as u64;
            self.emit_public(crate::hidden::KEY_AUTO, player, payload);
            self.answer = Some(options[0].choice);
            return;
        }
        self.pending = Some(Decision { player, kind, options });
    }

    fn run_task(&mut self, t: Task) {
        match t {
            Task::MulliganDeclare { p } => self.mulligan_declare(p),
            Task::MulliganRun => self.mulligan_run(),
            Task::MulliganBottom { p, left } => self.mulligan_bottom(p, left),
            Task::BeginTurn => self.begin_turn(),
            Task::BeginStep => self.begin_step(),
            Task::NextStep => self.next_step(),
            Task::Priority => {
                self.agenda.push(Task::PriorityAsk);
                self.agenda.push(Task::SbaCheck { keep: Vec::new() });
            }
            Task::SbaCheck { keep } => self.sba_check(keep),
            Task::PutTriggers => self.put_triggers(),
            Task::PriorityAsk => self.priority_ask(),
            Task::DeclareAttackers { next } => self.declare_attackers(next),
            Task::DeclareBlockers { i } => self.declare_blockers(i),
            Task::CombatDamage { first, att } => self.combat_damage(first, att),
            Task::AssignDamage { att, j, left } => self.assign_damage(att, j, left),
            Task::CleanupDiscard => self.cleanup_discard(),
            Task::CleanupRest => self.cleanup_rest(),
            Task::ChooseTargets { id, req } => self.choose_targets(id, req),
            Task::CastPay { id } => self.cast_pay(id),
            Task::CastDone { id } => self.cast_done(id),
            Task::PayMana(pm) => self.pay_mana(pm),
            Task::Resolve => self.resolve_top(),
            Task::Run(f) => self.run_frame(f),
            Task::FinishResolve { id } => self.finish_resolve(id),
            Task::ActPay { id } => self.act_pay(id),
            Task::PayCosts { id, picked } => self.pay_costs(id, picked),
            Task::CastOption { id } => self.cast_option(id),
            Task::CastAlt { id } => self.cast_alt(id),
            Task::Divide { id } => self.divide(id),
            Task::ChooseModes { id } => self.choose_modes(id),
            Task::ManaNow { src } => self.mana_now(src),
            Task::ManaNowPick { p, prods } => self.mana_now_pick(p, prods),
            Task::AsEnters { card } => self.as_enters(card),
        }
    }

    // ---- mulligans (CR 103.5) ----------------------------------------------

    fn mulligan_declare(&mut self, p: PlayerId) {
        let pl = &self.players[p as usize];
        if !pl.kept {
            match self.answer.take() {
                Some(Choice::Yes) => self.players[p as usize].mulligan_declared = true,
                Some(_) => self.players[p as usize].kept = true,
                None => {
                    if pl.mulligans >= self.spec.hand_size {
                        self.players[p as usize].kept = true;
                    } else {
                        let opts = vec![
                            Opt { action: act::PASS, choice: Choice::Pass },
                            Opt { action: act::YES, choice: Choice::Yes },
                        ];
                        self.ask(Task::MulliganDeclare { p }, p, crate::decision::DecisionKind::Mulligan, opts);
                        return;
                    }
                }
            }
        }
        if p == self.turn.starting {
            self.agenda.push(Task::MulliganDeclare { p: opp(p) });
        } else {
            self.agenda.push(Task::MulliganRun);
        }
    }

    fn mulligan_run(&mut self) {
        let order = [self.turn.starting, opp(self.turn.starting)];
        let takers: Vec<PlayerId> =
            order.iter().copied().filter(|&p| self.players[p as usize].mulligan_declared).collect();
        if takers.is_empty() {
            return; // everyone kept
        }
        for &p in &takers {
            let pl = &mut self.players[p as usize];
            pl.mulligan_declared = false;
            pl.mulligans += 1;
            let hand = pl.hand.clone();
            for c in hand {
                self.move_card(c, Zone::Library, Pos::Top);
            }
            self.shuffle_library(p);
            for _ in 0..self.spec.hand_size {
                self.draw(p);
            }
            self.note(|g| format!("P{p} takes mulligan {}", g.players[p as usize].mulligans));
        }
        // Next round after everyone has bottomed; bottoms in turn order.
        self.agenda.push(Task::MulliganDeclare { p: self.turn.starting });
        for &p in takers.iter().rev() {
            let n = self.players[p as usize].mulligans;
            self.agenda.push(Task::MulliganBottom { p, left: n });
        }
    }

    fn mulligan_bottom(&mut self, p: PlayerId, left: u8) {
        if let Some(Choice::Card(c)) = self.answer.take() {
            self.scramble(p, &[], true, false); // a private choice (ENGINE.md 10)
            self.move_card(c, Zone::Library, Pos::Bottom);
            if left > 1 {
                self.agenda.push(Task::MulliganBottom { p, left: left - 1 });
            }
            return;
        }
        if left == 0 || self.players[p as usize].hand.is_empty() {
            return;
        }
        let opts = self.hand_options(p);
        self.ask(Task::MulliganBottom { p, left }, p, DecisionKind::MulliganBottom, opts);
    }

    // ---- turns and steps (CR 500-514) --------------------------------------

    fn begin_turn(&mut self) {
        // CR 500.7: extra turns are inserted; the normal order goes on from
        // the last normal turn.
        let next = if self.turn.number == 0 {
            self.turn.normal = self.turn.starting;
            self.turn.starting
        } else if let Some(p) = self.extra_turns.pop() {
            p
        } else {
            self.turn.normal = opp(self.turn.normal);
            self.turn.normal
        };
        if self.turn.number >= self.spec.max_turns {
            self.outcome = Some(Outcome { winner: None, reason: EndReason::TurnLimit });
            return;
        }
        self.turn.number += 1;
        self.turn.active = next;
        self.turn.step = Step::Untap;
        let n = self.turn.number;
        for pl in &mut self.players {
            pl.left_battlefield = false;
            pl.drawn = 0;
            pl.spells_cast = 0;
            pl.noncreature_cast = 0;
            pl.gained_life = false;
        }
        let pl = &mut self.players[next as usize];
        pl.last_turn = n;
        pl.turns += 1;
        pl.lands_played = 0;
        self.note(|_| format!("turn {n}: P{next}"));
        self.agenda.push(Task::BeginStep);
    }

    /// Turn-based actions of the current step, then priority (CR 117.3a).
    fn begin_step(&mut self) {
        let ap = self.turn.active;
        self.turn.priority = ap;
        self.turn.passes = 0;
        match self.turn.step {
            Step::Setup => unreachable!("setup is not a step"),
            Step::Untap => {
                // CR 502.3; no player gets priority (502.4). "Doesn't untap
                // during your untap step"; a stun counter is removed instead
                // of untapping (122.1d).
                let mine: Vec<CardIdx> = self.permanents_of(ap).collect();
                for c in mine {
                    if self.objs[c.i()].tapped && !self.def_of(c).no_untap {
                        self.untap(c);
                    }
                }
                self.agenda.push(Task::NextStep);
            }
            Step::Draw => {
                // CR 504.1; the starting player skips their first draw (103.8a).
                if self.turn.number > 1 {
                    self.draw(ap);
                }
                self.event(Event::player(EventKind::BeginningOfDrawStep, ap));
                self.agenda.push(Task::Priority);
            }
            Step::DeclareAttackers => {
                self.agenda.push(Task::Priority);
                self.agenda.push(Task::DeclareAttackers { next: 0 });
            }
            Step::DeclareBlockers => {
                self.agenda.push(Task::Priority);
                self.agenda.push(Task::DeclareBlockers { i: 0 });
            }
            Step::FirstStrikeDamage => {
                self.agenda.push(Task::Priority);
                self.agenda.push(Task::CombatDamage { first: true, att: 0 });
            }
            Step::CombatDamage => {
                self.agenda.push(Task::Priority);
                self.agenda.push(Task::CombatDamage { first: false, att: 0 });
            }
            Step::Cleanup => {
                self.turn.cleanup_again = false;
                self.agenda.push(Task::CleanupRest);
                self.agenda.push(Task::CleanupDiscard);
            }
            Step::Upkeep | Step::Main1 | Step::BeginCombat | Step::EndCombat | Step::Main2 | Step::End => {
                // "At the beginning of ..." triggers (CR 503.1a, 505.1,
                // 507.1, 513.1a): the active player is the subject.
                let kind = match self.turn.step {
                    Step::Upkeep => Some(EventKind::BeginningOfUpkeep),
                    Step::Main1 => Some(EventKind::BeginningOfPrecombatMain),
                    Step::BeginCombat => Some(EventKind::BeginningOfCombat),
                    Step::End => Some(EventKind::BeginningOfEndStep),
                    _ => None,
                };
                if let Some(k) = kind {
                    self.event(Event::player(k, ap));
                }
                // Delayed triggered abilities whose moment it is (603.7).
                if !self.delayed.is_empty() {
                    self.fire_delayed();
                }
                self.agenda.push(Task::Priority);
            }
        }
    }

    /// End the current step (mana empties, CR 500.4) and begin the next.
    fn next_step(&mut self) {
        for p in &mut self.players {
            p.pool.clear();
        }
        let next = match self.turn.step {
            Step::Setup => unreachable!(),
            Step::Untap => Step::Upkeep,
            Step::Upkeep => Step::Draw,
            Step::Draw => Step::Main1,
            Step::Main1 => Step::BeginCombat,
            Step::BeginCombat => Step::DeclareAttackers,
            Step::DeclareAttackers => {
                // CR 508.8: no attackers -> skip blockers and damage.
                if self.battlefield.iter().any(|&c| self.objs[c.i()].attacking) {
                    Step::DeclareBlockers
                } else {
                    Step::EndCombat
                }
            }
            Step::DeclareBlockers => {
                if self.first_strike_step_needed() {
                    Step::FirstStrikeDamage
                } else {
                    Step::CombatDamage
                }
            }
            Step::FirstStrikeDamage => Step::CombatDamage,
            Step::CombatDamage => Step::EndCombat,
            Step::EndCombat => {
                self.remove_from_combat(); // CR 511.3
                Step::Main2
            }
            Step::Main2 => Step::End,
            Step::End => Step::Cleanup,
            Step::Cleanup => {
                if self.turn.cleanup_again {
                    Step::Cleanup
                } else {
                    self.agenda.push(Task::BeginTurn);
                    return;
                }
            }
        };
        self.turn.step = next;
        self.agenda.push(Task::BeginStep);
    }

    // ---- priority (CR 117) -------------------------------------------------

    fn priority_ask(&mut self) {
        let p = self.turn.priority;
        match self.answer.take() {
            None => {
                let opts = self.priority_options(p);
                self.ask(Task::PriorityAsk, p, DecisionKind::Priority, opts);
            }
            Some(Choice::Pass) => {
                self.turn.passes += 1;
                if self.turn.passes >= 2 {
                    // CR 117.4: all players passed in succession.
                    self.turn.passes = 0;
                    if self.stack.is_empty() {
                        self.agenda.push(Task::NextStep);
                    } else {
                        self.agenda.push(Task::Priority);
                        self.agenda.push(Task::Resolve);
                    }
                } else {
                    // CR 117.3d
                    self.turn.priority = opp(p);
                    self.agenda.push(Task::Priority);
                }
            }
            Some(Choice::Play(c)) => {
                // Special action (CR 116.2a, 305.1); the player keeps priority.
                self.turn.passes = 0;
                self.players[p as usize].lands_played += 1;
                self.note(|g| format!("P{p} plays {}", g.name(c)));
                self.agenda.push(Task::Priority);
                self.put_onto_battlefield(c, p);
            }
            Some(Choice::Cast(c)) => {
                // CR 117.3c: after casting, the caster receives priority.
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                self.begin_cast(p, c);
            }
            Some(Choice::Ability(c, slot)) => {
                // CR 117.3c also after activating (and 605.3a: a mana
                // ability leaves priority where it is).
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                self.begin_activate(p, c, slot);
            }
            Some(other) => unreachable!("priority answer {other:?}"),
        }
    }

    // ---- cleanup (CR 514) --------------------------------------------------

    fn cleanup_discard(&mut self) {
        let ap = self.turn.active;
        if let Some(Choice::Card(c)) = self.answer.take() {
            self.discard(c);
        }
        if self.players[ap as usize].hand.len() > self.spec.max_hand as usize {
            let opts = self.hand_options(ap);
            self.ask(Task::CleanupDiscard, ap, DecisionKind::Discard, opts);
        }
    }

    fn cleanup_rest(&mut self) {
        // CR 514.2: damage wears off and "until end of turn" effects end,
        // simultaneously.
        for &c in &self.battlefield {
            let o = &mut self.objs[c.i()];
            o.damage = 0;
            o.deathtouched = false;
        }
        self.end_turn_effects();
        // CR 514.3a: an SBA or a trigger -> priority, then another cleanup.
        if self.sba_needed() || !self.waiting.is_empty() {
            self.turn.cleanup_again = true;
            self.agenda.push(Task::Priority);
        } else {
            self.agenda.push(Task::NextStep);
        }
    }
}
