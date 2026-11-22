//! Near-tie branched games (zero-core's near-tie branch rule, docs/search.md 2.11; ENGINE.md 233): the game side of
//! the two `SearchGame` hooks.
//!
//! * **`merge_key`**: a branch replayed from a near-tie decision with the runner-up forced ends where it reaches a
//!   state whose key equals one its game reached after the branch point, and its rows take the game's outcome. Equal
//!   keys must mean equivalent states - every player's information state the same, the hidden cards the same in
//!   distribution - so a false merge never labels rows with another future's outcome; a missed merge only costs
//!   compute, so every case this module cannot key safely is None.
//! * **`branch_alt_ok`**: whether a runner-up is a real alternative to the move played (not the same card doing the
//!   same thing, not a mana or trigger-order detail).
//!
//! **The key** is a stable 128-bit hash ([`StableHash`]: SplitMix64 lanes over little-endian words, no `HashMap`, no
//! process state) of:
//! 1. both seats' descriptions (`build::describe_for_key`, ENGINE.md 95's `describe` made finer: id-free, cards by
//!    name, objects by place, timing by turn, every known library position and who knows it, hand cards known to the
//!    opponent, counters, the stack, turn, step, priority and passes, clouds, plus the age of stale references, the
//!    turn's last known information and loop actions). A position `describe` refuses (a decision inside a cast or a
//!    resolution, a private peek of ENGINE.md 192, the rare states it does not describe) has no key;
//! 2. the hidden parts as multisets: each library's cards at the positions nobody knows (their order is not keyed:
//!    nobody knows it, so under the engine's knowledge model - the one `determinize` deals from, ENGINE.md 10 - it
//!    is a uniform arrangement in both states), each hand's cards with who knows each, the sideboards, the hidden
//!    decks' known pools, and the hidden decklists' sightings in the order their deal reads them (ENGINE.md 140.9);
//! 3. the match: format, game, results, wins, who chose and started, the reveals of this game and the earlier ones,
//!    each seat's deck, the match's settings that differ by seat (keep, dealt, table);
//! 4. the decision: who moves, its tag, its legal actions;
//! 5. both seats' whole layout v7 rows (`tail_encode::Row`, bit for bit), as a guard: anything the network reads
//!    that differs stops a merge even where the description would miss it.
//!
//! It leaves out the RNG states (the game's and the match's: future chance, uniform in both lines), object ids and
//! incarnations, the log, the counters that only number things (timestamps beyond their order, stack ids, the event
//! groups, the decisions answered, the sightings' clock) and the true order of library cards nobody knows. The
//! graveyards keep their order (the description's; the safe side - no card of the merge pools reads it, but
//! dropping it would need a pool check). No key (None): sideboarding, the deck choice, between or after games, a
//! position-lab game, a game over, a determinized world, a forced-find spec (ENGINE.md 180: its records are not
//! keyed), a library search's finds the other player tells apart (ENGINE.md 140.9, 223), a loop action's
//! activation in progress, and everything `describe` refuses.
//!
//! Keys compare states of one match (a branch and its game): the match's fixed parts (the pools, the spec) are not
//! keyed.

use super::MtgAz;
use crate::db::AbilitySlot;
use crate::decision::{act, Choice, DecisionKind};
use crate::game::Game;
use crate::ids::{CardIdx, Target};
use crate::rng::split64;
use zero_search::game::SearchGame;

/// The key's layout version: bump it when what is keyed changes (keys of different versions never meet - a key is
/// compared within one process - but a stored key would then say which).
const VERSION: u64 = 1;

/// A stable 128-bit hash: two SplitMix64 lanes over 64-bit words (little-endian bytes), the length mixed in at the
/// end. The same value on every platform and in every process for the same words.
pub struct StableHash {
    a: u64,
    b: u64,
    n: u64,
}

impl Default for StableHash {
    fn default() -> Self {
        Self::new()
    }
}

impl StableHash {
    pub fn new() -> StableHash {
        StableHash { a: 0x6A09_E667_F3BC_C908, b: 0xBB67_AE85_84CA_A73B, n: 0 }
    }

    pub fn word(&mut self, w: u64) {
        self.a = split64(self.a ^ w);
        self.b = split64(self.b.rotate_left(23) ^ w ^ 0x3C6E_F372_FE94_F82B);
        self.n = self.n.wrapping_add(1);
    }

    /// A byte string: its length, then its bytes in words.
    pub fn bytes(&mut self, s: &[u8]) {
        self.word(s.len() as u64);
        for c in s.chunks(8) {
            let mut x = [0u8; 8];
            x[..c.len()].copy_from_slice(c);
            self.word(u64::from_le_bytes(x));
        }
    }

    pub fn finish(mut self) -> u128 {
        let n = self.n;
        self.word(n);
        let hi = split64(self.a ^ self.b.rotate_left(32));
        let lo = split64(self.b ^ self.a.rotate_left(17) ^ 0xA54F_F53A_5F1D_36F1);
        ((hi as u128) << 64) | lo as u128
    }
}

/// The merge key of `a`'s position (module docs); None where it cannot be keyed safely.
pub fn merge_key(a: &MtgAz) -> Option<u128> {
    let (g, m) = (&a.g, &a.m);
    if m.sb.is_some() || m.choice.is_some() || m.between || m.over || a.lab.is_some() {
        return None;
    }
    if g.outcome.is_some() || g.observer.is_some() || g.spec.forced_find.is_some() {
        return None;
    }
    // (The payment search's transient wild pool, ENGINE.md 231.4, 236: empty outside the search - anything else is not a
    // state the key knows. A locked cast cost is refused by `describe`.)
    if g.pool_wild.iter().any(|w| w.iter().any(|&n| n != 0)) || !g.cast_lock.is_empty() {
        return None;
    }
    if !g.found.is_empty() || !g.finds_told.is_empty() || g.loop_open.is_some() {
        return None;
    }
    let d0 = super::build::describe_for_key(g, 0).ok()?;
    let d1 = super::build::describe_for_key(g, 1).ok()?;
    let mut h = StableHash::new();
    h.word(VERSION);
    h.bytes(d0.to_string().as_bytes());
    h.bytes(d1.to_string().as_bytes());
    hidden(g, &mut h);
    // the match
    h.word(u64::MAX - 1);
    h.word(matches!(m.format, super::matches::Format::Bo3) as u64);
    h.word(m.game_no as u64);
    m.results.iter().chain(&m.wins).for_each(|&x| h.word(x as u64));
    m.choosers.iter().chain(&m.starters).for_each(|&x| h.word(x as u64));
    h.word(m.draws as u64);
    h.word(m.turns_before as u64);
    for p in 0..2 {
        m.seen[p].iter().chain(&m.earlier[p]).for_each(|&x| h.word(x));
        m.earlier_by_game[p].iter().flatten().for_each(|&x| h.word(x));
        h.word(u64::MAX - 2);
        m.decks[p].iter().for_each(|&x| h.word(x as u64));
        h.word(u64::MAX - 3);
        h.word(m.keep[p] as u64 | (m.table[p].is_some() as u64) << 1 | (m.deck_priors[p].is_some() as u64) << 2 | (m.is_dealt(p as u8) as u64) << 3);
    }
    h.word(m.keep_known as u64 | (m.post_board as u64) << 1 | (m.constructed_bo3 as u64) << 2 | (m.game_episodes as u64) << 3 | (m.priors.is_some() as u64) << 4);
    h.word(m.pairing.as_ref().map_or(u64::MAX, |p| (p.decks[0] as u64) << 8 | p.decks[1] as u64));
    // the decision
    h.word(u64::MAX - 4);
    h.word(a.to_move().map_or(9, |p| p as u64));
    h.word(a.decision_tag() as u64);
    let mut legal = Vec::new();
    a.legal_actions(&mut legal);
    legal.iter().for_each(|&x| h.word(x as u64));
    // both rows, bit for bit (the guard): the head, the sealed tail tokens, the decider's pointer overrides
    let mut head = vec![0.0f32; super::encode::NUM_FEATURES];
    let mut tail = zero_search::tail::TailBuf::new(&super::tail_encode::TAILS);
    let mut ov = Vec::new();
    for v in 0..2 {
        h.word(u64::MAX - 5);
        head.iter_mut().for_each(|x| *x = 0.0);
        tail.clear();
        a.encode_row(v, &mut head, &mut tail);
        tail.seal();
        floats(&mut h, &head);
        h.word(tail.seg().len() as u64);
        for (&sg, &sl) in tail.seg().iter().zip(tail.slot()) {
            h.word((sg as u64) << 32 | sl as u64);
        }
        floats(&mut h, tail.val());
        if a.to_move() == Some(v) {
            ov.clear();
            a.option_overrides(v, &legal, &mut ov);
            ov.iter().for_each(|&(i, x)| {
                h.word(i as u64);
                h.word(x as u64);
            });
        }
    }
    Some(h.finish())
}

/// Floats bit for bit, two a word.
fn floats(h: &mut StableHash, xs: &[f32]) {
    h.word(xs.len() as u64);
    for pair in xs.chunks(2) {
        h.word(pair[0].to_bits() as u64 | (pair.get(1).map_or(0, |x| x.to_bits()) as u64) << 32);
    }
}

/// The hidden parts (module docs, item 2): each library's cards at the positions nobody knows (a multiset), each
/// hand's cards with who knows each (a multiset; the descriptions hold them too), the sideboards, the known pools
/// and the sightings as the hidden deck's deal reads them (per viewer and owner, by definition, then in the order
/// seen: `HiddenDeal::seen_of`).
fn hidden(g: &Game, h: &mut StableHash) {
    for p in 0..2usize {
        let pl = &g.players[p];
        let mut lib: Vec<u64> = pl.library.iter().filter(|&&c| g.objs[c.i()].known & 0b11 == 0).map(|&c| g.ident(c) as u64).collect();
        lib.sort_unstable();
        h.word(u64::MAX - 10);
        lib.into_iter().for_each(|x| h.word(x));
        let mut hand: Vec<u64> = pl.hand.iter().map(|&c| (g.ident(c) as u64) << 2 | (g.objs[c.i()].known & 0b11) as u64).collect();
        hand.sort_unstable();
        h.word(u64::MAX - 11);
        hand.into_iter().for_each(|x| h.word(x));
        h.word(u64::MAX - 12);
        pl.sideboard.iter().for_each(|&d| h.word(d as u64));
        h.word(u64::MAX - 13);
        if let Some(k) = &g.known_pools[p] {
            h.word(k.len() as u64);
            k.iter().for_each(|&d| h.word(d as u64));
        }
    }
    h.word(u64::MAX - 14);
    h.word(g.sightings_lost as u64);
    let mut s: Vec<&crate::hidden::Sighting> = g.sightings.iter().collect();
    s.sort_by_key(|x| (x.viewer, x.owner, x.def, x.at));
    for x in s {
        h.word((x.viewer as u64) << 56 | (x.owner as u64) << 48 | (x.def as u64) << 16 | (x.searched as u64) << 1 | x.grows as u64);
        h.word(x.a);
        h.word(x.b);
        h.word(x.x as i64 as u64);
    }
}

/// Whether `alt` is a real alternative to `played` at the pending decision (module docs): false for the same
/// action, for decisions that are not worth a branch - the mana payment's (`PayMana`, `PayPool`), a mana colour
/// (`ChooseMana`), activating a mana ability (`ActivateMana`, and while a spell is cast, `CastMana`: CR 601.2g,
/// ENGINE.md 231), which mana ability or how much X (a `ChooseMode` those ask), the order of triggers
/// (`OrderTriggers`) - for opening
/// the mana-ability window at priority ("Activate mana abilities") or tapping a permanent for mana there, and for
/// two options that are the same card doing the same thing (`equivalent`). True otherwise, including every
/// sideboarding and deck-choice option.
pub fn branch_alt_ok(a: &MtgAz, played: u16, alt: u16) -> bool {
    if played == alt {
        return false;
    }
    if a.m.sb.is_some() || a.m.choice.is_some() {
        return true;
    }
    let g = &a.g;
    let Some(d) = g.pending_decision() else { return true };
    if matches!(
        d.kind,
        DecisionKind::PayMana | DecisionKind::PayPool | DecisionKind::ChooseMana | DecisionKind::ActivateMana | DecisionKind::CastMana | DecisionKind::OrderTriggers
    ) {
        return false;
    }
    // (Which of a permanent's mana abilities, or how much of a mana ability's X: a mode asked by the mana-ability
    // window, the 601.2g step or a mana ability's resolution - a mana detail too.)
    if d.kind == DecisionKind::ChooseMode
        && matches!(g.agenda.last(), Some(crate::flow::Task::ActivateMana { .. } | crate::flow::Task::CastMana { .. } | crate::flow::Task::ManaNow { .. }))
    {
        return false;
    }
    let choice = |x: u16| d.options.iter().find(|o| o.action == x).map(|o| o.choice);
    let (Some(cp), Some(ca)) = (choice(played), choice(alt)) else { return true };
    if d.kind == DecisionKind::Priority && (mana_entry(g, played, cp) || mana_entry(g, alt, ca)) {
        return false;
    }
    !equivalent(g, cp, ca)
}

/// At priority: opening the mana-ability window (`YES`, "Activate mana abilities", ENGINE.md 100.4.3) or activating
/// a permanent's mana ability (a float, "Tap X for mana").
fn mana_entry(g: &Game, action: u16, c: Choice) -> bool {
    match c {
        Choice::Yes => action == act::YES,
        Choice::Ability(i, AbilitySlot::Activated(k)) => g.def_of(i).activated.get(k as usize).is_some_and(|ad| ad.mana().is_some()),
        _ => false,
    }
}

/// Two options that are the same card doing the same thing: the same kind of choice of twin objects (`twins`), with
/// the same ability slot or face bits.
fn equivalent(g: &Game, x: Choice, y: Choice) -> bool {
    use Choice as C;
    match (x, y) {
        (C::Card(p), C::Card(q)) | (C::Play(p), C::Play(q)) | (C::Cast(p), C::Cast(q)) => twins(g, p, q),
        (C::CastFace(p), C::CastFace(q)) | (C::PlayFace(p), C::PlayFace(q)) => twins(g, p, q),
        (C::Faces(p, m), C::Faces(q, n)) => m == n && twins(g, p, q),
        (C::Ability(p, s), C::Ability(q, t)) => s == t && twins(g, p, q),
        (C::Target(Target::Obj(r)), C::Target(Target::Obj(s))) => g.live(r) && g.live(s) && twins(g, r.idx, s.idx),
        _ => false,
    }
}

/// Two objects no rule tells apart: the same card (definition and identity) with every object field equal but the
/// incarnation and the timestamp (two untapped Volcanic Islands, two Lightning Bolts in a hand that the opponent
/// knows alike), the same counters, and neither named by an effect, a mark, a link, an attachment or a stack item
/// (where it would be told apart, they count as different: a missed exclusion only costs a branch).
fn twins(g: &Game, p: CardIdx, q: CardIdx) -> bool {
    if p == q {
        return true;
    }
    let (mut x, mut y) = (g.objs[p.i()], g.objs[q.i()]);
    x.inc = 0;
    y.inc = 0;
    x.timestamp = 0;
    y.timestamp = 0;
    if x != y || g.ident(p) != g.ident(q) || g.counters_of(p) != g.counters_of(q) {
        return false;
    }
    let named = |c: CardIdx| {
        g.effects.iter().any(|e| matches!(e.on, crate::layers::Affected::Obj(r) if r.idx == c))
            || g.marks.iter().any(|(r, _)| r.idx == c)
            || g.links.iter().any(|l| l.source.idx == c || l.card.idx == c)
            || g.battlefield.iter().any(|&b| g.objs[b.i()].attached.is_some_and(|r| r.idx == c))
            || g.stack.iter().any(|s| s.targets.iter().any(|(_, t)| matches!(t, Target::Obj(r) if r.idx == c)))
    };
    !named(p) && !named(q)
}
