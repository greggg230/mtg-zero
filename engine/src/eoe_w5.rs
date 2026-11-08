//! Edge of Eternities, W5 (ENGINE.md 99.15): alternative costs a permanent's
//! static ability gives the cards in its controller's hand (CR 118.9, 604.1,
//! 611.3a): "Artifact cards and red creature cards in your hand have warp
//! {2}{R}" (Tannuk, Steadfast Second). The grants live on the granting
//! definition (`CastingRules::hand_alt_grants`, at most one a printing). In
//! `ChooseAltCost` (CR 601.2b) a granted cost is named by its permanent
//! (`Choice::Card`, the permanent's `MY_PERM` id), so any number of granters
//! is reachable within the existing action ranges; chosen, the spell's
//! `StackItem::alt` is one past its own alternative costs and the grant is
//! noted on the spell (`Mark::AltGranted`: the permanent, its definition, the
//! grant).
//!
//! The rules:
//! * A card in its owner's hand has each grant of each permanent the owner
//!   controls whose filter it matches (CR 611.3a: continuously, by the
//!   characteristics it has; casting a face, by the face's, 601.3e). Warp is
//!   cast "from your hand" (CR 702.185a): from hand only, and as an
//!   alternative cost, never with another (118.9a, 601.2b).
//! * The cost is the granted one with the spell's cost increases and
//!   reductions (CR 118.9d, 601.2f); it has no {X}, so X is 0 (107.3b).
//! * The spell keeps the granted ability while the granting permanent's
//!   ability would still apply to it: the same object, on the battlefield,
//!   controlled by the card's owner, with the ability, the spell matching its
//!   filter (CR 400.7g, 611.3a-b). Warp's exile is created as the permanent
//!   enters only if the spell has the ability then (CR 608.3g); it was cast
//!   for its warp cost either way (void, CR 702.185c; "if that creature was
//!   cast for its warp cost").
//! * A copy of the spell copies the choice (CR 707.10) but not the granted
//!   ability (not a copiable value, 707.2; not a card cast from a hand,
//!   400.7g): no exile, and not cast.

use crate::cast::Ctx;
use crate::db::{AltCost, Cost};
use crate::game::{CastFrom, Game, Mark, StackItem, StackKind};
use crate::ids::{CardIdx, DefId, ObjRef, PlayerId, Zone};
use crate::svec::SVec;

impl Game {
    /// The alternative costs other permanents' static abilities give `card`
    /// now, as (granting permanent, its grant): in its owner's hand, each
    /// grant of each permanent the owner controls whose filter the card
    /// matches, in battlefield order (CR 611.3a); a spell cast from its
    /// owner's hand (not a copy), the grants that would still apply to it
    /// (CR 400.7g): the ones its cast may choose among while it is cast.
    pub(crate) fn granted_alts(&self, card: CardIdx) -> SVec<(CardIdx, u8), 2> {
        let mut out = SVec::new();
        if !self.db.hand_alt_grants {
            return out;
        }
        let o = &self.objs[card.i()];
        let ok = match o.zone {
            Zone::Hand => true,
            Zone::Stack => {
                !self.is_copy_obj(card) && self.stack.iter().any(|s| s.kind == StackKind::Spell { card } && s.origin == Zone::Hand)
            }
            _ => false,
        };
        if !ok {
            return out;
        }
        let owner = o.owner;
        let ctx = Ctx { controller: owner, source: None };
        for g in self.permanents_of(owner) {
            for (i, gr) in self.abilities(g).casting.hand_alt_grants.iter().enumerate() {
                if self.card_matches(&gr.filter, card, &ctx) {
                    out.push((g, i as u8));
                }
            }
        }
        out
    }

    /// Alternative cost `k` of `card` as it may be chosen now: its own k-th,
    /// or (past its own) the granted one `k - own` of [`granted_alts`]
    /// (Self::granted_alts), with the definition holding it (`None`: the
    /// card's own).
    pub(crate) fn alt_at(&self, card: CardIdx, k: usize) -> Option<(&'static AltCost, Option<DefId>)> {
        let own = &self.def_of(card).alt_costs;
        if let Some(a) = own.get(k) {
            return Some((a, None));
        }
        let (g, i) = *self.granted_alts(card).get(k - own.len())?;
        let d = self.objs[g.i()].def;
        Some((&self.db.def(d).casting.hand_alt_grants[i as usize].alt, Some(d)))
    }

    /// The granted alternative cost spell `card` was cast for, if one
    /// (`Mark::AltGranted`): (granting permanent, its definition, its grant).
    fn alt_granted_mark(&self, card: CardIdx) -> Option<(ObjRef, DefId, u8)> {
        if self.marks.is_empty() {
            return None;
        }
        let r = self.obj_ref(card);
        self.marks.iter().find_map(|&(o, m)| match m {
            Mark::AltGranted { granter, def, grant } if o == r => Some((granter, def, grant)),
            _ => None,
        })
    }

    /// The alternative cost spell `card` (stack item `item`) was cast for:
    /// its own, or the granted one noted as it was chosen (which no longer
    /// depends on its granter: the total cost is locked in, CR 601.2f). None
    /// for its mana cost, and for a copy of a spell cast for a granted cost
    /// (the copy has the choice, CR 707.10, not the ability).
    pub(crate) fn item_alt(&self, card: CardIdx, item: &StackItem) -> Option<(&'static AltCost, Option<DefId>)> {
        let k = (item.alt as usize).checked_sub(1)?;
        if let Some(a) = self.def_of(card).alt_costs.get(k) {
            return Some((a, None));
        }
        let (_, def, grant) = self.alt_granted_mark(card)?;
        Some((&self.db.def(def).casting.hand_alt_grants.get(grant as usize)?.alt, Some(def)))
    }

    /// The non-mana part of the alternative cost spell `item` is cast for
    /// (`alt_rest` for its own; a granted one's own rest).
    pub(crate) fn item_alt_rest(&self, card: CardIdx, item: &StackItem) -> &'static Cost {
        let (a, home) = self.item_alt(card, item).expect("the alternative cost it is cast for");
        match home {
            None => self.alt_rest(card, item.from, item.alt as usize - 1).unwrap_or(&a.cost),
            Some(_) => &a.cost,
        }
    }

    /// Does spell `card`, cast for a granted alternative cost, still have
    /// that ability: its granting permanent's ability would still apply to it
    /// (CR 400.7g, 611.3a-b): the same object on the battlefield, controlled
    /// by the card's owner ("your hand"), with the ability (not lost, the
    /// same definition), the spell matching its filter?
    pub(crate) fn alt_grant_applies(&self, card: CardIdx) -> bool {
        let Some((g, def, grant)) = self.alt_granted_mark(card) else { return false };
        let owner = self.objs[card.i()].owner;
        let go = &self.objs[g.idx.i()];
        self.live(g)
            && go.zone == Zone::Battlefield
            && go.controller == owner
            && go.def == def
            && !self.lost_abilities(g.idx)
            && self.db.def(def).casting.hand_alt_grants.get(grant as usize).is_some_and(|gr| {
                self.card_matches(&gr.filter, card, &Ctx { controller: owner, source: None })
            })
    }

    /// Can `p` cast card `c` from their hand for one of the alternative costs
    /// other permanents give it (its condition, its mana with increases and
    /// reductions, the rest; CR 118.9, 118.9d)?
    pub(crate) fn granted_alt_castable(&self, p: PlayerId, c: CardIdx, classes: &[crate::pay::Class]) -> bool {
        let own = self.def_of(c).alt_costs.len();
        (0..self.granted_alts(c).len()).any(|j| self.alt_payable(p, c, own + j, classes, CastFrom::Hand))
    }

    /// Does `c` have warp from another permanent's ability (`Filter::HasWarp`,
    /// CR 702.185a): a card in hand with a granted warp, or a spell cast for
    /// a granted warp cost that still has it (CR 400.7g)?
    pub(crate) fn has_granted_warp(&self, c: CardIdx) -> bool {
        if !self.db.hand_alt_grants {
            return false;
        }
        match self.objs[c.i()].zone {
            Zone::Hand => self.granted_alts(c).iter().any(|&(g, i)| self.abilities(g).casting.hand_alt_grants[i as usize].alt.warp),
            Zone::Stack => {
                self.alt_grant_applies(c)
                    && self
                        .alt_granted_mark(c)
                        .is_some_and(|(_, d, i)| self.db.def(d).casting.hand_alt_grants[i as usize].alt.warp)
            }
            _ => false,
        }
    }

    /// The spell `card` is cast for the alternative cost permanent `g` gives
    /// it (`ChooseAltCost`'s answer `Choice::Card(g)`, CR 601.2b; a printing
    /// has at most one such grant, the lowering's rule, so the permanent
    /// names it): noted on the spell with its granting permanent.
    pub(crate) fn note_granted_alt(&mut self, card: CardIdx, g: CardIdx) {
        let (_, grant) = *self.granted_alts(card).iter().find(|x| x.0 == g).expect("a granted alternative cost offered");
        let r = self.obj_ref(card);
        let granter = self.obj_ref(g);
        let def = self.objs[g.i()].def;
        self.marks.push((r, Mark::AltGranted { granter, def, grant }));
    }

    /// Is spell `card` cast for a granted alternative cost, which has no {X}
    /// (the lowering denies one): X is 0 (CR 107.3b)?
    pub(crate) fn cast_for_granted_alt(&self, card: CardIdx, item: &StackItem) -> bool {
        self.item_alt(card, item).is_some_and(|(_, home)| home.is_some())
    }
}
