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
            if crate::layers::self_typed(self.g.def_of(c)) {
                self.g.n_self_types += 1;
            }
            // A planeswalker entered with its printed loyalty (CR 306.5b).
            if let (Some(n), true) = (self.g.def_of(c).loyalty, self.g.def_of(c).is(crate::db::types::PLANESWALKER)) {
                self.g.add_counters(c, crate::db::Counter::Loyalty, n as u16);
            }
        }
        self
    }

    /// `p`'s first permanent named `name` (its card's name) has its back
    /// face up (it entered transformed earlier, CR 712.14a: a planeswalker
    /// face with its printed loyalty).
    pub fn transformed(mut self, p: PlayerId, name: &str) -> Self {
        let c = self.g.find_card(p, name, Zone::Battlefield).unwrap_or_else(|| panic!("no {name} of P{p}"));
        let back = self.g.def_of(c).back.unwrap_or_else(|| panic!("{name} has no back face"));
        self.g.set_face(c, back);
        if let (Some(n), true) = (self.g.def_of(c).loyalty, self.g.def_of(c).is(crate::db::types::PLANESWALKER)) {
            self.g.add_counters(c, crate::db::Counter::Loyalty, n as u16);
        }
        self
    }

    /// `p` has played `n` lands this turn.
    pub fn lands_played(mut self, p: PlayerId, n: u8) -> Self {
        self.g.players[p as usize].lands_played = n;
        self
    }

    /// It is day (`Some(true)`) or night (`Some(false)`) (CR 731).
    pub fn day(mut self, day: Option<bool>) -> Self {
        self.g.day = day;
        self
    }

    /// Cards in exile, owned by `p`.
    pub fn exile(mut self, p: PlayerId, names: &[&str]) -> Self {
        for n in names {
            let c = self.add(p, n, Zone::Exile);
            self.g.exile.push(c);
        }
        self
    }

    /// An emblem `p` got earlier from the card named `card` (CR 114).
    pub fn emblem(mut self, p: PlayerId, card: &str) -> Self {
        let name = format!("{card} emblem");
        let def = self.g.db.token_by_name(&name).unwrap_or_else(|| panic!("no emblem of {card}"));
        self.g.create_emblem(def, p);
        self
    }

    /// Counters of a kind on `p`'s first permanent named `name`.
    pub fn counters(mut self, p: PlayerId, name: &str, k: crate::db::Counter, n: u16) -> Self {
        let c = self.g.find_card(p, name, Zone::Battlefield).unwrap_or_else(|| panic!("no {name} of P{p}"));
        let have = self.g.counters_on(c, k);
        self.g.remove_counters(c, k, have);
        self.g.add_counters(c, k, n);
        self
    }

    /// `p` has the initiative (CR 725).
    pub fn initiative(mut self, p: PlayerId) -> Self {
        self.g.initiative = Some(p);
        self
    }

    /// `p` has Undercity in the command zone with the venture marker on
    /// room `room` (`dungeon::ROOMS`), and has completed `completed`
    /// dungeons (CR 309).
    pub fn dungeon(mut self, p: PlayerId, room: Option<u8>, completed: u8) -> Self {
        if let Some(r) = room {
            let def = self.g.db.token_by_name(crate::dungeon::UNDERCITY).expect("Undercity");
            let c = self.g.add_card(def, p, Zone::Command);
            self.g.command.push(c);
            self.g.dungeons[p as usize].obj = Some(c);
            self.g.dungeons[p as usize].room = r;
        }
        self.g.dungeons[p as usize].completed = completed;
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

    /// A token of the token definition `name`, owned by `p`, put straight
    /// into `p`'s graveyard, as between a token dying and the next
    /// state-based check (CR 704.5d). For rules tests of "cards in a
    /// graveyard" (a token isn't a card, 108.2).
    pub fn put_token_in_graveyard(&mut self, p: PlayerId, name: &str) -> CardIdx {
        let def = self.db.token_by_name(name).unwrap_or_else(|| panic!("no token named {name}"));
        let c = self.add_card(def, p, Zone::Graveyard);
        self.objs[c.i()].known = 0b11;
        self.token_slot_made(c);
        self.players[p as usize].graveyard.push(c);
        c
    }

    /// Does condition `c` hold for `p` now (rules tests)?
    pub fn holds(&self, c: &crate::db::Cond, p: PlayerId) -> bool {
        self.cond_holds(c, p)
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
            v.extend([o.chosen_player.map_or(9, |p| p as u64), o.monstrous as u64]);
            // What it attacks (a planeswalker by its place), loyalty
            // activations this turn, when it entered (public).
            v.push(o.attacks.map_or(u64::MAX, |r| self.battlefield.iter().position(|&x| x == r.idx).unwrap_or(999) as u64));
            v.extend([o.pw_turn as u64, o.pw_used as u64, o.pw_extra as u64, o.entered as u64]);
            // Counters and attachments (public, CR 122, 301.5).
            for (k, n) in self.counters_of(c) {
                v.extend([h64(&k), n as u64]);
            }
            v.push(o.attached.map_or(u64::MAX, |h| self.battlefield.iter().position(|&x| x == h.idx).unwrap_or(999) as u64));
            // Copies (public, CR 707): what it is without copy effects, its
            // copy effects' exceptions, token-ness.
            v.extend([self.own_def(c) as u64, h64(&self.exc(c)), self.is_token(c) as u64, self.is_prepared(c) as u64]);
        };
        for &c in &self.battlefield {
            obj(&mut v, c);
        }
        v.push(u64::MAX);
        for &c in &self.exile {
            obj(&mut v, c);
        }
        v.push(u64::MAX);
        // Cards exiled face down (ENGINE.md 30), per owner: how many, and the
        // identities the viewer knows (a pile with no order to see).
        for p in 0..2u8 {
            let mine: Vec<CardIdx> = self.face_down.iter().copied().filter(|&c| self.objs[c.i()].owner == p).collect();
            let mut known: Vec<u64> = mine.iter().filter(|&&c| self.knows(viewer, c)).map(|&c| self.objs[c.i()].def as u64).collect();
            known.sort_unstable();
            v.push(mine.len() as u64);
            v.extend(known);
            v.push(u64::MAX - 3);
        }
        v.push(u64::MAX);
        // Emblems (public, CR 114).
        for &c in &self.command {
            v.extend([self.objs[c.i()].def as u64, self.objs[c.i()].controller as u64]);
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
        let live_place = |r: crate::ids::ObjRef| -> u64 { if self.live(r) { place(r.idx) } else { 7777 } };
        for s in &self.stack {
            match s.kind {
                crate::game::StackKind::Spell { card } => {
                    v.extend([1, self.objs[card.i()].def as u64, self.is_copy_obj(card) as u64, s.times as u64])
                }
                crate::game::StackKind::Ability { ability, source } => {
                    let slot = match ability.slot {
                        crate::db::AbilitySlot::Spell => 0,
                        crate::db::AbilitySlot::Trigger(i) => 100 + i as u64,
                        crate::db::AbilitySlot::Activated(i) => 200 + i as u64,
                        crate::db::AbilitySlot::Hand(i) => 300 + i as u64,
                        crate::db::AbilitySlot::Graveyard(i) => 400 + i as u64,
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
                    // An ability by its stack position (public).
                    Target::Stack(id) => 9000 + self.stack.iter().position(|x| x.id == id).map_or(99, |i| i as u64),
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
            // (A duration "while <it> remains on the battlefield" names it
            // by its place.)
            let until = match e.until {
                crate::layers::Until::EndOfTurn => 0,
                crate::layers::Until::Forever => 1,
                crate::layers::Until::YourNextTurn => 2,
                crate::layers::Until::WhileOnBattlefield(r) => 3000 + live_place(r),
                crate::layers::Until::WhileSource => unreachable!("a lowered duration"),
            };
            v.extend([e.ts as u64, e.controller as u64, until]);
            match &e.on {
                crate::layers::Affected::Obj(r) => v.push(500 + place(r.idx)),
                crate::layers::Affected::Group(_) => v.push(1),
            }
            v.push(h64(&e.m));
        }
        v.push(u64::MAX);
        for l in &self.links {
            // (A stale reference names no object: its slot's place would
            // tell which physical card a hidden choice was.)
            v.extend([l.until as u64, l.back as u64, live_place(l.source), live_place(l.card)]);
            v.push(l.until_monarch.map_or(9, |p| p as u64));
        }
        v.push(u64::MAX);
        v.push(self.monarch.map_or(9, |p| p as u64));
        // The initiative and the dungeons (public, CR 725, 309).
        v.push(self.initiative.map_or(9, |p| p as u64));
        for d in &self.dungeons {
            v.extend([d.obj.is_some() as u64, d.room as u64, d.completed as u64]);
        }
        // Day and night (public, CR 731); optional actions done once this
        // turn (public), by the source's place.
        v.push(self.day.map_or(9, |d| d as u64));
        for &(r, k) in &self.done_once {
            v.extend([live_place(r), k as u64]);
        }
        v.push(u64::MAX);
        // Discarded this turn (public), triggered abilities until a next
        // turn (public), spells exiled instead of going to a graveyard.
        for &r in &self.discarded {
            v.push(live_place(r));
        }
        for t in &self.temp_triggers {
            v.extend([t.controller as u64, t.source_def as u64, t.slot as u64]);
        }
        for &c in &self.gy_exile {
            v.push(place(c));
        }
        v.push(u64::MAX);
        for pm in &self.permissions {
            // (A prepared permanent by its place, CR 722.3c.)
            let kind = match pm.kind {
                crate::game::PermKind::Prepared { source } => 5_000_000 + live_place(source),
                k => h64(&k),
            };
            v.extend([live_place(pm.card), pm.player as u64, pm.turn.unwrap_or(0) as u64, kind]);
        }
        v.push(u64::MAX);
        for p in 0..2usize {
            for r in &self.players[p].attacked_with {
                v.push(if self.live(*r) { place(r.idx) } else { 7777 });
            }
            v.push(u64::MAX);
        }
        for &(r, n) in &self.resolved {
            // (Which object, by its place; `inc` is not observable.)
            v.extend([if self.live(r) { place(r.idx) } else { 7777 }, n as u64]);
        }
        v.push(u64::MAX);
        for d in &self.delayed {
            v.extend([d.when as u64, d.controller as u64, d.source_def as u64, d.slot as u64, d.ev.amount as u64]);
            v.push(d.ev.obj.map_or(u64::MAX, live_place));
        }
        v.push(u64::MAX);
        for p in 0..2u8 {
            let pl = &self.players[p as usize];
            v.extend([pl.life as i64 as u64, pl.hand.len() as u64, pl.library.len() as u64, pl.lands_played as u64]);
            v.push(pl.cant_gain_life as u64);
            v.extend([pl.lost_life as u64, pl.flash_sorceries as u64, pl.poison as u64, pl.experience as u64]);
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
        self.command.hash(&mut h);
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
        self.any_loss.hash(&mut h);
        self.links.hash(&mut h);
        self.delayed.hash(&mut h);
        self.resolved.hash(&mut h);
        self.permissions.hash(&mut h);
        self.monarch.hash(&mut h);
        self.lki_counters.hash(&mut h);
        self.counters.hash(&mut h);
        self.n_static.hash(&mut h);
        self.n_self_types.hash(&mut h);
        self.day.hash(&mut h);
        self.entering_face.hash(&mut h);
        self.done_once.hash(&mut h);
        self.discarded.hash(&mut h);
        self.temp_triggers.hash(&mut h);
        self.gy_exile.hash(&mut h);
        self.batch_start.hash(&mut h);
        self.copies.hash(&mut h);
        self.copy_objs.hash(&mut h);
        self.prepared.hash(&mut h);
        self.lki_copy.hash(&mut h);
        self.spell_lki.hash(&mut h);
        self.may_copies.hash(&mut h);
        self.initiative.hash(&mut h);
        self.dungeons.hash(&mut h);
        self.rules_src.hash(&mut h);
        self.activations.hash(&mut h);
        self.miracle_check.hash(&mut h);
        self.marks.hash(&mut h);
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
        for &c in &self.face_down {
            mark(c, Zone::FaceDown, None)?;
        }
        for &c in &self.exile {
            mark(c, Zone::Exile, None)?;
        }
        for &c in &self.command {
            mark(c, Zone::Command, None)?;
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
            // (A completed dungeon and the source of the initiative's
            // abilities are rules objects in no zone, ENGINE.md 27.4.)
            if self.objs[i].zone == Zone::Nowhere && !self.is_token(CardIdx(i as u16)) && !self.def_of(CardIdx(i as u16)).rules {
                return Err(format!("card {i} is nowhere"));
            }
            // CR 712.8a, 715.4: only a permanent or a spell has a face up
            // other than its front.
            // (A copy of a prepare spell in exile has its characteristics,
            // CR 722.3c.)
            if self.db.is_face(self.objs[i].def)
                && !matches!(self.objs[i].zone, Zone::Battlefield | Zone::Stack)
                && !(self.objs[i].zone == Zone::Exile && self.is_copy_obj(CardIdx(i as u16)))
            {
                return Err(format!("card {i} ({}) has a face up in {:?}", self.name(CardIdx(i as u16)), self.objs[i].zone));
            }
        }
        if self.entering_face.is_some() {
            return Err("an entering face left over".into());
        }
        // The static-ability fast paths count the faces that are up.
        let n_static = self.battlefield.iter().filter(|&&c| !self.def_of(c).statics.is_empty()).count()
            + self.command.iter().filter(|&&c| !self.def_of(c).statics.is_empty()).count();
        if n_static != self.n_static as usize {
            return Err(format!("n_static {} but {n_static} permanents and emblems with static abilities", self.n_static));
        }
        // The 704.5d check looks only from the first token slot.
        if let Some(i) = (0..(self.first_token as usize).min(self.objs.len())).find(|&i| self.is_token(CardIdx(i as u16))) {
            return Err(format!("slot {i} is a token below the first token slot {}", self.first_token));
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
