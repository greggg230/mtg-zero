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
                // (Its name as a copy may have it, CR 707.9b.)
                let name = self.name(legends[i]);
                let group: Vec<CardIdx> = legends
                    .iter()
                    .enumerate()
                    .filter(|&(j, &c)| j >= i && self.name(c) == name)
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
            } else if (o.damage as i64 >= t as i64 || (o.deathtouched && o.damage > 0)) && !self.indestructible(c) {
                destroyed.push(c);
            }
        }
        (zero, destroyed)
    }

    /// A cheap test (no allocation) that no SBA applies: nobody lost, no
    /// creature dies, no player controls two legendary permanents with the
    /// same name.
    fn sba_possible(&self) -> bool {
        if self.players.iter().any(|p| p.life <= 0 || p.drew_from_empty || p.poison >= 10) {
            return true;
        }
        // 704.5t: a dungeon whose venture marker is on its bottommost room.
        if self.dungeons.iter().any(|d| d.obj.is_some() && d.room == crate::dungeon::BOTTOM) && !self.finished_dungeons().is_empty() {
            return true;
        }
        if self.tokens > 0 && self.token_range().any(|c| self.stray_token(c)) {
            return true;
        }
        if !self.counters.is_empty() && self.battlefield.iter().any(|&c| self.counter_pair(c)) {
            return true;
        }
        for (i, &c) in self.battlefield.iter().enumerate() {
            if (self.objs[c.i()].attached.is_some() || self.def_of(c).bestowed || self.def_of(c).reanimates) && self.bad_attachment(c) {
                return true;
            }
            let ty = self.types_of(c);
            if ty & crate::db::types::CREATURE != 0 {
                let o = &self.objs[c.i()];
                let t = self.toughness(c);
                if t <= 0 || ((o.damage as i64 >= t as i64 || (o.deathtouched && o.damage > 0)) && !self.indestructible(c)) {
                    return true;
                }
            }
            if ty & crate::db::types::PLANESWALKER != 0 && self.counters_on(c, Counter::Loyalty) == 0 {
                return true;
            }
            if ty & crate::db::types::ENCHANTMENT != 0 {
                let ch = self.abilities(c).chapters as crate::num::Count;
                if ch > 0 && self.counters_on(c, Counter::Lore) >= ch && self.finished_sagas().contains(&c) {
                    return true;
                }
            }
            if self.is_legendary(c) {
                let (ctl, name) = (self.objs[c.i()].controller, self.name(c));
                if self.battlefield[i + 1..]
                    .iter()
                    .any(|&d| self.objs[d.i()].controller == ctl && self.is_legendary(d) && self.name(d) == name)
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
        self.battlefield.iter().copied().filter(|&c| self.counter_pair(c)).collect()
    }

    fn counter_pair(&self, c: CardIdx) -> bool {
        self.counters_on(c, Counter::P1P1) > 0 && self.counters_on(c, Counter::M1M1) > 0
    }

    /// 704.5n: Equipment attached to an illegal permanent (gone, or not a
    /// creature); 702.103f: a bestowed Aura attached to an illegal object or
    /// to none.
    fn bad_attachments(&self) -> Vec<CardIdx> {
        self.battlefield.iter().copied().filter(|&c| self.bad_attachment(c)).collect()
    }

    fn bad_attachment(&self, c: CardIdx) -> bool {
        if self.def_of(c).reanimates {
            return self.bad_reanimating_aura(c);
        }
        let illegal = |h: crate::ids::ObjRef| {
            !self.live(h)
                || self.objs[h.idx.i()].zone != Zone::Battlefield
                || !self.is_creature(h.idx)
                || self.protected_from(h.idx, &self.damage_source_of(c))
        };
        // 704.5p: a creature, or a permanent that is no longer an
        // Equipment (a copy effect ended), becomes unattached.
        let attachable = || {
            !self.is_creature(c)
                && (self.def_of(c).bestowed || self.db.equipment_subtype().is_some_and(|s| self.has_subtype(c, s)))
        };
        match self.objs[c.i()].attached {
            Some(h) => illegal(h) || !attachable(),
            None => self.def_of(c).bestowed,
        }
    }

    /// CR 704.5m for a reanimating Aura (once it is an Aura): it enchants the
    /// creature card in a graveyard it entered attached to (Animate Dead,
    /// until its ability resolves) or the creature it put onto the
    /// battlefield; anything else, or nothing, is illegal.
    fn bad_reanimating_aura(&self, c: CardIdx) -> bool {
        let aura = self.db.subtype_id("Aura").is_some_and(|s| self.has_subtype(c, s));
        if !aura {
            return false;
        }
        let Some(h) = self.objs[c.i()].attached else { return true };
        if !self.live(h) {
            return true;
        }
        match self.objs[h.idx.i()].zone {
            Zone::Graveyard => !self.def_of(h.idx).is(crate::db::types::CREATURE),
            Zone::Battlefield => {
                let r = self.obj_ref(c);
                !self.is_creature(h.idx)
                    || !self.marks.contains(&(r, crate::game::Mark::Reanimated(h)))
                    || self.protected_from(h.idx, &self.damage_source_of(c))
            }
            _ => true,
        }
    }

    /// Tokens in a zone other than the battlefield (704.5d), in slot order.
    fn stray_tokens(&self) -> Vec<CardIdx> {
        if self.tokens == 0 {
            return Vec::new();
        }
        self.token_range().filter(|&c| self.stray_token(c)).collect()
    }

    /// The slots that may be tokens or copies (from the first one made;
    /// cards are never tokens).
    fn token_range(&self) -> impl Iterator<Item = CardIdx> {
        (self.first_token as usize..self.objs.len()).map(|i| CardIdx(i as u16))
    }

    fn stray_token(&self, c: CardIdx) -> bool {
        // (704.5e: a copy of a spell or of a card is a token here; one
        // leaving the stack ceases to exist at once, `move_card`.)
        let z = self.objs[c.i()].zone;
        z != Zone::Battlefield
            && z != Zone::Nowhere
            && z != Zone::Stack
            && self.is_token(c)
            && !(z == Zone::Exile && self.prepare_copy_live(c))
    }

    /// Would an SBA happen now? (Cleanup, 514.3a.)
    pub(crate) fn sba_needed(&self) -> bool {
        let (z, d) = self.dying_creatures();
        !self.stray_tokens().is_empty()
            || !self.counter_pairs().is_empty()
            || !self.bad_attachments().is_empty()
            || self.players.iter().any(|p| p.life <= 0 || p.drew_from_empty || p.poison >= 10)
            || !z.is_empty()
            || !d.is_empty()
            || !self.legend_groups().is_empty()
            || !self.dead_walkers().is_empty()
            || !self.finished_sagas().is_empty()
            || !self.finished_dungeons().is_empty()
    }

    /// One SBA check as part of the priority loop (117.5): choices for the
    /// legend rule first, then every action at once (704.3); repeat while
    /// something happened; then put waiting triggers on the stack.
    pub(crate) fn sba_check(&mut self, mut keep: Vec<CardIdx>) {
        if let Some(Choice::Card(c)) = self.answer.take() {
            keep.push(c);
        }
        // Miracle (CR 702.94a): the reveal question about a first card drawn
        // this turn, before anyone gets priority.
        if let Some((p, card)) = self.miracle_check.take() {
            self.agenda.push(Task::SbaCheck { keep });
            self.agenda.push(Task::MiracleReveal { p, card });
            return;
        }
        // Ascend (CR 702.131b): the city's blessing, for the rest of the
        // game, once a player with an ascend permanent controls ten (a
        // static ability: noted here, before anyone gets priority).
        let ascend = self.seen & crate::game::SEEN_ASCEND != 0 && self.battlefield.iter().any(|&c| self.db.def(self.objs[c.i()].def).ascend);
        for p in 0..2u8 {
            if ascend && !self.players[p as usize].city_blessing && self.ascends(p) {
                self.players[p as usize].city_blessing = true;
                self.note(|_| format!("P{p} gets the city's blessing"));
            }
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
        // 704.5a, 704.5b, 704.5c: losing the game.
        let lost: Vec<bool> = self.players.iter().map(|p| p.life <= 0 || p.drew_from_empty || p.poison >= 10).collect();
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
        // 704.5t: completed dungeons leave the game (at the same time).
        let dungeons = self.finished_dungeons();
        self.remove_finished_dungeons(&dungeons);
        // 704.5q and 704.5n happen at the same time as the rest.
        let pairs = self.counter_pairs();
        for &c in &pairs {
            let n = self.counters_on(c, Counter::P1P1).min(self.counters_on(c, Counter::M1M1));
            self.remove_counters(c, Counter::P1P1, n);
            self.remove_counters(c, Counter::M1M1, n);
        }
        let unattach = self.bad_attachments();
        // 704.5m: an illegal reanimating Aura goes to its owner's graveyard.
        let (dead_auras, unattach): (Vec<CardIdx>, Vec<CardIdx>) = unattach.into_iter().partition(|&c| self.def_of(c).reanimates);
        for &c in &unattach {
            self.objs[c.i()].attached = None;
            // CR 702.103f: an unattached bestowed Aura ceases to be bestowed:
            // it is its card again (an enchantment creature; an exception to
            // 704.5m).
            if self.def_of(c).bestowed {
                let front = self.def_of(c).card_def(self.objs[c.i()].def);
                self.set_face(c, front);
                self.note(|g| format!("{} is no longer bestowed", g.name(c)));
            }
        }
        let mut legend_out: Vec<CardIdx> = Vec::new();
        for (_, g) in groups {
            legend_out.extend(g.iter().copied().filter(|c| !keep.contains(c)));
        }
        let mut all: Vec<CardIdx> = Vec::new();
        // 704.5i: a planeswalker with loyalty 0 goes to its owner's
        // graveyard (not destroyed: indestructible doesn't stop it).
        let walkers = self.dead_walkers();
        // 704.5s: a Saga past its final chapter is sacrificed.
        let sagas = self.finished_sagas();
        for c in zero
            .into_iter()
            .chain(walkers.iter().copied())
            .chain(legend_out)
            .chain(destroyed.iter().copied())
            .chain(sagas.iter().copied())
            .chain(dead_auras.iter().copied())
        {
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
            } else if sagas.contains(&c) && self.objs[c.i()].zone == Zone::Battlefield {
                self.sacrifice(c);
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
        // (Prepared designations whose permanent or copy is gone.)
        if !self.prepared.is_empty() {
            let objs = &self.objs;
            self.prepared.retain(|&(s, c)| objs[s.idx.i()].inc == s.inc && objs[s.idx.i()].zone == Zone::Battlefield && objs[c.i()].zone == Zone::Exile);
        }
        for &c in &self.battlefield {
            self.objs[c.i()].deathtouched = false;
        }
        !all.is_empty() || !stray.is_empty() || !pairs.is_empty() || !unattach.is_empty() || !dungeons.is_empty()
    }
}
