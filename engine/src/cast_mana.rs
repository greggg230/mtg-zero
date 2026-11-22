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
        // (In the payment search only the feeding abilities are offered; an answer, or the question of a permanent's
        // ability, reads them all: the option it searches from may be any.)
        let sources = self.cast_mana_sources(id, p, spell, self.oracle && self.answer.is_none() && card.is_none());
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
    fn cast_mana_sources(&self, id: u32, p: PlayerId, spell: CardIdx, feed_only: bool) -> Vec<(CardIdx, Vec<AbilitySlot>)> {
        let mut out: Vec<(CardIdx, Vec<AbilitySlot>)> = Vec::new();
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

    /// Castability through the 601.2g step (ENGINE.md 222.4): can `p` cast spell `card` the way `from` says, for its
    /// mana cost (`alt` 0) or alternative cost `alt` - 1, with the help of mana abilities activated while it is cast
    /// that put cards into `p`'s graveyard for its delve or its cost's "exile n cards from your graveyard" and that the
    /// payment's own castability does not count: one with a mana cost of its own for a spell whose mana can't be spent
    /// (Chromatic Star's {1} paid by another mana ability, its mana floating: Hogaak), any for such a spell's exile
    /// (escape), one with a mana cost or a discard for another spell's exile, one that discards or whose last counter
    /// sacrifices it for delve. Asked after the castability of the payment alone says no, and only when such an
    /// ability exists (`feed_gap`).
    ///
    /// Exact by construction: the spell is put on the stack of a copy of the game the way casting it would (each
    /// announcement castability considers: each additional cost option, escalate's discards, a spree mode, emerge's
    /// permanent; X its least) and the cast's own 601.2g step is searched (`payment_completes`, as `Game::completing`
    /// searches it while casting): every subset and order of the feeding abilities (`cast_mana_feeder`) with every way
    /// to pay their costs (their payments' own decisions: which permanents pay a Star's {1}, a creature tapped for it
    /// then not convoking), down to the payment's castability after the step (`cast_mana_pass_ok`: the total cost locked
    /// as the first activation determined it, CR 601.2f). The same search the step makes when the spell is cast, so
    /// castability and the cast agree.
    pub(crate) fn feed_castable(&self, p: PlayerId, card: CardIdx, from: crate::game::CastFrom, alt: u8) -> bool {
        if from == crate::eoe_c::FREE_CAST || !self.feed_gap(p, card, from, alt) {
            return false;
        }
        let def = self.def_of(card);
        // The additional cost options castability considers (`options_payable`: a kicker option needs its mode's
        // targets); one cost for an alternative cost (`alt_payable`).
        let options: Vec<u8> = if alt > 0 || def.additional.is_empty() {
            vec![0]
        } else {
            (0..def.additional.len() as u8)
                .filter(|&k| self.route_extra(card, from, k).is_some())
                .filter(|&k| !def.kicker || def.spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| self.spell_targets_available(&m.list[k as usize], p, card)))
                .collect()
        };
        // The modes chosen as castability considers them: escalate's discards (`escalate_any`: e more modes with
        // targets), one spree mode with its targets (`spree_extras`); none otherwise.
        let mut modes: Vec<Vec<u8>> = vec![Vec::new()];
        if let Some(m) = def.spell.as_ref().and_then(|ab| ab.modes.as_ref()) {
            let with: Vec<u8> = (0..m.list.len() as u8).filter(|&i| self.spell_targets_available(&m.list[i as usize], p, card)).collect();
            if m.escalate_discard {
                let n = with.len().min(m.max as usize).max(1);
                modes = (1..=n).map(|k| with[..k.min(with.len())].to_vec()).collect();
            } else if !m.costs.is_empty() {
                modes = with.iter().map(|&i| vec![i]).collect();
            }
        }
        // Emerge's permanent, chosen as its cost is determined (CR 702.119a, 601.2f): each candidate.
        let mut noted: Vec<Vec<crate::ids::ObjRef>> = vec![Vec::new()];
        if alt > 0 {
            if let Some(f) = self.alt_at(card, alt as usize - 1).and_then(|(a, _)| a.emerge.clone()) {
                let s = crate::db::SacCost { filter: f, n: 1, another: false };
                noted = self.sacrifice_candidates(p, card, &s).into_iter().map(|x| vec![self.obj_ref(x)]).collect();
            }
        }
        for &option in &options {
            for m in &modes {
                for n in &noted {
                    if self.feed_search(p, card, from, alt, option, m, n) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Is there a mana ability the 601.2g step may activate that puts a card into `p`'s graveyard which spell `card`'s
    /// payment (cast the way `from` and `alt` say) reads and its castability does not count (`feed_castable`)? Its
    /// cost reads the graveyard (delve, an exile from it); `p` has such an ability (whether it may be activated now is
    /// the search's).
    fn feed_gap(&self, p: PlayerId, card: CardIdx, from: crate::game::CastFrom, alt: u8) -> bool {
        let def = self.def_of(card);
        let exile = if alt > 0 {
            self.alt_rest(card, from, alt as usize - 1).is_some_and(|r| r.exile_graveyard > 0)
        } else {
            (0..def.additional.len().max(1) as u8).any(|k| self.route_extra(card, from, k).is_some_and(|r| r.exile_graveyard > 0))
        };
        if !def.delve && !exile {
            return false;
        }
        let no_mana = def.casting.no_mana;
        // What castability counts (`delve_classes`, `no_mana_feeders`, `cost_payable_with`'s `delve_sources`): of a
        // spell that spends mana, delve: an ability that sacrifices (its source or another permanent), exile: one that
        // sacrifices its own source with no mana cost; of a spell of no mana, delve: one that sacrifices with no mana
        // cost, exile: none.
        let counted = |ad: &ActivatedDef, own: bool| {
            let k = &ad.cost;
            let sacs = k.sac_self || k.sacrifice.is_some();
            let free = k.mana.is_zero() && !ad.x;
            if exile && (no_mana || !(own && k.sac_self && free)) {
                return false;
            }
            !def.delve || if no_mana { sacs && free } else { sacs }
        };
        let gap = |ad: &ActivatedDef, slot: AbilitySlot, own: bool| ad.mana().is_some() && !ad.instant_only && Self::cast_mana_feeder(ad, slot) && !counted(ad, own);
        let rain = self.seen & crate::game::SEEN_SAC_MANA != 0 && self.effects.iter().any(|e| matches!(e.m, crate::db::Mod::GainSacMana(_)));
        for c in self.permanents_of(p) {
            let d = self.abilities(c);
            if d.activated.iter().enumerate().any(|(i, ad)| gap(ad, AbilitySlot::Activated(i as u8), true)) {
                return true;
            }
            if self.granted_activated(c).into_iter().any(|g| gap(self.db.def(g.def).activated_def(AbilitySlot::Activated(g.slot)), AbilitySlot::Activated(g.slot), false)) {
                return true;
            }
            // (A granted "sacrifice this: add <mana>", Rain of Filth's: counted for delve as a sacrifice slot.)
            if rain && (exile || no_mana) && self.sac_mana_of(c) != 0 {
                return true;
            }
        }
        self.players[p as usize].hand.iter().any(|&c| self.def_of(c).hand_abilities.iter().enumerate().any(|(i, ad)| gap(ad, AbilitySlot::Hand(i as u8), true)))
    }

    /// One announcement of `feed_castable`: on a copy of the game, spell `card` on the stack (put there, or already
    /// there while its alternative cost is chosen) with alternative cost `alt`, additional cost option `option`,
    /// modes `modes`, emerge's permanent `noted` and its least X; its 601.2g step searched.
    #[allow(clippy::too_many_arguments)]
    fn feed_search(&self, p: PlayerId, card: CardIdx, from: crate::game::CastFrom, alt: u8, option: u8, modes: &[u8], noted: &[crate::ids::ObjRef]) -> bool {
        let mut g = self.clone();
        g.pending = None;
        g.answer = None;
        g.page = None;
        let def = self.def_of(card);
        let x = def.x_min as u32;
        let granter = (alt as usize > def.alt_costs.len()).then(|| self.granted_alts(card)[alt as usize - 1 - def.alt_costs.len()].0);
        let id = match g.stack.iter().position(|s| s.kind == StackKind::Spell { card }) {
            Some(pos) => {
                let it = &mut g.stack[pos];
                (it.alt, it.cost_option, it.x) = (alt, option, x);
                it.modes = modes.to_vec();
                it.noted = noted.to_vec();
                it.id
            }
            None => {
                let origin = g.objs[card.i()].zone;
                g.move_card_to_stack(card, p);
                let id = g.new_stack_id();
                g.stack.push(crate::game::StackItem {
                    id,
                    kind: StackKind::Spell { card },
                    controller: p,
                    targets: Vec::new(),
                    cost_option: option,
                    modes: modes.to_vec(),
                    ev: Default::default(),
                    alt,
                    from,
                    delved: 0,
                    delve_floor: 0,
                    discarded_land: false,
                    x,
                    division: Vec::new(),
                    attack_as: None,
                    noted: noted.to_vec(),
                    flips: 0,
                    origin,
                    times: 0,
                    spliced: Vec::new(),
                });
                id
            }
        };
        if let Some(gr) = granter {
            g.note_granted_alt(card, gr);
        }
        let floor = g.agenda.len();
        g.agenda.push(Task::CastMana { id, card: None, again: false });
        g.payment_completes(floor, p)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::{AbilitySlot, CardDb};
    use crate::decision::{Choice, DecisionKind};
    use crate::flow::Task;
    use crate::game::{Game, Step};
    use crate::ids::{CardIdx, Target, Zone};
    use crate::testkit::Scenario;

    /// Every state at the next priority from `g`, every decision answered every way but `skip`'s options; with
    /// `then`, each such state after `then` (a state it returns `None` for is left out).
    fn ends(g: &Game, skip: &dyn Fn(&Game, Choice) -> bool, out: &mut Vec<Game>) {
        if g.kind() == Some(DecisionKind::Priority) || g.is_over() {
            out.push(g.clone());
            return;
        }
        let d = g.pending_decision().expect("a decision");
        for o in d.options.iter().filter(|o| !skip(g, o.choice)) {
            let mut h = g.clone();
            h.choose(o.choice);
            ends(&h, skip, out);
        }
    }

    /// Both players' loop hashes of each state (ENGINE.md 222.3: the states compared as the game tells them apart),
    /// sorted, each once.
    fn hashes(states: &[Game]) -> Vec<(u64, u64)> {
        let mut v: Vec<(u64, u64)> = states.iter().map(|g| (g.loop_hash(0, None), g.loop_hash(1, None))).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// ENGINE.md 222.3 (the collapse): a Mountain's "{T}: add {R}" activated in Lightning Bolt's 601.2g step reaches
    /// exactly the states the cast without it, then the Mountain activated with the priority right after (when it is
    /// still untapped; tapped by the payment it is the same), reach. Chromatic Star makes the step ask (never chosen).
    #[test]
    fn a_collapsed_land_activated_while_casting_reaches_the_states_of_activating_it_after() {
        let mut g = Scenario::new(CardDb::cube())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 8])
            .library(1, &["Island"; 8])
            .battlefield(0, &["Chromatic Star", "Mountain", "Mountain"])
            .hand(0, &["Lightning Bolt"])
            .at(Step::Main1, 0)
            .start();
        let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
        let star = g.card(0, "Chromatic Star", Zone::Battlefield);
        let lands: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Mountain").collect();
        g.choose(Choice::Cast(bolt));
        g.choose(Choice::Target(Target::Player(1)));
        assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{:?}", g.options_desc());
        assert!(!g.offered().contains(&Choice::Card(lands[0])), "the Mountain is collapsed: {:?}", g.options_desc());
        let id = g.stack().last().expect("the Bolt").id;
        let no_star = |_: &Game, c: Choice| c == Choice::Card(star);
        for &m in &lands {
            // Path 1: the Mountain in the step, then everything else.
            let mut a = g.clone();
            a.pending = None;
            a.cast_mana_run(id, 0, m, AbilitySlot::Activated(0));
            a.advance();
            let mut one = Vec::new();
            ends(&a, &no_star, &mut one);
            // Path 2: the step passed, the cast completed every way, then the Mountain at that priority if untapped.
            let mut b = g.clone();
            b.choose(Choice::Pass);
            let mut cast = Vec::new();
            ends(&b, &no_star, &mut cast);
            let mut two = Vec::new();
            for e in cast {
                if e.objs[m.i()].tapped {
                    two.push(e);
                    continue;
                }
                let mut f = e.clone();
                f.pending = None;
                f.agenda.push(Task::Priority);
                f.begin_activate(0, m, AbilitySlot::Activated(0));
                f.advance();
                assert_eq!(f.kind(), Some(DecisionKind::Priority), "{}", f.log_lines().join("\n"));
                two.push(f);
            }
            assert!(!one.is_empty());
            assert_eq!(hashes(&one), hashes(&two), "the Mountain {m:?}: the states differ");
        }
    }

    /// CR 601.2f-g (ENGINE.md 222): the total cost is locked as the step's first mana ability is activated, so a cost
    /// reducer the ability sacrifices (Emry's "costs {1} less for each artifact you control": Chromatic Star) doesn't
    /// raise it; the lock is gone once the spell is cast.
    #[test]
    fn the_total_cost_is_locked_at_the_steps_first_activation() {
        let mut g = Scenario::new(CardDb::cube())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 8])
            .library(1, &["Island"; 8])
            .battlefield(0, &["Chromatic Star", "Lotus Petal", "Island"])
            .hand(0, &["Emry, Lurker of the Loch"])
            .at(Step::Main1, 0)
            .start();
        let emry = g.card(0, "Emry, Lurker of the Loch", Zone::Hand);
        let star = g.card(0, "Chromatic Star", Zone::Battlefield);
        g.choose(Choice::Cast(emry));
        assert_eq!(g.kind(), Some(DecisionKind::CastMana), "{:?}", g.options_desc());
        let id = g.stack().last().expect("Emry").id;
        let before = g.total_mana(emry, g.stack().last().unwrap());
        assert_eq!(before.total(), 1, "{{2}}{{U}} less two artifacts");
        g.choose(Choice::Card(star));
        // (The Star's {1}: the Island; its mana: blue.)
        for _ in 0..20 {
            if g.kind() == Some(DecisionKind::CastMana) {
                break;
            }
            let c = match g.kind() {
                Some(DecisionKind::ChooseMana) => g.offered().into_iter().find(|c| matches!(c, Choice::Number(1))).unwrap_or(g.offered()[0]),
                _ => g.offered().into_iter().find(|c| *c == Choice::Card(g.card(0, "Island", Zone::Battlefield))).unwrap_or(g.offered()[0]),
            };
            g.choose(c);
        }
        assert_eq!(g.objs[star.i()].zone, Zone::Graveyard, "{}", g.log_lines().join("\n"));
        assert_eq!(g.locked_cost(id), Some(before), "locked as the Star was activated");
        assert_eq!(g.total_mana(emry, g.stack().last().unwrap()), before, "one artifact left, the cost unchanged");
        let mut h = g.clone();
        h.cast_lock.clear();
        assert_eq!(h.total_mana(emry, h.stack().last().unwrap()).total(), 2, "unlocked it would be {{1}}{{U}}");
        for _ in 0..20 {
            if g.kind() == Some(DecisionKind::Priority) {
                break;
            }
            let c = g.offered().into_iter().find(|c| *c == Choice::Pass).unwrap_or(g.offered()[0]);
            g.choose(c);
        }
        assert_eq!(g.objs[emry.i()].zone, Zone::Stack, "{}", g.log_lines().join("\n"));
        assert!(g.cast_lock.is_empty(), "cleared at 601.2i");
    }
}
