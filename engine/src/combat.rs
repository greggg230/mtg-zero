//! Combat (CR 506-511; ENGINE.md 12).

use crate::db::{EventKind, Kw};
use crate::triggers::{Event, Subj};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::Game;
use crate::ids::{opp, CardIdx, PlayerId, Target};

impl Game {
    /// Attacking creatures in canonical order (the active player's slots).
    pub fn attackers(&self) -> Vec<CardIdx> {
        self.permanents_of(self.turn.active).filter(|&c| self.objs[c.i()].attacking).collect()
    }

    /// What attacking creature `c` is attacking now (CR 506.3, 508.1b): the
    /// defending player, a planeswalker that player controls, or nothing
    /// once that planeswalker was removed from combat (506.4: it left the
    /// battlefield, changed controller or stopped being a planeswalker;
    /// 506.4c).
    pub fn attack_dest(&self, c: CardIdx) -> Option<Target> {
        let o = &self.objs[c.i()];
        if !o.attacking {
            return None;
        }
        let dp = opp(o.controller);
        match o.attacks {
            None => Some(Target::Player(dp)),
            Some(r) => (self.live(r)
                && self.objs[r.idx.i()].zone == crate::ids::Zone::Battlefield
                && self.objs[r.idx.i()].controller == dp
                && self.is_type(r.idx, crate::db::types::PLANESWALKER))
            .then_some(Target::Obj(r)),
        }
    }

    /// Planeswalkers `p` controls (the ones `p`'s opponent may attack).
    pub(crate) fn walkers_of(&self, p: PlayerId) -> Vec<CardIdx> {
        self.permanents_of(p).filter(|&c| self.is_type(c, crate::db::types::PLANESWALKER)).collect()
    }

    /// CR 508.1b / 508.4: which player or planeswalker attacking creature
    /// `card` attacks, chosen by its controller when the defending player
    /// controls a planeswalker (`PLAYER` + 1 = the defending player,
    /// `OPP_PERM` = one of their planeswalkers); otherwise the player.
    pub(crate) fn choose_attack_target(&mut self, card: CardIdx) {
        let o = &self.objs[card.i()];
        if o.zone != crate::ids::Zone::Battlefield || !o.attacking {
            return;
        }
        let p = o.controller;
        let dp = opp(p);
        match self.answer.take() {
            Some(Choice::Target(Target::Obj(r))) => {
                self.objs[card.i()].attacks = Some(r);
                self.note(|g| format!("{} attacks {}", g.name(card), g.name(r.idx)));
            }
            Some(Choice::Target(Target::Player(_))) => self.objs[card.i()].attacks = None,
            Some(other) => unreachable!("attack target answer {other:?}"),
            None => {
                let walkers = self.walkers_of(dp);
                if walkers.is_empty() {
                    return;
                }
                // CR 701.38a: goaded, it attacks a player other than the
                // goader if able: goaded by its own controller (2p), it
                // attacks the defending player, not a planeswalker.
                if self.goaded_by(card, p) || self.must_attack_player(card) {
                    return;
                }
                let mut opts = vec![Opt { action: act::PLAYER + 1, choice: Choice::Target(Target::Player(dp)) }];
                for w in walkers {
                    let r = self.obj_ref(w);
                    opts.push(Opt { action: self.perm_action(p, w), choice: Choice::Target(Target::Obj(r)) });
                }
                self.ask(Task::AttackTarget { card }, p, DecisionKind::AttackTarget, opts);
            }
        }
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

    /// Can `b` block `a` (CR 509.1b restrictions)? Flying attackers need
    /// flying or reach (702.9b); "can't be blocked"; protection (702.16e:
    /// not by creatures with that quality); "can block only creatures with
    /// flying"; "can't be blocked by" creatures matching a filter.
    pub(crate) fn can_block(&self, b: CardIdx, a: CardIdx) -> bool {
        let (ka, kb) = (self.keywords_of(a), self.keywords_of(b));
        if ka.has(Kw::Flying) && !kb.has(Kw::Flying) && !kb.has(Kw::Reach) {
            return false;
        }
        // CR 702.28b: shadow blocks and is blocked only by shadow.
        if ka.has(Kw::Shadow) != kb.has(Kw::Shadow) {
            return false;
        }
        if self.flags_of(a).cant_be_blocked || self.protected_from(a, &self.damage_source_of(b)) {
            return false;
        }
        if self.abilities(b).blocks_only_flyers && !ka.has(Kw::Flying) {
            return false;
        }
        // "This can't block or be blocked by non-Spirit creatures."
        if let Some(s) = self.abilities(b).blocks_only_subtype {
            if !self.has_subtype(a, s) {
                return false;
            }
        }
        if let Some(s) = self.abilities(a).blocks_only_subtype {
            if !self.has_subtype(b, s) {
                return false;
            }
        }
        if let Some(f) = &self.abilities(a).unblockable_by {
            let ctx = crate::cast::Ctx { controller: self.objs[a.i()].controller, source: Some(self.obj_ref(a)) };
            if self.card_matches(f, b, &ctx) {
                return false;
            }
        }
        true
    }

    /// The fewest creatures that may block `a` (menace 702.110b: two; "can't
    /// be blocked except by three or more creatures").
    pub(crate) fn min_blockers(&self, a: CardIdx) -> usize {
        let menace = if self.has_kw(a, Kw::Menace) { 2 } else { 1 };
        menace.max(self.abilities(a).min_blockers as usize)
    }

    /// Untapped creatures of the defending player that can block (CR
    /// 509.1a).
    pub(crate) fn potential_blockers(&self) -> Vec<CardIdx> {
        let dp = opp(self.turn.active);
        self.creatures_of(dp).filter(|&c| !self.objs[c.i()].tapped && !self.flags_of(c).cant_block).collect()
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
                // CR 508.1b: what it attacks, when there is a choice.
                self.agenda.push(Task::AttackTarget { card: c });
                return;
            }
            Some(Choice::Pass) => {
                // CR 508.1f: attackers without vigilance tap.
                let attackers = self.attackers();
                for &c in &attackers {
                    if !self.has_kw(c, Kw::Vigilance) {
                        self.tap(c);
                    }
                }
                for &c in &attackers {
                    let r = self.obj_ref(c);
                    self.players[ap as usize].attacked_with.push(r);
                }
                // CR 508.1m: abilities that trigger on attacking.
                for &c in &attackers {
                    let r = self.obj_ref(c);
                    self.event(Event::obj(EventKind::Attacks, r));
                }
                if !attackers.is_empty() {
                    self.event(Event::player(EventKind::YouAttack, ap));
                }
                if let [only] = attackers[..] {
                    let r = self.obj_ref(only);
                    self.event(Event::obj(EventKind::AttacksAlone, r)); // exalted
                }
                return;
            }
            Some(other) => unreachable!("attack answer {other:?}"),
            None => {}
        }
        if next == 0 {
            // CR 508.1d: creatures that attack each combat if able are
            // declared first (no attack here has a cost, so they can).
            let mut forced = Vec::new();
            for c in self.permanents_of(ap).collect::<Vec<_>>() {
                if !self.objs[c.i()].attacking && self.can_attack(c) && self.flags_of(c).must_attack {
                    self.objs[c.i()].attacking = true;
                    self.note(|g| format!("{} must attack", g.name(c)));
                    forced.push(c);
                }
            }
            if !forced.is_empty() {
                // Their attack targets first (508.1b), in slot order.
                // (Run again: they are attacking now, so nothing repeats.)
                self.agenda.push(Task::DeclareAttackers { next: 0 });
                for &c in forced.iter().rev() {
                    self.agenda.push(Task::AttackTarget { card: c });
                }
                return;
            }
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
            // CR 509.1i (509.2a-like): "whenever this blocks" triggers.
            let blocks: Vec<(CardIdx, CardIdx)> = self
                .battlefield
                .iter()
                .filter_map(|&c| self.objs[c.i()].blocking.map(|a| (c, a)))
                .collect();
            for (c, a) in blocks {
                let (r, ar) = (self.obj_ref(c), self.obj_ref(a));
                self.event(Event { kind: EventKind::Blocks, a: Subj::Obj(r), b: Subj::Obj(ar), amount: 0 });
            }
            // "Whenever one or more creatures an opponent controls attack you
            // and aren't blocked" (Coveted Jewel): attacking the player.
            if self.seen & crate::game::SEEN_RARE_EVENTS != 0
                && self.attackers().into_iter().any(|a| !self.objs[a.i()].blocked && self.objs[a.i()].attacks.is_none())
            {
                let ap = self.turn.active;
                self.event(Event { kind: EventKind::AttackersUnblocked, a: Subj::Player(ap), b: Subj::Player(dp), amount: 0 });
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
    /// still make the declaration legal? Every attacker that needs n
    /// blockers (menace 702.110b: two) has zero or at least n. A matching of
    /// the missing blocker places of the short attackers to distinct
    /// remaining blockers.
    fn blocks_completable(&self, extra: Option<(CardIdx, CardIdx)>, rest: &[CardIdx]) -> bool {
        let mut short: Vec<CardIdx> = Vec::new();
        for a in self.attackers() {
            let need = self.min_blockers(a);
            if need < 2 {
                continue;
            }
            let n = self.blockers_of(a).len() + extra.map_or(0, |(_, x)| (x == a) as usize);
            if n > 0 && n < need {
                // One place per missing blocker.
                short.extend(std::iter::repeat_n(a, need - n));
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
            // CR 510.1b: to what it attacks (nothing once a planeswalker it
            // attacked left combat, 506.4c; 702.19f: never to the player).
            let dest = self.attack_dest(a);
            let trample = self.has_kw(a, Kw::Trample) && dest.is_some();
            if !self.objs[a.i()].blocked {
                if let Some(t) = dest {
                    self.combat_dmg.push((a, t, crate::num::unsigned(power)));
                }
            } else if blockers.is_empty() {
                // CR 509.1h / 702.19d: blocked, blockers gone: only trample.
                if let (true, Some(t)) = (trample, dest) {
                    self.combat_dmg.push((a, t, crate::num::unsigned(power)));
                }
            } else if blockers.len() == 1 && !trample {
                let b = self.obj_ref(blockers[0]);
                self.combat_dmg.push((a, Target::Obj(b), crate::num::unsigned(power)));
            } else {
                self.agenda.push(Task::CombatDamage { first, att: k as u8 + 1 });
                self.agenda.push(Task::AssignDamage { att: a, j: 0, left: crate::num::unsigned(power) });
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
                self.combat_dmg.push((c, Target::Obj(ar), crate::num::unsigned(p)));
            }
        }
        // CR 510.2: all at once. Sources are read before anything changes.
        let list = std::mem::take(&mut self.combat_dmg);
        let sources: Vec<_> = list.iter().map(|&(s, _, _)| self.damage_source_of(s)).collect();
        let refs: Vec<_> = list.iter().map(|&(s, _, _)| self.obj_ref(s)).collect();
        let mut to_players: Vec<(crate::ids::ObjRef, crate::ids::PlayerId, i32)> = Vec::new();
        let mut to_creatures: Vec<(crate::ids::ObjRef, crate::ids::ObjRef, i32)> = Vec::new();
        let mut to_walkers: Vec<(crate::ids::ObjRef, crate::ids::ObjRef, i32)> = Vec::new();
        self.combat_damage_now = true;
        // Combat damage each source dealt in all (one "deals damage" event
        // per source: the damage is dealt at once, CR 510.2).
        let mut by_source: Vec<(crate::ids::ObjRef, i32)> = Vec::new();
        // Combat damage each creature or planeswalker was dealt in all (one
        // "is dealt damage" event per recipient: Screaming Nemesis blocked by
        // two creatures triggers once, for the total; ruling 66fc62ef).
        let mut by_recipient: Vec<(crate::ids::ObjRef, i32)> = Vec::new();
        for ((&(s, t, n), src), r) in list.iter().zip(sources).zip(refs) {
            if first {
                self.objs[s.i()].struck_first = true;
            }
            // (A creature or planeswalker recipient, as the damage is dealt.)
            let to_creature = matches!(t, Target::Obj(o) if self.live(o) && self.is_creature(o.idx));
            let to_walker =
                matches!(t, Target::Obj(o) if self.live(o) && self.is_type(o.idx, crate::db::types::PLANESWALKER));
            let dealt = self.deal_damage(src, t, crate::num::signed(n));
            // (Totals saturate, as every amount: ENGINE.md 46.)
            if dealt > 0 {
                match by_source.iter_mut().find(|x| x.0 == r) {
                    Some(x) => x.1 = x.1.saturating_add(dealt),
                    None => by_source.push((r, dealt)),
                }
            }
            if let (Target::Obj(o), true) = (t, dealt > 0) {
                match by_recipient.iter_mut().find(|x| x.0 == o) {
                    Some(x) => x.1 = x.1.saturating_add(dealt),
                    None => by_recipient.push((o, dealt)),
                }
            }
            if let (Target::Player(p), true) = (t, dealt > 0) {
                to_players.push((r, p, dealt));
            }
            if let (Target::Obj(o), true, true) = (t, dealt > 0, to_creature) {
                to_creatures.push((r, o, dealt));
            }
            if let (Target::Obj(o), true, true) = (t, dealt > 0, to_walker) {
                to_walkers.push((r, o, dealt));
            }
        }
        self.combat_damage_now = false;
        for (o, n) in by_recipient {
            let mut ev = Event::obj(EventKind::DealtDamage, o);
            ev.amount = crate::num::unsigned(n);
            self.event(ev);
        }
        for (r, n) in by_source {
            self.event(Event { kind: EventKind::DealsDamage, a: Subj::Obj(r), b: Subj::None, amount: crate::num::unsigned(n) });
        }
        for (r, o, n) in to_creatures {
            self.event(Event {
                kind: EventKind::DealsCombatDamageToCreature,
                a: Subj::Obj(r),
                b: Subj::Obj(o),
                amount: crate::num::unsigned(n),
            });
        }
        // CR 724.2: "whenever a creature deals combat damage to the monarch,
        // its controller becomes the monarch" (the monarch's inherent
        // triggered ability, controlled by the monarch; one per creature).
        if let Some(m) = self.monarch {
            for &(r, p, _) in &to_players {
                if p == m {
                    let c = self.objs[r.idx.i()].controller;
                    let ev = crate::triggers::EvCtx { player: Some(c), obj: Some(r), ..Default::default() };
                    self.monarch_trigger(m, crate::dungeon::MONARCH_DAMAGED, ev);
                }
            }
        }
        // CR 725.2: "whenever one or more creatures a player controls deal
        // combat damage to the player who has the initiative, the controller
        // of those creatures takes the initiative" (controlled by the player
        // who has it; once per damage step and attacking player).
        if let Some(h) = self.initiative {
            let mut takers: Vec<crate::ids::PlayerId> = Vec::new();
            for &(r, p, _) in &to_players {
                let c = self.objs[r.idx.i()].controller;
                if p == h && c != h && !takers.contains(&c) {
                    takers.push(c);
                }
            }
            for c in takers {
                let ev = crate::triggers::EvCtx { player: Some(c), ..Default::default() };
                self.initiative_trigger(h, crate::dungeon::DAMAGED, ev);
            }
        }
        // CR 510.3a: "deals combat damage to a player [or planeswalker]"
        // triggers (their predicates say which recipients count). The damage
        // of one step is dealt at once: "whenever one or more creatures deal
        // combat damage" triggers once for it (a batch, `TrigPred::Batch`).
        let batch = !self.batching;
        if batch {
            self.begin_batch();
        }
        for (r, p, n) in to_players {
            self.event(Event {
                kind: EventKind::DealsCombatDamage,
                a: Subj::Obj(r),
                b: Subj::Player(p),
                amount: crate::num::unsigned(n),
            });
        }
        for (r, o, n) in to_walkers {
            self.event(Event {
                kind: EventKind::DealsCombatDamage,
                a: Subj::Obj(r),
                b: Subj::Obj(o),
                amount: crate::num::unsigned(n),
            });
        }
        if batch {
            self.end_batch();
        }
    }

    /// Lethal damage for assignment purposes (CR 510.1c-d, 702.2c): what the
    /// blocker still needs, counting damage already assigned this step.
    pub(crate) fn lethal_needed(&self, att: CardIdx, b: CardIdx) -> u32 {
        let assigned: u32 = self
            .combat_dmg
            .iter()
            .filter(|&&(_, t, _)| t == Target::Obj(self.obj_ref(b)))
            .fold(0u32, |s, &(_, _, n)| s.saturating_add(n));
        let have = self.objs[b.i()].damage.saturating_add(assigned);
        if self.has_kw(att, Kw::Deathtouch) {
            return if assigned > 0 { 0 } else { 1 };
        }
        crate::num::unsigned(self.toughness(b)).saturating_sub(have)
    }

    /// The attacking player divides `att`'s damage among its blockers in
    /// order: `NUMBER` n to blocker `j`. Without trample everything goes to
    /// blockers; with trample the player gets the rest only if every blocker
    /// was assigned lethal damage (702.19c).
    pub(crate) fn assign_damage(&mut self, att: CardIdx, j: u8, left: u32) {
        let blockers = self.blockers_of(att);
        // CR 702.19b: the rest to the player or planeswalker it attacks
        // (with nothing to attack, everything to the blockers, 702.19f).
        let dest = self.attack_dest(att);
        let trample = self.has_kw(att, Kw::Trample) && dest.is_some();
        if let Some(Choice::Number(k)) = self.answer.take() {
            // With 32 or more to divide, 30 = lethal and 31 = all the rest.
            let n = match (left >= act::NUMBER_N as u32, k) {
                (true, 30) => self.lethal_needed(att, blockers[j as usize]),
                (true, 31) => left,
                _ => k as u32,
            };
            let b = self.obj_ref(blockers[j as usize]);
            if n > 0 {
                self.combat_dmg.push((att, Target::Obj(b), n));
            }
            self.agenda.push(Task::AssignDamage { att, j: j + 1, left: left - n });
            return;
        }
        if j as usize >= blockers.len() {
            if left > 0 {
                debug_assert!(trample);
                if let Some(t) = dest {
                    self.combat_dmg.push((att, t, left));
                }
            }
            return;
        }
        let b = blockers[j as usize];
        let last = j as usize == blockers.len() - 1;
        let prev_lethal = blockers[..j as usize].iter().all(|&x| self.lethal_needed(att, x) == 0);
        let lethal = self.lethal_needed(att, b);
        // The amounts the player may assign to this blocker: `lo..=hi`
        // (a range, not a list: `left` may be any amount, ENGINE.md 46).
        let (lo, hi) = if !last {
            (0, left)
        } else if !trample || !prev_lethal || left < lethal {
            (left, left)
        } else {
            (lethal, left)
        };
        let has = |n: u32| (lo..=hi).contains(&n);
        let number = |n: u32| Opt { action: act::NUMBER + n as u16, choice: Choice::Number(n as u8) };
        // `NUMBER` n = n damage. With 32 or more to divide, the amounts
        // offered are 0..=29 exactly, the blocker's lethal damage (n = 30)
        // and all of the rest (n = 31): the divisions that differ in what
        // dies and what tramples over (ENGINE.md 13).
        let opts = if left < act::NUMBER_N as u32 {
            (lo..=hi).map(number).collect()
        } else {
            let mut v: Vec<Opt> = (lo..=hi.min(29)).map(number).collect();
            if lethal > 29 && has(lethal) {
                v.push(Opt { action: act::NUMBER + 30, choice: Choice::Number(30) });
            }
            if left > 29 && left != lethal && has(left) {
                v.push(Opt { action: act::NUMBER + 31, choice: Choice::Number(31) });
            }
            v
        };
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
            o.attacks = None;
        }
    }
}
