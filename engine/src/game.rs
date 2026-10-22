//! The game state and the primitives every rule uses (ENGINE.md 2, 4, 10).

use crate::db::{types, AbilityRef, CardDb, CardDef, Kw};
use crate::decision::{Choice, Decision};
use crate::flow::Task;
use crate::hidden::{KEY_DRAW, KEY_REVEAL};
use crate::ids::{opp, CardIdx, DefId, ObjRef, PlayerId, Target, Zone};
use crate::mana::ManaPool;
use crate::rng::Rng;
use crate::triggers::{Event, PendingTrigger};
use crate::db::EventKind;

/// Settings of a game (zero-core's `Spec` will wrap this plus the decks).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GameSpec {
    pub starting_life: i32,
    /// Opening hand size (CR 103.5).
    pub hand_size: u8,
    /// Maximum hand size (CR 402.2).
    pub max_hand: u8,
    /// London mulligan (CR 103.5); false = everyone keeps seven.
    pub mulligans: bool,
    /// The game is a draw after this many turns (an engine cap, not a rule).
    pub max_turns: u32,
    /// Answer single-option decisions internally (ENGINE.md 4).
    pub auto_single: bool,
    /// Keep a human-readable log (tests and debugging only).
    pub log: bool,
}

impl Default for GameSpec {
    fn default() -> Self {
        GameSpec {
            starting_life: 20,
            hand_size: 7,
            max_hand: 7,
            mulligans: true,
            max_turns: 60,
            auto_single: true,
            log: false,
        }
    }
}

/// One card slot (ENGINE.md 2). Per-object state resets on zone changes.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Obj {
    pub def: DefId,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub zone: Zone,
    pub inc: u16,
    /// CR 613.7; refreshed when entering the battlefield.
    pub timestamp: u32,
    pub tapped: bool,
    /// Marked damage (CR 120.3e).
    pub damage: u16,
    /// Dealt damage by a deathtouch source since the last SBA check (704.5h).
    pub deathtouched: bool,
    /// Turn number when its controller gained control (CR 302.6).
    pub control_since: u32,
    pub attacking: bool,
    /// An attacking creature that became blocked (CR 509.1h).
    pub blocked: bool,
    /// The attacker this creature blocks.
    pub blocking: Option<CardIdx>,
    /// Dealt combat damage in the first-strike damage step (CR 510.4).
    pub struck_first: bool,
    /// Bit p: player p knows the card's identity (hidden zones; ENGINE.md 10).
    pub known: u8,
    /// The permanent this one (an Equipment) is attached to (CR 301.5);
    /// stale once the host leaves, until SBA 704.5n unattaches it.
    pub attached: Option<ObjRef>,
    /// It entered as an evoked spell (CR 702.74a).
    pub evoked: bool,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Player {
    pub life: i32,
    /// Index 0 = bottom, last = top.
    pub library: Vec<CardIdx>,
    pub hand: Vec<CardIdx>,
    /// Last = top.
    pub graveyard: Vec<CardIdx>,
    pub pool: ManaPool,
    pub lands_played: u8,
    /// Attempted to draw from an empty library since the last SBA check (704.5b).
    pub drew_from_empty: bool,
    pub lost: bool,
    pub mulligans: u8,
    pub kept: bool,
    /// Declared a mulligan in the current round.
    pub mulligan_declared: bool,
    /// Number of this player's most recent turn (0 = none yet).
    pub last_turn: u32,
    /// Turns this player has begun (extra turns included).
    pub turns: u32,
    /// A permanent left the battlefield under this player's control this
    /// turn (revolt).
    pub left_battlefield: bool,
    /// Cards drawn this turn.
    pub drawn: u8,
    /// Spells cast this turn, and noncreature spells.
    pub spells_cast: u8,
    pub noncreature_cast: u8,
    /// Gained life this turn.
    pub gained_life: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum StackKind {
    Spell { card: CardIdx },
    Ability { ability: AbilityRef, source: ObjRef },
}

/// An object on the stack (CR 405). Spells are their card (zone `Stack`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct StackItem {
    pub id: u32,
    pub kind: StackKind,
    pub controller: PlayerId,
    /// (target requirement index, target), in the order chosen.
    pub targets: Vec<(u8, Target)>,
    /// The additional cost the caster chose (index into the card's
    /// options, CR 601.2b); 0 when there is at most one.
    pub cost_option: u8,
    /// The chosen mode instances of a modal spell or ability, in
    /// non-decreasing order (CR 700.2).
    pub modes: Vec<u8>,
    /// A triggered ability's event (its subjects and amount).
    pub ev: crate::triggers::EvCtx,
    /// The cost a spell was cast for: 0 = its mana cost, k = alternative
    /// cost k - 1 (CR 118.9).
    pub alt: u8,
    /// Divided damage: the amount for each target of the divided
    /// requirement, in the order chosen (CR 601.2d).
    pub division: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Step {
    /// Before the first turn: mulligans.
    Setup,
    Untap,
    Upkeep,
    Draw,
    Main1,
    BeginCombat,
    DeclareAttackers,
    DeclareBlockers,
    FirstStrikeDamage,
    CombatDamage,
    EndCombat,
    Main2,
    End,
    Cleanup,
}

impl Step {
    pub fn is_main(self) -> bool {
        matches!(self, Step::Main1 | Step::Main2)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Turn {
    /// 1 = the first turn of the game.
    pub number: u32,
    pub active: PlayerId,
    pub step: Step,
    pub priority: PlayerId,
    /// Consecutive passes (CR 117.4).
    pub passes: u8,
    pub starting: PlayerId,
    /// The player of the most recent normal (not extra) turn (CR 500.7).
    pub normal: PlayerId,
    /// Another cleanup step follows this one (CR 514.3a).
    pub cleanup_again: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EndReason {
    /// A player lost (life, empty library, ...): see the losers' flags.
    Sba,
    /// Engine cap (`GameSpec.max_turns`).
    TurnLimit,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Outcome {
    /// `None` = draw.
    pub winner: Option<PlayerId>,
    pub reason: EndReason,
}

/// Where `move_card` puts a card in an ordered zone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pos {
    Top,
    Bottom,
    /// With n cards above it (0 = top).
    FromTop(u8),
}

/// The whole game (ENGINE.md 2). `Clone` captures a game suspended anywhere.
#[derive(Clone)]
pub struct Game {
    pub(crate) db: &'static CardDb,
    pub(crate) spec: GameSpec,
    pub(crate) rng: Rng,
    pub(crate) objs: Vec<Obj>,
    pub(crate) players: [Player; 2],
    /// Timestamp order.
    pub(crate) battlefield: Vec<CardIdx>,
    pub(crate) exile: Vec<CardIdx>,
    /// Last = top.
    pub(crate) stack: Vec<StackItem>,
    pub(crate) turn: Turn,
    pub(crate) agenda: Vec<Task>,
    pub(crate) pending: Option<Decision>,
    pub(crate) answer: Option<Choice>,
    pub(crate) waiting: Vec<PendingTrigger>,
    pub(crate) extra_turns: Vec<PlayerId>,
    /// Combat damage assigned so far in the current damage step.
    pub(crate) combat_dmg: Vec<(CardIdx, Target, u16)>,
    pub(crate) next_ts: u32,
    pub(crate) next_stack_id: u32,
    pub(crate) outcome: Option<Outcome>,
    /// Partial knowledge (ENGINE.md 10).
    pub(crate) clouds: Vec<crate::hidden::Cloud>,
    /// Tokens created so far (a cheap "any token?" for the SBA check).
    pub(crate) tokens: u32,
    /// A simultaneous move off the battlefield is in progress (`leaving`).
    pub(crate) batching: bool,
    /// The permanent being put onto the battlefield is a resolving spell
    /// (it was cast).
    pub(crate) entering_cast: bool,
    /// ... cast for its evoke cost.
    pub(crate) entering_evoked: bool,
    /// Set by `determinize`: private reveals to others are keyed as hidden.
    pub(crate) observer: Option<PlayerId>,
    /// Chance keys of the current `apply`; always empty between calls.
    pub(crate) keys: Vec<u64>,
    /// Outcomes queued by `force_chance` (zero-core `chance_k`): every
    /// emitted key consumes one; a draw, a look at the top of a library and
    /// a random discard follow theirs when possible (ENGINE.md 10).
    pub(crate) forced: Vec<u64>,
    /// The next queued outcome in `forced`.
    pub(crate) forced_at: usize,
    /// Decisions answered by `apply` (not counting auto-answered ones).
    pub(crate) decisions: u32,
    /// While refreshing a decision after `determinize`: never auto-answer.
    pub(crate) no_auto: bool,
    /// Continuous effects from resolved spells and abilities (CR 611.2).
    pub(crate) effects: Vec<crate::layers::Effect>,
    /// Counters on permanents: (card, kind, number) (CR 122).
    pub(crate) counters: Vec<(CardIdx, crate::db::Counter, u16)>,
    /// Permanents with static abilities on the battlefield (a fast path for
    /// the characteristic accessors).
    pub(crate) n_static: u16,
    /// Objects that already left the battlefield in the current
    /// simultaneous move (their leave triggers look back, CR 603.10a).
    pub(crate) leaving: Vec<crate::triggers::Lki>,
    pub(crate) log: Option<Vec<String>>,
}

impl Game {
    /// A new game: shuffle, pick the starting player, draw opening hands,
    /// then run to the first decision (the first mulligan, ENGINE.md 5).
    /// `decks[p]` are player p's cards (40 for limited).
    pub fn new(db: &'static CardDb, spec: GameSpec, decks: [&[DefId]; 2], seed: u64) -> Result<Game, String> {
        for (p, d) in decks.iter().enumerate() {
            if d.is_empty() {
                return Err(format!("player {p} has an empty deck"));
            }
            if let Some(&bad) = d.iter().find(|&&id| id as usize >= db.len()) {
                return Err(format!("player {p}: unknown definition {bad}"));
            }
        }
        let mut g = Game::empty(db, spec, seed);
        for p in 0..2u8 {
            for &def in decks[p as usize] {
                let idx = g.add_card(def, p, Zone::Library);
                g.players[p as usize].library.push(idx);
            }
        }
        for p in 0..2 {
            let mut lib = std::mem::take(&mut g.players[p].library);
            g.rng.shuffle(&mut lib);
            g.players[p].library = lib;
        }
        g.turn.starting = g.rng.below(2) as PlayerId;
        g.turn.active = g.turn.starting;
        g.turn.priority = g.turn.starting;
        for p in [g.turn.starting, opp(g.turn.starting)] {
            for _ in 0..spec.hand_size {
                g.draw(p);
            }
        }
        g.agenda.push(Task::BeginTurn);
        if spec.mulligans {
            g.agenda.push(Task::MulliganDeclare { p: g.turn.starting });
        }
        g.advance();
        // Setup is not a mid-game event: its reveals are not chance keys.
        g.keys.clear();
        Ok(g)
    }

    /// A game with no cards, for `new` and the test kit.
    pub(crate) fn empty(db: &'static CardDb, spec: GameSpec, seed: u64) -> Game {
        let player = Player {
            life: spec.starting_life,
            library: Vec::new(),
            hand: Vec::new(),
            graveyard: Vec::new(),
            pool: ManaPool::default(),
            lands_played: 0,
            drew_from_empty: false,
            lost: false,
            mulligans: 0,
            kept: false,
            mulligan_declared: false,
            last_turn: 0,
            turns: 0,
            left_battlefield: false,
            drawn: 0,
            spells_cast: 0,
            noncreature_cast: 0,
            gained_life: false,
        };
        Game {
            db,
            spec,
            rng: Rng::new(seed),
            objs: Vec::new(),
            players: [player.clone(), player],
            battlefield: Vec::new(),
            exile: Vec::new(),
            stack: Vec::new(),
            turn: Turn {
                number: 0,
                active: 0,
                step: Step::Setup,
                priority: 0,
                passes: 0,
                starting: 0,
                normal: 0,
                cleanup_again: false,
            },
            agenda: Vec::new(),
            pending: None,
            answer: None,
            waiting: Vec::new(),
            extra_turns: Vec::new(),
            combat_dmg: Vec::new(),
            next_ts: 1,
            next_stack_id: 1,
            outcome: None,
            clouds: Vec::new(),
            tokens: 0,
            observer: None,
            keys: Vec::new(),
            forced: Vec::new(),
            forced_at: 0,
            decisions: 0,
            no_auto: false,
            effects: Vec::new(),
            counters: Vec::new(),
            n_static: 0,
            leaving: Vec::new(),
            batching: false,
            entering_cast: false,
            entering_evoked: false,
            log: if spec.log { Some(Vec::new()) } else { None },
        }
    }

    /// Append a card slot (not yet in any zone list).
    pub(crate) fn add_card(&mut self, def: DefId, owner: PlayerId, zone: Zone) -> CardIdx {
        assert!(self.objs.len() < u16::MAX as usize, "too many objects");
        let idx = CardIdx(self.objs.len() as u16);
        self.objs.push(Obj {
            def,
            owner,
            controller: owner,
            zone,
            inc: 0,
            timestamp: 0,
            tapped: false,
            damage: 0,
            deathtouched: false,
            control_since: 0,
            attacking: false,
            blocked: false,
            blocking: None,
            struck_first: false,
            known: 0,
            attached: None,
            evoked: false,
        });
        idx
    }

    // ---- public read API -------------------------------------------------

    pub fn db(&self) -> &'static CardDb {
        self.db
    }
    pub fn spec(&self) -> &GameSpec {
        &self.spec
    }
    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }
    pub fn is_over(&self) -> bool {
        self.outcome.is_some()
    }
    pub fn turn(&self) -> &Turn {
        &self.turn
    }
    pub fn player(&self, p: PlayerId) -> &Player {
        &self.players[p as usize]
    }
    pub fn life(&self, p: PlayerId) -> i32 {
        self.players[p as usize].life
    }
    pub fn obj(&self, idx: CardIdx) -> &Obj {
        &self.objs[idx.i()]
    }
    pub fn objs(&self) -> &[Obj] {
        &self.objs
    }
    pub fn battlefield(&self) -> &[CardIdx] {
        &self.battlefield
    }
    pub fn exile_zone(&self) -> &[CardIdx] {
        &self.exile
    }
    pub fn stack(&self) -> &[StackItem] {
        &self.stack
    }
    pub fn waiting_triggers(&self) -> &[PendingTrigger] {
        &self.waiting
    }
    /// Decisions answered through `apply`.
    pub fn decisions(&self) -> u32 {
        self.decisions
    }
    pub fn log_lines(&self) -> &[String] {
        self.log.as_deref().unwrap_or(&[])
    }
    pub fn def_of(&self, idx: CardIdx) -> &'static CardDef {
        self.db.def(self.objs[idx.i()].def)
    }
    pub fn name(&self, idx: CardIdx) -> &'static str {
        &self.def_of(idx).name
    }

    /// The current object of a slot.
    #[inline]
    pub fn obj_ref(&self, idx: CardIdx) -> ObjRef {
        ObjRef { idx, inc: self.objs[idx.i()].inc }
    }

    /// Does `r` still name an existing object (CR 400.7)?
    #[inline]
    pub fn live(&self, r: ObjRef) -> bool {
        let o = &self.objs[r.idx.i()];
        o.inc == r.inc && o.zone != Zone::Nowhere
    }

    // ---- characteristics (the rest in layers.rs, ENGINE.md 2a) -------------

    pub fn is_type(&self, idx: CardIdx, t: u16) -> bool {
        self.types_of(idx) & t != 0
    }
    pub fn is_creature(&self, idx: CardIdx) -> bool {
        self.is_type(idx, types::CREATURE)
    }
    pub fn is_legendary(&self, idx: CardIdx) -> bool {
        self.def_of(idx).supertypes & crate::db::supertypes::LEGENDARY != 0
    }
    /// Summoning sickness (CR 302.6): not continuously controlled since the
    /// start of its controller's most recent turn.
    pub fn is_sick(&self, idx: CardIdx) -> bool {
        let o = &self.objs[idx.i()];
        o.control_since >= self.players[o.controller as usize].last_turn
    }

    /// Permanents controlled by `p`, in timestamp order.
    pub fn permanents_of(&self, p: PlayerId) -> impl Iterator<Item = CardIdx> + '_ {
        self.battlefield.iter().copied().filter(move |&c| self.objs[c.i()].controller == p)
    }

    pub fn creatures_of(&self, p: PlayerId) -> impl Iterator<Item = CardIdx> + '_ {
        self.permanents_of(p).filter(move |&c| self.is_creature(c))
    }

    // ---- logging -----------------------------------------------------------

    #[inline]
    pub(crate) fn note(&mut self, f: impl FnOnce(&Game) -> String) {
        if self.log.is_some() {
            let line = f(self);
            let t = format!("T{} {:?} ", self.turn.number, self.turn.step);
            if let Some(log) = &mut self.log {
                log.push(t + &line);
            }
        }
    }

    // ---- zone changes (ENGINE.md 2) ---------------------------------------

    fn zone_list(&mut self, idx: CardIdx, zone: Zone) -> Option<&mut Vec<CardIdx>> {
        let owner = self.objs[idx.i()].owner as usize;
        match zone {
            Zone::Library => Some(&mut self.players[owner].library),
            Zone::Hand => Some(&mut self.players[owner].hand),
            Zone::Graveyard => Some(&mut self.players[owner].graveyard),
            Zone::Battlefield => Some(&mut self.battlefield),
            Zone::Exile => Some(&mut self.exile),
            Zone::Stack | Zone::Nowhere => None,
        }
    }

    /// Move a card to another zone: the one place zone lists change. Bumps
    /// the incarnation (CR 400.7), resets per-object state, updates
    /// knowledge, emits the reveal key and the zone-change event. A spell
    /// leaving the stack loses its stack item. Battlefield entry goes through
    /// [`put_onto_battlefield`](Self::put_onto_battlefield) (controller).
    pub(crate) fn move_card(&mut self, idx: CardIdx, to: Zone, pos: Pos) -> ObjRef {
        self.move_card_ctl(idx, to, pos, None)
    }

    pub(crate) fn move_card_to_stack(&mut self, idx: CardIdx, controller: PlayerId) -> ObjRef {
        self.move_card_ctl(idx, Zone::Stack, Pos::Top, Some(controller))
    }

    /// Put a card onto the battlefield, applying "enters tapped" (CR
    /// 614.1c, decided before it moves) and queueing "as this enters"
    /// choices (614.12, a task that runs next).
    pub(crate) fn put_onto_battlefield(&mut self, idx: CardIdx, controller: PlayerId) -> ObjRef {
        // CR 614.1a: "If a nontoken creature would enter and it wasn't
        // cast, exile it instead."
        let cast = std::mem::take(&mut self.entering_cast);
        let evoked = std::mem::take(&mut self.entering_evoked);
        if !cast && self.def_of(idx).is(types::CREATURE) && !self.def_of(idx).token {
            let replaced = self
                .battlefield
                .iter()
                .any(|&c| self.def_of(c).exiles_uncast_creatures);
            if replaced {
                self.note(|g| format!("{} is exiled instead (not cast)", g.name(idx)));
                return self.move_card(idx, Zone::Exile, Pos::Top);
            }
        }
        let tapped = self.enters_tapped(idx, controller);
        self.entering_evoked = evoked; // read as it moves (its ETB sees it)
        let r = self.move_card_ctl(idx, Zone::Battlefield, Pos::Top, Some(controller));
        if tapped {
            self.objs[idx.i()].tapped = true;
        }
        // CR 122.6: "enters with counters".
        for &(k, n) in &self.def_of(idx).enters_counters {
            self.add_counters(idx, k, n as u16);
        }
        if self.def_of(idx).shock_life > 0 {
            self.agenda.push(Task::AsEnters { card: idx });
        }
        r
    }

    /// Create a token (CR 111.2): a new object slot, put onto the
    /// battlefield under `controller` (its owner, CR 111.2), entering like
    /// any permanent (triggers, "enters tapped").
    pub(crate) fn create_token(&mut self, def: DefId, controller: PlayerId, tapped: bool) -> CardIdx {
        let idx = self.add_card(def, controller, Zone::Nowhere);
        self.tokens += 1;
        self.note(|g| format!("P{controller} creates a {} token", g.name(idx)));
        self.put_onto_battlefield(idx, controller);
        if tapped {
            self.tap(idx);
        }
        idx
    }

    /// Lose life (CR 119.3; paying life is losing it, 119.4).
    pub(crate) fn lose_life(&mut self, p: PlayerId, n: i32) {
        if n > 0 {
            self.players[p as usize].life -= n;
        }
    }

    fn move_card_ctl(&mut self, idx: CardIdx, to: Zone, pos: Pos, controller: Option<PlayerId>) -> ObjRef {
        let from = self.objs[idx.i()].zone;
        let old = self.obj_ref(idx);
        let old_controller = self.objs[idx.i()].controller;
        // Leave-the-battlefield abilities look back in time (CR 603.10a):
        // the events are recorded before the object moves.
        if from == Zone::Battlefield {
            if to == Zone::Graveyard {
                self.event(Event::obj(EventKind::Dies, old));
            }
            self.event(Event::obj(EventKind::LeavesBattlefield, old));
        }
        if let Some(list) = self.zone_list(idx, from) {
            let i = list.iter().position(|&c| c == idx).expect("card in its zone list");
            list.remove(i);
        }
        if from == Zone::Stack {
            self.stack.retain(|s| s.kind != StackKind::Spell { card: idx });
        }
        if from == Zone::Battlefield {
            self.players[old_controller as usize].left_battlefield = true;
        }
        let owner = self.objs[idx.i()].owner;
        let ts = if to == Zone::Battlefield {
            self.next_ts += 1;
            self.next_ts
        } else {
            0
        };
        let turn = self.turn.number;
        let known_before = if from.is_hidden() { self.objs[idx.i()].known } else { 0b11 };
        {
            let o = &mut self.objs[idx.i()];
            o.zone = to;
            o.inc = o.inc.wrapping_add(1);
            o.controller = match (to, controller) {
                (Zone::Battlefield, Some(c)) | (Zone::Stack, Some(c)) => c,
                _ => owner,
            };
            o.timestamp = ts;
            o.tapped = false;
            o.damage = 0;
            o.deathtouched = false;
            o.control_since = turn;
            o.attacking = false;
            o.blocked = false;
            o.blocking = None;
            o.struck_first = false;
            o.known = if to.is_hidden() { known_before } else { 0b11 };
            o.attached = None;
            o.evoked = to == Zone::Battlefield && std::mem::take(&mut self.entering_evoked);
        }
        self.layers_on_move(idx, old, from, to);
        if from == Zone::Battlefield && self.batching {
            let def = self.objs[idx.i()].def;
            self.leaving.push(crate::triggers::Lki { old, def, controller: old_controller });
        }
        if let Some(list) = self.zone_list(idx, to) {
            match pos {
                Pos::Top => list.push(idx),
                Pos::Bottom => list.insert(0, idx),
                Pos::FromTop(n) => {
                    let at = list.len().saturating_sub(n as usize);
                    list.insert(at, idx);
                }
            }
        }
        // A card leaving a hidden zone for a public one is revealed to all.
        // It is keyed even if everyone knew it: which known card a hidden
        // choice (a hand slot) named can differ between sampled worlds.
        if from.is_hidden() {
            let def = self.objs[idx.i()].def;
            if to.is_hidden() {
                self.knowledge_moves(idx, owner, from, to);
            } else {
                self.knowledge_leaves(idx, owner, from, def, known_before);
                if to != Zone::Nowhere {
                    self.emit_public(KEY_REVEAL, owner, def as u64);
                }
            }
        }
        let new = self.obj_ref(idx);
        self.note(|g| format!("{} {:?} -> {:?}", g.name(idx), from, to));
        if to == Zone::Battlefield {
            self.event(Event::obj(EventKind::Enters, new));
        }
        new
    }

    /// Draw a card (CR 121.2); from an empty library, flag 704.5b instead.
    pub(crate) fn draw(&mut self, p: PlayerId) {
        if let Some(&top) = self.players[p as usize].library.last() {
            self.force_library_card(top, crate::hidden::KEY_DRAW, p);
        }
        let Some(&top) = self.players[p as usize].library.last() else {
            self.players[p as usize].drew_from_empty = true;
            self.note(|_| format!("P{p} draws from an empty library"));
            return;
        };
        self.move_card(top, Zone::Hand, Pos::Top);
        self.objs[top.i()].known |= 1 << p;
        let def = self.objs[top.i()].def;
        self.emit_private(KEY_DRAW, p, def as u64);
        let pl = &mut self.players[p as usize];
        pl.drawn = pl.drawn.saturating_add(1);
        self.event(Event::player(EventKind::DrawsCard, p));
    }

    /// Shuffle a library (CR 701.20): positions become unknown to everyone;
    /// what another player knew of its cards becomes "somewhere in that
    /// library" (ENGINE.md 10).
    pub(crate) fn shuffle_library(&mut self, p: PlayerId) {
        self.scramble(p, &[], false, true);
        let mut lib = std::mem::take(&mut self.players[p as usize].library);
        self.rng.shuffle(&mut lib);
        for &c in &lib {
            self.objs[c.i()].known = 0;
        }
        self.players[p as usize].library = lib;
    }

    /// Tap a permanent (a "becomes tapped" event if it was untapped).
    pub(crate) fn tap(&mut self, idx: CardIdx) {
        if !self.objs[idx.i()].tapped && self.objs[idx.i()].zone == Zone::Battlefield {
            self.objs[idx.i()].tapped = true;
            let r = self.obj_ref(idx);
            self.event(Event::obj(EventKind::BecomesTapped, r));
        }
    }

    /// Untap a permanent; with a stun counter, remove one instead (CR
    /// 122.1d).
    pub(crate) fn untap(&mut self, idx: CardIdx) {
        if self.objs[idx.i()].tapped && self.remove_counters(idx, crate::db::Counter::Stun, 1) == 0 {
            self.objs[idx.i()].tapped = false;
        }
    }

    /// Discard a card (CR 701.9): from its owner's hand to their graveyard,
    /// then a "discards" event.
    pub(crate) fn discard(&mut self, idx: CardIdx) {
        let owner = self.objs[idx.i()].owner;
        let r = self.move_card(idx, Zone::Graveyard, Pos::Top);
        self.event(Event {
            kind: EventKind::CardDiscarded,
            a: crate::triggers::Subj::Player(owner),
            b: crate::triggers::Subj::Obj(r),
            amount: 0,
        });
    }

    /// Sacrifice a permanent (CR 701.17): to its owner's graveyard, a
    /// "sacrificed" event first (seen as it was, 603.10a).
    pub(crate) fn sacrifice(&mut self, idx: CardIdx) {
        if self.objs[idx.i()].zone != Zone::Battlefield {
            return;
        }
        let r = self.obj_ref(idx);
        self.event(Event::obj(EventKind::IsSacrificed, r));
        self.move_card(idx, Zone::Graveyard, Pos::Top);
    }

    pub(crate) fn gain_life(&mut self, p: PlayerId, n: i32) {
        if n > 0 {
            self.players[p as usize].life += n;
            self.players[p as usize].gained_life = true;
        }
    }

    /// Deal damage (CR 120.3): to a player as life loss, to a creature as
    /// marked damage; deathtouch marks 704.5h; lifelink gains life.
    /// Damage to a permanent with protection from the source is prevented
    /// (702.16e). Returns the damage dealt.
    pub(crate) fn deal_damage(&mut self, src: DamageSource, t: Target, amount: i32) -> i32 {
        if amount <= 0 {
            return 0;
        }
        match t {
            Target::Player(p) => {
                self.players[p as usize].life -= amount;
            }
            Target::Obj(r) => {
                if !self.live(r) || self.objs[r.idx.i()].zone != Zone::Battlefield {
                    return 0;
                }
                if !self.is_creature(r.idx) {
                    return 0; // planeswalkers and battles: later
                }
                if self.protected_from(r.idx, &src) {
                    self.note(|g| format!("damage to {} prevented (protection)", g.describe_target(t)));
                    return 0;
                }
                let heals = self.def_of(r.idx).heals_on_damage;
                let o = &mut self.objs[r.idx.i()];
                let amt = amount.min(u16::MAX as i32) as u16;
                o.damage = if heals { amt } else { o.damage.saturating_add(amt) };
                if heals {
                    o.deathtouched = false;
                }
                if src.deathtouch {
                    o.deathtouched = true;
                }
                let mut ev = Event::obj(EventKind::DealtDamage, r);
                ev.amount = amount.min(u16::MAX as i32) as u16;
                self.event(ev);
            }
        }
        if src.lifelink {
            self.gain_life(src.controller, amount);
        }
        self.note(|g| format!("{amount} damage to {}", g.describe_target(t)));
        amount
    }

    /// Does permanent `c` have protection from a source with these
    /// characteristics (CR 702.16)?
    pub(crate) fn protected_from(&self, c: CardIdx, src: &DamageSource) -> bool {
        let p = self.protection_of(c);
        if p.is_empty() {
            return false;
        }
        p.everything
            || p.colors & src.colors != 0
            || (p.colorless && src.colors == 0)
            || p.types & src.types != 0
            || (p.opponents && src.controller != self.objs[c.i()].controller)
    }

    /// The characteristics of a permanent (or spell) as a source of damage
    /// or of a targeting spell or ability.
    pub(crate) fn damage_source_of(&self, idx: CardIdx) -> DamageSource {
        let kws = self.keywords_of(idx);
        DamageSource {
            controller: self.objs[idx.i()].controller,
            deathtouch: kws.has(Kw::Deathtouch),
            lifelink: kws.has(Kw::Lifelink),
            colors: self.colors_of(idx),
            types: self.types_of(idx),
        }
    }

    pub fn describe_target(&self, t: Target) -> String {
        match t {
            Target::Player(p) => format!("P{p}"),
            Target::Obj(r) => format!("{}#{}", self.name(r.idx), r.idx.0),
        }
    }
}

/// The characteristics of a damage source that change damage results.
#[derive(Clone, Copy, Debug)]
pub struct DamageSource {
    pub controller: PlayerId,
    pub deathtouch: bool,
    pub lifelink: bool,
    /// Its colours and card types (protection, CR 702.16).
    pub colors: crate::mana::ManaMask,
    pub types: u16,
}
