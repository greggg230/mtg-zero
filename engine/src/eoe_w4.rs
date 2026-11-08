//! Edge of Eternities, W4 (ENGINE.md 99.14): exile, graveyard, copies and
//! control. The ops W4's cards add, named after the rules actions, each
//! citing its rule; never after a card.

use crate::db::Counter;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::Game;
use crate::ids::{CardIdx, Target, Zone};
use crate::program::Who;
use crate::resolve::{Flow, Frame};

/// W4's instructions (`Ext::EoeW4`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeW4 {
    /// "... unless that player discards a card at random" (ward, CR
    /// 702.21a; a random discard, 701.9b): `who` may pay; paid, a card of
    /// their hand chosen at random is discarded and the program continues at
    /// `if_paid`. Choice: `YesNo` (`YES` only with a card in hand; asked
    /// with an empty hand too, as ward's life payment is).
    OfferDiscardRandom { who: Who, if_paid: u16 },
    /// "Return <targets> from your graveyard to the battlefield. Put a
    /// <counter> on each of them": every legal target of `tgt` still in a
    /// graveyard is put onto the battlefield under the controller's control,
    /// together (one instruction, as Exhume's), then a counter of kind
    /// `counter` is put on each that entered (CR 122.1; after they enter,
    /// not as they enter, 614.1c). With `tapped`, they enter tapped; with
    /// `attacking`, attacking (CR 508.4: during combat, under the active
    /// player's control; their controller chooses what each attacks, and
    /// they were never declared as attackers).
    GraveyardTargetsOntoBattlefield { tgt: u8, counter: Option<Counter>, tapped: bool, attacking: bool },
    /// Each legal target of `tgt` (a card in exile) is put on the bottom of
    /// its owner's library (CR 401.4; public, so known to all there).
    ExiledTargetToLibraryBottom { tgt: u8 },
}

impl EoeW4 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeW4::OfferDiscardRandom { who, .. } => matches!(who, Who::EventPlayer),
            EoeW4::GraveyardTargetsOntoBattlefield { .. } | EoeW4::ExiledTargetToLibraryBottom { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            EoeW4::OfferDiscardRandom { if_paid, .. } => Some(if_paid),
            EoeW4::GraveyardTargetsOntoBattlefield { .. } | EoeW4::ExiledTargetToLibraryBottom { .. } => None,
        }
    }
}

impl Game {
    /// The mana value of a target object (0 for a player or an ability).
    fn target_mv(&self, t: Target) -> u32 {
        match t {
            Target::Obj(r) => self.mana_value_of(r.idx) as u32,
            Target::Player(_) | Target::Stack(_) => 0,
        }
    }

    /// "Up to n targets with total mana value m or less" (CR 601.2c): the
    /// targets are chosen one at a time, and only a candidate that keeps
    /// the chosen ones' total within `total` is offered. That is the
    /// announced set's check: every set so built is within the total, and
    /// every set within it can be built (mana values are never negative, so
    /// each part of a legal set is within the total too, in any order). The
    /// targets are cards in a graveyard, whose mana values nothing changes
    /// there, so the total holds as the spell resolves (608.2b checks each
    /// target on its own).
    pub(crate) fn retain_within_total_mv(&self, total: u8, chosen: &[Target], cands: &mut Vec<Target>) {
        let sum: u32 = chosen.iter().map(|&t| self.target_mv(t)).sum();
        cands.retain(|&t| sum + self.target_mv(t) <= total as u32);
    }

    /// A target in exile (ENGINE.md 99.14): its option is `EXILE` + its
    /// place among the decision's offered exiled cards, in exile order (as
    /// `ChooseExiled`'s "k-th offered card"; the exile is public, so the
    /// list is the same in every world). Beyond the 32 `EXILE` slots the
    /// first 32 are offered (docs/shortcuts.md 58.4).
    pub(crate) fn name_exile_targets(&self, kind: DecisionKind, options: &mut Vec<Opt>) {
        if !matches!(kind, DecisionKind::Target | DecisionKind::NewTarget) {
            return;
        }
        let mut k = 0u16;
        options.retain_mut(|o| {
            if let Choice::Target(Target::Obj(r)) = o.choice {
                if self.objs[r.idx.i()].zone == Zone::Exile {
                    if k >= act::EXILE_N {
                        return false;
                    }
                    o.action = act::EXILE + k;
                    k += 1;
                }
            }
            true
        });
    }

    /// Is `c` a warped card: in exile by warp's delayed ability, which may
    /// be cast from exile on a later turn (CR 702.185a; `Ext::WarpExile`'s
    /// permission: from a later turn, for its mana cost, cast only)?
    pub(crate) fn is_warped(&self, c: CardIdx) -> bool {
        let r = self.obj_ref(c);
        self.objs[c.i()].zone == Zone::Exile
            && self.permissions.iter().any(|q| {
                q.card == r
                    && q.turn.is_some_and(|t| t & crate::resolve_tail2::PERM_LATER != 0)
                    && matches!(q.kind, crate::game::PermKind::Play { free: false, cast_only: true, .. })
            })
    }

    /// The warped creature cards `p` owns in exile, in exile order.
    fn warped_creature_cards(&self, p: crate::ids::PlayerId) -> Vec<CardIdx> {
        self.exile
            .iter()
            .copied()
            .filter(|&c| self.objs[c.i()].owner == p && !self.is_copy_obj(c) && self.is_creature(c) && self.is_warped(c))
            .collect()
    }

    /// Can `p` choose "a creature you control or a warped creature card you
    /// own in exile" (ENGINE.md 99.14)?
    pub(crate) fn chosen_for_cost_candidates(&self, p: crate::ids::PlayerId) -> bool {
        self.creatures_of(p).next().is_some() || !self.warped_creature_cards(p).is_empty()
    }

    /// `Task::ChooseForCost`: "as an additional cost to cast this spell,
    /// choose a creature you control or a warped creature card you own in
    /// exile" (CR 601.2h). One choice in two decisions: an exiled card
    /// (`ChooseExiled`, `EXILE` + k; `PASS` = a creature instead, offered
    /// when there is one), then a creature (`ChoosePermanent`); a decision
    /// with nothing of its kind is skipped. Each candidate is reached by
    /// exactly one sequence. The choice is noted on the spell.
    pub(crate) fn choose_for_cost(&mut self, id: u32, perm: bool) {
        let Some(pos) = self.item_index(id) else { return };
        let p = self.stack[pos].controller;
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                self.note(|g| format!("P{p} chooses {} for the cost", g.name(c)));
                let r = self.obj_ref(c);
                self.stack[pos].noted = vec![r];
                return;
            }
            Some(Choice::Pass) => {
                self.agenda.push(Task::ChooseForCost { id, perm: true });
                return;
            }
            Some(other) => unreachable!("cost choice {other:?}"),
            None => {}
        }
        let creatures: Vec<CardIdx> = self.creatures_of(p).collect();
        if !perm {
            let cards = self.warped_creature_cards(p);
            if !cards.is_empty() {
                let mut opts: Vec<Opt> = cards
                    .iter()
                    .take(act::EXILE_N as usize)
                    .enumerate()
                    .map(|(k, &c)| Opt { action: act::EXILE + k as u16, choice: Choice::Card(c) })
                    .collect();
                if !creatures.is_empty() {
                    opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                }
                self.ask(Task::ChooseForCost { id, perm: false }, p, DecisionKind::ChooseExiled, opts);
                return;
            }
        }
        let opts: Vec<Opt> = creatures.iter().map(|&c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) }).collect();
        if opts.is_empty() {
            return; // (Castability requires a candidate.)
        }
        self.ask(Task::ChooseForCost { id, perm: true }, p, DecisionKind::ChoosePermanent, opts);
    }

    /// `Amount::ChosenPower` (CR 608.2h).
    pub(crate) fn chosen_power(&self, f: &Frame) -> i32 {
        let Some(&r) = f.noted.first() else { return 0 };
        let o = &self.objs[r.idx.i()];
        if self.live(r) {
            return match o.zone {
                Zone::Battlefield => self.power(r.idx),
                // A card in exile: its own power (no effect applies there).
                _ => self.def_of(r.idx).power.unwrap_or(0),
            };
        }
        // Gone: as it last existed where it was (a permanent's last known
        // power; a card that left exile, its card's printed power).
        match o.lki_power {
            Some((inc, p)) if inc == r.inc => p,
            _ => self.db.def(o.def).power.unwrap_or(0),
        }
    }

    pub(crate) fn exec_eoe_w4(&mut self, f: &mut Frame, op: &EoeW4) -> Flow {
        match op {
            EoeW4::OfferDiscardRandom { who, if_paid } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        // A card at random (CR 701.9b): the hand's canonical
                        // order makes the pick independent of insertion order;
                        // the discard's reveal key names the card.
                        let sorted = self.hand_sorted(p);
                        if sorted.is_empty() {
                            return Flow::Next;
                        }
                        let c = match self.forced_hand_pick(p, &sorted) {
                            Some(c) => c,
                            None => sorted[self.rng.below(sorted.len() as u32) as usize],
                        };
                        self.note(|g| format!("P{p} discards {} at random to pay", g.name(c)));
                        self.discard(c);
                        Flow::Jump(*if_paid)
                    }
                    Some(_) => Flow::Next,
                    None => {
                        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                        if !self.players[p as usize].hand.is_empty() {
                            opts.push(Opt { action: act::YES, choice: Choice::Yes });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                        Flow::Suspend
                    }
                }
            }
            EoeW4::GraveyardTargetsOntoBattlefield { tgt, counter, tapped, attacking } => {
                use crate::game::Step;
                let mut entered: Vec<CardIdx> = Vec::new();
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone != Zone::Graveyard {
                        continue;
                    }
                    self.entering_tapped = *tapped;
                    self.put_onto_battlefield(o.idx, f.controller);
                    self.entering_tapped = false;
                    if self.objs[o.idx.i()].zone == Zone::Battlefield {
                        entered.push(o.idx);
                    }
                }
                if let Some(k) = counter {
                    for &c in &entered {
                        self.add_counters(c, *k, 1);
                    }
                }
                if *attacking {
                    let in_combat =
                        matches!(self.turn.step, Step::DeclareAttackers | Step::DeclareBlockers | Step::FirstStrikeDamage | Step::CombatDamage);
                    let mut asks = Vec::new();
                    for &c in &entered {
                        if in_combat && f.controller == self.turn.active && self.is_creature(c) {
                            self.objs[c.i()].attacking = true;
                            asks.push(c);
                        }
                    }
                    // CR 508.4: its controller chooses what each attacks.
                    for &c in asks.iter().rev() {
                        self.agenda.push(Task::AttackTarget { card: c });
                    }
                }
                Flow::Next
            }
            EoeW4::ExiledTargetToLibraryBottom { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone == Zone::Exile {
                            self.move_card(o.idx, Zone::Library, crate::game::Pos::Bottom);
                        }
                    }
                }
                Flow::Next
            }
        }
    }
}
