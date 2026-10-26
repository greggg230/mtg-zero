//! M7b card tests, creatures (A-I): Agent Bishop, Archon of Cruelty, Auriok
//! Salvagers, Bejeweled Warg, Bristly Bill, Cathar Commando, Figure of
//! Fable, Giver of Runes, Goblin Engineer, Grief, Harvester of Misery,
//! Haywire Mite, Iron-Shield Elf; each against its Oracle text.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
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

fn def(name: &str) -> u16 {
    db().id(name).unwrap()
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

fn subtype(g: &Game, c: CardIdx, s: &str) -> bool {
    db().subtype_id(s).is_some_and(|id| g.has_subtype(c, id))
}

/// Pay with the first source offered (and the first production) until the
/// payment is done.
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

/// Cast `c` from wherever it is, alternative cost `alt` if asked (0 = the
/// mana cost), then pay with the first sources.
fn cast_card(g: &mut Game, c: CardIdx, alt: u8) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::ChooseAltCost) || g.kind() == Some(DecisionKind::PayMana)
    });
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(alt));
    }
    pay(g);
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    cast_card(g, c, 0);
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

/// Attack with `attackers` (P0's declare attackers step must be pending),
/// no blocks, answer the rest with `answer`, until P0's second main phase.
fn attack(g: &mut Game, attackers: &[CardIdx], answer: impl Fn(&Game) -> Option<Choice>) {
    assert_eq!(g.kind(), Some(DecisionKind::DeclareAttackers));
    let mut sorted = attackers.to_vec();
    sorted.sort_by_key(|&c| g.battlefield().iter().position(|&x| x == c));
    for &a in &sorted {
        g.choose(Choice::Card(a));
    }
    g.choose(Choice::Pass);
    for _ in 0..500 {
        if g.turn().step == Step::Main2 && at_priority(g) && g.stack().is_empty() {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => answer(g).unwrap_or(d.options[0].choice),
        };
        g.choose(c);
    }
    panic!("attack: no progress");
}

// ---- Agent Bishop ------------------------------------------------------------

#[test]
fn agent_bishop_counters_up_to_two_target_creatures_at_your_beginning_of_combat() {
    // "At the beginning of combat on your turn, put a +1/+1 counter on each
    // of up to two target creatures."
    let mut g = scen()
        .battlefield(0, &["Agent Bishop, Man in Black", "Test Bear"])
        .battlefield(1, &["Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    let bishop = bf(&g, 0, "Agent Bishop, Man in Black");
    let bear = bf(&g, 0, "Test Bear");
    let reacher = bf(&g, 1, "Test Reacher");
    assert_eq!(g.pt(bishop), (1, 2));
    assert!(g.is_legendary(bishop));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(g.turn().step, Step::BeginCombat);
    assert!(g.has_option(g.tgt(bishop)) && g.has_option(g.tgt(reacher)), "any creatures");
    assert!(g.has_option(Choice::Pass), "up to two");
    g.choose(g.tgt(bear));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bear)), "two different creatures (one word 'target')");
    g.choose(g.tgt(reacher));
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    assert_eq!(g.counters_on(reacher, Counter::P1P1), 1);
    assert_eq!(g.counters_on(bishop, Counter::P1P1), 0);
    // Not at the beginning of the opponent's combat.
    let t0 = g.turn().number;
    for _ in 0..2000 {
        if g.turn().number == t0 + 2 {
            break;
        }
        let d = g.pending_decision().unwrap();
        assert!(d.kind != DecisionKind::Target || g.turn().active == 0, "no trigger on P1's turn");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
}

// ---- Archon of Cruelty ---------------------------------------------------------

#[test]
fn archon_of_cruelty_enters_or_attacks_edict_discard_drain_and_draw() {
    // "Flying. Whenever this creature enters or attacks, target opponent
    // sacrifices a creature or planeswalker of their choice, discards a
    // card, and loses 3 life. You draw a card and gain 3 life."
    let mut g = scen()
        .battlefield(0, &["Swamp"; 8])
        .hand(0, &["Archon of Cruelty"])
        .battlefield(1, &["Test Bear", "Test Reacher", "Sol Ring"])
        .hand(1, &["Island", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Archon of Cruelty");
    let reacher = bf(&g, 1, "Test Reacher");
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    let hand0 = g.player(0).hand.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            assert!(!g.has_option(player(0)), "target opponent");
            Some(player(1))
        }
        Some(DecisionKind::Sacrifice) => {
            assert_eq!(g.to_move(), Some(1), "of their choice");
            assert!(!g.has_option(Choice::Card(bf(g, 1, "Sol Ring"))), "a creature or planeswalker");
            Some(Choice::Card(bf(g, 1, "Test Reacher")))
        }
        Some(DecisionKind::Discard) => {
            assert_eq!(g.to_move(), Some(1));
            Some(Choice::Card(g.card(1, "Lightning Bolt", Zone::Hand)))
        }
        _ => None,
    });
    let archon = bf(&g, 0, "Archon of Cruelty");
    assert_eq!(g.pt(archon), (6, 6));
    assert!(g.has_kw(archon, Kw::Flying));
    assert_eq!(g.obj(reacher).zone, Zone::Graveyard);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.player(1).hand.len(), 1);
    assert_eq!(g.life(1), 17);
    assert_eq!(g.life(0), 23);
    assert_eq!(g.player(0).hand.len(), hand0 + 1);
    // Attacks: the same again (P1 has the Bear and the Island left).
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let (hand1, gy1) = (g.player(1).hand.len(), g.player(1).graveyard.len());
    attack(&mut g, &[archon], |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(1)));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.player(1).hand.len(), hand1 - 1, "discarded a card");
    assert_eq!(g.player(1).graveyard.len(), gy1 + 2, "the Bear and the discarded card");
    assert_eq!(g.life(1), 17 - 3 - 6, "3 life lost and 6 combat damage");
    assert_eq!(g.life(0), 26);
}

#[test]
fn archon_of_cruelty_against_an_empty_board_still_discards_and_drains() {
    let mut g = scen().battlefield(0, &["Swamp"; 8]).hand(0, &["Archon of Cruelty"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Archon of Cruelty");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(1)));
    assert_eq!((g.life(0), g.life(1)), (23, 17));
}

// ---- Auriok Salvagers ----------------------------------------------------------

#[test]
fn auriok_salvagers_returns_an_artifact_card_with_mana_value_one_or_less() {
    // "{1}{W}: Return target artifact card with mana value 1 or less from
    // your graveyard to your hand."
    let mut g = scen()
        .battlefield(0, &["Auriok Salvagers", "Plains", "Plains", "Plains", "Plains"])
        .graveyard(0, &["Sol Ring", "Mox Pearl", "Mind Stone", "Lightning Bolt"])
        .graveyard(1, &["Mox Jet"])
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, "Auriok Salvagers");
    assert_eq!(g.pt(s), (2, 4));
    let ring = g.card(0, "Sol Ring", Zone::Graveyard);
    let pearl = g.card(0, "Mox Pearl", Zone::Graveyard);
    g.choose(act(s, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(ring)) && g.has_option(g.tgt(pearl)));
    assert!(!g.has_option(g.tgt(g.card(0, "Mind Stone", Zone::Graveyard))), "mana value 2");
    assert!(!g.has_option(g.tgt(g.card(0, "Lightning Bolt", Zone::Graveyard))), "not an artifact");
    assert!(!g.has_option(g.tgt(g.card(1, "Mox Jet", Zone::Graveyard))), "your graveyard");
    g.choose(g.tgt(ring));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(ring).zone, Zone::Hand);
    // No {T}: again, for the Mox.
    g.choose(act(s, 0));
    g.choose(g.tgt(pearl));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(pearl).zone, Zone::Hand);
}

// ---- Bejeweled Warg --------------------------------------------------------------

#[test]
fn bejeweled_warg_combat_damage_to_a_player_counter_on_a_wolf_or_a_treasure() {
    // "Trample. Whenever this creature deals combat damage to a player,
    // choose one - Put a +1/+1 counter on target Wolf you control; or Create
    // a Treasure token."
    for mode in 0..2u8 {
        let mut g = scen().battlefield(0, &["Bejeweled Warg"]).at(Step::DeclareAttackers, 0).start();
        let warg = bf(&g, 0, "Bejeweled Warg");
        assert_eq!(g.pt(warg), (3, 2));
        assert!(g.has_kw(warg, Kw::Trample));
        attack(&mut g, &[warg], |g| match g.kind() {
            Some(DecisionKind::ChooseMode) => {
                assert!(g.has_option(Choice::Number(0)) && g.has_option(Choice::Number(1)));
                Some(Choice::Number(mode))
            }
            Some(DecisionKind::Target) => Some(g.tgt(bf(g, 0, "Bejeweled Warg"))),
            _ => None,
        });
        assert_eq!(g.life(1), 17);
        if mode == 0 {
            assert_eq!(g.counters_on(warg, Counter::P1P1), 1);
            assert_eq!(g.count_in(0, "Treasure", Zone::Battlefield), 0);
        } else {
            assert_eq!(g.counters_on(warg, Counter::P1P1), 0);
            let t = bf(&g, 0, "Treasure");
            assert!(g.is_type(t, engine::db::types::ARTIFACT));
        }
    }
    // Blocked and dealing no damage to the player: no trigger.
    let mut g =
        scen().battlefield(0, &["Bejeweled Warg"]).battlefield(1, &["Test Wall"]).at(Step::DeclareAttackers, 0).start();
    let warg = bf(&g, 0, "Bejeweled Warg");
    let wall = bf(&g, 1, "Test Wall");
    g.choose(Choice::Card(warg));
    g.choose(Choice::Pass);
    for _ in 0..200 {
        if g.turn().step == Step::Main2 {
            break;
        }
        let d = g.pending_decision().unwrap();
        assert_ne!(d.kind, DecisionKind::ChooseMode, "no combat damage to a player");
        let c = match d.kind {
            DecisionKind::DeclareBlockers => {
                if g.has_option(Choice::Card(warg)) {
                    Choice::Card(warg)
                } else {
                    Choice::Pass
                }
            }
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.obj(wall).damage, 3);
    assert_eq!(g.life(1), 20, "3 power, 4 toughness wall: no trample excess");
}

// ---- Bristly Bill ------------------------------------------------------------------

#[test]
fn bristly_bill_landfall_counter_and_doubling() {
    // "Landfall - Whenever a land you control enters, put a +1/+1 counter on
    // target creature. {3}{G}{G}: Double the number of +1/+1 counters on
    // each creature you control."
    let mut g = scen()
        .battlefield(0, &["Bristly Bill, Spine Sower", "Test Bear"])
        .battlefield(0, &["Forest"; 5])
        .battlefield(1, &["Test Reacher"])
        .hand(0, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let bill = bf(&g, 0, "Bristly Bill, Spine Sower");
    let bear = bf(&g, 0, "Test Bear");
    let reacher = bf(&g, 1, "Test Reacher");
    let f = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(reacher)), "target creature: any");
    g.choose(g.tgt(bear));
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    g.choose(act(bill, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 2, "doubled");
    assert_eq!(g.counters_on(bill, Counter::P1P1), 0);
    assert_eq!(g.pt(bear), (4, 4));
    // An opponent's land entering doesn't trigger it.
    let mut g = scen().battlefield(0, &["Bristly Bill, Spine Sower"]).hand(1, &["Forest"]).at(Step::Main1, 1).start();
    let f = g.card(1, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert!(at_priority(&g) && g.stack().is_empty(), "a land you control");
}

// ---- Cathar Commando --------------------------------------------------------------

#[test]
fn cathar_commando_has_flash_and_sacrifices_to_destroy_an_artifact_or_enchantment() {
    // "Flash. {1}, Sacrifice this creature: Destroy target artifact or
    // enchantment."
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Cathar Commando"])
        .battlefield(1, &["Sol Ring", "Oath of Druids", "Test Bear"])
        .at(Step::Upkeep, 1)
        .start();
    g.pass(); // P1 passes in their upkeep: P0 may cast it (flash)
    assert_eq!(g.to_move(), Some(0));
    cast(&mut g, 0, "Cathar Commando");
    settle(&mut g);
    let cc = bf(&g, 0, "Cathar Commando");
    assert_eq!(g.pt(cc), (3, 1));
    g.drive_until(|g| g.to_move() == Some(0) && at_priority(g));
    g.choose(act(cc, 0));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Test Bear"))), "an artifact or enchantment");
    assert!(g.has_option(g.tgt(bf(&g, 1, "Sol Ring"))));
    let oath = bf(&g, 1, "Oath of Druids");
    g.choose(g.tgt(oath));
    pay(&mut g);
    assert_eq!(g.obj(cc).zone, Zone::Graveyard, "sacrificed as a cost");
    settle(&mut g);
    assert_eq!(g.obj(oath).zone, Zone::Graveyard);
}

// ---- Figure of Fable --------------------------------------------------------------

#[test]
fn figure_of_fable_levels_up_scout_soldier_avatar() {
    // ruling: 5bbf98e3 bfa9cd25
    let mut g = scen()
        .battlefield(0, &["Figure of Fable"])
        .battlefield(0, &["Forest"; 13])
        .hand(1, &["Lightning Bolt"])
        .battlefield(1, &["Mountain"])
        .at(Step::Main1, 0)
        .start();
    let fig = bf(&g, 0, "Figure of Fable");
    assert_eq!(g.pt(fig), (1, 1));
    assert!(subtype(&g, fig, "Kithkin"));
    // "{1}{G/W}{G/W}: If this creature is a Scout, it becomes ..." Not a
    // Scout yet: nothing happens.
    g.choose(act(fig, 1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.pt(fig), (1, 1), "not a Scout");
    // "{G/W}: This creature becomes a Kithkin Scout with base power and
    // toughness 2/3."
    g.choose(act(fig, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.pt(fig), (2, 3));
    assert!(subtype(&g, fig, "Kithkin") && subtype(&g, fig, "Scout"));
    g.choose(act(fig, 1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.pt(fig), (4, 5));
    assert!(subtype(&g, fig, "Soldier") && !subtype(&g, fig, "Scout"), "205.1b: its creature types become these");
    // "{3}{G/W}{G/W}{G/W}: If this creature is a Soldier, it becomes a
    // Kithkin Avatar with base power and toughness 7/8 and protection from
    // each of your opponents."
    g.choose(act(fig, 2));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.pt(fig), (7, 8));
    assert!(subtype(&g, fig, "Avatar") && !subtype(&g, fig, "Soldier"));
    assert!(g.protection_of(fig).opponents);
    // It lasts (no "until end of turn") and the opponent can't target it.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 1 && g.turn().step == Step::Main1 && g.to_move() == Some(1));
    assert_eq!(g.pt(fig), (7, 8));
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    assert!(!g.has_option(g.tgt(fig)), "protection from each of your opponents");
}

// ---- Giver of Runes -----------------------------------------------------------------

#[test]
fn giver_of_runes_gives_another_creature_you_control_protection_from_a_color() {
    // "{T}: Another target creature you control gains protection from
    // colorless or from the color of your choice until end of turn."
    let mut g = scen()
        .battlefield(0, &["Giver of Runes", "Test Bear"])
        .battlefield(1, &["Test Reacher", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let giver = bf(&g, 0, "Giver of Runes");
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(giver), (1, 2));
    g.choose(act(giver, 0));
    if g.kind() == Some(DecisionKind::Target) {
        assert!(!g.has_option(g.tgt(giver)), "another");
        assert!(!g.has_option(g.tgt(bf(&g, 1, "Test Reacher"))), "you control");
        g.choose(g.tgt(bear));
    }
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::ChooseColor));
    assert_eq!(g.pending_decision().unwrap().options.len(), 6, "five colours or colorless");
    g.choose(Choice::Number(3)); // red
    settle(&mut g);
    assert!(g.obj(giver).tapped);
    assert_eq!(g.protection_of(bear).colors, 1 << engine::mana::R);
    // P1's Bolt can't target it (702.16b).
    g.pass();
    assert_eq!(g.to_move(), Some(1));
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    assert!(!g.has_option(g.tgt(bear)), "protection from red");
    assert!(g.has_option(g.tgt(giver)));
}

// ---- Goblin Engineer -----------------------------------------------------------------

#[test]
fn goblin_engineer_entombs_an_artifact_and_reanimates_a_cheap_one() {
    // "When this creature enters, you may search your library for an
    // artifact card, put it into your graveyard, then shuffle."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Goblin Engineer"])
        .library(0, &["Island", "Mind Stone", "Lightning Bolt", "Kaldra Compleat"])
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Goblin Engineer");
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "you may");
    g.choose(Choice::Yes);
    let d = g.pending_decision().unwrap();
    assert_eq!(d.kind, DecisionKind::SearchLibrary);
    let mut offered: Vec<u16> =
        d.options.iter().filter_map(|o| if let Choice::Def(x) = o.choice { Some(x) } else { None }).collect();
    offered.sort();
    let mut want = vec![def("Mind Stone"), def("Kaldra Compleat")];
    want.sort();
    assert_eq!(offered, want, "artifact cards");
    g.choose(Choice::Def(def("Mind Stone")));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Mind Stone", Zone::Graveyard), 1);
    // "{R}, {T}, Sacrifice an artifact: Return target artifact card with
    // mana value 3 or less from your graveyard to the battlefield."
    let mut g = scen()
        .battlefield(0, &["Goblin Engineer", "Mountain", "Sol Ring"])
        .graveyard(0, &["Mind Stone", "Kaldra Compleat"])
        .graveyard(1, &["Grim Monolith"])
        .at(Step::Main1, 0)
        .start();
    let ge = bf(&g, 0, "Goblin Engineer");
    let ring = bf(&g, 0, "Sol Ring");
    let stone = g.card(0, "Mind Stone", Zone::Graveyard);
    g.choose(act(ge, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(g.card(0, "Kaldra Compleat", Zone::Graveyard))), "mana value 7");
    assert!(!g.has_option(g.tgt(g.card(1, "Grim Monolith", Zone::Graveyard))), "your graveyard");
    g.choose(g.tgt(stone));
    g.drive_until(|g| {
        at_priority(g) || matches!(g.kind(), Some(DecisionKind::Sacrifice) | Some(DecisionKind::PayMana))
    });
    pay(&mut g);
    if g.kind() == Some(DecisionKind::Sacrifice) {
        g.choose(Choice::Card(ring));
    }
    g.drive_until(at_priority);
    assert_eq!(g.obj(ring).zone, Zone::Graveyard, "sacrificed");
    settle(&mut g);
    assert_eq!(g.obj(stone).zone, Zone::Battlefield);
    assert_eq!(g.obj(stone).controller, 0);
}

// ---- Grief ------------------------------------------------------------------------------

#[test]
fn grief_evoked_takes_a_nonland_card_from_the_revealed_hand_then_is_sacrificed() {
    // "Menace. When this creature enters, target opponent reveals their
    // hand. You choose a nonland card from it. That player discards that
    // card. Evoke-Exile a black card from your hand."
    let mut g = scen()
        .hand(0, &["Grief", "Dark Ritual", "Lightning Bolt"])
        .hand(1, &["Counterspell", "Island"])
        .at(Step::Main1, 0)
        .start();
    let grief = g.card(0, "Grief", Zone::Hand);
    g.choose(Choice::Cast(grief));
    g.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::ChooseAltCost) || g.kind() == Some(DecisionKind::ExileCost)
    });
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(1));
    }
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::ExileCost));
    assert_eq!(g.kind(), Some(DecisionKind::ExileCost));
    let ritual = g.card(0, "Dark Ritual", Zone::Hand);
    assert!(!g.has_option(Choice::Card(g.card(0, "Lightning Bolt", Zone::Hand))), "a black card");
    g.choose(Choice::Card(ritual));
    assert_eq!(g.obj(ritual).zone, Zone::Exile);
    let cs = g.card(1, "Counterspell", Zone::Hand);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseDiscard) => {
            assert_eq!(g.to_move(), Some(0), "you choose");
            assert_eq!(g.pending_decision().unwrap().options.len(), 1, "nonland: the Island isn't offered");
            None
        }
        _ => None,
    });
    assert_eq!(g.obj(cs).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Island", Zone::Hand), 1);
    assert_eq!(g.obj(grief).zone, Zone::Graveyard, "evoked: sacrificed");
}

#[test]
fn grief_hard_cast_stays_with_menace() {
    let mut g =
        scen().battlefield(0, &["Swamp"; 4]).hand(0, &["Grief"]).hand(1, &["Island"]).at(Step::Main1, 0).start();
    let grief = g.card(0, "Grief", Zone::Hand);
    cast_card(&mut g, grief, 0);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(1)));
    assert_eq!(g.obj(grief).zone, Zone::Battlefield);
    assert_eq!(g.pt(grief), (3, 2));
    assert!(g.has_kw(grief, Kw::Menace));
    assert_eq!(g.count_in(1, "Island", Zone::Hand), 1, "only a land: nothing discarded");
}

// ---- Harvester of Misery -----------------------------------------------------------------

#[test]
fn harvester_of_misery_shrinks_other_creatures_and_is_a_removal_from_hand() {
    // "Menace. When this creature enters, other creatures get -2/-2 until
    // end of turn."
    let mut g = scen()
        .battlefield(0, &["Swamp"; 5])
        .battlefield(0, &["Test Reacher"])
        .hand(0, &["Harvester of Misery"])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Harvester of Misery");
    settle(&mut g);
    let h = bf(&g, 0, "Harvester of Misery");
    assert_eq!(g.pt(h), (5, 4), "not itself");
    assert!(g.has_kw(h, Kw::Menace));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1, "2/2: dies (704.5f)");
    assert_eq!(g.pt(bf(&g, 0, "Test Reacher")), (-1, 1), "yours too");
    assert_eq!(g.pt(bf(&g, 1, "Test Giant")), (3, 3));
    // Until end of turn.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 1 && at_priority(g));
    assert_eq!(g.pt(bf(&g, 1, "Test Giant")), (5, 5));
    // "{1}{B}, Discard this card: Target creature gets -2/-2 until end of
    // turn."
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Harvester of Misery"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Upkeep, 0)
        .start();
    let h = g.card(0, "Harvester of Misery", Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Ability(h, AbilitySlot::Hand(0)));
    g.choose(g.tgt(bear));
    pay(&mut g);
    assert_eq!(g.obj(h).zone, Zone::Graveyard, "discarded as a cost");
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

// ---- Haywire Mite --------------------------------------------------------------------------

#[test]
fn haywire_mite_exiles_a_noncreature_artifact_or_enchantment_and_gains_two_on_death() {
    // "When this creature dies, you gain 2 life. {G}, Sacrifice this
    // creature: Exile target noncreature artifact or noncreature
    // enchantment."
    let mut g = scen()
        .battlefield(0, &["Haywire Mite", "Forest"])
        .battlefield(1, &["Sol Ring", "Oath of Druids", "Ravenous Robots"])
        .at(Step::Main1, 0)
        .start();
    let mite = bf(&g, 0, "Haywire Mite");
    assert_eq!(g.pt(mite), (1, 1));
    assert!(g.is_type(mite, engine::db::types::ARTIFACT) && g.is_creature(mite));
    let ring = bf(&g, 1, "Sol Ring");
    g.choose(act(mite, 0));
    assert!(g.has_option(g.tgt(bf(&g, 1, "Oath of Druids"))), "a noncreature enchantment");
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Ravenous Robots"))), "an artifact creature is not");
    g.choose(g.tgt(ring));
    pay(&mut g);
    assert_eq!(g.obj(mite).zone, Zone::Graveyard);
    settle(&mut g);
    assert_eq!(g.obj(ring).zone, Zone::Exile);
    assert_eq!(g.life(0), 22, "it died (sacrificed): gain 2 life");
}

// ---- Iron-Shield Elf ------------------------------------------------------------------------

#[test]
fn iron_shield_elf_discards_for_indestructible_and_taps() {
    // "Discard a card: This creature gains indestructible until end of
    // turn. Tap it."
    let mut g = scen()
        .battlefield(0, &["Iron-Shield Elf"])
        .hand(0, &["Island"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let elf = bf(&g, 0, "Iron-Shield Elf");
    assert_eq!(g.pt(elf), (3, 1));
    g.choose(act(elf, 0));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::DiscardCost));
    if g.kind() == Some(DecisionKind::DiscardCost) {
        let island = g.card(0, "Island", Zone::Hand);
        g.choose(Choice::Card(island));
    }
    assert_eq!(g.count_in(0, "Island", Zone::Graveyard), 1);
    settle(&mut g);
    assert!(g.has_kw(elf, Kw::Indestructible));
    assert!(g.obj(elf).tapped, "tap it");
    // Lethal damage doesn't destroy it (702.12b).
    g.pass();
    g.drive_until(|g| g.to_move() == Some(1) && at_priority(g));
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(g.tgt(elf));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(elf).zone, Zone::Battlefield);
    // With an empty hand it can't be activated.
    let g = scen().battlefield(0, &["Iron-Shield Elf"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Iron-Shield Elf"), 0)));
}
