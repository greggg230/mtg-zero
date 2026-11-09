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
    cost_need, payable_n, payable_within_n, purposes, restriction, search_exhausted, tag, Budget, ManaCost, ManaMask, Tag, Unit, COLORS, FOR_COST, MADE,
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
}

impl Default for Src {
    /// (A filler for inline storage, never a real source.)
    fn default() -> Src {
        Src { card: CardIdx(0), slot: AbilitySlot::Spell }
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
fn most_colors_matching(units: &[Tag], cost: &ManaCost, marked: &[Tag], fixed: u8, allow: u8) -> Option<u32> {
    if cost.hybrid.iter().any(|&n| n > 0) || cost.phyrexian.iter().any(|&n| n > 0) {
        return None;
    }
    if cost.pips.iter().map(|&n| n as usize).sum::<usize>() + cost.generic as usize + marked.len() != units.len() {
        return None; // (not an exact payment by these units)
    }
    fn augment(slot: usize, slots: &[Tag], units: &[Tag], seen: &mut [bool], owner: &mut [usize]) -> bool {
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
        let mut slots: Vec<Tag> = marked.to_vec();
        let mut shows = 0;
        for t in 0..6u8 {
            let show = t < 5 && s & (1 << t) != 0 && fixed & (1 << t) == 0;
            shows += (show && cost.pips[t as usize] == 0) as u8;
            let need = cost.pips[t as usize].max(show as u8);
            slots.extend(std::iter::repeat_n(cost_need(1u8 << t, allow), need as usize));
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
    /// Orcish Lumberjack (`LJ_FOR`): how many Lumberjacks can be tapped
    /// (each activation taps one); 0 otherwise. Gene Pollinator (`POL_FOR`):
    /// how many Pollinators can activate.
    pub share: u8,
    /// How much of `share` one use takes (0: 1): 2 for a Pollinator that
    /// could activate tapped for another's ability, 1 for one tapped for
    /// improvise or convoke (it can't activate then; ENGINE.md 99.12).
    pub taps: u8,
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
            share: 0,
            taps: 0,
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
            self.uses(s) < cl.reuse
        } else {
            self.used_res(classes, s.card) & cl.res == 0
        }
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
    /// cost taps before the mana is paid: teamwork's creatures). A
    /// Lumberjack's sacrifices (`LJ_FOR`, `Class::share` Lumberjacks to tap)
    /// lose the Lumberjacks among `cards` and go when none is left; the other
    /// creatures tapped leave them (ENGINE.md 71.5: every shared class went
    /// whenever any creature was tapped, so a teamwork pick of a creature
    /// without a mana ability found the Lumberjack's mana gone that the
    /// cost's check, tapping no source, had counted).
    /// A source that only removes counters (`Class::res` 0) stays: it taps
    /// and sacrifices nothing (CR 601.2g). A Pollinator among `cards` no
    /// longer activates (`POL_FOR`'s share and the improvise and convoke
    /// members it splits, ENGINE.md 99.12), nor a Lumberjack.
    pub(crate) fn classes_without(&self, classes: &[Class], cards: &[CardIdx]) -> Vec<Class> {
        let mut actors: Option<Vec<CardIdx>> = None;
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
                let mut share = cl.share;
                if share > 0 {
                    let tapped = if cl.members[0].slot == LJ_FOR {
                        let land = cl.members[0].card;
                        cards.iter().filter(|&&c| self.lumberjack_for(c, land)).count()
                    } else {
                        let p = self.objs[cl.members[0].card.i()].controller;
                        let acts = actors.get_or_insert_with(|| self.pol_activators(p));
                        cards.iter().filter(|c| acts.contains(c)).count()
                    };
                    share = share.saturating_sub(tapped.min(u8::MAX as usize) as u8);
                    if share == 0 {
                        return None;
                    }
                }
                Some(Class { members: m, share, ..cl.clone() })
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
        let mut sac_by: SVec<(CardIdx, u8), 4> = SVec::new();
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
                None => out.push(Class { res: TAP, ..Class::of(def, src, Yield::One(Unit::free(0u8))) }),
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
            // (It pays the spell's cost only, never a mana ability's:
            // `FOR_COST`.)
            let yld = Yield::One(Unit::free(tag((self.colors_of(c) & COLORS) | RESTRICTED, FOR_COST)));
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
        (Budget { life: pl.life.saturating_sub(later as i32).max(0), sac: sac.max(0), tap: i32::MAX }, later > 0)
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

    /// The candidates of a payment of a cost of `total` mana (`ManaCost::total`;
    /// it bounds the uses of a source that only removes counters,
    /// `UseCap`).
    fn cands(&self, p: PlayerId, keep: &Keep, classes: &[Class], use_src: &dyn Fn(&Src) -> bool, dmg: bool, total: usize) -> Cands {
        let cap = UseCap::of(classes, total, 0);
        let taps = self.taps_owed(p, keep, classes, &|_| true);
        cands(classes, use_src, &|_| (0, 0), dmg, &|cl, s| self.reserved_n(p, keep, cl, s), keep.allow, 0, cap, taps.as_ref())
    }

    /// `keep.taps` for the feasibility (`cands`): the need, and each
    /// candidate `free` keeps (not used by a pick) with what it counts for
    /// and whether tapping it takes one of the shared taps (a Pollinator
    /// that could activate, a Lumberjack: `Class::share`).
    #[allow(clippy::type_complexity)]
    fn taps_owed(&self, p: PlayerId, keep: &Keep, classes: &[Class], free: &dyn Fn(CardIdx) -> bool) -> Option<(i64, Vec<(CardIdx, i64, bool)>)> {
        let t = keep.taps.as_ref()?;
        let shared = classes.iter().any(|cl| cl.share > 0);
        let actors = if shared { self.pol_activators(p) } else { Vec::new() };
        let lj = |c: CardIdx| classes.iter().any(|cl| cl.members[0].slot == LJ_FOR && self.lumberjack_for(c, cl.members[0].card));
        let cands = t
            .cands
            .iter()
            .filter(|&&(c, n)| n > 0 && free(c))
            .map(|&(c, n)| (c, n, shared && (actors.contains(&c) || lj(c))))
            .collect();
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
        let taps = self.taps_owed(pm.payer, &pm.keep, classes, &|c| used(&Src { card: c, slot: AbilitySlot::Spell }).0 & (TAP | SAC) == 0);
        let mut c = cands(&classes[from..], &|_| true, &used, dmg, &|cl, s| self.reserved_n(pm.payer, &pm.keep, cl, s), pm.keep.allow, marked, cap, taps.as_ref());
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
    fn vgroups(&self, pm: &PayMana) -> SVec<(u16, u8), 12> {
        let pool = &self.players[pm.payer as usize].pool;
        let kinds = self.pool_kinds(pm.payer);
        let any = pm.keep.allow | crate::eoe_w2::SPEND_ABILITIES;
        let of = |v: &[crate::eoe_w2::RKind], t: usize| v.iter().filter(|k| k.0 as usize == t).fold(0u8, |a, k| a.saturating_add(k.2));
        let made_of = |t: usize, only: u8| pm.extra_made_restr.iter().find(|k| k.0 as usize == t && k.1 == only).map_or(0, |k| k.2);
        let mut g: SVec<(u16, u8), 12> = SVec::new();
        let push = |g: &mut SVec<(u16, u8), 12>, kind: u16, n: u8| {
            if n == 0 {
                return;
            }
            match g.iter_mut().find(|x| x.0 == kind) {
                Some(x) => x.1 = x.1.saturating_add(n),
                None => g.push((kind, n)),
            }
        };
        for t in 0..6 {
            let rk = |only: u8| t as u16 | 16 | (purposes(restriction(only)) as u16) << 8;
            // (Saturating: at most 255 of a type, shortcuts.md 46.3.)
            let restr = of(&kinds, t).saturating_add(of(&pm.extra_restr, t));
            let made_restr = of(&pm.extra_made_restr, t);
            let plain = pool.amt[t].saturating_add(pm.extra[t]) - pm.extra_made[t] - (restr - made_restr);
            push(&mut g, t as u16, plain);
            for &(_, only, n) in kinds.iter().filter(|k| k.0 as usize == t) {
                if crate::eoe_w2::spendable(only, any) {
                    push(&mut g, rk(only), n);
                }
            }
            for &(_, only, n) in pm.extra_restr.iter().filter(|k| k.0 as usize == t) {
                push(&mut g, rk(only), n - made_of(t, only));
            }
            push(&mut g, t as u16 | 8, pm.extra_made[t] - made_restr);
            for &(_, only, n) in pm.extra_made_restr.iter().filter(|k| k.0 as usize == t) {
                push(&mut g, rk(only) | 8, n);
            }
        }
        g
    }

    /// The most mana of the virtual pool `groups` a payment of `pm.cost`
    /// and `marked` can spend with every unit of `chosen` and some of
    /// `rest`; `None` if it can't be paid. Wizard's Rockets' colours count
    /// only when nothing else pays (ENGINE.md 18).
    fn most_spent(&self, pm: &PayMana, groups: &[(u16, u8)], marked: &[Tag], chosen: &[Tag], rest: &Cands, budget: Budget) -> Option<u8> {
        self.most_spent_from(pm, groups, marked, chosen, rest, budget, 0)
    }

    /// [`most_spent`](Self::most_spent) if it is `floor` or more.
    #[allow(clippy::too_many_arguments)]
    fn most_spent_from(
        &self,
        pm: &PayMana,
        groups: &[(u16, u8)],
        marked: &[Tag],
        chosen: &[Tag],
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
                    let mut ch: SVec<Tag, 16> = chosen.iter().copied().collect();
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
    fn singles_vm(&self, pm: &PayMana, classes: &[Class], groups: &[(u16, u8)], marked: &[Tag], plus: Option<Src>, budget: Budget, dmg: bool) -> Option<u8> {
        let first = classes.iter().position(|cl| matches!(cl.yld, Yield::One(_))).unwrap_or(classes.len());
        let rest = self.pay_cands(pm, classes, first.max(pm.min_class as usize), plus, dmg);
        self.most_spent(pm, groups, marked, &[], &rest, budget)
    }

    /// The most of the virtual pool payment `pm` can spend with its picks
    /// so far and the single-mana sources from `pm.min_class` on paying the
    /// rest (`vm` from the state alone: equal to the `vm` the picks were
    /// made with; a decision refreshed in another world, ENGINE.md 10, or a
    /// pool changed by a test, reads it again).
    fn vm_now(&self, pm: &PayMana, classes: &[Class]) -> Option<u8> {
        if pm.marked.is_empty() && !pm.convert && pm.extra == [0; 6] && self.players[pm.payer as usize].pool.total() == 0 {
            return Some(0);
        }
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
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
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
        let budget = self.after_picks(pm, classes, dmg, budget, None).expect("picks within the budget");
        pm.vm = self.singles_vm(pm, classes, &self.vgroups(pm), &pm.marked, None, budget, dmg);
    }

    /// Can `p` pay `cost` (Phyrexian pips included) from their pool and the
    /// sources in `classes`, leaving what `keep` needs?
    pub(crate) fn can_pay_with(&self, p: PlayerId, cost: &ManaCost, classes: &[Class], keep: &Keep) -> bool {
        let classes = self.classes_for_self_use(classes, keep.self_use);
        self.might_pay(p, cost, &classes, None) && self.phyrexian_feasible(p, cost, &classes, keep, keep.life, None)
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
        self.might_pay(p, cost, &classes, without) && self.phyrexian_feasible(p, cost, &classes, keep, keep.life, without)
    }

    /// `classes` for a cost that takes `su` of its source (`SelfUse`, CR
    /// 601.2g-h; docs/shortcuts.md 13.3): without the source's sources that
    /// conflict with it. A source that only removes counters keeps the uses
    /// the cost's counters leave (and, when the cost needs the source, not
    /// the one whose last counter sacrifices it); a Pollinator or a
    /// Lumberjack the cost taps no longer activates (`Class::share`).
    pub(crate) fn classes_for_self_use<'a>(&self, classes: &'a [Class], su: Option<SelfUse>) -> std::borrow::Cow<'a, [Class]> {
        let Some(su) = su else { return std::borrow::Cow::Borrowed(classes) };
        let actor = su.tap && classes.iter().any(|cl| cl.share > 0 && cl.members[0].slot != LJ_FOR) && {
            let p = self.objs[su.card.i()].controller;
            self.pol_activators(p).contains(&su.card)
        };
        if !actor && !classes.iter().any(|cl| cl.members.iter().any(|s| s.card == su.card) || cl.share > 0) {
            return std::borrow::Cow::Borrowed(classes);
        }
        let conflict = if su.tap { TAP | SAC } else if su.moves { SAC } else { 0 };
        let mut out = Vec::with_capacity(classes.len());
        for cl in classes {
            let first = cl.members[0];
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
            let shared = cl.share > 0;
            if shared && su.tap {
                let tapped = if first.slot == LJ_FOR { self.lumberjack_for(su.card, first.card) } else { actor };
                cl.share -= tapped as u8;
            }
            if !cl.members.is_empty() && !(shared && cl.share == 0) && cl.reuse > 0 {
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
                let mut c = self.cands(p, keep, classes, &|s| Some(s.card) != without, dmg, cost.total() as usize);
                self.add_pool(p, keep.allow | needs_allow(classes), &mut c);
                cands_payable(cost, &[], &[], &c, budget)
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
                        let kind = t as u16 | if cl.need != 0 { 8 } else { 0 } | if cl.only != 0 { 16 | (purposes(restriction(cl.only)) as u16) << 8 } else { 0 };
                        match groups.iter_mut().find(|g| g.0 == kind) {
                            Some(g) => g.1 += prod[t],
                            None => groups.push((kind, prod[t])),
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
                    let had = |kind: u16| base.iter().find(|g| g.0 == kind).map_or(0, |g| g.1);
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
        let (budget, dmg) = self.budget(pm.payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
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
                    let classes = self.payment_classes(&pm);
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
                let (budget, dmg) = self.budget(payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
                let rest = self.pay_cands(&pm, &classes, 0, Some(src), dmg);
                // (One mana, its type; several (Black Lotus's three of one
                // colour), a production: each that leaves the rest payable.)
                let cl = &classes[ci];
                let all: Vec<[u8; 6]> = match &cl.yld {
                    Yield::One(u) => (0..6).filter(|&t| u.mask & (1 << t) != 0).map(|t| one(t, 1)).collect(),
                    Yield::Multi { prods, .. } => prods.clone(),
                };
                let kind_of = |t: usize| t as u16 | if cl.only != 0 { 16 | (purposes(restriction(cl.only)) as u16) << 8 } else { 0 };
                let prods: Vec<[u8; 6]> = all
                    .into_iter()
                    .filter(|prod| {
                        let mut groups = self.vgroups(&pm);
                        for t in (0..6).filter(|&t| prod[t] > 0) {
                            match groups.iter_mut().find(|g| g.0 == kind_of(t)) {
                                Some(g) => g.1 += prod[t],
                                None => groups.push((kind_of(t), prod[t])),
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
                    Yield::One(_) => {
                        // (Paying instead of the pool's mana: the pool's
                        // spend again, now with this pick, 13.13.)
                        let covered = pm.pool_covers();
                        pm.chosen.push(src);
                        if covered {
                            let (budget, dmg) = self.budget(payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
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
        let (budget, dmg) = self.budget(payer, &pm.keep, pm.keep.life + pm.phyrexian_life);
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
        let mut card_slots: Vec<(CardIdx, Vec<AbilitySlot>)> = Vec::new();
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
                // for in `card_slots`. A sacrifice past the permanent's
                // fourth `ACTIVATE` id too: docs/shortcuts.md 31.7.2k.)
                let perm_id = by_perm(rep.slot) || (is_sac_slot(rep.slot) && cl.slot_id >= 4);
                if perm_id {
                    match card_slots.iter_mut().find(|(c, _)| *c == rep.card) {
                        Some((_, v)) => {
                            v.push(rep.slot);
                            continue;
                        }
                        None => card_slots.push((rep.card, vec![rep.slot])),
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
        let mut parts: Vec<((u8, u8, bool, bool), u8)> = Vec::new();
        let mut add = |key: (u8, u8, bool, bool), n: u8| {
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
            add((t, 0, false, false), pool.amt[t as usize] - restr - haste);
            add((t, 0, true, false), haste);
        }
        for &(t, only, n) in kinds.iter() {
            add((t, only, false, false), n);
        }
        // The several-mana sources picked: each production with its class's
        // restriction, haste and need.
        let any = pm.keep.allow | pm.marked.iter().fold(0, |a, &m| a | purposes(m));
        for (src, prod) in &pm.multi {
            let cl = classes.iter().find(|cl| cl.members.contains(src)).expect("a picked source's class");
            for t in 0..6u8 {
                add((t, cl.only, cl.haste && t as usize == R, cl.need != 0), prod[t as usize]);
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
        let groups: SVec<(u8, u8), 12> = spendable.iter().map(|&i| (i as u8, parts[i].1)).collect();
        let mask_of = |i: usize| {
            let (t, only, _, made) = parts[i].0;
            let m = ((1u8 << t) | if made { MADE } else { 0 } | if only != 0 { RESTRICTED } else { 0 }) as Tag | restriction(only);
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
                spent[g.0 as usize] = n;
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
            pm.marked.extend(std::iter::repeat_n(cl.need, cl.need_n as usize));
        }
        if cl.haste {
            pm.extra_haste = pm.extra_haste.saturating_add(prod[crate::mana::R]);
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
            for t in 0..6 {
                let made = spent[t].min(pm.extra_made[t]);
                units.extend(std::iter::repeat_n(conv(((1 << t) | MADE) as Tag), made as usize));
                let mut art = 0u8;
                for &(_, only, n) in vkinds.iter().filter(|k| k.0 as usize == t) {
                    let left = rem.restr.iter().find(|k| k.0 as usize == t && k.1 == only).map_or(0, |k| k.2);
                    let k = n.saturating_sub(left).min(spent[t] - made - art);
                    art += k;
                    units.extend(std::iter::repeat_n(conv(((1 << t) | RESTRICTED) as Tag | restriction(only)), k as usize));
                }
                units.extend(std::iter::repeat_n(conv(1 << t), (spent[t] - made - art) as usize));
            }
            self.record_converge(pm, &units);
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
    fn record_converge(&mut self, pm: &PayMana, units: &[Tag]) {
        let Some(k) = self.marks.iter().position(|(_, m)| matches!(m, crate::game::Mark::ConvergePending(_))) else { return };
        let crate::game::Mark::ConvergePending(fixed) = self.marks[k].1 else { unreachable!() };
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
        let n = if units.len() <= 7 && pm.marked.is_empty() {
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
fn each_subset<K>(groups: &[(K, u8)], k: usize, cur: &mut SVec<u8, 12>, f: &mut dyn FnMut(&[u8]) -> bool) -> bool {
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

/// The units of `counts` of `groups` as tags (`MADE` for kinds + 8,
/// `RESTRICTED` for kinds + 16, the purposes in bits 8-15: `vgroups`).
fn masks_of(groups: &[(u16, u8)], counts: &[u8]) -> SVec<Tag, 16> {
    let mut v = SVec::new();
    for (g, &n) in groups.iter().zip(counts) {
        let low = (1u8 << (g.0 & 7)) | if g.0 & 8 != 0 { MADE } else { 0 } | if g.0 & 16 != 0 { RESTRICTED } else { 0 };
        let m = tag(low, (g.0 >> 8) as u8);
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
    /// How many Lumberjacks there are to tap (`Budget::tap`).
    tap_limit: i32,
    /// Units of sources that take one of the shared taps and whose cards
    /// have no other way (Gene Pollinator tapping a permanent without a
    /// mana ability): as many as the taps left allow are fixed units
    /// (they are worth as much as any other such source, and cost its card
    /// nothing: no branching on them; ENGINE.md 99.12).
    free_shared: Units,
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
/// often as it is used. An ability with a mana cost makes `MADE` mana, which
/// pays only the cost (`mana::fits`), so it is used at most `total` times.
/// Uses past these are never spent: leaving them out changes no answer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UseCap {
    total: crate::num::Count,
    needs: crate::num::Count,
}

impl UseCap {
    fn of(classes: &[Class], total: usize, marked: usize) -> UseCap {
        let t = total as u64;
        let mut needs = t + marked as u64;
        for cl in classes.iter().filter(|cl| cl.need != 0) {
            let uses = if cl.res == 0 { (cl.reuse as u64).min(t) } else { 1 };
            needs = needs.saturating_add(cl.need_n as u64 * cl.members.len() as u64 * uses);
        }
        let count = |n: u64| n.min(crate::num::Count::MAX as u64) as crate::num::Count;
        UseCap { total: count(t), needs: count(needs) }
    }

    /// The most uses of one member of `cl` a payment spends.
    fn uses(&self, cl: &Class) -> crate::num::Count {
        if cl.need != 0 {
            self.total
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
    /// It taps a Lumberjack (`Budget::tap`).
    tap: i32,
    /// It taps its card for a later tap cost: what the card counts for
    /// there (`TapsOwed`, `Cands::crew`); 0 for a mana way.
    crew: i64,
}

/// `used`: what the payment's picks use of a source's card already
/// (`Class::res`) and how many times it was picked (a source whose card
/// use is taken, or picked as often as its class allows, is out).
fn cands(
    classes: &[Class],
    use_src: &dyn Fn(&Src) -> bool,
    used: &dyn Fn(&Src) -> (u8, crate::num::Count),
    dmg: bool,
    reserved: &dyn Fn(&Class, &Src) -> i32,
    allow: u8,
    marked: u8,
    cap: UseCap,
    taps: Option<&(i64, Vec<(CardIdx, i64, bool)>)>,
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
        let free_member = |cl: &Class, s: &Src| {
            cl.taps <= 1 && !other.iter().any(|&(c, n)| c == s.card && n > 0) && !taps.is_some_and(|(_, v)| v.iter().any(|x| x.0 == s.card))
        };
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
    // (A candidate of a later tap cost is never one unit: tapping it for
    // that cost is another way of its card, `Way::crew`.)
    let tapper = |c: CardIdx| taps.is_some_and(|(_, v)| v.iter().any(|x| x.0 == c));
    let alone = |c: CardIdx| count.iter().any(|&(x, n)| x == c && n == 1) && !tapper(c);
    let mut out = Cands {
        fixed: Units::new(),
        alts: Vec::new(),
        convert: Vec::new(),
        converted: false,
        allow,
        tap_limit: i32::MAX,
        free_shared: Units::new(),
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
            li += 1;
            if n == 0 {
                continue;
            }
            let sac = reserved(cl, s);
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
                let free = life == 0 && sac == 0 && cl.need == 0 && !units.is_empty();
                for _ in 0..n {
                    if free {
                        out.fixed.extend(units.iter().copied());
                    } else if let Yield::Multi { prods, units: None } = &cl.yld {
                        let way = |p: &[u8; 6]| Way {
                            units: units_of(p).map(|u| Unit::free(u.mask | made as Tag | restr)).collect(),
                            life,
                            sac,
                            need: cl.need,
                            need_n: cl.need_n,
                            tap: 0,
                            crew: 0,
                        };
                        out.alts.push(prods.iter().map(way).collect());
                    } else {
                        out.alts.push(vec![Way { units: units.clone(), life, sac, need: cl.need, need_n: cl.need_n, tap: 0, crew: 0 }]);
                    }
                }
                continue;
            }
            if cl.convert {
                // (Its card's other ways are kept apart, below: activating
                // it taps and sacrifices the card, so they are out then.)
                let units = if cl.delve { std::iter::once(Unit::free(0u8)).collect() } else { SVec::new() };
                out.convert.push((Way { units, life: extra, sac, need: 0, need_n: 0, tap: 0, crew: 0 }, None));
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
                Way { units, life: extra, sac, need: cl.need, need_n: cl.need_n, tap, crew: 0 }
            };
            let ways: Vec<Way> = match &cl.yld {
                Yield::One(u) => {
                    let units = std::iter::repeat_n(Unit::free(u.mask | restr), n as usize).collect();
                    vec![Way { units, life: u.life as i32 + extra, sac, need: 0, need_n: 0, tap, crew: 0 }]
                }
                Yield::Multi { units: Some(us), .. } => {
                    vec![multi(us.iter().map(|u| Unit::free(u.mask | made as Tag | restr)).collect())]
                }
                Yield::Multi { prods, units: None } => {
                    prods.iter().map(|p| multi(units_of(p).map(|u| Unit::free(u.mask | made as Tag | restr)).collect())).collect()
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
    // at once, unless it takes a shared tap (a Pollinator, a Lumberjack).
    if let Some((_, tv)) = taps {
        for &(c, n, shared) in tv {
            let way = Way { units: SVec::new(), life: 0, sac: 0, need: 0, need_n: 0, tap: shared as i32, crew: n };
            match per_card.iter_mut().find(|(x, _)| *x == c) {
                Some((_, w)) => w.push((way, TAP | SAC)),
                None if shared || conv_cards.contains(&c) => per_card.push((c, vec![(way, TAP | SAC)])),
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
                    tap: t.tap + s.tap,
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
        if ways.len() > 1 && ways.iter().all(|w| w.life == 0 && w.sac == 0 && w.need == 0 && w.tap == 0 && w.crew == 0) {
            if let Some(k) = (0..ways.len()).find(|&k| ways.iter().all(|w| covers(&ways[k].units, &w.units))) {
                out.fixed.extend(ways[k].units.iter().map(|u| Unit::free(u.mask)));
                continue;
            }
        }
        let ways = std::mem::take(&mut ways);
        let same_use = |a: &Way, b: &Way| (a.life, a.sac, a.need, a.need_n) == (b.life, b.sac, b.need, b.need_n);
        if ways.iter().all(|w| w.units.len() == 1 && w.need == 0 && w.tap == 0 && w.crew == 0 && same_use(w, &ways[0]) && purposes(w.units[0].mask) == purposes(ways[0].units[0].mask)) {
            let mask = ways.iter().fold(0, |m, w| m | w.units[0].mask);
            out.fixed.push(Unit { mask, ..as_unit(&ways[0]) });
        } else {
            out.alts.push(ways);
        }
    }
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
        && (la & MADE == 0 || lb & MADE != 0)
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
            allow: c.allow,
            tap_limit: c.tap_limit,
            free_shared: Units::new(),
            cut: c.cut,
            crew: c.crew,
        };
        (&with_free, Budget { tap: budget.tap - k as i32, ..budget })
    };
    if c.converted {
        return payable_converted(cost, marked, chosen, c, budget, None);
    }
    if ways_payable(cost, c.allow, marked, chosen, &c.fixed, &c.alts, budget, c.crew) {
        return true;
    }
    // Each usable Wizard's Rockets, without its card's other ways.
    convert
        && c.convert.iter().any(|(w, skip)| {
            let left = Budget { life: budget.life - w.life, sac: budget.sac - w.sac, tap: budget.tap };
            if left.life < 0 || left.sac < 0 {
                return false;
            }
            // (Its own units: delve's {1} of its card, `Class::delve`.)
            if w.units.is_empty() {
                return payable_converted(cost, marked, chosen, c, left, *skip);
            }
            let mut with = c.clone();
            with.fixed.extend(w.units.iter().copied());
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
    ways_payable(cost, c.allow, marked, &chosen, &fixed, &alts, budget, c.crew)
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
#[allow(clippy::too_many_arguments)]
fn ways_payable(cost: &ManaCost, allow: u8, marked: &[Tag], chosen: &[Tag], fixed: &Units, alts: &[Vec<Way>], budget: Budget, crew: i64) -> bool {
    /// A way that takes nothing a payment shares: its units are free.
    fn free_way(w: &Way) -> bool {
        w.life <= 0 && w.sac <= 0 && w.tap <= 0 && w.need == 0
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
        nodes: &mut u32,
    ) -> Option<bool> {
        *nodes = nodes.checked_sub(1)?;
        let Some((&(ways, m), rest)) = groups.split_first() else {
            return Some(crew <= 0 && payable_within_n(cost, allow, needs, chosen, units, b));
        };
        let none_ok = !ways.iter().any(free_way);
        each_way(cost, allow, needs, chosen, units, ways, 0, m, none_ok, rest, b, crew, nodes)
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
        nodes: &mut u32,
    ) -> Option<bool> {
        if left == 0 || i == ways.len() {
            if left > 0 && !none_ok {
                return Some(false);
            }
            return go(cost, allow, needs, chosen, units, rest, b, crew, nodes);
        }
        let w = &ways[i];
        let (len, nl) = (units.len(), needs.len());
        let mut lb = b;
        let mut lc = crew;
        let mut t = 0;
        let found = loop {
            if each_way(cost, allow, needs, chosen, units, ways, i + 1, left - t, none_ok, rest, lb, lc, nodes)? {
                break true;
            }
            if t == left {
                break false;
            }
            lb = Budget { life: lb.life - w.life, sac: lb.sac - w.sac, tap: lb.tap - w.tap };
            if lb.life < 0 || lb.sac < 0 || lb.tap < 0 {
                break false;
            }
            units.extend(w.units.iter().copied());
            if w.need != 0 {
                needs.extend(std::iter::repeat_n(w.need, w.need_n as usize));
            }
            lc = lc.saturating_sub(w.crew);
            t += 1;
        };
        units.truncate(len);
        needs.truncate(nl);
        Some(found)
    }
    if alts.is_empty() {
        return crew <= 0 && payable_within_n(cost, allow, marked, chosen, fixed, budget);
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
    go(cost, allow, &mut needs, chosen, &mut units, &groups, budget, crew, &mut nodes).unwrap_or_else(|| {
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
        let way = |m: Tag, life: i32| Way { units: std::iter::once(Unit::free(m)).collect(), life, sac: 0, need: 0, need_n: 0, tap: 0, crew: 0 };
        let alts: Vec<Vec<Way>> = (0..21).map(|i| vec![way(1 << 5, i + 1), way(1 << 3, i + 1)]).collect();
        let cost = ManaCost::parse("{W}").unwrap();
        ways_payable(&cost, 0, &[], &[], &Units::new(), &alts, Budget::FREE, 0);
    }
}
