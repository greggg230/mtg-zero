//! M7b card tests, lands: every supported cube land no earlier test named,
//! checked against its Oracle text (CR 305 lands, 305.6 basic land types,
//! 614.1c "enters tapped", 614.12 shock lands, 701.19 searches, 701.42
//! surveil, 702.29 cycling, 605 mana abilities, 119.4 life payments).
//! Cycles are table-driven: each row names a card and its specifics.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter};
use engine::mana::ManaCost;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const LIB: &[&str] = &["Island"; 10];

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, LIB).library(1, LIB)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap()
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn settle(g: &mut Game) {
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
}

fn play(g: &mut Game, name: &str) -> CardIdx {
    let c = g.card(0, name, Zone::Hand);
    g.choose(Choice::Play(c));
    c
}

/// The colours (of "WUBRG") one copy of `name` on the battlefield, alone,
/// can pay one mana of; and it makes one mana only (it can't pay {2}).
fn land_colors(name: &str) -> String {
    let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{2}")), "{name} makes one mana");
    "WUBRG".chars().filter(|c| g.can_pay(0, &cost(&format!("{{{c}}}")))).collect()
}

/// `name` has exactly the basic land types `types` (CR 305.6, 205.3i).
fn assert_land_types(name: &str, types: &[&str]) {
    let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
    let c = bf(&g, 0, name);
    for t in ["Plains", "Island", "Swamp", "Mountain", "Forest"] {
        let has = db().subtype_id(t).is_some_and(|s| g.has_subtype(c, s));
        assert_eq!(has, types.contains(&t), "{name}: land type {t}");
    }
}

/// The definitions offered by a library search.
fn offered_defs(g: &Game) -> Vec<u16> {
    let d = g.pending_decision().unwrap();
    assert_eq!(d.kind, DecisionKind::SearchLibrary, "{:?}", g.options_desc());
    let mut v: Vec<u16> = d
        .options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Def(x) => Some(x),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

// ---- fetch lands -----------------------------------------------------------

/// The library a fetch searches: the five basics, two dual lands (Tundra is
/// a Plains Island, Bayou a Swamp Forest), a typeless nonbasic land and a
/// spell.
const FETCH_LIB: &[&str] =
    &["Plains", "Island", "Swamp", "Mountain", "Forest", "Tundra", "Bayou", "Wasteland", "Lightning Bolt"];

/// "{T}, Pay 1 life, Sacrifice this land: Search your library for a <A> or
/// <B> card, put it onto the battlefield, then shuffle." Any card with one
/// of the land types qualifies (a dual land too), nothing else; the land
/// enters untapped; the search may fail (701.19b: a stated quality).
#[test]
fn fetch_lands_find_a_land_card_with_one_of_their_two_types_for_one_life() {
    // ruling: 576821b5
    let rows: &[(&str, [&str; 2])] = &[
        ("Arid Mesa", ["Mountain", "Plains"]),
        ("Bloodstained Mire", ["Swamp", "Mountain"]),
        ("Marsh Flats", ["Plains", "Swamp"]),
        ("Misty Rainforest", ["Forest", "Island"]),
        ("Scalding Tarn", ["Island", "Mountain"]),
        ("Verdant Catacombs", ["Swamp", "Forest"]),
        ("Wooded Foothills", ["Mountain", "Forest"]),
    ];
    for &(fetch, types) in rows {
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .battlefield(0, &[fetch])
            .library(0, FETCH_LIB)
            .library(1, LIB)
            .at(Step::Main1, 0)
            .start();
        let f = bf(&g, 0, fetch);
        // No mana ability: the fetch land pays for nothing.
        assert!(!g.can_pay(0, &cost("{1}")), "{fetch} makes no mana");
        g.choose(act(f, 0));
        g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
        assert_eq!(g.life(0), 19, "{fetch}: 1 life");
        assert_eq!(g.obj(f).zone, Zone::Graveyard, "{fetch}: sacrificed");
        let mut want: Vec<u16> = ["Plains", "Island", "Swamp", "Mountain", "Forest"]
            .into_iter()
            .filter(|t| types.contains(t))
            .map(def)
            .collect();
        if types.iter().any(|t| *t == "Plains" || *t == "Island") {
            want.push(def("Tundra"));
        }
        if types.iter().any(|t| *t == "Swamp" || *t == "Forest") {
            want.push(def("Bayou"));
        }
        want.sort();
        assert_eq!(offered_defs(&g), want, "{fetch}: a {} or {} card", types[0], types[1]);
        assert!(g.has_option(Choice::Pass), "{fetch}: may find nothing");
        let pick = types[1];
        g.choose(Choice::Def(def(pick)));
        let found = bf(&g, 0, pick);
        assert!(!g.obj(found).tapped, "{fetch}: onto the battlefield untapped");
        assert_eq!(g.player(0).library.len(), FETCH_LIB.len() - 1);
        settle(&mut g);
    }
}

#[test]
fn prismatic_vista_finds_a_basic_land_card_only() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Prismatic Vista"])
        .library(0, FETCH_LIB)
        .library(1, LIB)
        .at(Step::Main1, 0)
        .start();
    let v = bf(&g, 0, "Prismatic Vista");
    g.choose(act(v, 0));
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.life(0), 19);
    assert_eq!(g.obj(v).zone, Zone::Graveyard);
    let mut want: Vec<u16> = ["Plains", "Island", "Swamp", "Mountain", "Forest"].map(def).to_vec();
    want.sort();
    assert_eq!(offered_defs(&g), want, "basic land cards only (no Tundra, Bayou, Wasteland)");
    g.choose(Choice::Def(def("Swamp")));
    assert!(!g.obj(bf(&g, 0, "Swamp")).tapped);
}

// ---- surveil lands ---------------------------------------------------------

/// "(T: Add A or B.) This land enters tapped. When this land enters, surveil
/// 1." They have both basic land types (so fetch lands find them).
#[test]
fn surveil_lands_enter_tapped_surveil_one_and_have_two_land_types() {
    let rows: &[(&str, &str, [&str; 2])] = &[
        ("Commercial District", "RG", ["Mountain", "Forest"]),
        ("Elegant Parlor", "WR", ["Mountain", "Plains"]),
        ("Lush Portico", "WG", ["Forest", "Plains"]),
        ("Meticulous Archive", "WU", ["Plains", "Island"]),
        ("Raucous Theater", "BR", ["Swamp", "Mountain"]),
        ("Shadowy Backstreet", "WB", ["Plains", "Swamp"]),
        ("Thundering Falls", "UR", ["Island", "Mountain"]),
        ("Undercity Sewers", "UB", ["Island", "Swamp"]),
        ("Underground Mortuary", "BG", ["Swamp", "Forest"]),
    ];
    for &(name, colors, types) in rows {
        assert_eq!(land_colors(name), colors, "{name}");
        assert_land_types(name, &types);
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .hand(0, &[name])
            .library(0, &["Lightning Bolt", "Island", "Island"])
            .library(1, LIB)
            .at(Step::Main1, 0)
            .start();
        let c = play(&mut g, name);
        assert!(g.obj(c).tapped, "{name}: enters tapped (614.1c)");
        g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
        // 701.42a: look at the top card, may put it into the graveyard.
        assert_eq!(g.kind(), Some(DecisionKind::PutInGraveyard), "{name}");
        assert_eq!(g.pending_decision().unwrap().options.len(), 2, "{name}: the top card or pass");
        let bolt = g.card(0, "Lightning Bolt", Zone::Library);
        g.choose(Choice::Card(bolt));
        settle(&mut g);
        assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "{name}");
        assert_eq!(g.player(0).library.len(), 2, "surveil 1: one card looked at");
    }
}

// ---- fast lands ------------------------------------------------------------

/// "This land enters tapped unless you control two or fewer other lands.
/// {T}: Add A or B." (614.1c, the other lands counted as it enters.)
#[test]
fn fast_lands_enter_untapped_with_two_or_fewer_other_lands() {
    // ruling: 3b8f4b59 a32635f0
    let rows: &[(&str, &str)] = &[
        ("Blooming Marsh", "BG"),
        ("Botanical Sanctum", "UG"),
        ("Concealed Courtyard", "WB"),
        ("Copperline Gorge", "RG"),
        ("Darkslick Shores", "UB"),
        ("Inspiring Vantage", "WR"),
        ("Razorverge Thicket", "WG"),
        ("Seachrome Coast", "WU"),
        ("Spirebluff Canal", "UR"),
    ];
    for &(name, colors) in rows {
        assert_eq!(land_colors(name), colors, "{name}");
        assert_land_types(name, &[]);
        let mut g = scen().battlefield(0, &["Swamp", "Mountain"]).hand(0, &[name]).at(Step::Main1, 0).start();
        let c = play(&mut g, name);
        assert!(!g.obj(c).tapped, "{name}: two other lands");
        let mut g = scen().battlefield(0, &["Swamp", "Mountain", "Island"]).hand(0, &[name]).at(Step::Main1, 0).start();
        let c = play(&mut g, name);
        assert!(g.obj(c).tapped, "{name}: three other lands");
        let mut g = scen().hand(0, &[name]).at(Step::Main1, 0).start();
        let c = play(&mut g, name);
        assert!(!g.obj(c).tapped, "{name}: no other land");
    }
}

// ---- horizon lands ---------------------------------------------------------

/// "{T}, Pay 1 life: Add A or B. {1}, {T}, Sacrifice this land: Draw a card."
#[test]
fn horizon_lands_pay_one_life_per_mana_and_sacrifice_to_draw() {
    let rows: &[(&str, &str)] =
        &[("Fiery Islet", "UR"), ("Nurturing Peatland", "BG"), ("Sunbaked Canyon", "WR"), ("Waterlogged Grove", "UG")];
    for &(name, colors) in rows {
        assert_eq!(land_colors(name), colors, "{name}");
        // Paying a spell's {1} with it costs 1 life.
        let mut g = scen().battlefield(0, &[name]).hand(0, &["Test Zero"]).at(Step::Main1, 0).start();
        let z = g.card(0, "Test Zero", Zone::Hand);
        g.choose(Choice::Cast(z));
        g.drive_until(at_priority);
        assert_eq!(g.life(0), 19, "{name}: {{T}}, Pay 1 life");
        assert!(g.obj(bf(&g, 0, name)).tapped);
        // 119.4: at 1 life it can still pay (going to 0).
        let g = scen().battlefield(0, &[name]).life(0, 1).at(Step::Main1, 0).start();
        assert!(g.can_pay(0, &cost("{1}")), "{name}: 1 life can be paid at 1 life");
        // {1}, {T}, sacrifice: draw a card (its own mana can't pay the {1}).
        let mut g = scen().battlefield(0, &[name, "Island"]).at(Step::Main1, 0).start();
        let h = bf(&g, 0, name);
        assert!(g.has_option(act(h, 1)), "{name}: the draw ability");
        g.choose(act(h, 1));
        g.drive_until(at_priority);
        assert_eq!(g.obj(h).zone, Zone::Graveyard, "{name}: sacrificed");
        settle(&mut g);
        assert_eq!(g.player(0).hand.len(), 1, "{name}: drew a card");
        assert_eq!(g.life(0), 20, "{name}: the Island paid the {{1}}");
        // Without another mana source the ability can't be activated.
        let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
        let h = bf(&g, 0, name);
        assert!(!g.has_option(act(h, 1)), "{name}: its {{T}} can't pay its own {{1}}");
    }
}

// ---- shock lands -----------------------------------------------------------

/// "(T: Add A or B.) As this land enters, you may pay 2 life. If you don't,
/// it enters tapped." (614.12; 119.4 below 2 life.)
#[test]
fn shock_lands_pay_two_life_or_enter_tapped() {
    let rows: &[(&str, &str, [&str; 2])] = &[
        ("Godless Shrine", "WB", ["Plains", "Swamp"]),
        ("Hallowed Fountain", "WU", ["Plains", "Island"]),
        ("Overgrown Tomb", "BG", ["Swamp", "Forest"]),
        ("Sacred Foundry", "WR", ["Mountain", "Plains"]),
        ("Stomping Ground", "RG", ["Mountain", "Forest"]),
        ("Temple Garden", "WG", ["Forest", "Plains"]),
    ];
    for &(name, colors, types) in rows {
        assert_eq!(land_colors(name), colors, "{name}");
        assert_land_types(name, &types);
        let mut g = scen().hand(0, &[name]).at(Step::Main1, 0).start();
        let c = play(&mut g, name);
        assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(0)), "{name}");
        g.choose(Choice::Yes);
        assert!(!g.obj(c).tapped && g.life(0) == 18, "{name}: paid 2 life, untapped");
        let mut g = scen().hand(0, &[name]).at(Step::Main1, 0).start();
        let c = play(&mut g, name);
        g.choose(Choice::Pass);
        assert!(g.obj(c).tapped && g.life(0) == 20, "{name}: declined, tapped");
        let mut g = scen().hand(0, &[name]).life(0, 1).at(Step::Main1, 0).start();
        play(&mut g, name);
        assert_eq!(g.options_desc(), vec!["0=pass".to_string()], "{name}: 1 life can't pay 2");
    }
}

// ---- dual lands ------------------------------------------------------------

/// The original duals: "(T: Add A or B.)" from their two basic land types
/// (305.6), untapped, nothing else.
#[test]
fn dual_lands_tap_for_either_of_their_two_basic_land_types() {
    // ruling: 2ad14f7c
    let rows: &[(&str, &str, [&str; 2])] = &[
        ("Plateau", "WR", ["Mountain", "Plains"]),
        ("Savannah", "WG", ["Forest", "Plains"]),
        ("Scrubland", "WB", ["Plains", "Swamp"]),
        ("Taiga", "RG", ["Mountain", "Forest"]),
    ];
    for &(name, colors, types) in rows {
        assert_eq!(land_colors(name), colors, "{name}");
        assert_land_types(name, &types);
        let mut g = scen().hand(0, &[name]).at(Step::Main1, 0).start();
        let c = play(&mut g, name);
        assert!(!g.obj(c).tapped && at_priority(&g), "{name}: enters untapped, no choice");
        assert_eq!(g.def_of(c).supertypes & engine::db::supertypes::BASIC, 0, "{name} is not basic");
    }
}

// ---- Triomes and the Streets of New Capenna tri-lands -----------------------

/// "(T: Add A, B, or C.) This land enters tapped. Cycling {3}."
#[test]
fn triomes_enter_tapped_tap_for_three_colors_and_cycle_for_three() {
    let rows: &[(&str, &str, [&str; 3])] = &[
        ("Indatha Triome", "WBG", ["Plains", "Swamp", "Forest"]),
        ("Raugrin Triome", "WUR", ["Island", "Mountain", "Plains"]),
        ("Savai Triome", "WBR", ["Mountain", "Plains", "Swamp"]),
        ("Zagoth Triome", "UBG", ["Swamp", "Forest", "Island"]),
        ("Jetmir's Garden", "WRG", ["Mountain", "Forest", "Plains"]),
        ("Spara's Headquarters", "WUG", ["Forest", "Plains", "Island"]),
        ("Xander's Lounge", "UBR", ["Island", "Swamp", "Mountain"]),
        ("Ziatora's Proving Ground", "BRG", ["Swamp", "Mountain", "Forest"]),
    ];
    for &(name, colors, types) in rows {
        assert_eq!(land_colors(name), colors, "{name}");
        assert_land_types(name, &types);
        let mut g = scen().hand(0, &[name]).at(Step::Main1, 0).start();
        let c = play(&mut g, name);
        assert!(g.obj(c).tapped, "{name}: enters tapped");
        // Cycling {3} (702.29): from hand, at instant speed (the upkeep).
        let mut g = scen().battlefield(0, &["Island"; 3]).hand(0, &[name]).at(Step::Upkeep, 0).start();
        let c = g.card(0, name, Zone::Hand);
        assert!(g.has_option(Choice::Ability(c, AbilitySlot::Hand(0))), "{name}: cycling");
        g.choose(Choice::Ability(c, AbilitySlot::Hand(0)));
        g.drive_until(at_priority);
        assert_eq!(g.obj(c).zone, Zone::Graveyard, "{name}: discarded as the cost");
        assert!(g.player(0).hand.is_empty());
        settle(&mut g);
        assert_eq!(g.player(0).hand.len(), 1, "{name}: drew a card");
        assert!(g.battlefield().iter().all(|&l| g.obj(l).tapped), "{name}: {{3}} paid");
        // Two lands can't pay {3}.
        let g = scen().battlefield(0, &["Island"; 2]).hand(0, &[name]).at(Step::Upkeep, 0).start();
        let c = g.card(0, name, Zone::Hand);
        assert!(!g.has_option(Choice::Ability(c, AbilitySlot::Hand(0))), "{name}: {{3}}");
    }
}

// ---- Verges ----------------------------------------------------------------

/// "{T}: Add A. {T}: Add B. Activate only if you control a <X> or a <Y>."
#[test]
fn verges_make_their_second_color_only_with_a_land_type() {
    // Bleachbone Verge: {B}; {W} only with a Plains or a Swamp.
    let g = scen().battlefield(0, &["Bleachbone Verge"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{B}")) && !g.can_pay(0, &cost("{W}")));
    let g = scen().battlefield(0, &["Bleachbone Verge", "Mountain"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{W}")), "a Mountain doesn't enable {{W}}");
    let g = scen().battlefield(0, &["Bleachbone Verge", "Swamp"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}{B}")), "a Swamp enables {{W}}");
    let g = scen().battlefield(0, &["Bleachbone Verge", "Plains"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}{W}")), "a Plains enables {{W}}");
    let g = scen().battlefield(0, &["Bleachbone Verge", "Scrubland"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}{W}")), "Scrubland is a Plains Swamp");
    // Thornspire Verge: {R}; {G} only with a Mountain or a Forest.
    let g = scen().battlefield(0, &["Thornspire Verge"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{R}")) && !g.can_pay(0, &cost("{G}")));
    let g = scen().battlefield(0, &["Thornspire Verge", "Island"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{G}")));
    let g = scen().battlefield(0, &["Thornspire Verge", "Mountain"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{G}{R}")));
    let g = scen().battlefield(0, &["Thornspire Verge", "Forest"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{G}{G}")));
    for v in ["Bleachbone Verge", "Thornspire Verge"] {
        assert_land_types(v, &[]);
        let mut g = scen().hand(0, &[v]).at(Step::Main1, 0).start();
        let c = play(&mut g, v);
        assert!(!g.obj(c).tapped, "{v} enters untapped");
        let g = scen().battlefield(0, &[v, "Forest", "Swamp"]).at(Step::Main1, 0).start();
        assert!(!g.can_pay(0, &cost("{4}")), "{v}: one mana, not both abilities");
    }
}

// ---- Tolarian Academy, Abandoned Air Temple ---------------------------------

#[test]
fn tolarian_academy_adds_blue_for_each_artifact_you_control() {
    // "{T}: Add {U} for each artifact you control." Legendary.
    let g = scen().battlefield(0, &["Tolarian Academy"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{U}")), "no artifact, no mana");
    let academy = bf(&g, 0, "Tolarian Academy");
    assert!(g.is_legendary(academy));
    let g = scen()
        .battlefield(0, &["Tolarian Academy", "Skullclamp", "Sensei's Divining Top"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    assert!(g.can_pay(0, &cost("{U}{U}")), "two artifacts you control");
    assert!(!g.can_pay(0, &cost("{U}{U}{U}")), "the opponent's Sol Ring doesn't count");
    // The mana is paid at once by one activation; the extra stays (106.4).
    let mut g = scen()
        .battlefield(0, &["Tolarian Academy", "Skullclamp", "Sensei's Divining Top"])
        .hand(0, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let f = g.card(0, "Test Flyer", Zone::Hand);
    g.choose(Choice::Cast(f)); // {1}{U}
    g.drive_until(at_priority);
    assert!(g.obj(bf(&g, 0, "Tolarian Academy")).tapped);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Flyer", Zone::Battlefield), 1);
}

#[test]
fn abandoned_air_temple_enters_tapped_without_a_basic_and_pumps_your_team() {
    // "This land enters tapped unless you control a basic land."
    let mut g = scen().battlefield(0, &["Tundra"]).hand(0, &["Abandoned Air Temple"]).at(Step::Main1, 0).start();
    let t = play(&mut g, "Abandoned Air Temple");
    assert!(g.obj(t).tapped, "no basic land");
    let mut g = scen().battlefield(0, &["Plains"]).hand(0, &["Abandoned Air Temple"]).at(Step::Main1, 0).start();
    let t = play(&mut g, "Abandoned Air Temple");
    assert!(!g.obj(t).tapped, "a basic land");
    // "{T}: Add {W}."
    assert_eq!(land_colors("Abandoned Air Temple"), "W");
    // "{3}{W}, {T}: Put a +1/+1 counter on each creature you control."
    let mut g = scen()
        .battlefield(0, &["Abandoned Air Temple", "Plains", "Plains", "Plains", "Plains", "Test Bear", "Test Flyer"])
        .battlefield(1, &["Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    let t = bf(&g, 0, "Abandoned Air Temple");
    g.choose(act(t, 1));
    g.drive_until(at_priority);
    assert!(g.obj(t).tapped);
    settle(&mut g);
    for n in ["Test Bear", "Test Flyer"] {
        assert_eq!(g.counters_on(bf(&g, 0, n), Counter::P1P1), 1, "{n}");
    }
    assert_eq!(g.pt(bf(&g, 0, "Test Bear")), (3, 3));
    assert_eq!(g.counters_on(bf(&g, 1, "Test Reacher"), Counter::P1P1), 0, "not the opponent's");
    // Its own {T} can't pay its {3}{W}: four other mana are needed.
    let g = scen().battlefield(0, &["Abandoned Air Temple", "Plains", "Plains", "Plains"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Abandoned Air Temple"), 1)));
}
