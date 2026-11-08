//! Casting spells, choosing targets, filters (CR 601, 115; ENGINE.md 6).

use crate::db::{types, AbilityDef, AbilityRef, AbilitySlot, Filter, Kw, TargetReq};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos, StackItem, StackKind};
use crate::ids::{CardIdx, ObjRef, PlayerId, Target, Zone};
use crate::pay::PayMana;
use crate::svec::SVec;

/// How many stack items below the item whose targets are chosen a target may
/// be among (`Game::stack_in_reach`; ENGINE.md 58.1): the `STACK` slots but
/// the top one, which that item takes.
pub(crate) const STACK_REACH: usize = act::STACK_N as usize - 1;

/// Who is asking whether a filter matches: the spell or ability's
/// controller ("you") and source ("this", "another").
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ctx {
    pub controller: PlayerId,
    pub source: Option<ObjRef>,
}

/// `ChooseRoute`'s codes (`NUMBER`): the ways a card in its owner's
/// graveyard may be cast when several permissions allow it (CR 601.2a-b).
pub(crate) mod route {
    /// Its flashback (printed or granted, CR 702.34).
    pub const FLASHBACK: u8 = 0;
    /// "You may play lands and cast spells from your graveyard" (Yawgmoth's
    /// Will): for its mana cost.
    pub const WILL: u8 = 1;
    /// Escape (printed, or granted by Underworld Breach, CR 702.138).
    pub const ESCAPE: u8 = 2;
    /// Granted retrace (CR 702.81).
    pub const RETRACE: u8 = 3;
    /// Mayhem (CR 702.187).
    pub const MAYHEM: u8 = 4;
    /// A permanent's once-each-turn permission whose permanent gains a
    /// trigger (Serra Paragon).
    pub const PARAGON: u8 = 5;
    /// One that grants nothing (Lurrus of the Dream-Den).
    pub const LURRUS: u8 = 6;
    /// An effect's permission for this card (its only route).
    pub const EFFECT: u8 = 7;
    /// A granted flashback (its mana cost) besides the card's printed one
    /// (CR 702.34a: either flashback cost may be paid).
    pub const FLASHBACK_MANA: u8 = 8;
    /// Its own "you may cast this card from your graveyard using its warp
    /// ability" (ENGINE.md 99.4): for its warp cost only.
    pub const WARP: u8 = 9;
}

/// A way to cast a graveyard card (`route::*`), with the permanent whose
/// permission it uses, if one.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Route {
    pub code: u8,
    pub granter: Option<CardIdx>,
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
        // "Spells your opponents cast with the chosen name cost {2} more to
        // cast" (Anointed Peacekeeper).
        if self.seen & crate::game::SEEN_NAMED_TAX != 0 {
            cost.generic = cost.generic.saturating_add(self.named_tax(card, Some(p)));
        }
        // Then reductions (CR 601.2f: increases, then reductions; generic
        // mana only, not below zero).
        for d in &self.def_of(card).discounts {
            let n = match d {
                crate::db::Discount::IfTargets(f, n) => self.target_discount(card, p, f, *n),
                d => self.discount(d, p),
            };
            cost.generic = cost.generic.saturating_sub(n);
        }
        // "The second spell you cast each turn costs {2} less", "spells you
        // cast have affinity for artifacts" (ENGINE.md 99.4).
        for c in self.permanents_of(p) {
            for d in &self.abilities(c).casting.spell_discounts {
                cost.generic = cost.generic.saturating_sub(self.discount(d, p));
            }
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
        // "Spells you cast from anywhere other than your hand cost {1} less"
        // (Bilbo): castability from another zone, or a spell cast from one.
        let outside = || match self.objs[card.i()].zone {
            Zone::Hand => false,
            Zone::Stack => self.stack.iter().any(|s| s.kind == StackKind::Spell { card } && s.origin != Zone::Hand),
            _ => true,
        };
        if self.seen & crate::game::SEEN_OUTSIDE_DISCOUNT != 0 && cost.generic > 0 && outside() {
            for c in self.permanents_of(p) {
                cost.generic = cost.generic.saturating_sub(self.abilities(c).outside_hand_discount);
            }
        }
        cost
    }

    /// The colour mask of the basic land type chosen as `c` entered
    /// (Multiversal Passage).
    pub(crate) fn chosen_land_color(&self, c: CardIdx) -> crate::mana::ManaMask {
        let r = self.obj_ref(c);
        self.marks
            .iter()
            .find_map(|&(o, m)| match m {
                crate::game::Mark::ChosenColor(k) if o == r => Some(1 << k),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// How much a cost reduction takes off for `p` now (generic mana).
    pub(crate) fn discount(&self, d: &crate::db::Discount, p: PlayerId) -> u8 {
        let ctx = Ctx { controller: p, source: None };
        match d {
            crate::db::Discount::PerBasicLandType(n) => self.basic_land_types(p).saturating_mul(*n),
            crate::db::Discount::PerControlled(f, n) => {
                (self.permanents_of(p).filter(|&c| self.card_matches(f, c, &ctx)).count() as u8).saturating_mul(*n)
            }
            crate::db::Discount::PerDifferentlyNamedLand(n) => self.differently_named_lands(p).saturating_mul(*n),
            crate::db::Discount::If(c, n) => {
                if self.cond_holds(c, p) {
                    *n
                } else {
                    0
                }
            }
            // (A spell's own, read with its targets: `cost_with`.)
            crate::db::Discount::IfTargets(..) => 0,
            // (An activated ability's, read with its source: `act_mana_x`.)
            crate::db::Discount::SourcePower => 0,
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

    /// The spending restrictions casting `card` satisfies (`SPEND_*`: an
    /// artifact spell, a legendary spell; CR 106.6).
    pub(crate) fn spend_allow(&self, card: CardIdx) -> u8 {
        let mut a = 0;
        if self.is_type(card, types::ARTIFACT) {
            a |= crate::db::SPEND_ARTIFACT;
        }
        if self.supertypes_of(card) & crate::db::supertypes::LEGENDARY != 0 {
            a |= crate::db::SPEND_LEGENDARY;
        }
        a
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
        if self.seen & crate::game::SEEN_TOP != 0 {
            self.top_options(p, &classes, sorcery_ok, land_ok, &mut opts);
        }
        self.activation_options(p, &classes, &slots, sorcery_ok, &mut opts);
        // The companion special action (CR 116.2g; ENGINE.md 35).
        if let Some(o) = self.companion_option(p, sorcery_ok, &classes) {
            opts.push(o);
        }
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
            with_artifacts = [classes, &self.improvise_classes(p, None, types::ARTIFACT)[..]].concat();
            &with_artifacts[..]
        } else if def.casting.convoke {
            // Convoke (CR 702.51a): the untapped creatures help pay.
            with_artifacts = [classes, &self.convoke_classes(p, None)[..]].concat();
            &with_artifacts[..]
        } else {
            classes
        };
        // Delve: its mana sources that sacrifice themselves too (ENGINE.md
        // 18).
        let delved = self.delve_classes(p, c, cls);
        let cls = &delved[..];
        let alt_ok = (0..def.alt_costs.len()).any(|k| self.alt_payable(p, c, k, cls, crate::game::CastFrom::Hand));
        // One another permanent gives it in hand (ENGINE.md 99.15).
        let granted_ok = self.granted_alt_castable(p, c, cls);
        // The first spell of a turn without paying its mana cost (ENGINE.md
        // 99.4).
        let free = self.first_spell_free(p, crate::game::CastFrom::Hand) && self.route_payable(p, c, crate::eoe_c::FREE_CAST, cls);
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
            // Mana that can only be spent on some spells (Mishra's
            // Workshop, Delighted Halfling): with what this spell allows.
            let allow = self.spend_allow(c);
            if allow != 0 && (cls.iter().any(|cl| cl.only & allow != 0) || self.players[p as usize].pool.art > 0) {
                let keep = crate::pay::Keep { allow, ..Default::default() };
                return self.can_pay_without(p, &cost, cls, &keep, Some(c));
            }
            if self.hand_mana_source(c) || def.improvise || def.casting.convoke || def.delve {
                // A card cast from hand is on the stack while its cost is
                // paid: its own mana ability (a Spirit Guide's) cannot help.
                self.can_pay_without(p, &cost, cls, &Default::default(), Some(c))
            } else {
                // (The cheap test first: the candidates are built only for
                // a cost that might be paid.)
                self.might_pay(p, &cost, cls, None) && {
                    let plain = plain.get_or_insert_with(|| self.plain_cands(p, cls));
                    self.can_pay_plain(p, &cost, cls, plain)
                }
            }
        };
        if alt_ok || granted_ok || free {
            // castable for an alternative cost (601.2b); the mana cost
            // may be payable too (asked on casting)
        } else if let Some(extras) = &extras {
            if !extras.iter().any(|x| payable(x)) {
                return false;
            }
        } else if !payable(&Default::default()) && !self.target_discount_castable(p, c, crate::game::CastFrom::Hand, cls) {
            return false;
        }
        if let Some(ab) = &def.spell {
            // Its targets, chosen with it on the stack (`as_cast`); not
            // overloaded, its own target is needed.
            let ok = |g: &Game, r: ObjRef| {
                g.targets_available(ab, p, Some(r))
                    && (!def.overload || alt_ok || ab.modes.as_ref().is_some_and(|m| g.targets_available(&m.list[0], p, Some(r))))
            };
            if !(if ab.has_targets() { self.as_cast(c, p, ok) } else { ok(self, self.obj_ref(c)) }) {
                return false;
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
        // "Each player can't cast more than one noncreature spell each
        // turn" (every cast's check passes here; ENGINE.md 99.4).
        if self.noncreature_spell_locked(p, card) {
            return false;
        }
        // An additional cost that chooses (ENGINE.md 99.14): something to
        // choose (CR 601.2h: a cost that can't be paid).
        if self.def_of(card).casting.choose_creature_or_warped && !self.chosen_for_cost_candidates(p) {
            return false;
        }
        if sorcery_ok {
            return true;
        }
        self.instant_speed(p, self.def_of(card)) && !self.permanents_of(crate::ids::opp(p)).any(|c| self.abilities(c).opp_sorcery_speed)
    }

    /// Could `p` cast a spell with definition `def` any time they could cast
    /// an instant (an instant, flash, a sorcery with Teferi's +1)?
    pub(crate) fn instant_speed(&self, p: PlayerId, def: &crate::db::CardDef) -> bool {
        def.is(types::INSTANT)
            || def.keywords.has(Kw::Flash)
            || def.flash_permission
            || (def.is(types::SORCERY) && self.players[p as usize].flash_sorceries)
    }

    /// Does permission `pm` let `p` play its card now?
    pub(crate) fn permission_ok(&self, pm: &crate::game::Permission, p: PlayerId) -> bool {
        if !self.live(pm.card) || !self.perm_turn_ok(pm.turn, pm.player) {
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
            // "If you control a <subtype>" (EoE W3).
            crate::game::PermKind::Play { only_if: crate::game::PlayIf::ControlSubtype(s), .. } => {
                self.permanents_of(p).any(|c| self.has_subtype(c, s))
            }
            crate::game::PermKind::Play { only_if: crate::game::PlayIf::AttackedWith(s), .. } => {
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
            .find(|pm| {
                pm.card == r
                    && self.permission_ok(pm, p)
                    && matches!(
                        pm.kind,
                        crate::game::PermKind::Play { .. } | crate::game::PermKind::Taxed(_) | crate::game::PermKind::Airbent(_) | crate::game::PermKind::TaxedLandsTapped(_)
                    )
            })
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
        self.cast_route_esc(p, card, &mut None, &mut None)
    }

    /// [`cast_route`](Self::cast_route) remembering in `esc` whether `p`
    /// controls a permanent that grants escape (one walk of `p`'s
    /// permanents for a whole graveyard).
    /// (`para` likewise remembers the mana value Serra Paragons allow.)
    fn cast_route_esc(&self, p: PlayerId, card: CardIdx, esc: &mut Option<bool>, para: &mut Option<u8>) -> Option<crate::game::CastFrom> {
        use crate::game::CastFrom;
        let def = self.def_of(card);
        match self.objs[card.i()].zone {
            // Bolas's Citadel: the top card of the caster's library.
            Zone::Library if self.top_play_ok(p, card) => Some(CastFrom::LibraryTop),
            Zone::Hand if self.miracle_cast == Some(card) => Some(CastFrom::Miracle),
            Zone::Hand => Some(CastFrom::Hand),
            // (The first of its routes; the player chooses among those that
            // can be completed as it is cast, `begin_cast_face`.)
            Zone::Graveyard => {
                let _ = def;
                self.gy_routes_esc(p, card, None, esc, para).first().map(|r| self.route_from(card, p, *r))
            }
            Zone::Exile | Zone::FaceDown => match self.exile_permission(p, card)? {
                crate::game::PermKind::Play { free, any_mana, .. } => Some(CastFrom::Permission { free, any_mana }),
                // CR 715.3d: cast it (not as an Adventure) for its mana cost.
                crate::game::PermKind::Adventure => Some(CastFrom::Permission { free: false, any_mana: false }),
                crate::game::PermKind::Taxed(n) | crate::game::PermKind::TaxedLandsTapped(n) => Some(CastFrom::Taxed(n)),
                // CR 722.3c: a copy of a prepare spell, for its mana cost.
                crate::game::PermKind::Prepared { .. } => Some(CastFrom::Permission { free: false, any_mana: false }),
                crate::game::PermKind::Airbent(n) => Some(CastFrom::Airbent(n)),
                crate::game::PermKind::Flashback | crate::game::PermKind::CastThenExile => None,
            },
            _ => None,
        }
    }

    /// Every way `p` may cast `card` from their graveyard with face `face`
    /// up (`None`: as it is there), in `route::*` order: flashback (printed
    /// or granted), Yawgmoth's Will's permission, escape (printed or
    /// granted), granted retrace, mayhem, and each kind of once-each-turn
    /// permission of a permanent for a nonland permanent spell whose mana
    /// value it allows (Serra Paragon, whose permanent gains a trigger;
    /// Lurrus). CR 601.2a-b: the player chooses which (`Task::ChooseRoute`).
    /// An effect's permission for this card ("you may cast it" now, free or
    /// for its mana cost: Bilbo, Malcolm) is then its only route: the effect
    /// offered it that way (`can_cast_now`; a cheaper or free cast is never
    /// worse). `esc` and `para` remember `p`'s granters for a whole
    /// graveyard.
    pub(crate) fn gy_routes_esc(
        &self,
        p: PlayerId,
        card: CardIdx,
        face: Option<crate::ids::DefId>,
        esc: &mut Option<bool>,
        para: &mut Option<u8>,
    ) -> SVec<Route, 8> {
        let mut out: SVec<Route, 8> = SVec::new();
        if self.objs[card.i()].owner != p || self.objs[card.i()].zone != Zone::Graveyard {
            return out;
        }
        let def = self.def_of(card);
        let r = self.obj_ref(card);
        let mut granted = false;
        let mut effect = false;
        for pm in &self.permissions {
            if pm.card != r || !self.permission_ok(pm, p) {
                continue;
            }
            match pm.kind {
                crate::game::PermKind::Flashback => granted = true,
                crate::game::PermKind::Play { .. } | crate::game::PermKind::CastThenExile => effect = true,
                _ => {}
            }
        }
        if effect {
            out.push(Route { code: route::EFFECT, granter: None });
            return out;
        }
        if def.flashback.is_some() || granted {
            out.push(Route { code: route::FLASHBACK, granter: None });
        }
        if def.flashback.is_some() && granted && def.flashback != Some(def.cost) {
            out.push(Route { code: route::FLASHBACK_MANA, granter: None });
        }
        if self.gy_play_now(p) {
            out.push(Route { code: route::WILL, granter: None });
        }
        // (Granted escape with the card's own additional costs: both paid
        // as one cost, `CardDb::merged_cost`.)
        if def.escape.is_some()
            || (!def.is(types::LAND) && *esc.get_or_insert_with(|| self.escape_granter(p).is_some()) && self.granted_options_ok(card, None))
        {
            out.push(Route { code: route::ESCAPE, granter: None });
        }
        if self.retrace_granted(p, card) && self.granted_options_ok(card, Some(crate::db::RETRACE_ROUTE)) {
            out.push(Route { code: route::RETRACE, granter: None });
        }
        if self.mayhem_ok(p, card) {
            out.push(Route { code: route::MAYHEM, granter: None });
        }
        // (With Yawgmoth's Will's permission, that route allows the warp
        // cost too: `alt_allowed`.)
        if def.casting.warp_from_graveyard && !out.iter().any(|x| x.code == route::WILL) {
            out.push(Route { code: route::WARP, granter: None });
        }
        // The face that would be cast (a card with another face too: its
        // front, or `face`).
        let fd = face.map_or(def, |f| self.db.def(f));
        if !fd.is(types::LAND) && fd.is_permanent_card() && { let m = *para.get_or_insert_with(|| self.paragon_mv(p)); m > 0 && fd.mana_value() <= m } {
            for g in self.paragons(p, fd.mana_value(), false) {
                let code = if self.abilities(g).gy_play_k != crate::db::NO_GRANT { route::PARAGON } else { route::LURRUS };
                if !out.iter().any(|x| x.code == code) {
                    out.push(Route { code, granter: Some(g) });
                }
            }
        }
        out
    }

    /// [`gy_routes_esc`](Self::gy_routes_esc) without remembering.
    pub(crate) fn gy_routes(&self, p: PlayerId, card: CardIdx, face: Option<crate::ids::DefId>) -> SVec<Route, 8> {
        self.gy_routes_esc(p, card, face, &mut None, &mut None)
    }

    /// The cast a route means.
    pub(crate) fn route_from(&self, card: CardIdx, p: PlayerId, r: Route) -> crate::game::CastFrom {
        use crate::game::CastFrom;
        match r.code {
            route::FLASHBACK => CastFrom::Flashback,
            route::FLASHBACK_MANA => CastFrom::FlashbackMana,
            route::WILL => CastFrom::Permission { free: false, any_mana: false },
            route::ESCAPE => CastFrom::Escape,
            route::RETRACE => CastFrom::Retrace,
            route::MAYHEM => CastFrom::Mayhem,
            route::PARAGON | route::LURRUS => CastFrom::Paragon,
            _ => {
                let o = self.obj_ref(card);
                self.permissions
                    .iter()
                    .find_map(|pm| match pm.kind {
                        crate::game::PermKind::Play { free, any_mana, .. } if pm.card == o && self.permission_ok(pm, p) => {
                            Some(CastFrom::Permission { free, any_mana })
                        }
                        _ => None,
                    })
                    .unwrap_or(CastFrom::Permission { free: false, any_mana: false })
            }
        }
    }

    /// The routes of graveyard card `card` (face `face`) whose cast `p` can
    /// complete now (CR 601.2a-b).
    pub(crate) fn castable_gy_routes(&self, p: PlayerId, card: CardIdx, face: Option<crate::ids::DefId>) -> SVec<Route, 8> {
        let routes = self.gy_routes(p, card, face);
        if routes.len() <= 1 {
            return routes;
        }
        let classes = self.mana_classes(p, None);
        let sorcery_ok = self.turn.active == p && self.turn.step.is_main() && self.stack.is_empty();
        let mut g = None;
        let view: &Game = match face {
            Some(f) if f != self.objs[card.i()].def => {
                let mut c = self.clone();
                c.objs[card.i()].def = f;
                g = Some(c);
                g.as_ref().unwrap()
            }
            _ => self,
        };
        let out = routes.iter().copied().filter(|r| view.castable_via(p, card, self.route_from(card, p, *r), &classes, sorcery_ok)).collect();
        drop(g);
        out
    }

    /// CR 601.2a-b: which permission `p` casts graveyard card `card` (face
    /// `face`) with (`NUMBER` = `route::*` code), when several can.
    pub(crate) fn choose_route(&mut self, p: PlayerId, card: CardIdx, face: Option<crate::ids::DefId>) {
        match self.answer.take() {
            Some(Choice::Number(code)) => {
                let r = self.gy_routes(p, card, face).iter().copied().find(|r| r.code == code).expect("a route offered");
                self.route_choice = Some(r);
                self.begin_cast_face(p, card, face);
            }
            Some(other) => unreachable!("route answer {other:?}"),
            None => {
                let opts: Vec<Opt> = self
                    .castable_gy_routes(p, card, face)
                    .iter()
                    .map(|r| Opt { action: act::NUMBER + r.code as u16, choice: Choice::Number(r.code) })
                    .collect();
                self.ask(Task::ChooseRoute { p, card, face }, p, DecisionKind::ChooseRoute, opts);
            }
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
            CastFrom::FlashbackMana => with(def.cost),
            CastFrom::Escape => with(def.escape.as_ref().map_or(def.cost, |e| e.0)),
            // "Without paying its mana cost" (an alternative cost of no
            // mana, CR 118.9; increases still apply, 601.2f).
            CastFrom::Permission { free: true, .. } => with(Default::default()),
            CastFrom::Permission { free: false, any_mana: true } => any_colour(with(def.cost)),
            CastFrom::Permission { .. } => with(def.cost),
            CastFrom::Retrace => with(def.cost),
            // "The mayhem cost is equal to its mana cost" (an alternative
            // cost, CR 702.187b; increases apply, 601.2f).
            CastFrom::Mayhem => with(def.cost),
            // "A spell cast this way costs {n} more" (CR 601.2f).
            CastFrom::Taxed(n) => with(def.cost.plus(&crate::mana::ManaCost { generic: n, ..Default::default() })),
            CastFrom::Paragon => with(def.cost),
            CastFrom::Miracle => with(def.miracle.as_ref().map_or(def.cost, |m| m.0)),
            CastFrom::Airbent(n) => with(crate::mana::ManaCost { generic: n, ..Default::default() }),
            // Life rather than its mana cost (increases still apply, 601.2f).
            CastFrom::LibraryTop => with(Default::default()),
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
            // A permanent's: during its controller's turn (Six).
            || self.seen & crate::game::SEEN_RETRACE != 0
                && self.turn.active == p
                && self.permanents_of(p).any(|c| self.abilities(c).grants_retrace.as_ref().is_some_and(|f| self.card_matches(f, card, &ctx)))
    }

    /// The non-mana part of the cost for a cast `from` (escape's exiled
    /// cards; the additional cost chosen; a granted escape or retrace and
    /// the card's own additional cost option as one cost, CR 601.2b).
    pub(crate) fn route_extra(&self, card: CardIdx, from: crate::game::CastFrom, option: u8) -> Option<&'static crate::db::Cost> {
        let def = self.def_of(card);
        let db: &'static crate::db::CardDb = self.db;
        match from {
            crate::game::CastFrom::Retrace if def.additional.is_empty() => Some(retrace_cost()),
            crate::game::CastFrom::Retrace => db.merged_cost(self.objs[card.i()].def, crate::db::RETRACE_ROUTE, option),
            crate::game::CastFrom::Escape => match &def.escape {
                Some(e) => Some(&e.1),
                None => {
                    let p = self.objs[card.i()].owner;
                    let g = self.escape_granter(p)?;
                    if def.additional.is_empty() {
                        self.abilities(g).grants_escape.as_ref()
                    } else {
                        db.merged_cost(self.objs[card.i()].def, self.objs[g.i()].def, option)
                    }
                }
            },
            _ => def.additional.get(option as usize),
        }
    }

    /// A granted route (escape from `escape_granter`, or retrace with
    /// `Some(RETRACE_ROUTE)`) for `card` with its own additional costs: can
    /// some option be paid together with it (`CardDb::merged_cost`)?
    fn granted_options_ok(&self, card: CardIdx, retrace: Option<crate::ids::DefId>) -> bool {
        let def = self.def_of(card);
        if def.additional.is_empty() {
            return true;
        }
        let route = match retrace {
            Some(r) => r,
            None => match self.escape_granter(self.objs[card.i()].owner) {
                Some(g) => self.objs[g.i()].def,
                None => return false,
            },
        };
        let me = self.objs[card.i()].def;
        (0..def.additional.len()).any(|k| self.db.merged_cost(me, route, k as u8).is_some())
    }

    /// May a spell cast the way `from` says be cast for one of its own
    /// alternative costs (CR 118.9, 118.9a: one alternative cost at most)?
    /// From hand, and with a permission to cast the card for its costs (an
    /// effect's "you may cast" or "play that card", Elite Spellbinder's,
    /// Lurrus's or Serra Paragon's permission, retrace, which adds a cost):
    /// yes (rulings eb35fd18, 9950a418). Flashback, escape, mayhem,
    /// miracle, airbending, Bolas's Citadel's life and "without paying its
    /// mana cost" are alternative costs themselves: no.
    pub(crate) fn alt_allowed(from: crate::game::CastFrom) -> bool {
        use crate::game::CastFrom;
        matches!(
            from,
            CastFrom::Hand | CastFrom::Permission { free: false, .. } | CastFrom::Taxed(_) | CastFrom::Paragon | CastFrom::Retrace
        )
    }

    /// Can `p` cast `card` (not in hand) the way `from` says now?
    pub(crate) fn castable_via(&self, p: PlayerId, card: CardIdx, from: crate::game::CastFrom, classes: &[crate::pay::Class], sorcery_ok: bool) -> bool {
        let def = self.def_of(card);
        if !self.cast_timing_ok(p, card, sorcery_ok) {
            return false;
        }
        // For one of its alternative costs (CR 118.9; the mana cost may be
        // payable too: asked on casting, `cast_alt`).
        let alt_ok = !def.alt_costs.is_empty()
            && Self::alt_allowed(from)
            && (0..def.alt_costs.len()).any(|k| self.alt_payable(p, card, k, classes, from));
        // The first spell of a turn without paying its mana cost (ENGINE.md
        // 99.4).
        // (From a graveyard by its own warp ability only: its warp cost, no
        // other cost, CR 118.9a.)
        let warp_only = self.warp_only(p, card, from);
        let free = !warp_only && self.first_spell_free(p, from) && self.route_payable(p, card, crate::eoe_c::FREE_CAST, classes);
        let plain = !warp_only && (self.route_payable(p, card, from, classes) || self.target_discount_castable(p, card, from, classes));
        if !(alt_ok || free || plain) {
            return false;
        }
        // Overloaded (an alternative cost), no target is needed; otherwise
        // an overload spell's own target is.
        let spell = match &def.spell {
            Some(ab) if def.overload && !alt_ok => ab.modes.as_ref().map(|m| &m.list[0]),
            Some(_) if def.overload => return true,
            other => other.as_ref(),
        };
        match spell {
            None => true,
            Some(ab) => self.spell_targets_available(ab, p, card),
        }
    }

    /// Can `p` pay the cost of casting `card` (not in hand) the way `from`
    /// says (its mana cost or the route's cost, not an alternative cost of
    /// the card's own)?
    pub(crate) fn route_payable(&self, p: PlayerId, card: CardIdx, from: crate::game::CastFrom, classes: &[crate::pay::Class]) -> bool {
        let def = self.def_of(card);
        // Improvise (CR 702.126).
        let with_artifacts;
        let classes = if def.improvise {
            with_artifacts = [classes, &self.improvise_classes(p, None, types::ARTIFACT)[..]].concat();
            &with_artifacts[..]
        } else if def.casting.convoke {
            // Convoke (CR 702.51a).
            with_artifacts = [classes, &self.convoke_classes(p, None)[..]].concat();
            &with_artifacts[..]
        } else {
            classes
        };
        let delved = self.delve_classes(p, card, classes);
        let classes = &delved[..];
        let keep = self.escape_exiles(card, from);
        // Bolas's Citadel: life equal to its mana value (CR 119.4), kept from
        // the mana payment.
        let top_life = if from == crate::game::CastFrom::LibraryTop { def.mana_value() } else { 0 };
        if top_life as i32 > self.players[p as usize].life {
            return false;
        }
        // `extra`: the mana of additional costs announced (CR 601.2f).
        let paid = |extra: &crate::mana::ManaCost| {
            let mana = self.delve_reduced(card, p, self.route_mana_plus(card, p, from, extra), keep);
            match from {
                _ if !def.additional.is_empty() => self.options_payable(p, card, from, extra, keep, classes, None),
                crate::game::CastFrom::Escape | crate::game::CastFrom::Retrace => {
                    self.cost_payable(p, card, self.route_extra(card, from, 0).expect("an escape or retrace cost"), &mana, classes, None)
                }
                _ => self.can_pay_with(p, &mana, classes, &crate::pay::Keep { allow: self.spend_allow(card), life: top_life, ..Default::default() }),
            }
        };
        // Spree (CR 702.172a): at least one mode's additional cost is paid.
        match self.spree_extras(card, p) {
            Some(extras) => extras.iter().any(|x| paid(x)),
            None => paid(&Default::default()),
        }
    }

    /// CR 601.2b: announce X, from 0 to the most the caster can pay (31 at
    /// most; 0 when cast without paying its mana cost, 107.3b). Choice:
    /// `NUMBER`.
    pub(crate) fn choose_x(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        if let StackKind::Ability { ability, source } = self.stack[pos].kind {
            // An activated ability's {X} (CR 602.2b): as much as can be paid,
            // and no more than its "X targets" can have.
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
            let ad = self.db.def(ability.def).activated_def(ability.slot);
            let ctx = Ctx { controller: p, source: Some(source) };
            let mut most = (act::NUMBER_N - 1) as u8;
            if let crate::db::ActEffect::Stack(ab) = &ad.effect {
                // (On the stack: the items in reach counted from below it,
                // ENGINE.md 58.1.)
                self.targeting = id;
                for tr in ab.targets.iter().filter(|t| t.max == crate::db::X_TARGETS) {
                    most = most.min(self.target_candidates(tr, &ctx, &[Target::Stack(id)]).len().min(255) as u8);
                }
                self.targeting = 0;
            }
            // "Tap X untapped artifacts you control" (EoE W3): as many as
            // can be tapped with the mana still payable.
            if ad.cost.tap_x.is_some() {
                most = most.min(self.tap_x_most(p, source.idx, ad));
            }
            // (Not with the source's own mana when the cost taps it: an
            // artifact Urza could tap for mana, ENGINE.md 31.)
            let exclude = Self::mana_exclude(source.idx, ad);
            let classes = self.mana_classes(p, exclude);
            let keep = Self::keep_for(source.idx, &ad.cost, exclude);
            let mut x = 0u8;
            while x < most && self.can_pay_without(p, &self.act_mana_x(p, source.idx, ad, x + 1), &classes, &keep, exclude) {
                x += 1;
            }
            let opts = (0..=x).map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k) }).collect();
            self.ask(Task::ChooseX { id }, p, DecisionKind::ChooseMode, opts);
            return;
        }
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
        // (Cast for a granted alternative cost, which has no {X}: X is 0 too,
        // CR 107.3b; ENGINE.md 99.15.)
        let free = matches!(item.from, crate::game::CastFrom::Permission { free: true, .. } | crate::game::CastFrom::LibraryTop)
            || self.cast_for_granted_alt(card, item);
        if self.def_of(card).times_cost.as_ref().is_some_and(|t| t.x) {
            // Kicker {X} (CR 601.2b): announced only if kicked, at least
            // `x_min`; as much as can be paid.
            if item.times == 0 {
                return;
            }
            let lo = self.def_of(card).x_min;
            let mut tmp = item.clone();
            let mut most = lo;
            while most < (act::NUMBER_N - 1) as u8 {
                tmp.x = most + 1;
                if !self.can_pay_spell(card, &tmp, &self.total_mana(card, &tmp)) {
                    break;
                }
                most += 1;
            }
            let opts = (lo..=most).map(|x| Opt { action: act::NUMBER + x as u16, choice: Choice::Number(x) }).collect();
            self.ask(Task::ChooseX { id }, p, DecisionKind::ChooseMode, opts);
            return;
        }
        let mut most = 0u8;
        if self.def_of(card).x_life {
            // "Pay X life" (CR 119.4: no more than one's life total; X isn't
            // in the mana cost, so a free cast doesn't fix it, 107.3b).
            // The life the mana payment may need is kept (a painland).
            let life = self.players[p as usize].life.clamp(0, (act::NUMBER_N - 1) as i32) as u8;
            let cost = self.total_mana(card, item);
            let classes = self.mana_classes(p, None);
            let allow = self.spend_allow(card);
            while most < life && self.can_pay_with(p, &cost, &classes, &crate::pay::Keep { life: most + 1, allow, ..Default::default() }) {
                most += 1;
            }
        } else if !free {
            // CR 601.2f: X is part of the mana cost, before reductions. Cast
            // with a permission for a mana value n or less (Lurrus, Serra
            // Paragon), no more than keeps the spell's mana value n (202.3e:
            // X counts on the stack; ruling 90a8974d).
            let r = self.obj_ref(card);
            let limit = match self.marks.iter().find_map(|&(o, m)| match m {
                crate::game::Mark::MvCap(n) if o == r => Some(n),
                _ => None,
            }) {
                Some(n) => {
                    let d = self.def_of(card);
                    (n.saturating_sub(d.mana_value()) / d.x_pips.max(1)).min((act::NUMBER_N - 1) as u8)
                }
                None => (act::NUMBER_N - 1) as u8,
            };
            let mut tmp = item.clone();
            while most < limit {
                tmp.x = most + 1;
                if !self.can_pay_spell(card, &tmp, &self.total_mana(card, &tmp)) {
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
        let delved = self.delve_classes(p, card, classes);
        let classes = &delved[..];
        // Bolas's Citadel's life, paid after the mana: kept (ENGINE.md 38).
        let top_life = if from == crate::game::CastFrom::LibraryTop { def.mana_value() } else { 0 };
        // (A granted escape or retrace with the option: their one cost.)
        def.additional.iter().enumerate().filter_map(|(k, _)| self.route_extra(card, from, k as u8).map(|o| (k, o))).any(|(k, o)| {
            (!def.kicker
                || def
                    .spell
                    .as_ref()
                    .and_then(|ab| ab.modes.as_ref())
                    .is_some_and(|m| self.spell_targets_available(&m.list[k], p, card)))
                && {
                    let cost = self.delve_reduced(card, p, self.route_mana_plus(card, p, from, &extra.plus(&o.mana)), keep);
                    self.cost_payable(p, card, &Self::with_life(o, top_life), &cost, classes, exclude)
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

    /// `p`'s mana sources that sacrifice themselves and are cards (one mana,
    /// no mana cost: Lotus Petal), in canonical order: delve may exile them
    /// once their mana ability is activated (CR 601.2g-h). Only `p`'s own
    /// cards: another player's Lotus Petal that `p` controls goes to its
    /// owner's graveyard (CR 400.3; ENGINE.md 51).
    pub(crate) fn delve_sources(&self, p: PlayerId) -> Vec<CardIdx> {
        let classes = self.mana_classes(p, None);
        let mut out: Vec<CardIdx> = Vec::new();
        for cl in &classes {
            if cl.sac_self && cl.need == 0 && matches!(cl.yld, crate::pay::Yield::One(_)) && !crate::pay::is_sac_slot(cl.members[0].slot) {
                for s in cl.members.iter() {
                    // (Only a card that would reach the graveyard: not under
                    // Yawgmoth's Will or an opponent's Dauthi Voidwalker,
                    // CR 614.1a; ENGINE.md 38.)
                    if self.feeds_graveyard(p, s.card) && !out.contains(&s.card) {
                        out.push(s.card);
                    }
                }
            }
        }
        out.sort_by_key(|c| self.permanents_of(p).position(|x| x == *c));
        out
    }

    /// Is graveyard card `c` chosen for a delve, to be exiled as the cost is
    /// paid (`Mark::DelveExile`)?
    pub(crate) fn delve_exiling(&self, c: CardIdx) -> bool {
        !self.marks.is_empty() && self.marks.iter().any(|&(o, m)| o.idx == c && m == crate::game::Mark::DelveExile && self.live(o))
    }

    /// Exile the graveyard cards chosen for delve (in the order chosen) as
    /// the payment of their spell's cost ends (CR 601.2h, after the mana
    /// abilities of 601.2g; ENGINE.md 51).
    /// (`spell`: the spell being paid for, whose permanent may count the
    /// cards exiled with it, `record_exiled_to_pay`.)
    pub(crate) fn exile_delved(&mut self, p: PlayerId, spell: Option<CardIdx>) {
        if !self.marks.iter().any(|&(_, m)| m == crate::game::Mark::DelveExile) {
            return;
        }
        let picked: Vec<ObjRef> = self.marks.iter().filter(|&&(_, m)| m == crate::game::Mark::DelveExile).map(|&(o, _)| o).collect();
        self.marks.retain(|&(_, m)| m != crate::game::Mark::DelveExile);
        for r in picked {
            if self.live(r) && self.objs[r.idx.i()].zone == Zone::Graveyard {
                self.note(|g| format!("P{p} exiles {} (delve)", g.name(r.idx)));
                let e = self.move_card(r.idx, Zone::Exile, Pos::Top);
                self.record_exiled_to_pay(spell, e);
            }
        }
    }

    /// A card `e` (in exile) that delve exiled to pay the cost of `spell`:
    /// noted on the spell when its permanent counts the cards exiled with it
    /// (CR 607.2q: "exiled with it" are the cards exiled to pay the cost of
    /// the spell that became it; Murktide Regent, ENGINE.md 54). Nothing for
    /// any other spell.
    pub(crate) fn record_exiled_to_pay(&mut self, spell: Option<CardIdx>, e: ObjRef) {
        if let Some(s) = spell.filter(|&s| self.def_of(s).counters_per_exiled.is_some()) {
            if self.objs[e.idx.i()].zone == Zone::Exile && self.live(e) {
                let r = self.obj_ref(s);
                self.marks.push((r, crate::game::Mark::ExiledToPay(e)));
            }
        }
    }

    /// The cards a resolving permanent spell's permanent enters with a
    /// counter for (`CardDef::counters_per_exiled`): those its delve exiled
    /// that are still in exile (the same objects, CR 400.7) and match the
    /// filter by their own characteristics (CR 607.2q). 0 for a copy of the
    /// spell (it wasn't cast: no cost was paid, CR 707.10) or any other spell.
    pub(crate) fn exiled_to_pay_count(&self, spell: CardIdx) -> u8 {
        let Some(f) = &self.def_of(spell).counters_per_exiled else { return 0 };
        if self.marks.is_empty() || self.is_copy_obj(spell) {
            return 0;
        }
        let r = self.obj_ref(spell);
        let n = self
            .marks
            .iter()
            .filter(|&&(o, m)| match m {
                crate::game::Mark::ExiledToPay(e) => {
                    o == r
                        && self.live(e)
                        && self.objs[e.idx.i()].zone == Zone::Exile
                        && self.db.def_matches(f, self.objs[e.idx.i()].def) == Some(true)
                }
                _ => false,
            })
            .count();
        n.min(u8::MAX as usize) as u8
    }

    /// Was `c` chosen to be exiled for delve once activated?
    pub(crate) fn delve_late(&self, c: CardIdx) -> bool {
        let r = self.obj_ref(c);
        self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::DelveLate)
    }

    /// `classes` with delve's due (ENGINE.md 18): each mana source that
    /// sacrifices itself and is a card pays its mana and, exiled from the
    /// graveyard afterwards, one more generic mana of a delve spell.
    pub(crate) fn delve_classes<'a>(&self, p: PlayerId, card: CardIdx, classes: &'a [crate::pay::Class]) -> std::borrow::Cow<'a, [crate::pay::Class]> {
        use crate::pay::Yield;
        if !self.def_of(card).delve
            || !classes.iter().any(|cl| cl.sac_self && cl.need == 0 && matches!(cl.yld, Yield::One(_)) && !crate::pay::is_sac_slot(cl.members[0].slot))
        {
            return std::borrow::Cow::Borrowed(classes);
        }
        let mut out = classes.to_vec();
        let mut kept = Vec::new();
        for cl in out.iter_mut() {
            if !(cl.sac_self && cl.need == 0 && !crate::pay::is_sac_slot(cl.members[0].slot)) {
                continue;
            }
            let Yield::One(u) = cl.yld else { continue };
            // (Tokens cease to exist: no delve; nor a card exiled instead of
            // going to the graveyard, under Yawgmoth's Will or an opponent's
            // Dauthi Voidwalker: `delve_sources`, ENGINE.md 49; nor another
            // player's card, which goes to its owner's graveyard, CR 400.3,
            // ENGINE.md 51.)
            let mut cards = crate::pay::Members::new();
            for s in cl.members.iter() {
                if !self.feeds_graveyard(p, s.card) {
                    kept.push((cl.clone(), *s));
                } else {
                    cards.push(*s);
                }
            }
            if cards.is_empty() {
                continue;
            }
            cl.members = cards;
            let prods = (0..6).filter(|&t| u.mask & (1 << t) != 0).map(|t| std::array::from_fn(|i| (i == t) as u8)).collect();
            cl.yld = Yield::Multi { prods, units: Some(vec![u, crate::mana::Unit::free(0)]) };
        }
        for (cl, s) in kept {
            let mut m = crate::pay::Members::new();
            m.push(s);
            out.push(crate::pay::Class { members: m, ..cl });
        }
        std::borrow::Cow::Owned(out)
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
            // A mana source that sacrifices itself: activated while paying,
            // then exiled (CR 601.2g-h; `Mark::DelveLate`).
            Some(Choice::Card(c)) if self.objs[c.i()].zone == Zone::Battlefield => {
                let r = self.obj_ref(c);
                self.marks.push((r, crate::game::Mark::DelveLate));
                self.stack[pos].delved += 1;
                self.agenda.push(Task::Delve { id });
                return;
            }
            // A graveyard card: chosen now, exiled as the total cost is
            // paid (CR 601.2h, 702.66a), after the mana abilities (601.2g),
            // so a mana ability that reads the graveyard sees it whole:
            // Fanatic of Rhonas's {G}{G}{G}{G} with Dragon's Rage Channeler
            // grown by delirium (fuzz game 10013181: delve exiled a creature
            // card first, delirium was gone and Dig Through Time's
            // {3}{U}{U} had no payment; ENGINE.md 51). It stays where it is
            // until then (`Mark::DelveExile`; `exile_delved`).
            Some(Choice::Card(c)) => {
                let at = self.players[p as usize].graveyard.iter().position(|&x| x == c).unwrap_or(0);
                let r = self.obj_ref(c);
                self.marks.push((r, crate::game::Mark::DelveExile));
                let item = &mut self.stack[pos];
                item.delved += 1;
                item.delve_floor = (at + 1) as u8;
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
        let allow = self.spend_allow(card);
        let need = (0..=cost.generic).find(|&d| self.can_pay_for(p, &cost.less_generic(d), allow)).unwrap_or(cost.generic) as usize;
        let floor = item.delve_floor as usize;
        let keep = self.escape_exiles(card, item.from) as usize;
        let gy = &self.players[p as usize].graveyard;
        let late = self.delve_sources(p);
        let committed: Vec<CardIdx> = late.iter().copied().filter(|&c| self.delve_late(c)).collect();
        // Picks go up the graveyard (each set once), leaving enough cards
        // above for the picks still needed. (Cards only: a token there isn't
        // one, CR 108.2. The cards picked so far, below `floor`, are still
        // there: not counted.)
        let card_at = |i: usize| !self.is_token(gy[i]);
        let n_cards = (0..gy.len()).filter(|&i| card_at(i)).count().saturating_sub((item.delved as usize).saturating_sub(committed.len()));
        let mut opts: Vec<Opt> = gy
            .iter()
            .enumerate()
            .filter(|&(i, _)| card_at(i) && i >= floor && (i..gy.len()).filter(|&j| card_at(j)).count() >= need && n_cards > keep)
            .map(|(_, &c)| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) })
            .collect();
        // Mana sources that sacrifice themselves (Lotus Petal), first and
        // in canonical order: in the graveyard by the time delve's cards are
        // exiled (CR 601.2g before 601.2h), when the rest stays payable.
        if item.delved as usize == committed.len() {
            let gy_left = n_cards.saturating_sub(keep);
            let after = late.iter().rposition(|c| committed.contains(c)).map_or(0, |i| i + 1);
            for (j, &c) in late.iter().enumerate().skip(after) {
                let rest = cost.less_generic(1);
                // (The sources after it may be delved as well: castability
                // counted each, `delve_classes`; ENGINE.md 49.)
                let later = late.len() - j - 1;
                if (0..=(gy_left + later).min(rest.generic as usize)).any(|d| self.can_pay_for(p, &rest.less_generic(d as u8), allow)) {
                    opts.push(Opt { action: self.perm_action(p, c), choice: Choice::Card(c) });
                }
            }
        }
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

    /// Mox Diamond put onto the battlefield by an effect (waiting in no
    /// zone, `put_onto_battlefield`): `p` may discard a land card; then it
    /// enters, else it goes to its owner's graveyard (CR 614.1a).
    pub(crate) fn enter_by_discard_any(&mut self, card: CardIdx, p: PlayerId, tapped: bool) {
        if self.objs[card.i()].zone != Zone::Nowhere {
            return;
        }
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                self.discard(c);
                self.discard_entered = true;
                self.entering_tapped = tapped;
                self.put_onto_battlefield(card, p);
            }
            Some(Choice::Pass) => {
                self.move_card(card, Zone::Graveyard, Pos::Top);
            }
            Some(other) => unreachable!("discard answer {other:?}"),
            None => {
                let ctx = Ctx { controller: p, source: None };
                let land = Filter::Type(types::LAND);
                let mut opts: Vec<Opt> = self
                    .hand_options(p)
                    .into_iter()
                    .filter(|o| matches!(o.choice, Choice::Card(c) if self.card_matches(&land, c, &ctx)))
                    .collect();
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::EnterByDiscardAny { card, controller: p, tapped }, p, DecisionKind::Discard, opts);
            }
        }
    }

    /// A permanent of `p` giving the nonland cards in their graveyard
    /// escape (Underworld Breach).
    pub(crate) fn escape_granter(&self, p: PlayerId) -> Option<CardIdx> {
        self.permanents_of(p).find(|&c| self.abilities(c).grants_escape.is_some())
    }

    /// "You may play lands from your graveyard" (Crucible of Worlds).
    /// A Serra Paragon (or Lurrus) of `p` whose "once during each of your
    /// turns" is unused this turn and that allows mana value `mv` (a land
    /// with `land`; CR 601.2a, 305.1).
    pub(crate) fn paragon(&self, p: PlayerId, mv: u8, land: bool) -> Option<CardIdx> {
        self.paragons(p, mv, land).next()
    }

    /// Every such permanent (Serra Paragons and Lurruses alike).
    pub(crate) fn paragons(&self, p: PlayerId, mv: u8, land: bool) -> impl Iterator<Item = CardIdx> + '_ {
        let turn = self.turn.number as u16;
        let mine = self.turn.active == p;
        self.permanents_of(p).filter(move |&c| {
            let d = self.abilities(c);
            mine && d.gy_play_mv > 0 && mv <= d.gy_play_mv && (!land || d.gy_play_land) && {
                let r = self.obj_ref(c);
                !self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::UsedTurn(turn))
            }
        })
    }

    /// The most mana value an unused Serra Paragon of `p` allows this turn
    /// (0: none; a land needs only one).
    pub(crate) fn paragon_mv(&self, p: PlayerId) -> u8 {
        if self.turn.active != p {
            return 0;
        }
        let turn = self.turn.number as u16;
        self.permanents_of(p)
            .filter(|&c| self.abilities(c).gy_play_mv > 0)
            .filter(|&c| {
                let r = self.obj_ref(c);
                !self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::UsedTurn(turn))
            })
            .map(|c| self.abilities(c).gy_play_mv)
            .max()
            .unwrap_or(0)
    }

    /// Use Serra Paragon `c`'s permission this turn; the trigger the
    /// permanent will gain (none for Lurrus).
    pub(crate) fn use_paragon(&mut self, c: CardIdx) -> Option<(crate::ids::DefId, u8)> {
        let r = self.obj_ref(c);
        self.marks.push((r, crate::game::Mark::UsedTurn(self.turn.number as u16)));
        let def = self.objs[c.i()].def;
        let d = self.abilities(c);
        (d.gy_play_k != crate::db::NO_GRANT).then(|| (def, d.delayed_base + d.gy_play_k))
    }

    pub(crate) fn lands_from_graveyard(&self, p: PlayerId) -> bool {
        self.gy_play_now(p) || self.permanents_of(p).any(|c| self.abilities(c).lands_from_graveyard)
    }

    /// "Until end of turn, you may play lands and cast spells from your
    /// graveyard" holds for `p` (Yawgmoth's Will).
    #[inline]
    pub(crate) fn gy_play_now(&self, p: PlayerId) -> bool {
        self.players[p as usize].gy_play_turn == self.turn.number && self.turn.number > 0
    }

    /// Priority options for cards in exile with a permission (`EXILE`
    /// slots) and in the graveyard (`GRAVEYARD` slots): lands to play,
    /// spells to cast.
    fn other_zone_options(&self, p: PlayerId, classes: &[crate::pay::Class], sorcery_ok: bool, land_ok: bool, opts: &mut Vec<Opt>) {
        use crate::decision::{FACE_CAST, FACE_PLAY};
        let mut k = 0u16;
        // (Cards exiled face down that a permission lets `p` play follow the
        // face-up ones: Headliner Scarlett.)
        for &c in self.exile.iter().chain(self.face_down.iter()) {
            let cast_only = match self.exile_permission(p, c) {
                Some(crate::game::PermKind::Play { cast_only, .. }) => cast_only,
                // (Its owner may play it: a land too.)
                Some(crate::game::PermKind::Taxed(_) | crate::game::PermKind::TaxedLandsTapped(_)) => false,
                // (Airbent: cast.)
                Some(crate::game::PermKind::Airbent(_)) => true,
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
        let mut esc = None;
        let mut para = None;
        let lands_ok = land_ok
            && (self.lands_from_graveyard(p) || *para.get_or_insert_with(|| self.paragon_mv(p)) > 0 && self.paragon(p, 0, true).is_some());
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
            } else if self.gy_routes_esc(p, c, None, &mut esc, &mut para).iter().any(|r| self.castable_via(p, c, self.route_from(c, p, *r), classes, sorcery_ok)) {
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

    /// Runs `f` on the game as it is once `p` has moved spell `card` to the
    /// stack to cast it (CR 601.2a), with the card's reference there: its
    /// targets are chosen then (601.2c), so a cast is offered only if they
    /// can be (ENGINE.md 6). On the stack the card is a spell (protection
    /// from spells with a quality, 702.16b: Emrakul's from coloured spells)
    /// and has left the zone it was in (the card types in a graveyard a
    /// goyf's power counts; the hand's size Carnage Interpreter's bonus
    /// reads). On a copy of the game with the card moved when that can
    /// change a target's legality (`cast_move_matters`), else in place (the
    /// same answer, without the copy).
    pub(crate) fn as_cast<R>(&self, card: CardIdx, p: PlayerId, f: impl FnOnce(&Game, ObjRef) -> R) -> R {
        if !self.cast_move_matters(card) {
            return f(self, self.obj_ref(card));
        }
        let mut g = self.clone();
        let r = g.move_card_to_stack(card, p);
        f(&g, r)
    }

    /// Can moving spell `card` from where it is to the stack (CR 601.2a)
    /// change which objects are legal targets ([`as_cast`](Self::as_cast))?
    /// Not for a card on the stack already (an announcement of the cast in
    /// progress). From a hand or exile only through what reads it: the card
    /// becomes a spell, so a coloured one where something has protection from
    /// spells that are one or more colors (702.16b, the only quality a spell
    /// has and its card hasn't: `protected_from`); a static ability in play
    /// whose condition reads that zone (`Cond::reads_zone`: the hand's size,
    /// the cards exiled with its source). From anywhere else, always: a
    /// graveyard's card types (a goyf's power, delirium), a library's size,
    /// and a source quality `source_quality` reads only in the zones above.
    pub(crate) fn cast_move_matters(&self, card: CardIdx) -> bool {
        let zone = self.objs[card.i()].zone;
        match zone {
            Zone::Stack => false,
            Zone::Hand | Zone::Exile => {
                (self.colors_of(card) != 0 && self.spell_protection_in_play())
                    || (self.n_static > 0
                        && self
                            .battlefield
                            .iter()
                            .chain(self.command.iter())
                            .any(|&c| self.abilities(c).statics.iter().any(|s| s.cond.as_ref().is_some_and(|k| k.reads_zone(zone)))))
            }
            _ => true,
        }
    }

    /// Might some permanent have protection from spells that are one or more
    /// colors (printed, from a static ability or from an effect: the sources
    /// `protection_of` reads)?
    fn spell_protection_in_play(&self) -> bool {
        let spells = |m: &crate::db::Mod| matches!(m, crate::db::Mod::Protection(x) if x.colored_spells);
        self.battlefield.iter().any(|&c| self.def_of(c).protection.colored_spells)
            || self.effects.iter().any(|e| spells(&e.m))
            || (self.n_static > 0
                && self
                    .battlefield
                    .iter()
                    .chain(self.command.iter())
                    .any(|&c| self.abilities(c).statics.iter().any(|s| s.mods.iter().any(spells))))
    }

    /// [`targets_available`](Self::targets_available) for `ab`, spell
    /// `card`'s effect or one of its modes, as `p` would cast it now
    /// ([`as_cast`](Self::as_cast); in place when it has no targets).
    pub(crate) fn spell_targets_available(&self, ab: &AbilityDef, p: PlayerId, card: CardIdx) -> bool {
        if !ab.has_targets() {
            return self.targets_available(ab, p, Some(self.obj_ref(card)));
        }
        self.as_cast(card, p, |g, r| g.targets_available(ab, p, Some(r)))
    }

    /// Every target requirement has at least `min` legal candidates; for a
    /// modal ability, enough modes can be chosen (a mode whose targets
    /// cannot be chosen cannot be, CR 700.2, 601.2c).
    pub(crate) fn targets_available(&self, ab: &AbilityDef, controller: PlayerId, source: Option<ObjRef>) -> bool {
        match &ab.modes {
            None => {
                let ctx = Ctx { controller, source };
                if let Some(r) = ab.targets.iter().position(|tr| tr.pair != 0) {
                    // (Goblin Welder: an artifact whose controller has an
                    // artifact card in their graveyard.)
                    let k = ab.targets[r].pair as usize - 1;
                    let firsts = self.target_candidates(&ab.targets[k], &ctx, &[]);
                    let seconds = self.target_candidates(&ab.targets[r], &ctx, &[]);
                    return firsts.iter().any(|&a| seconds.iter().any(|&b| self.paired(a, b)));
                }
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
                if m.once_each {
                    let src = self.item_source(&self.stack[pos]);
                    match self.marks.iter_mut().find(|(o, x)| *o == src && matches!(x, crate::game::Mark::ModesUsed(_))) {
                        Some((_, crate::game::Mark::ModesUsed(b))) => *b |= 1 << k,
                        _ => self.marks.push((src, crate::game::Mark::ModesUsed(1 << k))),
                    }
                }
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
        // (Not the ability itself, already on the stack: CR 115.5; the stack
        // items in reach counted from below it, ENGINE.md 58.1.)
        self.choosing_for = id;
        self.targeting = id;
        let mut avail: Vec<bool> = m.list.iter().map(|x| self.targets_available(x, p, source)).collect();
        self.choosing_for = 0;
        self.targeting = 0;
        // Spree (CR 702.172a, 700.2h): a mode whose additional cost can't be
        // paid with the others chosen can't be chosen.
        if !m.costs.is_empty() {
            if let StackKind::Spell { card } = item.kind {
                for (k, a) in avail.iter_mut().enumerate() {
                    *a = *a && self.can_pay_spell(card, item, &self.announced_cost(card, item, &m.costs[k]));
                }
            }
        }
        // "Choose one that hasn't been chosen" (the source object's record).
        if m.once_each {
            let src = self.item_source(item);
            let used = self.marks.iter().find_map(|&(o, k)| match k {
                crate::game::Mark::ModesUsed(b) if o == src => Some(b),
                _ => None,
            });
            for (k, a) in avail.iter_mut().enumerate() {
                *a = *a && used.is_none_or(|b| b & (1 << k) == 0);
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
        if opts.is_empty() && m.once_each && chosen == 0 {
            // Every mode was chosen before: no mode can be chosen, the
            // triggered ability is removed (CR 700.2b, 603.3c).
            self.note(|_| "no mode left: the trigger is removed".to_string());
            self.stack.remove(pos);
            return;
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
        for &d in &item.spliced {
            if let Some((_, m)) = &self.db.def(d).splice {
                c = c.plus(m);
            }
        }
        if let Some(tc) = &def.times_cost {
            for _ in 0..item.times {
                c = c.plus(&tc.mana);
                if tc.x {
                    c.generic = c.generic.saturating_add(item.x);
                }
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
    pub(crate) fn announced_cost_pub(&self, card: CardIdx, item: &StackItem, more: &crate::mana::ManaCost) -> crate::mana::ManaCost {
        self.announced_cost(card, item, more)
    }

    /// The life the rest of spell `card`'s cost takes after its mana (CR
    /// 601.2h after 601.2g): "pay X life" and Bolas's Citadel's life equal
    /// to its mana value (118.9). A check that a mana cost announced while
    /// casting can be paid keeps it (ENGINE.md 38: Three Steps Ahead's spree
    /// costs from the Citadel at low life, Mana Confluence's life).
    pub(crate) fn life_later(&self, card: CardIdx, item: &StackItem) -> u8 {
        let x_life = if self.def_of(card).x_life { item.x } else { 0 };
        x_life.saturating_add(if item.from == crate::game::CastFrom::LibraryTop { self.mana_value_of(card) } else { 0 })
    }

    /// Cost `o` with `life` more life to pay after its mana (X life,
    /// Bolas's Citadel's: kept by its mana payment, CR 601.2h).
    fn with_life(o: &crate::db::Cost, life: u8) -> std::borrow::Cow<'_, crate::db::Cost> {
        if life == 0 {
            return std::borrow::Cow::Borrowed(o);
        }
        let mut c = o.clone();
        c.life = c.life.saturating_add(life);
        std::borrow::Cow::Owned(c)
    }

    /// Can the caster of `item` pay `cost` for spell `card` (its spending
    /// restrictions, the life its cost takes later kept)?
    pub(crate) fn can_pay_spell(&self, card: CardIdx, item: &StackItem, cost: &crate::mana::ManaCost) -> bool {
        let p = item.controller;
        let keep = crate::pay::Keep { allow: self.spend_allow(card), life: self.life_later(card, item), ..Default::default() };
        self.can_pay_with(p, cost, &self.mana_classes(p, None), &keep)
    }

    fn announced_cost(&self, card: CardIdx, item: &StackItem, more: &crate::mana::ManaCost) -> crate::mana::ManaCost {
        let p = item.controller;
        let extra = self.extra_costs(item).plus(more);
        match item.alt {
            0 => self.route_mana_plus(card, p, item.from, &extra),
            // (Its own, or the granted one noted on it: ENGINE.md 99.15.)
            _ => {
                let (a, _) = self.item_alt(card, item).expect("the alternative cost it is cast for");
                self.alt_mana_of(card, p, item.from, a, &extra)
            }
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
        let ab = self.def_of(card).spell.as_ref()?;
        let m = ab.modes.as_ref()?;
        if m.costs.is_empty() {
            return None;
        }
        // (Targets chosen with the spell on the stack: `as_cast`.)
        let pick = |g: &Game, r: ObjRef| -> Vec<crate::mana::ManaCost> {
            m.list.iter().zip(m.costs.iter()).filter(|(x, _)| g.targets_available(x, p, Some(r))).map(|(_, c)| *c).collect()
        };
        Some(if ab.has_targets() { self.as_cast(card, p, pick) } else { pick(self, self.obj_ref(card)) })
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
            if tc.x {
                // Kicker {X}: at least the least X.
                more.generic = more.generic.saturating_add(self.def_of(card).x_min);
            }
            if !self.can_pay_spell(card, &self.stack[pos], &self.announced_cost(card, &self.stack[pos], &more)) {
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
        // Spells and abilities on the stack in reach, top first (an ability
        // only for a filter that names abilities: a cheap test first).
        let abilities = wants_ability(&tr.filter);
        let spells = self.stack_in_reach().filter_map(move |s| match s.kind {
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
        let ex = wants_exile(&tr.filter);
        let exiled = self.exile.iter().filter(move |_| ex).map(|&c| Target::Obj(self.obj_ref(c)));
        players.chain(perms).chain(spells).chain(graves).chain(exiled).filter(|&t| ok(t)).nth(n - 1).is_some()
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

    /// Can `p` cast `card` for alternative cost `k` now, cast the way
    /// `from` says (its condition, its mana plus taxes, and the rest of it;
    /// with retrace, its land discard too)? Warp only from hand.
    pub(crate) fn alt_payable(&self, p: PlayerId, card: CardIdx, k: usize, classes: &[crate::pay::Class], from: crate::game::CastFrom) -> bool {
        // (Past its own: one another permanent gives it in its owner's hand,
        // cast from there; ENGINE.md 99.15.)
        if k >= self.def_of(card).alt_costs.len() && from != crate::game::CastFrom::Hand {
            return false;
        }
        let Some((a, _)) = self.alt_at(card, k) else { return false };
        if a.turn.is_some_and(|yours| (self.turn.active == p) != yours) {
            return false;
        }
        if a.warp && from != crate::game::CastFrom::Hand && !self.warp_from_graveyard_ok(card, from) {
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
            if !g.db.def(face).spell.as_ref().is_some_and(|ab| g.spell_targets_available(ab, p, card)) {
                return false;
            }
            return g.alt_payable(p, card, k, classes, from);
        }
        if a.only_if.as_ref().is_some_and(|c| !self.cond_holds(c, p)) {
            return false;
        }
        let Some(rest) = self.alt_rest(card, from, k) else { return false };
        if a.emerge.is_some() {
            // A permanent to sacrifice that leaves the reduced mana and
            // `rest` (retrace's land discard) payable (`emerge_options`).
            return !self.emerge_options(p, card, k, classes, from).is_empty();
        }
        let mana = self.alt_mana_via(card, p, from, k, &Default::default());
        // (A card cast from hand is on the stack while its cost is paid.)
        self.cost_payable(p, card, rest, &mana, classes, Some(card))
    }

    /// The non-mana part of alternative cost `k` of `card` cast the way
    /// `from` says: its own, with retrace's land discard as one cost
    /// (`CardDb::merged_cost`; `None`: the two can't be one cost).
    pub(crate) fn alt_rest(&self, card: CardIdx, from: crate::game::CastFrom, k: usize) -> Option<&'static crate::db::Cost> {
        let db: &'static crate::db::CardDb = self.db;
        if from == crate::game::CastFrom::Retrace {
            db.merged_cost(self.objs[card.i()].def, crate::db::RETRACE_ROUTE, crate::db::ALT_OPTION + k as u8)
        } else {
            self.alt_at(card, k).map(|(a, _)| &a.cost)
        }
    }

    /// The mana of alternative cost `k` (and `extra`, additional costs
    /// announced) with the cost increases and reductions (CR 601.2f, 118.9d:
    /// both apply to an alternative cost), cast the way `from` says: Elite
    /// Spellbinder's {2} more; Robber of the Rich's mana of any colour.
    pub(crate) fn alt_mana_via(
        &self,
        card: CardIdx,
        p: PlayerId,
        from: crate::game::CastFrom,
        k: usize,
        extra: &crate::mana::ManaCost,
    ) -> crate::mana::ManaCost {
        let (a, _) = self.alt_at(card, k).expect("an alternative cost of the card");
        self.alt_mana_of(card, p, from, a, extra)
    }

    /// [`alt_mana_via`](Self::alt_mana_via) for alternative cost `a` (its
    /// own or a granted one: CR 118.9d, the increases and reductions apply to
    /// either).
    pub(crate) fn alt_mana_of(
        &self,
        card: CardIdx,
        p: PlayerId,
        from: crate::game::CastFrom,
        a: &crate::db::AltCost,
        extra: &crate::mana::ManaCost,
    ) -> crate::mana::ManaCost {
        let mut base = a.mana.plus(extra);
        if let crate::game::CastFrom::Taxed(n) = from {
            base = base.plus(&crate::mana::ManaCost { generic: n, ..Default::default() });
        }
        let c = self.cost_with(card, p, base);
        match from {
            crate::game::CastFrom::Permission { any_mana: true, .. } => any_colour(c),
            _ => c,
        }
    }

    /// Can `p` cast `card` for its mana cost (with its additional cost, if
    /// any)?
    pub(crate) fn normal_payable(&self, p: PlayerId, card: CardIdx, classes: &[crate::pay::Class]) -> bool {
        let def = self.def_of(card);
        let delved = self.delve_classes(p, card, classes);
        let classes = &delved[..];
        let cost = self.spell_cost(card, p);
        let cost = self.delve_reduced(card, p, cost, 0);
        if !def.additional.is_empty() {
            self.options_payable(p, card, crate::game::CastFrom::Hand, &Default::default(), 0, classes, Some(card))
        } else {
            self.can_pay_without(p, &cost, classes, &crate::pay::Keep { allow: self.spend_allow(card), ..Default::default() }, Some(card))
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
            Some(Choice::Number(crate::eoe_c::FREE_FIRST_SPELL)) => {
                // Without paying its mana cost (CR 118.9; ENGINE.md 99.4).
                self.stack[pos].from = crate::eoe_c::FREE_CAST;
                self.note(|_| "without paying its mana cost".to_string());
            }
            Some(Choice::Card(g)) => {
                // The alternative cost permanent `g` gives it in hand (CR
                // 118.9, 601.2b; ENGINE.md 99.15): past its own.
                self.stack[pos].alt = self.def_of(card).alt_costs.len() as u8 + 1;
                self.note_granted_alt(card, g);
                self.note(|gm| format!("the alternative cost {} gives", gm.name(g)));
            }
            Some(Choice::Number(k)) => {
                self.stack[pos].alt = k;
                self.note(|_| format!("cost {k}"));
                if self.def_of(card).overload {
                    // CR 702.96a: overloaded, "target" becomes "each".
                    self.stack[pos].modes = vec![(k > 0) as u8];
                }
                if k > 0 && self.def_of(card).alt_costs[k as usize - 1].emerge.is_some() {
                    // CR 702.119: the permanent to sacrifice, before the
                    // total cost (601.2f) is determined.
                    self.agenda.push(Task::Emerge { id, sac: false });
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
                let from = item.from;
                let plain_targets = !def.overload
                    || def.spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| {
                        self.targets_available(&m.list[0], p, Some(self.obj_ref(card)))
                    });
                let plain = match from {
                    crate::game::CastFrom::Hand => self.normal_payable(p, card, &classes),
                    _ => self.route_payable(p, card, from, &classes),
                };
                if plain_targets && plain {
                    opts.push(Opt { action: act::NUMBER, choice: Choice::Number(0) });
                }
                for k in 0..self.def_of(card).alt_costs.len() {
                    if self.alt_payable(p, card, k, &classes, from) {
                        opts.push(Opt { action: act::NUMBER + 1 + k as u16, choice: Choice::Number(k as u8 + 1) });
                    }
                }
                // The ones other permanents give it in hand, each named by
                // its permanent (ENGINE.md 99.15: any number of granters,
                // each reachable; one grant a printing). Not overloaded: its
                // own target is needed, as for its mana cost.
                let own = self.def_of(card).alt_costs.len();
                for (j, &(g, _)) in self.granted_alts(card).iter().enumerate() {
                    if plain_targets && self.alt_payable(p, card, own + j, &classes, from) {
                        opts.push(Opt { action: self.perm_action(p, g), choice: Choice::Card(g) });
                    }
                }
                let free = crate::eoe_c::FREE_FIRST_SPELL;
                if plain_targets && self.first_spell_free(p, from) && self.route_payable(p, card, crate::eoe_c::FREE_CAST, &classes) {
                    opts.push(Opt { action: act::NUMBER + free as u16, choice: Choice::Number(free) });
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
        // A split card is cast as one of its halves (CR 709.3): the left one
        // unless the right one is chosen. (Before the permissions are
        // checked: whether a half can be cast from a graveyard is the half's
        // question, as the offer asked it; ENGINE.md 38.)
        let face = face.or(self.def_of(card).halves.map(|h| h.0));
        // From a graveyard, the player chooses among the permissions that
        // allow the cast (CR 601.2a-b; `Task::ChooseRoute`).
        let (from, granter) = match self.route_choice.take() {
            Some(r) => (self.route_from(card, p, r), r.granter),
            None if self.objs[card.i()].zone == Zone::Graveyard => {
                let routes = self.castable_gy_routes(p, card, face);
                match routes.first() {
                    Some(&r) if routes.len() == 1 => (self.route_from(card, p, r), r.granter),
                    Some(_) => {
                        self.agenda.push(Task::ChooseRoute { p, card, face });
                        return;
                    }
                    None => panic!("a castable card"),
                }
            }
            None => (self.cast_route(p, card).expect("a castable card"), None),
        };
        self.miracle_cast = None;
        // Serra Paragon's (or Lurrus's) permission, found while the card is
        // in the graveyard (used as it is cast).
        let mv = face.map_or(self.def_of(card).mana_value(), |f| self.db.def(f).mana_value());
        let paragon = if from == crate::game::CastFrom::Paragon { granter.or_else(|| self.paragon(p, mv, false)) } else { None };
        // "If that spell would be put into your graveyard, exile it
        // instead": a permission from a graveyard that says so (unless
        // flashback, which exiles it anyway, is how it is cast).
        let r = self.obj_ref(card);
        let origin = self.objs[card.i()].zone;
        let exile_after = self.objs[card.i()].zone == Zone::Graveyard
            && !matches!(from, crate::game::CastFrom::Flashback | crate::game::CastFrom::FlashbackMana)
            && self.permissions.iter().any(|pm| pm.card == r && pm.kind == crate::game::PermKind::CastThenExile && self.permission_ok(pm, p));
        // Cast from a graveyard by its own warp ability: for its warp cost,
        // no other (no choice; ENGINE.md 99.4).
        let forced_warp = if self.warp_only(p, card, from) { self.def_of(card).alt_costs.iter().position(|a| a.warp) } else { None };
        self.move_card_to_stack(card, p);
        if exile_after {
            self.gy_exile.push(card);
        }
        if let Some(f) = face {
            self.objs[card.i()].def = f;
        }
        self.note(|g| format!("P{p} casts {} ({from:?})", g.name(card)));
        if let Some(src) = paragon.filter(|_| self.def_of(card).x_pips > 0) {
            // The permission's mana value limit holds for the spell with X
            // (CR 202.3e): the X announced keeps it (`choose_x`).
            let r = self.obj_ref(card);
            let cap = self.abilities(src).gy_play_mv;
            self.marks.push((r, crate::game::Mark::MvCap(cap)));
        }
        if let Some((def, slot)) = paragon.and_then(|src| self.use_paragon(src)) {
            // (The card is on the stack: the spell is marked.)
            let r = self.obj_ref(card);
            self.marks.push((r, crate::game::Mark::GrantOnEnter { def, slot }));
        }
        if matches!(from, crate::game::CastFrom::Flashback | crate::game::CastFrom::FlashbackMana) {
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
            alt: forced_warp.map_or(0, |k| k as u8 + 1),
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
            spliced: Vec::new(),
        });
        self.agenda.push(Task::CastDone { id });
        if self.def_of(card).spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| m.escalate_discard) {
            // CR 702.120a: the discards, after the mana (which keeps the
            // cards they need).
            self.agenda.push(Task::Escalate { id });
        }
        self.agenda.push(Task::CastPay { id });
        if self.def_of(card).casting.choose_creature_or_warped {
            // The additional cost's choice, before the mana (CR 601.2h: the
            // costs in any order; ENGINE.md 99.14).
            self.agenda.push(Task::ChooseForCost { id, perm: false });
        }
        if self.def_of(card).delve {
            self.agenda.push(Task::Delve { id }); // CR 702.66a: while paying
        }

        if self.def_of(card).spell.as_ref().is_some_and(|ab| ab.divide.is_some()) {
            self.agenda.push(Task::Divide { id }); // CR 601.2d
        }
        if self.def_of(card).cleanup_sac.is_some() {
            let sorcery_time = self.turn.active == p && self.turn.step.is_main() && self.stack.len() <= 1;
            if !sorcery_time {
                let r = self.obj_ref(card);
                self.marks.push((r, crate::game::Mark::NotSorceryTiming));
            }
        }
        let splice = self.splice_possible(p, card);
        if splice || self.def_of(card).spell.as_ref().is_some_and(|ab| ab.has_targets()) {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
        }
        if splice {
            self.agenda.push(Task::Splice { id }); // CR 601.2b, 702.47a
        }
        if self.def_of(card).additional.len() > 1 {
            self.agenda.push(Task::CastOption { id });
        }
        if self.def_of(card).x_pips > 0 || self.def_of(card).x_life || self.def_of(card).times_cost.as_ref().is_some_and(|t| t.x) {
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
        // "without paying its mana cost" are alternative costs; a
        // permission to cast a card for its costs allows one: `alt_allowed`).
        // (Or one another permanent gives it in hand: ENGINE.md 99.15.)
        if forced_warp.is_none()
            && (!self.def_of(card).alt_costs.is_empty() && Self::alt_allowed(from)
                || self.first_spell_free(p, from)
                || from == crate::game::CastFrom::Hand && !self.granted_alts(card).is_empty())
        {
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
                let classes = self.delve_classes(p, card, &classes).into_owned();
                let keep = self.escape_exiles(card, item.from);
                let life_later = self.life_later(card, item);
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
                        kicker_ok(k)
                            && self
                                .route_extra(card, item.from, k as u8)
                                .is_some_and(|o| self.cost_payable(p, card, &Self::with_life(o, life_later), &cost, &classes, None))
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
        let reqs = crate::resolve_tail3::item_reqs(self.db, item, ar);
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
            // "X targets": exactly X (CR 601.2c).
            _ if tr.max == crate::db::X_TARGETS => item.x,
            // "Up to X targets".
            _ if tr.max == crate::db::X_UP_TO => item.x,
            // "Up to X targets, X the times it was kicked" (its Enters note).
            _ if tr.max == crate::db::KICKS_UP_TO => ((item.ev.amount >> 1) & 0xFF) as u8,
            _ => tr.max,
        };
        let min = if tr.max == crate::db::X_TARGETS { item.x } else { tr.min };
        if already.len() >= max as usize {
            self.agenda.push(Task::ChooseTargets { id, req: req + 1 });
            return;
        }
        // (The item is on the stack: the stack items in reach are counted
        // from below it, ENGINE.md 58.1.)
        self.targeting = id;
        let mut cands = self.target_candidates(tr, &ctx, &exclude);
        if let Some(n) = tr.total_mv {
            // "With total mana value n or less" (CR 601.2c; ENGINE.md 99.14).
            self.retain_within_total_mv(n, &already, &mut cands);
        }
        if tr.pair != 0 {
            // A card of the player whose permanent was chosen.
            let k = tr.pair - 1;
            let first: Vec<Target> = item.targets.iter().filter(|(r, _)| *r == k).map(|&(_, t)| t).collect();
            cands.retain(|&b| first.iter().any(|&a| self.paired(a, b)));
        }
        if let Some(later) = reqs.iter().find(|r| r.pair == req + 1) {
            // Only a permanent a later linked target can pair with (a cast is
            // offered only if it can be completed).
            let seconds = self.target_candidates(later, &ctx, &[]);
            cands.retain(|&a| seconds.iter().any(|&b| self.paired(a, b)));
        }
        if reqs.iter().any(|r| r.distinct & (1 << req) != 0) {
            // Keep a later requirement that must differ from this one
            // choosable (a cast is offered only if it can be completed).
            let chosen = item.targets.clone();
            cands.retain(|&t| self.later_distinct_ok(&reqs, req, t, &chosen, &ctx));
        }
        self.targeting = 0;
        // "This spell costs {n} less if it targets ...": only targets with
        // which its cost can still be paid (a cast is offered only if it can
        // be completed; ENGINE.md 99.4).
        if let StackKind::Spell { card } = self.stack[pos].kind {
            if self.def_of(card).discounts.iter().any(|d| matches!(d, crate::db::Discount::IfTargets(..))) {
                cands.retain(|&t| {
                    let mut item = self.stack[pos].clone();
                    item.targets.push((req, t));
                    let mut g = self.clone();
                    g.stack[pos] = item.clone();
                    g.can_pay_spell(card, &item, &g.total_mana(card, &item))
                });
            }
        }
        let mut opts: Vec<Opt> = cands
            .iter()
            .map(|&t| Opt { action: self.target_action(ctx.controller, t), choice: Choice::Target(t) })
            .collect();
        if already.len() >= min as usize {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        if opts.is_empty() {
            // Only reachable if the offer's check and this choice disagree
            // (a cast is offered only if it can be completed: `as_cast`).
            panic!(
                "no legal target for a required target: {} requirement {req} ({:?}; {} chosen), turn {} {:?}, world of {:?}",
                self.describe_target(Target::Stack(id)),
                tr.filter,
                already.len(),
                self.turn.number,
                self.turn.step,
                self.observer
            );
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
        // "As an additional cost, pay X life" (CR 601.2h; 119.4), and Bolas's
        // Citadel's life equal to its mana value rather than its mana cost
        // (118.9): paid after the mana abilities are activated (601.2g; a
        // painland's damage first), by the payment (`PayMana::life_after`).
        let life_later = self.life_later(card, item);
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        if item.alt > 0 {
            // (Its own alternative cost, or the granted one noted on it:
            // ENGINE.md 99.15.)
            let (a, _) = self.item_alt(card, item).expect("the alternative cost it is cast for");
            // Emerge: reduced by the chosen permanent's mana value; its mana
            // abilities that sacrifice it can't pay (it is sacrificed next).
            let emerge = a.emerge.as_ref().and(item.noted.first().copied()).filter(|r| self.live(*r));
            let cost = match emerge {
                Some(r) => cost.less_generic(self.mana_value_of(r.idx)),
                None => cost,
            };
            let mut pm = PayMana::new(p, cost);
            pm.improvise = def.improvise;
            pm.convoke = def.casting.convoke;
            let rest = self.item_alt_rest(card, item);
            pm.keep = Self::keep_for(card, rest, None);
            pm.keep.allow = self.spend_allow(card);
            pm.keep.spell = Some(card);
            if let Some(r) = emerge {
                pm.keep.exclude = self.sacrifices_itself_for_mana(r.idx).then_some(r.idx);
                self.agenda.push(Task::Emerge { id, sac: true });
            }
            self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
            if !pm.cost.is_zero() {
                pm.keep.life = pm.keep.life.saturating_add(life_later);
                pm.life_after = life_later;
                self.agenda.push(Task::PayMana(Box::new(pm)));
            } else {
                if life_later > 0 {
                    self.note(|_| format!("P{p} pays {life_later} life"));
                    self.lose_life(p, life_later as i32);
                }
                // (No mana to pay: delve's cards now, ENGINE.md 51.)
                self.exile_delved(p, Some(card));
            }
            return;
        }
        let mut pm = PayMana::new(p, cost);
        pm.improvise = def.improvise;
        pm.convoke = def.casting.convoke;
        // Delve's mana sources (`Mark::DelveLate`), activated first.
        if def.delve && self.marks.iter().any(|(_, m)| *m == crate::game::Mark::DelveLate) {
            let late: Vec<CardIdx> = self.delve_sources(p).into_iter().filter(|&c| self.delve_late(c)).collect();
            self.marks.retain(|(_, m)| *m != crate::game::Mark::DelveLate);
            let classes = self.mana_classes(p, None);
            for c in late {
                if let Some(cl) = classes.iter().find(|cl| cl.sac_self && cl.members.iter().any(|s| s.card == c)) {
                    let s = *cl.members.iter().find(|s| s.card == c).unwrap();
                    pm.forced.push((s, true));
                }
            }
        }
        if def.converge {
            // The payment records the colours spent (ENGINE.md 28).
            let r = self.obj_ref(card);
            self.marks.push((r, crate::game::Mark::ConvergePending(0)));
        }
        // Teamwork (a crew-like additional cost): its taps are paid before
        // the mana (CR 601.2h: any order), leaving the mana payable.
        let mut taps_first = false;
        if let Some(extra) = self.route_extra(card, item.from, item.cost_option) {
            pm.keep = Self::keep_for(card, extra, None);
            pm.forced.extend(self.exile_sources(p, card, extra, None));
            if extra.crew > 0 {
                taps_first = true;
            } else {
                self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
            }
        }
        pm.keep.allow = self.spend_allow(card);
        pm.keep.spell = Some(card);
        let escalated = item.modes.len().saturating_sub(1) as u8;
        if escalated > 0 && def.spell.as_ref().and_then(|ab| ab.modes.as_ref()).is_some_and(|m| m.escalate_discard) {
            pm.keep.discard = Some((escalated, card));
        }
        let _ = def;
        pm.keep.life = pm.keep.life.saturating_add(life_later);
        pm.life_after = life_later;
        self.agenda.push(Task::PayMana(Box::new(pm)));
        if taps_first {
            self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
        }
    }

    /// 601.2i: the spell becomes cast; "whenever you cast" abilities
    /// trigger.
    pub(crate) fn cast_done(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let p = item.controller;
        // Void: a spell was warped this turn (cast for its warp cost, CR
        // 702.185a; ENGINE.md 99.4).
        // (Its own warp or one another permanent gave it: ENGINE.md 99.15.)
        if self.item_alt(card, item).is_some_and(|(a, _)| a.warp) {
            self.void_turn = self.turn.number;
        }
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
        let before = self.players.iter().map(|pl| pl.spells_cast as u32).sum::<u32>() - 1;
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
    /// Is `b` (a card) owned by the controller of `a` (a permanent)?
    pub(crate) fn paired(&self, a: Target, b: Target) -> bool {
        match (a, b) {
            (Target::Obj(x), Target::Obj(y)) => self.objs[x.idx.i()].controller == self.objs[y.idx.i()].owner,
            _ => false,
        }
    }

    /// The stack items a spell or ability may target now, top first (an
    /// engineering limit of the action layout, class B: ENGINE.md 58.1,
    /// docs/shortcuts.md 58.1). The `STACK` ids name the top
    /// [`act::STACK_N`] (32) stack slots, and targets are chosen with their
    /// spell or ability on top of the stack (CR 601.2a-c, 602.2a-b, 603.3d),
    /// so a target may be one of the [`STACK_REACH`] (31) items below it: the
    /// top 31 items not counting the item whose targets are chosen
    /// (`Game::targeting`, while it is on the stack; a check before it is put
    /// there counts every item, and the item then goes on top). The deeper
    /// (older) items of a taller stack can't be targeted. A check and the
    /// choice after it agree: the same items are in reach.
    pub(crate) fn stack_in_reach(&self) -> impl Iterator<Item = &StackItem> + '_ {
        let me = self.targeting;
        let mut above = 0usize;
        self.stack.iter().rev().take_while(move |s| {
            let ok = above < STACK_REACH;
            if s.id != me {
                above += 1;
            }
            ok
        })
    }

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
        for s in self.stack_in_reach() {
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
        // Cards in exile, in exile order (ENGINE.md 99.14).
        if wants_exile(&tr.filter) {
            for &c in &self.exile {
                consider(Target::Obj(self.obj_ref(c)), &mut out);
            }
        }
        out
    }

    /// Hexproof and shroud (CR 702.11, 702.18); protection (702.16b: not
    /// by spells with that quality, nor abilities from sources with it).
    pub(crate) fn targetable(&self, t: Target, ctx: &Ctx) -> bool {
        match t {
            // CR 702.16b, j: a player with protection from everything can't
            // be targeted.
            Target::Player(p) => !self.player_protected(p),
            Target::Stack(_) => true,
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
            infect: false,
            spell: false,
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
                    Zone::Exile => self.exile_ok(f, r.idx, ctx),
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
            | Filter::InGraveyard(_)
            | Filter::InExile(_) => false,
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

    /// A card in exile matches only through `InExile(f)` (face up, a card:
    /// not a copy of one; ENGINE.md 99.14).
    fn exile_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::InExile(inner) => !self.is_copy_obj(c) && self.card_matches(inner, c, ctx),
            Filter::And(a, b) => self.exile_ok(a, c, ctx) && self.exile_ok(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.exile_ok(x, c, ctx)),
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
            Filter::TotalPtAtMost(n) => self.is_creature(c) && self.power(c) as i64 + self.toughness(c) as i64 <= *n as i64,
            Filter::Subtype(s) => self.has_subtype(c, *s),
            Filter::Supertype(bits) => self.supertypes_of(c) & bits != 0,
            Filter::ManaValueAtMost(n) => self.mana_value_of(c) <= *n,
            Filter::MvAtMostConverge => self.mana_value_of(c) <= self.converge_of(ctx.source),
            Filter::PowerAtLeast(n) => self.is_creature(c) && self.power(c) >= *n,
            Filter::PowerAtMost(n) => self.is_creature(c) && self.power(c) <= *n,
            Filter::ToughnessAtMost(n) => self.is_creature(c) && self.toughness(c) <= *n,
            Filter::Controller(you) => (self.objs[c.i()].controller == ctx.controller) == *you,
            Filter::Source => ctx.source.is_some_and(|s| s.idx == c && self.live(s)),
            Filter::MvAtMostManaSpent => ctx.source.is_some_and(|s| self.mana_value_of(c) <= self.mana_spent_of(s)),
            Filter::HostOfSource => {
                ctx.source.is_some_and(|s| self.live(s) && self.objs[s.idx.i()].attached.is_some_and(|h| h.idx == c && self.live(h)))
            }
            Filter::Token => self.is_token(c),
            Filter::Attacking => self.objs[c.i()].attacking,
            Filter::Tapped => self.objs[c.i()].tapped,
            Filter::Blocking => self.objs[c.i()].blocking.is_some(),
            Filter::AttachedTo(inner) => self.objs[c.i()].attached.is_some_and(|h| {
                self.live(h) && self.objs[h.idx.i()].zone == Zone::Battlefield && self.card_matches(inner, h.idx, ctx)
            }),
            Filter::DrewThisTurn(_) | Filter::InGraveyard(_) => false,
            Filter::Evoked => self.objs[c.i()].evoked,
            Filter::Named(n) => self.db.def(self.objs[c.i()].def).name == *n,
            Filter::PtIs(p, t) => self.is_creature(c) && self.power(c) == *p && self.toughness(c) == *t,
            Filter::Saddled => {
                let r = self.obj_ref(c);
                let t = self.turn.number as u16;
                self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::Saddled(t))
            }
            Filter::MvAtMostSourceX => {
                let x = ctx.source.and_then(|s| {
                    self.marks.iter().find_map(|&(o, m)| match m {
                        crate::game::Mark::KickerX(x) if o == s => Some(x),
                        _ => None,
                    })
                });
                x.is_some_and(|x| self.mana_value_of(c) <= x)
            }
            Filter::MvAtMostX => {
                let x = ctx.source.and_then(|s| self.stack.iter().find(|i| i.kind == StackKind::Spell { card: s.idx })).map_or(0, |i| i.x);
                self.mana_value_of(c) <= x
            }
            Filter::Harnessed => {
                let r = self.obj_ref(c);
                self.marks.iter().any(|&(o, m)| o == r && m == crate::game::Mark::Harnessed)
            }
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
            // Its printed abilities, or none if it lost them (granted ones
            // aren't seen: the one use is a companion's deck condition,
            // ENGINE.md 35).
            Filter::HasActivatedAbility => self.abilities(c).has_activated_ability(),
            Filter::DealtDamageThisTurn => self.dealt_damage_this_turn(c),
            Filter::CastForWarp => {
                !self.marks.is_empty() && self.marks.contains(&(self.obj_ref(c), crate::game::Mark::WarpCast))
            }
            // (Exile matches only through `filter_ok`'s `exile_ok`.)
            Filter::InExile(_) => false,
            // (Or warp another permanent gives it: ENGINE.md 99.15.)
            Filter::HasWarp => self.abilities(c).alt_costs.iter().any(|a| a.warp) || self.has_granted_warp(c),
            Filter::MvAtMostLandsYouControl => {
                self.mana_value_of(c) as u32 <= self.count_controlled(ctx.controller, &Filter::Type(crate::db::types::LAND))
            }
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

/// Can a filter match a card in exile (`InExile`)?
pub(crate) fn wants_exile(f: &Filter) -> bool {
    match f {
        Filter::InExile(_) => true,
        Filter::And(a, b) => wants_exile(a) || wants_exile(b),
        Filter::Or(v) => v.iter().any(wants_exile),
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

/// "Spend mana as though it were mana of any color" (CR 609.4b): every
/// coloured, hybrid and Phyrexian symbol of `c` is paid by any mana ({C}
/// stays {C}).
pub(crate) fn any_colour(mut c: crate::mana::ManaCost) -> crate::mana::ManaCost {
    let colored: u8 = c.pips[..5].iter().sum::<u8>() + c.hybrid.iter().sum::<u8>() + c.phyrexian.iter().sum::<u8>();
    c.generic = c.generic.saturating_add(colored);
    c.pips[..5].fill(0);
    c.hybrid = Default::default();
    c.phyrexian = Default::default();
    c
}
