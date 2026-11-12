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
    pub starting_life: i64,
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
    /// Who chooses which player takes the first turn (CR 103.1): `None` =
    /// a player chosen at random (the first game of a match: a die roll in
    /// the game's setup); `Some(p)` = `p` (in later games of a match, the
    /// loser of the previous game; ENGINE.md 35).
    pub chooser: Option<PlayerId>,
    /// The decks are hidden within the pools (a best-of-three match, ENGINE.md
    /// 36.11): each player knows the other's pool but their first picks, not
    /// which pool cards are in the deck. Questions whose asking would depend
    /// on a deck are then asked regardless (the companion reveal), and a
    /// name is chosen among the names the chooser can know (`known_pools`).
    pub hidden_decks: bool,
    /// Hidden decklists (EoE sealed, ENGINE.md 102; needs `hidden_decks`):
    /// the other player knows nothing of a player's deck but the format and
    /// what it has seen. Clouds over a player's whole hand and library are
    /// kept (they record shown cards still in the deck), and the other
    /// player's sideboard size is hidden (out of `view_of` and the encoding).
    pub hidden_decklists: bool,
    /// Clouds over a player's whole hand and library are kept (needs
    /// `hidden_decks`): the record that cards the other player saw are still
    /// in the deck, which a dealt deck must hold. The later games of a
    /// constructed best-of-three (ENGINE.md 120: each player's 60 hidden
    /// within their public 75; the sideboard's size stays public).
    /// `hidden_decklists` keeps them too.
    pub deck_clouds: bool,
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
            chooser: None,
            hidden_decks: false,
            hidden_decklists: false,
            deck_clouds: false,
        }
    }
}

/// One card slot (ENGINE.md 2). Per-object state resets on zone changes.
/// Plain data (`Copy`): `Vec<Obj>` clones as one copy, which is most of a
/// `Game::clone` (ENGINE.md 17). Rare wide facts live in side lists on
/// `Game` (`lki_power`, `loyalty`), keeping an object at 64 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Obj {
    pub def: DefId,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub zone: Zone,
    /// The slot's incarnation (`ObjRef::inc`): counted exactly, a 2^32nd
    /// zone change aborts the game (ENGINE.md 100.7).
    pub inc: u32,
    /// CR 613.7; refreshed when entering the battlefield.
    pub timestamp: u32,
    pub tapped: bool,
    /// Marked damage (CR 120.3e): exact within `num::AMOUNT_MAX`, past it
    /// the game aborts (ENGINE.md 100.7).
    pub damage: u64,
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
    /// Times this object transformed (CR 701.27f: an ability of a permanent
    /// transforms it only if it hasn't transformed since the ability was
    /// put on the stack): counted exactly, the game aborts past `u32::MAX`
    /// (a wrapping count would alias).
    pub flips: u32,
}

/// Loyalty abilities of a permanent activated (CR 606.3; `Game::loyalty`):
/// in turn `turn`, `used` of them, `extra` more allowed than one (Comet;
/// both counted exactly, the game aborts past `u32::MAX`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Loyalty {
    pub card: CardIdx,
    pub turn: u16,
    pub used: u32,
    pub extra: u32,
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
    /// From a graveyard for a granted flashback cost equal to its mana cost
    /// (Snapcaster Mage) when the card has a printed flashback too: either
    /// may be paid (CR 702.34a; ruling a92b37a5).
    FlashbackMana,
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
    /// With a permission that taxes it (from exile): its mana cost plus {n}
    /// (Elite Spellbinder).
    Taxed(u8),
    /// From its owner's graveyard, once during their turn, with Serra
    /// Paragon's permission: for its mana cost.
    Paragon,
    /// From hand by its miracle trigger, for its miracle cost (CR 702.94a).
    Miracle,
    /// From exile, airbent: for this generic cost rather than its mana cost
    /// (an alternative cost, CR 118.9).
    Airbent(u8),
    /// From the top of its owner's library (Bolas's Citadel): life equal to
    /// its mana value rather than its mana cost (an alternative cost, CR
    /// 118.9; X is 0, 107.3b).
    LibraryTop,
    /// From the top of its owner's library by a permission to cast spells of
    /// a kind from there ("you may cast artifact spells from the top of your
    /// library", CR 601.3; EoE W6): for its costs, as from a hand.
    LibraryTopMana,
}

/// An `Enters` event's note bit 16: the times an additional cost was paid
/// or the X don't fit the note's bits 1-8 and 9-14; the permanent's
/// `Mark::Entered` holds them (ENGINE.md 100.2).
pub(crate) const ENTER_BIG: u32 = 1 << 16;

/// The objects of a game (cards, tokens, copies: `CardIdx` is a `u32`) at
/// which it ends aborted (ENGINE.md 100.7). Memory runs out long before: 2^32
/// objects of ~90 bytes are ~400 GB. The margin of 1,024 lets the running
/// task finish below `u32::MAX` (a hard assert, the backstop).
pub(crate) const OBJECT_LIMIT: usize = u32::MAX as usize - 1024;

impl Game {
    /// The times an additional cost was paid and the X a permanent entered
    /// with, from its `Enters` event's note `amount` (bits 1-8 and 9-14, or
    /// `ENTER_BIG` and the entered object's `Mark::Entered`).
    pub(crate) fn entered_counts(&self, amount: u64, obj: Option<ObjRef>) -> (u32, u32) {
        if amount & ENTER_BIG as u64 == 0 {
            return (((amount >> 1) & 0xFF) as u32, ((amount >> 9) & 0x3F) as u32);
        }
        obj.and_then(|r| {
            self.marks.iter().find_map(|&(o, m)| match m {
                Mark::Entered { times, x } if o == r => Some((times, x)),
                _ => None,
            })
        })
        .expect("a permanent's Mark::Entered for its ENTER_BIG note")
    }
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
    /// cost], [spending mana as though of any colour], [only while `only_if`
    /// holds].
    Play { free: bool, cast_only: bool, any_mana: bool, only_if: PlayIf },
    /// It has flashback; the flashback cost is its mana cost (Snapcaster
    /// Mage).
    Flashback,
    /// Its owner may play it for as long as it stays exiled; a spell cast
    /// this way costs {n} more (Elite Spellbinder; CR 601.2f).
    Taxed(u8),
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
    /// Airbent (Aang): its owner may cast it for {n} rather than its mana
    /// cost while it stays exiled.
    Airbent(u8),
    /// As `Taxed`, and a land played this way enters tapped (Lightstall
    /// Inquisitor; ENGINE.md 99.4).
    TaxedLandsTapped(u8),
}

/// When a `PermKind::Play` permission lets its player play the card (CR
/// 601.2a, 305.1: checked as the card would be played).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PlayIf {
    /// Whenever it could otherwise be played.
    Always,
    /// "During any turn you attacked with a <subtype>" (ENGINE.md 99.4).
    AttackedWith(u16),
    /// "If you control a <subtype>" (Possibility Technician; EoE W3,
    /// ENGINE.md 99.13).
    ControlSubtype(u16),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Player {
    /// The life total (CR 119.1): an amount, exact within
    /// +-`num::AMOUNT_MAX`; past it the game aborts (ENGINE.md 100.7).
    pub life: i64,
    /// Index 0 = bottom, last = top.
    pub library: Vec<CardIdx>,
    pub hand: Vec<CardIdx>,
    /// Last = top.
    pub graveyard: Vec<CardIdx>,
    pub pool: ManaPool,
    /// Lands played this turn (CR 305.2; exact: a 256th land in a turn
    /// ends the game aborted, ENGINE.md 100.7, the encoding reading it as is).
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
    pub drawn: u32,
    /// Spells cast this turn, and noncreature spells.
    pub spells_cast: u32,
    pub noncreature_cast: u32,
    /// Gained life this turn.
    pub gained_life: bool,
    /// An artifact entered the battlefield under this player's control
    /// this turn (Mechan Shieldmate; ENGINE.md 99.11).
    pub artifact_entered: bool,
    /// The life gained this turn ("if you've gained 3 or more life this
    /// turn"; EoE lane D, ENGINE.md 99.5): an amount.
    pub life_gained: i64,
    /// Can't gain life for the rest of the game (Screaming Nemesis).
    pub cant_gain_life: bool,
    /// Creatures declared as attackers this turn ("during any turn you
    /// attacked with a Rogue").
    pub attacked_with: Vec<ObjRef>,
    /// Cards drawn in this player's own draw step this turn.
    pub draw_step_draws: u32,
    /// Spells cast this game.
    pub spells_game: u32,
    /// Energy counters (CR 122.1).
    pub energy: crate::num::Count,
    /// Lost life this turn (CR 119.3; damage and payments included).
    pub lost_life: bool,
    /// "Until your next turn, you may cast sorcery spells as though they
    /// had flash."
    pub flash_sorceries: bool,
    /// Poison counters (CR 122.1f; ten or more lose the game, 704.5c).
    pub poison: crate::num::Count,
    /// Experience counters (CR 122.1).
    pub experience: crate::num::Count,
    /// "Until end of turn, you may play lands and cast spells from your
    /// graveyard" this turn (the turn number; Yawgmoth's Will).
    pub gy_play_turn: u32,
    /// "If a card would be put into your graveyard from anywhere this turn,
    /// exile that card instead" this turn (the turn number).
    pub gy_exile_turn: u32,
    /// The city's blessing (CR 702.131b: for the rest of the game).
    pub city_blessing: bool,
    /// Protection from everything until this player's turn count reaches
    /// this (CR 702.16j; The One Ring): 0 = none.
    pub prot_until: u32,
    /// The player's sideboard (CR 100.4, 400.11a: cards outside the game;
    /// in limited the rest of their pool, 100.4b), sorted by definition.
    /// Hidden from the other player but a revealed companion (ENGINE.md 35).
    pub sideboard: std::sync::Arc<[DefId]>,
    /// The companion this player revealed from their sideboard (CR 103.2b,
    /// 702.139a): a public card in no zone (`Zone::Nowhere`) until the
    /// special action puts it into their hand (116.2g). Once in the game
    /// the slot is a card like any other (hidden in a library, say).
    pub companion: Option<CardIdx>,
    /// The revealed companion's identity: public for the rest of the game.
    pub companion_def: Option<DefId>,
    /// The companion special action was taken (once per game, 116.2g).
    pub companion_used: bool,
    /// The "the next spell you cast this turn can't be countered" effects
    /// waiting for this player's next spell (Mistrise Village; CR 101.2,
    /// 611.2): each applies to the same next spell, which uses them all up as
    /// it becomes cast (601.2i); they end in the cleanup step (514.2).
    /// Counted exactly (past `u32::MAX` the game aborts). ENGINE.md 123.
    pub uncounterable_next: u32,
}

/// A player's dungeon (CR 309): the dungeon card in the command zone with the
/// venture marker on a room (Undercity's rooms 0..8, `dungeon.rs`), and the
/// dungeons the player completed (309.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Dungeon {
    pub obj: Option<CardIdx>,
    pub room: u8,
    /// Dungeons completed (counted exactly; the game aborts past
    /// `u32::MAX`).
    pub completed: u32,
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
    /// The value of X (CR 107.3a; any amount, ENGINE.md 100.2).
    pub x: u32,
    /// Divided damage: the amount for each target of the divided
    /// requirement, in the order chosen (CR 601.2d).
    pub division: Vec<u64>,
    /// What a creature this ability puts onto the battlefield attacking
    /// attacks (ninjutsu, CR 702.49c): `Some(None)` = the defending
    /// player, `Some(Some(pw))` = that planeswalker.
    pub attack_as: Option<Option<ObjRef>>,
    /// Objects a triggered ability noted as it was put on the stack: the
    /// permanents the spell that triggered it targets (Dack Fayden's
    /// emblem, "gain control of those permanents", still so if that spell
    /// is countered first). An activated ability's: the permanents tapped
    /// for its "tap an untapped <permanent>" cost (station's tapped
    /// creature, `Amount::TappedPower`).
    pub noted: Vec<ObjRef>,
    /// An ability's source's `Obj::flips` as the ability was put on the
    /// stack (CR 701.27f).
    pub flips: u32,
    /// The zone a spell was cast from (`Nowhere` for an ability).
    pub origin: Zone,
    /// The times an additional cost that may be paid repeatedly was paid
    /// (replicate, squad, offspring: `CardDef::times_cost`).
    pub times: u32,
    /// Cards spliced onto this spell (CR 702.47): their spell abilities
    /// follow its own, their targets after its own.
    pub spliced: Vec<DefId>,
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
    /// Additional combat phases directly after the current phase (CR
    /// 500.8: "after this phase", whatever phase it is; a combat phase's
    /// own and those after it, one after another).
    pub extra_combats: u32,
    /// Where the turn goes on after a combat phase with no additional one
    /// to follow: the postcombat main phase (`Main2`), or, after combat
    /// phases added to another phase (500.8), the phase that came next
    /// there: the precombat main phase (`Main1`, after the beginning
    /// phase), the ending phase (`End`, after the postcombat main phase),
    /// or the next turn (`Untap`, after the ending phase).
    pub after_combat: Step,
    /// The player who chose who takes the first turn (CR 103.1).
    pub chooser: PlayerId,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EndReason {
    /// A player lost (life, empty library, ...): see the losers' flags.
    Sba,
    /// Engine cap (`GameSpec.max_turns`).
    TurnLimit,
    /// An effect says a player wins (CR 104.2b; Thassa's Oracle) or loses
    /// (104.3e; Pact of Negation's trigger, ENGINE.md 124.1).
    Effect,
    /// The game can't go on exactly: a number past the engine's exact range
    /// (64-bit stored values, ENGINE.md 100.7) or past the object limit. Not
    /// a win, a loss or a draw: self-play drops the game (`Game::aborted`;
    /// zero-core's `SearchGame::aborted`).
    Aborted,
    /// A player conceded (CR 104.3a: it leaves the game and loses; the other
    /// player wins, 104.2a): `Game::concede`, zero-core's resign rule in a
    /// match with one episode per game (ENGINE.md 121).
    Conceded,
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

/// What a draw instruction did (`Game::draw_instruction`).
pub(crate) enum Drew {
    Done,
    /// It stopped after a card its owner is asked about revealing for
    /// miracle (CR 702.94a), with this many draws left.
    Miracle(CardIdx, u32),
    /// It waits for a question it queued (`Task::DrawOrder`).
    Asked,
}

/// The `db::left_to` bits of a 616.1 choice's results (`Dest::Choose`: bit
/// 1 the library, 2 exile, 3 exile with a void counter).
fn choice_left_to(results: u8) -> u8 {
    let mut m = 0;
    if results & 0b0010 != 0 {
        m |= crate::db::left_to::LIBRARY;
    }
    if results & 0b1_1100 != 0 {
        m |= crate::db::left_to::EXILE;
    }
    m
}

/// A triggered ability a player has for a while (until their next turn, or
/// `this_turn`: until the turn ends): trigger `slot` of definition
/// `source_def`, created by `source`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TempTrigger {
    pub controller: PlayerId,
    pub source: ObjRef,
    pub source_def: DefId,
    pub slot: u8,
    pub this_turn: bool,
    /// The object it is about ("whenever that creature ..."): only events
    /// whose first subject is that object trigger it.
    pub obj: Option<ObjRef>,
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
    /// The turn it was created in (`Turn::number`; read by
    /// `DelayedWhen::YourNextTurnEndStep`).
    pub turn: u32,
}

/// Where a moving card goes after the replacement effects of its
/// destination (`Game::replaced_dest`, CR 616.1).
pub(crate) enum Dest {
    To(Zone),
    Library,
    Exile,
    Void(crate::db::Counter),
    /// Exiled on an adventure (CR 715.4): its controller may cast the card
    /// (not as an Adventure) while it stays exiled.
    Adventure,
    /// Several results (bits: 1 library, 2 exile, 3 exile with a void
    /// counter, 4 on an adventure) and the void counter's kind: the
    /// controller chooses.
    Choose(u8, Option<crate::db::Counter>),
}

/// Where `move_card` puts a card in an ordered zone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pos {
    Top,
    Bottom,
    /// With n cards above it (0 = top; any n, the bottom past the zone's
    /// cards: CR 401.4).
    FromTop(u32),
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
    /// Cards exiled face down (`Zone::FaceDown`), both players', in the
    /// order they were exiled.
    pub(crate) face_down: Vec<CardIdx>,
    /// Face-down permanents (CR 708; manifest dread): the slot, whose `def`
    /// is the database's face-down definition, and the card it is. Its
    /// identity is hidden like a library card's (`Obj::known`, ENGINE.md
    /// 31).
    pub(crate) face_down_ids: Vec<(CardIdx, DefId)>,
    /// The next permanent put onto the battlefield enters face down
    /// (`put_onto_battlefield`, then `move_card`).
    pub(crate) entering_face_down: bool,
    pub(crate) moving_face_down: bool,
    /// Emblems (CR 114), in creation order.
    pub(crate) command: Vec<CardIdx>,
    /// Last = top.
    pub(crate) stack: Vec<StackItem>,
    pub(crate) turn: Turn,
    pub(crate) agenda: Vec<Task>,
    pub(crate) pending: Option<Decision>,
    pub(crate) answer: Option<Choice>,
    /// The page of the pending decision when its permanents page, kept by
    /// the engine (`Game::ask`: a choice naming a permanent past slot 95;
    /// ENGINE.md 100.4.8); `None` otherwise.
    pub(crate) page: Option<crate::decision::Page>,
    pub(crate) waiting: Vec<PendingTrigger>,
    pub(crate) extra_turns: Vec<PlayerId>,
    /// Combat damage assigned so far in the current damage step (amounts
    /// within `num::AMOUNT_MAX`).
    pub(crate) combat_dmg: Vec<(CardIdx, Target, u64)>,
    pub(crate) next_ts: u32,
    pub(crate) next_stack_id: u32,
    pub(crate) outcome: Option<Outcome>,
    /// Partial knowledge (ENGINE.md 10).
    pub(crate) clouds: Vec<crate::hidden::Cloud>,
    /// Tokens created so far (a cheap "any token?" for the SBA check).
    pub(crate) tokens: u32,
    /// The lowest slot made as a token or a copy (`u32::MAX`: none yet).
    /// Slots below it are cards, which are never tokens: the 704.5d check
    /// looks only from here (ENGINE.md 17).
    pub(crate) first_token: u32,
    /// A simultaneous move off the battlefield is in progress (`leaving`).
    pub(crate) batching: bool,
    /// The permanents that left in the simultaneous move in progress whose
    /// "until this leaves" cards return once it is over (CR 610.3: "a
    /// second one-shot effect is created immediately after the specified
    /// event"; `end_batch`).
    pub(crate) releasing: Vec<ObjRef>,
    /// The current event group (CR 603.2c, 700.1: one instruction of a
    /// resolving spell or ability, one simultaneous move, one cost is one
    /// event however many objects it touches): bumped as an instruction ends,
    /// a resolution begins and a batch begins or ends.
    pub(crate) ev_seq: u32,
    /// "Whenever one or more ..." abilities that triggered in event group
    /// `ev_seq`: (source, ability slot, group); one trigger per group.
    pub(crate) batch_fired: Vec<(ObjRef, u8, u32)>,
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
    /// A copy the payment search runs (`Game::payment_completes`, ENGINE.md
    /// 100.4.9): its decisions offer every option, unchecked.
    pub(crate) oracle: bool,
    /// Such a copy reached a part of a cost it can't pay: a dead end.
    pub(crate) oracle_dead: bool,
    /// Continuous effects from resolved spells and abilities (CR 611.2).
    pub(crate) effects: Vec<crate::layers::Effect>,
    /// Some effect makes a permanent lose all abilities (a fast path for
    /// [`abilities`](Self::abilities)).
    pub(crate) any_loss: bool,
    /// Counters on permanents: (card, kind, number) (CR 122).
    pub(crate) counters: Vec<(CardIdx, crate::db::Counter, crate::num::Count)>,
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
    /// ("if this is the second time this ability has resolved this turn";
    /// counted exactly).
    pub(crate) resolved: Vec<(ObjRef, u32)>,
    /// Permissions to play cards from other zones (CR 601.2a).
    pub(crate) permissions: Vec<Permission>,
    /// Combat damage is being dealt (CR 510.2).
    pub(crate) combat_damage_now: bool,
    /// A move off the battlefield that isn't a leave (a card exiled
    /// "instead" of entering, after its copy choice): no leave events.
    pub(crate) quiet_leave: bool,
    /// The monarch (CR 724).
    pub(crate) monarch: Option<PlayerId>,
    /// Counters on permanents that left the battlefield this turn, as they
    /// last existed there (CR 608.2h).
    pub(crate) lki_counters: Vec<(ObjRef, crate::db::Counter, crate::num::Count)>,
    /// ... cast with escape; cast for its dash cost (delayed ability k).
    pub(crate) entering_escaped: bool,
    /// ... the X it was cast with.
    pub(crate) entering_x: u32,
    /// ... the colours of mana spent to cast it (sunburst, CR 702.44a).
    pub(crate) entering_sunburst: u8,
    /// ... the matching cards exiled to pay its cost ("a +1/+1 counter for
    /// each instant and sorcery card exiled with it", CR 607.2q;
    /// `CardDef::counters_per_exiled`, ENGINE.md 54).
    pub(crate) entering_exiled: u8,
    /// Effects that apply to the next permanent put onto the battlefield as
    /// it enters ("it's an enchantment", "it's a Phyrexian in addition";
    /// CR 614.1c-like; set and cleared around one move).
    pub(crate) entering_mods: Vec<crate::db::Mod>,
    /// The zone the permanent whose `LeavesBattlefield` event is being
    /// matched goes to (`db::left_to` bits, `TrigPred::LeftTo`: "dies or is
    /// put into exile"; set and cleared around that event, ENGINE.md 99.11).
    pub(crate) leaving_to: u8,
    /// While the leave event of a card whose destination waits for a 616.1
    /// choice (`Task::ChooseReplacement`) is matched: the destinations it
    /// may still go to (`db::left_to` bits), and the card; 0 otherwise.
    pub(crate) leaving_choice: u8,
    pub(crate) leaving_card: CardIdx,
    /// The resolving spell, cast as an Adventure, is being put into its
    /// owner's graveyard (CR 715.4's replacement applies: `replaced_dest`).
    pub(crate) resolving_adventure: bool,
    /// The event group (`ev_seq`) of the latest object given a timestamp
    /// (entering, becoming attached) and the first timestamp given in it:
    /// objects given one in the same event got them simultaneously, and the
    /// active player orders them (CR 613.7m, `Task::OrderTimestamps`).
    pub(crate) ts_group: (u32, u32),
    /// Triggered abilities that trigger only if a pending 616.1 choice
    /// sends its card to one of their zones (`triggers::Held`), released
    /// with the answer (`release_held`).
    pub(crate) held: Vec<crate::triggers::Held>,
    /// Counters the permanent being put onto the battlefield enters with
    /// because of the effect that puts it there ("with a corpse counter on
    /// it", persist; CR 614.1c, 122.6): on it before its `Enters` event.
    pub(crate) entering_counters: Vec<(crate::db::Counter, crate::num::Count)>,
    /// The effect putting it onto the battlefield says "tapped" (CR 614.1c:
    /// it enters tapped; it doesn't become tapped).
    pub(crate) entering_tapped: bool,
    /// Mox Diamond's "you may discard a land card instead" was answered (a
    /// land was discarded): it now enters (`Task::EnterByDiscardAny`).
    pub(crate) discard_entered: bool,
    /// The replacement effects of the next move's destination are already
    /// applied (a choice among them was made, CR 616.1).
    pub(crate) repl_decided: bool,
    /// The route chosen for the cast about to begin (`Task::ChooseRoute`).
    pub(crate) route_choice: Option<crate::cast::Route>,
    /// The entering permanent's spell's delayed ability k (dash, warp) and
    /// the definition holding it: its own (`None`) or the one whose granted
    /// alternative cost it was cast for (ENGINE.md 99.15).
    pub(crate) entering_dash: Option<(u8, Option<DefId>)>,
    /// The entering permanent's spell was warped (CR 702.185): `entering_dash`
    /// is warp's delayed ability (no haste).
    pub(crate) entering_warp: bool,
    /// The card whose miracle trigger is casting it (CR 702.94a): cast for
    /// its miracle cost (`CastFrom::Miracle`).
    pub(crate) miracle_cast: Option<CardIdx>,
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
    /// Void (Edge of Eternities' ability word: "if a nonland permanent left
    /// the battlefield this turn or a spell was warped this turn"): the
    /// number of the last turn in which either happened (0: none), so void
    /// holds while it is the current turn's number (ENGINE.md 99.4).
    pub(crate) void_turn: u32,
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
    /// Who will control each player during that player's next turn (CR
    /// 722.1; the last effect created wins, 722.1a; ENGINE.md 99.14).
    pub(crate) next_turn_controller: [Option<PlayerId>; 2],
    /// The player controlling the active player during this turn (CR
    /// 722.1: from its beginning until the next turn begins).
    pub(crate) turn_controller: Option<PlayerId>,
    /// Slots made as copies, not cards (token copies, copies of spells and
    /// of cards), in increasing order.
    pub(crate) copy_objs: Vec<CardIdx>,
    /// Copiable values of permanents that left the battlefield this turn,
    /// as they last existed there (CR 608.2h).
    pub(crate) lki_copy: Vec<(ObjRef, crate::copies::CopyVals)>,
    /// The permanents that permanents leaving the battlefield this turn were
    /// attached to, as they last existed there ("enchanted creature" of an
    /// Aura that left, CR 608.2h; ENGINE.md 99.11).
    pub(crate) lki_attached: Vec<(ObjRef, ObjRef)>,
    /// Permanents that left the battlefield this turn as damage sources, as
    /// they last existed there (CR 608.2h, 113.7a; kept when
    /// `SEEN_LKI_DAMAGE`).
    pub(crate) lki_damage: Vec<(ObjRef, DamageSource)>,
    /// Permanents that left the battlefield this turn, their toughness and
    /// controller as they last existed (CR 608.2h; kept when
    /// `CardDb::watch_toughness`: stationing by toughness, ENGINE.md 99.8).
    pub(crate) lki_toughness: Vec<(ObjRef, i64, PlayerId)>,
    /// Last known power of the slots that left the battlefield (CR 608.2h),
    /// the latest departure of each: (slot, its `inc` on the battlefield,
    /// its power as it left; an amount, the game aborts past
    /// `num::AMOUNT_MAX`). Read through `Game::lki_power_of`; out of `Obj`
    /// (rare and wide: objects stay small to copy, ENGINE.md 17).
    pub(crate) lki_power: Vec<(CardIdx, u32, i64)>,
    /// Loyalty activations (CR 606.3) of the slots that have any in their
    /// current incarnation (an entry goes as its object changes zones):
    /// read through `Game::loyalty_of` (out of `Obj` like `lki_power`).
    pub(crate) loyalty: Vec<Loyalty>,
    /// Mana in the pools with a spending restriction other than Mishra's
    /// Workshop's (`ManaPool::art`): (player, type, purposes, amount), a
    /// part of that player's `ManaPool::amt` (CR 106.6; ENGINE.md 99.12).
    /// Empty but in games with such mana; emptied with the pools.
    pub(crate) pool_restr: Vec<(PlayerId, u8, crate::db::Spend, u8)>,
    /// The lands that entered the battlefield under each player's control
    /// during turn `.0` (kept when `CardDb::watch_lands_entered`; ENGINE.md
    /// 99.12).
    pub(crate) lands_entered: (u32, [u32; 2]),
    /// A card of this game says "nonbasic lands are Mountains" (its faces
    /// included; a copy copies a card of the game): the layers look
    /// (`Game::moon_on`; ENGINE.md 99.12).
    pub(crate) moon_game: bool,
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
    pub(crate) entering_times: u32,
    pub(crate) enter_note: u32,
    pub(crate) defer_enters: bool,
    /// The simultaneous entries in progress, innermost last: their
    /// permanents' `Enters` events wait to be matched together (CR 603.6a;
    /// `Game::begin_entering`).
    pub(crate) entries: Vec<crate::triggers::Entry>,
    /// A card of a looked-at pile is being put where the effect says
    /// (`Op::PileTo`); and the players whose library a replacement
    /// ("shuffle it into its owner's library instead": Blightsteel
    /// Colossus surveilled into the graveyard) shuffles once the pile's
    /// other cards are put too (CR 701.25a: one action; ENGINE.md 51).
    /// Both clear between ops.
    pub(crate) pile_moving: bool,
    pub(crate) pile_shuffles: u8,
    /// Prepared permanents (CR 722.3a) and the copy of their prepare spell
    /// in exile (722.3c).
    pub(crate) prepared: Vec<(ObjRef, CardIdx)>,
    /// The loop actions taken this turn (`loops.rs`; CR 732.2a, MTR 4.4):
    /// (player, the loop hash of the state as their decider saw it then,
    /// the action). The action is not offered to that player again at a
    /// state with the same hash: everything reachable by taking it there was
    /// reachable when they took it, so taking it again could only repeat an
    /// iteration that made no progress. Cleared as a turn begins.
    pub(crate) loop_taken: Vec<(PlayerId, u64, crate::loops::LoopAct)>,
    /// `SEEN_*` bits: a card of this game has the rare rule (ENGINE.md 29).
    pub(crate) seen: u16,
    // ---- M7b part 3b: the initiative and dungeons (ENGINE.md 27.4) ----
    /// The player who has the initiative (CR 725).
    pub(crate) initiative: Option<PlayerId>,
    /// Each player's dungeon (CR 309).
    pub(crate) dungeons: [Dungeon; 2],
    /// The objects the inherent triggered abilities of the initiative and
    /// the monarch name as their source (CR 725.2, 724.2: they have none):
    /// a slot in no zone per rules definition, made the first time one
    /// triggers.
    pub(crate) rules_srcs: Vec<CardIdx>,
    /// While the modes of a stack item are chosen, its id: it can't target
    /// itself (CR 115.5). 0 otherwise (always between tasks).
    pub(crate) choosing_for: u32,
    // ---- M7c: the single-card tail (ENGINE.md 28) ----
    /// Rare per-object facts, one list (a clone copies one empty vector
    /// in the common case): exhaust abilities activated, colours chosen
    /// as a permanent entered, ...
    pub(crate) marks: Vec<(ObjRef, Mark)>,
    /// EoE workstream W3's state (ENGINE.md 99.13): phasing, granted
    /// triggered abilities, regeneration.
    pub(crate) w3: crate::eoe_w3::W3State,
    /// EoE workstream W6's state (ENGINE.md 99.16).
    pub(crate) w6: crate::eoe_w6::W6State,
    /// With hidden decks (`GameSpec::hidden_decks`, ENGINE.md 36.11): per
    /// player, the card names of their pool the other player knows (the pool
    /// but the first picks not seen in earlier games, and the basic lands),
    /// sorted; set by the match. `None`: open decklists.
    pub(crate) known_pools: [Option<std::sync::Arc<[DefId]>>; 2],
    pub(crate) log: Option<Vec<String>>,
}

/// `Game::seen`: some card of the game has ascend; an ability discount
/// (Zirda); a discount for spells cast from outside the hand (Bilbo); a
/// retrace grant (Six); miracle, per owner (bit 4 + player).
pub(crate) const SEEN_ASCEND: u16 = 1;
pub(crate) const SEEN_ABILITY_DISCOUNT: u16 = 2;
pub(crate) const SEEN_OUTSIDE_DISCOUNT: u16 = 4;
pub(crate) const SEEN_RETRACE: u16 = 8;
pub(crate) const SEEN_MIRACLE_P0: u16 = 16;
/// (Bits 16 and 32 are miracle's.) A trigger on cards exiled from a library
/// or graveyard (Laelia).
pub(crate) const SEEN_EXILE_WATCH: u16 = 64;
/// A replacement of draws (Hullbreacher, Quantum Riddler; ENGINE.md 30).
pub(crate) const SEEN_DRAW_REPL: u16 = 128;
/// A triggered ability that functions in a graveyard (Sword of the Meek).
pub(crate) const SEEN_GY_TRIGGER: u16 = 256;
/// Triggers on +1/+1 counters put on, cards put into a library, attackers
/// unblocked (Emperor of Bones, Wan Shi Tong, Coveted Jewel).
pub(crate) const SEEN_RARE_EVENTS: u16 = 512;
/// "If a card would be put into an opponent's graveyard from anywhere,
/// instead exile it with a void counter on it" (Dauthi Voidwalker; M7d).
pub(crate) const SEEN_VOID: u16 = 1024;
/// Taxes on a chosen card name (Anointed Peacekeeper; M7d).
pub(crate) const SEEN_NAMED_TAX: u16 = 2048;
/// The top card of a library looked at any time and played (Bolas's
/// Citadel; M7d).
pub(crate) const SEEN_TOP: u16 = 4096;
/// A granted "sacrifice this: add <mana>" (Rain of Filth; M7d).
pub(crate) const SEEN_SAC_MANA: u16 = 8192;
/// "That creature deals damage" of a triggered ability, which uses the
/// creature as it last existed on the battlefield if it left (Pyrogoyf;
/// `Game::lki_damage`).
pub(crate) const SEEN_LKI_DAMAGE: u16 = 16384;
/// A trigger on a card leaving a graveyard (Murktide Regent; ENGINE.md 54).
pub(crate) const SEEN_GY_LEAVE: u16 = 32768;

/// A rare per-object fact (`Game::marks`); stale with its object (CR
/// 400.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mark {
    /// It entered with an additional cost paid more than 255 times or an
    /// X above 63, which its `Enters` event's note can't hold
    /// (`ENTER_BIG`; `Game::entered_counts`, ENGINE.md 100.2).
    Entered { times: u32, x: u32 },
    /// Entering "as a copy" while Containment Priest's replacement would
    /// exile it: decided after the copy choice (CR 614.12, 616.1).
    PriestPending,
    /// Its exhaust ability k was activated (CR 702.177a).
    Exhausted(u8),
    /// A spell with converge being paid for: the colours of the pool mana
    /// spent so far.
    ConvergePending(u8),
    /// The colours of mana spent to cast this spell (a WUBRG mask; converge,
    /// sunburst read how many: ENGINE.md 100.5.1).
    Converge(u8),
    /// The colour (index W U B R G) chosen as it entered (CR 614.12).
    ChosenColor(u8),
    /// The creature type chosen as it entered (CR 614.12, 205.3m: the index
    /// of `doomsday_b::CREATURE_TYPES_CR`; Cavern of Souls, ENGINE.md 124.2).
    ChosenCreatureType(u16),
    /// This spell can't be countered (mana spent on it said so).
    Uncounterable,
    /// It has a triggered ability an effect gave it: "when this is put into
    /// a graveyard from the battlefield [or exiled], ..." (trigger `slot`
    /// of definition `def`; Serra Paragon, earthbend).
    LeaveTrigger { def: DefId, slot: u8, exile: bool },
    /// This spell, cast with Serra Paragon's permission, gives the permanent
    /// it becomes that trigger.
    GrantOnEnter { def: DefId, slot: u8 },
    /// Its "once during each of your turns" was used in turn n.
    UsedTurn(u16),
    /// This spell was cast with a permission for a spell with mana value n
    /// or less (Serra Paragon, Lurrus): X is announced so that it stays so
    /// (CR 202.3e, 601.2b; ruling 90a8974d).
    MvCap(u8),
    /// It has been harnessed (The Mind Stone).
    Harnessed,
    /// Its boast ability was activated in turn n (CR 702.142a).
    Boasted(u16),
    /// It entered as a spell whose kicker {X} was paid with this X.
    KickerX(u32),
    /// It became saddled in turn n (CR 702.171b: until end of turn).
    Saddled(u16),
    /// The modes (a bit each) chosen so far for its "choose one that hasn't
    /// been chosen" ability.
    ModesUsed(u8),
    /// "Odd" (1) or "even" (0) chosen as it entered (Gollum).
    Parity(u8),
    /// This reanimating Aura put this creature onto the battlefield (and
    /// enchants it).
    Reanimated(ObjRef),
    /// This spell was cast when a sorcery couldn't have been (Necromancy).
    NotSorceryTiming,
    /// The triggered ability (`slot` of `def`) a static ability grants this
    /// object triggered `n` times in turn `turn` (M7d: Nadu's "only twice
    /// each turn"), granted by the object `src` (the granting permanent: a
    /// Nadu that left and came back grants a new ability, whose count starts
    /// anew, CR 400.7, ruling 96e9a886; another Nadu is another object even
    /// with the same incarnation number, ENGINE.md 71.2).
    Triggered { def: DefId, slot: u8, turn: u16, n: u8, src: ObjRef },
    /// It looked at an opponent's hand as it entered (Anointed Peacekeeper:
    /// the name is chosen next).
    LookedHand,
    /// The card name chosen as it entered (a definition; `NO_NAME`: a name
    /// of no card in the game).
    ChosenName(DefId),
    /// It was exerted (CR 701.39): it won't untap during its controller's
    /// next untap step.
    Exerted,
    /// Mana that gives haste was spent on this creature spell (Arena of
    /// Glory): the permanent it becomes gains haste until end of turn.
    HasteMana,
    /// A mana source that sacrifices itself, chosen to be exiled by the
    /// delve of the spell being cast once its mana ability is activated
    /// (CR 601.2g before 601.2h; ENGINE.md 18).
    DelveLate,
    /// A graveyard card chosen for the delve of the spell being cast: exiled
    /// as the total cost is paid, after the mana abilities (CR 601.2g-h,
    /// 702.66a), as the payment ends (ENGINE.md 51).
    DelveExile,
    /// A hand card chosen for the delve of the spell being cast that the
    /// spell's cost discards (retrace's land card): exiled once discarded
    /// (CR 601.2h: the discard paid before the mana; ENGINE.md 100.4.9).
    DelvePromised,
    /// On a spell whose permanent enters with a counter for each card
    /// exiled with it (Murktide Regent, `CardDef::counters_per_exiled`): a
    /// card (in exile, this object) its delve exiled to pay its cost (CR
    /// 607.2q; ENGINE.md 54).
    ExiledToPay(ObjRef),
    /// It was dealt damage in turn n ("if that creature wasn't dealt damage
    /// this turn"; kept only when a card of the table reads it,
    /// `CardDb::watch_damage`; EoE lane D, ENGINE.md 99.5).
    DealtDamage(u16),
    /// It has triggered ability `slot` of definition `def`, which an effect
    /// gave it ("<it> gains '<triggered ability>'", CR 613.1f: layer 6), until
    /// end of turn (`this_turn`: it ends in the cleanup step, 514.2) or for
    /// as long as it stays this object (400.7). It is the permanent's own
    /// ability: its source is the permanent and its controller the
    /// permanent's (113.7, 113.8). `ts`: its timestamp (613.7a): a layer-6
    /// "loses all abilities" effect with a later one removes it, one with an
    /// earlier one doesn't (613.1f, 613.7). EoE lane D's "when this creature
    /// dies" (ENGINE.md 99.5), generalised by workstream W3 (99.13).
    Granted { def: DefId, slot: u8, this_turn: bool, ts: u32 },
    /// It entered as a spell cast for its warp cost (CR 702.185a: "if that
    /// creature was cast for its warp cost"; ENGINE.md 99.8).
    WarpCast,
    /// A regeneration shield (CR 701.19a): the next time it would be
    /// destroyed this turn, instead its damage is removed, its controller
    /// taps it and it is removed from combat. One mark per shield; they end
    /// in the cleanup step (514.2). EoE W3, ENGINE.md 99.13.
    RegenShield,
    /// The amount of mana spent to cast it (CR 601.2h): on a spell as it is
    /// paid, then on the permanent it becomes ("the amount of mana spent to
    /// cast this creature"; EoE W3, kept for cards that read it).
    ManaSpent(u32),
    /// This spell was cast for alternative cost `grant` of definition `def`,
    /// which permanent `granter`'s static ability gave it in its owner's hand
    /// (`CastingRules::hand_alt_grants`; CR 118.9, 400.7g; ENGINE.md 99.15).
    AltGranted { granter: ObjRef, def: DefId, grant: u8 },
    /// A spell cast from its caster's hand without paying its mana cost
    /// (its `CastFrom` is then `eoe_c::FREE_CAST`): not cast from anywhere
    /// other than the hand (CR 106.6; ENGINE.md 99.12).
    FreeFromHand,
}

/// `Mark::ChosenName` of a name no card of the game has.
pub(crate) const NO_NAME: DefId = DefId::MAX;

impl Game {
    /// A new game without sideboards ([`with_sideboards`](Self::with_sideboards)).
    pub fn new(db: &'static CardDb, spec: GameSpec, decks: [&[DefId]; 2], seed: u64) -> Result<Game, String> {
        Game::with_sideboards(db, spec, decks, [&[], &[]], seed)
    }

    /// A new game (CR 103; ENGINE.md 5, 35): `decks[p]` are player p's
    /// cards (40 for limited), `sideboards[p]` the cards outside the game
    /// (CR 100.4b: the rest of their pool). The chooser (random, or
    /// `spec.chooser`) is determined and the game runs to the first
    /// decision: the chooser's play-or-draw choice (103.1). The rest of the
    /// setup follows it (`Task::PlayDraw`): companions (103.2b), shuffles
    /// (103.3), opening hands and mulligans (103.5), opening-hand actions
    /// (103.6).
    pub fn with_sideboards(
        db: &'static CardDb,
        spec: GameSpec,
        decks: [&[DefId]; 2],
        sideboards: [&[DefId]; 2],
        seed: u64,
    ) -> Result<Game, String> {
        for (p, d) in decks.iter().enumerate() {
            if d.is_empty() {
                return Err(format!("player {p} has an empty deck"));
            }
            if let Some(&bad) = d.iter().chain(sideboards[p].iter()).find(|&&id| id as usize >= db.len() || db.def(id).token) {
                return Err(format!("player {p}: {bad} is not a card"));
            }
        }
        if spec.chooser.is_some_and(|p| p > 1) {
            return Err("chooser must be player 0 or 1".into());
        }
        if spec.hidden_decklists && !spec.hidden_decks {
            return Err("hidden_decklists needs hidden_decks (ENGINE.md 102)".into());
        }
        if spec.deck_clouds && !spec.hidden_decks {
            return Err("deck_clouds needs hidden_decks (ENGINE.md 120)".into());
        }
        // (Per-object turn numbers are u16: `Obj::entered`, `pw_turn`,
        // marks; ENGINE.md 46.)
        if spec.max_turns > u16::MAX as u32 {
            return Err(format!("max_turns {} above {} (turn numbers are u16)", spec.max_turns, u16::MAX));
        }
        let mut g = Game::empty(db, spec, seed);
        for p in 0..2u8 {
            for &def in decks[p as usize] {
                let idx = g.add_card(def, p, Zone::Library);
                g.players[p as usize].library.push(idx);
            }
            // CR 103.2a: the sideboard is set aside; what is left is the
            // starting deck.
            let mut sb = sideboards[p as usize].to_vec();
            sb.sort_unstable();
            g.players[p as usize].sideboard = std::sync::Arc::from(sb);
        }
        // CR 103.1: the chooser (the first game of a match: chosen at
        // random) chooses who takes the first turn. The die roll is part of
        // the initial state (no transition reveals it; it is public).
        let chooser = spec.chooser.unwrap_or_else(|| g.rng.below(2) as PlayerId);
        g.turn.chooser = chooser;
        g.turn.starting = chooser;
        g.turn.active = chooser;
        g.turn.priority = chooser;
        g.agenda.push(Task::PlayDraw { p: chooser });
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
            artifact_entered: false,
            life_gained: 0,
            cant_gain_life: false,
            attacked_with: Vec::new(),
            draw_step_draws: 0,
            spells_game: 0,
            energy: 0,
            lost_life: false,
            flash_sorceries: false,
            poison: 0,
            experience: 0,
            gy_play_turn: 0,
            gy_exile_turn: 0,
            city_blessing: false,
            prot_until: 0,
            sideboard: std::sync::Arc::from(Vec::new()),
            companion: None,
            companion_def: None,
            companion_used: false,
            uncounterable_next: 0,
        };
        Game {
            db,
            spec,
            rng: Rng::new(seed),
            objs: Vec::new(),
            players: [player.clone(), player],
            battlefield: Vec::new(),
            exile: Vec::new(),
            face_down: Vec::new(),
            face_down_ids: Vec::new(),
            entering_face_down: false,
            moving_face_down: false,
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
                extra_combats: 0,
                after_combat: Step::Main2,
                chooser: 0,
            },
            agenda: Vec::new(),
            pending: None,
            answer: None,
            page: None,
            waiting: Vec::new(),
            extra_turns: Vec::new(),
            combat_dmg: Vec::new(),
            next_ts: 1,
            next_stack_id: 1,
            outcome: None,
            clouds: Vec::new(),
            tokens: 0,
            first_token: u32::MAX,
            observer: None,
            keys: Vec::new(),
            forced: Vec::new(),
            forced_at: 0,
            decisions: 0,
            no_auto: false,
            oracle: false,
            oracle_dead: false,
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
            quiet_leave: false,
            monarch: None,
            lki_counters: Vec::new(),
            entering_escaped: false,
            entering_x: 0,
            entering_sunburst: 0,
            entering_exiled: 0,
            entering_mods: Vec::new(),
            leaving_to: 0,
            leaving_choice: 0,
            leaving_card: CardIdx::new(0),
            resolving_adventure: false,
            ts_group: (0, 0),
            held: Vec::new(),
            entering_counters: Vec::new(),
            entering_tapped: false,
            discard_entered: false,
            repl_decided: false,
            route_choice: None,
            entering_dash: None,
            entering_warp: false,
            miracle_cast: None,
            entering_face: None,
            day: None,
            done_once: Vec::new(),
            discarded: Vec::new(),
            void_turn: 0,
            temp_triggers: Vec::new(),
            gy_exile: Vec::new(),
            copies: Vec::new(),
            next_turn_controller: [None; 2],
            turn_controller: None,
            copy_objs: Vec::new(),
            lki_copy: Vec::new(),
            lki_attached: Vec::new(),
            lki_damage: Vec::new(),
            lki_toughness: Vec::new(),
            lki_power: Vec::new(),
            loyalty: Vec::new(),
            pool_restr: Vec::new(),
            lands_entered: (0, [0; 2]),
            moon_game: false,
            spell_lki: Vec::new(),
            entering_copy: None,
            entering_times: 0,
            enter_note: 0,
            defer_enters: false,
            entries: Vec::new(),
            pile_moving: false,
            pile_shuffles: 0,
            prepared: Vec::new(),
            loop_taken: Vec::new(),
            seen: 0,
            initiative: None,
            dungeons: [Dungeon::default(); 2],
            rules_srcs: Vec::new(),
            choosing_for: 0,
            marks: Vec::new(),
            w3: Default::default(),
            w6: Default::default(),
            known_pools: [None, None],
            batching: false,
            releasing: Vec::new(),
            ev_seq: 0,
            batch_fired: Vec::new(),
            entering_cast: false,
            entering_evoked: false,
            log: if spec.log { Some(Vec::new()) } else { None },
        }
    }

    /// Slot `idx` was just made as a token or a copy (CR 111, 707.10).
    pub(crate) fn token_slot_made(&mut self, idx: CardIdx) {
        self.tokens = self.exact_inc(self.tokens, "tokens made");
        self.first_token = self.first_token.min(idx.get());
    }

    /// Append a card slot (not yet in any zone list). At `OBJECT_LIMIT`
    /// objects the game ends aborted (ENGINE.md 100.7); the running task
    /// still finishes, within the margin to `u32::MAX` (the hard assert).
    pub(crate) fn add_card(&mut self, def: DefId, owner: PlayerId, zone: Zone) -> CardIdx {
        if self.objs.len() >= OBJECT_LIMIT {
            let n = self.objs.len();
            self.abort(|| format!("{n} objects: past the engine's object limit"));
        }
        assert!(self.objs.len() < u32::MAX as usize, "too many objects");
        let idx = CardIdx::new(self.objs.len() as u32);
        // Rare rules some card of this game has (its faces included; a copy
        // copies a card of the game): the hot paths look only if one does.
        let d = self.db.def(def);
        let seen = d.seen_bits() | d.back.map_or(0, |b| self.db.def(b).seen_bits());
        self.seen |= seen;
        self.w3.seen |= d.w3.seen | d.back.map_or(0, |b| self.db.def(b).w3.seen);
        self.w6.seen |= d.w6.seen | d.back.map_or(0, |b| self.db.def(b).w6.seen);
        if d.miracle.is_some() {
            self.seen |= SEEN_MIRACLE_P0 << owner;
        }
        if d.nonbasic_lands_are_mountains || d.back.is_some_and(|b| self.db.def(b).nonbasic_lands_are_mountains) {
            self.moon_game = true;
        }
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
            flashback: false,
            escaped: false,
            chosen_player: None,
            monstrous: false,
            attacks: None,
            entered: 0,
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
    /// The game ended aborted (`EndReason::Aborted`, ENGINE.md 100.7): it has
    /// no result, and self-play drops it.
    pub fn aborted(&self) -> bool {
        matches!(self.outcome, Some(Outcome { reason: EndReason::Aborted, .. }))
    }
    /// The game ends with `winner` (`None`: a draw) for `reason`, unless it
    /// already ended: a game aborted before (a number past the exact range
    /// could have decided this) stays aborted, and a game that ended before an
    /// overflow keeps its exact result.
    pub(crate) fn end_game(&mut self, winner: Option<PlayerId>, reason: EndReason) {
        if self.outcome.is_none() {
            self.outcome = Some(Outcome { winner, reason });
        }
    }
    /// Player `p` concedes (CR 104.3a: a player can concede the game at any
    /// time; it leaves the game and loses it): the other player wins (104.2a),
    /// unless the game already ended. ENGINE.md 121: zero-core's resign rule.
    pub fn concede(&mut self, p: PlayerId) {
        self.end_game(Some(crate::ids::opp(p)), EndReason::Conceded);
    }
    /// The game ends aborted (ENGINE.md 100.7): `why` (logged) is the number
    /// or limit the engine can't keep exact. The running task finishes with
    /// whatever value it had (the game is dropped); `advance` stops after it.
    pub(crate) fn abort(&mut self, why: impl FnOnce() -> String) {
        if self.outcome.is_none() {
            let w = why();
            self.note(|_| format!("game aborted: {w}"));
            self.outcome = Some(Outcome { winner: None, reason: EndReason::Aborted });
        }
    }
    /// Void holds: a nonland permanent left the battlefield this turn or a
    /// spell was warped this turn (ENGINE.md 99.4).
    pub fn void_active(&self) -> bool {
        self.void_turn != 0 && self.void_turn == self.turn.number
    }

    pub fn turn(&self) -> &Turn {
        &self.turn
    }
    pub fn player(&self, p: PlayerId) -> &Player {
        &self.players[p as usize]
    }
    pub fn life(&self, p: PlayerId) -> i64 {
        self.players[p as usize].life
    }
    pub fn obj(&self, idx: CardIdx) -> &Obj {
        &self.objs[idx.i()]
    }
    pub fn objs(&self) -> &[Obj] {
        &self.objs
    }
    /// Object `r`'s power as it last existed on the battlefield (CR
    /// 608.2h), if `r` is its slot's latest incarnation there to leave.
    pub fn lki_power_of(&self, r: ObjRef) -> Option<i64> {
        self.lki_power.iter().find(|x| x.0 == r.idx).and_then(|&(_, inc, p)| (inc == r.inc).then_some(p))
    }
    /// Slot `idx` left the battlefield as incarnation `inc`, its power `p`.
    pub(crate) fn set_lki_power(&mut self, idx: CardIdx, inc: u32, p: i64) {
        match self.lki_power.iter_mut().find(|x| x.0 == idx) {
            Some(x) => *x = (idx, inc, p),
            None => self.lki_power.push((idx, inc, p)),
        }
    }
    /// Permanent `c`'s loyalty activations (CR 606.3); none: all 0.
    pub fn loyalty_of(&self, c: CardIdx) -> Loyalty {
        self.loyalty.iter().find(|l| l.card == c).copied().unwrap_or(Loyalty { card: c, ..Loyalty::default() })
    }
    /// Permanent `c`'s loyalty activations, to change (an entry of all 0
    /// made if it has none).
    pub(crate) fn loyalty_mut(&mut self, c: CardIdx) -> &mut Loyalty {
        let i = match self.loyalty.iter().position(|l| l.card == c) {
            Some(i) => i,
            None => {
                self.loyalty.push(Loyalty { card: c, ..Loyalty::default() });
                self.loyalty.len() - 1
            }
        };
        &mut self.loyalty[i]
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
        self.supertypes_of(idx) & crate::db::supertypes::LEGENDARY != 0
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
            Zone::FaceDown => Some(&mut self.face_down),
            Zone::Command => Some(&mut self.command),
            Zone::PhasedOut => Some(&mut self.w3.phased),
            Zone::Stack | Zone::Nowhere => None,
        }
    }

    /// Move a card to another zone: the one place zone lists change. Bumps
    /// the incarnation (CR 400.7), resets per-object state, updates
    /// knowledge, emits the reveal key and the zone-change event. A spell
    /// leaving the stack loses its stack item. Battlefield entry goes through
    /// [`put_onto_battlefield`](Self::put_onto_battlefield) (controller).
    pub(crate) fn move_card(&mut self, idx: CardIdx, to: Zone, pos: Pos) -> ObjRef {
        let r = self.move_card_ctl(idx, to, pos, None);
        // (An Aura's control effect may have begun or ended: EoE W6.)
        if self.w6.seen & crate::eoe_w6::SEEN_W6_CONTROL != 0 {
            self.w6_sync_control();
        }
        r
    }

    pub(crate) fn move_card_to_stack(&mut self, idx: CardIdx, controller: PlayerId) -> ObjRef {
        self.move_card_ctl(idx, Zone::Stack, Pos::Top, Some(controller))
    }

    /// Put a card onto the battlefield, applying "enters tapped" (CR
    /// 614.1c, decided before it moves) and queueing "as this enters"
    /// choices (614.12, a task that runs next).
    pub(crate) fn put_onto_battlefield(&mut self, idx: CardIdx, controller: PlayerId) -> ObjRef {
        self.put_onto_battlefield_attached(idx, controller, None)
    }

    /// [`put_onto_battlefield`](Self::put_onto_battlefield) of an Aura that
    /// enters attached to `attach` (CR 303.4f, chosen as it enters). Its
    /// `Enters` event is matched with the others of its entry: an effect
    /// putting several permanents onto the battlefield at once begins one
    /// around them (CR 603.6a; `Game::begin_entering`), else it is an entry
    /// of its own.
    pub(crate) fn put_onto_battlefield_attached(&mut self, idx: CardIdx, controller: PlayerId, attach: Option<ObjRef>) -> ObjRef {
        self.begin_entering(false);
        let r = self.enter_permanent(idx, controller, attach);
        self.end_entering();
        r
    }

    /// [`put_onto_battlefield_attached`](Self::put_onto_battlefield_attached)
    /// from a task its entry queued as a permanent entered (CR 614.1a, 303.4f:
    /// what enters instead, or an Aura once it has something to enchant): it
    /// enters with that entry's permanents.
    pub(crate) fn put_onto_battlefield_joining(&mut self, idx: CardIdx, controller: PlayerId, attach: Option<ObjRef>) -> ObjRef {
        self.begin_entering(true);
        let r = self.enter_permanent(idx, controller, attach);
        self.end_entering();
        r
    }

    fn enter_permanent(&mut self, idx: CardIdx, controller: PlayerId, attach: Option<ObjRef>) -> ObjRef {
        let is_card = idx.get() < self.first_token;
        // Mox Diamond: "If this would enter, you may discard a land card
        // instead. If you do, put it onto the battlefield. If you don't, put
        // it into its owner's graveyard" (CR 614.1a), however it would enter
        // (cast, the choice is made as the spell resolves: `EnterByDiscard`).
        // Decided before it enters: the card waits revealed in no zone and
        // `Task::EnterByDiscardAny` asks its would-be controller.
        if self.def_of(idx).discard_land_to_enter
            && !self.entering_cast
            && !self.entering_face_down
            && !std::mem::take(&mut self.discard_entered)
        {
            let tapped = std::mem::take(&mut self.entering_tapped);
            self.entering_counters.clear();
            let from = self.objs[idx.i()].zone;
            let owner = self.objs[idx.i()].owner;
            let def = self.db.def(self.objs[idx.i()].def).card_def(self.objs[idx.i()].def);
            self.note(|g| format!("{} would enter: a land card may be discarded", g.name(idx)));
            let r = self.move_card(idx, Zone::Nowhere, Pos::Top);
            if from.is_hidden() {
                // (Revealed as it leaves a hidden zone, as for any public
                // destination.)
                self.emit_public(KEY_REVEAL, owner, def as u64);
            }
            self.agenda.push(Task::EnterByDiscardAny { card: idx, controller, tapped });
            return r;
        }
        // CR 304.4, 307.4: an instant or sorcery card can't enter the
        // battlefield; it stays where it is (Flickerwisp returning a
        // manifested Lightning Bolt; ENGINE.md 38). A manifested one enters
        // face down (701.40a); a face an effect names may be a land (712.12).
        if !self.entering_face_down {
            let d = self.entering_face.unwrap_or(self.objs[idx.i()].def);
            if self.db.def(d).is(types::INSTANT | types::SORCERY) {
                self.entering_cast = false;
                self.clear_entering();
                self.note(|g| format!("{} can't enter the battlefield", g.name(idx)));
                return self.obj_ref(idx);
            }
        }
        // Face down (CR 708; manifest): a 2/2 with no text from the moment it
        // enters; the card it is waits in `face_down_ids` (ENGINE.md 31).
        if std::mem::take(&mut self.entering_face_down) {
            let real = self.objs[idx.i()].def;
            self.face_down_ids.push((idx, real));
            self.objs[idx.i()].def = self.db.face_down_def();
            self.moving_face_down = true;
            self.entering_face = None;
        }
        // The face it enters with (CR 712.13-14): the one it had on the
        // stack, its front, or the face an effect names ("return it
        // transformed", 712.14a; an MDFC's land face, 712.12); a daybound
        // card enters transformed at night (702.145b). Set before it moves,
        // so what applies as it enters sees that face (CR 614.12).
        // Daybound's "if it is night and this permanent is represented by a
        // double-faced card, it enters transformed" is the permanent's own
        // ability, read from it as it would exist on the battlefield
        // (614.12): not a token (a double-faced token enters front face up
        // and transforms at once, 702.145c: `day_night_check` after its
        // `Enters` event), and not one entering with no abilities (Xu-Ifit:
        // it stays front face up, with no daybound).
        let has_daybound = !self.entering_mods.contains(&crate::db::Mod::LoseAllAbilities) && self.def_of(idx).daybound;
        if let Some(face) = self.entering_face.take() {
            self.objs[idx.i()].def = face;
        } else if self.day == Some(false) && is_card && has_daybound {
            if let Some(b) = self.def_of(idx).back {
                self.objs[idx.i()].def = b;
            }
        }
        // CR 614.1a: "If a nontoken creature would enter and it wasn't
        // cast, exile it instead."
        let cast = std::mem::take(&mut self.entering_cast);
        let evoked = std::mem::take(&mut self.entering_evoked);
        let times = std::mem::take(&mut self.entering_times);
        // CR 303.4f-g: an Aura put onto the battlefield other than by
        // resolving as an Aura spell: its controller chooses, as it enters,
        // a legal object for its enchant ability (not targeted); with none
        // it stays in its zone. The cube's one such Aura is Animate Dead
        // ("enchant creature card in a graveyard"; returned by Portable Hole,
        // Leyline Binding, Flickerwisp, Phelia; ruling aff80611, ENGINE.md
        // 39). (Its ability then returns that card, 603.4.)
        let mut attach = attach;
        if !cast && attach.is_none() && self.def_of(idx).aura_spell && !self.moving_face_down {
            let cands = self.aura_candidates(idx, controller);
            if cands.len() != 1 {
                let tapped = std::mem::take(&mut self.entering_tapped);
                self.clear_entering();
                if cands.is_empty() {
                    self.note(|g| format!("{} has nothing to enchant and stays where it is", g.name(idx)));
                } else {
                    // The card is being put onto the battlefield, a public
                    // zone, and what it enchants is chosen as it enters: it
                    // is revealed now, from a hand or a library, so every
                    // player's worlds keep it while the choice is pending
                    // (`determinize` deals only cards a viewer doesn't know;
                    // the question names this card; ENGINE.md 10, 100.5.1:
                    // EoE fuzz seed 10071, Anticausal Vestige putting Tractor
                    // Beam from the hand).
                    self.reveal(idx);
                    self.agenda.push(Task::EnterAura { card: idx, controller, tapped, page: crate::decision::Page::FIRST });
                }
                return self.obj_ref(idx);
            }
            attach = Some(self.obj_ref(cands[0]));
        }
        let mut priest_after_copy = false;
        if !cast && self.def_of(idx).is(types::CREATURE) && !self.is_token(idx) {
            // Only a permanent on the battlefield before this event applies:
            // not one entering at the same time (Exhume returning Containment
            // Priest; CR 614.12a, its ruling), as for `EnterAsCopy`.
            let since = self.entry_since();
            let replaced =
                self.battlefield.iter().any(|&c| self.abilities(c).exiles_uncast_creatures && self.objs[c.i()].timestamp < since);
            if replaced && self.def_of(idx).enters_as_copy.is_some() {
                // "You may have this enter as a copy of ..." is a
                // replacement effect too: its controller applies it first
                // (CR 616.1), and the Priest's applies only if what would
                // enter is still a creature (614.12: Phyrexian Metamorph
                // reanimated as a copy of Sol Ring enters). Decided after
                // the copy choice (`Mark::PriestPending`,
                // `Game::enter_as_copy`; ENGINE.md 38).
                priest_after_copy = true;
            } else if replaced {
                self.entering_copy = None;
                // (A card that would have entered face down is exiled face
                // up: it never was a permanent.)
                if std::mem::take(&mut self.moving_face_down) {
                    if let Some(i) = self.face_down_ids.iter().position(|x| x.0 == idx) {
                        let (_, real) = self.face_down_ids.remove(i);
                        self.objs[idx.i()].def = real;
                    }
                }
                self.note(|g| format!("{} is exiled instead (not cast)", g.name(idx)));
                return self.move_card(idx, Zone::Exile, Pos::Top);
            }
        }
        // "You may have this enter as a copy of ..." (CR 707.5): the choice
        // comes first (`Task::EnterAsCopy`), then what it entered as enters.
        // An effect that puts it onto the battlefield with no abilities
        // ("It's a Skeleton ... and has no abilities": Xu-Ifit) applies as it
        // enters: its own abilities that modify how it enters (enters tapped,
        // with counters, as a copy, "as it enters" choices) don't apply, and
        // its "when this enters" abilities don't trigger (CR 614.12; Xu-Ifit's
        // ruling of 2025-07-25; ENGINE.md 99.11).
        let own = !self.entering_mods.contains(&crate::db::Mod::LoseAllAbilities);
        // (A nonbasic land entering as a Mountain too: CR 305.7, 614.12;
        // ENGINE.md 99.12.)
        let copy_choice = own && self.def_of(idx).enters_as_copy.is_some() && !(self.moon_game && self.moon_entering(idx));
        let fx_tapped = std::mem::take(&mut self.entering_tapped);
        let fx_counters = std::mem::take(&mut self.entering_counters);
        let tapped = !copy_choice && self.enters_tapped_own(idx, controller, own);
        self.entering_evoked = evoked; // read as it moves (its ETB sees it)
        let escaped = std::mem::take(&mut self.entering_escaped);
        let dash = std::mem::take(&mut self.entering_dash);
        let x = std::mem::take(&mut self.entering_x);
        self.entering_escaped = escaped && cast;
        // Its `Enters` event says whether it was cast and how many times an
        // additional cost was paid ("if you cast it", squad, offspring).
        // (A copy of a spell resolving wasn't cast, CR 707.10.)
        // (Bits 9-14: the spell's X, for ravenous, CR 702.156a; bit 15: it
        // came from a graveyard or exile, "enters from anywhere other than".
        // Times past 255 or an X past 63: bit 16, `ENTER_BIG`, and both kept
        // exactly in a `Mark::Entered` of the permanent; ENGINE.md 100.2.)
        let from_gy_exile = matches!(self.objs[idx.i()].zone, Zone::Graveyard | Zone::Exile);
        let small = times <= 0xFF && x <= 0x3F;
        let counts = if small { times << 1 | x << 9 } else { ENTER_BIG };
        let note = (cast && !self.is_copy_obj(idx)) as u32 | counts | (from_gy_exile as u32) << 15;
        self.enter_note = note;
        // (Its `Enters` event comes after what applies as it enters: "enters
        // tapped", "enters with counters", CR 614.1c, 122.6.)
        self.defer_enters = true;
        let r = self.move_card_ctl(idx, Zone::Battlefield, Pos::Top, Some(controller));
        if !small {
            self.marks.push((r, Mark::Entered { times, x }));
        }
        // (Its "enters with" counters: EoE W6, `W6State::entering`.)
        let entering_before = self.w6.entering.replace((idx, self.entry_since()));
        if let Some(h) = attach {
            self.objs[idx.i()].attached = Some(h);
            self.note(|g| format!("{} enters attached to {}", g.name(idx), g.name(h.idx)));
        }
        // (Control effects: EoE W6.)
        if self.w6.seen & crate::eoe_w6::SEEN_W6_CONTROL != 0 {
            self.w6_entered_under(r, controller);
            self.w6_sync_control();
        }
        if fx_tapped {
            self.objs[idx.i()].tapped = true;
        }
        for (k, n) in fx_counters {
            self.add_counters(idx, k, n);
        }
        let warp = std::mem::take(&mut self.entering_warp);
        // (A copy of a spell isn't cast, CR 707.10: not "cast for its warp
        // cost"; ENGINE.md 99.15.)
        if warp && cast && !self.is_copy_obj(idx) {
            self.marks.push((r, Mark::WarpCast));
        }
        // (The mana spent to cast it: EoE W3.)
        if let Some(n) = self.w3.entering_spent.take() {
            if cast {
                self.w3_entered_spent(idx, r, n);
            }
        }
        if let (Some((k, home)), true) = (dash, cast) {
            // CR 702.109a: it gains haste, and returns to its owner's hand
            // at the beginning of the next end step. (Warp, 702.185a: no
            // haste; exiled at the next end step.)
            if !warp {
                let mut haste = crate::db::KwSet::default();
                haste.insert(Kw::Haste);
                self.add_effect(
                    crate::layers::Affected::Obj(r),
                    crate::db::Mod::Keywords(haste),
                    crate::layers::Until::Forever,
                    controller,
                );
            }
            // (A granted warp's exile is the granting definition's delayed
            // ability; its source is still this permanent, CR 603.7d.)
            let def = home.unwrap_or(self.objs[idx.i()].def);
            self.delayed.push(Delayed {
                when: crate::db::DelayedWhen::NextEndStep,
                controller,
                source: r,
                source_def: def,
                slot: self.db.def(def).delayed_base + k,
                ev: crate::triggers::EvCtx { obj: Some(r), ..Default::default() },
                turn: self.turn.number,
            });
        }
        if copy_choice {
            if priest_after_copy {
                self.marks.push((r, Mark::PriestPending));
            }
            self.agenda.push(Task::EnterAsCopy { card: idx, note, since: self.entry_since(), page: crate::decision::Page::FIRST });
            self.w6.entering = entering_before;
            return r;
        }
        self.entering_own(idx, tapped, x, own);
        // Multikicker (CR 702.33c): a +1/+1 counter for each time it was
        // kicked; kicker {X}: its X, for "mana value X or less".
        if cast && times > 0 {
            if self.def_of(idx).enters_times_counters {
                self.add_counters(idx, crate::db::Counter::P1P1, times as crate::num::Count);
            }
            if self.def_of(idx).times_cost.as_ref().is_some_and(|t| t.x) {
                self.marks.push((r, Mark::KickerX(x)));
            }
        }
        self.w6.entering = entering_before;
        if self.objs[idx.i()].zone == Zone::Battlefield && self.objs[idx.i()].inc == r.inc {
            self.entered_event(r, note);
        }
        r
    }

    /// Forget what the effect putting a card onto the battlefield set up
    /// (it doesn't enter after all).
    fn clear_entering(&mut self) {
        self.entering_copy = None;
        self.entering_counters.clear();
        let _ = std::mem::take(&mut self.entering_dash);
        self.entering_escaped = false;
        self.entering_evoked = false;
        self.entering_face = None;
        let _ = std::mem::take(&mut self.entering_sunburst);
        let _ = std::mem::take(&mut self.entering_exiled);
        self.entering_tapped = false;
        let _ = std::mem::take(&mut self.entering_times);
        let _ = std::mem::take(&mut self.entering_warp);
        let _ = std::mem::take(&mut self.entering_x);
    }

    /// The legal objects for an Aura entering without being cast to
    /// enchant (CR 303.4f) under `p`'s control: Animate Dead's "creature card
    /// in a graveyard", or the permanents its enchant ability allows
    /// (`aura_enchants`, ENGINE.md 99.2).
    pub(crate) fn aura_candidates(&self, aura: CardIdx, p: PlayerId) -> Vec<CardIdx> {
        if !self.def_of(aura).reanimates {
            return self.battlefield.iter().copied().filter(|&h| self.aura_enchants(aura, p, h)).collect();
        }
        (0..2u8).flat_map(|p| self.graveyard_cards(p)).filter(|&c| c != aura && self.def_of(c).is(types::CREATURE)).collect()
    }

    /// `Task::EnterAura`: the Aura's controller chooses what it enchants;
    /// it enters attached to that (CR 303.4f).
    pub(crate) fn enter_aura(&mut self, card: CardIdx, p: PlayerId, tapped: bool, page: crate::decision::Page) {
        match self.answer.take() {
            Some(Choice::Page(next)) => self.agenda.push(Task::EnterAura { card, controller: p, tapped, page: next }),
            Some(Choice::Card(c)) => {
                self.entering_tapped = tapped;
                let h = self.obj_ref(c);
                self.put_onto_battlefield_joining(card, p, Some(h));
            }
            Some(other) => unreachable!("enchant choice {other:?}"),
            None => {
                // (A permanent to enchant: by its slot, ENGINE.md 99.2.)
                let gy = self.def_of(card).reanimates;
                let opts: Vec<crate::decision::Opt> = self
                    .aura_candidates(card, p)
                    .into_iter()
                    .map(|c| crate::decision::Opt {
                        action: if gy { self.graveyard_action(p, c) } else { self.perm_action(p, c) },
                        choice: Choice::Card(c),
                    })
                    .collect();
                let kind = if gy { crate::decision::DecisionKind::ChooseGraveyardCard } else { crate::decision::DecisionKind::ChoosePermanent };
                self.ask_object_opts(Task::EnterAura { card, controller: p, tapped, page }, page, p, kind, opts);
            }
        }
    }

    /// What applies as a permanent enters (CR 614.1c, 614.12): "enters
    /// tapped", counters, loyalty, a Saga's lore counter, day and night,
    /// "as this enters" choices.
    pub(crate) fn entering(&mut self, idx: CardIdx, tapped: bool, x: u32) {
        self.entering_own(idx, tapped, x, true);
    }

    /// [`entering`](Self::entering); without `own`, none of its own
    /// abilities that apply as it enters (it enters with no abilities: CR
    /// 614.12), only the rules' (loyalty, lore counters).
    pub(crate) fn entering_own(&mut self, idx: CardIdx, tapped: bool, x: u32, own: bool) {
        if tapped {
            self.objs[idx.i()].tapped = true;
        }
        // "Each creature you control enters with an additional +1/+1 counter
        // on it for each land that entered ... this turn"; this land counted
        // after (ENGINE.md 99.12).
        if self.db.watch_lands_entered {
            self.lands_entering(idx);
        }
        // (A nonbasic land entering as a Mountain has none of its own
        // abilities, these replacement effects among them: CR 305.7, 614.12;
        // ENGINE.md 99.12.)
        let own = own && !(self.moon_game && self.moon_entering(idx));
        let sun = std::mem::take(&mut self.entering_sunburst);
        let exiled = std::mem::take(&mut self.entering_exiled);
        if own {
            self.entering_abilities(idx, x, sun, exiled);
        }
        // CR 306.5b: a planeswalker enters with its printed loyalty.
        if let (Some(n), true) = (self.def_of(idx).loyalty, self.def_of(idx).is(types::PLANESWALKER)) {
            // (A printed number; never negative, else none: CR 107.1b.)
            self.add_counters(idx, crate::db::Counter::Loyalty, n.max(0) as crate::num::Count);
        }
        // CR 714.3a: a Saga enters with a lore counter on it (its chapter I
        // ability triggers, 714.2b).
        if self.def_of(idx).chapters > 0 {
            self.add_counters(idx, crate::db::Counter::Lore, 1);
        }
        if own {
            self.entering_choices(idx);
        }
    }

    /// The permanent's own abilities that put counters on it as it enters.
    fn entering_abilities(&mut self, idx: CardIdx, x: u32, sun: u8, exiled: u8) {
        // CR 122.6: "enters with counters".
        for &(k, n) in &self.def_of(idx).enters_counters {
            self.add_counters(idx, k, n as crate::num::Count);
        }
        if self.def_of(idx).enters_x_counters && x > 0 {
            self.add_counters(idx, crate::db::Counter::P1P1, x as crate::num::Count);
        }
        // CR 702.44a-b: sunburst counters (none if it wasn't cast).
        if let (Some(k), true) = (self.def_of(idx).sunburst, sun > 0) {
            let k = if self.is_creature(idx) { crate::db::Counter::P1P1 } else { k };
            self.add_counters(idx, k, sun as crate::num::Count);
        }
        // CR 607.2q, 614.1c: a +1/+1 counter for each matching card exiled
        // to pay the cost of the spell that became it (none if it wasn't
        // cast; Murktide Regent, ENGINE.md 54).
        if exiled > 0 && self.def_of(idx).counters_per_exiled.is_some() {
            self.add_counters(idx, crate::db::Counter::P1P1, exiled as crate::num::Count);
        }
        // "A +1/+1 counter on it for each other <filter> you control" (CR
        // 614.1c): the matching permanents already there, not those entering
        // at the same time (CR 614.12a; `entry_since`).
        if let Some(f) = self.def_of(idx).enters_counters_per.clone() {
            let p = self.objs[idx.i()].controller;
            let since = self.entry_since();
            let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(idx)) };
            let n = self.permanents_of(p).filter(|&c| c != idx && self.objs[c.i()].timestamp < since && self.card_matches(&f, c, &ctx)).count();
            if n > 0 {
                self.add_counters(idx, crate::db::Counter::P1P1, n as crate::num::Count);
            }
        }
    }

    /// The permanent's own "as it enters" abilities (choices, "enters
    /// prepared"). (Day and night, CR 702.145c-g, are not "as it enters":
    /// they apply once it is on the battlefield, `Game::day_night_entered`.)
    fn entering_choices(&mut self, idx: CardIdx) {
        // Devour land n (CR 702.82a): which lands are sacrificed, then its
        // counters (`Task::Devour`; ENGINE.md 99.12).
        if self.def_of(idx).devour_land > 0 {
            self.agenda.push(Task::Devour { card: idx, picked: Vec::new() });
            // (Its counters, once the lands are chosen, are counters it enters
            // with: EoE W6, `W6State::deferred_since`.)
            if let Some(e) = self.w6.entering.filter(|e| e.0 == idx) {
                self.w6.deferred_since.push(e);
            }
        }
        if self.def_of(idx).shock_life > 0
            || self.def_of(idx).chooses_player
            || self.def_of(idx).paints
            || self.def_of(idx).chooses_land_type
            || self.def_of(idx).chooses_parity
            || self.def_of(idx).chooses_name
            || self.def_of(idx).w6.chooses_color
            || self.def_of(idx).dd_b.chooses_creature_type
        {
            self.agenda.push(Task::AsEnters { card: idx, page: crate::decision::Page::FIRST });
        }
        // "This creature enters prepared" (CR 722.3a).
        if self.def_of(idx).enters_prepared {
            self.become_prepared(idx);
        }
    }

    /// Create `n` tokens (CR 111.2) under `controller`, with the
    /// replacement effects that multiply them ("twice that many of those
    /// tokens are created instead", CR 614.1a); the first is first. They
    /// enter at the same time (one entry, CR 603.6a).
    pub(crate) fn create_tokens(&mut self, def: DefId, controller: PlayerId, n: u32, tapped: bool) -> Vec<CardIdx> {
        // (A replacement with copies of a permanent: EoE W6.)
        if n > 0 && self.w6.seen & crate::eoe_w6::SEEN_W6_TOKENS != 0 {
            match self.w6_token_replacement(controller) {
                Err(()) => return Vec::new(),
                Ok(Some(vals)) => return self.w6_create_copies(vals, controller, n, tapped),
                Ok(None) => {}
            }
        }
        let n = self.token_count(controller, n);
        self.begin_entering(false);
        let made = (0..n).map(|_| self.create_token(def, controller, tapped)).collect();
        self.end_entering();
        made
    }

    /// How many tokens `controller` creates instead of n ("twice that many
    /// of those tokens", CR 614.1a): any number (docs/shortcuts.md 24.7.4,
    /// 26.11.6: a player may have any number of permanents, ENGINE.md
    /// 100.4.8), exact. Past the objects the engine can hold (`OBJECT_LIMIT`,
    /// where `add_card` aborts the game, ENGINE.md 100.7) the game ends
    /// aborted now, and none are created: creating them would abort it there.
    pub(crate) fn token_count(&mut self, controller: PlayerId, n: u32) -> u32 {
        let mut n = n as crate::num::Wide;
        for c in self.permanents_of(controller) {
            let f = self.abilities(c).token_factor;
            if f > 1 {
                // (Past the objects left after a few doublings: no overflow.)
                n = n.saturating_mul(f as crate::num::Wide);
            }
        }
        let room = OBJECT_LIMIT.saturating_sub(self.objs.len()) as crate::num::Wide;
        if n > room {
            self.abort(|| format!("{n} tokens: past the {OBJECT_LIMIT} objects the engine holds"));
            return 0;
        }
        n as u32
    }

    /// Create one token (CR 111.2): a new object slot, put onto the
    /// battlefield under `controller` (its owner, CR 111.2), entering like
    /// any permanent (triggers, "enters tapped"). No replacement effects:
    /// effects create tokens through [`create_tokens`](Self::create_tokens).
    pub(crate) fn create_token(&mut self, def: DefId, controller: PlayerId, tapped: bool) -> CardIdx {
        let idx = self.add_card(def, controller, Zone::Nowhere);
        self.token_slot_made(idx);
        self.note(|g| format!("P{controller} creates a {} token", g.name(idx)));
        self.entering_tapped = tapped;
        self.put_onto_battlefield(idx, controller);
        idx
    }

    /// An Aura or Equipment becomes attached: it gets a new timestamp (CR
    /// 613.7e), and moves to the end of the battlefield (kept in timestamp
    /// order).
    pub(crate) fn attached_timestamp(&mut self, c: CardIdx) {
        if self.objs[c.i()].zone != Zone::Battlefield {
            return;
        }
        self.next_ts = self.exact_inc(self.next_ts, "timestamps");
        self.objs[c.i()].timestamp = self.next_ts;
        // (Its control effect follows the attachment: EoE W6.)
        if self.w6.seen & crate::eoe_w6::SEEN_W6_CONTROL != 0 {
            self.w6_sync_control();
        }
        if let Some(i) = self.battlefield.iter().position(|&x| x == c) {
            self.battlefield.remove(i);
            self.battlefield.push(c);
        }
        self.stamped(self.next_ts);
    }

    /// An object got timestamp `ts` (entering, becoming attached): with
    /// another given one in the same event whose order can matter, the
    /// active player orders them (CR 613.7m; `Task::OrderTimestamps`).
    pub(crate) fn stamped(&mut self, ts: u32) {
        if self.ts_group.0 != self.ev_seq || self.ts_group.1 == 0 {
            self.ts_group = (self.ev_seq, ts);
            return;
        }
        let first = self.ts_group.1;
        if self.agenda.iter().any(|t| matches!(t, crate::flow::Task::OrderTimestamps { first: f, .. } if *f == first)) {
            return;
        }
        let members = self.ts_members(first, true);
        if members.len() >= 2 && members.iter().any(|&c| self.order_sensitive(c, true)) {
            self.agenda.push(crate::flow::Task::OrderTimestamps { first, settled: false, picked: Vec::new() });
        }
    }

    /// Does permanent `c` have a static ability whose effect another
    /// effect's order can change (CR 613.7: one that sets a value, replaces
    /// types, removes abilities or changes control, not one that only adds,
    /// whose results commute); `maybe`: or a copy choice still to make?
    fn order_sensitive(&self, c: CardIdx, maybe: bool) -> bool {
        use crate::db::Mod;
        self.abilities(c).statics.iter().any(|st| {
            st.mods.iter().any(|m| {
                matches!(
                    m,
                    Mod::SetPt(..) | Mod::SwitchPt | Mod::Types { replace: true, .. } | Mod::BecomeTypes { .. } | Mod::LoseAllAbilities | Mod::Colors(_) | Mod::ControlFrom(_)
                )
            })
        }) || maybe && self.def_of(c).enters_as_copy.is_some() && !self.is_copy(c)
    }

    /// The permanents given a timestamp since `first` whose timestamps
    /// order effects (a static ability: layers 1-7, an Aura's or an
    /// Equipment's; a Painter's colour; a granted activated ability), in
    /// timestamp order; `maybe`: also one whose copy choice is still to be
    /// made (it may copy one).
    pub(crate) fn ts_members(&self, first: u32, maybe: bool) -> Vec<CardIdx> {
        self.battlefield
            .iter()
            .copied()
            .filter(|&c| self.objs[c.i()].timestamp >= first)
            .filter(|&c| {
                let d = self.abilities(c);
                !d.statics.is_empty()
                    || d.paints
                    || !d.w3.grants_activated.is_empty()
                    || d.grants_equipped.is_some()
                    || maybe && self.def_of(c).enters_as_copy.is_some() && !self.is_copy(c)
            })
            .collect()
    }

    /// CR 613.7m: the active player orders the timestamps of the objects
    /// given ones simultaneously since `first` (`ts_members`), earliest
    /// first (`ChoosePermanent`), once their "as this enters" choices are
    /// made (`settled`: it waits under them). Asked only while the order can
    /// matter: a member's effect is order-sensitive (`order_sensitive`), and
    /// the rest aren't all one definition (identical effects give the same
    /// result in either order); the rest then in their order. The chosen order permutes the members'
    /// timestamps and battlefield places (the battlefield is in timestamp
    /// order).
    pub(crate) fn order_timestamps(&mut self, first: u32, settled: bool, mut picked: Vec<CardIdx>) {
        use crate::flow::Task;
        if !settled {
            // (Below the entering choices of the same event still waiting:
            // a copy choice decides what the permanent's effects are.)
            let mut k = self.agenda.len();
            while k > 0 && matches!(self.agenda[k - 1], Task::AsEnters { .. } | Task::EnterAsCopy { .. } | Task::EnterAura { .. } | Task::EnterByDiscardAny { .. }) {
                k -= 1;
            }
            self.agenda.insert(k, Task::OrderTimestamps { first, settled: true, picked });
            return;
        }
        if let Some(Choice::Card(c)) = self.answer.take() {
            picked.push(c);
        }
        let members = self.ts_members(first, false);
        picked.retain(|c| members.contains(c));
        let rest: Vec<CardIdx> = members.iter().copied().filter(|c| !picked.contains(c)).collect();
        // (Effects that only add commute: their order can't matter.)
        let interchangeable = rest.iter().all(|&c| self.objs[c.i()].def == self.objs[rest[0].i()].def) || !members.iter().any(|&c| self.order_sensitive(c, false));
        if rest.len() >= 2 && !interchangeable {
            let ap = self.turn.active;
            let opts = rest.iter().map(|&c| crate::decision::Opt { action: self.perm_action(ap, c), choice: Choice::Card(c) }).collect();
            self.ask(Task::OrderTimestamps { first, settled: true, picked }, ap, crate::decision::DecisionKind::ChoosePermanent, opts);
            return;
        }
        if picked.is_empty() {
            return;
        }
        let order: Vec<CardIdx> = picked.iter().copied().chain(rest).collect();
        let mut stamps: Vec<u32> = members.iter().map(|&c| self.objs[c.i()].timestamp).collect();
        stamps.sort_unstable();
        let mut places: Vec<usize> = members.iter().map(|&c| self.battlefield.iter().position(|&x| x == c).expect("on the battlefield")).collect();
        places.sort_unstable();
        for (i, &c) in order.iter().enumerate() {
            self.objs[c.i()].timestamp = stamps[i];
            self.battlefield[places[i]] = c;
        }
        self.note(|g| format!("timestamps ordered: {}", order.iter().map(|&c| g.name(c)).collect::<Vec<_>>().join(", ")));
        self.effects_changed();
    }

    /// Lose life (CR 119.3; paying life is losing it, 119.4).
    pub(crate) fn lose_life(&mut self, p: PlayerId, n: impl Into<crate::num::Wide>) {
        let n = n.into();
        if n > 0 {
            self.sub_life(p, n);
            self.players[p as usize].lost_life = true;
        }
    }

    /// A life total changes by `-n` (CR 119.3; no bound, 119): exact within
    /// +-`num::AMOUNT_MAX`, past it the game ends aborted (ENGINE.md 100.7).
    fn sub_life(&mut self, p: PlayerId, n: crate::num::Wide) {
        let l = self.players[p as usize].life as crate::num::Wide - n;
        self.players[p as usize].life = self.exact(l, "a life total");
    }

    /// `p` gains control of permanent `c` (CR 613.1b; an effect without a
    /// duration). It is removed from combat (506.4).
    pub(crate) fn change_control(&mut self, c: CardIdx, p: PlayerId) {
        self.set_control(c, p);
    }

    /// [`change_control`](Self::change_control), also for an exchange of
    /// control (CR 701.12).
    pub(crate) fn set_control(&mut self, c: CardIdx, p: PlayerId) {
        // (With an Aura's control effect in the game, control is worked out
        // from every control effect in timestamp order: EoE W6.)
        if self.w6.seen & crate::eoe_w6::SEEN_W6_CONTROL != 0 && !self.w6.syncing {
            self.w6_control_effect(c, p, false);
            return;
        }
        self.apply_control(c, p);
    }

    /// The permanent's controller becomes `p` (CR 613.1b): it is removed
    /// from combat (506.4); the controller of a face-down permanent may
    /// look at it (708.5).
    pub(crate) fn apply_control(&mut self, c: CardIdx, p: PlayerId) {
        if self.objs[c.i()].zone != Zone::Battlefield || self.objs[c.i()].controller == p {
            return;
        }
        let turn = self.turn.number;
        // CR 708.5: the controller of a face-down permanent may look at it.
        if !self.face_down_ids.is_empty() && self.face_down_ids.iter().any(|x| x.0 == c) {
            self.look_face_down(c, p);
        }
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
        // A nonbasic land that is a Mountain (CR 305.7: its rules text's
        // abilities replaced by "{T}: Add {R}"; ENGINE.md 99.12).
        if self.moon_game && self.moon_on(c) {
            return self.db.def(self.moon_def(c));
        }
        self.db.def(self.objs[c.i()].def)
    }

    fn move_card_ctl(&mut self, idx: CardIdx, to: Zone, pos: Pos, controller: Option<PlayerId>) -> ObjRef {
        let from = self.objs[idx.i()].zone;
        // Entering face down (manifest, CR 701.40a): its identity stays as
        // hidden as in the library it comes from.
        let fd_enter = to == Zone::Battlefield && std::mem::take(&mut self.moving_face_down);
        // (A face-down permanent's knowledge bits, before the move resets
        // them: who knew the card it is.)
        let fd_known = self.objs[idx.i()].known;
        let old = self.obj_ref(idx);
        let old_controller = self.objs[idx.i()].controller;
        // (Leyline of the Void's replacement as this permanent last existed,
        // for the rest of a simultaneous move: `Lki::opp_gy_exile`.)
        let lki_void = from == Zone::Battlefield && self.batching && self.seen & SEEN_VOID != 0;
        let (lki_gy_exile, lki_void_exile) = if lki_void { (self.abilities(idx).opp_gy_exile, self.abilities(idx).void_exile) } else { (false, None) };
        // Replacement effects of where it goes (CR 614.1a), decided before
        // the events, in the order the affected object's controller (its
        // owner if it has none) chooses (616.1): `replaced_dest`.
        let decided = std::mem::take(&mut self.repl_decided);
        let mut adventure = false;
        let (to, void, choose) = match if decided { Dest::To(to) } else { self.replaced_dest(idx, from, to) } {
            Dest::To(z) => (z, None, None),
            Dest::Library => return self.shuffle_in_instead(idx, from),
            Dest::Exile => (Zone::Exile, None, None),
            Dest::Void(k) => (Zone::Exile, Some(k), None),
            Dest::Adventure => {
                adventure = true;
                (Zone::Exile, None, None)
            }
            // CR 616.1: its controller (owner) chooses which replacement
            // applies before the event; until the answer, right after the
            // move and before anything else, the card is in no zone, so
            // nothing sees a destination it may not get
            // (`Task::ChooseReplacement`), and leave triggers that depend on
            // the destination wait for it (`triggers::Held`).
            Dest::Choose(results, k) => {
                let chooser = if matches!(from, Zone::Battlefield | Zone::Stack) { old_controller } else { self.objs[idx.i()].owner };
                (Zone::Nowhere, None, Some((results, k, chooser)))
            }
        };
        // "Whenever a card leaves your graveyard" looks back in time too
        // (CR 603.10a): recorded before the card moves, the graveyard's
        // owner and the card as it was there (Murktide Regent; ENGINE.md 54).
        if from == Zone::Graveyard && to != Zone::Graveyard && self.seen & SEEN_GY_LEAVE != 0 && !self.is_token(idx) {
            let owner = self.objs[idx.i()].owner;
            self.event(Event { kind: EventKind::LeavesGraveyard, a: crate::triggers::Subj::Player(owner), b: crate::triggers::Subj::Obj(old), amount: 0 });
        }
        // Leave-the-battlefield abilities look back in time (CR 603.10a):
        // the events are recorded before the object moves.
        if from == Zone::Battlefield && !self.quiet_leave {
            // Void: a nonland permanent (as it last existed on the
            // battlefield) left it this turn (ENGINE.md 99.4).
            if !self.is_type(idx, crate::db::types::LAND) {
                self.void_turn = self.turn.number;
            }
            if to == Zone::Graveyard {
                self.event(Event::obj(EventKind::Dies, old));
            }
            // (Where it goes, for "dies or is put into exile": read as the
            // event's triggers are matched, ENGINE.md 99.11; each place a
            // pending replacement choice may still send it, ENGINE.md
            // 100.3.)
            self.leaving_to = crate::db::left_to::bit(to);
            if let Some((results, ..)) = choose {
                self.leaving_choice = choice_left_to(results);
                self.leaving_card = idx;
            }
            self.event(Event::obj(EventKind::LeavesBattlefield, old));
            self.leaving_to = 0;
            self.leaving_choice = 0;
        }
        // (Last known power: a stored amount, ENGINE.md 100.7.)
        let lki_power = if from == Zone::Battlefield {
            let p = self.power(idx);
            Some((old.inc, self.exact(p, "a last known power")))
        } else {
            None
        };
        // (An effect "loses all abilities for as long as" this remains on the
        // battlefield ends as it leaves, CR 611.2b: `layers_on_move`.)
        let loss_ended = from == Zone::Battlefield
            && self.any_loss
            && self.effects.iter().any(|e| e.m == crate::db::Mod::LoseAllAbilities && e.until == crate::layers::Until::WhileOnBattlefield(old));
        if from == Zone::Battlefield && (self.w3.seen != 0 || self.w3.granted) {
            self.w3_leaves(idx, old);
        }
        let lost_granted = from == Zone::Battlefield && !self.marks.is_empty() && self.lost_abilities(idx);
        // Its copiable values as it last existed on the battlefield (CR
        // 608.2h: "create a token that's a copy of it").
        if from == Zone::Battlefield {
            let v = self.copiable(idx);
            self.lki_copy.push((old, v));
            if let Some(h) = self.objs[idx.i()].attached {
                self.lki_attached.push((old, h));
            }
            if self.seen & SEEN_LKI_DAMAGE != 0 {
                let d = self.damage_source_of(idx);
                self.lki_damage.push((old, d));
            }
            // (Its controller as it last existed too: "that creature's
            // controller", ENGINE.md 99.12.)
            if self.db.watch_toughness || self.db.watch_lki_controller {
                let t = self.toughness(idx);
                let t = self.exact(t, "a last known toughness");
                self.lki_toughness.push((old, t, self.objs[idx.i()].controller));
            }
        }
        // CR 707.10a, 704.5e: a copy of a spell (or of a card) leaving the
        // stack ceases to exist, unless it becomes a permanent (608.3f);
        // nothing can see it in another zone first.
        let to = if from == Zone::Stack && to != Zone::Battlefield && self.is_copy_obj(idx) { Zone::Nowhere } else { to };
        // CR 702.34a: a flashback spell leaving the stack is exiled instead;
        // "if that spell would be put into your graveyard, exile it instead".
        // (A spell's "exile it instead" leaves with it.)
        if from == Zone::Stack && !self.gy_exile.is_empty() {
            self.gy_exile.retain(|&c| c != idx);
        }
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
            self.next_ts = self.exact_inc(self.next_ts, "timestamps");
            // (The first timestamp of its entry: what enters at the same
            // time, CR 614.12a; `entry_since`.)
            if let Some(e) = self.entries.last_mut() {
                if e.since == u32::MAX {
                    e.since = self.next_ts;
                }
            }
            self.next_ts
        } else {
            0
        };
        let stamp = ts;
        let turn = self.turn.number;
        let known_before = if from.is_hidden() { self.objs[idx.i()].known } else { 0b11 };
        // (Counted exactly: a slot's `u32::MAX`th zone change aborts the
        // game, so no `ObjRef` aliases a newer object.)
        let inc = self.exact_inc(self.objs[idx.i()].inc, "a card's zone changes");
        {
            let o = &mut self.objs[idx.i()];
            o.zone = to;
            o.inc = inc;
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
            o.known = if to.is_hidden() || fd_enter { known_before } else { 0b11 };
            o.attached = None;
            o.evoked = to == Zone::Battlefield && std::mem::take(&mut self.entering_evoked);
            o.flashback = false;
            o.chosen_player = None;
            o.monstrous = false;
            o.attacks = None;
            o.entered = if to == Zone::Battlefield { turn as u16 } else { 0 };
            o.flips = 0;
            o.escaped = to == Zone::Battlefield && std::mem::take(&mut self.entering_escaped);
        }
        if let Some((inc, p)) = lki_power {
            self.set_lki_power(idx, inc, p);
        }
        if !self.loyalty.is_empty() {
            self.loyalty.retain(|l| l.card != idx);
        }
        // A token created as a copy (CR 707.2, 111.10): a copy effect for as
        // long as it exists, in place before it is seen entering.
        if to == Zone::Battlefield {
            if let Some(v) = self.entering_copy.take() {
                // (The face it entered with: a copy of a double-faced
                // permanent back face up enters back face up, 707.8a; a
                // token never enters transformed by daybound, 702.145b.)
                let d = self.objs[idx.i()].def;
                self.next_ts = self.exact_inc(self.next_ts, "timestamps");
                let fx = crate::copies::CopyFx {
                    obj: ObjRef { idx, inc: self.objs[idx.i()].inc },
                    ts: self.next_ts,
                    until: crate::layers::Until::Forever,
                    vals: crate::copies::CopyVals { def: d, exc: v.exc, dfc: v.dfc },
                    own: d,
                };
                self.copies.push(fx);
            }
        }
        self.layers_on_move(idx, old, from, to);
        if from == Zone::Battlefield && self.batching {
            let def = self.objs[idx.i()].def;
            self.leaving.push(crate::triggers::Lki { old, def, controller: old_controller, opp_gy_exile: lki_gy_exile, void_exile: lki_void_exile });
        }
        // Its copy effects end with it (CR 400.7): it has its own
        // definition again (the copied one was its last known information,
        // used above).
        if !self.copies.is_empty() {
            self.copies_on_move(idx, old);
        }
        // A face-down permanent leaving the battlefield is revealed and is
        // its card again (CR 708.9; it left as a face-down 2/2, above).
        if from == Zone::Battlefield && !self.face_down_ids.is_empty() && self.face_down_ids.iter().any(|x| x.0 == idx) {
            self.unface(idx, fd_known);
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
                    // (n cards above it; with fewer cards there, all of
                    // them: the bottom.)
                    let n = n as usize;
                    let at = if n >= list.len() { 0 } else { list.len() - n };
                    list.insert(at, idx);
                }
            }
        }
        if stamp != 0 {
            self.stamped(stamp);
        }
        // A card leaving a hidden zone for a public one is revealed to all.
        // It is keyed even if everyone knew it: which known card a hidden
        // choice (a hand slot) named can differ between sampled worlds.
        if from.is_hidden() {
            // (The card's identity: a land played with its back face up, an
            // MDFC, has that face already, `put_onto_battlefield`.)
            let def = self.db.def(self.objs[idx.i()].def).card_def(self.objs[idx.i()].def);
            if fd_enter {
                // (Hidden to hidden: a cloud keeps it by its slot, as a card
                // exiled face down.)
                self.knowledge_moves(idx, owner, from, Zone::FaceDown);
            } else if to.is_hidden() {
                self.knowledge_moves(idx, owner, from, to);
            } else {
                self.knowledge_leaves(idx, owner, from, def, known_before);
                // (Waiting for a replacement choice, it is revealed: every
                // result shows it, CR 616.1.)
                if to != Zone::Nowhere || choose.is_some() {
                    self.emit_public(KEY_REVEAL, owner, def as u64);
                }
            }
        }
        let new = self.obj_ref(idx);
        self.note(|g| format!("{} {:?} -> {:?}", g.name(idx), from, to));
        if adventure {
            self.on_adventure(new, old_controller);
        }
        if let Some(k) = void {
            if self.objs[idx.i()].zone == Zone::Exile {
                self.add_counters(idx, k, 1);
            }
        }
        if to == Zone::Battlefield && !self.entering_mods.is_empty() {
            let ctl = self.objs[idx.i()].controller;
            for m in std::mem::take(&mut self.entering_mods) {
                self.add_effect(crate::layers::Affected::Obj(new), m, crate::layers::Until::Forever, ctl);
            }
        }
        if to == Zone::Battlefield {
            // (The note: cast, times paid; an "enter as a copy" choice holds
            // the event back, `copies.rs`.)
            let note = std::mem::take(&mut self.enter_note);
            if !std::mem::take(&mut self.defer_enters) {
                self.entered_event(new, note);
            }
        }
        if from == Zone::Battlefield && !self.links.is_empty() {
            // (In a simultaneous move, once it is over: `end_batch`.)
            if self.batching {
                self.releasing.push(old);
            } else {
                self.release_links(old);
            }
        }
        // "Whenever one or more cards are put into a library from anywhere".
        if to == Zone::Library && from != Zone::Library && self.seen & SEEN_RARE_EVENTS != 0 {
            self.event(Event::player(EventKind::CardsPutIntoLibrary, owner));
        }
        // "When this is put into a graveyard from anywhere" (its own
        // ability, from the graveyard).
        if to == Zone::Graveyard && self.db.def(self.objs[idx.i()].def).trig_mask & (1u64 << EventKind::ToGraveyard as u32) != 0 {
            self.event(Event::obj(EventKind::ToGraveyard, new));
        }
        // "Whenever one or more cards are put into exile from your library
        // and/or your graveyard" (Laelia).
        if matches!(to, Zone::Exile | Zone::FaceDown)
            && matches!(from, Zone::Library | Zone::Graveyard)
            && self.seen & SEEN_EXILE_WATCH != 0
            && choose.is_none()
        {
            self.event(Event::player(EventKind::ExiledFromLibGy, owner));
        }
        if let Some((results, k, chooser)) = choose {
            self.agenda.push(crate::flow::Task::ChooseReplacement { card: new, from, results, void: k, chooser });
        }
        if from == Zone::Battlefield && !self.marks.is_empty() && !lost_granted {
            // Triggered abilities an effect gave it (CR 603.10a: they look
            // back): "when it dies [or is exiled]", about the card now. (Not
            // if it had lost all abilities: those grants came first, as it
            // entered or before the loss, CR 613.1f, 613.7.)
            // (Waiting for a replacement choice, "when it dies or is
            // exiled" triggers if it is exiled: held until the answer.)
            let fired: Vec<(DefId, u8, bool)> = self
                .marks
                .iter()
                .filter_map(|&(r, m)| match m {
                    Mark::LeaveTrigger { def, slot, exile } if r == old && (to == Zone::Graveyard || exile && to == Zone::Exile) => {
                        Some((def, slot, false))
                    }
                    Mark::LeaveTrigger { def, slot, exile: true } if r == old && choose.is_some() => Some((def, slot, true)),
                    _ => None,
                })
                .collect();
            for (def, slot, held) in fired {
                let ev = crate::triggers::EvCtx { obj: Some(new), ..Default::default() };
                let t = crate::triggers::PendingTrigger { controller: old_controller, source: old, source_def: def, slot, ev };
                if held {
                    let f = crate::triggers::Fire { t, key: (old, slot), batch: false, check_done: false, once: false, times: None };
                    self.held.push(crate::triggers::Held { card: idx, zones: crate::db::left_to::EXILE, seq: self.ev_seq, f });
                } else {
                    self.waiting.push(t);
                }
            }
        }
        // A permanent that lost its abilities for as long as this one
        // remained (The Wondrous Wasp, Tishana's Tidebinder) has them again:
        // daybound and nightbound apply at once (CR 702.145c-g).
        if loss_ended {
            self.day_night_any_time();
        }
        new
    }

    /// Blightsteel's replacement: revealed (from a hidden zone), put into its
    /// owner's library, which is shuffled. A permanent going there doesn't
    /// die (CR 700.4); its leave triggers see it leave.
    /// Where a card moving from `from` to `to` goes after the replacement
    /// effects of its destination (CR 614.1a), applied in an order its
    /// controller (owner) chooses, each at most once, until none applies
    /// (616.1, 616.1f): "reveal it and shuffle it into its owner's library
    /// instead" (Blightsteel), "exile that card instead" (Yawgmoth's Will),
    /// "exile it with a void counter instead" (Dauthi Voidwalker), "if it
    /// would die / leave the battlefield, exile it instead" (Suplex, From the
    /// Catacombs, a finality counter), flashback's exile (702.34a), a
    /// permission's "exile it instead" and "if a card would be put into an
    /// opponent's graveyard from anywhere, exile it instead" (Leyline of the
    /// Void, ENGINE.md 124.3). None of them is a self-replacement
    /// effect (616.1a). `Dest::Choose` when orders give different results.
    pub(crate) fn replaced_dest(&self, idx: CardIdx, from: Zone, to: Zone) -> Dest {
        if from == Zone::Nowhere && to != Zone::Graveyard {
            return Dest::To(to);
        }
        // (From no zone: a card set aside, Mox Diamond not given a land;
        // a token or a rules object never goes to a graveyard from there.)
        let from_known = from != Zone::Nowhere || !self.is_token(idx) && !self.def_of(idx).rules;
        let owner = self.objs[idx.i()].owner;
        let card = !self.is_token(idx) && !(from == Zone::Stack && self.is_copy_obj(idx));
        let gy = to == Zone::Graveyard;
        // Which effects could apply (to a graveyard destination unless
        // said), by index: 0 library, 1 Will, 2 void, 3 dies-exile,
        // 4 leaves-exile, 5 flashback, 6 the spell's exile, 7 the
        // Adventure's exile, 8 Leyline of the Void's exile.
        let mut can = 0u16;
        let mut void = None;
        // CR 715.4: "instead of putting a spell that was cast as an
        // Adventure into its owner's graveyard as it resolves, its controller
        // exiles it" (Bilbo's ruling of 2026-06-29: it is one of the
        // replacement effects the controller orders).
        if gy && from == Zone::Stack && self.resolving_adventure {
            can |= 128;
        }
        if gy && from_known && self.def_of(idx).gy_to_library {
            can |= 1;
        }
        if gy && card && self.players[owner as usize].gy_exile_turn == self.turn.number && self.turn.number > 0 {
            can |= 2;
        }
        if gy && card && self.seen & SEEN_VOID != 0 && from_known {
            void = self.void_counter_for(owner);
            if void.is_some() {
                can |= 4;
            }
            // "If a card would be put into an opponent's graveyard from
            // anywhere, exile it instead" (CR 614.1a; ENGINE.md 124.3).
            if self.opp_gy_exile_for(owner) {
                can |= 256;
            }
        }
        if from == Zone::Battlefield && to != Zone::Exile {
            let fl = self.flags_of(idx);
            // A finality counter: "if this would be put into a graveyard from
            // the battlefield, exile it instead" (CR 122.1i).
            let finality = gy && !self.counters.is_empty() && self.db.finality.is_some_and(|k| self.counters_on(idx, k) > 0);
            if gy && (fl.exile_if_dies || finality) {
                can |= 8;
            }
            if fl.exile_if_leaves {
                can |= 16;
            }
        }
        if from == Zone::Stack {
            if self.objs[idx.i()].flashback {
                can |= 32;
            }
            if gy && !self.gy_exile.is_empty() && self.gy_exile.contains(&idx) {
                can |= 64;
            }
        }
        if can == 0 {
            return Dest::To(to);
        }
        // States: 0 = `to`, 1 = the library, 2 = exile, 3 = exile with a
        // void counter, 4 = exile on an adventure. The results every order
        // reaches (bit per state).
        fn walk(can: u16, gy: bool, to_exile: bool, state: u8, applied: u16, out: &mut u8) {
            let mut any = false;
            for e in 0..9u8 {
                if can & (1 << e) == 0 || applied & (1 << e) != 0 {
                    continue;
                }
                let at_gy = state == 0 && gy;
                let exiled = state >= 2 || (state == 0 && to_exile);
                let next = match e {
                    0 if at_gy => 1,
                    1 | 3 | 6 | 8 if at_gy => 2,
                    2 if at_gy => 3,
                    7 if at_gy => 4,
                    4 | 5 if !exiled => 2,
                    _ => continue,
                };
                any = true;
                walk(can, gy, to_exile, next, applied | 1 << e, out);
            }
            if !any {
                *out |= 1 << state;
            }
        }
        let mut out = 0u8;
        walk(can, gy, to == Zone::Exile, 0, 0, &mut out);
        match out {
            0b0001 => Dest::To(to),
            0b0010 => Dest::Library,
            0b0100 => Dest::Exile,
            0b1000 => Dest::Void(void.expect("a void counter")),
            0b1_0000 => Dest::Adventure,
            _ => Dest::Choose(out, void),
        }
    }

    /// CR 616.1: the affected object's controller (owner) chooses which
    /// replacement applies to `card`, in no zone until then (`move_card`):
    /// `NUMBER` 1 = shuffled into its owner's library, 2 = exile, 3 = exile
    /// with a void counter, 4 = exiled on an adventure (the results the
    /// orders reach; each offered once). The card goes there as the object the move made (one zone
    /// change, CR 400.7), and the triggered abilities that waited for the
    /// destination trigger (`release_held`).
    pub(crate) fn choose_replacement(&mut self, card: ObjRef, from: Zone, results: u8, void: Option<crate::db::Counter>, chooser: PlayerId) {
        if self.objs[card.idx.i()].inc != card.inc || self.objs[card.idx.i()].zone != Zone::Nowhere {
            return;
        }
        let idx = card.idx;
        let owner = self.objs[idx.i()].owner;
        match self.answer.take() {
            Some(Choice::Number(k)) => {
                self.note(|g| format!("P{chooser} applies {} to {}", ["", "the library", "exile", "a void counter", "an adventure"][k as usize], g.name(idx)));
                let to = if k == 1 { Zone::Library } else { Zone::Exile };
                self.repl_decided = true;
                self.move_card_ctl(idx, to, Pos::Top, None);
                self.objs[idx.i()].inc = card.inc;
                match k {
                    1 => self.shuffle_library(owner),
                    4 => self.on_adventure(card, chooser),
                    _ => {
                        if k == 3 {
                            if let Some(c) = void {
                                self.add_counters(idx, c, 1);
                            }
                        }
                        if matches!(from, Zone::Library | Zone::Graveyard) && self.seen & SEEN_EXILE_WATCH != 0 {
                            self.event(Event::player(EventKind::ExiledFromLibGy, owner));
                        }
                    }
                }
                self.release_held(idx, crate::db::left_to::bit(to));
            }
            Some(other) => unreachable!("replacement answer {other:?}"),
            None => {
                let opts: Vec<crate::decision::Opt> = (1..5u8)
                    .filter(|&k| results & (1 << k) != 0)
                    .map(|k| crate::decision::Opt { action: crate::decision::act::NUMBER + k as u16, choice: Choice::Number(k) })
                    .collect();
                self.ask(
                    crate::flow::Task::ChooseReplacement { card, from, results, void, chooser },
                    chooser,
                    crate::decision::DecisionKind::ChooseReplacement,
                    opts,
                );
            }
        }
    }

    /// CR 715.4: card `r`, exiled as its Adventure resolved, is on an
    /// adventure: `controller` may cast it (not as an Adventure) for as long
    /// as it stays exiled.
    fn on_adventure(&mut self, r: ObjRef, controller: PlayerId) {
        self.note(|g| format!("{} goes on an adventure", g.name(r.idx)));
        self.permissions.push(crate::game::Permission { card: r, player: controller, turn: None, kind: crate::game::PermKind::Adventure });
    }

    fn shuffle_in_instead(&mut self, idx: CardIdx, from: Zone) -> ObjRef {
        if from.is_hidden() {
            self.reveal(idx);
        }
        self.note(|g| format!("{} is shuffled into its owner's library instead", g.name(idx)));
        let owner = self.objs[idx.i()].owner;
        // (From its owner's library, a mill or a surveil: it stays there.)
        let r = if from == Zone::Library && self.objs[idx.i()].owner == owner {
            self.obj_ref(idx)
        } else {
            self.move_card_ctl(idx, Zone::Library, Pos::Top, None)
        };
        if self.pile_moving && from == Zone::Library {
            // A card of a looked-at pile (a surveil's, put into the
            // graveyard): the pile's other cards go where the effect puts
            // them in the same action (CR 701.25a, 608.2c), and the library
            // is shuffled after it (`flush_pile_shuffles`). Shuffled now, the
            // looked-at cards still to be placed were in it, unknown to the
            // player placing them, and a world `determinize` sampled for
            // that player dealt them again (FUZZ_POOL=interactions match
            // 10036882 at 075649c: Kaito's surveil; ENGINE.md 51).
            self.pile_shuffles |= 1 << owner;
        } else {
            self.shuffle_library(owner);
        }
        r
    }

    /// The library shuffles a looked-at pile's replacement put off
    /// (`pile_shuffles`), once the pile's cards are placed.
    pub(crate) fn flush_pile_shuffles(&mut self) {
        let s = std::mem::take(&mut self.pile_shuffles);
        for p in 0..2u8 {
            if s & (1 << p) != 0 {
                self.shuffle_library(p);
            }
        }
    }

    /// `p` mills `n` (CR 701.13a): the top `n` cards of their library are
    /// put into their graveyard together, one action. A card a replacement
    /// shuffles into its library instead (Blightsteel Colossus, CR 614.1a)
    /// stays where it is until all of them have moved, and the library is
    /// shuffled after the action, not between its cards - shuffled at once,
    /// the next cards milled came from the shuffled library, not the top
    /// `n`, and a card already milled and shuffled back could be milled
    /// again (a mill's pile then held one card twice: ENGINE.md 74). With
    /// `forced`, each card takes its queued outcome first (zero-core
    /// `force_chance`, ENGINE.md 10) and is revealed (the effects that read
    /// the milled cards). Returns the cards milled, top first, wherever they
    /// went (`Zone::Graveyard`: put there).
    pub(crate) fn mill(&mut self, p: PlayerId, n: usize, forced: bool) -> Vec<CardIdx> {
        let cards: Vec<CardIdx> = self.players[p as usize].library.iter().rev().take(n).copied().collect();
        let outer = std::mem::replace(&mut self.pile_moving, true);
        for &c in &cards {
            if forced {
                self.force_library_card(c, KEY_REVEAL, p);
                self.reveal(c);
            }
            self.move_card(c, Zone::Graveyard, Pos::Top);
        }
        self.pile_moving = outer;
        // (Inside a looked-at pile's placement, its own flush comes later.)
        if !outer {
            self.flush_pile_shuffles();
        }
        cards
    }

    /// CR 610.3a: the cards exiled "until" `source` leaves the battlefield
    /// return to their previous zones at once; links whose card left exile
    /// are dropped.
    pub(crate) fn release_links(&mut self, source: ObjRef) {
        let (back, keep): (Vec<Link>, Vec<Link>) =
            self.links.iter().copied().partition(|l| l.until && l.source == source);
        self.links = keep;
        // (The ones returning to the battlefield enter at once: one entry,
        // CR 603.6a.)
        self.begin_entering(false);
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
        self.end_entering();
        let objs = &self.objs;
        self.links.retain(|l| objs[l.card.idx.i()].inc == l.card.inc && objs[l.card.idx.i()].zone == Zone::Exile);
    }

    /// `p` draws n cards (one instruction, CR 121.2): "if you would draw one
    /// or more cards, you draw that many plus d instead" (Quantum Riddler,
    /// while its controller has few cards in hand) applies to it as a whole.
    /// True when a miracle question interrupted it (CR 702.94a: a card is
    /// revealed as it is drawn, before the instruction's next card): the
    /// question and the rest of the draws are queued (`Task::MiracleReveal`,
    /// `Task::DrawRest`), and the caller's own work must wait under them (an
    /// op returning `Flow::Next` does: `run_frame`; `wait_for_draw`).
    pub(crate) fn draw_cards(&mut self, p: PlayerId, n: u32) -> bool {
        match self.draw_instruction(p, n, false) {
            Drew::Miracle(card, left) => {
                self.queue_miracle(p, card, left, false);
                true
            }
            Drew::Asked => true,
            Drew::Done => false,
        }
    }

    /// [`draw_cards`](Self::draw_cards) without queueing a miracle
    /// question (`step`: the draw step's, whose beginning event follows the
    /// draws).
    pub(crate) fn draw_instruction(&mut self, p: PlayerId, n: u32, step: bool) -> Drew {
        let mut n = n;
        // One card, which an opponent's Hullbreacher would replace too, and
        // Quantum Riddler's "draw that many plus one": both replace the same
        // draw, and the drawer chooses the order (CR 616.1;
        // `Task::DrawOrder`). (To more than one card Quantum Riddler applies
        // first, to the instruction, its ruling: then Hullbreacher to each
        // draw.)
        if n == 1 && self.seen & SEEN_DRAW_REPL != 0 && self.opp_draw_token(p).is_some() {
            let plus = self.draw_plus(p);
            if plus > 0 {
                self.agenda.push(crate::flow::Task::DrawOrder { p, plus, ev: self.ev_seq, step });
                return Drew::Asked;
            }
            return self.draw(p).map_or(Drew::Done, |c| Drew::Miracle(c, 0));
        }
        if n > 0 && self.seen & SEEN_DRAW_REPL != 0 {
            let plus = self.draw_plus(p);
            if plus > 0 {
                self.note(|_| format!("P{p} draws {plus} more (a replacement)"));
                n = self.exact_u32(n as crate::num::Wide + plus as crate::num::Wide, "cards to draw");
            }
        }
        self.draw_seq(p, n).map_or(Drew::Done, |(c, left)| Drew::Miracle(c, left))
    }

    /// "If you would draw one or more cards, you draw that many plus d
    /// instead" while `p` has few cards in hand (Quantum Riddler): the d's.
    fn draw_plus(&self, p: PlayerId) -> u32 {
        let hand = self.players[p as usize].hand.len();
        self.permanents_of(p)
            .filter_map(|c| self.abilities(c).draw_plus)
            .filter(|&(_, at_most)| hand <= at_most as usize)
            .map(|(d, _)| d as u32)
            .sum()
    }

    /// CR 616.1: `p` orders an opponent's "create a token instead" and
    /// their own "draw that many plus `plus` instead" on one draw: `NUMBER`
    /// 5 = the token first (no draw is left to add to: one token), 6 = the
    /// added draws first (each draw then replaced). Then as the draw
    /// instruction would go on (`ev`, `step`: `draw_rest`'s).
    pub(crate) fn draw_order(&mut self, p: PlayerId, plus: u32, ev: u32, step: bool) {
        match self.answer.take() {
            Some(Choice::Number(k)) => {
                let saved = self.ev_seq;
                self.ev_seq = ev;
                let cut = if k == 5 {
                    self.draw(p).map(|c| (c, 0))
                } else {
                    self.note(|_| format!("P{p} draws {plus} more (a replacement)"));
                    let n = self.exact_u32(1 + plus as crate::num::Wide, "cards to draw");
                    self.draw_seq(p, n)
                };
                match cut {
                    Some((card, left)) => self.queue_miracle(p, card, left, step),
                    None if step => self.event(Event::player(EventKind::BeginningOfDrawStep, p)),
                    None => {}
                }
                self.ev_seq = saved;
            }
            Some(other) => unreachable!("draw order answer {other:?}"),
            None => {
                let opts = [5u8, 6]
                    .iter()
                    .map(|&k| crate::decision::Opt { action: crate::decision::act::NUMBER + k as u16, choice: Choice::Number(k) })
                    .collect();
                self.ask(crate::flow::Task::DrawOrder { p, plus, ev, step }, p, crate::decision::DecisionKind::ChooseReplacement, opts);
            }
        }
    }

    /// `p` draws `n` cards one at a time, stopping after one a miracle
    /// question is due for: that card and the draws left.
    fn draw_seq(&mut self, p: PlayerId, n: u32) -> Option<(CardIdx, u32)> {
        for k in 0..n {
            if let Some(c) = self.draw(p) {
                return Some((c, n - k - 1));
            }
        }
        None
    }

    /// Queue the miracle question about `card`, then the `left` draws of
    /// its instruction (and, `step`, the draw step's beginning event after
    /// them), in the current event group.
    pub(crate) fn queue_miracle(&mut self, p: PlayerId, card: CardIdx, left: u32, step: bool) {
        if left > 0 || step {
            self.agenda.push(crate::flow::Task::DrawRest { p, n: left, ev: self.ev_seq, step });
        }
        self.agenda.push(crate::flow::Task::MiracleReveal { p, card });
    }

    /// The rest of a draw instruction a miracle question interrupted
    /// (`queue_miracle`), in the instruction's event group (CR 603.2c).
    pub(crate) fn draw_rest(&mut self, p: PlayerId, n: u32, ev: u32, step: bool) {
        let saved = self.ev_seq;
        self.ev_seq = ev;
        let cut = self.draw_seq(p, n);
        match cut {
            Some((card, left)) => self.queue_miracle(p, card, left, step),
            None if step => self.event(Event::player(EventKind::BeginningOfDrawStep, p)),
            None => {}
        }
        self.ev_seq = saved;
    }

    /// The frame `f`, re-entered at the op that drew, waits under the tasks
    /// a draw queued since the agenda had `queued` (`draw_cards`).
    pub(crate) fn wait_for_draw(&mut self, queued: usize, f: &crate::resolve::Frame) {
        let tasks = self.agenda.split_off(queued);
        self.agenda.push(crate::flow::Task::Run(f.clone()));
        self.agenda.extend(tasks);
    }

    /// The token an opponent's Hullbreacher would create instead if `p` drew
    /// a card now ("except the first one they draw in each of their draw
    /// steps").
    fn opp_draw_token(&self, p: PlayerId) -> Option<u16> {
        if self.turn.number == 0 {
            return None;
        }
        let first_in_draw_step = self.turn.step == Step::Draw && self.turn.active == p && self.players[p as usize].draw_step_draws == 0;
        if first_in_draw_step {
            return None;
        }
        self.permanents_of(opp(p)).find_map(|c| self.abilities(c).opp_draw_token)
    }

    /// Draw a card (CR 121.2); from an empty library, flag 704.5b instead.
    /// The card, when its owner is asked about revealing it for miracle
    /// (`draw_cards` asks before the next draw).
    pub(crate) fn draw(&mut self, p: PlayerId) -> Option<CardIdx> {
        // "Each opponent can't draw more than one card each turn" (Narset).
        if self.players[p as usize].drawn >= 1
            && self.turn.number > 0
            && self.permanents_of(opp(p)).any(|c| self.abilities(c).opp_draw_limit)
        {
            self.note(|_| format!("P{p} can't draw another card this turn"));
            return None;
        }
        // "If an opponent would draw a card except the first one they draw in
        // each of their draw steps, instead you create a Treasure token"
        // (Hullbreacher, CR 614.1a: the draw doesn't happen).
        if self.seen & SEEN_DRAW_REPL != 0 {
            if let Some(token) = self.opp_draw_token(p) {
                let q = opp(p);
                self.note(|_| format!("P{p} would draw: P{q} creates a token instead"));
                let def = self.db.token_def(token);
                self.create_tokens(def, q, 1, false);
                return None;
            }
        }
        // "If you would draw a card while your library has no cards in it,
        // you win the game instead" (CR 614.1a, 104.2b).
        if self.players[p as usize].library.is_empty()
            && self.outcome.is_none()
            && self.permanents_of(p).any(|c| self.abilities(c).wins_on_empty_draw)
        {
            self.note(|_| format!("P{p} would draw from an empty library: wins the game"));
            self.players[opp(p) as usize].lost = true;
            self.end_game(Some(p), EndReason::Effect);
            return None;
        }
        if self.turn.step == Step::Draw && self.turn.active == p {
            let n = self.players[p as usize].draw_step_draws;
            self.players[p as usize].draw_step_draws = self.exact_inc(n, "cards drawn in the draw step");
        }
        if let Some(&top) = self.players[p as usize].library.last() {
            self.force_library_card(top, crate::hidden::KEY_DRAW, p);
        }
        let Some(&top) = self.players[p as usize].library.last() else {
            self.players[p as usize].drew_from_empty = true;
            self.note(|_| format!("P{p} draws from an empty library"));
            return None;
        };
        self.move_card(top, Zone::Hand, Pos::Top);
        self.objs[top.i()].known |= 1 << p;
        let def = self.objs[top.i()].def;
        self.emit_private(KEY_DRAW, p, def as u64);
        if self.turn_controller.is_some() {
            self.controller_sees(p, top); // CR 722.3 (ENGINE.md 100.5)
        }
        let n = self.players[p as usize].drawn;
        self.players[p as usize].drawn = self.exact_inc(n, "cards drawn this turn");
        // Miracle (CR 702.94a): the first card drawn this turn may be
        // revealed as it is drawn. Asked of a player whose deck has a
        // miracle card at all (both decklists are known, ENGINE.md 10), on
        // every first draw, so that the question says nothing about the
        // card.
        let miracle = self.players[p as usize].drawn == 1
            && self.turn.number > 0
            && self.deck_question(p, |d| d.miracle.is_some()).unwrap_or_else(|| self.has_miracle_card(p));
        self.event(Event::player(EventKind::DrawsCard, p));
        miracle.then_some(top)
    }

    /// With hidden decks (ENGINE.md 36.11), whether a question asked because
    /// `p`'s deck has a card matching `f` is asked: when the part of `p`'s
    /// pool the other player knows has one (`known_pools`), whatever the deck
    /// holds (its asking would tell the deck); `None` with open decklists
    /// (the deck decides, as before).
    pub(crate) fn deck_question(&self, p: PlayerId, f: impl Fn(&CardDef) -> bool) -> Option<bool> {
        self.known_pools[p as usize].as_ref().map(|k| k.iter().any(|&d| f(self.db.def(d))))
    }

    /// Does `p` own a card with miracle (ENGINE.md 29)?
    pub(crate) fn has_miracle_card(&self, p: PlayerId) -> bool {
        self.seen & (SEEN_MIRACLE_P0 << p) != 0
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

    pub(crate) fn gain_life(&mut self, p: PlayerId, n: impl Into<crate::num::Wide>) {
        let n = n.into();
        if n > 0 && !self.players[p as usize].cant_gain_life {
            self.sub_life(p, -n);
            self.players[p as usize].gained_life = true;
            let gained = self.players[p as usize].life_gained as crate::num::Wide + n;
            self.players[p as usize].life_gained = self.exact(gained, "the life gained this turn");
        }
    }

    /// Deal damage (CR 120.3): to a player as life loss, to a creature as
    /// marked damage; deathtouch marks 704.5h; lifelink gains life.
    /// Damage to a permanent with protection from the source is prevented
    /// (702.16e). Returns the damage dealt. An amount dealt past
    /// `num::AMOUNT_MAX` ends the game aborted (ENGINE.md 100.7); 0 or less
    /// isn't dealt (CR 120.8, a rules floor).
    pub(crate) fn deal_damage(&mut self, src: DamageSource, t: Target, amount: impl Into<crate::num::Wide>) -> i64 {
        let amount = amount.into();
        if amount <= 0 {
            return 0;
        }
        if self.combat_damage_now && !self.effects.is_empty() && self.combat_damage_prevented(&src) {
            return 0;
        }
        match t {
            // (An ability can't be dealt damage.)
            Target::Stack(_) => return 0,
            Target::Player(p) => {
                // CR 702.16e, j: damage to a player with protection from
                // everything is prevented.
                if self.player_protected(p) && !(self.combat_damage_now && self.combat_unpreventable(src.controller)) {
                    self.note(|_| format!("damage to P{p} prevented (protection)"));
                    return 0;
                }
                if src.infect {
                    // CR 702.90b: poison counters instead of life loss.
                    let n = self.players[p as usize].poison as crate::num::Wide + amount;
                    self.players[p as usize].poison = self.exact_count(n, "poison counters");
                } else {
                    // CR 120.3a: the player loses that much life.
                    self.sub_life(p, amount);
                    self.players[p as usize].lost_life = true;
                }
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
                let amt = self.exact_count(amount, "damage");
                if walker {
                    // CR 120.3c: that many loyalty counters are removed.
                    self.remove_counters(r.idx, crate::db::Counter::Loyalty, amt);
                }
                if creature && src.infect {
                    // CR 702.90c: -1/-1 counters instead of marked damage.
                    self.add_counters(r.idx, crate::db::Counter::M1M1, amt);
                    if src.deathtouch {
                        self.objs[r.idx.i()].deathtouched = true;
                    }
                } else if creature {
                    // CR 120.3e: damage is marked (both, for a planeswalker
                    // creature).
                    let heals = self.abilities(r.idx).heals_on_damage;
                    let marked = if heals {
                        amt
                    } else {
                        self.exact_count(self.objs[r.idx.i()].damage as crate::num::Wide + amt as crate::num::Wide, "marked damage")
                    };
                    let o = &mut self.objs[r.idx.i()];
                    o.damage = marked;
                    if heals {
                        o.deathtouched = false;
                    }
                    if src.deathtouch {
                        o.deathtouched = true;
                    }
                }
                if self.db.watch_damage {
                    self.note_dealt_damage(r);
                }
                // "Whenever this is dealt damage": combat damage from several
                // sources at once is one event with the total (Screaming
                // Nemesis's ruling 66fc62ef, CR 510.2; `combat.rs` emits it).
                if !self.combat_damage_now {
                    let mut ev = Event::obj(EventKind::DealtDamage, r);
                    ev.amount = amt;
                    self.event(ev);
                }
            }
        }
        if src.lifelink {
            self.gain_life(src.controller, amount);
        }
        self.note(|g| format!("{amount} damage to {}", g.describe_target(t)));
        // (Dealt: positive, and within `num::AMOUNT_MAX` or the game aborted.)
        let amount = self.exact(amount, "damage");
        // "Whenever this deals damage" (combat damage: one event per source
        // for the whole step, `combat.rs`).
        if let (false, Some(o)) = (self.combat_damage_now, src.obj) {
            self.event(crate::triggers::Event {
                kind: EventKind::DealsDamage,
                a: crate::triggers::Subj::Obj(o),
                b: crate::triggers::Subj::None,
                amount: amount as u64,
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
        // (At once: one entry, CR 603.6a.)
        self.begin_entering(false);
        for l in back {
            if self.live(l.card) && self.objs[l.card.idx.i()].zone == Zone::Exile {
                let owner = self.objs[l.card.idx.i()].owner;
                self.put_onto_battlefield(l.card.idx, owner);
            }
        }
        self.end_entering();
    }

    /// Player `p` has protection from everything (CR 702.16j).
    pub(crate) fn player_protected(&self, p: PlayerId) -> bool {
        let pl = &self.players[p as usize];
        pl.turns < pl.prot_until
    }

    /// "Combat damage that would be dealt by creatures you control can't be
    /// prevented" (Questing Beast) for `p`.
    pub(crate) fn combat_unpreventable(&self, p: PlayerId) -> bool {
        self.permanents_of(p).any(|c| self.abilities(c).combat_unpreventable) || self.all_combat_unpreventable()
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
            || (p.colored_spells && src.spell && src.colors != 0)
    }

    /// The characteristics of a permanent (or spell) as a source of damage
    /// or of a targeting spell or ability.
    pub(crate) fn damage_source_of(&self, idx: CardIdx) -> DamageSource {
        let kws = self.keywords_of(idx);
        DamageSource {
            controller: self.objs[idx.i()].controller,
            deathtouch: kws.has(Kw::Deathtouch),
            lifelink: kws.has(Kw::Lifelink),
            infect: kws.has(Kw::Infect),
            spell: self.objs[idx.i()].zone == Zone::Stack,
            colors: self.colors_of(idx),
            types: self.types_of(idx),
            obj: Some(self.obj_ref(idx)),
        }
    }

    pub fn describe_target(&self, t: Target) -> String {
        match t {
            Target::Player(p) => format!("P{p}"),
            Target::Obj(r) => format!("{}#{}", self.name(r.idx), r.idx.get()),
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DamageSource {
    pub controller: PlayerId,
    pub deathtouch: bool,
    pub lifelink: bool,
    /// Infect (CR 702.90b-c).
    pub infect: bool,
    /// The source is a spell (on the stack).
    pub spell: bool,
    /// Its colours and card types (protection, CR 702.16).
    pub colors: crate::mana::ManaMask,
    pub types: u16,
    /// The object dealing the damage, if one ("whenever this deals
    /// damage").
    pub obj: Option<ObjRef>,
}
