//! Combat (CR 506-511; ENGINE.md 12).

use crate::db::Kw;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::Game;
use crate::ids::{opp, CardIdx, Target};

impl Game {
    /// Attacking creatures in canonical order (the active player's slots).
    pub fn attackers(&self) -> Vec<CardIdx> {
        self.permanents_of(self.turn.active).filter(|&c| self.objs[c.i()].attacking).collect()
    }

    /// Creatures blocking `att`, in canonical order.
    pub fn blockers_of(&self, att: CardIdx) -> Vec<CardIdx> {
        self.battlefield.iter().copied().filter(|&c| self.objs[c.i()].blocking == Some(att)).collect()
    }

    /// CR 508.1a: untapped, no summoning sickness unless haste, no defender.
    pub(crate) fn can_attack(&self, c: CardIdx) -> bool {
        self.is_creature(c)
            && !self.objs[c.i()].tapped
            && (!self.is_sick(c) || self.has_kw(c, Kw::Haste))
            && !self.has_kw(c, Kw::Defender)
    }

    /// CR 509.1a-b: untapped; flying attackers need flying or reach.
    pub(crate) fn can_block(&self, b: CardIdx, a: CardIdx) -> bool {
        !self.has_kw(a, Kw::Flying) || self.has_kw(b, Kw::Flying) || self.has_kw(b, Kw::Reach)
    }

    pub(crate) fn potential_blockers(&self) -> Vec<CardIdx> {
        let dp = opp(self.turn.active);
        self.creatures_of(dp).filter(|&c| !self.objs[c.i()].tapped).collect()
    }

    /// Declare attackers (CR 508.1): one creature per decision in increasing
    /// slot order; `PASS` finishes. Each attacking set is reachable once.
    pub(crate) fn declare_attackers(&mut self, next: u8) {
        let ap = self.turn.active;
        match self.answer.take() {
            Some(Choice::Card(c)) => {
                self.objs[c.i()].attacking = true;
                let slot = self.perm_action(ap, c) - act::MY_PERM;
                self.note(|g| format!("attacks with {}", g.name(c)));
                self.agenda.push(Task::DeclareAttackers { next: slot as u8 + 1 });
                return;
            }
            Some(Choice::Pass) => {
                // CR 508.1f: attackers without vigilance tap.
                for c in self.attackers() {
                    if !self.has_kw(c, Kw::Vigilance) {
                        self.tap(c);
                    }
                }
                return;
            }
            Some(other) => unreachable!("attack answer {other:?}"),
            None => {}
        }
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        for c in self.permanents_of(ap).collect::<Vec<_>>() {
            let a = self.perm_action(ap, c);
            if a - act::MY_PERM >= next as u16 && !self.objs[c.i()].attacking && self.can_attack(c) {
                opts.push(Opt { action: a, choice: Choice::Card(c) });
            }
        }
        self.ask(Task::DeclareAttackers { next }, ap, DecisionKind::DeclareAttackers, opts);
    }

    /// Declare blockers (CR 509.1): for each potential blocker in slot order,
    /// which attacker it blocks (`OPP_PERM` slot) or `PASS`. Options that
    /// would make the declaration illegal (menace) are not offered.
    pub(crate) fn declare_blockers(&mut self, i: u8) {
        let dp = opp(self.turn.active);
        let pot = self.potential_blockers();
        if let Some(ch) = self.answer.take() {
            let b = pot[i as usize];
            if let Choice::Card(a) = ch {
                self.objs[b.i()].blocking = Some(a);
                self.note(|g| format!("{} blocks {}", g.name(b), g.name(a)));
            }
            self.agenda.push(Task::DeclareBlockers { i: i + 1 });
            return;
        }
        let Some(&b) = pot.get(i as usize) else {
            // CR 509.1h: an attacker with blockers becomes blocked.
            for a in self.attackers() {
                let blocked = !self.blockers_of(a).is_empty();
                self.objs[a.i()].blocked = blocked;
            }
            return;
        };
        let rest = &pot[i as usize + 1..];
        let mut opts = Vec::new();
        if self.blocks_completable(None, rest) {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        for a in self.attackers() {
            if self.can_block(b, a) && self.blocks_completable(Some((b, a)), rest) {
                opts.push(Opt { action: self.perm_action(dp, a), choice: Choice::Card(a) });
            }
        }
        self.ask(Task::DeclareBlockers { i }, dp, DecisionKind::DeclareBlockers, opts);
    }

    /// With the current blocks plus `extra`, can the creatures in `rest`
    /// still make the declaration legal? Menace (702.110b): every attacker
    /// with menace has zero or at least two blockers. A matching of the
    /// one-short menace attackers to distinct remaining blockers.
    fn blocks_completable(&self, extra: Option<(CardIdx, CardIdx)>, rest: &[CardIdx]) -> bool {
        let mut short: Vec<CardIdx> = Vec::new();
        for a in self.attackers() {
            if !self.has_kw(a, Kw::Menace) {
                continue;
            }
            let n = self.blockers_of(a).len() + extra.map_or(0, |(_, x)| (x == a) as usize);
            if n == 1 {
                short.push(a);
            }
        }
        if short.is_empty() {
            return true;
        }
        // Augmenting paths: assign each short attacker a distinct blocker.
        let mut owner: Vec<Option<usize>> = vec![None; rest.len()];
        fn augment(
            g: &Game,
            ai: usize,
            short: &[CardIdx],
            rest: &[CardIdx],
            owner: &mut Vec<Option<usize>>,
            seen: &mut Vec<bool>,
        ) -> bool {
            for (bi, &b) in rest.iter().enumerate() {
                if seen[bi] || !g.can_block(b, short[ai]) {
                    continue;
                }
                seen[bi] = true;
                if owner[bi].is_none() || augment(g, owner[bi].unwrap(), short, rest, owner, seen) {
                    owner[bi] = Some(ai);
                    return true;
                }
            }
            false
        }
        (0..short.len()).all(|ai| {
            let mut seen = vec![false; rest.len()];
            augment(self, ai, &short, rest, &mut owner, &mut seen)
        })
    }

    /// CR 510.4: a first-strike damage step happens if an attacking or
    /// blocking creature has first strike or double strike.
    pub(crate) fn first_strike_step_needed(&self) -> bool {
        self.battlefield.iter().any(|&c| {
            let o = &self.objs[c.i()];
            (o.attacking || o.blocking.is_some())
                && (self.has_kw(c, Kw::FirstStrike) || self.has_kw(c, Kw::DoubleStrike))
        })
    }

    /// Does `c` deal combat damage in this damage step (CR 510.4, 702.4b,
    /// 702.7b)?
    fn deals_damage_now(&self, c: crate::ids::CardIdx, first: bool) -> bool {
        let fs = self.has_kw(c, Kw::FirstStrike);
        let ds = self.has_kw(c, Kw::DoubleStrike);
        if first {
            fs || ds
        } else {
            ds || !self.objs[c.i()].struck_first
        }
    }

    /// Combat damage step (CR 510.1-2): assignments attacker by attacker
    /// (decisions where the attacking player divides damage), then the
    /// blockers', then all damage at once.
    pub(crate) fn combat_damage(&mut self, first: bool, att: u8) {
        let dp = opp(self.turn.active);
        let attackers = self.attackers();
        for (k, &a) in attackers.iter().enumerate().skip(att as usize) {
            if !self.deals_damage_now(a, first) {
                continue;
            }
            let power = self.power(a);
            if power <= 0 {
                continue; // CR 510.1a
            }
            let blockers = self.blockers_of(a);
            let trample = self.has_kw(a, Kw::Trample);
            if !self.objs[a.i()].blocked {
                self.combat_dmg.push((a, Target::Player(dp), power as u16));
            } else if blockers.is_empty() {
                // CR 509.1h / 702.19e: blocked, blockers gone: only trample.
                if trample {
                    self.combat_dmg.push((a, Target::Player(dp), power as u16));
                }
            } else if blockers.len() == 1 && !trample {
                let b = self.obj_ref(blockers[0]);
                self.combat_dmg.push((a, Target::Obj(b), power as u16));
            } else {
                self.agenda.push(Task::CombatDamage { first, att: k as u8 + 1 });
                self.agenda.push(Task::AssignDamage { att: a, j: 0, left: power as u16 });
                return;
            }
        }
        // Blockers deal damage to the attacker they block (510.1d).
        for &c in &self.battlefield.clone() {
            let Some(a) = self.objs[c.i()].blocking else { continue };
            if !self.deals_damage_now(c, first) || !self.objs[a.i()].attacking {
                continue;
            }
            let p = self.power(c);
            if p > 0 {
                let ar = self.obj_ref(a);
                self.combat_dmg.push((c, Target::Obj(ar), p as u16));
            }
        }
        // CR 510.2: all at once. Sources are read before anything changes.
        let list = std::mem::take(&mut self.combat_dmg);
        let sources: Vec<_> = list.iter().map(|&(s, _, _)| self.damage_source_of(s)).collect();
        for (&(s, t, n), src) in list.iter().zip(sources) {
            if first {
                self.objs[s.i()].struck_first = true;
            }
            self.deal_damage(src, t, n as i32);
        }
    }

    /// Lethal damage for assignment purposes (CR 510.1c-d, 702.2c): what the
    /// blocker still needs, counting damage already assigned this step.
    pub(crate) fn lethal_needed(&self, att: CardIdx, b: CardIdx) -> u16 {
        let assigned: u16 = self
            .combat_dmg
            .iter()
            .filter(|&&(_, t, _)| t == Target::Obj(self.obj_ref(b)))
            .map(|&(_, _, n)| n)
            .sum();
        let have = self.objs[b.i()].damage + assigned;
        if self.has_kw(att, Kw::Deathtouch) {
            return if assigned > 0 { 0 } else { 1 };
        }
        (self.toughness(b).max(0) as u16).saturating_sub(have)
    }

    /// The attacking player divides `att`'s damage among its blockers in
    /// order: `NUMBER` n to blocker `j`. Without trample everything goes to
    /// blockers; with trample the player gets the rest only if every blocker
    /// was assigned lethal damage (702.19c).
    pub(crate) fn assign_damage(&mut self, att: CardIdx, j: u8, left: u16) {
        let blockers = self.blockers_of(att);
        let trample = self.has_kw(att, Kw::Trample);
        let dp = opp(self.turn.active);
        if let Some(Choice::Number(n)) = self.answer.take() {
            let b = self.obj_ref(blockers[j as usize]);
            if n > 0 {
                self.combat_dmg.push((att, Target::Obj(b), n as u16));
            }
            self.agenda.push(Task::AssignDamage { att, j: j + 1, left: left - n as u16 });
            return;
        }
        if j as usize >= blockers.len() {
            if left > 0 {
                debug_assert!(trample);
                self.combat_dmg.push((att, Target::Player(dp), left));
            }
            return;
        }
        let b = blockers[j as usize];
        let last = j as usize == blockers.len() - 1;
        let prev_lethal = blockers[..j as usize].iter().all(|&x| self.lethal_needed(att, x) == 0);
        let lethal = self.lethal_needed(att, b);
        let amounts: Vec<u16> = if !last {
            (0..=left).collect()
        } else if !trample || !prev_lethal || left < lethal {
            vec![left]
        } else {
            (lethal..=left).collect()
        };
        assert!(left < act::NUMBER_N, "damage {left} exceeds the NUMBER range");
        let opts = amounts
            .into_iter()
            .map(|n| Opt { action: act::NUMBER + n, choice: Choice::Number(n as u8) })
            .collect();
        let ap = self.turn.active;
        self.ask(Task::AssignDamage { att, j, left }, ap, DecisionKind::AssignDamage, opts);
    }

    /// CR 511.3: remove all creatures from combat.
    pub(crate) fn remove_from_combat(&mut self) {
        for &c in &self.battlefield {
            let o = &mut self.objs[c.i()];
            o.attacking = false;
            o.blocked = false;
            o.blocking = None;
            o.struck_first = false;
        }
    }
}
