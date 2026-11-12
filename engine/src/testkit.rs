//! Scenario builder and helpers for tests (not used by the engine), and
//! [`Game::view_of`], whose state part the engine's loop rule also hashes
//! (`loops.rs`).

use crate::db::CardDb;
use crate::decision::{act, Choice, DecisionKind};
use crate::flow::Task;
use crate::game::{Game, GameSpec, Step};
use crate::ids::{CardIdx, PlayerId, Target, Zone};
use std::hash::{Hash, Hasher};

/// A hash of a value (for `view_of` and the loop hash).
pub(crate) fn h64<T: Hash>(x: &T) -> u64 {
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
            self.g.next_ts = self.g.exact_inc(self.g.next_ts, "timestamps");
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
                self.g.add_counters(c, crate::db::Counter::Loyalty, n.max(0) as crate::num::Count);
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
            self.g.add_counters(c, crate::db::Counter::Loyalty, n.max(0) as crate::num::Count);
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
    pub fn counters(mut self, p: PlayerId, name: &str, k: crate::db::Counter, n: crate::num::Count) -> Self {
        let c = self.g.find_card(p, name, Zone::Battlefield).unwrap_or_else(|| panic!("no {name} of P{p}"));
        let have = self.g.counters_on(c, k);
        self.g.remove_counters(c, k, have);
        // (Put by its controller: who puts counters, EoE W6.)
        let actor = self.g.w6.actor.replace(self.g.objs[c.i()].controller);
        self.g.add_counters(c, k, n);
        self.g.w6.actor = actor;
        self
    }

    /// `p` is the monarch (CR 724).
    pub fn monarch(mut self, p: PlayerId) -> Self {
        self.g.monarch = Some(p);
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
        self.g.dungeons[p as usize].completed = completed as u32;
        self
    }

    pub fn graveyard(mut self, p: PlayerId, names: &[&str]) -> Self {
        for n in names {
            let c = self.add(p, n, Zone::Graveyard);
            self.g.players[p as usize].graveyard.push(c);
        }
        self
    }

    /// Mana in `p`'s pool as the step starts (W U B R G C; `haste` of the
    /// red is Arena of Glory's).
    pub fn pool(mut self, p: PlayerId, amt: [u8; 6], haste: u8) -> Self {
        self.g.players[p as usize].pool = crate::mana::ManaPool { amt, art: 0, haste };
        self
    }

    /// `n` more mana of type `ty` in `p`'s pool with the spending
    /// restriction (and rider) `only` (`SPEND_*`, `MANA_UNCOUNTERABLE`; CR
    /// 106.6): Mishra's Workshop's artifact-only {C} is the pool's `art`.
    pub fn pool_restricted(mut self, p: PlayerId, ty: usize, only: crate::db::Spend, n: u8) -> Self {
        self.g.players[p as usize].pool.amt[ty] += n;
        let mut kinds: Vec<crate::eoe_w2::RKind> = self.g.pool_kinds(p).iter().copied().collect();
        crate::eoe_w2::add_kind(&mut kinds, ty as u8, only, n);
        self.g.set_pool_kinds(p, &kinds);
        self
    }

    /// `p`'s sideboard (cards outside the game, ENGINE.md 35).
    pub fn sideboard(mut self, p: PlayerId, names: &[&str]) -> Self {
        let mut sb: Vec<_> = names.iter().map(|n| self.g.db.expect_id(n).unwrap_or_else(|e| panic!("{e}"))).collect();
        sb.extend(self.g.players[p as usize].sideboard.iter().copied());
        sb.sort_unstable();
        self.g.players[p as usize].sideboard = std::sync::Arc::from(sb);
        self
    }

    /// `p` revealed `name` from their sideboard as their companion (CR
    /// 103.2b): in the sideboard, and a public card in no zone.
    pub fn companion(mut self, p: PlayerId, name: &str) -> Self {
        self = self.sideboard(p, &[name]);
        let c = self.add(p, name, Zone::Nowhere);
        self.g.players[p as usize].companion = Some(c);
        self.g.players[p as usize].companion_def = Some(self.g.objs[c.i()].def);
        self
    }

    pub fn life(mut self, p: PlayerId, n: i64) -> Self {
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
            .map(|i| CardIdx::new(i as u32))
            .find(|&c| self.objs[c.i()].owner == owner && self.objs[c.i()].zone == zone && self.name(c) == name)
    }

    pub fn card(&self, owner: PlayerId, name: &str, zone: Zone) -> CardIdx {
        self.find_card(owner, name, zone)
            .unwrap_or_else(|| panic!("no {name} of P{owner} in {zone:?}\n{}", self.log_lines().join("\n")))
    }

    pub fn count_in(&self, owner: PlayerId, name: &str, zone: Zone) -> usize {
        (0..self.objs.len())
            .map(|i| CardIdx::new(i as u32))
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

    /// Tap or untap permanent `c` directly: a test's setup, not a game
    /// action (no event; ENGINE.md 99.3).
    pub fn set_tapped(&mut self, c: CardIdx, tapped: bool) {
        self.objs[c.i()].tapped = tapped;
    }

    /// Player `p` gains control of permanent `c` (as a control-changing
    /// effect does, CR 613.1b): a test's setup; the pending decision is
    /// asked again.
    pub fn take_control(&mut self, c: CardIdx, p: PlayerId) {
        self.change_control(c, p);
        self.refresh_pending();
    }

    pub fn kind(&self) -> Option<DecisionKind> {
        self.pending.as_ref().map(|d| d.kind)
    }

    /// `c` is an option of the pending decision; at Priority, a cast or play
    /// from the graveyard counts when "cast or play from the graveyard"
    /// leads to it (ENGINE.md 100.5).
    pub fn has_option(&self, c: Choice) -> bool {
        self.pending.as_ref().is_some_and(|d| d.action_of(c).is_some()) || self.graveyard_route(c)
    }

    /// The choices of the pending decision; at Priority, "cast or play from
    /// the graveyard" stands for the casts and plays it leads to (ENGINE.md
    /// 100.5): what each was as a Priority option before.
    pub fn offered(&self) -> Vec<Choice> {
        let Some(d) = self.pending.as_ref() else { return Vec::new() };
        let mut v: Vec<Choice> = d.options.iter().map(|o| o.choice).filter(|&c| c != Choice::FromGraveyard).collect();
        if d.kind == DecisionKind::Priority && d.action_of(Choice::FromGraveyard).is_some() {
            v.extend(self.graveyard_cast_options(d.player).iter().map(|o| o.choice));
        }
        v
    }

    /// Priority offers `Choice::FromGraveyard` and `c` is one of the casts
    /// and plays it leads to (ENGINE.md 100.5).
    fn graveyard_route(&self, c: Choice) -> bool {
        self.pending.as_ref().is_some_and(|d| d.kind == DecisionKind::Priority && d.action_of(Choice::FromGraveyard).is_some())
            && self.graveyard_cast_options(self.pending.as_ref().unwrap().player).iter().any(|o| o.choice == c)
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
        // (A cast or play from the graveyard: "cast or play from the
        // graveyard", then that card, ENGINE.md 100.5.)
        if self.pending.as_ref().is_some_and(|d| d.action_of(c).is_none()) && self.graveyard_route(c) {
            let mut keys = Vec::new();
            self.apply(act::GRAVEYARD, &mut keys).unwrap();
            if self.pending.as_ref().is_some_and(|d| d.action_of(c).is_some()) && matches!(self.agenda.last(), Some(Task::GraveyardCast { .. })) {
                keys.extend(self.choose(c));
            }
            return keys;
        }
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

    /// Answer the play-or-draw decision of a new game (CR 103.1): the
    /// chooser plays first. Returns the chance keys (the opening hands').
    pub fn play_first(&mut self) -> Vec<u64> {
        assert_eq!(self.kind(), Some(DecisionKind::PlayDraw));
        let c = self.turn.chooser;
        self.choose(Choice::Target(Target::Player(c)))
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

    /// Set the X of the top stack item (tests: an X past what a payment
    /// reaches today, the u8 generic mana of a cost).
    pub fn set_top_x(&mut self, x: u32) {
        self.stack.last_mut().expect("a stack item").x = x;
    }

    /// Everything `viewer` can observe, as numbers (hidden-information
    /// tests): public zones and objects, zone sizes, life and mana, the
    /// turn, the viewer's own hand, every card the viewer knows exactly
    /// where it is (library positions; the other hand as a multiset, its
    /// cards are not told apart), the library positions the other player
    /// knows (ENGINE.md 72), the viewer's clouds, and the pending
    /// decision (its options only if the viewer decides). Two worlds a
    /// `determinize(viewer)` may produce must agree on it. The state part
    /// ([`view_state`](Self::view_state)) and the pending decision.
    /// The engine's continuation (the agenda's last `n` tasks, top last), for a failure's message.
    #[doc(hidden)]
    pub fn agenda_tail(&self, n: usize) -> String {
        let a = &self.agenda;
        format!("{:?}", &a[a.len().saturating_sub(n)..])
    }

    pub fn view_of(&self, viewer: PlayerId) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::with_capacity(256);
        self.view_state(viewer, None, &mut v);
        if let Some(d) = &self.pending {
            v.extend([d.player as u64, d.kind as u64]);
            if d.player == viewer {
                v.extend(d.options.iter().map(|o| o.action as u64));
                // (A page or a digit: the decider's, as the encoding's
                // context floats, ENGINE.md 100.1.)
                if d.stage != crate::decision::Stage::One {
                    v.push(h64(&d.stage));
                }
            }
        }
        v
    }

    /// An object by its public place (`view_of`; CardIdx identities are
    /// not observable): its battlefield position, its stack position, its
    /// definition in another public zone, nothing in a hidden one.
    pub(crate) fn view_place(&self, c: CardIdx) -> u64 {
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
    }

    /// [`view_place`](Self::view_place) of a reference; a stale one names
    /// no object (7777).
    pub(crate) fn view_live_place(&self, r: crate::ids::ObjRef) -> u64 {
        if self.live(r) {
            self.view_place(r.idx)
        } else {
            7777
        }
    }

    /// The state part of [`view_of`](Self::view_of): everything `viewer`
    /// observes but the pending decision. With `canon` (the loop hash,
    /// `loops.rs`), the consecutive passes are left out (taking a loop
    /// action sets them to 0, CR 117.3c; a may-copy is asked while a spell
    /// resolves, with none) and timestamps are named by their rank in
    /// `canon` (a uniform renumbering is the same state, CR 613.7).
    pub(crate) fn view_state(&self, viewer: PlayerId, canon: Option<&crate::loops::Canon>, v: &mut Vec<u64>) {
        let t = &self.turn;
        let passes = if canon.is_some() { u64::MAX - 11 } else { t.passes as u64 };
        v.extend([t.number as u64, t.step as u64, t.active as u64, t.priority as u64, passes]);
        v.push(self.outcome.map_or(9, |o| o.winner.map_or(2, |w| w as u64)));
        // The turn's other public parts and the extra turns to come (ENGINE.md
        // 58.6: the public-state gate's view).
        v.extend([t.normal as u64, t.extra_combats as u64, t.after_combat as u64, t.cleanup_again as u64]);
        v.extend(self.extra_turns.iter().map(|&p| p as u64));
        v.push(u64::MAX - 6);
        let obj = |v: &mut Vec<u64>, c: CardIdx| {
            let o = &self.objs[c.i()];
            v.extend([o.def as u64, o.owner as u64, o.controller as u64, o.tapped as u64, o.damage as u64]);
            v.extend([o.attacking as u64, o.blocked as u64, o.blocking.is_some() as u64, o.escaped as u64]);
            v.extend([o.chosen_player.map_or(9, |p| p as u64), o.monstrous as u64]);
            // What it attacks (a planeswalker by its place), loyalty
            // activations this turn, when it entered (public).
            v.push(o.attacks.map_or(u64::MAX, |r| self.battlefield.iter().position(|&x| x == r.idx).unwrap_or(999) as u64));
            let l = self.loyalty_of(c);
            v.extend([l.turn as u64, l.used as u64, l.extra as u64, o.entered as u64]);
            // Since when its controller controls it, what it blocks (by
            // place), first-strike damage dealt, evoked, cast with flashback
            // (public; ENGINE.md 58.6).
            v.push(o.blocking.map_or(u64::MAX, |b| self.battlefield.iter().position(|&x| x == b).unwrap_or(999) as u64));
            v.extend([o.control_since as u64, o.struck_first as u64, o.evoked as u64, o.flashback as u64]);
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
            obj(v, c);
            // A face-down permanent (CR 708): its card only if the viewer
            // knows it (ENGINE.md 31).
            if self.objs[c.i()].def == self.db.face_down_def() {
                v.push(if self.knows(viewer, c) { self.ident(c) as u64 } else { u64::MAX - 4 });
            }
        }
        v.push(u64::MAX);
        for &c in &self.exile {
            obj(v, c);
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
        // definition in another public zone (`view_place`).
        let place = |c: CardIdx| -> u64 { self.view_place(c) };
        let live_place = |r: crate::ids::ObjRef| -> u64 { self.view_live_place(r) };
        for s in &self.stack {
            // (The loop hash reads the board: a loop action's own stack
            // object is no progress, `loops::loop_product`.)
            if canon.is_some() && crate::loops::loop_product(s) {
                continue;
            }
            match s.kind {
                crate::game::StackKind::Spell { card } => {
                    v.extend([1, self.objs[card.i()].def as u64, self.is_copy_obj(card) as u64, s.times as u64]);
                    // (Cast with flashback: exiled as it leaves, ENGINE.md 58.6.)
                    v.push(self.objs[card.i()].flashback as u64)
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
            // How it was cast and announced (public; ENGINE.md 58.6): the
            // additional and alternative costs, from where, delve, X, the
            // division, splices, a ninjutsu attack, a trigger's event and
            // noted permanents (objects by place, a stack item by position).
            v.extend([s.cost_option as u64, s.alt as u64, h64(&s.from), s.delved as u64, s.x as u64, s.origin as u64]);
            v.extend(s.division.iter().map(|&n| n as u64));
            v.extend(s.spliced.iter().map(|&d| d as u64));
            v.push(s.attack_as.map_or(u64::MAX, |a| a.map_or(u64::MAX - 1, live_place)));
            v.extend([s.ev.player.map_or(9, |p| p as u64), s.ev.obj.map_or(u64::MAX, live_place), s.ev.obj2.map_or(u64::MAX, live_place)]);
            v.extend([s.ev.amount as u64, self.view_stack_pos(s.ev.item, canon.is_some())]);
            v.extend(s.noted.iter().map(|&r| live_place(r)));
            v.push(u64::MAX - 7);
            for &(r, tg) in &s.targets {
                v.push(r as u64);
                v.push(match tg {
                    Target::Player(p) => p as u64,
                    Target::Obj(o) => 100 + place(o.idx),
                    // An ability by its stack position (public).
                    Target::Stack(id) => 9000 + self.view_stack_pos(id, canon.is_some()),
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
        // place (the loop hash: but those that change nothing,
        // `loops::inert`).
        for e in &self.effects {
            if canon.is_some() && crate::loops::inert(e) {
                continue;
            }
            // (A duration "while <it> remains on the battlefield" names it
            // by its place.)
            let until = match e.until {
                crate::layers::Until::EndOfTurn => 0,
                crate::layers::Until::Forever => 1,
                crate::layers::Until::YourNextTurn => 2,
                crate::layers::Until::WhileOnBattlefield(r) => 3000 + live_place(r),
                crate::layers::Until::WhileSource => unreachable!("a lowered duration"),
            };
            v.extend([canon.map_or(e.ts as u64, |k| k.rank(e.ts)), e.controller as u64, until]);
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
        // Rare per-object facts (public; ENGINE.md 58.6): exhausted, a colour or
        // name chosen, exerted, can't be countered, ... (objects by place).
        for &(r, m) in &self.marks {
            // (A mark naming another object names it by its place too: its
            // slot and incarnation are not observable, and differ between
            // worlds when it came from a hidden zone - Animate Dead's
            // creature from a resampled hand, a Nadu from a searched
            // library; ENGINE.md 71.2.)
            let what = match m {
                crate::game::Mark::ExiledToPay(x) => 1_000_000 + live_place(x),
                crate::game::Mark::Reanimated(x) => 2_000_000 + live_place(x),
                crate::game::Mark::Triggered { def, slot, turn, n, src } => h64(&(def, slot, turn, n, live_place(src))),
                // (Its timestamp by its rank: the loop hash.)
                crate::game::Mark::Granted { def, slot, this_turn, ts } if canon.is_some() => {
                    h64(&(def, slot, this_turn, canon.unwrap().rank(ts), "granted"))
                }
                m => h64(&m),
            };
            v.extend([live_place(r), what]);
        }
        v.push(u64::MAX);
        // Phased-out permanents (public, CR 702.26; EoE W3): what they are,
        // and whether each phased out with what it is attached to.
        for &c in &self.w3.phased {
            obj(v, c);
            v.push(self.w3.indirect.contains(&c) as u64);
        }
        v.push(u64::MAX);
        // Recorded control-changing effects (public, CR 613.1b; EoE W6): in
        // timestamp order, each by its permanent's place, its player and
        // whether it ends with the turn (not the timestamps themselves).
        // (And the turn each player last created tokens, public.)
        v.extend(self.w6.tokens_turn.iter().map(|&t| t as u64));
        let mut ctl = self.w6.control.clone();
        ctl.sort_by_key(|x| x.ts);
        for x in ctl {
            v.extend([live_place(x.obj), x.to as u64, x.eot as u64, (x.ts == 0) as u64]);
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
        // Void (public; ENGINE.md 99.4).
        v.push(self.void_active() as u64);
        // Controlling another player (public, CR 722; ENGINE.md 99.14).
        v.extend(self.next_turn_controller.iter().map(|q| q.map_or(9, |q| q as u64)));
        v.push(self.turn_controller.map_or(9, |q| q as u64));
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
            if t.this_turn {
                v.push(1 << 40);
            }
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
            // (The turn matters only to "your next turn", ENGINE.md 99.3.)
            if d.when == crate::db::DelayedWhen::YourNextTurnEndStep {
                v.push(d.turn as u64);
            }
        }
        v.push(u64::MAX);
        // Who chose who takes the first turn (CR 103.1); sideboards (ENGINE.md
        // 35): the viewer's own, the other's size; revealed companions
        // (public) and whether their special action was taken.
        v.extend([t.chooser as u64, t.starting as u64]);
        for p in 0..2u8 {
            let pl = &self.players[p as usize];
            if p == viewer {
                v.extend(pl.sideboard.iter().map(|&d| d as u64));
            }
            // (With hidden decklists the other player's sideboard size is
            // hidden too: it tells their basic lands, ENGINE.md 102.1.)
            let size = if self.spec.hidden_decklists && p != viewer { u64::MAX - 9 } else { self.sideboard_size(p) as u64 };
            v.extend([size, pl.companion_used as u64]);
            v.push(pl.companion_def.map_or(u64::MAX, |d| d as u64));
            // The London mulligan's public state (CR 103.5; ENGINE.md 55):
            // mulligans taken, kept, a declared mulligan not yet taken.
            v.extend([pl.mulligans as u64, pl.kept as u64, pl.mulligan_declared as u64]);
            v.push(u64::MAX - 5);
        }
        for p in 0..2u8 {
            let pl = &self.players[p as usize];
            v.extend([pl.life as i64 as u64, pl.hand.len() as u64, pl.library.len() as u64, pl.lands_played as u64]);
            v.push(pl.cant_gain_life as u64);
            v.extend([pl.lost_life as u64, pl.flash_sorceries as u64, pl.poison as u64, pl.experience as u64]);
            v.extend(pl.pool.amt.iter().map(|&n| n as u64));
            // The rest of a player's public state (ENGINE.md 58.6): the pool's
            // tagged mana, losses, turns, this turn's counts and permissions,
            // energy, blessings and protection, the companion's card.
            v.extend([pl.pool.art as u64, pl.pool.haste as u64, pl.drew_from_empty as u64, pl.lost as u64, pl.turns as u64]);
            // (Lands that entered this turn, the pool's other restricted mana:
            // ENGINE.md 99.12.)
            v.push(self.lands_entered_this_turn(p) as u64);
            for &(q, ty, only, n) in &self.pool_restr {
                if q == p {
                    v.extend([u64::MAX - 10, ty as u64, only as u64, n as u64]);
                }
            }
            v.extend([pl.left_battlefield as u64, pl.drawn as u64, pl.spells_cast as u64, pl.noncreature_cast as u64]);
            v.extend([pl.gained_life as u64, pl.draw_step_draws as u64, pl.spells_game as u64, pl.energy as u64]);
            v.push(pl.life_gained as u64);
            v.push(pl.artifact_entered as u64);
            v.extend([pl.gy_play_turn as u64, pl.gy_exile_turn as u64, pl.city_blessing as u64, pl.prot_until as u64]);
            // ("The next spell you cast this turn can't be countered"
            // effects waiting: ENGINE.md 123.)
            if pl.uncounterable_next > 0 {
                v.extend([u64::MAX - 70, pl.uncounterable_next as u64]);
            }
            // The revealed companion: still outside the game or not (its
            // slot's zone once in the game is not observable: a twin in a
            // hand the viewer does not know may be the one that leaves it,
            // ENGINE.md 71.2; where it is in public is the zones' own).
            v.push(pl.companion.map_or(u64::MAX, |c| (self.objs[c.i()].zone == Zone::Nowhere) as u64));
            v.extend(pl.graveyard.iter().map(|&c| self.objs[c.i()].def as u64));
            v.push(u64::MAX);
            // Library cards the viewer knows exactly, by position.
            for (i, &c) in pl.library.iter().enumerate() {
                if self.knows(viewer, c) {
                    v.extend([i as u64, self.objs[c.i()].def as u64]);
                }
            }
            v.push(u64::MAX);
            // The positions the other player knows (who knows is public: a
            // look, a put-back, a shuffle are seen; ENGINE.md 72).
            let other = crate::ids::opp(viewer);
            for (i, &c) in pl.library.iter().enumerate() {
                if self.objs[c.i()].known >> other & 1 != 0 {
                    v.push(i as u64);
                }
            }
            v.push(u64::MAX - 9);
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
            // Its own hand: which of its cards the other player knows (the
            // encoding's "known to the opponent"; twins are canonical,
            // `canon_hand_knowledge`), as the multiset of (definition,
            // known) - an Urza's Bauble's look is the owner's to see
            // (ENGINE.md 23.7, 71.1).
            if p == viewer {
                let o = crate::ids::opp(viewer);
                let mut k: Vec<u64> = pl.hand.iter().map(|&c| (self.objs[c.i()].def as u64) << 1 | (self.objs[c.i()].known >> o & 1) as u64).collect();
                k.sort_unstable();
                v.extend(k);
                v.push(u64::MAX - 8);
            }
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
        self.ev_seq.hash(&mut h);
        self.batch_fired.hash(&mut h);
        self.entries.hash(&mut h);
        self.copies.hash(&mut h);
        self.copy_objs.hash(&mut h);
        self.prepared.hash(&mut h);
        self.lki_copy.hash(&mut h);
        self.lki_damage.hash(&mut h);
        self.spell_lki.hash(&mut h);
        self.loop_taken.hash(&mut h);
        self.initiative.hash(&mut h);
        self.dungeons.hash(&mut h);
        self.rules_srcs.hash(&mut h);
        self.marks.hash(&mut h);
        self.w3.hash(&mut h);
        self.w6.hash(&mut h);
        self.face_down.hash(&mut h);
        self.face_down_ids.hash(&mut h);
        self.lki_power.hash(&mut h);
        self.loyalty.hash(&mut h);
        h.finish()
    }

    /// Check the structural invariants (fuzzer): every card is in exactly
    /// the zone list its `zone` says, once; spells on the stack have items.
    pub fn check_invariants(&self) -> Result<(), String> {
        let mut seen = vec![0u8; self.objs.len()];
        let mut mark = |c: CardIdx, z: Zone, owner: Option<PlayerId>| -> Result<(), String> {
            let o = &self.objs[c.i()];
            if o.zone != z {
                return Err(format!("card {} listed in {z:?} but zone is {:?}", c.get(), o.zone));
            }
            if let Some(p) = owner {
                if o.owner != p {
                    return Err(format!("card {} in P{p}'s zone but owned by P{}", c.get(), o.owner));
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
        for &c in &self.w3.phased {
            mark(c, Zone::PhasedOut, None)?;
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
                return Err(format!("card {i} ({}) appears {n} times", self.name(CardIdx::new(i as u32))));
            }
            // (A completed dungeon and the source of the initiative's
            // abilities are rules objects in no zone, ENGINE.md 27.4.)
            // (Nor is a Mox Diamond whose "discard a land card instead" is
            // being decided, `Task::EnterByDiscardAny`, nor a card whose
            // replacement effects' order is (616.1, `Task::ChooseReplacement`),
            // nor a revealed companion outside the game, ENGINE.md 35.)
            if self.objs[i].zone == Zone::Nowhere
                && !self.is_token(CardIdx::new(i as u32))
                && !self.def_of(CardIdx::new(i as u32)).rules
                && !self.agenda.iter().any(|t| matches!(t, Task::EnterByDiscardAny { card, .. } if card.get() == i as u32))
                && !self.agenda.iter().any(|t| matches!(t, Task::ChooseReplacement { card, .. } if card.idx.get() == i as u32))
                && !self.players.iter().any(|p| p.companion == Some(CardIdx::new(i as u32)))
            {
                return Err(format!("card {i} is nowhere"));
            }
            // CR 712.8a, 715.4: only a permanent or a spell has a face up
            // other than its front.
            // (A copy of a prepare spell in exile has its characteristics,
            // CR 722.3c.)
            if self.db.is_face(self.objs[i].def)
                && !matches!(self.objs[i].zone, Zone::Battlefield | Zone::Stack | Zone::PhasedOut)
                && !(self.objs[i].zone == Zone::Exile && self.is_copy_obj(CardIdx::new(i as u32)))
            {
                return Err(format!("card {i} ({}) has a face up in {:?}", self.name(CardIdx::new(i as u32)), self.objs[i].zone));
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
        if let Some(i) = (0..(self.first_token as usize).min(self.objs.len())).find(|&i| self.is_token(CardIdx::new(i as u32))) {
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

// ---- the Legacy Sneak and Show tests' views (ENGINE.md 123) ------------------

impl Game {
    /// The rare per-object facts of object `c` (`Game::marks`).
    #[doc(hidden)]
    pub fn marks_of(&self, c: CardIdx) -> Vec<crate::game::Mark> {
        let r = self.obj_ref(c);
        self.marks.iter().filter(|x| x.0 == r).map(|x| x.1).collect()
    }

    /// The extra turns to come, in order (CR 500.7).
    #[doc(hidden)]
    pub fn extra_turns(&self) -> &[PlayerId] {
        &self.extra_turns
    }

    /// The pending decision asked again (after a test changed the state by
    /// hand, `set_tapped`).
    #[doc(hidden)]
    pub fn refresh_decision(&mut self) {
        self.refresh_pending();
    }
}
