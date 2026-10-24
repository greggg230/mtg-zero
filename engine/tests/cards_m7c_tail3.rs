//! M7c, the single-card tail part 3 (ENGINE.md 30): the cards that needed
//! three or more features, and the face-down cards. Test names cite the CR.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| {
        s.auto_single = false;
        s.log = true;
    }).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

#[allow(dead_code)]
fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Pay with the first source offered until the payment is over.
fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)));
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
    panic!("resolve_with: no progress\n{}", log(g));
}

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Cast `name` from P0's hand announcing X (an X decision first), then pay.
fn cast_x(g: &mut Game, name: &str, x: u8) -> CardIdx {
    let c = g.card(0, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(x));
    pay(g);
    c
}

// ---- Vicious Rivalry, Toxic Deluge (X life) ---------------------------------

/// "As an additional cost to cast this spell, pay X life. Destroy all
/// artifacts and creatures with mana value X or less" (CR 107.3, 601.2b).
#[test]
fn vicious_rivalry_pays_x_life_and_destroys_mana_value_x_or_less_cr107_3() {
    let mut g = scen()
        .battlefield(0, &["Bayou", "Bayou", "Bayou", "Bayou", "Test Bear", "Sol Ring"])
        .hand(0, &["Vicious Rivalry"])
        .battlefield(1, &["Test Giant", "Mind Stone", "Test Flyer", "Forest"])
        .at(Step::Main1, 0)
        .start();
    cast_x(&mut g, "Vicious Rivalry", 2);
    settle(&mut g);
    assert_eq!(g.life(0), 18, "X life paid\n{}", log(&g));
    for (p, n) in [(0, "Test Bear"), (0, "Sol Ring"), (1, "Mind Stone"), (1, "Test Flyer")] {
        assert_eq!(g.count_in(p, n, Zone::Graveyard), 1, "{n} destroyed\n{}", log(&g));
    }
    assert_eq!(g.count_in(1, "Test Giant", Zone::Battlefield), 1, "mana value 5 stays");
    assert_eq!(g.count_in(1, "Forest", Zone::Battlefield), 1, "a land isn't an artifact or creature");
}

/// "As an additional cost to cast this spell, pay X life. All creatures get
/// -X/-X until end of turn" (X locked in, CR 608.2h).
#[test]
fn toxic_deluge_gives_all_creatures_minus_x_cr107_3() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Test Bear"])
        .hand(0, &["Toxic Deluge"])
        .battlefield(1, &["Test Giant", "Test Flyer", "Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    cast_x(&mut g, "Toxic Deluge", 2);
    settle(&mut g);
    assert_eq!(g.life(0), 18);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Test Flyer", Zone::Graveyard), 1);
    assert_eq!(g.pt(bf(&g, 1, "Test Reacher")), (-1, 1), "1/3 survives as -1/1");
    let giant = bf(&g, 1, "Test Giant");
    assert_eq!(g.pt(giant), (3, 3), "5/5 gets -2/-2");
}

// ---- Winds of Abandon ---------------------------------------------------------

/// Overloaded (CR 702.96): "exile each creature you don't control. For each
/// creature exiled this way, its controller searches their library for a
/// basic land card. Those players put those cards onto the battlefield
/// tapped, then shuffle."
#[test]
fn winds_of_abandon_overloaded_exiles_each_and_its_controller_searches_per_creature_cr702_96() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 6])
        .battlefield(0, &["Test Bear"])
        .hand(0, &["Winds of Abandon"])
        .battlefield(1, &["Test Giant", "Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Winds of Abandon", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    assert_eq!(g.pending_decision().unwrap().player, 1, "their controller searches");
    let forest = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
    g.choose(forest);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "a second basic land for the second creature");
    g.choose(forest);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Giant", Zone::Exile) + g.count_in(1, "Test Flyer", Zone::Exile), 2);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1, "yours stay");
    let lands: Vec<_> = g.permanents_of(1).filter(|&c| g.name(c) == "Forest").collect();
    assert_eq!(lands.len(), 2, "two basic lands\n{}", log(&g));
    assert!(lands.iter().all(|&c| g.obj(c).tapped), "tapped");
}

// ---- Mawloc -------------------------------------------------------------------

/// Ravenous (CR 702.156a) and "it fights up to one target creature an
/// opponent controls. If that creature would die this turn, exile it
/// instead" (701.14, 614).
#[test]
fn mawloc_fights_and_the_creature_that_would_die_is_exiled_cr701_14() {
    let mut g = scen()
        .battlefield(0, &["Taiga", "Taiga", "Taiga"])
        .hand(0, &["Mawloc"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast_x(&mut g, "Mawloc", 1);
    let bear = bf(&g, 1, "Test Bear");
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::Target) { Some(g.tgt(bear)) } else { None });
    let m = bf(&g, 0, "Mawloc");
    assert_eq!(g.pt(m), (3, 3), "one +1/+1 counter");
    assert_eq!(g.obj(m).damage, 2, "the Bear fought back");
    assert_eq!(g.count_in(1, "Test Bear", Zone::Exile), 1, "exiled instead of dying\n{}", log(&g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 0);
}

// ---- Fastbond -------------------------------------------------------------------

/// "You may play any number of lands on each of your turns. Whenever you
/// play a land, if it wasn't the first land you played this turn, this
/// enchantment deals 1 damage to you" (CR 305.2, 603.4).
#[test]
fn fastbond_any_number_of_lands_and_a_damage_after_the_first_cr305_2() {
    let mut g = scen()
        .battlefield(0, &["Fastbond"])
        .hand(0, &["Forest", "Forest", "Forest"])
        .at(Step::Main1, 0)
        .start();
    for k in 0..3 {
        let f = g.card(0, "Forest", Zone::Hand);
        assert!(g.has_option(Choice::Play(f)), "land {k} may be played");
        g.choose(Choice::Play(f));
        settle(&mut g);
    }
    assert_eq!(g.count_in(0, "Forest", Zone::Battlefield), 3);
    assert_eq!(g.life(0), 18, "1 damage for the second and the third\n{}", log(&g));
}

// ---- Pest Infestation -------------------------------------------------------------

/// "Destroy up to X target artifacts and/or enchantments. Create twice X 1/1
/// black and green Pest creature tokens" (X announced first, CR 601.2b-c).
#[test]
fn pest_infestation_up_to_x_targets_and_twice_x_pests_cr601_2c() {
    let mut g = scen()
        .battlefield(0, &["Forest"; 5])
        .hand(0, &["Pest Infestation"])
        .battlefield(1, &["Sol Ring", "Mind Stone", "Fastbond"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Pest Infestation", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let ring = bf(&g, 1, "Sol Ring");
    g.choose(g.tgt(ring));
    // Up to X: one is enough.
    assert!(g.has_option(Choice::Pass));
    g.choose(Choice::Pass);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Mind Stone", Zone::Battlefield), 1);
    let pests: Vec<_> = g.permanents_of(0).filter(|&c| g.name(c) == "Pest").collect();
    assert_eq!(pests.len(), 4, "twice X\n{}", log(&g));
    assert_eq!(g.pt(pests[0]), (1, 1));
}

/// "When this artifact is put into a graveyard from the battlefield, draw a
/// card": the IR's event is `leaves_battlefield` (as Chromatic Star's);
/// blocked (docs/ir_defects.md 15).
#[test]
fn wizards_rockets_is_blocked_by_its_ir_event() {
    let err = engine::db::CardDb::cube().expect_id("Wizard's Rockets").unwrap_err();
    assert!(err.contains("IR:TriggeredAbility.event"), "{err}");
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
    panic!("quiet_until: no progress\n{}", log(g));
}

/// Pass priority until attackers are declared, then attack with `names`.
fn attack_with(g: &mut Game, names: &[&str]) {
    for _ in 0..100 {
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers) || at_priority(g));
        if g.kind() == Some(DecisionKind::DeclareAttackers) {
            break;
        }
        g.pass();
    }
    for n in names {
        let c = bf(g, 0, n);
        g.choose(Choice::Card(c));
        g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    }
    g.choose(Choice::Pass);
}

// ---- Generous Plunderer ------------------------------------------------------------

/// "At the beginning of your upkeep, you may create a Treasure token. When
/// you do, target opponent creates a tapped Treasure token" (a reflexive
/// trigger, CR 603.12). "Whenever this creature attacks, it deals damage to
/// defending player equal to the number of artifacts they control."
#[test]
fn generous_plunderer_reflexive_treasure_cr603_12_and_damage_by_their_artifacts() {
    let mut g = scen()
        .battlefield(0, &["Generous Plunderer"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main2, 1)
        .start();
    quiet_until(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(Choice::Target(engine::Target::Player(1))));
    assert!(!g.has_option(Choice::Target(engine::Target::Player(0))), "target opponent");
    g.choose(Choice::Target(engine::Target::Player(1)));
    settle(&mut g);
    let t0: Vec<_> = g.permanents_of(0).filter(|&c| g.name(c) == "Treasure").collect();
    let t1: Vec<_> = g.permanents_of(1).filter(|&c| g.name(c) == "Treasure").collect();
    assert_eq!((t0.len(), t1.len()), (1, 1), "{}", log(&g));
    assert!(!g.obj(t0[0]).tapped && g.obj(t1[0]).tapped, "theirs is tapped");
    attack_with(&mut g, &["Generous Plunderer"]);
    settle(&mut g);
    assert_eq!(g.life(1), 18, "Sol Ring and the Treasure: 2 damage\n{}", log(&g));
}

/// Declining the Treasure: no reflexive trigger.
#[test]
fn generous_plunderer_declined_makes_no_reflexive_trigger_cr603_12() {
    let mut g = scen().battlefield(0, &["Generous Plunderer"]).at(Step::Main2, 1).start();
    quiet_until(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Pass);
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::Target));
    assert!(at_priority(&g) && g.stack().is_empty(), "nothing waits");
    assert_eq!(g.permanents_of(1).count(), 0);
}

// ---- Inti, Seneschal of the Sun -------------------------------------------------------

/// "Whenever you attack, you may discard a card. When you do, put a +1/+1
/// counter on target attacking creature. It gains trample until end of
/// turn." "Whenever you discard one or more cards, exile the top card of
/// your library. You may play that card until your next end step."
#[test]
fn inti_discard_then_reflexive_counter_cr603_12_and_play_until_your_next_end_step() {
    let mut g = scen()
        .battlefield(0, &["Inti, Seneschal of the Sun", "Test Bear"])
        .hand(0, &["Lightning Bolt"])
        .library(0, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    attack_with(&mut g, &["Inti, Seneschal of the Sun", "Test Bear"]);
    g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::Discard) | Some(DecisionKind::ChooseDiscard)));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Card(bolt));
    let bear = bf(&g, 0, "Test Bear");
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::Target) { Some(g.tgt(bear)) } else { None });
    assert_eq!(g.pt(bear), (3, 3), "a +1/+1 counter\n{}", log(&g));
    assert!(g.has_kw(bear, engine::db::Kw::Trample));
    let forest = g.card(0, "Forest", Zone::Exile);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(g.has_option(Choice::Play(forest)), "playable until your next end step");
    quiet_until(&mut g, |g| g.turn().step == Step::End && at_priority(g) && g.turn().active == 0);
    assert!(!g.has_option(Choice::Play(forest)), "not in the end step");
}

// ---- The One Ring -------------------------------------------------------------------------

/// "When The One Ring enters, if you cast it, you gain protection from
/// everything until your next turn" (CR 702.16j: can't be targeted, damage
/// prevented); "{T}: Put a burden counter on The One Ring, then draw a card
/// for each burden counter"; "at the beginning of your upkeep, you lose 1
/// life for each burden counter".
#[test]
fn the_one_ring_protection_from_everything_cr702_16j_and_burden_counters() {
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["The One Ring"])
        .battlefield(1, &["Mountain", "Test Bear"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "The One Ring", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    settle(&mut g);
    assert!(g.log_lines().iter().any(|l| l.contains("protection from everything")), "{}", log(&g));
    let ring = bf(&g, 0, "The One Ring");
    g.choose(act(ring, 0));
    settle(&mut g);
    let burden = engine::db::Counter::Named(db().subtype_id("counter:burden").unwrap());
    assert_eq!(g.counters_on(ring, burden), 1);
    assert_eq!(g.hand_sorted(0).len(), 1, "one card drawn");
    // P1's turn: P0 can't be targeted.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(Choice::Target(engine::Target::Player(0))), "protection: not a legal target");
    assert!(g.has_option(Choice::Target(engine::Target::Player(1))));
    g.choose(Choice::Target(engine::Target::Player(1)));
    // P0's next upkeep: the protection is gone; 1 life per burden counter.
    let life = g.life(0);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.life(0), life - 1, "{}", log(&g));
}

/// Put onto the battlefield without being cast: no protection.
#[test]
fn the_one_ring_not_cast_no_protection_cr603_4() {
    let mut g = scen().battlefield(0, &["The One Ring"]).battlefield(1, &["Mountain"]).hand(1, &["Lightning Bolt"]).at(Step::Main1, 1).start();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(Choice::Target(engine::Target::Player(0))));
}

// ---- Pentad Prism ---------------------------------------------------------------------------

/// Sunburst (CR 702.44a): "This artifact enters with a charge counter on it
/// for each color of mana spent to cast it." "Remove a charge counter from
/// this artifact: Add one mana of any color."
#[test]
fn pentad_prism_sunburst_counters_cr702_44a_and_its_mana() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Plains"])
        .hand(0, &["Pentad Prism"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Pentad Prism", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    settle(&mut g);
    let prism = bf(&g, 0, "Pentad Prism");
    let charge = engine::db::Counter::Named(db().subtype_id("counter:charge").unwrap());
    assert_eq!(g.counters_on(prism, charge), 2, "red and white\n{}", log(&g));
    // Activated with priority (no {T}: it may be activated again).
    g.choose(act(prism, 0));
    g.drive_until(|g| at_priority(g) || matches!(g.kind(), Some(DecisionKind::ChooseMana) | Some(DecisionKind::ChooseColor)));
    let k = g.pending_decision().unwrap().options[0].choice;
    g.choose(k);
    g.drive_until(at_priority);
    assert_eq!(g.counters_on(prism, charge), 1, "{}", log(&g));
}

// ---- "Put into a graveyard from anywhere": Worldspine Wurm, Emrakul, Blightsteel ----

/// "When this creature dies, create three 5/5 green Wurm creature tokens with
/// trample. When Worldspine Wurm is put into a graveyard from anywhere,
/// shuffle it into its owner's library" (its own ability from the
/// graveyard, CR 603.6a; the owner controls it).
#[test]
fn worldspine_wurm_dies_makes_wurms_and_shuffles_itself_in_cr603_6a() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 4])
        .hand(0, &["Wrath of God"])
        .battlefield(1, &["Worldspine Wurm"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Wrath of God", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    settle(&mut g);
    let wurms: Vec<_> = g.permanents_of(1).filter(|&c| g.name(c) == "Wurm").collect();
    assert_eq!(wurms.len(), 3, "{}", log(&g));
    assert_eq!(g.pt(wurms[0]), (5, 5));
    assert_eq!(g.count_in(1, "Worldspine Wurm", Zone::Library), 1, "shuffled into its owner's library\n{}", log(&g));
    assert_eq!(g.count_in(1, "Worldspine Wurm", Zone::Graveyard), 0);
}

/// From the library (Entomb): put into a graveyard from anywhere.
#[test]
fn worldspine_wurm_entombed_is_shuffled_back_cr603_6a() {
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .hand(0, &["Entomb"])
        .library(0, &["Worldspine Wurm", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Entomb", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    let wurm = g.card(0, "Worldspine Wurm", Zone::Library);
    let d = g.obj(wurm).def;
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::SearchLibrary) { Some(Choice::Def(d)) } else { None });
    assert_eq!(g.count_in(0, "Worldspine Wurm", Zone::Library), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Worldspine Wurm", Zone::Graveyard), 0);
}

/// Emrakul, the Aeons Torn: "When you cast this spell, take an extra turn
/// after this one"; protection from spells that are one or more colors (CR
/// 702.16: not a legal target of a coloured spell); annihilator 6 (702.86a).
#[test]
fn emrakul_extra_turn_colored_spell_protection_and_annihilator_cr702_86a() {
    let mut g = scen()
        .battlefield(0, &["Island"; 15])
        .hand(0, &["Emrakul, the Aeons Torn"])
        .battlefield(1, &["Mountain", "Mountain", "Forest", "Forest", "Test Bear", "Sol Ring", "Mind Stone"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Emrakul, the Aeons Torn", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    settle(&mut g);
    let emrakul = bf(&g, 0, "Emrakul, the Aeons Torn");
    let t = g.turn().number;
    // The extra turn: P0 again.
    quiet_until(&mut g, |g| g.turn().number == t + 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.turn().active, 0, "an extra turn\n{}", log(&g));
    let _ = emrakul;
    attack_with(&mut g, &["Emrakul, the Aeons Torn"]);
    // Annihilator 6: P1 sacrifices six of seven permanents.
    resolve_with(&mut g, |_| None);
    assert_eq!(g.permanents_of(1).count(), 1, "six sacrificed\n{}", log(&g));
}

/// "When Emrakul is put into a graveyard from anywhere, its owner shuffles
/// their graveyard into their library."
#[test]
fn emrakul_put_into_a_graveyard_shuffles_the_graveyard_in_cr603_6a() {
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .hand(0, &["Entomb"])
        .graveyard(0, &["Test Bear", "Lightning Bolt"])
        .library(0, &["Emrakul, the Aeons Torn", "Island"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Entomb", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    let e = g.card(0, "Emrakul, the Aeons Torn", Zone::Library);
    let d = g.obj(e).def;
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::SearchLibrary) { Some(Choice::Def(d)) } else { None });
    assert_eq!(g.graveyard_cards(0).count(), 0, "the graveyard, Emrakul and Entomb too\n{}", log(&g));
    assert_eq!(g.count_in(0, "Emrakul, the Aeons Torn", Zone::Library), 1);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Library), 1);
}

/// Blightsteel Colossus: infect (CR 702.90b-c: poison counters for a
/// player, -1/-1 counters for a creature); "if Blightsteel Colossus would
/// be put into a graveyard from anywhere, reveal it and shuffle it into its
/// owner's library instead" (614.1a).
#[test]
fn blightsteel_infect_poison_and_minus_counters_cr702_90() {
    let mut g = scen()
        .battlefield(0, &["Blightsteel Colossus"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 1, "Test Giant");
    attack_with(&mut g, &["Blightsteel Colossus"]);
    // P1 blocks with the Giant: 5 -1/-1 counters kill it; 6 trample over as
    // poison.
    for _ in 0..200 {
        match g.kind() {
            Some(DecisionKind::DeclareBlockers) => {
                let o = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
                g.choose(o);
            }
            Some(DecisionKind::AssignDamage) => {
                let d = g.pending_decision().unwrap();
                let k = d.options.iter().find(|o| o.choice == Choice::Number(5)).map(|o| o.choice).unwrap_or(d.options[0].choice);
                g.choose(k);
            }
            Some(DecisionKind::Priority) if g.turn().step == Step::Main2 => break,
            Some(DecisionKind::Priority) => {
                g.pass();
            }
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    assert_eq!(g.count_in(1, "Test Giant", Zone::Graveyard), 1, "-1/-1 counters\n{}", log(&g));
    assert_eq!(g.player(1).poison, 6, "trample damage as poison\n{}", log(&g));
    assert_eq!(g.life(1), 20, "no life lost");
    let _ = giant;
}

/// The replacement from the library (Entomb) and from the battlefield (no
/// "dies", CR 700.4).
#[test]
fn blightsteel_would_go_to_a_graveyard_is_shuffled_into_the_library_cr614_1a() {
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .hand(0, &["Entomb"])
        .library(0, &["Blightsteel Colossus", "Island"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Entomb", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    let b = g.card(0, "Blightsteel Colossus", Zone::Library);
    let d = g.obj(b).def;
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::SearchLibrary) { Some(Choice::Def(d)) } else { None });
    assert_eq!(g.count_in(0, "Blightsteel Colossus", Zone::Library), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Blightsteel Colossus", Zone::Graveyard), 0);
    assert!(g.log_lines().iter().any(|l| l.contains("shuffled into its owner's library instead")));
}

/// Protection from spells that are one or more colors (CR 702.16b): a red
/// spell can't target Emrakul; an ability can.
#[test]
fn emrakul_cant_be_targeted_by_a_colored_spell_cr702_16b() {
    let mut g = scen()
        .battlefield(0, &["Emrakul, the Aeons Torn"])
        .battlefield(1, &["Mountain", "Test Bear"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let emrakul = bf(&g, 0, "Emrakul, the Aeons Torn");
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(emrakul)), "protection from coloured spells");
    assert!(g.has_option(g.tgt(bf(&g, 1, "Test Bear"))));
}

// ---- Draw replacements: Hullbreacher, Quantum Riddler ----------------------------------

/// "If an opponent would draw a card except the first one they draw in each
/// of their draw steps, instead you create a Treasure token" (CR 614.1a).
#[test]
fn hullbreacher_replaces_an_opponents_draws_but_the_first_in_their_draw_step_cr614_1a() {
    let mut g = scen()
        .battlefield(0, &["Hullbreacher"])
        .battlefield(1, &["Island"])
        .hand(1, &["Ancestral Recall"])
        .at(Step::Main1, 1)
        .start();
    let c = g.card(1, "Ancestral Recall", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(engine::Target::Player(1)));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.hand_sorted(1).len(), 0, "no card drawn\n{}", log(&g));
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Treasure").count(), 3);
    // P1's next draw step: the first draw is theirs.
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.hand_sorted(1).len(), 1, "{}", log(&g));
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Treasure").count(), 3);
}

/// "As long as you have one or fewer cards in hand, if you would draw one or
/// more cards, you draw that many cards plus one instead" (CR 614.1a: one
/// draw instruction).
#[test]
fn quantum_riddler_draws_one_more_with_one_or_fewer_cards_in_hand_cr614_1a() {
    let mut g = scen()
        .battlefield(0, &["Island"; 5])
        .hand(0, &["Quantum Riddler"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Quantum Riddler", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || matches!(g.kind(), Some(DecisionKind::ChooseAltCost) | Some(DecisionKind::PayMana)));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
    }
    pay(&mut g);
    settle(&mut g);
    // Its enter trigger: "draw a card" with an empty hand: two.
    assert_eq!(g.hand_sorted(0).len(), 2, "{}", log(&g));
    // Next draw step with two cards in hand: one.
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.hand_sorted(0).len(), 3, "{}", log(&g));
}

// ---- Kicker on permanents: Batroc, Sowing Mycospawn, Thieving Skydiver ----------------

/// Multikicker {2} (CR 702.33c): "Batroc enters with a +1/+1 counter on him
/// for each time he was kicked. When Batroc enters, he deals damage equal to
/// his power to each of up to X targets, where X is the number of times he
/// was kicked."
#[test]
fn batroc_multikicked_twice_counters_and_up_to_two_targets_cr702_33c() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .hand(0, &["Batroc the Leaper"])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Batroc the Leaper", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert!(g.has_option(Choice::Number(2)) && !g.has_option(Choice::Number(3)), "kicked at most twice with six lands");
    g.choose(Choice::Number(2));
    pay(&mut g);
    let bear = bf(&g, 1, "Test Bear");
    let mut picked = 0;
    for _ in 0..50 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || g.kind() == Some(DecisionKind::Target) || at_priority(g));
        match g.kind() {
            Some(DecisionKind::Target) => {
                let c = match picked {
                    0 => g.tgt(bear),
                    1 => Choice::Target(engine::Target::Player(1)),
                    _ => Choice::Pass,
                };
                assert!(g.has_option(c), "pick {picked}");
                picked += 1;
                g.choose(c);
            }
            _ if g.stack().is_empty() => break,
            _ => {
                g.pass();
            }
        }
    }
    assert_eq!(picked, 2, "two targets, no third offered\n{}", log(&g));
    let batroc = bf(&g, 0, "Batroc the Leaper");
    assert_eq!(g.pt(batroc), (4, 4), "two +1/+1 counters");
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.life(1), 16, "4 damage\n{}", log(&g));
}

/// Kicker {1}{C} (the IR's {2}, read from its text: a {C} pip needs
/// colorless mana, CR 107.4c); devoid (702.114a); "when you cast this
/// spell, search your library for a land card, put it onto the battlefield,
/// then shuffle. When you cast this spell, if it was kicked, exile target
/// land."
#[test]
fn sowing_mycospawn_kicked_with_colorless_mana_cast_triggers_cr702_33d() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Sol Ring"])
        .hand(0, &["Sowing Mycospawn"])
        .library(0, &["Island", "Island"])
        .battlefield(1, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Sowing Mycospawn", Zone::Hand);
    assert_eq!(g.colors_of(c), 0, "devoid");
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1));
    pay(&mut g);
    let forest = bf(&g, 1, "Forest");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(forest)),
        Some(DecisionKind::SearchLibrary) => g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).map(|o| o.choice),
        _ => None,
    });
    assert_eq!(g.count_in(1, "Forest", Zone::Exile), 1, "kicked: exile target land\n{}", log(&g));
    assert_eq!(g.count_in(0, "Island", Zone::Battlefield), 1, "a land from the library");
    assert_eq!(g.count_in(0, "Sowing Mycospawn", Zone::Battlefield), 1);
}

/// Without colorless mana the {C} of the kicker can't be paid: no kick.
#[test]
fn sowing_mycospawn_kicker_needs_colorless_mana_cr107_4c() {
    let mut g = scen()
        .battlefield(0, &["Forest"; 6])
        .hand(0, &["Sowing Mycospawn"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Sowing Mycospawn", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode) || g.kind() == Some(DecisionKind::PayMana));
    assert!(!g.has_option(Choice::Number(1)), "no {{C}}: not kicked\n{}", log(&g));
}

/// Kicker {X}, X can't be 0 (CR 601.2b, 107.3): "When this creature enters,
/// if it was kicked, gain control of target artifact with mana value X or
/// less. If that artifact is an Equipment, attach it to this creature."
#[test]
fn thieving_skydiver_kicker_x_steals_and_attaches_an_equipment_cr702_33d() {
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Thieving Skydiver"])
        .battlefield(1, &["Lightning Greaves", "Coalition Relic"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Thieving Skydiver", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode) || g.kind() == Some(DecisionKind::PayMana));
    assert!(!g.has_option(Choice::Number(0)), "X can't be 0");
    assert!(g.has_option(Choice::Number(2)) && !g.has_option(Choice::Number(3)));
    g.choose(Choice::Number(2));
    pay(&mut g);
    let greaves = bf(&g, 1, "Lightning Greaves");
    let monolith = bf(&g, 1, "Coalition Relic");
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || (g.stack().is_empty() && at_priority(g)) || at_priority(g));
    while g.kind() != Some(DecisionKind::Target) {
        g.pass();
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    }
    assert!(!g.has_option(g.tgt(monolith)), "mana value 3 is more than X");
    g.choose(g.tgt(greaves));
    settle(&mut g);
    let sky = bf(&g, 0, "Thieving Skydiver");
    assert_eq!(g.obj(greaves).controller, 0, "{}", log(&g));
    assert_eq!(g.obj(greaves).attached.map(|r| r.idx), Some(sky), "attached");
}

// ---- Splice onto Arcane: Goryo's Vengeance, Through the Breach ----------------------------

/// "Return target legendary creature card from your graveyard to the
/// battlefield. That creature gains haste. Exile it at the beginning of the
/// next end step" (CR 603.7).
#[test]
fn goryos_vengeance_returns_a_legend_with_haste_exiled_at_end_step_cr603_7() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Goryo's Vengeance"])
        .graveyard(0, &["Griselbrand", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Goryo's Vengeance", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::ChooseHandCard));
    if g.kind() == Some(DecisionKind::ChooseHandCard) {
        g.choose(Choice::Pass);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    }
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    assert!(!g.has_option(g.tgt(bear)), "legendary only");
    let gris = g.card(0, "Griselbrand", Zone::Graveyard);
    g.choose(g.tgt(gris));
    pay(&mut g);
    settle(&mut g);
    let gris = bf(&g, 0, "Griselbrand");
    assert!(g.has_kw(gris, engine::db::Kw::Haste));
    quiet_until(&mut g, |g| g.turn().step == Step::Cleanup || g.turn().active == 1);
    assert_eq!(g.count_in(0, "Griselbrand", Zone::Exile), 1, "{}", log(&g));
}

/// Splice onto Arcane (CR 702.47): Through the Breach with Goryo's Vengeance
/// spliced: one spell with both effects, the spliced card revealed and kept
/// in hand, its splice cost paid with the rest, its target chosen with the
/// spell's (702.47b-c).
#[test]
fn through_the_breach_with_goryos_spliced_cr702_47() {
    let mut g = scen()
        .battlefield(0, &["Badlands"; 8])
        .hand(0, &["Through the Breach", "Goryo's Vengeance", "Test Bear"])
        .graveyard(0, &["Griselbrand"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Through the Breach", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    let goryo = g.card(0, "Goryo's Vengeance", Zone::Hand);
    assert!(g.has_option(Choice::Card(goryo)));
    assert!(!g.has_option(Choice::Card(g.card(0, "Test Bear", Zone::Hand))), "not a splice card");
    g.choose(Choice::Card(goryo));
    // Spliced once: only "no more" left.
    g.drive_until(|g| g.kind() != Some(DecisionKind::ChooseHandCard) || g.pending_decision().unwrap().options.len() == 1);
    if g.kind() == Some(DecisionKind::ChooseHandCard) {
        g.choose(Choice::Pass);
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let gris = g.card(0, "Griselbrand", Zone::Graveyard);
    g.choose(g.tgt(gris));
    pay(&mut g);
    assert!(g.obj(goryo).known == 0b11, "revealed");
    let bear = g.card(0, "Test Bear", Zone::Hand);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) => Some(Choice::Card(bear)),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Goryo's Vengeance", Zone::Hand), 1, "the spliced card stays in hand\n{}", log(&g));
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1, "Through the Breach's effect");
    assert_eq!(g.count_in(0, "Griselbrand", Zone::Battlefield), 1, "the spliced effect");
    assert!(g.player(0).pool.total() == 0 && g.permanents_of(0).filter(|&c| g.obj(c).tapped).count() == 8, "4R + 2B paid");
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1, "sacrificed at the end step");
    assert_eq!(g.count_in(0, "Griselbrand", Zone::Exile), 1, "exiled at the end step");
}

/// The splice cost must be payable with the rest: not offered with five
/// lands.
#[test]
fn splice_not_offered_without_its_cost_cr702_47a() {
    let mut g = scen()
        .battlefield(0, &["Badlands"; 5])
        .hand(0, &["Through the Breach", "Goryo's Vengeance"])
        .graveyard(0, &["Griselbrand"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Through the Breach", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard) || g.kind() == Some(DecisionKind::PayMana));
    if g.kind() == Some(DecisionKind::ChooseHandCard) {
        assert_eq!(g.pending_decision().unwrap().options.len(), 1, "only no");
    }
}

// ---- "Name Sticker" Goblin, Parallax Wave, Caustic Bronco, Sword of the Meek --------------

/// "When this creature enters from anywhere other than a graveyard or exile,
/// if it's on the battlefield and you control 9 or fewer creatures named
/// "Name Sticker" Goblin, roll a 20-sided die" (CR 706): 1-6 {R}x4, 7-14
/// {R}x5, 15-20 {R}x6.
#[test]
fn name_sticker_goblin_rolls_for_red_mana_cr706() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 3])
        .hand(0, &["\"Name Sticker\" Goblin"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "\"Name Sticker\" Goblin", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    settle(&mut g);
    let r = g.player(0).pool.amt[3];
    assert!((4..=6).contains(&r), "four to six red\n{}", log(&g));
}

/// From a graveyard (Exhume): no roll.
#[test]
fn name_sticker_goblin_from_a_graveyard_doesnt_roll_cr603_6a() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Exhume"])
        .graveyard(0, &["\"Name Sticker\" Goblin"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Exhume", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "\"Name Sticker\" Goblin", Zone::Battlefield), 1, "{}", log(&g));
    assert_eq!(g.player(0).pool.total(), 0, "no roll");
}

/// Fading 5 (CR 702.32a); "Remove a fade counter from this enchantment:
/// Exile target creature. When this enchantment leaves the battlefield,
/// each player returns to the battlefield all cards they own exiled with it"
/// (607.2a).
#[test]
fn parallax_wave_exiles_and_returns_when_it_leaves_cr702_32a_cr607_2a() {
    let fade = engine::db::Counter::Named(db().subtype_id("counter:fade").unwrap());
    let mut g = scen()
        .battlefield(0, &["Parallax Wave", "Scrubland", "Scrubland", "Scrubland"])
        .counters(0, "Parallax Wave", fade, 5)
        .hand(0, &["Vindicate"])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let wave = bf(&g, 0, "Parallax Wave");
    let bear = bf(&g, 1, "Test Bear");
    let giant = bf(&g, 1, "Test Giant");
    for t in [bear, giant] {
        g.choose(act(wave, 0));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(g.tgt(t));
        settle(&mut g);
    }
    assert_eq!(g.permanents_of(1).count(), 0, "both exiled\n{}", log(&g));
    let c = g.card(0, "Vindicate", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(wave));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.permanents_of(1).count(), 2, "they return under their owner's control\n{}", log(&g));
}

/// Fading: at the beginning of your upkeep, remove a fade counter; if you
/// can't, sacrifice it (CR 702.32a).
#[test]
fn parallax_wave_fading_sacrificed_without_counters_cr702_32a() {
    let fade = engine::db::Counter::Named(db().subtype_id("counter:fade").unwrap());
    let mut g = scen().battlefield(0, &["Parallax Wave"]).counters(0, "Parallax Wave", fade, 5).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let wave = bf(&g, 0, "Parallax Wave");
    assert_eq!(g.counters_on(wave, fade), 5);
    let bear = bf(&g, 1, "Test Bear");
    for _ in 0..5 {
        g.choose(act(wave, 0));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
        if g.kind() == Some(DecisionKind::Target) {
            g.choose(g.tgt(bear));
        }
        settle(&mut g);
        if g.permanents_of(1).count() == 0 {
            break;
        }
    }
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.turn().step == Step::Main1 && at_priority(g));
    let left = g.counters_on(wave, fade);
    assert!(left == 3 || g.count_in(0, "Parallax Wave", Zone::Graveyard) == 1, "a fade counter removed ({left})\n{}", log(&g));
}

/// Saddle 3 (CR 702.171a: tap other creatures with total power 3 or more,
/// as a sorcery); "Whenever this creature attacks, reveal the top card of
/// your library and put it into your hand. You lose life equal to that
/// card's mana value if this creature isn't saddled. Otherwise, each
/// opponent loses that much life."
#[test]
fn caustic_bronco_saddled_makes_the_opponent_lose_cr702_171() {
    let mut g = scen()
        .battlefield(0, &["Caustic Bronco", "Test Bear", "Test Flyer"])
        .library(0, &["Test Giant", "Island"])
        .at(Step::Main1, 0)
        .start();
    let bronco = bf(&g, 0, "Caustic Bronco");
    g.choose(act(bronco, 0));
    resolve_with(&mut g, |_| None);
    assert!(g.obj(bf(&g, 0, "Test Bear")).tapped && g.obj(bf(&g, 0, "Test Flyer")).tapped, "2 + 2 power tapped");
    attack_with(&mut g, &["Caustic Bronco"]);
    settle(&mut g);
    assert_eq!(g.life(0), 20, "{}", log(&g));
    assert_eq!(g.life(1), 15, "the Giant's mana value");
    assert_eq!(g.count_in(0, "Test Giant", Zone::Hand), 1);
}

/// Not saddled: you lose the life.
#[test]
fn caustic_bronco_unsaddled_you_lose_cr702_171() {
    let mut g = scen().battlefield(0, &["Caustic Bronco"]).library(0, &["Test Giant", "Island"]).at(Step::Main1, 0).start();
    attack_with(&mut g, &["Caustic Bronco"]);
    settle(&mut g);
    assert_eq!(g.life(0), 15, "{}", log(&g));
}

/// "Whenever a 1/1 creature you control enters, you may return this card
/// from your graveyard to the battlefield, then attach it to that creature"
/// (an ability that functions in the graveyard).
#[test]
fn sword_of_the_meek_returns_from_the_graveyard_and_attaches_cr301_5() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Test Dies Shock"])
        .graveyard(0, &["Sword of the Meek"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Test Dies Shock", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::YesNo) { Some(Choice::Yes) } else { None });
    let sword = bf(&g, 0, "Sword of the Meek");
    let host = bf(&g, 0, "Test Dies Shock");
    assert_eq!(g.obj(sword).attached.map(|r| r.idx), Some(host), "{}", log(&g));
    assert_eq!(g.pt(host), (2, 3), "+1/+2");
}

/// On the battlefield the ability does nothing (it returns "this card from
/// your graveyard").
#[test]
fn sword_of_the_meek_on_the_battlefield_doesnt_trigger() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Sword of the Meek"])
        .hand(0, &["Test Dies Shock"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Test Dies Shock", Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(&mut g);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g) || g.kind() == Some(DecisionKind::YesNo));
    assert_ne!(g.kind(), Some(DecisionKind::YesNo), "{}", log(&g));
}
