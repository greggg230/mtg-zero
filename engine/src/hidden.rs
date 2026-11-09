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
use crate::ids::{CardIdx, DefId, PlayerId, Zone};
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
/// Payload of a private key emitted while another player is the observer.
pub const HIDDEN: u64 = 0xFFFF_FFFF;

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

/// The rare-rule bits (`Game::seen`) of each definition of `db`, its back
/// face's included (as `Game::add_card` sets them; not the per-player
/// miracle bits); computed once.
fn seen_bits_of(db: &'static crate::db::CardDb) -> &'static [u16] {
    static BITS: std::sync::OnceLock<(usize, Vec<u16>)> = std::sync::OnceLock::new();
    let key = db as *const crate::db::CardDb as usize;
    let b = BITS.get_or_init(|| {
        let v = (0..db.len())
            .map(|i| {
                let d = db.def(i as DefId);
                d.seen_bits() | d.back.map_or(0, |b| db.def(b).seen_bits())
            })
            .collect();
        (key, v)
    });
    assert_eq!(b.0, key, "one card database per process");
    &b.1
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
    /// card a sampled world dealt where. But a hand card a trigger, a stack
    /// item or a delayed trigger names (its event's object: a card revealed
    /// for miracle is played revealed until its ability resolves, CR
    /// 702.94b) is told apart from its twins: it keeps its bits, or a world
    /// `determinize` deals for another player could give that slot another
    /// identity while the ability still acts on it (ENGINE.md 71.3).
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
                    let twins: SVec<CardIdx, 16> = h[i..j].iter().copied().filter(|&c| !self.named_by_an_ability(c)).collect();
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

    /// Does a waiting trigger, a stack item or a delayed trigger name card
    /// `c` as its event's object (`canon_hand_knowledge`)?
    fn named_by_an_ability(&self, c: CardIdx) -> bool {
        let names = |ev: &crate::triggers::EvCtx| [ev.obj, ev.obj2].into_iter().flatten().any(|r| r.idx == c && self.live(r));
        self.waiting.iter().any(|w| names(&w.ev)) || self.stack.iter().any(|s| names(&s.ev)) || self.delayed.iter().any(|d| names(&d.ev))
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
        for k in self.clouds.iter_mut().filter(|k| k.viewer == q) {
            if Self::in_domain_at(k, owner, c, zone) {
                take_one(&mut k.defs, d);
                k.slots.retain(|&x| x != c);
            }
        }
        self.objs[c.i()].known |= 1 << q;
        self.tidy_clouds();
    }

    /// Reveal a card in a hidden zone to everyone (CR 701.16; 602.2a).
    pub(crate) fn reveal(&mut self, idx: CardIdx) {
        let o = &self.objs[idx.i()];
        if self.hidden_obj(idx) && o.known != 0b11 {
            let (owner, def) = (o.owner, self.ident(idx));
            for q in 0..2 {
                self.learn(q, idx);
            }
            self.emit_public(KEY_REVEAL, owner, def as u64);
        }
    }

    /// Card `c` (definition `d`) left hidden zone `from` for a public zone;
    /// `known_before` were its knowledge bits. Every viewer who did not know
    /// it exactly sees one card of identity `d` leave: a cloud whose domain
    /// held it loses one `d`; if it left the hand, where cards are not told
    /// apart, another card of the hand known to be a `d` may have been the
    /// one, so that exact knowledge becomes "a `d` was there" (the cloud's
    /// entry stays, or the card is no longer known).
    pub(crate) fn knowledge_leaves(&mut self, c: CardIdx, owner: PlayerId, from: Zone, d: DefId, known_before: u8) {
        for q in 0..2u8 {
            if q == owner || known_before & (1 << q) != 0 {
                for k in self.clouds.iter_mut().filter(|k| k.viewer == q) {
                    k.slots.retain(|&x| x != c);
                }
                continue;
            }
            let twin = (from == Zone::Hand)
                .then(|| {
                    self.players[owner as usize]
                        .hand
                        .iter()
                        .copied()
                        .find(|&x| self.objs[x.i()].def == d && self.objs[x.i()].known & (1 << q) != 0)
                })
                .flatten();
            for k in self.clouds.iter_mut().filter(|k| k.viewer == q) {
                if Self::in_domain_at(k, owner, c, from) {
                    k.slots.retain(|&x| x != c);
                    if twin.is_none() {
                        take_one(&mut k.defs, d);
                    }
                }
            }
            if let Some(t) = twin {
                // The known `d` in hand becomes one of the hand's unknowns;
                // a cloud over the hand keeps "a `d` is there" if it lists
                // one, else nothing remains known about it.
                self.objs[t.i()].known &= !(1 << q);
            }
        }
        self.tidy_clouds();
    }

    /// Card `c` moved between hidden zones in a way every player sees (a
    /// draw: the top card goes to the hand; the second step of a private
    /// move whose rearrangement was already scrambled). A cloud whose
    /// domain held it still holds it.
    pub(crate) fn knowledge_moves(&mut self, c: CardIdx, owner: PlayerId, from: Zone, to: Zone) {
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
    /// domain touches, become one cloud.
    pub(crate) fn scramble(&mut self, owner: PlayerId, slots: &[CardIdx], hand: bool, library: bool) {
        for q in 0..2u8 {
            if q == owner {
                continue;
            }
            let mut k = Cloud { viewer: q, owner, hand, library, slots: slots.to_vec(), defs: Vec::new() };
            k.slots.retain(|&c| {
                let z = self.objs[c.i()].zone;
                !((hand && z == Zone::Hand) || (library && z == Zone::Library))
            });
            for c in self.domain(&k) {
                let d = self.ident(c);
                let o = &mut self.objs[c.i()];
                if o.known & (1 << q) != 0 {
                    o.known &= !(1 << q);
                    insert_sorted(&mut k.defs, d);
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
    /// keeps them there, so it stays true in every world).
    pub(crate) fn forget_order(&mut self, owner: PlayerId, cards: &[CardIdx]) {
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
    /// empty clouds and clouds over all of the owner's hidden cards (they
    /// say nothing a determinize would not keep anyway).
    fn tidy_clouds(&mut self) {
        if self.clouds.is_empty() {
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
        self.clouds.retain(|k| !(k.defs.is_empty() || (k.hand && k.library)));
        self.clouds.sort_by_key(|k| (k.viewer, k.owner, k.hand, k.library, k.slots.first().copied()));
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
    /// finite and > 0.
    pub fn determinize_weighted(&mut self, viewer: PlayerId, rng: &mut Rng, w: &dyn Fn(DefId) -> f64) {
        let payer = self.payment_in_progress();
        for owner in 0..2u8 {
            if owner == viewer {
                self.deal_owner(viewer, owner, payer, rng);
            } else {
                let (slots, pinned) = self.hidden_slots_with(viewer, owner, payer);
                let defs: Vec<DefId> = slots.iter().map(|&c| self.ident(c)).collect();
                self.resample_weighted(viewer, owner, &slots, &pinned, defs, rng, w);
            }
        }
        self.after_deal(viewer, rng);
    }

    /// What `determinize` does after its deal: reseed, observe, refresh.
    fn after_deal(&mut self, viewer: PlayerId, rng: &mut Rng) {
        self.rng = Rng::new(rng.next_u64());
        self.observer = Some(viewer);
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
        }
    }

    /// `deal_hidden` of one owner's cards.
    fn deal_owner(&mut self, viewer: PlayerId, owner: PlayerId, payer: Option<PlayerId>, rng: &mut Rng) {
        let (slots, pinned) = self.hidden_slots_with(viewer, owner, payer);
        let mut defs: SVec<DefId, 64> = slots.iter().map(|&c| self.ident(c)).collect();
        if !self.clouds.iter().any(|k| k.viewer == viewer && k.owner == owner) {
            rng.shuffle(&mut defs);
            for (&c, &d) in slots.iter().zip(defs.iter()) {
                self.set_ident(c, d);
            }
            return;
        }
        self.resample_with_clouds(viewer, owner, &slots, &pinned, defs.to_vec(), rng);
    }

    /// `determinize_weighted`'s deal of `owner`'s cards `slots` (identities
    /// `defs`): `resample_with_clouds` with the hand weighted (ENGINE.md 69.2).
    #[allow(clippy::too_many_arguments)]
    fn resample_weighted(
        &mut self,
        viewer: PlayerId,
        owner: PlayerId,
        slots: &[CardIdx],
        pinned: &[CardIdx],
        defs: Vec<DefId>,
        rng: &mut Rng,
        w: &dyn Fn(DefId) -> f64,
    ) {
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
        // The hand's slots outside every cloud (the hand is one pool: a cloud
        // covers all of it or none): weighted draws from the pool.
        let free_hand = (0..slots.len()).filter(|&i| in_hand[i] && home[i].is_none()).count();
        let hand_ids = weighted_draws(&mut pool, free_hand, w, rng);
        rng.shuffle(&mut pool);
        let mut free = pool.into_iter();
        let mut per_cloud: Vec<Vec<DefId>> = needs;
        for (ki, v) in per_cloud.iter_mut().enumerate() {
            let mine: Vec<usize> = (0..slots.len()).filter(|&i| home[i] == Some(ki)).collect();
            while v.len() < mine.len() {
                v.push(free.next().expect("pool covers every place"));
            }
            // its hand slots (in its slot order) by the weights, the rest uniform
            let hand_pos: Vec<usize> = mine.iter().enumerate().filter(|&(_, &i)| in_hand[i]).map(|(k, _)| k).collect();
            if hand_pos.is_empty() {
                rng.shuffle(v);
            } else {
                v.sort_unstable();
                let drawn = weighted_draws(v, hand_pos.len(), w, rng);
                rng.shuffle(v);
                let mut rest = std::mem::take(v).into_iter();
                let mut d = drawn.into_iter();
                *v = (0..mine.len()).map(|k| (if hand_pos.contains(&k) { d.next() } else { rest.next() }).expect("a cloud's cards fill its places")).collect();
            }
        }
        let mut next = vec![0usize; per_cloud.len()];
        let mut hand_next = hand_ids.into_iter();
        for (i, &c) in slots.iter().enumerate() {
            let d = match home[i] {
                Some(ki) => {
                    next[ki] += 1;
                    per_cloud[ki][next[ki] - 1]
                }
                None if in_hand[i] => hand_next.next().expect("a draw for every hand slot"),
                None => free.next().expect("pool covers every place"),
            };
            self.set_ident(c, d);
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
