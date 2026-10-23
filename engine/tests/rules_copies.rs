//! M7b part 3 (ENGINE.md 26): the rules of copies (CR 707), with cube cards:
//! copiable values (707.2) and layer 1 (613.2), copies of copies (707.3),
//! becoming a copy (707.4), entering as a copy (707.5), double-faced
//! permanents (707.8), exceptions (707.9), copies of spells (707.10) and of
//! permanent spells (707.10f, 608.3f, 111.13), last known information
//! (608.2h), the legend rule (704.5j), hidden information.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
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

/// Answer with first options (passing priority) until `stop`.
fn answer_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..1000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = if d.kind == DecisionKind::Priority { Choice::Pass } else { d.options[0].choice };
        g.choose(c);
    }
    panic!("answer_until: no progress");
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn id(name: &str) -> u16 {
    db().expect_id(name).unwrap()
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

fn tokens_named(g: &Game, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.is_token(c) && g.name(c) == name).collect()
}

const SAHEELI: &str = "Saheeli, Sublime Artificer";

/// Saheeli's -2: `artifact` becomes a copy of `other`.
fn saheeli_copy(g: &mut Game, artifact: CardIdx, other: CardIdx) {
    let sa = bf(g, 0, SAHEELI);
    g.choose(Choice::Ability(sa, AbilitySlot::Activated(0)));
    let first = std::cell::Cell::new(true);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            let t = if first.get() { artifact } else { other };
            first.set(false);
            Some(g.tgt(t))
        }
        _ => None,
    });
}

#[test]
fn copiable_values_have_no_counters_or_other_effects_cr_707_2() {
    // Sol Ring becomes a copy of a Test Bear with a +1/+1 counter: a 2/2
    // (the counter isn't copied).
    let mut g = scen().battlefield(0, &[SAHEELI, "Sol Ring", "Test Bear"]).counters(0, "Test Bear", Counter::P1P1, 1).start();
    let (ring, bear) = (bf(&g, 0, "Sol Ring"), bf(&g, 0, "Test Bear"));
    assert_eq!(g.pt(bear), (3, 3));
    saheeli_copy(&mut g, ring, bear);
    assert_eq!(g.pt(ring), (2, 2));
}

#[test]
fn counters_on_a_copy_apply_after_layer_1_cr_613_2() {
    // A +1/+1 counter on the Sol Ring stays and applies on top of the copied
    // values (layer 7c after layer 1).
    let mut g = scen().battlefield(0, &[SAHEELI, "Sol Ring", "Test Bear"]).counters(0, "Sol Ring", Counter::P1P1, 1).start();
    let (ring, bear) = (bf(&g, 0, "Sol Ring"), bf(&g, 0, "Test Bear"));
    saheeli_copy(&mut g, ring, bear);
    assert_eq!(g.pt(ring), (3, 3));
}

#[test]
fn becoming_a_copy_triggers_no_enter_ability_cr_707_4() {
    let mut g = scen().battlefield(0, &[SAHEELI, "Sol Ring", "Thraben Inspector"]).start();
    let (ring, insp) = (bf(&g, 0, "Sol Ring"), bf(&g, 0, "Thraben Inspector"));
    saheeli_copy(&mut g, ring, insp);
    assert_eq!(g.name(ring), "Thraben Inspector");
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 0);
}

#[test]
fn a_copy_of_a_copy_copies_its_copiable_values_cr_707_3() {
    // Saheeli makes the Sol Ring a copy of a Phyrexian Metamorph that is a
    // copy of Thraben Inspector: an Inspector (an artifact by both
    // exceptions), not a Metamorph.
    let mut g = scen()
        .battlefield(0, &[SAHEELI, "Sol Ring", "Thraben Inspector", "Island", "Island", "Island", "Island"])
        .hand(0, &["Phyrexian Metamorph"])
        .start();
    let (ring, insp) = (bf(&g, 0, "Sol Ring"), bf(&g, 0, "Thraben Inspector"));
    let m = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(insp)),
        // (Pay with the Islands.)
        Some(DecisionKind::PayMana) => g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == ring))
            .map(|o| o.choice),
        _ => None,
    });
    saheeli_copy(&mut g, ring, m);
    assert_eq!(g.name(ring), "Thraben Inspector");
    assert_eq!(g.copiable(ring).def, id("Thraben Inspector"));
    assert!(g.is_type(ring, engine::db::types::ARTIFACT));
}

#[test]
fn a_token_copy_of_a_transformed_permanent_has_its_back_face_up_cr_707_8a() {
    // Reflection of Kiki-Jiki copies a transformed Ulvenwald Oddity: the token
    // is an Ulvenwald Behemoth (whose static ability then pumps the others).
    let mut g = scen()
        .battlefield(0, &["Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Mountain", "Ulvenwald Oddity // Ulvenwald Behemoth"])
        .transformed(0, "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki")
        .transformed(0, "Ulvenwald Oddity // Ulvenwald Behemoth")
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let b = bf(&g, 0, "Ulvenwald Behemoth");
    g.choose(Choice::Ability(r, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(b)));
    let t = tokens_named(&g, "Ulvenwald Behemoth");
    assert_eq!(t.len(), 1);
    assert_eq!(g.pt(t[0]), (9, 9), "8/8, +1/+1 from the other Behemoth");
    assert_eq!(g.pt(b), (9, 9));
}

#[test]
fn a_token_copy_of_a_legend_meets_the_legend_rule_cr_704_5j() {
    // Three Steps Ahead copies Questing Beast: two legendary Questing Beasts;
    // their controller keeps one. The token's mana value is the copied
    // card's (CR 707.2).
    let mut g = scen().battlefield(0, &["Island"; 4]).battlefield(0, &["Questing Beast"]).hand(0, &["Three Steps Ahead"]).start();
    let qb = bf(&g, 0, "Questing Beast");
    let s = g.card(0, "Three Steps Ahead", Zone::Hand);
    g.choose(Choice::Cast(s));
    let kept = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(if g.has_option(Choice::Number(1)) { Choice::Number(1) } else { Choice::Pass }),
        Some(DecisionKind::Target) => Some(g.tgt(qb)),
        Some(DecisionKind::LegendRule) => {
            kept.set(true);
            Some(Choice::Card(qb))
        }
        _ => None,
    });
    assert!(kept.get());
    assert_eq!(g.obj(qb).zone, Zone::Battlefield);
    assert!(tokens_named(&g, "Questing Beast").is_empty());
}

#[test]
fn a_token_copy_of_a_token_copy_keeps_its_exceptions_cr_707_9b() {
    // Reflection's hasty Inspector token, copied by Three Steps Ahead: the new
    // token has haste too (part of the copied token's copiable values).
    let mut g = scen()
        .battlefield(0, &["Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Thraben Inspector"])
        .battlefield(0, &["Island"; 5])
        .transformed(0, "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki")
        .hand(0, &["Three Steps Ahead"])
        .start();
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    let insp = bf(&g, 0, "Thraben Inspector");
    g.choose(Choice::Ability(r, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(insp)));
    let t1 = tokens_named(&g, "Thraben Inspector")[0];
    let s = g.card(0, "Three Steps Ahead", Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(if g.has_option(Choice::Number(1)) { Choice::Number(1) } else { Choice::Pass }),
        Some(DecisionKind::Target) => Some(g.tgt(t1)),
        _ => None,
    });
    let ts = tokens_named(&g, "Thraben Inspector");
    assert_eq!(ts.len(), 2);
    assert!(ts.iter().all(|&t| g.has_kw(t, Kw::Haste)));
}

#[test]
fn a_token_copy_of_an_exiled_permanent_uses_its_last_known_values_cr_608_2h() {
    // Fractured Identity exiles P1's Metamorph copying Thraben Inspector: P0's
    // token is an Inspector and an artifact (the copiable values it had), and
    // its own "when this enters" investigates for P0.
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Island", "Island", "Island"])
        .battlefield(1, &["Island", "Island", "Island", "Island", "Thraben Inspector"])
        .hand(0, &["Fractured Identity"])
        .hand(1, &["Phyrexian Metamorph"])
        .at(engine::Step::Main1, 1)
        .start();
    let insp = bf(&g, 1, "Thraben Inspector");
    let m = g.card(1, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChoosePermanent)).then_some(Choice::Card(insp)));
    // P0's turn.
    for _ in 0..200 {
        if g.turn().active == 0 && g.turn().step == engine::Step::Main1 && at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = if d.kind == DecisionKind::Priority || d.kind == DecisionKind::DeclareAttackers { Choice::Pass } else { d.options[0].choice };
        g.choose(c);
    }
    let clues = g.count_in(0, "Clue", Zone::Battlefield);
    let fi = g.card(0, "Fractured Identity", Zone::Hand);
    g.choose(Choice::Cast(fi));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(m)));
    assert_eq!(g.obj(m).zone, Zone::Exile);
    assert_eq!(g.name(m), "Phyrexian Metamorph", "in exile it is its card");
    let t = tokens_named(&g, "Thraben Inspector");
    assert_eq!(t.len(), 1);
    assert_eq!(g.obj(t[0]).controller, 0);
    assert!(g.is_type(t[0], engine::db::types::ARTIFACT));
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), clues + 1);
}

#[test]
fn a_copy_of_a_spell_isnt_cast_cr_707_10() {
    // Lose Focus replicated once under Saheeli and Witherbloom Apprentice:
    // Saheeli's "whenever you cast a noncreature spell" triggers once;
    // magecraft ("whenever you cast or copy") twice.
    let mut g = scen()
        .battlefield(0, &[SAHEELI, "Witherbloom Apprentice", "Island", "Island", "Island"])
        .battlefield(1, &["Mountain"])
        .hand(0, &["Lose Focus"])
        .hand(1, &["Lightning Bolt"])
        .at(engine::Step::Main1, 1)
        .start();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let lf = g.card(0, "Lose Focus", Zone::Hand);
    g.choose(Choice::Cast(lf));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Obj(g.obj_ref(bolt)))),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Servo", Zone::Battlefield), 1);
    assert_eq!(g.life(1), 18, "magecraft twice");
    assert_eq!(g.life(0), 22);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
}

#[test]
fn a_copy_of_a_spell_copies_x_and_ceases_to_exist_cr_707_10_707_10a() {
    // Lutri copies Traumatic Critique with X = 2: the copy deals 2 too.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Mountain", "Mountain", "Mountain", "Island"])
        .hand(0, &["Traumatic Critique", "Lutri, the Spellchaser"])
        .start();
    let tc = g.card(0, "Traumatic Critique", Zone::Hand);
    g.choose(Choice::Cast(tc));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    answer_until(&mut g, at_priority);
    let l = g.card(0, "Lutri, the Spellchaser", Zone::Hand);
    g.choose(Choice::Cast(l));
    let copy = std::cell::Cell::new(None);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Obj(g.obj_ref(tc)))),
        Some(DecisionKind::NewTarget) => {
            if let engine::game::StackKind::Spell { card } = g.stack().last().unwrap().kind {
                copy.set(Some(card));
                assert_eq!(g.stack().last().unwrap().x, 2);
            }
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(g.life(1), 16);
    let c = copy.get().unwrap();
    assert_eq!(g.obj(c).zone, Zone::Nowhere);
    assert!(g.is_copy_obj(c));
}

#[test]
fn a_copy_of_a_permanent_spell_becomes_a_token_not_created_cr_608_3f_111_13() {
    // Sage of the Skies's copy under Elspeth, Storm Slayer ("if one or more
    // tokens would be created, twice that many"): one token Sage, not two:
    // the copy becomes a token as it resolves, it isn't created.
    let mut g = scen()
        .battlefield(0, &["Elspeth, Storm Slayer", "Plains", "Plains", "Plains"])
        .hand(0, &["Mox Pearl", "Sage of the Skies"])
        .start();
    let mox = g.card(0, "Mox Pearl", Zone::Hand);
    g.choose(Choice::Cast(mox));
    resolve_with(&mut g, |_| None);
    let s = g.card(0, "Sage of the Skies", Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve_with(&mut g, |_| None);
    assert_eq!(tokens_named(&g, "Sage of the Skies").len(), 1);
}

#[test]
fn a_copy_of_a_spell_can_be_countered_and_ceases_to_exist_cr_707_10a() {
    // P1 pays {R}{R} to copy Chain Lightning back; P0 counters the copy.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Island", "Island"])
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(0, &["Chain Lightning", "Counterspell"])
        .start();
    let cl = g.card(0, "Chain Lightning", Zone::Hand);
    g.choose(Choice::Cast(cl));
    let copy = std::cell::Cell::new(None);
    for _ in 0..300 {
        if g.kind() == Some(DecisionKind::Priority) && g.turn().priority == 0 && g.stack().iter().any(|s| matches!(s.kind, engine::game::StackKind::Spell { card } if g.is_copy_obj(card))) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::Target => player(1),
            DecisionKind::YesNo => Choice::Yes,
            DecisionKind::NewTarget => {
                if let engine::game::StackKind::Spell { card } = g.stack().last().unwrap().kind {
                    copy.set(Some(card));
                }
                player(0)
            }
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    let c = copy.get().expect("P1 copied it");
    let cs = g.card(0, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| Choice::Target(Target::Obj(g.obj_ref(c)))));
    assert_eq!(g.obj(c).zone, Zone::Nowhere);
    assert_eq!(g.life(0), 20);
    assert_eq!(g.life(1), 17);
}

#[test]
fn copies_are_public_and_survive_determinize() {
    // A Metamorph copying the opponent's creature: what is copied is public,
    // so a world sampled for either player keeps the pending choice and the
    // copy, and its encoding is the same.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .battlefield(1, &["Thraben Inspector"])
        .hand(0, &["Phyrexian Metamorph", "Island", "Forest"])
        .hand(1, &["Island", "Mountain", "Forest"])
        .start();
    let insp = bf(&g, 1, "Thraben Inspector");
    let m = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(m));
    answer_until(&mut g, |g| g.kind() == Some(DecisionKind::ChoosePermanent));
    // The copy choice is pending; P1's world keeps it.
    for viewer in [0u8, 1] {
        let mut w = g.clone();
        let mut rng = engine::rng::Rng::new(7);
        w.determinize(viewer, &mut rng);
        assert_eq!(w.pending_decision(), g.pending_decision());
        assert_eq!(w.view_of(viewer), g.view_of(viewer));
    }
    g.choose(Choice::Card(insp));
    resolve_with(&mut g, |_| None);
    assert!(g.is_copy(m));
    let n = engine::az::encode::NUM_FEATURES;
    for viewer in [0u8, 1] {
        let mut x0 = vec![0.0; n];
        g.encode_features(viewer, &mut x0);
        for seed in 0..4 {
            let mut w = g.clone();
            let mut rng = engine::rng::Rng::new(seed);
            w.determinize(viewer, &mut rng);
            assert!(w.is_copy(m) && w.copiable(m) == g.copiable(m));
            let mut x1 = vec![0.0; n];
            w.encode_features(viewer, &mut x1);
            assert!(x0 == x1, "viewer {viewer}: the encoding differs in a sampled world");
        }
    }
}

#[test]
fn a_copy_of_a_spell_choosing_new_targets_survives_determinize_cr_707_10c() {
    // P1 pays {R}{R} and copies Chain Lightning; the copy's new-target
    // decision is pending with both hands hidden: every sampled world keeps
    // it, the copy on the stack and the encoding.
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .battlefield(1, &["Mountain", "Mountain", "Thraben Inspector"])
        .hand(0, &["Chain Lightning", "Island", "Forest"])
        .hand(1, &["Island", "Lightning Bolt", "Forest"])
        .start();
    let cl = g.card(0, "Chain Lightning", Zone::Hand);
    g.choose(Choice::Cast(cl));
    for _ in 0..100 {
        if g.kind() == Some(DecisionKind::NewTarget) {
            break;
        }
        let c = match g.kind() {
            Some(DecisionKind::Target) => player(1),
            Some(DecisionKind::YesNo) => Choice::Yes,
            Some(DecisionKind::Priority) => Choice::Pass,
            _ => g.pending_decision().unwrap().options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget));
    assert_eq!(g.to_move(), Some(1));
    let n = engine::az::encode::NUM_FEATURES;
    for viewer in [0u8, 1] {
        let mut x0 = vec![0.0; n];
        g.encode_features(viewer, &mut x0);
        for seed in 0..4 {
            let mut w = g.clone();
            let mut rng = engine::rng::Rng::new(seed);
            w.determinize(viewer, &mut rng);
            assert_eq!(w.pending_decision(), g.pending_decision());
            assert_eq!(w.view_of(viewer), g.view_of(viewer));
            let mut x1 = vec![0.0; n];
            w.encode_features(viewer, &mut x1);
            assert!(x0 == x1, "viewer {viewer}: the encoding differs in a sampled world");
        }
    }
}
