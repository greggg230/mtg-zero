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
use crate::mana::{
    payable_n, payable_within_n, purposes, restriction, search_exhausted, tag, Budget, ManaCost, ManaMask, Tag, Unit, COLORS, FOR_COST, MADE,
    RESTRICTED, SEARCH_NODES, TYPES,
};
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

/// The sources a payment offers by their permanent's own id (`MY_PERM`,
/// `PayMana::card_slots`, then which one when it stands for several): a
/// permanent tapped for improvise, convoke or another's mana ability, and
/// an ability at a place past the permanent's four `ACTIVATE` ids (a
/// granted one, ENGINE.md 100.4.5; docs/shortcuts.md 31.7.2k).
pub(crate) fn by_perm(slot: AbilitySlot) -> bool {
    slot == IMPROVISE || slot == TAP_FOR || slot == POL_FOR || matches!(slot, AbilitySlot::Activated(i) if i >= 4)
}

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
    /// A shared activation's pool (`Class::pool`: Gene Pollinators or Orcish
    /// Lumberjacks alike, ENGINE.md 100.4.7), 0 otherwise: one permanent is
    /// a source of each pool that may tap or sacrifice it.
    pub g: u16,
}

impl Src {
    /// Source `slot` of `card`, of no pool.
    pub(crate) const fn of(card: CardIdx, slot: AbilitySlot) -> Src {
        Src { card, slot, g: 0 }
    }
}

impl Default for Src {
    /// (A filler for inline storage, never a real source.)
    fn default() -> Src {
        Src::of(CardIdx(0), AbilitySlot::Spell)
    }
}

/// The members of a class, inline up to eight (ENGINE.md 17).
pub(crate) type Members = SVec<Src, 8>;

/// The chain id (`mana::chain`) of the `k`-th activation in a payment of
/// source `s` of `classes`, an ability with a mana cost (docs/shortcuts.md
/// 13.15): its place among such sources of the classes (from 1), its uses
/// after it (a source that only removes counters is used several times); 0
/// for another source. The classes of a payment are built alike at each of
/// its steps, so its picks' ids and its candidates' agree.
fn chain_id(classes: &[Class], s: &Src, k: crate::num::Count) -> u64 {
    let mut i = 0u64;
    for cl in classes.iter().filter(|cl| cl.need != 0) {
        for m in cl.members.iter() {
            i += 1;
            if m == s {
                assert!(k < 1 << 24 && i < 1 << 24, "chain ids: {i}, use {k}");
                return i << 24 | k as u64;
            }
        }
    }
    0
}

/// Converge (exact, any number of units): the most colours (with `fixed`,
/// already spent) an exact payment of `cost` and of the `marked` needs (the
/// mana costs of mana abilities activated in the payment: not spent on the
/// spell) by one mana from each of `units` (the types each can make) can
/// show. A colour set S is reachable iff the units can be matched to the
/// coloured pips, the marked needs and one mana of each colour of S not yet
/// shown (the rest pay the generic part): a bipartite matching per S.
/// `None` with hybrid or Phyrexian pips (not modelled here).
fn most_colors_matching(units: &[Tag], cost: &ManaCost, marked: &[Tag], fixed: u8, allow: u8) -> Option<u32> {
    if cost.hybrid.iter().any(|&n| n > 0) || cost.phyrexian.iter().any(|&n| n > 0) {
        return None;
    }
    if cost.pips.iter().map(|&n| n as usize).sum::<usize>() + cost.generic as usize + marked.len() != units.len() {
        return None; // (not an exact payment by these units)
    }
    let mut best = None;
    for s in 0u8..32 {
        // (Each colour shown not among the pips: one generic pip of it;
        // every unit used, exactly: `payable_n`, which tries the orders of
        // the chained mana abilities, 13.15.)
        let mut c = *cost;
        let mut shows = 0;
        for t in 0..5 {
            if s & (1 << t) != 0 && fixed & (1 << t) == 0 && cost.pips[t] == 0 {
                c.pips[t] = 1;
                shows += 1;
            }
        }
        if shows > cost.generic {
            continue;
        }
        c.generic -= shows;
        let ok = crate::mana::payable_n(&c, allow, marked, units, &[]);
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
    /// itself). Its purposes (`Tag`): what activating the ability is, the
    /// spending restrictions it satisfies (`Game::ability_allow`, CR 106.6).
    pub need: Tag,
    /// How many such needs its cost is: 1, or n for {n} (Cascading
    /// Cataracts' {5}; ENGINE.md 99.12).
    pub need_n: u8,
    /// How many times a member may be activated in one payment (Pentad
    /// Prism: once per charge counter, counted exactly; 1 otherwise).
    pub reuse: crate::num::Count,
    /// "{X}, {T}, sacrifice this: add X mana in any combination of colors"
    /// (Wizard's Rockets): picked, the payment's mana may be of any colour
    /// (its X is paid by the mana it turns into colours).
    pub convert: bool,
    /// Its mana gives haste to the creature spell it is spent on (Arena of
    /// Glory, `ManaPool::haste`).
    pub haste: bool,
    /// What its activation uses of its card (`TAP`, `SAC`; both by default;
    /// 0 for an ability that only removes counters, Pentad Prism's, used
    /// `reuse` times besides the card's other sources): a card's sources
    /// that use different ones combine (CR 601.2g).
    pub res: u8,
    /// The `ACTIVATE` slot of a sacrifice for another's or a granted
    /// ability (`SAC_FOR`, `SQ_FOR`, `LJ_FOR`): after the card's own.
    pub slot_id: u8,
    /// A shared activation (ENGINE.md 100.4.7, docs/shortcuts.md N23): the
    /// pool of permanents one of which activates it and is tapped (Orcish
    /// Lumberjacks for `LJ_FOR`, Gene Pollinators for `POL_FOR`; 1.., one per
    /// group of actors whose activations are alike: the same mana, with the
    /// bonus their tap adds, and the same permanents they may tap or
    /// sacrifice); 0 for none. Every source of the payment that taps one of
    /// a pool's actors leaves the pool one fewer (`PoolCtx`).
    pub pool: u16,
    /// The permanents that may activate it: `pool`'s actors, or the
    /// Squandered Resources that may sacrifice the land (`SQ_FOR`, no
    /// pool: it taps nothing). Which one does is the player's choice
    /// (`PayMana::actors`).
    pub actors: Vec<CardIdx>,
    /// Its card, sacrificed for this mana, is in the graveyard before a
    /// delve spell's cards are exiled (CR 601.2g before 601.2h) and pays
    /// {1} of it too: a typeless unit with each production
    /// (`Game::delve_classes`, a several-mana source; docs/shortcuts.md
    /// 13.16).
    pub delve: bool,
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
            pool: 0,
            actors: Vec::new(),
            delve: false,
        }
    }
}

/// A mana source a delve or "exile n cards from your graveyard" can count
/// on: it sacrifices itself (so its card is in the graveyard by 601.2h),
/// has no mana cost of its own, and is the card's own ability or a granted
/// one (not another permanent's sacrifice of it); one mana or several, or
/// Wizard's Rockets' (docs/shortcuts.md 13.16).
pub(crate) fn delve_source(cl: &Class) -> bool {
    cl.sac_self && cl.need == 0 && !is_sac_slot(cl.members[0].slot)
}

/// The mana cost of a mana ability as `NOT_MADE` needs: one mana, or {n}
/// (n needs of any type; the lowering checks), paying which is `allow`
/// (`Game::ability_allow` of its source): (tag, how many; 0: none).
fn need_of(cost: &ManaCost, allow: u8) -> (Tag, u8) {
    let pip = (0..6).find(|&t| cost.pips[t] > 0).map(|t| 1u8 << t);
    let hyb = (0..10).find(|&i| cost.hybrid[i] > 0).map(crate::mana::pair_mask);
    let generic = (cost.generic > 0).then_some(TYPES);
    let n = if pip.or(hyb).is_some() { 1 } else { cost.generic };
    (pip.or(hyb).or(generic).map_or(0, |m| tag(m | crate::mana::NOT_MADE, allow)), n)
}

/// The purposes of the needs classes' abilities may add to a payment
/// (`Class::need`), together.
pub(crate) fn needs_allow(classes: &[Class]) -> u8 {
    classes.iter().filter(|cl| cl.need != 0).fold(0, |a, cl| a | purposes(cl.need))
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
    /// Life later parts of the cost take (a life cost, "pay X life",
    /// Bolas's Citadel's): any amount up to a life total (u32: exact).
    pub life: u32,
    /// The later parts of the cost that take permanents or cards in hand a
    /// mana source might use (`Reserve`), each with the card whose cost it
    /// is (never one of them): every one must find its candidates left,
    /// together (ENGINE.md 100.4.7: one was kept before; docs/shortcuts.md
    /// 24.7.5k, 40.1).
    pub reserves: Vec<(Reserve, CardIdx)>,
    /// The spending restrictions this payment satisfies (`SPEND_*` bits:
    /// casting an artifact / legendary spell; ENGINE.md 28).
    pub allow: u8,
    /// The spell being cast, if this pays for one ("that spell can't be
    /// countered").
    pub spell: Option<CardIdx>,
    /// A tap cost paid after the mana abilities (crew, saddle, teamwork,
    /// "tap n untapped <permanents>": CR 601.2h after 601.2g), with its
    /// candidates and what each counts for (`TapsOwed`).
    pub taps: Option<TapsOwed>,
    /// Permanents a cost already tapped (teamwork's creatures, tapped
    /// before the mana in the engine's order): by the rules they were tapped
    /// after the mana abilities (601.2h after 601.2g), so no mana ability
    /// of the payment taps or sacrifices them.
    pub committed: Vec<CardIdx>,
    /// What the cost takes of its own source after the mana (`SelfUse`).
    pub self_use: Option<SelfUse>,
}

/// A later part of a cost that takes permanents or cards in hand (CR 601.2h
/// after 601.2g: the mana abilities come first, so a mana source that uses
/// one of its candidates leaves it one fewer).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Reserve {
    /// Permanents to sacrifice, return to their owner's hand or remove a
    /// counter from (`SacCost`: n matching, "another"); with `true`, to tap
    /// ("tap X untapped artifacts you control", EoE W3: a source that taps
    /// one uses it up too).
    Perm(SacCost, bool),
    /// n cards in hand to discard, matching the filter if any (retrace's
    /// "a land card").
    Discard(u8, Option<Filter>),
    /// A card in hand matching the filter to exile.
    Exile(Filter),
}

/// The candidates left for a cost's several reservations (`Keep::reserves`,
/// ENGINE.md 100.4.7), grouped by the reservations they are candidates of
/// (a mask): a payment holds when, for every set of reservations, the
/// candidates of any of them left are at least what they need together
/// (Hall's condition: then distinct candidates can be given to each; a
/// candidate serves one reservation, a card discarded is not exiled and a
/// permanent sacrificed is not returned). A mana source that sacrifices a
/// permanent or exiles a card in hand uses its candidate up (`Way::rsig`).
/// Empty with one reservation or none (`Budget::sac` counts it).
#[derive(Clone, Debug, Default)]
pub(crate) struct ResCtx {
    /// Per group: the mask of its reservations, how many candidates are left.
    groups: Vec<(u8, i32)>,
    /// Each candidate left, with its group.
    cards: Vec<(CardIdx, u16)>,
    /// What each reservation needs.
    need: SVec<i32, 4>,
}

impl ResCtx {
    /// The group of candidate `c` (`NO_POOL`: none).
    fn group_of(&self, c: CardIdx) -> u16 {
        self.cards.iter().find(|x| x.0 == c).map_or(NO_POOL, |x| x.1)
    }

    /// Hall's condition with `used` candidates of each group used up.
    fn ok(&self, used: &[i32]) -> bool {
        let k = self.need.len();
        (1u32..(1 << k)).all(|s| {
            let need: i32 = (0..k).filter(|&r| s & (1 << r) != 0).map(|r| self.need[r]).sum();
            let have: i32 = self.groups.iter().enumerate().filter(|(_, g)| g.0 as u32 & s != 0).map(|(i, g)| g.1 - used.get(i).copied().unwrap_or(0)).sum();
            have >= need
        })
    }
}

/// What a cost takes of its own source, paid after the mana abilities (CR
/// 601.2h after 601.2g; docs/shortcuts.md 13.3): it taps it, moves it
/// (sacrifice, exile, discard), removes counters from it. Only the
/// source's mana abilities that conflict are out of the payment
/// (`Game::classes_for_self_use`): one that taps or moves it when the cost
/// taps it, one that moves it when the cost moves it, and uses of one that
/// removes the same counters past what the cost leaves. Tapping a land for
/// mana and then sacrificing it for "{1}, Sacrifice this land" is one
/// payment.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct SelfUse {
    pub card: CardIdx,
    pub tap: bool,
    pub moves: bool,
    pub counters: Option<(crate::db::Counter, u8)>,
}

impl SelfUse {
    /// `card`'s cost `cost`'s use of `card`, if any.
    pub(crate) fn of(card: CardIdx, cost: &crate::db::Cost) -> Option<SelfUse> {
        let su = SelfUse { card, tap: cost.tap, moves: cost.sac_self || cost.exile_self || cost.discard_self, counters: cost.remove_counters };
        (su.tap || su.moves || su.counters.is_some()).then_some(su)
    }
}

/// A tap cost the mana payment must leave payable (docs/shortcuts.md
/// 27.13.4, N8): `need` power (crew, saddle, teamwork) or permanents ("tap n
/// untapped <permanents>": each counts 1) from `cands` (each with what it
/// counts for). A mana ability that taps or sacrifices a candidate (or taps
/// it as a Pollinator or a Lumberjack, `Class::share`) makes it unable to
/// pay this; the feasibility tries each candidate both ways (`Way::crew`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub(crate) struct TapsOwed {
    pub need: i64,
    pub cands: Vec<(CardIdx, i64)>,
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
    pub marked: Vec<Tag>,
    /// Several-mana sources picked, with their productions.
    pub multi: Vec<(Src, [u8; 6])>,
    /// Their mana (spent, or left in the pool at the end).
    pub extra: [u8; 6],
    /// The part of `extra` with a spending restriction, by kind (the
    /// Workshop's artifact-only {C} among it; ENGINE.md 99.12).
    pub extra_restr: Vec<crate::eoe_w2::RKind>,
    /// The part of `extra` made by abilities with a mana cost (`MADE`).
    pub extra_made: [u8; 6],
    /// That part by activation: its chain id (`chain_id`) and production
    /// (the order of such activations is the payment's: a `MADE` unit pays
    /// the cost of one after its own, `mana::chain`; docs/shortcuts.md 13.15).
    pub made_by: Vec<(u64, [u8; 6])>,
    /// The part of `extra_restr` made by abilities with a mana cost.
    pub extra_made_restr: Vec<crate::eoe_w2::RKind>,
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
    pub vm: Option<u32>,
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
    pub life_after: u32,
    /// What later parts of the cost need.
    pub keep: Keep,
    /// Life to pay for Phyrexian pips once the mana abilities are done.
    pub phyrexian_life: u32,
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
    pub card_slots: Vec<(CardIdx, Vec<(AbilitySlot, u16)>)>,
    pub which: Option<CardIdx>,
    /// The permanent that activates each shared source picked (`Class::
    /// actors`: which Gene Pollinator or Orcish Lumberjack taps, which
    /// Squandered Resources sacrifices; CR 602.2b, docs/shortcuts.md N23),
    /// chosen right after the pick (`actor_for`).
    pub actors: Vec<(Src, CardIdx)>,
    pub actor_for: Option<Src>,
    /// A production chosen one mana at a time (more productions than
    /// `NUMBER` ids: Cascading Cataracts' five of any colours): the mana
    /// chosen so far and the least colour the next may be (colours in
    /// non-decreasing order, each production once; ENGINE.md 99.12).
    pub now_taken: [u8; 6],
    pub now_min: u8,
    /// The page of the `PayPool` decision (A0's `ask_paged`: more pools
    /// than `NUMBER` ids), or of the question which source a permanent
    /// picked by its own id stands for (more than `NUMBER` ids).
    pub pool_page: crate::decision::Page,
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
            made_by: Vec::new(),
            extra_made_restr: Vec::new(),
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
            actors: Vec::new(),
            actor_for: None,
            now_taken: [0; 6],
            now_min: 0,
            pool_page: crate::decision::Page::FIRST,
        }
    }

    /// The types of untapped permanents that may pay for {1} each.
    pub(crate) fn improvise_types(&self) -> u16 {
        (if self.improvise { crate::db::types::ARTIFACT } else { 0 })
            | (if self.waterbend { crate::db::types::ARTIFACT | crate::db::types::CREATURE } else { 0 })
    }

    /// How many times source `s` was picked (once, but Pentad Prism's
    /// counters).
    fn uses(&self, s: &Src) -> crate::num::Count {
        (self.chosen.iter().filter(|x| *x == s).count() + self.multi.iter().filter(|(x, _)| x == s).count()) as crate::num::Count
    }

    /// What the picks so far use of card `c` (`Class::res`; an actor of a
    /// shared source picked is tapped).
    fn used_res(&self, classes: &[Class], c: CardIdx) -> u8 {
        let acts = if self.actors.iter().any(|(s, a)| *a == c && s.slot != SQ_FOR) { TAP } else { 0 };
        self.chosen
            .iter()
            .chain(self.multi.iter().map(|m| &m.0))
            .filter(|s| s.card == c)
            .fold(acts, |m, s| m | classes.iter().find(|cl| cl.members.contains(s)).map_or(TAP | SAC, |cl| cl.res))
    }

    /// The picks of shared sources whose actor isn't chosen yet, as their
    /// pools (`Class::pool`).
    fn anonymous_acts(&self, classes: &[Class]) -> SVec<u16, 4> {
        self.chosen
            .iter()
            .chain(self.multi.iter().map(|m| &m.0))
            .filter(|s| s.g > 0 && !self.actors.iter().any(|(x, _)| x == *s))
            .filter_map(|s| classes.iter().find(|cl| cl.members.contains(s)).map(|cl| cl.pool).filter(|&g| g > 0))
            .collect()
    }

    /// Is member `s` of class `cl` still available?
    fn free(&self, classes: &[Class], cl: &Class, s: &Src) -> bool {
        if cl.res == 0 {
            self.uses(s) < cl.reuse
        } else {
            self.used_res(classes, s.card) & cl.res == 0
        }
    }

    /// The life the payment keeps for after its mana: the cost's later life
    /// and the Phyrexian pips' (in u64: exact).
    fn later_life(&self) -> u64 {
        self.keep.life as u64 + self.phyrexian_life as u64
    }

    /// The mana the payment needs: the cost and the picked abilities' own.
    fn need_total(&self) -> usize {
        self.cost.total() as usize + self.marked.len()
    }

    /// The picks so far and the virtual pool pay the rest (ENGINE.md
    /// 100.4.3: then `PASS` = spend from the pool, and the sources whose
    /// pick leaves the payment payable are offered too; 13.13).
    fn pool_covers(&self) -> bool {
        self.vm.is_some_and(|vm| self.chosen.len() + vm as usize == self.need_total())
    }
}

/// A mana ability's life cost, one unit's (`Unit::life`): at most 99 (the
/// lowering's bound on a printed life cost).
fn mana_life(ad: &ActivatedDef) -> u8 {
    u8::try_from(ad.cost.life).expect("a mana ability's life: at most 99, lowering")
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
        // (Saturating: at most 255 of a type, shortcuts.md 46.3.)
        ManaProd::ChosenN(n) => {
            let m = src.map_or(0, |c| g.chosen_land_color(c));
            (0..5).filter(|&t| m & (1 << t) != 0).map(|t| one(t, *n)).collect()
        }
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
    /// cost taps before the mana is paid: teamwork's creatures). A shared
    /// activation's pool (`Class::pool`) loses the actors among `cards` and
    /// goes when none is left; the other creatures tapped leave it
    /// (ENGINE.md 71.5: every shared class went whenever any creature was
    /// tapped, so a teamwork pick of a creature without a mana ability found
    /// the Lumberjack's mana gone that the cost's check, tapping no source,
    /// had counted).
    /// A source that only removes counters (`Class::res` 0) stays: it taps
    /// and sacrifices nothing (CR 601.2g).
    pub(crate) fn classes_without(&self, classes: &[Class], cards: &[CardIdx]) -> Vec<Class> {
        classes
            .iter()
            .filter_map(|cl| {
                let mut m = Members::new();
                for s in cl.members.iter() {
                    if cl.res == 0 || !cards.contains(&s.card) {
                        m.push(*s);
                    }
                }
                if m.is_empty() {
                    return None;
                }
                let mut actors = cl.actors.clone();
                if cl.pool > 0 {
                    actors.retain(|a| !cards.contains(a));
                    if actors.is_empty() {
                        return None;
                    }
                }
                Some(Class { members: m, actors, ..cl.clone() })
            })
            .collect()
    }

    /// The sources of payment `pm` (`pay_classes`), without those of the
    /// permanents a cost tapped already (`Keep::committed`: teamwork's
    /// creatures, docs/shortcuts.md 13.17).
    fn payment_classes(&self, pm: &PayMana) -> Vec<Class> {
        let classes = self.pay_classes(pm.payer, pm.keep.exclude, pm.improvise_types(), pm.convoke);
        let classes = if pm.keep.committed.is_empty() { classes } else { self.classes_without(&classes, &pm.keep.committed) };
        // (And the source's own conflicting sources, `SelfUse`.)
        match self.classes_for_self_use(&classes, pm.keep.self_use) {
            std::borrow::Cow::Borrowed(_) => classes,
            std::borrow::Cow::Owned(v) => v,
        }
    }

    /// The additional mana tapping permanent `x` for mana adds (CR 106.12,
    /// 605.1b: triggered mana abilities, resolved at once): `bonus`'s "whenever
    /// you tap a <land subtype> for mana" for each of its land subtypes, and
    /// `creature_bonus` (Badgermole Cub's "whenever you tap a creature for
    /// mana") when it is a creature and the ability has {T} (`tap`). It
    /// depends on the object: an actor of a shared activation adds its own
    /// (ENGINE.md 100.4.7).
    pub(crate) fn tap_bonus(&self, x: CardIdx, tap: bool, bonus: &[(u16, u8)], creature_bonus: &[u8; 6]) -> [u8; 6] {
        let mut extra = [0u8; 6];
        if !bonus.is_empty() && self.is_type(x, crate::db::types::LAND) {
            for &(sub, t) in bonus {
                if self.has_subtype(x, sub) {
                    extra[t as usize] += 1;
                }
            }
        }
        if *creature_bonus != [0; 6] && tap && self.is_creature(x) {
            for t in 0..6 {
                extra[t] += creature_bonus[t];
            }
        }
        extra
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

    /// The ability `slot` of a mana source names on permanent `c`: its own
    /// (a Mountain's while it is one, `source_def`) or, at a place past its
    /// own, one another permanent grants it (`Game::activated_home`:
    /// Mm'menon, the Right Hand's "artifacts you control have '{T}: Add
    /// {U}...'", CR 613.1f; ENGINE.md 100.4.5).
    pub(crate) fn src_ability(&self, c: CardIdx, slot: AbilitySlot) -> &'static ActivatedDef {
        if let AbilitySlot::Activated(i) = slot {
            if i as usize >= self.own_ability_slots(c) {
                let (d, s, _) = self.activated_home(c, slot);
                return self.db.def(d).activated_def(s);
            }
        }
        self.source_def(c).activated_def(slot)
    }

    /// Can a permanent have activated abilities past its own now (an
    /// attached Equipment with `grants_equipped`, a "<permanents> you
    /// control have" static, a basic land type given by an effect)?
    pub(crate) fn grants_activated_now(&self) -> bool {
        self.w3.seen & crate::eoe_w3::SEEN_W3_GRANT_ACT != 0
            || !self.db.basic_abilities.is_empty()
            || self.battlefield.iter().any(|&g| self.objs[g.i()].attached.is_some() && self.abilities(g).grants_equipped.is_some())
    }

    /// The mana sources `p` can use to pay a cost, by class (ENGINE.md 18):
    /// several-mana permanents, then single-mana permanents; within each,
    /// by first appearance in canonical order. A permanent's abilities are
    /// its own, then those other permanents grant it (CR 613.1f;
    /// `Game::granted_activated`, at the places past its own).
    pub(crate) fn mana_classes(&self, p: PlayerId, exclude: Option<CardIdx>) -> Vec<Class> {
        let mut out: Vec<Class> = Vec::with_capacity(8);
        let grants = self.grants_activated_now();
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
        let mut sac_by: Vec<(CardIdx, &ActivatedDef)> = Vec::new();
        for c in self.permanents_of(p) {
            if Some(c) == exclude {
                continue;
            }
            let def = self.objs[c.i()].def;
            let d = self.abilities(c);
            // (Granted abilities: the definition holding each, `home`.)
            let granted = if grants { self.granted_activated(c) } else { Vec::new() };
            let own = if granted.is_empty() { 0 } else { self.own_ability_slots(c) };
            let all = d.activated.iter().enumerate().map(|(i, ad)| (i, ad, None)).chain(
                granted.iter().enumerate().map(|(k, g)| (own + k, self.db.def(g.def).activated_def(AbilitySlot::Activated(g.slot)), Some((g.def, g.slot)))),
            );
            for (i, ad, home) in all {
                let Some(m) = ad.mana() else { continue };
                if ad.instant_only || !self.activation_ready(p, c, ad, locks) || (home.is_none() && d.latent > 0 && !self.has_activated(c, i)) {
                    continue;
                }
                if ad.exhaust && self.exhausted(c, i as u8) {
                    continue; // CR 702.177a: once
                }
                if ad.cost.sacrifice.is_some() {
                    sac_by.push((c, ad));
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
                // (`Some`: it only removes counters, `Class::res` 0.)
                let mut reuse: Option<crate::num::Count> = None;
                if let Some((k, n)) = ad.cost.remove_counters {
                    let have = self.counters_on(c, k);
                    if have < n as crate::num::Count {
                        continue;
                    }
                    sac_self |= m.sac_if_no == Some(k) && have == n as crate::num::Count;
                    // Without {T} (Pentad Prism): once per n counters, every
                    // one counted (docs/shortcuts.md N10).
                    if !(ad.cost.tap || ad.cost.sac_self || ad.cost.exile_self || ad.cost.discard_self) && !sac_self {
                        reuse = Some(have / n as crate::num::Count);
                    }
                }
                let src = Src::of(c, AbilitySlot::Activated(i as u8));
                // The additional mana tapping this source adds (its land
                // subtypes; a creature's), which depends on the object.
                let extra_of = |x: CardIdx| self.tap_bonus(x, ad.cost.tap, &bonus, &creature_bonus);
                let extra = extra_of(c);
                let bonuses = !bonus.is_empty() || creature_bonus != [0; 6];
                // Same definition and ability: the same yield (it depends on
                // the controller only; the linked colours on the source; the
                // additional mana of the object: an animated land of a
                // definition is a creature, another isn't).
                // (A granted ability: the same grant, `home`.)
                let same = |cl: &&mut Class| {
                    cl.def == def
                        && cl.members[0].slot == src.slot
                        && cl.sac_self == sac_self
                        && (!bonuses || extra_of(cl.members[0].card) == extra)
                        && home.is_none_or(|(hd, hs)| {
                            let (d2, s2, _) = self.activated_home(cl.members[0].card, src.slot);
                            (d2, s2) == (hd, AbilitySlot::Activated(hs))
                        })
                };
                let per_source =
                    matches!(m.prod, ManaProd::Linked | ManaProd::Chosen | ManaProd::ChosenN(_) | ManaProd::IfCounter { .. } | ManaProd::SameTypeCounters { .. }) || reuse.is_some();
                if let Some(cl) = out.iter_mut().find(same).filter(|_| !per_source) {
                    cl.members.push(src);
                    continue;
                }
                // ("{X}: add X mana in any combination of colors": the
                // payment's mana may become any colours, `Class::convert`.)
                let (need, need_n) = if ad.cost.mana.is_zero() { (0, 0) } else { need_of(&ad.cost.mana, self.ability_allow(c)) };
                let yld = if ad.x {
                    Yield::Multi { prods: vec![[0; 6]], units: Some(Vec::new()) }
                } else {
                    match self.yield_with(p, &m.prod, mana_life(ad), extra, Some(c)) {
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
                let res = if reuse.is_some() {
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
                    reuse: reuse.unwrap_or(1),
                    convert: ad.x,
                    haste: m.haste,
                    res: if res == 0 && reuse.is_none() { TAP | SAC } else { res },
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
        // (The pools of shared activations, numbered from 1: `Class::pool`.)
        let mut pools: u16 = 0;
        if rain || !sac_by.is_empty() {
            let groups = self.sac_groups(p, &sac_by, &bonus, &creature_bonus, &mut pools);
            for c in self.permanents_of(p) {
                if Some(c) == exclude {
                    continue;
                }
                for cl in self.sac_classes(p, c, locks, rain, &groups) {
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
                let src = Src::of(x, TAP_FOR);
                match out.iter_mut().find(|cl| cl.def == def && cl.members[0].slot == TAP_FOR) {
                    Some(cl) => cl.members.push(src),
                    None => out.push(Class { res: TAP, ..Class::of(def, src, Yield::One(Unit::free(mask))) }),
                }
            }
        }
        // The permanents a Pollinator's ability taps (`POL_FOR`).
        if !pol.is_empty() {
            // (Several-mana ones among the several-mana classes.)
            let _ = self.pol_classes(p, exclude, &pol, &bonus, &creature_bonus, n_multi, &mut out, &mut pools);
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
                    let src = Src::of(c, AbilitySlot::Hand(i as u8));
                    if let Some(cl) = out.iter_mut().find(|cl| cl.def == def && cl.members[0].slot == src.slot) {
                        cl.members.push(src);
                        continue;
                    }
                    let Some(yld) = self.yield_of(p, &m.prod, mana_life(ad), Some(c)) else { continue };
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
            let src = Src::of(c, IMPROVISE);
            match out.iter_mut().find(|cl| cl.def == def) {
                Some(cl) => cl.members.push(src),
                None => out.push(Class { res: TAP, ..Class::of(def, src, Yield::One(Unit::free(0u8))) }),
            }
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
            let src = Src::of(c, IMPROVISE);
            // (It pays the spell's cost only, never a mana ability's:
            // `FOR_COST`.)
            let yld = Yield::One(Unit::free(tag((self.colors_of(c) & COLORS) | RESTRICTED, FOR_COST)));
            match out.iter_mut().find(|cl| cl.def == def && cl.yld == yld) {
                Some(cl) => cl.members.push(src),
                None => out.push(Class { res: TAP, ..Class::of(def, src, yld) }),
            }
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
    fn budget(&self, p: PlayerId, keep: &Keep, later: u64) -> (Budget, bool) {
        let pl = &self.players[p as usize];
        // One reservation: how many candidates it leaves to spare (several:
        // `ResCtx`, the search's).
        let sac = match &keep.reserves[..] {
            [(r, card)] => {
                let (cands, n) = self.reserve_cands(p, r, *card);
                cands.len() as i64 - n as i64
            }
            _ => i32::MAX as i64,
        };
        // (In i64: what is left is within the life total.)
        let life = (pl.life as i64 - later.min(i64::MAX as u64) as i64).max(0) as i32;
        (Budget { life, sac: sac.clamp(0, i32::MAX as i64) as i32 }, later > 0)
    }

    /// The candidates of reservation `r` of the cost of `card` (never `card`
    /// itself) and how many it takes.
    pub(crate) fn reserve_cands(&self, p: PlayerId, r: &Reserve, card: CardIdx) -> (Vec<CardIdx>, u32) {
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
        let hand = |f: Option<&Filter>| -> Vec<CardIdx> {
            self.players[p as usize].hand.iter().copied().filter(|&c| c != card && f.is_none_or(|f| self.card_matches(f, c, &ctx))).collect()
        };
        match r {
            Reserve::Perm(sc, _) => (self.sacrifice_candidates(p, card, sc), sc.n as u32),
            Reserve::Discard(n, f) => (hand(f.as_ref()), *n as u32),
            Reserve::Exile(f) => (hand(Some(f)), 1),
        }
    }

    /// Does source `s` of class `cl` use up a candidate of reservation `r`
    /// of the cost of `card`: it sacrifices that permanent (or taps it, for
    /// a reservation that taps), or exiles that card from the hand?
    fn uses_reserve(&self, p: PlayerId, r: &Reserve, card: CardIdx, cl: &Class, s: &Src) -> bool {
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
        match r {
            Reserve::Perm(sc, taps) if cl.sac_self || *taps && cl.res != 0 => !(sc.another && s.card == card) && self.card_matches(&sc.filter, s.card, &ctx),
            Reserve::Perm(..) => false,
            Reserve::Discard(_, f) => matches!(s.slot, AbilitySlot::Hand(_)) && s.card != card && f.as_ref().is_none_or(|f| self.card_matches(f, s.card, &ctx)),
            Reserve::Exile(f) => matches!(s.slot, AbilitySlot::Hand(_)) && s.card != card && self.card_matches(f, s.card, &ctx),
        }
    }

    /// Is `a`, an actor of a shared activation (`Class::actors`), one of the
    /// permanents a later "tap X untapped <permanents>" needs (a
    /// `Reserve::Perm` that taps)? Activating it taps it (ENGINE.md 99.12:
    /// the Pollinators of which only some count are told apart, `PoolCtx`).
    fn actor_reserved(&self, p: PlayerId, keep: &Keep, a: CardIdx) -> bool {
        keep.reserves.iter().any(|(r, card)| match r {
            Reserve::Perm(sc, true) => {
                let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(*card)) };
                !(sc.another && a == *card) && self.card_matches(&sc.filter, a, &ctx)
            }
            _ => false,
        })
    }

    /// Mana sources whose use takes a candidate of the cost's one
    /// reservation (`Budget::sac`; with several, `consumes` and `ResCtx`).
    fn reserved(&self, p: PlayerId, keep: &Keep, cl: &Class, s: &Src) -> bool {
        match &keep.reserves[..] {
            [(r, card)] => self.uses_reserve(p, r, *card, cl, s),
            _ => false,
        }
    }

    /// With several reservations: does source `s` of class `cl` use up a
    /// candidate of one of them (`ResCtx`)?
    fn consumes(&self, p: PlayerId, keep: &Keep, cl: &Class, s: &Src) -> bool {
        keep.reserves.len() > 1 && keep.reserves.iter().any(|(r, card)| self.uses_reserve(p, r, *card, cl, s))
    }

    /// The candidates left for `keep`'s reservations when there are several
    /// (`ResCtx`; empty otherwise), without those `gone` (used up by picks).
    /// (Several reservations take permanents to move or cards in hand: "tap
    /// X untapped <permanents>" is paid alone, the lowering's
    /// `check_cost_w3` and `merge_costs`.)
    fn res_ctx(&self, p: PlayerId, keep: &Keep, gone: &dyn Fn(CardIdx) -> bool) -> ResCtx {
        if keep.reserves.len() < 2 {
            return ResCtx::default();
        }
        assert!(!keep.reserves.iter().any(|(r, _)| matches!(r, Reserve::Perm(_, true))), "reservations: {:?}", keep.reserves);
        self.res_ctx_of(p, &keep.reserves, gone)
    }

    /// Can `reserves` all be met with distinct candidates, those `gone`
    /// left out (`pay_costs`' picks)?
    pub(crate) fn reserves_left_ok(&self, p: PlayerId, reserves: &[(Reserve, CardIdx)], gone: &dyn Fn(CardIdx) -> bool) -> bool {
        self.res_ctx_of(p, reserves, gone).ok(&[])
    }

    /// `ResCtx` of any number of reservations (a cost's parts: at most the
    /// eight a `ResCtx` mask holds, `Cost`'s fields that take cards).
    fn res_ctx_of(&self, p: PlayerId, reserves: &[(Reserve, CardIdx)], gone: &dyn Fn(CardIdx) -> bool) -> ResCtx {
        let mut ctx = ResCtx::default();
        assert!(reserves.len() <= 8, "reservations: {reserves:?}");
        let sets: Vec<Vec<CardIdx>> = reserves
            .iter()
            .map(|(r, card)| {
                let (cands, n) = self.reserve_cands(p, r, *card);
                ctx.need.push(n.min(i32::MAX as u32) as i32);
                cands
            })
            .collect();
        for set in &sets {
            for &c in set {
                if gone(c) || ctx.cards.iter().any(|x| x.0 == c) {
                    continue;
                }
                let mask = sets.iter().enumerate().filter(|(_, s)| s.contains(&c)).fold(0u8, |m, (k, _)| m | 1 << k);
                let g = match ctx.groups.iter().position(|g| g.0 == mask) {
                    Some(g) => g,
                    None => {
                        ctx.groups.push((mask, 0));
                        ctx.groups.len() - 1
                    }
                };
                ctx.groups[g].1 += 1;
                ctx.cards.push((c, g as u16));
            }
        }
        ctx
    }

    /// Can `keep`'s reservations all be met with the candidates there are
    /// (no mana source used)?
    pub(crate) fn reserves_ok(&self, p: PlayerId, keep: &Keep) -> bool {
        match &keep.reserves[..] {
            [] => true,
            [(r, card)] => {
                let (cands, n) = self.reserve_cands(p, r, *card);
                cands.len() as u64 >= n as u64
            }
            _ => self.res_ctx(p, keep, &|_| false).ok(&[]),
        }
    }

    /// The candidates of a payment of a cost of `total` mana (`ManaCost::total`;
    /// it bounds the uses of a source that only removes counters,
    /// `UseCap`).
    fn cands(&self, p: PlayerId, keep: &Keep, classes: &[Class], use_src: &dyn Fn(&Src) -> bool, dmg: bool, total: usize) -> Cands {
        let cap = UseCap::of(classes, total, 0);
        let taps = self.taps_owed(keep, &|_| true);
        let pools = PoolCtx::of(classes, &|_| false, &|a| self.actor_reserved(p, keep, a), &[]);
        let res = self.res_ctx(p, keep, &|_| false);
        let consumes = |cl: &Class, s: &Src| self.consumes(p, keep, cl, s);
        let chain_of = |s: &Src| chain_id(classes, s, 0);
        cands(classes, use_src, &|_| (0, 0), dmg, &|cl, s| self.reserved(p, keep, cl, s) as i32, &consumes, &chain_of, keep.allow, 0, cap, taps.as_ref(), pools, res)
    }

    /// `keep.taps` for the feasibility (`cands`): the need, and each
    /// candidate `free` keeps (not used by a pick) with what it counts for
    /// (tapping one that is an actor of a shared activation leaves its pool
    /// one fewer: `PoolCtx`).
    #[allow(clippy::type_complexity)]
    fn taps_owed(&self, keep: &Keep, free: &dyn Fn(CardIdx) -> bool) -> Option<(i64, Vec<(CardIdx, i64)>)> {
        let t = keep.taps.as_ref()?;
        let cands = t.cands.iter().filter(|&&(c, n)| n > 0 && free(c)).copied().collect();
        Some((t.need, cands))
    }

    /// The candidates of the rest of payment `pm`: the sources of
    /// `classes[from..]` not picked yet (Pentad Prism: its counters left),
    /// `plus` counted as picked once more.
    fn pay_cands(&self, pm: &PayMana, classes: &[Class], from: usize, plus: Option<Src>, dmg: bool) -> Cands {
        let plus_res = plus.map_or(0, |x| classes.iter().find(|cl| cl.members.contains(&x)).map_or(TAP | SAC, |cl| cl.res));
        let used = |s: &Src| {
            let more = plus.is_some_and(|x| x.card == s.card);
            (pm.used_res(classes, s.card) | if more { plus_res } else { 0 }, pm.uses(s) + (plus == Some(*s)) as crate::num::Count)
        };
        let marked = pm.marked.iter().fold(0, |a, &m| a | purposes(m));
        let cap = UseCap::of(&classes[from..], pm.cost.total() as usize, pm.marked.len());
        // (A candidate of a later tap cost that a pick taps or sacrifices
        // can't pay it.)
        let taps = self.taps_owed(&pm.keep, &|c| used(&Src::of(c, AbilitySlot::Spell)).0 & (TAP | SAC) == 0);
        // (The shared activations' actors: those a pick taps are out, a
        // shared pick whose actor isn't chosen yet, `plus` too, takes one:
        // `PoolCtx`; read from every class, not only those from `from` on.)
        let mut anon = pm.anonymous_acts(classes);
        if let Some(x) = plus {
            if let Some(cl) = classes.iter().find(|cl| cl.members.contains(&x)).filter(|cl| cl.pool > 0) {
                anon.push(cl.pool);
            }
        }
        let tapped = |c: CardIdx| used(&Src::of(c, AbilitySlot::Spell)).0 & TAP != 0;
        let pools = PoolCtx::of(classes, &tapped, &|a| self.actor_reserved(pm.payer, &pm.keep, a), &anon);
        // (The reservations' candidates a pick, `plus` too, uses up.)
        let consumes = |cl: &Class, s: &Src| self.consumes(pm.payer, &pm.keep, cl, s);
        let gone = |c: CardIdx| {
            pm.chosen.iter().chain(pm.multi.iter().map(|m| &m.0)).chain(plus.iter()).any(|s| {
                s.card == c && classes.iter().find(|cl| cl.members.contains(s)).is_some_and(|cl| consumes(cl, s))
            })
        };
        let res = self.res_ctx(pm.payer, &pm.keep, &gone);
        // (Chain ids from every class: the picks' and the candidates' alike.)
        let chain_of = |s: &Src| chain_id(classes, s, 0);
        let mut c = cands(&classes[from..], &|_| true, &used, dmg, &|cl, s| self.reserved(pm.payer, &pm.keep, cl, s) as i32, &consumes, &chain_of, pm.keep.allow, marked, cap, taps.as_ref(), pools, res);
        c.converted = pm.convert;
        c
    }

    /// The pool's mana as candidate units: any of it may pay (CR 106.4;
    /// restricted mana, `RESTRICTED`: never turned into colours by Wizard's
    /// Rockets, with its purposes, when it may pay a need of the payment:
    /// `any`, the cost's purposes and those of the needs mana abilities may
    /// add, ENGINE.md 100.4.5).
    fn add_pool(&self, p: PlayerId, any: u8, c: &mut Cands) {
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
                c.fixed.push(Unit::free(1u8 << t));
            }
            for &(_, only, n) in kinds.iter().filter(|k| k.0 as usize == t) {
                if crate::eoe_w2::spendable(only, any) {
                    for _ in 0..n {
                        c.fixed.push(Unit::free(((1 << t) | RESTRICTED) as Tag | restriction(only)));
                    }
                }
            }
        }
    }

    /// The virtual pool of payment `pm` (ENGINE.md 18): the player's pool
    /// and the mana of the several-mana sources picked, as groups (kind,
    /// count); a kind is a type, + 8 for `MADE`, + 16 for mana with a
    /// spending restriction (`RESTRICTED`: Wizard's Rockets can't turn it
    /// into colours, ENGINE.md 49), its purposes in bits 8-15 (`masks_of`).
    /// Restricted mana is in it when it may pay a need of the payment: the
    /// cost's purposes or an ability's (Sunken Citadel's mana for Cascading
    /// Cataracts' {5}, ENGINE.md 100.4.5); the pool's other restricted mana
    /// stays (ENGINE.md 99.12).
    fn vgroups(&self, pm: &PayMana) -> SVec<(Kind, u32), 12> {
        let pool = &self.players[pm.payer as usize].pool;
        let kinds = self.pool_kinds(pm.payer);
        let any = pm.keep.allow | crate::eoe_w2::SPEND_ABILITIES;
        // (Counted in u32: the pool's and the picks' mana of a type together
        // can pass a u8; ENGINE.md 100.4.6.)
        let of = |v: &[crate::eoe_w2::RKind], t: usize| v.iter().filter(|k| k.0 as usize == t).map(|k| k.2 as u32).sum::<u32>();
        let made_of = |t: usize, only: u8| pm.extra_made_restr.iter().find(|k| k.0 as usize == t && k.1 == only).map_or(0, |k| k.2 as u32);
        let mut g: SVec<(Kind, u32), 12> = SVec::new();
        let push = |g: &mut SVec<(Kind, u32), 12>, kind: Kind, n: u32| {
            if n == 0 {
                return;
            }
            match g.iter_mut().find(|x| x.0 == kind) {
                Some(x) => x.1 += n,
                None => g.push((kind, n)),
            }
        };
        for t in 0..6 {
            let rk = |only: u8| t as Kind | 16 | (purposes(restriction(only)) as Kind) << 8;
            let restr = of(&kinds, t) + of(&pm.extra_restr, t);
            let made_restr = of(&pm.extra_made_restr, t);
            let plain = pool.amt[t] as u32 + pm.extra[t] as u32 - pm.extra_made[t] as u32 - (restr - made_restr);
            push(&mut g, t as Kind, plain);
            for &(_, only, n) in kinds.iter().filter(|k| k.0 as usize == t) {
                if crate::eoe_w2::spendable(only, any) {
                    push(&mut g, rk(only), n as u32);
                }
            }
            for &(_, only, n) in pm.extra_restr.iter().filter(|k| k.0 as usize == t) {
                push(&mut g, rk(only), n as u32 - made_of(t, only));
            }
            // (By activation: its chain id, ENGINE.md 100.4.7. Mana with a
            // restriction is never made by an ability with a mana cost: the
            // lowering refuses both.)
            debug_assert_eq!(made_restr, 0, "restricted mana of an ability with a mana cost");
            for &(id, prod) in &pm.made_by {
                push(&mut g, t as Kind | 8 | (id as Kind) << 16, prod[t] as u32);
            }
            for &(_, only, n) in pm.extra_made_restr.iter().filter(|k| k.0 as usize == t) {
                push(&mut g, rk(only) | 8, n as u32);
            }
        }
        g
    }

    /// The most mana of the virtual pool `groups` a payment of `pm.cost`
    /// and `marked` can spend with every unit of `chosen` and some of
    /// `rest`; `None` if it can't be paid. Wizard's Rockets' colours count
    /// only when nothing else pays (ENGINE.md 18).
    fn most_spent(&self, pm: &PayMana, groups: &[(Kind, u32)], marked: &[Tag], chosen: &[Tag], rest: &Cands, budget: Budget) -> Option<u32> {
        self.most_spent_from(pm, groups, marked, chosen, rest, budget, 0)
    }

    /// [`most_spent`](Self::most_spent) if it is `floor` or more.
    #[allow(clippy::too_many_arguments)]
    fn most_spent_from(
        &self,
        pm: &PayMana,
        groups: &[(Kind, u32)],
        marked: &[Tag],
        chosen: &[Tag],
        rest: &Cands,
        budget: Budget,
        floor: usize,
    ) -> Option<u32> {
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
                    let mut ch: SVec<Tag, 16> = chosen.iter().copied().collect();
                    ch.extend(masks_of(groups, cnt));
                    found = cands_payable_c(&pm.cost, marked, &ch, rest, budget, convert);
                    found
                });
                if found {
                    return Some(k as u32);
                }
            }
            None
        };
        most(false).or_else(|| if rest.convert.is_empty() { None } else { most(true) })
    }

    /// The most of virtual pool `groups` a payment (with `marked` needs) can
    /// spend with single-mana sources paying the rest (`plus` picked once
    /// more); `None` if they can't.
    fn singles_vm(&self, pm: &PayMana, classes: &[Class], groups: &[(Kind, u32)], marked: &[Tag], plus: Option<Src>, budget: Budget, dmg: bool) -> Option<u32> {
        let first = classes.iter().position(|cl| matches!(cl.yld, Yield::One(_))).unwrap_or(classes.len());
        let rest = self.pay_cands(pm, classes, first.max(pm.min_class as usize), plus, dmg);
        self.most_spent(pm, groups, marked, &[], &rest, budget)
    }

    /// The most of the virtual pool payment `pm` can spend with its picks
    /// so far and the single-mana sources from `pm.min_class` on paying the
    /// rest (`vm` from the state alone: equal to the `vm` the picks were
    /// made with; a decision refreshed in another world, ENGINE.md 10, or a
    /// pool changed by a test, reads it again).
    fn vm_now(&self, pm: &PayMana, classes: &[Class]) -> Option<u32> {
        if pm.marked.is_empty() && !pm.convert && pm.extra == [0; 6] && self.players[pm.payer as usize].pool.total() == 0 {
            return Some(0);
        }
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.later_life());
        let budget = self.after_picks(pm, classes, dmg, budget, None)?;
        let first = classes.iter().position(|cl| matches!(cl.yld, Yield::One(_))).unwrap_or(classes.len());
        let rest = self.pay_cands(pm, classes, first.max(pm.min_class as usize), None, dmg);
        let chosen: SVec<Tag, 16> = pm.chosen.iter().map(|s| chosen_mask(classes, s)).collect();
        self.most_spent(pm, &self.vgroups(pm), &pm.marked, &chosen, &rest, budget)
    }

    /// `pm.vm` again (after a several-mana pick).
    fn recount(&self, pm: &mut PayMana, classes: &[Class]) {
        // (An empty virtual pool spends nothing: whether single-mana
        // sources can pay is asked of each option, ENGINE.md 17.)
        if pm.marked.is_empty() && !pm.convert && pm.extra == [0; 6] && self.players[pm.payer as usize].pool.total() == 0 {
            pm.vm = Some(0);
            return;
        }
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.later_life());
        let budget = self.after_picks(pm, classes, dmg, budget, None).expect("picks within the budget");
        pm.vm = self.singles_vm(pm, classes, &self.vgroups(pm), &pm.marked, None, budget, dmg);
    }

    /// Can `p` pay `cost` (Phyrexian pips included) from their pool and the
    /// sources in `classes`, leaving what `keep` needs?
    pub(crate) fn can_pay_with(&self, p: PlayerId, cost: &ManaCost, classes: &[Class], keep: &Keep) -> bool {
        let classes = self.classes_for_self_use(classes, keep.self_use);
        self.might_pay(p, cost, &classes, None) && self.phyrexian_feasible(p, cost, &classes, keep, keep.life as u64, None)
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
        let mut have = self.players[p as usize].pool.total() as u64;
        for cl in classes {
            if have >= need as u64 {
                return true;
            }
            let most = cl.delve as u32
                + match &cl.yld {
                    Yield::One(_) => 1,
                    Yield::Multi { prods, units } => {
                        let most = prods.iter().map(|v| v.iter().map(|&n| n as u32).sum::<u32>()).max().unwrap_or(0);
                        most.max(units.as_ref().map_or(0, |us| us.len() as u32))
                    }
                };
            // (In u64: exact for any number of uses, `Class::reuse`.)
            let uses = cl.reuse as u64 * cl.members.iter().filter(|s| Some(s.card) != without).count() as u64;
            have = have.saturating_add(most as u64 * uses);
        }
        have >= need as u64
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
        let classes = self.classes_excluding(p, classes, without);
        let classes = self.classes_for_self_use(&classes, keep.self_use);
        self.might_pay(p, cost, &classes, without) && self.phyrexian_feasible(p, cost, &classes, keep, keep.life as u64, without)
    }

    /// `classes` for a cost that takes `su` of its source (`SelfUse`, CR
    /// 601.2g-h; docs/shortcuts.md 13.3): without the source's sources that
    /// conflict with it. A source that only removes counters keeps the uses
    /// the cost's counters leave (and, when the cost needs the source, not
    /// the one whose last counter sacrifices it); a Pollinator or a
    /// Lumberjack the cost taps no longer activates (`Class::actors`).
    pub(crate) fn classes_for_self_use<'a>(&self, classes: &'a [Class], su: Option<SelfUse>) -> std::borrow::Cow<'a, [Class]> {
        let Some(su) = su else { return std::borrow::Cow::Borrowed(classes) };
        let actor = su.tap && classes.iter().any(|cl| cl.pool > 0 && cl.actors.contains(&su.card));
        if !actor && !classes.iter().any(|cl| cl.members.iter().any(|s| s.card == su.card)) {
            return std::borrow::Cow::Borrowed(classes);
        }
        let conflict = if su.tap { TAP | SAC } else if su.moves { SAC } else { 0 };
        let mut out = Vec::with_capacity(classes.len());
        for cl in classes {
            let mut cl = cl.clone();
            let mut reuse = cl.reuse;
            cl.members.retain(|s| {
                if s.card != su.card {
                    return true;
                }
                if cl.res & conflict != 0 {
                    return false;
                }
                // (Counters: only an ability of the source removes them.)
                let (AbilitySlot::Activated(_) | AbilitySlot::Hand(_)) = s.slot else { return true };
                let ad = self.src_ability(s.card, s.slot);
                let Some((k, n)) = ad.cost.remove_counters else { return true };
                let have = self.counters_on(s.card, k);
                let keep_n = match su.counters {
                    Some((k2, m)) if k2 == k => m as crate::num::Count,
                    // (Its last counter's use sacrifices it, before the
                    // cost is paid.)
                    _ if (su.tap || su.moves || su.counters.is_some()) && ad.mana().is_some_and(|m| m.sac_if_no == Some(k)) => 1,
                    _ => 0,
                };
                let uses = have.saturating_sub(keep_n) / n.max(1) as crate::num::Count;
                if cl.res == 0 {
                    reuse = reuse.min(uses);
                }
                uses > 0
            });
            cl.reuse = reuse;
            if cl.pool > 0 && su.tap {
                cl.actors.retain(|&a| a != su.card);
            }
            if !cl.members.is_empty() && !(cl.pool > 0 && cl.actors.is_empty()) && cl.reuse > 0 {
                out.push(cl);
            }
        }
        std::borrow::Cow::Owned(out)
    }

    /// `classes` (built with nothing excluded) as a payment that excludes
    /// card `without` (`Keep::exclude`: `mana_classes(p, without)`) has
    /// them, but for `without`'s own members, which the feasibility leaves
    /// out itself. The sources another permanent's mana ability makes
    /// depend on that permanent: a Gene Pollinator excluded can't activate
    /// for the permanents it would tap (`POL_FOR`), nor an excluded Urza
    /// for its artifacts (`TAP_FOR`), an excluded Squandered Resources or
    /// Orcish Lumberjack for the permanents it sacrifices; those classes are
    /// built again with `without` excluded, so castability and the payment
    /// agree (ENGINE.md 49; EoE fuzz seed 11752 of W6's table: a Pollinator,
    /// a Food by Ragost, activating "{2}, {T}, Sacrifice this artifact"
    /// counted itself among the Pollinators that could activate).
    pub(crate) fn classes_excluding<'a>(&self, p: PlayerId, classes: &'a [Class], without: Option<CardIdx>) -> std::borrow::Cow<'a, [Class]> {
        let dependent = |cl: &Class| {
            let s = cl.members[0].slot;
            s == POL_FOR || s == TAP_FOR || is_sac_slot(s)
        };
        let Some(c) = without else { return std::borrow::Cow::Borrowed(classes) };
        if self.objs[c.i()].zone != Zone::Battlefield || !classes.iter().any(dependent) {
            return std::borrow::Cow::Borrowed(classes);
        }
        let mut out: Vec<Class> = classes.iter().filter(|cl| !dependent(cl)).cloned().collect();
        out.extend(self.mana_classes(p, without).into_iter().filter(dependent));
        std::borrow::Cow::Owned(out)
    }

    /// Some split of the Phyrexian pips into life and mana leaves the rest
    /// payable from the pool and `classes` (but card `without`'s sources).
    fn phyrexian_feasible(
        &self,
        p: PlayerId,
        cost: &ManaCost,
        classes: &[Class],
        keep: &Keep,
        later: u64,
        without: Option<CardIdx>,
    ) -> bool {
        let life_total = self.players[p as usize].life;
        match (0..5).find(|&c| cost.phyrexian[c] > 0) {
            None => {
                if cost.is_zero() {
                    return true;
                }
                let (budget, dmg) = self.budget(p, keep, later);
                let mut c = self.cands(p, keep, classes, &|s| Some(s.card) != without, dmg, cost.total() as usize);
                self.add_pool(p, keep.allow | needs_allow(classes), &mut c);
                cands_payable(cost, &[], &[], &c, budget)
            }
            Some(col) => (0..=cost.phyrexian[col]).any(|k| {
                let later = later + 2 * k as u64;
                later <= life_total.max(0) as u64 && self.phyrexian_feasible(p, &split_phyrexian(cost, col, k), classes, keep, later, without)
            }),
        }
    }

    /// The candidates of an unrestricted payment by `p` (computed once per
    /// priority decision).
    pub(crate) fn plain_cands(&self, p: PlayerId, classes: &[Class]) -> Cands {
        let mut c = self.cands(p, &Keep::default(), classes, &|_| true, false, PLAIN_TOTAL);
        self.add_pool(p, needs_allow(classes), &mut c);
        c
    }

    /// [`can_pay_with`](Self::can_pay_with) for an unrestricted payment,
    /// with candidates from [`plain_cands`](Self::plain_cands).
    pub(crate) fn can_pay_plain(&self, p: PlayerId, cost: &ManaCost, classes: &[Class], plain: &Cands) -> bool {
        if cost.has_phyrexian() {
            return self.can_pay_with(p, cost, classes, &Keep::default());
        }
        // (Candidates whose counter sources were cut for smaller costs are
        // built again for this one: `UseCap`.)
        if plain.cut && cost.total() as usize > PLAIN_TOTAL {
            return self.can_pay_with(p, cost, classes, &Keep::default());
        }
        let life = self.players[p as usize].life.max(0);
        cost.is_zero() || cands_payable(cost, &[], &[], plain, Budget { life, sac: i32::MAX })
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
        let ad = self.src_ability(c, src.slot);
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
            self.lose_life(p, i32::try_from(cost.life).expect("a mana ability's life: at most 99, lowering"));
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
        let (budget, dmg) = self.budget(p, &pm.keep, pm.later_life());
        let Some(budget) = self.after_picks(pm, classes, dmg, budget, Some((ci, src))) else { return Vec::new() };
        let first = classes.iter().position(|cl| matches!(cl.yld, Yield::One(_))).unwrap_or(classes.len());
        let singles = self.pay_cands(pm, classes, first.max(ci), Some(src), dmg);
        let base = self.vgroups(pm);
        let mut marked = pm.marked.clone();
        // (Its activation's chain id: its cost a `NOT_MADE` need, its mana
        // `MADE`.)
        let id = chain_id(classes, &src, pm.uses(&src));
        if cl.need != 0 {
            marked.extend(std::iter::repeat_n(crate::mana::with_chain(cl.need, id), cl.need_n as usize));
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
                        let kind = t as Kind
                            | if cl.need != 0 { 8 | (id as Kind) << 16 } else { 0 }
                            | if cl.only != 0 { 16 | (purposes(restriction(cl.only)) as Kind) << 8 } else { 0 };
                        match groups.iter_mut().find(|g| g.0 == kind) {
                            Some(g) => g.1 += prod[t] as u32,
                            None => groups.push((kind, prod[t] as u32)),
                        }
                    }
                }
                // Useful: the pool pays more, or more several-mana sources
                // are needed still (and then feasible with them). When the
                // pool covers the rest already, any production that leaves
                // the payment payable (13.13: the source paying instead of
                // the pool's mana, ENGINE.md 100.4.3).
                let floor = pm.vm.map_or(0, |a| a as usize + 1);
                if pm.pool_covers() {
                    // (Its mana pays part of the cost: some completing spend
                    // takes more of a kind than the virtual pool had without
                    // it. A production none of whose mana is spent only
                    // floats, as with priority before the payment.)
                    let rest = self.pay_cands(pm, classes, ci, Some(src), dmg);
                    let had = |kind: Kind| base.iter().find(|g| g.0 == kind).map_or(0, |g| g.1);
                    let total: usize = groups.iter().map(|g| g.1 as usize).sum();
                    let top = total.min(pm.cost.total() as usize + marked.len());
                    (1..=top).rev().any(|k| {
                        each_subset(&groups, k, &mut SVec::new(), &mut |cnt| {
                            groups.iter().zip(cnt.iter()).any(|(g, &n)| n > had(g.0)) && {
                                let ch: SVec<Tag, 16> = masks_of(&groups, cnt);
                                cands_payable_c(&pm.cost, &marked, &ch, &rest, budget, true)
                            }
                        })
                    })
                } else {
                match (pm.vm, self.most_spent_from(pm, &groups, &marked, &[], &singles, budget, floor)) {
                    (_, Some(_)) => true,
                    // (Its mana may still be spent on the mana cost of a
                    // several-mana source after it: Sunken Citadel's for
                    // Cascading Cataracts' {5}, ENGINE.md 100.4.5.)
                    (Some(_), None) if self.most_spent_from(pm, &groups, &marked, &[], &singles, budget, 0).is_some() => {
                        let rest = self.pay_cands(pm, classes, ci, Some(src), dmg);
                        self.most_spent_from(pm, &groups, &marked, &[], &rest, budget, floor).is_some()
                    }
                    (_, None) => {
                        let rest = self.pay_cands(pm, classes, ci, Some(src), dmg);
                        self.most_spent(pm, &groups, &marked, &[], &rest, budget).is_some()
                    }
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
    /// payment payable? (Offered like any source, 13.14: keeping a land of
    /// another colour untapped is a choice; ENGINE.md 100.4.3. Before, only
    /// when nothing else paid.)
    fn convert_ok(&self, pm: &PayMana, classes: &[Class], ci: usize, src: Src) -> bool {
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.later_life());
        let Some(left) = self.after_picks(pm, classes, dmg, budget, Some((ci, src))) else { return false };
        let groups = self.vgroups(pm);
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
            let sac = self.reserved(pm.payer, &pm.keep, cl, &s) as i32;
            b = Budget { life: b.life - life, sac: b.sac - sac };
            if b.life < 0 || b.sac < 0 {
                return None;
            }
        }
        // (The actors chosen for shared picks are tapped: one a later "tap X
        // untapped <permanents>" needs is one fewer left for it. Whether the
        // pools have actors enough for the picks is the candidates' part,
        // `PoolCtx`.)
        for (s, a) in &pm.actors {
            if s.slot != SQ_FOR && self.actor_reserved(pm.payer, &pm.keep, *a) {
                b.sac -= 1;
            }
        }
        (b.sac >= 0).then_some(b)
    }

    /// The `PayMana` task (ENGINE.md 6, 18).
    pub(crate) fn pay_mana(&mut self, mut pm: Box<PayMana>) {
        let payer = pm.payer;
        // CR 107.4f: per colour, how many Phyrexian pips are paid with 2 life
        // (paid at the end, after the mana abilities).
        if let Some(col) = (0..5).find(|&c| pm.cost.phyrexian[c] > 0) {
            match self.answer.take() {
                Some(Choice::Number(k)) => {
                    pm.phyrexian_life += 2 * k as u32;
                    pm.cost = split_phyrexian(&pm.cost, col, k);
                    self.agenda.push(Task::PayMana(pm));
                }
                Some(other) => unreachable!("Phyrexian answer {other:?}"),
                None => {
                    let classes = self.payment_classes(&pm);
                    let life = self.players[payer as usize].life;
                    let opts: Vec<Opt> = (0..=pm.cost.phyrexian[col])
                        .filter(|&k| {
                            let later = pm.later_life() + 2 * k as u64;
                            later <= life.max(0) as u64
                                && self.phyrexian_feasible(payer, &split_phyrexian(&pm.cost, col, k), &classes, &pm.keep, later, None)
                        })
                        .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k) })
                        .collect();
                    self.ask(Task::PayMana(pm), payer, DecisionKind::PayPhyrexian, opts);
                }
            }
            return;
        }
        // Which permanent activates the shared source just picked (CR 602.2b:
        // which Gene Pollinator or Orcish Lumberjack taps, which Squandered
        // Resources sacrifices; docs/shortcuts.md N23): its class's actors not
        // tapped by a pick whose choice leaves the payment payable, by their
        // own ids (`ChoosePermanent`), asked when there are several.
        if let Some(src) = pm.actor_for {
            let classes = self.payment_classes(&pm);
            match self.answer.take() {
                Some(Choice::Card(a)) => {
                    pm.actor_for = None;
                    pm.actors.push((src, a));
                    self.agenda.push(Task::PayMana(pm));
                }
                Some(other) => unreachable!("actor answer {other:?}"),
                None => {
                    let ok = self.actor_options(&pm, &classes, src);
                    assert!(!ok.is_empty(), "PayMana: no actor for {src:?}: {pm:?} classes {classes:?}\n{}", self.log_lines().join("\n"));
                    if ok.len() == 1 {
                        pm.actor_for = None;
                        pm.actors.push((src, ok[0]));
                        self.agenda.push(Task::PayMana(pm));
                        return;
                    }
                    let opts = ok.iter().map(|&a| Opt { action: self.perm_action(payer, a), choice: Choice::Card(a) }).collect();
                    self.ask(Task::PayMana(pm), payer, DecisionKind::ChoosePermanent, opts);
                }
            }
            return;
        }
        // CR 601.2h: which of the pool's mana pays, the sources picked.
        if let Some(rems) = pm.spend.take() {
            match self.answer.take() {
                Some(Choice::Item(k)) => self.finish_payment(&pm, rems[k as usize].clone()),
                Some(Choice::Page(p)) => {
                    pm.pool_page = p;
                    pm.spend = Some(rems);
                    self.agenda.push(Task::PayMana(pm));
                }
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
            // (The classes only for the several-mana picks' restrictions.)
            let classes = if pm.multi.is_empty() { Vec::new() } else { self.payment_classes(&pm) };
            let rem = self.pool_rems(&pm, &classes).swap_remove(0);
            self.finish_payment(&pm, rem);
            return;
        }
        let classes = self.payment_classes(&pm);
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
                if delve {
                    pm.delve_late.push(src.card);
                }
                if classes[ci].convert {
                    // (Wizard's Rockets: the payment's mana of any colour.)
                    pm.convert = true;
                    pm.multi.push((src, [0; 6]));
                    self.recount(&mut pm, &classes);
                    self.agenda.push(Task::PayMana(pm));
                    return;
                }
                let (budget, dmg) = self.budget(payer, &pm.keep, pm.later_life());
                let rest = self.pay_cands(&pm, &classes, 0, Some(src), dmg);
                // (One mana, its type; several (Black Lotus's three of one
                // colour), a production: each that leaves the rest payable.)
                let cl = &classes[ci];
                let all: Vec<[u8; 6]> = match &cl.yld {
                    Yield::One(u) => (0..6).filter(|&t| u.mask & (1 << t) != 0).map(|t| one(t, 1)).collect(),
                    Yield::Multi { prods, .. } => prods.clone(),
                };
                let kind_of = |t: usize| t as Kind | if cl.only != 0 { 16 | (purposes(restriction(cl.only)) as Kind) << 8 } else { 0 };
                let prods: Vec<[u8; 6]> = all
                    .into_iter()
                    .filter(|prod| {
                        let mut groups = self.vgroups(&pm);
                        for t in (0..6).filter(|&t| prod[t] > 0) {
                            match groups.iter_mut().find(|g| g.0 == kind_of(t)) {
                                Some(g) => g.1 += prod[t] as u32,
                                None => groups.push((kind_of(t), prod[t] as u32)),
                            }
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
                    if !pm.forced.is_empty() || pm.actor_for.is_some() {
                        // (The next of delve's sources first; a shared
                        // source's actor.)
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
        // (Read from the state when a decision is asked: ENGINE.md 10.)
        if self.answer.is_none() && pm.which.is_none() {
            pm.vm = self.vm_now(&pm, &classes);
        }
        // The pool covers the rest: `PASS` spends it (unless Wizard's
        // Rockets must be picked for the colours: no pool left), and the
        // sources that may pay instead are offered (13.13; ENGINE.md
        // 100.4.3).
        let covers = pm.pool_covers() && !self.pool_rems(&pm, &classes).is_empty();
        // (Which source a permanent picked by its own id stands for, asked
        // when it stands for several: ENGINE.md 99.12.)
        // (Paged past the `NUMBER` ids, A0's helper: any number of sources.)
        let ask_which = |g: &mut Game, pm: Box<PayMana>, card: CardIdx| {
            let n = pm.card_slots.iter().find(|(c, _)| *c == card).map_or(0, |(_, v)| v.len());
            if n <= act::NUMBER_N as usize {
                let opts = (0..n).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) }).collect();
                g.ask(Task::PayMana(pm), payer, DecisionKind::ChooseMode, opts);
                return;
            }
            let items: Vec<(u32, Choice)> = (0..n as u32).map(|k| (k, Choice::Item(k))).collect();
            let page = pm.pool_page;
            g.ask_paged(Task::PayMana(pm), page, payer, DecisionKind::ChooseMode, act::NUMBER, act::NUMBER_N, &items, Vec::new());
        };
        if let (Some(card), None) = (pm.which, &self.answer) {
            return ask_which(self, pm, card);
        }
        if let Some(ch) = self.answer.take() {
            let slot_of = |pm: &PayMana, card: CardIdx, k: usize| pm.card_slots.iter().find(|(c, _)| *c == card).map(|(_, v)| v[k]).expect("a source the permanent stands for");
            // (A source offered by its `ACTIVATE` id is the only one of its
            // slot on the permanent: a shared source of one group.)
            let by_slot = |card: CardIdx, slot: AbilitySlot| {
                classes.iter().flat_map(|cl| cl.members.iter()).find(|s| s.card == card && s.slot == slot).map_or(0, |s| s.g)
            };
            let (card, (slot, g)) = match ch {
                Choice::Ability(card, slot) => (card, (slot, by_slot(card, slot))),
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
                Choice::Item(k) if pm.which.is_some() => {
                    let card = pm.which.take().expect("the permanent picked");
                    pm.pool_page = crate::decision::Page::FIRST;
                    (card, slot_of(&pm, card, k as usize))
                }
                Choice::Page(page) if pm.which.is_some() => {
                    let card = pm.which.expect("the permanent picked");
                    pm.pool_page = page;
                    return ask_which(self, pm, card);
                }
                // The pool pays the rest (13.13).
                Choice::Pass => {
                    pm.card_slots.clear();
                    let rems = self.pool_rems(&pm, &classes);
                    self.end_payment(pm, rems);
                    return;
                }
                other => unreachable!("pay answer {other:?}"),
            };
            pm.card_slots.clear();
            let src = Src { card, slot, g };
            let ci = classes.iter().position(|cl| cl.members.contains(&src)).expect("source class");
            pm.min_class = ci as u8;
            // (A shared source's actor: asked next, `actor_for`.)
            if !classes[ci].actors.is_empty() && matches!(classes[ci].yld, Yield::One(_)) {
                pm.actor_for = Some(src);
            }
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
                    Yield::One(_) => {
                        // (Paying instead of the pool's mana: the pool's
                        // spend again, now with this pick, 13.13.)
                        let covered = pm.pool_covers();
                        pm.chosen.push(src);
                        if covered {
                            let (budget, dmg) = self.budget(payer, &pm.keep, pm.later_life());
                            let budget = self.after_picks(&pm, &classes, dmg, budget, None).expect("picks within the budget");
                            let chosen: SVec<Tag, 16> = pm.chosen.iter().map(|s| chosen_mask(&classes, s)).collect();
                            let rest = self.pay_cands(&pm, &classes, pm.min_class as usize, None, dmg);
                            pm.vm = self.most_spent(&pm, &self.vgroups(&pm), &pm.marked, &chosen, &rest, budget);
                        }
                    }
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
        let (budget, dmg) = self.budget(payer, &pm.keep, pm.later_life());
        let unit_of = |s: &Src| -> Unit {
            match classes.iter().find(|cl| cl.members.contains(s)).map(|cl| &cl.yld) {
                Some(Yield::One(u)) => *u,
                _ => unreachable!("chosen sources make one mana"),
            }
        };
        let chosen_masks: SVec<Tag, 16> = pm.chosen.iter().map(|s| unit_of(s).mask).collect();
        // The spends of `vm` mana of the virtual pool that can still complete
        // the payment (one, empty, when the pool is empty).
        let groups = self.vgroups(&pm);
        let spends: Vec<SVec<Tag, 16>> = if groups.is_empty() {
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
        let mut card_slots: Vec<(CardIdx, Vec<(AbilitySlot, u16)>)> = Vec::new();
        // (The purposes a need of the payment may have: the cost's, its
        // picked abilities', those the sources still to pick may add;
        // ENGINE.md 100.4.5.)
        let any = pm.keep.allow | pm.marked.iter().fold(0, |a, &m| a | purposes(m)) | needs_allow(&classes[(pm.min_class as usize).min(classes.len())..]);
        for (ci, cl) in classes.iter().enumerate().skip(pm.min_class as usize) {
            if !crate::eoe_w2::spendable(cl.only, any) {
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
                                if covers {
                                    // (Paying instead of some of the pool's
                                    // mana: any spend of it, 13.13.)
                                    let mut ch = chosen_masks.clone();
                                    ch.push(u.mask);
                                    self.most_spent(&pm, &groups, &pm.marked, &ch, &rest, left).is_some()
                                } else if groups.is_empty() {
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
            // (And the pools it is an actor of: tapping it leaves them one
            // fewer, ENGINE.md 100.4.7.)
            let kind_of = |s: &Src| -> (bool, Vec<usize>, Vec<u16>) {
                let others = classes.iter().enumerate().filter(|&(j, c)| j != ci && c.members.iter().any(|m| m.card == s.card)).map(|(j, _)| j);
                let pools = classes.iter().filter(|c| c.pool > 0 && c.actors.contains(&s.card)).map(|c| c.pool);
                (self.reserved(payer, &pm.keep, cl, s), others.collect(), pools.collect())
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
                // for in `card_slots`. A sacrifice past the permanent's
                // fourth `ACTIVATE` id too: docs/shortcuts.md 31.7.2k.)
                // (Or a shared source of a permanent with another of its
                // slot, of another group: ENGINE.md 100.4.7.)
                let twin = || classes.iter().filter(|c| c.members.iter().any(|m| m.card == rep.card && m.slot == rep.slot)).count() > 1;
                let perm_id = by_perm(rep.slot) || (is_sac_slot(rep.slot) && (cl.slot_id >= 4 || twin()));
                if perm_id {
                    match card_slots.iter_mut().find(|(c, _)| *c == rep.card) {
                        Some((_, v)) => {
                            v.push((rep.slot, rep.g));
                            continue;
                        }
                        None => card_slots.push((rep.card, vec![(rep.slot, rep.g)])),
                    }
                }
                opts.push(if perm_id {
                    // Improvise or convoke, a permanent tapped for another's
                    // mana ability, a source past the permanent's four
                    // `ACTIVATE` ids: the permanent's own slot (`MY_PERM`).
                    Opt { action: self.perm_action(payer, rep.card), choice: Choice::Card(rep.card) }
                } else if is_sac_slot(rep.slot) {
                    // A granted ability or another's sacrificing it: after
                    // its own and those granted to it (`ACTIVATE` +
                    // `slot_id`, `sac_classes`).
                    let k = cl.slot_id as u16;
                    Opt { action: act::ACTIVATE + (self.perm_action(payer, rep.card) - act::MY_PERM) * 4 + k, choice: Choice::Ability(rep.card, rep.slot) }
                } else {
                    Opt { action: self.ability_action(payer, rep.card, rep.slot), choice: Choice::Ability(rep.card, rep.slot) }
                });
            }
        }
        pm.card_slots = card_slots;
        if covers {
            if opts.is_empty() {
                let rems = self.pool_rems(&pm, &classes);
                self.end_payment(pm, rems);
                return;
            }
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
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

    /// The actors of shared source `src`, just picked, that may activate it
    /// (`PayMana::actor_for`): its class's actors that no pick taps (a
    /// Squandered Resources is tapped by nothing) and whose choice leaves the
    /// payment payable (with the picks so far and the sources still to pick).
    fn actor_options(&self, pm: &PayMana, classes: &[Class], src: Src) -> Vec<CardIdx> {
        let cl = classes.iter().find(|cl| cl.members.contains(&src)).expect("the shared source's class");
        let tapped = |a: CardIdx| src.slot != SQ_FOR && pm.used_res(classes, a) & TAP != 0;
        cl.actors
            .iter()
            .copied()
            .filter(|&a| !tapped(a))
            .filter(|&a| {
                let mut q = pm.clone();
                q.actor_for = None;
                q.actors.push((src, a));
                self.payable_after_picks(&q, classes)
            })
            .collect()
    }

    /// Can payment `pm` still be completed with its picks (and actors) and
    /// the sources of the classes from `pm.min_class` on?
    fn payable_after_picks(&self, pm: &PayMana, classes: &[Class]) -> bool {
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.later_life());
        let Some(b) = self.after_picks(pm, classes, dmg, budget, None) else { return false };
        let rest = self.pay_cands(pm, classes, pm.min_class as usize, None, dmg);
        let chosen: SVec<Tag, 16> = pm.chosen.iter().map(|s| chosen_mask(classes, s)).collect();
        self.most_spent(pm, &self.vgroups(pm), &pm.marked, &chosen, &rest, b).is_some()
    }

    /// The sources are picked (CR 601.2g): which mana of the virtual pool
    /// pays (601.2h), asked when the pools it can leave differ (`PayPool`).
    fn end_payment(&mut self, mut pm: Box<PayMana>, rems: Vec<Rem>) {
        if rems.len() == 1 {
            self.finish_payment(&pm, rems[0].clone());
            return;
        }
        // (Any number of pools: paged past the `NUMBER` ids, A0's helper;
        // docs/shortcuts.md N11.)
        let items: Vec<(u32, Choice)> = (0..rems.len() as u32).map(|k| (k, Choice::Item(k))).collect();
        let (payer, page) = (pm.payer, pm.pool_page);
        pm.spend = Some(rems);
        self.ask_paged(Task::PayMana(pm), page, payer, DecisionKind::PayPool, act::NUMBER, act::NUMBER_N, &items, Vec::new());
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
        // The virtual pool's mana by what tells two units apart: its type,
        // its spending restriction and rider (`only`, 0 for none), whether it
        // gives haste (Arena of Glory), and whether a mana ability with a
        // mana cost made it in this payment (`MADE`: it can't pay such a
        // cost). Units alike in all four are the same choice (CR 106.4: the
        // pool holds amounts of mana, and two such units leave the same
        // pool). The pool's mana first (made by nothing in this payment).
        let mut parts: Vec<((u8, u8, bool, u64), u8)> = Vec::new();
        let mut add = |key: (u8, u8, bool, u64), n: u8| {
            if n == 0 {
                return;
            }
            match parts.iter_mut().find(|x| x.0 == key) {
                // (Saturating: at most 255 of a type, shortcuts.md 46.3.)
                Some(x) => x.1 = x.1.saturating_add(n),
                None => parts.push((key, n)),
            }
        };
        let kinds = self.pool_kinds(pm.payer);
        for t in 0..6u8 {
            let restr: u8 = kinds.iter().filter(|k| k.0 == t).map(|k| k.2).sum();
            let haste = if t as usize == R { pool.haste } else { 0 };
            add((t, 0, false, 0), pool.amt[t as usize] - restr - haste);
            add((t, 0, true, 0), haste);
        }
        for &(t, only, n) in kinds.iter() {
            add((t, only, false, 0), n);
        }
        // The several-mana sources picked: each production with its class's
        // restriction, haste and need (its activation's chain id).
        let any = pm.keep.allow | pm.marked.iter().fold(0, |a, &m| a | purposes(m));
        for (i, (src, prod)) in pm.multi.iter().enumerate() {
            let cl = classes.iter().find(|cl| cl.members.contains(src)).expect("a picked source's class");
            let id = if cl.need != 0 { chain_id(classes, src, pm.multi[..i].iter().filter(|m| m.0 == *src).count() as crate::num::Count) } else { 0 };
            for t in 0..6u8 {
                add((t, cl.only, cl.haste && t as usize == R, id), prod[t as usize]);
            }
        }
        let rem_of = |spent: &[u8]| {
            let mut r = Rem::default();
            for (i, &((t, only, haste, _), n)) in parts.iter().enumerate() {
                let left = n - spent.get(i).copied().unwrap_or(0);
                r.amt[t as usize] = r.amt[t as usize].saturating_add(left);
                if haste {
                    r.haste = r.haste.saturating_add(left);
                }
                if only != 0 {
                    crate::eoe_w2::add_kind(&mut r.restr, t, only, left);
                }
            }
            r
        };
        let vm = pm.vm.expect("the pool's spend is known") as usize;
        if vm == 0 {
            return vec![rem_of(&[])];
        }
        // The units this payment may spend (CR 106.6: restricted mana only
        // when one of its purposes is this payment's), as groups.
        // (Restricted mana: when one of its purposes is the cost's or a
        // picked ability's, ENGINE.md 100.4.5.)
        let spendable: Vec<usize> = (0..parts.len()).filter(|&i| crate::eoe_w2::spendable(parts[i].0 .1, any)).collect();
        let groups: SVec<(u8, u32), 12> = spendable.iter().map(|&i| (i as u8, parts[i].1 as u32)).collect();
        let mask_of = |i: usize| {
            let (t, only, _, made) = parts[i].0;
            let m = crate::mana::with_chain(((1u8 << t) | if made != 0 { MADE } else { 0 } | if only != 0 { RESTRICTED } else { 0 }) as Tag | restriction(only), made);
            if pm.convert {
                convert(m)
            } else {
                m
            }
        };
        let chosen: SVec<Tag, 16> = pm.chosen.iter().map(|s| chosen_mask(classes, s)).map(|m| if pm.convert { convert(m) } else { m }).collect();
        // Every spend of `vm` units that completes the payment, and the pool
        // each leaves: every distinct one is the player's choice (601.2h).
        let mut out: Vec<Rem> = Vec::new();
        each_subset(&groups, vm, &mut SVec::new(), &mut |cnt| {
            let mut ch = chosen.clone();
            let mut spent = vec![0u8; parts.len()];
            for (g, &n) in groups.iter().zip(cnt.iter()) {
                spent[g.0 as usize] = u8::try_from(n).expect("at most its part's count, a u8");
                for _ in 0..n {
                    ch.push(mask_of(g.0 as usize));
                }
            }
            if payable_n(&pm.cost, pm.keep.allow, &pm.marked, &ch, &[]) {
                let r = rem_of(&spent);
                if !out.contains(&r) {
                    out.push(r);
                }
            }
            false
        });
        // (The first keeps the most types, then the most of the earlier
        // ones: the heuristic bots' answer.)
        out.sort_by(|x, y| {
            let key = |r: &Rem| (std::cmp::Reverse(r.amt.iter().filter(|&&n| n > 0).count()), std::cmp::Reverse(r.amt), std::cmp::Reverse(r.haste));
            key(x).cmp(&key(y)).then_with(|| y.restr.cmp(&x.restr))
        });
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
            // (Its activation's chain id: its cost and its mana, 13.15.)
            let id = chain_id(classes, &src, pm.uses(&src));
            pm.marked.extend(std::iter::repeat_n(crate::mana::with_chain(cl.need, id), cl.need_n as usize));
            pm.made_by.push((id, prod));
        }
        if cl.haste {
            pm.extra_haste = pm.extra_haste.saturating_add(prod[crate::mana::R]);
        }
        // (A shared source's actor: asked next.)
        if !cl.actors.is_empty() {
            pm.actor_for = Some(src);
        }
        if cl.only != 0 {
            for t in 0..6 {
                crate::eoe_w2::add_kind(&mut pm.extra_restr, t as u8, cl.only, prod[t]);
                if cl.need != 0 {
                    crate::eoe_w2::add_kind(&mut pm.extra_made_restr, t as u8, cl.only, prod[t]);
                }
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
        let mut spent = [0u8; 6];
        for t in 0..6 {
            spent[t] = pool.amt[t].saturating_add(pm.extra[t]) - rem.amt[t];
        }
        if self.marks.iter().any(|(_, m)| matches!(m, crate::game::Mark::ConvergePending(_))) {
            let classes = self.payment_classes(pm);
            let conv = |m: Tag| if pm.convert { convert(m) } else { m };
            // (A creature tapped for convoke spends no mana, CR 702.51a; an
            // artifact tapped for improvise, a unit of no colour, adds none.)
            let mut units: Vec<Tag> = pm.chosen.iter().filter(|s| s.slot != IMPROVISE).map(|s| conv(chosen_mask(&classes, s))).collect();
            // (Mana with a spending restriction spent, by kind with its
            // purposes: never turned into colours.)
            let vkinds = self.virtual_kinds(pm);
            // (The mana made by abilities with a mana cost spent: as little
            // as the rest of the pool allows, which pays more, CR 106.4; whose
            // is chosen with the colours, `record_converge`.)
            let mut made_spent = [0u8; 6];
            for t in 0..6 {
                let plain = (pool.amt[t] as u32 + pm.extra[t] as u32).saturating_sub(pm.extra_made[t] as u32);
                made_spent[t] = (spent[t] as u32).saturating_sub(plain).min(pm.extra_made[t] as u32) as u8;
                let made = made_spent[t];
                let mut art = 0u8;
                for &(_, only, n) in vkinds.iter().filter(|k| k.0 as usize == t) {
                    let left = rem.restr.iter().find(|k| k.0 as usize == t && k.1 == only).map_or(0, |k| k.2);
                    let k = n.saturating_sub(left).min(spent[t] - made - art);
                    art += k;
                    units.extend(std::iter::repeat_n(conv(((1 << t) | RESTRICTED) as Tag | restriction(only)), k as usize));
                }
                units.extend(std::iter::repeat_n(conv(1 << t), (spent[t] - made - art) as usize));
            }
            self.record_converge(pm, &units, made_spent);
        }
        // "The amount of mana spent to cast this" (CR 601.2h; EoE W3): the
        // payment's units (a creature tapped for convoke or an artifact for
        // improvise spends none, 702.51a, 702.126a).
        if let Some(card) = pm.keep.spell {
            if self.def_of(card).w3.mana_spent || self.db.w6.mana_spent_all {
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
                .any(|s| !special(s) && self.src_ability(s.card, s.slot).mana().is_some_and(|m| m.uncounterable));
            // (Or such mana of the pool: its rider, CR 106.6, kept there.)
            let rider = |v: &[crate::eoe_w2::RKind]| v.iter().filter(|k| k.1 & crate::db::MANA_UNCOUNTERABLE != 0).map(|k| k.2 as u32).sum::<u32>();
            let rider_spent = rider(&self.virtual_kinds(pm)) > rider(&rem.restr);
            if uncounterable || rider_spent {
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
            let ad = self.src_ability(s.card, s.slot);
            let dmg = ad.mana().map_or(0, |m| m.damage_to_you);
            let last = ad.mana().and_then(|m| m.sac_if_no).is_some_and(|k| self.counters_on(s.card, k) <= 1);
            (false, ad.cost.sac_self || last, dmg > 0, ad.cost.life == 0)
        });
        // (The permanent each shared source's activation is: the player's
        // choice, `PayMana::actors`.)
        let actor_of = |src: &Src| pm.actors.iter().find(|(x, _)| x == src).map(|x| x.1);
        for src in order {
            if src.slot == IMPROVISE {
                let why = if pm.convoke { "convoke" } else { "improvise" };
                self.note(|g| format!("P{payer} taps {} for {why}", g.name(src.card)));
                self.tap(src.card);
            } else if src.slot == POL_FOR {
                // "{T}, Tap an untapped permanent you control": the
                // Pollinator chosen and this permanent.
                let actor = actor_of(&src).expect("the Pollinator chosen (PayMana::actors)");
                self.note(|g| format!("P{payer} activates a mana ability of {}, tapping {}", g.name(actor), g.name(src.card)));
                self.tap(actor);
                self.tap(src.card);
            } else if src.slot == TAP_FOR {
                // (A cost of another permanent's mana ability: not "tapped
                // for mana", CR 106.12a.)
                self.note(|g| format!("P{payer} taps {} for a mana ability", g.name(src.card)));
                self.tap(src.card);
            } else if is_sac_slot(src.slot) {
                self.activate_sac_for(src, actor_of(&src));
            } else {
                if let AbilitySlot::Activated(i) = src.slot {
                    if self.src_ability(src.card, src.slot).exhaust {
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
        // (Both within the life total, an i32: the budget kept them.)
        if pm.phyrexian_life > 0 {
            self.lose_life(payer, i32::try_from(pm.phyrexian_life).expect("Phyrexian life within the life total"));
            let n = pm.phyrexian_life;
            self.note(|_| format!("P{payer} pays {n} life for Phyrexian mana"));
        }
        if pm.life_after > 0 {
            let n = pm.life_after;
            self.note(|_| format!("P{payer} pays {n} life"));
            self.lose_life(payer, i32::try_from(n).expect("life within the life total"));
        }
    }

    /// Converge (ENGINE.md 28): the colours of mana spent on a spell, from
    /// the payment's `units` (the picked single-mana sources' types, the
    /// pool's and several-mana sources' mana spent): each single-mana
    /// source makes the colour the player prefers, so the count is the most
    /// colours an exact payment by those units can show (the player chooses
    /// each mana's type as the ability resolves, CR 106.1b; 601.2h which
    /// pays what).
    /// `made`: the mana made by abilities with a mana cost spent, by type,
    /// of which activations (`PayMana::made_by`) the most colours any
    /// choice shows (ENGINE.md 100.4.7).
    fn record_converge(&mut self, pm: &PayMana, plain_units: &[Tag], made: [u8; 6]) {
        let Some(k) = self.marks.iter().position(|(_, m)| matches!(m, crate::game::Mark::ConvergePending(_))) else { return };
        let crate::game::Mark::ConvergePending(fixed) = self.marks[k].1 else { unreachable!() };
        // Each choice of the activations whose made mana was spent.
        let conv = |m: Tag| if pm.convert { convert(m) } else { m };
        let mut splits: Vec<Vec<Tag>> = vec![Vec::new()];
        for t in 0..6 {
            let mut next = Vec::new();
            for s in &splits {
                // (`left` of type t to place among the activations from `i`.)
                fn place(by: &[(u64, [u8; 6])], t: usize, i: usize, left: u8, cur: &mut Vec<Tag>, out: &mut Vec<Vec<Tag>>, conv: &dyn Fn(Tag) -> Tag) {
                    if left == 0 {
                        out.push(cur.clone());
                        return;
                    }
                    if i == by.len() {
                        return;
                    }
                    for n in (0..=left.min(by[i].1[t])).rev() {
                        let len = cur.len();
                        cur.extend(std::iter::repeat_n(conv(crate::mana::with_chain(((1u8 << t) | MADE) as Tag, by[i].0)), n as usize));
                        place(by, t, i + 1, left - n, cur, out, conv);
                        cur.truncate(len);
                    }
                }
                place(&pm.made_by, t, 0, made[t], &mut s.clone(), &mut next, &conv);
            }
            splits = next;
        }
        let mut n = 0;
        for s in &splits {
            let mut units = plain_units.to_vec();
            units.extend(s.iter().copied());
            n = n.max(self.converge_colors(pm, &units, fixed));
        }
        self.marks[k].1 = crate::game::Mark::Converge(n as u8);
    }

    /// The most colours (with `fixed`, shown already) an exact payment by
    /// `units` can show (`record_converge`).
    fn converge_colors(&self, pm: &PayMana, units: &[Tag], fixed: u8) -> u32 {
        #[allow(clippy::too_many_arguments)]
        fn best(i: usize, units: &[Tag], cost: &ManaCost, allow: u8, got: &mut Vec<Tag>, fixed: u8) -> u32 {
            if i == units.len() {
                if !crate::mana::payable(cost, allow, &[], got) {
                    return 0;
                }
                return (got.iter().fold(fixed, |m, &x| m | crate::mana::low(x)) & COLORS).count_ones();
            }
            let options: SVec<Tag, 6> = if units[i] & TYPES as Tag == 0 {
                [units[i]].into_iter().collect()
            } else {
                (0..6).filter(|t| units[i] & (1 << t) != 0).map(|t| (1 << t) | (units[i] & !(TYPES as Tag))).collect()
            };
            let mut b = 0;
            for o in options.iter() {
                got.push(*o);
                b = b.max(best(i + 1, units, cost, allow, got, fixed));
                got.pop();
            }
            b
        }
        if units.len() <= 7 && pm.marked.is_empty() {
            best(0, units, &pm.cost, pm.keep.allow, &mut Vec::new(), fixed)
        } else {
            match most_colors_matching(units, &pm.cost, &pm.marked, fixed, pm.keep.allow) {
                Some(n) => n,
                // (Hybrid or Phyrexian pips left with many units: no
                // converge spell in the cube has them; an upper bound.)
                None => {
                    let all = units.iter().fold(fixed, |m, &x| m | crate::mana::low(x)) & COLORS;
                    all.count_ones().min(fixed.count_ones() + units.len() as u32)
                }
            }
        }
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

/// The tag of a picked single-mana source's unit.
fn chosen_mask(classes: &[Class], s: &Src) -> Tag {
    match classes.iter().find(|cl| cl.members.contains(s)).map(|cl| &cl.yld) {
        Some(Yield::One(u)) => u.mask,
        _ => unreachable!("chosen sources make one mana"),
    }
}

/// A unit's mask with Wizard's Rockets: any colour as well (a unit of no
/// type, improvise's, stays generic only; mana with a spending restriction,
/// `RESTRICTED`, can't pay the Rockets' {X}: CR 106.6, ENGINE.md 49).
fn convert(m: Tag) -> Tag {
    if m & TYPES as Tag == 0 || m & RESTRICTED as Tag != 0 {
        m
    } else {
        m | COLORS as Tag
    }
}

/// Every multiset of `k` units of `groups` (kind, count), as counts per
/// group in `cur`, until `f` says stop (returned).
fn each_subset<K>(groups: &[(K, u32)], k: usize, cur: &mut SVec<u32, 12>, f: &mut dyn FnMut(&[u32]) -> bool) -> bool {
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
        cur.push(n as u32);
        let stop = each_subset(groups, k - n, cur, f);
        cur.truncate(i);
        if stop {
            return true;
        }
    }
    false
}

/// A kind of the virtual pool's mana (`vgroups`): its type (bits 0-2), + 8
/// for `MADE`, + 16 for `RESTRICTED`, its purposes in bits 8-15, its
/// activation's chain id from bit 16 (`mana::chain`).
type Kind = u64;

/// The units of `counts` of `groups` as tags (`MADE` for kinds + 8,
/// `RESTRICTED` for kinds + 16, the purposes in bits 8-15, the chain id
/// from bit 16: `vgroups`).
fn masks_of(groups: &[(Kind, u32)], counts: &[u32]) -> SVec<Tag, 16> {
    let mut v = SVec::new();
    for (g, &n) in groups.iter().zip(counts) {
        let low = (1u8 << (g.0 & 7)) | if g.0 & 8 != 0 { MADE } else { 0 } | if g.0 & 16 != 0 { RESTRICTED } else { 0 };
        let m = crate::mana::with_chain(tag(low, (g.0 >> 8) as u8), g.0 >> 16);
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
#[derive(Clone)]
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
    /// The purposes of the cost being paid (`Keep::allow`).
    allow: u8,
    /// The actors of the shared activations (`PoolCtx`).
    pools: PoolCtx,
    /// The candidates of a cost's several reservations (`ResCtx`).
    res: ResCtx,
    /// The uses of a source that only removes counters were cut to what a
    /// cost of the size these were built for can spend (`UseCap`).
    pub(crate) cut: bool,
    /// What a later tap cost still needs from the candidates with another
    /// way (`TapsOwed`: the others counted already); 0 or less: nothing.
    crew: i64,
}

/// The cost size `plain_cands` are built for (`UseCap`): a larger cost with
/// a source cut builds its own.
const PLAIN_TOTAL: usize = 64;

/// The most uses of a source that only removes counters (`Class::res` 0,
/// Pentad Prism) a payment can spend (docs/shortcuts.md N10): each use makes
/// mana, and in a payment that uses no source it could leave out every use
/// has a mana spent, one per need of the payment. The needs: the cost's
/// (`total`), the mana costs of the abilities picked (`marked`), and those
/// the classes' abilities may add (`Class::need`), each member once, or as
/// often as it is used. An ability with a mana cost is never cut: its mana may
/// pay the cost of its next use (13.15: one before another's, `mana::chain`).
/// Uses past these are never spent: leaving them out changes no answer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UseCap {
    needs: crate::num::Count,
}

impl UseCap {
    fn of(classes: &[Class], total: usize, marked: usize) -> UseCap {
        let t = total as u64;
        let mut needs = t + marked as u64;
        for cl in classes.iter().filter(|cl| cl.need != 0) {
            let uses = if cl.res == 0 { cl.reuse as u64 } else { 1 };
            needs = needs.saturating_add(cl.need_n as u64 * cl.members.len() as u64 * uses);
        }
        let count = |n: u64| n.min(crate::num::Count::MAX as u64) as crate::num::Count;
        UseCap { needs: count(needs) }
    }

    /// The most uses of one member of `cl` a payment spends.
    fn uses(&self, cl: &Class) -> crate::num::Count {
        // (An ability with a mana cost is never cut, 13.15.)
        if cl.need != 0 {
            crate::num::Count::MAX
        } else {
            self.needs
        }
    }
}

/// Candidate units, inline up to 32 (ENGINE.md 17).
type Units = SVec<Unit, 32>;

#[derive(Clone, Debug, PartialEq)]
struct Way {
    units: SVec<Unit, 4>,
    life: i32,
    sac: i32,
    /// The ability's own mana cost (`Class::need`), 0 for none, and how
    /// many such needs (`Class::need_n`).
    need: Tag,
    need_n: u8,
    /// The pools of the shared activations it is (`PoolCtx`'s indices,
    /// `NO_POOL` for none): each needs an actor of its pool (two when it taps
    /// a permanent for a Pollinator's ability and sacrifices it for a
    /// Lumberjack's).
    acts: [u16; 2],
    /// The signature of the actor it taps otherwise (its card: `PoolCtx`),
    /// `NO_POOL` for none.
    sig: u16,
    /// The group of the reservations' candidate it uses up (its card:
    /// `ResCtx`), `NO_POOL` for none.
    rsig: u16,
    /// It taps its card for a later tap cost: what the card counts for
    /// there (`TapsOwed`, `Cands::crew`); 0 for a mana way.
    crew: i64,
}

impl Way {
    /// It needs or taps no actor of a shared activation, and uses up no
    /// candidate of several reservations.
    fn unpooled(&self) -> bool {
        self.acts == [NO_POOL; 2] && self.sig == NO_POOL && self.rsig == NO_POOL
    }
}

/// No pool or signature (`Way::acts`, `Way::sig`).
const NO_POOL: u16 = u16::MAX;

/// The actors of a payment's shared activations (ENGINE.md 100.4.7,
/// docs/shortcuts.md N23). Each activation of a pool (`Class::pool`: a Gene
/// Pollinator's or an Orcish Lumberjack's ability, alike within the pool)
/// needs one of the pool's actors that nothing else taps (CR 602.2b: its
/// {T}). The actors are grouped by signature, the pools they belong to and
/// whether a later "tap X untapped <permanents>" needs them (a
/// `Reserve::Perm` that taps): any actor of a signature can do what another can, so a
/// search counts the activations it chooses per pool and the actors it taps
/// otherwise per signature (their own mana abilities, tapped for another's
/// ability, for improvise or convoke, or for a later tap cost). A leaf
/// holds when the activations get distinct actors left (a matching, per
/// pool when no actor is in two), the fewest reserved ones among them, which
/// the sacrifice budget pays. (One count of Pollinators, `Class::share`,
/// shared by the Lumberjacks too, could not tell a creature Pollinator's
/// mana from another's, nor which actor a later cost needs.)
#[derive(Clone, Debug, Default)]
pub(crate) struct PoolCtx {
    /// Per signature: its pools (`Class::pool` - 1), whether it is
    /// reserved, how many of its actors are left.
    sigs: Vec<(SVec<u16, 2>, bool, i32)>,
    /// Each actor left, with its signature.
    cards: Vec<(CardIdx, u16)>,
    /// The activations counted already, per pool: shared picks whose actor
    /// isn't chosen yet, and those committed as fixed units (`cands`).
    acts: SVec<i32, 4>,
}

impl PoolCtx {
    /// The pools of `classes` (each pool's actors from its classes), without
    /// the actors `used` taps, with the activations `anon` (their pools)
    /// counted; `reserved`: an actor a later tap cost needs.
    fn of(classes: &[Class], used: &dyn Fn(CardIdx) -> bool, reserved: &dyn Fn(CardIdx) -> bool, anon: &[u16]) -> PoolCtx {
        let np = classes.iter().map(|cl| cl.pool).max().unwrap_or(0) as usize;
        let mut ctx = PoolCtx::default();
        if np == 0 {
            return ctx;
        }
        ctx.acts = std::iter::repeat_n(0, np).collect();
        for &g in anon {
            ctx.acts[g as usize - 1] += 1;
        }
        let mut members: Vec<Option<&[CardIdx]>> = vec![None; np];
        for cl in classes.iter().filter(|cl| cl.pool > 0) {
            members[cl.pool as usize - 1].get_or_insert(&cl.actors);
        }
        for m in members.iter().flatten() {
            for &a in m.iter() {
                if ctx.cards.iter().any(|x| x.0 == a) || used(a) {
                    continue;
                }
                let pools: SVec<u16, 2> = (0..np).filter(|&k| members[k].is_some_and(|m| m.contains(&a))).map(|k| k as u16).collect();
                let r = reserved(a);
                let si = match ctx.sigs.iter().position(|s| s.0[..] == pools[..] && s.1 == r) {
                    Some(i) => i,
                    None => {
                        ctx.sigs.push((pools, r, 0));
                        ctx.sigs.len() - 1
                    }
                };
                ctx.sigs[si].2 += 1;
                ctx.cards.push((a, si as u16));
            }
        }
        ctx
    }

    /// The signature of actor `c` (`NO_POOL`: not an actor left).
    fn sig_of(&self, c: CardIdx) -> u16 {
        self.cards.iter().find(|x| x.0 == c).map_or(NO_POOL, |x| x.1)
    }

    /// No actor is in two pools.
    fn apart(&self) -> bool {
        self.sigs.iter().all(|s| s.0.len() == 1)
    }

    /// The actors left in pool `p` with the actors `used` per signature tapped
    /// otherwise: (not reserved, reserved).
    fn left_in(&self, p: usize, used: &[i32]) -> (i32, i32) {
        let (mut free, mut rsv) = (0, 0);
        for (k, s) in self.sigs.iter().enumerate() {
            if s.0.contains(&(p as u16)) {
                let n = s.2 - used.get(k).copied().unwrap_or(0);
                if s.1 {
                    rsv += n;
                } else {
                    free += n;
                }
            }
        }
        (free, rsv)
    }

    /// A necessary condition (to prune a search): no pool has more
    /// activations than actors left.
    fn bound_ok(&self, acts: &[i32], used: &[i32]) -> bool {
        (0..acts.len()).all(|p| {
            let (f, r) = self.left_in(p, used);
            acts[p] <= f + r
        })
    }

    /// The fewest reserved actors the activations `acts` (per pool) take with
    /// the actors `used` (per signature) tapped otherwise, or `None` when
    /// they can't all have an actor. Per pool when the pools are apart; else
    /// a matching, whose largest part into actors not reserved is the most
    /// any matching of every activation uses (an augmenting path keeps every
    /// actor it had).
    fn fit(&self, acts: &[i32], used: &[i32]) -> Option<i32> {
        if acts.iter().all(|&a| a == 0) {
            return Some(0);
        }
        if self.apart() {
            let mut res = 0;
            for (p, &a) in acts.iter().enumerate() {
                let (f, r) = self.left_in(p, used);
                if a > f + r {
                    return None;
                }
                res += (a - f).max(0);
            }
            return Some(res);
        }
        let left: Vec<u32> = acts.iter().map(|&a| a.max(0) as u32).collect();
        let right: Vec<u32> = self.sigs.iter().enumerate().map(|(k, s)| (s.2 - used.get(k).copied().unwrap_or(0)).max(0) as u32).collect();
        let edges: Vec<Vec<bool>> = (0..acts.len()).map(|p| self.sigs.iter().map(|s| s.0.contains(&(p as u16))).collect()).collect();
        let total: u32 = left.iter().sum();
        if crate::mana::max_flow(&left, &right, &edges) < total {
            return None;
        }
        let free: Vec<u32> = self.sigs.iter().zip(&right).map(|(s, &n)| if s.1 { 0 } else { n }).collect();
        Some((total - crate::mana::max_flow(&left, &free, &edges)) as i32)
    }
}

/// `used`: what the payment's picks use of a source's card already
/// (`Class::res`) and how many times it was picked (a source whose card
/// use is taken, or picked as often as its class allows, is out).
/// `pools`: the shared activations' actors left (`PoolCtx`); `res`: the
/// candidates of a cost's several reservations (`ResCtx`), which a source
/// `consumes` uses up one of. `chain_of`: a source's chain id (`chain_id`,
/// its uses after it), which its mana cost's needs and its mana carry.
#[allow(clippy::too_many_arguments)]
fn cands(
    classes: &[Class],
    use_src: &dyn Fn(&Src) -> bool,
    used: &dyn Fn(&Src) -> (u8, crate::num::Count),
    dmg: bool,
    reserved: &dyn Fn(&Class, &Src) -> i32,
    consumes: &dyn Fn(&Class, &Src) -> bool,
    chain_of: &dyn Fn(&Src) -> u64,
    allow: u8,
    marked: u8,
    cap: UseCap,
    taps: Option<&(i64, Vec<(CardIdx, i64)>)>,
    mut pools: PoolCtx,
    res: ResCtx,
) -> Cands {
    // (Sources whose mana can pay no need of this payment are left out: CR
    // 106.6, its restriction allows neither the cost's purposes, `allow`,
    // nor those of an ability's mana cost, picked, `marked`, or among the
    // classes; ENGINE.md 100.4.5.)
    let any = allow | marked | needs_allow(classes);
    let filtered: Vec<Class>;
    let classes = if classes.iter().any(|cl| !crate::eoe_w2::spendable(cl.only, any)) {
        filtered = classes.iter().filter(|cl| crate::eoe_w2::spendable(cl.only, any)).cloned().collect();
        &filtered[..]
    } else {
        classes
    };
    // How many times each member may still be used (in order), and how
    // many usable abilities each card has: a card with one simple way goes
    // straight to the fixed units.
    // (A source that only removes counters, `Class::res` 0, uses nothing of
    // its card the others need: its uses are units of their own, at most
    // as many as the payment can spend, `UseCap`.)
    let mut lefts: SVec<crate::num::Count, 32> = SVec::new();
    let mut count: SVec<(CardIdx, u32), 32> = SVec::new();
    let mut cut = false;
    for cl in classes {
        for s in cl.members.iter() {
            let n = if !use_src(s) {
                0
            } else {
                let (res, k) = used(s);
                if cl.res == 0 {
                    let n = cl.reuse.saturating_sub(k);
                    let most = cap.uses(cl);
                    cut |= n > most;
                    n.min(most)
                } else {
                    (res & cl.res == 0) as crate::num::Count
                }
            };
            lefts.push(n);
            if n > 0 && cl.res != 0 {
                match count.iter_mut().find(|(c, _)| *c == s.card) {
                    Some((_, k)) => *k += 1,
                    None => count.push((s.card, 1)),
                }
            }
        }
    }
    // (A candidate of a later tap cost is never one unit: tapping it for
    // that cost is another way of its card, `Way::crew`.)
    let tapper = |c: CardIdx| taps.is_some_and(|(_, v)| v.iter().any(|x| x.0 == c));
    // The pool a source's activation needs an actor of (`PoolCtx`'s index),
    // and the signature of the actor its use taps (its card).
    let act_of = |cl: &Class| if cl.pool > 0 { cl.pool - 1 } else { NO_POOL };
    let sig_use = |cl: &Class, s: &Src, pools: &PoolCtx| if cl.res & TAP != 0 { pools.sig_of(s.card) } else { NO_POOL };
    let rsig_use = |cl: &Class, s: &Src| if !res.need.is_empty() && consumes(cl, s) { res.group_of(s.card) } else { NO_POOL };
    // Gene Pollinator's sources (`POL_FOR`) tapping a permanent that has
    // another way, or that is an actor itself: when each pool's free
    // targets (permanents whose only ways are Pollinator activations, not
    // actors, not reserved, not a later tap cost's candidates) are at least
    // the activations that can still be made (one per actor left), any such
    // activation moves to an unused free target of its pool (the same mana,
    // nothing else of its card used) and is left out, so the permanent stays
    // one unit (no branching on every land; ENGINE.md 99.12, 100.4.7).
    if classes.iter().any(|cl| cl.members[0].slot == POL_FOR) {
        let mut other: SVec<CardIdx, 32> = SVec::new();
        let mut i = 0;
        for cl in classes {
            for s in cl.members.iter() {
                if lefts[i] > 0 && cl.members[0].slot != POL_FOR && !other.contains(&s.card) {
                    other.push(s.card);
                }
                i += 1;
            }
        }
        let free_target =
            |cl: &Class, s: &Src| pools.sig_of(s.card) == NO_POOL && !other.contains(&s.card) && !tapper(s.card) && reserved(cl, s) == 0 && rsig_use(cl, s) == NO_POOL;
        let more = pools.sigs.iter().map(|s| s.2).sum::<i32>() - pools.acts.iter().sum::<i32>();
        let np = pools.acts.len();
        let mut enough = true;
        for p in 0..np {
            let mut free: SVec<CardIdx, 32> = SVec::new();
            let mut any = false;
            i = 0;
            for cl in classes {
                for s in cl.members.iter() {
                    if cl.members[0].slot == POL_FOR && cl.pool as usize == p + 1 {
                        any = true;
                        if lefts[i] > 0 && free_target(cl, s) && !free.contains(&s.card) {
                            free.push(s.card);
                        }
                    }
                    i += 1;
                }
            }
            enough &= !any || free.len() as i32 >= more;
        }
        if enough {
            i = 0;
            for cl in classes {
                for s in cl.members.iter() {
                    if lefts[i] > 0 && cl.members[0].slot == POL_FOR && !free_target(cl, s) {
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
    // Free targets whose only way is one pool's activation, committed as
    // fixed units: as many as the pool's actors left that nothing else may
    // tap (not reserved, no other way of their card that taps it, not a
    // later tap cost's candidate) allow. Such an activation takes an actor
    // nobody else could use and makes the mana any of the pool's makes,
    // costing its card nothing: no branching on them (ENGINE.md 99.12).
    let mut committed: SVec<bool, 32> = std::iter::repeat_n(false, lefts.len()).collect();
    let mut pre_units = Units::new();
    if pools.apart() && classes.iter().any(|cl| cl.members[0].slot == POL_FOR) {
        let np = pools.acts.len();
        // (The actors with another way that taps them.)
        let mut busy: SVec<CardIdx, 8> = SVec::new();
        let mut i = 0;
        for cl in classes {
            for s in cl.members.iter() {
                if lefts[i] > 0 && cl.res & TAP != 0 && pools.sig_of(s.card) != NO_POOL && !busy.contains(&s.card) {
                    busy.push(s.card);
                }
                i += 1;
            }
        }
        for &(c, _) in taps.map_or(&[][..], |t| &t.1[..]) {
            if pools.sig_of(c) != NO_POOL && !busy.contains(&c) {
                busy.push(c);
            }
        }
        for p in 0..np {
            let idle = pools.cards.iter().filter(|&&(c, k)| !pools.sigs[k as usize].1 && pools.sigs[k as usize].0[0] as usize == p && !busy.contains(&c)).count() as i32;
            let mut k = (idle - pools.acts[p]).max(0);
            i = 0;
            for cl in classes {
                for s in cl.members.iter() {
                    let only = count.iter().any(|&(x, n)| x == s.card && n == 1);
                    if k > 0
                        && lefts[i] > 0
                        && cl.members[0].slot == POL_FOR
                        && cl.pool as usize == p + 1
                        && only
                        && pools.sig_of(s.card) == NO_POOL
                        && !tapper(s.card)
                        && reserved(cl, s) == 0
                        && rsig_use(cl, s) == NO_POOL
                        && cl.dmg == 0
                    {
                        let restr = if cl.only != 0 { RESTRICTED as Tag | restriction(cl.only) } else { 0 };
                        match &cl.yld {
                            Yield::One(u) if u.life == 0 => pre_units.push(Unit::free(u.mask | restr)),
                            Yield::Multi { units: Some(us), .. } if cl.need == 0 => pre_units.extend(us.iter().map(|u| Unit::free(u.mask | restr))),
                            _ => {
                                i += 1;
                                continue;
                            }
                        }
                        committed[i] = true;
                        pools.acts[p] += 1;
                        k -= 1;
                    }
                    i += 1;
                }
            }
        }
    }
    let mut li = 0;
    let alone = |c: CardIdx| count.iter().any(|&(x, n)| x == c && n == 1) && !tapper(c);
    let mut out = Cands {
        fixed: pre_units,
        alts: Vec::new(),
        convert: Vec::new(),
        converted: false,
        allow,
        pools: PoolCtx::default(),
        res: ResCtx::default(),
        cut,
        crew: taps.map_or(0, |t| t.0),
    };
    let mut per_card: Vec<(CardIdx, Vec<(Way, u8)>)> = Vec::new();
    // The converters' cards, in `out.convert`'s order.
    let mut conv_cards: SVec<CardIdx, 2> = SVec::new();
    for cl in classes {
        let extra = if dmg { cl.dmg as i32 } else { 0 };
        let made = if cl.need != 0 { MADE } else { 0 };
        // (Mana with a spending restriction this payment meets: never
        // turned into colours, `convert`.)
        let restr = if cl.only != 0 { RESTRICTED as Tag | restriction(cl.only) } else { 0 };
        for s in cl.members.iter() {
            let n = lefts[li];
            let done = committed[li];
            li += 1;
            if n == 0 || done {
                continue;
            }
            let sac = reserved(cl, s);
            let sig = sig_use(cl, s, &pools);
            let rsig = rsig_use(cl, s);
            let acts = [act_of(cl), NO_POOL];
            // (An ability with a mana cost: its activation's chain id on its
            // cost's needs and on its mana, 13.15; each use its own.)
            let base = if cl.need != 0 { chain_of(s) + used(s).1 as u64 } else { 0 };
            let chained = |t: Tag, k: u64| if cl.need != 0 && t & MADE as Tag != 0 { crate::mana::with_chain(t, base + k) } else { t };
            let need_of = |k: u64| if cl.need != 0 { crate::mana::with_chain(cl.need, base + k) } else { 0 };
            if cl.res == 0 {
                // Its uses, each one way of its own (identical: the search
                // counts how many, `ways_payable`), or fixed units when they
                // take nothing a payment shares.
                let units: SVec<Unit, 4> = match &cl.yld {
                    Yield::One(u) => std::iter::once(Unit::free(u.mask | restr)).collect(),
                    Yield::Multi { units: Some(us), .. } => us.iter().map(|u| Unit::free(u.mask | made as Tag | restr)).collect(),
                    Yield::Multi { .. } => SVec::new(),
                };
                let life = extra + if let Yield::One(u) = &cl.yld { u.life as i32 } else { 0 };
                let free = life == 0 && sac == 0 && rsig == NO_POOL && cl.need == 0 && !units.is_empty();
                for k in 0..n {
                    let k = k as u64;
                    if free {
                        out.fixed.extend(units.iter().copied());
                    } else if let Yield::Multi { prods, units: None } = &cl.yld {
                        let way = |p: &[u8; 6]| Way {
                            units: units_of(p).map(|u| Unit::free(chained(u.mask | made as Tag | restr, k))).collect(),
                            life,
                            sac,
                            need: need_of(k),
                            need_n: cl.need_n,
                            acts: [NO_POOL; 2],
                            sig: NO_POOL,
                            rsig,
                            crew: 0,
                        };
                        out.alts.push(prods.iter().map(way).collect());
                    } else {
                        let units = units.iter().map(|u| Unit { mask: chained(u.mask, k), ..*u }).collect();
                        out.alts.push(vec![Way { units, life, sac, need: need_of(k), need_n: cl.need_n, acts: [NO_POOL; 2], sig: NO_POOL, rsig, crew: 0 }]);
                    }
                }
                continue;
            }
            if cl.convert {
                // (Its card's other ways are kept apart, below: activating
                // it taps and sacrifices the card, so they are out then.)
                let units = if cl.delve { std::iter::once(Unit::free(0u8)).collect() } else { SVec::new() };
                out.convert.push((Way { units, life: extra, sac, need: 0, need_n: 0, acts: [NO_POOL; 2], sig, rsig, crew: 0 }, None));
                conv_cards.push(s.card);
                continue;
            }
            let pooled = acts[0] != NO_POOL || sig != NO_POOL || rsig != NO_POOL;
            match &cl.yld {
                Yield::One(u) if alone(s.card) && !pooled => {
                    for _ in 0..n {
                        out.fixed.push(Unit { mask: u.mask | restr, life: (u.life as i32 + extra) as u8, sac: sac > 0 });
                    }
                    continue;
                }
                Yield::Multi { units: Some(us), .. } if alone(s.card) && extra == 0 && sac == 0 && cl.need == 0 && !pooled => {
                    out.fixed.extend(us.iter().map(|u| Unit::free(u.mask | restr)));
                    if cl.delve {
                        out.fixed.push(Unit::free(0u8));
                    }
                    continue;
                }
                _ => {}
            }
            // (Delve's {1} of the card a several-mana source sacrifices: a
            // typeless unit with each production, `Class::delve`.)
            let multi = |mut units: SVec<Unit, 4>| {
                if cl.delve {
                    units.push(Unit::free(0u8));
                }
                Way { units, life: extra, sac, need: need_of(0), need_n: cl.need_n, acts, sig, rsig, crew: 0 }
            };
            let ways: Vec<Way> = match &cl.yld {
                Yield::One(u) => {
                    let units = std::iter::repeat_n(Unit::free(u.mask | restr), n as usize).collect();
                    vec![Way { units, life: u.life as i32 + extra, sac, need: 0, need_n: 0, acts, sig, rsig, crew: 0 }]
                }
                Yield::Multi { units: Some(us), .. } => {
                    vec![multi(us.iter().map(|u| Unit::free(chained(u.mask | made as Tag | restr, 0))).collect())]
                }
                Yield::Multi { prods, units: None } => {
                    prods.iter().map(|p| multi(units_of(p).map(|u| Unit::free(chained(u.mask | made as Tag | restr, 0))).collect())).collect()
                }
            };
            let ways = ways.into_iter().map(|w| (w, cl.res));
            match per_card.iter_mut().find(|(c, _)| *c == s.card) {
                Some((_, w)) => w.extend(ways),
                None => per_card.push((s.card, ways.collect())),
            }
        }
    }
    // A later tap cost's candidates (`TapsOwed`): tapping one for it is
    // another way of its card (none of its other ways then: they tap or
    // sacrifice it, CR 601.2g before 601.2h); one with no other way counts
    // at once, unless it is an actor of a shared activation (tapping it
    // leaves its pool one fewer).
    if let Some((_, tv)) = taps {
        for &(c, n) in tv {
            let sig = pools.sig_of(c);
            let way = Way { units: SVec::new(), life: 0, sac: 0, need: 0, need_n: 0, acts: [NO_POOL; 2], sig, rsig: NO_POOL, crew: n };
            match per_card.iter_mut().find(|(x, _)| *x == c) {
                Some((_, w)) => w.push((way, TAP | SAC)),
                None if sig != NO_POOL || conv_cards.contains(&c) => per_card.push((c, vec![(way, TAP | SAC)])),
                None => out.crew = out.crew.saturating_sub(n),
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
                    acts: [t.acts[0], s.acts[0]],
                    sig: t.sig,
                    rsig: if t.rsig != NO_POOL { t.rsig } else { s.rsig },
                    crew: 0,
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
        if ways.len() > 1 && ways.iter().all(|w| w.life == 0 && w.sac == 0 && w.need == 0 && w.unpooled() && w.crew == 0) {
            if let Some(k) = (0..ways.len()).find(|&k| ways.iter().all(|w| covers(&ways[k].units, &w.units))) {
                out.fixed.extend(ways[k].units.iter().map(|u| Unit::free(u.mask)));
                continue;
            }
        }
        let ways = std::mem::take(&mut ways);
        let same_use = |a: &Way, b: &Way| (a.life, a.sac, a.need, a.need_n) == (b.life, b.sac, b.need, b.need_n);
        if ways.iter().all(|w| w.units.len() == 1 && w.need == 0 && w.unpooled() && w.crew == 0 && same_use(w, &ways[0]) && purposes(w.units[0].mask) == purposes(ways[0].units[0].mask)) {
            let mask = ways.iter().fold(0, |m, w| m | w.units[0].mask);
            out.fixed.push(Unit { mask, ..as_unit(&ways[0]) });
        } else {
            out.alts.push(ways);
        }
    }
    out.pools = pools;
    out.res = res;
    out
}

/// May a unit tagged `a` be used wherever one tagged `b` may (every need `b`
/// pays)? Its types hold `b`'s; `MADE`, `RESTRICTED` and a spending
/// restriction only narrow where a unit goes, so `a` has one only if `b` has
/// it (a restriction then allowing at least `b`'s purposes). A bitwise
/// superset is not enough: Sunken Citadel's restricted {B}{B} has more bits
/// than its plain {B} (EoE fuzz seed 3271 of W6's table, ENGINE.md 100.4.5).
fn unit_covers(a: Tag, b: Tag) -> bool {
    let (la, lb) = (crate::mana::low(a), crate::mana::low(b));
    lb & TYPES & !la == 0
        && (la & MADE == 0 || lb & MADE != 0 && crate::mana::chain(a) == crate::mana::chain(b))
        && (la & RESTRICTED == 0 || lb & RESTRICTED != 0)
        && (purposes(a) == 0 || (purposes(b) != 0 && purposes(b) & !purposes(a) == 0))
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
            if !seen[i] && unit_covers(a[i].mask, b[j].mask) {
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
fn cands_payable(cost: &ManaCost, marked: &[Tag], chosen: &[Tag], c: &Cands, budget: Budget) -> bool {
    cands_payable_c(cost, marked, chosen, c, budget, true)
}

/// [`cands_payable`], Wizard's Rockets' colours only with `convert`.
fn cands_payable_c(cost: &ManaCost, marked: &[Tag], chosen: &[Tag], c: &Cands, budget: Budget, convert: bool) -> bool {
    if c.converted {
        return payable_converted(cost, marked, chosen, c, budget, None);
    }
    if ways_payable(cost, c.allow, marked, chosen, &c.fixed, &c.alts, budget, c.crew, &c.pools, &c.res) {
        return true;
    }
    // Each usable Wizard's Rockets, without its card's other ways.
    convert
        && c.convert.iter().any(|(w, skip)| {
            let left = Budget { life: budget.life - w.life, sac: budget.sac - w.sac };
            if left.life < 0 || left.sac < 0 {
                return false;
            }
            // (Its own units: delve's {1} of its card, `Class::delve`.)
            if w.units.is_empty() && w.sig == NO_POOL && w.rsig == NO_POOL {
                return payable_converted(cost, marked, chosen, c, left, *skip);
            }
            let mut with = c.clone();
            with.fixed.extend(w.units.iter().copied());
            // (Its card tapped: an actor of a shared activation, one fewer;
            // sacrificed: a reservation's candidate one fewer.)
            if w.sig != NO_POOL {
                with.pools.sigs[w.sig as usize].2 -= 1;
            }
            if w.rsig != NO_POOL {
                with.res.groups[w.rsig as usize].1 -= 1;
            }
            payable_converted(cost, marked, chosen, &with, left, *skip)
        })
}

/// [`cands_payable`] with every unit of any colour (Wizard's Rockets),
/// without the ways `alts[skip]` (the converter's card's others).
fn payable_converted(cost: &ManaCost, marked: &[Tag], chosen: &[Tag], c: &Cands, budget: Budget, skip: Option<usize>) -> bool {
    let chosen: SVec<Tag, 16> = chosen.iter().map(|&m| convert(m)).collect();
    let fixed: Units = c.fixed.iter().map(|u| Unit { mask: convert(u.mask), ..*u }).collect();
    let alts: Vec<Vec<Way>> = c
        .alts
        .iter()
        .enumerate()
        .filter(|&(k, _)| Some(k) != skip)
        .map(|(_, ws)| ws.iter().map(|w| Way { units: w.units.iter().map(|u| Unit { mask: convert(u.mask), ..*u }).collect(), ..w.clone() }).collect())
        .collect();
    ways_payable(cost, c.allow, marked, &chosen, &fixed, &alts, budget, c.crew, &c.pools, &c.res)
}

/// The counts of a feasibility search's shared activations per pool and of
/// the actors it taps otherwise per signature (`PoolCtx`, `ways_payable`),
/// and of the reservations' candidates it uses up per group (`ResCtx`).
struct Shared<'a> {
    pools: &'a PoolCtx,
    acts: SVec<i32, 4>,
    used: SVec<i32, 4>,
    res: &'a ResCtx,
    rused: SVec<i32, 4>,
}

impl Shared<'_> {
    /// Count way `w` chosen `k` more times (`k` < 0: fewer).
    fn add(&mut self, w: &Way, k: i32) {
        for &a in w.acts.iter().filter(|&&a| a != NO_POOL) {
            self.acts[a as usize] += k;
        }
        if w.sig != NO_POOL {
            self.used[w.sig as usize] += k;
        }
        if w.rsig != NO_POOL {
            self.rused[w.rsig as usize] += k;
        }
    }

    /// A necessary condition of a branch (to cut it).
    fn bound_ok(&self) -> bool {
        self.pools.bound_ok(&self.acts, &self.used) && self.res.ok(&self.rused)
    }

    /// A leaf's budget: `b` less the reserved actors its activations take,
    /// or `None` when they can't all have an actor or the reservations
    /// aren't met.
    fn leaf(&self, b: Budget) -> Option<Budget> {
        if !self.res.ok(&self.rused) {
            return None;
        }
        let r = self.pools.fit(&self.acts, &self.used)?;
        (r <= b.sac).then_some(Budget { sac: b.sac - r, ..b })
    }
}

/// [`payable_within_n`] with each card of `alts` using one of its ways or
/// none. Exact, with two reductions that change no answer (EoE fuzz seed
/// 10023: two Gene Pollinators and 13 lands, every land a card of three
/// choices, 3^13 leaves per check, made thousands of times per decision;
/// ENGINE.md 100.4):
///
/// * **A dominated "none".** A way that takes no budget and adds no need
///   only adds units the payment may leave unused (available units are
///   optional in `payable_within_n`), so a card with such a way is never left
///   out: a payment that leaves it out is a payment with that way, its units
///   unused.
/// * **Identical cards.** Cards whose ways are equal are interchangeable:
///   which of them uses which way changes no unit, budget or need. They are
///   one group, and the search chooses how many of the group use each way
///   (a multiset), not which card does.
///
/// Ways that take a shared budget (a Pollinator's tap) are still tried each,
/// within the budget: with k Pollinators the search is polynomial in the
/// cards (degree k).
///
/// A later tap cost (`crew`: what it still needs, `Way::crew`) holds at a
/// leaf when the ways chosen tap enough for it. Tapping for it uses no
/// budget and only helps, so a crew way also dominates "none".
///
/// The shared activations' actors (`pools`): a leaf holds when the
/// activations chosen get actors left (`PoolCtx::fit`), the reserved ones
/// among them out of the sacrifice budget; a branch with more activations of
/// a pool than its actors left is cut.
#[allow(clippy::too_many_arguments)]
fn ways_payable(cost: &ManaCost, allow: u8, marked: &[Tag], chosen: &[Tag], fixed: &Units, alts: &[Vec<Way>], budget: Budget, crew: i64, pools: &PoolCtx, res: &ResCtx) -> bool {
    /// A way that takes nothing a payment shares: its units are free.
    fn free_way(w: &Way) -> bool {
        w.life <= 0 && w.sac <= 0 && w.need == 0 && w.unpooled()
    }
    // (`None`: the search's nodes ran out.)
    #[allow(clippy::too_many_arguments)]
    fn go(
        cost: &ManaCost,
        allow: u8,
        needs: &mut SVec<Tag, 8>,
        chosen: &[Tag],
        units: &mut Units,
        groups: &[(&[Way], usize)],
        b: Budget,
        crew: i64,
        sh: &mut Shared,
        nodes: &mut u32,
    ) -> Option<bool> {
        *nodes = nodes.checked_sub(1)?;
        let Some((&(ways, m), rest)) = groups.split_first() else {
            return Some(crew <= 0 && sh.leaf(b).is_some_and(|b| payable_within_n(cost, allow, needs, chosen, units, b)));
        };
        let none_ok = !ways.iter().any(free_way);
        each_way(cost, allow, needs, chosen, units, ways, 0, m, none_ok, rest, b, crew, sh, nodes)
    }
    /// The `left` cards of a group still to place, among `ways[i..]` (and
    /// none, with `none_ok`): how many use `ways[i]`, the rest after it.
    #[allow(clippy::too_many_arguments)]
    fn each_way(
        cost: &ManaCost,
        allow: u8,
        needs: &mut SVec<Tag, 8>,
        chosen: &[Tag],
        units: &mut Units,
        ways: &[Way],
        i: usize,
        left: usize,
        none_ok: bool,
        rest: &[(&[Way], usize)],
        b: Budget,
        crew: i64,
        sh: &mut Shared,
        nodes: &mut u32,
    ) -> Option<bool> {
        if left == 0 || i == ways.len() {
            if left > 0 && !none_ok {
                return Some(false);
            }
            return go(cost, allow, needs, chosen, units, rest, b, crew, sh, nodes);
        }
        let w = &ways[i];
        let (len, nl) = (units.len(), needs.len());
        let mut lb = b;
        let mut lc = crew;
        let mut t = 0;
        let found = loop {
            match each_way(cost, allow, needs, chosen, units, ways, i + 1, left - t, none_ok, rest, lb, lc, sh, nodes) {
                Some(false) => {}
                other => break other,
            }
            if t == left {
                break Some(false);
            }
            lb = Budget { life: lb.life - w.life, sac: lb.sac - w.sac };
            if lb.life < 0 || lb.sac < 0 {
                break Some(false);
            }
            sh.add(w, 1);
            t += 1;
            if !w.unpooled() && !sh.bound_ok() {
                break Some(false);
            }
            units.extend(w.units.iter().copied());
            if w.need != 0 {
                needs.extend(std::iter::repeat_n(w.need, w.need_n as usize));
            }
            lc = lc.saturating_sub(w.crew);
        };
        sh.add(w, -(t as i32));
        units.truncate(len);
        needs.truncate(nl);
        found
    }
    let mut sh = Shared {
        pools,
        acts: pools.acts.clone(),
        used: std::iter::repeat_n(0, pools.sigs.len()).collect(),
        res,
        rused: std::iter::repeat_n(0, res.groups.len()).collect(),
    };
    if alts.is_empty() {
        return crew <= 0 && sh.leaf(budget).is_some_and(|b| payable_within_n(cost, allow, marked, chosen, fixed, b));
    }
    // Identical cards, grouped (in first-appearance order).
    let mut groups: Vec<(&[Way], usize)> = Vec::new();
    for ws in alts {
        match groups.iter_mut().find(|(g, _)| *g == &ws[..]) {
            Some((_, m)) => *m += 1,
            None => groups.push((&ws[..], 1)),
        }
    }
    let mut units = fixed.clone();
    let mut needs: SVec<Tag, 8> = marked.iter().copied().collect();
    let mut nodes = SEARCH_NODES;
    go(cost, allow, &mut needs, chosen, &mut units, &groups, budget, crew, &mut sh, &mut nodes).unwrap_or_else(|| {
        search_exhausted(format_args!(
            "cost {cost}, needs {marked:?}, chosen {chosen:?}, fixed units {fixed:?}, cards with ways {alts:?}, budget {budget:?}, crew {crew}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cube with Snapcaster Mage granting artifacts you control "{T}:
    /// Add {U}. Spend this mana only to cast a spell from anywhere other
    /// than your hand" (Mm'menon, the Right Hand's grant, built as W3's
    /// lowering builds Ragost's: a latent ability of the granter and
    /// `W3Def::grants_activated`; no supported card grants a mana ability
    /// yet, ENGINE.md 100.4.5).
    fn granting_db() -> &'static crate::db::CardDb {
        static DB: std::sync::OnceLock<&'static crate::db::CardDb> = std::sync::OnceLock::new();
        DB.get_or_init(|| {
            use crate::db::{ActEffect, ActivatedDef, CardDb, Cost, ManaAbility, ManaProd, SPEND_OUTSIDE_HAND};
            let mut db = CardDb::from_document(&crate::db::load_cube_document());
            let id = db.expect_id("Snapcaster Mage").unwrap();
            let mana = ManaAbility { prod: ManaProd::One(1 << crate::mana::U), damage_to_you: 0, sac_if_no: None, only: SPEND_OUTSIDE_HAND, uncounterable: false, haste: false, opp_life: 0 };
            let ad = ActivatedDef {
                cost: Cost { tap: true, ..Cost::default() },
                effect: ActEffect::Mana(mana),
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
            };
            let yours = Filter::And(Box::new(Filter::Type(crate::db::types::ARTIFACT)), Box::new(Filter::Controller(true)));
            let d = db.def_mut(id);
            let k = d.activated.len();
            d.activated.push(ad);
            d.latent += 1;
            d.w3.grants_activated.push((yours, k as u8));
            d.w3.seen |= crate::eoe_w3::SEEN_W3_GRANT_ACT;
            db.spend_kinds |= SPEND_OUTSIDE_HAND;
            db.leak()
        })
    }

    /// The cube and a land with four activated abilities ("{T}: Add {C}" and
    /// three "{1}: Draw a card."), to reach a payment source past a
    /// permanent's fourth `ACTIVATE` id (docs/shortcuts.md 31.7.2k).
    fn four_ability_db() -> &'static crate::db::CardDb {
        static DB: std::sync::OnceLock<&'static crate::db::CardDb> = std::sync::OnceLock::new();
        DB.get_or_init(|| {
            use serde_json::json;
            let draw = json!({"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": {"$t": "ManaCost", "generic": 1}, "mana_str": "{1}"},
                "effect": {"$t": "DrawCards", "amount": 1}});
            let tap_c = json!({"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true},
                "effect": {"$t": "AddManaToPool", "produces": [["C", 1]]}, "is_mana_ability": true});
            let printing = json!({"$t": "CardPrinting", "name": "Test Four Ability Land", "card_types": ["Land"], "subtypes": [], "supertypes": [],
                "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "type_line": "Land", "activated_abilities": [tap_c, draw.clone(), draw.clone(), draw]});
            let card: card_ir::Card = serde_json::from_value(json!({"name": "Test Four Ability Land", "custom": true, "printing": printing})).expect("a card");
            let mut cards = crate::db::load_cube_document().cards;
            cards.push(card);
            let db = crate::db::CardDb::from_cards(&cards);
            db.expect_id("Test Four Ability Land").expect("supported");
            db.leak()
        })
    }

    /// CR 601.2g, 106.7: Squandered Resources' "Sacrifice a land: Add one mana
    /// of any type that land could produce" is a source on a land with four
    /// abilities of its own, offered by the land's own id (`MY_PERM`, then
    /// which: `ChooseMode`) since its `ACTIVATE` ids are taken
    /// (docs/shortcuts.md 31.7.2k: it was not offered). The land taps for
    /// {C} and is then sacrificed for another {C}: {2}.
    /// The payment's numbers, exact (ENGINE.md 100.4.6): a cost's symbols
    /// counted past 255 (the u8 sum wrapped: {250}{R}{R}{R}{R}{R}{R} read as
    /// a cost of none), and the virtual pool's mana of a type past 255 (the
    /// pool's 200 {R} and a picked source's 100 were 255).
    #[test]
    fn cr_601_2h_the_payments_amounts_pass_255() {
        let c = ManaCost { generic: 250, pips: [0, 0, 0, 6, 0, 0], ..ManaCost::default() };
        assert_eq!(c.total(), 256);
        assert!(!c.is_zero());
        let db = four_ability_db();
        let mut g = crate::testkit::Scenario::new(db).library(0, &["Island"; 10]).library(1, &["Island"; 10]).at(crate::game::Step::Main1, 0).start();
        g.players[0].pool.amt[crate::mana::R] = 200;
        let mut pm = PayMana::new(0, c);
        pm.extra[crate::mana::R] = 100;
        let groups = g.vgroups(&pm);
        assert_eq!(groups.iter().find(|x| x.0 == crate::mana::R as Kind).map(|x| x.1), Some(300));
    }

    #[test]
    fn cr_601_2g_a_source_past_the_fourth_ability_id_is_offered() {
        use crate::testkit::Scenario;
        let mut g = Scenario::new(four_ability_db())
            .spec(|s| {
                s.auto_single = false;
                s.log = true;
            })
            .library(0, &["Island"; 10])
            .library(1, &["Island"; 10])
            .battlefield(0, &["Squandered Resources", "Test Four Ability Land"])
            .hand(0, &["Sol Ring"])
            .at(crate::game::Step::Main1, 0)
            .start();
        let land = g.permanents_of(0).find(|&c| g.def_of(c).name == "Test Four Ability Land").unwrap();
        let ring = g.card(0, "Sol Ring", Zone::Hand);
        assert!(g.can_pay(0, &ManaCost::parse("{2}").unwrap()), "tap it, then sacrifice it");
        assert!(!g.can_pay(0, &ManaCost::parse("{3}").unwrap()));
        g.choose(Choice::Cast(ring));
        // Sol Ring's {1}: the land's {C} or its sacrifice, the latter by the
        // land's own id and then which source.
        assert!(g.has_option(Choice::Card(land)), "{:?}", g.options_desc());
        g.choose(Choice::Card(land));
        for _ in 0..20 {
            if g.objs[ring.i()].zone == Zone::Stack {
                break;
            }
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
        assert_eq!(g.objs[ring.i()].zone, Zone::Stack, "{}", g.log_lines().join("\\n"));
    }

    /// CR 613.1f, 605.3a, 106.6: a mana ability another permanent grants is a
    /// source of a payment like the permanent's own (docs/faithfulness_audit.md
    /// 100.4.4: `mana_classes` read `abilities(c).activated` only). Two
    /// Crucibles of Worlds tapped for the granted {U} and a Mountain cast
    /// Faithless Looting by flashback ({2}{R}, from the graveyard); that mana
    /// pays nothing cast from the hand.
    #[test]
    fn cr_613_1f_a_granted_mana_ability_pays_a_cost() {
        use crate::testkit::Scenario;
        let scen = || {
            Scenario::new(granting_db())
                .spec(|s| {
                    s.auto_single = false;
                    s.log = true;
                })
                .library(0, &["Island"; 10])
                .library(1, &["Island"; 10])
        };
        let mut g = scen()
            .battlefield(0, &["Snapcaster Mage", "Crucible of Worlds", "Crucible of Worlds", "Mountain"])
            .graveyard(0, &["Faithless Looting"])
            .hand(0, &["Island"])
            .at(crate::game::Step::Main1, 0)
            .start();
        let fl = g.card(0, "Faithless Looting", Zone::Graveyard);
        assert!(g.can_pay_for(0, &ManaCost::parse("{2}{R}").unwrap(), crate::db::SPEND_OUTSIDE_HAND));
        assert!(!g.can_pay_for(0, &ManaCost::parse("{2}{R}").unwrap(), 0), "not for a spell from the hand");
        let a = g.graveyard_action(0, fl);
        let legal: Vec<u16> = g.pending_decision().unwrap().options.iter().map(|o| o.action).collect();
        assert!(legal.contains(&a), "{:?}", g.options_desc());
        g.apply(a, &mut Vec::new()).unwrap();
        for _ in 0..40 {
            if g.obj(fl).zone == Zone::Stack && g.kind() == Some(DecisionKind::Priority) {
                break;
            }
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
        assert_eq!(g.obj(fl).zone, Zone::Stack, "{}", g.log_lines().join("
"));
        let crucibles: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.def_of(c).name == "Crucible of Worlds").collect();
        assert!(crucibles.len() == 2 && crucibles.iter().all(|&c| g.objs[c.i()].tapped), "{}", g.log_lines().join("
"));
    }

    fn units(masks: &[Tag]) -> Vec<Unit> {
        masks.iter().map(|&m| Unit::free(m)).collect()
    }

    /// `covers` is a matching whatever the number of units (ENGINE.md 51:
    /// the search it replaced kept its used units in eight bits and never
    /// ended past eight: Gaea's Cradle's 28 {G}).
    #[test]
    fn covers_matches_any_number_of_units() {
        const W: Tag = 1;
        const G: Tag = 1 << 4;
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
    /// 21 cards each of two ways ({C} or {R}) that cost life, {W} paid by
    /// none of them, 3^21 paths.
    #[test]
    #[should_panic(expected = "a feasibility search visited")]
    fn an_exhausted_feasibility_search_panics_with_its_inputs() {
        // (Each way costs life, a different amount for each card: no way is
        // free, so leaving a card out is a choice too, and no two cards are
        // interchangeable: `ways_payable`'s reductions don't apply.)
        let way = |m: Tag, life: i32| Way { units: std::iter::once(Unit::free(m)).collect(), life, sac: 0, need: 0, need_n: 0, acts: [NO_POOL; 2], sig: NO_POOL, rsig: NO_POOL, crew: 0 };
        let alts: Vec<Vec<Way>> = (0..21).map(|i| vec![way(1 << 5, i + 1), way(1 << 3, i + 1)]).collect();
        let cost = ManaCost::parse("{W}").unwrap();
        ways_payable(&cost, 0, &[], &[], &Units::new(), &alts, Budget::FREE, 0, &PoolCtx::default(), &ResCtx::default());
    }
}
