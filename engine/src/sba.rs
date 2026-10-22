//! State-based actions (CR 704; ENGINE.md 9).

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

    /// Would an SBA happen now? (Cleanup, 514.3a.)
    pub(crate) fn sba_needed(&self) -> bool {
        let (z, d) = self.dying_creatures();
        self.players.iter().any(|p| p.life <= 0 || p.drew_from_empty)
            || !z.is_empty()
            || !d.is_empty()
            || !self.legend_groups().is_empty()
    }

    /// One SBA check as part of the priority loop (117.5): choices for the
    /// legend rule first, then every action at once (704.3); repeat while
    /// something happened; then put waiting triggers on the stack.
    pub(crate) fn sba_check(&mut self, mut keep: Vec<CardIdx>) {
        if let Some(Choice::Card(c)) = self.answer.take() {
            keep.push(c);
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
        let mut legend_out: Vec<CardIdx> = Vec::new();
        for (_, g) in groups {
            legend_out.extend(g.iter().copied().filter(|c| !keep.contains(c)));
        }
        let mut all: Vec<CardIdx> = Vec::new();
        for c in zero.into_iter().chain(legend_out).chain(destroyed.iter().copied()) {
            if !all.contains(&c) {
                all.push(c);
            }
        }
        // Battlefield order, so simultaneous events are listed canonically.
        all.sort_by_key(|c| self.objs[c.i()].timestamp);
        for &c in &all {
            if destroyed.contains(&c) {
                self.destroy(c);
            } else {
                self.move_card(c, Zone::Graveyard, Pos::Top);
            }
        }
        for &c in &self.battlefield {
            self.objs[c.i()].deathtouched = false;
        }
        !all.is_empty()
    }
}
