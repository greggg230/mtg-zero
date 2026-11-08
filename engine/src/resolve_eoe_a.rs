//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): station (CR
//! 702.184, 722), counters, artifacts, Equipment and Auras, as the engine
//! resolves them: `Amount::TappedPower` and the `Ext::EoeA` instructions.

use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, PlayerId, Zone};
use crate::program::{EoeA, On};
use crate::resolve::{Flow, Frame};

impl Game {
    /// `Amount::TappedPower`: the power of the creature tapped to pay the
    /// resolving ability's cost (its first noted object, `StackItem::noted`),
    /// now if it is still on the battlefield, else as it last existed there
    /// (CR 702.184a, 608.2h; station's ruling: "use the tapped creature's
    /// power as the station ability resolves"). None noted: 0.
    pub(crate) fn tapped_power(&self, f: &Frame) -> i32 {
        let Some(&r) = f.noted.first() else { return 0 };
        if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield {
            return self.power(r.idx);
        }
        match self.objs[r.idx.i()].lki_power {
            Some((inc, p)) if inc == r.inc => p,
            _ => 0,
        }
    }

    /// Could Aura `aura` (its controller `p`) enchant permanent `h` (CR
    /// 303.4a, 702.5a): what its enchant ability allows (its spell's target
    /// filter, "you" its controller), not itself, not protected from it
    /// (702.16c)?
    pub(crate) fn aura_enchants(&self, aura: CardIdx, p: PlayerId, h: CardIdx) -> bool {
        let Some(req) = self.def_of(aura).spell.as_ref().and_then(|s| s.targets.first()) else { return false };
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(aura)) };
        h != aura
            && self.objs[h.i()].zone == Zone::Battlefield
            && self.card_matches(&req.filter, h, &ctx)
            && !self.protected_from(h, &self.damage_source_of(aura))
    }

    /// CR 704.5m for an Aura that enchants a permanent: attached to nothing,
    /// to an illegal object, or itself a creature (303.4d), it is put into
    /// its owner's graveyard.
    pub(crate) fn bad_aura(&self, c: CardIdx) -> bool {
        let Some(h) = self.objs[c.i()].attached else { return true };
        !self.live(h) || self.is_creature(c) || !self.aura_enchants(c, self.objs[c.i()].controller, h.idx)
    }

    pub(crate) fn exec_eoe_a(&mut self, f: &mut Frame, op: &EoeA) -> Flow {
        match op {
            EoeA::OwnerPutsTopOrBottom { tgt } => {
                let Some(&c) = self.objs_on(f, &On::Target(*tgt)).first() else { return Flow::Next };
                let owner = self.objs[c.i()].owner;
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        self.note(|g| format!("P{owner} puts {} on top of their library", g.name(c)));
                        self.move_card(c, Zone::Library, Pos::Top);
                    }
                    Some(_) => {
                        self.note(|g| format!("P{owner} puts {} on the bottom of their library", g.name(c)));
                        self.move_card(c, Zone::Library, Pos::Bottom);
                    }
                    None => {
                        let opts = vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                        self.ask(Task::Run(f.clone()), owner, DecisionKind::YesNo, opts);
                        return Flow::Suspend;
                    }
                }
                Flow::Next
            }
            EoeA::DestroyEventObj => {
                // "Destroy it": the event's object, if it is still that
                // permanent (CR 701.8a).
                if let Some(r) = f.ev.obj {
                    if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield {
                        self.destroy(r.idx);
                    }
                }
                Flow::Next
            }
            EoeA::MarkPile => {
                f.x = f.pile.len() as i32;
                Flow::Next
            }
            EoeA::CounterOnSourceUnlessPileShrank { counter } => {
                if f.pile.len() as i32 >= f.x {
                    for c in self.objs_on(f, &On::Source) {
                        self.add_counters(c, *counter, 1);
                    }
                }
                Flow::Next
            }
            EoeA::PutSourceCountersOnTarget { tgt } => {
                // The source's counters: on it now, or as it last existed on
                // the battlefield (its last-known counters, CR 608.2h).
                let src = f.source;
                let counters: Vec<(crate::db::Counter, crate::num::Count)> =
                    if self.live(src) && self.objs[src.idx.i()].zone == Zone::Battlefield {
                        self.counters_of(src.idx)
                    } else {
                        let mut v: Vec<(crate::db::Counter, crate::num::Count)> =
                            self.lki_counters.iter().filter(|x| x.0 == src && x.2 > 0).map(|x| (x.1, x.2)).collect();
                        v.sort();
                        v
                    };
                let Some(&c) = self.objs_on(f, &On::Target(*tgt)).first() else { return Flow::Next };
                for (k, n) in counters {
                    self.add_counters(c, k, n);
                }
                Flow::Next
            }
        }
    }
}
