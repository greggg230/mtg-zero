//! Events and triggered abilities (CR 603; ENGINE.md 8).

use crate::db::{AbilityRef, AbilitySlot, EventKind, Filter, Slot, TrigPred};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, StackItem, StackKind};
use crate::ids::{opp, CardIdx, DefId, ObjRef, PlayerId, Target, Zone};

/// A subject of an event: an object (as it was when the event happened)
/// or a player.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Subj {
    None,
    Obj(ObjRef),
    Player(PlayerId),
    /// A spell or ability on the stack, by its item id (the one that
    /// targets, `BecomesTarget`); its controller is its player.
    Item(u32),
}

/// Something that happened (603.2), with its subjects (`EventKind`
/// documents them) and an amount (damage dealt: any amount, ENGINE.md 46;
/// or a note's bits, `Game::put_onto_battlefield`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Event {
    pub kind: EventKind,
    pub a: Subj,
    pub b: Subj,
    pub amount: u32,
}

impl Event {
    pub(crate) fn obj(kind: EventKind, r: ObjRef) -> Event {
        Event { kind, a: Subj::Obj(r), b: Subj::None, amount: 0 }
    }
    pub(crate) fn player(kind: EventKind, p: PlayerId) -> Event {
        Event { kind, a: Subj::Player(p), b: Subj::None, amount: 0 }
    }
    fn subject(&self, s: Slot) -> Subj {
        // "Becomes the target": the target is a player or a permanent
        // (`Target`), or a spell (`TargetSpell`, amount 1).
        if self.kind == EventKind::BecomesTarget && (s == Slot::Target) != (self.amount == 0) && s != Slot::Controller {
            return Subj::None;
        }
        match self.kind.slot_index(s) {
            Some(0) => self.a,
            Some(_) => self.b,
            None => Subj::None,
        }
    }
}

/// Last known information of an object that left the battlefield earlier
/// in the current simultaneous move: its leave-the-battlefield abilities
/// still see the others leave (CR 603.10a).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Lki {
    pub old: ObjRef,
    pub def: DefId,
    pub controller: PlayerId,
}

/// What a triggered ability remembers of its event for resolution: "that
/// player", "that creature", "that much" (CR 603.7c-like references).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct EvCtx {
    pub player: Option<PlayerId>,
    pub obj: Option<ObjRef>,
    /// The second object subject ("that creature" dealt damage).
    pub obj2: Option<ObjRef>,
    pub amount: u32,
    /// The spell or ability of the event, by stack item id (0 = none):
    /// the one that targets ("counter that spell or ability", ward).
    pub item: u32,
}

/// A triggered ability waiting to be put on the stack (603.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PendingTrigger {
    pub controller: PlayerId,
    /// The source object (possibly gone: last known information).
    pub source: ObjRef,
    pub source_def: DefId,
    pub slot: u8,
    /// The event's subjects and amount.
    pub ev: EvCtx,
}

/// An object whose abilities may trigger on an event.
#[derive(Clone, Copy)]
struct Observer {
    r: ObjRef,
    def: DefId,
    controller: PlayerId,
    /// A card in a graveyard: only its abilities that function there.
    gy: bool,
    /// A spell being cast: only its "when you cast this spell" abilities
    /// function (CR 113.6: a permanent card's other abilities work on the
    /// battlefield).
    spell: bool,
}

/// Does the predicate say "one or more" (`Batch`)?
/// The ability functions in the graveyard (`TrigPred::InGraveyard` at the
/// top of the predicate).
pub(crate) fn in_graveyard(p: &TrigPred) -> bool {
    match p {
        TrigPred::InGraveyard => true,
        TrigPred::And(a, b) => in_graveyard(a) || in_graveyard(b),
        _ => false,
    }
}

/// The subject slot of an ability that permanents have because an effect
/// grants it ("other creatures you control have ward", `Granted`): that
/// permanent is the ability's source (CR 113.7: the object that has it).
fn granted_slot(p: &TrigPred) -> Option<Slot> {
    match p {
        TrigPred::Granted(s) => Some(*s),
        TrigPred::And(a, b) => granted_slot(a).or_else(|| granted_slot(b)),
        _ => None,
    }
}

fn has_batch(p: &TrigPred) -> bool {
    match p {
        TrigPred::Batch => true,
        TrigPred::And(a, b) | TrigPred::Or(a, b) => has_batch(a) || has_batch(b),
        _ => false,
    }
}

/// Does the predicate say "this ability triggers only once each turn"
/// (`Once`, at the top level)?
fn has_once(p: &TrigPred) -> bool {
    match p {
        TrigPred::Once => true,
        TrigPred::And(a, b) => has_once(a) || has_once(b),
        _ => false,
    }
}

/// The subject slot of a "triggers only n times each turn" limit, if the
/// predicate has one (at the top level).
fn times_slot(p: &TrigPred) -> Option<crate::db::Slot> {
    match p {
        TrigPred::TimesPerSubject(s, _) => Some(*s),
        TrigPred::And(a, b) => times_slot(a).or_else(|| times_slot(b)),
        _ => None,
    }
}

/// Does the predicate say "cast or copy" (`OrCopy`)?
fn has_or_copy(p: &TrigPred) -> bool {
    match p {
        TrigPred::OrCopy => true,
        TrigPred::And(a, b) | TrigPred::Or(a, b) => has_or_copy(a) || has_or_copy(b),
        _ => false,
    }
}

/// Does the predicate name "this spell" (`IsSelf(Spell)`)?
fn names_this_spell(p: &TrigPred) -> bool {
    match p {
        TrigPred::IsSelf(Slot::Spell) => true,
        TrigPred::And(a, b) | TrigPred::Or(a, b) => names_this_spell(a) || names_this_spell(b),
        TrigPred::Not(a) => names_this_spell(a),
        _ => false,
    }
}

impl Game {
    /// Record the triggers an event sets off (603.2): abilities of
    /// permanents on the battlefield, of objects that left in the same
    /// simultaneous move (look back, 603.10a), and of a spell being cast
    /// ("when you cast this spell"). Leave-the-battlefield events are
    /// recorded before the object moves, so they see the game as it was.
    pub(crate) fn event(&mut self, ev: Event) {
        let bit = 1u64 << ev.kind as u32;
        let mut obs: Vec<Observer> = Vec::new();
        // Permanents (not one that lost all its abilities, CR 613.1f), then
        // emblems (their abilities function in the command zone, 114.4).
        for &c in &self.battlefield {
            let o = &self.objs[c.i()];
            if self.db.def(o.def).trig_mask & bit != 0 && !self.lost_abilities(c) {
                obs.push(Observer { r: ObjRef { idx: c, inc: o.inc }, def: o.def, controller: o.controller, spell: false, gy: false });
            }
        }
        for &c in &self.command {
            let o = &self.objs[c.i()];
            if self.db.def(o.def).trig_mask & bit != 0 {
                obs.push(Observer { r: ObjRef { idx: c, inc: o.inc }, def: o.def, controller: o.controller, spell: false, gy: false });
            }
        }
        // Cards in graveyards with abilities that function there (Sword of
        // the Meek; their owner controls them).
        if self.seen & crate::game::SEEN_GY_TRIGGER != 0 {
            for p in 0..2 {
                for &c in &self.players[p].graveyard {
                    let o = &self.objs[c.i()];
                    let d = self.db.def(o.def);
                    if d.gy_triggers && d.trig_mask & bit != 0 {
                        obs.push(Observer { r: ObjRef { idx: c, inc: o.inc }, def: o.def, controller: o.owner, spell: false, gy: true });
                    }
                }
            }
        }
        if matches!(ev.kind, EventKind::Dies | EventKind::LeavesBattlefield | EventKind::IsSacrificed) {
            for l in &self.leaving {
                if self.db.def(l.def).trig_mask & bit != 0 {
                    obs.push(Observer { r: l.old, def: l.def, controller: l.controller, spell: false, gy: false });
                }
            }
        }
        // "When this is put into a graveyard from anywhere": the card itself,
        // its owner's ability.
        if ev.kind == EventKind::ToGraveyard {
            if let Subj::Obj(r) = ev.a {
                let o = &self.objs[r.idx.i()];
                obs.push(Observer { r, def: o.def, controller: o.owner, spell: false, gy: false });
            }
        }
        if ev.kind == EventKind::IsCast {
            if let Subj::Obj(r) = ev.a {
                let o = &self.objs[r.idx.i()];
                if self.db.def(o.def).trig_mask & bit != 0 {
                    obs.push(Observer { r, def: o.def, controller: o.controller, spell: true, gy: false });
                }
            }
        }
        if obs.is_empty() && self.temp_triggers.is_empty() {
            return;
        }
        // ("That player" of an event with a spell or ability is its
        // controller: "unless that player pays", ward.)
        let item = [ev.a, ev.b].iter().find_map(|s| match s {
            Subj::Item(id) => Some(*id),
            _ => None,
        });
        let ctx = EvCtx {
            player: match item.and_then(|id| self.stack_item(id)) {
                Some(s) => Some(s.controller),
                None => [ev.a, ev.b].iter().find_map(|s| match s {
                    Subj::Player(p) => Some(*p),
                    _ => None,
                }),
            },
            obj: [ev.a, ev.b].iter().find_map(|s| match s {
                Subj::Obj(r) => Some(*r),
                _ => None,
            }),
            obj2: match (ev.a, ev.b) {
                (Subj::Obj(_), Subj::Obj(r)) => Some(r),
                _ => None,
            },
            amount: ev.amount,
            item: item.unwrap_or(0),
        };
        // A copy of a spell isn't cast (CR 707.10): only "whenever you cast
        // or copy" abilities see its event.
        let copied = ev.kind == EventKind::IsCast && ev.amount == crate::copies::COPIED;
        for o in obs {
            let def = self.db.def(o.def);
            // (Delayed and temporary abilities, from `delayed_base` on,
            // trigger only as such.)
            for (i, t) in def.triggers.iter().enumerate().take(def.delayed_base as usize) {
                if t.event == ev.kind
                    && (!def.gy_triggers || o.gy == in_graveyard(&t.pred))
                    && (!o.spell || names_this_spell(&t.pred))
                    && (!copied || has_or_copy(&t.pred))
                    && self.pred_matches(&t.pred, &ev, o)
                {
                    // "Whenever one or more ...": once per event (CR 603.2c:
                    // one instruction, one cost, one simultaneous move is one
                    // event; separate events of one resolution trigger it
                    // separately). "Whenever one or more cards are put into a
                    // library" says so too.
                    let batch = has_batch(&t.pred) || ev.kind == crate::db::EventKind::CardsPutIntoLibrary;
                    if batch && !self.batch_once(o.r, i as u8) {
                        continue;
                    }
                    // CR 603.2h: "Do this only once each turn": it triggers
                    // only if its controller hasn't done it this turn.
                    if !self.done_once.is_empty() && self.done_once.contains(&(o.r, i as u8)) {
                        continue;
                    }
                    // (A granted ability's source is the permanent that has
                    // it; the ability is the granting card's definition's.)
                    let source = match granted_slot(&t.pred).map(|s| ev.subject(s)) {
                        Some(Subj::Obj(r)) => r,
                        _ => o.r,
                    };
                    self.waiting.push(PendingTrigger {
                        controller: o.controller,
                        source,
                        source_def: o.def,
                        slot: i as u8,
                        ev: ctx,
                    });
                    // "This ability triggers only once each turn": not
                    // again this turn (the object's; a new object is new).
                    if has_once(&t.pred) {
                        self.done_once.push((o.r, i as u8));
                    }
                    // "... only twice each turn", per granted object (Nadu).
                    if let Some(s) = times_slot(&t.pred) {
                        if let Subj::Obj(r) = ev.subject(s) {
                            self.note_granted_trigger(r, o.def, i as u8, o.r.inc);
                        }
                    }
                }
            }
        }
        // Triggered abilities a player has for a while ("until your next
        // turn, whenever ...").
        for k in 0..self.temp_triggers.len() {
            let tt = self.temp_triggers[k];
            let t = &self.db.def(tt.source_def).triggers[tt.slot as usize];
            let o = Observer { r: tt.source, def: tt.source_def, controller: tt.controller, spell: false, gy: false };
            if t.event == ev.kind
                && tt.obj.is_none_or(|b| ev.a == Subj::Obj(b))
                && (!copied || has_or_copy(&t.pred))
                && self.pred_matches(&t.pred, &ev, o)
                && !self.done_once.contains(&(tt.source, tt.slot))
                // "Whenever one or more ...": once per event.
                && (!has_batch(&t.pred) || self.batch_once(tt.source, tt.slot))
            {
                self.waiting.push(PendingTrigger {
                    controller: tt.controller,
                    source: tt.source,
                    source_def: tt.source_def,
                    slot: tt.slot,
                    ev: ctx,
                });
            }
        }
    }

    fn pred_matches(&self, p: &TrigPred, ev: &Event, src: Observer) -> bool {
        match p {
            TrigPred::Always => true,
            TrigPred::And(a, b) => self.pred_matches(a, ev, src) && self.pred_matches(b, ev, src),
            TrigPred::Or(a, b) => self.pred_matches(a, ev, src) || self.pred_matches(b, ev, src),
            TrigPred::Not(a) => !self.pred_matches(a, ev, src),
            TrigPred::IsSelf(s) => match ev.subject(*s) {
                Subj::Obj(r) => r == src.r,
                // "At the beginning of your upkeep": the source's own
                // event (the IR's IsSelf on a player event).
                Subj::Player(_) => *s == Slot::Default,
                Subj::None | Subj::Item(_) => false,
            },
            TrigPred::Subject(s, f) => {
                let ctx = crate::cast::Ctx { controller: src.controller, source: Some(src.r) };
                match ev.subject(*s) {
                    // (A card leaving a graveyard is seen there, before it
                    // moves: CR 603.10a; ENGINE.md 53.)
                    Subj::Obj(r) if ev.kind == EventKind::LeavesGraveyard => {
                        self.live(r) && self.objs[r.idx.i()].zone == Zone::Graveyard && self.card_matches(f, r.idx, &ctx)
                    }
                    Subj::Obj(r) => self.live(r) && self.subject_matches(f, r.idx, &ctx),
                    Subj::Player(q) => self.filter_ok(f, Target::Player(q), &ctx),
                    Subj::None | Subj::Item(_) => false,
                }
            }
            TrigPred::Controlled(s, you) => {
                let who = match ev.subject(*s) {
                    Subj::Obj(r) => self.objs[r.idx.i()].controller,
                    Subj::Player(q) => q,
                    Subj::Item(id) => match self.stack_item(id) {
                        Some(item) => item.controller,
                        None => return false,
                    },
                    Subj::None => return false,
                };
                (who == src.controller) == *you
            }
            TrigPred::If(c) | TrigPred::State(c) => self.cond_holds_from(c, src.controller, Some(src.r)),
            TrigPred::Source(f) => {
                let ctx = crate::cast::Ctx { controller: src.controller, source: Some(src.r) };
                self.live(src.r) && self.subject_matches(f, src.r.idx, &ctx)
            }
            TrigPred::IsHost(s) => match ev.subject(*s) {
                Subj::Obj(r) => self.objs[src.r.idx.i()].attached == Some(r) && self.live(r),
                _ => false,
            },
            TrigPred::HostAttacksWithAnother => {
                let host = self.objs[src.r.idx.i()].attached;
                host.is_some_and(|h| self.live(h) && self.objs[h.idx.i()].attacking) && self.attackers().len() >= 2
            }
            TrigPred::Chapter(n) => ev.a == Subj::Obj(src.r) && ev.amount == *n as u32,
            TrigPred::Level(n) => ev.kind == EventKind::ClassLevel && ev.a == Subj::Obj(src.r) && ev.amount == *n as u32,
            TrigPred::TimesPerSubject(s, n) => match ev.subject(*s) {
                Subj::Obj(r) => self.granted_triggered(r, src.def, src.r.inc) < *n,
                _ => false,
            },
            TrigPred::Batch => true,
            // The `Enters` event's note (`Game::put_onto_battlefield`).
            TrigPred::EnteredCast => ev.kind == EventKind::Enters && ev.amount & 1 != 0,
            TrigPred::EnteredPaid => ev.kind == EventKind::Enters && (ev.amount >> 1) & 0xFF != 0,
            TrigPred::EnteredXAtLeast(n) => ev.kind == EventKind::Enters && (ev.amount >> 9) & 0x3F >= *n as u32,
            TrigPred::EnteredNotFromGyExile => ev.kind == EventKind::Enters && ev.amount >> 15 == 0,
            TrigPred::InGraveyard => true,
            TrigPred::SpellMvParity => match (ev.a, self.parity_of(src.r)) {
                (Subj::Obj(r), Some(k)) => self.mana_value_of(r.idx) % 2 == k,
                _ => false,
            },
            TrigPred::SpellPaid => match ev.a {
                Subj::Obj(r) => self.stack.iter().any(|s| s.kind == StackKind::Spell { card: r.idx } && s.times > 0),
                _ => false,
            },
            TrigPred::OrCopy => true,
            TrigPred::Once => true,
            // CR 724, 508.1b: it attacks the player who is the monarch.
            TrigPred::AttacksMonarch => match ev.a {
                Subj::Obj(r) => {
                    let o = &self.objs[r.idx.i()];
                    o.attacks.is_none() && self.monarch == Some(opp(o.controller))
                }
                _ => false,
            },
            TrigPred::FirstAttack => match ev.a {
                Subj::Obj(r) => self.players.iter().map(|pl| pl.attacked_with.iter().filter(|&&x| x == r).count()).sum::<usize>() == 1,
                _ => false,
            },
            TrigPred::Granted(s) => match ev.subject(*s) {
                Subj::Obj(r) => {
                    let ts = self.objs[src.r.idx.i()].timestamp;
                    self.live(r) && self.loss_ts(r.idx).is_none_or(|l| l < ts)
                }
                _ => false,
            },
            TrigPred::FormerHost(s) => match ev.subject(*s) {
                // Evaluated as the host leaves (look back): the Equipment
                // is still attached to it.
                Subj::Obj(r) => self.objs[src.r.idx.i()].attached == Some(r),
                _ => false,
            },
        }
    }

    /// A subject filter on an object in any public zone (a spell being
    /// cast, a permanent): its characteristics.
    fn subject_matches(&self, f: &Filter, c: CardIdx, ctx: &crate::cast::Ctx) -> bool {
        match self.objs[c.i()].zone {
            Zone::Battlefield | Zone::Stack => self.card_matches(f, c, ctx),
            _ => false,
        }
    }

    /// Put waiting triggers on the stack in APNAP order (603.3b): the active
    /// player's first, each player ordering their own; targets are chosen as
    /// each goes on the stack (603.3d).
    pub(crate) fn put_triggers(&mut self) {
        if self.waiting.is_empty() {
            return;
        }
        let ap = self.turn.active;
        let p = if self.waiting.iter().any(|t| t.controller == ap) { ap } else { opp(ap) };
        let mine: Vec<usize> = (0..self.waiting.len()).filter(|&i| self.waiting[i].controller == p).collect();
        // Triggers of the same ability whose effects ignore the event's
        // subjects are interchangeable.
        let db = self.db;
        let same = |a: &PendingTrigger, b: &PendingTrigger| {
            a.source_def == b.source_def
                && a.slot == b.slot
                && (a.ev == b.ev || !db.def(a.source_def).triggers[a.slot as usize].uses_event)
        };
        // More waiting triggers than `TRIGGER` slots (a board of tokens
        // dying): the first of each interchangeable group first, the copies
        // after (their order among themselves can't matter), so that the
        // groups' ids stay in range.
        let mine = if mine.len() > act::TRIGGER_N as usize && self.answer.is_none() {
            let (mut firsts, mut rest) = (Vec::new(), Vec::new());
            for (k, &i) in mine.iter().enumerate() {
                if mine[..k].iter().any(|&j| same(&self.waiting[j], &self.waiting[i])) {
                    rest.push(self.waiting[i]);
                } else {
                    firsts.push(self.waiting[i]);
                }
            }
            firsts.extend(rest);
            for (&i, t) in mine.iter().zip(firsts) {
                self.waiting[i] = t;
            }
            mine
        } else {
            mine
        };
        let pick = match self.answer.take() {
            Some(Choice::Trigger(k)) => mine[k as usize],
            Some(other) => unreachable!("trigger answer {other:?}"),
            None => {
                let first = self.waiting[mine[0]];
                if mine.iter().all(|&i| same(&self.waiting[i], &first)) {
                    mine[0]
                } else {
                    // Identical triggers are offered once (their order cannot
                    // matter while their effects ignore the source's state).
                    let mut opts = Vec::new();
                    for (k, &i) in mine.iter().enumerate() {
                        if mine[..k].iter().any(|&j| same(&self.waiting[j], &self.waiting[i])) {
                            continue;
                        }
                        // (More distinct triggers than `TRIGGER` slots, each
                        // card a discard made: the first ones are offered; the
                        // others come next. A limit of the action layout.)
                        if k as u16 >= act::TRIGGER_N {
                            break;
                        }
                        opts.push(Opt { action: act::TRIGGER + k as u16, choice: Choice::Trigger(k as u8) });
                    }
                    self.ask(Task::PutTriggers, p, DecisionKind::OrderTriggers, opts);
                    return;
                }
            }
        };
        let t = self.waiting[pick];
        self.agenda.push(Task::PutTriggers);
        let ability = AbilityRef { def: t.source_def, slot: AbilitySlot::Trigger(t.slot), mode: None };
        let ab = self.db.ability(ability);
        // (Checked while the trigger still waits: its source's last known
        // information is found through it.)
        let ok = self.targets_available(ab, t.controller, Some(t.source));
        self.waiting.remove(pick);
        if !ok {
            self.note(|_| "trigger removed: no legal target".to_string());
            return; // 603.3d
        }
        // "Gain control of those permanents" (Dack Fayden's emblem): the
        // permanents the triggering spell targets now.
        let mut noted = Vec::new();
        if ab.program.iter().any(|op| matches!(op, crate::program::Op::Ext(crate::program::Ext::GainControlOfEventSpellTargets { .. }))) {
            if let Some(spell) = t.ev.obj.filter(|&r| self.live(r)) {
                if let Some(s) = self.stack.iter().find(|s| s.kind == StackKind::Spell { card: spell.idx }) {
                    for &(_, x) in &s.targets {
                        if let Target::Obj(r) = x {
                            if self.live(r) && self.objs[r.idx.i()].zone == crate::ids::Zone::Battlefield && !noted.contains(&r) {
                                noted.push(r);
                            }
                        }
                    }
                }
            }
        }
        let id = self.new_stack_id();
        self.stack.push(StackItem {
            id,
            kind: StackKind::Ability { ability, source: t.source },
            controller: t.controller,
            targets: Vec::new(),
            cost_option: 0,
            modes: Vec::new(),
            ev: t.ev,
            alt: 0,
            from: Default::default(),
            delved: 0,
            delve_floor: 0,
            discarded_land: false,
            x: 0,
            division: Vec::new(),
            attack_as: None,
            noted,
            flips: if self.live(t.source) { self.objs[t.source.idx.i()].flips } else { 0 },
            origin: Zone::Nowhere,
            times: 0,
            spliced: Vec::new(),
        });
        self.note(|g| format!("trigger of {} goes on the stack", g.db.def(t.source_def).name));
        if ab.divide.is_some() {
            self.agenda.push(Task::Divide { id }); // CR 603.3d, 601.2d
        }
        if ab.has_targets() {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
        }
        if ab.modes.is_some() {
            self.agenda.push(Task::ChooseModes { id }); // CR 700.2b, 603.3c
        }
    }

    /// Delayed triggered abilities that trigger at the beginning of the
    /// current step (CR 603.7): each triggers once and is gone.
    pub(crate) fn fire_delayed(&mut self) {
        use crate::db::DelayedWhen as W;
        let (step, ap) = (self.turn.step, self.turn.active);
        let fires = |d: &crate::game::Delayed| match d.when {
            W::NextEndStep => step == crate::game::Step::End,
            W::NextCleanup => step == crate::game::Step::Cleanup,
            W::NextUpkeep => step == crate::game::Step::Upkeep,
            W::YourNextMainPhase => step.is_main() && ap == d.controller,
        };
        let (now, later): (Vec<_>, Vec<_>) = self.delayed.iter().copied().partition(|d| fires(d));
        self.delayed = later;
        for d in now {
            self.waiting.push(PendingTrigger {
                controller: d.controller,
                source: d.source,
                source_def: d.source_def,
                slot: d.slot,
                ev: d.ev,
            });
        }
    }

    /// CR 603.4: a triggered ability with an intervening "if" does nothing
    /// on resolution unless the condition still holds (for its source: "a
    /// red permanent other than Ajani").
    pub(crate) fn intervening_holds(&self, ability: AbilityRef, controller: PlayerId, source: crate::ids::ObjRef) -> bool {
        let AbilitySlot::Trigger(i) = ability.slot else { return true };
        let t = &self.db.def(ability.def).triggers[i as usize];
        t.pred.intervening().into_iter().all(|c| self.cond_holds_from(c, controller, Some(source)))
    }

    /// `t` became the target of the spell or ability of stack item `id`
    /// (its targets were chosen or changed): a "becomes the target" event
    /// for a player, a permanent or a spell (ward, CR 702.21a; Leovold).
    /// Graveyard cards and abilities have no such trigger in the cube.
    pub(crate) fn became_target(&mut self, id: u32, t: Target) {
        let (a, amount) = match t {
            Target::Player(q) => (Subj::Player(q), 0),
            Target::Obj(r) if self.live(r) => match self.objs[r.idx.i()].zone {
                Zone::Battlefield => (Subj::Obj(r), 0),
                Zone::Stack => (Subj::Obj(r), 1),
                _ => return,
            },
            _ => return,
        };
        self.event(Event { kind: EventKind::BecomesTarget, a, b: Subj::Item(id), amount });
    }

    /// Every distinct target of stack item `id` becomes its target (a copy
    /// of a spell put onto the stack with its targets, CR 707.10c).
    pub(crate) fn all_became_targets(&mut self, id: u32) {
        let Some(item) = self.stack_item(id) else { return };
        let mut ts: Vec<Target> = Vec::new();
        for &(_, t) in &item.targets {
            if !ts.contains(&t) {
                ts.push(t);
            }
        }
        for t in ts {
            self.became_target(id, t);
        }
    }

    /// Start a simultaneous move of several objects off the battlefield
    /// (SBAs, "destroy all", a cost that sacrifices several): each one's
    /// leave abilities also see the others leave (603.10a).
    pub(crate) fn begin_batch(&mut self) {
        self.leaving.clear();
        self.batching = true;
        self.new_event();
    }

    pub(crate) fn end_batch(&mut self) {
        self.leaving.clear();
        self.batching = false;
        self.new_event();
    }

    /// A new event group begins (CR 603.2c: an instruction ended, a
    /// resolution or a simultaneous move began).
    #[inline]
    pub(crate) fn new_event(&mut self) {
        self.ev_seq = self.ev_seq.wrapping_add(1);
    }

    /// A "whenever one or more ..." ability of `source` (slot `slot`)
    /// triggers once per event group: true the first time in the current
    /// group (recorded), false after.
    fn batch_once(&mut self, source: ObjRef, slot: u8) -> bool {
        let q = self.ev_seq;
        if self.batch_fired.iter().any(|&(s, k, g)| s == source && k == slot && g == q) {
            return false;
        }
        self.batch_fired.retain(|&(_, _, g)| g == q);
        self.batch_fired.push((source, slot, q));
        true
    }
}
