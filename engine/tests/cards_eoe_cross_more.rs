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

// ---- Full Bore -----------------------------------------------------------------------------

/// Full Bore: "Target creature you control gets +3/+2 until end of turn. If
/// that creature was cast for its warp cost, it also gains trample and haste
/// until end of turn" (CR 702.185a): a creature cast for its mana cost gets
/// only the +3/+2; an opponent's creature isn't a legal target.
#[test]
fn full_bore_pumps_target_creature_you_control_cr702_185a() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Cross Bear"])
        .battlefield(1, &["Cross Giant"])
        .hand(0, &["Full Bore"])
        .at(Step::Main1, 0)
        .start();
    let (bear, giant) = (bf(&g, 0, "Cross Bear"), bf(&g, 1, "Cross Giant"));
    let c = g.card(0, "Full Bore", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(giant)), "a creature you control");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!((g.power(bear), g.toughness(bear)), (5, 4), "{}", log(&g));
    assert!(!g.has_kw(bear, Kw::Trample) && !g.has_kw(bear, Kw::Haste), "not cast for its warp cost");
    next_main(&mut g, 1);
    assert_eq!((g.power(bear), g.toughness(bear)), (2, 2), "until end of turn");
}

/// A creature cast for its warp cost (Broodguard Elite, "Warp {X}{G}", X =
/// 1) gets trample and haste too, and can attack the turn it entered (CR
/// 702.19, 702.10, 302.6).
#[test]
fn full_bore_on_a_warped_creature_adds_trample_and_haste_cr702_185a() {
    let mut g = scen().battlefield(0, &["Forest", "Forest", "Mountain"]).hand(0, &["Broodguard Elite", "Full Bore"]).at(Step::Main1, 0).start();
    let elite = warp_broodguard(&mut g);
    assert!(g.is_sick(elite));
    let target = g.tgt(elite);
    cast(&mut g, 0, "Full Bore", &[target]);
    settle(&mut g);
    assert_eq!((g.power(elite), g.toughness(elite)), (4, 3), "{}", log(&g));
    assert!(g.has_kw(elite, Kw::Trample) && g.has_kw(elite, Kw::Haste));
    attack_with(&mut g, 0, &["Broodguard Elite"]);
    assert!(g.obj(elite).attacking, "haste: attacks the turn it entered");
}

/// Cast Broodguard Elite from P0's hand for its warp cost with X = 1 and
/// resolve it.
fn warp_broodguard(g: &mut Game) -> engine::CardIdx {
    let c = g.card(0, "Broodguard Elite", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    g.choose(Choice::Number(1));
    pay(g);
    settle(g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield, "{}", log(g));
    c
}

/// Exiled by warp at the end step and cast from exile on a later turn for
/// its mana cost, it is a new object that wasn't cast for its warp cost (CR
/// 400.7, 702.185a): Full Bore gives it +3/+2 only.
#[test]
fn full_bore_a_warped_card_cast_again_from_exile_isnt_warped_cr400_7() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Mountain"])
        .hand(0, &["Broodguard Elite", "Full Bore"])
        .at(Step::Main1, 0)
        .start();
    let elite = warp_broodguard(&mut g);
    next_main(&mut g, 0);
    assert_eq!(g.obj(elite).zone, Zone::Exile, "{}", log(&g));
    assert!(g.has_option(Choice::Cast(elite)), "castable from exile: {:?}", g.options_desc());
    g.choose(Choice::Cast(elite));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        assert!(!g.has_option(Choice::Number(1)), "warp only from hand: {:?}", g.options_desc());
        g.choose(Choice::Number(0));
        g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    }
    g.choose(Choice::Number(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(elite).zone, Zone::Battlefield, "{}", log(&g));
    let target = g.tgt(elite);
    cast(&mut g, 0, "Full Bore", &[target]);
    settle(&mut g);
    assert_eq!((g.power(elite), g.toughness(elite)), (4, 3), "{}", log(&g));
    assert!(!g.has_kw(elite, Kw::Trample) && !g.has_kw(elite, Kw::Haste), "cast for its mana cost this time");
}

// ---- The Seriema ---------------------------------------------------------------------------

/// The Seriema: "When The Seriema enters, search your library for a
/// legendary creature card, reveal it, put it into your hand, then shuffle"
/// (CR 701.23a, 205.4d).
#[test]
fn the_seriema_searches_for_a_legendary_creature_card_cr701_23a() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["The Seriema"])
        .library(0, &["Sliver Overlord", "Cross Bear", "The Seriema"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "The Seriema", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    let (overlord, bear, seriema) = (db().id("Sliver Overlord").unwrap(), db().id("Cross Bear").unwrap(), db().id("The Seriema").unwrap());
    assert!(g.has_option(Choice::Def(overlord)), "{:?}", g.options_desc());
    assert!(!g.has_option(Choice::Def(bear)), "not legendary");
    assert!(!g.has_option(Choice::Def(seriema)), "legendary, not a creature card");
    g.choose(Choice::Def(overlord));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Sliver Overlord", Zone::Hand), 1, "{}", log(&g));
}

/// "Other tapped legendary creatures you control have indestructible" (CR
/// 702.12b, 611.3a): only while tapped, only its controller's, not itself.
#[test]
fn the_seriema_gives_other_tapped_legendary_creatures_indestructible_cr702_12b() {
    let mut g = scen()
        .battlefield(0, &["The Seriema", "Sliver Overlord", "Cross Bear"])
        .counters(0, "The Seriema", charge(), 7)
        .battlefield(1, &["Alpharael, Dreaming Acolyte"])
        .at(Step::Main1, 0)
        .start();
    let (ship, overlord, bear, theirs) =
        (bf(&g, 0, "The Seriema"), bf(&g, 0, "Sliver Overlord"), bf(&g, 0, "Cross Bear"), bf(&g, 1, "Alpharael, Dreaming Acolyte"));
    assert!(!g.has_kw(overlord, Kw::Indestructible), "untapped");
    for c in [overlord, bear, theirs, ship] {
        g.set_tapped(c, true);
    }
    assert!(g.has_kw(overlord, Kw::Indestructible), "tapped, legendary, yours");
    assert!(!g.has_kw(bear, Kw::Indestructible), "not legendary");
    assert!(!g.has_kw(theirs, Kw::Indestructible), "an opponent's");
    assert!(!g.has_kw(ship, Kw::Indestructible), "other");
    assert!(is_creature(&g, ship) && g.has_kw(ship, Kw::Flying), "7+ | Flying [5/5]");
    assert_eq!((g.power(ship), g.toughness(ship)), (5, 5));
}

// ---- Infinite Guideline Station ------------------------------------------------------------

/// Infinite Guideline Station: "When Infinite Guideline Station enters,
/// create a tapped 2/2 colorless Robot artifact creature token for each
/// multicolored permanent you control" (CR 105.2a, 111.2): itself (five
/// colours) and Alpharael (blue and black), not the opponent's.
#[test]
fn infinite_guideline_station_makes_a_tapped_robot_per_multicolored_permanent_cr105_2a() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Swamp", "Mountain", "Forest", "Alpharael, Dreaming Acolyte", "Cross Bear"])
        .battlefield(1, &["Alpharael, Dreaming Acolyte"])
        .hand(0, &["Infinite Guideline Station"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Infinite Guideline Station", &[]);
    settle(&mut g);
    let robots: Vec<engine::CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Robot").collect();
    assert_eq!(robots.len(), 2, "{}", log(&g));
    for r in robots {
        assert!(g.obj(r).tapped, "tapped");
        assert_eq!((g.power(r), g.toughness(r)), (2, 2));
        assert_eq!(g.colors_of(r), 0, "colorless");
        assert!(g.types_of(r) & types::ARTIFACT != 0 && is_creature(&g, r));
    }
    assert_eq!(g.permanents_of(1).filter(|&c| g.name(c) == "Robot").count(), 0);
}

/// "Station. 12+ | Flying" [7/15]; "Whenever Infinite Guideline Station
/// attacks, draw a card for each multicolored permanent you control."
#[test]
fn infinite_guideline_station_attacks_and_draws_per_multicolored_permanent_cr722_2b() {
    let mut g = scen()
        .battlefield(0, &["Infinite Guideline Station", "Alpharael, Dreaming Acolyte"])
        .counters(0, "Infinite Guideline Station", charge(), 12)
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Infinite Guideline Station");
    assert!(is_creature(&g, ship) && g.has_kw(ship, Kw::Flying));
    assert_eq!((g.power(ship), g.toughness(ship)), (7, 15));
    let hand = g.player(0).hand.len();
    attack_with(&mut g, 0, &["Infinite Guideline Station"]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.player(0).hand.len(), hand + 2, "{}", log(&g));
    assert_eq!(g.life(1), 13);
}

// ---- Roving Actuator -----------------------------------------------------------------------

/// Kill P1's Bear with Cross Bolt (void: a nonland permanent left the
/// battlefield), then cast Roving Actuator; stop at its trigger's target.
fn actuator_with_void(extra_gy: &[&str]) -> Game {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Cross Bear", "Cross Giant"])
        .hand(0, &["Cross Bolt", "Roving Actuator"])
        .graveyard(0, extra_gy)
        .graveyard(1, &["Cross Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Cross Bear");
    let t = g.tgt(bear);
    cast(&mut g, 0, "Cross Bolt", &[t]);
    settle(&mut g);
    assert!(g.void_active());
    cast(&mut g, 0, "Roving Actuator", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g
}

/// Roving Actuator: "Void — When this creature enters, if a nonland
/// permanent left the battlefield this turn or a spell was warped this turn,
/// exile up to one target instant or sorcery card with mana value 2 or less
/// from your graveyard. Copy it. You may cast the copy without paying its
/// mana cost" (CR 707.12, 118.9): the copy is cast (its own target chosen),
/// the card stays exiled.
#[test]
fn roving_actuator_exiles_and_casts_a_copy_free_cr707_12() {
    let mut g = actuator_with_void(&[]);
    let bolt = g.card(0, "Cross Bolt", Zone::Graveyard);
    g.choose(g.tgt(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    assert_eq!(g.obj(bolt).zone, Zone::Exile, "exiled as it resolves\n{}", log(&g));
    assert!(g.has_option(Choice::Yes), "{:?}", g.options_desc());
    let lands = untapped(&g, 0);
    g.choose(Choice::Yes);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let giant = bf(&g, 1, "Cross Giant");
    g.choose(g.tgt(giant));
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Graveyard, "the copy resolved\n{}", log(&g));
    assert_eq!(untapped(&g, 0), lands, "without paying its mana cost");
    assert_eq!(g.obj(bolt).zone, Zone::Exile, "the card stays exiled");
    assert_eq!(g.count_in(0, "Cross Bolt", Zone::Exile), 1, "the copy ceased to exist (CR 707.10a)");
    assert_eq!(g.count_in(0, "Cross Bolt", Zone::Graveyard), 0);
}

/// "Up to one target instant or sorcery card with mana value 2 or less from
/// your graveyard": not a mana value 3 card, a creature card or an
/// opponent's card; no target is allowed.
#[test]
fn roving_actuator_targets_a_small_instant_or_sorcery_in_your_graveyard_cr115_1() {
    let g = actuator_with_void(&["Cross Blast", "Cross Bear"]);
    let (blast, bear, theirs) = (g.card(0, "Cross Blast", Zone::Graveyard), g.card(0, "Cross Bear", Zone::Graveyard), g.card(1, "Cross Bolt", Zone::Graveyard));
    let mine = g.card(0, "Cross Bolt", Zone::Graveyard);
    assert!(g.has_option(g.tgt(mine)), "{:?}", g.options_desc());
    assert!(!g.has_option(g.tgt(blast)), "mana value 3");
    assert!(!g.has_option(g.tgt(bear)), "a creature card");
    assert!(!g.has_option(g.tgt(theirs)), "your graveyard");
    assert!(g.has_option(Choice::Pass), "up to one");
}

/// Declined, the copy ceases to exist (CR 704.5e); the card stays exiled.
#[test]
fn roving_actuator_declined_copy_ceases_to_exist_cr704_5e() {
    let mut g = actuator_with_void(&[]);
    let bolt = g.card(0, "Cross Bolt", Zone::Graveyard);
    g.choose(g.tgt(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
    assert_eq!(g.count_in(0, "Cross Bolt", Zone::Exile), 1, "{}", log(&g));
    assert_eq!(g.count_in(1, "Cross Giant", Zone::Battlefield), 1);
}

/// Without void it doesn't trigger (CR 603.4).
#[test]
fn roving_actuator_without_void_doesnt_trigger_cr603_4() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Roving Actuator"])
        .graveyard(0, &["Cross Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Roving Actuator", &[]);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Cross Bolt", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Roving Actuator", Zone::Battlefield), 1);
}

// ---- Vote Out (convoke) --------------------------------------------------------------------

/// Vote Out ({3}{B}): "Convoke ... Destroy target creature" (CR 702.51a,
/// 701.8a): a Swamp pays {B}, three green creatures pay {3} (each a {1}),
/// summoning sickness doesn't matter (convoke isn't {T}: CR 302.6).
#[test]
fn vote_out_convokes_with_creatures_cr702_51a() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Swamp", "Cross Giant", "Cross Sliver"])
        .battlefield(1, &["Cross Giant"])
        .hand(0, &["Vote Out", "Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = cast(&mut g, 0, "Cross Bear", &[]);
    settle(&mut g);
    assert!(g.is_sick(bear));
    let c = g.card(0, "Vote Out", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{{3}}{{B}}: a Swamp and three creatures: {:?}", g.options_desc());
    let theirs = bf(&g, 1, "Cross Giant");
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(theirs));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(theirs).zone, Zone::Graveyard, "{}", log(&g));
    for n in ["Swamp", "Cross Bear", "Cross Giant", "Cross Sliver"] {
        assert!(g.obj(bf(&g, 0, n)).tapped, "{n} tapped to pay\n{}", log(&g));
    }
    assert!(log(&g).contains("for convoke"), "{}", log(&g));
}

/// A black creature pays the {B} (a creature of that colour, CR 702.51a);
/// green creatures can't, nor a colourless one.
#[test]
fn vote_out_a_creature_pays_a_pip_of_its_colour_cr702_51a() {
    let g = scen()
        .battlefield(0, &["Cross Bear", "Cross Giant", "Cross Sliver", "Cross Robot"])
        .hand(0, &["Vote Out"])
        .battlefield(1, &["Cross Giant"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Vote Out", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "no black source for {{B}}");
    let mut g = scen()
        .battlefield(0, &["Alpharael, Dreaming Acolyte", "Cross Giant", "Cross Sliver", "Cross Robot"])
        .hand(0, &["Vote Out"])
        .battlefield(1, &["Cross Giant"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Vote Out", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "Alpharael is black: {:?}", g.options_desc());
    let theirs = bf(&g, 1, "Cross Giant");
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(theirs));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(theirs).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.permanents_of(0).filter(|&x| g.obj(x).tapped).count(), 4, "all four convoked");
}

/// Convoke is the spell's own: a spell without it can't be paid with
/// creatures (Embrace Oblivion's {B} with only creatures).
#[test]
fn convoke_only_on_a_spell_with_convoke_cr702_51a() {
    let g = scen()
        .battlefield(0, &["Alpharael, Dreaming Acolyte", "Cross Trinket"])
        .battlefield(1, &["Cross Giant"])
        .hand(0, &["Embrace Oblivion"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Embrace Oblivion", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "{:?}", g.options_desc());
}

// ---- Tapestry Warden -----------------------------------------------------------------------

/// Tapestry Warden [3/4]: "Vigilance / Each creature you control with
/// toughness greater than its power assigns combat damage equal to its
/// toughness rather than its power" (CR 510.1a): the Warden deals 4, the
/// 1/4 Wall 4, the 2/2 Bear 2; it doesn't tap to attack (702.20b).
#[test]
fn tapestry_warden_creatures_assign_combat_damage_by_toughness_cr510_1a() {
    let mut g = scen().battlefield(0, &["Tapestry Warden", "Cross Wall", "Cross Bear"]).at(Step::Main1, 0).start();
    let w = bf(&g, 0, "Tapestry Warden");
    assert_eq!((g.power(w), g.toughness(w)), (3, 4));
    assert!(g.has_kw(w, Kw::Vigilance));
    attack_with(&mut g, 0, &["Tapestry Warden", "Cross Wall", "Cross Bear"]);
    assert!(!g.obj(w).tapped, "vigilance");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 20 - 4 - 4 - 2, "{}", log(&g));
}

/// Only its controller's creatures: the opponent's blocking Wall deals 1
/// (its power); P0's blocking Wall deals 4 (CR 510.1c-d).
#[test]
fn tapestry_warden_only_your_creatures_and_blockers_too_cr510_1d() {
    let mut g = scen()
        .battlefield(0, &["Tapestry Warden", "Cross Giant"])
        .battlefield(1, &["Cross Wall"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 0, "Cross Giant");
    let theirs = bf(&g, 1, "Cross Wall");
    attack_with(&mut g, 0, &["Cross Giant"]);
    for _ in 0..20 {
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step == Step::Main2);
        if g.kind() != Some(DecisionKind::DeclareBlockers) {
            break;
        }
        let c = if g.obj(theirs).blocking.is_none() && g.has_option(Choice::Card(giant)) { Choice::Card(giant) } else { Choice::Pass };
        g.choose(c);
    }
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.objs()[giant.i()].damage, 1, "the opponent's Wall: its power\n{}", log(&g));
    // P0's Wall blocks P1's Giant: 4 damage to it.
    let mut g = scen()
        .battlefield(0, &["Tapestry Warden", "Cross Wall"])
        .battlefield(1, &["Cross Giant"])
        .at(Step::Main1, 1)
        .start();
    let (wall, giant) = (bf(&g, 0, "Cross Wall"), bf(&g, 1, "Cross Giant"));
    // (The Warden tapped: the Wall is the one blocker.)
    let w = bf(&g, 0, "Tapestry Warden");
    g.set_tapped(w, true);
    attack_with(&mut g, 1, &["Cross Giant"]);
    for _ in 0..20 {
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step == Step::Main2);
        if g.kind() != Some(DecisionKind::DeclareBlockers) {
            break;
        }
        let c = if g.obj(wall).blocking.is_none() && g.pending_decision().unwrap().player == 0 && g.has_option(Choice::Card(giant)) {
            Choice::Card(giant)
        } else {
            Choice::Pass
        };
        g.choose(c);
    }
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.objs()[giant.i()].damage, 4, "P0's Wall: its toughness\n{}", log(&g));
}

/// "Each creature you control with toughness greater than its power stations
/// permanents using its toughness rather than its power" (CR 702.184c): the
/// 1/4 Wall puts 4 charge counters; without the Warden, 1.
#[test]
fn tapestry_warden_creatures_station_by_toughness_cr702_184c() {
    for (warden, n) in [(true, 4), (false, 1)] {
        let mut bfv = vec!["Pinnacle Kill-Ship", "Cross Wall"];
        if warden {
            bfv.push("Tapestry Warden");
        }
        let mut g = scen().battlefield(0, &bfv).at(Step::Main1, 0).start();
        let (ship, wall) = (bf(&g, 0, "Pinnacle Kill-Ship"), bf(&g, 0, "Cross Wall"));
        station(&mut g, ship, wall);
        assert_eq!(g.counters_on(ship, charge()), n, "warden {warden}\n{}", log(&g));
    }
}

/// The tapped creature gone before the station ability resolves: its
/// toughness and power as it last existed (CR 608.2h, 702.184c).
#[test]
fn tapestry_warden_station_reads_last_known_toughness_cr608_2h() {
    let mut g = scen()
        .battlefield(0, &["Pinnacle Kill-Ship", "Cross Wall", "Tapestry Warden", "Mountain"])
        .hand(0, &["Cross Bolt"])
        .at(Step::Main1, 0)
        .start();
    let (ship, wall) = (bf(&g, 0, "Pinnacle Kill-Ship"), bf(&g, 0, "Cross Wall"));
    let s = station_slot(&g, ship);
    g.choose(act(ship, s));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
    g.choose(Choice::Card(wall));
    g.drive_until(|g| at_priority(g));
    let t = g.tgt(wall);
    cast(&mut g, 0, "Cross Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(wall).zone, Zone::Graveyard);
    assert_eq!(g.counters_on(ship, charge()), 4, "{}", log(&g));
}

/// The pending "cast the copy?" decision, determinized for either viewer
/// (ENGINE.md 10): the copy made in exile and everything else public are
/// the same, and the decider's actions too. (A refresh runs the decision's
/// task again; the exile and the copy were made once, fuzz seed 624 of
/// eoe-base 38d6a3b: the task exiled nothing the second time and the
/// ability ran on past the question.)
#[test]
fn roving_actuator_pending_copy_decision_survives_determinize_cr707_12() {
    let mut g = actuator_with_void(&[]);
    let bolt = g.card(0, "Cross Bolt", Zone::Graveyard);
    g.choose(g.tgt(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    let mover = g.to_move().unwrap();
    assert_eq!(mover, 0);
    let mut rng = engine::rng::Rng::new(624);
    for viewer in [0u8, 1] {
        let mut d = g.clone();
        d.determinize(viewer, &mut rng);
        assert_eq!(d.to_move(), Some(mover), "viewer P{viewer}: still the decision");
        assert_eq!(d.kind(), Some(DecisionKind::YesNo));
        assert_eq!(g.view_of(viewer), d.view_of(viewer), "viewer P{viewer}: the view\n{}", log(&d));
        let (mut a, mut b) = (Vec::new(), Vec::new());
        g.legal_actions(&mut a);
        d.legal_actions(&mut b);
        assert_eq!(a, b, "viewer P{viewer}: the decider's actions");
        assert_eq!(d.count_in(0, "Cross Bolt", Zone::Exile), 2, "the card and its copy, once");
        // The determinized world plays on: the copy is cast and resolves.
        d.choose(Choice::Yes);
        d.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        let giant = bf(&d, 1, "Cross Giant");
        d.choose(d.tgt(giant));
        settle(&mut d);
        assert_eq!(d.obj(giant).zone, Zone::Graveyard, "{}", log(&d));
        assert_eq!(d.count_in(0, "Cross Bolt", Zone::Exile), 1);
    }
}
