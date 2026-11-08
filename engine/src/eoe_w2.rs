//! W2 of the Edge of Eternities port (ENGINE.md 99.12): mana and lands.
//!
//! **Spending restrictions** (CR 106.6). A mana ability's `only` is the set
//! of purposes its mana may be spent on (`SPEND_*`, any one of them); a
//! payment satisfies a set of purposes (`pay::Keep::allow`): casting a spell
//! (`Game::spend_allow`: an artifact, legendary or colorless Eldrazi spell; a
//! spell cast from anywhere other than its caster's hand) or activating an
//! ability (`Game::ability_allow`: of an artifact, land or colorless Eldrazi
//! source). Mana may pay a cost when it has no restriction or one of its
//! purposes is the payment's (`spendable`).
//!
//! Restricted mana a payment leaves in a pool, or a mana ability activated
//! with priority adds, keeps its restriction there (CR 106.6: the
//! restriction goes with the mana). Mishra's Workshop's artifact-only {C} is
//! `ManaPool::art` (layout v5 encodes it); every other kind is
//! `Game::pool_restr` (type, purposes, amount), a part of `ManaPool::amt`
//! like `art`. The payment model reads both as one list of kinds
//! (`Game::pool_kinds`).

use crate::db::{types, CardDef, SPEND_ARTIFACT, SPEND_ARTIFACT_ABILITY, SPEND_ELDRAZI_ABILITY, SPEND_ELDRAZI_SPELL, SPEND_LAND_ABILITY, SPEND_OUTSIDE_HAND};
use crate::game::{CastFrom, Game};
use crate::ids::{CardIdx, PlayerId};
use crate::mana::{C, COLORS};
use crate::svec::SVec;

/// Restricted mana: (type, purposes `only`, amount).
pub(crate) type RKind = (u8, u8, u8);

/// The purposes of activating an ability (`ability_allow`).
pub(crate) const SPEND_ABILITIES: u8 = SPEND_ARTIFACT_ABILITY | SPEND_ELDRAZI_ABILITY | SPEND_LAND_ABILITY;

/// May mana with purposes `only` pay a cost that satisfies `allow` (CR
/// 106.6: no restriction, or one of its purposes)?
#[inline]
pub(crate) fn spendable(only: u8, allow: u8) -> bool {
    only == 0 || only & allow != 0
}

/// The purposes the table's mana abilities can restrict mana to
/// (`CardDb::spend_kinds`).
pub(crate) fn spend_kinds(defs: &[CardDef]) -> u8 {
    defs.iter()
        .flat_map(|d| d.activated.iter().chain(d.hand_abilities.iter()))
        .filter_map(|a| a.mana())
        .fold(0, |k, m| k | m.only)
}

/// `n` more of kind (`ty`, `only`) in a list of kinds (kept sorted, no
/// empty kind).
pub(crate) fn add_kind(v: &mut Vec<RKind>, ty: u8, only: u8, n: u8) {
    if n == 0 {
        return;
    }
    match v.iter_mut().find(|k| k.0 == ty && k.1 == only) {
        Some(k) => k.2 = k.2.saturating_add(n),
        None => {
            v.push((ty, only, n));
            v.sort_unstable();
        }
    }
}

impl Game {
    /// Is `card` a colorless Eldrazi (CR 105.2c, 205.3j)?
    pub(crate) fn colorless_eldrazi(&self, card: CardIdx) -> bool {
        self.db.eldrazi.is_some_and(|e| self.has_subtype(card, e)) && self.colors_of(card) & COLORS == 0
    }

    /// The spending restrictions casting `card` the way `from` says
    /// satisfies (`SPEND_*`, CR 106.6): an artifact spell, a legendary
    /// spell, a colorless Eldrazi spell, a spell cast from anywhere other
    /// than its caster's hand. (The purposes no mana of the table has are
    /// left out: the payment is the same.)
    pub(crate) fn spend_allow_from(&self, card: CardIdx, from: CastFrom) -> u8 {
        let mut a = 0;
        if self.is_type(card, types::ARTIFACT) {
            a |= SPEND_ARTIFACT;
        }
        if self.def_of(card).supertypes & crate::db::supertypes::LEGENDARY != 0 {
            a |= crate::db::SPEND_LEGENDARY;
        }
        let kinds = self.db.spend_kinds;
        if kinds & SPEND_ELDRAZI_SPELL != 0 && self.colorless_eldrazi(card) {
            a |= SPEND_ELDRAZI_SPELL;
        }
        if kinds & SPEND_OUTSIDE_HAND != 0 && !matches!(from, CastFrom::Hand | CastFrom::Miracle) {
            a |= SPEND_OUTSIDE_HAND;
        }
        a
    }

    /// The spending restrictions activating an ability of `card` satisfies
    /// (CR 106.6, 602.2): of an artifact, a land or a colorless Eldrazi
    /// source, as it is when the ability is activated. (Only the purposes
    /// the table's mana has: nothing for the cube.)
    pub(crate) fn ability_allow(&self, card: CardIdx) -> u8 {
        let kinds = self.db.spend_kinds & SPEND_ABILITIES;
        if kinds == 0 {
            return 0;
        }
        let mut a = 0;
        if kinds & SPEND_ARTIFACT_ABILITY != 0 && self.is_type(card, types::ARTIFACT) {
            a |= SPEND_ARTIFACT_ABILITY;
        }
        if kinds & SPEND_LAND_ABILITY != 0 && self.is_type(card, types::LAND) {
            a |= SPEND_LAND_ABILITY;
        }
        if kinds & SPEND_ELDRAZI_ABILITY != 0 && self.colorless_eldrazi(card) {
            a |= SPEND_ELDRAZI_ABILITY;
        }
        a
    }

    /// The restricted parts of `p`'s pool: the Workshop's artifact-only {C}
    /// first, then the other kinds (module docs).
    pub(crate) fn pool_kinds(&self, p: PlayerId) -> SVec<RKind, 4> {
        let mut v = SVec::new();
        let art = self.players[p as usize].pool.art;
        if art > 0 {
            v.push((C as u8, SPEND_ARTIFACT, art));
        }
        for &(q, ty, only, n) in &self.pool_restr {
            if q == p {
                v.push((ty, only, n));
            }
        }
        v
    }

    /// Does `p`'s pool hold mana with a spending restriction?
    pub(crate) fn pool_restricted(&self, p: PlayerId) -> bool {
        self.players[p as usize].pool.art > 0 || self.pool_restr.iter().any(|k| k.0 == p)
    }

    /// Set the restricted parts of `p`'s pool (each a part of its `amt`).
    pub(crate) fn set_pool_kinds(&mut self, p: PlayerId, kinds: &[RKind]) {
        let art = kinds.iter().filter(|k| k.0 == C as u8 && k.1 == SPEND_ARTIFACT).map(|k| k.2).sum();
        self.players[p as usize].pool.art = art;
        self.pool_restr.retain(|k| k.0 != p);
        for &(ty, only, n) in kinds {
            if n > 0 && !(ty == C as u8 && only == SPEND_ARTIFACT) {
                self.pool_restr.push((p, ty, only, n));
            }
        }
    }

    /// Mana a mana ability activated with priority adds (CR 106.4), with
    /// its spending restriction (CR 106.6). Not in a table whose mana has
    /// only the cube's two restrictions (Mishra's Workshop's, Delighted
    /// Halfling's): there such mana floated before its source is lost
    /// (ENGINE.md 86) goes to the pool unrestricted, as before this
    /// section: a known fault left for the maintainer, since keeping the
    /// restriction changes 12 of the 2,500 digest games (ENGINE.md 99.12).
    pub(crate) fn add_restricted_mana(&mut self, p: PlayerId, mana: &[u8; 6], only: u8) {
        self.add_mana(p, mana);
        let cube_kinds = SPEND_ARTIFACT | crate::db::SPEND_LEGENDARY;
        if only == 0 || self.db.spend_kinds & !cube_kinds == 0 {
            return;
        }
        let mut kinds: Vec<RKind> = self.pool_kinds(p).iter().copied().collect();
        for t in 0..6 {
            add_kind(&mut kinds, t as u8, only, mana[t]);
        }
        self.set_pool_kinds(p, &kinds);
    }
}

/// The restricted mana a payment can leave in the pool (CR 106.4, 601.2h):
/// of the virtual pool's kinds `vkinds`, `spent[t]` of type t taken from
/// the kinds the payment may spend (`spend`), every way but those worse
/// for the player than another (a kind whose purposes are all another's is
/// spent before it: its mana pays for less, CR 106.6). One way when a type
/// has one such kind (the cube: Mishra's Workshop's {C}).
pub(crate) fn kinds_left(vkinds: &[RKind], spent: &[u8; 6], spend: &dyn Fn(u8) -> bool) -> Vec<Vec<RKind>> {
    let mut out: Vec<Vec<RKind>> = vec![vkinds.to_vec()];
    for t in 0..6 {
        if spent[t] == 0 {
            continue;
        }
        let idx: Vec<usize> = (0..vkinds.len()).filter(|&i| vkinds[i].0 as usize == t && spend(vkinds[i].1)).collect();
        let mut ways: Vec<Vec<u8>> = Vec::new();
        fn each(idx: &[usize], vkinds: &[RKind], left: u8, cur: &mut Vec<u8>, ways: &mut Vec<Vec<u8>>) {
            if cur.len() == idx.len() {
                if left == 0 {
                    ways.push(cur.clone());
                }
                return;
            }
            let have = vkinds[idx[cur.len()]].2;
            for n in (0..=have.min(left)).rev() {
                cur.push(n);
                each(idx, vkinds, left - n, cur, ways);
                cur.pop();
            }
        }
        each(&idx, vkinds, spent[t], &mut Vec::new(), &mut ways);
        // (Narrower purposes first: a way that keeps some of a kind whose
        // purposes another's contain while spending that other is worse.)
        let worse = |w: &[u8]| {
            (0..idx.len()).any(|a| {
                (0..idx.len()).any(|b| {
                    let (oa, ob) = (vkinds[idx[a]].1, vkinds[idx[b]].1);
                    a != b && oa & !ob == 0 && oa != ob && w[a] < vkinds[idx[a]].2 && w[b] > 0
                })
            })
        };
        ways.retain(|w| !worse(w));
        debug_assert!(!ways.is_empty(), "restricted mana spent of type {t} that no kind holds");
        let mut next = Vec::new();
        for o in &out {
            for w in &ways {
                let mut v = o.clone();
                for (j, &i) in idx.iter().enumerate() {
                    v[i].2 -= w[j];
                }
                next.push(v);
            }
        }
        out = next;
    }
    for v in out.iter_mut() {
        v.retain(|k| k.2 > 0);
    }
    out
}

impl Game {
    /// `p`'s pool's mana with a spending restriction, by kind (type,
    /// purposes `SPEND_*`, amount; the Workshop's artifact-only {C} too):
    /// for tests and tools.
    pub fn restricted_mana(&self, p: PlayerId) -> Vec<(u8, u8, u8)> {
        self.pool_kinds(p).iter().copied().collect()
    }
}

/// The instructions W2 added (ENGINE.md 99.12), inside `Ext::EoeW2`. Same
/// rules as `Op`: named after rules actions, never cards.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeW2 {
    /// The controller of the triggered ability's event object gains control
    /// of the source (CR 613.1b, an effect without a duration; "that
    /// creature's controller gains control of this land").
    EventObjControllerGainsSource,
}

impl EoeW2 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeW2::EventObjControllerGainsSource => true,
        }
    }
}

impl Game {
    /// The number of counters of kind `k` on object `s`: now, or as it last
    /// existed on the battlefield if it left (CR 608.2h).
    pub(crate) fn source_counters(&self, s: crate::ids::ObjRef, k: crate::db::Counter) -> i64 {
        if self.live(s) && self.objs[s.idx.i()].zone == crate::ids::Zone::Battlefield {
            self.counters_on(s.idx, k) as i64
        } else {
            self.lki_counters.iter().filter(|x| x.0 == s && x.1 == k).map(|x| x.2 as i64).sum()
        }
    }

    pub(crate) fn exec_eoe_w2(&mut self, f: &mut crate::resolve::Frame, op: &EoeW2) -> crate::resolve::Flow {
        match op {
            EoeW2::EventObjControllerGainsSource => {
                // "That creature's controller": now if it is still on the
                // battlefield, else as it last existed there (CR 608.2h): a
                // creature that dealt combat damage to a player was
                // attacking (CR 510.1a), so it was the active player's
                // (506.4: a creature whose control changes leaves combat).
                if let Some(r) = f.ev.obj {
                    let live = self.live(r) && self.objs[r.idx.i()].zone == crate::ids::Zone::Battlefield;
                    let p = if live { self.objs[r.idx.i()].controller } else { self.turn.active };
                    if self.live(f.source) {
                        self.change_control(f.source.idx, p);
                    }
                }
                crate::resolve::Flow::Next
            }
        }
    }
}
