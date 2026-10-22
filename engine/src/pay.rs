//! Paying mana costs: mana sources, feasibility and the `PayMana` task
//! (CR 601.2g-h, 605, 107.4e-f, 119.4; ENGINE.md 6, 18).
//!
//! The player picks the mana abilities to activate; nothing is activated
//! until the picks cover the cost. Then everything is activated in an order
//! the rules allow (sources that keep their permanent first, life payments
//! before damage, Phyrexian life last), so every check is made against the
//! state at the start of the payment.

use crate::db::{AbilitySlot, ActivatedDef, Filter, Kw, ManaProd};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{DamageSource, Game, Pos};
use crate::ids::{CardIdx, DefId, PlayerId, Target, Zone};
use crate::mana::{payable_within, ManaCost, ManaMask, ManaPool, Unit, COLORS};

/// A mana ability that can help pay a cost: of a permanent, or of a card
/// in hand (a Spirit Guide).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Src {
    pub card: CardIdx,
    pub slot: AbilitySlot,
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

/// Equivalent sources (same zone, definition and ability), canonical order:
/// several-mana classes first, then permanents, then cards in hand.
#[derive(Clone, Debug)]
pub(crate) struct Class {
    pub members: Vec<Src>,
    pub yld: Yield,
    /// Damage the ability's source deals to its controller (Talismans).
    pub dmg: u8,
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
    /// Single-mana sources picked.
    pub chosen: Vec<Src>,
    pub min_class: u8,
    pub pooled: bool,
    /// Not usable as a source (the ability's own source when its cost taps
    /// or moves it).
    pub exclude: Option<CardIdx>,
    /// Life and hand cards that later parts of the cost still need.
    pub life_keep: u8,
    pub hand_keep: u8,
    /// Life to pay for Phyrexian pips once the mana abilities are done.
    pub phyrexian_life: u8,
    /// A several-mana source whose production is being chosen, with the
    /// feasible productions.
    pub now: Option<(Src, Vec<[u8; 6]>)>,
}

impl PayMana {
    pub fn new(payer: PlayerId, cost: ManaCost) -> PayMana {
        PayMana {
            payer,
            cost,
            multi: Vec::new(),
            extra: [0; 6],
            chosen: Vec::new(),
            min_class: 0,
            pooled: false,
            exclude: None,
            life_keep: 0,
            hand_keep: 0,
            phyrexian_life: 0,
            now: None,
        }
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

/// Every production of `prod` for player `p`, in canonical order.
pub(crate) fn productions(g: &Game, p: PlayerId, prod: &ManaProd) -> Vec<[u8; 6]> {
    match prod {
        ManaProd::One(m) => (0..6).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect(),
        ManaProd::ColorAmong(f) => {
            let m = g.colors_among(p, f);
            (0..5).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect()
        }
        ManaProd::Fixed(v) => vec![*v],
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
    pub(crate) fn activation_ready(&self, p: PlayerId, card: CardIdx, ad: &ActivatedDef) -> bool {
        let o = &self.objs[card.i()];
        if ad.cost.tap && (o.tapped || (self.is_creature(card) && self.is_sick(card) && !self.has_kw(card, Kw::Haste)))
        {
            return false;
        }
        if ad.your_turn && self.turn.active != p {
            return false;
        }
        if let Some(c) = &ad.only_if {
            if !self.cond_holds(c, p) {
                return false;
            }
        }
        true
    }

    /// The mana sources `p` can use to pay a cost, by class (ENGINE.md 18):
    /// several-mana permanents, then single-mana permanents, then cards in
    /// hand; within each, by first appearance in canonical order.
    pub(crate) fn mana_classes(&self, p: PlayerId, exclude: Option<CardIdx>) -> Vec<Class> {
        type L = Vec<(DefId, AbilitySlot, Class)>;
        let (mut multi, mut one, mut hand): (L, L, L) = (Vec::new(), Vec::new(), Vec::new());
        let add = |list: &mut L, def: DefId, src: Src, yld: Yield, dmg: u8| match list
            .iter_mut()
            .find(|(d, s, _)| *d == def && *s == src.slot)
        {
            Some((_, _, cl)) => cl.members.push(src),
            None => list.push((def, src.slot, Class { members: vec![src], yld, dmg })),
        };
        for c in self.permanents_of(p) {
            if Some(c) == exclude {
                continue;
            }
            let def = self.objs[c.i()].def;
            for (i, ad) in self.def_of(c).activated.iter().enumerate() {
                let Some(m) = ad.mana() else { continue };
                if ad.instant_only || !self.activation_ready(p, c, ad) {
                    continue;
                }
                let src = Src { card: c, slot: AbilitySlot::Activated(i as u8) };
                match self.yield_of(p, &m.prod, ad.cost.life) {
                    Some(y @ Yield::One(_)) => add(&mut one, def, src, y, m.damage_to_you),
                    Some(y) => add(&mut multi, def, src, y, m.damage_to_you),
                    None => {}
                }
            }
        }
        for c in self.hand_sorted(p) {
            if Some(c) == exclude {
                continue;
            }
            let def = self.objs[c.i()].def;
            for (i, ad) in self.def_of(c).hand_abilities.iter().enumerate() {
                let Some(m) = ad.mana() else { continue };
                if ad.instant_only || !self.activation_ready(p, c, ad) {
                    continue;
                }
                let src = Src { card: c, slot: AbilitySlot::Hand(i as u8) };
                let ManaProd::One(mask) = m.prod else { unreachable!("lowering keeps hand mana to one mana") };
                let unit = Unit { mask, life: ad.cost.life, hand: ad.cost.exile_self || ad.cost.discard_self };
                add(&mut hand, def, src, Yield::One(unit), m.damage_to_you);
            }
        }
        multi.into_iter().chain(one).chain(hand).map(|(_, _, cl)| cl).collect()
    }

    fn yield_of(&self, p: PlayerId, prod: &ManaProd, life: u8) -> Option<Yield> {
        match prod {
            ManaProd::One(m) => Some(Yield::One(Unit { mask: *m, life, hand: false })),
            ManaProd::ColorAmong(f) => {
                let m = self.colors_among(p, f) & COLORS;
                (m != 0).then_some(Yield::One(Unit { mask: m, life, hand: false }))
            }
            _ => {
                debug_assert!(life == 0, "lowering keeps life costs to one-mana abilities");
                let prods = productions(self, p, prod);
                if prods.is_empty() {
                    return None;
                }
                let units = match prod {
                    ManaProd::SameType { mask, .. } if mask.count_ones() > 1 => None,
                    ManaProd::Combo { mask, n } => Some(vec![Unit::free(*mask); *n as usize]),
                    _ => Some(units_of(&prods[0]).collect()),
                };
                Some(Yield::Multi { prods, units })
            }
        }
    }

    /// Life `p` may spend on mana abilities: CR 119.4 lets a player pay life
    /// only up to their life total. When life is paid later in the same
    /// cost (Phyrexian pips, a life cost), that is reserved and the damage
    /// mana abilities deal (dealt first, in 601.2g) counts too.
    fn life_budget(&self, p: PlayerId, later: u8) -> (i32, bool) {
        ((self.players[p as usize].life - later as i32).max(0), later > 0)
    }

    /// Cards from hand `p` may use for mana, keeping `keep` for a later
    /// discard (whether those exist is checked by the caller).
    fn hand_budget(&self, p: PlayerId, exclude: Option<CardIdx>, keep: u8) -> i32 {
        let h = &self.players[p as usize].hand;
        (h.iter().filter(|&&c| Some(c) != exclude).count() as i32 - keep as i32).max(0)
    }

    /// Can `p` pay `cost` (Phyrexian pips included) from their pool and the
    /// sources in `classes`, keeping `life_keep` life and `hand_keep` cards?
    pub(crate) fn can_pay_with(
        &self,
        p: PlayerId,
        cost: &ManaCost,
        classes: &[Class],
        exclude: Option<CardIdx>,
        life_keep: u8,
        hand_keep: u8,
    ) -> bool {
        let hand = self.hand_budget(p, exclude, hand_keep);
        self.phyrexian_feasible(p, cost, classes, life_keep, hand)
    }

    /// Some split of the Phyrexian pips into life and mana leaves the rest
    /// payable from the pool and `classes`.
    fn phyrexian_feasible(&self, p: PlayerId, cost: &ManaCost, classes: &[Class], later: u8, hand: i32) -> bool {
        let life_total = self.players[p as usize].life;
        match (0..5).find(|&c| cost.phyrexian[c] > 0) {
            None => {
                let mut c = *cost;
                let mut pool = self.players[p as usize].pool;
                pool.pay_into(&mut c);
                let (life, dmg) = self.life_budget(p, later);
                c.is_zero() || cands_payable(&c, &[], &cands(classes, &|_| true, dmg), life, hand)
            }
            Some(col) => (0..=cost.phyrexian[col]).any(|k| {
                let later = later as i32 + 2 * k as i32;
                later <= life_total
                    && self.phyrexian_feasible(p, &split_phyrexian(cost, col, k), classes, later as u8, hand)
            }),
        }
    }

    /// Can `p` pay `cost` with everything available?
    pub fn can_pay(&self, p: PlayerId, cost: &ManaCost) -> bool {
        let classes = self.mana_classes(p, None);
        self.can_pay_with(p, cost, &classes, None, 0, 0)
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
                let (a, _) = self
                    .hand_slots(viewer)
                    .into_iter()
                    .find(|&(_, c)| self.objs[c.i()].def == def)
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
        if self.objs[c.i()].zone == Zone::Hand {
            self.reveal(c); // CR 602.2a
        }
        if cost.tap {
            self.tap(c);
        }
        if cost.life > 0 {
            self.lose_life(p, cost.life as i32);
        }
        if cost.discard_hand {
            for h in self.hand_sorted(p) {
                if h != c {
                    self.move_card(h, Zone::Graveyard, Pos::Top);
                }
            }
        }
        if cost.sac_self || cost.discard_self {
            self.move_card(c, Zone::Graveyard, Pos::Top);
        }
        if cost.exile_self {
            self.move_card(c, Zone::Exile, Pos::Top);
        }
        if dmg > 0 {
            self.deal_damage(DamageSource { controller: p, ..dsrc }, Target::Player(p), dmg as i32);
        }
    }

    /// Productions of the several-mana source `src` (class `ci`) that pay
    /// part of the cost and leave the rest payable by the sources of the
    /// classes from `ci` on.
    fn feasible_prods(&self, pm: &PayMana, classes: &[Class], ci: usize, src: Src) -> Vec<[u8; 6]> {
        let Yield::Multi { prods, .. } = &classes[ci].yld else { unreachable!() };
        let later = pm.life_keep + pm.phyrexian_life;
        let (life, dmg) = self.life_budget(pm.payer, later);
        let hand = self.hand_budget(pm.payer, pm.exclude, pm.hand_keep);
        let used_life = self.used_life(pm, classes, dmg) + if dmg { classes[ci].dmg as i32 } else { 0 };
        let rest = cands(&classes[ci..], &|s| s.card != src.card && !pm.used(s.card), dmg);
        prods
            .iter()
            .copied()
            .filter(|prod| {
                let mut cost = pm.cost;
                let mut pool = ManaPool { amt: pm.extra };
                pool.add(prod);
                pool.pay_into(&mut cost);
                cost.total() < pm.cost.total()
                    && life - used_life >= 0
                    && (cost.is_zero() || cands_payable(&cost, &[], &rest, life - used_life, hand))
            })
            .collect()
    }

    /// Life already committed by the picks (life costs, plus damage when it
    /// counts).
    fn used_life(&self, pm: &PayMana, classes: &[Class], dmg: bool) -> i32 {
        let class_of = |s: &Src| classes.iter().find(|cl| cl.members.contains(s)).expect("picked source class");
        let mut n = 0;
        for s in &pm.chosen {
            let cl = class_of(s);
            if let Yield::One(u) = cl.yld {
                n += u.life as i32;
            }
            if dmg {
                n += cl.dmg as i32;
            }
        }
        if dmg {
            n += pm.multi.iter().map(|(s, _)| class_of(s).dmg as i32).sum::<i32>();
        }
        n
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
                    let classes = self.mana_classes(payer, pm.exclude);
                    let hand = self.hand_budget(payer, pm.exclude, pm.hand_keep);
                    let life = self.players[payer as usize].life;
                    let opts: Vec<Opt> = (0..=pm.cost.phyrexian[col])
                        .filter(|&k| {
                            let later = pm.life_keep as i32 + pm.phyrexian_life as i32 + 2 * k as i32;
                            later <= life
                                && self.phyrexian_feasible(
                                    payer,
                                    &split_phyrexian(&pm.cost, col, k),
                                    &classes,
                                    later as u8,
                                    hand,
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
            self.players[payer as usize].pool.pay_into(&mut pm.cost);
            pm.pooled = true;
        }
        if let Some((src, prods)) = pm.now.take() {
            match self.answer.take() {
                Some(Choice::Number(k)) => Self::pick_multi(&mut pm, src, prods[k as usize]),
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
        let classes = self.mana_classes(payer, pm.exclude);
        if let Some(ch) = self.answer.take() {
            let Choice::Ability(card, slot) = ch else { unreachable!("pay answer {ch:?}") };
            let src = Src { card, slot };
            let ci = classes.iter().position(|cl| cl.members.contains(&src)).expect("source class");
            pm.min_class = ci as u8;
            match &classes[ci].yld {
                Yield::One(_) => {
                    if self.objs[card.i()].zone == Zone::Hand {
                        self.reveal(card); // CR 602.2a, at once: never a hidden pick
                    }
                    pm.chosen.push(src);
                }
                Yield::Multi { prods, .. } => {
                    if prods.len() == 1 {
                        Self::pick_multi(&mut pm, src, prods[0]);
                    } else {
                        pm.now = Some((src, self.feasible_prods(&pm, &classes, ci, src)));
                    }
                }
            }
            self.agenda.push(Task::PayMana(pm));
            return;
        }
        let later = pm.life_keep + pm.phyrexian_life;
        let (life, dmg) = self.life_budget(payer, later);
        let hand = self.hand_budget(payer, pm.exclude, pm.hand_keep);
        let unit_of = |s: &Src| -> Unit {
            match classes.iter().find(|cl| cl.members.contains(s)).map(|cl| &cl.yld) {
                Some(Yield::One(u)) => *u,
                _ => unreachable!("chosen sources make one mana"),
            }
        };
        let chosen_units: Vec<Unit> = pm.chosen.iter().map(unit_of).collect();
        let used_life = self.used_life(&pm, &classes, dmg);
        let used_hand = chosen_units.iter().filter(|u| u.hand).count() as i32;
        let mut opts = Vec::new();
        for (ci, cl) in classes.iter().enumerate().skip(pm.min_class as usize) {
            // A card already picked for one of its abilities is spent.
            let Some(&rep) = cl.members.iter().find(|s| !pm.used(s.card)) else { continue };
            let ok = match &cl.yld {
                Yield::One(u) => {
                    let l = life - used_life - u.life as i32 - if dmg { cl.dmg as i32 } else { 0 };
                    let h = hand - used_hand - u.hand as i32;
                    let mut ch: Vec<ManaMask> = chosen_units.iter().map(|u| u.mask).collect();
                    ch.push(u.mask);
                    // Only classes from this one on can complete the set
                    // (the canonical order), so feasibility is checked
                    // against those; they all make one mana.
                    let rest = cands(&classes[ci..], &|s| s.card != rep.card && !pm.used(s.card), dmg);
                    l >= 0 && h >= 0 && cands_payable(&pm.cost, &ch, &rest, l, h)
                }
                Yield::Multi { .. } => {
                    debug_assert!(pm.chosen.is_empty(), "several-mana classes come first");
                    !self.feasible_prods(&pm, &classes, ci, rep).is_empty()
                }
            };
            if ok {
                opts.push(Opt {
                    action: self.ability_action(payer, rep.card, rep.slot),
                    choice: Choice::Ability(rep.card, rep.slot),
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
    fn pick_multi(pm: &mut PayMana, src: Src, prod: [u8; 6]) {
        let mut pool = ManaPool { amt: pm.extra };
        pool.add(&prod);
        pool.pay_into(&mut pm.cost);
        pm.extra = pool.amt;
        pm.multi.push((src, prod));
    }

    /// The picks cover the cost: activate everything (CR 601.2g) in an
    /// order the rules allow, then pay Phyrexian life (601.2h). Single-mana
    /// sources' mana is spent at once (colours by matching); the unspent
    /// mana of several-mana sources stays in the pool.
    fn finish_payment(&mut self, pm: &PayMana) {
        let payer = pm.payer;
        let mut order: Vec<Src> = pm.chosen.iter().copied().chain(pm.multi.iter().map(|m| m.0)).collect();
        // Sources that keep their permanent first ("activate only if you
        // control three artifacts" still holds; counts are unchanged), life
        // payments before damage, sacrifices last.
        order.sort_by_key(|s| {
            let ad = self.def_of(s.card).activated_def(s.slot);
            let dmg = ad.mana().map_or(0, |m| m.damage_to_you);
            (ad.cost.sac_self, dmg > 0, ad.cost.life == 0)
        });
        for src in order {
            self.activate_mana_costs(src);
        }
        self.add_mana(payer, &pm.extra);
        if pm.phyrexian_life > 0 {
            self.lose_life(payer, pm.phyrexian_life as i32);
            let n = pm.phyrexian_life;
            self.note(|_| format!("P{payer} pays {n} life for Phyrexian mana"));
        }
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
/// contributes one of its alternatives: fixed units when it has one way to
/// help (or several single-mana ways with the same costs, merged into one
/// unit), otherwise a list of alternatives to try. With `dmg`, the damage
/// an ability deals its controller weighs like life.
struct Cands {
    fixed: Vec<Unit>,
    alts: Vec<Vec<Vec<Unit>>>,
}

fn cands(classes: &[Class], keep: &dyn Fn(&Src) -> bool, dmg: bool) -> Cands {
    let mut per_card: Vec<(CardIdx, Vec<Vec<Unit>>)> = Vec::new();
    for cl in classes {
        let extra = if dmg { cl.dmg } else { 0 };
        for s in cl.members.iter().filter(|s| keep(s)) {
            let weigh = |mut us: Vec<Unit>| {
                if let Some(u) = us.first_mut() {
                    u.life += extra;
                }
                us
            };
            let ways: Vec<Vec<Unit>> = match &cl.yld {
                Yield::One(u) => vec![weigh(vec![*u])],
                Yield::Multi { units: Some(us), .. } => vec![weigh(us.clone())],
                Yield::Multi { prods, units: None } => prods.iter().map(|p| weigh(units_of(p).collect())).collect(),
            };
            match per_card.iter_mut().find(|(c, _)| *c == s.card) {
                Some((_, w)) => w.extend(ways),
                None => per_card.push((s.card, ways)),
            }
        }
    }
    let mut out = Cands { fixed: Vec::new(), alts: Vec::new() };
    for (_, ways) in per_card {
        if ways.len() == 1 {
            out.fixed.extend(ways.into_iter().next().unwrap());
        } else if ways.iter().all(|w| w.len() == 1 && w[0].life == ways[0][0].life && w[0].hand == ways[0][0].hand) {
            let mask = ways.iter().fold(0, |m, w| m | w[0].mask);
            out.fixed.push(Unit { mask, ..ways[0][0] });
        } else {
            out.alts.push(ways);
        }
    }
    out
}

/// [`payable_within`] over candidates, trying each alternative of the
/// cards that have several.
fn cands_payable(cost: &ManaCost, chosen: &[ManaMask], c: &Cands, life: i32, hand: i32) -> bool {
    fn go(
        cost: &ManaCost,
        chosen: &[ManaMask],
        units: &mut Vec<Unit>,
        alts: &[Vec<Vec<Unit>>],
        life: i32,
        hand: i32,
    ) -> bool {
        let Some((first, rest)) = alts.split_first() else {
            return payable_within(cost, chosen, units, life, hand);
        };
        first.iter().any(|w| {
            let len = units.len();
            units.extend(w.iter().copied());
            let ok = go(cost, chosen, units, rest, life, hand);
            units.truncate(len);
            ok
        })
    }
    let mut units = c.fixed.clone();
    go(cost, chosen, &mut units, &c.alts, life, hand)
}
