//! Priority, the stack, timing and targets (CR 117, 405, 601, 608, 103.8, 500.4).

mod common;

use common::db;
use engine::hidden::{key_kind, key_player, KEY_DRAW};
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, GameSpec, Step, Target, Zone};

fn manual() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

/// Cast `card` from P's hand, choosing `targets`, then pay with the first
/// offered sources until priority returns.
fn cast(g: &mut Game, p: u8, name: &str, targets: &[Choice]) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    for &t in targets {
        assert_eq!(g.kind(), Some(DecisionKind::Target), "{:?}", g.options_desc());
        g.choose(t);
    }
    while g.kind() == Some(DecisionKind::PayMana) {
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
    }
}

#[test]
fn cr_117_3a_active_player_receives_priority_first() {
    let g = manual().library(0, &["Island"; 3]).library(1, &["Island"; 3]).at(Step::Upkeep, 1).start();
    assert_eq!(g.kind(), Some(DecisionKind::Priority));
    assert_eq!(g.to_move(), Some(1));
    assert_eq!(g.turn().step, Step::Upkeep);
}

#[test]
fn cr_117_3b_c_d_priority_after_cast_pass_and_resolution() {
    let mut g = manual()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Lightning Bolt", &[Choice::Target(Target::Player(1))]);
    // 117.3c: the caster receives priority again.
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Priority), Some(0)));
    assert_eq!(g.stack().len(), 1);
    g.pass();
    // 117.3d: a pass gives priority to the next player.
    assert_eq!(g.to_move(), Some(1));
    g.pass();
    // 117.4: both passed -> the top object resolves; 117.3b: then the
    // active player receives priority.
    assert_eq!(g.life(1), 17);
    assert!(g.stack().is_empty());
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Priority), Some(0)));
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1);
}

#[test]
fn cr_117_3c_non_active_caster_keeps_priority() {
    // The Python engine gives priority back to the active player here; the
    // CR keeps it with the caster.
    let mut g = manual()
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt"])
        .at(Step::Upkeep, 0)
        .start();
    g.pass(); // P0
    assert_eq!(g.to_move(), Some(1));
    cast(&mut g, 1, "Lightning Bolt", &[Choice::Target(Target::Player(0))]);
    assert_eq!(g.to_move(), Some(1));
    cast(&mut g, 1, "Lightning Bolt", &[Choice::Target(Target::Player(0))]);
    assert_eq!(g.stack().len(), 2);
    assert_eq!(g.to_move(), Some(1));
}

#[test]
fn cr_117_4_all_pass_with_empty_stack_ends_the_step() {
    let mut g = manual().at(Step::Main1, 0).start();
    g.pass();
    g.pass();
    assert_eq!(g.turn().step, Step::BeginCombat);
    assert_eq!(g.to_move(), Some(0));
}

#[test]
fn cr_405_5_stack_resolves_last_in_first_out() {
    let mut g = manual()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Lightning Bolt", &[Choice::Target(Target::Player(1))]);
    g.pass();
    let bolt = g.card(0, "Lightning Bolt", Zone::Stack);
    let t = g.tgt(bolt);
    cast(&mut g, 1, "Counterspell", &[t]);
    assert_eq!(g.stack().len(), 2);
    g.pass(); // P1 holds priority after casting; passes
    g.pass(); // P0 passes: Counterspell (top) resolves first
    assert_eq!(g.stack().len(), 0, "Counterspell resolved and countered the Bolt");
    assert_eq!(g.life(1), 20);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Counterspell", Zone::Graveyard), 1);
}

#[test]
fn cr_305_2_one_land_per_turn() {
    let mut g = manual().hand(0, &["Island", "Mountain"]).at(Step::Main1, 0).start();
    let island = g.card(0, "Island", Zone::Hand);
    let mountain = g.card(0, "Mountain", Zone::Hand);
    assert!(g.has_option(Choice::Play(mountain)));
    g.choose(Choice::Play(island));
    // Playing a land is a special action: the player keeps priority (116.2a).
    assert_eq!(g.to_move(), Some(0));
    assert!(!g.has_option(Choice::Play(mountain)));
}

#[test]
fn cr_307_1_sorcery_timing_and_cr_702_8_flash() {
    let mut g = manual()
        .battlefield(0, &["Mountain", "Mountain", "Island", "Island"])
        .hand(0, &["Flame Slash", "Lightning Bolt", "Test Flash"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Upkeep, 1)
        .start();
    g.pass(); // P1 passes in their upkeep
    let slash = g.card(0, "Flame Slash", Zone::Hand);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let flash = g.card(0, "Test Flash", Zone::Hand);
    assert_eq!(g.to_move(), Some(0));
    assert!(!g.has_option(Choice::Cast(slash)), "sorcery on the opponent's turn");
    assert!(g.has_option(Choice::Cast(bolt)), "instant");
    assert!(g.has_option(Choice::Cast(flash)), "flash creature");

    // In its own main phase a sorcery is castable only with an empty stack.
    let mut g = manual()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Flame Slash", "Lightning Bolt"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let slash = g.card(0, "Flame Slash", Zone::Hand);
    assert!(g.has_option(Choice::Cast(slash)));
    cast(&mut g, 0, "Lightning Bolt", &[Choice::Target(Target::Player(1))]);
    assert!(!g.has_option(Choice::Cast(slash)), "stack not empty");
}

#[test]
fn cr_115_5_a_spell_cannot_target_itself() {
    let mut g = manual()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island", "Island", "Island"])
        .hand(1, &["Counterspell", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    // Nothing to counter: Counterspell is not castable (601.2c needs a target).
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.pass();
    assert!(!g.has_option(Choice::Cast(cs)));
    g.pass(); // step ends -> beginning of combat, P0 priority
    assert_eq!(g.turn().step, Step::BeginCombat);
    cast(&mut g, 0, "Lightning Bolt", &[Choice::Target(Target::Player(1))]);
    g.pass();
    let bolt = g.card(0, "Lightning Bolt", Zone::Stack);
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    // The only legal target is the Bolt, not the Counterspell itself.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert_eq!(g.pending_decision().unwrap().options.len(), 1);
    g.choose(g.tgt(bolt));
    while g.kind() == Some(DecisionKind::PayMana) {
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
    }
    // A second Counterspell can target either spell but not itself.
    let cs2 = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs2));
    let opts = g.options_desc();
    assert_eq!(opts.len(), 2, "{opts:?}");
}

#[test]
fn cr_608_2b_spell_with_all_targets_illegal_does_not_resolve() {
    let mut g = manual()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    let t = g.tgt(bear);
    cast(&mut g, 0, "Lightning Bolt", &[t]);
    cast(&mut g, 0, "Lightning Bolt", &[t]);
    g.pass();
    g.pass(); // second Bolt resolves; the Bear dies (SBA before priority)
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.stack().len(), 1);
    g.pass();
    g.pass(); // first Bolt: its target is gone -> it doesn't resolve
    assert!(g.stack().is_empty());
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 2);
    assert_eq!(g.life(1), 20);
}

#[test]
fn cr_500_4_mana_empties_between_steps() {
    let mut g = manual()
        .battlefield(0, &["Swamp"])
        .hand(0, &["Dark Ritual", "Test Deathtouch"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Dark Ritual", &[]);
    g.pass();
    g.pass();
    assert_eq!(g.player(0).pool.amt[engine::mana::B], 3);
    // The pool pays first: no mana source decision.
    let dt = g.card(0, "Test Deathtouch", Zone::Hand);
    g.choose(Choice::Cast(dt));
    assert_eq!(g.kind(), Some(DecisionKind::Priority));
    assert_eq!(g.player(0).pool.amt[engine::mana::B], 2);
    g.pass();
    g.pass(); // creature resolves
    g.pass();
    g.pass(); // main 1 ends
    assert_eq!(g.turn().step, Step::BeginCombat);
    assert_eq!(g.player(0).pool.total(), 0);
}

#[test]
fn cr_103_8a_starting_player_skips_the_first_draw() {
    let db = db();
    let deck: Vec<u16> = vec![db.id("Island").unwrap(); 40];
    let spec = GameSpec { mulligans: false, auto_single: false, ..GameSpec::default() };
    let mut g = Game::new(db, spec, [&deck, &deck], 5).unwrap();
    let start = g.turn().starting;
    assert_eq!(g.turn().number, 1);
    assert_eq!(g.turn().step, Step::Upkeep);
    g.pass();
    g.pass();
    assert_eq!(g.turn().step, Step::Draw);
    assert_eq!(g.player(start).hand.len(), 7);
    // Run to the other player's draw step: they draw (with a chance key).
    let mut keys = Vec::new();
    while !(g.turn().number == 2 && g.turn().step == Step::Draw) {
        let a = g.pending_decision().unwrap().options[0].action;
        g.apply(a, &mut keys).unwrap();
    }
    let other = 1 - start;
    assert_eq!(g.player(other).hand.len(), 8);
    let draws: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_DRAW).collect();
    assert_eq!(draws.len(), 1);
    assert_eq!(key_player(draws[0]), other);
}
