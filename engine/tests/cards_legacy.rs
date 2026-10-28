//! The legacy cards of the Delver mirror (ENGINE.md 53) against their Oracle
//! text, and their official rulings (`// ruling: <hash8>`, as the cube's in
//! `docs/rulings.md`; `data/rulings/legacy.rulings.json`). They are in the
//! card table of constructed play only (`CardDb::constructed`).
//!
//! Murktide Regent {5}{U}{U}, Creature — Dragon 3/3: "Delve. Flying. This
//! creature enters with a +1/+1 counter on it for each instant and sorcery
//! card exiled with it. Whenever an instant or sorcery card leaves your
//! graveyard, put a +1/+1 counter on this creature."
//!
//! Prismari Charm {U}{R}, Instant: "Choose one — Surveil 2, then draw a card;
//! Prismari Charm deals 1 damage to each of one or two targets; Return target
//! nonland permanent to its owner's hand."

use engine::db::{AbilitySlot, CardDb, Counter, Kw};
use engine::decision::act as ids;
use engine::game::StackKind;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const MURKTIDE: &str = "Murktide Regent";
const CHARM: &str = "Prismari Charm";
const TAMIYO: &str = "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar";
const SCHOLAR: &str = "Tamiyo, Seasoned Scholar";

fn scen() -> Scenario {
    Scenario::new(CardDb::constructed()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn p1p1(g: &Game, c: CardIdx) -> u32 {
    g.counters_on(c, Counter::P1P1)
}

/// Answer the decisions of a cast (or activation) until priority: delve's
/// graveyard picks from `picks` in order (then `PASS`), `answer` for the
/// rest (else the first option).
fn pay_with(g: &mut Game, picks: &[CardIdx], answer: impl Fn(&Game) -> Option<Choice>) -> usize {
    let mut i = 0;
    for _ in 0..200 {
        if at_priority(g) || g.is_over() {
            return i;
        }
        let c = if let Some(c) = answer(g) {
            c
        } else if g.kind() == Some(DecisionKind::ChooseGraveyardCard) && i < picks.len() {
            i += 1;
            Choice::Card(picks[i - 1])
        } else if g.kind() == Some(DecisionKind::ChooseGraveyardCard) && g.has_option(Choice::Pass) {
            Choice::Pass
        } else {
            g.pending_decision().unwrap().options[0].choice
        };
        g.choose(c);
    }
    panic!("no priority\n{}", g.log_lines().join("\n"));
}

/// Resolve the stack (passing priority), `answer` answering what is asked.
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
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
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn resolve(g: &mut Game) {
    resolve_with(g, |_| None);
}

fn legal(g: &Game) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// Pass priority and decline attacks and blocks until P`p`'s next precombat
/// main phase, with priority and the stack empty.
fn quiet_until_turn(g: &mut Game, p: PlayerId) {
    let start = g.turn().number;
    for _ in 0..5000 {
        if g.turn().number > start && g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty() {
            return;
        }
        let d = g.pending_decision().expect("game over");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until_turn: no progress");
}

fn gy(g: &Game, p: PlayerId) -> Vec<CardIdx> {
    g.player(p).graveyard.clone()
}

/// Cast Murktide Regent from P`p`'s hand delving `picks` (graveyard cards,
/// bottom first), then resolve everything; the Regent on the battlefield.
fn cast_murktide(g: &mut Game, p: PlayerId, picks: &[CardIdx]) -> CardIdx {
    let m = g.card(p, MURKTIDE, Zone::Hand);
    g.choose(Choice::Cast(m));
    let n = pay_with(g, picks, |_| None);
    assert_eq!(n, picks.len(), "delve picks: {}", opts(g));
    resolve(g);
    assert_eq!(g.obj(m).zone, Zone::Battlefield, "{}", g.log_lines().join("\n"));
    m
}

// ---- Murktide Regent ---------------------------------------------------------------

#[test]
fn murktide_regent_is_a_3_3_flying_dragon_with_delve() {
    let g = scen().battlefield(0, &[MURKTIDE]).start();
    let m = bf(&g, 0, MURKTIDE);
    assert_eq!(g.pt(m), (3, 3));
    assert!(g.has_kw(m, Kw::Flying));
    let d = g.db().def(g.obj(m).def);
    assert!(d.delve && d.counters_per_exiled.is_some());
    assert_eq!(d.mana_value(), 7);
    // Entering without being cast: no counter (CR 607.2q: none were exiled
    // to pay its cost).
    assert_eq!(p1p1(&g, m), 0);
}

#[test]
fn murktide_enters_with_a_counter_for_each_instant_and_sorcery_card_it_delved_cr_607_2q() {
    // Five cards delved: two instants, a sorcery, a land, an artifact: three
    // counters. Its own delve doesn't trigger it (it isn't on the
    // battlefield as the cards leave the graveyard, CR 603.10a, 603.6).
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .graveyard(0, &["Brainstorm", "Ponder", "Lightning Bolt", "Volcanic Island", "Mishra's Bauble"])
        .hand(0, &[MURKTIDE])
        .at(Step::Main1, 0)
        .start();
    let picks = gy(&g, 0);
    let m = cast_murktide(&mut g, 0, &picks);
    assert_eq!(p1p1(&g, m), 3);
    assert_eq!(g.pt(m), (6, 6));
    assert!(g.player(0).graveyard.is_empty());
    assert_eq!(g.objs().iter().filter(|o| o.zone == Zone::Exile).count(), 5);
}

#[test]
fn murktide_counts_only_the_cards_exiled_to_pay_its_cost_cr_607_2q() {
    // Instants exiled earlier by another Regent's delve stay in exile but
    // were not exiled with this one; this one delves two lands only.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Volcanic Island"])
        .graveyard(0, &["Brainstorm", "Ponder", "Island", "Island"])
        .hand(0, &[MURKTIDE, MURKTIDE])
        .at(Step::Main1, 0)
        .start();
    let g0 = gy(&g, 0);
    // The first Regent: two instants and sorceries delved, and the lands
    // pay the rest ({3}{U}{U} with five lands).
    let first = cast_murktide(&mut g, 0, &g0[..2]);
    assert_eq!(p1p1(&g, first), 2);
    // Next turn: the second delves the two Islands (no instant or sorcery).
    let mut g2 = g.clone();
    quiet_until_turn(&mut g2, 0);
    let lands = gy(&g2, 0);
    assert_eq!(lands.len(), 2);
    let second = cast_murktide(&mut g2, 0, &lands);
    assert_eq!(p1p1(&g2, second), 0, "the instants in exile weren't exiled with it");
    // (Lands leaving the graveyard don't trigger the first either.)
    assert_eq!(p1p1(&g2, first), 2);
}

#[test]
fn a_second_regents_delve_triggers_the_first_once_per_instant_or_sorcery_card_cr_603_10a() {
    let mut g = scen()
        .battlefield(0, &[MURKTIDE, "Island", "Island"])
        .graveyard(0, &["Brainstorm", "Ponder", "Island", "Island", "Island"])
        .hand(0, &[MURKTIDE])
        .at(Step::Main1, 0)
        .start();
    let first = bf(&g, 0, MURKTIDE);
    let picks = gy(&g, 0);
    let second = g.card(0, MURKTIDE, Zone::Hand);
    g.choose(Choice::Cast(second));
    pay_with(&mut g, &picks, |_| None);
    // Cast: the two triggers go on the stack above the spell (CR 603.3), one
    // per card (not "one or more").
    let st = g.stack();
    assert_eq!(st.len(), 3, "{:?}", st);
    assert!(matches!(st[0].kind, StackKind::Spell { card } if card == second));
    for it in &st[1..] {
        assert!(matches!(it.kind, StackKind::Ability { source, .. } if source.idx == first));
    }
    resolve(&mut g);
    assert_eq!(p1p1(&g, first), 2);
    assert_eq!(p1p1(&g, second), 2);
    assert_eq!(g.pt(first), (5, 5));
}

#[test]
fn cards_leaving_an_opponents_graveyard_dont_trigger_it() {
    // P1 delves seven instants for Treasure Cruise: P0's Regent gets nothing.
    let mut g = scen()
        .battlefield(0, &[MURKTIDE])
        .battlefield(1, &["Island"])
        .graveyard(1, &["Brainstorm", "Ponder", "Lightning Bolt", "Daze", "Force of Will", "Unholy Heat", "Brainstorm"])
        .hand(1, &["Treasure Cruise"])
        .at(Step::Main1, 1)
        .start();
    let m = bf(&g, 0, MURKTIDE);
    let tc = g.card(1, "Treasure Cruise", Zone::Hand);
    let picks = gy(&g, 1);
    g.choose(Choice::Cast(tc));
    pay_with(&mut g, &picks, |_| None);
    resolve(&mut g);
    assert_eq!(g.player(1).graveyard.len(), 1, "Treasure Cruise itself");
    assert_eq!(p1p1(&g, m), 0);
}

#[test]
fn tamiyos_minus_three_returning_an_instant_triggers_it() {
    // A card returned from the graveyard to the hand leaves the graveyard.
    let mut g = scen()
        .battlefield(0, &[MURKTIDE, TAMIYO])
        .transformed(0, TAMIYO)
        .counters(0, SCHOLAR, Counter::Loyalty, 3)
        .graveyard(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let m = bf(&g, 0, MURKTIDE);
    let t = bf(&g, 0, SCHOLAR);
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    g.choose(Choice::Ability(t, AbilitySlot::Activated(1)));
    g.choose(g.tgt(bolt));
    resolve(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Hand);
    assert_eq!(p1p1(&g, m), 1);
}

#[test]
fn a_flashback_spell_cast_from_the_graveyard_triggers_it() {
    // Faithless Looting cast with flashback: it leaves the graveyard for the
    // stack (CR 601.2a); its two discards (a land, an instant) go to the
    // graveyard, which isn't leaving it.
    let mut g = scen()
        .battlefield(0, &[MURKTIDE, "Volcanic Island", "Volcanic Island", "Volcanic Island"])
        .graveyard(0, &["Faithless Looting"])
        .hand(0, &["Island", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let m = bf(&g, 0, MURKTIDE);
    let fl = g.card(0, "Faithless Looting", Zone::Graveyard);
    let a = g.graveyard_action(0, fl);
    assert!(legal(&g).contains(&a), "{}", opts(&g));
    g.apply(a, &mut Vec::new()).unwrap();
    pay_with(&mut g, &[], |g| (g.kind() == Some(DecisionKind::ChooseRoute)).then(|| g.pending_decision().unwrap().options[0].choice));
    resolve(&mut g);
    assert_eq!(g.obj(fl).zone, Zone::Exile, "flashback exiles it");
    assert_eq!(p1p1(&g, m), 1);
}

#[test]
fn murktide_put_onto_the_battlefield_without_being_cast_has_no_counters() {
    // Reanimate: not cast, nothing exiled to pay its cost (CR 607.2q).
    let mut g = scen()
        .battlefield(0, &["Underground Sea"])
        .graveyard(0, &["Brainstorm", MURKTIDE, "Ponder"])
        .hand(0, &["Reanimate"])
        .at(Step::Main1, 0)
        .start();
    let m = g.card(0, MURKTIDE, Zone::Graveyard);
    let r = g.card(0, "Reanimate", Zone::Hand);
    g.choose(Choice::Cast(r));
    let t = g.tgt(m);
    pay_with(&mut g, &[], |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    resolve(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Battlefield);
    assert_eq!(p1p1(&g, m), 0);
    assert_eq!(g.pt(m), (3, 3));
    assert_eq!(g.life(0), 13, "Reanimate: its mana value 7");
}

#[test]
fn delve_doesnt_change_its_mana_value_cr_702_66a() {
    // ruling: 87078369
    // Cast for {U}{U} with five cards exiled: Mana Drain counters it and
    // its controller gets {C} x 7 at its next main phase.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .graveyard(0, &["Island", "Island", "Island", "Island", "Island"])
        .hand(0, &[MURKTIDE])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Mana Drain"])
        .at(Step::Main1, 0)
        .start();
    let m = g.card(0, MURKTIDE, Zone::Hand);
    let picks = gy(&g, 0);
    g.choose(Choice::Cast(m));
    pay_with(&mut g, &picks, |_| None);
    g.pass();
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let md = g.card(1, "Mana Drain", Zone::Hand);
    g.choose(Choice::Cast(md));
    let t = g.tgt(m);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(m).zone, Zone::Graveyard);
    quiet_until_turn(&mut g, 1);
    g.drive_until(|g| g.turn().step == Step::Main1 && at_priority(g));
    resolve(&mut g);
    assert_eq!(g.player(1).pool.amt[5], 7, "{:?}", g.player(1).pool.amt);
}

#[test]
fn delve_exiles_at_most_its_generic_mana_cr_702_66a() {
    // ruling: 05ce86bd
    // Ten cards in the graveyard and seven lands: at most five exiled.
    let mut g = scen()
        .battlefield(0, &["Island"; 7])
        .graveyard(0, &["Brainstorm"; 10])
        .hand(0, &[MURKTIDE])
        .at(Step::Main1, 0)
        .start();
    let picks = gy(&g, 0);
    let m = g.card(0, MURKTIDE, Zone::Hand);
    g.choose(Choice::Cast(m));
    let n = pay_with(&mut g, &picks, |_| None);
    assert_eq!(n, 5, "delve offered past its {{5}}");
    assert_eq!(g.player(0).graveyard.len(), 5);
    resolve(&mut g);
    assert_eq!(p1p1(&g, m), 5);
    // With two lands, fewer than five cards can't pay: not castable.
    let g = scen().battlefield(0, &["Island", "Island"]).graveyard(0, &["Brainstorm"; 4]).hand(0, &[MURKTIDE]).at(Step::Main1, 0).start();
    let m = g.card(0, MURKTIDE, Zone::Hand);
    assert!(!g.has_option(Choice::Cast(m)), "{}", opts(&g));
}

#[test]
fn delve_works_with_an_alternative_cost_cr_702_66b() {
    // ruling: 0a2cf1a9
    // Underworld Breach grants escape (an alternative cost: its mana cost
    // and three other graveyard cards exiled). Delve pays the {5}; only the
    // cards delve exiled are "exiled with it": Murktide's escape isn't
    // printed on it (CR 607.2q), so the escape's three instants don't count.
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Island", "Island"])
        .graveyard(0, &["Brainstorm", "Ponder", "Island", "Island", "Island", MURKTIDE, "Lightning Bolt", "Daze", "Unholy Heat"])
        .at(Step::Main1, 0)
        .start();
    let m = g.card(0, MURKTIDE, Zone::Graveyard);
    let gy0 = gy(&g, 0);
    let a = g.graveyard_action(0, m);
    assert!(legal(&g).contains(&a), "{}", opts(&g));
    g.apply(a, &mut Vec::new()).unwrap();
    // Delve: the first five (two of them instants and sorceries); escape's
    // exile: the three after it.
    let delve: Vec<CardIdx> = gy0[..5].to_vec();
    let mut picked = 0;
    for _ in 0..60 {
        if at_priority(&g) {
            break;
        }
        let c = match g.kind() {
            Some(DecisionKind::ChooseRoute) => g.pending_decision().unwrap().options[0].choice,
            Some(DecisionKind::ChooseGraveyardCard) if picked < delve.len() && g.has_option(Choice::Card(delve[picked])) => {
                picked += 1;
                Choice::Card(delve[picked - 1])
            }
            Some(DecisionKind::ChooseGraveyardCard) if g.has_option(Choice::Pass) && picked < delve.len() => Choice::Pass,
            _ => g.pending_decision().unwrap().options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(picked, 5, "{}", g.log_lines().join("\n"));
    resolve(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Battlefield, "{}", g.log_lines().join("\n"));
    assert_eq!(p1p1(&g, m), 2);
    assert!(g.player(0).graveyard.is_empty());
}

// ---- Prismari Charm ---------------------------------------------------------------

fn charm_scen() -> Scenario {
    scen().battlefield(0, &["Volcanic Island", "Volcanic Island"]).hand(0, &[CHARM]).at(Step::Main1, 0)
}

/// Cast the Charm choosing mode `m`; `answer` answers the rest.
fn cast_charm(g: &mut Game, m: u8, answer: impl Fn(&Game) -> Option<Choice>) {
    let c = g.card(0, CHARM, Zone::Hand);
    g.choose(Choice::Cast(c));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "{}", opts(g));
    g.choose(Choice::Number(m));
    pay_with(g, &[], answer);
}

#[test]
fn prismari_charm_is_choose_one() {
    // CR 700.2: one mode, chosen as it's cast; no second.
    let mut g = charm_scen().battlefield(1, &["Dragon's Rage Channeler"]).start();
    let c = g.card(0, CHARM, Zone::Hand);
    g.choose(Choice::Cast(c));
    let d = g.pending_decision().unwrap();
    assert_eq!(d.kind, DecisionKind::ChooseMode);
    let modes: Vec<Choice> = d.options.iter().map(|o| o.choice).collect();
    assert_eq!(modes, [Choice::Number(0), Choice::Number(1), Choice::Number(2)], "{}", opts(&g));
    g.choose(Choice::Number(0));
    assert_ne!(g.kind(), Some(DecisionKind::ChooseMode));
}

#[test]
fn prismari_charm_surveils_two_then_draws() {
    // Surveil 2 (CR 701.25): the two top cards, here both to the graveyard;
    // then draw the third.
    let mut g = charm_scen().library(0, &["Lightning Bolt", "Ponder", "Brainstorm"]).start();
    let hand0 = g.player(0).hand.len();
    // (The scenario's first library card is the top one.)
    cast_charm(&mut g, 0, |_| None);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutInGraveyard) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard) + g.count_in(0, "Ponder", Zone::Graveyard), 2, "{}", g.log_lines().join("\n"));
    assert_eq!(g.count_in(0, "Brainstorm", Zone::Hand), 1, "the third card drawn");
    assert_eq!(g.player(0).hand.len(), hand0, "the Charm left, a card came");
    assert_eq!(g.count_in(0, CHARM, Zone::Graveyard), 1);
}

#[test]
fn prismari_charm_deals_one_damage_to_each_of_two_targets() {
    let mut g = charm_scen().battlefield(1, &["Dragon's Rage Channeler", "Volcanic Island"]).start();
    let drc = bf(&g, 1, "Dragon's Rage Channeler");
    let (t_drc, t_p1) = (g.tgt(drc), Choice::Target(Target::Player(1)));
    let asked = std::cell::Cell::new(0);
    cast_charm(&mut g, 1, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            asked.set(asked.get() + 1);
            if asked.get() == 1 {
                Some(t_drc)
            } else {
                // CR 115.3: the same creature can't be the second target.
                assert!(!g.has_option(t_drc), "{}", opts(g));
                // A land isn't "any target".
                assert!(!g.options_desc().iter().any(|d| d.contains("Volcanic Island")), "{}", opts(g));
                Some(t_p1)
            }
        }
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.count_in(1, "Dragon's Rage Channeler", Zone::Graveyard), 1);
    assert_eq!(g.life(1), 19);
}

#[test]
fn prismari_charm_may_have_one_target_only() {
    let mut g = charm_scen().start();
    let asked = std::cell::Cell::new(0);
    cast_charm(&mut g, 1, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            asked.set(asked.get() + 1);
            if asked.get() == 1 {
                Some(Choice::Target(Target::Player(1)))
            } else {
                assert!(g.has_option(Choice::Pass), "one or two: done after one\n{}", opts(g));
                Some(Choice::Pass)
            }
        }
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.life(1), 19);
    assert_eq!(g.life(0), 20);
}

#[test]
fn prismari_charm_with_one_target_gone_still_damages_the_other_cr_608_2b() {
    // P1 answers by returning its creature to hand (another Charm): the
    // player still takes 1.
    let mut g = charm_scen().battlefield(1, &["Dragon's Rage Channeler", "Volcanic Island", "Volcanic Island"]).hand(1, &[CHARM]).start();
    let drc = bf(&g, 1, "Dragon's Rage Channeler");
    let (t_drc, t_p1) = (g.tgt(drc), Choice::Target(Target::Player(1)));
    let asked = std::cell::Cell::new(0);
    cast_charm(&mut g, 1, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            asked.set(asked.get() + 1);
            Some(if asked.get() == 1 { t_drc } else { t_p1 })
        }
        _ => None,
    });
    g.pass();
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let c1 = g.card(1, CHARM, Zone::Hand);
    g.choose(Choice::Cast(c1));
    g.choose(Choice::Number(2));
    let t = g.tgt(drc);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(drc).zone, Zone::Hand);
    assert_eq!(g.life(1), 19, "{}", g.log_lines().join("\n"));
}

#[test]
fn prismari_charm_returns_a_nonland_permanent_to_its_owners_hand() {
    // An artifact of the opponent's; not a land.
    let mut g = charm_scen().battlefield(1, &["Cori-Steel Cutter", "Volcanic Island"]).start();
    let cutter = bf(&g, 1, "Cori-Steel Cutter");
    let t = g.tgt(cutter);
    cast_charm(&mut g, 2, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            assert!(!g.options_desc().iter().any(|d| d.contains("Volcanic Island")), "a land: {}", opts(g));
            Some(t)
        }
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.obj(cutter).zone, Zone::Hand);
    assert_eq!(g.obj(cutter).owner, 1);
    // A token returned to a hand ceases to exist (CR 111.7, 704.5d).
    let mut g = charm_scen().battlefield(0, &[TAMIYO]).start();
    // (Tamiyo investigates when she attacks: a Clue.)
    let tam = bf(&g, 0, TAMIYO);
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(tam));
    g.choose(Choice::Pass);
    resolve(&mut g);
    g.drive_until(|g| g.turn().step == Step::Main2 && at_priority(g));
    let clue = g.battlefield().iter().copied().find(|&c| g.name(c) == "Clue").expect("a Clue");
    let t = g.tgt(clue);
    cast_charm(&mut g, 2, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    resolve(&mut g);
    assert!(!g.battlefield().contains(&clue));
    assert_eq!(g.obj(clue).zone, Zone::Nowhere);
    let _ = ids::PASS;
}
