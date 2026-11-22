//! Mana abilities while a spell is cast (CR 601.2g; ENGINE.md 222).
//!
//! "If the total cost includes a mana payment, the player then has a chance to activate mana abilities. Mana
//! abilities must be activated before costs are paid" (601.2g), and then "the player pays the total cost" in any
//! order (601.2h): delve's exiles, convoke's taps, the cost's other parts and the mana. The payment (`PayMana`)
//! activates the sources whose mana it spends; this step, `Task::CastMana`, asked once the total cost is determined
//! (601.2b-f: after the targets, modes, X and the alternative or additional cost are announced) and before delve's
//! cards are chosen, lets the caster activate any mana ability it may activate now (not an "activate only as an
//! instant" one: Lion's Eye Diamond's, which needs priority), its mana kept in the pool for the payment or
//! floating, its card or the permanent it sacrificed in the graveyard for delve and the cost's exile, its
//! triggered abilities waiting until the spell is cast (601.2i, 117.2, 603.3: on the stack with the spell's own
//! cast triggers, above the spell).
//!
//! * **The cost is locked** (601.2f) at the first activation (`Game::cast_lock`): a sacrificed artifact or creature no
//!   longer changes it.
//! * **Every option leaves the payment completable** (601.2h: a cost is paid whole; `cost_order::completing`): the
//!   step's options and every decision of an ability it activates are searched (`Game::payment_completes`) down to
//!   the step, where the search asks the payment's castability (`Game::can_pay_spell`) after any further abilities
//!   that may put cards into the caster's graveyard (the only ones that can make a payment completable that wasn't:
//!   delve's and the exile's cards; any other ability's mana the payment's own sources make, ENGINE.md 222.4).
//! * **One collapse** (ENGINE.md 222.3, the proof): a permanent's own "{T}: add <fixed mana>" ability, nothing else in
//!   its cost or effect, is not offered when activating it now and activating it with the priority the caster
//!   receives right after the cast reach the same states (`Game::cast_mana_collapsed`): nothing between reads or
//!   changes what it does.

use crate::db::{AbilityDef, AbilitySlot, ActivatedDef, EventKind, ManaProd, TrigPred};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, StackKind};
use crate::ids::{CardIdx, PlayerId, Zone};
use crate::mana::ManaCost;
use crate::pay::Src;

/// What the collapse of a pure mana ability reads once per step (`Game::cast_mana_collapsed`).
struct Collapse {
    /// The checks that do not depend on the ability's permanent: the caster receives priority right after the cast,
    /// nothing reacts to a permanent becoming tapped, and no triggered ability that may be put on the stack before
    /// that priority reads whether a permanent is tapped.
    global: bool,
    /// What the rest of the cast may change (`Game::cast_rest_reach`), read once a permanent is asked about.
    reach: Option<crate::cost_order::RestReach>,
}

impl Game {
    /// The total cost locked for stack item `id` (CR 601.2f) while its mana abilities were activated (601.2g).
    pub(crate) fn locked_cost(&self, id: u32) -> Option<ManaCost> {
        if self.cast_lock.is_empty() {
            return None;
        }
        self.cast_lock.iter().find(|x| x.0 == id).map(|x| x.1)
    }

    /// The payment search's floor for a decision of the 601.2g step (`cost_order::completing_floor`): the step's own
    /// options (it is pushed at the agenda's length) and, while an ability it activates is paid and resolved, every
    /// decision above it (the step's task waiting under them). `None` for any other decision.
    pub(crate) fn cast_mana_floor(&self, task: &Task) -> Option<(usize, PlayerId)> {
        if let Task::CastMana { id, .. } = task {
            return self.item_index(*id).map(|i| (self.agenda.len(), self.stack[i].controller));
        }
        if self.cast_lock.is_empty() {
            return None;
        }
        let at = self.agenda.iter().rposition(|t| matches!(t, Task::CastMana { .. }))?;
        let Task::CastMana { id, .. } = self.agenda[at] else { unreachable!("the step's task") };
        self.item_index(id).map(|i| (at, self.stack[i].controller))
    }

    /// Is spell `card` (cast or about to be) still before its 601.2g step's end: not on the stack (castability, CR
    /// 601.2: the step comes), or on it with the step's task waiting (the announcements, the step's decisions)?
    /// Castability's delve then counts the cards abilities activated in the step may put into the graveyard
    /// (`Game::delve_classes`); after it, delve's own sources only (ENGINE.md 222.4).
    pub(crate) fn before_cast_mana(&self, card: CardIdx) -> bool {
        if self.objs[card.i()].zone != Zone::Stack {
            return true;
        }
        self.agenda.iter().any(|t| match t {
            Task::CastMana { id, .. } => self.item_index(*id).is_some_and(|i| self.stack[i].kind == StackKind::Spell { card }),
            _ => false,
        })
    }

    /// `Task::CastMana` (CR 601.2g): the caster of spell `id` activates mana abilities, one at a time, its total cost
    /// determined; `PASS` pays it. A permanent is picked by its own id (`MY_PERM`) and which of its abilities next when
    /// it has several (`ChooseMode`, `NUMBER` + k); a card in hand by its ability's id (`HAND_ABILITY`). Not asked when
    /// the total cost has no mana (601.2g: "if the total cost includes a mana payment") or nothing may be activated
    /// but `PASS`. In the payment search (`Game::oracle`) it asks only the abilities that may feed the graveyard a
    /// payment reads, and `PASS` asks the payment's castability.
    pub(crate) fn cast_mana(&mut self, id: u32, card: Option<CardIdx>, again: bool) {
        let Some(pos) = self.item_index(id) else { return };
        let StackKind::Spell { card: spell } = self.stack[pos].kind else { return };
        let p = self.stack[pos].controller;
        if !again && self.answer.is_none() && self.total_mana(spell, &self.stack[pos]).is_zero() {
            return;
        }
        let sources = self.cast_mana_sources(id, p, spell);
        let slots_of = |c: CardIdx| sources.iter().find(|(x, _)| *x == c).map(|(_, v)| v.clone()).unwrap_or_default();
        match self.answer.take() {
            Some(Choice::Pass) => {
                if self.oracle && !self.cast_mana_pass_ok(id) {
                    self.oracle_dead = true;
                }
            }
            Some(Choice::Card(c)) => {
                let slots = slots_of(c);
                if slots.len() == 1 {
                    self.cast_mana_run(id, p, c, slots[0]);
                } else {
                    let opts = (0..slots.len()).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) }).collect();
                    self.ask(Task::CastMana { id, card: Some(c), again }, p, DecisionKind::ChooseMode, opts);
                }
            }
            Some(Choice::Number(k)) => {
                let c = card.expect("the permanent picked");
                let slot = slots_of(c)[k as usize];
                self.cast_mana_run(id, p, c, slot);
            }
            Some(Choice::Ability(c, slot)) => self.cast_mana_run(id, p, c, slot),
            Some(other) => unreachable!("CastMana answer {other:?}"),
            // (A refreshed decision, ENGINE.md 10: the permanent's question again.)
            None if card.is_some() => {
                let slots = slots_of(card.expect("the permanent"));
                let opts = (0..slots.len()).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) }).collect();
                self.ask(Task::CastMana { id, card, again }, p, DecisionKind::ChooseMode, opts);
            }
            None => {
                if sources.is_empty() {
                    // (Nothing to activate: the cost is paid. In the search, whether it can be.)
                    if self.oracle && !self.cast_mana_pass_ok(id) {
                        self.oracle_dead = true;
                    }
                    return;
                }
                let mut opts: Vec<Opt> = sources
                    .iter()
                    .map(|(c, slots)| match slots[0] {
                        AbilitySlot::Hand(_) => Opt { action: self.ability_action(p, *c, slots[0]), choice: Choice::Ability(*c, slots[0]) },
                        _ => Opt { action: self.perm_action(p, *c), choice: Choice::Card(*c) },
                    })
                    .collect();
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                let task = Task::CastMana { id, card: None, again };
                // (Only `PASS` leaves the payment completable: no decision.)
                if !self.oracle && self.completing(&task, opts.clone()).iter().all(|o| o.choice == Choice::Pass) {
                    return;
                }
                self.ask(task, p, DecisionKind::CastMana, opts);
            }
        }
    }

    /// Activate source `slot` of `c` in spell `id`'s 601.2g step: its total cost locked first (601.2f), the step asked
    /// again after the ability's choices and costs (no state-based actions, no triggered ability put on the stack: no
    /// player receives priority while a spell is cast, CR 601.2, 117.5).
    fn cast_mana_run(&mut self, id: u32, p: PlayerId, c: CardIdx, slot: AbilitySlot) {
        if self.locked_cost(id).is_none() {
            let pos = self.item_index(id).expect("the spell being cast");
            let StackKind::Spell { card } = self.stack[pos].kind else { unreachable!("a spell") };
            let cost = self.total_mana(card, &self.stack[pos]);
            self.cast_lock.push((id, cost));
        }
        self.agenda.push(Task::CastMana { id, card: None, again: true });
        if slot == crate::pay::SAC_FOR {
            // A granted "sacrifice this: add <mana>" (Rain of Filth).
            let mask = self.sac_mana_of(c);
            self.activate_sac_for(Src::of(c, slot), None);
            let prods: Vec<[u8; 6]> = (0..6).filter(|&t| mask & (1 << t) != 0).map(|t| std::array::from_fn(|i| (i == t) as u8)).collect();
            self.agenda.push(Task::ManaNowPick { p, prods, only: 0, page: crate::decision::Page::FIRST });
        } else {
            self.begin_activate(p, c, slot);
        }
    }

    /// The mana abilities `p` may activate in spell `id`'s 601.2g step (CR 605.3a: "whenever they are asked to pay a
    /// cost that includes a mana payment"), as `ActivateMana`'s: for each permanent with some, in canonical order, its
    /// slots (its own abilities, those other permanents grant it, a granted "sacrifice this: add <mana>"); then each
    /// card in hand with one, one per definition (the card a hand slot takes, ENGINE.md 147.6). Not an "activate only
    /// as an instant" ability (Lion's Eye Diamond: no player has priority, ruling 2004-10-04), not one the collapse
    /// leaves to the priority after the cast (`cast_mana_collapsed`). In the payment search only those that may put a
    /// card into `p`'s graveyard, when the payment reads the graveyard (`cast_mana_feeder`).
    fn cast_mana_sources(&self, id: u32, p: PlayerId, spell: CardIdx) -> Vec<(CardIdx, Vec<AbilitySlot>)> {
        let mut out: Vec<(CardIdx, Vec<AbilitySlot>)> = Vec::new();
        let feed_only = self.oracle;
        if feed_only {
            let Some(pos) = self.item_index(id) else { return out };
            if !self.payment_reads_graveyard(spell, &self.stack[pos]) {
                return out;
            }
        }
        let classes = self.mana_classes(p, None);
        let locks = self.activation_locks();
        let rain = self.seen & crate::game::SEEN_SAC_MANA != 0 && self.effects.iter().any(|e| matches!(e.m, crate::db::Mod::GainSacMana(_)));
        let mut collapse: Option<Collapse> = None;
        for c in self.permanents_of(p) {
            let mut slots = Vec::new();
            let d = self.abilities(c);
            for (i, ad) in d.activated.iter().enumerate() {
                if ad.mana().is_none() || ad.instant_only {
                    continue;
                }
                if (d.latent > 0 && !self.has_activated(c, i)) || (ad.exhaust && self.exhausted(c, i as u8)) {
                    continue;
                }
                if feed_only && !Self::cast_mana_feeder(ad, AbilitySlot::Activated(i as u8)) {
                    continue;
                }
                if !self.mana_ability_now(p, c, ad, &classes, false, locks) {
                    continue;
                }
                if !feed_only && self.cast_mana_collapsed(&mut collapse, id, p, spell, c, ad) {
                    continue;
                }
                slots.push(AbilitySlot::Activated(i as u8));
            }
            let own = self.own_ability_slots(c);
            for (j, g) in self.granted_activated(c).into_iter().enumerate() {
                let ad = self.db.def(g.def).activated_def(AbilitySlot::Activated(g.slot));
                if ad.mana().is_none() || ad.instant_only || (feed_only && !Self::cast_mana_feeder(ad, AbilitySlot::Activated(g.slot))) {
                    continue;
                }
                if self.mana_ability_now(p, c, ad, &classes, false, locks) {
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
            for c in self.hand_slot_order_in(p) {
                let def = self.objs[c.i()].def;
                if defs.contains(&def) || !self.hand_mana_source(c) {
                    continue;
                }
                let slots: Vec<AbilitySlot> = self
                    .def_of(c)
                    .hand_abilities
                    .iter()
                    .enumerate()
                    .filter(|(i, ad)| {
                        ad.mana().is_some()
                            && !ad.instant_only
                            && (!feed_only || Self::cast_mana_feeder(ad, AbilitySlot::Hand(*i as u8)))
                            && self.mana_ability_now(p, c, ad, &classes, false, locks)
                    })
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

    /// May activating mana ability `ad` (at `slot`) put a card into its controller's graveyard (a sacrifice: of its
    /// source, of another permanent, Gemstone Mine's at its last counter, Rain of Filth's granted one; a discard)?
    /// Only such abilities can make a payment that reads the graveyard (delve, an exile from the graveyard)
    /// completable that wasn't (ENGINE.md 222.4).
    fn cast_mana_feeder(ad: &ActivatedDef, slot: AbilitySlot) -> bool {
        let k = &ad.cost;
        slot == crate::pay::SAC_FOR
            || k.sac_self
            || k.sacrifice.is_some()
            || k.discard > 0
            || k.discard_also.is_some()
            || k.discard_hand
            || k.discard_self
            || ad.mana().is_some_and(|m| m.sac_if_no.is_some())
    }

    /// Does spell `card`'s payment read its caster's graveyard: delve's cards (CR 702.66a), the cost's "exile n cards
    /// from your graveyard" (escape, 702.138a)?
    pub(crate) fn payment_reads_graveyard(&self, card: CardIdx, item: &crate::game::StackItem) -> bool {
        self.def_of(card).delve || self.spell_rest(card, item).is_some_and(|r| r.exile_graveyard > 0)
    }

    /// The delve fuel of a spell whose mana can't be spent (CR 113.6e) from the mana abilities its 601.2g step may
    /// activate (castability, `Game::delve_classes`; ENGINE.md 222.4): each `p`'s mana source that sacrifices its card
    /// (itself; a land for another's ability, Squandered Resources', Rain of Filth's) with no mana cost to pay (no
    /// mana is paid for such a spell's cost; "{X}" as 0), a class of the delve unit alone (`Class::delve`: no mana, the
    /// spell's cost takes none), its card's use whole (it can't then be tapped for convoke: sacrificed first, 601.2g
    /// before 601.2h). Only cards that reach `p`'s graveyard (`feeds_graveyard`).
    pub(crate) fn no_mana_feeders(&self, p: PlayerId) -> Vec<crate::pay::Class> {
        let mut out = Vec::new();
        for cl in self.mana_classes(p, None) {
            let sacs = cl.sac_self || cl.members.first().is_some_and(|s| crate::pay::is_sac_slot(s.slot));
            if !sacs || cl.need != 0 {
                continue;
            }
            let mut members = crate::pay::Members::new();
            for s in cl.members.iter() {
                if self.feeds_graveyard(p, s.card) {
                    members.push(*s);
                }
            }
            if members.is_empty() {
                continue;
            }
            out.push(crate::pay::Class {
                members,
                yld: crate::pay::Yield::Multi { prods: vec![[0; 6]], units: Some(Vec::new()) },
                delve: true,
                convert: false,
                only: 0,
                dmg: 0,
                res: crate::pay::TAP | crate::pay::SAC,
                ..cl
            });
        }
        out
    }

    /// The search's question at the end of the 601.2g step: can the payment of spell `id` from here (delve's cards,
    /// the cost's parts, the mana: CR 601.2h) be completed? The payment's castability (`can_pay_spell`, ENGINE.md
    /// 100.4.11) with the total cost locked; and emerge's permanent, chosen as that cost was determined (CR 702.119a,
    /// 601.2f), still on the battlefield to be sacrificed.
    fn cast_mana_pass_ok(&self, id: u32) -> bool {
        let Some(pos) = self.item_index(id) else { return true };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return true };
        if item.alt > 0 && self.item_alt(card, item).is_some_and(|(a, _)| a.emerge.is_some()) && !item.noted.first().is_some_and(|r| self.live(*r)) {
            return false;
        }
        let cost = self.total_mana(card, item);
        self.can_pay_spell(card, item, &cost)
    }

    /// The collapse (ENGINE.md 222.3): is permanent `c`'s own mana ability `ad` left out of spell `id`'s 601.2g step,
    /// activating it now and activating it with the priority `p` receives right after the cast reaching the same
    /// states? When:
    /// 1. its cost is {T} alone and it only adds mana of a fixed production (one of its types, fixed amounts: what it
    ///    makes reads nothing of the game), with no damage, no life, no sacrifice, no condition;
    /// 2. its tap adds nothing ("whenever you tap ... for mana") and no ability reacts to a permanent becoming tapped;
    /// 3. `p` receives priority right after the cast (CR 601.2i: it was cast with priority);
    /// 4. no triggered ability that may be put on the stack before that priority (the waiting ones, those of every
    ///    object that could trigger meanwhile) reads, as it is put on the stack, whether a permanent is tapped (its
    ///    trigger condition, its intervening "if", its targets and modes);
    /// 5. nothing the rest of the cast may do removes `c`, changes a permanent's types or abilities (a static ability's
    ///    source leaving, its condition), removes counters or changes a toughness (`cast_rest_reach`).
    fn cast_mana_collapsed(&self, ctx: &mut Option<Collapse>, id: u32, p: PlayerId, spell: CardIdx, c: CardIdx, ad: &ActivatedDef) -> bool {
        let Some(m) = ad.mana() else { return false };
        let k = &ad.cost;
        let tap_only = k.tap
            && k.mana.is_zero()
            && !ad.x
            && k.life == 0
            && !(k.sac_self || k.exile_self || k.discard_self || k.discard_hand || k.exert || k.waterbend)
            && k.sacrifice.is_none()
            && k.bounce.is_none()
            && k.discard == 0
            && k.discard_also.is_none()
            && k.exile_from_hand.is_none()
            && k.exile_graveyard == 0
            && k.remove_counters.is_none()
            && k.remove_counter_from.is_none()
            && k.tap_untapped.is_none()
            && k.tap_x.is_none()
            && k.crew == 0;
        let plain = ad.special.is_none() && ad.only_if.is_none() && !ad.your_turn && ad.loyalty.is_none() && !ad.sorcery && !ad.instant_only;
        let fixed = matches!(
            m.prod,
            ManaProd::One(_) | ManaProd::Fixed(_) | ManaProd::SameType { .. } | ManaProd::Combo { .. } | ManaProd::OneOf(_) | ManaProd::Chosen | ManaProd::ChosenN(_)
        );
        if !(tap_only && plain && fixed && m.damage_to_you == 0 && m.sac_if_no.is_none() && m.opp_life == 0) {
            return false;
        }
        if self.tap_bonus_wide(c, true) != [0; 6] {
            return false;
        }
        let ctx = ctx.get_or_insert_with(|| Collapse { global: self.collapse_global(id, p, spell), reach: None });
        if !ctx.global {
            return false;
        }
        let pos = self.item_index(id).expect("the spell being cast");
        let reach = ctx.reach.get_or_insert_with(|| self.cast_rest_reach(p, spell, &self.stack[pos]));
        !reach.unsettled && !reach.chars && !reach.leave.contains(&c)
    }

    /// The collapse's checks 2-4 that do not depend on the permanent (`cast_mana_collapsed`).
    fn collapse_global(&self, id: u32, p: PlayerId, spell: CardIdx) -> bool {
        // 3: the caster receives priority after the cast (the priority task under the cast's last).
        let Some(done) = self.agenda.iter().rposition(|t| matches!(t, Task::CastDone { id: x } if *x == id)) else { return false };
        if done == 0 || !matches!(self.agenda[done - 1], Task::Priority) || self.turn.priority != p {
            return false;
        }
        // (EoE W3's granted triggered abilities: not looked at, so not collapsed.)
        if self.w3.granted {
            return false;
        }
        // The objects whose triggered abilities may trigger before that priority: the permanents and emblems (also
        // the ones that leave meanwhile, which look back), the cards in graveyards (abilities that function there, "put
        // into a graveyard from anywhere"), the caster's hand (a card a cost discards), the spell itself; and the
        // waiting triggers and temporary ones.
        let bit = 1u64 << EventKind::BecomesTapped as u32;
        let mut defs: Vec<crate::ids::DefId> = Vec::new();
        for &c in self.battlefield.iter().chain(self.command.iter()) {
            defs.push(self.objs[c.i()].def);
        }
        for pl in 0..2 {
            for &c in &self.players[pl].graveyard {
                defs.push(self.objs[c.i()].def);
            }
        }
        for &c in &self.players[p as usize].hand {
            defs.push(self.objs[c.i()].def);
        }
        defs.push(self.objs[spell.i()].def);
        for t in &self.waiting {
            defs.push(t.source_def);
        }
        for t in &self.temp_triggers {
            defs.push(t.source_def);
        }
        defs.sort_unstable();
        defs.dedup();
        defs.iter().all(|&d| {
            let def = self.db.def(d);
            // 2: nothing reacts to a permanent becoming tapped.
            def.trig_mask & bit == 0 && def.triggers.iter().all(|t| t.event != EventKind::BecomesTapped && !self.pred_reads_tapped(&t.pred) && !Self::ability_reads_tapped(&t.ability))
        })
    }

    /// Does a trigger condition read, as its event happens, whether a permanent is tapped (exhaustive: a new condition
    /// says; collapse check 4)?
    fn pred_reads_tapped(&self, pr: &TrigPred) -> bool {
        match pr {
            TrigPred::And(a, b) | TrigPred::Or(a, b) => self.pred_reads_tapped(a) || self.pred_reads_tapped(b),
            TrigPred::Not(a) => self.pred_reads_tapped(a),
            TrigPred::Subject(_, f) | TrigPred::Source(f) => crate::cost_order::filter_reads_tapped(f),
            TrigPred::If(k) | TrigPred::State(k) => self.cond_reads_tapped(k),
            TrigPred::Always
            | TrigPred::IsSelf(_)
            | TrigPred::Controlled(..)
            | TrigPred::FormerHost(_)
            | TrigPred::HostAttacksWithAnother
            | TrigPred::IsHost(_)
            | TrigPred::Chapter(_)
            | TrigPred::Level(_)
            | TrigPred::TimesPerSubject(..)
            | TrigPred::Batch
            | TrigPred::EnteredCast
            | TrigPred::EnteredPaid
            | TrigPred::SpellPaid
            | TrigPred::OrCopy
            | TrigPred::Granted(_)
            | TrigPred::FirstAttack
            | TrigPred::EnteredXAtLeast(_)
            | TrigPred::Once
            | TrigPred::EnteredNotFromGyExile
            | TrigPred::InGraveyard
            | TrigPred::SpellMvParity
            | TrigPred::AttacksMonarch
            | TrigPred::LeftTo(_)
            | TrigPred::CountersPutBy(_) => false,
        }
    }

    /// Do the choices made as a triggered ability is put on the stack (its targets, its modes' targets: CR 603.3d,
    /// 700.2a) read whether a permanent is tapped?
    fn ability_reads_tapped(ab: &AbilityDef) -> bool {
        ab.targets.iter().any(|t| crate::cost_order::filter_reads_tapped(&t.filter)) || ab.modes.as_ref().is_some_and(|m| m.list.iter().any(Self::ability_reads_tapped))
    }
}
