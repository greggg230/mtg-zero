//! The `Ext` instructions of the single-card tail, part 2 (M7c, ENGINE.md
//! 29).

use crate::db::types;
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{CardIdx, PlayerId, Target, Zone};
use crate::program::{Ext, On};
use crate::resolve::{Flow, Frame};

/// `Permission::turn` with this bit: "until the end of your next turn", the
/// low bits the permission player's turn count it ends with.
pub const PERM_UNTIL_TURNS: u32 = 1 << 31;
/// `Permission::turn` with this bit: "on a later turn" (warp), the low bits
/// the turn number after which it holds.
pub const PERM_LATER: u32 = 1 << 30;

fn yes_no() -> Vec<Opt> {
    vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }]
}

impl Game {
    pub(crate) fn exec_tail2(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::SacrificeN { who, filter, n } => self.sacrifice_n(f, *who, filter, *n),
            Ext::FromGraveyardAs { tgt, mods } => {
                f.made = None;
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone != Zone::Graveyard {
                        continue;
                    }
                    self.entering_mods = mods.clone();
                    let r = self.put_onto_battlefield(o.idx, f.controller);
                    self.entering_mods.clear();
                    if self.objs[r.idx.i()].zone == Zone::Battlefield {
                        f.made = Some(r.idx);
                    }
                }
                Flow::Next
            }
            Ext::ReturnEventCardAs { mods } => {
                f.made = None;
                if let Some(r) = f.ev.obj {
                    let o = &self.objs[r.idx.i()];
                    // The card where it went (a leave trigger's own event),
                    // or, for "when this dies", the card it became in the
                    // graveyard (one move later).
                    let here = self.live(r) && matches!(o.zone, Zone::Exile | Zone::Graveyard)
                        || o.zone == Zone::Graveyard && o.inc == r.inc.wrapping_add(1);
                    if here && !self.is_token(r.idx) {
                        let owner = self.objs[r.idx.i()].owner;
                        self.entering_mods = mods.clone();
                        self.put_onto_battlefield(r.idx, owner);
                        self.entering_mods.clear();
                        f.made = Some(r.idx);
                    }
                }
                Flow::Next
            }
            Ext::Harness => {
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    let r = self.obj_ref(c);
                    if !self.marks.contains(&(r, crate::game::Mark::Harnessed)) {
                        self.marks.push((r, crate::game::Mark::Harnessed));
                        self.note(|g| format!("{} is harnessed", g.name(c)));
                    }
                }
                Flow::Next
            }
            Ext::ExilePlayUntilNextTurn { tgt } => {
                let p = f.controller;
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone != Zone::Graveyard {
                        continue;
                    }
                    let card = self.move_card(o.idx, Zone::Exile, Pos::Top);
                    // "Until the end of your next turn" (the turn count it
                    // ends with: this player's next turn).
                    let until = self.players[p as usize].turns + 1;
                    self.permissions.push(crate::game::Permission {
                        card,
                        player: p,
                        turn: Some(PERM_UNTIL_TURNS | until),
                        kind: crate::game::PermKind::Play { free: false, cast_only: false, any_mana: false, attacked_subtype: None },
                    });
                }
                Flow::Next
            }
            Ext::ExileEventCardLinked { optional } => {
                let p = f.controller;
                let Some(r) = f.ev.obj else { return Flow::Next };
                if !self.live(r) || self.objs[r.idx.i()].zone != Zone::Graveyard || self.objs[r.idx.i()].owner != p {
                    return Flow::Next;
                }
                if *optional {
                    match self.answer.take() {
                        Some(Choice::Yes) => {}
                        Some(_) => return Flow::Next,
                        None => {
                            self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, yes_no());
                            return Flow::Suspend;
                        }
                    }
                }
                let card = self.move_card(r.idx, Zone::Exile, Pos::Top);
                self.links.push(crate::game::Link { source: f.source, card, until: false, back: Zone::Graveyard, until_monarch: None });
                Flow::Next
            }
            Ext::LinkedToGraveyardThenToken { land, nonland } => {
                let p = f.controller;
                f.made = None;
                let pick = match self.answer.take() {
                    Some(Choice::Card(c)) => c,
                    Some(other) => unreachable!("linked card answer {other:?}"),
                    None => {
                        let cands = self.linked_cards(f.source);
                        if cands.is_empty() {
                            return Flow::Next;
                        }
                        let opts = cands
                            .iter()
                            .take(act::EXILE_N as usize)
                            .enumerate()
                            .map(|(k, &c)| Opt { action: act::EXILE + k as u16, choice: Choice::Card(c) })
                            .collect();
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseExiled, opts);
                        return Flow::Suspend;
                    }
                };
                let is_land = self.def_of(pick).is(types::LAND);
                self.move_card(pick, Zone::Graveyard, Pos::Top);
                let token = if is_land { *land } else { *nonland };
                let def = self.db.token_def(token);
                let made = self.create_tokens(def, p, 1, false);
                f.made = made.first().copied();
                f.made_all = made;
                Flow::Next
            }
            Ext::PlayFromGraveyardThisTurn => {
                let n = self.turn.number;
                self.players[f.controller as usize].gy_play_turn = n;
                Flow::Next
            }
            Ext::GraveyardToExileThisTurn => {
                let n = self.turn.number;
                self.players[f.controller as usize].gy_exile_turn = n;
                Flow::Next
            }
            Ext::Cascade => self.cascade(f),
            Ext::ExtraCombat => {
                use crate::game::Step;
                let combat = matches!(
                    self.turn.step,
                    Step::BeginCombat | Step::DeclareAttackers | Step::DeclareBlockers | Step::FirstStrikeDamage | Step::CombatDamage | Step::EndCombat
                );
                if combat {
                    self.turn.extra_combats = self.turn.extra_combats.saturating_add(1);
                    self.note(|_| "an additional combat phase follows".to_string());
                }
                Flow::Next
            }
            Ext::WarpExile => {
                if let Some(&c) = self.objs_on(f, &On::EventObj).first() {
                    let owner = self.objs[c.i()].owner;
                    let token = self.is_token(c);
                    let card = self.move_card(c, Zone::Exile, Pos::Top);
                    if !token && self.objs[c.i()].zone == Zone::Exile {
                        // "Then you may cast it from exile on a later turn."
                        self.permissions.push(crate::game::Permission {
                            card,
                            player: owner,
                            turn: Some(PERM_LATER | self.turn.number),
                            kind: crate::game::PermKind::Play { free: false, cast_only: true, any_mana: false, attacked_subtype: None },
                        });
                    }
                }
                Flow::Next
            }
            Ext::MiracleCast => {
                let p = f.controller;
                let Some(r) = f.ev.obj else { return Flow::Next };
                let card = r.idx;
                if !self.live(r) || self.objs[card.i()].zone != Zone::Hand || self.objs[card.i()].owner != p {
                    return Flow::Next;
                }
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        // The rest of the effect waits for the cast (CR
                        // 608.2g).
                        let mut back = f.clone();
                        back.pc += 1;
                        back.i = 0;
                        self.agenda.push(Task::Run(back));
                        self.miracle_cast = Some(card);
                        self.begin_cast(p, card);
                        Flow::Suspend
                    }
                    Some(_) => Flow::Next,
                    None => {
                        if !self.can_cast_miracle(p, card) {
                            return Flow::Next;
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, yes_no());
                        Flow::Suspend
                    }
                }
            }
            Ext::Airbend { tgt, cost } => {
                for t in self.targets_of(f, *tgt) {
                    let Target::Obj(o) = t else { continue };
                    if !matches!(self.objs[o.idx.i()].zone, Zone::Battlefield | Zone::Stack) {
                        continue;
                    }
                    let owner = self.objs[o.idx.i()].owner;
                    let token = self.is_token(o.idx);
                    self.note(|g| format!("{} is airbent", g.name(o.idx)));
                    let card = self.move_card(o.idx, Zone::Exile, Pos::Top);
                    if !token && self.objs[o.idx.i()].zone == Zone::Exile {
                        self.permissions.push(crate::game::Permission {
                            card,
                            player: owner,
                            turn: None,
                            kind: crate::game::PermKind::Airbent(cost.generic),
                        });
                    }
                }
                Flow::Next
            }
            Ext::CopyTokensEnteredThisTurn => {
                let p = f.controller;
                let turn = self.turn.number as u16;
                let tokens: Vec<CardIdx> = self.permanents_of(p).filter(|&c| self.is_token(c) && self.objs[c.i()].entered == turn).collect();
                let mut all = Vec::new();
                for c in tokens {
                    let vals = self.copiable(c);
                    all.extend(self.create_token_copies(vals, p, 1));
                }
                f.made = all.first().copied();
                f.made_all = all;
                Flow::Next
            }
            Ext::MillThenLandToHand { n } => {
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.move_card(c, Zone::Hand, Pos::Top);
                        f.picked.clear();
                        return Flow::Next;
                    }
                    Some(_) => {
                        f.picked.clear();
                        return Flow::Next;
                    }
                    None => {}
                }
                if f.i == 0 {
                    f.i = 1;
                    f.picked.clear();
                    for _ in 0..*n {
                        let Some(&top) = self.players[p as usize].library.last() else { break };
                        self.move_card(top, Zone::Graveyard, Pos::Top);
                        if self.objs[top.i()].zone == Zone::Graveyard {
                            f.picked.push(top);
                        }
                    }
                }
                let lands: Vec<CardIdx> =
                    f.picked.iter().copied().filter(|&c| self.objs[c.i()].zone == Zone::Graveyard && self.def_of(c).is(types::LAND)).collect();
                if lands.is_empty() {
                    f.picked.clear();
                    return Flow::Next;
                }
                let mut opts: Vec<Opt> = lands.iter().map(|&c| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) }).collect();
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseGraveyardCard, opts);
                Flow::Suspend
            }
            Ext::MayCastFromGraveyardNow { filter, exile } => {
                let p = f.controller;
                // After the cast (`f.i` 1, the card in `f.picked`): "if an
                // instant or sorcery spell cast this way would be put into
                // your graveyard, exile it instead" — the spell's type, the
                // face it was cast with (an Adventure, CR 715.3b).
                if f.i == 1 {
                    if let Some(&c) = f.picked.first() {
                        if *exile && self.objs[c.i()].zone == Zone::Stack && self.def_of(c).is(types::INSTANT | types::SORCERY) {
                            self.gy_exile.push(c);
                        }
                    }
                    f.picked.clear();
                    return Flow::Next;
                }
                let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        // The rest of the effect waits for the cast (CR
                        // 608.2g).
                        let mut back = f.clone();
                        back.i = 1;
                        back.floor = 0;
                        back.picked.clear();
                        back.picked.push(c);
                        self.agenda.push(Task::Run(back));
                        let r = self.obj_ref(c);
                        let kind = crate::game::PermKind::Play { free: false, cast_only: true, any_mana: false, attacked_subtype: None };
                        self.permissions.push(crate::game::Permission { card: r, player: p, turn: Some(self.turn.number), kind });
                        // A spell, not a card: an adventurer card may be
                        // cast as its Adventure (an instant or sorcery
                        // spell), and which is the player's choice (CR
                        // 715.3, 601.2a; ruling d26ce62d).
                        let front = self.card_matches(filter, c, &ctx) && self.can_cast_now(p, c);
                        let face = self.gy_spell_face_now(p, c, filter, &ctx);
                        match (front, face) {
                            (true, Some(_)) => self.agenda.push(Task::ChooseFace { p, card: c, mask: crate::decision::FACE_CAST | crate::decision::OTHER_CAST }),
                            (false, Some(d)) => self.begin_cast_face(p, c, Some(d)),
                            _ => self.begin_cast(p, c),
                        }
                        Flow::Suspend
                    }
                    Some(_) => Flow::Next,
                    None => {
                        let cands: Vec<CardIdx> = self
                            .graveyard_cards(p)
                            .filter(|&c| (self.card_matches(filter, c, &ctx) && self.can_cast_now(p, c)) || self.gy_spell_face_now(p, c, filter, &ctx).is_some())
                            .collect();
                        if cands.is_empty() {
                            return Flow::Next;
                        }
                        let mut opts: Vec<Opt> = cands.iter().map(|&c| Opt { action: self.graveyard_action(p, c), choice: Choice::Card(c) }).collect();
                        opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseGraveyardCard, opts);
                        Flow::Suspend
                    }
                }
            }
            other => unreachable!("not a tail-2 op: {other:?}"),
        }
    }

    /// The other face (an Adventure, an MDFC's spell face) graveyard card
    /// `c` may be cast with now as a spell matching `filter` (CR 715.3a,
    /// 712.11c: only that face is evaluated), if any.
    fn gy_spell_face_now(&self, p: PlayerId, c: CardIdx, filter: &crate::db::Filter, ctx: &crate::cast::Ctx) -> Option<crate::ids::DefId> {
        let face = self.other_face(c)?;
        if self.db.def(face).is(types::LAND) {
            return None;
        }
        let mut g = self.clone();
        g.objs[c.i()].def = face;
        (g.card_matches(filter, c, ctx) && g.can_cast_now(p, c)).then_some(face)
    }

    /// Cascade (CR 702.85a). `f.i`: 0 = exile and offer the cast; 1 = the
    /// cast was answered; 2 = after the cast, put `f.picked` on the bottom.
    /// `f.picked`: the cards exiled, the hit card last.
    fn cascade(&mut self, f: &mut Frame) -> Flow {
        let p = f.controller;
        if f.i == 0 {
            // "This spell's mana value" (as it last existed, 608.2h).
            let mv = self.amount(f, crate::program::Amount::EventSpellMv) as u8;
            f.picked.clear();
            let mut hit = None;
            while let Some(&top) = self.players[p as usize].library.last() {
                self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                // One card at a time, each its own event (Laelia's ruling
                // 1178d637, CR 603.2c; ENGINE.md 38).
                self.new_event();
                self.move_card(top, Zone::Exile, Pos::Top);
                f.picked.push(top);
                if !self.def_of(top).is(types::LAND) && self.def_of(top).mana_value() < mv {
                    hit = Some(top);
                    break;
                }
            }
            f.i = 1;
            match hit {
                Some(c) if self.can_cast_free_now(p, c) => {
                    self.note(|g| format!("P{p} cascades into {}", g.name(c)));
                    self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, yes_no());
                    return Flow::Suspend;
                }
                _ => {
                    let rest = std::mem::take(&mut f.picked);
                    self.put_on_bottom_randomly(p, rest);
                    return Flow::Next;
                }
            }
        }
        if f.i == 1 {
            let hit = *f.picked.last().expect("a hit card");
            match self.answer.take() {
                Some(Choice::Yes) if self.objs[hit.i()].zone == Zone::Exile => {
                    // The rest go to the bottom after the cast (CR
                    // 702.85a: "then").
                    let mut back = f.clone();
                    back.i = 2;
                    back.picked.pop();
                    self.agenda.push(Task::Run(back));
                    self.cast_free_now(p, hit);
                    return Flow::Suspend;
                }
                Some(_) => {
                    let rest = std::mem::take(&mut f.picked);
                    self.put_on_bottom_randomly(p, rest);
                    return Flow::Next;
                }
                // (Asked again: a refresh of the pending decision.)
                None => {
                    if self.objs[hit.i()].zone == Zone::Exile && self.can_cast_free_now(p, hit) {
                        self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, yes_no());
                        return Flow::Suspend;
                    }
                    let rest = std::mem::take(&mut f.picked);
                    self.put_on_bottom_randomly(p, rest);
                    return Flow::Next;
                }
            }
        }
        let rest: Vec<CardIdx> = std::mem::take(&mut f.picked).into_iter().filter(|c| self.objs[c.i()].zone == Zone::Exile).collect();
        self.put_on_bottom_randomly(p, rest);
        Flow::Next
    }

    /// Could `p` cast `card` (in hand) for its miracle cost now (CR 702.94a,
    /// 608.2g: timing ignored)?
    pub(crate) fn can_cast_miracle(&self, p: PlayerId, card: CardIdx) -> bool {
        if self.def_of(card).miracle.is_none() || self.def_of(card).is(types::LAND) || !self.may_cast_spells(p) {
            return false;
        }
        if self.permanents_of(crate::ids::opp(p)).any(|c| self.abilities(c).opp_sorcery_speed) {
            return false;
        }
        let classes = self.mana_classes(p, None);
        self.castable_via(p, card, crate::game::CastFrom::Miracle, &classes, true)
    }

    /// Miracle (CR 702.94a): the first card `p` drew this turn may be
    /// revealed; if it is, its trigger waits. Asked even when the card has no
    /// miracle (only `PASS`; ENGINE.md 10). Choice: yes/no.
    pub(crate) fn miracle_reveal(&mut self, p: PlayerId, card: CardIdx) {
        let ok = self.objs[card.i()].zone == Zone::Hand && self.objs[card.i()].owner == p && self.def_of(card).miracle.is_some();
        match self.answer.take() {
            Some(Choice::Yes) if ok => {
                self.reveal(card);
                let r = self.obj_ref(card);
                let def = self.objs[card.i()].def;
                let k = self.def_of(card).miracle.as_ref().map_or(0, |m| m.1);
                self.note(|g| format!("P{p} reveals {} (miracle)", g.name(card)));
                self.waiting.push(crate::triggers::PendingTrigger {
                    controller: p,
                    source: r,
                    source_def: def,
                    slot: self.db.def(def).delayed_base + k,
                    ev: crate::triggers::EvCtx { obj: Some(r), player: Some(p), ..Default::default() },
                });
            }
            Some(_) => {}
            None => {
                let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                if ok {
                    opts.push(Opt { action: act::YES, choice: Choice::Yes });
                }
                self.ask(Task::MiracleReveal { p, card }, p, DecisionKind::YesNo, opts);
            }
        }
    }

    /// The cards exiled with `source` still there (CR 607.2a), in link
    /// order.
    pub(crate) fn linked_cards(&self, source: crate::ids::ObjRef) -> Vec<CardIdx> {
        self.links
            .iter()
            .filter(|l| l.source == source && self.live(l.card) && self.objs[l.card.idx.i()].zone == Zone::Exile)
            .map(|l| l.card.idx)
            .collect()
    }

    /// Is a permission's turn bound still open for `player` now (`turn`
    /// exact, or `PERM_UNTIL_TURNS`: through the end of their k-th turn)?
    pub(crate) fn perm_turn_ok(&self, turn: Option<u32>, player: PlayerId) -> bool {
        match turn {
            None => true,
            Some(t) if t & PERM_LATER != 0 => self.turn.number > t & !PERM_LATER,
            Some(t) if t & PERM_UNTIL_TURNS != 0 => {
                let k = t & !PERM_UNTIL_TURNS;
                let n = self.players[player as usize].turns;
                n < k || n == k && self.turn.active == player
            }
            Some(t) if t & crate::resolve_tail3::PERM_UNTIL_END_STEP != 0 => {
                let k = t & !crate::resolve_tail3::PERM_UNTIL_END_STEP;
                let n = self.players[player as usize].turns;
                n < k || n == k && self.turn.active == player && self.turn.step < crate::game::Step::End
            }
            Some(t) => t == self.turn.number,
        }
    }

    /// `who` sacrifices n permanents matching `filter` (a set chosen one
    /// permanent per decision in increasing slot order, then sacrificed at
    /// once, CR 701.21a). `f.picked`: the permanents chosen so far.
    fn sacrifice_n(&mut self, f: &mut Frame, who: crate::program::Who, filter: &crate::db::Filter, n: u8) -> Flow {
        let Some(p) = self.who(f, who) else { return Flow::Next };
        if self.shielded_from(p, f.controller) {
            return Flow::Next;
        }
        match self.answer.take() {
            Some(Choice::Card(c)) => f.picked.push(c),
            Some(other) => unreachable!("sacrifice answer {other:?}"),
            None => {}
        }
        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
        let all: Vec<CardIdx> = self.permanents_of(p).filter(|&c| self.card_matches(filter, c, &ctx)).collect();
        let done = if all.len() <= n as usize {
            // No choice: all of them.
            all
        } else if f.picked.len() >= n as usize {
            std::mem::take(&mut f.picked)
        } else {
            let floor = f.picked.iter().map(|&c| self.perm_action(p, c)).max();
            let mut acts: Vec<(u16, CardIdx)> = all
                .iter()
                .filter(|c| !f.picked.contains(c))
                .map(|&c| (self.perm_action(p, c), c))
                .filter(|&(a, _)| floor.is_none_or(|fl| a > fl))
                .collect();
            acts.sort();
            // Leave enough after this pick to reach n.
            let still = n as usize - f.picked.len();
            let len = acts.len();
            let opts: Vec<Opt> = acts
                .into_iter()
                .enumerate()
                .filter(|&(k, _)| len - k >= still)
                .map(|(_, (a, c))| Opt { action: a, choice: Choice::Card(c) })
                .collect();
            self.ask(Task::Run(f.clone()), p, DecisionKind::Sacrifice, opts);
            return Flow::Suspend;
        };
        f.picked.clear();
        if !done.is_empty() {
            self.begin_batch();
            for c in done {
                if self.objs[c.i()].zone == Zone::Battlefield {
                    self.note(|g| format!("P{p} sacrifices {}", g.name(c)));
                    self.sacrifice(c);
                }
            }
            self.end_batch();
        }
        Flow::Next
    }
}
