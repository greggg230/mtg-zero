//! M7b part 3b (ENGINE.md 27.1-27.2): spells and abilities on the stack as
//! targets (CR 115.1, 115.4, 115.5, 601.2c), countering an ability (701.5a,
//! 608.2b), gaining control of a spell (608.3a) and choosing new targets
//! (115.7), and "becomes the target" (603.2e) for players, permanents,
//! spells and copies of spells (707.10c).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter};
use engine::hidden::{key, KEY_COIN};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
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

/// Answer the decisions of a cast or activation (`answer`, else the first
/// option) until a player has priority again.
fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress");
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

/// Target `c` when a target is asked.
fn tgt_if(g: &Game, c: CardIdx) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(c))
}

/// Target player `p` when a target is asked.
fn player_if(g: &Game, p: PlayerId) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target)).then(|| player(p))
}

/// The ability target on the stack offered by the pending decision, if any.
fn stack_target(g: &Game) -> Option<Choice> {
    options(g).into_iter().find(|c| matches!(c, Choice::Target(Target::Stack(_))))
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}

const TIDEBINDER: &str = "Tishana's Tidebinder";
const BALLISTA: &str = "Walking Ballista";

/// P0's Walking Ballista (two counters) pings P1; P1 answers with Tishana's
/// Tidebinder, whose trigger targets the ping. Returns (Ballista,
/// Tidebinder).
fn ballista_vs_tidebinder(g: &mut Game) -> (CardIdx, CardIdx) {
    let wb = bf(g, 0, BALLISTA);
    g.choose(Choice::Ability(wb, AbilitySlot::Activated(1)));
    finish(g, |g| g.has_option(player(1)).then_some(player(1)));
    assert_eq!(g.stack().len(), 1, "the ping is on the stack");
    g.pass(); // P0 passes; P1 has priority
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    finish(g, |_| None);
    resolve_with(g, stack_target);
    (wb, g.card(1, TIDEBINDER, Zone::Battlefield))
}

#[test]
fn an_activated_ability_on_the_stack_is_targeted_and_countered_cr_115_1_701_5a() {
    // Tishana's Tidebinder: "When this creature enters, counter up to one
    // target activated or triggered ability. If an ability of an artifact,
    // creature, or planeswalker is countered this way, that permanent loses
    // all abilities for as long as this creature remains on the
    // battlefield."
    let mut g = scen()
        .battlefield(0, &[BALLISTA])
        .counters(0, BALLISTA, Counter::P1P1, 2)
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &[TIDEBINDER])
        .start();
    let (wb, tb) = ballista_vs_tidebinder(&mut g);
    assert_eq!(g.life(1), 20, "the ping was countered: it didn't resolve");
    assert_eq!(g.counters_on(wb, Counter::P1P1), 1, "its cost was paid");
    // The Ballista (an artifact creature) lost all its abilities.
    assert!(!g.has_option(Choice::Ability(wb, AbilitySlot::Activated(1))));
    let _ = tb;
}

#[test]
fn the_loss_of_abilities_lasts_while_tidebinder_remains_cr_611_2b() {
    let mut g = scen()
        .battlefield(0, &[BALLISTA, "Mountain"])
        .counters(0, BALLISTA, Counter::P1P1, 2)
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &[TIDEBINDER])
        .start();
    let (wb, tb) = ballista_vs_tidebinder(&mut g);
    assert!(!g.has_option(Choice::Ability(wb, AbilitySlot::Activated(1))));
    // Bolt Tidebinder (3/2): the Ballista has its abilities again.
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, tb));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(tb).zone, Zone::Graveyard);
    assert!(g.has_option(Choice::Ability(wb, AbilitySlot::Activated(1))), "{:?}", g.options_desc());
}

#[test]
fn an_ability_cant_target_itself_cr_115_5() {
    // With nothing else on the stack, Tidebinder's trigger has no target:
    // "up to one" lets it go on the stack with none.
    let mut g = scen().battlefield(0, &["Island", "Island", "Island"]).hand(0, &[TIDEBINDER]).start();
    let tb = g.card(0, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    finish(&mut g, |_| None);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(options(&g), vec![Choice::Pass]);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(g.card(0, TIDEBINDER, Zone::Battlefield)).zone, Zone::Battlefield);
}

#[test]
fn a_mana_ability_never_is_on_the_stack_to_be_targeted_cr_605_3b() {
    // A Llanowar Elves' mana pays for a spell without using the stack: no
    // ability target for Tidebinder's trigger.
    let mut g = scen().battlefield(0, &["Island", "Island", "Llanowar Elves"]).hand(0, &[TIDEBINDER]).start();
    let tb = g.card(0, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    finish(&mut g, |_| None);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(stack_target(&g).is_none());
}

#[test]
fn a_targeted_ability_that_left_the_stack_is_an_illegal_target_cr_608_2b() {
    // Two Tidebinders target the same ping; the second's trigger resolves
    // first and counters it; the first's target is gone, so it doesn't
    // resolve (and its "loses all abilities" doesn't apply).
    let mut g = scen()
        .battlefield(0, &[BALLISTA])
        .counters(0, BALLISTA, Counter::P1P1, 2)
        .battlefield(1, &["Island"; 6])
        .hand(1, &[TIDEBINDER, TIDEBINDER])
        .start();
    let wb = bf(&g, 0, BALLISTA);
    g.choose(Choice::Ability(wb, AbilitySlot::Activated(1)));
    finish(&mut g, |g| g.has_option(player(1)).then_some(player(1)));
    g.pass();
    // P1: the first Tidebinder resolves, its trigger targets the ping.
    let tb1 = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb1));
    finish(&mut g, |_| None);
    g.pass();
    g.pass(); // it resolves
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let c = stack_target(&g).unwrap();
    g.choose(c);
    g.drive_until(at_priority);
    assert_eq!(g.turn().priority, 0, "the active player gets priority (117.3b): P0 passes");
    g.pass();
    // P1 responds with the second Tidebinder: its trigger targets the ping
    // too (the first trigger is an ability as well: either can be chosen).
    let tb2 = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb2));
    finish(&mut g, |_| None);
    g.pass();
    g.pass();
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let ping = g.stack()[0].id;
    g.choose(Choice::Target(Target::Stack(ping)));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(1), 20);
    assert!(g.log_lines().iter().any(|l| l.contains("fizzles")), "the first trigger's target was gone");
    // One "loses all abilities" effect, from the second Tidebinder: bolting
    // it would give the abilities back (the first one's never applied).
    assert_eq!(g.effects().len(), 1);
}

#[test]
fn becoming_the_target_triggers_once_per_spell_cr_603_2e() {
    // Leovold: "Whenever you or a permanent you control becomes the target
    // of a spell or ability an opponent controls, you may draw a card."
    // Mystic Confluence returns Leovold twice (two "target" words, 700.2d):
    // one trigger.
    let mut g = scen()
        .battlefield(0, &["Leovold, Emissary of Trest"])
        .battlefield(1, &["Island"; 5])
        .hand(1, &["Mystic Confluence"])
        .at(Step::Main1, 1)
        .start();
    let leo = bf(&g, 0, "Leovold, Emissary of Trest");
    let mc = g.card(1, "Mystic Confluence", Zone::Hand);
    g.choose(Choice::Cast(mc));
    finish(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(1)) => Some(Choice::Number(1)),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(2)),
        Some(DecisionKind::Target) => Some(g.tgt(leo)),
        _ => None,
    });
    let hand = g.player(0).hand.len();
    let asked = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => {
            asked.set(asked.get() + 1);
            Some(Choice::Yes)
        }
        _ => None,
    });
    assert_eq!(asked.get(), 1, "one trigger");
    assert_eq!(g.player(0).hand.len(), hand + 2, "a card from Leovold, and Leovold itself");
}

#[test]
fn a_player_becoming_the_target_is_seen_by_you_or_a_permanent_you_control() {
    // Leovold: P1's Lightning Bolt at P0 triggers it; P0's own Bolt at P0
    // doesn't (not an opponent's spell).
    let mut g = scen()
        .battlefield(0, &["Leovold, Emissary of Trest", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let bolt1 = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt1));
    finish(&mut g, |g| player_if(g, 0));
    let hand = g.player(0).hand.len();
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    assert_eq!(g.player(0).hand.len(), hand + 1);
    assert_eq!(g.life(0), 17);
}

#[test]
fn a_spell_becoming_the_target_is_a_target_spell_subject() {
    // Surrak: "Whenever a creature you control or a creature spell you
    // control becomes the target of a spell or ability an opponent
    // controls, draw a card." Counterspell on P0's creature spell.
    let mut g = scen()
        .battlefield(0, &["Surrak, Elusive Hunter", "Forest", "Forest"])
        .hand(0, &["Test Bear"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    finish(&mut g, |_| None);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    finish(&mut g, |g| tgt_if(g, bear));
    let hand = g.player(0).hand.len();
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).hand.len(), hand + 1, "Surrak's draw");
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

#[test]
fn surrak_cant_be_countered_cr_101_2() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest"])
        .hand(0, &["Surrak, Elusive Hunter"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .start();
    let s = g.card(0, "Surrak, Elusive Hunter", Zone::Hand);
    g.choose(Choice::Cast(s));
    finish(&mut g, |_| None);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    finish(&mut g, |g| tgt_if(g, s));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(s).zone, Zone::Battlefield);
}

#[test]
fn copies_of_a_spell_become_targets_as_they_are_put_on_the_stack_cr_707_10c() {
    // Lose Focus with replicate paid once, cast by P1 at P0's creature
    // spell: the spell and its copy each target it, so Surrak triggers
    // twice.
    let mut g = scen()
        .battlefield(0, &["Surrak, Elusive Hunter", "Forest", "Forest"])
        .hand(0, &["Test Bear"])
        .battlefield(1, &["Island"; 4])
        .hand(1, &["Lose Focus"])
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    finish(&mut g, |_| None);
    g.pass();
    let lf = g.card(1, "Lose Focus", Zone::Hand);
    g.choose(Choice::Cast(lf));
    finish(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    let hand = g.player(0).hand.len();
    // P0 doesn't pay for either.
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.player(0).hand.len(), hand + 2, "two of Surrak's draws");
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

#[test]
fn invert_polarity_gains_control_of_a_creature_spell_cr_608_3a() {
    // "Choose target spell, then flip a coin. If you win the flip, gain
    // control of that spell and you may choose new targets for it. If you
    // lose the flip, counter that spell."
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .hand(0, &["Test Bear"])
        .battlefield(1, &["Island", "Island", "Mountain"])
        .hand(1, &["Invert Polarity"])
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    finish(&mut g, |_| None);
    g.pass();
    let ip = g.card(1, "Invert Polarity", Zone::Hand);
    g.choose(Choice::Cast(ip));
    finish(&mut g, |g| tgt_if(g, bear));
    g.pass(); // P1
    g.force_chance(&[key(KEY_COIN, 1, 1)]);
    g.pass(); // P0: Invert Polarity resolves, P1 wins the flip
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.obj(bear).controller, 1, "it enters under the spell's new controller");
    assert_eq!(g.obj(bear).owner, 0);
}

#[test]
fn invert_polarity_counters_the_spell_when_the_flip_is_lost() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .hand(0, &["Test Bear"])
        .battlefield(1, &["Island", "Island", "Mountain"])
        .hand(1, &["Invert Polarity"])
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    finish(&mut g, |_| None);
    g.pass();
    let ip = g.card(1, "Invert Polarity", Zone::Hand);
    g.choose(Choice::Cast(ip));
    finish(&mut g, |g| tgt_if(g, bear));
    g.pass();
    g.force_chance(&[key(KEY_COIN, 1, 0)]);
    g.pass();
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

#[test]
fn a_stolen_spell_may_get_new_targets_which_become_targets_cr_115_7() {
    // P0's Lightning Bolt at P1; P1 steals it and redirects it at P0's
    // Leovold... at P0: the new target is chosen by P1.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Test Bear"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island", "Mountain"])
        .hand(1, &["Invert Polarity"])
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| player_if(g, 1));
    g.pass();
    let ip = g.card(1, "Invert Polarity", Zone::Hand);
    g.choose(Choice::Cast(ip));
    finish(&mut g, |g| tgt_if(g, bolt));
    g.pass();
    g.force_chance(&[key(KEY_COIN, 1, 1)]);
    g.pass();
    g.drive_until(|g| g.kind() == Some(DecisionKind::NewTarget));
    assert_eq!(g.pending_decision().unwrap().player, 1);
    assert!(g.has_option(Choice::Pass), "may keep it");
    g.choose(g.tgt(bear));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(1), 20);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.obj(bolt).owner, 0);
}

#[test]
fn ertai_counters_an_ability_and_its_controller_draws() {
    // Ertai Resurrected: "choose up to one - Counter target spell,
    // activated ability, or triggered ability. Its controller draws a card.
    // / Destroy another target creature or planeswalker. Its controller
    // draws a card."
    let mut g = scen()
        .battlefield(0, &[BALLISTA])
        .counters(0, BALLISTA, Counter::P1P1, 2)
        .battlefield(1, &["Island", "Island", "Swamp", "Swamp"])
        .hand(1, &["Ertai Resurrected"])
        .start();
    let wb = bf(&g, 0, BALLISTA);
    g.choose(Choice::Ability(wb, AbilitySlot::Activated(1)));
    finish(&mut g, |g| g.has_option(player(1)).then_some(player(1)));
    g.pass();
    let er = g.card(1, "Ertai Resurrected", Zone::Hand);
    g.choose(Choice::Cast(er));
    finish(&mut g, |_| None);
    let hand0 = g.player(0).hand.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(0)) => Some(Choice::Number(0)),
        Some(DecisionKind::Target) => stack_target(g),
        _ => None,
    });
    assert_eq!(g.life(1), 20, "the ping was countered");
    assert_eq!(g.player(0).hand.len(), hand0 + 1, "the ability's controller drew");
}

#[test]
fn ertai_destroys_and_the_creatures_controller_draws_even_if_indestructible() {
    let mut g = scen()
        .battlefield(0, &["Test Indestructible"])
        .battlefield(1, &["Island", "Island", "Swamp", "Swamp"])
        .hand(1, &["Ertai Resurrected"])
        .at(Step::Main1, 1)
        .start();
    let ti = bf(&g, 0, "Test Indestructible");
    let er = g.card(1, "Ertai Resurrected", Zone::Hand);
    g.choose(Choice::Cast(er));
    finish(&mut g, |_| None);
    let hand0 = g.player(0).hand.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(1)) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.tgt(ti)),
        _ => None,
    });
    assert_eq!(g.obj(ti).zone, Zone::Battlefield, "indestructible");
    assert_eq!(g.player(0).hand.len(), hand0 + 1, "its controller still draws");
}
