//! M7b part 2 (ENGINE.md 25): cards with more than one face, part 4: Norman
//! Osborn // Green Goblin (an MDFC that transforms; connive, mayhem), Life //
//! Death (a split card, CR 709), Ajani, Nacatl Pariah (its back face's
//! reflexive trigger, CR 603.12), and the encoding of faces under
//! determinize.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::decision::{FACE_CAST, OTHER_CAST};
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

fn option_of(g: &Game, c: CardIdx) -> Option<Choice> {
    g.pending_decision()?.options.iter().map(|o| o.choice).find(|ch| match *ch {
        Choice::Cast(x) | Choice::Play(x) | Choice::CastFace(x) | Choice::PlayFace(x) | Choice::Faces(x, _) => x == c,
        _ => false,
    })
}

fn choose_face(g: &mut Game, c: CardIdx, face: u8) {
    let ch = option_of(g, c).unwrap_or_else(|| panic!("no option for the card: {:?}", g.options_desc()));
    g.choose(ch);
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(face));
    }
}

// ---- Norman Osborn // Green Goblin -----------------------------------------------

const NORMAN: &str = "Norman Osborn // Green Goblin";
const GOBLIN: &str = "Green Goblin";

#[test]
fn norman_osborn_cant_be_blocked_and_connives_cr_701_50() {
    // "Norman Osborn can't be blocked. Whenever Norman Osborn deals combat
    // damage to a player, he connives."
    let mut g = scen()
        .battlefield(0, &[NORMAN])
        .battlefield(1, &["Test Reacher"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let n = bf(&g, 0, NORMAN);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(n));
    g.choose(Choice::Pass);
    // No blocker can block it: the Reacher's only option is not to block.
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    assert!(!g.has_option(Choice::Card(n)));
    g.choose(Choice::Pass);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    // Discard the Bolt (a nonland card): a +1/+1 counter.
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::Discard));
    g.choose(Choice::Card(bolt));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 19);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.counters_on(n, Counter::P1P1), 1);
    assert_eq!(g.player(0).hand.len(), 1, "drew one, discarded one");
}

#[test]
fn norman_osborn_transforms_into_green_goblin_and_green_goblin_is_castable() {
    // ruling: 56f9c956
    // "{1}{U}{B}{R}: Transform Norman Osborn. Activate only as a sorcery."
    let mut g = scen().battlefield(0, &[NORMAN, "Island", "Swamp", "Mountain", "Mountain"]).start();
    let n = bf(&g, 0, NORMAN);
    g.choose(act(n, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.name(n), GOBLIN);
    assert_eq!(g.pt(n), (3, 3));
    assert!(g.has_kw(n, Kw::Flying) && g.has_kw(n, Kw::Menace));
    assert_eq!(g.def_of(n).mana_value(), 4, "an MDFC's back face: its own mana value (CR 712.8f)");
    // The MDFC's back face cast from hand (CR 712.11b).
    let mut g = scen().battlefield(0, &["Island", "Swamp", "Mountain", "Mountain"]).hand(0, &[NORMAN]).start();
    let c = g.card(0, NORMAN, Zone::Hand);
    assert_eq!(option_of(&g, c), Some(Choice::Faces(c, FACE_CAST | OTHER_CAST)));
    choose_face(&mut g, c, 1);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(bf(&g, 0, GOBLIN), c);
}

#[test]
fn green_goblin_makes_spells_cast_from_your_graveyard_cheaper() {
    // ruling: 7536ad72
    // "Spells you cast from your graveyard cost {2} less to cast."
    // Faithless Looting's flashback {2}{R} costs {R}.
    for goblin in [true, false] {
        let mut sc = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 4])
            .library(1, &["Forest"; 10])
            .battlefield(0, &["Mountain"])
            .graveyard(0, &["Faithless Looting"]);
        if goblin {
            sc = sc.battlefield(0, &[NORMAN]).transformed(0, NORMAN);
        }
        let mut g = sc.start();
        let fl = g.card(0, "Faithless Looting", Zone::Graveyard);
        assert_eq!(option_of(&g, fl).is_some(), goblin);
        if goblin {
            g.choose(Choice::Cast(fl));
            pay(&mut g);
            assert_eq!(g.stack().len(), 1);
            settle(&mut g);
            assert_eq!(g.obj(fl).zone, Zone::Exile, "flashback");
        }
    }
}

#[test]
fn mayhem_lasts_for_the_turn_the_card_was_discarded() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Test Bear", "Island", "Island", "Island"])
        .library(1, &["Forest"; 10])
        .battlefield(0, &[NORMAN, "Mountain", "Forest", "Forest"])
        .transformed(0, NORMAN)
        .hand(0, &["Faithless Looting"])
        .start();
    let loot = g.card(0, "Faithless Looting", Zone::Hand);
    g.choose(Choice::Cast(loot));
    pay(&mut g);
    let bear = g.objs().iter().enumerate().find(|(_, o)| o.zone == Zone::Library && db().def(o.def).name == "Test Bear").map(|(i, _)| CardIdx::new(i as u32)).unwrap();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Discard) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c == Choice::Card(bear)).or(Some(g.pending_decision().unwrap().options[0].choice)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    // {1}{G} less {2}: {G}. Castable now (a creature: sorcery speed).
    assert_eq!(option_of(&g, bear), Some(Choice::Cast(bear)));
    g.choose(Choice::Cast(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    let untapped = g.battlefield().iter().filter(|&&c| g.obj(c).controller == 0 && g.is_type(c, engine::db::types::LAND) && !g.obj(c).tapped).count();
    assert_eq!(untapped, 1, "the Bear cost {{G}} only");
    // A card discarded in an earlier turn has no mayhem.
    let g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 4])
        .library(1, &["Forest"; 10])
        .battlefield(0, &[NORMAN, "Forest", "Forest"])
        .transformed(0, NORMAN)
        .graveyard(0, &["Test Bear"])
        .start();
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    assert_eq!(option_of(&g, bear), None);
}

// ---- Life // Death (a split card) -------------------------------------------------

const LIFE_DEATH: &str = "Life // Death";

#[test]
fn life_death_is_cast_as_one_half_cr_709_3() {
    // ruling: 9fbd1525 c622b600
    let mut g = scen()
        .battlefield(0, &["Forest", "Swamp", "Swamp"])
        .graveyard(0, &["Test Giant"])
        .hand(0, &[LIFE_DEATH])
        .start();
    let c = g.card(0, LIFE_DEATH, Zone::Hand);
    // In hand: both halves combined (CR 709.4): mana value 3, black and
    // green.
    assert_eq!(g.def_of(c).mana_value(), 3);
    assert_eq!(g.colors_of(c), (1 << 2) | (1 << 4));
    assert_eq!(option_of(&g, c), Some(Choice::Faces(c, FACE_CAST | OTHER_CAST)));
    // Death: "Return target creature card from your graveyard to the
    // battlefield. You lose life equal to its mana value."
    choose_face(&mut g, c, 1);
    assert_eq!(g.name(c), "Death");
    assert_eq!(g.def_of(c).mana_value(), 2, "only the half cast (CR 709.3b, 202.3d)");
    let giant = g.card(0, "Test Giant", Zone::Graveyard);
    g.choose(g.tgt(giant));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert_eq!(g.life(0), 15);
    assert_eq!(g.obj(c).zone, Zone::Graveyard);
    assert_eq!(g.name(c), LIFE_DEATH);
}

#[test]
fn life_makes_your_lands_one_one_creatures_until_end_of_turn() {
    // ruling: 64340627
    // "All lands you control become 1/1 creatures until end of turn.
    // They're still lands."
    let mut g = scen().battlefield(0, &["Forest", "Swamp"]).battlefield(1, &["Island"]).hand(0, &[LIFE_DEATH]).start();
    let c = g.card(0, LIFE_DEATH, Zone::Hand);
    choose_face(&mut g, c, 0);
    assert_eq!(g.name(c), "Life");
    pay(&mut g);
    settle(&mut g);
    let forest = bf(&g, 0, "Forest");
    assert!(g.is_creature(forest) && g.is_type(forest, engine::db::types::LAND));
    assert_eq!(g.pt(forest), (1, 1));
    assert!(!g.is_creature(bf(&g, 1, "Island")));
    // They keep their subtypes and the mana abilities those give (ruling
    // 64340627): the untapped Swamp is a Swamp and still taps for {B}.
    let swamp = bf(&g, 0, "Swamp");
    assert!(g.is_creature(swamp) && g.has_subtype(swamp, db().subtype_id("Swamp").unwrap()));
    assert!(g.can_pay(0, &engine::mana::ManaCost::parse("{B}").unwrap()), "{}", g.log_lines().join("
"));
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert!(!g.is_creature(forest));
}

// ---- Ajani, Nacatl Pariah -------------------------------------------------------

const AJANI: &str = "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger";
const AVENGER: &str = "Ajani, Nacatl Avenger";

fn ajani_scen() -> Scenario {
    scen()
}

#[test]
fn ajani_flips_once_when_two_cats_die_together() {
    // "When Ajani enters, create a 2/1 white Cat Warrior creature token.
    // Whenever one or more other Cats you control die, you may exile Ajani,
    // then return him to the battlefield transformed under his owner's
    // control."
    let mut g = ajani_scen()
        .battlefield(0, &["Plains", "Plains", "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &[AJANI, "Fiery Confluence"])
        .start();
    let a = g.card(0, AJANI, Zone::Hand);
    g.choose(Choice::Cast(a));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Cat Warrior", Zone::Battlefield), 1);
    assert_eq!(g.pt(a), (1, 2));
    // Fiery Confluence kills the Cat Warrior (2/1), not Ajani (1/2).
    let fc = g.card(0, "Fiery Confluence", Zone::Hand);
    g.choose(Choice::Cast(fc));
    // Modes: 1 damage to each creature, then 2 to each opponent twice.
    g.choose(Choice::Number(0));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(1));
    pay(&mut g);
    let yes = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => {
            yes.set(yes.get() + 1);
            Some(Choice::Yes)
        }
        _ => None,
    });
    assert_eq!(yes.get(), 1);
    let av = bf(&g, 0, AVENGER);
    assert_eq!(av, a);
    assert!(g.is_type(av, engine::db::types::PLANESWALKER));
    assert_eq!(g.counters_on(av, Counter::Loyalty), 3);
    assert_eq!(g.colors_of(av), 1 | (1 << 3));
}

#[test]
fn ajani_one_or_more_cats_dying_at_once_trigger_once() {
    // Two Cats die at once (Fiery Confluence's 1 damage to each creature
    // kills the two 2/1 Cats and spares Ajani, 1/2): one trigger. Then one
    // more Cat alone: another.
    let mut g = ajani_scen()
        .battlefield(0, &[AJANI, "Test Cat", "Test Cat", "Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Fiery Confluence"])
        .start();
    let fc = g.card(0, "Fiery Confluence", Zone::Hand);
    g.choose(Choice::Cast(fc));
    g.choose(Choice::Number(0));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(1));
    pay(&mut g);
    // Declining: nothing happens; one decision only.
    let asked = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => {
            asked.set(asked.get() + 1);
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(g.count_in(0, "Test Cat", Zone::Graveyard), 2);
    assert_eq!(asked.get(), 1, "one or more Cats: one trigger");
    assert_eq!(g.count_in(0, AJANI, Zone::Battlefield), 1);
}

#[test]
fn ajani_nacatl_avenger_loyalty_abilities() {
    let mut g = ajani_scen()
        .battlefield(0, &[AJANI])
        .transformed(0, AJANI)
        .battlefield(0, &["Test Bear", "Mountain"])
        .battlefield(1, &["Sol Ring", "Test Bear", "Mind Stone", "Forest"])
        .start();
    let av = bf(&g, 0, AVENGER);
    assert_eq!(g.counters_on(av, Counter::Loyalty), 3);
    // 0: a Cat Warrior. "When you do, if you control a red permanent other
    // than Ajani" (none: the Mountain is colourless, Ajani himself doesn't
    // count): the reflexive ability doesn't trigger, and nothing asks for a
    // target (CR 603.4, 603.12).
    g.choose(act(av, 1));
    resolve_with(&mut g, |g| {
        assert_ne!(g.kind(), Some(DecisionKind::Target), "no target without the reflexive trigger");
        None
    });
    assert_eq!(g.count_in(0, "Cat Warrior", Zone::Battlefield), 1);
    assert_eq!(g.life(1), 20);
    // Next turn: +2: a +1/+1 counter on each Cat you control (not the Bear).
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    let cat = bf(&g, 0, "Cat Warrior");
    g.choose(act(av, 0));
    settle(&mut g);
    assert_eq!(g.counters_on(cat, Counter::P1P1), 1);
    assert_eq!(g.counters_on(bf(&g, 0, "Test Bear"), Counter::P1P1), 0);
    assert_eq!(g.counters_on(av, Counter::Loyalty), 5);
    // Then -4: the opponent keeps an artifact and a creature (no
    // enchantment or planeswalker) and sacrifices the rest of its nonland
    // permanents.
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().number == 7 && g.turn().step == Step::Main1 && at_priority(g));
    g.choose(act(av, 2));
    let ring = bf(&g, 1, "Sol Ring");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => {
            assert_eq!(g.to_move(), Some(1), "each opponent chooses");
            if g.has_option(Choice::Card(ring)) {
                Some(Choice::Card(ring))
            } else {
                None
            }
        }
        _ => None,
    });
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Battlefield), 1);
    assert_eq!(g.count_in(1, "Mind Stone", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1);
    assert_eq!(g.count_in(1, "Forest", Zone::Battlefield), 1, "lands stay");
}

/// Ajani, Nacatl Avenger's 0: "Create a 2/1 white Cat Warrior creature token.
/// When you do, if you control a red permanent other than Ajani, he deals
/// damage equal to the number of creatures you control to any target." The
/// token first, then a reflexive triggered ability (CR 603.12) whose target
/// is chosen as it is put on the stack (603.3d), with an intervening "if"
/// (603.4) checked as it triggers and again as it resolves.
#[test]
fn ajani_nacatl_avenger_zero_is_a_reflexive_trigger_cr603_12() {
    let mut g = ajani_scen()
        .battlefield(0, &[AJANI])
        .transformed(0, AJANI)
        .battlefield(0, &["Test Bear", "Test Hasty"])
        .at(Step::Main1, 0)
        .start();
    let av = bf(&g, 0, AVENGER);
    // No target is asked as the loyalty ability is activated.
    g.choose(act(av, 1));
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    assert_eq!(g.stack().len(), 1, "the loyalty ability, no target");
    assert!(g.stack()[0].targets.is_empty());
    // It resolves: the Cat, then the reflexive trigger asks its target.
    g.pass();
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.count_in(0, "Cat Warrior", Zone::Battlefield), 1);
    assert_eq!(g.kind(), Some(DecisionKind::Target), "its target as it goes on the stack");
    g.choose(player(1));
    settle(&mut g);
    // Test Bear, Test Hasty (red) and the Cat: 3 damage.
    assert_eq!(g.life(1), 17);
}

/// The reflexive trigger's intervening "if" is checked again as it resolves
/// (CR 603.4): the red permanent gone in response, it does nothing.
#[test]
fn ajani_nacatl_avenger_reflexive_trigger_checks_its_if_again_cr603_4() {
    let mut g = ajani_scen()
        .battlefield(0, &[AJANI])
        .transformed(0, AJANI)
        .battlefield(0, &["Test Hasty"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let av = bf(&g, 0, AVENGER);
    let hasty = bf(&g, 0, "Test Hasty");
    g.choose(act(av, 1));
    // Both pass: the loyalty ability resolves; the trigger targets P1.
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    g.pass();
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(at_priority);
    assert_eq!(g.stack().len(), 1, "the reflexive trigger");
    // P0 passes; P1 bolts Test Hasty in response.
    g.pass();
    g.drive_until(|g| at_priority(g) && g.to_move() == Some(1));
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(g.tgt(hasty));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(hasty).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20, "no red permanent other than Ajani: nothing (603.4)");
}

// ---- encoding ---------------------------------------------------------------------

#[test]
fn the_encoding_of_faces_is_the_same_in_every_world_of_the_viewer() {
    // A transformed permanent, an MDFC face choice pending, a card on an
    // adventure, day: the viewer's features don't change under its own
    // determinize nor the other player's (ENGINE.md 10, 21.2).
    let mut g = scen()
        .battlefield(0, &["Ulvenwald Oddity // Ulvenwald Behemoth", "Mountain", "Mountain", "Mountain", "Island"])
        .transformed(0, "Ulvenwald Oddity // Ulvenwald Behemoth")
        .battlefield(1, &["Sol Ring"])
        .day(Some(true))
        .hand(0, &["Embereth Shieldbreaker // Battle Display", "Shatterskull Smashing // Shatterskull, the Hammer Pass", "Island"])
        .hand(1, &["Forest", "Lightning Bolt"])
        .start();
    let s = g.card(0, "Embereth Shieldbreaker // Battle Display", Zone::Hand);
    choose_face(&mut g, s, 1);
    g.choose(g.tgt(bf(&g, 1, "Sol Ring")));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Exile);
    let sk = g.card(0, "Shatterskull Smashing // Shatterskull, the Hammer Pass", Zone::Hand);
    g.choose(option_of(&g, sk).unwrap());
    assert_eq!(g.kind(), Some(DecisionKind::ChooseFace));
    let n = engine::az::encode::NUM_FEATURES;
    for viewer in [0u8, 1] {
        let mut base = vec![0f32; n];
        g.encode_features(viewer, &mut base);
        for seed in 0..10 {
            for who in [0u8, 1] {
                let mut w = g.clone();
                w.determinize(who, &mut engine::rng::Rng::new(seed));
                if who != viewer {
                    continue;
                }
                let mut f = vec![0f32; n];
                w.encode_features(viewer, &mut f);
                assert!(f == base, "P{viewer}'s features changed under its determinize");
            }
        }
    }
    // The back face up and the face choice show.
    let mut f = vec![0f32; n];
    g.encode_features(0, &mut f);
    assert!(f.iter().any(|&x| x != 0.0));
}
