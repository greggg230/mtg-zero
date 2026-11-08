//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): the set's
//! counter, artifact and Equipment cards, each against its Oracle text.
//! Test names cite the CR.

mod eoe_a_kit;

use eoe_a_kit::*;
use engine::db::{types, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};

fn cast(g: &mut Game, c: CardIdx) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
}

fn target(g: &mut Game, c: CardIdx) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(c));
}

fn creature(g: &Game, c: CardIdx) -> bool {
    g.types_of(c) & types::CREATURE != 0
}

// ---- Beyond the Quiet ------------------------------------------------------

/// "Exile all creatures and Spacecraft." A Spacecraft below its threshold
/// (not a creature) is exiled too; other artifacts stay.
#[test]
fn beyond_the_quiet_exiles_creatures_and_spacecraft_cr406_2() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 5])
        .battlefield(0, &["Lane A Bear", "Wurmwall Sweeper"])
        .battlefield(1, &["Lane A Giant", "Lane A Trinket", "Lumen-Class Frigate"])
        .hand(0, &["Beyond the Quiet"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Beyond the Quiet", Zone::Hand);
    cast(&mut g, c);
    pay(&mut g);
    settle(&mut g);
    for (p, n) in [(0, "Lane A Bear"), (0, "Wurmwall Sweeper"), (1, "Lane A Giant"), (1, "Lumen-Class Frigate")] {
        assert_eq!(g.count_in(p, n, Zone::Exile), 1, "{n}");
    }
    assert_eq!(g.count_in(1, "Lane A Trinket", Zone::Battlefield), 1);
}

// ---- Cerebral Download -----------------------------------------------------

/// "Surveil X, where X is the number of artifacts you control. Then draw
/// three cards." (CR 701.25a; X counted as it resolves.)
#[test]
fn cerebral_download_surveils_by_artifacts_then_draws_three_cr701_25a() {
    let mut g = scen()
        .battlefield(0, &["Island"; 5])
        .battlefield(0, &["Lane A Trinket", "Lane A Robot", "Lane A Bear"])
        .battlefield(1, &["Lane A Trinket"])
        .hand(0, &["Cerebral Download"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Cerebral Download", Zone::Hand);
    cast(&mut g, c);
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInGraveyard));
    // Two artifacts: the top two cards are looked at.
    let mut milled = 0;
    while g.kind() == Some(DecisionKind::PutInGraveyard) {
        let o = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).map(|o| o.choice);
        match o {
            Some(o) => {
                g.choose(o);
                milled += 1;
            }
            None => {
                g.choose(Choice::Pass);
            }
        }
        g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || g.stack().is_empty());
    }
    settle(&mut g);
    assert_eq!(milled, 2, "{}", log(&g));
    assert_eq!(g.player(0).graveyard.len(), 3, "two surveilled away and the spell");
    assert_eq!(g.player(0).hand.len(), 3);
}

// ---- Drill Too Deep --------------------------------------------------------

/// "Choose one — Put five charge counters on target Spacecraft or Planet you
/// control. • Destroy target artifact." Counters from another effect give
/// a Spacecraft its abilities (station's ruling, CR 722.2b).
#[test]
fn drill_too_deep_puts_five_charge_counters_or_destroys_an_artifact_cr700_2() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Galvanizing Sawship", "Lane A Planet"])
        .battlefield(1, &["Wurmwall Sweeper"])
        .hand(0, &["Drill Too Deep"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Drill Too Deep", Zone::Hand);
    let saw = bf(&g, 0, "Galvanizing Sawship");
    cast(&mut g, c);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bf(&g, 0, "Lane A Planet"))), "a Planet you control");
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Wurmwall Sweeper"))), "not an opponent's");
    g.choose(g.tgt(saw));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(saw, charge()), 5);
    assert!(creature(&g, saw) && g.has_kw(saw, Kw::Haste));
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .battlefield(1, &["Lane A Trinket"])
        .hand(0, &["Drill Too Deep"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Drill Too Deep", Zone::Hand);
    cast(&mut g, c);
    g.choose(Choice::Number(1));
    let trinket = bf(&g, 1, "Lane A Trinket");
    target(&mut g, trinket);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Lane A Trinket", Zone::Graveyard), 1);
}

// ---- Drix Fatemaker --------------------------------------------------------

/// "When this creature enters, put a +1/+1 counter on target creature. Each
/// creature you control with a +1/+1 counter on it has trample." (CR 613.1f;
/// the affected set is checked continuously, 611.3a.)
#[test]
fn drix_fatemaker_grants_trample_to_your_creatures_with_counters_cr611_3a() {
    let mut g = scen()
        .battlefield(0, &["Forest"; 4])
        .battlefield(0, &["Lane A Bear", "Lane A Giant"])
        .battlefield(1, &["Lane A Robot"])
        .counters(1, "Lane A Robot", Counter::P1P1, 1)
        .hand(0, &["Drix Fatemaker"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Drix Fatemaker", Zone::Hand);
    let (bear, giant, robot) = (bf(&g, 0, "Lane A Bear"), bf(&g, 0, "Lane A Giant"), bf(&g, 1, "Lane A Robot"));
    cast(&mut g, c);
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(g.pending_decision().unwrap().options[0].choice);
    }
    pay(&mut g);
    target(&mut g, bear);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "{}", log(&g));
    assert!(g.has_kw(bear, Kw::Trample));
    assert!(!g.has_kw(giant, Kw::Trample), "no counter");
    assert!(!g.has_kw(robot, Kw::Trample), "not yours");
    let drix = bf(&g, 0, "Drix Fatemaker");
    assert!(!g.has_kw(drix, Kw::Trample));
}

// ---- Dual-Sun Technique ----------------------------------------------------

/// "Target creature you control gains double strike until end of turn. If it
/// has a +1/+1 counter on it, draw a card." (Checked as it resolves.)
#[test]
fn dual_sun_technique_draws_only_with_a_counter_cr608_2c() {
    for counter in [0, 1] {
        let mut g = scen()
            .battlefield(0, &["Plains", "Plains", "Lane A Bear"])
            .battlefield(1, &["Lane A Giant"])
            .counters(0, "Lane A Bear", Counter::P1P1, counter)
            .hand(0, &["Dual-Sun Technique"])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, "Dual-Sun Technique", Zone::Hand);
        let bear = bf(&g, 0, "Lane A Bear");
        cast(&mut g, c);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Giant"))), "a creature you control");
        g.choose(g.tgt(bear));
        pay(&mut g);
        settle(&mut g);
        assert!(g.has_kw(bear, Kw::DoubleStrike));
        assert_eq!(g.player(0).hand.len(), counter as usize);
    }
}

// ---- Equipment that attach as they enter ------------------------------------

/// "When this Equipment enters, attach it to target creature you control.
/// That creature gains <keyword> until end of turn." (CR 701.3a.) Illvoi
/// Light Jammer (flash, +1/+2, hexproof), Meltstrider's Gear (+2/+1 and
/// reach), Squire's Lightblade (flash, +1/+0, first strike).
#[test]
fn equipment_attach_to_target_creature_you_control_as_they_enter_cr701_3a() {
    let cases: &[(&str, &str, (i32, i32), &[Kw], &[Kw])] = &[
        ("Illvoi Light Jammer", "Island", (3, 4), &[Kw::Hexproof], &[]),
        ("Meltstrider's Gear", "Forest", (4, 3), &[Kw::Reach], &[Kw::Reach]),
        ("Squire's Lightblade", "Plains", (3, 2), &[Kw::FirstStrike], &[]),
    ];
    for &(name, land, pt, kws, lasting) in cases {
        let mut g = scen()
            .battlefield(0, &[land, land, "Lane A Bear"])
            .battlefield(1, &["Lane A Giant"])
            .hand(0, &[name])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, name, Zone::Hand);
        let bear = bf(&g, 0, "Lane A Bear");
        cast(&mut g, c);
        pay(&mut g);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Giant"))), "{name}: a creature you control");
        g.choose(g.tgt(bear));
        settle(&mut g);
        let eq = bf(&g, 0, name);
        assert_eq!(g.obj(eq).attached.map(|r| r.idx), Some(bear), "{name}");
        assert_eq!((g.power(bear), g.toughness(bear)), pt, "{name}");
        for &k in kws {
            assert!(g.has_kw(bear, k), "{name} {k:?}");
        }
        // Next turn: the until-end-of-turn keyword is gone, the static one
        // stays.
        next_main(&mut g, 1);
        for &k in kws {
            assert_eq!(g.has_kw(bear, k), lasting.contains(&k), "{name} {k:?} next turn");
        }
    }
}

/// Flash (CR 702.8a): Illvoi Light Jammer can be cast in the opponent's turn.
#[test]
fn illvoi_light_jammer_has_flash_cr702_8a() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Lane A Bear"])
        .hand(0, &["Illvoi Light Jammer"])
        .at(Step::Main1, 1)
        .start();
    g.pass();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let c = g.card(0, "Illvoi Light Jammer", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)));
}

// ---- Invasive Maneuvers ----------------------------------------------------

/// "Invasive Maneuvers deals 3 damage to target creature. It deals 5 damage
/// instead if you control a Spacecraft." (Any Spacecraft: one that isn't a
/// creature counts.)
#[test]
fn invasive_maneuvers_deals_five_with_a_spacecraft_cr608_2c() {
    for (ship, dmg) in [(false, 3), (true, 5)] {
        let mut bfv = vec!["Mountain", "Mountain"];
        if ship {
            bfv.push("Wurmwall Sweeper");
        }
        let mut g = scen().battlefield(0, &bfv).battlefield(1, &["Lane A Giant"]).hand(0, &["Invasive Maneuvers"]).at(Step::Main1, 0).start();
        let c = g.card(0, "Invasive Maneuvers", Zone::Hand);
        let giant = bf(&g, 1, "Lane A Giant");
        cast(&mut g, c);
        target(&mut g, giant);
        pay(&mut g);
        settle(&mut g);
        if dmg < 5 {
            assert_eq!(g.obj(giant).damage, dmg as u32);
        } else {
            assert_eq!(g.count_in(1, "Lane A Giant", Zone::Graveyard), 1);
        }
    }
}

// ---- Lost in Space ---------------------------------------------------------

/// "Target artifact or creature's owner puts it on their choice of the top or
/// bottom of their library. Surveil 1." The owner chooses (not the caster).
#[test]
fn lost_in_space_the_owner_chooses_top_or_bottom_cr701_25a() {
    for top in [true, false] {
        let mut g = scen()
            .battlefield(0, &["Island"; 4])
            .battlefield(1, &["Lane A Giant", "Lane A Totem"])
            .hand(0, &["Lost in Space"])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, "Lost in Space", Zone::Hand);
        let giant = bf(&g, 1, "Lane A Giant");
        cast(&mut g, c);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Totem"))), "an artifact or creature");
        g.choose(g.tgt(giant));
        pay(&mut g);
        g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
        assert_eq!(g.pending_decision().unwrap().player, 1, "the owner chooses");
        g.choose(if top { Choice::Yes } else { Choice::Pass });
        settle(&mut g);
        let lib = &g.player(1).library;
        let at = lib.iter().position(|&x| g.name(x) == "Lane A Giant").expect("in the library");
        // (The library's last card is its top.)
        assert_eq!(at, if top { lib.len() - 1 } else { 0 }, "top {top}");
    }
}

// ---- Meltstrider Eulogist --------------------------------------------------

/// "Whenever a creature you control with a +1/+1 counter on it dies, draw a
/// card." (Seen as it last existed, CR 603.10a.)
#[test]
fn meltstrider_eulogist_draws_when_a_countered_creature_dies_cr603_10a() {
    let mut g = scen()
        .battlefield(0, &["Meltstrider Eulogist", "Lane A Bear", "Lane A Robot", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Lane A Giant"])
        .counters(0, "Lane A Bear", Counter::P1P1, 1)
        .counters(1, "Lane A Giant", Counter::P1P1, 1)
        .hand(0, &["Lane A Bolt", "Lane A Bolt", "Lane A Bolt"])
        .at(Step::Main1, 0)
        .start();
    let hand = g.player(0).hand.len();
    for (p, n, draws) in [(0, "Lane A Robot", 0), (1, "Lane A Giant", 0), (0, "Lane A Bear", 1)] {
        let bolt = g.card(0, "Lane A Bolt", Zone::Hand);
        let t = bf(&g, p, n);
        cast(&mut g, bolt);
        target(&mut g, t);
        pay(&mut g);
        settle(&mut g);
        assert_eq!(g.count_in(p, n, Zone::Graveyard), 1);
        let used = 3 - g.count_in(0, "Lane A Bolt", Zone::Hand);
        assert_eq!(g.player(0).hand.len() + used, hand + draws, "{n}");
    }
}

// ---- Radiant Strike, Shattered Wings ---------------------------------------

/// "Destroy target artifact or tapped creature. You gain 3 life."
#[test]
fn radiant_strike_destroys_an_artifact_or_a_tapped_creature_cr115_1() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 4])
        .battlefield(1, &["Lane A Giant", "Lane A Bear", "Lane A Trinket"])
        .hand(0, &["Radiant Strike"])
        .at(Step::Main1, 1)
        .start();
    // P1 attacks with the Giant (tapped), keeps the Bear home.
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bf(&g, 1, "Lane A Giant")));
    g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    g.choose(Choice::Pass);
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let c = g.card(0, "Radiant Strike", Zone::Hand);
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bf(&g, 1, "Lane A Trinket"))));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Bear"))), "untapped");
    let giant = bf(&g, 1, "Lane A Giant");
    g.choose(g.tgt(giant));
    pay(&mut g);
    let life = g.life(0);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Lane A Giant", Zone::Graveyard), 1);
    assert_eq!(g.life(0), life + 3);
}

/// "Destroy target artifact, enchantment, or creature with flying. Surveil
/// 1."
#[test]
fn shattered_wings_targets_artifacts_enchantments_and_flyers_cr115_1() {
    let mut g = scen()
        .battlefield(0, &["Forest"; 3])
        .battlefield(1, &["Lane A Giant", "Lane A Trinket", "Lane A Totem", "Wurmwall Sweeper"])
        .counters(1, "Wurmwall Sweeper", charge(), 4)
        .hand(0, &["Shattered Wings"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Shattered Wings", Zone::Hand);
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    for n in ["Lane A Trinket", "Lane A Totem", "Wurmwall Sweeper"] {
        assert!(g.has_option(g.tgt(bf(&g, 1, n))), "{n}");
    }
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Giant"))), "no flying");
    g.choose(g.tgt(bf(&g, 1, "Lane A Totem")));
    pay(&mut g);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::PutInGraveyard)).then_some(Choice::Pass));
    assert_eq!(g.count_in(1, "Lane A Totem", Zone::Graveyard), 1);
}

// ---- Ruinous Rampage, Scour for Scrap ----------------------------------------

/// "Choose one — Ruinous Rampage deals 3 damage to each opponent. • Exile all
/// artifacts with mana value 3 or less."
#[test]
fn ruinous_rampage_exiles_small_artifacts_cr700_2() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Lane A Robot"])
        .battlefield(1, &["Lane A Trinket", "Extinguisher Battleship", "Lane A Giant"])
        .hand(0, &["Ruinous Rampage", "Ruinous Rampage"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Ruinous Rampage", Zone::Hand);
    cast(&mut g, c);
    g.choose(Choice::Number(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Lane A Robot", Zone::Exile), 1);
    assert_eq!(g.count_in(1, "Lane A Trinket", Zone::Exile), 1);
    assert_eq!(g.count_in(1, "Extinguisher Battleship", Zone::Battlefield), 1, "mana value 8");
    assert_eq!(g.count_in(1, "Lane A Giant", Zone::Battlefield), 1);
    let _ = Target::Player(1);
}

/// "Choose one or both — Search your library for an artifact card, reveal
/// it, put it into your hand, then shuffle. • Return target artifact card
/// from your graveyard to your hand."
#[test]
fn scour_for_scrap_searches_and_returns_artifact_cards_cr700_2() {
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .graveyard(0, &["Lane A Trinket", "Lane A Bear"])
        .library(0, &["Lane A Robot"])
        .hand(0, &["Scour for Scrap"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Scour for Scrap", Zone::Hand);
    cast(&mut g, c);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(0));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    if g.kind() == Some(DecisionKind::ChooseMode) {
        g.choose(Choice::Number(1));
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(g.card(0, "Lane A Bear", Zone::Graveyard))), "an artifact card");
    g.choose(g.tgt(g.card(0, "Lane A Trinket", Zone::Graveyard)));
    pay(&mut g);
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| {
            g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice
        })
    });
    assert_eq!(g.count_in(0, "Lane A Trinket", Zone::Hand), 1);
    assert_eq!(g.count_in(0, "Lane A Robot", Zone::Hand), 1, "{}", log(&g));
}

// ---- Synthesizer Labship ---------------------------------------------------

/// "2+ | At the beginning of combat on your turn, up to one other target
/// artifact you control becomes an artifact creature with base power and
/// toughness 2/2 and gains flying until end of turn." A stationed Spacecraft
/// takes the later base P/T (CR 613.7, 613.4b; "it will use whatever base
/// power and toughness was set by the effect that made it a creature").
#[test]
fn synthesizer_labship_animates_another_artifact_cr205_1b() {
    let mut g = scen()
        .battlefield(0, &["Synthesizer Labship", "Lane A Trinket", "Atmospheric Greenhouse"])
        .battlefield(1, &["Lane A Robot"])
        .counters(0, "Synthesizer Labship", charge(), 2)
        .counters(0, "Atmospheric Greenhouse", charge(), 8)
        .at(Step::Main1, 0)
        .start();
    let (lab, trinket, gh) = (bf(&g, 0, "Synthesizer Labship"), bf(&g, 0, "Lane A Trinket"), bf(&g, 0, "Atmospheric Greenhouse"));
    assert_eq!((g.power(gh), g.toughness(gh)), (5, 4));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(lab)), "other");
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Robot"))), "you control");
    assert!(g.has_option(Choice::Pass), "up to one");
    g.choose(g.tgt(trinket));
    settle(&mut g);
    assert!(creature(&g, trinket) && g.types_of(trinket) & types::ARTIFACT != 0);
    assert_eq!((g.power(trinket), g.toughness(trinket)), (2, 2));
    assert!(g.has_kw(trinket, Kw::Flying));
    next_main(&mut g, 1);
    assert!(!creature(&g, trinket), "until end of turn");
    // Next turn, the stationed Greenhouse: base 2/2 now.
    next_main(&mut g, 0);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(gh));
    settle(&mut g);
    assert_eq!((g.power(gh), g.toughness(gh)), (2, 2));
    assert!(g.has_kw(gh, Kw::Trample), "its own abilities stay");
}

/// Below 2 charge counters the Labship has no triggered ability (CR 722.2a).
#[test]
fn synthesizer_labship_needs_two_charge_counters_cr722_2a() {
    let mut g = scen()
        .battlefield(0, &["Synthesizer Labship", "Lane A Trinket"])
        .counters(0, "Synthesizer Labship", charge(), 1)
        .at(Step::Main1, 0)
        .start();
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 || g.kind() == Some(DecisionKind::Target));
    assert_eq!(g.turn().step, Step::Main2);
}

// ---- Virulent Silencer -----------------------------------------------------

/// "Whenever a nontoken artifact creature you control deals combat damage to
/// a player, that player gets two poison counters." (CR 122.1f.)
#[test]
fn virulent_silencer_poisons_for_artifact_creatures_cr122_1f() {
    let mut g = scen()
        .battlefield(0, &["Virulent Silencer", "Lane A Robot", "Lane A Bear"])
        .at(Step::Main1, 0)
        .start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for n in ["Virulent Silencer", "Lane A Robot", "Lane A Bear"] {
        g.choose(Choice::Card(bf(&g, 0, n)));
        g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    }
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.player(1).poison, 4, "two artifact creatures, not the Bear\n{}", log(&g));
}

// ---- Warmaker Gunship ------------------------------------------------------

/// "When this Spacecraft enters, it deals damage equal to the number of
/// artifacts you control to target creature an opponent controls."
#[test]
fn warmaker_gunship_deals_damage_by_your_artifacts_cr120_3() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 3])
        .battlefield(0, &["Lane A Trinket", "Lane A Bear"])
        .battlefield(1, &["Lane A Giant", "Lane A Trinket"])
        .hand(0, &["Warmaker Gunship"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Warmaker Gunship", Zone::Hand);
    cast(&mut g, c);
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bf(&g, 0, "Lane A Bear"))), "an opponent's creature");
    let giant = bf(&g, 1, "Lane A Giant");
    g.choose(g.tgt(giant));
    settle(&mut g);
    // The Trinket and the Gunship itself.
    assert_eq!(g.obj(giant).damage, 2);
    let ship = bf(&g, 0, "Warmaker Gunship");
    assert!(!creature(&g, ship));
}

// ---- searching for Equipment ------------------------------------------------

/// "Search your library for an Equipment card, put it onto the battlefield,
/// then shuffle" (Honored Knight-Captain's ability, on a custom card: the
/// Knight-Captain's IR is defective, docs/ir_defects.md 23). A search for a
/// card with a stated quality may fail (CR 701.19b).
#[test]
fn search_for_an_equipment_to_the_battlefield_cr701_19b() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Lane A Quartermaster"])
        .library(0, &["Lane A Trinket", "Meltstrider's Gear", "Lane A Bear"])
        .at(Step::Main1, 0)
        .start();
    let q = bf(&g, 0, "Lane A Quartermaster");
    g.choose(act(q, 0));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    let opts = g.options_desc();
    assert_eq!(opts.len(), 2, "the Gear or nothing: {opts:?}");
    assert!(g.has_option(Choice::Pass));
    let o = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
    g.choose(o);
    settle(&mut g);
    let gear = bf(&g, 0, "Meltstrider's Gear");
    assert_eq!(g.obj(gear).attached, None, "it enters unattached (CR 301.5b)");
}
