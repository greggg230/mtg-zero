//! Casting spells, choosing targets, paying mana (CR 601, 115; ENGINE.md 6).

use crate::db::{types, AbilityDef, AbilityRef, AbilitySlot, Filter, Kw, TargetReq};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::{PayMana, Task};
use crate::game::{Game, StackItem, StackKind};
use crate::ids::{CardIdx, ObjRef, PlayerId, Target, Zone};
use crate::mana::{payable, ManaCost, ManaMask};

impl Game {
    // ---- priority options (CR 117.1, 116.2a, 307.1) -----------------------

    pub(crate) fn priority_options(&self, p: PlayerId) -> Vec<Opt> {
        let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
        let sorcery_ok = self.turn.active == p && self.turn.step.is_main() && self.stack.is_empty();
        let land_ok = sorcery_ok && self.players[p as usize].lands_played < 1;
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
            if !self.can_pay(p, &def.cost) {
                continue;
            }
            if let Some(ab) = &def.spell {
                if !self.targets_available(ab, p, None) {
                    continue;
                }
            }
            opts.push(Opt { action, choice: Choice::Cast(c) });
        }
        opts
    }

    /// Every target requirement has at least `min` legal candidates.
    pub(crate) fn targets_available(&self, ab: &AbilityDef, controller: PlayerId, source: Option<ObjRef>) -> bool {
        ab.targets.iter().all(|tr| self.target_candidates(tr, controller, source, &[]).len() >= tr.min as usize)
    }

    // ---- casting (CR 601.2) ------------------------------------------------

    /// 601.2a: the card moves to the stack; then targets (601.2c), cost
    /// (601.2f-h) and "becomes cast" (601.2i) run as tasks.
    pub(crate) fn begin_cast(&mut self, p: PlayerId, card: CardIdx) {
        self.note(|g| format!("P{p} casts {}", g.name(card)));
        self.move_card_to_stack(card, p);
        let id = self.new_stack_id();
        self.stack.push(StackItem { id, kind: StackKind::Spell { card }, controller: p, targets: Vec::new() });
        self.agenda.push(Task::CastDone { id });
        self.agenda.push(Task::CastPay { id });
        if self.def_of(card).spell.as_ref().is_some_and(|ab| !ab.targets.is_empty()) {
            self.agenda.push(Task::ChooseTargets { id, req: 0 });
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
        let controller = item.controller;
        let source = self.item_source(item);
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
        let cands = self.target_candidates(tr, controller, Some(source), &already);
        let mut opts: Vec<Opt> = cands
            .iter()
            .map(|&t| Opt { action: self.target_action(controller, t), choice: Choice::Target(t) })
            .collect();
        if already.len() >= tr.min as usize {
            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        if opts.is_empty() {
            // Only reachable if legality changed between the cast check and
            // now, which nothing can do yet.
            panic!("no legal target for a required target");
        }
        self.ask(Task::ChooseTargets { id, req }, controller, DecisionKind::Target, opts);
    }

    /// 601.2f-h: determine the total cost and pay it.
    pub(crate) fn cast_pay(&mut self, id: u32) {
        let Some(pos) = self.item_index(id) else { return };
        let item = &self.stack[pos];
        let StackKind::Spell { card } = item.kind else { return };
        let cost = self.def_of(card).cost;
        let payer = item.controller;
        self.agenda.push(Task::PayMana(PayMana { payer, cost, chosen: Vec::new(), min_class: 0, pooled: false }));
    }

    /// 601.2i: the spell becomes cast (cast triggers come here later).
    pub(crate) fn cast_done(&mut self, _id: u32) {}

    // ---- targets (CR 115) --------------------------------------------------

    /// Legal candidates for a target requirement, in canonical order:
    /// players (controller first), permanents (battlefield order), spells
    /// (stack, top first). A spell or ability cannot target itself (115.5);
    /// `exclude` holds the targets already chosen for this requirement
    /// (the same object once per "target", 115.3).
    pub(crate) fn target_candidates(
        &self,
        tr: &TargetReq,
        controller: PlayerId,
        source: Option<ObjRef>,
        exclude: &[Target],
    ) -> Vec<Target> {
        let mut out = Vec::new();
        let consider = |t: Target, out: &mut Vec<Target>| {
            if !exclude.contains(&t) && self.filter_ok(&tr.filter, t) && self.targetable(t, controller) {
                out.push(t);
            }
        };
        for q in [controller, crate::ids::opp(controller)] {
            consider(Target::Player(q), &mut out);
        }
        for &c in &self.battlefield {
            consider(Target::Obj(self.obj_ref(c)), &mut out);
        }
        for s in self.stack.iter().rev() {
            if let StackKind::Spell { card } = s.kind {
                let r = self.obj_ref(card);
                if Some(r) != source {
                    consider(Target::Obj(r), &mut out);
                }
            }
        }
        out
    }

    /// Hexproof, shroud, protection and ward hook in here (none yet).
    pub(crate) fn targetable(&self, _t: Target, _by: PlayerId) -> bool {
        true
    }

    /// Is `t` (still) a legal choice for `tr` (CR 608.2b)?
    pub(crate) fn target_legal(&self, tr: &TargetReq, t: Target, controller: PlayerId) -> bool {
        if let Target::Obj(r) = t {
            if !self.live(r) {
                return false;
            }
        }
        self.filter_ok(&tr.filter, t) && self.targetable(t, controller)
    }

    /// Top-level filter: type predicates mean permanents, `Spell(f)` a spell
    /// on the stack, `Player` a player.
    pub(crate) fn filter_ok(&self, f: &Filter, t: Target) -> bool {
        match f {
            Filter::Any => true,
            Filter::Player => matches!(t, Target::Player(_)),
            Filter::Type(bits) => match t {
                Target::Obj(r) => self.on_battlefield(r) && self.types_of(r.idx) & bits != 0,
                Target::Player(_) => false,
            },
            Filter::TotalPtAtMost(n) => match t {
                Target::Obj(r) => {
                    self.on_battlefield(r)
                        && self.is_creature(r.idx)
                        && self.power(r.idx) + self.toughness(r.idx) <= *n
                }
                Target::Player(_) => false,
            },
            Filter::Spell(inner) => match t {
                Target::Obj(r) => {
                    self.live(r) && self.objs[r.idx.i()].zone == Zone::Stack && self.card_matches(inner, r.idx)
                }
                Target::Player(_) => false,
            },
            Filter::And(a, b) => self.filter_ok(a, t) && self.filter_ok(b, t),
            Filter::Or(v) => v.iter().any(|x| self.filter_ok(x, t)),
            Filter::Not(x) => !self.filter_ok(x, t),
        }
    }

    fn on_battlefield(&self, r: ObjRef) -> bool {
        self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield
    }

    /// A filter applied to a card's characteristics, whatever its zone.
    pub(crate) fn card_matches(&self, f: &Filter, c: CardIdx) -> bool {
        match f {
            Filter::Any => true,
            Filter::Player | Filter::Spell(_) => false,
            Filter::Type(bits) => self.types_of(c) & bits != 0,
            Filter::TotalPtAtMost(n) => self.is_creature(c) && self.power(c) + self.toughness(c) <= *n,
            Filter::And(a, b) => self.card_matches(a, c) && self.card_matches(b, c),
            Filter::Or(v) => v.iter().any(|x| self.card_matches(x, c)),
            Filter::Not(x) => !self.card_matches(x, c),
        }
    }

    // ---- mana (CR 106, 601.2g-h; ENGINE.md 6) ------------------------------

    /// `p`'s untapped mana sources in battlefield order with their masks.
    pub(crate) fn mana_sources(&self, p: PlayerId) -> Vec<(CardIdx, ManaMask)> {
        self.permanents_of(p)
            .filter(|&c| {
                let o = &self.objs[c.i()];
                // {T} abilities of creatures need 302.6 (no creature sources yet).
                !o.tapped && self.def_of(c).produces != 0 && !(self.is_creature(c) && self.is_sick(c))
            })
            .map(|c| (c, self.def_of(c).produces))
            .collect()
    }

    /// Can `p` pay `cost` from their pool plus untapped sources?
    pub fn can_pay(&self, p: PlayerId, cost: &ManaCost) -> bool {
        let mut c = *cost;
        let mut pool = self.players[p as usize].pool;
        pool.pay_into(&mut c);
        if c.is_zero() {
            return true;
        }
        let masks: Vec<ManaMask> = self.mana_sources(p).iter().map(|s| s.1).collect();
        payable(&c, &[], &masks)
    }

    /// Classes of equivalent mana sources (same definition; untapped lands
    /// have no other relevant state yet), in order of first appearance.
    fn source_classes(&self, p: PlayerId) -> Vec<Vec<(CardIdx, ManaMask)>> {
        let mut classes: Vec<Vec<(CardIdx, ManaMask)>> = Vec::new();
        for (c, m) in self.mana_sources(p) {
            let def = self.objs[c.i()].def;
            match classes.iter_mut().find(|cl| self.objs[cl[0].0.i()].def == def) {
                Some(cl) => cl.push((c, m)),
                None => classes.push(vec![(c, m)]),
            }
        }
        classes
    }

    /// The `PayMana` task (ENGINE.md 6).
    pub(crate) fn pay_mana(&mut self, mut pm: PayMana) {
        if !pm.pooled {
            self.players[pm.payer as usize].pool.pay_into(&mut pm.cost);
            pm.pooled = true;
        }
        let classes = self.source_classes(pm.payer);
        if let Some(ch) = self.answer.take() {
            let Choice::Card(c) = ch else { unreachable!("pay answer {ch:?}") };
            pm.chosen.push(c);
            pm.min_class = classes.iter().position(|cl| cl.iter().any(|s| s.0 == c)).expect("source class") as u8;
        }
        if pm.chosen.len() == pm.cost.total() as usize {
            for &c in &pm.chosen {
                self.tap(c); // the mana ability's cost; its mana is spent at once
            }
            let payer = pm.payer;
            let n = pm.chosen.len();
            self.note(|_| format!("P{payer} pays with {n} sources"));
            return;
        }
        let mask_of = |c: CardIdx| self.def_of(c).produces;
        let chosen_masks: Vec<ManaMask> = pm.chosen.iter().map(|&c| mask_of(c)).collect();
        let mut opts = Vec::new();
        for (ci, cl) in classes.iter().enumerate().skip(pm.min_class as usize) {
            let Some(&(rep, m)) = cl.iter().find(|s| !pm.chosen.contains(&s.0)) else { continue };
            let mut ch = chosen_masks.clone();
            ch.push(m);
            // Only classes from this one on can complete the set (the
            // canonical order), so feasibility is checked against those.
            let avail: Vec<ManaMask> = classes[ci..]
                .iter()
                .flatten()
                .filter(|s| s.0 != rep && !pm.chosen.contains(&s.0))
                .map(|s| s.1)
                .collect();
            if payable(&pm.cost, &ch, &avail) {
                opts.push(Opt { action: self.perm_action(pm.payer, rep), choice: Choice::Card(rep) });
            }
        }
        let payer = pm.payer;
        self.ask(Task::PayMana(pm), payer, DecisionKind::PayMana, opts);
    }
}
