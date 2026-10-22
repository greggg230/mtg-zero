//! Casting spells, choosing targets, filters (CR 601, 115; ENGINE.md 6).

use crate::db::{types, AbilityDef, AbilityRef, AbilitySlot, Filter, Kw, TargetReq};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, StackItem, StackKind};
use crate::ids::{CardIdx, ObjRef, PlayerId, Target, Zone};
use crate::pay::PayMana;

/// Who is asking whether a filter matches: the spell or ability's
/// controller ("you") and source ("this", "another").
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ctx {
    pub controller: PlayerId,
    pub source: Option<ObjRef>,
}

impl Game {
    /// The total mana cost of casting `card` (CR 601.2f): its mana cost
    /// plus the increases of static abilities ("noncreature spells cost {1}
    /// more").
    pub(crate) fn spell_cost(&self, card: CardIdx, p: PlayerId) -> crate::mana::ManaCost {
        let mut cost = self.def_of(card).cost;
        let ctx = Ctx { controller: p, source: None };
        for &c in &self.battlefield {
            for (f, n) in &self.def_of(c).spell_tax {
                if self.card_matches(f, card, &ctx) {
                    cost.generic = cost.generic.saturating_add(*n);
                }
            }
        }
        cost
    }

    // ---- priority options (CR 117.1, 116.2a, 307.1, 602.2) ----------------

    pub(crate) fn priority_options(&self, p: PlayerId) -> Vec<Opt> {
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        let sorcery_ok = self.turn.active == p && self.turn.step.is_main() && self.stack.is_empty();
        let land_ok = sorcery_ok && self.players[p as usize].lands_played < 1;
        let classes = self.mana_classes(p, None);
        let mut plain = None;
        let slots = self.hand_slots(p);
        for &(action, c) in &slots {
            let def = self.def_of(c);
            if def.is(types::LAND) {
                if land_ok {
                    opts.push(Opt { action, choice: Choice::Play(c) });
                }
                continue;
            }
            let instant_speed = def.is(types::INSTANT) || def.keywords.has(Kw::Flash);
            if !(instant_speed || sorcery_ok) {
                continue;
            }
            let cls = &classes[..];
            let cost = self.spell_cost(c, p);
            if !def.additional.is_empty() {
                // (Excluding the card itself: on the stack while it is paid.)
                if !def.additional.iter().any(|o| self.cost_payable(p, c, o, &cost, cls, Some(c))) {
                    continue;
                }
            } else if self.hand_mana_source(c) {
                // A card cast from hand is on the stack while its cost is
                // paid: its own mana ability (a Spirit Guide's) cannot help.
                if !self.can_pay_with(p, &cost, &classes_without(cls, c), &Default::default()) {
                    continue;
                }
            } else {
                let plain = plain.get_or_insert_with(|| self.plain_cands(p, cls));
                if !self.can_pay_plain(p, &cost, cls, plain) {
                    continue;
                }
            }
            if let Some(ab) = &def.spell {
                if !self.targets_available(ab, p, Some(self.obj_ref(c))) {
                    continue;
                }
            }
            opts.push(Opt { action, choice: Choice::Cast(c) });
        }
        self.activation_options(p, &classes, &slots, sorcery_ok, &mut opts);
        opts
    }

    /// Every target requirement has at least `min` legal candidates; for a
    /// modal ability, enough modes can be chosen (a mode whose targets
    /// cannot be chosen cannot be, CR 700.2, 601.2c).
    pub(crate) fn targets_available(&self, ab: &AbilityDef, controller: PlayerId, source: Option<ObjRef>) -> bool {
        match &ab.modes {
            None => {
                let ctx = Ctx { controller, source };
                ab.targets.iter().all(|tr| tr.min == 0 || self.has_candidates(tr, &ctx, tr.min as usize))
            }
            Some(m) => {
                let avail = m.list.iter().filter(|x| self.targets_available(x, controller, source)).count();
                m.min == 0 || if m.repeat { avail > 0 } else { avail >= m.min as usize }
            }
        }
    }

    /// 601.2b / 603.3c: choose the modes of stack item `id`, one decision
    /// per mode instance, in non-decreasing order (each choice of modes
    /// once); only modes whose targets can be chosen, and only while the
    /// rest can still reach the minimum.
    pub(crate) fn choose_modes(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let Some(ar) = self.item_ability(item) else { return };
        let Some(m) = &self.db.ability(ar).modes else { return };
        match self.answer.take() {
            Some(Choice::Number(k)) => {
                self.note(|_| format!("mode {k}"));
                self.stack[pos].modes.push(k);
                self.agenda.push(Task::ChooseModes { id });
                return;
            }
            Some(Choice::Pass) => {
                // 700.2b: a triggered ability with no mode chosen is
                // removed from the stack.
                if item.modes.is_empty() && matches!(ar.slot, crate::db::AbilitySlot::Trigger(_)) {
                    self.note(|_| "no mode: the trigger is removed".to_string());
                    self.stack.remove(pos);
                }
                return;
            }
            Some(other) => unreachable!("mode answer {other:?}"),
            None => {}
        }
        let chosen = item.modes.len();
        if chosen >= m.max as usize {
            return;
        }
        let (p, source) = (item.controller, Some(self.item_source(item)));
        let avail: Vec<bool> = m.list.iter().map(|x| self.targets_available(x, p, source)).collect();
        let floor = item.modes.last().map_or(0, |&l| if m.repeat { l } else { l + 1 }) as usize;
        let need = (m.min as usize).saturating_sub(chosen + 1);
        let mut opts = Vec::new();
        for k in floor..m.list.len() {
            // After k, the later picks come from k on (k itself if modes
            // repeat): enough must be available.
            let later = if m.repeat { avail[k] as usize * need } else { avail[k + 1..].iter().filter(|&&a| a).count() };
            if avail[k] && later >= need {
                opts.push(Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) });
            }
        }
        if chosen >= m.min as usize {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        self.ask(Task::ChooseModes { id }, p, DecisionKind::ChooseMode, opts);
    }

    /// At least `n` legal candidates for `tr` (the same order and rules as
    /// [`target_candidates`](Self::target_candidates), without building it).
    fn has_candidates(&self, tr: &TargetReq, ctx: &Ctx, n: usize) -> bool {
        let ok = |t: Target| self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx);
        let players = [ctx.controller, crate::ids::opp(ctx.controller)].into_iter().map(Target::Player);
        let perms = self.battlefield.iter().map(|&c| Target::Obj(self.obj_ref(c)));
        let spells = self.stack.iter().rev().filter_map(|s| match s.kind {
            StackKind::Spell { card } => Some(self.obj_ref(card)).filter(|&r| Some(r) != ctx.source).map(Target::Obj),
            StackKind::Ability { .. } => None,
        });
        let gy = wants_graveyard(&tr.filter);
        let graves = [ctx.controller, crate::ids::opp(ctx.controller)]
            .into_iter()
            .filter(move |_| gy)
            .flat_map(|q| self.players[q as usize].graveyard.iter().map(|&c| Target::Obj(self.obj_ref(c))));
        players.chain(perms).chain(spells).chain(graves).filter(|&t| ok(t)).nth(n - 1).is_some()
    }

    // ---- casting (CR 601.2) ------------------------------------------------

    /// 601.2a: the card moves to the stack; then targets (601.2c), cost
    /// (601.2f-h) and "becomes cast" (601.2i) run as tasks.
    pub(crate) fn begin_cast(&mut self, p: PlayerId, card: CardIdx) {
        self.note(|g| format!("P{p} casts {}", g.name(card)));
        self.move_card_to_stack(card, p);
        let id = self.new_stack_id();
        self.stack.push(StackItem {
            id,
            kind: StackKind::Spell { card },
            controller: p,
            targets: Vec::new(),
            cost_option: 0,
            modes: Vec::new(),
            ev: Default::default(),
        });
        self.agenda.push(Task::CastDone { id });
        self.agenda.push(Task::CastPay { id });
        if self.def_of(card).spell.as_ref().is_some_and(|ab| ab.has_targets()) {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
        }
        if self.def_of(card).additional.len() > 1 {
            self.agenda.push(Task::CastOption { id });
        }
        if self.def_of(card).spell.as_ref().is_some_and(|ab| ab.modes.is_some()) {
            self.agenda.push(Task::ChooseModes { id }); // CR 601.2b, 700.2a
        }
    }

    /// 601.2b: choose which additional cost to pay (among those that can
    /// be paid along with the rest of the cost).
    pub(crate) fn cast_option(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let p = item.controller;
        match self.answer.take() {
            Some(Choice::Number(k)) => self.stack[pos].cost_option = k,
            Some(other) => unreachable!("cost option answer {other:?}"),
            None => {
                let def = self.def_of(card);
                let classes = self.mana_classes(p, None);
                let cost = self.spell_cost(card, p);
                let opts: Vec<Opt> = (0..def.additional.len())
                    .filter(|&k| self.cost_payable(p, card, &def.additional[k], &cost, &classes, None))
                    .map(|k| Opt { action: act::NUMBER + k as u16, choice: Choice::Number(k as u8) })
                    .collect();
                self.ask(Task::CastOption { id }, p, DecisionKind::ChooseCost, opts);
            }
        }
    }

    pub(crate) fn new_stack_id(&mut self) -> u32 {
        let id = self.next_stack_id;
        self.next_stack_id += 1;
        id
    }

    pub(crate) fn item_index(&self, id: u32) -> Option<usize> {
        self.stack.iter().position(|s| s.id == id)
    }

    /// The ability a stack item resolves (`None`: a permanent spell).
    pub(crate) fn item_ability(&self, item: &StackItem) -> Option<AbilityRef> {
        match item.kind {
            StackKind::Spell { card } => {
                let def = self.objs[card.i()].def;
                self.db.def(def).spell.as_ref().map(|_| AbilityRef { def, slot: AbilitySlot::Spell, mode: None })
            }
            StackKind::Ability { ability, .. } => Some(ability),
        }
    }

    /// The object a stack item's ability comes from (the spell itself, or
    /// the permanent of an ability).
    pub(crate) fn item_source(&self, item: &StackItem) -> ObjRef {
        match item.kind {
            StackKind::Spell { card } => self.obj_ref(card),
            StackKind::Ability { source, .. } => source,
        }
    }

    /// Choose targets for stack item `id`, requirement `req` onwards
    /// (CR 601.2c / 603.3d; one decision per target).
    pub(crate) fn choose_targets(&mut self, id: u32, req: u8) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let Some(ar) = self.item_ability(item) else { return };
        let reqs = self.db.ability(ar).reqs(&item.modes);
        let Some(&tr) = reqs.get(req as usize) else { return };
        let ctx = Ctx { controller: item.controller, source: Some(self.item_source(item)) };
        let already: Vec<Target> =
            item.targets.iter().filter(|(r, _)| *r == req).map(|&(_, t)| t).collect();
        match self.answer.take() {
            Some(Choice::Pass) => {
                self.agenda.push(Task::ChooseTargets { id, req: req + 1 });
                return;
            }
            Some(Choice::Target(t)) => {
                self.note(|g| format!("target {}", g.describe_target(t)));
                self.stack[pos].targets.push((req, t));
                self.agenda.push(Task::ChooseTargets { id, req });
                return;
            }
            Some(other) => unreachable!("target answer {other:?}"),
            None => {}
        }
        if already.len() >= tr.max as usize {
            self.agenda.push(Task::ChooseTargets { id, req: req + 1 });
            return;
        }
        let cands = self.target_candidates(tr, &ctx, &already);
        let mut opts: Vec<Opt> = cands
            .iter()
            .map(|&t| Opt { action: self.target_action(ctx.controller, t), choice: Choice::Target(t) })
            .collect();
        if already.len() >= tr.min as usize {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        if opts.is_empty() {
            // Only reachable if legality changed between the cast check and
            // now, which nothing can do yet.
            panic!("no legal target for a required target");
        }
        self.ask(Task::ChooseTargets { id, req }, ctx.controller, DecisionKind::Target, opts);
    }

    /// 601.2f-h: determine the total cost and pay it.
    pub(crate) fn cast_pay(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let def = self.def_of(card);
        let mut pm = PayMana::new(item.controller, self.spell_cost(card, item.controller));
        if let Some(extra) = def.additional.get(item.cost_option as usize) {
            pm.keep = Self::keep_for(card, extra, None);
            self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
        }
        self.agenda.push(Task::PayMana(pm));
    }

    /// 601.2i: the spell becomes cast; "whenever you cast" abilities
    /// trigger.
    pub(crate) fn cast_done(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let p = item.controller;
        let pl = &mut self.players[p as usize];
        pl.spells_cast = pl.spells_cast.saturating_add(1);
        if !self.def_of(card).is(types::CREATURE) {
            let pl = &mut self.players[p as usize];
            pl.noncreature_cast = pl.noncreature_cast.saturating_add(1);
        }
        let r = self.obj_ref(card);
        self.event(crate::triggers::Event {
            kind: crate::db::EventKind::IsCast,
            a: crate::triggers::Subj::Obj(r),
            b: crate::triggers::Subj::Player(p),
            amount: 0,
        });
    }

    // ---- targets (CR 115) --------------------------------------------------

    /// Legal candidates for a target requirement, in canonical order:
    /// players (controller first), permanents (battlefield order), spells
    /// (stack, top first). A spell or ability cannot target itself (115.5);
    /// `exclude` holds the targets already chosen for this requirement
    /// (the same object once per "target", 115.3).
    pub(crate) fn target_candidates(&self, tr: &TargetReq, ctx: &Ctx, exclude: &[Target]) -> Vec<Target> {
        let mut out = Vec::new();
        let consider = |t: Target, out: &mut Vec<Target>| {
            if !exclude.contains(&t) && self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx) {
                out.push(t);
            }
        };
        for q in [ctx.controller, crate::ids::opp(ctx.controller)] {
            consider(Target::Player(q), &mut out);
        }
        for &c in &self.battlefield {
            consider(Target::Obj(self.obj_ref(c)), &mut out);
        }
        for s in self.stack.iter().rev() {
            if let StackKind::Spell { card } = s.kind {
                let r = self.obj_ref(card);
                if Some(r) != ctx.source {
                    consider(Target::Obj(r), &mut out);
                }
            }
        }
        if wants_graveyard(&tr.filter) {
            for q in [ctx.controller, crate::ids::opp(ctx.controller)] {
                for &c in &self.players[q as usize].graveyard {
                    consider(Target::Obj(self.obj_ref(c)), &mut out);
                }
            }
        }
        out
    }

    /// Hexproof and shroud (CR 702.11, 702.18); protection (702.16b: not
    /// by spells with that quality, nor abilities from sources with it).
    pub(crate) fn targetable(&self, t: Target, ctx: &Ctx) -> bool {
        match t {
            Target::Player(_) => true,
            Target::Obj(r) => {
                if self.objs[r.idx.i()].zone != Zone::Battlefield {
                    return true;
                }
                let k = self.keywords_of(r.idx);
                if k.has(Kw::Shroud) || (k.has(Kw::Hexproof) && self.objs[r.idx.i()].controller != ctx.controller) {
                    return false;
                }
                match self.source_quality(ctx) {
                    Some(q) => !self.protected_from(r.idx, &q),
                    None => true,
                }
            }
        }
    }

    /// The characteristics of the object a spell or ability comes from:
    /// the object itself while it exists, else its definition (last known
    /// information), found through the ability on the stack or waiting to
    /// be put there (never through the card, which may be hidden now).
    pub(crate) fn source_quality(&self, ctx: &Ctx) -> Option<crate::game::DamageSource> {
        let r = ctx.source?;
        if self.live(r) && matches!(self.objs[r.idx.i()].zone, Zone::Battlefield | Zone::Stack | Zone::Hand) {
            return Some(crate::game::DamageSource { controller: ctx.controller, ..self.damage_source_of(r.idx) });
        }
        let def = self
            .stack
            .iter()
            .find_map(|s| match s.kind {
                StackKind::Ability { ability, source } if source == r => Some(ability.def),
                _ => None,
            })
            .or_else(|| self.waiting.iter().find(|w| w.source == r).map(|w| w.source_def))?;
        let d = self.db.def(def);
        Some(crate::game::DamageSource {
            controller: ctx.controller,
            deathtouch: false,
            lifelink: false,
            colors: d.colors,
            types: d.types,
        })
    }

    /// Is `t` (still) a legal choice for `tr` (CR 608.2b)?
    pub(crate) fn target_legal(&self, tr: &TargetReq, t: Target, ctx: &Ctx) -> bool {
        if let Target::Obj(r) = t {
            if !self.live(r) {
                return false;
            }
        }
        self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx)
    }

    /// Top-level filter. Players and permanents are one universe (a bare
    /// `Not(Creature)` matches players too, as in the IR); characteristic
    /// predicates mean permanents. Spells on the stack match only through
    /// `Spell(f)` (possibly inside `And` / `Or`).
    pub(crate) fn filter_ok(&self, f: &Filter, t: Target, ctx: &Ctx) -> bool {
        match t {
            Target::Player(p) => match f {
                Filter::Any | Filter::Player => true,
                Filter::Opponent => p != ctx.controller,
                Filter::DrewThisTurn(n) => self.players[p as usize].drawn == *n,
                Filter::And(a, b) => self.filter_ok(a, t, ctx) && self.filter_ok(b, t, ctx),
                Filter::Or(v) => v.iter().any(|x| self.filter_ok(x, t, ctx)),
                Filter::Not(x) => !self.filter_ok(x, t, ctx),
                _ => false,
            },
            Target::Obj(r) => {
                if !self.live(r) {
                    return false;
                }
                match self.objs[r.idx.i()].zone {
                    Zone::Battlefield => self.perm_ok(f, r.idx, ctx),
                    Zone::Stack => self.spell_ok(f, r.idx, ctx),
                    Zone::Graveyard => self.graveyard_ok(f, r.idx, ctx),
                    _ => false,
                }
            }
        }
    }

    fn perm_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::Player
            | Filter::Spell(_)
            | Filter::Opponent
            | Filter::DrewThisTurn(_)
            | Filter::InGraveyard(_) => false,
            Filter::And(a, b) => self.perm_ok(a, c, ctx) && self.perm_ok(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.perm_ok(x, c, ctx)),
            Filter::Not(x) => !self.perm_ok(x, c, ctx),
            _ => self.card_matches(f, c, ctx),
        }
    }

    /// A card in a graveyard matches only through `InGraveyard(f)`.
    fn graveyard_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::InGraveyard(inner) => self.card_matches(inner, c, ctx),
            Filter::And(a, b) => self.graveyard_ok(a, c, ctx) && self.graveyard_ok(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.graveyard_ok(x, c, ctx)),
            _ => false,
        }
    }

    fn spell_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::Spell(inner) => self.card_matches(inner, c, ctx),
            Filter::And(a, b) => self.spell_ok(a, c, ctx) && self.spell_ok(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.spell_ok(x, c, ctx)),
            _ => false,
        }
    }

    /// A filter applied to an object's characteristics (and its controller
    /// and identity relative to `ctx`), whatever its zone.
    pub(crate) fn card_matches(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        let def = self.def_of(c);
        match f {
            Filter::Any => true,
            Filter::Player | Filter::Spell(_) | Filter::Opponent => false,
            Filter::Type(bits) => self.types_of(c) & bits != 0,
            Filter::Color(m) => self.colors_of(c) & m != 0,
            Filter::TotalPtAtMost(n) => self.is_creature(c) && self.power(c) + self.toughness(c) <= *n,
            Filter::Subtype(s) => self.has_subtype(c, *s),
            Filter::Supertype(bits) => def.supertypes & bits != 0,
            Filter::ManaValueAtMost(n) => def.mana_value() <= *n,
            Filter::PowerAtLeast(n) => self.is_creature(c) && self.power(c) >= *n,
            Filter::PowerAtMost(n) => self.is_creature(c) && self.power(c) <= *n,
            Filter::ToughnessAtMost(n) => self.is_creature(c) && self.toughness(c) <= *n,
            Filter::Controller(you) => (self.objs[c.i()].controller == ctx.controller) == *you,
            Filter::Source => ctx.source.is_some_and(|s| s.idx == c && self.live(s)),
            Filter::Token => def.token,
            Filter::Attacking => self.objs[c.i()].attacking,
            Filter::Tapped => self.objs[c.i()].tapped,
            Filter::DrewThisTurn(_) | Filter::InGraveyard(_) => false,
            Filter::Owner(you) => (self.objs[c.i()].owner == ctx.controller) == *you,
            Filter::And(a, b) => self.card_matches(a, c, ctx) && self.card_matches(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.card_matches(x, c, ctx)),
            Filter::Not(x) => !self.card_matches(x, c, ctx),
        }
    }
}

/// Can a filter match a card in a graveyard (`InGraveyard`)?
fn wants_graveyard(f: &Filter) -> bool {
    match f {
        Filter::InGraveyard(_) => true,
        Filter::And(a, b) => wants_graveyard(a) || wants_graveyard(b),
        Filter::Or(v) => v.iter().any(wants_graveyard),
        _ => false,
    }
}

/// `classes` without the sources of one card.
pub(crate) fn classes_without(classes: &[crate::pay::Class], card: CardIdx) -> Vec<crate::pay::Class> {
    classes
        .iter()
        .filter_map(|cl| {
            let members: Vec<_> = cl.members.iter().copied().filter(|s| s.card != card).collect();
            (!members.is_empty()).then(|| crate::pay::Class { members, ..cl.clone() })
        })
        .collect()
}
