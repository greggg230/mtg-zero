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
//!   someone knows below the top three, the stack items' relations past the head's, and the order of
//!   the graveyard cards a positional reader can pick.
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
    /// The number of segments.
    pub const N: usize = 18;
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
/// permanent's as its slot shows it, any other card's only where `knows`.
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
    pub const W: usize = BY_OPP + 1;
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

pub const DELAYED_W: usize = CODE + dl::W;
pub const LINK_W: usize = CODE + lk::W;
pub const EXO_W: usize = CODE + xo::W;
pub const LK_W: usize = CODE + lb::W;
pub const SR_W: usize = CODE + sr::W;
pub const GO_W: usize = CODE + go::W;

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
];

/// Every segment's head slots (`count`) in layout v7.
pub const SEG_HEAD: [usize; seg::N] = [1, 2 * encode::TOPS, encode::PERMS, encode::PERMS, encode::HANDS, encode::STACKS, encode::TRIGS, encode::LIST, encode::GYAS, 1, 1, 0, 0, 0, 0, 0, 0, 0];

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
pub const TAILS: [TailSeg; 14] = [
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
];

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
    pub picks: Picks,
    stale: RefCell<Vec<ObjRef>>,
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

    /// The identity the viewer sees of card `c` where it is now, if any.
    pub fn seen_def(&self, c: CardIdx) -> Option<DefId> {
        let g = self.g;
        match g.objs[c.i()].zone {
            Zone::Battlefield | Zone::PhasedOut => Some(g.perm_identity(self.knower, c)),
            _ if g.knows(self.knower, c) => Some(g.objs[c.i()].def),
            _ => None,
        }
    }

    /// An object reference (`rf::*`).
    pub fn write_ref(&self, out: &mut [f32], r: ObjRef) {
        let g = self.g;
        let c = r.idx;
        let o = &g.objs[c.i()];
        out[rf::PRESENT] = 1.0;
        let side_of = |p: PlayerId| if p == self.me { rf::MINE } else { rf::THEIRS };
        if g.live(r) {
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
                Zone::FaceDown => g.face_down.iter().position(|&x| x == c),
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
        if let Some(d) = self.seen_def(c) {
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

    /// The picks in progress (`Picks`): the asking task's, or else the frames' of the agenda's
    /// resolutions. At most one task of the agenda holds picks (else a loud failure: a state the
    /// encoding's picks don't cover yet).
    fn picks_in_progress(&self) -> Picks {
        let g = self.g;
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
                Task::PayCosts { id, picked, .. } => {
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
            assert!(
                holders == 1,
                "picks in progress in two tasks of the agenda (the second: {t:?}): the encoding's picks (ENGINE.md 125) cover one"
            );
            out.objs = objs.iter().enumerate().map(|(k, &c)| (c, by(k) == self.me)).collect();
            if !defs.is_empty() {
                out.defs = defs.to_vec();
                out.searcher = Some(by(0));
            }
        }
        out
    }
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
    // Cards exiled with an object (CR 610.3): those still exiled.
    for l in &g.links {
        if !(g.live(l.card) && matches!(g.objs[l.card.idx.i()].zone, Zone::Exile | Zone::FaceDown)) {
            continue;
        }
        let x = link_token(tail, &mut k, lk::K_EXILED_WITH);
        if let Some(d) = cx.seen_def(l.card.idx) {
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
        if !g.live(o) {
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
                if let Some(d) = cx.seen_def(r.idx) {
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
    // in the frame player's hand; hidden from the knower: nothing (its own picks are seen).
    for (rank, &(c, by_me)) in cx.picks.objs.iter().enumerate() {
        let zone = g.objs[c.i()].zone;
        let in_list = list.contains(&Some(c));
        if zone == Zone::Battlefield || in_list || (zone == Zone::Hand && g.objs[c.i()].owner == cx.me) {
            continue;
        }
        if g.hidden_obj(c) && !g.knows(cx.knower, c) {
            continue;
        }
        let x = link_token(tail, &mut k, lk::K_PICKED);
        if let Some(d) = cx.seen_def(c) {
            code(&mut x[..CODE], g.db, ids, d);
        }
        let st = &mut x[CODE..];
        cx.write_ref(&mut st[lk::B..lk::B + rf::W], g.obj_ref(c));
        st[lk::RANK] = (rank + 1) as f32 / 8.0;
        st[if by_me { lk::BY_ME } else { lk::BY_OPP }] = 1.0;
    }
    if cx.picks.searcher == Some(cx.knower) {
        for (rank, &d) in cx.picks.defs.iter().enumerate() {
            let x = link_token(tail, &mut k, lk::K_PICKED_DEF);
            code(&mut x[..CODE], g.db, ids, d);
            x[CODE + lk::RANK] = (rank + 1) as f32 / 8.0;
            x[CODE + lk::BY_ME] = 1.0;
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
            Zone::FaceDown => g.face_down.iter().position(|&x| x == card),
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

/// A new `stack_relations` token at the next slot `k`: kind `kind` of the item at `pos` from the
/// top, index `index` in its list.
fn sr_token<'a>(tail: &'a mut TailBuf, k: &mut u32, kind: usize, pos: usize, index: usize) -> &'a mut [f32] {
    let x = tail.token(seg::STACK_RELATIONS, *k);
    *k += 1;
    x[CODE + sr::PRESENT] = 1.0;
    x[CODE + sr::KIND + kind] = 1.0;
    x[CODE + sr::ITEM_POS] = (pos + 1) as f32 / 32.0;
    x[CODE + sr::INDEX] = (index + 1) as f32 / 8.0;
    x
}

/// The `stack_relations` segment (`sr::*`), the stack from the top.
fn write_stack_relations(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    let mut k = 0u32;
    for (pos, item) in g.stack.iter().rev().enumerate() {
        for (j, &(req, t)) in item.targets.iter().enumerate() {
            if j < 2 && head_target(g, t) {
                continue;
            }
            let x = sr_token(tail, &mut k, sr::K_TARGET, pos, j);
            x[CODE + sr::REQ] = (req as f32 + 1.0) / 8.0;
            match t {
                Target::Player(p) => {
                    x[CODE + sr::PLAYER] = 1.0;
                    x[CODE + if p == cx.me { sr::PLAYER_ME } else { sr::PLAYER_OPP }] = 1.0;
                }
                Target::Obj(r) => {
                    if let Some(d) = cx.seen_def(r.idx) {
                        code(&mut x[..CODE], g.db, cx.ids, d);
                    }
                    cx.write_ref(&mut x[CODE + sr::OBJ..CODE + sr::OBJ + rf::W], r);
                }
                Target::Stack(id) => cx.write_item_ref(&mut x[CODE + sr::OBJ..CODE + sr::OBJ + rf::W], id),
            }
        }
        for (j, &n) in item.division.iter().enumerate().skip(2) {
            let x = sr_token(tail, &mut k, sr::K_DIVISION, pos, j);
            x[CODE + sr::AMOUNT] = squash(n as f32 / 8.0);
            bits(&mut x[CODE + sr::AMOUNT_BITS..CODE + sr::AMOUNT_BITS + 64], n);
        }
        for (j, &r) in item.noted.iter().enumerate() {
            if j == 0 && g.live(r) && g.objs[r.idx.i()].zone == Zone::Battlefield {
                continue;
            }
            let x = sr_token(tail, &mut k, sr::K_NOTED, pos, j);
            if let Some(d) = cx.seen_def(r.idx) {
                code(&mut x[..CODE], g.db, cx.ids, d);
            }
            cx.write_ref(&mut x[CODE + sr::OBJ..CODE + sr::OBJ + rf::W], r);
        }
        for (j, &d) in item.spliced.iter().enumerate() {
            let x = sr_token(tail, &mut k, sr::K_SPLICED, pos, j);
            code(&mut x[..CODE], g.db, cx.ids, d);
        }
        if let Some(Some(pw)) = item.attack_as {
            let x = sr_token(tail, &mut k, sr::K_ATTACK_AS, pos, 0);
            if let Some(d) = cx.seen_def(pw.idx) {
                code(&mut x[..CODE], g.db, cx.ids, d);
            }
            cx.write_ref(&mut x[CODE + sr::OBJ..CODE + sr::OBJ + rf::W], pw);
        }
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
        let mut head = vec![0.0f32; encode::NUM_FEATURES];
        let mut t = TailBuf::new(&TAILS);
        g.encode_features_row(viewer, &mut head, &mut t);
        t.seal();
        let mut tail = Vec::with_capacity(t.len());
        let mut off = 0;
        for (&sg, &sl) in t.seg().iter().zip(t.slot()) {
            let w = SEG_W[sg as usize];
            tail.push((sg, sl, t.val()[off..off + w].to_vec()));
            off += w;
        }
        let mut ov = Vec::new();
        let mut legal = Vec::new();
        g.legal_actions(&mut legal);
        if !legal.is_empty() {
            option_overrides(g, viewer, &legal, &mut ov);
        }
        Row { head, tail, ov }
    }

    /// The row as one vector: the head, then each tail token's segment, slot and floats, then the
    /// overrides (equal rows, equal vectors; tests compare rows by it).
    pub fn flat(&self) -> Vec<f32> {
        let mut v = self.head.clone();
        v.push(-1e9);
        for (sg, sl, x) in &self.tail {
            v.push(*sg as f32);
            v.push(*sl as f32);
            v.extend_from_slice(x);
        }
        v.push(-2e9);
        for &(o, t) in &self.ov {
            v.push(o as f32);
            v.push(t as f32);
            v.push((t >> 24) as f32);
        }
        v
    }

    /// The floats of the token at (segment, slot): a head slot (None when not present) or a tail
    /// token.
    pub fn token(&self, sg: u16, slot: u32) -> Option<&[f32]> {
        let i = sg as usize;
        if (slot as usize) < SEG_HEAD[i] {
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
        let mut v: Vec<(u32, &[f32])> = (0..SEG_HEAD[sg as usize] as u32).filter_map(|s| self.token(sg, s).map(|x| (s, x))).collect();
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
        use zero_search::game::SearchGame;
        let mut head = vec![0.0f32; encode::NUM_FEATURES];
        let mut t = TailBuf::new(&TAILS);
        a.encode_row(viewer, &mut head, &mut t);
        t.seal();
        let mut tail = Vec::with_capacity(t.len());
        let mut off = 0;
        for (&sg, &sl) in t.seg().iter().zip(t.slot()) {
            let w = SEG_W[sg as usize];
            tail.push((sg, sl, t.val()[off..off + w].to_vec()));
            off += w;
        }
        let mut ov = Vec::new();
        let mut legal = Vec::new();
        a.legal_actions(&mut legal);
        if !legal.is_empty() {
            a.option_overrides(viewer, &legal, &mut ov);
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
