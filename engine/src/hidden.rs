//! Chance keys, knowledge and `determinize` (ENGINE.md 10).
//!
//! Knowledge has two layers. Exact: `Obj.known` bit p = player p knows the
//! card's identity where it is. Partial ([`Cloud`]): after a private
//! rearrangement (Brainstorm's put-back, a scry, a search, a shuffle), a
//! viewer who knew some of the cards involved knows only that those
//! identities are somewhere among a set of cards. `determinize` keeps both:
//! exactly known cards stay; a cloud's identities stay inside its cards.
//! The rules below never claim knowledge a player does not have (no leak),
//! and keep everything they do have that fits these two forms.

use crate::game::Game;
use crate::ids::{CardIdx, DefId, ObjRef, PlayerId, Zone};
use crate::rng::Rng;
use crate::svec::SVec;

/// A card was drawn (private to the drawer). Payload: its definition.
pub const KEY_DRAW: u64 = 1;
/// A card's identity became known to everyone: it moved from a hidden zone
/// to a public one, or was revealed (a hand reveal, a revealed search).
/// Payload: its definition.
pub const KEY_REVEAL: u64 = 2;
/// A player looked at a card in a hidden zone (the top of their library,
/// another player's hand): private to the looker. Payload: its definition.
pub const KEY_LOOK: u64 = 3;
/// A decision with a single option was answered by the engine (`auto_single`):
/// the player had no alternative. Payload: the decision kind (bits 16+) and
/// the action id.
pub const KEY_AUTO: u64 = 4;
/// A coin flip (CR 705): payload 1 = its player won, 0 = lost. Public.
pub const KEY_COIN: u64 = 5;
/// A die roll (CR 706): payload = the result. Public.
pub const KEY_DIE: u64 = 6;
/// A later game of a best-of-three match begins (ENGINE.md 36.11): public
/// facts the sideboarding (hidden) decided. Payload: each player's deck size
/// and number of sideboard cards (`deck0 | deck1 << 8 | side0 << 16 | side1
/// << 24`). Emitted by the match, not by a `Game`.
pub const KEY_GAME: u64 = 7;
/// A dealt seat's 60 for the next game of a best-of-three was dealt (ENGINE.md 154, the spec's
/// `"dealt_decks"`): private to that player (its player). Payload: the 60's copies per entry of
/// the public 75's list, nine 5-bit counts a key in list order, the key's index in bits 45-47
/// (`matches::deal_dealt_deck`); for another observer the same number of keys, each `HIDDEN`.
/// Emitted by the match, not by a `Game`.
pub const KEY_DEALT: u64 = 8;
/// A hand card a reference of the public state tells apart (`Game::told_apart`,
/// ENGINE.md 156) left the hand or was revealed where a hand slot or a random
/// pick could have named one of its twins instead in another world: every
/// player sees that it was that card. Public. Payload: its card index. (Kind 9: lane I's
/// branch numbered it 8, which dealt decks' `KEY_DEALT` took first - ENGINE.md 157.)
pub const KEY_TOLD: u64 = 9;
/// Payload of a private key emitted while another player is the observer.
pub const HIDDEN: u64 = 0xFFFF_FFFF;
/// The action part of another player's forced answer (`KEY_AUTO`, payload `kind << 16 | action`) that names a card
/// by a place the observer doesn't see (a hand slot, a library or pile slot), in that observer's worlds (ENGINE.md
/// 161, `Game::forced_answer_seen`): no action id is this large (`decision::NUM_ACTIONS`).
pub const MASKED_ACTION: u64 = 0xFFFF;
const _: () = assert!(MASKED_ACTION as usize >= crate::decision::NUM_ACTIONS);

/// A look at a card at random of another player's hand (`Ext::LookAtRandomHandCard`, Urza's Bauble), the one look
/// keyed public (`KEY_LOOK` with the card for every observer: its owner sees which card was seen, ENGINE.md 141.2).
/// `Game::hand_looks` holds the ones the last `apply` made, for the fuzz's private-key check (ENGINE.md 206): from
/// outside a step only the hands before and after it are visible, and a step can change the hand around the look (a
/// card drawn or returned to it, then looked at; looked at, then discarded or shuffled away; the game ended). Nothing
/// in play reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandLook {
    /// The look's key: its index among the keys that `apply` emitted.
    pub key: u32,
    /// The player who looked.
    pub looker: PlayerId,
    /// The hand's owner.
    pub owner: PlayerId,
    /// The card seen, its identity and its incarnation then (`Obj::inc`: a zone change later in the step moves it).
    pub card: CardIdx,
    pub def: DefId,
    pub inc: u32,
}

/// The deals `determinize` draws at most for a face-down choice to be a legal
/// one (ENGINE.md 123): each succeeds with a probability of at least one in
/// the hidden cards and the hand's size (the real deal is one), so this many
/// failing is a broken invariant, loudly. The uniform deal's only: that bound
/// does not hold under weights, and the belief-weighted deal draws the choice
/// with the deal, exactly and with no try count (`deal_weighted_pick`,
/// ENGINE.md 147.5).
const HIDDEN_PICK_TRIES: u32 = 1_000_000;

/// Pack a key: kind in bits 56-63, player in 48-55, payload in 0-47.
pub fn key(kind: u64, player: PlayerId, payload: u64) -> u64 {
    debug_assert!(payload < 1 << 48);
    (kind << 56) | ((player as u64) << 48) | payload
}

pub fn key_kind(k: u64) -> u64 {
    k >> 56
}
pub fn key_player(k: u64) -> PlayerId {
    ((k >> 48) & 0xFF) as PlayerId
}
pub fn key_payload(k: u64) -> u64 {
    k & ((1 << 48) - 1)
}

/// Partial knowledge (ENGINE.md 10): `viewer` knows that the identities
/// `defs` (a multiset, sorted) are among `owner`'s cards that `viewer` does
/// not know exactly in the domain: `slots` (library cards), plus the whole
/// hand if `hand`, plus the whole library if `library`. The hand is one
/// pool: its cards are not told apart by anyone but their owner, so a
/// cloud covers all of it or none. Domains of one viewer's clouds over one
/// owner are disjoint; `viewer != owner` (an owner's own knowledge is
/// always exact or nothing).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Cloud {
    pub viewer: PlayerId,
    pub owner: PlayerId,
    pub hand: bool,
    pub library: bool,
    pub slots: Vec<CardIdx>,
    pub defs: Vec<DefId>,
}

/// A sighting (ENGINE.md 140.9): `viewer` learned that `owner`'s card `obj`, hidden from it, is `def` (its card).
/// `at` orders the sightings of one game (`Game::sight_at`); a card found by its owner's library search
/// (`searched`) is ordered at its search, when it left the cards the viewer had not seen. `grows`: no cloud of the
/// viewer's could have listed it, so it is one more distinct card seen (`sample::knowledge`'s R grew); else it may
/// be a listed card seen again. Its chance given the deck under the deal's model just before it (the clouds' listed
/// cards uniform in their domains, the free cards over the free places): b (D_def - R_def) + a up to a common scale
/// - a the share of the place's listed copies (the copies a cloud lists in its domain x the free cards; with no free
/// card, the listed copies alone: ENGINE.md 162), b the place's free share (its domain's free places, or 1 outside
/// every cloud); b = 0: a chance the deck does not change. `x`: R_def before it less the sightings that had grown R (a payment's pinned cards; ENGINE.md 140.9).
/// Kept only where whole-deck clouds are (`hidden_decklists`, `deck_clouds`: R never loses a card there), and only
/// in a game no `determinize` dealt (`Game::observer`: a world's are dropped, `sightings_lost`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Sighting {
    pub viewer: PlayerId,
    pub owner: PlayerId,
    pub obj: CardIdx,
    pub def: DefId,
    pub at: u32,
    pub searched: bool,
    pub grows: bool,
    pub a: u64,
    pub b: u64,
    pub x: i32,
}

/// The rare-rule bits (`Game::seen`) of each definition of `db`, its back
/// face's included (as `Game::add_card` sets them; not the per-player
/// miracle bits); computed once.
fn seen_bits_of(db: &'static crate::db::CardDb) -> &'static [u16] {
    // (Per card table: a process may play the cube's games and sealed ones, ENGINE.md 102.6.)
    static BITS: std::sync::Mutex<Vec<(usize, &'static [u16])>> = std::sync::Mutex::new(Vec::new());
    let key = db as *const crate::db::CardDb as usize;
    let mut v = BITS.lock().unwrap();
    if let Some(&(_, b)) = v.iter().find(|(k, _)| *k == key) {
        return b;
    }
    let bits: Vec<u16> = (0..db.len())
        .map(|i| {
            let d = db.def(i as DefId);
            d.seen_bits() | d.back.map_or(0, |b| db.def(b).seen_bits())
        })
        .collect();
    let b: &'static [u16] = Box::leak(bits.into_boxed_slice());
    v.push((key, b));
    b
}

/// `k` sequential draws without replacement from the sorted multiset
/// `pool` (removed from it, which stays sorted), each card of identity d
/// with probability proportional to `w(d)` (Wallenius' urn; ENGINE.md 69.2):
/// a function of the multiset and `rng` only, never of where the cards lie.
fn weighted_draws(pool: &mut Vec<DefId>, k: usize, w: &dyn Fn(DefId) -> f64, rng: &mut Rng) -> Vec<DefId> {
    let mut out = Vec::with_capacity(k);
    let mut wt: Vec<f64> = pool.iter().map(|&d| w(d)).collect();
    debug_assert!(wt.iter().all(|x| x.is_finite() && *x > 0.0), "deal weights finite and > 0");
    for _ in 0..k.min(pool.len()) {
        let tot: f64 = wt.iter().sum();
        // (a uniform in [0, 1) from 53 bits)
        let mut x = (rng.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64) * tot;
        let mut pick = pool.len() - 1;
        for (j, &q) in wt.iter().enumerate() {
            x -= q;
            if x < 0.0 {
                pick = j;
                break;
            }
        }
        wt.remove(pick);
        out.push(pool.remove(pick));
    }
    out
}

/// A uniform in [0, 1) from 53 bits of `rng`.
fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// `weighted_draws` of `k` cards with draw `j` (0-based) a card of the kind
/// (`kind`: its identities, sorted), for `deal_weighted_pick` (ENGINE.md
/// 147.5): a card of the kind drawn first, by weight among the pool's cards
/// of the kind; the other k - 1 by `weighted_draws` from the rest; the first
/// moved to place j. Returned with the importance ratio of that draw against
/// the weighted draw of the same cards conditioned on place j being of the
/// kind: (S_K / S) prod_t S_t(first) / S_t(at j), S_t the pool's weight left
/// before draw t in the order the cards were drawn with the card of the kind
/// first and in the order returned; the cards' own weights and copies are the
/// same factors in both orders (copies left fall m, m - 1, ... per identity
/// whatever the interleaving). The ratio is at most S_K / (S - T_j), T_j the
/// pool's j heaviest cards' weight. No card of the kind in the pool: the plain
/// draws and ratio 0.
fn weighted_draws_forced(pool: &mut Vec<DefId>, k: usize, j: usize, kind: &[DefId], w: &dyn Fn(DefId) -> f64, rng: &mut Rng) -> (Vec<DefId>, f64) {
    debug_assert!(j < k && k <= pool.len());
    let s: f64 = pool.iter().map(|&d| w(d)).sum();
    let s_k: f64 = pool.iter().filter(|d| kind.binary_search(d).is_ok()).map(|&d| w(d)).sum();
    if s_k <= 0.0 {
        return (weighted_draws(pool, k, w, rng), 0.0);
    }
    let mut x = unit(rng) * s_k;
    let mut pick = None;
    for (i, &d) in pool.iter().enumerate() {
        if kind.binary_search(&d).is_ok() {
            pick = Some(i);
            x -= w(d);
            if x < 0.0 {
                break;
            }
        }
    }
    let first = pool.remove(pick.expect("a card of the kind"));
    let rest = weighted_draws(pool, k - 1, w, rng);
    // The remaining weights before each draw: card of the kind first, then at place j.
    let (mut a, mut b, mut r) = (s, s, s_k / s);
    for t in 0..k {
        let at = if t == 0 { first } else { rest[t - 1] };
        let bt = match t.cmp(&j) {
            std::cmp::Ordering::Less => rest[t],
            std::cmp::Ordering::Equal => first,
            std::cmp::Ordering::Greater => rest[t - 1],
        };
        r *= a / b;
        a -= w(at);
        b -= w(bt);
    }
    let mut out = rest;
    out.insert(j, first);
    (out, r)
}

/// `determinize_weighted`'s plan of one owner's hidden cards (`Game::weighted_plan`).
struct WeightedPlan {
    /// The slots' identities less every cloud's listed cards, sorted (the free cards).
    pool: Vec<DefId>,
    /// Per slot: its cloud (of the viewer's over the owner).
    home: Vec<Option<usize>>,
    /// Per cloud: its listed cards (`cloud_needs`).
    needs: Vec<Vec<DefId>>,
    in_hand: Vec<bool>,
    /// The hand's places outside every cloud: draws from `pool`.
    free_hand: usize,
    clouds: Vec<PlanCloud>,
}

/// A cloud's places in a `WeightedPlan`: its slots (indices into the slots, in order) and which of them are in the hand.
struct PlanCloud {
    mine: Vec<usize>,
    hand_pos: Vec<usize>,
}

/// A dealt hand place of `deal_weighted_pick`: draw `j` of the free hand places' sequence (`seq` None) or of cloud `seq`'s hand places.
#[derive(Clone, Copy, Debug)]
struct Force {
    seq: Option<usize>,
    j: usize,
}

/// Remove one `d` from a sorted multiset; false if absent.
fn take_one(v: &mut Vec<DefId>, d: DefId) -> bool {
    match v.binary_search(&d) {
        Ok(i) => {
            v.remove(i);
            true
        }
        Err(_) => false,
    }
}

fn insert_sorted(v: &mut Vec<DefId>, d: DefId) {
    let i = v.partition_point(|&x| x <= d);
    v.insert(i, d);
}

impl Game {
    /// A fact revealed to everyone.
    pub(crate) fn emit_public(&mut self, kind: u64, player: PlayerId, payload: u64) {
        self.keys.push(key(kind, player, payload));
        self.consume_forced();
    }

    /// A fact revealed only to `to`: hidden from any other observer.
    pub(crate) fn emit_private(&mut self, kind: u64, to: PlayerId, payload: u64) {
        let payload = match self.observer {
            Some(o) if o != to => HIDDEN,
            _ => payload,
        };
        self.keys.push(key(kind, to, payload));
        self.consume_forced();
    }

    // ---- forced outcomes (zero-core `force_chance`, ENGINE.md 10) ---------

    /// Queue outcomes for the next chance events (zero-core `chance_k`:
    /// replaying a key sampled in another determinized world). Every key the
    /// engine emits consumes one queued key, in order, so later keys stay
    /// aligned with their events. A draw, a look at the top of a library and
    /// a random discard follow their queued key when this world allows it
    /// (the identity is among the cards nobody knows, in the same knowledge
    /// domain); otherwise, and for every other kind (reveals of chosen
    /// cards, `KEY_AUTO`, keys masked as `HIDDEN`), the world's own outcome
    /// happens. The emitted key is always the actual outcome. `&[]` clears
    /// the queue.
    pub fn force_chance(&mut self, keys: &[u64]) {
        self.forced.clear();
        self.forced.extend_from_slice(keys);
        self.forced_at = 0;
    }

    fn consume_forced(&mut self) {
        if self.forced_at < self.forced.len() {
            self.forced_at += 1;
        }
    }

    /// The queued key the next emitted key will consume, if it is of this
    /// kind and player and names a definition.
    pub(crate) fn forced_def(&self, kind: u64, player: PlayerId) -> Option<DefId> {
        let k = *self.forced.get(self.forced_at)?;
        let d = key_payload(k);
        (key_kind(k) == kind && key_player(k) == player && d != HIDDEN && (d as usize) < self.db.len())
            .then_some(d as DefId)
    }

    /// Before library card `c` is drawn or looked at (keyed `kind` for
    /// `player`): if the next queued key names another identity, swap it in
    /// from a card that holds it and is interchangeable with `c` for every
    /// player (both unknown to everyone, the same cloud domains), so every
    /// player's knowledge still holds in the changed world.
    pub(crate) fn force_library_card(&mut self, c: CardIdx, kind: u64, player: PlayerId) {
        let Some(d) = self.forced_def(kind, player) else { return };
        let o = &self.objs[c.i()];
        if o.def == d || o.zone != Zone::Library || o.known != 0 {
            return;
        }
        let owner = o.owner;
        let homes = |g: &Game, x: CardIdx| -> Vec<bool> {
            g.clouds.iter().map(|k| Self::in_domain_at(k, owner, x, Zone::Library)).collect()
        };
        let mine = homes(self, c);
        let cands: Vec<CardIdx> = self.players[owner as usize]
            .library
            .iter()
            .copied()
            .filter(|&x| x != c && self.objs[x.i()].def == d && self.objs[x.i()].known == 0)
            .filter(|&x| homes(self, x) == mine)
            .collect();
        if cands.is_empty() {
            return;
        }
        let x = cands[self.rng.below(cands.len() as u32) as usize];
        let (a, b) = (self.objs[c.i()].def, self.objs[x.i()].def);
        self.objs[c.i()].def = b;
        self.objs[x.i()].def = a;
    }

    /// A random discard from `p`'s hand (`hand` in canonical order): the
    /// card the next queued reveal names, if the hand holds one.
    /// Hand cards of one definition are interchangeable (ENGINE.md 10):
    /// which of them another player knows is not information. Their
    /// knowledge bits are normalised so the better-known come first in
    /// canonical order (by definition, then card), making the state, and
    /// what an encoding reads per hand slot, independent of which physical
    /// card a sampled world dealt where. But a hand card a reference of the
    /// public state names (`told_apart`: a card revealed for miracle is
    /// played revealed until its ability resolves, CR 702.94b; an ability's
    /// source revealed by its activation, 602.2a, 701.20a; ENGINE.md 156) is
    /// told apart from its twins: it keeps its bits, or a world `determinize`
    /// deals for another player could give that slot another identity while
    /// the ability still acts on it (ENGINE.md 71.3), and the reference would
    /// show what one world's viewer knows of it and not another's.
    pub(crate) fn canon_hand_knowledge(&mut self) {
        for p in 0..2u8 {
            if self.players[p as usize].hand.len() < 2 {
                continue;
            }
            let h = self.hand_sorted_in(p);
            let mut i = 0;
            while i < h.len() {
                let d = self.objs[h[i].i()].def;
                let mut j = i + 1;
                while j < h.len() && self.objs[h[j].i()].def == d {
                    j += 1;
                }
                if j - i > 1 {
                    let twins: SVec<CardIdx, 16> = h[i..j].iter().copied().filter(|&c| !self.told_apart(c)).collect();
                    let mut bits: SVec<u8, 16> = twins.iter().map(|c| self.objs[c.i()].known).collect();
                    bits.sort_unstable_by(|a, b| b.cmp(a));
                    for (k, &c) in twins.iter().enumerate() {
                        self.objs[c.i()].known = bits[k];
                    }
                }
                i = j;
            }
        }
    }

    /// Every object reference the public state holds (ENGINE.md 156), the
    /// ones layout v7's encoding writes as references or reads an identity
    /// through: the waiting triggers' sources and event objects, each stack
    /// item's (an ability's source, its event's objects, its targets, its
    /// noted objects, a ninjutsu creature's planeswalker), the delayed
    /// triggers' sources and event objects, the temporary triggers' sources
    /// and objects, `links` (both ends) and the marks (the marked object and
    /// the objects a mark names). Live or stale (`f` sees each as it is).
    /// A new holder of references the encoding reads must be listed here:
    /// `tail_encode::Cx::ref_def` refuses a live reference to a hand card
    /// that no listed reference tells apart (`told_apart`).
    pub(crate) fn for_each_public_ref(&self, mut f: impl FnMut(ObjRef) -> bool) -> bool {
        let ev = |e: &crate::triggers::EvCtx, f: &mut dyn FnMut(ObjRef) -> bool| e.obj.is_some_and(&mut *f) || e.obj2.is_some_and(&mut *f);
        for w in &self.waiting {
            if f(w.source) || ev(&w.ev, &mut f) {
                return true;
            }
        }
        for s in &self.stack {
            if let crate::game::StackKind::Ability { source, .. } = s.kind {
                if f(source) {
                    return true;
                }
            }
            if ev(&s.ev, &mut f) || s.noted.iter().any(|&r| f(r)) || matches!(s.attack_as, Some(Some(pw)) if f(pw)) {
                return true;
            }
            if s.targets.iter().any(|&(_, t)| matches!(t, crate::ids::Target::Obj(r) if f(r))) {
                return true;
            }
        }
        for d in &self.delayed {
            if f(d.source) || ev(&d.ev, &mut f) {
                return true;
            }
        }
        for t in &self.temp_triggers {
            if f(t.source) || t.obj.is_some_and(&mut f) {
                return true;
            }
        }
        for l in &self.links {
            if f(l.source) || f(l.card) {
                return true;
            }
        }
        // ENGINE.md 142.7: the `effects` tokens' references - an effect's object and the object
        // "for as long as" names, a copy effect's permanent.
        for e in &self.effects {
            if matches!(e.on, crate::layers::Affected::Obj(r) if f(r)) || matches!(e.until, crate::layers::Until::WhileOnBattlefield(r) if f(r)) {
                return true;
            }
        }
        if self.copies.iter().any(|c| f(c.obj)) {
            return true;
        }
        for &(o, m) in &self.marks {
            let named = match m {
                crate::game::Mark::Reanimated(x) | crate::game::Mark::ExiledToPay(x) => Some(x),
                crate::game::Mark::Triggered { src, .. } => Some(src),
                crate::game::Mark::AltGranted { granter, .. } => Some(granter),
                _ => None,
            };
            if f(o) || named.is_some_and(&mut f) {
                return true;
            }
        }
        false
    }

    /// Is hand card `c` told apart from its twins (ENGINE.md 128, 156): does
    /// a reference of the public state (`for_each_public_ref`) name it, live?
    /// A card an ability's activation revealed in the hand stays revealed
    /// while the ability is on the stack (CR 602.2a, 701.20a: cycling's
    /// source while its costs are paid, ninjutsu's card, 702.49b), a card
    /// revealed for miracle until its ability leaves the stack (702.94b), a
    /// card promised to a delve is revealed (ENGINE.md 100.4.9): the
    /// reference follows that one object, so which twin it is, and what each
    /// player knows of it, is not interchangeable with its twins'
    /// (`canon_hand_knowledge`, `knowledge_leaves`, `Game::hand_slot_order_in`
    /// leave it alone). A stale reference names an object that is gone (CR
    /// 400.7): the card that is now in the hand is a new object, as
    /// interchangeable as any.
    pub(crate) fn told_apart(&self, c: CardIdx) -> bool {
        let o = &self.objs[c.i()];
        o.zone == Zone::Hand && self.for_each_public_ref(|r| r.idx == c && r.inc == o.inc)
    }

    /// Key that told-apart hand card `c` is the one a hand slot (`random`:
    /// a random pick) names as it leaves the hand or is revealed (`KEY_TOLD`,
    /// ENGINE.md 156). A slot names a definition's cards the other player
    /// knows first, a told-apart one first among those (`hand_slot_order_in`),
    /// so a known one is the same card in every world of the other player's:
    /// no key needed. One it doesn't know may be this card in one world and
    /// a twin in another, as may any card a random pick takes; every player
    /// sees which it was (the reference follows it), and the key keeps those
    /// worlds apart.
    pub(crate) fn key_told(&mut self, c: CardIdx, random: bool) {
        let o = &self.objs[c.i()];
        if o.zone == Zone::Hand && (random || o.known & (1 << crate::ids::opp(o.owner)) == 0) && self.told_apart(c) {
            let owner = o.owner;
            self.emit_public(KEY_TOLD, owner, c.get() as u64);
        }
    }

    /// The last known identity `viewer` has of the gone object `r` (a stale
    /// reference; CR 400.7, 608.2h): what it was as it left its zone, if
    /// `viewer` knew that then (`Game::ref_lki`; ENGINE.md 156).
    pub(crate) fn lki_identity(&self, viewer: PlayerId, r: ObjRef) -> Option<DefId> {
        self.ref_lki.iter().rev().find(|x| x.0 == r).filter(|x| x.2 >> viewer & 1 != 0).map(|x| x.1)
    }

    /// At a turn's start: the departures no reference of the public state
    /// names any more are forgotten (`Game::ref_lki`).
    pub(crate) fn prune_ref_lki(&mut self) {
        if self.ref_lki.is_empty() {
            return;
        }
        let mut named: Vec<ObjRef> = Vec::new();
        self.for_each_public_ref(|r| {
            if !self.live(r) {
                named.push(r);
            }
            false
        });
        self.ref_lki.retain(|x| named.contains(&x.0));
    }

    /// The queued outcome of a coin flip by `p` (`force_chance`), if the
    /// next queued key is one.
    pub(crate) fn forced_coin(&self, p: PlayerId) -> Option<bool> {
        let k = *self.forced.get(self.forced_at)?;
        (key_kind(k) == KEY_COIN && key_player(k) == p).then(|| key_payload(k) == 1)
    }

    pub(crate) fn forced_hand_pick(&self, p: PlayerId, hand: &[CardIdx]) -> Option<CardIdx> {
        let d = self.forced_def(KEY_REVEAL, p)?;
        hand.iter().copied().find(|&c| self.objs[c.i()].def == d)
    }

    /// A card of `p`'s hand at random (CR 701.9b; `hand` in canonical
    /// order): the card the next queued reveal names if the hand holds one
    /// (`forced_hand_pick`), else a uniform draw. A told-apart card is keyed
    /// (`key_told`: in another world the draw may take one of its twins).
    pub(crate) fn random_hand_card(&mut self, p: PlayerId, hand: &[CardIdx]) -> CardIdx {
        let c = match self.forced_hand_pick(p, hand) {
            Some(c) => c,
            None => hand[self.rng.below(hand.len() as u32) as usize],
        };
        self.key_told(c, true);
        c
    }

    /// Does `viewer` know the identity of this card exactly?
    pub fn knows(&self, viewer: PlayerId, idx: CardIdx) -> bool {
        let o = &self.objs[idx.i()];
        let hidden = o.zone.is_hidden() || (!self.face_down_ids.is_empty() && o.def == self.db.face_down_def());
        !hidden || o.known & (1 << viewer) != 0
    }

    /// Is `c`'s identity hidden where it is: in a hidden zone, or a
    /// face-down permanent (CR 708; ENGINE.md 31)?
    #[inline]
    pub(crate) fn hidden_obj(&self, c: CardIdx) -> bool {
        let o = &self.objs[c.i()];
        o.zone.is_hidden() || (!self.face_down_ids.is_empty() && o.def == self.db.face_down_def())
    }

    /// The partial knowledge in the game (tests, analysis).
    pub fn clouds(&self) -> &[Cloud] {
        &self.clouds
    }

    /// Cards whose identity `viewer` does not know, per owner: the other
    /// player's hand and both libraries, minus known cards.
    pub fn unknown_to(&self, viewer: PlayerId, owner: PlayerId) -> Vec<CardIdx> {
        self.unknown_to_in(viewer, owner).to_vec()
    }

    /// [`unknown_to`](Self::unknown_to) in inline storage (no allocation for
    /// up to 64 cards; ENGINE.md 17).
    fn unknown_to_in(&self, viewer: PlayerId, owner: PlayerId) -> SVec<CardIdx, 64> {
        let pl = &self.players[owner as usize];
        let hand = if owner != viewer { &pl.hand[..] } else { &[][..] };
        // (Cards exiled face down: hidden too, ENGINE.md 30; face-down
        // permanents, 31.)
        let face_down = self.face_down.iter().copied().filter(|&c| self.objs[c.i()].owner == owner);
        let fd_perms = self.face_down_ids.iter().map(|x| x.0).filter(|&c| self.objs[c.i()].owner == owner);
        hand.iter().chain(pl.library.iter()).copied().chain(face_down).chain(fd_perms).filter(|&c| !self.knows(viewer, c)).collect()
    }

    // ---- knowledge events (ENGINE.md 10) ----------------------------------

    /// Is `c` (in hidden zone `zone`) in the domain of cloud `k`?
    fn in_domain_at(k: &Cloud, owner: PlayerId, c: CardIdx, zone: Zone) -> bool {
        k.owner == owner
            && ((k.hand && zone == Zone::Hand) || (k.library && zone == Zone::Library) || k.slots.contains(&c))
    }

    /// The domain's cards (hidden, the owner's), known or not.
    fn domain(&self, k: &Cloud) -> Vec<CardIdx> {
        let pl = &self.players[k.owner as usize];
        let mut v: Vec<CardIdx> = Vec::new();
        if k.hand {
            v.extend_from_slice(&pl.hand);
        }
        if k.library {
            v.extend_from_slice(&pl.library);
        }
        for &c in &k.slots {
            let o = &self.objs[c.i()];
            if o.owner == k.owner && self.hidden_obj(c) && !v.contains(&c) {
                v.push(c);
            }
        }
        v
    }

    /// `q` learns `c`'s identity exactly (a look, a reveal) while it is in a
    /// hidden zone. A cloud whose domain holds `c` and lists its identity
    /// loses that entry (the listed card may be this one).
    pub(crate) fn learn(&mut self, q: PlayerId, c: CardIdx) {
        // (A look of a controlled player is its controller's too, CR 722.3:
        // also a look at a card it knew already; ENGINE.md 100.5.)
        if self.turn_controller.is_some() {
            self.controller_sees(q, c);
        }
        let o = &self.objs[c.i()];
        if o.known & (1 << q) != 0 || !self.hidden_obj(c) {
            return;
        }
        let (owner, zone, d) = (o.owner, o.zone, self.ident(c));
        let terms = self.sight_terms(q, owner, c, zone, d);
        let mut merged = false;
        for k in self.clouds.iter_mut().filter(|k| k.viewer == q) {
            if Self::in_domain_at(k, owner, c, zone) {
                merged |= take_one(&mut k.defs, d);
                k.slots.retain(|&x| x != c);
            }
        }
        self.objs[c.i()].known |= 1 << q;
        // (A listed card the viewer may be seeing again is not a new one, and its chance is the mixture: 140.9.)
        self.sighted(q, owner, c, d, !merged, terms);
        if q != owner {
            self.untrack_found(c);
        }
        self.tidy_clouds();
    }

    // ---- sightings (ENGINE.md 140.9) --------------------------------------

    /// Are sightings kept: where whole-deck clouds are, so every card seen stays in R (ENGINE.md 102.1, 120.2).
    fn keeps_sightings(&self) -> bool {
        self.spec.hidden_decklists || self.spec.deck_clouds
    }

    /// `q` sees `owner`'s card `c` (identity `d`), hidden from it: a sighting, ordered at its owner's search if it was
    /// found by one and the viewer could tell it apart since (`found`), else now; `grows` and `terms`
    /// (`sight_terms`, taken before the knowledge changed) as `Sighting` says. In a world (`observer`) nothing is
    /// kept and the world's sightings are lost (a deal from it weighs every seen card blind).
    fn sighted(&mut self, q: PlayerId, owner: PlayerId, c: CardIdx, d: DefId, grows: bool, terms: Option<(u64, u64, i32)>) {
        if q == owner || !self.keeps_sightings() {
            return;
        }
        let Some((a, b, x)) = terms else {
            self.sightings_lost = true;
            return;
        };
        let def = self.db.def(d).card_def(d);
        let (at, searched) = match self.found.iter().find(|f| f.0 == c) {
            Some(&(_, t)) => (t, true),
            None => {
                let t = self.sight_at;
                self.sight_at += 1;
                (t, false)
            }
        };
        self.sightings.push(Sighting { viewer: q, owner, obj: c, def, at, searched, grows, a, b, x });
    }

    /// The terms of `q`'s sighting of `owner`'s card `c` (identity `d`, in hidden zone `zone`) under the deal's model
    /// as the knowledge stands before it (`Sighting`): the deal's places (`hidden_slots`, `c` among them), each in the
    /// domain of at most one cloud, each cloud's listed copies (`cloud_needs`'s rule: a pinned card in the domain
    /// fulfils its identity, at most the domain's room), the free cards F = places - listed copies; `c`'s place in
    /// cloud k of room m holding n listed cards, cnt of them `d`'s card: a = cnt F, b = m - n (with F = 0 every domain
    /// is full and the chance is cnt / m: a = cnt, b = 0, ENGINE.md 162; outside every cloud a = 0, b = 1; a pinned
    /// `c`: fixed, b = 0); `x` = R_d (the deck's cards not among the places - known or pinned -,
    /// the listed copies, less the cards outside the deck among the places) less the grown sightings of `d`'s card
    /// so far. None where nothing is kept (`sighted`).
    fn sight_terms(&self, q: PlayerId, owner: PlayerId, c: CardIdx, zone: Zone, d: DefId) -> Option<(u64, u64, i32)> {
        if q == owner || !self.keeps_sightings() || self.observer.is_some() {
            return None;
        }
        let db = self.db;
        let def = db.def(d).card_def(d);
        let card = |x: CardIdx| {
            let e = self.face_down_id(x).unwrap_or_else(|| self.own_def(x));
            db.def(e).card_def(e)
        };
        let (slots0, pinned) = self.hidden_slots(q, owner);
        let mut slots: Vec<CardIdx> = slots0.to_vec();
        let fixed = pinned.contains(&c);
        if !fixed && !slots.contains(&c) {
            slots.push(c);
        }
        let zone_of = |x: CardIdx| if x == c { zone } else { self.objs[x.i()].zone };
        let clouds: Vec<&Cloud> = self.clouds.iter().filter(|k| k.viewer == q && k.owner == owner).collect();
        let home: Vec<Option<usize>> = slots.iter().map(|&x| clouds.iter().position(|k| Self::in_domain_at(k, owner, x, zone_of(x)))).collect();
        let mut needs: Vec<Vec<DefId>> = Vec::with_capacity(clouds.len());
        for (ki, k) in clouds.iter().enumerate() {
            let mut need = k.defs.clone();
            for &p in &pinned {
                if Self::in_domain_at(k, owner, p, self.objs[p.i()].zone) {
                    take_one(&mut need, self.objs[p.i()].def);
                }
            }
            need.truncate(home.iter().filter(|&&h| h == Some(ki)).count());
            needs.push(need);
        }
        let free = slots.len() - needs.iter().map(|n| n.len()).sum::<usize>();
        let (a, b) = if fixed {
            (1, 0)
        } else {
            let i = slots.iter().position(|&x| x == c).expect("the seen card is one of the deal's places");
            match home[i] {
                None => (0, 1),
                Some(k) => {
                    let room = home.iter().filter(|&&h| h == Some(k)).count();
                    let cnt = needs[k].iter().filter(|&&x| db.def(x).card_def(x) == def).count();
                    // The chance cnt / m + (m - n) / m x (D - R) / F, scaled by m F; with no free card (F = 0: every place
                    // listed, so every domain full, m = n, and the deck is R) it is cnt / m, scaled by m - never 0 for a
                    // listed card: an `a` of 0 says no listed copy stood, and a find would then ask a copy past R, which
                    // no deck has (ENGINE.md 162).
                    let n = needs[k].len();
                    if free == 0 {
                        debug_assert_eq!(room, n, "no free card, so every domain is full");
                        (cnt as u64, 0)
                    } else {
                        ((cnt * free) as u64, (room - n) as u64)
                    }
                }
            }
        };
        let deck = |x: CardIdx| x.get() < self.first_token && self.players[owner as usize].companion != Some(x);
        let mut r: i32 = needs.iter().flatten().filter(|&&x| db.def(x).card_def(x) == def).count() as i32;
        for i in 0..self.objs.len() {
            let x = CardIdx::new(i as u32);
            if self.objs[i].owner != owner || card(x) != def {
                continue;
            }
            match (deck(x), slots.contains(&x)) {
                (true, false) => r += 1,
                (false, true) => r -= 1,
                _ => {}
            }
        }
        let grown = self.sightings.iter().filter(|s| s.viewer == q && s.owner == owner && s.def == def && s.grows).count() as i32;
        Some((a, b, r - grown))
    }

    /// Card `c`, just found by its owner's library search (CR 701.19; `DecisionKind::SearchLibrary`), is a find the
    /// other player can tell apart (ENGINE.md 140.9) until it is seen, goes into a hand, or is shuffled with cards
    /// that are not finds: its sighting is a search's.
    pub(crate) fn found_by_search(&mut self, c: CardIdx) {
        if !self.keeps_sightings() || self.observer.is_some() {
            return;
        }
        let t = self.sight_at;
        self.sight_at += 1;
        self.found.retain(|x| x.0 != c);
        self.found.push((c, t));
    }

    /// Card `c` is no longer a find the other player can tell apart (ENGINE.md 140.9).
    pub(crate) fn untrack_found(&mut self, c: CardIdx) {
        if !self.found.is_empty() {
            self.found.retain(|x| x.0 != c);
        }
    }

    /// The sightings of the game, in the order kept (tests, analysis; ENGINE.md 140.9).
    pub fn sightings(&self) -> &[Sighting] {
        &self.sightings
    }

    /// The state was changed by hand, outside the rules (tests' perturbations, `public_state`): the sightings no
    /// longer describe what each player saw, so they are dropped and the deals weigh every seen card blind (ENGINE.md
    /// 140, 140.9). Never called by the engine. The finds (`found`) stay: which cards the other player tells apart
    /// as a search's finds is public, layout v7's `found` tokens (ENGINE.md 142.12), and a deal without sightings
    /// reads none of it.
    #[doc(hidden)]
    pub fn lose_sightings(&mut self) {
        self.sightings.clear();
        self.sightings_lost = true;
    }

    /// Were the sightings lost to a change by hand (`lose_sightings`)?
    pub(crate) fn sightings_lost(&self) -> bool {
        self.sightings_lost
    }

    /// Reveal a card in a hidden zone to everyone (CR 701.16; 602.2a),
    /// keyed by its identity. A card every player knew already is keyed too
    /// when it is in a hand (ENGINE.md 128): a hand card is named by its
    /// definition's hand slot, whose place counts the hidden cards'
    /// definitions (ENGINE.md 11), so one action can name another known card
    /// in another world of the other player's; the key, which tells no one
    /// anything new, keeps those worlds apart, as a known card leaving a
    /// hidden zone is keyed (`move_card`). A known card elsewhere (a library
    /// card by its place, a face-down card by its object) is the same card
    /// in every world: no key.
    pub(crate) fn reveal(&mut self, idx: CardIdx) {
        let o = &self.objs[idx.i()];
        if !self.hidden_obj(idx) || o.known == 0b11 && o.zone != Zone::Hand {
            return;
        }
        let (owner, def, all_knew) = (o.owner, self.ident(idx), o.known == 0b11);
        // (A told-apart hand card the other player didn't know: which twin a
        // slot named, ENGINE.md 156.)
        self.key_told(idx, false);
        if !all_knew {
            for q in 0..2 {
                self.learn(q, idx);
            }
        }
        self.emit_public(KEY_REVEAL, owner, def as u64);
    }

    /// Card `c` (definition `d`) left hidden zone `from` for a public zone;
    /// `known_before` were its knowledge bits. Every viewer who did not know
    /// it exactly sees one card of identity `d` leave: a cloud whose domain
    /// held it loses one `d`; if it left the hand, where cards are not told
    /// apart, another card of the hand known to be a `d` may have been the
    /// one, so that exact knowledge becomes "a `d` was there" (the cloud's
    /// entry stays, or the card is no longer known). Unless a reference told
    /// the leaving card apart (`told`, `Game::told_apart` before it moved:
    /// every player saw which card it was), or the known twin is told apart
    /// (it is still there): ENGINE.md 156.
    pub(crate) fn knowledge_leaves(&mut self, c: CardIdx, owner: PlayerId, from: Zone, d: DefId, known_before: u8, told: bool) {
        for q in 0..2u8 {
            if q == owner || known_before & (1 << q) != 0 {
                for k in self.clouds.iter_mut().filter(|k| k.viewer == q) {
                    k.slots.retain(|&x| x != c);
                }
                continue;
            }
            let twin = (from == Zone::Hand && !told)
                .then(|| {
                    self.players[owner as usize]
                        .hand
                        .iter()
                        .copied()
                        .find(|&x| self.objs[x.i()].def == d && self.objs[x.i()].known & (1 << q) != 0 && !self.told_apart(x))
                })
                .flatten();
            let terms = self.sight_terms(q, owner, c, from, d);
            let mut merged = twin.is_some();
            for k in self.clouds.iter_mut().filter(|k| k.viewer == q) {
                if Self::in_domain_at(k, owner, c, from) {
                    k.slots.retain(|&x| x != c);
                    if twin.is_none() {
                        merged |= take_one(&mut k.defs, d);
                    }
                }
            }
            // (A card the viewer may have seen before is not a new one, and its chance is the mixture: 140.9.)
            self.sighted(q, owner, c, d, !merged, terms);
            if let Some(t) = twin {
                // The known `d` in hand becomes one of the hand's unknowns;
                // a cloud over the hand keeps "a `d` is there" if it lists
                // one, else nothing remains known about it.
                self.objs[t.i()].known &= !(1 << q);
            }
        }
        self.untrack_found(c);
        self.tidy_clouds();
    }

    /// Card `c` moved between hidden zones in a way every player sees (a
    /// draw: the top card goes to the hand; the second step of a private
    /// move whose rearrangement was already scrambled). A cloud whose
    /// domain held it still holds it.
    pub(crate) fn knowledge_moves(&mut self, c: CardIdx, owner: PlayerId, from: Zone, to: Zone) {
        // (A find going into a hand is one of its cards, which nobody else tells apart: ENGINE.md 140.9.)
        if to == Zone::Hand {
            self.untrack_found(c);
        }
        let mut changed = false;
        for k in self.clouds.iter_mut() {
            if !Self::in_domain_at(k, owner, c, from) {
                continue;
            }
            k.slots.retain(|&x| x != c);
            match to {
                Zone::Hand => k.hand = true,
                Zone::Library if !k.library => k.slots.push(c),
                // (Exiled face down: it stays in the domain, by its slot.)
                Zone::FaceDown => k.slots.push(c),
                _ => {}
            }
            changed = true;
        }
        if changed {
            self.tidy_clouds();
        }
    }

    /// `owner` rearranges cards privately: every other player loses track
    /// of which card is which among the domain (`slots`, plus the hand,
    /// plus the library). What they knew exactly there, and the clouds the
    /// domain touches, become one cloud. A card another player knew that its
    /// owner does not (a look at the opponent's library, ENGINE.md 192) is
    /// listed too, as that player's private entry (`Game::peeks`): the
    /// owner's worlds draw it again (`deal_keeping_others`).
    ///
    /// A told-apart hand card (`told_apart`: a reference of the public state
    /// names it, its card revealed - a miracle card until its trigger leaves
    /// the stack, CR 702.94b; an ability's source from the hand while the
    /// ability is on the stack, 602.2a) is not lost track of: every player
    /// sees which card it is, in the hand or as it leaves (`move_card`), so
    /// what each player knows of it stays (ENGINE.md 210; 156's rule, as
    /// `canon_hand_knowledge` and `knowledge_leaves` keep it).
    pub(crate) fn scramble(&mut self, owner: PlayerId, slots: &[CardIdx], hand: bool, library: bool) {
        let told: Vec<CardIdx> = if hand { self.players[owner as usize].hand.iter().copied().filter(|&c| self.told_apart(c)).collect() } else { Vec::new() };
        for q in 0..2u8 {
            if q == owner {
                continue;
            }
            let mut k = Cloud { viewer: q, owner, hand, library, slots: slots.to_vec(), defs: Vec::new() };
            k.slots.retain(|&c| {
                let z = self.objs[c.i()].zone;
                !((hand && z == Zone::Hand) || (library && z == Zone::Library))
            });
            let domain = self.domain(&k);
            // (ENGINE.md 140.9: finds mixed with cards the viewer does not know that are not finds can no longer be
            // told from them; among finds only - a Doomsday pile - every card the viewer may see there is one.)
            if !self.found.is_empty() {
                let mixed = domain.iter().any(|&c| self.objs[c.i()].known & (1 << q) == 0 && !self.found.iter().any(|x| x.0 == c));
                if mixed {
                    self.found.retain(|x| !domain.contains(&x.0));
                }
            }
            for c in domain {
                if told.contains(&c) {
                    continue;
                }
                let d = self.ident(c);
                let o = &mut self.objs[c.i()];
                if o.known & (1 << q) != 0 {
                    o.known &= !(1 << q);
                    insert_sorted(&mut k.defs, d);
                    if o.known & (1 << owner) == 0 {
                        // (only a library card: the owner knows its hand and its looked-at piles)
                        debug_assert!(library, "a private entry outside a library cloud");
                        self.peeks.push((q, owner, d));
                    }
                }
            }
            self.clouds.push(k);
        }
        self.tidy_clouds();
    }

    /// Cards just put into `owner`'s library in an order nobody knows: the
    /// owner forgets them (an owner's knowledge is exact or nothing), and
    /// what another player knew of them becomes "somewhere in that
    /// library" (a cloud over the library: the owner's own determinize
    /// keeps them there, so it stays true in every world); a card its owner
    /// did not know is that player's private entry (as `scramble`, ENGINE.md
    /// 192).
    pub(crate) fn forget_order(&mut self, owner: PlayerId, cards: &[CardIdx]) {
        for &c in cards {
            self.untrack_found(c);
        }
        // A cloud naming one of these slots now covers the whole library
        // (the owner's determinize may move what is there within it).
        for k in self.clouds.iter_mut().filter(|k| k.owner == owner) {
            if k.slots.iter().any(|c| cards.contains(c)) {
                k.library = true;
            }
        }
        for q in 0..2u8 {
            if q == owner {
                continue;
            }
            let mut defs = Vec::new();
            for &c in cards {
                let o = &mut self.objs[c.i()];
                if o.known & (1 << q) != 0 {
                    o.known &= !(1 << q);
                    insert_sorted(&mut defs, o.def);
                    if o.known & (1 << owner) == 0 {
                        self.peeks.push((q, owner, o.def));
                    }
                }
            }
            if !defs.is_empty() {
                self.clouds.push(Cloud { viewer: q, owner, hand: false, library: true, slots: Vec::new(), defs });
            }
        }
        for &c in cards {
            self.objs[c.i()].known &= !(1 << owner);
        }
        self.tidy_clouds();
    }

    /// Merge clouds with intersecting domains (same viewer and owner); drop
    /// empty clouds and clouds over all of the owner's hand and library
    /// (with open decklists they say nothing a determinize would not keep
    /// anyway; hidden decklists keep them, ENGINE.md 102.1).
    fn tidy_clouds(&mut self) {
        if self.clouds.is_empty() {
            self.peeks.clear();
            return;
        }
        let mut i = 0;
        while i < self.clouds.len() {
            let mut j = i + 1;
            let mut merged = false;
            while j < self.clouds.len() {
                let (a, b) = (&self.clouds[i], &self.clouds[j]);
                if a.viewer == b.viewer && a.owner == b.owner && self.domains_meet(a, b) {
                    let b = self.clouds.remove(j);
                    let a = &mut self.clouds[i];
                    a.hand |= b.hand;
                    a.library |= b.library;
                    for c in b.slots {
                        if !a.slots.contains(&c) {
                            a.slots.push(c);
                        }
                    }
                    for d in b.defs {
                        insert_sorted(&mut a.defs, d);
                    }
                    merged = true;
                } else {
                    j += 1;
                }
            }
            if !merged {
                i += 1;
            }
        }
        let objs = &self.objs;
        let fd = self.db.face_down_def();
        for k in &mut self.clouds {
            let (hand, library) = (k.hand, k.library);
            k.slots.retain(|&c| {
                let z = objs[c.i()].zone;
                (z.is_hidden() || objs[c.i()].def == fd) && !((hand && z == Zone::Hand) || (library && z == Zone::Library))
            });
            k.slots.sort_unstable();
        }
        // (With hidden decklists, or in a constructed best-of-three's hidden
        // games, a cloud over the whole hand and library is kept: it is the
        // record that cards the viewer saw are still in the deck, which a
        // sampled deck must hold, ENGINE.md 102.1, 120.)
        let keep_whole = self.spec.hidden_decklists || self.spec.deck_clouds;
        self.clouds.retain(|k| !(k.defs.is_empty() || (k.hand && k.library && !keep_whole)));
        self.clouds.sort_by_key(|k| (k.viewer, k.owner, k.hand, k.library, k.slots.first().copied()));
        self.tidy_peeks();
    }

    /// The private entries (`Game::peeks`, ENGINE.md 192) follow their clouds: each lives in its (viewer, owner)'s
    /// cloud over the library (a library scramble made it, and a cloud never loses its library); an entry that cloud
    /// no longer lists - a card of that identity left or was seen (the entry may have been it: the remaining one of a
    /// public entry and a private one is the private one), or the cloud was dropped - goes.
    fn tidy_peeks(&mut self) {
        if self.peeks.is_empty() {
            return;
        }
        let mut keep: Vec<(PlayerId, PlayerId, DefId)> = Vec::with_capacity(self.peeks.len());
        for &(q, o, d) in &self.peeks {
            let listed = self
                .clouds
                .iter()
                .find(|k| k.viewer == q && k.owner == o && k.library)
                .map_or(0, |k| k.defs.iter().filter(|&&x| x == d).count());
            if keep.iter().filter(|&&x| x == (q, o, d)).count() < listed {
                keep.push((q, o, d));
            }
        }
        self.peeks = keep;
    }

    fn domains_meet(&self, a: &Cloud, b: &Cloud) -> bool {
        if (a.hand && b.hand) || (a.library && b.library) {
            return true;
        }
        let covers = |k: &Cloud, c: CardIdx| {
            let z = self.objs[c.i()].zone;
            (k.hand && z == Zone::Hand) || (k.library && z == Zone::Library) || k.slots.contains(&c)
        };
        a.slots.iter().any(|&c| covers(b, c)) || b.slots.iter().any(|&c| covers(a, c))
    }

    /// Check the clouds against the true state (fuzzer): each cloud's
    /// identities are among the unknown cards of its domain; domains of one
    /// viewer over one owner are disjoint.
    pub fn check_knowledge(&self) -> Result<(), String> {
        for (i, k) in self.clouds.iter().enumerate() {
            if k.viewer == k.owner || k.defs.is_empty() {
                return Err(format!("degenerate cloud {k:?}"));
            }
            let mut have: Vec<DefId> = self
                .domain(k)
                .into_iter()
                .filter(|&c| !self.knows(k.viewer, c))
                .map(|c| self.ident(c))
                .collect();
            have.sort_unstable();
            for &d in &k.defs {
                if !take_one(&mut have, d) {
                    return Err(format!("cloud {k:?} lists {d} not in its domain"));
                }
            }
            for l in &self.clouds[i + 1..] {
                if l.viewer == k.viewer && l.owner == k.owner && self.domains_meet(k, l) {
                    return Err(format!("clouds meet: {k:?} {l:?}"));
                }
            }
        }
        Ok(())
    }

    // ---- determinize ------------------------------------------------------

    /// Resample everything hidden from `viewer` (ENGINE.md 10): for each
    /// owner, the identities of the cards `viewer` does not know exactly are
    /// shuffled among those cards, keeping each cloud's identities inside
    /// its domain; the state's RNG is reseeded; later private keys are
    /// masked for `viewer`; the pending decision is recomputed.
    pub fn determinize(&mut self, viewer: PlayerId, rng: &mut Rng) {
        let payer = self.payment_in_progress();
        self.deal_hidden(viewer, payer, rng);
        self.after_deal(viewer, rng);
    }

    /// `determinize` with the hand of `viewer`'s opponent dealt by weights
    /// (ENGINE.md 69: zero-core's belief-weighted determinization): every
    /// constraint of `determinize` kept - known cards, clouds, pinned cards,
    /// the public zones, the hand size -, the reseeding and the refresh the
    /// same; the opponent's hand slots that no cloud covers are filled by
    /// sequential sampling without replacement from the hidden pool, each
    /// draw taking a card of identity d with probability proportional to
    /// `w(d)` x its copies left (Wallenius' urn); a cloud that covers the
    /// hand places its listed cards in its domain as before, then deals its
    /// hand slots from its cards by the same weights; everything else
    /// (libraries, face-down cards, the viewer's own unknown cards) is
    /// shuffled uniformly as `determinize` does. Weights all equal: the
    /// uniform deal's distribution (another random stream). `w` must be
    /// finite and > 0. A pending face-down choice of the opponent's hand
    /// (ENGINE.md 123) is dealt with it, exactly, by `deal_weighted_pick`.
    pub fn determinize_weighted(&mut self, viewer: PlayerId, rng: &mut Rng, w: &dyn Fn(DefId) -> f64) {
        let payer = self.payment_in_progress();
        for owner in 0..2u8 {
            if owner == viewer {
                self.deal_owner(viewer, owner, payer, rng);
            } else {
                let (slots, pinned) = self.hidden_slots_with(viewer, owner, payer);
                let defs: Vec<DefId> = slots.iter().map(|&c| self.ident(c)).collect();
                let plan = self.weighted_plan(viewer, owner, &slots, &pinned, defs);
                match self.hidden_pick(viewer, owner) {
                    None => {
                        self.deal_weighted(&plan, &slots, rng, w, None);
                    }
                    Some((k, _, filter)) => self.deal_weighted_pick(owner, &plan, &slots, k, &filter, rng, w),
                }
            }
        }
        self.after_deal(viewer, rng);
    }

    /// `determinize_weighted`'s deal of `owner`'s hidden cards with a
    /// face-down choice of a card of `owner`'s hand pending (ENGINE.md 123,
    /// 147.5; `hidden_pick`: the frame at agenda `k`, `filter` the kind).
    /// The world is the weighted deal conditioned as the uniform deal is
    /// (`hidden_pick_ok`): a card of the hand drawn uniformly must be of the
    /// kind, and is the choice - each (deal, card) in proportion to the deal's
    /// probability under the weights times [the card is of the kind]. That
    /// was a rejection of whole deals, accepting with probability (cards of
    /// the kind in the hand) / |hand|: about 1 / (49 R^2) when the belief puts
    /// the hand's one card of the kind at the clip's floor and the rest at its
    /// ceiling R, so past R ~ 150 it exhausted `HIDDEN_PICK_TRIES` and
    /// panicked (the Fable review's 2026-11-12 hidden B4).
    ///
    /// Now the choice is dealt with the deal. The target is a mixture over
    /// which hand card is chosen: one of the hand cards the deal doesn't move
    /// (each of the kind: the plain deal, mass 1), or a dealt hand place -
    /// draw j of a Wallenius sequence (the free hand places' from the pool, or
    /// a cloud's hand places' from its cards) - with mass P(draw j is of the
    /// kind). That mass has no closed form (Wallenius' urn is not
    /// exchangeable), so the place is proposed by an upper bound M_j of it
    /// and corrected: the sequence is drawn with its card of the kind first
    /// (by weight among its kind, `weighted_draws_forced`), the rest as
    /// always, the first card moved to place j; the importance ratio of that
    /// is (S_K / S) prod_t S_t(first) / S_t(at j) <= S_K / (S - T_j) = M_j
    /// (S, S_K: the sequence's pool's weight and its kind's; T_j: its j
    /// heaviest; the products of the draws' remaining weights telescope),
    /// and the proposal is accepted with probability ratio / M_j. Every other
    /// part of the deal is drawn from its own conditional, exactly as
    /// `deal_weighted` draws it. So a world comes with probability exactly
    /// the target's, with no try count: a round is accepted with probability
    /// (A + E[cards of the kind among the dealt hand places]) / (A + sum M_j)
    /// - near 1 in the review's case (the kind's own weight cancels), and for
    /// any weights at least (S - T_{n-1}) / (n S) of the free sequence's
    /// part. A cloud's cards are random (its fill of free cards), so its
    /// bound is taken over every fill. No card of the kind possible anywhere
    /// (every M and A zero) is a broken invariant (ENGINE.md 129.3: a world's
    /// deck always holds a legal pick): a loud panic, not a stall.
    #[allow(clippy::too_many_arguments)]
    fn deal_weighted_pick(
        &mut self,
        owner: PlayerId,
        plan: &WeightedPlan,
        slots: &[CardIdx],
        k: usize,
        filter: &crate::db::Filter,
        rng: &mut Rng,
        w: &dyn Fn(DefId) -> f64,
    ) {
        let ctx = crate::cast::Ctx { controller: owner, source: None };
        let hand = self.players[owner as usize].hand.clone();
        // The hand cards the deal doesn't move (known, pinned) of the kind: chosen with the plain deal.
        let fixed: Vec<CardIdx> = hand.iter().copied().filter(|c| !slots.contains(c) && self.card_matches(filter, *c, &ctx)).collect();
        // Which identities are of the kind: a dealt hand card given each one (a copy of the state).
        let mut kind: Vec<DefId> = Vec::new();
        if let Some(&probe_card) = hand.iter().find(|c| slots.contains(c)) {
            let mut ids: Vec<DefId> = plan.pool.iter().chain(plan.needs.iter().flatten()).copied().collect();
            ids.sort_unstable();
            ids.dedup();
            let mut probe = self.clone();
            for d in ids {
                probe.set_ident(probe_card, d);
                if probe.card_matches(filter, probe_card, &ctx) {
                    kind.push(d);
                }
            }
        }
        let is_kind = |d: DefId| kind.binary_search(&d).is_ok();
        let wt = |v: &[DefId]| -> Vec<f64> { v.iter().map(|&d| w(d)).collect() };
        // The dealt hand places, each a draw of a sequence, and their bounds M_j.
        let mut places: Vec<(Force, f64)> = Vec::new();
        if plan.free_hand > 0 {
            let all = wt(&plan.pool);
            let s: f64 = all.iter().sum();
            let s_k: f64 = plan.pool.iter().zip(&all).filter(|(d, _)| is_kind(**d)).map(|(_, x)| x).sum();
            let mut desc = all.clone();
            desc.sort_by(|a, b| b.total_cmp(a));
            for j in 0..plan.free_hand {
                let t: f64 = desc[..j].iter().sum();
                places.push((Force { seq: None, j }, if s_k > 0.0 { s_k / (s - t) } else { 0.0 }));
            }
        }
        for (ki, c) in plan.clouds.iter().enumerate() {
            if c.hand_pos.is_empty() {
                continue;
            }
            // (Its cards: its listed ones and a fill of free cards; the bound over every fill.)
            let fill = c.mine.len() - plan.needs[ki].len();
            let need_w = wt(&plan.needs[ki]);
            let mut free_kind: Vec<f64> = plan.pool.iter().filter(|&&d| is_kind(d)).map(|&d| w(d)).collect();
            free_kind.sort_by(|a, b| b.total_cmp(a));
            let s_k_max: f64 = plan.needs[ki].iter().zip(&need_w).filter(|(d, _)| is_kind(**d)).map(|(_, x)| x).sum::<f64>()
                + free_kind.iter().take(fill).sum::<f64>();
            let mut asc: Vec<f64> = need_w.iter().copied().chain(wt(&plan.pool)).collect();
            asc.sort_by(|a, b| a.total_cmp(b));
            for j in 0..c.hand_pos.len() {
                let lb: f64 = asc[..c.mine.len() - j].iter().sum();
                places.push((Force { seq: Some(ki), j }, if s_k_max > 0.0 { s_k_max / lb } else { 0.0 }));
            }
        }
        let total = fixed.len() as f64 + places.iter().map(|p| p.1).sum::<f64>();
        assert!(
            total > 0.0,
            "determinize: no deal of P{owner}'s hand can hold a legal face-down choice (ENGINE.md 123, 129.3: a world's deck always holds one)"
        );
        loop {
            let mut x = unit(rng) * total;
            if x < fixed.len() as f64 {
                let c = fixed[(x as usize).min(fixed.len() - 1)];
                self.deal_weighted(plan, slots, rng, w, None);
                self.set_hidden_pick(k, c);
                return;
            }
            x -= fixed.len() as f64;
            let (f, m) = *places.iter().find(|p| {
                if x < p.1 {
                    return true;
                }
                x -= p.1;
                false
            })
            .unwrap_or_else(|| places.iter().rev().find(|p| p.1 > 0.0).expect("a place of positive mass"));
            let (ratio, c) = self.deal_weighted(plan, slots, rng, w, Some((f, kind.as_slice())));
            if unit(rng) * m < ratio {
                self.set_hidden_pick(k, c.expect("the forced place's card"));
                return;
            }
        }
    }

    /// What `determinize` does after its deal: canonicalize the hands'
    /// knowledge, reseed, observe, refresh.
    fn after_deal(&mut self, viewer: PlayerId, rng: &mut Rng) {
        // A world is in the normal form every state at rest is in
        // (`canon_hand_knowledge`, run after each step): the deal may have
        // given a card below a known twin the twin's definition, and the
        // known one must come first (a hand slot names its definition's first
        // card, `hand_slot_order_in`: the same knowledge in every world,
        // ENGINE.md 128). Knowledge bits move among same-definition hand
        // cards only: what anyone knows (a multiset per hand) is unchanged.
        self.canon_hand_knowledge();
        // A gone object's identity the viewer didn't know (another player
        // may have: its stale references show it to that player, ENGINE.md
        // 156) is its card's in this world: the same physical card, whose
        // identity the deal may have changed. The ones the viewer knew stay.
        for i in 0..self.ref_lki.len() {
            let (r, _, known) = self.ref_lki[i];
            if known >> viewer & 1 == 0 {
                self.ref_lki[i].1 = self.ident(r.idx);
            }
        }
        self.rng = Rng::new(rng.next_u64());
        self.observer = Some(viewer);
        // The loop actions the viewer decided keep their records (loop
        // hashes of the viewer's own views, the same in every world it
        // deals: its options stay the same). Another decider's were hashes
        // of views holding what the viewer doesn't know; in a world they
        // would match only where the deal got those things right, so they
        // go: that decider's loops may repeat once more here, never fewer
        // times (`loops.rs`).
        // (An activation in progress, `loop_open`, likewise: another
        // decider's goes - its loop may repeat once more here.)
        if !self.loop_taken.is_empty() || self.loop_open.is_some() {
            let keep = [self.decider_of(0) == viewer, self.decider_of(1) == viewer];
            self.loop_taken.retain(|x| keep[x.p as usize]);
            if self.loop_open.as_ref().is_some_and(|o| !keep[o.rec.p as usize]) {
                self.loop_open = None;
            }
        }
        // The viewer's own options never name a card whose identity was
        // resampled (rule: a choice of a card in a zone hidden from its
        // decider names its definition, not its slot; the decider's hand is
        // never resampled for them). Only another player's decision needs
        // recomputing. Debug builds check the rule exactly.
        if self.pending.as_ref().is_some_and(|d| self.decider_of(d.player) != viewer) {
            self.refresh_pending();
        } else if cfg!(debug_assertions) && self.pending.is_some() {
            let mut c = self.clone();
            c.refresh_pending();
            assert_eq!(c.pending, self.pending, "determinize changed the viewer's decision");
        }
    }

    /// `determinize`'s deal: every owner's cards hidden from `viewer` dealt
    /// again (but the cards a payment in progress by `payer` pins).
    fn deal_hidden(&mut self, viewer: PlayerId, payer: Option<PlayerId>, rng: &mut Rng) {
        for owner in 0..2u8 {
            self.deal_owner(viewer, owner, payer, rng);
            // A face-down choice of a card in `owner`'s hand that `viewer`
            // doesn't know (Show and Tell's, CR 101.4a): the deal is drawn
            // again until a card of the hand drawn uniformly is a legal
            // choice, which becomes the choice (`Game::hidden_pick_ok`,
            // ENGINE.md 123). (Nothing is drawn without one.)
            let mut tries = 0u32;
            while !self.hidden_pick_ok(viewer, owner, rng) {
                tries += 1;
                assert!(tries < HIDDEN_PICK_TRIES, "determinize: no deal of P{owner}'s hand holds a legal face-down choice (ENGINE.md 123)");
                self.deal_owner(viewer, owner, payer, rng);
            }
        }
    }

    /// `deal_hidden` of one owner's cards.
    fn deal_owner(&mut self, viewer: PlayerId, owner: PlayerId, payer: Option<PlayerId>, rng: &mut Rng) {
        let (slots, pinned) = self.hidden_slots_with(viewer, owner, payer);
        let mut defs: SVec<DefId, 64> = slots.iter().map(|&c| self.ident(c)).collect();
        if viewer == owner && self.deal_keeping_others(owner, &slots, rng) {
            return;
        }
        if !self.clouds.iter().any(|k| k.viewer == viewer && k.owner == owner) {
            rng.shuffle(&mut defs);
            for (&c, &d) in slots.iter().zip(defs.iter()) {
                self.set_ident(c, d);
            }
            return;
        }
        self.resample_with_clouds(viewer, owner, &slots, &pinned, defs.to_vec(), rng);
    }

    /// The owner's deal of its own hidden cards `slots` (`viewer == owner`: its library) when the other player knows
    /// some of them exactly and has clouds over the owner's cards (ENGINE.md 192: a look at a library card its owner
    /// doesn't know - Mishra's Bauble, Jace's +2 - and a cloud of that player's from an earlier card both knew). A
    /// plain shuffle keeps every card that player knows a card it knows, but could move a cloud's listed identity onto
    /// one of them, and the cloud ("these identities are among the domain's cards I don't know") would be false in the
    /// world. So each such cloud's listed identities - less those of its domain's cards it doesn't know that the deal
    /// leaves (the owner's known cards) - are placed among the domain's dealt cards that player doesn't know, the rest
    /// of the pool over every remaining place: `resample_with_clouds`'s deal, for the other player's clouds. And that
    /// player's private entries (`Game::peeks`: the identity of a card it saw that the owner did not, listed since a
    /// shuffle or search) are not the owner's to know: they are drawn again, after the deal, from the identities of
    /// their cloud's domain cards that player doesn't know, less the cloud's other entries (uniform, without
    /// replacement) - the world never shows the owner what the other player saw. -> false (nothing dealt) when the
    /// other player has no cloud over the owner, or no dealt card is known to it and no entry is private: then a plain
    /// shuffle keeps its knowledge (every dealt card is unknown to it, so every domain's unknown part keeps its
    /// multiset - the invariant before section 192).
    fn deal_keeping_others(&mut self, owner: PlayerId, slots: &[CardIdx], rng: &mut Rng) -> bool {
        let q = crate::ids::opp(owner);
        let private: Vec<DefId> = self.peeks.iter().filter(|x| x.0 == q && x.1 == owner).map(|x| x.2).collect();
        if !self.clouds.iter().any(|k| k.viewer == q && k.owner == owner)
            || (private.is_empty() && !slots.iter().any(|&c| self.objs[c.i()].known & (1 << q) != 0))
        {
            return false;
        }
        let clouds: Vec<&Cloud> = self.clouds.iter().filter(|k| k.viewer == q && k.owner == owner).collect();
        let unknown_to_q = |c: CardIdx| self.objs[c.i()].known & (1 << q) == 0;
        let home: Vec<Option<usize>> = slots
            .iter()
            .map(|&c| {
                let z = self.objs[c.i()].zone;
                if unknown_to_q(c) {
                    clouds.iter().position(|k| Self::in_domain_at(k, owner, c, z))
                } else {
                    None
                }
            })
            .collect();
        let mut needs: Vec<Vec<DefId>> = Vec::with_capacity(clouds.len());
        for (ki, k) in clouds.iter().enumerate() {
            let mut need = k.defs.clone();
            // (The private entries are drawn after the deal: only the cloud's other entries place cards.)
            if k.library {
                for &d in &private {
                    assert!(take_one(&mut need, d), "determinize: P{q}'s private entry {d} is not in its cloud {k:?}");
                }
            }
            // (Its domain's cards it doesn't know that the deal leaves hold some of the listed identities.)
            for c in self.domain(k) {
                if unknown_to_q(c) && !slots.contains(&c) {
                    take_one(&mut need, self.ident(c));
                }
            }
            let room = home.iter().filter(|&&h| h == Some(ki)).count();
            assert!(need.len() <= room, "determinize: P{q}'s cloud {k:?} lists more than its domain's dealt cards hold");
            needs.push(need);
        }
        let mut pool: Vec<DefId> = slots.iter().map(|&c| self.ident(c)).collect();
        pool.sort_unstable();
        for need in &needs {
            for &d in need {
                assert!(take_one(&mut pool, d), "determinize: P{q}'s cloud card {d} is not among P{owner}'s hidden cards");
            }
        }
        rng.shuffle(&mut pool);
        let mut free = pool.into_iter();
        let mut per_cloud = needs;
        for (ki, v) in per_cloud.iter_mut().enumerate() {
            let room = home.iter().filter(|&&h| h == Some(ki)).count();
            while v.len() < room {
                v.push(free.next().expect("pool covers every place"));
            }
            rng.shuffle(v);
        }
        let mut next = vec![0usize; per_cloud.len()];
        for (i, &c) in slots.iter().enumerate() {
            let d = match home[i] {
                Some(ki) => {
                    next[ki] += 1;
                    per_cloud[ki][next[ki] - 1]
                }
                None => free.next().expect("pool covers every place"),
            };
            self.set_ident(c, d);
        }
        if !private.is_empty() {
            let ki = self.clouds.iter().position(|k| k.viewer == q && k.owner == owner && k.library).expect("the private entries' cloud");
            let k = &self.clouds[ki];
            let mut public = k.defs.clone();
            for &d in &private {
                take_one(&mut public, d);
            }
            let mut pool: Vec<DefId> = self.domain(k).into_iter().filter(|&c| self.objs[c.i()].known & (1 << q) == 0).map(|c| self.ident(c)).collect();
            pool.sort_unstable();
            for &d in &public {
                assert!(take_one(&mut pool, d), "determinize: P{q}'s cloud entry {d} is not among its domain's cards");
            }
            assert!(pool.len() >= private.len(), "determinize: P{q}'s private entries outnumber its domain's free cards");
            rng.shuffle(&mut pool);
            let drawn: Vec<DefId> = pool[..private.len()].to_vec();
            let mut defs = public;
            for &d in &drawn {
                insert_sorted(&mut defs, d);
            }
            self.clouds[ki].defs = defs;
            self.peeks.retain(|x| !(x.0 == q && x.1 == owner));
            self.peeks.extend(drawn.into_iter().map(|d| (q, owner, d)));
        }
        true
    }

    /// `determinize_weighted`'s plan of `owner`'s cards `slots` (identities
    /// `defs`), as `resample_with_clouds` places them (ENGINE.md 69.2): each
    /// cloud's listed cards out of the pool, the slots' clouds, the hand's
    /// places outside every cloud (the hand is one pool: a cloud covers all of
    /// it or none) and each cloud's places and hand places.
    fn weighted_plan(&self, viewer: PlayerId, owner: PlayerId, slots: &[CardIdx], pinned: &[CardIdx], defs: Vec<DefId>) -> WeightedPlan {
        let mut pool = defs;
        pool.sort_unstable();
        let clouds = self.clouds.iter().any(|k| k.viewer == viewer && k.owner == owner);
        let (home, needs) = if clouds { self.cloud_needs(viewer, owner, slots, pinned) } else { (vec![None; slots.len()], Vec::new()) };
        for need in &needs {
            for &d in need {
                assert!(take_one(&mut pool, d), "determinize: a cloud's card {d} is not among P{owner}'s hidden cards for P{viewer}");
            }
        }
        let in_hand: Vec<bool> = slots.iter().map(|&c| self.objs[c.i()].zone == Zone::Hand).collect();
        let free_hand = (0..slots.len()).filter(|&i| in_hand[i] && home[i].is_none()).count();
        let clouds = (0..needs.len())
            .map(|ki| {
                let mine: Vec<usize> = (0..slots.len()).filter(|&i| home[i] == Some(ki)).collect();
                let hand_pos: Vec<usize> = mine.iter().enumerate().filter(|&(_, &i)| in_hand[i]).map(|(k, _)| k).collect();
                PlanCloud { mine, hand_pos }
            })
            .collect();
        WeightedPlan { pool, home, needs, in_hand, free_hand, clouds }
    }

    /// `determinize_weighted`'s deal of `slots` by `plan`: the hand's places
    /// outside every cloud get weighted draws from the pool; the rest of the
    /// pool shuffled; each cloud its listed cards and as many free cards as it
    /// has places, its hand places drawn from them by the weights, the rest
    /// uniform; the other places the rest of the pool. `force`: one hand
    /// place's draw dealt with its card of the kind first
    /// (`weighted_draws_forced`; `deal_weighted_pick`): its importance ratio
    /// and the card at that place are returned (else 1 and None).
    fn deal_weighted(
        &mut self,
        plan: &WeightedPlan,
        slots: &[CardIdx],
        rng: &mut Rng,
        w: &dyn Fn(DefId) -> f64,
        force: Option<(Force, &[DefId])>,
    ) -> (f64, Option<CardIdx>) {
        let mut ratio = 1.0;
        let mut pool = plan.pool.clone();
        let hand_ids = match force {
            Some((Force { seq: None, j }, kind)) => {
                let (v, r) = weighted_draws_forced(&mut pool, plan.free_hand, j, kind, w, rng);
                ratio = r;
                v
            }
            _ => weighted_draws(&mut pool, plan.free_hand, w, rng),
        };
        rng.shuffle(&mut pool);
        let mut free = pool.into_iter();
        let mut per_cloud: Vec<Vec<DefId>> = plan.needs.clone();
        for (ki, v) in per_cloud.iter_mut().enumerate() {
            let PlanCloud { mine, hand_pos } = &plan.clouds[ki];
            while v.len() < mine.len() {
                v.push(free.next().expect("pool covers every place"));
            }
            // its hand slots (in its slot order) by the weights, the rest uniform
            if hand_pos.is_empty() {
                rng.shuffle(v);
            } else {
                v.sort_unstable();
                let drawn = match force {
                    Some((Force { seq: Some(fk), j }, kind)) if fk == ki => {
                        let (d, r) = weighted_draws_forced(v, hand_pos.len(), j, kind, w, rng);
                        ratio = r;
                        d
                    }
                    _ => weighted_draws(v, hand_pos.len(), w, rng),
                };
                rng.shuffle(v);
                let mut rest = std::mem::take(v).into_iter();
                let mut d = drawn.into_iter();
                *v = (0..mine.len()).map(|k| (if hand_pos.contains(&k) { d.next() } else { rest.next() }).expect("a cloud's cards fill its places")).collect();
            }
        }
        let mut next = vec![0usize; per_cloud.len()];
        let mut hand_next = hand_ids.into_iter();
        let mut free_hand_k = 0usize;
        let mut picked = None;
        for (i, &c) in slots.iter().enumerate() {
            let d = match plan.home[i] {
                Some(ki) => {
                    next[ki] += 1;
                    if let Some((Force { seq: Some(fk), j }, _)) = force {
                        if fk == ki && plan.clouds[ki].hand_pos.get(j) == Some(&(next[ki] - 1)) {
                            picked = Some(c);
                        }
                    }
                    per_cloud[ki][next[ki] - 1]
                }
                None if plan.in_hand[i] => {
                    if let Some((Force { seq: None, j }, _)) = force {
                        if j == free_hand_k {
                            picked = Some(c);
                        }
                    }
                    free_hand_k += 1;
                    hand_next.next().expect("a draw for every hand slot")
                }
                None => free.next().expect("pool covers every place"),
            };
            self.set_ident(c, d);
        }
        (ratio, picked)
    }

    /// Show and Tell's pending face-down choice (the frame at agenda `k`,
    /// `hidden_pick`) is card `c` in this world.
    fn set_hidden_pick(&mut self, k: usize, c: CardIdx) {
        if let crate::flow::Task::Run(f) = &mut self.agenda[k] {
            f.picked[0] = c;
        }
    }

    /// The position lab's `"actual"` deal (ENGINE.md 76): every card stays where it is - both
    /// hands, every card some player knows exactly - but the library cards nobody knows, whose
    /// identities are shuffled among themselves within each group of library cards that every
    /// cloud treats alike (the same membership in every cloud's domain, as `force_library_card`
    /// swaps), so each cloud's identities stay inside its domain and every player's knowledge
    /// holds; then the state's random stream is reseeded. What is still random from the spot on
    /// (the library orders nobody has seen) is dealt again; nothing else changes. Not used by any
    /// search (`determinize` deals the decider's worlds).
    #[doc(hidden)]
    pub fn redeal_unknown_library_orders(&mut self, rng: &mut Rng) {
        for owner in 0..2u8 {
            let lib: Vec<CardIdx> =
                self.players[owner as usize].library.iter().copied().filter(|&c| self.objs[c.i()].known == 0).collect();
            let sig: Vec<Vec<bool>> = lib
                .iter()
                .map(|&c| self.clouds.iter().map(|k| Self::in_domain_at(k, owner, c, Zone::Library)).collect())
                .collect();
            let mut done = vec![false; lib.len()];
            for i in 0..lib.len() {
                if done[i] {
                    continue;
                }
                let group: Vec<usize> = (i..lib.len()).filter(|&j| !done[j] && sig[j] == sig[i]).collect();
                let mut defs: Vec<DefId> = group.iter().map(|&j| self.ident(lib[j])).collect();
                rng.shuffle(&mut defs);
                for (&j, &d) in group.iter().zip(&defs) {
                    done[j] = true;
                    self.set_ident(lib[j], d);
                }
            }
        }
        self.rng = Rng::new(rng.next_u64());
    }

    /// Tests and analysis (ENGINE.md 67): `determinize`'s deal of every card
    /// hidden from `viewer` **including** the ones a payment in progress pins
    /// (the payer's mana sources in hand, the cards a cost may exile from
    /// it), and nothing else: no reseeding, no observer, the pending decision
    /// left as it is (the world may be one that could not ask it). What
    /// `viewer` sees cannot depend on any of those cards, so its encoding
    /// must be the same: the leak check of a non-deciding viewer's encoding
    /// (zero-core's root-view leaf values evaluate positions from it), which
    /// `determinize` alone cannot make for the pinned cards.
    #[doc(hidden)]
    pub fn resample_unpinned(&mut self, viewer: PlayerId, rng: &mut Rng) {
        self.deal_hidden(viewer, None, rng);
    }

    /// Tests and analysis: the cards `determinize(viewer)` keeps for a
    /// payment in progress (ENGINE.md 10's pinned cards; none for the payer
    /// itself).
    #[doc(hidden)]
    pub fn pinned_for(&self, viewer: PlayerId) -> Vec<CardIdx> {
        let payer = self.payment_in_progress();
        (0..2u8).flat_map(|o| self.hidden_slots_with(viewer, o, payer).1).collect()
    }

    /// The cards of `owner` whose identities `determinize(viewer)` deals
    /// again (those `viewer` does not know exactly, ENGINE.md 10), and the
    /// pinned ones it keeps (a payment in progress by `owner` may count on
    /// them).
    pub(crate) fn hidden_slots(&self, viewer: PlayerId, owner: PlayerId) -> (SVec<CardIdx, 64>, Vec<CardIdx>) {
        self.hidden_slots_with(viewer, owner, self.payment_in_progress())
    }

    fn hidden_slots_with(&self, viewer: PlayerId, owner: PlayerId, payer: Option<PlayerId>) -> (SVec<CardIdx, 64>, Vec<CardIdx>) {
        let mut slots = self.unknown_to_in(viewer, owner);
        let mut pinned: Vec<CardIdx> = Vec::new();
        // A token in a hidden zone (it ceases to exist at the next state-based
        // actions, CR 111.8, 704.5d) is not a card: its identity is never dealt
        // to a card's slot, nor a card's to it (ENGINE.md 102.3: a world held a
        // Robot token's identity in a card's slot). It stays, as a pinned card.
        if slots.iter().any(|c| c.get() >= self.first_token) {
            slots.retain(|&c| {
                let token = self.is_token(c);
                if token {
                    pinned.push(c);
                }
                !token
            });
        }
        if payer == Some(owner) && owner != viewer {
            // A payment in progress may count on the payer's mana sources in
            // hand, and on the cards a cost exiles from hand: they are pinned
            // (ENGINE.md 10).
            let exile = self.exile_filters_in_progress();
            slots.retain(|&c| {
                let pin = self.hand_mana_source(c)
                    || (self.objs[c.i()].zone == Zone::Hand && exile.iter().any(|(f, ctx)| self.card_matches(f, c, ctx)));
                if pin {
                    pinned.push(c);
                }
                !pin
            });
        }
        (slots, pinned)
    }

    /// Hidden decks (ENGINE.md 36.11): give `owner`'s cards `slots` (those
    /// `viewer` does not know; `pinned`, kept by a payment in progress, are
    /// not among them) the identities `defs` (one each; a sampled deck's
    /// cards), each cloud of (`viewer`, `owner`)'s identities among the
    /// slots of its domain (`resample_with_clouds`), and set their
    /// sideboard. The rare-rule bits (`seen`) learn the new identities
    /// (with hidden decks the miracle question does not read them:
    /// `Game::deck_question`).
    ///
    /// (The deal must hold the clouds itself: `determinize`, which follows,
    /// keeps the payer's hand mana sources where they are (ENGINE.md 10),
    /// and a card a cloud places in the library dealt to such a hand slot
    /// stayed there, so the cloud's card was dealt twice and another card
    /// of the deck not at all; ENGINE.md 51.)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn redeal(
        &mut self,
        viewer: PlayerId,
        owner: PlayerId,
        slots: &[CardIdx],
        pinned: &[CardIdx],
        defs: &[DefId],
        sideboard: std::sync::Arc<[DefId]>,
        rng: &mut Rng,
    ) {
        assert_eq!(slots.len(), defs.len(), "redeal: a sampled deck that does not fit P{owner}'s cards");
        let bits = seen_bits_of(self.db);
        for (&c, &d) in slots.iter().zip(defs) {
            if self.ident(c) != d {
                self.seen |= bits[d as usize];
                self.set_ident(c, d);
            }
        }
        if self.clouds.iter().any(|k| k.viewer == viewer && k.owner == owner) {
            let pool: Vec<DefId> = slots.iter().map(|&c| self.ident(c)).collect();
            self.resample_with_clouds(viewer, owner, slots, pinned, pool, rng);
        }
        self.players[owner as usize].sideboard = sideboard;
    }

    /// What the clouds of (`viewer`, `owner`) place among `owner`'s hidden
    /// cards `slots` (`determinize`'s deal, and the hidden pool's sampled
    /// deck, ENGINE.md 36.11): per slot the cloud whose domain holds it, and
    /// per cloud its listed identities less those of the `pinned` cards in
    /// its domain (exactly known for this pass), at most as many as its
    /// domain's slots.
    pub(crate) fn cloud_needs(
        &self,
        viewer: PlayerId,
        owner: PlayerId,
        slots: &[CardIdx],
        pinned: &[CardIdx],
    ) -> (Vec<Option<usize>>, Vec<Vec<DefId>>) {
        let clouds: Vec<&Cloud> = self.clouds.iter().filter(|k| k.viewer == viewer && k.owner == owner).collect();
        // Per slot: the cloud whose domain holds it.
        let home: Vec<Option<usize>> = slots
            .iter()
            .map(|&c| {
                let z = self.objs[c.i()].zone;
                clouds.iter().position(|k| Self::in_domain_at(k, owner, c, z))
            })
            .collect();
        let mut needs: Vec<Vec<DefId>> = Vec::with_capacity(clouds.len());
        for (ki, k) in clouds.iter().enumerate() {
            let mut need = k.defs.clone();
            // A pinned card in the domain is exactly known for this pass.
            for &p in pinned {
                let z = self.objs[p.i()].zone;
                if Self::in_domain_at(k, owner, p, z) {
                    take_one(&mut need, self.objs[p.i()].def);
                }
            }
            let room = home.iter().filter(|&&h| h == Some(ki)).count();
            need.truncate(room);
            needs.push(need);
        }
        (home, needs)
    }

    /// The identities of `slots` (all of them, `pool`) are dealt so each
    /// cloud's listed identities land in its domain: uniform over the
    /// assignments consistent with the clouds (listed cards placed in their
    /// domain at random, the rest of the pool over every remaining place).
    fn resample_with_clouds(
        &mut self,
        viewer: PlayerId,
        owner: PlayerId,
        slots: &[CardIdx],
        pinned: &[CardIdx],
        mut pool: Vec<DefId>,
        rng: &mut Rng,
    ) {
        pool.sort_unstable();
        let (home, needs) = self.cloud_needs(viewer, owner, slots, pinned);
        for need in &needs {
            for &d in need {
                // (Never a world that holds a card twice and another not at
                // all: ENGINE.md 51.)
                assert!(take_one(&mut pool, d), "determinize: a cloud's card {d} is not among P{owner}'s hidden cards for P{viewer}");
            }
        }
        rng.shuffle(&mut pool);
        let mut free = pool.into_iter();
        let mut per_cloud: Vec<Vec<DefId>> = needs;
        for (ki, v) in per_cloud.iter_mut().enumerate() {
            let room = home.iter().filter(|&&h| h == Some(ki)).count();
            while v.len() < room {
                v.push(free.next().expect("pool covers every place"));
            }
            rng.shuffle(v);
        }
        let mut next = vec![0usize; per_cloud.len()];
        for (i, &c) in slots.iter().enumerate() {
            let d = match home[i] {
                Some(ki) => {
                    next[ki] += 1;
                    per_cloud[ki][next[ki] - 1]
                }
                None => free.next().expect("pool covers every place"),
            };
            self.set_ident(c, d);
        }
    }

    /// The player paying a cost (or casting or activating something whose
    /// cost is still to be paid), if any: from the cast or activation until
    /// its mana is paid, and through a "pay {X}?" payment during
    /// resolution.
    pub(crate) fn payment_in_progress(&self) -> Option<PlayerId> {
        use crate::flow::Task;
        self.agenda.iter().rev().find_map(|t| match t {
            Task::PayMana(pm) => Some(pm.payer),
            // Mana abilities activated with priority (ENGINE.md 100.4.3):
            // the ones offered counted on the player's mana sources in hand
            // (a Spirit Guide's, or one that pays another's mana cost).
            Task::ActivateMana { p, .. } => Some(*p),
            Task::CastPay { id } | Task::ActPay { id } | Task::PayCosts { id, .. } | Task::CastDone { id } | Task::ChooseForCost { id, .. } => {
                self.item_index(*id).map(|i| self.stack[i].controller)
            }
            // "Which face?" of a card being cast (CR 601.2a-b, 712.11b,
            // 715.3): the cast has begun, and the faces offered were found
            // castable counting the caster's hand (a Spirit Guide's mana), so
            // it pins as a payment does (ENGINE.md 71.4).
            Task::ChooseFace { p, mask, .. } if mask & (crate::decision::FACE_CAST | crate::decision::OTHER_CAST) != 0 => Some(*p),
            // "Which graveyard card?" after Priority's "cast or play from the
            // graveyard" (ENGINE.md 100.5): the casts offered were found
            // castable counting the caster's hand, as a face choice's.
            Task::GraveyardCast { p } => Some(*p),
            _ => None,
        })
    }

    /// The filters of the hand cards a cost of a spell being cast may still
    /// take: "exile a card from your hand" (any of its alternative costs),
    /// retrace's land card; with their context.
    pub(crate) fn exile_filters_in_progress(&self) -> Vec<(crate::db::Filter, crate::cast::Ctx)> {
        use crate::flow::Task;
        let mut out = Vec::new();
        for t in &self.agenda {
            // "Which graveyard card?" (`payment_in_progress`, ENGINE.md
            // 100.5): the casts it offers may count on a hand card as a cast's
            // do (an alternative cost's, retrace's land card).
            if let Task::GraveyardCast { p } = *t {
                let offered: Vec<CardIdx> = self.graveyard_cast_options(p).iter().filter_map(|o| match o.choice {
                    crate::decision::Choice::Cast(c) | crate::decision::Choice::CastFace(c) | crate::decision::Choice::Faces(c, _) => Some(c),
                    _ => None,
                }).collect();
                for c in offered {
                    let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(c)) };
                    let def = self.def_of(c);
                    let faces = [def.halves.map(|h| h.0).unwrap_or(self.objs[c.i()].def), def.halves.map(|h| h.1).unwrap_or(self.objs[c.i()].def)];
                    for face in faces.into_iter().chain(def.other_face()) {
                        for a in &self.db.def(face).alt_costs {
                            if let Some(f) = &a.cost.exile_from_hand {
                                out.push((f.clone(), ctx));
                            }
                        }
                    }
                    if self.gy_routes(p, c, None).iter().any(|r| r.code == crate::cast::route::RETRACE) {
                        if let Some(f) = &crate::cast::retrace_cost().discard_filter {
                            out.push((f.clone(), ctx));
                        }
                    }
                }
                continue;
            }
            // A face choice (`payment_in_progress`): the alternative costs of
            // the faces it offers to cast may count on a hand card too.
            if let Task::ChooseFace { p, card, mask } = *t {
                let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
                let def = self.def_of(card);
                let front = match def.halves {
                    Some((left, _)) => left,
                    None => self.objs[card.i()].def,
                };
                let faces = [(crate::decision::FACE_CAST, Some(front)), (crate::decision::OTHER_CAST, def.other_face())];
                for (bit, face) in faces {
                    let Some(face) = face.filter(|_| mask & bit != 0) else { continue };
                    for a in &self.db.def(face).alt_costs {
                        if let Some(f) = &a.cost.exile_from_hand {
                            out.push((f.clone(), ctx));
                        }
                    }
                }
                continue;
            }
            let Task::CastDone { id } = t else { continue };
            let Some(i) = self.item_index(*id) else { continue };
            let item = &self.stack[i];
            let crate::game::StackKind::Spell { card } = item.kind else { continue };
            let ctx = crate::cast::Ctx { controller: item.controller, source: Some(self.obj_ref(card)) };
            for a in &self.def_of(card).alt_costs {
                if let Some(f) = &a.cost.exile_from_hand {
                    out.push((f.clone(), ctx));
                }
            }
            // Retrace's land card to discard (CR 702.81a): the cast counted
            // on one being in hand.
            if item.from == crate::game::CastFrom::Retrace {
                if let Some(f) = &crate::cast::retrace_cost().discard_filter {
                    out.push((f.clone(), ctx));
                }
            }
        }
        out
    }

    /// Re-run the asker of the pending decision so its options reflect the
    /// current (resampled) state. Never auto-answers.
    pub(crate) fn refresh_pending(&mut self) {
        if self.pending.take().is_some() {
            self.no_auto = true;
            self.advance();
            self.no_auto = false;
            debug_assert!(self.keys.is_empty(), "refreshing a decision revealed something");
            self.keys.clear();
        }
    }
}

#[cfg(test)]
mod told_apart_tests {
    //! ENGINE.md 156: a hand card a reference of the public state tells apart (here: a card revealed
    //! for miracle, its trigger on the stack naming it, CR 702.94b).
    use super::*;
    use crate::db::CardDb;
    use crate::decision::{Choice, DecisionKind};
    use crate::game::Step;
    use crate::testkit::Scenario;

    const FANATIC: &str = "Metamorphosis Fanatic";

    /// P0 drew Metamorphosis Fanatic, its first draw, and revealed it for miracle (its trigger on the
    /// stack names it); another Fanatic, which P1 doesn't know, sits at a lower card index in P0's
    /// hand (128.6's setup). The game, the twin, the revealed card.
    fn miracle() -> (Game, CardIdx, CardIdx) {
        let mut g = Scenario::new(CardDb::constructed())
            .spec(|s| s.auto_single = false)
            .hand(0, &[FANATIC])
            .library(0, &[FANATIC, "Island", "Island"])
            .battlefield(0, &["Swamp", "Swamp"])
            .library(1, &["Island"; 8])
            .turn_number(3)
            .at(Step::Upkeep, 0)
            .start();
        let twin = g.card(0, FANATIC, Zone::Hand);
        g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
        g.choose(Choice::Yes);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Priority) && !g.stack().is_empty());
        let drawn = g.players[0].hand.iter().copied().find(|&c| c != twin && g.objs[c.i()].def == g.objs[twin.i()].def).expect("the drawn Fanatic");
        assert!(twin.get() < drawn.get());
        assert!(g.told_apart(drawn) && !g.told_apart(twin), "the trigger names the revealed card");
        assert!(g.knows(1, drawn) && !g.knows(1, twin));
        (g, twin, drawn)
    }

    fn told_keys(g: &Game, from: usize, c: CardIdx) -> usize {
        g.keys[from..].iter().filter(|&&k| key_kind(k) == KEY_TOLD && key_payload(k) == c.get() as u64).count()
    }

    /// A random pick (CR 701.9b) that takes the told-apart card is keyed with it (`KEY_TOLD`): in
    /// another world of P1's the same draw may take its twin, and every player sees which it was.
    /// One that takes the twin is not.
    #[test]
    fn a_random_pick_of_a_told_apart_card_is_keyed() {
        let (g, twin, drawn) = miracle();
        let mut took = [0, 0];
        for s in 0..64 {
            let mut w = g.clone();
            w.rng = Rng::new(s);
            let from = w.keys.len();
            let hand = w.hand_sorted(0);
            let c = w.random_hand_card(0, &hand);
            assert_eq!(told_keys(&w, from, drawn), (c == drawn) as usize, "seed {s}");
            if c == drawn {
                took[0] += 1;
            } else if c == twin {
                took[1] += 1;
            }
        }
        assert!(took[0] > 0 && took[1] > 0, "both cards drawn: {took:?}");
    }

    /// A slot takes a told-apart card the other player knows first, the same card in every world of
    /// that player's: its reveal or its leaving needs no key. One the other player doesn't know is
    /// keyed (a slot may name a twin in another world).
    #[test]
    fn a_told_apart_card_the_other_player_doesnt_know_is_keyed_as_a_slot_takes_it() {
        let (g, _, drawn) = miracle();
        let fanatic = g.objs[drawn.i()].def;
        let first = g.hand_slot_order_in(0).iter().copied().find(|&c| g.objs[c.i()].def == fanatic);
        assert_eq!(first, Some(drawn), "the slot takes the known told-apart card first, not the twin below it");
        let mut w = g.clone();
        let from = w.keys.len();
        w.key_told(drawn, false);
        assert_eq!(told_keys(&w, from, drawn), 0, "P1 knows it: no key");
        w.objs[drawn.i()].known &= !(1 << 1);
        w.key_told(drawn, false);
        assert_eq!(told_keys(&w, from, drawn), 1, "P1 doesn't know it: keyed");
    }

    /// An unknown twin leaving the hand (CR 701.9: a discard, keyed by its identity) takes no knowledge
    /// off the told-apart card: P1 still knows the revealed Fanatic is there (`knowledge_leaves`;
    /// before ENGINE.md 156 the twin rule cleared P1's knowledge of it, the "a Fanatic was there" of an
    /// untold hand). And `canon_hand_knowledge` leaves its bits alone.
    #[test]
    fn a_twin_leaving_takes_no_knowledge_off_a_told_apart_card() {
        let (mut g, twin, drawn) = miracle();
        g.canon_hand_knowledge();
        assert!(g.knows(1, drawn) && !g.knows(1, twin), "canon leaves a told-apart card's bits");
        g.discard(twin);
        assert_eq!(g.objs[twin.i()].zone, Zone::Graveyard);
        assert!(g.knows(1, drawn), "P1 still knows the revealed card");
    }

    /// ENGINE.md 210 (the fuzz steward's 88c42634bd1f): the owner rearranging its hand privately
    /// (Brainstorm's put-back, `scramble`) loses the other player track of the hand's other cards,
    /// not of the revealed miracle card (CR 702.94b): P1's knowledge of it stays, and a card P1 knew
    /// that is not told apart goes into the cloud (here the twin, made known to P1 by hand).
    #[test]
    fn a_private_rearrangement_of_the_hand_keeps_a_told_apart_cards_knowledge() {
        let (mut g, twin, drawn) = miracle();
        g.objs[twin.i()].known |= 1 << 1;
        g.scramble(0, &[], true, false);
        assert!(g.knows(1, drawn), "P1 still knows the revealed card");
        assert!(!g.knows(1, twin), "P1 lost track of the card it knew that is not told apart");
        let k = g.clouds.iter().find(|k| k.viewer == 1 && k.owner == 0 && k.hand).expect("a cloud over P0's hand");
        assert_eq!(k.defs, vec![g.objs[twin.i()].def], "the cloud lists the card P1 lost track of, not the revealed one");
    }
}
