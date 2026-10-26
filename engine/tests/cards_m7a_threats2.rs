//! M7a: amass (CR 701.47), persist (702.79), monstrosity (701.37), living
//! weapon / For Mirrodin! (702.92, 702.163), "choose a player" (614.12),
//! combat damage that can't be prevented, devotion (700.5) and winning by
//! an effect (104.2b).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn base() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn scen() -> Scenario {
    base().library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn pay(g: &mut Game) {
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(at_priority);
    }
}

fn cast_card(g: &mut Game, c: CardIdx) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana) || g.kind() == Some(DecisionKind::Target));
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    cast_card(g, c);
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(g);
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g) || g.is_over());
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress");
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

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

#[test]
fn orcish_bowmasters_pings_and_amasses_on_entering_and_on_extra_draws() {
    // ruling: d78e9d4c
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Orcish Bowmasters"])
        .battlefield(1, &["Island"])
        .hand(1, &["Ancestral Recall"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Orcish Bowmasters");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(player(1))).then(|| player(1)));
    assert_eq!(g.life(1), 19);
    let army = bf(&g, 0, "Orc Army");
    assert_eq!(g.pt(army), (1, 1));
    // P1's own draw step: the first draw doesn't trigger.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.life(1), 19);
    // Ancestral Recall: three more draws, three triggers.
    let ar = g.card(1, "Ancestral Recall", Zone::Hand);
    g.choose(Choice::Cast(ar));
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.pending_decision().unwrap().player == 1 => Some(player(1)),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.life(1), 16);
    assert_eq!(g.pt(army), (4, 4), "one Army: amass grows it");
    assert_eq!(g.count_in(0, "Orc Army", Zone::Battlefield), 1);
}

#[test]
fn questing_beast_combat_damage_cant_be_prevented_by_protection() {
    let mut g = scen()
        .battlefield(0, &["Questing Beast"])
        .battlefield(1, &["Mother of Runes", "Test Giant"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let qb = bf(&g, 0, "Questing Beast");
    let mom = bf(&g, 1, "Mother of Runes");
    let giant = bf(&g, 1, "Test Giant");
    g.choose(Choice::Card(qb));
    g.choose(Choice::Pass);
    g.drive_until(at_priority);
    // P0 passes; P1 gives the Giant protection from green.
    g.pass();
    g.choose(Choice::Ability(mom, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(giant)),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(4)),
        _ => None,
    });
    assert!(g.has_option(Choice::Pass));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers));
    // Mother (power 1) can't block it; the Giant can.
    g.choose(Choice::Card(qb));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g) || g.is_over());
    assert_eq!(g.obj(giant).zone, Zone::Graveyard, "4 deathtouch damage, not prevented: {}", g.log_lines().join("\n"));
}

#[test]
fn cr_614_12_true_name_nemesis_has_protection_from_the_chosen_player() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["True-Name Nemesis"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "True-Name Nemesis");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            assert!(g.has_option(player(0)) && g.has_option(player(1)), "either player");
            Some(player(1))
        }
        _ => None,
    });
    let tnn = bf(&g, 0, "True-Name Nemesis");
    assert_eq!(g.obj(tnn).chosen_player, Some(1));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| !at_priority(g));
    assert!(!g.has_option(g.tgt(tnn)), "can't be targeted by the chosen player's spells");
}

#[test]
fn cr_702_79_woodfall_primus_persists_once() {
    // ruling: 6363d351 e7319af2
    let mut g = scen()
        .battlefield(0, &["Woodfall Primus"])
        .battlefield(1, &["Swamp", "Swamp", "Swamp", "Swamp"])
        .hand(1, &["Snuff Out", "Snuff Out"])
        .at(Step::Main1, 1)
        .start();
    let wp = bf(&g, 0, "Woodfall Primus");
    for round in 0..2 {
        let so = g.card(1, "Snuff Out", Zone::Hand);
        g.choose(Choice::Cast(so));
        g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
        if g.kind() == Some(DecisionKind::ChooseAltCost) {
            g.choose(Choice::Number(1)); // 4 life
        }
        let wp_now = g.card(0, "Woodfall Primus", Zone::Battlefield);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) if g.has_option(g.tgt(wp_now)) => Some(g.tgt(wp_now)),
            Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass),
            _ => None,
        });
        if round == 0 {
            let back = bf(&g, 0, "Woodfall Primus");
            assert_eq!(back, wp, "the same card");
            assert_eq!(g.counters_on(back, Counter::M1M1), 1);
            assert_eq!(g.pt(back), (5, 5));
        } else {
            assert_eq!(g.obj(wp).zone, Zone::Graveyard, "it had a -1/-1 counter");
        }
    }
}

#[test]
fn carnage_interpreter_discards_investigates_and_grows_with_a_small_hand() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain"])
        .hand(0, &["Carnage Interpreter", "Island", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Carnage Interpreter");
    resolve_with(&mut g, |_| None);
    let ci = bf(&g, 0, "Carnage Interpreter");
    assert_eq!(g.player(0).hand.len(), 0);
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 4);
    assert_eq!(g.pt(ci), (5, 5));
    assert!(g.has_kw(ci, Kw::Menace));
}

#[test]
fn skyclave_apparition_exiles_and_its_leave_trigger_gives_an_illusion() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Skyclave Apparition"])
        .battlefield(1, &["Sol Ring", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 1, "Sol Ring");
    cast(&mut g, 0, "Skyclave Apparition");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(ring))).then(|| g.tgt(ring)));
    assert_eq!(g.obj(ring).zone, Zone::Exile);
    let sa = bf(&g, 0, "Skyclave Apparition");
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| !at_priority(g));
    g.choose(g.tgt(sa));
    g.drive_until(at_priority);
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(sa).zone, Zone::Graveyard);
    assert_eq!(g.obj(ring).zone, Zone::Exile, "the card stays exiled");
    let illusion = bf(&g, 1, "Illusion");
    assert_eq!(g.pt(illusion), (1, 1), "Sol Ring's mana value");
}

#[test]
fn cr_701_37_alpha_deathclaw_monstrosity_once_and_its_trigger() {
    // ruling: 51f91fae
    let mut g = scen()
        .battlefield(0, &["Alpha Deathclaw", "Swamp", "Forest", "Swamp", "Forest", "Swamp", "Forest", "Swamp"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let ad = bf(&g, 0, "Alpha Deathclaw");
    let ring = bf(&g, 1, "Sol Ring");
    g.choose(Choice::Ability(ad, AbilitySlot::Activated(0)));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(ring))).then(|| g.tgt(ring)));
    assert_eq!(g.pt(ad), (10, 10));
    assert!(g.obj(ad).monstrous);
    assert_eq!(g.obj(ring).zone, Zone::Graveyard, "becomes monstrous: destroy target permanent");
}

#[test]
fn living_weapon_nettlecyst_germ_counts_artifacts_and_enchantments() {
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Mox Pearl", "Island"])
        .hand(0, &["Nettlecyst"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Nettlecyst");
    resolve_with(&mut g, |_| None);
    let germ = bf(&g, 0, "Phyrexian Germ");
    let nc = bf(&g, 0, "Nettlecyst");
    assert_eq!(g.obj(nc).attached.map(|r| r.idx), Some(germ));
    assert_eq!(g.pt(germ), (3, 3), "Sol Ring, Mox Pearl, Nettlecyst");
}

#[test]
fn for_mirrodin_glimmer_lens_draws_when_its_creature_attacks_with_another() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Test Hasty"])
        .hand(0, &["Glimmer Lens"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Glimmer Lens");
    resolve_with(&mut g, |_| None);
    let rebel = bf(&g, 0, "Rebel");
    assert_eq!(g.pt(rebel), (2, 2));
    // Next turn (the Rebel is no longer summoning sick).
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 + 1 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let hasty = bf(&g, 0, "Test Hasty");
    let hand = g.player(0).hand.len();
    // (Attackers are declared in slot order.)
    g.choose(Choice::Card(hasty));
    g.choose(Choice::Card(rebel));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).hand.len(), hand + 1);
}

#[test]
fn cr_104_2b_thassas_oracle_wins_with_devotion_covering_the_library() {
    let mut g = base()
        .library(0, &["Island", "Island"])
        .library(1, &["Forest"; 10])
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Thassa's Oracle"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Thassa's Oracle");
    resolve_with(&mut g, |_| None);
    assert!(g.is_over());
    assert_eq!(g.outcome().unwrap().winner, Some(0));
    // With a bigger library it just looks.
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &["Thassa's Oracle"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Thassa's Oracle");
    resolve_with(&mut g, |_| None);
    assert!(!g.is_over());
    assert_eq!(g.player(0).library.len(), 10);
}
