//! Copies (CR 707; M7b part 3, ENGINE.md 26): copy effects on permanents
//! (layer 1, 613.2a), tokens that are copies, permanents that enter as
//! copies, and copies of spells.
//!
//! A permanent's `Obj::def` is the definition whose copiable values it has,
//! as for a face (ENGINE.md 25: the definition every rule reads): a copy
//! effect turns the copied face up, and the other layers apply on top
//! (613.2). `Game::copies` holds the copy effects in timestamp order; the
//! latest one on an object gives its copiable values (the copied face and
//! the copy effect's exceptions, 707.9) and remembers the definition the
//! object has without copy effects (`own`: what it reverts to when they end,
//! and what it is for identity: a Phyrexian Metamorph copying a Grizzly
//! Bears is still the Metamorph card). The exceptions ("except it's an
//! artifact in addition to its other types", "his name is ...", "it's 1/1")
//! are read by the characteristic accessors (`layers.rs`).
//!
//! Tokens that are copies and copies of spells are new object slots that are
//! not cards (`Game::copy_objs`): a token copy's definition is the copied
//! face (so it has the identity of the card it copies, `az/identity.rs`), a
//! spell copy's the copied spell's face. A spell copy leaves the stack only
//! to become a token (608.3f) or to cease to exist (707.10a, 704.5e).

use crate::db::{types, KwSet};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, StackItem, StackKind};
use crate::ids::{opp, CardIdx, DefId, ObjRef, PlayerId, Target, Zone};
use crate::layers::Until;
use crate::program::{CopyOf, SpellOf};
use crate::resolve::Frame;
use crate::triggers::{Event, Subj};

/// The exceptions a copy effect makes to the copying (CR 707.9a-b): part of
/// the copy's copiable values, so a copy of the copy has them too.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct Except {
    /// Card types "in addition to its other types".
    pub types: u16,
    /// Subtypes in addition (interned ids).
    pub subtypes: [Option<u16>; 4],
    /// Keyword abilities it has (707.9a: "except it has haste").
    pub kws: KwSet,
    /// Its base power and toughness (707.9b; 707.9d: then no
    /// characteristic-defining ability sets them).
    pub pt: Option<(i8, i8)>,
    /// Its name is this definition's ("his name is Superior Spider-Man").
    pub name: Option<DefId>,
    /// Its colours are exactly these (eternalize's "it's black", CR
    /// 702.129a).
    pub colors: Option<crate::mana::ManaMask>,
    /// It has no mana cost (eternalize; its mana value is 0, 202.3a).
    pub no_cost: bool,
    /// Supertypes in addition (`supertypes::*` bits: "except it's
    /// legendary", CR 707.9b; Adagia).
    pub supers: u8,
}

impl Except {
    pub fn is_empty(&self) -> bool {
        *self == Except::default()
    }

    pub fn has_subtype(&self, s: u16) -> bool {
        self.subtypes.contains(&Some(s))
    }

    /// Add a subtype; false if there is no room (the lowering checks the
    /// printed exceptions fit).
    pub fn add_subtype(&mut self, s: u16) -> bool {
        if self.has_subtype(s) {
            return true;
        }
        match self.subtypes.iter_mut().find(|x| x.is_none()) {
            Some(slot) => {
                *slot = Some(s);
                true
            }
            None => false,
        }
    }

    /// These exceptions applied to copiable values that already have
    /// `base`'s (a copy of a copy, CR 707.9b, 707.3): types, subtypes and
    /// keywords add up; a later name or base power and toughness wins.
    pub fn over(&self, base: Except) -> Except {
        let mut e = base;
        e.types |= self.types;
        for &s in self.subtypes.iter().flatten() {
            let fits = e.add_subtype(s);
            debug_assert!(fits, "too many subtypes in a copy's exceptions");
        }
        e.kws.0 |= self.kws.0;
        if self.pt.is_some() {
            e.pt = self.pt;
        }
        if self.name.is_some() {
            e.name = self.name;
        }
        if self.colors.is_some() {
            e.colors = self.colors;
        }
        e.no_cost |= self.no_cost;
        e.supers |= self.supers;
        e
    }
}

/// Copiable values (CR 707.2): the definition copied (a face, 707.8) and the
/// exceptions of the copy effects that made them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CopyVals {
    pub def: DefId,
    pub exc: Except,
}

/// A copy effect on a permanent (CR 707, layer 1a).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CopyFx {
    /// The permanent (the effect ends when it changes zones, CR 400.7).
    pub obj: ObjRef,
    /// CR 613.7.
    pub ts: u32,
    /// `Forever`: entering as a copy, a token created as a copy;
    /// `EndOfTurn`: "becomes a copy ... until end of turn".
    pub until: Until,
    pub vals: CopyVals,
    /// The definition the permanent has without copy effects.
    pub own: DefId,
}

/// `Event::amount` of an `IsCast` event for a copy of a spell (CR 707.10:
/// not cast; "whenever you cast or copy" sees it, `TrigPred::OrCopy`). A
/// cast spell's event amount is the number of spells cast before it this
/// turn (storm), never this.
pub(crate) const COPIED: u32 = u32::MAX;

/// At most this many copies a turn by "may copy this spell" effects. Chain
/// of Smog's copies cost nothing, so players who always copy would loop
/// forever; the CR has a loop of optional actions repeated a chosen number
/// of times (732.2a) and broken when it comes back to the same game state
/// (732.3). 31 (the `NUMBER` range) keeps the drain loop of Chain of Smog
/// and a magecraft creature lethal (ENGINE.md 26.11).
pub(crate) const MAY_COPY_CAP: u8 = 31;

impl Game {
    /// The copy effect that gives `c` its copiable values now (the latest),
    /// if any.
    #[inline]
    pub(crate) fn copy_fx(&self, c: CardIdx) -> Option<&CopyFx> {
        if self.copies.is_empty() {
            return None;
        }
        let r = self.obj_ref(c);
        self.copies.iter().rev().find(|x| x.obj == r)
    }

    /// Is a copy effect applying to `c` (CR 707)?
    pub fn is_copy(&self, c: CardIdx) -> bool {
        self.copy_fx(c).is_some()
    }

    /// The definition `c` has without copy effects: its face up (ENGINE.md
    /// 25), or, for a token created as a copy, the face it was created as.
    #[inline]
    pub fn own_def(&self, c: CardIdx) -> DefId {
        match self.copy_fx(c) {
            Some(x) => x.own,
            None => self.objs[c.i()].def,
        }
    }

    /// The exceptions in `c`'s copiable values, if it has any.
    #[inline]
    pub(crate) fn exc(&self, c: CardIdx) -> Option<&Except> {
        self.copy_fx(c).map(|x| &x.vals.exc).filter(|e| !e.is_empty())
    }

    /// Its supertypes (CR 205.4): its definition's and a copy's "except
    /// it's legendary" (707.9b).
    pub fn supertypes_of(&self, c: CardIdx) -> u8 {
        self.def_of(c).supertypes | self.exc(c).map_or(0, |e| e.supers)
    }

    /// Is `c` a slot made as a copy, not a card: a token copy (CR 111.10), a
    /// copy of a spell (707.10), a copy of a card (722.3c)?
    #[inline]
    pub fn is_copy_obj(&self, c: CardIdx) -> bool {
        !self.copy_objs.is_empty() && self.copy_objs.binary_search(&c).is_ok()
    }

    /// Is `c` a token (CR 111)? Token-ness is not a copiable value: a card
    /// copying a token isn't one, a token copying a card is.
    #[inline]
    pub fn is_token(&self, c: CardIdx) -> bool {
        self.db.def(self.own_def(c)).token || self.is_copy_obj(c)
    }

    /// `c`'s copiable values now (CR 707.2): the latest copy effect's, else
    /// its face up (a bestowed Aura's card: being bestowed isn't copied,
    /// 702.103b is not a copy effect).
    pub fn copiable(&self, c: CardIdx) -> CopyVals {
        if let Some(x) = self.copy_fx(c) {
            return x.vals;
        }
        let d = self.objs[c.i()].def;
        let dd = self.db.def(d);
        let d = if dd.bestowed { dd.card_def(d) } else { d };
        CopyVals { def: d, exc: Except::default() }
    }

    /// The copiable values of permanent `r`: now, or as it last existed on
    /// the battlefield (CR 608.2h, 111.12).
    pub(crate) fn copiable_of(&self, r: ObjRef) -> Option<CopyVals> {
        if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield {
            return Some(self.copiable(r.idx));
        }
        self.lki_copy.iter().rev().find(|x| x.0 == r).map(|x| x.1)
    }

    /// `c` becomes a copy with these copiable values until `until` (CR
    /// 707.2, 613.2a). It stays the same object (707.4: no enter or leave
    /// triggers); noncopy effects, counters and status stay.
    pub(crate) fn add_copy_effect(&mut self, c: CardIdx, vals: CopyVals, until: Until) {
        let own = self.own_def(c);
        self.next_ts += 1;
        let ts = self.next_ts;
        let obj = self.obj_ref(c);
        self.copies.push(CopyFx { obj, ts, until, vals, own });
        self.set_face(c, vals.def);
        self.note(|g| format!("{} becomes a copy of {} ({until:?})", g.db.def(own).name, g.db.def(vals.def).name));
    }

    /// The copy effects lasting `until` end (CR 514.2 for "until end of
    /// turn"): each permanent has the copiable values of the latest
    /// remaining one, or its own definition.
    pub(crate) fn end_copy_effects(&mut self, until: Until) {
        if !self.copies.iter().any(|x| x.until == until) {
            return;
        }
        let (ended, keep): (Vec<CopyFx>, Vec<CopyFx>) = self.copies.iter().copied().partition(|x| x.until == until);
        self.copies = keep;
        for x in ended {
            let c = x.obj.idx;
            if !self.live(x.obj) {
                continue;
            }
            let d = self.copy_fx(c).map_or(x.own, |y| y.vals.def);
            if self.objs[c.i()].def != d {
                self.set_face(c, d);
                self.note(|g| format!("{} is no longer a copy of {}", g.db.def(x.own).name, g.db.def(x.vals.def).name));
            }
        }
    }

    /// `c` (its object `old`) changed zones: its copy effects end (CR
    /// 400.7) and it has its own definition again (`move_card`).
    pub(crate) fn copies_on_move(&mut self, c: CardIdx, old: ObjRef) {
        let mut own = None;
        self.copies.retain(|x| {
            if x.obj == old {
                own = Some(x.own);
                false
            } else {
                true
            }
        });
        if let Some(d) = own {
            self.objs[c.i()].def = d;
        }
    }

    /// `controller` creates n tokens that are copies with these copiable
    /// values (CR 707.2, 111.2), multiplied by the replacement effects that
    /// multiply tokens; the first is first. No token copy of an instant or
    /// sorcery card is created (111.5).
    pub(crate) fn create_token_copies(&mut self, vals: CopyVals, controller: PlayerId, n: u32) -> Vec<CardIdx> {
        if self.db.def(vals.def).is(types::INSTANT | types::SORCERY) {
            return Vec::new();
        }
        let n = self.token_count(controller, n);
        let mut made = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let idx = self.add_card(vals.def, controller, Zone::Nowhere);
            self.copy_objs.push(idx);
            self.token_slot_made(idx);
            self.note(|g| format!("P{controller} creates a token copy of {}", g.db.def(vals.def).name));
            self.entering_copy = Some(vals);
            self.put_onto_battlefield(idx, controller);
            made.push(idx);
        }
        made
    }

    /// Put a copy of spell `item` (whose card has definition `def` on the
    /// stack: the face cast) onto the stack under `controller` (CR 707.10):
    /// not cast; its modes, X, targets, division and costs are the
    /// original's; it is owned by `controller`. A "whenever you cast or
    /// copy" event follows (`COPIED`). Returns its stack id.
    pub(crate) fn put_spell_copy(&mut self, def: DefId, item: &StackItem, controller: PlayerId) -> u32 {
        let idx = self.add_card(def, controller, Zone::Nowhere);
        self.copy_objs.push(idx);
        self.token_slot_made(idx);
        let r = self.move_card_to_stack(idx, controller);
        self.objs[idx.i()].def = def;
        let id = self.new_stack_id();
        self.stack.push(StackItem {
            id,
            kind: StackKind::Spell { card: idx },
            controller,
            targets: item.targets.clone(),
            cost_option: item.cost_option,
            modes: item.modes.clone(),
            ev: item.ev,
            alt: item.alt,
            from: item.from,
            delved: 0,
            delve_floor: 0,
            discarded_land: item.discarded_land,
            x: item.x,
            division: item.division.clone(),
            attack_as: None,
            noted: Vec::new(),
            flips: 0,
            origin: Zone::Nowhere,
            times: item.times,
            spliced: item.spliced.clone(),
        });
        self.note(|g| format!("P{controller} copies {}", g.db.def(def).name));
        self.event(Event { kind: crate::db::EventKind::IsCast, a: Subj::Obj(r), b: Subj::Player(controller), amount: COPIED });
        id
    }

    /// A spell leaving the stack whose last known information a waiting or
    /// stacked triggered ability may need ("copy it", CR 608.2h): remember
    /// its item (`Game::spell_lki`).
    pub(crate) fn note_spell_lki(&mut self, card: CardIdx, item: &StackItem) {
        let r = self.obj_ref(card);
        let wanted = self.waiting.iter().any(|w| w.ev.obj == Some(r))
            || self.stack.iter().any(|s| matches!(s.kind, StackKind::Ability { .. }) && s.ev.obj == Some(r));
        if wanted {
            self.spell_lki.push((r, self.objs[card.i()].def, item.clone()));
        }
    }

    /// A spell (its definition and item): on the stack now, or as it last
    /// existed there.
    fn spell_now_or_lki(&self, r: ObjRef) -> Option<(DefId, StackItem)> {
        if self.live(r) && self.objs[r.idx.i()].zone == Zone::Stack {
            let item = self.stack.iter().find(|s| s.kind == StackKind::Spell { card: r.idx })?;
            return Some((self.objs[r.idx.i()].def, item.clone()));
        }
        self.spell_lki.iter().rev().find(|x| x.0 == r).map(|x| (x.1, x.2.clone()))
    }

    /// The spell a `CopySpell` copies.
    fn spell_source(&self, f: &Frame, of: SpellOf) -> Option<(DefId, StackItem)> {
        match of {
            SpellOf::This => self.spell_now_or_lki(f.source),
            SpellOf::EventSpell => self.spell_now_or_lki(f.ev.obj?),
            SpellOf::Target(i) => self.targets_of(f, i).into_iter().find_map(|t| match t {
                Target::Obj(r) if self.objs[r.idx.i()].zone == Zone::Stack => self.spell_now_or_lki(r),
                _ => None,
            }),
        }
    }

    /// The times the additional cost of the event's spell was paid.
    pub(crate) fn event_spell_times(&self, f: &Frame) -> u8 {
        f.ev.obj.and_then(|r| self.spell_now_or_lki(r)).map_or(0, |(_, item)| item.times)
    }

    /// The copiable values a token copy or a "becomes a copy" effect takes
    /// (`None`: no such object, CR 111.12).
    pub(crate) fn copy_source(&self, f: &Frame, of: CopyOf) -> Option<CopyVals> {
        match of {
            CopyOf::Target { i, lki } => {
                let r = if lki {
                    // (Still the object chosen, legal as resolution began.)
                    f.targets.iter().enumerate().find_map(|(j, &(req, t))| match t {
                        Target::Obj(r) if req == i && f.legal & (1 << j) != 0 => Some(r),
                        _ => None,
                    })?
                } else {
                    self.targets_of(f, i).into_iter().find_map(|t| match t {
                        Target::Obj(r) => Some(r),
                        Target::Player(_) | Target::Stack(_) => None,
                    })?
                };
                if self.live(r) && self.objs[r.idx.i()].zone == Zone::Graveyard {
                    // A card: its printed values (its front face, 712.8a).
                    return Some(CopyVals { def: self.objs[r.idx.i()].def, exc: Except::default() });
                }
                self.copiable_of(r)
            }
            CopyOf::Source => self.copiable_of(f.source),
            CopyOf::SourceHost => {
                if !self.live(f.source) || self.objs[f.source.idx.i()].zone != Zone::Battlefield {
                    return None;
                }
                let h = self.objs[f.source.idx.i()].attached?;
                (self.live(h) && self.objs[h.idx.i()].zone == Zone::Battlefield).then(|| self.copiable(h.idx))
            }
            // The card as it last existed in its graveyard: its printed
            // values (its front face, CR 712.8a).
            CopyOf::SourceCard => {
                let d = self.db.def(f.source_def).card_def(f.source_def);
                Some(CopyVals { def: d, exc: Except::default() })
            }
        }
    }

    /// The copy instructions (`program.rs`, M7b part 3).
    pub(crate) fn exec_copies(&mut self, f: &mut Frame, op: &crate::program::Ext) -> crate::resolve::Flow {
        use crate::program::Ext;
        use crate::resolve::Flow;
        match op {
            Ext::CreateTokenCopy { of, who, n, exc } => {
                f.made = None;
                f.made_all.clear();
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                let Some(base) = self.copy_source(f, *of) else { return Flow::Next };
                let vals = CopyVals { def: base.def, exc: exc.over(base.exc) };
                let n = self.amount(f, *n).max(0) as u32;
                let made = self.create_token_copies(vals, p, n);
                f.made = made.first().copied();
                f.made_all = made;
                Flow::Next
            }
            Ext::BecomeCopy { on, of, exc, until } => {
                let Some(base) = self.copy_source(f, *of) else { return Flow::Next };
                let vals = CopyVals { def: base.def, exc: exc.over(base.exc) };
                for c in self.objs_on(f, on) {
                    self.add_copy_effect(c, vals, *until);
                }
                Flow::Next
            }
            Ext::CopySpell { of, who, n, may, new_targets } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                let Some((def, item)) = self.spell_source(f, *of) else { return Flow::Next };
                // `f.i`: the copies made so far (the "may" is asked first).
                if *may && f.i == 0 {
                    match self.answer.take() {
                        Some(Choice::Yes) => self.may_copies = self.may_copies.saturating_add(1),
                        Some(_) => return Flow::Next,
                        None if self.may_copies >= MAY_COPY_CAP => return Flow::Next,
                        None => {
                            let opts = vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }];
                            self.ask(Task::Run(f.clone()), p, DecisionKind::YesNo, opts);
                            return Flow::Suspend;
                        }
                    }
                }
                let n = self.amount(f, *n).clamp(0, 255) as u8;
                if f.i >= n {
                    return Flow::Next;
                }
                let id = self.put_spell_copy(def, &item, p);
                let mut back = f.clone();
                back.i = f.i + 1;
                self.agenda.push(Task::Run(back));
                if *new_targets && !item.targets.is_empty() {
                    self.agenda.push(Task::CopyTargets { id, k: 0, copy: true, page: crate::decision::Page::FIRST });
                } else {
                    // The copy's targets become its targets (CR 707.10c).
                    self.all_became_targets(id);
                }
                Flow::Suspend
            }
            Ext::BecomePrepared { on } => {
                for c in self.objs_on(f, on) {
                    self.become_prepared(c);
                }
                Flow::Next
            }
            Ext::ExileEventCard => {
                if let Some(r) = f.ev.obj {
                    if self.live(r) && self.objs[r.idx.i()].zone == Zone::Graveyard {
                        self.move_card(r.idx, Zone::Exile, crate::game::Pos::Top);
                    }
                }
                Flow::Next
            }
            other => unreachable!("not a copy instruction: {other:?}"),
        }
    }

    /// Permanent `c` becomes prepared (CR 722.3a, c): if it has a prepare
    /// spell and isn't prepared, its controller creates a copy of it in
    /// exile with only its prepare spell's characteristics, which they may
    /// cast while `c` stays prepared (`PermKind::Prepared`).
    pub(crate) fn become_prepared(&mut self, c: CardIdx) {
        let r = self.obj_ref(c);
        let Some(face) = self.def_of(c).prepare else { return };
        if self.objs[c.i()].zone != Zone::Battlefield || self.prepared.iter().any(|x| x.0 == r) {
            return;
        }
        let p = self.objs[c.i()].controller;
        let copy = self.add_card(face, p, Zone::Nowhere);
        self.copy_objs.push(copy);
        self.token_slot_made(copy);
        let card = self.move_card(copy, Zone::Exile, crate::game::Pos::Top);
        // (Its characteristics are the prepare spell's in exile too.)
        self.objs[copy.i()].def = face;
        self.prepared.push((r, copy));
        self.permissions.push(crate::game::Permission {
            card,
            player: p,
            turn: None,
            kind: crate::game::PermKind::Prepared { source: r },
        });
        self.note(|g| format!("{} becomes prepared", g.name(c)));
    }

    /// Is `c` a copy of a prepare spell in exile whose permanent is still
    /// prepared on the battlefield (the exception to 704.5e, CR 722.3c)?
    pub(crate) fn prepare_copy_live(&self, c: CardIdx) -> bool {
        self.prepared.iter().any(|&(s, x)| x == c && self.live(s) && self.objs[s.idx.i()].zone == Zone::Battlefield)
    }

    /// Is permanent `c` prepared (CR 722.3a)?
    pub fn is_prepared(&self, c: CardIdx) -> bool {
        let r = self.obj_ref(c);
        !self.prepared.is_empty() && self.prepared.iter().any(|x| x.0 == r)
    }

    /// Containment Priest's replacement for `card`, entering "as a copy"
    /// and not cast, after its copy choice (CR 614.12, 616.1; ENGINE.md 38):
    /// still a nontoken creature, it is exiled instead (it never entered:
    /// no leave or enter events). True if it was.
    fn priest_exiles(&mut self, card: CardIdx) -> bool {
        let pending = |&(o, m): &(ObjRef, crate::game::Mark)| o.idx == card && m == crate::game::Mark::PriestPending;
        let Some(i) = self.marks.iter().position(pending) else { return false };
        self.marks.swap_remove(i);
        if !self.is_creature(card) || self.is_token(card) {
            return false;
        }
        self.note(|g| format!("{} is exiled instead (not cast)", g.name(card)));
        self.quiet_leave = true;
        self.move_card(card, Zone::Exile, crate::game::Pos::Top);
        self.quiet_leave = false;
        true
    }

    /// CR 707.10c: the controller of copy `id` may choose new targets: for
    /// its target `k` (in order), keep it (`PASS`, even if illegal) or
    /// choose another legal target for that requirement (`NewTarget`). The
    /// copy is put onto the stack with the targets so decided: each becomes
    /// its target then. For a spell that is not a copy (`copy` false: "you
    /// may choose new targets for it", 115.7), each new target becomes its
    /// target as it is chosen.
    /// (`page`: the decision's page, `Game::ask_objects`, ENGINE.md 100.2.)
    pub(crate) fn copy_targets(&mut self, id: u32, k: u8, copy: bool, page: crate::decision::Page) {
        let Some(pos) = self.item_index(id) else { return };
        if k as usize >= self.stack[pos].targets.len() {
            if copy {
                self.all_became_targets(id);
            }
            return;
        }
        match self.answer.take() {
            Some(Choice::Target(t)) => {
                self.note(|g| format!("new target {}", g.describe_target(t)));
                self.stack[pos].targets[k as usize].1 = t;
                if !copy && !self.stack[pos].targets.iter().enumerate().any(|(j, &(_, x))| j != k as usize && x == t) {
                    self.became_target(id, t);
                }
                self.agenda.push(Task::CopyTargets { id, k: k + 1, copy, page: crate::decision::Page::FIRST });
                return;
            }
            Some(Choice::Pass) => {
                self.agenda.push(Task::CopyTargets { id, k: k + 1, copy, page: crate::decision::Page::FIRST });
                return;
            }
            Some(Choice::Page(next)) => {
                self.agenda.push(Task::CopyTargets { id, k, copy, page: next });
                return;
            }
            Some(other) => unreachable!("new target answer {other:?}"),
            None => {}
        }
        let item = &self.stack[pos];
        let Some(ar) = self.item_ability(item) else { return };
        let reqs = crate::resolve_tail3::item_reqs(self.db, item, ar);
        let (req, cur) = item.targets[k as usize];
        let tr = reqs[req as usize];
        let p = item.controller;
        let ctx = crate::cast::Ctx { controller: p, source: Some(self.item_source(item)) };
        // The other targets of the same "target" word (CR 115.3), and those
        // it must differ from ("another target").
        let exclude: Vec<Target> = item
            .targets
            .iter()
            .enumerate()
            .filter(|&(j, &(r, _))| {
                j != k as usize && (r == req || tr.distinct & (1 << r) != 0 || reqs[r as usize].distinct & (1 << req) != 0)
            })
            .map(|(_, &(_, t))| t)
            .collect();
        // (`PASS` keeps the current target.)
        let mut cands = self.target_candidates(tr, &ctx, &exclude);
        if let Some(n) = tr.total_mv {
            // The new set within the total too (CR 601.2c, 707.10c).
            let others: Vec<Target> =
                item.targets.iter().enumerate().filter(|&(j, &(r, _))| j != k as usize && r == req).map(|(_, &(_, t))| t).collect();
            self.retain_within_total_mv(n, &others, &mut cands);
        }
        let items: Vec<Choice> = cands.into_iter().filter(|&t| t != cur).map(Choice::Target).collect();
        if items.is_empty() {
            // Nothing else to choose: the copy keeps it.
            self.agenda.push(Task::CopyTargets { id, k: k + 1, copy, page: crate::decision::Page::FIRST });
            return;
        }
        let pass = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        self.ask_objects(Task::CopyTargets { id, k, copy, page }, page, p, DecisionKind::NewTarget, &items, pass);
    }

    /// "You may have this enter as a copy of ..." (CR 707.5, 614.1c) for
    /// `card`, just put onto the battlefield with its `Enters` event held
    /// back (`put_onto_battlefield`): its controller chooses what it copies
    /// (`ChoosePermanent` or `ChooseGraveyardCard`, `PASS` = nothing); then
    /// what it entered as enters: its replacement effects apply ("enters
    /// tapped", "enters with counters", "as this enters" choices: 707.5,
    /// 707.6) and its `Enters` event (with `note`) happens.
    pub(crate) fn enter_as_copy(&mut self, card: CardIdx, note: u16, since: u32) {
        if self.objs[card.i()].zone != Zone::Battlefield {
            self.marks.retain(|&(o, m)| !(o.idx == card && m == crate::game::Mark::PriestPending));
            return;
        }
        let own = self.own_def(card);
        let Some(eac) = self.db.def(own).enters_as_copy.as_deref() else { return };
        let p = self.objs[card.i()].controller;
        match self.answer.take() {
            Some(Choice::Card(x)) => {
                let base = if eac.from_graveyard {
                    CopyVals { def: self.objs[x.i()].def, exc: Except::default() }
                } else {
                    self.copiable(x)
                };
                let mut exc = eac.exc;
                if eac.keep_name {
                    exc.name = Some(own);
                }
                let vals = CopyVals { def: base.def, exc: exc.over(base.exc) };
                self.add_copy_effect(card, vals, Until::Forever);
                if self.priest_exiles(card) {
                    return;
                }
                if let Some(k) = eac.exile_k {
                    // "When you do, exile that card" (603.11, 707.9g).
                    let slot = self.db.def(own).delayed_base + k;
                    let ev = crate::triggers::EvCtx { obj: Some(self.obj_ref(x)), ..Default::default() };
                    let source = self.obj_ref(card);
                    self.waiting.push(crate::triggers::PendingTrigger { controller: p, source, source_def: own, slot, ev });
                }
            }
            Some(Choice::Pass) => {
                if self.priest_exiles(card) {
                    return;
                }
            }
            Some(other) => unreachable!("enter-as-copy answer {other:?}"),
            None => {
                let ctx = crate::cast::Ctx { controller: p, source: Some(self.obj_ref(card)) };
                let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                if eac.from_graveyard {
                    for q in [p, opp(p)] {
                        for x in self.graveyard_cards(q) {
                            if self.card_matches(&eac.filter, x, &ctx) {
                                opts.push(Opt { action: self.graveyard_action(p, x), choice: Choice::Card(x) });
                            }
                        }
                    }
                } else {
                    // Not a permanent entering at the same time or since (it
                    // isn't on the battlefield as this one enters; CR
                    // 614.12a, 707.5 ruling: Exhume): timestamps from
                    // `since`, the first of its event's entries.
                    for &x in &self.battlefield {
                        if x != card && self.objs[x.i()].timestamp < since && self.card_matches(&eac.filter, x, &ctx) {
                            opts.push(Opt { action: self.perm_action(p, x), choice: Choice::Card(x) });
                        }
                    }
                }
                let kind = if eac.from_graveyard { DecisionKind::ChooseGraveyardCard } else { DecisionKind::ChoosePermanent };
                self.ask(Task::EnterAsCopy { card, note, since }, p, kind, opts);
                return;
            }
        }
        let r = self.obj_ref(card);
        let tapped = self.enters_tapped(card, p);
        self.entering(card, tapped, 0);
        self.note_entered(card);
        self.event(Event { kind: crate::db::EventKind::Enters, a: Subj::Obj(r), b: Subj::None, amount: note as u32 });
    }
}
