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
    /// card a sampled world dealt where.
    pub(crate) fn canon_hand_knowledge(&mut self) {
        for p in 0..2u8 {
            if self.players[p as usize].hand.len() < 2 {
                continue;
            }
            let h = self.hand_sorted(p);
            let mut i = 0;
            while i < h.len() {
                let d = self.objs[h[i].i()].def;
                let mut j = i + 1;
                while j < h.len() && self.objs[h[j].i()].def == d {
                    j += 1;
                }
                if j - i > 1 {
                    let mut bits: Vec<u8> = h[i..j].iter().map(|c| self.objs[c.i()].known).collect();
                    bits.sort_unstable_by(|a, b| b.cmp(a));
                    for (k, &c) in h[i..j].iter().enumerate() {
                        self.objs[c.i()].known = bits[k];
                    }
                }
                i = j;
            }
        }
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
        !o.zone.is_hidden() || o.known & (1 << viewer) != 0
    }

    /// The partial knowledge in the game (tests, analysis).
    pub fn clouds(&self) -> &[Cloud] {
        &self.clouds
    }

    /// Cards whose identity `viewer` does not know, per owner: the other
    /// player's hand and both libraries, minus known cards.
    pub fn unknown_to(&self, viewer: PlayerId, owner: PlayerId) -> Vec<CardIdx> {
        let pl = &self.players[owner as usize];
        let hand = if owner != viewer { &pl.hand[..] } else { &[][..] };
        hand.iter().chain(pl.library.iter()).copied().filter(|&c| !self.knows(viewer, c)).collect()
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
            if o.owner == k.owner && o.zone.is_hidden() && !v.contains(&c) {
                v.push(c);
            }
        }
        v
    }

    /// `q` learns `c`'s identity exactly (a look, a reveal) while it is in a
    /// hidden zone. A cloud whose domain holds `c` and lists its identity
    /// loses that entry (the listed card may be this one).
    pub(crate) fn learn(&mut self, q: PlayerId, c: CardIdx) {
        let o = &self.objs[c.i()];
        if o.known & (1 << q) != 0 || !o.zone.is_hidden() {
            return;
        }
        let (owner, zone, d) = (o.owner, o.zone, o.def);
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
        if o.zone.is_hidden() && o.known != 0b11 {
            let (owner, def) = (o.owner, o.def);
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
                let o = &mut self.objs[c.i()];
                if o.known & (1 << q) != 0 {
                    o.known &= !(1 << q);
                    insert_sorted(&mut k.defs, o.def);
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
        for k in &mut self.clouds {
            let (hand, library) = (k.hand, k.library);
            k.slots.retain(|&c| {
                let z = objs[c.i()].zone;
                z.is_hidden() && !((hand && z == Zone::Hand) || (library && z == Zone::Library))
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
                .map(|c| self.objs[c.i()].def)
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
        for owner in 0..2u8 {
            let mut slots = self.unknown_to(viewer, owner);
            let mut pinned: Vec<CardIdx> = Vec::new();
            if payer == Some(owner) && owner != viewer {
                // A payment in progress may count on the payer's mana
                // sources in hand, and on the cards a cost exiles from hand:
                // they are pinned (ENGINE.md 10).
                let exile = self.exile_filters_in_progress();
                slots.retain(|&c| {
                    let pin = self.hand_mana_source(c)
                        || (self.objs[c.i()].zone == Zone::Hand
                            && exile.iter().any(|(f, ctx)| self.card_matches(f, c, ctx)));
                    if pin {
                        pinned.push(c);
                    }
                    !pin
                });
            }
            let mut defs: Vec<DefId> = slots.iter().map(|c| self.objs[c.i()].def).collect();
            if !self.clouds.iter().any(|k| k.viewer == viewer && k.owner == owner) {
                rng.shuffle(&mut defs);
                for (c, d) in slots.iter().zip(defs) {
                    self.objs[c.i()].def = d;
                }
                continue;
            }
            self.resample_with_clouds(viewer, owner, &slots, &pinned, defs, rng);
        }
        self.rng = Rng::new(rng.next_u64());
        self.observer = Some(viewer);
        // The viewer's own options never name a card whose identity was
        // resampled (rule: a choice of a card in a zone hidden from its
        // decider names its definition, not its slot; the decider's hand is
        // never resampled for them). Only another player's decision needs
        // recomputing. Debug builds check the rule exactly.
        if self.pending.as_ref().is_some_and(|d| d.player != viewer) {
            self.refresh_pending();
        } else if cfg!(debug_assertions) && self.pending.is_some() {
            let mut c = self.clone();
            c.refresh_pending();
            assert_eq!(c.pending, self.pending, "determinize changed the viewer's decision");
        }
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
        let clouds: Vec<Cloud> =
            self.clouds.iter().filter(|k| k.viewer == viewer && k.owner == owner).cloned().collect();
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
            for &d in &need {
                let ok = take_one(&mut pool, d);
                debug_assert!(ok, "cloud identity {d} not in the pool");
            }
            needs.push(need);
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
            self.objs[c.i()].def = d;
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
            Task::CastPay { id } | Task::ActPay { id } | Task::PayCosts { id, .. } | Task::CastDone { id } => {
                self.item_index(*id).map(|i| self.stack[i].controller)
            }
            _ => None,
        })
    }

    /// The filters of "exile a card from your hand" costs a spell being
    /// cast may still pay (any of its alternative costs), with their
    /// context.
    pub(crate) fn exile_filters_in_progress(&self) -> Vec<(crate::db::Filter, crate::cast::Ctx)> {
        use crate::flow::Task;
        let mut out = Vec::new();
        for t in &self.agenda {
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
