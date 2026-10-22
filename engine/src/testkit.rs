//! Scenario builder and helpers for tests (not used by the engine).

use crate::db::CardDb;
use crate::decision::{Choice, DecisionKind};
use crate::flow::Task;
use crate::game::{Game, GameSpec, Step};
use crate::ids::{CardIdx, PlayerId, Target, Zone};
use std::hash::{Hash, Hasher};

/// Build a game in a chosen position: zones by card name, then start at a
/// step with its turn-based actions (e.g. `Step::Main1` -> priority).
pub struct Scenario {
    g: Game,
    step: Step,
    active: PlayerId,
    number: u32,
}

impl Scenario {
    /// No cards anywhere, player 0 active on turn 3, no mulligans,
    /// single-option decisions auto-answered, logging on.
    pub fn new(db: &'static CardDb) -> Scenario {
        let spec = GameSpec { mulligans: false, log: true, ..GameSpec::default() };
        Scenario { g: Game::empty(db, spec, 1), step: Step::Main1, active: 0, number: 3 }
    }

    pub fn spec(mut self, f: impl FnOnce(&mut GameSpec)) -> Self {
        f(&mut self.g.spec);
        self.g.log = if self.g.spec.log { Some(Vec::new()) } else { None };
        self
    }

    pub fn seed(mut self, seed: u64) -> Self {
        self.g.rng = crate::rng::Rng::new(seed);
        self
    }

    fn add(&mut self, p: PlayerId, name: &str, zone: Zone) -> CardIdx {
        let def = self.g.db.expect_id(name).unwrap_or_else(|e| panic!("{e}"));
        let c = self.g.add_card(def, p, zone);
        let known = if zone.is_hidden() { 1 << p } else { 0b11 };
        self.g.objs[c.i()].known = known;
        c
    }

    /// Library contents, `names[0]` on top.
    pub fn library(mut self, p: PlayerId, names: &[&str]) -> Self {
        for n in names.iter().rev() {
            let c = self.add(p, n, Zone::Library);
            self.g.objs[c.i()].known = 0;
            self.g.players[p as usize].library.push(c);
        }
        self
    }

    pub fn hand(mut self, p: PlayerId, names: &[&str]) -> Self {
        for n in names {
            let c = self.add(p, n, Zone::Hand);
            self.g.players[p as usize].hand.push(c);
        }
        self
    }

    /// Permanents that have been under their controller's control since
    /// before this turn (no summoning sickness).
    pub fn battlefield(mut self, p: PlayerId, names: &[&str]) -> Self {
        for n in names {
            let c = self.add(p, n, Zone::Battlefield);
            self.g.next_ts += 1;
            self.g.objs[c.i()].timestamp = self.g.next_ts;
            self.g.objs[c.i()].control_since = 0;
            self.g.battlefield.push(c);
        }
        self
    }

    pub fn graveyard(mut self, p: PlayerId, names: &[&str]) -> Self {
        for n in names {
            let c = self.add(p, n, Zone::Graveyard);
            self.g.players[p as usize].graveyard.push(c);
        }
        self
    }

    pub fn life(mut self, p: PlayerId, n: i32) -> Self {
        self.g.players[p as usize].life = n;
        self
    }

    pub fn at(mut self, step: Step, active: PlayerId) -> Self {
        self.step = step;
        self.active = active;
        self
    }

    pub fn turn_number(mut self, n: u32) -> Self {
        self.number = n;
        self
    }

    /// Start: run the step's turn-based actions, then to the first decision.
    pub fn start(mut self) -> Game {
        let g = &mut self.g;
        g.turn.number = self.number;
        g.turn.active = self.active;
        g.turn.starting = if self.number % 2 == 1 { self.active } else { 1 - self.active };
        g.turn.step = self.step;
        g.players[self.active as usize].last_turn = self.number;
        g.players[1 - self.active as usize].last_turn = self.number.saturating_sub(1);
        for p in &mut g.players {
            p.kept = true;
        }
        g.agenda.push(Task::BeginTurn);
        // The turn continues from `step`; the next turn starts normally.
        g.agenda.pop();
        g.agenda.push(Task::BeginStep);
        g.advance();
        g.keys.clear();
        self.g
    }
}

impl Game {
    /// First card named `name` owned by `owner` in `zone` (canonical order).
    pub fn find_card(&self, owner: PlayerId, name: &str, zone: Zone) -> Option<CardIdx> {
        (0..self.objs.len())
            .map(|i| CardIdx(i as u16))
            .find(|&c| self.objs[c.i()].owner == owner && self.objs[c.i()].zone == zone && self.name(c) == name)
    }

    pub fn card(&self, owner: PlayerId, name: &str, zone: Zone) -> CardIdx {
        self.find_card(owner, name, zone)
            .unwrap_or_else(|| panic!("no {name} of P{owner} in {zone:?}\n{}", self.log_lines().join("\n")))
    }

    pub fn count_in(&self, owner: PlayerId, name: &str, zone: Zone) -> usize {
        (0..self.objs.len())
            .map(|i| CardIdx(i as u16))
            .filter(|&c| self.objs[c.i()].owner == owner && self.objs[c.i()].zone == zone && self.name(c) == name)
            .count()
    }

    /// The choice "target this card's current object".
    pub fn tgt(&self, c: CardIdx) -> Choice {
        Choice::Target(Target::Obj(self.obj_ref(c)))
    }

    pub fn kind(&self) -> Option<DecisionKind> {
        self.pending.as_ref().map(|d| d.kind)
    }

    pub fn has_option(&self, c: Choice) -> bool {
        self.pending.as_ref().is_some_and(|d| d.action_of(c).is_some())
    }

    pub fn options_desc(&self) -> Vec<String> {
        match &self.pending {
            Some(d) => d.options.iter().map(|o| format!("{}={}", o.action, self.describe_choice(o.choice))).collect(),
            None => Vec::new(),
        }
    }

    /// Answer the pending decision with `c` (panics if it is not legal).
    /// Returns the chance keys.
    pub fn choose(&mut self, c: Choice) -> Vec<u64> {
        let d = self.pending.as_ref().unwrap_or_else(|| panic!("no decision; outcome {:?}", self.outcome));
        let a = d.action_of(c).unwrap_or_else(|| {
            panic!(
                "{c:?} is not an option of {:?} for P{}: {:?}\n{}",
                d.kind,
                d.player,
                self.options_desc(),
                self.log_lines().join("\n")
            )
        });
        let mut keys = Vec::new();
        self.apply(a, &mut keys).unwrap();
        keys
    }

    pub fn pass(&mut self) -> Vec<u64> {
        self.choose(Choice::Pass)
    }

    /// Pass while the pending decision is priority and `stop` is false.
    pub fn pass_until(&mut self, stop: impl Fn(&Game) -> bool) {
        for _ in 0..10_000 {
            if stop(self) || self.kind() != Some(DecisionKind::Priority) {
                return;
            }
            self.pass();
        }
        panic!("pass_until: no progress");
    }

    /// A hash of the whole mutable state (determinism checks).
    pub fn fingerprint(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.rng.hash(&mut h);
        self.objs.hash(&mut h);
        self.players.hash(&mut h);
        self.battlefield.hash(&mut h);
        self.exile.hash(&mut h);
        self.stack.hash(&mut h);
        self.turn.hash(&mut h);
        self.agenda.hash(&mut h);
        self.pending.hash(&mut h);
        self.answer.hash(&mut h);
        self.waiting.hash(&mut h);
        self.extra_turns.hash(&mut h);
        self.combat_dmg.hash(&mut h);
        self.next_ts.hash(&mut h);
        self.next_stack_id.hash(&mut h);
        self.outcome.hash(&mut h);
        self.observer.hash(&mut h);
        h.finish()
    }

    /// Check the structural invariants (fuzzer): every card is in exactly
    /// the zone list its `zone` says, once; spells on the stack have items.
    pub fn check_invariants(&self) -> Result<(), String> {
        let mut seen = vec![0u8; self.objs.len()];
        let mut mark = |c: CardIdx, z: Zone, owner: Option<PlayerId>| -> Result<(), String> {
            let o = &self.objs[c.i()];
            if o.zone != z {
                return Err(format!("card {} listed in {z:?} but zone is {:?}", c.0, o.zone));
            }
            if let Some(p) = owner {
                if o.owner != p {
                    return Err(format!("card {} in P{p}'s zone but owned by P{}", c.0, o.owner));
                }
            }
            seen[c.i()] += 1;
            Ok(())
        };
        for p in 0..2u8 {
            let pl = &self.players[p as usize];
            for &c in &pl.library {
                mark(c, Zone::Library, Some(p))?;
            }
            for &c in &pl.hand {
                mark(c, Zone::Hand, Some(p))?;
            }
            for &c in &pl.graveyard {
                mark(c, Zone::Graveyard, Some(p))?;
            }
        }
        for &c in &self.battlefield {
            mark(c, Zone::Battlefield, None)?;
        }
        for &c in &self.exile {
            mark(c, Zone::Exile, None)?;
        }
        for s in &self.stack {
            if let crate::game::StackKind::Spell { card } = s.kind {
                mark(card, Zone::Stack, None)?;
            }
        }
        for (i, &n) in seen.iter().enumerate() {
            if n != 1 {
                return Err(format!("card {i} ({}) appears {n} times", self.name(CardIdx(i as u16))));
            }
        }
        if !self.keys.is_empty() {
            return Err("chance keys left in the state".into());
        }
        if self.outcome.is_none() && self.pending.is_none() {
            return Err("no decision pending in a running game".into());
        }
        let mut ts: Vec<u32> = self.battlefield.iter().map(|c| self.objs[c.i()].timestamp).collect();
        if ts.windows(2).any(|w| w[0] >= w[1]) {
            return Err("battlefield not in timestamp order".into());
        }
        ts.clear();
        Ok(())
    }
}
