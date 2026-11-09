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
        if self.supertypes_of(card) & crate::db::supertypes::LEGENDARY != 0 {
            a |= crate::db::SPEND_LEGENDARY;
        }
        let kinds = self.db.spend_kinds;
        if kinds & SPEND_ELDRAZI_SPELL != 0 && self.colorless_eldrazi(card) {
            a |= SPEND_ELDRAZI_SPELL;
        }
        let from_hand = matches!(from, CastFrom::Hand | CastFrom::Miracle)
            || (from == crate::eoe_c::FREE_CAST && self.marks.contains(&(self.obj_ref(card), crate::game::Mark::FreeFromHand)));
        if kinds & SPEND_OUTSIDE_HAND != 0 && !from_hand {
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

/// Does a card's program read a creature's last known controller
/// (`EoeW2::EventObjControllerGainsSource`)?
pub(crate) fn reads_lki_controller(d: &CardDef) -> bool {
    let reads = |ab: &crate::db::AbilityDef| ab.program.iter().any(|op| matches!(op, crate::program::Op::Ext(crate::program::Ext::EoeW2(EoeW2::EventObjControllerGainsSource))));
    d.triggers.iter().any(|t| reads(&t.ability))
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
                // battlefield, else as it last existed there (CR 608.2h,
                // `lki_toughness`, kept for this: `CardDb::watch_lki_controller`).
                if let Some(r) = f.ev.obj {
                    let live = self.live(r) && self.objs[r.idx.i()].zone == crate::ids::Zone::Battlefield;
                    let lki = self.lki_toughness.iter().rev().find(|x| x.0 == r).map(|x| x.2);
                    let p = if live { self.objs[r.idx.i()].controller } else { lki.expect("its last known controller (kept for this ability)") };
                    if self.live(f.source) {
                        self.change_control(f.source.idx, p);
                    }
                }
                crate::resolve::Flow::Next
            }
        }
    }
}

/// The land types (CR 205.3i), for "its land types become ..." (CR 305.7).
pub(crate) const LAND_TYPES: [&str; 18] = [
    "Cave", "Cloud", "Desert", "Forest", "Gate", "Island", "Lair", "Locus", "Mine", "Mountain", "Plains", "Planet", "Power-Plant", "Sphere", "Swamp", "Tower", "Town", "Urza's",
];

/// The abilities of a nonbasic land that is a Mountain (CR 305.7): its rules
/// text's are gone, and it has "{T}: Add {R}" (305.6).
pub(crate) fn mountain_def() -> &'static CardDef {
    use crate::db::{ActEffect, ActivatedDef, Cost, ManaAbility, ManaProd};
    static M: std::sync::OnceLock<CardDef> = std::sync::OnceLock::new();
    M.get_or_init(|| CardDef {
        activated: vec![ActivatedDef {
            cost: Cost { tap: true, ..Cost::default() },
            effect: ActEffect::Mana(ManaAbility {
                prod: ManaProd::One(1 << crate::mana::R),
                damage_to_you: 0,
                sac_if_no: None,
                only: 0,
                uncounterable: false,
                haste: false,
                opp_life: 0,
            }),
            sorcery: false,
            instant_only: false,
            exhaust: false,
            only_if: None,
            your_turn: false,
            loyalty: None,
            discounts: Vec::new(),
            boast: false,
            x: false,
            special: None,
        }],
        ..CardDef::default()
    })
}

impl Game {
    /// A permanent says "nonbasic lands are Mountains" (CR 305.7; one that
    /// lost its abilities doesn't).
    pub(crate) fn moon_source(&self) -> bool {
        self.battlefield.iter().any(|&x| self.db.def(self.objs[x.i()].def).nonbasic_lands_are_mountains && !self.lost_abilities(x))
    }

    /// Is the nonbasic land `c` (a land by its card types and a copy's;
    /// not basic) a Mountain because of such a permanent (CR 305.7, layer
    /// 4: 613.1d)? Wherever it is (it is about to enter: CR 614.12).
    pub(crate) fn moon_entering(&self, c: CardIdx) -> bool {
        let d = self.def_of(c);
        let types = d.types | self.exc(c).map_or(0, |e| e.types);
        types & types::LAND != 0 && d.supertypes & crate::db::supertypes::BASIC == 0 && self.moon_source()
    }

    /// The definition of a Mountain's abilities for `c` (`CardDb::moon_of`:
    /// its latent abilities kept).
    pub(crate) fn moon_def(&self, c: CardIdx) -> crate::ids::DefId {
        let d = self.objs[c.i()].def;
        self.db.moon_of.iter().find(|x| x.0 == d).map_or(self.db.moon_base, |x| x.1)
    }

    /// The timestamp of the latest permanent saying "nonbasic lands are
    /// Mountains" (CR 613.7: land types set by later effects apply after).
    pub(crate) fn moon_ts(&self) -> u32 {
        self.battlefield
            .iter()
            .filter(|&&x| self.db.def(self.objs[x.i()].def).nonbasic_lands_are_mountains && !self.lost_abilities(x))
            .map(|&x| self.objs[x.i()].timestamp)
            .max()
            .unwrap_or(0)
    }

    /// The places of permanent `c`'s own activated abilities, before those
    /// other permanents grant it (`Game::granted_activated`): its card's, or
    /// a Mountain's abilities if more (`moon_def`).
    pub(crate) fn own_ability_slots(&self, c: CardIdx) -> usize {
        let n = self.db.def(self.objs[c.i()].def).activated.len();
        if self.moon_game && self.moon_on(c) {
            n.max(self.db.def(self.moon_def(c)).activated.len())
        } else {
            n
        }
    }

    /// [`moon_entering`](Self::moon_entering) for a permanent on the
    /// battlefield.
    pub(crate) fn moon_on(&self, c: CardIdx) -> bool {
        self.objs[c.i()].zone == crate::ids::Zone::Battlefield && self.moon_entering(c)
    }
}

impl Game {
    /// The definition whose activated abilities a mana source's slot names
    /// (`pay::Src`): its own, or a Mountain's abilities while it is one
    /// (`mountain_def`; ENGINE.md 99.12).
    pub(crate) fn source_def(&self, c: CardIdx) -> &'static CardDef {
        if self.moon_game && self.moon_on(c) {
            self.db.def(self.moon_def(c))
        } else {
            self.def_of(c)
        }
    }
}

impl Game {
    /// The lands that entered the battlefield under `p`'s control this turn
    /// (`Game::lands_entered`).
    pub(crate) fn lands_entered_this_turn(&self, p: PlayerId) -> u8 {
        if self.lands_entered.0 == self.turn.number {
            self.lands_entered.1[p as usize]
        } else {
            0
        }
    }

    /// Permanent `c` is entering the battlefield (CR 614.1c, 614.12): a
    /// creature gets an additional +1/+1 counter per land that entered under
    /// its controller's control this turn for each of that player's
    /// permanents that says so (Bioengineered Future); then a land is
    /// counted.
    pub(crate) fn lands_entering(&mut self, c: CardIdx) {
        let p = self.objs[c.i()].controller;
        if self.is_creature(c) {
            let n = self.lands_entered_this_turn(p);
            let k = self.permanents_of(p).filter(|&x| x != c && self.abilities(x).etb_counter_per_land_entered).count();
            if n > 0 && k > 0 {
                self.add_counters(c, crate::db::Counter::P1P1, (n as usize * k) as crate::num::Count);
            }
        }
        if self.is_type(c, types::LAND) {
            let t = self.turn.number;
            if self.lands_entered.0 != t {
                self.lands_entered = (t, [0; 2]);
            }
            let x = &mut self.lands_entered.1[p as usize];
            *x = x.saturating_add(1);
        }
    }
}

impl Game {
    /// `p`'s permanents ready to activate a "{T}, Tap an untapped permanent
    /// you control: Add <mana>" ability (Gene Pollinator; `pay::POL_FOR`),
    /// in battlefield order.
    pub(crate) fn pol_activators(&self, p: PlayerId) -> Vec<CardIdx> {
        let locks = self.activation_locks();
        self.permanents_of(p)
            .filter(|&c| {
                let d = self.abilities(c);
                d.activated.iter().enumerate().any(|(i, ad)| {
                    ad.mana().is_some()
                        && ad.cost.tap
                        && ad.cost.tap_untapped.is_some()
                        && !ad.instant_only
                        && self.activation_ready(p, c, ad, locks)
                        && !(d.latent > 0 && !self.has_activated(c, i))
                        && !(ad.exhaust && self.exhausted(c, i as u8))
                })
            })
            .collect()
    }

    /// The sources of the Pollinators' "{T}, Tap an untapped permanent you
    /// control: Add <mana>" (`pol`: each ready one's mana and filter): one
    /// per untapped permanent it may tap (CR 602.2b, 601.2g), the activating
    /// Pollinator tapped as well: `share` = how many can activate, a
    /// permanent that could activate itself taking two (`taps`). Its id
    /// is the permanent's own (`MY_PERM`; which source asked when it stands
    /// for several, `PayMana::which`).
    /// With a "whenever you tap a creature for mana" bonus (CR 605.1b) and
    /// creatures activating, each source makes that mana as well (a
    /// several-mana source then, inserted at `n_multi`; the number inserted
    /// returned).
    pub(crate) fn pol_classes(
        &self,
        p: PlayerId,
        exclude: Option<CardIdx>,
        pol: &[(CardIdx, crate::mana::ManaMask, &crate::db::Filter)],
        creature_bonus: [u8; 6],
        n_multi: usize,
        out: &mut Vec<crate::pay::Class>,
    ) -> usize {
        use crate::pay::{Class, Src, Yield, POL_FOR, TAP};
        let (_, mask, filter) = pol[0];
        // (The bonus: when every Pollinator that can activate is a creature,
        // as the one activated then is; with some that are not, it depends on
        // which activates: not modelled, a loud failure.)
        let bonus = creature_bonus != [0; 6];
        if bonus {
            assert!(pol.iter().all(|x| self.is_creature(x.0)), "Pollinators of which some are creatures with a creature-mana bonus (ENGINE.md 99.12)");
        }
        let yld = if bonus {
            let prods: Vec<[u8; 6]> = (0..6)
                .filter(|&t| mask & (1 << t) != 0)
                .map(|t| {
                    let mut v = creature_bonus;
                    v[t] += 1;
                    v
                })
                .collect();
            let mut units = vec![crate::mana::Unit::free(mask)];
            for t in 0..6 {
                units.extend(std::iter::repeat_n(crate::mana::Unit::free(1 << t), creature_bonus[t] as usize));
            }
            Yield::Multi { prods, units: Some(units) }
        } else {
            Yield::One(crate::mana::Unit::free(mask))
        };
        let mut inserted = 0;
        debug_assert!(pol.iter().all(|x| x.1 == mask && x.2 == filter), "one kind of Pollinator ability (CardDb::new checks)");
        let actors: Vec<CardIdx> = pol.iter().map(|x| x.0).collect();
        let k = actors.len().min(u8::MAX as usize) as u8;
        let ctx = crate::cast::Ctx { controller: p, source: None };
        for x in self.permanents_of(p) {
            if Some(x) == exclude || self.objs[x.i()].tapped || !self.card_matches(filter, x, &ctx) {
                continue;
            }
            let taps = if actors.contains(&x) { 2 } else { 1 };
            if taps > k {
                continue;
            }
            let def = self.objs[x.i()].def;
            let src = Src { card: x, slot: POL_FOR };
            match out.iter_mut().find(|cl| cl.def == def && cl.members[0].slot == POL_FOR && cl.taps == taps) {
                Some(cl) => cl.members.push(src),
                None => {
                    let cl = Class { res: TAP, share: k, taps, ..Class::of(def, src, yld.clone()) };
                    if bonus {
                        out.insert(n_multi + inserted, cl);
                        inserted += 1;
                    } else {
                        out.push(cl);
                    }
                }
            }
        }
        inserted
    }

    /// A Pollinator that could activate, tapped for improvise or convoke,
    /// can't activate any more: those members take one of the Pollinators'
    /// `share` (ENGINE.md 99.12).
    pub(crate) fn pol_split(&self, p: PlayerId, classes: &mut Vec<crate::pay::Class>) {
        use crate::pay::{Members, IMPROVISE};
        let actors = self.pol_activators(p);
        let k = actors.len().min(u8::MAX as usize) as u8;
        if k == 0 {
            return;
        }
        let mut split = Vec::new();
        for cl in classes.iter_mut().filter(|cl| cl.members[0].slot == IMPROVISE) {
            let (mut a, mut rest) = (Members::new(), Members::new());
            for s in cl.members.iter() {
                if actors.contains(&s.card) {
                    a.push(*s);
                } else {
                    rest.push(*s);
                }
            }
            if !a.is_empty() {
                split.push(crate::pay::Class { members: a, share: k, taps: 1, ..cl.clone() });
                cl.members = rest;
            }
        }
        classes.retain(|cl| !cl.members.is_empty());
        classes.extend(split);
    }
}

impl Game {
    /// Devour land n (CR 702.82a): as `card` enters, its controller chooses
    /// lands to sacrifice, one decision each (`Sacrifice`: a land after the
    /// last one chosen, in battlefield order, so each set once; `PASS`
    /// stops). Nothing happens until the choice ends (re-entrant: ENGINE.md
    /// 99.8): then they are sacrificed together, and it gets n +1/+1
    /// counters for each.
    pub(crate) fn devour(&mut self, card: CardIdx, mut picked: Vec<CardIdx>) {
        use crate::decision::{act, Choice, DecisionKind, Opt};
        let p = self.objs[card.i()].controller;
        match self.answer.take() {
            Some(Choice::Card(c)) => picked.push(c),
            Some(Choice::Pass) => return self.devour_done(card, &picked),
            Some(other) => unreachable!("devour answer {other:?}"),
            None => {}
        }
        let after = picked.last().and_then(|l| self.battlefield.iter().position(|x| x == l));
        let cands: Vec<CardIdx> = self
            .battlefield
            .iter()
            .enumerate()
            .filter(|&(i, &c)| after.is_none_or(|a| i > a) && c != card && self.objs[c.i()].controller == p && self.is_type(c, types::LAND))
            .map(|(_, &c)| c)
            .collect();
        if cands.is_empty() || self.objs[card.i()].zone != crate::ids::Zone::Battlefield {
            return self.devour_done(card, &picked);
        }
        let mut opts: Vec<Opt> = cands.iter().map(|&c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) }).collect();
        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        self.ask(crate::flow::Task::Devour { card, picked }, p, DecisionKind::Sacrifice, opts);
    }

    fn devour_done(&mut self, card: CardIdx, picked: &[CardIdx]) {
        // (The counters it enters with, put by its controller: EoE W6.)
        let since = match self.w6.deferred_since.iter().position(|x| x.0 == card) {
            Some(k) => self.w6.deferred_since.remove(k),
            None => (card, self.objs[card.i()].timestamp),
        };
        if picked.is_empty() {
            return;
        }
        let n = self.def_of(card).devour_land as usize * picked.len();
        self.note(|g| format!("{} devours {} land(s)", g.name(card), picked.len()));
        self.begin_batch();
        for &c in picked {
            if self.objs[c.i()].zone == crate::ids::Zone::Battlefield {
                self.sacrifice(c);
            }
        }
        self.end_batch();
        if self.objs[card.i()].zone == crate::ids::Zone::Battlefield {
            let before = self.w6.entering.replace(since);
            self.add_counters(card, crate::db::Counter::P1P1, n.min(u16::MAX as usize) as crate::num::Count);
            self.w6.entering = before;
        }
    }
}
