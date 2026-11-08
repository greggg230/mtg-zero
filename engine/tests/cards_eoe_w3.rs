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

// ---- Requiem Monolith (a granted triggered ability) ---------------------------------

/// Activate Requiem Monolith targeting `t` and answer the damage question
/// with `yes` (its asker must be `asker`).
fn monolith(g: &mut engine::Game, t: engine::CardIdx, asker: engine::PlayerId, yes: bool) {
    let m = bf(g, 0, "Requiem Monolith");
    g.choose(act(m, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let c = g.tgt(t);
    g.choose(c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo) || (at_priority(g) && g.stack().is_empty()) || at_priority(g) && g.pending_decision().unwrap().player == 1);
    resolve_with(g, |g| {
        if g.kind() == Some(DecisionKind::YesNo) {
            assert_eq!(g.pending_decision().unwrap().player, asker, "the creature's controller decides");
            Some(if yes { Choice::Yes } else { Choice::Pass })
        } else {
            None
        }
    });
}

/// Requiem Monolith: "{T}: Until end of turn, target creature gains
/// 'Whenever this creature is dealt damage, you draw that many cards and
/// lose that much life.' That creature's controller may have this artifact
/// deal 1 damage to it. Activate only as a sorcery." (CR 613.1f: a granted
/// triggered ability; 608.2d: the creature's controller chooses; "you" is
/// the creature's controller, the ability's, 113.8.)
#[test]
fn requiem_monolith_grants_draw_and_lose_life_on_damage_cr613_1f() {
    for yes in [true, false] {
        let mut g = scen().battlefield(0, &["Requiem Monolith", "W3 Giant"]).at(Step::Main1, 0).start();
        let giant = bf(&g, 0, "W3 Giant");
        let hand = g.player(0).hand.len();
        monolith(&mut g, giant, 0, yes);
        let n = yes as usize;
        assert_eq!(g.obj(giant).damage, n as u32, "{}", log(&g));
        assert_eq!(g.player(0).hand.len(), hand + n, "draws that many\n{}", log(&g));
        assert_eq!(g.life(0), 20 - n as i32, "loses that much");
    }
}

/// The opponent's creature: its controller decides and draws (CR 113.8:
/// the granted ability is the creature's, controlled by its controller).
/// Later damage that turn triggers it again, with that amount; the next
/// turn the ability is gone (514.2).
#[test]
fn requiem_monolith_the_creatures_controller_draws_until_end_of_turn_cr514_2() {
    let mut g = scen()
        .battlefield(0, &["Requiem Monolith", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["W3 Giant"])
        .hand(0, &["W3 Blast", "W3 Blast"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 1, "W3 Giant");
    let hand1 = g.player(1).hand.len();
    monolith(&mut g, giant, 1, true);
    assert_eq!(g.player(1).hand.len(), hand1 + 1, "{}", log(&g));
    assert_eq!(g.life(1), 19);
    let t = g.tgt(giant);
    cast(&mut g, 0, "W3 Blast", &[t]);
    settle(&mut g);
    assert_eq!(g.player(1).hand.len(), hand1 + 4, "3 more\n{}", log(&g));
    assert_eq!(g.life(1), 16);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "4 damage to a 5/5");
    next_main(&mut g, 0);
    let (hand, life) = (g.player(1).hand.len(), g.life(1));
    let t = g.tgt(giant);
    cast(&mut g, 0, "W3 Blast", &[t]);
    settle(&mut g);
    assert_eq!((g.player(1).hand.len(), g.life(1)), (hand, life), "the ability ended with the turn
{}", log(&g));
}

/// "Activate only as a sorcery" (CR 602.5d): not in the opponent's turn.
#[test]
fn requiem_monolith_only_as_a_sorcery_cr602_5d() {
    let mut g = scen().battlefield(0, &["Requiem Monolith", "W3 Giant"]).at(Step::Main1, 1).start();
    g.pass();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let m = bf(&g, 0, "Requiem Monolith");
    assert!(!g.has_option(act(m, 0)));
}

// ---- Raging Ravine (a granted "whenever this creature attacks") ---------------------

/// Raging Ravine: "This land enters tapped. {T}: Add {R} or {G}.
/// {2}{R}{G}: Until end of turn, this land becomes a 3/3 red and green
/// Elemental creature with 'Whenever this creature attacks, put a +1/+1
/// counter on it.' It's still a land." (CR 205.1b, 613.1f.)
#[test]
fn raging_ravine_animated_attacks_and_gets_a_counter_cr613_1f() {
    let mut g = scen()
        .battlefield(0, &["Raging Ravine", "Mountain", "Forest", "Mountain", "Forest"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let rv = bf(&g, 0, "Raging Ravine");
    g.choose(act(rv, 1));
    pay_without(&mut g, rv);
    settle(&mut g);
    assert_eq!((g.power(rv), g.toughness(rv)), (3, 3), "{}", log(&g));
    assert!(g.types_of(rv) & engine::db::types::LAND != 0 && g.types_of(rv) & engine::db::types::CREATURE != 0);
    attack_with(&mut g, 0, &["Raging Ravine"]);
    settle(&mut g);
    assert_eq!(g.counters_on(rv, Counter::P1P1), 1, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 16, "a 4/4 attacker");
    // The next turn it is a land again, without the ability; animated
    // again, it gets the ability anew and keeps its counter.
    next_main(&mut g, 0);
    assert_eq!(g.types_of(rv) & engine::db::types::CREATURE, 0);
    g.choose(act(rv, 1));
    pay_without(&mut g, rv);
    settle(&mut g);
    assert_eq!((g.power(rv), g.toughness(rv)), (4, 4), "3/3 and its counter");
    attack_with(&mut g, 0, &["Raging Ravine"]);
    settle(&mut g);
    assert_eq!(g.counters_on(rv, Counter::P1P1), 2, "one trigger, not two
{}", log(&g));
}

/// The granted ability doesn't outlast the turn (CR 514.2): it is gone even
/// if the land is a creature by other means (none here: it isn't), so an
/// unanimated Ravine that can't attack has nothing to trigger; enters tapped.
#[test]
fn raging_ravine_enters_tapped_cr614_1c() {
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &["Raging Ravine"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Raging Ravine", Zone::Hand);
    g.choose(Choice::Play(c));
    settle(&mut g);
    assert!(g.obj(c).tapped);
}

// ---- Terminal Velocity (granted triggered abilities for good) ----------------------

/// Terminal Velocity: "You may put an artifact or creature card from your
/// hand onto the battlefield. That permanent gains haste, 'When this
/// permanent leaves the battlefield, it deals damage equal to its mana value
/// to each creature,' and 'At the beginning of your end step, sacrifice
/// this permanent.'" (CR 611.2a: no duration; 113.7a: the permanent as it
/// last existed deals the damage.)
#[test]
fn terminal_velocity_puts_a_hasty_creature_sacrificed_at_end_step_cr603_10a() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(0, &["W3 Bear"])
        .battlefield(1, &["W3 Wall", "W3 Starship"])
        .hand(0, &["Terminal Velocity", "W3 Giant", "W3 Trinket"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let (bear, wall, ship) = (bf(&g, 0, "W3 Bear"), bf(&g, 1, "W3 Wall"), bf(&g, 1, "W3 Starship"));
    let giant = g.card(0, "W3 Giant", Zone::Hand);
    let trinket = g.card(0, "W3 Trinket", Zone::Hand);
    cast(&mut g, 0, "Terminal Velocity", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert!(opts.contains(&Choice::Card(giant)) && opts.contains(&Choice::Card(trinket)) && opts.contains(&Choice::Pass), "{opts:?}");
    g.choose(Choice::Card(giant));
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.has_kw(giant, Kw::Haste));
    attack_with(&mut g, 0, &["W3 Giant"]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 15, "haste: it attacks the turn it arrives
{}", log(&g));
    to_end_step(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Graveyard, "sacrificed at the end step
{}", log(&g));
    // It left: 5 damage (its mana value) to each creature.
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(wall).zone, Zone::Graveyard);
    assert_eq!(g.obj(ship).zone, Zone::Graveyard);
}

/// An artifact (mana value 1) put with it deals 1 damage to each creature
/// as it leaves; the end-step trigger is its controller's (CR 113.8): it
/// waits for that player's end step.
#[test]
fn terminal_velocity_an_artifact_deals_its_mana_value_as_it_leaves_cr113_7a() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(1, &["W3 Rat", "W3 Bear"])
        .hand(0, &["Terminal Velocity", "W3 Trinket"])
        .at(Step::Main1, 0)
        .start();
    let (rat, bear) = (bf(&g, 1, "W3 Rat"), bf(&g, 1, "W3 Bear"));
    let trinket = g.card(0, "W3 Trinket", Zone::Hand);
    cast(&mut g, 0, "Terminal Velocity", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    g.choose(Choice::Card(trinket));
    settle(&mut g);
    to_end_step(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(trinket).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(rat).zone, Zone::Graveyard, "1 damage to a 1/1");
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.obj(bear).damage, 1);
}

/// "You may": nothing put, nothing happens (CR 603.5).
#[test]
fn terminal_velocity_may_put_nothing_cr608_2d() {
    let mut g = scen().battlefield(0, &["Mountain"; 6]).hand(0, &["Terminal Velocity", "W3 Giant"]).at(Step::Main1, 0).start();
    let giant = g.card(0, "W3 Giant", Zone::Hand);
    cast(&mut g, 0, "Terminal Velocity", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Hand);
}


// ---- Pain for All -------------------------------------------------------------------

/// Pain for All: "Enchant creature you control / When this Aura enters,
/// enchanted creature deals damage equal to its power to any other target"
/// (CR 303.4a; 115.4: any target but that creature; 120.3: the creature is
/// the source).
#[test]
fn pain_for_all_enchanted_creature_deals_its_power_to_any_other_target_cr115_4() {
    let mut g = scen()
        .battlefield(0, &["W3 Giant", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["W3 Wall"])
        .hand(0, &["Pain for All"])
        .at(Step::Main1, 0)
        .start();
    let (giant, wall) = (bf(&g, 0, "W3 Giant"), bf(&g, 1, "W3 Wall"));
    let c = g.card(0, "Pain for All", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(giant)) && !g.has_option(g.tgt(wall)), "enchant creature you control");
    g.choose(g.tgt(giant));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(giant)), "any other target");
    assert!(g.has_option(g.tgt(wall)) && g.has_option(player(1)) && g.has_option(player(0)));
    g.choose(player(1));
    settle(&mut g);
    assert_eq!(g.obj(c).attached, Some(g.obj_ref(giant)), "{}", log(&g));
    assert_eq!(g.life(1), 15, "5: its power\n{}", log(&g));
}

/// "Whenever enchanted creature is dealt damage, it deals that much damage
/// to each opponent" (CR 120.3): all of it, even past lethal, and the
/// creature, gone, still deals it (608.2h).
#[test]
fn pain_for_all_damage_to_the_creature_is_dealt_to_each_opponent_cr608_2h() {
    let mut g = scen()
        .battlefield(0, &["W3 Bear", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Pain for All", "W3 Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "W3 Bear");
    let tb = g.tgt(bear);
    cast(&mut g, 0, "Pain for All", &[tb]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    settle(&mut g);
    assert_eq!(g.life(1), 18, "2: its power");
    let tb = g.tgt(bear);
    cast(&mut g, 0, "W3 Bolt", &[tb]);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 8, "10 more: that much damage\n{}", log(&g));
    assert_eq!(g.life(0), 20, "not its controller");
}

/// The Aura gone before its enters trigger resolves: "enchanted creature"
/// is the creature it enchanted as it last existed (CR 608.2h), which still
/// deals the damage.
#[test]
fn pain_for_all_gone_its_last_enchanted_creature_deals_the_damage_cr608_2h() {
    let mut g = scen()
        .battlefield(0, &["W3 Giant", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Island"])
        .hand(0, &["Pain for All"])
        .hand(1, &["W3 Shatter"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 0, "W3 Giant");
    let tg = g.tgt(giant);
    let aura = cast(&mut g, 0, "Pain for All", &[tg]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.stack().len(), 1, "the enters trigger waits");
    to_opponent_priority(&mut g);
    let t = g.tgt(aura);
    cast(&mut g, 1, "W3 Shatter", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(aura).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 15, "{}", log(&g));
}

// ---- Astelli Reclaimer (the amount of mana spent to cast it) ----------------------

/// The graveyard cards Astelli Reclaimer's enters trigger may target.
fn astelli_targets(g: &mut engine::Game) -> Vec<Choice> {
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || (at_priority(g) && g.stack().is_empty()));
    if g.kind() != Some(DecisionKind::Target) {
        return Vec::new();
    }
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).filter(|c| *c != Choice::Pass).collect()
}

fn gy_target(g: &engine::Game, name: &str) -> Choice {
    Choice::Target(engine::Target::Obj(g.obj_ref(g.card(0, name, Zone::Graveyard))))
}

/// Astelli Reclaimer [5/4, flying]: "When this creature enters, return target
/// noncreature, nonland permanent card with mana value X or less from your
/// graveyard to the battlefield, where X is the amount of mana spent to cast
/// this creature." Cast for {3}{W}{W}: X = 5 (CR 601.2h).
#[test]
fn astelli_reclaimer_cast_for_five_returns_mana_value_five_or_less_cr601_2h() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 5])
        .graveyard(0, &["W3 Bear", "Island", "W3 Monument", "W3 Hulk", "W3 Totem"])
        .graveyard(1, &["W3 Trinket"])
        .hand(0, &["Astelli Reclaimer"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Astelli Reclaimer", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost) || g.kind() == Some(DecisionKind::PayMana));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
    }
    pay(&mut g);
    let (monument, hulk, totem) = (gy_target(&g, "W3 Monument"), gy_target(&g, "W3 Hulk"), gy_target(&g, "W3 Totem"));
    let opts = astelli_targets(&mut g);
    assert!(opts.contains(&monument) && opts.contains(&hulk) && opts.contains(&totem), "{opts:?}");
    assert_eq!(opts.len(), 3, "not a creature or land card, nor an opponent's card: {opts:?}");
    g.choose(monument);
    settle(&mut g);
    assert!(g.find_card(0, "W3 Monument", Zone::Battlefield).is_some(), "{}", log(&g));
    assert!(g.has_kw(c, Kw::Flying));
    assert_eq!((g.power(c), g.toughness(c)), (5, 4));
}

/// Cast for its warp cost {2}{W}: X = 3, so a mana value 4 card can't be
/// targeted (CR 601.2h, 702.185a).
#[test]
fn astelli_reclaimer_warped_for_three_cr702_185a() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 5])
        .graveyard(0, &["W3 Monument", "W3 Hulk"])
        .hand(0, &["Astelli Reclaimer"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Astelli Reclaimer", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1));
    pay(&mut g);
    let hulk = gy_target(&g, "W3 Hulk");
    let opts = astelli_targets(&mut g);
    assert_eq!(opts, vec![hulk], "mana value 3 or less\n{}", log(&g));
    assert_eq!(untapped(&g, 0), 2);
}

/// Put onto the battlefield without being cast (Terminal Velocity): no mana
/// was spent to cast it, X = 0 (CR 601.2h): only a mana value 0 card.
#[test]
fn astelli_reclaimer_not_cast_x_is_zero_cr601_2h() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .graveyard(0, &["W3 Trinket", "W3 Idol"])
        .hand(0, &["Terminal Velocity", "Astelli Reclaimer"])
        .at(Step::Main1, 0)
        .start();
    let a = g.card(0, "Astelli Reclaimer", Zone::Hand);
    cast(&mut g, 0, "Terminal Velocity", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    g.choose(Choice::Card(a));
    let idol = gy_target(&g, "W3 Idol");
    let opts = astelli_targets(&mut g);
    assert_eq!(opts, vec![idol], "{}", log(&g));
}

// ---- Dyadrine, Synthesis Amalgam ----------------------------------------------------

/// Dyadrine [0/1, trample]: "{X}{G}{W} ... enters with a number of +1/+1
/// counters on it equal to the amount of mana spent to cast it" (CR 122.6,
/// 614.1c, 601.2h): X = 2 is four mana, four counters.
#[test]
fn dyadrine_enters_with_a_counter_per_mana_spent_cr601_2h() {
    let mut g = scen().battlefield(0, &["Forest", "Forest", "Plains", "Plains"]).hand(0, &["Dyadrine, Synthesis Amalgam"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Dyadrine, Synthesis Amalgam", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.counters_on(c, Counter::P1P1), 4, "{}", log(&g));
    assert_eq!((g.power(c), g.toughness(c)), (4, 5));
    assert!(g.has_kw(c, Kw::Trample));
}

/// Not cast (Terminal Velocity): no mana spent, no counters; a 0/1.
#[test]
fn dyadrine_not_cast_enters_without_counters_cr601_2h() {
    let mut g = scen().battlefield(0, &["Mountain"; 6]).hand(0, &["Terminal Velocity", "Dyadrine, Synthesis Amalgam"]).at(Step::Main1, 0).start();
    let d = g.card(0, "Dyadrine, Synthesis Amalgam", Zone::Hand);
    cast(&mut g, 0, "Terminal Velocity", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    g.choose(Choice::Card(d));
    settle(&mut g);
    assert_eq!(g.obj(d).zone, Zone::Battlefield);
    assert_eq!(g.counters_on(d, Counter::P1P1), 0);
    assert_eq!((g.power(d), g.toughness(d)), (0, 1));
}

/// "Whenever you attack, you may remove a +1/+1 counter from each of two
/// creatures you control. If you do, draw a card and create a 2/2 colorless
/// Robot artifact creature token" (CR 608.2d; 111.2). Two different
/// creatures; declining does nothing; with only one creature with a
/// counter it can't be done (no question).
#[test]
fn dyadrine_attack_trigger_removes_two_counters_draws_and_makes_a_robot_cr111_2() {
    for case in 0..3 {
        let mut b = scen()
            .battlefield(0, &["Dyadrine, Synthesis Amalgam", "W3 Bear"])
            .counters(0, "Dyadrine, Synthesis Amalgam", Counter::P1P1, 2)
            .at(Step::Main1, 0)
            .turn_number(3);
        if case != 2 {
            b = b.counters(0, "W3 Bear", Counter::P1P1, 1);
        }
        let mut g = b.start();
        let (d, bear) = (bf(&g, 0, "Dyadrine, Synthesis Amalgam"), bf(&g, 0, "W3 Bear"));
        let hand = g.player(0).hand.len();
        attack_with(&mut g, 0, &["W3 Bear"]);
        for _ in 0..20 {
            g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent) || at_priority(g));
            if g.kind() == Some(DecisionKind::ChoosePermanent) || g.stack().is_empty() {
                break;
            }
            g.pass();
        }
        if case == 2 {
            settle(&mut g);
            assert_eq!(g.player(0).hand.len(), hand, "one creature with a counter: nothing");
            continue;
        }
        let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
        assert!(opts.contains(&Choice::Pass) && opts.contains(&Choice::Card(d)), "{opts:?} {:?}
{}", g.options_desc(), log(&g));
        if case == 1 {
            g.choose(Choice::Pass);
            settle(&mut g);
            assert_eq!(g.player(0).hand.len(), hand);
            assert_eq!(g.counters_on(d, Counter::P1P1), 2);
            continue;
        }
        g.choose(Choice::Card(d));
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
        let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
        assert_eq!(opts, vec![Choice::Card(bear)], "another creature, no PASS once begun");
        g.choose(Choice::Card(bear));
        settle(&mut g);
        assert_eq!(g.counters_on(d, Counter::P1P1), 1, "{}", log(&g));
        assert_eq!(g.counters_on(bear, Counter::P1P1), 0);
        assert_eq!(g.player(0).hand.len(), hand + 1);
        let robot = g.permanents_of(0).find(|&c| g.name(c) == "Robot").expect("a Robot");
        assert_eq!((g.power(robot), g.toughness(robot)), (2, 2));
        assert_eq!(g.types_of(robot), engine::db::types::ARTIFACT | engine::db::types::CREATURE);
    }
}

/// The mana payment keeps a creature to remove the counter from: an Eldrazi
/// Scion sacrificed for mana would be gone before the cost is paid (CR
/// 601.2g-h). With one Plains the {2} needs the Scion: not activatable;
/// with two, the Scion stays and gives up its counter.
#[test]
fn sunstar_chaplain_mana_payment_keeps_the_creature_with_the_counter_cr601_2g() {
    for plains in [1usize, 2] {
        let mut g = scen()
            .battlefield(0, &["Sunstar Chaplain", "W3 Scion"])
            .battlefield(0, &vec!["Plains"; plains])
            .battlefield(1, &["W3 Trinket"])
            .counters(0, "W3 Scion", Counter::P1P1, 1)
            .at(Step::Main1, 0)
            .start();
        let (ch, scion, trinket) = (bf(&g, 0, "Sunstar Chaplain"), bf(&g, 0, "W3 Scion"), bf(&g, 1, "W3 Trinket"));
        if plains == 1 {
            assert!(!g.has_option(act(ch, 0)), "the {{2}} needs the Scion\n{}", log(&g));
            continue;
        }
        g.choose(act(ch, 0));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(g.tgt(trinket));
        pay(&mut g);
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
        g.choose(Choice::Card(scion));
        settle(&mut g);
        assert_eq!(g.obj(scion).zone, Zone::Battlefield, "{}", log(&g));
        assert_eq!(g.counters_on(scion, Counter::P1P1), 0);
        assert!(g.obj(trinket).tapped);
    }
}

// ---- Secluded Starforge ("tap X untapped artifacts") --------------------------------

/// Activate Secluded Starforge's pump with X = `x` targeting `t`, tapping
/// the artifacts `taps` (in slot order), and resolve it.
fn starforge_pump(g: &mut engine::Game, x: u8, t: engine::CardIdx, taps: &[engine::CardIdx]) {
    let sf = bf(g, 0, "Secluded Starforge");
    g.choose(act(sf, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(x));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let c = g.tgt(t);
    g.choose(c);
    for &a in taps {
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
        g.choose(Choice::Card(a));
    }
    pay(g);
    settle(g);
}

/// Secluded Starforge: "{2}, {T}, Tap X untapped artifacts you control:
/// Target creature gets +X/+0 until end of turn. Activate only as a sorcery"
/// (CR 107.3a, 601.2b: X announced; the artifacts tapped as the cost is
/// paid, summoning sickness no matter, 302.6).
#[test]
fn secluded_starforge_taps_x_artifacts_for_plus_x_cr601_2b() {
    let mut g = scen()
        .battlefield(0, &["Secluded Starforge", "Plains", "Plains", "W3 Trinket", "W3 Monument", "W3 Idol", "W3 Bear"])
        .at(Step::Main1, 0)
        .start();
    let (bear, trinket, monument, idol) = (bf(&g, 0, "W3 Bear"), bf(&g, 0, "W3 Trinket"), bf(&g, 0, "W3 Monument"), bf(&g, 0, "W3 Idol"));
    let sf = bf(&g, 0, "Secluded Starforge");
    g.choose(act(sf, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    let xs: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(xs.last(), Some(&Choice::Number(3)), "three untapped artifacts: X up to 3 {xs:?}");
    g.choose(Choice::Number(2));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let c = g.tgt(bear);
    g.choose(c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
    let first: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(first.len(), 2, "leaving one for the second tap: {first:?}");
    let mut arts = vec![trinket, monument, idol];
    arts.sort_by_key(|&a| g.perm_action(0, a));
    g.choose(Choice::Card(arts[0]));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
    g.choose(Choice::Card(arts[2]));
    pay(&mut g);
    settle(&mut g);
    assert_eq!((g.power(bear), g.toughness(bear)), (4, 2), "{}", log(&g));
    assert!(g.obj(arts[0]).tapped && g.obj(arts[2]).tapped && !g.obj(arts[1]).tapped);
    assert!(g.obj(sf).tapped);
    assert_eq!(untapped(&g, 0), 0, "{{2}} paid");
}

/// An artifact that makes mana and is needed for the {2} can't also be
/// tapped for X (CR 601.2g-h): with a Plains and a mana artifact, X is at
/// most the other artifacts.
#[test]
fn secluded_starforge_x_leaves_the_mana_payable_cr601_2h() {
    let mut g = scen()
        .battlefield(0, &["Secluded Starforge", "Plains", "W3 Mox", "W3 Trinket", "W3 Bear"])
        .at(Step::Main1, 0)
        .start();
    let (bear, trinket) = (bf(&g, 0, "W3 Bear"), bf(&g, 0, "W3 Trinket"));
    let sf = bf(&g, 0, "Secluded Starforge");
    g.choose(act(sf, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    let xs: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(xs, vec![Choice::Number(0), Choice::Number(1)], "{xs:?}");
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let c = g.tgt(bear);
    g.choose(c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(opts, vec![Choice::Card(trinket)], "not the Mox: the {{2}} needs it");
    g.choose(Choice::Card(trinket));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.power(bear), 3, "{}", log(&g));
}

/// X = 0 taps nothing; "{5}, {T}: Create a 2/2 colorless Robot artifact
/// creature token"; "{T}: Add {C}"; "activate only as a sorcery" (CR
/// 602.5d).
#[test]
fn secluded_starforge_robot_and_x_zero_cr111_2() {
    let mut g = scen()
        .battlefield(0, &["Secluded Starforge", "Plains", "Plains", "W3 Trinket", "W3 Bear"])
        .at(Step::Main1, 0)
        .start();
    let (bear, trinket) = (bf(&g, 0, "W3 Bear"), bf(&g, 0, "W3 Trinket"));
    starforge_pump(&mut g, 0, bear, &[]);
    assert_eq!(g.power(bear), 2);
    assert!(!g.obj(trinket).tapped);
    let mut g = scen().battlefield(0, &["Secluded Starforge", "Plains", "Plains", "Plains", "Plains", "Plains"]).at(Step::Main1, 0).start();
    let sf = bf(&g, 0, "Secluded Starforge");
    g.choose(act(sf, 2));
    pay(&mut g);
    settle(&mut g);
    let robot = g.permanents_of(0).find(|&c| g.name(c) == "Robot").expect("a Robot");
    assert_eq!((g.power(robot), g.toughness(robot)), (2, 2));
    let mut g = scen().battlefield(0, &["Secluded Starforge", "Plains", "Plains", "W3 Bear"]).at(Step::Main1, 1).start();
    g.pass();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let sf = bf(&g, 0, "Secluded Starforge");
    assert!(!g.has_option(act(sf, 1)), "sorcery speed");
}

// ---- Possibility Technician ("you may play it if you control a Kavu") -------------

/// Possibility Technician [3/3 Kavu Artificer]: "Whenever this creature or
/// another Kavu you control enters, exile the top card of your library. For
/// as long as that card remains exiled, you may play it if you control a
/// Kavu" (CR 601.2a, 305.1: the condition is checked as the card would be
/// played).
#[test]
fn possibility_technician_exiles_the_top_card_playable_while_you_control_a_kavu_cr601_2a() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Forest", "Forest"])
        .battlefield(1, &["Swamp"])
        .library(0, &["W3 Bear"])
        .hand(0, &["Possibility Technician"])
        .hand(1, &["W3 Murder"])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(0, "W3 Bear", Zone::Library);
    let pt = cast(&mut g, 0, "Possibility Technician", &[]);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Exile, "{}", log(&g));
    assert!(g.has_option(Choice::Cast(bear)), "a Kavu: it may be cast {:?}", g.options_desc());
    // Without a Kavu, it can't be played; the permission lasts.
    to_opponent_priority(&mut g);
    let t = g.tgt(pt);
    cast(&mut g, 1, "W3 Murder", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(pt).zone, Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(bear)), "no Kavu: {:?}", g.options_desc());
}

/// "Or another Kavu you control enters": a second card; an opponent's Kavu
/// doesn't trigger it. A land exiled this way may be played as the land drop.
#[test]
fn possibility_technician_another_kavu_you_control_triggers_it_cr603_2() {
    let mut g = scen()
        .battlefield(0, &["Possibility Technician", "Mountain", "Mountain"])
        .battlefield(1, &["Swamp", "Swamp"])
        .library(0, &["Mountain", "W3 Bear"])
        .hand(0, &["W3 Kavu"])
        .hand(1, &["W3 Kavu"])
        .at(Step::Main1, 0)
        .start();
    let (bear, mtn) = (g.card(0, "W3 Bear", Zone::Library), g.card(0, "Mountain", Zone::Library));
    let _ = bear;
    cast(&mut g, 0, "W3 Kavu", &[]);
    settle(&mut g);
    assert_eq!(g.obj(mtn).zone, Zone::Exile, "{}", log(&g));
    assert!(g.has_option(Choice::Play(mtn)), "a land: played as the land drop {:?}", g.options_desc());
    g.choose(Choice::Play(mtn));
    settle(&mut g);
    assert_eq!(g.obj(mtn).zone, Zone::Battlefield);
    // The opponent's Kavu: nothing.
    next_main(&mut g, 1);
    let lib = g.player(0).library.len();
    cast(&mut g, 1, "W3 Kavu", &[]);
    settle(&mut g);
    assert_eq!(g.player(0).library.len(), lib, "not a Kavu you control\n{}", log(&g));
}

// ---- Robe of Stars (phasing) --------------------------------------------------------

/// Equip Robe of Stars to `c` and resolve it.
fn equip_robe(g: &mut engine::Game, c: engine::CardIdx) {
    let robe = bf(g, 0, "Robe of Stars");
    g.choose(act(robe, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let t = g.tgt(c);
    g.choose(t);
    pay(g);
    settle(g);
}

/// Robe of Stars: "Equipped creature gets +0/+3. Astral Projection — {1}{W}:
/// Equipped creature phases out. Equip {1}" (CR 702.26b: treated as though
/// it doesn't exist; 702.26e: the Robe attached to it phases out with it;
/// 502.1, 702.26h: both phase in during its controller's next untap step,
/// still attached, the same objects, 702.26d).
#[test]
fn robe_of_stars_phases_the_creature_out_until_its_controllers_next_untap_cr702_26() {
    let mut g = scen()
        .battlefield(0, &["Robe of Stars", "W3 Bear", "Plains", "Plains", "Plains"])
        .battlefield(1, &["Swamp"])
        .hand(1, &["W3 Murder"])
        .counters(0, "W3 Bear", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let (robe, bear) = (bf(&g, 0, "Robe of Stars"), bf(&g, 0, "W3 Bear"));
    let r = g.obj_ref(bear);
    equip_robe(&mut g, bear);
    assert_eq!((g.power(bear), g.toughness(bear)), (3, 6), "+0/+3 and its counter");
    g.choose(act(robe, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::PhasedOut, "{}", log(&g));
    assert_eq!(g.obj(robe).zone, Zone::PhasedOut, "the Robe phases out with it");
    assert!(g.permanents_of(0).all(|c| c != bear && c != robe));
    // The opponent's turn: it doesn't exist; no target for Murder.
    next_main(&mut g, 1);
    let murder = g.card(1, "W3 Murder", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(murder)), "no creature to target");
    assert_eq!(g.obj(bear).zone, Zone::PhasedOut, "not the opponent's untap step");
    // Its controller's untap step: back, attached, the same object.
    next_main(&mut g, 0);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj_ref(bear), r, "the same object (CR 702.26d)");
    assert_eq!(g.obj(robe).zone, Zone::Battlefield);
    assert_eq!(g.obj(robe).attached, Some(r), "still attached");
    assert_eq!((g.power(bear), g.toughness(bear)), (3, 6), "its counter stays");
    assert!(!log(&g).contains("Battlefield -> "), "no zone change");
}

/// A creature that phases out leaves combat (CR 506.4): an attacker deals
/// no combat damage; phasing in triggers no enters ability (702.26d).
#[test]
fn robe_of_stars_a_phased_out_attacker_leaves_combat_cr506_4() {
    let mut g = scen()
        .battlefield(0, &["Robe of Stars", "W3 Bear", "Plains", "Plains", "Plains"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let (robe, bear) = (bf(&g, 0, "Robe of Stars"), bf(&g, 0, "W3 Bear"));
    equip_robe(&mut g, bear);
    attack_with(&mut g, 0, &["W3 Bear"]);
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    g.choose(act(robe, 0));
    pay(&mut g);
    settle(&mut g);
    assert!(!g.obj(bear).attacking);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 20, "{}", log(&g));
}

/// "Until end of turn" effects on it end while it is phased out (CR
/// 702.26d, 514.2); damage marked on it is removed in the cleanup step too.
#[test]
fn robe_of_stars_until_end_of_turn_effects_end_while_phased_out_cr702_26d() {
    let mut g = scen()
        .battlefield(0, &["Robe of Stars", "W3 Bear", "Plains", "Plains", "Plains", "Forest", "Mountain"])
        .hand(0, &["W3 Growth"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let (robe, bear) = (bf(&g, 0, "Robe of Stars"), bf(&g, 0, "W3 Bear"));
    equip_robe(&mut g, bear);
    let t = g.tgt(bear);
    cast(&mut g, 0, "W3 Growth", &[t]);
    settle(&mut g);
    assert_eq!(g.power(bear), 5);
    g.choose(act(robe, 0));
    pay(&mut g);
    settle(&mut g);
    next_main(&mut g, 0);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!((g.power(bear), g.toughness(bear)), (2, 5), "the +3/+3 ended; the Robe's +0/+3 applies\n{}", log(&g));
}

// ---- Ragost, Deft Gastronaut (Foods; a granted activated ability) ------------------

fn food(g: &engine::Game, c: engine::CardIdx) -> bool {
    let k = db().subtype_id("Food").expect("the Food subtype");
    g.has_subtype(c, k)
}

/// Ragost [2/2]: "Artifacts you control are Foods in addition to their other
/// types and have '{2}, {T}, Sacrifice this artifact: You gain 3 life.'"
/// (CR 205.3g, 613.1d: layer 4; 613.1f: layer 6; the Food ability, 111.10b.)
/// The opponent's artifacts aren't; an artifact with an ability of its own
/// has the granted one in the next slot.
#[test]
fn ragost_your_artifacts_are_foods_with_the_food_ability_cr111_10b() {
    let mut g = scen()
        .battlefield(0, &["Ragost, Deft Gastronaut", "W3 Trinket", "W3 Mox", "Plains", "Plains"])
        .battlefield(1, &["W3 Idol"])
        .at(Step::Main1, 0)
        .start();
    let (ragost, trinket, mox, idol) = (bf(&g, 0, "Ragost, Deft Gastronaut"), bf(&g, 0, "W3 Trinket"), bf(&g, 0, "W3 Mox"), bf(&g, 1, "W3 Idol"));
    assert!(food(&g, trinket) && food(&g, mox), "artifacts you control are Foods");
    assert!(!food(&g, idol) && !food(&g, ragost));
    assert!(g.has_option(act(trinket, 0)), "the Food ability: {:?}", g.options_desc());
    assert!(g.has_option(act(mox, 1)), "after the Mox's own mana ability");
    g.choose(act(trinket, 0));
    pay_without(&mut g, mox);
    settle(&mut g);
    assert_eq!(g.obj(trinket).zone, Zone::Graveyard, "sacrificed\n{}", log(&g));
    assert_eq!(g.life(0), 23, "you gain 3 life");
    assert_eq!(untapped(&g, 0), 0, "{{2}}");
}

/// "{1}, {T}, Sacrifice a Food: Ragost deals 3 damage to each opponent" (an
/// artifact made a Food by its first ability counts, CR 205.3g).
#[test]
fn ragost_sacrifices_a_food_for_3_damage_to_each_opponent_cr701_21a() {
    let mut g = scen()
        .battlefield(0, &["Ragost, Deft Gastronaut", "W3 Trinket", "Plains"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let (ragost, trinket) = (bf(&g, 0, "Ragost, Deft Gastronaut"), bf(&g, 0, "W3 Trinket"));
    g.choose(act(ragost, 0));
    pay(&mut g);
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::Sacrifice) { Some(Choice::Card(trinket)) } else { None });
    assert_eq!(g.obj(trinket).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(1), 17);
    assert!(g.obj(ragost).tapped);
}

/// "At the beginning of each end step, if you gained life this turn, untap
/// Ragost" (CR 603.4: an intervening "if"; each end step, the opponent's
/// too). Without life gained, it doesn't trigger.
#[test]
fn ragost_untaps_at_each_end_step_if_you_gained_life_cr603_4() {
    for gained in [false, true] {
        let mut g = scen()
            .battlefield(0, &["Ragost, Deft Gastronaut", "W3 Trinket", "W3 Monument", "Plains", "Plains", "Plains"])
            .at(Step::Main1, 0)
            .turn_number(3)
            .start();
        let (ragost, trinket, monument) = (bf(&g, 0, "Ragost, Deft Gastronaut"), bf(&g, 0, "W3 Trinket"), bf(&g, 0, "W3 Monument"));
        // Tap Ragost: its damage ability, sacrificing the Monument.
        g.choose(act(ragost, 0));
        pay(&mut g);
        resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::Sacrifice) { Some(Choice::Card(monument)) } else { None });
        assert!(g.obj(ragost).tapped);
        if gained {
            g.choose(act(trinket, 0));
            pay(&mut g);
            settle(&mut g);
            assert_eq!(g.life(0), 23);
        }
        to_end_step(&mut g);
        settle(&mut g);
        assert_eq!(!g.obj(ragost).tapped, gained, "gained life: {gained}\n{}", log(&g));
    }
}
