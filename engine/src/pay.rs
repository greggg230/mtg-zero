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
use crate::mana::{payable_n, payable_within_n, search_exhausted, Budget, ManaCost, ManaMask, Unit, COLORS, MADE, RESTRICTED, SEARCH_NODES, TYPES};
use crate::svec::SVec;

/// The `Src` slot of an artifact tapped for improvise (CR 702.126a: not a
/// mana ability; it pays for {1} of the generic mana).
pub(crate) const IMPROVISE: AbilitySlot = AbilitySlot::Spell;

/// The `Src` slot of a permanent tapped for another permanent's mana
/// ability whose cost taps it ("tap an untapped artifact you control: add
/// {U}", Urza; ENGINE.md 31): a source of that mana while paying.
pub(crate) const TAP_FOR: AbilitySlot = AbilitySlot::Trigger(u8::MAX);

/// The `Src` slot of a granted "sacrifice this: add <mana>" ability (Rain of
/// Filth, `Mod::GainSacMana`). It may follow the permanent's own tap ability
/// in one payment (CR 601.2g: tap it for mana, then sacrifice it; ENGINE.md
/// 18, 31): a sacrifice and a tap of one card combine.
pub(crate) const SAC_FOR: AbilitySlot = AbilitySlot::Trigger(u8::MAX - 1);

/// The `Src` slot of a permanent sacrificed for Squandered Resources'
/// "sacrifice a land: add one mana of any type that land could produce"
/// (CR 106.7), a source on each land (as `SAC_FOR`).
pub(crate) const SQ_FOR: AbilitySlot = AbilitySlot::Trigger(u8::MAX - 2);

/// The `Src` slot of a Forest sacrificed for Orcish Lumberjack's "{T},
/// sacrifice a Forest: add three mana in any combination of {R} and/or
/// {G}" (the Lumberjack tapped as well: `Class::share`).
pub(crate) const LJ_FOR: AbilitySlot = AbilitySlot::Trigger(u8::MAX - 3);

/// The `Src` slot of a permanent tapped for another permanent's "{T}, Tap
/// an untapped permanent you control: Add <mana>" (Gene Pollinator): a
/// source on each permanent it may tap, the activating permanent tapped as
/// well (`Class::share` of them, `Class::taps` each source takes; ENGINE.md
/// 99.12). Its id: the permanent's own (`MY_PERM`, `PayMana::card_slots`).
pub(crate) const POL_FOR: AbilitySlot = AbilitySlot::Trigger(u8::MAX - 4);

/// A source's use of its card (`Class::res`): tapped, sacrificed (or
/// exiled). Two sources of one card combine when they use different ones.
pub(crate) const TAP: u8 = 1;
pub(crate) const SAC: u8 = 2;

/// The slots of a permanent sacrificed for another's or a granted mana
/// ability.
pub(crate) fn is_sac_slot(slot: AbilitySlot) -> bool {
    slot == SAC_FOR || slot == SQ_FOR || slot == LJ_FOR
}

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
        Src { card: CardIdx::new(0), slot: AbilitySlot::Spell }
    }
}

/// The members of a class, inline up to eight (ENGINE.md 17).
pub(crate) type Members = SVec<Src, 8>;

/// Converge (exact, any number of units): the most colours (with `fixed`,
/// already spent) an exact payment of `cost` and of the `marked` needs (the
/// mana costs of mana abilities activated in the payment: not spent on the
/// spell) by one mana from each of `units` (the types each can make) can
/// show. A colour set S is reachable iff the units can be matched to the
/// coloured pips, the marked needs and one mana of each colour of S not yet
/// shown (the rest pay the generic part): a bipartite matching per S.
/// `None` with hybrid or Phyrexian pips (not modelled here).
fn most_colors_matching(units: &[ManaMask], cost: &ManaCost, marked: &[ManaMask], fixed: u8) -> Option<u32> {
    if cost.hybrid.iter().any(|&n| n > 0) || cost.phyrexian.iter().any(|&n| n > 0) {
        return None;
    }
    if cost.pips.iter().map(|&n| n as usize).sum::<usize>() + cost.generic as usize + marked.len() != units.len() {
        return None; // (not an exact payment by these units)
    }
    fn augment(slot: usize, slots: &[u8], units: &[ManaMask], seen: &mut [bool], owner: &mut [usize]) -> bool {
        for (u, &m) in units.iter().enumerate() {
            if crate::mana::fits(m, slots[slot]) && !seen[u] {
                seen[u] = true;
                if owner[u] == usize::MAX || augment(owner[u], slots, units, seen, owner) {
                    owner[u] = slot;
                    return true;
                }
            }
        }
        false
    }
    let mut best = None;
    for s in 0u8..32 {
        let mut slots: Vec<u8> = marked.to_vec();
        let mut shows = 0;
        for t in 0..6u8 {
            let show = t < 5 && s & (1 << t) != 0 && fixed & (1 << t) == 0;
            shows += (show && cost.pips[t as usize] == 0) as u8;
            let need = cost.pips[t as usize].max(show as u8);
            slots.extend(std::iter::repeat_n(1u8 << t, need as usize));
        }
        if slots.len() > units.len() || shows > cost.generic {
            continue;
        }
        let mut owner = vec![usize::MAX; units.len()];
        let ok = (0..slots.len()).all(|k| augment(k, &slots, units, &mut vec![false; units.len()], &mut owner));
        if ok {
            let n = ((s | fixed) & COLORS).count_ones();
            best = Some(best.map_or(n, |b: u32| b.max(n)));
        }
    }
    best.or(Some(0))
}

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
    /// The ability's own mana cost, one mana paid within the payment (CR
    /// 605.3a, 602.2b: Sunken Ruins' {U/B}): `NOT_MADE` | its types, else 0.
    /// Its mana is `MADE` (it never pays such a cost: nothing pays for
    /// itself).
    pub need: ManaMask,
    /// How many such needs its cost is: 1, or n for {n} (Cascading
    /// Cataracts' {5}; ENGINE.md 99.12).
    pub need_n: u8,
    /// How many times a member may be activated in one payment (Pentad
    /// Prism: once per charge counter; 1 otherwise).
    pub reuse: u8,
    /// "{X}, {T}, sacrifice this: add X mana in any combination of colors"
    /// (Wizard's Rockets): picked, the payment's mana may be of any colour
    /// (its X is paid by the mana it turns into colours).
    pub convert: bool,
    /// Its mana gives haste to the creature spell it is spent on (Arena of
    /// Glory, `ManaPool::haste`).
    pub haste: bool,
    /// What its activation uses of its card (`TAP`, `SAC`; both by default,
    /// 0 for Pentad Prism's counters): a card's sources that use different
    /// ones combine (CR 601.2g).
    pub res: u8,
    /// The `ACTIVATE` slot of a sacrifice for another's or a granted
    /// ability (`SAC_FOR`, `SQ_FOR`, `LJ_FOR`): after the card's own.
    pub slot_id: u8,
    /// Orcish Lumberjack (`LJ_FOR`): how many Lumberjacks can be tapped
    /// (each activation taps one); 0 otherwise. Gene Pollinator (`POL_FOR`):
    /// how many Pollinators can activate.
    pub share: u8,
    /// How much of `share` one use takes (0: 1): 2 for a Pollinator that
    /// could activate tapped for another's ability, 1 for one tapped for
    /// improvise or convoke (it can't activate then; ENGINE.md 99.12).
    pub taps: u8,
}

impl Class {
    /// A class of one source with nothing special.
    pub(crate) fn of(def: DefId, src: Src, yld: Yield) -> Class {
        Class {
            def,
            members: one_member(src),
            yld,
            dmg: 0,
            sac_self: false,
            only: 0,
            need: 0,
            need_n: 0,
            reuse: 1,
            convert: false,
            haste: false,
            res: TAP | SAC,
            slot_id: 0,
            share: 0,
            taps: 0,
        }
    }
}

/// The mana cost of a mana ability as `NOT_MADE` needs: one mana, or {n}
/// (n needs of any type; the lowering checks): (mask, how many; 0: none).
fn need_of(cost: &ManaCost) -> (ManaMask, u8) {
    let pip = (0..6).find(|&t| cost.pips[t] > 0).map(|t| 1u8 << t);
    let hyb = (0..10).find(|&i| cost.hybrid[i] > 0).map(crate::mana::pair_mask);
    let generic = (cost.generic > 0).then_some(TYPES);
    let n = if pip.or(hyb).is_some() { 1 } else { cost.generic };
    (pip.or(hyb).or(generic).map_or(0, |m| m | crate::mana::NOT_MADE), n)
}

/// What the pool holds after a payment (CR 106.4, 601.2h): the amounts,
/// and the restricted mana (the artifact-only {C} among it; CR 106.6,
/// `eoe_w2::RKind`, ENGINE.md 99.12) and the haste {R} among them.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub(crate) struct Rem {
    pub amt: [u8; 6],
    pub restr: Vec<crate::eoe_w2::RKind>,
    pub haste: u8,
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
    /// The permanents `sac` reserves are to be tapped by the cost ("tap X
    /// untapped artifacts you control", EoE W3): a mana source whose
    /// activation taps or sacrifices one of them uses it up (CR 601.2g-h).
    pub sac_taps: bool,
}

/// Paying a mana cost (ENGINE.md 6, 18). Stages: Phyrexian pips (how many
/// of each colour are paid with life), then sources, one class at a time in
/// non-decreasing class order, then which mana of the pool pays (601.2h).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct PayMana {
    pub payer: PlayerId,
    /// The cost (after the Phyrexian split): paid at the end by the picked
    /// single-mana sources and `vm` mana of the virtual pool (the player's
    /// pool and the several-mana sources' mana).
    pub cost: ManaCost,
    /// The mana costs of the mana abilities picked that have one
    /// (`NOT_MADE` needs, paid within the payment: CR 605.3a).
    pub marked: Vec<ManaMask>,
    /// Several-mana sources picked, with their productions.
    pub multi: Vec<(Src, [u8; 6])>,
    /// Their mana (spent, or left in the pool at the end).
    pub extra: [u8; 6],
    /// The part of `extra` with a spending restriction, by kind (the
    /// Workshop's artifact-only {C} among it; ENGINE.md 99.12).
    pub extra_restr: Vec<crate::eoe_w2::RKind>,
    /// The part of `extra` made by abilities with a mana cost (`MADE`).
    pub extra_made: [u8; 6],
    /// The part of `extra[R]` that gives haste (Arena of Glory).
    pub extra_haste: u8,
    /// Single-mana sources picked.
    pub chosen: Vec<Src>,
    pub min_class: u8,
    /// `vm` is known.
    pub pooled: bool,
    /// How much of the virtual pool's mana the payment spends: as much as
    /// it can, the single-mana sources paying the rest (ENGINE.md 18);
    /// `None` while several-mana sources must still be picked.
    pub vm: Option<u8>,
    /// Wizard's Rockets picked: the payment's mana may be of any colour.
    pub convert: bool,
    /// The pools the payment may leave (`PayPool`, CR 106.4, 601.2h), asked
    /// once the sources are picked.
    pub spend: Option<Vec<Rem>>,
    /// Sources the payment activates first (CR 601.2g): with `true`, exiled
    /// from the graveyard for delve after (601.2h: a Lotus Petal the spell's
    /// delve exiles); else for another cost's "exile n cards from your
    /// graveyard" paid next.
    pub forced: Vec<(Src, bool)>,
    /// Their cards, exiled once the mana abilities are activated.
    pub delve_late: Vec<CardIdx>,
    /// Life the cost takes after the mana (X life, Bolas's Citadel's): paid
    /// at the end (CR 601.2h after 601.2g), reserved in `keep.life`.
    pub life_after: u8,
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
    /// The spell has convoke (CR 702.51a): untapped creatures may pay for
    /// {1} or one mana of their colour each (`Game::convoke_classes`).
    pub convoke: bool,
    /// The permanents offered by their own `MY_PERM` id (improvise, convoke,
    /// another's tap: `TAP_FOR`, `POL_FOR`) and the sources each stands for;
    /// a permanent picked that stands for several (`which`) is asked which
    /// next (`ChooseMode`, `NUMBER` + k; ENGINE.md 99.12).
    pub card_slots: Vec<(CardIdx, Vec<AbilitySlot>)>,
    pub which: Option<CardIdx>,
    /// A production chosen one mana at a time (more productions than
    /// `NUMBER` ids: Cascading Cataracts' five of any colours): the mana
    /// chosen so far and the least colour the next may be (colours in
    /// non-decreasing order, each production once; ENGINE.md 99.12).
    pub now_taken: [u8; 6],
    pub now_min: u8,
}

impl PayMana {
    pub fn new(payer: PlayerId, cost: ManaCost) -> PayMana {
        PayMana {
            payer,
            cost,
            marked: Vec::new(),
            multi: Vec::new(),
            extra: [0; 6],
            extra_restr: Vec::new(),
            extra_made: [0; 6],
            extra_haste: 0,
            chosen: Vec::new(),
            min_class: 0,
            pooled: false,
            vm: None,
            convert: false,
            spend: None,
            forced: Vec::new(),
            delve_late: Vec::new(),
            life_after: 0,
            keep: Keep::default(),
            phyrexian_life: 0,
            now: None,
            improvise: false,
            waterbend: false,
            convoke: false,
            card_slots: Vec::new(),
            which: None,
            now_taken: [0; 6],
            now_min: 0,
        }
    }

    /// The types of untapped permanents that may pay for {1} each.
    pub(crate) fn improvise_types(&self) -> u16 {
        (if self.improvise { crate::db::types::ARTIFACT } else { 0 })
            | (if self.waterbend { crate::db::types::ARTIFACT | crate::db::types::CREATURE } else { 0 })
    }

    /// How many times card `c` was picked (once, but Pentad Prism's
    /// counters).
    fn uses(&self, c: CardIdx) -> u8 {
        (self.chosen.iter().filter(|s| s.card == c).count() + self.multi.iter().filter(|(s, _)| s.card == c).count()) as u8
    }

    /// What the picks so far use of card `c` (`Class::res`).
    fn used_res(&self, classes: &[Class], c: CardIdx) -> u8 {
        self.chosen
            .iter()
            .chain(self.multi.iter().map(|m| &m.0))
            .filter(|s| s.card == c)
            .fold(0, |m, s| m | classes.iter().find(|cl| cl.members.contains(s)).map_or(TAP | SAC, |cl| cl.res))
    }

    /// Is member `s` of class `cl` still available?
    fn free(&self, classes: &[Class], cl: &Class, s: &Src) -> bool {
        if cl.res == 0 {
            self.uses(s.card) < cl.reuse
        } else {
            self.used_res(classes, s.card) & cl.res == 0
        }
    }

    /// The mana the payment needs: the cost and the picked abilities' own.
    fn need_total(&self) -> usize {
        self.cost.total() as usize + self.marked.len()
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
        // One mana of a type in the mask per X (`activate::mana_now_costs`
        // asks X times); also the types it could make (CR 106.7).
        ManaProd::ComboX { mask } => (0..6).filter(|&t| mask & (1 << t) != 0).map(|t| one(t, 1)).collect(),
        ManaProd::Chosen => {
            let m = src.map_or(0, |c| g.chosen_land_color(c));
            (0..5).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect()
        }
        ManaProd::IfCounter { counter, then, els } => {
            let m = if src.is_some_and(|c| g.counters_on(c, *counter) > 0) { *then } else { *els };
            (0..6).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect()
        }
        // (Read from the sacrificed land as the ability is activated.)
        ManaProd::SacrificedLand => Vec::new(),
        ManaProd::LandsCouldProduce => {
            let m = g.lands_could_produce(p);
            (0..6).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect()
        }
        ManaProd::ColorAmong(f) => {
            let m = g.colors_among(p, f);
            (0..5).filter(|&t| m & (1 << t) != 0).map(|t| one(t, 1)).collect()
        }
        ManaProd::Fixed(v) => vec![*v],
        ManaProd::IfElse { cond, then, els } => vec![if g.cond_holds(cond, p) { *then } else { *els }],
        ManaProd::SameType { mask, n } => (0..6).filter(|&t| mask & (1 << t) != 0).map(|t| one(t, *n)).collect(),
        // (At most 255 of a type, the pool's u8: past it the game aborts as
        // the ability is activated, `Game::check_counter_mana`, F7.)
        ManaProd::SameTypeCounters { mask, counter } => match src.map_or(0, |c| g.counters_on(c, *counter)).min(255) as u8 {
            0 => Vec::new(),
            n => (0..6).filter(|&t| mask & (1 << t) != 0).map(|t| one(t, n)).collect(),
        },
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
    /// `classes` without the sources of the cards in `cards` (permanents a
    /// cost taps before the mana is paid: teamwork's creatures). A
    /// Lumberjack's sacrifices (`LJ_FOR`, `Class::share` Lumberjacks to tap)
    /// lose the Lumberjacks among `cards` and go when none is left; the other
    /// creatures tapped leave them (ENGINE.md 71.5: every shared class went
    /// whenever any creature was tapped, so a teamwork pick of a creature
    /// without a mana ability found the Lumberjack's mana gone that the
    /// cost's check, tapping no source, had counted).
    pub(crate) fn classes_without(&self, classes: &[Class], cards: &[CardIdx]) -> Vec<Class> {
        classes
            .iter()
            .filter_map(|cl| {
                let mut m = Members::new();
                for s in cl.members.iter() {
                    if !cards.contains(&s.card) {
                        m.push(*s);
                    }
                }
                if m.is_empty() {
                    return None;
                }
                let mut share = cl.share;
                if share > 0 {
                    let land = cl.members[0].card;
                    let tapped = cards.iter().filter(|&&c| self.lumberjack_for(c, land)).count();
                    share = share.saturating_sub(tapped.min(u8::MAX as usize) as u8);
                    if share == 0 {
                        return None;
                    }
                }
                Some(Class { members: m, share, ..cl.clone() })
            })
            .collect()
    }

    /// Is `c` one of the Lumberjacks a `LJ_FOR` class of `land` counts
    /// (`mana_classes`' `sac_by`, `sac_classes`): a mana ability of it, ready
    /// to activate, sacrifices a permanent `land` matches and makes mana of
    /// its own (not Squandered Resources' "any type that land could
    /// produce")?
    fn lumberjack_for(&self, c: CardIdx, land: CardIdx) -> bool {
        let p = self.objs[c.i()].controller;
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(c)) };
        let locks = self.activation_locks();
        let d = self.abilities(c);
        d.activated.iter().enumerate().any(|(i, ad)| {
            ad.mana().is_some_and(|m| !matches!(m.prod, ManaProd::SacrificedLand))
                && !ad.instant_only
                && self.activation_ready(p, c, ad, locks)
                && !(d.latent > 0 && !self.has_activated(c, i))
                && !(ad.exhaust && self.exhausted(c, i as u8))
                && ad.cost.sacrifice.as_ref().is_some_and(|sc| self.card_matches(&sc.filter, land, &ctx))
        })
    }

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
            if !self.cond_holds_from(c, p, Some(self.obj_ref(card))) {
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
        // "Tap an untapped artifact you control: Add {U}" (Urza): the mask of
        // the mana and the filter, once per payment (several such abilities
        // make the same mana from the same permanents).
        let mut tap_for: Option<(ManaMask, &Filter)> = None;
        // (Gene Pollinator's: the permanents whose ability it is.)
        let mut pol: Vec<(CardIdx, ManaMask, &Filter)> = Vec::new();
        // Mana abilities that sacrifice another permanent (Squandered
        // Resources, Orcish Lumberjack): a source on each permanent they
        // may sacrifice, below.
        let mut sac_by: SVec<(CardIdx, u8), 4> = SVec::new();
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
                if ad.exhaust && self.exhausted(c, i as u8) {
                    continue; // CR 702.177a: once
                }
                if ad.cost.sacrifice.is_some() {
                    sac_by.push((c, i as u8));
                    continue;
                }
                if let Some(t) = &ad.cost.tap_untapped {
                    if let ManaProd::One(mask) = m.prod {
                        if ad.cost.tap {
                            // (Gene Pollinator: `POL_FOR`, below.)
                            pol.push((c, mask, &t.filter));
                        } else if tap_for.is_none() {
                            tap_for = Some((mask, &t.filter));
                        }
                    }
                    continue;
                }
                // "Remove a counter from this" (Gemstone Mine): it needs
                // one; removing the last one sacrifices it (`sac_if_no`).
                let mut sac_self = ad.cost.sac_self;
                let mut reuse = 1u8;
                if let Some((k, n)) = ad.cost.remove_counters {
                    let have = self.counters_on(c, k);
                    if have < n as crate::num::Count {
                        continue;
                    }
                    sac_self |= m.sac_if_no == Some(k) && have == n as crate::num::Count;
                    // Without {T} (Pentad Prism): once per counter.
                    if !(ad.cost.tap || ad.cost.sac_self || ad.cost.exile_self) {
                        reuse = (have / n as crate::num::Count).min(32) as u8;
                    }
                }
                let src = Src { card: c, slot: AbilitySlot::Activated(i as u8) };
                // The additional mana tapping this source adds (its land
                // subtypes; a creature's), which depends on the object.
                let extra_of = |x: CardIdx| {
                    let mut extra = [0u8; 6];
                    if !bonus.is_empty() && self.is_type(x, crate::db::types::LAND) {
                        for &(sub, t) in &bonus {
                            if self.has_subtype(x, sub) {
                                extra[t as usize] += 1;
                            }
                        }
                    }
                    if creature_bonus != [0; 6] && ad.cost.tap && self.is_creature(x) {
                        for t in 0..6 {
                            extra[t] += creature_bonus[t];
                        }
                    }
                    extra
                };
                let extra = extra_of(c);
                let bonuses = !bonus.is_empty() || creature_bonus != [0; 6];
                // Same definition and ability: the same yield (it depends on
                // the controller only; the linked colours on the source; the
                // additional mana of the object: an animated land of a
                // definition is a creature, another isn't).
                let same = |cl: &&mut Class| {
                    cl.def == def && cl.members[0].slot == src.slot && cl.sac_self == sac_self && (!bonuses || extra_of(cl.members[0].card) == extra)
                };
                let per_source =
                    matches!(m.prod, ManaProd::Linked | ManaProd::Chosen | ManaProd::IfCounter { .. } | ManaProd::SameTypeCounters { .. }) || reuse > 1;
                if let Some(cl) = out.iter_mut().find(same).filter(|_| !per_source) {
                    cl.members.push(src);
                    continue;
                }
                // ("{X}: add X mana in any combination of colors": the
                // payment's mana may become any colours, `Class::convert`.)
                let (need, need_n) = if ad.cost.mana.is_zero() { (0, 0) } else { need_of(&ad.cost.mana) };
                let yld = if ad.x {
                    Yield::Multi { prods: vec![[0; 6]], units: Some(Vec::new()) }
                } else {
                    match self.yield_with(p, &m.prod, ad.cost.life, extra, Some(c)) {
                        // (One mana with a mana cost, Chromatic Star: its
                        // mana joins the virtual pool like a several-mana
                        // source's, its type chosen.)
                        Some(Yield::One(u)) if need != 0 => Yield::Multi {
                            prods: (0..6).filter(|&t| u.mask & (1 << t) != 0).map(|t| one(t, 1)).collect(),
                            units: Some(vec![Unit::free(u.mask)]),
                        },
                        Some(yld) => yld,
                        None => continue,
                    }
                };
                let res = if reuse > 1 {
                    0
                } else {
                    (if ad.cost.tap { TAP } else { 0 }) | (if sac_self || ad.cost.exile_self || ad.cost.discard_self { SAC } else { 0 })
                };
                let cl = Class {
                    dmg: m.damage_to_you,
                    sac_self,
                    only: m.only,
                    need,
                    need_n,
                    reuse,
                    convert: ad.x,
                    haste: m.haste,
                    res: if res == 0 && reuse == 1 { TAP | SAC } else { res },
                    ..Class::of(def, src, yld)
                };
                if matches!(cl.yld, Yield::Multi { .. }) {
                    out.insert(n_multi, cl);
                    n_multi += 1;
                } else {
                    out.push(cl);
                }
            }
        }
        // Granted "sacrifice this: add <mana>" abilities (Rain of Filth) and
        // the permanents another's mana ability sacrifices (Squandered
        // Resources, Orcish Lumberjack): sources on each permanent.
        let rain = self.seen & crate::game::SEEN_SAC_MANA != 0 && self.effects.iter().any(|e| matches!(e.m, crate::db::Mod::GainSacMana(_)));
        if rain || !sac_by.is_empty() {
            for c in self.permanents_of(p) {
                if Some(c) == exclude {
                    continue;
                }
                for cl in self.sac_classes(p, c, locks, rain, &sac_by, creature_bonus) {
                    if matches!(cl.yld, Yield::Multi { .. }) {
                        out.insert(n_multi, cl);
                        n_multi += 1;
                    } else {
                        out.push(cl);
                    }
                }
            }
        }
        // The permanents another's mana ability taps (`TAP_FOR`), one class
        // per definition, after the permanents' own abilities.
        if let Some((mask, filter)) = tap_for {
            let ctx = crate::cast::Ctx { controller: p, source: None };
            for x in self.permanents_of(p) {
                if Some(x) == exclude || self.objs[x.i()].tapped || !self.card_matches(filter, x, &ctx) {
                    continue;
                }
                let def = self.objs[x.i()].def;
                let src = Src { card: x, slot: TAP_FOR };
                match out.iter_mut().find(|cl| cl.def == def && cl.members[0].slot == TAP_FOR) {
                    Some(cl) => cl.members.push(src),
                    None => out.push(Class { res: TAP, ..Class::of(def, src, Yield::One(Unit::free(mask))) }),
                }
            }
        }
        // The permanents a Pollinator's ability taps (`POL_FOR`).
        if !pol.is_empty() {
            // (Several-mana ones among the several-mana classes.)
            let _ = self.pol_classes(p, exclude, &pol, creature_bonus, n_multi, &mut out);
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
                    out.push(Class { dmg: m.damage_to_you, only: m.only, ..Class::of(def, src, yld) });
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
                None => out.push(Class { res: TAP, ..Class::of(def, src, Yield::One(Unit::free(0))) }),
            }
        }
        // (A Pollinator tapped for improvise can't activate: castability and
        // the payment both, ENGINE.md 99.12.)
        if self.db.pollinator {
            self.pol_split(p, &mut out);
        }
        out
    }

    /// Convoke (CR 702.51a): `p`'s untapped creatures as sources that each
    /// pay for {1} or one mana of one of the creature's colours (a unit of its
    /// colours: a pip of one of them, or generic; a colourless creature pays
    /// generic only), one class per definition and colours. Not a mana
    /// ability and no mana (702.51a: "rather than pay that mana"): never a
    /// colour of mana spent (converge skips `IMPROVISE` sources), and
    /// `RESTRICTED` so Wizard's Rockets never turns it into colours (it
    /// can't pay the Rockets' {X}). Summoning sickness doesn't matter
    /// (302.6). ENGINE.md 99.8.
    pub(crate) fn convoke_classes(&self, p: PlayerId, exclude: Option<CardIdx>) -> Vec<Class> {
        let mut out: Vec<Class> = Vec::new();
        for c in self.permanents_of(p) {
            if Some(c) == exclude || self.objs[c.i()].tapped || !self.is_creature(c) {
                continue;
            }
            let def = self.objs[c.i()].def;
            let src = Src { card: c, slot: IMPROVISE };
            let yld = Yield::One(Unit::free((self.colors_of(c) & COLORS) | RESTRICTED));
            match out.iter_mut().find(|cl| cl.def == def && cl.yld == yld) {
                Some(cl) => cl.members.push(src),
                None => out.push(Class { res: TAP, ..Class::of(def, src, yld) }),
            }
        }
        // (A Pollinator tapped for convoke can't activate.)
        if self.db.pollinator {
            self.pol_split(p, &mut out);
        }
        out
    }

    /// The sources of a payment: mana abilities, and with improvise the
    /// untapped artifacts, with convoke the untapped creatures.
    pub(crate) fn pay_classes(&self, p: PlayerId, exclude: Option<CardIdx>, improvise: u16, convoke: bool) -> Vec<Class> {
        let mut classes = self.mana_classes(p, exclude);
        if convoke {
            let tapped_for: Vec<CardIdx> =
                classes.iter().filter(|cl| cl.members[0].slot == TAP_FOR).flat_map(|cl| cl.members.iter().map(|s| s.card)).collect();
            for mut cl in self.convoke_classes(p, exclude) {
                cl.members.retain(|s| !tapped_for.contains(&s.card));
                if !cl.members.is_empty() {
                    classes.push(cl);
                }
            }
        }
        if improvise != 0 {
            // (A permanent another's mana ability taps for mana isn't also
            // offered for improvise: the same tap, whose mana pays for {1}
            // as well, ENGINE.md 31.)
            let tapped_for: Vec<CardIdx> =
                classes.iter().filter(|cl| cl.members[0].slot == TAP_FOR).flat_map(|cl| cl.members.iter().map(|s| s.card)).collect();
            for mut cl in self.improvise_classes(p, exclude, improvise) {
                if !tapped_for.is_empty() {
                    cl.members.retain(|s| !tapped_for.contains(&s.card));
                    if cl.members.is_empty() {
                        continue;
                    }
                }
                classes.push(cl);
            }
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
            // (Its {X} is announced with priority only, never while paying.)
            ManaProd::ComboX { .. } => None,
            ManaProd::Linked => {
                let m = src.map_or(0, |c| self.linked_colors(c));
                (m != 0).then_some(Yield::One(Unit { life, ..Unit::free(m) }))
            }
            ManaProd::Chosen => {
                let m = src.map_or(0, |c| self.chosen_land_color(c));
                (m != 0).then_some(Yield::One(Unit { life, ..Unit::free(m) }))
            }
            ManaProd::IfCounter { counter, then, els } => {
                let m = if src.is_some_and(|c| self.counters_on(c, *counter) > 0) { *then } else { *els };
                (m != 0).then_some(Yield::One(Unit { life, ..Unit::free(m) }))
            }
            ManaProd::ColorAmong(f) => {
                let m = self.colors_among(p, f) & COLORS;
                (m != 0).then_some(Yield::One(Unit { life, ..Unit::free(m) }))
            }
            ManaProd::LandsCouldProduce => {
                let m = self.lands_could_produce(p);
                (m != 0).then_some(Yield::One(Unit { life, ..Unit::free(m) }))
            }
            _ => {
                debug_assert!(life == 0, "lowering keeps life costs to one-mana abilities");
                let prods = productions(self, p, prod, src);
                if prods.is_empty() {
                    return None;
                }
                let units = match prod {
                    ManaProd::SameType { mask, .. } | ManaProd::SameTypeCounters { mask, .. } if mask.count_ones() > 1 => None,
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
        // (The life a payment may spend: a life total past `i32::MAX` is the
        // same budget, as a payment spends far less: exact. F7, ENGINE.md
        // 100.7.)
        let life = (pl.life as crate::num::Wide - later as crate::num::Wide).clamp(0, i32::MAX as crate::num::Wide) as i32;
        (Budget { life, sac: sac.max(0), tap: i32::MAX }, later > 0)
    }

    /// Mana sources whose use takes a permanent a later sacrifice needs, or
    /// a card in hand a later discard needs.
    /// How many of the permanents a later cost needs source `s` of class
    /// `cl` uses: [`reserved`](Self::reserved)'s, and for a Pollinator's
    /// source (`POL_FOR`) under a later "tap n untapped permanents" the
    /// Pollinator that activates too (ENGINE.md 99.12).
    fn reserved_n(&self, p: PlayerId, keep: &Keep, cl: &Class, s: &Src) -> i32 {
        let mut n = self.reserved(p, keep, cl, s) as i32;
        if s.slot == POL_FOR && keep.sac_taps {
            if let Some((sc, card)) = &keep.sac {
                let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(*card)) };
                let counts = |a: CardIdx| !(sc.another && a == *card) && self.card_matches(&sc.filter, a, &ctx);
                let acts = self.pol_activators(p);
                let k = acts.iter().filter(|&&a| counts(a)).count();
                // (Which Pollinator activates would matter when only some
                // count: open, a loud failure.)
                assert!(k == 0 || k == acts.len(), "Pollinators of which only some a later tap cost needs (ENGINE.md 99.12)");
                n += (k > 0) as i32;
            }
        }
        n
    }

    fn reserved(&self, p: PlayerId, keep: &Keep, cl: &Class, s: &Src) -> bool {
        match &keep.sac {
            Some((sc, card)) if cl.sac_self || keep.sac_taps && cl.res != 0 => {
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
        cands(classes, use_src, &|_| (0, 0), dmg, &|cl, s| self.reserved_n(p, keep, cl, s), keep.allow)
    }

    /// The candidates of the rest of payment `pm`: the sources of
    /// `classes[from..]` not picked yet (Pentad Prism: its counters left),
    /// `plus` counted as picked once more.
    fn pay_cands(&self, pm: &PayMana, classes: &[Class], from: usize, plus: Option<Src>, dmg: bool) -> Cands {
        let plus_res = plus.map_or(0, |x| classes.iter().find(|cl| cl.members.contains(&x)).map_or(TAP | SAC, |cl| cl.res));
        let used = |s: &Src| {
            let more = plus.is_some_and(|x| x.card == s.card);
            (pm.used_res(classes, s.card) | if more { plus_res } else { 0 }, pm.uses(s.card) + more as u8)
        };
        let mut c = cands(&classes[from..], &|_| true, &used, dmg, &|cl, s| self.reserved_n(pm.payer, &pm.keep, cl, s), pm.keep.allow);
        c.converted = pm.convert;
        c
    }

    /// The pool's mana as candidate units: any of it may pay (CR 106.4;
    /// artifact-only {C} when `allow` lets it, `RESTRICTED`: never turned
    /// into colours by Wizard's Rockets).
    fn add_pool(&self, p: PlayerId, allow: u8, c: &mut Cands) {
        let pool = &self.players[p as usize].pool;
        if pool.total() == 0 {
            return;
        }
        // (Mana with a spending restriction: when this payment meets it,
        // ENGINE.md 99.12.)
        let kinds = self.pool_kinds(p);
        for t in 0..6 {
            let restr: u8 = kinds.iter().filter(|k| k.0 as usize == t).map(|k| k.2).sum();
            for _ in 0..pool.amt[t] - restr {
                c.fixed.push(Unit::free(1 << t));
            }
            for &(_, only, n) in kinds.iter().filter(|k| k.0 as usize == t) {
                if crate::eoe_w2::spendable(only, allow) {
                    for _ in 0..n {
                        c.fixed.push(Unit::free((1 << t) | RESTRICTED));
                    }
                }
            }
        }
    }

    /// The virtual pool of payment `pm` (ENGINE.md 18): the player's pool
    /// and the mana of the several-mana sources picked, as groups (kind,
    /// count); a kind is a type, + 8 for `MADE`, + 16 for mana with a
    /// spending restriction the payment meets (`RESTRICTED`: Wizard's
    /// Rockets can't turn it into colours, ENGINE.md 49; the pool's other
    /// restricted mana stays, ENGINE.md 99.12).
    fn vgroups(&self, pm: &PayMana) -> SVec<(u8, u8), 12> {
        let pool = &self.players[pm.payer as usize].pool;
        let kinds = self.pool_kinds(pm.payer);
        let mut g = SVec::new();
        for t in 0..6 {
            let (mut held, mut restr) = (0u8, 0u8);
            for &(_, only, n) in kinds.iter().filter(|k| k.0 as usize == t) {
                if crate::eoe_w2::spendable(only, pm.keep.allow) {
                    restr = restr.saturating_add(n);
                } else {
                    held += n;
                }
            }
            for k in pm.extra_restr.iter().filter(|k| k.0 as usize == t) {
                restr = restr.saturating_add(k.2);
            }
            // (Saturating: at most 255 of a type, shortcuts.md 46.3.)
            let n = (pool.amt[t] - held).saturating_add(pm.extra[t]) - pm.extra_made[t];
            let art = restr.min(n);
            if n > art {
                g.push((t as u8, n - art));
            }
            if art > 0 {
                g.push((t as u8 | 16, art));
            }
            if pm.extra_made[t] > 0 {
                g.push((t as u8 | 8, pm.extra_made[t]));
            }
        }
        g
    }

    /// The most mana of the virtual pool `groups` a payment of `pm.cost`
    /// and `marked` can spend with every unit of `chosen` and some of
    /// `rest`; `None` if it can't be paid. Wizard's Rockets' colours count
    /// only when nothing else pays (ENGINE.md 18).
    fn most_spent(&self, pm: &PayMana, groups: &[(u8, u8)], marked: &[ManaMask], chosen: &[ManaMask], rest: &Cands, budget: Budget) -> Option<u8> {
        self.most_spent_from(pm, groups, marked, chosen, rest, budget, 0)
    }

    /// [`most_spent`](Self::most_spent) if it is `floor` or more.
    #[allow(clippy::too_many_arguments)]
    fn most_spent_from(
        &self,
        pm: &PayMana,
        groups: &[(u8, u8)],
        marked: &[ManaMask],
        chosen: &[ManaMask],
        rest: &Cands,
        budget: Budget,
        floor: usize,
    ) -> Option<u8> {
        let total: usize = groups.iter().map(|g| g.1 as usize).sum();
        // (The virtual pool may pay the mana costs of mana abilities among
        // `rest` too, the most each card's ways need: Cascading Cataracts'
        // {5}, ENGINE.md 99.12.)
        let rest_needs: usize = rest.alts.iter().map(|ws| ws.iter().filter(|w| w.need != 0).map(|w| w.need_n as usize).max().unwrap_or(0)).sum();
        let need = pm.cost.total() as usize + marked.len() + rest_needs;
        let top = total.min(need.saturating_sub(chosen.len()));
        // (Each spend tried is a node of the search: ENGINE.md 51.)
        let mut nodes = SEARCH_NODES;
        let mut most = |convert: bool| {
            for k in (floor..=top).rev() {
                let mut found = false;
                each_subset(groups, k, &mut SVec::new(), &mut |cnt| {
                    nodes = nodes.checked_sub(1).unwrap_or_else(|| {
                        search_exhausted(format_args!(
                            "the virtual pool's spends: cost {}, groups {groups:?}, needs {marked:?}, chosen {chosen:?}, budget {budget:?}",
                            pm.cost
                        ))
                    });
                    let mut ch: SVec<ManaMask, 16> = chosen.iter().copied().collect();
                    ch.extend(masks_of(groups, cnt));
                    found = cands_payable_c(&pm.cost, marked, &ch, rest, budget, convert);
                    found
                });
                if found {
                    return Some(k as u8);
                }
            }
            None
        };
        most(false).or_else(|| if rest.convert.is_empty() { None } else { most(true) })
    }

    /// The most of virtual pool `groups` a payment (with `marked` needs) can
    /// spend with single-mana sources paying the rest (`plus` picked once
    /// more); `None` if they can't.
    fn singles_vm(&self, pm: &PayMana, classes: &[Class], groups: &[(u8, u8)], marked: &[ManaMask], plus: Option<Src>, budget: Budget, dmg: bool) -> Option<u8> {
        let first = classes.iter().position(|cl| matches!(cl.yld, Yield::One(_))).unwrap_or(classes.len());
        let rest = self.pay_cands(pm, classes, first.max(pm.min_class as usize), plus, dmg);
        self.most_spent(pm, groups, marked, &[], &rest, budget)
    }

    /// `pm.vm` again (after a several-mana pick).
    fn recount(&self, pm: &mut PayMana, classes: &[Class]) {
        // (An empty virtual pool spends nothing: whether single-mana
        // sources can pay is asked of each option, ENGINE.md 17.)
        if pm.marked.is_empty() && !pm.convert && pm.extra == [0; 6] && self.players[pm.payer as usize].pool.total() == 0 {
            pm.vm = Some(0);
            return;
        }
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
        let budget = self.after_picks(pm, classes, dmg, budget, None).expect("picks within the budget");
        pm.vm = self.singles_vm(pm, classes, &self.vgroups(pm), &pm.marked, None, budget, dmg);
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
                Yield::One(_) => cl.reuse as u32,
                Yield::Multi { prods, units } => {
                    let most = prods.iter().map(|v| v.iter().map(|&n| n as u32).sum::<u32>()).max().unwrap_or(0);
                    most.max(units.as_ref().map_or(0, |us| us.len() as u32))
                }
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
                if cost.is_zero() {
                    return true;
                }
                let (budget, dmg) = self.budget(p, keep, later);
                let mut c = self.cands(p, keep, classes, &|s| Some(s.card) != without, dmg);
                self.add_pool(p, keep.allow, &mut c);
                cands_payable(cost, &[], &[], &c, budget)
            }
            Some(col) => (0..=cost.phyrexian[col]).any(|k| {
                let later = later as i32 + 2 * k as i32;
                later as i64 <= life_total
                    && self.phyrexian_feasible(p, &split_phyrexian(cost, col, k), classes, keep, later as u8, without)
            }),
        }
    }

    /// The candidates of an unrestricted payment by `p` (computed once per
    /// priority decision).
    pub(crate) fn plain_cands(&self, p: PlayerId, classes: &[Class]) -> Cands {
        let mut c = self.cands(p, &Keep::default(), classes, &|_| true, false);
        self.add_pool(p, 0, &mut c);
        c
    }

    /// [`can_pay_with`](Self::can_pay_with) for an unrestricted payment,
    /// with candidates from [`plain_cands`](Self::plain_cands).
    pub(crate) fn can_pay_plain(&self, p: PlayerId, cost: &ManaCost, classes: &[Class], plain: &Cands) -> bool {
        if cost.has_phyrexian() {
            return self.can_pay_with(p, cost, classes, &Keep::default());
        }
        // (A life total past `i32::MAX` is the same budget: exact, above.)
        let life = self.players[p as usize].life.clamp(0, i32::MAX as i64) as i32;
        cost.is_zero() || cands_payable(cost, &[], &[], plain, Budget { life, sac: i32::MAX, tap: i32::MAX })
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
        let ad = self.source_def(c).activated_def(src.slot);
        let (cost, dmg) = (ad.cost.clone(), ad.mana().map_or(0, |m| m.damage_to_you));
        let dsrc = self.damage_source_of(c);
        self.note(|g| format!("P{p} activates a mana ability of {}", g.name(c)));
        if cost.tap {
            self.tap(c);
        }
        if let Some((k, n)) = cost.remove_counters {
            self.remove_counters(c, k, n as crate::num::Count);
        }
        if cost.exert {
            // CR 701.39a: it won't untap during its controller's next untap
            // step.
            let r = self.obj_ref(c);
            if !self.marks.contains(&(r, crate::game::Mark::Exerted)) {
                self.marks.push((r, crate::game::Mark::Exerted));
            }
            self.note(|g| format!("P{p} exerts {}", g.name(c)));
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
        // "Each opponent gains 1 life" (ENGINE.md 99.3).
        if let Some(n) = ad.mana().map(|m| m.opp_life).filter(|&n| n > 0) {
            self.gain_life(crate::ids::opp(p), n as i32);
        }
        // "If there are no <kind> counters on this, sacrifice it" (the
        // ability's effect, after its mana).
        if let Some(k) = ad.mana().and_then(|m| m.sac_if_no) {
            if self.objs[c.i()].zone == Zone::Battlefield && self.counters_on(c, k) == 0 {
                self.sacrifice(c);
            }
        }
    }

    /// Productions of the several-mana source `src` (class `ci`) whose mana
    /// the payment spends some of (the virtual pool pays more: CR 601.2g,
    /// no mana is made for nothing) and that leave the rest payable by the
    /// sources of the classes from `ci` on.
    fn feasible_prods(&self, pm: &PayMana, classes: &[Class], ci: usize, src: Src) -> Vec<[u8; 6]> {
        self.feasible_prods_where(pm, classes, ci, src, &|_| true, false)
    }

    /// [`feasible_prods`](Self::feasible_prods) among the productions
    /// `among` keeps; with `only_one`, the first only (is there one?): a class
    /// of many productions (Cascading Cataracts' 126) is checked lazily
    /// (ENGINE.md 99.12).
    fn feasible_prods_where(&self, pm: &PayMana, classes: &[Class], ci: usize, src: Src, among: &dyn Fn(&[u8; 6]) -> bool, only_one: bool) -> Vec<[u8; 6]> {
        let cl = &classes[ci];
        let Yield::Multi { prods, .. } = &cl.yld else { unreachable!() };
        let p = pm.payer;
        let (budget, dmg) = self.budget(p, &pm.keep, pm.keep.life + pm.phyrexian_life);
        let Some(budget) = self.after_picks(pm, classes, dmg, budget, Some((ci, src))) else { return Vec::new() };
        let first = classes.iter().position(|cl| matches!(cl.yld, Yield::One(_))).unwrap_or(classes.len());
        let singles = self.pay_cands(pm, classes, first.max(ci), Some(src), dmg);
        let base = self.vgroups(pm);
        let mut marked = pm.marked.clone();
        if cl.need != 0 {
            marked.extend(std::iter::repeat_n(cl.need, cl.need_n as usize));
        }
        let mut out = Vec::new();
        for prod in prods.iter().copied().filter(|v| among(v)) {
            let ok = {
                let prod = &prod;
                let mut groups = base.clone();
                for t in 0..6 {
                    if prod[t] > 0 {
                        // (Mana with a spending restriction, Mishra's
                        // Workshop's artifact-only {C}: its own kind.)
                        let kind = t as u8 | if cl.need != 0 { 8 } else { 0 } | if cl.only != 0 { 16 } else { 0 };
                        match groups.iter_mut().find(|g| g.0 == kind) {
                            Some(g) => g.1 += prod[t],
                            None => groups.push((kind, prod[t])),
                        }
                    }
                }
                // Useful: the pool pays more, or more several-mana sources
                // are needed still (and then feasible with them).
                let floor = pm.vm.map_or(0, |a| a as usize + 1);
                match (pm.vm, self.most_spent_from(pm, &groups, &marked, &[], &singles, budget, floor)) {
                    (_, Some(_)) => true,
                    (Some(_), None) if self.most_spent_from(pm, &groups, &marked, &[], &singles, budget, 0).is_some() => false,
                    (_, None) => {
                        let rest = self.pay_cands(pm, classes, ci, Some(src), dmg);
                        self.most_spent(pm, &groups, &marked, &[], &rest, budget).is_some()
                    }
                }
            };
            if ok {
                out.push(prod);
                if only_one {
                    break;
                }
            }
        }
        out
    }

    /// Can Wizard's Rockets (class `ci`, member `src`), picked now, leave the
    /// payment payable, when it can't be paid without it (ENGINE.md 18:
    /// using it otherwise only keeps a land of another colour untapped,
    /// and its card is drawn with priority as well)?
    fn convert_ok(&self, pm: &PayMana, classes: &[Class], ci: usize, src: Src) -> bool {
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
        let Some(left) = self.after_picks(pm, classes, dmg, budget, Some((ci, src))) else { return false };
        let groups = self.vgroups(pm);
        let mut without = self.pay_cands(pm, classes, ci, None, dmg);
        without.convert.clear();
        let now = self.after_picks(pm, classes, dmg, budget, None).expect("picks within the budget");
        if self.most_spent(pm, &groups, &pm.marked, &[], &without, now).is_some() {
            return false;
        }
        let mut with = self.pay_cands(pm, classes, ci, Some(src), dmg);
        with.converted = true;
        with.convert.clear();
        self.most_spent(pm, &groups, &pm.marked, &[], &with, left).is_some()
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
            let sac = self.reserved_n(pm.payer, &pm.keep, cl, &s);
            let tap = if cl.share > 0 { b.tap.min(cl.share as i32) - cl.taps.max(1) as i32 } else { b.tap };
            b = Budget { life: b.life - life, sac: b.sac - sac, tap };
            if b.life < 0 || b.sac < 0 || b.tap < 0 {
                return None;
            }
        }
        Some(b)
    }

    /// The `PayMana` task (ENGINE.md 6, 18).
    pub(crate) fn pay_mana(&mut self, mut pm: Box<PayMana>) {
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
                    let classes = self.pay_classes(payer, pm.keep.exclude, pm.improvise_types(), pm.convoke);
                    let life = self.players[payer as usize].life;
                    let opts: Vec<Opt> = (0..=pm.cost.phyrexian[col])
                        .filter(|&k| {
                            let later = pm.keep.life as i32 + pm.phyrexian_life as i32 + 2 * k as i32;
                            later as i64 <= life
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
        // CR 601.2h: which of the pool's mana pays, the sources picked.
        if let Some(rems) = pm.spend.take() {
            match self.answer.take() {
                Some(Choice::Number(k)) => self.finish_payment(&pm, rems[k as usize].clone()),
                Some(other) => unreachable!("pool answer {other:?}"),
                None => self.end_payment(pm, rems),
            }
            return;
        }
        // (Paid by single-mana sources alone: done, no sources looked at;
        // unless sources a later "exile n cards from your graveyard" needs
        // are still to be activated: a cost of no mana, Psychic Frog's,
        // with two Lotus Petals, ENGINE.md 49.)
        if pm.vm == Some(0) && pm.now.is_none() && pm.forced.is_empty() && pm.chosen.len() == pm.need_total() {
            let rem = self.pool_rems(&pm, &[]).swap_remove(0);
            self.finish_payment(&pm, rem);
            return;
        }
        let classes = self.pay_classes(payer, pm.keep.exclude, pm.improvise_types(), pm.convoke);
        if !pm.pooled {
            // The pool pays as much as it can (ENGINE.md 18).
            pm.pooled = true;
            self.recount(&mut pm, &classes);
        }
        // Sources delve exiles once activated: first, each with its mana's
        // type (its mana joins the virtual pool).
        if pm.now.is_none() && self.answer.is_none() {
            if let Some((src, delve)) = pm.forced.pop() {
                let ci = classes.iter().position(|cl| cl.members.contains(&src)).expect("a delve source's class");
                let Yield::One(u) = classes[ci].yld else { unreachable!("a delve source makes one mana") };
                if delve {
                    pm.delve_late.push(src.card);
                }
                let (budget, dmg) = self.budget(payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
                let rest = self.pay_cands(&pm, &classes, 0, Some(src), dmg);
                let prods: Vec<[u8; 6]> = (0..6)
                    .filter(|&t| u.mask & (1 << t) != 0)
                    .map(|t| one(t, 1))
                    .filter(|prod| {
                        let mut groups = self.vgroups(&pm);
                        let t = prod.iter().position(|&n| n > 0).unwrap() as u8;
                        match groups.iter_mut().find(|g| g.0 == t) {
                            Some(g) => g.1 += 1,
                            None => groups.push((t, 1)),
                        }
                        self.most_spent(&pm, &groups, &pm.marked, &[], &rest, budget).is_some()
                    })
                    .collect();
                if prods.len() == 1 {
                    self.pick_multi(&mut pm, &classes, ci, src, prods[0]);
                    self.agenda.push(Task::PayMana(pm));
                    return;
                }
                pm.now = Some((src, prods));
            }
        }
        if let Some((src, prods)) = pm.now.take() {
            // (More productions than `NUMBER` ids: chosen one mana at a
            // time, `PayMana::now_taken`; the productions all of one total.)
            let big = prods.len() > act::NUMBER_N as usize || pm.now_taken != [0; 6];
            let fits = |v: &[u8; 6], taken: &[u8; 6], min: usize| (0..6).all(|t| if t < min { v[t] == taken[t] } else { v[t] >= taken[t] });
            if big {
                assert!(prods.iter().all(|v| v.iter().sum::<u8>() == prods[0].iter().sum::<u8>()), "productions of different totals chosen one mana at a time");
                match self.answer.take() {
                    Some(Choice::Number(t)) => {
                        let t = t as usize;
                        pm.now_taken[t] += 1;
                        pm.now_min = t as u8;
                        let taken = pm.now_taken;
                        let rest: Vec<[u8; 6]> = prods.into_iter().filter(|v| fits(v, &taken, t)).collect();
                        if rest.len() == 1 && rest[0] == taken {
                            pm.now_taken = [0; 6];
                            pm.now_min = 0;
                            let ci = classes.iter().position(|cl| cl.members.contains(&src)).expect("source class");
                            self.pick_multi(&mut pm, &classes, ci, src, rest[0]);
                        } else {
                            pm.now = Some((src, rest));
                        }
                        self.agenda.push(Task::PayMana(pm));
                    }
                    Some(other) => unreachable!("production answer {other:?}"),
                    None => {
                        // (A colour some feasible production goes on with.)
                        let (taken, min) = (pm.now_taken, pm.now_min as usize);
                        let ci = classes.iter().position(|cl| cl.members.contains(&src)).expect("source class");
                        let opts: Vec<Opt> = (min..6)
                            .filter(|&t| prods.iter().any(|v| fits(v, &taken, t) && v[t] > taken[t]))
                            .filter(|&t| !self.feasible_prods_where(&pm, &classes, ci, src, &|v| fits(v, &taken, t) && v[t] > taken[t], true).is_empty())
                            .map(|t| Opt { action: act::NUMBER + t as u16, choice: Choice::Number(t as u8) })
                            .collect();
                        pm.now = Some((src, prods));
                        self.ask(Task::PayMana(pm), payer, DecisionKind::ChooseMana, opts);
                    }
                }
                return;
            }
            match self.answer.take() {
                Some(Choice::Number(k)) => {
                    let ci = classes.iter().position(|cl| cl.members.contains(&src)).expect("source class");
                    self.pick_multi(&mut pm, &classes, ci, src, prods[k as usize]);
                    if !pm.forced.is_empty() {
                        // (The next of delve's sources first.)
                        self.agenda.push(Task::PayMana(pm));
                        return;
                    }
                }
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
        if pm.vm.is_some_and(|vm| pm.chosen.len() + vm as usize == pm.need_total()) {
            // (Unless Wizard's Rockets must be picked for the colours.)
            let rems = self.pool_rems(&pm, &classes);
            if !rems.is_empty() {
                self.end_payment(pm, rems);
                return;
            }
        }
        // (Which source a permanent picked by its own id stands for, asked
        // when it stands for several: ENGINE.md 99.12.)
        let ask_which = |g: &mut Game, pm: Box<PayMana>, card: CardIdx| {
            let n = pm.card_slots.iter().find(|(c, _)| *c == card).map_or(0, |(_, v)| v.len());
            let opts = (0..n).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) }).collect();
            g.ask(Task::PayMana(pm), payer, DecisionKind::ChooseMode, opts);
        };
        if let (Some(card), None) = (pm.which, &self.answer) {
            return ask_which(self, pm, card);
        }
        if let Some(ch) = self.answer.take() {
            let slot_of = |pm: &PayMana, card: CardIdx, k: usize| pm.card_slots.iter().find(|(c, _)| *c == card).map(|(_, v)| v[k]).expect("a source the permanent stands for");
            let (card, slot) = match ch {
                Choice::Ability(card, slot) => (card, slot),
                // A permanent tapped for improvise or convoke, or for
                // another's mana ability.
                Choice::Card(card) => {
                    if pm.card_slots.iter().any(|(c, v)| *c == card && v.len() > 1) {
                        pm.which = Some(card);
                        return ask_which(self, pm, card);
                    }
                    (card, slot_of(&pm, card, 0))
                }
                Choice::Number(k) if pm.which.is_some() => {
                    let card = pm.which.take().expect("the permanent picked");
                    (card, slot_of(&pm, card, k as usize))
                }
                other => unreachable!("pay answer {other:?}"),
            };
            pm.card_slots.clear();
            let src = Src { card, slot };
            let ci = classes.iter().position(|cl| cl.members.contains(&src)).expect("source class");
            pm.min_class = ci as u8;
            if matches!(slot, AbilitySlot::Hand(_)) {
                // CR 602.2a / 605.3b: activating it from hand reveals it. Its
                // identity is public from the pick on, so no determinize
                // resamples a card the payment counts on (ENGINE.md 10).
                self.reveal(card);
            }
            if classes[ci].convert {
                pm.convert = true;
                pm.multi.push((src, [0; 6]));
                self.recount(&mut pm, &classes);
            } else {
                match &classes[ci].yld {
                    Yield::One(_) => pm.chosen.push(src),
                    Yield::Multi { prods, .. } => {
                        if prods.len() == 1 {
                            let prod = prods[0];
                            self.pick_multi(&mut pm, &classes, ci, src, prod);
                        } else {
                            // (Many productions: all of them, each mana
                            // then chosen among the feasible ones.)
                            pm.now = Some((src, if prods.len() > act::NUMBER_N as usize { prods.clone() } else { self.feasible_prods(&pm, &classes, ci, src) }));
                        }
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
        // The spends of `vm` mana of the virtual pool that can still complete
        // the payment (one, empty, when the pool is empty).
        let groups = self.vgroups(&pm);
        let spends: Vec<SVec<ManaMask, 16>> = if groups.is_empty() {
            Vec::new() // (not used)
        } else {
            let now = self.after_picks(&pm, &classes, dmg, budget, None).expect("picks within the budget");
            let rest = self.pay_cands(&pm, &classes, pm.min_class as usize, None, dmg);
            let mut v = Vec::new();
            each_subset(&groups, pm.vm.map_or(usize::MAX, |vm| vm as usize), &mut SVec::new(), &mut |cnt| {
                let s = masks_of(&groups, cnt);
                let mut ch = chosen_masks.clone();
                ch.extend(s.iter().copied());
                if cands_payable(&pm.cost, &pm.marked, &ch, &rest, now) {
                    v.push(s);
                }
                false
            });
            v
        };
        let mut opts = Vec::new();
        let mut card_slots: Vec<(CardIdx, Vec<AbilitySlot>)> = Vec::new();
        for (ci, cl) in classes.iter().enumerate().skip(pm.min_class as usize) {
            if !crate::eoe_w2::spendable(cl.only, pm.keep.allow) {
                continue; // its mana can't be spent on this (CR 106.6)
            }
            let ok_of = |rep: Src| -> bool {
                if cl.convert {
                    !pm.convert && pm.chosen.is_empty() && self.convert_ok(&pm, &classes, ci, rep)
                } else {
                    match &cl.yld {
                        Yield::One(u) => match self.after_picks(&pm, &classes, dmg, budget, Some((ci, rep))) {
                            None => false,
                            Some(left) => {
                                // Only classes from this one on can complete the
                                // set (the canonical order), so feasibility is
                                // checked against those; they all make one mana.
                                let rest = self.pay_cands(&pm, &classes, ci, Some(rep), dmg);
                                if groups.is_empty() {
                                    let mut ch = chosen_masks.clone();
                                    ch.push(u.mask);
                                    cands_payable(&pm.cost, &pm.marked, &ch, &rest, left)
                                } else {
                                    spends.iter().any(|s| {
                                        let mut ch = chosen_masks.clone();
                                        ch.push(u.mask);
                                        ch.extend(s.iter().copied());
                                        cands_payable(&pm.cost, &pm.marked, &ch, &rest, left)
                                    })
                                }
                            }
                        },
                        Yield::Multi { .. } => {
                            debug_assert!(pm.chosen.is_empty(), "several-mana classes come first");
                            !self.feasible_prods_where(&pm, &classes, ci, rep, &|_| true, true).is_empty()
                        }
                    }
                }
            };
            // A card already picked for one of its abilities is spent. The
            // members of a class are alike but for their cards' other
            // sources and reservations (Mox Sapphire a copy of Llanowar
            // Elves, Saheeli's: also an artifact Urza's ability taps;
            // ENGINE.md 49): the first free member whose pick leaves the
            // payment payable, each kind of member tried once.
            let kind_of = |s: &Src| -> (bool, Vec<usize>) {
                let others = classes.iter().enumerate().filter(|&(j, c)| j != ci && c.members.iter().any(|m| m.card == s.card)).map(|(j, _)| j);
                (self.reserved(payer, &pm.keep, cl, s), others.collect())
            };
            let mut free = cl.members.iter().copied().filter(|s| pm.free(&classes, cl, s));
            let Some(first) = free.next() else { continue };
            let rep = if ok_of(first) {
                Some(first)
            } else {
                let mut tried = vec![kind_of(&first)];
                free.find(|s| {
                    let k = kind_of(s);
                    if tried.contains(&k) {
                        return false;
                    }
                    tried.push(k);
                    ok_of(*s)
                })
            };
            if let Some(rep) = rep {
                // (One `MY_PERM` option a permanent; the sources it stands
                // for in `card_slots`.)
                if rep.slot == IMPROVISE || rep.slot == TAP_FOR || rep.slot == POL_FOR {
                    match card_slots.iter_mut().find(|(c, _)| *c == rep.card) {
                        Some((_, v)) => {
                            v.push(rep.slot);
                            continue;
                        }
                        None => card_slots.push((rep.card, vec![rep.slot])),
                    }
                }
                opts.push(if is_sac_slot(rep.slot) {
                    // A granted ability or another's sacrificing it: after
                    // its own (`ACTIVATE` + `slot_id`; `sac_classes` checks
                    // there is room).
                    let k = cl.slot_id as u16;
                    Opt { action: act::ACTIVATE + (self.perm_action(payer, rep.card) - act::MY_PERM) * 4 + k, choice: Choice::Ability(rep.card, rep.slot) }
                } else if rep.slot == IMPROVISE || rep.slot == TAP_FOR || rep.slot == POL_FOR {
                    // Improvise or convoke, or a permanent tapped for
                    // another's mana ability: the permanent's own slot
                    // (`MY_PERM`).
                    Opt { action: self.perm_action(payer, rep.card), choice: Choice::Card(rep.card) }
                } else {
                    Opt { action: self.ability_action(payer, rep.card, rep.slot), choice: Choice::Ability(rep.card, rep.slot) }
                });
            }
        }
        pm.card_slots = card_slots;
        assert!(
            !opts.is_empty(),
            "PayMana: no way to pay {} for P{payer}: {pm:?} classes {classes:?} pool {:?}\n{}",
            pm.cost,
            self.players[payer as usize].pool,
            self.log_lines().join("\n")
        );
        self.ask(Task::PayMana(pm), payer, DecisionKind::PayMana, opts);
    }

    /// The sources are picked (CR 601.2g): which mana of the virtual pool
    /// pays (601.2h), asked when the pools it can leave differ (`PayPool`).
    fn end_payment(&mut self, mut pm: Box<PayMana>, rems: Vec<Rem>) {
        if rems.len() == 1 {
            self.finish_payment(&pm, rems[0].clone());
            return;
        }
        let opts = (0..rems.len()).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) }).collect();
        let payer = pm.payer;
        pm.spend = Some(rems);
        self.ask(Task::PayMana(pm), payer, DecisionKind::PayPool, opts);
    }

    /// The pools payment `pm` can leave, each once (CR 106.4: two mana of
    /// one type and tag are the same): `vm` mana of the virtual pool spent
    /// with every picked source's. Mana with a spending restriction (the
    /// artifact-only {C}) is spent before other mana of its type, and haste
    /// {R} after other {R} on anything but a creature spell (never worse for
    /// the player); on a creature spell which {R} pays is the player's
    /// choice (CR 106.4: the haste goes to this spell or stays for another),
    /// and so is which of two restrictions neither of which allows all the
    /// other does stays (ENGINE.md 99.12).
    fn pool_rems(&self, pm: &PayMana, classes: &[Class]) -> Vec<Rem> {
        use crate::mana::R;
        let pool = self.players[pm.payer as usize].pool;
        let groups = self.vgroups(pm);
        let mut vamt = pool.amt;
        for t in 0..6 {
            vamt[t] = vamt[t].saturating_add(pm.extra[t]);
        }
        let vkinds = self.virtual_kinds(pm);
        let vhaste = pool.haste.saturating_add(pm.extra_haste).min(vamt[R]);
        let vm = pm.vm.expect("the pool's spend is known") as usize;
        if vm == 0 {
            return vec![Rem { amt: vamt, restr: vkinds, haste: vhaste }];
        }
        let creature = pm.keep.spell.is_some_and(|c| self.is_type(c, crate::db::types::CREATURE));
        let chosen: SVec<ManaMask, 16> = pm.chosen.iter().map(|s| chosen_mask(classes, s)).collect();
        let conv = |m: ManaMask| if pm.convert { convert(m) } else { m };
        // The spends that complete the payment: the mana of each type spent,
        // and the most restricted mana of each type among them (spent before
        // other mana; its own kind of the virtual pool, which Wizard's
        // Rockets doesn't turn into colours): the greatest such amounts.
        let mut spends: Vec<([u8; 6], [u8; 6])> = Vec::new();
        let covers = |a: &[u8; 6], b: &[u8; 6]| (0..6).all(|t| a[t] >= b[t]);
        each_subset(&groups, vm, &mut SVec::new(), &mut |cnt| {
            let mut ch: SVec<ManaMask, 16> = chosen.iter().map(|&m| conv(m)).collect();
            ch.extend(masks_of(&groups, cnt).iter().map(|&m| conv(m)));
            if !payable_n(&pm.cost, &pm.marked, &ch, &[]) {
                return false;
            }
            let (mut spent, mut restr) = ([0u8; 6], [0u8; 6]);
            for (g, &n) in groups.iter().zip(cnt.iter()) {
                spent[(g.0 & 7) as usize] += n;
                if g.0 & 16 != 0 {
                    restr[(g.0 & 7) as usize] += n;
                }
            }
            if spends.iter().any(|s| s.0 == spent && covers(&s.1, &restr)) {
                return false;
            }
            match spends.iter().position(|s| s.0 == spent && covers(&restr, &s.1)) {
                Some(i) => {
                    spends[i].1 = restr;
                    let mut j = i + 1;
                    while j < spends.len() {
                        if spends[j].0 == spent && covers(&restr, &spends[j].1) {
                            spends.remove(j);
                        } else {
                            j += 1;
                        }
                    }
                }
                None => spends.push((spent, restr)),
            }
            false
        });
        let mut out: Vec<Rem> = Vec::new();
        for (spent, restr_spent) in spends {
            let mut amt = vamt;
            for t in 0..6 {
                amt[t] -= spent[t];
            }
            let lo = spent[R].saturating_sub(vamt[R] - vhaste);
            let hi = if creature { spent[R].min(vhaste) } else { lo };
            for restr in crate::eoe_w2::kinds_left(&vkinds, &restr_spent, &|only| pm_spendable(pm, only)) {
                for h in lo..=hi {
                    let r = Rem { amt, restr: restr.clone(), haste: vhaste - h };
                    if !out.contains(&r) {
                        out.push(r);
                    }
                }
            }
        }
        // (The first keeps the most types, then the most of the earlier
        // ones; at most `NUMBER` options.)
        out.sort_by_key(|r| (std::cmp::Reverse(r.amt.iter().filter(|&&n| n > 0).count()), std::cmp::Reverse(r.amt), std::cmp::Reverse(r.haste)));
        out.truncate(act::NUMBER_N as usize);
        out
    }

    /// The virtual pool's mana with a spending restriction, by kind: the
    /// pool's (also what this payment may not spend) and the picked
    /// several-mana sources' (ENGINE.md 99.12).
    pub(crate) fn virtual_kinds(&self, pm: &PayMana) -> Vec<crate::eoe_w2::RKind> {
        let mut v: Vec<crate::eoe_w2::RKind> = self.pool_kinds(pm.payer).iter().copied().collect();
        v.sort_unstable();
        for &(t, only, n) in &pm.extra_restr {
            crate::eoe_w2::add_kind(&mut v, t, only, n);
        }
        v
    }

    /// A several-mana source picked with production `prod`: its mana joins
    /// the virtual pool (activated at the end); a mana cost of its own is
    /// one more need of the payment.
    fn pick_multi(&self, pm: &mut PayMana, classes: &[Class], ci: usize, src: Src, prod: [u8; 6]) {
        let cl = &classes[ci];
        // (Saturating: at most 255 of a type, shortcuts.md 46.3.)
        for t in 0..6 {
            pm.extra[t] = pm.extra[t].saturating_add(prod[t]);
            if cl.need != 0 {
                pm.extra_made[t] = pm.extra_made[t].saturating_add(prod[t]);
            }
        }
        if cl.need != 0 {
            pm.marked.extend(std::iter::repeat_n(cl.need, cl.need_n as usize));
        }
        if cl.haste {
            pm.extra_haste = pm.extra_haste.saturating_add(prod[crate::mana::R]);
        }
        if cl.only != 0 {
            for t in 0..6 {
                crate::eoe_w2::add_kind(&mut pm.extra_restr, t as u8, cl.only, prod[t]);
            }
        }
        pm.multi.push((src, prod));
        self.recount(pm, classes);
    }

    /// The picks cover the cost: activate everything (CR 601.2g) in an
    /// order the rules allow, leave `rem` in the pool (601.2h), then pay
    /// Phyrexian life.
    fn finish_payment(&mut self, pm: &PayMana, rem: Rem) {
        let payer = pm.payer;
        let pool = self.players[payer as usize].pool;
        // F7 (ENGINE.md 100.7): the virtual pool (the pool's mana and the
        // picked several-mana sources', `pm.extra`) keeps a `u8` a type, as
        // the pool does (workstream B's model); past it the game ends
        // aborted, counted here exactly from the picks.
        for t in 0..6 {
            let total = pool.amt[t] as u32 + pm.multi.iter().map(|m| m.1[t] as u32).sum::<u32>();
            if total > u8::MAX as u32 {
                self.abort(|| format!("P{payer}'s pool and payment: {total} mana of one type, past 255"));
            }
        }
        for &(src, _) in &pm.multi {
            if let AbilitySlot::Activated(_) = src.slot {
                if let Some(m) = self.source_def(src.card).activated_def(src.slot).mana() {
                    self.check_counter_mana(src.card, &m.prod);
                }
            }
        }
        let mut spent = [0u8; 6];
        for t in 0..6 {
            spent[t] = pool.amt[t].saturating_add(pm.extra[t]) - rem.amt[t];
        }
        if self.marks.iter().any(|(_, m)| matches!(m, crate::game::Mark::ConvergePending(_))) {
            let classes = self.pay_classes(payer, pm.keep.exclude, pm.improvise_types(), pm.convoke);
            let conv = |m: ManaMask| if pm.convert { convert(m) } else { m };
            // (A creature tapped for convoke spends no mana, CR 702.51a; an
            // artifact tapped for improvise, a unit of no colour, adds none.)
            let mut units: Vec<ManaMask> = pm.chosen.iter().filter(|s| s.slot != IMPROVISE).map(|s| conv(chosen_mask(&classes, s))).collect();
            // (Mana with a spending restriction spent: never turned into
            // colours.)
            let vkinds = self.virtual_kinds(pm);
            for t in 0..6 {
                let made = spent[t].min(pm.extra_made[t]);
                let of = |v: &[crate::eoe_w2::RKind]| v.iter().filter(|k| k.0 as usize == t).map(|k| k.2 as u32).sum::<u32>();
                let art = (of(&vkinds).saturating_sub(of(&rem.restr)) as u8).min(spent[t] - made);
                units.extend(std::iter::repeat_n(conv((1 << t) | MADE), made as usize));
                units.extend(std::iter::repeat_n(conv((1 << t) | RESTRICTED), art as usize));
                units.extend(std::iter::repeat_n(conv(1 << t), (spent[t] - made - art) as usize));
            }
            self.record_converge(pm, &units);
        }
        // "The amount of mana spent to cast this" (CR 601.2h; EoE W3): the
        // payment's units (a creature tapped for convoke or an artifact for
        // improvise spends none, 702.51a, 702.126a).
        if let Some(card) = pm.keep.spell {
            if self.def_of(card).w3.mana_spent {
                let n = pm.chosen.iter().filter(|s| s.slot != IMPROVISE).count() + spent.iter().map(|&x| x as usize).sum::<usize>();
                self.note_mana_spent(card, n);
            }
        }
        // "That spell can't be countered" (Delighted Halfling): its mana
        // was spent on the spell (read before the sources are activated).
        if let Some(card) = pm.keep.spell {
            let special = |s: &Src| s.slot == IMPROVISE || s.slot == TAP_FOR || s.slot == POL_FOR || is_sac_slot(s.slot);
            let uncounterable = pm
                .chosen
                .iter()
                .chain(pm.multi.iter().map(|m| &m.0))
                .any(|s| !special(s) && self.source_def(s.card).activated_def(s.slot).mana().is_some_and(|m| m.uncounterable));
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
            if s.slot == TAP_FOR || s.slot == POL_FOR {
                return (false, false, false, false);
            }
            if is_sac_slot(s.slot) {
                return (false, true, false, true);
            }
            let ad = self.source_def(s.card).activated_def(s.slot);
            let dmg = ad.mana().map_or(0, |m| m.damage_to_you);
            let last = ad.mana().and_then(|m| m.sac_if_no).is_some_and(|k| self.counters_on(s.card, k) <= 1);
            (false, ad.cost.sac_self || last, dmg > 0, ad.cost.life == 0)
        });
        // The Pollinators that activate for the `POL_FOR` picks: the first
        // ready ones no pick uses (ENGINE.md 99.12).
        let mut pol_actors: Vec<CardIdx> = if order.iter().any(|s| s.slot == POL_FOR) {
            let used: Vec<CardIdx> = order.iter().map(|s| s.card).collect();
            let mut v: Vec<CardIdx> = self.pol_activators(payer).into_iter().filter(|c| !used.contains(c)).collect();
            v.reverse();
            v
        } else {
            Vec::new()
        };
        for src in order {
            if src.slot == IMPROVISE {
                let why = if pm.convoke { "convoke" } else { "improvise" };
                self.note(|g| format!("P{payer} taps {} for {why}", g.name(src.card)));
                self.tap(src.card);
            } else if src.slot == POL_FOR {
                // "{T}, Tap an untapped permanent you control": a
                // Pollinator (assigned below) and this permanent.
                let actor = pol_actors.pop().expect("a Pollinator to activate (the budget counts them)");
                self.note(|g| format!("P{payer} activates a mana ability of {}, tapping {}", g.name(actor), g.name(src.card)));
                self.tap(actor);
                self.tap(src.card);
            } else if src.slot == TAP_FOR {
                // (A cost of another permanent's mana ability: not "tapped
                // for mana", CR 106.12a.)
                self.note(|g| format!("P{payer} taps {} for a mana ability", g.name(src.card)));
                self.tap(src.card);
            } else if is_sac_slot(src.slot) {
                self.activate_sac_for(src);
            } else {
                if let AbilitySlot::Activated(i) = src.slot {
                    if self.source_def(src.card).activated_def(src.slot).exhaust {
                        let r = self.obj_ref(src.card);
                        self.marks.push((r, crate::game::Mark::Exhausted(i))); // CR 702.177a
                    }
                }
                self.activate_mana_costs(src);
            }
        }
        let p = &mut self.players[payer as usize].pool;
        p.amt = rem.amt;
        p.haste = rem.haste;
        self.set_pool_kinds(payer, &rem.restr);
        // Delve's graveyard cards, then those that were mana sources (CR
        // 601.2h, after 601.2g; ENGINE.md 51).
        self.exile_delved(payer, pm.keep.spell);
        for &c in &pm.delve_late {
            if self.objs[c.i()].zone == Zone::Graveyard {
                self.note(|g| format!("P{payer} exiles {} (delve)", g.name(c)));
                let e = self.move_card(c, Zone::Exile, Pos::Top);
                self.record_exiled_to_pay(pm.keep.spell, e);
            }
        }
        // Arena of Glory's mana spent on a creature spell gives it haste.
        if pool.haste + pm.extra_haste > rem.haste {
            if let Some(c) = pm.keep.spell.filter(|&c| self.is_type(c, crate::db::types::CREATURE)) {
                let r = self.obj_ref(c);
                self.marks.push((r, crate::game::Mark::HasteMana));
            }
        }
        if pm.phyrexian_life > 0 {
            self.lose_life(payer, pm.phyrexian_life as i32);
            let n = pm.phyrexian_life;
            self.note(|_| format!("P{payer} pays {n} life for Phyrexian mana"));
        }
        if pm.life_after > 0 {
            let n = pm.life_after;
            self.note(|_| format!("P{payer} pays {n} life"));
            self.lose_life(payer, n as i32);
        }
    }

    /// Converge (ENGINE.md 28): the colours of mana spent on a spell, from
    /// the payment's `units` (the picked single-mana sources' types, the
    /// pool's and several-mana sources' mana spent): each single-mana
    /// source makes the colour the player prefers, so the count is the most
    /// colours an exact payment by those units can show (the player chooses
    /// each mana's type as the ability resolves, CR 106.1b; 601.2h which
    /// pays what).
    fn record_converge(&mut self, pm: &PayMana, units: &[ManaMask]) {
        let Some(k) = self.marks.iter().position(|(_, m)| matches!(m, crate::game::Mark::ConvergePending(_))) else { return };
        let crate::game::Mark::ConvergePending(fixed) = self.marks[k].1 else { unreachable!() };
        fn best(i: usize, units: &[ManaMask], cost: &ManaCost, got: &mut Vec<ManaMask>, fixed: u8) -> u32 {
            if i == units.len() {
                if !crate::mana::payable(cost, &[], got) {
                    return 0;
                }
                return (got.iter().fold(fixed, |m, &x| m | x) & COLORS).count_ones();
            }
            let options: SVec<ManaMask, 6> = if units[i] & TYPES == 0 {
                [units[i]].into_iter().collect()
            } else {
                (0..6).filter(|t| units[i] & (1 << t) != 0).map(|t| (1u8 << t) | (units[i] & MADE)).collect()
            };
            let mut b = 0;
            for o in options.iter() {
                got.push(*o);
                b = b.max(best(i + 1, units, cost, got, fixed));
                got.pop();
            }
            b
        }
        let n = if units.len() <= 7 && pm.marked.is_empty() {
            best(0, units, &pm.cost, &mut Vec::new(), fixed)
        } else {
            match most_colors_matching(units, &pm.cost, &pm.marked, fixed) {
                Some(n) => n,
                // (Hybrid or Phyrexian pips left with many units: no
                // converge spell in the cube has them; an upper bound.)
                None => {
                    let all = units.iter().fold(fixed, |m, &x| m | x) & COLORS;
                    all.count_ones().min(fixed.count_ones() + units.len() as u32)
                }
            }
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

    /// Add mana to `p`'s pool (CR 106.4): past 255 of a type (the pool's
    /// `u8`, workstream B) the game ends aborted (ENGINE.md 100.7).
    pub(crate) fn add_mana(&mut self, p: PlayerId, mana: &[u8; 6]) {
        if !self.players[p as usize].pool.add(mana) {
            self.abort(|| format!("P{p}'s pool: more than 255 mana of one type"));
        }
    }

    /// F7 (ENGINE.md 100.7): a mana ability of `src` that makes mana equal to
    /// the counters on it (`ManaProd::SameTypeCounters`) makes more than the
    /// pool's 255 of a type (`productions` reads at most 255: workstream B's
    /// `u8` model): the game ends aborted.
    pub(crate) fn check_counter_mana(&mut self, src: CardIdx, prod: &ManaProd) {
        if let ManaProd::SameTypeCounters { counter, .. } = prod {
            let n = self.counters_on(src, *counter);
            if n > u8::MAX as crate::num::Count {
                self.abort(|| format!("{n} mana of one type from counters: past the pool's 255"));
            }
        }
    }
}

/// May payment `pm` spend mana restricted to the purposes `only` (CR
/// 106.6)?
fn pm_spendable(pm: &PayMana, only: u8) -> bool {
    crate::eoe_w2::spendable(only, pm.keep.allow)
}

/// `cost` with `k` of its Phyrexian pips of colour `col` paid with life and
/// the others turned into ordinary pips.
fn split_phyrexian(cost: &ManaCost, col: usize, k: u8) -> ManaCost {
    let mut c = *cost;
    c.pips[col] += c.phyrexian[col] - k;
    c.phyrexian[col] = 0;
    c
}

/// The mask of a picked single-mana source's unit.
fn chosen_mask(classes: &[Class], s: &Src) -> ManaMask {
    match classes.iter().find(|cl| cl.members.contains(s)).map(|cl| &cl.yld) {
        Some(Yield::One(u)) => u.mask,
        _ => unreachable!("chosen sources make one mana"),
    }
}

/// A unit's mask with Wizard's Rockets: any colour as well (a unit of no
/// type, improvise's, stays generic only; mana with a spending restriction,
/// `RESTRICTED`, can't pay the Rockets' {X}: CR 106.6, ENGINE.md 49).
fn convert(m: ManaMask) -> ManaMask {
    if m & TYPES == 0 || m & RESTRICTED != 0 {
        m
    } else {
        m | COLORS
    }
}

/// Every multiset of `k` units of `groups` (kind, count), as counts per
/// group in `cur`, until `f` says stop (returned).
fn each_subset(groups: &[(u8, u8)], k: usize, cur: &mut SVec<u8, 12>, f: &mut dyn FnMut(&[u8]) -> bool) -> bool {
    let i = cur.len();
    if i == groups.len() {
        return k == 0 && f(cur);
    }
    let left: usize = groups[i + 1..].iter().map(|g| g.1 as usize).sum();
    let lo = k.saturating_sub(left);
    let hi = k.min(groups[i].1 as usize);
    if lo > hi {
        return false;
    }
    for n in (lo..=hi).rev() {
        cur.push(n as u8);
        let stop = each_subset(groups, k - n, cur, f);
        cur.truncate(i);
        if stop {
            return true;
        }
    }
    false
}

/// The units of `counts` of `groups` as masks (`MADE` for kinds + 8,
/// `RESTRICTED` for kinds + 16).
fn masks_of(groups: &[(u8, u8)], counts: &[u8]) -> SVec<ManaMask, 16> {
    let mut v = SVec::new();
    for (g, &n) in groups.iter().zip(counts) {
        let m = (1u8 << (g.0 & 7)) | if g.0 & 8 != 0 { MADE } else { 0 } | if g.0 & 16 != 0 { RESTRICTED } else { 0 };
        for _ in 0..n {
            v.push(m);
        }
    }
    v
}

/// Feasibility candidates. A card's mana abilities are mutually exclusive
/// (each taps, sacrifices or exiles it; the lowering checks), so each card
/// contributes at most one way to help. A one-mana way is a unit carrying
/// its own budget use; several single-mana ways with the same use merge into
/// one unit. Otherwise the card is a list of ways to try (each with the
/// budget its activation takes, charged once, all its mana or none; an
/// ability with a mana cost adds its need). With `dmg`, the damage an
/// ability deals its controller weighs like life.
pub(crate) struct Cands {
    fixed: Units,
    alts: Vec<Vec<Way>>,
    /// Wizard's Rockets can be used: the payment's mana may be of any
    /// colour. Each usable one: the budget its activation takes, and the
    /// index in `alts` of its card's other ways (Urza's "tap an untapped
    /// artifact", improvise), which its activation rules out: it taps and
    /// sacrifices the card (CR 601.2g; ENGINE.md 49).
    convert: Vec<(Way, Option<usize>)>,
    /// It was picked: every unit is of any colour.
    converted: bool,
    /// How many Lumberjacks there are to tap (`Budget::tap`).
    tap_limit: i32,
    /// Units of sources that take one of the shared taps and whose cards
    /// have no other way (Gene Pollinator tapping a permanent without a
    /// mana ability): as many as the taps left allow are fixed units
    /// (they are worth as much as any other such source, and cost its card
    /// nothing: no branching on them; ENGINE.md 99.12).
    free_shared: Units,
}

/// Candidate units, inline up to 32 (ENGINE.md 17).
type Units = SVec<Unit, 32>;

#[derive(Clone, Debug)]
struct Way {
    units: SVec<Unit, 4>,
    life: i32,
    sac: i32,
    /// The ability's own mana cost (`Class::need`), 0 for none, and how
    /// many such needs (`Class::need_n`).
    need: ManaMask,
    need_n: u8,
    /// It taps a Lumberjack (`Budget::tap`).
    tap: i32,
}

/// `used`: what the payment's picks use of a source's card already
/// (`Class::res`) and how many times it was picked (a source whose card
/// use is taken, or picked as often as its class allows, is out).
fn cands(
    classes: &[Class],
    use_src: &dyn Fn(&Src) -> bool,
    used: &dyn Fn(&Src) -> (u8, u8),
    dmg: bool,
    reserved: &dyn Fn(&Class, &Src) -> i32,
    allow: u8,
) -> Cands {
    // (Sources whose mana this payment may not spend are left out.)
    let filtered: Vec<Class>;
    let classes = if classes.iter().any(|cl| !crate::eoe_w2::spendable(cl.only, allow)) {
        filtered = classes.iter().filter(|cl| crate::eoe_w2::spendable(cl.only, allow)).cloned().collect();
        &filtered[..]
    } else {
        classes
    };
    // How many times each member may still be used (in order), and how
    // many usable abilities each card has: a card with one simple way goes
    // straight to the fixed units.
    let mut lefts: SVec<u8, 32> = SVec::new();
    let mut count: SVec<(CardIdx, u8), 32> = SVec::new();
    for cl in classes {
        for s in cl.members.iter() {
            let n = if !use_src(s) {
                0
            } else {
                let (res, k) = used(s);
                if cl.res == 0 {
                    cl.reuse.saturating_sub(k)
                } else {
                    (res & cl.res == 0) as u8
                }
            };
            lefts.push(n);
            if n > 0 {
                match count.iter_mut().find(|(c, _)| *c == s.card) {
                    Some((_, k)) => *k += 1,
                    None => count.push((s.card, 1)),
                }
            }
        }
    }
    // Gene Pollinator's sources (`POL_FOR`): with as many permanents
    // without another way as Pollinators can activate, a source on a
    // permanent with another way (or on a Pollinator, two taps) is never
    // better than one of those: left out, so the permanent stays one unit
    // (no branching on every land; ENGINE.md 99.12).
    if classes.iter().any(|cl| cl.members[0].slot == POL_FOR) {
        let k = classes.iter().filter(|cl| cl.share > 0).map(|cl| cl.share as usize).min().unwrap_or(0);
        let mut other: SVec<(CardIdx, u8), 32> = SVec::new();
        let mut i = 0;
        for cl in classes {
            for s in cl.members.iter() {
                if lefts[i] > 0 && cl.members[0].slot != POL_FOR {
                    match other.iter_mut().find(|(c, _)| *c == s.card) {
                        Some((_, n)) => *n += 1,
                        None => other.push((s.card, 1)),
                    }
                }
                i += 1;
            }
        }
        let free_member = |cl: &Class, s: &Src| cl.taps <= 1 && !other.iter().any(|&(c, n)| c == s.card && n > 0);
        let mut free = 0;
        i = 0;
        for cl in classes {
            for s in cl.members.iter() {
                free += (lefts[i] > 0 && cl.members[0].slot == POL_FOR && free_member(cl, s)) as usize;
                i += 1;
            }
        }
        if free >= k {
            i = 0;
            for cl in classes {
                for s in cl.members.iter() {
                    if lefts[i] > 0 && cl.members[0].slot == POL_FOR && !free_member(cl, s) {
                        lefts[i] = 0;
                        if let Some((_, n)) = count.iter_mut().find(|(c, _)| *c == s.card) {
                            *n -= 1;
                        }
                    }
                    i += 1;
                }
            }
        }
    }
    let mut li = 0;
    let alone = |c: CardIdx| count.iter().any(|&(x, n)| x == c && n == 1);
    let mut out = Cands { fixed: Units::new(), alts: Vec::new(), convert: Vec::new(), converted: false, tap_limit: i32::MAX, free_shared: Units::new() };
    let mut per_card: Vec<(CardIdx, Vec<(Way, u8)>)> = Vec::new();
    // The converters' cards, in `out.convert`'s order.
    let mut conv_cards: SVec<CardIdx, 2> = SVec::new();
    for cl in classes {
        let extra = if dmg { cl.dmg as i32 } else { 0 };
        let made = if cl.need != 0 { MADE } else { 0 };
        // (Mana with a spending restriction this payment meets: never
        // turned into colours, `convert`.)
        let restr = if cl.only != 0 { RESTRICTED } else { 0 };
        for s in cl.members.iter() {
            let n = lefts[li];
            li += 1;
            if n == 0 {
                continue;
            }
            let sac = reserved(cl, s);
            if cl.convert {
                // (Its card's other ways are kept apart, below: activating
                // it taps and sacrifices the card, so they are out then.)
                out.convert.push((Way { units: SVec::new(), life: extra, sac, need: 0, need_n: 0, tap: 0 }, None));
                conv_cards.push(s.card);
                continue;
            }
            let tap = if cl.share > 0 { cl.taps.max(1) as i32 } else { 0 };
            if tap > 0 {
                out.tap_limit = out.tap_limit.min(cl.share as i32);
            }
            if let Yield::One(u) = &cl.yld {
                if tap == 1 && n == 1 && alone(s.card) && sac == 0 && extra == 0 && u.life == 0 {
                    out.free_shared.push(Unit::free(u.mask | restr));
                    continue;
                }
            }
            match &cl.yld {
                // (A source taking a shared tap, Gene Pollinator's, stays a
                // way: `tap`.)
                Yield::One(u) if alone(s.card) && tap == 0 => {
                    for _ in 0..n {
                        out.fixed.push(Unit { mask: u.mask | restr, life: (u.life as i32 + extra) as u8, sac: sac > 0 });
                    }
                    continue;
                }
                Yield::Multi { units: Some(us), .. } if alone(s.card) && extra == 0 && sac == 0 && cl.need == 0 && tap == 0 => {
                    out.fixed.extend(us.iter().map(|u| Unit::free(u.mask | restr)));
                    continue;
                }
                _ => {}
            }
            let multi = |units: SVec<Unit, 4>| Way { units, life: extra, sac, need: cl.need, need_n: cl.need_n, tap };
            let ways: Vec<Way> = match &cl.yld {
                Yield::One(u) => {
                    let units = std::iter::repeat_n(Unit::free(u.mask | restr), n as usize).collect();
                    vec![Way { units, life: u.life as i32 + extra, sac, need: 0, need_n: 0, tap }]
                }
                Yield::Multi { units: Some(us), .. } => {
                    vec![multi(us.iter().map(|u| Unit::free(u.mask | made | restr)).collect())]
                }
                Yield::Multi { prods, units: None } => {
                    prods.iter().map(|p| multi(units_of(p).map(|u| Unit::free(u.mask | made | restr)).collect())).collect()
                }
            };
            let ways = ways.into_iter().map(|w| (w, cl.res));
            match per_card.iter_mut().find(|(c, _)| *c == s.card) {
                Some((_, w)) => w.extend(ways),
                None => per_card.push((s.card, ways.collect())),
            }
        }
    }
    // A single unit carries its budget use itself.
    let as_unit = |w: &Way| Unit { life: w.life as u8, sac: w.sac > 0, ..w.units[0] };
    for (card, tagged) in per_card {
        // A tap way and a sacrifice way of one card combine (tap it for
        // mana, then sacrifice it: CR 601.2g; Rain of Filth, Squandered
        // Resources, Orcish Lumberjack).
        let mut ways: Vec<Way> = tagged.iter().map(|(w, _)| w.clone()).collect();
        for (t, _) in tagged.iter().filter(|(_, r)| *r == TAP) {
            for (s, _) in tagged.iter().filter(|(_, r)| *r == SAC) {
                if t.need != 0 && s.need != 0 {
                    continue; // (one need per way; no such pair in the cube)
                }
                let mut units = t.units.clone();
                units.extend(s.units.iter().copied());
                ways.push(Way {
                    units,
                    life: t.life + s.life,
                    sac: t.sac + s.sac,
                    need: t.need | s.need,
                    need_n: t.need_n.max(s.need_n),
                    tap: t.tap + s.tap,
                });
            }
        }
        // A converter's card (Wizard's Rockets with Urza's "tap an untapped
        // artifact", or improvise): its other ways are an alternative of
        // their own, which its conversion leaves out (`cands_payable_c`).
        if let Some(k) = conv_cards.iter().position(|&c| c == card) {
            out.convert[k].1 = Some(out.alts.len());
            out.alts.push(ways);
            continue;
        }
        // A card whose ways take no budget and one of which makes every
        // other's units (a tap-then-sacrifice): that way stands for the card
        // (more units never make a cost unpayable), so the search doesn't
        // branch on it.
        if ways.len() > 1 && ways.iter().all(|w| w.life == 0 && w.sac == 0 && w.need == 0 && w.tap == 0) {
            if let Some(k) = (0..ways.len()).find(|&k| ways.iter().all(|w| covers(&ways[k].units, &w.units))) {
                out.fixed.extend(ways[k].units.iter().map(|u| Unit::free(u.mask)));
                continue;
            }
        }
        let ways = std::mem::take(&mut ways);
        let same_use = |a: &Way, b: &Way| (a.life, a.sac, a.need, a.need_n) == (b.life, b.sac, b.need, b.need_n);
        if ways.iter().all(|w| w.units.len() == 1 && w.need == 0 && w.tap == 0 && same_use(w, &ways[0])) {
            let mask = ways.iter().fold(0, |m, w| m | w.units[0].mask);
            out.fixed.push(Unit { mask, ..as_unit(&ways[0]) });
        } else {
            out.alts.push(ways);
        }
    }
    out
}

/// Can the units `a` make every unit of `b` (each of `b`'s units by its own
/// unit of `a` whose mask holds it)? A bipartite matching by augmenting
/// paths: a way may have any number of units (Gaea's Cradle's, one per
/// creature, 255 at most), and the search this replaces kept the units it
/// had used in eight bits, so past eight units it went through every
/// permutation of them and never ended (fuzz game 10049522: an animated
/// Cradle's 28 {G} against its own tap for Aang's waterbend; ENGINE.md 51).
fn covers(a: &[Unit], b: &[Unit]) -> bool {
    fn augment(j: usize, a: &[Unit], b: &[Unit], seen: &mut [bool], owner: &mut [usize]) -> bool {
        for i in 0..a.len() {
            if !seen[i] && b[j].mask & !a[i].mask == 0 {
                seen[i] = true;
                if owner[i] == usize::MAX || augment(owner[i], a, b, seen, owner) {
                    owner[i] = j;
                    return true;
                }
            }
        }
        false
    }
    if b.len() > a.len() {
        return false;
    }
    let mut owner: SVec<usize, 8> = std::iter::repeat_n(usize::MAX, a.len()).collect();
    let mut seen: SVec<bool, 8> = std::iter::repeat_n(false, a.len()).collect();
    (0..b.len()).all(|j| {
        seen.fill(false);
        augment(j, a, b, &mut seen, &mut owner)
    })
}

/// [`payable_within_n`] over candidates (with `marked` needs and every unit
/// of `chosen`), trying each way (or none) of the cards that have several
/// or whose activation takes budget, then Wizard's Rockets' colours.
fn cands_payable(cost: &ManaCost, marked: &[ManaMask], chosen: &[ManaMask], c: &Cands, budget: Budget) -> bool {
    cands_payable_c(cost, marked, chosen, c, budget, true)
}

/// [`cands_payable`], Wizard's Rockets' colours only with `convert`.
fn cands_payable_c(cost: &ManaCost, marked: &[ManaMask], chosen: &[ManaMask], c: &Cands, budget: Budget, convert: bool) -> bool {
    let budget = Budget { tap: budget.tap.min(c.tap_limit), ..budget };
    // (The shared taps' free units first: `Cands::free_shared`.)
    let with_free;
    let (c, budget) = if c.free_shared.is_empty() {
        (c, budget)
    } else {
        let k = c.free_shared.len().min(budget.tap.max(0) as usize);
        let mut fixed = c.fixed.clone();
        fixed.extend(c.free_shared.iter().take(k).copied());
        with_free = Cands {
            fixed,
            alts: c.alts.clone(),
            convert: c.convert.clone(),
            converted: c.converted,
            tap_limit: c.tap_limit,
            free_shared: Units::new(),
        };
        (&with_free, Budget { tap: budget.tap - k as i32, ..budget })
    };
    if c.converted {
        return payable_converted(cost, marked, chosen, c, budget, None);
    }
    if ways_payable(cost, marked, chosen, &c.fixed, &c.alts, budget) {
        return true;
    }
    // Each usable Wizard's Rockets, without its card's other ways.
    convert
        && c.convert.iter().any(|(w, skip)| {
            let left = Budget { life: budget.life - w.life, sac: budget.sac - w.sac, tap: budget.tap };
            left.life >= 0 && left.sac >= 0 && payable_converted(cost, marked, chosen, c, left, *skip)
        })
}

/// [`cands_payable`] with every unit of any colour (Wizard's Rockets),
/// without the ways `alts[skip]` (the converter's card's others).
fn payable_converted(cost: &ManaCost, marked: &[ManaMask], chosen: &[ManaMask], c: &Cands, budget: Budget, skip: Option<usize>) -> bool {
    let chosen: SVec<ManaMask, 16> = chosen.iter().map(|&m| convert(m)).collect();
    let fixed: Units = c.fixed.iter().map(|u| Unit { mask: convert(u.mask), ..*u }).collect();
    let alts: Vec<Vec<Way>> = c
        .alts
        .iter()
        .enumerate()
        .filter(|&(k, _)| Some(k) != skip)
        .map(|(_, ws)| ws.iter().map(|w| Way { units: w.units.iter().map(|u| Unit { mask: convert(u.mask), ..*u }).collect(), ..w.clone() }).collect())
        .collect();
    ways_payable(cost, marked, &chosen, &fixed, &alts, budget)
}

fn ways_payable(cost: &ManaCost, marked: &[ManaMask], chosen: &[ManaMask], fixed: &Units, alts: &[Vec<Way>], budget: Budget) -> bool {
    // (`None`: the search's nodes ran out.)
    #[allow(clippy::too_many_arguments)]
    fn go(
        cost: &ManaCost,
        needs: &mut SVec<ManaMask, 8>,
        chosen: &[ManaMask],
        units: &mut Units,
        alts: &[Vec<Way>],
        b: Budget,
        nodes: &mut u32,
    ) -> Option<bool> {
        *nodes = nodes.checked_sub(1)?;
        let Some((first, rest)) = alts.split_first() else {
            return Some(payable_within_n(cost, needs, chosen, units, b));
        };
        for w in first {
            let left = Budget { life: b.life - w.life, sac: b.sac - w.sac, tap: b.tap - w.tap };
            if left.life < 0 || left.sac < 0 || left.tap < 0 {
                continue;
            }
            let (len, nl) = (units.len(), needs.len());
            units.extend(w.units.iter().copied());
            if w.need != 0 {
                needs.extend(std::iter::repeat_n(w.need, w.need_n as usize));
            }
            let ok = go(cost, needs, chosen, units, rest, left, nodes);
            units.truncate(len);
            needs.truncate(nl);
            if ok? {
                return Some(true);
            }
        }
        go(cost, needs, chosen, units, rest, b, nodes)
    }
    if alts.is_empty() {
        return payable_within_n(cost, marked, chosen, fixed, budget);
    }
    let mut units = fixed.clone();
    let mut needs: SVec<ManaMask, 8> = marked.iter().copied().collect();
    let mut nodes = SEARCH_NODES;
    go(cost, &mut needs, chosen, &mut units, alts, budget, &mut nodes).unwrap_or_else(|| {
        search_exhausted(format_args!(
            "cost {cost}, needs {marked:?}, chosen {chosen:?}, fixed units {fixed:?}, cards with ways {alts:?}, budget {budget:?}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(masks: &[ManaMask]) -> Vec<Unit> {
        masks.iter().map(|&m| Unit::free(m)).collect()
    }

    /// `covers` is a matching whatever the number of units (ENGINE.md 51:
    /// the search it replaced kept its used units in eight bits and never
    /// ended past eight: Gaea's Cradle's 28 {G}).
    #[test]
    fn covers_matches_any_number_of_units() {
        const W: ManaMask = 1;
        const G: ManaMask = 1 << 4;
        let g = |n: usize| units(&vec![G; n]);
        assert!(covers(&g(255), &g(255)));
        assert!(covers(&g(28), &units(&[0])));
        assert!(!covers(&units(&[0]), &g(28)));
        assert!(!covers(&g(254), &g(255)));
        let mut b = g(30);
        b.push(Unit::free(W));
        assert!(!covers(&g(40), &b), "no unit makes {{W}}");
        let mut a = g(30);
        a.push(Unit::free(W | G));
        assert!(covers(&a, &b), "the {{W}}{{G}} unit for the {{W}}");
        // (An augmenting path: the first {G} taken by the any-colour unit
        // moves to a plain one.)
        assert!(covers(&units(&[G | W, G]), &units(&[G, W])));
        assert!(!covers(&units(&[G | W, G]), &units(&[W, W])));
    }

    /// A feasibility search past `SEARCH_NODES` is a loud failure that
    /// prints its inputs, never an answer it did not find (ENGINE.md 51):
    /// 21 cards each of two ways ({C} or {R}), {W} paid by none of them,
    /// 3^21 paths.
    #[test]
    #[should_panic(expected = "a feasibility search visited")]
    fn an_exhausted_feasibility_search_panics_with_its_inputs() {
        let way = |m: ManaMask| Way { units: std::iter::once(Unit::free(m)).collect(), life: 0, sac: 0, need: 0, need_n: 0, tap: 0 };
        let alts: Vec<Vec<Way>> = (0..21).map(|_| vec![way(1 << 5), way(1 << 3)]).collect();
        let cost = ManaCost::parse("{W}").unwrap();
        ways_payable(&cost, &[], &[], &Units::new(), &alts, Budget::FREE);
    }
}
