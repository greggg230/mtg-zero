//! M8d (ENGINE.md 37): the official rulings of the cube's stack cards
//! (Tishana's Tidebinder, Ertai Resurrected) as engine tests: countering an
//! ability (701.5), an effect that lasts "for as long as" (611.2b), the
//! layer-6 timestamp order of an ability loss and a later grant (613.7),
//! what a resolving ability checks (608.2), a countered delayed trigger
//! (603.7). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use rulings_util::*;

const TIDEBINDER: &str = "Tishana's Tidebinder";
const BALLISTA: &str = "Walking Ballista";

fn stack_target(g: &Game) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Target(Target::Stack(_))))
}

fn act(g: &Game, c: CardIdx) -> Choice {
    (0..8u8)
        .map(|i| Choice::Ability(c, AbilitySlot::Activated(i)))
        .find(|&a| g.has_option(a))
        .unwrap_or_else(|| panic!("no ability of {} offered: {}", g.name(c), opts(g)))
}

/// P0's Walking Ballista pings P1 (its ability on the stack); P1 casts
/// Tishana's Tidebinder, which enters; its trigger targets the ping and
/// waits on the stack with P0 to act. Returns (Ballista, Tidebinder).
fn tidebinder_trigger_on_the_ping(g: &mut Game) -> (CardIdx, CardIdx) {
    let wb = bf(g, 0, BALLISTA);
    g.choose(Choice::Ability(wb, AbilitySlot::Activated(1)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(at_priority);
    g.pass();
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    for _ in 0..100 {
        let trig_up = g.stack().len() == 2 && g.obj(tb).zone == Zone::Battlefield;
        if trig_up && at_priority(g) && g.pending_decision().unwrap().player == 0 {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::Target => stack_target(g).unwrap(),
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.stack().len(), 2, "the ping and the trigger\n{}", g.log_lines().join("\n"));
    (wb, tb)
}

#[test]
fn tidebinder_gone_before_its_trigger_resolves_still_counters_but_nothing_loses_abilities_cr_611_2b() {
    // ruling: da0512a5
    let mut g = scen()
        .battlefield(0, &[BALLISTA, "Mountain"])
        .counters(0, BALLISTA, Counter::P1P1, 2)
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &[TIDEBINDER])
        .start();
    let (wb, tb) = tidebinder_trigger_on_the_ping(&mut g);
    // P0 Bolts the Tidebinder (3/2) in response.
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(tb));
    resolve(&mut g);
    assert_eq!(g.obj(tb).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20, "the ping was still countered");
    assert!(g.has_option(Choice::Ability(wb, AbilitySlot::Activated(1))), "no loss of abilities: {}", opts(&g));
}

#[test]
fn tidebinder_an_ability_gained_after_the_loss_is_kept_cr_613_7() {
    // ruling: 6cadb1cc
    // After the Ballista loses all abilities, Lightning Greaves is attached
    // to it: haste and shroud, from a later timestamp, apply.
    let mut g = scen()
        .battlefield(0, &[BALLISTA, "Lightning Greaves"])
        .counters(0, BALLISTA, Counter::P1P1, 2)
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &[TIDEBINDER])
        .start();
    let (wb, _tb) = tidebinder_trigger_on_the_ping(&mut g);
    resolve(&mut g);
    assert!(!g.has_option(Choice::Ability(wb, AbilitySlot::Activated(1))), "lost its abilities");
    let gr = bf(&g, 0, "Lightning Greaves");
    g.choose(act(&g, gr));
    let t = g.tgt(wb);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.obj(gr).attached.map(|h| h.idx), Some(wb));
    assert!(g.has_kw(wb, Kw::Haste) && g.has_kw(wb, Kw::Shroud), "{:?}", g.keywords_of(wb));
}

#[test]
fn tidebinder_checks_the_sources_types_as_its_trigger_resolves_cr_608_2() {
    // ruling: 4ff964f0
    // Celestial Colonnade's "becomes a creature" ability, countered: the
    // Colonnade is a land (not an artifact, creature or planeswalker) as the
    // trigger resolves, so it keeps its abilities.
    let mut g = scen()
        .battlefield(0, &["Celestial Colonnade", "Plains", "Plains", "Plains", "Island", "Island", "Island", "Island", "Plains", "Plains"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &[TIDEBINDER])
        .start();
    let col = bf(&g, 0, "Celestial Colonnade");
    g.choose(act(&g, col));
    // Pay with the other lands; then P1 casts the Tidebinder.
    for _ in 0..50 {
        if at_priority(&g) && g.pending_decision().unwrap().player == 1 {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::PayMana => d.options.iter().find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == col)).unwrap().choice,
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.stack().len(), 1, "the animation on the stack");
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => stack_target(g),
        _ => None,
    });
    assert_eq!(g.count_in(1, TIDEBINDER, Zone::Battlefield), 1);
    assert!(!g.is_creature(col), "the animation was countered");
    // It still has its abilities: animating it again is offered.
    assert!((0..4u8).any(|i| g.has_option(Choice::Ability(col, AbilitySlot::Activated(i)))), "{}", opts(&g));
}

#[test]
fn a_countered_next_end_step_delayed_trigger_doesnt_trigger_again_cr_603_7c() {
    // ruling: 4e573280
    // Memory Jar's "at the beginning of the next end step, each player
    // discards their hand and returns ... each card they exiled this way",
    // countered by Tishana's Tidebinder: the exiled cards stay exiled, also
    // after the following end steps.
    // P1's library is all Tidebinders: it draws seven after the Jar.
    let mut g = scen()
        .battlefield(0, &["Memory Jar"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Test Giant"])
        .library(0, &["Island"; 20])
        .library(1, &[TIDEBINDER; 20])
        .start();
    let jar = bf(&g, 0, "Memory Jar");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let giant = g.card(1, "Test Giant", Zone::Hand);
    g.choose(act(&g, jar));
    resolve(&mut g);
    assert_eq!(g.player(1).hand.len(), 7);
    // P0's end step: the delayed trigger on the stack; P1 answers it.
    let countered = std::cell::Cell::new(false);
    quiet_until_with(
        &mut g,
        |g| g.turn().active == 1 && g.turn().step == Step::Main1,
        |g| {
            let d = g.pending_decision().unwrap();
            if d.kind == DecisionKind::Priority && d.player == 1 && g.turn().step == Step::End && !g.stack().is_empty() && !countered.get() {
                countered.set(true);
                return d.options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Cast(_)));
            }
            if d.kind == DecisionKind::Target {
                return stack_target(g);
            }
            None
        },
    );
    assert!(countered.get());
    assert_eq!(g.obj(bolt).zone, Zone::FaceDown, "never returned
{}", g.log_lines().join("
"));
    assert_eq!(g.obj(giant).zone, Zone::FaceDown);
    assert!(g.player(0).hand.len() >= 7, "nothing discarded");
    // Nor at the following end step.
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1);
    assert_eq!(g.obj(bolt).zone, Zone::FaceDown);
    assert_eq!(g.obj(giant).zone, Zone::FaceDown);
}

// ---- spells that can't be countered; storm; copies; checks on resolution -------------------------

const EMRAKUL: &str = "Emrakul, the Aeons Torn";

/// P0 casts Emrakul (15 Islands); P1 gets priority with the spell (and its
/// cast trigger) on the stack.
fn emrakul_cast(p1_lands: &[&str], p1_hand: &[&str]) -> (Game, CardIdx) {
    let mut g = scen().battlefield(0, &["Island"; 15]).battlefield(1, p1_lands).hand(0, &[EMRAKUL]).hand(1, p1_hand).library(1, &["Forest"; 10]).start();
    let e = g.card(0, EMRAKUL, Zone::Hand);
    g.choose(Choice::Cast(e));
    g.drive_until(at_priority);
    g.pass();
    (g, e)
}

/// The option naming `name` in the pending decision.
fn named_option(g: &Game, name: &str) -> Option<Choice> {
    let i = g.options_desc().iter().position(|d| d.contains(name))?;
    Some(g.pending_decision().unwrap().options[i].choice)
}

#[test]
fn remand_on_a_spell_that_cant_be_countered_still_draws_cr_101_2() {
    // ruling: 20aee8cd
    let (mut g, e) = emrakul_cast(&["Island", "Island"], &["Remand"]);
    let r = g.card(1, "Remand", Zone::Hand);
    let lib = g.player(1).library.len();
    g.choose(Choice::Cast(r));
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::Target) { named_option(g, "Emrakul") } else { None });
    assert_eq!(g.obj(e).zone, Zone::Battlefield, "neither countered nor returned");
    assert_eq!(g.player(1).library.len(), lib - 1, "Remand's draw");
}

#[test]
fn mana_drain_on_a_spell_that_cant_be_countered_still_adds_the_mana_cr_101_2() {
    // ruling: f4c8ec33
    let (mut g, e) = emrakul_cast(&["Island", "Island"], &["Mana Drain"]);
    let md = g.card(1, "Mana Drain", Zone::Hand);
    g.choose(Choice::Cast(md));
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::Target) { named_option(g, "Emrakul") } else { None });
    assert_eq!(g.obj(e).zone, Zone::Battlefield);
    // P0's extra turn, then P1's turn: the delayed trigger at P1's first main
    // phase adds {C} equal to Emrakul's mana value.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    resolve(&mut g);
    assert_eq!(g.turn().active, 1);
    assert_eq!(g.player(1).pool.amt[5], 15, "{}", g.log_lines().join("\n"));
}

#[test]
fn subtlety_puts_a_spell_that_cant_be_countered_into_its_library_cr_101_2() {
    // ruling: 7f2f6484 43fd075d
    let (mut g, e) = emrakul_cast(&["Island"; 4], &["Subtlety"]);
    let s = g.card(1, "Subtlety", Zone::Hand);
    g.choose(Choice::Cast(s));
    let seen_target = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(g.pending_decision().unwrap().options[0].choice),
        Some(DecisionKind::Target) => {
            // A spell on the stack only: no permanent is offered.
            assert!(g.options_desc().iter().all(|d| !d.contains("Island")), "{}", opts(g));
            seen_target.set(true);
            named_option(g, "Emrakul")
        }
        _ => None,
    });
    assert!(seen_target.get());
    assert_eq!(g.obj(e).zone, Zone::Library, "{}", g.log_lines().join("\n"));
}

#[test]
fn storm_counts_spells_that_were_countered_and_every_players_spells_cr_702_40a() {
    // ruling: 158501c3
    // P0: Lightning Bolt (resolves), Sol Ring (countered by P1's
    // Counterspell), then Brain Freeze: three spells before it, three copies;
    // P1 mills 4 x 3 = 12.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Island", "Island", "Island"])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &["Lightning Bolt", "Sol Ring", "Brain Freeze"])
        .hand(1, &["Counterspell"])
        .library(1, &["Forest"; 20])
        .start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    cast_targets(&mut g, b, &[player(1)]);
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    g.drive_until(at_priority);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::Target) { named_option(g, "Sol Ring") } else { None });
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    let lib = g.player(1).library.len();
    let bfz = g.card(0, "Brain Freeze", Zone::Hand);
    g.choose(Choice::Cast(bfz));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) | Some(DecisionKind::NewTarget) if g.has_option(player(1)) => Some(player(1)),
        _ => None,
    });
    assert_eq!(lib - g.player(1).library.len(), 12, "{}", g.log_lines().join("\n"));
}

#[test]
fn a_countered_storm_trigger_makes_no_copies_cr_702_40a() {
    // ruling: 543beb2e
    let mut g = scen()
        .battlefield(0, &["Mountain", "Island", "Island"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt", "Brain Freeze"])
        .hand(1, &[TIDEBINDER])
        .library(1, &["Forest"; 20])
        .start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    cast_targets(&mut g, b, &[player(1)]);
    let lib = g.player(1).library.len();
    let bfz = g.card(0, "Brain Freeze", Zone::Hand);
    g.choose(Choice::Cast(bfz));
    for _ in 0..20 {
        if at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = if g.has_option(player(1)) { player(1) } else { d.options[0].choice };
        g.choose(c);
    }
    assert_eq!(g.stack().len(), 2, "Brain Freeze and its storm trigger");
    g.pass();
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.pending_decision().unwrap().player == 1 => {
            // The Tidebinder's trigger: the storm trigger (the top ability).
            let st: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).filter(|c| matches!(c, Choice::Target(Target::Stack(_)))).collect();
            st.last().copied()
        }
        Some(DecisionKind::Target) | Some(DecisionKind::NewTarget) => g.has_option(player(1)).then_some(player(1)),
        _ => None,
    });
    assert_eq!(lib - g.player(1).library.len(), 3, "the original only\n{}", g.log_lines().join("\n"));
}

#[test]
fn lutris_copy_isnt_cast_and_resolves_before_the_original_cr_707_10() {
    // ruling: c574b96b
    // Saheeli makes a Servo whenever P0 casts a noncreature spell: one for
    // the Bolt, none for Lutri (a creature) or its copy (not cast). The copy
    // and the Bolt each deal 3.
    let mut g = scen()
        .battlefield(0, &["Saheeli, Sublime Artificer", "Mountain", "Island", "Island", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lutri, the Spellchaser"])
        .start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(at_priority);
    let l = g.card(0, "Lutri, the Spellchaser", Zone::Hand);
    g.choose(Choice::Cast(l));
    let copied = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) | Some(DecisionKind::NewTarget) => {
            if let Some(c) = named_option(g, "Lightning Bolt") {
                copied.set(true);
                return Some(c);
            }
            g.has_option(player(1)).then_some(player(1))
        }
        _ => None,
    });
    assert!(copied.get(), "{}", g.log_lines().join("\n"));
    assert_eq!(g.life(1), 14);
    assert_eq!(tokens_named(&g, "Servo").len(), 1, "{}", g.log_lines().join("\n"));
}

#[test]
fn chain_lightnings_copy_is_controlled_by_the_player_who_copies_it_cr_707_10() {
    // ruling: 9f05d6cd 532a71a4
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(0, &["Chain Lightning"])
        .start();
    let cl = g.card(0, "Chain Lightning", Zone::Hand);
    g.choose(Choice::Cast(cl));
    resolve_with(&mut g, |g| {
        let d = g.pending_decision().unwrap();
        match d.kind {
            DecisionKind::Target | DecisionKind::NewTarget => {
                // P0 aims at P1; P1's copy at P0.
                let p = if d.player == 0 { 1 } else { 0 };
                g.has_option(player(p)).then_some(player(p))
            }
            DecisionKind::YesNo if d.player == 1 => Some(Choice::Yes),
            _ => None,
        }
    });
    assert_eq!(g.life(1), 17);
    assert_eq!(g.life(0), 17, "P1's copy\n{}", g.log_lines().join("\n"));
}

#[test]
fn fatal_push_may_target_any_creature_and_checks_its_mana_value_on_resolution_cr_608_2b() {
    // ruling: adbe4a9a
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Polluted Delta"])
        .battlefield(1, &["Test Giant", "Test Trampler"])
        .hand(0, &["Fatal Push", "Fatal Push"])
        .start();
    let giant = bf(&g, 1, "Test Giant");
    let tr = bf(&g, 1, "Test Trampler");
    let fp = g.card(0, "Fatal Push", Zone::Hand);
    let t = g.tgt(giant);
    cast_targets(&mut g, fp, &[t]);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "mana value 5: nothing");
    // Revolt (the fetch land left): mana value 4 is destroyed.
    let pd = bf(&g, 0, "Polluted Delta");
    g.choose(act(&g, pd));
    resolve(&mut g);
    assert_ne!(g.obj(pd).zone, Zone::Battlefield);
    let fp2 = g.card(0, "Fatal Push", Zone::Hand);
    let t = g.tgt(tr);
    cast_targets(&mut g, fp2, &[t]);
    assert_eq!(g.obj(tr).zone, Zone::Graveyard, "{}", g.log_lines().join("\n"));
}

#[test]
fn pyroblast_may_target_a_nonblue_permanent_and_does_nothing_to_it_cr_608_2b() {
    // ruling: 4128e050
    let mut g = scen().battlefield(0, &["Mountain"]).battlefield(1, &["Test Bear"]).hand(0, &["Pyroblast"]).start();
    let bear = bf(&g, 1, "Test Bear");
    let p = g.card(0, "Pyroblast", Zone::Hand);
    g.choose(Choice::Cast(p));
    let offered = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => named_option(g, "estroy").or_else(|| Some(g.pending_decision().unwrap().options[0].choice)),
        Some(DecisionKind::Target) => {
            offered.set(g.has_option(g.tgt(bear)));
            Some(g.tgt(bear))
        }
        _ => None,
    });
    assert!(offered.get());
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.obj(p).zone, Zone::Graveyard);
}

#[test]
fn teferi_time_ravelers_restriction_beats_flash_cr_101_2() {
    // ruling: b3c067e2
    let mut g = scen()
        .battlefield(0, &["Teferi, Time Raveler"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &["Brazen Borrower // Petty Theft", "Test Flash"])
        .start();
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 1);
    assert_eq!(g.pending_decision().unwrap().options.len(), 1, "only passing: {}", opts(&g));
}

#[test]
fn ninjutsu_the_ninja_enters_only_as_the_ability_resolves_cr_702_49a() {
    // ruling: df42df1d 34862bd0
    let mut g = scen()
        .battlefield(0, &["Test Bear", "Island", "Island", "Swamp", "Swamp"])
        .hand(0, &["Fallen Shinobi"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let sh = g.card(0, "Fallen Shinobi", Zone::Hand);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::DeclareBlockers && at_priority(g));
    let nj = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Ability(x, _) if *x == sh)).expect("ninjutsu");
    g.choose(nj);
    for _ in 0..30 {
        if at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = if g.has_option(Choice::Card(bear)) { Choice::Card(bear) } else { d.options[0].choice };
        g.choose(c);
    }
    assert_eq!(g.obj(bear).zone, Zone::Hand, "returned as a cost");
    assert_eq!(g.obj(sh).zone, Zone::Hand, "not yet on the battlefield");
    assert_eq!(g.stack().len(), 1);
    resolve(&mut g);
    assert_eq!(g.obj(sh).zone, Zone::Battlefield);
    assert!(g.obj(sh).tapped && g.obj(sh).attacking);
}
