//! Decisions, choices and the action-id layout (ENGINE.md 11).

use crate::db::AbilitySlot;
use crate::ids::{CardIdx, DefId, PlayerId, Target};

/// Global action ids are `< NUM_ACTIONS` (zero-core's `SearchGame::NUM_ACTIONS`).
pub const NUM_ACTIONS: usize = 1024;

/// Decision-relative action id ranges. Object slots are canonical and
/// relative to the decider (ENGINE.md 11).
pub mod act {
    /// Pass priority / no / keep / done / no block / decline.
    pub const PASS: u16 = 0;
    /// Yes / take a mulligan / pay.
    pub const YES: u16 = 1;
    /// 2 = the decider, 3 = the opponent.
    pub const PLAYER: u16 = 2;
    /// n = 0..31.
    pub const NUMBER: u16 = 4;
    pub const NUMBER_N: u16 = 32;
    /// The decider's hand, slot 0..63.
    pub const HAND: u16 = 36;
    pub const HAND_N: u16 = 64;
    /// The decider's permanents, slot 0..95.
    pub const MY_PERM: u16 = 100;
    pub const PERM_N: u16 = 96;
    /// The opponent's permanents, slot 0..95.
    pub const OPP_PERM: u16 = 196;
    /// Stack slot 0..31, 0 = top.
    pub const STACK: u16 = 292;
    pub const STACK_N: u16 = 32;
    /// The decider's waiting triggers, slot 0..15.
    pub const TRIGGER: u16 = 324;
    pub const TRIGGER_N: u16 = 16;
    /// A searched / looked-at pile, slot 0..63 (M4b).
    pub const LIBRARY: u16 = 340;
    pub const LIBRARY_N: u16 = 64;
    /// Graveyards: the decider's 0..63, the opponent's 64..127 (M4b).
    pub const GRAVEYARD: u16 = 404;
    /// Activate ability i of the decider's permanent in `MY_PERM` slot s:
    /// `ACTIVATE + s * 4 + i` (at priority, and mana abilities while paying).
    pub const ACTIVATE: u16 = 532;
    /// Activate the ability of the card in the decider's hand slot 0..63
    /// (cycling, Spirit Guides).
    pub const HAND_ABILITY: u16 = 916;
    /// Play / cast the k-th card in exile the decider may play (exile
    /// order; M7a), k = 0..31.
    pub const EXILE: u16 = 980;
    pub const EXILE_N: u16 = 32;
    /// First reserved id.
    pub const RESERVED: u16 = 1012;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DecisionKind {
    /// Pass, play a land, cast a spell (CR 117).
    Priority,
    /// Keep (`PASS`) or take a mulligan (`YES`) (CR 103.5).
    Mulligan,
    /// Put a hand card on the bottom after a mulligan (CR 103.5).
    MulliganBottom,
    /// Choose a target (CR 601.2c, 603.3d); `PASS` = no more targets.
    Target,
    /// Choose the next mana source to tap (CR 601.2g).
    PayMana,
    /// Add an attacker (increasing slot order) or `PASS` = done (CR 508.1).
    DeclareAttackers,
    /// Which attacker the next potential blocker blocks, or `PASS` (CR 509.1).
    DeclareBlockers,
    /// Combat damage to the next blocker (`NUMBER`) (CR 510.1c).
    AssignDamage,
    /// Pay a cost / do an optional action: `YES` or `PASS`.
    YesNo,
    /// Choose a card in hand for an effect (Brainstorm).
    ChooseHandCard,
    /// Discard to hand size (CR 514.1).
    Discard,
    /// Which waiting trigger goes on the stack next (CR 603.3b).
    OrderTriggers,
    /// Which legendary permanent to keep (CR 704.5j).
    LegendRule,
    /// How many Phyrexian pips of a colour to pay with 2 life each
    /// (`NUMBER`, CR 107.4f).
    PayPhyrexian,
    /// Which mana a mana ability makes (`NUMBER` = production index, CR
    /// 106.1b).
    ChooseMana,
    /// A permanent to sacrifice as a cost (CR 701.17).
    Sacrifice,
    /// A card to discard as a cost.
    DiscardCost,
    /// Which additional cost to pay (`NUMBER` = option index, CR 601.2b).
    ChooseCost,
    /// A card to find in a library search (`LIBRARY` = the slot of its
    /// definition in the library sorted by definition; `PASS` = find
    /// nothing; CR 701.19).
    SearchLibrary,
    /// The next card of a looked-at pile to put on top of the library
    /// (`LIBRARY` = its slot in the pile sorted by definition).
    PutOnTop,
    /// The next card of the pile to put on the bottom (`PASS` = done when
    /// the number is free: scry).
    PutOnBottom,
    /// A card of the pile to put into the graveyard (`PASS` = done:
    /// surveil).
    PutInGraveyard,
    /// A card of the pile to put into the hand (`PASS` = done when allowed).
    PutInHand,
    /// A card of another player's revealed hand for them to discard
    /// (`LIBRARY` = its slot in that hand sorted by definition; `PASS` =
    /// none, when allowed).
    ChooseDiscard,
    /// The next mode of a modal spell or ability (`NUMBER` = mode index, in
    /// non-decreasing order; `PASS` = no more, once enough are chosen; CR
    /// 700.2).
    ChooseMode,
    /// A colour (`NUMBER` = W U B R G index; 5 = colorless).
    ChooseColor,
    /// A permanent chosen by an effect (not targeted), one per decision in
    /// increasing slot order; `PASS` = no more.
    ChoosePermanent,
    /// A card in the decider's graveyard (`GRAVEYARD` slot).
    ChooseGraveyardCard,
    /// Which cost to cast a spell for: `NUMBER` 0 = its mana cost, k + 1 =
    /// alternative cost k (CR 118.9, 601.2b).
    ChooseAltCost,
    /// A card in the decider's hand to exile as a cost.
    ExileCost,
    /// A permanent to return to its owner's hand as a cost.
    BounceCost,
    /// The amount of divided damage for the next target (`NUMBER`, CR
    /// 601.2d).
    DivideDamage,
    // (At most 64 kinds: the network's context block has 64 slots, layout v3. "Top or
    // bottom" is a `YesNo`: `YES` = top.)
}

/// What an action means to the task that asked.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Choice {
    Pass,
    Yes,
    Number(u8),
    /// A card or permanent (hand card, attacker, blocker's attacker, mana
    /// source, legend to keep).
    Card(CardIdx),
    /// Priority: play this land.
    Play(CardIdx),
    /// Priority: cast this spell.
    Cast(CardIdx),
    Target(Target),
    /// Index into the decider's waiting triggers.
    Trigger(u8),
    /// Activate this ability of this card (priority; a mana source while
    /// paying).
    Ability(CardIdx, AbilitySlot),
    /// A card of this definition in a zone hidden from the decider (a
    /// library search): which physical card is never part of the choice
    /// (ENGINE.md 10).
    Def(DefId),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Opt {
    pub action: u16,
    pub choice: Choice,
}

/// The one pending decision: options sorted by action id, no duplicates,
/// never empty.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Decision {
    pub player: PlayerId,
    pub kind: DecisionKind,
    pub options: Vec<Opt>,
}

impl Decision {
    pub fn choice_of(&self, action: u16) -> Option<Choice> {
        self.options
            .binary_search_by_key(&action, |o| o.action)
            .ok()
            .map(|i| self.options[i].choice)
    }

    pub fn action_of(&self, choice: Choice) -> Option<u16> {
        self.options.iter().find(|o| o.choice == choice).map(|o| o.action)
    }
}

// ---- the slot codec (ENGINE.md 11) -----------------------------------------

use crate::game::{Game, StackKind};
use crate::ids::Zone;

impl Game {
    /// `p`'s hand in canonical order: by definition, then card index.
    pub fn hand_sorted(&self, p: PlayerId) -> Vec<CardIdx> {
        let mut h = self.players[p as usize].hand.clone();
        h.sort_by_key(|c| (self.objs[c.i()].def, c.0));
        h
    }

    /// One (slot, card) per distinct definition in `p`'s hand: cards in a
    /// hidden zone have no other state, so equal definitions are equivalent.
    pub fn hand_slots(&self, p: PlayerId) -> Vec<(u16, CardIdx)> {
        let h = self.hand_sorted(p);
        let mut out = Vec::with_capacity(h.len());
        for (i, &c) in h.iter().enumerate() {
            if i > 0 && self.objs[h[i - 1].i()].def == self.objs[c.i()].def {
                continue;
            }
            assert!((i as u16) < act::HAND_N, "hand slot {i} out of range");
            out.push((act::HAND + i as u16, c));
        }
        out
    }

    /// Options "choose a card in `p`'s hand", one per distinct definition.
    pub(crate) fn hand_options(&self, p: PlayerId) -> Vec<Opt> {
        self.hand_slots(p).into_iter().map(|(a, c)| Opt { action: a, choice: Choice::Card(c) }).collect()
    }

    /// [`hand_options`](Self::hand_options) without one card (an ability's
    /// source in hand); a definition keeps its canonical slot.
    pub(crate) fn hand_options_except(&self, p: PlayerId, except: Option<CardIdx>) -> Vec<Opt> {
        let h = self.hand_sorted(p);
        let mut out = Vec::new();
        let mut i = 0;
        while i < h.len() {
            let def = self.objs[h[i].i()].def;
            let mut j = i;
            while j < h.len() && self.objs[h[j].i()].def == def {
                j += 1;
            }
            if let Some(&c) = h[i..j].iter().find(|&&c| Some(c) != except) {
                assert!((i as u16) < act::HAND_N, "hand slot {i} out of range");
                out.push(Opt { action: act::HAND + i as u16, choice: Choice::Card(c) });
            }
            i = j;
        }
        out
    }

    /// `p`'s library in canonical order: by definition, then card index.
    pub fn library_sorted(&self, p: PlayerId) -> Vec<CardIdx> {
        let mut l = self.players[p as usize].library.clone();
        l.sort_by_key(|c| (self.objs[c.i()].def, c.0));
        l
    }

    /// `MY_PERM` / `OPP_PERM` slot of a permanent, relative to `viewer`.
    pub fn perm_action(&self, viewer: PlayerId, idx: CardIdx) -> u16 {
        let ctl = self.objs[idx.i()].controller;
        let slot = self.permanents_of(ctl).position(|c| c == idx).expect("permanent on the battlefield") as u16;
        assert!(slot < act::PERM_N, "permanent slot {slot} out of range");
        if ctl == viewer {
            act::MY_PERM + slot
        } else {
            act::OPP_PERM + slot
        }
    }

    /// `STACK` slot (0 = top) of the stack item with this id.
    pub fn stack_action(&self, id: u32) -> u16 {
        let pos = self.stack.iter().position(|s| s.id == id).expect("stack item");
        let slot = (self.stack.len() - 1 - pos) as u16;
        assert!(slot < act::STACK_N, "stack slot {slot} out of range");
        act::STACK + slot
    }

    /// The action id of choosing target `t`, relative to `viewer`.
    pub fn target_action(&self, viewer: PlayerId, t: Target) -> u16 {
        match t {
            Target::Player(q) => act::PLAYER + (q != viewer) as u16,
            Target::Obj(r) => match self.objs[r.idx.i()].zone {
                Zone::Battlefield => self.perm_action(viewer, r.idx),
                Zone::Stack => {
                    let id = self
                        .stack
                        .iter()
                        .find(|s| s.kind == StackKind::Spell { card: r.idx })
                        .expect("spell on the stack")
                        .id;
                    self.stack_action(id)
                }
                Zone::Graveyard => self.graveyard_action(viewer, r.idx),
                z => panic!("no target slot for an object in {z:?}"),
            },
        }
    }

    /// `GRAVEYARD` slot of a card: its position in its owner's graveyard
    /// (0 = bottom), the decider's first (ENGINE.md 11).
    pub fn graveyard_action(&self, viewer: PlayerId, idx: CardIdx) -> u16 {
        let owner = self.objs[idx.i()].owner;
        let pos = self.players[owner as usize].graveyard.iter().position(|&c| c == idx).expect("card in a graveyard");
        assert!(pos < 64, "graveyard slot {pos} out of range");
        act::GRAVEYARD + if owner == viewer { 0 } else { 64 } + pos as u16
    }

    /// Human-readable description of an option (tests, logs).
    pub fn describe_choice(&self, c: Choice) -> String {
        match c {
            Choice::Pass => "pass".into(),
            Choice::Yes => "yes".into(),
            Choice::Number(n) => format!("{n}"),
            Choice::Card(i) => format!("{}#{}", self.name(i), i.0),
            Choice::Play(i) => format!("play {}#{}", self.name(i), i.0),
            Choice::Cast(i) => format!("cast {}#{}", self.name(i), i.0),
            Choice::Target(t) => format!("target {}", self.describe_target(t)),
            Choice::Trigger(k) => format!("trigger {k}"),
            Choice::Ability(i, slot) => format!("activate {}#{} {slot:?}", self.name(i), i.0),
            Choice::Def(d) => format!("a {}", self.db.def(d).name),
        }
    }
}
