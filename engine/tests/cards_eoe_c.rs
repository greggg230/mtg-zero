//! Edge of Eternities lane C (ENGINE.md 99.4): void, spells cast this turn,
//! cost reductions, graveyard / exile / library / hand effects. Test names
//! cite the CR.

mod eoe_c_util;

use engine::db::Counter;
use engine::{Choice, DecisionKind, Step, Target, Zone};
use eoe_c_util::*;

// ---- Void ---------------------------------------------------------------------

/// Void (an ability word, CR 207.2c): "if a nonland permanent left the
/// battlefield this turn or a spell was warped this turn". Plasma Bolt deals
/// 2, then 3 once a creature died; void ends with the turn.
#[test]
fn void_plasma_bolt_deals_three_once_a_nonland_permanent_left_cr207_2c() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Plasma Bolt", "Plasma Bolt", "Plasma Bolt"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    assert!(!g.void_active());
    cast(&mut g, 0, "Plasma Bolt", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    assert_eq!(g.player(1).life, 18, "2 without void\n{}", log(&g));
    assert!(!g.void_active());
    let bear = bf(&g, 1, "Test Bear");
    let t = g.tgt(bear);
    cast(&mut g, 0, "Plasma Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert!(g.void_active(), "a nonland permanent left the battlefield");
    cast(&mut g, 0, "Plasma Bolt", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    assert_eq!(g.player(1).life, 15, "3 with void\n{}", log(&g));
    quiet_until(&mut g, |g| g.turn().number == 4 && at_priority(g));
    assert!(!g.void_active(), "this turn only");
}

/// A land leaving the battlefield is not void ("nonland"). Scavenger Grounds:
/// "{2}, {T}, Sacrifice a Desert: Exile all graveyards." (itself too: it is
/// in its owner's graveyard as the ability resolves, CR 602.2a, 601.2h).
#[test]
fn scavenger_grounds_exiles_all_graveyards_and_a_land_leaving_is_not_void_cr406() {
    let mut g = scen()
        .battlefield(0, &["Scavenger Grounds", "Mountain", "Mountain", "Mountain"])
        .graveyard(0, &["Island", "Plasma Bolt"])
        .graveyard(1, &["Forest", "Test Bear"])
        .hand(0, &["Plasma Bolt"])
        .at(Step::Main1, 0)
        .start();
    let sg = bf(&g, 0, "Scavenger Grounds");
    g.choose(act(sg, 1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.player(0).graveyard.len() + g.player(1).graveyard.len(), 0, "{}", log(&g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Exile), 1);
    assert_eq!(g.count_in(0, "Scavenger Grounds", Zone::Exile), 1, "the sacrificed Desert too");
    assert!(!g.void_active(), "a land left: not void");
    cast(&mut g, 0, "Plasma Bolt", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    assert_eq!(g.player(1).life, 18);
}

/// A spell warped this turn (cast for its warp cost, CR 702.185a) is void,
/// from the moment it is cast. Codecracker Hound: "look at the top two cards
/// of your library. Put one into your hand and the other into your
/// graveyard." Tragic Trajectory then gives -10/-10.
#[test]
fn void_a_warped_spell_codecracker_hound_and_tragic_trajectory_cr702_185a() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Swamp"])
        .battlefield(1, &["Test Giant"])
        .hand(0, &["Codecracker Hound", "Tragic Trajectory"])
        .library(0, &["Swamp", "Island"])
        .at(Step::Main1, 0)
        .start();
    let hound = g.card(0, "Codecracker Hound", Zone::Hand);
    g.choose(Choice::Cast(hound));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1)); // warp
    pay(&mut g);
    assert!(g.void_active(), "warped as it was cast\n{}", log(&g));
    let hand = g.player(0).hand.len();
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInHand));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2, "the top two: {:?}", g.options_desc());
    let pick = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .find(|o| match o.choice {
            Choice::Def(d) => g.db().def(d).name == "Island",
            Choice::Card(c) => g.name(c) == "Island",
            _ => false,
        })
        .unwrap_or_else(|| panic!("{:?}", g.options_desc()))
        .choice;
    g.choose(pick);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand + 1);
    assert_eq!(g.count_in(0, "Island", Zone::Hand), 1);
    assert_eq!(g.count_in(0, "Swamp", Zone::Graveyard), 1, "the other one");
    assert_eq!(g.player(0).library.len(), 12);
    let giant = bf(&g, 1, "Test Giant");
    let t = g.tgt(giant);
    cast(&mut g, 0, "Tragic Trajectory", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Graveyard, "-10/-10 with void\n{}", log(&g));
}

/// Without void, Tragic Trajectory gives -2/-2.
#[test]
fn tragic_trajectory_without_void_gives_minus_two() {
    let mut g = scen().battlefield(0, &["Swamp"]).battlefield(1, &["Test Giant"]).hand(0, &["Tragic Trajectory"]).at(Step::Main1, 0).start();
    let giant = bf(&g, 1, "Test Giant");
    let t = g.tgt(giant);
    cast(&mut g, 0, "Tragic Trajectory", &[t]);
    settle(&mut g);
    assert_eq!((g.power(giant), g.toughness(giant)), (4, 4));
}

/// Void triggered abilities: "At the beginning of your end step, if [void],
/// ..." (an intervening "if", CR 603.4): Kavaron Skywarden, Insatiable
/// Skittermaw (a +1/+1 counter), Voidforged Titan (draw a card, lose 1
/// life). Without void none triggers.
#[test]
fn void_end_step_triggers_only_with_void_cr603_4() {
    let names = ["Kavaron Skywarden", "Insatiable Skittermaw", "Voidforged Titan", "Mountain"];
    let mut g = scen().battlefield(0, &names).at(Step::Main2, 0).start();
    let hand = g.player(0).hand.len();
    to_end_step(&mut g);
    assert!(g.stack().is_empty() && at_priority(&g), "nothing triggers\n{}", log(&g));
    assert_eq!(g.counters_on(bf(&g, 0, "Kavaron Skywarden"), Counter::P1P1), 0);

    let mut g = scen().battlefield(0, &names).battlefield(1, &["Test Bear"]).hand(0, &["Plasma Bolt"]).at(Step::Main2, 0).start();
    let bear = bf(&g, 1, "Test Bear");
    let t = g.tgt(bear);
    cast(&mut g, 0, "Plasma Bolt", &[t]);
    settle(&mut g);
    let hand = g.player(0).hand.len().max(hand);
    to_end_step(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(bf(&g, 0, "Kavaron Skywarden"), Counter::P1P1), 1, "{}", log(&g));
    assert_eq!(g.counters_on(bf(&g, 0, "Insatiable Skittermaw"), Counter::P1P1), 1);
    assert_eq!(g.player(0).hand.len(), hand + 1, "Voidforged Titan draws");
    assert_eq!(g.player(0).life, 19);
}

/// Interceptor Mechan: "When this creature enters, return target artifact or
/// creature card from your graveyard to your hand." and its void ability.
#[test]
fn interceptor_mechan_returns_an_artifact_or_creature_card() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Mountain", "Mountain"])
        .graveyard(0, &["Test Bear", "Test Spacecraft", "Forest"])
        .hand(0, &["Interceptor Mechan"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Interceptor Mechan", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let opts = g.pending_decision().unwrap().options.len();
    assert_eq!(opts, 2, "the Bear and the artifact, not the land: {:?}", g.options_desc());
    let sc = g.card(0, "Test Spacecraft", Zone::Graveyard);
    let t = g.tgt(sc);
    g.choose(t);
    settle(&mut g);
    assert_eq!(g.obj(sc).zone, Zone::Hand);
}

/// Decode Transmissions: "You draw two cards and lose 2 life. Void - ...
/// instead you draw two cards and each opponent loses 2 life."
#[test]
fn decode_transmissions_both_ways() {
    let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp"]).hand(0, &["Decode Transmissions"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Decode Transmissions", &[]);
    settle(&mut g);
    assert_eq!((g.player(0).hand.len(), g.player(0).life, g.player(1).life), (2, 18, 20));

    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Mountain"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Decode Transmissions", "Plasma Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let t = g.tgt(bear);
    cast(&mut g, 0, "Plasma Bolt", &[t]);
    settle(&mut g);
    cast(&mut g, 0, "Decode Transmissions", &[]);
    settle(&mut g);
    assert_eq!((g.player(0).hand.len(), g.player(0).life, g.player(1).life), (2, 20, 18), "{}", log(&g));
}

/// Hymn of the Faller: "Surveil 1, then you draw a card and lose 1 life.
/// Void - ... draw another card."
#[test]
fn hymn_of_the_faller_draws_another_with_void() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Mountain"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Hymn of the Faller", "Plasma Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let t = g.tgt(bear);
    cast(&mut g, 0, "Plasma Bolt", &[t]);
    settle(&mut g);
    cast(&mut g, 0, "Hymn of the Faller", &[]);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 2, "two cards\n{}", log(&g));
    assert_eq!(g.player(0).life, 19);
}

/// Temporal Intervention: "Void - This spell costs {2} less to cast if ..."
/// (CR 601.2f). "Target opponent reveals their hand. You choose a nonland
/// card from it. That player discards that card."
#[test]
fn temporal_intervention_costs_two_less_with_void_cr601_2f() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Temporal Intervention", "Plasma Bolt"])
        .hand(1, &["Test Giant", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let ti = g.card(0, "Temporal Intervention", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(ti)), "{{2}}{{B}} without void");
    let bear = bf(&g, 1, "Test Bear");
    let t = g.tgt(bear);
    cast(&mut g, 0, "Plasma Bolt", &[t]);
    settle(&mut g);
    assert!(g.has_option(Choice::Cast(ti)), "{{B}} with void: {:?}", g.options_desc());
    cast(&mut g, 0, "Temporal Intervention", &[Choice::Target(Target::Player(1))]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseDiscard));
    assert_eq!(g.pending_decision().unwrap().options.len(), 1, "the nonland card only: {:?}", g.options_desc());
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Giant", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Forest", Zone::Hand), 1);
}

// ---- Spells cast this turn and cost reductions -----------------------------------

/// Gigastorm Titan: "This spell costs {3} less to cast if you've cast another
/// spell this turn" (CR 601.2f: the cost is determined before the spell
/// becomes cast, 601.2i).
#[test]
fn gigastorm_titan_costs_three_less_after_another_spell_cr601_2f() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Mountain"])
        .hand(0, &["Gigastorm Titan", "Plasma Bolt"])
        .at(Step::Main1, 0)
        .start();
    let titan = g.card(0, "Gigastorm Titan", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(titan)));
    cast(&mut g, 0, "Plasma Bolt", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    assert!(g.has_option(Choice::Cast(titan)), "{:?}", g.options_desc());
    cast(&mut g, 0, "Gigastorm Titan", &[]);
    settle(&mut g);
    assert_eq!(g.obj(titan).zone, Zone::Battlefield);
    assert_eq!(untapped(&g, 0), 0, "{{1}}{{U}}");
}

/// Uthros Psionicist: "The second spell you cast each turn costs {2} less to
/// cast." The first and the third pay in full.
#[test]
fn uthros_psionicist_reduces_the_second_spell_each_turn() {
    let mut g = scen()
        .battlefield(0, &["Uthros Psionicist", "Forest", "Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Test Bear", "Test Bear", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Test Bear", &[]);
    settle(&mut g);
    assert_eq!(untapped(&g, 0), 3);
    cast(&mut g, 0, "Test Bear", &[]);
    settle(&mut g);
    assert_eq!(untapped(&g, 0), 2, "the second: {{G}}");
    cast(&mut g, 0, "Test Bear", &[]);
    settle(&mut g);
    assert_eq!(untapped(&g, 0), 0, "the third: {{1}}{{G}}");
}

/// Lashwhip Predator: "This spell costs {2} less to cast if your opponents
/// control three or more creatures."
#[test]
fn lashwhip_predator_costs_two_less_against_three_creatures() {
    let lands = ["Forest", "Forest", "Forest", "Forest"];
    let g = scen().battlefield(0, &lands).battlefield(1, &["Test Bear", "Test Bear"]).hand(0, &["Lashwhip Predator"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Lashwhip Predator", Zone::Hand))));
    let g = scen()
        .battlefield(0, &lands)
        .battlefield(1, &["Test Bear", "Test Bear", "Test Bear"])
        .hand(0, &["Lashwhip Predator"])
        .at(Step::Main1, 0)
        .start();
    assert!(g.has_option(Choice::Cast(g.card(0, "Lashwhip Predator", Zone::Hand))), "{:?}", g.options_desc());
    // The opponent's creatures, not the caster's.
    let g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Test Bear", "Test Bear", "Test Bear"])
        .hand(0, &["Lashwhip Predator"])
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Lashwhip Predator", Zone::Hand))));
}

/// Sami, Wildcat Captain: "Spells you cast have affinity for artifacts. (They
/// cost {1} less to cast for each artifact you control.)" (CR 702.41a).
#[test]
fn sami_wildcat_captain_gives_spells_affinity_for_artifacts_cr702_41a() {
    let mut g = scen()
        .battlefield(0, &["Sami, Wildcat Captain", "Test Spacecraft", "Test Spacecraft", "Forest", "Forest", "Forest"])
        .battlefield(1, &["Test Spacecraft"])
        .hand(0, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Test Giant", &[]);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Giant", Zone::Battlefield), 1);
    assert_eq!(untapped(&g, 0), 0, "{{4}}{{G}} less two (the opponent's artifact doesn't count)");
}

/// Brightspear Zealot: "This creature gets +2/+0 as long as you've cast two
/// or more spells this turn."
#[test]
fn brightspear_zealot_gets_plus_two_after_two_spells() {
    let mut g = scen()
        .battlefield(0, &["Brightspear Zealot", "Mountain", "Mountain"])
        .hand(0, &["Plasma Bolt", "Plasma Bolt"])
        .at(Step::Main1, 0)
        .start();
    let z = bf(&g, 0, "Brightspear Zealot");
    assert_eq!(g.power(z), 2);
    cast(&mut g, 0, "Plasma Bolt", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    assert_eq!(g.power(z), 2);
    cast(&mut g, 0, "Plasma Bolt", &[Choice::Target(Target::Player(1))]);
    assert_eq!(g.power(z), 4, "cast (on the stack) counts");
    settle(&mut g);
    assert_eq!((g.power(z), g.toughness(z)), (4, 4));
}

/// Illvoi Infiltrator: "This creature can't be blocked if you've cast two or
/// more spells this turn."
#[test]
fn illvoi_infiltrator_cant_be_blocked_after_two_spells() {
    for spells in [1usize, 2] {
        let mut g = scen()
            .battlefield(0, &["Illvoi Infiltrator", "Mountain", "Mountain"])
            .battlefield(1, &["Test Bear"])
            .hand(0, &["Plasma Bolt", "Plasma Bolt"])
            .at(Step::Main1, 0)
            .start();
        for _ in 0..spells {
            cast(&mut g, 0, "Plasma Bolt", &[Choice::Target(Target::Player(1))]);
            settle(&mut g);
        }
        attack_with(&mut g, &["Illvoi Infiltrator"]);
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step >= Step::CombatDamage);
        let can_block = g.kind() == Some(DecisionKind::DeclareBlockers) && g.pending_decision().unwrap().options.len() > 1;
        assert_eq!(can_block, spells < 2, "{spells} spells: {:?}", g.options_desc());
    }
}

// ---- Graveyard, exile, library and hand ------------------------------------------

/// Cosmogoyf: "power is equal to the number of cards you own in exile and its
/// toughness is equal to that number plus 1" (CR 604.3, a characteristic-
/// defining ability).
#[test]
fn cosmogoyf_counts_the_cards_you_own_in_exile_cr604_3() {
    let g = scen()
        .battlefield(0, &["Cosmogoyf"])
        .exile(0, &["Island", "Plasma Bolt"])
        .exile(1, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let c = bf(&g, 0, "Cosmogoyf");
    assert_eq!((g.power(c), g.toughness(c)), (2, 3));
}

/// Chrome Companion: "Whenever this creature becomes tapped, you gain 1
/// life. {2}, {T}: Put target card from a graveyard on the bottom of its
/// owner's library."
#[test]
fn chrome_companion_puts_a_graveyard_card_on_the_bottom_of_its_owners_library_cr401_4() {
    let mut g = scen()
        .battlefield(0, &["Chrome Companion", "Island", "Island"])
        .graveyard(1, &["Test Bear", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let cc = bf(&g, 0, "Chrome Companion");
    g.choose(act(cc, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2, "any graveyard card: {:?}", g.options_desc());
    let bear = g.card(1, "Test Bear", Zone::Graveyard);
    let t = g.tgt(bear);
    g.choose(t);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Library);
    assert_eq!(g.player(1).library[0], bear, "the bottom");
    assert_eq!(g.player(0).life, 21, "it became tapped");
}

/// Seedship Broodtender: "When this creature enters, mill three cards. /
/// {3}{B}{G}, Sacrifice this creature: Return target creature or Spacecraft
/// card from your graveyard to the battlefield. Activate only as a sorcery."
#[test]
fn seedship_broodtender_returns_a_creature_or_spacecraft_card() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Forest", "Swamp", "Forest", "Swamp", "Forest", "Swamp"])
        .graveyard(0, &["Test Spacecraft", "Test Bear"])
        .hand(0, &["Seedship Broodtender"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Seedship Broodtender", &[]);
    settle(&mut g);
    assert_eq!(g.player(0).graveyard.len(), 5, "milled three");
    let sb = bf(&g, 0, "Seedship Broodtender");
    g.choose(act(sb, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2, "the Spacecraft and the Bear: {:?}", g.options_desc());
    let sc = g.card(0, "Test Spacecraft", Zone::Graveyard);
    let t = g.tgt(sc);
    g.choose(t);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(sc).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.count_in(0, "Seedship Broodtender", Zone::Graveyard), 1);
}

/// Space-Time Anomaly: "Target player mills cards equal to your life total."
#[test]
fn space_time_anomaly_mills_your_life_total() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Island", "Island"])
        .hand(0, &["Space-Time Anomaly"])
        .life(0, 5)
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Space-Time Anomaly", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    assert_eq!(g.player(1).graveyard.len(), 5);
    assert_eq!(g.player(1).library.len(), 7);
}

/// Starwinder: "Whenever a creature you control deals combat damage to a
/// player, you may draw that many cards." (a choice: CR 603.5).
#[test]
fn starwinder_may_draw_that_many_cards_cr603_5() {
    for yes in [true, false] {
        let mut g = scen().battlefield(0, &["Starwinder"]).at(Step::Main1, 0).start();
        attack_with(&mut g, &["Starwinder"]);
        g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
        assert_eq!(g.kind(), Some(DecisionKind::YesNo), "{}", log(&g));
        g.choose(if yes { Choice::Yes } else { Choice::Pass });
        settle(&mut g);
        assert_eq!(g.player(1).life, 13);
        assert_eq!(g.player(0).hand.len(), if yes { 7 } else { 0 });
    }
}

/// Virus Beetle: "When this creature enters, each opponent discards a card"
/// (of their choice, CR 701.9a).
#[test]
fn virus_beetle_makes_each_opponent_discard_cr701_9a() {
    let mut g = scen().battlefield(0, &["Swamp", "Swamp"]).hand(0, &["Virus Beetle"]).hand(1, &["Forest", "Test Bear"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Virus Beetle", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard) || (g.stack().is_empty() && at_priority(g)));
    assert_eq!(g.pending_decision().unwrap().player, 1, "the opponent chooses");
    settle(&mut g);
    assert_eq!(g.player(1).hand.len(), 1);
    assert_eq!(g.player(1).graveyard.len(), 1);
}

/// Warping Wail's first mode: "Exile target creature with power or toughness
/// 1 or less."
#[test]
fn warping_wail_exiles_a_creature_with_power_or_toughness_one_or_less() {
    let mut g = scen()
        .battlefield(0, &["Ancient Tomb"])
        .battlefield(1, &["Test Bear", "Chrome Companion"])
        .hand(0, &["Warping Wail"])
        .at(Step::Main1, 0)
        .start();
    let w = g.card(0, "Warping Wail", Zone::Hand);
    g.choose(Choice::Cast(w));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(g.pending_decision().unwrap().options.len(), 1, "toughness 1 only: {:?}", g.options_desc());
    let cc = bf(&g, 1, "Chrome Companion");
    let t = g.tgt(cc);
    g.choose(t);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(cc).zone, Zone::Exile);
}

// ---- Lane C features of cards that need other lanes (custom cards) --------------

/// Chorale of the Void's "At the beginning of your end step, sacrifice this
/// Aura unless [void]".
#[test]
fn sacrifice_unless_void_at_the_end_step() {
    let mut g = scen().battlefield(0, &["Test Void Altar"]).at(Step::Main2, 0).start();
    to_end_step(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Void Altar", Zone::Graveyard), 1, "no void: sacrificed");

    let mut g = scen().battlefield(0, &["Test Void Altar", "Mountain"]).battlefield(1, &["Test Bear"]).hand(0, &["Plasma Bolt"]).at(Step::Main2, 0).start();
    let bear = bf(&g, 1, "Test Bear");
    let t = g.tgt(bear);
    cast(&mut g, 0, "Plasma Bolt", &[t]);
    settle(&mut g);
    to_end_step(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Void Altar", Zone::Battlefield), 1, "void: kept\n{}", log(&g));
}

/// "Void - Whenever this attacks, if [void], ..." (Alpharael, Stonechosen;
/// a state predicate as an intervening "if", CR 603.4).
#[test]
fn void_state_predicate_as_an_intervening_if_cr603_4() {
    for void in [false, true] {
        let mut b = scen().battlefield(0, &["Test Void Raider", "Mountain"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0);
        if void {
            b = b.hand(0, &["Plasma Bolt"]);
        }
        let mut g = b.start();
        if void {
            let bear = bf(&g, 1, "Test Bear");
            let t = g.tgt(bear);
            cast(&mut g, 0, "Plasma Bolt", &[t]);
            settle(&mut g);
        }
        attack_with(&mut g, &["Test Void Raider"]);
        settle(&mut g);
        assert_eq!(g.player(0).hand.len(), void as usize, "void {void}\n{}", log(&g));
    }
}

/// Entropic Battlecruiser's "each opponent discards a card. Each opponent
/// who can't loses 3 life."
#[test]
fn each_opponent_discards_or_loses_life() {
    for cards in [0usize, 1] {
        let hand: Vec<&str> = vec!["Forest"; cards];
        let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp"]).hand(0, &["Test Tormentor"]).hand(1, &hand).at(Step::Main1, 0).start();
        cast(&mut g, 0, "Test Tormentor", &[]);
        settle(&mut g);
        assert_eq!(g.player(1).hand.len(), 0);
        assert_eq!(g.player(1).life, if cards == 0 { 17 } else { 20 }, "{cards} cards\n{}", log(&g));
    }
}

/// Specimen Freighter's "defending player mills four cards" (each opponent).
#[test]
fn each_opponent_mills() {
    let mut g = scen().battlefield(0, &["Island", "Island", "Island"]).hand(0, &["Test Miller"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Test Miller", &[]);
    settle(&mut g);
    assert_eq!(g.player(1).graveyard.len(), 4);
    assert_eq!(g.player(0).graveyard.len(), 0);
}

/// Infinite Guideline Station's "draw a card for each multicolored permanent
/// you control" (CR 105.2a).
#[test]
fn draw_a_card_per_multicolored_permanent_cr105_2a() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Sami, Wildcat Captain", "Test Bear"])
        .battlefield(1, &["Sami, Wildcat Captain"])
        .hand(0, &["Test Prism"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Test Prism", &[]);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 2, "the Prism and Sami\n{}", log(&g));
}

/// Hardlight Containment's "exile target creature an opponent controls until
/// this Aura leaves the battlefield" (CR 610.3).
#[test]
fn exile_target_creature_an_opponent_controls_until_this_leaves_cr610_3() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Test Bear"])
        .battlefield(1, &["Test Giant"])
        .hand(0, &["Test Containment"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Test Containment", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || (g.stack().is_empty() && at_priority(g)));
    let giant = bf(&g, 1, "Test Giant");
    if g.kind() == Some(DecisionKind::Target) {
        assert_eq!(g.pending_decision().unwrap().options.len(), 1, "the opponent's creature only: {:?}", g.options_desc());
        let t = g.tgt(giant);
        g.choose(t);
    }
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Exile, "{}", log(&g));
}

/// "Exile target creature or planeswalker" with its target spec.
#[test]
fn exile_target_creature_as_its_target_spec_says() {
    let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp"]).battlefield(1, &["Test Giant"]).hand(0, &["Test Exile"]).at(Step::Main1, 0).start();
    let giant = bf(&g, 1, "Test Giant");
    let t = g.tgt(giant);
    cast(&mut g, 0, "Test Exile", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Exile);
}

/// Mental Modulation's "This spell costs {1} less to cast during your turn."
#[test]
fn costs_less_during_your_turn_cr601_2f() {
    let g = scen().battlefield(0, &["Island"]).hand(0, &["Test Modulation"]).at(Step::Main1, 0).start();
    assert!(g.has_option(Choice::Cast(g.card(0, "Test Modulation", Zone::Hand))), "{{U}} in its controller's turn");
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &["Test Modulation"]).at(Step::Main1, 1).start();
    g.pass();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    assert!(!g.has_option(Choice::Cast(g.card(0, "Test Modulation", Zone::Hand))), "{{1}}{{U}} in the opponent's turn");
}

/// Starport Security's "This ability costs {2} less to activate if you
/// control a <permanent>" (CR 602.2b, 601.2f), and Thaumaton Torpedo's "...
/// {3} less to activate if you attacked with a <creature> this turn".
#[test]
fn activated_ability_costs_less_if_you_control_or_attacked_with_cr602_2b() {
    let g = scen().battlefield(0, &["Test Beacon", "Island"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Test Beacon"), 0)));
    let g = scen().battlefield(0, &["Test Beacon", "Island", "Test Giant"]).at(Step::Main1, 0).start();
    assert!(g.has_option(act(bf(&g, 0, "Test Beacon"), 0)), "{{1}} with a Giant");

    let mut g = scen().battlefield(0, &["Test Torpedo", "Island", "Test Giant"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Test Torpedo"), 0)));
    attack_with(&mut g, &["Test Giant"]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(g.has_option(act(bf(&g, 0, "Test Torpedo"), 0)), "{{1}} after attacking with a Giant: {:?}", g.options_desc());
}

/// All-Fates Scroll: "{7}, {T}, Sacrifice this artifact: Draw X cards, where
/// X is the number of differently named lands you control" (CR 201.2).
#[test]
fn all_fates_scroll_draws_per_differently_named_land_cr201_2() {
    let mut g = scen()
        .battlefield(0, &["All-Fates Scroll", "Island", "Island", "Island", "Forest", "Forest", "Mountain", "Mountain", "Plains"])
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, "All-Fates Scroll");
    g.choose(act(s, 1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 4, "Island, Forest, Mountain, Plains\n{}", log(&g));
    assert_eq!(g.count_in(0, "All-Fates Scroll", Zone::Graveyard), 1);
}

/// Memorial Vault: "{T}, Sacrifice another artifact: Exile the top X cards of
/// your library, where X is one plus the mana value of the sacrificed
/// artifact. You may play those cards this turn."
#[test]
fn memorial_vault_exiles_one_plus_the_sacrificed_mana_value_playable_this_turn() {
    let mut g = scen()
        .battlefield(0, &["Memorial Vault", "Test Spacecraft"])
        .library(0, &["Mountain", "Plasma Bolt", "Island", "Swamp", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let v = bf(&g, 0, "Memorial Vault");
    g.choose(act(v, 0));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Spacecraft", Zone::Graveyard), 1);
    assert_eq!(g.player(0).library.len(), 13, "1 + 3 cards exiled\n{}", log(&g));
    let m = g.card(0, "Mountain", Zone::Exile);
    assert!(g.has_option(Choice::Play(m)), "play it this turn: {:?}", g.options_desc());
    g.choose(Choice::Play(m));
    let bolt = g.card(0, "Plasma Bolt", Zone::Exile);
    assert!(g.has_option(Choice::Cast(bolt)), "{:?}", g.options_desc());
    assert_eq!(g.count_in(0, "Forest", Zone::Exile), 0, "the fifth card stays");
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.has_option(Choice::Cast(bolt)), "this turn only");
}

/// Lightstall Inquisitor: "When this creature enters, each opponent exiles a
/// card from their hand and may play that card for as long as it remains
/// exiled. Each spell cast this way costs {1} more to cast. Each land played
/// this way enters tapped." (The opponent chooses: their own hand.)
#[test]
fn lightstall_inquisitor_exiles_an_opponents_card_they_may_play_taxed_cr601_2f() {
    let mut g = scen()
        .battlefield(0, &["Plains"])
        .battlefield(1, &["Forest", "Forest"])
        .hand(0, &["Lightstall Inquisitor"])
        .hand(1, &["Test Bear", "Forest"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    cast(&mut g, 0, "Lightstall Inquisitor", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseDiscard));
    assert_eq!(g.pending_decision().unwrap().player, 1, "the opponent chooses\n{}", log(&g));
    let bear = g.card(1, "Test Bear", Zone::Hand);
    g.choose(Choice::Card(bear));
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    quiet_until(&mut g, |g| g.turn().number == 4 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.has_option(Choice::Cast(bear)), "{{1}}{{G}} plus {{1}}: three mana, two Forests");
    let f = g.card(1, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    assert!(g.has_option(Choice::Cast(bear)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(untapped(&g, 1), 0);

    // A land played this way enters tapped.
    let mut g = scen()
        .battlefield(0, &["Plains"])
        .hand(0, &["Lightstall Inquisitor"])
        .hand(1, &["Forest"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    cast(&mut g, 0, "Lightstall Inquisitor", &[]);
    settle(&mut g);
    let f = g.card(1, "Forest", Zone::Exile);
    quiet_until(&mut g, |g| g.turn().number == 4 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(Choice::Play(f)), "{:?}", g.options_desc());
    g.choose(Choice::Play(f));
    assert!(g.obj(f).tapped, "enters tapped\n{}", log(&g));
}

/// Deafening Silence: "Each player can't cast more than one noncreature
/// spell each turn." Creature spells are not limited.
#[test]
fn deafening_silence_one_noncreature_spell_each_turn() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Forest", "Forest"])
        .battlefield(1, &["Deafening Silence"])
        .hand(0, &["Plasma Bolt", "Plasma Bolt", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Plasma Bolt", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    let bolt = g.card(0, "Plasma Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(bolt)), "a second noncreature spell: {:?}", g.options_desc());
    let bear = g.card(0, "Test Bear", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bear)), "a creature spell");
    cast(&mut g, 0, "Test Bear", &[]);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(Choice::Cast(bolt)), "a new turn");
}

/// Weftwalking: "The first spell each player casts during each of their
/// turns may be cast without paying its mana cost" (an alternative cost,
/// CR 118.9: a choice).
#[test]
fn weftwalking_the_first_spell_each_turn_may_be_cast_free_cr118_9() {
    let mut g = scen()
        .battlefield(0, &["Weftwalking", "Forest", "Forest"])
        .hand(0, &["Test Giant", "Test Bear", "Test Giant"])
        .hand(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let giant = g.card(0, "Test Giant", Zone::Hand);
    assert!(g.has_option(Choice::Cast(giant)), "free: {:?}", g.options_desc());
    g.choose(Choice::Cast(giant));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost), "{}", log(&g));
    g.choose(Choice::Number((engine::act::NUMBER_N - 1) as u8));
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert_eq!(untapped(&g, 0), 2, "without paying its mana cost");
    let giant2 = g.card(0, "Test Giant", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(giant2)), "the second spell pays");
    // The first spell may be paid for normally too: a choice.
    let bear = g.card(0, "Test Bear", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bear)));
    // Each player, during their own turn.
    quiet_until(&mut g, |g| g.turn().number == 4 && g.turn().step == Step::Main1 && at_priority(g));
    let b1 = g.card(1, "Test Bear", Zone::Hand);
    assert!(g.has_option(Choice::Cast(b1)), "the opponent's first spell in their turn: {:?}", g.options_desc());
}

/// Weftwalking: "When this enchantment enters, if you cast it, shuffle your
/// hand and graveyard into your library, then draw seven cards."
#[test]
fn weftwalking_shuffles_hand_and_graveyard_then_draws_seven() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &["Weftwalking", "Test Bear", "Test Bear"])
        .graveyard(0, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Weftwalking", &[]);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 7, "{}", log(&g));
    assert_eq!(g.player(0).graveyard.len(), 0);
    assert_eq!(g.player(0).library.len(), 12 + 3 - 7);
}

/// Mystifying Maze's "Exile target attacking creature an opponent controls.
/// At the beginning of the next end step, return it to the battlefield
/// tapped under its owner's control" (CR 603.7).
#[test]
fn exile_an_attacking_creature_until_the_next_end_step_cr603_7() {
    let mut g = scen().battlefield(0, &["Test Giant"]).battlefield(1, &["Test Maze", "Island"]).at(Step::Main1, 0).start();
    attack_with(&mut g, &["Test Giant"]);
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 1);
    let maze = bf(&g, 1, "Test Maze");
    let giant = bf(&g, 0, "Test Giant");
    g.choose(act(maze, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let t = g.tgt(giant);
    g.choose(t);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Exile, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::Cleanup || g.turn().number > 3);
    let giant = g.card(0, "Test Giant", Zone::Battlefield);
    assert!(g.obj(giant).tapped, "returned tapped\n{}", log(&g));
    assert_eq!(g.player(1).life, 20);
}

/// Plaza of Heroes' "{3}, {T}, Exile this land: ..." (an exile-this cost of a
/// permanent, CR 602.2b, 118.3).
#[test]
fn exile_this_permanent_as_a_cost_cr118_3() {
    let mut g = scen().battlefield(0, &["Test Escape Pod", "Island"]).at(Step::Main1, 0).start();
    let pod = bf(&g, 0, "Test Escape Pod");
    g.choose(act(pod, 0));
    pay(&mut g);
    assert_eq!(g.obj(pod).zone, Zone::Exile, "paid before the ability resolves");
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1);
}

/// Sliver Overlord's "{3}: Search your library for a Sliver card, reveal
/// that card, put it into your hand, then shuffle."
#[test]
fn search_for_a_sliver_card_cr701_19() {
    let mut g = scen().battlefield(0, &["Test Hive Seeker", "Island"]).library(0, &["Test Sliver", "Test Bear"]).at(Step::Main1, 0).start();
    let s = bf(&g, 0, "Test Hive Seeker");
    g.choose(act(s, 0));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    let opts = g.options_desc();
    assert!(opts.iter().any(|o| o.contains("Test Sliver")) && !opts.iter().any(|o| o.contains("Test Bear")), "{opts:?}");
    let pick = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
    g.choose(pick);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Sliver", Zone::Hand), 1);
}

/// Entropic Battlecruiser's "Whenever an opponent discards a card, ..." (the
/// IR's `discards_card` event: its one subject is the discarding player).
#[test]
fn whenever_an_opponent_discards_a_card() {
    let mut g = scen()
        .battlefield(0, &["Test Discard Watcher", "Swamp", "Swamp"])
        .hand(0, &["Virus Beetle"])
        .hand(1, &["Forest", "Forest"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Virus Beetle", &[]);
    settle(&mut g);
    assert_eq!(g.player(1).hand.len(), 1);
    assert_eq!(g.player(0).life, 22, "{}", log(&g));
}

/// Timeline Culler: "Haste / You may cast this card from your graveyard
/// using its warp ability. / Warp-{B}, Pay 2 life." (CR 702.185a, 119.4).
/// From hand: its mana cost or its warp cost (a choice).
#[test]
fn timeline_culler_warps_from_hand_paying_two_life_cr702_185a() {
    let mut g = scen().battlefield(0, &["Swamp", "Swamp"]).hand(0, &["Timeline Culler"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Timeline Culler", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2, "{:?}", g.options_desc());
    g.choose(Choice::Number(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    assert_eq!(g.player(0).life, 18);
    assert_eq!(untapped(&g, 0), 1, "{{B}}");
    assert!(g.void_active());
    quiet_until(&mut g, |g| g.turn().number == 4 && at_priority(g));
    assert_eq!(g.obj(c).zone, Zone::Exile, "warp: exiled at the end step\n{}", log(&g));
}

/// From its owner's graveyard: for its warp cost only (no choice of its
/// mana cost there), then exiled at the end step like any warped
/// permanent.
#[test]
fn timeline_culler_is_cast_from_the_graveyard_for_its_warp_cost_cr702_185a() {
    let mut g = scen().battlefield(0, &["Swamp", "Swamp"]).graveyard(0, &["Timeline Culler"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Timeline Culler", Zone::Graveyard);
    let opt = g.pending_decision().unwrap().options.iter().find(|o| matches!(o.choice, Choice::Cast(x) if x == c)).map(|o| o.choice);
    let cast_it = opt.unwrap_or_else(|| panic!("castable from the graveyard: {:?}", g.options_desc()));
    g.choose(cast_it);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.player(0).life, 18);
    assert_eq!(untapped(&g, 0), 1, "{{B}}, not {{B}}{{B}}");
    quiet_until(&mut g, |g| g.turn().number == 4 && at_priority(g));
    assert_eq!(g.obj(c).zone, Zone::Exile, "{}", log(&g));

    // With 1 life its warp cost can't be paid (CR 119.4), and its mana cost
    // is no way to cast it from a graveyard.
    let g = scen().battlefield(0, &["Swamp", "Swamp"]).graveyard(0, &["Timeline Culler"]).life(0, 1).at(Step::Main1, 0).start();
    let c = g.card(0, "Timeline Culler", Zone::Graveyard);
    assert!(!g.pending_decision().unwrap().options.iter().any(|o| matches!(o.choice, Choice::Cast(x) if x == c)), "{:?}", g.options_desc());
}

/// Dark Endurance: "This spell costs {1} less to cast if it targets a
/// blocking creature. Target creature gets +2/+0 and gains indestructible
/// until end of turn." The targets are chosen before the cost is determined
/// (CR 601.2c, 601.2f): with one Swamp, only a blocking creature can be its
/// target.
#[test]
fn dark_endurance_costs_one_less_targeting_a_blocking_creature_cr601_2f() {
    let mut g = scen()
        .battlefield(0, &["Test Giant", "Test Bear", "Swamp"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Dark Endurance"])
        .at(Step::Main1, 1)
        .start();
    let de = g.card(0, "Dark Endurance", Zone::Hand);
    // P1 attacks with its Bear; P0's Giant blocks.
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers) || (at_priority(g) && g.pending_decision().unwrap().player == 0));
    if at_priority(&g) {
        assert!(!g.has_option(Choice::Cast(de)), "{{1}}{{B}} with one Swamp and no blocker");
        g.pass();
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    }
    let attacker = g.card(1, "Test Bear", Zone::Battlefield);
    g.choose(Choice::Card(attacker));
    g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    let giant = bf(&g, 0, "Test Giant");
    for _ in 0..4 {
        if g.kind() != Some(DecisionKind::DeclareBlockers) {
            break;
        }
        // The Giant blocks; the Bear doesn't (blockers decide in order).
        let c = if g.obj(giant).blocking.is_none() { Choice::Card(attacker) } else { Choice::Pass };
        g.choose(c);
    }
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0 && g.turn().step == Step::DeclareBlockers);
    assert!(g.obj(giant).blocking.is_some(), "{}", log(&g));
    assert!(g.has_option(Choice::Cast(de)), "{{B}} targeting the blocker: {:?}", g.options_desc());
    g.choose(Choice::Cast(de));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(g.pending_decision().unwrap().options.len(), 1, "only the blocking creature keeps it payable: {:?}", g.options_desc());
    let t = g.tgt(giant);
    g.choose(t);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.power(giant), 8, "{}", log(&g));
    assert!(g.has_kw(giant, engine::db::Kw::Indestructible));
}

/// Cast from a graveyard "using its warp ability", Timeline Culler is cast
/// for its warp cost: Weftwalking's "without paying its mana cost" is another
/// alternative cost, and only one applies (CR 118.9a).
#[test]
fn timeline_culler_from_the_graveyard_is_never_free_cr118_9a() {
    let g = scen().battlefield(0, &["Weftwalking"]).graveyard(0, &["Timeline Culler"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Timeline Culler", Zone::Graveyard);
    assert!(!g.pending_decision().unwrap().options.iter().any(|o| matches!(o.choice, Choice::Cast(x) if x == c)), "{:?}", g.options_desc());
}
