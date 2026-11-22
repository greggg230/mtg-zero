//! Which sources of a mana payment are interchangeable (CR 601.2g-h, 702.51a, 702.126a;
//! ENGINE.md 229.2). Named after the rule, never after a card.
//!
//! The payment (`pay::PayMana`) offers its sources by class (`pay::Class`): one option a class,
//! and the pick takes the class's first free member whose pick leaves the payment payable
//! (ENGINE.md 18, 49). So a class's members must be rules-equivalent in every reachable state:
//! whichever one is tapped or sacrificed, nothing the game does later can tell. The class
//! builders (`mana_classes`, `convoke_classes`, `improvise_classes`, the shared activations')
//! group by definition and ability (what a source yields), not by the permanent's state or by
//! what names it: two creatures of one definition, one summoning sick or one with a +1/+1
//! counter, were one option of Vote Out's convoke (ENGINE.md 220.1.5), two Lion's Eye Diamonds
//! one tapped and one not one option, a land creature with damage and its undamaged twin one.
//!
//! [`Game::split_unlike`] splits each class of a payment (`Game::payment_classes`) into runs of
//! twins, in the class's member order (so the classes keep the canonical order the payment's
//! picks go up: ENGINE.md 18). Two permanents are twins when
//!
//! * everything of its own the game keeps about each is equal ([`Twin`]): every field of `Obj`
//!   but its identity (its slot, incarnation, timestamp; `control_since` and `entered` as the only
//!   facts the rules read of them, summoning sickness, CR 302.6, and "entered this turn"), its
//!   counters (122), its marks (`Game::marks`, compared as a multiset), token and copy status
//!   (111, 707), a face-down permanent's identity (708) and what its controller knows of it, its
//!   loyalty activations this turn (606.3), and its other records of this turn keyed by it alone:
//!   how many times its abilities resolved, its once-each-turn actions done, whether it attacked;
//! * nothing else in the state names either of them (`Game::named_objects`: every holder of an
//!   object reference - the stack, the agenda's tasks, triggers waiting, delayed and temporary
//!   triggers, continuous and copy effects, links, marks that name an object, attachments, blocks,
//!   attacks on a planeswalker, permissions, ... - but the payment's own task, which holds its
//!   picks); and
//! * neither has an ability whose continuous effects its timestamp orders (CR 613.7: static
//!   abilities, a Painter's colour, granted abilities, an Aura's control; `ts_sensitive`): the
//!   only place a permanent's timestamp is read besides the canonical order of a simultaneous
//!   event (where twins are interchangeable) and an event in progress (no decision interrupts).
//!
//! Twins are then swapped by a relabelling of the two objects (their slots, incarnations,
//! timestamps and battlefield places) that maps the game to itself; tapping or sacrificing one or
//! the other gives states the relabelling maps onto each other, which differ only in which slot
//! (and which battlefield place, an action id) holds the tapped one. Every exhaustive pattern
//! below (`Game`, `Obj`, `Player`, `StackItem`, `Frame`, `Task`, `Mark`, `Choice`) fails to compile
//! when a field or variant is added: a new holder of references is classified here first.
//!
//! Cards in a hand (a Spirit Guide's) keep their classes: the action space names a hand card by
//! its definition's slot (`HAND_ABILITY`, ENGINE.md 128, 156), so the payment's hand sources follow
//! every other hand choice of the engine.

use crate::db::Counter;
use crate::decision::Choice;
use crate::flow::Task;
use crate::game::{Game, Mark, Obj, Player, StackItem, StackKind};
use crate::ids::{CardIdx, DefId, ObjRef, PlayerId, Target, Zone};
use crate::num::Count;
use crate::pay::{Class, Members};
use crate::resolve::Frame;
use crate::triggers::EvCtx;

/// What the game keeps of a permanent besides its identity (the module's docs): equal for twins.
#[derive(PartialEq, Debug)]
struct Own {
    def: DefId,
    owner: PlayerId,
    controller: PlayerId,
    zone: Zone,
    tapped: bool,
    damage: u64,
    deathtouched: bool,
    /// CR 302.6, `Game::is_sick`: equal now, equal for good (both stop being sick as their
    /// controller's next turn begins; a control change is an effect naming one).
    sick: bool,
    attacking: bool,
    blocked: bool,
    blocking: Option<CardIdx>,
    struck_first: bool,
    /// What each player knows of a face-down permanent's identity (CR 708; ENGINE.md 31); 0 for
    /// one face up, whose identity is public.
    known: u8,
    attached: Option<ObjRef>,
    evoked: bool,
    flashback: bool,
    escaped: bool,
    chosen_player: Option<PlayerId>,
    monstrous: bool,
    attacks: Option<ObjRef>,
    /// `Obj::entered` as the rules read it (the turn it entered is the current one: the loyalty
    /// abilities' "if it entered this turn", the tokens created this turn): equal now, equal for
    /// good.
    entered_now: bool,
    flips: u32,
    token: bool,
    copy_obj: bool,
    face_down: Option<DefId>,
    loyalty: (u16, u32, u32),
    /// Its own records of this turn, keyed by the permanent alone (no other object): how many
    /// times its abilities resolved (`Game::resolved`: "the second time this ability has
    /// resolved this turn"), and whether it attacked under each player's control
    /// (`Player::attacked_with`).
    resolved: u32,
    attacked_with: [bool; 2],
}

/// A permanent's [`Own`] facts, its counters (sorted), its marks (a multiset) and its optional
/// actions done this turn that may be done only once each turn (`Game::done_once`, sorted).
#[derive(Debug)]
pub(crate) struct Twin {
    own: Own,
    counters: Vec<(Counter, Count)>,
    marks: Vec<Mark>,
    done_once: Vec<u8>,
}

impl Twin {
    /// Are two permanents with these facts twins (given that nothing names them)?
    fn same(&self, o: &Twin) -> bool {
        if self.own != o.own || self.counters != o.counters || self.done_once != o.done_once || self.marks.len() != o.marks.len() {
            return false;
        }
        let mut used = vec![false; o.marks.len()];
        self.marks.iter().all(|m| match (0..o.marks.len()).find(|&j| !used[j] && o.marks[j] == *m) {
            Some(j) => {
                used[j] = true;
                true
            }
            None => false,
        })
    }
}

/// The objects a state's references name (`Game::named_objects`).
struct Named<'a> {
    g: &'a Game,
    out: Vec<CardIdx>,
}

impl Named<'_> {
    /// A reference: its object if it is still there (a stale one names a gone object, CR 400.7).
    fn obj(&mut self, r: ObjRef) {
        if self.g.live(r) {
            self.out.push(r.idx);
        }
    }
    fn opt(&mut self, r: Option<ObjRef>) {
        if let Some(r) = r {
            self.obj(r);
        }
    }
    /// A slot held by index (whatever its incarnation: a superset).
    fn card(&mut self, c: CardIdx) {
        self.out.push(c);
    }
    fn cards(&mut self, cs: &[CardIdx]) {
        self.out.extend_from_slice(cs);
    }
    fn target(&mut self, t: &Target) {
        match *t {
            Target::Obj(r) => self.obj(r),
            Target::Player(_) | Target::Stack(_) => {}
        }
    }
    fn ev(&mut self, e: &EvCtx) {
        let EvCtx { player: _, obj, obj2, amount: _, item: _ } = *e;
        self.opt(obj);
        self.opt(obj2);
    }
    fn item(&mut self, s: &StackItem) {
        let StackItem {
            id: _,
            kind,
            controller: _,
            targets,
            cost_option: _,
            modes: _,
            ev,
            alt: _,
            from: _,
            delved: _,
            delve_floor: _,
            discarded_land: _,
            x: _,
            division: _,
            attack_as,
            noted,
            flips: _,
            origin: _,
            times: _,
            spliced: _,
        } = s;
        match *kind {
            StackKind::Spell { card } => self.card(card),
            StackKind::Ability { ability: _, source } => self.obj(source),
        }
        for (_, t) in targets {
            self.target(t);
        }
        self.ev(ev);
        if let Some(a) = attack_as {
            self.opt(*a);
        }
        for &r in noted {
            self.obj(r);
        }
    }
    fn frame(&mut self, f: &Frame) {
        let Frame {
            ability: _,
            pc: _,
            source,
            source_def: _,
            controller: _,
            targets,
            legal: _,
            i: _,
            lki: _,
            pile,
            floor: _,
            ev,
            x: _,
            picked,
            picked_defs: _,
            made,
            alt: _,
            division: _,
            spell_x: _,
            made_all,
            attack_as,
            noted,
            flips: _,
            page: _,
            step: _,
        } = f;
        self.obj(*source);
        for (_, t) in targets {
            self.target(t);
        }
        self.cards(pile);
        self.ev(ev);
        self.cards(picked);
        if let Some(c) = made {
            self.card(*c);
        }
        self.cards(made_all);
        if let Some(a) = attack_as {
            self.opt(*a);
        }
        for &r in noted {
            self.obj(r);
        }
    }
    fn choice(&mut self, c: &Choice) {
        match *c {
            Choice::Card(x) | Choice::Play(x) | Choice::Cast(x) | Choice::Ability(x, _) | Choice::CastFace(x) | Choice::PlayFace(x) | Choice::Faces(x, _) => self.card(x),
            Choice::Target(t) => self.target(&t),
            Choice::Pass
            | Choice::Yes
            | Choice::Number(_)
            | Choice::Trigger(_)
            | Choice::Def(_)
            | Choice::Companion
            | Choice::Page(_)
            | Choice::FromGraveyard
            | Choice::Item(_) => {}
        }
    }
    /// A task of the agenda; a payment's own (`PayMana`) is left out: its picks are the payment's
    /// own, and its other objects (the source it excludes, the permanents a cost committed, the
    /// source's own use) are out of its classes already (`payment_classes`).
    fn task(&mut self, t: &Task) {
        match t {
            Task::PayMana(_) => {}
            Task::MaySkipTurn { p: _, vault } => self.card(*vault),
            Task::SbaCheck { keep } => self.cards(keep),
            Task::MiracleReveal { p: _, card } => self.card(*card),
            Task::OrderTimestamps { first: _, settled: _, picked } => self.cards(picked),
            Task::AssignDamage { att, j: _, left: _, step: _ } => self.card(*att),
            Task::Run(f) => self.frame(f),
            Task::PayCosts { id: _, picked, early: _, order } => {
                self.cards(picked);
                if let Some(o) = order {
                    let crate::cost_order::Order { p: _, card, cost: _, x: _, loyalty: _, parts: _, done: _, cur: _, floor: _, started: _, emerge, granter } = &**o;
                    self.card(*card);
                    if let Some(c) = emerge {
                        self.card(*c);
                    }
                    if let Some(c) = granter {
                        self.card(*c);
                    }
                }
            }
            Task::ConvokePay { id: _, picked } => self.cards(picked),
            Task::ManaNow { src, step: _ } => self.card(src.card),
            Task::ChooseCreatureType { card, step: _ } => self.card(*card),
            // (The step's permanent picked for its ability; `after` and `sac`, the payment search's order marks, name no
            // object: card indices, bounds only, ENGINE.md 231.4, 236.)
            Task::CastMana { id: _, card, again: _, after: _, sac: _ } => {
                if let Some(c) = card {
                    self.card(*c);
                }
            }
            Task::ActivateMana { p: _, card, again: _ } => {
                if let Some(c) = card {
                    self.card(*c);
                }
            }
            Task::Devour { card, picked } => {
                self.card(*card);
                self.cards(picked);
            }
            Task::ManaNowCosts { src, sac, x: _, taps } => {
                self.card(src.card);
                if let Some(c) = sac {
                    self.card(*c);
                }
                self.cards(taps);
            }
            Task::AsEnters { card, page: _ } => self.card(*card),
            Task::EnterByDiscardAny { card, controller: _, tapped: _ } => self.card(*card),
            Task::EnterAura { card, controller: _, tapped: _, page: _ } => self.card(*card),
            Task::ChooseReplacement { card, from: _, results: _, void: _, chooser: _ } => self.obj(*card),
            Task::AttackTarget { card } => self.card(*card),
            Task::ChooseFace { p: _, card, mask: _ } => self.card(*card),
            Task::ChooseAbility { p: _, card, page: _ } => self.card(*card),
            Task::ColorsSpent { p: _, spell, sets: _ } => self.obj(*spell),
            Task::ChooseRoute { p: _, card, face: _ } => self.card(*card),
            Task::EnterAsCopy { card, note: _, since: _, page: _ } => self.card(*card),
            Task::Special { p: _, card, sp: _ } => self.card(*card),
            // (No object: players, stack ids - the stack is read whole -, steps, pages.)
            Task::PlayDraw { .. }
            | Task::RevealCompanion { .. }
            | Task::DrawHands
            | Task::CompanionToHand { .. }
            | Task::MulliganDeclare { .. }
            | Task::MulliganRun
            | Task::MulliganBottom { .. }
            | Task::BeginTurn
            | Task::BeginStep
            | Task::NextStep
            | Task::Priority
            | Task::PutTriggers { .. }
            | Task::DrawRest { .. }
            | Task::DrawOrder { .. }
            | Task::PriorityAsk { .. }
            | Task::DeclareAttackers { .. }
            | Task::DeclareBlockers { .. }
            | Task::CombatDamage { .. }
            | Task::CleanupDiscard
            | Task::CleanupRest
            | Task::ChooseTargets { .. }
            | Task::CastPay { .. }
            | Task::CastDone { .. }
            | Task::Resolve
            | Task::FinishResolve { .. }
            | Task::ActPay { .. }
            | Task::CastOption { .. }
            | Task::CastAlt { .. }
            | Task::Divide { .. }
            | Task::ChooseModes { .. }
            | Task::ManaNowPick { .. }
            | Task::Delve { .. }
            | Task::EnterByDiscard { .. }
            | Task::ChooseX { .. }
            | Task::ChooseForCost { .. }
            | Task::GraveyardCast { .. }
            | Task::EntersTogether
            | Task::CopyTargets { .. }
            | Task::ChooseTimes { .. }
            | Task::Splice { .. }
            | Task::Opening { .. }
            | Task::Emerge { .. } => {}
            #[cfg(test)]
            Task::Probe(_) => {}
        }
    }
}

impl Game {
    /// `classes` (a payment's: `payment_classes`) with each class split into runs of twins (the
    /// module's docs), in member order and in the class's place: each run is offered as its own
    /// option, by its first free member. A class of one member, or of hand cards, stays whole.
    pub(crate) fn split_unlike(&self, classes: Vec<Class>) -> Vec<Class> {
        let splittable = |cl: &Class| cl.members.len() > 1 && cl.members.iter().any(|s| self.objs[s.card.i()].zone == Zone::Battlefield);
        if !classes.iter().any(splittable) {
            return classes;
        }
        let named = self.named_objects();
        let mut out = Vec::with_capacity(classes.len() + 4);
        for cl in classes {
            if !splittable(&cl) {
                out.push(cl);
                continue;
            }
            let keys: Vec<Option<Twin>> = cl.members.iter().map(|s| self.twin_of(s.card, &named)).collect();
            // (Each run: the index of its first member, its members.)
            let mut runs: Vec<(usize, Members)> = Vec::new();
            for (i, &s) in cl.members.iter().enumerate() {
                let at = keys[i].as_ref().and_then(|k| runs.iter().position(|(j, _)| keys[*j].as_ref().is_some_and(|kj| kj.same(k))));
                match at {
                    Some(r) => runs[r].1.push(s),
                    None => {
                        let mut m = Members::new();
                        m.push(s);
                        runs.push((i, m));
                    }
                }
            }
            if runs.len() == 1 {
                out.push(cl);
                continue;
            }
            for (_, m) in runs {
                out.push(Class { members: m, ..cl.clone() });
            }
        }
        out
    }

    /// The facts of payment source `c` a twin must share (`Twin`), or `None` when `c` has none:
    /// something names it (`named`, sorted), or its timestamp orders effects (`ts_sensitive`). A
    /// card not on the battlefield (a hand's) has its zone and definition only (the module's
    /// docs).
    fn twin_of(&self, c: CardIdx, named: &[CardIdx]) -> Option<Twin> {
        let o = &self.objs[c.i()];
        let Obj {
            def,
            owner,
            controller,
            zone,
            inc: _,
            timestamp: _,
            tapped,
            damage,
            deathtouched,
            control_since: _,
            attacking,
            blocked,
            blocking,
            struck_first,
            known,
            // (A library card's alone, ENGINE.md 235: no payment source is one.)
            lost: _,
            attached,
            evoked,
            flashback,
            escaped,
            chosen_player,
            monstrous,
            attacks,
            entered,
            flips,
        } = *o;
        if zone != Zone::Battlefield {
            let own = Own {
                def,
                owner,
                controller,
                zone,
                tapped: false,
                damage: 0,
                deathtouched: false,
                sick: false,
                attacking: false,
                blocked: false,
                blocking: None,
                struck_first: false,
                known: 0,
                attached: None,
                evoked: false,
                flashback: false,
                escaped: false,
                chosen_player: None,
                monstrous: false,
                attacks: None,
                entered_now: false,
                flips: 0,
                token: false,
                copy_obj: false,
                face_down: None,
                loyalty: (0, 0, 0),
                resolved: 0,
                attacked_with: [false; 2],
            };
            return Some(Twin { own, counters: Vec::new(), marks: Vec::new(), done_once: Vec::new() });
        }
        if named.binary_search(&c).is_ok() || self.ts_sensitive(c) {
            return None;
        }
        let face_down = self.face_down_ids.iter().find(|x| x.0 == c).map(|x| x.1);
        let l = self.loyalty_of(c);
        let r = self.obj_ref(c);
        let own = Own {
            def,
            owner,
            controller,
            zone,
            tapped,
            damage,
            deathtouched,
            sick: self.is_sick(c),
            attacking,
            blocked,
            blocking,
            struck_first,
            known: if face_down.is_some() { known } else { 0 },
            attached,
            evoked,
            flashback,
            escaped,
            chosen_player,
            monstrous,
            attacks,
            entered_now: entered as u32 == self.turn.number,
            flips,
            token: self.is_token(c),
            copy_obj: self.is_copy_obj(c),
            face_down,
            loyalty: (l.turn, l.used, l.extra),
            resolved: self.resolved.iter().filter(|x| x.0 == r).map(|x| x.1).sum(),
            attacked_with: [0, 1].map(|p: usize| self.players[p].attacked_with.contains(&r)),
        };
        let mut counters: Vec<(Counter, Count)> = self.counters.iter().filter(|x| x.0 == c).map(|x| (x.1, x.2)).collect();
        counters.sort_unstable();
        let marks: Vec<Mark> = self.marks.iter().filter(|x| x.0 == r).map(|x| x.1).collect();
        let mut done_once: Vec<u8> = self.done_once.iter().filter(|x| x.0 == r).map(|x| x.1).collect();
        done_once.sort_unstable();
        Some(Twin { own, counters, marks, done_once })
    }

    /// Does permanent `c` have an ability whose continuous effects its timestamp orders (CR
    /// 613.7a: a static ability's effects have its source's timestamp; a Painter's colour, a
    /// granted activated ability or ward, an Equipment's grant, an Aura's control: ENGINE.md 125,
    /// `Game::ts_members`)? Its timestamp is then observable (another effect between two twins'
    /// timestamps applies before one's and after the other's).
    fn ts_sensitive(&self, c: CardIdx) -> bool {
        let d = self.abilities(c);
        !d.statics.is_empty() || d.paints || d.grants_equipped.is_some() || d.grants_ward.is_some() || !d.w3.grants_activated.is_empty() || d.w6.aura_control
    }

    /// Every object a reference of the state names, sorted (`split_unlike`): every holder of an
    /// object reference the game keeps, but the pending decision and its answer and the payment's
    /// own task (`Named::task`). The exhaustive pattern classifies every field of `Game`.
    pub(crate) fn named_objects(&self) -> Vec<CardIdx> {
        let Game {
            // (No object reference: the database, the spec, the generator, numbers and flags.)
            db: _,
            spec: _,
            rng: _,
            // (Each object's own facts: `twin_of`; the other objects' references below.)
            objs,
            players,
            // (Zones: where objects are, no reference to one.)
            battlefield: _,
            exile: _,
            face_down: _,
            // (A face-down permanent's identity: `twin_of`.)
            face_down_ids: _,
            entering_face_down: _,
            moving_face_down: _,
            command: _,
            stack,
            turn: _,
            agenda,
            // (The payment's own decision and its answer: `split_unlike` must give the payment's
            // classes alike at each of its steps, ENGINE.md 18.)
            pending: _,
            answer: _,
            page: _,
            waiting,
            extra_turns: _,
            combat_dmg,
            next_ts: _,
            next_stack_id: _,
            outcome: _,
            // (What each player knows of hidden zones' cards, by identity: twins are public, on
            // the battlefield, of one definition; ENGINE.md 229.2.)
            clouds: _,
            sightings: _,
            sight_at: _,
            found: _,
            finds_told: _,
            sightings_lost: _,
            ff: _,
            tokens: _,
            first_token: _,
            batching: _,
            releasing,
            gy_arrivals,
            ev_seq: _,
            batch_fired,
            entering_cast: _,
            entering_evoked: _,
            observer: _,
            keys: _,
            hand_looks: _,
            forced: _,
            forced_at: _,
            decisions: _,
            no_auto: _,
            oracle: _,
            oracle_dead: _,
            effects,
            any_loss: _,
            // (Each object's own: `twin_of`.)
            counters: _,
            n_static: _,
            n_self_types: _,
            leaving,
            links,
            delayed,
            // (Each object's own: `twin_of`.)
            resolved: _,
            permissions,
            combat_damage_now: _,
            quiet_leave: _,
            monarch: _,
            lki_counters,
            entering_escaped: _,
            entering_x: _,
            entering_sunburst: _,
            entering_exiled: _,
            entering_mods: _,
            leaving_to: _,
            leaving_choice,
            leaving_card,
            resolving_adventure: _,
            ts_group: _,
            held,
            entering_counters: _,
            entering_tapped: _,
            discard_entered: _,
            repl_decided: _,
            route_choice,
            entering_dash: _,
            // (A spell's locked total cost by stack id; the search's mana of no chosen type: no object, ENGINE.md 231.)
            cast_lock: _,
            pool_wild: _,
            entering_warp: _,
            miracle_cast,
            entering_face: _,
            day: _,
            // (Each object's own: `twin_of`.)
            done_once: _,
            discarded,
            void_turn: _,
            temp_triggers,
            gy_exile,
            copies,
            next_turn_controller: _,
            turn_controller: _,
            // (Each object's own: `twin_of`.)
            copy_objs: _,
            lki_copy,
            lki_attached,
            lki_damage,
            lki_toughness,
            ref_lki,
            lki_power,
            // (Each object's own: `twin_of`.)
            loyalty: _,
            pool_restr: _,
            lands_entered: _,
            moon_game: _,
            spell_lki,
            entering_copy: _,
            entering_times: _,
            enter_note: _,
            defer_enters: _,
            entries,
            pile_moving: _,
            pile_shuffles: _,
            prepared,
            // (Action ids and hashes of a loop's record.)
            loop_taken: _,
            loop_open: _,
            seen: _,
            initiative: _,
            dungeons,
            rules_srcs,
            choosing_for: _,
            marks,
            w3,
            w6,
            known_pools: _,
            log: _,
            peeks: _,
            log_mark: _,
        } = self;
        let mut n = Named { g: self, out: Vec::new() };
        // Other objects' references: an attachment, a block, an attack on a planeswalker.
        for o in objs.iter().filter(|o| o.zone == Zone::Battlefield) {
            n.opt(o.attached);
            if let Some(b) = o.blocking {
                n.card(b);
            }
            n.opt(o.attacks);
        }
        for pl in players {
            let Player {
                life: _,
                library: _,
                hand: _,
                graveyard: _,
                pool: _,
                lands_played: _,
                drew_from_empty: _,
                lost: _,
                mulligans: _,
                kept: _,
                mulligan_declared: _,
                last_turn: _,
                turns: _,
                left_battlefield: _,
                drawn: _,
                spells_cast: _,
                noncreature_cast: _,
                gained_life: _,
                artifact_entered: _,
                life_gained: _,
                cant_gain_life: _,
                // (Each object's own: `twin_of`.)
                attacked_with: _,
                draw_step_draws: _,
                spells_game: _,
                energy: _,
                lost_life: _,
                flash_sorceries: _,
                poison: _,
                experience: _,
                gy_play_turn: _,
                gy_exile_turn: _,
                city_blessing: _,
                prot_until: _,
                sideboard: _,
                companion,
                companion_def: _,
                companion_used: _,
                uncounterable_next: _,
            } = pl;
            // (The companion slot names its card only while it is the public card in no zone;
            // once in the game the slot is a card like any other, CR 702.139a, 116.2g.)
            if let Some(c) = companion.filter(|c| self.objs[c.i()].zone == Zone::Nowhere) {
                n.card(c);
            }
        }
        for s in stack {
            n.item(s);
        }
        for t in agenda {
            n.task(t);
        }
        for w in waiting {
            n.obj(w.source);
            n.ev(&w.ev);
        }
        for &(c, t, _) in combat_dmg {
            n.card(c);
            n.target(&t);
        }
        for &r in releasing {
            n.obj(r);
        }
        // (A simultaneous move's graveyard arrivals, matched once it is over: ENGINE.md 220.3.)
        for &crate::triggers::Event { kind: _, a, b, amount: _ } in gy_arrivals {
            for s in [a, b] {
                if let crate::triggers::Subj::Obj(r) = s {
                    n.obj(r);
                }
            }
        }
        for &(r, _, _) in batch_fired {
            n.obj(r);
        }
        for e in effects {
            if let crate::layers::Affected::Obj(r) = e.on {
                n.obj(r);
            }
            if let crate::layers::Until::WhileOnBattlefield(r) = e.until {
                n.obj(r);
            }
        }
        for l in leaving {
            n.obj(l.old);
        }
        for l in links {
            n.obj(l.source);
            n.obj(l.card);
        }
        for d in delayed {
            n.obj(d.source);
            n.ev(&d.ev);
        }
        for pm in permissions {
            n.obj(pm.card);
        }
        for &(r, _, _) in lki_counters {
            n.obj(r);
        }
        // (The card whose leave event waits for a replacement choice: only while one does,
        // `leaving_choice`; the field keeps its last card - or card 0 - otherwise.)
        if *leaving_choice != 0 {
            n.card(*leaving_card);
        }
        for h in held {
            n.card(h.card);
            n.obj(h.f.t.source);
            n.ev(&h.f.t.ev);
            n.obj(h.f.key.0);
            if let Some((a, _, _, b)) = h.f.times {
                n.obj(a);
                n.obj(b);
            }
        }
        if let Some(r) = route_choice {
            if let Some(c) = r.granter {
                n.card(c);
            }
        }
        if let Some(c) = miracle_cast {
            n.card(*c);
        }
        for &r in discarded {
            n.obj(r);
        }
        for t in temp_triggers {
            n.obj(t.source);
            n.opt(t.obj);
        }
        n.cards(gy_exile);
        for c in copies {
            n.obj(c.obj);
        }
        for (r, _, _) in lki_copy {
            n.obj(*r);
        }
        for &(a, b) in lki_attached {
            n.obj(a);
            n.obj(b);
        }
        for (r, _) in lki_damage {
            n.obj(*r);
        }
        for &(r, _, _) in lki_toughness {
            n.obj(r);
        }
        for &(r, _, _) in ref_lki {
            n.obj(r);
        }
        for &(c, inc, _) in lki_power {
            n.obj(ObjRef { idx: c, inc });
        }
        for (r, _, s, _) in spell_lki {
            n.obj(*r);
            n.item(s);
        }
        for e in entries {
            for &(r, _) in &e.held {
                n.obj(r);
            }
        }
        for &(r, c) in prepared {
            n.obj(r);
            n.card(c);
        }
        for d in dungeons {
            if let Some(c) = d.obj {
                n.card(c);
            }
        }
        n.cards(rules_srcs);
        // The marks that name another object (a mark on an object is that object's own:
        // `twin_of`).
        for &(_, m) in marks {
            match m {
                Mark::Reanimated(x) | Mark::ExiledToPay(x) => n.obj(x),
                Mark::Triggered { src, .. } => n.obj(src),
                Mark::AltGranted { granter, .. } => n.obj(granter),
                Mark::Entered { .. }
                | Mark::PriestPending
                | Mark::Exhausted(_)
                | Mark::ConvergePending(_)
                | Mark::Converge(_)
                | Mark::ChosenColor(_)
                | Mark::ChosenCreatureType(_)
                | Mark::Uncounterable
                | Mark::LeaveTrigger { .. }
                | Mark::GrantOnEnter { .. }
                | Mark::UsedTurn(_)
                | Mark::MvCap(_)
                | Mark::Harnessed
                | Mark::Boasted(_)
                | Mark::KickerX(_)
                | Mark::Saddled(_)
                | Mark::ModesUsed(_)
                | Mark::Parity(_)
                | Mark::NotSorceryTiming
                | Mark::LookedHand
                | Mark::ChosenName(_)
                | Mark::Exerted
                | Mark::HasteMana
                | Mark::DelveLate
                | Mark::DelveExile
                | Mark::DelvePromised
                | Mark::DealtDamage(_)
                | Mark::Granted { .. }
                | Mark::WarpCast
                | Mark::RegenShield
                | Mark::ManaSpent(_)
                | Mark::FreeFromHand => {}
            }
        }
        let crate::eoe_w3::W3State { no_regen: _, granted: _, seen: _, lki_host, entering_spent: _, lki_loss, phased, indirect } = w3;
        for &(a, b) in lki_host {
            n.obj(a);
            n.obj(b);
        }
        for &(r, _) in lki_loss {
            n.obj(r);
        }
        n.cards(phased);
        n.cards(indirect);
        let crate::eoe_w6::W6State {
            seen: _,
            entering,
            actor: _,
            control,
            syncing: _,
            tokens_turn: _,
            token_question: _,
            token_asking: _,
            token_answer,
            deferred_since,
            token_frame: _,
        } = w6;
        if let Some((c, _)) = entering {
            n.card(*c);
        }
        for cf in control {
            n.obj(cf.obj);
        }
        if let Some(ch) = token_answer {
            n.choice(ch);
        }
        for &(c, _) in deferred_since {
            n.card(c);
        }
        let mut out = n.out;
        out.sort_unstable();
        out.dedup();
        out
    }
}
