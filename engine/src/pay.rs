//! Paying mana costs: mana sources, feasibility and the `PayMana` task
//! (CR 601.2g-h, 605, 107.4e-f, 119.4; ENGINE.md 6, 18).
//!
//! The player picks the mana abilities to activate; nothing is activated
//! until the picks cover the cost. Then everything is activated in an order
//! the rules allow (sources that keep their permanent first, life payments
//! before damage, Phyrexian life last), so every check is made against the
//! state at the start of the payment.

use crate::db::{AbilitySlot, ActivatedDef, Filter, Kw, ManaProd, SacCost};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{DamageSource, Game, Pos};
use crate::ids::{CardIdx, DefId, PlayerId, Target, Zone};
use crate::mana::{payable_within, Budget, ManaCost, ManaMask, ManaPool, Unit, COLORS};
use crate::svec::SVec;

/// The `Src` slot of an artifact tapped for improvise (CR 702.126a: not a
/// mana ability; it pays for {1} of the generic mana).
pub(crate) const IMPROVISE: AbilitySlot = AbilitySlot::Spell;

/// A mana ability that can help pay a cost: of a permanent, or of a card in
/// hand (the Spirit Guides, CR 605.3a; ENGINE.md 18); or an artifact tapped
/// for improvise (`IMPROVISE`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Src {
    pub card: CardIdx,
    pub slot: AbilitySlot,
}

impl Default for Src {
    /// (A filler for inline storage, never a real source.)
    fn default() -> Src {
        Src { card: CardIdx(0), slot: AbilitySlot::Spell }
    }
}

/// The members of a class, inline up to eight (ENGINE.md 17).
pub(crate) type Members = SVec<Src, 8>;

fn one_member(src: Src) -> Members {
    let mut m = Members::new();
    m.push(src);
    m
}

/// What a source contributes to a payment.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Yield {
    /// One mana of a type in the mask, all of it spent on the cost: these
    /// sources are chosen as a set, and which colour each pays for is a
    /// matching (ENGINE.md 6).
    One(Unit),
    /// Several mana: the excess stays in the pool, so the player picks the
    /// production. `units` describes every production at once when they
    /// are independent single mana; `None` for "n mana of one chosen type"
    /// (feasibility tries each production).
    Multi { prods: Vec<[u8; 6]>, units: Option<Vec<Unit>> },
}

/// Equivalent sources (same definition and ability), canonical order:
/// several-mana classes first, then single-mana ones.
#[derive(Clone, Debug)]
pub(crate) struct Class {
    pub def: DefId,
    pub members: Members,
    pub yld: Yield,
    /// Damage the ability's source deals to its controller (Talismans).
    pub dmg: u8,
    /// The ability sacrifices its source.
    pub sac_self: bool,
    /// Its mana's spending restriction (`ManaAbility::only`): usable only
    /// when the payment allows it (`Keep::allow`).
    pub only: u8,
}

/// What later parts of a cost need from the payment of its mana (601.2h:
/// the mana is paid first): the source itself is not a mana source, life
/// stays available, and enough permanents stay to sacrifice.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub(crate) struct Keep {
    pub exclude: Option<CardIdx>,
    pub life: u8,
    /// A later "sacrifice n permanents matching" and the card whose cost it
    /// is ("another").
    pub sac: Option<(SacCost, CardIdx)>,
    /// A later "discard n cards" and the card whose cost it is (never one
    /// of the n): mana sources in hand must leave n other cards.
    pub discard: Option<(u8, CardIdx)>,
    /// A later "exile a card matching the filter from your hand": mana
    /// sources in hand must leave one such card.
    pub exile: Option<(crate::db::Filter, CardIdx)>,
    /// The spending restrictions this payment satisfies (`SPEND_*` bits:
    /// casting an artifact / legendary spell; ENGINE.md 28).
    pub allow: u8,
    /// The spell being cast, if this pays for one ("that spell can't be
    /// countered").
    pub spell: Option<CardIdx>,
}

/// Paying a mana cost (ENGINE.md 6, 18). Stages: Phyrexian pips (how many
/// of each colour are paid with life), the pool, then sources, one class at
/// a time in non-decreasing class order.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct PayMana {
    pub payer: PlayerId,
    /// Remaining cost (after the pool and the several-mana picks).
    pub cost: ManaCost,
    /// Several-mana sources picked, with their productions.
    pub multi: Vec<(Src, [u8; 6])>,
    /// Their mana not yet spent (it stays in the pool at the end).
    pub extra: [u8; 6],
    /// The part of `extra[C]` that is artifact-only.
    pub extra_art: u8,
    /// Single-mana sources picked.
    pub chosen: Vec<Src>,
    pub min_class: u8,
    pub pooled: bool,
    /// What later parts of the cost need.
    pub keep: Keep,
    /// Life to pay for Phyrexian pips once the mana abilities are done.
    pub phyrexian_life: u8,
    /// A several-mana source whose production is being chosen, with the
    /// feasible productions.
    pub now: Option<(Src, Vec<[u8; 6]>)>,
    /// The spell has improvise (CR 702.126): untapped artifacts may pay for
    /// {1} each.
    pub improvise: bool,
    /// A waterbend cost (CR 701.67? "while paying a waterbend cost, you can
    /// tap your untapped artifacts and creatures to help; each one pays for
    /// {1}"): improvise with creatures too.
    pub waterbend: bool,
}

impl PayMana {
    pub fn new(payer: PlayerId, cost: ManaCost) -> PayMana {
        PayMana {
            payer,
            cost,
            multi: Vec::new(),
            extra: [0; 6],
            extra_art: 0,
            chosen: Vec::new(),
            min_class: 0,
            pooled: false,
            keep: Keep::default(),
            phyrexian_life: 0,
            now: None,
            improvise: false,
            waterbend: false,
        }
    }

    /// The types of untapped permanents that may pay for {1} each.
    pub(crate) fn improvise_types(&self) -> u16 {
        (if self.improvise { crate::db::types::ARTIFACT } else { 0 })
            | (if self.waterbend { crate::db::types::ARTIFACT | crate::db::types::CREATURE } else { 0 })
    }

    fn used(&self, c: CardIdx) -> bool {
        self.chosen.iter().any(|s| s.card == c) || self.multi.iter().any(|(s, _)| s.card == c)
    }
}

fn one(t: usize, n: u8) -> [u8; 6] {
    let mut v = [0u8; 6];
    v[t] = n;
    v
}

/// Every production of `prod` of source `src` for player `p`, in canonical
/// order.
pub(crate) fn productions(g: &Game, p: PlayerId, prod: &ManaProd, src: Option<CardIdx>) -> Vec<[u8; 6]> {
    match prod {
        ManaProd::One(m) => (0..6).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect(),
        ManaProd::Linked => {
            let m = src.map_or(0, |c| g.linked_colors(c));
            (0..5).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect()
        }
        ManaProd::OneOf(v) => v.clone(),
        ManaProd::Chosen => {
            let m = src.map_or(0, |c| g.chosen_land_color(c));
            (0..5).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect()
        }
        // (Read from the sacrificed land as the ability is activated.)
        ManaProd::SacrificedLand => Vec::new(),
        ManaProd::ColorAmong(f) => {
            let m = g.colors_among(p, f);
            (0..5).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect()
        }
        ManaProd::Fixed(v) => vec![*v],
        ManaProd::IfElse { cond, then, els } => vec![if g.cond_holds(cond, p) { *then } else { *els }],
        ManaProd::SameType { mask, n } => (0..6).filter(|&t| mask & (1 << t) != 0).map(|t| one(t, *n)).collect(),
        ManaProd::Combo { mask, n } => {
            let ts: Vec<usize> = (0..6).filter(|&t| mask & (1 << t) != 0).collect();
            let mut out = Vec::new();
            fn go(ts: &[usize], left: u8, cur: &mut [u8; 6], out: &mut Vec<[u8; 6]>) {
                match ts.split_first() {
                    None => {
                        if left == 0 {
                            out.push(*cur);
                        }
                    }
                    Some((&t, rest)) => {
                        for k in (0..=left).rev() {
                            cur[t] = k;
                            go(rest, left - k, cur, out);
                        }
                        cur[t] = 0;
                    }
                }
            }
            go(&ts, *n, &mut [0; 6], &mut out);
            out
        }
        ManaProd::PerPermanent { ty, filter } => {
            let n = g.count_controlled(p, filter);
            if n == 0 {
                Vec::new()
            } else {
                vec![one(*ty as usize, n.min(255) as u8)]
            }
        }
    }
}

fn units_of(v: &[u8; 6]) -> impl Iterator<Item = Unit> + '_ {
    (0..6).flat_map(move |t| std::iter::repeat_n(Unit::free(1u8 << t), v[t] as usize))
}

impl Game {
    /// The mana `p`'s permanents add whenever `p` taps a creature for mana
    /// (Badgermole Cub).
    pub(crate) fn creature_mana_bonus(&self, p: PlayerId) -> [u8; 6] {
        let mut v = [0u8; 6];
        for x in self.permanents_of(p) {
            let e = &self.abilities(x).extra_mana_creature;
            if *e != [0; 6] {
                for t in 0..6 {
                    v[t] += e[t];
                }
            }
        }
        v
    }

    /// The colours of the cards exiled with permanent `c` (CR 607.2a: an
    /// imprinted card), still in exile.
    pub(crate) fn linked_colors(&self, c: CardIdx) -> ManaMask {
        let r = self.obj_ref(c);
        self.links
            .iter()
            .filter(|l| l.source == r && self.live(l.card) && self.objs[l.card.idx.i()].zone == Zone::Exile)
            .fold(0, |m, l| m | self.colors_of(l.card.idx))
            & COLORS
    }

    /// Colours among permanents `p` controls matching `f` (Mox Amber).
    pub(crate) fn colors_among(&self, p: PlayerId, f: &Filter) -> ManaMask {
        let ctx = crate::cast::Ctx { controller: p, source: None };
        self.permanents_of(p).filter(|&c| self.card_matches(f, c, &ctx)).fold(0, |m, c| m | self.def_of(c).colors)
    }

    pub(crate) fn count_controlled(&self, p: PlayerId, f: &Filter) -> u32 {
        let ctx = crate::cast::Ctx { controller: p, source: None };
        self.permanents_of(p).filter(|&c| self.card_matches(f, c, &ctx)).count() as u32
    }

    /// The parts of an activated ability's cost and restrictions that do not
    /// depend on choices: {T} (untapped; a creature also free of summoning
    /// sickness, CR 302.6 / 602.5a), "only during your turn", "only if".
    /// `locks`: the battlefield's [`activation_locks`](Self::activation_locks),
    /// read once by the caller for many abilities.
    pub(crate) fn activation_ready(&self, p: PlayerId, card: CardIdx, ad: &ActivatedDef, locks: u16) -> bool {
        if self.activation_locked_by(card, locks) {
            return false;
        }
        let o = &self.objs[card.i()];
        if ad.cost.tap && (o.tapped || (self.is_creature(card) && self.is_sick(card) && !self.has_kw(card, Kw::Haste)))
        {
            return false;
        }
        if ad.your_turn && self.turn.active != p {
            return false;
        }
        // Boast (CR 702.142a): it attacked this turn; once each turn.
        if ad.boast {
            let r = self.obj_ref(card);
            let turn = self.turn.number as u16;
            if !self.players.iter().any(|pl| pl.attacked_with.contains(&r))
                || self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::Boasted(turn))
            {
                return false;
            }
        }
        if let Some(c) = &ad.only_if {
            if !self.cond_holds(c, p) {
                return false;
            }
        }
        true
    }

    /// "Activated abilities of [types] can't be activated" (CR 602.5) by a
    /// static ability of a permanent on the battlefield (mana abilities
    /// included).
    fn activation_locked_by(&self, card: CardIdx, locks: u16) -> bool {
        if self.objs[card.i()].zone != Zone::Battlefield {
            return false;
        }
        // (Some permanent's lock covers one of its types iff the union of
        // the locks does; its types are read only when there is a lock.)
        locks != 0 && self.types_of(card) & locks != 0
    }

    /// The card types whose activated abilities some permanent's static
    /// ability says can't be activated (CR 602.5), together.
    pub(crate) fn activation_locks(&self) -> u16 {
        self.battlefield.iter().fold(0, |m, &c| m | self.abilities(c).locks_activated)
    }

    /// The mana sources `p` can use to pay a cost, by class (ENGINE.md 18):
    /// several-mana permanents, then single-mana permanents; within each,
    /// by first appearance in canonical order.
    pub(crate) fn mana_classes(&self, p: PlayerId, exclude: Option<CardIdx>) -> Vec<Class> {
        let mut out: Vec<Class> = Vec::with_capacity(8);
        let mut n_multi = 0;
        let locks = self.activation_locks();
        // "Whenever you tap a <land subtype> for mana, add an additional
        // <mana>" (triggered mana abilities, CR 605.1b: they resolve at once,
        // so such a land makes more mana).
        // (And "whenever you tap a creature for mana, add an additional
        // {G}": one walk for both.)
        let mut bonus: Vec<(u16, u8)> = Vec::new();
        let mut creature_bonus = [0u8; 6];
        for x in self.permanents_of(p) {
            let a = self.abilities(x);
            bonus.extend(a.extra_mana_on_tap.iter().copied());
            if a.extra_mana_creature != [0; 6] {
                for t in 0..6 {
                    creature_bonus[t] += a.extra_mana_creature[t];
                }
            }
        }
        for c in self.permanents_of(p) {
            if Some(c) == exclude {
                continue;
            }
            let def = self.objs[c.i()].def;
            let d = self.abilities(c);
            for (i, ad) in d.activated.iter().enumerate() {
                let Some(m) = ad.mana() else { continue };
                if ad.instant_only || !self.activation_ready(p, c, ad, locks) || (d.latent > 0 && !self.has_activated(c, i)) {
                    continue;
                }
                // "Remove a counter from this" (Gemstone Mine): it needs
                // one; removing the last one sacrifices it (`sac_if_no`).
                let mut sac_self = ad.cost.sac_self;
                if let Some((k, n)) = ad.cost.remove_counters {
                    let have = self.counters_on(c, k);
                    if have < n as u16 {
                        continue;
                    }
                    sac_self |= m.sac_if_no == Some(k) && have == n as u16;
                }
                let src = Src { card: c, slot: AbilitySlot::Activated(i as u8) };
                // Same definition and ability: the same yield (it depends on
                // the controller only; the linked colours on the source).
                let same = |cl: &&mut Class| cl.def == def && cl.members[0].slot == src.slot && cl.sac_self == sac_self;
                if let Some(cl) = out.iter_mut().find(same).filter(|_| m.prod != ManaProd::Linked && m.prod != ManaProd::Chosen) {
                    cl.members.push(src);
                    continue;
                }
                let mut extra = [0u8; 6];
                if !bonus.is_empty() && self.is_type(c, crate::db::types::LAND) {
                    for &(sub, t) in &bonus {
                        if self.has_subtype(c, sub) {
                            extra[t as usize] += 1;
                        }
                    }
                }
                if creature_bonus != [0; 6] && ad.cost.tap && self.is_creature(c) {
                    for t in 0..6 {
                        extra[t] += creature_bonus[t];
                    }
                }
                let Some(yld) = self.yield_with(p, &m.prod, ad.cost.life, extra, Some(c)) else { continue };
                let cl = Class { def, members: one_member(src), yld, dmg: m.damage_to_you, sac_self, only: m.only };
                if matches!(cl.yld, Yield::Multi { .. }) {
                    out.insert(n_multi, cl);
                    n_multi += 1;
                } else {
                    out.push(cl);
                }
            }
        }
        // Mana abilities of cards in hand (CR 605.3a: activated while
        // paying like any other; a Spirit Guide exiles itself). One mana
        // each (the lowering checks), after the permanents.
        if self.players[p as usize].hand.iter().any(|&c| self.hand_mana_source(c)) {
            for c in self.hand_sorted_in(p) {
                if Some(c) == exclude || !self.hand_mana_source(c) {
                    continue;
                }
                let def = self.objs[c.i()].def;
                for (i, ad) in self.def_of(c).hand_abilities.iter().enumerate() {
                    let Some(m) = ad.mana() else { continue };
                    if ad.instant_only || !self.activation_ready(p, c, ad, locks) {
                        continue;
                    }
                    let src = Src { card: c, slot: AbilitySlot::Hand(i as u8) };
                    if let Some(cl) = out.iter_mut().find(|cl| cl.def == def && cl.members[0].slot == src.slot) {
                        cl.members.push(src);
                        continue;
                    }
                    let Some(yld) = self.yield_of(p, &m.prod, ad.cost.life, Some(c)) else { continue };
                    out.push(Class { def, members: one_member(src), yld, dmg: m.damage_to_you, sac_self: false, only: m.only });
                }
            }
        }
        out
    }

    /// Improvise (CR 702.126a): `p`'s untapped artifacts as sources that pay
    /// for {1} of generic mana each (a unit of no mana type: only generic
    /// pips take it), one class per definition. An artifact with a mana
    /// ability has both as alternatives (both tap it).
    pub(crate) fn improvise_classes(&self, p: PlayerId, exclude: Option<CardIdx>, types: u16) -> Vec<Class> {
        let mut out: Vec<Class> = Vec::new();
        for c in self.permanents_of(p) {
            if Some(c) == exclude || self.objs[c.i()].tapped || !self.is_type(c, types) {
                continue;
            }
            let def = self.objs[c.i()].def;
            let src = Src { card: c, slot: IMPROVISE };
            match out.iter_mut().find(|cl| cl.def == def) {
                Some(cl) => cl.members.push(src),
                None => out.push(Class { def, members: one_member(src), yld: Yield::One(Unit::free(0)), dmg: 0, sac_self: false, only: 0 }),
            }
        }
        out
    }

    /// The sources of a payment: mana abilities, and with improvise the
    /// untapped artifacts.
    pub(crate) fn pay_classes(&self, p: PlayerId, exclude: Option<CardIdx>, improvise: u16) -> Vec<Class> {
        let mut classes = self.mana_classes(p, exclude);
        if improvise != 0 {
            classes.extend(self.improvise_classes(p, exclude, improvise));
        }
        classes
    }

    /// A card in hand with a mana ability (a Spirit Guide).
    pub(crate) fn hand_mana_source(&self, c: CardIdx) -> bool {
        self.objs[c.i()].zone == Zone::Hand && self.def_of(c).hand_abilities.iter().any(|a| a.mana().is_some())
    }

    /// A source's yield with `extra` mana its tapping adds (a triggered
    /// mana ability; none for a land whose ability costs life: none in the
    /// cube has both).
    fn yield_with(&self, p: PlayerId, prod: &ManaProd, life: u8, extra: [u8; 6], src: Option<CardIdx>) -> Option<Yield> {
        let base = self.yield_of(p, prod, life, src)?;
        if extra == [0; 6] || life > 0 {
            return Some(base);
        }
        let more: Vec<Unit> = units_of(&extra).collect();
        let add = |v: &[u8; 6]| {
            let mut w = *v;
            for t in 0..6 {
                w[t] += extra[t];
            }
            w
        };
        Some(match base {
            Yield::One(u) => {
                let prods = (0..6).filter(|&t| u.mask & (1 << t) != 0).map(|t| add(&one(t, 1))).collect();
                let mut units = vec![Unit::free(u.mask)];
                units.extend(more);
                Yield::Multi { prods, units: Some(units) }
            }
            Yield::Multi { prods, units } => Yield::Multi {
                prods: prods.iter().map(add).collect(),
                units: units.map(|mut us| {
                    us.extend(more);
                    us
                }),
            },
        })
    }

    fn yield_of(&self, p: PlayerId, prod: &ManaProd, life: u8, src: Option<CardIdx>) -> Option<Yield> {
        match prod {
            ManaProd::One(m) => Some(Yield::One(Unit { life, ..Unit::free(*m) })),
            ManaProd::Linked => {
                let m = src.map_or(0, |c| self.linked_colors(c));
                (m != 0).then_some(Yield::One(Unit { life, ..Unit::free(m) }))
            }
            ManaProd::Chosen => {
                let m = src.map_or(0, |c| self.chosen_land_color(c));
                (m != 0).then_some(Yield::One(Unit { life, ..Unit::free(m) }))
            }
            ManaProd::ColorAmong(f) => {
                let m = self.colors_among(p, f) & COLORS;
                (m != 0).then_some(Yield::One(Unit { life, ..Unit::free(m) }))
            }
            _ => {
                debug_assert!(life == 0, "lowering keeps life costs to one-mana abilities");
                let prods = productions(self, p, prod, src);
                if prods.is_empty() {
                    return None;
                }
                let units = match prod {
                    ManaProd::SameType { mask, .. } if mask.count_ones() > 1 => None,
                    ManaProd::OneOf(_) => None,
                    ManaProd::Combo { mask, n } => Some(vec![Unit::free(*mask); *n as usize]),
                    _ => Some(units_of(&prods[0]).collect()),
                };
                Some(Yield::Multi { prods, units })
            }
        }
    }

    /// The budget of a payment by `p` (ENGINE.md 18). CR 119.4: a player can
    /// pay life only up to their life total; life paid later in the same
    /// cost (`later`: Phyrexian pips, a life cost) is reserved, and then the
    /// damage mana abilities deal (dealt first, in 601.2g) counts too
    /// (the returned flag). Sacrifice candidates that later parts need stay
    /// out of reach.
    fn budget(&self, p: PlayerId, keep: &Keep, later: u8) -> (Budget, bool) {
        let pl = &self.players[p as usize];
        // At most one kind of reservation per cost (the lowering checks).
        let sac = match (&keep.sac, keep.discard, &keep.exile) {
            (Some((s, card)), _, _) => self.sacrifice_candidates(p, *card, s).len() as i32 - s.n as i32,
            (None, Some((n, card)), _) => {
                let in_hand = self.objs[card.i()].zone == Zone::Hand;
                pl.hand.len() as i32 - in_hand as i32 - n as i32
            }
            (None, None, Some((f, card))) => {
                let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(*card)) };
                let n = pl.hand.iter().filter(|&&c| c != *card && self.card_matches(f, c, &ctx)).count();
                n as i32 - 1
            }
            (None, None, None) => i32::MAX,
        };
        (Budget { life: (pl.life - later as i32).max(0), sac: sac.max(0) }, later > 0)
    }

    /// Mana sources whose use takes a permanent a later sacrifice needs, or
    /// a card in hand a later discard needs.
    fn reserved(&self, p: PlayerId, keep: &Keep, cl: &Class, s: &Src) -> bool {
        match &keep.sac {
            Some((sc, card)) if cl.sac_self => {
                let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(*card)) };
                !(sc.another && s.card == *card) && self.card_matches(&sc.filter, s.card, &ctx)
            }
            _ => {
                matches!(s.slot, AbilitySlot::Hand(_))
                    && (keep.discard.is_some()
                        || keep.exile.as_ref().is_some_and(|(f, card)| {
                            let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(*card)) };
                            self.card_matches(f, s.card, &ctx)
                        }))
            }
        }
    }

    fn cands(&self, p: PlayerId, keep: &Keep, classes: &[Class], use_src: &dyn Fn(&Src) -> bool, dmg: bool) -> Cands {
        cands(classes, use_src, dmg, &|cl, s| self.reserved(p, keep, cl, s), keep.allow)
    }

    /// Can `p` pay `cost` (Phyrexian pips included) from their pool and the
    /// sources in `classes`, leaving what `keep` needs?
    pub(crate) fn can_pay_with(&self, p: PlayerId, cost: &ManaCost, classes: &[Class], keep: &Keep) -> bool {
        self.might_pay(p, cost, classes, None) && self.phyrexian_feasible(p, cost, classes, keep, keep.life, None)
    }

    /// A cheap necessary condition for paying `cost` from the pool and
    /// `classes` (but card `without`'s sources): every non-Phyrexian pip
    /// takes one mana, and every source makes at most its largest
    /// production (ENGINE.md 17: most unaffordable costs fail here, before
    /// any candidates are built).
    pub(crate) fn might_pay(&self, p: PlayerId, cost: &ManaCost, classes: &[Class], without: Option<CardIdx>) -> bool {
        let need = cost.total() as u32 - cost.phyrexian.iter().map(|&n| n as u32).sum::<u32>();
        if need == 0 {
            return true;
        }
        let mut have = self.players[p as usize].pool.total() as u32;
        for cl in classes {
            if have >= need {
                return true;
            }
            let most = match &cl.yld {
                Yield::One(_) => 1,
                Yield::Multi { prods, .. } => prods.iter().map(|v| v.iter().map(|&n| n as u32).sum::<u32>()).max().unwrap_or(0),
            };
            have += most * cl.members.iter().filter(|s| Some(s.card) != without).count() as u32;
        }
        have >= need
    }

    /// [`can_pay_with`](Self::can_pay_with) without the sources of card
    /// `without` (a card cast from hand is on the stack while its cost is
    /// paid; a source the cost taps or moves).
    pub(crate) fn can_pay_without(
        &self,
        p: PlayerId,
        cost: &ManaCost,
        classes: &[Class],
        keep: &Keep,
        without: Option<CardIdx>,
    ) -> bool {
        self.might_pay(p, cost, classes, without) && self.phyrexian_feasible(p, cost, classes, keep, keep.life, without)
    }

    /// Some split of the Phyrexian pips into life and mana leaves the rest
    /// payable from the pool and `classes` (but card `without`'s sources).
    fn phyrexian_feasible(
        &self,
        p: PlayerId,
        cost: &ManaCost,
        classes: &[Class],
        keep: &Keep,
        later: u8,
        without: Option<CardIdx>,
    ) -> bool {
        let life_total = self.players[p as usize].life;
        match (0..5).find(|&c| cost.phyrexian[c] > 0) {
            None => {
                let mut c = *cost;
                let mut pool = self.players[p as usize].pool;
                pool.pay_into_for(&mut c, keep.allow & crate::db::SPEND_ARTIFACT != 0);
                let (budget, dmg) = self.budget(p, keep, later);
                c.is_zero() || cands_payable(&c, &[], &self.cands(p, keep, classes, &|s| Some(s.card) != without, dmg), budget)
            }
            Some(col) => (0..=cost.phyrexian[col]).any(|k| {
                let later = later as i32 + 2 * k as i32;
                later <= life_total
                    && self.phyrexian_feasible(p, &split_phyrexian(cost, col, k), classes, keep, later as u8, without)
            }),
        }
    }

    /// The candidates of an unrestricted payment by `p` (computed once per
    /// priority decision).
    pub(crate) fn plain_cands(&self, p: PlayerId, classes: &[Class]) -> Cands {
        self.cands(p, &Keep::default(), classes, &|_| true, false)
    }

    /// [`can_pay_with`](Self::can_pay_with) for an unrestricted payment,
    /// with candidates from [`plain_cands`](Self::plain_cands).
    pub(crate) fn can_pay_plain(&self, p: PlayerId, cost: &ManaCost, classes: &[Class], plain: &Cands) -> bool {
        if cost.has_phyrexian() {
            return self.can_pay_with(p, cost, classes, &Keep::default());
        }
        let mut c = *cost;
        let mut pool = self.players[p as usize].pool;
        pool.pay_into(&mut c);
        let life = self.players[p as usize].life.max(0);
        c.is_zero() || cands_payable(&c, &[], plain, Budget { life, sac: i32::MAX })
    }

    /// [`can_pay`](Self::can_pay) for a payment that satisfies the spending
    /// restrictions `allow` (casting an artifact or legendary spell).
    pub(crate) fn can_pay_for(&self, p: PlayerId, cost: &ManaCost, allow: u8) -> bool {
        let classes = self.mana_classes(p, None);
        self.can_pay_with(p, cost, &classes, &Keep { allow, ..Default::default() })
    }

    /// Can `p` pay `cost` with everything available?
    pub fn can_pay(&self, p: PlayerId, cost: &ManaCost) -> bool {
        let classes = self.mana_classes(p, None);
        self.can_pay_with(p, cost, &classes, &Keep::default())
    }

    /// The action id of activating `slot` of `card` for `viewer`:
    /// `ACTIVATE` for a permanent's ability, `HAND_ABILITY` for a hand
    /// card's (by its definition's canonical hand slot).
    pub fn ability_action(&self, viewer: PlayerId, card: CardIdx, slot: AbilitySlot) -> u16 {
        match slot {
            AbilitySlot::Activated(i) => {
                let perm = self.perm_action(viewer, card) - act::MY_PERM;
                act::ACTIVATE + perm * 4 + i as u16
            }
            AbilitySlot::Hand(_) => {
                let def = self.objs[card.i()].def;
                let (a, _) = *self
                    .hand_slots_in(viewer)
                    .iter()
                    .find(|&&(_, c)| self.objs[c.i()].def == def)
                    .expect("card in hand");
                act::HAND_ABILITY + (a - act::HAND)
            }
            other => panic!("{other:?} is not activated"),
        }
    }

    /// Pay a mana ability's costs and apply its side effects (CR 605.3b:
    /// it resolves at once). The mana itself is added or spent by the
    /// caller.
    pub(crate) fn activate_mana_costs(&mut self, src: Src) {
        let c = src.card;
        let p = self.objs[c.i()].controller;
        let ad = self.def_of(c).activated_def(src.slot);
        let (cost, dmg) = (ad.cost.clone(), ad.mana().map_or(0, |m| m.damage_to_you));
        let dsrc = self.damage_source_of(c);
        self.note(|g| format!("P{p} activates a mana ability of {}", g.name(c)));
        if cost.tap {
            self.tap(c);
        }
        if let Some((k, n)) = cost.remove_counters {
            self.remove_counters(c, k, n as u16);
        }
        if cost.life > 0 {
            self.lose_life(p, cost.life as i32);
        }
        if cost.discard_hand {
            for h in self.hand_sorted(p) {
                if h != c {
                    self.discard(h);
                }
            }
        }
        if cost.sac_self {
            self.sacrifice(c);
        }
        if cost.discard_self {
            self.discard(c);
        }
        if cost.exile_self {
            self.move_card(c, Zone::Exile, Pos::Top);
        }
        if dmg > 0 {
            self.deal_damage(DamageSource { controller: p, ..dsrc }, Target::Player(p), dmg as i32);
        }
        // "If there are no <kind> counters on this, sacrifice it" (the
        // ability's effect, after its mana).
        if let Some(k) = ad.mana().and_then(|m| m.sac_if_no) {
            if self.objs[c.i()].zone == Zone::Battlefield && self.counters_on(c, k) == 0 {
                self.sacrifice(c);
            }
        }
    }

    /// Productions of the several-mana source `src` (class `ci`) that pay
    /// part of the cost and leave the rest payable by the sources of the
    /// classes from `ci` on.
    fn feasible_prods(&self, pm: &PayMana, classes: &[Class], ci: usize, src: Src) -> Vec<[u8; 6]> {
        let Yield::Multi { prods, .. } = &classes[ci].yld else { unreachable!() };
        let p = pm.payer;
        let (budget, dmg) = self.budget(p, &pm.keep, pm.keep.life + pm.phyrexian_life);
        let Some(budget) = self.after_picks(pm, classes, dmg, budget, Some((ci, src))) else { return Vec::new() };
        let rest = self.cands(p, &pm.keep, &classes[ci..], &|s| s.card != src.card && !pm.used(s.card), dmg);
        prods
            .iter()
            .copied()
            .filter(|prod| {
                let mut cost = pm.cost;
                let mut pool = ManaPool { amt: pm.extra, art: 0 };
                pool.add(prod);
                pool.pay_into_for(&mut cost, true);
                cost.total() < pm.cost.total() && (cost.is_zero() || cands_payable(&cost, &[], &rest, budget))
            })
            .collect()
    }

    /// The budget left after the picks so far (and `extra`, a source about
    /// to be picked): `None` if they exceed it.
    fn after_picks(
        &self,
        pm: &PayMana,
        classes: &[Class],
        dmg: bool,
        budget: Budget,
        extra: Option<(usize, Src)>,
    ) -> Option<Budget> {
        let class_of = |s: &Src| classes.iter().find(|cl| cl.members.contains(s)).expect("picked source class");
        let mut b = budget;
        let picks = pm.chosen.iter().chain(pm.multi.iter().map(|m| &m.0)).map(|s| (class_of(s), *s));
        let extra = extra.map(|(ci, s)| (&classes[ci], s));
        for (cl, s) in picks.chain(extra) {
            let mut life = if dmg { cl.dmg as i32 } else { 0 };
            if let Yield::One(u) = cl.yld {
                life += u.life as i32;
            }
            let sac = self.reserved(pm.payer, &pm.keep, cl, &s) as i32;
            b = Budget { life: b.life - life, sac: b.sac - sac };
            if b.life < 0 || b.sac < 0 {
                return None;
            }
        }
        Some(b)
    }

    /// The `PayMana` task (ENGINE.md 6, 18).
    pub(crate) fn pay_mana(&mut self, mut pm: PayMana) {
        let payer = pm.payer;
        // CR 107.4f: per colour, how many Phyrexian pips are paid with 2 life
        // (paid at the end, after the mana abilities).
        if let Some(col) = (0..5).find(|&c| pm.cost.phyrexian[c] > 0) {
            match self.answer.take() {
                Some(Choice::Number(k)) => {
                    pm.phyrexian_life += 2 * k;
                    pm.cost = split_phyrexian(&pm.cost, col, k);
                    self.agenda.push(Task::PayMana(pm));
                }
                Some(other) => unreachable!("Phyrexian answer {other:?}"),
                None => {
                    let classes = self.pay_classes(payer, pm.keep.exclude, pm.improvise_types());
                    let life = self.players[payer as usize].life;
                    let opts: Vec<Opt> = (0..=pm.cost.phyrexian[col])
                        .filter(|&k| {
                            let later = pm.keep.life as i32 + pm.phyrexian_life as i32 + 2 * k as i32;
                            later <= life
                                && self.phyrexian_feasible(
                                    payer,
                                    &split_phyrexian(&pm.cost, col, k),
                                    &classes,
                                    &pm.keep,
                                    later as u8,
                                    None,
                                )
                        })
                        .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k) })
                        .collect();
                    self.ask(Task::PayMana(pm), payer, DecisionKind::PayPhyrexian, opts);
                }
            }
            return;
        }
        if !pm.pooled {
            let before = self.players[payer as usize].pool;
            self.players[payer as usize].pool.pay_into_for(&mut pm.cost, pm.keep.allow & crate::db::SPEND_ARTIFACT != 0);
            pm.pooled = true;
            if !self.marks.is_empty() {
                let after = self.players[payer as usize].pool;
                let spent = (0..5).filter(|&t| before.amt[t] > after.amt[t]).fold(0u8, |m, t| m | (1 << t));
                for (_, m) in self.marks.iter_mut() {
                    if let crate::game::Mark::ConvergePending(x) = m {
                        *x |= spent;
                    }
                }
            }
        }
        if let Some((src, prods)) = pm.now.take() {
            match self.answer.take() {
                Some(Choice::Number(k)) => self.pick_multi(&mut pm, src, prods[k as usize]),
                Some(other) => unreachable!("production answer {other:?}"),
                None => {
                    let opts = (0..prods.len())
                        .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) })
                        .collect();
                    pm.now = Some((src, prods));
                    self.ask(Task::PayMana(pm), payer, DecisionKind::ChooseMana, opts);
                    return;
                }
            }
        }
        if pm.chosen.len() == pm.cost.total() as usize {
            self.finish_payment(&pm);
            return;
        }
        let classes = self.pay_classes(payer, pm.keep.exclude, pm.improvise_types());
        if let Some(ch) = self.answer.take() {
            let (card, slot) = match ch {
                Choice::Ability(card, slot) => (card, slot),
                // An artifact tapped for improvise.
                Choice::Card(card) => (card, IMPROVISE),
                other => unreachable!("pay answer {other:?}"),
            };
            let src = Src { card, slot };
            let ci = classes.iter().position(|cl| cl.members.contains(&src)).expect("source class");
            pm.min_class = ci as u8;
            if matches!(slot, AbilitySlot::Hand(_)) {
                // CR 602.2a / 605.3b: activating it from hand reveals it. Its
                // identity is public from the pick on, so no determinize
                // resamples a card the payment counts on (ENGINE.md 10).
                self.reveal(card);
            }
            match &classes[ci].yld {
                Yield::One(_) => pm.chosen.push(src),
                Yield::Multi { prods, .. } => {
                    if prods.len() == 1 {
                        self.pick_multi(&mut pm, src, prods[0]);
                    } else {
                        pm.now = Some((src, self.feasible_prods(&pm, &classes, ci, src)));
                    }
                }
            }
            self.agenda.push(Task::PayMana(pm));
            return;
        }
        let (budget, dmg) = self.budget(payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
        let unit_of = |s: &Src| -> Unit {
            match classes.iter().find(|cl| cl.members.contains(s)).map(|cl| &cl.yld) {
                Some(Yield::One(u)) => *u,
                _ => unreachable!("chosen sources make one mana"),
            }
        };
        let chosen_masks: SVec<ManaMask, 16> = pm.chosen.iter().map(|s| unit_of(s).mask).collect();
        let mut opts = Vec::new();
        for (ci, cl) in classes.iter().enumerate().skip(pm.min_class as usize) {
            if cl.only & !pm.keep.allow != 0 {
                continue; // its mana can't be spent on this (CR 106.6)
            }
            // A card already picked for one of its abilities is spent.
            let Some(&rep) = cl.members.iter().find(|s| !pm.used(s.card)) else { continue };
            let ok = match &cl.yld {
                Yield::One(u) => match self.after_picks(&pm, &classes, dmg, budget, Some((ci, rep))) {
                    None => false,
                    Some(left) => {
                        let mut ch = chosen_masks.clone();
                        ch.push(u.mask);
                        // Only classes from this one on can complete the set
                        // (the canonical order), so feasibility is checked
                        // against those; they all make one mana.
                        let rest = self.cands(
                            payer,
                            &pm.keep,
                            &classes[ci..],
                            &|s| s.card != rep.card && !pm.used(s.card),
                            dmg,
                        );
                        cands_payable(&pm.cost, &ch, &rest, left)
                    }
                },
                Yield::Multi { .. } => {
                    debug_assert!(pm.chosen.is_empty(), "several-mana classes come first");
                    !self.feasible_prods(&pm, &classes, ci, rep).is_empty()
                }
            };
            if ok {
                opts.push(if rep.slot == IMPROVISE {
                    // Improvise: the artifact's own slot (`MY_PERM`).
                    Opt { action: self.perm_action(payer, rep.card), choice: Choice::Card(rep.card) }
                } else {
                    Opt { action: self.ability_action(payer, rep.card, rep.slot), choice: Choice::Ability(rep.card, rep.slot) }
                });
            }
        }
        assert!(
            !opts.is_empty(),
            "PayMana: no way to pay {} for P{payer}: {pm:?} classes {classes:?} pool {:?}\n{}",
            pm.cost,
            self.players[payer as usize].pool,
            self.log_lines().join("\n")
        );
        self.ask(Task::PayMana(pm), payer, DecisionKind::PayMana, opts);
    }

    /// A several-mana source picked with production `prod`: its mana pays
    /// what it can of the rest now (it is activated at the end).
    fn pick_multi(&self, pm: &mut PayMana, src: Src, prod: [u8; 6]) {
        // (Artifact-only mana is spent first; what is left stays
        // artifact-only in the pool.)
        let only = self.def_of(src.card).activated_def(src.slot).mana().map_or(0, |m| m.only);
        let mut pool = ManaPool { amt: pm.extra, art: pm.extra_art };
        pool.add(&prod);
        if only & crate::db::SPEND_ARTIFACT != 0 {
            pool.art += prod[crate::mana::C];
        }
        pool.pay_into_for(&mut pm.cost, true);
        pm.extra = pool.amt;
        pm.extra_art = pool.art;
        pm.multi.push((src, prod));
    }

    /// The picks cover the cost: activate everything (CR 601.2g) in an
    /// order the rules allow, then pay Phyrexian life (601.2h). Single-mana
    /// sources' mana is spent at once (colours by matching); the unspent
    /// mana of several-mana sources stays in the pool.
    fn finish_payment(&mut self, pm: &PayMana) {
        let payer = pm.payer;
        if self.marks.iter().any(|(_, m)| matches!(m, crate::game::Mark::ConvergePending(_))) {
            self.record_converge(pm);
        }
        // "That spell can't be countered" (Delighted Halfling): its mana
        // was spent on the spell (read before the sources are activated).
        if let Some(card) = pm.keep.spell {
            let uncounterable = pm
                .chosen
                .iter()
                .chain(pm.multi.iter().map(|m| &m.0))
                .any(|s| s.slot != IMPROVISE && self.def_of(s.card).activated_def(s.slot).mana().is_some_and(|m| m.uncounterable));
            if uncounterable {
                let r = self.obj_ref(card);
                self.marks.push((r, crate::game::Mark::Uncounterable));
            }
        }
        let mut order: Vec<Src> = pm.chosen.iter().copied().chain(pm.multi.iter().map(|m| m.0)).collect();
        // Sources that keep their permanent first ("activate only if you
        // control three artifacts" still holds; counts are unchanged), life
        // payments before damage, sacrifices last.
        // (Artifacts tapped for improvise after the mana abilities, CR
        // 702.126a.)
        order.sort_by_key(|s| {
            if s.slot == IMPROVISE {
                return (true, false, false, false);
            }
            let ad = self.def_of(s.card).activated_def(s.slot);
            let dmg = ad.mana().map_or(0, |m| m.damage_to_you);
            let last = ad.mana().and_then(|m| m.sac_if_no).is_some_and(|k| self.counters_on(s.card, k) <= 1);
            (false, ad.cost.sac_self || last, dmg > 0, ad.cost.life == 0)
        });
        for src in order {
            if src.slot == IMPROVISE {
                self.note(|g| format!("P{payer} taps {} for improvise", g.name(src.card)));
                self.tap(src.card);
            } else {
                self.activate_mana_costs(src);
            }
        }
        self.add_mana(payer, &pm.extra);
        self.players[payer as usize].pool.art += pm.extra_art;
        if pm.phyrexian_life > 0 {
            self.lose_life(payer, pm.phyrexian_life as i32);
            let n = pm.phyrexian_life;
            self.note(|_| format!("P{payer} pays {n} life for Phyrexian mana"));
        }
    }

    /// Converge (ENGINE.md 28): the colours of mana spent on a spell. The
    /// pool's mana counts as spent; several-mana sources' spent mana by
    /// type; each single-mana source makes the colour the player prefers,
    /// so the count is the most colours an exact payment of the rest by
    /// those sources can show (the player chooses each mana's type as the
    /// ability resolves, CR 106.1b).
    fn record_converge(&mut self, pm: &PayMana) {
        let Some(k) = self.marks.iter().position(|(_, m)| matches!(m, crate::game::Mark::ConvergePending(_))) else { return };
        let crate::game::Mark::ConvergePending(mut fixed) = self.marks[k].1 else { unreachable!() };
        let mut made = [0u8; 6];
        for (_, v) in &pm.multi {
            for t in 0..6 {
                made[t] += v[t];
            }
        }
        for t in 0..5 {
            if made[t] > pm.extra[t] {
                fixed |= 1 << t;
            }
        }
        let classes = self.pay_classes(pm.payer, pm.keep.exclude, pm.improvise_types());
        let units: Vec<ManaMask> = pm
            .chosen
            .iter()
            .map(|s| match classes.iter().find(|cl| cl.members.contains(s)).map(|cl| &cl.yld) {
                Some(Yield::One(u)) => u.mask,
                _ => 0,
            })
            .collect();
        fn best(i: usize, units: &[ManaMask], cost: &ManaCost, got: &mut Vec<ManaMask>, fixed: u8) -> u32 {
            if i == units.len() {
                if !crate::mana::payable(cost, &[], got) {
                    return 0;
                }
                return (got.iter().fold(fixed, |m, &x| m | x) & COLORS).count_ones();
            }
            let options: SVec<ManaMask, 6> = if units[i] == 0 { [0u8].into_iter().collect() } else { (0..6).filter(|t| units[i] & (1 << t) != 0).map(|t| 1u8 << t).collect() };
            let mut b = 0;
            for o in options.iter() {
                got.push(*o);
                b = b.max(best(i + 1, units, cost, got, fixed));
                got.pop();
            }
            b
        }
        let n = if units.len() <= 7 {
            best(0, &units, &pm.cost, &mut Vec::new(), fixed)
        } else {
            // (Many sources: each colour any of them could make, at most
            // one per source.)
            let all = units.iter().fold(fixed, |m, &x| m | x) & COLORS;
            all.count_ones().min(fixed.count_ones() + units.len() as u32)
        };
        self.marks[k].1 = crate::game::Mark::Converge(n as u8);
    }

    /// The number of colours of mana spent to cast the spell `source`
    /// (converge; 0 if none was recorded).
    pub(crate) fn converge_of(&self, source: Option<crate::ids::ObjRef>) -> u8 {
        let Some(r) = source else { return 0 };
        self.marks
            .iter()
            .find_map(|&(o, m)| match m {
                crate::game::Mark::Converge(n) if o == r => Some(n),
                crate::game::Mark::ConvergePending(x) if o == r => Some(x.count_ones() as u8),
                _ => None,
            })
            .unwrap_or(0)
    }

    pub(crate) fn add_mana(&mut self, p: PlayerId, mana: &[u8; 6]) {
        self.players[p as usize].pool.add(mana);
    }
}

/// `cost` with `k` of its Phyrexian pips of colour `col` paid with life and
/// the others turned into ordinary pips.
fn split_phyrexian(cost: &ManaCost, col: usize, k: u8) -> ManaCost {
    let mut c = *cost;
    c.pips[col] += c.phyrexian[col] - k;
    c.phyrexian[col] = 0;
    c
}

/// Feasibility candidates. A card's mana abilities are mutually exclusive
/// (each taps, sacrifices or exiles it; the lowering checks), so each card
/// contributes at most one way to help. A one-mana way is a unit carrying
/// its own budget use; several single-mana ways with the same use merge into
/// one unit. Otherwise the card is a list of ways to try (each with the
/// budget its activation takes, charged once, all its mana or none). With
/// `dmg`, the damage an ability deals its controller weighs like life.
pub(crate) struct Cands {
    fixed: Units,
    alts: Vec<Vec<Way>>,
}

/// Candidate units, inline up to 32 (ENGINE.md 17).
type Units = SVec<Unit, 32>;

#[derive(Clone)]
struct Way {
    units: SVec<Unit, 4>,
    life: i32,
    sac: i32,
}

fn cands(
    classes: &[Class],
    use_src: &dyn Fn(&Src) -> bool,
    dmg: bool,
    reserved: &dyn Fn(&Class, &Src) -> bool,
    allow: u8,
) -> Cands {
    // (Sources whose mana this payment may not spend are left out.)
    let filtered: Vec<Class>;
    let classes = if classes.iter().any(|cl| cl.only & !allow != 0) {
        filtered = classes.iter().filter(|cl| cl.only & !allow == 0).cloned().collect();
        &filtered[..]
    } else {
        classes
    };
    // How many usable abilities each card has: a card with one simple way
    // goes straight to the fixed units.
    let mut count: SVec<(CardIdx, u8), 32> = SVec::new();
    for cl in classes {
        for s in cl.members.iter().filter(|s| use_src(s)) {
            match count.iter_mut().find(|(c, _)| *c == s.card) {
                Some((_, n)) => *n += 1,
                None => count.push((s.card, 1)),
            }
        }
    }
    let alone = |c: CardIdx| count.iter().any(|&(x, n)| x == c && n == 1);
    let mut out = Cands { fixed: Units::new(), alts: Vec::new() };
    let mut per_card: Vec<(CardIdx, Vec<Way>)> = Vec::new();
    for cl in classes {
        let extra = if dmg { cl.dmg as i32 } else { 0 };
        for s in cl.members.iter().filter(|s| use_src(s)) {
            let sac = reserved(cl, s) as i32;
            match &cl.yld {
                Yield::One(u) if alone(s.card) => {
                    out.fixed.push(Unit { mask: u.mask, life: (u.life as i32 + extra) as u8, sac: sac > 0 });
                    continue;
                }
                Yield::Multi { units: Some(us), .. } if alone(s.card) && extra == 0 && sac == 0 => {
                    out.fixed.extend(us.iter().map(|u| Unit::free(u.mask)));
                    continue;
                }
                _ => {}
            }
            let multi = |units: SVec<Unit, 4>| Way { units, life: extra, sac };
            let ways: Vec<Way> = match &cl.yld {
                Yield::One(u) => {
                    let mut units = SVec::new();
                    units.push(Unit::free(u.mask));
                    vec![Way { units, life: u.life as i32 + extra, sac }]
                }
                Yield::Multi { units: Some(us), .. } => {
                    vec![multi(us.iter().map(|u| Unit::free(u.mask)).collect())]
                }
                Yield::Multi { prods, units: None } => prods.iter().map(|p| multi(units_of(p).collect())).collect(),
            };
            match per_card.iter_mut().find(|(c, _)| *c == s.card) {
                Some((_, w)) => w.extend(ways),
                None => per_card.push((s.card, ways)),
            }
        }
    }
    // A single unit carries its budget use itself.
    let as_unit = |w: &Way| Unit { life: w.life as u8, sac: w.sac > 0, ..w.units[0] };
    for (_, ways) in per_card {
        let same_use = |a: &Way, b: &Way| (a.life, a.sac) == (b.life, b.sac);
        if ways.iter().all(|w| w.units.len() == 1 && same_use(w, &ways[0])) {
            let mask = ways.iter().fold(0, |m, w| m | w.units[0].mask);
            out.fixed.push(Unit { mask, ..as_unit(&ways[0]) });
        } else {
            out.alts.push(ways);
        }
    }
    out
}

/// [`payable_within`] over candidates, trying each way (or none) of the
/// cards that have several or whose activation takes budget.
fn cands_payable(cost: &ManaCost, chosen: &[ManaMask], c: &Cands, budget: Budget) -> bool {
    fn go(cost: &ManaCost, chosen: &[ManaMask], units: &mut Units, alts: &[Vec<Way>], b: Budget) -> bool {
        let Some((first, rest)) = alts.split_first() else {
            return payable_within(cost, chosen, units, b);
        };
        let use_way = |w: &Way, units: &mut Units| {
            let left = Budget { life: b.life - w.life, sac: b.sac - w.sac };
            if left.life < 0 || left.sac < 0 {
                return false;
            }
            let len = units.len();
            units.extend(w.units.iter().copied());
            let ok = go(cost, chosen, units, rest, left);
            units.truncate(len);
            ok
        };
        first.iter().any(|w| use_way(w, units)) || go(cost, chosen, units, rest, b)
    }
    if c.alts.is_empty() {
        return payable_within(cost, chosen, &c.fixed, budget);
    }
    let mut units = c.fixed.clone();
    go(cost, chosen, &mut units, &c.alts, budget)
}
