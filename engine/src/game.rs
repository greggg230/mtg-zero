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
    /// Last known information of its last time on the battlefield (CR
    /// 608.2h): (its `inc` there, its power as it left).
    pub lki_power: Option<(u16, i32)>,
    /// A spell cast with flashback: exiled whenever it would leave the
    /// stack (CR 702.34a).
    pub flashback: bool,
    /// It entered as a spell cast with escape (CR 702.138c).
    pub escaped: bool,
    /// The player chosen as it entered (CR 614.12).
    pub chosen_player: Option<PlayerId>,
    /// It is monstrous (CR 701.37b).
    pub monstrous: bool,
    /// What it attacks while `attacking` (CR 506.3, 508.1b): `None` = the
    /// defending player, `Some(pw)` = that planeswalker (attacking nothing
    /// once it is removed from combat, 506.4c).
    pub attacks: Option<ObjRef>,
    /// The turn it entered the battlefield (0 = never; turn numbers as
    /// u16, which the turn cap keeps far from).
    pub entered: u16,
    /// Loyalty abilities activated this turn (CR 606.3): in turn `pw_turn`,
    /// `pw_used` of them, `pw_extra` more allowed than one (Comet).
    pub pw_turn: u16,
    pub pw_used: u8,
    pub pw_extra: u8,
    /// Times this object transformed (CR 701.27f: an ability of a permanent
    /// transforms it only if it hasn't transformed since the ability was
    /// put on the stack); wraps.
    pub flips: u8,
}

/// How a spell was cast (CR 601.2a-b, 118.9).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CastFrom {
    /// From hand, for its mana cost or an alternative cost (`alt`).
    #[default]
    Hand,
    /// From a graveyard for its flashback cost (printed, or granted: its
    /// mana cost).
    Flashback,
    /// From its owner's graveyard for its escape cost.
    Escape,
    /// With a permission (from exile): for its mana cost, without paying
    /// it (`free`), or with mana spent as though of any colour.
    Permission { free: bool, any_mana: bool },
    /// From its owner's graveyard with retrace (CR 702.81a): its mana cost
    /// and a land card discarded.
    Retrace,
    /// From its owner's graveyard with mayhem (CR 702.187b), discarded this
    /// turn: for its mana cost.
    Mayhem,
}

/// A permission to play a card from a zone other than a hand (CR 601.2a,
/// 305.1): "you may play that card this turn".
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Permission {
    /// The card (stale once it moves: the permission ends, CR 400.7).
    pub card: ObjRef,
    pub player: PlayerId,
    /// Only during this turn (`Some(turn number)`), else while it stays.
    pub turn: Option<u32>,
    pub kind: PermKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PermKind {
    /// Play it (a land too unless `cast_only`), [without paying its mana
    /// cost], [spending mana as though of any colour], [only during a turn
    /// you attacked with a creature of this subtype].
    Play { free: bool, cast_only: bool, any_mana: bool, attacked_subtype: Option<u16> },
    /// It has flashback; the flashback cost is its mana cost (Snapcaster
    /// Mage).
    Flashback,
    /// An adventurer card on an adventure (CR 715.3d): its owner may cast
    /// it for as long as it stays exiled, but not as an Adventure.
    Adventure,
    /// Cast it from a graveyard (for its mana cost); if that spell would be
    /// put into a graveyard, it is exiled instead (Jace, Telepath Unbound).
    CastThenExile,
    /// A copy of a prepare spell in exile (CR 722.3c): the controller of
    /// `source`, the prepared permanent, may cast it (for its mana cost)
    /// while `source` stays prepared on the battlefield.
    Prepared { source: ObjRef },
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
    /// Can't gain life for the rest of the game (Screaming Nemesis).
    pub cant_gain_life: bool,
    /// Creatures declared as attackers this turn ("during any turn you
    /// attacked with a Rogue").
    pub attacked_with: Vec<ObjRef>,
    /// Cards drawn in this player's own draw step this turn.
    pub draw_step_draws: u8,
    /// Spells cast this game.
    pub spells_game: u16,
    /// Energy counters (CR 122.1).
    pub energy: u16,
    /// Lost life this turn (CR 119.3; damage and payments included).
    pub lost_life: bool,
    /// "Until your next turn, you may cast sorcery spells as though they
    /// had flash."
    pub flash_sorceries: bool,
    /// Poison counters (CR 122.1f; ten or more lose the game, 704.5c).
    pub poison: u16,
    /// Experience counters (CR 122.1).
    pub experience: u16,
}

/// A player's dungeon (CR 309): the dungeon card in the command zone with the
/// venture marker on a room (Undercity's rooms 0..8, `dungeon.rs`), and the
/// dungeons the player completed (309.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Dungeon {
    pub obj: Option<CardIdx>,
    pub room: u8,
    pub completed: u8,
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
    /// Where it was cast from, and how.
    pub from: CastFrom,
    /// Cards exiled for delve so far, and the graveyard slot the next pick
    /// starts from (CR 702.66).
    pub delved: u8,
    pub delve_floor: u8,
    /// A land card was discarded for it to enter (Mox Diamond).
    pub discarded_land: bool,
    /// The value of X (CR 107.3a).
    pub x: u8,
    /// Divided damage: the amount for each target of the divided
    /// requirement, in the order chosen (CR 601.2d).
    pub division: Vec<u8>,
    /// What a creature this ability puts onto the battlefield attacking
    /// attacks (ninjutsu, CR 702.49c): `Some(None)` = the defending
    /// player, `Some(Some(pw))` = that planeswalker.
    pub attack_as: Option<Option<ObjRef>>,
    /// Objects a triggered ability noted as it was put on the stack: the
    /// permanents the spell that triggered it targets (Dack Fayden's
    /// emblem, "gain control of those permanents", still so if that spell
    /// is countered first).
    pub noted: Vec<ObjRef>,
    /// An ability's source's `Obj::flips` as the ability was put on the
    /// stack (CR 701.27f).
    pub flips: u8,
    /// The zone a spell was cast from (`Nowhere` for an ability).
    pub origin: Zone,
    /// The times an additional cost that may be paid repeatedly was paid
    /// (replicate, squad, offspring: `CardDef::times_cost`).
    pub times: u8,
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
    /// An effect says a player wins (CR 104.2b; Thassa's Oracle).
    Effect,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Outcome {
    /// `None` = draw.
    pub winner: Option<PlayerId>,
    pub reason: EndReason,
}

/// A card exiled with a permanent (CR 610.3; 607.2a: linked abilities).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Link {
    /// The permanent it was exiled with.
    pub source: ObjRef,
    /// The exiled card.
    pub card: ObjRef,
    /// "Until the source leaves the battlefield": then the card returns to
    /// `back` (its previous zone; a permanent under its owner's control).
    pub until: bool,
    pub back: Zone,
    /// "Until an opponent of this player becomes the monarch" (Palace
    /// Jailer): then it returns to the battlefield.
    pub until_monarch: Option<PlayerId>,
}

/// A triggered ability a player has for a while (until their next turn):
/// trigger `slot` of definition `source_def`, created by `source`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TempTrigger {
    pub controller: PlayerId,
    pub source: ObjRef,
    pub source_def: DefId,
    pub slot: u8,
}

/// A delayed triggered ability (CR 603.7): it triggers once, at `when`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Delayed {
    pub when: crate::db::DelayedWhen,
    pub controller: PlayerId,
    /// The source of the ability that created it (603.7d).
    pub source: ObjRef,
    pub source_def: DefId,
    /// Its index in `source_def`'s triggers.
    pub slot: u8,
    /// "That card", "that much".
    pub ev: crate::triggers::EvCtx,
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
    /// Emblems (CR 114), in creation order.
    pub(crate) command: Vec<CardIdx>,
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
    /// `waiting` as the current batch began ("whenever one or more ...":
    /// one trigger per batch).
    pub(crate) batch_start: usize,
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
    /// Some effect makes a permanent lose all abilities (a fast path for
    /// [`abilities`](Self::abilities)).
    pub(crate) any_loss: bool,
    /// Counters on permanents: (card, kind, number) (CR 122).
    pub(crate) counters: Vec<(CardIdx, crate::db::Counter, u16)>,
    /// Permanents with static abilities on the battlefield (a fast path for
    /// the characteristic accessors).
    pub(crate) n_static: u16,
    /// Permanents with a static ability that changes their own types
    /// (Kaito; a fast path for `types_of`).
    pub(crate) n_self_types: u16,
    /// Objects that already left the battlefield in the current
    /// simultaneous move (their leave triggers look back, CR 603.10a).
    pub(crate) leaving: Vec<crate::triggers::Lki>,
    /// Cards exiled with a permanent ("until it leaves the battlefield",
    /// CR 610.3; "the exiled card").
    pub(crate) links: Vec<Link>,
    /// Delayed triggered abilities waiting for their moment (CR 603.7).
    pub(crate) delayed: Vec<Delayed>,
    /// How many times abilities of these objects resolved this turn
    /// ("if this is the second time this ability has resolved this turn").
    pub(crate) resolved: Vec<(ObjRef, u8)>,
    /// Permissions to play cards from other zones (CR 601.2a).
    pub(crate) permissions: Vec<Permission>,
    /// Combat damage is being dealt (CR 510.2).
    pub(crate) combat_damage_now: bool,
    /// The monarch (CR 724).
    pub(crate) monarch: Option<PlayerId>,
    /// Counters on permanents that left the battlefield this turn, as they
    /// last existed there (CR 608.2h).
    pub(crate) lki_counters: Vec<(ObjRef, crate::db::Counter, u16)>,
    /// ... cast with escape; cast for its dash cost (delayed ability k).
    pub(crate) entering_escaped: bool,
    /// ... the X it was cast with.
    pub(crate) entering_x: u8,
    pub(crate) entering_dash: Option<u8>,
    /// ... with this face up (a back face: "transformed", CR 712.14a; an
    /// MDFC's land face played, 712.12) instead of its front (712.14).
    pub(crate) entering_face: Option<DefId>,
    /// Day (`Some(true)`) or night (`Some(false)`), once the game has one
    /// of them (CR 731.1).
    pub(crate) day: Option<bool>,
    /// Optional actions done this turn that may be done "only once each
    /// turn": (source, its triggered ability).
    pub(crate) done_once: Vec<(ObjRef, u8)>,
    /// Cards discarded this turn, as the objects they became in a graveyard
    /// (mayhem, CR 702.187b).
    pub(crate) discarded: Vec<ObjRef>,
    /// Triggered abilities a player has until their next turn ("until your
    /// next turn, whenever ..."; `Ext::TriggerUntilYourNextTurn`).
    pub(crate) temp_triggers: Vec<TempTrigger>,
    /// Spells on the stack that are exiled instead of being put into a
    /// graveyard ("if that spell would be put into your graveyard, exile it
    /// instead").
    pub(crate) gy_exile: Vec<CardIdx>,
    // ---- copies (M7b part 3, `copies.rs`; ENGINE.md 26) ----
    /// Copy effects on permanents (CR 707, layer 1), in timestamp order.
    pub(crate) copies: Vec<crate::copies::CopyFx>,
    /// Slots made as copies, not cards (token copies, copies of spells and
    /// of cards), in increasing order.
    pub(crate) copy_objs: Vec<CardIdx>,
    /// Copiable values of permanents that left the battlefield this turn,
    /// as they last existed there (CR 608.2h).
    pub(crate) lki_copy: Vec<(ObjRef, crate::copies::CopyVals)>,
    /// Spells that left the stack while a triggered ability about them
    /// waited: their definition and item (CR 608.2h: "copy it").
    pub(crate) spell_lki: Vec<(ObjRef, DefId, StackItem)>,
    /// The token being created is a copy with these values (consumed as it
    /// enters).
    pub(crate) entering_copy: Option<crate::copies::CopyVals>,
    /// ... its additional cost was paid this many times (consumed by
    /// `put_onto_battlefield`); the note of its `Enters` event (bit 0: cast,
    /// the rest: times paid; consumed by `move_card`); its `Enters` event
    /// waits for an "enter as a copy" choice.
    pub(crate) entering_times: u8,
    pub(crate) enter_note: u16,
    pub(crate) defer_enters: bool,
    /// Prepared permanents (CR 722.3a) and the copy of their prepare spell
    /// in exile (722.3c).
    pub(crate) prepared: Vec<(ObjRef, CardIdx)>,
    /// Copies made this turn by "may copy this spell" effects (Chain of
    /// Smog, Chain Lightning): at `copies::MAY_COPY_CAP` no more are offered
    /// (an engine limit on a loop of optional actions, CR 732.2a, 732.3;
    /// ENGINE.md 26.11).
    pub(crate) may_copies: u8,
    // ---- M7b part 3b: the initiative and dungeons (ENGINE.md 27.4) ----
    /// The player who has the initiative (CR 725).
    pub(crate) initiative: Option<PlayerId>,
    /// Each player's dungeon (CR 309).
    pub(crate) dungeons: [Dungeon; 2],
    /// The object the initiative's inherent triggered abilities name as
    /// their source (CR 725.2: they have none): a slot in no zone, made the
    /// first time one triggers.
    pub(crate) rules_src: Option<CardIdx>,
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
            cant_gain_life: false,
            attacked_with: Vec::new(),
            draw_step_draws: 0,
            spells_game: 0,
            energy: 0,
            lost_life: false,
            flash_sorceries: false,
            poison: 0,
            experience: 0,
        };
        Game {
            db,
            spec,
            rng: Rng::new(seed),
            objs: Vec::new(),
            players: [player.clone(), player],
            battlefield: Vec::new(),
            exile: Vec::new(),
            command: Vec::new(),
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
            any_loss: false,
            counters: Vec::new(),
            n_static: 0,
            n_self_types: 0,
            leaving: Vec::new(),
            links: Vec::new(),
            delayed: Vec::new(),
            resolved: Vec::new(),
            permissions: Vec::new(),
            combat_damage_now: false,
            monarch: None,
            lki_counters: Vec::new(),
            entering_escaped: false,
            entering_x: 0,
            entering_dash: None,
            entering_face: None,
            day: None,
            done_once: Vec::new(),
            discarded: Vec::new(),
            temp_triggers: Vec::new(),
            gy_exile: Vec::new(),
            copies: Vec::new(),
            copy_objs: Vec::new(),
            lki_copy: Vec::new(),
            spell_lki: Vec::new(),
            entering_copy: None,
            entering_times: 0,
            enter_note: 0,
            defer_enters: false,
            prepared: Vec::new(),
            may_copies: 0,
            initiative: None,
            dungeons: [Dungeon::default(); 2],
            rules_src: None,
            batching: false,
            batch_start: 0,
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
            lki_power: None,
            flashback: false,
            escaped: false,
            chosen_player: None,
            monstrous: false,
            attacks: None,
            entered: 0,
            pw_turn: 0,
            pw_used: 0,
            pw_extra: 0,
            flips: 0,
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
    /// Emblems in the command zone (CR 114).
    pub fn command_zone(&self) -> &[CardIdx] {
        &self.command
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
    /// Its name (a copy's "except his name is ...", CR 707.9b).
    pub fn name(&self, idx: CardIdx) -> &'static str {
        match self.exc(idx).and_then(|e| e.name) {
            Some(d) => &self.db.def(d).name,
            None => &self.def_of(idx).name,
        }
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

    /// The cards in `p`'s graveyard, bottom first: a token there (until
    /// 704.5d) is not a card (CR 108.2, 111.1), so it is never counted
    /// (threshold, delirium, the Lhurgoyfs), chosen or exiled as one.
    pub fn graveyard_cards(&self, p: PlayerId) -> impl Iterator<Item = CardIdx> + '_ {
        let gy = &self.players[p as usize].graveyard;
        let any_token = self.tokens > 0;
        gy.iter().copied().filter(move |&c| !any_token || !self.is_token(c))
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
            Zone::Command => Some(&mut self.command),
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
        // The face it enters with (CR 712.13-14): the one it had on the
        // stack, its front, or the face an effect names ("return it
        // transformed", 712.14a; an MDFC's land face, 712.12); a daybound
        // card enters transformed at night (702.145b). Set before it moves,
        // so what applies as it enters sees that face (CR 614.12).
        if let Some(face) = self.entering_face.take() {
            self.objs[idx.i()].def = face;
        } else if self.day == Some(false) && self.def_of(idx).daybound {
            if let Some(b) = self.def_of(idx).back {
                self.objs[idx.i()].def = b;
            }
        }
        // CR 614.1a: "If a nontoken creature would enter and it wasn't
        // cast, exile it instead."
        let cast = std::mem::take(&mut self.entering_cast);
        let evoked = std::mem::take(&mut self.entering_evoked);
        let times = std::mem::take(&mut self.entering_times);
        if !cast && self.def_of(idx).is(types::CREATURE) && !self.is_token(idx) {
            let replaced = self
                .battlefield
                .iter()
                .any(|&c| self.abilities(c).exiles_uncast_creatures);
            if replaced {
                self.entering_copy = None;
                self.note(|g| format!("{} is exiled instead (not cast)", g.name(idx)));
                return self.move_card(idx, Zone::Exile, Pos::Top);
            }
        }
        // "You may have this enter as a copy of ..." (CR 707.5): the choice
        // comes first (`Task::EnterAsCopy`), then what it entered as enters.
        let copy_choice = self.def_of(idx).enters_as_copy.is_some();
        let tapped = !copy_choice && self.enters_tapped(idx, controller);
        self.entering_evoked = evoked; // read as it moves (its ETB sees it)
        let escaped = std::mem::take(&mut self.entering_escaped);
        let dash = std::mem::take(&mut self.entering_dash);
        let x = std::mem::take(&mut self.entering_x);
        self.entering_escaped = escaped && cast;
        // Its `Enters` event says whether it was cast and how many times an
        // additional cost was paid ("if you cast it", squad, offspring).
        // (A copy of a spell resolving wasn't cast, CR 707.10.)
        let note = (cast && !self.is_copy_obj(idx)) as u16 | (times as u16) << 1;
        self.enter_note = note;
        self.defer_enters = copy_choice;
        let r = self.move_card_ctl(idx, Zone::Battlefield, Pos::Top, Some(controller));
        if let (Some(k), true) = (dash, cast) {
            // CR 702.109a: it gains haste, and returns to its owner's hand
            // at the beginning of the next end step.
            let mut haste = crate::db::KwSet::default();
            haste.insert(Kw::Haste);
            self.add_effect(
                crate::layers::Affected::Obj(r),
                crate::db::Mod::Keywords(haste),
                crate::layers::Until::Forever,
                controller,
            );
            let def = self.objs[idx.i()].def;
            self.delayed.push(Delayed {
                when: crate::db::DelayedWhen::NextEndStep,
                controller,
                source: r,
                source_def: def,
                slot: self.db.def(def).delayed_base + k,
                ev: crate::triggers::EvCtx { obj: Some(r), ..Default::default() },
            });
        }
        if copy_choice {
            self.agenda.push(Task::EnterAsCopy { card: idx, note });
            return r;
        }
        self.entering(idx, tapped, x);
        r
    }

    /// What applies as a permanent enters (CR 614.1c, 614.12): "enters
    /// tapped", counters, loyalty, a Saga's lore counter, day and night,
    /// "as this enters" choices.
    pub(crate) fn entering(&mut self, idx: CardIdx, tapped: bool, x: u8) {
        if tapped {
            self.objs[idx.i()].tapped = true;
        }
        // CR 122.6: "enters with counters".
        for &(k, n) in &self.def_of(idx).enters_counters {
            self.add_counters(idx, k, n as u16);
        }
        if self.def_of(idx).enters_x_counters && x > 0 {
            self.add_counters(idx, crate::db::Counter::P1P1, x as u16);
        }
        // CR 306.5b: a planeswalker enters with its printed loyalty.
        if let (Some(n), true) = (self.def_of(idx).loyalty, self.def_of(idx).is(types::PLANESWALKER)) {
            self.add_counters(idx, crate::db::Counter::Loyalty, n as u16);
        }
        // CR 714.3a: a Saga enters with a lore counter on it (its chapter I
        // ability triggers, 714.2b).
        if self.def_of(idx).chapters > 0 {
            self.add_counters(idx, crate::db::Counter::Lore, 1);
        }
        // CR 702.145d, g: a daybound (nightbound) permanent makes it day
        // (night) if it is neither; at night a daybound one transforms.
        if self.def_of(idx).daybound || self.def_of(idx).nightbound {
            self.day_night_check();
        }
        if self.def_of(idx).shock_life > 0 || self.def_of(idx).chooses_player {
            self.agenda.push(Task::AsEnters { card: idx });
        }
        // "This creature enters prepared" (CR 722.3a).
        if self.def_of(idx).enters_prepared {
            self.become_prepared(idx);
        }
    }

    /// Create `n` tokens (CR 111.2) under `controller`, with the
    /// replacement effects that multiply them ("twice that many of those
    /// tokens are created instead", CR 614.1a); the first is first.
    pub(crate) fn create_tokens(&mut self, def: DefId, controller: PlayerId, n: u32, tapped: bool) -> Vec<CardIdx> {
        let n = self.token_count(controller, n);
        (0..n).map(|_| self.create_token(def, controller, tapped)).collect()
    }

    /// How many tokens `controller` creates instead of n ("twice that many
    /// of those tokens", CR 614.1a; at most 64), and no more than leave them
    /// 96 permanents (an engine limit: the `MY_PERM` / `OPP_PERM` action
    /// slots; ENGINE.md 26.11).
    pub(crate) fn token_count(&self, controller: PlayerId, n: u32) -> u32 {
        let mut n = n;
        let mut have = 0u32;
        for c in self.permanents_of(controller) {
            have += 1;
            let f = self.abilities(c).token_factor;
            if f > 1 {
                n = n.saturating_mul(f as u32).min(64);
            }
        }
        n.min((crate::decision::act::PERM_N as u32).saturating_sub(have))
    }

    /// Create one token (CR 111.2): a new object slot, put onto the
    /// battlefield under `controller` (its owner, CR 111.2), entering like
    /// any permanent (triggers, "enters tapped"). No replacement effects:
    /// effects create tokens through [`create_tokens`](Self::create_tokens).
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
            self.players[p as usize].lost_life = true;
        }
    }

    /// `p` gains control of permanent `c` (CR 613.1b; an effect without a
    /// duration). It is removed from combat (506.4).
    pub(crate) fn change_control(&mut self, c: CardIdx, p: PlayerId) {
        if self.objs[c.i()].zone != Zone::Battlefield || self.objs[c.i()].controller == p {
            return;
        }
        let turn = self.turn.number;
        let o = &mut self.objs[c.i()];
        o.controller = p;
        o.control_since = turn;
        o.attacking = false;
        o.blocked = false;
        o.blocking = None;
        o.attacks = None;
        self.note(|g| format!("P{p} gains control of {}", g.name(c)));
    }

    /// `p` gets an emblem (CR 114.2): an object in the command zone, owned
    /// and controlled by `p`, whose abilities function there (114.4).
    pub(crate) fn create_emblem(&mut self, def: DefId, p: PlayerId) -> CardIdx {
        let idx = self.add_card(def, p, Zone::Command);
        self.command.push(idx);
        if !self.db.def(def).statics.is_empty() {
            self.n_static += 1;
        }
        self.note(|g| format!("P{p} gets an emblem ({})", g.name(idx)));
        idx
    }

    /// The definition whose abilities a permanent has: its own, or none
    /// once it lost all abilities (CR 613.1f; Oko's Elk).
    #[inline]
    pub(crate) fn abilities(&self, c: CardIdx) -> &'static CardDef {
        if self.lost_abilities(c) {
            return blank_def();
        }
        self.db.def(self.objs[c.i()].def)
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
        let lki_power = (from == Zone::Battlefield).then(|| (old.inc, self.power(idx)));
        // Its copiable values as it last existed on the battlefield (CR
        // 608.2h: "create a token that's a copy of it").
        if from == Zone::Battlefield {
            let v = self.copiable(idx);
            self.lki_copy.push((old, v));
        }
        // CR 707.10a, 704.5e: a copy of a spell (or of a card) leaving the
        // stack ceases to exist, unless it becomes a permanent (608.3f);
        // nothing can see it in another zone first.
        let to = if from == Zone::Stack && to != Zone::Battlefield && self.is_copy_obj(idx) { Zone::Nowhere } else { to };
        // CR 702.34a: a flashback spell leaving the stack is exiled instead;
        // "if that spell would be put into your graveyard, exile it instead".
        let to = if from == Zone::Stack && self.objs[idx.i()].flashback { Zone::Exile } else { to };
        let to = if from == Zone::Stack && !self.gy_exile.is_empty() && self.gy_exile.contains(&idx) {
            self.gy_exile.retain(|&c| c != idx);
            if to == Zone::Graveyard {
                Zone::Exile
            } else {
                to
            }
        } else {
            to
        };
        // "If it would die this turn, exile it instead" (Suplex); "if it
        // would leave the battlefield, exile it instead of putting it
        // anywhere else" (From the Catacombs; CR 614.1a).
        let to = if from == Zone::Battlefield && to != Zone::Exile {
            let fl = self.flags_of(idx);
            if (to == Zone::Graveyard && fl.exile_if_dies) || fl.exile_if_leaves {
                Zone::Exile
            } else {
                to
            }
        } else {
            to
        };
        if let Some(list) = self.zone_list(idx, from) {
            let i = list.iter().position(|&c| c == idx).expect("card in its zone list");
            list.remove(i);
        }
        if from == Zone::Stack {
            if let Some(i) = self.stack.iter().position(|s| s.kind == StackKind::Spell { card: idx }) {
                let item = self.stack.remove(i);
                self.note_spell_lki(idx, &item);
            }
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
            if lki_power.is_some() {
                o.lki_power = lki_power;
            }
            o.flashback = false;
            o.chosen_player = None;
            o.monstrous = false;
            o.attacks = None;
            o.entered = if to == Zone::Battlefield { turn as u16 } else { 0 };
            o.pw_turn = 0;
            o.pw_used = 0;
            o.pw_extra = 0;
            o.flips = 0;
            o.escaped = to == Zone::Battlefield && std::mem::take(&mut self.entering_escaped);
        }
        // A token created as a copy (CR 707.2, 111.10): a copy effect for as
        // long as it exists, in place before it is seen entering.
        if to == Zone::Battlefield {
            if let Some(v) = self.entering_copy.take() {
                // (It may have entered with another face up: 702.145b.)
                let d = self.objs[idx.i()].def;
                self.next_ts += 1;
                let fx = crate::copies::CopyFx {
                    obj: ObjRef { idx, inc: self.objs[idx.i()].inc },
                    ts: self.next_ts,
                    until: crate::layers::Until::Forever,
                    vals: crate::copies::CopyVals { def: d, exc: v.exc },
                    own: d,
                };
                self.copies.push(fx);
            }
        }
        self.layers_on_move(idx, old, from, to);
        if from == Zone::Battlefield && self.batching {
            let def = self.objs[idx.i()].def;
            self.leaving.push(crate::triggers::Lki { old, def, controller: old_controller });
        }
        // Its copy effects end with it (CR 400.7): it has its own
        // definition again (the copied one was its last known information,
        // used above).
        if !self.copies.is_empty() {
            self.copies_on_move(idx, old);
        }
        // CR 712.8a, 715.4: outside the battlefield and the stack a card has
        // only its front face's characteristics (the face it had there was
        // last known information, used above). A spell becoming a permanent
        // keeps the face it had on the stack (712.13); a card put onto the
        // battlefield from elsewhere comes with its front face up unless it
        // enters transformed (712.14, `put_onto_battlefield`).
        if to != Zone::Battlefield && to != Zone::Stack {
            let o = &mut self.objs[idx.i()];
            o.def = self.db.def(o.def).card_def(o.def);
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
            // (The card's identity: a land played with its back face up, an
            // MDFC, has that face already, `put_onto_battlefield`.)
            let def = self.db.def(self.objs[idx.i()].def).card_def(self.objs[idx.i()].def);
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
            // (The note: cast, times paid; an "enter as a copy" choice holds
            // the event back, `copies.rs`.)
            let note = std::mem::take(&mut self.enter_note);
            if !std::mem::take(&mut self.defer_enters) {
                self.event(Event { kind: EventKind::Enters, a: crate::triggers::Subj::Obj(new), b: crate::triggers::Subj::None, amount: note });
            }
        }
        if from == Zone::Battlefield && !self.links.is_empty() {
            self.release_links(old);
        }
        new
    }

    /// CR 610.3a: the cards exiled "until" `source` leaves the battlefield
    /// return to their previous zones at once; links whose card left exile
    /// are dropped.
    fn release_links(&mut self, source: ObjRef) {
        let (back, keep): (Vec<Link>, Vec<Link>) =
            self.links.iter().copied().partition(|l| l.until && l.source == source);
        self.links = keep;
        for l in back {
            if !self.live(l.card) || self.objs[l.card.idx.i()].zone != Zone::Exile {
                continue;
            }
            let owner = self.objs[l.card.idx.i()].owner;
            self.note(|g| format!("{} returns ({:?})", g.name(l.card.idx), l.back));
            match l.back {
                Zone::Battlefield => {
                    self.put_onto_battlefield(l.card.idx, owner);
                }
                z => {
                    self.move_card(l.card.idx, z, Pos::Top);
                }
            }
        }
        let objs = &self.objs;
        self.links.retain(|l| objs[l.card.idx.i()].inc == l.card.inc && objs[l.card.idx.i()].zone == Zone::Exile);
    }

    /// Draw a card (CR 121.2); from an empty library, flag 704.5b instead.
    pub(crate) fn draw(&mut self, p: PlayerId) {
        // "Each opponent can't draw more than one card each turn" (Narset).
        if self.players[p as usize].drawn >= 1
            && self.turn.number > 0
            && self.permanents_of(opp(p)).any(|c| self.abilities(c).opp_draw_limit)
        {
            self.note(|_| format!("P{p} can't draw another card this turn"));
            return;
        }
        // "If you would draw a card while your library has no cards in it,
        // you win the game instead" (CR 614.1a, 104.2b).
        if self.players[p as usize].library.is_empty()
            && self.outcome.is_none()
            && self.permanents_of(p).any(|c| self.abilities(c).wins_on_empty_draw)
        {
            self.note(|_| format!("P{p} would draw from an empty library: wins the game"));
            self.players[opp(p) as usize].lost = true;
            self.outcome = Some(Outcome { winner: Some(p), reason: EndReason::Effect });
            return;
        }
        if self.turn.step == Step::Draw && self.turn.active == p {
            let pl = &mut self.players[p as usize];
            pl.draw_step_draws = pl.draw_step_draws.saturating_add(1);
        }
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

    /// Put cards on the bottom of `p`'s library in a random order. Nobody
    /// knows the order (ENGINE.md 10).
    pub(crate) fn put_on_bottom_randomly(&mut self, p: PlayerId, mut cards: Vec<CardIdx>) {
        if cards.is_empty() {
            return;
        }
        self.rng.shuffle(&mut cards);
        for &c in &cards {
            self.move_card(c, Zone::Library, Pos::Bottom);
        }
        self.forget_order(p, &cards);
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
        if self.objs[idx.i()].zone == Zone::Graveyard {
            self.discarded.push(r);
        }
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
        if n > 0 && !self.players[p as usize].cant_gain_life {
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
            // (An ability can't be dealt damage.)
            Target::Stack(_) => return 0,
            Target::Player(p) => {
                // CR 120.3a: the player loses that much life.
                self.players[p as usize].life -= amount;
                self.players[p as usize].lost_life = true;
            }
            Target::Obj(r) => {
                if !self.live(r) || self.objs[r.idx.i()].zone != Zone::Battlefield {
                    return 0;
                }
                let creature = self.is_creature(r.idx);
                let walker = self.is_type(r.idx, types::PLANESWALKER);
                if !creature && !walker {
                    return 0; // (battles: none in the cube)
                }
                if self.protected_from(r.idx, &src) && !(self.combat_damage_now && self.combat_unpreventable(src.controller)) {
                    self.note(|g| format!("damage to {} prevented (protection)", g.describe_target(t)));
                    return 0;
                }
                let amt = amount.min(u16::MAX as i32) as u16;
                if walker {
                    // CR 120.3c: that many loyalty counters are removed.
                    self.remove_counters(r.idx, crate::db::Counter::Loyalty, amt);
                }
                if creature {
                    // CR 120.3e: damage is marked (both, for a planeswalker
                    // creature).
                    let heals = self.abilities(r.idx).heals_on_damage;
                    let o = &mut self.objs[r.idx.i()];
                    o.damage = if heals { amt } else { o.damage.saturating_add(amt) };
                    if heals {
                        o.deathtouched = false;
                    }
                    if src.deathtouch {
                        o.deathtouched = true;
                    }
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
        // "Whenever this deals damage" (combat damage: one event per source
        // for the whole step, `combat.rs`).
        if let (false, Some(o)) = (self.combat_damage_now, src.obj) {
            self.event(crate::triggers::Event {
                kind: EventKind::DealsDamage,
                a: crate::triggers::Subj::Obj(o),
                b: crate::triggers::Subj::None,
                amount: amount.min(u16::MAX as i32) as u16,
            });
        }
        amount
    }

    /// `p` becomes the monarch (CR 724.1); a card exiled "until an
    /// opponent becomes the monarch" returns.
    pub(crate) fn set_monarch(&mut self, p: PlayerId) {
        if self.monarch == Some(p) {
            return;
        }
        self.monarch = Some(p);
        self.note(|_| format!("P{p} becomes the monarch"));
        let (back, keep): (Vec<Link>, Vec<Link>) =
            self.links.iter().copied().partition(|l| l.until_monarch.is_some_and(|q| q != p));
        self.links = keep;
        for l in back {
            if self.live(l.card) && self.objs[l.card.idx.i()].zone == Zone::Exile {
                let owner = self.objs[l.card.idx.i()].owner;
                self.put_onto_battlefield(l.card.idx, owner);
            }
        }
    }

    /// "Combat damage that would be dealt by creatures you control can't be
    /// prevented" (Questing Beast) for `p`.
    pub(crate) fn combat_unpreventable(&self, p: PlayerId) -> bool {
        self.permanents_of(p).any(|c| self.abilities(c).combat_unpreventable)
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
            || (p.chosen_player && self.objs[c.i()].chosen_player == Some(src.controller))
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
            obj: Some(self.obj_ref(idx)),
        }
    }

    pub fn describe_target(&self, t: Target) -> String {
        match t {
            Target::Player(p) => format!("P{p}"),
            Target::Obj(r) => format!("{}#{}", self.name(r.idx), r.idx.0),
            Target::Stack(id) => match self.stack.iter().find(|s| s.id == id) {
                Some(s) => match s.kind {
                    StackKind::Ability { ability, .. } => format!("ability of {} (stack #{id})", self.db.def(ability.def).name),
                    StackKind::Spell { card } => format!("{} (stack #{id})", self.name(card)),
                },
                None => format!("stack #{id} (gone)"),
            },
        }
    }
}

/// A definition with no abilities (what a permanent that lost all its
/// abilities has, CR 613.1f).
#[cold]
pub(crate) fn blank_def() -> &'static CardDef {
    static BLANK: std::sync::OnceLock<CardDef> = std::sync::OnceLock::new();
    BLANK.get_or_init(CardDef::default)
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
    /// The object dealing the damage, if one ("whenever this deals
    /// damage").
    pub obj: Option<ObjRef>,
}
