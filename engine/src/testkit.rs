//! Scenario builder and helpers for tests (not used by the engine).

use crate::db::CardDb;
use crate::decision::{Choice, DecisionKind};
use crate::flow::Task;
use crate::game::{Game, GameSpec, Step};
use crate::ids::{CardIdx, PlayerId, Target, Zone};
use std::hash::{Hash, Hasher};

/// A hash of a value (for `view_of`).
fn h64<T: Hash>(x: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    x.hash(&mut h);
    h.finish()
}

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

    /// Hand cards both players know (as if revealed earlier).
    pub fn hand_known(mut self, p: PlayerId, names: &[&str]) -> Self {
        for n in names {
            let c = self.add(p, n, Zone::Hand);
            self.g.objs[c.i()].known = 0b11;
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
            if !self.g.def_of(c).statics.is_empty() {
                self.g.n_static += 1;
            }
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
        g.turn.normal = self.active;
        g.players[self.active as usize].last_turn = self.number;
        g.players[1 - self.active as usize].last_turn = self.number.saturating_sub(1);
        // Turns begun so far: the starting player has had the odd ones.
        let starting = g.turn.starting as usize;
        g.players[starting].turns = self.number.div_ceil(2);
        g.players[1 - starting].turns = self.number / 2;
        for p in &mut g.players {
            p.kept = true;
        }
        // The turn continues from `step`; the next turn starts normally.
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

    /// Answer forced decisions (one option) and pass priority until `stop`
    /// holds or a real decision other than priority is pending. Meant for
    /// games with `auto_single = false`, where it stops exactly at `stop`.
    /// Returns the chance keys.
    pub fn drive_until(&mut self, stop: impl Fn(&Game) -> bool) -> Vec<u64> {
        let mut keys = Vec::new();
        for _ in 0..100_000 {
            if stop(self) {
                return keys;
            }
            let Some(d) = &self.pending else { return keys };
            let c = if d.options.len() == 1 {
                d.options[0].choice
            } else if d.kind == DecisionKind::Priority {
                Choice::Pass
            } else {
                return keys;
            };
            keys.extend(self.choose(c));
        }
        panic!("drive_until: no progress");
    }

    /// Everything `viewer` can observe, as numbers (hidden-information
    /// tests): public zones and objects, zone sizes, life and mana, the
    /// turn, the viewer's own hand, every card the viewer knows exactly
    /// where it is (library positions; the other hand as a multiset, its
    /// cards are not told apart), the viewer's clouds, and the pending
    /// decision (its options only if the viewer decides). Two worlds a
    /// `determinize(viewer)` may produce must agree on it.
    pub fn view_of(&self, viewer: PlayerId) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::with_capacity(256);
        let t = &self.turn;
        v.extend([t.number as u64, t.step as u64, t.active as u64, t.priority as u64, t.passes as u64]);
        v.push(self.outcome.map_or(9, |o| o.winner.map_or(2, |w| w as u64)));
        let obj = |v: &mut Vec<u64>, c: CardIdx| {
            let o = &self.objs[c.i()];
            v.extend([o.def as u64, o.owner as u64, o.controller as u64, o.tapped as u64, o.damage as u64]);
            v.extend([o.attacking as u64, o.blocked as u64, o.blocking.is_some() as u64, o.escaped as u64]);
            // Counters and attachments (public, CR 122, 301.5).
            for (k, n) in self.counters_of(c) {
                v.extend([h64(&k), n as u64]);
            }
            v.push(o.attached.map_or(u64::MAX, |h| self.battlefield.iter().position(|&x| x == h.idx).unwrap_or(999) as u64));
        };
        for &c in &self.battlefield {
            obj(&mut v, c);
        }
        v.push(u64::MAX);
        for &c in &self.exile {
            obj(&mut v, c);
        }
        v.push(u64::MAX);
        // Objects are named by their public place (CardIdx identities are
        // not observable): battlefield position, stack position, their
        // definition in another public zone.
        let place = |c: CardIdx| -> u64 {
            match self.objs[c.i()].zone {
                Zone::Battlefield => self.battlefield.iter().position(|&x| x == c).unwrap() as u64,
                Zone::Stack => {
                    1000 + self
                        .stack
                        .iter()
                        .position(|s| s.kind == crate::game::StackKind::Spell { card: c })
                        .unwrap_or(99) as u64
                }
                // In a hidden zone the slot's identity is not public (an
                // ability's own definition says where it came from).
                z if z.is_hidden() => 3000,
                _ => 2000 + self.objs[c.i()].def as u64,
            }
        };
        for s in &self.stack {
            match s.kind {
                crate::game::StackKind::Spell { card } => v.extend([1, self.objs[card.i()].def as u64]),
                crate::game::StackKind::Ability { ability, source } => {
                    let slot = match ability.slot {
                        crate::db::AbilitySlot::Spell => 0,
                        crate::db::AbilitySlot::Trigger(i) => 100 + i as u64,
                        crate::db::AbilitySlot::Activated(i) => 200 + i as u64,
                        crate::db::AbilitySlot::Hand(i) => 300 + i as u64,
                    };
                    v.extend([2, ability.def as u64, slot, place(source.idx)])
                }
            }
            v.push(s.controller as u64);
            for &(r, tg) in &s.targets {
                v.push(r as u64);
                v.push(match tg {
                    Target::Player(p) => p as u64,
                    Target::Obj(o) => 100 + place(o.idx),
                });
            }
            v.extend(s.modes.iter().map(|&m| m as u64));
            v.push(u64::MAX - 1);
        }
        for w in &self.waiting {
            v.extend([w.controller as u64, w.source_def as u64, w.slot as u64]);
        }
        v.push(u64::MAX);
        // Continuous effects (public), with the objects they affect by
        // place.
        for e in &self.effects {
            v.extend([e.ts as u64, e.controller as u64, e.until as u64]);
            match &e.on {
                crate::layers::Affected::Obj(r) => v.push(500 + place(r.idx)),
                crate::layers::Affected::Group(_) => v.push(1),
            }
            v.push(h64(&e.m));
        }
        v.push(u64::MAX);
        for l in &self.links {
            v.extend([l.until as u64, l.back as u64, place(l.source.idx), place(l.card.idx)]);
        }
        v.push(u64::MAX);
        for pm in &self.permissions {
            v.extend([place(pm.card.idx), pm.player as u64, pm.turn.unwrap_or(0) as u64, h64(&pm.kind)]);
        }
        v.push(u64::MAX);
        for p in 0..2usize {
            for r in &self.players[p].attacked_with {
                v.push(if self.live(*r) { place(r.idx) } else { 7777 });
            }
            v.push(u64::MAX);
        }
        for &(r, n) in &self.resolved {
            v.extend([place(r.idx), r.inc as u64, n as u64]);
        }
        v.push(u64::MAX);
        for d in &self.delayed {
            v.extend([d.when as u64, d.controller as u64, d.source_def as u64, d.slot as u64, d.ev.amount as u64]);
            v.push(d.ev.obj.map_or(u64::MAX, |r| place(r.idx)));
        }
        v.push(u64::MAX);
        for p in 0..2u8 {
            let pl = &self.players[p as usize];
            v.extend([pl.life as i64 as u64, pl.hand.len() as u64, pl.library.len() as u64, pl.lands_played as u64]);
            v.push(pl.cant_gain_life as u64);
            v.extend(pl.pool.amt.iter().map(|&n| n as u64));
            v.extend(pl.graveyard.iter().map(|&c| self.objs[c.i()].def as u64));
            v.push(u64::MAX);
            // Library cards the viewer knows exactly, by position.
            for (i, &c) in pl.library.iter().enumerate() {
                if self.knows(viewer, c) {
                    v.extend([i as u64, self.objs[c.i()].def as u64]);
                }
            }
            v.push(u64::MAX);
            // The hand: all of it if it is the viewer's, else the multiset
            // of what the viewer knows exactly.
            let mut h: Vec<u64> = pl
                .hand
                .iter()
                .filter(|&&c| p == viewer || self.knows(viewer, c))
                .map(|&c| self.objs[c.i()].def as u64)
                .collect();
            h.sort_unstable();
            v.extend(h);
            v.push(u64::MAX);
        }
        for k in self.clouds.iter().filter(|k| k.viewer == viewer) {
            v.extend([k.owner as u64, k.hand as u64, k.library as u64]);
            let lib = &self.players[k.owner as usize].library;
            let mut pos: Vec<u64> = k
                .slots
                .iter()
                .map(|c| lib.iter().position(|x| x == c).map_or(999, |i| i as u64))
                .collect();
            pos.sort_unstable();
            v.extend(pos);
            v.extend(k.defs.iter().map(|&d| d as u64));
            v.push(u64::MAX - 2);
        }
        if let Some(d) = &self.pending {
            v.extend([d.player as u64, d.kind as u64]);
            if d.player == viewer {
                v.extend(d.options.iter().map(|o| o.action as u64));
            }
        }
        v
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
        self.clouds.hash(&mut h);
        self.tokens.hash(&mut h);
        self.observer.hash(&mut h);
        self.effects.hash(&mut h);
        self.links.hash(&mut h);
        self.delayed.hash(&mut h);
        self.resolved.hash(&mut h);
        self.permissions.hash(&mut h);
        self.counters.hash(&mut h);
        self.n_static.hash(&mut h);
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
            // A token that ceased to exist is in no zone (CR 704.5d).
            let want = (self.objs[i].zone != Zone::Nowhere) as u8;
            if n != want {
                return Err(format!("card {i} ({}) appears {n} times", self.name(CardIdx(i as u16))));
            }
            if self.objs[i].zone == Zone::Nowhere && !self.def_of(CardIdx(i as u16)).token {
                return Err(format!("card {i} is nowhere"));
            }
        }
        if !self.keys.is_empty() {
            return Err("chance keys left in the state".into());
        }
        if self.outcome.is_none() && self.pending.is_none() {
            return Err("no decision pending in a running game".into());
        }
        self.check_knowledge()?;
        let mut ts: Vec<u32> = self.battlefield.iter().map(|c| self.objs[c.i()].timestamp).collect();
        if ts.windows(2).any(|w| w[0] >= w[1]) {
            return Err("battlefield not in timestamp order".into());
        }
        ts.clear();
        Ok(())
    }
}
