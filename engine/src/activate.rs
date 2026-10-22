//! Activated abilities (CR 602, 605; ENGINE.md 18): what can be activated
//! at priority, the activation process (602.2: announce, targets, costs),
//! free-standing mana abilities, and "as this enters" choices (614.12).

use crate::db::{types, AbilityRef, AbilitySlot, ActEffect, ActivatedDef, Cond, Cost, Kw};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos, StackItem, StackKind};
use crate::ids::{CardIdx, PlayerId, Zone};
use crate::pay::{productions, Class, Keep, PayMana, Src};

impl Game {
    /// Does condition `c` hold for player `p`?
    pub(crate) fn cond_holds(&self, c: &Cond, p: PlayerId) -> bool {
        let pl = &self.players[p as usize];
        match c {
            Cond::ControlAtLeast(f, n) => self.count_controlled(p, f) >= *n as u32,
            Cond::ControlAtMost(f, n) => self.count_controlled(p, f) <= *n as u32,
            Cond::HandExactly(n) => pl.hand.len() == *n as usize,
            Cond::GraveyardAtLeast(n) => pl.graveyard.len() >= *n as usize,
            Cond::Delirium(n) => {
                // CR 205.2a card types (kindred included) among the cards.
                let t = pl.graveyard.iter().fold(0u16, |m, &c| m | self.types_of(c));
                t.count_ones() >= *n as u32
            }
            Cond::Revolt => pl.left_battlefield,
            Cond::YourTurnAtMost(n) => self.turn.active == p && pl.turns <= *n as u32,
            Cond::HandAtMost(n) => pl.hand.len() <= *n as usize,
            Cond::GainedLifeThisTurn => pl.gained_life,
            Cond::OpponentHasMoreCards => self.players[crate::ids::opp(p) as usize].hand.len() > pl.hand.len(),
            Cond::SpellsCastThisTurn { n, noncreature } => {
                (if *noncreature { pl.noncreature_cast } else { pl.spells_cast }) == *n
            }
        }
    }

    /// The activated abilities `p` may activate with priority now (CR
    /// 602.2, 117.1b): permanents' abilities, then hand abilities. Mana
    /// abilities are activated while paying costs instead (ENGINE.md 18),
    /// except those that may be activated only as an instant.
    pub(crate) fn activation_options(
        &self,
        p: PlayerId,
        classes: &[Class],
        slots: &[(u16, CardIdx)],
        sorcery_ok: bool,
        out: &mut Vec<Opt>,
    ) {
        for c in self.permanents_of(p) {
            for (i, ad) in self.def_of(c).activated.iter().enumerate() {
                if ad.mana().is_some() && !ad.instant_only {
                    continue; // paid with while paying a cost (ENGINE.md 18)
                }
                let slot = AbilitySlot::Activated(i as u8);
                if self.can_activate(p, c, ad, classes, sorcery_ok) {
                    out.push(Opt { action: self.ability_action(p, c, slot), choice: Choice::Ability(c, slot) });
                }
            }
        }
        for &(a, c) in slots {
            for (i, ad) in self.def_of(c).hand_abilities.iter().enumerate() {
                let slot = AbilitySlot::Hand(i as u8);
                if self.can_activate(p, c, ad, classes, sorcery_ok) {
                    // One hand ability per card (lowering): the card's slot.
                    out.push(Opt { action: act::HAND_ABILITY + (a - act::HAND), choice: Choice::Ability(c, slot) });
                }
            }
        }
    }

    /// The source is excluded from paying the ability's mana when the cost
    /// also taps or moves it (ENGINE.md 18).
    fn mana_exclude(card: CardIdx, ad: &ActivatedDef) -> Option<CardIdx> {
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
    ) -> bool {
        let mana_ability = ad.mana().is_some();
        if mana_ability && !ad.instant_only {
            return false; // activated while paying (ENGINE.md 18)
        }
        if ad.sorcery && !sorcery_ok {
            return false; // CR 602.5d
        }
        if !self.activation_ready(p, card, ad) {
            return false;
        }
        if mana_ability {
            // Free-standing (instant-only) mana abilities have no mana cost.
            return !productions(self, p, &ad.mana().unwrap().prod).is_empty();
        }
        if !self.cost_payable(p, card, &ad.cost, &ad.cost.mana, classes, Self::mana_exclude(card, ad)) {
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
        if ad.mana().is_some() {
            self.agenda.push(Task::ManaNow { src: Src { card, slot } });
            return;
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
        });
        self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
        self.agenda.push(Task::ActPay { id });
        let ab = self.db.ability(ability);
        if ab.has_targets() {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
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
                let c = self.db.def(self.objs[card.i()].def).additional.get(item.cost_option as usize)?;
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
        if mana.is_zero() {
            return true;
        }
        let cls: Vec<Class>;
        let classes = match exclude {
            Some(x) => {
                cls = crate::cast::classes_without(classes, x);
                &cls[..]
            }
            None => classes,
        };
        self.can_pay_with(p, mana, classes, &Self::keep_for(card, cost, exclude))
    }

    /// What the parts of `cost` paid after its mana need from that payment.
    pub(crate) fn keep_for(card: CardIdx, cost: &Cost, exclude: Option<CardIdx>) -> Keep {
        debug_assert!(cost.sacrifice.is_none() || cost.discard == 0, "one reservation per cost (lowering)");
        Keep {
            exclude,
            life: cost.life,
            sac: cost.sacrifice.clone().map(|s| (s, card)),
            discard: (cost.discard > 0).then_some((cost.discard, card)),
        }
    }

    /// 601.2g-h for an activation: the mana part first.
    pub(crate) fn act_pay(&mut self, id: u32) {
        let Some((p, card, ad)) = self.act_item(id) else { return };
        if ad.cost.mana.is_zero() {
            return;
        }
        let mut pm = PayMana::new(p, ad.cost.mana);
        pm.keep = Self::keep_for(card, &ad.cost, Self::mana_exclude(card, ad));
        self.agenda.push(Task::PayMana(pm));
    }

    /// The rest of a cost after its mana (601.2h): sacrifice and discard
    /// choices, one decision per object; then {T}, life, and the source's
    /// own costs.
    pub(crate) fn pay_costs(&mut self, id: u32, mut picked: Vec<CardIdx>) {
        let Some((p, card, cost)) = self.item_cost(id) else { return };
        let n_sac = cost.sacrifice.as_ref().map_or(0, |s| s.n) as usize;
        if let Some(ch) = self.answer.take() {
            let Choice::Card(c) = ch else { unreachable!("cost answer {ch:?}") };
            if picked.len() < n_sac {
                picked.push(c);
            } else {
                // Discards happen one at a time: a chosen hand card is never
                // left hidden while another decision is pending.
                self.move_card(c, Zone::Graveyard, Pos::Top);
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
        if picked.len() < n_sac + cost.discard as usize {
            let opts = self.hand_options_except(p, Some(card));
            self.ask(Task::PayCosts { id, picked }, p, DecisionKind::DiscardCost, opts);
            return;
        }
        // Everything chosen: pay. Sacrificed permanents leave together.
        if cost.tap {
            self.tap(card);
        }
        let mut sacs: Vec<CardIdx> = picked.iter().copied().take(n_sac).collect();
        sacs.sort_by_key(|c| self.objs[c.i()].timestamp);
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
                    self.move_card(h, Zone::Graveyard, Pos::Top);
                }
            }
        }
        if cost.sac_self {
            self.sacrifice(card);
        }
        if cost.discard_self {
            self.move_card(card, Zone::Graveyard, Pos::Top);
        }
        if cost.exile_self {
            self.move_card(card, Zone::Exile, Pos::Top);
        }
    }

    /// A mana ability activated with priority (only "activate only as an
    /// instant" ones, Lion's Eye Diamond): pay its costs, then choose the
    /// mana (CR 605.3b, 106.1b).
    pub(crate) fn mana_now(&mut self, src: Src) {
        let p = self.objs[src.card.i()].controller;
        let ad = self.def_of(src.card).activated_def(src.slot);
        // What it can make is read before the costs are paid (none of the
        // supported costs changes it).
        let prods = productions(self, p, &ad.mana().unwrap().prod);
        self.activate_mana_costs(src);
        self.agenda.push(Task::ManaNowPick { p, prods });
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
        let n = self.def_of(card).shock_life as i32;
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

    /// Would a permanent with this definition enter tapped for `p` (CR
    /// 614.1c)? Checked before it moves, so "other lands" are the lands
    /// already there.
    pub(crate) fn enters_tapped(&self, card: CardIdx, p: PlayerId) -> bool {
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
