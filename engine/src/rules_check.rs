//! Rules invariants (ENGINE.md 131): facts the Comprehensive Rules require
//! of every reachable state, checked by the fuzz loops after every step.
//!
//! The fuzzer's other checks (`testkit::check_invariants`, the information
//! sets, the option completion, ...) are about the engine's own consistency:
//! a rule implemented wrongly but consistently passes them. These checks are
//! written from the rules (cited by number, the CR of `CR_EDITION`), reading
//! the state only through characteristics (the layer system's answers:
//! types, power and toughness, keywords, counters, control) and the engine's
//! records of rules facts (the damage marked on a permanent, which attacker
//! a creature blocks, the stack's objects). They never decide how the rules
//! apply from the engine's own procedures (`sba_needed`, `can_block`, ...):
//! each check states the rule itself.
//!
//! Exactness (the owner's rule: no approximations). Every check holds for
//! every state the rules allow, with every exception they make; a check
//! whose exception could not be decided exactly from the state is not here
//! (ENGINE.md 131.3 lists them and why). Checks that compare two states seen
//! one step apart apply only where the rules fix the outcome between them:
//! when no spell or ability resolved in between or was resolving at either
//! state (so no effect was created), a property the rules let only effects
//! change must be unchanged. That narrows *when* such a check applies, never
//! *what* it accepts.
//!
//! Zero cost in release: nothing in the engine calls this module; it only
//! reads a `Game`. `RulesCheck::new` at a game's start, `RulesCheck::step`
//! after every `apply` (`MatchCheck` for a best-of-three's games, `check_state`
//! for a lone state, `DraftCheck` for a team draft event's steps).

use crate::db::{supertypes, types, Counter, Kw};
use crate::decision::{Choice, DecisionKind};
use crate::flow::Task;
use crate::game::{Game, StackKind, Step, Turn};
use crate::ids::{opp, CardIdx, DefId, ObjRef, PlayerId, Target, Zone};

/// The Comprehensive Rules the citations follow (magic.wizards.com/en/rules).
pub const CR_EDITION: &str = "2026-11-06";

/// A violated rule: its number and what was seen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub rule: &'static str,
    pub what: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rules check: CR {} violated: {}", self.rule, self.what)
    }
}

pub type Checked = Result<(), Violation>;

fn fail(rule: &'static str, what: String) -> Checked {
    Err(Violation { rule, what })
}

/// How often each kind of check ran (the fuzz prints them: a check that
/// never ran is a check that tested nothing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// States checked (`step` / `check_state` calls).
    pub states: u64,
    /// States where a player was about to receive priority (704, 117.5).
    pub priority: u64,
    /// Steps whose stack was compared with the one before (405, 608.1).
    pub stack_steps: u64,
    /// Steps where a stack object that stayed was compared (choices fixed).
    pub stack_items: u64,
    /// Turn changes whose player was checked (500.7, 102.1).
    pub new_turns: u64,
    /// Declare-attackers decisions whose options were checked (508.1a).
    pub attack_options: u64,
    /// Declare-blockers decisions whose options were checked (509.1a-b).
    pub block_options: u64,
    /// Damage-assignment decisions whose options were checked (510.1c, 702.19b).
    pub assign_options: u64,
    /// ... of them too large for number options (digits: not compared).
    pub assign_digits: u64,
    /// Attack declarations checked as made (508.1).
    pub attacks: u64,
    /// Block declarations checked as made (509.1).
    pub blocks: u64,
    /// Steps begun with nothing cast since: mana pools checked empty (500.5).
    pub pools: u64,
    /// Token moves checked (111.8).
    pub token_moves: u64,
}

impl Stats {
    pub fn add(&mut self, o: &Stats) {
        self.states += o.states;
        self.priority += o.priority;
        self.stack_steps += o.stack_steps;
        self.stack_items += o.stack_items;
        self.new_turns += o.new_turns;
        self.attack_options += o.attack_options;
        self.block_options += o.block_options;
        self.assign_options += o.assign_options;
        self.assign_digits += o.assign_digits;
        self.attacks += o.attacks;
        self.blocks += o.blocks;
        self.pools += o.pools;
        self.token_moves += o.token_moves;
    }
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} states, {} at priority (704/117.5), stack {} steps / {} objects compared (405.2, 608.1, 601.2), {} new turns (500.7), options: {} attack (508.1a), {} block (509.1), {} damage assignments (510.1c, 702.19b; {} in digits not compared), declarations: {} attacks (508.1), {} blocks (509.1), {} steps begun with empty pools (500.5), {} token moves (111.8)",
            self.states,
            self.priority,
            self.stack_steps,
            self.stack_items,
            self.new_turns,
            self.attack_options,
            self.block_options,
            self.assign_options,
            self.assign_digits,
            self.attacks,
            self.blocks,
            self.pools,
            self.token_moves
        )
    }
}

// ---- helpers --------------------------------------------------------------

/// The printed card a slot is (CR 108.1; one card whatever face is up,
/// 712.1, 709.1, 715.1): its own definition without copy effects (a copy
/// effect changes copiable values, not the card, 707.2), a face-down
/// permanent's card (708.2), its front face.
fn card_of(g: &Game, c: CardIdx) -> DefId {
    let own = g.own_def(c);
    let d = if own == g.db.face_down_def() { g.face_down_ids.iter().find(|x| x.0 == c).map_or(own, |x| x.1) } else { own };
    g.db.def(d).card_def(d)
}

/// A slot that is a card (not a token, a copy, an emblem or a rules object
/// such as a dungeon).
fn is_card(g: &Game, c: CardIdx) -> bool {
    let d = g.db.def(g.objs[c.i()].def);
    !g.is_token(c) && !d.rules && !d.emblem
}

/// `p`'s revealed companion still outside the game (CR 702.139a, 400.11a).
fn companion_outside(g: &Game, p: PlayerId, c: CardIdx) -> bool {
    let pl = &g.players[p as usize];
    pl.companion == Some(c) && !pl.companion_used
}

/// Every card `p` owns, in the game or outside it (the sideboard, CR 100.4,
/// 400.11a), by printed card, sorted.
fn cards_of(g: &Game, p: PlayerId) -> Vec<DefId> {
    let mut v: Vec<DefId> = (0..g.objs.len())
        .map(|i| CardIdx::new(i as u32))
        .filter(|&c| g.objs[c.i()].owner == p && is_card(g, c) && !companion_outside(g, p, c))
        .map(|c| card_of(g, c))
        .collect();
    let pl = &g.players[p as usize];
    let mut side: Vec<DefId> = pl.sideboard.iter().copied().collect();
    // (The companion put into the hand is in the game now, 702.139c: the
    // sideboard list keeps its definition.)
    if pl.companion_used {
        if let Some(d) = pl.companion_def {
            if let Some(i) = side.iter().position(|&x| x == d) {
                side.remove(i);
            }
        }
    }
    v.extend(side);
    v.sort_unstable();
    v
}

/// The players' companions, whether each was put into the hand, and their
/// sideboards (which list).
fn companions(g: &Game) -> [(Option<CardIdx>, bool, usize); 2] {
    [0, 1].map(|p| (g.players[p].companion, g.players[p].companion_used, std::sync::Arc::as_ptr(&g.players[p].sideboard) as *const DefId as usize))
}

/// The order of steps in a turn (CR 500.1, 501-514).
fn step_ord(s: Step) -> u8 {
    match s {
        Step::Setup => 0,
        Step::Untap => 1,
        Step::Upkeep => 2,
        Step::Draw => 3,
        Step::Main1 => 4,
        Step::BeginCombat => 5,
        Step::DeclareAttackers => 6,
        Step::DeclareBlockers => 7,
        Step::FirstStrikeDamage => 8,
        Step::CombatDamage => 9,
        Step::EndCombat => 10,
        Step::Main2 => 11,
        Step::End => 12,
        Step::Cleanup => 13,
    }
}

fn is_combat_step(s: Step) -> bool {
    matches!(s, Step::BeginCombat | Step::DeclareAttackers | Step::DeclareBlockers | Step::FirstStrikeDamage | Step::CombatDamage | Step::EndCombat)
}

/// The stack object a task belongs to, while it is being cast, activated,
/// put on the stack (its targets, 603.3d) or given new targets (115.7).
fn task_item(t: &Task) -> Option<u32> {
    match t {
        Task::ChooseTargets { id, .. }
        | Task::CastPay { id }
        | Task::CastDone { id }
        | Task::FinishResolve { id }
        | Task::ActPay { id }
        | Task::PayCosts { id, .. }
        | Task::CastOption { id }
        | Task::CastAlt { id }
        | Task::Divide { id, .. }
        | Task::ChooseModes { id }
        | Task::Delve { id }
        | Task::EnterByDiscard { id }
        | Task::ChooseX { id, .. }
        | Task::ChooseForCost { id, .. }
        | Task::CopyTargets { id, .. }
        | Task::ChooseTimes { id, .. }
        | Task::Splice { id }
        | Task::Emerge { id, .. } => Some(*id),
        _ => None,
    }
}

fn subtype(g: &Game, name: &str) -> Option<u16> {
    g.db.subtype_id(name)
}

fn has_sub(g: &Game, c: CardIdx, s: Option<u16>) -> bool {
    s.is_some_and(|s| g.has_subtype(c, s))
}

fn describe(g: &Game, c: CardIdx) -> String {
    format!("{}#{}", g.name(c), c.get())
}

/// The object a stack item is about (for messages).
fn item_name(g: &Game, k: &StackKind) -> String {
    match *k {
        StackKind::Spell { card } => describe(g, card),
        StackKind::Ability { ability, source } => format!("an ability of {} ({}#{})", g.db.def(ability.def).name, g.db.def(g.objs[source.idx.i()].def).name, source.idx.get()),
    }
}

// ---- one state ------------------------------------------------------------

/// The checks that need one state only: the state-based actions (704.5) and
/// triggered abilities (117.5, 603.3) when a player is about to receive
/// priority, the tokens and copies (704.5d-e), the step (502.4), the
/// options of combat decisions (508.1a, 509.1a-b, 510.1c, 702.19b).
pub fn check_state(g: &Game) -> Checked {
    check_state_stats(g, &mut Stats::default())
}

fn check_state_stats(g: &Game, st: &mut Stats) -> Checked {
    st.states += 1;
    // CR 104.2a, 104.3: a player who lost has left the game: with two
    // players the game is over.
    if g.outcome.is_none() {
        if let Some(p) = (0..2).find(|&p| g.players[p].lost) {
            return fail("104.3", format!("P{p} lost but the game goes on"));
        }
    }
    let Some(d) = g.pending.as_ref() else { return Ok(()) };
    match d.kind {
        DecisionKind::Priority => {
            st.priority += 1;
            // CR 502.4: no player receives priority during the untap step
            // (nor before the first turn: mulligans are no step).
            if matches!(g.turn.step, Step::Untap | Step::Setup) {
                return fail("502.4", format!("P{} has priority in the {:?} step", d.player, g.turn.step));
            }
            priority_now(g)?;
        }
        DecisionKind::DeclareAttackers => attack_options(g, st)?,
        DecisionKind::DeclareBlockers => block_options(g, st)?,
        DecisionKind::AssignDamage => assign_options(g, st)?,
        _ => {}
    }
    Ok(())
}

/// A player is about to receive priority: the game has performed every
/// state-based action that applies, repeatedly, and put every triggered
/// ability that triggered on the stack (CR 117.5, 704.3, 603.3).
fn priority_now(g: &Game) -> Checked {
    if let Some(w) = g.waiting.first() {
        return fail("117.5, 603.3", format!("a triggered ability of {} waits while P{} has priority", g.db.def(w.source_def).name, g.turn.priority));
    }
    // 704.5a-c: losing the game. (No effect of the engine's cards says a
    // player can't lose the game or can't lose for having 0 life, 104.3b:
    // there is no such exception to model.)
    for (p, pl) in g.players.iter().enumerate() {
        if pl.life <= 0 {
            return fail("704.5a", format!("P{p} has {} life", pl.life));
        }
        if pl.drew_from_empty {
            return fail("704.5b", format!("P{p} drew from an empty library"));
        }
        if pl.poison >= 10 {
            return fail("704.5c", format!("P{p} has {} poison counters", pl.poison));
        }
    }
    tokens_now(g)?;
    let aura = subtype(g, "Aura");
    let equipment = g.db.equipment_subtype();
    let fortification = subtype(g, "Fortification");
    let saga = subtype(g, "Saga");
    let role = subtype(g, "Role");
    let mut worlds: Vec<CardIdx> = Vec::new();
    for (i, &c) in g.battlefield.iter().enumerate() {
        let o = &g.objs[c.i()];
        let ty = g.types_of(c);
        if ty & types::CREATURE != 0 {
            let t = g.toughness(c);
            // 704.5f: toughness 0 or less (regeneration and indestructible
            // don't apply: not destruction).
            if t <= 0 {
                return fail("704.5f", format!("{} has toughness {t}", describe(g, c)));
            }
            let indestructible = g.has_kw(c, Kw::Indestructible);
            // 704.5g: lethal damage (702.12b: not if indestructible; a
            // regeneration shield removes the damage, 701.19a).
            if o.damage > 0 && o.damage as crate::num::Wide >= t && !indestructible {
                return fail("704.5g", format!("{} has {} damage, toughness {t}", describe(g, c), o.damage));
            }
            // 704.5h, 702.2b: dealt damage by a deathtouch source since the
            // last check (damage from a source with infect too, which isn't
            // marked: 702.90c).
            if o.deathtouched && !indestructible {
                return fail("704.5h", format!("{} was dealt damage by a source with deathtouch (toughness {t}, {} damage marked) and wasn't destroyed", describe(g, c), o.damage));
            }
        }
        // 704.5h: "since the last time state-based actions were checked":
        // the game just checked them, so no permanent has been dealt
        // deathtouch damage since (the engine's record, `Obj::deathtouched`,
        // must be clear, or a later check reads an old event).
        if o.deathtouched {
            return fail("704.5h", format!("{} still counts as dealt deathtouch damage since the last state-based check, which just happened", describe(g, c)));
        }
        // 704.5i, 306.9: a planeswalker with loyalty 0 (its loyalty is its
        // loyalty counters, 306.5b).
        if ty & types::PLANESWALKER != 0 && g.counters_on(c, Counter::Loyalty) == 0 {
            return fail("704.5i", format!("{} has loyalty 0", describe(g, c)));
        }
        // 704.5v-y: battles. The engine has no defense or protector (no
        // battle card is supported): a battle here is a state this check
        // can't decide.
        if ty & types::BATTLE != 0 {
            return fail("704.5v-y", format!("{} is a battle: defense and protectors are not modelled, extend the rules check", describe(g, c)));
        }
        // 704.5j: the legend rule (201.2a: an object with no name shares a
        // name with nothing).
        if g.is_legendary(c) {
            let name = g.name(c);
            if !name.is_empty() {
                if let Some(&d) = g.battlefield[i + 1..]
                    .iter()
                    .find(|&&d| g.objs[d.i()].controller == o.controller && g.is_legendary(d) && g.name(d) == name)
                {
                    return fail("704.5j", format!("P{} controls two legendary permanents named {name}: {} and {}", o.controller, describe(g, c), describe(g, d)));
                }
            }
        }
        // 704.5k: the world rule.
        if g.supertypes_of(c) & supertypes::WORLD != 0 {
            worlds.push(c);
        }
        // 704.5q: +1/+1 and -1/-1 counters annihilate.
        if g.counters_on(c, Counter::P1P1) > 0 && g.counters_on(c, Counter::M1M1) > 0 {
            return fail("704.5q", format!("{} has both +1/+1 and -1/-1 counters", describe(g, c)));
        }
        // 704.5s: a Saga with lore counters at least its final chapter
        // number (715.2d: 0 without chapter abilities) that isn't the source
        // of a chapter ability that triggered and hasn't left the stack.
        if has_sub(g, c, saga) {
            let last = g.abilities(c).chapters as crate::num::Count;
            if last > 0 && g.counters_on(c, Counter::Lore) >= last && !chapter_pending(g, c) {
                return fail("704.5s", format!("{} has {} lore counters, final chapter {last}, and no chapter ability pending", describe(g, c), g.counters_on(c, Counter::Lore)));
            }
        }
        attachment_now(g, c, aura, equipment, fortification)?;
        // 704.5z: Roles (none in the engine's cards: checked as written if
        // one ever is).
        if role.is_some() && has_sub(g, c, role) {
            if let Some(h) = o.attached {
                if let Some(&d) = g.battlefield[i + 1..]
                    .iter()
                    .find(|&&d| has_sub(g, d, role) && g.objs[d.i()].attached == Some(h) && g.objs[d.i()].controller == o.controller)
                {
                    return fail("704.5z", format!("two Roles of P{} on one permanent: {} and {}", o.controller, describe(g, c), describe(g, d)));
                }
            }
        }
    }
    if worlds.len() > 1 {
        return fail("704.5k", format!("{} permanents with the world supertype", worlds.len()));
    }
    // 704.5t, 309.6: a venture marker on a dungeon's bottommost room, no room
    // ability of it waiting or on the stack.
    for (p, d) in g.dungeons.iter().enumerate() {
        if let Some(c) = d.obj {
            if d.room == crate::dungeon::BOTTOM {
                let r = g.obj_ref(c);
                let pending = g.waiting.iter().any(|w| w.source == r)
                    || g.stack.iter().any(|s| matches!(s.kind, StackKind::Ability { source, .. } if source == r));
                if !pending {
                    return fail("704.5t", format!("P{p}'s venture marker is on the bottommost room with no room ability pending"));
                }
            }
        }
    }
    Ok(())
}

/// A chapter ability of Saga `c` triggered and hasn't left the stack.
fn chapter_pending(g: &Game, c: CardIdx) -> bool {
    let r = g.obj_ref(c);
    let chapter = |def: DefId, slot: u8| g.db.def(def).triggers.get(slot as usize).is_some_and(|t| t.event == crate::db::EventKind::Chapter);
    g.waiting.iter().any(|w| w.source == r && chapter(w.source_def, w.slot))
        || g.stack.iter().any(|s| match s.kind {
            StackKind::Ability { ability, source } => source == r && matches!(ability.slot, crate::db::AbilitySlot::Trigger(i) if chapter(ability.def, i)),
            StackKind::Spell { .. } => false,
        })
}

/// 704.5m, 704.5n, 704.5p (303.4c-d, 301.5c, 702.16c-d, 702.103f): what
/// permanent `c` is attached to.
fn attachment_now(g: &Game, c: CardIdx, aura: Option<u16>, equipment: Option<u16>, fortification: Option<u16>) -> Checked {
    let o = &g.objs[c.i()];
    let creature = g.is_creature(c);
    let battle = g.is_type(c, types::BATTLE);
    if has_sub(g, c, aura) {
        let Some(h) = o.attached else {
            // (A bestowed Aura that becomes unattached stops being bestowed,
            // 702.103f: it is then no Aura.)
            return fail("704.5m", format!("Aura {} is attached to nothing", describe(g, c)));
        };
        if creature {
            return fail("303.4d, 704.5m", format!("Aura {} is a creature and enchants {}", describe(g, c), describe(g, h.idx)));
        }
        if !g.live(h) {
            return fail("704.5m", format!("Aura {} enchants an object that no longer exists", describe(g, c)));
        }
        if h.idx == c {
            return fail("303.4d", format!("Aura {} enchants itself", describe(g, c)));
        }
        let legal = if g.def_of(c).reanimates {
            // "Enchant creature card in a graveyard", which becomes "enchant
            // creature put onto the battlefield with <this>" (Animate Dead,
            // Necromancy; the engine's record of that creature:
            // `Mark::Reanimated`).
            match g.objs[h.idx.i()].zone {
                Zone::Graveyard => g.def_of(h.idx).is(types::CREATURE),
                Zone::Battlefield => {
                    g.is_creature(h.idx)
                        && g.marks.contains(&(g.obj_ref(c), crate::game::Mark::Reanimated(h)))
                        && !g.protected_from(h.idx, &g.damage_source_of(c))
                }
                _ => false,
            }
        } else {
            // What its enchant ability allows (its target requirement,
            // 303.4a), on the battlefield, not protected from it (702.16c).
            g.objs[h.idx.i()].zone == Zone::Battlefield && g.aura_enchants(c, o.controller, h.idx)
        };
        if !legal {
            return fail("704.5m, 303.4c", format!("Aura {} enchants {}, which its enchant ability doesn't allow", describe(g, c), describe(g, h.idx)));
        }
        return Ok(());
    }
    let Some(h) = o.attached else { return Ok(()) };
    let equip = has_sub(g, c, equipment);
    let fort = has_sub(g, c, fortification);
    if creature || battle || !(equip || fort) {
        // 704.5p: a creature or battle attached, or a permanent that is no
        // Aura, Equipment or Fortification (301.5c: an Equipment that is a
        // creature can't equip; reconfigure, the exception, isn't a keyword
        // of the engine).
        return fail("704.5p", format!("{} is attached to {} but is a creature, a battle, or no Aura, Equipment or Fortification", describe(g, c), describe(g, h.idx)));
    }
    // 704.5n, 301.5c: attached to an illegal permanent (gone, not a creature
    // (an Equipment) or a land (a Fortification), itself, protected from it,
    // 702.16d).
    let host_ok = g.live(h)
        && g.objs[h.idx.i()].zone == Zone::Battlefield
        && h.idx != c
        && if equip { g.is_creature(h.idx) } else { g.is_type(h.idx, types::LAND) }
        && !g.protected_from(h.idx, &g.damage_source_of(c));
    if !host_ok {
        return fail("704.5n, 301.5c", format!("{} is attached to an illegal permanent {}", describe(g, c), describe(g, h.idx)));
    }
    Ok(())
}

/// 704.5d-e (111.8, 707.10a, 723.3c): at a state-based check no token is
/// outside the battlefield, no copy of a spell outside the stack, no copy
/// of a card outside the stack and the battlefield (but a prepared
/// permanent's prepare-spell copy in exile, 723.3c). (A phased-out token is
/// still on the battlefield, 702.26d.)
fn tokens_now(g: &Game) -> Checked {
    if g.tokens == 0 {
        return Ok(());
    }
    for i in (g.first_token as usize).min(g.objs.len())..g.objs.len() {
        let c = CardIdx::new(i as u32);
        if !g.is_token(c) {
            continue;
        }
        let z = g.objs[c.i()].zone;
        let ok = match z {
            Zone::Battlefield | Zone::PhasedOut | Zone::Nowhere => true,
            // A copy of a spell (707.10a); a token (no copy) can't be a spell.
            Zone::Stack => g.is_copy_obj(c),
            Zone::Exile => g.is_copy_obj(c) && g.prepared.iter().any(|&(s, x)| x == c && g.live(s) && g.objs[s.idx.i()].zone == Zone::Battlefield),
            _ => false,
        };
        if !ok {
            return fail(if g.is_copy_obj(c) { "704.5d-e, 707.10a" } else { "704.5d, 111.8" }, format!("token or copy {} is in {z:?} at a state-based check", describe(g, c)));
        }
    }
    Ok(())
}

// ---- combat decisions' options ----------------------------------------------

/// Could `c` attack, by 508.1a (untapped, no battle), 302.6 / 702.10b
/// (controlled since its controller's most recent turn began, or haste) and
/// 702.3b (defender, unless an effect lets it attack as though it didn't
/// have defender)? (`tapped`: whether it is tapped at the declaration.)
fn may_attack(g: &Game, c: CardIdx, tapped: bool) -> Result<(), String> {
    let o = &g.objs[c.i()];
    if o.zone != Zone::Battlefield {
        return Err("not on the battlefield".into());
    }
    if !g.is_creature(c) {
        return Err("not a creature".into());
    }
    if g.is_type(c, types::BATTLE) {
        return Err("a battle".into());
    }
    if tapped {
        return Err("tapped".into());
    }
    // 302.6: controlled continuously since its controller's most recent
    // turn began (control_since: the turn control began; last_turn: the
    // number of that player's most recent turn).
    let since_turn_began = o.control_since < g.players[o.controller as usize].last_turn;
    if !since_turn_began && !g.has_kw(c, Kw::Haste) {
        return Err("summoning sick and no haste".into());
    }
    if g.has_kw(c, Kw::Defender) && !g.flags_of(c).attack_despite_defender {
        return Err("defender".into());
    }
    Ok(())
}

/// Can `b` block `a` by the restrictions of 509.1b that concern the pair:
/// flying (702.9b; reach 702.17b), shadow (702.28b), landwalk (702.14c),
/// protection (702.16f), "can't be blocked", "can block only creatures with
/// flying", "can't block or be blocked by non-<subtype> creatures", "can't
/// be blocked by <filter>".
fn may_block_pair(g: &Game, b: CardIdx, a: CardIdx) -> Result<(), String> {
    let (ka, kb) = (g.keywords_of(a), g.keywords_of(b));
    if ka.has(Kw::Flying) && !kb.has(Kw::Flying) && !kb.has(Kw::Reach) {
        return Err("flying (702.9b)".into());
    }
    if ka.has(Kw::Shadow) && !kb.has(Kw::Shadow) {
        return Err("shadow (702.28b)".into());
    }
    if !ka.has(Kw::Shadow) && kb.has(Kw::Shadow) {
        return Err("a blocker with shadow (702.28b)".into());
    }
    let dp = g.objs[b.i()].controller;
    for (k, land) in [(Kw::Plainswalk, "Plains"), (Kw::Islandwalk, "Island"), (Kw::Swampwalk, "Swamp"), (Kw::Mountainwalk, "Mountain"), (Kw::Forestwalk, "Forest")] {
        if ka.has(k) {
            let s = subtype(g, land);
            if g.permanents_of(dp).any(|x| g.is_type(x, types::LAND) && has_sub(g, x, s)) {
                return Err(format!("{land}walk (702.14c)"));
            }
        }
    }
    if g.protected_from(a, &g.damage_source_of(b)) {
        return Err("protection (702.16f)".into());
    }
    if g.flags_of(a).cant_be_blocked {
        return Err("can't be blocked".into());
    }
    if g.abilities(b).blocks_only_flyers && !ka.has(Kw::Flying) {
        return Err("can block only creatures with flying".into());
    }
    if let Some(s) = g.abilities(b).blocks_only_subtype {
        if !g.has_subtype(a, s) {
            return Err("its blocker can block only that subtype".into());
        }
    }
    if let Some(s) = g.abilities(a).blocks_only_subtype {
        if !g.has_subtype(b, s) {
            return Err("can be blocked only by that subtype".into());
        }
    }
    if let Some(f) = &g.abilities(a).unblockable_by {
        let ctx = crate::cast::Ctx { controller: g.objs[a.i()].controller, source: Some(g.obj_ref(a)) };
        if g.card_matches(f, b, &ctx) {
            return Err("can't be blocked by such creatures".into());
        }
    }
    Ok(())
}

/// May `b` block at all (509.1a: an untapped creature of the defending
/// player, no battle; 509.1b: "can't block")?
fn may_block(g: &Game, b: CardIdx, tapped: bool) -> Result<(), String> {
    let o = &g.objs[b.i()];
    if o.zone != Zone::Battlefield {
        return Err("not on the battlefield".into());
    }
    if o.controller != opp(g.turn.active) {
        return Err("not the defending player's".into());
    }
    if !g.is_creature(b) {
        return Err("not a creature".into());
    }
    if g.is_type(b, types::BATTLE) {
        return Err("a battle".into());
    }
    if tapped {
        return Err("tapped".into());
    }
    if g.flags_of(b).cant_block {
        return Err("can't block".into());
    }
    Ok(())
}

/// 508.1a: every creature offered as an attacker may attack.
fn attack_options(g: &Game, st: &mut Stats) -> Checked {
    let d = g.pending.as_ref().unwrap();
    if d.player != g.turn.active {
        return fail("508.1", format!("P{} declares attackers in P{}'s turn", d.player, g.turn.active));
    }
    st.attack_options += 1;
    for o in &d.options {
        if let Choice::Card(c) = o.choice {
            if g.objs[c.i()].controller != g.turn.active {
                return fail("508.1a", format!("{} is offered as an attacker but P{} doesn't control it", describe(g, c), g.turn.active));
            }
            if g.objs[c.i()].attacking {
                return fail("508.1a", format!("{} is offered as an attacker twice", describe(g, c)));
            }
            if let Err(why) = may_attack(g, c, g.objs[c.i()].tapped) {
                return fail("508.1a, 302.6, 702.3b", format!("{} is offered as an attacker: {why}", describe(g, c)));
            }
        }
    }
    Ok(())
}

/// 509.1a-b: the creature being asked may block, and each attacker offered
/// to it may be blocked by it.
fn block_options(g: &Game, st: &mut Stats) -> Checked {
    let d = g.pending.as_ref().unwrap();
    let Some(Task::DeclareBlockers { i }) = g.agenda.last() else { return Ok(()) };
    let dp = opp(g.turn.active);
    if d.player != dp {
        return fail("509.1", format!("P{} declares blockers against P{}", d.player, g.turn.active));
    }
    // The creatures that may block, in the order the engine asks them.
    let pot = g.potential_blockers();
    for &b in &pot {
        if let Err(why) = may_block(g, b, g.objs[b.i()].tapped) {
            return fail("509.1a-b", format!("{} is asked to block: {why}", describe(g, b)));
        }
    }
    let Some(&b) = pot.get(*i as usize) else { return Ok(()) };
    st.block_options += 1;
    for o in &d.options {
        if let Choice::Card(a) = o.choice {
            if !g.objs[a.i()].attacking || g.objs[a.i()].controller != g.turn.active {
                return fail("509.1a", format!("{} is offered {} to block, which isn't attacking", describe(g, b), describe(g, a)));
            }
            if let Err(why) = may_block_pair(g, b, a) {
                return fail("509.1b", format!("{} is offered to block {}: {why}", describe(g, b), describe(g, a)));
            }
        }
    }
    Ok(())
}

/// The damage assignment's options (510.1c, 702.19b, 702.2c): the amounts
/// the rules allow the blocker being assigned to receive.
fn assign_options(g: &Game, st: &mut Stats) -> Checked {
    let d = g.pending.as_ref().unwrap();
    let Some(&Task::AssignDamage { att, j, left, step }) = g.agenda.last() else { return Ok(()) };
    if d.player != g.turn.active {
        return fail("510.1", format!("P{} assigns the attacking creature {}'s damage", d.player, describe(g, att)));
    }
    let blockers = g.blockers_of(att);
    let Some(&b) = blockers.get(j as usize) else { return Ok(()) };
    // 510.1a: the attacker assigns damage equal to its power (or toughness,
    // by an effect: the engine's amount); what it assigned so far, and what
    // is left.
    let mine = |x: CardIdx| -> u64 {
        let r = g.obj_ref(x);
        g.combat_dmg.iter().filter(|&&(s, t, _)| s == att && t == Target::Obj(r)).map(|&(_, _, n)| n).sum()
    };
    let assigned: u64 = blockers[..j as usize].iter().map(|&x| mine(x)).sum();
    let amount = g.combat_damage_amount(att);
    if amount as i128 - assigned as i128 != left as i128 {
        return fail("510.1a", format!("{} assigns {amount} damage, {assigned} assigned, but {left} is left to assign", describe(g, att)));
    }
    // Lethal damage (702.19b): damage marked and damage other creatures
    // assign to it this step count; with deathtouch any nonzero amount is
    // lethal (702.2c).
    let deathtouch = g.has_kw(att, Kw::Deathtouch);
    let lethal = |x: CardIdx| -> crate::num::Wide {
        let r = g.obj_ref(x);
        let others: u64 = g.combat_dmg.iter().filter(|&&(s, t, _)| s != att && t == Target::Obj(r)).map(|&(_, _, n)| n).sum();
        let have = g.objs[x.i()].damage as crate::num::Wide + others as crate::num::Wide;
        let t = g.toughness(x);
        if have >= t {
            0
        } else if deathtouch {
            1
        } else {
            t - have
        }
    };
    // 510.1c: the damage is divided among the blockers as its controller
    // chooses; with trample (702.19b) the rest may go to what it attacks
    // once every blocker has lethal damage (nothing attacked, 506.4c: all
    // to the blockers).
    let trample = g.has_kw(att, Kw::Trample) && g.attack_dest(att).is_some();
    let last = j as usize + 1 == blockers.len();
    let prev_lethal = blockers[..j as usize].iter().all(|&x| mine(x) as crate::num::Wide >= lethal(x));
    let (lo, hi): (u64, u64) = if !last {
        (0, left)
    } else if !trample || !prev_lethal || (left as crate::num::Wide) < lethal(b) {
        (left, left)
    } else {
        (lethal(b) as u64, left)
    };
    let numbers: Option<Vec<u64>> = d.options.iter().map(|o| if let Choice::Number(n) = o.choice { Some(n as u64) } else { None }).collect();
    let Some(ns) = numbers else {
        return fail("510.1", format!("{}'s damage assignment offers no amounts", describe(g, att)));
    };
    if step != crate::decision::NumberStep::FIRST {
        st.assign_digits += 1; // (a later digit of a big amount)
        return Ok(());
    }
    // The amounts as the engine asks them (ENGINE.md 100.2): each amount
    // itself while the largest is at most 31; else the amount less the
    // least while those fit, else "31 or more" and digits.
    let shown: Vec<u64> = if hi <= 31 {
        (lo..=hi).collect()
    } else if hi - lo <= 31 {
        (0..=hi - lo).collect()
    } else {
        st.assign_digits += 1;
        (0..=31).collect()
    };
    st.assign_options += 1;
    if ns != shown || (hi > 31 && hi - lo > 31) != matches!(d.stage, crate::decision::Stage::Paged { .. }) {
        return fail(
            "510.1c, 702.19b, 702.2c",
            format!(
                "{} ({}{}) assigns to blocker {} of {} ({}): {ns:?} offered ({:?}), the rules allow {lo}..={hi} ({left} left)",
                describe(g, att),
                if trample { "trample" } else { "no trample" },
                if deathtouch { ", deathtouch" } else { "" },
                j + 1,
                blockers.len(),
                describe(g, b),
                d.stage
            ),
        );
    }
    Ok(())
}

// ---- states one step apart ----------------------------------------------------

#[derive(Clone, Debug)]
struct ItemSnap {
    id: u32,
    kind: StackKind,
    controller: PlayerId,
    targets: Vec<(u8, Target)>,
    modes: Vec<u8>,
    x: u32,
    /// Its casting, activation or putting on the stack is over, it isn't
    /// resolving and no effect is choosing new targets for it.
    settled: bool,
    /// It is resolving (it was the top object as it began to; spells cast
    /// while it resolves are put above it, 608.2g, 707.12).
    resolving: bool,
}

#[derive(Clone, Debug)]
struct Snap {
    turn: Turn,
    extra_turns: Vec<PlayerId>,
    stack: Vec<ItemSnap>,
    next_stack_id: u32,
    /// A "you may skip that turn" choice (Time Vault) is being made.
    may_skip: bool,
    /// A spell or ability is resolving (its effect may be partly done).
    resolving_any: bool,
    /// Attackers (508.1) or blockers (509.1) are being declared.
    declaring_attackers: bool,
    declaring_blockers: bool,
    life: [i64; 2],
    cant_gain: [bool; 2],
    /// Token slots: (slot, zone, incarnation, entering: a choice made as it
    /// enters names it, 614.12, 616.1 - in no zone meanwhile, it hasn't
    /// ceased to exist).
    tokens: Vec<(CardIdx, Zone, u32, bool)>,
}

impl Snap {
    fn of(g: &Game) -> Snap {
        let busy: Vec<u32> = g.agenda.iter().filter_map(task_item).collect();
        let stack = g
            .stack
            .iter()
            .map(|s| ItemSnap {
                id: s.id,
                kind: s.kind,
                controller: s.controller,
                targets: s.targets.clone(),
                modes: s.modes.clone(),
                x: s.x,
                settled: !busy.contains(&s.id) && g.choosing_for != s.id,
                resolving: g.agenda.iter().any(|t| matches!(t, Task::FinishResolve { id } if *id == s.id)),
            })
            .collect();
        let tokens = if g.tokens == 0 {
            Vec::new()
        } else {
            ((g.first_token as usize).min(g.objs.len())..g.objs.len())
                .map(|i| CardIdx::new(i as u32))
                .filter(|&c| g.is_token(c))
                .map(|c| (c, g.objs[c.i()].zone, g.objs[c.i()].inc, entering(g, c)))
                .collect()
        };
        Snap {
            turn: g.turn,
            extra_turns: g.extra_turns.clone(),
            stack,
            next_stack_id: g.next_stack_id,
            may_skip: g.agenda.iter().any(|t| matches!(t, Task::MaySkipTurn { .. })),
            resolving_any: resolving(g),
            declaring_attackers: g.agenda.iter().any(|t| matches!(t, Task::DeclareAttackers { .. })),
            declaring_blockers: g.agenda.iter().any(|t| matches!(t, Task::DeclareBlockers { .. })),
            life: [g.players[0].life, g.players[1].life],
            cant_gain: [g.players[0].cant_gain_life, g.players[1].cant_gain_life],
            tokens,
        }
    }
}

/// Some stack object left the stack between `p` and `g` (one that was there,
/// or one put there since): a spell or ability resolved in between (an
/// object leaves the stack only by resolving or by an effect, which a
/// resolving object makes; 405.5, 608.1).
fn left_stack(p: &Snap, g: &Game) -> bool {
    let here = |id: u32| g.stack.iter().any(|s| s.id == id);
    p.stack.iter().any(|it| !here(it.id)) || !new_items_stayed(p, g)
}

/// A choice made as `c` enters (or for the replacement of its move, 614.12,
/// 616.1) is being made: it waits outside every zone meanwhile.
fn entering(g: &Game, c: CardIdx) -> bool {
    g.agenda.iter().any(|t| match t {
        Task::AsEnters { card, .. }
        | Task::ChooseCreatureType { card, .. }
        | Task::Devour { card, .. }
        | Task::EnterByDiscardAny { card, .. }
        | Task::EnterAura { card, .. }
        | Task::EnterAsCopy { card, .. } => *card == c,
        Task::ChooseReplacement { card, .. } => card.idx == c,
        _ => false,
    })
}

/// A spell or ability is resolving (CR 608.2: its effects happen in order;
/// a decision in the middle of it is a state with part of them done).
fn resolving(g: &Game) -> bool {
    g.agenda.iter().any(|t| matches!(t, Task::FinishResolve { .. } | Task::Run(_)))
}

/// An effect may have been created between `p` and `g`: a spell or ability
/// resolved (left the stack) or was resolving at either state.
fn effects_between(p: &Snap, g: &Game) -> bool {
    p.resolving_any || resolving(g) || left_stack(p, g)
}

/// Every stack object put on the stack since `p` is still there.
fn new_items_stayed(p: &Snap, g: &Game) -> bool {
    let new = g.stack.iter().filter(|s| s.id >= p.next_stack_id).count() as u64;
    new == (g.next_stack_id - p.next_stack_id) as u64
}

/// Rules invariants over a game, step by step (ENGINE.md 131). Make it at
/// the game's start (or at any state: the conservation checks compare with
/// that state), call [`step`](Self::step) after every `apply`.
#[derive(Clone, Debug)]
pub struct RulesCheck {
    /// Slots at the start, and each card slot's owner and printed card.
    base_len: usize,
    base_cards: Vec<Option<(PlayerId, DefId)>>,
    /// Every card each player owns, in the game or outside it.
    base_total: [Vec<DefId>; 2],
    prev: Option<Snap>,
    /// The player of the most recent normal turn, as this check followed the
    /// turns (500.7); `None` until known.
    normal: Option<PlayerId>,
    /// Token slots seen as permanents (on the battlefield or phased out).
    token_perm: Vec<bool>,
    /// Chance outcomes were forced in this game (`force_chance`): an unknown
    /// library card may be dealt another identity of its owner's library.
    forced: bool,
    /// The slots and the companions' state when every player's cards were
    /// last counted (`cards`: counted again only when they change).
    counted: (usize, [(Option<CardIdx>, bool, usize); 2]),
    /// A violation was found: the game's later states may follow from it,
    /// so this game is checked no further (`step` returns `Ok`).
    stopped: bool,
    pub stats: Stats,
}

impl RulesCheck {
    pub fn new(g: &Game) -> RulesCheck {
        let base_cards = (0..g.objs.len())
            .map(|i| CardIdx::new(i as u32))
            .map(|c| is_card(g, c).then(|| (g.objs[c.i()].owner, card_of(g, c))))
            .collect();
        RulesCheck {
            base_len: g.objs.len(),
            base_cards,
            base_total: [cards_of(g, 0), cards_of(g, 1)],
            prev: None,
            normal: (g.turn.number > 0).then_some(g.turn.normal),
            token_perm: Vec::new(),
            forced: !g.forced.is_empty(),
            counted: (g.objs.len(), companions(g)),
            stopped: false,
            stats: Stats::default(),
        }
    }

    /// Check the state reached by a step (every rule; the ones that compare
    /// two states compare it with the state of the previous call). After a
    /// violation the game is checked no further.
    pub fn step(&mut self, g: &Game) -> Checked {
        if self.stopped {
            return Ok(());
        }
        let r = self.step_checked(g);
        self.stopped = r.is_err();
        r
    }

    fn step_checked(&mut self, g: &Game) -> Checked {
        self.forced |= !g.forced.is_empty();
        check_state_stats(g, &mut self.stats)?;
        self.cards(g)?;
        let now = Snap::of(g);
        if let Some(p) = self.prev.take() {
            self.tokens(&p, g)?;
            self.stack(&p, g)?;
            self.turns(&p, g)?;
            self.combat(&p, &now, g)?;
            self.pools(&p, g)?;
            // 119.7: a player who can't gain life never ends with more life.
            for q in 0..2 {
                if p.cant_gain[q] && g.players[q].life > p.life[q] {
                    return fail("119.7", format!("P{q} can't gain life but went from {} to {} life", p.life[q], g.players[q].life));
                }
            }
        }
        self.mark_permanents(g);
        self.prev = Some(now);
        Ok(())
    }

    /// Card conservation (CR 108.1, 108.3, 400.11, 100.4, 702.139): a card
    /// keeps its owner and stays the card it is, whatever face is up; no card
    /// enters the game from nowhere (the only card brought in from outside
    /// it is a companion, 702.139a, revealed from the sideboard); each
    /// player's cards in the game and outside it are the ones they started
    /// with.
    fn cards(&mut self, g: &Game) -> Checked {
        for (i, b) in self.base_cards.iter().enumerate() {
            let Some((owner, card)) = *b else { continue };
            let c = CardIdx::new(i as u32);
            if g.objs[i].owner != owner {
                return fail("108.3", format!("card {} changed owner from P{owner} to P{}", describe(g, c), g.objs[i].owner));
            }
            if !is_card(g, c) {
                return fail("108.1", format!("card #{i} ({}) is no longer a card", g.db.def(card).name));
            }
            let now = card_of(g, c);
            // (A forced chance outcome deals an unknown library card the
            // identity of another one of its owner's library: the world is
            // another world, the multiset below still holds.)
            let reshuffled = self.forced && g.objs[i].zone == Zone::Library && g.objs[i].known == 0;
            if now != card && !reshuffled {
                return fail("108.1", format!("card #{i} was {} and is now {} ({:?}, known {:#b}, observer {:?})", g.db.def(card).name, g.db.def(now).name, g.objs[i].zone, g.objs[i].known, g.observer));
            }
        }
        for i in self.base_len..g.objs.len() {
            let c = CardIdx::new(i as u32);
            let owner = g.objs[i].owner;
            if is_card(g, c) && g.players[owner as usize].companion != Some(c) {
                return fail("400.11b, 108.3", format!("card #{i} ({}) entered the game from nowhere", g.name(c)));
            }
        }
        // Every player's cards, counted again when a slot was made, a
        // companion or a sideboard changed, or chance was forced (with none,
        // the checks above imply the count: the baseline's cards are
        // unchanged and there is no new card).
        let state = (g.objs.len(), companions(g));
        if !self.forced && state == self.counted {
            return Ok(());
        }
        self.counted = state;
        for p in 0..2u8 {
            let now = cards_of(g, p);
            if now != self.base_total[p as usize] {
                let names = |v: &[DefId]| v.iter().map(|&d| g.db.def(d).name.clone()).collect::<Vec<_>>().join(", ");
                let gone: Vec<DefId> = diff(&self.base_total[p as usize], &now);
                let new: Vec<DefId> = diff(&now, &self.base_total[p as usize]);
                return fail("108.3, 400.11a, 100.4", format!("P{p}'s cards changed: gone [{}], new [{}]", names(&gone), names(&new)));
            }
        }
        Ok(())
    }

    fn mark_permanents(&mut self, g: &Game) {
        if g.tokens == 0 {
            return;
        }
        if self.token_perm.len() < g.objs.len() {
            self.token_perm.resize(g.objs.len(), false);
        }
        for i in (g.first_token as usize).min(g.objs.len())..g.objs.len() {
            if matches!(g.objs[i].zone, Zone::Battlefield | Zone::PhasedOut) && g.is_token(CardIdx::new(i as u32)) {
                self.token_perm[i] = true;
            }
        }
    }

    /// 111.8: a token that has left the battlefield can't move to another
    /// zone or come back onto the battlefield; 707.10a: a copy of a spell
    /// that left the stack ceases to exist (it can't come back to it).
    fn tokens(&mut self, p: &Snap, g: &Game) -> Checked {
        for &(c, z, inc, pending) in &p.tokens {
            let o = &g.objs[c.i()];
            if o.inc == inc {
                continue;
            }
            self.stats.token_moves += 1;
            // (A token is created on the battlefield, 111.1, 111.2; a copy
            // of a permanent made as a token too; a copy of a spell or a card
            // is made on the stack or in its card's zone, 707.10, 707.12.)
            let was_perm = self.token_perm.get(c.i()).copied().unwrap_or(false) || g.db.def(g.own_def(c)).token;
            match z {
                Zone::Battlefield | Zone::PhasedOut => {
                    if matches!(o.zone, Zone::Battlefield | Zone::PhasedOut | Zone::Stack) {
                        return fail("111.8", format!("token {} left the battlefield and came back to {:?}", describe(g, c), o.zone));
                    }
                }
                Zone::Stack if !was_perm => {
                    // A copy of a spell: to the battlefield as it resolves
                    // (707.10f), otherwise it ceases to exist (707.10a).
                    if o.zone == Zone::Stack {
                        return fail("707.10a", format!("copy {} left the stack and came back", describe(g, c)));
                    }
                }
                Zone::Nowhere if !pending => {
                    if o.zone != Zone::Nowhere {
                        return fail("704.5d, 111.8", format!("token or copy {} ceased to exist and is now in {:?}", describe(g, c), o.zone));
                    }
                }
                _ if was_perm => {
                    // A token that left the battlefield: only to no zone.
                    if o.zone != Zone::Nowhere {
                        return fail("111.8", format!("token {} moved from {z:?} to {:?} after leaving the battlefield", describe(g, c), o.zone));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The stack (CR 405.2, 405.4, 405.5, 608.1, 601.2, 602.2, 603.3, 700.2,
    /// 107.3a, 115.7).
    fn stack(&mut self, p: &Snap, g: &Game) -> Checked {
        self.stats.stack_steps += 1;
        let here = |id: u32| g.stack.iter().position(|s| s.id == id);
        // 405.2: an object is put on top of all objects already there, and
        // nothing reorders the stack: the objects that stayed keep their
        // order, below every object put on it since.
        let stayed: Vec<usize> = p.stack.iter().filter_map(|it| here(it.id)).collect();
        if stayed.windows(2).any(|w| w[0] >= w[1]) {
            return fail("405.2", "the stack's objects changed order".to_string());
        }
        for (k, s) in g.stack.iter().enumerate() {
            let old = p.stack.iter().any(|it| it.id == s.id);
            if !old && k < stayed.len() {
                return fail("405.2", format!("{} was put on the stack below an object that was already there", item_name(g, &s.kind)));
            }
            if old && k >= stayed.len() {
                return fail("405.2", format!("{} is above an object put on the stack after it", item_name(g, &s.kind)));
            }
            if !old && s.id < p.next_stack_id {
                return fail("405.2", format!("{} came back to the stack", item_name(g, &s.kind)));
            }
        }
        // 608.1, 405.5: only the top object resolves. An object that left
        // while an object above it stayed didn't resolve (unless it was
        // resolving already: spells cast as it resolved are above it): an
        // effect removed it (a counterspell), which only a resolving object
        // makes: one above that one, or one put on the stack since (and gone
        // too).
        // (A resolving object is still on the stack: its effects may have
        // been done in part.)
        let came_and_went = !new_items_stayed(p, g);
        let resolving_now = |id: u32| g.agenda.iter().any(|t| matches!(t, Task::FinishResolve { id: x } if *x == id));
        let new_resolving = g.stack.iter().any(|s| s.id >= p.next_stack_id && resolving_now(s.id));
        for (i, it) in p.stack.iter().enumerate() {
            if here(it.id).is_some() || it.resolving {
                continue;
            }
            if let Some(j) = (i + 1..p.stack.len()).find(|&j| here(p.stack[j].id).is_some()) {
                let above_acted = (j + 1..p.stack.len()).any(|k| here(p.stack[k].id).is_none()) || (j..p.stack.len()).any(|k| resolving_now(p.stack[k].id));
                if !above_acted && !came_and_went && !new_resolving {
                    return fail("608.1, 405.5", format!("{} left the stack while {} above it stayed, and nothing above that resolved", item_name(g, &it.kind), item_name(g, &p.stack[j].kind)));
                }
            }
        }
        // The choices made as an object was put on the stack are fixed
        // (modes 700.2, X 107.3a, what it is), its controller is who cast,
        // activated or controlled the source (405.4), its targets change only
        // by an effect (115.7): compared when it was settled in both states.
        let resolved = effects_between(p, g);
        for it in p.stack.iter().filter(|it| it.settled) {
            let Some(k) = here(it.id) else { continue };
            let s = &g.stack[k];
            let busy = g.agenda.iter().filter_map(task_item).any(|id| id == s.id) || g.choosing_for == s.id;
            if busy {
                continue;
            }
            self.stats.stack_items += 1;
            if s.kind != it.kind {
                return fail("405.1", format!("stack object #{} was {} and is now {}", s.id, item_name(g, &it.kind), item_name(g, &s.kind)));
            }
            if s.modes != it.modes {
                return fail("700.2", format!("{}'s modes changed from {:?} to {:?}", item_name(g, &s.kind), it.modes, s.modes));
            }
            if s.x != it.x {
                return fail("107.3a", format!("{}'s X changed from {} to {}", item_name(g, &s.kind), it.x, s.x));
            }
            if !resolved {
                if s.controller != it.controller {
                    return fail("405.4", format!("{}'s controller changed from P{} to P{} with no effect", item_name(g, &s.kind), it.controller, s.controller));
                }
                if s.targets != it.targets {
                    return fail("115.7", format!("{}'s targets changed with no effect: {:?} -> {:?}", item_name(g, &s.kind), it.targets, s.targets));
                }
            }
        }
        Ok(())
    }

    /// The turn structure (CR 500.1, 102.1, 500.7, 500.8, 500.11, 514.3a).
    fn turns(&mut self, p: &Snap, g: &Game) -> Checked {
        let (a, b) = (p.turn, g.turn);
        if b.number < a.number {
            return fail("500.1", format!("turn {} after turn {}", b.number, a.number));
        }
        if b.number == a.number {
            if b.number > 0 && b.active != a.active {
                return fail("102.1", format!("turn {} changed its active player from P{} to P{}", a.number, a.active, b.active));
            }
            // Steps follow in order (500.1); an earlier step again only for
            // a combat phase an effect added (500.8: directly after the phase
            // it names; one added or still to come at `p`, or one an effect
            // between the two states may have added), then the phase that
            // came next there: the precombat main phase after a combat phase
            // added to the beginning phase. (Another cleanup step, 514.3a, is
            // the same step again.)
            let back = step_ord(b.step) < step_ord(a.step);
            if back {
                let ok = is_combat_step(b.step) && (a.extra_combats > 0 || effects_between(p, g))
                    || b.step == Step::Main1 && is_combat_step(a.step) && a.after_combat == Step::Main1;
                if !ok {
                    return fail("500.1, 500.8", format!("turn {}: the {:?} step after the {:?} step with no phase added", b.number, b.step, a.step));
                }
            }
            return Ok(());
        }
        // A new turn. The first: the starting player's (103.1, 103.8).
        if a.number == 0 {
            if b.number == 1 && b.active != b.starting {
                return fail("103.8", format!("the first turn is P{}'s, but P{} starts", b.active, b.starting));
            }
            self.normal = Some(g.turn.normal);
            return Ok(());
        }
        // The next turn (500.7): the most recently added extra turn, else
        // the player after the most recent normal turn's. Checked when no
        // effect could have added or skipped a turn in between (nothing left
        // the stack; no "skip that turn" choice, 614.10).
        if b.number == a.number + 1 && !effects_between(p, g) && !p.may_skip {
            self.stats.new_turns += 1;
            match p.extra_turns.last() {
                Some(&x) => {
                    if b.active != x {
                        return fail("500.7", format!("turn {} is P{}'s but P{x}'s extra turn was next", b.number, b.active));
                    }
                }
                None => {
                    if let Some(n) = self.normal {
                        if b.active != opp(n) {
                            return fail("500.7, 102.1", format!("turn {} is P{}'s but follows P{n}'s normal turn with no extra turn", b.number, b.active));
                        }
                    }
                    self.normal = Some(b.active);
                }
            }
        } else {
            self.normal = Some(g.turn.normal);
        }
        Ok(())
    }

    /// The declarations of attackers (508.1) and blockers (509.1), checked
    /// as made: at the first state after one, when nothing left the stack
    /// since the state before it (so no effect changed what they read).
    fn combat(&mut self, p: &Snap, now: &Snap, g: &Game) -> Checked {
        // (Nothing put on the stack since `p` left it or is resolving.)
        if g.outcome.is_some() || !new_items_stayed(p, g) || resolving(g) {
            return Ok(());
        }
        // (`p` before the declaration: in an earlier step, whose stack
        // emptied before the declaration's step began, 500.2; or during it,
        // with the stack empty.)
        let (a, b) = (p.turn, g.turn);
        let earlier = |s: Step| b.number > a.number || step_ord(a.step) < step_ord(s);
        if b.step == Step::DeclareAttackers
            && !now.declaring_attackers
            && (earlier(Step::DeclareAttackers) || p.declaring_attackers && p.stack.is_empty() && a.number == b.number)
        {
            self.stats.attacks += 1;
            attacks_made(g).map_err(|e| Violation { rule: e.rule, what: format!("{} (as declared: the state before was turn {} {:?}, {} on the stack, next stack id {} -> {})", e.what, a.number, a.step, p.stack.len(), p.next_stack_id, g.next_stack_id) })?;
        }
        if b.step == Step::DeclareBlockers && !now.declaring_blockers && (earlier(Step::DeclareBlockers) || p.declaring_blockers && p.stack.is_empty() && a.number == b.number) {
            self.stats.blocks += 1;
            blocks_made(g).map_err(|e| Violation { rule: e.rule, what: format!("{} (as declared: the state before was turn {} {:?}, {} on the stack, next stack id {} -> {})", e.what, a.number, a.step, p.stack.len(), p.next_stack_id, g.next_stack_id) })?;
        }
        Ok(())
    }

    /// 500.5: mana empties as each step and phase ends. At the first state of
    /// a step, with no object put on the stack since the state before it,
    /// nothing in this step could have added mana yet (no spell or ability
    /// resolved, no mana ability was activated: that needs a decision).
    fn pools(&mut self, p: &Snap, g: &Game) -> Checked {
        if g.outcome.is_some() || g.next_stack_id != p.next_stack_id {
            return Ok(());
        }
        let (a, b) = (p.turn, g.turn);
        if (a.number, step_ord(a.step)) == (b.number, step_ord(b.step)) || b.step == Step::Setup {
            return Ok(());
        }
        self.stats.pools += 1;
        for (q, pl) in g.players.iter().enumerate() {
            if pl.pool.total() > 0 {
                return fail("500.5", format!("P{q} has {:?} mana in the {:?} step (turn {}), left from the {:?} step", pl.pool.amt, b.step, b.number, a.step));
            }
        }
        if !g.pool_restr.is_empty() {
            return fail("500.5", "restricted mana kept past a step's end".to_string());
        }
        Ok(())
    }
}

/// The multiset difference `a` - `b` (both sorted).
fn diff(a: &[DefId], b: &[DefId]) -> Vec<DefId> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() {
        if j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < b.len() && b[j] < a[i] {
            j += 1;
        } else {
            out.push(a[i]);
            i += 1;
        }
    }
    out
}

/// The attackers as declared (508.1a-b, 508.1d, 508.1f; 302.6, 702.3b,
/// 702.10b, 702.20b).
fn attacks_made(g: &Game) -> Checked {
    let ap = g.turn.active;
    let dp = opp(ap);
    for &c in &g.battlefield {
        let o = &g.objs[c.i()];
        if !o.attacking {
            // 508.1d: a creature that attacks each combat if able attacks if
            // it can (the engine's cards have no restriction a requirement
            // could conflict with, and no attack costs).
            if o.controller == ap && g.is_creature(c) && g.flags_of(c).must_attack && may_attack(g, c, o.tapped).is_ok() {
                return fail("508.1d", format!("{} attacks each combat if able and could attack, but doesn't", describe(g, c)));
            }
            continue;
        }
        if o.controller != ap {
            return fail("508.1a", format!("{} attacks but P{ap} doesn't control it", describe(g, c)));
        }
        // 508.1f: attacking taps it unless it has vigilance (702.20b); it was
        // untapped (508.1a).
        let vigilance = g.has_kw(c, Kw::Vigilance);
        if vigilance == o.tapped {
            return fail("508.1a, 508.1f, 702.20b", format!("attacker {} is {} (vigilance: {vigilance})", describe(g, c), if o.tapped { "tapped" } else { "untapped" }));
        }
        if let Err(why) = may_attack(g, c, false) {
            return fail("508.1a, 302.6, 702.3b", format!("{} attacks: {why}", describe(g, c)));
        }
        // 508.1b: it attacks the defending player or a planeswalker that
        // player controls.
        if let Some(r) = o.attacks {
            if !(g.live(r) && g.objs[r.idx.i()].zone == Zone::Battlefield && g.objs[r.idx.i()].controller == dp && g.is_type(r.idx, types::PLANESWALKER)) {
                return fail("508.1b", format!("{} attacks {}, not the defending player or one of their planeswalkers", describe(g, c), describe(g, r.idx)));
            }
        }
        if o.blocking.is_some() {
            return fail("508.1", format!("attacker {} is also blocking", describe(g, c)));
        }
    }
    Ok(())
}

/// The blockers as declared (509.1a-b, 509.1h; 702.9b, 702.17b, 702.28b,
/// 702.14c, 702.16f, 702.111b).
fn blocks_made(g: &Game) -> Checked {
    for &b in &g.battlefield {
        let o = &g.objs[b.i()];
        let Some(a) = o.blocking else { continue };
        if let Err(why) = may_block(g, b, o.tapped) {
            return fail("509.1a-b", format!("{} blocks {}: {why}", describe(g, b), describe(g, a)));
        }
        let oa = &g.objs[a.i()];
        if oa.zone != Zone::Battlefield || !oa.attacking || oa.controller != g.turn.active {
            return fail("509.1a", format!("{} blocks {}, which isn't attacking", describe(g, b), describe(g, a)));
        }
        if let Err(why) = may_block_pair(g, b, a) {
            return fail("509.1b", format!("{} blocks {}: {why}", describe(g, b), describe(g, a)));
        }
    }
    for &a in &g.battlefield {
        if !g.objs[a.i()].attacking {
            continue;
        }
        let n = g.battlefield.iter().filter(|&&x| g.objs[x.i()].blocking == Some(a)).count();
        // 509.1b: menace (702.111b) and "can't be blocked except by n or
        // more creatures"; an Aura's "can't be blocked by more than one".
        let min = if g.has_kw(a, Kw::Menace) { 2 } else { 1 }.max(g.abilities(a).min_blockers as usize);
        if n > 0 && n < min {
            return fail("509.1b, 702.111b", format!("{} is blocked by {n} creature(s) but can't be blocked except by {min} or more", describe(g, a)));
        }
        let r = g.obj_ref(a);
        let max_one = g.battlefield.iter().any(|&x| g.objs[x.i()].attached == Some(r) && g.abilities(x).attached_max_one_blocker);
        if max_one && n > 1 {
            return fail("509.1b", format!("{} can't be blocked by more than one creature but is blocked by {n}", describe(g, a)));
        }
        // 509.1h: blocked iff a blocker was declared for it.
        if g.objs[a.i()].blocked != (n > 0) {
            return fail("509.1h", format!("{} has {n} blockers but is {}", describe(g, a), if g.objs[a.i()].blocked { "blocked" } else { "unblocked" }));
        }
    }
    Ok(())
}

/// [`RulesCheck`] over a match's games (`az::MtgAz`): a new check at each
/// game's first state; none while the players sideboard or choose decks.
#[derive(Clone, Debug, Default)]
pub struct MatchCheck {
    game: Option<(u8, RulesCheck)>,
    /// The finished games' checks.
    pub stats: Stats,
}

impl MatchCheck {
    pub fn new() -> MatchCheck {
        MatchCheck::default()
    }

    pub fn step(&mut self, a: &crate::az::MtgAz) -> Checked {
        if a.m.sb.is_some() || a.m.choice.is_some() {
            self.finish();
            return Ok(());
        }
        if self.game.as_ref().is_some_and(|(n, _)| *n != a.m.game_no) {
            self.finish();
        }
        let (_, rc) = self.game.get_or_insert_with(|| (a.m.game_no, RulesCheck::new(&a.g)));
        rc.step(&a.g)
    }

    fn finish(&mut self) {
        if let Some((_, rc)) = self.game.take() {
            self.stats.add(&rc.stats);
        }
    }

    /// Every check run so far (finished games and the current one).
    pub fn total(&self) -> Stats {
        let mut s = self.stats;
        if let Some((_, rc)) = &self.game {
            s.add(&rc.stats);
        }
        s
    }
}

// ---- team draft events --------------------------------------------------------

/// Rules of the limited format over a team draft event's steps: each pick
/// takes one card of the pack its picker holds into that picker's pool (the
/// event's procedure, DESIGN.md 2.3); a deck holds at least forty cards, its
/// cards the pool's (plus basic lands), and the sideboard is the rest of the
/// pool (CR 100.2b, 100.4b).
#[derive(Clone, Debug, Default)]
pub struct DraftCheck {
    /// The previous state: its phase, the seat to pick and the pack it held,
    /// every seat's pool.
    prev: Option<(crate::az::draft::Phase, u8, Vec<u16>, Vec<Vec<u16>>)>,
    pub picks: u64,
    pub decks: u64,
}

impl DraftCheck {
    pub fn new() -> DraftCheck {
        DraftCheck::default()
    }

    pub fn step(&mut self, g: &crate::az::draft::TeamDraftAz) -> Checked {
        use crate::az::draft::Phase;
        let n = g.ev.n;
        let pools: Vec<Vec<u16>> = (0..n).map(|s| g.pool(s)).collect();
        if let Some((phase, picker, held, before)) = self.prev.take() {
            if phase == Phase::Draft {
                self.picks += 1;
                for s in 0..n as usize {
                    let grew: Vec<u16> = diff16(&pools[s], &before[s]);
                    let lost = diff16(&before[s], &pools[s]);
                    if !lost.is_empty() {
                        return fail("draft (DESIGN.md 2.3)", format!("seat {s}'s pool lost cards {lost:?}"));
                    }
                    let want = (s == picker as usize) as usize;
                    if grew.len() != want {
                        return fail("draft (DESIGN.md 2.3)", format!("seat {s} gained {} cards in one pick by seat {picker}", grew.len()));
                    }
                    if let Some(&c) = grew.first() {
                        if !held.contains(&c) {
                            return fail("draft (DESIGN.md 2.3)", format!("seat {s} picked card {c}, which wasn't in the pack it held"));
                        }
                    }
                }
            } else {
                for s in 0..n as usize {
                    if pools[s] != before[s] {
                        return fail("draft (DESIGN.md 2.3)", format!("seat {s}'s pool changed after the draft"));
                    }
                }
            }
        }
        if g.phase() != Phase::Draft {
            let cube = crate::az::draft::cube().map_err(|e| Violation { rule: "100.2b", what: e })?;
            for s in 0..n {
                let Some(b) = g.build(s) else { continue };
                let finished = g.phase() == Phase::Done || s < g.builder();
                if !finished {
                    continue;
                }
                self.decks += 1;
                let deck = b.deck(cube);
                if deck.len() < 40 {
                    return fail("100.2b", format!("seat {s}'s deck has {} cards", deck.len()));
                }
                let mut pool = g.pool(s);
                pool.sort_unstable();
                let mut basics_free: Vec<u16> = deck.iter().copied().filter(|c| cube.basic_ids.contains(c)).collect();
                basics_free.sort_unstable();
                let deck_pool: Vec<u16> = diff16(&deck, &basics_free);
                let not_in_pool = diff16(&deck_pool, &pool);
                if !not_in_pool.is_empty() {
                    return fail("100.2b", format!("seat {s}'s deck holds cards not in its pool: {not_in_pool:?}"));
                }
                if let Some((dk, sb)) = g.deck_and_sideboard(s) {
                    let mut dk: Vec<DefId> = dk;
                    let mut sb: Vec<DefId> = sb;
                    dk.sort_unstable();
                    sb.sort_unstable();
                    let basic_defs: Vec<DefId> = cube.basic_ids.iter().map(|&i| cube.def_of(i)).collect();
                    let mut all: Vec<DefId> = dk.iter().copied().filter(|d| !basic_defs.contains(d)).chain(sb.iter().copied().filter(|d| !basic_defs.contains(d))).collect();
                    all.sort_unstable();
                    let mut want: Vec<DefId> = pool.iter().map(|&c| cube.def_of(c)).filter(|d| !basic_defs.contains(d)).collect();
                    want.sort_unstable();
                    if all != want {
                        return fail("100.4b", format!("seat {s}: deck + sideboard is not its pool"));
                    }
                }
            }
        }
        let picker = if g.phase() == Phase::Draft { g.decider().unwrap_or(u8::MAX) } else { u8::MAX };
        let held = g.pack_held(picker).map(|p| g.pack(p).to_vec()).unwrap_or_default();
        self.prev = Some((g.phase(), picker, held, pools));
        Ok(())
    }
}

fn diff16(a: &[u16], b: &[u16]) -> Vec<u16> {
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    a.sort_unstable();
    b.sort_unstable();
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() {
        if j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < b.len() && b[j] < a[i] {
            j += 1;
        } else {
            out.push(a[i]);
            i += 1;
        }
    }
    out
}

/// An object reference's current object (for messages).
#[allow(dead_code)]
fn obj_name(g: &Game, r: ObjRef) -> String {
    describe(g, r.idx)
}

// ---- tests: each invariant catches a crafted violation and passes the legal
// states that look like one (the rules' exceptions) --------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CardDb, CardDef};
    use crate::decision::{act, Decision, Opt, Stage};
    use crate::testkit::Scenario;

    fn db() -> &'static CardDb {
        CardDb::cube()
    }

    /// A supported card of the cube matching `f` (its front face).
    fn card(f: impl Fn(&CardDef) -> bool) -> &'static str {
        let db = db();
        (0..db.num_cards())
            .map(|i| db.def(i as DefId))
            .find(|d| !d.token && d.front.is_none() && db.id(&d.name).is_some() && f(d))
            .map(|d| d.name.as_str())
            .expect("a card of the cube with these properties")
    }

    fn creature(f: impl Fn(&CardDef) -> bool) -> &'static str {
        card(|d| {
            d.is(types::CREATURE)
                && !d.is(types::PLANESWALKER)
                && d.statics.is_empty()
                && d.triggers.is_empty()
                && d.toughness.unwrap_or(0) >= 1
                && d.pt_cda.is_none()
                && d.back.is_none()
                && f(d)
        })
    }

    fn vanilla() -> &'static str {
        creature(|d| d.keywords.0 == 0 && d.toughness == Some(2) && d.supertypes == 0)
    }

    fn token_creature() -> String {
        (db().num_cards()..db().len()).map(|i| db().def(i as DefId)).find(|d| d.token && d.is(types::CREATURE)).unwrap().name.clone()
    }

    /// A game at P0's priority in its precombat main phase (no single option
    /// answered for them: every decision is seen).
    fn at_priority(sc: Scenario) -> Game {
        let g = sc.spec(|s| s.auto_single = false).at(Step::Main1, 0).start();
        assert_eq!(g.pending.as_ref().map(|d| d.kind), Some(DecisionKind::Priority), "{:?}", g.pending);
        g
    }

    fn rule(r: Checked) -> &'static str {
        r.expect_err("the rules check should fail").rule
    }

    fn priority(g: &mut Game, p: PlayerId) {
        g.pending = Some(Decision { player: p, kind: DecisionKind::Priority, options: vec![Opt { action: act::PASS, choice: Choice::Pass }], stage: Stage::One });
    }

    fn other_decision(g: &mut Game) {
        g.pending = Some(Decision {
            player: 0,
            kind: DecisionKind::YesNo,
            options: vec![Opt { action: act::PASS, choice: Choice::Pass }, Opt { action: act::YES, choice: Choice::Yes }],
            stage: Stage::One,
        });
    }

    fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
        g.card(p, name, Zone::Battlefield)
    }

    fn item(g: &mut Game, c: CardIdx, ability: crate::db::AbilityRef) -> u32 {
        let id = g.new_stack_id();
        g.stack.push(crate::game::StackItem {
            id,
            kind: StackKind::Ability { ability, source: g.obj_ref(c) },
            controller: 0,
            targets: Vec::new(),
            cost_option: 0,
            modes: Vec::new(),
            ev: Default::default(),
            alt: 0,
            from: crate::game::CastFrom::Hand,
            delved: 0,
            delve_floor: 0,
            discarded_land: false,
            x: 0,
            division: Vec::new(),
            attack_as: None,
            noted: Vec::new(),
            flips: 0,
            origin: Zone::Nowhere,
            times: 0,
            spliced: Vec::new(),
        });
        id
    }

    fn push_item(g: &mut Game, c: CardIdx) -> u32 {
        let ability = crate::db::AbilityRef { def: g.objs[c.i()].def, slot: crate::db::AbilitySlot::Trigger(0), mode: None };
        item(g, c, ability)
    }

    // -- 704.5a-c, 117.5, 502.4

    #[test]
    fn cr_704_5a_c_a_player_at_0_life_or_10_poison_has_lost_before_priority() {
        let mut g = at_priority(Scenario::new(db()).library(0, &["Island"; 3]).library(1, &["Island"; 3]));
        check_state(&g).unwrap();
        g.players[1].life = 1;
        check_state(&g).unwrap();
        g.players[1].life = 0;
        assert_eq!(rule(check_state(&g)), "704.5a");
        g.players[1].life = -3;
        assert_eq!(rule(check_state(&g)), "704.5a");
        g.players[1].life = 20;
        g.players[0].poison = 9;
        check_state(&g).unwrap();
        g.players[0].poison = 10;
        assert_eq!(rule(check_state(&g)), "704.5c");
        g.players[0].poison = 0;
        g.players[0].drew_from_empty = true;
        assert_eq!(rule(check_state(&g)), "704.5b");
        // Not when no player is about to receive priority (between checks).
        other_decision(&mut g);
        check_state(&g).unwrap();
    }

    #[test]
    fn cr_117_5_triggered_abilities_are_put_on_the_stack_before_priority() {
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[vanilla()]));
        let c = bf(&g, 0, vanilla());
        g.waiting.push(crate::triggers::PendingTrigger { controller: 0, source: g.obj_ref(c), source_def: g.objs[c.i()].def, slot: 0, ev: Default::default() });
        assert_eq!(rule(check_state(&g)), "117.5, 603.3");
        other_decision(&mut g);
        check_state(&g).unwrap();
    }

    #[test]
    fn cr_502_4_no_priority_in_the_untap_step() {
        let mut g = at_priority(Scenario::new(db()));
        g.turn.step = Step::Untap;
        assert_eq!(rule(check_state(&g)), "502.4");
        g.turn.step = Step::Upkeep;
        check_state(&g).unwrap();
    }

    #[test]
    fn cr_104_3_a_player_who_lost_ends_the_game() {
        let mut g = at_priority(Scenario::new(db()));
        g.players[1].lost = true;
        assert_eq!(rule(check_state(&g)), "104.3");
    }

    // -- 704.5f-h

    #[test]
    fn cr_704_5f_toughness_0_and_704_5g_lethal_damage_but_indestructible() {
        let v = vanilla();
        let ind = creature(|d| d.keywords.has(Kw::Indestructible));
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[v, ind]));
        let (c, i) = (bf(&g, 0, v), bf(&g, 0, ind));
        g.objs[c.i()].damage = 1;
        check_state(&g).unwrap();
        g.objs[c.i()].damage = 2;
        assert_eq!(rule(check_state(&g)), "704.5g");
        g.objs[c.i()].damage = 0;
        // Indestructible ignores lethal damage (702.12b) ...
        g.objs[i.i()].damage = 100;
        check_state(&g).unwrap();
        // ... but not toughness 0 (704.5f).
        g.add_counters(i, Counter::M1M1, 100);
        assert_eq!(rule(check_state(&g)), "704.5f");
        g.counters.retain(|x| x.0 != i);
        g.add_counters(c, Counter::M1M1, 2);
        assert_eq!(rule(check_state(&g)), "704.5f");
    }

    #[test]
    fn cr_704_5h_deathtouch_damage_destroys_even_unmarked_infect_damage() {
        let v = vanilla();
        let ind = creature(|d| d.keywords.has(Kw::Indestructible));
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[v, ind]));
        let (c, i) = (bf(&g, 0, v), bf(&g, 0, ind));
        // Dealt deathtouch damage (1 marked) since the last check: destroyed.
        g.objs[c.i()].damage = 1;
        g.objs[c.i()].deathtouched = true;
        assert_eq!(rule(check_state(&g)), "704.5h");
        // Damage from a deathtouch source with infect isn't marked (702.90c):
        // still dealt deathtouch damage.
        g.objs[c.i()].damage = 0;
        assert_eq!(rule(check_state(&g)), "704.5h");
        g.objs[c.i()].deathtouched = false;
        check_state(&g).unwrap();
        // Indestructible: not destroyed, but the record is from before the
        // check that just happened.
        g.objs[i.i()].deathtouched = true;
        g.objs[i.i()].damage = 1;
        let e = check_state(&g).unwrap_err();
        assert!(e.rule == "704.5h" && e.what.contains("since the last state-based check"), "{e}");
    }

    // -- 704.5i, j, q

    #[test]
    fn cr_704_5i_a_planeswalker_with_0_loyalty() {
        let pw = card(|d| d.is(types::PLANESWALKER) && d.loyalty.is_some_and(|l| l >= 2) && d.back.is_none() && d.statics.is_empty());
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[pw]));
        let c = bf(&g, 0, pw);
        check_state(&g).unwrap();
        let n = g.counters_on(c, Counter::Loyalty);
        g.remove_counters(c, Counter::Loyalty, n - 1);
        check_state(&g).unwrap();
        g.remove_counters(c, Counter::Loyalty, 1);
        assert_eq!(rule(check_state(&g)), "704.5i");
    }

    #[test]
    fn cr_704_5j_the_legend_rule_per_controller() {
        let l = creature(|d| d.supertypes & supertypes::LEGENDARY != 0);
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[l]).battlefield(1, &[l]));
        // Two players each controlling one: legal.
        check_state(&g).unwrap();
        let c = bf(&g, 1, l);
        g.objs[c.i()].controller = 0;
        assert_eq!(rule(check_state(&g)), "704.5j");
    }

    #[test]
    fn cr_704_5q_counters_annihilate() {
        let v = vanilla();
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[v]));
        let c = bf(&g, 0, v);
        g.add_counters(c, Counter::P1P1, 2);
        check_state(&g).unwrap();
        g.counters.push((c, Counter::M1M1, 1));
        assert_eq!(rule(check_state(&g)), "704.5q");
    }

    // -- 704.5m, n, p

    #[test]
    fn cr_704_5m_n_p_attachments() {
        let eq = card(|d| {
            d.is(types::ARTIFACT) && !d.is(types::CREATURE) && db().equipment_subtype().is_some_and(|s| d.subtypes.contains(&s)) && d.protection.is_empty()
        });
        let v = vanilla();
        let land = "Island";
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[eq, v, land]));
        let (e, c, l) = (bf(&g, 0, eq), bf(&g, 0, v), bf(&g, 0, land));
        check_state(&g).unwrap();
        g.objs[e.i()].attached = Some(g.obj_ref(c));
        check_state(&g).unwrap();
        // Equipment attached to a land (704.5n) or to itself.
        g.objs[e.i()].attached = Some(g.obj_ref(l));
        assert_eq!(rule(check_state(&g)), "704.5n, 301.5c");
        g.objs[e.i()].attached = Some(g.obj_ref(e));
        assert_eq!(rule(check_state(&g)), "704.5n, 301.5c");
        g.objs[e.i()].attached = None;
        // A creature attached to something (704.5p).
        g.objs[c.i()].attached = Some(g.obj_ref(l));
        assert_eq!(rule(check_state(&g)), "704.5p");
        g.objs[c.i()].attached = None;
        // An Aura (Edge of Eternities' table: the cube's only Aura is Animate
        // Dead): unattached, or attached to what its enchant ability can't
        // enchant (704.5m), or attached legally.
        let edb = CardDb::eoe();
        let ename = |f: &dyn Fn(&CardDef) -> bool| -> &'static str {
            (0..edb.num_cards()).map(|i| edb.def(i as DefId)).find(|d| !d.token && d.front.is_none() && edb.id(&d.name).is_some() && f(d)).map(|d| d.name.as_str()).expect("an EoE card")
        };
        let aura = ename(&|d| d.aura_spell && !d.reanimates && !d.bestowed && !d.is(types::CREATURE) && d.spell.as_ref().is_some_and(|s| s.targets.len() == 1));
        let ev = ename(&|d| d.is(types::CREATURE) && d.statics.is_empty() && d.triggers.is_empty() && d.keywords.0 == 0 && d.back.is_none() && d.toughness.unwrap_or(0) >= 1);
        let mut g = at_priority(Scenario::new(edb).graveyard(0, &[aura]).battlefield(0, &[ev, land]).battlefield(1, &[ev]));
        // (Put onto the battlefield unattached by hand: the engine's own
        // check would put it into the graveyard first.)
        let (a, c) = (g.card(0, aura, Zone::Graveyard), bf(&g, 0, ev));
        g.players[0].graveyard.retain(|&x| x != a);
        g.objs[a.i()].zone = Zone::Battlefield;
        g.next_ts += 1;
        g.objs[a.i()].timestamp = g.next_ts;
        g.battlefield.push(a);
        g.n_static += !g.def_of(a).statics.is_empty() as u16;
        assert_eq!(rule(check_state(&g)), "704.5m");
        let hosts = [c, bf(&g, 1, ev), bf(&g, 0, land)];
        let host = hosts.into_iter().find(|&h| g.aura_enchants(a, 0, h)).expect("something the Aura enchants");
        g.objs[a.i()].attached = Some(g.obj_ref(host));
        check_state(&g).unwrap();
        if let Some(bad) = hosts.into_iter().find(|&h| !g.aura_enchants(a, 0, h)) {
            g.objs[a.i()].attached = Some(g.obj_ref(bad));
            assert_eq!(rule(check_state(&g)), "704.5m, 303.4c");
        }
        g.objs[a.i()].attached = Some(g.obj_ref(a));
        assert!(rule(check_state(&g)).starts_with("303.4d"));
        // Animate Dead (704.5m with its changed enchant ability): a creature
        // card in a graveyard, not a land card.
        let mut g = at_priority(Scenario::new(db()).graveyard(0, &["Animate Dead"]).graveyard(1, &[v, land]));
        let a = g.card(0, "Animate Dead", Zone::Graveyard);
        g.players[0].graveyard.retain(|&x| x != a);
        g.objs[a.i()].zone = Zone::Battlefield;
        g.next_ts += 1;
        g.objs[a.i()].timestamp = g.next_ts;
        g.battlefield.push(a);
        g.n_static += !g.def_of(a).statics.is_empty() as u16;
        g.objs[a.i()].attached = Some(g.obj_ref(g.card(1, v, Zone::Graveyard)));
        check_state(&g).unwrap();
        g.objs[a.i()].attached = Some(g.obj_ref(g.card(1, land, Zone::Graveyard)));
        assert_eq!(rule(check_state(&g)), "704.5m, 303.4c");
    }

    // -- 704.5d-e, 111.8

    #[test]
    fn cr_704_5d_a_token_out_of_the_battlefield_ceases_to_exist_at_the_check() {
        let mut g = at_priority(Scenario::new(db()));
        let t = g.put_token_in_graveyard(0, &token_creature());
        assert_eq!(rule(check_state(&g)), "704.5d, 111.8");
        // Between two checks it may be there (its dies triggers see it).
        other_decision(&mut g);
        check_state(&g).unwrap();
        // Phased out, a token is still on the battlefield (702.26d).
        priority(&mut g, 0);
        g.players[0].graveyard.retain(|&x| x != t);
        g.objs[t.i()].zone = Zone::PhasedOut;
        g.w3.phased.push(t);
        check_state(&g).unwrap();
    }

    #[test]
    fn cr_111_8_a_token_that_left_the_battlefield_never_comes_back() {
        let name = token_creature();
        let mut g = at_priority(Scenario::new(db()));
        let t = g.put_token_in_graveyard(0, &name);
        other_decision(&mut g);
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        // Back onto the battlefield (a new object, as a zone change makes).
        g.players[0].graveyard.retain(|&x| x != t);
        g.objs[t.i()].zone = Zone::Battlefield;
        g.objs[t.i()].inc += 1;
        g.battlefield.push(t);
        assert_eq!(rule(rc.step(&g)), "111.8");
        // Ceasing to exist instead: fine.
        let mut g = at_priority(Scenario::new(db()));
        let t = g.put_token_in_graveyard(0, &name);
        other_decision(&mut g);
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        g.players[0].graveyard.retain(|&x| x != t);
        g.objs[t.i()].zone = Zone::Nowhere;
        g.objs[t.i()].inc += 1;
        rc.step(&g).unwrap();
    }

    // -- 704.5s, 704.5t

    #[test]
    fn cr_704_5s_a_finished_saga_without_a_pending_chapter() {
        let saga = card(|d| d.chapters > 0 && d.back.is_none() && !d.is(types::CREATURE));
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[saga]));
        let c = bf(&g, 0, saga);
        // (The precombat main phase's lore counter put a chapter ability on
        // the stack: none here, the counters set directly.)
        g.stack.clear();
        g.waiting.clear();
        let last = g.def_of(c).chapters as crate::num::Count;
        g.counters.retain(|x| x.0 != c);
        g.counters.push((c, Counter::Lore, last - 1));
        check_state(&g).unwrap();
        g.counters.retain(|x| x.0 != c);
        g.counters.push((c, Counter::Lore, last));
        assert_eq!(rule(check_state(&g)), "704.5s");
        // Its final chapter ability on the stack: it stays.
        let slot = g.def_of(c).triggers.iter().position(|t| t.event == crate::db::EventKind::Chapter).unwrap() as u8;
        let ability = crate::db::AbilityRef { def: g.objs[c.i()].def, slot: crate::db::AbilitySlot::Trigger(slot), mode: None };
        item(&mut g, c, ability);
        check_state(&g).unwrap();
    }

    #[test]
    fn cr_704_5t_a_finished_dungeon() {
        let mut g = at_priority(Scenario::new(db()).dungeon(0, Some(crate::dungeon::BOTTOM - 1), 0));
        check_state(&g).unwrap();
        g.dungeons[0].room = crate::dungeon::BOTTOM;
        assert_eq!(rule(check_state(&g)), "704.5t");
    }

    // -- 108.1, 108.3, 400.11b, 100.4, 702.139

    #[test]
    fn cr_108_cards_keep_their_owner_and_identity_whatever_face() {
        let dfc = card(|d| d.back.is_some() && !d.modal && d.is(types::CREATURE));
        let v = vanilla();
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[dfc, v]).hand(1, &["Island"]).sideboard(1, &["Mountain"]));
        let (c, x) = (bf(&g, 0, dfc), bf(&g, 0, v));
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        // Transformed (its back face up): the same card.
        let back = g.def_of(c).back.unwrap();
        g.set_face(c, back);
        rc.step(&g).unwrap();
        // Another owner (108.3).
        g.objs[x.i()].owner = 1;
        assert_eq!(rule(rc.clone().step(&g)), "108.3");
        g.objs[x.i()].owner = 0;
        // Another card.
        let isl = g.card(1, "Island", Zone::Hand);
        g.objs[isl.i()].def = db().expect_id("Mountain").unwrap();
        assert_eq!(rule(rc.clone().step(&g)), "108.1");
        g.objs[isl.i()].def = db().expect_id("Island").unwrap();
        rc.step(&g).unwrap();
        // A card from nowhere (400.11b): the sideboard's Mountain.
        let m = g.add_card(db().expect_id("Mountain").unwrap(), 1, Zone::Hand);
        g.players[1].hand.push(m);
        assert_eq!(rule(rc.step(&g)), "400.11b, 108.3");
    }

    #[test]
    fn cr_702_139_a_companion_enters_the_game_from_the_sideboard() {
        let comp = card(|d| d.companion.is_some());
        let mut g = at_priority(Scenario::new(db()).companion(0, comp));
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        let c = g.players[0].companion.unwrap();
        g.players[0].companion_used = true;
        g.objs[c.i()].zone = Zone::Hand;
        g.players[0].hand.push(c);
        rc.step(&g).unwrap();
        // A second copy of it from nowhere is not.
        let d = g.add_card(g.objs[c.i()].def, 0, Zone::Hand);
        g.players[0].hand.push(d);
        assert!(rc.step(&g).is_err());
        // (After a violation the game is checked no further.)
        rc.step(&g).unwrap();
    }

    // -- the stack: 405.2, 608.1, 700.2, 107.3a, 115.7

    #[test]
    fn cr_608_1_only_the_top_object_resolves() {
        let v = vanilla();
        let base = || {
            let mut g = at_priority(Scenario::new(db()).battlefield(0, &[v]));
            let c = bf(&g, 0, v);
            let a = push_item(&mut g, c);
            let b = push_item(&mut g, c);
            let mut rc = RulesCheck::new(&g);
            rc.step(&g).unwrap();
            (g, rc, a, b)
        };
        // The top one leaves: it resolved.
        let (mut g, mut rc, _, b) = base();
        g.stack.retain(|s| s.id != b);
        rc.step(&g).unwrap();
        // Both leave (the top one countered the other, or both resolved).
        let (mut g, mut rc, ..) = base();
        g.stack.clear();
        rc.step(&g).unwrap();
        // The bottom one leaves while the top one stays: nothing resolved.
        let (mut g, mut rc, a, _) = base();
        g.stack.retain(|s| s.id != a);
        assert_eq!(rule(rc.step(&g)), "608.1, 405.5");
        // ... unless something cast since resolved (and left).
        let (mut g, mut rc, a, _) = base();
        g.stack.retain(|s| s.id != a);
        g.new_stack_id();
        rc.step(&g).unwrap();
        // ... or it was resolving already (spells cast as it resolved are
        // above it, 608.2g).
        let (mut g, _, a, _) = base();
        g.agenda.push(Task::FinishResolve { id: a });
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        g.agenda.pop();
        g.stack.retain(|s| s.id != a);
        rc.step(&g).unwrap();
        // Reordered (405.2).
        let (mut g, mut rc, ..) = base();
        g.stack.swap(0, 1);
        assert_eq!(rule(rc.step(&g)), "405.2");
    }

    #[test]
    fn cr_115_7_targets_and_choices_stay_fixed_on_the_stack() {
        let v = vanilla();
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[v]).battlefield(1, &[v]));
        let c = bf(&g, 0, v);
        let o = bf(&g, 1, v);
        push_item(&mut g, c);
        let id = push_item(&mut g, c);
        g.stack[0].targets = vec![(0, Target::Obj(g.obj_ref(c)))];
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        let mut h = g.clone();
        let mut rh = rc.clone();
        h.stack[0].targets = vec![(0, Target::Obj(h.obj_ref(o)))];
        assert_eq!(rule(rh.step(&h)), "115.7");
        // An effect (something resolved in between) may change them.
        let mut h = g.clone();
        let mut rh = rc.clone();
        h.stack[0].targets = vec![(0, Target::Obj(h.obj_ref(o)))];
        h.stack.retain(|s| s.id != id);
        rh.step(&h).unwrap();
        // Modes and X never change.
        let mut h = g.clone();
        let mut rh = rc.clone();
        h.stack[0].x = 3;
        assert_eq!(rule(rh.step(&h)), "107.3a");
        let mut h = g.clone();
        h.stack[0].modes = vec![1];
        assert_eq!(rule(rc.step(&h)), "700.2");
    }

    // -- turns: 500.1, 102.1, 500.7, 500.8

    #[test]
    fn cr_500_the_turn_structure() {
        let g0 = at_priority(Scenario::new(db()));
        let step_to = |s: Step, f: &dyn Fn(&mut Game)| {
            let mut g = g0.clone();
            let mut rc = RulesCheck::new(&g);
            f(&mut g);
            rc.step(&g).unwrap();
            g.turn.step = s;
            rc.step(&g)
        };
        step_to(Step::Main2, &|_| {}).unwrap();
        assert_eq!(rule(step_to(Step::Upkeep, &|_| {})), "500.1, 500.8");
        // An earlier combat step only for an added combat phase.
        assert_eq!(rule(step_to(Step::BeginCombat, &|g| g.turn.step = Step::Main2)), "500.1, 500.8");
        step_to(Step::BeginCombat, &|g| {
            g.turn.step = Step::Main2;
            g.turn.extra_combats = 1;
        })
        .unwrap();
        // The active player is fixed within a turn (102.1); turns never go
        // back (500.1).
        let mut g = g0.clone();
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        g.turn.active = 1;
        assert_eq!(rule(rc.clone().step(&g)), "102.1");
        g.turn.active = 0;
        g.turn.number -= 1;
        assert_eq!(rule(rc.step(&g)), "500.1");
        // The next turn is the other player's ...
        let next = |extra: Vec<PlayerId>, active: PlayerId| {
            let mut g = g0.clone();
            g.extra_turns = extra;
            let mut rc = RulesCheck::new(&g);
            rc.step(&g).unwrap();
            g.extra_turns.pop();
            g.turn.number += 1;
            g.turn.active = active;
            g.turn.step = Step::Upkeep;
            rc.step(&g)
        };
        next(Vec::new(), 1).unwrap();
        assert_eq!(rule(next(Vec::new(), 0)), "500.7, 102.1");
        // ... unless an extra turn was added (500.7: the latest first).
        next(vec![1, 0], 0).unwrap();
        assert_eq!(rule(next(vec![0, 1], 0)), "500.7");
    }

    // -- combat: 508.1, 509.1, 510.1c, 702.19b

    #[test]
    fn cr_508_1a_only_creatures_that_may_attack_are_offered() {
        let v = vanilla();
        let haste = creature(|d| d.keywords.has(Kw::Haste) && !d.keywords.has(Kw::Defender));
        let mut g = Scenario::new(db()).spec(|s| s.auto_single = false).battlefield(0, &[v, haste]).at(Step::DeclareAttackers, 0).start();
        assert_eq!(g.pending.as_ref().map(|d| d.kind), Some(DecisionKind::DeclareAttackers));
        check_state(&g).unwrap();
        let (c, h) = (bf(&g, 0, v), bf(&g, 0, haste));
        // Both came under control this turn: the haste creature may attack.
        g.objs[c.i()].control_since = g.turn.number;
        g.objs[h.i()].control_since = g.turn.number;
        let e = check_state(&g).unwrap_err();
        assert!(e.what.contains(v) && e.what.contains("summoning sick"), "{e}");
        g.pending.as_mut().unwrap().options.retain(|o| o.choice != Choice::Card(c));
        check_state(&g).unwrap();
        g.objs[h.i()].tapped = true;
        assert!(check_state(&g).unwrap_err().what.contains("tapped"));
    }

    #[test]
    fn cr_509_1_blocks_by_flying_and_menace() {
        let flyer = creature(|d| d.keywords.has(Kw::Flying) && d.keywords.0 == 1 << Kw::Flying as u32);
        let menace = creature(|d| d.keywords.has(Kw::Menace) && !d.keywords.has(Kw::Flying) && !d.keywords.has(Kw::Vigilance));
        let v = vanilla();
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[flyer, menace]).battlefield(1, &[v, v]));
        let (f, m) = (bf(&g, 0, flyer), bf(&g, 0, menace));
        let bs: Vec<CardIdx> = g.permanents_of(1).collect();
        // Declared in the declare blockers step, checked at the next state.
        g.turn.step = Step::DeclareAttackers;
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        let declare = |blocks: &[(CardIdx, CardIdx)]| {
            let mut h = g.clone();
            for &a in &[f, m] {
                h.objs[a.i()].attacking = true;
                h.objs[a.i()].tapped = true;
            }
            for &(b, a) in blocks {
                h.objs[b.i()].blocking = Some(a);
                h.objs[a.i()].blocked = true;
            }
            h.turn.step = Step::DeclareBlockers;
            let mut r = rc.clone();
            r.step(&h)
        };
        declare(&[]).unwrap();
        declare(&[(bs[0], m), (bs[1], m)]).unwrap();
        assert_eq!(rule(declare(&[(bs[0], m)])), "509.1b, 702.111b");
        assert_eq!(rule(declare(&[(bs[0], f)])), "509.1b");
    }

    #[test]
    fn cr_508_1_attackers_as_declared() {
        let v = vanilla();
        let vig = creature(|d| d.keywords.has(Kw::Vigilance) && !d.keywords.has(Kw::Defender));
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[v, vig]));
        let (c, w) = (bf(&g, 0, v), bf(&g, 0, vig));
        g.turn.step = Step::BeginCombat;
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        let declare = |f: &dyn Fn(&mut Game)| {
            let mut h = g.clone();
            h.objs[c.i()].attacking = true;
            h.objs[c.i()].tapped = true;
            h.objs[w.i()].attacking = true;
            f(&mut h);
            h.turn.step = Step::DeclareAttackers;
            rc.clone().step(&h)
        };
        declare(&|_| {}).unwrap();
        // 508.1f: attacking taps it unless it has vigilance (702.20b).
        assert_eq!(rule(declare(&|h| h.objs[c.i()].tapped = false)), "508.1a, 508.1f, 702.20b");
        assert_eq!(rule(declare(&|h| h.objs[w.i()].tapped = true)), "508.1a, 508.1f, 702.20b");
        // 302.6: summoning sick.
        assert_eq!(rule(declare(&|h| h.objs[c.i()].control_since = h.turn.number)), "508.1a, 302.6, 702.3b");
    }

    #[test]
    fn cr_702_19b_trample_damage_assignment_options() {
        let tr = creature(|d| d.keywords.has(Kw::Trample) && !d.keywords.has(Kw::Deathtouch) && !d.keywords.has(Kw::FirstStrike) && !d.keywords.has(Kw::DoubleStrike) && d.power.unwrap_or(0) >= 5);
        let v = vanilla();
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[tr]).battlefield(1, &[v, v]));
        let a = bf(&g, 0, tr);
        let bs: Vec<CardIdx> = g.permanents_of(1).collect();
        g.objs[a.i()].attacking = true;
        g.objs[a.i()].blocked = true;
        for &b in &bs {
            g.objs[b.i()].blocking = Some(a);
        }
        g.turn.step = Step::CombatDamage;
        g.pending = None;
        g.agenda.push(Task::CombatDamage { first: false, att: 0 });
        g.advance();
        assert_eq!(g.pending.as_ref().map(|d| d.kind), Some(DecisionKind::AssignDamage));
        let mut st = Stats::default();
        check_state_stats(&g, &mut st).unwrap();
        assert_eq!(st.assign_options, 1);
        // Assign 2 (lethal) to the first: the second takes 2 or more, the rest
        // to the player.
        let two = g.pending.as_ref().unwrap().action_of(Choice::Number(2)).unwrap();
        let mut k = Vec::new();
        g.apply(two, &mut k).unwrap();
        check_state(&g).unwrap();
        let p = g.power(a) as usize;
        assert_eq!(g.pending.as_ref().unwrap().options.len(), p - 2 - 2 + 1);
        // An option the rules don't allow (less than lethal with the rest
        // to the player) is caught.
        g.pending.as_mut().unwrap().options.insert(0, Opt { action: act::NUMBER + 1, choice: Choice::Number(1) });
        assert_eq!(rule(check_state(&g)), "510.1c, 702.19b, 702.2c");
    }

    // -- 500.5, 119.7

    #[test]
    fn cr_500_5_mana_empties_between_steps() {
        let mut g = at_priority(Scenario::new(db()));
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        g.players[0].pool.amt[0] = 1;
        // In the same step it may float.
        rc.step(&g).unwrap();
        let mut h = g.clone();
        h.turn.step = Step::BeginCombat;
        assert_eq!(rule(rc.clone().step(&h)), "500.5");
        // Something put on the stack since (it may have made the mana): not
        // compared.
        h.new_stack_id();
        rc.step(&h).unwrap();
    }

    #[test]
    fn cr_119_7_no_life_gained_by_a_player_who_cant() {
        let mut g = at_priority(Scenario::new(db()));
        g.players[0].cant_gain_life = true;
        let mut rc = RulesCheck::new(&g);
        rc.step(&g).unwrap();
        g.players[0].life -= 3;
        rc.step(&g).unwrap();
        g.players[0].life += 1;
        assert_eq!(rule(rc.step(&g)), "119.7");
    }

    // -- engine findings (docs/rules_check_findings.md): each test asserts what
    // the rules require and fails on the engine as it is (ignored until fixed)

    /// F1 (fuzz, default pool seed 366): an indestructible creature dealt
    /// deathtouch damage keeps the engine's 704.5h record (`deathtouched`)
    /// through state-based checks that perform nothing (`sba_check`'s fast
    /// path), so once it loses indestructible a later check destroys it for
    /// damage dealt before earlier checks. CR 704.5h: only deathtouch damage
    /// "since the last time state-based actions were checked" destroys it.
    #[test]
    #[ignore = "engine rules bug F1 (docs/rules_check_findings.md): sba_check's fast path keeps a stale deathtouch record"]
    fn f1_cr_704_5h_old_deathtouch_damage_does_not_destroy_after_indestructible_ends() {
        let v = vanilla();
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[v, "Kaldra Compleat"]));
        let (c, k) = (bf(&g, 0, v), bf(&g, 0, "Kaldra Compleat"));
        g.objs[k.i()].attached = Some(g.obj_ref(c));
        assert!(g.has_kw(c, Kw::Indestructible));
        // Dealt 1 damage by a source with deathtouch (as combat damage marks it).
        g.objs[c.i()].damage = 1;
        g.objs[c.i()].deathtouched = true;
        // A player is about to receive priority: state-based actions are
        // checked (it is indestructible: nothing happens).
        g.pending = None;
        g.agenda.push(Task::Priority);
        g.advance();
        assert_eq!(g.objs[c.i()].zone, Zone::Battlefield);
        // The Equipment becomes unattached (moved to another creature, say):
        // a 2/2 with 1 damage, dealt deathtouch damage before the last check.
        g.objs[k.i()].attached = None;
        g.pending = None;
        g.agenda.push(Task::Priority);
        g.advance();
        assert_eq!(g.objs[c.i()].zone, Zone::Battlefield, "destroyed for deathtouch damage dealt before the last state-based check
{}", g.log_lines().join("
"));
    }

    /// F2 (found writing 704.5h's check): damage from a source with infect and
    /// deathtouch puts -1/-1 counters instead of marking damage (702.90c) but
    /// is still damage from a deathtouch source (704.5h, 702.2b): the creature
    /// is destroyed. The engine's check reads deathtouch only with marked
    /// damage (`deathtouched && damage > 0`).
    #[test]
    #[ignore = "engine rules bug F2 (docs/rules_check_findings.md): infect + deathtouch damage doesn't destroy"]
    fn f2_cr_704_5h_deathtouch_damage_with_infect_destroys() {
        let v = vanilla();
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[v]).battlefield(1, &["Blightsteel Colossus"]));
        let (c, b) = (bf(&g, 0, v), bf(&g, 1, "Blightsteel Colossus"));
        let mut src = g.damage_source_of(b);
        assert!(src.infect);
        src.deathtouch = true;
        g.deal_damage(src, Target::Obj(g.obj_ref(c)), 1);
        assert_eq!(g.counters_on(c, Counter::M1M1), 1);
        g.pending = None;
        g.agenda.push(Task::Priority);
        g.advance();
        assert_eq!(g.objs[c.i()].zone, Zone::Graveyard, "dealt damage by a deathtouch source and not destroyed
{}", g.log_lines().join("
"));
    }

    /// F3 (found writing 510.1c's check): lethal damage for trample's
    /// assignment counts damage already marked on a blocker (702.19b); with
    /// deathtouch any nonzero amount is lethal (702.2c), so a blocker whose
    /// marked damage is already lethal (an indestructible one) needs none.
    /// The engine's `lethal_needed` with deathtouch ignores marked damage:
    /// the trampler must still assign it 1.
    #[test]
    #[ignore = "engine rules bug F3 (docs/rules_check_findings.md): deathtouch trample ignores lethal damage already marked"]
    fn f3_cr_702_19b_deathtouch_trample_counts_marked_lethal_damage() {
        let tr = creature(|d| d.keywords.has(Kw::Trample) && !d.keywords.has(Kw::FirstStrike) && !d.keywords.has(Kw::DoubleStrike) && d.power.unwrap_or(0) >= 3);
        let ind = creature(|d| d.keywords.has(Kw::Indestructible));
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[tr]).battlefield(1, &[ind]));
        let (a, b) = (bf(&g, 0, tr), bf(&g, 1, ind));
        g.next_ts += 1;
        let ts = g.next_ts;
        g.effects.push(crate::layers::Effect { ts, until: crate::layers::Until::EndOfTurn, on: crate::layers::Affected::Obj(g.obj_ref(a)), m: crate::db::Mod::Keywords(crate::db::KwSet(1 << Kw::Deathtouch as u32)), controller: 0 });
        assert!(g.has_kw(a, Kw::Deathtouch));
        g.objs[b.i()].damage = g.toughness(b) as u64;
        g.objs[a.i()].attacking = true;
        g.objs[a.i()].blocked = true;
        g.objs[b.i()].blocking = Some(a);
        g.turn.step = Step::CombatDamage;
        g.pending = None;
        g.agenda.push(Task::CombatDamage { first: false, att: 0 });
        g.advance();
        // Every option from 0 (the blocker already has lethal damage) up.
        assert_eq!(g.pending.as_ref().map(|d| d.kind), Some(DecisionKind::AssignDamage));
        assert!(g.pending.as_ref().unwrap().choice_of(act::NUMBER).is_some(), "0 to the blocker is not offered: {:?}", g.options_desc());
    }

    /// F4 (found writing the turn checks): 508.8 skips the declare blockers
    /// and combat damage steps only if no creature was declared as an
    /// attacker (or put onto the battlefield attacking). The engine skips
    /// them when no creature is attacking as the declare attackers step ends,
    /// so an attacker declared and then removed from combat in that step (an
    /// instant kills it) costs both players their priority in those steps.
    #[test]
    #[ignore = "engine rules bug F4 (docs/rules_check_findings.md): declare blockers skipped after an attacker left combat"]
    fn f4_cr_508_8_declared_attackers_keep_the_declare_blockers_step() {
        let v = vanilla();
        let mut g = Scenario::new(db()).spec(|s| s.auto_single = false).battlefield(0, &[v]).at(Step::DeclareAttackers, 0).start();
        let c = bf(&g, 0, v);
        let attack = g.pending.as_ref().unwrap().action_of(Choice::Card(c)).unwrap();
        let mut k = Vec::new();
        g.apply(attack, &mut k).unwrap();
        g.apply(act::PASS, &mut k).unwrap();
        assert!(g.objs[c.i()].attacking);
        assert_eq!((g.turn.step, g.pending.as_ref().map(|d| d.kind)), (Step::DeclareAttackers, Some(DecisionKind::Priority)));
        // Removed from combat in the declare attackers step (CR 506.4).
        g.objs[c.i()].attacking = false;
        while g.turn.step == Step::DeclareAttackers {
            g.apply(act::PASS, &mut k).unwrap();
        }
        assert_eq!(g.turn.step, Step::DeclareBlockers, "{}", g.log_lines().join("
"));
    }

    /// F5 (found writing the damage checks): 510.4: in the second combat
    /// damage step deal damage only the creatures that had neither first nor
    /// double strike as the first combat damage step began, and those with
    /// double strike. The engine reads "dealt damage in the first step"
    /// (`struck_first`), set only when damage was assigned, so a first
    /// striker with 0 power in the first step that is pumped before the
    /// second deals damage then.
    #[test]
    #[ignore = "engine rules bug F5 (docs/rules_check_findings.md): a first striker without damage in the first step deals damage in the second"]
    fn f5_cr_510_4_a_first_striker_never_deals_damage_in_the_second_step() {
        let fs = creature(|d| d.keywords.has(Kw::FirstStrike) && !d.keywords.has(Kw::DoubleStrike) && d.power.unwrap_or(0) >= 1);
        let mut g = at_priority(Scenario::new(db()).battlefield(0, &[fs]));
        let a = bf(&g, 0, fs);
        g.objs[a.i()].attacking = true;
        g.objs[a.i()].tapped = true;
        // Its power is 0 as the first-strike damage step begins.
        let p = g.power(a) as i64;
        g.next_ts += 1;
        let ts = g.next_ts;
        g.effects.push(crate::layers::Effect { ts, until: crate::layers::Until::EndOfTurn, on: crate::layers::Affected::Obj(g.obj_ref(a)), m: crate::db::Mod::Pt(-p, 0), controller: 0 });
        assert_eq!(g.power(a), 0);
        let life = g.players[1].life;
        g.turn.step = Step::FirstStrikeDamage;
        g.pending = None;
        g.agenda.push(Task::CombatDamage { first: true, att: 0 });
        g.advance();
        assert_eq!(g.players[1].life, life);
        // Pumped back before the second combat damage step.
        g.effects.pop();
        g.turn.step = Step::CombatDamage;
        g.pending = None;
        g.agenda.push(Task::CombatDamage { first: false, att: 0 });
        g.advance();
        assert_eq!(g.players[1].life, life, "the first striker dealt damage in the second step
{}", g.log_lines().join("
"));
    }

    // -- the team draft: 100.2b, 100.4b

    #[test]
    fn cr_100_2b_100_4b_draft_events() {
        use crate::az::draft::{DraftSpec, Phase, TeamDraftAz};
        let mut g = TeamDraftAz::new(&DraftSpec::parse("{}").unwrap(), 2, 7).unwrap();
        let mut dc = DraftCheck::new();
        let mut rng = crate::rng::Rng::new(3);
        let (mut l, mut k) = (Vec::new(), Vec::new());
        dc.step(&g).unwrap();
        while g.decider().is_some() {
            g.legal(&mut l);
            g.apply(l[rng.below(l.len() as u32) as usize], &mut k).unwrap();
            dc.step(&g).unwrap();
        }
        assert!(dc.picks > 0 && dc.decks > 0);
        assert_eq!(g.phase(), Phase::Done);
        // A deck of 39 cards.
        let b = &mut g.builds[0];
        match b.basics.iter().position(|&n| n > 0) {
            Some(i) => b.basics[i] -= 1,
            None => b.inn &= b.inn - 1,
        }
        assert_eq!(rule(dc.step(&g)), "100.2b");
    }
}
