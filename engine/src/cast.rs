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
            cost.generic = cost.generic.saturating_sub(self.discount(d, p));
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

    /// How much a cost reduction takes off for `p` now (generic mana).
    pub(crate) fn discount(&self, d: &crate::db::Discount, p: PlayerId) -> u8 {
        let ctx = Ctx { controller: p, source: None };
        match d {
            crate::db::Discount::PerBasicLandType(n) => self.basic_land_types(p).saturating_mul(*n),
            crate::db::Discount::PerControlled(f, n) => {
                (self.permanents_of(p).filter(|&c| self.card_matches(f, c, &ctx)).count() as u8).saturating_mul(*n)
            }
        }
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
        // (Jennifer Walters): only lands and abilities are left. (Checked
        // only when a spell could be cast.)
        let casts = |o: &Opt| matches!(o.choice, Choice::Cast(_) | Choice::CastFace(_) | Choice::Faces(..));
        if opts.iter().any(casts) && !self.may_cast_spells(p) {
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
        if self.players[p as usize].spells_cast >= 1 && self.battlefield.iter().any(|&c| self.abilities(c).one_spell_each_turn) {
            return false;
        }
        let ap = self.turn.active;
        !(ap != p && self.permanents_of(ap).any(|c| self.abilities(c).opp_no_spells_your_turn))
    }

    fn priority_options_all(&self, p: PlayerId) -> Vec<Opt> {
        use crate::decision::{FACE_CAST, FACE_PLAY};
        // (Room for the usual options without growing.)
        let mut opts = Vec::with_capacity(16);
        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        let sorcery_ok = self.turn.active == p && self.turn.step.is_main() && self.stack.is_empty();
        let drops = 1 + self.permanents_of(p).map(|c| self.abilities(c).extra_lands as u32).sum::<u32>();
        let land_ok = sorcery_ok && (self.players[p as usize].lands_played as u32) < drops;
        let classes = self.mana_classes(p, None);
        let mut plain = None;
        let slots = self.hand_slots_in(p);
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
            let other = if def.other_face().is_some() { self.other_face_options(p, c, sorcery_ok, land_ok, &classes) } else { 0 };
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
        // Improvise (CR 702.126): the untapped artifacts help pay (not with
        // the shared plain candidates, which have none).
        let with_artifacts;
        let cls = if def.improvise {
            with_artifacts = [classes, &self.improvise_classes(p, None)[..]].concat();
            &with_artifacts[..]
        } else {
            classes
        };
        let alt_ok = (0..def.alt_costs.len()).any(|k| self.alt_payable(p, c, k, cls));
        // Spree (CR 702.172a): at least one mode's additional cost is paid.
        let extras = self.spree_extras(c, p);
        // `extra`: the mana of additional costs announced (CR 601.2f: added
        // before reductions).
        let mut payable = |extra: &crate::mana::ManaCost| -> bool {
            if !def.additional.is_empty() {
                // (Excluding the card itself: on the stack while it is paid.)
                return self.options_payable(p, c, crate::game::CastFrom::Hand, extra, 0, cls, Some(c));
            }
            let cost = self.delve_reduced(c, p, self.route_mana_plus(c, p, crate::game::CastFrom::Hand, extra), 0);
            if self.hand_mana_source(c) || def.improvise {
                // A card cast from hand is on the stack while its cost is
                // paid: its own mana ability (a Spirit Guide's) cannot help.
                self.can_pay_without(p, &cost, cls, &Default::default(), Some(c))
            } else {
                let plain = plain.get_or_insert_with(|| self.plain_cands(p, cls));
                self.can_pay_plain(p, &cost, cls, plain)
            }
        };
        if alt_ok {
            // castable for an alternative cost (601.2b); the mana cost
            // may be payable too (asked on casting)
        } else if let Some(extras) = &extras {
            if !extras.iter().any(|x| payable(x)) {
                return false;
            }
        } else if !payable(&Default::default()) {
            return false;
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
        self.instant_speed(p, self.def_of(card)) && !self.permanents_of(crate::ids::opp(p)).any(|c| self.abilities(c).opp_sorcery_speed)
    }

    /// Could `p` cast a spell with definition `def` any time they could cast
    /// an instant (an instant, flash, a sorcery with Teferi's +1)?
    pub(crate) fn instant_speed(&self, p: PlayerId, def: &crate::db::CardDef) -> bool {
        def.is(types::INSTANT) || def.keywords.has(Kw::Flash) || (def.is(types::SORCERY) && self.players[p as usize].flash_sorceries)
    }

    /// Does permission `pm` let `p` play its card now?
    pub(crate) fn permission_ok(&self, pm: &crate::game::Permission, p: PlayerId) -> bool {
        if !self.live(pm.card) || pm.turn.is_some_and(|t| t != self.turn.number) {
            return false;
        }
        // CR 722.3c: the prepared permanent's controller, while it stays
        // prepared.
        if let crate::game::PermKind::Prepared { source } = pm.kind {
            return self.prepare_copy_live(pm.card.idx) && self.objs[source.idx.i()].controller == p;
        }
        if pm.player != p {
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
                self.permissions.iter().find(|pm| {
                    pm.card == r
                        && self.permission_ok(pm, p)
                        && matches!(pm.kind, crate::game::PermKind::Adventure | crate::game::PermKind::Prepared { .. })
                })
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
                // CR 722.3c: a copy of a prepare spell, for its mana cost.
                crate::game::PermKind::Prepared { .. } => Some(CastFrom::Permission { free: false, any_mana: false }),
                crate::game::PermKind::Flashback | crate::game::PermKind::CastThenExile => None,
            },
            _ => None,
        }
    }

    /// The mana to pay to cast `card` the way `from` says, with increases
    /// and reductions.
    pub(crate) fn route_mana(&self, card: CardIdx, p: PlayerId, from: crate::game::CastFrom) -> crate::mana::ManaCost {
        self.route_mana_plus(card, p, from, &Default::default())
    }

    /// [`route_mana`](Self::route_mana) with the mana of additional costs
    /// `extra` (spree modes, squad, offspring, replicate): CR 601.2f, the
    /// total cost is the mana cost (or alternative cost) plus additional
    /// costs and increases, less reductions, so a reduction can reduce the
    /// additional costs too.
    pub(crate) fn route_mana_plus(&self, card: CardIdx, p: PlayerId, from: crate::game::CastFrom, extra: &crate::mana::ManaCost) -> crate::mana::ManaCost {
        use crate::game::CastFrom;
        let def = self.def_of(card);
        let with = |base: crate::mana::ManaCost| self.cost_with(card, p, base.plus(extra));
        match from {
            CastFrom::Hand => with(def.cost),
            // "The flashback cost is equal to its mana cost" when granted.
            CastFrom::Flashback => with(def.flashback.unwrap_or(def.cost)),
            CastFrom::Escape => with(def.escape.as_ref().map_or(def.cost, |e| e.0)),
            // "Without paying its mana cost" (an alternative cost of no
            // mana, CR 118.9; increases still apply, 601.2f).
            CastFrom::Permission { free: true, .. } => with(Default::default()),
            CastFrom::Permission { free: false, any_mana: true } => {
                // "Spend mana as though it were mana of any color" (CR
                // 609.4b): every coloured, hybrid and Phyrexian symbol is
                // paid by any mana ({C} stays {C}).
                let mut c = with(def.cost);
                let colored: u8 = c.pips[..5].iter().sum::<u8>() + c.hybrid.iter().sum::<u8>() + c.phyrexian.iter().sum::<u8>();
                c.generic = c.generic.saturating_add(colored);
                c.pips[..5].fill(0);
                c.hybrid = Default::default();
                c.phyrexian = Default::default();
                c
            }
            CastFrom::Permission { .. } => with(def.cost),
            CastFrom::Retrace => with(def.cost),
            // "The mayhem cost is equal to its mana cost" (an alternative
            // cost, CR 702.187b; increases apply, 601.2f).
            CastFrom::Mayhem => with(def.cost),
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
        // Improvise (CR 702.126).
        let with_artifacts;
        let classes = if def.improvise {
            with_artifacts = [classes, &self.improvise_classes(p, None)[..]].concat();
            &with_artifacts[..]
        } else {
            classes
        };
        let keep = self.escape_exiles(card, from);
        // `extra`: the mana of additional costs announced (CR 601.2f).
        let paid = |extra: &crate::mana::ManaCost| {
            let mana = self.delve_reduced(card, p, self.route_mana_plus(card, p, from, extra), keep);
            match from {
                crate::game::CastFrom::Escape | crate::game::CastFrom::Retrace => {
                    self.cost_payable(p, card, self.route_extra(card, from, 0).expect("an escape or retrace cost"), &mana, classes, None)
                }
                _ if !def.additional.is_empty() => self.options_payable(p, card, from, extra, keep, classes, None),
                _ => self.can_pay_with(p, &mana, classes, &Default::default()),
            }
        };
        // Spree (CR 702.172a): at least one mode's additional cost is paid.
        let ok = match self.spree_extras(card, p) {
            Some(extras) => extras.iter().any(|x| paid(x)),
            None => paid(&Default::default()),
        };
        if !ok {
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
        let free = matches!(item.from, crate::game::CastFrom::Permission { free: true, .. });
        let mut most = 0u8;
        if !free {
            // CR 601.2f: X is part of the mana cost, before reductions.
            let mut tmp = item.clone();
            while most < (act::NUMBER_N - 1) as u8 {
                tmp.x = most + 1;
                if !self.can_pay(p, &self.total_mana(card, &tmp)) {
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
        // A copy "with no mana cost" (eternalize) has mana value 0.
        if self.exc(c).is_some_and(|e| e.no_cost) {
            return 0;
        }
        let mv = self.def_of(c).mana_value();
        if self.objs[c.i()].zone != Zone::Stack {
            return mv;
        }
        let x = self.stack.iter().find(|s| s.kind == StackKind::Spell { card: c }).map_or(0, |s| s.x);
        mv.saturating_add(x.saturating_mul(self.def_of(c).x_pips))
    }

    /// Can `p` pay one of the spell's additional-cost options (CR 601.2b),
    /// cast the way `from` says with the additional mana `extra` announced:
    /// the option's mana is part of the total cost before reductions
    /// (601.2f; `keep`: graveyard cards escape exiles, left out of delve).
    /// For kicker, only an option whose effect has its targets.
    #[allow(clippy::too_many_arguments)]
    fn options_payable(
        &self,
        p: PlayerId,
        card: CardIdx,
        from: crate::game::CastFrom,
        extra: &crate::mana::ManaCost,
        keep: u8,
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
                && {
                    let cost = self.delve_reduced(card, p, self.route_mana_plus(card, p, from, &extra.plus(&o.mana)), keep);
                    self.cost_payable(p, card, o, &cost, classes, exclude)
                }
        })
    }

    /// A delve spell's cost with as much generic mana as its caster's
    /// graveyard can pay (castability, CR 702.66).
    /// (`keep`: graveyard cards another part of the cost exiles, escape.)
    pub(crate) fn delve_reduced(&self, card: CardIdx, p: PlayerId, cost: crate::mana::ManaCost, keep: u8) -> crate::mana::ManaCost {
        if !self.def_of(card).delve {
            return cost;
        }
        let n = self.graveyard_cards(p).filter(|&c| c != card).count().saturating_sub(keep as usize).min(255) as u8;
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
        let cost = self.total_mana(card, item).less_generic(item.delved);
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
        // above for the picks still needed. (Cards only: a token there isn't
        // one, CR 108.2.)
        let card_at = |i: usize| !self.is_token(gy[i]);
        let n_cards = (0..gy.len()).filter(|&i| card_at(i)).count();
        let mut opts: Vec<Opt> = gy
            .iter()
            .enumerate()
            .filter(|&(i, _)| card_at(i) && i >= floor && (i..gy.len()).filter(|&j| card_at(j)).count() >= need && n_cards > keep)
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
                // CR 722.3c: a copy of a prepare spell (cast only).
                Some(crate::game::PermKind::Prepared { .. }) => true,
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
            let other = if self.def_of(c).other_face().is_some() { self.other_face_options(p, c, sorcery_ok, land_ok && !cast_only, classes) } else { 0 };
            if let Some(choice) = crate::faces::face_choice(c, front | other) {
                opts.push(Opt { action, choice });
            }
        }
        let lands_ok = land_ok && self.lands_from_graveyard(p);
        // (A token in a graveyard is not a card: nothing casts or plays it.)
        for c in self.graveyard_cards(p) {
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
            let other = if self.def_of(c).other_face().is_some() { self.other_face_options(p, c, sorcery_ok, lands_ok, classes) } else { 0 };
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
                if ab.targets.iter().any(|tr| tr.distinct != 0) {
                    let reqs: Vec<&TargetReq> = ab.targets.iter().collect();
                    return self.distinct_choosable(&reqs, 0, &mut Vec::new(), &ctx);
                }
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
        // (Not the ability itself, already on the stack: CR 115.5.)
        self.choosing_for = id;
        let mut avail: Vec<bool> = m.list.iter().map(|x| self.targets_available(x, p, source)).collect();
        self.choosing_for = 0;
        // Spree (CR 702.172a, 700.2h): a mode whose additional cost can't be
        // paid with the others chosen can't be chosen.
        if !m.costs.is_empty() {
            if let StackKind::Spell { card } = item.kind {
                for (k, a) in avail.iter_mut().enumerate() {
                    *a = *a && self.can_pay(p, &self.announced_cost(card, item, &m.costs[k]));
                }
            }
        }
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

    /// Can requirements `reqs[k..]` each get their `min` targets (at most
    /// one each where they must differ, the lowering checks), none being a
    /// target `chosen` for a requirement it must differ from (CR 115.3)?
    fn distinct_choosable(&self, reqs: &[&TargetReq], k: usize, chosen: &mut Vec<(u8, Target)>, ctx: &Ctx) -> bool {
        let Some(tr) = reqs.get(k) else { return true };
        if tr.min == 0 {
            return self.distinct_choosable(reqs, k + 1, chosen, ctx);
        }
        if tr.distinct == 0 && !reqs[k + 1..].iter().any(|r| r.distinct & (1 << k) != 0) {
            return self.has_candidates(tr, ctx, tr.min as usize) && self.distinct_choosable(reqs, k + 1, chosen, ctx);
        }
        let exclude: Vec<Target> = chosen.iter().filter(|(r, _)| tr.distinct & (1 << r) != 0).map(|&(_, t)| t).collect();
        for t in self.target_candidates(tr, ctx, &exclude) {
            chosen.push((k as u8, t));
            let ok = self.distinct_choosable(reqs, k + 1, chosen, ctx);
            chosen.pop();
            if ok {
                return true;
            }
        }
        false
    }

    /// With `t` chosen for requirement `req`, can the later requirements
    /// still be chosen (with the targets `chosen` so far)?
    fn later_distinct_ok(&self, reqs: &[&TargetReq], req: u8, t: Target, chosen: &[(u8, Target)], ctx: &Ctx) -> bool {
        let mut c: Vec<(u8, Target)> = chosen.to_vec();
        c.push((req, t));
        self.distinct_choosable(reqs, req as usize + 1, &mut c, ctx)
    }

    /// The mana of the additional costs a spell's announcements added
    /// (CR 601.2b, f): the times an additional cost is paid (replicate,
    /// squad, offspring) and spree's mode costs (702.172a).
    pub(crate) fn extra_costs(&self, item: &StackItem) -> crate::mana::ManaCost {
        let StackKind::Spell { card } = item.kind else { return Default::default() };
        let def = self.def_of(card);
        let mut c = crate::mana::ManaCost::default();
        if let Some(tc) = &def.times_cost {
            for _ in 0..item.times {
                c = c.plus(&tc.mana);
            }
        }
        if let Some(m) = def.spell.as_ref().and_then(|ab| ab.modes.as_ref()) {
            for &k in &item.modes {
                if let Some(x) = m.costs.get(k as usize) {
                    c = c.plus(x);
                }
            }
        }
        c
    }

    /// The mana of spell `card`'s cost as announced so far (CR 601.2b) and
    /// `more`: its mana cost or alternative cost plus the additional costs,
    /// with increases and reductions (601.2f).
    fn announced_cost(&self, card: CardIdx, item: &StackItem, more: &crate::mana::ManaCost) -> crate::mana::ManaCost {
        let p = item.controller;
        let extra = self.extra_costs(item).plus(more);
        match item.alt {
            0 => self.route_mana_plus(card, p, item.from, &extra),
            k => self.cost_with(card, p, self.def_of(card).alt_costs[k as usize - 1].mana.plus(&extra)),
        }
    }

    /// The total mana cost of spell `card` as cast so far (CR 601.2f): its
    /// mana cost (with X) or the alternative cost it is cast for, plus the
    /// mana of every additional cost announced (the option chosen: kicker,
    /// buyback; spree modes; times paid) and the increases, less the
    /// reductions (which may reduce the additional costs too). Delve and
    /// improvise pay from it later (702.66b, 702.126b).
    pub(crate) fn total_mana(&self, card: CardIdx, item: &StackItem) -> crate::mana::ManaCost {
        let def = self.def_of(card);
        let mut more = crate::mana::ManaCost::default();
        if item.alt == 0 {
            if let Some(c) = self.route_extra(card, item.from, item.cost_option) {
                more = more.plus(&c.mana);
            }
        }
        more.generic = more.generic.saturating_add(item.x.saturating_mul(def.x_pips));
        self.announced_cost(card, item, &more)
    }

    /// A spree spell's mode costs (CR 702.172a) for the modes whose targets
    /// `p` could choose now; `None` for other spells.
    fn spree_extras(&self, card: CardIdx, p: PlayerId) -> Option<Vec<crate::mana::ManaCost>> {
        let m = self.def_of(card).spell.as_ref()?.modes.as_ref()?;
        if m.costs.is_empty() {
            return None;
        }
        let src = Some(self.obj_ref(card));
        Some(
            m.list
                .iter()
                .zip(m.costs.iter())
                .filter(|(ab, _)| self.targets_available(ab, p, src))
                .map(|(_, c)| *c)
                .collect(),
        )
    }

    /// CR 601.2b: announce how many times an additional cost is paid
    /// (replicate, squad: any number; offspring: once or not), among the
    /// numbers that leave the cost payable (`NUMBER` = times; at most 31).
    pub(crate) fn choose_times(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let StackKind::Spell { card } = self.stack[pos].kind else { return };
        let p = self.stack[pos].controller;
        match self.answer.take() {
            Some(Choice::Number(n)) => {
                self.stack[pos].times = n;
                self.note(|_| format!("additional cost paid {n} time(s)"));
                return;
            }
            Some(other) => unreachable!("times answer {other:?}"),
            None => {}
        }
        let Some(tc) = self.def_of(card).times_cost.clone() else { return };
        let mut most = 0u8;
        let mut more = crate::mana::ManaCost::default();
        while most < tc.max.min(act::NUMBER_N as u8 - 1) {
            more = more.plus(&tc.mana);
            if !self.can_pay(p, &self.announced_cost(card, &self.stack[pos], &more)) {
                break;
            }
            most += 1;
        }
        let opts = (0..=most).map(|n| Opt { action: act::NUMBER + n as u16, choice: Choice::Number(n) }).collect();
        self.ask(Task::ChooseTimes { id }, p, DecisionKind::ChooseMode, opts);
    }

    /// At least `n` legal candidates for `tr` (the same order and rules as
    /// [`target_candidates`](Self::target_candidates), without building it).
    fn has_candidates(&self, tr: &TargetReq, ctx: &Ctx, n: usize) -> bool {
        let ok = |t: Target| self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx);
        let players = [ctx.controller, crate::ids::opp(ctx.controller)].into_iter().map(Target::Player);
        let perms = self.battlefield.iter().map(|&c| Target::Obj(self.obj_ref(c)));
        // Spells and abilities on the stack, top first (an ability only for
        // a filter that names abilities: a cheap test first).
        let abilities = wants_ability(&tr.filter);
        let spells = self.stack.iter().rev().filter_map(move |s| match s.kind {
            StackKind::Spell { card } => Some(self.obj_ref(card)).filter(|&r| Some(r) != ctx.source).map(Target::Obj),
            // (Not the ability whose modes are being chosen, CR 115.5.)
            StackKind::Ability { .. } => (abilities && s.id != self.choosing_for).then_some(Target::Stack(s.id)),
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
        if a.bestow && !self.def_of(card).bestowed {
            // CR 702.103b, d: cast bestowed, it is an Aura spell, and only
            // those characteristics are evaluated: it needs a creature to
            // target, and cost increases for noncreature spells apply (on a
            // copy of the game with the face up: only cards with bestow).
            // Its timing is the card's (the face keeps its keywords).
            let Some(face) = self.def_of(card).bestow else { return false };
            let mut g = self.clone();
            g.objs[card.i()].def = face;
            if !g.db.def(face).spell.as_ref().is_some_and(|ab| g.targets_available(ab, p, Some(g.obj_ref(card)))) {
                return false;
            }
            return g.alt_payable(p, card, k, classes);
        }
        if a.only_if.as_ref().is_some_and(|c| !self.cond_holds(c, p)) {
            return false;
        }
        let mana = self.alt_mana(card, p, k);
        // (A card cast from hand is on the stack while its cost is paid.)
        self.cost_payable(p, card, &a.cost, &mana, classes, Some(card))
    }

    /// The mana of alternative cost `k` with the cost increases and
    /// reductions (CR 601.2f, 118.9d: both apply to an alternative cost).
    pub(crate) fn alt_mana(&self, card: CardIdx, p: PlayerId, k: usize) -> crate::mana::ManaCost {
        self.cost_with(card, p, self.def_of(card).alt_costs[k].mana)
    }

    /// Can `p` cast `card` for its mana cost (with its additional cost, if
    /// any)?
    pub(crate) fn normal_payable(&self, p: PlayerId, card: CardIdx, classes: &[crate::pay::Class]) -> bool {
        let def = self.def_of(card);
        let cost = self.spell_cost(card, p);
        let cost = self.delve_reduced(card, p, cost, 0);
        if !def.additional.is_empty() {
            self.options_payable(p, card, crate::game::CastFrom::Hand, &Default::default(), 0, classes, Some(card))
        } else {
            self.can_pay_without(p, &cost, classes, &Default::default(), Some(card))
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
                if k > 0 && self.def_of(card).alt_costs[k as usize - 1].bestow {
                    // CR 702.103b: cast bestowed, it becomes an Aura spell
                    // with enchant creature, which targets (601.2c).
                    if let Some(face) = self.def_of(card).bestow {
                        self.objs[card.i()].def = face;
                        self.agenda.push(Task::ChooseTargets { id, req: 0 });
                    }
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
            times: 0,
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
        if self.def_of(card).times_cost.is_some() {
            // CR 601.2b: replicate, squad, offspring (before X).
            self.agenda.push(Task::ChooseTimes { id });
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
                let keep = self.escape_exiles(card, item.from);
                let src = Some(self.obj_ref(card));
                let kicker_ok = |k: usize| {
                    !def.kicker
                        || def.spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| self.targets_available(&m.list[k], p, src))
                };
                // CR 601.2f: the option's mana is added before reductions.
                let mut base = item.clone();
                let opts: Vec<Opt> = (0..def.additional.len())
                    .filter(|&k| {
                        base.cost_option = k as u8;
                        let cost = self.delve_reduced(card, p, self.total_mana(card, &base), keep);
                        kicker_ok(k) && self.cost_payable(p, card, &def.additional[k], &cost, &classes, None)
                    })
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
        // "Another target" (CR 115.3): not one chosen for a requirement it
        // must differ from; never the ability itself (115.5).
        let mut exclude = already.clone();
        exclude.extend(item.targets.iter().filter(|(r, _)| tr.distinct & (1 << r) != 0).map(|&(_, t)| t));
        exclude.push(Target::Stack(id));
        match self.answer.take() {
            Some(Choice::Pass) => {
                self.agenda.push(Task::ChooseTargets { id, req: req + 1 });
                return;
            }
            Some(Choice::Target(t)) => {
                self.note(|g| format!("target {}", g.describe_target(t)));
                let fresh = !self.stack[pos].targets.iter().any(|&(_, x)| x == t);
                self.stack[pos].targets.push((req, t));
                if fresh {
                    self.became_target(id, t);
                }
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
        let mut cands = self.target_candidates(tr, &ctx, &exclude);
        if reqs.iter().any(|r| r.distinct & (1 << req) != 0) {
            // Keep a later requirement that must differ from this one
            // choosable (a cast is offered only if it can be completed).
            let chosen = item.targets.clone();
            cands.retain(|&t| self.later_distinct_ok(&reqs, req, t, &chosen, &ctx));
        }
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
        // CR 601.2f: the total cost, then delve's exiled cards pay for some
        // of its generic mana (702.66b).
        let cost = self.total_mana(card, item).less_generic(item.delved);
        if item.alt > 0 {
            let k = item.alt as usize - 1;
            let mut pm = PayMana::new(p, cost);
            pm.improvise = def.improvise;
            pm.keep = Self::keep_for(card, &def.alt_costs[k].cost, None);
            self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
            if !pm.cost.is_zero() {
                self.agenda.push(Task::PayMana(pm));
            }
            return;
        }
        let mut pm = PayMana::new(p, cost);
        pm.improvise = def.improvise;
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
        // CR 722.3c: casting the copy of its prepare spell unprepares the
        // permanent.
        if !self.prepared.is_empty() {
            self.prepared.retain(|x| x.1 != card);
        }
        // The event's amount: the spells cast before it this turn, by any
        // player (storm, CR 702.40a).
        let before = self.players.iter().map(|pl| pl.spells_cast as u16).sum::<u16>() - 1;
        self.event(crate::triggers::Event {
            kind: crate::db::EventKind::IsCast,
            a: crate::triggers::Subj::Obj(r),
            b: crate::triggers::Subj::Player(p),
            amount: before,
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
        let abilities = wants_ability(&tr.filter);
        for s in self.stack.iter().rev() {
            match s.kind {
                StackKind::Spell { card } => {
                    let r = self.obj_ref(card);
                    if Some(r) != ctx.source {
                        consider(Target::Obj(r), &mut out);
                    }
                }
                // (Not the ability itself, CR 115.5: `choose_targets`.)
                StackKind::Ability { .. } if abilities => consider(Target::Stack(s.id), &mut out),
                StackKind::Ability { .. } => {}
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
            Target::Player(_) | Target::Stack(_) => true,
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
        match t {
            Target::Obj(r) if !self.live(r) => return false,
            // An ability that left the stack (resolved or countered) is
            // gone.
            Target::Stack(id) if self.item_index(id).is_none() => return false,
            _ => {}
        }
        self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx)
    }

    /// The stack item of an ability target (`Target::Stack`), if still on
    /// the stack.
    pub(crate) fn stack_item(&self, id: u32) -> Option<&StackItem> {
        self.stack.iter().find(|s| s.id == id)
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
            Target::Stack(id) => match self.stack_item(id) {
                Some(s) => ability_ok(f, s),
                None => false,
            },
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

    /// A card in a graveyard matches only through `InGraveyard(f)` (a token
    /// there is not a card, CR 108.2).
    fn graveyard_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::InGraveyard(inner) => !self.is_token(c) && self.card_matches(inner, c, ctx),
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
            Filter::Token => self.is_token(c),
            Filter::Attacking => self.objs[c.i()].attacking,
            Filter::Tapped => self.objs[c.i()].tapped,
            Filter::AttachedTo(inner) => self.objs[c.i()].attached.is_some_and(|h| {
                self.live(h) && self.objs[h.idx.i()].zone == Zone::Battlefield && self.card_matches(inner, h.idx, ctx)
            }),
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
                // (Back faces have no mana cost, CR 712.8e; nor has an
                // eternalized copy.)
                !def.is(types::LAND)
                    && def.x_pips == 0
                    && def.front_mv.is_none()
                    && !self.exc(c).is_some_and(|e| e.no_cost)
                    && v.contains(&def.cost)
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
                        Target::Player(_) | Target::Stack(_) => false,
                    })
                }),
            // (Abilities on the stack match only at top level, `filter_ok`.)
            Filter::Ability(_) => false,
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

/// Can a filter match an ability on the stack (`Ability`)?
fn wants_ability(f: &Filter) -> bool {
    match f {
        Filter::Ability(_) => true,
        Filter::And(a, b) => wants_ability(a) || wants_ability(b),
        Filter::Or(v) => v.iter().any(wants_ability),
        _ => false,
    }
}

/// Does an ability on the stack match the filter ("target activated or
/// triggered ability")? An activated ability is one of a permanent or of a
/// card in hand (CR 602); a triggered ability any other (603; delayed,
/// reflexive and sourceless ones included).
fn ability_ok(f: &Filter, s: &StackItem) -> bool {
    let StackKind::Ability { ability, .. } = s.kind else { return false };
    let kind = match ability.slot {
        AbilitySlot::Activated(_) | AbilitySlot::Hand(_) | AbilitySlot::Graveyard(_) => crate::db::ABILITY_ACTIVATED,
        AbilitySlot::Trigger(_) | AbilitySlot::Spell => crate::db::ABILITY_TRIGGERED,
    };
    match f {
        Filter::Ability(k) => k & kind != 0,
        Filter::And(a, b) => ability_ok(a, s) && ability_ok(b, s),
        Filter::Or(v) => v.iter().any(|x| ability_ok(x, s)),
        _ => false,
    }
}
