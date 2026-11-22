//! Layout v8's own segments (ENGINE.md 236; the game `mtg_v8`): six tail-only gated segments
//! appended after layout v7's, so a v7 network widens onto v8 bit for bit (zero-core docs/az.md
//! 43.8: a new gated segment's tokens weigh exactly 0 at the widen) and a v8 row is the v7 row plus
//! these tokens (and the overrides into `pay_options`).
//!
//! * **`payment`** (`pm::*`): the payments in progress on the agenda (`Task::PayMana`, innermost
//!   first, each with its depth) - one token per picked source (its object, which of its sources -
//!   own ability and slot, hand ability, improvise / convoke, another's tap or sacrifice -, its pool,
//!   a several-mana source's production, its actor, its rank), one per source the payment activates
//!   first (`forced`), one for the payment's state (payer, the pool's mana spent, the Phyrexian
//!   life, the subject being asked: `now`'s source and the mana taken so far, `which`'s permanent,
//!   `actor_for`'s source; the pools it may leave, the life after, flags, the class of its last pick)
//!   and one per non-zero part of its cost (exact 64-bit amounts); and each 601.2g step in progress
//!   (`Task::CastMana`: the spell, the permanent whose ability is being chosen, asked again). The
//!   payment is public (601.2g-h): both players' rows; a picked hand card's identity only where the
//!   viewer knows it.
//! * **`pay_options`** (`po::*`, `columns: 1`): the option meanings of the viewer's payment decision -
//!   `ChooseMana`'s productions (or, chosen one mana at a time, the colour), `ChooseMode`'s sources
//!   of the permanent `which` names or the abilities of the permanent a 601.2g step asks about,
//!   `PayPool`'s pools (with their restricted parts as tokens of their own) - slot k the option's
//!   item k; every such option points at its token (`pay_overrides`).
//! * **`lki`** (`lk8::*`): per stale reference the row holds (slot k = its `rf::STALE_KEY` k, the
//!   key every holder of the reference shows) the gone object's last known information (CR 608.2h):
//!   its copiable values and mana value, power, toughness and controller, its qualities as a damage
//!   source (recorded whatever the cards of the game: `Game::lki_qualities`), what it was attached to (an
//!   Aura's host), whether a spell's item is recorded and its
//!   mana value; and one token per kind of counter it had.
//! * **`lki_spells`** / **`lki_relations`**: a gone spell's item (`Game::spell_lki`: "copy it") as a
//!   stack slot shows an item, and its targets, divisions, noted objects, spliced cards and
//!   ninjutsu planeswalker as `stack_relations` tokens show an item's - every one, each with the key.
//! * **`find_groups`** (`fg::*`): the groups of places that hold search finds mixed with other cards
//!   (`Game::find_groups`, ENGINE.md 223's decision 4): owner, the group's places (contiguous runs:
//!   zone, top depth and size, the whole library), how many hold finds and how many other cards.

use zero_search::tail::{pack_tok, TailBuf};

use super::encode::{code, squash, CODE};
use super::tail_encode::{ct, ef, rf, seg, Cx};
use crate::db::AbilitySlot;
use crate::decision::{act, Choice, DecisionKind};
use crate::flow::Task;
use crate::game::{Game, StackItem};
use crate::ids::{CardIdx, ObjRef, PlayerId, Zone};
use crate::pay::{PayMana, Rem, Src};

/// n's binary digits, least significant first.
fn bits(out: &mut [f32], n: u64) {
    for (b, x) in out.iter_mut().enumerate() {
        *x = (n >> b & 1) as f32;
    }
}

/// A source's kind (`src::KIND` one-hot) and index: an own activated ability (its slot), a hand
/// ability (its slot), improvise / convoke / waterbend (`pay::IMPROVISE`), another's tap
/// (`TAP_FOR`), a Pollinator's (`POL_FOR`), a granted or another's sacrifice (`SAC_FOR`, `SQ_FOR`,
/// `LJ_FOR`), another triggered slot, a graveyard ability. Exhaustive over `AbilitySlot`.
fn src_kind(slot: AbilitySlot) -> (usize, u8) {
    match slot {
        AbilitySlot::Activated(i) => (0, i),
        AbilitySlot::Hand(i) => (1, i),
        AbilitySlot::Spell => (2, 0),
        s if s == crate::pay::TAP_FOR => (3, 0),
        s if s == crate::pay::POL_FOR => (4, 0),
        s if s == crate::pay::SAC_FOR => (5, 0),
        s if s == crate::pay::SQ_FOR => (6, 0),
        s if s == crate::pay::LJ_FOR => (7, 0),
        AbilitySlot::Trigger(i) => (8, i),
        AbilitySlot::Graveyard(i) => (9, i),
    }
}

/// A source's block (`src::*`): its kind, slot index and pool.
pub mod src {
    pub const KIND: usize = 0;
    pub const KINDS: usize = 10;
    pub const SLOT_BITS: usize = KIND + KINDS;
    pub const POOL_BITS: usize = SLOT_BITS + 8;
    pub const W: usize = POOL_BITS + 16;
}

fn write_src(out: &mut [f32], slot: AbilitySlot, g: u16) {
    let (k, i) = src_kind(slot);
    out[src::KIND + k] = 1.0;
    bits(&mut out[src::SLOT_BITS..src::SLOT_BITS + 8], i as u64);
    bits(&mut out[src::POOL_BITS..src::POOL_BITS + 16], g as u64);
}

/// The `payment` token (after the card code): a kind, the payment task's depth among the agenda's
/// payment tasks (innermost 0), then the kind's own region from `R`.
pub mod pm {
    use super::{rf, src};
    pub const PRESENT: usize = 0;
    pub const KIND: usize = 1;
    pub const K_PICK: usize = 0;
    pub const K_FORCED: usize = 1;
    pub const K_STATE: usize = 2;
    pub const K_STEP: usize = 3;
    pub const K_COST: usize = 4;
    pub const KINDS: usize = 5;
    pub const DEPTH: usize = KIND + KINDS;
    pub const DEPTH_BITS: usize = DEPTH + 1;
    pub const R: usize = DEPTH_BITS + 8;
    // A picked source (`chosen`, `multi`) or a forced one (`forced`).
    pub const OBJ: usize = R;
    pub const SRC: usize = OBJ + rf::W;
    pub const MULTI: usize = SRC + src::W;
    pub const PROD: usize = MULTI + 1;
    pub const ACTOR: usize = PROD + 6;
    pub const ACTOR_OBJ: usize = ACTOR + 1;
    pub const RANK: usize = ACTOR_OBJ + rf::W;
    pub const RANK_BITS: usize = RANK + 1;
    pub const DELVE: usize = RANK_BITS + 16;
    pub const PICK_END: usize = DELVE + 1;
    // The payment's state.
    pub const PAYER_ME: usize = R;
    pub const PAYER_OPP: usize = R + 1;
    pub const VM: usize = R + 2;
    pub const VM_V: usize = VM + 1;
    pub const VM_BITS: usize = VM_V + 1;
    pub const POOLED: usize = VM_BITS + 32;
    pub const CONVERT: usize = POOLED + 1;
    pub const IMPROVISE: usize = CONVERT + 1;
    pub const WATERBEND: usize = IMPROVISE + 1;
    pub const CONVOKE: usize = WATERBEND + 1;
    pub const ORACLE: usize = CONVOKE + 1;
    pub const SPEND: usize = ORACLE + 1;
    pub const SPEND_N: usize = SPEND + 1;
    pub const SPEND_BITS: usize = SPEND_N + 1;
    pub const PHY: usize = SPEND_BITS + 16;
    pub const PHY_BITS: usize = PHY + 1;
    pub const LIFE_AFTER: usize = PHY_BITS + 64;
    pub const LIFE_AFTER_BITS: usize = LIFE_AFTER + 1;
    pub const MIN_CLASS_BITS: usize = LIFE_AFTER_BITS + 32;
    pub const NOW: usize = MIN_CLASS_BITS + 32;
    pub const NOW_OBJ: usize = NOW + 1;
    pub const NOW_SRC: usize = NOW_OBJ + rf::W;
    pub const NOW_N: usize = NOW_SRC + src::W;
    pub const NOW_N_BITS: usize = NOW_N + 1;
    pub const NOW_TAKEN: usize = NOW_N_BITS + 16;
    pub const NOW_MIN: usize = NOW_TAKEN + 6;
    pub const WHICH: usize = NOW_MIN + 6;
    pub const WHICH_OBJ: usize = WHICH + 1;
    pub const WHICH_N: usize = WHICH_OBJ + rf::W;
    pub const WHICH_N_BITS: usize = WHICH_N + 1;
    pub const ACTOR_FOR: usize = WHICH_N_BITS + 16;
    pub const AF_OBJ: usize = ACTOR_FOR + 1;
    pub const AF_SRC: usize = AF_OBJ + rf::W;
    pub const DELVE_LATE: usize = AF_SRC + src::W;
    pub const DELVE_LATE_BITS: usize = DELVE_LATE + 1;
    pub const STATE_END: usize = DELVE_LATE_BITS + 16;
    // A 601.2g step (`Task::CastMana`).
    pub const CASTER_ME: usize = R;
    pub const CASTER_OPP: usize = R + 1;
    pub const SPELL: usize = R + 2;
    pub const CARD: usize = SPELL + rf::W;
    pub const CARD_OBJ: usize = CARD + 1;
    pub const AGAIN: usize = CARD_OBJ + rf::W;
    pub const STEP_END: usize = AGAIN + 1;
    // A non-zero part of the payment's cost: which (generic, the six pips, the ten hybrid
    // pairs, the five Phyrexian pips), its amount squashed / 4 and its 64 binary digits.
    pub const PART: usize = R;
    pub const PARTS: usize = 22;
    pub const VAL: usize = PART + PARTS;
    pub const VAL_BITS: usize = VAL + 1;
    pub const COST_END: usize = VAL_BITS + 64;
    pub const W: usize = STATE_END;
}
const _: () = assert!(pm::PICK_END <= pm::W && pm::STEP_END <= pm::W && pm::COST_END <= pm::W);
pub const PM_W: usize = CODE + pm::W;

/// The `pay_options` token (after a zero card code): a kind, the option's item (its own; a pool's
/// restricted part: the item of the pool it belongs to), and the kinds' fields.
pub mod po {
    use super::src;
    pub const PRESENT: usize = 0;
    pub const KIND: usize = 1;
    /// `ChooseMana`: production k of the several-mana source `now` names.
    pub const K_PROD: usize = 0;
    /// `ChooseMana` one mana at a time: the next mana's colour.
    pub const K_COLOR: usize = 1;
    /// `ChooseMode` of a payment: source k of the permanent `which` names.
    pub const K_SRC: usize = 2;
    /// `ChooseMode` of a 601.2g step: ability k of the permanent picked.
    pub const K_SLOT: usize = 3;
    /// `PayPool`: pool k the payment may leave.
    pub const K_REM: usize = 4;
    /// A restricted part of a `K_REM` pool (`Rem::restr`), one token each.
    pub const K_REM_RESTR: usize = 5;
    pub const KINDS: usize = 6;
    pub const ITEM: usize = KIND + KINDS;
    pub const ITEM_BITS: usize = ITEM + 1;
    pub const PROD: usize = ITEM_BITS + 16;
    pub const COLOR: usize = PROD + 6;
    pub const SRC: usize = COLOR + 6;
    pub const HASTE: usize = SRC + src::W;
    pub const RESTR_N: usize = HASTE + 1;
    pub const RESTR_N_BITS: usize = RESTR_N + 1;
    pub const RTYPE_BITS: usize = RESTR_N_BITS + 8;
    pub const RSPEND_BITS: usize = RTYPE_BITS + 8;
    pub const RAMT: usize = RSPEND_BITS + 32;
    pub const W: usize = RAMT + 1;
}
pub const PO_W: usize = CODE + po::W;

/// The `lki` token (after the card code of the gone object's copiable values where the viewer
/// knew it): its kind, the stale key it describes, then the kind's region from `R`.
pub mod lk8 {
    use super::{ct, ef, rf};
    pub const PRESENT: usize = 0;
    pub const K_OBJECT: usize = 1;
    pub const K_COUNTER: usize = 2;
    pub const KEY: usize = 3;
    pub const KEY_BITS: usize = 4;
    pub const R: usize = KEY_BITS + 16;
    // The object (CR 608.2h).
    pub const HAS_COPY: usize = R;
    /// Its copiable values as an `effects` token's copy-effect fields show a copy effect's (`ef::*`).
    pub const COPYV: usize = HAS_COPY + 1;
    pub const MV: usize = COPYV + ef::W;
    pub const MV_BITS: usize = MV + 1;
    pub const HAS_POWER: usize = MV_BITS + 64;
    pub const POWER: usize = HAS_POWER + 1;
    pub const HAS_TOUGHNESS: usize = POWER + ef::NUM_W;
    pub const TOUGHNESS: usize = HAS_TOUGHNESS + 1;
    pub const T_ME: usize = TOUGHNESS + ef::NUM_W;
    pub const T_OPP: usize = T_ME + 1;
    pub const HAS_DAMAGE: usize = T_OPP + 1;
    pub const D_ME: usize = HAS_DAMAGE + 1;
    pub const D_OPP: usize = D_ME + 1;
    pub const DEATHTOUCH: usize = D_OPP + 1;
    pub const LIFELINK: usize = DEATHTOUCH + 1;
    pub const INFECT: usize = LIFELINK + 1;
    pub const SPELL: usize = INFECT + 1;
    pub const D_COLORS: usize = SPELL + 1;
    pub const D_TYPES: usize = D_COLORS + 8;
    pub const D_OBJ: usize = D_TYPES + 16;
    pub const D_OBJ_REF: usize = D_OBJ + 1;
    pub const HAS_ATTACHED: usize = D_OBJ_REF + rf::W;
    pub const ATTACHED: usize = HAS_ATTACHED + 1;
    pub const HAS_SPELL: usize = ATTACHED + rf::W;
    pub const SPELL_MV: usize = HAS_SPELL + 1;
    pub const SPELL_MV_BITS: usize = SPELL_MV + 1;
    pub const OBJECT_END: usize = SPELL_MV_BITS + 64;
    /// A kind of counter it had: a `counters` token's fields (`ct::*`, no object).
    pub const CTR: usize = R;
    pub const W: usize = if OBJECT_END > CTR + ct::W { OBJECT_END } else { CTR + ct::W };
}
pub const LK8_W: usize = CODE + lk8::W;

/// The `lki_spells` token: a stack slot's floats (`encode::STACK_W`, the card code first) of the
/// gone spell's item, its key and its mana value as it last existed on the stack.
pub mod ls {
    pub const KEY: usize = super::super::encode::STACK_W;
    pub const KEY_BITS: usize = KEY + 1;
    pub const MV: usize = KEY_BITS + 16;
    pub const MV_BITS: usize = MV + 1;
    pub const W: usize = MV_BITS + 64;
}
pub const LS_W: usize = ls::W;

/// The `lki_relations` token: a `stack_relations` token's floats (the card code first, `sr::*`) of
/// the gone spell's item, and the key.
pub mod lr {
    pub const KEY: usize = super::super::tail_encode::SR_W;
    pub const KEY_BITS: usize = KEY + 1;
    pub const W: usize = KEY_BITS + 16;
}
pub const LR_W: usize = lr::W;

/// The `find_groups` token (after a zero card code): one per contiguous run of a group's places.
pub mod fg {
    pub const PRESENT: usize = 0;
    pub const OWNER_ME: usize = 1;
    pub const OWNER_OPP: usize = 2;
    pub const GROUP: usize = 3;
    pub const GROUP_BITS: usize = 4;
    pub const ZONE: usize = GROUP_BITS + 16;
    /// The run's zone: a library, face-down exile, a hand (a count: a hand is a bag), elsewhere (a state
    /// changed by hand only).
    pub const ZONES: usize = 4;
    pub const TOP: usize = ZONE + ZONES;
    pub const TOP_BITS: usize = TOP + 1;
    pub const SIZE: usize = TOP_BITS + 16;
    pub const SIZE_BITS: usize = SIZE + 1;
    pub const WHOLE: usize = SIZE_BITS + 16;
    pub const FINDS: usize = WHOLE + 1;
    pub const FINDS_BITS: usize = FINDS + 1;
    pub const OTHERS: usize = FINDS_BITS + 16;
    pub const OTHERS_BITS: usize = OTHERS + 1;
    pub const RUNS: usize = OTHERS_BITS + 16;
    pub const W: usize = RUNS + 1;
}
pub const FG_W: usize = CODE + fg::W;

/// Layout v8's segments (ENGINE.md 236), after layout v7's tokens: the payments, the option
/// meanings, the find groups, then the last known information of every stale reference met (its
/// own references may meet more, written in turn).
pub(crate) fn write_v8_segments(cx: &Cx, tail: &mut TailBuf) {
    write_payment(cx, tail);
    write_pay_options(cx, tail);
    write_find_groups(cx, tail);
    write_lki(cx, tail);
}

// ---- payment ------------------------------------------------------------------------------

/// The payment tasks of the agenda, innermost first.
fn pay_tasks(g: &Game) -> impl Iterator<Item = &Task> {
    g.agenda.iter().rev().filter(|t| matches!(t, Task::PayMana(_) | Task::CastMana { .. }))
}

/// A picked or forced source's object reference: a permanent, or a hand card (its identity where
/// the viewer knows it: `Cx::seen_def`, as a pick in progress shows a card).
fn write_src_obj(cx: &Cx, out: &mut [f32], c: CardIdx) -> Option<crate::ids::DefId> {
    let d = cx.seen_def(c);
    cx.write_obj_as(out, c, d);                    // (named by where it is: written live, ENGINE.md 235)
    d
}

fn header(x: &mut [f32], kind: usize, depth: usize) {
    let s = &mut x[CODE..];
    s[pm::PRESENT] = 1.0;
    s[pm::KIND + kind] = 1.0;
    s[pm::DEPTH] = (depth + 1) as f32 / 8.0;
    bits(&mut s[pm::DEPTH_BITS..pm::DEPTH_BITS + 8], depth as u64);
}

fn cost_parts(c: &crate::mana::ManaCost) -> impl Iterator<Item = (usize, u64)> + '_ {
    std::iter::once(c.generic).chain(c.pips.iter().copied()).chain(c.hybrid.iter().copied()).chain(c.phyrexian.iter().copied()).enumerate().filter(|&(_, n)| n > 0)
}

/// The `payment` segment (`pm::*`).
fn write_payment(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    let mut k = 0u32;
    for (depth, t) in pay_tasks(g).enumerate() {
        match t {
            Task::PayMana(pm) => {
                let picks = pm.chosen.iter().map(|s| (s, None)).chain(pm.multi.iter().map(|(s, p)| (s, Some(p))));
                for (rank, (s, prod)) in picks.enumerate() {
                    let x = tail.token(seg::PAYMENT, k);
                    k += 1;
                    header(x, pm::K_PICK, depth);
                    pick_fields(cx, x, pm, s, rank, prod.copied(), None);
                }
                for (rank, (s, delve)) in pm.forced.iter().enumerate() {
                    let x = tail.token(seg::PAYMENT, k);
                    k += 1;
                    header(x, pm::K_FORCED, depth);
                    pick_fields(cx, x, pm, s, rank, None, Some(*delve));
                }
                let x = tail.token(seg::PAYMENT, k);
                k += 1;
                header(x, pm::K_STATE, depth);
                state_fields(cx, &mut x[CODE..], pm);
                for (part, n) in cost_parts(&pm.cost) {
                    let x = tail.token(seg::PAYMENT, k);
                    k += 1;
                    header(x, pm::K_COST, depth);
                    let s = &mut x[CODE..];
                    s[pm::PART + part] = 1.0;
                    s[pm::VAL] = squash(n as f32 / 4.0);
                    bits(&mut s[pm::VAL_BITS..pm::VAL_BITS + 64], n);
                }
            }
            Task::CastMana { id, card, again, .. } => {
                let x = tail.token(seg::PAYMENT, k);
                k += 1;
                header(x, pm::K_STEP, depth);
                let s = &mut x[CODE..];
                if let Some(i) = g.item_index(*id) {
                    let item = &g.stack[i];
                    s[if item.controller == cx.me { pm::CASTER_ME } else { pm::CASTER_OPP }] = 1.0;
                    cx.write_item_ref(&mut s[pm::SPELL..pm::SPELL + rf::W], *id);
                }
                if let Some(c) = card {
                    s[pm::CARD] = 1.0;
                    cx.write_obj(&mut s[pm::CARD_OBJ..pm::CARD_OBJ + rf::W], *c);
                }
                s[pm::AGAIN] = *again as u8 as f32;
            }
            _ => unreachable!("a payment task"),
        }
    }
}

/// A picked (`prod`: a several-mana source's production) or forced (`delve`) source's fields.
fn pick_fields(cx: &Cx, x: &mut [f32], pm: &PayMana, s: &Src, rank: usize, prod: Option<[u8; 6]>, delve: Option<bool>) {
    let d = write_src_obj(cx, &mut x[CODE + pm::OBJ..CODE + pm::OBJ + rf::W], s.card);
    if let Some(d) = d {
        code(&mut x[..CODE], cx.g.db, cx.ids, d);
    }
    let o = &mut x[CODE..];
    write_src(&mut o[pm::SRC..pm::SRC + src::W], s.slot, s.g);
    if let Some(p) = prod {
        o[pm::MULTI] = 1.0;
        for t in 0..6 {
            o[pm::PROD + t] = p[t] as f32 / 4.0;
        }
    }
    if let Some(&(_, a)) = pm.actors.iter().find(|(x, _)| x == s) {
        o[pm::ACTOR] = 1.0;
        cx.write_obj(&mut o[pm::ACTOR_OBJ..pm::ACTOR_OBJ + rf::W], a);
    }
    o[pm::RANK] = (rank + 1) as f32 / 32.0;
    bits(&mut o[pm::RANK_BITS..pm::RANK_BITS + 16], rank as u64);
    if let Some(dl) = delve {
        o[pm::DELVE] = dl as u8 as f32;
    }
}

fn state_fields(cx: &Cx, s: &mut [f32], pm: &PayMana) {
    let g = cx.g;
    s[if pm.payer == cx.me { pm::PAYER_ME } else { pm::PAYER_OPP }] = 1.0;
    if let Some(v) = pm.vm {
        s[pm::VM] = 1.0;
        s[pm::VM_V] = squash(v as f32 / 4.0);
        bits(&mut s[pm::VM_BITS..pm::VM_BITS + 32], v as u64);
    }
    s[pm::POOLED] = pm.pooled as u8 as f32;
    s[pm::CONVERT] = pm.convert as u8 as f32;
    s[pm::IMPROVISE] = pm.improvise as u8 as f32;
    s[pm::WATERBEND] = pm.waterbend as u8 as f32;
    s[pm::CONVOKE] = pm.convoke as u8 as f32;
    s[pm::ORACLE] = pm.oracle.is_some() as u8 as f32;
    if let Some(r) = &pm.spend {
        s[pm::SPEND] = 1.0;
        s[pm::SPEND_N] = squash(r.len() as f32 / 8.0);
        bits(&mut s[pm::SPEND_BITS..pm::SPEND_BITS + 16], r.len() as u64);
    }
    s[pm::PHY] = squash(pm.phyrexian_life as f32 / 8.0);
    bits(&mut s[pm::PHY_BITS..pm::PHY_BITS + 64], pm.phyrexian_life);
    s[pm::LIFE_AFTER] = squash(pm.life_after as f32 / 8.0);
    bits(&mut s[pm::LIFE_AFTER_BITS..pm::LIFE_AFTER_BITS + 32], pm.life_after as u64);
    bits(&mut s[pm::MIN_CLASS_BITS..pm::MIN_CLASS_BITS + 32], pm.min_class as u64);
    if let Some((src0, prods)) = &pm.now {
        s[pm::NOW] = 1.0;
        write_src_obj(cx, &mut s[pm::NOW_OBJ..pm::NOW_OBJ + rf::W], src0.card);
        write_src(&mut s[pm::NOW_SRC..pm::NOW_SRC + src::W], src0.slot, src0.g);
        s[pm::NOW_N] = squash(prods.len() as f32 / 8.0);
        bits(&mut s[pm::NOW_N_BITS..pm::NOW_N_BITS + 16], prods.len() as u64);
    }
    for t in 0..6 {
        s[pm::NOW_TAKEN + t] = pm.now_taken[t] as f32 / 4.0;
    }
    if pm.now_taken != [0; 6] {
        s[pm::NOW_MIN + pm.now_min as usize] = 1.0;
    }
    if let Some(c) = pm.which {
        s[pm::WHICH] = 1.0;
        cx.write_obj(&mut s[pm::WHICH_OBJ..pm::WHICH_OBJ + rf::W], c);
        let n = pm.card_slots.iter().find(|(x, _)| *x == c).map_or(0, |(_, v)| v.len());
        s[pm::WHICH_N] = squash(n as f32 / 8.0);
        bits(&mut s[pm::WHICH_N_BITS..pm::WHICH_N_BITS + 16], n as u64);
    }
    if let Some(a) = &pm.actor_for {
        s[pm::ACTOR_FOR] = 1.0;
        write_src_obj(cx, &mut s[pm::AF_OBJ..pm::AF_OBJ + rf::W], a.card);
        write_src(&mut s[pm::AF_SRC..pm::AF_SRC + src::W], a.slot, a.g);
    }
    s[pm::DELVE_LATE] = squash(pm.delve_late.len() as f32 / 4.0);
    bits(&mut s[pm::DELVE_LATE_BITS..pm::DELVE_LATE_BITS + 16], pm.delve_late.len() as u64);
}

// ---- the payment decisions' options ------------------------------------------------------

/// What the viewer's payment decision's items mean, if it is one (`pay_options`): the item list.
enum PayItems<'a> {
    Prods(&'a [[u8; 6]]),
    Colors,
    Srcs(&'a [(AbilitySlot, u16)]),
    Slots(Vec<AbilitySlot>),
    Rems(&'a [Rem]),
}

/// The items of `g`'s pending decision when it is a payment decision of `viewer`'s (the option
/// whose choice is `Number(k)` / `Item(k)` means item k).
fn pay_items<'a>(g: &'a Game, d: &crate::decision::Decision) -> Option<PayItems<'a>> {
    match (d.kind, g.agenda.last()?) {
        (DecisionKind::ChooseMana, Task::PayMana(pm)) => {
            let (_, prods) = pm.now.as_ref()?;
            let big = prods.len() > act::NUMBER_N as usize || pm.now_taken != [0; 6];
            Some(if big { PayItems::Colors } else { PayItems::Prods(prods) })
        }
        (DecisionKind::ChooseMode, Task::PayMana(pm)) => {
            let c = pm.which?;
            pm.card_slots.iter().find(|(x, _)| *x == c).map(|(_, v)| PayItems::Srcs(v))
        }
        (DecisionKind::PayPool, Task::PayMana(pm)) => pm.spend.as_deref().map(PayItems::Rems),
        (DecisionKind::ChooseMode, Task::CastMana { id, card: Some(c), .. }) => Some(PayItems::Slots(g.cast_mana_slots(*id, *c))),
        _ => None,
    }
}

/// A new `pay_options` token at `slot`: kind `kind` of item `item`; its fields after the code.
fn po_token(tail: &mut TailBuf, slot: u32, kind: usize, item: usize) -> &mut [f32] {
    let x = tail.token(seg::PAY_OPTIONS, slot);
    let s = &mut x[CODE..];
    s[po::PRESENT] = 1.0;
    s[po::KIND + kind] = 1.0;
    s[po::ITEM] = (item + 1) as f32 / 32.0;
    bits(&mut s[po::ITEM_BITS..po::ITEM_BITS + 16], item as u64);
    s
}

/// Is `g`'s pending decision `d` a payment decision whose options name items (`pay_options`)?
pub(crate) fn pays_items(g: &Game, d: &crate::decision::Decision) -> bool {
    pay_items(g, d).is_some()
}

/// The `pay_options` segment (`po::*`).
fn write_pay_options(cx: &Cx, tail: &mut TailBuf) {
    let Some(d) = cx.d else { return };
    let Some(items) = pay_items(cx.g, d) else { return };
    match items {
        PayItems::Prods(prods) => {
            for (k, p) in prods.iter().enumerate() {
                let s = po_token(tail, k as u32, po::K_PROD, k);
                for t in 0..6 {
                    s[po::PROD + t] = p[t] as f32 / 4.0;
                }
            }
        }
        PayItems::Colors => {
            for o in &d.options {
                if let Choice::Number(t) = o.choice {
                    let s = po_token(tail, t as u32, po::K_COLOR, t as usize);
                    s[po::COLOR + t as usize] = 1.0;
                }
            }
        }
        PayItems::Srcs(v) => {
            for (k, &(slot, gp)) in v.iter().enumerate() {
                let s = po_token(tail, k as u32, po::K_SRC, k);
                write_src(&mut s[po::SRC..po::SRC + src::W], slot, gp);
            }
        }
        PayItems::Slots(v) => {
            for (k, &slot) in v.iter().enumerate() {
                let s = po_token(tail, k as u32, po::K_SLOT, k);
                write_src(&mut s[po::SRC..po::SRC + src::W], slot, 0);
            }
        }
        PayItems::Rems(rems) => {
            let mut extra = rems.len() as u32;
            for (k, r) in rems.iter().enumerate() {
                let s = po_token(tail, k as u32, po::K_REM, k);
                for t in 0..6 {
                    s[po::PROD + t] = r.amt[t] as f32 / 4.0;
                }
                s[po::HASTE] = r.haste as f32 / 4.0;
                s[po::RESTR_N] = squash(r.restr.len() as f32 / 4.0);
                bits(&mut s[po::RESTR_N_BITS..po::RESTR_N_BITS + 8], r.restr.len() as u64);
                for &(ty, only, amt) in &r.restr {
                    let s = po_token(tail, extra, po::K_REM_RESTR, k);
                    extra += 1;
                    bits(&mut s[po::RTYPE_BITS..po::RTYPE_BITS + 8], ty as u64);
                    bits(&mut s[po::RSPEND_BITS..po::RSPEND_BITS + 32], only as u64);
                    s[po::RAMT] = amt as f32 / 4.0;
                }
            }
        }
    }
}

/// Layout v8's pointer overrides (`option_overrides_layout`): each option of the viewer's payment
/// decision whose choice names item k (`Number(k)`, `Item(k)`) points at `pay_options` token k.
pub(crate) fn pay_overrides(g: &Game, viewer: PlayerId, legal: &[u16], out: &mut Vec<(u32, i64)>) {
    let Some(d) = g.pending.as_ref() else { return };
    if !matches!(d.kind, DecisionKind::ChooseMana | DecisionKind::ChooseMode | DecisionKind::PayPool) {
        return;
    }
    if d.player != g.encoding_seat(viewer) || g.decider_of(d.player) != viewer {
        return;
    }
    if pay_items(g, d).is_none() {
        return;
    }
    for (k, &a) in legal.iter().enumerate() {
        if let Some(item) = option_item(d, a) {
            out.push((k as u32, pack_tok(seg::PAY_OPTIONS, 0, item)));
        }
    }
}

/// The item a payment decision's option names (`Number(k)` / `Item(k)`), if any.
pub(crate) fn option_item(d: &crate::decision::Decision, a: u16) -> Option<u32> {
    match d.choice_of(a)? {
        Choice::Number(k) => Some(k as u32),
        Choice::Item(k) => Some(k),
        _ => None,
    }
}

// ---- find groups --------------------------------------------------------------------------

/// A group's members' places as contiguous runs: (zone index, top place, size), sorted; and
/// whether the members are their owner's whole library.
pub(crate) fn group_runs(g: &Game, grp: &crate::hidden::FindGroup) -> (Vec<(usize, usize, usize)>, bool) {
    let lib = &g.players[grp.owner as usize].library;
    let mut places: Vec<(usize, usize)> = grp
        .members
        .iter()
        .map(|&c| match g.objs[c.i()].zone {
            Zone::Library => (0, lib.iter().rev().position(|&x| x == c).expect("a member in its library")),
            // (Face-down exile and a hand are bags: a count, ENGINE.md 142.3.)
            Zone::FaceDown => (1, 0),
            Zone::Hand => (2, 0),
            // (Never by the rules: a member leaving its zone ends its group, `move_card`; a state changed by
            // hand - the public-state gate's moves - may hold one elsewhere, a bag.)
            _ => (3, 0),
        })
        .collect();
    places.sort_unstable();
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    for (z, p) in places {
        match runs.last_mut() {
            Some(r) if r.0 == z && (z != 0 || r.1 + r.2 == p) => r.2 += 1,
            _ => runs.push((z, p, 1)),
        }
    }
    let whole = !lib.is_empty() && grp.members.len() == lib.len() && grp.members.iter().all(|&c| g.objs[c.i()].zone == Zone::Library);
    (runs, whole)
}

/// The find groups in their public order: by owner (the frame player's first), then by their
/// first run.
pub(crate) fn groups_in_order<'a>(g: &'a Game, me: PlayerId) -> Vec<(&'a crate::hidden::FindGroup, Vec<(usize, usize, usize)>, bool)> {
    let mut v: Vec<_> = g.find_groups.iter().map(|grp| {
        let (r, w) = group_runs(g, grp);
        (grp, r, w)
    }).collect();
    v.sort_by(|a, b| ((a.0.owner != me), &a.1, a.0.finds).cmp(&((b.0.owner != me), &b.1, b.0.finds)));
    v
}

/// The `find_groups` segment (`fg::*`): one token per run of each group's places.
fn write_find_groups(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    if g.find_groups.is_empty() {
        return;
    }
    let mut k = 0u32;
    for (gi, (grp, runs, whole)) in groups_in_order(g, cx.me).into_iter().enumerate() {
        for &(z, top, size) in &runs {
            let x = tail.token(seg::FIND_GROUPS, k);
            k += 1;
            let s = &mut x[CODE..];
            s[fg::PRESENT] = 1.0;
            s[if grp.owner == cx.me { fg::OWNER_ME } else { fg::OWNER_OPP }] = 1.0;
            s[fg::GROUP] = (gi + 1) as f32 / 32.0;
            bits(&mut s[fg::GROUP_BITS..fg::GROUP_BITS + 16], gi as u64);
            s[fg::ZONE + z] = 1.0;
            s[fg::TOP] = (top + 1) as f32 / 64.0;
            bits(&mut s[fg::TOP_BITS..fg::TOP_BITS + 16], top as u64);
            s[fg::SIZE] = size as f32 / 64.0;
            bits(&mut s[fg::SIZE_BITS..fg::SIZE_BITS + 16], size as u64);
            s[fg::WHOLE] = whole as u8 as f32;
            let others = grp.members.len() as u64 - grp.finds as u64;
            s[fg::FINDS] = squash(grp.finds as f32 / 4.0);
            bits(&mut s[fg::FINDS_BITS..fg::FINDS_BITS + 16], grp.finds as u64);
            s[fg::OTHERS] = squash(others as f32 / 4.0);
            bits(&mut s[fg::OTHERS_BITS..fg::OTHERS_BITS + 16], others);
            s[fg::RUNS] = squash(runs.len() as f32 / 4.0);
        }
    }
}

// ---- last known information ---------------------------------------------------------------

/// The last known information `g` holds of gone object `r` (CR 608.2h), each record's latest.
pub(crate) struct Lki<'a> {
    pub copy: Option<&'a (ObjRef, crate::copies::CopyVals, u64)>,
    pub power: Option<i64>,
    pub toughness: Option<(i64, PlayerId)>,
    pub damage: Option<crate::game::DamageSource>,
    pub attached: Option<ObjRef>,
    pub spell: Option<&'a (ObjRef, crate::ids::DefId, StackItem, u64)>,
    pub counters: Vec<(crate::db::Counter, crate::num::Count)>,
}

impl Lki<'_> {
    pub fn is_empty(&self) -> bool {
        self.copy.is_none()
            && self.power.is_none()
            && self.toughness.is_none()
            && self.damage.is_none()
            && self.attached.is_none()
            && self.spell.is_none()
            && self.counters.is_empty()
    }
}

/// `r`'s last known information (empty for a live reference: an object that is there has none).
pub(crate) fn lki_of(g: &Game, r: ObjRef) -> Lki<'_> {
    if g.live(r) {
        return Lki { copy: None, power: None, toughness: None, damage: None, attached: None, spell: None, counters: Vec::new() };
    }
    // (Counters: the record's entries of `r`, in their order; a kind once, its latest count.)
    let mut counters: Vec<(crate::db::Counter, crate::num::Count)> = Vec::new();
    for &(x, k, n) in &g.lki_counters {
        if x == r {
            match counters.iter_mut().find(|e| e.0 == k) {
                Some(e) => e.1 = n,
                None => counters.push((k, n)),
            }
        }
    }
    Lki {
        copy: g.lki_copy.iter().rev().find(|x| x.0 == r),
        power: g.lki_power_of(r),
        toughness: g.lki_toughness.iter().rev().find(|x| x.0 == r).map(|x| (x.1, x.2)),
        // (Recorded whatever the cards of the game: `Game::lki_qualities`, not the rules' `lki_damage`.)
        damage: g.lki_qualities.iter().rev().find(|x| x.0 == r).map(|x| x.1),
        // (EoE W3's `lki_host` is the same entry, kept when a card reads it.)
        attached: g.lki_attached.iter().find(|x| x.0 == r).map(|x| x.1),
        spell: g.spell_lki.iter().rev().find(|x| x.0 == r),
        counters,
    }
}

/// The `lki`, `lki_spells` and `lki_relations` segments: every stale reference met in the row, in
/// key order (the references these tokens write may meet more, taken in turn).
fn write_lki(cx: &Cx, tail: &mut TailBuf) {
    let g = cx.g;
    // Every gone object a reference of the public state names (`Game::for_each_public_ref`, the view's), met
    // now if no token of the row wrote it: its key follows every v7 token's.
    g.for_each_public_ref(|r| {
        if !g.live(r) {
            cx.stale_key_of(r);
        }
        false
    });
    let (mut kc, mut kr) = (0u32, 0u32);
    let mut key = 0usize;
    loop {
        let stale = cx.stale_refs();
        if key >= stale.len() {
            break;
        }
        let r = stale[key];
        let l = lki_of(g, r);
        if !l.is_empty() {
            let knew = g.lki_identity(cx.knower, r).is_some();
            let key_floats = |s: &mut [f32], at: usize| {
                s[at] = (key + 1) as f32 / 32.0;
                bits(&mut s[at + 1..at + 17], key as u64);
            };
            {
                let x = tail.token(seg::LKI, key as u32);
                let s = &mut x[CODE..];
                s[lk8::PRESENT] = 1.0;
                s[lk8::K_OBJECT] = 1.0;
                key_floats(s, lk8::KEY);
                if let Some((_, vals, mv)) = l.copy {
                    s[lk8::HAS_COPY] = 1.0;
                    if knew {
                        super::tail_encode::copy_vals(g, cx.ids, &mut s[lk8::COPYV..lk8::COPYV + ef::W], vals);
                    }
                    s[lk8::MV] = squash(*mv as f32 / 8.0);
                    bits(&mut s[lk8::MV_BITS..lk8::MV_BITS + 64], *mv);
                }
                if let Some(p) = l.power {
                    s[lk8::HAS_POWER] = 1.0;
                    super::tail_encode::num(&mut s[lk8::POWER..lk8::POWER + ef::NUM_W], p);
                }
                if let Some((t, ctl)) = l.toughness {
                    s[lk8::HAS_TOUGHNESS] = 1.0;
                    super::tail_encode::num(&mut s[lk8::TOUGHNESS..lk8::TOUGHNESS + ef::NUM_W], t);
                    s[if ctl == cx.me { lk8::T_ME } else { lk8::T_OPP }] = 1.0;
                }
                if let Some(d) = l.damage {
                    s[lk8::HAS_DAMAGE] = 1.0;
                    s[if d.controller == cx.me { lk8::D_ME } else { lk8::D_OPP }] = 1.0;
                    s[lk8::DEATHTOUCH] = d.deathtouch as u8 as f32;
                    s[lk8::LIFELINK] = d.lifelink as u8 as f32;
                    s[lk8::INFECT] = d.infect as u8 as f32;
                    s[lk8::SPELL] = d.spell as u8 as f32;
                    bits(&mut s[lk8::D_COLORS..lk8::D_COLORS + 8], d.colors as u64);
                    bits(&mut s[lk8::D_TYPES..lk8::D_TYPES + 16], d.types as u64);
                    if let Some(o) = d.obj {
                        s[lk8::D_OBJ] = 1.0;
                        cx.write_ref(&mut s[lk8::D_OBJ_REF..lk8::D_OBJ_REF + rf::W], o);
                    }
                }
                if let Some(a) = l.attached {
                    s[lk8::HAS_ATTACHED] = 1.0;
                    cx.write_ref(&mut s[lk8::ATTACHED..lk8::ATTACHED + rf::W], a);
                }
                if let Some((_, _, _, mv)) = l.spell {
                    s[lk8::HAS_SPELL] = 1.0;
                    s[lk8::SPELL_MV] = squash(*mv as f32 / 8.0);
                    bits(&mut s[lk8::SPELL_MV_BITS..lk8::SPELL_MV_BITS + 64], *mv);
                }
                if let (Some((_, vals, _)), true) = (l.copy, knew) {
                    code(&mut x[..CODE], g.db, cx.ids, vals.def);
                }
            }
            for &(kind, n) in &l.counters {
                let x = tail.token(seg::LKI, (1 << 24) | kc);
                kc += 1;
                let s = &mut x[CODE..];
                s[lk8::PRESENT] = 1.0;
                s[lk8::K_COUNTER] = 1.0;
                key_floats(s, lk8::KEY);
                super::tail_encode::counter_floats(g, &mut s[lk8::CTR..lk8::CTR + ct::W], kind, n);
            }
            if let Some((_, def, item, mv)) = l.spell {
                {
                    let x = tail.token(seg::LKI_SPELLS, key as u32);
                    g.write_departed_item(cx, *def, item, &mut x[..super::encode::STACK_W]);
                    key_floats(x, ls::KEY);
                    x[ls::MV] = squash(*mv as f32 / 8.0);
                    bits(&mut x[ls::MV_BITS..ls::MV_BITS + 64], *mv);
                }
                super::tail_encode::item_relations(cx, item, None, tail, seg::LKI_RELATIONS, &mut kr, Some((lr::KEY, key)));
            }
        }
        key += 1;
    }
}

// ---- the public view (`Game::view_of`) of v8's facts ---------------------------------------

/// h64 of a value (`testkit`'s, for the view).
fn h64<T: std::hash::Hash>(t: &T) -> u64 {
    crate::testkit::h64(t)
}

/// The payments in progress as `viewer` observes them (`Game::view_of`, ENGINE.md 236): every
/// field `payment` and `pay_options` show, objects by their public place.
pub(crate) fn payment_view(g: &Game, viewer: PlayerId, v: &mut Vec<u64>) {
    let place = |c: CardIdx| -> u64 {
        // (A hand card: hidden unless the viewer knows it - its owner's place only.)
        match g.objs[c.i()].zone {
            Zone::Hand => h64(&("a hand card", g.objs[c.i()].owner, g.knows(viewer, c).then_some(g.objs[c.i()].def))),
            _ => g.view_place(c),
        }
    };
    let src = |s: &Src| -> [u64; 3] { [place(s.card), h64(&s.slot), s.g as u64] };
    for (depth, t) in pay_tasks(g).enumerate() {
        v.push(u64::MAX - 120 - depth as u64);
        match t {
            Task::PayMana(pm) => {
                v.push(pm.payer as u64);
                for s in &pm.chosen {
                    v.extend(src(s));
                    v.push(pm.actors.iter().find(|(x, _)| x == s).map_or(u64::MAX, |&(_, a)| place(a)));
                }
                v.push(u64::MAX - 1);
                for (s, p) in &pm.multi {
                    v.extend(src(s));
                    v.push(h64(p));
                    v.push(pm.actors.iter().find(|(x, _)| x == s).map_or(u64::MAX, |&(_, a)| place(a)));
                }
                v.push(u64::MAX - 2);
                for (s, dl) in &pm.forced {
                    v.extend(src(s));
                    v.push(*dl as u64);
                }
                v.push(u64::MAX - 3);
                v.extend([h64(&pm.cost), h64(&pm.vm), pm.pooled as u64, pm.convert as u64, pm.improvise as u64, pm.waterbend as u64, pm.convoke as u64]);
                v.extend([pm.oracle.is_some() as u64, pm.phyrexian_life, pm.life_after as u64, pm.min_class as u64, pm.delve_late.len() as u64]);
                match &pm.now {
                    Some((s, prods)) => {
                        v.extend(src(s));
                        v.push(h64(prods));
                    }
                    None => v.push(u64::MAX - 4),
                }
                v.extend([h64(&pm.now_taken), if pm.now_taken != [0; 6] { pm.now_min as u64 } else { 0 }]);
                match pm.which {
                    Some(c) => {
                        v.push(place(c));
                        v.push(h64(&pm.card_slots.iter().find(|(x, _)| *x == c).map(|(_, s)| s.clone())));
                    }
                    None => v.push(u64::MAX - 5),
                }
                match &pm.actor_for {
                    Some(s) => v.extend(src(s)),
                    None => v.push(u64::MAX - 6),
                }
                match &pm.spend {
                    Some(r) => v.push(h64(r)),
                    None => v.push(u64::MAX - 7),
                }
            }
            Task::CastMana { id, card, again, .. } => {
                v.push(g.item_index(*id).map_or(u64::MAX, |i| (g.stack.len() - 1 - i) as u64));
                v.push(card.map_or(u64::MAX, place));
                v.push(*again as u64);
            }
            _ => unreachable!("a payment task"),
        }
    }
}

/// The last known information of every gone object a reference of the public state names (and
/// those the information itself names), as `viewer` observes it (`Game::view_of`, ENGINE.md 236).
pub(crate) fn lki_view(g: &Game, viewer: PlayerId, v: &mut Vec<u64>) {
    let mut refs: Vec<ObjRef> = Vec::new();
    g.for_each_public_ref(|r| {
        if !g.live(r) && !refs.contains(&r) {
            refs.push(r);
        }
        false
    });
    let mut i = 0;
    let mut out: Vec<Vec<u64>> = Vec::new();
    while i < refs.len() {
        let r = refs[i];
        i += 1;
        let l = lki_of(g, r);
        if l.is_empty() {
            continue;
        }
        let mut more = |x: ObjRef| {
            if !g.live(x) && !refs.contains(&x) {
                refs.push(x);
            }
        };
        let live = |x: ObjRef| g.view_live_place(x);
        let mut e: Vec<u64> = vec![g.lki_identity(viewer, r).map_or(u64::MAX, |d| d as u64)];
        let knew = g.lki_identity(viewer, r).is_some();
        e.push(l.copy.map_or(u64::MAX, |(_, vals, mv)| h64(&(if knew { Some(vals) } else { None }, mv))));
        e.push(l.power.map_or(u64::MAX, |p| p as u64));
        e.push(l.toughness.map_or(u64::MAX, |t| h64(&t)));
        e.push(l.damage.map_or(u64::MAX, |d| h64(&(d.controller, d.deathtouch, d.lifelink, d.infect, d.spell, d.colors, d.types, d.obj.map(live)))));
        e.push(l.attached.map_or(u64::MAX, live));
        e.push(h64(&l.counters));
        if let Some(d) = l.damage.and_then(|d| d.obj) {
            more(d);
        }
        if let Some(a) = l.attached {
            more(a);
        }
        if let Some((_, def, item, mv)) = l.spell {
            e.extend([*def as u64, *mv, item.controller as u64, item.cost_option as u64, item.alt as u64, h64(&item.from), item.delved as u64]);
            e.extend([item.x as u64, h64(&item.origin), item.times as u64, h64(&item.modes), h64(&item.division), h64(&item.spliced)]);
            e.push(item.attack_as.map_or(u64::MAX, |a| a.map_or(u64::MAX - 1, live)));
            e.extend([item.ev.player.map_or(9, |p| p as u64), item.ev.obj.map_or(u64::MAX, live), item.ev.obj2.map_or(u64::MAX, live), item.ev.amount]);
            e.extend(item.noted.iter().map(|&x| live(x)));
            for &(req, t) in &item.targets {
                e.push(req as u64);
                e.push(match t {
                    crate::ids::Target::Player(p) => p as u64,
                    crate::ids::Target::Obj(o) => 100 + live(o),
                    crate::ids::Target::Stack(sid) => 9000 + g.item_index(sid).map_or(99, |i| (g.stack.len() - 1 - i) as u64),
                });
            }
        }
        out.push(e);
    }
    // (In no order of the encoder's: a multiset.)
    out.sort();
    for e in out {
        v.push(u64::MAX - 130);
        v.extend(e);
    }
}

/// The find groups as everyone observes them (`Game::view_of`, ENGINE.md 236): per group its
/// owner, runs of places, finds and other cards.
pub(crate) fn find_groups_view(g: &Game, v: &mut Vec<u64>) {
    let mut out: Vec<Vec<u64>> = g
        .find_groups
        .iter()
        .map(|grp| {
            let (runs, whole) = group_runs(g, grp);
            let mut e = vec![grp.owner as u64, grp.finds as u64, grp.members.len() as u64, whole as u64];
            e.extend(runs.iter().flat_map(|&(z, t, n)| [z as u64, t as u64, n as u64]));
            e
        })
        .collect();
    out.sort();
    for e in out {
        v.push(u64::MAX - 140);
        v.extend(e);
    }
}
