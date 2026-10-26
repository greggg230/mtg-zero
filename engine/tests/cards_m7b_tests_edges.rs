//! M7b card tests, rules edges of cards first tested in M7b: 611.2c (a
//! continuous effect from a resolved ability affects only the objects there
//! as it resolves), 610.3c (a "until this leaves" exile from a source that
//! already left exiles nothing), a removal rider that doesn't depend on the
//! destruction, a mana source paying its own untap cost (601.2g / 605.3a),
//! knowledge after a hand reveal (701.16a), granted flashback ending,
//! tokens leaving (111.7 / 704.5d), "your second spell".

mod common;

use common::db;
use engine::db::{AbilitySlot, Kw};
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

fn pay(g: &mut Game) {
    let paying = |g: &Game| matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana));
    g.drive_until(|g| at_priority(g) || !paying(g));
    while paying(g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !paying(g));
    }
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(g);
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

// ---- CR 611.2c ---------------------------------------------------------------------

#[test]
fn cr_611_2c_harvester_of_misery_spares_creatures_that_enter_later() {
    // ruling: b300f11d
    // "Other creatures get -2/-2 until end of turn": the set of affected
    // creatures is fixed as the ability resolves.
    let mut g = scen()
        .battlefield(0, &["Swamp"; 5])
        .battlefield(0, &["Forest", "Forest"])
        .hand(0, &["Harvester of Misery", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Harvester of Misery");
    settle(&mut g);
    cast(&mut g, 0, "Test Bear");
    settle(&mut g);
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(bear), (2, 2), "entered after the ability resolved");
}

#[test]
fn cr_611_2c_spectacular_spider_man_protects_only_the_creatures_there() {
    // "Creatures you control gain hexproof and indestructible until end of
    // turn." Also: flash (cast in the opponent's turn).
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Forest", "Forest", "Island", "Test Reacher"])
        .hand(0, &["Spectacular Spider-Man", "Test Bear"])
        .at(Step::Upkeep, 1)
        .start();
    g.pass();
    assert_eq!(g.to_move(), Some(0));
    cast(&mut g, 0, "Spectacular Spider-Man");
    settle(&mut g);
    let ssm = bf(&g, 0, "Spectacular Spider-Man");
    // Back to P0's own turn to cast the Bear later.
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    g.choose(act(ssm, 1));
    pay(&mut g);
    settle(&mut g);
    let reacher = bf(&g, 0, "Test Reacher");
    assert!(g.has_kw(reacher, Kw::Hexproof) && g.has_kw(reacher, Kw::Indestructible));
    cast(&mut g, 0, "Test Bear");
    settle(&mut g);
    let bear = bf(&g, 0, "Test Bear");
    assert!(!g.has_kw(bear, Kw::Hexproof) && !g.has_kw(bear, Kw::Indestructible), "entered later");
}

#[test]
fn cr_611_2c_ravenous_robots_hastes_only_the_tokens_already_there() {
    // "{R}, {T}: Creature tokens you control gain haste until end of turn."
    let mut g = scen()
        .battlefield(0, &["Ravenous Robots", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Sol Ring", "Mox Pearl"])
        .at(Step::Main1, 0)
        .start();
    let rr = bf(&g, 0, "Ravenous Robots");
    cast(&mut g, 0, "Mox Pearl");
    settle(&mut g);
    let first: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&c| g.name(c) == "Robot").collect();
    assert_eq!(first.len(), 1);
    g.choose(act(rr, 0));
    pay(&mut g);
    settle(&mut g);
    assert!(g.has_kw(first[0], Kw::Haste));
    cast(&mut g, 0, "Sol Ring");
    settle(&mut g);
    let second: Vec<CardIdx> =
        g.battlefield().iter().copied().filter(|&c| g.name(c) == "Robot" && c != first[0]).collect();
    assert_eq!(second.len(), 1);
    assert!(!g.has_kw(second[0], Kw::Haste), "created after the ability resolved");
}

// ---- CR 610.3c ----------------------------------------------------------------------

#[test]
fn cr_610_3c_touch_the_spirit_realm_gone_before_its_trigger_resolves_exiles_nothing() {
    // ruling: 8ba35794
    // With the enters trigger on the stack, P0 sacrifices Cathar Commando to
    // destroy Touch the Spirit Realm; the trigger then exiles nothing.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Island", "Cathar Commando"])
        .hand(0, &["Touch the Spirit Realm"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let cc = bf(&g, 0, "Cathar Commando");
    cast(&mut g, 0, "Touch the Spirit Realm");
    // Resolve the spell; stop with the trigger (its target chosen) on the
    // stack.
    for _ in 0..50 {
        if g.stack().len() == 1
            && at_priority(&g)
            && g.to_move() == Some(0)
            && g.count_in(0, "Touch the Spirit Realm", Zone::Battlefield) == 1
        {
            break;
        }
        match g.kind() {
            Some(DecisionKind::Target) => {
                let t = g.tgt(bear);
                g.choose(t);
            }
            Some(DecisionKind::Priority) => {
                g.pass();
            }
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    assert_eq!(g.stack().len(), 1, "the enters trigger waits");
    let tsr = bf(&g, 0, "Touch the Spirit Realm");
    g.choose(act(cc, 0));
    g.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana)
    });
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(tsr));
    }
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(tsr).zone, Zone::Graveyard);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "610.3c: nothing exiled");
}

// ---- a rider independent of the destruction ---------------------------------------------

#[test]
fn erode_lets_the_controller_search_even_if_the_creature_isnt_destroyed() {
    // "Destroy target creature or planeswalker. Its controller may search
    // ..." The search doesn't depend on the destruction (no "if").
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Plains"])
        .hand(0, &["Erode"])
        .battlefield(1, &["Test Indestructible"])
        .library(0, &["Island"; 5])
        .library(1, &["Swamp", "Island"])
        .at(Step::Main1, 0)
        .start();
    let ind = bf(&g, 1, "Test Indestructible");
    let erode = g.card(0, "Erode", Zone::Hand);
    g.choose(Choice::Cast(erode));
    g.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana)
    });
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(ind));
    }
    pay(&mut g);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.obj(ind).zone, Zone::Battlefield, "indestructible");
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(1)), "its controller may still search");
    g.choose(Choice::Yes);
    g.choose(Choice::Def(def("Swamp")));
    settle(&mut g);
    assert!(g.obj(bf(&g, 1, "Swamp")).tapped);
}

// ---- Basalt Monolith pays its own untap cost ------------------------------------------------

#[test]
fn cr_605_3a_basalt_monolith_can_pay_for_its_own_untap_ability() {
    // "{3}: Untap this artifact." The cost doesn't tap it, so its own mana
    // ability may pay it (601.2g): it ends untapped, with no mana gained.
    let mut g = scen().battlefield(0, &["Basalt Monolith"]).at(Step::Main1, 0).start();
    let mono = bf(&g, 0, "Basalt Monolith");
    assert!(g.has_option(act(mono, 1)), "{:?}", g.options_desc());
    g.choose(act(mono, 1));
    pay(&mut g);
    assert!(g.obj(mono).tapped, "tapped for {{C}}{{C}}{{C}} to pay");
    settle(&mut g);
    assert!(!g.obj(mono).tapped);
}

// ---- 701.16a: Grief's reveal ---------------------------------------------------------------

#[test]
fn grief_reveals_the_opponents_hand_to_everyone() {
    let mut g = scen()
        .battlefield(0, &["Swamp"; 4])
        .hand(0, &["Grief"])
        .hand(1, &["Counterspell", "Island", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let island = g.card(1, "Island", Zone::Hand);
    let ring = g.card(1, "Sol Ring", Zone::Hand);
    assert!(!g.knows(0, island));
    cast(&mut g, 0, "Grief");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseDiscard) => {
            assert_eq!(g.pending_decision().unwrap().options.len(), 2, "Counterspell and Sol Ring; not the Island");
            None
        }
        _ => None,
    });
    assert!(g.knows(0, island), "revealed");
    assert_eq!(g.player(1).hand.len(), 2);
    assert!(g.player(1).hand.iter().all(|&c| g.knows(0, c)), "the rest of the hand stays known");
    let _ = ring;
}

// ---- Flashback's grant ends with the turn ---------------------------------------------------

#[test]
fn flashback_granted_by_flashback_ends_at_end_of_turn() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Flashback"])
        .graveyard(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    cast(&mut g, 0, "Flashback");
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(g.card(0, "Lightning Bolt", Zone::Graveyard)))
    });
    assert!(g.has_option(Choice::Cast(bolt)));
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 1 && g.to_move() == Some(0) && at_priority(g));
    assert!(!g.has_option(Choice::Cast(bolt)), "until end of turn");
}

// ---- tokens leaving --------------------------------------------------------------------------

#[test]
fn phelia_exiling_a_token_does_not_return_it() {
    // ruling: aded29b5
    // A token that leaves the battlefield ceases to exist (111.7, 704.5d):
    // "return that card" finds nothing.
    let mut g = scen()
        .battlefield(0, &["Phelia, Exuberant Shepherd", "Retrofitter Foundry", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let rf = bf(&g, 0, "Retrofitter Foundry");
    g.choose(act(rf, 1));
    pay(&mut g);
    settle(&mut g);
    let servo = bf(&g, 0, "Servo");
    let phelia = bf(&g, 0, "Phelia, Exuberant Shepherd");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(phelia));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bf(g, 0, "Servo"))));
    assert_eq!(g.obj(servo).zone, Zone::Nowhere);
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.count_in(0, "Servo", Zone::Battlefield), 0, "the token doesn't return");
    assert_eq!(g.counters_on(phelia, engine::db::Counter::P1P1), 0, "nothing entered");
}

// ---- Cori-Steel Cutter: your second spell -------------------------------------------------------

#[test]
fn cori_steel_cutter_counts_only_your_spells() {
    // P0 casts a spell, P1 responds with one: P0's next spell is P0's second
    // (flurry), P1's was not P0's.
    let mut g = scen()
        .battlefield(0, &["Cori-Steel Cutter", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = |g: &mut Game, p: PlayerId, t: PlayerId| {
        let b = g.card(p, "Lightning Bolt", Zone::Hand);
        g.choose(Choice::Cast(b));
        g.choose(player(t));
        pay(g);
    };
    bolt(&mut g, 0, 1);
    g.drive_until(at_priority);
    g.pass();
    assert_eq!(g.to_move(), Some(1));
    bolt(&mut g, 1, 0);
    g.drive_until(at_priority);
    assert!(g.waiting_triggers().is_empty() && g.stack().len() == 2, "P1's spell is not P0's second");
    settle(&mut g);
    assert_eq!(g.count_in(0, "Monk", Zone::Battlefield), 0);
    bolt(&mut g, 0, 1);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.count_in(0, "Monk", Zone::Battlefield), 1, "P0's second spell");
}

// ---- 601.2h for an activated ability ---------------------------------------------------------

#[test]
fn cr_602_2b_goblin_engineer_may_sacrifice_the_mox_that_paid_its_red() {
    // "{R}, {T}, Sacrifice an artifact: ..." Mox Ruby taps for {R} while the
    // cost is paid (605.3a), then is sacrificed (601.2h via 602.2b).
    let mut g = scen()
        .battlefield(0, &["Goblin Engineer", "Mox Ruby"])
        .graveyard(0, &["Mind Stone"])
        .at(Step::Main1, 0)
        .start();
    let ge = bf(&g, 0, "Goblin Engineer");
    let mox = bf(&g, 0, "Mox Ruby");
    assert!(g.has_option(act(ge, 0)), "the Mox pays {{R}} and is the artifact sacrificed");
    g.choose(act(ge, 0));
    g.drive_until(|g| {
        !matches!(g.kind(), Some(DecisionKind::Target)) || g.pending_decision().unwrap().options.len() > 1
    });
    let stone = g.card(0, "Mind Stone", Zone::Graveyard);
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(stone));
    }
    pay(&mut g);
    if g.kind() == Some(DecisionKind::Sacrifice) {
        g.choose(Choice::Card(mox));
    }
    settle(&mut g);
    assert_eq!(g.obj(mox).zone, Zone::Graveyard);
    assert_eq!(g.obj(stone).zone, Zone::Battlefield);
}

// ---- 702.34a: a flashback spell leaving the stack is exiled ---------------------------------

#[test]
fn cr_702_34a_reprieve_on_a_flashed_back_echo_of_eons_exiles_it() {
    // ruling: ef0edbdb
    // Reprieve would return the spell to its owner's hand; a spell cast
    // with flashback is exiled instead whenever it would leave the stack.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .graveyard(0, &["Echo of Eons"])
        .battlefield(1, &["Plains", "Plains"])
        .hand(1, &["Reprieve"])
        .at(Step::Main1, 0)
        .start();
    let echo = g.card(0, "Echo of Eons", Zone::Graveyard);
    g.choose(Choice::Cast(echo));
    pay(&mut g);
    g.drive_until(at_priority);
    g.pass();
    assert_eq!(g.to_move(), Some(1));
    let r = g.card(1, "Reprieve", Zone::Hand);
    g.choose(Choice::Cast(r));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(echo).zone, Zone::Exile, "exiled, not returned to hand");
    assert_eq!(g.player(0).hand.len(), 0, "Echo never resolved");
}

#[test]
fn echo_of_eons_cast_from_hand_ends_in_the_graveyard_after_the_shuffle() {
    // ruling: 65329593
    // It is on the stack while it resolves (608.2n: then put into its
    // owner's graveyard), so it isn't shuffled into the library.
    let mut g = scen()
        .battlefield(0, &["Island"; 6])
        .hand(0, &["Echo of Eons", "Lightning Bolt"])
        .graveyard(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let echo = g.card(0, "Echo of Eons", Zone::Hand);
    g.choose(Choice::Cast(echo));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(echo).zone, Zone::Graveyard);
    assert_eq!(g.player(0).graveyard.len(), 1, "only Echo itself");
    assert_eq!(g.player(0).hand.len(), 7);
    assert_eq!(g.player(0).library.len(), 10 + 2 - 7, "Bolt and the Bear shuffled in");
}

// ---- 614.1c however the land enters ----------------------------------------------------------

#[test]
fn cr_614_1c_crop_rotation_puts_a_triome_or_fast_land_in_as_it_would_enter() {
    for (land, others, tapped) in [
        ("Raugrin Triome", 1, true),
        ("Spirebluff Canal", 2, false),
        ("Spirebluff Canal", 3, true),
        ("Thundering Falls", 1, true),
    ] {
        let mut lands = vec!["Forest"];
        lands.extend(std::iter::repeat_n("Island", others));
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .battlefield(0, &lands)
            .hand(0, &["Crop Rotation"])
            .library(0, &[land, "Island", "Island"])
            .library(1, &["Forest"; 5])
            .at(Step::Main1, 0)
            .start();
        let cr = g.card(0, "Crop Rotation", Zone::Hand);
        g.choose(Choice::Cast(cr));
        g.drive_until(|g| {
            at_priority(g) || matches!(g.kind(), Some(DecisionKind::Sacrifice) | Some(DecisionKind::PayMana))
        });
        pay(&mut g);
        if g.kind() == Some(DecisionKind::Sacrifice) {
            // Sacrifice an Island (the Forest paid {G} or not; keep the
            // count of other lands as set).
            let island = g.battlefield().iter().copied().find(|&c| g.name(c) == "Island").unwrap();
            g.choose(Choice::Card(island));
        }
        g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
        g.choose(Choice::Def(def(land)));
        let c = bf(&g, 0, land);
        // The other lands it sees: `others` (the Forest plus Islands, one
        // Island sacrificed).
        assert_eq!(g.obj(c).tapped, tapped, "{land} with {others} other lands");
        settle(&mut g);
    }
}
