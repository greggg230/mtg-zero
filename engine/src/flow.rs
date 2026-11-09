//! The agenda runner, the decision API, the turn structure, priority,
//! mulligans and cleanup (ENGINE.md 4, 5, 11).

use crate::decision::{act, Choice, Decision, DecisionKind, Opt};
use crate::game::{EndReason, Game, Pos, Step};
use crate::ids::{opp, CardIdx, PlayerId, Zone};
use crate::pay::{PayMana, Src};
use crate::resolve::Frame;
use crate::db::EventKind;
use crate::triggers::Event;

/// One unit of pending work (ENGINE.md 4). The agenda is a stack: the last
/// task runs next. Tasks that ask follow ask-or-consume.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Task {
    // setup (CR 103; ENGINE.md 35)
    /// CR 103.1: `p` chooses who takes the first turn.
    PlayDraw { p: PlayerId },
    /// CR 103.2b: `p` may reveal a companion from their sideboard.
    RevealCompanion { p: PlayerId },
    /// CR 103.3, 103.5: the libraries are shuffled and the opening hands
    /// drawn.
    DrawHands,
    /// CR 116.2g: `p` paid {3}: their companion is put into their hand.
    CompanionToHand { p: PlayerId },
    MulliganDeclare { p: PlayerId },
    MulliganRun,
    MulliganBottom { p: PlayerId, left: u8 },
    // turn structure (CR 500-514)
    BeginTurn,
    /// "You may skip that turn instead" (Time Vault): `p`'s turn is about
    /// to begin while `vault` is tapped (ENGINE.md 28).
    MaySkipTurn { p: PlayerId, vault: CardIdx },
    BeginStep,
    NextStep,
    /// Before priority: SBAs and triggers until stable (CR 117.5), then ask.
    Priority,
    SbaCheck { keep: Vec<CardIdx> },
    /// (`page`: the page of the order decision, ENGINE.md 100.2.)
    PutTriggers { page: crate::decision::Page },
    /// Miracle (CR 702.94a): may `p` reveal `card`, the first card they drew
    /// this turn? (Asked before the next priority.)
    MiracleReveal { p: PlayerId, card: CardIdx },
    /// The `n` draws of an instruction left after a miracle question (CR
    /// 702.94a), in event group `ev`; `step`: then the draw step's
    /// beginning event (`Game::queue_miracle`).
    DrawRest { p: PlayerId, n: u32, ev: u32, step: bool },
    /// CR 616.1: `p` orders an opponent's "create a token instead" and their
    /// own "draw that many plus `plus` instead" on one draw
    /// (`Game::draw_order`).
    DrawOrder { p: PlayerId, plus: u32, ev: u32, step: bool },
    /// CR 613.7m: the active player orders the timestamps of the objects
    /// given ones simultaneously since `first` (`Game::order_timestamps`).
    OrderTimestamps { first: u32, settled: bool, picked: Vec<CardIdx> },
    /// (`page`: the page of the priority decision, ENGINE.md 100.2.)
    PriorityAsk { page: crate::decision::Page },
    // combat (CR 508-510)
    DeclareAttackers { next: u8 },
    DeclareBlockers { i: u8 },
    CombatDamage { first: bool, att: u8 },
    /// (`step`: where the amount's digits stand, ENGINE.md 100.2.)
    AssignDamage { att: CardIdx, j: u32, left: u64, step: crate::decision::NumberStep },
    // cleanup (CR 514)
    CleanupDiscard,
    CleanupRest,
    // casting and the stack (CR 601, 608)
    /// (`page`: the page of the decision, ENGINE.md 100.2.)
    ChooseTargets { id: u32, req: u8, page: crate::decision::Page },
    CastPay { id: u32 },
    CastDone { id: u32 },
    PayMana(Box<PayMana>),
    Resolve,
    Run(Frame),
    FinishResolve { id: u32 },
    // activated abilities (CR 602, 605)
    ActPay { id: u32 },
    PayCosts { id: u32, picked: Vec<CardIdx> },
    CastOption { id: u32 },
    CastAlt { id: u32 },
    Divide { id: u32, step: crate::decision::NumberStep },
    ChooseModes { id: u32 },
    ManaNow { src: Src, step: crate::decision::NumberStep },
    /// (`only`: the mana's spending restriction, kept in the pool, CR
    /// 106.6; ENGINE.md 99.12.)
    ManaNowPick { p: PlayerId, prods: Vec<[u8; 6]>, only: u8, page: crate::decision::Page },
    /// Mana abilities activated with priority (`DecisionKind::ActivateMana`;
    /// `card`: the permanent whose ability is asked next; `again`: one was
    /// activated, so `PASS` ends it; ENGINE.md 100.4.3).
    ActivateMana { p: PlayerId, card: Option<CardIdx>, again: bool },
    /// Devour land (CR 702.82a): the lands chosen so far, in battlefield
    /// order (ENGINE.md 99.12).
    Devour { card: CardIdx, picked: Vec<CardIdx> },
    /// The rest of a priority mana ability's cost (ENGINE.md 28).
    ManaNowCosts { src: Src, sac: Option<CardIdx>, x: u32, taps: Vec<CardIdx> },
    // entering the battlefield (CR 614.12)
    /// (`page`: the page of a paged choice, ENGINE.md 100.2.)
    AsEnters { card: CardIdx, page: crate::decision::Page },
    /// Exile graveyard cards for delve (CR 702.66).
    Delve { id: u32 },
    /// Discard a land for Mox Diamond to enter, or not.
    EnterByDiscard { id: u32 },
    /// Mox Diamond put onto the battlefield other than by resolving: discard
    /// a land card (it enters, under `controller`) or not (its owner's
    /// graveyard).
    EnterByDiscardAny { card: CardIdx, controller: PlayerId, tapped: bool },
    /// CR 303.4f: an Aura put onto the battlefield other than by resolving
    /// (Animate Dead returned from exile): `controller` chooses, as it
    /// enters, the creature card in a graveyard it enchants; it waits in its
    /// zone meanwhile.
    EnterAura { card: CardIdx, controller: PlayerId, tapped: bool, page: crate::decision::Page },
    /// CR 616.1: `chooser` orders the replacement effects of `card`'s move
    /// from `from` (exiled for now): `results` the reachable results.
    ChooseReplacement { card: crate::ids::ObjRef, from: Zone, results: u8, void: Option<crate::db::Counter>, chooser: PlayerId },
    /// Announce X (CR 601.2b, 107.3).
    ChooseX { id: u32, step: crate::decision::NumberStep },
    /// Discard for escalate (CR 702.120a).
    Escalate { id: u32 },
    /// "As an additional cost, choose a creature you control or a warped
    /// creature card you own in exile" (CR 601.2h; ENGINE.md 99.14): an
    /// exiled card (`ChooseExiled`, `PASS` = a creature instead), then, with
    /// `perm`, a creature (`ChoosePermanent`).
    ChooseForCost { id: u32, perm: bool, page: crate::decision::Page },
    /// Which player or planeswalker an attacking creature attacks (CR
    /// 508.1b, 508.4).
    AttackTarget { card: CardIdx },
    /// Which face of a card to cast or play (CR 712.11b, 712.12, 715.3;
    /// `mask`: `decision::FACE_*` bits).
    ChooseFace { p: PlayerId, card: CardIdx, mask: u8 },
    /// Which activated ability of permanent `card` `p` activates, among its
    /// abilities from the fourth on (CR 602.2: one with more abilities than
    /// its `ACTIVATE` slots; EoE W3). Choice: `ChooseMode`, `LIBRARY` + place
    /// - 3 (the list block holds the definition with the ability and its
    /// index there), paged past 64 (`page`; ENGINE.md 100.2).
    ChooseAbility { p: PlayerId, card: CardIdx, page: crate::decision::Page },
    /// CR 601.2a-b: which permission casts graveyard card `card` (face
    /// `face`; `cast::route`).
    ChooseRoute { p: PlayerId, card: CardIdx, face: Option<crate::ids::DefId> },
    /// What a permanent that may enter as a copy copies (CR 707.5), then
    /// it enters (`note`: its `Enters` event's).
    EnterAsCopy { card: CardIdx, note: u32, since: u32, page: crate::decision::Page },
    /// New targets for a copy of a spell (`copy`, CR 707.10c) or a spell
    /// whose controller may choose new targets (115.7), from its target `k`
    /// on.
    CopyTargets { id: u32, k: u8, copy: bool, page: crate::decision::Page },
    /// Announce how many times an additional cost is paid (CR 601.2b:
    /// replicate, squad, offspring).
    ChooseTimes { id: u32, step: crate::decision::NumberStep },
    /// Splice cards from hand onto the spell (CR 702.47a, 601.2b): one card
    /// per decision, `PASS` = no more.
    Splice { id: u32 },
    /// A special action whose cost was just paid (CR 116.2; M7d).
    Special { p: PlayerId, card: CardIdx, sp: crate::db::Special },
    /// Opening-hand actions (CR 103.6): `p` may begin the game with a card
    /// on the battlefield; `exile` cards still to exile from hand for it.
    Opening { p: PlayerId, exile: u8 },
    /// Emerge (CR 702.119): choose the permanent to sacrifice (`sac`: false),
    /// or sacrifice it once the mana is paid (601.2h).
    Emerge { id: u32, sac: bool },
    /// A unit test's paged choice or big number (`decision::tests`, ENGINE.md
    /// 100.1): asks through `ask_paged_ranges` or `ask_number` and logs the
    /// answer. No game asks it.
    #[cfg(test)]
    Probe(Box<crate::decision::tests::Probe>),
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
        // (Who decides: the controlling player during a controlled turn,
        // CR 722.4; the decision's ids stay the controlled player's.)
        self.pending.as_ref().map(|d| self.decider_of(d.player))
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
                // (A draw; an aborted game has no result and is dropped by
                // self-play: 0.5 each is what a driver that doesn't drop it
                // would see.)
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
        self.decisions = self.exact_inc(self.decisions, "decisions");
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
        // "You may look at the top card of your library any time" (Bolas's
        // Citadel): before any decision, its controller knows it. (Priority,
        // whose options name the top card, looks before computing them:
        // `priority_ask`. No other decision's options read a top card its
        // controller has not seen, so a queued outcome of this look changes
        // none; debug builds check it, ENGINE.md 74.)
        if self.seen & crate::game::SEEN_TOP != 0 && self.outcome.is_none() && self.look_tops() && cfg!(debug_assertions) {
            let mut c = self.clone();
            c.keys.clear();
            c.refresh_pending();
            assert_eq!(c.pending, self.pending, "a forced look at a top card changed the pending decision");
        }
        if self.turn_controller.is_some() {
            self.sync_controller_view(); // CR 722.3 (ENGINE.md 99.14)
        }
        self.canon_hand_knowledge();
    }

    /// Ask `player` (ENGINE.md 4): push the asker back and set the decision,
    /// or answer a single option directly (`auto_single`).
    pub(crate) fn ask(&mut self, task: Task, player: PlayerId, kind: DecisionKind, mut options: Vec<Opt>) {
        assert!(!options.is_empty(), "{kind:?} with no options");
        // A graveyard card's option outside Priority: its slot in the
        // decision's graveyard list (ENGINE.md 65.10).
        self.name_graveyard_options(player, kind, &mut options);
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
        self.pending = Some(Decision { player, kind, options, stage: crate::decision::Stage::One });
    }

    fn run_task(&mut self, t: Task) {
        match t {
            Task::PlayDraw { p } => self.play_draw(p),
            Task::RevealCompanion { p } => self.reveal_companion(p),
            Task::DrawHands => self.draw_hands(),
            Task::CompanionToHand { p } => self.companion_to_hand(p),
            Task::MulliganDeclare { p } => self.mulligan_declare(p),
            Task::MulliganRun => self.mulligan_run(),
            Task::MulliganBottom { p, left } => self.mulligan_bottom(p, left),
            Task::BeginTurn => self.begin_turn(),
            Task::MaySkipTurn { p, vault } => self.may_skip_turn(p, vault),
            Task::BeginStep => self.begin_step(),
            Task::NextStep => self.next_step(),
            Task::Priority => {
                self.agenda.push(Task::PriorityAsk { page: crate::decision::Page::FIRST });
                self.agenda.push(Task::SbaCheck { keep: Vec::new() });
            }
            Task::SbaCheck { keep } => self.sba_check(keep),
            Task::MiracleReveal { p, card } => self.miracle_reveal(p, card),
            Task::DrawRest { p, n, ev, step } => self.draw_rest(p, n, ev, step),
            Task::DrawOrder { p, plus, ev, step } => self.draw_order(p, plus, ev, step),
            Task::OrderTimestamps { first, settled, picked } => self.order_timestamps(first, settled, picked),
            Task::PutTriggers { page } => self.put_triggers(page),
            Task::PriorityAsk { page } => self.priority_ask(page),
            Task::DeclareAttackers { next } => self.declare_attackers(next),
            Task::DeclareBlockers { i } => self.declare_blockers(i),
            Task::CombatDamage { first, att } => self.combat_damage(first, att),
            Task::AssignDamage { att, j, left, step } => self.assign_damage(att, j, left, step),
            Task::CleanupDiscard => self.cleanup_discard(),
            Task::CleanupRest => self.cleanup_rest(),
            Task::ChooseTargets { id, req, page } => self.choose_targets(id, req, page),
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
            Task::Divide { id, step } => self.divide(id, step),
            Task::ChooseModes { id } => self.choose_modes(id),
            Task::ManaNow { src, step } => self.mana_now(src, step),
            Task::ManaNowPick { p, prods, only, page } => self.mana_now_pick(p, prods, only, page),
            Task::ActivateMana { p, card, again } => self.activate_mana(p, card, again),
            Task::ManaNowCosts { src, sac, x, taps } => self.mana_now_costs(src, sac, x, taps),
            Task::AsEnters { card, page } => self.as_enters(card, page),
            Task::Devour { card, picked } => self.devour(card, picked),
            Task::Delve { id } => self.delve(id),
            Task::EnterByDiscard { id } => self.enter_by_discard(id),
            Task::EnterByDiscardAny { card, controller, tapped } => self.enter_by_discard_any(card, controller, tapped),
            Task::EnterAura { card, controller, tapped, page } => self.enter_aura(card, controller, tapped, page),
            Task::ChooseReplacement { card, from, results, void, chooser } => self.choose_replacement(card, from, results, void, chooser),
            Task::ChooseX { id, step } => self.choose_x(id, step),
            Task::Escalate { id } => self.escalate(id),
            Task::ChooseForCost { id, perm, page } => self.choose_for_cost(id, perm, page),
            Task::AttackTarget { card } => self.choose_attack_target(card),
            Task::ChooseFace { p, card, mask } => self.choose_face(p, card, mask),
            Task::ChooseAbility { p, card, page } => self.choose_ability(p, card, page),
            Task::ChooseRoute { p, card, face } => self.choose_route(p, card, face),
            Task::EnterAsCopy { card, note, since, page } => self.enter_as_copy(card, note, since, page),
            Task::CopyTargets { id, k, copy, page } => self.copy_targets(id, k, copy, page),
            Task::ChooseTimes { id, step } => self.choose_times(id, step),
            Task::Splice { id } => self.choose_splice(id),
            Task::Special { p, card, sp } => self.special_action(p, card, sp),
            Task::Opening { p, exile } => self.opening_actions(p, exile),
            Task::Emerge { id, sac } => self.emerge(id, sac),
            #[cfg(test)]
            Task::Probe(p) => self.run_probe(*p),
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
            self.end_game(None, EndReason::TurnLimit);
            return;
        }
        // "If you would begin your turn while this artifact is tapped, you
        // may skip that turn instead" (a replacement effect, CR 614.10: the
        // turn is skipped, the next one is computed as usual).
        let vault = self
            .permanents_of(next)
            .find(|&c| self.objs[c.i()].tapped && self.abilities(c).skip_turn_untap);
        match vault {
            Some(vault) => self.agenda.push(Task::MaySkipTurn { p: next, vault }),
            None => self.start_turn(next),
        }
    }

    /// Time Vault's choice (ENGINE.md 28): skip the turn and untap it, or
    /// begin the turn.
    fn may_skip_turn(&mut self, p: PlayerId, vault: CardIdx) {
        match self.answer.take() {
            Some(Choice::Yes) => {
                self.note(|g| format!("P{p} skips a turn to untap {}", g.name(vault)));
                self.untap(vault);
                self.agenda.push(Task::BeginTurn);
            }
            Some(_) => self.start_turn(p),
            None => {
                let opts = vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                self.ask(Task::MaySkipTurn { p, vault }, p, DecisionKind::YesNo, opts);
            }
        }
    }

    /// `next`'s turn begins (CR 500.1).
    fn start_turn(&mut self, next: PlayerId) {
        // CR 731.2: the previous turn's active player's spells decide
        // whether day becomes night or night becomes day (checked as the
        // untap step's second part, below).
        let prev_spells = (self.turn.number > 0).then(|| self.players[self.turn.active as usize].spells_cast);
        self.turn.number += 1;
        self.turn.active = next;
        // CR 722.1: a player-controlling effect applies to the next turn the
        // player takes, all of it, until the next turn begins.
        self.turn_controller = self.next_turn_controller[next as usize].take().filter(|&q| q != next);
        self.turn.extra_combats = 0;
        self.turn.after_combat = Step::Main2;
        self.turn.step = Step::Untap;
        let n = self.turn.number;
        for pl in &mut self.players {
            pl.attacked_with.clear();
            pl.draw_step_draws = 0;
            pl.left_battlefield = false;
            pl.drawn = 0;
            pl.spells_cast = 0;
            pl.noncreature_cast = 0;
            pl.gained_life = false;
            pl.artifact_entered = false;
            pl.life_gained = 0;
            pl.lost_life = false;
        }
        // "Until your next turn" ends as that turn begins (CR 611.2b).
        self.players[next as usize].flash_sorceries = false;
        self.end_until_next_turn(next);
        // ("Until your next turn, whenever ..." ends as that turn begins;
        // "whenever ... this turn" ended in the cleanup step, 514.2.)
        if !self.temp_triggers.is_empty() {
            self.temp_triggers.retain(|t| t.controller != next);
        }
        self.resolved.clear();
        self.done_once.clear();
        if !self.marks.is_empty() {
            // Facts about objects that are gone (CR 400.7).
            let marks = std::mem::take(&mut self.marks);
            self.marks = marks.into_iter().filter(|&(r, _)| self.live(r)).collect();
        }
        self.discarded.clear();
        self.lki_counters.clear();
        self.lki_copy.clear();
        self.lki_attached.clear();
        self.lki_damage.clear();
        self.lki_toughness.clear();
        self.w3.lki_host.clear();
        self.w3.lki_loss.clear();
        self.spell_lki.clear();
        self.may_copies = 0;
        self.activations.clear();
        // Permissions end with their turn, or when the card moves.
        let perms = std::mem::take(&mut self.permissions);
        self.permissions = perms
            .into_iter()
            .filter(|p| {
                p.turn.is_none_or(|t| {
                    if t & crate::resolve_tail2::PERM_LATER != 0 {
                        true
                    } else if t & (crate::resolve_tail2::PERM_UNTIL_TURNS | crate::resolve_tail3::PERM_UNTIL_END_STEP) != 0 {
                        // (Through the end of the player's k-th turn: their
                        // count once this turn begins.)
                        let k = t & !(crate::resolve_tail2::PERM_UNTIL_TURNS | crate::resolve_tail3::PERM_UNTIL_END_STEP);
                        let turns = self.players[p.player as usize].turns + (p.player == next) as u32;
                        turns < k || turns == k && p.player == next
                    } else {
                        t >= n
                    }
                }) && self.live(p.card)
            })
            .collect();
        let pl = &mut self.players[next as usize];
        pl.last_turn = n;
        pl.turns += 1;
        pl.lands_played = 0;
        self.note(|_| format!("turn {n}: P{next}"));
        if let (Some(spells), Some(_)) = (prev_spells, self.day) {
            self.day_night_turn(spells);
        }
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
                // CR 502.1: first, phasing (EoE W3).
                if !self.w3.phased.is_empty() {
                    self.phase_in_untap(ap);
                }
                // CR 502.3; no player gets priority (502.4). "Doesn't untap
                // during your untap step"; a stun counter is removed instead
                // of untapping (122.1d).
                let mine: Vec<CardIdx> = self.permanents_of(ap).collect();
                for c in mine {
                    // CR 701.39b: an exerted permanent doesn't untap during
                    // its controller's next untap step.
                    if !self.marks.is_empty() {
                        let r = self.obj_ref(c);
                        if let Some(i) = self.marks.iter().position(|&m| m == (r, crate::game::Mark::Exerted)) {
                            self.marks.remove(i);
                            continue;
                        }
                    }
                    if self.objs[c.i()].tapped && !self.abilities(c).no_untap && !(self.w6.seen & crate::eoe_w6::SEEN_W6_NO_UNTAP != 0 && self.w6_no_untap(c)) {
                        self.untap(c);
                    }
                }
                self.agenda.push(Task::NextStep);
            }
            Step::Draw => {
                // CR 504.1; the starting player skips their first draw (103.8a).
                let queued = self.agenda.len();
                let cut = if self.turn.number > 1 { self.draw_instruction(ap, 1, true) } else { crate::game::Drew::Done };
                match cut {
                    // (Interrupted by a miracle question with draws left, a
                    // replacement's, or by a question about the order of
                    // replacements: the step's event after them.)
                    crate::game::Drew::Miracle(card, left) if left > 0 => self.queue_miracle(ap, card, left, true),
                    crate::game::Drew::Asked => {}
                    crate::game::Drew::Miracle(card, _) => {
                        self.event(Event::player(EventKind::BeginningOfDrawStep, ap));
                        self.queue_miracle(ap, card, 0, false);
                    }
                    crate::game::Drew::Done => self.event(Event::player(EventKind::BeginningOfDrawStep, ap)),
                }
                let tasks = self.agenda.split_off(queued);
                self.agenda.push(Task::Priority);
                self.agenda.extend(tasks);
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
                // "At the beginning of the next cleanup step" (514.3a: a
                // trigger then gives priority, and another cleanup follows).
                if !self.delayed.is_empty() {
                    self.fire_delayed();
                }
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
                // CR 714.3c: as the precombat main phase begins, its player
                // puts a lore counter on each Saga they control (a
                // turn-based action).
                if self.turn.step == Step::Main1 {
                    self.lore_turn_action(ap);
                }
                if let Some(k) = kind {
                    self.event(Event::player(k, ap));
                }
                // CR 725.2: "at the beginning of the upkeep of the player who
                // has the initiative, that player ventures into Undercity".
                if self.turn.step == Step::Upkeep && self.initiative == Some(ap) {
                    let ev = crate::triggers::EvCtx { player: Some(ap), ..Default::default() };
                    self.initiative_trigger(ap, crate::dungeon::UPKEEP, ev);
                }
                // CR 724.2: "at the beginning of the monarch's end step, that
                // player draws a card" (the monarch's inherent triggered
                // ability).
                if self.turn.step == Step::End && self.monarch == Some(ap) {
                    let ev = crate::triggers::EvCtx { player: Some(ap), ..Default::default() };
                    self.monarch_trigger(ap, crate::dungeon::MONARCH_END_STEP, ev);
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
        self.pool_restr.clear();
        let next = match self.turn.step {
            Step::Setup => unreachable!(),
            Step::Untap => Step::Upkeep,
            Step::Upkeep => Step::Draw,
            // CR 500.8: combat phases added after the beginning phase or
            // the postcombat main phase come directly after it; the turn
            // goes on with the phase that came next there.
            Step::Draw if self.turn.extra_combats > 0 => {
                self.turn.extra_combats -= 1;
                self.turn.after_combat = Step::Main1;
                Step::BeginCombat
            }
            Step::Draw => Step::Main1,
            // (Combat phases added after the precombat main phase follow
            // its combat phase: they are combat phases one after another.)
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
                if self.turn.extra_combats > 0 {
                    // CR 500.8: an additional combat phase follows.
                    self.turn.extra_combats -= 1;
                    Step::BeginCombat
                } else {
                    match std::mem::replace(&mut self.turn.after_combat, Step::Main2) {
                        Step::Untap => {
                            self.agenda.push(Task::BeginTurn);
                            return;
                        }
                        s => s,
                    }
                }
            }
            Step::Main2 if self.turn.extra_combats > 0 => {
                self.turn.extra_combats -= 1;
                self.turn.after_combat = Step::End;
                Step::BeginCombat
            }
            Step::Main2 => Step::End,
            Step::End => Step::Cleanup,
            Step::Cleanup => {
                if self.turn.cleanup_again {
                    Step::Cleanup
                } else if self.turn.extra_combats > 0 {
                    // (After the ending phase: the turn ends after them.)
                    self.turn.extra_combats -= 1;
                    self.turn.after_combat = Step::Untap;
                    Step::BeginCombat
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

    fn priority_ask(&mut self, page: crate::decision::Page) {
        let p = self.turn.priority;
        match self.answer.take() {
            None => {
                // Bolas's Citadel: the options name the top card of the
                // library by what it is (cast it, play it, its faces), so its
                // controller looks at a new top card before they are
                // computed, not after (`advance`'s look): a look's queued
                // outcome (`force_chance`) may give the card another identity,
                // and the options must be that card's (ENGINE.md 74).
                if self.seen & crate::game::SEEN_TOP != 0 {
                    let _ = self.look_tops();
                }
                // (Exiled cards to play and graveyard abilities: every one,
                // paged past their ids, ENGINE.md 100.2.)
                let (opts, exile, gy) = self.priority_options(p);
                let ranges = [
                    crate::decision::PagedRange { base: act::EXILE, n: act::EXILE_N, items: &exile },
                    crate::decision::PagedRange { base: act::GY_ABILITY, n: act::GY_ABILITY_N, items: &gy },
                ];
                self.ask_paged_ranges(Task::PriorityAsk { page }, page, p, DecisionKind::Priority, &ranges, opts);
            }
            Some(Choice::Page(next)) => self.agenda.push(Task::PriorityAsk { page: next }),
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
                self.agenda.push(Task::Priority);
                self.play_land(p, c, None);
            }
            Some(Choice::Cast(c)) => {
                // CR 117.3c: after casting, the caster receives priority.
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                self.begin_cast(p, c);
            }
            Some(Choice::CastFace(c)) => {
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                let face = self.other_face(c).expect("a card with another face");
                self.begin_cast_face(p, c, Some(face));
            }
            Some(Choice::PlayFace(c)) => {
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                self.play_face(p, c);
            }
            Some(Choice::Faces(c, mask)) => {
                // Both faces can be cast or played: which one is the next
                // decision (CR 712.11b, 712.12, 715.3). The card will be
                // public either way, so it is revealed now (a refreshed
                // decision must find the same card, ENGINE.md 10).
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                self.reveal(c);
                self.agenda.push(Task::ChooseFace { p, card: c, mask });
            }
            Some(Choice::Ability(c, slot)) => {
                // CR 117.3c also after activating (and 605.3a: a mana
                // ability leaves priority where it is).
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                // (The fourth slot of a permanent with more abilities than
                // slots: which of the fourth and later ones, EoE W3.)
                if self.paged_slot(c, slot) {
                    self.agenda.push(Task::ChooseAbility { p, card: c, page: crate::decision::Page::FIRST });
                } else {
                    self.begin_activate(p, c, slot);
                }
            }
            Some(Choice::Yes) => {
                // Activate mana abilities (CR 605.3a, 117.1d; ENGINE.md
                // 100.4.3): an action, after which the player keeps
                // priority (605.3b; the first asked offers no `PASS`, so an
                // activation follows).
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                self.agenda.push(Task::ActivateMana { p, card: None, again: false });
            }
            Some(Choice::Companion) => {
                // A special action (CR 116.2g); the player keeps priority
                // (116.3).
                self.turn.passes = 0;
                self.agenda.push(Task::Priority);
                self.begin_companion(p);
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
        if self.players[ap as usize].hand.len() > self.spec.max_hand as usize && !self.no_max_hand(ap) {
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
