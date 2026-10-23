//! Casting spells, choosing targets, filters (CR 601, 115; ENGINE.md 6).

use crate::db::{types, AbilityDef, AbilityRef, AbilitySlot, Filter, Kw, TargetReq};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos, StackItem, StackKind};
use crate::ids::{CardIdx, ObjRef, PlayerId, Target, Zone};
use crate::pay::PayMana;

/// Who is asking whether a filter matches: the spell or ability's
/// controller ("you") and source ("this", "another").
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ctx {
    pub controller: PlayerId,
    pub source: Option<ObjRef>,
}

impl Game {
    /// The total mana cost of casting `card` (CR 601.2f): its mana cost
    /// plus the increases of static abilities ("noncreature spells cost {1}
    /// more").
    pub(crate) fn spell_cost(&self, card: CardIdx, p: PlayerId) -> crate::mana::ManaCost {
        self.cost_with(card, p, self.def_of(card).cost)
    }

    /// A cost to cast `card` (its mana cost, flashback cost, ...) with the
    /// increases and reductions that apply (CR 601.2f).
    pub(crate) fn cost_with(&self, card: CardIdx, p: PlayerId, base: crate::mana::ManaCost) -> crate::mana::ManaCost {
        let mut cost = base;
        let ctx = Ctx { controller: p, source: None };
        for &c in &self.battlefield {
            for (f, n) in &self.abilities(c).spell_tax {
                if self.card_matches(f, card, &ctx) {
                    cost.generic = cost.generic.saturating_add(*n);
                }
            }
        }
        // Then reductions (CR 601.2f: increases, then reductions; generic
        // mana only, not below zero).
        for d in &self.def_of(card).discounts {
            let less = match d {
                crate::db::Discount::PerBasicLandType(n) => self.basic_land_types(p) * n,
                crate::db::Discount::PerControlled(f, n) => {
                    self.permanents_of(p).filter(|&c| self.card_matches(f, c, &ctx)).count() as u8 * n
                }
            };
            cost.generic = cost.generic.saturating_sub(less);
        }
        // "Spells you cast from your graveyard cost {n} less" (Green
        // Goblin): the card is in its owner's graveyard (castability) or a
        // spell cast from there.
        let from_gy = match self.objs[card.i()].zone {
            Zone::Graveyard => true,
            Zone::Stack => self.stack.iter().any(|s| s.kind == StackKind::Spell { card } && s.origin == Zone::Graveyard),
            _ => false,
        };
        if from_gy {
            for c in self.permanents_of(p) {
                cost.generic = cost.generic.saturating_sub(self.abilities(c).gy_spell_discount);
            }
        }
        cost
    }

    /// Mayhem (CR 702.187b): `card`, a nonland card `p` discarded this turn,
    /// in `p`'s graveyard, while `p` controls a permanent that gives the
    /// nonland cards in their graveyard mayhem for their mana cost.
    pub(crate) fn mayhem_ok(&self, p: PlayerId, card: CardIdx) -> bool {
        let r = self.obj_ref(card);
        !self.def_of(card).is(types::LAND)
            && self.discarded.contains(&r)
            && self.permanents_of(p).any(|c| self.abilities(c).grants_mayhem)
    }

    // ---- priority options (CR 117.1, 116.2a, 307.1, 602.2) ----------------

    pub(crate) fn priority_options(&self, p: PlayerId) -> Vec<Opt> {
        let mut opts = self.priority_options_all(p);
        // "Each player can't cast more than one spell each turn" (Archon of
        // Emeria); "your opponents can't cast spells during your turn"
        // (Jennifer Walters): only lands and abilities are left.
        if !self.may_cast_spells(p) {
            use crate::decision::{FACE_PLAY, OTHER_PLAY};
            opts.retain_mut(|o| match o.choice {
                Choice::Cast(_) | Choice::CastFace(_) => false,
                Choice::Faces(c, m) => match crate::faces::face_choice(c, m & (FACE_PLAY | OTHER_PLAY)) {
                    Some(ch) => {
                        o.choice = ch;
                        true
                    }
                    None => false,
                },
                _ => true,
            });
        }
        opts
    }

    /// May `p` cast spells now at all ("each player can't cast more than one
    /// spell each turn", "your opponents can't cast spells during your
    /// turn")?
    pub(crate) fn may_cast_spells(&self, p: PlayerId) -> bool {
        let one = self.players[p as usize].spells_cast >= 1 && self.battlefield.iter().any(|&c| self.abilities(c).one_spell_each_turn);
        let ap = self.turn.active;
        let locked = ap != p && self.permanents_of(ap).any(|c| self.abilities(c).opp_no_spells_your_turn);
        !one && !locked
    }

    fn priority_options_all(&self, p: PlayerId) -> Vec<Opt> {
        use crate::decision::{FACE_CAST, FACE_PLAY};
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        let sorcery_ok = self.turn.active == p && self.turn.step.is_main() && self.stack.is_empty();
        let drops = 1 + self.permanents_of(p).map(|c| self.abilities(c).extra_lands as u32).sum::<u32>();
        let land_ok = sorcery_ok && (self.players[p as usize].lands_played as u32) < drops;
        let classes = self.mana_classes(p, None);
        let mut plain = None;
        let slots = self.hand_slots(p);
        for &(action, c) in &slots {
            let def = self.def_of(c);
            let front = if def.halves.is_some() {
                self.split_left_options(p, c, sorcery_ok, &classes)
            } else if def.is(types::LAND) {
                if land_ok {
                    FACE_PLAY
                } else {
                    0
                }
            } else if self.hand_castable(p, c, sorcery_ok, &classes, &mut plain) {
                FACE_CAST
            } else {
                0
            };
            // An MDFC's back face or an Adventure (CR 712.11b, 712.12,
            // 715.3).
            let other = self.other_face_options(p, c, sorcery_ok, land_ok, &classes);
            if let Some(choice) = crate::faces::face_choice(c, front | other) {
                opts.push(Opt { action, choice });
            }
        }
        self.other_zone_options(p, &classes, sorcery_ok, land_ok, &mut opts);
        self.activation_options(p, &classes, &slots, sorcery_ok, &mut opts);
        opts
    }

    /// Can `p` cast card `c` from their hand now (timing, the cost with its
    /// options, targets), with the face it has?
    pub(crate) fn hand_castable(
        &self,
        p: PlayerId,
        c: CardIdx,
        sorcery_ok: bool,
        classes: &[crate::pay::Class],
        plain: &mut Option<crate::pay::Cands>,
    ) -> bool {
        let def = self.def_of(c);
        if !self.cast_timing_ok(p, c, sorcery_ok) {
            return false;
        }
        let cls = classes;
        let cost = self.delve_reduced(c, p, self.spell_cost(c, p), 0);
        let alt_ok = (0..def.alt_costs.len()).any(|k| self.alt_payable(p, c, k, cls));
        if alt_ok {
            // castable for an alternative cost (601.2b); the mana cost
            // may be payable too (asked on casting)
        } else if !def.additional.is_empty() {
            // (Excluding the card itself: on the stack while it is paid.)
            if !self.options_payable(p, c, &cost, cls, Some(c)) {
                return false;
            }
        } else if self.hand_mana_source(c) {
            // A card cast from hand is on the stack while its cost is
            // paid: its own mana ability (a Spirit Guide's) cannot help.
            if !self.can_pay_with(p, &cost, &classes_without(cls, c), &Default::default()) {
                return false;
            }
        } else {
            let plain = plain.get_or_insert_with(|| self.plain_cands(p, cls));
            if !self.can_pay_plain(p, &cost, cls, plain) {
                return false;
            }
        }
        if let Some(ab) = &def.spell {
            if !self.targets_available(ab, p, Some(self.obj_ref(c))) {
                return false;
            }
            // Not overloaded: its own target is needed.
            if def.overload && !alt_ok {
                let plain = ab.modes.as_ref().map(|m| &m.list[0]);
                if !plain.is_some_and(|x| self.targets_available(x, p, Some(self.obj_ref(c)))) {
                    return false;
                }
            }
        }
        true
    }

    /// May `p` cast `card` now as far as timing goes (CR 307.1, 702.8):
    /// instants and flash any time they have priority, the rest at sorcery
    /// speed; "you may cast sorcery spells as though they had flash" (Teferi,
    /// Time Raveler's +1); "each opponent can cast spells only any time they
    /// could cast a sorcery" (his static ability) wins over both.
    pub(crate) fn cast_timing_ok(&self, p: PlayerId, card: CardIdx, sorcery_ok: bool) -> bool {
        if sorcery_ok {
            return true;
        }
        let def = self.def_of(card);
        let instant_speed = def.is(types::INSTANT)
            || def.keywords.has(Kw::Flash)
            || (def.is(types::SORCERY) && self.players[p as usize].flash_sorceries);
        instant_speed && !self.permanents_of(crate::ids::opp(p)).any(|c| self.abilities(c).opp_sorcery_speed)
    }

    /// Does permission `pm` let `p` play its card now?
    pub(crate) fn permission_ok(&self, pm: &crate::game::Permission, p: PlayerId) -> bool {
        if pm.player != p || !self.live(pm.card) || pm.turn.is_some_and(|t| t != self.turn.number) {
            return false;
        }
        match pm.kind {
            crate::game::PermKind::Play { attacked_subtype: Some(s), .. } => {
                // "During any turn you attacked with a <subtype>": the
                // attackers as they are now, or as they last were.
                self.players[p as usize].attacked_with.iter().any(|&r| {
                    if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield {
                        self.has_subtype(r.idx, s)
                    } else {
                        self.def_of(r.idx).subtypes.contains(&s)
                    }
                })
            }
            _ => true,
        }
    }

    /// The permission that lets `p` play `card` from exile now, if any.
    pub(crate) fn exile_permission(&self, p: PlayerId, card: CardIdx) -> Option<crate::game::PermKind> {
        let r = self.obj_ref(card);
        // (A general permission first: it may allow the card's Adventure
        // too, CR 715.3d.)
        self.permissions
            .iter()
            .find(|pm| pm.card == r && self.permission_ok(pm, p) && matches!(pm.kind, crate::game::PermKind::Play { .. }))
            .or_else(|| {
                self.permissions
                    .iter()
                    .find(|pm| pm.card == r && self.permission_ok(pm, p) && pm.kind == crate::game::PermKind::Adventure)
            })
            .map(|pm| pm.kind)
    }

    /// How `p` may cast `card` from where it is (CR 601.2a): from hand; from
    /// their graveyard with flashback (printed or granted) or escape; from
    /// exile with a permission.
    pub(crate) fn cast_route(&self, p: PlayerId, card: CardIdx) -> Option<crate::game::CastFrom> {
        use crate::game::CastFrom;
        let def = self.def_of(card);
        match self.objs[card.i()].zone {
            Zone::Hand => Some(CastFrom::Hand),
            Zone::Graveyard => {
                if self.objs[card.i()].owner != p {
                    return None;
                }
                let r = self.obj_ref(card);
                let granted = self.permissions.iter().any(|pm| {
                    pm.card == r && pm.kind == crate::game::PermKind::Flashback && self.permission_ok(pm, p)
                });
                let permitted = self.permissions.iter().any(|pm| {
                    pm.card == r
                        && matches!(pm.kind, crate::game::PermKind::Play { .. } | crate::game::PermKind::CastThenExile)
                        && self.permission_ok(pm, p)
                });
                if def.flashback.is_some() || granted {
                    Some(CastFrom::Flashback)
                } else if permitted {
                    Some(CastFrom::Permission { free: false, any_mana: false })
                } else if def.escape.is_some()
                    // (Granted escape with the card's own additional costs is
                    // not offered: one combined cost isn't built; ENGINE.md
                    // 23.11.)
                    || (!def.is(types::LAND) && def.additional.is_empty() && self.escape_granter(p).is_some())
                {
                    Some(CastFrom::Escape)
                } else if def.additional.is_empty() && self.retrace_granted(p, card) {
                    // (Likewise not with the card's own additional costs.)
                    Some(CastFrom::Retrace)
                } else if self.mayhem_ok(p, card) {
                    Some(CastFrom::Mayhem)
                } else {
                    None
                }
            }
            Zone::Exile => match self.exile_permission(p, card)? {
                crate::game::PermKind::Play { free, any_mana, .. } => Some(CastFrom::Permission { free, any_mana }),
                // CR 715.3d: cast it (not as an Adventure) for its mana cost.
                crate::game::PermKind::Adventure => Some(CastFrom::Permission { free: false, any_mana: false }),
                crate::game::PermKind::Flashback | crate::game::PermKind::CastThenExile => None,
            },
            _ => None,
        }
    }

    /// The mana to pay to cast `card` the way `from` says, with increases
    /// and reductions.
    pub(crate) fn route_mana(&self, card: CardIdx, p: PlayerId, from: crate::game::CastFrom) -> crate::mana::ManaCost {
        use crate::game::CastFrom;
        let def = self.def_of(card);
        match from {
            CastFrom::Hand => self.spell_cost(card, p),
            // "The flashback cost is equal to its mana cost" when granted.
            CastFrom::Flashback => self.cost_with(card, p, def.flashback.unwrap_or(def.cost)),
            CastFrom::Escape => self.cost_with(card, p, def.escape.as_ref().map_or(def.cost, |e| e.0)),
            // "Without paying its mana cost" (an alternative cost of no
            // mana, CR 118.9; increases still apply, 601.2f).
            CastFrom::Permission { free: true, .. } => self.cost_with(card, p, Default::default()),
            CastFrom::Permission { free: false, any_mana: true } => {
                // "Spend mana as though it were mana of any color" (CR
                // 609.4b): every coloured, hybrid and Phyrexian symbol is
                // paid by any mana ({C} stays {C}).
                let mut c = self.spell_cost(card, p);
                let colored: u8 = c.pips[..5].iter().sum::<u8>() + c.hybrid.iter().sum::<u8>() + c.phyrexian.iter().sum::<u8>();
                c.generic = c.generic.saturating_add(colored);
                c.pips[..5].fill(0);
                c.hybrid = Default::default();
                c.phyrexian = Default::default();
                c
            }
            CastFrom::Permission { .. } => self.spell_cost(card, p),
            CastFrom::Retrace => self.spell_cost(card, p),
            // "The mayhem cost is equal to its mana cost" (an alternative
            // cost, CR 702.187b; increases apply, 601.2f).
            CastFrom::Mayhem => self.spell_cost(card, p),
        }
    }

    /// An emblem of `p` gives `card` (in `p`'s graveyard) retrace (Wrenn and
    /// Six: "instant and sorcery cards in your graveyard have retrace").
    pub(crate) fn retrace_granted(&self, p: PlayerId, card: CardIdx) -> bool {
        let ctx = Ctx { controller: p, source: None };
        self.command.iter().any(|&e| {
            self.objs[e.i()].controller == p
                && self.def_of(e).grants_retrace.as_ref().is_some_and(|f| self.card_matches(f, card, &ctx))
        })
    }

    /// The non-mana part of the cost for a cast `from` (escape's exiled
    /// cards; the additional cost chosen).
    pub(crate) fn route_extra(&self, card: CardIdx, from: crate::game::CastFrom, option: u8) -> Option<&'static crate::db::Cost> {
        let def = self.def_of(card);
        match from {
            crate::game::CastFrom::Retrace => Some(retrace_cost()),
            crate::game::CastFrom::Escape => match &def.escape {
                Some(e) => Some(&e.1),
                None => {
                    let p = self.objs[card.i()].owner;
                    self.escape_granter(p).and_then(|g| self.abilities(g).grants_escape.as_ref())
                }
            },
            _ => def.additional.get(option as usize),
        }
    }

    /// Can `p` cast `card` (not in hand) the way `from` says now?
    pub(crate) fn castable_via(&self, p: PlayerId, card: CardIdx, from: crate::game::CastFrom, classes: &[crate::pay::Class], sorcery_ok: bool) -> bool {
        let def = self.def_of(card);
        if !self.cast_timing_ok(p, card, sorcery_ok) {
            return false;
        }
        let mana = self.delve_reduced(card, p, self.route_mana(card, p, from), self.escape_exiles(card, from));
        let paid = match from {
            crate::game::CastFrom::Escape | crate::game::CastFrom::Retrace => {
                self.cost_payable(p, card, self.route_extra(card, from, 0).expect("an escape or retrace cost"), &mana, classes, None)
            }
            _ if !def.additional.is_empty() => self.options_payable(p, card, &mana, classes, None),
            _ => self.can_pay_with(p, &mana, classes, &Default::default()),
        };
        if !paid {
            return false;
        }
        // Not from hand, an overload spell can't be overloaded (one
        // alternative cost at most, 118.9a): its own target is needed.
        let spell = match &def.spell {
            Some(ab) if def.overload => ab.modes.as_ref().map(|m| &m.list[0]),
            other => other.as_ref(),
        };
        match spell {
            None => true,
            // Leaving the graveyard can change what is targetable (a goyf's
            // power counts graveyard card types): check with the card on
            // the stack (CR 601.2a before 601.2c).
            Some(ab) if self.objs[card.i()].zone == Zone::Graveyard => {
                let mut g = self.clone();
                let r = g.move_card_to_stack(card, p);
                g.targets_available(ab, p, Some(r))
            }
            Some(ab) => self.targets_available(ab, p, Some(self.obj_ref(card))),
        }
    }

    /// CR 601.2b: announce X, from 0 to the most the caster can pay (31 at
    /// most; 0 when cast without paying its mana cost, 107.3b). Choice:
    /// `NUMBER`.
    pub(crate) fn choose_x(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let StackKind::Spell { card } = self.stack[pos].kind else { return };
        let p = self.stack[pos].controller;
        match self.answer.take() {
            Some(Choice::Number(x)) => {
                self.stack[pos].x = x;
                self.note(|_| format!("X = {x}"));
                return;
            }
            Some(other) => unreachable!("X answer {other:?}"),
            None => {}
        }
        let item = &self.stack[pos];
        let k = self.def_of(card).x_pips;
        let free = matches!(item.from, crate::game::CastFrom::Permission { free: true, .. });
        let base = self.route_mana(card, p, item.from);
        let mut most = 0u8;
        if !free {
            while most < (act::NUMBER_N - 1) as u8 {
                let mut c = base;
                c.generic = c.generic.saturating_add((most + 1).saturating_mul(k));
                if !self.can_pay(p, &c) {
                    break;
                }
                most += 1;
            }
        }
        let opts = (0..=most).map(|x| Opt { action: act::NUMBER + x as u16, choice: Choice::Number(x) }).collect();
        self.ask(Task::ChooseX { id }, p, DecisionKind::ChooseMode, opts);
    }

    /// Mana value (CR 202.3), with X on the stack (202.3e).
    pub(crate) fn mana_value_of(&self, c: CardIdx) -> u8 {
        let mv = self.def_of(c).mana_value();
        if self.objs[c.i()].zone != Zone::Stack {
            return mv;
        }
        let x = self.stack.iter().find(|s| s.kind == StackKind::Spell { card: c }).map_or(0, |s| s.x);
        mv.saturating_add(x.saturating_mul(self.def_of(c).x_pips))
    }

    /// Can `p` pay one of the spell's additional-cost options (CR 601.2b)
    /// with `cost` plus the option's own mana; for kicker, only an option
    /// whose effect has its targets.
    fn options_payable(
        &self,
        p: PlayerId,
        card: CardIdx,
        cost: &crate::mana::ManaCost,
        classes: &[crate::pay::Class],
        exclude: Option<CardIdx>,
    ) -> bool {
        let def = self.def_of(card);
        let src = Some(self.obj_ref(card));
        def.additional.iter().enumerate().any(|(k, o)| {
            (!def.kicker
                || def
                    .spell
                    .as_ref()
                    .and_then(|ab| ab.modes.as_ref())
                    .is_some_and(|m| self.targets_available(&m.list[k], p, src)))
                && self.cost_payable(p, card, o, &cost.plus(&o.mana), classes, exclude)
        })
    }

    /// A delve spell's cost with as much generic mana as its caster's
    /// graveyard can pay (castability, CR 702.66).
    /// (`keep`: graveyard cards another part of the cost exiles, escape.)
    pub(crate) fn delve_reduced(&self, card: CardIdx, p: PlayerId, cost: crate::mana::ManaCost, keep: u8) -> crate::mana::ManaCost {
        if !self.def_of(card).delve {
            return cost;
        }
        let gy = &self.players[p as usize].graveyard;
        let n = gy.iter().filter(|&&c| c != card).count().saturating_sub(keep as usize).min(255) as u8;
        cost.less_generic(n)
    }

    /// Graveyard cards an escape cost exiles (0 for other routes).
    pub(crate) fn escape_exiles(&self, card: CardIdx, from: crate::game::CastFrom) -> u8 {
        match from {
            crate::game::CastFrom::Escape => self.route_extra(card, from, 0).map_or(0, |c| c.exile_graveyard),
            _ => 0,
        }
    }

    /// CR 702.66a: exile cards from the graveyard, each paying {1}; one
    /// decision per card in increasing slot order, `PASS` when the rest of
    /// the cost can be paid.
    pub(crate) fn delve(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let StackKind::Spell { card } = self.stack[pos].kind else { return };
        let p = self.stack[pos].controller;
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                let at = self.players[p as usize].graveyard.iter().position(|&x| x == c).unwrap_or(0);
                self.move_card(c, Zone::Exile, Pos::Top);
                let item = &mut self.stack[pos];
                item.delved += 1;
                item.delve_floor = at as u8;
                self.agenda.push(Task::Delve { id });
                return;
            }
            Some(Choice::Pass) => return,
            Some(other) => unreachable!("delve answer {other:?}"),
            None => {}
        }
        let item = &self.stack[pos];
        let mut cost = self.route_mana(card, p, item.from).less_generic(item.delved);
        if let Some(extra) = self.route_extra(card, item.from, item.cost_option) {
            cost = cost.plus(&extra.mana);
        }
        if cost.generic == 0 {
            return;
        }
        // The fewest more cards that leave a payable cost (castability
        // made sure some number does).
        let need = (0..=cost.generic).find(|&d| self.can_pay(p, &cost.less_generic(d))).unwrap_or(cost.generic) as usize;
        let floor = item.delve_floor as usize;
        let keep = self.escape_exiles(card, item.from) as usize;
        let gy = &self.players[p as usize].graveyard;
        // Picks go up the graveyard (each set once), leaving enough cards
        // above for the picks still needed.
        let mut opts: Vec<Opt> = gy
            .iter()
            .enumerate()
            .filter(|&(i, _)| i >= floor && gy.len() - i >= need && gy.len() > keep)
            .map(|(_, &c)| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) })
            .collect();
        if need == 0 || opts.is_empty() {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        self.ask(Task::Delve { id }, p, DecisionKind::ChooseGraveyardCard, opts);
    }

    /// Escalate (CR 702.120a): discard a card for each mode beyond the
    /// first, one decision per card.
    pub(crate) fn escalate(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let p = self.stack[pos].controller;
        if let Some(Choice::Card(c)) = self.answer.take() {
            self.discard(c);
            self.stack[pos].delve_floor += 1; // (discards paid so far)
        }
        let item = &self.stack[pos];
        let owed = item.modes.len().saturating_sub(1);
        if (item.delve_floor as usize) < owed && !self.players[p as usize].hand.is_empty() {
            let opts = self.hand_options(p);
            self.ask(Task::Escalate { id }, p, DecisionKind::DiscardCost, opts);
        }
    }

    /// Mox Diamond as it resolves: discard a land card (it enters) or not
    /// (it goes to the graveyard).
    pub(crate) fn enter_by_discard(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let p = self.stack[pos].controller;
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                self.discard(c);
                self.stack[pos].discarded_land = true;
            }
            Some(Choice::Pass) => {}
            Some(other) => unreachable!("discard answer {other:?}"),
            None => {
                let ctx = Ctx { controller: p, source: None };
                let land = Filter::Type(types::LAND);
                let mut opts: Vec<Opt> = self
                    .hand_options(p)
                    .into_iter()
                    .filter(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(&land, c, &ctx)))
                    .collect();
                // (Asked even without a land card: the hand is hidden.)
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::EnterByDiscard { id }, p, DecisionKind::Discard, opts);
            }
        }
    }

    /// A permanent of `p` giving the nonland cards in their graveyard
    /// escape (Underworld Breach).
    pub(crate) fn escape_granter(&self, p: PlayerId) -> Option<CardIdx> {
        self.permanents_of(p).find(|&c| self.abilities(c).grants_escape.is_some())
    }

    /// "You may play lands from your graveyard" (Crucible of Worlds).
    pub(crate) fn lands_from_graveyard(&self, p: PlayerId) -> bool {
        self.permanents_of(p).any(|c| self.abilities(c).lands_from_graveyard)
    }

    /// Priority options for cards in exile with a permission (`EXILE`
    /// slots) and in the graveyard (`GRAVEYARD` slots): lands to play,
    /// spells to cast.
    fn other_zone_options(&self, p: PlayerId, classes: &[crate::pay::Class], sorcery_ok: bool, land_ok: bool, opts: &mut Vec<Opt>) {
        use crate::decision::{FACE_CAST, FACE_PLAY};
        let mut k = 0u16;
        for &c in &self.exile {
            let cast_only = match self.exile_permission(p, c) {
                Some(crate::game::PermKind::Play { cast_only, .. }) => cast_only,
                // CR 715.3d: an adventurer card on an adventure (a creature
                // card: cast only).
                Some(crate::game::PermKind::Adventure) => true,
                _ => continue,
            };
            if k >= act::EXILE_N {
                break;
            }
            let action = act::EXILE + k;
            k += 1;
            let front = if self.def_of(c).halves.is_some() {
                self.split_left_options(p, c, sorcery_ok, classes)
            } else if self.def_of(c).is(types::LAND) {
                if land_ok && !cast_only {
                    FACE_PLAY
                } else {
                    0
                }
            } else if self.cast_route(p, c).is_some_and(|from| self.castable_via(p, c, from, classes, sorcery_ok)) {
                FACE_CAST
            } else {
                0
            };
            let other = self.other_face_options(p, c, sorcery_ok, land_ok && !cast_only, classes);
            if let Some(choice) = crate::faces::face_choice(c, front | other) {
                opts.push(Opt { action, choice });
            }
        }
        let lands_ok = land_ok && self.lands_from_graveyard(p);
        for &c in &self.players[p as usize].graveyard {
            let front = if self.def_of(c).halves.is_some() {
                self.split_left_options(p, c, sorcery_ok, classes)
            } else if self.def_of(c).is(types::LAND) {
                if lands_ok {
                    FACE_PLAY
                } else {
                    0
                }
            } else if self.cast_route(p, c).is_some_and(|from| self.castable_via(p, c, from, classes, sorcery_ok)) {
                FACE_CAST
            } else {
                0
            };
            let other = self.other_face_options(p, c, sorcery_ok, lands_ok, classes);
            if let Some(choice) = crate::faces::face_choice(c, front | other) {
                opts.push(Opt { action: self.graveyard_action(p, c), choice });
            }
        }
    }

    /// Every target requirement has at least `min` legal candidates; for a
    /// modal ability, enough modes can be chosen (a mode whose targets
    /// cannot be chosen cannot be, CR 700.2, 601.2c).
    pub(crate) fn targets_available(&self, ab: &AbilityDef, controller: PlayerId, source: Option<ObjRef>) -> bool {
        match &ab.modes {
            None => {
                let ctx = Ctx { controller, source };
                ab.targets.iter().all(|tr| tr.min == 0 || self.has_candidates(tr, &ctx, tr.min as usize))
            }
            Some(m) => {
                let avail = m.list.iter().filter(|x| self.targets_available(x, controller, source)).count();
                m.min == 0 || if m.repeat { avail > 0 } else { avail >= m.min as usize }
            }
        }
    }

    /// 601.2b / 603.3c: choose the modes of stack item `id`, one decision
    /// per mode instance, in non-decreasing order (each choice of modes
    /// once); only modes whose targets can be chosen, and only while the
    /// rest can still reach the minimum.
    pub(crate) fn choose_modes(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let Some(ar) = self.item_ability(item) else { return };
        let Some(m) = &self.db.ability(ar).modes else { return };
        match self.answer.take() {
            Some(Choice::Number(k)) => {
                self.note(|_| format!("mode {k}"));
                self.stack[pos].modes.push(k);
                self.agenda.push(Task::ChooseModes { id });
                return;
            }
            Some(Choice::Pass) => {
                // 700.2b: a triggered ability with no mode chosen is
                // removed from the stack.
                if item.modes.is_empty() && matches!(ar.slot, crate::db::AbilitySlot::Trigger(_)) {
                    self.note(|_| "no mode: the trigger is removed".to_string());
                    self.stack.remove(pos);
                }
                return;
            }
            Some(other) => unreachable!("mode answer {other:?}"),
            None => {}
        }
        let chosen = item.modes.len();
        if chosen >= m.max as usize {
            return;
        }
        // Escalate (702.120a): another mode needs another card to discard.
        let escalate_cap = m.escalate_discard && chosen >= 1 && {
            let StackKind::Spell { card } = item.kind else { return };
            let p = item.controller;
            // Enough other cards, and the mana payable while keeping them.
            let mana = self.route_mana(card, p, item.from);
            let keep = crate::pay::Keep { discard: Some((chosen as u8, card)), ..Default::default() };
            self.players[p as usize].hand.len() < chosen || !self.can_pay_with(p, &mana, &self.mana_classes(p, None), &keep)
        };
        let (p, source) = (item.controller, Some(self.item_source(item)));
        let avail: Vec<bool> = m.list.iter().map(|x| self.targets_available(x, p, source)).collect();
        let floor = item.modes.last().map_or(0, |&l| if m.repeat { l } else { l + 1 }) as usize;
        let need = (m.min as usize).saturating_sub(chosen + 1);
        let mut opts = Vec::new();
        for k in floor..m.list.len() {
            // After k, the later picks come from k on (k itself if modes
            // repeat): enough must be available.
            let later = if m.repeat { avail[k] as usize * need } else { avail[k + 1..].iter().filter(|&&a| a).count() };
            if avail[k] && later >= need && !escalate_cap {
                opts.push(Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) });
            }
        }
        if chosen >= m.min as usize {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        self.ask(Task::ChooseModes { id }, p, DecisionKind::ChooseMode, opts);
    }

    /// At least `n` legal candidates for `tr` (the same order and rules as
    /// [`target_candidates`](Self::target_candidates), without building it).
    fn has_candidates(&self, tr: &TargetReq, ctx: &Ctx, n: usize) -> bool {
        let ok = |t: Target| self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx);
        let players = [ctx.controller, crate::ids::opp(ctx.controller)].into_iter().map(Target::Player);
        let perms = self.battlefield.iter().map(|&c| Target::Obj(self.obj_ref(c)));
        let spells = self.stack.iter().rev().filter_map(|s| match s.kind {
            StackKind::Spell { card } => Some(self.obj_ref(card)).filter(|&r| Some(r) != ctx.source).map(Target::Obj),
            StackKind::Ability { .. } => None,
        });
        let gy = wants_graveyard(&tr.filter);
        let graves = [ctx.controller, crate::ids::opp(ctx.controller)]
            .into_iter()
            .filter(move |_| gy)
            .flat_map(|q| self.players[q as usize].graveyard.iter().map(|&c| Target::Obj(self.obj_ref(c))))
            // (A card cast from a graveyard is on the stack when it targets.)
            .filter(|&t| Some(t) != ctx.source.map(Target::Obj));
        players.chain(perms).chain(spells).chain(graves).filter(|&t| ok(t)).nth(n - 1).is_some()
    }

    // ---- casting (CR 601.2) ------------------------------------------------

    /// CR 601.2d: divide the amount among the chosen targets of the divided
    /// requirement, at least 1 each: one decision per target but the last
    /// (`NUMBER` = its amount), the last gets the rest.
    pub(crate) fn divide(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let Some(ar) = self.item_ability(item) else { return };
        let Some((req, amount)) = self.db.ability(ar).divide else { return };
        let amount = amount.of(item.x);
        let n = item.targets.iter().filter(|(r, _)| *r == req).count();
        if let Some(Choice::Number(k)) = self.answer.take() {
            self.stack[pos].division.push(k);
        }
        let item = &self.stack[pos];
        let given: u8 = item.division.iter().sum();
        let k = item.division.len();
        if n == 0 || k >= n {
            return;
        }
        let left = amount - given;
        if k + 1 == n {
            self.stack[pos].division.push(left);
            return;
        }
        // Leave at least 1 for each target after this one (amounts within
        // the `NUMBER` range: a larger share goes to a later target).
        let most = (left - (n - k - 1) as u8).min(act::NUMBER_N as u8 - 1);
        let opts = (1..=most).map(|a| Opt { action: act::NUMBER + a as u16, choice: Choice::Number(a) }).collect();
        let p = item.controller;
        self.ask(Task::Divide { id }, p, DecisionKind::DivideDamage, opts);
    }

    /// Can `p` cast `card` for alternative cost `k` now (its condition,
    /// its mana plus taxes, and the rest of it)?
    pub(crate) fn alt_payable(&self, p: PlayerId, card: CardIdx, k: usize, classes: &[crate::pay::Class]) -> bool {
        let a = &self.def_of(card).alt_costs[k];
        if a.turn.is_some_and(|yours| (self.turn.active == p) != yours) {
            return false;
        }
        if a.only_if.as_ref().is_some_and(|c| !self.cond_holds(c, p)) {
            return false;
        }
        let mana = self.alt_mana(card, p, k);
        // (A card cast from hand is on the stack while its cost is paid.)
        self.cost_payable(p, card, &a.cost, &mana, &classes_without(classes, card), Some(card))
    }

    /// The mana of alternative cost `k` with cost increases (CR 601.2f:
    /// they apply to alternative costs too).
    pub(crate) fn alt_mana(&self, card: CardIdx, p: PlayerId, k: usize) -> crate::mana::ManaCost {
        let tax = self.spell_cost(card, p).generic.saturating_sub(self.def_of(card).cost.generic);
        let mut m = self.def_of(card).alt_costs[k].mana;
        m.generic = m.generic.saturating_add(tax);
        m
    }

    /// Can `p` cast `card` for its mana cost (with its additional cost, if
    /// any)?
    pub(crate) fn normal_payable(&self, p: PlayerId, card: CardIdx, classes: &[crate::pay::Class]) -> bool {
        let def = self.def_of(card);
        let cost = self.spell_cost(card, p);
        let cost = self.delve_reduced(card, p, cost, 0);
        if !def.additional.is_empty() {
            self.options_payable(p, card, &cost, classes, Some(card))
        } else {
            self.can_pay_with(p, &cost, &classes_without(classes, card), &Default::default())
        }
    }

    /// 601.2b: choose to pay the mana cost (`NUMBER` 0) or alternative cost
    /// k (`NUMBER` k + 1), among those that can be paid.
    pub(crate) fn cast_alt(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let p = item.controller;
        match self.answer.take() {
            Some(Choice::Number(k)) => {
                self.stack[pos].alt = k;
                self.note(|_| format!("cost {k}"));
                if self.def_of(card).overload {
                    // CR 702.96a: overloaded, "target" becomes "each".
                    self.stack[pos].modes = vec![(k > 0) as u8];
                }
            }
            Some(other) => unreachable!("cost answer {other:?}"),
            None => {
                let classes = self.mana_classes(p, None);
                let mut opts = Vec::new();
                let def = self.def_of(card);
                let plain_targets = !def.overload
                    || def.spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| {
                        self.targets_available(&m.list[0], p, Some(self.obj_ref(card)))
                    });
                if plain_targets && self.normal_payable(p, card, &classes) {
                    opts.push(Opt { action: act::NUMBER, choice: Choice::Number(0) });
                }
                for k in 0..self.def_of(card).alt_costs.len() {
                    if self.alt_payable(p, card, k, &classes) {
                        opts.push(Opt { action: act::NUMBER + 1 + k as u16, choice: Choice::Number(k as u8 + 1) });
                    }
                }
                self.ask(Task::CastAlt { id }, p, DecisionKind::ChooseAltCost, opts);
            }
        }
    }

    /// 601.2a: the card moves to the stack; then targets (601.2c), cost
    /// (601.2f-h) and "becomes cast" (601.2i) run as tasks.
    pub(crate) fn begin_cast(&mut self, p: PlayerId, card: CardIdx) {
        self.begin_cast_face(p, card, None);
    }

    /// Cast `card` with face `face` up (an MDFC's back face, an Adventure:
    /// CR 712.11b, 715.3; `None` = its front): the face is chosen before
    /// the card goes on the stack, and only that face's characteristics
    /// exist there (712.8f, 715.3b).
    pub(crate) fn begin_cast_face(&mut self, p: PlayerId, card: CardIdx, face: Option<crate::ids::DefId>) {
        let from = self.cast_route(p, card).expect("a castable card");
        // "If that spell would be put into your graveyard, exile it
        // instead": a permission from a graveyard that says so (unless
        // flashback, which exiles it anyway, is how it is cast).
        let r = self.obj_ref(card);
        let origin = self.objs[card.i()].zone;
        // A split card is cast as one of its halves (CR 709.3): the left one
        // unless the right one is chosen.
        let face = face.or(self.def_of(card).halves.map(|h| h.0));
        let exile_after = self.objs[card.i()].zone == Zone::Graveyard
            && from != crate::game::CastFrom::Flashback
            && self.permissions.iter().any(|pm| pm.card == r && pm.kind == crate::game::PermKind::CastThenExile && self.permission_ok(pm, p));
        self.move_card_to_stack(card, p);
        if exile_after {
            self.gy_exile.push(card);
        }
        if let Some(f) = face {
            self.objs[card.i()].def = f;
        }
        self.note(|g| format!("P{p} casts {} ({from:?})", g.name(card)));
        if from == crate::game::CastFrom::Flashback {
            self.objs[card.i()].flashback = true;
        }
        let id = self.new_stack_id();
        self.stack.push(StackItem {
            id,
            kind: StackKind::Spell { card },
            controller: p,
            targets: Vec::new(),
            cost_option: 0,
            modes: Vec::new(),
            ev: Default::default(),
            alt: 0,
            from,
            delved: 0,
            delve_floor: 0,
            discarded_land: false,
            x: 0,
            division: Vec::new(),
            attack_as: None,
            noted: Vec::new(),
            flips: 0,
            origin,
        });
        self.agenda.push(Task::CastDone { id });
        if self.def_of(card).spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| m.escalate_discard) {
            // CR 702.120a: the discards, after the mana (which keeps the
            // cards they need).
            self.agenda.push(Task::Escalate { id });
        }
        self.agenda.push(Task::CastPay { id });
        if self.def_of(card).delve {
            self.agenda.push(Task::Delve { id }); // CR 702.66a: while paying
        }

        if self.def_of(card).spell.as_ref().is_some_and(|ab| ab.divide.is_some()) {
            self.agenda.push(Task::Divide { id }); // CR 601.2d
        }
        if self.def_of(card).spell.as_ref().is_some_and(|ab| ab.has_targets()) {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
        }
        if self.def_of(card).additional.len() > 1 {
            self.agenda.push(Task::CastOption { id });
        }
        if self.def_of(card).x_pips > 0 {
            self.agenda.push(Task::ChooseX { id }); // CR 601.2b
        }
        if self.def_of(card).overload {
            // Not overloaded unless the alternative cost is chosen.
            if let Some(pos) = self.item_index(id) {
                self.stack[pos].modes = vec![0];
            }
        }
        if self.def_of(card).spell.as_ref().is_some_and(|ab| ab.modes.is_some())
            && !self.def_of(card).kicker
            && !self.def_of(card).overload
        {
            self.agenda.push(Task::ChooseModes { id }); // CR 601.2b, 700.2a
        }
        // CR 118.9a: one alternative cost at most (flashback, escape and
        // "without paying its mana cost" are alternative costs).
        if !self.def_of(card).alt_costs.is_empty() && from == crate::game::CastFrom::Hand {
            self.agenda.push(Task::CastAlt { id }); // CR 601.2b
        }
    }

    /// 601.2b: choose which additional cost to pay (among those that can
    /// be paid along with the rest of the cost).
    pub(crate) fn cast_option(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let p = item.controller;
        match self.answer.take() {
            Some(Choice::Number(k)) => {
                self.stack[pos].cost_option = k;
                if self.def_of(card).kicker {
                    // Kicked or not: the spell's effect follows (CR 702.33d).
                    self.stack[pos].modes = vec![k];
                    self.note(|_| format!("kicked: {}", k == 1));
                }
            }
            Some(other) => unreachable!("cost option answer {other:?}"),
            None => {
                let def = self.def_of(card);
                let classes = self.mana_classes(p, None);
                let cost = self.delve_reduced(card, p, self.route_mana(card, p, item.from), self.escape_exiles(card, item.from));
                let src = Some(self.obj_ref(card));
                let kicker_ok = |k: usize| {
                    !def.kicker
                        || def.spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| self.targets_available(&m.list[k], p, src))
                };
                let opts: Vec<Opt> = (0..def.additional.len())
                    .filter(|&k| kicker_ok(k) && self.cost_payable(p, card, &def.additional[k], &cost.plus(&def.additional[k].mana), &classes, None))
                    .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) })
                    .collect();
                self.ask(Task::CastOption { id }, p, DecisionKind::ChooseCost, opts);
            }
        }
    }

    pub(crate) fn new_stack_id(&mut self) -> u32 {
        let id = self.next_stack_id;
        self.next_stack_id += 1;
        id
    }

    pub(crate) fn item_index(&self, id: u32) -> Option<usize> {
        self.stack.iter().position(|s| s.id == id)
    }

    /// The ability a stack item resolves (`None`: a permanent spell).
    pub(crate) fn item_ability(&self, item: &StackItem) -> Option<AbilityRef> {
        match item.kind {
            StackKind::Spell { card } => {
                let def = self.objs[card.i()].def;
                self.db.def(def).spell.as_ref().map(|_| AbilityRef { def, slot: AbilitySlot::Spell, mode: None })
            }
            StackKind::Ability { ability, .. } => Some(ability),
        }
    }

    /// The object a stack item's ability comes from (the spell itself, or
    /// the permanent of an ability).
    pub(crate) fn item_source(&self, item: &StackItem) -> ObjRef {
        match item.kind {
            StackKind::Spell { card } => self.obj_ref(card),
            StackKind::Ability { source, .. } => source,
        }
    }

    /// Choose targets for stack item `id`, requirement `req` onwards
    /// (CR 601.2c / 603.3d; one decision per target).
    pub(crate) fn choose_targets(&mut self, id: u32, req: u8) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let Some(ar) = self.item_ability(item) else { return };
        let reqs = self.db.ability(ar).reqs(&item.modes);
        let Some(&tr) = reqs.get(req as usize) else { return };
        let ctx = Ctx { controller: item.controller, source: Some(self.item_source(item)) };
        let already: Vec<Target> =
            item.targets.iter().filter(|(r, _)| *r == req).map(|&(_, t)| t).collect();
        match self.answer.take() {
            Some(Choice::Pass) => {
                self.agenda.push(Task::ChooseTargets { id, req: req + 1 });
                return;
            }
            Some(Choice::Target(t)) => {
                self.note(|g| format!("target {}", g.describe_target(t)));
                self.stack[pos].targets.push((req, t));
                self.agenda.push(Task::ChooseTargets { id, req });
                return;
            }
            Some(other) => unreachable!("target answer {other:?}"),
            None => {}
        }
        // Divided damage: each target gets at least 1 (CR 601.2d), so no
        // more targets than the amount (an X spell's X is announced first).
        let max = match self.db.ability(ar).divide {
            Some((r, amount)) if r == req && ar.mode.is_none() => (tr.max).min(amount.of(item.x)),
            _ => tr.max,
        };
        if already.len() >= max as usize {
            self.agenda.push(Task::ChooseTargets { id, req: req + 1 });
            return;
        }
        let cands = self.target_candidates(tr, &ctx, &already);
        let mut opts: Vec<Opt> = cands
            .iter()
            .map(|&t| Opt { action: self.target_action(ctx.controller, t), choice: Choice::Target(t) })
            .collect();
        if already.len() >= tr.min as usize {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        if opts.is_empty() {
            // Only reachable if legality changed between the cast check and
            // now, which nothing can do yet.
            panic!("no legal target for a required target");
        }
        self.ask(Task::ChooseTargets { id, req }, ctx.controller, DecisionKind::Target, opts);
    }

    /// 601.2f-h: determine the total cost and pay it.
    pub(crate) fn cast_pay(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let def = self.def_of(card);
        let p = item.controller;
        if item.alt > 0 {
            let k = item.alt as usize - 1;
            let mut pm = PayMana::new(p, self.alt_mana(card, p, k));
            pm.keep = Self::keep_for(card, &def.alt_costs[k].cost, None);
            self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
            if !pm.cost.is_zero() {
                self.agenda.push(Task::PayMana(pm));
            }
            return;
        }
        let mut cost = self.route_mana(card, p, item.from).less_generic(item.delved);
        if let Some(extra) = self.route_extra(card, item.from, item.cost_option) {
            cost = cost.plus(&extra.mana);
        }
        cost.generic = cost.generic.saturating_add(item.x.saturating_mul(def.x_pips));
        let mut pm = PayMana::new(p, cost);
        if let Some(extra) = self.route_extra(card, item.from, item.cost_option) {
            pm.keep = Self::keep_for(card, extra, None);
            self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
        }
        let escalated = item.modes.len().saturating_sub(1) as u8;
        if escalated > 0 && def.spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| m.escalate_discard) {
            pm.keep.discard = Some((escalated, card));
        }
        let _ = def;
        self.agenda.push(Task::PayMana(pm));
    }

    /// 601.2i: the spell becomes cast; "whenever you cast" abilities
    /// trigger.
    pub(crate) fn cast_done(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let p = item.controller;
        let pl = &mut self.players[p as usize];
        pl.spells_cast = pl.spells_cast.saturating_add(1);
        pl.spells_game = pl.spells_game.saturating_add(1);
        if !self.def_of(card).is(types::CREATURE) {
            let pl = &mut self.players[p as usize];
            pl.noncreature_cast = pl.noncreature_cast.saturating_add(1);
        }
        let r = self.obj_ref(card);
        self.event(crate::triggers::Event {
            kind: crate::db::EventKind::IsCast,
            a: crate::triggers::Subj::Obj(r),
            b: crate::triggers::Subj::Player(p),
            amount: 0,
        });
    }

    // ---- targets (CR 115) --------------------------------------------------

    /// Legal candidates for a target requirement, in canonical order:
    /// players (controller first), permanents (battlefield order), spells
    /// (stack, top first). A spell or ability cannot target itself (115.5);
    /// `exclude` holds the targets already chosen for this requirement
    /// (the same object once per "target", 115.3).
    pub(crate) fn target_candidates(&self, tr: &TargetReq, ctx: &Ctx, exclude: &[Target]) -> Vec<Target> {
        let mut out = Vec::new();
        let consider = |t: Target, out: &mut Vec<Target>| {
            if !exclude.contains(&t) && self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx) {
                out.push(t);
            }
        };
        for q in [ctx.controller, crate::ids::opp(ctx.controller)] {
            consider(Target::Player(q), &mut out);
        }
        for &c in &self.battlefield {
            consider(Target::Obj(self.obj_ref(c)), &mut out);
        }
        for s in self.stack.iter().rev() {
            if let StackKind::Spell { card } = s.kind {
                let r = self.obj_ref(card);
                if Some(r) != ctx.source {
                    consider(Target::Obj(r), &mut out);
                }
            }
        }
        if wants_graveyard(&tr.filter) {
            for q in [ctx.controller, crate::ids::opp(ctx.controller)] {
                for &c in &self.players[q as usize].graveyard {
                    let r = self.obj_ref(c);
                    if Some(r) != ctx.source {
                        consider(Target::Obj(r), &mut out);
                    }
                }
            }
        }
        out
    }

    /// Hexproof and shroud (CR 702.11, 702.18); protection (702.16b: not
    /// by spells with that quality, nor abilities from sources with it).
    pub(crate) fn targetable(&self, t: Target, ctx: &Ctx) -> bool {
        match t {
            Target::Player(_) => true,
            Target::Obj(r) => {
                if self.objs[r.idx.i()].zone != Zone::Battlefield {
                    return true;
                }
                let k = self.keywords_of(r.idx);
                if k.has(Kw::Shroud) || (k.has(Kw::Hexproof) && self.objs[r.idx.i()].controller != ctx.controller) {
                    return false;
                }
                match self.source_quality(ctx) {
                    Some(q) => !self.protected_from(r.idx, &q),
                    None => true,
                }
            }
        }
    }

    /// The characteristics of the object a spell or ability comes from:
    /// the object itself while it exists, else its definition (last known
    /// information), found through the ability on the stack or waiting to
    /// be put there (never through the card, which may be hidden now).
    pub(crate) fn source_quality(&self, ctx: &Ctx) -> Option<crate::game::DamageSource> {
        let r = ctx.source?;
        // (A card being cast is its own source wherever it is cast from.)
        if self.live(r)
            && matches!(self.objs[r.idx.i()].zone, Zone::Battlefield | Zone::Stack | Zone::Hand | Zone::Graveyard | Zone::Exile)
        {
            return Some(crate::game::DamageSource { controller: ctx.controller, ..self.damage_source_of(r.idx) });
        }
        let def = self
            .stack
            .iter()
            .find_map(|s| match s.kind {
                StackKind::Ability { ability, source } if source == r => Some(ability.def),
                _ => None,
            })
            .or_else(|| self.waiting.iter().find(|w| w.source == r).map(|w| w.source_def))?;
        let d = self.db.def(def);
        Some(crate::game::DamageSource {
            controller: ctx.controller,
            deathtouch: false,
            lifelink: false,
            colors: d.colors,
            types: d.types,
            obj: None,
        })
    }

    /// Is `t` (still) a legal choice for `tr` (CR 608.2b)?
    pub(crate) fn target_legal(&self, tr: &TargetReq, t: Target, ctx: &Ctx) -> bool {
        if let Target::Obj(r) = t {
            if !self.live(r) {
                return false;
            }
        }
        self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx)
    }

    /// Top-level filter. Players and permanents are one universe (a bare
    /// `Not(Creature)` matches players too, as in the IR); characteristic
    /// predicates mean permanents. Spells on the stack match only through
    /// `Spell(f)` (possibly inside `And` / `Or`).
    pub(crate) fn filter_ok(&self, f: &Filter, t: Target, ctx: &Ctx) -> bool {
        match t {
            Target::Player(p) => match f {
                Filter::Any | Filter::Player => true,
                Filter::Opponent => p != ctx.controller,
                Filter::DrewThisTurn(n) => self.players[p as usize].drawn == *n,
                Filter::OathTarget => {
                    let ap = self.turn.active;
                    p != ap && self.creatures_of(p).count() > self.creatures_of(ap).count()
                }
                Filter::FirstDrawInDrawStep => {
                    self.turn.step == crate::game::Step::Draw
                        && self.turn.active == p
                        && self.players[p as usize].draw_step_draws == 1
                }
                Filter::And(a, b) => self.filter_ok(a, t, ctx) && self.filter_ok(b, t, ctx),
                Filter::Or(v) => v.iter().any(|x| self.filter_ok(x, t, ctx)),
                Filter::Not(x) => !self.filter_ok(x, t, ctx),
                _ => false,
            },
            Target::Obj(r) => {
                if !self.live(r) {
                    return false;
                }
                match self.objs[r.idx.i()].zone {
                    Zone::Battlefield => self.perm_ok(f, r.idx, ctx),
                    Zone::Stack => self.spell_ok(f, r.idx, ctx),
                    Zone::Graveyard => self.graveyard_ok(f, r.idx, ctx),
                    _ => false,
                }
            }
        }
    }

    fn perm_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::Player
            | Filter::Spell(_)
            | Filter::Opponent
            | Filter::DrewThisTurn(_)
            | Filter::FirstDrawInDrawStep
            | Filter::OathTarget
            | Filter::InGraveyard(_) => false,
            Filter::And(a, b) => self.perm_ok(a, c, ctx) && self.perm_ok(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.perm_ok(x, c, ctx)),
            Filter::Not(x) => !self.perm_ok(x, c, ctx),
            _ => self.card_matches(f, c, ctx),
        }
    }

    /// A card in a graveyard matches only through `InGraveyard(f)`.
    fn graveyard_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::InGraveyard(inner) => self.card_matches(inner, c, ctx),
            Filter::And(a, b) => self.graveyard_ok(a, c, ctx) && self.graveyard_ok(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.graveyard_ok(x, c, ctx)),
            _ => false,
        }
    }

    fn spell_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::Spell(inner) => self.card_matches(inner, c, ctx),
            Filter::And(a, b) => self.spell_ok(a, c, ctx) && self.spell_ok(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.spell_ok(x, c, ctx)),
            _ => false,
        }
    }

    /// A filter applied to an object's characteristics (and its controller
    /// and identity relative to `ctx`), whatever its zone.
    pub(crate) fn card_matches(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        let def = self.def_of(c);
        match f {
            Filter::Any => true,
            Filter::Player | Filter::Spell(_) | Filter::Opponent => false,
            Filter::Type(bits) => self.types_of(c) & bits != 0,
            Filter::Color(m) => self.colors_of(c) & m != 0,
            Filter::TotalPtAtMost(n) => self.is_creature(c) && self.power(c) + self.toughness(c) <= *n,
            Filter::Subtype(s) => self.has_subtype(c, *s),
            Filter::Supertype(bits) => def.supertypes & bits != 0,
            Filter::ManaValueAtMost(n) => self.mana_value_of(c) <= *n,
            Filter::PowerAtLeast(n) => self.is_creature(c) && self.power(c) >= *n,
            Filter::PowerAtMost(n) => self.is_creature(c) && self.power(c) <= *n,
            Filter::ToughnessAtMost(n) => self.is_creature(c) && self.toughness(c) <= *n,
            Filter::Controller(you) => (self.objs[c.i()].controller == ctx.controller) == *you,
            Filter::Source => ctx.source.is_some_and(|s| s.idx == c && self.live(s)),
            Filter::Token => def.token,
            Filter::Attacking => self.objs[c.i()].attacking,
            Filter::Tapped => self.objs[c.i()].tapped,
            Filter::DrewThisTurn(_) | Filter::InGraveyard(_) => false,
            Filter::Evoked => self.objs[c.i()].evoked,
            Filter::Owner(you) => (self.objs[c.i()].owner == ctx.controller) == *you,
            Filter::FirstDrawInDrawStep | Filter::OathTarget => false,
            Filter::NoCounters(k) => self.counters_on(c, *k) == 0,
            Filter::HasCounter(k) => self.counters_on(c, *k) > 0,
            // CR 509.1h: an attacking creature becomes blocked or unblocked
            // once blockers are declared.
            Filter::Unblocked => {
                let o = &self.objs[c.i()];
                o.attacking
                    && !o.blocked
                    && matches!(
                        self.turn.step,
                        crate::game::Step::DeclareBlockers
                            | crate::game::Step::FirstStrikeDamage
                            | crate::game::Step::CombatDamage
                            | crate::game::Step::EndCombat
                    )
            }
            Filter::ManaValueAtLeast(n) => self.mana_value_of(c) >= *n,
            Filter::ManaCostIn(v) => {
                // (Back faces have no mana cost, CR 712.8e.)
                !def.is(types::LAND) && def.x_pips == 0 && def.front_mv.is_none() && v.contains(&def.cost)
            }
            Filter::HasKeyword(k) => self.has_kw(c, *k),
            Filter::Colorless => self.colors_of(c) == 0,
            // A spell: one or more of its targets (as chosen, still the
            // same objects) matches.
            Filter::TargetsMatching(inner) => self
                .stack
                .iter()
                .find(|s| s.kind == StackKind::Spell { card: c })
                .is_some_and(|s| {
                    s.targets.iter().any(|&(_, t)| match t {
                        Target::Obj(r) => {
                            self.live(r)
                                && self.objs[r.idx.i()].zone == Zone::Battlefield
                                && self.card_matches(inner, r.idx, ctx)
                        }
                        Target::Player(_) => false,
                    })
                }),
            Filter::And(a, b) => self.card_matches(a, c, ctx) && self.card_matches(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.card_matches(x, c, ctx)),
            Filter::Not(x) => !self.card_matches(x, c, ctx),
        }
    }
}

/// Retrace's additional cost (CR 702.81a): discard a land card.
pub(crate) fn retrace_cost() -> &'static crate::db::Cost {
    static COST: std::sync::OnceLock<crate::db::Cost> = std::sync::OnceLock::new();
    COST.get_or_init(|| crate::db::Cost { discard: 1, discard_filter: Some(Filter::Type(types::LAND)), ..Default::default() })
}

/// Can a filter match a card in a graveyard (`InGraveyard`)?
fn wants_graveyard(f: &Filter) -> bool {
    match f {
        Filter::InGraveyard(_) => true,
        Filter::And(a, b) => wants_graveyard(a) || wants_graveyard(b),
        Filter::Or(v) => v.iter().any(wants_graveyard),
        _ => false,
    }
}

/// `classes` without the sources of one card.
pub(crate) fn classes_without(classes: &[crate::pay::Class], card: CardIdx) -> Vec<crate::pay::Class> {
    classes
        .iter()
        .filter_map(|cl| {
            let members: Vec<_> = cl.members.iter().copied().filter(|s| s.card != card).collect();
            (!members.is_empty()).then(|| crate::pay::Class { members, ..cl.clone() })
        })
        .collect()
}
