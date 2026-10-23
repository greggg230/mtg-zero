//! M7b part 2 (ENGINE.md 25): cards with more than one face, part 2: modal
//! double-faced cards (CR 712.3, 712.11-12: Shatterskull Smashing, Sink into
//! Stupor, Witch Enchanter, Jennifer Walters' back face) and Adventures (CR
//! 715: Brazen Borrower, Embereth Shieldbreaker); each face against its
//! Oracle text, and the hidden-information side of choosing a face.

mod common;

use common::db;
use engine::db::{AbilitySlot, Kw};
use engine::decision::{FACE_CAST, FACE_PLAY, OTHER_CAST, OTHER_PLAY};
use engine::hidden::{key_kind, KEY_REVEAL};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

fn pay(g: &mut Game) {
    g.drive_until(|g| {
        at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
    });
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| {
            at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
        });
    }
}

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

/// The priority option of card `c` (whatever face it offers).
fn option_of(g: &Game, c: CardIdx) -> Option<Choice> {
    g.pending_decision()?.options.iter().map(|o| o.choice).find(|ch| match *ch {
        Choice::Cast(x) | Choice::Play(x) | Choice::CastFace(x) | Choice::PlayFace(x) | Choice::Faces(x, _) => x == c,
        _ => false,
    })
}

/// Choose card `c` at priority and, if asked, face `face` (0 = front).
fn choose_face(g: &mut Game, c: CardIdx, face: u8) {
    let ch = option_of(g, c).unwrap_or_else(|| panic!("no option for the card: {:?}", g.options_desc()));
    g.choose(ch);
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(face));
    }
}

// ---- Shatterskull Smashing // Shatterskull, the Hammer Pass -------------------

const SHATTERSKULL: &str = "Shatterskull Smashing // Shatterskull, the Hammer Pass";
const HAMMER_PASS: &str = "Shatterskull, the Hammer Pass";

#[test]
fn shatterskull_offers_both_faces_and_its_land_face_is_a_land_drop_cr_712_12() {
    let mut g = scen().battlefield(0, &["Mountain", "Mountain"]).hand(0, &[SHATTERSKULL]).start();
    let s = g.card(0, SHATTERSKULL, Zone::Hand);
    // In hand it is the sorcery (CR 712.8a): mana value 2 ({X}{R}{R}).
    assert!(g.def_of(s).is(engine::db::types::SORCERY));
    assert_eq!(g.def_of(s).mana_value(), 2);
    assert_eq!(option_of(&g, s), Some(Choice::Faces(s, FACE_CAST | OTHER_PLAY)));
    choose_face(&mut g, s, 1);
    // "As this land enters, you may pay 3 life. If you don't, it enters
    // tapped."
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    let land = bf(&g, 0, HAMMER_PASS);
    assert_eq!(land, s);
    assert!(!g.obj(land).tapped);
    assert_eq!(g.life(0), 17);
    assert_eq!(g.player(0).lands_played, 1);
    assert!(g.is_type(land, engine::db::types::LAND));
    assert_eq!(g.def_of(land).mana_value(), 0, "its own (no) mana cost: CR 712.8f");
    // "{T}: Add {R}."
    assert_eq!(g.def_of(land).activated.len(), 1);
}

#[test]
fn shatterskull_land_face_enters_tapped_unpaid_and_needs_a_land_drop() {
    let mut g = scen().hand(0, &[SHATTERSKULL, "Mountain"]).start();
    let s = g.card(0, SHATTERSKULL, Zone::Hand);
    // No mana: only the land face.
    assert_eq!(option_of(&g, s), Some(Choice::PlayFace(s)));
    g.choose(Choice::PlayFace(s));
    g.choose(Choice::Pass);
    assert!(g.obj(bf(&g, 0, HAMMER_PASS)).tapped);
    assert_eq!(g.life(0), 20);
    // The land drop is used: the Mountain can't be played.
    let m = g.card(0, "Mountain", Zone::Hand);
    assert_eq!(option_of(&g, m), None);
}

#[test]
fn shatterskull_smashing_divides_x_damage_and_doubles_at_six() {
    // "Shatterskull Smashing deals X damage divided as you choose among up
    // to two target creatures and/or planeswalkers. If X is 6 or more, it
    // deals twice X damage divided as you choose among them instead."
    let mut g = scen()
        .battlefield(0, &["Mountain"; 5])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .hand(0, &[SHATTERSKULL])
        .lands_played(0, 1)
        .start();
    let s = g.card(0, SHATTERSKULL, Zone::Hand);
    let (bear, giant) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Giant"));
    choose_face(&mut g, s, 0);
    // X (CR 601.2b): up to 3 with five Mountains.
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode));
    assert!(g.has_option(Choice::Number(3)) && !g.has_option(Choice::Number(4)));
    g.choose(Choice::Number(3));
    g.choose(g.tgt(bear));
    g.choose(g.tgt(giant));
    // CR 601.2d: 1 or 2 to the first, the rest to the second.
    assert_eq!(g.kind(), Some(DecisionKind::DivideDamage));
    g.choose(Choice::Number(2));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(giant).damage, 1);
    // The card goes to the graveyard as the sorcery (front face).
    assert_eq!(g.count_in(0, SHATTERSKULL, Zone::Graveyard), 1);

    // X = 6: twelve damage.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 8])
        .battlefield(1, &["Test Giant", "Test Giant"])
        .hand(0, &[SHATTERSKULL])
        .lands_played(0, 1)
        .start();
    let s = g.card(0, SHATTERSKULL, Zone::Hand);
    choose_face(&mut g, s, 0);
    g.choose(Choice::Number(6));
    let giants: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&c| g.obj(c).controller == 1).collect();
    g.choose(g.tgt(giants[0]));
    g.choose(g.tgt(giants[1]));
    assert!(g.has_option(Choice::Number(11)));
    g.choose(Choice::Number(6));
    pay(&mut g);
    settle(&mut g);
    assert!(giants.iter().all(|&c| g.obj(c).zone == Zone::Graveyard));
}

#[test]
fn shatterskull_smashing_with_x_zero_has_no_targets() {
    let mut g = scen().battlefield(0, &["Mountain"; 2]).battlefield(1, &["Test Bear"]).hand(0, &[SHATTERSKULL]).lands_played(0, 1).start();
    let s = g.card(0, SHATTERSKULL, Zone::Hand);
    choose_face(&mut g, s, 0);
    g.choose(Choice::Number(0));
    // Each target needs at least 1 damage (CR 601.2d): none can be chosen.
    pay(&mut g);
    assert!(g.stack().len() == 1 && g.stack()[0].targets.is_empty());
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1);
}

// ---- Sink into Stupor // Soporific Springs -----------------------------------

const SINK: &str = "Sink into Stupor // Soporific Springs";

#[test]
fn sink_into_stupor_bounces_a_spell_or_an_opponents_nonland_permanent() {
    // "Return target spell or nonland permanent an opponent controls to its
    // owner's hand."
    let mut g = scen()
        .battlefield(0, &["Island"; 3])
        .battlefield(0, &["Test Bear"])
        .battlefield(1, &["Mountain", "Test Reacher"])
        .hand(0, &[SINK])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(0));
    pay(&mut g);
    g.pass();
    let sink = g.card(0, SINK, Zone::Hand);
    // An instant: castable in P1's turn (the land face isn't playable).
    assert_eq!(option_of(&g, sink), Some(Choice::Cast(sink)));
    g.choose(Choice::Cast(sink));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let reacher = bf(&g, 1, "Test Reacher");
    let mountain = bf(&g, 1, "Mountain");
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.has_option(g.tgt(bolt)) && g.has_option(g.tgt(reacher)));
    assert!(!g.has_option(g.tgt(mountain)) && !g.has_option(g.tgt(bear)), "nonland, an opponent's");
    g.choose(g.tgt(bolt));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Hand);
    assert_eq!(g.life(0), 20);
}

#[test]
fn soporific_springs_is_a_blue_land() {
    let mut g = scen().hand(0, &[SINK]).start();
    let s = g.card(0, SINK, Zone::Hand);
    g.choose(Choice::PlayFace(s));
    g.choose(Choice::Pass);
    let land = bf(&g, 0, "Soporific Springs");
    assert!(g.obj(land).tapped);
    assert!(g.def_of(land).activated[0].mana().is_some());
}

// ---- Witch Enchanter // Witch-Blessed Meadow ---------------------------------

const WITCH: &str = "Witch Enchanter // Witch-Blessed Meadow";

#[test]
fn witch_enchanter_destroys_an_opponents_artifact_or_enchantment() {
    // "When this creature enters, destroy target artifact or enchantment an
    // opponent controls."
    let mut g = scen()
        .battlefield(0, &["Plains"; 4])
        .battlefield(0, &["Sol Ring"])
        .battlefield(1, &["Mind Stone"])
        .hand(0, &[WITCH])
        .lands_played(0, 1)
        .start();
    let w = g.card(0, WITCH, Zone::Hand);
    assert_eq!(option_of(&g, w), Some(Choice::Cast(w)), "no land drop left: the creature only");
    g.choose(Choice::Cast(w));
    pay(&mut g);
    let (ring, stone) = (bf(&g, 0, "Sol Ring"), bf(&g, 1, "Mind Stone"));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            assert!(!g.has_option(g.tgt(ring)));
            Some(g.tgt(stone))
        }
        _ => None,
    });
    assert_eq!(g.obj(stone).zone, Zone::Graveyard);
    assert_eq!(g.pt(bf(&g, 0, WITCH)), (2, 2));
}

#[test]
fn witch_blessed_meadow_pays_three_life_or_enters_tapped() {
    let mut g = scen().battlefield(0, &["Plains"; 4]).hand(0, &[WITCH]).start();
    let w = g.card(0, WITCH, Zone::Hand);
    assert_eq!(option_of(&g, w), Some(Choice::Faces(w, FACE_CAST | OTHER_PLAY)));
    choose_face(&mut g, w, 1);
    g.choose(Choice::Yes);
    let m = bf(&g, 0, "Witch-Blessed Meadow");
    assert!(!g.obj(m).tapped);
    assert_eq!(g.life(0), 17);
    // At 2 life only "no" (CR 119.4).
    let mut g = scen().life(0, 2).hand(0, &[WITCH]).start();
    let w = g.card(0, WITCH, Zone::Hand);
    g.choose(Choice::PlayFace(w));
    assert!(!g.has_option(Choice::Yes));
}

#[test]
fn an_mdfc_land_face_goes_to_the_graveyard_as_its_front_face_cr_712_8a() {
    let mut g = scen().hand(0, &[WITCH]).battlefield(1, &["Wasteland"]).start();
    let w = g.card(0, WITCH, Zone::Hand);
    g.choose(Choice::PlayFace(w));
    g.choose(Choice::Pass);
    let land = bf(&g, 0, "Witch-Blessed Meadow");
    assert!(db().is_face(g.obj(land).def));
    g.pass();
    // "{T}, Sacrifice Wasteland: Destroy target nonbasic land."
    let waste = bf(&g, 1, "Wasteland");
    g.choose(Choice::Ability(waste, AbilitySlot::Activated(1)));
    g.choose(g.tgt(land));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(land).zone, Zone::Graveyard);
    assert_eq!(g.name(land), WITCH);
    assert!(!db().is_face(g.obj(land).def));
}

// ---- Jennifer Walters // The Sensational She-Hulk (back face cast) -----------

const JENNIFER: &str = "Jennifer Walters // The Sensational She-Hulk";
const SHE_HULK: &str = "The Sensational She-Hulk";

#[test]
fn the_sensational_she_hulk_can_be_cast_from_hand_cr_712_11b() {
    let mut g = scen().battlefield(0, &["Plains", "Plains", "Forest", "Forest", "Forest", "Forest"]).hand(0, &[JENNIFER]).start();
    let j = g.card(0, JENNIFER, Zone::Hand);
    assert_eq!(option_of(&g, j), Some(Choice::Faces(j, FACE_CAST | OTHER_CAST)));
    choose_face(&mut g, j, 1);
    // On the stack: only the back face's characteristics (CR 712.8f).
    assert_eq!(g.name(j), SHE_HULK);
    pay(&mut g);
    assert!(g.player(0).pool.amt.iter().all(|&n| n == 0));
    assert_eq!(g.battlefield().iter().filter(|&&c| g.obj(c).tapped).count(), 6, "{{3}}{{G}}{{W}}{{W}}");
    settle(&mut g);
    let s = bf(&g, 0, SHE_HULK);
    assert_eq!(g.pt(s), (6, 6));
    assert_eq!(g.def_of(s).mana_value(), 6);
}

#[test]
fn she_hulk_redirects_damage_only_once_each_turn_cr_603_2h() {
    // "Whenever a creature you control is dealt damage, you may have The
    // Sensational She-Hulk deal that much damage to any target. Do this only
    // once each turn."
    let mut g = scen()
        .battlefield(0, &[JENNIFER, "Test Giant", "Test Bear"])
        .transformed(0, JENNIFER)
        .battlefield(1, &["Mountain"; 3])
        .hand(1, &["Lightning Bolt"; 3])
        .at(Step::Main1, 1)
        .start();
    let (asked, targeted) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
    // Declined the first time, done the second; the third time it doesn't
    // trigger (CR 603.2h: no target to choose either).
    for (k, victim) in ["Test Giant", SHE_HULK, "Test Bear"].into_iter().enumerate() {
        let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
        g.choose(Choice::Cast(bolt));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        let v = bf(&g, 0, victim);
        g.choose(g.tgt(v));
        pay(&mut g);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => {
                targeted.set(targeted.get() + 1);
                Some(player(1))
            }
            Some(DecisionKind::YesNo) => {
                asked.set(asked.get() + 1);
                Some(if k == 0 { Choice::Pass } else { Choice::Yes })
            }
            _ => None,
        });
    }
    assert_eq!((asked.get(), targeted.get()), (2, 2));
    assert_eq!(g.life(1), 17);
    // A new turn: again.
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
}

// ---- Brazen Borrower // Petty Theft --------------------------------------------

const BORROWER: &str = "Brazen Borrower // Petty Theft";

#[test]
fn brazen_borrower_goes_on_an_adventure_then_is_cast_from_exile_cr_715_3d() {
    let mut g = scen()
        .battlefield(0, &["Island"; 5])
        .battlefield(1, &["Test Bear", "Forest"])
        .hand(0, &[BORROWER])
        .at(Step::Upkeep, 1)
        .start();
    g.pass(); // P0 has priority in P1's upkeep
    let b = g.card(0, BORROWER, Zone::Hand);
    // In hand it is the creature (CR 715.4): flash, so both faces.
    assert!(g.def_of(b).is(engine::db::types::CREATURE));
    assert_eq!(g.def_of(b).mana_value(), 3);
    assert_eq!(option_of(&g, b), Some(Choice::Faces(b, FACE_CAST | OTHER_CAST)));
    choose_face(&mut g, b, 1);
    // As an Adventure: "Petty Theft", an instant, mana value 2 (715.3b).
    assert_eq!(g.name(b), "Petty Theft");
    assert!(g.def_of(b).is(engine::db::types::INSTANT));
    let bear = bf(&g, 1, "Test Bear");
    let forest = bf(&g, 1, "Forest");
    assert!(!g.has_option(g.tgt(forest)), "nonland");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Hand);
    // Exiled on an adventure, as the creature card.
    assert_eq!(g.obj(b).zone, Zone::Exile);
    assert_eq!(g.name(b), BORROWER);
    // Castable from exile as the creature only (flash: still P1's turn).
    g.pass_until(|g| g.turn().priority == 0);
    assert_eq!(option_of(&g, b), Some(Choice::Cast(b)));
    g.choose(Choice::Cast(b));
    pay(&mut g);
    settle(&mut g);
    let bb = bf(&g, 0, BORROWER);
    assert_eq!(g.pt(bb), (3, 1));
    assert!(g.has_kw(bb, Kw::Flash) && g.has_kw(bb, Kw::Flying));
}

#[test]
fn a_countered_adventure_goes_to_the_graveyard() {
    // CR 715.3d is about resolving: a countered Petty Theft is put into its
    // owner's graveyard, with no permission.
    let mut g = scen()
        .battlefield(0, &["Island"; 2])
        .battlefield(1, &["Test Bear", "Island", "Island"])
        .hand(0, &[BORROWER])
        .hand(1, &["Counterspell"])
        .lands_played(0, 1)
        .start();
    let b = g.card(0, BORROWER, Zone::Hand);
    choose_face(&mut g, b, 1);
    g.choose(g.tgt(bf(&g, 1, "Test Bear")));
    pay(&mut g);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(b));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(b).zone, Zone::Graveyard);
    assert_eq!(g.name(b), BORROWER, "the card, not the Adventure (CR 715.4)");
    assert_eq!(option_of(&g, b), None);
}

#[test]
fn an_adventure_whose_target_is_gone_goes_to_the_graveyard_cr_608_2b() {
    let mut g = scen()
        .battlefield(0, &["Island"; 2])
        .battlefield(1, &["Test Bear", "Mountain"])
        .hand(0, &[BORROWER])
        .hand(1, &["Lightning Bolt"])
        .lands_played(0, 1)
        .start();
    let b = g.card(0, BORROWER, Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    choose_face(&mut g, b, 1);
    g.choose(g.tgt(bear));
    pay(&mut g);
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(b).zone, Zone::Graveyard, "it didn't resolve: no adventure");
}

#[test]
fn brazen_borrower_can_block_only_flyers() {
    let mut g = scen()
        .battlefield(0, &["Test Bear", "Test Flyer"])
        .battlefield(1, &[BORROWER])
        .at(Step::Main1, 0)
        .start();
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    let (bear, flyer) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Flyer"));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Card(flyer));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    assert!(g.has_option(Choice::Card(flyer)) && !g.has_option(Choice::Card(bear)));
}

// ---- Embereth Shieldbreaker // Battle Display ----------------------------------

const SHIELDBREAKER: &str = "Embereth Shieldbreaker // Battle Display";

#[test]
fn embereth_shieldbreaker_battle_display_destroys_an_artifact_at_sorcery_speed() {
    // Battle Display: "Destroy target artifact." (a sorcery Adventure)
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Sol Ring"])
        .hand(0, &[SHIELDBREAKER])
        .lands_played(0, 1)
        .start();
    let s = g.card(0, SHIELDBREAKER, Zone::Hand);
    assert_eq!(option_of(&g, s), Some(Choice::Faces(s, FACE_CAST | OTHER_CAST)));
    choose_face(&mut g, s, 1);
    assert!(g.def_of(s).is(engine::db::types::SORCERY));
    g.choose(g.tgt(bf(&g, 1, "Sol Ring")));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Graveyard), 1);
    assert_eq!(g.obj(s).zone, Zone::Exile);
    // Then the 2/1 from exile.
    assert_eq!(option_of(&g, s), Some(Choice::Cast(s)));
    g.choose(Choice::Cast(s));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.pt(bf(&g, 0, SHIELDBREAKER)), (2, 1));
    // Not in the opponent's turn (a sorcery Adventure, a creature).
    let mut g = scen().battlefield(0, &["Mountain"; 3]).battlefield(1, &["Sol Ring"]).hand(0, &[SHIELDBREAKER]).at(Step::Upkeep, 1).start();
    g.pass();
    let s = g.card(0, SHIELDBREAKER, Zone::Hand);
    assert_eq!(option_of(&g, s), None);
}

// ---- hidden information ---------------------------------------------------------

#[test]
fn choosing_a_face_reveals_the_card_and_survives_determinize() {
    let mut g = scen().battlefield(0, &["Mountain", "Mountain"]).hand(0, &[SHATTERSKULL, "Island", "Forest"]).start();
    let s = g.card(0, SHATTERSKULL, Zone::Hand);
    assert!(!g.knows(1, s));
    let keys = g.choose(option_of(&g, s).unwrap());
    assert_eq!(g.kind(), Some(DecisionKind::ChooseFace));
    assert!(g.knows(1, s), "public either way: revealed (ENGINE.md 10)");
    assert!(keys.iter().any(|&k| key_kind(k) == KEY_REVEAL));
    // The other player's determinize keeps the decision and the card.
    let before = g.pending_decision().cloned();
    for seed in 0..20 {
        let mut w = g.clone();
        w.determinize(1, &mut engine::rng::Rng::new(seed));
        assert_eq!(w.pending_decision().cloned(), before);
        assert_eq!(w.obj(s).def, g.obj(s).def);
    }
    g.choose(Choice::Number(1));
    g.choose(Choice::Pass);
    assert_eq!(g.count_in(0, HAMMER_PASS, Zone::Battlefield), 1);
}

#[test]
fn a_card_on_an_adventure_is_public_and_its_hand_twin_is_not_cr_715() {
    let mut g = scen().battlefield(0, &["Mountain"]).battlefield(1, &["Sol Ring"]).hand(0, &[SHIELDBREAKER, SHIELDBREAKER]).lands_played(0, 1).start();
    let s = g.card(0, SHIELDBREAKER, Zone::Hand);
    g.choose(option_of(&g, s).unwrap());
    g.choose(g.tgt(bf(&g, 1, "Sol Ring")));
    pay(&mut g);
    settle(&mut g);
    let exiled: Vec<CardIdx> = g.exile_zone().to_vec();
    assert_eq!(exiled.len(), 1);
    let twin = g.player(0).hand[0];
    assert!(!g.knows(1, twin));
    for seed in 0..10 {
        let mut w = g.clone();
        w.determinize(1, &mut engine::rng::Rng::new(seed));
        assert_eq!(w.obj(exiled[0]).def, g.obj(exiled[0]).def);
        assert_eq!(w.view_of(1), g.view_of(1));
    }
}

#[test]
fn faces_are_never_up_in_hidden_zones_cr_712_8a() {
    // A transformed permanent bounced to hand is its front face there.
    let mut g = scen()
        .battlefield(1, &["Ulvenwald Oddity // Ulvenwald Behemoth"])
        .transformed(1, "Ulvenwald Oddity // Ulvenwald Behemoth")
        .battlefield(0, &["Island"; 3])
        .hand(0, &[SINK])
        .start();
    let b = bf(&g, 1, "Ulvenwald Behemoth");
    let sink = g.card(0, SINK, Zone::Hand);
    choose_face(&mut g, sink, 0);
    g.choose(g.tgt(b));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(b).zone, Zone::Hand);
    assert_eq!(g.name(b), "Ulvenwald Oddity // Ulvenwald Behemoth");
    assert!(!db().is_face(g.obj(b).def));
    assert!(g.check_invariants().is_ok());
    let _ = (AbilitySlot::Spell, FACE_PLAY);
}
