//! M7b part 3 (ENGINE.md 26): copies, part 2: copies of spells (Chain
//! Lightning, Chain of Smog, Lose Focus, Lutri, the Spellchaser, Sage of the
//! Skies) and the costs that make copies (Three Steps Ahead's spree,
//! Sicarian Infiltrator's squad, Thundertrap Trainer's offspring), each
//! against its Oracle text.

mod common;

use common::db;
use engine::db::{types, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Target, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
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

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn tokens_named(g: &Game, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == name).collect()
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

// ---- Three Steps Ahead (spree) ----------------------------------------------------------------

const STEPS: &str = "Three Steps Ahead";

#[test]
fn three_steps_ahead_spree_pays_each_chosen_modes_cost_cr_702_172a() {
    // "Spree. + {1}{U} - Counter target spell. + {3} - Create a token that's a
    // copy of target artifact or creature you control. + {2} - Draw two
    // cards, then discard a card."
    let mut g = scen().battlefield(0, &["Island"; 6]).battlefield(0, &["Test Bear"]).hand(0, &[STEPS, "Forest"]).start();
    let s = g.card(0, STEPS, Zone::Hand);
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Cast(s));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    // No spell to counter: the first mode can't be chosen; no mode yet: no
    // pass (one or more modes).
    assert!(!g.has_option(Choice::Number(0)) && !g.has_option(Choice::Pass));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(2));
    let hand = g.player(0).hand.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    // {U} + {3} + {2}: all six Islands.
    assert_eq!(g.lands_of(0).filter(|&c| g.obj(c).tapped).count(), 6);
    let t = tokens_named(&g, "Test Bear");
    assert_eq!(t.len(), 1);
    assert_eq!(g.pt(t[0]), (2, 2));
    assert_eq!(g.player(0).hand.len(), hand + 1, "drew two, discarded one");
}

#[test]
fn three_steps_ahead_offers_only_modes_whose_costs_can_be_paid_cr_700_2h() {
    // Five Islands: {U} + {3} is payable, then {2} more is not.
    let mut g = scen().battlefield(0, &["Island"; 5]).battlefield(0, &["Test Bear"]).hand(0, &[STEPS]).start();
    let s = g.card(0, STEPS, Zone::Hand);
    g.choose(Choice::Cast(s));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1));
    assert!(!g.has_option(Choice::Number(2)));
    assert!(g.has_option(Choice::Pass));
}

#[test]
fn three_steps_ahead_counters_a_spell_at_instant_speed() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .battlefield(1, &["Mountain"])
        .hand(0, &[STEPS])
        .hand(1, &["Lightning Bolt"])
        .at(engine::Step::Main1, 1)
        .start();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(0));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let s = g.card(0, STEPS, Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(0)) => Some(Choice::Number(0)),
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Obj(g.obj_ref(bolt)))),
        _ => None,
    });
    assert_eq!(g.life(0), 20);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
}

// ---- Sicarian Infiltrator (squad) ---------------------------------------------------------

const SICARIAN: &str = "Sicarian Infiltrator";

#[test]
fn sicarian_infiltrator_squad_makes_a_copy_per_payment_cr_702_157a() {
    // "Flash. Squad {2}. Benediction of the Omnissiah - When this creature
    // enters, draw a card."
    let mut g = scen().battlefield(0, &["Island"; 7]).hand(0, &[SICARIAN]).start();
    let s = g.card(0, SICARIAN, Zone::Hand);
    let hand = g.player(0).hand.len();
    g.choose(Choice::Cast(s));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    // {2}{U} plus {2} any number of times: up to twice with seven lands.
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(opts, vec![Choice::Number(0), Choice::Number(1), Choice::Number(2)]);
    g.choose(Choice::Number(2));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.lands_of(0).filter(|&c| g.obj(c).tapped).count(), 7);
    let t = tokens_named(&g, SICARIAN);
    assert_eq!(t.len(), 2);
    for &c in &t {
        assert_eq!(g.pt(c), (1, 2));
        assert!(g.is_type(c, types::ARTIFACT));
    }
    // The card and each copy drew a card (its "when this enters"); the
    // copies' squad wasn't paid: no more copies.
    assert_eq!(g.player(0).hand.len(), hand - 1 + 3);
}

#[test]
fn sicarian_infiltrator_without_squad_makes_no_copy() {
    let mut g = scen().battlefield(0, &["Island"; 7]).hand(0, &[SICARIAN]).start();
    let s = g.card(0, SICARIAN, Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseMode)).then_some(Choice::Number(0)));
    assert!(tokens_named(&g, SICARIAN).is_empty());
    assert_eq!(g.lands_of(0).filter(|&c| g.obj(c).tapped).count(), 3);
}

// ---- Thundertrap Trainer (offspring) ------------------------------------------------------

const TRAINER: &str = "Thundertrap Trainer";

#[test]
fn thundertrap_trainer_offspring_makes_a_1_1_copy_cr_702_175a() {
    // "Offspring {4}. When this creature enters, look at the top four cards of
    // your library. You may reveal a noncreature, nonland card from among
    // them and put it into your hand. Put the rest on the bottom of your
    // library in a random order."
    let mut g = scen().battlefield(0, &["Island"; 6]).hand(0, &[TRAINER]).start();
    let t = g.card(0, TRAINER, Zone::Hand);
    g.choose(Choice::Cast(t));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(opts, vec![Choice::Number(0), Choice::Number(1)], "paid once or not");
    g.choose(Choice::Number(1));
    let looks = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::PutInHand) {
            looks.set(looks.get() + 1);
        }
        None
    });
    let copies = tokens_named(&g, TRAINER);
    assert_eq!(copies.len(), 1);
    assert_eq!(g.pt(copies[0]), (1, 1), "except it's 1/1");
    assert_eq!(g.pt(t), (1, 2));
    assert_eq!(looks.get(), 2, "the card's and the copy's look");
}

#[test]
fn thundertrap_trainer_offspring_needs_its_mana() {
    let mut g = scen().battlefield(0, &["Island"; 5]).hand(0, &[TRAINER]).start();
    let t = g.card(0, TRAINER, Zone::Hand);
    g.choose(Choice::Cast(t));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(opts, vec![Choice::Number(0)]);
}

// ---- Chain Lightning ------------------------------------------------------------------------

#[test]
fn chain_lightning_the_damaged_player_may_pay_to_copy_it_cr_707_10c() {
    // "Chain Lightning deals 3 damage to any target. Then that player or that
    // permanent's controller may pay {R}{R}. If the player does, they may copy
    // this spell and may choose a new target for that copy."
    let mut g = scen().battlefield(0, &["Mountain"]).battlefield(1, &["Mountain", "Mountain"]).hand(0, &["Chain Lightning"]).start();
    let cl = g.card(0, "Chain Lightning", Zone::Hand);
    g.choose(Choice::Cast(cl));
    let copy = std::cell::Cell::new(None);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        // P1 pays and copies; P0, out of mana, can't.
        Some(DecisionKind::YesNo) if g.to_move() == Some(1) => Some(Choice::Yes),
        Some(DecisionKind::NewTarget) => {
            assert_eq!(g.to_move(), Some(1), "the copy's controller chooses");
            let item = g.stack().last().unwrap();
            if let engine::game::StackKind::Spell { card } = item.kind {
                copy.set(Some(card));
            }
            assert!(g.has_option(Choice::Pass), "keep the target");
            Some(player(0))
        }
        _ => None,
    });
    assert_eq!(g.life(1), 17);
    assert_eq!(g.life(0), 17);
    let c = copy.get().expect("a copy was made");
    assert_eq!(g.obj(c).zone, Zone::Nowhere, "a copy of a spell ceases to exist (CR 707.10a)");
    assert_eq!(g.obj(cl).zone, Zone::Graveyard);
    assert_eq!(g.lands_of(1).filter(|&x| g.obj(x).tapped).count(), 2);
}

#[test]
fn chain_lightning_on_a_creature_asks_its_controller() {
    let mut g = scen().battlefield(0, &["Mountain"]).battlefield(1, &["Mountain", "Mountain", "Questing Beast"]).hand(0, &["Chain Lightning"]).start();
    let cl = g.card(0, "Chain Lightning", Zone::Hand);
    let qb = bf(&g, 1, "Questing Beast");
    g.choose(Choice::Cast(cl));
    let asked = std::cell::Cell::new(None);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(qb)),
        Some(DecisionKind::YesNo) => {
            asked.set(g.to_move());
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(asked.get(), Some(1), "the creature's controller may pay");
    assert_eq!(g.obj(qb).damage, 3);
}

// ---- Chain of Smog ---------------------------------------------------------------------------

#[test]
fn chain_of_smog_the_target_player_may_copy_it_cr_707_10() {
    // "Target player discards two cards. That player may copy this spell and
    // may choose a new target for that copy."
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Chain of Smog", "Island", "Forest"])
        .hand(1, &["Island", "Forest", "Mountain"])
        .start();
    let cs = g.card(0, "Chain of Smog", Zone::Hand);
    g.choose(Choice::Cast(cs));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        // P1 copies it at P0; P0 doesn't copy back.
        Some(DecisionKind::YesNo) => Some(if g.to_move() == Some(1) { Choice::Yes } else { Choice::Pass }),
        Some(DecisionKind::NewTarget) => Some(player(0)),
        _ => None,
    });
    assert_eq!(g.player(1).hand.len(), 1);
    assert_eq!(g.player(0).hand.len(), 0);
}

// ---- Lose Focus (replicate) -----------------------------------------------------------------

#[test]
fn lose_focus_replicate_copies_it_for_each_payment_cr_702_56a() {
    // "Replicate {U}. Counter target spell unless its controller pays {2}."
    // P1's Lightning Bolt; P0 casts Lose Focus with replicate paid once: the
    // copy counters the Bolt (P1 can't pay), the original then has no legal
    // target.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .battlefield(1, &["Mountain"])
        .hand(0, &["Lose Focus"])
        .hand(1, &["Lightning Bolt"])
        .at(engine::Step::Main1, 1)
        .start();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(0));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let lf = g.card(0, "Lose Focus", Zone::Hand);
    g.choose(Choice::Cast(lf));
    let copies = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            // {1}{U} plus {U} once: three Islands.
            assert!(!g.has_option(Choice::Number(2)));
            Some(Choice::Number(1))
        }
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Obj(g.obj_ref(bolt)))),
        Some(DecisionKind::NewTarget) => {
            // The copy may target the original Lose Focus instead (CR
            // 707.10c); it keeps the Bolt.
            copies.set(copies.get() + 1);
            assert!(g.has_option(Choice::Target(Target::Obj(g.obj_ref(lf)))));
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(copies.get(), 1);
    assert_eq!(g.life(0), 20);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.obj(lf).zone, Zone::Graveyard);
}

// ---- Lutri, the Spellchaser --------------------------------------------------------------

const LUTRI: &str = "Lutri, the Spellchaser";

#[test]
fn lutri_cast_copies_your_instant_cr_707_10() {
    // "Flash. When Lutri enters, if you cast it, copy target instant or
    // sorcery spell you control. You may choose new targets for the copy."
    let mut g = scen().battlefield(0, &["Mountain", "Mountain", "Island", "Island"]).hand(0, &["Lightning Bolt", LUTRI]).start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(|g| at_priority(g));
    let l = g.card(0, LUTRI, Zone::Hand);
    g.choose(Choice::Cast(l));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Obj(g.obj_ref(bolt)))),
        Some(DecisionKind::NewTarget) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.life(1), 14);
    assert_eq!(g.obj(l).zone, Zone::Battlefield);
}

#[test]
fn lutri_put_onto_the_battlefield_copies_nothing_cr_603_4() {
    // Put onto the battlefield by Flash: not cast, no copy.
    let mut g = scen().battlefield(0, &["Mountain", "Island", "Island"]).hand(0, &["Lightning Bolt", "Flash", LUTRI]).start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(|g| at_priority(g));
    let fl = g.card(0, "Flash", Zone::Hand);
    let l = g.card(0, LUTRI, Zone::Hand);
    g.choose(Choice::Cast(fl));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) => Some(Choice::Card(l)),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.life(1), 17);
}

// ---- Sage of the Skies --------------------------------------------------------------------

const SAGE: &str = "Sage of the Skies";

#[test]
fn sage_of_the_skies_copies_itself_after_another_spell_cr_707_10f() {
    // "When you cast this spell, if you've cast another spell this turn, copy
    // this spell. (The copy becomes a token.) Flying, lifelink."
    let mut g = scen().battlefield(0, &["Plains"; 3]).hand(0, &["Mox Pearl", SAGE]).start();
    let mox = g.card(0, "Mox Pearl", Zone::Hand);
    g.choose(Choice::Cast(mox));
    resolve_with(&mut g, |_| None);
    let s = g.card(0, SAGE, Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(s).zone, Zone::Battlefield);
    let t = tokens_named(&g, SAGE);
    assert_eq!(t.len(), 1, "the copy became a token (CR 608.3f)");
    assert_eq!(g.pt(t[0]), (2, 3));
    assert!(g.has_kw(t[0], Kw::Flying) && g.has_kw(t[0], Kw::Lifelink));
}

#[test]
fn sage_of_the_skies_alone_makes_no_copy() {
    let mut g = scen().battlefield(0, &["Plains"; 3]).hand(0, &[SAGE]).start();
    let s = g.card(0, SAGE, Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |_| None);
    assert!(tokens_named(&g, SAGE).is_empty());
}
