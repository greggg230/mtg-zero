//! State-based actions (CR 704; ENGINE.md 9).

use crate::db::Counter;
use crate::decision::{Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{EndReason, Game, Outcome, Pos};
use crate::ids::{CardIdx, PlayerId, Zone};

impl Game {
    /// Legend-rule groups (704.5j): a player's legendary permanents sharing
    /// a name, two or more, in canonical order.
    fn legend_groups(&self) -> Vec<(PlayerId, Vec<CardIdx>)> {
        let mut out: Vec<(PlayerId, Vec<CardIdx>)> = Vec::new();
        for p in 0..2u8 {
            let legends: Vec<CardIdx> = self.permanents_of(p).filter(|&c| self.is_legendary(c)).collect();
            let mut done: Vec<bool> = vec![false; legends.len()];
            for i in 0..legends.len() {
                if done[i] {
                    continue;
                }
                let name = &self.def_of(legends[i]).name;
                let group: Vec<CardIdx> = legends
                    .iter()
                    .enumerate()
                    .filter(|&(j, &c)| j >= i && &self.def_of(c).name == name)
                    .map(|(j, &c)| {
                        done[j] = true;
                        c
                    })
                    .collect();
                if group.len() > 1 {
                    out.push((p, group));
                }
            }
        }
        out
    }

    /// Creatures that SBAs put into the graveyard: 704.5f (toughness <= 0)
    /// and 704.5g/h (lethal or deathtouch damage, "destroy").
    fn dying_creatures(&self) -> (Vec<CardIdx>, Vec<CardIdx>) {
        let mut zero = Vec::new();
        let mut destroyed = Vec::new();
        for &c in &self.battlefield {
            if !self.is_creature(c) {
                continue;
            }
            let t = self.toughness(c);
            let o = &self.objs[c.i()];
            if t <= 0 {
                zero.push(c);
            } else if (o.damage as i32 >= t || (o.deathtouched && o.damage > 0)) && !self.indestructible(c) {
                destroyed.push(c);
            }
        }
        (zero, destroyed)
    }

    /// A cheap test (no allocation) that no SBA applies: nobody lost, no
    /// creature dies, no player controls two legendary permanents with the
    /// same name.
    fn sba_possible(&self) -> bool {
        if self.players.iter().any(|p| p.life <= 0 || p.drew_from_empty) {
            return true;
        }
        if self.tokens > 0 && !self.stray_tokens().is_empty() {
            return true;
        }
        if !self.counter_pairs().is_empty() {
            return true;
        }
        for (i, &c) in self.battlefield.iter().enumerate() {
            if self.objs[c.i()].attached.is_some() && self.bad_attachments().contains(&c) {
                return true;
            }
            let ty = self.types_of(c);
            if ty & crate::db::types::CREATURE != 0 {
                let o = &self.objs[c.i()];
                let t = self.toughness(c);
                if t <= 0 || ((o.damage as i32 >= t || (o.deathtouched && o.damage > 0)) && !self.indestructible(c)) {
                    return true;
                }
            }
            if ty & crate::db::types::PLANESWALKER != 0 && self.counters_on(c, Counter::Loyalty) == 0 {
                return true;
            }
            if self.is_legendary(c) {
                let (ctl, name) = (self.objs[c.i()].controller, &self.def_of(c).name);
                if self.battlefield[i + 1..]
                    .iter()
                    .any(|&d| self.objs[d.i()].controller == ctl && &self.def_of(d).name == name)
                {
                    return true;
                }
            }
        }
        false
    }

    /// 704.5i: planeswalkers with loyalty 0 (CR 306.9).
    fn dead_walkers(&self) -> Vec<CardIdx> {
        self.battlefield
            .iter()
            .copied()
            .filter(|&c| self.is_type(c, crate::db::types::PLANESWALKER) && self.counters_on(c, Counter::Loyalty) == 0)
            .collect()
    }

    /// 704.5q: permanents with both +1/+1 and -1/-1 counters.
    fn counter_pairs(&self) -> Vec<CardIdx> {
        if self.counters.is_empty() {
            return Vec::new();
        }
        self.battlefield
            .iter()
            .copied()
            .filter(|&c| self.counters_on(c, Counter::P1P1) > 0 && self.counters_on(c, Counter::M1M1) > 0)
            .collect()
    }

    /// 704.5n: Equipment attached to an illegal permanent (gone, or not a
    /// creature).
    fn bad_attachments(&self) -> Vec<CardIdx> {
        self.battlefield
            .iter()
            .copied()
            .filter(|&c| {
                self.objs[c.i()].attached.is_some_and(|h| {
                    !self.live(h)
                        || self.objs[h.idx.i()].zone != Zone::Battlefield
                        || !self.is_creature(h.idx)
                        || self.protected_from(h.idx, &self.damage_source_of(c))
                })
            })
            .collect()
    }

    /// Tokens in a zone other than the battlefield (704.5d), in slot order.
    fn stray_tokens(&self) -> Vec<CardIdx> {
        if self.tokens == 0 {
            return Vec::new();
        }
        (0..self.objs.len())
            .map(|i| CardIdx(i as u16))
            .filter(|&c| {
                let z = self.objs[c.i()].zone;
                self.def_of(c).token && z != Zone::Battlefield && z != Zone::Nowhere
            })
            .collect()
    }

    /// Would an SBA happen now? (Cleanup, 514.3a.)
    pub(crate) fn sba_needed(&self) -> bool {
        let (z, d) = self.dying_creatures();
        !self.stray_tokens().is_empty()
            || !self.counter_pairs().is_empty()
            || !self.bad_attachments().is_empty()
            || self.players.iter().any(|p| p.life <= 0 || p.drew_from_empty)
            || !z.is_empty()
            || !d.is_empty()
            || !self.legend_groups().is_empty()
            || !self.dead_walkers().is_empty()
    }

    /// One SBA check as part of the priority loop (117.5): choices for the
    /// legend rule first, then every action at once (704.3); repeat while
    /// something happened; then put waiting triggers on the stack.
    pub(crate) fn sba_check(&mut self, mut keep: Vec<CardIdx>) {
        if let Some(Choice::Card(c)) = self.answer.take() {
            keep.push(c);
        }
        if keep.is_empty() && !self.sba_possible() {
            if !self.waiting.is_empty() {
                self.agenda.push(Task::SbaCheck { keep });
                self.agenda.push(Task::PutTriggers);
            }
            return;
        }
        let groups = self.legend_groups();
        for (p, g) in &groups {
            if !g.iter().any(|c| keep.contains(c)) {
                let opts: Vec<Opt> = g
                    .iter()
                    .map(|&c| Opt { action: self.perm_action(*p, c), choice: Choice::Card(c) })
                    .collect();
                self.ask(Task::SbaCheck { keep }, *p, DecisionKind::LegendRule, opts);
                return;
            }
        }
        let did = self.perform_sbas(&groups, &keep);
        if self.outcome.is_some() {
            return;
        }
        if !self.waiting.is_empty() {
            self.agenda.push(Task::SbaCheck { keep: Vec::new() });
            self.agenda.push(Task::PutTriggers);
        } else if did {
            self.agenda.push(Task::SbaCheck { keep: Vec::new() });
        }
    }

    fn perform_sbas(&mut self, groups: &[(PlayerId, Vec<CardIdx>)], keep: &[CardIdx]) -> bool {
        // 704.5a, 704.5b: losing the game.
        let lost: Vec<bool> = self.players.iter().map(|p| p.life <= 0 || p.drew_from_empty).collect();
        for p in &mut self.players {
            p.drew_from_empty = false;
        }
        if lost.iter().any(|&l| l) {
            for (i, &l) in lost.iter().enumerate() {
                self.players[i].lost = l;
            }
            // CR 104.4a: all players lose at once -> draw.
            let winner = match (lost[0], lost[1]) {
                (true, true) => None,
                (true, false) => Some(1),
                (false, true) => Some(0),
                _ => unreachable!(),
            };
            self.note(|_| format!("game over: winner {winner:?}"));
            self.outcome = Some(Outcome { winner, reason: EndReason::Sba });
            return true;
        }
        let (zero, destroyed) = self.dying_creatures();
        // 704.5q and 704.5n happen at the same time as the rest.
        let pairs = self.counter_pairs();
        for &c in &pairs {
            let n = self.counters_on(c, Counter::P1P1).min(self.counters_on(c, Counter::M1M1));
            self.remove_counters(c, Counter::P1P1, n);
            self.remove_counters(c, Counter::M1M1, n);
        }
        let unattach = self.bad_attachments();
        for &c in &unattach {
            self.objs[c.i()].attached = None;
        }
        let mut legend_out: Vec<CardIdx> = Vec::new();
        for (_, g) in groups {
            legend_out.extend(g.iter().copied().filter(|c| !keep.contains(c)));
        }
        let mut all: Vec<CardIdx> = Vec::new();
        // 704.5i: a planeswalker with loyalty 0 goes to its owner's
        // graveyard (not destroyed: indestructible doesn't stop it).
        let walkers = self.dead_walkers();
        for c in zero.into_iter().chain(walkers.iter().copied()).chain(legend_out).chain(destroyed.iter().copied()) {
            if !all.contains(&c) {
                all.push(c);
            }
        }
        // Battlefield order, so simultaneous events are listed canonically.
        all.sort_by_key(|c| self.objs[c.i()].timestamp);
        self.begin_batch();
        for &c in &all {
            if destroyed.contains(&c) && !walkers.contains(&c) {
                self.destroy(c);
            } else {
                self.move_card(c, Zone::Graveyard, Pos::Top);
            }
        }
        self.end_batch();
        // 704.5d: a token outside the battlefield ceases to exist.
        let stray = self.stray_tokens();
        for &c in &stray {
            self.note(|g| format!("{} token ceases to exist", g.name(c)));
            self.move_card(c, Zone::Nowhere, Pos::Top);
        }
        for &c in &self.battlefield {
            self.objs[c.i()].deathtouched = false;
        }
        !all.is_empty() || !stray.is_empty() || !pairs.is_empty() || !unattach.is_empty()
    }
}
