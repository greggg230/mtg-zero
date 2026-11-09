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
                    Some(back) if !self.is_token(c) && !self.db.def(back).is(types::INSTANT | types::SORCERY) => {
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
                    this_turn: false,
                    obj: None,
                });
                Flow::Next
            }
            Ext::Poison { who, n } => {
                if let Some(p) = self.who(f, *who) {
                    // (Etali: that many, any number: CR 122.1f.)
                    let n = crate::num::count(self.amount(f, *n) as i64);
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
            Ext::Connive { on, n } => {
                // CR 701.50a-b: the conniving permanent's controller (the
                // ability's controller if it left: last known information)
                // draws n cards, then discards n; each nonland card
                // discarded puts a +1/+1 counter on it if it is still that
                // permanent. `f.i`: 0 = to draw, then 1 + discards made;
                // `f.x`: nonland cards discarded.
                let n = self.amount(f, *n).clamp(0, 255) as u8;
                if f.i == 0 {
                    f.x = 0;
                }
                let it = self.objs_on(f, on).first().copied();
                let p = it.map_or(f.controller, |c| self.objs[c.i()].controller);
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        if !self.def_of(c).is(types::LAND) {
                            f.x += 1;
                        }
                        self.discard(c);
                        f.i += 1;
                    }
                    Some(other) => unreachable!("connive answer {other:?}"),
                    None => {}
                }
                if f.i == 0 {
                    f.i = 1;
                    let queued = self.agenda.len();
                    if self.draw_cards(p, n as u32) {
                        // (The discard after the miracle question and the
                        // rest of the draw, CR 702.94a.)
                        self.wait_for_draw(queued, f);
                        return Flow::Suspend;
                    }
                }
                if f.i > n || self.players[p as usize].hand.is_empty() {
                    if let (Some(c), true) = (it, f.x > 0) {
                        self.add_counters(c, crate::db::Counter::P1P1, crate::num::count(f.x as i64));
                    }
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
        self.draw_cards(p, n as u32);
    }

    /// Etali's "each player exiles cards from the top of their library until
    /// they exile a nonland card; you may cast any number of spells from
    /// among the nonland cards exiled this way without paying their mana
    /// costs": the exiles first (each card revealed as it moves; the nonland
    /// cards in `f.picked`), then, one at a time in any order, a card to
    /// cast (`ChooseExiled`, `PASS` = no more), each cast at once (CR
    /// 608.2g) while the rest of the effect waits.
    fn exile_until_nonland_cast_free(&mut self, f: &mut Frame) -> Flow {
        let p = f.controller;
        match self.answer.take() {
            Some(Choice::Card(card)) => {
                f.picked.retain(|&c| c != card);
                let mut back = f.clone();
                back.i = 1;
                self.agenda.push(Task::Run(back));
                let r = self.obj_ref(card);
                self.permissions.push(crate::game::Permission {
                    card: r,
                    player: p,
                    turn: Some(self.turn.number),
                    kind: crate::game::PermKind::Play { free: true, cast_only: true, any_mana: false, only_if: crate::game::PlayIf::Always },
                });
                self.begin_cast(p, card);
                return Flow::Suspend;
            }
            Some(Choice::Pass) => {
                f.picked.clear();
                return Flow::Next;
            }
            Some(other) => unreachable!("exiled-card answer {other:?}"),
            None => {}
        }
        if f.i == 0 {
            f.i = 1;
            for q in [self.turn.active, crate::ids::opp(self.turn.active)] {
                while let Some(&top) = self.players[q as usize].library.last() {
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, q);
                    self.reveal(top);
                    // "Until": one card at a time, each its own event (a
                    // "one or more cards are exiled" trigger per card:
                    // Laelia's ruling 1178d637, CR 603.2c; ENGINE.md 38).
                    self.new_event();
                    self.move_card(top, Zone::Exile, Pos::Top);
                    if !self.def_of(top).is(types::LAND) {
                        f.picked.push(top);
                        break;
                    }
                }
            }
        }
        // Asked whenever a card exiled this way is still there with a spell
        // face (public), even if none can be cast now: castability counts
        // the caster's hidden hand (a spree's mode costs paid with a Spirit
        // Guide), so whether the question comes must not tell, and a world
        // `determinize` deals for the other player must find the same
        // decision (ENGINE.md 10's second rule; 71.3).
        if !self.may_cast_during_resolution(p) || !self.exile.iter().any(|c| f.picked.contains(c) && self.has_spell_face(*c)) {
            f.picked.clear();
            return Flow::Next;
        }
        // (In exile order: `EXILE` + k = the k-th card offered.)
        let cands: Vec<CardIdx> = self
            .exile
            .iter()
            .copied()
            .filter(|c| f.picked.contains(c) && self.can_cast_free_now(p, *c))
            .take(act::EXILE_N as usize)
            .collect();
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        opts.extend(cands.into_iter().enumerate().map(|(k, c)| Opt { action: act::EXILE + k as u16, choice: Choice::Card(c) }));
        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseExiled, opts);
        Flow::Suspend
    }

    /// Could `p` cast `card` (in exile) now without paying its mana cost,
    /// during the resolution of an effect that lets them (CR 608.2g)?
    pub(crate) fn can_cast_free_now(&self, p: PlayerId, card: CardIdx) -> bool {
        self.faces_castable_now(p, card, true) != 0
    }

    /// The faces of `card` (in exile or a graveyard) `p` could cast now,
    /// during the resolution of an effect that lets them (CR 608.2g),
    /// without paying its mana cost (`free`) or for its costs: `FACE_CAST`
    /// for its front (a split card's left half), `OTHER_CAST` for its other
    /// spell face (a split card's right half, an Adventure: CR 709.3,
    /// 715.3; ruling 6187a75b). Only the face cast is evaluated.
    pub(crate) fn faces_castable_now(&self, p: PlayerId, card: CardIdx, free: bool) -> u8 {
        if !self.may_cast_during_resolution(p) {
            return 0;
        }
        let classes = self.mana_classes(p, None);
        let from = crate::game::CastFrom::Permission { free, any_mana: false };
        let def = self.def_of(card);
        let face_ok = |face: DefId| {
            if self.db.def(face).is(types::LAND) {
                return false;
            }
            let mut g = self.clone();
            g.objs[card.i()].def = face;
            g.castable_via(p, card, from, &classes, true)
        };
        let front = match def.halves {
            Some((left, _)) => face_ok(left),
            None => !def.is(types::LAND) && self.castable_via(p, card, from, &classes, true),
        };
        let other = match def.other_face() {
            Some(face) => face_ok(face),
            None => false,
        };
        (if front { FACE_CAST } else { 0 }) | (if other { OTHER_CAST } else { 0 })
    }

    /// May `p` cast a spell during a resolution at all (CR 608.2g): no
    /// "can't cast spells" and no "only any time they could cast a sorcery"
    /// (the stack isn't empty) applies. Public, unlike castability, which
    /// counts the caster's hand; `faces_castable_now`'s and `can_cast_now`'s
    /// first check, and with `has_spell_face` when "you may cast it" is
    /// asked (ENGINE.md 10's second rule, 71.3).
    pub(crate) fn may_cast_during_resolution(&self, p: PlayerId) -> bool {
        self.may_cast_spells(p) && !self.permanents_of(crate::ids::opp(p)).any(|c| self.abilities(c).opp_sorcery_speed)
    }

    /// Has `card` a face `faces_castable_now` may find castable (a spell
    /// face: not a land)? Public (its faces), unlike whether it can be cast
    /// now: the question "you may cast it" is asked on this (ENGINE.md 10's
    /// second rule, 71.3).
    pub(crate) fn has_spell_face(&self, card: CardIdx) -> bool {
        let def = self.def_of(card);
        let front = match def.halves {
            Some((left, _)) => !self.db.def(left).is(types::LAND),
            None => !def.is(types::LAND),
        };
        front || def.other_face().is_some_and(|face| !self.db.def(face).is(types::LAND))
    }

    /// Begin casting `card` with the permission just given (during a
    /// resolution): either face when both can be cast (`ChooseFace`), else
    /// the one that can (`faces_castable_now`).
    pub(crate) fn begin_cast_either_face(&mut self, p: PlayerId, card: CardIdx, free: bool) {
        let mask = self.faces_castable_now(p, card, free);
        if mask == FACE_CAST | OTHER_CAST {
            // The card will be public either way (a spell on the stack), so
            // it is revealed now, as at priority: a world `determinize`
            // samples for the other player must find the same card (a
            // hideaway card exiled face down, Shelldock Isle; ENGINE.md 10,
            // 49).
            self.reveal(card);
            self.agenda.push(crate::flow::Task::ChooseFace { p, card, mask });
        } else if mask == OTHER_CAST {
            let face = self.other_face(card).expect("a card with another face");
            self.begin_cast_face(p, card, Some(face));
        } else {
            self.begin_cast(p, card);
        }
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
        self.def_of(c).other_face()
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
        // How the card may be cast from where it is: from a graveyard, any of
        // its routes for that face (CR 601.2a-b), a granted permission
        // judged by the face being cast ("instant and sorcery cards in your
        // graveyard have retrace" reaches an adventurer creature card's
        // Adventure, not the creature: Lier's ruling of 2025-10-02, CR
        // 601.3e; `gy_routes_esc`); elsewhere, the card's route. Whether
        // that face can be cast, from the face alone.
        let routes: Vec<crate::game::CastFrom> = match zone {
            Zone::Hand => Vec::new(),
            Zone::Graveyard => self.gy_routes(p, c, Some(face)).iter().map(|r| self.route_from(c, p, *r)).collect(),
            _ => self.cast_route(p, c).into_iter().collect(),
        };
        if zone != Zone::Hand && routes.is_empty() {
            return false;
        }
        // (Timing first, from the face alone: it saves the copy below.)
        if !sorcery_ok && !self.instant_speed(p, self.db.def(face)) {
            return false;
        }
        let mut g = self.clone();
        g.objs[c.i()].def = face;
        if zone == Zone::Hand {
            return g.hand_castable(p, c, sorcery_ok, classes, &mut None);
        }
        routes.into_iter().any(|from| g.castable_via(p, c, from, classes, sorcery_ok))
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
        self.players[p as usize].lands_played = self.players[p as usize].lands_played.saturating_add(1);
        self.entering_face = face;
        // From a graveyard without a "play lands from your graveyard"
        // permanent: Serra Paragon's permission.
        let paragon = if self.objs[c.i()].zone == Zone::Graveyard && !self.lands_from_graveyard(p) {
            self.paragon(p, 0, true).and_then(|s| self.use_paragon(s))
        } else {
            None
        };
        // "Each land played this way enters tapped" (an exile permission,
        // ENGINE.md 99.4).
        let tapped = matches!(self.objs[c.i()].zone, Zone::Exile | Zone::FaceDown)
            && matches!(self.exile_permission(p, c), Some(crate::game::PermKind::TaxedLandsTapped(_)));
        self.entering_tapped |= tapped;
        let r = self.put_onto_battlefield(c, p);
        if tapped {
            self.entering_tapped = false;
        }
        if let Some((def, slot)) = paragon {
            if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield {
                self.marks.push((r, crate::game::Mark::LeaveTrigger { def, slot, exile: false }));
            }
        }
        self.note(|g| format!("P{p} plays {}", g.name(c)));
        // "Whenever a player plays a land" (City of Traitors).
        self.event(crate::triggers::Event {
            kind: crate::db::EventKind::PlaysLand,
            a: crate::triggers::Subj::Obj(r),
            b: crate::triggers::Subj::Player(p),
            amount: 0,
        });
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
    /// A double-faced card that is copying something transforms too
    /// (701.27c is about permanents that aren't transforming double-faced
    /// cards): its own face turns over while the copy effect still gives it
    /// its characteristics (layer 1, 707.2), and it shows that face when the
    /// effect ends.
    pub(crate) fn transform(&mut self, c: CardIdx, day_night: bool) -> bool {
        let o = &self.objs[c.i()];
        if o.zone != Zone::Battlefield {
            return false;
        }
        let r = self.obj_ref(c);
        // A token created as a copy of a double-faced permanent is a
        // double-faced token (CR 707.8a: its copy effect follows the face).
        let created = self.is_copy_obj(c)
            && self.copies.iter().filter(|x| x.obj == r).all(|x| x.until == crate::layers::Until::Forever && x.own == x.vals.def);
        let copying = self.is_copy(c) && !created;
        // (The face it has: its own, under a copy effect.)
        let own = if copying { self.own_def(c) } else { o.def };
        let d = self.db.def(own);
        let Some(other) = d.back else { return false };
        if self.db.def(other).is(types::INSTANT | types::SORCERY) {
            return false;
        }
        if self.is_copy_obj(c) && !created {
            // (A copy of a card or a token copy whose copy effect changed:
            // not a double-faced card, 701.27c.)
            return false;
        }
        if created {
            for x in self.copies.iter_mut().filter(|x| x.obj == r) {
                x.vals.def = other;
                x.own = other;
            }
        } else if d.token && d.front != Some(other) && self.db.def(other).front != Some(own) {
            // (A token transforms only if it is double-faced: the Incubator,
            // whose back face's front is the token, ENGINE.md 28.)
            return false;
        }
        let a = self.abilities(c);
        if (a.daybound || a.nightbound) && !day_night {
            return false;
        }
        if copying {
            for x in self.copies.iter_mut().filter(|x| x.obj == r) {
                x.own = other;
            }
        } else {
            self.set_face(c, other);
        }
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
                // (Daybound: "as it becomes night, if this permanent is front
                // face up, transform it"; nightbound: "... day, if back face
                // up" (CR 702.145b, e): its own face, under a copy effect.)
                let a = self.abilities(c);
                let back = self.db.def(self.own_def(c)).front.is_some();
                (a.daybound && !day && !back) || (a.nightbound && day && back)
            })
            .collect();
        for c in flip {
            self.transform(c, true);
        }
    }

    /// CR 702.145c-d, f-g for permanent `c`, which just entered, once it is
    /// on the battlefield: after its `Enters` event, so it is seen entering
    /// with the face it entered with. A daybound (nightbound) permanent makes
    /// it day (night) if it is neither; a double-faced token with daybound
    /// entering front face up at night (it doesn't enter transformed: that
    /// needs a double-faced card, 702.145b) transforms now.
    pub(crate) fn day_night_entered(&mut self, c: CardIdx) {
        let d = self.def_of(c);
        if (d.daybound || d.nightbound) && self.objs[c.i()].zone == Zone::Battlefield {
            self.day_night_check();
        }
    }

    /// CR 702.145c-d and f-g apply "any time", not only as a permanent
    /// enters or as day and night change: after something that can give a
    /// permanent daybound or nightbound (a copy effect starting or ending,
    /// an effect that took its abilities ending) or turn a face up without
    /// its entering (turned face up, 712.12a; phasing in, 702.26d). Checked
    /// when a daybound or nightbound face is on the battlefield
    /// (`day_night_check` reads the abilities each permanent has now).
    pub(crate) fn day_night_any_time(&mut self) {
        if self.battlefield.iter().any(|&c| {
            let d = self.def_of(c);
            d.daybound || d.nightbound
        }) {
            self.day_night_check();
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
    pub(crate) fn chapters_reached(&mut self, c: CardIdx, before: crate::num::Count, after: crate::num::Count) {
        let last = self.abilities(c).chapters as crate::num::Count;
        let r = self.obj_ref(c);
        for n in before.saturating_add(1)..=after.min(last) {
            self.event(Event { kind: EventKind::Chapter, a: Subj::Obj(r), b: Subj::None, amount: n });
        }
    }

    /// CR 704.5s, 714.4: Sagas with at least their final chapter number of
    /// lore counters that aren't the source of a chapter ability that has
    /// triggered and not yet left the stack.
    pub(crate) fn finished_sagas(&self) -> Vec<CardIdx> {
        let mut out = Vec::new();
        for &c in &self.battlefield {
            // (A Saga is an enchantment, CR 714.1.)
            let last = self.abilities(c).chapters as crate::num::Count;
            if last == 0 || self.counters_on(c, Counter::Lore) < last || !self.is_type(c, types::ENCHANTMENT) {
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
