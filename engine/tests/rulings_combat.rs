//! M8d part 4 (ENGINE.md 40): the cube's combat rulings left after part 3:
//! creatures put onto the battlefield attacking were never declared as
//! attackers (CR 508.4: no attack triggers, no boast, exalted unchanged)
//! and their controller chooses what each attacks; ninjutsu's timing and
//! what the ninja attacks (702.49); what can attack: control since the
//! turn began for lands and Vehicles made creatures (302.6), haste lost,
//! "attacks if able" (508.1d); blocks that stand once declared (509.1h,
//! 506.4); Equipment untapped while its creature attacks; separate damage
//! events of a repeated mode; exert's untap. `ruling: <hash8>` names the
//! ruling (docs/rulings.md).

mod common;
mod rulings_util;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn ab(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn pay(g: &mut Game) {
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::PayPool)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// The first option of a payment decision that doesn't use `avoid`.
fn pay_avoiding(g: &Game, avoid: &[CardIdx]) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| match c {
        Choice::Card(x) | Choice::Ability(x, _) => !avoid.contains(&x),
        _ => true,
    })
}

fn first_non_pass(g: &Game) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass)
}

/// Declare `attackers` (at the defending player) and pass.
fn attack(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    let defender = 1 - g.turn().active;
    for &a in attackers {
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(defender));
        }
    }
    g.choose(Choice::Pass);
}

/// Answer `Target` decisions with `t` when it is an option (else first
/// options) and resolve the stack.
fn resolve_at(g: &mut Game, t: CardIdx) {
    let c = g.tgt(t);
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(c)).then_some(c));
}

/// P`p` casts `name` from hand, answering targets with `t`, and everything
/// resolves.
fn cast_at(g: &mut Game, p: u8, name: &str, t: CardIdx) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    resolve_at(g, t);
}

/// Pass (declining attacks and blocks, first options else) until `step`
/// with an empty stack and P`p` to act.
fn to_step(g: &mut Game, step: Step, p: u8) {
    quiet_until(g, |g| g.turn().step == step && g.stack().is_empty() && at_priority(g) && g.turn().priority == p);
}

const TAMIYO: &str = "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar";

/// P1's turn 3: P1 activates Tamiyo, Seasoned Scholar's +2 ("until your
/// next turn, whenever a creature attacks you or a planeswalker you
/// control, it gets -1/-0 until end of turn"); then P0's turn 4 with `p0`
/// on the battlefield (`lib0` its library, `hand0` its hand), stopped at
/// P0's declare attackers decision.
fn under_tamiyo(p0: &[&str], lib0: &[&str], hand0: &[&str]) -> Game {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, lib0)
        .library(1, &["Forest"; 10])
        .battlefield(0, p0)
        .hand(0, hand0)
        .battlefield(1, &[TAMIYO])
        .transformed(1, TAMIYO)
        .at(Step::Main1, 1)
        .start();
    let t = bf(&g, 1, "Tamiyo, Seasoned Scholar");
    g.choose(ab(t, 0));
    resolve(&mut g);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::DeclareAttackers));
    g
}

// ---- put onto the battlefield attacking: never declared (CR 508.4) -----------------------------

#[test]
fn creatures_put_onto_the_battlefield_attacking_dont_trigger_attack_abilities_cr_508_4() {
    // ruling: c97738b6 d1f9751e 8a2d01a9 45eda9dd 2de2fb41
    // Under P1's Tamiyo +2: Voice of Victory attacks alone (Noble
    // Hierarch's exalted triggers) and mobilizes two Warriors; Adeline's
    // Human, Gut's Skeleton (a Test Bear sacrificed), Raph & Mikey's Test
    // Bear: the creatures declared get -1/-0, those put onto the
    // battlefield attacking don't. The exalted bonus resolves though three
    // creatures attack by then.
    let mut g = under_tamiyo(&["Voice of Victory", "Noble Hierarch"], &["Island"; 10], &[]);
    let v = bf(&g, 0, "Voice of Victory");
    attack(&mut g, &[v]);
    to_step(&mut g, Step::DeclareAttackers, 0);
    let w = tokens_named(&g, "Warrior");
    assert_eq!(w.len(), 2, "{}", log(&g));
    for &t in &w {
        assert!(g.obj(t).attacking);
        assert_eq!(g.pt(t), (1, 1), "a Warrior wasn't declared: no -1/-0\n{}", log(&g));
    }
    assert_eq!(g.pt(v), (1, 4), "Voice: 1/3, -1/-0, exalted +1/+1\n{}", log(&g));
    // Adeline (2 creatures: power 2) and Gut.
    let mut g = under_tamiyo(&["Adeline, Resplendent Cathar", "Gut, True Soul Zealot", "Test Bear"], &["Island"; 10], &[]);
    let (ad, gut, bear) = (bf(&g, 0, "Adeline, Resplendent Cathar"), bf(&g, 0, "Gut, True Soul Zealot"), bf(&g, 0, "Test Bear"));
    attack(&mut g, &[ad, gut]);
    quiet_until_with(&mut g, |g| g.turn().step == Step::DeclareAttackers && g.stack().is_empty() && at_priority(g) && g.turn().priority == 0, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::Sacrifice) | Some(DecisionKind::ChoosePermanent) if g.has_option(Choice::Card(bear)) => Some(Choice::Card(bear)),
        _ => None,
    });
    let human = tokens_named(&g, "Human")[0];
    let skel = tokens_named(&g, "Skeleton")[0];
    assert_eq!(g.pt(human), (1, 1), "the Human: no -1/-0\n{}", log(&g));
    assert_eq!(g.pt(skel), (4, 1), "the Skeleton: no -1/-0\n{}", log(&g));
    assert_eq!(g.pt(gut), (1, 2), "Gut was declared");
    // Raph & Mikey.
    let mut g = under_tamiyo(&["Raph & Mikey, Troublemakers"], &["Island", "Test Bear", "Island", "Island"], &[]);
    let rm = bf(&g, 0, "Raph & Mikey, Troublemakers");
    attack(&mut g, &[rm]);
    to_step(&mut g, Step::DeclareAttackers, 0);
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.obj(bear).attacking);
    assert_eq!(g.pt(bear), (2, 2), "{}", log(&g));
    assert_eq!(g.pt(rm), (6, 7));
}

#[test]
fn a_ninja_put_onto_the_battlefield_attacking_wasnt_declared_cr_508_4_702_49c() {
    // ruling: eed15a5f 6995549f
    // Under P1's Tamiyo +2: Kaito and Fallen Shinobi enter by ninjutsu: no
    // -1/-0.
    for (ninja, pt) in [("Kaito, Bane of Nightmares", (3, 4)), ("Fallen Shinobi", (5, 4))] {
        let mut g = under_tamiyo(&["Test Bear", "Island", "Island", "Swamp", "Swamp"], &["Island"; 10], &[ninja]);
        let bear = bf(&g, 0, "Test Bear");
        attack(&mut g, &[bear]);
        quiet_until(&mut g, |g| g.turn().step == Step::DeclareBlockers && at_priority(g) && g.turn().priority == 0);
        assert_eq!(g.pt(bear), (1, 2), "the Bear was declared");
        let n = g.card(0, ninja, Zone::Hand);
        ninjutsu(&mut g, n, bear);
        resolve(&mut g);
        let n = bf(&g, 0, ninja);
        assert!(g.obj(n).attacking);
        assert_eq!(g.pt(n), pt, "{ninja}: not declared, no -1/-0\n{}", log(&g));
    }
}

/// Activate `ninja`'s ninjutsu (in hand), returning `back`, paying with
/// first sources.
fn ninjutsu(g: &mut Game, ninja: CardIdx, back: CardIdx) {
    g.choose(Choice::Ability(ninja, AbilitySlot::Hand(0)));
    for _ in 0..30 {
        if at_priority(g) {
            return;
        }
        let c = if g.has_option(Choice::Card(back)) { Choice::Card(back) } else { g.pending_decision().unwrap().options[0].choice };
        g.choose(c);
    }
    panic!("ninjutsu: no progress\n{}", log(g));
}

#[test]
fn boast_cant_be_activated_by_a_creature_put_onto_the_battlefield_attacking_cr_702_142a_508_4() {
    // ruling: 931f5f75
    // Raph & Mikey reveal Broadside Bombardiers: it enters attacking but
    // never attacked: no boast. (Declared, it may: the Oracle test.)
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Broadside Bombardiers", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Raph & Mikey, Troublemakers", "Sol Ring"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let rm = bf(&g, 0, "Raph & Mikey, Troublemakers");
    attack(&mut g, &[rm]);
    to_step(&mut g, Step::DeclareAttackers, 0);
    let bb = bf(&g, 0, "Broadside Bombardiers");
    assert!(g.obj(bb).attacking, "{}", log(&g));
    assert!(!g.has_option(ab(bb, 0)), "never declared: no boast: {}", opts(&g));
    to_step(&mut g, Step::Main2, 0);
    assert!(!g.has_option(ab(bb, 0)), "{}", opts(&g));
}

#[test]
fn each_creature_put_onto_the_battlefield_attacking_chooses_what_it_attacks_cr_508_4() {
    // ruling: 9d3f1a8a bdd4c462 7a850527
    // P1 controls Jace, the Mind Sculptor. Voice of Victory's two Warriors:
    // the first attacks Jace, the second P1. Adeline's Human and Gut's
    // Skeleton attack Jace.
    let base = |p0: &[&str]| {
        Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 5])
            .library(1, &["Forest"; 5])
            .battlefield(0, p0)
            .battlefield(1, &["Jace, the Mind Sculptor"])
            .counters(1, "Jace, the Mind Sculptor", Counter::Loyalty, 10)
            .at(Step::DeclareAttackers, 0)
            .start()
    };
    let mut g = base(&["Voice of Victory"]);
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    let v = bf(&g, 0, "Voice of Victory");
    attack(&mut g, &[v]);
    let asked = std::cell::Cell::new(0);
    let tj = g.tgt(jace);
    quiet_until_with(&mut g, |g| g.turn().step == Step::DeclareAttackers && g.stack().is_empty() && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::AttackTarget) => {
            asked.set(asked.get() + 1);
            Some(if asked.get() == 1 { tj } else { player(1) })
        }
        _ => None,
    });
    assert_eq!(asked.get(), 2, "one choice per token\n{}", log(&g));
    let w = tokens_named(&g, "Warrior");
    let dests: Vec<_> = w.iter().map(|&t| g.attack_dest(t)).collect();
    assert!(dests.contains(&Some(Target::Obj(g.obj_ref(jace)))) && dests.contains(&Some(Target::Player(1))), "{dests:?}");
    assert_eq!(g.attack_dest(v), Some(Target::Player(1)));
    // Adeline and Gut.
    let mut g = base(&["Adeline, Resplendent Cathar", "Gut, True Soul Zealot", "Sol Ring"]);
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    let (ad, gut) = (bf(&g, 0, "Adeline, Resplendent Cathar"), bf(&g, 0, "Gut, True Soul Zealot"));
    let ring = bf(&g, 0, "Sol Ring");
    attack(&mut g, &[ad, gut]);
    let tj = g.tgt(jace);
    quiet_until_with(&mut g, |g| g.turn().step == Step::DeclareAttackers && g.stack().is_empty() && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::AttackTarget) => Some(tj),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ if g.has_option(Choice::Card(ring)) => Some(Choice::Card(ring)),
        _ => None,
    });
    let human = tokens_named(&g, "Human")[0];
    let skel = tokens_named(&g, "Skeleton")[0];
    assert_eq!(g.attack_dest(human), Some(Target::Obj(g.obj_ref(jace))), "{}", log(&g));
    assert_eq!(g.attack_dest(skel), Some(Target::Obj(g.obj_ref(jace))), "{}", log(&g));
}

// ---- ninjutsu (CR 702.49) ---------------------------------------------------------------------

#[test]
fn ninjutsu_after_blockers_in_the_first_strike_step_or_after_damage_cr_702_49_510_4() {
    // ruling: ec3f111d 68e1969c 7ad97095 d4961537 666134f3 350567b9
    // P0 attacks Jace with a Test Bear and P1 with a Test Striker (first
    // strike). Before blockers: no ninjutsu. In the first-strike damage
    // step, Fallen Shinobi returns the Bear and attacks Jace in its place,
    // dealing its 5 in the regular damage step. Kaito, ninjutsu'd in the
    // combat damage step after the damage (returning the Striker), deals
    // none.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Test Bear", "Test Striker", "Island", "Island", "Swamp", "Swamp", "Island", "Swamp", "Swamp"])
        .hand(0, &["Fallen Shinobi", "Kaito, Bane of Nightmares"])
        .battlefield(1, &["Jace, the Mind Sculptor"])
        .counters(1, "Jace, the Mind Sculptor", Counter::Loyalty, 10)
        .at(Step::DeclareAttackers, 0)
        .start();
    let (bear, st) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Striker"));
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    let fs = g.card(0, "Fallen Shinobi", Zone::Hand);
    let kaito = g.card(0, "Kaito, Bane of Nightmares", Zone::Hand);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(g.tgt(jace));
    g.choose(Choice::Card(st));
    g.choose(player(1));
    g.choose(Choice::Pass);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    assert_eq!(g.turn().step, Step::DeclareAttackers);
    assert!(!g.has_option(Choice::Ability(fs, AbilitySlot::Hand(0))), "no unblocked attacker before blockers");
    to_step(&mut g, Step::FirstStrikeDamage, 0);
    assert_eq!(g.life(1), 18, "the Striker's first strike");
    ninjutsu(&mut g, fs, bear);
    resolve(&mut g);
    let fs = bf(&g, 0, "Fallen Shinobi");
    assert_eq!(g.attack_dest(fs), Some(Target::Obj(g.obj_ref(jace))), "what the Bear attacked\n{}", log(&g));
    to_step(&mut g, Step::CombatDamage, 0);
    assert_eq!(g.counters_on(jace, Counter::Loyalty), 5, "the Shinobi's 5 in the regular step\n{}", log(&g));
    assert_eq!(g.life(1), 18, "the Striker deals no regular damage");
    // Kaito after the damage: returns the Striker, deals nothing.
    ninjutsu(&mut g, kaito, st);
    resolve(&mut g);
    let k = bf(&g, 0, "Kaito, Bane of Nightmares");
    assert!(g.obj(k).attacking, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 18, "no more combat damage");
    assert_eq!(g.counters_on(jace, Counter::Loyalty), 5);
}

// ---- attacking (CR 302.6, 508.1) --------------------------------------------------------------

#[test]
fn a_permanent_made_a_creature_attacks_and_taps_only_if_controlled_since_the_turn_began_cr_302_6() {
    // ruling: 0333f482 3d047ce6 cdbf63ec
    // P0 plays a Forest and casts Life: the new Forest can't attack or pay
    // mana, the old Forest can. Celestial Colonnade and Restless Vinestalk
    // played this turn (untapped by Candelabra of Tawnos) and animated:
    // they can't attack; old ones can.
    let mut g = scen()
        .battlefield(0, &["Forest", "Mountain"])
        .hand(0, &["Forest", "Life // Death", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let old = bf(&g, 0, "Forest");
    let new = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(new));
    let ld = g.card(0, "Life // Death", Zone::Hand);
    g.choose(Choice::Cast(ld));
    let c = Choice::Card(old);
    resolve_with(&mut g, |g| g.has_option(c).then_some(c));
    assert!(g.is_creature(new) && g.is_creature(old));
    assert!(g.obj(old).tapped && !g.obj(new).tapped);
    // The Bolt: {R} from the Mountain; the new Forest isn't a source.
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{}", log(&g));
    assert!(!g.has_option(Choice::Card(new)), "a sick land creature can't tap for mana: {}", opts(&g));
    pay(&mut g);
    resolve(&mut g);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(!g.has_option(Choice::Card(new)), "nor attack: {}", opts(&g));
    // Manlands.
    for land in ["Celestial Colonnade", "Restless Vinestalk"] {
        let mut g = scen()
            .battlefield(0, &[land, "Candelabra of Tawnos", "Plains", "Island", "Forest", "Forest", "Island", "Plains", "Island", "Plains", "Island", "Forest", "Forest", "Plains", "Island", "Plains", "Island"])
            .hand(0, &[land])
            .at(Step::Main1, 0)
            .start();
        let old = bf(&g, 0, land);
        let new = g.card(0, land, Zone::Hand);
        g.choose(Choice::Play(new));
        resolve(&mut g);
        assert!(g.obj(new).tapped, "{land} enters tapped");
        let cand = bf(&g, 0, "Candelabra of Tawnos");
        g.choose(ab(cand, 0));
        let tn = g.tgt(new);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
            Some(DecisionKind::Target) => Some(tn),
            Some(DecisionKind::PayMana) => pay_avoiding(g, &[new, old]),
            _ => None,
        });
        assert!(!g.obj(new).tapped, "{land}: untapped by the Candelabra\n{}", log(&g));
        for l in [new, old] {
            g.choose(ab(l, 1));
            resolve_with(&mut g, |g| match g.kind() {
                Some(DecisionKind::PayMana) => pay_avoiding(g, &[new, old]),
                _ => None,
            });

        }
        assert!(g.is_creature(new) && g.is_creature(old), "{land}\n{}", log(&g));
        quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
        assert!(g.has_option(Choice::Card(old)), "{land}: controlled since the turn began: {}", opts(&g));
        assert!(!g.has_option(Choice::Card(new)), "{land}: played this turn: summoning sick: {}", opts(&g));
    }
}

#[test]
fn an_attacking_colonnade_tapped_for_mana_stays_in_combat_cr_506_4() {
    // ruling: 90f93ad0
    // Celestial Colonnade (vigilance) attacks; in the declare blockers step
    // P0 taps it for {U} to cast Brainstorm. It stays attacking and deals
    // its 4.
    let mut g = scen()
        .battlefield(0, &["Celestial Colonnade", "Plains", "Island", "Plains", "Island", "Plains"])
        .hand(0, &["Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let cc = bf(&g, 0, "Celestial Colonnade");
    g.choose(ab(cc, 1));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayMana) => pay_avoiding(g, &[cc]),
        _ => None,
    });
    assert!(g.is_creature(cc));
    attack(&mut g, &[cc]);
    assert!(!g.obj(cc).tapped, "vigilance");
    to_step(&mut g, Step::DeclareBlockers, 0);
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::PayMana));
    let tap = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| matches!(c, Choice::Card(x) | Choice::Ability(x, _) if x == cc));
    g.choose(tap.unwrap_or_else(|| panic!("the Colonnade pays: {}", opts(&g))));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) | Some(DecisionKind::PutOnTop) => first_non_pass(g),
        _ => None,
    });
    assert!(g.obj(cc).tapped, "{}", log(&g));
    assert!(g.obj(cc).attacking, "still attacking");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 16, "{}", log(&g));
}

#[test]
fn a_dashed_creature_doesnt_have_to_attack_cr_702_109a() {
    // ruling: 7929f582
    let mut g = scen().battlefield(0, &["Mountain", "Mountain"]).hand(0, &["Ragavan, Nimble Pilferer"]).at(Step::Main1, 0).start();
    let r = g.card(0, "Ragavan, Nimble Pilferer", Zone::Hand);
    g.choose(Choice::Cast(r));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        _ => None,
    });
    assert!(g.has_kw(r, Kw::Haste), "dashed\n{}", log(&g));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(g.has_option(Choice::Card(r)) && g.has_option(Choice::Pass));
    g.choose(Choice::Pass);
    assert!(!g.obj(r).attacking);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 20);
}

#[test]
fn robber_attacking_a_planeswalker_reads_its_controller_as_the_defending_player_cr_506_2() {
    // ruling: 69d95378
    // Robber of the Rich attacks Jace: P1 (more cards in hand) is the
    // defending player: P1's top card is exiled.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Lightning Bolt", "Forest", "Forest"])
        .battlefield(0, &["Robber of the Rich"])
        .battlefield(1, &["Jace, the Mind Sculptor"])
        .hand(1, &["Forest", "Forest"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let r = bf(&g, 0, "Robber of the Rich");
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(r));
    g.choose(g.tgt(jace));
    g.choose(Choice::Pass);
    resolve(&mut g);
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Exile), 1, "{}", log(&g));
}

#[test]
fn a_creature_attacks_alone_only_if_declared_alone_cr_702_83b() {
    // ruling: 8d6c08d2
    // Two attackers with Ignoble Hierarch: P1 Bolts one in the declare
    // attackers step; the other, now the only attacker, gets no exalted.
    let mut g = scen()
        .battlefield(0, &["Ignoble Hierarch", "Test Bear", "Test Cat"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let (bear, cat) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Cat"));
    attack(&mut g, &[bear, cat]);
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.turn().step, Step::DeclareAttackers);
    cast_at(&mut g, 1, "Lightning Bolt", cat);
    assert_eq!(g.obj(cat).zone, Zone::Graveyard);
    assert!(g.waiting_triggers().is_empty() && g.stack().is_empty());
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 18, "the Bear alone, no exalted\n{}", log(&g));
}

#[test]
fn must_attack_is_excused_when_it_cant_and_means_the_player_not_a_planeswalker_cr_508_1d() {
    // ruling: 7bf7bcc3 dbf6f20e
    // Ursine Monstrosity tapped (it crewed Smuggler's Copter) or cast this
    // turn: its trigger resolves, it doesn't attack. Untapped, with P1's
    // Jace on the battlefield: it attacks P1, not Jace (no choice asked).
    let mut g = scen()
        .battlefield(0, &["Ursine Monstrosity", "Smuggler's Copter"])
        .at(Step::Main1, 0)
        .start();
    let u = bf(&g, 0, "Ursine Monstrosity");
    let copter = bf(&g, 0, "Smuggler's Copter");
    g.choose(ab(copter, 0));
    resolve_with(&mut g, |g| g.has_option(Choice::Card(u)).then_some(Choice::Card(u)));
    assert!(g.obj(u).tapped);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(!g.obj(u).attacking, "tapped: it doesn't attack\n{}", log(&g));
    assert!(g.has_kw(u, Kw::Indestructible), "the trigger resolved");
    // Cast this turn.
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest"])
        .hand(0, &["Ursine Monstrosity"])
        .at(Step::Main1, 0)
        .start();
    let u = g.card(0, "Ursine Monstrosity", Zone::Hand);
    g.choose(Choice::Cast(u));
    resolve(&mut g);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers) || g.turn().step >= Step::Main2);
    assert!(!g.obj(u).attacking, "summoning sick: it doesn't attack\n{}", log(&g));
    // P1's planeswalker.
    let mut g = scen()
        .battlefield(0, &["Ursine Monstrosity"])
        .battlefield(1, &["Jace, the Mind Sculptor"])
        .at(Step::Main1, 0)
        .start();
    let u = bf(&g, 0, "Ursine Monstrosity");
    quiet_until_with(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers), |g| {
        assert_ne!(g.kind(), Some(DecisionKind::AttackTarget), "the chosen player, not a planeswalker");
        None
    });
    assert!(g.obj(u).attacking);
    assert_eq!(g.attack_dest(u), Some(Target::Player(1)));
}

#[test]
fn blocks_declared_stand_unblockable_cant_block_and_menace_afterwards_cr_509_1h_506_4() {
    // ruling: 78a2ab04 f21fe774 992f009b
    // P0's Test Bear is blocked by P1's Test Cat; after blocks P0 makes the
    // Bear unblockable (Manifold Key) and the Cat unable to block (Zirda):
    // the Bear stays blocked, the Cat blocking; the Bear deals its damage
    // to the Cat. Carnage Interpreter blocked by one creature gains menace
    // (P0 casts a card, one left in hand): still blocked.
    let mut g = scen()
        .battlefield(0, &["Test Bear", "Manifold Key", "Zirda, the Dawnwaker", "Mountain", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Test Cat"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let cat = bf(&g, 1, "Test Cat");
    attack(&mut g, &[bear]);
    quiet_until_with(&mut g, |g| g.turn().step == Step::DeclareBlockers && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::DeclareBlockers) if g.has_option(Choice::Card(bear)) => Some(Choice::Card(bear)),
        _ => None,
    });
    assert_eq!(g.obj(cat).blocking, Some(bear), "{}", log(&g));
    let key = bf(&g, 0, "Manifold Key");
    let z = bf(&g, 0, "Zirda, the Dawnwaker");
    g.choose(ab(key, 1));
    resolve_at(&mut g, bear);
    g.choose(ab(z, 0));
    resolve_at(&mut g, cat);
    assert!(g.obj(bear).blocked, "still blocked\n{}", log(&g));
    assert_eq!(g.obj(cat).blocking, Some(bear), "still blocking");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 20, "{}", log(&g));
    assert_eq!(g.obj(cat).zone, Zone::Graveyard);
    // Carnage Interpreter.
    let mut g = scen()
        .battlefield(0, &["Carnage Interpreter", "Mountain"])
        .hand(0, &["Lightning Bolt", "Island"])
        .battlefield(1, &["Test Cat"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let ci = bf(&g, 0, "Carnage Interpreter");
    let cat = bf(&g, 1, "Test Cat");
    assert_eq!(g.pt(ci), (3, 3));
    attack(&mut g, &[ci]);
    quiet_until_with(&mut g, |g| g.turn().step == Step::DeclareBlockers && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::DeclareBlockers) if g.has_option(Choice::Card(ci)) => Some(Choice::Card(ci)),
        _ => None,
    });
    assert_eq!(g.obj(cat).blocking, Some(ci));
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    assert!(g.has_kw(ci, Kw::Menace) && g.pt(ci) == (5, 5), "{}", log(&g));
    assert!(g.obj(ci).blocked && g.obj(cat).blocking == Some(ci), "menace afterwards: still blocked");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 17, "the Bolt only");
}

#[test]
fn haste_lost_before_attacking_means_no_attack_one_greaves_for_one_new_creature_cr_302_6() {
    // ruling: 402f58a3
    // Two Test Bears cast this turn; Lightning Greaves equipped to the
    // first, then moved to the second: the first can't attack.
    let mut g = scen()
        .battlefield(0, &["Lightning Greaves", "Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Test Bear", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    for _ in 0..2 {
        let b = g.card(0, "Test Bear", Zone::Hand);
        g.choose(Choice::Cast(b));
        resolve(&mut g);
    }
    let bears: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Test Bear").collect();
    let gr = bf(&g, 0, "Lightning Greaves");
    g.choose(ab(gr, 0));
    resolve_at(&mut g, bears[0]);
    assert!(g.has_kw(bears[0], Kw::Haste));
    g.choose(ab(gr, 0));
    resolve_at(&mut g, bears[1]);
    assert_eq!(g.obj(gr).attached, Some(g.obj_ref(bears[1])));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(!g.has_option(Choice::Card(bears[0])), "lost haste: can't attack: {}", opts(&g));
    assert!(g.has_option(Choice::Card(bears[1])));
}

#[test]
fn a_vehicle_must_be_a_creature_as_attackers_or_blockers_are_declared_and_follows_creature_rules_cr_301_7_302_6() {
    // ruling: bfeae3be 8de9353a 3a81a6c3
    // Crewed in the beginning of combat step, the Copter attacks. Crewed in
    // P1's declare attackers step (after attackers are declared), it
    // blocks. A Copter cast this turn and crewed by a creature cast this
    // turn (any untapped creature crews) can't attack.
    let mut g = scen().battlefield(0, &["Smuggler's Copter", "Test Bear"]).at(Step::Main1, 0).start();
    let copter = bf(&g, 0, "Smuggler's Copter");
    let bear = bf(&g, 0, "Test Bear");
    to_step(&mut g, Step::BeginCombat, 0);
    g.choose(ab(copter, 0));
    resolve_with(&mut g, |g| g.has_option(Choice::Card(bear)).then_some(Choice::Card(bear)));
    assert!(g.is_creature(copter));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(g.has_option(Choice::Card(copter)), "crewed in the beginning of combat: {}", opts(&g));
    // Blocking.
    let mut g = scen().battlefield(0, &["Test Cat"]).battlefield(1, &["Smuggler's Copter", "Test Bear"]).at(Step::DeclareAttackers, 0).start();
    let cat = bf(&g, 0, "Test Cat");
    let copter = bf(&g, 1, "Smuggler's Copter");
    let bear = bf(&g, 1, "Test Bear");
    attack(&mut g, &[cat]);
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.turn().step, Step::DeclareAttackers);
    g.choose(ab(copter, 0));
    resolve_with(&mut g, |g| g.has_option(Choice::Card(bear)).then_some(Choice::Card(bear)));
    quiet_until_with(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers) && g.has_option(Choice::Card(cat)) || g.turn().step >= Step::CombatDamage, |_| None);
    assert_eq!(g.kind(), Some(DecisionKind::DeclareBlockers), "{}", log(&g));
    assert!(g.has_option(Choice::Card(cat)));
    g.choose(Choice::Card(cat));
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.obj(copter).blocking, Some(cat), "crewed in the declare attackers step: it blocks\n{}", log(&g));
    // Cast this turn.
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Smuggler's Copter", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    for n in ["Smuggler's Copter", "Test Bear"] {
        let c = g.card(0, n, Zone::Hand);
        g.choose(Choice::Cast(c));
        resolve(&mut g);
    }
    let copter = bf(&g, 0, "Smuggler's Copter");
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.is_sick(bear));
    assert!(g.has_option(ab(copter, 0)), "a creature cast this turn crews");
    g.choose(ab(copter, 0));
    resolve_with(&mut g, |g| g.has_option(Choice::Card(bear)).then_some(Choice::Card(bear)));
    assert!(g.is_creature(copter) && g.obj(bear).tapped);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers) || g.turn().step >= Step::Main2);
    assert!(!g.has_option(Choice::Card(copter)), "summoning sick: {}", opts(&g));
}

#[test]
fn an_equipment_doesnt_tap_with_the_creature_it_equips_cr_301_5() {
    // ruling: dde8305d
    // Lion Sash reconfigured onto a Test Bear that attacks: the Bear taps,
    // the Sash doesn't; unattached after combat, it's untapped and blocks
    // on P1's turn.
    let mut g = scen()
        .battlefield(0, &["Lion Sash", "Test Bear", "Plains", "Plains", "Plains", "Plains"])
        .battlefield(1, &["Test Cat"])
        .at(Step::Main1, 0)
        .start();
    let sash = bf(&g, 0, "Lion Sash");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(ab(sash, 1));
    resolve_at(&mut g, bear);
    assert_eq!(g.obj(sash).attached, Some(g.obj_ref(bear)), "{}", log(&g));
    attack(&mut g, &[bear]);
    assert!(g.obj(bear).tapped && !g.obj(sash).tapped);
    to_step(&mut g, Step::Main2, 0);
    let unattach = (0..4u8).map(|i| ab(sash, i)).filter(|&a| g.has_option(a)).last().expect("reconfigure");
    g.choose(unattach);
    resolve(&mut g);
    assert!(g.is_creature(sash) && !g.obj(sash).tapped, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let cat = bf(&g, 1, "Test Cat");
    attack(&mut g, &[cat]);
    quiet_until_with(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step >= Step::CombatDamage, |_| None);
    assert!(g.has_option(Choice::Card(cat)), "the Sash blocks: {}", opts(&g));
}

#[test]
fn caustic_bronco_stays_saddled_until_the_turn_ends_cr_702_171b() {
    // ruling: cfd9220f
    // Saddled in the first main phase, Caustic Bronco is still saddled as
    // it attacks (P1 loses the revealed card's mana value); on P0's next
    // turn it isn't (P0 loses it).
    let mut g = scen()
        .battlefield(0, &["Caustic Bronco", "Test Bear", "Test Flyer"])
        .library(0, &["Test Giant", "Island", "Test Giant", "Island"])
        .at(Step::Main1, 0)
        .start();
    let bronco = bf(&g, 0, "Caustic Bronco");
    g.choose(ab(bronco, 0));
    resolve(&mut g);
    attack(&mut g, &[bronco]);
    to_step(&mut g, Step::Main2, 0);
    assert_eq!((g.life(0), g.life(1)), (20, 13), "saddled through the turn (5, and 2 combat damage)\n{}", log(&g));
    quiet_until(&mut g, |g| g.turn().number == 5 && g.kind() == Some(DecisionKind::DeclareAttackers));
    attack(&mut g, &[bronco]);
    to_step(&mut g, Step::Main2, 0);
    assert_eq!(g.life(0), 15, "not saddled any more: P0 loses\n{}", log(&g));
}

#[test]
fn a_mode_chosen_three_times_deals_damage_three_times_cr_700_2d_120_3() {
    // ruling: 9403db83
    // Fiery Confluence's "1 damage to each creature" chosen three times:
    // P1's Screaming Nemesis is dealt damage in three events and triggers
    // three times (1 each).
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Fiery Confluence"])
        .battlefield(1, &["Screaming Nemesis"])
        .at(Step::Main1, 0)
        .start();
    let fc = g.card(0, "Fiery Confluence", Zone::Hand);
    g.choose(Choice::Cast(fc));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(0)),
        Some(DecisionKind::Target) => Some(player(0)),
        _ => None,
    });
    let n = g.log_lines().iter().filter(|l| l.contains("trigger of Screaming Nemesis goes on the stack")).count();
    assert_eq!(n, 3, "three damage events\n{}", log(&g));
    assert_eq!(g.life(0), 17, "1 + 1 + 1");
}

#[test]
fn an_exerted_land_untapped_before_the_next_untap_step_untaps_normally_after_it_cr_701_39b() {
    // ruling: 5b2bbb6a
    // Arena of Glory exerted for a Test Dwarf, then untapped by Candelabra
    // of Tawnos: at P0's next untap step the exert effect expires doing
    // nothing; tapped again that turn, Arena untaps at the following one.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Arena of Glory", "Candelabra of Tawnos", "Island"])
        .hand(0, &["Test Dwarf", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let arena = bf(&g, 0, "Arena of Glory");
    let dwarf = g.card(0, "Test Dwarf", Zone::Hand);
    g.choose(Choice::Cast(dwarf));
    g.drive_until(|g| !at_priority(g));
    g.choose(ab(arena, 1));
    pay(&mut g);
    resolve(&mut g);
    assert!(g.obj(arena).tapped && g.has_kw(dwarf, Kw::Haste), "exerted\n{}", log(&g));
    let cand = bf(&g, 0, "Candelabra of Tawnos");
    g.choose(ab(cand, 0));
    let ta = g.tgt(arena);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(ta),
        _ => None,
    });
    assert!(!g.obj(arena).tapped, "untapped by the Candelabra\n{}", log(&g));
    // P0's next turn: untapped; tap it for the Bolt's {R}.
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.obj(arena).tapped);
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| matches!(c, Choice::Card(x) | Choice::Ability(x, _) if x == arena)),
        _ => None,
    });
    assert!(g.obj(arena).tapped, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().number == 7 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.obj(arena).tapped, "the exert effect expired: it untaps\n{}", log(&g));
}

#[test]
fn boast_once_a_turn_even_attacking_in_two_combats_cr_702_142a_500_8() {
    // ruling: 909ac1ac
    // Fear of Missing Out (delirium) untaps Broadside Bombardiers and adds
    // a combat phase. Boasted in the first combat (Sol Ring), Bombardiers
    // attacks again in the second: no second boast.
    let mut g = scen()
        .battlefield(0, &["Fear of Missing Out", "Broadside Bombardiers", "Sol Ring", "Mind Stone"])
        .graveyard(0, &["Lightning Bolt", "Test Bear", "Forest", "Sol Ring"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let f = bf(&g, 0, "Fear of Missing Out");
    let b = bf(&g, 0, "Broadside Bombardiers");
    let ring = bf(&g, 0, "Sol Ring");
    attack(&mut g, &[f, b]);
    let tb = g.tgt(b);
    quiet_until_with(&mut g, |g| g.stack().is_empty() && at_priority(g) && g.turn().priority == 0, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(tb),
        _ => None,
    });
    assert!(!g.obj(b).tapped, "untapped by Fear of Missing Out\n{}", log(&g));
    assert!(g.has_option(ab(b, 0)), "boast: {}", opts(&g));
    g.choose(ab(b, 0));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ if g.has_option(Choice::Card(ring)) => Some(Choice::Card(ring)),
        _ => None,
    });
    assert_eq!(g.obj(ring).zone, Zone::Graveyard, "{}", log(&g));
    // The second combat: Bombardiers attacks again.
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(g.has_option(Choice::Card(b)), "{}", opts(&g));
    attack(&mut g, &[b]);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    assert!(g.obj(b).attacking);
    assert!(!g.has_option(ab(b, 0)), "boast only once each turn: {}", opts(&g));
}
