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
    // ---- priority options (CR 117.1, 116.2a, 307.1, 602.2) ----------------

    pub(crate) fn priority_options(&self, p: PlayerId) -> Vec<Opt> {
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        let sorcery_ok = self.turn.active == p && self.turn.step.is_main() && self.stack.is_empty();
        let land_ok = sorcery_ok && self.players[p as usize].lands_played < 1;
        let classes = self.mana_classes(p, None);
        for (action, c) in self.hand_slots(p) {
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
            // The card moves to the stack before its cost is paid (601.2a),
            // so it cannot pay for itself (a Spirit Guide).
            let own: Vec<crate::pay::Class>;
            let cls = if def.hand_abilities.iter().any(|a| a.mana().is_some()) {
                own = classes_without(&classes, c);
                &own[..]
            } else {
                &classes[..]
            };
            if def.additional.is_empty() {
                let keep = crate::pay::Keep { exclude: Some(c), ..Default::default() };
                if !self.can_pay_with(p, &def.cost, cls, &keep) {
                    continue;
                }
            } else if !def.additional.iter().any(|o| self.cost_payable(p, c, o, &def.cost, cls, Some(c))) {
                continue;
            }
            if let Some(ab) = &def.spell {
                if !self.targets_available(ab, p, None) {
                    continue;
                }
            }
            opts.push(Opt { action, choice: Choice::Cast(c) });
        }
        self.activation_options(p, &classes, sorcery_ok, &mut opts);
        opts
    }

    /// Every target requirement has at least `min` legal candidates.
    pub(crate) fn targets_available(&self, ab: &AbilityDef, controller: PlayerId, source: Option<ObjRef>) -> bool {
        let ctx = Ctx { controller, source };
        ab.targets.iter().all(|tr| self.target_candidates(tr, &ctx, &[]).len() >= tr.min as usize)
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
        });
        self.agenda.push(Task::CastDone { id });
        self.agenda.push(Task::CastPay { id });
        if self.def_of(card).spell.as_ref().is_some_and(|ab| !ab.targets.is_empty()) {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
        }
        if self.def_of(card).additional.len() > 1 {
            self.agenda.push(Task::CastOption { id });
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
                let opts: Vec<Opt> = (0..def.additional.len())
                    .filter(|&k| self.cost_payable(p, card, &def.additional[k], &def.cost, &classes, None))
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
                self.db.def(def).spell.as_ref().map(|_| AbilityRef { def, slot: AbilitySlot::Spell })
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
        let ab = self.db.ability(ar);
        let Some(tr) = ab.targets.get(req as usize) else { return };
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
        let mut pm = PayMana::new(item.controller, def.cost);
        if let Some(extra) = def.additional.get(item.cost_option as usize) {
            pm.keep = Self::keep_for(card, extra, None);
            self.agenda.push(Task::PayCosts { id, picked: Vec::new() });
        }
        self.agenda.push(Task::PayMana(pm));
    }

    /// 601.2i: the spell becomes cast (cast triggers come here later).
    pub(crate) fn cast_done(&mut self, _id: u32) {}

    // ---- targets (CR 115) --------------------------------------------------

    /// Legal candidates for a target requirement, in canonical order:
    /// players (controller first), permanents (battlefield order), spells
    /// (stack, top first). A spell or ability cannot target itself (115.5);
    /// `exclude` holds the targets already chosen for this requirement
    /// (the same object once per "target", 115.3).
    pub(crate) fn target_candidates(&self, tr: &TargetReq, ctx: &Ctx, exclude: &[Target]) -> Vec<Target> {
        let mut out = Vec::new();
        let consider = |t: Target, out: &mut Vec<Target>| {
            if !exclude.contains(&t) && self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx.controller) {
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
        out
    }

    /// Hexproof and shroud (CR 702.11, 702.18); protection and ward later.
    pub(crate) fn targetable(&self, t: Target, by: PlayerId) -> bool {
        match t {
            Target::Player(_) => true,
            Target::Obj(r) => {
                if self.objs[r.idx.i()].zone != Zone::Battlefield {
                    return true;
                }
                !(self.has_kw(r.idx, Kw::Shroud)
                    || (self.has_kw(r.idx, Kw::Hexproof) && self.objs[r.idx.i()].controller != by))
            }
        }
    }

    /// Is `t` (still) a legal choice for `tr` (CR 608.2b)?
    pub(crate) fn target_legal(&self, tr: &TargetReq, t: Target, ctx: &Ctx) -> bool {
        if let Target::Obj(r) = t {
            if !self.live(r) {
                return false;
            }
        }
        self.filter_ok(&tr.filter, t, ctx) && self.targetable(t, ctx.controller)
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
                    _ => false,
                }
            }
        }
    }

    fn perm_ok(&self, f: &Filter, c: CardIdx, ctx: &Ctx) -> bool {
        match f {
            Filter::Player | Filter::Spell(_) | Filter::Opponent => false,
            Filter::And(a, b) => self.perm_ok(a, c, ctx) && self.perm_ok(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.perm_ok(x, c, ctx)),
            Filter::Not(x) => !self.perm_ok(x, c, ctx),
            _ => self.card_matches(f, c, ctx),
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
            Filter::Color(m) => def.colors & m != 0,
            Filter::TotalPtAtMost(n) => self.is_creature(c) && self.power(c) + self.toughness(c) <= *n,
            Filter::Subtype(s) => def.subtypes.contains(s),
            Filter::Supertype(bits) => def.supertypes & bits != 0,
            Filter::ManaValueAtMost(n) => def.mana_value() <= *n,
            Filter::PowerAtLeast(n) => self.is_creature(c) && self.power(c) >= *n,
            Filter::PowerAtMost(n) => self.is_creature(c) && self.power(c) <= *n,
            Filter::ToughnessAtMost(n) => self.is_creature(c) && self.toughness(c) <= *n,
            Filter::Controller(you) => (self.objs[c.i()].controller == ctx.controller) == *you,
            Filter::Source => ctx.source.is_some_and(|s| s.idx == c && self.live(s)),
            Filter::And(a, b) => self.card_matches(a, c, ctx) && self.card_matches(b, c, ctx),
            Filter::Or(v) => v.iter().any(|x| self.card_matches(x, c, ctx)),
            Filter::Not(x) => !self.card_matches(x, c, ctx),
        }
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
