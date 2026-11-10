//! Activated abilities (CR 602, 605; ENGINE.md 18): what can be activated
//! at priority, the activation process (602.2: announce, targets, costs),
//! free-standing mana abilities, and "as this enters" choices (614.12).

use crate::db::{types, AbilityRef, AbilitySlot, ActEffect, ActivatedDef, Cond, Cost, Kw};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos, StackItem, StackKind};
use crate::ids::{Target, CardIdx, PlayerId, Zone};
use crate::loops::LoopAct;
use crate::pay::{productions, Class, Keep, PayMana, Src};

/// The `ACTIVATE` action slots of one permanent (`decision::act::ACTIVATE`:
/// slot * 4 + i).
pub(crate) const ACTIVATE_SLOTS: usize = 4;

impl Game {
    /// Does condition `c` hold for player `p`?
    pub(crate) fn cond_holds(&self, c: &Cond, p: PlayerId) -> bool {
        self.cond_holds_from(c, p, None)
    }

    /// A condition for `p`, of an ability whose source is `source` ("other
    /// than this").
    pub(crate) fn cond_holds_from(&self, c: &Cond, p: PlayerId, source: Option<crate::ids::ObjRef>) -> bool {
        let pl = &self.players[p as usize];
        match c {
            Cond::ControlAtLeast(f, n) => self.count_controlled(p, f) >= *n as u32,
            Cond::ControlAtMost(f, n) => self.count_controlled(p, f) <= *n as u32,
            Cond::HandExactly(n) => pl.hand.len() == *n as usize,
            // (Cards: a token in a graveyard isn't one, CR 108.2.)
            Cond::GraveyardAtLeast(n) => self.graveyard_cards(p).count() >= *n as usize,
            Cond::Delirium(n) => {
                // CR 205.2a card types (kindred included) among the cards.
                let t = self.graveyard_cards(p).fold(0u16, |m, c| m | self.types_of(c));
                t.count_ones() >= *n as u32
            }
            Cond::Revolt => pl.left_battlefield,
            Cond::YourTurnAtMost(n) => self.turn.active == p && pl.turns <= *n as u32,
            Cond::HandAtMost(n) => pl.hand.len() <= *n as usize,
            Cond::DevotionCoversLibrary(color) => self.devotion(p, *color) as usize >= pl.library.len(),
            Cond::FirstSpellThisGame => pl.spells_game == 0,
            Cond::IsMonarch => self.monarch == Some(p),
            // A static ability's own condition is read in layers.rs; with a
            // source (an activated or triggered ability's "N+ |", CR 722.2a:
            // the permanent has it while it has N or more charge counters),
            // the source on the battlefield now.
            Cond::SourceCountersIn(kind, lo, hi) => source.is_some_and(|s| {
                let n = if self.live(s) && self.objs[s.idx.i()].zone == Zone::Battlefield { self.counters_on(s.idx, *kind) } else { 0 };
                n >= *lo as crate::num::Count && hi.is_none_or(|h| n <= h as crate::num::Count)
            }),
            Cond::GainedLifeThisTurn => pl.gained_life,
            Cond::ArtifactEnteredThisTurn => pl.artifact_entered,
            Cond::APlayerControlsNoCreatures => (0..2u8).any(|q| self.creatures_of(q).next().is_none()),
            Cond::GainedLifeAtLeast(n) => pl.life_gained >= *n as i64,
            Cond::OpponentHasMoreCards => self.players[crate::ids::opp(p) as usize].hand.len() > pl.hand.len(),
            Cond::OpponentControlsMoreLands => {
                let lands = |q: PlayerId| self.permanents_of(q).filter(|&x| self.is_type(x, crate::db::types::LAND)).count();
                lands(crate::ids::opp(p)) > lands(p)
            }
            Cond::SpellsCastThisTurn { n, noncreature } => {
                (if *noncreature { pl.noncreature_cast } else { pl.spells_cast }) == *n as u32
            }
            Cond::LibraryEmpty => pl.library.is_empty(),
            Cond::YourTurn => self.turn.active == p,
            // (In `Wide`: twice a life total is exact.)
            Cond::LifeAtMostHalfStart => pl.life as crate::num::Wide * 2 <= self.spec.starting_life as crate::num::Wide,
            Cond::ControlOtherOfColor(m) => {
                self.permanents_of(p).any(|x| self.colors_of(x) & m != 0 && Some(self.obj_ref(x)) != source)
            }
            Cond::All(v) => v.iter().all(|c| self.cond_holds_from(c, p, source)),
            Cond::SpellsCastAtLeast(n) => pl.spells_cast >= *n as u32,
            Cond::HasInitiative => self.initiative == Some(p),
            Cond::CompletedDungeon => self.dungeons[p as usize].completed > 0,
            Cond::LinkedTypesAtLeast(n) => source.is_some_and(|s| self.linked_types(s).count_ones() >= *n as u32),
            Cond::CityBlessing => pl.city_blessing || self.ascends(p),
            Cond::LandsPlayedAtLeast(n) => pl.lands_played >= *n,
            Cond::SourceAttached => source.is_some_and(|s| self.live(s) && self.objs[s.idx.i()].attached.is_some()),
            Cond::Void => self.void_active(),
            Cond::OpponentsControlAtLeast(f, n) => {
                let ctx = crate::cast::Ctx { controller: p, source };
                let count = self
                    .battlefield
                    .iter()
                    .filter(|&&c| self.objs[c.i()].controller != p && self.card_matches(f, c, &ctx))
                    .count();
                count >= *n as usize
            }
            Cond::AttackedWithSubtype(k) => pl.attacked_with.iter().any(|r| self.db.def(self.objs[r.idx.i()].def).subtypes.contains(k)),
        }
    }

    /// Ascend (CR 702.131a): `p` controls a permanent with ascend and ten or
    /// more permanents.
    pub(crate) fn ascends(&self, p: PlayerId) -> bool {
        self.permanents_of(p).any(|c| self.abilities(c).ascend) && self.permanents_of(p).count() >= 10
    }

    /// The card types among the cards exiled with `source` (CR 607.2a,
    /// 205.2a): linked cards still in exile.
    pub(crate) fn linked_types(&self, source: crate::ids::ObjRef) -> u16 {
        self.links
            .iter()
            .filter(|l| l.source == source && self.live(l.card) && self.objs[l.card.idx.i()].zone == Zone::Exile)
            .fold(0u16, |m, l| m | self.types_of(l.card.idx))
    }

    /// CR 606.3: no loyalty ability of this permanent was activated this
    /// turn (or fewer than the extra activations an effect allows).
    pub(crate) fn loyalty_ready(&self, card: CardIdx) -> bool {
        let o = self.loyalty_of(card);
        // (`used < 1 + extra`, without overflow.)
        o.turn != self.turn.number as u16 || o.used <= o.extra
    }

    /// A loyalty ability of `card` is being activated (CR 606.3).
    fn note_loyalty_activation(&mut self, card: CardIdx) {
        let turn = self.turn.number as u16;
        let o = self.loyalty_mut(card);
        if o.turn != turn {
            o.turn = turn;
            o.used = 0;
            o.extra = 0;
        }
        let used = o.used;
        let used = self.exact_inc(used, "loyalty activations this turn");
        self.loyalty_mut(card).used = used;
    }

    /// The activated abilities `p` may activate with priority now (CR
    /// 602.2, 117.1b): permanents' abilities, then hand abilities. Mana
    /// abilities are activated while paying costs instead (ENGINE.md 18),
    /// except those `mana_with_priority` names.
    pub(crate) fn activation_options(
        &self,
        p: PlayerId,
        classes: &[Class],
        slots: &[(u16, CardIdx)],
        sorcery_ok: bool,
        out: &mut Vec<Opt>,
        gy: &mut Vec<(u32, Choice)>,
    ) {
        // (The battlefield's activation locks, read once; the loop hash of
        // this decision, computed once if needed, `loops.rs`.)
        let locks = self.activation_locks();
        let loop_hash = std::cell::OnceCell::new();
        for c in self.permanents_of(p) {
            let live = self.permanent_activations(p, c, classes, sorcery_ok, locks, &loop_hash);
            // A permanent with more activated abilities than its four
            // `ACTIVATE` slots (its card's own and those other permanents
            // grant it, EoE W3): the fourth slot opens a choice among the
            // fourth and later ones (`Task::ChooseAbility`), so every one
            // stays reachable.
            let paged = self.ability_slots_total(c) > ACTIVATE_SLOTS;
            for &i in &live {
                if !paged || (i as usize) < ACTIVATE_SLOTS - 1 {
                    let slot = AbilitySlot::Activated(i);
                    out.push(Opt { action: self.ability_action(p, c, slot), choice: Choice::Ability(c, slot) });
                }
            }
            if paged && live.iter().any(|&i| i as usize >= ACTIVATE_SLOTS - 1) {
                let slot = AbilitySlot::Activated((ACTIVATE_SLOTS - 1) as u8);
                out.push(Opt { action: self.ability_action(p, c, slot), choice: Choice::Ability(c, slot) });
            }
        }
        for &(a, c) in slots {
            for (i, ad) in self.def_of(c).hand_abilities.iter().enumerate() {
                let slot = AbilitySlot::Hand(i as u8);
                if self.can_activate(p, c, ad, classes, sorcery_ok, locks) {
                    // One hand ability per card (lowering): the card's slot.
                    out.push(Opt { action: act::HAND_ABILITY + (a - act::HAND), choice: Choice::Ability(c, slot) });
                }
            }
        }
        // Abilities of cards in the player's graveyard (CR 602.2; ENGINE.md
        // 27.5): position k of the `GY_ABILITY` range for the k-th card
        // there that has one (one each, the lowering checks), every one
        // (paged past the 12 ids, ENGINE.md 100.2).
        let mut k = 0u32;
        for c in self.graveyard_cards(p) {
            let d = self.def_of(c);
            let Some(ad) = d.gy_abilities.first() else { continue };
            let pos = k;
            k += 1;
            if self.can_activate(p, c, ad, classes, sorcery_ok, locks) {
                gy.push((pos, Choice::Ability(c, AbilitySlot::Graveyard(0))));
            }
        }
    }

    /// The activated abilities of permanent `c` that `p` may activate with
    /// priority now (CR 602.2, 117.1b), by their place among its card's own
    /// abilities and then those other permanents grant it (EoE W3; ascending):
    /// mana abilities only where `mana_at_priority` says; a special action
    /// (CR 116.2b, 708.7: turning a face-down permanent face up) survives
    /// "loses all abilities". A non-mana ability `p` activated this turn at
    /// a state with the same loop hash is not offered (a loop of optional
    /// actions that made no progress, `loops.rs`; `loop_hash` caches the
    /// decision's hash).
    pub(crate) fn permanent_activations(
        &self,
        p: PlayerId,
        c: CardIdx,
        classes: &[Class],
        sorcery_ok: bool,
        locks: u16,
        loop_hash: &std::cell::OnceCell<u64>,
    ) -> Vec<u8> {
        let mut live = Vec::new();
        let d = self.abilities(c);
        let place = if self.loop_taken.is_empty() { None } else { self.battlefield.iter().position(|&x| x == c).map(|k| k as u32) };
        let repeats = |i: usize| place.is_some_and(|place| self.loop_repeats(p, LoopAct::Activate { place, slot: i as u8 }, None, loop_hash));
        for (i, ad) in d.activated.iter().enumerate() {
            if ad.mana().is_some() && !self.mana_at_priority(c, ad) {
                continue; // paid with while paying a cost (ENGINE.md 18)
            }
            if d.latent > 0 && !self.has_activated(c, i) {
                continue; // a latent ability it hasn't gained
            }
            if ad.exhaust && self.exhausted(c, i as u8) {
                continue; // CR 702.177a: only once
            }
            if repeats(i) {
                continue; // a loop that made no progress (CR 732.2a, MTR 4.4)
            }
            if self.can_activate(p, c, ad, classes, sorcery_ok, locks) {
                live.push(i as u8);
            }
        }
        if self.lost_abilities(c) {
            for (i, ad) in self.db.def(self.objs[c.i()].def).activated.iter().enumerate() {
                if ad.special.is_some() && self.can_activate(p, c, ad, classes, sorcery_ok, locks) {
                    live.push(i as u8);
                }
            }
        }
        // Activated abilities other permanents grant it (an attached
        // Equipment's "equipped creature has", CR 301.5, ENGINE.md 99.14; "artifacts
        // you control have", 99.13; 613.1f), after its card's own places
        // whether or not it has them now.
        let own = self.own_ability_slots(c);
        for (j, g) in self.granted_activated(c).into_iter().enumerate() {
            let i = own + j;
            let ad = self.db.def(g.def).activated_def(AbilitySlot::Activated(g.slot));
            if repeats(i) {
                continue;
            }
            if self.can_activate(p, c, ad, classes, sorcery_ok, locks) {
                live.push(i as u8);
            }
        }
        live
    }

    /// The mana `p` pays to activate `ad` of `card` (CR 602.2b, 601.2f):
    /// its mana cost less its own reductions ("costs {1} less for each
    /// legendary creature you control"), and, for an artifact `p` controls,
    /// "activated abilities of artifacts you control cost {1} less", which
    /// leaves at least one mana. Generic mana only.
    pub(crate) fn act_mana(&self, p: PlayerId, card: CardIdx, ad: &ActivatedDef) -> crate::mana::ManaCost {
        self.act_mana_x(p, card, ad, 0)
    }

    /// [`act_mana`](Self::act_mana) with X announced (CR 601.2f: X is part
    /// of the cost before reductions).
    pub(crate) fn act_mana_x(&self, p: PlayerId, card: CardIdx, ad: &ActivatedDef, x: u32) -> crate::mana::ManaCost {
        let mut m = ad.cost.mana;
        // (An X that taps artifacts is no mana: EoE W3. "{X}{X}": the X paid
        // twice, `Cost::x_times`, ENGINE.md 99.12.)
        if ad.cost.tap_x.is_none() {
            // (Any X: the cost's parts are counts, ENGINE.md 100.4.8.)
            let xs = x as u64 * ad.cost.x_times.max(1) as u64;
            m.generic = m.generic.saturating_add(xs);
        }
        // "Activated abilities of sources with the chosen name cost {2} more
        // to activate unless they're mana abilities" (Anointed Peacekeeper;
        // CR 602.2b, 601.2f: increases first).
        if self.seen & crate::game::SEEN_NAMED_TAX != 0 && ad.mana().is_none() && ad.special.is_none() {
            m.generic = m.generic.saturating_add(self.named_tax(card, None));
        }
        if m.is_zero() {
            return m;
        }
        for d in &ad.discounts {
            let n = match d {
                // "Where X is this creature's power" (ENGINE.md 99.14).
                // (Past u64, a reduction to 0 as u64::MAX is: exact, CR 601.2f.)
                crate::db::Discount::SourcePower if self.is_creature(card) => u64::try_from(self.power(card).max(0)).unwrap_or(u64::MAX),
                _ => self.discount(d, p),
            };
            m.generic = m.generic.saturating_sub(n);
        }
        let ob = &self.objs[card.i()];
        if ob.zone == Zone::Battlefield && ob.controller == p && self.is_type(card, types::ARTIFACT) {
            for c in self.permanents_of(p) {
                let n = self.abilities(c).artifact_ability_discount;
                if n > 0 {
                    m.generic = m.generic.saturating_sub((n as u64).min(m.total().saturating_sub(1)));
                }
            }
        }
        // "Abilities you activate that aren't mana abilities cost {2} less"
        // (Zirda; at least one mana stays).
        if self.seen & crate::game::SEEN_ABILITY_DISCOUNT != 0 && ad.mana().is_none() && ad.special.is_none() && m.generic > 0 && m.total() > 1 {
            for c in self.permanents_of(p) {
                let n = self.abilities(c).ability_discount;
                if n > 0 {
                    m.generic = m.generic.saturating_sub((n as u64).min(m.total().saturating_sub(1)));
                }
            }
        }
        m
    }

    /// The card whose mana abilities can't pay an ability's mana at all:
    /// none (ENGINE.md 18 excluded the source whole when the cost tapped or
    /// moved it). No ability's source is excluded whole any more: the payment leaves
    /// out its mana abilities that conflict with the cost, `Keep::self_use`
    /// from `keep_for`; docs/shortcuts.md 13.3.)
    pub(crate) fn mana_exclude(_card: CardIdx, _ad: &ActivatedDef) -> Option<CardIdx> {
        None
    }

    /// Could `p` activate `ad` of `card` now and complete the activation
    /// (timing, restrictions, every part of the cost, targets)? An
    /// activation is offered only if it can be completed (ENGINE.md 6).
    fn can_activate(
        &self,
        p: PlayerId,
        card: CardIdx,
        ad: &ActivatedDef,
        classes: &[Class],
        sorcery_ok: bool,
        locks: u16,
    ) -> bool {
        let mana_ability = ad.mana().is_some();
        if mana_ability && !self.mana_at_priority(card, ad) {
            return false; // activated while paying (ENGINE.md 18)
        }
        // Turning a face-down permanent face up (CR 708.7, 116.2b): any time
        // its controller has priority, for its card's mana cost.
        if ad.special == Some(crate::db::Special::TurnFaceUp) {
            return self.face_down_cost(card).is_some_and(|m| self.can_pay_without(p, &m, classes, &Keep::default(), Some(card)) || m.is_zero());
        }
        match ad.loyalty {
            // CR 606.3: once each turn, at sorcery speed ("as long as this
            // entered this turn, any time you could cast an instant": The
            // Wandering Emperor); 606.6: a negative cost needs that much
            // loyalty.
            Some(d) => {
                let instant_ok =
                    self.abilities(card).loyalty_instant_if_entered && self.objs[card.i()].entered == self.turn.number as u16;
                if !(sorcery_ok || instant_ok) || !self.loyalty_ready(card) {
                    return false;
                }
                if d < 0 && self.counters_on(card, crate::db::Counter::Loyalty) < (-(d as i64)) as crate::num::Count {
                    return false;
                }
            }
            None => {
                if ad.sorcery && !sorcery_ok {
                    return false; // CR 602.5d
                }
            }
        }
        if !self.activation_ready(p, card, ad, locks) {
            return false;
        }
        if mana_ability {
            // A mana ability activated with priority (ENGINE.md 18, 28): its
            // whole cost, and some mana to make (what a sacrificed land
            // could make is read as it is sacrificed; it may be none).
            if !self.cost_payable_with(p, card, &ad.cost, &self.act_mana(p, card, ad), classes, Self::mana_exclude(card, ad), false, self.ability_allow(card)) {
                return false;
            }
            let prod = &ad.mana().unwrap().prod;
            return matches!(prod, crate::db::ManaProd::SacrificedLand) || !productions(self, p, prod, Some(card)).is_empty();
        }
        let with_bending;
        let classes = if ad.cost.waterbend {
            with_bending = [classes, &self.improvise_classes(p, Self::mana_exclude(card, ad), types::ARTIFACT | types::CREATURE)[..]].concat();
            &with_bending[..]
        } else {
            classes
        };
        // (A non-mana ability: with no mana to pay, a permanent with a mana
        // ability may be tapped for a "tap an untapped <permanent>" cost.)
        // (Its parts in any order when that can matter: ENGINE.md 100.4.9.)
        let loyalty = ad.loyalty.map_or(0, |d| d as i32);
        let mana = self.act_mana(p, card, ad);
        let exclude = Self::mana_exclude(card, ad);
        let ordered = self.cost_payable_order(p, card, &ad.cost, &mana, classes, exclude, self.ability_allow(card), loyalty, false);
        if !ordered.unwrap_or_else(|| self.cost_payable_with(p, card, &ad.cost, &mana, classes, exclude, true, self.ability_allow(card))) {
            return false;
        }
        let ActEffect::Stack(ab) = &ad.effect else { unreachable!() };
        self.targets_available(ab, p, Some(self.obj_ref(card)))
    }

    /// Permanents `p` could sacrifice for `s` (canonical order).
    pub(crate) fn sacrifice_candidates(&self, p: PlayerId, source: CardIdx, s: &crate::db::SacCost) -> Vec<CardIdx> {
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(source)) };
        self.permanents_of(p)
            .filter(|&c| !(s.another && c == source) && self.card_matches(&s.filter, c, &ctx))
            .collect()
    }

    /// Activate `slot` of `card` (CR 602.2): a mana ability resolves at
    /// once (605.3); any other ability goes on the stack first (602.2a),
    /// then targets (601.2c), then the cost (601.2f-h).
    pub(crate) fn begin_activate(&mut self, p: PlayerId, card: CardIdx, slot: AbilitySlot) {
        // (A granted ability is its granting definition's: an Equipment's
        // face, ENGINE.md 99.14, or a static ability's card, 99.13; `slot`
        // stays the permanent's place for its records.)
        let (def, aslot, granter) = self.activated_home(card, slot);
        // A loop action (a non-mana ability of a permanent on the
        // battlefield, `loops.rs`): noted with the state it is taken at,
        // before anything changes.
        if let (AbilitySlot::Activated(i), crate::db::ActEffect::Stack(_), Zone::Battlefield) =
            (slot, &self.db.def(def).activated_def(aslot).effect, self.objs[card.i()].zone)
        {
            let place = self.battlefield.iter().position(|&x| x == card).expect("a permanent") as u32;
            self.note_loop(p, LoopAct::Activate { place, slot: i }, None);
        }
        self.note(|g| format!("P{p} activates {} ({slot:?})", g.name(card)));
        if self.objs[card.i()].zone == Zone::Hand {
            self.reveal(card); // 602.2a: activated from a hidden zone
        }
        let ad = self.db.def(def).activated_def(aslot);
        if ad.exhaust {
            if let AbilitySlot::Activated(i) = slot {
                let r = self.obj_ref(card);
                self.marks.push((r, crate::game::Mark::Exhausted(i)));
            }
        }
        if ad.boast {
            let r = self.obj_ref(card);
            self.marks.push((r, crate::game::Mark::Boasted(self.turn.number as u16)));
        }
        if ad.mana().is_some() {
            self.agenda.push(Task::ManaNow { src: Src::of(card, slot), step: crate::decision::NumberStep::FIRST });
            return;
        }
        // A special action (CR 116.2): its cost, then it happens; no stack.
        if let Some(sp) = ad.special {
            if sp == crate::db::Special::TurnFaceUp {
                // CR 701.40b: "show all players that the card representing
                // that permanent is a creature card and what that card's mana
                // cost is, pay that cost, then turn the permanent face up":
                // the card is shown before the payment, which its cost would
                // otherwise tell (ENGINE.md 36.11).
                self.reveal(card);
            }
            self.agenda.push(Task::Special { p, card, sp });
            let mana = self.special_mana(p, card, ad);
            if !mana.is_zero() {
                let mut pm = PayMana::new(p, mana);
                pm.keep = Self::keep_for(card, &ad.cost, Some(card));
                self.agenda.push(Task::PayMana(Box::new(pm)));
            }
            return;
        }
        if ad.loyalty.is_some() {
            self.note_loyalty_activation(card);
        }
        let id = self.new_stack_id();
        let ability = AbilityRef { def, slot: aslot, mode: None };
        self.stack.push(StackItem {
            id,
            kind: StackKind::Ability { ability, source: self.obj_ref(card) },
            controller: p,
            targets: Vec::new(),
            cost_option: 0,
            modes: Vec::new(),
            ev: Default::default(),
            alt: 0,
            from: Default::default(),
            delved: 0,
            delve_floor: 0,
            discarded_land: false,
            x: 0,
            division: Vec::new(),
            attack_as: None,
            noted: granter.map(|e| vec![self.obj_ref(e)]).unwrap_or_default(),
            flips: self.objs[card.i()].flips,
            origin: Zone::Nowhere,
            times: 0,
            spliced: Vec::new(),
        });
        self.agenda.push(Task::PayCosts { id, picked: Vec::new(), early: None, order: None });
        self.agenda.push(Task::ActPay { id });
        let ab = self.db.ability(ability);
        if ab.has_targets() {
            self.agenda.push(Task::ChooseTargets { id, req: 0, page: crate::decision::Page::FIRST });
        }
        if ad.x {
            self.agenda.push(Task::ChooseX { id, step: crate::decision::NumberStep::FIRST }); // CR 602.2b, 601.2b
        }
        if ab.modes.is_some() {
            self.agenda.push(Task::ChooseModes { id }); // CR 700.2a, 602.2b
        }
    }

    /// The activated ability and source card of stack item `id`.
    fn act_item(&self, id: u32) -> Option<(PlayerId, CardIdx, &'static ActivatedDef)> {
        let pos = self.item_index(id)?;
        let item = &self.stack[pos];
        let StackKind::Ability { ability, source } = item.kind else { return None };
        let ad = self.db.def(ability.def).activated_def(ability.slot);
        Some((item.controller, source.idx, ad))
    }

    /// The cost of stack item `id` besides its mana: an activated ability's,
    /// or the additional cost the caster of a spell chose.
    fn item_cost(&self, id: u32) -> Option<(PlayerId, CardIdx, &'static Cost)> {
        let pos = self.item_index(id)?;
        let item = &self.stack[pos];
        match item.kind {
            StackKind::Ability { ability, source } => {
                Some((item.controller, source.idx, &self.db.def(ability.def).activated_def(ability.slot).cost))
            }
            StackKind::Spell { card } => {
                // (Its own alternative cost or a granted one: ENGINE.md 99.15.)
                let c = if item.alt > 0 {
                    self.item_alt_rest(card, item)
                } else {
                    self.route_extra(card, item.from, item.cost_option)?
                };
                Some((item.controller, card, c))
            }
        }
    }

    /// Can `p` pay the non-mana parts of `cost` for the card `card` and,
    /// together, its `mana` (CR 601.2h; the parts paid after the mana are
    /// reserved from it)?
    pub(crate) fn cost_payable(
        &self,
        p: PlayerId,
        card: CardIdx,
        cost: &Cost,
        mana: &crate::mana::ManaCost,
        classes: &[Class],
        exclude: Option<CardIdx>,
    ) -> bool {
        // (A spell's: its parts in any order when that can matter, ENGINE.md
        // 100.4.9.)
        if let Some(r) = self.cost_payable_order(p, card, cost, mana, classes, exclude, 0, 0, true) {
            return r;
        }
        self.cost_payable_with(p, card, cost, mana, classes, exclude, false, 0)
    }

    /// Castability of a cost whose parts' order can matter (CR 601.2h;
    /// `order_free`, ENGINE.md 100.4.9): `Some` of whether some order pays
    /// it, with its mana first (`payable_in_some_order`); `None` when the
    /// order is free (the fixed order's checks are exact). `cast`: a spell's
    /// cost (improvise, convoke; the spell on the stack).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cost_payable_order(
        &self,
        p: PlayerId,
        card: CardIdx,
        cost: &Cost,
        mana: &crate::mana::ManaCost,
        classes: &[Class],
        exclude: Option<CardIdx>,
        allow: u8,
        loyalty: i32,
        cast: bool,
    ) -> Option<bool> {
        let o = crate::cost_order::Order::new(p, card, cost, 0, loyalty, 0);
        if self.order_free(&o) {
            return None;
        }
        let keep = Keep { exclude, allow, spell: cast.then_some(card), ..Keep::default() };
        let forced = self.exile_sources(p, card, cost, exclude);
        let pm = if mana.is_zero() && forced.is_empty() {
            None
        } else {
            if !mana.is_zero() && !self.can_pay_with(p, mana, classes, &keep) {
                return Some(false);
            }
            let mut pm = PayMana::new(p, *mana);
            pm.keep = keep;
            pm.forced = forced;
            pm.waterbend = cost.waterbend;
            if cast {
                pm.improvise = self.def_of(card).improvise;
                pm.convoke = self.def_of(card).casting.convoke;
            }
            Some(pm)
        };
        Some(self.payable_in_some_order(o, pm, cast))
    }

    /// [`cost_payable`](Self::cost_payable). A "tap n untapped <permanent>"
    /// part, crew or saddle may tap a permanent with a mana ability: the
    /// mana is paid with the others (CR 601.2g before 601.2h; the payment
    /// keeps the taps payable, `Keep::taps`; docs/shortcuts.md 27.13.4).
    /// (`_zero_mana_taps`: no longer read, every candidate counts.)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cost_payable_with(
        &self,
        p: PlayerId,
        card: CardIdx,
        cost: &Cost,
        mana: &crate::mana::ManaCost,
        classes: &[Class],
        exclude: Option<CardIdx>,
        _zero_mana_taps: bool,
        // The spending restrictions the payment meets (`Keep::allow`: an
        // ability's source, ENGINE.md 99.12).
        allow: u8,
    ) -> bool {
        let pl = &self.players[p as usize];
        if (cost.life as i64) > pl.life {
            return false; // CR 119.4
        }
        if let Some(s) = &cost.sacrifice {
            if self.sacrifice_candidates(p, card, s).len() < s.n as usize {
                return false;
            }
        }
        let in_hand = self.objs[card.i()].zone == Zone::Hand;
        if cost.discard as usize > pl.hand.len() - in_hand as usize {
            return false;
        }
        if let Some(f) = &cost.discard_filter {
            let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
            let n = pl.hand.iter().filter(|&&c| c != card && self.card_matches(f, c, &ctx)).count();
            if (cost.discard as usize) > n {
                return false;
            }
        }
        if let Some(f) = &cost.exile_from_hand {
            if self.exile_candidates(p, card, f).is_empty() {
                return false;
            }
        }
        if let Some(b) = &cost.bounce {
            if self.sacrifice_candidates(p, card, b).len() < b.n as usize {
                return false;
            }
        }
        // (Its parts that take permanents or cards in hand, together:
        // distinct candidates for each, ENGINE.md 100.4.7.)
        if !self.reserves_ok(p, &Self::keep_for(card, cost, exclude)) {
            return false;
        }
        if let Some((k, n)) = cost.remove_counters {
            if self.counters_on(card, k) < n as crate::num::Count {
                return false;
            }
        }
        if let Some((f, k)) = &cost.remove_counter_from {
            if self.remove_counter_candidates(p, card, f, *k).is_empty() {
                return false;
            }
        }
        if cost.crew > 0 {
            if self.is_spell_card(card) {
                // Teamwork: the taps (paid first) and the mana compete.
                let cands = self.crew_eligible(p, card);
                let keep = Self::keep_for(card, cost, exclude);
                if !self.crew_mana_ok(p, cost.crew as crate::num::Wide, &[], &cands, mana, classes, &keep) {
                    return false;
                }
            } else if self.crew_power(p, card, !cost.saddle) < cost.crew as crate::num::Wide {
                return false;
            }
        }
        // (Every candidate counts: one tapped for mana instead is the
        // payment's choice, `Keep::taps`, checked with the mana below.)
        if let Some(t) = &cost.tap_untapped {
            if self.tap_candidates(p, card, t).count() < t.n as usize {
                return false;
            }
        }
        // (Mana sources that sacrifice themselves are cards in the graveyard
        // by then: activated first, CR 601.2g-h. So are the cards the cost
        // itself discards and the nontoken permanents it sacrifices: paid
        // before the graveyard's cards are chosen, 601.2h; ruling bea302d9.)
        let short = self.exile_short(p, card, cost);
        if short > 0 {
            let d: Vec<CardIdx> =
                self.delve_sources(p).into_iter().filter(|&c| c != card && Some(c) != exclude && self.gy_cost_card(cost, c)).collect();
            if short > d.len() + self.cost_feeds_graveyard(p, card, cost, &d) {
                return false;
            }
        }
        if mana.is_zero() {
            return true;
        }
        let keep = Keep { allow, taps: self.taps_for(p, card, cost), ..Self::keep_for(card, cost, exclude) };
        self.can_pay_without(p, mana, classes, &keep, exclude)
    }

    /// The tap cost `card`'s `cost` pays after its mana (`Keep::taps`): crew
    /// or saddle of a permanent (teamwork's taps are paid before the mana:
    /// `crew_mana_ok`, `Keep::committed`), or "tap n untapped <permanents>".
    /// (The lowering refuses a cost with two of them.)
    pub(crate) fn taps_for(&self, p: PlayerId, card: CardIdx, cost: &Cost) -> Option<crate::pay::TapsOwed> {
        if cost.crew > 0 && !self.is_spell_card(card) {
            let bonus = !cost.saddle;
            let cands = self.crew_eligible(p, card).into_iter().map(|c| (c, self.crew_power_of(c, bonus))).collect();
            return Some(crate::pay::TapsOwed { need: cost.crew as crate::num::Wide, cands });
        }
        let t = cost.tap_untapped.as_ref()?;
        Some(crate::pay::TapsOwed { need: t.n as crate::num::Wide, cands: self.tap_candidates(p, card, t).map(|c| (c, 1)).collect() })
    }

    /// Sacrifice the permanents chosen for a cost (together, one batch).
    pub(crate) fn sacrifice_picked(&mut self, p: PlayerId, id: u32, picked: &[CardIdx]) {
        let mut sacs: Vec<CardIdx> = picked.to_vec();
        sacs.sort_by_key(|c| self.objs[c.i()].timestamp);
        // "The sacrificed permanent's mana value" (as it last existed, CR
        // 608.2h): an activated ability's item keeps it (`Amount::CostMvPlus`).
        if let (Some(&c), Some(pos)) = (sacs.first(), self.item_index(id)) {
            if matches!(self.stack[pos].kind, StackKind::Ability { .. }) {
                self.stack[pos].ev.amount = self.mana_value_of(c) as u64;
                // (And the permanent, for its power: `Amount::CostPower`.)
                if self.stack[pos].ev.obj.is_none() {
                    self.stack[pos].ev.obj = Some(self.obj_ref(c));
                }
            }
        }
        self.begin_batch();
        for c in sacs {
            self.note(|g| format!("P{p} sacrifices {}", g.name(c)));
            self.sacrifice(c);
        }
        self.end_batch();
    }

    /// Would permanent `c`, sacrificed, be a card in `p`'s graveyard (not
    /// a token; not exiled instead, CR 614.1a; `p`'s own card: a permanent
    /// `p` controls but another player owns goes to its owner's graveyard,
    /// CR 400.3, ENGINE.md 51)?
    pub(crate) fn feeds_graveyard(&self, p: PlayerId, c: CardIdx) -> bool {
        self.objs[c.i()].owner == p
            && !self.is_token(c)
            && matches!(self.replaced_dest(c, Zone::Battlefield, Zone::Graveyard), crate::game::Dest::To(Zone::Graveyard))
    }

    /// The cards `cost`'s discards can put into `p`'s graveyard: as many as
    /// it discards, of the hand cards it may discard that would go there
    /// (none under "exile it instead", CR 614.1a; not Blightsteel Colossus,
    /// shuffled into its library: ENGINE.md 49).
    pub(crate) fn discards_feed(&self, p: PlayerId, card: CardIdx, cost: &Cost) -> usize {
        // (A second discard, `discard_also`, is retrace's land card merged
        // with a card's own discard option, and an option is one kind of
        // cost: no such cost exiles graveyard cards too, `merge_costs`.)
        assert!(cost.discard_also.is_none() || cost.exile_graveyard == 0, "a cost of two discards that exiles graveyard cards");
        if cost.discard == 0 {
            return 0;
        }
        let n = self.players[p as usize].hand.iter().filter(|&&c| c != card && self.discard_feeds(p, card, cost, c)).count();
        n.min(cost.discard as usize)
    }

    /// Would hand card `c`, discarded for `card`'s `cost`, go to the
    /// graveyard (and may the cost discard it)?
    fn discard_feeds(&self, p: PlayerId, card: CardIdx, cost: &Cost, c: CardIdx) -> bool {
        let ok = cost.discard_filter.as_ref().is_none_or(|f| {
            let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
            self.card_matches(f, c, &ctx)
        });
        ok && self.gy_cost_card(cost, c) && matches!(self.replaced_dest(c, Zone::Hand, Zone::Graveyard), crate::game::Dest::To(Zone::Graveyard))
    }

    /// The cards `cost` itself puts into `p`'s graveyard before its
    /// graveyard cards are chosen (CR 601.2h; ruling bea302d9): its
    /// discards, and as many of its sacrifices as can be nontoken cards
    /// besides the mana sources `d` counted already (each card once).
    pub(crate) fn cost_feeds_graveyard(&self, p: PlayerId, card: CardIdx, cost: &Cost, d: &[CardIdx]) -> usize {
        let sacs = cost.sacrifice.as_ref().map_or(0, |s| {
            (s.n as usize).min(
                self.sacrifice_candidates(p, card, s)
                    .into_iter()
                    .filter(|&c| !d.contains(&c) && self.feeds_graveyard(p, c) && self.gy_cost_card(cost, c))
                    .count(),
            )
        });
        self.discards_feed(p, card, cost) + sacs
    }

    /// Untapped permanents `p` controls that a "tap n untapped <filter> you
    /// control" cost of `card` may tap (not `card` itself).
    pub(crate) fn tap_candidates<'a>(&'a self, p: PlayerId, card: CardIdx, t: &'a crate::db::SacCost) -> impl Iterator<Item = CardIdx> + 'a {
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
        self.permanents_of(p)
            .filter(move |&c| c != card && !self.objs[c.i()].tapped && self.card_matches(&t.filter, c, &ctx))
    }

    /// Does permanent `c` have a mana ability?
    pub(crate) fn has_mana_ability(&self, c: CardIdx) -> bool {
        self.abilities(c).activated.iter().any(|a| a.mana().is_some()) || self.tapped_for_mana(c)
    }

    /// Could `c` be tapped while paying for another permanent's mana ability
    /// ("tap an untapped artifact you control: add {U}", Urza)? Then it
    /// counts as a mana source for the costs that tap creatures besides the
    /// mana (ENGINE.md 29: they and the payment never compete).
    pub(crate) fn tapped_for_mana(&self, c: CardIdx) -> bool {
        let p = self.objs[c.i()].controller;
        let ctx = crate::cast::Ctx { controller: p, source: None };
        self.permanents_of(p).any(|x| {
            self.abilities(x).activated.iter().any(|a| {
                a.mana().is_some() && a.cost.tap_untapped.as_ref().is_some_and(|t| self.card_matches(&t.filter, c, &ctx))
            })
        })
    }

    /// Creatures that can crew `vehicle` for `p`: untapped creatures they
    /// control other than it. (Teamwork, a spell's crew-like cost: a
    /// creature with a mana ability too; the spell's taps are paid before
    /// its mana and leave the mana payable, `crew_mana_ok`.)
    pub(crate) fn crew_eligible(&self, p: PlayerId, vehicle: CardIdx) -> Vec<CardIdx> {
        self.creatures_of(p).filter(|&c| c != vehicle && !self.objs[c.i()].tapped).collect()
    }

    /// Is `card` a spell or a card that may become one (teamwork's cost is a
    /// spell's; crew and saddle are abilities of permanents)?
    pub(crate) fn is_spell_card(&self, card: CardIdx) -> bool {
        matches!(self.objs[card.i()].zone, Zone::Hand | Zone::Stack | Zone::Graveyard | Zone::Exile | Zone::Library)
    }

    /// The most power `p` can tap to crew `vehicle` (`bonus`: crew itself,
    /// where "crews Vehicles as though its power were greater" applies).
    /// (Powers are summed in `Wide`: exact, ENGINE.md 46, 100.7.)
    pub(crate) fn crew_power(&self, p: PlayerId, vehicle: CardIdx, bonus: bool) -> crate::num::Wide {
        self.crew_eligible(p, vehicle).into_iter().map(|c| self.crew_power_of(c, bonus).max(0)).sum()
    }

    /// A creature's power for crewing (a Pilot crews as though its power
    /// were greater: crew only, not saddle or teamwork, CR 702.122,
    /// 702.171a).
    pub(crate) fn crew_power_of(&self, c: CardIdx, bonus: bool) -> crate::num::Wide {
        self.power(c) + if bonus { self.abilities(c).crew_bonus as crate::num::Wide } else { 0 }
    }

    /// Teamwork (a spell's "tap any number of untapped creatures you control
    /// with total power n or more", paid in the engine's order before its
    /// mana): can `p` tap creatures of `cands`, with the ones already chosen
    /// (`tapped_in`), for total power `need` and still pay `mana` from
    /// `classes`? By the rules the mana abilities come first (CR 601.2g,
    /// then 601.2h), so a creature tapped or sacrificed for mana can't be
    /// tapped for teamwork: the feasibility tries each creature both ways
    /// (`Keep::taps`, exact for any number of creatures; docs/shortcuts.md
    /// N8: only 10 were tried before).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn crew_mana_ok(
        &self,
        p: PlayerId,
        need: crate::num::Wide,
        tapped_in: &[CardIdx],
        cands: &[CardIdx],
        mana: &crate::mana::ManaCost,
        classes: &[crate::pay::Class],
        keep: &Keep,
    ) -> bool {
        let have: crate::num::Wide = tapped_in.iter().map(|&c| self.crew_power_of(c, false)).sum();
        let rest: crate::num::Wide = cands.iter().map(|&c| self.crew_power_of(c, false).max(0)).sum();
        if have + rest < need {
            return false;
        }
        if mana.is_zero() {
            return true;
        }
        let left = self.classes_without(classes, tapped_in);
        let taps = crate::pay::TapsOwed { need: need - have, cands: cands.iter().map(|&c| (c, self.crew_power_of(c, false))).collect() };
        self.can_pay_with(p, mana, &left, &Keep { taps: Some(taps), committed: tapped_in.to_vec(), ..keep.clone() })
    }

    /// Cards in `p`'s hand matching `f` other than `card`, one per
    /// definition (canonical slots).
    pub(crate) fn exile_candidates(&self, p: PlayerId, card: CardIdx, f: &crate::db::Filter) -> Vec<crate::decision::Opt> {
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
        self.hand_options_except(p, Some(card))
            .into_iter()
            .filter(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(f, c, &ctx)))
            .collect()
    }

    /// Do the parts of `card`'s `cost` that take permanents or cards in hand
    /// still find distinct candidates once `next` is picked for the part
    /// being paid, the picks so far (`picked`, `pay_costs`' order) left out
    /// (ENGINE.md 100.4.7: a pick could take the only candidate of a later
    /// part)?
    fn cost_picks_ok(&self, p: PlayerId, card: CardIdx, cost: &Cost, picked: &[CardIdx], next: CardIdx) -> bool {
        use crate::pay::Reserve;
        let n_sac = cost.sacrifice.as_ref().map_or(0, |s| s.n) as usize;
        let n_also = n_sac + cost.discard_also.as_ref().map_or(0, |d| d.0) as usize;
        let n_disc = n_also + cost.discard as usize;
        let n_exile = n_disc + cost.exile_from_hand.is_some() as usize;
        let n_bounce = n_exile + cost.bounce.as_ref().map_or(0, |b| b.n) as usize;
        // (What part [lo, hi) of the picks still needs, `next` picked.)
        let k = picked.len() + 1;
        let left = |lo: usize, hi: usize| ((hi - lo) - k.saturating_sub(lo).min(hi - lo)) as u8;
        let mut rs: Vec<(Reserve, CardIdx)> = Vec::new();
        let perm = |rs: &mut Vec<(Reserve, CardIdx)>, s: &crate::db::SacCost, n: u32| {
            if n > 0 {
                rs.push((Reserve::Perm(crate::db::SacCost { n, ..s.clone() }, false), card));
            }
        };
        if let Some(s) = &cost.sacrifice {
            perm(&mut rs, s, left(0, n_sac) as u32);
        }
        if let Some(b) = &cost.bounce {
            perm(&mut rs, b, left(n_exile, n_bounce) as u32);
        }
        if let Some(r) = Self::remove_counter_reserve(cost) {
            perm(&mut rs, &r, r.n);
        }
        if let Some((_, f)) = &cost.discard_also {
            let n = left(n_sac, n_also);
            if n > 0 {
                rs.push((Reserve::Discard(n, Some(f.clone())), card));
            }
        }
        let n = left(n_also, n_disc);
        if n > 0 {
            rs.push((Reserve::Discard(n, cost.discard_filter.clone()), card));
        }
        if let Some(f) = &cost.exile_from_hand {
            if left(n_disc, n_exile) > 0 {
                rs.push((Reserve::Exile(f.clone()), card));
            }
        }
        self.reserves_left_ok(p, &rs, &|c| c == next || picked.contains(&c))
    }

    /// What the parts of `cost` paid after its mana need from that payment:
    /// each part that takes permanents or cards in hand a reservation
    /// (`Keep::reserves`, checked together: ENGINE.md 100.4.7).
    pub(crate) fn keep_for(card: CardIdx, cost: &Cost, exclude: Option<CardIdx>) -> Keep {
        use crate::pay::Reserve;
        let mut reserves: Vec<(Reserve, CardIdx)> = Vec::new();
        // (A permanent to return to its owner's hand, ninjutsu's unblocked
        // attacker, is reserved like one to sacrifice: a mana ability that
        // sacrifices it can't pay the mana first. So is one to remove a
        // counter from, CR 601.2g; EoE W3.)
        for s in [cost.sacrifice.clone(), cost.bounce.clone(), Self::remove_counter_reserve(cost)].into_iter().flatten() {
            reserves.push((Reserve::Perm(s, false), card));
        }
        if cost.discard > 0 {
            reserves.push((Reserve::Discard(cost.discard, cost.discard_filter.clone()), card));
        }
        if let Some((n, f)) = &cost.discard_also {
            reserves.push((Reserve::Discard(*n, Some(f.clone())), card));
        }
        if let Some(f) = &cost.exile_from_hand {
            reserves.push((Reserve::Exile(f.clone()), card));
        }
        Keep {
            exclude,
            life: cost.life,
            reserves,
            allow: 0,
            spell: None,
            taps: None,
            committed: Vec::new(),
            // (Its source's mana abilities that conflict with the cost: out
            // of the payment, the others in; docs/shortcuts.md 13.3.)
            self_use: crate::pay::SelfUse::of(card, cost),
        }
    }

    /// 601.2g-h for an activation: the mana part first.
    pub(crate) fn act_pay(&mut self, id: u32) {
        let Some((p, card, ad)) = self.act_item(id) else { return };
        let x = self.item_index(id).map_or(0, |pos| self.stack[pos].x);
        let mana = self.act_mana_x(p, card, ad, x);
        // CR 601.2h: the parts in any order, when that can matter (ENGINE.md
        // 100.4.9): the mana first (601.2g), with every option leaving the
        // whole payment completable; then the parts as the player orders
        // them (`PayCosts::order`, below on the agenda).
        let mut o = crate::cost_order::Order::new(p, card, &ad.cost, self.tap_x_n(id, &ad.cost) as u32, ad.loyalty.map_or(0, |d| d as i32), 0);
        if !self.order_free(&o) {
            let floor = self.agenda.len() - 1;
            assert!(matches!(self.agenda.last(), Some(Task::PayCosts { id: i, order: None, .. }) if *i == id), "the activation's cost below its mana");
            o.floor = floor as u32;
            *self.agenda.last_mut().expect("the cost") = Task::PayCosts { id, picked: Vec::new(), early: None, order: Some(Box::new(o)) };
            let exclude = Self::mana_exclude(card, ad);
            let mut pm = PayMana::new(p, mana);
            pm.keep = Keep { exclude, allow: self.ability_allow(card), ..Keep::default() };
            pm.waterbend = ad.cost.waterbend;
            pm.forced = self.exile_sources(p, card, &ad.cost, exclude);
            pm.oracle = Some(floor as u32);
            if !(mana.is_zero() && pm.forced.is_empty()) {
                self.agenda.push(Task::PayMana(Box::new(pm)));
            }
            return;
        }
        if mana.is_zero() && self.exile_short(p, card, &ad.cost) == 0 {
            return;
        }
        let mut pm = PayMana::new(p, mana);
        pm.keep = Self::keep_for(card, &ad.cost, Self::mana_exclude(card, ad));
        // (Mana spendable on abilities of its source's kind, CR 106.6.)
        pm.keep.allow = self.ability_allow(card);
        // Crew, saddle, "tap n untapped <permanents>": paid next, so the
        // mana leaves them payable (27.13.4).
        pm.keep.taps = self.taps_for(p, card, &ad.cost);
        // "Tap X untapped artifacts": X of them stay for the cost (EoE W3).
        if let Some(r) = Self::tap_x_reserve(&ad.cost, x) {
            pm.keep.reserves.push((crate::pay::Reserve::Perm(r, true), card));
        }
        pm.waterbend = ad.cost.waterbend;
        pm.forced = self.exile_sources(p, card, &ad.cost, Self::mana_exclude(card, ad));
        self.agenda.push(Task::PayMana(Box::new(pm)));
    }

    /// How many more cards "exile n cards from your graveyard" needs than
    /// the graveyard has (`card`, the cost's, left out).
    pub(crate) fn exile_short(&self, p: PlayerId, card: CardIdx, cost: &Cost) -> usize {
        if cost.exile_graveyard == 0 {
            return 0;
        }
        // (Not the cards the delve of the spell being cast exiles first,
        // `Mark::DelveExile`: ENGINE.md 51.)
        (cost.exile_graveyard as usize)
            .saturating_sub(self.graveyard_cards(p).filter(|&c| c != card && !self.delve_exiling(c) && self.gy_cost_card(cost, c)).count())
    }

    /// The mana sources a payment activates first so that they are in the
    /// graveyard for `cost`'s "exile n cards from your graveyard" (CR
    /// 601.2g-h; the first in canonical order: Lotus Petals alike).
    pub(crate) fn exile_sources(&self, p: PlayerId, card: CardIdx, cost: &Cost, exclude: Option<CardIdx>) -> Vec<(crate::pay::Src, bool)> {
        // (The cost's own discards are graveyard cards by then: ruling
        // bea302d9.)
        let short = self.exile_short(p, card, cost).saturating_sub(self.discards_feed(p, card, cost));
        if short == 0 {
            return Vec::new();
        }
        let classes = self.mana_classes(p, exclude);
        self.delve_sources(p)
            .into_iter()
            .filter(|&c| c != card && Some(c) != exclude && self.gy_cost_card(cost, c))
            .take(short)
            .filter_map(|c| classes.iter().find_map(|cl| cl.members.iter().find(|s| s.card == c && cl.sac_self).copied()))
            .map(|s| (s, false))
            .collect()
    }

    /// The options of a cost's choice of a permanent to sacrifice or return
    /// (`Sacrifice`, `BounceCost`): `MY_PERM` + its slot, and for an untapped
    /// mana source also `ACTIVATE` + its slot x 4 + i = "activate its mana
    /// ability i first, then sacrifice or return it" (ENGINE.md 86.2; CR
    /// 601.2g-h; `float_before_cost`, `life_after` = the cost's life).
    pub(crate) fn cost_pick_options(&self, p: PlayerId, cands: &[CardIdx], life_after: u32) -> Vec<Opt> {
        let mut opts = Vec::with_capacity(cands.len());
        for &c in cands {
            opts.push(Opt { action: self.perm_action(p, c), choice: Choice::Card(c) });
            for i in self.float_before_cost(p, c, life_after) {
                let slot = crate::db::AbilitySlot::Activated(i);
                opts.push(Opt { action: self.ability_action(p, c, slot), choice: Choice::Ability(c, slot) });
            }
        }
        opts
    }

    /// The rest of a cost after its mana (601.2h): sacrifice and discard
    /// choices, one decision per object; then {T}, life, and the source's
    /// own costs.
    ///
    /// The picks, in `picked`, part by part: the sacrifices, the discards,
    /// the card exiled from hand, the returns; then the graveyard cards to
    /// exile, the taps of "tap n untapped <permanents>", the permanent to
    /// remove a counter from and crew's creatures. A cost that sacrifices and
    /// exiles graveyard cards sacrifices before its graveyard cards are chosen
    /// (CR 601.2h, any order: a card sacrificed may be exiled, ruling
    /// bea302d9); its taps, counter and crew are chosen and paid before that
    /// sacrifice, so a permanent may be tapped (or crew) and then sacrificed
    /// (ENGINE.md 100.4.8), and its graveyard cards come last. `early`: the
    /// number of picks once those parts and the sacrifice are paid (the
    /// graveyard cards' picks follow).
    pub(crate) fn pay_costs(&mut self, id: u32, mut picked: Vec<CardIdx>, mut early: Option<u32>) {
        let Some((p, card, cost)) = self.item_cost(id) else { return };
        let n_sac = cost.sacrifice.as_ref().map_or(0, |s| s.n) as usize;
        // (A second discard with a filter of its own first: retrace's land
        // card with the card's own discard, docs/shortcuts.md 24.7.5k.)
        let n_also = n_sac + cost.discard_also.as_ref().map_or(0, |d| d.0) as usize;
        let n_disc = n_also + cost.discard as usize;
        let n_exile = n_disc + cost.exile_from_hand.is_some() as usize;
        let n_bounce = n_exile + cost.bounce.as_ref().map_or(0, |b| b.n) as usize;
        let n_gyc = cost.exile_graveyard as usize;
        let n_taps = cost.tap_untapped.as_ref().map_or(0, |t| t.n) as usize + self.tap_x_n(id, cost);
        // "Remove a counter from a <permanent> you control": one choice
        // (never with crew, whose picks come last: lowering).
        let n_rcs = cost.remove_counter_from.is_some() as usize;
        // The sacrifice comes before the graveyard cards (CR 601.2h).
        let late = n_sac > 0 && n_gyc > 0;
        // Where each part's picks are: the graveyard cards' range, the taps',
        // then the counter's (`n_tap .. n_rc`) and crew's (from `n_rc`).
        let (gy_r, tap_r) = if late {
            let gy = early.map_or(usize::MAX..usize::MAX, |e| e as usize..e as usize + n_gyc);
            (gy, n_bounce..n_bounce + n_taps)
        } else {
            (n_bounce..n_bounce + n_gyc, n_bounce + n_gyc..n_bounce + n_gyc + n_taps)
        };
        let n_tap = tap_r.end;
        let n_rc = n_tap + n_rcs;
        let mut crew_done = false;
        // The cost is one event (CR 603.2c; its discards, exiles): a group of
        // its own. (Not again when its first choice shows another page of
        // its permanents, ENGINE.md 100.4.8.)
        if picked.is_empty() && self.answer.is_none() && self.page.is_none() {
            self.new_event();
            self.pay_exile_granter(id);
        }
        // A permanent to return or sacrifice, its mana ability activated
        // first (ENGINE.md 86.2).
        let mut float = None;
        if let Some(ch) = self.answer.take() {
            let c = match ch {
                Choice::Card(c) => c,
                Choice::Ability(c, slot) => {
                    float = Some(slot);
                    c
                }
                // CR 702.122a: enough power tapped for crew.
                Choice::Pass => {
                    crew_done = true;
                    CardIdx::new(0)
                }
                other => unreachable!("cost answer {other:?}"),
            };
            if crew_done {
            } else if picked.len() < n_sac {
                picked.push(c);
            } else if picked.len() < n_disc {
                // Discards happen one at a time: a chosen hand card is never
                // left hidden while another decision is pending.
                self.discard(c);
                picked.push(c);
            } else if picked.len() < n_exile {
                // Likewise the card exiled from hand (revealed as it moves).
                self.move_card(c, Zone::Exile, Pos::Top);
                picked.push(c);
            } else {
                picked.push(c);
            }
            if let Some(slot) = float {
                // CR 601.2g before 601.2h: the mana ability now, its mana to
                // the pool (it stays there until the step ends); then the
                // rest of the cost as chosen.
                debug_assert!(picked.len() <= n_sac || (n_exile < picked.len() && picked.len() <= n_bounce));
                self.agenda.push(Task::PayCosts { id, picked, early, order: None });
                self.begin_activate(p, c, slot);
                return;
            }
        }
        if picked.len() < n_sac {
            let s = cost.sacrifice.as_ref().unwrap();
            // A cost that also exiles graveyard cards: while the graveyard
            // (with the cost's discards and the cards sacrificed so far)
            // would be short by as many cards as sacrifices are left, only
            // a card that will be there helps (ruling bea302d9).
            let need = if cost.exile_graveyard > 0 {
                let fed = picked.iter().filter(|&&c| self.feeds_graveyard(p, c) && self.gy_cost_card(cost, c)).count()
                    + self.discards_feed(p, card, cost);
                self.exile_short(p, card, cost).saturating_sub(fed)
            } else {
                0
            };
            let only_cards = need > 0 && need >= n_sac - picked.len();
            // (Each pick leaves the cost's other parts that take permanents or
            // cards in hand their candidates: ENGINE.md 100.4.7.)
            let cands: Vec<CardIdx> = self
                .sacrifice_candidates(p, card, s)
                .into_iter()
                .filter(|c| !picked.contains(c) && (!only_cards || self.feeds_graveyard(p, *c) && self.gy_cost_card(cost, *c)))
                .filter(|&c| self.cost_picks_ok(p, card, cost, &picked, c))
                .collect();
            let opts = self.cost_pick_options(p, &cands, cost.life);
            self.ask(Task::PayCosts { id, picked, early, order: None }, p, DecisionKind::Sacrifice, opts);
            return;
        }
        if picked.len() < n_disc {
            let mut opts = self.hand_options_except(p, Some(card));
            let filter = if picked.len() < n_also { cost.discard_also.as_ref().map(|d| &d.1) } else { cost.discard_filter.as_ref() };
            if let Some(f) = filter {
                let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
                opts.retain(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(f, c, &ctx)));
            }
            opts.retain(|o| matches!(o.choice, Choice::Card(c) if self.cost_picks_ok(p, card, cost, &picked, c)));
            // A cost that also exiles graveyard cards (escape): while the
            // graveyard (the discards so far in it, the sacrifices chosen to
            // come) would be short by as many cards as discards are left,
            // only a card that goes there helps (not Blightsteel Colossus;
            // ENGINE.md 49, as the sacrifices above).
            if cost.exile_graveyard > 0 {
                let fed = picked[..n_sac].iter().filter(|&&c| self.feeds_graveyard(p, c) && self.gy_cost_card(cost, c)).count();
                let need = self.exile_short(p, card, cost).saturating_sub(fed);
                if need > 0 && need >= n_disc - picked.len() {
                    opts.retain(|o| matches!(o.choice, Choice::Card(c) if self.discard_feeds(p, card, cost, c)));
                }
            }
            self.ask(Task::PayCosts { id, picked, early, order: None }, p, DecisionKind::DiscardCost, opts);
            return;
        }
        if picked.len() < n_exile {
            let f = cost.exile_from_hand.as_ref().unwrap();
            let mut opts = self.exile_candidates(p, card, f);
            opts.retain(|o| matches!(o.choice, Choice::Card(c) if self.cost_picks_ok(p, card, cost, &picked, c)));
            self.ask(Task::PayCosts { id, picked, early, order: None }, p, DecisionKind::ExileCost, opts);
            return;
        }
        if picked.len() < n_bounce {
            let b = cost.bounce.as_ref().unwrap();
            // (Not a permanent picked to be sacrificed: it is returned first.)
            let cands: Vec<CardIdx> = self
                .sacrifice_candidates(p, card, b)
                .into_iter()
                .filter(|c| !picked.contains(c) && self.cost_picks_ok(p, card, cost, &picked, *c))
                .collect();
            let opts = self.cost_pick_options(p, &cands, cost.life);
            self.ask(Task::PayCosts { id, picked, early, order: None }, p, DecisionKind::BounceCost, opts);
            return;
        }
        if !late && picked.len() < gy_r.end {
            self.ask_cost_graveyard_cards(p, id, card, cost, picked, gy_r.start, early);
            return;
        }
        if early.is_none() && picked.len() < n_tap {
            // "Tap n untapped <permanents> you control" ("tap X ..."): one
            // decision each (a set: increasing slot order).
            let tx;
            let t = match (&cost.tap_untapped, &cost.tap_x) {
                (Some(t), _) => t,
                (None, Some(f)) => {
                    tx = crate::db::SacCost { filter: f.clone(), n: 0, another: true };
                    &tx
                }
                (None, None) => unreachable!("taps owed without a tap cost"),
            };
            let floor = picked[tap_r.start..].last().map(|&c| self.perm_slot(c));
            // (A permanent picked to be sacrificed or returned is still
            // there, and may be tapped first: CR 601.2h, the parts in any
            // order; the taps come before the sacrifices and returns.)
            let cands: Vec<CardIdx> = self
                .tap_candidates(p, card, t)
                .filter(|c| !picked[tap_r.start..].contains(c))
                .filter(|&c| floor.is_none_or(|f| self.perm_slot(c) > f))
                .collect();
            // (Leaving enough later candidates for the taps still owed.)
            let still = n_tap - picked.len();
            let opts: Vec<Opt> = cands
                .iter()
                .enumerate()
                .filter(|(k, _)| cands.len() - k >= still)
                .map(|(_, &c)| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                .collect();
            self.ask(Task::PayCosts { id, picked, early, order: None }, p, DecisionKind::ChoosePermanent, opts);
            return;
        }
        if early.is_none() && picked.len() < n_rc {
            // "Remove a <k> counter from a <permanent> you control" (CR
            // 118.3): the permanent.
            let (f, k) = cost.remove_counter_from.as_ref().unwrap();
            let opts: Vec<Opt> = self
                .remove_counter_candidates(p, card, f, *k)
                .into_iter()
                .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                .collect();
            self.ask(Task::PayCosts { id, picked, early, order: None }, p, DecisionKind::ChoosePermanent, opts);
            return;
        }
        if cost.crew > 0 && !crew_done && early.is_none() {
            // Crew (CR 702.122a): untapped creatures other than the source,
            // one decision each in increasing slot order; PASS once their
            // total power is enough. A pick must leave the total reachable.
            let need = cost.crew as crate::num::Wide;
            let bonus = !cost.saddle && !self.is_spell_card(card);
            let tapped_in: Vec<CardIdx> = picked[n_rc..].to_vec();
            let have: crate::num::Wide = tapped_in.iter().map(|&c| self.crew_power_of(c, bonus)).sum();
            let floor = tapped_in.last().map(|&c| self.perm_slot(c));
            let cands: Vec<CardIdx> = self
                .crew_eligible(p, card)
                .into_iter()
                .filter(|c| !tapped_in.contains(c) && floor.is_none_or(|f| self.perm_slot(*c) > f))
                .collect();
            // Teamwork: the spell's mana is paid next (`cast.rs`); every
            // choice leaves it payable without the creatures tapped.
            let spell_mana = if self.is_spell_card(card) {
                self.agenda.iter().rev().find_map(|t| match t {
                    Task::PayMana(pm) => Some((pm.cost, pm.keep.clone())),
                    _ => None,
                })
            } else {
                None
            };
            let classes = if spell_mana.is_some() { self.mana_classes(p, None) } else { Vec::new() };
            let mut opts: Vec<Opt> = Vec::new();
            for (k, &c) in cands.iter().enumerate() {
                let ok = match &spell_mana {
                    Some((mana, keep)) => {
                        let mut t = tapped_in.clone();
                        t.push(c);
                        self.crew_mana_ok(p, need, &t, &cands[k + 1..], mana, &classes, keep)
                    }
                    None => {
                        let rest: crate::num::Wide = cands[k + 1..].iter().map(|&x| self.crew_power_of(x, bonus).max(0)).sum();
                        have + self.crew_power_of(c, bonus) + rest >= need
                    }
                };
                if ok {
                    opts.push(Opt { action: self.perm_action(p, c), choice: Choice::Card(c) });
                }
            }
            let done = match &spell_mana {
                Some((mana, keep)) => self.crew_mana_ok(p, need, &tapped_in, &[], mana, &classes, keep),
                None => have >= need,
            };
            if done {
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
            }
            if !(opts.len() == 1 && done) {
                self.ask(Task::PayCosts { id, picked, early, order: None }, p, DecisionKind::ChoosePermanent, opts);
                return;
            }
        }
        // The taps, the counter and crew are paid, then (`late`) the
        // sacrifice, before the graveyard cards are chosen (CR 601.2h).
        if early.is_none() {
            let crew: Vec<CardIdx> = picked[n_rc..].to_vec();
            self.pay_tap_parts(id, card, cost, &picked[tap_r.clone()], &crew, picked.get(n_tap).filter(|_| n_rcs > 0).copied());
            if late {
                self.sacrifice_picked(p, id, &picked[..n_sac]);
                early = Some(picked.len() as u32);
            }
        }
        let gy_r = if late { early.map_or(gy_r, |e| e as usize..e as usize + n_gyc) } else { gy_r };
        if late && picked.len() < gy_r.end {
            self.ask_cost_graveyard_cards(p, id, card, cost, picked, gy_r.start, early);
            return;
        }
        for &c in &picked[gy_r] {
            self.move_card(c, Zone::Exile, Pos::Top);
        }
        if let Some((k, n)) = cost.remove_counters {
            self.remove_counters(card, k, n as crate::num::Count);
        }
        // Permanents returned as a cost leave together. An attacking
        // creature returned for ninjutsu leaves what it attacked to the
        // creature put onto the battlefield (CR 702.49c).
        let mut bounced: Vec<CardIdx> = picked[n_exile..n_bounce].to_vec();
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
        // Everything chosen: pay. Sacrificed permanents leave together.
        if cost.tap {
            self.tap(card);
        }
        if !late {
            self.sacrifice_picked(p, id, &picked[..n_sac]);
        }
        if cost.life > 0 {
            // (At most the life total, an i32: CR 119.4 was checked.)
            self.lose_life(p, i32::try_from(cost.life).expect("life within the life total"));
        }
        if cost.discard_hand {
            for h in self.hand_sorted(p) {
                if h != card {
                    self.discard(h);
                }
            }
        }
        if cost.sac_self {
            self.sacrifice(card);
        }
        if cost.discard_self {
            self.discard(card);
        }
        if cost.exile_self {
            self.move_card(card, Zone::Exile, Pos::Top);
        }
        // CR 606.4: a loyalty ability's cost puts on or removes loyalty
        // counters.
        if let Some((_, src, ad)) = self.act_item(id) {
            match ad.loyalty {
                Some(d) if d > 0 => self.add_counters(src, crate::db::Counter::Loyalty, d as crate::num::Count),
                Some(d) if d < 0 => {
                    self.remove_counters(src, crate::db::Counter::Loyalty, (-(d as i64)) as crate::num::Count);
                }
                _ => {}
            }
        }
    }

    /// `pay_costs`' taps of "tap n untapped <permanents>" (`taps`), then
    /// crew's (`crew`, a spell's teamwork committed: its mana is paid after),
    /// the counter removed from `rc`, and the tapped creature noted for an
    /// ability that reads it (station).
    pub(crate) fn pay_tap_parts(&mut self, id: u32, card: CardIdx, cost: &Cost, taps: &[CardIdx], crew: &[CardIdx], rc: Option<CardIdx>) {
        for &c in taps.iter().chain(crew) {
            self.tap(c);
        }
        // Teamwork's creatures, tapped before the spell's mana in the
        // engine's order: by the rules after its mana abilities (CR 601.2g
        // before 601.2h), so none of them is a mana source of it
        // (`Keep::committed`; docs/shortcuts.md 13.17).
        if cost.crew > 0 && self.is_spell_card(card) && !crew.is_empty() {
            if let Some(Task::PayMana(pm)) = self.agenda.iter_mut().rev().find(|t| matches!(t, Task::PayMana(_))) {
                pm.keep.committed.extend_from_slice(crew);
            }
        }
        if let (Some((_, k)), Some(c)) = (&cost.remove_counter_from, rc) {
            self.remove_counters(c, *k, 1);
        }
        // "The tapped creature" of an activated ability's "tap an untapped
        // creature you control" cost (station, CR 702.184a): the stack item
        // notes it (`Amount::TappedPower`).
        if !taps.is_empty() {
            if let Some(pos) = self.item_index(id) {
                if matches!(self.stack[pos].kind, StackKind::Ability { .. }) {
                    let tapped: Vec<crate::ids::ObjRef> = taps.iter().map(|&c| self.obj_ref(c)).collect();
                    self.stack[pos].noted = tapped;
                }
            }
        }
    }

    /// `pay_costs`' choice of a graveyard card to exile for "exile n cards
    /// from your graveyard": a set picked in increasing position from
    /// `picked[from..]`, leaving enough later ones (cards only: not the
    /// source, not a token, CR 108.2).
    #[allow(clippy::too_many_arguments)]
    fn ask_cost_graveyard_cards(&mut self, p: PlayerId, id: u32, card: CardIdx, cost: &Cost, picked: Vec<CardIdx>, from: usize, early: Option<u32>) {
        let gy: Vec<CardIdx> = self.graveyard_cards(p).filter(|&c| c != card && self.gy_cost_card(cost, c)).collect();
        let after = picked[from..].last().map(|c| gy.iter().position(|x| x == c).unwrap() + 1).unwrap_or(0);
        let still = from + cost.exile_graveyard as usize - picked.len();
        let opts: Vec<Opt> = gy
            .iter()
            .enumerate()
            .filter(|&(i, _)| i >= after && gy.len() - i >= still)
            .map(|(_, &c)| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) })
            .collect();
        self.ask(Task::PayCosts { id, picked, early, order: None }, p, DecisionKind::ChooseGraveyardCard, opts);
    }

    /// Is mana ability `ad` of `c` offered with priority (ENGINE.md 18)? One
    /// that may be activated only as an instant (Lion's Eye Diamond: never
    /// while paying), or one that sacrifices its source when the source has
    /// a triggered ability of its own as it goes (Chromatic Star, Wizard's
    /// Rockets: "when this is put into a graveyard, draw a card"; activated
    /// for nothing but that, CR 605.3a). Every other mana ability is
    /// activated while paying a cost only - floating its mana first can't
    /// matter (the pool empties between steps, CR 106.4, 500.4) - but for a
    /// source about to be lost (`mana_at_priority`, ENGINE.md 86).
    pub(crate) fn mana_with_priority(&self, c: CardIdx, ad: &ActivatedDef) -> bool {
        fn is_self(p: &crate::db::TrigPred) -> bool {
            match p {
                crate::db::TrigPred::IsSelf(_) => true,
                crate::db::TrigPred::And(a, b) => is_self(a) || is_self(b),
                _ => false,
            }
        }
        ad.mana().is_some()
            && (ad.instant_only
                || (ad.cost.sac_self
                    && self.def_of(c).triggers.iter().any(|t| {
                        matches!(t.event, crate::db::EventKind::Dies | crate::db::EventKind::LeavesBattlefield) && is_self(&t.pred)
                    })))
    }

    /// Is mana ability `ad` of permanent `c` offered with priority now: one
    /// `mana_with_priority` names, or any mana ability of a source about to
    /// be lost (`floats_in_response`)?
    pub(crate) fn mana_at_priority(&self, c: CardIdx, ad: &ActivatedDef) -> bool {
        self.mana_with_priority(c, ad) || self.floats_in_response(c)
    }

    /// Floating mana in response (ENGINE.md 86.1; docs/shortcuts.md 13.12):
    /// permanent `c` is untapped and an object on the stack targets it (a
    /// Wasteland's ability at a Volcanic Island, a Lightning Bolt at a Birds
    /// of Paradise). Its controller may then activate its mana abilities
    /// with priority (CR 605.3a, 117.1d): the mana stays in the pool until
    /// the step ends (106.4, 500.4), to be spent after the source is gone.
    /// Every other priority leaves mana abilities to the payments (ENGINE.md
    /// 18): floating mana there is the same as activating the ability while
    /// paying later in the step.
    pub(crate) fn floats_in_response(&self, c: CardIdx) -> bool {
        if self.stack.is_empty() || self.objs[c.i()].tapped {
            return false;
        }
        let t = Target::Obj(self.obj_ref(c));
        self.stack.iter().any(|s| s.targets.iter().any(|&(_, x)| x == t))
    }

    /// The mana abilities of permanent `c` that `p` may activate before a
    /// cost returns or sacrifices `c` (ENGINE.md 86.2; CR 601.2g comes before
    /// 601.2h): Daze's Island tapped for {U}, then returned. Only an untapped
    /// source's abilities whose cost is {T} (and life: `life_after` more is
    /// paid by the cost afterwards, CR 119.4), so the activation leaves `c`
    /// where the cost needs it and nothing else the cost chooses from
    /// changes. A cost that taps the source itself (a fetch land, Wasteland:
    /// the source of the ability) is not one of these: its source isn't
    /// picked.
    pub(crate) fn float_before_cost(&self, p: PlayerId, c: CardIdx, life_after: u32) -> crate::svec::SVec<u8, 4> {
        let mut out = crate::svec::SVec::new();
        let o = &self.objs[c.i()];
        if o.zone != Zone::Battlefield || o.controller != p || o.tapped {
            return out;
        }
        let d = self.abilities(c);
        let locks = self.activation_locks();
        let life = self.players[p as usize].life;
        for (i, ad) in d.activated.iter().enumerate() {
            let Some(m) = ad.mana() else { continue };
            let k = &ad.cost;
            let only_tap = k.tap
                && k.mana.is_zero()
                && !ad.x
                && !(k.sac_self || k.exile_self || k.discard_self || k.discard_hand || k.exert || k.waterbend)
                && k.sacrifice.is_none()
                && k.bounce.is_none()
                && k.discard == 0
                && k.exile_from_hand.is_none()
                && k.exile_graveyard == 0
                && k.remove_counters.is_none()
                && k.tap_untapped.is_none()
                && k.crew == 0
                && m.sac_if_no.is_none()
                && !matches!(m.prod, crate::db::ManaProd::SacrificedLand);
            if !only_tap || ad.special.is_some() || ad.instant_only {
                continue;
            }
            if (d.latent > 0 && !self.has_activated(c, i)) || (ad.exhaust && self.exhausted(c, i as u8)) {
                continue;
            }
            if !self.activation_ready(p, c, ad, locks) {
                continue;
            }
            // CR 119.4: its life, and the cost's life after it (with a
            // painland's damage first).
            // (In `Wide` with the life total: exact.)
            let (life, spent) = (life as crate::num::Wide, k.life as crate::num::Wide + if life_after > 0 { m.damage_to_you as crate::num::Wide } else { 0 });
            if (k.life as crate::num::Wide) > life || (life_after > 0 && life - spent < life_after as crate::num::Wide) {
                continue;
            }
            if productions(self, p, &m.prod, Some(c)).is_empty() {
                continue;
            }
            out.push(i as u8);
        }
        out
    }

    /// Has `c` activated its exhaust ability `i` (CR 702.177a)?
    pub(crate) fn exhausted(&self, c: CardIdx, i: u8) -> bool {
        let r = self.obj_ref(c);
        self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::Exhausted(i))
    }

    /// A mana ability activated with priority ("activate only as an
    /// instant", Lion's Eye Diamond; one whose cost has a mana part or a
    /// choice, ENGINE.md 28): pay its costs (the mana first, CR 601.2g-h by
    /// 602.2b), then choose the mana (CR 605.3b, 106.1b).
    pub(crate) fn mana_now(&mut self, src: Src, step: crate::decision::NumberStep) {
        let p = self.objs[src.card.i()].controller;
        let ad = self.src_ability(src.card, src.slot);
        // "{X}, {T}, ...: Add X mana" (CR 107.3, 602.2b: X is announced as
        // the ability is activated): as much as can be paid, not with the
        // source's own mana. Choice: `ChooseMode`, `Game::ask_amount` (any
        // X it can pay, digits past 31: ENGINE.md 100.2, 100.4.8).
        let x = if ad.x {
            let exclude = Self::mana_exclude(src.card, ad);
            let classes = self.mana_classes(p, exclude);
            let keep = Keep { allow: self.ability_allow(src.card), ..Self::keep_for(src.card, &ad.cost, exclude) };
            let most = crate::decision::most_allowed(0, u32::MAX as u64, |x| {
                let m = self.act_mana_x(p, src.card, ad, x as u32);
                self.can_pay_without(p, &m, &classes, &keep, exclude)
            });
            match self.answer.take() {
                Some(Choice::Number(k)) => match step.answer_amount(0, most, k) {
                    crate::decision::NumberAnswer::Next(s) => {
                        self.agenda.push(Task::ManaNow { src, step: s });
                        return;
                    }
                    crate::decision::NumberAnswer::Value(x) => {
                        self.note(|_| format!("X = {x}"));
                        x as u32
                    }
                },
                Some(other) => unreachable!("X answer {other:?}"),
                None => {
                    self.ask_amount(Task::ManaNow { src, step }, step, p, DecisionKind::ChooseMode, 0, most);
                    return;
                }
            }
        } else {
            0
        };
        let mana = self.act_mana_x(p, src.card, ad, x);
        self.agenda.push(Task::ManaNowCosts { src, sac: None, x, taps: Vec::new() });
        if !mana.is_zero() {
            let mut pm = PayMana::new(p, mana);
            pm.keep = Self::keep_for(src.card, &ad.cost, Self::mana_exclude(src.card, ad));
            pm.keep.allow = self.ability_allow(src.card);
            self.agenda.push(Task::PayMana(Box::new(pm)));
        }
    }

    /// The rest of a priority mana ability's cost: the permanent to
    /// sacrifice (a decision), then {T}, life, the source's own costs.
    pub(crate) fn mana_now_costs(&mut self, src: Src, mut sac: Option<CardIdx>, x: u32, mut taps: Vec<CardIdx>) {
        let p = self.objs[src.card.i()].controller;
        let ad = self.src_ability(src.card, src.slot);
        if let Some(s) = &ad.cost.sacrifice {
            if sac.is_none() {
                match self.answer.take() {
                    Some(Choice::Card(c)) => sac = Some(c),
                    Some(other) => unreachable!("sacrifice answer {other:?}"),
                    None => {
                        let opts: Vec<Opt> = self
                            .sacrifice_candidates(p, src.card, s)
                            .into_iter()
                            .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                            .collect();
                        self.ask(Task::ManaNowCosts { src, sac, x, taps }, p, DecisionKind::Sacrifice, opts);
                        return;
                    }
                }
            }
        }
        // "Tap an untapped <permanent> you control" (Urza; Gene Pollinator's
        // with {T}): which ones, one decision each (a set: increasing slot
        // order), as a non-mana ability's (CR 601.2h by 602.2b).
        if let Some(t) = &ad.cost.tap_untapped {
            if let Some(Choice::Card(c)) = self.answer {
                if taps.len() < t.n as usize && !taps.contains(&c) {
                    self.answer = None;
                    taps.push(c);
                }
            }
            if taps.len() < t.n as usize {
                let floor = taps.last().map(|&c| self.perm_slot(c));
                let cands: Vec<Opt> = self
                    .tap_candidates(p, src.card, t)
                    .filter(|c| !taps.contains(c) && Some(*c) != sac)
                    .filter(|&c| floor.is_none_or(|f| self.perm_slot(c) > f))
                    .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                    .collect();
                let still = t.n as usize - taps.len();
                let opts: Vec<Opt> = cands.iter().enumerate().filter(|(k, _)| cands.len() - k >= still).map(|(_, o)| *o).collect();
                self.ask(Task::ManaNowCosts { src, sac, x, taps }, p, DecisionKind::ChoosePermanent, opts);
                return;
            }
        }
        // What it can make is read before the costs are paid; "any type the
        // sacrificed land could produce" from that land as it is (CR 106.7).
        let prod = &ad.mana().unwrap().prod;
        let prods: Vec<[u8; 6]> = match (prod, sac) {
            (crate::db::ManaProd::SacrificedLand, Some(land)) => {
                let m = self.could_produce(land);
                (0..6).filter(|&t| m & (1 << t) != 0).map(|t| std::array::from_fn(|i| (i == t) as u8)).collect()
            }
            _ => {
                // (F7: mana from counters past the pool's u8 aborts.)
                self.check_counter_mana(src.card, prod);
                productions(self, p, prod, Some(src.card))
            }
        };
        // "Whenever you tap a creature for mana, add an additional {G}",
        // "whenever you tap a <land subtype> for mana, add an additional
        // <mana>" (triggered mana abilities, CR 605.1b, 605.4a: they resolve
        // at once, so the mana is added with the ability's; as in a payment,
        // `Game::mana_classes`).
        let mut prods = prods;
        if ad.cost.tap {
            let mut more = if self.is_creature(src.card) { self.creature_mana_bonus(p) } else { [0; 6] };
            if self.objs[src.card.i()].zone == Zone::Battlefield && self.is_type(src.card, types::LAND) {
                for x in self.permanents_of(p) {
                    for &(sub, t) in &self.abilities(x).extra_mana_on_tap {
                        if self.has_subtype(src.card, sub) {
                            more[t as usize] += 1;
                        }
                    }
                }
            }
            if more != [0; 6] {
                for v in prods.iter_mut() {
                    for t in 0..6 {
                        // (F7: past the pool's u8 the game aborts.)
                        v[t] = match v[t].checked_add(more[t]) {
                            Some(n) => n,
                            None => {
                                self.abort(|| "more than 255 mana of one type from one ability".to_string());
                                u8::MAX
                            }
                        };
                    }
                }
            }
        }
        if let Some(c) = sac {
            self.sacrifice(c);
        }
        for &c in &taps {
            // (A cost of the mana ability: not "tapped for mana", CR
            // 106.12a.)
            self.note(|g| format!("P{p} taps {} for a mana ability", g.name(c)));
            self.tap(c);
        }
        self.activate_mana_costs(src);
        // Mana that gives haste to the creature spell it is spent on (Arena
        // of Glory: one fixed production, the lowering checks).
        if ad.mana().is_some_and(|m| m.haste) && prods.len() == 1 {
            let v = prods[0];
            self.add_mana(p, &v);
            let pool = &mut self.players[p as usize].pool;
            // (Exact: at most the red mana in the pool, a u8.)
            pool.haste = (pool.haste as u16 + v[crate::mana::R] as u16).min(pool.amt[crate::mana::R] as u16) as u8;
            return;
        }
        if !prods.is_empty() {
            // X mana in any combination: one mana, one choice, X times.
            let picks = if matches!(prod, crate::db::ManaProd::ComboX { .. }) { x } else { 1 };
            for _ in 0..picks {
                // (Its restriction and its rider go with the mana, CR 106.6.)
                let only = ad.mana().map_or(0, |m| m.only | if m.uncounterable { crate::db::MANA_UNCOUNTERABLE } else { 0 });
                self.agenda.push(Task::ManaNowPick { p, prods: prods.clone(), only, page: crate::decision::Page::FIRST });
            }
        }
    }

    /// The types of mana permanent `c`'s mana abilities could make now
    /// (CR 106.7): the abilities it has (not a latent one it hasn't gained:
    /// Urza's Saga before its chapter I; ruling a755e0f5), those other
    /// permanents grant it and the intrinsic ones of basic land types an
    /// effect gave it (`Game::granted_activated`; ENGINE.md 100.4.5).
    pub(crate) fn could_produce(&self, c: CardIdx) -> crate::mana::ManaMask {
        let p = self.objs[c.i()].controller;
        let d = self.abilities(c);
        let granted = if self.grants_activated_now() { self.granted_activated(c) } else { Vec::new() };
        d.activated
            .iter()
            .enumerate()
            .filter(|&(i, _)| d.latent == 0 || self.has_activated(c, i))
            .map(|(_, a)| a)
            .chain(granted.iter().map(|g| self.db.def(g.def).activated_def(crate::db::AbilitySlot::Activated(g.slot))))
            .filter_map(|a| a.mana())
            .flat_map(|m| productions(self, p, &m.prod, Some(c)))
            .fold(0, |m, v| (0..6).filter(|&t| v[t] > 0).fold(m, |m, t| m | (1 << t)))
    }

    pub(crate) fn mana_now_pick(&mut self, p: PlayerId, prods: Vec<[u8; 6]>, only: u8, page: crate::decision::Page) {
        match self.answer.take() {
            Some(Choice::Number(k)) => self.add_restricted_mana(p, &prods[k as usize], only),
            Some(Choice::Item(k)) => self.add_restricted_mana(p, &prods[k as usize], only),
            Some(Choice::Page(next)) => self.agenda.push(Task::ManaNowPick { p, prods, only, page: next }),
            Some(other) => unreachable!("mana answer {other:?}"),
            None => {
                if prods.len() == 1 {
                    self.add_restricted_mana(p, &prods[0], only);
                    return;
                }
                if prods.len() > act::NUMBER_N as usize {
                    // (More productions than `NUMBER` ids, Cascading
                    // Cataracts' 126: paged, A0's helper.)
                    let items: Vec<(u32, Choice)> = (0..prods.len() as u32).map(|k| (k, Choice::Item(k))).collect();
                    self.ask_paged(Task::ManaNowPick { p, prods, only, page }, page, p, DecisionKind::ChooseMana, act::NUMBER, act::NUMBER_N, &items, Vec::new());
                    return;
                }
                let opts = (0..prods.len())
                    .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) })
                    .collect();
                self.ask(Task::ManaNowPick { p, prods, only, page }, p, DecisionKind::ChooseMana, opts);
            }
        }
    }

    /// "As this enters, you may pay N life. If you don't, it enters tapped"
    /// (CR 614.12). Asked right after the land is put onto the battlefield,
    /// before anything else happens (nothing can observe the order).
    pub(crate) fn as_enters(&mut self, card: CardIdx, page: crate::decision::Page) {
        let o = &self.objs[card.i()];
        if o.zone != Zone::Battlefield {
            return;
        }
        let p = o.controller;
        // "As this enters, choose a color" (EoE W6).
        if self.def_of(card).w6.chooses_color && self.chosen_land_color(card) == 0 {
            self.w6_choose_color_as_enters(card, p);
            return;
        }
        if self.def_of(card).chooses_player && self.objs[card.i()].chosen_player.is_none() {
            // "As this enters, choose a player." Choice: `PLAYER` (2 = you,
            // 3 = the opponent).
            match self.answer.take() {
                Some(Choice::Target(Target::Player(q))) => {
                    self.objs[card.i()].chosen_player = Some(q);
                    self.note(|_| format!("chosen player: P{q}"));
                }
                Some(other) => unreachable!("player answer {other:?}"),
                None => {
                    let opts = vec![
                        Opt { action: act::PLAYER, choice: Choice::Target(Target::Player(p)) },
                        Opt { action: act::PLAYER + 1, choice: Choice::Target(Target::Player(crate::ids::opp(p))) },
                    ];
                    self.ask(Task::AsEnters { card, page: crate::decision::Page::FIRST }, p, DecisionKind::Target, opts);
                }
            }
            return;
        }
        if self.def_of(card).chooses_land_type && self.chosen_land_color(card) == 0 {
            // "As this land enters, choose a basic land type" (CR 614.12):
            // index W U B R G = Plains Island Swamp Mountain Forest. "This
            // land is the chosen type" (its subtype; its mana ability is
            // `ManaProd::Chosen`). Then the life question below. Choice:
            // `ChooseColor`.
            let r = self.obj_ref(card);
            match self.answer.take() {
                Some(Choice::Number(k)) => {
                    const BASICS: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];
                    self.marks.push((r, crate::game::Mark::ChosenColor(k)));
                    if let Some(s) = self.db.subtype_id(BASICS[k as usize]) {
                        let m = crate::db::Mod::Types { add: 0, subtypes: vec![s], replace: false };
                        self.add_effect(crate::layers::Affected::Obj(r), m, crate::layers::Until::Forever, p);
                    }
                    self.note(|_| format!("chosen basic land type: {}", BASICS[k as usize]));
                    self.agenda.push(Task::AsEnters { card, page: crate::decision::Page::FIRST });
                }
                Some(other) => unreachable!("land type answer {other:?}"),
                None => {
                    let opts = (0..5u16).map(|k| Opt { action: act::NUMBER + k, choice: Choice::Number(k as u8) }).collect();
                    self.ask(Task::AsEnters { card, page: crate::decision::Page::FIRST }, p, DecisionKind::ChooseColor, opts);
                }
            }
            return;
        }
        if self.def_of(card).chooses_parity && self.parity_of(self.obj_ref(card)).is_none() {
            // "As Gollum enters, choose odd or even" (CR 614.12). Choice:
            // `ChooseMode`: `NUMBER` 0 = even, 1 = odd.
            let r = self.obj_ref(card);
            match self.answer.take() {
                Some(Choice::Number(k)) => {
                    self.marks.push((r, crate::game::Mark::Parity(k)));
                    self.note(|_| format!("chosen: {}", if k == 1 { "odd" } else { "even" }));
                }
                Some(other) => unreachable!("parity answer {other:?}"),
                None => {
                    let opts = (0..2u16).map(|k| Opt { action: act::NUMBER + k, choice: Choice::Number(k as u8) }).collect();
                    self.ask(Task::AsEnters { card, page: crate::decision::Page::FIRST }, p, DecisionKind::ChooseMode, opts);
                }
            }
            return;
        }
        if self.def_of(card).chooses_name && self.chosen_name(self.obj_ref(card)).is_none() {
            self.choose_name_as_enters(card, page);
            return;
        }
        if self.def_of(card).paints {
            // "As this enters, choose a color" (CR 614.12). Choice:
            // `ChooseColor`.
            let r = self.obj_ref(card);
            match self.answer.take() {
                Some(Choice::Number(k)) => {
                    self.marks.push((r, crate::game::Mark::ChosenColor(k)));
                    self.note(|_| format!("chosen colour: {}", crate::mana::LETTERS[k as usize]));
                }
                Some(other) => unreachable!("colour answer {other:?}"),
                None => {
                    let opts = (0..5u16).map(|k| Opt { action: act::NUMBER + k, choice: Choice::Number(k as u8) }).collect();
                    self.ask(Task::AsEnters { card, page: crate::decision::Page::FIRST }, p, DecisionKind::ChooseColor, opts);
                }
            }
            return;
        }
        let n = self.def_of(card).shock_life as i64;
        if n == 0 {
            return;
        }
        match self.answer.take() {
            Some(Choice::Yes) => {
                self.note(|_| format!("P{p} pays {n} life"));
                self.lose_life(p, n);
            }
            Some(_) => self.tap(card),
            None => {
                let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                if self.players[p as usize].life >= n {
                    opts.push(Opt { action: act::YES, choice: Choice::Yes }); // CR 119.4
                }
                self.ask(Task::AsEnters { card, page: crate::decision::Page::FIRST }, p, DecisionKind::YesNo, opts);
            }
        }
    }

    /// The parity chosen as `r` entered (1 = odd), if any.
    pub(crate) fn parity_of(&self, r: crate::ids::ObjRef) -> Option<u8> {
        self.marks.iter().find_map(|&(o, m)| match m {
            crate::game::Mark::Parity(k) if o == r => Some(k),
            _ => None,
        })
    }

    /// Would a permanent with this definition enter tapped for `p` (CR
    /// 614.1c)? "Other lands" are the lands already there: not those
    /// entering at the same time, nor their effects (CR 614.12a; the fast
    /// lands' rulings: "it doesn't take those lands into consideration";
    /// `entry_since`).
    pub(crate) fn enters_tapped(&self, card: CardIdx, p: PlayerId) -> bool {
        self.enters_tapped_own(card, p, true)
    }

    /// [`enters_tapped`](Self::enters_tapped); without `own`, its own "enters
    /// tapped" doesn't apply (it enters with no abilities, CR 614.12).
    pub(crate) fn enters_tapped_own(&self, card: CardIdx, p: PlayerId, own: bool) -> bool {
        // "<Types> / nonbasic lands your opponents control enter tapped" (a
        // permanent already there, CR 614.12a).
        let d = self.def_of(card);
        let q = crate::ids::opp(p);
        let since = self.entry_since();
        let forced = self.permanents_of(q).any(|c| {
            let od = self.abilities(c);
            self.objs[c.i()].timestamp < since
                && (od.opp_enter_tapped & d.types != 0
                    || (od.opp_nonbasic_enter_tapped
                        && d.is(crate::db::types::LAND)
                        && d.supertypes & crate::db::supertypes::BASIC == 0))
        });
        if forced {
            return true;
        }
        // (Not its own: it enters with no abilities, or as a Mountain, CR
        // 305.7, 614.12; ENGINE.md 99.12.)
        if !own || (self.moon_game && self.moon_entering(card)) {
            return false;
        }
        match &self.def_of(card).enters_tapped {
            crate::db::EntersTapped::No => false,
            crate::db::EntersTapped::Always => true,
            crate::db::EntersTapped::Unless(c) => !self.cond_before_entry(c, p, since),
        }
    }

    /// Condition `c` for `p` in the game as it was before the entry whose
    /// first timestamp is `since` (CR 614.12a): the permanents entering at
    /// the same time (timestamp `since` or later) are not there yet, nor
    /// their static abilities. Read on a copy of the game without them when
    /// there are any.
    fn cond_before_entry(&self, c: &Cond, p: PlayerId, since: u32) -> bool {
        if !self.battlefield.iter().any(|&x| self.objs[x.i()].timestamp >= since) {
            return self.cond_holds(c, p);
        }
        let mut before = self.clone();
        before.log = None;
        before.battlefield.retain(|&x| self.objs[x.i()].timestamp < since);
        before.cond_holds(c, p)
    }

    /// Is this permanent indestructible (CR 702.12)?
    pub(crate) fn indestructible(&self, c: CardIdx) -> bool {
        self.has_kw(c, Kw::Indestructible)
    }

    /// Lands among `p`'s permanents (tests and logs).
    pub fn lands_of(&self, p: PlayerId) -> impl Iterator<Item = CardIdx> + '_ {
        self.permanents_of(p).filter(move |&c| self.is_type(c, types::LAND))
    }
}

impl Game {
    /// Mana ability `ad` (a permanent's, or a card's in hand) of `card`,
    /// activated by `p` with priority now (CR 605.3a, 117.1d): its timing,
    /// its restrictions, its whole cost, and some mana to make (what a
    /// sacrificed land could make is read as it is sacrificed).
    fn mana_ability_now(&self, p: PlayerId, card: CardIdx, ad: &ActivatedDef, classes: &[Class], sorcery_ok: bool, locks: u16) -> bool {
        if ad.sorcery && !sorcery_ok || !self.activation_ready(p, card, ad, locks) {
            return false;
        }
        // (A "tap an untapped <permanent>" cost may tap one with a mana
        // ability when the ability has no mana to pay: `zero_mana_taps`.)
        if !self.cost_payable_with(p, card, &ad.cost, &self.act_mana(p, card, ad), classes, Self::mana_exclude(card, ad), true, self.ability_allow(card)) {
            return false;
        }
        let prod = &ad.mana().expect("a mana ability").prod;
        matches!(prod, crate::db::ManaProd::SacrificedLand) || !productions(self, p, prod, Some(card)).is_empty()
    }

    /// The mana abilities `p` may activate now through `ActivateMana` (CR
    /// 605.3a): for each permanent with some, in canonical order, its
    /// slots (its own abilities, those other permanents grant it, a granted
    /// "sacrifice this: add <mana>", `pay::SAC_FOR`); then each card in hand
    /// with one (a Spirit Guide's), one per definition. Not the ones
    /// offered at priority directly (`mana_at_priority`: Lion's Eye Diamond,
    /// a source about to be lost, ENGINE.md 86), so each activation is one
    /// action.
    pub(crate) fn mana_float_sources(&self, p: PlayerId, classes: &[Class], sorcery_ok: bool) -> Vec<(CardIdx, Vec<AbilitySlot>)> {
        let locks = self.activation_locks();
        let mut out: Vec<(CardIdx, Vec<AbilitySlot>)> = Vec::new();
        let rain = self.seen & crate::game::SEEN_SAC_MANA != 0 && self.effects.iter().any(|e| matches!(e.m, crate::db::Mod::GainSacMana(_)));
        for c in self.permanents_of(p) {
            let mut slots = Vec::new();
            let d = self.abilities(c);
            for (i, ad) in d.activated.iter().enumerate() {
                if ad.mana().is_none() || self.mana_at_priority(c, ad) {
                    continue;
                }
                if (d.latent > 0 && !self.has_activated(c, i)) || (ad.exhaust && self.exhausted(c, i as u8)) {
                    continue;
                }
                if self.mana_ability_now(p, c, ad, classes, sorcery_ok, locks) {
                    slots.push(AbilitySlot::Activated(i as u8));
                }
            }
            let own = self.own_ability_slots(c);
            for (j, g) in self.granted_activated(c).into_iter().enumerate() {
                let ad = self.db.def(g.def).activated_def(AbilitySlot::Activated(g.slot));
                if ad.mana().is_some() && !self.mana_at_priority(c, ad) && self.mana_ability_now(p, c, ad, classes, sorcery_ok, locks) {
                    slots.push(AbilitySlot::Activated((own + j) as u8));
                }
            }
            if rain && self.sac_mana_of(c) != 0 && !(locks != 0 && self.types_of(c) & locks != 0) {
                slots.push(crate::pay::SAC_FOR);
            }
            if !slots.is_empty() {
                out.push((c, slots));
            }
        }
        if self.players[p as usize].hand.iter().any(|&c| self.hand_mana_source(c)) {
            let mut defs = Vec::new();
            for c in self.hand_sorted_in(p) {
                let def = self.objs[c.i()].def;
                if defs.contains(&def) || !self.hand_mana_source(c) {
                    continue;
                }
                let slots: Vec<AbilitySlot> = self
                    .def_of(c)
                    .hand_abilities
                    .iter()
                    .enumerate()
                    .filter(|(_, ad)| ad.mana().is_some() && self.mana_ability_now(p, c, ad, classes, sorcery_ok, locks))
                    .map(|(i, _)| AbilitySlot::Hand(i as u8))
                    .collect();
                if !slots.is_empty() {
                    defs.push(def);
                    out.push((c, slots));
                }
            }
        }
        out
    }

    /// `ActivateMana` (CR 605.3a, 117.1d; ENGINE.md 100.4.3): `p` activates
    /// mana abilities with priority, one at a time; after the first, `PASS`
    /// ends it. A permanent is picked by its own id (`MY_PERM`), and which
    /// of its abilities next when it has several (`ChooseMode`, `NUMBER` +
    /// k); a card in hand by its ability's id (`HAND_ABILITY`). The mana goes
    /// to the pool with its restrictions; the player keeps priority (the
    /// `Priority` task under this one).
    pub(crate) fn activate_mana(&mut self, p: PlayerId, card: Option<CardIdx>, again: bool) {
        let sorcery_ok = self.turn.active == p && self.turn.step.is_main() && self.stack.is_empty();
        let classes = self.mana_classes(p, None);
        let sources = self.mana_float_sources(p, &classes, sorcery_ok);
        if sources.is_empty() {
            // (Only after an activation: nothing more to activate.)
            assert!(again, "ActivateMana with no mana ability to activate");
            self.answer = None;
            return;
        }
        let slots_of = |c: CardIdx| sources.iter().find(|(x, _)| *x == c).map(|(_, v)| v.clone()).unwrap_or_default();
        let run = |g: &mut Game, c: CardIdx, slot: AbilitySlot| {
            // (The next activation after this one's choices.)
            g.agenda.push(Task::ActivateMana { p, card: None, again: true });
            if slot == crate::pay::SAC_FOR {
                // A granted "sacrifice this: add <mana>" (Rain of Filth).
                let mask = g.sac_mana_of(c);
                g.activate_sac_for(Src::of(c, slot), None);
                let prods: Vec<[u8; 6]> = (0..6).filter(|&t| mask & (1 << t) != 0).map(|t| std::array::from_fn(|i| (i == t) as u8)).collect();
                g.agenda.push(Task::ManaNowPick { p, prods, only: 0, page: crate::decision::Page::FIRST });
            } else {
                g.begin_activate(p, c, slot);
            }
        };
        match self.answer.take() {
            Some(Choice::Pass) => {}
            Some(Choice::Card(c)) => {
                let slots = slots_of(c);
                if slots.len() == 1 {
                    run(self, c, slots[0]);
                } else {
                    let opts = (0..slots.len()).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) }).collect();
                    self.ask(Task::ActivateMana { p, card: Some(c), again }, p, DecisionKind::ChooseMode, opts);
                }
            }
            Some(Choice::Number(k)) => {
                let c = card.expect("the permanent picked");
                let slot = slots_of(c)[k as usize];
                run(self, c, slot);
            }
            Some(Choice::Ability(c, slot)) => run(self, c, slot),
            Some(other) => unreachable!("ActivateMana answer {other:?}"),
            // (A refreshed decision, ENGINE.md 10: the permanent's question
            // again.)
            None if card.is_some() => {
                let slots = slots_of(card.expect("the permanent"));
                let opts = (0..slots.len()).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) }).collect();
                self.ask(Task::ActivateMana { p, card, again }, p, DecisionKind::ChooseMode, opts);
            }
            None => {
                let mut opts: Vec<Opt> = sources
                    .iter()
                    .map(|(c, slots)| match slots[0] {
                        AbilitySlot::Hand(_) => Opt { action: self.ability_action(p, *c, slots[0]), choice: Choice::Ability(*c, slots[0]) },
                        _ => Opt { action: self.perm_action(p, *c), choice: Choice::Card(*c) },
                    })
                    .collect();
                if again {
                    opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                }
                self.ask(Task::ActivateMana { p, card: None, again }, p, DecisionKind::ActivateMana, opts);
            }
        }
    }
}
