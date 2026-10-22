//! Chance keys, knowledge and `determinize` (ENGINE.md 10).

use crate::game::Game;
use crate::ids::{CardIdx, DefId, PlayerId};
use crate::rng::Rng;

/// A card was drawn (private to the drawer). Payload: its definition.
pub const KEY_DRAW: u64 = 1;
/// A card moved from a hidden zone to a public one. Payload: its definition.
pub const KEY_REVEAL: u64 = 2;
/// Payload of a private key emitted while another player is the observer.
pub const HIDDEN: u64 = 0xFFFF_FFFF;

/// Pack a key: kind in bits 56-63, player in 48-55, payload in 0-47.
pub fn key(kind: u64, player: PlayerId, payload: u64) -> u64 {
    debug_assert!(payload < 1 << 48);
    (kind << 56) | ((player as u64) << 48) | payload
}

pub fn key_kind(k: u64) -> u64 {
    k >> 56
}
pub fn key_player(k: u64) -> PlayerId {
    ((k >> 48) & 0xFF) as PlayerId
}
pub fn key_payload(k: u64) -> u64 {
    k & ((1 << 48) - 1)
}

impl Game {
    /// A fact revealed to everyone.
    pub(crate) fn emit_public(&mut self, kind: u64, player: PlayerId, payload: u64) {
        self.keys.push(key(kind, player, payload));
    }

    /// A fact revealed only to `to`: hidden from any other observer.
    pub(crate) fn emit_private(&mut self, kind: u64, to: PlayerId, payload: u64) {
        let payload = match self.observer {
            Some(o) if o != to => HIDDEN,
            _ => payload,
        };
        self.keys.push(key(kind, to, payload));
    }

    /// Does `viewer` know the identity of this card?
    pub fn knows(&self, viewer: PlayerId, idx: CardIdx) -> bool {
        let o = &self.objs[idx.i()];
        !o.zone.is_hidden() || o.known & (1 << viewer) != 0
    }

    /// Cards whose identity `viewer` does not know, per owner: the other
    /// player's hand and both libraries, minus known cards.
    pub fn unknown_to(&self, viewer: PlayerId, owner: PlayerId) -> Vec<CardIdx> {
        let pl = &self.players[owner as usize];
        let hand = if owner != viewer { &pl.hand[..] } else { &[][..] };
        hand.iter().chain(pl.library.iter()).copied().filter(|&c| !self.knows(viewer, c)).collect()
    }

    /// Resample everything hidden from `viewer` (ENGINE.md 10): the
    /// identities of each owner's unknown cards are shuffled among
    /// themselves; the state's RNG is reseeded; later private keys are
    /// masked for `viewer`; the pending decision is recomputed.
    pub fn determinize(&mut self, viewer: PlayerId, rng: &mut Rng) {
        let payer = self.payment_in_progress();
        for owner in 0..2u8 {
            let mut slots = self.unknown_to(viewer, owner);
            if payer == Some(owner) && owner != viewer {
                // A payment in progress may count on the payer's mana
                // sources in hand: they are pinned (ENGINE.md 10).
                slots.retain(|&c| !self.hand_mana_source(c));
            }
            let mut defs: Vec<DefId> = slots.iter().map(|c| self.objs[c.i()].def).collect();
            rng.shuffle(&mut defs);
            for (c, d) in slots.iter().zip(defs) {
                self.objs[c.i()].def = d;
            }
        }
        self.rng = Rng::new(rng.next_u64());
        self.observer = Some(viewer);
        // The viewer's own options never name a card whose identity was
        // resampled (rule: a choice of a card in a zone hidden from its
        // decider names its definition, not its slot; the decider's hand is
        // never resampled for them). Only another player's decision needs
        // recomputing. Debug builds check the rule exactly.
        if self.pending.as_ref().is_some_and(|d| d.player != viewer) {
            self.refresh_pending();
        } else if cfg!(debug_assertions) && self.pending.is_some() {
            let mut c = self.clone();
            c.refresh_pending();
            assert_eq!(c.pending, self.pending, "determinize changed the viewer's decision");
        }
    }

    /// The player paying a cost (or casting or activating something whose
    /// cost is still to be paid), if any: from the cast or activation until
    /// its mana is paid, and through a "pay {X}?" payment during
    /// resolution.
    pub(crate) fn payment_in_progress(&self) -> Option<PlayerId> {
        use crate::flow::Task;
        self.agenda.iter().rev().find_map(|t| match t {
            Task::PayMana(pm) => Some(pm.payer),
            Task::CastPay { id } | Task::ActPay { id } => {
                self.item_index(*id).map(|i| self.stack[i].controller)
            }
            _ => None,
        })
    }

    /// Re-run the asker of the pending decision so its options reflect the
    /// current (resampled) state. Never auto-answers.
    pub(crate) fn refresh_pending(&mut self) {
        if self.pending.take().is_some() {
            self.no_auto = true;
            self.advance();
            self.no_auto = false;
            debug_assert!(self.keys.is_empty(), "refreshing a decision revealed something");
            self.keys.clear();
        }
    }
}
