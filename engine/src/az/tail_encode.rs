//! Layout v7's variable-length part (ENGINE.md 125; zero-core docs/search.md 1.4, 2.15, docs/az.md
//! 43): the tail tokens of the seven capped segments, the seven tail-only (gated) segments, the
//! blocks they share (an object reference, a triggered ability's event), the frame's picks, and the
//! pointer overrides.
//!
//! * **Head + tail.** Each object of a capped segment is written by one writer (`encode.rs`), into
//!   its head slot when its absolute position is below the segment's head slots, else into a tail
//!   token at that position ([`put`]). Positions are absolute on a range page too (a page's options
//!   point at their objects by overrides, [`option_overrides`]).
//! * **Tail-only segments** (`count` 0, `gate: true`: a widen adds them at gate 0, so a widened
//!   network computes the old one's outputs exactly): the other player's waiting triggers, the
//!   delayed and temporary triggers, the links between objects, the exile offers, the library cards
//!   someone knows below the top three, the stack items' relations past the head's, the order of
//!   the graveyard cards a positional reader can pick, the face-down cards (who knows each), and the
//!   identities the frame's player revealed in earlier games of a match.
//! * **No hidden information.** An identity is written only where the viewer knows it
//!   (`Game::knows`); positions, zones, counts and who knows are public.

use std::cell::RefCell;

use zero_search::tail::{pack_tok, TailBuf, TailSeg};

use super::encode::{self, code, digits, squash, CODE};
use crate::decision::{act, Choice, Decision, OptObject};
use crate::flow::Task;
use crate::game::{Game, Mark, StackKind};
use crate::ids::{CardIdx, DefId, ObjRef, PlayerId, Target, Zone};
use crate::triggers::EvCtx;

/// Segment indices of layout v7 (the layout's `segments` list; ENGINE.md 125).
pub mod seg {
    pub const CONTEXT: u16 = 0;
    pub const TOPS: u16 = 1;
    pub const MY_PERM: u16 = 2;
    pub const OPP_PERM: u16 = 3;
    pub const HAND: u16 = 4;
    pub const STACK: u16 = 5;
    pub const TRIGGERS: u16 = 6;
    pub const LIST: u16 = 7;
    pub const GYA: u16 = 8;
    pub const MY_COMP: u16 = 9;
    pub const OPP_COMP: u16 = 10;
    pub const OPP_TRIGGERS: u16 = 11;
    pub const DELAYED: u16 = 12;
    pub const LINKS: u16 = 13;
    pub const EXILE_OFFERED: u16 = 14;
    pub const LIBRARY_KNOWN: u16 = 15;
    pub const STACK_RELATIONS: u16 = 16;
    pub const GRAVEYARD_ORDER: u16 = 17;
    pub const FACE_DOWN: u16 = 18;
    pub const MY_REVEALED: u16 = 19;
    /// ENGINE.md 142.7: continuous effects, counters, numbers past float16's exact range.
    pub const EFFECTS: u16 = 20;
    pub const COUNTERS: u16 = 21;
    pub const NUMBERS: u16 = 22;
    /// ENGINE.md 142.8: the other player's earlier reveals, by game (`mr::*`, as `MY_REVEALED`'s).
    pub const OPP_REVEALED: u16 = 23;
    /// ENGINE.md 142.12: the cards found by their owner's library search that the other player still
    /// tells apart (`Game::found`, ENGINE.md 140.9).
    pub const FOUND: u16 = 24;
    /// The number of segments (layout v7).
    pub const N: usize = 25;
    /// Layout v8 (ENGINE.md 236, the game `mtg_v8`): tail-only gated segments appended after v7's
    /// (`tail_v8`): a payment's state and picks in progress, the payment decisions' option
    /// meanings, a gone object's last known information (by its stale key), a gone spell's item
    /// and its relations, and the groups of places holding search finds mixed with other cards.
    pub const PAYMENT: u16 = 25;
    pub const PAY_OPTIONS: u16 = 26;
    pub const LKI: u16 = 27;
    pub const LKI_SPELLS: u16 = 28;
    pub const LKI_RELATIONS: u16 = 29;
    pub const FIND_GROUPS: u16 = 30;
    /// The number of segments of layout v8.
    pub const N_V8: usize = 31;
}

/// An object reference (`ObjRef`) inside a token: what it names now, publicly (ENGINE.md 125).
/// `ZONE` is the object's zone while the reference is live (the same incarnation, CR 400.7);
/// `MINE` / `THEIRS` its controller (battlefield, stack) or owner (elsewhere) is the frame's
/// player or the other; `POS` its place, (k + 1) / 32: among its controller's permanents
/// (timestamp order, the segments' slot), from the top of the stack, in the frame player's sorted
/// hand, in exile or face-down exile (exile order), among the phased-out permanents (0 in a
/// graveyard, which is unordered, a library or the command zone); `STALE_KEY` for a reference
/// that no longer names its object: (k + 1) / 32, k its rank among the row's stale references in
/// the order the encoder meets them (equal keys: the same gone object, Memory Jar's links and its
/// delayed trigger); `DIGITS` the identity (`encode::digits`) where the viewer knows it: a
/// permanent's as its slot shows it, any other card's only where `knows`. A live reference to a
/// library card the viewer lost track of (`Game::ref_seen_live`, ENGINE.md 235) is written as a
/// stale one (a key, no zone, no identity): whether it still names its object is not the viewer's
/// to know. An object named by where it is now (a find by its place, a pick in progress, a
/// counter's or a face-down card's object: `Cx::write_obj`) is live.
pub mod rf {
    pub const PRESENT: usize = 0;
    pub const LIVE: usize = 1;
    pub const ZONE: usize = 2;
    pub const ZONES: usize = 10;
    pub const MINE: usize = ZONE + ZONES;
    pub const THEIRS: usize = MINE + 1;
    pub const POS: usize = THEIRS + 1;
    pub const STALE_KEY: usize = POS + 1;
    pub const DIGITS: usize = STALE_KEY + 1;
    pub const W: usize = DIGITS + 32;
}
const _: () = assert!(rf::W == 48);

/// A triggered ability's event (`EvCtx`: "that player", "that creature", "that much", the spell
/// or ability it is about; ENGINE.md 125): on the trigger slots of both players, delayed and
/// temporary triggers, and stack items. `AMOUNT` is the amount / 8 (squashed), `AMOUNT_BITS` its
/// 64 binary digits (exact: Mana Drain's mana, a note); `ITEM` the event's stack item, its place
/// from the top while it is on the stack.
pub mod ev {
    use super::rf;
    pub const PLAYER: usize = 0;
    pub const PLAYER_ME: usize = 1;
    pub const PLAYER_OPP: usize = 2;
    pub const OBJ: usize = 3;
    pub const OBJ2: usize = OBJ + rf::W;
    pub const ITEM: usize = OBJ2 + rf::W;
    pub const ITEM_ON_STACK: usize = ITEM + 1;
    pub const ITEM_POS: usize = ITEM + 2;
    pub const AMOUNT: usize = ITEM + 3;
    pub const AMOUNT_BITS: usize = AMOUNT + 1;
    pub const W: usize = AMOUNT_BITS + 64;
}
const _: () = assert!(ev::W == 167);

/// A delayed or temporary triggered ability (`delayed` segment: `Game::delayed`, then
/// `Game::temp_triggers`, each in its order), after the card code of its source definition:
/// which struct, when (`DelayedWhen`, one-hot), until the end of this turn (a temporary one),
/// its controller, its trigger slot (k + 1) / 8, created this turn and the turns since (squashed /
/// 4: "your next turn" reads the turn it was made in), its source object and its event (a
/// temporary trigger's object in the event's `OBJ`).
pub mod dl {
    use super::{ev, rf};
    pub const PRESENT: usize = 0;
    pub const KIND_DELAYED: usize = 1;
    pub const KIND_TEMP: usize = 2;
    pub const WHEN: usize = 3;
    pub const WHENS: usize = 7;
    pub const THIS_TURN: usize = WHEN + WHENS;
    pub const MINE: usize = THIS_TURN + 1;
    pub const THEIRS: usize = MINE + 1;
    pub const SLOT: usize = THEIRS + 1;
    pub const CREATED_THIS_TURN: usize = SLOT + 1;
    pub const TURNS_SINCE: usize = CREATED_THIS_TURN + 1;
    pub const SOURCE: usize = TURNS_SINCE + 1;
    pub const EV: usize = SOURCE + rf::W;
    pub const W: usize = EV + ev::W;
}

/// A link (`links` segment, ENGINE.md 125): a relation between two objects, or an object and an
/// ability, that no slot holds, any number of them. After the card code of `B` (the exiled card,
/// the granted ability's card, the picked card, ...): its kind (one-hot, `K_*`), object `A` (the
/// source the card was exiled with, the permanent or spell a mark is on) and `B` as references,
/// the trigger slot, `Link`'s duration fields, and the kinds' own fields.
pub mod lk {
    use super::rf;
    pub const PRESENT: usize = 0;
    pub const KIND: usize = 1;
    /// `Game::links`: B was exiled with A (CR 610.3, 607.2a).
    pub const K_EXILED_WITH: usize = 0;
    /// `Mark::Granted`: permanent A has trigger slot `SLOT` of B's card (CR 613.1f).
    pub const K_GRANTED: usize = 1;
    /// `Mark::LeaveTrigger`: permanent A has that leave-the-battlefield trigger (Serra Paragon).
    pub const K_LEAVE_TRIGGER: usize = 2;
    /// `Mark::GrantOnEnter`: spell A gives the permanent it becomes that trigger.
    pub const K_GRANT_ON_ENTER: usize = 3;
    /// `Mark::Triggered`: the granted trigger of B's card triggered `N` times this turn for A,
    /// granted by object `B` (Nadu's "only twice each turn").
    pub const K_TRIGGERED: usize = 4;
    /// `Mark::Reanimated`: Aura A put creature B onto the battlefield.
    pub const K_REANIMATED: usize = 5;
    /// `Mark::ExiledToPay`: spell A's delve exiled card B (CR 607.2q).
    pub const K_EXILED_TO_PAY: usize = 6;
    /// A pick in progress of the pending decision's frame (`picks`): object B at rank `RANK`.
    pub const K_PICKED: usize = 7;
    /// A library pick in progress by definition (a search for several cards: the searcher's own
    /// rows only): the card code's definition at rank `RANK`.
    pub const K_PICKED_DEF: usize = 8;
    /// `Mark::DelvePromised`: hand card B is promised to the delve of the spell being cast
    /// (revealed: its cost discards it; ENGINE.md 100.4.9).
    pub const K_DELVE_PROMISED: usize = 9;
    pub const KINDS: usize = 10;
    pub const A: usize = KIND + KINDS;
    pub const B: usize = A + rf::W;
    pub const SLOT: usize = B + rf::W;
    pub const UNTIL: usize = SLOT + 1;
    pub const BACK: usize = UNTIL + 1;
    pub const MONARCH: usize = BACK + rf::ZONES;
    pub const MONARCH_ME: usize = MONARCH + 1;
    pub const MONARCH_OPP: usize = MONARCH + 2;
    pub const THIS_TURN: usize = MONARCH + 3;
    pub const ACTIVE: usize = THIS_TURN + 1;
    pub const EXILE: usize = ACTIVE + 1;
    pub const N: usize = EXILE + 1;
    pub const CURRENT_TURN: usize = N + 1;
    pub const RANK: usize = CURRENT_TURN + 1;
    pub const BY_ME: usize = RANK + 1;
    pub const BY_OPP: usize = BY_ME + 1;
    /// A pick in progress (`K_PICKED`, `K_PICKED_DEF`) of an enclosing task (ENGINE.md 142.10, X3):
    /// its depth / 8 - 0 the innermost holder's (the head's pick floats are its), k the k-th task
    /// holding picks around it (Etali, Primal Conqueror's frame keeps the exiled cards while a cast
    /// from among them pays a cost that picks objects).
    pub const DEPTH: usize = BY_OPP + 1;
    pub const W: usize = DEPTH + 1;
}

/// An exile offer (`exile_offered` segment): the `EXILE` range's item at this slot's position of
/// the viewer's pending decision (ENGINE.md 125; v6 had no token for them, 100.5.1), after the
/// card's code: face down, its owner, the option's choice (one-hot: cast, play, cast the other
/// face, play the other face, both faces, another choice) and face bits, the permission the viewer
/// plays it by (kind one-hot, free, any mana, a tax / 4, this turn only), its place in exile.
pub mod xo {
    pub const PRESENT: usize = 0;
    pub const FACE_DOWN: usize = 1;
    pub const MINE: usize = 2;
    pub const THEIRS: usize = 3;
    pub const CHOICE: usize = 4;
    pub const CHOICES: usize = 6;
    pub const FACES: usize = CHOICE + CHOICES;
    pub const PERM: usize = FACES + 4;
    pub const PERM_KIND: usize = PERM + 1;
    pub const PERM_KINDS: usize = 8;
    pub const FREE: usize = PERM_KIND + PERM_KINDS;
    pub const ANY_MANA: usize = FREE + 1;
    pub const CAST_ONLY: usize = ANY_MANA + 1;
    pub const TAX: usize = CAST_ONLY + 1;
    pub const THIS_TURN: usize = TAX + 1;
    pub const POS: usize = THIS_TURN + 1;
    pub const W: usize = POS + 1;
}

/// A library card someone knows below the top three (`library_known` segment; the top three are
/// the tops block, ENGINE.md 72): after its card code (where the viewer knows it), whose library,
/// its depth (k + 1) / 64 and k's 16 binary digits (k from the top), who knows it.
pub mod lb {
    pub const PRESENT: usize = 0;
    pub const MINE: usize = 1;
    pub const THEIRS: usize = 2;
    pub const DEPTH: usize = 3;
    pub const DEPTH_BITS: usize = 4;
    pub const OWNER_KNOWS: usize = DEPTH_BITS + 16;
    pub const OTHER_KNOWS: usize = OWNER_KNOWS + 1;
    pub const VIEWER_KNOWS: usize = OTHER_KNOWS + 1;
    pub const W: usize = VIEWER_KNOWS + 1;
}

/// A stack item's relation the stack slot doesn't hold exactly (`stack_relations` segment): its
/// targets past the first two and any target the head's target block can't name (a card in a
/// graveyard, exile, a hand; a gone object), the divided amounts past the first two, the noted
/// objects past the first and a first one not on the battlefield, the spliced cards, a ninjutsu
/// creature's planeswalker. After the card code of the object or spliced card (where seen): the
/// kind, the item's place from the top, the index in its list, a target's requirement, a player,
/// the object, an amount.
pub mod sr {
    use super::rf;
    pub const PRESENT: usize = 0;
    pub const KIND: usize = 1;
    pub const K_TARGET: usize = 0;
    pub const K_DIVISION: usize = 1;
    pub const K_NOTED: usize = 2;
    pub const K_SPLICED: usize = 3;
    pub const K_ATTACK_AS: usize = 4;
    pub const KINDS: usize = 5;
    pub const ITEM_POS: usize = KIND + KINDS;
    pub const INDEX: usize = ITEM_POS + 1;
    pub const REQ: usize = INDEX + 1;
    pub const PLAYER: usize = REQ + 1;
    pub const PLAYER_ME: usize = PLAYER + 1;
    pub const PLAYER_OPP: usize = PLAYER + 2;
    pub const OBJ: usize = PLAYER + 3;
    pub const AMOUNT: usize = OBJ + rf::W;
    pub const AMOUNT_BITS: usize = AMOUNT + 1;
    pub const W: usize = AMOUNT_BITS + 64;
}

/// The order of a graveyard's cards that a positional reader of the card table can pick
/// (`graveyard_order` segment; `lower::graveyard_order`, R17): per reader and player (the frame's
/// first), each card its filter matches, top first, after its card code: whose graveyard, its rank
/// from the top (k + 1) / 32 and k's 16 binary digits, the reader's identity (the smallest identity
/// among the cards with that reader). Graveyards are otherwise unordered (bags).
pub mod go {
    pub const PRESENT: usize = 0;
    pub const MINE: usize = 1;
    pub const THEIRS: usize = 2;
    pub const RANK: usize = 3;
    pub const RANK_BITS: usize = 4;
    pub const READER: usize = RANK_BITS + 16;
    pub const W: usize = READER + 32;
}

/// A face-down card (`face_down` segment, ENGINE.md 125.7): every card exiled face down
/// (`Game::face_down`, in exile order), then every face-down permanent (`Game::face_down_ids`), after
/// its card code where the viewer knows it: the object (a reference: zone, controller or owner,
/// place, identity where known), its owner, who knows it (the owner, the other player, the viewer)
/// and who may play it (an exiled one's permission, `Game::exile_permission`). Face-up and face-down
/// exiled cards differ to the rules (a "card in exile" target, a warped creature card, a cast from
/// exile read face-up ones), and what each player knows of a face-down card is the viewer's
/// knowledge: the owner's bags show a known one like a face-up one, these tokens what it is.
pub mod fd {
    use super::rf;
    pub const PRESENT: usize = 0;
    pub const EXILED: usize = 1;
    pub const PERMANENT: usize = 2;
    pub const OBJ: usize = 3;
    pub const OWNER_MINE: usize = OBJ + rf::W;
    pub const OWNER_THEIRS: usize = OWNER_MINE + 1;
    pub const OWNER_KNOWS: usize = OWNER_THEIRS + 1;
    pub const OTHER_KNOWS: usize = OWNER_KNOWS + 1;
    pub const VIEWER_KNOWS: usize = OTHER_KNOWS + 1;
    pub const PLAY_ME: usize = VIEWER_KNOWS + 1;
    pub const PLAY_OPP: usize = PLAY_ME + 1;
    pub const W: usize = PLAY_OPP + 1;
}

/// An identity the frame's player revealed (made public) in an earlier game of the match
/// (`my_revealed` segment, `MatchState::earlier`; ENGINE.md 125.7): what the other player knows of
/// its pool, by identity (the card code's identity digits; slot: the identity); and, the same token,
/// an identity the other player revealed (`opp_revealed` segment, ENGINE.md 142.8; the bag
/// `OPP_REVEALED` counts them too). `GAME`: in which earlier games (`MatchState::earlier_by_game`,
/// games 1-3: the user's decision of 2026-11-13, how a player boarded game 2 is an input for game
/// 3, so game 2's reveals are told apart from game 1's).
pub mod mr {
    pub const PRESENT: usize = 0;
    pub const GAME: usize = 1;
    pub const W: usize = GAME + 3;
}

/// A card found by its owner's library search that the other player still tells apart (`found`
/// segment, ENGINE.md 142.12; `Game::found`, ENGINE.md 140.9: a tutor's card put on top of the
/// library, a Doomsday pile's cards, a find exiled face down - until it is seen, goes into a hand or
/// is shuffled with cards that are not finds), slot k its place in the order of the searches. After
/// the card code where the viewer knows the card (`Cx::ref_def`: its owner, who found it; the other
/// player knows it only once it is no find): the object (`rf::*`, `Cx::write_ref`: its zone, its
/// owner, a face-down card's place) and, in a library, its depth from the top + 1 / 64 and in 16
/// binary digits. Slot k is the k-th told-apart find by place (ENGINE.md 223), not the k-th search.
pub mod fs {
    use super::rf;
    pub const PRESENT: usize = 0;
    pub const OBJ: usize = 1;
    pub const DEPTH: usize = OBJ + rf::W;
    pub const DEPTH_BITS: usize = DEPTH + 1;
    pub const W: usize = DEPTH_BITS + 16;
}

/// A continuous effect (`effects` segment, ENGINE.md 142.7): one token per `Game::effects` entry
/// (a resolved spell's or ability's effect, CR 611.2), slot k its place in timestamp order (CR
/// 613.7: the layers apply effects in that order), then one per copy effect (`Game::copies`, CR
/// 707), after the effects. After the affected permanent's card code where the viewer sees it
/// (zero for a group's rules effect): the affected object (a reference), or the group's filter
/// (`group_filter`: card types, and whose - any player's, the effect's controller's, its
/// opponents'); the `Mod` (`mod_index`, one-hot: the 21 variants an effect can have) and its
/// parameters - power and toughness amounts (squashed, sign, 64 binary digits: exact), keywords,
/// protection's qualities, colours, card types and `replace`, up to `SUBS` subtypes (each a stable
/// name id's 32 digits, `name_id`), the mana a granted sacrifice ability makes, which latent
/// ability, whose control it was; its duration (`Until`, one-hot, and the object "for as long as"
/// names) and its controller ("you" of "until your next turn", the goader). A copy effect: the
/// copied definition's identity, its exceptions (CR 707.9: card types, subtypes, keywords, base
/// power and toughness, a name, colours, no mana cost, supertypes), double-faced, its duration.
pub mod ef {
    use super::rf;
    pub const PRESENT: usize = 0;
    pub const COPY: usize = 1;
    pub const OBJ: usize = 2;
    pub const GROUP: usize = OBJ + rf::W;
    pub const GROUP_TYPES: usize = GROUP + 1;
    pub const TYPE_N: usize = 9;
    pub const GROUP_ANY: usize = GROUP_TYPES + TYPE_N;
    pub const GROUP_YOURS: usize = GROUP_ANY + 1;
    pub const GROUP_OPPONENTS: usize = GROUP_ANY + 2;
    pub const MOD: usize = GROUP_ANY + 3;
    pub const MODS: usize = 21;
    /// A number: squashed / 8, negative, 64 binary digits of its magnitude.
    pub const NUM_W: usize = 66;
    pub const POWER: usize = MOD + MODS;
    pub const PT_PRESENT: usize = POWER + NUM_W;
    pub const TOUGHNESS: usize = PT_PRESENT + 1;
    pub const KWS: usize = TOUGHNESS + NUM_W;
    pub const KW_N: usize = 22;
    pub const PROT: usize = KWS + KW_N;
    pub const PROT_N: usize = 19;
    pub const COLORS: usize = PROT + PROT_N;
    pub const COLORS_SET: usize = COLORS + 6;
    pub const TYPES: usize = COLORS_SET + 1;
    pub const REPLACE: usize = TYPES + TYPE_N;
    pub const SUBTYPES_N: usize = REPLACE + 1;
    pub const SUBTYPES: usize = SUBTYPES_N + 1;
    pub const SUBS: usize = 4;
    pub const SAC_MASK: usize = SUBTYPES + SUBS * 32;
    pub const LATENT: usize = SAC_MASK + 6;
    pub const LATENT_N: usize = 8;
    pub const CONTROL_FROM_ME: usize = LATENT + LATENT_N;
    pub const CONTROL_FROM_OPP: usize = CONTROL_FROM_ME + 1;
    pub const UNTIL: usize = CONTROL_FROM_OPP + 1;
    pub const UNTIL_N: usize = 4;
    pub const UNTIL_OBJ: usize = UNTIL + UNTIL_N;
    pub const CONTROLLER_ME: usize = UNTIL_OBJ + rf::W;
    pub const CONTROLLER_OPP: usize = CONTROLLER_ME + 1;
    /// A copy effect's copied definition (32 identity digits), its exception's name (present, 32
    /// digits), no mana cost, supertypes (8 bits), double-faced.
    pub const COPY_DEF: usize = CONTROLLER_OPP + 1;
    pub const EXC_NAME: usize = COPY_DEF + 32;
    pub const EXC_NAME_DIGITS: usize = EXC_NAME + 1;
    pub const EXC_NO_COST: usize = EXC_NAME_DIGITS + 32;
    pub const EXC_SUPERS: usize = EXC_NO_COST + 1;
    pub const COPY_DFC: usize = EXC_SUPERS + 8;
    pub const W: usize = COPY_DFC + 1;
}

/// The counters of one kind on one object (`counters` segment, ENGINE.md 142.7): one token per
/// (object, kind) of every permanent (the frame player's, then the other's, in their slots'
/// order), every face-up exiled card (exile order) and every phased-out permanent, kinds in their
/// order: after the object's card code, the object (a reference), the kind (+1/+1, -1/-1, stun, a
/// keyword counter, a named counter, loyalty, lore; CR 122.1), a keyword counter's keyword, a named
/// counter's name (a stable name id's 32 digits, `name_id`), the number (squashed / 4, 64 binary
/// digits: exact).
pub mod ct {
    use super::rf;
    pub const PRESENT: usize = 0;
    pub const OBJ: usize = 1;
    pub const KIND: usize = OBJ + rf::W;
    pub const KINDS: usize = 7;
    pub const KW: usize = KIND + KINDS;
    pub const NAME: usize = KW + super::ef::KW_N;
    pub const COUNT: usize = NAME + 32;
    pub const COUNT_BITS: usize = COUNT + 1;
    pub const W: usize = COUNT_BITS + 64;
}

/// A number the head writes squashed past float16's exact range (`numbers` segment, ENGINE.md
/// 142.7; zero-core's shards store floats as float16): one token per such number whose magnitude
/// is at least `BIG` - a player's life, poison, energy and experience counters (the frame player
/// first), a permanent's power, toughness and damage (the frame player's permanents, then the
/// other's) - with whose it is (a player, or the permanent as a reference), which number, its sign
/// and its magnitude's 128 binary digits (power and toughness are 128-bit, ENGINE.md 100.7). Below
/// `BIG` the head's float is exact in float16 for every one of them (life / 20 to 1,280; power,
/// toughness, damage / 8 and the counters / 4 to the squash threshold).
pub mod nb {
    use super::rf;
    pub const BIG: u128 = 512;
    pub const PRESENT: usize = 0;
    pub const PLAYER_ME: usize = 1;
    pub const PLAYER_OPP: usize = 2;
    pub const OBJ: usize = 3;
    pub const WHICH: usize = OBJ + rf::W;
    pub const LIFE: usize = 0;
    pub const POISON: usize = 1;
    pub const ENERGY: usize = 2;
    pub const EXPERIENCE: usize = 3;
    pub const POWER: usize = 4;
    pub const TOUGHNESS: usize = 5;
    pub const DAMAGE: usize = 6;
    pub const WHICH_N: usize = 7;
    pub const NEG: usize = WHICH + WHICH_N;
    pub const BITS: usize = NEG + 1;
    pub const W: usize = BITS + 128;
}

pub const DELAYED_W: usize = CODE + dl::W;
pub const LINK_W: usize = CODE + lk::W;
pub const EXO_W: usize = CODE + xo::W;
pub const LK_W: usize = CODE + lb::W;
pub const SR_W: usize = CODE + sr::W;
pub const GO_W: usize = CODE + go::W;
pub const FD_W: usize = CODE + fd::W;
pub const MR_W: usize = CODE + mr::W;
pub const EF_W: usize = CODE + ef::W;
pub const CT_W: usize = CODE + ct::W;
pub const NB_W: usize = CODE + nb::W;
pub const FS_W: usize = CODE + fs::W;

/// Every segment's slot width in layout v7, by segment index.
pub const SEG_W: [usize; seg::N] = [
    encode::CONTEXT,
    encode::TOP_W,
    encode::PERM_W,
    encode::PERM_W,
    encode::HAND_W,
    encode::STACK_W,
    encode::TRIG_W,
    encode::LIST_W,
    encode::GYA_W,
    encode::COMP_W,
    encode::COMP_W,
    encode::TRIG_W,
    DELAYED_W,
    LINK_W,
    EXO_W,
    LK_W,
    SR_W,
    GO_W,
    FD_W,
    MR_W,
    EF_W,
    CT_W,
    NB_W,
    MR_W,
    FS_W,
];

/// Every segment's head slots (`count`) in layout v7.
pub const SEG_HEAD: [usize; seg::N] = [1, 2 * encode::TOPS, encode::PERMS, encode::PERMS, encode::HANDS, encode::STACKS, encode::TRIGS, encode::LIST, encode::GYAS, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// The layout's head tokens, `1 + sum count` (zero-core `SearchGame::HEAD_TOKENS`): the global
/// token and every head slot.
pub const HEAD_TOKENS: usize = {
    let mut n = 1;
    let mut i = 0;
    while i < seg::N {
        n += SEG_HEAD[i];
        i += 1;
    }
    n
};
const _: () = assert!(HEAD_TOKENS == 170);

/// A row's resource limit (`SearchGame::MAX_ROW_TOKENS`, the user's decision D4): past it the
/// game is aborted, counted and logged by zero-core's driver (never truncated).
pub const MAX_ROW_TOKENS: usize = 4096;

const fn ts(seg: u16, name: &'static str) -> TailSeg {
    TailSeg { seg, name, width: SEG_W[seg as usize], head: SEG_HEAD[seg as usize] }
}

/// The tail segments of layout v7, in layout order (zero-core `SearchGame::TAILS`).
pub const TAILS: [TailSeg; 21] = [
    ts(seg::MY_PERM, "my_permanents"),
    ts(seg::OPP_PERM, "opponent_permanents"),
    ts(seg::HAND, "hand"),
    ts(seg::STACK, "stack"),
    ts(seg::TRIGGERS, "triggers"),
    ts(seg::LIST, "list"),
    ts(seg::GYA, "graveyard_abilities"),
    ts(seg::OPP_TRIGGERS, "opponent_triggers"),
    ts(seg::DELAYED, "delayed"),
    ts(seg::LINKS, "links"),
    ts(seg::EXILE_OFFERED, "exile_offered"),
    ts(seg::LIBRARY_KNOWN, "library_known"),
    ts(seg::STACK_RELATIONS, "stack_relations"),
    ts(seg::GRAVEYARD_ORDER, "graveyard_order"),
    ts(seg::FACE_DOWN, "face_down"),
    ts(seg::MY_REVEALED, "my_revealed"),
    ts(seg::EFFECTS, "effects"),
    ts(seg::COUNTERS, "counters"),
    ts(seg::NUMBERS, "numbers"),
    ts(seg::OPP_REVEALED, "opp_revealed"),
    ts(seg::FOUND, "found"),
];

/// Every segment's slot width in layout v8 (ENGINE.md 236): v7's, then the six tail-only
/// segments of `tail_v8`.
pub const SEG_W_V8: [usize; seg::N_V8] = {
    let mut w = [0usize; seg::N_V8];
    let mut i = 0;
    while i < seg::N {
        w[i] = SEG_W[i];
        i += 1;
    }
    w[seg::PAYMENT as usize] = super::tail_v8::PM_W;
    w[seg::PAY_OPTIONS as usize] = super::tail_v8::PO_W;
    w[seg::LKI as usize] = super::tail_v8::LK8_W;
    w[seg::LKI_SPELLS as usize] = super::tail_v8::LS_W;
    w[seg::LKI_RELATIONS as usize] = super::tail_v8::LR_W;
    w[seg::FIND_GROUPS as usize] = super::tail_v8::FG_W;
    w
};

/// A segment's slot width (layouts v7 and v8: v8 only appends segments).
pub fn seg_w(sg: u16) -> usize {
    SEG_W_V8[sg as usize]
}

/// A segment's head slots (v8's appended segments have none).
pub fn seg_head(sg: u16) -> usize {
    SEG_HEAD.get(sg as usize).copied().unwrap_or(0)
}

const fn ts8(seg: u16, name: &'static str) -> TailSeg {
    TailSeg { seg, name, width: SEG_W_V8[seg as usize], head: 0 }
}

/// The tail segments of layout v8 (the game `mtg_v8`, ENGINE.md 236): v7's, then its own.
pub const TAILS_V8: [TailSeg; 27] = {
    let mut t = [ts8(seg::PAYMENT, "payment"); 27];
    let mut i = 0;
    while i < TAILS.len() {
        t[i] = TAILS[i];
        i += 1;
    }
    t[21] = ts8(seg::PAYMENT, "payment");
    t[22] = ts8(seg::PAY_OPTIONS, "pay_options");
    t[23] = ts8(seg::LKI, "lki");
    t[24] = ts8(seg::LKI_SPELLS, "lki_spells");
    t[25] = ts8(seg::LKI_RELATIONS, "lki_relations");
    t[26] = ts8(seg::FIND_GROUPS, "find_groups");
    t
};

/// The index of a zone in a reference's one-hot (exhaustive: a new zone is a compile error here).
fn zone_index(z: Zone) -> usize {
    match z {
        Zone::Battlefield => 0,
        Zone::Stack => 1,
        Zone::Graveyard => 2,
        Zone::Exile => 3,
        Zone::FaceDown => 4,
        Zone::Hand => 5,
        Zone::Library => 6,
        Zone::Command => 7,
        Zone::PhasedOut => 8,
        Zone::Nowhere => 9,
    }
}

/// n's binary digits, least significant first, into `out`.
fn bits(out: &mut [f32], n: u64) {
    for (b, x) in out.iter_mut().enumerate() {
        *x = (n >> b & 1) as f32;
    }
}

/// Write object `s` (its absolute position) of segment `sg` with `f`, which fills a zeroed slice
/// of the segment's layout v7 width: into its head slot when `s` is below the segment's head
/// slots (layout v6, narrower: v7's slot written aside, its first v6 floats copied: v7 only adds
/// floats at a slot's end), else into a tail token at `s` (layout v7), else nowhere (layout v6
/// has no tail: `mtg_eoe`'s encoding, ENGINE.md 124.2).
pub(crate) fn put(out: &mut [f32], lay: &encode::Lay, tail: &mut Option<&mut TailBuf>, scratch: &mut Vec<f32>, sg: u16, s: usize, f: impl FnOnce(&mut [f32])) {
    let i = sg as usize;
    let w7 = SEG_W[i];
    if s < SEG_HEAD[i] {
        let (off, w) = (lay.off[i], lay.w[i]);
        let o = off + s * w;
        if w == w7 {
            f(&mut out[o..o + w]);
        } else {
            scratch.clear();
            scratch.resize(w7, 0.0);
            f(scratch);
            out[o..o + w].copy_from_slice(&scratch[..w]);
        }
    } else if let Some(t) = tail.as_deref_mut() {
        f(t.token(sg, s as u32));
    }
}

/// The objects the pending decision's frame has picked so far (lane A's "picks in progress",
/// ENGINE.md 125): `Frame::picked` of the resolution asking (any op's remembered objects, in
/// order: Ext::Choose, SacrificeN, Balance, Doomsday's graveyard picks, a vote's two votes,
/// ...), or the picks of the task asking (`PayCosts`, `OrderTimestamps`, `Devour`); `defs`, a
/// library search's picks by definition (`Frame::picked_defs`), for the searcher only.
#[derive(Default)]
pub(crate) struct Picks {
    /// (object, picked by the frame player): in pick order, repeats kept (votes).
    pub objs: Vec<(CardIdx, bool)>,
    pub defs: Vec<DefId>,
    pub searcher: Option<PlayerId>,
    /// The enclosing tasks' picks (ENGINE.md 142.10, X3), innermost first: depth k + 1 for entry
    /// k; `links` tokens only (`lk::DEPTH`).
    pub outer: Vec<Picks>,
}

impl Picks {
    /// The rank (1-based, its first pick) and the times picked by the frame player and the other
    /// of object `c`.
    pub fn of(&self, c: CardIdx) -> Option<(usize, usize, usize)> {
        let first = self.objs.iter().position(|&(x, _)| x == c)?;
        let me = self.objs.iter().filter(|&&(x, m)| x == c && m).count();
        let opp = self.objs.iter().filter(|&&(x, m)| x == c && !m).count();
        Some((first + 1, me, opp))
    }
}

/// Per-encode context shared by the slot writers (`encode.rs`) and the tail segments' writers.
pub(crate) struct Cx<'g> {
    pub g: &'g Game,
    pub ids: &'static [u16],
    /// The viewer, whose knowledge the encoding shows.
    pub knower: PlayerId,
    /// The frame's player ("me", `Game::encoding_seat`) and the other.
    pub me: PlayerId,
    pub op_: PlayerId,
    /// The pending decision when the knower makes it in the frame's seat (its options are the
    /// encoding's; ENGINE.md 100.5).
    pub d: Option<&'g Decision>,
    /// The frame's permanents and the other player's, in timestamp order.
    pub mine: Vec<CardIdx>,
    pub theirs: Vec<CardIdx>,
    /// The frame player's hand, sorted (the hand slots' order).
    pub hand: Vec<CardIdx>,
    /// The cards exiled face down (`Game::face_down`) in their canonical order for the knower
    /// (`face_down_order`): the `face_down` tokens' slots and a reference's place.
    pub fd: Vec<CardIdx>,
    pub picks: Picks,
    stale: RefCell<Vec<ObjRef>>,
}

/// The cards exiled face down in the order the encoding shows them to `knower` (ENGINE.md 142.3):
/// sorted by what the knower can tell of each - whose (the frame player's first), known to the
/// knower or not (known first), its identity where known, who knows it, who may play it - and in
/// exile order among cards alike in all of these. `Game::face_down` holds them in the order they
/// were exiled, and several at once in the order of the zone they came from: Memory Jar exiles each
/// hand in the hand vector's order, so a card the knower knew among the opponent's hidden cards
/// sat at a place that told how many of them had been drawn before it (the Fable review's B7). The
/// public view has no order for the pile (`Game::view_of`: a multiset per owner), and no rule reads
/// one; cards alike for the knower in every field above are interchangeable in its row.
pub(crate) fn face_down_order(g: &Game, knower: PlayerId, me: PlayerId) -> Vec<CardIdx> {
    let mut v = g.face_down.clone();
    let op_ = crate::ids::opp(me);
    v.sort_by_cached_key(|&c| {
        let o = &g.objs[c.i()];
        let known = g.knows(knower, c);
        (
            (o.owner != me) as u8,
            !known as u8,
            if known { o.def as u32 + 1 } else { 0 },
            o.known & 3,
            g.exile_permission(me, c).is_some() as u8,
            g.exile_permission(op_, c).is_some() as u8,
        )
    });
    v
}

impl<'g> Cx<'g> {
    pub fn new(g: &'g Game, knower: PlayerId) -> Cx<'g> {
        let me = g.encoding_seat(knower);
        let op_ = crate::ids::opp(me);
        let d = g.pending.as_ref().filter(|d| d.player == me && g.decider_of(d.player) == knower);
        let mut cx = Cx {
            g,
            ids: super::identity::identities(g.db),
            knower,
            me,
            op_,
            d,
            mine: g.permanents_of(me).collect(),
            theirs: g.permanents_of(op_).collect(),
            hand: g.hand_sorted(me),
            fd: face_down_order(g, knower, me),
            picks: Picks::default(),
            stale: RefCell::new(Vec::new()),
        };
        cx.picks = cx.picks_in_progress();
        cx
    }

    /// Is `a` an option of the knower's decision?
    pub fn legal(&self, a: u16) -> bool {
        self.d.is_some_and(|d| d.options.binary_search_by_key(&a, |o| o.action).is_ok())
    }

    /// The id the object at absolute position `pos` of range `base .. base + n` has on this
    /// decision's page (whose first position is `first`), if it has one: on the page, and not its
    /// "next page" id.
    pub fn opt_id(&self, base: u16, n: u16, first: usize, pos: usize) -> Option<u16> {
        if pos < first || pos - first >= n as usize {
            return None;
        }
        let a = base + (pos - first) as u16;
        match self.d.and_then(|d| d.choice_of(a)) {
            Some(Choice::Page(_)) => None,
            _ => Some(a),
        }
    }

    /// The object at position `pos` of that range is an option of the knower's decision.
    pub fn opt_legal(&self, base: u16, n: u16, first: usize, pos: usize) -> bool {
        self.opt_id(base, n, first, pos).is_some_and(|a| self.legal(a))
    }

    /// The page's first position of range `base .. base + n` (0 without a knower's decision).
    pub fn first(&self, base: u16, n: u16) -> usize {
        self.d.map_or(0, |d| d.range_first(base, n) as usize)
    }

    pub fn first_perms(&self, side: usize) -> usize {
        self.d.map_or(0, |d| d.range_first_perms(side) as usize)
    }

    /// A permanent's place: (side, position) among the frame's or the other player's.
    pub fn perm_pos(&self, c: CardIdx) -> Option<(usize, usize)> {
        let ctl = self.g.objs[c.i()].controller;
        let (side, list) = if ctl == self.me { (0, &self.mine) } else { (1, &self.theirs) };
        list.iter().position(|&x| x == c).map(|p| (side, p))
    }

    /// The place from the top of the stack item with id `id`.
    pub fn stack_pos(&self, id: u32) -> Option<usize> {
        let s = &self.g.stack;
        s.iter().position(|x| x.id == id).map(|i| s.len() - 1 - i)
    }

    /// The place from the top of spell `card`'s stack item.
    pub fn spell_pos(&self, card: CardIdx) -> Option<usize> {
        let s = &self.g.stack;
        s.iter().position(|x| x.kind == StackKind::Spell { card }).map(|i| s.len() - 1 - i)
    }

    /// The key of a stale reference (`rf::STALE_KEY`): its rank among the stale references met so
    /// far, met first.
    fn stale_key(&self, r: ObjRef) -> usize {
        let mut v = self.stale.borrow_mut();
        match v.iter().position(|&x| x == r) {
            Some(k) => k,
            None => {
                v.push(r);
                v.len() - 1
            }
        }
    }

    /// The stale references met so far, in key order (`rf::STALE_KEY` k is entry k; layout v8's
    /// `lki` tokens, ENGINE.md 236).
    pub(crate) fn stale_refs(&self) -> Vec<ObjRef> {
        self.stale.borrow().clone()
    }

    /// The key of stale reference `r` (`rf::STALE_KEY`), met now if it wasn't (layout v8's `lki` tokens of
    /// the gone objects the public state names that no token of the row wrote: ENGINE.md 236).
    pub(crate) fn stale_key_of(&self, r: ObjRef) -> usize {
        self.stale_key(r)
    }

    /// The identity the viewer sees of card `c` where it is now, if any: the
    /// zone readers' (a card of the list, an exile offer, a pick). A reference
    /// reads [`ref_def`](Self::ref_def).
    pub fn seen_def(&self, c: CardIdx) -> Option<DefId> {
        let g = self.g;
        match g.objs[c.i()].zone {
            Zone::Battlefield | Zone::PhasedOut => Some(g.perm_identity(self.knower, c)),
            _ if g.knows(self.knower, c) => Some(g.objs[c.i()].def),
            _ => None,
        }
    }

    /// The identity the viewer sees of the object reference `r` names
    /// (ENGINE.md 156): every segment that writes a reference's identity or
    /// card code reads it here (`write_ref` does), so the row is a function
    /// of what the viewer knows.
    ///
    /// * **A live reference** (the same object, CR 400.7): as
    ///   [`seen_def`](Self::seen_def) sees the card where it is. A card in
    ///   another player's hand must be one a reference of the public state
    ///   tells apart (`Game::told_apart`, which lists the holders in
    ///   `Game::for_each_public_ref`): its knowledge is then its own, the
    ///   same in every world of the viewer's (a hand's twins are otherwise
    ///   interchangeable: `canon_hand_knowledge` moves their knowledge).
    ///   Refused loudly otherwise: a new holder of references must be listed
    ///   there.
    /// * **A stale reference** (the object is gone): its last known identity
    ///   (`Game::lki_identity`), recorded as it left its zone, where the
    ///   viewer knew it then. Never the card's identity now: that is a new
    ///   object, which may have joined a hand's interchangeable twins or a
    ///   library dealt again in each world.
    /// * **A library card the viewer lost track of** (`Game::ref_seen_live`,
    ///   ENGINE.md 235): as a stale reference - nothing (it has no departure
    ///   recorded), as its departure would record for the viewer.
    pub fn ref_def(&self, r: ObjRef) -> Option<DefId> {
        if !self.live(r) {
            return self.g.lki_identity(self.knower, r);
        }
        self.live_def(r)
    }

    /// The identity the viewer sees of the object at card slot `c` now, named
    /// by where it is (a find by its place, a pick in progress, a counter's or
    /// a face-down card's object), not by a reference the public state holds:
    /// [`ref_def`](Self::ref_def)'s live branch (ENGINE.md 235: a library card
    /// the viewer lost track of is lost only to a reference made before).
    pub fn obj_def(&self, c: CardIdx) -> Option<DefId> {
        self.live_def(self.g.obj_ref(c))
    }

    fn live_def(&self, r: ObjRef) -> Option<DefId> {
        let g = self.g;
        let o = &g.objs[r.idx.i()];
        assert!(
            !(o.zone == Zone::Hand && o.owner != self.knower && g.decider_of(o.owner) != self.knower) || g.told_apart(r.idx),
            "a reference to a card in P{}'s hand that no reference of the public state tells apart (ENGINE.md 156: list its holder in Game::for_each_public_ref)",
            o.owner
        );
        self.seen_def(r.idx)
    }

    /// Does reference `r` name an existing object as the knower sees it
    /// (`Game::ref_seen_live`, ENGINE.md 235)? A library card the knower lost
    /// track of is written as a gone object is.
    pub fn live(&self, r: ObjRef) -> bool {
        self.g.ref_seen_live(self.knower, r)
    }

    /// An object reference (`rf::*`), its identity [`ref_def`](Self::ref_def)'s.
    pub fn write_ref(&self, out: &mut [f32], r: ObjRef) {
        let d = self.ref_def(r);
        self.write_ref_as(out, r, d);
    }

    /// The object at card slot `c` now (`rf::*`), named by where it is - not a
    /// reference the public state holds - with its identity
    /// [`obj_def`](Self::obj_def)'s: written live.
    pub fn write_obj(&self, out: &mut [f32], c: CardIdx) {
        let d = self.obj_def(c);
        self.write_obj_as(out, c, d);
    }

    /// [`write_obj`](Self::write_obj) with identity `d` (a pick in progress
    /// whose card is hidden from the viewer: none).
    pub(crate) fn write_obj_as(&self, out: &mut [f32], c: CardIdx, d: Option<DefId>) {
        self.write_ref_parts(out, self.g.obj_ref(c), d, true);
    }

    /// An object reference (`rf::*`) with identity `d` (`write_ref`).
    fn write_ref_as(&self, out: &mut [f32], r: ObjRef, d: Option<DefId>) {
        self.write_ref_parts(out, r, d, self.live(r));
    }

    /// `rf::*` of the object `r` names: as a live one's (`live`) or a gone one's.
    fn write_ref_parts(&self, out: &mut [f32], r: ObjRef, d: Option<DefId>, live: bool) {
        let g = self.g;
        let c = r.idx;
        let o = &g.objs[c.i()];
        out[rf::PRESENT] = 1.0;
        let side_of = |p: PlayerId| if p == self.me { rf::MINE } else { rf::THEIRS };
        if live {
            out[rf::LIVE] = 1.0;
            out[rf::ZONE + zone_index(o.zone)] = 1.0;
            let who = match o.zone {
                Zone::Battlefield | Zone::Stack | Zone::PhasedOut => o.controller,
                _ => o.owner,
            };
            out[side_of(who)] = 1.0;
            let pos = match o.zone {
                Zone::Battlefield => self.perm_pos(c).map(|x| x.1),
                Zone::Stack => self.spell_pos(c),
                Zone::Hand if o.owner == self.me => self.hand.iter().position(|&x| x == c),
                Zone::Exile => g.exile.iter().position(|&x| x == c),
                Zone::FaceDown => self.fd.iter().position(|&x| x == c),
                Zone::PhasedOut => g.w3.phased.iter().position(|&x| x == c),
                _ => None,
            };
            if let Some(p) = pos {
                out[rf::POS] = (p + 1) as f32 / 32.0;
            }
        } else {
            out[side_of(o.owner)] = 1.0;
            out[rf::STALE_KEY] = (self.stale_key(r) + 1) as f32 / 32.0;
        }
        if let Some(d) = d {
            digits(&mut out[rf::DIGITS..rf::DIGITS + 32], self.ids, d);
        }
    }

    /// A reference to the stack item with id `id` (a target that is an ability, an event's item):
    /// zone Stack and its place, or present only when it left.
    pub fn write_item_ref(&self, out: &mut [f32], id: u32) {
        out[rf::PRESENT] = 1.0;
        if let Some(p) = self.stack_pos(id) {
            out[rf::LIVE] = 1.0;
            out[rf::ZONE + zone_index(Zone::Stack)] = 1.0;
            let ctl = self.g.stack.iter().find(|x| x.id == id).map(|x| x.controller).unwrap_or(self.me);
            out[if ctl == self.me { rf::MINE } else { rf::THEIRS }] = 1.0;
            out[rf::POS] = (p + 1) as f32 / 32.0;
        }
    }

    /// A triggered ability's event (`ev::*`).
    pub fn write_ev(&self, out: &mut [f32], e: &EvCtx) {
        if let Some(p) = e.player {
            out[ev::PLAYER] = 1.0;
            out[if p == self.me { ev::PLAYER_ME } else { ev::PLAYER_OPP }] = 1.0;
        }
        if let Some(r) = e.obj {
            self.write_ref(&mut out[ev::OBJ..ev::OBJ + rf::W], r);
        }
        if let Some(r) = e.obj2 {
            self.write_ref(&mut out[ev::OBJ2..ev::OBJ2 + rf::W], r);
        }
        if e.item != 0 {
            out[ev::ITEM] = 1.0;
            if let Some(p) = self.stack_pos(e.item) {
                out[ev::ITEM_ON_STACK] = 1.0;
                out[ev::ITEM_POS] = (p + 1) as f32 / 32.0;
            }
        }
        out[ev::AMOUNT] = squash(e.amount as f32 / 8.0);
        bits(&mut out[ev::AMOUNT_BITS..ev::AMOUNT_BITS + 64], e.amount);
    }

    /// A waiting trigger's v7 floats after the v6 slot's (`trig::SOURCE`, `trig::EV`).
    pub fn write_trigger_v7(&self, st: &mut [f32], t: &crate::triggers::PendingTrigger) {
        self.write_ref(&mut st[encode::trig::SOURCE..encode::trig::SOURCE + rf::W], t.source);
        self.write_ev(&mut st[encode::trig::EV..encode::trig::EV + ev::W], &t.ev);
    }

    /// The picks in progress (`Picks`): the innermost task's of the agenda holding picks (the
    /// asking task's, or else a resolution's frame's), and each enclosing task's in `outer`,
    /// innermost first (ENGINE.md 142.10, X3: Etali, Primal Conqueror's frame keeps the exiled
    /// cards while a cast from among them pays a cost that picks objects).
    fn picks_in_progress(&self) -> Picks {
        agenda_picks(self.g, self.me)
    }
}

/// The picks in progress of the agenda (`Cx::picks_in_progress`), framed in seat `me` (which
/// player picked each object: `by_me`).
pub(crate) fn agenda_picks(g: &Game, me: PlayerId) -> Picks {
    let mut out = Picks::default();
    if g.pending.is_none() {
        return out;
    }
    let mut holders = 0;
    for t in g.agenda.iter().rev() {
        let (objs, defs, by): (&[CardIdx], &[DefId], Box<dyn Fn(usize) -> PlayerId>) = match t {
            Task::Run(f) => {
                let op = g.db.ability(f.ability).program.get(f.pc as usize);
                let voting = matches!(op, Some(crate::program::Op::Ext(crate::program::Ext::VoteExile { .. })));
                let ctl = f.controller;
                let by: Box<dyn Fn(usize) -> PlayerId> = if voting { Box::new(move |k| if k == 0 { ctl } else { crate::ids::opp(ctl) }) } else { Box::new(move |_| ctl) };
                (&f.picked, &f.picked_defs, by)
            }
            // (Convoke's taps for a spell whose mana can't be spent: ENGINE.md
            // 220.1.)
            Task::PayCosts { id, picked, .. } | Task::ConvokePay { id, picked } => {
                let p = g.stack.iter().find(|s| s.id == *id).map_or(g.pending.as_ref().unwrap().player, |s| s.controller);
                (picked, &[], Box::new(move |_| p))
            }
            Task::OrderTimestamps { picked, .. } => {
                let p = g.turn.active;
                (picked, &[], Box::new(move |_| p))
            }
            Task::Devour { card, picked } => {
                let p = g.objs[card.i()].controller;
                (picked, &[], Box::new(move |_| p))
            }
            _ => continue,
        };
        if objs.is_empty() && defs.is_empty() {
            continue;
        }
        holders += 1;
        // (The innermost holder's picks are the head's floats and their tokens; each enclosing
        // task's are tokens with their depth: ENGINE.md 142.10, X3.)
        let mut p = Picks { objs: objs.iter().enumerate().map(|(k, &c)| (c, by(k) == me)).collect(), ..Picks::default() };
        if !defs.is_empty() {
            p.defs = defs.to_vec();
            p.searcher = Some(by(0));
        }
        if holders == 1 {
            out = p;
        } else {
            out.outer.push(p);
        }
    }
    out
}

/// The tail-only segments and the list's paged picks, written after the head (layout v7 only).
/// `list` the list block's objects by absolute position (the knower's decision's), to tell which
/// picks it holds.
pub(crate) fn write_tail_segments(cx: &Cx, list: &[Option<CardIdx>], tail: &mut TailBuf) {
    let g = cx.g;
    let ids = cx.ids;
    // The other player's waiting triggers (G2), as the frame's own are in `triggers`.
    for (k, t) in g.waiting.iter().filter(|t| t.controller == cx.op_).enumerate() {
        let x = tail.token(seg::OPP_TRIGGERS, k as u32);
        code(&mut x[..CODE], g.db, ids, t.source_def);
        x[CODE] = 1.0;
        x[CODE + 1] = (t.slot as f32 + 1.0) / 4.0;
        cx.write_trigger_v7(&mut x[CODE..], t);
    }
    // Delayed triggers, then temporary ones.
    let mut k = 0u32;
    for d in &g.delayed {
        let x = tail.token(seg::DELAYED, k);
        k += 1;
        code(&mut x[..CODE], g.db, ids, d.source_def);
        let s = &mut x[CODE..];
        s[dl::PRESENT] = 1.0;
        s[dl::KIND_DELAYED] = 1.0;
        use crate::db::DelayedWhen as W;
        let w = match d.when {
            W::NextEndStep => 0,
            W::NextCleanup => 1,
            W::NextUpkeep => 2,
            W::YourNextMainPhase => 3,
            W::YourNextTurnEndStep => 4,
            W::EndOfCombat => 5,
            W::YourNextUpkeep => 6,
        };
        s[dl::WHEN + w] = 1.0;
        s[if d.controller == cx.me { dl::MINE } else { dl::THEIRS }] = 1.0;
        s[dl::SLOT] = (d.slot as f32 + 1.0) / 8.0;
        s[dl::CREATED_THIS_TURN] = (d.turn == g.turn.number) as u8 as f32;
        s[dl::TURNS_SINCE] = squash(g.turn.number.saturating_sub(d.turn) as f32 / 4.0);
        cx.write_ref(&mut s[dl::SOURCE..dl::SOURCE + rf::W], d.source);
        cx.write_ev(&mut s[dl::EV..dl::EV + ev::W], &d.ev);
    }
    for t in &g.temp_triggers {
        let x = tail.token(seg::DELAYED, k);
        k += 1;
        code(&mut x[..CODE], g.db, ids, t.source_def);
        let s = &mut x[CODE..];
        s[dl::PRESENT] = 1.0;
        s[dl::KIND_TEMP] = 1.0;
        s[dl::THIS_TURN] = t.this_turn as u8 as f32;
        s[if t.controller == cx.me { dl::MINE } else { dl::THEIRS }] = 1.0;
        s[dl::SLOT] = (t.slot as f32 + 1.0) / 8.0;
        cx.write_ref(&mut s[dl::SOURCE..dl::SOURCE + rf::W], t.source);
        if let Some(r) = t.obj {
            cx.write_ref(&mut s[dl::EV + ev::OBJ..dl::EV + ev::OBJ + rf::W], r);
        }
    }
    write_links(cx, list, tail);
    write_exile_offered(cx, tail);
    write_library_known(cx, tail);
    write_stack_relations(cx, tail);
    write_graveyard_order(cx, tail);
    write_face_down(cx, tail);
    // ENGINE.md 142.7.
    write_effects(cx, tail);
    write_counters(cx, tail);
    write_numbers(cx, tail);
    write_found(cx, tail);
}

/// The `face_down` segment (`fd::*`): every card exiled face down, in their canonical order for the
/// viewer (`face_down_order`, ENGINE.md 142.3), then every face-down permanent, in their order
/// (slots k). An identity only where the viewer knows it.
fn write_face_down(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    let cards = cx.fd.iter().map(|&c| (c, true)).chain(g.face_down_ids.iter().map(|&(c, _)| (c, false)));
    for (k, (c, exiled)) in cards.enumerate() {
        let x = tail.token(seg::FACE_DOWN, k as u32);
        let o = &g.objs[c.i()];
        let viewer_knows = g.knows(cx.knower, c);
        if viewer_knows {
            let def = if exiled { o.def } else { g.perm_identity(cx.knower, c) };
            code(&mut x[..CODE], g.db, cx.ids, def);
        }
        let s = &mut x[CODE..];
        s[fd::PRESENT] = 1.0;
        s[if exiled { fd::EXILED } else { fd::PERMANENT }] = 1.0;
        cx.write_obj(&mut s[fd::OBJ..fd::OBJ + rf::W], c);
        s[if o.owner == cx.me { fd::OWNER_MINE } else { fd::OWNER_THEIRS }] = 1.0;
        s[fd::OWNER_KNOWS] = (o.known >> o.owner & 1) as f32;
        s[fd::OTHER_KNOWS] = (o.known >> crate::ids::opp(o.owner) & 1) as f32;
        s[fd::VIEWER_KNOWS] = viewer_knows as u8 as f32;
        if exiled {
            s[fd::PLAY_ME] = g.exile_permission(cx.me, c).is_some() as u8 as f32;
            s[fd::PLAY_OPP] = g.exile_permission(cx.op_, c).is_some() as u8 as f32;
        }
    }
}

/// A stable id of an interned name (a subtype or a named counter, `CardDb::subtype_name`): its
/// FNV-1a 32-bit hash (ENGINE.md 142.7). The interned ids are each table's own; the network needs
/// one id a name in every table. `tests/v7_tails.rs::name_ids_are_distinct` checks that no two names
/// of the tables share one.
pub fn name_id(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// A number's floats (`ef::NUM_W`): squashed / 8, negative, its magnitude's 64 binary digits.
pub(crate) fn num(out: &mut [f32], v: i64) {
    out[0] = squash(v as f32 / 8.0);
    out[1] = (v < 0) as u8 as f32;
    bits(&mut out[2..ef::NUM_W], v.unsigned_abs());
}

/// The bits of a card-type mask (`types::*`, 9 bits).
fn type_mask(out: &mut [f32], t: u16) {
    for (k, x) in out[..ef::TYPE_N].iter_mut().enumerate() {
        *x = (t >> k & 1) as f32;
    }
}

/// A keyword's index among `encode::KWS` then `encode::KWS_V7` (`ef::KWS`, `ct::KW`).
fn kw_index(k: crate::db::Kw) -> usize {
    let f = encode::kw_float(k);
    if f >= encode::perm::KW_V7 {
        encode::KWS.len() + f - encode::perm::KW_V7
    } else {
        f - encode::perm::KEYWORDS
    }
}

/// The index of an effect's `Mod` in `ef::MOD`: an exhaustive match. The four variants only
/// static abilities have never occur in an effect (`Mod::static_only`: `Game::add_effect` refuses
/// them).
pub fn mod_index(m: &crate::db::Mod) -> usize {
    use crate::db::Mod as M;
    match m {
        M::Types { .. } => 0,
        M::Colors(_) => 1,
        M::Keywords(_) => 2,
        M::Protection(_) => 3,
        M::SetPt(..) => 4,
        M::Pt(..) => 5,
        M::CantBlock => 6,
        M::CantBeBlocked => 7,
        M::MustAttack => 8,
        M::ExileIfDies => 9,
        M::GainSacMana(_) => 10,
        M::LoseAllAbilities => 11,
        M::AllCreatureTypes => 12,
        M::BecomeTypes { .. } => 13,
        M::GainLatent(_) => 14,
        M::SwitchPt => 15,
        M::Goad => 16,
        M::ExileIfLeaves => 17,
        M::ControlFrom(_) => 18,
        M::PreventCombatDamage => 19,
        M::MustAttackPlayer => 20,
        M::PtPer { .. } | M::PtPerSourceCounter { .. } | M::PtPerGraveyardCards { .. } | M::AttackDespiteDefender | M::PowerPlusGreatestMv { .. } => {
            unreachable!("a static ability's mod {m:?} in an effect (Game::add_effect refuses it)")
        }
    }
}

/// The filter of a group's rules effect as the `effects` token writes it: its card types, and
/// whose (0 any player's, 1 the effect's controller's, 2 its controller's opponents'). The lowering
/// makes `Type(t)` and `Type(t)` with `Controller(b)` (`Ext::ApplyRules`, ENGINE.md 142.7); any other
/// shape fails loudly here until the token has floats for it (`tests/v7_tails.rs` checks every
/// table's group effects).
pub fn group_filter(f: &crate::db::Filter) -> (u16, usize) {
    use crate::db::Filter as F;
    let shape = match f {
        F::Type(t) => Some((*t, 0)),
        F::And(a, b) => match (&**a, &**b) {
            (F::Type(t), F::Controller(y)) | (F::Controller(y), F::Type(t)) => Some((*t, if *y { 1 } else { 2 })),
            _ => None,
        },
        _ => None,
    };
    shape.unwrap_or_else(|| panic!("a group effect's filter {f:?} has no floats in the effects token (ENGINE.md 142.7)"))
}

/// Subtypes (interned ids) into `ef::SUBTYPES_N` / `ef::SUBTYPES`: the count / 4 and each one's
/// stable name id (`name_id`), at most `ef::SUBS` (a loud failure past it).
fn subtypes(g: &Game, s: &mut [f32], ids: &[u16]) {
    assert!(ids.len() <= ef::SUBS, "an effect with {} subtypes: the effects token holds {} (ENGINE.md 142.7)", ids.len(), ef::SUBS);
    s[ef::SUBTYPES_N] = ids.len() as f32 / 4.0;
    for (k, &t) in ids.iter().enumerate() {
        bits(&mut s[ef::SUBTYPES + 32 * k..ef::SUBTYPES + 32 * (k + 1)], name_id(g.db.subtype_name(t)) as u64);
    }
}

/// A duration into `ef::UNTIL` / `ef::UNTIL_OBJ`.
fn until(cx: &Cx, s: &mut [f32], u: crate::layers::Until) {
    use crate::layers::Until as U;
    let k = match u {
        U::EndOfTurn => 0,
        U::Forever => 1,
        U::YourNextTurn => 2,
        U::WhileOnBattlefield(r) => {
            cx.write_ref(&mut s[ef::UNTIL_OBJ..ef::UNTIL_OBJ + rf::W], r);
            3
        }
        U::WhileSource => unreachable!("a lowered duration in an effect (Game::until_for)"),
    };
    s[ef::UNTIL + k] = 1.0;
}

/// The `effects` segment (`ef::*`; ENGINE.md 142.7).
fn write_effects(cx: &Cx, tail: &mut TailBuf) {
    use crate::db::Mod as M;
    use crate::layers::Affected;
    let g = cx.g;
    let mut k = 0u32;
    for e in &g.effects {
        let x = tail.token(seg::EFFECTS, k);
        k += 1;
        match &e.on {
            Affected::Obj(r) => {
                if let Some(d) = cx.ref_def(*r) {
                    code(&mut x[..CODE], g.db, cx.ids, d);
                }
                cx.write_ref(&mut x[CODE + ef::OBJ..CODE + ef::OBJ + rf::W], *r);
            }
            Affected::Group(f) => {
                let (t, whose) = group_filter(f);
                x[CODE + ef::GROUP] = 1.0;
                type_mask(&mut x[CODE + ef::GROUP_TYPES..], t);
                x[CODE + ef::GROUP_ANY + whose] = 1.0;
            }
        }
        let s = &mut x[CODE..];
        s[ef::PRESENT] = 1.0;
        s[ef::MOD + mod_index(&e.m)] = 1.0;
        match &e.m {
            M::Types { add, subtypes: st, replace } => {
                type_mask(&mut s[ef::TYPES..], *add);
                s[ef::REPLACE] = *replace as u8 as f32;
                subtypes(g, s, st);
            }
            M::BecomeTypes { types, subtypes: st } => {
                type_mask(&mut s[ef::TYPES..], *types);
                subtypes(g, s, st);
            }
            M::Colors(m) => {
                bits(&mut s[ef::COLORS..ef::COLORS + 6], *m as u64);
                s[ef::COLORS_SET] = 1.0;
            }
            M::Keywords(set) => {
                for kw in encode::KWS.iter().chain(encode::KWS_V7.iter()) {
                    s[ef::KWS + kw_index(*kw)] = set.has(*kw) as u8 as f32;
                }
            }
            M::Protection(p) => {
                let q = &mut s[ef::PROT..ef::PROT + ef::PROT_N];
                bits(&mut q[..5], p.colors as u64);
                q[5] = p.colorless as u8 as f32;
                q[6] = p.everything as u8 as f32;
                type_mask(&mut q[7..], p.types);
                q[16] = p.opponents as u8 as f32;
                q[17] = p.chosen_player as u8 as f32;
                q[18] = p.colored_spells as u8 as f32;
            }
            M::SetPt(pw, th) | M::Pt(pw, th) => {
                num(&mut s[ef::POWER..ef::POWER + ef::NUM_W], *pw);
                num(&mut s[ef::TOUGHNESS..ef::TOUGHNESS + ef::NUM_W], *th);
                s[ef::PT_PRESENT] = 1.0;
            }
            M::GainSacMana(m) => bits(&mut s[ef::SAC_MASK..ef::SAC_MASK + 6], *m as u64),
            M::GainLatent(l) => {
                assert!((*l as usize) < ef::LATENT_N, "latent ability {l} past the effects token's {} (ENGINE.md 142.7)", ef::LATENT_N);
                s[ef::LATENT + *l as usize] = 1.0;
            }
            M::ControlFrom(p) => s[if *p == cx.me { ef::CONTROL_FROM_ME } else { ef::CONTROL_FROM_OPP }] = 1.0,
            M::CantBlock
            | M::CantBeBlocked
            | M::MustAttack
            | M::ExileIfDies
            | M::LoseAllAbilities
            | M::AllCreatureTypes
            | M::SwitchPt
            | M::Goad
            | M::ExileIfLeaves
            | M::PreventCombatDamage
            | M::MustAttackPlayer => {}
            M::PtPer { .. } | M::PtPerSourceCounter { .. } | M::PtPerGraveyardCards { .. } | M::AttackDespiteDefender | M::PowerPlusGreatestMv { .. } => {
                unreachable!("a static ability's mod in an effect")
            }
        }
        until(cx, s, e.until);
        s[if e.controller == cx.me { ef::CONTROLLER_ME } else { ef::CONTROLLER_OPP }] = 1.0;
    }
    // Copy effects (CR 707, layer 1), after the effects.
    for c in &g.copies {
        let x = tail.token(seg::EFFECTS, k);
        k += 1;
        if let Some(d) = cx.ref_def(c.obj) {
            code(&mut x[..CODE], g.db, cx.ids, d);
        }
        cx.write_ref(&mut x[CODE + ef::OBJ..CODE + ef::OBJ + rf::W], c.obj);
        let s = &mut x[CODE..];
        s[ef::PRESENT] = 1.0;
        copy_vals(g, cx.ids, s, &c.vals);
        until(cx, s, c.until);
    }
}

/// Copiable values (CR 707.2) in an `effects` token's copy-effect fields (`ef::COPY` ...
/// `ef::COPY_DFC`): a copy effect's, and a gone object's last known ones (layout v8's `lki`
/// tokens, ENGINE.md 236).
pub(crate) fn copy_vals(g: &Game, ids: &[u16], s: &mut [f32], vals: &crate::copies::CopyVals) {
    s[ef::COPY] = 1.0;
    digits(&mut s[ef::COPY_DEF..ef::COPY_DEF + 32], ids, vals.def);
    s[ef::COPY_DFC] = vals.dfc as u8 as f32;
    let e = &vals.exc;
    type_mask(&mut s[ef::TYPES..], e.types);
    let st: Vec<u16> = (0..64u8).filter(|&b| e.subtypes >> b & 1 == 1).map(|b| g.db.exc_subtype_of_bit(b)).collect();
    subtypes(g, s, &st);
    for kw in encode::KWS.iter().chain(encode::KWS_V7.iter()) {
        s[ef::KWS + kw_index(*kw)] = e.kws.has(*kw) as u8 as f32;
    }
    if let Some((pw, th)) = e.pt {
        num(&mut s[ef::POWER..ef::POWER + ef::NUM_W], pw as i64);
        num(&mut s[ef::TOUGHNESS..ef::TOUGHNESS + ef::NUM_W], th as i64);
        s[ef::PT_PRESENT] = 1.0;
    }
    if let Some(n) = e.name {
        s[ef::EXC_NAME] = 1.0;
        digits(&mut s[ef::EXC_NAME_DIGITS..ef::EXC_NAME_DIGITS + 32], ids, n);
    }
    if let Some(m) = e.colors {
        bits(&mut s[ef::COLORS..ef::COLORS + 6], m as u64);
        s[ef::COLORS_SET] = 1.0;
    }
    s[ef::EXC_NO_COST] = e.no_cost as u8 as f32;
    bits(&mut s[ef::EXC_SUPERS..ef::EXC_SUPERS + 8], e.supers as u64);
}

/// The `counters` segment (`ct::*`; ENGINE.md 142.7).
fn write_counters(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    if g.counters.is_empty() {
        return;
    }
    let mut k = 0u32;
    for &c in cx.mine.iter().chain(&cx.theirs).chain(&g.exile).chain(&g.w3.phased) {
        for (kind, n) in g.counters_of(c) {
            let x = tail.token(seg::COUNTERS, k);
            k += 1;
            if let Some(d) = cx.obj_def(c) {
                code(&mut x[..CODE], g.db, cx.ids, d);
            }
            let s = &mut x[CODE..];
            s[ct::PRESENT] = 1.0;
            cx.write_obj(&mut s[ct::OBJ..ct::OBJ + rf::W], c);
            counter_floats(g, s, kind, n);
        }
    }
}

/// A kind of counter and its number in a `counters` token's fields (`ct::KIND` ... `ct::COUNT_BITS`):
/// an object's, and a gone object's last known ones (layout v8's `lki` tokens, ENGINE.md 236).
pub(crate) fn counter_floats(g: &Game, s: &mut [f32], kind: crate::db::Counter, n: crate::num::Count) {
    use crate::db::Counter as K;
    let i = match kind {
        K::P1P1 => 0,
        K::M1M1 => 1,
        K::Stun => 2,
        K::Kw(kw) => {
            s[ct::KW + kw_index(kw)] = 1.0;
            3
        }
        K::Named(id) => {
            bits(&mut s[ct::NAME..ct::NAME + 32], name_id(g.db.subtype_name(id)) as u64);
            4
        }
        K::Loyalty => 5,
        K::Lore => 6,
    };
    s[ct::KIND + i] = 1.0;
    s[ct::COUNT] = squash(n as f32 / 4.0);
    bits(&mut s[ct::COUNT_BITS..ct::COUNT_BITS + 64], n as u64);
}

/// One `numbers` token (`nb::*`) if `v` is past `nb::BIG`: a player's (`p`) or a permanent's (`c`).
fn number(cx: &Cx, tail: &mut TailBuf, k: &mut u32, p: Option<PlayerId>, c: Option<CardIdx>, which: usize, v: i128) {
    if v.unsigned_abs() < nb::BIG {
        return;
    }
    let g = cx.g;
    let x = tail.token(seg::NUMBERS, *k);
    *k += 1;
    if let Some(c) = c {
        if let Some(d) = cx.obj_def(c) {
            code(&mut x[..CODE], g.db, cx.ids, d);
        }
    }
    let s = &mut x[CODE..];
    s[nb::PRESENT] = 1.0;
    if let Some(p) = p {
        s[if p == cx.me { nb::PLAYER_ME } else { nb::PLAYER_OPP }] = 1.0;
    }
    if let Some(c) = c {
        cx.write_obj(&mut s[nb::OBJ..nb::OBJ + rf::W], c);
    }
    s[nb::WHICH + which] = 1.0;
    s[nb::NEG] = (v < 0) as u8 as f32;
    let m = v.unsigned_abs();
    bits(&mut s[nb::BITS..nb::BITS + 64], m as u64);
    bits(&mut s[nb::BITS + 64..nb::BITS + 128], (m >> 64) as u64);
}

/// The `numbers` segment (`nb::*`; ENGINE.md 142.7).
fn write_numbers(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    let mut k = 0u32;
    for p in [cx.me, cx.op_] {
        let pl = &g.players[p as usize];
        number(cx, tail, &mut k, Some(p), None, nb::LIFE, pl.life as i128);
        number(cx, tail, &mut k, Some(p), None, nb::POISON, pl.poison as i128);
        number(cx, tail, &mut k, Some(p), None, nb::ENERGY, pl.energy as i128);
        number(cx, tail, &mut k, Some(p), None, nb::EXPERIENCE, pl.experience as i128);
    }
    for &c in cx.mine.iter().chain(&cx.theirs) {
        let (pw, th) = g.pt(c);
        number(cx, tail, &mut k, None, Some(c), nb::POWER, pw);
        number(cx, tail, &mut k, None, Some(c), nb::TOUGHNESS, th);
        number(cx, tail, &mut k, None, Some(c), nb::DAMAGE, g.objs[c.i()].damage as i128);
    }
}

/// The identities player `me` revealed in earlier games of match `m` (`my_revealed`, `mr::*`): one
/// token each, its slot the identity (ENGINE.md 125.7). The number written.
pub(crate) fn write_my_revealed(m: &super::matches::MatchState, me: PlayerId, tail: &mut TailBuf) -> usize {
    write_revealed(m, me, seg::MY_REVEALED, tail) + write_revealed(m, crate::ids::opp(me), seg::OPP_REVEALED, tail)
}

/// Player `p`'s earlier reveals as tokens of segment `sg` (`mr::*`): one an identity, its slot the
/// identity, with the games it was revealed in (ENGINE.md 142.8). The number written.
fn write_revealed(m: &super::matches::MatchState, p: PlayerId, sg: u16, tail: &mut TailBuf) -> usize {
    let mut n = 0;
    for (w, &bits) in m.earlier[p as usize].iter().enumerate() {
        let mut b = bits;
        while b != 0 {
            let i = w * 64 + b.trailing_zeros() as usize;
            let x = tail.token(sg, i as u32);
            x[(i >> 6) & 15] = 1.0;
            x[16 + ((i >> 3) & 7)] = 1.0;
            x[24 + (i & 7)] = 1.0;
            x[CODE + mr::PRESENT] = 1.0;
            for g in 0..3 {
                x[CODE + mr::GAME + g] = (m.earlier_by_game[p as usize][g][w] >> (i & 63) & 1) as f32;
            }
            n += 1;
            b &= b - 1;
        }
    }
    n
}

/// How many `my_revealed` and `opp_revealed` tokens the frame player `me`'s row of match `m` has:
/// both players' earlier reveals, its own and the other's (R1: a head-only encoding of a row with any
/// refuses).
pub(crate) fn my_revealed_count(m: &super::matches::MatchState, me: PlayerId) -> usize {
    let n = |p: PlayerId| m.earlier[p as usize].iter().map(|w| w.count_ones() as usize).sum::<usize>();
    n(me) + n(crate::ids::opp(me))
}

/// A new `links` token at the next slot `k`, of kind `kind`.
fn link_token<'a>(tail: &'a mut TailBuf, k: &mut u32, kind: usize) -> &'a mut [f32] {
    let x = tail.token(seg::LINKS, *k);
    *k += 1;
    x[CODE + lk::PRESENT] = 1.0;
    x[CODE + lk::KIND + kind] = 1.0;
    x
}

/// The `links` segment (`lk::*`).
fn write_links(cx: &Cx, list: &[Option<CardIdx>], tail: &mut TailBuf) {
    let g = cx.g;
    let ids = cx.ids;
    let mut k = 0u32;
    let side = |p: PlayerId| if p == cx.me { 0 } else { 1 };
    // Cards exiled with an object (CR 610.3): those still exiled; those exiled face up in link
    // order, then the face-down ones in their canonical order (ENGINE.md 142.3: a face-down
    // pile's link order is the order its cards were exiled in, a hand's order for Memory Jar's).
    let mut exiled: Vec<&crate::game::Link> =
        g.links.iter().filter(|l| g.live(l.card) && matches!(g.objs[l.card.idx.i()].zone, Zone::Exile | Zone::FaceDown)).collect();
    exiled.sort_by_key(|l| match g.objs[l.card.idx.i()].zone {
        Zone::FaceDown => 1 + cx.fd.iter().position(|&x| x == l.card.idx).expect("a face-down card in Game::face_down"),
        _ => 0,
    });
    for l in exiled {
        let x = link_token(tail, &mut k, lk::K_EXILED_WITH);
        if let Some(d) = cx.ref_def(l.card) {
            code(&mut x[..CODE], g.db, ids, d);
        }
        let st = &mut x[CODE..];
        cx.write_ref(&mut st[lk::A..lk::A + rf::W], l.source);
        cx.write_ref(&mut st[lk::B..lk::B + rf::W], l.card);
        st[lk::UNTIL] = l.until as u8 as f32;
        st[lk::BACK + zone_index(l.back)] = 1.0;
        if let Some(p) = l.until_monarch {
            st[lk::MONARCH] = 1.0;
            st[lk::MONARCH_ME + side(p)] = 1.0;
        }
    }
    // The marks that relate an object to another object or an ability, any number per object.
    for &(o, m) in &g.marks {
        if !cx.live(o) {
            continue;
        }
        let (kind, def, slot, b): (usize, Option<DefId>, Option<u8>, Option<ObjRef>) = match m {
            Mark::Granted { def, slot, .. } => (lk::K_GRANTED, Some(def), Some(slot), None),
            Mark::LeaveTrigger { def, slot, .. } => (lk::K_LEAVE_TRIGGER, Some(def), Some(slot), None),
            Mark::GrantOnEnter { def, slot } => (lk::K_GRANT_ON_ENTER, Some(def), Some(slot), None),
            Mark::Triggered { def, slot, src, .. } => (lk::K_TRIGGERED, Some(def), Some(slot), Some(src)),
            Mark::Reanimated(x) => (lk::K_REANIMATED, None, None, Some(x)),
            Mark::ExiledToPay(x) => (lk::K_EXILED_TO_PAY, None, None, Some(x)),
            Mark::DelvePromised => (lk::K_DELVE_PROMISED, None, None, Some(o)),
            _ => continue,
        };
        let x = link_token(tail, &mut k, kind);
        match (def, b) {
            (Some(d), _) => code(&mut x[..CODE], g.db, ids, d),
            (None, Some(r)) => {
                if let Some(d) = cx.ref_def(r) {
                    code(&mut x[..CODE], g.db, ids, d);
                }
            }
            (None, None) => {}
        }
        let st = &mut x[CODE..];
        cx.write_ref(&mut st[lk::A..lk::A + rf::W], o);
        if let Some(r) = b {
            cx.write_ref(&mut st[lk::B..lk::B + rf::W], r);
        }
        if let Some(sl) = slot {
            st[lk::SLOT] = (sl as f32 + 1.0) / 8.0;
        }
        match m {
            Mark::Granted { this_turn, ts, .. } => {
                st[lk::THIS_TURN] = this_turn as u8 as f32;
                // (Removed by a later "loses all abilities": CR 613.7, `granted_observers`.)
                st[lk::ACTIVE] = g.loss_ts(o.idx).is_none_or(|l| ts > l) as u8 as f32;
            }
            Mark::LeaveTrigger { exile, .. } => st[lk::EXILE] = exile as u8 as f32,
            Mark::Triggered { turn, n, .. } => {
                st[lk::N] = n as f32 / 2.0;
                st[lk::CURRENT_TURN] = (turn == g.turn.number as u16) as u8 as f32;
            }
            _ => {}
        }
    }
    // Picks in progress the other blocks don't hold: not a permanent, not in the list block, not
    // in the frame player's hand (ENGINE.md 125, 156).
    // * The other player's pick of a card of its own hand: a token, with no identity, whatever the
    //   knower knows of that hand. That player names a hand slot, whose card depends on the hand's
    //   hidden cards (ENGINE.md 11, 128.2's fifth way: a cost's picks move, keyed, when paid), and
    //   Show and Tell's pick stays face down (CR 101.4a, its ruling: it is clear which card was
    //   chosen, not what it is; 101.4b; the knower's worlds draw it again, `hidden_pick_ok`).
    //   That a card was picked, and its rank, is public.
    // * Any other pick (the knower's own - its choices are seen -, a card in a public zone, a
    //   library card by its place, a face-down card by its object): its identity where the
    //   knower knows it; a card hidden from the knower: no token (the same in every world: that
    //   knowledge is the card's own).
    // (An enclosing task's picks, ENGINE.md 142.10: every one a token, `lk::DEPTH` its depth - the
    // head's floats are the innermost holder's.)
    let levels = std::iter::once(&cx.picks).chain(cx.picks.outer.iter()).enumerate();
    for (depth, picks) in levels {
        for (rank, &(c, by_me)) in picks.objs.iter().enumerate() {
            let zone = g.objs[c.i()].zone;
            let in_list = list.contains(&Some(c));
            if depth == 0 && (zone == Zone::Battlefield || in_list || (zone == Zone::Hand && g.objs[c.i()].owner == cx.me)) {
                continue;
            }
            let others_hand_pick = zone == Zone::Hand && !by_me && cx.knower == cx.me;
            let d = if others_hand_pick {
                None
            } else if g.hidden_obj(c) && !g.knows(cx.knower, c) {
                continue;
            } else {
                cx.seen_def(c)
            };
            let x = link_token(tail, &mut k, lk::K_PICKED);
            if let Some(d) = d {
                code(&mut x[..CODE], g.db, ids, d);
            }
            let st = &mut x[CODE..];
            cx.write_obj_as(&mut st[lk::B..lk::B + rf::W], c, d);
            st[lk::RANK] = (rank + 1) as f32 / 8.0;
            st[if by_me { lk::BY_ME } else { lk::BY_OPP }] = 1.0;
            st[lk::DEPTH] = depth as f32 / 8.0;
        }
        if picks.searcher == Some(cx.knower) {
            for (rank, &d) in picks.defs.iter().enumerate() {
                let x = link_token(tail, &mut k, lk::K_PICKED_DEF);
                code(&mut x[..CODE], g.db, ids, d);
                x[CODE + lk::RANK] = (rank + 1) as f32 / 8.0;
                x[CODE + lk::BY_ME] = 1.0;
                x[CODE + lk::DEPTH] = depth as f32 / 8.0;
            }
        }
    }
}

/// The `found` segment (`fs::*`, ENGINE.md 142.12): the finds everyone tells apart
/// (`Game::finds_told`) by place, slot k the k-th (`Game::finds_by_place`, ENGINE.md 223: which
/// search found which is not public after a shuffle or a private rearrangement among them, the set
/// of their places is). (A find is a card in a library or exiled face down - it leaves them as it
/// goes into a hand -, so its reference never names a hand card: no holder for
/// `Game::for_each_public_ref`.)
fn write_found(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    if g.finds_told.is_empty() {
        return;
    }
    for (k, c) in g.finds_by_place().into_iter().enumerate() {
        let x = tail.token(seg::FOUND, k as u32);
        // (By its place: the object there now, ENGINE.md 235.)
        if let Some(d) = cx.obj_def(c) {
            code(&mut x[..CODE], g.db, cx.ids, d);
        }
        let s = &mut x[CODE..];
        s[fs::PRESENT] = 1.0;
        cx.write_obj(&mut s[fs::OBJ..fs::OBJ + rf::W], c);
        let o = &g.objs[c.i()];
        if o.zone == Zone::Library {
            let lib = &g.players[o.owner as usize].library;
            let depth = lib.iter().rev().position(|&x| x == c).expect("a library card in its owner's library");
            s[fs::DEPTH] = (depth + 1) as f32 / 64.0;
            bits(&mut s[fs::DEPTH_BITS..fs::DEPTH_BITS + 16], depth as u64);
        }
    }
}

/// The `exile_offered` segment (`xo::*`): the `EXILE` range's items of the knower's decision.
fn write_exile_offered(cx: &Cx, tail: &mut TailBuf) {
    let Some(d) = cx.d else { return };
    let g = cx.g;
    for o in d.options.iter() {
        let Some(OptObject::Exile(pos)) = d.option_object(o.action) else { continue };
        let card = match o.choice {
            Choice::Cast(c) | Choice::Play(c) | Choice::CastFace(c) | Choice::PlayFace(c) | Choice::Faces(c, _) | Choice::Card(c) => c,
            Choice::Target(Target::Obj(r)) => r.idx,
            other => panic!("{:?}: an EXILE option {} names no card ({other:?})", d.kind, o.action),
        };
        let x = tail.token(seg::EXILE_OFFERED, pos);
        if let Some(def) = cx.seen_def(card) {
            code(&mut x[..CODE], g.db, cx.ids, def);
        }
        let s = &mut x[CODE..];
        let ob = &g.objs[card.i()];
        s[xo::PRESENT] = 1.0;
        s[xo::FACE_DOWN] = (ob.zone == Zone::FaceDown) as u8 as f32;
        s[if ob.owner == cx.me { xo::MINE } else { xo::THEIRS }] = 1.0;
        let k = match o.choice {
            Choice::Cast(_) => 0,
            Choice::Play(_) => 1,
            Choice::CastFace(_) => 2,
            Choice::PlayFace(_) => 3,
            Choice::Faces(_, m) => {
                for b in 0..4 {
                    s[xo::FACES + b] = (m >> b & 1) as f32;
                }
                4
            }
            _ => 5,
        };
        s[xo::CHOICE + k] = 1.0;
        if let Some(pm) = g.permissions.iter().find(|pm| pm.card == g.obj_ref(card) && pm.player == cx.me && g.exile_permission(cx.me, card) == Some(pm.kind)) {
            use crate::game::PermKind as P;
            s[xo::PERM] = 1.0;
            let (kind, tax) = match pm.kind {
                P::Play { free, cast_only, any_mana, .. } => {
                    s[xo::FREE] = free as u8 as f32;
                    s[xo::CAST_ONLY] = cast_only as u8 as f32;
                    s[xo::ANY_MANA] = any_mana as u8 as f32;
                    (0, 0)
                }
                P::Flashback => (1, 0),
                P::Taxed(n) => (2, n),
                P::Adventure => (3, 0),
                P::CastThenExile => (4, 0),
                P::Prepared { .. } => (5, 0),
                P::Airbent(n) => (6, n),
                P::TaxedLandsTapped(n) => (7, n),
            };
            s[xo::PERM_KIND + kind] = 1.0;
            s[xo::TAX] = tax as f32 / 4.0;
            s[xo::THIS_TURN] = pm.turn.is_some() as u8 as f32;
        }
        let place = match ob.zone {
            Zone::Exile => g.exile.iter().position(|&x| x == card),
            Zone::FaceDown => cx.fd.iter().position(|&x| x == card),
            _ => None,
        };
        if let Some(p) = place {
            s[xo::POS] = (p + 1) as f32 / 32.0;
        }
    }
}

/// The `library_known` segment (`lb::*`): each library card below the top three that someone
/// knows, the frame player's library first, top down.
fn write_library_known(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    let mut k = 0u32;
    for (side, p) in [cx.me, cx.op_].into_iter().enumerate() {
        let lib = &g.players[p as usize].library;
        for depth in encode::TOPS..lib.len() {
            let c = lib[lib.len() - 1 - depth];
            let known = g.objs[c.i()].known;
            if known == 0 {
                continue;
            }
            let x = tail.token(seg::LIBRARY_KNOWN, k);
            k += 1;
            let viewer_knows = g.knows(cx.knower, c);
            if viewer_knows {
                code(&mut x[..CODE], g.db, cx.ids, g.objs[c.i()].def);
            }
            let s = &mut x[CODE..];
            s[lb::PRESENT] = 1.0;
            s[if side == 0 { lb::MINE } else { lb::THEIRS }] = 1.0;
            s[lb::DEPTH] = (depth + 1) as f32 / 64.0;
            assert!(depth < 1 << 16, "a library card at depth {depth}");
            bits(&mut s[lb::DEPTH_BITS..lb::DEPTH_BITS + 16], depth as u64);
            s[lb::OWNER_KNOWS] = (known >> p & 1) as f32;
            s[lb::OTHER_KNOWS] = (known >> crate::ids::opp(p) & 1) as f32;
            s[lb::VIEWER_KNOWS] = viewer_knows as u8 as f32;
        }
    }
}

/// Is target `t` (of index < 2) named exactly by the stack slot's target block? A player, a live
/// permanent, a live spell, an ability still on the stack.
pub(crate) fn head_target(g: &Game, t: Target) -> bool {
    match t {
        Target::Player(_) => true,
        Target::Obj(r) => g.live(r) && matches!(g.objs[r.idx.i()].zone, Zone::Battlefield | Zone::Stack),
        Target::Stack(id) => g.stack.iter().any(|x| x.id == id),
    }
}

/// A new relations token at the next slot `k` of segment `sg` (`stack_relations`; layout v8's
/// `lki_relations`): kind `kind` of the item at `pos` from the top (a gone spell: none), index
/// `index` in its list; `key` (layout v8): where the gone spell's stale key goes, and the key.
fn sr_token<'a>(tail: &'a mut TailBuf, sg: u16, k: &mut u32, kind: usize, pos: Option<usize>, index: usize, key: Option<(usize, usize)>) -> &'a mut [f32] {
    let x = tail.token(sg, *k);
    *k += 1;
    x[CODE + sr::PRESENT] = 1.0;
    x[CODE + sr::KIND + kind] = 1.0;
    if let Some(pos) = pos {
        x[CODE + sr::ITEM_POS] = (pos + 1) as f32 / 32.0;
    }
    x[CODE + sr::INDEX] = (index + 1) as f32 / 8.0;
    if let Some((at, key)) = key {
        x[at] = (key + 1) as f32 / 32.0;
        bits(&mut x[at + 1..at + 17], key as u64);
    }
    x
}

/// The `stack_relations` segment (`sr::*`), the stack from the top.
fn write_stack_relations(cx: &Cx, tail: &mut TailBuf) {
    let mut k = 0u32;
    for (pos, item) in cx.g.stack.iter().rev().enumerate() {
        item_relations(cx, item, Some(pos), tail, seg::STACK_RELATIONS, &mut k, None);
    }
}

/// An item's relations as tokens of segment `sg` from slot `k` on (`sr::*`): an item on the stack
/// at `pos` (`stack_relations`: what its stack slot doesn't hold - targets past the first two or
/// that the head's target block can't name, divisions past two, noted objects past the first and
/// a first one off the battlefield, the spliced cards, a ninjutsu creature's planeswalker), or a
/// gone spell's (`pos` None, layout v8's `lki_relations`, ENGINE.md 236: every one, with `key`).
pub(crate) fn item_relations(cx: &Cx, item: &crate::game::StackItem, pos: Option<usize>, tail: &mut TailBuf, sg: u16, k: &mut u32, key: Option<(usize, usize)>) {
    let g = cx.g;
    let all = pos.is_none();
    for (j, &(req, t)) in item.targets.iter().enumerate() {
        if !all && j < 2 && head_target(g, t) {
            continue;
        }
        let x = sr_token(tail, sg, k, sr::K_TARGET, pos, j, key);
        x[CODE + sr::REQ] = (req as f32 + 1.0) / 8.0;
        match t {
            Target::Player(p) => {
                x[CODE + sr::PLAYER] = 1.0;
                x[CODE + if p == cx.me { sr::PLAYER_ME } else { sr::PLAYER_OPP }] = 1.0;
            }
            Target::Obj(r) => {
                if let Some(d) = cx.ref_def(r) {
                    code(&mut x[..CODE], g.db, cx.ids, d);
                }
                cx.write_ref(&mut x[CODE + sr::OBJ..CODE + sr::OBJ + rf::W], r);
            }
            Target::Stack(id) => cx.write_item_ref(&mut x[CODE + sr::OBJ..CODE + sr::OBJ + rf::W], id),
        }
    }
    for (j, &n) in item.division.iter().enumerate().skip(if all { 0 } else { 2 }) {
        let x = sr_token(tail, sg, k, sr::K_DIVISION, pos, j, key);
        x[CODE + sr::AMOUNT] = squash(n as f32 / 8.0);
        bits(&mut x[CODE + sr::AMOUNT_BITS..CODE + sr::AMOUNT_BITS + 64], n);
    }
    for (j, &r) in item.noted.iter().enumerate() {
        if !all && j == 0 && g.live(r) && g.objs[r.idx.i()].zone == Zone::Battlefield {
            continue;
        }
        let x = sr_token(tail, sg, k, sr::K_NOTED, pos, j, key);
        if let Some(d) = cx.ref_def(r) {
            code(&mut x[..CODE], g.db, cx.ids, d);
        }
        cx.write_ref(&mut x[CODE + sr::OBJ..CODE + sr::OBJ + rf::W], r);
    }
    for (j, &d) in item.spliced.iter().enumerate() {
        let x = sr_token(tail, sg, k, sr::K_SPLICED, pos, j, key);
        code(&mut x[..CODE], g.db, cx.ids, d);
    }
    if let Some(Some(pw)) = item.attack_as {
        let x = sr_token(tail, sg, k, sr::K_ATTACK_AS, pos, 0, key);
        if let Some(d) = cx.ref_def(pw) {
            code(&mut x[..CODE], g.db, cx.ids, d);
        }
        cx.write_ref(&mut x[CODE + sr::OBJ..CODE + sr::OBJ + rf::W], pw);
    }
}

/// The `graveyard_order` segment (`go::*`): for each positional graveyard reader of the table
/// (`lower::graveyard_order::readers`) and each player, the frame's first, the cards of their
/// graveyard its filter matches, top first. (A reader's filter reads printed characteristics only,
/// checked at the table's build, R17; their types in a graveyard are the printed ones, asserted
/// here.)
fn write_graveyard_order(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    let readers = crate::lower::graveyard_order::readers(g.db);
    if readers.is_empty() {
        return;
    }
    let mut k = 0u32;
    for r in readers {
        for (side, p) in [cx.me, cx.op_].into_iter().enumerate() {
            let cards: Vec<CardIdx> = g.graveyard_cards(p).collect();
            let mut rank = 0u64;
            for &c in cards.iter().rev() {
                assert_eq!(g.types_of(c), g.def_of(c).types, "a graveyard card's types aren't its printed ones (R17, ENGINE.md 125)");
                if !crate::lower::graveyard_order::matches(g.db, &r.filter, g.def_of(c)) {
                    continue;
                }
                let x = tail.token(seg::GRAVEYARD_ORDER, k);
                k += 1;
                code(&mut x[..CODE], g.db, cx.ids, g.objs[c.i()].def);
                let s = &mut x[CODE..];
                s[go::PRESENT] = 1.0;
                s[if side == 0 { go::MINE } else { go::THEIRS }] = 1.0;
                s[go::RANK] = (rank + 1) as f32 / 32.0;
                bits(&mut s[go::RANK_BITS..go::RANK_BITS + 16], rank);
                digits(&mut s[go::READER..go::READER + 32], cx.ids, r.def);
                rank += 1;
            }
        }
    }
}

/// The pointer overrides of `viewer`'s row (zero-core docs/search.md 2.15; ENGINE.md 125): for each
/// option of `legal` (indexes ascending) about an object whose token is not the one the layout's
/// static table gives its action (zero-core's rule 2: a present head slot whose action it is), that
/// token's packed (segment, column, slot): an object past a segment's head slots (a tail token),
/// any object on a range page (absolute positions), an exile offer (`exile_offered`, no static
/// table). And -1 (the generic logit) for "next page" where the static table would score it by the
/// object in that slot (the list's `LIBRARY` + 63 on a paged list's first page, whose slot 63 holds
/// the pile's 64th card). A row whose objects all fit their head slots, unpaged and without exile
/// offers, gets none: every option keeps the logit of before (a widened network's, exactly).
/// The pointer overrides of `viewer`'s row in layout v8 (`v8`: v7's, then those into the
/// `pay_options` tokens, `tail_v8::pay_overrides`; ENGINE.md 236) or v7, by option index.
pub(crate) fn option_overrides_layout(g: &Game, viewer: PlayerId, legal: &[u16], out: &mut Vec<(u32, i64)>, v8: bool) {
    let n0 = out.len();
    option_overrides(g, viewer, legal, out);
    if v8 {
        super::tail_v8::pay_overrides(g, viewer, legal, out);
        out[n0..].sort_by_key(|o| o.0);
    }
}

pub(crate) fn option_overrides(g: &Game, viewer: PlayerId, legal: &[u16], out: &mut Vec<(u32, i64)>) {
    let Some(d) = g.pending.as_ref() else { return };
    // (The common row: an unpaged decision whose object options all name objects at head slots,
    // without exile offers, has none: no work beyond this scan.)
    let past_head = |a: u16| -> bool {
        let rel = |base: u16, n: u16| (base..base + n).contains(&a).then(|| a - base);
        rel(act::MY_PERM, act::PERM_N).is_some_and(|j| j >= encode::PERMS as u16)
            || rel(act::OPP_PERM, act::PERM_N).is_some_and(|j| j >= encode::PERMS as u16)
            || rel(act::ACTIVATE, act::PERM_N * 4).is_some_and(|j| j / 4 >= encode::PERMS as u16)
            || rel(act::HAND, act::HAND_N).is_some_and(|j| j >= encode::HANDS as u16)
            || rel(act::HAND_ABILITY, act::HAND_N).is_some_and(|j| j >= encode::HANDS as u16)
            || rel(act::STACK, act::STACK_N).is_some_and(|j| j >= encode::STACKS as u16)
            || rel(act::TRIGGER, act::TRIGGER_N).is_some_and(|j| j >= encode::TRIGS as u16)
            || rel(act::GY_ABILITY, act::GY_ABILITY_N).is_some_and(|j| j >= encode::GYAS as u16)
            || rel(act::EXILE, act::EXILE_N).is_some()
    };
    if d.stage == crate::decision::Stage::One && !legal.iter().any(|&a| past_head(a)) {
        return;
    }
    let cx = Cx::new(g, viewer);
    // (Only the knower's decision has object tokens for its options; another row's options keep
    // the static table.)
    if cx.d.is_none() {
        return;
    }
    let list: Vec<bool> = g.list_entries_v7(&cx).iter().map(|e| e.is_some()).collect();
    let n_trig = g.waiting.iter().filter(|t| t.controller == cx.me).count();
    let n_gya = g.graveyard_cards(cx.me).filter(|&c| !g.def_of(c).gy_abilities.is_empty()).count();
    // Is head slot `s` of segment `sg` present (layout v7: the object at absolute position s)?
    let present = |sg: u16, s: usize| -> bool {
        match sg {
            seg::MY_PERM => s < cx.mine.len(),
            seg::OPP_PERM => s < cx.theirs.len(),
            seg::HAND => s < cx.hand.len(),
            seg::STACK => s < g.stack.len(),
            seg::TRIGGERS => s < n_trig,
            seg::LIST => list.get(s).copied().unwrap_or(false),
            seg::GYA => s < n_gya,
            seg::MY_COMP => g.players[cx.me as usize].companion_def.is_some(),
            _ => false,
        }
    };
    // The static table of layout v7 (`layout_json`'s `actions`): action -> (segment, column, slot).
    let statik = |a: u16| -> Option<(u16, u8, usize)> {
        let hit = |base: u16, n: usize, stride: u16| -> Option<usize> {
            (a >= base && (a - base) % stride == 0 && ((a - base) / stride) < n as u16).then(|| ((a - base) / stride) as usize)
        };
        if let Some(s) = hit(act::MY_PERM, encode::PERMS, 1) {
            return Some((seg::MY_PERM, 0, s));
        }
        for i in 0..4u16 {
            if let Some(s) = hit(act::ACTIVATE + i, encode::PERMS, 4) {
                return Some((seg::MY_PERM, 1 + i as u8, s));
            }
        }
        if let Some(s) = hit(act::OPP_PERM, encode::PERMS, 1) {
            return Some((seg::OPP_PERM, 0, s));
        }
        if let Some(s) = hit(act::HAND, encode::HANDS, 1) {
            return Some((seg::HAND, 0, s));
        }
        if let Some(s) = hit(act::HAND_ABILITY, encode::HANDS, 1) {
            return Some((seg::HAND, 1, s));
        }
        if let Some(s) = hit(act::STACK, encode::STACKS, 1) {
            return Some((seg::STACK, 0, s));
        }
        if let Some(s) = hit(act::TRIGGER, encode::TRIGS, 1) {
            return Some((seg::TRIGGERS, 0, s));
        }
        if let Some(s) = hit(act::LIBRARY, encode::LIST, 1) {
            return Some((seg::LIST, 0, s));
        }
        if let Some(s) = hit(act::GY_ABILITY, encode::GYAS, 1) {
            return Some((seg::GYA, 0, s));
        }
        if a == act::COMPANION {
            return Some((seg::MY_COMP, 0, 0));
        }
        None
    };
    for (k, &a) in legal.iter().enumerate() {
        let want: Option<(u16, u8, u32)> = d.option_object(a).map(|o| match o {
            OptObject::MyPerm(p) => (seg::MY_PERM, 0, p),
            OptObject::Activate(p, i) => (seg::MY_PERM, 1 + i, p),
            OptObject::OppPerm(p) => (seg::OPP_PERM, 0, p),
            OptObject::Hand(p) => (seg::HAND, 0, p),
            OptObject::HandAbility(p) => (seg::HAND, 1, p),
            OptObject::Stack(p) => (seg::STACK, 0, p),
            OptObject::Trigger(p) => (seg::TRIGGERS, 0, p),
            OptObject::List(p) => (seg::LIST, 0, p),
            OptObject::GyAbility(p) => (seg::GYA, 0, p),
            OptObject::Exile(p) => (seg::EXILE_OFFERED, 0, p),
            OptObject::Companion => (seg::MY_COMP, 0, 0),
        });
        // (A target the row lacks - a head slot without its object, a missing tail token - is an
        // engine bug zero-core's `rows.resolve` refuses loudly (R6), and the fuzz's
        // `check_pointers` looks for; a state changed by hand, the public-state gate's, may have a
        // decision of before.)
        let st = statik(a).filter(|&(sg, _, s)| present(sg, s));
        let tok = match (want, st) {
            (Some((sg, col, s)), Some((sg2, col2, s2))) if sg == sg2 && col == col2 && s as usize == s2 => continue,
            (Some((sg, col, s)), _) => pack_tok(sg, col, s),
            // "Next page" where a static slot holds an object: the generic logit.
            (None, Some(_)) if matches!(d.choice_of(a), Some(Choice::Page(_))) => -1,
            (None, _) => continue,
        };
        out.push((k as u32, tok));
    }
}

// ---- a row read back (tests, the fuzz, the census: ENGINE.md 125) ----------------------------

/// A whole layout v7 row of a viewer: the head, the sealed tail (`(seg, slot, floats)`, sorted)
/// and the pointer overrides of its options (against `legal_actions`' order).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub head: Vec<f32>,
    pub tail: Vec<(u16, u32, Vec<f32>)>,
    pub ov: Vec<(u32, i64)>,
}

impl Row {
    /// `viewer`'s row of game `g` (its pending decision's options, if the viewer decides them).
    pub fn of(g: &Game, viewer: PlayerId) -> Row {
        Row::of_layout(g, viewer, false)
    }

    /// `viewer`'s layout v8 row of game `g` (ENGINE.md 236: v7's row and the tokens of v8's
    /// segments, with the overrides into them).
    pub fn of_v8(g: &Game, viewer: PlayerId) -> Row {
        Row::of_layout(g, viewer, true)
    }

    /// `viewer`'s row of game `g` in layout v8 (`v8`) or v7.
    pub fn of_layout(g: &Game, viewer: PlayerId, v8: bool) -> Row {
        let mut head = vec![0.0f32; encode::NUM_FEATURES];
        let mut t = TailBuf::new(if v8 { &TAILS_V8 } else { &TAILS });
        if v8 {
            g.encode_features_row_v8(viewer, &mut head, &mut t);
        } else {
            g.encode_features_row(viewer, &mut head, &mut t);
        }
        let tail = Row::sealed(&mut t);
        let mut ov = Vec::new();
        let mut legal = Vec::new();
        g.legal_actions(&mut legal);
        if !legal.is_empty() {
            option_overrides_layout(g, viewer, &legal, &mut ov, v8);
        }
        Row { head, tail, ov }
    }

    /// A buffer's tokens, sealed, as `(seg, slot, floats)`.
    fn sealed(t: &mut TailBuf) -> Vec<(u16, u32, Vec<f32>)> {
        t.seal();
        let mut tail = Vec::with_capacity(t.len());
        let mut off = 0;
        for (&sg, &sl) in t.seg().iter().zip(t.slot()) {
            let w = seg_w(sg);
            tail.push((sg, sl, t.val()[off..off + w].to_vec()));
            off += w;
        }
        tail
    }

    /// The row as one vector: the head, then each tail token's segment, slot and floats, then the
    /// overrides. Equal rows give equal vectors and **different rows different vectors** (tests
    /// compare rows by it: the public-state gate, the hidden-information tests, the fuzz's
    /// information sets): every integer is written exactly, in 16-bit pieces (an f32 holds 24
    /// bits; ENGINE.md 142.1: the override's packed `seg << 40 | col << 32 | slot` as one f32 lost
    /// the slot below bit 21 - slots 33, 34 and 70,000 compared equal - and a tail slot past 2^24,
    /// a `my_revealed` identity, would have too). The floats compare by value (0.0 == -0.0: the
    /// network reads them alike).
    pub fn flat(&self) -> Vec<f32> {
        fn int(v: &mut Vec<f32>, x: u64, pieces: u32) {
            for k in (0..pieces).rev() {
                v.push((x >> (16 * k) & 0xFFFF) as f32);
            }
        }
        let mut v = self.head.clone();
        v.push(-1e9);
        for (sg, sl, x) in &self.tail {
            v.push(*sg as f32);
            int(&mut v, *sl as u64, 2);
            v.extend_from_slice(x);
        }
        v.push(-2e9);
        for &(o, t) in &self.ov {
            int(&mut v, o as u64, 2);
            int(&mut v, t as u64, 4);
        }
        v
    }

    /// Where index `i` of `flat()` lies, for a check's message (ENGINE.md 223): a head float (its
    /// segment's slot and the float in it, or a block outside the segments), a tail token's float
    /// (segment name, absolute slot, the float in the token) or an override.
    pub fn locate(&self, i: usize) -> String {
        let nh = self.head.len();
        if i < nh {
            for s in 0..seg::N {
                let (o, w, n) = (encode::LAY7.off[s], SEG_W[s], SEG_HEAD[s]);
                if o != usize::MAX && n > 0 && i >= o && i < o + n * w {
                    return format!("head segment {s} slot {} float {}", (i - o) / w, (i - o) % w);
                }
            }
            return format!("head float {i} (outside the segments)");
        }
        let mut at = nh + 1;
        for (sg, sl, x) in &self.tail {
            let w = 3 + x.len();
            if i < at + w {
                let name = TAILS_V8.iter().find(|t| t.seg == *sg).map_or("?", |t| t.name);
                return match i - at {
                    0 => format!("tail token {name} slot {sl}: its segment"),
                    1 | 2 => format!("tail token {name} slot {sl}: its slot"),
                    k => format!("tail token {name} slot {sl} float {}", k - 3),
                };
            }
            at += w;
        }
        if i == at {
            return "the tail's end".into();
        }
        format!("override entry {}", (i - at - 1) / 6)
    }

    /// The floats of the token at (segment, slot): a head slot (None when not present) or a tail
    /// token.
    pub fn token(&self, sg: u16, slot: u32) -> Option<&[f32]> {
        let i = sg as usize;
        if (slot as usize) < seg_head(sg) {
            let (o, w) = (encode::LAY7.off[i], SEG_W[i]);
            let x = &self.head[o + slot as usize * w..o + (slot as usize + 1) * w];
            let p = match sg {
                seg::MY_PERM | seg::OPP_PERM => CODE + encode::perm::PRESENT,
                seg::MY_COMP | seg::OPP_COMP => CODE + encode::comp::PRESENT,
                seg::TOPS => 0,
                _ => CODE,
            };
            return (x[p] > 0.5).then_some(x);
        }
        self.tail.iter().find(|t| t.0 == sg && t.1 == slot).map(|t| &t.2[..])
    }

    /// The tokens of segment `sg` by slot: its present head slots, then its tail tokens.
    pub fn tokens(&self, sg: u16) -> Vec<(u32, &[f32])> {
        let mut v: Vec<(u32, &[f32])> = (0..seg_head(sg) as u32).filter_map(|s| self.token(sg, s).map(|x| (s, x))).collect();
        v.extend(self.tail.iter().filter(|t| t.0 == sg).map(|t| (t.1, &t.2[..])));
        v
    }

    /// The token that scores option `k` (`legal[k]`, action `a`): its override's target, else the
    /// present head slot whose static action `a` is (zero-core's rule 2); None for the generic
    /// logit.
    pub fn pointer(&self, k: usize, a: u16) -> Option<(u16, u8, u32)> {
        if let Some(&(_, t)) = self.ov.iter().find(|o| o.0 as usize == k) {
            return (t >= 0).then(|| zero_search::tail::unpack_tok(t));
        }
        static_slot(a).filter(|&(sg, _, s)| self.token(sg, s as u32).is_some()).map(|(sg, c, s)| (sg, c, s as u32))
    }

    /// The tail's tokens past `max_row_tokens`' head: 1 + sum count + tail (the row's capacity).
    pub fn capacity(&self) -> usize {
        HEAD_TOKENS + self.tail.len()
    }
}

/// The static table of layout v7 (`layout_json`'s `actions`): action -> (segment, column, slot).
pub fn static_slot(a: u16) -> Option<(u16, u8, usize)> {
    let hit = |base: u16, n: usize, stride: u16| -> Option<usize> {
        (a >= base && (a - base) % stride == 0 && ((a - base) / stride) < n as u16).then(|| ((a - base) / stride) as usize)
    };
    if let Some(s) = hit(act::MY_PERM, encode::PERMS, 1) {
        return Some((seg::MY_PERM, 0, s));
    }
    for i in 0..4u16 {
        if let Some(s) = hit(act::ACTIVATE + i, encode::PERMS, 4) {
            return Some((seg::MY_PERM, 1 + i as u8, s));
        }
    }
    if let Some(s) = hit(act::OPP_PERM, encode::PERMS, 1) {
        return Some((seg::OPP_PERM, 0, s));
    }
    if let Some(s) = hit(act::HAND, encode::HANDS, 1) {
        return Some((seg::HAND, 0, s));
    }
    if let Some(s) = hit(act::HAND_ABILITY, encode::HANDS, 1) {
        return Some((seg::HAND, 1, s));
    }
    if let Some(s) = hit(act::STACK, encode::STACKS, 1) {
        return Some((seg::STACK, 0, s));
    }
    if let Some(s) = hit(act::TRIGGER, encode::TRIGS, 1) {
        return Some((seg::TRIGGERS, 0, s));
    }
    if let Some(s) = hit(act::LIBRARY, encode::LIST, 1) {
        return Some((seg::LIST, 0, s));
    }
    if let Some(s) = hit(act::GY_ABILITY, encode::GYAS, 1) {
        return Some((seg::GYA, 0, s));
    }
    if a == act::COMPANION {
        return Some((seg::MY_COMP, 0, 0));
    }
    None
}

/// The identity of a card code's digits (`encode::digits`), None when it has none.
pub fn code_identity(x: &[f32]) -> Option<usize> {
    let d = |a: usize, n: usize| x[a..a + n].iter().position(|&v| v == 1.0);
    Some(d(0, 16)? * 64 + d(16, 8)? * 8 + d(24, 8)?)
}

/// Every option of `viewer`'s decision points at the token of the object it names (ENGINE.md
/// 125; the fuzz's check, generalizing v6's `check_hand_options` / `check_list_options`): its
/// pointer (static or override) is a token of its range's segment at the object's absolute
/// position, holding that object's identity and, for a capped segment, its option flag; an
/// option about no object keeps the generic logit or a static slot (the companion's `PLAYER` id).
/// `Err` with the first mismatch.
pub fn check_pointers(g: &Game, viewer: PlayerId, row: &Row) -> Result<usize, String> {
    check_pointers_layout(g, viewer, row, false)
}

/// `check_pointers` of a layout v8 row (`v8`, ENGINE.md 236) or v7: in v8 an option of the viewer's payment
/// decision naming item k (`ChooseMana`, `ChooseMode`, `PayPool`: `tail_v8::pay_overrides`) points at
/// `pay_options` token k, whose item is k.
pub fn check_pointers_layout(g: &Game, viewer: PlayerId, row: &Row, v8: bool) -> Result<usize, String> {
    let Some(d) = g.pending.as_ref() else { return Ok(0) };
    let cx = Cx::new(g, viewer);
    let Some(dd) = cx.d else { return Ok(0) };
    let ids = cx.ids;
    let mut legal = Vec::new();
    g.legal_actions(&mut legal);
    let mut checked = 0;
    for (k, &a) in legal.iter().enumerate() {
        let want = dd.option_object(a);
        let ptr = row.pointer(k, a);
        let what = || format!("{:?} option {a} ({}): {want:?}, pointer {ptr:?}", d.kind, g.describe_choice(dd.choice_of(a).expect("an option")));
        let Some(obj) = want else {
            // (Layout v8: a payment decision's item, its `pay_options` token.)
            let mut v8_item = None;
            if v8 && super::tail_v8::pays_items(g, dd) {
                v8_item = super::tail_v8::option_item(dd, a);
            }
            if let Some(item) = v8_item {
                if ptr != Some((seg::PAY_OPTIONS, 0, item)) {
                    return Err(format!("{} should point at pay_options token {item}", what()));
                }
                let Some(x) = row.token(seg::PAY_OPTIONS, item) else { return Err(format!("{}: no pay_options token {item}", what())) };
                let mut k = 0u32;
                for b in 0..16 {
                    k |= ((x[CODE + super::tail_v8::po::ITEM_BITS + b] > 0.5) as u32) << b;
                }
                if k != item || x[CODE + super::tail_v8::po::PRESENT] != 1.0 {
                    return Err(format!("{}: pay_options token {item} names item {k}", what()));
                }
                checked += 1;
                continue;
            }
            // (No object: the generic logit, or `PLAYER`'s static companion slot as before.)
            match ptr {
                None => continue,
                Some((sg, _, _)) if sg == seg::MY_COMP && a == act::COMPANION => continue,
                Some(_) => return Err(format!("{} points at a token", what())),
            }
        };
        let Some((sg, col, slot)) = ptr else { return Err(format!("{} keeps the generic logit", what())) };
        let (wsg, wcol, wpos) = match obj {
            OptObject::MyPerm(p) => (seg::MY_PERM, 0, p),
            OptObject::Activate(p, i) => (seg::MY_PERM, 1 + i, p),
            OptObject::OppPerm(p) => (seg::OPP_PERM, 0, p),
            OptObject::Hand(p) => (seg::HAND, 0, p),
            OptObject::HandAbility(p) => (seg::HAND, 1, p),
            OptObject::Stack(p) => (seg::STACK, 0, p),
            OptObject::Trigger(p) => (seg::TRIGGERS, 0, p),
            OptObject::List(p) => (seg::LIST, 0, p),
            OptObject::GyAbility(p) => (seg::GYA, 0, p),
            OptObject::Exile(p) => (seg::EXILE_OFFERED, 0, p),
            OptObject::Companion => (seg::MY_COMP, 0, 0),
        };
        if (sg, col, slot) != (wsg, wcol, wpos) {
            return Err(format!("{} should point at ({wsg}, {wcol}, {wpos})", what()));
        }
        let Some(x) = row.token(sg, slot) else { return Err(format!("{}: no token there", what())) };
        // The object's identity in the token (where the viewer sees it).
        let card: Option<CardIdx> = match obj {
            OptObject::MyPerm(p) | OptObject::Activate(p, _) => cx.mine.get(p as usize).copied(),
            OptObject::OppPerm(p) => cx.theirs.get(p as usize).copied(),
            OptObject::Hand(p) | OptObject::HandAbility(p) => cx.hand.get(p as usize).copied(),
            OptObject::Stack(p) => match g.stack.get(g.stack.len().wrapping_sub(1 + p as usize)).map(|s| s.kind) {
                Some(StackKind::Spell { card }) => Some(card),
                _ => None,
            },
            _ => None,
        };
        // A list option's token: the definition or card its choice names (a search, a pile, a seen
        // hand, a name, an ability's definition, a graveyard card).
        if let OptObject::List(_) = obj {
            let def = match dd.choice_of(a) {
                Some(Choice::Def(x)) => Some(x),
                Some(Choice::Card(c) | Choice::Cast(c) | Choice::Play(c) | Choice::CastFace(c) | Choice::PlayFace(c) | Choice::Faces(c, _)) => Some(g.objs[c.i()].def),
                Some(Choice::Target(Target::Obj(r))) => Some(g.objs[r.idx.i()].def),
                Some(Choice::Ability(c, slot)) => Some(g.activated_home_pub(c, slot)),
                _ => None,
            };
            if let Some(def) = def {
                if code_identity(&x[..CODE]) != Some(ids[def as usize] as usize) {
                    return Err(format!("{}: the list token is not {}", what(), g.db.def(def).name));
                }
            }
        }
        let id = code_identity(&x[..CODE]);
        if let Some(c) = card {
            let def = match g.objs[c.i()].zone {
                Zone::Battlefield => g.perm_identity(cx.knower, c),
                _ => g.objs[c.i()].def,
            };
            if id != Some(ids[def as usize] as usize) {
                return Err(format!("{}: the token's identity {id:?} is not {}'s", what(), g.name(c)));
            }
        }
        // Its option flag (capped segments).
        let flag = match obj {
            OptObject::MyPerm(_) => Some(x[CODE + encode::perm::OPTION]),
            OptObject::Activate(_, i) => Some(x[CODE + encode::perm::ACTIVATE + i as usize]),
            OptObject::OppPerm(_) => Some(x[CODE + encode::perm::OPTION]),
            OptObject::Hand(_) => Some(x[CODE + encode::hand::LEGAL]),
            OptObject::HandAbility(_) => Some(x[CODE + encode::hand::ABILITY_LEGAL]),
            OptObject::Trigger(_) => Some(x[CODE + encode::trig::LEGAL]),
            OptObject::List(_) => Some(x[CODE + encode::list::OPTION]),
            OptObject::GyAbility(_) => Some(x[CODE + 1]),
            OptObject::Exile(_) => Some(x[CODE + xo::PRESENT]),
            OptObject::Stack(_) | OptObject::Companion => None,
        };
        if flag.is_some_and(|f| f != 1.0) {
            return Err(format!("{}: the token's option flag is {flag:?}", what()));
        }
        checked += 1;
    }
    Ok(checked)
}

impl Row {
    /// `viewer`'s row of match `a` (zero-core's `encode_row` and `option_overrides`: the match block
    /// too, sideboarding's and the deck choice's encodings).
    pub fn of_az(a: &super::MtgAz, viewer: usize) -> Row {
        Row::of_az_layout(a, viewer, false)
    }

    /// `viewer`'s layout v8 row of match `a` (the game `mtg_v8`'s `encode_row` and
    /// `option_overrides`, ENGINE.md 236).
    pub fn of_az_v8(a: &super::MtgAz, viewer: usize) -> Row {
        Row::of_az_layout(a, viewer, true)
    }

    /// `viewer`'s row of match `a` in layout v8 (`v8`) or v7.
    pub fn of_az_layout(a: &super::MtgAz, viewer: usize, v8: bool) -> Row {
        use zero_search::game::SearchGame;
        let mut head = vec![0.0f32; encode::NUM_FEATURES];
        let mut t = TailBuf::new(if v8 { &TAILS_V8 } else { &TAILS });
        a.encode_row_layout(viewer, &mut head, &mut t, v8);
        let tail = Row::sealed(&mut t);
        let mut ov = Vec::new();
        let mut legal = Vec::new();
        a.legal_actions(&mut legal);
        if !legal.is_empty() {
            a.option_overrides_layout(viewer, &legal, &mut ov, v8);
        }
        Row { head, tail, ov }
    }
}

/// Synthetic big states for layout v7's tests (ENGINE.md 125; `tests/v7_tails.rs`): boards past
/// every old cap, built by hand (no rules run: the public-state gate's way), and the perturbations
/// of objects past the caps and of their order.
pub mod board {
    use crate::db::{CardDb, DelayedWhen};
    use crate::game::{CastFrom, Delayed, Game, Link, StackItem, StackKind};
    use crate::ids::{CardIdx, ObjRef, PlayerId, Target, Zone};
    use crate::testkit::Scenario;
    use crate::triggers::{EvCtx, PendingTrigger};

    /// The sizes of a board: permanents a side, the frame player's hand, stack items, waiting
    /// triggers a side, delayed triggers.
    #[derive(Clone, Copy, Debug)]
    pub struct Size {
        pub perms: usize,
        pub hand: usize,
        pub stack: usize,
        pub triggers: usize,
        pub delayed: usize,
    }

    /// A spell on the stack, made by hand: a new card of `name` for `p`, targeting the other player.
    fn push_spell(g: &mut Game, name: &str, p: PlayerId) {
        let def = g.db.expect_id(name).unwrap();
        let card = g.add_card(def, p, Zone::Stack);
        g.objs[card.i()].zone = Zone::Stack;
        let id = g.new_stack_id();
        g.stack.push(StackItem {
            id,
            kind: StackKind::Spell { card },
            controller: p,
            targets: vec![(0, Target::Player(crate::ids::opp(p)))],
            cost_option: 0,
            modes: Vec::new(),
            ev: EvCtx::default(),
            alt: 0,
            from: CastFrom::Hand,
            delved: 0,
            delve_floor: 0,
            discarded_land: false,
            x: 0,
            division: Vec::new(),
            attack_as: None,
            noted: Vec::new(),
            flips: 0,
            origin: Zone::Hand,
            times: 0,
            spliced: Vec::new(),
        });
    }

    /// A board of size `n` at P0's main phase (P0 the frame's player when it decides): Islands and
    /// Mountains alternating on both battlefields, Lightning Bolts and Counterspells in P0's hand,
    /// spells on the stack, Ouroboroid triggers waiting for both players, delayed triggers (the
    /// first with Mana Drain's amount 5,000), two cards exiled with gone sources (Memory Jar's
    /// pairing) and a library card known at depth 5 for each player.
    pub fn of(n: Size) -> Game {
        let db = CardDb::cube();
        let lands = |k: usize| -> Vec<&'static str> { (0..k).map(|i| if i % 2 == 0 { "Island" } else { "Mountain" }).collect() };
        let hand: Vec<&str> = (0..n.hand).map(|i| if i % 2 == 0 { "Lightning Bolt" } else { "Counterspell" }).collect();
        let mut g = Scenario::new(db)
            .spec(|s| s.auto_single = false)
            .library(0, &["Ponder"; 12])
            .library(1, &["Brainstorm"; 12])
            .battlefield(0, &lands(n.perms))
            .battlefield(1, &lands(n.perms))
            .hand(0, &hand)
            .exile(1, &["Ponder", "Brainstorm"])
            .at(crate::game::Step::Main1, 0)
            .start();
        for k in 0..n.stack {
            push_spell(&mut g, if k % 2 == 0 { "Lightning Bolt" } else { "Counterspell" }, (k % 2) as PlayerId);
        }
        let oro = g.db.expect_id("Ouroboroid").unwrap();
        for p in 0..2u8 {
            let perms: Vec<CardIdx> = g.permanents_of(p).collect();
            for k in 0..n.triggers {
                let src = perms[k % perms.len().max(1)];
                g.waiting.push(PendingTrigger {
                    controller: p,
                    source: g.obj_ref(src),
                    source_def: oro,
                    slot: 0,
                    ev: EvCtx { amount: k as u64, ..Default::default() },
                });
            }
        }
        let jar = g.db.expect_id("Memory Jar").unwrap();
        let drain = g.db.expect_id("Mana Drain").unwrap();
        for k in 0..n.delayed {
            let p = (k % 2) as PlayerId;
            let src = g.permanents_of(p).next().unwrap();
            g.delayed.push(Delayed {
                when: DelayedWhen::YourNextMainPhase,
                controller: p,
                source: g.obj_ref(src),
                source_def: drain,
                slot: 0,
                ev: EvCtx { amount: 5000 + k as u64, ..Default::default() },
                turn: g.turn.number,
            });
        }
        // Two Memory Jars gone, each with its exiled card and its delayed trigger.
        let exiled: Vec<CardIdx> = g.exile.clone();
        for (k, &card) in exiled.iter().enumerate() {
            let src = g.permanents_of(1).nth(k).unwrap();
            let gone = ObjRef { idx: src, inc: g.objs[src.i()].inc + 1 };
            g.links.push(Link { source: gone, card: g.obj_ref(card), until: false, back: Zone::Hand, until_monarch: None });
            g.delayed.push(Delayed {
                when: DelayedWhen::NextEndStep,
                controller: 1,
                source: gone,
                source_def: jar,
                slot: 0,
                ev: EvCtx::default(),
                turn: g.turn.number,
            });
        }
        for p in 0..2usize {
            let lib = &g.players[p].library;
            let c = lib[lib.len() - 1 - 5];
            g.objs[c.i()].known |= 1 << p;
        }
        g
    }

    /// Swap the cards at positions `i` and `j` of player `p`'s graveyard (bottom first), by hand.
    pub fn swap_graveyard(g: &mut Game, p: PlayerId, i: usize, j: usize) {
        g.players[p as usize].graveyard.swap(i, j);
    }

    /// A perturbation of a big board: a change past an old cap, or an order swap.
    pub type Perturb = fn(&mut Game) -> Option<String>;

    fn nth_perm(g: &Game, p: PlayerId, k: usize) -> Option<CardIdx> {
        g.permanents_of(p).nth(k)
    }

    /// The perturbations past the old caps (ENGINE.md 125; the design's section 9.2) and the
    /// order swaps, each named.
    pub fn perturbations() -> Vec<(&'static str, Perturb)> {
        vec![
            ("P0's 41st permanent is tapped", |g| {
                let c = nth_perm(g, 0, 40)?;
                g.objs[c.i()].tapped = !g.objs[c.i()].tapped;
                Some(g.name(c).into())
            }),
            ("a counter on P1's 34th permanent", |g| {
                let c = nth_perm(g, 1, 33)?;
                g.counters.push((c, crate::db::Counter::P1P1, 1));
                Some(g.name(c).into())
            }),
            ("P0's 35th permanent is attached to its 34th", |g| {
                let (a, b) = (nth_perm(g, 0, 34)?, nth_perm(g, 0, 33)?);
                g.objs[a.i()].attached = Some(g.obj_ref(b));
                Some("attached".into())
            }),
            ("the 10th stack item targets the other player", |g| {
                let n = g.stack.len();
                let it = g.stack.get_mut(n.checked_sub(10)?)?;
                let (_, t) = it.targets.first_mut()?;
                if let Target::Player(p) = t {
                    *p = crate::ids::opp(*p);
                }
                Some("retargeted".into())
            }),
            ("P0's 5th waiting trigger has another event amount", |g| {
                let t = g.waiting.iter_mut().filter(|t| t.controller == 0).nth(4)?;
                t.ev.amount += 1;
                Some(format!("{}", t.ev.amount))
            }),
            ("P0's 17th waiting trigger names an object", |g| {
                let c = nth_perm(g, 1, 0)?;
                let r = g.obj_ref(c);
                let t = g.waiting.iter_mut().filter(|t| t.controller == 0).nth(16)?;
                t.ev.obj = Some(r);
                Some("named".into())
            }),
            ("P1's 6th waiting trigger has another event amount", |g| {
                let t = g.waiting.iter_mut().filter(|t| t.controller == 1).nth(5)?;
                t.ev.amount += 1;
                Some(format!("{}", t.ev.amount))
            }),
            ("Mana Drain's delayed amount 5,000 -> 5,001", |g| {
                let d = g.delayed.iter_mut().find(|d| d.ev.amount == 5000)?;
                d.ev.amount = 5001;
                Some("5,001".into())
            }),
            ("two gone Memory Jars' linked cards swap", |g| {
                if g.links.len() < 2 {
                    return None;
                }
                let (a, b) = (g.links[0].card, g.links[1].card);
                g.links[0].card = b;
                g.links[1].card = a;
                Some("swapped".into())
            }),
            ("P1's library card at depth 5 becomes known to P0", |g| {
                let lib = &g.players[1].library;
                let c = lib[lib.len() - 1 - 5];
                g.objs[c.i()].known |= 1;
                Some("known".into())
            }),
            ("P0's 17th hand card is known to P1", |g| {
                let h = g.hand_sorted(0);
                let c = *h.get(16)?;
                g.objs[c.i()].known |= 1 << 1;
                Some(g.name(c).into())
            }),
            ("two of P0's permanents past the 32nd swap places", |g| {
                let (a, b) = (nth_perm(g, 0, 33)?, nth_perm(g, 0, 36)?);
                if g.objs[a.i()].def == g.objs[b.i()].def {
                    return None;
                }
                let (i, j) = (g.battlefield.iter().position(|&x| x == a)?, g.battlefield.iter().position(|&x| x == b)?);
                g.battlefield.swap(i, j);
                let (ta, tb) = (g.objs[a.i()].timestamp, g.objs[b.i()].timestamp);
                g.objs[a.i()].timestamp = tb;
                g.objs[b.i()].timestamp = ta;
                Some("swapped".into())
            }),
            ("two stack items past the 8th swap", |g| {
                let n = g.stack.len();
                let (i, j) = (n.checked_sub(10)?, n.checked_sub(11)?);
                g.stack.swap(i, j);
                Some("swapped".into())
            }),
            ("two of P0's waiting triggers past the 4th swap", |g| {
                let idx: Vec<usize> = (0..g.waiting.len()).filter(|&i| g.waiting[i].controller == 0).collect();
                let (i, j) = (*idx.get(5)?, *idx.get(6)?);
                g.waiting.swap(i, j);
                Some("swapped".into())
            }),
        ]
    }
}
