//! Edge of Eternities, W6 (ENGINE.md 99.16): the rules the set's remaining
//! cards need. Named after the rules actions, each citing its rule; never
//! after a card.

use crate::game::Game;
use crate::ids::{Target, Zone};
use crate::resolve::{Flow, Frame};

/// The printing facts W6 adds (`CardDef::w6`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W6Def {
    /// Its `SEEN_W6_*` bits (`seen_of`).
    pub seen: u16,
    /// One of its abilities reads the amount of mana spent to cast another
    /// spell (CR 601.2h; "if the amount of mana spent to cast that spell was
    /// less than its mana value"): every spell's payment is recorded
    /// (`Mark::ManaSpent`) in a table with such a card.
    pub reads_spells_mana_spent: bool,
    /// "If one or more counters would be put on a <filter> you control,
    /// twice that many of each of those kinds of counters are put on it
    /// instead" (CR 614.1a, 122.6): the recipients (the static ability's
    /// controller controls them).
    pub counter_doubler: Option<crate::db::Filter>,
}

/// The table facts W6 adds (`CardDb::w6`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W6Db {
    /// A card of the table reads the mana spent to cast any spell: every
    /// payment for a spell is recorded (CR 601.2h).
    pub mana_spent_all: bool,
}

impl W6Db {
    pub(crate) fn of(defs: &[crate::db::CardDef]) -> W6Db {
        W6Db { mana_spent_all: defs.iter().any(|d| d.w6.reads_spells_mana_spent) }
    }
}

/// W6's game state (`Game::w6`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct W6State {
    /// `SEEN_W6_*` bits: a card of this game needs the rule (as
    /// `Game::seen`).
    pub seen: u16,
    /// The permanent being put onto the battlefield whose "enters with"
    /// counters are being put on it (CR 122.6, 614.1c), with the first
    /// timestamp of the entries of its event (what entered at the same time:
    /// `Game::group_ts`): set and cleared around its entry.
    pub entering: Option<(crate::ids::CardIdx, u32)>,
    /// The controller of the spell or ability whose instruction is being
    /// carried out (`Game::run_frame`): the player who puts counters its
    /// effect puts (CR 122.6, 608.2): set and restored around each op.
    pub actor: Option<crate::ids::PlayerId>,
}

/// `W6State::seen`: a replacement effect multiplies counters put on
/// permanents (`W6Def::counter_doubler`).
pub(crate) const SEEN_W6_COUNTERS: u16 = 1;
/// `W6State::seen`: a triggered ability reads who put +1/+1 counters and
/// how many (`TrigPred::CountersPutBy`): the `CountersAdded` event carries
/// them.
pub(crate) const SEEN_W6_PUTTER: u16 = 2;

/// The abilities of a definition that go on the stack (its spell, triggered
/// and activated abilities).
fn abilities_of(d: &crate::db::CardDef) -> impl Iterator<Item = &crate::db::AbilityDef> {
    d.triggers.iter().map(|t| &t.ability).chain(d.spell.iter()).chain(d.activated.iter().filter_map(|a| match &a.effect {
        crate::db::ActEffect::Stack(ab) => Some(ab),
        crate::db::ActEffect::Mana(_) => None,
    }))
}

/// The facts W6 reads off a lowered definition's programs (`W6Def`), after
/// its printing is lowered.
pub(crate) fn finish_def(d: &mut crate::db::CardDef) {
    let mut reads = false;
    for ab in abilities_of(d) {
        for op in &ab.program {
            if let crate::program::Op::Ext(crate::program::Ext::EoeW6(W6::JumpUnlessSpentBelowMv { .. })) = op {
                reads = true;
            }
        }
    }
    d.w6.reads_spells_mana_spent = reads;
    d.w6.seen = seen_of(d);
}

/// The `SEEN_W6_*` bits of a definition.
pub(crate) fn seen_of(d: &crate::db::CardDef) -> u16 {
    let mut bits = 0;
    if d.w6.counter_doubler.is_some() {
        bits |= SEEN_W6_COUNTERS;
    }
    bits
}

/// W6's instructions (`Ext::EoeW6`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum W6 {
    /// Jump unless the amount of mana spent to cast target spell `tgt` was
    /// less than its mana value (CR 601.2h, 202.3; an {X} spell's mana value
    /// counts its X, 202.3e). Read before the spell is countered: the values
    /// are those of the spell as it last existed on the stack (608.2h), the
    /// same before and after it leaves. A copy of a spell wasn't cast: no
    /// mana was spent (707.10).
    JumpUnlessSpentBelowMv { tgt: u8, to: u16 },
}

impl W6 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            W6::JumpUnlessSpentBelowMv { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            W6::JumpUnlessSpentBelowMv { to, .. } => Some(to),
        }
    }
}

impl Game {
    /// `n` counters of a kind are about to be put on permanent `c` (CR
    /// 122.6): how many are put once the replacement effects that multiply
    /// them apply (CR 614.1a; Loading Zone). Each such effect of a
    /// permanent whose controller controls `c` and whose recipients match
    /// `c` doubles them: two give four times as many, and so on (its
    /// ruling of 2025-07-25). The affected object's controller would order
    /// them (616.1), but doublings commute, and the lowering refuses any
    /// other counter-modifying replacement: no order to choose. Counters a
    /// permanent enters with (122.6) get only the effects that already
    /// exist (614.12): not those of permanents entering at the same time.
    pub(crate) fn w6_counters_put(&self, c: crate::ids::CardIdx, n: crate::num::Count) -> crate::num::Count {
        if self.objs[c.i()].zone != Zone::Battlefield {
            return n;
        }
        let entering = self.w6.entering.filter(|e| e.0 == c).map(|e| e.1);
        let ctl = self.objs[c.i()].controller;
        let mut n = n;
        for &d in &self.battlefield {
            let Some(f) = &self.abilities(d).w6.counter_doubler else { continue };
            if self.objs[d.i()].controller != ctl || entering.is_some_and(|since| self.objs[d.i()].timestamp >= since) {
                continue;
            }
            let ctx = crate::cast::Ctx { controller: ctl, source: Some(self.obj_ref(d)) };
            if self.card_matches(f, c, &ctx) {
                n = n.saturating_mul(2);
            }
        }
        n
    }

    /// The player who puts counters on permanent `c` now (CR 122.6): for
    /// counters it enters with, its controller (122.6a: no effect of the
    /// table names another player); else the controller of the spell or
    /// ability whose instruction puts them (608.2).
    pub(crate) fn w6_putter(&self, c: crate::ids::CardIdx) -> crate::ids::PlayerId {
        if self.w6.entering.is_some_and(|e| e.0 == c) {
            return self.objs[c.i()].controller;
        }
        self.w6.actor.unwrap_or_else(|| panic!("counters put on {} outside a spell or ability (EoE W6: who puts them?)", self.name(c)))
    }

    pub(crate) fn exec_eoe_w6(&mut self, f: &mut Frame, op: &W6) -> Flow {
        match op {
            W6::JumpUnlessSpentBelowMv { tgt, to } => {
                let below = self.targets_of(f, *tgt).into_iter().any(|t| match t {
                    Target::Obj(o) if self.live(o) && self.objs[o.idx.i()].zone == Zone::Stack => {
                        (self.mana_spent_of(o) as u32) < self.mana_value_of(o.idx) as u32
                    }
                    _ => false,
                });
                if below {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
        }
    }
}
