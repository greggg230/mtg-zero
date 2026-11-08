//! Card tests of the Edge of Eternities cards W1 unblocks by fixing their
//! card IR in mtg-bot (ENGINE.md 99.11; docs/ir_defects.md 21-101), each
//! ability against its Oracle text and the official rulings: counterspells
//! with riders (Divert Disaster, Unravel), graveyard returns (Archenemy's
//! Charm, Pull Through the Weft, Fell Gravship, Xu-Ifit), a linked exile and
//! its return (Endless Sands), a bounce with an "if it was attacking" rider
//! (Desculpting Blast).

mod eoe_b;

use engine::db::{types, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use eoe_b::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn tokens_named(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    g.permanents_of(p).filter(|&c| g.name(c) == name && g.def_of(c).token).collect()
}

fn in_zone(g: &Game, p: PlayerId, name: &str, z: Zone) -> bool {
    g.find_card(p, name, z).is_some()
}

/// P1 casts `spell` (targets: first option); P0 gets priority with it on the
/// stack.
fn p1_casts(g: &mut Game, spell: &str) {
    let c = g.card(1, spell, Zone::Hand);
    start_with(g, Choice::Cast(c), |_| None);
    assert_eq!(g.stack().len(), 1, "{spell} on the stack\n{}", log(g));
    g.pass();
    assert!(at_priority(g) && g.pending_decision().unwrap().player == 0, "P0's priority\n{}", log(g));
}

// ---- Divert Disaster (docs/ir_defects.md 43) --------------------------------

/// Divert Disaster: "Counter target spell unless its controller pays {2}. If
/// they do, you create a Lander token." Not paid: countered, no Lander. Paid:
/// the spell resolves and Divert Disaster's controller (not the payer)
/// creates the Lander (CR 111.10). Unable to pay: no option to (CR 118.3),
/// countered.
#[test]
fn cr_701_6a_divert_disaster_counters_unless_paid_and_the_payment_makes_you_a_lander() {
    for (lands, pay) in [(4, false), (4, true), (2, false)] {
        let mut g = scen()
            .battlefield(0, &["Island", "Island"])
            .hand(0, &["Divert Disaster"])
            .battlefield(1, &vec!["Forest"; lands])
            .hand(1, &["Test Bear"])
            .at(Step::Main1, 1)
            .start();
        p1_casts(&mut g, "Test Bear");
        let dd = g.card(0, "Divert Disaster", Zone::Hand);
        start_with(&mut g, Choice::Cast(dd), |_| None);
        resolve_with(&mut g, |g| {
            if g.kind() == Some(DecisionKind::YesNo) {
                assert_eq!(g.pending_decision().unwrap().player, 1, "the spell's controller decides");
                return Some(if pay && g.has_option(Choice::Yes) { Choice::Yes } else { Choice::Pass });
            }
            None
        });
        let resolved = in_zone(&g, 1, "Test Bear", Zone::Battlefield);
        assert_eq!(resolved, pay, "lands {lands} pay {pay}\n{}", log(&g));
        assert_eq!(in_zone(&g, 1, "Test Bear", Zone::Graveyard), !pay);
        assert_eq!(tokens_named(&g, 0, "Lander").len(), pay as usize, "the counterspell's controller creates it");
        assert!(tokens_named(&g, 1, "Lander").is_empty());
        if pay {
            let l = tokens_named(&g, 0, "Lander")[0];
            assert!(g.def_of(l).token && g.is_type(l, types::ARTIFACT) && subtype(&g, l, "Lander"));
        }
    }
    // With two lands only (both tapped for the Bear), paying isn't offered.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Divert Disaster"])
        .battlefield(1, &["Forest", "Forest"])
        .hand(1, &["Test Bear"])
        .at(Step::Main1, 1)
        .start();
    p1_casts(&mut g, "Test Bear");
    let dd = g.card(0, "Divert Disaster", Zone::Hand);
    start_with(&mut g, Choice::Cast(dd), |_| None);
    let mut asked_yes = false;
    for _ in 0..50 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        if at_priority(&g) {
            g.pass();
            continue;
        }
        if g.kind() == Some(DecisionKind::YesNo) {
            asked_yes |= g.has_option(Choice::Yes);
            g.choose(Choice::Pass);
            continue;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert!(!asked_yes, "no mana to pay {{2}}");
    assert!(in_zone(&g, 1, "Test Bear", Zone::Graveyard));
}

// ---- Archenemy's Charm (docs/ir_defects.md 62) ------------------------------

/// Archenemy's Charm, first mode: "Exile target creature or planeswalker."
/// A planeswalker or a creature is a legal target; a land or a noncreature
/// artifact isn't.
#[test]
fn cr_115_1_archenemys_charm_exiles_a_creature_or_a_planeswalker() {
    for walker in [true, false] {
        let mut g = scen()
            .battlefield(0, &["Swamp"; 3])
            .hand(0, &["Archenemy's Charm"])
            .battlefield(1, &["Tezzeret, Cruel Captain", "Test Bear", "Nutrient Block", "Forest"])
            .at(Step::Main1, 0)
            .start();
        let (tez, bear, block, forest) = (bf(&g, 1, "Tezzeret, Cruel Captain"), bf(&g, 1, "Test Bear"), bf(&g, 1, "Nutrient Block"), bf(&g, 1, "Forest"));
        let c = g.card(0, "Archenemy's Charm", Zone::Hand);
        g.choose(Choice::Cast(c));
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
        g.choose(Choice::Number(0));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        assert!(g.has_option(g.tgt(tez)) && g.has_option(g.tgt(bear)), "a planeswalker, a creature");
        assert!(!g.has_option(g.tgt(block)) && !g.has_option(g.tgt(forest)), "not an artifact, not a land");
        let t = if walker { tez } else { bear };
        let tc = g.tgt(t);
        g.choose(tc);
        run_until(&mut g, at_priority, |_| None);
        settle(&mut g);
        assert_eq!(g.obj(t).zone, Zone::Exile, "{}\n{}", g.name(t), log(&g));
    }
}

/// Archenemy's Charm, second mode: "Return one or two target creature and/or
/// planeswalker cards from your graveyard to your hand." Creature and
/// planeswalker cards in your graveyard are legal; a land card, an instant
/// card or a card in an opponent's graveyard isn't (CR 115.1, 601.2c: one or
/// two of them).
#[test]
fn cr_601_2c_archenemys_charm_returns_one_or_two_creature_and_or_planeswalker_cards() {
    for two in [true, false] {
        let mut g = scen()
            .battlefield(0, &["Swamp"; 3])
            .hand(0, &["Archenemy's Charm"])
            .graveyard(0, &["Test Bear", "Tezzeret, Cruel Captain", "Forest", "Gravkill"])
            .graveyard(1, &["Test Flyer"])
            .at(Step::Main1, 0)
            .start();
        let gy = |g: &Game, p, n| g.card(p, n, Zone::Graveyard);
        let (bear, tez, forest, kill, theirs) = (gy(&g, 0, "Test Bear"), gy(&g, 0, "Tezzeret, Cruel Captain"), gy(&g, 0, "Forest"), gy(&g, 0, "Gravkill"), gy(&g, 1, "Test Flyer"));
        let c = g.card(0, "Archenemy's Charm", Zone::Hand);
        g.choose(Choice::Cast(c));
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
        g.choose(Choice::Number(1));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        assert!(g.has_option(g.tgt(bear)) && g.has_option(g.tgt(tez)));
        assert!(!g.has_option(g.tgt(forest)) && !g.has_option(g.tgt(kill)) && !g.has_option(g.tgt(theirs)));
        assert!(!g.has_option(Choice::Pass), "at least one");
        let tb = g.tgt(bear);
        g.choose(tb);
        if g.kind() == Some(DecisionKind::Target) {
            if two {
                let tt = g.tgt(tez);
                g.choose(tt);
            } else {
                g.choose(Choice::Pass);
            }
        } else {
            panic!("a second target may be chosen\n{}", log(&g));
        }
        run_until(&mut g, at_priority, |_| None);
        settle(&mut g);
        assert_eq!(g.obj(bear).zone, Zone::Hand);
        assert_eq!(g.obj(tez).zone, if two { Zone::Hand } else { Zone::Graveyard });
        assert_eq!(g.obj(forest).zone, Zone::Graveyard);
    }
}

/// Archenemy's Charm, third mode: "Put two +1/+1 counters on target creature
/// you control. It gains lifelink until end of turn."
#[test]
fn cr_122_1_archenemys_charm_puts_two_counters_and_grants_lifelink() {
    let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp", "Test Bear"]).hand(0, &["Archenemy's Charm"]).battlefield(1, &["Test Flyer"]).at(Step::Main1, 0).start();
    let (bear, flyer) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Flyer"));
    let c = g.card(0, "Archenemy's Charm", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(flyer)), "a creature you control");
    let tb = g.tgt(bear);
    g.choose(tb);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 2);
    assert!(g.has_kw(bear, Kw::Lifelink));
}

// ---- Pull Through the Weft (docs/ir_defects.md 49) --------------------------

/// Pull Through the Weft: "Return up to two target nonland permanent cards
/// from your graveyard to your hand, then return up to two target land cards
/// from your graveyard to the battlefield tapped." Two target requirements
/// (CR 601.2c), each up to two; the lands enter tapped (614.1c). An instant
/// card is no nonland permanent card.
#[test]
fn cr_601_2c_pull_through_the_weft_returns_permanents_to_hand_and_lands_tapped() {
    let mut g = scen()
        .battlefield(0, &["Forest"; 5])
        .hand(0, &["Pull Through the Weft"])
        .graveyard(0, &["Test Bear", "Nutrient Block", "Test Flyer", "Gravkill", "Plains", "Island", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let gy = |g: &Game, n| g.card(0, n, Zone::Graveyard);
    let (bear, block, flyer, kill, plains, island, swamp) = (gy(&g, "Test Bear"), gy(&g, "Nutrient Block"), gy(&g, "Test Flyer"), gy(&g, "Gravkill"), gy(&g, "Plains"), gy(&g, "Island"), gy(&g, "Swamp"));
    let c = g.card(0, "Pull Through the Weft", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    // First requirement: nonland permanent cards.
    assert!(g.has_option(g.tgt(bear)) && g.has_option(g.tgt(block)) && g.has_option(g.tgt(flyer)));
    assert!(!g.has_option(g.tgt(kill)) && !g.has_option(g.tgt(plains)));
    assert!(g.has_option(Choice::Pass), "up to two");
    for t in [bear, block] {
        let tc = g.tgt(t);
        g.choose(tc);
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    // Second requirement: land cards.
    assert!(g.has_option(g.tgt(plains)) && g.has_option(g.tgt(island)) && !g.has_option(g.tgt(flyer)));
    for t in [plains, island] {
        let tc = g.tgt(t);
        g.choose(tc);
    }
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!((g.obj(bear).zone, g.obj(block).zone), (Zone::Hand, Zone::Hand), "{}", log(&g));
    for l in [plains, island] {
        assert_eq!(g.obj(l).zone, Zone::Battlefield, "{}", g.name(l));
        assert!(g.obj(l).tapped, "{} enters tapped", g.name(l));
    }
    assert_eq!((g.obj(flyer).zone, g.obj(kill).zone, g.obj(swamp).zone), (Zone::Graveyard, Zone::Graveyard, Zone::Graveyard));
}

/// "Up to": the spell can be cast with an empty graveyard and does nothing
/// (CR 115.1d, 601.2c).
#[test]
fn cr_115_1d_pull_through_the_weft_with_no_targets() {
    let mut g = scen().battlefield(0, &["Forest"; 5]).hand(0, &["Pull Through the Weft"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Pull Through the Weft", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)));
    cast_and_resolve(&mut g, 0, "Pull Through the Weft", |_| None);
    assert!(in_zone(&g, 0, "Pull Through the Weft", Zone::Graveyard));
}

// ---- Fell Gravship (docs/ir_defects.md 100) ---------------------------------

/// Fell Gravship: "When this Spacecraft enters, mill three cards, then return
/// a creature or Spacecraft card from your graveyard to your hand." The card
/// is chosen as the ability resolves, after the mill (CR 115.1d: not a
/// target), so a milled card can be returned; a Spacecraft card is as good
/// as a creature card, an instant card isn't.
#[test]
fn cr_608_2c_fell_gravship_returns_a_milled_creature_or_spacecraft_card() {
    let mut g = scen()
        .battlefield(0, &["Swamp"; 3])
        .hand(0, &["Fell Gravship"])
        .library(0, &["Test Bear", "Wurmwall Sweeper", "Gravkill"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Fell Gravship", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |_| None);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
            let names: Vec<String> = g.pending_decision().unwrap().options.iter().filter_map(|o| match o.choice {
                Choice::Card(c) => Some(g.name(c).to_string()),
                _ => None,
            }).collect();
            assert!(!names.contains(&"Gravkill".to_string()), "an instant card isn't offered: {names:?}");
            let ship = g.card(0, "Wurmwall Sweeper", Zone::Graveyard);
            return Some(Choice::Card(ship));
        }
        None
    });
    assert!(in_zone(&g, 0, "Fell Gravship", Zone::Battlefield));
    assert!(in_zone(&g, 0, "Wurmwall Sweeper", Zone::Hand), "the Spacecraft card\n{}", log(&g));
    assert!(in_zone(&g, 0, "Test Bear", Zone::Graveyard) && in_zone(&g, 0, "Gravkill", Zone::Graveyard), "milled");
}

/// With no creature or Spacecraft card in the graveyard as it triggers, the
/// ability still goes on the stack and mills (CR 603.3d: no target needed);
/// with none after the mill either, nothing returns.
#[test]
fn cr_603_3d_fell_gravship_mills_with_nothing_to_return() {
    let mut g = scen().battlefield(0, &["Swamp"; 3]).hand(0, &["Fell Gravship"]).library(0, &["Gravkill", "Forest", "Plains"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Fell Gravship", |_| None);
    for n in ["Gravkill", "Forest", "Plains"] {
        assert!(in_zone(&g, 0, n, Zone::Graveyard), "{n} milled\n{}", log(&g));
    }
}

// ---- Desculpting Blast (docs/ir_defects.md 101) ------------------------------

/// Desculpting Blast: "Return target nonland permanent to its owner's hand.
/// If it was attacking, create a 1/1 colorless Drone artifact creature token
/// with flying and 'This token can block only creatures with flying.'" An
/// attacking creature: the Drone (as it last existed, CR 608.2h); a
/// non-attacking permanent (an artifact): no Drone; a land isn't a target.
#[test]
fn cr_608_2h_desculpting_blast_makes_a_drone_only_for_an_attacker() {
    // An attacker.
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &["Desculpting Blast"]).battlefield(1, &["Test Bear", "Forest"]).at(Step::Main1, 1).start();
    let bear = bf(&g, 1, "Test Bear");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    run_until(&mut g, |g| at_priority(g) && g.pending_decision().unwrap().player == 0, |_| None);
    assert!(g.obj(bear).attacking);
    let c = g.card(0, "Desculpting Blast", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Forest"))), "not a land");
    let tb = g.tgt(bear);
    g.choose(tb);
    run_until(&mut g, at_priority, |_| None);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Hand);
    let drones = tokens_named(&g, 0, "Drone");
    assert_eq!(drones.len(), 1, "{}", log(&g));
    let d = drones[0];
    assert_eq!(g.pt(d), (1, 1));
    assert!(g.is_type(d, types::ARTIFACT) && g.is_creature(d) && subtype(&g, d, "Drone"));
    assert_eq!(g.colors_of(d), 0, "colorless");
    assert!(g.has_kw(d, Kw::Flying));
    assert!(g.def_of(d).blocks_only_flyers, "can block only creatures with flying");
    // Not attacking (an artifact in a main phase).
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &["Desculpting Blast"]).battlefield(1, &["Nutrient Block"]).at(Step::Main1, 0).start();
    let block = bf(&g, 1, "Nutrient Block");
    let tb = g.tgt(block);
    cast_and_resolve(&mut g, 0, "Desculpting Blast", move |g| pick(g, &[tb]));
    assert_eq!(g.obj(block).zone, Zone::Hand, "a nonland permanent");
    assert!(tokens_named(&g, 0, "Drone").is_empty());
}

// ---- Endless Sands (docs/ir_defects.md 61) -----------------------------------

/// Endless Sands: "{2}, {T}: Exile target creature you control. / {4}, {T},
/// Sacrifice this land: Return each creature card exiled with this land to
/// the battlefield under its owner's control." The exile has no duration
/// (CR 607.2a: the cards are linked to the land); only creature cards come
/// back: a token ceases to exist (111.7), and a Spacecraft (an artifact card,
/// a creature only at its threshold, 722.2b) stays exiled (the card's
/// rulings of 2017-07-14).
#[test]
fn cr_607_2a_endless_sands_exiles_your_creatures_and_returns_the_creature_cards() {
    let mut g = scen()
        .battlefield(0, &["Endless Sands", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Test Bear", "Wurmwall Sweeper"])
        .battlefield(1, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let (sands, bear, ship, theirs) = (bf(&g, 0, "Endless Sands"), bf(&g, 0, "Test Bear"), bf(&g, 0, "Wurmwall Sweeper"), bf(&g, 1, "Test Flyer"));
    // Exile the Bear.
    g.choose(act(sands, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert!(!g.has_option(g.tgt(theirs)), "a creature you control");
    assert!(g.has_option(g.tgt(bear)));
    assert!(!g.is_creature(ship) && !g.has_option(g.tgt(ship)), "a Spacecraft below its threshold isn't a creature");
    let tb = g.tgt(bear);
    g.choose(tb);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Exile, "{}", log(&g));
    // It stays exiled while the land stays (no duration), into the next
    // turn.
    quiet_until(&mut g, |g| g.turn().active == 1);
    quiet_until(&mut g, main1_of(0));
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    // Sacrifice the land: the Bear returns under its owner's control.
    g.choose(act(sands, 2));
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(sands).zone, Zone::Graveyard);
    assert!(in_zone(&g, 0, "Test Bear", Zone::Battlefield), "{}", log(&g));
}

/// The cards are linked to the land that exiled them (CR 607.2a): another
/// Endless Sands' return doesn't bring them back (the card's ruling of
/// 2017-07-14: if the land leaves first, they stay exiled).
#[test]
fn cr_607_2a_endless_sands_returns_only_the_cards_it_exiled() {
    let mut g = scen()
        .battlefield(0, &["Endless Sands", "Endless Sands", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let sands: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Endless Sands").collect();
    let bear = bf(&g, 0, "Test Bear");
    g.choose(act(sands[0], 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    let tb = g.tgt(bear);
    g.choose(tb);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    quiet_until(&mut g, |g| g.turn().active == 1);
    quiet_until(&mut g, main1_of(0));
    g.choose(act(sands[1], 2));
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(sands[1]).zone, Zone::Graveyard);
    assert_eq!(g.obj(bear).zone, Zone::Exile, "linked to the other land
{}", log(&g));
}

// ---- Xu-Ifit, Osteoharmonist (docs/ir_defects.md 45) -------------------------

/// Xu-Ifit: "{T}: Return target creature card from your graveyard to the
/// battlefield. It's a Skeleton in addition to its other types and has no
/// abilities. Activate only as a sorcery." The returned creature keeps its
/// types and adds Skeleton (layer 4), has no abilities (layer 6: no flying),
/// and its "when this enters" ability doesn't trigger: it loses it as it
/// enters (the card's ruling of 2025-07-25).
#[test]
fn cr_613_1f_xu_ifit_returns_a_creature_as_a_skeleton_with_no_abilities() {
    let mut g = scen()
        .battlefield(0, &["Xu-Ifit, Osteoharmonist"])
        .graveyard(0, &["Test Flyer", "Galactic Wayfarer", "Forest"])
        .graveyard(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let xu = bf(&g, 0, "Xu-Ifit, Osteoharmonist");
    let (flyer, way, forest, theirs) = (g.card(0, "Test Flyer", Zone::Graveyard), g.card(0, "Galactic Wayfarer", Zone::Graveyard), g.card(0, "Forest", Zone::Graveyard), g.card(1, "Test Bear", Zone::Graveyard));
    g.choose(act(xu, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(flyer)) && g.has_option(g.tgt(way)));
    assert!(!g.has_option(g.tgt(forest)) && !g.has_option(g.tgt(theirs)), "a creature card from your graveyard");
    let tf = g.tgt(flyer);
    g.choose(tf);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    let f = g.card(0, "Test Flyer", Zone::Battlefield);
    assert!(g.is_creature(f) && subtype(&g, f, "Skeleton"), "a Skeleton creature");
    assert!(!g.has_kw(f, Kw::Flying), "no abilities");
    // Next turn: Galactic Wayfarer's enters trigger doesn't make a Lander.
    quiet_until(&mut g, |g| g.turn().active == 1);
    quiet_until(&mut g, main1_of(0));
    let xu = bf(&g, 0, "Xu-Ifit, Osteoharmonist");
    let tw = g.tgt(g.card(0, "Galactic Wayfarer", Zone::Graveyard));
    g.choose(act(xu, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(tw);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    let w = g.card(0, "Galactic Wayfarer", Zone::Battlefield);
    assert!(subtype(&g, w, "Skeleton") && subtype(&g, w, "Human"), "in addition to its other types");
    assert!(tokens_named(&g, 0, "Lander").is_empty(), "its enters ability is lost before it triggers\n{}", log(&g));
}

/// "Enters with a +1/+1 counter" is lost too (the card's ruling of
/// 2025-07-25: abilities that modify how it enters don't apply).
#[test]
fn cr_614_12_xu_ifit_returns_dockworker_drone_without_its_counter() {
    let mut g = scen().battlefield(0, &["Xu-Ifit, Osteoharmonist"]).graveyard(0, &["Dockworker Drone"]).at(Step::Main1, 0).start();
    let xu = bf(&g, 0, "Xu-Ifit, Osteoharmonist");
    let td = g.tgt(g.card(0, "Dockworker Drone", Zone::Graveyard));
    g.choose(act(xu, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(td);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    let d = g.card(0, "Dockworker Drone", Zone::Battlefield);
    assert_eq!(g.counters_on(d, Counter::P1P1), 0, "{}", log(&g));
    assert_eq!(g.pt(d), (1, 1));
}

/// "Activate only as a sorcery" (CR 602.5d): not on the opponent's turn, not
/// with something on the stack; and {T} needs no summoning sickness (302.6).
#[test]
fn cr_602_5d_xu_ifit_only_as_a_sorcery() {
    let g = scen().battlefield(0, &["Xu-Ifit, Osteoharmonist"]).graveyard(0, &["Test Bear"]).at(Step::Main1, 1).start();
    let xu = bf(&g, 0, "Xu-Ifit, Osteoharmonist");
    assert!(!g.has_option(act(xu, 0)), "not on the opponent's turn");
    let g = scen().battlefield(0, &["Xu-Ifit, Osteoharmonist"]).graveyard(0, &["Test Bear"]).at(Step::Main1, 0).start();
    assert!(g.has_option(act(bf(&g, 0, "Xu-Ifit, Osteoharmonist"), 0)));
}

// ---- Syr Vondam, Sunstar Exemplar (docs/ir_defects.md 80) --------------------

/// P0 casts `spell` at `target` (one of P0's or P1's permanents), and the
/// stack resolves; triggered abilities' targets: `trig` if offered.
fn cast_at(g: &mut Game, spell: &str, target: CardIdx, trig: Option<CardIdx>) {
    let t = g.tgt(target);
    let tt = trig.map(|c| g.tgt(c));
    let c = g.card(0, spell, Zone::Hand);
    start_with(g, Choice::Cast(c), move |g| pick(g, &[t]));
    resolve_with(g, move |g| match g.kind() {
        Some(DecisionKind::Target) => tt.filter(|&c| g.has_option(c)),
        _ => None,
    });
}

/// Syr Vondam: "Whenever another creature you control dies or is put into
/// exile, put a +1/+1 counter on Syr Vondam and you gain 1 life." Dying
/// (CR 700.4) or exile from the battlefield triggers it; a return to a
/// hand doesn't (it isn't "dies or is put into exile"), nor an opponent's
/// creature dying.
#[test]
fn cr_700_4_syr_vondam_grows_when_another_creature_you_control_dies_or_is_exiled() {
    for (spell, whose, grows) in [("Bombard", 0, true), ("Gravkill", 0, true), ("Desculpting Blast", 0, false), ("Bombard", 1, false)] {
        let mut g = scen()
            .battlefield(0, &["Syr Vondam, Sunstar Exemplar", "Test Bear", "Mountain", "Mountain", "Mountain", "Swamp", "Swamp", "Swamp", "Swamp", "Island", "Island"])
            .battlefield(1, &["Test Bear"])
            .hand(0, &[spell])
            .at(Step::Main1, 0)
            .start();
        let sv = bf(&g, 0, "Syr Vondam, Sunstar Exemplar");
        let victim = bf(&g, whose, "Test Bear");
        let life = g.player(0).life;
        cast_at(&mut g, spell, victim, None);
        assert_ne!(g.obj(victim).zone, Zone::Battlefield, "{spell}\n{}", log(&g));
        assert_eq!(g.counters_on(sv, Counter::P1P1), grows as u32, "{spell} on P{whose}'s creature\n{}", log(&g));
        assert_eq!(g.player(0).life, life + grows as i32, "{spell}");
    }
}

/// "When Syr Vondam dies or is put into exile while its power is 4 or
/// greater, destroy up to one target nonland permanent." Its power as it
/// last existed on the battlefield (CR 603.10a, 603.6c): with two +1/+1
/// counters (4/4) dying or exiled, it triggers; a 3/3 doesn't; bounced at
/// 4/4, it doesn't (not "dies or is put into exile").
#[test]
fn cr_603_10a_syr_vondam_leaves_with_power_four_or_greater() {
    for (spell, counters, fires) in [("Bombard", 2, true), ("Gravkill", 2, true), ("Bombard", 1, false), ("Desculpting Blast", 2, false)] {
        let mut g = scen()
            .battlefield(0, &["Syr Vondam, Sunstar Exemplar", "Mountain", "Mountain", "Mountain", "Swamp", "Swamp", "Swamp", "Swamp", "Island", "Island"])
            .counters(0, "Syr Vondam, Sunstar Exemplar", Counter::P1P1, counters)
            .battlefield(1, &["Test Flyer", "Forest"])
            .hand(0, &[spell])
            .at(Step::Main1, 0)
            .start();
        let sv = bf(&g, 0, "Syr Vondam, Sunstar Exemplar");
        let (block, forest) = (bf(&g, 1, "Test Flyer"), bf(&g, 1, "Forest"));
        assert_eq!(g.pt(sv), (2 + counters as i32, 2 + counters as i32));
        // (Bombard's 4 damage kills a 4/4 but not a 5/5: the 3/3 and 4/4.)
        cast_at(&mut g, spell, sv, Some(block));
        assert_ne!(g.obj(sv).zone, Zone::Battlefield, "{spell} {counters}\n{}", log(&g));
        assert_eq!(g.obj(block).zone != Zone::Battlefield, fires, "{spell} with {counters} counters: stack {} {:?}\n{}", g.stack().len(), g.kind(), log(&g));
        assert_eq!(g.obj(forest).zone, Zone::Battlefield, "not a land");
    }
}

// ---- Luxknight Breacher (docs/ir_defects.md 21) ------------------------------

/// Luxknight Breacher: "This creature enters with a +1/+1 counter on it for
/// each other creature and/or artifact you control." A replacement (CR
/// 614.1c, 122.6): it is on the battlefield with the counters from the
/// start; an artifact creature counts once, an opponent's permanents and
/// lands don't.
#[test]
fn cr_614_1c_luxknight_breacher_enters_with_a_counter_per_other_creature_and_artifact() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains", "Test Bear", "Nutrient Block", "Dockworker Drone"])
        .battlefield(1, &["Test Bear", "Nutrient Block"])
        .hand(0, &["Luxknight Breacher"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Luxknight Breacher", |_| None);
    let lux = bf(&g, 0, "Luxknight Breacher");
    assert_eq!(g.counters_on(lux, Counter::P1P1), 3, "Bear, Block, Drone (an artifact creature once)\n{}", log(&g));
    assert_eq!(g.pt(lux), (5, 5));
    assert!(g.stack().is_empty(), "no enters trigger");
}

/// With no other creature or artifact, it is a 2/2 with no counters.
#[test]
fn cr_614_1c_luxknight_breacher_alone_has_no_counters() {
    let mut g = scen().battlefield(0, &["Plains"; 4]).hand(0, &["Luxknight Breacher"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Luxknight Breacher", |_| None);
    let lux = bf(&g, 0, "Luxknight Breacher");
    assert_eq!((g.counters_on(lux, Counter::P1P1), g.pt(lux)), (0, (2, 2)));
}

// ---- Mechan Shieldmate (docs/ir_defects.md 24) ------------------------------

/// Mechan Shieldmate: "Defender / As long as an artifact entered the
/// battlefield under your control this turn, this creature can attack as
/// though it didn't have defender." It keeps defender (CR 702.3b); an
/// artifact entering under the opponent's control doesn't count; one that
/// entered and left still does (the card's ruling of 2025-07-25); the
/// permission lasts the turn only.
#[test]
fn cr_702_3b_mechan_shieldmate_attacks_once_an_artifact_entered_under_your_control() {
    let can_attack = |g: &Game, c: CardIdx| g.pending_decision().is_some_and(|d| d.kind == DecisionKind::DeclareAttackers) && g.has_option(Choice::Card(c));
    // No artifact entered: no attack.
    let mut g = scen().battlefield(0, &["Mechan Shieldmate", "Plains"]).at(Step::Main1, 0).start();
    let m = bf(&g, 0, "Mechan Shieldmate");
    assert!(g.has_kw(m, Kw::Defender));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers) || g.turn().step == Step::Main2);
    assert!(!can_attack(&g, m), "defender\n{}", log(&g));
    // An artifact entered under P0's control (then sacrificed for its
    // ability): it can attack, keeping defender.
    let mut g = scen().battlefield(0, &["Mechan Shieldmate", "Plains", "Plains", "Plains"]).hand(0, &["Nutrient Block"]).at(Step::Main1, 0).start();
    let m = bf(&g, 0, "Mechan Shieldmate");
    cast_and_resolve(&mut g, 0, "Nutrient Block", |_| None);
    let nb = bf(&g, 0, "Nutrient Block");
    start_with(&mut g, act(nb, 0), |_| None);
    settle(&mut g);
    assert_ne!(g.obj(nb).zone, Zone::Battlefield, "sacrificed\n{}", log(&g));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(can_attack(&g, m), "an artifact entered this turn\n{}", log(&g));
    assert!(g.has_kw(m, Kw::Defender), "it still has defender");
    g.choose(Choice::Card(m));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.player(1).life, 17, "3 damage\n{}", log(&g));
    // Its next turn: no artifact entered, no attack.
    quiet_until(&mut g, |g| g.turn().active == 1);
    quiet_until(&mut g, |g| g.turn().active == 0 && (g.kind() == Some(DecisionKind::DeclareAttackers) || g.turn().step == Step::Main2));
    assert!(!can_attack(&g, m), "the permission was this turn's\n{}", log(&g));
    // An artifact entering under the opponent's control doesn't count.
    let mut g = scen().battlefield(0, &["Mechan Shieldmate"]).battlefield(1, &["Plains"]).hand(1, &["Nutrient Block"]).at(Step::Main1, 1).start();
    cast_and_resolve(&mut g, 1, "Nutrient Block", |_| None);
    assert!(g.player(1).artifact_entered && !g.player(0).artifact_entered);
}

// ---- Mutavault (docs/ir_defects.md 41) ---------------------------------------

/// Mutavault: "{T}: Add {C}. / {1}: This land becomes a 2/2 creature with all
/// creature types until end of turn. It's still a land." Every creature type
/// (CR 205.3m: a Knight, a Robot, a Skeleton) but no other subtype (not a
/// Spacecraft, an artifact type, 205.3g; it has no changeling, its ruling of
/// 2008-04-01); until end of turn.
#[test]
fn cr_205_3m_mutavault_becomes_a_2_2_with_all_creature_types() {
    let mut g = scen().battlefield(0, &["Mutavault", "Plains"]).at(Step::Main1, 0).start();
    let mv = bf(&g, 0, "Mutavault");
    assert!(!g.is_creature(mv));
    assert!(!subtype(&g, mv, "Knight"));
    start_with(&mut g, act(mv, 1), |_| None);
    settle(&mut g);
    assert!(g.is_creature(mv) && g.is_type(mv, types::LAND), "a land creature\n{}", log(&g));
    assert_eq!(g.pt(mv), (2, 2));
    for s in ["Knight", "Robot", "Skeleton", "Human", "Drone"] {
        assert!(subtype(&g, mv, s), "{s}");
    }
    assert!(!subtype(&g, mv, "Spacecraft"), "not an artifact type");
    assert!(!g.has_kw(mv, Kw::Flying));
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert!(!g.is_creature(mv) && !subtype(&g, mv, "Knight"), "until end of turn");
}

// ---- Mirrorpool (docs/ir_defects.md 67) --------------------------------------

/// Mirrorpool's costs have a colorless pip: "{2}{C}" and "{4}{C}" (CR
/// 107.4b: only colorless mana pays {C}), and "{T}: Add {C}."; it enters
/// tapped.
#[test]
fn cr_107_4b_mirrorpool_costs_need_colorless_mana() {
    let g = scen().battlefield(0, &["Mirrorpool"]).at(Step::Main1, 0).start();
    let mp = bf(&g, 0, "Mirrorpool");
    let d = g.def_of(mp);
    let costs: Vec<engine::mana::ManaCost> = d.activated.iter().filter(|a| a.cost.sac_self).map(|a| a.cost.mana).collect();
    assert_eq!(costs, vec![engine::mana::ManaCost::parse("{2}{C}").unwrap(), engine::mana::ManaCost::parse("{4}{C}").unwrap()]);
    // Three Plains and Mirrorpool can't pay {2}{C} (Mirrorpool's own {C}
    // can't: it taps as a cost too).
    let g = scen().battlefield(0, &["Mirrorpool", "Plains", "Plains", "Plains", "Test Bear"]).at(Step::Main1, 0).start();
    let mp = bf(&g, 0, "Mirrorpool");
    assert!(!g.has_option(act(mp, 2)), "no colorless mana for {{4}}{{C}}");
}

/// "{4}{C}, {T}, Sacrifice this land: Create a token that's a copy of target
/// creature you control." The token copies the creature's copiable values
/// (its counters aren't copied: the card's ruling of 2016-01-22).
#[test]
fn cr_707_2_mirrorpool_makes_a_token_copy_of_your_creature() {
    let mut g = scen()
        .battlefield(0, &["Mirrorpool", "Plains", "Plains", "Plains", "Plains", "Bonders' Enclave", "Test Bear"])
        .counters(0, "Test Bear", Counter::P1P1, 2)
        .battlefield(1, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let (mp, bear, theirs) = (bf(&g, 0, "Mirrorpool"), bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Flyer"));
    g.choose(act(mp, 2));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert!(!g.has_option(g.tgt(theirs)), "a creature you control");
    let tb = g.tgt(bear);
    g.choose(tb);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(mp).zone, Zone::Graveyard, "sacrificed\n{}", log(&g));
    let copies: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Test Bear" && c != bear).collect();
    assert_eq!(copies.len(), 1, "{}", log(&g));
    assert!(g.is_token(copies[0]), "a token");
    assert_eq!(g.pt(copies[0]), (g.def_of(bear).power.unwrap(), g.def_of(bear).toughness.unwrap()), "no counters copied");
}

/// "{2}{C}, {T}, Sacrifice this land: Copy target instant or sorcery spell you
/// control. You may choose new targets for the copy." (CR 707.10)
#[test]
fn cr_707_10_mirrorpool_copies_your_instant() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Plains", "Plains", "Bonders' Enclave", "Mirrorpool"])
        .battlefield(1, &["Test Bear", "Test Flyer"])
        .hand(0, &["Bombard"])
        .at(Step::Main1, 0)
        .start();
    let (mp, bear, flyer) = (bf(&g, 0, "Mirrorpool"), bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Flyer"));
    let c = g.card(0, "Bombard", Zone::Hand);
    let tb = g.tgt(bear);
    start_with(&mut g, Choice::Cast(c), move |g| pick(g, &[tb]));
    assert_eq!(g.stack().len(), 1);
    let tf = g.tgt(flyer);
    start_with(&mut g, act(mp, 1), |_| None);
    let mut asked = false;
    for _ in 0..200 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        if at_priority(&g) {
            g.pass();
            continue;
        }
        let c = match g.kind() {
            // "You may choose new targets for the copy" (CR 707.10c).
            Some(DecisionKind::NewTarget) if g.has_option(tf) => {
                asked = true;
                tf
            }
            _ => g.pending_decision().unwrap().options[0].choice,
        };
        g.choose(c);
    }
    assert!(asked, "a new target may be chosen\n{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "the original\n{}", log(&g));
    assert_eq!(g.obj(flyer).zone, Zone::Graveyard, "the copy's new target\n{}", log(&g));
}

// ---- Pinnacle Starcage (docs/ir_defects.md 65) -------------------------------

/// Pinnacle Starcage: "When this artifact enters, exile all artifacts and
/// creatures with mana value 2 or less until this artifact leaves the
/// battlefield." Both players' (CR 610.3); a 3-drop and lands stay; a token
/// exiled ceases to exist (the card's rulings of 2025-07-25). When it
/// leaves, the cards return.
#[test]
fn cr_610_3_pinnacle_starcage_exiles_small_artifacts_and_creatures_until_it_leaves() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Test Bear", "Nutrient Block"])
        .battlefield(1, &["Test Flyer", "Hullcarver", "Forest"])
        .hand(0, &["Pinnacle Starcage"])
        .at(Step::Main1, 0)
        .start();
    let (bear, block, flyer, big) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Nutrient Block"), bf(&g, 1, "Test Flyer"), bf(&g, 1, "Hullcarver"));
    let mvs: Vec<u8> = [bear, block, flyer, big].iter().map(|&c| g.def_of(c).mana_value()).collect();
    cast_and_resolve(&mut g, 0, "Pinnacle Starcage", |_| None);
    for (c, mv) in [bear, block, flyer, big].into_iter().zip(mvs) {
        assert_eq!(g.obj(c).zone == Zone::Exile, mv <= 2, "{} (mana value {mv})\n{}", g.name(c), log(&g));
    }
    assert!(in_zone(&g, 0, "Pinnacle Starcage", Zone::Battlefield));
    assert!(in_zone(&g, 1, "Forest", Zone::Battlefield));
}

/// "{6}{W}{W}: Put each card exiled with this artifact into its owner's
/// graveyard, then create a 2/2 colorless Robot artifact creature token for
/// each card put into a graveyard this way. Sacrifice this artifact." The
/// cards go to their owners' graveyards (not back); the Robots are the
/// printed token (CR 111.4).
#[test]
fn cr_607_2a_pinnacle_starcage_turns_the_exiled_cards_into_robots() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 11])
        .battlefield(0, &["Test Bear"])
        .battlefield(1, &["Test Flyer"])
        .hand(0, &["Pinnacle Starcage"])
        .at(Step::Main1, 0)
        .start();
    let (bear, flyer) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Flyer"));
    cast_and_resolve(&mut g, 0, "Pinnacle Starcage", |_| None);
    assert_eq!((g.obj(bear).zone, g.obj(flyer).zone), (Zone::Exile, Zone::Exile));
    let ps = bf(&g, 0, "Pinnacle Starcage");
    start_with(&mut g, act(ps, 0), |_| None);
    settle(&mut g);
    assert_eq!(g.obj(ps).zone, Zone::Graveyard, "sacrificed\n{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "to its owner's graveyard, not back");
    assert_eq!(g.obj(flyer).zone, Zone::Graveyard);
    let robots = tokens_named(&g, 0, "Robot");
    assert_eq!(robots.len(), 2, "{}", log(&g));
    for r in robots {
        assert_eq!(g.pt(r), (2, 2));
        assert!(g.is_type(r, types::ARTIFACT) && g.is_creature(r) && subtype(&g, r, "Robot"));
        assert_eq!(g.colors_of(r), 0, "colorless");
    }
}

// ---- Meltstrider's Resolve (docs/ir_defects.md 28) ---------------------------

/// Meltstrider's Resolve: "Enchant creature you control / When this Aura
/// enters, enchanted creature fights up to one target creature an opponent
/// controls. / Enchanted creature gets +0/+2 and can't be blocked by more
/// than one creature." The fight (CR 701.14a); +0/+2.
#[test]
fn cr_701_14a_meltstriders_resolve_fights_and_gives_plus_zero_two() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Test Bear"])
        .battlefield(1, &["Test Flyer"])
        .hand(0, &["Meltstrider's Resolve"])
        .at(Step::Main1, 0)
        .start();
    let (bear, flyer) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Flyer"));
    let (tb, tf) = (g.tgt(bear), g.tgt(flyer));
    let bear_pt = g.pt(bear);
    let flyer_pt = g.pt(flyer);
    let c = g.card(0, "Meltstrider's Resolve", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), move |g| pick(g, &[tb]));
    resolve_with(&mut g, move |g| match g.kind() {
        Some(DecisionKind::Target) => pick(g, &[tf]),
        _ => None,
    });
    assert_eq!(g.pt(bear), (bear_pt.0, bear_pt.1 + 2), "+0/+2");
    assert_eq!(g.obj(flyer).zone == Zone::Battlefield, bear_pt.0 < flyer_pt.1, "the Bear dealt its power\n{}", log(&g));
    assert_eq!(g.obj(bear).damage, flyer_pt.0 as u32, "the Flyer dealt its power\n{}", log(&g));
}

/// "Can't be blocked by more than one creature" (CR 509.1b; not menace): one
/// blocker may block it, a second can't join.
#[test]
fn cr_509_1b_meltstriders_resolve_allows_only_one_blocker() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Test Bear"])
        .battlefield(1, &["Test Flyer", "Test Wall"])
        .hand(0, &["Meltstrider's Resolve"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let tb = g.tgt(bear);
    let c = g.card(0, "Meltstrider's Resolve", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), move |g| pick(g, &[tb]));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Pass));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    run_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers), |_| None);
    assert!(g.has_option(Choice::Card(bear)), "the first blocker may block it");
    g.choose(Choice::Card(bear));
    assert_eq!(g.kind(), Some(DecisionKind::DeclareBlockers), "the second blocker's turn");
    assert!(!g.has_option(Choice::Card(bear)), "a second can't join\n{}", log(&g));
}

// ---- Sothera, the Supervoid (docs/ir_defects.md 180, W4's entry) -------------

/// Sothera: "Whenever a creature you control dies, each opponent chooses a
/// creature they control and exiles it. / At the beginning of your end step,
/// if a player controls no creatures, sacrifice Sothera, then put a creature
/// card exiled with it onto the battlefield under your control with two
/// additional +1/+1 counters on it." The opponent chooses (CR 608.2d); the
/// end-step ability checks its "if" (603.4); Sothera is sacrificed first, the
/// card still linked to it (607.2a), and the creature enters under its
/// controller's control with the counters (614.1c).
#[test]
fn cr_607_2a_sothera_exiles_then_sacrifices_and_returns_a_creature_with_counters() {
    let mut g = scen()
        .battlefield(0, &["Sothera, the Supervoid", "Test Bear", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Test Flyer", "Test Wall"])
        .hand(0, &["Bombard"])
        .at(Step::Main1, 0)
        .start();
    let (bear, flyer, wall) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Flyer"), bf(&g, 1, "Test Wall"));
    let tb = g.tgt(bear);
    let c = g.card(0, "Bombard", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), move |g| pick(g, &[tb]));
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChoosePermanent) {
            let d = g.pending_decision().unwrap();
            assert!(d.options.iter().all(|o| matches!(o.choice, Choice::Card(x) if g.obj(x).controller == 1)), "a creature they control");
            return Some(Choice::Card(flyer));
        }
        None
    });
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(flyer).zone, Zone::Exile, "the opponent exiled their chosen creature\n{}", log(&g));
    assert_eq!(g.obj(wall).zone, Zone::Battlefield);
    // Both players control creatures? P0 none: the end-step ability triggers.
    quiet_until(&mut g, |g| g.turn().step == Step::Cleanup || g.turn().active == 1);
    let sothera = g.card(0, "Sothera, the Supervoid", Zone::Graveyard);
    assert_eq!(g.obj(sothera).zone, Zone::Graveyard, "sacrificed\n{}", log(&g));
    let f = g.card(1, "Test Flyer", Zone::Battlefield);
    assert_eq!(g.obj(f).controller, 0, "under Sothera's controller's control");
    assert_eq!(g.counters_on(f, Counter::P1P1), 2, "two additional +1/+1 counters\n{}", log(&g));
}

/// "If a player controls no creatures": with creatures on both sides the
/// end-step ability doesn't trigger (CR 603.4).
#[test]
fn cr_603_4_sothera_stays_while_both_players_control_creatures() {
    let mut g = scen().battlefield(0, &["Sothera, the Supervoid", "Test Bear"]).battlefield(1, &["Test Flyer"]).at(Step::Main1, 0).start();
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert!(in_zone(&g, 0, "Sothera, the Supervoid", Zone::Battlefield));
}
