//! The network input of the `mtg` game (ENGINE.md 21.2, 22.2, 24): a fixed-width
//! f32 vector for a viewer, with no hidden information. Layout v4 (33,652
//! floats; M8a and M8b, ENGINE.md 35.6, 36.7: open until the matches of
//! phase 2 are done) is layout v3 (29,860 floats; frozen at the end of the
//! M7b card batch, ENGINE.md 22.6) plus the companions block, a global and
//! two per-player globals (M8a), and (M8b) a list block of 64 slots with
//! four more floats each, two more bags and the match block; in order:
//!
//! | Block | Floats | What |
//! |---|---:|---|
//! | globals | 128 | turn, step, who is active / has priority, per player (viewer first) life, zone sizes, mana pool, board summary, lost life this turn, sorceries as though they had flash; M7b part 2: poison, cards on an adventure, temporary triggered abilities, no maximum hand size; day and night; part 3b: the initiative, the dungeon room, dungeons completed, experience counters |
//! | context | 256 | the pending decision: kind (64 kinds), decider, the asking task, its card (card code) and ability, the resolving op, the cost left to pay, damage left, counters of the asking task |
//! | bags | 17 x 1024 | per identity counts (0.25 per card): own hand, both battlefields, graveyards, exiles, own library, the opponent's hidden pool, the opponent's hand known exactly and by clouds, own library cards known, the opponent's library known exactly or by clouds, own and the opponent's emblems (by the identity of the card that made them); M8b: the opponent's pool as the viewer knows it, the cards the opponent revealed in earlier games of the match |
//! | tops | 2 x 3 x 54 | the top three library positions of each player: present, known to the viewer, the card code |
//! | permanents | 2 x 32 x 148 | `MY_PERM` / `OPP_PERM` slots 0..31: card code + status (tapped, damage, P/T, sickness, combat and what it attacks, keywords, loyalty, loyalty ability available, decision flags, options, lore counters, back face up; M7b part 3: a copy and the identity of what it copies, what it is attached to; part 3b: ward and its cost, lost all abilities, goaded, exiled if it would leave) |
//! | hand | 16 x 60 | `HAND` slots 0..15 (the viewer's hand sorted by definition): card code, count, options (which face its `HAND` id casts or plays), known to the opponent |
//! | stack | 8 x 84 | `STACK` slots 0..7 (top first): source card code, kind, controller, targets (a player, a permanent, a stack item); a copy of a spell, times an additional cost was paid, bestowed; part 3b: targeted by another stack item, can't be countered, the ability's index, a graveyard card's ability |
//! | triggers | 4 x 56 | `TRIGGER` slots 0..3 (the viewer's waiting triggers) |
//! | list | 64 x 60 | `LIBRARY` slots 0..63 of the viewer's own decision: the searched library, the looked-at pile or the seen hand, sorted by definition; M8b: sideboarding's pool list (`list::*`: copies in the pool, in the deck, in the deck at the start, a first pick, a basic land, already decided) |
//! | graveyard abilities | 4 x 56 | `GY_ABILITY` slots 0..3 (part 3b): the cards in the viewer's graveyard with an ability that works there, in graveyard order, and whether it may be activated now |
//! | companions | 2 x 56 | v4 (M8a): the viewer's then the opponent's revealed companion (CR 702.139): card code, revealed, its special action still available (outside the game, not taken), the special action is an option of the viewer's decision now (`act::COMPANION`, the viewer's slot), put into the hand |
//! | match | 32 | v4 (M8b, ENGINE.md 36.7; globals): the format, the game number, both players' game wins, draws, each game's result and who played first, sideboarding now and its order, deck sizes (`mtch::*`) |
//!
//! v4's globals (ENGINE.md 35): `g[124]` the viewer chose who takes the first
//! turn (CR 103.1; `g[18]`: the viewer is the starting player); per player
//! (`g[24 + 40 * side..]`) `glob::SIDEBOARD`: the cards of their sideboard
//! still outside the game / 15, `glob::COMPANION`: a revealed companion
//! whose special action is available. After the freeze (ENGINE.md 55, in
//! spare globals, no width or offset changed): `g[125 + side]`
//! (`glob::MULLIGAN_DECLARED`), the player declared a mulligan not yet taken
//! (CR 103.5).
//!
//! A card code (52 floats) is the card's stable identity (`identity.rs`: its
//! position in the cube's IR card list, tokens from a registry; capacity
//! 1024) as three one-hots (index / 64, index / 8 % 8, index % 8: 16 + 8 + 8)
//! plus a descriptor (card types, colours, mana value, printed power and
//! toughness, token, legendary; M7b: planeswalker, printed loyalty, emblem, a
//! face other than the front is up: then the descriptor is that face's).
//! M7b part 3 (copies, ENGINE.md 26): a permanent's identity is what it is
//! without copy effects (the physical card, or the card a token copy copies);
//! its descriptor shows the copiable values it has (the copied face), and
//! its status the identity of what it copies.
//! Layout v1 (M5a, 13,532 floats) used the
//! card table's index (capacity 256), which renumbers as support grows;
//! v2 (M5b, 23,324 floats) had a 192-float context block with room for 32
//! decision kinds.
//! Every value is finite and bounded (M6, ENGINE.md 46): a number the game
//! leaves unbounded (life, power, toughness, damage, counters, a zone's or a
//! list's size, the turn: CR 107.1, 122.1) goes through [`squash`], exact
//! within +-[`SQUASH_FROM`] (every normal state), then logarithmic; every
//! other value is bounded by its type or an engine cap (a `u8` / 4, a bag
//! by the 96-permanent cap); none exceeds [`FEATURE_BOUND`].
//! "Me" is the viewer. Options of the pending decision are encoded only
//! when the viewer decides (its own legal actions); the list block too.
//! Everything else is public or the viewer's own knowledge, so the vector is
//! the same in every world `determinize(viewer)` can produce (tested).

use crate::db::{types, AbilitySlot, CardDb, Kw, WardCost};
use crate::decision::{act, Choice, FACE_CAST, FACE_PLAY, OTHER_CAST, OTHER_PLAY};
use crate::flow::Task;
use crate::game::{Game, StackKind};
use crate::ids::{opp, CardIdx, DefId, PlayerId, Target, Zone};
use crate::program::Op;

use super::identity;

/// Identity width: the capacity of the stable card identities (`identity.rs`).
pub const D: usize = identity::D;
/// Card code: the identity's three one-hot digits, then the descriptor.
pub const CODE: usize = 52;
pub const GLOBALS: usize = 128;
pub const CONTEXT: usize = 256;
pub const BAGS: usize = 17;
pub const TOPS: usize = 3;
pub const TOP_W: usize = CODE + 2;
pub const PERMS: usize = 32;
pub const PERM_W: usize = CODE + 96;
pub const HANDS: usize = 16;
pub const HAND_W: usize = CODE + 8;
pub const STACKS: usize = 8;
pub const STACK_W: usize = CODE + 32;
pub const TRIGS: usize = 4;
pub const TRIG_W: usize = CODE + 4;
/// M8b: 64 slots (every `LIBRARY` id; a sideboarding pool list has up to
/// 64 entries), four more floats (`list::*`).
pub const LIST: usize = 64;
pub const LIST_W: usize = CODE + 8;
pub const GYAS: usize = 4;
pub const GYA_W: usize = CODE + 4;
/// Companions (layout v4): the viewer's, then the opponent's.
pub const COMPS: usize = 2;
pub const COMP_W: usize = CODE + 4;
/// The match block (layout v4, M8b; globals).
pub const MATCH: usize = 32;

pub const O_GLOBALS: usize = 0;
pub const O_CONTEXT: usize = O_GLOBALS + GLOBALS;
pub const O_BAGS: usize = O_CONTEXT + CONTEXT;
pub const O_TOPS: usize = O_BAGS + BAGS * D;
pub const O_MY_PERM: usize = O_TOPS + 2 * TOPS * TOP_W;
pub const O_OPP_PERM: usize = O_MY_PERM + PERMS * PERM_W;
pub const O_HAND: usize = O_OPP_PERM + PERMS * PERM_W;
pub const O_STACK: usize = O_HAND + HANDS * HAND_W;
pub const O_TRIG: usize = O_STACK + STACKS * STACK_W;
pub const O_LIST: usize = O_TRIG + TRIGS * TRIG_W;
pub const O_GYA: usize = O_LIST + LIST * LIST_W;
pub const O_COMP: usize = O_GYA + GYAS * GYA_W;
pub const O_MATCH: usize = O_COMP + COMPS * COMP_W;
pub const NUM_FEATURES: usize = O_MATCH + MATCH;

/// Features are exact within +-`SQUASH_FROM` (ENGINE.md 46): a 1,024-power
/// creature (P/T / 8), 2,560 total power or life (/ 20), any bag. The
/// behaviour digests' largest value is 68.1 (a 545/545 creature of random
/// play), so none of their states changes.
pub const SQUASH_FROM: f32 = 128.0;
/// No feature exceeds this magnitude: beyond `SQUASH_FROM` the log adds at
/// most ln(f32::MAX) = 88.7 (and about 24 for the engine's largest sums), and
/// the unsquashed values are at most 255 (a `u8` as it is: lands played
/// this turn) (the zero-core shards store features as float16, whose largest
/// finite value is 65,504).
pub const FEATURE_BOUND: f32 = 256.0;

/// The feature transform (ENGINE.md 46): `v` within +-[`SQUASH_FROM`]; beyond,
/// `sign(v) (SQUASH_FROM + ln(1 + |v| - SQUASH_FROM))` (continuous, slope 1
/// at the threshold, monotone): 2^31 power / 8 is 147.4, every power summed
/// (192 x 2^31 / 20) 151.7, f32::MAX 216.7; an infinity (never written)
/// [`FEATURE_BOUND`]. A NaN stays NaN (a bug, never hidden: the fuzzer and
/// zero-core's guard refuse it).
#[inline]
pub fn squash(v: f32) -> f32 {
    let a = v.abs();
    if !(a > SQUASH_FROM) {
        return v;
    }
    (SQUASH_FROM + (a - SQUASH_FROM).ln_1p()).min(FEATURE_BOUND).copysign(v)
}


/// Offsets inside a list slot (after its card code). A decision's list:
/// `PRESENT`, `OPTION`, `COPIES` (copies in the list / 4). Sideboarding's
/// pool list (M8b, ENGINE.md 36.7): `COPIES` = copies in the pool / 4 (0 for
/// a basic land: any number), the rest below.
pub mod list {
    pub const PRESENT: usize = 0;
    /// Its `LIBRARY` id is an option of the viewer's decision.
    pub const OPTION: usize = 1;
    pub const COPIES: usize = 2;
    /// Copies in the deck being built / 4.
    pub const IN_DECK: usize = 3;
    /// Copies in the deck of the last game / 4.
    pub const AT_START: usize = 4;
    /// One of the viewer's first picks (part b: hidden from the opponent).
    pub const FIRST_PICK: usize = 5;
    /// A basic land (any number may be in a deck, CR 100.2b).
    pub const BASIC: usize = 6;
    /// Decided: at or before the last entry changed (entries change in
    /// increasing order).
    pub const DECIDED: usize = 7;
}

/// Offsets inside the match block (layout v4, M8b, ENGINE.md 36.7). "Me" is
/// the viewer.
pub mod mtch {
    /// A best-of-three match (else one game).
    pub const BO3: usize = 0;
    /// The current (or last) game's number / 3.
    pub const GAME: usize = 1;
    /// Game wins / 2: mine, the opponent's; drawn games / 3.
    pub const MY_WINS: usize = 2;
    pub const OPP_WINS: usize = 3;
    pub const DRAWS: usize = 4;
    /// Sideboarding now; I sideboard first (I choose who takes the first
    /// turn of the next game, CR 103.1); I have chosen my deck.
    pub const SIDEBOARDING: usize = 5;
    pub const SB_FIRST: usize = 6;
    pub const SB_DONE: usize = 7;
    /// My deck's size / 40 (being built, while I sideboard), the
    /// opponent's last deck's size / 40 (open decklists).
    pub const MY_DECK: usize = 8;
    pub const OPP_DECK: usize = 9;
    /// Per game 0..2: 3 floats, I won, I lost, drawn (games played).
    pub const RESULTS: usize = 10;
    /// Per game 0..2 played: I took the first turn; I chose who did.
    pub const STARTED: usize = 19;
    pub const CHOSE: usize = 22;
    /// The entry (k + 1) / 64 the pending `SideboardCount` is about (mine),
    /// and the last entry changed (k + 1) / 64.
    pub const COUNT_ENTRY: usize = 25;
    pub const LAST_ENTRY: usize = 26;
    pub const END: usize = 27;
}
const _: () = assert!(mtch::END <= MATCH);

/// Offsets inside a companion slot (after its card code; layout v4).
pub mod comp {
    /// A revealed companion (CR 103.2b).
    pub const PRESENT: usize = 0;
    /// Still outside the game with its special action not taken (116.2g).
    pub const AVAILABLE: usize = 1;
    /// The special action is an option of the viewer's pending decision.
    pub const LEGAL: usize = 2;
    /// Put into its owner's hand (it is in the game: 702.139c).
    pub const IN_GAME: usize = 3;
}

/// Bags (each `D` wide, 0.25 per card).
pub mod bag {
    pub const MY_HAND: usize = 0;
    pub const MY_BATTLEFIELD: usize = 1;
    pub const OPP_BATTLEFIELD: usize = 2;
    pub const MY_GRAVEYARD: usize = 3;
    pub const OPP_GRAVEYARD: usize = 4;
    pub const MY_EXILE: usize = 5;
    pub const OPP_EXILE: usize = 6;
    pub const MY_LIBRARY: usize = 7;
    /// The opponent's hand and library cards the viewer does not know
    /// exactly (open decklists: their identities as a multiset). With hidden
    /// decks (a best-of-three match, ENGINE.md 36.11): the candidates, the
    /// cards of their pool the viewer knows of (but basic lands) less those
    /// the viewer knows are elsewhere (their cards it knows in the game,
    /// its clouds' identities, a revealed companion).
    pub const OPP_HIDDEN: usize = 8;
    pub const OPP_HAND_KNOWN: usize = 9;
    /// Identities the viewer's clouds place in the opponent's hand.
    pub const OPP_HAND_CLOUD: usize = 10;
    pub const MY_LIBRARY_KNOWN: usize = 11;
    /// The opponent's library cards known exactly, plus the identities the
    /// viewer's clouds place in it.
    pub const OPP_LIBRARY_KNOWN: usize = 12;
    /// Emblems (CR 114), by the identity of the card that made them.
    pub const MY_EMBLEMS: usize = 13;
    pub const OPP_EMBLEMS: usize = 14;
    /// M8b (ENGINE.md 36.7): the opponent's pool as the viewer knows it
    /// (best-of-one, open decklists: all of it; best-of-three: but their
    /// first picks not revealed in an earlier game, ENGINE.md 36.11).
    pub const OPP_POOL: usize = 15;
    /// The identities the opponent revealed (made public) in earlier games
    /// of the match.
    pub const OPP_REVEALED: usize = 16;
}

/// Offsets inside a permanent's status (after its card code).
pub mod perm {
    pub const PRESENT: usize = 0;
    pub const TAPPED: usize = 1;
    pub const DAMAGE: usize = 2;
    pub const POWER: usize = 3;
    pub const TOUGHNESS: usize = 4;
    pub const SICK: usize = 5;
    pub const ATTACKING: usize = 6;
    pub const BLOCKED: usize = 7;
    pub const BLOCKING: usize = 8;
    /// (slot + 1) / 32 of the attacker it blocks.
    pub const BLOCKING_SLOT: usize = 9;
    pub const STRUCK_FIRST: usize = 10;
    /// The potential blocker the pending `DeclareBlockers` decision is about.
    pub const ASKED_BLOCKER: usize = 11;
    /// The attacker / the blocker of the pending `AssignDamage` decision.
    pub const DAMAGE_ATTACKER: usize = 12;
    pub const DAMAGE_BLOCKER: usize = 13;
    /// Its slot id (`MY_PERM` / `OPP_PERM`) is an option of the viewer's decision.
    pub const OPTION: usize = 14;
    /// `ACTIVATE` options of abilities 0..3 (the viewer's permanents).
    pub const ACTIVATE: usize = 15;
    pub const TARGETED: usize = 19;
    pub const BLOCKERS: usize = 20;
    pub const KEYWORDS: usize = 21;
    pub const TOKEN: usize = 36;
    pub const NEW: usize = 37;
    pub const STOLEN: usize = 38;
    /// Loyalty counters / 8 (CR 306.5c).
    pub const LOYALTY: usize = 39;
    /// A planeswalker whose loyalty ability may still be activated this
    /// turn (CR 606.3).
    pub const LOYALTY_READY: usize = 40;
    /// An attacking creature attacking a planeswalker (not the player), and
    /// that planeswalker's slot (slot + 1) / 32 (CR 506.3, 508.1b).
    pub const ATTACKS_WALKER: usize = 41;
    pub const ATTACKS_SLOT: usize = 42;
    /// Creatures attacking this planeswalker / 4.
    pub const ATTACKED_BY: usize = 43;
    /// The attacker the pending `AttackTarget` decision is about.
    pub const ASKED_ATTACKER: usize = 44;
    /// Lore counters / 4 (a Saga, CR 714.3).
    pub const LORE: usize = 45;
    /// Its back face is up (transformed, CR 701.27g; an MDFC's back face).
    pub const BACK_FACE: usize = 46;
    /// M7b part 3: a copy effect applies to it (CR 707): its descriptor is
    /// the copied face's; `COPY_OF` (32 floats) is the identity of what it
    /// copies, as the card code's digits.
    pub const COPY: usize = 47;
    pub const COPY_OF: usize = 48;
    /// Attached to a permanent (an Equipment, a bestowed Aura: CR 301.5,
    /// 702.103), that permanent's slot ((slot + 1) / 32), and whether the
    /// viewer's opponent controls it.
    pub const ATTACHED: usize = 80;
    pub const ATTACHED_SLOT: usize = 81;
    pub const ATTACHED_OPP: usize = 82;
    /// Prepared (CR 722.3a): a copy of its prepare spell in exile may be
    /// cast.
    pub const PREPARED: usize = 83;
    /// M7b part 3b (ENGINE.md 27): its ward abilities / 2 (its own and
    /// those a permanent its controller controls gives it, CR 702.21), the
    /// largest ward mana cost's mana value / 4, the largest ward life cost
    /// / 4, a ward cost of discarding a card.
    pub const WARD: usize = 84;
    pub const WARD_MANA: usize = 85;
    pub const WARD_LIFE: usize = 86;
    pub const WARD_DISCARD: usize = 87;
    /// It lost all abilities (CR 613.1f; Tishana's Tidebinder).
    pub const LOST_ABILITIES: usize = 88;
    /// Goaded (CR 701.38: it attacks each combat if able).
    pub const GOADED: usize = 89;
    /// It is exiled if it would leave the battlefield (From the Catacombs).
    pub const EXILE_IF_LEAVES: usize = 90;
}

/// Offsets inside a stack slot's status (after its card code).
pub mod stack {
    /// M7b part 3: a copy of a spell (CR 707.10), the times an additional
    /// cost was paid / 4 (replicate, squad, offspring), a bestowed Aura
    /// spell (702.103b).
    pub const COPY: usize = 24;
    pub const TIMES: usize = 25;
    pub const BESTOWED: usize = 26;
    /// M7b part 3b: another stack item targets it (CR 115.7); a spell that
    /// can't be countered (101.2); an ability's index in its card's list
    /// ((index + 1) / 8: which triggered or activated ability; a ward
    /// trigger is one of its card's triggers); an ability of a card in a
    /// graveyard (ENGINE.md 27.5).
    pub const TARGETED: usize = 27;
    pub const UNCOUNTERABLE: usize = 28;
    pub const SLOT: usize = 29;
    pub const GRAVEYARD: usize = 30;
}

/// Offsets inside the per-player globals (`g[24 + 40 * side..]`, viewer
/// first) added in M7b part 3b, and the room one-hots.
pub mod glob {
    /// Has the initiative (CR 725).
    pub const INITIATIVE: usize = 34;
    /// Has a dungeon in the command zone (its venture marker is in a room,
    /// CR 309.4).
    pub const IN_DUNGEON: usize = 35;
    /// Dungeons completed / 2 (CR 309.7).
    pub const COMPLETED: usize = 36;
    /// Experience counters / 4 (CR 122.1).
    pub const EXPERIENCE: usize = 37;
    /// The venture marker's room (`dungeon::ROOMS` index), one-hot:
    /// `g[ROOM + 9 * side + room]`.
    pub const ROOM: usize = 106;
    /// Layout v4 (M8a, ENGINE.md 35): the cards of the player's sideboard
    /// still outside the game / 15 (CR 400.11a), and a revealed companion
    /// whose special action is available (702.139a, 116.2g).
    pub const SIDEBOARD: usize = 38;
    pub const COMPANION: usize = 39;
    /// `g[CHOOSER]`: the viewer chose who takes the first turn (CR 103.1).
    pub const CHOOSER: usize = 124;
    /// `g[MULLIGAN_DECLARED + side]` (viewer first): the player declared a
    /// mulligan in this round of declarations and has not taken it yet (CR
    /// 103.5: the declarations are public; ENGINE.md 55). Set only while the
    /// second player declares after the starting player declared a mulligan
    /// (once every player has declared, the mulligans are taken). In layout
    /// v4's spare globals 125-126 (127 stays spare).
    pub const MULLIGAN_DECLARED: usize = 125;
}
const _: () = assert!(
    glob::ROOM + 2 * crate::dungeon::ROOMS.len() <= glob::CHOOSER
        && glob::CHOOSER < glob::MULLIGAN_DECLARED
        && glob::MULLIGAN_DECLARED + 2 <= GLOBALS
);

/// A card code: the identity's digits (16, 8, 8), then types, colours, size.
fn code(out: &mut [f32], db: &CardDb, ids: &[u16], def: DefId) {
    code_of(out, db, ids, def, def);
}

/// The identity digits (16, 8, 8) of definition `def`'s identity.
fn digits(out: &mut [f32], ids: &[u16], def: DefId) {
    let i = ids[def as usize] as usize;
    out[(i >> 6) & 15] = 1.0;
    out[16 + ((i >> 3) & 7)] = 1.0;
    out[24 + (i & 7)] = 1.0;
}

/// A card code with the identity of `who` and the descriptor of `def` (a
/// copy: what it is, and the copiable values it has; ENGINE.md 26).
impl Game {
    /// A permanent's identity for viewer `me` (ENGINE.md 26, 31): what it is
    /// without copy effects; a face-down permanent's card only for a viewer
    /// who knows it (else the face-down definition: an unknown 2/2).
    fn perm_identity(&self, me: PlayerId, c: CardIdx) -> DefId {
        match self.face_down_id(c) {
            Some(d) if self.knows(me, c) => d,
            Some(_) => self.objs[c.i()].def,
            None => self.own_def(c),
        }
    }
}

fn code_of(out: &mut [f32], db: &CardDb, ids: &[u16], who: DefId, def: DefId) {
    digits(out, ids, who);
    let d = db.def(def);
    for (k, t) in [types::ARTIFACT, types::CREATURE, types::ENCHANTMENT, types::INSTANT, types::LAND, types::SORCERY]
        .iter()
        .enumerate()
    {
        if d.types & t != 0 {
            out[32 + k] = 1.0;
        }
    }
    for c in 0..5 {
        if d.colors & (1 << c) != 0 {
            out[38 + c] = 1.0;
        }
    }
    out[43] = d.mana_value() as f32 / 8.0;
    out[44] = d.power.unwrap_or(0) as f32 / 8.0;
    out[45] = d.toughness.unwrap_or(0) as f32 / 8.0;
    out[46] = d.token as u8 as f32;
    out[47] = (d.supertypes & crate::db::supertypes::LEGENDARY != 0) as u8 as f32;
    out[48] = d.is(types::PLANESWALKER) as u8 as f32;
    out[49] = d.loyalty.unwrap_or(0) as f32 / 8.0;
    out[50] = d.emblem as u8 as f32;
    // M7b part 2: a face other than the card's front is up (a transformed
    // permanent, an MDFC's back face, an Adventure on the stack; ENGINE.md
    // 25). The identity is the card's; the descriptor is the face's.
    out[51] = d.front.is_some() as u8 as f32;
}

fn bag_add(out: &mut [f32], ids: &[u16], b: usize, def: DefId) {
    out[O_BAGS + b * D + ids[def as usize] as usize] += 0.25;
}

/// Kinds of the asking task (context block).
fn task_code(t: &Task) -> usize {
    match t {
        Task::PriorityAsk => 0,
        Task::MulliganDeclare { .. } => 1,
        Task::MulliganBottom { .. } => 2,
        Task::DeclareAttackers { .. } => 3,
        Task::DeclareBlockers { .. } => 4,
        Task::AssignDamage { .. } => 5,
        Task::CleanupDiscard => 6,
        Task::ChooseTargets { .. } => 7,
        Task::PayMana(_) => 8,
        Task::Run(_) => 9,
        Task::PutTriggers => 10,
        Task::SbaCheck { .. } => 11,
        Task::PayCosts { .. } | Task::ManaNowCosts { .. } | Task::Emerge { .. } => 12,
        Task::CastOption { .. } => 13,
        Task::ChooseModes { .. } => 14,
        Task::ManaNowPick { .. } => 15,
        Task::AsEnters { .. } | Task::MaySkipTurn { .. } | Task::MiracleReveal { .. } | Task::Opening { .. } => 16,
        Task::CastPay { .. } | Task::ActPay { .. } => 17,
        Task::AttackTarget { .. } => 19,
        Task::ChooseFace { .. } => 20,
        Task::EnterAsCopy { .. } => 21,
        Task::CopyTargets { .. } => 22,
        Task::ChooseTimes { .. } | Task::Splice { .. } => 23,
        // M8a (ENGINE.md 35).
        Task::PlayDraw { .. } => 24,
        Task::RevealCompanion { .. } => 25,
        _ => 18,
    }
}

/// The asking-task code of sideboarding (no game task asks it; ENGINE.md
/// 36.7).
const TASK_SIDEBOARD: usize = 26;

/// The effect op a frame is at (context block); exhaustive, so a new op is
/// a compile error here.
fn op_code(op: &Op) -> usize {
    match op {
        Op::DealDamage { .. } => 0,
        Op::DamageAll { .. } => 1,
        Op::DamagePlayer { .. } => 2,
        Op::Draw { .. } => 3,
        Op::GainLife { .. } => 4,
        Op::AddMana { .. } => 5,
        Op::Counter { .. } => 6,
        Op::OfferPay { .. } => 7,
        Op::PutFromHandOnTop { .. } => 8,
        Op::DiscardRandom { .. } => 9,
        Op::Destroy { .. } => 10,
        Op::Exile { .. } => 11,
        Op::ExtraTurn { .. } => 12,
        Op::ReturnToHand { .. } => 13,
        Op::Untap { .. } => 14,
        Op::Discard { .. } => 15,
        Op::DiscardHand { .. } => 16,
        Op::LoseLife { .. } => 17,
        Op::Remember { .. } => 18,
        Op::DestroyAll { .. } => 19,
        Op::PutInLibrary { .. } => 20,
        Op::AddManaChoice { .. } => 21,
        Op::Jump { .. } => 22,
        Op::JumpUnless { .. } => 23,
        Op::JumpUnlessTarget { .. } => 24,
        Op::May { .. } => 25,
        Op::Search { .. } => 26,
        Op::LookTop { .. } => 27,
        Op::PileTo { .. } => 28,
        Op::Shuffle { .. } => 29,
        Op::SourceToLibraryTop => 30,
        Op::CreateToken { .. } => 31,
        Op::Sacrifice { .. } => 32,
        Op::ShuffleIntoLibrary { .. } => 33,
        Op::PutFromHand { .. } => 34,
        Op::RevealHand { .. } => 35,
        Op::ChooseDiscard { .. } => 36,
        Op::Ext(_) => 37,
    }
}

/// The context of the pending decision.
pub(crate) mod ctx {
    /// Decision kinds (one-hot; `DecisionKind as usize` must stay below `KINDS`).
    pub const KINDS: usize = 64;
    /// Asking-task codes (`task_code`).
    pub const TASKS: usize = 32;
    /// Effect-op codes (`op_code`).
    pub const OPS: usize = 64;
    pub const KIND: usize = 0;
    pub const DECIDER_IS_VIEWER: usize = KIND + KINDS;
    pub const PENDING: usize = DECIDER_IS_VIEWER + 1;
    pub const TASK: usize = PENDING + 1;
    pub const CARD: usize = TASK + TASKS; // a card code
    pub const HAS_CARD: usize = CARD + super::CODE;
    pub const SLOT_KIND: usize = HAS_CARD + 1; // 5
    pub const SLOT_INDEX: usize = SLOT_KIND + 5;
    pub const MODE: usize = SLOT_INDEX + 1;
    pub const OP: usize = MODE + 1;
    pub const PC: usize = OP + OPS;
    pub const OP_I: usize = PC + 1;
    pub const COST: usize = OP_I + 1; // 9
    pub const DAMAGE_LEFT: usize = COST + 9;
    pub const REQ: usize = DAMAGE_LEFT + 1;
    pub const NEXT_ATTACKER: usize = REQ + 1;
    pub const BLOCKER_INDEX: usize = NEXT_ATTACKER + 1;
    pub const BOTTOMS_LEFT: usize = BLOCKER_INDEX + 1;
    pub const PILE: usize = BOTTOMS_LEFT + 1;
    pub const OPTIONS: usize = PILE + 1;
    pub const MODES_CHOSEN: usize = OPTIONS + 1;
    pub const PICKED: usize = MODES_CHOSEN + 1;
    /// A `ChooseFace` decision: the front face is cast / played, the other
    /// face is cast / played (ENGINE.md 25).
    pub const FACES: usize = PICKED + 1;
    pub const END: usize = FACES + 4;
}
const _: () = assert!(ctx::END <= CONTEXT);

impl Game {
    /// The network input for `viewer` (module docs); `out` has
    /// [`NUM_FEATURES`] floats, zeroed by the caller.
    pub fn encode_features(&self, viewer: PlayerId, out: &mut [f32]) {
        assert_eq!(out.len(), NUM_FEATURES);
        let ids = identity::identities(self.db);
        let me = viewer;
        let op_ = opp(me);
        let decision = self.pending.as_ref();
        let mine_to_decide = decision.is_some_and(|d| d.player == me);
        let legal = |a: u16| -> bool {
            mine_to_decide && decision.is_some_and(|d| d.options.binary_search_by_key(&a, |o| o.action).is_ok())
        };
        self.encode_globals(me, &mut out[O_GLOBALS..O_GLOBALS + GLOBALS]);
        self.encode_bags(me, ids, out);
        // Tops of the libraries.
        for (side, p) in [me, op_].into_iter().enumerate() {
            let lib = &self.players[p as usize].library;
            for k in 0..TOPS.min(lib.len()) {
                let c = lib[lib.len() - 1 - k];
                let o = O_TOPS + (side * TOPS + k) * TOP_W;
                out[o] = 1.0;
                if self.knows(me, c) {
                    out[o + 1] = 1.0;
                    code(&mut out[o + 2..o + 2 + CODE], self.db, ids, self.objs[c.i()].def);
                }
            }
        }
        // Permanents.
        let targeted: Vec<CardIdx> = self
            .stack
            .iter()
            .flat_map(|s| s.targets.iter())
            .filter_map(|&(_, t)| match t {
                Target::Obj(r) if self.live(r) => Some(r.idx),
                _ => None,
            })
            .collect();
        let (asked_blocker, dmg_att, dmg_blk) = self.combat_context();
        let asked_attacker = match (self.pending.is_some(), self.agenda.last()) {
            (true, Some(Task::AttackTarget { card })) => Some(*card),
            _ => None,
        };
        let mine: Vec<CardIdx> = self.permanents_of(me).collect();
        let theirs: Vec<CardIdx> = self.permanents_of(op_).collect();
        let slot_of = |c: CardIdx| -> Option<usize> {
            let ctl = self.objs[c.i()].controller;
            let list = if ctl == me { &mine } else { &theirs };
            list.iter().position(|&x| x == c)
        };
        for (side, list) in [&mine, &theirs].into_iter().enumerate() {
            let base = if side == 0 { O_MY_PERM } else { O_OPP_PERM };
            let range = if side == 0 { act::MY_PERM } else { act::OPP_PERM };
            for (s, &c) in list.iter().enumerate().take(PERMS) {
                let o = base + s * PERM_W;
                let ob = &self.objs[c.i()];
                // What it is (its identity) and the copiable values it has
                // (the descriptor; a copy's copied face, CR 707.2).
                code_of(&mut out[o..o + CODE], self.db, ids, self.perm_identity(me, c), ob.def);
                out[o + 46] = self.is_token(c) as u8 as f32;
                // A copy's exceptions to the copiable values (CR 707.9b:
                // eternalize's black 4/4 with no mana cost).
                if let Some(e) = self.exc(c) {
                    for (k, t) in [types::ARTIFACT, types::CREATURE, types::ENCHANTMENT, types::INSTANT, types::LAND, types::SORCERY]
                        .iter()
                        .enumerate()
                    {
                        if e.types & t != 0 {
                            out[o + 32 + k] = 1.0;
                        }
                    }
                    if let Some(m) = e.colors {
                        for k in 0..5 {
                            out[o + 38 + k] = (m & (1 << k) != 0) as u8 as f32;
                        }
                    }
                    if e.no_cost {
                        out[o + 43] = 0.0;
                    }
                    if let Some((pw, th)) = e.pt {
                        out[o + 44] = pw as f32 / 8.0;
                        out[o + 45] = th as f32 / 8.0;
                    }
                }
                let st = &mut out[o + CODE..o + PERM_W];
                st[perm::PRESENT] = 1.0;
                st[perm::TAPPED] = ob.tapped as u8 as f32;
                st[perm::DAMAGE] = squash(ob.damage as f32 / 8.0);
                let (pw, th) = self.pt(c);
                st[perm::POWER] = squash(pw as f32 / 8.0);
                st[perm::TOUGHNESS] = squash(th as f32 / 8.0);
                st[perm::SICK] = (self.is_creature(c) && self.is_sick(c) && !self.has_kw(c, Kw::Haste)) as u8 as f32;
                st[perm::ATTACKING] = ob.attacking as u8 as f32;
                st[perm::BLOCKED] = ob.blocked as u8 as f32;
                if let Some(a) = ob.blocking {
                    st[perm::BLOCKING] = 1.0;
                    st[perm::BLOCKING_SLOT] = slot_of(a).map_or(0.0, |x| (x + 1) as f32 / 32.0);
                }
                st[perm::STRUCK_FIRST] = ob.struck_first as u8 as f32;
                st[perm::ASKED_BLOCKER] = (asked_blocker == Some(c)) as u8 as f32;
                st[perm::DAMAGE_ATTACKER] = (dmg_att == Some(c)) as u8 as f32;
                st[perm::DAMAGE_BLOCKER] = (dmg_blk == Some(c)) as u8 as f32;
                st[perm::OPTION] = legal(range + s as u16) as u8 as f32;
                if side == 0 {
                    for i in 0..4u16 {
                        st[perm::ACTIVATE + i as usize] = legal(act::ACTIVATE + s as u16 * 4 + i) as u8 as f32;
                    }
                }
                st[perm::TARGETED] = targeted.contains(&c) as u8 as f32;
                st[perm::BLOCKERS] =
                    squash(self.battlefield.iter().filter(|&&x| self.objs[x.i()].blocking == Some(c)).count() as f32 / 4.0);
                for (k, kw) in KWS.iter().enumerate() {
                    st[perm::KEYWORDS + k] = self.has_kw(c, *kw) as u8 as f32;
                }
                st[perm::TOKEN] = self.is_token(c) as u8 as f32;
                st[perm::NEW] = (ob.control_since == self.turn.number) as u8 as f32;
                st[perm::STOLEN] = (ob.controller != ob.owner) as u8 as f32;
                let walker = self.is_type(c, types::PLANESWALKER);
                st[perm::LOYALTY] = squash(self.counters_on(c, crate::db::Counter::Loyalty) as f32 / 8.0);
                st[perm::LOYALTY_READY] = (walker && self.loyalty_ready(c)) as u8 as f32;
                if let Some(Target::Obj(r)) = self.attack_dest(c) {
                    st[perm::ATTACKS_WALKER] = 1.0;
                    st[perm::ATTACKS_SLOT] = slot_of(r.idx).map_or(0.0, |x| (x + 1) as f32 / 32.0);
                }
                if walker {
                    let by = self
                        .battlefield
                        .iter()
                        .filter(|&&x| self.attack_dest(x) == Some(Target::Obj(self.obj_ref(c))))
                        .count();
                    st[perm::ATTACKED_BY] = squash(by as f32 / 4.0);
                }
                st[perm::ASKED_ATTACKER] = (asked_attacker == Some(c)) as u8 as f32;
                st[perm::LORE] = squash(self.counters_on(c, crate::db::Counter::Lore) as f32 / 4.0);
                st[perm::BACK_FACE] = self.db.is_face(ob.def) as u8 as f32;
                if self.is_copy(c) {
                    st[perm::COPY] = 1.0;
                    digits(&mut st[perm::COPY_OF..perm::COPY_OF + 32], ids, ob.def);
                }
                if let Some(h) = ob.attached.filter(|&h| self.live(h) && self.objs[h.idx.i()].zone == Zone::Battlefield) {
                    st[perm::ATTACHED] = 1.0;
                    st[perm::ATTACHED_SLOT] = slot_of(h.idx).map_or(0.0, |x| (x + 1) as f32 / 32.0);
                    st[perm::ATTACHED_OPP] = (self.objs[h.idx.i()].controller != me) as u8 as f32;
                }
                st[perm::PREPARED] = self.is_prepared(c) as u8 as f32;
                let wards = self.wards_of(c);
                st[perm::WARD] = wards.len() as f32 / 2.0;
                for w in wards {
                    match w {
                        WardCost::Mana(m) => {
                            st[perm::WARD_MANA] = st[perm::WARD_MANA].max(m.total() as f32 / 4.0)
                        }
                        WardCost::Life(n) => st[perm::WARD_LIFE] = st[perm::WARD_LIFE].max(n as f32 / 4.0),
                        WardCost::Discard => st[perm::WARD_DISCARD] = 1.0,
                    }
                }
                st[perm::LOST_ABILITIES] = self.lost_abilities(c) as u8 as f32;
                st[perm::GOADED] = [me, op_].iter().any(|&p| self.goaded_by(c, p)) as u8 as f32;
                st[perm::EXILE_IF_LEAVES] = self.flags_of(c).exile_if_leaves as u8 as f32;
            }
        }
        // The viewer's hand.
        let hand = self.hand_sorted(me);
        let opp_knows = |c: CardIdx| self.objs[c.i()].known & (1 << op_) != 0;
        for (s, &c) in hand.iter().enumerate().take(HANDS) {
            let o = O_HAND + s * HAND_W;
            let def = self.objs[c.i()].def;
            code(&mut out[o..o + CODE], self.db, ids, def);
            let st = &mut out[o + CODE..o + HAND_W];
            st[0] = 1.0;
            st[1] = hand.iter().filter(|&&x| self.objs[x.i()].def == def).count() as f32 / 4.0;
            st[2] = legal(act::HAND + s as u16) as u8 as f32;
            st[3] = legal(act::HAND_ABILITY + s as u16) as u8 as f32;
            st[4] = opp_knows(c) as u8 as f32;
            st[5] = self.def_of(c).is(types::LAND) as u8 as f32;
            // What its `HAND` id does: cast or play its front face, its other
            // face (an MDFC's back face, an Adventure), or both (a face
            // choice follows, ENGINE.md 25).
            if mine_to_decide {
                let (front, other) = match decision.and_then(|d| d.choice_of(act::HAND + s as u16)) {
                    Some(Choice::Cast(_)) | Some(Choice::Play(_)) => (true, false),
                    Some(Choice::CastFace(_)) | Some(Choice::PlayFace(_)) => (false, true),
                    Some(Choice::Faces(_, m)) => (m & (FACE_CAST | FACE_PLAY) != 0, m & (OTHER_CAST | OTHER_PLAY) != 0),
                    _ => (false, false),
                };
                st[6] = front as u8 as f32;
                st[7] = other as u8 as f32;
            }
        }
        // The stack, top first.
        let stack_targets: Vec<Target> = self.stack.iter().flat_map(|s| s.targets.iter().map(|&(_, t)| t)).collect();
        for (s, item) in self.stack.iter().rev().enumerate().take(STACKS) {
            let o = O_STACK + s * STACK_W;
            let (def, kind) = match item.kind {
                StackKind::Spell { card } => (self.objs[card.i()].def, 1),
                StackKind::Ability { ability, .. } => {
                    (ability.def, if matches!(ability.slot, AbilitySlot::Trigger(_)) { 3 } else { 2 })
                }
            };
            code(&mut out[o..o + CODE], self.db, ids, def);
            let st = &mut out[o + CODE..o + STACK_W];
            st[0] = 1.0;
            st[kind] = 1.0;
            st[4] = (item.controller == me) as u8 as f32;
            st[5] = item.modes.len() as f32 / 4.0;
            st[6] = item.cost_option as f32 / 4.0;
            st[7] = item.targets.len() as f32 / 4.0;
            match item.kind {
                StackKind::Spell { card } => {
                    st[stack::COPY] = self.is_copy_obj(card) as u8 as f32;
                    st[stack::TIMES] = item.times as f32 / 4.0;
                    st[stack::BESTOWED] = self.def_of(card).bestowed as u8 as f32;
                    st[stack::UNCOUNTERABLE] = self.spell_uncounterable(card) as u8 as f32;
                    st[stack::TARGETED] =
                        stack_targets.iter().any(|t| matches!(t, Target::Obj(r) if r.idx == card && self.live(*r))) as u8
                            as f32;
                }
                StackKind::Ability { ability, .. } => {
                    st[stack::TARGETED] = stack_targets.contains(&Target::Stack(item.id)) as u8 as f32;
                    let i = match ability.slot {
                        AbilitySlot::Spell => None,
                        AbilitySlot::Trigger(i) | AbilitySlot::Activated(i) | AbilitySlot::Hand(i) => Some(i),
                        AbilitySlot::Graveyard(i) => {
                            st[stack::GRAVEYARD] = 1.0;
                            Some(i)
                        }
                    };
                    st[stack::SLOT] = i.map_or(0.0, |i| (i as f32 + 1.0) / 8.0);
                }
            }
            for (k, &(_, t)) in item.targets.iter().take(2).enumerate() {
                let tb = &mut st[8 + 8 * k..16 + 8 * k];
                match t {
                    Target::Player(p) => tb[(p != me) as usize] = 1.0,
                    Target::Obj(r) if self.live(r) => match self.objs[r.idx.i()].zone {
                        Zone::Battlefield => {
                            let side = (self.objs[r.idx.i()].controller != me) as usize;
                            tb[2 + 2 * side] = 1.0;
                            tb[3 + 2 * side] = slot_of(r.idx).map_or(0.0, |x| x as f32 / 32.0);
                        }
                        Zone::Stack => {
                            tb[6] = 1.0;
                            let pos = self
                                .stack
                                .iter()
                                .rev()
                                .position(|x| x.kind == StackKind::Spell { card: r.idx })
                                .unwrap_or(0);
                            tb[7] = pos as f32 / 8.0;
                        }
                        _ => {}
                    },
                    Target::Obj(_) => {}
                    // An ability on the stack: its stack slot.
                    Target::Stack(id) => {
                        if let Some(pos) = self.stack.iter().rev().position(|x| x.id == id) {
                            tb[6] = 1.0;
                            tb[7] = pos as f32 / 8.0;
                        }
                    }
                }
            }
        }
        // The viewer's waiting triggers (the `TRIGGER` slots when it orders them).
        for (k, t) in self.waiting.iter().filter(|t| t.controller == me).enumerate().take(TRIGS) {
            let o = O_TRIG + k * TRIG_W;
            code(&mut out[o..o + CODE], self.db, ids, t.source_def);
            out[o + CODE] = 1.0;
            out[o + CODE + 1] = (t.slot as f32 + 1.0) / 4.0;
            out[o + CODE + 2] = legal(act::TRIGGER + k as u16) as u8 as f32;
        }
        // The `LIBRARY` list of the viewer's own decision.
        if mine_to_decide {
            let list = self.decision_list(me);
            for (k, &def) in list.iter().enumerate().take(LIST) {
                let o = O_LIST + k * LIST_W;
                code(&mut out[o..o + CODE], self.db, ids, def);
                out[o + CODE] = 1.0;
                out[o + CODE + 1] = legal(act::LIBRARY + k as u16) as u8 as f32;
                out[o + CODE + 2] = list.iter().filter(|&&x| x == def).count() as f32 / 4.0;
            }
        }
        // The cards in the viewer's graveyard with a graveyard ability (the
        // `GY_ABILITY` ids, graveyard order: `Game::activation_options`).
        for (k, c) in self.graveyard_cards(me).filter(|&c| !self.def_of(c).gy_abilities.is_empty()).take(GYAS).enumerate() {
            let o = O_GYA + k * GYA_W;
            code(&mut out[o..o + CODE], self.db, ids, self.objs[c.i()].def);
            out[o + CODE] = 1.0;
            out[o + CODE + 1] = legal(act::GY_ABILITY + k as u16) as u8 as f32;
        }
        // Revealed companions (public; layout v4, ENGINE.md 35).
        for (side, p) in [me, op_].into_iter().enumerate() {
            let pl = &self.players[p as usize];
            // (Its identity as revealed: once in the game the card may be
            // hidden again, a library card.)
            let Some(def) = pl.companion_def else { continue };
            let o = O_COMP + side * COMP_W;
            code(&mut out[o..o + CODE], self.db, ids, def);
            out[o + CODE + comp::PRESENT] = 1.0;
            out[o + CODE + comp::AVAILABLE] = self.companion_outside(p).is_some() as u8 as f32;
            let at_priority = decision.is_some_and(|d| d.kind == crate::decision::DecisionKind::Priority);
            out[o + CODE + comp::LEGAL] = (p == me && at_priority && legal(act::COMPANION)) as u8 as f32;
            out[o + CODE + comp::IN_GAME] = pl.companion_used as u8 as f32;
        }
        self.encode_context(me, ids, &mut out[O_CONTEXT..O_CONTEXT + CONTEXT]);
    }

    /// The ward abilities permanent `c` has (CR 702.21): its own and those a
    /// permanent its controller controls gives it ("other creatures you
    /// control have ward"), unless it lost them (613.7: a later "loses all
    /// abilities"; the `TrigPred::Granted` test).
    fn wards_of(&self, c: CardIdx) -> Vec<WardCost> {
        let mut v: Vec<WardCost> = self.abilities(c).ward.into_iter().collect();
        if self.is_creature(c) {
            let loss = self.loss_ts(c);
            for q in self.permanents_of(self.objs[c.i()].controller) {
                if q == c {
                    continue;
                }
                if let Some(w) = self.abilities(q).grants_ward {
                    if loss.is_none_or(|l| l < self.objs[q.i()].timestamp) {
                        v.push(w);
                    }
                }
            }
        }
        v
    }

    /// Per-identity counts.
    fn encode_bags(&self, me: PlayerId, ids: &[u16], out: &mut [f32]) {
        let op_ = opp(me);
        for &c in &self.players[me as usize].hand {
            bag_add(out, ids, bag::MY_HAND, self.objs[c.i()].def);
        }
        for &c in &self.battlefield {
            // (By what it is: a copy by its own identity, ENGINE.md 26.)
            let o = &self.objs[c.i()];
            bag_add(out, ids, if o.controller == me { bag::MY_BATTLEFIELD } else { bag::OPP_BATTLEFIELD }, self.perm_identity(me, c));
        }
        for p in [me, op_] {
            for &c in &self.players[p as usize].graveyard {
                bag_add(out, ids, if p == me { bag::MY_GRAVEYARD } else { bag::OPP_GRAVEYARD }, self.objs[c.i()].def);
            }
        }
        for &c in &self.exile {
            let o = &self.objs[c.i()];
            bag_add(out, ids, if o.owner == me { bag::MY_EXILE } else { bag::OPP_EXILE }, o.def);
        }
        // Cards exiled face down (ENGINE.md 30): by identity where the viewer
        // knows it (its owner saw it go there); the others are in the
        // opponent's hidden pool below.
        for &c in &self.face_down {
            let o = &self.objs[c.i()];
            if self.knows(me, c) {
                bag_add(out, ids, if o.owner == me { bag::MY_EXILE } else { bag::OPP_EXILE }, o.def);
            }
        }
        for &c in &self.command {
            // (Dungeons are in the globals, ENGINE.md 27.7.)
            if self.is_dungeon(c) {
                continue;
            }
            let o = &self.objs[c.i()];
            bag_add(out, ids, if o.controller == me { bag::MY_EMBLEMS } else { bag::OPP_EMBLEMS }, o.def);
        }
        for &c in &self.players[me as usize].library {
            bag_add(out, ids, bag::MY_LIBRARY, self.objs[c.i()].def);
            if self.knows(me, c) {
                bag_add(out, ids, bag::MY_LIBRARY_KNOWN, self.objs[c.i()].def);
            }
        }
        let opl = &self.players[op_ as usize];
        let opp_face_down = self.face_down.iter().copied().filter(|&c| self.objs[c.i()].owner == op_ && !self.knows(me, c));
        // (Their face-down permanents the viewer doesn't know: in the
        // hidden pool; ENGINE.md 31.)
        let opp_fd_perms = self.face_down_ids.iter().map(|x| x.0).filter(|&c| self.objs[c.i()].owner == op_ && !self.knows(me, c));
        // (With hidden decks the opponent's unknown cards are not read: the
        // match writes `OPP_HIDDEN` from what the viewer knows of their
        // pool, ENGINE.md 36.11.)
        let open = !self.spec.hidden_decks;
        for c in opl.hand.iter().copied().chain(opl.library.iter().copied()).chain(opp_face_down).chain(opp_fd_perms) {
            let d = self.ident(c);
            if !self.knows(me, c) {
                if open {
                    bag_add(out, ids, bag::OPP_HIDDEN, d);
                }
            } else if self.objs[c.i()].zone == Zone::Hand {
                bag_add(out, ids, bag::OPP_HAND_KNOWN, d);
            } else {
                bag_add(out, ids, bag::OPP_LIBRARY_KNOWN, d);
            }
        }
        for k in self.clouds.iter().filter(|k| k.viewer == me && k.owner == op_) {
            let b = if k.hand { bag::OPP_HAND_CLOUD } else { bag::OPP_LIBRARY_KNOWN };
            for &d in &k.defs {
                bag_add(out, ids, b, d);
            }
        }
    }

    fn encode_globals(&self, me: PlayerId, g: &mut [f32]) {
        let t = &self.turn;
        g[0] = squash(t.number as f32 / 50.0);
        g[1 + t.step as usize] = 1.0; // 14 steps: 1..15
        g[15] = (t.active == me) as u8 as f32;
        g[16] = (t.priority == me) as u8 as f32;
        g[17] = t.passes as f32 / 2.0;
        g[18] = (t.starting == me) as u8 as f32;
        g[19] = squash(self.spec.max_turns.saturating_sub(t.number) as f32 / 50.0);
        g[20] = squash(self.stack.len() as f32 / 8.0);
        g[21] = squash(self.waiting.len() as f32 / 4.0);
        g[22] = squash(self.extra_turns.len() as f32);
        g[23] = self.outcome.is_some() as u8 as f32;
        for (side, p) in [me, opp(me)].into_iter().enumerate() {
            let pl = &self.players[p as usize];
            let o = &mut g[24 + 40 * side..64 + 40 * side];
            o[0] = squash(pl.life as f32 / 20.0);
            o[1] = squash(pl.hand.len() as f32 / 10.0);
            o[2] = squash(pl.library.len() as f32 / 40.0);
            o[3] = squash(pl.graveyard.len() as f32 / 20.0);
            o[4] = squash(self.exile.iter().chain(self.face_down.iter()).filter(|&&c| self.objs[c.i()].owner == p).count() as f32 / 10.0);
            o[5] = pl.lands_played as f32;
            for k in 0..6 {
                o[6 + k] = pl.pool.amt[k] as f32 / 4.0;
            }
            o[12] = pl.mulligans as f32 / 4.0;
            o[13] = pl.kept as u8 as f32;
            o[14] = pl.drew_from_empty as u8 as f32;
            o[15] = pl.lost as u8 as f32;
            o[16] = squash(pl.turns as f32 / 25.0);
            o[17] = pl.left_battlefield as u8 as f32;
            // (Power and toughness summed in i64: no maximum, ENGINE.md 46.)
            let (mut perms, mut creatures, mut lands, mut untapped_lands, mut power, mut tough) = (0, 0, 0, 0, 0i64, 0i64);
            let (mut ready, mut artifacts, mut tokens) = (0, 0, 0);
            for c in self.permanents_of(p) {
                perms += 1;
                let ob = &self.objs[c.i()];
                if self.is_creature(c) {
                    creatures += 1;
                    let (pw, th) = self.pt(c);
                    power += pw.max(0) as i64;
                    tough += th.max(0) as i64;
                    if !ob.tapped && (!self.is_sick(c) || self.has_kw(c, Kw::Haste)) {
                        ready += 1;
                    }
                }
                if self.is_type(c, types::LAND) {
                    lands += 1;
                    if !ob.tapped {
                        untapped_lands += 1;
                    }
                }
                if self.is_type(c, types::ARTIFACT) {
                    artifacts += 1;
                }
                if self.is_token(c) {
                    tokens += 1;
                }
            }
            o[18] = squash(perms as f32 / 20.0);
            o[19] = squash(creatures as f32 / 10.0);
            o[20] = squash(lands as f32 / 10.0);
            o[21] = squash(untapped_lands as f32 / 10.0);
            o[22] = squash(power as f32 / 20.0);
            o[23] = squash(tough as f32 / 20.0);
            o[24] = squash(ready as f32 / 10.0);
            o[25] = squash(artifacts as f32 / 10.0);
            o[26] = squash(tokens as f32 / 10.0);
            o[27] = squash(self.extra_turns.iter().filter(|&&x| x == p).count() as f32);
            o[28] = pl.lost_life as u8 as f32;
            o[29] = pl.flash_sorceries as u8 as f32;
            // M7b part 2 (ENGINE.md 25).
            o[30] = squash(pl.poison as f32 / 10.0);
            o[31] = squash(
                self.permissions
                    .iter()
                    .filter(|pm| pm.player == p && pm.kind == crate::game::PermKind::Adventure && self.live(pm.card))
                    .count() as f32
                    / 4.0,
            );
            o[32] = squash(self.temp_triggers.iter().filter(|t| t.controller == p).count() as f32 / 2.0);
            o[33] = self.no_max_hand(p) as u8 as f32;
            // M7b part 3b (ENGINE.md 27.4).
            let (room, completed) = self.dungeon(p);
            o[glob::INITIATIVE] = self.has_initiative(p) as u8 as f32;
            o[glob::IN_DUNGEON] = room.is_some() as u8 as f32;
            o[glob::COMPLETED] = completed as f32 / 2.0;
            o[glob::EXPERIENCE] = squash(pl.experience as f32 / 4.0);
            // Layout v4 (ENGINE.md 35).
            o[glob::SIDEBOARD] = self.sideboard_size(p) as f32 / 15.0;
            o[glob::COMPANION] = self.companion_outside(p).is_some() as u8 as f32;
        }
        g[glob::CHOOSER] = (t.chooser == me) as u8 as f32;
        for (side, p) in [me, opp(me)].into_iter().enumerate() {
            if let (Some(r), _) = self.dungeon(p) {
                g[glob::ROOM + crate::dungeon::ROOMS.len() * side + r as usize] = 1.0;
            }
            // ENGINE.md 55: a declared mulligan not yet taken (public).
            g[glob::MULLIGAN_DECLARED + side] = self.players[p as usize].mulligan_declared as u8 as f32;
        }
        // Day and night (CR 731).
        g[104] = (self.day == Some(true)) as u8 as f32;
        g[105] = (self.day == Some(false)) as u8 as f32;
    }

    /// The potential blocker being asked (DeclareBlockers), and the attacker
    /// and blocker of a damage assignment (AssignDamage).
    fn combat_context(&self) -> (Option<CardIdx>, Option<CardIdx>, Option<CardIdx>) {
        if self.pending.is_none() {
            return (None, None, None);
        }
        match self.agenda.last() {
            Some(Task::DeclareBlockers { i }) => (self.potential_blockers().get(*i as usize).copied(), None, None),
            Some(Task::AssignDamage { att, j, .. }) => {
                let blk = self.blockers_of(*att);
                (None, Some(*att), blk.get(*j as usize).copied())
            }
            _ => (None, None, None),
        }
    }

    /// The definitions behind the `LIBRARY` slots of `p`'s pending decision:
    /// a searched library, a looked-at pile, a seen hand (sorted by
    /// definition, ENGINE.md 11).
    fn decision_list(&self, p: PlayerId) -> Vec<DefId> {
        use crate::decision::DecisionKind as K;
        let Some(d) = &self.pending else { return Vec::new() };
        // A companion to reveal (ENGINE.md 35): `LIBRARY` + k is the k-th
        // candidate, by definition.
        if d.kind == K::RevealCompanion {
            return self.companion_candidates(p);
        }
        let cards: Vec<CardIdx> = match d.kind {
            K::SearchLibrary | K::NameCard => self.library_sorted(p),
            K::PutOnTop | K::PutOnBottom | K::PutInGraveyard | K::PutInHand | K::PutOntoBattlefield => match self.agenda.last() {
                Some(Task::Run(f)) => {
                    let mut v = f.pile.clone();
                    v.sort_by_key(|c| (self.objs[c.i()].def, c.0));
                    v
                }
                _ => Vec::new(),
            },
            K::ChooseDiscard => {
                let owner = d
                    .options
                    .iter()
                    .find_map(|o| match o.choice {
                        crate::decision::Choice::Card(c) => Some(self.objs[c.i()].owner),
                        _ => None,
                    })
                    .unwrap_or(opp(p));
                self.hand_sorted(owner)
            }
            // Bolas's Citadel (ENGINE.md 31): the top card of the decider's
            // library, which they may look at any time, cast or played at
            // `LIBRARY` + 0.
            K::Priority => d
                .options
                .iter()
                .find_map(|o| match o.choice {
                    crate::decision::Choice::Cast(c) | crate::decision::Choice::Play(c) if o.action == act::LIBRARY => {
                        Some(vec![c])
                    }
                    _ => None,
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        cards.iter().map(|c| self.objs[c.i()].def).collect()
    }

    fn encode_context(&self, me: PlayerId, ids: &[u16], x: &mut [f32]) {
        let Some(d) = &self.pending else { return };
        assert!((d.kind as usize) < ctx::KINDS, "decision kind {:?} beyond the context block", d.kind);
        x[ctx::KIND + d.kind as usize] = 1.0;
        x[ctx::DECIDER_IS_VIEWER] = (d.player == me) as u8 as f32;
        x[ctx::PENDING] = 1.0;
        if d.player == me {
            x[ctx::OPTIONS] = d.options.len() as f32 / 32.0;
        }
        let Some(task) = self.agenda.last() else { return };
        let t = task_code(task);
        assert!(t < ctx::TASKS);
        x[ctx::TASK + t] = 1.0;
        // The card behind the decision: the resolving or paid-for ability,
        // the spell being cast, the land entering, the attacker.
        let mut card: Option<(DefId, Option<AbilitySlot>, Option<u8>)> = None;
        let stack_item = |id: u32| -> Option<(DefId, Option<AbilitySlot>, Option<u8>)> {
            let i = self.item_index(id)?;
            Some(match self.stack[i].kind {
                StackKind::Spell { card } => (self.objs[card.i()].def, Some(AbilitySlot::Spell), None),
                StackKind::Ability { ability, .. } => (ability.def, Some(ability.slot), ability.mode),
            })
        };
        let frame_card = |t: &Task| -> Option<(DefId, Option<AbilitySlot>, Option<u8>)> {
            match t {
                Task::Run(f) => Some((f.ability.def, Some(f.ability.slot), f.ability.mode)),
                Task::ChooseTargets { id, .. }
                | Task::CastOption { id }
                | Task::ChooseModes { id }
                | Task::PayCosts { id, .. }
                | Task::CastPay { id }
                | Task::ActPay { id }
                | Task::CopyTargets { id, .. }
                | Task::ChooseTimes { id }
                | Task::Splice { id } => stack_item(*id),
                _ => None,
            }
        };
        match task {
            Task::Run(f) => {
                card = frame_card(task);
                let prog = &self.db.ability(f.ability).program;
                if let Some(op) = prog.get(f.pc as usize) {
                    let o = op_code(op);
                    assert!(o < ctx::OPS);
                    x[ctx::OP + o] = 1.0;
                }
                x[ctx::PC] = squash(f.pc as f32 / 32.0);
                x[ctx::OP_I] = f.i as f32 / 8.0;
                x[ctx::PILE] = squash(f.pile.len() as f32 / 8.0);
            }
            Task::PayMana(pm) => {
                // The cost being paid: the nearest task below that names it.
                card = self.agenda.iter().rev().skip(1).find_map(&frame_card);
                let c = &pm.cost;
                x[ctx::COST] = c.generic as f32 / 8.0;
                for k in 0..6 {
                    x[ctx::COST + 1 + k] = c.pips[k] as f32 / 4.0;
                }
                x[ctx::COST + 7] = c.hybrid.iter().map(|&n| n as f32).sum::<f32>() / 4.0;
                x[ctx::COST + 8] = c.phyrexian.iter().map(|&n| n as f32).sum::<f32>() / 4.0;
            }
            Task::AsEnters { card: c } => card = Some((self.objs[c.i()].def, None, None)),
            Task::AssignDamage { att, left, .. } => {
                card = Some((self.objs[att.i()].def, None, None));
                x[ctx::DAMAGE_LEFT] = squash(*left as f32 / 16.0);
            }
            Task::ChooseTargets { req, .. } => {
                card = frame_card(task);
                x[ctx::REQ] = *req as f32 / 4.0;
            }
            Task::ChooseModes { id } => {
                card = frame_card(task);
                if let Some(i) = self.item_index(*id) {
                    x[ctx::MODES_CHOSEN] = self.stack[i].modes.len() as f32 / 4.0;
                }
            }
            Task::PayCosts { picked, .. } => {
                card = frame_card(task);
                x[ctx::PICKED] = picked.len() as f32 / 4.0;
            }
            Task::DeclareAttackers { next } => x[ctx::NEXT_ATTACKER] = *next as f32 / 32.0,
            Task::AttackTarget { card: c } => card = Some((self.objs[c.i()].def, None, None)),
            // The card entering as a copy (by what it is), the copy whose
            // targets may change (its target position).
            Task::EnterAsCopy { card: c, .. } => card = Some((self.own_def(*c), None, None)),
            Task::CopyTargets { k, .. } => {
                card = frame_card(task);
                x[ctx::REQ] = *k as f32 / 4.0;
            }
            Task::ChooseFace { card: c, mask, .. } => {
                card = Some((self.objs[c.i()].def, None, None));
                for (k, bit) in [FACE_CAST, FACE_PLAY, OTHER_CAST, OTHER_PLAY].into_iter().enumerate() {
                    x[ctx::FACES + k] = (mask & bit != 0) as u8 as f32;
                }
            }
            Task::DeclareBlockers { i } => x[ctx::BLOCKER_INDEX] = *i as f32 / 16.0,
            Task::MulliganBottom { left, .. } => x[ctx::BOTTOMS_LEFT] = *left as f32 / 7.0,
            Task::ManaNow { src } => card = Some((self.objs[src.card.i()].def, Some(src.slot), None)),
            _ => card = frame_card(task),
        }
        if let Some((def, slot, mode)) = card {
            code(&mut x[ctx::CARD..ctx::CARD + CODE], self.db, ids, def);
            x[ctx::HAS_CARD] = 1.0;
            if let Some(s) = slot {
                let (k, i) = match s {
                    AbilitySlot::Spell => (0, 0),
                    AbilitySlot::Trigger(i) => (1, i),
                    AbilitySlot::Activated(i) => (2, i),
                    AbilitySlot::Hand(i) => (3, i),
                    AbilitySlot::Graveyard(i) => (4, i),
                };
                x[ctx::SLOT_KIND + k] = 1.0;
                x[ctx::SLOT_INDEX] = i as f32 / 4.0;
            }
            if let Some(m) = mode {
                x[ctx::MODE] = (m as f32 + 1.0) / 8.0;
            }
        }
    }
}

/// The match block and the match bags of `a` for `viewer` (ENGINE.md 36.7);
/// the rest of `out` is the game's (`Game::encode_features`) or
/// sideboarding's.
pub fn encode_match(a: &super::MtgAz, viewer: PlayerId, out: &mut [f32]) {
    let m = &a.m;
    let me = viewer;
    let op_ = opp(me);
    let db = a.g.db();
    let ids = identity::identities(db);
    let x = &mut out[O_MATCH..O_MATCH + MATCH];
    x[mtch::BO3] = (m.format == super::Format::Bo3) as u8 as f32;
    x[mtch::GAME] = m.game_no as f32 / 3.0;
    x[mtch::MY_WINS] = m.wins[me as usize] as f32 / 2.0;
    x[mtch::OPP_WINS] = m.wins[op_ as usize] as f32 / 2.0;
    x[mtch::DRAWS] = m.draws as f32 / 3.0;
    for i in 0..3 {
        let r = m.results[i];
        if r == 0 {
            continue;
        }
        let k = match r {
            3 => 2,
            w if w - 1 == me => 0,
            _ => 1,
        };
        x[mtch::RESULTS + 3 * i + k] = 1.0;
        x[mtch::STARTED + i] = (m.starters[i] == me) as u8 as f32;
        x[mtch::CHOSE + i] = (m.choosers[i] == me) as u8 as f32;
    }
    let size = |c: &[u8]| c.iter().map(|&n| n as f32).sum::<f32>() / 40.0;
    x[mtch::MY_DECK] = size(&m.decks[me as usize]);
    x[mtch::OPP_DECK] = size(&m.decks[op_ as usize]);
    if let Some(sb) = &m.sb {
        x[mtch::SIDEBOARDING] = 1.0;
        x[mtch::SB_FIRST] = (sb.order[0] == me) as u8 as f32;
        x[mtch::SB_DONE] = sb.done[me as usize].is_some() as u8 as f32;
        if sb.decider() == me {
            x[mtch::MY_DECK] = size(&sb.counts);
            x[mtch::LAST_ENTRY] = sb.last.map_or(0.0, |l| (l as f32 + 1.0) / 64.0);
            x[mtch::COUNT_ENTRY] = sb.asking.map_or(0.0, |k| (k as f32 + 1.0) / 64.0);
        } else if let Some(d) = &sb.done[me as usize] {
            x[mtch::MY_DECK] = size(d);
        }
    }
    // The opponent's pool as the viewer knows it (best-of-one, open
    // decklists: all of it; best-of-three: but the first picks not revealed
    // in an earlier game, ENGINE.md 36.11) and what they revealed before.
    let pool = &m.pools[op_ as usize];
    let hidden = m.format == super::Format::Bo3;
    for &(def, n) in &pool.list[..pool.n_cards] {
        if !hidden || super::sample::known_before(m, op_, ids, def) {
            out[O_BAGS + bag::OPP_POOL * D + ids[def as usize] as usize] += 0.25 * n as f32;
        }
    }
    if hidden && m.sb.is_none() {
        opp_candidates(a, me, ids, out);
    }
    for (w, &bits) in m.earlier[op_ as usize].iter().enumerate() {
        let mut b = bits;
        while b != 0 {
            let i = w * 64 + b.trailing_zeros() as usize;
            out[O_BAGS + bag::OPP_REVEALED * D + i] += 0.25;
            b &= b - 1;
        }
    }
}

/// `bag::OPP_HIDDEN` with hidden decks (ENGINE.md 36.11): the cards of the
/// opponent's pool the viewer knows of (`OPP_POOL`), less those of their
/// cards it knows are in the game (exactly, or by its clouds) and a revealed
/// companion; basic lands are left out. What the viewer knows only: the same
/// in every world of its.
fn opp_candidates(a: &super::MtgAz, me: PlayerId, ids: &[u16], out: &mut [f32]) {
    let g = &a.g;
    let m = &a.m;
    let op_ = opp(me);
    let pool = &m.pools[op_ as usize];
    let mut left: Vec<(DefId, i32)> = pool.list[..pool.n_cards]
        .iter()
        .filter(|e| super::sample::known_before(m, op_, ids, e.0))
        .map(|e| (e.0, e.1 as i32))
        .collect();
    let mut drop = |d: DefId| {
        if let Ok(i) = left.binary_search_by_key(&d, |e| e.0) {
            left[i].1 -= 1;
        }
    };
    for i in 0..g.objs.len() {
        let c = crate::ids::CardIdx(i as u16);
        let o = &g.objs[i];
        if o.owner == op_ && o.zone != Zone::Nowhere && !g.is_token(c) && !g.is_copy_obj(c) && g.knows(me, c) {
            drop(super::sample::card_of(g, c));
        }
    }
    for k in g.clouds.iter().filter(|k| k.viewer == me && k.owner == op_) {
        for &d in &k.defs {
            drop(g.db().def(d).card_def(d));
        }
    }
    if let Some(d) = g.players[op_ as usize].companion_def {
        if g.players[op_ as usize].companion.is_some_and(|c| g.objs[c.i()].zone == Zone::Nowhere) {
            drop(d);
        }
    }
    for (d, n) in left {
        if n > 0 {
            out[O_BAGS + bag::OPP_HIDDEN * D + ids[d as usize] as usize] += 0.25 * n as f32;
        }
    }
}

/// The encoding while sideboarding (ENGINE.md 36.7): the match block and
/// bags, the context (the decision kind, the decider, the entry a
/// `SideboardCount` is about) and, for the decider, the pool list in the
/// list block. The game blocks are empty (the last game is over). The
/// other player's deck being built is not shown (a simultaneous choice).
pub fn encode_sideboarding(a: &super::MtgAz, viewer: PlayerId, out: &mut [f32]) {
    assert_eq!(out.len(), NUM_FEATURES);
    encode_match(a, viewer, out);
    let sb = a.sideboarding().expect("sideboarding");
    let db = a.g.db();
    let ids = identity::identities(db);
    let d = &sb.decision;
    let mine = d.player == viewer;
    let x = &mut out[O_CONTEXT..O_CONTEXT + CONTEXT];
    // (The other player sees that sideboarding goes on, not which of its
    // decisions: a count says a basic land or a card with copies was
    // chosen, ENGINE.md 36.11.)
    let kind = if mine { d.kind } else { crate::decision::DecisionKind::Sideboard };
    x[ctx::KIND + kind as usize] = 1.0;
    x[ctx::DECIDER_IS_VIEWER] = mine as u8 as f32;
    x[ctx::PENDING] = 1.0;
    x[ctx::TASK + TASK_SIDEBOARD] = 1.0;
    if !mine {
        return;
    }
    x[ctx::OPTIONS] = d.options.len() as f32 / 32.0;
    let pool = &a.m.pools[viewer as usize];
    if let Some(k) = sb.asking {
        code(&mut x[ctx::CARD..ctx::CARD + CODE], db, ids, pool.list[k as usize].0);
        x[ctx::HAS_CARD] = 1.0;
    }
    let legal = |id: u16| d.options.binary_search_by_key(&id, |o| o.action).is_ok();
    for (k, &(def, n)) in pool.list.iter().enumerate().take(LIST) {
        let o = O_LIST + k * LIST_W;
        code(&mut out[o..o + CODE], db, ids, def);
        let s = &mut out[o + CODE..o + LIST_W];
        let basic = k >= pool.n_cards;
        s[list::PRESENT] = 1.0;
        s[list::OPTION] = (d.kind == crate::decision::DecisionKind::Sideboard && legal(act::LIBRARY + k as u16)) as u8 as f32;
        s[list::COPIES] = if basic { 0.0 } else { n as f32 / 4.0 };
        s[list::IN_DECK] = sb.counts[k] as f32 / 4.0;
        s[list::AT_START] = sb.start[k] as f32 / 4.0;
        s[list::FIRST_PICK] = pool.first_picks.binary_search(&def).is_ok() as u8 as f32;
        s[list::BASIC] = basic as u8 as f32;
        s[list::DECIDED] = sb.last.is_some_and(|l| k <= l as usize) as u8 as f32;
    }
}

/// The object layout of this encoding for zero-core's token network (arch `tokens`, zero-core
/// `python/zero/net.py` `TokenNet`; ENGINE.md 22): JSON with the identity capacity as the
/// vocabulary, the object segments (slot offsets and widths, the present flag, the identity as
/// one-hot digits of 16, 8 and 8, the action ids each slot scores) and the bags (17 multisets of
/// 1024 identities). Every other feature (the 128 globals and the match block) is the global
/// token's input.
/// Checked in as `mtgz/az/layout_v4.json` (a test keeps the two equal). `cards` (the name behind
/// each identity of `db`, for analysis; null where none) is not read by the network.
pub fn layout_json(db: &'static CardDb) -> String {
    use serde_json::json;
    let id = json!({"at": 0, "radix": [16, 8, 8]});
    let seg = |name: &str, offset: usize, count: usize, width: usize, present: usize, actions: serde_json::Value| {
        json!({"name": name, "offset": offset, "count": count, "width": width, "present": present,
               "id": id.clone(), "actions": actions})
    };
    let a = |base: u16, stride: u16| json!([base, stride]);
    let ids = identity::identities(db);
    let mut cards: Vec<Option<String>> = vec![None; D];
    for (def, &i) in ids.iter().enumerate() {
        // An emblem shares the identity of the card that makes it, a face
        // its card's.
        if i != identity::UNKNOWN && !db.def(def as DefId).emblem && !db.is_face(def as DefId) {
            cards[i as usize] = Some(db.def(def as DefId).name.clone());
        }
    }
    let v = json!({
        "version": 4,
        "features": NUM_FEATURES,
        "vocab": D,
        "segments": [
            {"name": "context", "offset": O_CONTEXT, "count": 1, "width": CONTEXT, "present": null,
             "id": {"at": ctx::CARD, "radix": [16, 8, 8]}},
            {"name": "tops", "offset": O_TOPS, "count": 2 * TOPS, "width": TOP_W, "present": 1,
             "id": {"at": 2, "radix": [16, 8, 8]}},
            seg("my_permanents", O_MY_PERM, PERMS, PERM_W, CODE + perm::PRESENT,
                json!([a(act::MY_PERM, 1), a(act::ACTIVATE, 4), a(act::ACTIVATE + 1, 4), a(act::ACTIVATE + 2, 4),
                       a(act::ACTIVATE + 3, 4)])),
            seg("opponent_permanents", O_OPP_PERM, PERMS, PERM_W, CODE + perm::PRESENT, json!([a(act::OPP_PERM, 1)])),
            seg("hand", O_HAND, HANDS, HAND_W, CODE, json!([a(act::HAND, 1), a(act::HAND_ABILITY, 1)])),
            seg("stack", O_STACK, STACKS, STACK_W, CODE, json!([a(act::STACK, 1)])),
            seg("triggers", O_TRIG, TRIGS, TRIG_W, CODE, json!([a(act::TRIGGER, 1)])),
            seg("list", O_LIST, LIST, LIST_W, CODE, json!([a(act::LIBRARY, 1)])),
            seg("graveyard_abilities", O_GYA, GYAS, GYA_W, CODE, json!([a(act::GY_ABILITY, 1)])),
            // Layout v4 (ENGINE.md 35): the companion special action is the
            // viewer's companion's.
            seg("my_companion", O_COMP, 1, COMP_W, CODE + comp::PRESENT, json!([a(act::COMPANION, 1)])),
            seg("opponent_companion", O_COMP + COMP_W, 1, COMP_W, CODE + comp::PRESENT, json!([])),
        ],
        "bags": [{"name": "bags", "offset": O_BAGS, "count": BAGS, "width": D}],
        "cards": cards,
    });
    serde_json::to_string_pretty(&v).unwrap() + "\n"
}

/// Keywords in the permanent status, in this order.
const KWS: [Kw; 15] = [
    Kw::Flying,
    Kw::Reach,
    Kw::FirstStrike,
    Kw::DoubleStrike,
    Kw::Deathtouch,
    Kw::Trample,
    Kw::Lifelink,
    Kw::Vigilance,
    Kw::Haste,
    Kw::Defender,
    Kw::Menace,
    Kw::Flash,
    Kw::Hexproof,
    Kw::Shroud,
    Kw::Indestructible,
];
