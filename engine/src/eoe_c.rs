//! Edge of Eternities lane C (ENGINE.md 99.4): the ops of void, warp, spells
//! cast this turn and the graveyard / exile / library / hand effects the
//! set adds. Named after the rules actions, each citing its rule.

use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, PlayerId, Target, Zone};
use crate::program::{Amount, Who};
use crate::resolve::{Flow, Frame};

/// Lane C's instructions (`Ext::EoeC`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeC {
    /// "Exile all graveyards": every card in every graveyard is exiled at
    /// once (one event group, CR 603.2c).
    ExileAllGraveyards,
    /// "Put target card from a graveyard on the bottom of its owner's
    /// library" (CR 401.4: its owner's library).
    GraveyardCardToBottom { tgt: u8 },
    /// `who` loses `n` life if they have no card in hand ("each opponent
    /// discards a card; each opponent who can't loses n life": before the
    /// discard, who can't is who has no card).
    LoseLifeIfHandEmpty { who: Who, n: u8 },
    /// Exile the top `n` cards of the controller's library; they may play
    /// them this turn (face up, revealed as they are exiled).
    ExileTopPlayThisTurn { n: Amount },
    /// `who` exiles a card of their choice from their hand (a decision of
    /// theirs: they know it); they may play it for as long as it stays
    /// exiled, a spell cast this way costing {tax} more (CR 601.2f) and a
    /// land played this way entering tapped (`PermKind::TaxedLandsTapped`).
    ExileFromOwnHandPlayable { who: Who, tax: u8 },
    /// A delayed trigger's card (exiled) returns to the battlefield tapped
    /// under its owner's control.
    ReturnEventCardTapped,
}

/// `ChooseAltCost`'s answer for "without paying its mana cost" by a
/// permanent's "the first spell each player casts during each of their
/// turns may be cast without paying its mana cost" (the last `NUMBER`).
pub(crate) const FREE_FIRST_SPELL: u8 = (act::NUMBER_N - 1) as u8;

/// How a spell cast without paying its mana cost is cast (CR 118.9).
pub(crate) const FREE_CAST: crate::game::CastFrom = crate::game::CastFrom::Permission { free: true, any_mana: false };

impl Game {
    /// "Each player can't cast more than one noncreature spell each turn":
    /// `p` has cast one, and `card` (with the face it would be cast as) is a
    /// noncreature spell.
    pub(crate) fn noncreature_spell_locked(&self, p: PlayerId, card: CardIdx) -> bool {
        self.players[p as usize].noncreature_cast >= 1
            && !self.def_of(card).is(crate::db::types::CREATURE)
            && self.battlefield.iter().any(|&c| self.abilities(c).casting.one_noncreature_spell)
    }

    /// May `p` cast the spell they are casting (or would cast) the way
    /// `from` says without paying its mana cost: the first spell they cast
    /// during their own turn while a permanent says so (CR 118.9; 118.9a:
    /// not with another alternative cost, `alt_allowed`)?
    pub(crate) fn first_spell_free(&self, p: PlayerId, from: crate::game::CastFrom) -> bool {
        self.turn.active == p
            && self.players[p as usize].spells_cast == 0
            && Self::alt_allowed(from)
            && self.battlefield.iter().any(|&c| self.abilities(c).casting.first_spell_free)
    }

    pub(crate) fn exec_eoe_c(&mut self, f: &mut Frame, op: &EoeC) -> Flow {
        match op {
            EoeC::ExileAllGraveyards => {
                let mut cards = Vec::new();
                for p in [self.turn.active, crate::ids::opp(self.turn.active)] {
                    cards.extend(self.players[p as usize].graveyard.iter().copied());
                }
                for c in cards {
                    if self.objs[c.i()].zone == Zone::Graveyard {
                        self.move_card(c, Zone::Exile, Pos::Top);
                    }
                }
                Flow::Next
            }
            EoeC::GraveyardCardToBottom { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    if let Target::Obj(o) = t {
                        if self.objs[o.idx.i()].zone == Zone::Graveyard {
                            self.move_card(o.idx, Zone::Library, Pos::Bottom);
                        }
                    }
                }
                Flow::Next
            }
            EoeC::ExileTopPlayThisTurn { n } => {
                let q = f.controller;
                let n = self.amount(f, *n).max(0);
                for _ in 0..n {
                    let Some(&top) = self.players[q as usize].library.last() else { break };
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, q);
                    self.reveal(top);
                    let card = self.move_card(top, Zone::Exile, Pos::Top);
                    self.permissions.push(crate::game::Permission {
                        card,
                        player: q,
                        turn: Some(self.turn.number),
                        kind: crate::game::PermKind::Play { free: false, cast_only: false, any_mana: false, attacked_subtype: None },
                    });
                }
                Flow::Next
            }
            EoeC::ExileFromOwnHandPlayable { who, tax } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.note(|g| format!("P{p} exiles {} from their hand", g.name(c)));
                        let card = self.move_card(c, Zone::Exile, Pos::Top);
                        if self.objs[card.idx.i()].zone == Zone::Exile {
                            self.permissions.push(crate::game::Permission {
                                card,
                                player: p,
                                turn: None,
                                kind: crate::game::PermKind::TaxedLandsTapped(*tax),
                            });
                        }
                        Flow::Next
                    }
                    Some(other) => unreachable!("hand choice {other:?}"),
                    None => {
                        // One option per definition (the chooser's own hand).
                        let hand = self.hand_sorted(p);
                        let mut opts = Vec::new();
                        for (k, &c) in hand.iter().enumerate() {
                            if k > 0 && self.objs[hand[k - 1].i()].def == self.objs[c.i()].def {
                                continue;
                            }
                            assert!((k as u16) < act::LIBRARY_N, "hand slot {k} out of range");
                            opts.push(Opt { action: act::LIBRARY + k as u16, choice: Choice::Card(c) });
                        }
                        if opts.is_empty() {
                            return Flow::Next;
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseDiscard, opts);
                        Flow::Suspend
                    }
                }
            }
            EoeC::ReturnEventCardTapped => {
                if let Some(r) = f.ev.obj {
                    if self.live(r) && self.objs[r.idx.i()].zone == Zone::Exile {
                        let owner = self.objs[r.idx.i()].owner;
                        self.entering_tapped = true;
                        self.put_onto_battlefield(r.idx, owner);
                        self.entering_tapped = false;
                    }
                }
                Flow::Next
            }
            EoeC::LoseLifeIfHandEmpty { who, n } => {
                if let Some(p) = self.who(f, *who) {
                    if self.players[p as usize].hand.is_empty() {
                        self.lose_life(p, *n as i32);
                    }
                }
                Flow::Next
            }
        }
    }
}
