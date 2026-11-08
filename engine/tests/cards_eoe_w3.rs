//! Workstream W3 of the Edge of Eternities port (ENGINE.md 99.13): combat
//! and abilities, each card against its Oracle text. Test names cite the CR.

mod eoe_w3_kit;

use engine::db::{Counter, Kw};
use engine::{Choice, DecisionKind, Step, Zone};
use eoe_w3_kit::*;

// ---- Rust Harvester -----------------------------------------------------------------

/// Rust Harvester [1/1, menace]: "{2}, {T}, Exile an artifact card from your
/// graveyard: Put a +1/+1 counter on this creature, then it deals damage
/// equal to its power to any target" (CR 118.3, 602.2b: the card is chosen
/// as the cost is paid; only artifact cards).
#[test]
fn rust_harvester_exiles_an_artifact_card_from_your_graveyard_as_a_cost_cr602_2b() {
    let mut g = scen()
        .battlefield(0, &["Rust Harvester", "Mountain", "Mountain"])
        .graveyard(0, &["W3 Bear", "W3 Trinket", "W3 Robot", "Island"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let h = bf(&g, 0, "Rust Harvester");
    assert!(g.has_kw(h, Kw::Menace));
    let (bear, trinket, robot) =
        (g.card(0, "W3 Bear", Zone::Graveyard), g.card(0, "W3 Trinket", Zone::Graveyard), g.card(0, "W3 Robot", Zone::Graveyard));
    g.choose(act(h, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseGraveyardCard));
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert!(opts.contains(&Choice::Card(trinket)) && opts.contains(&Choice::Card(robot)), "artifact cards: {opts:?}");
    assert!(!opts.contains(&Choice::Card(bear)), "not a creature card that isn't an artifact");
    assert_eq!(opts.len(), 2);
    g.choose(Choice::Card(robot));
    settle(&mut g);
    assert_eq!(g.obj(robot).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.obj(trinket).zone, Zone::Graveyard);
    assert_eq!(g.counters_on(h, Counter::P1P1), 1);
    assert_eq!(g.life(1), 18, "2 damage: its power after the counter");
    assert!(g.obj(h).tapped);
}

/// Without an artifact card in its controller's graveyard the ability
/// can't be activated (CR 602.2b, 118.3); a token is no card (108.2).
#[test]
fn rust_harvester_needs_an_artifact_card_in_your_graveyard_cr118_3() {
    let g = scen()
        .battlefield(0, &["Rust Harvester", "Mountain", "Mountain"])
        .graveyard(0, &["W3 Bear", "Island"])
        .graveyard(1, &["W3 Trinket"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let h = bf(&g, 0, "Rust Harvester");
    assert!(!g.has_option(act(h, 0)), "no artifact card in its controller's graveyard");
}

// ---- Sunstar Chaplain ---------------------------------------------------------------

/// Sunstar Chaplain [3/2]: "{2}, Remove a +1/+1 counter from a creature you
/// control: Tap target artifact or creature" (CR 118.3, 602.2b: any creature
/// you control with such a counter, chosen as the cost is paid).
#[test]
fn sunstar_chaplain_removes_a_counter_from_a_creature_you_control_cr118_3() {
    let mut g = scen()
        .battlefield(0, &["Sunstar Chaplain", "W3 Bear", "W3 Giant", "Plains", "Plains"])
        .battlefield(1, &["W3 Trinket", "W3 Wall"])
        .counters(0, "W3 Bear", Counter::P1P1, 2)
        .counters(0, "W3 Giant", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let (ch, bear, giant) = (bf(&g, 0, "Sunstar Chaplain"), bf(&g, 0, "W3 Bear"), bf(&g, 0, "W3 Giant"));
    let (trinket, wall) = (bf(&g, 1, "W3 Trinket"), bf(&g, 1, "W3 Wall"));
    assert_eq!((g.power(ch), g.toughness(ch)), (3, 2));
    g.choose(act(ch, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(trinket)) && g.has_option(g.tgt(wall)), "an artifact or a creature");
    g.choose(g.tgt(trinket));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(opts, vec![Choice::Card(bear), Choice::Card(giant)], "creatures you control with a +1/+1 counter");
    g.choose(Choice::Card(giant));
    settle(&mut g);
    assert!(g.obj(trinket).tapped, "{}", log(&g));
    assert_eq!(g.counters_on(giant, Counter::P1P1), 0);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 2);
    assert!(!g.obj(ch).tapped, "no {{T}} in the cost");
}

/// No creature of yours with a +1/+1 counter: the cost can't be paid (an
/// opponent's creature's counters don't count).
#[test]
fn sunstar_chaplain_needs_a_counter_on_a_creature_you_control_cr602_2b() {
    let g = scen()
        .battlefield(0, &["Sunstar Chaplain", "W3 Bear", "Plains", "Plains"])
        .battlefield(1, &["W3 Giant"])
        .counters(1, "W3 Giant", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let ch = bf(&g, 0, "Sunstar Chaplain");
    assert!(!g.has_option(act(ch, 0)));
}

/// "At the beginning of your end step, if you control two or more tapped
/// creatures, put a +1/+1 counter on target creature you control" (CR
/// 603.4: an intervening "if", checked as it triggers).
#[test]
fn sunstar_chaplain_end_step_counter_needs_two_tapped_creatures_cr603_4() {
    for tapped in [1usize, 2] {
        let mut g = scen().battlefield(0, &["Sunstar Chaplain", "W3 Bear", "W3 Giant"]).at(Step::Main1, 0).start();
        let (ch, bear, giant) = (bf(&g, 0, "Sunstar Chaplain"), bf(&g, 0, "W3 Bear"), bf(&g, 0, "W3 Giant"));
        g.set_tapped(bear, true);
        if tapped == 2 {
            g.set_tapped(giant, true);
        }
        to_end_step(&mut g);
        if tapped == 2 {
            g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
            g.choose(g.tgt(ch));
            settle(&mut g);
            assert_eq!(g.counters_on(ch, Counter::P1P1), 1, "{}", log(&g));
        } else {
            settle(&mut g);
            assert_eq!(g.counters_on(ch, Counter::P1P1), 0, "one tapped creature: no trigger\n{}", log(&g));
        }
    }
}

// ---- Frenzied Baloth ----------------------------------------------------------------

/// Frenzied Baloth [3/2, trample, haste]: "This spell can't be countered"
/// and "Creature spells you control can't be countered" (CR 101.2: a
/// counterspell resolves and does nothing to them); a noncreature spell of
/// its controller still can be.
#[test]
fn frenzied_baloth_your_creature_spells_cant_be_countered_cr101_2() {
    let mut g = scen()
        .battlefield(0, &["Frenzied Baloth", "Forest", "Forest", "Forest"])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &["W3 Bear", "W3 Growth"])
        .hand(1, &["W3 Cancel", "W3 Cancel"])
        .at(Step::Main1, 0)
        .start();
    let baloth = bf(&g, 0, "Frenzied Baloth");
    assert!(g.has_kw(baloth, Kw::Trample) && g.has_kw(baloth, Kw::Haste));
    let bear = cast(&mut g, 0, "W3 Bear", &[]);
    to_opponent_priority(&mut g);
    let t = Choice::Target(engine::Target::Obj(g.obj_ref(bear)));
    cast(&mut g, 1, "W3 Cancel", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "a creature spell: not countered\n{}", log(&g));
    // A noncreature spell is countered.
    let tb = g.tgt(baloth);
    let growth = cast(&mut g, 0, "W3 Growth", &[tb]);
    to_opponent_priority(&mut g);
    let t = Choice::Target(engine::Target::Obj(g.obj_ref(growth)));
    cast(&mut g, 1, "W3 Cancel", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(growth).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.power(baloth), 3, "countered: no +3/+3");
}

/// Frenzied Baloth's own spell can't be countered either (CR 101.2).
#[test]
fn frenzied_baloth_this_spell_cant_be_countered_cr101_2() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .battlefield(1, &["Island"])
        .hand(0, &["Frenzied Baloth"])
        .hand(1, &["W3 Cancel"])
        .at(Step::Main1, 0)
        .start();
    let baloth = cast(&mut g, 0, "Frenzied Baloth", &[]);
    to_opponent_priority(&mut g);
    let t = Choice::Target(engine::Target::Obj(g.obj_ref(baloth)));
    cast(&mut g, 1, "W3 Cancel", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(baloth).zone, Zone::Battlefield, "{}", log(&g));
}

/// "Combat damage can't be prevented" (CR 615.12): a "prevent all combat
/// damage" effect prevents none, of any creature (the Baloth's controller's
/// and the opponent's alike).
#[test]
fn frenzied_baloth_combat_damage_cant_be_prevented_cr615_12() {
    for with_baloth in [true, false] {
        let mut bf0 = vec!["W3 Bear"];
        if with_baloth {
            bf0.push("Frenzied Baloth");
        }
        let mut g = scen().battlefield(0, &bf0).battlefield(1, &["Forest"]).hand(1, &["W3 Fog"]).at(Step::Main1, 0).turn_number(3).start();
        attack_with(&mut g, 0, &["W3 Bear"]);
        g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
        to_opponent_priority(&mut g);
        cast(&mut g, 1, "W3 Fog", &[]);
        settle(&mut g);
        quiet_until(&mut g, |g| g.turn().step == Step::Main2);
        let expect = if with_baloth { 18 } else { 20 };
        assert_eq!(g.life(1), expect, "with the Baloth: {with_baloth}\n{}", log(&g));
    }
}

// ---- Swarmyard (regeneration) -------------------------------------------------------

/// Regenerate `target` with Swarmyard and resolve it.
fn swarmyard_regenerates(g: &mut engine::Game, target: engine::CardIdx) {
    let yard = bf(g, 0, "Swarmyard");
    g.choose(act(yard, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let t = g.tgt(target);
    g.choose(t);
    settle(g);
}

/// Swarmyard: "{T}: Add {C}." and "{T}: Regenerate target Insect, Rat,
/// Spider, or Squirrel." (CR 701.19a: the next time it would be destroyed
/// this turn, instead its damage is removed and its controller taps it.)
/// Lethal damage (704.5g "destroy") is replaced; the creature stays, tapped,
/// with no damage. A Bear is no legal target.
#[test]
fn swarmyard_regenerates_a_rat_from_lethal_damage_cr701_19a() {
    let mut g = scen()
        .battlefield(0, &["Swarmyard", "W3 Rat", "W3 Bear"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["W3 Bolt"])
        .at(Step::Main1, 0)
        .start();
    let (rat, bear, yard) = (bf(&g, 0, "W3 Rat"), bf(&g, 0, "W3 Bear"), bf(&g, 0, "Swarmyard"));
    g.choose(act(yard, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(rat)) && !g.has_option(g.tgt(bear)), "{:?}", g.options_desc());
    g.choose(g.tgt(rat));
    settle(&mut g);
    assert!(!g.obj(rat).tapped);
    to_opponent_priority(&mut g);
    let t = g.tgt(rat);
    cast(&mut g, 1, "W3 Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(rat).zone, Zone::Battlefield, "regenerated\n{}", log(&g));
    assert!(g.obj(rat).tapped, "its controller taps it");
    assert_eq!(g.obj(rat).damage, 0, "its damage removed");
}

/// One shield, one destruction (CR 701.19a: "the next time"): the second
/// destruction that turn destroys it; a destroy spell is replaced like
/// lethal damage.
#[test]
fn swarmyard_a_shield_replaces_one_destruction_cr701_19a() {
    let mut g = scen()
        .battlefield(0, &["Swarmyard", "W3 Rat"])
        .battlefield(1, &["Swamp", "Swamp"])
        .hand(1, &["W3 Murder", "W3 Murder"])
        .at(Step::Main1, 0)
        .start();
    let rat = bf(&g, 0, "W3 Rat");
    swarmyard_regenerates(&mut g, rat);
    to_opponent_priority(&mut g);
    let t = g.tgt(rat);
    cast(&mut g, 1, "W3 Murder", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(rat).zone, Zone::Battlefield, "{}", log(&g));
    to_opponent_priority(&mut g);
    let t = g.tgt(rat);
    cast(&mut g, 1, "W3 Murder", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(rat).zone, Zone::Graveyard, "the shield was used up\n{}", log(&g));
}

/// The shield lasts this turn only (CR 701.19a, 514.2: it ends in the
/// cleanup step).
#[test]
fn swarmyard_the_shield_ends_with_the_turn_cr514_2() {
    let mut g = scen()
        .battlefield(0, &["Swarmyard", "W3 Rat"])
        .battlefield(1, &["Swamp"])
        .hand(1, &["W3 Murder"])
        .at(Step::Main1, 0)
        .start();
    let rat = bf(&g, 0, "W3 Rat");
    swarmyard_regenerates(&mut g, rat);
    next_main(&mut g, 1);
    let t = g.tgt(rat);
    cast(&mut g, 1, "W3 Murder", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(rat).zone, Zone::Graveyard, "{}", log(&g));
}

/// "Destroy all creatures. They can't be regenerated" (CR 701.19c): the
/// shield doesn't apply; "destroy all creatures" alone is replaced.
#[test]
fn swarmyard_cant_be_regenerated_ignores_the_shield_cr701_19c() {
    for (spell, dies) in [("W3 Wrath", true), ("W3 Sweep", false)] {
        // (A sorcery: the shield made at instant speed in P1's main phase.)
        let mut g = scen()
            .battlefield(0, &["Swarmyard", "W3 Rat"])
            .battlefield(1, &["Swamp", "Swamp"])
            .hand(1, &[spell])
            .at(Step::Main1, 1)
            .start();
        let rat = bf(&g, 0, "W3 Rat");
        g.pass();
        g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
        swarmyard_regenerates(&mut g, rat);
        assert_eq!(g.turn().active, 1);
        cast(&mut g, 1, spell, &[]);
        settle(&mut g);
        assert_eq!(g.obj(rat).zone == Zone::Graveyard, dies, "{spell}
{}", log(&g));
    }
}

/// A regenerated attacking creature is removed from combat (CR 701.19a,
/// 506.4): it deals no combat damage.
#[test]
fn swarmyard_a_regenerated_attacker_is_removed_from_combat_cr506_4() {
    let mut g = scen()
        .battlefield(0, &["Swarmyard", "W3 Rat"])
        .battlefield(1, &["Swamp"])
        .hand(1, &["W3 Murder"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let rat = bf(&g, 0, "W3 Rat");
    attack_with(&mut g, 0, &["W3 Rat"]);
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    swarmyard_regenerates(&mut g, rat);
    assert!(g.obj(rat).attacking);
    to_opponent_priority(&mut g);
    let t = g.tgt(rat);
    cast(&mut g, 1, "W3 Murder", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(rat).zone, Zone::Battlefield, "{}", log(&g));
    assert!(!g.obj(rat).attacking, "removed from combat");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 20, "no combat damage\n{}", log(&g));
}
