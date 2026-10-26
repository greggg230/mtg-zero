//! Helpers shared by the rulings suites (`tests/rulings_*.rs`, ENGINE.md 15
//! and 37). Each test there checks one official ruling of a cube card; the
//! comment `ruling: <hash8>` names it (the first 8 hex digits of the SHA-1 of
//! the ruling's text in `data/rulings/powerlsv.rulings.json`), which
//! `tools/rulings_doc.py` reads to build `docs/rulings.md`.

#![allow(dead_code)]

use crate::common::db;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Target, Zone};

pub fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

pub fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

pub fn id(name: &str) -> u16 {
    db().expect_id(name).unwrap()
}

pub fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

pub fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

/// Answer decisions with `answer` (else the first option) and pass priority
/// until the stack is empty and a player has priority.
pub fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g) || g.is_over());
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

/// `resolve_with` answering every decision with its first option.
pub fn resolve(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Answer decisions with `answer` (else: pass priority, decline attacks and
/// blocks, the first option) until `stop`.
pub fn quiet_until_with(g: &mut Game, stop: impl Fn(&Game) -> bool, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..20_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("game over\n{}", g.log_lines().join("\n")));
        let c = match answer(g) {
            Some(c) => c,
            None => match d.kind {
                DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
                _ => d.options[0].choice,
            },
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress\n{}", g.log_lines().join("\n"));
}

pub fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    quiet_until_with(g, stop, |_| None);
}

/// Cast `card` and resolve everything, answering targets in order from
/// `targets` (then `answer`, then first options).
pub fn cast_targets(g: &mut Game, card: CardIdx, targets: &[Choice]) {
    g.choose(Choice::Cast(card));
    answer_targets(g, targets);
}

/// Resolve the stack, answering `Target` decisions with `targets` in order.
pub fn answer_targets(g: &mut Game, targets: &[Choice]) {
    let i = std::cell::Cell::new(0);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) if i.get() < targets.len() => {
            let c = targets[i.get()];
            i.set(i.get() + 1);
            Some(c)
        }
        _ => None,
    });
}

pub fn tokens_named(g: &Game, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == name).collect()
}

/// The options' descriptions of the pending decision, for messages.
pub fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}
