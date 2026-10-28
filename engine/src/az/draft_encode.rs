//! The network input of `team_draft` (DESIGN.md 2.3.5): layout `draft_v2`
//! (ENGINE.md 47; `draft_v1` before deckbuilding chose the next card), 14,492
//! floats for a viewer, with nothing it cannot know (its own packs and picks;
//! after the draft its team's knowledge and its teammates' decks).
//!
//! | Block | Offset | Floats | What |
//! |---|---:|---:|---|
//! | globals | 0 | 68 | phase, round, pick, direction, team size, my place, who decides, sizes, my pool's colours, the deck being built, `DONE` offered, the pool cards still needed, the minimum (`gl::*`) |
//! | pack | 68 | 15 x 68 | my current pack (sorted by identity): card code, present, an option of my decision, my view number of this pack |
//! | pool | 1,088 | 45 x 76 | my picks in pick order: card code + `pool::*` (round, pick, first pick, in / out of the deck, still addable, an option now, build position, a companion the deck so far fulfills) |
//! | bags | 4,508 | 13 x 768 | identities x 0.25 (`bag::*`): my draft (taken by others between two of my views, passed with their fate unknown, wheeled), teammates' pools (3), teammates' decks (3), opponents' known pools (4) |
//!
//! A card code is 64 floats: the identity's digits (16, 8, 8; vocabulary
//! 1024, the `mtg` layout's) and a descriptor of 32 (`desc`).

use std::sync::OnceLock;

use crate::db::{supertypes, types, DeckCond};
use crate::ids::DefId;
use crate::mana::COLORS;

use super::decks;
use super::draft::{cube, DraftCube, Phase, TeamDraftAz, MAX_PACK, MAX_POOL, MAX_TEAM};
use super::identity;

pub const CODE: usize = 64;
pub const GLOBALS: usize = 68;
pub const PACK_W: usize = CODE + 4;
pub const POOL_W: usize = CODE + 12;
pub const BAGS: usize = 13;
/// Bag width: the cube's identities (`identity::CUBE_CAP`).
pub const BAG_W: usize = identity::CUBE_CAP;

pub const O_GLOBALS: usize = 0;
pub const O_PACK: usize = O_GLOBALS + GLOBALS;
pub const O_POOL: usize = O_PACK + MAX_PACK * PACK_W;
pub const O_BAGS: usize = O_POOL + MAX_POOL * POOL_W;
pub const NUM_FEATURES: usize = O_BAGS + BAGS * BAG_W;

/// Globals.
pub mod gl {
    pub const DRAFT: usize = 0;
    pub const BUILD: usize = 1;
    pub const DONE: usize = 2;
    pub const RESOLVED: usize = 3;
    /// The round, one-hot (3).
    pub const ROUND: usize = 4;
    pub const PICK: usize = 8;
    pub const PICKS_LEFT: usize = 9;
    pub const LEFT: usize = 10;
    pub const RIGHT: usize = 11;
    /// Team size 1..4, one-hot.
    pub const TEAM_SIZE: usize = 12;
    /// My player index in my team, one-hot (4).
    pub const MY_INDEX: usize = 16;
    pub const MY_TEAM_B: usize = 20;
    pub const I_DECIDE: usize = 21;
    pub const TEAMMATE_DECIDES: usize = 22;
    pub const OPPONENT_DECIDES: usize = 23;
    pub const MY_POOL: usize = 24;
    pub const MY_PACK: usize = 25;
    /// My pool's nonland cards by colour (W U B R G) and colourless / 15.
    pub const POOL_COLOURS: usize = 26;
    /// My deck so far: coloured pips (5) / 20, lands / 20.
    pub const DECK_PIPS: usize = 32;
    pub const DECK_LANDS: usize = 37;
    /// My deck so far: nonland cards by mana value 0..7+ / 10.
    pub const DECK_CURVE: usize = 38;
    pub const DECK_SIZE: usize = 46;
    /// My deck's basic lands (W U B R G) / 20.
    pub const DECK_BASICS: usize = 47;
    /// I choose the next pool card / a basic count (the type, one-hot 5).
    pub const BUILD_CARD: usize = 52;
    pub const BUILD_BASIC: usize = 53;
    pub const OPTIONS: usize = 58;
    pub const BUILD_POS: usize = 59;
    pub const SEATS_BUILT: usize = 60;
    pub const PACKS: usize = 61;
    pub const PACK_SIZE: usize = 62;
    pub const ROUNDS: usize = 63;
    /// `DONE` is one of my options (draft_v2).
    pub const DONE_OK: usize = 64;
    /// My deck's pool cards / 40; the pool cards it still needs before
    /// `DONE` / 40; the event's minimum / 40 (draft_v2).
    pub const POOL_IN: usize = 65;
    pub const NEED: usize = 66;
    pub const DECK_MIN: usize = 67;
}

/// Pack slot status (after the card code).
pub mod pack {
    pub const PRESENT: usize = 0;
    pub const OPTION: usize = 1;
    pub const VIEW: usize = 2;
}

/// Pool slot status (after the card code).
pub mod pool {
    pub const PRESENT: usize = 0;
    /// The round it was picked in, one-hot (3).
    pub const ROUND: usize = 1;
    pub const PICK: usize = 4;
    pub const FIRST_PICK: usize = 5;
    pub const IN: usize = 6;
    /// Before the build position and not in the deck (or every card not in
    /// it once the cards are chosen): the sideboard.
    pub const OUT: usize = 7;
    /// While I choose my cards: at or after the build position (it may still
    /// be added, now or later; draft_v1: the card decided now).
    pub const AVAILABLE: usize = 8;
    /// Adding it is one of my options now.
    pub const OPTION: usize = 9;
    pub const BUILD_POS: usize = 10;
    pub const COMPANION_OK: usize = 11;
}

/// Bags.
pub mod bag {
    pub const TAKEN_BY_OTHERS: usize = 0;
    pub const PASSED_UNKNOWN: usize = 1;
    pub const WHEELED: usize = 2;
    /// Teammates in seat order after me (3).
    pub const TEAMMATE_POOL: usize = 3;
    pub const TEAMMATE_DECK: usize = 6;
    /// Opponents in seat order from my left (4).
    pub const OPPONENT_KNOWN: usize = 9;
}

/// The descriptor of every identity (32 floats; zeros for none), from the
/// card's definition only.
fn descriptors(cube: &DraftCube) -> &'static [[f32; 32]] {
    static D: OnceLock<Vec<[f32; 32]>> = OnceLock::new();
    D.get_or_init(|| {
        (0..identity::CUBE_CAP)
            .map(|i| {
                let mut x = [0f32; 32];
                let def = cube.def[i];
                if def == DefId::MAX {
                    return x;
                }
                let d = cube.db.def(def);
                for (k, t) in [types::ARTIFACT, types::CREATURE, types::ENCHANTMENT, types::INSTANT, types::LAND, types::SORCERY, types::PLANESWALKER]
                    .iter()
                    .enumerate()
                {
                    x[k] = (d.types & t != 0) as u8 as f32;
                }
                for c in 0..5 {
                    x[7 + c] = (d.colors & (1 << c) != 0) as u8 as f32;
                }
                let land = d.is(types::LAND);
                x[12] = (d.colors & COLORS == 0 && !land) as u8 as f32;
                x[13] = d.mana_value() as f32 / 8.0;
                let cost = &d.cost;
                let mut pips = [0f32; 5];
                for (c, p) in pips.iter_mut().enumerate() {
                    *p += cost.pips[c] as f32 + cost.phyrexian[c] as f32;
                }
                for (k, &n) in cost.hybrid.iter().enumerate() {
                    let m = crate::mana::pair_mask(k);
                    for (c, p) in pips.iter_mut().enumerate() {
                        if m & (1 << c) != 0 {
                            *p += n as f32;
                        }
                    }
                }
                for c in 0..5 {
                    x[14 + c] = pips[c] / 3.0;
                }
                x[19] = (cost.hybrid.iter().any(|&n| n > 0) || cost.phyrexian.iter().any(|&n| n > 0)) as u8 as f32;
                x[20] = d.power.unwrap_or(0) as f32 / 8.0;
                x[21] = d.toughness.unwrap_or(0) as f32 / 8.0;
                x[22] = (d.supertypes & supertypes::LEGENDARY != 0) as u8 as f32;
                x[23] = d.companion.is_some() as u8 as f32;
                if land {
                    let m = decks::land_colors(cube.db, d);
                    for c in 0..5 {
                        x[24 + c] = (m & (1 << c) != 0) as u8 as f32;
                    }
                    x[29] = (m & (1 << 5) != 0) as u8 as f32;
                    x[30] = decks::is_fetch(d) as u8 as f32;
                }
                x[31] = decks::interacts(d) as u8 as f32;
                x
            })
            .collect()
    })
}

/// A card code: the identity's digits and the descriptor.
fn code(out: &mut [f32], desc: &[[f32; 32]], ident: u16) {
    let i = ident as usize;
    out[(i >> 6) & 15] = 1.0;
    out[16 + ((i >> 3) & 7)] = 1.0;
    out[24 + (i & 7)] = 1.0;
    out[32..64].copy_from_slice(&desc[i]);
}

#[inline]
fn bag_add(out: &mut [f32], b: usize, ident: u16, n: f32) {
    out[O_BAGS + b * BAG_W + ident as usize] += 0.25 * n;
}

/// Encode `g` for `viewer` into `out` (`NUM_FEATURES` floats, zeroed by the
/// caller). Every value is bounded by the event's sizes (ENGINE.md 46: at
/// most 40 copies of a basic land in a bag, 10; 45 picks / 15 = 3), far
/// within `encode::FEATURE_BOUND`; the draft fuzz checks it.
pub fn encode(g: &TeamDraftAz, viewer: u8, out: &mut [f32]) {
    assert_eq!(out.len(), NUM_FEATURES);
    let cube = cube().expect("the cube was loaded");
    let desc = descriptors(cube);
    let ev = &g.ev;
    let n = ev.n;
    let s0 = ev.size as usize;
    let phase = g.phase();
    let decider = g.decider();
    let x = &mut out[O_GLOBALS..O_GLOBALS + GLOBALS];
    match phase {
        Phase::Draft => x[gl::DRAFT] = 1.0,
        Phase::Build => x[gl::BUILD] = 1.0,
        Phase::Done => x[gl::DONE] = 1.0,
    }
    x[gl::RESOLVED] = g.outcome().is_some() as u8 as f32;
    let (round, pick, _) = g.pick_round();
    if phase == Phase::Draft {
        x[gl::ROUND + round as usize] = 1.0;
        x[gl::PICK] = pick as f32 / 15.0;
        x[gl::PICKS_LEFT] = (s0 - pick as usize) as f32 / 15.0;
        x[if ev.left(round as usize) { gl::LEFT } else { gl::RIGHT }] = 1.0;
    }
    x[gl::TEAM_SIZE + ev.t as usize - 1] = 1.0;
    x[gl::MY_INDEX + (viewer / 2) as usize] = 1.0;
    x[gl::MY_TEAM_B] = (viewer % 2) as f32;
    if let Some(d) = decider {
        let i = if d == viewer {
            gl::I_DECIDE
        } else if d % 2 == viewer % 2 {
            gl::TEAMMATE_DECIDES
        } else {
            gl::OPPONENT_DECIDES
        };
        x[i] = 1.0;
    }
    x[gl::PACKS] = ev.packs as f32 / 3.0;
    x[gl::PACK_SIZE] = ev.size as f32 / 15.0;
    x[gl::ROUNDS] = ev.rounds as f32 / 3.0;
    x[gl::SEATS_BUILT] = match phase {
        Phase::Draft => 0.0,
        Phase::Build => g.builder() as f32 / 8.0,
        Phase::Done => n as f32 / 8.0,
    };
    let picks = g.picks(viewer);
    x[gl::MY_POOL] = picks.len() as f32 / 45.0;
    for &(c, _, _) in &picks {
        let d = cube.db.def(cube.def_of(c));
        if d.is(types::LAND) {
            continue;
        }
        let cols = d.colors & COLORS;
        if cols == 0 {
            x[gl::POOL_COLOURS + 5] += 1.0 / 15.0;
        }
        for c in 0..5 {
            if cols & (1 << c) != 0 {
                x[gl::POOL_COLOURS + c] += 1.0 / 15.0;
            }
        }
    }
    let mut legal = Vec::new();
    let me_decide = decider == Some(viewer);
    if me_decide {
        g.legal(&mut legal);
        x[gl::OPTIONS] = legal.len() as f32 / 32.0;
    }

    // My deck so far (deckbuilding).
    x[gl::DECK_MIN] = ev.deck_min as f32 / 40.0;
    let my_build = g.build(viewer);
    if let Some(b) = my_build {
        let mut lands = 0f32;
        for i in 0..b.order.len() {
            if b.inn & (1 << i) == 0 {
                continue;
            }
            let d = cube.db.def(cube.def_of(b.order[i]));
            if d.is(types::LAND) {
                lands += 1.0;
                continue;
            }
            for c in 0..5 {
                x[gl::DECK_PIPS + c] += desc[b.order[i] as usize][14 + c] * 3.0 / 20.0;
            }
            x[gl::DECK_CURVE + (d.mana_value() as usize).min(7)] += 0.1;
        }
        for j in 0..5 {
            lands += b.basics[j] as f32;
            x[gl::DECK_BASICS + j] = b.basics[j] as f32 / 20.0;
        }
        x[gl::DECK_LANDS] = lands / 20.0;
        x[gl::DECK_SIZE] = b.size() as f32 / 40.0;
        x[gl::BUILD_POS] = b.k as f32 / 45.0;
        x[gl::POOL_IN] = b.pool_cards() as f32 / 40.0;
        x[gl::NEED] = b.need(ev.deck_min) as f32 / 40.0;
        if me_decide && phase == Phase::Build {
            if !b.cards_done() {
                x[gl::BUILD_CARD] = 1.0;
                x[gl::DONE_OK] = b.done_ok(ev.deck_min) as u8 as f32;
            } else {
                x[gl::BUILD_BASIC + b.basic_k as usize] = 1.0;
            }
        }
    }

    // My current pack (the draft).
    if let Some(p) = g.pack_held(viewer) {
        let (v, nv) = g.views(p, 1 << viewer);
        let view_no = v[..nv].iter().filter(|&&t| (t as u8) < pick).count();
        for (k, &c) in g.pack(p).iter().enumerate() {
            let o = O_PACK + k * PACK_W;
            code(&mut out[o..o + CODE], desc, c);
            let st = &mut out[o + CODE..o + PACK_W];
            st[pack::PRESENT] = 1.0;
            st[pack::OPTION] = (me_decide && legal.binary_search(&c).is_ok()) as u8 as f32;
            st[pack::VIEW] = view_no as f32 / 4.0;
        }
    }

    // My picks.
    let mut deck_now: Vec<DefId> = Vec::new();
    if let Some(b) = my_build {
        deck_now = b.deck(cube).iter().map(|&c| cube.def_of(c)).collect();
    }
    for (k, &(c, r, t)) in picks.iter().enumerate().take(MAX_POOL) {
        let o = O_POOL + k * POOL_W;
        code(&mut out[o..o + CODE], desc, c);
        let st = &mut out[o + CODE..o + POOL_W];
        st[pool::PRESENT] = 1.0;
        st[pool::ROUND + r as usize] = 1.0;
        st[pool::PICK] = t as f32 / 15.0;
        st[pool::FIRST_PICK] = (t == 0) as u8 as f32;
        if let Some(b) = my_build {
            if let Some(i) = b.order.iter().position(|&y| y == c) {
                st[pool::BUILD_POS] = i as f32 / 45.0;
                if i < b.k as usize {
                    st[if b.inn & (1 << i) != 0 { pool::IN } else { pool::OUT }] = 1.0;
                } else if me_decide && phase == Phase::Build {
                    st[pool::AVAILABLE] = 1.0;
                    st[pool::OPTION] = legal.binary_search(&c).is_ok() as u8 as f32;
                }
            }
            if let Some(cond) = &cube.db.def(cube.def_of(c)).companion {
                st[pool::COMPANION_OK] = companion_ok(cube, cond, &deck_now) as u8 as f32;
            }
        }
    }

    // My draft: what I saw.
    for p in 0..ev.n_packs() {
        let (v, nv) = g.views(p, 1 << viewer);
        for i in 0..nv {
            let t = v[i] as usize;
            if t >= g.taken(p) {
                break; // my view now: the pack segment
            }
            let next = if i + 1 < nv { Some(v[i + 1] as usize) } else { None };
            match next {
                Some(u) => {
                    for j in t + 1..u {
                        bag_add(out, bag::TAKEN_BY_OTHERS, g.cell(p, j), 1.0);
                    }
                    for j in u..s0 {
                        bag_add(out, bag::WHEELED, g.cell(p, j), 1.0);
                    }
                }
                None => {
                    for j in t + 1..s0 {
                        bag_add(out, bag::PASSED_UNKNOWN, g.cell(p, j), 1.0);
                    }
                }
            }
        }
    }

    // After the draft: my team's knowledge.
    if phase != Phase::Draft {
        for k in 1..ev.t {
            let m = (viewer + 2 * k) % n;
            let slot = (k - 1) as usize;
            for c in g.pool(m) {
                bag_add(out, bag::TEAMMATE_POOL + slot, c, 1.0);
            }
            if let Some(b) = g.build(m) {
                let started = phase == Phase::Done || m <= g.builder();
                if started {
                    for i in 0..(b.k as usize).min(b.order.len()) {
                        if b.inn & (1 << i) != 0 {
                            bag_add(out, bag::TEAMMATE_DECK + slot, b.order[i], 1.0);
                        }
                    }
                    for j in 0..5 {
                        if b.basics[j] > 0 {
                            bag_add(out, bag::TEAMMATE_DECK + slot, cube.basic_ids[j], b.basics[j] as f32);
                        }
                    }
                }
            }
        }
        for k in 0..ev.t {
            let o = (viewer + 2 * k + 1) % n;
            let fp = g.first_picks(o);
            for c in g.pool(o) {
                if fp.binary_search(&c).is_err() {
                    bag_add(out, bag::OPPONENT_KNOWN + k as usize, c, 1.0);
                }
            }
        }
    }
    debug_assert!((ev.t as usize) <= MAX_TEAM);
}

/// Does `deck` fulfill a companion's condition (CR 702.139a; the deck so far)?
fn companion_ok(cube: &DraftCube, cond: &DeckCond, deck: &[DefId]) -> bool {
    cube.db.deck_satisfies(cond, deck)
}

/// The object layout of this encoding for zero-core's token network
/// (`mtgz/az/layout_draft_v2.json`; a test keeps the two equal): the pack and
/// pool slots (identity digits 16, 8, 8; the present flag after the card
/// code); `id_actions: 0` = a slot scores the action of its card's identity
/// (zero-core's `TokenNet`, ENGINE.md 45.4: a pack slot its pick, a pool slot
/// adding its card to the deck next); the 13 bags; everything else globals.
/// `cards`: the name behind each identity (analysis only).
pub fn layout_json() -> String {
    use serde_json::json;
    let cube = cube().expect("the cube");
    let mut cards: Vec<Option<String>> = vec![None; identity::D];
    for i in 0..identity::CUBE_CAP {
        if cube.def[i] != DefId::MAX {
            cards[i] = Some(cube.name(i as u16).to_string());
        }
    }
    let id = json!({"at": 0, "radix": [16, 8, 8]});
    let v = json!({
        "version": 2,
        "game": "team_draft",
        "features": NUM_FEATURES,
        "vocab": identity::D,
        "segments": [
            {"name": "pack", "offset": O_PACK, "count": MAX_PACK, "width": PACK_W, "present": CODE + pack::PRESENT,
             "id": id.clone(), "actions": [], "id_actions": 0},
            {"name": "pool", "offset": O_POOL, "count": MAX_POOL, "width": POOL_W, "present": CODE + pool::PRESENT,
             "id": id, "actions": [], "id_actions": 0},
        ],
        "bags": [{"name": "draft", "offset": O_BAGS, "count": BAGS, "width": BAG_W}],
        "cards": cards,
    });
    serde_json::to_string_pretty(&v).unwrap() + "\n"
}
