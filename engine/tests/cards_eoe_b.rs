//! Edge of Eternities lane B card tests (ENGINE.md 99.3): Lander tokens
//! (CR 111.10), the landfall and land cards, lane B's mana abilities (CR
//! 605, 106.7), each against its Oracle text; the features of the cards the
//! IR defects block, on custom IR test cards (`eoe_b::lane_b_cards`).

mod eoe_b;

use engine::db::{types, Counter, Kw};
use engine::mana::ManaCost;
use engine::{Choice, DecisionKind, Game, Step, Target, Zone};
use eoe_b::*;

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

/// A scenario whose P0 library is exactly `lib` (top first).
fn scen_lib(lib: &[&str]) -> engine::testkit::Scenario {
    engine::testkit::Scenario::new(db()).spec(|s| s.auto_single = false).library(0, lib).library(1, &["Forest"; 10])
}

fn lander_of(g: &Game, p: u8) -> Option<engine::CardIdx> {
    g.permanents_of(p).find(|&c| g.name(c) == "Lander")
}

// ---- the Lander token (CR 111.10) --------------------------------------------

/// Kav Landseeker: "When this creature enters, create a Lander token." The
/// token: a colorless artifact named Lander with the subtype Lander and
/// "{2}, {T}, Sacrifice this token: Search your library for a basic land
/// card, put it onto the battlefield tapped, then shuffle."
#[test]
fn cr_111_10_the_lander_token_searches_a_basic_land_onto_the_battlefield_tapped() {
    let mut g = scen_lib(&["Forest", "Bombard", "Island"])
        .hand(0, &["Kav Landseeker"])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Kav Landseeker", |_| None);
    let kav = bf(&g, 0, "Kav Landseeker");
    assert!(g.has_kw(kav, Kw::Menace), "Kav Landseeker has menace");
    let lander = lander_of(&g, 0).expect("a Lander token");
    let d = g.def_of(lander);
    assert!(d.token && d.is(types::ARTIFACT) && !d.is(types::CREATURE));
    assert_eq!(g.colors_of(lander), 0, "colorless");
    assert!(subtype(&g, lander, "Lander"));
    assert_eq!(d.activated.len(), 1);
    let a = &d.activated[0];
    assert!(a.cost.tap && a.cost.sac_self && a.cost.mana == cost("{2}"), "{{2}}, {{T}}, Sacrifice this token");
    // Activate: search for a basic land (a choice among the library's basic
    // lands, by definition), onto the battlefield tapped.
    let forests_before = count_named(&g, 0, "Forest");
    start_with(&mut g, act(lander, 0), |_| None);
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| pick(g, &[Choice::Def(def("Forest"))])).flatten()
    });
    assert!(lander_of(&g, 0).is_none(), "sacrificed");
    assert_eq!(count_named(&g, 0, "Forest"), forests_before + 1);
    let forest = g.permanents_of(0).find(|&c| g.name(c) == "Forest").unwrap();
    assert!(g.obj(forest).tapped, "put onto the battlefield tapped");
    assert_eq!(g.player(0).library.len(), 2, "the Forest left the library");
}

/// The Lander's {T}: a tapped Lander can't be activated (CR 602.5a, 302.6
/// for a creature; the IR's printings drop it, docs/ir_defects.md 42).
#[test]
fn a_tapped_lander_cant_activate_its_ability() {
    let mut g = scen()
        .hand(0, &["Kav Landseeker", "Forest"])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Kav Landseeker", |_| None);
    let lander = lander_of(&g, 0).unwrap();
    assert!(g.has_option(act(lander, 0)));
    // (Tapped some other way: the test sets it tapped, then plays a land so
    // that priority's options are made again.)
    g.set_tapped(lander, true);
    play_land(&mut g, 0, "Forest", |_| None);
    assert!(!g.has_option(act(lander, 0)), "{{T}} in its cost");
}

/// The custom card with the IR's Lander printing once its {T} is restored:
/// the same token definition as the CR's (the 13 `CreateToken` cards will
/// lower to it).
#[test]
fn the_fixed_ir_lander_printing_is_the_rules_lander() {
    let mut g = scen().hand(0, &["Test Lander Maker", "Kav Landseeker"]).battlefield(0, &["Forest", "Forest", "Mountain", "Mountain", "Mountain", "Mountain"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Test Lander Maker", |_| None);
    cast_and_resolve(&mut g, 0, "Kav Landseeker", |_| None);
    let landers: Vec<_> = g.permanents_of(0).filter(|&c| g.name(c) == "Lander").collect();
    assert_eq!(landers.len(), 2);
    assert_eq!(g.obj(landers[0]).def, g.obj(landers[1]).def, "one token definition");
}

/// Kav Landseeker: "At the beginning of the end step on your next turn,
/// sacrifice that token" (a delayed triggered ability, CR 603.7): not this
/// turn's end step, not the opponent's.
#[test]
fn cr_603_7_kav_landseeker_sacrifices_its_lander_at_the_end_step_of_your_next_turn() {
    let mut g = scen()
        .hand(0, &["Kav Landseeker"])
        .battlefield(0, &["Mountain"; 4])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Kav Landseeker", |_| None);
    assert!(lander_of(&g, 0).is_some());
    quiet_until(&mut g, main1_of(1));
    assert!(lander_of(&g, 0).is_some(), "still there after this turn's end step");
    quiet_until(&mut g, main1_of(0));
    assert!(lander_of(&g, 0).is_some(), "still there after the opponent's turn");
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Cleanup);
    assert!(lander_of(&g, 0).is_none(), "sacrificed at the end step of your next turn");
    assert!(g.graveyard_cards(0).all(|c| g.name(c) != "Lander"), "a token ceases to exist (CR 111.7)");
}

/// Emergency Eject: "Destroy target nonland permanent. Its controller
/// creates a Lander token."
#[test]
fn emergency_eject_destroys_a_nonland_permanent_and_its_controller_gets_a_lander() {
    let mut g = scen()
        .hand(0, &["Emergency Eject"])
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .battlefield(1, &["Test Bear", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let forest = bf(&g, 1, "Forest");
    let c = g.card(0, "Emergency Eject", Zone::Hand);
    g.choose(Choice::Cast(c));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(forest)), "nonland");
    let t = g.tgt(bear);
    g.choose(t);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert!(lander_of(&g, 1).is_some(), "its controller creates the Lander");
    assert!(lander_of(&g, 0).is_none());
}

/// Orbital Plunge: "deals 6 damage to target creature. If excess damage was
/// dealt this way, create a Lander token" (CR 120.10: damage beyond lethal).
#[test]
fn cr_120_10_orbital_plunge_makes_a_lander_only_on_excess_damage() {
    for (target, excess) in [("Test Bear", true), ("Test Trampler", true)] {
        let mut g = scen()
            .hand(0, &["Orbital Plunge"])
            .battlefield(0, &["Mountain"; 4])
            .battlefield(1, &[target])
            .at(Step::Main1, 0)
            .start();
        let t = bf(&g, 1, target);
        let tc = g.tgt(t);
        cast_and_resolve(&mut g, 0, "Orbital Plunge", move |g| pick(g, &[tc]));
        assert_eq!(g.obj(t).zone, Zone::Graveyard, "{target} dies");
        assert_eq!(lander_of(&g, 0).is_some(), excess, "{target}");
    }
    // Exactly lethal: 6 damage to a creature with 6 toughness.
    let mut g = scen()
        .hand(0, &["Orbital Plunge"])
        .battlefield(0, &["Mountain"; 6])
        .battlefield(1, &["Harmonious Grovestrider", "Forest", "Forest", "Forest", "Forest", "Forest", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let h = bf(&g, 1, "Harmonious Grovestrider");
    assert_eq!(g.pt(h), (6, 6));
    let tc = g.tgt(h);
    // (Ward {2}: pay it.)
    cast_and_resolve(&mut g, 0, "Orbital Plunge", move |g| pick(g, &[tc, Choice::Yes]));
    assert_eq!(g.obj(h).zone, Zone::Graveyard, "ward paid, 6 damage dealt");
    assert!(lander_of(&g, 0).is_none(), "6 damage to a 6-toughness creature: no excess");
}

// ---- lands -------------------------------------------------------------------

/// Lotus Field: "Hexproof / This land enters tapped. / When this land enters,
/// sacrifice two lands. / {T}: Add three mana of any one color."
#[test]
fn lotus_field_enters_tapped_sacrifices_two_lands_and_taps_for_three_of_one_color() {
    let mut g = scen().hand(0, &["Lotus Field"]).battlefield(0, &["Forest", "Island", "Swamp"]).at(Step::Main1, 0).start();
    let swamp = bf(&g, 0, "Swamp");
    play_land(&mut g, 0, "Lotus Field", |g| {
        // Keep the Swamp: sacrifice the other two (one decision each).
        let opts: Vec<Choice> = g.pending_decision()?.options.iter().map(|o| o.choice).filter(|&c| c != Choice::Card(swamp)).collect();
        opts.first().copied()
    });
    let lotus = bf(&g, 0, "Lotus Field");
    assert!(g.obj(lotus).tapped, "enters tapped");
    assert!(g.has_kw(lotus, Kw::Hexproof));
    assert_eq!(g.permanents_of(0).count(), 2, "two lands sacrificed");
    assert_eq!(g.obj(swamp).zone, Zone::Battlefield);
    g.set_tapped(lotus, false);
    assert!(g.can_pay(0, &cost("{G}{G}{G}")), "three of one colour");
    assert!(!g.can_pay(0, &cost("{G}{G}{U}{B}")), "not of two colours (the Swamp makes the fourth)");
    assert!(g.can_pay(0, &cost("{U}{U}{U}{B}")));
}

/// Lotus Field may be sacrificed to its own trigger (CR 701.21a: any two
/// lands its controller controls).
#[test]
fn cr_701_21a_lotus_field_can_be_one_of_the_two_lands() {
    let mut g = scen().hand(0, &["Lotus Field"]).battlefield(0, &["Forest"]).at(Step::Main1, 0).start();
    play_land(&mut g, 0, "Lotus Field", |_| None);
    assert_eq!(g.permanents_of(0).count(), 0, "with two lands, both go");
}

/// Meteor Crater: "{T}: Choose a color of a permanent you control. Add one
/// mana of that color." (CR 105.2: colorless is no colour.)
#[test]
fn meteor_crater_makes_a_colour_of_a_permanent_you_control() {
    let g = scen().battlefield(0, &["Meteor Crater"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{1}")), "no coloured permanent: no mana");
    let g = scen().battlefield(0, &["Meteor Crater", "Test Bear"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{G}")));
    assert!(!g.can_pay(0, &cost("{U}")));
    let g = scen().battlefield(0, &["Meteor Crater", "Mm'menon, Uthros Exile"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{U}")) && g.can_pay(0, &cost("{R}")) && !g.can_pay(0, &cost("{W}")));
    let g = scen().battlefield(0, &["Meteor Crater"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{G}")), "a permanent you control");
}

/// Reflecting Pool: "{T}: Add one mana of any type that a land you control
/// could produce" (CR 106.7; two Reflecting Pools alone make nothing).
#[test]
fn cr_106_7_reflecting_pool_makes_what_your_lands_could_produce() {
    let g = scen().battlefield(0, &["Reflecting Pool"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{1}")));
    let g = scen().battlefield(0, &["Reflecting Pool", "Reflecting Pool"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{1}")), "two pools alone: no type");
    let g = scen().battlefield(0, &["Reflecting Pool", "Reflecting Pool", "Island"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{U}{U}{U}")));
    assert!(!g.can_pay(0, &cost("{G}")));
    // A tapped land still "could produce" (costs ignored).
    let mut g = scen().battlefield(0, &["Reflecting Pool", "Forest", "Strip Mine"]).at(Step::Main1, 0).start();
    let forest = bf(&g, 0, "Forest");
    g.set_tapped(forest, true);
    assert!(g.can_pay(0, &cost("{G}{C}")), "the pool makes {{G}} from the tapped Forest");
    assert!(g.can_pay(0, &cost("{C}{C}")), "or {{C}} (Strip Mine's)");
    let g = scen().battlefield(0, &["Reflecting Pool"]).battlefield(1, &["Forest"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{1}")), "lands you control");
}

/// Grove of the Burnwillows: "{T}: Add {C}. / {T}: Add {R} or {G}. Each
/// opponent gains 1 life."
#[test]
fn grove_of_the_burnwillows_coloured_mana_gives_each_opponent_a_life() {
    for (pay, opp_life) in [("{C}", 20), ("{R}", 21), ("{G}", 21)] {
        let mut g = scen().battlefield(0, &["Grove of the Burnwillows"]).hand(0, &["Test Zero"]).at(Step::Main1, 0).start();
        assert!(g.can_pay(0, &cost(pay)));
        let grove = bf(&g, 0, "Grove of the Burnwillows");
        let i = if pay == "{C}" { 0 } else { 1 };
        // Cast a {1} spell and pay it with the ability (mana abilities are
        // activated while paying, CR 605.3a); {R} or {G} is the second choice.
        let z = g.card(0, "Test Zero", Zone::Hand);
        g.choose(Choice::Cast(z));
        g.choose(act(grove, i));
        if g.kind() == Some(DecisionKind::ChooseMana) {
            let k = if pay == "{G}" { 1 } else { 0 };
            let c = g.pending_decision().unwrap().options[k].choice;
            g.choose(c);
        }
        g.drive_until(at_priority);
        assert_eq!(g.life(1), opp_life, "{pay}");
        assert_eq!(g.life(0), 20);
    }
}

/// Harmonious Grovestrider: "Ward {2} / ... power and toughness are each
/// equal to the number of lands you control" (CR 604.3, everywhere).
#[test]
fn cr_604_3_harmonious_grovestriders_pt_is_the_lands_you_control() {
    let mut g = scen()
        .battlefield(0, &["Harmonious Grovestrider", "Forest", "Forest", "Island"])
        .battlefield(1, &["Mountain"])
        .hand(0, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let h = bf(&g, 0, "Harmonious Grovestrider");
    assert_eq!(g.pt(h), (3, 3));
    play_land(&mut g, 0, "Forest", |_| None);
    assert_eq!(g.pt(h), (4, 4));
    assert!(g.def_of(h).ward.is_some(), "ward {{2}}");
}

/// Fungal Colossus: "This spell costs {X} less to cast, where X is the
/// number of differently named lands you control" (CR 601.2f).
#[test]
fn fungal_colossus_costs_one_less_per_differently_named_land() {
    let g = scen()
        .battlefield(0, &["Forest", "Forest", "Island", "Petrified Field"])
        .hand(0, &["Fungal Colossus"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Fungal Colossus", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{{6}}{{G}} minus 3 = {{3}}{{G}}, four lands");
    let g = scen().battlefield(0, &["Forest", "Forest", "Forest", "Forest"]).hand(0, &["Fungal Colossus"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Fungal Colossus", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "four Forests are one name: {{5}}{{G}}");
}

/// Petrified Field: "{T}: Add {C}. / {T}, Sacrifice this land: Return target
/// land card from your graveyard to your hand."
#[test]
fn petrified_field_returns_a_land_card_from_your_graveyard() {
    let mut g = scen()
        .battlefield(0, &["Petrified Field"])
        .graveyard(0, &["Lotus Field", "Bombard"])
        .graveyard(1, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    assert!(g.can_pay(0, &cost("{C}")));
    let pf = bf(&g, 0, "Petrified Field");
    let lotus = g.card(0, "Lotus Field", Zone::Graveyard);
    let bombard = g.card(0, "Bombard", Zone::Graveyard);
    let theirs = g.card(1, "Forest", Zone::Graveyard);
    g.choose(act(pf, 1));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bombard)) && !g.has_option(g.tgt(theirs)), "a land card in your graveyard");
    let t = g.tgt(lotus);
    g.choose(t);
    settle(&mut g);
    assert_eq!(g.obj(lotus).zone, Zone::Hand);
    assert_eq!(g.obj(pf).zone, Zone::Graveyard);
}

// ---- landfall ----------------------------------------------------------------

/// Tannuk, Memorial Ensign: "Landfall — ... deals 1 damage to each opponent.
/// If this is the second time this ability has resolved this turn, draw a
/// card." (Only the second, not the third.)
#[test]
fn tannuk_draws_on_the_second_landfall_resolution_only() {
    let mut g = scen()
        .battlefield(0, &["Tannuk, Memorial Ensign", "Test Temple"])
        .hand(0, &["Forest", "Bombard"])
        .battlefield(0, &["Icetill Explorer"])
        .graveyard(0, &["Mountain", "Island"])
        .at(Step::Main1, 0)
        .start();
    let hand0 = g.player(0).hand.len();
    play_land(&mut g, 0, "Forest", |_| None);
    assert_eq!(g.life(1), 19);
    assert_eq!(g.player(0).hand.len(), hand0 - 1, "first: no draw");
    // Icetill Explorer: an additional land, from the graveyard.
    let m = g.card(0, "Mountain", Zone::Graveyard);
    g.choose(Choice::Play(m));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(1), 18);
    assert_eq!(g.player(0).hand.len(), hand0, "second: draw a card");
}

/// Territorial Bruntar: "Reach / Landfall — ... exile cards from the top of
/// your library until you exile a nonland card. You may cast that card this
/// turn."
#[test]
fn territorial_bruntar_exiles_to_a_nonland_card_castable_this_turn() {
    let mut g = scen_lib(&["Island", "Swamp", "Bombard", "Plains"])
        .battlefield(0, &["Territorial Bruntar", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Forest"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let br = bf(&g, 0, "Territorial Bruntar");
    assert!(g.has_kw(br, Kw::Reach));
    play_land(&mut g, 0, "Forest", |_| None);
    let bombard = g.card(0, "Bombard", Zone::Exile);
    assert!(g.find_card(0, "Island", Zone::Exile).is_some() && g.find_card(0, "Swamp", Zone::Exile).is_some(), "the lands above it are exiled too");
    assert!(g.find_card(0, "Plains", Zone::Library).is_some());
    assert_eq!(g.player(0).library.len(), 1, "stops at the nonland card");
    assert!(g.has_option(Choice::Cast(bombard)), "castable this turn");
    // Not on a later turn.
    quiet_until(&mut g, main1_of(1));
    quiet_until(&mut g, main1_of(0));
    assert!(!g.has_option(Choice::Cast(bombard)), "this turn only");
}

// ---- lane B features on the custom cards (IR-blocked cards' shapes) ---------

/// "When this creature enters, if an opponent controls more lands than you,
/// create a Lander token" (CR 603.4: checked as it triggers and resolves).
#[test]
fn cr_603_4_if_an_opponent_controls_more_lands_than_you() {
    for (opp_lands, makes) in [(3usize, true), (2, false)] {
        let mut g = scen()
            .hand(0, &["Test Expansionist"])
            .battlefield(0, &["Plains", "Plains"])
            .battlefield(1, &vec!["Forest"; opp_lands])
            .at(Step::Main1, 0)
            .start();
        cast_and_resolve(&mut g, 0, "Test Expansionist", |_| None);
        assert_eq!(lander_of(&g, 0).is_some(), makes, "opponent has {opp_lands} lands, you 2");
    }
}

/// "{1}, {T}: Untap target land." and "{2}, {T}: You may put a basic land
/// card from your hand onto the battlefield tapped."
#[test]
fn untap_target_land_and_put_a_basic_land_from_hand_tapped() {
    let mut g = scen().battlefield(0, &["Test Temple", "Forest", "Island"]).at(Step::Main1, 0).start();
    let (temple, forest) = (bf(&g, 0, "Test Temple"), bf(&g, 0, "Forest"));
    g.set_tapped(forest, true);
    let tf = g.tgt(forest);
    start_with(&mut g, act(temple, 1), move |g| pick(g, &[tf]));
    settle(&mut g);
    assert!(!g.obj(forest).tapped && g.obj(temple).tapped);

    let mut g = scen().battlefield(0, &["Test Generator", "Forest", "Island"]).hand(0, &["Swamp", "Bombard"]).at(Step::Main1, 0).start();
    let gen = bf(&g, 0, "Test Generator");
    let swamp = g.card(0, "Swamp", Zone::Hand);
    start_with(&mut g, act(gen, 1), |_| None);
    resolve_with(&mut g, move |g| pick(g, &[Choice::Card(swamp)]));
    assert_eq!(g.obj(swamp).zone, Zone::Battlefield);
    assert!(g.obj(swamp).tapped);
    let _ = (Counter::P1P1, Target::Player(0));
}

// ---- manlands with granted abilities -----------------------------------------

/// Lavaclaw Reaches: "{1}{B}{R}: Until end of turn, this land becomes a 2/2
/// black and red Elemental creature with '{X}: This creature gets +X/+0
/// until end of turn.' It's still a land." The granted ability exists only
/// while the effect lasts (layer 6).
#[test]
fn lavaclaw_reaches_animates_with_an_x_pump() {
    let mut g = scen().battlefield(0, &["Lavaclaw Reaches", "Swamp", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"]).at(Step::Main1, 0).start();
    let l = bf(&g, 0, "Lavaclaw Reaches");
    let n = g.def_of(l).activated.len() as u8;
    assert!(!g.has_option(act(l, n - 1)), "no {{X}} ability before it animates");
    let i = (0..n).find(|&i| g.def_of(l).activated[i as usize].cost.mana == cost("{1}{B}{R}")).unwrap();
    start_with(&mut g, act(l, i), |_| None);
    settle(&mut g);
    assert_eq!(g.pt(l), (2, 2));
    assert_eq!(g.colors_of(l), (1 << engine::mana::B) | (1 << engine::mana::R));
    assert!(g.is_type(l, types::LAND) && subtype(&g, l, "Elemental"));
    start_with(&mut g, act(l, n - 1), |g| pick(g, &[Choice::Number(3)]));
    settle(&mut g);
    assert_eq!(g.pt(l), (5, 2), "+X/+0 with X = 3");
    quiet_until(&mut g, main1_of(1));
    assert!(!g.is_creature(l));
}

/// Wandering Fumarole: "... a 1/4 blue and red Elemental creature with '{0}:
/// Switch this creature's power and toughness until end of turn.'" (CR
/// 613.4d: layer 7d, after +1/+1 counters; two switches cancel.)
#[test]
fn cr_613_4d_wandering_fumarole_switches_its_power_and_toughness() {
    let mut g = scen().battlefield(0, &["Wandering Fumarole", "Island", "Island", "Mountain", "Mountain"]).at(Step::Main1, 0).start();
    let w = bf(&g, 0, "Wandering Fumarole");
    let n = g.def_of(w).activated.len() as u8;
    let i = (0..n).find(|&i| g.def_of(w).activated[i as usize].cost.mana == cost("{2}{U}{R}")).unwrap();
    start_with(&mut g, act(w, i), |_| None);
    settle(&mut g);
    assert_eq!(g.pt(w), (1, 4));
    start_with(&mut g, act(w, n - 1), |_| None);
    settle(&mut g);
    assert_eq!(g.pt(w), (4, 1));
    start_with(&mut g, act(w, n - 1), |_| None);
    settle(&mut g);
    assert_eq!(g.pt(w), (1, 4), "switched twice");
}

/// "Destroy target nonland permanent" (Thaumaton Torpedo's ability on a
/// custom card): a creature, an artifact or an enchantment, not a land or a
/// player.
#[test]
fn cr_701_8a_destroy_target_nonland_permanent() {
    let mut g = scen().battlefield(0, &["Test Torpedo"]).battlefield(0, &["Island"; 6]).battlefield(1, &["Seam Rip", "Forest", "Test Bear"]).at(Step::Main1, 0).start();
    let t = bf(&g, 0, "Test Torpedo");
    let (rip, forest) = (bf(&g, 1, "Seam Rip"), bf(&g, 1, "Forest"));
    g.choose(act(t, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(forest)) && !g.has_option(Choice::Target(Target::Player(1))));
    assert!(g.has_option(g.tgt(bf(&g, 1, "Test Bear"))) && g.has_option(g.tgt(t)));
    let tr = g.tgt(rip);
    g.choose(tr);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(rip).zone, Zone::Graveyard);
}
