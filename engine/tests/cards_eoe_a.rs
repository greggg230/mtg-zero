//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): station and the
//! station symbols (CR 702.184, 722), and the Spacecraft that need nothing
//! else. Test names cite the CR; the rulings quoted are the set's (Scryfall,
//! 2025-07-25), not in docs/rulings.md (the cube's).

mod eoe_a_kit;

use eoe_a_kit::*;
use engine::db::{types, Kw};
use engine::{Choice, DecisionKind, Game, Step, Target, Zone};

fn creature(g: &Game, c: engine::CardIdx) -> bool {
    g.types_of(c) & types::CREATURE != 0
}

fn cast(g: &mut Game, c: engine::CardIdx, target: Option<engine::CardIdx>) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
    if let Some(t) = target {
        if g.kind() == Some(DecisionKind::Target) {
            g.choose(g.tgt(t));
        }
    }
    pay(g);
}

// ---- station (CR 702.184a) ------------------------------------------------

/// "Tap another untapped creature you control: Put a number of charge
/// counters on this permanent equal to the tapped creature's power. Activate
/// only as a sorcery." The cost is a choice among the other untapped
/// creatures (a summoning-sick one too: not {T}, CR 302.6); the counters
/// add up.
#[test]
fn station_taps_another_creature_for_charge_counters_equal_to_its_power_cr702_184a() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Bear", "Lane A Giant", "Forest"])
        .hand(0, &["Lane A Dork"])
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    let (bear, giant) = (bf(&g, 0, "Lane A Bear"), bf(&g, 0, "Lane A Giant"));
    // A creature that entered this turn may be tapped for it.
    let dork = g.card(0, "Lane A Dork", Zone::Hand);
    cast(&mut g, dork, None);
    settle(&mut g);
    assert!(g.is_sick(dork));
    let s = station_slot(&g, ship);
    g.choose(act(ship, s));
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent));
    for c in [bear, giant, dork] {
        assert!(g.has_option(Choice::Card(c)), "{} may be tapped", g.name(c));
    }
    assert!(!g.has_option(Choice::Card(ship)), "not the Spacecraft itself");
    g.choose(Choice::Card(bear));
    assert!(g.objs()[bear.i()].tapped, "tapped as the cost is paid");
    settle(&mut g);
    assert_eq!(g.counters_on(ship, charge()), 2);
    assert!(!creature(&g, ship));
    // Another creature, the same turn: the counters add up.
    station(&mut g, ship, giant);
    assert_eq!(g.counters_on(ship, charge()), 7);
    assert!(creature(&g, ship), "{}", log(&g));
    // A tapped creature can't pay it (the Bear and the Giant are tapped).
    g.choose(act(ship, s));
    assert!(!g.has_option(Choice::Card(bear)));
    assert!(g.has_option(Choice::Card(dork)));
}

/// Station only as a sorcery (CR 702.184a, 307.1): not in the upkeep, not
/// in the opponent's turn, not while the stack holds something.
#[test]
fn station_only_as_a_sorcery_cr702_184a() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Bear", "Forest"])
        .hand(0, &["Lane A Growth"])
        .at(Step::Upkeep, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    let s = station_slot(&g, ship);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority));
    assert!(!g.has_option(act(ship, s)), "upkeep");
    quiet_until(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(act(ship, s)), "main phase, empty stack");
    let growth = g.card(0, "Lane A Growth", Zone::Hand);
    let bear = bf(&g, 0, "Lane A Bear");
    cast(&mut g, growth, Some(bear));
    assert!(!g.stack().is_empty() && at_priority(&g));
    assert!(!g.has_option(act(ship, s)), "with the stack not empty");
    settle(&mut g);
    next_main(&mut g, 1);
    g.pass();
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority) && g.turn().active == 1);
    assert!(!g.has_option(act(ship, s)));
}

/// "Use the tapped creature's power as the station ability resolves to
/// determine how many charge counters to put on the permanent with station."
#[test]
fn station_reads_the_tapped_creatures_power_as_it_resolves_cr702_184a() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Bear", "Forest"])
        .hand(0, &["Lane A Growth"])
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    let bear = bf(&g, 0, "Lane A Bear");
    let s = station_slot(&g, ship);
    g.choose(act(ship, s));
    g.choose(Choice::Card(bear));
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.stack().len(), 1);
    // In response, the Bear gets +3/+3: five counters.
    let growth = g.card(0, "Lane A Growth", Zone::Hand);
    cast(&mut g, growth, Some(bear));
    settle(&mut g);
    assert_eq!(g.counters_on(ship, charge()), 5);
    assert!(creature(&g, ship) && g.power(ship) == 2 && g.toughness(ship) == 2);
    assert!(g.has_kw(ship, Kw::Flying));
}

/// "If that creature isn't on the battlefield at that time, use its power
/// as it last existed on the battlefield." (CR 608.2h.)
#[test]
fn station_uses_the_last_known_power_of_a_creature_that_left_cr608_2h() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Giant", "Mountain"])
        .hand(0, &["Lane A Bolt"])
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    let giant = bf(&g, 0, "Lane A Giant");
    let s = station_slot(&g, ship);
    g.choose(act(ship, s));
    g.choose(Choice::Card(giant));
    g.drive_until(|g| at_priority(g));
    let bolt = g.card(0, "Lane A Bolt", Zone::Hand);
    cast(&mut g, bolt, Some(giant));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Lane A Giant", Zone::Graveyard), 1);
    assert_eq!(g.counters_on(ship, charge()), 5);
}

/// "If the tapped creature has negative power, no charge counters are put
/// onto or removed from the permanent with station." (CR 107.1b.)
#[test]
fn station_with_negative_power_puts_and_removes_nothing_cr702_184a() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Weakling"])
        .counters(0, "Wurmwall Sweeper", charge(), 2)
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    let weak = bf(&g, 0, "Lane A Weakling");
    assert_eq!(g.power(weak), -1);
    station(&mut g, ship, weak);
    assert_eq!(g.counters_on(ship, charge()), 2);
    assert!(g.objs()[weak.i()].tapped);
}

/// The cost needs another untapped creature: a creature with a mana ability
/// counts (no mana to pay competes with it); with none, station isn't
/// offered, and a stationed Spacecraft can't tap itself ("another").
#[test]
fn station_needs_another_untapped_creature_cr702_184a() {
    let g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Dork"])
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    assert!(g.has_option(act(ship, station_slot(&g, ship))), "the mana creature can pay it");
    let g = scen()
        .battlefield(0, &["Wurmwall Sweeper"])
        .counters(0, "Wurmwall Sweeper", charge(), 4)
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    assert!(creature(&g, ship));
    assert!(!g.has_option(act(ship, station_slot(&g, ship))), "no other creature");
}

/// "Even if a permanent with station has equal or greater charge counters
/// than the number in its last station symbol, you can still activate its
/// station ability." (CR 722.4.)
#[test]
fn station_can_be_activated_past_the_last_threshold_cr722_4() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Bear"])
        .counters(0, "Wurmwall Sweeper", charge(), 9)
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    let bear = bf(&g, 0, "Lane A Bear");
    station(&mut g, ship, bear);
    assert_eq!(g.counters_on(ship, charge()), 11);
}

// ---- station symbols (CR 722.2) -------------------------------------------

/// "{N+}[abilities][P/T]" means "as long as this permanent has N or more
/// charge counters on it, it has [abilities] and is a creature with base
/// power and toughness [P/T] in addition to its other types" (CR 722.2b);
/// below N it is a noncreature artifact. Each Spacecraft of the set whose
/// other abilities are supported, against its Oracle text.
#[test]
fn spacecraft_are_artifact_creatures_at_their_threshold_cr722_2b() {
    let ships: &[(&str, u32, i32, i32, &[Kw])] = &[
        ("Atmospheric Greenhouse", 8, 5, 4, &[Kw::Flying, Kw::Trample]),
        ("Debris Field Crusher", 8, 1, 5, &[Kw::Flying]),
        ("Extinguisher Battleship", 5, 10, 10, &[Kw::Flying, Kw::Trample]),
        ("Galvanizing Sawship", 3, 6, 5, &[Kw::Flying, Kw::Haste]),
        ("Lumen-Class Frigate", 12, 3, 5, &[Kw::Flying, Kw::Lifelink]),
        ("Rescue Skiff", 10, 5, 6, &[Kw::Flying]),
        ("Susurian Dirgecraft", 7, 4, 3, &[Kw::Flying]),
        ("Uthros Scanship", 8, 4, 4, &[Kw::Flying]),
        ("Wedgelight Rammer", 9, 3, 4, &[Kw::Flying, Kw::FirstStrike]),
        ("Wurmwall Sweeper", 4, 2, 2, &[Kw::Flying]),
    ];
    for &(name, n, p, t, kws) in ships {
        for k in [0, n - 1, n, n + 3] {
            let g = scen().battlefield(0, &[name]).counters(0, name, charge(), k as u64).at(Step::Main1, 0).start();
            let c = bf(&g, 0, name);
            assert!(g.types_of(c) & types::ARTIFACT != 0, "{name}");
            assert_eq!(creature(&g, c), k >= n, "{name} with {k}");
            for &kw in kws {
                assert_eq!(g.has_kw(c, kw), k >= n, "{name} {kw:?} with {k}");
            }
            if k >= n {
                assert_eq!((g.power(c), g.toughness(c)), (p.into(), t.into()), "{name}");
            }
            // The station ability, at all times (CR 722.4).
            let d = g.def_of(c);
            assert!(d.activated.last().is_some_and(|a| a.sorcery && a.cost.tap_untapped.is_some()), "{name}");
        }
    }
}

/// "If a station card has a power and toughness box printed in one of the
/// striations in its text box, it's not a creature unless it has the
/// appropriate number of charge counters on it. That card also doesn't have
/// that power and toughness in any zone other than the battlefield." (CR
/// 722.2c.)
#[test]
fn a_station_card_has_no_power_or_toughness_off_the_battlefield_cr722_2c() {
    let g = scen().hand(0, &["Galvanizing Sawship"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Galvanizing Sawship", Zone::Hand);
    assert_eq!(g.def_of(c).power, None);
    assert_eq!(g.def_of(c).toughness, None);
    assert!(!creature(&g, c));
}

/// "If a permanent with station becomes a creature, it will be able to
/// attack if it's been under your control continuously since the turn
/// began" (CR 302.6): a Spacecraft cast this turn and stationed can't
/// attack; one that has been there can, and haste at 3+ lets the new one.
#[test]
fn a_stationed_spacecraft_attacks_by_how_long_it_was_controlled_cr302_6() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Giant", "Lane A Bear", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .counters(0, "Wurmwall Sweeper", charge(), 4)
        .hand(0, &["Galvanizing Sawship"])
        .at(Step::Main1, 0)
        .start();
    let old = bf(&g, 0, "Wurmwall Sweeper");
    let saw = g.card(0, "Galvanizing Sawship", Zone::Hand);
    cast(&mut g, saw, None);
    settle(&mut g);
    let giant = bf(&g, 0, "Lane A Giant");
    station(&mut g, saw, giant);
    assert!(creature(&g, saw) && g.has_kw(saw, Kw::Haste));
    // Attack: the Sweeper (stationed earlier) and the hasty Sawship may.
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(g.has_option(Choice::Card(old)), "under control since the turn began");
    assert!(g.has_option(Choice::Card(saw)), "haste");
    let mut g = scen()
        .battlefield(0, &["Lane A Giant", "Mountain", "Mountain"])
        .hand(0, &["Lane A Tier Ship"])
        .at(Step::Main1, 0)
        .start();
    let ship = g.card(0, "Lane A Tier Ship", Zone::Hand);
    cast(&mut g, ship, None);
    settle(&mut g);
    let giant = bf(&g, 0, "Lane A Giant");
    station(&mut g, ship, giant);
    assert!(creature(&g, ship));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers) || g.turn().step == Step::Main2);
    assert!(!g.has_option(Choice::Card(ship)), "it entered this turn");
}

/// "{N+}[abilities]" (CR 722.2a): Lumen-Class Frigate's "2+ | Other
/// creatures you control get +1/+1" applies from two charge counters, not
/// to the Frigate itself (a creature at 12+, with flying and lifelink).
#[test]
fn lumen_class_frigate_two_plus_static_cr722_2a() {
    for (k, bear_pt) in [(1, (2, 2)), (2, (3, 3)), (12, (3, 3))] {
        let g = scen()
            .battlefield(0, &["Lumen-Class Frigate", "Lane A Bear"])
            .battlefield(1, &["Lane A Giant"])
            .counters(0, "Lumen-Class Frigate", charge(), k)
            .at(Step::Main1, 0)
            .start();
        let (f, bear, giant) = (bf(&g, 0, "Lumen-Class Frigate"), bf(&g, 0, "Lane A Bear"), bf(&g, 1, "Lane A Giant"));
        assert_eq!((g.power(bear), g.toughness(bear)), bear_pt, "with {k}");
        assert_eq!((g.power(giant), g.toughness(giant)), (5, 5), "only creatures you control");
        if k >= 12 {
            assert_eq!((g.power(f), g.toughness(f)), (3, 5), "not \"other\"");
            assert!(g.has_kw(f, Kw::Flying) && g.has_kw(f, Kw::Lifelink));
        } else {
            assert!(!creature(&g, f));
        }
    }
}

/// A triggered ability in a station striation (CR 722.2a): the permanent has
/// it, so it triggers, only with N or more charge counters. "The abilities
/// of a permanent with station are based on the number of charge counters
/// it has": stationing it up to 2 gives it the ability.
#[test]
fn a_triggered_station_ability_triggers_only_at_its_threshold_cr722_2a() {
    let mut g = scen()
        .battlefield(0, &["Lane A Tier Ship", "Lane A Bear"])
        .counters(0, "Lane A Tier Ship", charge(), 1)
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let ship = bf(&g, 0, "Lane A Tier Ship");
    let life = g.life(0);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(0), life, "one counter: no ability");
    next_main(&mut g, 0);
    let bear = bf(&g, 0, "Lane A Bear");
    station(&mut g, ship, bear);
    assert_eq!(g.counters_on(ship, charge()), 3);
    assert!(!creature(&g, ship), "a creature at 4+ only");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(0), life + 2, "{}", log(&g));
}

/// Station on a land (the planets, docs/ir_defects.md 20; CR 722.1: no
/// power/toughness box): a land with station is stationed like a
/// Spacecraft, never becomes a creature, and its activated abilities in a
/// striation can be activated from their threshold on (CR 722.2a), a mana
/// ability (paying a cost) too.
#[test]
fn a_land_with_station_gains_its_tier_abilities_cr722_2a() {
    let mut g = scen()
        .battlefield(0, &["Lane A Planet", "Lane A Bear", "Lane A Giant"])
        .hand(0, &["Wurmwall Sweeper"])
        .at(Step::Main1, 0)
        .start();
    let planet = bf(&g, 0, "Lane A Planet");
    let d = g.def_of(planet);
    // {T}: Add {G}; 3+ {T}: gain 3 life; 5+ {T}: Add {C}{C}; station.
    assert_eq!(d.activated.len(), 4);
    assert!(!g.has_option(act(planet, 1)), "3+ ability, no counters");
    let ship = g.card(0, "Wurmwall Sweeper", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(ship)), "{{G}} alone can't pay {{2}}");
    let bear = bf(&g, 0, "Lane A Bear");
    station(&mut g, planet, bear);
    assert_eq!(g.counters_on(planet, charge()), 2);
    assert!(!g.has_option(act(planet, 1)));
    let giant = bf(&g, 0, "Lane A Giant");
    station(&mut g, planet, giant);
    assert_eq!(g.counters_on(planet, charge()), 7);
    assert!(!creature(&g, planet));
    assert!(g.has_option(act(planet, 1)), "3+: {{T}}: gain 3 life");
    // 5+: its {T}: Add {C}{C} pays the Sweeper's {2}.
    assert!(g.has_option(Choice::Cast(ship)));
    cast(&mut g, ship, None);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Wurmwall Sweeper", Zone::Battlefield), 1, "{}", log(&g));
    let _ = Target::Player(0);
}
