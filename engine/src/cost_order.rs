//! The order of a cost's parts (CR 601.2h: "The player pays the total cost
//! in any order"; ENGINE.md 100.4.9, B6).
//!
//! The engine pays a cost's mana first (601.2g's mana abilities and the
//! mana, `PayMana`), then the rest in a fixed order (`Game::pay_costs`),
//! each part's objects chosen from the state before the payment. That
//! reaches every outcome a player could reach while paying one part can't
//! change what another may use but by taking the objects it takes, which the
//! fixed order handles (`Game::order_free`, the proof). Where it can (a
//! permanent returned to hand and then discarded, a static ability gone
//! with the permanent sacrificed), the player chooses which part is paid
//! next (`DecisionKind::CostPart`), each part's objects are chosen from the
//! state as it is after the parts already paid, and each part is paid as
//! its objects are chosen; every option is one that leaves the payment
//! completable, which a search of the payment's own decisions decides
//! (`Game::payment_completes`), and castability asks the same search.

use crate::db::{Cond, Cost, Filter, Mod, PtCda, Scope};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, PlayerId, Zone};

/// A cost's parts paid after its mana (`DecisionKind::CostPart`: `NUMBER` +
/// the part). Their order here is the one the payment uses when the order
/// of the parts left can't matter (`DOMINANT`).
pub(crate) mod part {
    pub const SACRIFICE: u8 = 0;
    pub const DISCARD_ALSO: u8 = 1;
    pub const DISCARD: u8 = 2;
    pub const EXILE_HAND: u8 = 3;
    pub const BOUNCE: u8 = 4;
    pub const EXILE_GRAVEYARD: u8 = 5;
    pub const TAP_UNTAPPED: u8 = 6;
    pub const REMOVE_COUNTER_FROM: u8 = 7;
    pub const CREW: u8 = 8;
    pub const REMOVE_COUNTERS: u8 = 9;
    pub const TAP: u8 = 10;
    pub const LIFE: u8 = 11;
    pub const DISCARD_HAND: u8 = 12;
    pub const SAC_SELF: u8 = 13;
    pub const DISCARD_SELF: u8 = 14;
    pub const EXILE_SELF: u8 = 15;
    pub const LOYALTY: u8 = 16;
    /// Emerge's sacrifice of the permanent chosen as the total cost was
    /// determined (CR 702.119a, 601.2f; ENGINE.md 100.4.10).
    pub const EMERGE: u8 = 17;
    /// "Exile <the Equipment granting this ability>" (EoE W4,
    /// `CardDef::exiles_granter`).
    pub const EXILE_GRANTER: u8 = 18;
    /// The number of parts.
    pub const N: u8 = 19;

    /// An order in which each part is paid no later than any part it can
    /// give objects to and no earlier than any part that can take its
    /// objects from it, by the cost rules alone (taps, counters and life
    /// before the permanents leave; a permanent returned before the hand's
    /// parts; the graveyard's cards after the parts that put cards there).
    /// With the order of the parts left free (`Game::order_free`), every
    /// outcome of any order is one of this order's.
    pub const DOMINANT: [u8; N as usize] = [
        LOYALTY,
        TAP_UNTAPPED,
        CREW,
        REMOVE_COUNTER_FROM,
        REMOVE_COUNTERS,
        TAP,
        LIFE,
        EXILE_GRANTER,
        BOUNCE,
        SACRIFICE,
        EMERGE,
        SAC_SELF,
        DISCARD_ALSO,
        DISCARD,
        EXILE_HAND,
        DISCARD_HAND,
        DISCARD_SELF,
        EXILE_SELF,
        EXILE_GRAVEYARD,
    ];

    /// The part's name (logs, the bots' and the viewer's words).
    pub fn name(k: u8) -> &'static str {
        match k {
            SACRIFICE => "sacrifice",
            DISCARD_ALSO => "discard (second filter)",
            DISCARD => "discard",
            EXILE_HAND => "exile from hand",
            BOUNCE => "return to hand",
            EXILE_GRAVEYARD => "exile from graveyard",
            TAP_UNTAPPED => "tap untapped permanents",
            REMOVE_COUNTER_FROM => "remove a counter from a permanent",
            CREW => "crew",
            REMOVE_COUNTERS => "remove counters from the source",
            TAP => "{T}",
            LIFE => "pay life",
            DISCARD_HAND => "discard your hand",
            SAC_SELF => "sacrifice the source",
            DISCARD_SELF => "discard the source",
            EXILE_SELF => "exile the source",
            LOYALTY => "loyalty",
            EMERGE => "sacrifice for emerge",
            EXILE_GRANTER => "exile the Equipment",
            _ => "?",
        }
    }
}

/// A cost whose parts the player orders (`Task::PayCosts::order`, ENGINE.md
/// 100.4.9): what it is and how far its payment is.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Order {
    pub p: PlayerId,
    /// The card whose cost it is (the ability's source, the spell).
    pub card: CardIdx,
    pub cost: Cost,
    /// "Tap X untapped <permanents>": X.
    pub x: u32,
    /// A loyalty ability's loyalty cost (CR 606.4): counters added (> 0) or
    /// removed (< 0).
    pub loyalty: i32,
    /// Its parts (bits `1 << part::*`).
    pub parts: u32,
    /// The parts paid.
    pub done: u32,
    /// The part whose objects are being chosen.
    pub cur: Option<u8>,
    /// The agenda's length once the payment is done: the floor of the
    /// search that decides which options complete it.
    pub floor: u32,
    /// Its event group is open (CR 603.2c: the cost is one event; once, so a
    /// refresh doesn't open another).
    pub started: bool,
    /// Emerge's permanent (`part::EMERGE`).
    pub emerge: Option<CardIdx>,
    /// The Equipment the cost exiles (`part::EXILE_GRANTER`).
    pub granter: Option<CardIdx>,
}

impl Order {
    pub(crate) fn new(p: PlayerId, card: CardIdx, cost: &Cost, x: u32, loyalty: i32, floor: u32) -> Order {
        Order { p, card, cost: cost.clone(), x, loyalty, parts: parts_of(cost, x, loyalty), done: 0, cur: None, floor, started: false, emerge: None, granter: None }
    }

    /// With emerge's sacrifice of `emerge` and the exile of the Equipment
    /// `granter` among its parts (ENGINE.md 100.4.10).
    pub(crate) fn with_moves(mut self, emerge: Option<CardIdx>, granter: Option<CardIdx>) -> Order {
        self.emerge = emerge;
        self.granter = granter;
        if emerge.is_some() {
            self.parts |= 1 << part::EMERGE;
        }
        if granter.is_some() {
            self.parts |= 1 << part::EXILE_GRANTER;
        }
        self
    }

    fn left(&self) -> u32 {
        self.parts & !self.done
    }
}

/// The parts of `cost` (bits `1 << part::*`), "tap X" with `x`, a loyalty
/// cost `loyalty`.
pub(crate) fn parts_of(cost: &Cost, x: u32, loyalty: i32) -> u32 {
    use part::*;
    let mut m = 0u32;
    let mut add = |on: bool, k: u8| {
        if on {
            m |= 1 << k;
        }
    };
    add(cost.sacrifice.as_ref().is_some_and(|s| s.n > 0), SACRIFICE);
    add(cost.discard_also.as_ref().is_some_and(|d| d.0 > 0), DISCARD_ALSO);
    add(cost.discard > 0, DISCARD);
    add(cost.exile_from_hand.is_some(), EXILE_HAND);
    add(cost.bounce.as_ref().is_some_and(|b| b.n > 0), BOUNCE);
    add(cost.exile_graveyard > 0, EXILE_GRAVEYARD);
    add(cost.tap_untapped.as_ref().is_some_and(|t| t.n > 0) || (cost.tap_x.is_some() && x > 0), TAP_UNTAPPED);
    add(cost.remove_counter_from.is_some(), REMOVE_COUNTER_FROM);
    add(cost.crew > 0, CREW);
    add(cost.remove_counters.is_some_and(|(_, n)| n > 0), REMOVE_COUNTERS);
    add(cost.tap, TAP);
    add(cost.life > 0, LIFE);
    add(cost.discard_hand, DISCARD_HAND);
    add(cost.sac_self, SAC_SELF);
    add(cost.discard_self, DISCARD_SELF);
    add(cost.exile_self, EXILE_SELF);
    add(loyalty != 0, LOYALTY);
    m
}

fn bits(m: u32) -> impl Iterator<Item = u8> {
    (0..part::N).filter(move |&k| m & (1 << k) != 0)
}

/// The kinds of characteristic a part reads or a continuous effect changes
/// (`Reads::kinds`, `Change`): types (card types, subtypes, supertypes),
/// colours, abilities (keywords, protection, activated abilities), power and
/// toughness; and `DEST`, where an object the part moves goes (the
/// replacement effects that apply to it, CR 614.1a, 614.6: "exile it
/// instead", a void counter; ENGINE.md 100.4.10).
mod kind {
    pub const TYPES: u8 = 1;
    pub const COLORS: u8 = 2;
    pub const ABILITIES: u8 = 4;
    pub const PT: u8 = 8;
    pub const DEST: u8 = 16;
    pub const ALL: u8 = 31;
}

/// What a part of a cost reads of its candidates that paying another part
/// can change: their characteristics (`kinds`: what continuous effects
/// change), whether they are tapped, their counters, the number of some
/// permanents; `hand`: the candidates are cards in a hand (where only
/// Painter's Servant and characteristic-defining abilities change
/// characteristics, CR 604.3, 611.3: a static ability of a permanent affects
/// permanents).
#[derive(Clone, Copy, Default, Debug)]
struct Reads {
    kinds: u8,
    tapped: bool,
    counters: bool,
    counts: bool,
    hand: bool,
}

impl Reads {
    fn or(self, o: Reads) -> Reads {
        Reads {
            kinds: self.kinds | o.kinds,
            tapped: self.tapped || o.tapped,
            counters: self.counters || o.counters,
            counts: self.counts || o.counts,
            hand: self.hand || o.hand,
        }
    }

    fn any(self) -> bool {
        self.kinds != 0 || self.tapped || self.counters || self.counts
    }
}

/// Which kinds of characteristic a change may change
/// (`Game::chars_may_change`).
#[derive(Clone, Copy, Default, Debug)]
struct Change(u8);

impl Change {
    fn hits(self, r: Reads) -> bool {
        self.0 & r.kinds != 0
    }
}

/// What a static ability's or an effect's modification changes (exhaustive:
/// a new modification says). Rules effects ("can't block") change nothing a
/// cost reads.
fn mod_change(m: &Mod) -> u8 {
    use kind::*;
    match m {
        Mod::SetPt(..) | Mod::Pt(..) | Mod::PtPer { .. } | Mod::PtPerSourceCounter { .. } | Mod::SwitchPt | Mod::PowerPlusGreatestMv { .. } => PT,
        Mod::Types { .. } | Mod::AllCreatureTypes | Mod::BecomeTypes { .. } => TYPES,
        Mod::Colors(_) => COLORS,
        Mod::Keywords(_) | Mod::Protection(_) | Mod::GainSacMana(_) | Mod::GainLatent(_) => ABILITIES,
        // (Where it goes if it would die or leave, CR 614.1a.)
        Mod::ExileIfDies | Mod::ExileIfLeaves => DEST,
        // (Losing all abilities loses static abilities too; a change of
        // control changes what "you control" reads.)
        Mod::LoseAllAbilities | Mod::ControlFrom(_) => ALL,
        Mod::CantBlock
        | Mod::CantBeBlocked
        | Mod::MustAttack
        | Mod::AttackDespiteDefender
        | Mod::Goad
        | Mod::PreventCombatDamage
        | Mod::MustAttackPlayer => 0,
    }
}

/// What a filter reads (exhaustive: a new filter says).
fn filter_reads(f: &Filter) -> Reads {
    let k = |kinds: u8| Reads { kinds, ..Reads::default() };
    match f {
        Filter::And(a, b) => filter_reads(a).or(filter_reads(b)),
        Filter::Or(v) => v.iter().fold(Reads::default(), |r, x| r.or(filter_reads(x))),
        Filter::Not(x) | Filter::Spell(x) | Filter::InGraveyard(x) | Filter::InExile(x) | Filter::TargetsMatching(x) => filter_reads(x),
        Filter::AttachedTo(x) => filter_reads(x).or(k(kind::ALL)),
        Filter::Type(_) | Filter::Subtype(_) | Filter::Supertype(_) => k(kind::TYPES),
        Filter::Color(_) | Filter::Colorless => k(kind::COLORS),
        Filter::HasKeyword(_) | Filter::HasActivatedAbility | Filter::HasWarp => k(kind::ABILITIES),
        Filter::TotalPtAtMost(_) | Filter::PowerAtLeast(_) | Filter::PowerAtMost(_) | Filter::ToughnessAtMost(_) | Filter::PtIs(..) => k(kind::PT),
        Filter::Tapped => Reads { tapped: true, ..Reads::default() },
        Filter::HasCounter(_) | Filter::NoCounters(_) | Filter::MvIsSourceCounters(_) => Reads { counters: true, ..Reads::default() },
        Filter::MvAtMostLandsYouControl | Filter::OathTarget => Reads { counts: true, ..Reads::default() },
        // (Facts no part of a cost changes: who controls or owns it, what it
        // is printed with or was cast for, combat, this turn's history.)
        Filter::Player
        | Filter::Opponent
        | Filter::Controller(_)
        | Filter::Owner(_)
        | Filter::Source
        | Filter::Any
        | Filter::Token
        | Filter::Attacking
        | Filter::Blocking
        | Filter::Unblocked
        | Filter::DrewThisTurn(_)
        | Filter::FirstDrawInDrawStep
        | Filter::Evoked
        | Filter::ManaValueAtMost(_)
        | Filter::ManaValueAtLeast(_)
        | Filter::MvAtMostConverge
        | Filter::MvAtMostX
        | Filter::MvAtMostSourceX
        | Filter::MvAtMostManaSpent
        | Filter::ManaCostIn(_)
        | Filter::Named(_)
        | Filter::Ability(_)
        | Filter::Harnessed
        | Filter::Saddled
        | Filter::DealtDamageThisTurn
        | Filter::CastForWarp
        | Filter::HostOfSource => Reads::default(),
    }
}

/// What paying parts of a cost may change (`Game::order_free`), or the mana
/// payment before them (`Game::mana_writes`).
#[derive(Clone, Default, Debug)]
pub(crate) struct Writes {
    /// Permanents that may leave the battlefield.
    leave: Vec<CardIdx>,
    /// The contents of a hand, graveyard or exile (cards moved).
    zones: bool,
    life: bool,
    /// Permanents that may lose counters (or, a loyalty cost's source, get
    /// some).
    counters: Vec<CardIdx>,
    /// Permanents that may become tapped.
    tapped: Vec<CardIdx>,
}

impl Writes {
    fn add(&mut self, o: &Writes) {
        self.leave.extend_from_slice(&o.leave);
        self.zones |= o.zones;
        self.life |= o.life;
        self.counters.extend_from_slice(&o.counters);
        self.tapped.extend_from_slice(&o.tapped);
    }

    fn is_empty(&self) -> bool {
        self.leave.is_empty() && !self.zones && !self.life && self.counters.is_empty() && self.tapped.is_empty()
    }
}

impl Game {
    /// What part `k` of `o`'s cost reads of the objects it may choose
    /// (`Reads`) and may change (`Writes`), from the state as it is.
    fn part_view(&self, o: &Order, k: u8) -> (Reads, Writes) {
        use part::*;
        let (p, card, cost) = (o.p, o.card, &o.cost);
        let mut w = Writes::default();
        let none = Reads::default();
        // A part that moves permanents reads the replacement effects that
        // say where they go (CR 614.1a: "exile it instead", a void counter;
        // `Game::replaced_dest`). (A card of the payer's hand goes where the
        // payer's cards go: the effects that apply to those are its own
        // card's, the turn's or an opponent's permanents', which no part of
        // the payer's cost changes.)
        let dest = Reads { kinds: kind::DEST, ..Reads::default() };
        let r = match k {
            SACRIFICE | BOUNCE => {
                let s = if k == SACRIFICE { cost.sacrifice.as_ref() } else { cost.bounce.as_ref() }.expect("the part's cost");
                w.leave = self.sacrifice_candidates(p, card, s);
                w.zones = true;
                // (And where they go: CR 614.1a.)
                filter_reads(&s.filter).or(dest)
            }
            SAC_SELF => {
                w.leave = vec![card];
                w.zones = true;
                dest
            }
            EMERGE => {
                w.leave = vec![o.emerge.expect("emerge's permanent")];
                w.zones = true;
                dest
            }
            EXILE_GRANTER => {
                // (To exile: no replacement applies, CR 614.1a.)
                w.leave = vec![o.granter.expect("the Equipment")];
                w.zones = true;
                none
            }
            EXILE_SELF => {
                if self.objs[card.i()].zone == Zone::Battlefield {
                    w.leave = vec![card];
                }
                w.zones = true;
                none
            }
            DISCARD_SELF | DISCARD_HAND | EXILE_GRAVEYARD => {
                // ("Exile n cards from your graveyard" reads their printed
                // types: a card in a graveyard has its printed
                // characteristics, CR 400.7; `gy_cost_card`.)
                w.zones = true;
                none
            }
            DISCARD | DISCARD_ALSO | EXILE_HAND => {
                w.zones = true;
                let f = match k {
                    DISCARD => cost.discard_filter.as_ref(),
                    DISCARD_ALSO => cost.discard_also.as_ref().map(|d| &d.1),
                    _ => cost.exile_from_hand.as_ref(),
                };
                Reads { hand: true, ..f.map_or(none, filter_reads) }
            }
            TAP_UNTAPPED => {
                let t = match (&cost.tap_untapped, &cost.tap_x) {
                    (Some(t), _) => t.clone(),
                    (None, Some(f)) => crate::db::SacCost { filter: f.clone(), n: 0, another: true },
                    (None, None) => unreachable!("taps owed without a tap cost"),
                };
                w.tapped = self.tap_candidates(p, card, &t).collect();
                filter_reads(&t.filter)
            }
            CREW => {
                // (Creatures, their power and "crews as though its power were
                // greater".)
                w.tapped = self.crew_eligible(p, card);
                Reads { kinds: kind::TYPES | kind::PT | kind::ABILITIES, ..Reads::default() }
            }
            REMOVE_COUNTER_FROM => {
                let (f, kind) = cost.remove_counter_from.as_ref().expect("the part's cost");
                w.counters = self.remove_counter_candidates(p, card, f, *kind);
                filter_reads(f).or(Reads { counters: true, ..Reads::default() })
            }
            REMOVE_COUNTERS | LOYALTY => {
                // (Its own counters, read by no other part but those that
                // remove some too: the counts add, CR 118.3.)
                w.counters = vec![card];
                none
            }
            TAP => {
                // ({T} is paid with the source untapped; summoning sickness
                // is a restriction on activating, checked as it begins, CR
                // 602.5a.)
                w.tapped = vec![card];
                none
            }
            LIFE => {
                w.life = true;
                none
            }
            _ => unreachable!("cost part {k}"),
        };
        (r, w)
    }

    /// CR 601.2h: can the order in which the parts of `o`'s cost still to be
    /// paid are paid change what any of them may take? `true` (the order is
    /// free: the payment needn't ask it) only when, by the rules, in this
    /// state: no part gives another objects the engine's order pays first
    /// (a permanent returned to hand, which the hand's parts could then take;
    /// "discard your hand" and the graveyard's cards; a loyalty cost's
    /// counters), and no part changes anything another part reads but by
    /// taking objects from it: no continuous effect (a static ability, an
    /// effect, a characteristic-defining ability, counters, Painter's Servant,
    /// a Blood Moon) whose source or input a part changes (`chars_may_change`)
    /// while another reads characteristics; no part taps or removes counters
    /// while another reads tapped permanents or counters. Then each part's
    /// candidates are the same in every order but for the objects other parts
    /// take, and the engine's order (taps and counters first, then what leaves
    /// the battlefield, the hand, the graveyard: `pay_costs`, `part::DOMINANT`)
    /// is one in which any part's objects could have been taken: every
    /// outcome of any order is one of its own (ENGINE.md 100.4.9). (Each part's
    /// writes are its candidates' now: by induction over the payment, while no
    /// part's payment changes what another reads, every part's candidates are
    /// its candidates now, less the objects taken.)
    pub(crate) fn order_free(&self, o: &Order) -> bool {
        self.order_free_with(o, Writes::default)
    }

    /// [`order_free`](Self::order_free) for a payment whose mana comes first
    /// (CR 601.2g before 601.2h): `pre`, what the mana payment may change
    /// (`mana_writes`), is paid before every part, so a part that reads what
    /// it changes (a painland's damage and a creature's power read by crew,
    /// through a static ability) can't be decided from the state before the
    /// mana: castability and the payment then search (ENGINE.md 100.4.10).
    /// `pre` is computed only when some part reads something.
    pub(crate) fn order_free_with(&self, o: &Order, pre: impl FnOnce() -> Writes) -> bool {
        use part::*;
        let left = o.left();
        if left == 0 {
            return true;
        }
        let views: Vec<(u8, Reads, Writes)> = bits(left).map(|k| {
            let (r, w) = self.part_view(o, k);
            (k, r, w)
        }).collect();
        let has = |k: u8| left & (1 << k) != 0;
        // (Emerge's permanent and the Equipment are one object each, which
        // the engine's order moves last and first: `pre` matters to them.)
        let moves = has(EMERGE) || has(EXILE_GRANTER);
        if !views.iter().any(|(_, r, _)| r.any()) && left.count_ones() < 2 && !moves {
            return true;
        }
        let pre = if views.iter().any(|(_, r, _)| r.any()) || moves { pre() } else { Writes::default() };
        // Emerge's sacrifice (paid last by the engine): no other part nor
        // the mana may take its permanent first, and the graveyard's cards
        // come after it (it gives them one).
        if let (true, Some(e)) = (has(EMERGE), o.emerge) {
            let takes = views.iter().any(|(k, _, w)| *k != EMERGE && w.leave.contains(&e));
            if takes || pre.leave.contains(&e) || has(EXILE_GRAVEYARD) {
                return false;
            }
        }
        // The Equipment's exile (paid first by the engine): no part may tap
        // it, remove a counter from it or take it first, nor the mana move it.
        if let (true, Some(g)) = (has(EXILE_GRANTER), o.granter) {
            let uses = views.iter().any(|(k, _, w)| *k != EXILE_GRANTER && (w.leave.contains(&g) || w.tapped.contains(&g) || w.counters.contains(&g)));
            if uses || pre.leave.contains(&g) {
                return false;
            }
        }
        if left.count_ones() < 2 && pre.is_empty() {
            return true;
        }
        // A permanent returned to its owner's hand may then be discarded or
        // exiled from it; "discard your hand" before or after it differ.
        if has(BOUNCE) && (has(DISCARD) || has(DISCARD_ALSO) || has(EXILE_HAND) || has(DISCARD_HAND)) {
            return false;
        }
        // The engine's order discards the hand after the graveyard's cards
        // are chosen.
        if has(EXILE_GRAVEYARD) && has(DISCARD_HAND) {
            return false;
        }
        // A loyalty cost that adds counters, then a part that removes them.
        if has(LOYALTY) && o.loyalty > 0 && (has(REMOVE_COUNTERS) || has(REMOVE_COUNTER_FROM)) {
            return false;
        }
        for (k, r, _) in &views {
            if !r.any() {
                continue;
            }
            let mut w = pre.clone();
            for (j, _, wj) in &views {
                if j != k {
                    w.add(wj);
                }
            }
            if (r.tapped && !w.tapped.is_empty()) || (r.counters && !w.counters.is_empty()) || (r.counts && !w.leave.is_empty()) {
                return false;
            }
            if r.kinds != 0 && self.chars_may_change(o.p, &w, r.hand).hits(*r) {
                return false;
            }
        }
        true
    }

    /// What the mana payment `pm` may change before the cost's other parts
    /// are paid (CR 601.2g's mana abilities, then the mana with its own
    /// alternatives, which the engine pays first: ENGINE.md 100.4.9), over
    /// every source it may use (`Game::pay_classes`): the permanents its
    /// sources and their actors tap (improvise, convoke and waterbend too),
    /// those they sacrifice, the counters they remove, the hand's cards they
    /// exile or discard, life (a cost of life, a painland's damage, an
    /// opponent's life gained, Phyrexian pips, life paid after the mana);
    /// with `delve`, the graveyard's cards it exiles (ENGINE.md 100.4.10).
    pub(crate) fn mana_writes(&self, pm: &crate::pay::PayMana, delve: bool) -> Writes {
        use crate::pay::{is_sac_slot, IMPROVISE, POL_FOR, SQ_FOR, TAP_FOR};
        let mut w = Writes { zones: delve, life: pm.cost.has_phyrexian() || pm.life_after > 0, ..Writes::default() };
        for cl in self.pay_classes(pm.payer, pm.keep.exclude, pm.improvise_types(), pm.convoke) {
            if cl.dmg > 0 {
                w.life = true;
            }
            if cl.sac_self || cl.convert {
                w.leave.extend(cl.members.iter().map(|s| s.card));
                w.zones = true;
            }
            for s in cl.members.iter() {
                let c = s.card;
                if s.slot == IMPROVISE || s.slot == TAP_FOR || s.slot == POL_FOR {
                    w.tapped.push(c);
                } else if is_sac_slot(s.slot) {
                    w.leave.push(c);
                    w.zones = true;
                }
                if s.slot != IMPROVISE && s.slot != TAP_FOR && s.slot != POL_FOR && !is_sac_slot(s.slot) {
                    let ad = self.src_ability(c, s.slot);
                    let cost = &ad.cost;
                    if cost.tap {
                        w.tapped.push(c);
                    }
                    if cost.sac_self || (cost.exile_self && self.objs[c.i()].zone == Zone::Battlefield) {
                        w.leave.push(c);
                    }
                    if cost.sac_self || cost.exile_self || cost.discard_self || cost.discard_hand || self.objs[c.i()].zone != Zone::Battlefield {
                        w.zones = true;
                    }
                    if cost.life > 0 {
                        w.life = true;
                    }
                    if cost.remove_counters.is_some() {
                        w.counters.push(c);
                    }
                    if let Some(m) = ad.mana() {
                        if m.damage_to_you > 0 || m.opp_life > 0 {
                            w.life = true;
                        }
                        if m.sac_if_no.is_some() {
                            w.leave.push(c);
                            w.zones = true;
                        }
                    }
                }
            }
            // (The actors of shared activations: a Pollinator or a
            // Lumberjack is tapped; Squandered Resources taps nothing.)
            if cl.members.first().is_some_and(|s| s.slot != SQ_FOR) {
                w.tapped.extend(cl.actors.iter().copied());
            }
        }
        w
    }

    /// Which kinds of characteristic could changes `w` change (`Change`,
    /// `kind`: types, colours, abilities, power and toughness)? Every
    /// continuous effect the layers apply (`layers.rs`) is looked at: static
    /// abilities of permanents and emblems (their source leaving, their
    /// condition, the objects they affect, their amounts), effects lasting
    /// while an object remains, characteristic-defining abilities, counters,
    /// Painter's Servant, a Blood Moon, W3's subtype grants and granted
    /// abilities, reconfigure; each by what it changes. `hand`: of cards in a
    /// hand (Painter's Servant and characteristic-defining abilities only):
    /// `p`'s, whose cards the cost may take (no other hidden card is read).
    fn chars_may_change(&self, p: PlayerId, w: &Writes, hand: bool) -> Change {
        let mut out = Change::default();
        if w.is_empty() {
            return out;
        }
        let leaving = |c: CardIdx| w.leave.contains(&c);
        // (What a scope's or a count's filter reads that `w` changes. An
        // object's own types and colours change only through the effects
        // looked at here themselves.)
        let reads_w = |f: &Filter| {
            let r = filter_reads(f);
            (r.tapped && !w.tapped.is_empty()) || (r.counters && !w.counters.is_empty()) || (r.counts && !w.leave.is_empty())
        };
        // Characteristic-defining power and toughness (in every zone, CR
        // 604.3).
        let cda_hit = |c: CardIdx| {
            self.def_of(c).pt_cda.as_ref().is_some_and(|cda| match cda {
                PtCda::GraveyardTypes { .. } | PtCda::OwnedInExile { .. } => w.zones || !w.leave.is_empty(),
                PtCda::PowerCount(_) | PtCda::Domain | PtCda::LandsYouControl => !w.leave.is_empty(),
                PtCda::Leveler { .. } => w.counters.contains(&c),
            })
        };
        let cdas = if hand { self.players[p as usize].hand.iter().any(|&c| cda_hit(c)) } else { self.battlefield.iter().any(|&c| cda_hit(c)) };
        if cdas {
            out.0 |= kind::PT;
        }
        // Painter's Servant: every card's colour (CR 613.1e).
        if w.leave.iter().any(|&c| self.abilities(c).paints) {
            out.0 |= kind::COLORS;
        }
        // A void counter's exile (Dauthi Voidwalker: "if a card would be put
        // into an opponent's graveyard from anywhere, exile it instead with a
        // void counter", `Game::void_counter_for`) leaving: where an
        // opponent's card the payer controls goes.
        // (So does Leyline of the Void's "exile it instead", ENGINE.md 110.3.)
        if w.leave.iter().any(|&c| self.abilities(c).void_exile.is_some() || self.abilities(c).opp_gy_exile) {
            out.0 |= kind::DEST;
        }
        if hand {
            return out;
        }
        // Counters are part of an object's power and toughness and keywords
        // (CR 122.1a-b).
        if !w.counters.is_empty() {
            out.0 |= kind::PT | kind::ABILITIES;
        }
        if self.n_static > 0 {
            for &s in self.battlefield.iter().chain(self.command.iter()) {
                let own = self.db.def(self.objs[s.i()].def);
                for st in self.abilities(s).statics.iter().chain(own.statics.iter()) {
                    let sensitive = (leaving(s) && st.scope != Scope::This)
                        || st.cond.as_ref().is_some_and(|k| self.cond_reads(k, s, w))
                        || matches!(&st.scope, Scope::Matching { filter, .. } if reads_w(filter));
                    for m in &st.mods {
                        let amount = match m {
                            Mod::PtPer { filter, .. } => !w.leave.is_empty() || reads_w(filter),
                            Mod::PtPerSourceCounter { .. } => w.counters.contains(&s),
                            Mod::PowerPlusGreatestMv { .. } => !w.leave.is_empty(),
                            _ => false,
                        };
                        if sensitive || amount {
                            out.0 |= mod_change(m);
                        }
                    }
                }
            }
        }
        for e in &self.effects {
            if let crate::layers::Until::WhileOnBattlefield(r) = e.until {
                if leaving(r.idx) {
                    out.0 |= mod_change(&e.m);
                }
            }
        }
        // (An Equipment's granted abilities; a Blood Moon's land types and
        // abilities; W3's granted subtypes and abilities; a reconfigure
        // creature unattached.)
        if !w.leave.is_empty() {
            if w.leave.iter().any(|&c| self.abilities(c).grants_equipped.is_some()) {
                out.0 |= kind::ABILITIES;
            }
            if self.moon_game {
                out.0 |= kind::TYPES | kind::ABILITIES;
            }
            if self.w3.seen & crate::eoe_w3::SEEN_W3_SUBTYPE != 0 {
                out.0 |= kind::TYPES;
            }
            if self.w3.seen & crate::eoe_w3::SEEN_W3_GRANT_ACT != 0 {
                out.0 |= kind::ABILITIES;
            }
            if self.battlefield.iter().any(|&c| self.def_of(c).reconfigure && self.objs[c.i()].attached.is_some_and(|h| leaving(h.idx))) {
                out.0 |= kind::ALL;
            }
        }
        out
    }

    /// Does condition `k` of a static ability of `s` read what `w` changes
    /// (exhaustive: a new condition says)?
    fn cond_reads(&self, k: &Cond, s: CardIdx, w: &Writes) -> bool {
        let leave = !w.leave.is_empty();
        match k {
            Cond::ControlAtLeast(f, _) | Cond::ControlAtMost(f, _) | Cond::OpponentsControlAtLeast(f, _) => {
                let r = filter_reads(f);
                leave || (r.tapped && !w.tapped.is_empty()) || (r.counters && !w.counters.is_empty())
            }
            Cond::APlayerControlsNoCreatures
            | Cond::ControlOtherOfColor(_)
            | Cond::OpponentControlsMoreLands
            | Cond::DevotionCoversLibrary(_)
            | Cond::SourceAttached
            | Cond::Revolt
            | Cond::Void => leave,
            Cond::HandExactly(_)
            | Cond::HandAtMost(_)
            | Cond::OpponentHasMoreCards
            | Cond::GraveyardAtLeast(_)
            | Cond::Delirium(_)
            | Cond::LinkedTypesAtLeast(_) => w.zones || leave,
            Cond::SourceCountersIn(..) => w.counters.contains(&s),
            Cond::LifeAtMostHalfStart => w.life,
            Cond::All(v) => v.iter().any(|x| self.cond_reads(x, s, w)),
            // (A turn's or the game's history no payment writes: spells are
            // counted as they become cast, after the costs, CR 601.2i.)
            Cond::YourTurnAtMost(_)
            | Cond::FirstSpellThisGame
            | Cond::IsMonarch
            | Cond::GainedLifeThisTurn
            | Cond::ArtifactEnteredThisTurn
            | Cond::GainedLifeAtLeast(_)
            | Cond::SpellsCastThisTurn { .. }
            | Cond::LibraryEmpty
            | Cond::YourTurn
            | Cond::SpellsCastAtLeast(_)
            | Cond::HasInitiative
            | Cond::CompletedDungeon
            | Cond::CityBlessing
            | Cond::LandsPlayedAtLeast(_)
            | Cond::AttackedWithSubtype(_) => false,
        }
    }

    /// The first part of `parts` in the order `part::DOMINANT`.
    fn dominant_first(parts: u32) -> u8 {
        part::DOMINANT.iter().copied().find(|&k| parts & (1 << k) != 0).expect("a part left")
    }

    // ---- the payment, the order the player's (ENGINE.md 100.4.9) ------------

    /// `pay_costs` for a cost whose parts the player orders (`o`): asks which
    /// part is paid next while the order of the parts left can matter
    /// (`DecisionKind::CostPart`, `NUMBER` + the part), else takes them in
    /// `part::DOMINANT`'s order; asks each part's objects from the state as
    /// it is, one per decision (`picked`), and pays the part once they are
    /// chosen. The options are those that leave the payment completable
    /// (`Game::ask`'s search, `payment_completes`).
    pub(crate) fn pay_order(&mut self, id: u32, mut picked: Vec<CardIdx>, mut o: Box<Order>) {
        if !o.started {
            o.started = true;
            // The cost is one event (CR 603.2c): a group of its own.
            self.new_event();
            // ("Exile <the Equipment granting this ability>" and emerge's
            // sacrifice are parts of it: ENGINE.md 100.4.10.)
        }
        if let Some(ch) = self.answer.take() {
            match o.cur {
                None => match ch {
                    Choice::Number(k) => o.cur = Some(k),
                    other => unreachable!("cost part answer {other:?}"),
                },
                Some(k) => {
                    if let Some(slot) = self.order_answer(&o, k, &mut picked, ch) {
                        // A mana ability of the permanent picked first (CR
                        // 601.2g before 601.2h; ENGINE.md 86.2): its mana to
                        // the pool, then the rest of the part.
                        let c = *picked.last().expect("the permanent picked");
                        let p = o.p;
                        self.agenda.push(Task::PayCosts { id, picked, early: None, order: Some(o) });
                        self.begin_activate(p, c, slot);
                        return;
                    }
                }
            }
        }
        loop {
            if let Some(k) = o.cur {
                match self.order_part(id, &o, k, &mut picked) {
                    Some(opts) => {
                        if opts.is_empty() {
                            // (The search's copies only: a part no object can
                            // pay; the real payment offers only completable
                            // options.)
                            self.oracle_dead(&o);
                            return;
                        }
                        let kind = Self::part_kind(k);
                        let p = o.p;
                        self.ask(Task::PayCosts { id, picked, early: None, order: Some(o) }, p, kind, opts);
                        return;
                    }
                    None => {
                        o.done |= 1 << k;
                        o.cur = None;
                        picked.clear();
                    }
                }
            }
            let left = o.left();
            if left == 0 {
                // The cards promised to delve, discarded: exiled (CR 601.2h,
                // 702.66a).
                self.exile_promised(o.p, Some(o.card));
                return;
            }
            if self.order_free(&o) {
                o.cur = Some(Self::dominant_first(left));
                continue;
            }
            let opts: Vec<Opt> = bits(left).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k) }).collect();
            let p = o.p;
            self.ask(Task::PayCosts { id, picked, early: None, order: Some(o) }, p, DecisionKind::CostPart, opts);
            return;
        }
    }

    /// The decision kind of part `k`'s objects.
    fn part_kind(k: u8) -> DecisionKind {
        use part::*;
        match k {
            SACRIFICE => DecisionKind::Sacrifice,
            BOUNCE => DecisionKind::BounceCost,
            DISCARD | DISCARD_ALSO => DecisionKind::DiscardCost,
            EXILE_HAND => DecisionKind::ExileCost,
            EXILE_GRAVEYARD => DecisionKind::ChooseGraveyardCard,
            _ => DecisionKind::ChoosePermanent,
        }
    }

    /// An answer for part `k`'s objects: noted in `picked` (a card discarded
    /// or exiled from hand at once, as `pay_costs` does: never left hidden
    /// while another decision waits). A permanent picked with its mana
    /// ability first returns that ability.
    fn order_answer(&mut self, o: &Order, k: u8, picked: &mut Vec<CardIdx>, ch: Choice) -> Option<crate::db::AbilitySlot> {
        use part::*;
        match ch {
            Choice::Card(c) => {
                // (A card of the definition promised to delve, if any.)
                let c = if matches!(k, DISCARD | DISCARD_ALSO) { self.promised_twin(o.p, c) } else { c };
                match k {
                    DISCARD | DISCARD_ALSO => self.discard(c),
                    EXILE_HAND => {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                    _ => {}
                }
                picked.push(c);
                None
            }
            Choice::Ability(c, slot) => {
                debug_assert!(k == SACRIFICE || k == BOUNCE, "a mana ability first for part {k}");
                picked.push(c);
                Some(slot)
            }
            // CR 702.122a: enough power tapped for crew.
            Choice::Pass => {
                debug_assert_eq!(k, CREW);
                let _ = o;
                picked.push(CREW_DONE);
                None
            }
            other => unreachable!("cost part {k} answer {other:?}"),
        }
    }

    /// Part `k` of `o`'s cost with `picked` chosen: the options of its next
    /// object (`Some`; empty when none can be chosen), or `None` once it is
    /// paid (now, with what was chosen).
    fn order_part(&mut self, id: u32, o: &Order, k: u8, picked: &mut Vec<CardIdx>) -> Option<Vec<Opt>> {
        use part::*;
        let (p, card) = (o.p, o.card);
        let cost = &o.cost;
        // (The life the cost still takes after a mana ability of a permanent
        // picked first: its own life, if not paid yet.)
        let life_after = if o.done & (1 << LIFE) == 0 { cost.life } else { 0 };
        match k {
            SACRIFICE | BOUNCE => {
                let s = if k == SACRIFICE { cost.sacrifice.as_ref() } else { cost.bounce.as_ref() }.expect("the part's cost");
                if picked.len() >= s.n as usize {
                    if k == SACRIFICE {
                        self.sacrifice_picked(p, id, picked);
                    } else {
                        self.return_picked(p, id, picked);
                    }
                    return None;
                }
                // (A set: increasing slot order.)
                let floor = picked.last().map(|&c| self.perm_slot(c));
                let cands: Vec<CardIdx> = self
                    .sacrifice_candidates(p, card, s)
                    .into_iter()
                    .filter(|&c| !picked.contains(&c) && floor.is_none_or(|f| self.perm_slot(c) > f))
                    .collect();
                Some(self.cost_pick_options(p, &cands, life_after))
            }
            DISCARD | DISCARD_ALSO => {
                let (n, f) = if k == DISCARD { (cost.discard, cost.discard_filter.as_ref()) } else { cost.discard_also.as_ref().map(|d| (d.0, Some(&d.1))).expect("the part's cost") };
                if picked.len() >= n as usize {
                    return None;
                }
                let mut opts = self.hand_options_except(p, Some(card));
                if let Some(f) = f {
                    let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
                    opts.retain(|x| matches!(x.choice, Choice::Card(c) if self.card_matches(f, c, &ctx)));
                }
                // (The cards promised to delve are the discards': each pick
                // leaves the discards after it those, ENGINE.md 100.4.9.)
                let other = |j: u8, m: usize| if o.done & (1 << j) == 0 { m } else { 0 };
                let left = if k == DISCARD_ALSO {
                    (n as usize - picked.len() - 1, other(DISCARD, cost.discard as usize))
                } else {
                    (other(DISCARD_ALSO, cost.discard_also.as_ref().map_or(0, |d| d.0 as usize)), n as usize - picked.len() - 1)
                };
                opts.retain(|x| matches!(x.choice, Choice::Card(c) if self.promises_kept(p, card, cost, self.promised_twin(p, c), left)));
                Some(opts)
            }
            EXILE_HAND => {
                if !picked.is_empty() {
                    return None;
                }
                let f = cost.exile_from_hand.as_ref().expect("the part's cost");
                Some(self.exile_candidates(p, card, f))
            }
            EXILE_GRAVEYARD => {
                let n = cost.exile_graveyard as usize;
                if picked.len() >= n {
                    for &c in picked.iter() {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                    return None;
                }
                let gy: Vec<CardIdx> = self.graveyard_cards(p).filter(|&c| c != card && self.gy_cost_card(cost, c)).collect();
                let after = picked.last().map_or(0, |c| gy.iter().position(|x| x == c).expect("a card picked") + 1);
                let still = n - picked.len();
                Some(
                    gy.iter()
                        .enumerate()
                        .filter(|&(i, _)| i >= after && gy.len() - i >= still)
                        .map(|(_, &c)| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) })
                        .collect(),
                )
            }
            TAP_UNTAPPED => {
                let n = cost.tap_untapped.as_ref().map_or(o.x as usize, |t| t.n as usize);
                if picked.len() >= n {
                    self.pay_tap_parts(id, card, cost, picked, &[], None);
                    return None;
                }
                let tx;
                let t = match (&cost.tap_untapped, &cost.tap_x) {
                    (Some(t), _) => t,
                    (None, Some(f)) => {
                        tx = crate::db::SacCost { filter: f.clone(), n: 0, another: true };
                        &tx
                    }
                    (None, None) => unreachable!("taps owed without a tap cost"),
                };
                let floor = picked.last().map(|&c| self.perm_slot(c));
                let cands: Vec<CardIdx> =
                    self.tap_candidates(p, card, t).filter(|&c| !picked.contains(&c) && floor.is_none_or(|f| self.perm_slot(c) > f)).collect();
                let still = n - picked.len();
                Some(
                    cands
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| cands.len() - i >= still)
                        .map(|(_, &c)| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                        .collect(),
                )
            }
            REMOVE_COUNTER_FROM => {
                let (f, kind) = cost.remove_counter_from.as_ref().expect("the part's cost");
                if let Some(&c) = picked.first() {
                    self.remove_counters(c, *kind, 1);
                    return None;
                }
                Some(
                    self.remove_counter_candidates(p, card, f, *kind)
                        .into_iter()
                        .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                        .collect(),
                )
            }
            CREW => {
                let bonus = !cost.saddle && !self.is_spell_card(card);
                if picked.last() == Some(&CREW_DONE) {
                    picked.pop();
                    let crew = picked.clone();
                    self.pay_tap_parts(id, card, cost, &[], &crew, None);
                    return None;
                }
                // (CR 702.122a: untapped creatures other than the source, one
                // decision each in increasing slot order; `PASS` once their
                // total power is enough, read as it is now.)
                let need = cost.crew as crate::num::Wide;
                let have: crate::num::Wide = picked.iter().map(|&c| self.crew_power_of(c, bonus)).sum();
                let floor = picked.last().map(|&c| self.perm_slot(c));
                let mut opts: Vec<Opt> = self
                    .crew_eligible(p, card)
                    .into_iter()
                    .filter(|&c| !picked.contains(&c) && floor.is_none_or(|f| self.perm_slot(c) > f))
                    .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                    .collect();
                if have >= need {
                    opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                }
                Some(opts)
            }
            // The parts with nothing to choose: paid now, if they can be
            // (CR 601.2h: unpayable costs can't be paid).
            _ => {
                if self.pay_whole_part(id, o, k) {
                    None
                } else {
                    Some(Vec::new())
                }
            }
        }
    }

    /// Pay part `k` of `o`'s cost, one with no objects to choose, if it can
    /// be paid now.
    fn pay_whole_part(&mut self, id: u32, o: &Order, k: u8) -> bool {
        use part::*;
        let (p, card) = (o.p, o.card);
        let cost = &o.cost;
        let zone = self.objs[card.i()].zone;
        match k {
            TAP => {
                if zone != Zone::Battlefield || self.objs[card.i()].tapped {
                    return false;
                }
                self.tap(card);
            }
            LIFE => {
                // CR 119.4: no more than one's life total.
                if cost.life as i64 > self.players[p as usize].life {
                    return false;
                }
                self.lose_life(p, i32::try_from(cost.life).expect("life within the life total"));
            }
            REMOVE_COUNTERS => {
                let (kind, n) = cost.remove_counters.expect("the part's cost");
                if zone != Zone::Battlefield || self.counters_on(card, kind) < n as crate::num::Count {
                    return false;
                }
                self.remove_counters(card, kind, n as crate::num::Count);
            }
            DISCARD_HAND => {
                for h in self.hand_sorted(p) {
                    if h != card {
                        self.discard(h);
                    }
                }
            }
            SAC_SELF => {
                if zone != Zone::Battlefield || self.objs[card.i()].controller != p {
                    return false;
                }
                self.sacrifice(card);
            }
            DISCARD_SELF => {
                if zone != Zone::Hand {
                    return false;
                }
                self.discard(card);
            }
            EXILE_SELF => {
                if !matches!(zone, Zone::Hand | Zone::Graveyard | Zone::Battlefield) {
                    return false;
                }
                self.move_card(card, Zone::Exile, Pos::Top);
            }
            EMERGE => {
                // CR 702.119a: the permanent chosen, if it is still there.
                let Some(e) = o.emerge.filter(|&e| self.objs[e.i()].zone == Zone::Battlefield && self.objs[e.i()].controller == p) else { return false };
                self.note(|g| format!("P{p} sacrifices {} for emerge", g.name(e)));
                self.sacrifice(e);
            }
            EXILE_GRANTER => {
                let Some(e) = o.granter.filter(|&e| self.objs[e.i()].zone == Zone::Battlefield) else { return false };
                self.note(|g| format!("P{p} exiles {} to pay", g.name(e)));
                self.move_card(e, Zone::Exile, Pos::Top);
            }
            LOYALTY => {
                // CR 606.4.
                let d = o.loyalty;
                if zone != Zone::Battlefield {
                    return false;
                }
                if d > 0 {
                    self.add_counters(card, crate::db::Counter::Loyalty, d as crate::num::Count);
                } else {
                    let n = (-(d as i64)) as crate::num::Count;
                    if self.counters_on(card, crate::db::Counter::Loyalty) < n {
                        return false;
                    }
                    self.remove_counters(card, crate::db::Counter::Loyalty, n);
                }
            }
            _ => unreachable!("cost part {k} has objects to choose"),
        }
        let _ = id;
        true
    }

    /// Return the permanents chosen for a cost to their owner's hand
    /// together; an attacking creature returned for ninjutsu leaves what it
    /// attacked to the creature put onto the battlefield (CR 702.49c).
    pub(crate) fn return_picked(&mut self, p: PlayerId, id: u32, picked: &[CardIdx]) {
        let mut bounced: Vec<CardIdx> = picked.to_vec();
        bounced.sort_by_key(|c| self.objs[c.i()].timestamp);
        if let (Some(&c), Some(pos)) = (bounced.first(), self.item_index(id)) {
            if self.objs[c.i()].attacking {
                self.stack[pos].attack_as = Some(self.objs[c.i()].attacks);
            }
        }
        for c in bounced {
            self.note(|g| format!("P{p} returns {}", g.name(c)));
            self.move_card(c, Zone::Hand, Pos::Top);
        }
    }

    // ---- the search (ENGINE.md 100.4.9) ------------------------------------

    /// In the search's copies (`Game::oracle`): a payment that can't go on
    /// (a part with no object to take) is a dead end.
    fn oracle_dead(&mut self, o: &Order) {
        assert!(self.oracle, "a cost part with no option outside the search: {:?}\n{}", o, self.log_lines().join("\n"));
        self.oracle_dead = true;
    }

    /// Can the payment whose tasks lie on the agenda above length `floor` be
    /// completed (CR 601.2h: a cost is paid whole; an option is offered only
    /// if the payment can still be completed, ENGINE.md 6, 49)? A search of
    /// the payment's own decisions on copies of the game, every option of
    /// each (`Game::oracle`: the copies ask without this check), until a
    /// copy has paid everything. A decision of another player than `payer`
    /// within the payment (616.1's order of replacement effects for a
    /// permanent of theirs sacrificed) must complete for each of its answers.
    pub(crate) fn payment_completes(&self, floor: usize, payer: PlayerId) -> bool {
        let mut g = self.clone();
        g.pending = None;
        g.payment_completes_from(floor, payer)
    }

    fn complete_from(mut g: Game, floor: usize, payer: PlayerId, nodes: &mut u64) -> bool {
        *nodes += 1;
        assert!(*nodes <= ORACLE_NODES, "the payment search passed {ORACLE_NODES} copies (ENGINE.md 100.4.9)");
        while g.pending.is_none() {
            if g.oracle_dead || g.outcome.is_some() {
                return false;
            }
            if g.agenda.len() <= floor {
                return true;
            }
            let t = g.agenda.pop().expect("a payment task");
            g.run_task(t);
        }
        if g.oracle_dead || g.outcome.is_some() {
            return false;
        }
        let d = g.pending.take().expect("a decision");
        let mut each = d.options.iter().map(|o| {
            let mut h = g.clone();
            h.inject(o.choice);
            Self::complete_from(h, floor, payer, nodes)
        });
        if d.player == payer {
            each.any(|x| x)
        } else {
            each.all(|x| x)
        }
    }

    /// Answer the pending decision with `choice` as `apply` does, without
    /// running on.
    fn inject(&mut self, choice: Choice) {
        match (self.page, choice) {
            (Some(_), Choice::Page(p)) => self.page = Some(p),
            _ => {
                self.page = None;
                self.answer = Some(choice);
            }
        }
    }

    /// The payment search's floor for `task`, if it is a payment whose
    /// options must leave it completable: a cost whose parts the player
    /// orders, or the mana payment before one.
    fn completing_floor(&self, task: &Task) -> Option<(usize, PlayerId)> {
        if self.oracle {
            return None;
        }
        match task {
            Task::PayCosts { order: Some(o), .. } => Some((o.floor as usize, o.p)),
            Task::PayMana(pm) => pm.oracle.map(|f| (f as usize, pm.payer)),
            _ => None,
        }
    }

    /// `options` of `task` that leave its payment completable (all of them
    /// for any other task).
    pub(crate) fn completing(&self, task: &Task, options: Vec<Opt>) -> Vec<Opt> {
        let Some((floor, payer)) = self.completing_floor(task) else { return options };
        options
            .into_iter()
            .filter(|o| {
                let mut g = self.clone();
                g.pending = None;
                g.agenda.push(task.clone());
                g.inject_fresh(o.choice);
                g.payment_completes_from(floor, payer)
            })
            .collect()
    }

    /// `completing` for the items of a paged choice.
    pub(crate) fn completing_items(&self, task: &Task, items: Vec<(u32, Choice)>) -> Vec<(u32, Choice)> {
        let Some((floor, payer)) = self.completing_floor(task) else { return items };
        items
            .into_iter()
            .filter(|&(_, c)| {
                let mut g = self.clone();
                g.pending = None;
                g.agenda.push(task.clone());
                g.inject_fresh(c);
                g.payment_completes_from(floor, payer)
            })
            .collect()
    }

    /// The answer `choice` to a decision just asked (not yet pending).
    fn inject_fresh(&mut self, choice: Choice) {
        self.page = None;
        self.answer = Some(choice);
    }

    /// `payment_completes` from this copy as it is (its task pushed and
    /// answered).
    fn payment_completes_from(mut self, floor: usize, payer: PlayerId) -> bool {
        self.oracle = true;
        self.oracle_dead = false;
        self.spec.log = false;
        let mut nodes = 0u64;
        Self::complete_from(self, floor, payer, &mut nodes)
    }

    /// Castability (ENGINE.md 49) of a cost whose parts' order can matter
    /// (`order_free`): can `p` pay `mana` (`pm`, the payment the cast or
    /// activation would make: its flags and what it keeps) and then the rest,
    /// `o`, in some order? The payment, with its spell on the stack (`cast`:
    /// moved there first, CR 601.2a), searched (`payment_completes`).
    pub(crate) fn payable_in_some_order(&self, o: Order, pm: Option<crate::pay::PayMana>, cast: bool) -> bool {
        let mut g = self.clone();
        g.pending = None;
        g.answer = None;
        g.page = None;
        if cast && !matches!(g.objs[o.card.i()].zone, Zone::Stack) {
            g.move_card_to_stack(o.card, o.p);
        }
        let floor = g.agenda.len();
        let mut o = o;
        o.floor = floor as u32;
        g.agenda.push(Task::PayCosts { id: NO_ITEM, picked: Vec::new(), early: None, order: Some(Box::new(o)) });
        if let Some(mut pm) = pm {
            pm.oracle = None;
            g.agenda.push(Task::PayMana(Box::new(pm)));
        }
        let payer = g.agenda.iter().rev().find_map(|t| match t {
            Task::PayCosts { order: Some(o), .. } => Some(o.p),
            _ => None,
        });
        g.payment_completes(floor, payer.expect("the cost just pushed"))
    }
}

/// `PayCosts::picked`'s mark of crew's `PASS` (enough power), for the run
/// that pays it (never pending).
const CREW_DONE: CardIdx = CardIdx::new(u32::MAX - 1);

/// The stack id of a payment the castability search makes with no stack item.
pub(crate) const NO_ITEM: u32 = u32::MAX;

/// The most copies one payment search makes before failing loudly (ENGINE.md
/// 100.4.9; 51's `SEARCH_NODES` likewise).
const ORACLE_NODES: u64 = 2_000_000;

