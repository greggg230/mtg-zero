//! Card tests of the Edge of Eternities lands the mtg-bot IR-fix batch
//! unblocks (ENGINE.md 99.9, docs/ir_defects.md 20, 40, 41, 46): the planets
//! with station (CR 702.184a) and their "12+ |" abilities (CR 722.2a; a
//! planet has no power/toughness box and never becomes a creature, 722.1),
//! the lands whose "{T}: Add {C}." the IR restored, and the two Nexus
//! manlands whose animation is an artifact. Each against its Oracle text.

mod eoe_b;

use engine::db::{types, Counter, Kw};
use engine::mana::{ManaCost, C, G, R, U};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use eoe_b::*;

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn charge() -> Counter {
    Counter::Named(db().subtype_id("counter:charge").expect("charge counters"))
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// The station ability: the last activated ability (the lowering appends it
/// after the printed ones, CR 722.4).
fn station_slot(g: &Game, c: CardIdx) -> u8 {
    (g.def_of(c).activated.len() - 1) as u8
}

/// Station `planet` by tapping `crew`, and resolve it.
fn station(g: &mut Game, planet: CardIdx, crew: CardIdx) {
    let s = station_slot(g, planet);
    assert!(g.has_option(act(planet, s)), "station offered\n{}", log(g));
    start_with(g, act(planet, s), move |g| pick(g, &[Choice::Card(crew)]));
    settle(g);
}

/// Cast `name` from P0's hand, paying with the first of `payers` each
/// payment decision offers (mana abilities are activated while paying, CR
/// 605.3a; docs/shortcuts.md 13.12), until the spell is on the stack.
fn cast_paying(g: &mut Game, name: &str, payers: &[Choice]) {
    let c = g.card(0, name, Zone::Hand);
    let payers = payers.to_vec();
    start_with(g, Choice::Cast(c), move |g| pick(g, &payers));
    assert_eq!(g.obj(c).zone, Zone::Stack, "{name} cast\n{}", log(g));
}

/// `c`'s mana ability that makes exactly one mana of the types in `mask`.
fn mana_slot(g: &Game, c: CardIdx, mask: u8) -> Option<u8> {
    (0..g.def_of(c).activated.len() as u8).find(|&i| match &g.def_of(c).activated[i as usize].effect {
        engine::db::ActEffect::Mana(m) => m.prod == engine::db::ManaProd::One(mask),
        _ => false,
    })
}

/// The mana abilities of `p`'s permanents named `name` (as payers).
fn sources(g: &Game, p: u8, name: &str) -> Vec<Choice> {
    g.permanents_of(p).filter(|&c| g.name(c) == name).map(|c| act(c, 0)).collect()
}

// ---- "{T}: Add {C}." (docs/ir_defects.md 40) ------------------------------------

/// Bonders' Enclave, Cathedral of War, Deserted Temple, High Market,
/// Inventors' Fair, Mystifying Maze, Terrain Generator, Blinkmoth Nexus,
/// Inkmoth Nexus: "{T}: Add {C}." A mana ability (CR 605.1a) of exactly one
/// colorless mana: it pays {C} (106.1b), not a coloured pip.
#[test]
fn cr_605_1a_the_colorless_lands_tap_for_c() {
    for name in [
        "Bonders' Enclave",
        "Cathedral of War",
        "Deserted Temple",
        "High Market",
        "Inventors' Fair",
        "Mystifying Maze",
        "Terrain Generator",
        "Blinkmoth Nexus",
        "Inkmoth Nexus",
    ] {
        let mut g = scen().battlefield(0, &[name]).hand(0, &["Test Zero"]).at(Step::Main1, 0).start();
        let land = bf(&g, 0, name);
        assert!(g.can_pay(0, &cost("{C}")), "{name}: {{T}}: Add {{C}}");
        assert!(!g.can_pay(0, &cost("{C}{C}")), "{name}: one mana");
        for pip in ["{W}", "{U}", "{B}", "{R}", "{G}"] {
            assert!(!g.can_pay(0, &cost(pip)), "{name}: not {pip}");
        }
        let slot = mana_slot(&g, land, 1 << C).unwrap_or_else(|| panic!("{name}: a mana ability making {{C}}"));
        let a = &g.def_of(land).activated[slot as usize];
        assert!(a.cost.tap && a.cost.mana == ManaCost::default() && a.cost.sacrifice.is_none() && !a.cost.sac_self, "{name}: {{T}} alone");
        // It pays a {1} spell.
        cast_paying(&mut g, "Test Zero", &[act(land, slot)]);
        assert!(g.obj(land).tapped, "{name}: {{T}}");
    }
}

// ---- Bonders' Enclave ------------------------------------------------------------

/// Bonders' Enclave: "{3}, {T}: Draw a card. Activate only if you control a
/// creature with power 4 or greater." (CR 602.5b.)
#[test]
fn cr_602_5b_bonders_enclave_draws_only_with_a_power_four_creature() {
    // A 2/2 of yours and the opponent's 4/4: not offered.
    let g = scen()
        .battlefield(0, &["Bonders' Enclave", "Island", "Island", "Island", "Test Bear"])
        .battlefield(1, &["Test Trampler"])
        .at(Step::Main1, 0)
        .start();
    let e = bf(&g, 0, "Bonders' Enclave");
    assert!(!g.has_option(act(e, 1)), "a creature you control with power 4 or greater");
    // A 4/4 of yours: offered; {3} from the Islands, {T} the Enclave.
    let mut g = scen().battlefield(0, &["Bonders' Enclave", "Island", "Island", "Island", "Test Trampler"]).at(Step::Main1, 0).start();
    let e = bf(&g, 0, "Bonders' Enclave");
    assert!(g.has_option(act(e, 1)));
    let hand = g.player(0).hand.len();
    let islands = sources(&g, 0, "Island");
    start_with(&mut g, act(e, 1), move |g| pick(g, &islands));
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand + 1, "draw a card");
    assert!(g.obj(e).tapped);
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Island" && g.obj(c).tapped).count(), 3, "{{3}}");
}

// ---- Cathedral of War --------------------------------------------------------------

/// Cathedral of War: "This land enters tapped." (CR 614.1c.)
#[test]
fn cr_614_1c_cathedral_of_war_enters_tapped() {
    let mut g = scen().hand(0, &["Cathedral of War"]).at(Step::Main1, 0).start();
    play_land(&mut g, 0, "Cathedral of War", |_| None);
    let c = bf(&g, 0, "Cathedral of War");
    assert!(g.obj(c).tapped);
    assert!(!g.can_pay(0, &cost("{C}")));
}

/// Cathedral of War: "Exalted (Whenever a creature you control attacks
/// alone, that creature gets +1/+1 until end of turn.)" (CR 702.83a): a lone
/// attacker gets +1/+1; two attackers, neither does.
#[test]
fn cr_702_83a_cathedral_of_war_exalted_pumps_a_creature_attacking_alone() {
    let mut g = scen().battlefield(0, &["Cathedral of War", "Test Bear"]).at(Step::Main1, 0).start();
    let bear = bf(&g, 0, "Test Bear");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.pt(bear), (3, 3), "attacks alone: +1/+1\n{}", log(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::EndCombat);
    assert_eq!(g.life(1), 17);
    quiet_until(&mut g, main1_of(1));
    assert_eq!(g.pt(bear), (2, 2), "until end of turn");

    let mut g = scen().battlefield(0, &["Cathedral of War", "Test Bear", "Test Flyer"]).at(Step::Main1, 0).start();
    let (bear, flyer) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Flyer"));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Card(flyer));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!((g.pt(bear), g.pt(flyer)), ((2, 2), (2, 2)), "two attackers: no exalted");
    quiet_until(&mut g, |g| g.turn().step == Step::EndCombat);
    assert_eq!(g.life(1), 16);
}

// ---- Deserted Temple ---------------------------------------------------------------

/// Deserted Temple: "{1}, {T}: Untap target land." Any land (an opponent's
/// too), never a nonland permanent.
#[test]
fn deserted_temple_untaps_target_land() {
    let mut g = scen()
        .battlefield(0, &["Deserted Temple", "Island", "Forest", "Test Bear"])
        .battlefield(1, &["Mountain"])
        .at(Step::Main1, 0)
        .start();
    let (temple, island, forest, bear) = (bf(&g, 0, "Deserted Temple"), bf(&g, 0, "Island"), bf(&g, 0, "Forest"), bf(&g, 0, "Test Bear"));
    let mountain = bf(&g, 1, "Mountain");
    g.set_tapped(forest, true);
    g.set_tapped(bear, true);
    g.set_tapped(mountain, true);
    // Pay {1} with the Island, then choose the target.
    g.choose(act(temple, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{:?}", g.options_desc());
    assert!(g.has_option(g.tgt(forest)) && g.has_option(g.tgt(mountain)), "a land, any player's");
    assert!(!g.has_option(g.tgt(bear)), "not a creature");
    assert!(!g.has_option(Choice::Target(Target::Player(1))));
    let tf = g.tgt(forest);
    g.choose(tf);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert!(!g.obj(forest).tapped, "untapped");
    assert!(g.obj(temple).tapped && g.obj(island).tapped, "{{1}}, {{T}}");
}

// ---- High Market (docs/ir_defects.md 46) -------------------------------------------

/// High Market: "{T}, Sacrifice a creature: You gain 1 life." (CR 701.21a:
/// a creature you control; the IR had widened it to "another creature or
/// artifact".) With only a noncreature artifact it can't be activated.
#[test]
fn cr_701_21a_high_market_sacrifices_a_creature_not_an_artifact() {
    let g = scen().battlefield(0, &["High Market", "Nutrient Block"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let hm = bf(&g, 0, "High Market");
    assert!(!g.has_option(act(hm, 1)), "a noncreature artifact or an opponent's creature can't pay it");

    let mut g = scen().battlefield(0, &["High Market", "Nutrient Block", "Test Bear", "Dauntless Scrapbot"]).at(Step::Main1, 0).start();
    let (hm, block, bear, bot) = (bf(&g, 0, "High Market"), bf(&g, 0, "Nutrient Block"), bf(&g, 0, "Test Bear"), bf(&g, 0, "Dauntless Scrapbot"));
    assert!(g.has_option(act(hm, 1)));
    g.choose(act(hm, 1));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
    assert!(matches!(g.kind(), Some(DecisionKind::Sacrifice) | Some(DecisionKind::ChoosePermanent)), "{:?}", g.options_desc());
    assert!(g.has_option(Choice::Card(bear)) && g.has_option(Choice::Card(bot)), "a creature (an artifact creature too)");
    assert!(!g.has_option(Choice::Card(block)), "not a noncreature artifact");
    assert!(!g.has_option(Choice::Card(hm)), "not the land itself");
    g.choose(Choice::Card(bear));
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.life(0), 21);
    assert!(g.obj(hm).tapped);
}

// ---- Inventors' Fair ---------------------------------------------------------------

/// Inventors' Fair: "At the beginning of your upkeep, if you control three
/// or more artifacts, you gain 1 life." (CR 603.4: an intervening "if".)
#[test]
fn cr_603_4_inventors_fair_gains_a_life_in_your_upkeep_with_three_artifacts() {
    for (blocks, life) in [(3usize, 21), (2, 20)] {
        let mut names = vec!["Inventors' Fair"];
        names.extend(std::iter::repeat("Nutrient Block").take(blocks));
        let mut g = scen().battlefield(0, &names).battlefield(1, &["Nutrient Block"; 3]).at(Step::Main1, 1).start();
        quiet_until(&mut g, main1_of(0));
        assert_eq!(g.life(0), life, "{blocks} artifacts");
        assert_eq!(g.life(1), 20, "your upkeep, your artifacts");
    }
}

/// Inventors' Fair: "Legendary Land. {4}, {T}, Sacrifice Inventors' Fair:
/// Search your library for an artifact card, reveal it, put it into your
/// hand, then shuffle. Activate only if you control three or more
/// artifacts." (CR 602.5b, 701.19b: a card with a stated quality may be
/// missed.)
#[test]
fn cr_602_5b_inventors_fair_searches_for_an_artifact_with_three_artifacts() {
    let g = scen().battlefield(0, &["Inventors' Fair", "Nutrient Block", "Nutrient Block", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    let fair = bf(&g, 0, "Inventors' Fair");
    assert!(g.is_legendary(fair));
    assert!(!g.has_option(act(fair, 1)), "two artifacts");

    let mut g = scen()
        .library(0, &["Island", "Meltstrider's Gear", "Test Bear", "Island"])
        .battlefield(0, &["Inventors' Fair", "Nutrient Block", "Nutrient Block", "Nutrient Block", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let fair = bf(&g, 0, "Inventors' Fair");
    let lib = g.player(0).library.len();
    assert!(g.has_option(act(fair, 1)));
    let islands = sources(&g, 0, "Island");
    start_with(&mut g, act(fair, 1), move |g| pick(g, &islands));
    assert_eq!(g.obj(fair).zone, Zone::Graveyard, "sacrificed as a cost");
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "{}", log(&g));
    assert!(g.has_option(Choice::Def(def("Meltstrider's Gear"))), "an artifact card");
    assert!(!g.has_option(Choice::Def(def("Test Bear"))) && !g.has_option(Choice::Def(def("Island"))), "only artifact cards");
    assert!(g.has_option(Choice::Pass), "may fail to find (CR 701.19b)");
    g.choose(Choice::Def(def("Meltstrider's Gear")));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Meltstrider's Gear", Zone::Hand), 1, "into your hand");
    assert!(g.knows(1, g.card(0, "Meltstrider's Gear", Zone::Hand)), "revealed");
    assert_eq!(g.player(0).library.len(), lib - 1, "the Gear left the library");
}

// ---- Mystifying Maze ---------------------------------------------------------------

/// Mystifying Maze: "{4}, {T}: Exile target attacking creature an opponent
/// controls. At the beginning of the next end step, return it to the
/// battlefield tapped under its owner's control." (CR 603.7: a delayed
/// trigger; the card returns as a new object, 400.7.)
#[test]
fn cr_603_7_mystifying_maze_exiles_an_attacker_until_the_end_step() {
    let mut g = scen()
        .battlefield(0, &["Mystifying Maze", "Island", "Island", "Island", "Island", "Test Bear"])
        .battlefield(1, &["Test Trampler", "Test Flyer"])
        .at(Step::Main1, 1)
        .start();
    let maze = bf(&g, 0, "Mystifying Maze");
    let (trampler, flyer, mine) = (bf(&g, 1, "Test Trampler"), bf(&g, 1, "Test Flyer"), bf(&g, 0, "Test Bear"));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(trampler));
    g.choose(Choice::Pass);
    // P1 passes in the declare attackers step; P0 activates the Maze.
    g.drive_until(|g| g.to_move() == Some(0) && at_priority(g));
    assert_eq!(g.turn().step, Step::DeclareAttackers);
    g.choose(act(maze, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{:?}", g.options_desc());
    assert!(g.has_option(g.tgt(trampler)), "an attacking creature an opponent controls");
    assert!(!g.has_option(g.tgt(flyer)), "not attacking");
    assert!(!g.has_option(g.tgt(mine)), "not yours");
    let t = g.tgt(trampler);
    g.choose(t);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(trampler).zone, Zone::Exile);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(0), 20, "removed from combat by leaving the battlefield");
    quiet_until(&mut g, main1_of(0));
    let back = g.find_card(1, "Test Trampler", Zone::Battlefield).unwrap_or_else(|| panic!("returned\n{}", log(&g)));
    assert_eq!(g.obj(back).controller, 1, "under its owner's control");
    assert!(g.obj(back).tapped, "tapped");
}

// ---- Terrain Generator -------------------------------------------------------------

/// Terrain Generator: "{2}, {T}: You may put a basic land card from your
/// hand onto the battlefield tapped." Not a land play (CR 305.2a: it
/// doesn't use the turn's land drop); a nonbasic land card isn't offered;
/// "may": it can be declined.
#[test]
fn terrain_generator_may_put_a_basic_land_from_hand_onto_the_battlefield_tapped() {
    let mut g = scen()
        .battlefield(0, &["Terrain Generator", "Island", "Island"])
        .hand(0, &["Swamp", "Cathedral of War", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let tg = bf(&g, 0, "Terrain Generator");
    let (swamp, cathedral, bear) = (g.card(0, "Swamp", Zone::Hand), g.card(0, "Cathedral of War", Zone::Hand), g.card(0, "Test Bear", Zone::Hand));
    start_with(&mut g, act(tg, 1), |_| None);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert!(!at_priority(&g), "a choice: {}", log(&g));
    assert!(g.has_option(Choice::Card(swamp)), "a basic land card");
    assert!(!g.has_option(Choice::Card(cathedral)) && !g.has_option(Choice::Card(bear)), "only basic land cards");
    assert!(g.has_option(Choice::Pass), "may");
    g.choose(Choice::Card(swamp));
    settle(&mut g);
    assert_eq!(g.obj(swamp).zone, Zone::Battlefield);
    assert!(g.obj(swamp).tapped, "tapped");
    assert_eq!(g.player(0).lands_played, 0, "not the land drop");
    let c = g.card(0, "Cathedral of War", Zone::Hand);
    assert!(g.has_option(Choice::Play(c)), "the land drop is still there");

    // Declined: the land stays in hand.
    let mut g = scen().battlefield(0, &["Terrain Generator", "Island", "Island"]).hand(0, &["Swamp"]).at(Step::Main1, 0).start();
    let tg = bf(&g, 0, "Terrain Generator");
    start_with(&mut g, act(tg, 1), |_| None);
    resolve_with(&mut g, |g| pick(g, &[Choice::Pass]));
    assert_eq!(g.count_in(0, "Swamp", Zone::Hand), 1);
}

// ---- Blinkmoth Nexus and Inkmoth Nexus (docs/ir_defects.md 41) ---------------------

/// Blinkmoth Nexus: "{1}: This land becomes a 1/1 Blinkmoth artifact
/// creature with flying until end of turn. It's still a land." (CR 205.1b:
/// it keeps its types and gains artifact and creature, layer 4; 613.1d.)
/// The animation ends at the cleanup step (514.2).
#[test]
fn cr_205_1b_blinkmoth_nexus_becomes_a_1_1_blinkmoth_artifact_creature_with_flying() {
    let mut g = scen().battlefield(0, &["Blinkmoth Nexus", "Island"]).at(Step::Main1, 0).start();
    let n = bf(&g, 0, "Blinkmoth Nexus");
    let islands = sources(&g, 0, "Island");
    assert!(!g.is_creature(n) && !g.is_type(n, types::ARTIFACT));
    start_with(&mut g, act(n, 0), move |g| pick(g, &islands));
    settle(&mut g);
    assert!(g.is_type(n, types::LAND), "it's still a land");
    assert!(g.is_type(n, types::ARTIFACT), "an artifact creature");
    assert!(g.is_creature(n));
    assert!(subtype(&g, n, "Blinkmoth"));
    assert!(g.has_kw(n, Kw::Flying));
    assert_eq!(g.pt(n), (1, 1));
    assert_eq!(g.colors_of(n), 0, "colorless");
    assert!(!g.obj(n).tapped, "{{1}}: not {{T}}");
    quiet_until(&mut g, main1_of(1));
    assert!(!g.is_creature(n) && !g.is_type(n, types::ARTIFACT), "until end of turn");
    assert!(!subtype(&g, n, "Blinkmoth") && !g.has_kw(n, Kw::Flying));
}

/// Blinkmoth Nexus: "{1}, {T}: Target Blinkmoth creature gets +1/+1 until
/// end of turn." The target is a creature with the subtype Blinkmoth: an
/// animated Blinkmoth Nexus or Inkmoth Nexus, not another creature, not an
/// unanimated Nexus.
#[test]
fn blinkmoth_nexus_pumps_target_blinkmoth_creature() {
    let mut g = scen()
        .battlefield(0, &["Blinkmoth Nexus", "Blinkmoth Nexus", "Inkmoth Nexus", "Island", "Island", "Island", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let nexi: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Blinkmoth Nexus").collect();
    let (a, b) = (nexi[0], nexi[1]);
    let ink = bf(&g, 0, "Inkmoth Nexus");
    let bear = bf(&g, 0, "Test Bear");
    let islands = sources(&g, 0, "Island");
    let pay = islands.clone();
    start_with(&mut g, act(a, 0), move |g| pick(g, &pay));
    settle(&mut g);
    let pay = islands.clone();
    start_with(&mut g, act(ink, 1), move |g| pick(g, &pay));
    settle(&mut g);
    assert!(g.is_creature(a) && g.is_creature(ink));
    // {1} from the last Island, {T} the other Nexus.
    let pay = islands.clone();
    g.choose(act(b, 2));
    run_until(&mut g, |g| g.kind() == Some(DecisionKind::Target) || at_priority(g), move |g| pick(g, &pay));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{:?}", g.options_desc());
    assert!(g.has_option(g.tgt(a)), "an animated Blinkmoth Nexus");
    assert!(g.has_option(g.tgt(ink)), "an animated Inkmoth Nexus is a Blinkmoth");
    assert!(!g.has_option(g.tgt(bear)), "not a Blinkmoth");
    assert!(!g.has_option(g.tgt(b)), "itself: not a creature");
    let ta = g.tgt(a);
    g.choose(ta);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.pt(a), (2, 2));
    assert!(g.obj(b).tapped, "{{T}}");
    quiet_until(&mut g, main1_of(1));
    assert!(!g.is_creature(a));
}

/// Inkmoth Nexus: "{1}: This land becomes a 1/1 Phyrexian Blinkmoth artifact
/// creature with flying and infect until end of turn. It's still a land."
/// Infect: its combat damage to a player is poison counters, not life loss
/// (CR 702.90b, 120.3b).
#[test]
fn cr_702_90b_inkmoth_nexus_becomes_a_phyrexian_blinkmoth_with_infect() {
    let mut g = scen().battlefield(0, &["Inkmoth Nexus", "Island"]).at(Step::Main1, 0).start();
    let n = bf(&g, 0, "Inkmoth Nexus");
    let islands = sources(&g, 0, "Island");
    start_with(&mut g, act(n, 1), move |g| pick(g, &islands));
    settle(&mut g);
    assert!(g.is_type(n, types::LAND) && g.is_type(n, types::ARTIFACT) && g.is_creature(n));
    assert!(subtype(&g, n, "Phyrexian") && subtype(&g, n, "Blinkmoth"));
    assert!(g.has_kw(n, Kw::Flying) && g.has_kw(n, Kw::Infect));
    assert_eq!(g.pt(n), (1, 1));
    assert_eq!(g.colors_of(n), 0);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(n));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::EndCombat);
    assert_eq!(g.life(1), 20, "no life lost");
    assert_eq!(g.player(1).poison, 1, "a poison counter");
    quiet_until(&mut g, main1_of(1));
    assert!(!g.is_creature(n) && !g.is_type(n, types::ARTIFACT) && !g.has_kw(n, Kw::Infect));

    // Blocked: its damage to a creature is -1/-1 counters (CR 702.90c).
    let mut g = scen().battlefield(0, &["Inkmoth Nexus", "Island"]).battlefield(1, &["Test Reacher"]).at(Step::Main1, 0).start();
    let n = bf(&g, 0, "Inkmoth Nexus");
    let reacher = bf(&g, 1, "Test Reacher");
    let islands = sources(&g, 0, "Island");
    start_with(&mut g, act(n, 1), move |g| pick(g, &islands));
    settle(&mut g);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(n));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(n));
    if g.kind() == Some(DecisionKind::DeclareBlockers) {
        g.choose(Choice::Pass);
    }
    quiet_until(&mut g, |g| g.turn().step == Step::EndCombat);
    assert_eq!(g.counters_on(reacher, Counter::M1M1), 1, "a -1/-1 counter\n{}", log(&g));
    assert_eq!(g.obj(reacher).damage, 0, "no damage marked");
    assert_eq!(g.pt(reacher), (0, 2));
}

// ---- the planets (docs/ir_defects.md 20) -------------------------------------------

/// Evendo, Kavaron, Uthros: "This land enters tapped. / {T}: Add {G}/{R}/{U}.
/// / Station" — each a Land — Planet, played tapped (CR 614.1c), with its
/// station ability: "Tap another creature you control: Put charge counters
/// equal to its power on this Planet. Station only as a sorcery." (CR
/// 702.184a). A planet has no power/toughness box: never a creature, at 12
/// or 20 counters either (CR 722.1, 722.2b).
#[test]
fn cr_702_184a_the_planets_enter_tapped_and_have_station() {
    for (name, pip, color) in [("Evendo, Waking Haven", "{G}", G), ("Kavaron, Memorial World", "{R}", R), ("Uthros, Titanic Godcore", "{U}", U)] {
        let mut g = scen().hand(0, &[name]).battlefield(0, &["Test Trampler", "Test Giant", "Test Bear"]).at(Step::Main1, 0).start();
        play_land(&mut g, 0, name, |_| None);
        let p = bf(&g, 0, name);
        assert!(g.obj(p).tapped, "{name}: enters tapped");
        assert!(subtype(&g, p, "Planet") && g.is_type(p, types::LAND), "{name}");
        g.set_tapped(p, false);
        assert!(g.can_pay(0, &cost(pip)) && !g.can_pay(0, &cost("{C}")), "{name}: {{T}}: Add {pip}");
        assert_eq!(mana_slot(&g, p, 1 << color), Some(0), "{name}: {{T}}: Add {pip}, its first ability");
        // Station: sorcery speed, tapping another creature (CR 302.6 doesn't
        // apply: not {T}); the counters add up.
        let s = station_slot(&g, p);
        let a = &g.def_of(p).activated[s as usize];
        assert!(a.sorcery && a.cost.tap_untapped.as_ref().is_some_and(|t| t.another), "{name}");
        let (trampler, giant, bear) = (bf(&g, 0, "Test Trampler"), bf(&g, 0, "Test Giant"), bf(&g, 0, "Test Bear"));
        station(&mut g, p, trampler);
        assert_eq!(g.counters_on(p, charge()), 4, "{name}: the tapped creature's power");
        assert!(g.obj(trampler).tapped);
        station(&mut g, p, giant);
        station(&mut g, p, bear);
        assert_eq!(g.counters_on(p, charge()), 11, "{name}");
        assert!(!g.is_creature(p) && g.def_of(p).power.is_none(), "{name}: never a creature");
    }
    for k in [12, 20] {
        let g = scen().battlefield(0, &["Uthros, Titanic Godcore"]).counters(0, "Uthros, Titanic Godcore", charge(), k).at(Step::Main1, 0).start();
        let p = bf(&g, 0, "Uthros, Titanic Godcore");
        assert!(!g.is_creature(p), "{k} counters: still not a creature");
        assert_eq!(g.types_of(p) & !types::LAND, 0, "a land only");
    }
}

/// Station only as a sorcery (CR 702.184a): not in the opponent's turn.
#[test]
fn cr_702_184a_a_planet_is_stationed_only_as_a_sorcery() {
    let mut g = scen().battlefield(0, &["Evendo, Waking Haven", "Test Bear"]).at(Step::Main1, 1).start();
    let p = bf(&g, 0, "Evendo, Waking Haven");
    let s = station_slot(&g, p);
    g.drive_until(|g| g.to_move() == Some(0) && at_priority(g));
    assert!(!g.has_option(act(p, s)), "the opponent's turn");
    quiet_until(&mut g, main1_of(0));
    assert!(g.has_option(act(p, s)), "your main phase, the stack empty");
}

/// Evendo, Waking Haven: "12+ | {G}, {T}: Add {G} for each creature you
/// control." (CR 722.2a: only with 12 or more charge counters; the
/// creatures you control counted as the mana ability resolves, not the
/// opponent's, not your noncreature permanents.) With Evendo and a Forest
/// alone, a three-mana spell is cast only through it.
#[test]
fn cr_722_2a_evendo_adds_g_for_each_creature_you_control_at_twelve() {
    for (k, mine, castable) in [(11, 3usize, false), (12, 3, true), (12, 2, false)] {
        let mut names = vec!["Evendo, Waking Haven", "Forest", "Nutrient Block"];
        names.extend(std::iter::repeat("Test Bear").take(mine));
        let mut g = scen()
            .battlefield(0, &names)
            .battlefield(1, &["Test Bear", "Test Bear", "Test Bear"])
            .hand(0, &["Galactic Wayfarer"])
            .counters(0, "Evendo, Waking Haven", charge(), k)
            .at(Step::Main1, 0)
            .start();
        let p = bf(&g, 0, "Evendo, Waking Haven");
        let forest = bf(&g, 0, "Forest");
        let w = g.card(0, "Galactic Wayfarer", Zone::Hand);
        assert_eq!(g.has_option(Choice::Cast(w)), castable, "{k} counters, {mine} creatures: {{2}}{{G}}");
        assert_eq!(g.can_pay(0, &cost("{G}{G}{G}")), castable, "{k} counters, {mine} creatures");
        if castable {
            assert!(!g.can_pay(0, &cost("{G}{G}{G}{G}")), "three creatures: three {{G}}");
            // {G} from the Forest pays the ability's cost; the ability pays the spell.
            cast_paying(&mut g, "Galactic Wayfarer", &[act(p, 1), act(forest, 0)]);
            assert!(g.obj(p).tapped && g.obj(forest).tapped);
            settle(&mut g);
            assert_eq!(g.count_in(0, "Galactic Wayfarer", Zone::Battlefield), 1, "{}", log(&g));
        }
    }
}

/// Uthros, Titanic Godcore: "12+ | {U}, {T}: Add {U} for each artifact you
/// control." (CR 722.2a; an artifact creature counts, the opponent's
/// artifacts don't.) With Uthros and an Island alone, a three-mana spell is
/// cast only through it.
#[test]
fn cr_722_2a_uthros_adds_u_for_each_artifact_you_control_at_twelve() {
    for (k, blocks, castable) in [(11, 2usize, false), (12, 2, true), (12, 1, false)] {
        let mut names = vec!["Uthros, Titanic Godcore", "Island", "Dauntless Scrapbot", "Test Bear"];
        names.extend(std::iter::repeat("Nutrient Block").take(blocks));
        let mut g = scen()
            .battlefield(0, &names)
            .battlefield(1, &["Nutrient Block", "Nutrient Block", "Nutrient Block"])
            .hand(0, &["Dauntless Scrapbot"])
            .counters(0, "Uthros, Titanic Godcore", charge(), k)
            .at(Step::Main1, 0)
            .start();
        let p = bf(&g, 0, "Uthros, Titanic Godcore");
        let island = bf(&g, 0, "Island");
        let bot = g.card(0, "Dauntless Scrapbot", Zone::Hand);
        assert_eq!(g.has_option(Choice::Cast(bot)), castable, "{k} counters, {} artifacts: {{3}}", blocks + 1);
        assert_eq!(g.can_pay(0, &cost("{U}{U}{U}")), castable, "{k} counters, {} artifacts", blocks + 1);
        if castable {
            assert!(!g.can_pay(0, &cost("{U}{U}{U}{U}")), "three artifacts: three {{U}}");
            cast_paying(&mut g, "Dauntless Scrapbot", &[act(p, 1), act(island, 0)]);
            assert!(g.obj(p).tapped && g.obj(island).tapped);
        }
    }
}

/// Kavaron, Memorial World: "12+ | {1}{R}, {T}, Sacrifice a land: Create a
/// 2/2 colorless Robot artifact creature token, then creatures you control
/// get +1/+0 and gain haste until end of turn." The Robot is made first, so
/// it gets the bonus too (CR 608.2c: in order).
#[test]
fn cr_722_2a_kavaron_makes_a_robot_then_pumps_your_creatures_at_twelve() {
    let g = scen()
        .battlefield(0, &["Kavaron, Memorial World", "Mountain", "Mountain", "Forest"])
        .counters(0, "Kavaron, Memorial World", charge(), 11)
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(act(bf(&g, 0, "Kavaron, Memorial World"), 1)), "11 counters");
    let mut g = scen()
        .battlefield(0, &["Kavaron, Memorial World", "Mountain", "Mountain", "Forest", "Test Bear"])
        .battlefield(1, &["Test Bear"])
        .counters(0, "Kavaron, Memorial World", charge(), 12)
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 0, "Kavaron, Memorial World");
    let forest = bf(&g, 0, "Forest");
    let bear = bf(&g, 0, "Test Bear");
    let theirs = bf(&g, 1, "Test Bear");
    assert!(g.has_option(act(k, 1)));
    let mountains = sources(&g, 0, "Mountain");
    g.choose(act(k, 1));
    run_until(&mut g, |g| g.kind() == Some(DecisionKind::Sacrifice) || at_priority(g), move |g| pick(g, &mountains));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice), "{}", log(&g));
    assert!(g.has_option(Choice::Card(forest)) && g.has_option(Choice::Card(k)), "a land (Kavaron itself too, CR 701.21a)");
    assert!(!g.has_option(Choice::Card(bear)), "a land");
    start_with(&mut g, Choice::Card(forest), |_| None);
    assert_eq!(g.obj(forest).zone, Zone::Graveyard, "a land sacrificed as a cost");
    settle(&mut g);
    let robots: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Robot").collect();
    assert_eq!(robots.len(), 1, "{}", log(&g));
    let r = robots[0];
    assert!(g.def_of(r).token && g.is_type(r, types::ARTIFACT) && g.is_creature(r) && subtype(&g, r, "Robot"));
    assert_eq!(g.colors_of(r), 0);
    assert_eq!(g.pt(r), (3, 2), "2/2, then +1/+0");
    assert!(g.has_kw(r, Kw::Haste), "the Robot can attack this turn");
    assert_eq!(g.pt(bear), (3, 2));
    assert!(g.has_kw(bear, Kw::Haste));
    assert_eq!(g.pt(theirs), (2, 2), "creatures you control");
    assert!(g.obj(k).tapped);
    quiet_until(&mut g, main1_of(1));
    assert_eq!(g.pt(bear), (2, 2), "until end of turn");
    assert!(!g.has_kw(r, Kw::Haste));
}
