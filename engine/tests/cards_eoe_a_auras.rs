//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): Auras that
//! enchant a permanent (CR 303.4, 702.5) and Cryoshatter. Test names cite
//! the CR.

mod eoe_a_kit;

use eoe_a_kit::*;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};

fn cast_at(g: &mut Game, c: CardIdx, t: CardIdx) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(t));
    pay(g);
}

/// "Enchant creature" (CR 702.5a, 303.4a): the Aura spell targets a creature
/// (any player's), not a noncreature permanent; it enters attached to it
/// (608.3c). "Enchanted creature gets -5/-0."
#[test]
fn cryoshatter_enchants_target_creature_cr303_4a() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .battlefield(1, &["Lane A Giant", "Lane A Trinket"])
        .hand(0, &["Cryoshatter"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Cryoshatter", Zone::Hand);
    let giant = bf(&g, 1, "Lane A Giant");
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Trinket"))), "a creature");
    g.choose(g.tgt(giant));
    pay(&mut g);
    settle(&mut g);
    let aura = bf(&g, 0, "Cryoshatter");
    assert_eq!(g.obj(aura).attached.map(|r| r.idx), Some(giant));
    assert_eq!((g.power(giant), g.toughness(giant)), (0, 5));
}

/// "When enchanted creature becomes tapped or is dealt damage, destroy it."
/// (Attacking taps it; damage destroys it too.)
#[test]
fn cryoshatter_destroys_the_creature_when_it_becomes_tapped_cr603_2() {
    // Tapped: the enchanted creature attacks.
    let mut g = scen()
        .battlefield(0, &["Island"])
        .battlefield(1, &["Lane A Giant", "Lane A Bear"])
        .hand(0, &["Cryoshatter"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Cryoshatter", Zone::Hand);
    let giant = bf(&g, 1, "Lane A Giant");
    cast_at(&mut g, c, giant);
    settle(&mut g);
    next_main(&mut g, 1);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(giant));
    g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Lane A Giant", Zone::Graveyard), 1, "{}", log(&g));
    // The Aura followed (CR 704.5m).
    assert_eq!(g.count_in(0, "Cryoshatter", Zone::Graveyard), 1);
}

/// "... or is dealt damage, destroy it": 3 damage doesn't kill the 5/5, the
/// trigger does.
#[test]
fn cryoshatter_destroys_the_creature_when_it_is_dealt_damage_cr120_3() {
    let mut g = scen()
        .battlefield(0, &["Island", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Lane A Giant"])
        .hand(0, &["Cryoshatter", "Debris Field Crusher"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Cryoshatter", Zone::Hand);
    let giant = bf(&g, 1, "Lane A Giant");
    cast_at(&mut g, c, giant);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Lane A Giant", Zone::Battlefield), 1, "untapped and undamaged: it stays");
    let d = g.card(0, "Debris Field Crusher", Zone::Hand);
    g.choose(Choice::Cast(d));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(giant));
    g.drive_until(|g| at_priority(g));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Lane A Giant", Zone::Graveyard), 1, "{}", log(&g));
}

/// CR 704.5m: an Aura attached to an illegal object goes to its owner's
/// graveyard: Cryoshatter on an artifact Synthesizer Labship animated, once
/// the artifact stops being a creature at end of turn.
#[test]
fn an_aura_on_an_object_it_cant_enchant_goes_to_the_graveyard_cr704_5m() {
    let mut g = scen()
        .battlefield(0, &["Synthesizer Labship", "Lane A Trinket", "Island"])
        .counters(0, "Synthesizer Labship", charge(), 2)
        .hand(0, &["Cryoshatter"])
        .at(Step::Main1, 0)
        .start();
    let trinket = bf(&g, 0, "Lane A Trinket");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(trinket));
    settle(&mut g);
    // In combat the Trinket is a 2/2 creature: enchant it.
    let c = g.card(0, "Cryoshatter", Zone::Hand);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    // (Cryoshatter isn't instant-speed: cast it in the second main phase,
    // the Trinket still a creature until end of turn.)
    cast_at(&mut g, c, trinket);
    settle(&mut g);
    let aura = bf(&g, 0, "Cryoshatter");
    assert_eq!(g.obj(aura).attached.map(|r| r.idx), Some(trinket));
    next_main(&mut g, 1);
    assert_eq!(g.count_in(0, "Cryoshatter", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Lane A Trinket", Zone::Battlefield), 1);
}

/// CR 303.4f-g: an Aura put onto the battlefield without being cast (Rescue
/// Skiff returning it) is attached to a permanent its controller chooses as
/// it enters (not targeted); with nothing it could enchant, it stays in the
/// graveyard.
#[test]
fn an_aura_returned_to_the_battlefield_chooses_what_it_enchants_cr303_4f() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 6])
        .battlefield(1, &["Lane A Giant", "Lane A Bear"])
        .graveyard(0, &["Cryoshatter"])
        .hand(0, &["Rescue Skiff"])
        .at(Step::Main1, 0)
        .start();
    let skiff = g.card(0, "Rescue Skiff", Zone::Hand);
    g.choose(Choice::Cast(skiff));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(g.card(0, "Cryoshatter", Zone::Graveyard)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
    let bear = bf(&g, 1, "Lane A Bear");
    assert!(g.has_option(Choice::Card(bf(&g, 1, "Lane A Giant"))) && g.has_option(Choice::Card(bear)));
    assert!(!g.has_option(Choice::Card(bf(&g, 0, "Rescue Skiff"))), "not a creature");
    g.choose(Choice::Card(bear));
    settle(&mut g);
    let aura = bf(&g, 0, "Cryoshatter");
    assert_eq!(g.obj(aura).attached.map(|r| r.idx), Some(bear));
    assert_eq!(g.power(bear), -3);
    // Nothing to enchant: it stays in the graveyard.
    let mut g = scen().battlefield(0, &["Plains"; 6]).graveyard(0, &["Cryoshatter"]).hand(0, &["Rescue Skiff"]).at(Step::Main1, 0).start();
    let skiff = g.card(0, "Rescue Skiff", Zone::Hand);
    g.choose(Choice::Cast(skiff));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Cryoshatter", Zone::Graveyard), 1);
}

/// CR 608.3a: an Aura spell whose target is illegal as it resolves doesn't
/// resolve; it goes to its owner's graveyard.
#[test]
fn an_aura_spell_with_an_illegal_target_fizzles_cr608_3a() {
    let mut g = scen()
        .battlefield(0, &["Island", "Mountain"])
        .battlefield(1, &["Lane A Giant"])
        .hand(0, &["Cryoshatter", "Lane A Bolt"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Cryoshatter", Zone::Hand);
    let giant = bf(&g, 1, "Lane A Giant");
    cast_at(&mut g, c, giant);
    g.drive_until(|g| at_priority(g));
    let bolt = g.card(0, "Lane A Bolt", Zone::Hand);
    cast_at(&mut g, bolt, giant);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Lane A Giant", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Cryoshatter", Zone::Graveyard), 1);
}

/// "Enchant artifact you control / Enchanted permanent has ward {1}"
/// (Hardlight Containment's, on a custom card; CR 702.21a): an opponent's
/// spell targeting the enchanted artifact is countered unless they pay {1};
/// the Aura can't target an opponent's artifact.
#[test]
fn an_aura_grants_ward_to_the_enchanted_permanent_cr702_21a() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Lane A Trinket"])
        .battlefield(1, &["Lane A Trinket", "Island"])
        .hand(0, &["Lane A Ward Aura"])
        .hand(1, &["Lane A Tap"])
        .at(Step::Main1, 0)
        .start();
    let aura = g.card(0, "Lane A Ward Aura", Zone::Hand);
    let mine = bf(&g, 0, "Lane A Trinket");
    g.choose(Choice::Cast(aura));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Trinket"))), "an artifact you control");
    g.choose(g.tgt(mine));
    pay(&mut g);
    settle(&mut g);
    // P1, in P0's turn: Tap targets the warded Trinket; P1 can't pay {1}
    // (their Island paid for the spell).
    g.pass();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 1);
    let tap = g.card(1, "Lane A Tap", Zone::Hand);
    g.choose(Choice::Cast(tap));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(mine));
    pay(&mut g);
    settle(&mut g);
    assert!(!g.obj(mine).tapped, "countered by ward\n{}", log(&g));
    assert_eq!(g.count_in(1, "Lane A Tap", Zone::Graveyard), 1);
}
