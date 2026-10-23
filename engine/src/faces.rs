//! Cards with more than one face (M7b part 2; ENGINE.md 25): double-faced
//! cards and transforming (CR 701.27, 712), modal double-faced cards
//! (712.3, 712.11-12), Adventures (715), Sagas (714), day and night (731,
//! 702.145).
//!
//! A face is a definition of its own (`CardDb`, after the tokens) that
//! knows its card (`CardDef.front`). An object's `def` is the face that is
//! up: the front face (the card's definition) everywhere but the
//! battlefield and the stack (712.8a, 715.4; `move_card` puts it back), the
//! face cast or played on the stack and the battlefield, the other face once
//! a permanent transforms (the same object, 712.18).

use crate::db::{types, Counter, EventKind};
use crate::decision::{act, Choice, DecisionKind, Opt, FACE_CAST, FACE_PLAY, OTHER_CAST, OTHER_PLAY};
use crate::flow::Task;
use crate::game::{Game, Pos, StackKind};
use crate::ids::{CardIdx, DefId, PlayerId, Zone};
use crate::program::{Ext, On};
use crate::resolve::{Flow, Frame};
use crate::triggers::{Event, Subj};

/// The priority option for a card whose faces allow `mask` (`FACE_*`
/// bits): nothing, one face's action, or a face choice to follow.
pub(crate) fn face_choice(c: CardIdx, mask: u8) -> Option<Choice> {
    match mask {
        0 => None,
        FACE_CAST => Some(Choice::Cast(c)),
        FACE_PLAY => Some(Choice::Play(c)),
        OTHER_CAST => Some(Choice::CastFace(c)),
        OTHER_PLAY => Some(Choice::PlayFace(c)),
        m => Some(Choice::Faces(c, m)),
    }
}

impl Game {
    /// The face-related `Ext` instructions (`program.rs`).
    pub(crate) fn exec_faces(&mut self, f: &mut Frame, op: &Ext) -> Flow {
        match op {
            Ext::TransformSource => {
                // CR 701.27f: not if it transformed since the ability was put
                // on the stack (even if it transformed back).
                if let Some(&c) = self.objs_on(f, &On::Source).first() {
                    if self.objs[c.i()].flips == f.flips {
                        self.transform(c, false);
                    }
                }
                Flow::Next
            }
            Ext::ExileReturnTransformed { yours } => {
                let Some(&c) = self.objs_on(f, &On::Source).first() else { return Flow::Next };
                let owner = self.objs[c.i()].owner;
                self.move_card(c, Zone::Exile, Pos::Top);
                // CR 712.14a: only a double-faced card returns transformed;
                // anything else stays in exile (a token, too, ceases to
                // exist there, 704.5d).
                let d = self.def_of(c);
                match d.back {
                    Some(back) if !d.token && !self.db.def(back).is(types::INSTANT | types::SORCERY) => {
                        self.entering_face = Some(back);
                        let ctl = if *yours { f.controller } else { owner };
                        self.put_onto_battlefield(c, ctl);
                        f.made = Some(c);
                    }
                    _ => {}
                }
                Flow::Next
            }
            Ext::OnceMay { skip } => {
                let slot = match f.ability.slot {
                    crate::db::AbilitySlot::Trigger(i) => i,
                    _ => u8::MAX,
                };
                let key = (f.source, slot);
                if self.done_once.contains(&key) {
                    return Flow::Jump(*skip);
                }
                match self.answer.take() {
                    Some(Choice::Yes) => {
                        self.done_once.push(key);
                        Flow::Next
                    }
                    Some(_) => Flow::Jump(*skip),
                    None => {
                        let opts = vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                        self.ask(Task::Run(f.clone()), f.controller, DecisionKind::YesNo, opts);
                        Flow::Suspend
                    }
                }
            }
            Ext::GraveyardCastThenExile { tgt } => {
                for t in self.targets_of(f, *tgt) {
                    let crate::ids::Target::Obj(o) = t else { continue };
                    if self.objs[o.idx.i()].zone == Zone::Graveyard {
                        self.permissions.push(crate::game::Permission {
                            card: o,
                            player: f.controller,
                            turn: Some(self.turn.number),
                            kind: crate::game::PermKind::CastThenExile,
                        });
                    }
                }
                Flow::Next
            }
            Ext::TriggerUntilYourNextTurn { k } => {
                let slot = self.db.def(f.source_def).delayed_base + k;
                self.temp_triggers.push(crate::game::TempTrigger {
                    controller: f.controller,
                    source: f.source,
                    source_def: f.source_def,
                    slot,
                });
                Flow::Next
            }
            Ext::Poison { who, n } => {
                if let Some(p) = self.who(f, *who) {
                    let n = self.amount(f, *n).max(0) as u16;
                    let pl = &mut self.players[p as usize];
                    pl.poison = pl.poison.saturating_add(n);
                    self.note(|_| format!("P{p} gets {n} poison counter(s)"));
                }
                Flow::Next
            }
            Ext::DiscardUpToThenDraw { n } => {
                // `f.i`: the cards discarded so far (one decision each).
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        self.discard(c);
                        f.i += 1;
                    }
                    Some(Choice::Pass) => {
                        self.draw_n(p, f.i);
                        return Flow::Next;
                    }
                    Some(other) => unreachable!("discard answer {other:?}"),
                    None => {}
                }
                if f.i >= *n || self.players[p as usize].hand.is_empty() {
                    self.draw_n(p, f.i);
                    return Flow::Next;
                }
                let mut opts = self.hand_options(p);
                opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                Flow::Suspend
            }
            Ext::EachExileUntilNonlandCastFree => self.exile_until_nonland_cast_free(f),
            Ext::KeepOneOfEachType { types, nonland } => {
                // `f.i`: the next card type; `f.picked`: the kept permanents.
                let q = crate::ids::opp(f.controller);
                if self.shielded_from(q, f.controller) {
                    return Flow::Next; // (Tamiyo, Collector of Tales)
                }
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        if !f.picked.contains(&c) {
                            f.picked.push(c);
                        }
                        f.i += 1;
                    }
                    Some(other) => unreachable!("keep answer {other:?}"),
                    None => {}
                }
                let candidates = |g: &Game, bits: u16| -> Vec<CardIdx> {
                    g.permanents_of(q).filter(|&c| g.types_of(c) & bits != 0 && !(*nonland && g.is_type(c, types::LAND))).collect()
                };
                while (f.i as usize) < types.len() {
                    let cands = candidates(self, types[f.i as usize]);
                    if cands.is_empty() {
                        f.i += 1;
                        continue;
                    }
                    let opts = cands.into_iter().map(|c| Opt { action: self.perm_action(q, c), choice: Choice::Card(c) }).collect();
                    self.ask(Task::Run(f.clone()), q, DecisionKind::ChoosePermanent, opts);
                    return Flow::Suspend;
                }
                // Then the rest are sacrificed at once.
                let rest: Vec<CardIdx> = self
                    .permanents_of(q)
                    .filter(|&c| !f.picked.contains(&c) && !(*nonland && self.is_type(c, types::LAND)))
                    .collect();
                f.picked.clear();
                self.begin_batch();
                for c in rest {
                    self.sacrifice(c);
                }
                self.end_batch();
                Flow::Next
            }
            Ext::Connive => {
                // CR 701.50a-b: the source's controller (as it last existed)
                // draws, then discards; a nonland card discarded puts a
                // +1/+1 counter on it if it is still that permanent.
                let p = f.controller;
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        let nonland = !self.def_of(c).is(types::LAND);
                        self.discard(c);
                        if nonland {
                            if let Some(&s) = self.objs_on(f, &On::Source).first() {
                                self.add_counters(s, crate::db::Counter::P1P1, 1);
                            }
                        }
                        return Flow::Next;
                    }
                    Some(other) => unreachable!("connive answer {other:?}"),
                    None => {}
                }
                if f.i == 0 {
                    f.i = 1;
                    self.draw(p);
                }
                if self.players[p as usize].hand.is_empty() {
                    return Flow::Next;
                }
                let opts = self.hand_options(p);
                self.ask(Task::Run(f.clone()), p, DecisionKind::Discard, opts);
                Flow::Suspend
            }
            other => unreachable!("not a face instruction: {other:?}"),
        }
    }

    fn draw_n(&mut self, p: PlayerId, n: u8) {
        for _ in 0..n {
            self.draw(p);
        }
    }

    /// Etali's "each player exiles cards from the top of their library until
    /// they exile a nonland card; you may cast any number of spells from
    /// among the nonland cards exiled this way without paying their mana
    /// costs": the exiles first (each card revealed as it moves), then one
    /// yes/no per nonland card in APNAP order (`f.picked`), each cast at
    /// once (CR 608.2g) while the rest of the effect waits.
    fn exile_until_nonland_cast_free(&mut self, f: &mut Frame) -> Flow {
        let p = f.controller;
        match self.answer.take() {
            Some(Choice::Yes) => {
                let card = f.picked.remove(0);
                let mut back = f.clone();
                back.i = 1;
                self.agenda.push(Task::Run(back));
                let r = self.obj_ref(card);
                self.permissions.push(crate::game::Permission {
                    card: r,
                    player: p,
                    turn: Some(self.turn.number),
                    kind: crate::game::PermKind::Play { free: true, cast_only: true, any_mana: false, attacked_subtype: None },
                });
                self.begin_cast(p, card);
                return Flow::Suspend;
            }
            Some(_) => {
                f.picked.remove(0);
            }
            None => {}
        }
        if f.i == 0 {
            f.i = 1;
            for q in [self.turn.active, crate::ids::opp(self.turn.active)] {
                while let Some(&top) = self.players[q as usize].library.last() {
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, q);
                    self.reveal(top);
                    self.move_card(top, Zone::Exile, Pos::Top);
                    if !self.def_of(top).is(types::LAND) {
                        f.picked.push(top);
                        break;
                    }
                }
            }
        }
        while let Some(&card) = f.picked.first() {
            if self.objs[card.i()].zone == Zone::Exile && self.can_cast_free_now(p, card) {
                let opts = vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                return Flow::Suspend;
            }
            f.picked.remove(0);
        }
        Flow::Next
    }

    /// Could `p` cast `card` (in exile) now without paying its mana cost,
    /// during the resolution of an effect that lets them (CR 608.2g)?
    pub(crate) fn can_cast_free_now(&self, p: PlayerId, card: CardIdx) -> bool {
        if self.def_of(card).is(types::LAND) || !self.may_cast_spells(p) {
            return false;
        }
        if self.permanents_of(crate::ids::opp(p)).any(|c| self.abilities(c).opp_sorcery_speed) {
            return false;
        }
        let classes = self.mana_classes(p, None);
        let from = crate::game::CastFrom::Permission { free: true, any_mana: false };
        self.castable_via(p, card, from, &classes, true)
    }

    /// "You have no maximum hand size" (CR 402.2): a permanent or emblem of
    /// `p` says so.
    pub(crate) fn no_max_hand(&self, p: PlayerId) -> bool {
        self.permanents_of(p).chain(self.command.iter().copied().filter(|&e| self.objs[e.i()].controller == p)).any(|c| self.abilities(c).no_max_hand)
    }

    /// Does permanent `c` have activated ability `i` of its definition now?
    /// A latent one only once an effect gave it (`Mod::GainLatent`).
    #[inline]
    pub(crate) fn has_activated(&self, c: CardIdx, i: usize) -> bool {
        let d = self.abilities(c);
        let base = d.activated.len() - d.latent as usize;
        if i < base {
            return true;
        }
        let r = self.obj_ref(c);
        let k = (i - base) as u8;
        self.effects.iter().any(|e| e.on == crate::layers::Affected::Obj(r) && e.m == crate::db::Mod::GainLatent(k))
    }

    /// The face card `c` may be cast or played with instead of its front:
    /// a modal double-faced card's back face (CR 712.11b, 712.12) or an
    /// adventurer card's Adventure (715.3).
    pub(crate) fn other_face(&self, c: CardIdx) -> Option<DefId> {
        let d = self.def_of(c);
        if let Some(h) = d.halves {
            return Some(h.1); // a split card's right half (CR 709.3)
        }
        if d.modal {
            d.back
        } else {
            d.adventure
        }
    }

    /// Is `def` an Adventure (CR 715.2)?
    pub(crate) fn is_adventure(&self, def: DefId) -> bool {
        self.db.def(def).front.is_some_and(|f| self.db.def(f).adventure == Some(def))
    }

    /// What card `c`'s other face lets `p` do now from where the card is
    /// (`OTHER_CAST`, `OTHER_PLAY` or 0): a land face is played where a
    /// land may be (`land_ok`); a spell face is cast if that face, alone,
    /// could be (CR 712.11c, 715.3a: only the face cast is evaluated).
    pub(crate) fn other_face_options(
        &self,
        p: PlayerId,
        c: CardIdx,
        sorcery_ok: bool,
        land_ok: bool,
        classes: &[crate::pay::Class],
    ) -> u8 {
        let Some(face) = self.other_face(c) else { return 0 };
        // CR 715.3d: the permission of a card on an adventure doesn't let
        // it be cast as an Adventure again.
        let zone = self.objs[c.i()].zone;
        if zone == Zone::Exile && self.exile_permission(p, c) == Some(crate::game::PermKind::Adventure) {
            return 0;
        }
        if self.db.def(face).is(types::LAND) {
            return if land_ok { OTHER_PLAY } else { 0 };
        }
        if self.face_castable(p, c, face, sorcery_ok, classes) {
            OTHER_CAST
        } else {
            0
        }
    }

    /// `FACE_CAST` if `p` may cast split card `c`'s left half now (a split
    /// card is always cast as one of its halves, CR 709.3).
    pub(crate) fn split_left_options(&self, p: PlayerId, c: CardIdx, sorcery_ok: bool, classes: &[crate::pay::Class]) -> u8 {
        match self.def_of(c).halves {
            Some((left, _)) if self.face_castable(p, c, left, sorcery_ok, classes) => FACE_CAST,
            _ => 0,
        }
    }

    /// Can `p` cast card `c` with face `face` up now (a spell face)?
    fn face_castable(&self, p: PlayerId, c: CardIdx, face: DefId, sorcery_ok: bool, classes: &[crate::pay::Class]) -> bool {
        let zone = self.objs[c.i()].zone;
        // How the card may be cast from where it is comes from the card as
        // it is there (its front: "instant and sorcery cards in your
        // graveyard have retrace" doesn't reach an adventurer creature card,
        // CR 715.4); whether that face can be cast, from the face alone.
        let route = if zone == Zone::Hand { None } else { self.cast_route(p, c) };
        if zone != Zone::Hand && route.is_none() {
            return false;
        }
        // (Timing first, from the face alone: it saves the copy below.)
        if !sorcery_ok && !self.instant_speed(p, self.db.def(face)) {
            return false;
        }
        let mut g = self.clone();
        g.objs[c.i()].def = face;
        match route {
            None => g.hand_castable(p, c, sorcery_ok, classes, &mut None),
            Some(from) => g.castable_via(p, c, from, classes, sorcery_ok),
        }
    }

    /// CR 712.11b, 712.12, 715.3: which face of `card` `p` casts or plays
    /// (`NUMBER` 0 = the front, 1 = the other face), among those `mask`
    /// allows (computed at priority, kept in the task so a refreshed
    /// decision asks the same question).
    pub(crate) fn choose_face(&mut self, p: PlayerId, card: CardIdx, mask: u8) {
        match self.answer.take() {
            Some(Choice::Number(0)) => {
                if mask & FACE_PLAY != 0 {
                    self.play_land(p, card, None);
                } else {
                    self.begin_cast(p, card);
                }
            }
            Some(Choice::Number(_)) => {
                if mask & OTHER_PLAY != 0 {
                    self.play_face(p, card);
                } else {
                    let face = self.other_face(card).expect("a card with another face");
                    self.begin_cast_face(p, card, Some(face));
                }
            }
            Some(other) => unreachable!("face answer {other:?}"),
            None => {
                let mut opts = Vec::new();
                if mask & (FACE_CAST | FACE_PLAY) != 0 {
                    opts.push(Opt { action: act::NUMBER, choice: Choice::Number(0) });
                }
                if mask & (OTHER_CAST | OTHER_PLAY) != 0 {
                    opts.push(Opt { action: act::NUMBER + 1, choice: Choice::Number(1) });
                }
                self.ask(Task::ChooseFace { p, card, mask }, p, DecisionKind::ChooseFace, opts);
            }
        }
    }

    /// Play land card `c` (CR 305.1, a special action; from a hand, or from
    /// another zone with a permission) with face `face` up (an MDFC's land
    /// face, 712.12; `None` = its front).
    pub(crate) fn play_land(&mut self, p: PlayerId, c: CardIdx, face: Option<DefId>) {
        self.players[p as usize].lands_played += 1;
        self.entering_face = face;
        self.put_onto_battlefield(c, p);
        self.note(|g| format!("P{p} plays {}", g.name(c)));
    }

    /// Play card `c`'s other face, a land (CR 712.12).
    pub(crate) fn play_face(&mut self, p: PlayerId, c: CardIdx) {
        let face = self.other_face(c).expect("a card with a land face");
        self.play_land(p, c, Some(face));
    }

    /// Turn face `def` up on `c` (a permanent: the same object, CR 712.18),
    /// keeping the static-ability fast paths right.
    pub(crate) fn set_face(&mut self, c: CardIdx, def: DefId) {
        let old = self.db.def(self.objs[c.i()].def);
        let new = self.db.def(def);
        if self.objs[c.i()].zone == Zone::Battlefield {
            match (old.statics.is_empty(), new.statics.is_empty()) {
                (true, false) => self.n_static += 1,
                (false, true) => self.n_static -= 1,
                _ => {}
            }
            match (crate::layers::self_typed(old), crate::layers::self_typed(new)) {
                (false, true) => self.n_self_types += 1,
                (true, false) => self.n_self_types -= 1,
                _ => {}
            }
        }
        self.objs[c.i()].def = def;
    }

    /// Transform permanent `c` (CR 701.27a): turn its other face up. Only a
    /// double-faced card can (701.27c: a token can't); not into an instant
    /// or sorcery face (701.27d); a daybound or nightbound permanent only
    /// through day and night (`day_night`, 702.145b, e). It stays the same
    /// object, with its counters, damage and attachments (712.18), and a
    /// permanent that becomes a planeswalker this way gets no loyalty
    /// counters (306.5b is about entering). Returns whether it transformed.
    pub(crate) fn transform(&mut self, c: CardIdx, day_night: bool) -> bool {
        let o = &self.objs[c.i()];
        if o.zone != Zone::Battlefield {
            return false;
        }
        let d = self.db.def(o.def);
        let Some(other) = d.back else { return false };
        if d.token || self.db.def(other).is(types::INSTANT | types::SORCERY) {
            return false;
        }
        let a = self.abilities(c);
        if (a.daybound || a.nightbound) && !day_night {
            return false;
        }
        self.set_face(c, other);
        let o = &mut self.objs[c.i()];
        o.flips = o.flips.wrapping_add(1);
        self.note(|g| format!("{} transforms", g.name(c)));
        let r = self.obj_ref(c);
        self.event(Event::obj(EventKind::Transforms, r));
        true
    }

    /// Day or night (CR 731.1), if the game has one.
    pub fn day(&self) -> Option<bool> {
        self.day
    }

    /// CR 702.145c-g, checked whenever a daybound or nightbound permanent
    /// appears and when day and night change: with neither, a daybound
    /// permanent makes it day (a nightbound one, with no daybound one,
    /// night); at night daybound permanents transform, by day nightbound
    /// ones do (at once, not a state-based action).
    pub(crate) fn day_night_check(&mut self) {
        if self.day.is_none() {
            let any = |g: &Game, night: bool| {
                g.battlefield.iter().any(|&c| if night { g.abilities(c).nightbound } else { g.abilities(c).daybound })
            };
            if any(self, false) {
                self.day = Some(true);
                self.note(|_| "it becomes day".to_string());
            } else if any(self, true) {
                self.day = Some(false);
                self.note(|_| "it becomes night".to_string());
            }
        }
        let Some(day) = self.day else { return };
        let flip: Vec<CardIdx> = self
            .battlefield
            .iter()
            .copied()
            .filter(|&c| {
                let a = self.abilities(c);
                (a.daybound && !day) || (a.nightbound && day)
            })
            .collect();
        for c in flip {
            self.transform(c, true);
        }
    }

    /// CR 731.2: as the untap step's second part, day becomes night if the
    /// previous turn's active player cast no spells that turn, night
    /// becomes day if they cast two or more.
    pub(crate) fn day_night_turn(&mut self, spells: u8) {
        let next = match self.day {
            Some(true) if spells == 0 => false,
            Some(false) if spells >= 2 => true,
            _ => return,
        };
        self.day = Some(next);
        self.note(|_| format!("it becomes {}", if next { "day" } else { "night" }));
        self.day_night_check();
    }

    /// CR 714.3c: `p` puts a lore counter on each Saga they control with
    /// one or more chapter abilities.
    pub(crate) fn lore_turn_action(&mut self, p: PlayerId) {
        let sagas: Vec<CardIdx> = self.permanents_of(p).filter(|&c| self.abilities(c).chapters > 0).collect();
        for c in sagas {
            self.add_counters(c, Counter::Lore, 1);
        }
    }

    /// Lore counters on Saga `c` went from `before` to `after`: each chapter
    /// ability N with before < N <= after triggers (CR 714.2b).
    pub(crate) fn chapters_reached(&mut self, c: CardIdx, before: u16, after: u16) {
        let last = self.abilities(c).chapters as u16;
        let r = self.obj_ref(c);
        for n in before + 1..=after.min(last) {
            self.event(Event { kind: EventKind::Chapter, a: Subj::Obj(r), b: Subj::None, amount: n });
        }
    }

    /// CR 704.5s, 714.4: Sagas with at least their final chapter number of
    /// lore counters that aren't the source of a chapter ability that has
    /// triggered and not yet left the stack.
    pub(crate) fn finished_sagas(&self) -> Vec<CardIdx> {
        let mut out = Vec::new();
        for &c in &self.battlefield {
            let last = self.abilities(c).chapters as u16;
            if last == 0 || self.counters_on(c, Counter::Lore) < last {
                continue;
            }
            let r = self.obj_ref(c);
            let chapter = |def: DefId, slot: u8| self.db.def(def).triggers.get(slot as usize).is_some_and(|t| t.event == EventKind::Chapter);
            let waiting = self.waiting.iter().any(|w| w.source == r && chapter(w.source_def, w.slot));
            let stacked = self.stack.iter().any(|s| match s.kind {
                StackKind::Ability { ability, source } => {
                    source == r && matches!(ability.slot, crate::db::AbilitySlot::Trigger(i) if chapter(ability.def, i))
                }
                StackKind::Spell { .. } => false,
            });
            if !waiting && !stacked {
                out.push(c);
            }
        }
        out
    }
}
