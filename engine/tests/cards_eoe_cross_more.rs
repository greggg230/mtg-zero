//! The cross-lane pass (ENGINE.md 99.8): cards finished by features no lane
//! built, each ability against its Oracle text. Test names cite the CR.

mod eoe_x_kit;

use engine::db::{types, Counter, Kw};
use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_x_kit::*;

fn is_creature(g: &Game, c: engine::CardIdx) -> bool {
    g.types_of(c) & types::CREATURE != 0
}

fn landers(g: &Game, p: engine::PlayerId) -> usize {
    g.permanents_of(p).filter(|&c| g.name(c) == "Lander").count()
}

// ---- Survey Mechan ---------------------------------------------------------------

/// Survey Mechan: "Flying / Hexproof" [1/3] (CR 702.9, 702.11b).
#[test]
fn survey_mechan_is_a_1_3_flyer_with_hexproof_cr702_11b() {
    let g = scen().battlefield(0, &["Survey Mechan"]).at(Step::Main1, 0).start();
    let m = bf(&g, 0, "Survey Mechan");
    assert_eq!((g.power(m), g.toughness(m)), (1, 3));
    assert!(g.has_kw(m, Kw::Flying) && g.has_kw(m, Kw::Hexproof));
}

/// "{10}, Sacrifice this creature: It deals 3 damage to any target. Target
/// player draws three cards and gains 3 life. This ability costs {X} less to
/// activate, where X is the number of differently named lands you control"
/// (CR 602.2b, 601.2f; 115.4 "any target"; 608.2h: the sacrificed creature
/// deals the damage). Five differently named basics: {5}.
#[test]
fn survey_mechan_sacrifice_deals_3_and_target_player_draws_3_gains_3_cr602_2b() {
    let g = scen().battlefield(0, &["Survey Mechan"]).battlefield(0, &["Plains"; 6]).at(Step::Main1, 0).start();
    let m = bf(&g, 0, "Survey Mechan");
    assert!(!g.has_option(act(m, 0)), "{{9}} with six Plains (one name)");
    let mut g = scen()
        .battlefield(0, &["Survey Mechan", "Plains", "Island", "Swamp", "Mountain", "Forest"])
        .battlefield(1, &["Cross Giant"])
        .at(Step::Main1, 0)
        .start();
    let m = bf(&g, 0, "Survey Mechan");
    let giant = bf(&g, 1, "Cross Giant");
    assert!(g.has_option(act(m, 0)), "{{5}} with five names: {:?}", g.options_desc());
    g.choose(act(m, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(giant)) && g.has_option(player(1)) && g.has_option(player(0)), "any target");
    g.choose(player(1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(giant)), "the second target is a player");
    g.choose(player(0));
    pay(&mut g);
    let hand = g.player(0).hand.len();
    settle(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Graveyard, "sacrificed as a cost");
    assert_eq!(g.life(1), 17, "{}", log(&g));
    assert_eq!(g.life(0), 23);
    assert_eq!(g.player(0).hand.len(), hand + 3);
    assert_eq!(untapped(&g, 0), 0);
}

// ---- Seedship Impact -----------------------------------------------------------------

/// Seedship Impact: "Destroy target artifact or enchantment. If its mana
/// value was 2 or less, create a Lander token" (CR 701.8a, 111.10): a mana
/// value 1 artifact gives a Lander; a mana value 3 one doesn't.
#[test]
fn seedship_impact_destroys_and_makes_a_lander_for_mana_value_2_or_less_cr111_10() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .battlefield(1, &["Cross Trinket", "Cross Bear", "Forest", "Cross Totem"])
        .hand(0, &["Seedship Impact"])
        .at(Step::Main1, 0)
        .start();
    let (trinket, bear, forest, totem) = (bf(&g, 1, "Cross Trinket"), bf(&g, 1, "Cross Bear"), bf(&g, 1, "Forest"), bf(&g, 1, "Cross Totem"));
    let c = g.card(0, "Seedship Impact", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(trinket)) && g.has_option(g.tgt(totem)), "an artifact or an enchantment");
    assert!(!g.has_option(g.tgt(bear)) && !g.has_option(g.tgt(forest)));
    g.choose(g.tgt(trinket));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(trinket).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(landers(&g, 0), 1, "mana value 1: a Lander for the caster");
    assert_eq!(landers(&g, 1), 0);
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .battlefield(1, &["Cross Hulk"])
        .hand(0, &["Seedship Impact"])
        .at(Step::Main1, 0)
        .start();
    let hulk = bf(&g, 1, "Cross Hulk");
    let t = g.tgt(hulk);
    cast(&mut g, 0, "Seedship Impact", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(hulk).zone, Zone::Graveyard);
    assert_eq!(landers(&g, 0), 0, "mana value 3: no Lander");
}

/// The Lander it makes is the rules' Lander (CR 111.10): "{2}, {T},
/// Sacrifice this token: Search your library for a basic land card, put it
/// onto the battlefield tapped, then shuffle."
#[test]
fn seedship_impacts_lander_fetches_a_basic_land_tapped_cr111_10() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest"])
        .battlefield(1, &["Cross Totem"])
        .hand(0, &["Seedship Impact"])
        .library(0, &["Plains", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let totem = bf(&g, 1, "Cross Totem");
    let t = g.tgt(totem);
    cast(&mut g, 0, "Seedship Impact", &[t]);
    settle(&mut g);
    let lander = g.permanents_of(0).find(|&c| g.name(c) == "Lander").expect("a Lander");
    g.choose(act(lander, 0));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    let plains = db().id("Plains").unwrap();
    g.choose(Choice::Def(plains));
    settle(&mut g);
    let p = g.card(0, "Plains", Zone::Battlefield);
    assert!(g.obj(p).tapped, "{}", log(&g));
    assert_eq!(landers(&g, 0), 0, "sacrificed");
}

// ---- Terrapact Intimidator -------------------------------------------------------------

/// Terrapact Intimidator [2/1]: "When this creature enters, target opponent
/// may have you create two Lander tokens. If they don't, put two +1/+1
/// counters on this creature" (CR 603.5, 608.2d: the opponent chooses).
#[test]
fn terrapact_intimidator_the_opponent_chooses_landers_or_counters_cr608_2d() {
    for opp_says_yes in [true, false] {
        let mut g = scen().battlefield(0, &["Mountain", "Mountain"]).hand(0, &["Terrapact Intimidator"]).at(Step::Main1, 0).start();
        let c = cast(&mut g, 0, "Terrapact Intimidator", &[]);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::YesNo));
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(player(0)), "target opponent");
            g.choose(player(1));
        }
        g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
        assert_eq!(g.pending_decision().unwrap().player, 1, "the opponent decides");
        g.choose(if opp_says_yes { Choice::Yes } else { Choice::Pass });
        settle(&mut g);
        if opp_says_yes {
            assert_eq!(landers(&g, 0), 2, "{}", log(&g));
            assert_eq!(landers(&g, 1), 0);
            assert_eq!(g.counters_on(c, Counter::P1P1), 0);
            assert_eq!((g.power(c), g.toughness(c)), (2, 1));
        } else {
            assert_eq!(landers(&g, 0), 0);
            assert_eq!(g.counters_on(c, Counter::P1P1), 2, "{}", log(&g));
            assert_eq!((g.power(c), g.toughness(c)), (4, 3));
        }
    }
}

// ---- Atomic Microsizer -----------------------------------------------------------------

/// Atomic Microsizer: "Equipped creature gets +1/+0. ... Equip {2}" (CR
/// 301.5, 702.6a).
#[test]
fn atomic_microsizer_equip_2_gives_plus_1_plus_0_cr702_6a() {
    let mut g = scen().battlefield(0, &["Atomic Microsizer", "Cross Bear", "Island", "Island"]).at(Step::Main1, 0).start();
    let (eq, bear) = (bf(&g, 0, "Atomic Microsizer"), bf(&g, 0, "Cross Bear"));
    g.choose(act(eq, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(eq).attached.map(|r| r.idx), Some(bear), "{}", log(&g));
    assert_eq!((g.power(bear), g.toughness(bear)), (3, 2));
}

/// "Whenever equipped creature attacks, choose up to one target creature.
/// That creature can't be blocked this turn and has base power and
/// toughness 1/1 until end of turn" (CR 509.1b, 613.4b: layer 7b, under the
/// Equipment's +1/+0 in 7c). The Giant made 1/1 is a 2/1 attacker no one
/// may block; the effect ends with the turn (514.2).
#[test]
fn atomic_microsizer_makes_a_creature_unblockable_with_base_1_1_cr613_4b() {
    let mut g = scen()
        .battlefield(0, &["Atomic Microsizer", "Cross Giant", "Island", "Island"])
        .battlefield(1, &["Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    let (eq, giant) = (bf(&g, 0, "Atomic Microsizer"), bf(&g, 0, "Cross Giant"));
    g.choose(act(eq, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(giant));
    pay(&mut g);
    settle(&mut g);
    assert_eq!((g.power(giant), g.toughness(giant)), (6, 5));
    let bear = bf(&g, 1, "Cross Bear");
    attack_with(&mut g, 0, &["Cross Giant"]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(Choice::Pass), "up to one");
    assert!(g.has_option(g.tgt(bear)), "any creature");
    g.choose(g.tgt(giant));
    g.drive_until(|g| at_priority(g) && g.stack().is_empty());
    assert_eq!((g.power(giant), g.toughness(giant)), (2, 1), "base 1/1, +1/+0\n{}", log(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(g.blockers_of(giant).is_empty());
    assert_eq!(g.life(1), 18, "2 unblocked damage\n{}", log(&g));
    next_main(&mut g, 1);
    assert_eq!((g.power(giant), g.toughness(giant)), (6, 5), "until end of turn");
}

/// "Can't be blocked": the opponent's Bear, offered as a blocker, can't
/// block the target attacker (CR 509.1b).
#[test]
fn atomic_microsizers_target_cant_be_blocked_cr509_1b() {
    let mut g = scen()
        .battlefield(0, &["Atomic Microsizer", "Cross Giant", "Island", "Island"])
        .battlefield(1, &["Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    let (eq, giant) = (bf(&g, 0, "Atomic Microsizer"), bf(&g, 0, "Cross Giant"));
    g.choose(act(eq, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(giant));
    pay(&mut g);
    settle(&mut g);
    attack_with(&mut g, 0, &["Cross Giant"]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(giant));
    for _ in 0..50 {
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step == Step::Main2);
        if g.kind() != Some(DecisionKind::DeclareBlockers) {
            break;
        }
        assert!(!g.has_option(Choice::Card(giant)), "can't be blocked: {:?}", g.options_desc());
        g.choose(Choice::Pass);
    }
    assert_eq!(g.life(1), 18, "{}", log(&g));
}

// ---- Larval Scoutlander -------------------------------------------------------------------

/// Larval Scoutlander: "When this Spacecraft enters, you may sacrifice a land
/// or Lander. If you do, search your library for up to two basic land cards,
/// put them onto the battlefield tapped, then shuffle" (CR 701.21a, 701.23):
/// one search, nonbasic lands not offered.
#[test]
fn larval_scoutlander_sacrifices_a_land_for_two_basics_tapped_cr701_23() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Cross Trinket"])
        .hand(0, &["Larval Scoutlander"])
        .library(0, &["Plains", "Island", "Lotus Field", "Swamp"])
        .at(Step::Main1, 0)
        .start();
    let trinket = bf(&g, 0, "Cross Trinket");
    let lib = g.player(0).library.len();
    cast(&mut g, 0, "Larval Scoutlander", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    assert!(!g.has_option(Choice::Card(trinket)), "a land or Lander");
    assert!(g.has_option(Choice::Pass), "may");
    let forest = g.lands_of(0).next().unwrap();
    g.choose(Choice::Card(forest));
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    let (plains, island, lotus) = (db().id("Plains").unwrap(), db().id("Island").unwrap(), db().id("Lotus Field").unwrap());
    assert!(!g.has_option(Choice::Def(lotus)), "a basic land card");
    g.choose(Choice::Def(island));
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "up to two");
    g.choose(Choice::Def(plains));
    settle(&mut g);
    let (p, i) = (g.card(0, "Plains", Zone::Battlefield), g.card(0, "Island", Zone::Battlefield));
    assert!(g.obj(p).tapped && g.obj(i).tapped, "{}", log(&g));
    assert_eq!(g.lands_of(0).count(), 4, "three Forests - one + two basics");
    assert_eq!(g.player(0).library.len(), lib - 2);
}

/// The "may": declined, nothing is sacrificed or searched; a Lander may be
/// sacrificed instead of a land, and the search may find fewer (CR 701.23b).
#[test]
fn larval_scoutlander_may_decline_or_sacrifice_a_lander_cr701_23b() {
    let mut g = scen().battlefield(0, &["Forest", "Forest", "Forest"]).hand(0, &["Larval Scoutlander"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Larval Scoutlander", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.lands_of(0).count(), 3, "{}", log(&g));
    assert_eq!(g.player(0).library.len(), 20);

    // Two Landers first (Terrapact Intimidator, the opponent says yes).
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Forest", "Forest", "Forest"])
        .hand(0, &["Terrapact Intimidator", "Larval Scoutlander"])
        .library(0, &["Plains", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Terrapact Intimidator", &[]);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(landers(&g, 0), 2);
    let lib = g.player(0).library.len();
    cast(&mut g, 0, "Larval Scoutlander", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    let lander = g.permanents_of(0).find(|&c| g.name(c) == "Lander").unwrap();
    assert!(g.has_option(Choice::Card(lander)), "{:?}", g.options_desc());
    g.choose(Choice::Card(lander));
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    g.choose(Choice::Def(db().id("Plains").unwrap()));
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(landers(&g, 0), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Plains", Zone::Battlefield), 1);
    assert_eq!(g.player(0).library.len(), lib - 1, "one card found");
}

/// "Station. 7+ | Flying" [3/3] (CR 702.184a, 722.2b).
#[test]
fn larval_scoutlander_station_7_is_a_3_3_flyer_cr722_2b() {
    let g = scen()
        .battlefield(0, &["Larval Scoutlander"])
        .counters(0, "Larval Scoutlander", charge(), 6)
        .at(Step::Main1, 0)
        .start();
    assert!(!is_creature(&g, bf(&g, 0, "Larval Scoutlander")));
    let mut g = scen().battlefield(0, &["Larval Scoutlander", "Cross Giant", "Cross Bear"]).at(Step::Main1, 0).start();
    let (ship, giant, bear) = (bf(&g, 0, "Larval Scoutlander"), bf(&g, 0, "Cross Giant"), bf(&g, 0, "Cross Bear"));
    station(&mut g, ship, giant);
    station(&mut g, ship, bear);
    assert_eq!(g.counters_on(ship, charge()), 7);
    assert!(is_creature(&g, ship) && g.has_kw(ship, Kw::Flying));
    assert_eq!((g.power(ship), g.toughness(ship)), (3, 3));
}

// ---- Alpharael, Dreaming Acolyte ----------------------------------------------------------------

/// Alpharael, Dreaming Acolyte: "When Alpharael enters, draw two cards. Then
/// discard two cards unless you discard an artifact card" (CR 701.9a: the
/// controller chooses). Discarding the artifact card ends it.
#[test]
fn alpharael_draws_two_then_discards_an_artifact_card_cr701_9a() {
    let mut g = scen()
        .battlefield(0, &["Island", "Swamp", "Swamp"])
        .hand(0, &["Alpharael, Dreaming Acolyte", "Cross Trinket", "Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Alpharael, Dreaming Acolyte", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    let (trinket, bear) = (g.card(0, "Cross Trinket", Zone::Hand), g.card(0, "Cross Bear", Zone::Hand));
    assert_eq!(g.player(0).hand.len(), 4, "drew two");
    assert!(g.has_option(Choice::Card(trinket)) && g.has_option(Choice::Pass), "{:?}", g.options_desc());
    assert!(!g.has_option(Choice::Card(bear)), "an artifact card");
    g.choose(Choice::Card(trinket));
    settle(&mut g);
    assert_eq!(g.obj(trinket).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.player(0).hand.len(), 3, "one card discarded");
}

/// Not discarding an artifact card (declining, or with none in hand): two
/// cards are discarded, any two (an artifact card among them too).
#[test]
fn alpharael_without_an_artifact_discard_discards_two_cr701_9a() {
    let mut g = scen()
        .battlefield(0, &["Island", "Swamp", "Swamp"])
        .hand(0, &["Alpharael, Dreaming Acolyte", "Cross Trinket", "Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Alpharael, Dreaming Acolyte", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    g.choose(Choice::Pass);
    let trinket = g.card(0, "Cross Trinket", Zone::Hand);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    assert!(g.has_option(Choice::Card(trinket)), "any card now");
    g.choose(Choice::Card(trinket));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    let bear = g.card(0, "Cross Bear", Zone::Hand);
    g.choose(Choice::Card(bear));
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 2, "{}", log(&g));
    // No artifact card: no choice, two discards.
    let mut g = scen()
        .battlefield(0, &["Island", "Swamp", "Swamp"])
        .hand(0, &["Alpharael, Dreaming Acolyte", "Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Alpharael, Dreaming Acolyte", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    assert!(!g.has_option(Choice::Pass), "must discard: {:?}", g.options_desc());
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1, "3 - 2\n{}", log(&g));
    assert_eq!(g.player(0).graveyard.len(), 2);
}

/// "During your turn, Alpharael has deathtouch" (CR 702.2, 611.3a) [2/3].
#[test]
fn alpharael_has_deathtouch_during_your_turn_cr702_2() {
    let mut g = scen().battlefield(0, &["Alpharael, Dreaming Acolyte"]).at(Step::Main1, 0).start();
    let a = bf(&g, 0, "Alpharael, Dreaming Acolyte");
    assert_eq!((g.power(a), g.toughness(a)), (2, 3));
    assert!(g.has_kw(a, Kw::Deathtouch));
    next_main(&mut g, 1);
    assert!(!g.has_kw(a, Kw::Deathtouch), "the opponent's turn");
}
