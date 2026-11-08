//! Card tests of the Edge of Eternities base cards new to mtg-zero
//! (`SUPPORTED_EOE_BASE` less the basics and the 20 cards the cube shares,
//! which its own tests cover; ENGINE.md 99.3, the base audit), each against
//! its Oracle text: bodies and keywords, warp (CR 702.185), enters and dies
//! triggers, landfall, "your second spell each turn", artifacts entering.

mod eoe_b;

use engine::db::{types, Counter, Kw};
use engine::mana::ManaCost;
use engine::{Choice, DecisionKind, Game, Step, Target, Zone};
use eoe_b::*;

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn tokens_named(g: &Game, p: u8, name: &str) -> Vec<engine::CardIdx> {
    g.permanents_of(p).filter(|&c| g.name(c) == name && g.def_of(c).token).collect()
}

// ---- bodies and keywords -----------------------------------------------------

/// Each card's printed power, toughness, card types and keywords, on the
/// battlefield (the Oracle text's; the IR was checked against it field by
/// field, ENGINE.md 99.3).
#[test]
fn eoe_base_bodies_and_keywords_match_the_oracle_text() {
    use Kw::*;
    let rows: &[(&str, (i32, i32), u16, &[Kw])] = &[
        ("Hullcarver", (1, 1), types::ARTIFACT | types::CREATURE, &[Deathtouch]),
        ("Monoist Sentry", (4, 1), types::ARTIFACT | types::CREATURE, &[Defender]),
        ("Bygone Colossus", (9, 9), types::ARTIFACT | types::CREATURE, &[]),
        ("Red Tiger Mechan", (3, 3), types::ARTIFACT | types::CREATURE, &[Haste]),
        ("Illvoi Galeblade", (1, 1), types::CREATURE, &[Flash, Flying]),
        ("Starbreach Whale", (3, 5), types::CREATURE, &[Flying]),
        ("Syr Vondam, the Lucent", (4, 4), types::CREATURE, &[Deathtouch, Lifelink]),
        ("Nebula Dragon", (4, 4), types::CREATURE, &[Flying]),
        ("Exalted Sunborn", (4, 5), types::CREATURE, &[Flying, Lifelink]),
        ("Dual-Sun Adepts", (2, 2), types::CREATURE, &[DoubleStrike]),
        ("Blooming Stinger", (2, 2), types::CREATURE, &[Deathtouch]),
        ("Icecave Crasher", (4, 4), types::CREATURE, &[Trample]),
        ("Remnant Elemental", (0, 4), types::CREATURE, &[Reach]),
        ("Mm'menon, Uthros Exile", (1, 3), types::CREATURE, &[Flying]),
        ("Cloudsculpt Technician", (1, 4), types::CREATURE, &[Flying]),
        ("Gravpack Monoist", (2, 1), types::CREATURE, &[Flying]),
        ("Weftstalker Ardent", (2, 3), types::CREATURE, &[]),
        ("Zookeeper Mechan", (1, 3), types::ARTIFACT | types::CREATURE, &[]),
        ("Nanoform Sentinel", (3, 2), types::ARTIFACT | types::CREATURE, &[]),
        ("Oreplate Pangolin", (2, 2), types::ARTIFACT | types::CREATURE, &[]),
        ("Kavaron Turbodrone", (2, 3), types::ARTIFACT | types::CREATURE, &[]),
        ("Slagdrill Scrapper", (1, 2), types::ARTIFACT | types::CREATURE, &[]),
        ("Mechan Assembler", (4, 4), types::ARTIFACT | types::CREATURE, &[]),
        ("Mechan Navigator", (2, 1), types::ARTIFACT | types::CREATURE, &[]),
    ];
    for &(name, pt, ty, kws) in rows {
        let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
        let c = bf(&g, 0, name);
        assert_eq!(g.pt(c), pt, "{name}");
        assert_eq!(g.def_of(c).types & (types::ARTIFACT | types::CREATURE), ty, "{name}");
        for &k in kws {
            assert!(g.has_kw(c, k), "{name}: {k:?}");
        }
    }
    let g = scen().battlefield(0, &["Syr Vondam, the Lucent", "Mm'menon, Uthros Exile"]).at(Step::Main1, 0).start();
    assert!(g.is_legendary(bf(&g, 0, "Syr Vondam, the Lucent")) && g.is_legendary(bf(&g, 0, "Mm'menon, Uthros Exile")));
    let g = scen().battlefield(0, &["Nutrient Block", "Dubious Delicacy"]).at(Step::Main1, 0).start();
    assert!(g.has_kw(bf(&g, 0, "Nutrient Block"), Indestructible));
    assert!(subtype(&g, bf(&g, 0, "Nutrient Block"), "Food") && subtype(&g, bf(&g, 0, "Dubious Delicacy"), "Food"));
    assert!(g.def_of(bf(&g, 0, "Dubious Delicacy")).keywords.has(Flash));
}

/// Monoist Sentry: "Defender" (CR 702.3b): it can't attack.
#[test]
fn monoist_sentry_cant_attack() {
    let mut g = scen().battlefield(0, &["Monoist Sentry", "Hullcarver"]).at(Step::Main1, 0).start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(!g.has_option(Choice::Card(bf(&g, 0, "Monoist Sentry"))));
    assert!(g.has_option(Choice::Card(bf(&g, 0, "Hullcarver"))));
}

// ---- warp (CR 702.185) -------------------------------------------------------

/// The set's warp cards: an alternative cost from hand of the Oracle text's
/// mana, exiled at the next end step.
#[test]
fn cr_702_185a_warp_costs_match_the_oracle_text() {
    let rows: &[(&str, &str)] = &[
        ("Bygone Colossus", "{3}"),
        ("Eusocial Engineering", "{1}{G}"),
        ("Exalted Sunborn", "{1}{W}"),
        ("Germinating Wurm", "{1}{G}"),
        ("Red Tiger Mechan", "{1}{R}"),
        ("Sinister Cryologist", "{U}"),
        ("Starbreach Whale", "{1}{U}"),
        ("Susurian Voidborn", "{B}"),
        ("Weftblade Enhancer", "{2}{W}"),
        ("Weftstalker Ardent", "{R}"),
    ];
    for &(name, warp) in rows {
        let d = db().def(def(name));
        let w: Vec<_> = d.alt_costs.iter().filter(|a| a.warp).collect();
        assert_eq!(w.len(), 1, "{name}");
        assert_eq!(w[0].mana, cost(warp), "{name}");
    }
}

/// Bygone Colossus warped in for {3}: a 9/9 until the end step, then in
/// exile, castable on a later turn.
#[test]
fn cr_702_185a_bygone_colossus_warps_in_for_three_and_is_exiled_at_the_end_step() {
    let mut g = scen().battlefield(0, &["Island", "Island", "Island"]).hand(0, &["Bygone Colossus"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Bygone Colossus", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |g| pick(g, &[Choice::Number(1)]));
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    assert_eq!(g.pt(c), (9, 9));
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 1 && g.turn().step == Step::Upkeep);
    assert_eq!(g.obj(c).zone, Zone::Exile);
}

// ---- enters and dies triggers ------------------------------------------------

/// Exalted Sunborn: "If one or more tokens would be created under your
/// control, twice that many of those tokens are created instead" (CR
/// 614.1a); an opponent's Sunborn doesn't apply. (Knight Luminary and
/// Cosmogrand Zenith, whose white Human Soldiers the IR now carries: ENGINE.md
/// 99.9, `cards_eoe_irfix*.rs`.)
#[test]
fn exalted_sunborn_doubles_the_tokens_you_create() {
    let mut g = scen().battlefield(0, &["Exalted Sunborn", "Eusocial Engineering"]).battlefield(1, &["Exalted Sunborn"]).hand(0, &["Forest"]).at(Step::Main1, 0).start();
    play_land(&mut g, 0, "Forest", |_| None);
    assert_eq!(tokens_named(&g, 0, "Robot").len(), 2, "twice that many");
}

/// Germinating Wurm: "When this creature enters, you gain 2 life." Nebula
/// Dragon: "When this creature enters, it deals 3 damage to any target."
#[test]
fn germinating_wurm_gains_two_and_nebula_dragon_deals_three() {
    let mut g = scen().battlefield(0, &["Forest"; 5]).hand(0, &["Germinating Wurm"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Germinating Wurm", |g| pick(g, &[Choice::Number(0)]));
    assert_eq!(g.life(0), 22);
    let mut g = scen().battlefield(0, &["Mountain"; 7]).hand(0, &["Nebula Dragon"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Nebula Dragon", |g| pick(g, &[Choice::Target(Target::Player(1))]));
    assert_eq!(g.life(1), 17);
}

/// Sinister Cryologist: "When this creature enters, target creature an
/// opponent controls gets -3/-0 until end of turn."
#[test]
fn sinister_cryologist_shrinks_an_opponents_creature() {
    let mut g = scen().battlefield(0, &["Island"; 3]).battlefield(0, &["Hullcarver"]).battlefield(1, &["Test Trampler"]).hand(0, &["Sinister Cryologist"]).at(Step::Main1, 0).start();
    let (mine, theirs) = (bf(&g, 0, "Hullcarver"), bf(&g, 1, "Test Trampler"));
    let c = g.card(0, "Sinister Cryologist", Zone::Hand);
    start_with(&mut g, Choice::Cast(c), |g| pick(g, &[Choice::Number(0)]));
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(g.tgt(bf(g, 0, "Hullcarver"))), "an opponent's creature");
        }
        pick(g, &[g.tgt(bf(g, 1, "Test Trampler"))])
    });
    assert_eq!(g.pt(theirs), (1, 4));
    assert_eq!(g.pt(mine), (1, 1));
}

/// Blooming Stinger: "When this creature enters, another target creature you
/// control gains deathtouch until end of turn."
#[test]
fn blooming_stinger_gives_another_creature_you_control_deathtouch() {
    let mut g = scen().battlefield(0, &["Forest"; 2]).battlefield(0, &["Test Bear"]).battlefield(1, &["Test Trampler"]).hand(0, &["Blooming Stinger"]).at(Step::Main1, 0).start();
    let bear = bf(&g, 0, "Test Bear");
    cast_and_resolve(&mut g, 0, "Blooming Stinger", |g| {
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(g.tgt(bf(g, 1, "Test Trampler"))), "you control");
            assert!(!g.has_option(g.tgt(bf(g, 0, "Blooming Stinger"))), "another");
        }
        pick(g, &[g.tgt(bf(g, 0, "Test Bear"))])
    });
    assert!(g.has_kw(bear, Kw::Deathtouch));
}

/// Weftblade Enhancer: "When this creature enters, put a +1/+1 counter on
/// each of up to two target creatures."
#[test]
fn weftblade_enhancer_counters_up_to_two_target_creatures() {
    let mut g = scen().battlefield(0, &["Plains"; 6]).battlefield(0, &["Test Bear"]).battlefield(1, &["Test Trampler"]).hand(0, &["Weftblade Enhancer"]).at(Step::Main1, 0).start();
    let (bear, tr) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Trampler"));
    let (tb, tt) = (g.tgt(bear), g.tgt(tr));
    cast_and_resolve(&mut g, 0, "Weftblade Enhancer", move |g| pick(g, &[Choice::Number(0), tb, tt, Choice::Pass]));
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    assert_eq!(g.counters_on(tr, Counter::P1P1), 1);
}

/// Thawbringer: "When this creature enters or dies, surveil 1." Starbreach
/// Whale: "When this creature enters, surveil 2" (CR 701.25a: look, then any
/// number into the graveyard).
#[test]
fn cr_701_25a_thawbringer_and_starbreach_whale_surveil() {
    let mut g = scen().battlefield(0, &["Forest"; 3]).hand(0, &["Thawbringer"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Thawbringer", |g| {
        // Into the graveyard (any choice that isn't "top").
        let d = g.pending_decision()?;
        (d.kind != DecisionKind::Priority).then(|| d.options.last().unwrap().choice)
    });
    let gy = g.player(0).graveyard.len();
    assert!(gy <= 1);
    let mut g = scen().battlefield(0, &["Island"; 5]).hand(0, &["Starbreach Whale"]).at(Step::Main1, 0).start();
    let lib = g.player(0).library.len();
    let seen = std::cell::Cell::new(0);
    cast_and_resolve(&mut g, 0, "Starbreach Whale", |g| {
        if g.kind() == Some(DecisionKind::PutInGraveyard) {
            seen.set(seen.get() + 1);
        }
        pick(g, &[Choice::Number(0)])
    });
    assert_eq!(g.player(0).library.len() + g.player(0).graveyard.len(), lib, "two looked at, none drawn");
}

/// Gravpack Monoist: "When this creature dies, create a tapped 2/2 colorless
/// Robot artifact creature token."
#[test]
fn gravpack_monoist_leaves_a_tapped_robot() {
    let mut g = scen().battlefield(0, &["Gravpack Monoist"]).battlefield(1, &["Mountain"; 3]).hand(1, &["Bombard"]).at(Step::Main1, 1).start();
    let m = bf(&g, 0, "Gravpack Monoist");
    let tm = g.tgt(m);
    cast_and_resolve(&mut g, 1, "Bombard", move |g| pick(g, &[tm]));
    let r = tokens_named(&g, 0, "Robot");
    assert_eq!(r.len(), 1);
    assert!(g.obj(r[0]).tapped && g.def_of(r[0]).is(types::ARTIFACT | types::CREATURE));
    assert_eq!((g.pt(r[0]), g.colors_of(r[0])), ((2, 2), 0));
}

/// Susurian Voidborn: "Whenever this creature or another creature or
/// artifact you control dies, target opponent loses 1 life and you gain 1
/// life."
#[test]
fn susurian_voidborn_drains_when_your_creatures_or_artifacts_die() {
    let mut g = scen().battlefield(0, &["Susurian Voidborn", "Hullcarver", "Nutrient Block"]).battlefield(1, &["Mountain"; 3]).hand(1, &["Bombard"]).at(Step::Main1, 1).start();
    let h = bf(&g, 0, "Hullcarver");
    let th = g.tgt(h);
    cast_and_resolve(&mut g, 1, "Bombard", move |g| pick(g, &[th, Choice::Target(Target::Player(1))]));
    assert_eq!((g.life(0), g.life(1)), (21, 19));
}

/// Nutrient Block: "{2}, {T}, Sacrifice this artifact: You gain 3 life. /
/// When this artifact is put into a graveyard from the battlefield, draw a
/// card."
#[test]
fn nutrient_block_gains_three_and_draws_when_it_goes_to_the_graveyard() {
    let mut g = scen().battlefield(0, &["Nutrient Block", "Island", "Island"]).at(Step::Main1, 0).start();
    let nb = bf(&g, 0, "Nutrient Block");
    let hand = g.player(0).hand.len();
    start_with(&mut g, act(nb, 0), |_| None);
    settle(&mut g);
    assert_eq!(g.life(0), 23);
    assert_eq!(g.player(0).hand.len(), hand + 1);
}

// ---- landfall ----------------------------------------------------------------

/// Eumidian Terrabotanist (gain 1), Eusocial Engineering (a 2/2 Robot),
/// Icecave Crasher (+1/+0), Remnant Elemental (+2/+0): "Landfall — Whenever a
/// land you control enters, ...". An opponent's land doesn't count.
#[test]
fn landfall_triggers_of_the_base_cards() {
    let mut g = scen()
        .battlefield(0, &["Eumidian Terrabotanist", "Eusocial Engineering", "Icecave Crasher", "Remnant Elemental"])
        .hand(0, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let (ic, re) = (bf(&g, 0, "Icecave Crasher"), bf(&g, 0, "Remnant Elemental"));
    play_land(&mut g, 0, "Forest", |_| None);
    assert_eq!(g.life(0), 21);
    assert_eq!(tokens_named(&g, 0, "Robot").len(), 1);
    assert_eq!(g.pt(ic), (5, 4));
    assert_eq!(g.pt(re), (2, 4));
    let mut g = scen().battlefield(0, &["Eumidian Terrabotanist"]).hand(1, &["Forest"]).at(Step::Main1, 1).start();
    play_land(&mut g, 1, "Forest", |_| None);
    assert_eq!(g.life(0), 20, "a land you control");
}

// ---- your second spell each turn --------------------------------------------

/// Illvoi Operative (+1/+1 counter), Station Monitor (a 1/1 Drone with
/// flying that can block only fliers), Sunstar Lightsmith (counter and
/// draw): "Whenever you cast your second spell each turn" — the second
/// only, not the first or third.
#[test]
fn second_spell_each_turn_triggers_once() {
    let mut g2 = scen()
        .battlefield(0, &["Illvoi Operative", "Station Monitor", "Sunstar Lightsmith"])
        .battlefield(0, &["Plains"; 3])
        .hand(0, &["Honor", "Honor", "Honor"])
        .at(Step::Main1, 0)
        .start();
    let op2 = bf(&g2, 0, "Illvoi Operative");
    let ls2 = bf(&g2, 0, "Sunstar Lightsmith");
    let t2 = g2.tgt(ls2);
    cast_and_resolve(&mut g2, 0, "Honor", move |g| pick(g, &[t2]));
    assert_eq!(g2.counters_on(op2, Counter::P1P1), 0, "first spell: nothing");
    let hand = g2.player(0).hand.len();
    cast_and_resolve(&mut g2, 0, "Honor", move |g| pick(g, &[t2]));
    assert_eq!(g2.counters_on(op2, Counter::P1P1), 1, "Illvoi Operative");
    assert_eq!(g2.counters_on(ls2, Counter::P1P1), 3, "two Honors and Sunstar Lightsmith's");
    assert_eq!(tokens_named(&g2, 0, "Drone").len(), 1, "Station Monitor");
    assert_eq!(g2.player(0).hand.len(), hand - 1 + 1 + 1, "Honor's draw and the Lightsmith's");
    let drone = tokens_named(&g2, 0, "Drone")[0];
    assert!(g2.has_kw(drone, Kw::Flying) && g2.def_of(drone).is(types::ARTIFACT | types::CREATURE));
    cast_and_resolve(&mut g2, 0, "Honor", move |g| pick(g, &[t2]));
    assert_eq!(g2.counters_on(op2, Counter::P1P1), 1, "third spell: nothing");
}

// ---- artifacts entering ------------------------------------------------------

/// Mechan Assembler: "Whenever another artifact you control enters, create a
/// 2/2 colorless Robot artifact creature token. This ability triggers only
/// once each turn."
#[test]
fn mechan_assembler_makes_one_robot_per_turn() {
    let mut g = scen().battlefield(0, &["Mechan Assembler", "Island", "Island", "Mountain", "Mountain"]).hand(0, &["Nutrient Block", "Nutrient Block"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Nutrient Block", |_| None);
    assert_eq!(tokens_named(&g, 0, "Robot").len(), 1, "the Robot is an artifact too, but once each turn");
    cast_and_resolve(&mut g, 0, "Nutrient Block", |_| None);
    assert_eq!(tokens_named(&g, 0, "Robot").len(), 1);
}

/// Oreplate Pangolin: "Whenever another artifact you control enters, you may
/// pay {1}. If you do, put a +1/+1 counter on this creature."
#[test]
fn oreplate_pangolin_may_pay_one_for_a_counter() {
    for (pay, n) in [(true, 1), (false, 0)] {
        let mut g = scen().battlefield(0, &["Oreplate Pangolin", "Island", "Island"]).hand(0, &["Nutrient Block"]).at(Step::Main1, 0).start();
        let p = bf(&g, 0, "Oreplate Pangolin");
        cast_and_resolve(&mut g, 0, "Nutrient Block", move |g| pick(g, &[if pay { Choice::Yes } else { Choice::Pass }]));
        assert_eq!(g.counters_on(p, Counter::P1P1), n, "paid: {pay}");
    }
}

/// Weapons Manufacturing: "Whenever a nontoken artifact you control enters,
/// create a colorless artifact token named Munitions with 'When this token
/// leaves the battlefield, it deals 2 damage to any target.'"
#[test]
fn weapons_manufacturing_munitions_deal_two_when_they_leave() {
    let mut g = scen().battlefield(0, &["Weapons Manufacturing", "Mechan Assembler", "Island"]).hand(0, &["Nutrient Block"]).battlefield(1, &["Mountain"; 3]).hand(1, &["Bombard"]).at(Step::Main1, 0).start();
    cast_and_resolve(&mut g, 0, "Nutrient Block", |_| None);
    let m = tokens_named(&g, 0, "Munitions");
    assert_eq!(m.len(), 1, "the Robot token (an artifact) makes none");
    assert!(g.def_of(m[0]).is(types::ARTIFACT) && !g.def_of(m[0]).is(types::CREATURE));
    assert_eq!(g.colors_of(m[0]), 0);
}

/// Weftstalker Ardent: "Whenever another creature or artifact you control
/// enters, this creature deals 1 damage to each opponent." Mm'menon: "Whenever
/// an artifact you control enters, put a +1/+1 counter on target creature."
#[test]
fn weftstalker_ardent_pings_and_mmmenon_counters_on_artifacts_entering() {
    let mut g = scen().battlefield(0, &["Weftstalker Ardent", "Mm'menon, Uthros Exile", "Island"]).hand(0, &["Nutrient Block"]).at(Step::Main1, 0).start();
    let w = bf(&g, 0, "Weftstalker Ardent");
    let tw = g.tgt(w);
    cast_and_resolve(&mut g, 0, "Nutrient Block", move |g| pick(g, &[tw]));
    assert_eq!(g.life(1), 19);
    assert_eq!(g.counters_on(w, Counter::P1P1), 1);
}

// ---- becomes tapped ----------------------------------------------------------

/// Mechan Navigator ("draw a card, then discard a card"), Starfighter Pilot
/// ("surveil 1"), Nanoform Sentinel ("untap another target permanent. This
/// ability triggers only once each turn"): "Whenever this creature becomes
/// tapped" — attacking taps them (CR 508.1f).
#[test]
fn becomes_tapped_triggers_on_attack() {
    let mut g = scen().battlefield(0, &["Mechan Navigator", "Starfighter Pilot", "Nanoform Sentinel", "Island"]).at(Step::Main1, 0).start();
    let island = bf(&g, 0, "Island");
    g.set_tapped(island, true);
    let (nav, pil, nan) = (bf(&g, 0, "Mechan Navigator"), bf(&g, 0, "Starfighter Pilot"), bf(&g, 0, "Nanoform Sentinel"));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    let hand = g.player(0).hand.len();
    let lib = g.player(0).library.len();
    for c in [nav, pil, nan] {
        g.choose(Choice::Card(c));
    }
    g.choose(Choice::Pass);
    let ti = g.tgt(island);
    resolve_with(&mut g, move |g| pick(g, &[ti]));
    assert_eq!(g.player(0).hand.len(), hand, "drew one, discarded one");
    assert!(g.player(0).library.len() < lib);
    assert!(!g.obj(island).tapped, "Nanoform Sentinel untapped the Island");
}

// ---- static abilities --------------------------------------------------------

/// Cloudsculpt Technician (+1/+0) and Gravblade Heavy (+1/+0 and deathtouch):
/// "As long as you control an artifact".
#[test]
fn cloudsculpt_technician_and_gravblade_heavy_with_an_artifact() {
    let g = scen().battlefield(0, &["Cloudsculpt Technician", "Gravblade Heavy"]).at(Step::Main1, 0).start();
    let (c, h) = (bf(&g, 0, "Cloudsculpt Technician"), bf(&g, 0, "Gravblade Heavy"));
    assert_eq!((g.pt(c), g.pt(h)), ((1, 4), (3, 4)));
    assert!(!g.has_kw(h, Kw::Deathtouch));
    let g = scen().battlefield(0, &["Cloudsculpt Technician", "Gravblade Heavy", "Nutrient Block"]).at(Step::Main1, 0).start();
    let (c, h) = (bf(&g, 0, "Cloudsculpt Technician"), bf(&g, 0, "Gravblade Heavy"));
    assert_eq!((g.pt(c), g.pt(h)), ((2, 4), (4, 4)));
    assert!(g.has_kw(h, Kw::Deathtouch));
    let g = scen().battlefield(0, &["Gravblade Heavy"]).battlefield(1, &["Nutrient Block"]).at(Step::Main1, 0).start();
    assert_eq!(g.pt(bf(&g, 0, "Gravblade Heavy")), (3, 4), "you control");
}

/// Thrumming Hivepool: "Affinity for Slivers / Slivers you control have
/// double strike and haste. / At the beginning of your upkeep, create two 1/1
/// colorless Sliver creature tokens."
#[test]
fn thrumming_hivepool_makes_slivers_with_double_strike_and_haste() {
    let mut g = scen().battlefield(0, &["Thrumming Hivepool"]).at(Step::End, 1).start();
    quiet_until(&mut g, main1_of(0));
    let s = tokens_named(&g, 0, "Sliver");
    assert_eq!(s.len(), 2);
    assert!(g.has_kw(s[0], Kw::DoubleStrike) && g.has_kw(s[0], Kw::Haste));
    assert_eq!((g.pt(s[0]), g.colors_of(s[0])), ((1, 1), 0));
    // Affinity: two Slivers, {4} with four lands.
    let mut g = scen().battlefield(0, &["Thrumming Hivepool", "Island", "Island", "Island", "Island"]).hand(0, &["Thrumming Hivepool"]).at(Step::End, 1).start();
    quiet_until(&mut g, main1_of(0));
    let h = g.card(0, "Thrumming Hivepool", Zone::Hand);
    assert!(g.has_option(Choice::Cast(h)), "{{6}} less two");
}

// ---- the cards the set shares with the cube ----------------------------------

/// The 25 cards of the EoE table that the cube also has (20 nonbasic and the
/// basics): the same IR printing in both exports, lowered to the same
/// definition (fresh interners: lowering reads nothing but the printing), and
/// supported in both tables with the same characteristics. Their behaviour
/// is covered by the cube's card tests (ENGINE.md 99.3).
#[test]
fn the_cards_the_set_shares_with_the_cube_lower_the_same() {
    let cube = engine::db::load_cube_document().cards;
    let eoe = engine::db::load_eoe_document().cards;
    let shared: Vec<_> = eoe.iter().filter_map(|e| cube.iter().find(|c| c.name == e.name).map(|c| (c, e))).collect();
    assert_eq!(shared.len(), 25);
    let (cdb, edb) = (engine::db::CardDb::cube(), engine::db::CardDb::eoe());
    for (c, e) in shared {
        assert_eq!(c.printing, e.printing, "{}: the same IR", c.name);
        let lc = engine::lower::lower_card(c, &mut Vec::new(), &mut Vec::new());
        let le = engine::lower::lower_card(e, &mut Vec::new(), &mut Vec::new());
        assert!(lc.is_ok(), "{}", c.name);
        assert_eq!(lc, le, "{}: the same definition", c.name);
        let (dc, de) = (cdb.def(cdb.id(&c.name).unwrap()), edb.def(edb.id(&e.name).unwrap()));
        assert_eq!((dc.types, dc.supertypes, dc.colors, &dc.cost, dc.power, dc.toughness), (de.types, de.supertypes, de.colors, &de.cost, de.power, de.toughness), "{}", c.name);
        assert_eq!((dc.activated.len(), dc.triggers.len(), dc.keywords), (de.activated.len(), de.triggers.len(), de.keywords), "{}", c.name);
    }
}
