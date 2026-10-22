//! Resolution (CR 608) and the effect VM (ENGINE.md 7).

use crate::db::{AbilityRef, Kw};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::{PayMana, Task};
use crate::game::{DamageSource, Game, Pos, StackKind};
use crate::ids::{DefId, ObjRef, PlayerId, Target, Zone};
use crate::program::{Amount, Op, SpellDest, Who};

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
}

enum Flow {
    Next,
    /// Conditional ops (M4b) jump forward.
    #[allow(dead_code)]
    Jump(u16),
    /// The op pushed the frame back (and asked or pushed a sub-task).
    Suspend,
}

impl Game {
    /// Resolve the top object of the stack (CR 608.1).
    pub(crate) fn resolve_top(&mut self) {
        let item = self.stack.last().expect("resolve with an empty stack").clone();
        let Some(ar) = self.item_ability(&item) else {
            // A permanent spell: it becomes a permanent (608.3).
            self.agenda.push(Task::FinishResolve { id: item.id });
            return;
        };
        let ab = self.db.ability(ar);
        let mut legal = 0u64;
        for (i, &(req, t)) in item.targets.iter().enumerate() {
            if self.target_legal(&ab.targets[req as usize], t, item.controller) {
                legal |= 1 << i;
            }
        }
        self.agenda.push(Task::FinishResolve { id: item.id });
        if !item.targets.is_empty() && legal == 0 {
            // CR 608.2b: every target is illegal; it doesn't resolve.
            self.note(|_| "fizzles".to_string());
            return;
        }
        let source = self.item_source(&item);
        let source_def = match item.kind {
            StackKind::Spell { card } => self.objs[card.i()].def,
            StackKind::Ability { ability, .. } => ability.def,
        };
        self.agenda.push(Task::Run(Frame {
            ability: ar,
            pc: 0,
            source,
            source_def,
            controller: item.controller,
            targets: item.targets.clone(),
            legal,
            i: 0,
        }));
    }

    /// The last step of resolution (608.2n, 608.3): the item leaves the
    /// stack; then the active player receives priority (117.3b).
    pub(crate) fn finish_resolve(&mut self, id: u32) {
        if let Some(pos) = self.item_index(id) {
            let item = self.stack.remove(pos);
            if let StackKind::Spell { card } = item.kind {
                if self.objs[card.i()].zone == Zone::Stack {
                    if self.def_of(card).is_permanent_card() {
                        self.put_onto_battlefield(card, item.controller);
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
            match self.exec(&mut f, op) {
                Flow::Next => {
                    f.pc += 1;
                    f.i = 0;
                }
                Flow::Jump(pc) => {
                    f.pc = pc;
                    f.i = 0;
                }
                Flow::Suspend => return,
            }
        }
    }

    /// Targets of requirement `req` that are still legal (608.2b) and exist.
    fn targets_of(&self, f: &Frame, req: u8) -> Vec<Target> {
        f.targets
            .iter()
            .enumerate()
            .filter(|&(i, &(r, t))| {
                r == req
                    && f.legal & (1 << i) != 0
                    && match t {
                        Target::Obj(o) => self.live(o),
                        Target::Player(_) => true,
                    }
            })
            .map(|(_, &(_, t))| t)
            .collect()
    }

    /// The player `who` names, if any (an illegal target names nobody).
    fn who(&self, f: &Frame, who: Who) -> Option<PlayerId> {
        match who {
            Who::You => Some(f.controller),
            Who::TargetPlayer(i) => self.targets_of(f, i).into_iter().find_map(|t| match t {
                Target::Player(p) => Some(p),
                Target::Obj(_) => None,
            }),
            Who::ControllerOfTarget(i) => self.targets_of(f, i).into_iter().find_map(|t| match t {
                Target::Obj(o) => Some(self.objs[o.idx.i()].controller),
                Target::Player(_) => None,
            }),
        }
    }

    fn amount(&self, _f: &Frame, a: Amount) -> i32 {
        match a {
            Amount::Fixed(n) => n,
        }
    }

    /// The damage source of a resolving spell or ability: the spell itself,
    /// or the ability's source (last known information).
    fn frame_damage_source(&self, f: &Frame) -> DamageSource {
        let kws = self.db.def(f.source_def).keywords;
        DamageSource {
            controller: f.controller,
            deathtouch: kws.has(Kw::Deathtouch),
            lifelink: kws.has(Kw::Lifelink),
        }
    }

    fn exec(&mut self, f: &mut Frame, op: &Op) -> Flow {
        match *op {
            Op::DealDamage { amount, to } => {
                let n = self.amount(f, amount);
                let src = self.frame_damage_source(f);
                for t in self.targets_of(f, to) {
                    self.deal_damage(src, t, n);
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
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone == Zone::Stack {
                            self.note(|g| format!("counters {}", g.name(o.idx)));
                            match dest {
                                SpellDest::Graveyard => self.move_card(o.idx, Zone::Graveyard, Pos::Top),
                                SpellDest::LibraryTop => self.move_card(o.idx, Zone::Library, Pos::Top),
                            };
                        }
                    }
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
                        self.agenda.push(Task::Run(back));
                        self.agenda.push(Task::PayMana(PayMana {
                            payer: p,
                            cost,
                            chosen: Vec::new(),
                            min_class: 0,
                            pooled: false,
                        }));
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
                if let Some(p) = self.who(f, who) {
                    for _ in 0..self.amount(f, n) {
                        let hand = &self.players[p as usize].hand;
                        if hand.is_empty() {
                            break;
                        }
                        // The hand's canonical order makes the pick independent
                        // of insertion order; the reveal key names the card.
                        let sorted = self.hand_sorted(p);
                        let c = sorted[self.rng.below(sorted.len() as u32) as usize];
                        self.move_card(c, Zone::Graveyard, Pos::Top);
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
        }
    }

    /// Destroy a permanent (CR 701.7): to its owner's graveyard
    /// (indestructible and regeneration hook in here later).
    pub(crate) fn destroy(&mut self, c: crate::ids::CardIdx) {
        self.move_card(c, Zone::Graveyard, Pos::Top);
    }
}
