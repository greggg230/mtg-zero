//! M7b card tests, mana sources and artifacts: Talismans, Moxen, Gleemox,
//! mana creatures, Basalt Monolith, Expedition Map, Manifold Key,
//! Retrofitter Foundry, Soul-Guide Lantern, Cryogen Relic, Cori-Steel
//! Cutter (CR 605 mana abilities, 302.6 summoning sickness, 502.3 "doesn't
//! untap", 122.1d stun counters, 301.5 equipment, 702.108 prowess).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::mana::ManaCost;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
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

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap()
}

/// Pay with the first source offered until priority.
fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| {
            at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
        });
    }
}

/// Run the stack down; `answer` answers what is asked (None: first option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

/// The colours of "WUBRG" one copy of `name` on the battlefield can pay.
fn colors_of_source(name: &str) -> String {
    let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
    "WUBRG".chars().filter(|c| g.can_pay(0, &cost(&format!("{{{c}}}")))).collect()
}

// ---- Talismans -------------------------------------------------------------

/// "{T}: Add {C}. {T}: Add A or B. This artifact deals 1 damage to you."
/// The two abilities are alternatives (one {T}).
#[test]
fn talismans_tap_for_colorless_or_one_of_two_colors_for_one_damage() {
    let rows: &[(&str, &str)] = &[
        ("Talisman of Conviction", "WR"),
        ("Talisman of Creativity", "UR"),
        ("Talisman of Curiosity", "UG"),
        ("Talisman of Indulgence", "BR"),
        ("Talisman of Progress", "WU"),
    ];
    for &(name, colors) in rows {
        assert_eq!(colors_of_source(name), colors, "{name}");
        let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
        assert!(g.can_pay(0, &cost("{C}")), "{name}: {{T}}: Add {{C}}");
        assert!(!g.can_pay(0, &cost("{2}")), "{name}: one mana per tap");
        let t = bf(&g, 0, name);
        assert_eq!(g.def_of(t).mana_value(), 2, "{name} costs {{2}}");
        // The {C} ability: no damage.
        let mut g = scen().battlefield(0, &[name]).hand(0, &["Test Zero"]).at(Step::Main1, 0).start();
        let t = bf(&g, 0, name);
        let z = g.card(0, "Test Zero", Zone::Hand);
        g.choose(Choice::Cast(z));
        assert!(g.has_option(act(t, 0)) && g.has_option(act(t, 1)), "{name}: either ability pays {{1}}");
        g.choose(act(t, 0));
        g.drive_until(at_priority);
        assert_eq!(g.life(0), 20, "{name}: {{C}} deals no damage");
        // The coloured ability: 1 damage to you.
        let mut g = scen().battlefield(0, &[name]).hand(0, &["Test Zero"]).at(Step::Main1, 0).start();
        let t = bf(&g, 0, name);
        let z = g.card(0, "Test Zero", Zone::Hand);
        g.choose(Choice::Cast(z));
        g.choose(act(t, 1));
        if g.kind() == Some(DecisionKind::ChooseMana) {
            assert_eq!(g.pending_decision().unwrap().options.len(), 2, "{name}: A or B");
            g.choose(g.pending_decision().unwrap().options[1].choice);
        }
        g.drive_until(at_priority);
        assert_eq!(g.life(0), 19, "{name}: the coloured ability deals 1 damage to you");
        assert!(g.obj(t).tapped);
    }
}

// ---- Moxen and Gleemox -----------------------------------------------------

/// "{T}: Add <colour>." A zero-cost artifact: cast for nothing and usable at
/// once (302.6 applies to creatures only).
#[test]
fn moxen_and_gleemox_cost_zero_and_tap_for_their_mana_at_once() {
    let rows: &[(&str, &str)] = &[("Mox Emerald", "G"), ("Mox Ruby", "R"), ("Mox Sapphire", "U"), ("Gleemox", "WUBRG")];
    for &(name, colors) in rows {
        assert_eq!(colors_of_source(name), colors, "{name}");
        let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
        assert!(!g.can_pay(0, &cost("{2}")), "{name}: one mana");
        let mut g = scen().hand(0, &[name, "Test Zero"]).at(Step::Main1, 0).start();
        let m = g.card(0, name, Zone::Hand);
        assert_eq!(g.def_of(m).mana_value(), 0, "{name}: {{0}}");
        assert!(g.is_type(m, engine::db::types::ARTIFACT));
        g.choose(Choice::Cast(m));
        settle(&mut g);
        assert_eq!(g.obj(m).zone, Zone::Battlefield, "{name}");
        let z = g.card(0, "Test Zero", Zone::Hand);
        assert!(g.has_option(Choice::Cast(z)), "{name} pays {{1}} the turn it enters");
    }
}

// ---- mana creatures --------------------------------------------------------

/// Avacyn's Pilgrim "{T}: Add {W}", Elvish Mystic "{T}: Add {G}", Birds of
/// Paradise "Flying; {T}: Add one mana of any color": creatures, so not the
/// turn they come under your control (302.6).
#[test]
fn mana_creatures_make_their_mana_but_not_while_summoning_sick() {
    let rows: &[(&str, &str, (i32, i32), &str)] = &[
        ("Avacyn's Pilgrim", "W", (1, 1), "G"),
        ("Elvish Mystic", "G", (1, 1), "G"),
        ("Birds of Paradise", "WUBRG", (0, 1), "G"),
    ];
    for &(name, colors, pt, cast_with) in rows {
        assert_eq!(colors_of_source(name), colors, "{name}");
        let g = scen().battlefield(0, &[name]).at(Step::Main1, 0).start();
        let c = bf(&g, 0, name);
        assert_eq!(g.pt(c), (pt.0.into(), pt.1.into()), "{name}");
        assert!(g.is_creature(c));
        assert_eq!(g.has_kw(c, Kw::Flying), name == "Birds of Paradise", "{name}: flying");
        let land = if cast_with == "G" { "Forest" } else { "Plains" };
        let mut g = scen().battlefield(0, &[land]).hand(0, &[name]).at(Step::Main1, 0).start();
        let c = g.card(0, name, Zone::Hand);
        g.choose(Choice::Cast(c));
        pay(&mut g);
        settle(&mut g);
        assert_eq!(g.obj(c).zone, Zone::Battlefield);
        assert!(!g.can_pay(0, &cost("{1}")), "{name}: summoning sick (302.6)");
    }
}

// ---- Basalt Monolith ---------------------------------------------------------

#[test]
fn basalt_monolith_makes_three_colorless_stays_tapped_and_untaps_for_three() {
    // "{T}: Add {C}{C}{C}."
    let g = scen().battlefield(0, &["Basalt Monolith"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{C}{C}{C}")) && !g.can_pay(0, &cost("{4}")));
    assert!(!g.can_pay(0, &cost("{U}")));
    let mut g = scen()
        .battlefield(0, &["Basalt Monolith", "Island", "Island", "Island"])
        .hand(0, &["Test Zero"])
        .at(Step::Main1, 0)
        .start();
    let mono = bf(&g, 0, "Basalt Monolith");
    // "{3}: Untap this artifact." Nothing to untap yet: still activatable
    // (the cost can be paid), but the Monolith is untapped.
    let z = g.card(0, "Test Zero", Zone::Hand);
    g.choose(Choice::Cast(z));
    g.choose(act(mono, 0));
    g.drive_until(at_priority);
    assert!(g.obj(mono).tapped);
    assert_eq!(g.player(0).pool.amt[engine::mana::C], 2, "the extra {{C}}{{C}} stay (106.4)");
    settle(&mut g);
    // "This artifact doesn't untap during your untap step." (502.3)
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.obj(mono).tapped, "still tapped after its controller's untap step");
    assert!(!g.obj(bf(&g, 0, "Island")).tapped, "the Islands untapped");
    g.choose(act(mono, 1));
    pay(&mut g);
    settle(&mut g);
    assert!(!g.obj(mono).tapped, "{{3}}: untapped");
    assert!(g.can_pay(0, &cost("{C}{C}{C}")));
}

// ---- Expedition Map, Manifold Key ---------------------------------------------

#[test]
fn expedition_map_sacrifices_to_find_any_land_card_revealed_into_hand() {
    // ruling: 7f205095
    // "{2}, {T}, Sacrifice this artifact: Search your library for a land
    // card, reveal it, put it into your hand, then shuffle."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Expedition Map", "Island", "Island"])
        .library(0, &["Lightning Bolt", "Wasteland", "Forest", "Test Bear"])
        .library(1, &["Island"; 5])
        .at(Step::Main1, 0)
        .start();
    let map = bf(&g, 0, "Expedition Map");
    g.choose(act(map, 0));
    pay(&mut g);
    assert_eq!(g.obj(map).zone, Zone::Graveyard);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    let mut offered: Vec<u16> = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Def(d) => Some(d),
            _ => None,
        })
        .collect();
    offered.sort();
    let mut want = vec![def("Wasteland"), def("Forest")];
    want.sort();
    assert_eq!(offered, want, "land cards only");
    g.choose(Choice::Def(def("Wasteland")));
    let w = g.card(0, "Wasteland", Zone::Hand);
    assert!(g.knows(1, w), "revealed");
    // Its own {T} is part of the cost: with fewer than two other mana it
    // can't be activated.
    let g = scen().battlefield(0, &["Expedition Map", "Island"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Expedition Map"), 0)));
}

#[test]
fn manifold_key_untaps_another_artifact_or_makes_a_creature_unblockable() {
    // "{1}, {T}: Untap another target artifact."
    let mut g = scen().battlefield(0, &["Manifold Key", "Basalt Monolith", "Island"]).at(Step::Main1, 0).start();
    let key = bf(&g, 0, "Manifold Key");
    let mono = bf(&g, 0, "Basalt Monolith");
    // Tap the Monolith for mana first (cast Test Zero) is not needed: target
    // it while tapped by paying the Key's {1} with the Monolith itself.
    g.choose(act(key, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(key)), "another target artifact");
    assert!(!g.has_option(g.tgt(bf(&g, 0, "Island"))), "an artifact");
    g.choose(g.tgt(mono));
    g.choose(act(mono, 0)); // pay {1} with the Monolith ({C}{C} float)
    g.drive_until(at_priority);
    assert!(g.obj(mono).tapped && g.obj(key).tapped);
    settle(&mut g);
    assert!(!g.obj(mono).tapped, "untapped");
    // "{3}, {T}: Target creature can't be blocked this turn."
    let mut g = scen()
        .battlefield(0, &["Manifold Key", "Island", "Island", "Island", "Test Bear"])
        .battlefield(1, &["Test Wall"])
        .at(Step::Main1, 0)
        .start();
    let key = bf(&g, 0, "Manifold Key");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(act(key, 1));
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    // The Wall can't block it: no blocker decision offers the Bear.
    for _ in 0..50 {
        if g.turn().step == Step::Main2 {
            break;
        }
        match g.kind() {
            Some(DecisionKind::DeclareBlockers) => {
                assert!(!g.has_option(Choice::Card(bear)), "can't be blocked");
                g.choose(Choice::Pass);
            }
            _ => {
                let c = if at_priority(&g) { Choice::Pass } else { g.pending_decision().unwrap().options[0].choice };
                g.choose(c);
            }
        }
    }
    assert_eq!(g.life(1), 18);
}

// ---- Retrofitter Foundry -------------------------------------------------------

#[test]
fn retrofitter_foundry_makes_servos_thopters_and_constructs() {
    let mut g =
        scen().battlefield(0, &["Retrofitter Foundry"]).battlefield(0, &["Island"; 6]).at(Step::Main1, 0).start();
    let rf = bf(&g, 0, "Retrofitter Foundry");
    // Nothing to sacrifice: the Thopter and Construct abilities can't be
    // activated.
    assert!(!g.has_option(act(rf, 2)) && !g.has_option(act(rf, 3)));
    // "{2}, {T}: Create a 1/1 colorless Servo artifact creature token."
    g.choose(act(rf, 1));
    pay(&mut g);
    settle(&mut g);
    let servo = bf(&g, 0, "Servo");
    assert_eq!(g.pt(servo), (1, 1));
    assert!(g.is_type(servo, engine::db::types::ARTIFACT) && g.is_creature(servo));
    assert_eq!(g.colors_of(servo), 0, "colorless");
    assert!(g.obj(rf).tapped);
    assert!(!g.has_option(act(rf, 2)), "tapped: no {{T}} ability");
    // "{3}: Untap this artifact."
    g.choose(act(rf, 0));
    pay(&mut g);
    settle(&mut g);
    assert!(!g.obj(rf).tapped);
    // A Servo is no Thopter: the Construct ability still can't be used.
    assert!(!g.has_option(act(rf, 3)));
    // "{1}, {T}, Sacrifice a Servo: Create a 1/1 colorless Thopter artifact
    // creature token with flying."
    g.choose(act(rf, 2));
    g.drive_until(|g| !matches!(g.kind(), Some(DecisionKind::Priority)) || g.stack().len() == 1);
    pay(&mut g);
    if g.kind() == Some(DecisionKind::Sacrifice) {
        g.choose(Choice::Card(servo));
    }
    g.drive_until(at_priority);
    assert_eq!(g.obj(servo).zone, Zone::Nowhere, "the Servo token was sacrificed (704.5d)");
    settle(&mut g);
    let thopter = bf(&g, 0, "Thopter");
    assert_eq!(g.pt(thopter), (1, 1));
    assert!(g.has_kw(thopter, Kw::Flying));
    assert_eq!(g.colors_of(thopter), 0, "colorless");
    // Tapped again and no mana left for the untap: the Construct waits for
    // the next turn. "{T}, Sacrifice a Thopter: Create a 4/4 colorless
    // Construct artifact creature token."
    assert!(!g.has_option(act(rf, 3)), "tapped");
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(act(rf, 3)));
    g.choose(act(rf, 3));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::Sacrifice));
    if g.kind() == Some(DecisionKind::Sacrifice) {
        g.choose(Choice::Card(thopter));
    }
    settle(&mut g);
    let construct = bf(&g, 0, "Construct");
    assert_eq!(g.pt(construct), (4, 4));
    assert!(g.is_type(construct, engine::db::types::ARTIFACT) && g.is_creature(construct));
    assert_eq!(g.colors_of(construct), 0, "colorless");
    assert_eq!(g.count_in(0, "Thopter", Zone::Battlefield), 0);
}

// ---- Soul-Guide Lantern, Cryogen Relic ----------------------------------------

#[test]
fn soul_guide_lantern_exiles_a_graveyard_card_then_a_graveyard_or_draws() {
    // "When this artifact enters, exile target card from a graveyard."
    let mut g = scen()
        .battlefield(0, &["Island"])
        .hand(0, &["Soul-Guide Lantern"])
        .graveyard(0, &["Test Bear"])
        .graveyard(1, &["Lightning Bolt", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let l = g.card(0, "Soul-Guide Lantern", Zone::Hand);
    g.choose(Choice::Cast(l));
    pay(&mut g);
    let bolt = g.card(1, "Lightning Bolt", Zone::Graveyard);
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bear)), "a card from any graveyard, yours too");
    assert!(!g.has_option(Choice::Pass), "exactly one target");
    g.choose(g.tgt(bolt));
    settle(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    // "{T}, Sacrifice this artifact: Exile each opponent's graveyard."
    let lantern = bf(&g, 0, "Soul-Guide Lantern");
    g.choose(act(lantern, 0));
    g.drive_until(at_priority);
    assert_eq!(g.obj(lantern).zone, Zone::Graveyard);
    settle(&mut g);
    assert!(g.player(1).graveyard.is_empty(), "the opponent's graveyard");
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "not yours");
    // "{1}, {T}, Sacrifice this artifact: Draw a card."
    let mut g = scen().battlefield(0, &["Soul-Guide Lantern", "Island"]).at(Step::Main1, 0).start();
    let lantern = bf(&g, 0, "Soul-Guide Lantern");
    g.choose(act(lantern, 1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1);
    assert_eq!(g.obj(lantern).zone, Zone::Graveyard);
}

#[test]
fn cryogen_relic_draws_on_entering_and_leaving_and_stuns_a_tapped_creature() {
    // "When this artifact enters or leaves the battlefield, draw a card."
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island"])
        .hand(0, &["Cryogen Relic"])
        .battlefield(1, &["Test Bear", "Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    let r = g.card(0, "Cryogen Relic", Zone::Hand);
    g.choose(Choice::Cast(r));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1, "drew on entering");
    // "{1}{U}, Sacrifice this artifact: Put a stun counter on up to one
    // target tapped creature." An untapped creature is no target.
    let relic = bf(&g, 0, "Cryogen Relic");
    let bear = bf(&g, 1, "Test Bear");
    g.choose(act(relic, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bear)), "untapped: not a target");
    assert!(g.has_option(Choice::Pass), "up to one");
    g.choose(Choice::Pass);
    pay(&mut g);
    assert_eq!(g.obj(relic).zone, Zone::Graveyard);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 2, "drew on leaving");
}

#[test]
fn cr_122_1d_cryogen_relics_stun_counter_replaces_an_untap() {
    let mut g = scen()
        .battlefield(0, &["Cryogen Relic", "Island", "Island"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let relic = bf(&g, 0, "Cryogen Relic");
    // P1 attacks with the Bear (it becomes tapped); after combat P0 stuns it.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::EndCombat && at_priority(g) && g.to_move() == Some(0));
    assert!(g.obj(bear).tapped);
    g.choose(act(relic, 0));
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::Stun), 1);
    // P1's next untap step removes the counter instead of untapping it.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.turn().step == Step::Main1);
    assert!(g.obj(bear).tapped, "stun: stays tapped");
    assert_eq!(g.counters_on(bear, Counter::Stun), 0, "the counter was removed instead");
}

// ---- Cori-Steel Cutter ---------------------------------------------------------

#[test]
fn cori_steel_cutter_makes_a_monk_on_your_second_spell_and_may_attach() {
    let mut g = scen()
        .battlefield(0, &["Cori-Steel Cutter", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let cutter = bf(&g, 0, "Cori-Steel Cutter");
    let bolt = |g: &mut Game| {
        let b = g.card(0, "Lightning Bolt", Zone::Hand);
        g.choose(Choice::Cast(b));
        g.choose(Choice::Target(Target::Player(1)));
        pay(g);
    };
    bolt(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Monk", Zone::Battlefield), 0, "first spell: no flurry");
    // "Flurry - Whenever you cast your second spell each turn, create a 1/1
    // white Monk creature token with prowess. You may attach this Equipment
    // to it."
    bolt(&mut g);
    let mut asked = false;
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::YesNo) { Some(Choice::Yes) } else { None });
    let monk = bf(&g, 0, "Monk");
    asked |= g.obj(cutter).attached.is_some();
    assert!(asked, "attached");
    assert_eq!(g.obj(cutter).attached.map(|r| r.idx), Some(monk));
    assert_eq!(g.colors_of(monk), 1 << engine::mana::W, "white");
    // "Equipped creature gets +1/+1 and has trample and haste."
    assert_eq!(g.pt(monk), (2, 2));
    assert!(g.has_kw(monk, Kw::Trample) && g.has_kw(monk, Kw::Haste));
    assert_eq!(g.life(1), 14);
    // The third spell: no flurry; prowess (702.108) pumps the Monk.
    bolt(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Monk", Zone::Battlefield), 1, "only the second spell");
    assert_eq!(g.pt(monk), (3, 3), "prowess until end of turn");
}

#[test]
fn cori_steel_cutter_equips_for_one_and_red_at_sorcery_speed() {
    // "Equip {1}{R}"
    let mut g =
        scen().battlefield(0, &["Cori-Steel Cutter", "Mountain", "Mountain", "Test Bear"]).at(Step::Main1, 0).start();
    let cutter = bf(&g, 0, "Cori-Steel Cutter");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(act(cutter, 0));
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.pt(bear), (3, 3));
    assert!(g.has_kw(bear, Kw::Trample) && g.has_kw(bear, Kw::Haste));
    // Not at instant speed (the upkeep).
    let g =
        scen().battlefield(0, &["Cori-Steel Cutter", "Mountain", "Mountain", "Test Bear"]).at(Step::Upkeep, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Cori-Steel Cutter"), 0)));
}
