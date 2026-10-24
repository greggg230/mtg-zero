//! Resolution (CR 608) and the effect VM (ENGINE.md 7).

use crate::db::{AbilityRef, Kw};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::pay::PayMana;
use crate::game::{DamageSource, Game, Pos, StackKind};
use crate::ids::{CardIdx, DefId, ObjRef, PlayerId, Target, Zone};
use crate::program::{Amount, Op, PileDest, SearchDest, SpellDest, Who, PILE_ANY};

/// A running program (ENGINE.md 7). Plain data: cloning the game clones a
/// resolution suspended at `pc`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Frame {
    pub ability: AbilityRef,
    pub pc: u16,
    pub source: ObjRef,
    pub source_def: DefId,
    pub controller: PlayerId,
    /// (requirement index, target), as chosen.
    pub targets: Vec<(u8, Target)>,
    /// Bit i: `targets[i]` was legal when resolution started (608.2b).
    pub legal: u64,
    /// Per-op loop counter (reset when `pc` moves).
    pub i: u8,
    /// Last known information remembered by `Op::Remember`: (power,
    /// controller).
    pub lki: Option<(i32, PlayerId)>,
    /// The cards looked at by `Op::LookTop` not yet placed.
    pub pile: Vec<CardIdx>,
    /// Per-op floor (reset when `pc` moves): the smallest definition the
    /// next pick of a set may take (each set chosen once).
    pub floor: DefId,
    /// A triggered ability's event ("that player", "that much").
    pub ev: crate::triggers::EvCtx,
    /// A number an op remembered for later ops (a revealed card's mana
    /// value).
    pub x: i32,
    /// Permanents chosen so far by the current `Ext::Choose`.
    pub picked: Vec<CardIdx>,
    /// The token the last `CreateToken` made.
    pub made: Option<CardIdx>,
    /// The spell was cast for an alternative cost (CR 118.9).
    pub alt: bool,
    /// Divided damage per target of the divided requirement (CR 601.2d).
    pub division: Vec<u8>,
    /// The spell's X (CR 107.3).
    pub spell_x: u8,
    /// Every token the last `CreateToken` made ("they gain haste").
    pub made_all: Vec<CardIdx>,
    /// What a creature put onto the battlefield attacking attacks
    /// (ninjutsu, from the stack item).
    pub attack_as: Option<Option<ObjRef>>,
    /// The objects the stack item noted (`StackItem::noted`).
    pub noted: Vec<ObjRef>,
    /// The source's transformations when the ability was put on the stack
    /// (`StackItem::flips`, CR 701.27f).
    pub flips: u8,
}

pub(crate) enum Flow {
    Next,
    /// Control flow jumps forward.
    Jump(u16),
    /// The op pushed the frame back (and asked or pushed a sub-task).
    Suspend,
}

impl Game {
    /// Resolve the top object of the stack (CR 608.1).
    pub(crate) fn resolve_top(&mut self) {
        let item = self.stack.last().expect("resolve with an empty stack").clone();
        if let StackKind::Spell { card } = item.kind {
            if self.def_of(card).bestowed {
                // A bestowed Aura spell (CR 702.103e, 608.3b): with an illegal
                // target it ceases to be bestowed and resolves as a creature
                // spell; else it enters attached to its target (608.3c).
                let ctx = crate::cast::Ctx { controller: item.controller, source: Some(self.obj_ref(card)) };
                let legal = match (self.item_ability(&item), item.targets.first()) {
                    (Some(ar), Some(&(req, t))) => self.target_legal(self.db.ability(ar).reqs(&item.modes)[req as usize], t, &ctx),
                    _ => false,
                };
                if !legal {
                    let front = self.def_of(card).card_def(self.objs[card.i()].def);
                    self.objs[card.i()].def = front;
                    self.note(|g| format!("{} is no longer bestowed", g.name(card)));
                }
                self.agenda.push(Task::FinishResolve { id: item.id });
                return;
            }
        }
        let Some(ar) = self.item_ability(&item) else {
            // A permanent spell: it becomes a permanent (608.3).
            self.agenda.push(Task::FinishResolve { id: item.id });
            if let StackKind::Spell { card } = item.kind {
                if self.def_of(card).discard_land_to_enter {
                    self.agenda.push(Task::EnterByDiscard { id: item.id });
                }
            }
            return;
        };
        let ab = self.db.ability(ar);
        let reqs = ab.reqs(&item.modes);
        let ctx = crate::cast::Ctx { controller: item.controller, source: Some(self.item_source(&item)) };
        let mut legal = 0u64;
        for (i, &(req, t)) in item.targets.iter().enumerate() {
            if self.target_legal(reqs[req as usize], t, &ctx) {
                legal |= 1 << i;
            }
        }
        if !item.targets.is_empty() && legal == 0 {
            // CR 608.2b: every target is illegal; it doesn't resolve (a
            // spell goes to its owner's graveyard: an Adventure too, 715.3d
            // being about resolving).
            self.note(|_| "fizzles".to_string());
            if let StackKind::Spell { card } = item.kind {
                // (Leaving the stack removes its item.)
                self.move_card(card, Zone::Graveyard, Pos::Top);
                self.turn.priority = self.turn.active;
                self.turn.passes = 0;
            } else {
                self.agenda.push(Task::FinishResolve { id: item.id });
            }
            return;
        }
        self.agenda.push(Task::FinishResolve { id: item.id });
        if !self.intervening_holds(ar, item.controller) {
            self.note(|_| "intervening if: does nothing".to_string());
            return; // CR 603.4
        }
        let source = self.item_source(&item);
        let source_def = match item.kind {
            StackKind::Spell { card } => self.objs[card.i()].def,
            StackKind::Ability { ability, .. } => ability.def,
        };
        let frame = |ability: AbilityRef, targets: Vec<(u8, Target)>, legal: u64| Frame {
            ability,
            pc: 0,
            source,
            source_def,
            controller: item.controller,
            targets,
            legal,
            i: 0,
            lki: None,
            pile: Vec::new(),
            floor: 0,
            ev: item.ev,
            x: 0,
            picked: Vec::new(),
            made: None,
            alt: item.alt > 0,
            division: item.division.clone(),
            spell_x: item.x,
            made_all: Vec::new(),
            attack_as: item.attack_as,
            noted: item.noted.clone(),
            flips: item.flips,
        };
        let Some(modes) = &ab.modes else {
            self.agenda.push(Task::Run(frame(ar, item.targets.clone(), legal)));
            return;
        };
        // A modal ability: each chosen mode instance runs in turn, in the
        // order chosen (non-decreasing, the order printed), with its own
        // targets (CR 700.2).
        let mut frames = Vec::with_capacity(item.modes.len());
        let mut offset = 0u8;
        for &m in &item.modes {
            let n = modes.list[m as usize].targets.len() as u8;
            let mut ts = Vec::new();
            let mut bits = 0u64;
            for (i, &(req, t)) in item.targets.iter().enumerate() {
                if (offset..offset + n).contains(&req) {
                    if legal & (1 << i) != 0 {
                        bits |= 1 << ts.len();
                    }
                    ts.push((req - offset, t));
                }
            }
            frames.push(frame(AbilityRef { mode: Some(m), ..ar }, ts, bits));
            offset += n;
        }
        for f in frames.into_iter().rev() {
            self.agenda.push(Task::Run(f));
        }
    }

    /// The last step of resolution (608.2n, 608.3): the item leaves the
    /// stack; then the active player receives priority (117.3b).
    pub(crate) fn finish_resolve(&mut self, id: u32) {
        if let Some(pos) = self.item_index(id) {
            let item = self.stack.remove(pos);
            if let StackKind::Spell { card } = item.kind {
                self.note_spell_lki(card, &item);
                if self.objs[card.i()].zone == Zone::Stack {
                    if self.def_of(card).discard_land_to_enter && !item.discarded_land {
                        self.move_card(card, Zone::Graveyard, Pos::Top);
                    } else if self.def_of(card).is_permanent_card() {
                        self.entering_cast = true;
                        let alt = (item.alt > 0).then(|| &self.def_of(card).alt_costs[item.alt as usize - 1]);
                        self.entering_evoked = alt.is_some_and(|a| a.evoke);
                        self.entering_dash = alt.and_then(|a| a.dash);
                        self.entering_warp = alt.is_some_and(|a| a.warp);
                        self.entering_escaped = item.from == crate::game::CastFrom::Escape;
                        self.entering_x = item.x;
                        self.entering_times = item.times;
                        let bestowed = self.def_of(card).bestowed;
                        // Cast with Serra Paragon's permission: the permanent
                        // gains its trigger.
                        let grant = if self.marks.is_empty() {
                            None
                        } else {
                            let r = self.obj_ref(card);
                            self.marks.iter().find_map(|&(o, m)| match m {
                                crate::game::Mark::GrantOnEnter { def, slot } if o == r => Some((def, slot)),
                                _ => None,
                            })
                        };
                        let entered = self.put_onto_battlefield(card, item.controller);
                        if let Some((def, slot)) = grant {
                            if self.live(entered) && self.objs[card.i()].zone == Zone::Battlefield {
                                self.marks.push((entered, crate::game::Mark::LeaveTrigger { def, slot, exile: false }));
                            }
                        }
                        // CR 608.3c: a bestowed Aura enters attached to the
                        // creature it targeted (checked as it began
                        // resolving).
                        if bestowed && self.objs[card.i()].zone == Zone::Battlefield {
                            if let Some(&(_, Target::Obj(h))) = item.targets.first() {
                                self.objs[card.i()].attached = Some(h);
                            }
                        }
                    } else if self.def_of(card).buyback && item.cost_option == 1 {
                        // CR 702.27a: bought back.
                        self.move_card(card, Zone::Hand, Pos::Top);
                    } else if self.is_adventure(self.objs[card.i()].def) && !self.objs[card.i()].flashback {
                        // CR 715.3d: an Adventure is exiled as it resolves;
                        // its controller may cast the card (not as an
                        // Adventure) while it stays exiled.
                        let r = self.move_card(card, Zone::Exile, Pos::Top);
                        self.note(|g| format!("{} goes on an adventure", g.name(card)));
                        self.permissions.push(crate::game::Permission {
                            card: r,
                            player: item.controller,
                            turn: None,
                            kind: crate::game::PermKind::Adventure,
                        });
                    } else {
                        self.move_card(card, Zone::Graveyard, Pos::Top);
                    }
                }
            }
        }
        self.turn.priority = self.turn.active;
        self.turn.passes = 0;
    }

    pub(crate) fn run_frame(&mut self, mut f: Frame) {
        let db = self.db;
        let prog = &db.ability(f.ability).program;
        while (f.pc as usize) < prog.len() {
            let op = &prog[f.pc as usize];
            let queued = self.agenda.len();
            match self.exec(&mut f, op) {
                Flow::Next => {
                    f.pc += 1;
                    f.i = 0;
                    f.floor = 0;
                }
                Flow::Jump(pc) => {
                    f.pc = pc;
                    f.i = 0;
                    f.floor = 0;
                }
                Flow::Suspend => return,
            }
            if self.agenda.len() > queued {
                // The op queued a task (an "as this enters" choice of a
                // permanent it put onto the battlefield, CR 614.12): it runs
                // before the rest of the program.
                let tasks = self.agenda.split_off(queued);
                self.agenda.push(Task::Run(f));
                self.agenda.extend(tasks);
                return;
            }
        }
    }

    /// Targets of requirement `req` that are still legal (608.2b) and exist.
    pub(crate) fn targets_of(&self, f: &Frame, req: u8) -> Vec<Target> {
        f.targets
            .iter()
            .enumerate()
            .filter(|&(i, &(r, t))| {
                r == req
                    && f.legal & (1 << i) != 0
                    && match t {
                        Target::Obj(o) => self.live(o),
                        Target::Player(_) => true,
                        Target::Stack(id) => self.item_index(id).is_some(),
                    }
            })
            .map(|(_, &(_, t))| t)
            .collect()
    }

    /// The player `who` names, if any (an illegal target names nobody).
    pub(crate) fn who(&self, f: &Frame, who: Who) -> Option<PlayerId> {
        match who {
            Who::You => Some(f.controller),
            Who::TargetPlayer(i) => self.targets_of(f, i).into_iter().find_map(|t| match t {
                Target::Player(p) => Some(p),
                Target::Obj(_) | Target::Stack(_) => None,
            }),
            Who::ControllerOfTarget(i) => self.targets_of(f, i).into_iter().find_map(|t| match t {
                Target::Obj(o) => Some(self.objs[o.idx.i()].controller),
                Target::Stack(id) => self.stack_item(id).map(|s| s.controller),
                Target::Player(_) => None,
            }),
            Who::LkiController => f.lki.map(|l| l.1),
            Who::Active => Some(self.turn.active),
            Who::NonActive => Some(crate::ids::opp(self.turn.active)),
            Who::Opponent => Some(crate::ids::opp(f.controller)),
            Who::EventPlayer => f.ev.player,
            Who::TargetOrItsController(i) => self.targets_of(f, i).into_iter().next().and_then(|t| match t {
                Target::Player(p) => Some(p),
                Target::Obj(o) => Some(self.objs[o.idx.i()].controller),
                Target::Stack(id) => self.stack_item(id).map(|s| s.controller),
            }),
            Who::NotLkiController => f.lki.map(|l| crate::ids::opp(l.1)),
        }
    }

    pub(crate) fn amount(&self, f: &Frame, a: Amount) -> i32 {
        match a {
            Amount::Fixed(n) => n,
            Amount::LkiPower => f.lki.map_or(0, |l| l.0),
            Amount::EventAmount => f.ev.amount as i32,
            Amount::SourcePower => {
                if self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield {
                    self.power(f.source.idx)
                } else {
                    self.db.def(f.source_def).power.unwrap_or(0)
                }
            }
            Amount::LkiManaValue => f.x,
            Amount::Devotion(color) => self.devotion(f.controller, color) as i32,
            Amount::X => f.spell_x as i32,
            Amount::XTimes(k) => f.spell_x as i32 * k as i32,
            Amount::CountTypes(bits) => {
                self.permanents_of(f.controller).filter(|&c| self.types_of(c) & bits != 0).count() as i32
            }
            Amount::EventObjPower => match f.ev.obj {
                Some(r) if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield => self.power(r.idx),
                Some(r) => match self.objs[r.idx.i()].lki_power {
                    Some((inc, p)) if inc == r.inc => p,
                    _ => 0,
                },
                None => 0,
            },
            Amount::SourceCounters(k) => {
                if self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield {
                    self.counters_on(f.source.idx, k) as i32
                } else {
                    self.lki_counters.iter().filter(|x| x.0 == f.source && x.1 == k).map(|x| x.2 as i32).sum()
                }
            }
            Amount::OpponentsLostLife => {
                self.players[crate::ids::opp(f.controller) as usize].lost_life as i32
            }
            Amount::Remembered => f.x,
            Amount::HalfLibraryUp => (self.players[f.controller as usize].library.len() as i32 + 1) / 2,
            Amount::EventTimes => ((f.ev.amount >> 1) & 0xFF) as i32,
            Amount::EventSpellTimes => self.event_spell_times(f) as i32,
            Amount::Attacking => self.battlefield.iter().filter(|&&c| self.objs[c.i()].attacking).count() as i32,
            Amount::Experience => self.players[f.controller as usize].experience as i32,
            Amount::CostMvPlus(n) => n as i32 + f.ev.amount as i32,
            Amount::EventSpellMv => match f.ev.obj {
                Some(r) if self.live(r) && self.objs[r.idx.i()].zone == Zone::Stack => self.mana_value_of(r.idx) as i32,
                Some(r) => self
                    .spell_lki
                    .iter()
                    .find(|x| x.0 == r)
                    .map_or(0, |x| self.db.def(x.1).mana_value() as i32 + x.2.x as i32 * self.db.def(x.1).x_pips as i32),
                None => 0,
            },
        }
    }

    /// The damage source of a resolving spell or ability: the spell itself,
    /// or the ability's source (last known information).
    pub(crate) fn frame_damage_source(&self, f: &Frame) -> DamageSource {
        if self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield {
            // An ability of a permanent: its current characteristics.
            return DamageSource { controller: f.controller, ..self.damage_source_of(f.source.idx) };
        }
        // A spell, or a source that left (last known information: printed).
        let d = self.db.def(f.source_def);
        DamageSource {
            controller: f.controller,
            deathtouch: d.keywords.has(Kw::Deathtouch),
            lifelink: d.keywords.has(Kw::Lifelink),
            colors: d.colors | if self.marks.is_empty() { 0 } else { self.painted() },
            types: d.types,
            obj: Some(f.source),
        }
    }

    fn exec(&mut self, f: &mut Frame, op: &Op) -> Flow {
        match *op {
            Op::Ext(ref e) => self.exec_ext(f, e),
            Op::DealDamage { amount, to } => {
                let n = self.amount(f, amount);
                let src = self.frame_damage_source(f);
                for t in self.targets_of(f, to) {
                    self.deal_damage(src, t, n);
                }
                Flow::Next
            }
            Op::DamageAll { amount, ref filter } => {
                let n = self.amount(f, amount);
                let src = self.frame_damage_source(f);
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let all: Vec<CardIdx> =
                    self.battlefield.iter().copied().filter(|&c| self.card_matches(filter, c, &ctx)).collect();
                for c in all {
                    let t = Target::Obj(self.obj_ref(c));
                    self.deal_damage(src, t, n);
                }
                Flow::Next
            }
            Op::DamagePlayer { amount, who } => {
                if let Some(p) = self.who(f, who) {
                    let n = self.amount(f, amount);
                    let src = self.frame_damage_source(f);
                    self.deal_damage(src, Target::Player(p), n);
                }
                Flow::Next
            }
            Op::Draw { who, n } => {
                if let Some(p) = self.who(f, who) {
                    for _ in 0..self.amount(f, n) {
                        self.draw(p);
                    }
                }
                Flow::Next
            }
            Op::GainLife { who, n } => {
                if let Some(p) = self.who(f, who) {
                    let n = self.amount(f, n);
                    self.gain_life(p, n);
                }
                Flow::Next
            }
            Op::AddMana { who, mana } => {
                if let Some(p) = self.who(f, who) {
                    for (i, n) in mana.iter().enumerate() {
                        self.players[p as usize].pool.amt[i] += n;
                    }
                }
                Flow::Next
            }
            Op::Counter { tgt, dest } => {
                for t in self.targets_of(f, tgt) {
                    self.counter(t, dest);
                }
                Flow::Next
            }
            Op::OfferPay { who, cost, if_paid } => {
                let Some(p) = self.who(f, who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        self.note(|_| format!("P{p} pays {cost}"));
                        let mut back = f.clone();
                        back.pc = if_paid;
                        back.i = 0;
                        back.floor = 0;
                        self.agenda.push(Task::Run(back));
                        self.agenda.push(Task::PayMana(PayMana::new(p, cost)));
                        Flow::Suspend
                    }
                    Some(_) => Flow::Next,
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
            Op::PutFromHandOnTop { who, n } => {
                let Some(p) = self.who(f, who) else { return Flow::Next };
                if let Some(ch) = self.answer.take() {
                    let Choice::Card(c) = ch else { unreachable!("hand card answer {ch:?}") };
                    // Which hand card goes back is private (ENGINE.md 10).
                    self.scramble(p, &[], true, false);
                    self.move_card(c, Zone::Library, Pos::Top);
                    f.i += 1;
                }
                if f.i >= n || self.players[p as usize].hand.is_empty() {
                    return Flow::Next;
                }
                let opts = self.hand_options(p);
                self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseHandCard, opts);
                Flow::Suspend
            }
            Op::DiscardRandom { who, n } => {
                // (Tamiyo, Collector of Tales: an opponent's effect can't
                // make this player discard.)
                if let Some(p) = self.who(f, who).filter(|&p| !self.shielded_from(p, f.controller)) {
                    for _ in 0..self.amount(f, n) {
                        let hand = &self.players[p as usize].hand;
                        if hand.is_empty() {
                            break;
                        }
                        // The hand's canonical order makes the pick independent
                        // of insertion order; the reveal key names the card.
                        let sorted = self.hand_sorted(p);
                        let c = match self.forced_hand_pick(p, &sorted) {
                            Some(c) => c,
                            None => sorted[self.rng.below(sorted.len() as u32) as usize],
                        };
                        self.discard(c);
                    }
                }
                Flow::Next
            }
            Op::Destroy { tgt } => {
                for t in self.targets_of(f, tgt) {
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone == Zone::Battlefield {
                            self.destroy(o.idx);
                        }
                    }
                }
                Flow::Next
            }
            Op::Exile { tgt } => {
                for t in self.targets_of(f, tgt) {
                    if let Target::Obj(o) = t {
                        self.move_card(o.idx, Zone::Exile, Pos::Top);
                    }
                }
                Flow::Next
            }
            Op::ExtraTurn { who } => {
                if let Some(p) = self.who(f, who) {
                    self.extra_turns.push(p);
                }
                Flow::Next
            }
            Op::ReturnToHand { tgt } => {
                for t in self.targets_of(f, tgt) {
                    if let Target::Obj(o) = t {
                        if matches!(self.objs[o.idx.i()].zone, Zone::Battlefield | Zone::Stack | Zone::Graveyard) {
                            self.move_card(o.idx, Zone::Hand, Pos::Top);
                        }
                    }
                }
                Flow::Next
            }
            Op::Untap { tgt } => {
                for t in self.targets_of(f, tgt) {
                    if let Target::Obj(o) = t {
                        self.untap(o.idx);
                    }
                }
                Flow::Next
            }
            Op::Discard { who, n } => {
                let Some(p) = self.who(f, who) else { return Flow::Next };
                if self.shielded_from(p, f.controller) {
                    return Flow::Next;
                }
                if let Some(ch) = self.answer.take() {
                    let Choice::Card(c) = ch else { unreachable!("discard answer {ch:?}") };
                    self.discard(c);
                    f.i += 1;
                }
                if f.i >= n || self.players[p as usize].hand.is_empty() {
                    return Flow::Next;
                }
                let opts = self.hand_options(p);
                self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                Flow::Suspend
            }
            Op::DiscardHand { who } => {
                if let Some(p) = self.who(f, who).filter(|&p| !self.shielded_from(p, f.controller)) {
                    for c in self.hand_sorted(p) {
                        self.discard(c);
                    }
                }
                Flow::Next
            }
            Op::LoseLife { who, n } => {
                if let Some(p) = self.who(f, who) {
                    let n = self.amount(f, n);
                    self.lose_life(p, n);
                }
                Flow::Next
            }
            Op::Remember { tgt } => {
                f.lki = self.targets_of(f, tgt).into_iter().find_map(|t| match t {
                    Target::Obj(o) => Some((self.power(o.idx), self.objs[o.idx.i()].controller)),
                    // An ability: its controller ("its controller draws a
                    // card", after it is countered).
                    Target::Stack(id) => self.stack_item(id).map(|s| (0, s.controller)),
                    Target::Player(_) => None,
                });
                Flow::Next
            }
            Op::DestroyAll { ref filter } => {
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let all: Vec<crate::ids::CardIdx> =
                    self.battlefield.iter().copied().filter(|&c| self.card_matches(filter, c, &ctx)).collect();
                self.begin_batch();
                for c in all {
                    self.destroy(c);
                }
                self.end_batch();
                Flow::Next
            }
            Op::PutInLibrary { tgt, depth } => {
                for t in self.targets_of(f, tgt) {
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone == Zone::Battlefield {
                            self.move_card(o.idx, Zone::Library, Pos::FromTop(depth));
                        }
                    }
                }
                Flow::Next
            }
            Op::AddManaChoice { who, ref prod } => {
                let Some(p) = self.who(f, who) else { return Flow::Next };
                let prods = crate::pay::productions(self, p, prod, Some(f.source.idx));
                match self.answer.take() {
                    Some(Choice::Number(k)) => {
                        self.add_mana(p, &prods[k as usize]);
                        Flow::Next
                    }
                    Some(other) => unreachable!("mana answer {other:?}"),
                    None => {
                        let opts = (0..prods.len())
                            .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) })
                            .collect();
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseMana, opts);
                        Flow::Suspend
                    }
                }
            }
            Op::Jump { to } => Flow::Jump(to),
            Op::JumpUnless { ref cond, to } => {
                if self.cond_holds_from(cond, f.controller, Some(f.source)) {
                    Flow::Next
                } else {
                    Flow::Jump(to)
                }
            }
            Op::JumpUnlessTarget { tgt, ref filter, to } => {
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let ts = self.targets_of(f, tgt);
                if !ts.is_empty() && ts.iter().all(|&t| self.filter_ok(filter, t, &ctx)) {
                    Flow::Next
                } else {
                    Flow::Jump(to)
                }
            }
            Op::Search { who, ref filter, dest, reveal, may_fail } => {
                let Some(p) = self.who(f, who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Def(d)) => self.finish_search(p, d, dest, reveal),
                    Some(Choice::Pass) => {
                        self.note(|_| format!("P{p} finds nothing"));
                        self.shuffle_library(p);
                    }
                    Some(other) => unreachable!("search answer {other:?}"),
                    None => {
                        // "You" in the filter is the searcher.
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                        let opts = self.search_options(p, filter, may_fail, &ctx);
                        self.ask(Task::Run(f.clone()), p, DecisionKind::SearchLibrary, opts);
                        return Flow::Suspend;
                    }
                }
                Flow::Next
            }
            Op::LookTop { who, n, reveal } => {
                if let Some(p) = self.who(f, who) {
                    self.look_top(f, p, n, reveal);
                }
                Flow::Next
            }
            Op::PileTo { who, dest, ref filter, min, max, rest, reveal } => {
                let Some(p) = self.who(f, who) else { return Flow::Next };
                let set = matches!(dest, PileDest::Graveyard | PileDest::Hand);
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        f.pile.retain(|&x| x != c);
                        f.i += 1;
                        if set {
                            f.floor = self.objs[c.i()].def;
                        }
                        self.put_from_pile(p, c, dest, reveal);
                    }
                    Some(Choice::Pass) => return Flow::Next,
                    Some(other) => unreachable!("pile answer {other:?}"),
                    None => {}
                }
                if rest && f.pile.is_empty() || !rest && max != PILE_ANY && f.i >= max {
                    return Flow::Next;
                }
                if rest && set {
                    // "The rest into your graveyard": no choice, canonical order.
                    let mut all = std::mem::take(&mut f.pile);
                    all.sort_by_key(|c| (self.objs[c.i()].def, c.0));
                    for c in all {
                        self.put_from_pile(p, c, dest, reveal);
                    }
                    return Flow::Next;
                }
                let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                let mut sorted = f.pile.clone();
                sorted.sort_by_key(|c| (self.objs[c.i()].def, c.0));
                // The cards that may be picked now (a set is picked in
                // non-decreasing definition order, so each set once).
                let eligible: Vec<DefId> = sorted
                    .iter()
                    .filter(|&&c| rest || (self.objs[c.i()].def >= f.floor && self.card_matches(filter, c, &ctx)))
                    .map(|c| self.objs[c.i()].def)
                    .collect();
                // Picks still required: as many as possible up to `min`.
                let need = if rest { 0 } else { (min as usize).saturating_sub(f.i as usize).min(eligible.len()) };
                let mut opts = Vec::new();
                for (k, &c) in sorted.iter().enumerate() {
                    let d = self.objs[c.i()].def;
                    if k > 0 && self.objs[sorted[k - 1].i()].def == d {
                        continue;
                    }
                    let Some(first) = eligible.iter().position(|&x| x == d) else { continue };
                    // A set pick must leave enough later picks for `need`.
                    if set && eligible.len() - first < need {
                        continue;
                    }
                    assert!((k as u16) < act::LIBRARY_N, "pile slot {k} out of range");
                    opts.push(Opt { action: act::LIBRARY + k as u16, choice: Choice::Card(c) });
                }
                if !rest && need == 0 {
                    if f.pile.is_empty() {
                        return Flow::Next;
                    }
                    // Even with no card to pick, ask (pass only): whether a
                    // card matches is hidden from the other player, whose
                    // determinize must find the same decision pending.
                    opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                }
                let kind = match dest {
                    PileDest::Top => DecisionKind::PutOnTop,
                    PileDest::Bottom => DecisionKind::PutOnBottom,
                    PileDest::Graveyard => DecisionKind::PutInGraveyard,
                    PileDest::Hand => DecisionKind::PutInHand,
                };
                self.ask(Task::Run(f.clone()), p, kind, opts);
                Flow::Suspend
            }
            Op::Shuffle { who } => {
                if let Some(p) = self.who(f, who) {
                    self.shuffle_library(p);
                }
                Flow::Next
            }
            Op::CreateToken { token, n, tapped } => {
                let def = self.db.token_def(token);
                let made = self.create_tokens(def, f.controller, n as u32, tapped);
                f.made = made.first().copied();
                f.made_all = made;
                Flow::Next
            }
            Op::Sacrifice { who, ref filter } => {
                let Some(p) = self.who(f, who) else { return Flow::Next };
                if self.shielded_from(p, f.controller) {
                    return Flow::Next;
                }
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} sacrifices {}", g.name(c)));
                        self.sacrifice(c);
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
            Op::ShuffleIntoLibrary { who, hand, graveyard } => {
                if let Some(p) = self.who(f, who) {
                    let pl = &self.players[p as usize];
                    let mut cards: Vec<CardIdx> = Vec::new();
                    if hand {
                        cards.extend(self.hand_sorted(p));
                    }
                    if graveyard {
                        cards.extend(pl.graveyard.iter().copied());
                    }
                    for c in cards {
                        self.move_card(c, Zone::Library, Pos::Top);
                    }
                    self.shuffle_library(p);
                }
                Flow::Next
            }
            Op::PutFromHand { who, ref filter, tapped, optional } => {
                let Some(p) = self.who(f, who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.put_onto_battlefield(c, p);
                        if tapped {
                            self.tap(c);
                        }
                        f.made = Some(c);
                        Flow::Next
                    }
                    Some(Choice::Pass) => {
                        f.made = None;
                        Flow::Next
                    }
                    Some(other) => unreachable!("put-from-hand answer {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                        let mut opts: Vec<Opt> = self
                            .hand_options(p)
                            .into_iter()
                            .filter(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(filter, c, &ctx)))
                            .collect();
                        if self.players[p as usize].hand.is_empty() {
                            return Flow::Next;
                        }
                        // With no matching card, ask anyway (pass only): the
                        // hand is hidden from the other player.
                        if optional || opts.is_empty() {
                            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseHandCard, opts);
                        Flow::Suspend
                    }
                }
            }
            Op::SourceToLibraryTop => {
                if self.live(f.source) && self.objs[f.source.idx.i()].zone == Zone::Battlefield {
                    self.move_card(f.source.idx, Zone::Library, Pos::Top);
                }
                Flow::Next
            }
            Op::RevealHand { whose, to } => {
                let Some(owner) = self.who(f, whose) else { return Flow::Next };
                let looker = match to {
                    Some(w) => match self.who(f, w) {
                        Some(p) => Some(p),
                        None => return Flow::Next,
                    },
                    None => None,
                };
                self.reveal_hand(owner, looker);
                Flow::Next
            }
            Op::ChooseDiscard { chooser, whose, ref filter, optional } => {
                // (`Frame::x`: whether a card was discarded, for "if you do".)
                f.x = 0;
                let (Some(p), Some(owner)) = (self.who(f, chooser), self.who(f, whose)) else { return Flow::Next };
                if self.shielded_from(owner, f.controller) {
                    return Flow::Next;
                }
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{owner} discards {}", g.name(c)));
                        self.discard(c);
                        f.x = 1;
                        Flow::Next
                    }
                    Some(Choice::Pass) => Flow::Next,
                    Some(other) => unreachable!("discard choice {other:?}"),
                    None => {
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
                        if opts.is_empty() {
                            return Flow::Next;
                        }
                        if optional {
                            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseDiscard, opts);
                        Flow::Suspend
                    }
                }
            }
            Op::May { who, skip } => {
                let Some(p) = self.who(f, who) else { return Flow::Jump(skip) };
                match self.answer.take() {
                    Some(Choice::Yes) => Flow::Next,
                    Some(_) => Flow::Jump(skip),
                    None => {
                        let opts = vec![
                            Opt { action: act::PASS, choice: Choice::Pass },
                            Opt { action: act::YES, choice: Choice::Yes },
                        ];
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                        Flow::Suspend
                    }
                }
            }
        }
    }

    /// `p` looks at the top `n` cards of their library (CR 701.18a,
    /// 701.42a): each is keyed for the looker (or revealed to all); the
    /// other player loses track of which is which as `p` holds them
    /// (ENGINE.md 10). They become the frame's pile.
    pub(crate) fn look_top(&mut self, f: &mut Frame, p: PlayerId, n: u8, reveal: bool) {
        let lib = &self.players[p as usize].library;
        let pile: Vec<CardIdx> = lib.iter().rev().take(n as usize).copied().collect();
        for &c in &pile {
            // A forced outcome (zero-core chance_k) swaps the identity in
            // before the card is seen (ENGINE.md 10).
            if reveal {
                self.force_library_card(c, crate::hidden::KEY_REVEAL, p);
            } else {
                self.force_library_card(c, crate::hidden::KEY_LOOK, p);
            }
            if reveal {
                self.reveal(c);
            } else {
                self.learn(p, c);
                let d = self.objs[c.i()].def;
                self.emit_private(crate::hidden::KEY_LOOK, p, d as u64);
            }
        }
        if !reveal {
            self.scramble(p, &pile, false, false);
        }
        self.note(|_| format!("P{p} looks at {} card(s)", pile.len()));
        f.pile = pile;
    }

    /// `owner`'s hand is revealed to everyone (`looker: None`) or looked at
    /// by `looker`: keys per card in definition order (CR 701.16).
    fn reveal_hand(&mut self, owner: PlayerId, looker: Option<PlayerId>) {
        for c in self.hand_sorted(owner) {
            match looker {
                None => self.reveal(c),
                Some(q) => {
                    self.learn(q, c);
                    let d = self.objs[c.i()].def;
                    self.emit_private(crate::hidden::KEY_LOOK, q, d as u64);
                }
            }
        }
        self.note(|_| format!("P{owner}'s hand is seen by {looker:?}"));
    }

    /// Move a card of the pile to `dest` (it is known to its owner).
    fn put_from_pile(&mut self, p: PlayerId, c: CardIdx, dest: PileDest, reveal: bool) {
        if reveal {
            self.reveal(c);
        }
        match dest {
            PileDest::Top => self.move_card(c, Zone::Library, Pos::Top),
            PileDest::Bottom => self.move_card(c, Zone::Library, Pos::Bottom),
            PileDest::Graveyard => self.move_card(c, Zone::Graveyard, Pos::Top),
            PileDest::Hand => self.move_card(c, Zone::Hand, Pos::Top),
        };
        if self.objs[c.i()].zone.is_hidden() {
            self.objs[c.i()].known |= 1 << p;
        }
    }

    /// Options of a library search by `p` (CR 701.19): one per definition
    /// among the matching cards, at the slot of its first card in the
    /// library sorted by definition (so the options depend only on the
    /// library's contents, which its owner always knows); `PASS` when the
    /// search may fail or nothing matches.
    pub(crate) fn search_options(&self, p: PlayerId, filter: &crate::db::Filter, may_fail: bool, ctx: &crate::cast::Ctx) -> Vec<Opt> {
        let lib = self.library_sorted(p);
        let mut opts = Vec::new();
        for (i, &c) in lib.iter().enumerate() {
            let d = self.objs[c.i()].def;
            if i > 0 && self.objs[lib[i - 1].i()].def == d {
                continue;
            }
            if self.card_matches(filter, c, ctx) {
                assert!((i as u16) < act::LIBRARY_N, "library slot {i} out of range");
                opts.push(Opt { action: act::LIBRARY + i as u16, choice: Choice::Def(d) });
            }
        }
        if may_fail || opts.is_empty() {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        opts
    }

    /// Put the found card (a card of definition `d` in `p`'s library) where
    /// the search says, then shuffle (CR 701.19, 701.20).
    fn finish_search(&mut self, p: PlayerId, d: DefId, dest: SearchDest, reveal: bool) {
        let c = self.library_sorted(p).into_iter().find(|&c| self.objs[c.i()].def == d).expect("found card");
        // Which card was taken is private (unless revealed): the other
        // player loses track of the library (ENGINE.md 10).
        self.scramble(p, &[], false, true);
        self.note(|g| format!("P{p} finds {}", g.name(c)));
        match dest {
            SearchDest::Hand => {
                self.move_card(c, Zone::Hand, Pos::Top);
                self.objs[c.i()].known |= 1 << p;
                if reveal {
                    self.reveal(c);
                }
                self.shuffle_library(p);
            }
            SearchDest::Top => {
                // "Reveal it, then shuffle and put that card on top."
                self.shuffle_library(p);
                self.move_card(c, Zone::Library, Pos::Top);
                self.objs[c.i()].known |= 1 << p;
                if reveal {
                    self.reveal(c);
                }
            }
            SearchDest::Graveyard => {
                self.move_card(c, Zone::Graveyard, Pos::Top);
                self.shuffle_library(p);
            }
            SearchDest::Battlefield { tapped } => {
                self.put_onto_battlefield(c, p);
                if tapped {
                    self.tap(c);
                }
                self.shuffle_library(p);
            }
        }
    }

    /// Counter a spell or an ability on the stack (CR 701.5a: it is removed
    /// from the stack and doesn't resolve): a spell goes to `dest` (its
    /// owner's graveyard unless the effect says otherwise), an ability
    /// ceases to exist. A spell that can't be countered stays (101.2).
    /// Returns whether it was countered.
    pub(crate) fn counter(&mut self, t: Target, dest: SpellDest) -> bool {
        match t {
            Target::Obj(o) if self.live(o) && self.objs[o.idx.i()].zone == Zone::Stack => {
                if self.spell_uncounterable(o.idx) {
                    self.note(|g| format!("{} can't be countered", g.name(o.idx)));
                    return false;
                }
                self.note(|g| format!("counters {}", g.name(o.idx)));
                match dest {
                    SpellDest::Graveyard => self.move_card(o.idx, Zone::Graveyard, Pos::Top),
                    SpellDest::LibraryTop => self.move_card(o.idx, Zone::Library, Pos::Top),
                    SpellDest::Exile => self.move_card(o.idx, Zone::Exile, Pos::Top),
                    SpellDest::Hand => self.move_card(o.idx, Zone::Hand, Pos::Top),
                };
                true
            }
            Target::Stack(id) => match self.item_index(id) {
                Some(pos) if matches!(self.stack[pos].kind, StackKind::Ability { .. }) => {
                    self.note(|g| format!("counters {}", g.describe_target(t)));
                    self.stack.remove(pos);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Can't spell `card` be countered: "this spell can't be countered"
    /// (CR 101.2), or its controller controls a permanent saying "spells you
    /// control can't be countered"?
    pub(crate) fn spell_uncounterable(&self, card: CardIdx) -> bool {
        if self.def_of(card).uncounterable {
            return true;
        }
        if !self.marks.is_empty() {
            let r = self.obj_ref(card);
            if self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::Uncounterable) {
                return true;
            }
        }
        let p = self.objs[card.i()].controller;
        self.permanents_of(p).any(|c| self.abilities(c).spells_uncounterable)
    }

    /// Destroy a permanent (CR 701.7): to its owner's graveyard
    /// (indestructible and regeneration hook in here later).
    pub(crate) fn destroy(&mut self, c: crate::ids::CardIdx) {
        if self.indestructible(c) {
            return; // CR 702.12b
        }
        self.move_card(c, Zone::Graveyard, Pos::Top);
    }
}
