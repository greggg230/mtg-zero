//! Card tests of the Edge of Eternities creatures, spells and Lander makers
//! the mtg-bot IR-fix batch unblocks (ENGINE.md 99.9; docs/ir_defects.md
//! 21-23, 42, 48, 51, 60, 62-64, 81, 82), each ability against its Oracle
//! text: the Lander token (CR 111.10) with its {T} restored, the white Human
//! Soldier tokens, "enters with" a counter (614.1c), the "one or more ...
//! deal combat damage to a player" batches (603.2c), "up to one target"
//! (115.1d), "creature or Spacecraft" and "creature or planeswalker"
//! targets, the discarding player's life loss, Nexus of Fate's "from
//! anywhere" replacement (614.1a), and Starfield Shepherd's one search for
//! either kind of card (701.19). The lands and planets are in
//! `cards_eoe_irfix_lands.rs`.

mod eoe_b;

use engine::db::{types, CardDb, Counter, Kw};
use engine::mana::{ManaCost, W};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};
use eoe_b::*;
use serde_json::json;
use std::sync::OnceLock;

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn charge() -> Counter {
    Counter::Named(db().subtype_id("counter:charge").expect("charge counters"))
}

fn tokens_named(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    g.permanents_of(p).filter(|&c| g.name(c) == name && g.def_of(c).token).collect()
}

fn landers(g: &Game, p: PlayerId) -> Vec<CardIdx> {
    tokens_named(g, p, "Lander")
}

fn hand(g: &Game, p: PlayerId) -> usize {
    g.player(p).hand.len()
}

/// The Lander of CR 111.10: "a colorless artifact token named Lander with
/// the artifact subtype Lander and '{2}, {T}, Sacrifice this token: Search
/// your library for a basic land card, put it onto the battlefield tapped,
/// then shuffle.'" Every maker's token is the one rules definition (the
/// IR's printing with {T}, docs/ir_defects.md 42).
fn assert_rules_lander(g: &Game, c: CardIdx, who: &str) {
    let d = g.def_of(c);
    assert_eq!(Some(g.obj(c).def), db().token_by_name("Lander"), "{who}: the rules' Lander");
    assert!(d.token && d.is(types::ARTIFACT) && !d.is(types::CREATURE), "{who}: an artifact token");
    assert!(subtype(g, c, "Lander"), "{who}: subtype Lander");
    assert_eq!(g.colors_of(c), 0, "{who}: colorless");
    assert_eq!(d.activated.len(), 1, "{who}");
    let a = &d.activated[0].cost;
    assert!(a.tap && a.sac_self && a.mana == cost("{2}"), "{who}: {{2}}, {{T}}, Sacrifice this token");
}

/// The white 1/1 Human Soldier creature token (docs/ir_defects.md 23, 51).
fn assert_white_soldier(g: &Game, c: CardIdx, who: &str) {
    let d = g.def_of(c);
    assert!(d.token && g.is_creature(c), "{who}: a creature token");
    assert_eq!(g.colors_of(c), 1 << W, "{who}: white");
    assert!(subtype(g, c, "Human") && subtype(g, c, "Soldier"), "{who}: Human Soldier");
    assert_eq!(g.pt(c), (1, 1), "{who}");
}

/// Declare `attackers` (P0's) in P0's next declare attackers step.
fn attack_with(g: &mut Game, attackers: &[CardIdx]) {
    quiet_until(g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &c in attackers {
        g.choose(Choice::Card(c));
        // With a planeswalker to attack: the player (CR 508.1b).
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(Choice::Target(Target::Player(1)));
        }
    }
    g.choose(Choice::Pass);
}

/// Until the end of combat of this turn, the first option answering
/// everything (triggers' targets included) but `answer`'s.
fn through_combat(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    run_until(g, |g| g.turn().step == Step::EndCombat || g.turn().step == Step::Main2, answer);
}

/// Sacrifice `victim` to an untapped High Market of P0's ("{T}, Sacrifice a
/// creature: You gain 1 life"; it dies, CR 700.4) and resolve everything
/// with `answer`.
fn sacrifice_to_market(g: &mut Game, victim: CardIdx, answer: impl Fn(&Game) -> Option<Choice> + Copy) {
    let hm = g.permanents_of(0).find(|&c| g.name(c) == "High Market" && !g.obj(c).tapped).expect("an untapped High Market");
    start_with(g, act(hm, 1), move |g| pick(g, &[Choice::Card(victim)]).or_else(|| answer(g)));
    assert_ne!(g.obj(victim).zone, Zone::Battlefield, "sacrificed\n{}", log(g));
    resolve_with(g, answer);
}

// ---- bodies, keywords, warp --------------------------------------------------

/// Each creature's printed power, toughness, types, subtypes and keywords
/// (the Oracle text's).
#[test]
fn bodies_and_keywords_match_the_oracle_text() {
    use Kw::*;
    let rows: &[(&str, (i32, i32), u16, &[&str], &[Kw])] = &[
        ("All-Fates Stalker", (2, 3), types::CREATURE, &["Drix", "Assassin"], &[]),
        ("Beamsaw Prospector", (2, 1), types::CREATURE, &["Human", "Artificer"], &[]),
        ("Biomechan Engineer", (2, 2), types::CREATURE, &["Insect", "Artificer"], &[]),
        ("Biotech Specialist", (1, 3), types::CREATURE, &["Insect", "Scientist"], &[]),
        ("Cosmogrand Zenith", (2, 4), types::CREATURE, &["Human", "Soldier"], &[]),
        ("Dauntless Scrapbot", (3, 1), types::ARTIFACT | types::CREATURE, &["Robot"], &[]),
        ("Dockworker Drone", (1, 1), types::ARTIFACT | types::CREATURE, &["Robot"], &[]),
        ("Edge Rover", (2, 2), types::ARTIFACT | types::CREATURE, &["Robot", "Scout"], &[Reach]),
        ("Elegy Acolyte", (4, 4), types::CREATURE, &["Human", "Cleric"], &[Lifelink]),
        ("Galactic Wayfarer", (3, 3), types::CREATURE, &["Human", "Scout"], &[]),
        ("Glacier Godmaw", (6, 6), types::CREATURE, &["Leviathan"], &[Trample]),
        ("Haliya, Ascendant Cadet", (3, 3), types::CREATURE, &["Human", "Soldier"], &[]),
        ("Honored Knight-Captain", (1, 1), types::CREATURE, &["Human", "Advisor", "Knight"], &[]),
        ("Knight Luminary", (3, 2), types::CREATURE, &["Human", "Knight"], &[]),
        ("Rayblade Trooper", (2, 2), types::CREATURE, &["Human", "Soldier"], &[]),
        ("Seedship Agrarian", (3, 3), types::CREATURE, &["Insect", "Scientist"], &[]),
        ("Starfield Shepherd", (3, 2), types::CREATURE, &["Angel"], &[Flying]),
        ("Sunstar Expansionist", (2, 3), types::CREATURE, &["Human", "Knight"], &[]),
    ];
    for &(name, pt, ty, subs, kws) in rows {
        let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
        let c = bf(&g, 0, name);
        assert_eq!(g.pt(c), pt, "{name}");
        assert_eq!(g.types_of(c) & (types::ARTIFACT | types::CREATURE), ty, "{name}");
        for s in subs {
            assert!(subtype(&g, c, s), "{name}: {s}");
        }
        for &k in kws {
            assert!(g.has_kw(c, k), "{name}: {k:?}");
        }
    }
    let g = scen().battlefield(0, &["Haliya, Ascendant Cadet", "Dawnsire, Sunstar Dreadnought"]).at(Step::Main1, 0).start();
    assert!(g.is_legendary(bf(&g, 0, "Haliya, Ascendant Cadet")) && g.is_legendary(bf(&g, 0, "Dawnsire, Sunstar Dreadnought")));
}

/// "Warp {1}{W}" (CR 702.185a) on All-Fates Stalker, Knight Luminary,
/// Rayblade Trooper and Starfield Shepherd.
#[test]
fn cr_702_185a_warp_costs_match_the_oracle_text() {
    for name in ["All-Fates Stalker", "Knight Luminary", "Rayblade Trooper", "Starfield Shepherd"] {
        let d = db().def(def(name));
        let w: Vec<_> = d.alt_costs.iter().filter(|a| a.warp).collect();
        assert_eq!(w.len(), 1, "{name}");
        assert_eq!(w[0].mana, cost("{1}{W}"), "{name}");
    }
}

// ---- the Lander makers (docs/ir_defects.md 42) ------------------------------

/// Galactic Wayfarer: "When this creature enters, create a Lander token."
/// The Lander: "{2}, {T}, Sacrifice this token: Search your library for a
/// basic land card, put it onto the battlefield tapped, then shuffle." (CR
/// 111.10, 701.23.)
#[test]
fn cr_111_10_galactic_wayfarer_creates_a_lander_that_fetches_a_basic_land_tapped() {
    let mut g = scen().hand(0, &["Galactic Wayfarer"]).battlefield(0, &["Forest"; 5]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Galactic Wayfarer", |_| None);
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1, "{}", log(&g));
    assert_rules_lander(&g, l[0], "Galactic Wayfarer");
    let lib = g.player(0).library.len();
    let islands = count_named(&g, 0, "Island");
    start_with(&mut g, act(l[0], 0), |_| None);
    assert!(landers(&g, 0).is_empty(), "sacrificed as a cost");
    resolve_with(&mut g, |g| pick(g, &[Choice::Def(def("Island"))]));
    assert!(landers(&g, 0).is_empty(), "sacrificed");
    assert_eq!(count_named(&g, 0, "Island"), islands + 1);
    let island = g.permanents_of(0).find(|&c| g.name(c) == "Island").unwrap();
    assert!(g.obj(island).tapped, "onto the battlefield tapped");
    assert_eq!(g.player(0).library.len(), lib - 1);
}

/// The Lander's {T} (CR 602.5a; the IR had dropped it, docs/ir_defects.md
/// 42): a tapped Lander can't be activated.
#[test]
fn cr_602_5a_a_tapped_lander_cant_be_activated() {
    let mut g = scen().hand(0, &["Galactic Wayfarer", "Forest"]).battlefield(0, &["Forest"; 5]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Galactic Wayfarer", |_| None);
    let l = landers(&g, 0)[0];
    assert!(g.has_option(act(l, 0)));
    // (Tapped some other way: the test taps it, then plays a land so that
    // priority's options are made again.)
    g.set_tapped(l, true);
    play_land(&mut g, 0, "Forest", |_| None);
    assert!(!g.has_option(act(l, 0)), "{{T}} in its cost");
}

/// Beamsaw Prospector: "When this creature dies, create a Lander token."
#[test]
fn beamsaw_prospector_creates_a_lander_when_it_dies() {
    let mut g = scen().battlefield(0, &["Beamsaw Prospector", "High Market"]).at(Step::Main1, 0).start();
    let b = bf(&g, 0, "Beamsaw Prospector");
    assert!(landers(&g, 0).is_empty());
    sacrifice_to_market(&mut g, b, |_| None);
    assert_eq!(g.obj(b).zone, Zone::Graveyard);
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1, "{}", log(&g));
    assert_rules_lander(&g, l[0], "Beamsaw Prospector");
}

/// Biomechan Engineer: "When this creature enters, create a Lander token.
/// / {8}: Draw two cards and create a 2/2 colorless Robot artifact creature
/// token." (No {T}: activated the turn it enters, CR 302.6.)
#[test]
fn biomechan_engineer_creates_a_lander_and_draws_two_with_a_robot_for_eight() {
    let mut g = scen()
        .hand(0, &["Biomechan Engineer"])
        .battlefield(0, &["Forest", "Island"])
        .battlefield(0, &["Island"; 8])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Biomechan Engineer", |_| None);
    let e = bf(&g, 0, "Biomechan Engineer");
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1);
    assert_rules_lander(&g, l[0], "Biomechan Engineer");
    assert!(g.is_sick(e));
    assert!(g.has_option(act(e, 0)), "{{8}}: no {{T}}");
    let h = hand(&g, 0);
    start_with(&mut g, act(e, 0), |_| None);
    settle(&mut g);
    assert_eq!(hand(&g, 0), h + 2, "draw two cards");
    let robots = tokens_named(&g, 0, "Robot");
    assert_eq!(robots.len(), 1);
    let r = robots[0];
    assert!(g.is_type(r, types::ARTIFACT) && g.is_creature(r) && subtype(&g, r, "Robot"));
    assert_eq!(g.colors_of(r), 0);
    assert_eq!(g.pt(r), (2, 2));
}

/// Biotech Specialist: "When this creature enters, create a Lander token. /
/// Whenever you sacrifice an artifact, this creature deals 2 damage to
/// target opponent." Sacrificing the Lander (an artifact) triggers it;
/// sacrificing a nonartifact creature doesn't.
#[test]
fn cr_701_21a_biotech_specialist_deals_two_when_you_sacrifice_an_artifact() {
    let mut g = scen()
        .hand(0, &["Biotech Specialist"])
        .battlefield(0, &["Mountain", "Forest", "Forest", "Forest", "High Market", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Biotech Specialist", |_| None);
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1);
    assert_rules_lander(&g, l[0], "Biotech Specialist");
    let bear = bf(&g, 0, "Test Bear");
    sacrifice_to_market(&mut g, bear, |_| None);
    assert_eq!(g.life(1), 20, "a nonartifact creature: no trigger");
    g.choose(act(l[0], 0));
    run_until(&mut g, |g| g.kind() == Some(DecisionKind::Target) || (g.stack().is_empty() && at_priority(g)), |_| None);
    assert_eq!(g.kind(), Some(DecisionKind::Target), "the trigger targets\n{}", log(&g));
    assert!(g.has_option(Choice::Target(Target::Player(1))), "target opponent");
    assert!(!g.has_option(Choice::Target(Target::Player(0))), "not you");
    g.choose(Choice::Target(Target::Player(1)));
    resolve_with(&mut g, |g| pick(g, &[Choice::Def(def("Island"))]));
    assert_eq!(g.life(1), 18, "2 damage to the opponent");
    assert!(landers(&g, 0).is_empty());
}

/// Dauntless Scrapbot: "When this creature enters, exile each opponent's
/// graveyard. Create a Lander token." Your own graveyard stays.
#[test]
fn dauntless_scrapbot_exiles_each_opponents_graveyard_and_creates_a_lander() {
    let mut g = scen()
        .hand(0, &["Dauntless Scrapbot"])
        .battlefield(0, &["Island", "Island", "Island"])
        .graveyard(0, &["Test Bear"])
        .graveyard(1, &["Test Bear", "Forest", "Honor"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Dauntless Scrapbot", |_| None);
    assert_eq!(g.player(1).graveyard.len(), 0, "exiled");
    assert_eq!(g.count_in(1, "Test Bear", Zone::Exile) + g.count_in(1, "Forest", Zone::Exile) + g.count_in(1, "Honor", Zone::Exile), 3);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1, "not yours");
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1);
    assert_rules_lander(&g, l[0], "Dauntless Scrapbot");
}

/// Edge Rover: "Reach / When this creature dies, each player creates a
/// Lander token." (Each under its own control, CR 111.2.)
#[test]
fn edge_rover_gives_each_player_a_lander_when_it_dies() {
    let mut g = scen().battlefield(0, &["Edge Rover", "High Market"]).at(Step::Main1, 0).start();
    let r = bf(&g, 0, "Edge Rover");
    sacrifice_to_market(&mut g, r, |_| None);
    for p in [0, 1] {
        let l = landers(&g, p);
        assert_eq!(l.len(), 1, "P{p}\n{}", log(&g));
        assert_eq!(g.obj(l[0]).controller, p);
        assert_rules_lander(&g, l[0], "Edge Rover");
    }
}

/// Glacier Godmaw: "Trample / When this creature enters, create a Lander
/// token. / Landfall — Whenever a land you control enters, creatures you
/// control get +1/+1 and gain vigilance and haste until end of turn." (The
/// Lander is no land: no landfall.)
#[test]
fn glacier_godmaw_creates_a_lander_and_landfall_pumps_your_creatures() {
    let mut g = scen()
        .hand(0, &["Glacier Godmaw", "Forest"])
        .battlefield(0, &["Forest"; 7])
        .battlefield(0, &["Test Bear"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Glacier Godmaw", |_| None);
    let gm = bf(&g, 0, "Glacier Godmaw");
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1);
    assert_rules_lander(&g, l[0], "Glacier Godmaw");
    assert_eq!(g.pt(gm), (6, 6), "the Lander entering is no landfall");
    let (bear, theirs) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Bear"));
    play_land(&mut g, 0, "Forest", |_| None);
    for c in [gm, bear] {
        assert_eq!(g.pt(c), if c == gm { (7, 7) } else { (3, 3) });
        assert!(g.has_kw(c, Kw::Vigilance) && g.has_kw(c, Kw::Haste), "{}", g.name(c));
    }
    assert_eq!(g.pt(theirs), (2, 2), "creatures you control");
    assert!(!g.has_kw(theirs, Kw::Haste));
    // Haste: the Godmaw attacks the turn it entered; vigilance: it doesn't tap.
    attack_with(&mut g, &[gm]);
    assert!(!g.obj(gm).tapped, "vigilance");
    through_combat(&mut g, |_| None);
    assert_eq!(g.life(1), 13);
    quiet_until(&mut g, main1_of(1));
    assert_eq!(g.pt(gm), (6, 6), "until end of turn");
    assert!(!g.has_kw(bear, Kw::Haste));
}

/// Lithobraking: "Create a Lander token. Then you may sacrifice an
/// artifact. When you do, Lithobraking deals 2 damage to each creature."
/// (CR 603.12: a reflexive trigger, only if the artifact was sacrificed.)
#[test]
fn cr_603_12_lithobraking_creates_a_lander_and_may_sacrifice_for_two_to_each_creature() {
    // Sacrifice the new Lander: 2 damage to each creature, both players'.
    let mut g = scen()
        .hand(0, &["Lithobraking"])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Test Bear", "Test Trampler"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let (bear, trampler, theirs) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Trampler"), bf(&g, 1, "Test Bear"));
    let c = g.card(0, "Lithobraking", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |_| None);
    g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::Sacrifice) | Some(DecisionKind::ChoosePermanent) | Some(DecisionKind::YesNo)));
    if g.kind() == Some(DecisionKind::YesNo) {
        g.choose(Choice::Yes);
    }
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1, "the Lander first\n{}", log(&g));
    assert_rules_lander(&g, l[0], "Lithobraking");
    assert!(g.has_option(Choice::Card(l[0])), "{:?}", g.options_desc());
    assert!(!g.has_option(Choice::Card(bear)), "an artifact");
    g.choose(Choice::Card(l[0]));
    settle(&mut g);
    assert!(landers(&g, 0).is_empty());
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(theirs).zone, Zone::Graveyard);
    assert_eq!(g.obj(trampler).damage, 2);
    assert_eq!(g.life(1), 20, "creatures only");
    // Declined: the Lander stays, no damage.
    let mut g = scen()
        .hand(0, &["Lithobraking"])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    cast_and_resolve(&mut g, 0, "Lithobraking", |g| pick(g, &[Choice::Pass]));
    assert_eq!(landers(&g, 0).len(), 1);
    assert_eq!(g.obj(bear).damage, 0, "no artifact sacrificed: no damage");
}

/// Sami's Curiosity: "You gain 2 life. Create a Lander token."
#[test]
fn samis_curiosity_gains_two_life_and_creates_a_lander() {
    let mut g = scen().hand(0, &["Sami's Curiosity"]).battlefield(0, &["Forest"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Sami's Curiosity", |_| None);
    assert_eq!(g.life(0), 22);
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1);
    assert_rules_lander(&g, l[0], "Sami's Curiosity");
}

/// Scrounge for Eternity: "As an additional cost to cast this spell,
/// sacrifice an artifact or creature. / Return target creature or
/// Spacecraft card with mana value 5 or less from your graveyard to the
/// battlefield. Then create a Lander token." (CR 601.2b, 601.2h.)
#[test]
fn scrounge_for_eternity_returns_a_creature_or_spacecraft_card_and_creates_a_lander() {
    // Nothing to sacrifice (a land isn't an artifact or creature): can't be cast.
    let g = scen()
        .hand(0, &["Scrounge for Eternity"])
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .graveyard(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Scrounge for Eternity", Zone::Hand))), "the additional cost can't be paid");

    let mut g = scen()
        .hand(0, &["Scrounge for Eternity"])
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Nutrient Block"])
        .graveyard(0, &["Test Bear", "Wurmwall Sweeper", "Starfield Shepherd", "Glacier Godmaw", "Honor", "Forest"])
        .graveyard(1, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let block = bf(&g, 0, "Nutrient Block");
    let gy = |n: &str| g.card(0, n, Zone::Graveyard);
    let (bear, sweeper, shepherd, godmaw, honor, forest) =
        (gy("Test Bear"), gy("Wurmwall Sweeper"), gy("Starfield Shepherd"), gy("Glacier Godmaw"), gy("Honor"), gy("Forest"));
    let theirs = g.card(1, "Test Flyer", Zone::Graveyard);
    let c = g.card(0, "Scrounge for Eternity", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{:?}", g.options_desc());
    assert!(g.has_option(g.tgt(bear)), "a creature card");
    assert!(g.has_option(g.tgt(sweeper)), "a Spacecraft card (a noncreature artifact card)");
    assert!(g.has_option(g.tgt(shepherd)), "mana value 5");
    assert!(!g.has_option(g.tgt(godmaw)), "mana value 7");
    assert!(!g.has_option(g.tgt(honor)) && !g.has_option(g.tgt(forest)), "neither a creature nor a Spacecraft card");
    assert!(!g.has_option(g.tgt(theirs)), "from your graveyard");
    let ts = g.tgt(sweeper);
    g.choose(ts);
    run_until(&mut g, at_priority, move |g| pick(g, &[Choice::Card(block)]));
    assert_eq!(g.obj(block).zone, Zone::Graveyard, "sacrificed as the additional cost");
    settle(&mut g);
    assert_eq!(g.obj(sweeper).zone, Zone::Battlefield, "{}", log(&g));
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1);
    assert_rules_lander(&g, l[0], "Scrounge for Eternity");
}

/// Seedship Agrarian: "Whenever this creature becomes tapped, create a
/// Lander token. / Landfall — Whenever a land you control enters, put a
/// +1/+1 counter on this creature." (Attacking taps it, CR 508.1f.)
#[test]
fn seedship_agrarian_creates_a_lander_when_it_becomes_tapped_and_grows_on_landfall() {
    let mut g = scen().battlefield(0, &["Seedship Agrarian"]).hand(0, &["Forest"]).at(Step::Main1, 0).start();
    let s = bf(&g, 0, "Seedship Agrarian");
    play_land(&mut g, 0, "Forest", |_| None);
    assert_eq!(g.counters_on(s, Counter::P1P1), 1, "landfall");
    assert_eq!(g.pt(s), (4, 4));
    assert!(landers(&g, 0).is_empty());
    attack_with(&mut g, &[s]);
    settle(&mut g);
    let l = landers(&g, 0);
    assert_eq!(l.len(), 1, "it became tapped\n{}", log(&g));
    assert_rules_lander(&g, l[0], "Seedship Agrarian");
    // Tapped to pay a planet's station cost (CR 702.184a): it becomes
    // tapped all the same.
    let mut g = scen().battlefield(0, &["Seedship Agrarian", "Evendo, Waking Haven"]).at(Step::Main1, 0).start();
    let (s, p) = (bf(&g, 0, "Seedship Agrarian"), bf(&g, 0, "Evendo, Waking Haven"));
    let st = (g.def_of(p).activated.len() - 1) as u8;
    start_with(&mut g, act(p, st), move |g| pick(g, &[Choice::Card(s)]));
    settle(&mut g);
    assert!(g.obj(s).tapped);
    assert_eq!(g.counters_on(p, charge()), 3);
    assert_eq!(landers(&g, 0).len(), 1, "tapped for station\n{}", log(&g));
}

/// Sunstar Expansionist: "When this creature enters, if an opponent controls
/// more lands than you, create a Lander token. / Landfall — Whenever a land
/// you control enters, this creature gets +1/+0 until end of turn." (CR
/// 603.4: an intervening "if".)
#[test]
fn cr_603_4_sunstar_expansionist_creates_a_lander_if_an_opponent_has_more_lands() {
    for (opp_lands, makes) in [(3usize, true), (2, false)] {
        let mut g = scen()
            .hand(0, &["Sunstar Expansionist", "Plains"])
            .battlefield(0, &["Plains", "Plains"])
            .battlefield(1, &vec!["Forest"; opp_lands])
            .at(Step::Main1, 0)
            .start();
        cast_and_resolve(&mut g, 0, "Sunstar Expansionist", |_| None);
        let l = landers(&g, 0);
        assert_eq!(l.len(), usize::from(makes), "opponent {opp_lands} lands, you 2");
        if makes {
            assert_rules_lander(&g, l[0], "Sunstar Expansionist");
        }
        let s = bf(&g, 0, "Sunstar Expansionist");
        play_land(&mut g, 0, "Plains", |_| None);
        assert_eq!(g.pt(s), (3, 3), "landfall: +1/+0");
        quiet_until(&mut g, main1_of(1));
        assert_eq!(g.pt(s), (2, 3), "until end of turn");
    }
}

// ---- the white Human Soldiers (docs/ir_defects.md 23, 51) -------------------

/// Cosmogrand Zenith: "Whenever you cast your second spell each turn, choose
/// one — • Create two 1/1 white Human Soldier creature tokens. • Put a
/// +1/+1 counter on each creature you control." The second spell only.
#[test]
fn cosmogrand_zenith_on_your_second_spell_makes_two_white_soldiers_or_counters() {
    // First mode.
    let mut g = scen()
        .battlefield(0, &["Cosmogrand Zenith", "Test Bear", "Plains", "Plains", "Plains"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Honor", "Honor", "Honor"])
        .at(Step::Main1, 0)
        .start();
    let (z, bear, theirs) = (bf(&g, 0, "Cosmogrand Zenith"), bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Bear"));
    let tb = g.tgt(bear);
    cast_and_resolve(&mut g, 0, "Honor", move |g| pick(g, &[tb]));
    assert!(tokens_named(&g, 0, "Human Soldier").is_empty(), "first spell: nothing");
    let honor = g.card(0, "Honor", Zone::Hand);
    g.choose(Choice::Cast(honor));
    run_until(&mut g, |g| g.kind() == Some(DecisionKind::ChooseMode), move |g| pick(g, &[tb]));
    assert!(g.has_option(Choice::Number(0)) && g.has_option(Choice::Number(1)), "choose one");
    g.choose(Choice::Number(0));
    resolve_with(&mut g, move |g| pick(g, &[tb]));
    let soldiers = tokens_named(&g, 0, "Human Soldier");
    assert_eq!(soldiers.len(), 2, "{}", log(&g));
    for s in soldiers {
        assert_white_soldier(&g, s, "Cosmogrand Zenith");
    }
    cast_and_resolve(&mut g, 0, "Honor", move |g| pick(g, &[tb]));
    assert_eq!(tokens_named(&g, 0, "Human Soldier").len(), 2, "third spell: nothing");
    let _ = (z, theirs);

    // Second mode: a counter on each creature you control.
    let mut g = scen()
        .battlefield(0, &["Cosmogrand Zenith", "Test Bear", "Plains", "Plains"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Honor", "Honor"])
        .at(Step::Main1, 0)
        .start();
    let (z, bear, theirs) = (bf(&g, 0, "Cosmogrand Zenith"), bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Bear"));
    let tt = g.tgt(theirs);
    cast_and_resolve(&mut g, 0, "Honor", move |g| pick(g, &[tt]));
    cast_and_resolve(&mut g, 0, "Honor", move |g| pick(g, &[tt, Choice::Number(1)]));
    assert_eq!(g.counters_on(z, Counter::P1P1), 1);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    assert_eq!(g.counters_on(theirs, Counter::P1P1), 2, "the two Honors only: creatures you control");
    assert!(tokens_named(&g, 0, "Human Soldier").is_empty());

    // "Your second spell": an opponent's spell in between doesn't count.
    let mut g = scen()
        .battlefield(0, &["Cosmogrand Zenith", "Test Bear", "Plains", "Plains"])
        .hand(0, &["Honor", "Honor"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Test Rummage", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let tb = g.tgt(bf(&g, 0, "Test Bear"));
    let honor = g.card(0, "Honor", Zone::Hand);
    start_with(&mut g, Choice::Cast(honor), move |g| pick(g, &[tb]));
    g.pass();
    assert_eq!(g.to_move(), Some(1));
    let r = g.card(1, "Test Rummage", Zone::Hand);
    start_with(&mut g, Choice::Cast(r), |_| None);
    resolve_with(&mut g, move |g| {
        assert_ne!(g.kind(), Some(DecisionKind::ChooseMode), "the opponent's spell is not your second\n{}", log(g));
        pick(g, &[tb])
    });
    assert_eq!(g.player(1).spells_cast, 1);
    cast_and_resolve(&mut g, 0, "Honor", move |g| pick(g, &[tb, Choice::Number(0)]));
    assert_eq!(tokens_named(&g, 0, "Human Soldier").len(), 2, "your second spell\n{}", log(&g));
}

/// Honored Knight-Captain: "When this creature enters, create a 1/1 white
/// Human Soldier creature token. / {4}{W}{W}, Sacrifice this creature:
/// Search your library for an Equipment card, put it onto the battlefield,
/// then shuffle."
#[test]
fn honored_knight_captain_makes_a_white_soldier_and_sacrifices_to_fetch_an_equipment() {
    let mut g = scen()
        .library(0, &["Squire's Lightblade", "Test Bear", "Nutrient Block"])
        .hand(0, &["Honored Knight-Captain"])
        .battlefield(0, &["Plains"; 8])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Honored Knight-Captain", |_| None);
    let k = bf(&g, 0, "Honored Knight-Captain");
    let s = tokens_named(&g, 0, "Human Soldier");
    assert_eq!(s.len(), 1);
    assert_white_soldier(&g, s[0], "Honored Knight-Captain");
    assert!(g.has_option(act(k, 0)), "no {{T}}: the turn it entered");
    start_with(&mut g, act(k, 0), |_| None);
    assert_eq!(g.obj(k).zone, Zone::Graveyard, "sacrificed as a cost");
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "{}", log(&g));
    assert!(g.has_option(Choice::Def(def("Squire's Lightblade"))), "an Equipment card");
    assert!(!g.has_option(Choice::Def(def("Nutrient Block"))), "an artifact that isn't an Equipment");
    assert!(!g.has_option(Choice::Def(def("Test Bear"))) && !g.has_option(Choice::Def(def("Island"))));
    g.choose(Choice::Def(def("Squire's Lightblade")));
    settle(&mut g);
    assert_eq!(count_named(&g, 0, "Squire's Lightblade"), 1, "onto the battlefield\n{}", log(&g));
}

/// Knight Luminary: "When this creature enters, create a 1/1 white Human
/// Soldier creature token. / Warp {1}{W}" (CR 702.185a: warped, it is
/// exiled at the next end step; the token stays).
#[test]
fn knight_luminary_makes_a_white_soldier_even_when_warped() {
    let mut g = scen().hand(0, &["Knight Luminary"]).battlefield(0, &["Plains", "Plains"]).at(Step::Main1, 0).start();
    let k = g.card(0, "Knight Luminary", Zone::Hand);
    start_with(&mut g, Choice::Cast(k), |g| pick(g, &[Choice::Number(1)]));
    settle(&mut g);
    assert_eq!(g.obj(k).zone, Zone::Battlefield, "warped for {{1}}{{W}}");
    let s = tokens_named(&g, 0, "Human Soldier");
    assert_eq!(s.len(), 1);
    assert_white_soldier(&g, s[0], "Knight Luminary");
    quiet_until(&mut g, main1_of(1));
    assert_eq!(g.obj(k).zone, Zone::Exile, "exiled at the end step");
    assert_eq!(tokens_named(&g, 0, "Human Soldier").len(), 1);
}

/// Rayblade Trooper: "When this creature enters, put a +1/+1 counter on
/// target creature you control."
#[test]
fn rayblade_trooper_puts_a_counter_on_target_creature_you_control() {
    let mut g = scen()
        .hand(0, &["Rayblade Trooper"])
        .battlefield(0, &["Plains", "Plains", "Plains", "Test Bear"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let (bear, theirs) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Bear"));
    let c = g.card(0, "Rayblade Trooper", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |_| None);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    assert!(g.has_option(g.tgt(bear)) && g.has_option(g.tgt(c)), "a creature you control (itself too)");
    assert!(!g.has_option(g.tgt(theirs)), "you control");
    let tb = g.tgt(bear);
    g.choose(tb);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
}

/// Rayblade Trooper: "Whenever a nontoken creature you control with a +1/+1
/// counter on it dies, create a 1/1 white Human Soldier creature token."
/// Not for a creature without a counter, nor a token; for Rayblade itself
/// (a leaves-the-battlefield ability looks back, CR 603.10a).
#[test]
fn cr_603_10a_rayblade_trooper_makes_a_white_soldier_when_a_countered_nontoken_creature_dies() {
    let mut g = scen()
        .battlefield(0, &["Rayblade Trooper", "High Market", "High Market", "High Market", "Test Bear", "Test Flyer", "Plains"])
        .counters(0, "Test Bear", Counter::P1P1, 1)
        .hand(0, &["Honor"])
        .at(Step::Main1, 0)
        .start();
    let (bear, flyer) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Flyer"));
    sacrifice_to_market(&mut g, flyer, |_| None);
    assert!(tokens_named(&g, 0, "Human Soldier").is_empty(), "no +1/+1 counter: nothing");
    sacrifice_to_market(&mut g, bear, |_| None);
    let s = tokens_named(&g, 0, "Human Soldier");
    assert_eq!(s.len(), 1, "{}", log(&g));
    assert_white_soldier(&g, s[0], "Rayblade Trooper");
    // A token with a counter dies: nothing.
    let ts = g.tgt(s[0]);
    cast_and_resolve(&mut g, 0, "Honor", move |g| pick(g, &[ts]));
    assert_eq!(g.counters_on(s[0], Counter::P1P1), 1);
    sacrifice_to_market(&mut g, s[0], |_| None);
    assert!(tokens_named(&g, 0, "Human Soldier").is_empty(), "a token: nothing");

    let mut g = scen()
        .battlefield(0, &["Rayblade Trooper", "High Market"])
        .counters(0, "Rayblade Trooper", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let r = bf(&g, 0, "Rayblade Trooper");
    sacrifice_to_market(&mut g, r, |_| None);
    assert_eq!(tokens_named(&g, 0, "Human Soldier").len(), 1, "Rayblade itself, with a counter\n{}", log(&g));

    // An opponent's creature with a counter dies: "a creature you control".
    let mut g = scen()
        .battlefield(0, &["Rayblade Trooper"])
        .battlefield(1, &["High Market", "Test Bear"])
        .counters(1, "Test Bear", Counter::P1P1, 1)
        .at(Step::Main1, 1)
        .start();
    let (hm, bear) = (bf(&g, 1, "High Market"), bf(&g, 1, "Test Bear"));
    start_with(&mut g, act(hm, 1), move |g| pick(g, &[Choice::Card(bear)]));
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert!(tokens_named(&g, 0, "Human Soldier").is_empty() && tokens_named(&g, 1, "Human Soldier").is_empty());
}

// ---- All-Fates Stalker (docs/ir_defects.md 60) ------------------------------

/// All-Fates Stalker: "When this creature enters, exile up to one target
/// non-Assassin creature until this creature leaves the battlefield." (CR
/// 115.1d, 610.3: the card returns when the Stalker leaves.)
#[test]
fn cr_610_3_all_fates_stalker_exiles_up_to_one_non_assassin_until_it_leaves() {
    let mut g = scen()
        .hand(0, &["All-Fates Stalker"])
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains", "High Market", "Test Bear"])
        .battlefield(1, &["Test Trampler", "All-Fates Stalker"])
        .at(Step::Main1, 0)
        .start();
    let (mine, trampler, other) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Trampler"), bf(&g, 1, "All-Fates Stalker"));
    let c = g.card(0, "All-Fates Stalker", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |_| None);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    assert!(g.has_option(g.tgt(trampler)) && g.has_option(g.tgt(mine)), "a non-Assassin creature, any player's");
    assert!(!g.has_option(g.tgt(other)), "not an Assassin");
    assert!(!g.has_option(g.tgt(c)), "not itself (an Assassin)");
    assert!(g.has_option(Choice::Pass), "up to one");
    let t = g.tgt(trampler);
    g.choose(t);
    settle(&mut g);
    assert_eq!(g.obj(trampler).zone, Zone::Exile);
    sacrifice_to_market(&mut g, c, |_| None);
    let back = g.find_card(1, "Test Trampler", Zone::Battlefield).unwrap_or_else(|| panic!("returned\n{}", log(&g)));
    assert_eq!(g.obj(back).controller, 1, "under its owner's control");
}

/// "Up to one": zero targets is a legal choice (CR 115.1d); and with no
/// other creature (nothing but Assassins) the ability has nothing to exile.
#[test]
fn cr_115_1d_all_fates_stalker_with_no_target_exiles_nothing() {
    // Zero targets chosen.
    let mut g = scen()
        .hand(0, &["All-Fates Stalker"])
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains"])
        .battlefield(1, &["Test Trampler"])
        .at(Step::Main1, 0)
        .start();
    let trampler = bf(&g, 1, "Test Trampler");
    cast_and_resolve(&mut g, 0, "All-Fates Stalker", |g| pick(g, &[Choice::Pass]));
    assert_eq!(g.obj(trampler).zone, Zone::Battlefield);
    assert_eq!(count_named(&g, 0, "All-Fates Stalker"), 1);
    // No legal target at all: the Stalker still resolves, nothing is exiled.
    let mut g = scen()
        .hand(0, &["All-Fates Stalker"])
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains"])
        .battlefield(1, &["All-Fates Stalker"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "All-Fates Stalker", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |_| None);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.stack().is_empty());
    if g.kind() == Some(DecisionKind::Target) {
        assert_eq!(g.options_desc().len(), 1, "only \"no target\": {:?}", g.options_desc());
        g.choose(Choice::Pass);
    }
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    assert_eq!(g.exile_zone().len(), 0, "nothing exiled\n{}", log(&g));
}

/// Warped (CR 702.185a), the All-Fates Stalker is exiled at the next end
/// step, and the creature it exiled returns then (CR 610.3).
#[test]
fn cr_702_185a_a_warped_all_fates_stalker_returns_the_creature_at_the_end_step() {
    let mut g = scen().hand(0, &["All-Fates Stalker"]).battlefield(0, &["Plains", "Plains"]).battlefield(1, &["Test Trampler"]).at(Step::Main1, 0).start();
    let trampler = bf(&g, 1, "Test Trampler");
    let c = g.card(0, "All-Fates Stalker", Zone::Hand);
    let t = g.tgt(trampler);
    start_with(&mut g, Choice::Cast(c), |g| pick(g, &[Choice::Number(1)]));
    resolve_with(&mut g, move |g| pick(g, &[t]));
    assert_eq!(g.obj(trampler).zone, Zone::Exile);
    quiet_until(&mut g, main1_of(1));
    assert_eq!(g.obj(c).zone, Zone::Exile, "warp: exiled at the end step");
    assert_eq!(g.count_in(1, "Test Trampler", Zone::Battlefield), 1, "returned\n{}", log(&g));
}

// ---- Starfield Shepherd (docs/ir_defects.md 48) -----------------------------

/// Starfield Shepherd: "Flying / When this creature enters, search your
/// library for a basic Plains card or a creature card with mana value 1 or
/// less, reveal it, put it into your hand, then shuffle." One search: the
/// player picks a card of either kind during it (CR 701.19), not a mode.
#[test]
fn cr_701_19_starfield_shepherd_searches_for_a_basic_plains_or_a_small_creature() {
    for want in ["Plains", "Test Hasty", "Test Zero"] {
        let mut g = scen()
            .library(0, &["Plains", "Test Hasty", "Test Zero", "Test Bear", "Honor", "Swamp"])
            .hand(0, &["Starfield Shepherd"])
            .battlefield(0, &["Plains"; 5])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, "Starfield Shepherd", Zone::Hand);
        start_with(&mut g, Choice::Cast(c), |_| None);
        g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary) || g.stack().is_empty() || g.kind() == Some(DecisionKind::ChooseMode));
        assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "one search, no mode\n{}", log(&g));
        for ok in ["Plains", "Test Hasty", "Test Zero"] {
            assert!(g.has_option(Choice::Def(def(ok))), "{ok}: a basic Plains or a creature with mana value 1 or less");
        }
        for no in ["Test Bear", "Honor", "Swamp", "Island"] {
            assert!(!g.has_option(Choice::Def(def(no))), "{no}");
        }
        assert!(g.has_option(Choice::Pass), "may fail to find (CR 701.19b)");
        g.choose(Choice::Def(def(want)));
        settle(&mut g);
        assert_eq!(g.count_in(0, want, Zone::Hand), 1, "{want} into the hand");
        assert!(g.knows(1, g.card(0, want, Zone::Hand)), "{want}: revealed");
        assert!(g.has_kw(c, Kw::Flying));
    }
}

/// Starfield Shepherd's warp ({1}{W}): its enters trigger searches the
/// same way.
#[test]
fn cr_702_185a_a_warped_starfield_shepherd_still_searches() {
    let mut g = scen().library(0, &["Plains"]).hand(0, &["Starfield Shepherd"]).battlefield(0, &["Plains", "Plains"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Starfield Shepherd", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |g| pick(g, &[Choice::Number(1)]));
    resolve_with(&mut g, |g| pick(g, &[Choice::Def(def("Plains"))]));
    assert_eq!(g.count_in(0, "Plains", Zone::Hand), 1);
    quiet_until(&mut g, main1_of(1));
    assert_eq!(g.obj(c).zone, Zone::Exile);
}

// ---- Dockworker Drone (docs/ir_defects.md 21) -------------------------------

/// Dockworker Drone: "This creature enters with a +1/+1 counter on it." A
/// replacement effect (CR 614.1c), not a trigger: it is never on the
/// battlefield without the counter, and nothing goes on the stack.
#[test]
fn cr_614_1c_dockworker_drone_enters_with_its_counter() {
    let d = db().def(def("Dockworker Drone"));
    assert_eq!(d.triggers.len(), 1, "only the dies trigger");
    let mut g = scen().hand(0, &["Dockworker Drone"]).battlefield(0, &["Plains", "Plains"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Dockworker Drone", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |_| None);
    g.pass();
    g.drive_until(|g| g.obj(c).zone == Zone::Battlefield || g.stack().is_empty());
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    assert_eq!(g.counters_on(c, Counter::P1P1), 1, "on entering");
    assert_eq!(g.pt(c), (2, 2));
    assert!(g.stack().is_empty() && g.waiting_triggers().is_empty(), "no trigger\n{}", log(&g));
    // Put onto the battlefield another way (Scrounge for Eternity's return,
    // not cast): it enters with the counter all the same.
    let mut g = scen()
        .hand(0, &["Scrounge for Eternity"])
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Test Bear"])
        .graveyard(0, &["Dockworker Drone"])
        .at(Step::Main1, 0)
        .start();
    let drone = g.card(0, "Dockworker Drone", Zone::Graveyard);
    let bear = bf(&g, 0, "Test Bear");
    let td = g.tgt(drone);
    cast_and_resolve(&mut g, 0, "Scrounge for Eternity", move |g| pick(g, &[td, Choice::Card(bear)]));
    let drone = g.card(0, "Dockworker Drone", Zone::Battlefield);
    assert_eq!(g.counters_on(drone, Counter::P1P1), 1, "returned: enters with the counter\n{}", log(&g));
}

/// Dockworker Drone: "When this creature dies, put its counters on target
/// creature you control." (Its counters as it last existed, CR 608.2h.)
#[test]
fn cr_608_2h_dockworker_drone_moves_its_counters_when_it_dies() {
    let mut g = scen()
        .battlefield(0, &["Dockworker Drone", "High Market", "Test Bear", "Test Zero"])
        .counters(0, "Dockworker Drone", Counter::P1P1, 2)
        .counters(0, "Test Zero", Counter::P1P1, 1)
        .battlefield(1, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let (d, bear, theirs) = (bf(&g, 0, "Dockworker Drone"), bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Flyer"));
    let hm = bf(&g, 0, "High Market");
    g.choose(act(hm, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    g.choose(Choice::Card(d));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    assert!(g.has_option(g.tgt(bear)) && g.has_option(g.tgt(bf(&g, 0, "Test Zero"))), "a creature you control");
    assert!(!g.has_option(g.tgt(theirs)), "not an opponent's");
    let tb = g.tgt(bear);
    g.choose(tb);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 2, "both counters");
    assert_eq!(g.pt(bear), (4, 4));
}

// ---- Haliya, Ascendant Cadet (docs/ir_defects.md 22) ------------------------

/// Haliya, Ascendant Cadet: "Whenever Haliya enters or attacks, put a +1/+1
/// counter on target creature you control."
#[test]
fn haliya_puts_a_counter_on_target_creature_you_control_when_it_enters_or_attacks() {
    let mut g = scen()
        .hand(0, &["Haliya, Ascendant Cadet"])
        .battlefield(0, &["Forest", "Plains", "Plains", "Plains", "Plains", "Test Bear"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let (bear, theirs) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Bear"));
    let c = g.card(0, "Haliya, Ascendant Cadet", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |_| None);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    assert!(g.has_option(g.tgt(bear)) && g.has_option(g.tgt(c)));
    assert!(!g.has_option(g.tgt(theirs)), "you control");
    let tb = g.tgt(bear);
    g.choose(tb);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "enters");
    // Attacks (the Haliya already there).
    let mut g = scen().battlefield(0, &["Haliya, Ascendant Cadet", "Test Bear"]).at(Step::Main1, 0).start();
    let (h, bear) = (bf(&g, 0, "Haliya, Ascendant Cadet"), bf(&g, 0, "Test Bear"));
    attack_with(&mut g, &[h]);
    let tb = g.tgt(bear);
    resolve_with(&mut g, move |g| pick(g, &[tb]));
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "attacks\n{}", log(&g));
}

/// Haliya: "Whenever one or more creatures you control with +1/+1 counters
/// on them deal combat damage to a player, draw a card." One draw for the
/// batch (CR 603.2c); a creature without a counter doesn't count.
#[test]
fn cr_603_2c_haliya_draws_once_for_a_batch_of_countered_creatures() {
    let mut g = scen()
        .battlefield(0, &["Haliya, Ascendant Cadet", "Test Bear", "Test Flyer"])
        .counters(0, "Test Bear", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let (h, bear, flyer) = (bf(&g, 0, "Haliya, Ascendant Cadet"), bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Flyer"));
    let n = hand(&g, 0);
    attack_with(&mut g, &[h, bear, flyer]);
    let th = g.tgt(h);
    through_combat(&mut g, move |g| pick(g, &[th]));
    assert_eq!(g.life(1), 20 - 4 - 3 - 2);
    assert_eq!(hand(&g, 0), n + 1, "one batch, one card\n{}", log(&g));
    // No creature with a counter connects: nothing.
    let mut g = scen().battlefield(0, &["Haliya, Ascendant Cadet", "Test Bear"]).at(Step::Main1, 0).start();
    let bear = bf(&g, 0, "Test Bear");
    let n = hand(&g, 0);
    attack_with(&mut g, &[bear]);
    through_combat(&mut g, |_| None);
    assert_eq!(g.life(1), 18);
    assert_eq!(hand(&g, 0), n);
}

/// The batch is per combat damage step, with no once-each-turn limit (the
/// IR's defect 22): a first-strike step and the regular step each trigger
/// it (CR 510.4, 603.2c).
#[test]
fn cr_510_4_haliya_triggers_in_the_first_strike_and_the_regular_damage_step() {
    let mut g = scen()
        .battlefield(0, &["Haliya, Ascendant Cadet", "Test Striker", "Test Bear"])
        .counters(0, "Test Striker", Counter::P1P1, 1)
        .counters(0, "Test Bear", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let (striker, bear) = (bf(&g, 0, "Test Striker"), bf(&g, 0, "Test Bear"));
    let n = hand(&g, 0);
    attack_with(&mut g, &[striker, bear]);
    through_combat(&mut g, |_| None);
    assert_eq!(g.life(1), 20 - 3 - 3);
    assert_eq!(hand(&g, 0), n + 2, "two combat damage steps, two cards\n{}", log(&g));
}

// ---- Elegy Acolyte (docs/ir_defects.md 63) ----------------------------------

/// Elegy Acolyte: "Lifelink / Whenever one or more creatures you control
/// deal combat damage to a player, you draw a card and lose 1 life." One
/// batch, one trigger (CR 603.2c).
#[test]
fn cr_603_2c_elegy_acolyte_draws_and_loses_one_once_per_batch() {
    let mut g = scen().battlefield(0, &["Elegy Acolyte", "Test Bear"]).at(Step::Main1, 0).start();
    let (a, bear) = (bf(&g, 0, "Elegy Acolyte"), bf(&g, 0, "Test Bear"));
    let n = hand(&g, 0);
    attack_with(&mut g, &[a, bear]);
    through_combat(&mut g, |_| None);
    assert_eq!(g.life(1), 14);
    assert_eq!(g.life(0), 20 + 4 - 1, "lifelink 4, then lose 1");
    assert_eq!(hand(&g, 0), n + 1, "{}", log(&g));
}

/// Elegy Acolyte's batch in a first-strike step and in the regular step:
/// twice (no once-each-turn limit, the IR's defect 63).
#[test]
fn cr_510_4_elegy_acolyte_triggers_in_the_first_strike_and_the_regular_damage_step() {
    let mut g = scen().battlefield(0, &["Elegy Acolyte", "Test Striker", "Test Bear"]).at(Step::Main1, 0).start();
    let (striker, bear) = (bf(&g, 0, "Test Striker"), bf(&g, 0, "Test Bear"));
    let n = hand(&g, 0);
    attack_with(&mut g, &[striker, bear]);
    through_combat(&mut g, |_| None);
    assert_eq!(g.life(1), 16);
    assert_eq!(g.life(0), 18, "lost 1 twice");
    assert_eq!(hand(&g, 0), n + 2, "{}", log(&g));
}

/// Elegy Acolyte: "Void — At the beginning of your end step, if a nonland
/// permanent left the battlefield this turn or a spell was warped this
/// turn, create a 2/2 colorless Robot artifact creature token."
#[test]
fn elegy_acolyte_void_creates_a_robot_at_your_end_step() {
    // A nonland permanent left (a creature sacrificed).
    let mut g = scen().battlefield(0, &["Elegy Acolyte", "High Market", "Test Bear"]).at(Step::Main1, 0).start();
    let bear = bf(&g, 0, "Test Bear");
    sacrifice_to_market(&mut g, bear, |_| None);
    quiet_until(&mut g, main1_of(1));
    let r = tokens_named(&g, 0, "Robot");
    assert_eq!(r.len(), 1, "{}", log(&g));
    assert!(g.is_type(r[0], types::ARTIFACT) && g.is_creature(r[0]) && g.pt(r[0]) == (2, 2) && g.colors_of(r[0]) == 0);
    // A spell warped.
    let mut g = scen().battlefield(0, &["Elegy Acolyte", "Plains", "Plains"]).hand(0, &["Knight Luminary"]).at(Step::Main1, 0).start();
    let k = g.card(0, "Knight Luminary", Zone::Hand);
    start_with(&mut g, Choice::Cast(k), |g| pick(g, &[Choice::Number(1)]));
    settle(&mut g);
    quiet_until(&mut g, main1_of(1));
    assert_eq!(tokens_named(&g, 0, "Robot").len(), 1, "warped\n{}", log(&g));
    // Neither: no Robot, at the end of either turn.
    let mut g = scen().battlefield(0, &["Elegy Acolyte"]).at(Step::Main1, 0).start();
    quiet_until(&mut g, main1_of(1));
    quiet_until(&mut g, main1_of(0));
    assert!(tokens_named(&g, 0, "Robot").is_empty());
}

// ---- Gravkill (docs/ir_defects.md 62) ---------------------------------------

/// Gravkill: "Exile target creature or Spacecraft." A Spacecraft below its
/// threshold (a noncreature artifact, CR 722.2b) is a legal target; another
/// noncreature artifact, a land (a Planet with station too) isn't.
#[test]
fn gravkill_exiles_target_creature_or_spacecraft() {
    for pick_ship in [true, false] {
        let mut g = scen()
            .hand(0, &["Gravkill"])
            .battlefield(0, &["Swamp"; 4])
            .battlefield(1, &["Wurmwall Sweeper", "Test Bear", "Nutrient Block", "Evendo, Waking Haven"])
            .at(Step::Main1, 0)
            .start();
        let (ship, bear, block, planet) = (bf(&g, 1, "Wurmwall Sweeper"), bf(&g, 1, "Test Bear"), bf(&g, 1, "Nutrient Block"), bf(&g, 1, "Evendo, Waking Haven"));
        assert!(!g.is_creature(ship), "below its threshold");
        let c = g.card(0, "Gravkill", Zone::Hand);
        g.choose(Choice::Cast(c));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
        assert_eq!(g.kind(), Some(DecisionKind::Target));
        assert!(g.has_option(g.tgt(ship)), "a Spacecraft");
        assert!(g.has_option(g.tgt(bear)), "a creature");
        assert!(!g.has_option(g.tgt(block)) && !g.has_option(g.tgt(planet)), "not another artifact, not a land");
        assert!(!g.has_option(Choice::Target(Target::Player(1))));
        let t = if pick_ship { ship } else { bear };
        let tc = g.tgt(t);
        g.choose(tc);
        run_until(&mut g, at_priority, |_| None);
        settle(&mut g);
        assert_eq!(g.obj(t).zone, Zone::Exile, "{}", g.name(t));
    }
}

// ---- Dawnsire, Sunstar Dreadnought (docs/ir_defects.md 81) -----------------

/// Dawnsire: "Station (... It's an artifact creature at 20+.) / 20+ |
/// Flying" — a 20/20 flying artifact creature at 20 charge counters, a
/// noncreature artifact below (CR 722.2b).
#[test]
fn cr_722_2b_dawnsire_is_a_20_20_flier_at_twenty() {
    for k in [0, 10, 19, 20] {
        let g = scen().battlefield(0, &["Dawnsire, Sunstar Dreadnought"]).counters(0, "Dawnsire, Sunstar Dreadnought", charge(), k).at(Step::Main1, 0).start();
        let d = bf(&g, 0, "Dawnsire, Sunstar Dreadnought");
        assert!(g.is_type(d, types::ARTIFACT) && subtype(&g, d, "Spacecraft"));
        assert_eq!(g.is_creature(d), k >= 20, "{k} counters");
        assert_eq!(g.has_kw(d, Kw::Flying), k >= 20, "{k} counters");
        if k >= 20 {
            assert_eq!(g.pt(d), (20, 20));
        }
        let a = g.def_of(d).activated.last().unwrap();
        assert!(a.sorcery && a.cost.tap_untapped.is_some(), "station");
    }
}

/// Dawnsire: "10+ | Whenever you attack, Dawnsire deals 100 damage to up to
/// one target creature or planeswalker." Once per attack (CR 508.3a: "you
/// attack" is one event however many creatures attack); a planeswalker
/// loses 100 loyalty (CR 120.3c) and dies (704.5i); a creature gets lethal
/// damage; zero targets is legal (115.1d); at 9 counters nothing.
#[test]
fn cr_120_3c_dawnsire_deals_100_to_up_to_one_creature_or_planeswalker_when_you_attack() {
    let setup = |k: u32| {
        scen()
            .battlefield(0, &["Dawnsire, Sunstar Dreadnought", "Test Bear", "Test Flyer"])
            .counters(0, "Dawnsire, Sunstar Dreadnought", charge(), k)
            .battlefield(1, &["Tezzeret, Cruel Captain", "Test Trampler"])
            .at(Step::Main1, 0)
            .start()
    };
    // A planeswalker.
    let mut g = setup(10);
    let (bear, flyer) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Flyer"));
    let (tez, trampler) = (bf(&g, 1, "Tezzeret, Cruel Captain"), bf(&g, 1, "Test Trampler"));
    attack_with(&mut g, &[bear, flyer]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.stack().is_empty() && g.waiting_triggers().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    assert!(g.has_option(g.tgt(tez)), "a planeswalker");
    assert!(g.has_option(g.tgt(trampler)) && g.has_option(g.tgt(bear)), "a creature");
    assert!(!g.has_option(Choice::Target(Target::Player(1))) && !g.has_option(Choice::Target(Target::Player(0))), "not a player");
    assert!(g.has_option(Choice::Pass), "up to one");
    let t = g.tgt(tez);
    g.choose(t);
    resolve_with(&mut g, |g| {
        assert_ne!(g.kind(), Some(DecisionKind::Target), "one trigger for two attackers\n{}", log(g));
        None
    });
    assert_eq!(g.obj(tez).zone, Zone::Graveyard, "100 damage to a planeswalker\n{}", log(&g));
    // A creature.
    let mut g = setup(10);
    let (bear, trampler) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Trampler"));
    let tt = g.tgt(trampler);
    attack_with(&mut g, &[bear]);
    resolve_with(&mut g, move |g| pick(g, &[tt]));
    assert_eq!(g.obj(trampler).zone, Zone::Graveyard, "100 damage to a creature");
    // No target.
    let mut g = setup(10);
    let bear = bf(&g, 0, "Test Bear");
    attack_with(&mut g, &[bear]);
    resolve_with(&mut g, |g| pick(g, &[Choice::Pass]));
    assert_eq!(count_named(&g, 1, "Tezzeret, Cruel Captain") + count_named(&g, 1, "Test Trampler"), 2, "nothing dealt damage");
    // Nine counters: no ability.
    let mut g = setup(9);
    let bear = bf(&g, 0, "Test Bear");
    attack_with(&mut g, &[bear]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert!(g.stack().is_empty() && g.kind() != Some(DecisionKind::Target), "9 counters: no trigger\n{}", log(&g));
}

// ---- Entropic Battlecruiser (docs/ir_defects.md 82) -------------------------

/// Entropic Battlecruiser: "Station (... It's an artifact creature at 8+.)
/// / 8+ | Flying, deathtouch" — a 3/10 at 8 charge counters.
#[test]
fn cr_722_2b_entropic_battlecruiser_is_a_3_10_flier_with_deathtouch_at_eight() {
    for k in [0, 7, 8] {
        let g = scen().battlefield(0, &["Entropic Battlecruiser"]).counters(0, "Entropic Battlecruiser", charge(), k).at(Step::Main1, 0).start();
        let b = bf(&g, 0, "Entropic Battlecruiser");
        assert_eq!(g.is_creature(b), k >= 8, "{k}");
        assert_eq!(g.has_kw(b, Kw::Flying) && g.has_kw(b, Kw::Deathtouch), k >= 8, "{k}");
        if k >= 8 {
            assert_eq!(g.pt(b), (3, 10));
        }
    }
}

/// Entropic Battlecruiser: "1+ | Whenever an opponent discards a card, they
/// lose 3 life." The discarding player loses it: no target (CR 115.1; the
/// IR wrote a target, docs/ir_defects.md 82); you discarding doesn't
/// trigger it; with no charge counter, nothing.
#[test]
fn entropic_battlecruiser_makes_a_discarding_opponent_lose_three() {
    for k in [0u32, 1] {
        let mut g = scen()
            .battlefield(0, &["Entropic Battlecruiser"])
            .counters(0, "Entropic Battlecruiser", charge(), k)
            .battlefield(1, &["Mountain"])
            .hand(1, &["Test Rummage", "Test Bear"])
            .at(Step::Main1, 1)
            .start();
        let r = g.card(1, "Test Rummage", Zone::Hand);
        let bear = g.card(1, "Test Bear", Zone::Hand);
        let mut asked_p0 = false;
        g.choose(Choice::Cast(r));
        for _ in 0..50 {
            if g.stack().is_empty() && at_priority(&g) {
                break;
            }
            let d = g.pending_decision().unwrap();
            if d.kind == DecisionKind::Target && d.player == 0 {
                asked_p0 = true;
            }
            let c = if d.kind == DecisionKind::Priority { Choice::Pass } else { pick(&g, &[Choice::Card(bear)]).unwrap_or(d.options[0].choice) };
            g.choose(c);
        }
        assert_eq!(g.obj(bear).zone, Zone::Graveyard, "discarded\n{}", log(&g));
        assert!(!asked_p0, "no target");
        assert_eq!(g.life(1), if k == 1 { 17 } else { 20 }, "{k} counters\n{}", log(&g));
    }
    // You discarding: nothing.
    let mut g = scen()
        .battlefield(0, &["Entropic Battlecruiser", "Mountain"])
        .counters(0, "Entropic Battlecruiser", charge(), 1)
        .hand(0, &["Test Rummage", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast_and_resolve(&mut g, 0, "Test Rummage", |_| None);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
    assert_eq!((g.life(0), g.life(1)), (20, 20));
}

/// Entropic Battlecruiser: "Whenever this Spacecraft attacks, each opponent
/// discards a card. Each opponent who can't loses 3 life." With a card, the
/// opponent discards it (their choice) and the 1+ ability makes them lose
/// 3; with an empty hand, they lose 3 from the attack trigger itself.
#[test]
fn entropic_battlecruiser_attacks_each_opponent_discards_or_loses_three() {
    for opp_hand in [&["Test Bear"][..], &[][..]] {
        let mut g = scen()
            .battlefield(0, &["Entropic Battlecruiser"])
            .counters(0, "Entropic Battlecruiser", charge(), 8)
            .hand(1, opp_hand)
            .at(Step::Main1, 0)
            .start();
        let b = bf(&g, 0, "Entropic Battlecruiser");
        attack_with(&mut g, &[b]);
        resolve_with(&mut g, |g| {
            let d = g.pending_decision()?;
            assert!(d.kind != DecisionKind::Target, "no target: {:?}", g.options_desc());
            if d.kind == DecisionKind::Discard {
                assert_eq!(d.player, 1, "the opponent chooses what to discard");
            }
            None
        });
        assert_eq!(g.turn().step, Step::DeclareAttackers, "{}", log(&g));
        assert_eq!(g.life(1), 17, "hand {opp_hand:?}: lose 3\n{}", log(&g));
        assert_eq!(g.player(1).hand.len(), 0);
        assert_eq!(g.player(1).graveyard.len(), opp_hand.len(), "discarded");
        through_combat(&mut g, |_| None);
        assert_eq!(g.life(1), 14, "3 combat damage");
    }
}

// ---- Nexus of Fate (docs/ir_defects.md 64) -----------------------------------

/// The EoE table plus a counterspell ("Counter target spell.", Counterspell's
/// IR), to counter Nexus of Fate: the set has none that counters an instant.
fn counter_db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_eoe_document().cards;
        let counter: card_ir::Card = serde_json::from_value(json!({"name": "Test Counterspell", "custom": true, "printing": {
            "$t": "CardPrinting", "card_types": ["Instant"], "mana_cost": {"$t": "ManaCost", "colored": [["U", 2]]},
            "mana_cost_str": "{U}{U}", "name": "Test Counterspell", "spell_effect": {"$t": "CounterTargetSpell"},
            "subtypes": [], "supertypes": [], "type_line": "Instant"}}))
        .expect("test counterspell IR");
        cards.push(counter);
        let db = CardDb::from_cards(&cards);
        db.expect_id("Test Counterspell").unwrap_or_else(|e| panic!("{e}"));
        db.leak()
    })
}

/// Nexus of Fate: "Take an extra turn after this one. / If Nexus of Fate
/// would be put into a graveyard from anywhere, reveal Nexus of Fate and
/// shuffle it into its owner's library instead." (CR 500.7, 614.1a.)
/// Resolved, it goes to the library, not the graveyard.
#[test]
fn cr_614_1a_nexus_of_fate_takes_an_extra_turn_and_shuffles_into_the_library() {
    let mut g = scen().hand(0, &["Nexus of Fate"]).battlefield(0, &["Island"; 7]).at(Step::Main1, 0).start();
    let lib = g.player(0).library.len();
    cast_and_resolve(&mut g, 0, "Nexus of Fate", |_| None);
    assert_eq!(g.count_in(0, "Nexus of Fate", Zone::Graveyard), 0, "not the graveyard");
    assert_eq!(g.count_in(0, "Nexus of Fate", Zone::Library), 1, "shuffled into the library");
    assert_eq!(g.player(0).library.len(), lib + 1);
    quiet_until(&mut g, |g| g.turn().number == 4 && at_priority(g));
    assert_eq!(g.turn().active, 0, "the extra turn");
    quiet_until(&mut g, |g| g.turn().number == 5 && at_priority(g));
    assert_eq!(g.turn().active, 1);
}

/// Discarded, Nexus of Fate goes to its owner's library ("from anywhere":
/// the hand). Temporal Intervention: "Target opponent reveals their hand.
/// You choose a nonland card from it. That player discards that card."
#[test]
fn cr_614_1a_a_discarded_nexus_of_fate_goes_to_the_library() {
    let mut g = scen()
        .hand(0, &["Temporal Intervention"])
        .battlefield(0, &["Swamp", "Swamp", "Swamp"])
        .hand(1, &["Nexus of Fate", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let n = g.card(1, "Nexus of Fate", Zone::Hand);
    let lib = g.player(1).library.len();
    cast_and_resolve(&mut g, 0, "Temporal Intervention", move |g| pick(g, &[Choice::Target(Target::Player(1)), Choice::Card(n)]));
    assert!(log(&g).contains("Nexus of Fate is shuffled into its owner's library instead"), "{}", log(&g));
    assert_eq!(g.count_in(1, "Nexus of Fate", Zone::Graveyard), 0);
    assert_eq!(g.count_in(1, "Nexus of Fate", Zone::Library), 1, "its owner's library");
    assert_eq!(g.player(1).library.len(), lib + 1);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Hand), 1);
}

/// Surveilled into the graveyard, Nexus of Fate stays in the library ("from
/// anywhere": the library; shuffled). Wurmwall Sweeper: "When this
/// Spacecraft enters, surveil 2."
#[test]
fn cr_614_1a_a_surveilled_nexus_of_fate_goes_to_the_library() {
    let mut g = scen().library(0, &["Nexus of Fate", "Island"]).hand(0, &["Wurmwall Sweeper"]).battlefield(0, &["Swamp", "Swamp"]).at(Step::Main1, 0).start();
    let n = g.find_card(0, "Nexus of Fate", Zone::Library).unwrap();
    let lib = g.player(0).library.len();
    cast_and_resolve(&mut g, 0, "Wurmwall Sweeper", move |g| pick(g, &[Choice::Card(n), Choice::Def(def("Nexus of Fate"))]));
    assert!(log(&g).contains("Nexus of Fate is shuffled into its owner's library instead"), "{}", log(&g));
    assert_eq!(g.count_in(0, "Nexus of Fate", Zone::Graveyard), 0);
    assert_eq!(g.count_in(0, "Nexus of Fate", Zone::Library), 1);
    assert_eq!(g.player(0).library.len(), lib);
}

/// Countered, Nexus of Fate goes to the library, and no extra turn is taken
/// (CR 701.6a: a countered spell is put into its owner's graveyard, here
/// replaced).
#[test]
fn cr_701_6a_a_countered_nexus_of_fate_goes_to_the_library() {
    let mut g = Scenario::new(counter_db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .hand(0, &["Nexus of Fate"])
        .battlefield(0, &["Island"; 7])
        .hand(1, &["Test Counterspell"])
        .battlefield(1, &["Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let n = g.card(0, "Nexus of Fate", Zone::Hand);
    start_with(&mut g, Choice::Cast(n), |_| None);
    g.pass();
    assert_eq!(g.to_move(), Some(1));
    let cs = g.card(1, "Test Counterspell", Zone::Hand);
    let tn = g.tgt(n);
    start_with(&mut g, Choice::Cast(cs), move |g| pick(g, &[tn]));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Counterspell", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Nexus of Fate", Zone::Graveyard), 0, "{}", log(&g));
    assert_eq!(g.count_in(0, "Nexus of Fate", Zone::Library), 1, "countered: into the library");
    quiet_until(&mut g, |g| g.turn().number == 4 && at_priority(g));
    assert_eq!(g.turn().active, 1, "no extra turn");
}
