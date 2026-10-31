//! Light decisions (ENGINE.md 77; zero-core docs/search.md 3.10): the "timing windows" a self-play
//! seat with zero-core's `sims_light` may search with a small budget.
//!
//! A timing window is a **priority decision with an empty stack outside the decider's own main
//! phases** - the definition of the measurement that found them (E:/mtg-zero-scratch/measure-play,
//! measure.py's `category`: 29% of the Delver mirror's searched decisions and of its self-play
//! simulations, the network acting in ~9% of them). Its class is the step and whose turn it is,
//! from the decider's side: `own_<step>` on the decider's turn, `opp_<step>` on the opponent's
//! (`CLASSES`), so a run selects the steps it searches light (zero-core's run setting
//! `selfplay_light_classes`, e.g. `all,-opp_end`).
//!
//! The edge cases, decided (ENGINE.md 77.2):
//! * **The decider's own main phases** are never windows (main 1, main 2: lands and sorcery-speed
//!   spells); the opponent's main phases are (`opp_main1`, `opp_main2`).
//! * **A non-empty stack** is a response, never a window - whoever's turn, whatever the stack holds.
//! * **Triggers waiting**: none at a priority decision. The `Priority` task puts every waiting
//!   trigger on the stack before it asks (ENGINE.md 5: "check SBAs; put waiting triggers on the
//!   stack" until neither happens, CR 117.5) - a trigger waiting makes the stack non-empty before
//!   the question, so the decision is a response. A trigger removed for want of a target leaves an
//!   empty stack and a window. (tests/light.rs checks every priority decision of whole games.)
//! * **Cleanup**: a priority decision in the cleanup step (CR 514.3a: an SBA happened there) is a
//!   window (`own_cleanup`, `opp_cleanup`) like the end step's; it is rare (none in the
//!   measurement's 54k decisions). The first-strike damage step is one like the others.
//! * **Untap and setup** never ask for priority (CR 502.4, mulligans): no class.
//! * Only `DecisionKind::Priority` - every other kind (paying, targets, attackers, blockers, a
//!   resolving spell's choices, mulligans, sideboarding) is never light.
//!
//! It reads only public facts of the live game (the decision's kind and decider, the turn, the
//! stack): the same in every determinized world. Nothing in the rules, the options, the encoding
//! or the keys depends on it.
use crate::decision::DecisionKind;
use crate::game::{Game, Step};

/// The classes: a window's step on the decider's own turn, then on the opponent's.
pub const CLASSES: &[&str] = &[
    "own_upkeep",
    "own_draw",
    "own_begin_combat",
    "own_declare_attackers",
    "own_declare_blockers",
    "own_first_strike_damage",
    "own_combat_damage",
    "own_end_combat",
    "own_end",
    "own_cleanup",
    "opp_upkeep",
    "opp_draw",
    "opp_main1",
    "opp_begin_combat",
    "opp_declare_attackers",
    "opp_declare_blockers",
    "opp_first_strike_damage",
    "opp_combat_damage",
    "opp_end_combat",
    "opp_main2",
    "opp_end",
    "opp_cleanup",
];

/// The class of a window in `step` on the decider's own turn (`own`) or the opponent's; None where
/// a priority decision with an empty stack is no window (the decider's main phases) or cannot happen
/// (untap, setup).
pub fn class_of(own: bool, step: Step) -> Option<usize> {
    let name = match step {
        Step::Setup | Step::Untap => return None,
        Step::Main1 | Step::Main2 if own => return None,
        Step::Upkeep => "upkeep",
        Step::Draw => "draw",
        Step::Main1 => "main1",
        Step::BeginCombat => "begin_combat",
        Step::DeclareAttackers => "declare_attackers",
        Step::DeclareBlockers => "declare_blockers",
        Step::FirstStrikeDamage => "first_strike_damage",
        Step::CombatDamage => "combat_damage",
        Step::EndCombat => "end_combat",
        Step::Main2 => "main2",
        Step::End => "end",
        Step::Cleanup => "cleanup",
    };
    let side = if own { "own_" } else { "opp_" };
    CLASSES.iter().position(|c| c.len() == side.len() + name.len() && c.starts_with(side) && c.ends_with(name))
}

/// The pending decision's window class (an index into `CLASSES`), None when it is no timing window.
pub fn window(g: &Game) -> Option<usize> {
    let d = g.pending_decision()?;
    if !matches!(d.kind, DecisionKind::Priority) || !g.stack().is_empty() {
        return None;
    }
    let t = g.turn();
    class_of(t.active == d.player, t.step)
}
