//! Activated abilities (CR 602, 605; ENGINE.md 18): what can be activated
//! at priority, the activation process (602.2: announce, targets, costs),
//! free-standing mana abilities, and "as this enters" choices (614.12).

use crate::db::{types, AbilityRef, AbilitySlot, ActEffect, ActivatedDef, Cond, Cost, Kw};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos, StackItem, StackKind};
use crate::ids::{Target, CardIdx, PlayerId, Zone};
use crate::pay::{productions, Class, Keep, PayMana, Src};

/// Activations of one permanent's non-mana ability in a turn the engine offers
/// (a loop of optional actions, CR 732.2a; as `copies::MAY_COPY_CAP`).
pub(crate) const ACTIVATION_CAP: u8 = 31;

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
            // (Only a static ability's own condition; read in layers.rs.)
            Cond::SourceCountersIn(..) => false,
            Cond::GainedLifeThisTurn => pl.gained_life,
            Cond::OpponentHasMoreCards => self.players[crate::ids::opp(p) as usize].hand.len() > pl.hand.len(),
            Cond::SpellsCastThisTurn { n, noncreature } => {
                (if *noncreature { pl.noncreature_cast } else { pl.spells_cast }) == *n
            }
            Cond::LibraryEmpty => pl.library.is_empty(),
            Cond::YourTurn => self.turn.active == p,
            Cond::LifeAtMostHalfStart => pl.life * 2 <= self.spec.starting_life,
            Cond::ControlOtherOfColor(m) => {
                self.permanents_of(p).any(|x| self.colors_of(x) & m != 0 && Some(self.obj_ref(x)) != source)
            }
            Cond::All(v) => v.iter().all(|c| self.cond_holds_from(c, p, source)),
            Cond::SpellsCastAtLeast(n) => pl.spells_cast >= *n,
            Cond::HasInitiative => self.initiative == Some(p),
            Cond::CompletedDungeon => self.dungeons[p as usize].completed > 0,
            Cond::LinkedTypesAtLeast(n) => source.is_some_and(|s| self.linked_types(s).count_ones() >= *n as u32),
            Cond::CityBlessing => pl.city_blessing || self.ascends(p),
            Cond::LandsPlayedAtLeast(n) => pl.lands_played >= *n,
            Cond::SourceAttached => source.is_some_and(|s| self.live(s) && self.objs[s.idx.i()].attached.is_some()),
        }
    }

    /// Ascend (CR 702.131a): `p` controls a permanent with ascend and ten or
    /// more permanents.
    pub(crate) fn ascends(&self, p: PlayerId) -> bool {
        self.permanents_of(p).any(|c| self.abilities(c).ascend) && self.permanents_of(p).count() >= 10
    }

    /// A permanent's activated ability `i` was activated `ACTIVATION_CAP`
    /// times this turn.
    pub(crate) fn activation_capped(&self, c: CardIdx, i: u8) -> bool {
        let r = self.obj_ref(c);
        self.activations.iter().any(|x| x.0 == r && x.1 == i && x.2 >= ACTIVATION_CAP)
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
        let o = &self.objs[card.i()];
        o.pw_turn != self.turn.number as u16 || o.pw_used < 1 + o.pw_extra
    }

    /// A loyalty ability of `card` is being activated (CR 606.3).
    fn note_loyalty_activation(&mut self, card: CardIdx) {
        let turn = self.turn.number as u16;
        let o = &mut self.objs[card.i()];
        if o.pw_turn != turn {
            o.pw_turn = turn;
            o.pw_used = 0;
            o.pw_extra = 0;
        }
        o.pw_used = o.pw_used.saturating_add(1);
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
    ) {
        // (The battlefield's activation locks, read once.)
        let locks = self.activation_locks();
        for c in self.permanents_of(p) {
            let d = self.abilities(c);
            for (i, ad) in d.activated.iter().enumerate() {
                if ad.mana().is_some() && !self.mana_with_priority(c, ad) {
                    continue; // paid with while paying a cost (ENGINE.md 18)
                }
                if d.latent > 0 && !self.has_activated(c, i) {
                    continue; // a latent ability it hasn't gained
                }
                if ad.exhaust && self.exhausted(c, i as u8) {
                    continue; // CR 702.177a: only once
                }
                if !self.activations.is_empty() && self.activation_capped(c, i as u8) {
                    continue; // a loop of optional actions (ENGINE.md 29)
                }
                let slot = AbilitySlot::Activated(i as u8);
                if self.can_activate(p, c, ad, classes, sorcery_ok, locks) {
                    out.push(Opt { action: self.ability_action(p, c, slot), choice: Choice::Ability(c, slot) });
                }
            }
            // A special action isn't an ability (CR 116.2b, 708.7): turning a
            // face-down permanent face up survives "loses all abilities".
            if self.lost_abilities(c) {
                for (i, ad) in self.db.def(self.objs[c.i()].def).activated.iter().enumerate() {
                    let slot = AbilitySlot::Activated(i as u8);
                    if ad.special.is_some() && self.can_activate(p, c, ad, classes, sorcery_ok, locks) {
                        out.push(Opt { action: self.ability_action(p, c, slot), choice: Choice::Ability(c, slot) });
                    }
                }
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
        // 27.5): `GY_ABILITY` + k for the k-th card there that has one (one
        // each, the lowering checks).
        let mut k = 0u16;
        for c in self.graveyard_cards(p) {
            let d = self.def_of(c);
            let Some(ad) = d.gy_abilities.first() else { continue };
            if k >= act::GY_ABILITY_N {
                break;
            }
            let action = act::GY_ABILITY + k;
            k += 1;
            if self.can_activate(p, c, ad, classes, sorcery_ok, locks) {
                out.push(Opt { action, choice: Choice::Ability(c, AbilitySlot::Graveyard(0)) });
            }
        }
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
    pub(crate) fn act_mana_x(&self, p: PlayerId, card: CardIdx, ad: &ActivatedDef, x: u8) -> crate::mana::ManaCost {
        let mut m = ad.cost.mana;
        m.generic = m.generic.saturating_add(x);
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
            m.generic = m.generic.saturating_sub(self.discount(d, p));
        }
        let ob = &self.objs[card.i()];
        if ob.zone == Zone::Battlefield && ob.controller == p && self.is_type(card, types::ARTIFACT) {
            for c in self.permanents_of(p) {
                let n = self.abilities(c).artifact_ability_discount;
                if n > 0 {
                    let floor = m.total().saturating_sub(1);
                    m.generic = m.generic.saturating_sub(n.min(floor));
                }
            }
        }
        // "Abilities you activate that aren't mana abilities cost {2} less"
        // (Zirda; at least one mana stays).
        if self.seen & crate::game::SEEN_ABILITY_DISCOUNT != 0 && ad.mana().is_none() && ad.special.is_none() && m.generic > 0 && m.total() > 1 {
            for c in self.permanents_of(p) {
                let n = self.abilities(c).ability_discount;
                if n > 0 {
                    let floor = m.total().saturating_sub(1);
                    m.generic = m.generic.saturating_sub(n.min(floor));
                }
            }
        }
        m
    }

    /// The source is excluded from paying the ability's mana when the cost
    /// also taps or moves it (ENGINE.md 18).
    pub(crate) fn mana_exclude(card: CardIdx, ad: &ActivatedDef) -> Option<CardIdx> {
        let c = &ad.cost;
        (c.tap || c.sac_self || c.exile_self || c.discard_self).then_some(card)
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
        if mana_ability && !self.mana_with_priority(card, ad) {
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
                if d < 0 && self.counters_on(card, crate::db::Counter::Loyalty) < (-(d as i32)) as u16 {
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
            if !self.cost_payable(p, card, &ad.cost, &self.act_mana(p, card, ad), classes, Self::mana_exclude(card, ad)) {
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
        if !self.cost_payable(p, card, &ad.cost, &self.act_mana(p, card, ad), classes, Self::mana_exclude(card, ad)) {
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
        let def = self.objs[card.i()].def;
        self.note(|g| format!("P{p} activates {} ({slot:?})", g.name(card)));
        if self.objs[card.i()].zone == Zone::Hand {
            self.reveal(card); // 602.2a: activated from a hidden zone
        }
        let ad = self.db.def(def).activated_def(slot);
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
        if let (AbilitySlot::Activated(i), crate::db::ActEffect::Stack(_), Zone::Battlefield) = (slot, &ad.effect, self.objs[card.i()].zone) {
            let r = self.obj_ref(card);
            match self.activations.iter_mut().find(|x| x.0 == r && x.1 == i) {
                Some(x) => x.2 = x.2.saturating_add(1),
                None => self.activations.push((r, i, 1)),
            }
        }
        if ad.mana().is_some() {
            self.agenda.push(Task::ManaNow { src: Src { card, slot } });
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
        let ability = AbilityRef { def, slot, mode: None };
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
            noted: Vec::new(),
            flips: self.objs[card.i()].flips,
            origin: Zone::Nowhere,
            times: 0,
            spliced: Vec::new(),
        });
        self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
        self.agenda.push(Task::ActPay { id });
        let ab = self.db.ability(ability);
        if ab.has_targets() {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
        }
        if ad.x {
            self.agenda.push(Task::ChooseX { id }); // CR 602.2b, 601.2b
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
                let def = self.db.def(self.objs[card.i()].def);
                let c = if item.alt > 0 {
                    &def.alt_costs[item.alt as usize - 1].cost
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
        let pl = &self.players[p as usize];
        if (cost.life as i32) > pl.life {
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
        if let Some((k, n)) = cost.remove_counters {
            if self.counters_on(card, k) < n as u16 {
                return false;
            }
        }
        if cost.crew > 0 {
            if self.is_spell_card(card) {
                // Teamwork: the taps (paid first) and the mana compete.
                let cands = self.crew_eligible(p, card);
                let keep = Self::keep_for(card, cost, exclude);
                if !self.crew_mana_ok(p, cost.crew as i32, &[], &cands, mana, classes, &keep) {
                    return false;
                }
            } else if self.crew_power(p, card, !cost.saddle) < cost.crew as i32 {
                return false;
            }
        }
        // (Not counting permanents that could be tapped for mana first: the
        // mana is paid before, 601.2g-h.)
        if let Some(t) = &cost.tap_untapped {
            if self.tap_candidates(p, card, t).filter(|&c| !self.has_mana_ability(c)).count() < t.n as usize {
                return false;
            }
        }
        // (Mana sources that sacrifice themselves are cards in the graveyard
        // by then: activated first, CR 601.2g-h.)
        let short = self.exile_short(p, card, cost);
        if short > 0 && short > self.delve_sources(p).into_iter().filter(|&c| c != card && Some(c) != exclude).count() {
            return false;
        }
        if mana.is_zero() {
            return true;
        }
        self.can_pay_without(p, mana, classes, &Self::keep_for(card, cost, exclude), exclude)
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
    pub(crate) fn crew_power(&self, p: PlayerId, vehicle: CardIdx, bonus: bool) -> i32 {
        self.crew_eligible(p, vehicle).into_iter().map(|c| self.crew_power_of(c, bonus).max(0)).sum()
    }

    /// A creature's power for crewing (a Pilot crews as though its power
    /// were greater: crew only, not saddle or teamwork, CR 702.122,
    /// 702.171a).
    pub(crate) fn crew_power_of(&self, c: CardIdx, bonus: bool) -> i32 {
        self.power(c) + if bonus { self.abilities(c).crew_bonus } else { 0 }
    }

    /// Teamwork (a spell's "tap any number of untapped creatures you control
    /// with total power n or more", paid before its mana, CR 601.2h): can
    /// `p` tap creatures of `cands`, with the ones already chosen
    /// (`tapped_in`), for total power `need` and still pay `mana` from
    /// `classes` without the mana abilities of the creatures tapped? (The
    /// taps and the mana compete for creatures with mana abilities: every
    /// set of those is tried.)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn crew_mana_ok(
        &self,
        p: PlayerId,
        need: i32,
        tapped_in: &[CardIdx],
        cands: &[CardIdx],
        mana: &crate::mana::ManaCost,
        classes: &[crate::pay::Class],
        keep: &Keep,
    ) -> bool {
        let have: i32 = tapped_in.iter().map(|&c| self.crew_power_of(c, false)).sum();
        let (with_mana, plain): (Vec<CardIdx>, Vec<CardIdx>) = cands.iter().copied().partition(|&c| self.has_mana_ability(c));
        let plain_power: i32 = plain.iter().map(|&c| self.crew_power_of(c, false).max(0)).sum();
        let k = with_mana.len().min(10);
        for mask in 0u32..(1 << k) {
            let extra: i32 = (0..k).filter(|i| mask >> i & 1 != 0).map(|i| self.crew_power_of(with_mana[i], false).max(0)).sum();
            if have + plain_power + extra < need {
                continue;
            }
            if mana.is_zero() {
                return true;
            }
            let tapped: Vec<CardIdx> =
                tapped_in.iter().copied().chain((0..k).filter(|i| mask >> i & 1 != 0).map(|i| with_mana[i])).collect();
            let left = crate::pay::classes_without(classes, &tapped);
            if self.can_pay_with(p, mana, &left, keep) {
                return true;
            }
        }
        false
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

    /// What the parts of `cost` paid after its mana need from that payment.
    pub(crate) fn keep_for(card: CardIdx, cost: &Cost, exclude: Option<CardIdx>) -> Keep {
        debug_assert!(
            [cost.sacrifice.is_some() || cost.bounce.is_some(), cost.discard > 0, cost.exile_from_hand.is_some()]
                .iter()
                .filter(|&&x| x)
                .count()
                <= 1,
            "one reservation per cost (lowering)"
        );
        debug_assert!(!(cost.sacrifice.is_some() && cost.bounce.is_some()), "one reservation per cost (lowering)");
        Keep {
            exclude,
            life: cost.life,
            // (A permanent to return to its owner's hand, ninjutsu's
            // unblocked attacker, is reserved like one to sacrifice: a mana
            // ability that sacrifices it can't pay the mana first.)
            sac: cost.sacrifice.clone().or_else(|| cost.bounce.clone()).map(|s| (s, card)),
            // (A land-card discard never competes with a mana source in
            // hand: those are not lands.)
            discard: (cost.discard > 0 && cost.discard_filter.is_none()).then_some((cost.discard, card)),
            exile: cost.exile_from_hand.clone().map(|f| (f, card)),
            allow: 0,
            spell: None,
        }
    }

    /// 601.2g-h for an activation: the mana part first.
    pub(crate) fn act_pay(&mut self, id: u32) {
        let Some((p, card, ad)) = self.act_item(id) else { return };
        let x = self.item_index(id).map_or(0, |pos| self.stack[pos].x);
        let mana = self.act_mana_x(p, card, ad, x);
        if mana.is_zero() && self.exile_short(p, card, &ad.cost) == 0 {
            return;
        }
        let mut pm = PayMana::new(p, mana);
        pm.keep = Self::keep_for(card, &ad.cost, Self::mana_exclude(card, ad));
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
        (cost.exile_graveyard as usize).saturating_sub(self.graveyard_cards(p).filter(|&c| c != card).count())
    }

    /// The mana sources a payment activates first so that they are in the
    /// graveyard for `cost`'s "exile n cards from your graveyard" (CR
    /// 601.2g-h; the first in canonical order: Lotus Petals alike).
    pub(crate) fn exile_sources(&self, p: PlayerId, card: CardIdx, cost: &Cost, exclude: Option<CardIdx>) -> Vec<(crate::pay::Src, bool)> {
        let short = self.exile_short(p, card, cost);
        if short == 0 {
            return Vec::new();
        }
        let classes = self.mana_classes(p, exclude);
        self.delve_sources(p)
            .into_iter()
            .filter(|&c| c != card && Some(c) != exclude)
            .take(short)
            .filter_map(|c| classes.iter().find_map(|cl| cl.members.iter().find(|s| s.card == c && cl.sac_self).copied()))
            .map(|s| (s, false))
            .collect()
    }

    /// The rest of a cost after its mana (601.2h): sacrifice and discard
    /// choices, one decision per object; then {T}, life, and the source's
    /// own costs.
    pub(crate) fn pay_costs(&mut self, id: u32, mut picked: Vec<CardIdx>) {
        let Some((p, card, cost)) = self.item_cost(id) else { return };
        let n_sac = cost.sacrifice.as_ref().map_or(0, |s| s.n) as usize;
        let n_disc = n_sac + cost.discard as usize;
        let n_exile = n_disc + cost.exile_from_hand.is_some() as usize;
        let n_bounce = n_exile + cost.bounce.as_ref().map_or(0, |b| b.n) as usize;
        let n_gy = n_bounce + cost.exile_graveyard as usize;
        let n_tap = n_gy + cost.tap_untapped.as_ref().map_or(0, |t| t.n) as usize;
        let mut crew_done = false;
        // The cost is one event (CR 603.2c; its discards, exiles): a group of
        // its own.
        if picked.is_empty() && self.answer.is_none() {
            self.new_event();
        }
        if let Some(ch) = self.answer.take() {
            let c = match ch {
                Choice::Card(c) => c,
                // CR 702.122a: enough power tapped for crew.
                Choice::Pass => {
                    crew_done = true;
                    CardIdx(u16::MAX)
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
        }
        if picked.len() < n_sac {
            let s = cost.sacrifice.as_ref().unwrap();
            let opts: Vec<Opt> = self
                .sacrifice_candidates(p, card, s)
                .into_iter()
                .filter(|c| !picked.contains(c))
                .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                .collect();
            self.ask(Task::PayCosts { id, picked }, p, DecisionKind::Sacrifice, opts);
            return;
        }
        if picked.len() < n_disc {
            let mut opts = self.hand_options_except(p, Some(card));
            if let Some(f) = &cost.discard_filter {
                let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
                opts.retain(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(f, c, &ctx)));
            }
            self.ask(Task::PayCosts { id, picked }, p, DecisionKind::DiscardCost, opts);
            return;
        }
        if picked.len() < n_exile {
            let f = cost.exile_from_hand.as_ref().unwrap();
            let opts = self.exile_candidates(p, card, f);
            self.ask(Task::PayCosts { id, picked }, p, DecisionKind::ExileCost, opts);
            return;
        }
        if picked.len() < n_bounce {
            let b = cost.bounce.as_ref().unwrap();
            let opts: Vec<Opt> = self
                .sacrifice_candidates(p, card, b)
                .into_iter()
                .filter(|c| !picked[n_exile..].contains(c))
                .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                .collect();
            self.ask(Task::PayCosts { id, picked }, p, DecisionKind::BounceCost, opts);
            return;
        }
        if picked.len() < n_gy {
            // Graveyard cards to exile, a set: picked in increasing
            // position, leaving enough later ones.
            // (Cards only: not the source, not a token, CR 108.2.)
            let gy: Vec<CardIdx> = self.graveyard_cards(p).filter(|&c| c != card).collect();
            let after = picked[n_bounce..].last().map(|c| gy.iter().position(|x| x == c).unwrap() + 1).unwrap_or(0);
            let still = n_gy - picked.len();
            let opts: Vec<Opt> = gy
                .iter()
                .enumerate()
                .filter(|&(i, _)| i >= after && gy.len() - i >= still)
                .map(|(_, &c)| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) })
                .collect();
            self.ask(Task::PayCosts { id, picked }, p, DecisionKind::ChooseGraveyardCard, opts);
            return;
        }
        if picked.len() < n_tap {
            // "Tap n untapped <permanents> you control": one decision each
            // (a set: increasing slot order).
            let t = cost.tap_untapped.as_ref().unwrap();
            let floor = picked[n_gy..].last().map(|&c| self.perm_action(p, c));
            let cands: Vec<Opt> = self
                .tap_candidates(p, card, t)
                .filter(|c| !picked.contains(c))
                .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                .filter(|o| floor.is_none_or(|f| o.action > f))
                .collect();
            // (Leaving enough later candidates for the taps still owed.)
            let still = n_tap - picked.len();
            let opts: Vec<Opt> = cands.iter().enumerate().filter(|(k, _)| cands.len() - k >= still).map(|(_, o)| *o).collect();
            self.ask(Task::PayCosts { id, picked }, p, DecisionKind::ChoosePermanent, opts);
            return;
        }
        if cost.crew > 0 && !crew_done {
            // Crew (CR 702.122a): untapped creatures other than the source,
            // one decision each in increasing slot order; PASS once their
            // total power is enough. A pick must leave the total reachable.
            let need = cost.crew as i32;
            let bonus = !cost.saddle && !self.is_spell_card(card);
            let tapped_in: Vec<CardIdx> = picked[n_tap..].to_vec();
            let have: i32 = tapped_in.iter().map(|&c| self.crew_power_of(c, bonus)).sum();
            let floor = tapped_in.last().map(|&c| self.perm_action(p, c));
            let cands: Vec<CardIdx> = self
                .crew_eligible(p, card)
                .into_iter()
                .filter(|c| !tapped_in.contains(c) && floor.is_none_or(|f| self.perm_action(p, *c) > f))
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
                        let rest: i32 = cands[k + 1..].iter().map(|&x| self.crew_power_of(x, bonus).max(0)).sum();
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
                self.ask(Task::PayCosts { id, picked }, p, DecisionKind::ChoosePermanent, opts);
                return;
            }
        }
        // (The taps of a "tap untapped permanents" cost, then crew's.)
        for &c in &picked[n_gy..] {
            self.tap(c);
        }
        for &c in &picked[n_bounce..n_gy] {
            self.move_card(c, Zone::Exile, Pos::Top);
        }
        if let Some((k, n)) = cost.remove_counters {
            self.remove_counters(card, k, n as u16);
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
        let mut sacs: Vec<CardIdx> = picked.iter().copied().take(n_sac).collect();
        sacs.sort_by_key(|c| self.objs[c.i()].timestamp);
        // "The sacrificed permanent's mana value" (as it last existed, CR
        // 608.2h): an activated ability's item keeps it (`Amount::CostMvPlus`).
        if let (Some(&c), Some(pos)) = (sacs.first(), self.item_index(id)) {
            if matches!(self.stack[pos].kind, StackKind::Ability { .. }) {
                self.stack[pos].ev.amount = self.mana_value_of(c) as u16;
            }
        }
        self.begin_batch();
        for c in sacs {
            self.note(|g| format!("P{p} sacrifices {}", g.name(c)));
            self.sacrifice(c);
        }
        self.end_batch();
        if cost.life > 0 {
            self.lose_life(p, cost.life as i32);
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
                Some(d) if d > 0 => self.add_counters(src, crate::db::Counter::Loyalty, d as u16),
                Some(d) if d < 0 => {
                    self.remove_counters(src, crate::db::Counter::Loyalty, (-(d as i32)) as u16);
                }
                _ => {}
            }
        }
    }

    /// Is mana ability `ad` of `c` offered with priority (ENGINE.md 18)? One
    /// that may be activated only as an instant (Lion's Eye Diamond: never
    /// while paying), or one that sacrifices its source when the source has
    /// a triggered ability of its own as it goes (Chromatic Star, Wizard's
    /// Rockets: "when this is put into a graveyard, draw a card"; activated
    /// for nothing but that, CR 605.3a). Every other mana ability is
    /// activated while paying a cost only: floating its mana first can't
    /// matter (the pool empties between steps, CR 106.4, 500.4).
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

    /// Has `c` activated its exhaust ability `i` (CR 702.177a)?
    pub(crate) fn exhausted(&self, c: CardIdx, i: u8) -> bool {
        let r = self.obj_ref(c);
        self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::Exhausted(i))
    }

    /// A mana ability activated with priority ("activate only as an
    /// instant", Lion's Eye Diamond; one whose cost has a mana part or a
    /// choice, ENGINE.md 28): pay its costs (the mana first, CR 601.2g-h by
    /// 602.2b), then choose the mana (CR 605.3b, 106.1b).
    pub(crate) fn mana_now(&mut self, src: Src) {
        let p = self.objs[src.card.i()].controller;
        let ad = self.def_of(src.card).activated_def(src.slot);
        // "{X}, {T}, ...: Add X mana" (CR 107.3, 602.2b: X is announced as
        // the ability is activated): as much as can be paid, not with the
        // source's own mana. Choice: `ChooseMode`, `NUMBER` + X.
        let x = if ad.x {
            match self.answer.take() {
                Some(Choice::Number(x)) => {
                    self.note(|_| format!("X = {x}"));
                    x
                }
                Some(other) => unreachable!("X answer {other:?}"),
                None => {
                    let exclude = Self::mana_exclude(src.card, ad);
                    let classes = self.mana_classes(p, exclude);
                    let keep = Self::keep_for(src.card, &ad.cost, exclude);
                    let mut x = 0u8;
                    while x < (act::NUMBER_N - 1) as u8
                        && self.can_pay_without(p, &self.act_mana_x(p, src.card, ad, x + 1), &classes, &keep, exclude)
                    {
                        x += 1;
                    }
                    let opts = (0..=x).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k) }).collect();
                    self.ask(Task::ManaNow { src }, p, DecisionKind::ChooseMode, opts);
                    return;
                }
            }
        } else {
            0
        };
        let mana = self.act_mana_x(p, src.card, ad, x);
        self.agenda.push(Task::ManaNowCosts { src, sac: None, x });
        if !mana.is_zero() {
            let mut pm = PayMana::new(p, mana);
            pm.keep = Self::keep_for(src.card, &ad.cost, Self::mana_exclude(src.card, ad));
            self.agenda.push(Task::PayMana(Box::new(pm)));
        }
    }

    /// The rest of a priority mana ability's cost: the permanent to
    /// sacrifice (a decision), then {T}, life, the source's own costs.
    pub(crate) fn mana_now_costs(&mut self, src: Src, mut sac: Option<CardIdx>, x: u8) {
        let p = self.objs[src.card.i()].controller;
        let ad = self.def_of(src.card).activated_def(src.slot);
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
                        self.ask(Task::ManaNowCosts { src, sac, x }, p, DecisionKind::Sacrifice, opts);
                        return;
                    }
                }
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
            _ => productions(self, p, prod, Some(src.card)),
        };
        // "Whenever you tap a creature for mana, add an additional {G}" (a
        // triggered mana ability, CR 605.1b).
        let mut prods = prods;
        if ad.cost.tap && self.is_creature(src.card) {
            let more = self.creature_mana_bonus(p);
            if more != [0; 6] {
                for v in prods.iter_mut() {
                    for t in 0..6 {
                        v[t] += more[t];
                    }
                }
            }
        }
        if let Some(c) = sac {
            self.sacrifice(c);
        }
        self.activate_mana_costs(src);
        // Mana that gives haste to the creature spell it is spent on (Arena
        // of Glory: one fixed production, the lowering checks).
        if ad.mana().is_some_and(|m| m.haste) && prods.len() == 1 {
            let v = prods[0];
            self.add_mana(p, &v);
            let pool = &mut self.players[p as usize].pool;
            pool.haste = pool.haste.saturating_add(v[crate::mana::R]).min(pool.amt[crate::mana::R]);
            return;
        }
        if !prods.is_empty() {
            // X mana in any combination: one mana, one choice, X times.
            let picks = if matches!(prod, crate::db::ManaProd::ComboX { .. }) { x } else { 1 };
            for _ in 0..picks {
                self.agenda.push(Task::ManaNowPick { p, prods: prods.clone() });
            }
        }
    }

    /// The types of mana permanent `c`'s mana abilities could make now
    /// (CR 106.7).
    pub(crate) fn could_produce(&self, c: CardIdx) -> crate::mana::ManaMask {
        let p = self.objs[c.i()].controller;
        self.abilities(c)
            .activated
            .iter()
            .filter_map(|a| a.mana())
            .flat_map(|m| productions(self, p, &m.prod, Some(c)))
            .fold(0, |m, v| (0..6).filter(|&t| v[t] > 0).fold(m, |m, t| m | (1 << t)))
    }

    pub(crate) fn mana_now_pick(&mut self, p: PlayerId, prods: Vec<[u8; 6]>) {
        match self.answer.take() {
            Some(Choice::Number(k)) => self.add_mana(p, &prods[k as usize]),
            Some(other) => unreachable!("mana answer {other:?}"),
            None => {
                if prods.len() == 1 {
                    self.add_mana(p, &prods[0]);
                    return;
                }
                let opts = (0..prods.len())
                    .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) })
                    .collect();
                self.ask(Task::ManaNowPick { p, prods }, p, DecisionKind::ChooseMana, opts);
            }
        }
    }

    /// "As this enters, you may pay N life. If you don't, it enters tapped"
    /// (CR 614.12). Asked right after the land is put onto the battlefield,
    /// before anything else happens (nothing can observe the order).
    pub(crate) fn as_enters(&mut self, card: CardIdx) {
        let o = &self.objs[card.i()];
        if o.zone != Zone::Battlefield {
            return;
        }
        let p = o.controller;
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
                    self.ask(Task::AsEnters { card }, p, DecisionKind::Target, opts);
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
                    self.agenda.push(Task::AsEnters { card });
                }
                Some(other) => unreachable!("land type answer {other:?}"),
                None => {
                    let opts = (0..5u16).map(|k| Opt { action: act::NUMBER + k, choice: Choice::Number(k as u8) }).collect();
                    self.ask(Task::AsEnters { card }, p, DecisionKind::ChooseColor, opts);
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
                    self.ask(Task::AsEnters { card }, p, DecisionKind::ChooseMode, opts);
                }
            }
            return;
        }
        if self.def_of(card).chooses_name && self.chosen_name(self.obj_ref(card)).is_none() {
            self.choose_name_as_enters(card);
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
                    self.ask(Task::AsEnters { card }, p, DecisionKind::ChooseColor, opts);
                }
            }
            return;
        }
        let n = self.def_of(card).shock_life as i32;
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
                self.ask(Task::AsEnters { card }, p, DecisionKind::YesNo, opts);
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
    /// 614.1c)? Checked before it moves, so "other lands" are the lands
    /// already there.
    pub(crate) fn enters_tapped(&self, card: CardIdx, p: PlayerId) -> bool {
        // "<Types> / nonbasic lands your opponents control enter tapped."
        let d = self.def_of(card);
        let q = crate::ids::opp(p);
        let forced = self.permanents_of(q).any(|c| {
            let od = self.abilities(c);
            od.opp_enter_tapped & d.types != 0
                || (od.opp_nonbasic_enter_tapped
                    && d.is(crate::db::types::LAND)
                    && d.supertypes & crate::db::supertypes::BASIC == 0)
        });
        if forced {
            return true;
        }
        match &self.def_of(card).enters_tapped {
            crate::db::EntersTapped::No => false,
            crate::db::EntersTapped::Always => true,
            crate::db::EntersTapped::Unless(c) => !self.cond_holds(c, p),
        }
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
