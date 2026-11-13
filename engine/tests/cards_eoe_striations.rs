//! Station striations (CR 722.2-722.3; ENGINE.md 146.1): "Any abilities
//! printed within the same text box striation as a station symbol are part of
//! its static ability", and a striation runs from its station symbol to the
//! next one or to the bottom of the text box. A station card's abilities
//! printed below an "N+ | ..." line are that striation's: the permanent has
//! them only with N or more charge counters (722.2a). Only the abilities above
//! the first station symbol are treated normally (722.4).
//!
//! Six Edge of Eternities cards print a second ability in their last
//! striation; until ENGINE.md 146 the IR left it untiered (docs/ir_defects.md
//! 340, mtg-bot's Spacecraft builder) and ENGINE.md 99.8 read that as CR
//! 722.4: The Seriema (7+), Debris Field Crusher (8+), Entropic Battlecruiser
//! (8+), Infinite Guideline Station (12+), Sledge-Class Seedship (7+),
//! Specimen Freighter (9+). The printed layouts were checked on Gatherer
//! (EOE 35, 131, 99, 219, 208, 76): each ability sits below its striation's
//! flying line, with no station symbol of its own. The set's rulings (in no
//! docs/rulings.md: the cube's): "each station symbol grants its
//! striation's abilities once the permanent has that many or more counters".
//! These tests are the Fable review's probes `zz_rules_c::c02`-`c04`, turned
//! round to the Oracle text, plus a sweep of every station card's IR.

mod eoe_x_kit;

use engine::db::{types, Kw};
use engine::{DecisionKind, Game, Step, Zone};
use eoe_x_kit::*;

fn is_creature(g: &Game, c: engine::CardIdx) -> bool {
    g.types_of(c) & types::CREATURE != 0
}

// ---- The Seriema (7+) -----------------------------------------------------------------

/// The Seriema: "7+ | Flying [5/5] / Other tapped legendary creatures you
/// control have indestructible." The grant is in the 7+ striation (CR
/// 722.2a): a tapped Haliya has indestructible only while The Seriema has
/// 7 or more charge counters.
#[test]
fn cr_722_2a_the_seriema_grants_indestructible_only_at_seven() {
    for k in [0u64, 6, 7] {
        let mut g = scen()
            .battlefield(0, &["The Seriema", "Haliya, Guided by Light"])
            .counters(0, "The Seriema", charge(), k)
            .at(Step::Main1, 0)
            .start();
        let (ship, haliya) = (bf(&g, 0, "The Seriema"), bf(&g, 0, "Haliya, Guided by Light"));
        g.set_tapped(haliya, true);
        assert_eq!(g.has_kw(haliya, Kw::Indestructible), k >= 7, "{k} charge counters");
        assert_eq!(is_creature(&g, ship) && g.has_kw(ship, Kw::Flying), k >= 7, "{k}: 7+ | Flying [5/5]");
    }
}

/// The probe `zz_rules_c::c02`: an unstationed The Seriema (no charge
/// counter) leaves a tapped Haliya, Guided by Light (3/3) to die to Bombard's
/// 4 damage (CR 704.5g); with 7 counters she is indestructible and survives
/// (702.12b).
#[test]
fn cr_722_2a_an_unstationed_seriema_doesnt_save_a_tapped_legend() {
    for k in [0u64, 7] {
        let mut g = scen()
            .battlefield(0, &["The Seriema", "Haliya, Guided by Light", "Mountain", "Mountain", "Mountain"])
            .counters(0, "The Seriema", charge(), k)
            .hand(0, &["Bombard"])
            .at(Step::Main1, 0)
            .start();
        let haliya = bf(&g, 0, "Haliya, Guided by Light");
        g.set_tapped(haliya, true);
        let t = g.tgt(haliya);
        cast(&mut g, 0, "Bombard", &[t]);
        settle(&mut g);
        let zone = g.obj(haliya).zone;
        assert_eq!(zone == Zone::Battlefield, k >= 7, "{k} charge counters: Haliya in {zone:?}\n{}", log(&g));
    }
}

// ---- Debris Field Crusher (8+) --------------------------------------------------------

/// The probe `zz_rules_c::c03`: Debris Field Crusher's "{1}{R}: This
/// Spacecraft gets +2/+0 until end of turn" is printed below "8+ | Flying":
/// the permanent has it, so it can be activated (CR 602.1), only with 8 or
/// more charge counters (722.2a).
#[test]
fn cr_722_2a_debris_field_crusher_has_its_pump_only_at_eight() {
    for k in [0u64, 7, 8] {
        let g = scen()
            .battlefield(0, &["Debris Field Crusher", "Mountain", "Mountain"])
            .counters(0, "Debris Field Crusher", charge(), k)
            .at(Step::Main1, 0)
            .start();
        let ship = bf(&g, 0, "Debris Field Crusher");
        assert_eq!(g.has_option(act(ship, 0)), k >= 8, "{k} charge counters: {:?}", g.options_desc());
    }
}

// ---- The attack triggers (Entropic Battlecruiser 8+, Infinite Guideline Station 12+,
// Sledge-Class Seedship 7+, Specimen Freighter 9+) --------------------------------------

/// `ship` with `k` charge counters attacks, made a 2/2 flying artifact
/// creature by Synthesizer Labship's 2+ ability ("At the beginning of combat
/// on your turn, up to one other target artifact you control becomes an
/// artifact creature with base power and toughness 2/2 and gains flying
/// until end of turn") however many counters it has (the probe
/// `zz_rules_c::c04`'s route; Tezzeret's emblem is another). Returns the game
/// with the attackers declared and their triggers on the stack.
fn attack_animated(ship: &str, k: u64, hand: &[&str]) -> Game {
    let mut g = scen()
        .battlefield(0, &["Synthesizer Labship", ship])
        .counters(0, "Synthesizer Labship", charge(), 2)
        .counters(0, ship, charge(), k)
        .hand(0, hand)
        .hand(1, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, ship);
    for _ in 0..50 {
        g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::Target) | Some(DecisionKind::DeclareAttackers)) || at_priority(g));
        match g.kind() {
            Some(DecisionKind::Target) => {
                let t = g.tgt(s);
                g.choose(t);
            }
            Some(DecisionKind::DeclareAttackers) => break,
            _ => {
                g.pass();
            }
        }
    }
    assert!(is_creature(&g, s) && g.has_kw(s, Kw::Flying), "{ship} animated\n{}", log(&g));
    attack_with(&mut g, 0, &[ship]);
    g.drive_until(at_priority);
    assert!(g.obj(s).attacking, "{}", log(&g));
    g
}

/// The attack trigger exists only from the striation's threshold (CR
/// 722.2a; 603.2, 603.3: an ability the permanent doesn't have doesn't
/// trigger): below it, attacking puts nothing on the stack; at it, the
/// trigger goes on the stack.
fn attack_trigger_needs(ship: &str, n: u64, hand: &[&str]) {
    let g = attack_animated(ship, n - 1, hand);
    assert!(g.stack().is_empty(), "{ship} with {} charge counters: no trigger\n{}", n - 1, log(&g));
    let g = attack_animated(ship, n, hand);
    assert_eq!(g.stack().len(), 1, "{ship} with {n} charge counters: its attack trigger\n{}", log(&g));
}

/// Specimen Freighter: "9+ | Flying [4/7] / Whenever this Spacecraft
/// attacks, defending player mills four cards." Below 9 counters: no mill.
#[test]
fn cr_722_2a_specimen_freighter_attack_trigger_only_at_nine() {
    let mut g = attack_animated("Specimen Freighter", 0, &[]);
    let lib = g.player(1).library.len();
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.player(1).library.len(), lib, "0 charge counters: nothing milled\n{}", log(&g));
    attack_trigger_needs("Specimen Freighter", 9, &[]);
}

/// Sledge-Class Seedship: "7+ | Flying [4/5] / Whenever this Spacecraft
/// attacks, you may put a creature card from your hand onto the
/// battlefield."
#[test]
fn cr_722_2a_sledge_class_seedship_attack_trigger_only_at_seven() {
    let mut g = attack_animated("Sledge-Class Seedship", 6, &["Cross Giant"]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.count_in(0, "Cross Giant", Zone::Hand), 1, "6 charge counters: nothing put onto the battlefield\n{}", log(&g));
    attack_trigger_needs("Sledge-Class Seedship", 7, &["Cross Giant"]);
}

/// Infinite Guideline Station: "12+ | Flying [7/15] / Whenever Infinite
/// Guideline Station attacks, draw a card for each multicolored permanent
/// you control."
#[test]
fn cr_722_2a_infinite_guideline_station_attack_trigger_only_at_twelve() {
    let mut g = attack_animated("Infinite Guideline Station", 11, &[]);
    let hand = g.player(0).hand.len();
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.player(0).hand.len(), hand, "11 charge counters: no card drawn\n{}", log(&g));
    attack_trigger_needs("Infinite Guideline Station", 12, &[]);
}

/// Entropic Battlecruiser: "1+ | Whenever an opponent discards a card, they
/// lose 3 life. / 8+ | Flying, deathtouch [3/10] / Whenever this Spacecraft
/// attacks, each opponent discards a card. Each opponent who can't loses 3
/// life." The attack trigger is the 8+ striation's: with 7 counters the
/// opponent keeps their card and their life (the 1+ trigger has no discard
/// to see).
#[test]
fn cr_722_2a_entropic_battlecruiser_attack_trigger_only_at_eight() {
    let mut g = attack_animated("Entropic Battlecruiser", 7, &[]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.player(1).hand.len(), 1, "7 charge counters: no discard\n{}", log(&g));
    assert_eq!(g.life(1), 18, "the 2/2's combat damage only\n{}", log(&g));
    attack_trigger_needs("Entropic Battlecruiser", 8, &[]);
}

// ---- The sweep: every station card's IR against its printed striations ---------------

/// Every station card of the Edge of Eternities table (22 Spacecraft, 5
/// Planets), each of its triggered, static and activated abilities with the
/// threshold of the striation it is printed in (0: above the first station
/// symbol, CR 722.4), read from the Oracle text (Scryfall's, which writes a
/// striation's further abilities on lines below its "N+ |" line). The IR must
/// say the same (`tier_threshold`): a re-export that drops a striation fails
/// here. Keywords and P/T boxes (`stationed_keywords`) are the striation of
/// `station_threshold`, checked by `cards_eoe_a.rs`; station itself is
/// appended by the lowering (702.184a).
#[test]
fn cr_722_2_every_station_cards_abilities_are_in_their_printed_striations() {
    // (name, station_threshold, triggered, static, activated): thresholds,
    // sorted.
    #[rustfmt::skip]
    let printed: &[(&str, i64, &[i64], &[i64], &[i64])] = &[
        // "Station ... / 8+ | Flying, trample" under "When this Spacecraft enters, put a +1/+1 counter ...".
        ("Atmospheric Greenhouse", 8, &[0], &[], &[]),
        // "10+ | Whenever you attack, Dawnsire deals 100 damage ... / 20+ | Flying".
        ("Dawnsire, Sunstar Dreadnought", 20, &[10], &[], &[]),
        // "When this Spacecraft enters, it deals 3 damage ... / 8+ | Flying / {1}{R}: ... +2/+0".
        ("Debris Field Crusher", 8, &[0], &[], &[8]),
        // "1+ | Whenever an opponent discards ... / 8+ | Flying, deathtouch / Whenever this Spacecraft attacks ...".
        ("Entropic Battlecruiser", 8, &[1, 8], &[], &[]),
        ("Extinguisher Battleship", 5, &[0], &[], &[]),
        ("Fell Gravship", 8, &[0], &[], &[]),
        ("Galvanizing Sawship", 3, &[], &[], &[]),
        // "When ... enters, create ... / 12+ | Flying / Whenever Infinite Guideline Station attacks ...".
        ("Infinite Guideline Station", 12, &[0, 12], &[], &[]),
        ("Larval Scoutlander", 7, &[0], &[], &[]),
        // "2+ | Other creatures you control get +1/+1. / 12+ | Flying, lifelink".
        ("Lumen-Class Frigate", 12, &[], &[2], &[]),
        ("Pinnacle Kill-Ship", 7, &[0], &[], &[]),
        ("Rescue Skiff", 10, &[0], &[], &[]),
        // "7+ | Flying / Whenever this Spacecraft attacks, you may put a creature card ...".
        ("Sledge-Class Seedship", 7, &[7], &[], &[]),
        // "When ... enters, return up to two ... / 9+ | Flying / Whenever this Spacecraft attacks, defending player mills four".
        ("Specimen Freighter", 9, &[0, 9], &[], &[]),
        ("Susurian Dirgecraft", 7, &[0], &[], &[]),
        // "2+ | At the beginning of combat on your turn, ... / 9+ | Flying, vigilance".
        ("Synthesizer Labship", 9, &[2], &[], &[]),
        // "{T}: Add {C}{C}{C}. / Station ... / 20+ | {T}: Add X mana of any one color ...".
        ("The Eternity Elevator", 20, &[], &[], &[0, 20]),
        // "When The Seriema enters, search ... / 7+ | Flying / Other tapped legendary creatures you control have indestructible."
        ("The Seriema", 7, &[0], &[7], &[]),
        ("Uthros Scanship", 8, &[0], &[], &[]),
        ("Warmaker Gunship", 6, &[0], &[], &[]),
        ("Wedgelight Rammer", 9, &[0], &[], &[]),
        ("Wurmwall Sweeper", 4, &[0], &[], &[]),
        // The Planets: "This land enters tapped. / {T}: Add {W}. / Station ... / 12+ | <ability>" (the
        // mana ability is `produces`).
        ("Adagia, Windswept Bastion", 12, &[], &[], &[12]),
        ("Evendo, Waking Haven", 12, &[], &[], &[12]),
        ("Kavaron, Memorial World", 12, &[], &[], &[12]),
        ("Susur Secundi, Void Altar", 12, &[], &[], &[12]),
        ("Uthros, Titanic Godcore", 12, &[], &[], &[12]),
    ];
    let doc = engine::db::load_eoe_document();
    let mut seen = Vec::new();
    for card in &doc.cards {
        let card_ir::cats::CardPrinting::CardPrinting(p) = &card.printing;
        let Some(st) = p.station_threshold else { continue };
        seen.push(card.name.clone());
        let row = printed.iter().find(|r| r.0 == card.name).unwrap_or_else(|| panic!("{}: a station card the table doesn't list", card.name));
        let sorted = |mut v: Vec<i64>| {
            v.sort();
            v
        };
        let trig = sorted(p.triggered_abilities.iter().map(|t| {
            let card_ir::cats::TriggeredAbility::TriggeredAbility(t) = t;
            t.tier_threshold
        }).collect());
        let stat = sorted(p.static_abilities.iter().map(|s| {
            let card_ir::cats::StaticAbility::StaticAbility(s) = s;
            s.tier_threshold
        }).collect());
        let act = sorted(p.activated_abilities.iter().map(|a| {
            let card_ir::cats::ActivatedAbility::ActivatedAbility(a) = a;
            a.tier_threshold
        }).collect());
        assert_eq!(st, row.1, "{}: station_threshold", card.name);
        assert_eq!((&trig[..], &stat[..], &act[..]), (row.2, row.3, row.4), "{}: (triggered, static, activated) striations", card.name);
        // Every lowered definition of a supported station card exists (the
        // tiers lower through `lower_*_tiered`).
        assert!(db().id(&card.name).is_some(), "{} supported", card.name);
    }
    seen.sort();
    let mut want: Vec<String> = printed.iter().map(|r| r.0.to_string()).collect();
    want.sort();
    assert_eq!(seen, want, "the station cards");
}
